use serde_json::{Value, json};
use solidcraft_doc::MAX_PLACEMENT_DEPTH;
use solidcraft_doc::canvas::base64_decode;
use solidcraft_io::paper::paper_png;

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

/// The scene of sheet `sheet` of the drawing open, built at once.
fn sheet_scene(s: &Session, sheet: u64) -> solidcraft_geom::paper::Scene {
    let d = s.drawing("test").unwrap().clone();
    crate::drawing_scene::build(s, &d, sheet).unwrap().scene
}

/// The SVG of sheet `sheet` of the drawing open, written straight from its built scene.
fn scene_svg(s: &Session, sheet: u64) -> Vec<u8> {
    solidcraft_io::paper::paper_svg(&sheet_scene(s, sheet).prepare().unwrap(), &NoPictures, DEFAULT_DPI).unwrap().0
}

/// The PNG of sheet `sheet` of the drawing open, written straight from its built scene.
fn scene_png(s: &Session, sheet: u64, options: PngOptions) -> Vec<u8> {
    let mut png = Vec::new();
    paper_png(&sheet_scene(s, sheet).prepare().unwrap(), options, &NoPictures, &mut png).unwrap();
    png
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
    assert_eq!(reply["text"].as_str().map(str::as_bytes), Some(svg.as_slice()));
    assert_eq!((&reply["format"], reply["bytes"].as_u64(), reply.get("path")), (&json!("svg"), Some(svg.len() as u64), None), "{reply}");
    // Each view on the sheet, in id order, as drawing.scene lists them.
    let views: Vec<_> = reply["views"].as_array().unwrap().iter().map(|v| (v["view"].clone(), v["status"].clone())).collect();
    let mut in_order = [front, top];
    in_order.sort_unstable();
    assert_eq!(views, in_order.map(|v| (json!(ids::encode(v)), json!("drawn"))));
    let name = s.drawing("test").unwrap().sheets[0].name.clone();
    assert_eq!(run(&mut s, CMD, json!({"sheet": name, "format": "svg"}))["text"], reply["text"]);
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc) && s.evaluations == evaluations);
    // The sheet named, not the first: a second sheet, with no views, exports as its own scene.
    let added = run(&mut s, "drawing.sheet.add", json!({}));
    let second = ids::decode(added["sheet"].as_str().unwrap()).unwrap();
    let other = run(&mut s, CMD, json!({"sheet": ids::encode(second)}));
    assert_eq!((&other["sheet"], other["views"].as_array().map(Vec::len)), (&json!(ids::encode(second)), Some(0)), "{other}");
    assert_eq!(other["text"].as_str().map(str::as_bytes), Some(scene_svg(&s, second).as_slice()));
    assert_ne!(other["text"], reply["text"]);
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
        (reply["path"].as_str(), reply.get("text"), reply["bytes"].as_u64()),
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
    assert_eq!(reply["text"].as_str().map(str::as_bytes), Some(scene_svg(&s, sheet).as_slice()));
}

/// A sheet exports as its scene's PNG, byte for byte: in the reply as base64 data, grey at the
/// sheet's own dpi when nothing says (300 for a new sheet, so A3 is 4961 x 3508 pixels: 420 and
/// 297 mm at 300 / 25.4 a millimetre, rounded; then 50 once the sheet says so), and at each colour
/// and a chosen dpi; with a path (its extension in any case) the file holds it
/// and the reply names it instead. The reply's shape is the SVG's, as its schema says.
#[test]
fn a_sheet_exports_as_its_scenes_png() {
    let (mut s, sheet, _) = front_and_top();
    let id = ids::encode(sheet);
    let schema = output_schema_for::<ExportResult>();
    let reply = run(&mut s, CMD, json!({"sheet": id, "format": "png"}));
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    let png = scene_png(&s, sheet, PngOptions::default());
    assert_eq!(reply["data"].as_str().and_then(base64_decode), Some(png.clone()));
    assert_eq!((reply["bytes"].as_u64(), &reply["report"]["pixels"]), (Some(png.len() as u64), &json!([4961, 3508])), "{reply}");
    assert_eq!((reply.get("text"), reply.get("path"), &reply["format"]), (None, None, &json!("png")));
    run(&mut s, "drawing.sheet.set", json!({"sheet": id, "dpi": 50}));
    let reply = run(&mut s, CMD, json!({"sheet": id, "format": "png"}));
    let png = scene_png(&s, sheet, PngOptions { dpi: 50.0, colour: PngColour::Grey });
    assert_eq!(reply["data"].as_str().and_then(base64_decode), Some(png), "the sheet's own dpi");
    for (colour, as_written) in
        [("grey", PngColour::Grey), ("black_white", PngColour::BlackWhite), ("rgb", PngColour::Rgb), ("transparent", PngColour::Transparent)]
    {
        let reply = run(&mut s, CMD, json!({"sheet": id, "format": "png", "dpi": 25, "colour": colour}));
        let png = scene_png(&s, sheet, PngOptions { dpi: 25.0, colour: as_written });
        assert_eq!(reply["data"].as_str().and_then(base64_decode), Some(png), "{colour}");
        assert_eq!(reply["report"]["pixels"], json!([414, 293]), "{colour}");
    }
    let folder = Folder::new("drawing-export-png");
    let path = folder.path("Sheet 1.PNG");
    let reply = run(&mut s, CMD, json!({"sheet": id, "format": "png", "path": path, "dpi": 2400.0 / 32.0}));
    let written = std::fs::read(&path).unwrap();
    assert_eq!(written, scene_png(&s, sheet, PngOptions { dpi: 75.0, colour: PngColour::Grey }));
    assert_eq!((reply["path"].as_str(), reply.get("data"), reply["bytes"].as_u64()), (Some(path.as_str()), None, Some(written.len() as u64)));
}

/// `paper` reaches both writers, and white is what no `paper` is, byte for byte. A blue paper's PNG
/// is rgb, asked or not; any other colour is refused by name, in place of being ignored.
#[test]
fn a_sheet_exports_on_its_paper() {
    let (mut s, sheet, _) = front_and_top();
    let id = ids::encode(sheet);
    let data = |reply: Value| reply["data"].as_str().and_then(base64_decode).unwrap();
    let plain = scene_svg(&s, sheet);
    let reply = run(&mut s, CMD, json!({"sheet": id, "paper": "white"}));
    assert_eq!(reply["text"].as_str().map(str::as_bytes), Some(plain.as_slice()));
    for colour in [json!({}), json!({"colour": "rgb"}), json!({"colour": "grey"}), json!({"colour": "black_white"}), json!({"colour": "transparent"})]
    {
        let mut asked = json!({"sheet": id, "format": "png", "dpi": 25});
        asked.as_object_mut().unwrap().extend(colour.as_object().unwrap().clone());
        let mut white = asked.clone();
        white["paper"] = json!("white");
        assert_eq!(data(run(&mut s, CMD, white)), data(run(&mut s, CMD, asked)), "{colour}");
    }
    for (name, paper, hex) in [("blueprint", PaperColour::Blueprint, "#123A76"), ("prussian_blue", PaperColour::PrussianBlue, "#003153")] {
        let reply = run(&mut s, CMD, json!({"sheet": id, "paper": name}));
        let text = reply["text"].as_str().unwrap();
        assert!(text.contains(&format!("fill=\"{hex}\"")) && text.contains("stroke=\"#E2EEFC\"") && !text.contains("#000"), "{name}");
        let png = scene_png(&s, sheet, PngOptions { dpi: 25.0, colour: PngColour::On(paper) });
        assert_eq!(data(run(&mut s, CMD, json!({"sheet": id, "format": "png", "dpi": 25, "paper": name}))), png, "{name}");
        assert_eq!(data(run(&mut s, CMD, json!({"sheet": id, "format": "png", "dpi": 25, "paper": name, "colour": "rgb"}))), png, "{name}");
        for colour in ["grey", "black_white", "transparent"] {
            let why = refused(&mut s, json!({"sheet": id, "format": "png", "paper": name, "colour": colour}));
            assert!(why.contains("a blue `paper` is rgb") && why.contains(colour), "{name}, {colour}: {why}");
        }
        assert!(refused(&mut s, json!({"sheet": id, "format": "dxf", "paper": name})).contains("not supported yet: DXF"), "{name}");
    }
    assert!(refused(&mut s, json!({"sheet": id, "paper": "cyan"})).contains("paper"));
    assert!(refused(&mut s, json!({"sheet": id, "paper": null})).contains("paper"));
}

/// A document held for the reply takes up to its limit and is refused past it, naming the way out.
#[test]
fn a_reply_holds_a_document_up_to_its_limit() {
    let (s, sheet, _) = front_and_top();
    let options = PngOptions { dpi: 25.0, colour: PngColour::Grey };
    let png = scene_png(&s, sheet, options);
    let mut exact = Held { bytes: Vec::new(), limit: png.len() };
    paper_png(&sheet_scene(&s, sheet).prepare().unwrap(), options, &NoPictures, &mut exact).unwrap();
    assert_eq!(exact.bytes, png);
    let mut short = Held { bytes: Vec::new(), limit: png.len() - 1 };
    let why = paper_png(&sheet_scene(&s, sheet).prepare().unwrap(), options, &NoPictures, &mut short).unwrap_err().to_string();
    assert!(why.contains("for the reply: give a path"), "{why}");
}

/// DXF isn't supported yet; a path without the format's extension, a dpi outside 25 to 2400, a dpi
/// or colour on an SVG, an unknown colour, an unknown sheet, an unknown key and no drawing open are
/// refused, and nothing is written.
#[test]
fn what_it_cant_write_is_refused() {
    let (mut s, sheet, _) = front_and_top();
    let folder = Folder::new("drawing-export-refused");
    let id = ids::encode(sheet);
    let long = format!("{}.svg", "a".repeat(MAX_PATH_BYTES));
    for (p, why) in [
        (json!({"sheet": id, "format": "dxf"}), "not supported yet: DXF"),
        (json!({"sheet": id, "format": "dxf", "path": folder.path("sheet.dxf")}), "not supported yet: DXF"),
        (json!({"sheet": id, "format": "pdf"}), "format"),
        (json!({"sheet": id, "path": folder.path("sheet.png")}), "path must be a .svg file"),
        (json!({"sheet": id, "format": "png", "path": folder.path("sheet.svg")}), "path must be a .png file"),
        (json!({"sheet": id, "format": "png", "path": folder.path("sheet")}), "path must be a .png file"),
        (json!({"sheet": id, "format": "png", "dpi": 24.999}), "`dpi` is 25 to 2400"),
        (json!({"sheet": id, "format": "png", "dpi": 2400.001}), "`dpi` is 25 to 2400"),
        (json!({"sheet": id, "format": "png", "dpi": 0}), "`dpi` is 25 to 2400"),
        (json!({"sheet": id, "format": "png", "dpi": -300}), "`dpi` is 25 to 2400"),
        (json!({"sheet": id, "format": "png", "dpi": 1e308}), "`dpi` is 25 to 2400"),
        (json!({"sheet": id, "format": "png", "dpi": null}), "dpi"),
        (json!({"sheet": id, "format": "png", "dpi": "300"}), "dpi"),
        (json!({"sheet": id, "format": "png", "colour": "cmyk"}), "colour"),
        (json!({"sheet": id, "format": "png", "colour": null}), "colour"),
        (json!({"sheet": id, "colour": "grey"}), "`dpi` and `colour` are for png only"),
        (json!({"sheet": id, "path": folder.path("sheet")}), "path must be a .svg file"),
        (json!({"sheet": id, "path": " "}), "path must be a .svg file"),
        (json!({"sheet": id, "path": long}), "path must be a .svg file"),
        (json!({"sheet": id, "path": null}), "path"),
        (json!({"sheet": "Sheet 9"}), "no sheet `Sheet 9`"),
        (json!({"sheet": 5}), "sheet"),
        (json!({}), "sheet"),
        (json!({"sheet": id, "dpi": 300}), "`dpi` and `colour` are for png only"),
        (json!({"sheet": id, "format": "png", "scale": 2}), "scale"),
    ] {
        let message = refused(&mut s, p.clone());
        assert!(message.contains(why), "{p}: {message}");
    }
    assert_eq!(std::fs::read_dir(&folder.0).unwrap().count(), 0);
    run(&mut s, "drawing.close", json!({}));
    assert!(refused(&mut s, json!({"sheet": id})).contains("open a drawing first"));
}
