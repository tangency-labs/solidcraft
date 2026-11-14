use serde_json::{Value, json};
use solidcraft_doc::MAX_PLACEMENT_DEPTH;
use solidcraft_doc::canvas::base64_decode;
use solidcraft_geom::paper::NoPictures;
use solidcraft_io::paper::{PngColour, paper_png};

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
    let reply = run(&mut s, CMD, json!({"sheet": ids::encode(sheet), "format": "png", "dpi": 50}));
    assert_eq!(reply["data"].as_str().and_then(base64_decode), Some(scene_png(&s, sheet, PngOptions { dpi: 50.0, colour: PngColour::Grey })));
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
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"visual_style": "shaded_visible"}}));
    let why = refused(&mut s, json!({"sheet": ids::encode(sheet), "path": path}));
    assert!(why.contains(&format!("views on the sheet failed: {} (not supported yet: shaded views with lines", ids::encode(top))), "{why}");
    assert!(!why.contains(&ids::encode(front)) && !std::path::Path::new(&path).exists(), "{why}");
    run(&mut s, "drawing.view.edit", json!({"view": ids::encode(top), "set": {"visual_style": "visible"}}));
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
    assert_eq!((reply.get("svg"), reply.get("path"), &reply["format"]), (None, None, &json!("png")));
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
    assert_eq!(reply["svg"].as_str().map(str::as_bytes), Some(plain.as_slice()));
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
        let text = reply["svg"].as_str().unwrap();
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

/// A sheet with shaded views exports their pictures as `<image>`s, the same bytes twice, and draws
/// them in a PNG. A picture past the grid's limit is refused and nothing is written.
#[test]
fn a_shaded_view_exports_its_picture() {
    let (mut s, sheet, [front, _]) = front_and_top();
    let png = |s: &mut Session| run(s, CMD, json!({"sheet": ids::encode(sheet), "format": "png", "dpi": 25}))["data"].clone();
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

/// The drawing milestone's plate (acceptance case 84, #4894) by commands alone: modelled from its
/// parameters, a top view at 1:2 and the views projected below and right of it, its length, width
/// and first bore dimensioned. With the ids of the top, below and right views, and each dimension's
/// id, value and text.
fn milestone_plate() -> (Session, [u64; 3], Vec<(Value, Value, Value)>) {
    let mut s = Session::default();
    let script: Value = serde_json::from_str(include_str!("../../../../examples/acceptance/drawing_m2.json")).unwrap();
    let made = s.run_script(&script).unwrap();
    let top = made.last().unwrap();
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

/// `id` with `p` run on the model, with the drawing `drawing` closed as a model edit has it, then
/// opened again.
fn on_the_model(s: &mut Session, drawing: &str, id: &str, p: Value) {
    run(s, "drawing.close", json!({}));
    run(s, id, p);
    run(s, "drawing.open", json!({"drawing": drawing}));
}

/// The plate's model edited with its drawing closed (acceptance cases 85 to 87, #4894): each view's
/// lines follow with the hand sizes, in the export and on the window's page, and each edit saved and
/// opened again exports the same SVG. Undo brings back the sheet before it byte for byte, and redo
/// the one after it. The edited model's own geometry is oracle cases 85 to 87's (tests/oracle), and
/// what the dimensions read across these edits is in drawing_dimension_tests.rs (#8288).
#[test]
fn the_milestone_plate_follows_its_model_and_undo_brings_its_sheet_back() {
    let (mut s, views, _) = milestone_plate();
    let drawing = ids::encode(s.drawing("test").unwrap().id);
    let sheet = |s: &mut Session| run(s, CMD, json!({"sheet": "Sheet 1"}))["svg"].clone();
    let folder = Folder::new("drawing-m2-edits");
    let before = sheet(&mut s);
    let (wide, plate) = (WIDE_VIEWS, PLATE_VIEWS);
    // The page drawn first, so each close keeps its lines to come back, as in the window.
    assert!(sized_as(page_sizes(&mut s, views), plate));
    for (case, id, edit, sizes) in [
        (85, "parameters.change", json!({"name": "width", "expression": "100 mm"}), wide),
        (86, "parameters.change", json!({"name": "bore1_d", "expression": "12 mm"}), plate),
        (87, "timeline.delete", json!({"feature": "Bore1"}), plate),
    ] {
        on_the_model(&mut s, &drawing, id, edit);
        let drawn = view_sizes(&mut s, views);
        assert!(sized_as(drawn, sizes), "case {case}: {drawn:?}");
        let paged = page_sizes(&mut s, views);
        assert!(sized_as(paged, sizes), "case {case}: the page shows {paged:?}");
        // The sheet the edit gives, and the same from the file.
        let after = sheet(&mut s);
        assert_ne!(after, before, "case {case}");
        let path = folder.path(&format!("case-{case}.solidcraft"));
        run(&mut s, "file.save", json!({"path": path}));
        let mut opened = Session::default();
        run(&mut opened, "doc.open", json!({"path": path}));
        run(&mut opened, "drawing.open", json!({"drawing": drawing}));
        assert_eq!(sheet(&mut opened), after, "case {case}: opened again");
        on_the_model(&mut s, &drawing, "edit.undo", json!({}));
        assert_eq!(sheet(&mut s), before, "case {case}: undone");
        on_the_model(&mut s, &drawing, "edit.redo", json!({}));
        assert_eq!(sheet(&mut s), after, "case {case}: redone");
        on_the_model(&mut s, &drawing, "edit.undo", json!({}));
    }
}

/// Sheet 1 of the drawing open, exported as SVG and as a PNG at 150 dpi (A3 is 2481 x 1754 pixels:
/// 420 and 297 mm are 2480.3 and 1753.9, each side rounded outward), and how many views, dimensions
/// and notes the export lists out of date.
fn exported(s: &mut Session) -> ([Value; 2], usize) {
    let svg = run(s, CMD, json!({"sheet": "Sheet 1"}));
    let png = run(s, CMD, json!({"sheet": "Sheet 1", "format": "png", "dpi": 150}));
    assert_eq!(png["report"]["pixels"], json!([2481, 1754]), "{}", png["report"]);
    assert_eq!(svg["out_of_date"], png["out_of_date"]);
    ([svg["svg"].clone(), png["data"].clone()], svg["out_of_date"].as_array().map_or(0, Vec::len))
}

/// [`exported`] once the deferred drawing open is updated.
fn updated(s: &mut Session) -> ([Value; 2], usize) {
    run(s, "drawing.update", json!({}));
    exported(s)
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

/// The M2 checkpoint (#4894 W23): the milestone plate's drawing made and checked end to end by
/// commands alone, in one session, in the checkpoint's order. At each step each dimension's text as
/// the engine reads it is the text the sheet shows, and the SVG and PNG are compared with the steps
/// before.
/// - Made: A3 is 420 x 297 mm in the SVG and 2481 x 1754 pixels in the PNG at 150 dpi. Deferred and
///   updated, the drawing exports the same bytes.
/// - The width, a driving size, changed to 100 with the drawing closed: held, the sheet exports the
///   same bytes and lists its three views and three dimensions out of date, showing 80.00 where the
///   engine reads 100.00. Updated, the views take case 85's sizes and the sheet shows 100.00.
/// - Bore1, which a dimension references, deleted: the dimension is lost with its last value 10 and
///   is drawn nowhere, and the sheet still exports. A new bore where Bore1 was doesn't take it;
///   repaired to the new bore on purpose, it reads Ø10.00 again, as one undo step. The repair undone
///   and redone, the sheet is each time the one it was.
/// - Saved and opened in a new session: the drawing waits for an update, as its setting is saved and
///   its last update isn't; updated, it exports the same bytes and reads the same.
/// - Every step undone and the drawing updated, it exports the first bytes again.
#[test]
fn the_milestone_plate_runs_the_m2_checkpoint_by_commands_alone() {
    let (mut s, views, made) = milestone_plate();
    let drawing = ids::encode(s.drawing("test").unwrap().id);
    let dimensions: Vec<Value> = made.into_iter().map(|(id, ..)| id).collect();
    let texts = |given: [Option<&str>; 3]| given.map(|t| t.map(str::to_string)).to_vec();
    let plate = texts([Some("100.00"), Some("80.00"), Some("Ø10.00")]);
    let wide = texts([Some("100.00"), Some("100.00"), Some("Ø10.00")]);

    // Made, then deferred and updated: the same files.
    assert_eq!(read_and_shown(&mut s, &dimensions), (plate.clone(), plate.clone()));
    let (first, waiting) = exported(&mut s);
    assert!(first[0].as_str().unwrap().contains(r#"width="420mm" height="297mm""#) && waiting == 0);
    run(&mut s, "drawing.set", json!({"updates": "deferred"}));
    assert!(updated(&mut s) == (first.clone(), 0), "deferred and updated, the files changed");

    // A driving size: held, the sheet shows the plate as it was; updated, as the model is.
    on_the_model(&mut s, &drawing, "parameters.change", json!({"name": "width", "expression": "100 mm"}));
    let status = run(&mut s, "drawing.status", json!({}));
    assert_eq!((&status["held"], &status["waiting"]), (&json!(true), &json!(3)), "{status}");
    assert!(exported(&mut s) == (first.clone(), 6), "held: the last lines, its views and dimensions out of date");
    assert_eq!(read_and_shown(&mut s, &dimensions), (wide.clone(), plate.clone()));
    let (wider, waiting) = updated(&mut s);
    assert!(waiting == 0 && wider != first, "updated, the sheet follows the model");
    let drawn = view_sizes(&mut s, views);
    assert!(sized_as(drawn, WIDE_VIEWS), "{drawn:?}");
    assert_eq!(read_and_shown(&mut s, &dimensions), (wide.clone(), wide.clone()));

    // A referenced hole deleted: lost, with its last value, drawn nowhere, and the sheet still exports.
    on_the_model(&mut s, &drawing, "timeline.delete", json!({"feature": "Bore1"}));
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
    on_the_model(&mut s, &drawing, "solid.hole", bore3);
    let (unrepaired, _) = updated(&mut s);
    assert_eq!(read_and_shown(&mut s, &dimensions), (gone.clone(), gone));
    let steps = s.undo.len();
    let pick = json!({"dimension": dimensions[2], "slot": "edge", "pick": {"edge": [25, 20, 20]}});
    let repaired = run(&mut s, "drawing.dimension.repair", pick);
    assert_eq!((&repaired["status"], &repaired["text"], s.undo.len()), (&json!("ok"), &json!("Ø10.00"), steps + 1), "{repaired}");
    let (fixed, waiting) = exported(&mut s);
    assert!(waiting == 0 && fixed != unrepaired, "the repaired dimension is drawn");
    assert_eq!(read_and_shown(&mut s, &dimensions), (wide.clone(), wide.clone()));
    run(&mut s, "edit.undo", json!({}));
    assert!(updated(&mut s) == (unrepaired, 0), "the repair undone");
    run(&mut s, "edit.redo", json!({}));
    assert!(updated(&mut s) == (fixed.clone(), 0), "the repair redone");

    // Saved, and opened in a new session: it waits for an update, then exports and reads the same.
    let folder = Folder::new("drawing-m2-checkpoint");
    run(&mut s, "file.save", json!({"path": folder.path("plate.solidcraft")}));
    let mut opened = Session::default();
    run(&mut opened, "doc.open", json!({"path": folder.path("plate.solidcraft")}));
    run(&mut opened, "drawing.open", json!({"drawing": drawing}));
    let status = run(&mut opened, "drawing.status", json!({}));
    assert_eq!((&status["held"], &status["waiting"]), (&json!(true), &json!(3)), "opened again: {status}");
    assert!(updated(&mut opened) == (fixed, 0), "opened again and updated");
    assert_eq!(read_and_shown(&mut opened, &dimensions), (wide.clone(), wide));

    // Every step undone: the repair, then the new bore, the delete and the width on the model.
    run(&mut s, "edit.undo", json!({}));
    for _ in 0..3 {
        on_the_model(&mut s, &drawing, "edit.undo", json!({}));
    }
    assert!(updated(&mut s) == (first, 0), "all undone, the first files");
    assert_eq!(read_and_shown(&mut s, &dimensions), (plate.clone(), plate));
}
