use serde_json::{Value, json};
use solidcraft_doc::MAX_PLACEMENT_DEPTH;
use solidcraft_geom::Vec2Geom;
use solidcraft_geom::paper::NoPictures;
use solidcraft_io::paper::{PngColour, paper_png};

use super::*;
use crate::drawing_scene::tests::{front_and_top, in_the_model, milestone_plate_script, nest, reopened};
use crate::schema::output_schema_for;
use crate::schema::tests::conforms;
use crate::tests::{Folder, png_size};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap()
}

/// Exports `p` to a file of its own (a PNG or PDF is always written to one): the reply and the
/// file's bytes.
fn export_file(s: &mut Session, mut p: Value) -> (Value, Vec<u8>) {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let folder = Folder::new(&format!("drawing-export-file-{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let path = folder.path(&format!("sheet.{}", p["format"].as_str().unwrap_or("svg")));
    p["path"] = json!(path);
    let reply = run(s, CMD, p);
    (reply, std::fs::read(&path).unwrap())
}

/// The bytes [`export_file`] writes.
fn exported(s: &mut Session, p: Value) -> Vec<u8> {
    export_file(s, p).1
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

/// A sheet with the built-in title block exports its labels and values as text: in SVG one path of
/// filled outlines for each run of the scene, and in PNG its pixels (#4890 B3).
#[test]
fn a_title_blocks_text_is_written_as_filled_outlines() {
    let (mut s, sheet, _) = front_and_top();
    let block = serde_json::to_value(solidcraft_geom::title_block::default_block()).unwrap();
    run(&mut s, "drawing.sheet.set", json!({"sheet": ids::encode(sheet), "title_block": block}));
    let scene = sheet_scene(&s, sheet);
    let runs = scene.groups.iter().flat_map(|g| &g.items).filter(|item| matches!(item, solidcraft_geom::paper::Item::Text(_))).count();
    let svg = run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}))["svg"].as_str().unwrap().to_string();
    assert!(runs > 12, "each label and value: {runs}");
    assert_eq!(svg.matches("<path fill=\"#000\" stroke=\"none\" d=\"M").count(), runs);
    let png = exported(&mut s, json!({"sheet": ids::encode(sheet), "format": "png", "dpi": 50}));
    assert_eq!(png, scene_png(&s, sheet, PngOptions { dpi: 50.0, colour: PngColour::Grey }));
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

/// A PNG export stopped part way (the window's background export, cancelled once its first band is
/// drawn) is refused and writes no file: the file is written only once the whole sheet is drawn.
#[test]
fn a_cancelled_png_export_writes_no_file() {
    /// Cancels as soon as the sheet PNG has reported a band, so nothing before it is stopped.
    #[derive(Default)]
    struct AfterABand(std::sync::atomic::AtomicBool);
    impl solidcraft_kernel::watch::Watch for AfterABand {
        fn cancelled(&self) -> bool {
            self.0.load(std::sync::atomic::Ordering::Relaxed)
        }
        fn progress(&self, stage: &'static str, _: usize, _: usize) {
            if stage == "sheet_png" {
                self.0.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }
    let (mut s, sheet, _) = front_and_top();
    let folder = Folder::new("drawing-export-cancelled");
    let path = folder.path("sheet.png");
    // A3 at 300 dpi draws in 5 bands.
    let p = json!({"sheet": ids::encode(sheet), "format": "png", "path": path});
    let watch = std::sync::Arc::new(AfterABand::default());
    let why = solidcraft_kernel::watch::watching(watch.clone(), || s.execute(CMD, &p)).unwrap_err().to_string();
    assert!(why.contains("the sheet PNG: cancelled") && watch.0.load(std::sync::atomic::Ordering::Relaxed), "{why}");
    assert!(!std::path::Path::new(&path).exists());
    run(&mut s, CMD, p);
    assert!(std::path::Path::new(&path).exists(), "unwatched, it is written");
}

/// A view drawing hidden lines exports with the rest of its sheet, dashed: a style that used to
/// refuse the whole sheet. The top view is projected from the front one, so it follows its style:
/// 4 dashes for the pocket seen from the front, 3 from above (its fourth edge is on the outline).
#[test]
fn a_view_with_hidden_lines_exports_dashed() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 10, "height": 10, "corner": [30, 40, 5], "operation": "cut", "targets": ["Block"]}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    assert!(!String::from_utf8(scene_svg(&s, sheet)).unwrap().contains("stroke-dasharray"));
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"visual_style": "visible_hidden"}}));
    let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}));
    let statuses: Vec<_> = reply["views"].as_array().unwrap().iter().map(|v| v["status"].clone()).collect();
    assert_eq!(statuses, [json!("drawn"), json!("drawn")], "{reply}");
    let svg = reply["svg"].as_str().unwrap();
    assert_eq!(svg.matches("stroke-dasharray=\"3 0.75\"").count(), 7, "{svg}");
    assert_eq!(svg.as_bytes(), scene_svg(&s, sheet).as_slice());
}

/// A view that failed stops the export, and the refusal names it with why; a placeholder draws
/// nothing and is exported all the same.
#[test]
fn a_failed_view_is_named_and_nothing_is_written() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let folder = Folder::new("drawing-export-failed");
    let path = folder.path("sheet.svg");
    crate::drawing_scene::tests::not_drawable_yet(&mut s, top);
    let why = refused(&mut s, json!({"sheet": ids::encode(sheet), "path": path}));
    let reason = crate::drawing_scene::tests::NOT_DRAWABLE_YET;
    assert!(why.contains(&format!("views on the sheet failed: {} ({reason}", ids::encode(top))), "{why}");
    assert!(!why.contains(&ids::encode(front)) && !std::path::Path::new(&path).exists(), "{why}");
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"visual_style": "visible", "pose": null}}));
    for level in 0..=MAX_PLACEMENT_DEPTH as u64 + 1 {
        nest(&mut s, level);
    }
    let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}));
    let statuses: Vec<_> = reply["views"].as_array().unwrap().iter().map(|v| v["status"].clone()).collect();
    assert_eq!(statuses, [json!("placeholder"), json!("placeholder")], "{reply}");
    assert_eq!(reply["svg"].as_str().map(str::as_bytes), Some(scene_svg(&s, sheet).as_slice()));
}

/// A view of a configuration row is exported as the row builds it (#13048): nothing has drawn it before
/// the export, so its copy is made for the export, on an automatic drawing and on a deferred one. A
/// view of a row the design no longer has, and the view that follows it, are placeholders that say
/// why, and the sheet is written all the same.
#[test]
fn a_view_of_a_configuration_row_exports_as_the_row_builds_it() {
    let (mut s, sheet, [front, _], _) = crate::drawing_scene::tests::configured_front_and_top();
    let wide = |shown: &crate::drawing_scene::SheetScene| shown.objects.iter().find(|o| o.object == front).map(|o| o.bounds_mm.size().x);
    let statuses = |reply: &Value| reply["views"].as_array().unwrap().iter().map(|v| (v["status"].clone(), v["reason"].clone())).collect::<Vec<_>>();
    let drawn = vec![(json!("drawn"), Value::Null); 2];
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"configuration": "Long"}}));
    let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}));
    assert_eq!(statuses(&reply), drawn, "{reply}");
    assert_eq!(reply["svg"].as_str().map(str::as_bytes), Some(scene_svg(&s, sheet).as_slice()));
    let d = s.drawing("test").unwrap().clone();
    assert!(wide(&crate::drawing_scene::build(&s, &d, sheet).unwrap()).is_some_and(|w| (w - 60.0).abs() < 1e-6));
    // Deferred and updated: the export draws it from the model as its update left it.
    run(&mut s, "drawing.set", json!({"updates": "deferred"}));
    run(&mut s, "drawing.update", json!({}));
    let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}));
    assert_eq!(statuses(&reply), drawn, "{reply}");
    let d = s.drawing("test").unwrap().clone();
    assert!(wide(&crate::drawing_scene::as_shown(&mut s, &d, sheet).unwrap()).is_some_and(|w| (w - 60.0).abs() < 1e-6));
    // Its row deleted.
    run(&mut s, "drawing.set", json!({"updates": "automatic"}));
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "config.row", json!({"name": "Long", "delete": true}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}));
    let why = json!("the configuration it draws is no longer in the design");
    assert_eq!(statuses(&reply), vec![(json!("placeholder"), why); 2], "{reply}");
    assert_eq!(reply["svg"].as_str().map(str::as_bytes), Some(scene_svg(&s, sheet).as_slice()));
}

/// A sheet exports as its scene's PNG, byte for byte, written to a file (never into the reply), grey
/// at the sheet's own dpi when nothing says (300 for a new sheet, so A3 is 4961 x 3508 pixels: 420 and
/// 297 mm at 300 / 25.4 a millimetre, rounded; then 50 once the sheet says so), and at each colour
/// and a chosen dpi; with a path (its extension in any case) the file holds it
/// and the reply names it instead. The reply's shape is the SVG's, as its schema says.
#[test]
fn a_sheet_exports_as_its_scenes_png() {
    let (mut s, sheet, _) = front_and_top();
    let id = ids::encode(sheet);
    let schema = output_schema_for::<ExportResult>();
    // Without a path it is refused, naming the way that works: its bytes are never text.
    assert!(refused(&mut s, json!({"sheet": id, "format": "png"})).contains("a PNG is written to a file: give `path`"));
    let (reply, written) = export_file(&mut s, json!({"sheet": id, "format": "png"}));
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    let png = scene_png(&s, sheet, PngOptions::default());
    assert_eq!(written, png);
    assert_eq!((reply["bytes"].as_u64(), &reply["report"]["pixels"]), (Some(png.len() as u64), &json!([4961, 3508])), "{reply}");
    assert_eq!((reply.get("svg"), reply.get("data"), &reply["format"]), (None, None, &json!("png")));
    run(&mut s, "drawing.sheet.set", json!({"sheet": id, "dpi": 50}));
    let png = scene_png(&s, sheet, PngOptions { dpi: 50.0, colour: PngColour::Grey });
    assert_eq!(exported(&mut s, json!({"sheet": id, "format": "png"})), png, "the sheet's own dpi");
    for (colour, as_written) in
        [("grey", PngColour::Grey), ("black_white", PngColour::BlackWhite), ("rgb", PngColour::Rgb), ("transparent", PngColour::Transparent)]
    {
        let (reply, written) = export_file(&mut s, json!({"sheet": id, "format": "png", "dpi": 25, "colour": colour}));
        let png = scene_png(&s, sheet, PngOptions { dpi: 25.0, colour: as_written });
        assert_eq!(written, png, "{colour}");
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
    let plain = scene_svg(&s, sheet);
    let reply = run(&mut s, CMD, json!({"sheet": id, "paper": "white"}));
    assert_eq!(reply["svg"].as_str().map(str::as_bytes), Some(plain.as_slice()));
    for colour in [json!({}), json!({"colour": "rgb"}), json!({"colour": "grey"}), json!({"colour": "black_white"}), json!({"colour": "transparent"})]
    {
        let mut asked = json!({"sheet": id, "format": "png", "dpi": 25});
        asked.as_object_mut().unwrap().extend(colour.as_object().unwrap().clone());
        let mut white = asked.clone();
        white["paper"] = json!("white");
        assert_eq!(exported(&mut s, white), exported(&mut s, asked), "{colour}");
    }
    for (name, paper, hex) in [("blueprint", PaperColour::Blueprint, "#123A76"), ("prussian_blue", PaperColour::PrussianBlue, "#003153")] {
        let reply = run(&mut s, CMD, json!({"sheet": id, "paper": name}));
        let text = reply["svg"].as_str().unwrap();
        assert!(text.contains(&format!("fill=\"{hex}\"")) && text.contains("stroke=\"#E2EEFC\"") && !text.contains("#000"), "{name}");
        let png = scene_png(&s, sheet, PngOptions { dpi: 25.0, colour: PngColour::On(paper) });
        assert_eq!(exported(&mut s, json!({"sheet": id, "format": "png", "dpi": 25, "paper": name})), png, "{name}");
        assert_eq!(exported(&mut s, json!({"sheet": id, "format": "png", "dpi": 25, "paper": name, "colour": "rgb"})), png, "{name}");
        for colour in ["grey", "black_white", "transparent"] {
            let why = refused(&mut s, json!({"sheet": id, "format": "png", "paper": name, "colour": colour}));
            assert!(why.contains("a blue `paper` is rgb") && why.contains(colour), "{name}, {colour}: {why}");
        }
        assert!(refused(&mut s, json!({"sheet": id, "format": "dxf", "paper": name})).contains("a dxf has no paper"), "{name}");
    }
    assert!(refused(&mut s, json!({"sheet": id, "paper": "cyan"})).contains("paper"));
    assert!(refused(&mut s, json!({"sheet": id, "paper": null})).contains("paper"));
}

/// Each format agrees with the sheet's own size (#4894, W23-3): A3 is 420 x 297 mm in the SVG, and
/// 2481 x 1754 pixels at 150 dpi in each colour's PNG header (420 and 297 mm are 2480.3 and 1753.9
/// pixels, and a sheet's grid rounds each side outward). Saved with the design and opened
/// again, the sheet exports the same bytes in every format and colour. PDF and DXF join as their
/// writers land.
#[test]
fn each_format_agrees_with_the_sheet_and_exports_the_same_once_reopened() {
    let (mut s, sheet, _) = front_and_top();
    let id = ids::encode(sheet);
    let exports = |s: &mut Session| {
        let mut all = vec![run(s, CMD, json!({"sheet": id}))["svg"].as_str().unwrap().as_bytes().to_vec()];
        for colour in ["grey", "black_white", "rgb", "transparent"] {
            all.push(exported(s, json!({"sheet": id, "format": "png", "dpi": 150, "colour": colour})));
        }
        all
    };
    let before = exports(&mut s);
    let svg = String::from_utf8_lossy(&before[0]).into_owned();
    assert!(svg.contains(r#"width="420mm" height="297mm" viewBox="0 0 420 297""#), "{}", &svg[..svg.len().min(300)]);
    for png in &before[1..] {
        assert_eq!((&png[12..16], png_size(png)), (&b"IHDR"[..], (2481, 1754)));
    }
    let drawing = ids::encode(s.drawing("test").unwrap().id);
    let folder = Folder::new("drawing-export-agree");
    run(&mut s, "file.save", json!({"path": folder.path("sheet.solidcraft")}));
    let mut opened = Session::default();
    run(&mut opened, "doc.open", json!({"path": folder.path("sheet.solidcraft")}));
    run(&mut opened, "drawing.open", json!({"drawing": drawing}));
    assert!(exports(&mut opened) == before, "a format exports differently once reopened");
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
        (json!({"sheet": id, "format": "dxf", "path": folder.path("sheet.svg")}), "path must be a .dxf file"),
        (json!({"sheet": id, "format": "dxf", "dpi": 300}), "`dpi` and `colour` are for png only"),
        (json!({"format": "dxf", "sheets": [id]}), "`sheets` is for pdf only"),
        (json!({"sheet": id, "format": "pdf", "paper": "blueprint"}), "a pdf is on white paper"),
        (json!({"sheet": id, "format": "pdf", "dpi": 300}), "`dpi` and `colour` are for png only"),
        (json!({"sheet": id, "format": "pdf", "path": folder.path("sheet.svg")}), "path must be a .pdf file"),
        (json!({"sheet": id, "format": "pdf", "sheets": [id]}), "give `sheet` or `sheets`, not both"),
        (json!({"format": "pdf", "sheets": []}), "`sheets` names at least one sheet"),
        (json!({"format": "pdf", "sheets": [id, id]}), "twice"),
        (json!({"format": "pdf", "sheets": ["Sheet 9"]}), "no sheet `Sheet 9`"),
        (json!({"format": "pdf", "sheets": id}), "sheets"),
        (json!({"format": "pdf", "sheets": [5]}), "sheets"),
        (json!({"sheets": [id]}), "`sheets` is for pdf only"),
        (json!({"format": "png", "sheets": [id]}), "`sheets` is for pdf only"),
        (json!({"format": "cdr"}), "format"),
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

/// A sheet with shaded views exports their pictures as `<image>`s, the same bytes twice, and draws
/// them in a PNG. A picture past the grid's limit is refused and nothing is written.
#[test]
fn a_shaded_view_exports_its_picture() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let png = |s: &mut Session| exported(s, json!({"sheet": ids::encode(sheet), "format": "png", "dpi": 25}));
    let lines_only = png(&mut s);
    let plain = run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}))["svg"].as_str().unwrap().to_owned();
    assert!(!plain.contains("<image"), "lines only: {}", plain.len());
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"visual_style": "shaded"}}));
    let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}));
    let svg = reply["svg"].as_str().unwrap();
    assert_eq!(svg.matches("<image").count(), 2, "the front view's, and the top view's, which follows it: {reply}");
    assert_eq!(run(&mut s, CMD, json!({"sheet": ids::encode(sheet)}))["svg"], reply["svg"], "the same bytes twice");
    assert_ne!(png(&mut s), lines_only, "the PNG draws the pictures too");
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
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"visual_style": "shaded"}}));
    let coloured = |s: &mut Session, color: &str| {
        in_the_model(s, "appearance.assign", json!({"bodies": ["Block"], "color": color}));
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
    s.doc_mut().add_user_param("Inset", "20 mm", Some("mm"), None).unwrap();
    let good = sheet_sketch(&mut s, sheet, &[("sketch.line", json!({"points": [[15,10],[55,10]], "infer": false}))]);
    let made = run(&mut s, "drawing.sketch.create", json!({"owner": ids::encode(sheet)}));
    let bad = made["sketch"].as_str().unwrap().to_string();
    run(&mut s, "sketch.line", json!({"points": [["Inset", "30"], ["Inset + 40", "30"]], "infer": false}));
    run(&mut s, "sketch.finish", json!({}));
    // The stored sketch is admissible, but its source can later become unresolved. Exercise
    // the exporter guard with that source failure; unsupported centerlines are refused on entry.
    s.doc_mut().params.iter_mut().find(|param| param.name == "Inset").unwrap().expr = "Missing + 1 mm".into();
    s.drawing("test").unwrap().validate().unwrap();
    let scene = run(&mut s, "drawing.scene", json!({"sheet": ids::encode(sheet)}));
    assert!(scene["views"].as_array().unwrap().iter().all(|view| view["status"] == "drawn"), "{scene}");
    let statuses = scene["sketches"].as_array().unwrap();
    assert_eq!(statuses.len(), 2);
    assert!(statuses.iter().any(|s| s["sketch"] == ids::encode(good) && s["status"] == "drawn"));
    let failed = statuses.iter().find(|s| s["sketch"] == bad).unwrap();
    assert_eq!(failed["status"], "failed");
    assert!(failed["reason"].as_str().unwrap().contains("Inset"));
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
        assert!(why.contains(&format!("sketches on the sheet failed: {bad} (")) && why.contains("Inset"), "{why}");
        assert!(!why.contains(&ids::encode(good)), "{why}");
    }
    assert!(!std::path::Path::new(&missing).exists());
    assert_eq!(std::fs::read(&existing).unwrap(), b"existing drawing bytes");
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc));
    assert_eq!((s.evaluations, s.undo.len(), s.redo.len()), (evaluations, undo, redo));
    crate::drawing_scene::tests::not_drawable_yet(&mut s, top);
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

/// The drawing milestone's plate (acceptance case 84, #4894) by commands alone: modelled from its
/// parameters, a top view at 1:2 and the views projected below and right of it, its length, width
/// and first bore dimensioned. With the ids of the top, below and right views, and each dimension's
/// id, value and text.
fn milestone_plate() -> (Session, [u64; 3], Vec<(Value, Value, Value)>) {
    let (mut s, top) = milestone_plate_script();
    let top = &top;
    let below = run(&mut s, "drawing.view.projected", json!({"parent": top["view"], "side": "below"}));
    let right = run(&mut s, "drawing.view.projected", json!({"parent": top["view"], "side": "right"}));
    for (view, expected) in [top, &below, &right].into_iter().zip(PLATE_VIEWS) {
        assert!(same_size(box_size(&view["boundary_mm"]), expected), "{view}");
    }
    let views = [top, &below, &right].map(|view| ids::decode(view["view"].as_str().unwrap()).unwrap());
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
    (s, views, dimensions)
}

/// Saved with the design and opened again, the plate's sheet exports the same SVG, byte for byte,
/// and each dimension reads the same.
#[test]
fn the_milestone_plate_exports_the_same_after_it_is_saved_and_opened() {
    let (mut s, _, dimensions) = milestone_plate();
    let svg = run(&mut s, CMD, json!({"sheet": "Sheet 1"}))["svg"].clone();
    let drawing = s.drawing("test").unwrap().id;
    let mut opened = reopened(&mut s, drawing, "plate.solidcraft");
    assert_eq!(run(&mut opened, CMD, json!({"sheet": "Sheet 1"}))["svg"], svg);
    for (dimension, value, text) in dimensions {
        let read = run(&mut opened, "drawing.dimension.inspect", json!({"dimension": dimension}));
        assert_eq!((&read["value"], &read["text"], &read["status"]), (&value, &text, &json!("ok")), "{read}");
    }
}

/// The plate's top, below and right views at 1:2, in sheet mm: length by width, length by thickness,
/// thickness by width.
const PLATE_VIEWS: [[f64; 2]; 3] = [[50.0, 40.0], [50.0, 10.0], [10.0, 40.0]];
/// The same with the width at 100 (acceptance case 85).
const WIDE_VIEWS: [[f64; 2]; 3] = [[50.0, 50.0], [50.0, 10.0], [10.0, 50.0]];

/// The size of a reply's box, `{min, max}` in sheet mm.
fn box_size(b: &Value) -> [f64; 2] {
    [0, 1].map(|i| b["max"][i].as_f64().unwrap() - b["min"][i].as_f64().unwrap())
}

/// Whether two sizes are the same within 1e-6 mm.
fn same_size(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6
}

/// What each of `views` draws on Sheet 1 of the drawing open: the box round its lines, in sheet mm,
/// as `drawing.scene` reads it. That is what the export draws, so a deferred drawing's is what its
/// last update allows.
fn view_sizes(s: &mut Session, views: [u64; 3]) -> [[f64; 2]; 3] {
    let reply = run(s, "drawing.scene", json!({"sheet": "Sheet 1"}));
    let objects = reply["objects"].as_array().unwrap();
    views.map(|view| box_size(&objects.iter().find(|o| o["object"] == json!(ids::encode(view))).unwrap()["bounds_mm"]))
}

/// What each of `views` draws on Sheet 1 as the window's page shows it once settled: the provider's
/// lines ([`crate::drawing_scene::extract`]), which it keeps between reads and across a close and
/// draws again only for a view out of date. For a drawing that updates at once, `drawing.scene` and
/// the export draw anew instead, so only this reads what the page keeps.
fn page_sizes(s: &mut Session, views: [u64; 3]) -> [[f64; 2]; 3] {
    let d = s.drawing("test").unwrap().clone();
    crate::drawing_scene::extract(s, d.sheets[0].id).unwrap();
    crate::drawing_scene::settle(s, &d);
    let page = crate::drawing_scene::extract(s, d.sheets[0].id).unwrap();
    let size = |view: u64| page.objects.iter().find(|o| o.object == view).unwrap().bounds_mm.size();
    views.map(|view| [size(view).x, size(view).y])
}

/// Whether three views' sizes are the ones `expected` gives.
fn sized_as(sizes: [[f64; 2]; 3], expected: [[f64; 2]; 3]) -> bool {
    sizes.into_iter().zip(expected).all(|(size, expected)| same_size(size, expected))
}

/// The plate's model edited with its drawing closed (acceptance cases 85 to 87, #4894): each view's
/// lines follow with the hand sizes, in the export and on the window's page, and each edit saved and
/// opened again exports the same SVG. Undo brings back the sheet before it byte for byte, and redo
/// the one after it. The edited model's own geometry is oracle cases 85 to 87's (tests/oracle), and
/// what the dimensions read across these edits is in drawing_dimension_tests.rs (#8288).
#[test]
fn the_milestone_plate_follows_its_model_and_undo_brings_its_sheet_back() {
    let (mut s, views, _) = milestone_plate();
    let drawing = s.drawing("test").unwrap().id;
    let sheet = |s: &mut Session| run(s, CMD, json!({"sheet": "Sheet 1"}))["svg"].clone();
    let before = sheet(&mut s);
    let (wide, plate) = (WIDE_VIEWS, PLATE_VIEWS);
    // The page drawn first, so each close keeps its lines to come back, as in the window.
    assert!(sized_as(page_sizes(&mut s, views), plate));
    for (case, id, edit, sizes) in [
        (85, "parameters.change", json!({"name": "width", "expression": "100 mm"}), wide),
        (86, "parameters.change", json!({"name": "bore1_d", "expression": "12 mm"}), plate),
        (87, "timeline.delete", json!({"feature": "Bore1"}), plate),
    ] {
        in_the_model(&mut s, id, edit);
        let drawn = view_sizes(&mut s, views);
        assert!(sized_as(drawn, sizes), "case {case}: {drawn:?}");
        let paged = page_sizes(&mut s, views);
        assert!(sized_as(paged, sizes), "case {case}: the page shows {paged:?}");
        // The sheet the edit gives, and the same from the file.
        let after = sheet(&mut s);
        assert_ne!(after, before, "case {case}");
        let mut opened = reopened(&mut s, drawing, &format!("case-{case}.solidcraft"));
        assert_eq!(sheet(&mut opened), after, "case {case}: opened again");
        in_the_model(&mut s, "edit.undo", json!({}));
        assert_eq!(sheet(&mut s), before, "case {case}: undone");
        in_the_model(&mut s, "edit.redo", json!({}));
        assert_eq!(sheet(&mut s), after, "case {case}: redone");
        in_the_model(&mut s, "edit.undo", json!({}));
    }
}

/// Sheet 1 of the drawing open, exported as SVG and as a PNG at 150 dpi (A3 is 2481 x 1754 pixels:
/// 420 and 297 mm are 2480.3 and 1753.9, each side rounded outward), and how many views, dimensions
/// and notes the export lists out of date: the SVG's text and the PNG file's bytes.
fn sheet_1_exported(s: &mut Session) -> ((Value, Vec<u8>), usize) {
    let svg = run(s, CMD, json!({"sheet": "Sheet 1"}));
    let (png, bytes) = export_file(s, json!({"sheet": "Sheet 1", "format": "png", "dpi": 150}));
    assert_eq!(png["report"]["pixels"], json!([2481, 1754]), "{}", png["report"]);
    assert_eq!(svg["out_of_date"], png["out_of_date"]);
    ((svg["svg"].clone(), bytes), svg["out_of_date"].as_array().map_or(0, Vec::len))
}

/// [`sheet_1_exported`] once the deferred drawing open is updated.
fn updated(s: &mut Session) -> ((Value, Vec<u8>), usize) {
    run(s, "drawing.update", json!({}));
    sheet_1_exported(s)
}

/// Each of `dimensions` as the engine reads it now (`drawing.dimension.inspect`'s text, none when it
/// reads none) and as Sheet 1 shows it (the text `drawing.scene` sets in its group, the scene every
/// export writes; none when it isn't drawn).
fn read_and_shown(s: &mut Session, dimensions: &[Value]) -> (Vec<Option<String>>, Vec<Option<String>>) {
    let reply = run(s, "drawing.scene", json!({"sheet": "Sheet 1"}));
    let groups = reply["scene"]["groups"].as_array().unwrap();
    let shown = dimensions
        .iter()
        .map(|dimension| {
            let id = ids::decode(dimension.as_str().unwrap()).unwrap();
            let items = groups.iter().find(|g| g["object"] == json!(id))?["items"].as_array().unwrap();
            Some(items.iter().filter(|item| item["kind"] == "text").map(|item| item["text"].as_str().unwrap()).collect())
        })
        .collect();
    let read = dimensions
        .iter()
        .map(|dimension| run(s, "drawing.dimension.inspect", json!({"dimension": dimension}))["text"].as_str().map(str::to_string))
        .collect();
    (read, shown)
}

/// The M2 checkpoint (#4894 W23): the milestone plate's drawing run in order by commands alone.
/// Its steps and checks: `docs/drawing-delivery.md#m2-checkpoint-w23`.
#[test]
fn the_milestone_plate_runs_the_m2_checkpoint_by_commands_alone() {
    let (mut s, views, made) = milestone_plate();
    let drawing = s.drawing("test").unwrap().id;
    let dimensions: Vec<Value> = made.into_iter().map(|(id, ..)| id).collect();
    let texts = |given: [Option<&str>; 3]| given.map(|t| t.map(str::to_string)).to_vec();
    let plate = texts([Some("100.00"), Some("80.00"), Some("Ø10.00")]);
    let wide = texts([Some("100.00"), Some("100.00"), Some("Ø10.00")]);

    // Made, then deferred and updated: the same files.
    assert_eq!(read_and_shown(&mut s, &dimensions), (plate.clone(), plate.clone()));
    let (first, waiting) = sheet_1_exported(&mut s);
    assert!(first.0.as_str().unwrap().contains(r#"width="420mm" height="297mm""#) && waiting == 0);
    run(&mut s, "drawing.set", json!({"updates": "deferred"}));
    assert!(updated(&mut s) == (first.clone(), 0), "deferred and updated, the files changed");

    // A driving size: held, the sheet shows the plate as it was; updated, as the model is.
    in_the_model(&mut s, "parameters.change", json!({"name": "width", "expression": "100 mm"}));
    let status = run(&mut s, "drawing.status", json!({}));
    assert_eq!((&status["held"], &status["waiting"]), (&json!(true), &json!(3)), "{status}");
    assert!(sheet_1_exported(&mut s) == (first.clone(), 6), "held: the last lines, its views and dimensions out of date");
    assert_eq!(read_and_shown(&mut s, &dimensions), (wide.clone(), plate.clone()));
    let (wider, waiting) = updated(&mut s);
    assert!(waiting == 0 && wider != first, "updated, the sheet follows the model");
    let drawn = view_sizes(&mut s, views);
    assert!(sized_as(drawn, WIDE_VIEWS), "{drawn:?}");
    assert_eq!(read_and_shown(&mut s, &dimensions), (wide.clone(), wide.clone()));

    // A referenced hole deleted: lost, with its last value, drawn nowhere, and the sheet still exports.
    in_the_model(&mut s, "timeline.delete", json!({"feature": "Bore1"}));
    let (without, waiting) = updated(&mut s);
    assert!(waiting == 0 && without != wider, "the sheet without Bore1");
    let lost = run(&mut s, "drawing.dimension.inspect", json!({"dimension": dimensions[2]}));
    assert!(lost["status"] == "lost" && lost["last_good"].as_f64().is_some_and(|v| (v - 10.0).abs() < 1e-9), "{lost}");
    let gone = texts([Some("100.00"), Some("100.00"), None]);
    assert_eq!(read_and_shown(&mut s, &dimensions), (gone.clone(), gone.clone()));
    let listed = run(&mut s, CMD, json!({"sheet": "Sheet 1"}))["dimensions"].clone();
    let bore1 = listed.as_array().unwrap().iter().find(|d| d["dimension"] == dimensions[2]).unwrap();
    assert_eq!((&bore1["status"], &bore1["drawn"]), (&json!("lost"), &json!(false)), "{listed}");

    // A new bore where Bore1 was doesn't take its dimension; a repair to it on purpose does.
    let bore3 = json!({"name": "Bore3", "position": [20, 20, 20], "direction": [0, 0, -1], "diameter": 10, "type": "simple"});
    in_the_model(&mut s, "solid.hole", bore3);
    let (unrepaired, _) = updated(&mut s);
    assert_eq!(read_and_shown(&mut s, &dimensions), (gone.clone(), gone));
    let steps = s.undo.len();
    let pick = json!({"dimension": dimensions[2], "slot": "edge", "pick": {"edge": [25, 20, 20]}});
    let repaired = run(&mut s, "drawing.dimension.repair", pick);
    assert_eq!((&repaired["status"], &repaired["text"], s.undo.len()), (&json!("ok"), &json!("Ø10.00"), steps + 1), "{repaired}");
    let (fixed, waiting) = sheet_1_exported(&mut s);
    assert!(waiting == 0 && fixed != unrepaired, "the repaired dimension is drawn");
    assert_eq!(read_and_shown(&mut s, &dimensions), (wide.clone(), wide.clone()));
    run(&mut s, "edit.undo", json!({}));
    assert!(updated(&mut s) == (unrepaired, 0), "the repair undone");
    run(&mut s, "edit.redo", json!({}));
    assert!(updated(&mut s) == (fixed.clone(), 0), "the repair redone");

    // Saved, and opened in a new session: it waits for an update, then exports and reads the same.
    let mut opened = reopened(&mut s, drawing, "plate.solidcraft");
    let status = run(&mut opened, "drawing.status", json!({}));
    assert_eq!((&status["held"], &status["waiting"]), (&json!(true), &json!(3)), "opened again: {status}");
    assert!(updated(&mut opened) == (fixed, 0), "opened again and updated");
    assert_eq!(read_and_shown(&mut opened, &dimensions), (wide.clone(), wide));

    // Every step undone: the repair, then the new bore, the delete and the width on the model.
    run(&mut s, "edit.undo", json!({}));
    for _ in 0..3 {
        in_the_model(&mut s, "edit.undo", json!({}));
    }
    assert!(updated(&mut s) == (first, 0), "all undone, the first files");
    assert_eq!(read_and_shown(&mut s, &dimensions), (plate.clone(), plate));
}

/// A PDF read back: each page's MediaBox in points and its content stream, inflated, in page order.
fn pdf_pages(bytes: &[u8]) -> Vec<([f64; 4], String)> {
    use std::io::Read;
    // ASCII only, byte for byte, so its offsets are the file's.
    let text: String = bytes.iter().map(|&b| if b.is_ascii() { b as char } else { '?' }).collect();
    // Each object's number and where its text starts and ends.
    let mut objects = std::collections::BTreeMap::new();
    let mut from = 0;
    while let Some(at) = text[from..].find(" 0 obj").map(|at| from + at) {
        let number: usize = text[..at].rsplit(|c: char| !c.is_ascii_digit()).next().unwrap().parse().unwrap();
        let end = at + text[at..].find("endobj").unwrap();
        objects.insert(number, (at, end));
        from = end;
    }
    let object = |n: usize| &text[objects[&n].0..objects[&n].1];
    let stream = |n: usize| {
        let (at, end) = objects[&n];
        let start = at + text[at..end].find("stream\n").unwrap() + 7;
        let stop = start + text[start..end].find("endstream").unwrap();
        let mut out = String::new();
        flate2::read::ZlibDecoder::new(&bytes[start..stop]).read_to_string(&mut out).unwrap();
        out
    };
    let reference = |dict: &str, key: &str| -> usize {
        let at = dict.find(key).unwrap() + key.len();
        dict[at..].split_whitespace().next().unwrap().parse().unwrap()
    };
    let tree = objects.keys().copied().find(|n| object(*n).contains("/Type /Pages")).unwrap();
    let kids = object(tree);
    let kids = &kids[kids.find("/Kids [").unwrap() + 7..];
    let kids: Vec<usize> = kids[..kids.find(']').unwrap()].split(" 0 R").filter_map(|k| k.trim().parse().ok()).collect();
    assert!(object(tree).contains(&format!("/Count {}", kids.len())), "{}", object(tree));
    kids.into_iter()
        .map(|page| {
            let dict = object(page);
            assert!(dict.contains("/Type /Page"), "{dict}");
            let media = &dict[dict.find("/MediaBox [").unwrap() + 11..];
            let media: Vec<f64> = media[..media.find(']').unwrap()].split_whitespace().map(|n| n.parse().unwrap()).collect();
            ([media[0], media[1], media[2], media[3]], stream(reference(dict, "/Contents ")))
        })
        .collect()
}

/// A sheet's PDF, read back: one page, its MediaBox the sheet's paper in points, its lines vector
/// paths (moves, lines and strokes, no picture), its title block's text filled outlines. The same
/// bytes twice, by id or by name, and written to a path.
#[test]
fn a_sheet_exports_as_a_vector_pdf_at_its_paper_size() {
    let (mut s, sheet, _) = front_and_top();
    let block = serde_json::to_value(solidcraft_geom::title_block::default_block()).unwrap();
    run(&mut s, "drawing.sheet.set", json!({"sheet": ids::encode(sheet), "title_block": block}));
    assert!(refused(&mut s, json!({"sheet": ids::encode(sheet), "format": "pdf"})).contains("a PDF is written to a file: give `path`"));
    let (reply, bytes) = export_file(&mut s, json!({"sheet": ids::encode(sheet), "format": "pdf"}));
    let schema = output_schema_for::<ExportResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    assert_eq!((&reply["format"], &reply["pages"]), (&json!("pdf"), &json!([ids::encode(sheet)])), "{reply}");
    assert!(bytes.starts_with(b"%PDF-"), "a PDF");
    assert_eq!(reply["bytes"].as_u64(), Some(bytes.len() as u64));
    let pages = pdf_pages(&bytes);
    assert_eq!(pages.len(), 1);
    let size = s.drawing("test").unwrap().sheets[0].sheet.size_mm().unwrap();
    let size = [size.x, size.y];
    let points = |mm: f64| mm * 72.0 / 25.4;
    let ([x0, y0, x1, y1], content) = &pages[0];
    assert_eq!([*x0, *y0], [0.0, 0.0]);
    assert!((x1 - points(size[0])).abs() < 1e-3 && (y1 - points(size[1])).abs() < 1e-3, "{:?} for {size:?} mm", pages[0].0);
    // Millimetres scaled to points once, so the page's numbers are the sheet's.
    assert!(content.starts_with(&format!("q {:.9} 0 0 {:.9} 0 0 cm", 72.0 / 25.4, 72.0 / 25.4)), "{content}");
    let ops: Vec<&str> = content.split_whitespace().filter(|t| t.chars().all(|c| c.is_ascii_alphabetic() || c == '*')).collect();
    for op in ["m", "l", "S", "Tj"] {
        assert!(ops.contains(&op), "{op} in {content}");
    }
    assert!(ops.iter().filter(|op| **op == "Tj").count() > 12, "each title block label and value set as text");
    assert!(!ops.contains(&"f"), "no glyph outline is filled: the text is real text");
    let file = String::from_utf8_lossy(&bytes);
    assert!(file.contains("/Subtype /Type3") && file.contains("/ToUnicode"), "an embedded font that maps back to Unicode");
    // The same sheet asked for outlines: the old file, whose text is filled outlines and no font.
    let (_, old) = export_file(&mut s, json!({"sheet": ids::encode(sheet), "format": "pdf", "text": "outlines"}));
    let old_content = &pdf_pages(&old)[0].1;
    assert!(old_content.split_whitespace().filter(|t| *t == "f").count() > 12, "each title block label and value filled");
    assert!(!old_content.contains("Tj") && !String::from_utf8_lossy(&old).contains("/Type3"), "outlines carry no text");
    assert!(refused(&mut s, json!({"sheet": ids::encode(sheet), "format": "svg", "text": "outlines"})).contains("`text` is for pdf only"));
    assert!(!content.contains(" Do"), "lines only: no picture");
    let name = s.drawing("test").unwrap().sheets[0].name.clone();
    assert_eq!(exported(&mut s, json!({"sheet": name, "format": "pdf"})), bytes, "the same bytes by name");
    let folder = Folder::new("drawing-export-pdf");
    let path = folder.path("Sheet 1.PDF");
    let written = run(&mut s, CMD, json!({"sheet": ids::encode(sheet), "format": "pdf", "path": path}));
    assert_eq!((written["path"].as_str(), written.get("data")), (Some(path.as_str()), None), "{written}");
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

/// A whole drawing's PDF: one page per sheet in the drawing's order, or in the order `sheets`
/// gives, each page its own sheet's size; a page with views draws their lines and an empty one
/// draws its border only.
#[test]
fn a_drawing_exports_as_one_pdf_page_per_sheet() {
    let (mut s, first, _) = front_and_top();
    let added = run(&mut s, "drawing.sheet.add", json!({"size": {"standard": "a4"}, "orientation": "portrait"}));
    let second = ids::decode(added["sheet"].as_str().unwrap()).unwrap();
    let (all, bytes) = export_file(&mut s, json!({"format": "pdf"}));
    assert_eq!(all["pages"], json!([ids::encode(first), ids::encode(second)]), "{all}");
    let pages = pdf_pages(&bytes);
    assert_eq!(pages.len(), 2);
    let d = s.drawing("test").unwrap().clone();
    for ((media, _), sheet) in pages.iter().zip(&d.sheets) {
        let size = sheet.sheet.size_mm().unwrap();
        let [w, h] = [size.x, size.y];
        assert!((media[2] - w * 72.0 / 25.4).abs() < 1e-3 && (media[3] - h * 72.0 / 25.4).abs() < 1e-3, "{media:?} for {w} x {h} mm");
    }
    assert_eq!(pages[1].0.map(|n| (n * 25.4 / 72.0).round()), [0.0, 0.0, 210.0, 297.0], "the second sheet's own size");
    assert_ne!(pages[0].0, pages[1].0, "a page the second sheet's size after the first sheet's");
    let strokes = |content: &str| content.split_whitespace().filter(|t| *t == "S" || *t == "s").count();
    assert!(strokes(&pages[0].1) > strokes(&pages[1].1), "the views' lines on the first page only");
    // Chosen sheets, in the order given.
    let (chosen, bytes) = export_file(&mut s, json!({"format": "pdf", "sheets": [ids::encode(second), d.sheets[0].name]}));
    assert_eq!(chosen["pages"], json!([ids::encode(second), ids::encode(first)]), "{chosen}");
    let reversed = pdf_pages(&bytes);
    assert_eq!((reversed[0].0, reversed[1].0), (pages[1].0, pages[0].0));
    // Each sheet's views are reported, the first page's first.
    assert_eq!(chosen["views"].as_array().map(Vec::len), all["views"].as_array().map(Vec::len));
}

/// A shaded view's picture is in its sheet's PDF as an image XObject drawn on the page, the same
/// bytes twice.
#[test]
fn a_shaded_view_is_embedded_in_the_pdf() {
    let (mut s, sheet, [front, _]) = front_and_top();
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"visual_style": "shaded"}}));
    let bytes = exported(&mut s, json!({"sheet": ids::encode(sheet), "format": "pdf"}));
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/Subtype /Image"), "an image XObject");
    let pages = pdf_pages(&bytes);
    assert_eq!(pages[0].1.matches(" Do").count(), 2, "the front view's picture, and the top view's, which follows it");
    assert_eq!(exported(&mut s, json!({"sheet": ids::encode(sheet), "format": "pdf"})), bytes, "the same bytes twice");
}

/// A DXF read back: each entity's type, its layer (group 8) and its own weight (group 370), in
/// file order, and each layer's name, linetype and weight from the LAYER table.
fn dxf_entities(text: &str) -> (Vec<(String, String, Option<i32>)>, Vec<(String, String, i32)>) {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let pairs: Vec<(&str, &str)> = lines.chunks(2).filter_map(|c| Some((c[0], *c.get(1)?))).collect();
    let (mut entities, mut layers) = (Vec::new(), Vec::new());
    let mut section = "";
    let mut i = 0;
    while i < pairs.len() {
        let (code, value) = pairs[i];
        if code == "2" && i > 0 && pairs[i - 1] == ("0", "SECTION") {
            section = value;
        }
        if pairs[i] == ("0", "ENDSEC") {
            section = "";
        }
        if code == "0" && (section == "ENTITIES" && value != "ENDSEC" || section == "TABLES" && value == "LAYER") {
            let kind = value.to_string();
            let mut j = i + 1;
            let (mut name, mut layer, mut linetype, mut weight) = (String::new(), String::new(), String::new(), None);
            while j < pairs.len() && pairs[j].0 != "0" {
                match pairs[j] {
                    ("2", v) => name = v.to_string(),
                    ("8", v) => layer = v.to_string(),
                    ("6", v) => linetype = v.to_string(),
                    ("370", v) => weight = v.parse().ok(),
                    _ => {}
                }
                j += 1;
            }
            if section == "ENTITIES" {
                entities.push((kind, layer, weight));
            } else if !name.is_empty() {
                layers.push((name, linetype, weight.unwrap_or(-3)));
            }
            i = j;
            continue;
        }
        i += 1;
    }
    (entities, layers)
}

/// A sheet's DXF: its views' visible edges on VISIBLE at the wide line's weight (0.5 mm, 50), its
/// hidden edges on HIDDEN with the HIDDEN linetype at the narrow line's (0.25 mm, 25), the title
/// block's lines and text on their layers, the borders too. In the reply as text, or written to a
/// path, the same bytes; a shaded view's picture is left out and counted.
#[test]
fn a_sheet_exports_as_dxf_with_a_layer_for_each_kind_of_line() {
    let (mut s, sheet, [front, top]) = front_and_top();
    let drawing = ids::encode(s.open_drawing.unwrap());
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 10, "height": 10, "corner": [30, 40, 5], "operation": "cut", "targets": ["Block"]}));
    run(&mut s, "drawing.open", json!({"drawing": drawing}));
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(front), "set": {"visual_style": "visible_hidden"}}));
    let block = serde_json::to_value(solidcraft_geom::title_block::default_block()).unwrap();
    run(&mut s, "drawing.sheet.set", json!({"sheet": ids::encode(sheet), "title_block": block}));
    let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet), "format": "dxf"}));
    let schema = output_schema_for::<ExportResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    assert_eq!((&reply["format"], reply.get("svg"), reply.get("data")), (&json!("dxf"), None, None), "{reply}");
    assert_eq!(reply["report"]["pictures_left_out"], json!(0));
    let text = reply["dxf"].as_str().unwrap();
    assert!(text.contains("$INSUNITS") && text.ends_with("EOF\n"), "a whole DXF in millimetres");
    let (entities, layers) = dxf_entities(text);
    let on = |layer: &str| entities.iter().filter(|(_, l, _)| l == layer).count();
    let layer = |name: &str| layers.iter().find(|(n, ..)| n == name).cloned();
    assert_eq!(layer("VISIBLE"), Some(("VISIBLE".into(), "Continuous".into(), 50)), "{layers:?}");
    assert_eq!(layer("HIDDEN"), Some(("HIDDEN".into(), "HIDDEN".into(), 25)), "{layers:?}");
    assert!(layer("TEXT").is_some() && layer("THIN").is_some(), "{layers:?}");
    assert!(on("VISIBLE") > 8 && on("HIDDEN") > 0 && on("TEXT") > 12 && on("THIN") > 0, "{:?}", layers);
    assert!(entities.iter().filter(|(_, l, _)| l == "HIDDEN").all(|(_, _, w)| *w == Some(25)), "hidden edges at the narrow line");
    assert!(entities.iter().all(|(_, l, _)| !l.is_empty() && l != "0"), "everything on a named layer");
    let folder = Folder::new("drawing-export-dxf");
    let path = folder.path("Sheet 1.DXF");
    let written = run(&mut s, CMD, json!({"sheet": ids::encode(sheet), "format": "dxf", "path": path}));
    assert_eq!((written["path"].as_str(), written.get("dxf")), (Some(path.as_str()), None), "{written}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    // A shaded view's lines become its picture, which a DXF can't hold.
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"visual_style": "shaded"}}));
    let shaded = run(&mut s, CMD, json!({"sheet": ids::encode(sheet), "format": "dxf"}));
    assert_eq!(shaded["report"]["pictures_left_out"], json!(1), "{}", shaded["report"]);
}

/// `drawing.print` is the whole drawing's PDF, as `drawing.export` makes it with no sheet named:
/// every sheet a page, asking to print at actual size (an export doesn't), written to its path.
/// The app's own print takes the same bytes in-process (`print_pdf`); the command never puts them
/// in its reply.
#[test]
fn print_is_the_whole_drawings_pdf() {
    let (mut s, first, _) = front_and_top();
    let added = run(&mut s, "drawing.sheet.add", json!({}));
    let second = added["sheet"].clone();
    let folder = Folder::new("drawing-print");
    let path = folder.path("Drawing.pdf");
    let printed = run(&mut s, "drawing.print", json!({"path": path}));
    let bytes = std::fs::read(&path).unwrap();
    let export = exported(&mut s, json!({"format": "pdf"}));
    assert!(!String::from_utf8_lossy(&export).contains("PrintScaling"), "an export leaves printing to the viewer");
    assert_eq!(printed["pages"], json!([ids::encode(first), second]), "{printed}");
    assert!(String::from_utf8_lossy(&bytes).contains("/PrintScaling /None"));
    assert_eq!(super::print_pdf(&mut s).unwrap(), bytes, "the app's print: the same bytes");
    assert!(s.execute("drawing.print", &json!({})).unwrap_err().to_string().contains("give `path`"));
    for (p, why) in [(json!({"path": folder.path("Drawing.svg")}), "path must be a .pdf file"), (json!({"sheet": "Sheet 1"}), "sheet")] {
        let message = s.execute("drawing.print", &p).unwrap_err().to_string();
        assert!(message.contains(why), "{p}: {message}");
    }
}

/// A plate 10 mm thick with an `nx` x `nz` grid of Ø4 through holes on 6 mm centres, 7 mm in from its
/// edges (4 x 3 holes: 32 x 26 mm, #212), drawn from the front at 1:1 on A3 with no title block: the
/// session and its sheet.
fn plate_with_a_grid_of_holes(nx: usize, nz: usize) -> (Session, u64) {
    let mut s = Session::default();
    let (pitch, start) = (6.0, 7.0);
    let length = start * 2.0 + pitch * (nx as f64 - 1.0);
    let height = start * 2.0 + pitch * (nz as f64 - 1.0);
    run(&mut s, "solid.box", json!({"length": length, "width": 10, "height": height, "body_name": "Block"}));
    for i in 0..nx {
        for j in 0..nz {
            let (x, z) = (start + pitch * i as f64, start + pitch * j as f64);
            let cut = json!({"radius": 2.0, "height": 12, "base": [x, -1, z], "axis": "Y", "operation": "cut", "targets": ["Block"]});
            run(&mut s, "solid.cylinder", cut);
        }
    }
    run(&mut s, "drawing.create", json!({"omit_first_sheet_title_block": true}));
    run(&mut s, "drawing.view.base", json!({"sheet": "Sheet 1", "scale": "1:1"}));
    let sheet = s.drawing("test").unwrap().sheets[0].id;
    (s, sheet)
}

/// A grid of round holes exports to DXF (#212): each hole's outline is one turn, however the rounding
/// of its pieces' sweeps adds up, so it is written as a CIRCLE, and the DXF reads back, by both
/// readers, with every arc in it less than a turn.
#[test]
fn a_plate_with_a_grid_of_holes_exports_as_dxf() {
    for (nx, nz) in [(4, 3), (1, 1), (2, 2), (3, 3)] {
        let (mut s, sheet) = plate_with_a_grid_of_holes(nx, nz);
        let reply = s.execute(CMD, &json!({"sheet": ids::encode(sheet), "format": "dxf"}));
        let reply = reply.unwrap_or_else(|e| panic!("{nx} x {nz} holes: {e}"));
        let text = reply["dxf"].as_str().unwrap();
        let file = solidcraft_io::read_dxf_file(text).unwrap_or_else(|e| panic!("{nx} x {nz} holes: {e}"));
        // Every arc as written: an ARC's ends apart, a bulge at most a half turn (|bulge| <= 1); each
        // hole, a whole turn, a CIRCLE.
        let (mut round, mut circles) = (0, 0);
        for e in file.entities() {
            match e.geometry() {
                solidcraft_io::DxfShape::Circle { r, .. } => {
                    assert!((r - 2.0).abs() < 1e-9, "{nx} x {nz} holes: circle of {r}");
                    circles += 1;
                }
                solidcraft_io::DxfShape::Arc { a0, a1, .. } => {
                    let sweep = (a1 - a0).rem_euclid(360.0);
                    assert!(sweep.is_finite() && sweep > 0.0 && sweep < 360.0, "{nx} x {nz} holes: arc {a0}..{a1}");
                    round += 1;
                }
                solidcraft_io::DxfShape::BulgedPolyline { vertices, .. } => {
                    for v in vertices {
                        assert!(v.bulge.is_finite() && v.bulge.abs() <= 1.0, "{nx} x {nz} holes: bulge {}", v.bulge);
                        round += usize::from(v.bulge != 0.0);
                    }
                }
                _ => {}
            }
        }
        assert_eq!((circles, round), (nx * nz, 0), "{nx} x {nz} holes: circles and other round pieces");
        // The sketch reader's view of the same file: every arc's ends on its circle and apart.
        let read = solidcraft_io::read_dxf(text).unwrap_or_else(|e| panic!("{nx} x {nz} holes: {e}"));
        let mut holes = 0;
        for g in &read {
            match g {
                solidcraft_io::Geom2::Circle(_, r) => holes += usize::from((r - 2.0).abs() < 1e-6),
                solidcraft_io::Geom2::Arc { c, a, b } => {
                    assert!(a.dist(*b) > 1e-9 && (a.dist(*c) - b.dist(*c)).abs() < 1e-6, "{nx} x {nz} holes: {g:?}");
                }
                _ => {}
            }
        }
        assert_eq!(holes, nx * nz, "{nx} x {nz} holes: {read:?}");
    }
}
