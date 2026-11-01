use serde_json::{Value, json};
use solidcraft_doc::MAX_PLACEMENT_DEPTH;

use super::*;
use crate::drawing_scene::tests::{front_and_top, nest};
use crate::schema::output_schema_for;
use crate::schema::tests::conforms;
use crate::tests::Folder;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap()
}

fn refused(s: &mut Session, p: Value) -> String {
    s.execute(CMD, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {p}"))
}

/// The SVG of sheet `sheet` of the drawing open, written straight from its built scene.
fn scene_svg(s: &Session, sheet: u64) -> Vec<u8> {
    let d = s.drawing("test").unwrap().clone();
    let built = crate::drawing_scene::build(s, &d, sheet).unwrap();
    solidcraft_io::paper::paper_svg(&built.scene.prepare().unwrap(), &NoPictures, DEFAULT_DPI).unwrap().0
}

/// A sheet exports as its scene's SVG, byte for byte, by id or by name: the reply holds it, with
/// its size and each view's status, as its schema says. Nothing changes and no model is evaluated.
#[test]
fn a_sheet_exports_as_its_scenes_svg() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let (doc, evaluations) = (s.doc.clone(), s.evaluations);
    let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}));
    let schema = output_schema_for::<ExportResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    let svg = scene_svg(&s, sheet);
    assert_eq!(reply["svg"].as_str().map(str::as_bytes), Some(svg.as_slice()));
    assert_eq!((&reply["format"], reply["bytes"].as_u64(), reply.get("path")), (&json!("svg"), Some(svg.len() as u64), None), "{reply}");
    // Each view on the sheet, in id order, as drawing.scene lists them.
    let views: Vec<_> = reply["views"].as_array().unwrap().iter().map(|v| (v["view"].clone(), v["status"].clone())).collect();
    let mut in_order = [front, top];
    in_order.sort_unstable();
    assert_eq!(views, in_order.map(|v| (json!(ids::encode(v)), json!("drawn"))));
    let name = s.drawing("test").unwrap().sheets[0].name.clone();
    assert_eq!(run(&mut s, CMD, json!({"sheet": name, "format": "svg"}))["svg"], reply["svg"]);
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc) && s.evaluations == evaluations);
    // The sheet named, not the first: a second sheet, with no views, exports as its own scene.
    let added = run(&mut s, "drawing.sheet.add", json!({}));
    let second = ids::decode(added["sheet"].as_str().unwrap()).unwrap();
    let other = run(&mut s, CMD, json!({"sheet": ids::encode(second)}));
    assert_eq!((&other["sheet"], other["views"].as_array().map(Vec::len)), (&json!(ids::encode(second)), Some(0)), "{other}");
    assert_eq!(other["svg"].as_str().map(str::as_bytes), Some(scene_svg(&s, second).as_slice()));
    assert_ne!(other["svg"], reply["svg"]);
}

/// With a path the file holds the same SVG and the reply names it instead.
#[test]
fn a_path_gets_the_svg_written_to_it() {
    let (mut s, sheet, _) = front_and_top();
    let folder = Folder::new("drawing-export");
    let path = folder.path("Sheet 1.SVG");
    let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet), "path": path}));
    let written = std::fs::read(&path).unwrap();
    assert_eq!(written, scene_svg(&s, sheet));
    assert_eq!(
        (reply["path"].as_str(), reply.get("svg"), reply["bytes"].as_u64()),
        (Some(path.as_str()), None, Some(written.len() as u64)),
        "{reply}"
    );
}

/// A view that failed stops the export, and the refusal names it with why; a placeholder draws
/// nothing and is exported all the same.
#[test]
fn a_failed_view_is_named_and_nothing_is_written() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let folder = Folder::new("drawing-export-failed");
    let path = folder.path("sheet.svg");
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"display_style": "shaded"}}));
    let why = refused(&mut s, json!({"sheet": ids::encode(sheet), "path": path}));
    assert!(why.contains(&format!("views on the sheet failed: {} (not supported yet: shaded views", ids::encode(top))), "{why}");
    assert!(!why.contains(&ids::encode(front)) && !std::path::Path::new(&path).exists(), "{why}");
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"display_style": "visible"}}));
    for level in 0..=MAX_PLACEMENT_DEPTH as u64 + 1 {
        nest(&mut s, level);
    }
    let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}));
    let statuses: Vec<_> = reply["views"].as_array().unwrap().iter().map(|v| v["status"].clone()).collect();
    assert_eq!(statuses, [json!("placeholder"), json!("placeholder")], "{reply}");
    assert_eq!(reply["svg"].as_str().map(str::as_bytes), Some(scene_svg(&s, sheet).as_slice()));
}

/// PNG and DXF aren't supported yet; a path that isn't a `.svg` file, an unknown sheet, an unknown
/// key and no drawing open are refused, and nothing is written.
#[test]
fn what_it_cant_write_is_refused() {
    let (mut s, sheet, _) = front_and_top();
    let folder = Folder::new("drawing-export-refused");
    let id = ids::encode(sheet);
    let long = format!("{}.svg", "a".repeat(MAX_PATH_BYTES));
    for (p, why) in [
        (json!({"sheet": id, "format": "png"}), "not supported yet: PNG"),
        (json!({"sheet": id, "format": "dxf"}), "not supported yet: DXF"),
        (json!({"sheet": id, "format": "pdf"}), "format"),
        (json!({"sheet": id, "path": folder.path("sheet.png")}), "path must be a .svg file"),
        (json!({"sheet": id, "path": folder.path("sheet")}), "path must be a .svg file"),
        (json!({"sheet": id, "path": " "}), "path must be a .svg file"),
        (json!({"sheet": id, "path": long}), "path must be a .svg file"),
        (json!({"sheet": id, "path": null}), "path"),
        (json!({"sheet": "Sheet 9"}), "no sheet `Sheet 9`"),
        (json!({"sheet": 5}), "sheet"),
        (json!({}), "sheet"),
        (json!({"sheet": id, "dpi": 300}), "dpi"),
    ] {
        let message = refused(&mut s, p.clone());
        assert!(message.contains(why), "{p}: {message}");
    }
    assert_eq!(std::fs::read_dir(&folder.0).unwrap().count(), 0);
    run(&mut s, "drawing.close", json!({}));
    assert!(refused(&mut s, json!({"sheet": id})).contains("open a drawing first"));
}

/// The drawing milestone's plate (acceptance case 84, #4894) by commands alone: modelled from its
/// parameters, a top view at 1:2 and the views projected below and right of it, its length, width
/// and first bore dimensioned, with each dimension's id, value and text.
fn milestone_plate() -> (Session, Vec<(Value, Value, Value)>) {
    let mut s = Session::default();
    let script: Value = serde_json::from_str(include_str!("../../../../examples/acceptance/drawing_m2.json")).unwrap();
    let made = s.run_script(&script).unwrap();
    let size = |reply: &Value| {
        let b = &reply["boundary_mm"];
        [0, 1].map(|i| b["max"][i].as_f64().unwrap() - b["min"][i].as_f64().unwrap())
    };
    let top = made.last().unwrap();
    let below = run(&mut s, "drawing.view.projected", json!({"parent": top["view"], "side": "below"}));
    let right = run(&mut s, "drawing.view.projected", json!({"parent": top["view"], "side": "right"}));
    // 100 x 80 x 20 at half size: the top shows length by width, below it length by thickness,
    // right of it thickness by width.
    for (view, expected) in [(top, [50.0, 40.0]), (&below, [50.0, 10.0]), (&right, [10.0, 40.0])] {
        let [w, h] = size(view);
        assert!((w - expected[0]).abs() < 1e-6 && (h - expected[1]).abs() < 1e-6, "{view}");
    }
    // The first bore's rim, by a point on it.
    let mut dimensions = Vec::new();
    for (kind, value) in [
        (json!({"length": {"edge": [50, 0, 20]}}), 100.0),
        (json!({"length": {"edge": [0, 40, 20]}}), 80.0),
        (json!({"radial": {"edge": [25, 20, 20]}}), 10.0),
    ] {
        let added = run(&mut s, "drawing.dimension.add", json!({"view": top["view"], "kind": kind}));
        assert!((added["value"].as_f64().unwrap() - value).abs() < 1e-9, "{added}");
        dimensions.push((added["dimension"].clone(), added["value"].clone(), added["text"].clone()));
    }
    (s, dimensions)
}

/// Saved with the design and opened again, the plate's sheet exports the same SVG, byte for byte,
/// and each dimension reads the same.
#[test]
fn the_milestone_plate_exports_the_same_after_it_is_saved_and_opened() {
    let (mut s, dimensions) = milestone_plate();
    let svg = run(&mut s, CMD, json!({"sheet": "Sheet 1"}))["svg"].clone();
    let drawing = ids::encode(s.drawing("test").unwrap().id);
    let folder = Folder::new("drawing-m2-plate");
    run(&mut s, "file.save", json!({"path": folder.path("plate.solidcraft")}));
    let mut opened = Session::default();
    run(&mut opened, "doc.open", json!({"path": folder.path("plate.solidcraft")}));
    run(&mut opened, "drawing.open", json!({"drawing": drawing}));
    assert_eq!(run(&mut opened, CMD, json!({"sheet": "Sheet 1"}))["svg"], svg);
    for (dimension, value, text) in dimensions {
        let read = run(&mut opened, "drawing.dimension.inspect", json!({"dimension": dimension}));
        assert_eq!((&read["value"], &read["text"], &read["status"]), (&value, &text, &json!("ok")), "{read}");
    }
}

/// What each view on the open drawing's first sheet fills, in sheet mm, sorted.
fn view_sizes(s: &Session) -> Vec<[f64; 2]> {
    let d = s.drawing("test").unwrap().clone();
    let scene = crate::drawing_scene::build(s, &d, d.sheets[0].id).unwrap().scene;
    let views = scene.groups.iter().filter(|group| group.kind == solidcraft_geom::paper::ObjectKind::View);
    let mut sizes: Vec<[f64; 2]> = views
        .map(|view| {
            let paths =
                view.items.iter().filter_map(|item| if let solidcraft_geom::paper::Item::Stroke { path, .. } = item { Some(path) } else { None });
            let points: Vec<solidcraft_geom::Vec2> = paths.flatten().flat_map(|seg| (0..=16).map(|i| seg.point_at(f64::from(i) / 16.0))).collect();
            let extent = |along: fn(&solidcraft_geom::Vec2) -> f64| {
                points.iter().map(along).fold(f64::NEG_INFINITY, f64::max) - points.iter().map(along).fold(f64::INFINITY, f64::min)
            };
            [extent(|p| p.x), extent(|p| p.y)]
        })
        .collect();
    sizes.sort_by(|a, b| a.partial_cmp(b).unwrap());
    sizes
}

/// The plate's model edited with its drawing closed (acceptance cases 85 to 87, #4894): the views
/// and the model follow with the hand values, and each edit saved and opened again exports the same
/// SVG. Undo brings back the sheet before it byte for byte, and redo the one after it. What the
/// dimensions read across these edits is E2E-01 to E2E-04, on #8288.
#[test]
fn the_milestone_plate_follows_its_model_and_undo_brings_its_sheet_back() {
    let (mut s, _) = milestone_plate();
    let drawing = ids::encode(s.drawing("test").unwrap().id);
    let on_the_model = |s: &mut Session, id: &str, p: Value| {
        run(s, "drawing.close", json!({}));
        run(s, id, p);
        run(s, "drawing.open", json!({"drawing": drawing}));
    };
    let sheet = |s: &mut Session| run(s, CMD, json!({"sheet": "Sheet 1"}))["svg"].clone();
    let folder = Folder::new("drawing-m2-edits");
    let before = sheet(&mut s);
    let wide = [[10.0, 50.0], [50.0, 10.0], [50.0, 50.0]];
    let plate = [[10.0, 40.0], [50.0, 10.0], [50.0, 40.0]];
    let bores = |one: f64, two: f64| 100.0 * 80.0 * 20.0 - std::f64::consts::PI * (one * one + two * two) / 4.0 * 20.0;
    for (case, id, edit, sizes, volume) in [
        (85, "parameters.change", json!({"name": "width", "expression": "100 mm"}), wide, 196_858.41),
        (86, "parameters.change", json!({"name": "bore1_d", "expression": "12 mm"}), plate, bores(12.0, 10.0)),
        (87, "timeline.delete", json!({"feature": "Bore1"}), plate, bores(0.0, 10.0)),
    ] {
        on_the_model(&mut s, id, edit);
        for (size, expected) in view_sizes(&s).iter().zip(sizes) {
            assert!((size[0] - expected[0]).abs() < 1e-6 && (size[1] - expected[1]).abs() < 1e-6, "case {case}: {size:?}");
        }
        run(&mut s, "drawing.close", json!({}));
        let measured = run(&mut s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
        run(&mut s, "drawing.open", json!({"drawing": drawing}));
        // Measured over facets, so a bore comes a little short of the hand's circle.
        assert!((measured - volume).abs() < 1.0, "case {case}: {measured}");
        // The sheet the edit gives, and the same from the file.
        let after = sheet(&mut s);
        assert_ne!(after, before, "case {case}");
        let path = folder.path(&format!("case-{case}.solidcraft"));
        run(&mut s, "file.save", json!({"path": path}));
        let mut opened = Session::default();
        run(&mut opened, "doc.open", json!({"path": path}));
        run(&mut opened, "drawing.open", json!({"drawing": drawing}));
        assert_eq!(sheet(&mut opened), after, "case {case}: opened again");
        on_the_model(&mut s, "edit.undo", json!({}));
        assert_eq!(sheet(&mut s), before, "case {case}: undone");
        on_the_model(&mut s, "edit.redo", json!({}));
        assert_eq!(sheet(&mut s), after, "case {case}: redone");
        on_the_model(&mut s, "edit.undo", json!({}));
    }
}
