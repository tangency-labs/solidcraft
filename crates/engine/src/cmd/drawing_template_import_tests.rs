use serde_json::json;

use super::*;
use crate::schema::tests::conforms;
use crate::tests::Folder;

/// A sheet format another program could write: an A4 landscape page in millimetres, a frame 10 mm
/// in, a title block's lines, a hole mark, a company name, a title FreeCAD marks editable and a
/// number marked with `data-field`. Written by hand from the SVG specification.
const A4_FORMAT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:freecad="https://www.freecad.org/wiki/index.php?title=Svg_Namespace"
     width="297mm" height="210mm" viewBox="0 0 297 210">
  <rect x="10" y="10" width="277" height="190" fill="none" stroke="black"/>
  <line x1="187" y1="170" x2="287" y2="170" stroke="black"/>
  <line x1="187" y1="170" x2="187" y2="200" stroke="black"/>
  <circle cx="20" cy="20" r="3"/>
  <text x="190" y="178" font-size="5">ACME LTD</text>
  <text x="190" y="188" font-size="5" freecad:editable="title"><tspan>Bracket</tspan></text>
  <text x="240" y="196" font-size="3.5" data-field="drawing_number">D-0001</text>
</svg>"#;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn refused(s: &mut Session, p: Value) -> String {
    s.execute(IMPORT, &p).err().map(|e| e.to_string()).unwrap_or_else(|| panic!("accepted: {p}"))
}

/// An SVG sheet format becomes a template: an A4 landscape sheet with no border or block of its own,
/// whose format draws the file's lines, circle and texts on the paper, its fields listed. A drawing
/// made from the file has that format, and one restyled by it takes it.
#[test]
fn an_svg_sheet_format_becomes_a_template() {
    let folder = Folder::new("drawing-template-import");
    let (file, path) = (folder.path("acme-a4.svg"), folder.path("acme-a4.json"));
    std::fs::write(&file, A4_FORMAT).unwrap();
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 10}));
    let dry = run(&mut s, IMPORT, json!({"file": file, "dry_run": true}));
    assert!(dry.get("path").is_none() && !std::path::Path::new(&path).exists(), "a dry run writes nothing: {dry}");
    let reply = run(&mut s, IMPORT, json!({"file": file, "path": path}));
    let schema = output_schema_for::<ImportResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    assert_eq!(reply["sheet"], json!({"size_mm": [297.0, 210.0], "standard": "a4", "orientation": "landscape"}));
    assert_eq!((reply["strokes"].as_u64(), reply["texts"].as_u64()), (Some(7), Some(3)), "4 frame sides, 2 lines, 1 circle: {reply}");
    assert_eq!(reply["fields"], json!([{"name": "title", "text": "Bracket"}, {"name": "drawing_number", "text": "D-0001"}]));
    // A drawing from the file: the page's top-left frame corner is 10 mm in from the sheet's top left.
    run(&mut s, "drawing.create", json!({"template": path}));
    let d = s.drawing("test").unwrap().clone();
    let sheet = &d.sheets[0];
    assert!(sheet.border.is_none() && sheet.title_block.is_none());
    let corners: Vec<Vec2> = sheet
        .format
        .strokes
        .iter()
        .filter_map(|s| match s.path[0] {
            Seg2::Line { a, .. } => Some(a),
            _ => None,
        })
        .collect();
    assert!(corners.iter().any(|c| (*c - Vec2::new(10.0, 200.0)).len() < 1e-9), "{corners:?}");
    let name = sheet.format.texts.iter().find(|t| t.text == "ACME LTD").unwrap();
    assert!((name.at[0] - 190.0).abs() < 1e-9 && (name.at[1] - 32.0).abs() < 1e-9, "{name:?}");
    // Its sheet exports with the format drawn.
    let svg = run(&mut s, "drawing.export", json!({"sheet": "Sheet 1"}));
    assert!(svg["svg"].as_str().is_some_and(|t| t.contains("<path")), "{svg}");
    // A drawing on the built-in template, restyled by the file, takes its format.
    run(&mut s, "drawing.close", json!({}));
    run(&mut s, "drawing.create", json!({"name": "Second"}));
    run(&mut s, "drawing.template.apply", json!({"template": path}));
    assert_eq!(s.drawing("test").unwrap().sheets[0].format, d.sheets[0].format);
}

/// A page that isn't a standard size keeps its own, lying as it does.
#[test]
fn a_page_of_its_own_size_is_kept() {
    let (sheet, standard) = paper(Vec2::new(200.0, 300.0));
    assert_eq!((sheet, standard), (Sheet { size: SheetSize::Custom { sides_mm: [200.0, 300.0] }, orientation: Orientation::Portrait }, None));
    let (sheet, standard) = paper(Vec2::new(431.0, 279.0));
    assert_eq!((sheet.size, standard.as_deref()), (SheetSize::Standard(PaperSize::AnsiB), Some("ansi_b")), "within 2 mm of ANSI B");
}

/// What isn't an SVG, has no page or writes nowhere is refused, naming why; a broken file is too.
#[test]
fn a_bad_import_is_refused() {
    let folder = Folder::new("drawing-template-import-bad");
    let mut s = Session::default();
    assert!(refused(&mut s, json!({"file": "format.dxf", "path": "x.json"})).contains(".svg file"));
    let file = folder.path("no-page.svg");
    std::fs::write(&file, r#"<svg xmlns="http://www.w3.org/2000/svg"><line x1="0" y1="0" x2="1" y2="1"/></svg>"#).unwrap();
    assert!(refused(&mut s, json!({"file": file, "path": folder.path("t.json")})).contains("no width and height"));
    let good = folder.path("good.svg");
    std::fs::write(&good, A4_FORMAT).unwrap();
    assert!(refused(&mut s, json!({"file": good})).contains("`path`"));
    assert!(refused(&mut s, json!({"file": good, "path": folder.path("t.txt")})).contains(".json"));
    let broken = folder.path("broken.svg");
    std::fs::write(&broken, r#"<svg xmlns="http://www.w3.org/2000/svg" width="1e400mm" height="10mm"><line x1="0" y1="0" x2="1" y2="1"/></svg>"#)
        .unwrap();
    refused(&mut s, json!({"file": broken, "path": folder.path("t.json")}));
}
