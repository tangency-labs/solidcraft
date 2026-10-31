use serde_json::{Value, json};
use solidcraft_doc::MAX_PLACEMENT_DEPTH;
use solidcraft_doc::canvas::base64_decode;
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
    solidcraft_io::paper::paper_svg(&sheet_scene(s, sheet).prepare().unwrap()).unwrap().0
}

/// The PNG of sheet `sheet` of the drawing open, written straight from its built scene.
fn scene_png(s: &Session, sheet: u64, options: PngOptions) -> Vec<u8> {
    let mut png = Vec::new();
    paper_png(&sheet_scene(s, sheet).prepare().unwrap(), options, &mut png).unwrap();
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
        assert_eq!(reply["report"]["pixels"], json!([413, 292]), "{colour}");
    }
    let folder = Folder::new("drawing-export-png");
    let path = folder.path("Sheet 1.PNG");
    let reply = run(&mut s, CMD, json!({"sheet": id, "format": "png", "path": path, "dpi": 2400.0 / 32.0}));
    let written = std::fs::read(&path).unwrap();
    assert_eq!(written, scene_png(&s, sheet, PngOptions { dpi: 75.0, colour: PngColour::Grey }));
    assert_eq!((reply["path"].as_str(), reply.get("data"), reply["bytes"].as_u64()), (Some(path.as_str()), None, Some(written.len() as u64)));
}

/// A document held for the reply takes up to its limit and is refused past it, naming the way out.
#[test]
fn a_reply_holds_a_document_up_to_its_limit() {
    let (s, sheet, _) = front_and_top();
    let options = PngOptions { dpi: 25.0, colour: PngColour::Grey };
    let png = scene_png(&s, sheet, options);
    let mut exact = Held { bytes: Vec::new(), limit: png.len() };
    paper_png(&sheet_scene(&s, sheet).prepare().unwrap(), options, &mut exact).unwrap();
    assert_eq!(exact.bytes, png);
    let mut short = Held { bytes: Vec::new(), limit: png.len() - 1 };
    let why = paper_png(&sheet_scene(&s, sheet).prepare().unwrap(), options, &mut short).unwrap_err().to_string();
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
        (json!({"sheet": id, "job": true}), "`job` is for a png written to a path"),
        (json!({"sheet": id, "path": folder.path("sheet.svg"), "job": true}), "`job` is for a png written to a path"),
        (json!({"sheet": id, "format": "png", "job": true}), "`job` is for a png written to a path"),
        (json!({"sheet": id, "format": "png", "path": folder.path("sheet.svg"), "job": true}), "path must be a .png file"),
        (json!({"sheet": id, "format": "png", "path": folder.path("sheet.png"), "job": "yes"}), "job"),
    ] {
        let message = refused(&mut s, p.clone());
        assert!(message.contains(why), "{p}: {message}");
    }
    assert_eq!(std::fs::read_dir(&folder.0).unwrap().count(), 0);
    run(&mut s, "drawing.close", json!({}));
    assert!(refused(&mut s, json!({"sheet": id})).contains("open a drawing first"));
}

/// A sheet's PNG written as a job: answered at once in a render job's shape, then a band a step,
/// the rows written only going up. Nothing is at the path until the last band, and then the file
/// is the PNG written at once, the reply it ends with the one for that file.
#[test]
fn a_png_written_as_a_job_is_the_png_written_at_once() {
    let (mut s, sheet, _) = front_and_top();
    let id = ids::encode(sheet);
    let folder = Folder::new("drawing-export-job");
    let path = folder.path("sheet.png");
    let reply = run(&mut s, CMD, json!({"sheet": id, "format": "png", "path": path, "job": true}));
    let schema = output_schema_for::<ExportReply>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    let job = reply["job"].as_u64().unwrap();
    assert_eq!(
        (&reply["progress"], &reply["error"], reply.get("result")),
        (&json!({"phase": "running", "done": 0, "total": 3508}), &Value::Null, None)
    );
    let mut done = Vec::new();
    while crate::drawing_export::step(&mut s) {
        assert!(!std::path::Path::new(&path).exists(), "nothing at the path until the last band");
        done.push(run(&mut s, "drawing.export.status", json!({"job": job}))["progress"]["done"].as_u64().unwrap());
    }
    assert!(done.len() > 1 && done.windows(2).all(|w| w[0] < w[1]), "{done:?}");
    assert!(!crate::drawing_export::step(&mut s), "an ended job takes no more steps");
    let status = run(&mut s, "drawing.export.status", json!({"job": job}));
    let schema = output_schema_for::<ExportJobStatus>();
    assert!(conforms(schema.as_value(), schema.as_value(), &status), "{status}");
    assert_eq!(status["progress"], json!({"phase": "completed", "done": 3508, "total": 3508}));
    assert_eq!(run(&mut s, "drawing.export.cancel", json!({"job": job})), status, "a completed job stays completed");
    assert_eq!(std::fs::read(&path).unwrap(), scene_png(&s, sheet, PngOptions::default()));
    // The same answer, apart from its path and what working out each view took.
    let answer = |mut v: Value| {
        v["path"] = Value::Null;
        for view in v["views"].as_array_mut().into_iter().flatten() {
            view.as_object_mut().map(|view| view.remove("stats"));
        }
        v
    };
    let at_once = run(&mut s, CMD, json!({"sheet": id, "format": "png", "path": folder.path("at once.png")}));
    assert_eq!(answer(status["result"].clone()), answer(at_once), "the at-once reply, but for its path");
    assert_eq!(std::fs::read_dir(&folder.0).unwrap().count(), 2, "no temporary file left");
}

/// A job cancelled part way, or unable to put its file in place, leaves what is at the path as it
/// was and no temporary file; it is answered as it ended and takes no more steps. A second job is
/// refused while one is under way, and only the latest job answers.
#[test]
fn a_cancelled_or_failed_job_leaves_the_path_as_it_was() {
    let (mut s, sheet, _) = front_and_top();
    let id = ids::encode(sheet);
    let folder = Folder::new("drawing-export-job-ended");
    let path = folder.path("sheet.png");
    std::fs::write(&path, b"before").unwrap();
    let start = json!({"sheet": id, "format": "png", "path": path, "job": true});
    let job = run(&mut s, CMD, start.clone())["job"].as_u64().unwrap();
    assert!(crate::drawing_export::step(&mut s));
    let why = refused(&mut s, start);
    assert!(why.contains(&format!("an export is under way (job {job})")), "{why}");
    let cancelled = run(&mut s, "drawing.export.cancel", json!({"job": job}));
    assert_eq!(cancelled["progress"]["phase"], "cancelled", "{cancelled}");
    assert_eq!(run(&mut s, "drawing.export.cancel", json!({"job": job})), cancelled, "an ended job stays as it ended");
    assert!(!crate::drawing_export::step(&mut s));
    assert_eq!(std::fs::read(&path).unwrap(), b"before");
    assert_eq!(std::fs::read_dir(&folder.0).unwrap().count(), 1, "no temporary file left");

    // A folder at the path: the file written can't take its place.
    let blocked = folder.path("blocked.png");
    std::fs::create_dir(&blocked).unwrap();
    let job = run(&mut s, CMD, json!({"sheet": id, "format": "png", "path": blocked, "dpi": 25, "job": true}))["job"].as_u64().unwrap();
    while crate::drawing_export::step(&mut s) {}
    let failed = run(&mut s, "drawing.export.status", json!({"job": job}));
    assert_eq!(
        (&failed["progress"]["phase"], failed["error"]["message"].is_string(), failed.get("result")),
        (&json!("failed"), true, None),
        "{failed}"
    );
    assert!(std::path::Path::new(&blocked).is_dir());
    assert_eq!(std::fs::read_dir(&folder.0).unwrap().count(), 2, "no temporary file left");
    for (cmd, other) in [("drawing.export.status", job - 1), ("drawing.export.cancel", job + 1)] {
        let why = s.execute(cmd, &json!({"job": other})).unwrap_err().to_string();
        assert!(why.contains(&format!("no export job {other}")), "{why}");
    }
}
