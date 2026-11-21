//! Tab on a sheet made from surfaces (#8325, #6748): a closed sketch on a flat panel's face adds
//! its region to that panel, through the thickness. These are the cases of the flat-first Tab
//! (#8326), the spec for the rebuild.

use serde_json::{Value, json};
use solidcraft_geom::prelude::*;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

fn flat(s: &mut Session) -> Value {
    run(s, "sheet.flat_pattern", json!({}))
}

fn bbox(s: &mut Session) -> Value {
    run(s, "inspect.measure", json!({}))["bodies"][0]["bbox"].clone()
}

fn contains(s: &Session, p: [f64; 3]) -> bool {
    s.model
        .state()
        .bodies
        .first()
        .is_some_and(|b| b.body.classify_point(solidcore_record::Vec3::new(p[0], p[1], p[2])).ok() == Some(solidcraft_kernel::PointClass::Inside))
}

/// A rectangle sketch named `name` on the face at `face`, from the corners `a` to `b` (on that
/// face, the body's coordinates).
fn face_rectangle(s: &mut Session, name: &str, face: [f64; 3], a: [f64; 3], b: [f64; 3]) {
    run(s, "sketch.create", json!({"plane": {"face": face}, "name": name}));
    let plane = s.model.state().sketches.last().map(|ss| ss.plane).unwrap_or_else(|| panic!("no sketch"));
    let local = |p: [f64; 3]| plane.to_local(solidcore_record::Vec3::new(p[0], p[1], p[2]));
    let (p0, p1) = (local(a), local(b));
    run(s, "sketch.rectangle.two_point", json!({"p0": [p0.x, p0.y], "p1": [p1.x, p1.y]}));
    run(s, "sketch.finish", json!({}));
}

/// The issue's example: a 2 mm rule, a 100 x 60 base on XY (its top face z = 2), and a 40 x 40
/// profile on its top face over the y = 60 edge by 10 mm (sketch `Ear`).
fn tab_sketch(s: &mut Session) {
    run(s, "sheet.manage_rules", json!({"name": "S2", "thickness": "2 mm", "bend_radius": "2 mm", "k_factor": "0.4", "active": true}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
    run(s, "sketch.finish", json!({}));
    run(s, "sheet.flange", json!({"sketch": "Base"}));
    face_rectangle(s, "Ear", [50.0, 30.0, 2.0], [30.0, 50.0, 2.0], [70.0, 90.0, 2.0]);
}

fn refused(s: &mut Session, p: Value, want: &str) {
    let before = s.doc.to_json();
    let e = s.execute("sheet.tab", &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("{p}: not refused"));
    assert!(e.contains(want), "{p}: {e}");
    assert_eq!(s.doc.to_json(), before, "{p}: a refused tab leaves the design as it was");
}

#[test]
fn a_tab_joins_the_sheet_and_unfolds_with_it() {
    let mut s = Session::default();
    tab_sketch(&mut s);
    // Flange on that sketch still makes a sheet of its own.
    let mut flange = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap_or_else(|e| panic!("{e}")));
    flange.refresh();
    assert_eq!(run(&mut flange, "sheet.flange", json!({"sketch": "Ear"}))["created_bodies"], json!(["Body2"]));
    // Tab: the 40 x 30 past the edge joins the same body.
    let r = run(&mut s, "sheet.tab", json!({"sketch": "Ear"}));
    assert_eq!((r["created_bodies"].clone(), r["changed_bodies"].clone()), (json!([]), json!(["Body1"])));
    assert_eq!(s.model.state().bodies.len(), 1);
    assert!((volume(&mut s) - 7200.0 * 2.0).abs() < 1e-6, "{}", volume(&mut s));
    let f = flat(&mut s);
    assert_eq!(f["flat_size_mm"], json!([100.0, 90.0, 2.0]));
    assert_eq!(f["outline"].as_array().map(Vec::len), Some(1), "one piece");
    assert_eq!(f["outline"][0].as_array().map(Vec::len), Some(8), "{}", f["outline"]);
    let dxf = run(&mut s, "sheet.export_dxf", json!({}))["dxf"].as_str().unwrap_or_default().to_string();
    assert_eq!(solidcraft_io::read_dxf(&dxf).map(|g| g.len()).ok(), Some(8));
    // The tab's far edge takes a flange; the sheet still unfolds in one piece.
    run(&mut s, "sheet.flange", json!({"edges": [[50, 90, 2]], "height": 15}));
    assert_eq!(s.model.state().bodies.len(), 1);
    assert_eq!(flat(&mut s)["outline"].as_array().map(Vec::len), Some(1));
    let with_flange = volume(&mut s);
    // Undo, redo and reopening keep it.
    run(&mut s, "edit.undo", json!({}));
    assert!((volume(&mut s) - 14400.0).abs() < 1e-6);
    run(&mut s, "edit.redo", json!({}));
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap_or_else(|e| panic!("{e}")));
    reopened.refresh();
    assert!(reopened.model.results.iter().all(|r| r.error.is_none()), "{:?}", reopened.model.results);
    assert!((volume(&mut reopened) - with_flange).abs() < 1e-6);
}

#[test]
fn a_tab_on_a_folded_wall_and_tabs_that_are_refused() {
    let mut s = Session::default();
    tab_sketch(&mut s);
    // The base sketch lies on the bottom face: its profile is the base already.
    refused(&mut s, json!({"sketch": "Base"}), solidcraft_doc::sheet::TAB_ADDS_NOTHING);
    // A 20 mm wall up from the y = 0 edge.
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, 2]], "height": 20}));
    let before = volume(&mut s);
    let b = bbox(&mut s);
    let (y0, top) = (b["min"][1].as_f64().unwrap_or(f64::NAN), b["max"][2].as_f64().unwrap_or(f64::NAN));
    // A 20 x 10 lug on the wall's outer face over its top edge.
    face_rectangle(&mut s, "Lug", [50.0, y0, top - 8.0], [40.0, y0, top - 5.0], [60.0, y0, top + 10.0]);
    let r = run(&mut s, "sheet.tab", json!({"sketch": "Lug"}));
    assert_eq!(r["changed_bodies"], json!(["Body1"]));
    assert_eq!(s.model.state().bodies.len(), 1);
    assert!((volume(&mut s) - before - 200.0 * 2.0).abs() < 1e-3, "{}", volume(&mut s) - before);
    assert_eq!(flat(&mut s)["outline"].as_array().map(Vec::len), Some(1));
    // Folded, the lug stands above the wall.
    assert!(contains(&s, [50.0, y0 + 1.0, top + 9.0]));
    // Refused: apart from the face's outline, into a bend, off the sheet, adding nothing.
    face_rectangle(&mut s, "Apart", [50.0, 30.0, 2.0], [30.0, 70.0, 2.0], [70.0, 90.0, 2.0]);
    refused(&mut s, json!({"sketch": "Apart"}), solidcraft_doc::sheet::TAB_APART);
    face_rectangle(&mut s, "Bend", [50.0, 30.0, 2.0], [30.0, -2.0, 2.0], [70.0, 10.0, 2.0]);
    refused(&mut s, json!({"sketch": "Bend"}), solidcraft_doc::sheet::TAB_IN_BEND);
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 10], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Above"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [30, 50], "p1": [70, 90]}));
    run(&mut s, "sketch.finish", json!({}));
    refused(&mut s, json!({"sketch": "Above"}), solidcraft_doc::sheet::TAB_OFF_SHEET);
    refused(&mut s, json!({"sketch": "Apart", "body": "Nope"}), "not a sheet metal body");
    run(&mut s, "sheet.unfold", json!({}));
    refused(&mut s, json!({"sketch": "Ear"}), "refold the sheet");
}

/// Which panel a tab joins is never a matter of the panels' or sheets' order (#6748): two plates
/// side by side in one plane, 10 mm apart, and a profile over the gap touching both is refused,
/// with the design as it was; one touching only the second joins the second.
#[test]
fn a_tab_two_sheets_would_take_is_refused() {
    let mut s = Session::default();
    run(&mut s, "sheet.manage_rules", json!({"name": "S2", "thickness": "2 mm", "bend_radius": "2 mm", "k_factor": "0.4", "active": true}));
    for (name, x0) in [("A", 0.0), ("B", 110.0)] {
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": name}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [x0, 0.0], "p1": [x0 + 100.0, 60.0]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sheet.flange", json!({"sketch": name}));
    }
    assert_eq!(s.model.state().bodies.len(), 2);
    let before = volume(&mut s);
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 2], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Both"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [95, 20], "p1": [115, 40]}));
    run(&mut s, "sketch.finish", json!({}));
    refused(&mut s, json!({"sketch": "Both"}), solidcraft_doc::sheet::TAB_SEVERAL);
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 2], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "One"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [200, 20], "p1": [230, 40]}));
    run(&mut s, "sketch.finish", json!({}));
    let r = run(&mut s, "sheet.tab", json!({"sketch": "One"}));
    assert_eq!(r["changed_bodies"], json!(["Body2"]));
    assert!((volume(&mut s) - before - 20.0 * 20.0 * 2.0).abs() < 1e-6, "{}", volume(&mut s) - before);
}

#[test]
fn hostile_tab_params_never_panic() {
    let mut s = Session::default();
    tab_sketch(&mut s);
    for p in [
        json!({"sketch": null}),
        json!({"sketch": 1e308}),
        json!({"sketch": -1}),
        json!({"sketch": [1, 2]}),
        json!({"sketch": "Ear", "profiles": "bogus"}),
        json!({"sketch": "Ear", "profiles": [99]}),
        json!({"sketch": "Ear", "profiles": [{"point": [1e308, -1e308]}]}),
        json!({"sketch": "Ear", "body": 5}),
        json!({"sketch": "Ear", "body": ""}),
        json!({"sketch": "Ear", "name": "x".repeat(10_000)}),
        json!({"sketch": "Ear", "participants": ["*"]}),
        json!([1, 2, 3]),
        json!("tab"),
    ] {
        let _ = s.execute("sheet.tab", &p);
    }
}
