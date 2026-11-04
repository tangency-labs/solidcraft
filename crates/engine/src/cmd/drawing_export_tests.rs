use serde_json::{Value, json};
use solidcraft_doc::MAX_PLACEMENT_DEPTH;
use solidcraft_geom::paper::NoPictures;

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

fn sheet_sketch(s: &mut Session, sheet: u64, commands: &[(&str, Value)]) -> u64 {
    let made = run(s, "drawing.sketch.create", json!({"owner": ids::encode(sheet)}));
    for (command, params) in commands {
        run(s, command, params.clone());
    }
    run(s, "sketch.finish", json!({}));
    ids::decode(made["sketch"].as_str().unwrap()).unwrap()
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
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"display_style": "shaded_visible"}}));
    let why = refused(&mut s, json!({"sheet": ids::encode(sheet), "path": path}));
    assert!(why.contains(&format!("views on the sheet failed: {} (not supported yet: shaded views with lines", ids::encode(top))), "{why}");
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

/// A sheet with shaded views exports their pictures as `<image>`s, the same bytes twice. A picture
/// past the grid's limit is refused and nothing is written.
#[test]
fn a_shaded_view_exports_its_picture() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let plain = run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}))["svg"].as_str().unwrap().to_owned();
    assert!(!plain.contains("<image"), "lines only: {}", plain.len());
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"display_style": "shaded"}}));
    let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}));
    let svg = reply["svg"].as_str().unwrap();
    assert_eq!(svg.matches("<image").count(), 2, "the front view's, and the top view's, which follows it: {reply}");
    assert_eq!(run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}))["svg"], reply["svg"], "the same bytes twice");
    let folder = Folder::new("drawing-export-picture");
    let path = folder.path("sheet.svg");
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"scale": "100:1"}}));
    let why = refused(&mut s, json!({"sheet": ids::encode(sheet), "path": path}));
    assert!(why.contains("over the limit"), "{why}");
    assert!(!std::path::Path::new(&path).exists(), "{why}");
}

/// A picture has no key of its own to go stale: each export draws it again, so a change of the part's
/// colour alone changes the file, and going back gives the first one again.
#[test]
fn a_shaded_views_picture_follows_the_parts_colour() {
    let (mut s, sheet, [front, _]) = front_and_top();
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"display_style": "shaded"}}));
    let drawing = json!({"drawing": ids::encode(s.drawing("test").unwrap().id)});
    let coloured = |s: &mut Session, color: &str| {
        run(s, "drawing.close", json!({}));
        run(s, "appearance.assign", json!({"bodies": ["Block"], "color": color}));
        run(s, "drawing.open", drawing.clone());
        run(s, CMD, json!({"sheet": ids::encode(sheet)}))["svg"].as_str().unwrap().to_owned()
    };
    let red = coloured(&mut s, "#ff0000");
    assert_ne!(red, coloured(&mut s, "#0000ff"), "the colour shows in the picture");
    assert_eq!(red, coloured(&mut s, "#ff0000"));
}

#[test]
fn sheet_sketch_statuses_and_supported_strokes_reach_the_export() {
    let (mut s, sheet, _) = front_and_top();
    let sketch = sheet_sketch(
        &mut s,
        sheet,
        &[
            ("sketch.line", json!({"points": [[15,10],[55,10]], "infer": false})),
            ("sketch.circle.center", json!({"center": [80,30], "radius": 5})),
            ("sketch.spline.fit_point", json!({"points": [[100,10],[102,12],[104,10]], "connect": false})),
            ("sketch.conic", json!({"start": [120,10], "end": [140,10], "apex": [130,30], "rho": 0.5, "connect": false})),
        ],
    );
    let (doc, model, evaluations, undo, redo) = (s.doc.clone(), s.model.state(), s.evaluations, s.undo.len(), s.redo.len());
    let scene = run(&mut s, "drawing.scene", json!({"sheet": ids::encode(sheet)}));
    let expected = json!([{"sketch": ids::encode(sketch), "status": "drawn"}]);
    assert_eq!(scene["sketches"], expected);
    let scene_schema = output_schema_for::<crate::schema::drawing_scene::SheetSceneResult>();
    assert!(conforms(scene_schema.as_value(), scene_schema.as_value(), &scene), "{scene}");
    let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}));
    let export_schema = output_schema_for::<ExportResult>();
    assert!(conforms(export_schema.as_value(), export_schema.as_value(), &reply), "{reply}");
    assert_eq!(reply["sketches"], expected);
    assert_eq!(reply["svg"].as_str().map(str::as_bytes), Some(scene_svg(&s, sheet).as_slice()));
    assert!(reply["svg"].as_str().unwrap().contains("data-kind=\"sketch\""));
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc) && std::sync::Arc::ptr_eq(&model, &s.model.state()));
    assert_eq!((s.evaluations, s.undo.len(), s.redo.len()), (evaluations, undo, redo));
}

#[test]
fn failed_sheet_sketches_are_named_before_any_file_is_written() {
    let (mut s, sheet, [_, top]) = front_and_top();
    let good = sheet_sketch(&mut s, sheet, &[("sketch.line", json!({"points": [[15,10],[55,10]], "infer": false}))]);
    let made = run(&mut s, "drawing.sketch.create", json!({"owner": ids::encode(sheet)}));
    let bad = made["sketch"].as_str().unwrap().to_string();
    let line = run(&mut s, "sketch.line", json!({"points": [[20,30],[60,30]], "infer": false}));
    run(&mut s, "sketch.centerline", json!({"curves": line["curves"], "value": true}));
    run(&mut s, "sketch.finish", json!({}));
    let scene = run(&mut s, "drawing.scene", json!({"sheet": ids::encode(sheet)}));
    let statuses = scene["sketches"].as_array().unwrap();
    assert_eq!(statuses.len(), 2);
    assert!(statuses.iter().any(|s| s["sketch"] == ids::encode(good) && s["status"] == "drawn"));
    let failed = statuses.iter().find(|s| s["sketch"] == bad).unwrap();
    assert_eq!(failed["status"], "failed");
    assert!(failed["reason"].as_str().unwrap().contains("centerline"));
    let schema = output_schema_for::<crate::schema::drawing_scene::SheetSceneResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &scene), "{scene}");
    let mut extra = scene.clone();
    extra["sketches"][0]["unexpected"] = json!(true);
    assert!(!conforms(schema.as_value(), schema.as_value(), &extra), "{extra}");
    let mut wrong_status = scene.clone();
    wrong_status["sketches"][0]["status"] = json!("placeholder");
    assert!(!conforms(schema.as_value(), schema.as_value(), &wrong_status), "{wrong_status}");
    let folder = Folder::new("drawing-export-sketch-failed");
    let missing = folder.path("new.svg");
    let existing = folder.path("existing.svg");
    std::fs::write(&existing, b"existing drawing bytes").unwrap();
    let (doc, evaluations, undo, redo) = (s.doc.clone(), s.evaluations, s.undo.len(), s.redo.len());
    for path in [&missing, &existing] {
        let why = refused(&mut s, json!({"sheet": ids::encode(sheet), "path": path}));
        assert!(why.contains(&format!("sketches on the sheet failed: {bad} (")) && why.contains("centerline"), "{why}");
        assert!(!why.contains(&ids::encode(good)), "{why}");
    }
    assert!(!std::path::Path::new(&missing).exists());
    assert_eq!(std::fs::read(&existing).unwrap(), b"existing drawing bytes");
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc));
    assert_eq!((s.evaluations, s.undo.len(), s.redo.len()), (evaluations, undo, redo));
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"display_style": "shaded_visible"}}));
    let why = refused(&mut s, json!({"sheet": ids::encode(sheet), "path": missing}));
    assert!(why.contains(&format!("views on the sheet failed: {} (", ids::encode(top))), "{why}");
    assert!(why.contains(&format!("sketches on the sheet failed: {bad} (")), "{why}");
    assert!(!std::path::Path::new(&missing).exists());
}

#[test]
fn exact_sheet_freeforms_match_the_scene_schema_and_export_via_shared_grid() {
    let (mut s, sheet, _) = front_and_top();
    let sketch = sheet_sketch(
        &mut s,
        sheet,
        &[
            ("sketch.line", json!({"points": [[15,10],[55,10]], "infer": false})),
            ("sketch.circle.center", json!({"center": [80,30], "radius": 5})),
            ("sketch.ellipse", json!({"center": [110,30], "major": [120,30], "minor_radius": 5})),
            ("sketch.spline.fit_point", json!({"points": [[140,10],[142,12],[144,10]], "connect": false})),
            ("sketch.conic", json!({"start": [160,10], "end": [180,10], "apex": [170,30], "rho": 0.4, "connect": false})),
            ("sketch.spline.control_point_5", json!({"points": [[200,10],[210,30],[220,35],[230,35],[240,30],[250,10]], "connect": false})),
        ],
    );
    let scene = run(&mut s, "drawing.scene", json!({"sheet": ids::encode(sheet)}));
    let schema = output_schema_for::<crate::schema::drawing_scene::SheetSceneResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &scene), "{scene}");
    assert_eq!(scene["sketches"], json!([{"sketch": ids::encode(sketch), "status": "drawn"}]));
    let group = scene["scene"]["groups"].as_array().unwrap().iter().find(|g| g["object"] == sketch).unwrap();
    assert_eq!(group["items"].as_array().unwrap().len(), 6, "every authored printable curve stays in the scene");
    let types: std::collections::BTreeSet<&str> = group["items"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|item| item["path"].as_array().unwrap())
        .map(|segment| segment["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["arc", "bezier", "conic", "cubic", "line"].into_iter().collect::<std::collections::BTreeSet<_>>());
    // A3b now admits these exact primitives through the one shared export grid.
    // The scene still owns every original curve; the command must write that scene.
    let folder = Folder::new("drawing-export-exact-supported");
    let missing = folder.path("new.svg");
    let existing = folder.path("existing.svg");
    std::fs::write(&existing, b"existing exact drawing").unwrap();
    let (doc, model, evaluations, undo, redo) = (s.doc.clone(), s.model.state(), s.evaluations, s.undo.len(), s.redo.len());
    let expected = scene_svg(&s, sheet);
    let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}));
    assert_eq!(reply["svg"].as_str().map(str::as_bytes), Some(expected.as_slice()));
    assert_eq!(reply["sketches"], scene["sketches"]);
    let svg_text = std::str::from_utf8(&expected).unwrap();
    let sketch_svg = svg_text.split(&format!("<g data-object=\"{sketch}\" data-kind=\"sketch\">")).nth(1).unwrap().split("</g>").next().unwrap();
    assert_eq!(sketch_svg.matches("<path ").count(), 6, "all six original curves reach the file");
    assert!(sketch_svg.contains('C'), "freeforms are written as shared-grid cubic paths");
    for path in [&missing, &existing] {
        let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet), "path": path}));
        assert_eq!(reply["bytes"].as_u64(), Some(expected.len() as u64));
        assert_eq!(reply["sketches"], scene["sketches"]);
        assert_eq!(std::fs::read(path).unwrap(), expected, "new and existing destinations receive the complete same scene");
    }
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc) && std::sync::Arc::ptr_eq(&model, &s.model.state()));
    assert_eq!((s.evaluations, s.undo.len(), s.redo.len()), (evaluations, undo, redo));
}
