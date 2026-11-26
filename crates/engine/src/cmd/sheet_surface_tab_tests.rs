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
        // A refusal is fine; a panic the session caught (an internal error) is not.
        if let Err(crate::EngineError::Internal(_, msg)) = s.execute("sheet.tab", &p) {
            panic!("sheet.tab {p} panicked: {msg}");
        }
    }
}

/// Issue #22259's part: the 2 mm rule (R 2, K 0.4), a 100 x 60 base, a 90 degree flange 20 high
/// on its y = 60 edge, and, where `cut`, a box cut [40, 55, -1] 20 x 6 x 7 that takes the whole
/// bend over x 40..60 with the base's last millimetre and the flange's first 2 mm. Volume:
/// 11200 + 100 x 3 pi + 3200 = 15342.478, less 40 + 60 pi + 80 for the cut = 15033.982.
fn gap_part(s: &mut Session, cut: bool) {
    run(s, "sheet.manage_rules", json!({"name": "S2", "thickness": "2 mm", "bend_radius": "2 mm", "k_factor": "0.4", "active": true}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
    run(s, "sketch.finish", json!({}));
    run(s, "sheet.flange", json!({"sketch": "Base"}));
    run(s, "sheet.flange", json!({"edges": [[50, 60, 2]], "height": 20, "angle": "90 deg"}));
    if cut {
        run(s, "solid.box", json!({"corner": [40, 55, -1], "length": 20, "width": 6, "height": 7, "operation": "cut"}));
    }
}

/// A rectangle sketch `name` on the base's top face (z = 2), from `a` to `b`.
fn top_rectangle(s: &mut Session, name: &str, a: [f64; 2], b: [f64; 2]) {
    run(s, "sketch.create", json!({"plane": {"origin": [0, 0, 2], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": name}));
    run(s, "sketch.rectangle.two_point", json!({"p0": a, "p1": b}));
    run(s, "sketch.finish", json!({}));
}

/// A tab in the gap where a cut took the bend away (#22259) fills what the cut took there, as a
/// tab fills a hole cut inside its panel: what it gives back of the bend rides flat with the
/// base. The flange's inner face is y = 58, the bend's tangent line on the base y = 56, and its
/// allowance pi / 2 x (2 + 0.4 x 2) = 4.398, so the flat is 56 + 4.398 + 16 = 76.398 long.
#[test]
fn a_tab_fills_the_gap_a_cut_took_out_of_a_bend() {
    let mut s = Session::default();
    gap_part(&mut s, true);
    let cut = volume(&mut s);
    assert!((cut - 15033.982).abs() < 1e-3, "{cut}");
    // To the flange's inner face and beyond it, short of where the bend's far side began:
    // 10 x 1 of the base and 10 x 3 of the bend, flat, through 2 mm.
    top_rectangle(&mut s, "Gap", [45.0, 50.0], [55.0, 59.0]);
    let r = run(&mut s, "sheet.tab", json!({"sketch": "Gap"}));
    assert_eq!(r["changed_bodies"], json!(["Body1"]));
    assert_eq!(s.model.state().bodies.len(), 1);
    let with = volume(&mut s);
    assert!((with - cut - 80.0).abs() < 1e-3, "{}", with - cut);
    let f = flat(&mut s);
    assert_eq!(f["outline"].as_array().map(Vec::len), Some(1), "one piece: {}", f["outline"]);
    let size = f["flat_size_mm"].as_array().cloned().unwrap_or_default();
    let size: Vec<f64> = size.iter().filter_map(Value::as_f64).collect();
    assert!(size.len() == 3 && (size[0] - 100.0).abs() < 1e-6 && (size[1] - 76.398).abs() < 1e-3, "{size:?}");
    // Folded, it lies flat with the base: in the wall's thickness, not standing up with it.
    assert!(contains(&s, [50.0, 58.5, 1.0]));
    assert!(!contains(&s, [50.0, 59.5, 1.0]));
    // Undo, redo and reopening keep it.
    run(&mut s, "edit.undo", json!({}));
    assert!((volume(&mut s) - cut).abs() < 1e-6);
    run(&mut s, "edit.redo", json!({}));
    assert!((volume(&mut s) - with).abs() < 1e-6);
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap_or_else(|e| panic!("{e}")));
    reopened.refresh();
    assert!(reopened.model.results.iter().all(|r| r.error.is_none()), "{:?}", reopened.model.results);
    assert!((volume(&mut reopened) - with).abs() < 1e-6);

    // Clear of the bend that is left, up to the flange's inner face: 16 x 1 and 16 x 2.
    let mut s = Session::default();
    gap_part(&mut s, true);
    top_rectangle(&mut s, "Short", [42.0, 55.0], [58.0, 58.0]);
    run(&mut s, "sheet.tab", json!({"sketch": "Short"}));
    assert!((volume(&mut s) - cut - 96.0).abs() < 1e-3, "{}", volume(&mut s) - cut);
}

/// What a tab in that gap cannot fill yet is refused, the design as it was (#22259): past the
/// bend's far side, where the flat would join the flange; touching the bend that is left, which
/// would bend with it; on into the flange; and apart from the base. With no cut, the bend is
/// whole, and the same tab reaches into it.
#[test]
fn tabs_a_cut_bend_cannot_take_are_refused() {
    let mut s = Session::default();
    gap_part(&mut s, true);
    for (name, a, b, want) in [
        ("Far", [45.0, 55.0], [55.0, 62.0], solidcraft_doc::sheet::TAB_PAST_BEND),
        ("Wide", [40.0, 55.0], [60.0, 57.5], solidcraft_doc::sheet::TAB_IN_BEND),
        ("Long", [45.0, 50.0], [55.0, 70.0], solidcraft_doc::sheet::TAB_PAST_BEND),
        ("Apart", [45.0, 57.0], [55.0, 58.0], solidcraft_doc::sheet::TAB_APART),
    ] {
        top_rectangle(&mut s, name, a, b);
        refused(&mut s, json!({"sketch": name}), want);
    }
    let mut s = Session::default();
    gap_part(&mut s, false);
    top_rectangle(&mut s, "Gap", [45.0, 50.0], [55.0, 59.0]);
    refused(&mut s, json!({"sketch": "Gap"}), solidcraft_doc::sheet::TAB_IN_BEND);
}

/// Where the bend of [`gap_part`]'s flange ends on the flange's side in the flat: its tangent line
/// on the base, y = 56, and its allowance, pi / 2 x (2 + 0.4 x 2).
const GAP_FAR: f64 = 56.0 + std::f64::consts::FRAC_PI_2 * 2.8;

/// A cut made in the flat: the sheet unfolded, a box through it from `a` to `b` (the flat's
/// coordinates, the base's own), and refolded.
fn flat_cut(s: &mut Session, a: [f64; 2], b: [f64; 2]) {
    run(s, "sheet.unfold", json!({}));
    run(s, "solid.box", json!({"corner": [a[0], a[1], -1], "length": b[0] - a[0], "width": b[1] - a[1], "height": 4, "operation": "cut"}));
    run(s, "sheet.refold", json!({}));
}

/// A closed polygon sketch `name` on the base's top face (z = 2), through `points`.
fn top_polygon(s: &mut Session, name: &str, points: &[[f64; 2]]) {
    run(s, "sketch.create", json!({"plane": {"origin": [0, 0, 2], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": name}));
    let mut closed = points.to_vec();
    closed.extend(points.first().copied());
    run(s, "sketch.line", json!({"points": closed}));
    run(s, "sketch.finish", json!({}));
}

/// A gap tab may end past the flange's outer face (y = 60), where the base's own trim ends, as
/// long as it ends short of the bend's far side (#22259): what it gives back there is the bend's,
/// riding flat with the base, not the base's outline as well. 10 x 1 of the base and 10 x 4.2 of
/// the bend, through 2 mm: +104.
#[test]
fn a_gap_tab_may_end_past_the_flanges_outer_face() {
    let mut s = Session::default();
    gap_part(&mut s, true);
    let cut = volume(&mut s);
    top_rectangle(&mut s, "Gap", [45.0, 50.0], [55.0, 60.2]);
    run(&mut s, "sheet.tab", json!({"sketch": "Gap"}));
    assert!((volume(&mut s) - cut - 104.0).abs() < 1e-3, "{}", volume(&mut s) - cut);
    assert_eq!(flat(&mut s)["outline"].as_array().map(Vec::len), Some(1));
    // Flat with the base, 0.1 mm past the flange's outer face, under where the cut stopped it.
    assert!(contains(&s, [50.0, 60.1, 1.0]));
    assert!(!contains(&s, [50.0, 60.1, 3.0]));
}

/// What a tab leaves of a cut it fills stays cut, however small (#22259): here a 0.05 x 0.1
/// corner of the cut that the tab's outline goes round. The tab gives back 0.05 x 0.4 and
/// 4.95 x 0.5 of the base, and 10 x 4 of base and bend: 42.495 mm2, through 2 mm.
#[test]
fn a_tab_leaves_what_it_does_not_cover_of_a_cut_however_small() {
    let mut s = Session::default();
    gap_part(&mut s, true);
    let cut = volume(&mut s);
    let lobe = [[40.0, 55.1], [40.05, 55.1], [40.05, 50.0], [55.0, 50.0], [55.0, 59.0], [45.0, 59.0], [45.0, 55.5], [40.0, 55.5]];
    top_polygon(&mut s, "Round", &lobe);
    run(&mut s, "sheet.tab", json!({"sketch": "Round"}));
    assert!((volume(&mut s) - cut - 84.99).abs() < 1e-3, "{}", volume(&mut s) - cut);
    assert!(!contains(&s, [40.025, 55.05, 1.0]));
    assert!(contains(&s, [40.025, 55.3, 1.0]));
}

/// A gap tab that ends on the bend's far side, or that runs past it where the cut stopped there,
/// is refused as running past the bend (#22259), not as reaching into it.
#[test]
fn a_gap_tab_to_the_bends_far_side_is_refused_as_past_it() {
    let mut s = Session::default();
    gap_part(&mut s, true);
    top_rectangle(&mut s, "OnFar", [45.0, 50.0], [55.0, GAP_FAR]);
    refused(&mut s, json!({"sketch": "OnFar"}), solidcraft_doc::sheet::TAB_PAST_BEND);
    // A cut in the flat that takes the bend over x 40..60 and stops at its far side.
    let mut s = Session::default();
    gap_part(&mut s, false);
    flat_cut(&mut s, [40.0, 55.0], [60.0, GAP_FAR]);
    top_rectangle(&mut s, "Into", [45.0, 50.0], [55.0, 65.0]);
    refused(&mut s, json!({"sketch": "Into"}), solidcraft_doc::sheet::TAB_PAST_BEND);
}

/// A tab over a cut across the bend at its end (#22259). Stopping short of the bend that is left,
/// it fills the cut under it: past the base's side it adds 10 x 7.5, less the bend relief that
/// opens there (the rule's: 2 wide, from 1 inside the tangent line y = 56), 75 - 2 x 2.5 = 70
/// mm2, and 10 x 2.5 of the cut, through 2 mm. Running on to the bend that is left (x = 20), what
/// it would give back joins that bend and would bend with it: refused, the design as it was.
/// Before tabs filled such cuts, that tab built (+140) with the part of it inside the base's
/// outline missing, without saying so.
#[test]
fn a_tab_over_a_cut_at_a_bends_end_fills_it_or_is_refused() {
    let mut s = Session::default();
    gap_part(&mut s, false);
    flat_cut(&mut s, [-1.0, 55.0], [20.0, GAP_FAR + 2.0]);
    let cut = volume(&mut s);
    top_rectangle(&mut s, "Side", [-10.0, 50.0], [10.0, 57.5]);
    run(&mut s, "sheet.tab", json!({"sketch": "Side"}));
    assert!((volume(&mut s) - cut - 2.0 * (70.0 + 25.0)).abs() < 1e-3, "{}", volume(&mut s) - cut);
    assert!(contains(&s, [-5.0, 52.0, 1.0]));
    assert!(contains(&s, [5.0, 55.5, 1.0]));

    let mut s = Session::default();
    gap_part(&mut s, false);
    flat_cut(&mut s, [-1.0, 55.0], [20.0, GAP_FAR + 2.0]);
    top_rectangle(&mut s, "Long", [-10.0, 50.0], [20.0, 57.5]);
    refused(&mut s, json!({"sketch": "Long"}), solidcraft_doc::sheet::TAB_IN_BEND);
}

/// A tab in the gap that touches the bend that is left is refused (#22259), also when it runs
/// off the base as well, where it adds material whatever the cut does: it is not built with its
/// part in the gap missing. The design is as it was.
#[test]
fn a_gap_tab_that_cannot_fill_the_cut_is_refused_though_it_runs_off_the_base() {
    let mut s = Session::default();
    gap_part(&mut s, true);
    top_rectangle(&mut s, "Down", [45.0, -10.0], [60.0, 57.5]);
    refused(&mut s, json!({"sketch": "Down"}), solidcraft_doc::sheet::TAB_IN_BEND);
}

/// A gap tab that ends on the bend's far side is refused as running past the bend (#22259), also
/// where the cut and the tab run on past the bend's end (x = 0): the cut keeps only what it takes
/// of the sheet, so what the tab gives back runs along the far side from the bend's end. The same
/// tab from the base's side is refused the same way.
#[test]
fn a_gap_tab_to_the_far_side_past_the_bends_end_is_refused_as_past_it() {
    for (name, from) in [("Side", 0.0), ("Past", -10.0)] {
        let mut s = Session::default();
        gap_part(&mut s, false);
        flat_cut(&mut s, [-1.0, 55.0], [20.0, GAP_FAR]);
        top_rectangle(&mut s, name, [from, 50.0], [10.0, GAP_FAR]);
        refused(&mut s, json!({"sketch": name}), solidcraft_doc::sheet::TAB_PAST_BEND);
    }
}
