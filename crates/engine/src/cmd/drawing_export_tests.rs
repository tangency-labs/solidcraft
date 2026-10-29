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
    solidcraft_io::paper::paper_svg(&built.scene.prepare().unwrap()).unwrap().0
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
