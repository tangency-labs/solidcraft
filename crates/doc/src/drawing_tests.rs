use serde_json::json;
use solidcraft_geom::drawing_measure::{LinearDirection, Reading};
use solidcraft_geom::drawing_sheet::{Margins, Orientation, PaperSize, SheetSize};
use solidcraft_geom::drawing_view::{NamedView, ProjectedSide};
use solidcraft_geom::paper::line::LineGroup;
use solidcraft_geom::title_block::{Cell, CellKind, TextCell};
use solidcraft_geom::{Region2Geom, Vec2Geom};

use solidcraft_render::VisualStyle;

use crate::drawing_view::{TangentEdges, ViewDirection, ViewOverrides};
use balloon::Balloon;
use dimension::{DimensionKind, ModelEdge, ModelPoint, TextPlace};
use view::{ViewKind, ViewPlace};

use super::*;

/// The seed every test drawing's ids come from.
const SEED: u64 = 3890;

/// A profile of its own: ISO 5457's filing margin and trimming marks, which the built-in leaves off.
fn own_border() -> DrawingBorderProfile {
    DrawingBorderProfile { filing_margin: true, trimming_mark_mm: Some([10.0, 5.0]), ..DrawingBorderProfile::iso() }
}

/// The design it is in, on two sheets: the first (A3, the ISO border) documents its component 7;
/// the second (A4 portrait, a border of its own, 600 dpi, left out of printing) the design itself.
/// On the first sheet, the front view at 1:2, its right view lined up with it and drawing hidden
/// lines too, and the right view's top view placed apart; on the second, an iso view at 1:1,
/// tangent edges shown, with reference_shown_percent 5. On the front view, a dimension of each kind:
/// between two corners, a placed copy's edge length along a direction, and an angle with its text
/// given; and a balloon on the placed copy's top edge.
pub(crate) fn plate() -> Drawing {
    let mut id_source = IdSource::seeded(SEED);
    let a3 = drawing_sheet::Sheet::standard(PaperSize::A3, Orientation::Landscape);
    let mut d = Drawing::new("Plate assembly", "en-GB", "Assembly", a3, Some(DrawingBorderProfile::iso()), &BTreeSet::new(), &mut id_source).unwrap();
    let id = d.allocate_id(&mut id_source).unwrap();
    d.sources.push(Source { id, path: None });
    let plate_id = d.sources[0].id;
    let id = d.allocate_id(&mut id_source).unwrap();
    d.sheets.push(Arc::new(Sheet {
        id,
        name: "Bolt".into(),
        sheet: drawing_sheet::Sheet::standard(PaperSize::A4, Orientation::Portrait),
        border: Some(own_border()),
        title_block: None,
        title_block_overrides: BTreeMap::new(),
        dpi: 600.0,
        documents: Some(SheetComponent { source: plate_id, component: 0 }),
        excluded: Excluded { count: false, print: true },
        line_group: None,
    }));
    Arc::make_mut(&mut d.sheets[0]).documents = Some(SheetComponent { source: plate_id, component: 7 });
    Arc::make_mut(&mut d.properties).set(crate::metadata::Owner::Design, "author", json!("Jane Doe")).unwrap();
    d.units = Units::Fixed { unit: "in".into() };
    let (assembly, bolt_sheet) = (d.sheets[0].id, d.sheets[1].id);
    let mut add = |d: &mut Drawing, sheet, kind, place, settings, reference_shown_percent| {
        let id = d.allocate_id(&mut id_source).unwrap();
        d.add_views([Arc::new(View { id, sheet, kind, place, settings, reference_shown_percent, label: None })]);
        id
    };
    let half = ViewOverrides { scale: Some("1:2".parse().unwrap()), ..ViewOverrides::default() };
    let front = ViewKind::Base { source: plate_id, view: ViewDirection::Named(NamedView::Front) };
    let front = add(&mut d, assembly, front, ViewPlace::At([150.0, 150.0]), half, 0.0);
    let hidden = ViewOverrides { visual_style: Some(VisualStyle::VisibleHidden), ..ViewOverrides::default() };
    let right = ViewKind::Projected { parent: front, side: ProjectedSide::Right };
    let right = add(&mut d, assembly, right, ViewPlace::Aligned(320.0), hidden, 0.0);
    let top = ViewKind::Projected { parent: right, side: ProjectedSide::Above };
    add(&mut d, assembly, top, ViewPlace::At([320.0, 250.0]), ViewOverrides::default(), 0.0);
    let iso = ViewKind::Base { source: plate_id, view: ViewDirection::Named(NamedView::IsoFrontRight) };
    let full_size = ViewOverrides { scale: Some("1:1".parse().unwrap()), tangent_edges: Some(TangentEdges::Show), ..ViewOverrides::default() };
    add(&mut d, bolt_sheet, iso, ViewPlace::At([105.0, 148.5]), full_size, 5.0);
    let corner = |edges: [&str; 2]| ModelPoint {
        body: "Plate".into(),
        origin_feature: 1,
        edges: edges.map(String::from).to_vec(),
        centre: false,
        path: Vec::new(),
    };
    let edge = |name: &str| ModelEdge { body: "Plate".into(), origin_feature: 1, name: name.into(), path: vec![3, 7] };
    let measures = [
        DimensionKind::Linear {
            a: corner(["F1:front|F1:bottom", "F1:front|F1:left"]),
            b: corner(["F1:front|F1:bottom", "F1:front|F1:right"]),
            direction: LinearDirection::Horizontal,
            reading: Reading::Projected,
        },
        DimensionKind::Length { edge: edge("F1:front|F1:top"), direction: LinearDirection::Along(Vec2::new(1.0, 1.0)), reading: Reading::Projected },
        DimensionKind::Angle { a: edge("F1:front|F1:top"), b: edge("F2:chamfer|F1:front"), reading: Reading::True, other_side: true },
    ];
    let texts = [(TextPlace::Centred, None), (TextPlace::At([0.0, -20.0]), None), (TextPlace::At([12.5, 30.0]), Some("≈45°".to_string()))];
    for (kind, (text, override_text)) in measures.into_iter().zip(texts) {
        let id = d.allocate_id(&mut id_source).unwrap();
        d.add_dimensions([Arc::new(Dimension {
            id,
            view: front,
            kind,
            offset_mm: -10.0,
            text,
            decimals: 1,
            override_text,
            last_good: None,
            from_model: None,
        })]);
    }
    let id = d.allocate_id(&mut id_source).unwrap();
    d.balloons.add([Arc::new(Balloon { id, view: front, edge: edge("F1:front|F1:top"), at: [60.0, 45.0], parts_list: None })]);
    d.validate().unwrap();
    d
}

/// One A4 sheet that documents nothing, no sources: the base the caps grow from.
fn bare() -> Drawing {
    let a4 = drawing_sheet::Sheet::standard(PaperSize::A4, Orientation::Landscape);
    Drawing::new("Bare", "en", "Sheet", a4, None, &BTreeSet::new(), &mut IdSource::seeded(SEED)).unwrap()
}

fn file(d: &Drawing) -> Value {
    serde_json::from_str(&d.to_json().unwrap()).unwrap()
}

fn read(file: &Value) -> Result<Drawing> {
    Drawing::from_json(&file.to_string())
}

/// Why `file` is refused, or "read".
fn refusal(file: &Value) -> String {
    read(file).map_or_else(|e| e.to_string(), |_| "read".into())
}

/// Why `d`'s file, changed by `edit`, is refused, or "read".
fn refused(d: &Drawing, edit: impl FnOnce(&mut Value)) -> String {
    let mut v = file(d);
    edit(&mut v);
    refusal(&v)
}

/// A drawing reads back as it was saved, and save, open and save again writes the same bytes, as
/// does another run from the same seed. The file holds each field under the record's name for it,
/// ids as their 13 characters, and no counter.
#[test]
fn a_drawing_reads_back_as_saved_and_saves_the_same_bytes_again() {
    let d = plate();
    let saved = d.to_json().unwrap();
    let reopened = Drawing::from_json(&saved).unwrap();
    assert_eq!(reopened, d);
    assert_eq!(reopened.to_json().unwrap(), saved);
    assert_eq!(plate().to_json().unwrap(), saved, "a seeded run makes the same drawing");
    let v = file(&d);
    let plate_id = ids::encode(d.sources[0].id);
    let [assembly_id, bolt_sheet_id] = [0, 1].map(|i| ids::encode(d.sheets[i].id));
    assert_eq!(v["id"], "1ge30wkvq38r0", "the seed's first id, as splitmix64 gives it");
    assert_eq!(v["format"], DRAWING.format_string(DRAWING.current()));
    assert_eq!(v.get("format_pending").is_some(), !DRAWING.pending_names().is_empty());
    assert_eq!(v.get("next_id"), None);
    assert_eq!((&v["name"], &v["language"]), (&json!("Plate assembly"), &json!("en-GB")));
    assert_eq!(v["sources"], json!([{"id": plate_id}]));
    assert_eq!(v["properties"], json!({"author": "Jane Doe"}));
    assert_eq!(v["units"], json!({"fixed": {"unit": "in"}}));
    assert_eq!((&v["sheets"][0]["id"], &v["sheets"][0]["dpi"]), (&json!(assembly_id), &json!(300.0)));
    assert_eq!(v["sheets"][0]["sheet"], json!({"size": {"standard": "a3"}, "orientation": "landscape"}));
    assert_eq!(v["sheets"][0]["documents"], json!({"source": plate_id, "component": 7}));
    assert_eq!(v["sheets"][0]["excluded"], json!({"count": false, "print": false}));
    assert_eq!(
        v["sheets"][1],
        json!({
            "id": bolt_sheet_id, "name": "Bolt", "sheet": {"size": {"standard": "a4"}, "orientation": "portrait"},
            "border": serde_json::to_value(own_border()).unwrap(), "dpi": 600.0,
            "documents": {"source": plate_id, "component": 0}, "excluded": {"count": false, "print": true}
        })
    );
}

/// A hand-written file may leave out what has a default, and reads as a new drawing has it; the
/// rest is needed.
#[test]
fn a_file_may_leave_out_what_has_a_default() {
    let minimal = json!({
        "format": DRAWING.format_string(1),
        "id": "0000000000002",
        "name": "Bracket",
        "sources": [],
        "language": "de",
        "sheets": [{"id": "0000000000001", "name": "Blatt 1", "sheet": {"size": {"standard": "a3"}}}]
    });
    let d = read(&minimal).unwrap();
    assert_eq!((d.sheets[0].id, &d.units, d.properties.is_empty()), (1, &Units::FollowModel, true));
    let sheet = &d.sheets[0];
    assert_eq!(sheet.sheet, drawing_sheet::Sheet::standard(PaperSize::A3, Orientation::Landscape));
    assert_eq!((sheet.border, sheet.dpi, sheet.documents, sheet.excluded), (None, DEFAULT_DPI, None, Excluded::default()));
    // The format as this build writes it, however the file spelt it.
    let mut spelt = minimal.clone();
    spelt["format"] = json!("solidcraft-drawing/+1");
    assert_eq!(read(&spelt).unwrap().format, DRAWING.format_string(DRAWING.current()));
    for key in ["id", "name", "sources", "language", "sheets"] {
        let mut v = minimal.clone();
        v.as_object_mut().unwrap().remove(key);
        let why = refusal(&v);
        assert!(why.starts_with("damaged drawing: ") && why.contains(&format!("missing field `{key}`")), "{why}");
    }
}

/// Every cap on both sides: sheets, sources, the bytes of names (two-byte characters too).
#[test]
fn every_cap_holds_on_both_sides() {
    let base = file(&bare());
    let with = |key: &str, value: Value| {
        let mut v = base.clone();
        v[key] = value;
        v
    };
    let sheet = base["sheets"][0].clone();
    let sheets = |n: u64| {
        let one = |id: u64| {
            let mut s = sheet.clone();
            s["id"] = json!(ids::encode(id));
            s
        };
        Value::Array((1..=n).map(one).collect())
    };
    assert_eq!(read(&with("sheets", sheets(1000))).unwrap().sheets.len(), 1000);
    for n in [0, 1001] {
        assert_eq!(refusal(&with("sheets", sheets(n))), "damaged drawing: a drawing has 1 to 1000 sheets", "{n} sheets");
    }
    // Sources from id 1: the drawing's and the sheet's ids are the seed's, far from them. Each names
    // another design's file by its path, but at most one: the design the drawing is in, by an id alone.
    let linked = |n: u64| Value::Array((1..=n).map(|id| json!({"id": ids::encode(id), "path": format!("parts/{id}.sc")})).collect());
    for n in [0, 1, 64] {
        assert_eq!(read(&with("sources", linked(n))).unwrap().sources.len(), n as usize);
    }
    assert_eq!(refusal(&with("sources", linked(65))), "damaged drawing: a drawing draws at most 64 designs");
    let own = |id: u64| json!({"id": ids::encode(id)});
    let both = json!([own(1), {"id": ids::encode(2), "path": "plate.sc"}]);
    assert_eq!(read(&with("sources", both)).unwrap().own_source(), Some(1));
    assert_eq!(refusal(&with("sources", json!([own(1), own(2)]))), "damaged drawing: a drawing draws the design it is in at most once");
    for broken in ["", "  ", "a\0b.sc", "a\nb.sc"] {
        let one = json!([{"id": ids::encode(1), "path": broken}]);
        let rule = "damaged drawing: a source's path is not empty, not only spaces, without control characters";
        assert_eq!(refusal(&with("sources", one)), rule, "{broken:?}");
    }
    // One file named twice still reads: only the file system can say two paths are one file, so
    // the engine refuses it when a source is added, never the record.
    let twice = json!([{"id": ids::encode(1), "path": "plate.sc"}, {"id": ids::encode(2), "path": "plate.sc"}]);
    assert_eq!(read(&with("sources", twice)).unwrap().sources.len(), 2);
    // No cap on a path's bytes but the drawing's own: a design deep in folders keeps its sources'
    // whole paths in memory, as a component link keeps its file's.
    let long = "é".repeat(3000) + ".sc";
    let deep = read(&with("sources", json!([{"id": ids::encode(1), "path": long}]))).unwrap();
    assert_eq!(deep.sources[0].path.as_deref(), Some(long.as_str()));
    let names = |name: &str| {
        let mut sheet_named = base.clone();
        sheet_named["sheets"][0]["name"] = json!(name);
        (refusal(&with("name", json!(name))), refusal(&sheet_named))
    };
    for fits in ["x".repeat(256), "é".repeat(128), "A".into(), "Blatt 1 – Übersicht".into()] {
        assert_eq!(names(fits.as_str()), ("read".to_string(), "read".to_string()), "{} bytes", fits.len());
    }
    let rule = "name is 1 to 256 bytes, not only spaces, without control characters";
    let sheet_id = ids::encode(bare().sheets[0].id);
    for broken in ["x".repeat(257), "é".repeat(128) + "a", String::new(), " ".into(), "two\nlines".into(), "bell\u{7}".into(), "nul\0".into()] {
        let expected = (format!("damaged drawing: the drawing's {rule}"), format!("damaged drawing: sheet {sheet_id}: its {rule}"));
        assert_eq!(names(broken.as_str()), expected, "{broken:?}");
    }
}

/// Sources read in id order whatever order a file lists them in, and a drawing that draws another
/// design's file saves the same bytes again: on its own (a design container's entry for it) and
/// inline in a design's JSON. A `null` path reads as none and is never written.
#[test]
fn sources_read_in_id_order_and_save_the_same_bytes() {
    let mut v = file(&bare());
    v["sources"] = json!([{"id": ids::encode(9), "path": "../parts/plate.sc"}, {"id": ids::encode(3), "path": null}]);
    let d = read(&v).unwrap();
    assert_eq!(d.sources, [Source { id: 3, path: None }, Source { id: 9, path: Some("../parts/plate.sc".into()) }]);
    let saved = d.to_json().unwrap();
    assert_eq!(Drawing::from_json(&saved).unwrap().to_json().unwrap(), saved, "save, open, save: the same bytes");
    let written: Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(written["sources"], json!([{"id": ids::encode(3)}, {"id": ids::encode(9), "path": "../parts/plate.sc"}]));
    let mut doc = crate::Document::new("Case");
    doc.drawings = vec![Arc::new(d)];
    let inline = doc.to_json();
    assert_eq!(crate::Document::from_json(&inline).unwrap().to_json(), inline);
}

/// The content language is a well-formed BCP 47 tag of 2 to 35 characters.
#[test]
fn the_language_is_a_bcp_47_tag_of_2_to_35_characters() {
    let base = file(&bare());
    let why = |tag: &str| {
        let mut v = base.clone();
        v["language"] = json!(tag);
        refusal(&v)
    };
    let longest = "en-abcdefgh-abcdefgh-abcdefgh-abcde";
    assert_eq!(longest.len(), 35);
    for tag in ["en", "de", "ast", "en-GB", "EN-gb", "zh-Hans-CN", "sr-Latn-RS", "es-419", longest] {
        assert_eq!(why(tag), "read", "{tag}");
    }
    let rule = "damaged drawing: the language is not a BCP 47 tag of 2 to 35 characters (such as `en` or `en-GB`)";
    let too_long = format!("{longest}f");
    let broken =
        ["e", "", too_long.as_str(), "en_GB", "en-", "-en", "en--GB", "e1", "x-private", "en-abcdefghi", "abcdefghi", "日本", "en GB", "en\n"];
    for tag in broken {
        assert_eq!(why(tag), rule, "{tag:?}");
    }
}

/// A drawing names its font by family: a file is refused when the drawing is read, as it is when a
/// command sets it.
#[test]
fn a_font_file_in_a_drawing_is_refused_when_read() {
    let base = file(&bare());
    let why = |font: &str| {
        let mut v = base.clone();
        v["font"] = json!(font);
        refusal(&v)
    };
    assert_eq!(why("IBM Plex Mono"), "read");
    for path in ["C:/Fonts/arial.ttf", "/usr/share/fonts/a", "Futura.otf"] {
        let e = why(path);
        assert!(e.contains("the drawing's font") && e.contains("drawings name fonts by family"), "{path}: {e}");
    }
}

/// A fixed unit is a length unit of the expression unit table, written as the table writes it.
#[test]
fn a_fixed_unit_is_a_length_unit_of_the_expression_table() {
    let base = file(&bare());
    let units = |units: Value| {
        let mut v = base.clone();
        v["units"] = units;
        read(&v).map(|d| d.units)
    };
    for (unit, kind, _) in expr::UNITS {
        assert_eq!(units(json!({"fixed": {"unit": unit}})).is_ok(), *kind == expr::Kind::Length, "{unit}");
    }
    assert_eq!(units(json!({"fixed": {"unit": "in"}})).unwrap(), Units::Fixed { unit: "in".into() });
    assert_eq!(units(json!("follow_model")).unwrap(), Units::FollowModel);
    let rule = "damaged drawing: the drawing's unit is not a length unit (such as `mm` or `in`)";
    for unit in ["furlong", "deg", "°", "none", "", " in", "in ", "IN"] {
        assert_eq!(units(json!({"fixed": {"unit": unit}})).unwrap_err().to_string(), rule, "{unit:?}");
    }
    for broken in [json!("follow_design"), json!({"fixed": "in"}), json!({"fixed": {}}), json!(null)] {
        assert!(units(broken.clone()).is_err(), "{broken}");
    }
}

/// A drawing always saves its line group, so a later default never changes it, and an older
/// drawing, which saved none, reads the 0.5 group; anything but a name of ISO 128's series is refused.
#[test]
fn a_drawings_line_group_is_always_saved_and_an_older_one_reads_0_5() {
    let d = bare();
    let v = file(&d);
    assert_eq!(v["line_group"], json!("0.5"));
    let mut older = v.clone();
    older.as_object_mut().unwrap().remove("line_group");
    assert_eq!(read(&older).unwrap().line_group, LineGroup::Mm0_5);
    let mut wider = v;
    wider["line_group"] = json!("0.7");
    assert_eq!(read(&wider).unwrap().line_group, LineGroup::Mm0_7);
    for hostile in [json!("0.6"), json!(0.5), json!(null), json!(""), json!("Mm0_5"), json!({"wide": "0.5"})] {
        assert_ne!(refused(&d, |v| v["line_group"] = hostile.clone()), "read", "{hostile}");
    }
}

/// A sheet saves a line group only when it sets one of its own; otherwise it follows the drawing's.
#[test]
fn a_sheets_line_group_is_saved_only_when_it_sets_one() {
    let mut d = bare();
    assert_eq!(file(&d)["sheets"][0].get("line_group"), None);
    Arc::make_mut(&mut d.sheets[0]).line_group = Some(LineGroup::Mm0_35);
    let v = file(&d);
    assert_eq!(v["sheets"][0]["line_group"], json!("0.35"));
    assert_eq!(read(&v).unwrap().sheets[0].line_group, Some(LineGroup::Mm0_35));
    for hostile in [json!("3"), json!(0.35), json!([])] {
        assert_ne!(refused(&d, |v| v["sheets"][0]["line_group"] = hostile.clone()), "read", "{hostile}");
    }
}

/// A sheet's border is saved as a copy of its profile, field by field, and read back as saved, so
/// a later built-in never changes it; a profile's name in its place is refused.
#[test]
fn a_border_is_saved_as_a_copy_of_its_profile() {
    let v = file(&plate());
    assert_eq!(v["sheets"][0]["border"], serde_json::to_value(DrawingBorderProfile::iso()).unwrap());
    assert_eq!(v["sheets"][1]["border"], serde_json::to_value(own_border()).unwrap());
    assert_eq!(read(&v).unwrap().sheets[1].border, Some(own_border()));
    let mut edited = v.clone();
    edited["sheets"][0]["border"]["margins"]["left_mm"] = json!(15.0);
    let iso = DrawingBorderProfile::iso();
    let wider = DrawingBorderProfile { margins: Margins { left_mm: 15.0, ..iso.margins }, ..iso };
    assert_eq!(read(&edited).unwrap().sheets[0].border, Some(wider));
    edited["sheets"][0]["border"] = json!("iso");
    assert!(refusal(&edited).starts_with("damaged drawing: "));
    edited["sheets"][0]["border"] = Value::Null;
    assert_eq!(read(&edited).unwrap().sheets[0].border, None, "no drawing border");
}

/// A sheet's drawing goes inside its drawing border: 10 mm in from each edge with the ISO border,
/// 20 mm on the left with a filing margin, and the whole paper with no border.
#[test]
fn a_sheets_drawing_space_is_inside_its_border_or_the_whole_paper() {
    let d = plate();
    let space = |sheet: &Sheet| sheet.drawing_space().map(|b| (b.min, b.max)).unwrap();
    assert_eq!(space(&d.sheets[0]), (Vec2::new(10.0, 10.0), Vec2::new(410.0, 287.0)), "A3 landscape, ISO");
    assert_eq!(space(&d.sheets[1]), (Vec2::new(20.0, 10.0), Vec2::new(200.0, 287.0)), "A4 portrait, filing margin");
    let bare = Sheet { border: None, ..(*d.sheets[0]).clone() };
    assert_eq!(space(&bare), (Vec2::ZERO, Vec2::new(420.0, 297.0)), "no border");
    let low = drawing_sheet::Sheet { size: SheetSize::Custom { sides_mm: [20.5, 297.0] }, ..d.sheets[0].sheet };
    let narrow = Sheet { sheet: low, ..(*d.sheets[0]).clone() };
    assert!(narrow.drawing_space().is_err(), "margins that leave no drawing space");
}

/// The most cells a block may have: 8 by 8 unlabelled cells of 20 x 6.75 mm, 160 x 54 mm in all.
fn full_block() -> TitleBlock {
    use solidcraft_geom::cell::{Content, Fit, HAlign, Part};
    let cell = |i: usize| {
        let (x, y) = ((i % 8) as f64 * 20.0 - 160.0, (i / 8) as f64 * 6.75);
        let value = TextCell { content: Content(vec![Part::Text("-".into())]), cap_mm: 2.5, align: HAlign::Left, fit: Fit::default(), image: None };
        Cell { name: format!("cell_{i}"), rect_mm: [x, y, x + 20.0, y + 6.75], label: None, kind: CellKind::Text(value) }
    };
    TitleBlock { size_mm: Vec2::new(160.0, 54.0), outline_line_mm: 0.7, cell_line_mm: 0.35, cells: (0..64).map(cell).collect() }
}

/// A sheet's title block is a copy saved with it, read back as it was and saved as the same bytes.
/// A sheet without one saves as before, with no `title_block`, so an older drawing reads as one
/// with none. A block needs a drawing border that holds it, and an unknown field is refused.
#[test]
fn a_sheets_title_block_is_a_copy_on_its_border() {
    let mut d = plate();
    assert!(!d.to_json().unwrap().contains("title_block"), "nothing saved for no block");
    Arc::make_mut(&mut d.sheets[0]).title_block = Some(full_block());
    let saved = d.to_json().unwrap();
    let read = Drawing::from_json(&saved).unwrap();
    assert_eq!(read, d);
    assert_eq!(read.to_json().unwrap(), saved, "the same bytes");
    let sheet = ids::encode(d.sheets[0].id);
    let no_border = refused(&d, |v| v["sheets"][0]["border"] = Value::Null);
    assert_eq!(no_border, format!("damaged drawing: sheet {sheet}: its title block sits on a drawing border, and it has none"));
    // The A3 sheet's ISO border is 400 mm wide: a block as wide fits, one wider doesn't.
    let wide = |width: f64| refused(&d, |v| v["sheets"][0]["title_block"]["size_mm"] = json!([width, 54.0]));
    assert_eq!(wide(400.0), "read");
    assert_eq!(wide(400.5), format!("damaged drawing: sheet {sheet}: title block: a title block fits inside the drawing border"));
    let unknown = refused(&d, |v| v["sheets"][0]["title_block"]["colour"] = json!("red"));
    assert!(unknown.starts_with("damaged drawing: unknown field `colour`"), "{unknown}");
}

/// A sheet's title block sits on its drawing border's lower-right corner: on A3 with the ISO
/// border, (230, 10) to (410, 59); none without a block.
#[test]
fn a_sheets_title_block_outline() {
    let mut d = Drawing::new(
        "Outline",
        "en",
        "Sheet",
        drawing_sheet::Sheet::standard(PaperSize::A3, Orientation::Landscape),
        Some(DrawingBorderProfile::iso()),
        &BTreeSet::new(),
        &mut IdSource::seeded(SEED),
    )
    .unwrap();
    assert_eq!(d.sheets[0].title_block_outline().unwrap(), None);
    Arc::make_mut(&mut d.sheets[0]).title_block = Some(solidcraft_geom::title_block::default_block());
    let outline = d.sheets[0].title_block_outline().unwrap().unwrap();
    assert_eq!((outline.min, outline.max), (Vec2::new(230.0, 10.0), Vec2::new(410.0, 59.0)));
}

/// The most a drawing's blocks can ask of every edit's check: 1000 sheets, each with a 64-cell block.
#[test]
fn a_thousand_sheets_of_full_blocks_are_checked() {
    let mut id_source = IdSource::seeded(SEED);
    let a3 = drawing_sheet::Sheet::standard(PaperSize::A3, Orientation::Landscape);
    let mut d = Drawing::new("Blocks", "en", "Sheet", a3, Some(DrawingBorderProfile::iso()), &BTreeSet::new(), &mut id_source).unwrap();
    Arc::make_mut(&mut d.sheets[0]).title_block = Some(full_block());
    while d.sheets.len() < MAX_SHEETS {
        let id = d.allocate_id(&mut id_source).unwrap();
        d.sheets.push(Arc::new(Sheet { id, ..(*d.sheets[0]).clone() }));
    }
    let started = std::time::Instant::now();
    d.validate().unwrap();
    eprintln!("1000 sheets of 64-cell blocks checked in {:?}", started.elapsed());
}

/// What reading refuses: each damage the rules name, refused rather than patched, and files this
/// build can't read.
#[test]
fn damaged_files_are_refused_and_never_patched() {
    let d = plate();
    let (drawing_id, plate_id) = (ids::encode(d.id), ids::encode(d.sources[0].id));
    let [assembly_id, bolt_sheet_id] = [0, 1].map(|i| ids::encode(d.sheets[i].id));
    // Two objects with one id, of one kind and of two kinds, the drawing itself among them.
    let two = |id: &str| format!("damaged drawing: two objects have the id {id}");
    assert_eq!(refused(&d, |v| v["sources"][0]["id"] = json!(drawing_id)), two(&drawing_id));
    assert_eq!(refused(&d, |v| v["sheets"][1]["id"] = json!(assembly_id)), two(&assembly_id));
    assert_eq!(refused(&d, |v| v["sources"][0]["id"] = json!(bolt_sheet_id)), two(&bolt_sheet_id));
    assert_eq!(refused(&d, |v| v["id"] = json!(assembly_id)), two(&assembly_id));
    // An id that isn't one, where an object has it and where a reference names it; no number form,
    // and no counter.
    let not_an_id = "damaged drawing: an id is 13 characters of lowercase Crockford base32, not all 0";
    for text in ["0000000000000", "FZZZZZZZZZZZZ", "000000000001", "0000000000001 ", "000000000000o", "g000000000000"] {
        assert_eq!(refused(&d, |v| v["id"] = json!(text)), not_an_id, "{text:?}");
        assert_eq!(refused(&d, |v| v["sources"][0]["id"] = json!(text)), not_an_id, "{text:?}");
        assert_eq!(refused(&d, |v| v["sheets"][1]["documents"]["source"] = json!(text)), not_an_id, "{text:?}");
    }
    assert!(refused(&d, |v| v["sources"][0]["id"] = json!(5)).starts_with("damaged drawing: invalid type"));
    assert!(refused(&d, |v| v["next_id"] = json!(6)).contains("unknown field `next_id`"));
    for zero in [|d: &mut Drawing| d.id = 0, |d: &mut Drawing| d.sources[0].id = 0] {
        let mut in_memory = d.clone();
        zero(&mut in_memory);
        assert_eq!(in_memory.validate().unwrap_err().to_string(), "an object has the id 0, which is never one");
    }
    // A revision tag on a sheet the drawing doesn't have.
    let mut tagged = d.clone();
    let record = std::sync::Arc::make_mut(&mut tagged.revisions);
    record.add(None, 1 << 44).unwrap();
    record.add_tag(1 << 44, (1 << 44) + 1, 1 << 43, [10.0, 10.0]).unwrap();
    let why = tagged.validate().unwrap_err().to_string();
    assert!(why.contains("a revision tag is on one of the drawing's sheets"), "{why}");
    // A reference to nothing, or to an object of another kind; a source a sheet documents removed.
    for missing in [ids::encode(99), assembly_id.clone()] {
        let expected = format!("damaged drawing: sheet {bolt_sheet_id}: it documents source {missing}, which the drawing doesn't have");
        assert_eq!(refused(&d, |v| v["sheets"][1]["documents"]["source"] = json!(missing)), expected);
    }
    let without_plate = refused(&d, |v| {
        v["sources"].as_array_mut().unwrap().remove(0);
    });
    assert_eq!(without_plate, format!("damaged drawing: sheet {assembly_id}: it documents source {plate_id}, which the drawing doesn't have"));
    // The paper, its border and its resolution, through the sheet's own rules.
    let custom = |sides: Value| refused(&d, |v| v["sheets"][0]["sheet"]["size"] = json!({"custom": {"sides_mm": sides}}));
    assert_eq!(custom(json!([5000.0, 100.0])), "read");
    // A side the border's margins fill leaves no drawing space; without a border it is paper.
    let no_space = format!("damaged drawing: sheet {assembly_id}: drawing sheet: the margins leave no drawing space");
    assert_eq!(custom(json!([5000.0, 0.5])), no_space);
    let bare = refused(&d, |v| {
        v["sheets"][0]["sheet"]["size"] = json!({"custom": {"sides_mm": [5000.0, 0.5]}});
        v["sheets"][0]["border"] = Value::Null;
    });
    assert_eq!(bare, "read");
    for sides in [json!([5000.1, 100.0]), json!([0.0, 100.0]), json!([-1.0, 100.0])] {
        let expected = format!("damaged drawing: sheet {assembly_id}: drawing sheet: a custom side is finite, above 0 and at most 5000 mm");
        assert_eq!(custom(sides), expected);
    }
    let why = custom(json!([1e10, 100.0]));
    assert!(why.starts_with("damaged drawing: the number ") && why.ends_with(" is out of range"), "checked before the record: {why}");
    let nan = d.to_json().unwrap().replacen("600.0", "NaN", 1);
    assert!(Drawing::from_json(&nan).unwrap_err().to_string().starts_with("not a SolidCraft drawing: "), "NaN isn't JSON");
    assert_eq!(
        refused(&d, |v| v["sheets"][1]["border"]["margins"]["top_mm"] = json!(-1.0)),
        format!("damaged drawing: sheet {bolt_sheet_id}: drawing sheet: a margin is finite and 0 to 5000 mm")
    );
    for dpi in [25.0, 2400.0] {
        assert_eq!(refused(&d, |v| v["sheets"][1]["dpi"] = json!(dpi)), "read", "{dpi}");
    }
    for dpi in [24.9, 2400.1, 0.0, -300.0] {
        let expected = format!("damaged drawing: sheet {bolt_sheet_id}: its pictures are 25 to 2400 dpi");
        assert_eq!(refused(&d, |v| v["sheets"][1]["dpi"] = json!(dpi)), expected, "{dpi}");
    }
    // In memory, where NaN can be, the same rules.
    let mut in_memory = plate();
    Arc::make_mut(&mut in_memory.sheets[1]).dpi = f64::NAN;
    assert_eq!(in_memory.validate().unwrap_err().to_string(), format!("sheet {bolt_sheet_id}: its pictures are 25 to 2400 dpi"));
    let sheet = Arc::make_mut(&mut in_memory.sheets[1]);
    sheet.dpi = DEFAULT_DPI;
    sheet.sheet.size = SheetSize::Custom { sides_mm: [f64::NAN, 100.0] };
    assert!(in_memory.validate().unwrap_err().to_string().starts_with(&format!("sheet {bolt_sheet_id}: drawing sheet: a custom side")));
    // An unknown field anywhere.
    let unknown: [fn(&mut Value); 8] = [
        |v| v["annotations"] = json!([]),
        |v| v["sources"][0]["name"] = json!("Plate"),
        |v| v["sheets"][0]["scale"] = json!(1),
        |v| v["sheets"][0]["sheet"]["colour"] = json!("white"),
        |v| v["sheets"][0]["border"]["style"] = json!("iso"),
        |v| v["sheets"][0]["documents"]["view"] = json!(1),
        |v| v["sheets"][0]["excluded"]["export"] = json!(true),
        |v| v["units"]["fixed"]["precision"] = json!(2),
    ];
    for (i, edit) in unknown.into_iter().enumerate() {
        let why = refused(&d, edit);
        assert!(why.starts_with("damaged drawing: ") && why.contains("unknown field"), "{i}: {why}");
    }
    // A newer build's file isn't held to this build's rules: a field it doesn't know, or a sheet with
    // a number out of range, stays out. With no sheet left to show, it says why.
    let (current, newer) = (DRAWING.current(), DRAWING.current() + 1);
    let why = refused(&d, |v| {
        v["format"] = json!(DRAWING.format_string(newer));
        v["annotations"] = json!([]);
    });
    assert_eq!(why, "read");
    let why = refused(&d, |v| {
        v["format"] = json!(DRAWING.format_string(newer));
        v["sheets"] = json!([{"id": "future", "a_future_field": true}]);
    });
    assert!(
        why.starts_with(&format!(
            "this drawing was saved by a newer SolidCraft (format {newer}, this version reads up to {current}) and this version can't read it: "
        )),
        "{why}"
    );
    // A list of pending changes means nothing beside a format made of none; beside the pending
    // number, a change this build doesn't know is a newer build's.
    let why = refused(&d, |v| v["format_pending"] = json!(["a-later-change"]));
    assert_eq!(why, "read");
    // A design is never read as a drawing, nor a drawing as a design.
    let design = crate::Document::new("Plate").to_json();
    let design_format = crate::format::format_string(crate::format::FORMAT);
    assert_eq!(Drawing::from_json(&design).unwrap_err().to_string(), format!("not a SolidCraft drawing (format `{design_format}`)"));
    let drawing_format = DRAWING.format_string(current);
    assert_eq!(
        crate::Document::from_json(&d.to_json().unwrap()).unwrap_err().to_string(),
        format!("not a SolidCraft design (format `{drawing_format}`)")
    );
    // Not an object, not JSON, nested too deeply, too large.
    assert_eq!(Drawing::from_json("[]").unwrap_err().to_string(), "not a SolidCraft drawing (expected a JSON object)");
    assert!(Drawing::from_json("").unwrap_err().to_string().starts_with("not a SolidCraft drawing: "));
    let deep: Value = serde_json::from_str(&format!("{}1{}", "[".repeat(100), "]".repeat(100))).unwrap();
    assert_eq!(refused(&d, |v| v["annotations"] = deep), "damaged drawing: nested too deeply");
    let deeper = format!("{{\"format\": \"{drawing_format}\", \"annotations\": {}1{}}}", "[".repeat(200), "]".repeat(200));
    assert!(Drawing::from_json(&deeper).unwrap_err().to_string().starts_with("not a SolidCraft drawing: "));
    let text = d.to_json().unwrap();
    let at_cap = text.clone() + &" ".repeat(MAX_DRAWING_BYTES - text.len());
    assert_eq!(Drawing::from_json(&at_cap).unwrap(), d);
    assert_eq!(Drawing::from_json(&(at_cap + " ")).unwrap_err().to_string(), "drawing too large (at most 16 MiB)");
}

/// A run never gives an id twice: not one in use, and not a removed object's. Only another run from
/// the same seed, which tests and replays start from the same file, can give a removed id again.
/// What is on a sheet is its views and the dimensions in them, and taking the sheet out takes
/// them too. A view elsewhere projected from one of them is left without its parent, which the
/// rules refuse.
#[test]
fn a_sheet_goes_with_everything_on_it() {
    let d = plate();
    let (assembly, bolt) = (d.sheets[0].id, d.sheets[1].id);
    let on = d.on_sheet(assembly);
    assert_eq!((on.views.len(), on.dimensions.len(), on.balloons.len()), (3, 3, 1), "{on:?}");
    assert_eq!(on.dimensions, d.dimensions.iter().map(|x| x.id).collect::<Vec<_>>());
    let bolt_on = d.on_sheet(bolt);
    assert_eq!((bolt_on.views.len(), bolt_on.dimensions.len(), bolt_on.balloons.len()), (1, 0, 0), "{bolt_on:?}");
    assert!(d.on_sheet(d.id).is_empty() && !on.is_empty(), "not a sheet: nothing on it");
    let mut without = d.clone();
    assert_eq!(without.remove_sheet(bolt), bolt_on);
    without.validate().unwrap();
    assert_eq!((without.sheets.len(), without.views.len(), without.dimensions.len(), without.balloons.len()), (1, 3, 3, 1));
    let mut without = d.clone();
    assert_eq!(without.remove_sheet(assembly), on);
    without.validate().unwrap();
    assert_eq!((without.sheets.len(), without.views.len(), without.dimensions.len(), without.balloons.len()), (1, 1, 0, 0));
    // The top view, placed apart, moved to the bolt's sheet: its parent is on the assembly's.
    let mut moved = d.clone();
    let top = moved.views.iter().position(|v| v.sheet == assembly && matches!((&v.kind, v.place), (ViewKind::Projected { .. }, ViewPlace::At(_))));
    Arc::make_mut(&mut moved.views[top.unwrap()]).sheet = bolt;
    moved.validate().unwrap();
    moved.remove_sheet(assembly);
    assert!(moved.validate().unwrap_err().to_string().contains("parent"), "{:?}", moved.validate());
}

#[test]
fn a_run_never_gives_an_id_twice() {
    let mut id_source = IdSource::seeded(SEED);
    let a4 = drawing_sheet::Sheet::standard(PaperSize::A4, Orientation::Landscape);
    let mut d = Drawing::new("Ids", "en", "Sheet", a4, None, &BTreeSet::new(), &mut id_source).unwrap();
    let mut given = BTreeSet::from([d.id, d.sheets[0].id]);
    for _ in 0..64 {
        let id = d.allocate_id(&mut id_source).unwrap();
        d.sheets.push(Arc::new(Sheet { id, ..(*d.sheets[0]).clone() }));
        assert!(given.insert(id), "{id:#x} given twice");
    }
    let removed = d.sheets.pop().map_or(0, |s| s.id);
    for _ in 0..10_000 {
        let id = d.allocate_id(&mut id_source).unwrap();
        assert!(id != removed && given.insert(id), "{id:#x} given twice");
    }
    // The same seed, from the start, over a file it made: every draw up to the removed id is in use.
    let reopened = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    let again = reopened.allocate_id(&mut IdSource::seeded(SEED)).unwrap();
    assert!(!reopened.ids().any(|id| id == again));
    assert_eq!(again, removed);
}

/// Properties follow the design metadata's rules: other tools' data and a well-known key of another
/// kind are kept as they are, never read as numbers, while the same number elsewhere is refused.
#[test]
fn properties_are_kept_as_they_are() {
    let mut v = file(&plate());
    v["properties"]["com.example.pdm"] = json!({"huge": 1e300, "ids": [1, 2, 3]});
    v["properties"]["revision"] = json!(3);
    let d = read(&v).unwrap();
    assert_eq!(d.properties.get("com.example.pdm"), Some(&json!({"huge": 1e300, "ids": [1, 2, 3]})));
    assert_eq!((d.properties.revision(), d.properties.get("revision")), (None, Some(&json!(3))));
    assert_eq!(read(&file(&d)).unwrap(), d);
    v["sheets"][0]["dpi"] = json!(1e300);
    let why = refusal(&v);
    assert!(why.starts_with("damaged drawing: the number ") && why.ends_with(" is out of range"), "{why}");
}

/// A drawing whose JSON would be over the read limit isn't saved. Other tools' properties are kept
/// as they came, and indenting them can grow a file read in under the limit past it, so the save
/// refuses rather than write a file that won't open: every drawing saved opens again.
#[test]
fn a_drawing_too_large_to_reopen_is_not_saved() {
    let file = |zeros: usize| {
        let items = vec!["0"; zeros].join(",");
        let sheet = r#"{"id":"0000000000001","name":"S","sheet":{"size":{"standard":"a4"}}}"#;
        let format = DRAWING.format_string(1);
        format!(
            r#"{{"format":"{format}","id":"0000000000002","name":"A","sources":[],"language":"en","sheets":[{sheet}],"properties":{{"com.example":{{"items":[{items}]}}}}}}"#
        )
    };
    // The case found in review (#3891): 2,000,000 zeros read in at about 4 MB and indent past 16 MiB.
    let big = Drawing::from_json(&file(2_000_000)).unwrap();
    assert_eq!(big.to_json().unwrap_err().to_string(), "drawing too large to save (at most 16 MiB)");
    let small = Drawing::from_json(&file(100_000)).unwrap();
    assert_eq!(Drawing::from_json(&small.to_json().unwrap()).unwrap(), small);
}

/// A new drawing has one sheet, the drawing's id the source's first and the sheet's the next,
/// follows its designs' units and renders pictures at 300 dpi; its id is none of its design's other
/// drawings'; a name, language or border that breaks the rules is refused.
#[test]
fn a_new_drawing_has_one_sheet_and_keeps_the_rules() {
    let d = bare();
    let mut seeded = IdSource::seeded(SEED);
    let first = seeded.next(&BTreeSet::new()).unwrap();
    let second = seeded.next(&BTreeSet::new()).unwrap();
    assert_eq!((d.id, d.sheets.len(), d.sheets[0].id, d.sheets[0].dpi, &d.units), (first, 1, second, DEFAULT_DPI, &Units::FollowModel));
    assert_eq!((d.format, d.format_pending), (DRAWING.format_string(DRAWING.current()), DRAWING.pending_names()));
    let a4 = drawing_sheet::Sheet::standard(PaperSize::A4, Orientation::Landscape);
    let beside = Drawing::new("Beside", "en", "Sheet", a4, None, &BTreeSet::from([first]), &mut IdSource::seeded(SEED)).unwrap();
    assert!(beside.id != first && beside.sheets[0].id != beside.id, "the design's drawing keeps its id");
    let thin = DrawingBorderProfile { drawing_border_line_mm: 0.0, ..DrawingBorderProfile::iso() };
    let broken = [("", "en", "Sheet", None), ("Bare", "en_GB", "Sheet", None), ("Bare", "en", "\n", None), ("Bare", "en", "Sheet", Some(thin))];
    for (name, language, sheet, border) in broken {
        let made = Drawing::new(name, language, sheet, a4, border, &BTreeSet::new(), &mut IdSource::seeded(SEED));
        assert!(made.is_err(), "{name:?} {language:?} {sheet:?} {border:?}");
    }
}

/// Hostile input never panics: a valid file's text cut, overwritten and given lies, and its JSON
/// values replaced, repeated and dropped. Each read gives an error, or a drawing that keeps the
/// rules and reads back as it saves.
#[test]
fn mutated_files_give_an_error_or_a_valid_drawing_never_a_panic() {
    let good = plate().to_json().unwrap();
    let tree: Value = serde_json::from_str(&good).unwrap();
    let nodes = count(&tree) as u64;
    let lies = [
        json!(null),
        json!(0),
        json!(-1),
        json!(1),
        json!(1e10),
        json!(u64::MAX),
        json!(""),
        json!("x".repeat(300)),
        json!("0000000000000"),
        json!("0000000000001"),
        json!("fzzzzzzzzzzzz"),
        json!("FZZZZZZZZZZZZ"),
        json!([]),
        json!({}),
        json!(true),
        json!("follow_model"),
        json!({"fixed": {"unit": "deg"}}),
    ];
    let words = ["NaN", "1e400", "-0", "18446744073709551616", "\"", "{", "]", ",", "null", "\u{0}", "é", "o", "I"];
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut read_back = 0;
    for i in 0..10_000 {
        let text = if i % 2 == 0 {
            let mut v = tree.clone();
            let lie = &lies[(rnd() % lies.len() as u64) as usize];
            mutate(&mut v, &mut ((rnd() % nodes) as usize), lie, rnd() % 3);
            v.to_string()
        } else {
            let mut s = good.clone().into_bytes();
            for _ in 0..1 + rnd() % 3 {
                // A run of up to 15 bytes taken out, overwritten by one byte or replaced by a lie.
                let at = (rnd() % (s.len() as u64 + 1)) as usize;
                let cut = ((rnd() % 16) as usize).min(s.len() - at);
                let byte = [(rnd() >> 56) as u8];
                let put: &[u8] = match rnd() % 3 {
                    0 => &[],
                    1 => &byte,
                    _ => words[(rnd() % words.len() as u64) as usize].as_bytes(),
                };
                let next = [&s[..at], put, &s[at + cut..]].concat();
                s = next;
                if rnd() % 8 == 0 {
                    s.truncate(at);
                }
            }
            String::from_utf8_lossy(&s).into_owned()
        };
        if let Ok(d) = Drawing::from_json(&text) {
            assert!(d.validate().is_ok() && d.ids().all(|id| id != 0), "{text}");
            assert_eq!(Drawing::from_json(&d.to_json().unwrap()).unwrap(), d, "{text}");
            read_back += 1;
        }
    }
    assert!(read_back > 50, "only {read_back} mutated files read: the mutations hardly reach the record's rules");
}

/// A design holding drawings, as plain JSON: each drawing inline under `drawings`, as its own
/// record with its own format. While the change is pending the design names `drawings` in its
/// `format_pending`, so a build without it refuses the design (`format::tests`) rather than open it
/// and save it without them. They read back in their order, by name, then id, and save as the same
/// bytes; a design without drawings writes no `drawings`.
#[test]
fn a_design_keeps_its_drawings_inline_in_plain_json() {
    let named = |name: &str, id: u64| Arc::new(Drawing { name: name.into(), id, ..plate() });
    let mut doc = crate::Document::new("Plate");
    doc.drawings = vec![named("Details", 7), Arc::new(plate()), named("Details", 3), named("Assembly", u64::MAX)];
    let read = crate::Document::from_json(&doc.to_json()).unwrap();
    let order: Vec<_> = read.drawings.iter().map(|d| (d.name.as_str(), d.id)).collect();
    assert_eq!(order, [("Assembly", u64::MAX), ("Details", 3), ("Details", 7), ("Plate assembly", plate().id)]);
    let saved = read.to_json();
    assert_eq!(crate::Document::from_json(&saved).unwrap().to_json(), saved, "save, open, save: the same bytes");
    let v: Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(v["drawings"][3], file(&plate()));
    // Drawings in a design are a change of design format 59 (folded for 0.45.0-rc.1): a design
    // that holds drawings is written at 59 or later, and no longer names the change as pending.
    let format = v["format"].as_str().and_then(crate::format::version_of);
    assert!(format.is_some_and(|n| n >= 59), "{}", v["format"]);
    assert!(!v["format_pending"].as_array().is_some_and(|names| names.contains(&json!("drawings"))), "{}", v["format_pending"]);
    let empty: Value = serde_json::from_str(&crate::Document::new("Plate").to_json()).unwrap();
    assert_eq!(empty.get("drawings"), None);
}

/// What a design refuses in its drawings, each time the whole design, never by dropping a drawing:
/// a damaged one,
/// `drawings` that isn't a list, two with one id, and more than a design may hold, counted before
/// any is read. A drawing a newer build saved (a newer number, or a change this build doesn't know)
/// opens with what this build can read of it, as a newer design does (#12166). A drawing's numbers are its own family's to check: other tools' data in its
/// properties is kept, a sheet's number out of range refused.
#[test]
fn a_drawing_a_design_cant_read_refuses_the_design() {
    let mut doc = crate::Document::new("Plate");
    doc.drawings = vec![Arc::new(plate())];
    let design: Value = serde_json::from_str(&doc.to_json()).unwrap();
    let refused = |edit: &dyn Fn(&mut Value)| {
        let mut v = design.clone();
        edit(&mut v);
        crate::Document::from_json(&v.to_string()).map_or_else(|e| e.to_string(), |d| format!("read {}", d.drawings.len()))
    };
    assert_eq!(refused(&|v| v["drawings"][0]["format"] = json!(DRAWING.format_string(DRAWING.current() + 1))), "read 1");
    assert_eq!(refused(&|v| v["drawings"][0]["format_pending"] = json!(["a-later-change"])), "read 1");
    let damaged = refused(&|v| v["drawings"][0]["language"] = json!("e"));
    assert!(damaged.starts_with("drawing 1: damaged drawing: the language"), "{damaged}");
    assert_eq!(refused(&|v| v["drawings"] = json!({})), "damaged design: `drawings` is not a list");
    let twice = refused(&|v| {
        let again = v["drawings"][0].clone();
        v["drawings"].as_array_mut().unwrap().push(again);
    });
    assert_eq!(twice, format!("damaged design: two drawings have the id {}", ids::encode(plate().id)));
    let at_cap: Vec<Value> = (1..=MAX_DRAWINGS as u64)
        .map(|id| {
            let mut d = file(&plate());
            d["id"] = json!(ids::encode(id));
            d
        })
        .collect();
    assert_eq!(refused(&|v| v["drawings"] = Value::Array(at_cap.clone())), format!("read {MAX_DRAWINGS}"));
    let over = refused(&|v| v["drawings"] = Value::Array(vec![json!({}); MAX_DRAWINGS + 1]));
    assert_eq!(over, format!("damaged design: a design holds at most {MAX_DRAWINGS} drawings"));
    assert_eq!(refused(&|v| v["drawings"][0]["properties"]["com.example"] = json!({"huge": 1e300})), "read 1");
    let out_of_range = refused(&|v| v["drawings"][0]["sheets"][0]["dpi"] = json!(1e300));
    assert!(out_of_range.starts_with("drawing 1: damaged drawing: the number "), "{out_of_range}");
}

/// A copy that must open again (an autosave, a crash report's design) keeps every drawing a read
/// takes: one breaking its own rules, the later of two with one id and those past the cap are left
/// out, each named with its rule.
#[test]
fn a_copy_that_must_open_leaves_out_only_the_drawings_a_read_refuses() {
    let numbered = |id: u64| Arc::new(Drawing { id, name: format!("Plate {id}"), ..plate() });
    let mut doc = crate::Document::new("Plate");
    doc.drawings = vec![numbered(1), numbered(2)];
    assert!(matches!(openable(&doc), (Cow::Borrowed(_), ref left_out) if left_out.is_empty()));

    let broken = Arc::new(Drawing { language: "?".into(), ..(*numbered(3)).clone() });
    let again = Arc::new(Drawing { name: "Plate 1 again".into(), ..(*numbered(1)).clone() });
    doc.drawings = vec![numbered(1), broken, again, numbered(4)];
    let (kept, left_out) = openable(&doc);
    assert_eq!(kept.drawings, [numbered(1), numbered(4)]);
    assert_eq!(left_out.len(), 2);
    let (three, one) = (ids::encode(3), ids::encode(1));
    assert!(left_out[0].starts_with(&format!(r#"drawing "Plate 3" ({three}) left out: the language is not"#)), "{left_out:?}");
    assert_eq!(left_out[1], format!(r#"drawing "Plate 1 again" ({one}) left out: two drawings have the id {one}"#));

    // A drawing left out takes no id from a later one.
    let broken_first = Arc::new(Drawing { language: "?".into(), ..(*numbered(1)).clone() });
    doc.drawings = vec![broken_first, numbered(1)];
    let (kept, left_out) = openable(&doc);
    assert_eq!(kept.drawings, [numbered(1)]);
    assert_eq!(left_out.len(), 1, "{left_out:?}");

    doc.drawings = (1..=MAX_DRAWINGS as u64 + 2).map(numbered).collect();
    let (kept, left_out) = openable(&doc);
    assert_eq!(kept.drawings, doc.drawings[..MAX_DRAWINGS]);
    let over = |id: u64| format!(r#"drawing "Plate {id}" ({}) left out: a design holds at most {MAX_DRAWINGS} drawings"#, ids::encode(id));
    assert_eq!(left_out, [over(MAX_DRAWINGS as u64 + 1), over(MAX_DRAWINGS as u64 + 2)]);
}

/// The number of values in `v`, itself included.
fn count(v: &Value) -> usize {
    1 + match v {
        Value::Array(a) => a.iter().map(count).sum::<usize>(),
        Value::Object(o) => o.values().map(count).sum(),
        _ => 0,
    }
}

/// Change the `pick`th value of `v`, depth first: `how` 1 repeats a list's first element (two
/// objects with one id), 2 drops an object's first field, and otherwise `lie` takes its place.
fn mutate(v: &mut Value, pick: &mut usize, lie: &Value, how: u64) -> bool {
    if *pick == 0 {
        match (v, how) {
            (Value::Array(a), 1) if !a.is_empty() => {
                let first = a[0].clone();
                a.push(first);
            }
            (Value::Object(o), 2) if !o.is_empty() => {
                let key = o.keys().next().cloned().unwrap_or_default();
                o.remove(&key);
            }
            (v, _) => *v = lie.clone(),
        }
        return true;
    }
    *pick -= 1;
    match v {
        Value::Array(a) => a.iter_mut().any(|x| mutate(x, pick, lie, how)),
        Value::Object(o) => o.values_mut().any(|x| mutate(x, pick, lie, how)),
        _ => false,
    }
}

/// A drawing a newer build saved opens, with what it can read, and saving keeps the rest.
#[test]
fn a_newer_builds_drawing_opens_and_keeps_what_it_cant_read() {
    let d = plate();
    let mut file: Value = file(&d);
    assert!(file.as_object().unwrap().keys().all(|key| RECORD_FIELDS.contains(&key.as_str())), "a new field goes in RECORD_FIELDS");
    let newer = DRAWING.format_string(DRAWING.current() + 1);
    file["format"] = json!(newer);
    // With nothing it can't read: the same drawing.
    let opened = Drawing::from_json(&file.to_string()).unwrap();
    assert_eq!(opened.sheets, d.sheets);
    assert!(opened.newer_file.as_ref().unwrap().notice().starts_with("Saved by a newer SolidCraft"));
    assert_eq!(serde_json::from_str::<Value>(&opened.to_json().unwrap()).unwrap()["format"], json!(newer));
    // A field and a second sheet it can't read stay out, named, and go back.
    file["future_views"] = json!([{"id": "a"}]);
    let mut future_sheet = file["sheets"][0].clone();
    future_sheet["name"] = json!("Future");
    future_sheet["future_layers"] = json!([1]);
    file["sheets"].as_array_mut().unwrap().push(future_sheet);
    let opened = Drawing::from_json(&file.to_string()).unwrap();
    assert_eq!(opened.sheets, d.sheets);
    let said = &opened.newer_file.as_ref().unwrap().not_understood;
    assert!(said.iter().any(|what| what.contains("future_views")) && said.iter().any(|what| what.contains("Future")), "{said:?}");
    let again: Value = serde_json::from_str(&opened.to_json().unwrap()).unwrap();
    assert_eq!(again["future_views"], file["future_views"]);
    assert_eq!(again["sheets"].as_array().unwrap().len(), d.sheets.len() + 1);
    assert_eq!(again["format"], json!(newer));
    // A source of another design's file reads; one it can't read (a field a later build adds) stays
    // out, named by its path, and goes back.
    let nut = json!({"id": ids::encode(2), "path": "parts/nut.sc"});
    let bolt = json!({"id": ids::encode(1), "path": "parts/bolt.sc", "future_digest": "ab"});
    file["sources"].as_array_mut().unwrap().extend([nut, bolt.clone()]);
    let opened = Drawing::from_json(&file.to_string()).unwrap();
    let nut = Source { id: 2, path: Some("parts/nut.sc".into()) };
    assert_eq!(opened.sources, [&[nut][..], &d.sources[..]].concat());
    assert_eq!(opened.own_source(), Some(d.sources[0].id));
    let said = &opened.newer_file.as_ref().unwrap().not_understood;
    assert!(said.iter().any(|what| what.contains("parts/bolt.sc")), "{said:?}");
    let again: Value = serde_json::from_str(&opened.to_json().unwrap()).unwrap();
    assert_eq!(again["sources"].as_array().unwrap().last(), Some(&bolt));
}

// W38a: original paper-mm payload, independent of any Drawing UI or model solver.
fn paper_sketch_file() -> Value {
    let mut d = file(&plate());
    let mut sketch = solidcraft_sketch::Sketch::new();
    let pts = [(15.0, 10.0), (55.0, 10.0), (55.0, 30.0), (15.0, 30.0)];
    for (i, (x, y)) in pts.into_iter().enumerate() {
        sketch.add_point(solidcore_record::Vec2::new(x, y), Some(&format!("p{i}"))).unwrap();
    }
    for i in 0..4 {
        sketch.add_line_pts(i + 1, (i + 1) % 4 + 1, Some(&format!("l{i}"))).unwrap();
    }
    sketch.add_constraint(solidcraft_sketch::ConstraintKind::Length { l: 0, value: 40.0 }, Some("Width".into())).unwrap();
    sketch.points[1].exprs = Some(Box::new(solidcraft_sketch::PointExprs { x: Some("Inset + 5 mm".into()), y: None }));
    d["sketches"] = json!([{"id": ids::encode(41), "owner": {"sheet": d["sheets"][0]["id"]}, "name":"Detail outline", "sketch": sketch}]);
    d
}

#[test]
fn sheet_sketch_records_round_trip_in_paper_millimetres() {
    let input = paper_sketch_file();
    let loaded = read(&input).unwrap();
    let saved = file(&loaded);
    assert_eq!(saved["sketches"], input["sketches"]);
    assert_eq!(saved["units"], json!({"fixed":{"unit":"in"}}));
    assert_eq!(saved["sketches"][0]["sketch"]["points"][2]["pos"], json!([55.0, 10.0]));
    assert_eq!(saved["sketches"][0]["sketch"]["constraints"][0]["value"], json!(40.0));
    assert_eq!(saved["sketches"][0]["sketch"]["constraints"][0]["param"], json!("Width"));
    assert_eq!(read(&saved).unwrap().to_json().unwrap(), loaded.to_json().unwrap());
}

#[test]
fn sheet_sketch_records_are_ordered_by_id_without_reordering_sheets_or_geometry() {
    let mut input = paper_sketch_file();
    let mut second = input["sketches"][0].clone();
    second["id"] = json!(ids::encode(40));
    second["owner"] = json!({"sheet": input["sheets"][1]["id"]});
    input["sketches"].as_array_mut().unwrap().push(second);
    let d = read(&input).unwrap();
    assert_eq!(d.sketches.iter().map(|s| s.id).collect::<Vec<_>>(), [40, 41]);
    assert_eq!(file(&d)["sheets"], input["sheets"]);
    let mut reverse = d.clone();
    reverse.sketches.reverse();
    assert_eq!(reverse.to_json().unwrap(), d.to_json().unwrap());
    assert_eq!(reverse.sketches[0].id, 41, "saving must not mutate the live record");
    assert_eq!(d.sheet_sketch(41).unwrap().owner, Owner::Sheet(d.sheets[0].id));
    assert!(bare().sheet_sketch(41).is_none(), "lookup must not fall back to another drawing");
}

#[test]
fn sheet_sketch_ids_share_the_drawing_namespace_and_owners_must_resolve() {
    let good = paper_sketch_file();
    for duplicate in [good["id"].clone(), good["sources"][0]["id"].clone(), good["sheets"][0]["id"].clone(), json!(ids::encode(0))] {
        let mut v = good.clone();
        v["sketches"][0]["id"] = duplicate;
        assert!(read(&v).is_err());
    }
    let mut v = good.clone();
    v["sketches"][0]["owner"] = json!({"sheet": good["sources"][0]["id"]});
    assert!(refusal(&v).contains("owner"));
    v["sketches"][0]["owner"] = json!({"sheet": ids::encode(999)});
    assert!(read(&v).is_err());
    let mut duplicate = good;
    let record = duplicate["sketches"][0].clone();
    duplicate["sketches"].as_array_mut().unwrap().push(record);
    assert!(read(&duplicate).is_err());

    let mut d = read(&paper_sketch_file()).unwrap();
    let mut generator = IdSource::seeded(777);
    let taken = generator.next(&BTreeSet::new()).unwrap();
    Arc::make_mut(&mut d.sketches[0]).id = taken;
    assert_ne!(d.allocate_id(&mut IdSource::seeded(777)).unwrap(), taken);
}

// W38b: a drawing sketch belongs to a sheet (paper millimetres) or to a view (model millimetres in
// the view's plane). The iso view on the second sheet has no projected views, dimensions or balloons.
fn iso_view(d: &Drawing) -> (u64, u64) {
    let view = d.views.iter().find(|view| view.sheet == d.sheets[1].id).unwrap();
    (view.id, view.sheet)
}

/// `paper_sketch_file` with a second sketch, id 40, owned by the iso view.
fn view_sketch_file() -> Value {
    let mut v = paper_sketch_file();
    let (view, _) = iso_view(&read(&v).unwrap());
    let mut on_view = v["sketches"][0].clone();
    on_view["id"] = json!(ids::encode(40));
    on_view["owner"] = json!({"view": ids::encode(view)});
    on_view["name"] = json!("Weld symbol outline");
    v["sketches"].as_array_mut().unwrap().insert(0, on_view);
    v
}

#[test]
fn drawing_sketches_belong_to_a_sheet_or_a_view_and_owners_must_resolve() {
    let good = view_sketch_file();
    let d = read(&good).unwrap();
    let (view, view_sheet) = iso_view(&d);
    assert_eq!(d.sketches.iter().map(|record| record.owner).collect::<Vec<_>>(), [Owner::View(view), Owner::Sheet(d.sheets[0].id)]);
    assert_eq!(d.sketches.iter().map(|record| d.sketch_sheet(record)).collect::<Vec<_>>(), [Some(view_sheet), Some(d.sheets[0].id)]);
    assert_eq!(file(&d)["sketches"], good["sketches"]);

    // An owner names one of this drawing's sheets or views, of the kind its tag says.
    let id = |n: u64| json!(ids::encode(n));
    let dimension = d.dimensions[0].id;
    let balloon = d.balloons.iter().next().unwrap().id;
    let wrong = [
        ("sheet", id(view)),
        ("sheet", id(dimension)),
        ("view", id(d.sheets[0].id)),
        ("view", id(dimension)),
        ("view", id(balloon)),
        ("view", good["sources"][0]["id"].clone()),
        ("view", good["id"].clone()),
        ("view", id(999)),
    ];
    for (tag, named) in wrong {
        let mut v = good.clone();
        v["sketches"][0]["owner"] = Value::Object(serde_json::Map::from_iter([(tag.to_string(), named.clone())]));
        let why = refusal(&v);
        assert!(why.contains("owner"), "{tag} {named}: {why}");
    }
    // A damaged owner is refused, never guessed: missing, two tags, an unknown tag, a bare id, and
    // the older `sheet` field beside or instead of it in a current file.
    let sheet = good["sheets"][0]["id"].clone();
    for broken in [json!({}), json!({"sheet": sheet, "view": id(view)}), json!({"note": sheet}), sheet.clone(), Value::Null] {
        let mut v = good.clone();
        v["sketches"][1]["owner"] = broken.clone();
        assert!(read(&v).is_err(), "{broken}");
    }
    let mut both = good.clone();
    both["sketches"][1]["sheet"] = sheet.clone();
    assert!(read(&both).is_err());
    let mut older_field = good;
    let record = older_field["sketches"][1].as_object_mut().unwrap();
    record.remove("owner");
    record.insert("sheet".into(), sheet);
    assert!(read(&older_field).is_err(), "a current file never takes the older field");
}

#[test]
fn a_view_sketch_goes_with_its_view_and_with_its_views_sheet() {
    let d = read(&view_sketch_file()).unwrap();
    let (view, view_sheet) = iso_view(&d);
    let with = d.with_contents(vec![view]);
    assert_eq!(with.sketches, [40]);
    assert_eq!(d.on_sheet(view_sheet).sketches, [40]);
    assert_eq!(d.on_sheet(d.sheets[0].id).sketches, [41]);

    let mut without_view = d.clone();
    without_view.remove_objects(&with);
    assert_eq!(without_view.sketches.iter().map(|record| record.id).collect::<Vec<_>>(), [41]);
    without_view.validate().unwrap();

    let mut without_sheet = d.clone();
    let on = without_sheet.remove_sheet(view_sheet);
    assert_eq!((on.views, on.sketches), (vec![view], vec![40]));
    without_sheet.validate().unwrap();

    // The view taken out without its sketch leaves an owner that doesn't resolve.
    let mut dangling = d.clone();
    let mut only_the_view = d.with_contents(vec![view]);
    only_the_view.sketches.clear();
    dangling.remove_objects(&only_the_view);
    let why = dangling.validate().unwrap_err().to_string();
    assert!(why.contains("owner"), "{why}");
}

/// A drawing saved before view sketches (its `format_pending` lacks `view-sketches`) opens with each
/// sheet sketch's `sheet` as its owner, keeps its id and geometry, and saves the same bytes again.
#[test]
fn an_older_drawings_sheet_sketches_open_with_their_sheet_as_owner() {
    let current = file(&read(&view_sketch_file()).unwrap());
    let mut older = current.clone();
    older["format_pending"] = json!(DRAWING.pending_names().into_iter().filter(|name| name != "view-sketches").collect::<Vec<_>>());
    let mut record = current["sketches"][1].clone();
    let object = record.as_object_mut().unwrap();
    object.remove("owner");
    object.insert("sheet".into(), current["sheets"][0]["id"].clone());
    older["sketches"] = json!([record]);

    let d = read(&older).unwrap();
    assert_eq!(d.sketches.iter().map(|record| (record.id, record.owner)).collect::<Vec<_>>(), [(41, Owner::Sheet(d.sheets[0].id))]);
    let saved = file(&d);
    assert_eq!(saved["sketches"], json!([current["sketches"][1]]));
    assert_eq!(saved["format_pending"], current["format_pending"]);
    let text = d.to_json().unwrap();
    assert_eq!(Drawing::from_json(&text).unwrap().to_json().unwrap(), text);
}

#[test]
fn sheet_sketches_keep_current_views_dimensions_balloons_and_revisions() {
    let mut d = read(&paper_sketch_file()).unwrap();
    let mut id_source = IdSource::seeded(7082);
    let revision = d.allocate_id(&mut id_source).unwrap();
    Arc::make_mut(&mut d.revisions).add(None, revision).unwrap();
    d.validate().unwrap();
    let saved = d.to_json().unwrap();
    let reopened = Drawing::from_json(&saved).unwrap();
    assert_eq!(reopened, d);
    assert_eq!(reopened.to_json().unwrap(), saved);
    assert_eq!((d.views.len(), d.dimensions.len(), d.balloons.len(), d.revisions.rows().len(), d.sketches.len()), (4, 3, 1, 1, 1));
    for existing in d
        .views
        .iter()
        .map(|record| record.id)
        .chain(d.dimensions.iter().map(|record| record.id))
        .chain(d.balloons.iter().map(|record| record.id))
        .chain(d.revisions.ids())
    {
        let mut duplicate = d.clone();
        Arc::make_mut(&mut duplicate.sketches[0]).id = existing;
        assert!(duplicate.validate().unwrap_err().to_string().contains("two objects have the id"));
        assert!(Drawing::from_json(&serde_json::to_string(&duplicate).unwrap()).is_err());
    }
    let mut without = d.clone();
    let sheet = d.sketch_sheet(&d.sketches[0]).unwrap();
    let on = without.remove_sheet(sheet);
    assert_eq!(on.sketches, vec![d.sketches[0].id]);
    assert!(!on.is_empty());
    assert!(without.sketches.is_empty());
    without.validate().unwrap();
    assert_eq!(without.revisions, d.revisions);
    assert_eq!(d.to_json().unwrap(), saved, "removing a sheet changed the original record");
}

#[test]
fn drawings_without_sketches_keep_their_old_shape_and_gain_only_the_pending_format() {
    let old = plate();
    let v = file(&old);
    assert!(v.get("sketches").is_none());
    assert!(read(&v).unwrap().sketches.is_empty());
    // Sheet sketches are a change of drawing format 2 (folded for 0.45.0-rc.1), no longer pending.
    assert!(DRAWING.current() >= 2 && !DRAWING.pending_names().iter().any(|n| n == "sheet-sketches"));
    let mut explicit_empty = v;
    explicit_empty["sketches"] = json!([]);
    assert!(file(&read(&explicit_empty).unwrap()).get("sketches").is_none());
}

#[test]
fn sheet_sketch_record_caps_run_before_typed_payload_decoding() {
    let base = paper_sketch_file();
    for (field, cap) in
        [("points", solidcraft_sketch::MAX_POINTS), ("curves", solidcraft_sketch::MAX_CURVES), ("constraints", solidcraft_sketch::MAX_CONSTRAINTS)]
    {
        let mut v = base.clone();
        // Null would fail typed SPoint/Curve/Constraint decoding; the size guard must run first.
        v["sketches"][0]["sketch"][field] = Value::Array(vec![Value::Null; cap + 1]);
        let why = refusal(&v);
        assert!(why.contains(&format!("{field} limit exceeded")), "{why}");
    }
    let mut too_many = base.clone();
    too_many["sketches"] = Value::Array(vec![Value::Null; MAX_SHEET_SKETCHES + 1]);
    assert!(refusal(&too_many).contains("too many sheet sketches"));
    for malformed in [Value::Null, json!({}), json!(true)] {
        let mut v = base.clone();
        v["sketches"] = malformed;
        assert!(read(&v).is_err());
    }
    let mut spline = base;
    spline["sketches"][0]["sketch"]["curves"][0] = json!({"type":"spline", "id":"s1", "pts":vec![0; solidcraft_sketch::MAX_SPLINE_POINTS + 1]});
    assert!(refusal(&spline).contains("spline point limit exceeded"));
}

#[test]
fn sheet_sketch_count_limit_is_inclusive_and_entity_budget_is_shared() {
    let mut v = paper_sketch_file();
    let template = v["sketches"][0].clone();
    let records: Vec<_> = (0..MAX_SHEET_SKETCHES)
        .map(|i| {
            let mut record = template.clone();
            record["id"] = json!(ids::encode(100 + i as u64));
            record["sketch"] = serde_json::to_value(solidcraft_sketch::Sketch::new()).unwrap();
            record
        })
        .collect();
    v["sketches"] = json!(records);
    assert_eq!(read(&v).unwrap().sketches.len(), MAX_SHEET_SKETCHES);
    // Individually allowed arrays must not multiply the drawing-wide budget.
    let payload = json!({"points":vec![Value::Null; solidcraft_sketch::MAX_POINTS]});
    let many: Vec<_> = (0..5).map(|i| json!({"id":ids::encode(100+i),"sketch":payload})).collect();
    v["sketches"] = json!(many);
    assert!(refusal(&v).contains("drawing sketch entity limit exceeded"));
}

#[test]
fn sheet_sketch_payload_refuses_bad_geometry_identity_and_constraints_without_repair() {
    let original = paper_sketch_file();
    let edits: Vec<(&str, Value)> = vec![
        ("/sketches/0/sketch/points/1/id", json!("origin")),
        ("/sketches/0/sketch/curves/0/id", json!("p0")),
        ("/sketches/0/sketch/points/0/fixed", json!(false)),
        ("/sketches/0/sketch/points/0/pos", json!([1.0, 0.0])),
        ("/sketches/0/sketch/curves/0/a", json!(9999)),
        ("/sketches/0/sketch/constraints/0/l", json!(9999)),
        ("/sketches/0/sketch/constraints/0/value", json!(-1.0)),
        ("/sketches/0/name", json!(" ")),
        ("/sketches/0/sketch/points/1/exprs/x", json!("x".repeat(4097))),
        ("/sketches/0/sketch/constraints/0/param", json!("\n")),
    ];
    for (pointer, bad) in edits {
        let mut v = original.clone();
        *v.pointer_mut(pointer).unwrap() = bad;
        assert!(read(&v).is_err(), "accepted {pointer}");
    }
    let mut d = read(&original).unwrap();
    Arc::make_mut(&mut d.sketches[0]).sketch.points[1].pos.x = f64::NAN;
    assert!(d.validate().is_err());
    assert!(d.to_json().is_err(), "NaN must not silently serialize as null");
    assert_eq!(original, paper_sketch_file(), "the oracle was not modified");
}

#[test]
fn sheet_sketch_records_refuse_unconnected_auxiliary_and_model_only_state_explicitly() {
    let original = paper_sketch_file();
    for field in ["links", "patterns", "offsets", "freeform_offsets", "wires", "equations"] {
        let mut v = original.clone();
        v["sketches"][0]["sketch"][field] = json!([null]);
        assert!(refusal(&v).contains("not supported yet"), "{field}");
    }
    for flag in ["slice", "three_d"] {
        let mut v = original.clone();
        v["sketches"][0]["sketch"]["view"] = json!({flag:true});
        assert!(refusal(&v).contains("model-only"));
    }
    let mut unknown = original;
    unknown["sketches"][0]["source_path"] = json!("another-design.solidcraft");
    assert!(refusal(&unknown).contains("unknown field"));
}

#[test]
fn inline_design_roundtrip_preserves_sketch_payload_without_model_parameter_creation() {
    let d = read(&paper_sketch_file()).unwrap();
    let mut doc = crate::Document::new("Source");
    let before = doc.to_json();
    doc.drawings.push(Arc::new(d));
    let reopened = crate::Document::from_json(&doc.to_json()).unwrap();
    assert_eq!(file(&reopened.drawings[0])["sketches"], paper_sketch_file()["sketches"]);
    let mut only_model = reopened;
    only_model.drawings.clear();
    assert_eq!(only_model.to_json(), before, "record IO must not create source parameters or features");
}

#[test]
fn sheet_sketch_json_mutations_are_errors_or_complete_reopenable_records() {
    let base = paper_sketch_file();
    let count = count(&base);
    for index in 0..count {
        for lie in [Value::Null, json!(true), json!("hostile"), json!(-1), json!(1e100)] {
            let mut v = base.clone();
            let mut pick = index;
            mutate(&mut v, &mut pick, &lie, 0);
            if let Ok(d) = read(&v) {
                d.validate().unwrap();
                let saved = d.to_json().unwrap();
                assert_eq!(Drawing::from_json(&saved).unwrap(), d);
            }
        }
    }
}

// W38a R2: the edit boundary uses the existing evaluator/solver, not a second sketcher.
fn sheet_edit_design() -> (crate::Document, u64) {
    use solidcraft_sketch::ConstraintKind;
    let mut drawing = read(&paper_sketch_file()).unwrap();
    let sketch = &mut Arc::make_mut(&mut drawing.sketches[0]).sketch;
    for l in [0, 2] {
        sketch.add_constraint(ConstraintKind::Horizontal { l }, None).unwrap();
    }
    for l in [1, 3] {
        sketch.add_constraint(ConstraintKind::Vertical { l }, None).unwrap();
    }
    sketch.add_constraint(ConstraintKind::Length { l: 1, value: 20.0 }, None).unwrap();
    let id = drawing.id;
    let mut doc = crate::Document::new("Read-only parameter source");
    doc.units = "in".into();
    doc.set_param("Width", "40", Some("mm"), None).unwrap();
    doc.set_param("Inset", "10", Some("mm"), None).unwrap();
    doc.put_drawing(Arc::new(drawing));
    (doc, id)
}

// Test-only container adoption of a staged Drawing edit. The production engine
// always publishes through Session::edit_drawing; no Document writer API remains.
fn edit_sheet_fixture<T>(
    doc: &mut crate::Document,
    drawing: u64,
    sketch: u64,
    change: impl FnOnce(&mut solidcraft_sketch::Sketch) -> Result<T>,
) -> Result<(T, SolvedSheetSketch)> {
    let mut owners = doc.drawings.iter().filter(|owner| owner.id == drawing);
    let owner = owners.next().ok_or_else(|| DocError::Invalid("missing drawing".into()))?;
    if owners.next().is_some() {
        return Err(DocError::Invalid("ambiguous drawing".into()));
    }
    let mut next = (**owner).clone();
    let out = next.edit_sheet_sketch(doc, sketch, change)?;
    next.validate()?;
    if next != **owner {
        doc.put_drawing(Arc::new(next));
    }
    Ok(out)
}

fn sheet_rectangle_size(sketch: &solidcraft_sketch::Sketch) -> (f64, f64) {
    (sketch.points[2].pos.dist(sketch.points[1].pos), sketch.points[3].pos.dist(sketch.points[2].pos))
}

#[test]
fn sheet_sketch_resolution_reads_source_values_without_changing_the_record_or_model() {
    let (doc, id) = sheet_edit_design();
    let before = doc.to_json();
    let owner = doc.drawing(id).unwrap().clone();
    let solved = doc.solve_sheet_sketch(id, 41).unwrap();
    assert_eq!((solved.drawing, solved.sheet, solved.id), (id, owner.sheets[0].id, 41));
    assert!(solved.report.keeps_orientation());
    assert_eq!(solved.report.dof, 0);
    let (width, height) = sheet_rectangle_size(&solved.sketch);
    assert!((width - 40.0).abs() < 1e-6 && (height - 20.0).abs() < 1e-6);
    assert_eq!(solved.sketch.points[1].pos, solidcore_record::Vec2::new(15.0, 10.0));
    assert_eq!(solved.sketch.points[1].exprs, owner.sketches[0].sketch.points[1].exprs);
    assert_eq!(solved.sketch.constraints[0].param.as_deref(), Some("Width"));
    assert_eq!(doc.to_json(), before);
    assert!(Arc::ptr_eq(doc.drawing(id).unwrap(), &owner));
}

#[test]
fn sheet_sketch_resolution_uses_fresh_parameters_and_paper_defaults_not_display_units() {
    let (mut doc, id) = sheet_edit_design();
    doc.set_param("Width", "2", Some("in"), None).unwrap();
    doc.set_param("Inset", "1", Some("in"), None).unwrap();
    let d = Arc::make_mut(doc.drawings.iter_mut().find(|d| d.id == id).unwrap());
    Arc::make_mut(&mut d.sketches[0]).sketch.points[1].exprs.as_mut().unwrap().x = Some("Inset + 5".into());
    let before = doc.to_json();
    let solved = doc.solve_sheet_sketch(id, 41).unwrap();
    let (width, height) = sheet_rectangle_size(&solved.sketch);
    assert!((width - 50.8).abs() < 1e-6 && (height - 20.0).abs() < 1e-6);
    // Inset is 25.4 mm, but the literal +5 is five paper mm, not five inches.
    assert!((solved.sketch.points[1].pos.x - 30.4).abs() < 1e-9);
    assert_eq!(doc.to_json(), before);
    doc.set_param("Width", "60", Some("mm"), None).unwrap();
    let solved_again = doc.solve_sheet_sketch(id, 41).unwrap();
    assert!((sheet_rectangle_size(&solved_again.sketch).0 - 60.0).abs() < 1e-6);
    assert_eq!(doc.drawing(id).unwrap().sketches[0].sketch.points[2].pos.x, 55.0, "read-only resolution never adopts a result");
}

#[test]
fn sheet_sketch_edit_solves_and_commits_only_its_own_drawing() {
    let (mut doc, id) = sheet_edit_design();
    let before = doc.clone();
    let mut other = (**doc.drawing(id).unwrap()).clone();
    other.id += 1;
    other.name = "Other drawing, same local sketch id".into();
    let other_id = other.id;
    doc.put_drawing(Arc::new(other));
    let other = doc.drawing(other_id).unwrap().clone();
    let (answer, solved) = edit_sheet_fixture(&mut doc, id, 41, |sketch| {
        sketch.constraints[0].param = None;
        sketch.constraints[0].kind.set_value(60.0);
        Ok("accepted")
    })
    .unwrap();
    assert_eq!(answer, "accepted");
    assert!((sheet_rectangle_size(&solved.sketch).0 - 60.0).abs() < 1e-6);
    assert_eq!(doc.drawing(id).unwrap().sketches[0].sketch, solved.sketch);
    assert_eq!(doc.params, before.params, "editing cannot create a source parameter");
    assert_eq!(doc.features, before.features);
    assert!(Arc::ptr_eq(doc.drawing(other_id).unwrap(), &other));
    assert_eq!(other.sketches[0].sketch.constraints[0].param.as_deref(), Some("Width"));
    let mut only_model = doc.clone();
    only_model.drawings = before.drawings.clone();
    assert_eq!(only_model.to_json(), before.to_json(), "no other document field changed");
    let reopened = crate::Document::from_json(&doc.to_json()).unwrap();
    assert_eq!(reopened.drawing(id).unwrap().sketches[0].sketch, solved.sketch);
}

#[test]
fn sheet_sketch_edit_refuses_bad_inputs_and_conflicts_atomically() {
    let (mut doc, id) = sheet_edit_design();
    let original = doc.to_json();
    let owner = doc.drawing(id).unwrap().clone();
    for kind in 0..5 {
        let result = edit_sheet_fixture(&mut doc, id, 41, |sketch| {
            match kind {
                0 => {
                    sketch.points[1].pos.x += 1.0;
                    return Err(DocError::Invalid("deliberately refused edit".into()));
                }
                1 => sketch.points[2].pos.x = f64::NAN,
                2 => sketch.curves[0].kind = solidcraft_sketch::CurveKind::Line { a: 99_999, b: 1 },
                3 => sketch.constraints[0].param = Some("Absent".into()),
                _ => {
                    sketch.add_constraint(solidcraft_sketch::ConstraintKind::Length { l: 0, value: 99.0 }, None)?;
                }
            }
            Ok(())
        });
        assert!(result.is_err(), "accepted failed edit {kind}");
        assert_eq!(doc.to_json(), original, "edit {kind} changed the stored document");
        assert!(Arc::ptr_eq(doc.drawing(id).unwrap(), &owner));
    }
}

#[test]
fn sheet_sketch_edit_does_not_publish_partial_expression_evaluation() {
    let (mut doc, id) = sheet_edit_design();
    let before = doc.to_json();
    let owner = doc.drawing(id).unwrap().clone();
    for expression in ["Absent + 1 mm", "90 deg", "1 / 0", "100000000000 mm", "("] {
        let result = edit_sheet_fixture(&mut doc, id, 41, |sketch| {
            sketch.points[1].exprs.as_mut().unwrap().x = Some("Inset + 15 mm".into());
            sketch.points[2].exprs = Some(Box::new(solidcraft_sketch::PointExprs { x: Some(expression.into()), y: None }));
            Ok(())
        });
        assert!(result.is_err(), "accepted {expression}");
        assert_eq!(doc.to_json(), before);
        assert!(Arc::ptr_eq(doc.drawing(id).unwrap(), &owner));
    }
}

#[test]
fn sheet_sketch_resolution_requires_a_declared_source_only_for_named_references() {
    let (mut doc, id) = sheet_edit_design();
    let d = Arc::make_mut(doc.drawings.iter_mut().find(|d| d.id == id).unwrap());
    // The shared plate fixture now has source-dependent view records. This
    // case isolates sheet expressions without leaving damaged view references.
    d.sources.clear();
    d.views.clear();
    d.dimensions.clear();
    d.balloons.clear();
    for sheet in &mut d.sheets {
        Arc::make_mut(sheet).documents = None;
    }
    d.validate().unwrap();
    let before = doc.to_json();
    let e = doc.solve_sheet_sketch(id, 41).unwrap_err().to_string();
    assert!(e.contains("declared design source"), "{e}");
    assert_eq!(doc.to_json(), before);
    let (_, solved) = edit_sheet_fixture(&mut doc, id, 41, |sketch| {
        sketch.constraints[0].param = None;
        sketch.points[1].exprs.as_mut().unwrap().x = Some("5".into());
        Ok(())
    })
    .unwrap();
    assert_eq!(solved.sketch.points[1].pos.x, 5.0, "bare values stay paper mm without a source");
    assert!((sheet_rectangle_size(&solved.sketch).0 - 40.0).abs() < 1e-6);
}

#[test]
fn sheet_sketch_source_errors_are_explicit_and_unrelated_errors_do_not_retarget_it() {
    let (mut doc, id) = sheet_edit_design();
    doc.set_param("Width", "90", Some("deg"), None).unwrap();
    assert!(doc.solve_sheet_sketch(id, 41).is_err());
    doc.set_param("Width", "40", Some("mm"), None).unwrap();
    let at = doc.params.iter().position(|p| p.name == "Width").unwrap();
    doc.params[at].expr = "Width + 1 mm".into();
    let before = doc.to_json();
    let error = doc.solve_sheet_sketch(id, 41).unwrap_err().to_string();
    assert!(error.contains("circular reference") && error.contains("Width"), "{error}");
    assert_eq!(doc.to_json(), before);
    let (_, solved) = edit_sheet_fixture(&mut doc, id, 41, |sketch| {
        sketch.constraints[0].param = None;
        Ok(())
    })
    .unwrap();
    assert!((sheet_rectangle_size(&solved.sketch).0 - 40.0).abs() < 1e-6, "unreferenced source error is not a fallback or a global blocker");
}

#[test]
fn sheet_sketch_target_is_exact_and_a_noop_preserves_the_record_arc() {
    let (mut doc, id) = sheet_edit_design();
    let before = doc.to_json();
    assert!(doc.solve_sheet_sketch(id + 1, 41).is_err());
    assert!(edit_sheet_fixture(&mut doc, id, 42, |_| Ok(())).is_err());
    assert_eq!(doc.to_json(), before);
    edit_sheet_fixture(&mut doc, id, 41, |_| Ok(())).unwrap();
    let owner = doc.drawing(id).unwrap().clone();
    let record = owner.sketches[0].clone();
    edit_sheet_fixture(&mut doc, id, 41, |_| Ok(())).unwrap();
    assert!(Arc::ptr_eq(doc.drawing(id).unwrap(), &owner));
    assert!(Arc::ptr_eq(&doc.drawing(id).unwrap().sketches[0], &record));
}

#[test]
fn sheet_sketch_resolution_refuses_ambiguous_drawing_identity() {
    let (mut doc, id) = sheet_edit_design();
    let owner = doc.drawing(id).unwrap().clone();
    doc.drawings.push(owner.clone());
    let before = doc.to_json();
    assert!(doc.solve_sheet_sketch(id, 41).unwrap_err().to_string().contains("ambiguous"));
    assert!(edit_sheet_fixture(&mut doc, id, 41, |_| Ok(())).is_err());
    assert_eq!(doc.to_json(), before);
    assert!(doc.drawings.iter().all(|drawing| Arc::ptr_eq(drawing, &owner)));
}

#[test]
fn sheet_sketch_resolution_refuses_parameter_binding_on_a_nondimension() {
    let (mut doc, id) = sheet_edit_design();
    let before = doc.to_json();
    let error = edit_sheet_fixture(&mut doc, id, 41, |sketch| {
        sketch.constraints[1].param = Some("Width".into());
        Ok(())
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("only a sheet sketch dimension"), "{error}");
    assert_eq!(doc.to_json(), before);
}

// Cam's W38a review: save has the same validation rule with or without sketches.
#[test]
fn saving_a_drawing_validates_its_record_even_without_sketches() {
    for with_sketch in [false, true] {
        let good = if with_sketch { read(&paper_sketch_file()).unwrap() } else { bare() };
        let before = good.to_json().unwrap();
        for problem in 0..5 {
            let mut damaged = good.clone();
            match problem {
                0 => damaged.name.clear(),
                1 => Arc::make_mut(&mut damaged.sheets[0]).dpi = f64::NAN,
                2 => damaged.sheets.clear(),
                3 => damaged.language = "???".into(),
                _ => damaged.units = Units::Fixed { unit: "deg".into() },
            }
            assert!(damaged.to_json().is_err(), "saved invalid record {problem}, sketches={with_sketch}");
        }
        assert_eq!(good.to_json().unwrap(), before);
        assert_eq!(Drawing::from_json(&before).unwrap(), good);
    }
}

#[test]
fn staged_sheet_sketch_edit_changes_only_the_given_drawing_not_the_source_design() {
    let (doc, id) = sheet_edit_design();
    let source = doc.to_json();
    let mut staged = (**doc.drawing(id).unwrap()).clone();
    let old_record = staged.sheet_sketch(41).unwrap().clone();
    let (_, solved) = staged
        .edit_sheet_sketch(&doc, 41, |sketch| {
            sketch.constraints[0].param = None;
            sketch.constraints[0].kind.set_value(60.0);
            Ok(())
        })
        .unwrap();
    staged.validate().unwrap();
    assert!((sheet_rectangle_size(&solved.sketch).0 - 60.0).abs() < 1e-6);
    assert!((sheet_rectangle_size(&staged.sheet_sketch(41).unwrap().sketch).0 - 60.0).abs() < 1e-6);
    assert_eq!(doc.to_json(), source, "a staged Drawing operation never publishes to the source design");
    assert!(Arc::ptr_eq(doc.drawing(id).unwrap().sheet_sketch(41).unwrap(), &old_record));
    assert_eq!(staged.sheet_sketch(41).unwrap().sketch.points[0].pos, solidcore_record::Vec2::ZERO);
    assert_eq!(staged.sheet_sketch(41).unwrap().owner, Owner::Sheet(staged.sheets[0].id));
}

#[test]
fn staged_sheet_sketch_edit_checks_the_cumulative_entity_cap_before_solving() {
    use solidcraft_sketch::{MAX_CONSTRAINTS, MAX_CURVES, MAX_POINTS, Sketch};
    let mut d = bare();
    let mut payload = Sketch::new();
    for i in 1..80 {
        payload.add_point(solidcore_record::Vec2::new(i as f64, 10.0), Some(&format!("p{i}"))).unwrap();
    }
    for i in 0..1000 {
        d.sketches.push(Arc::new(SheetSketch {
            id: 1000 + i,
            owner: Owner::Sheet(d.sheets[0].id),
            name: format!("Sketch {i}"),
            sketch: payload.clone(),
        }));
    }
    assert_eq!(d.sketches.len(), MAX_SHEET_SKETCHES);
    assert_eq!(d.sketches.iter().map(|s| s.sketch.points.len()).sum::<usize>(), MAX_POINTS + MAX_CURVES + MAX_CONSTRAINTS);
    d.validate().unwrap();
    let original = d.clone();
    let record = d.sheet_sketch(1000).unwrap().clone();
    let e = d
        .edit_sheet_sketch(&crate::Document::default(), 1000, |sketch| {
            sketch.add_point(solidcore_record::Vec2::new(3.0, 4.0), Some("over_cap"))?;
            Ok(())
        })
        .unwrap_err()
        .to_string();
    assert!(e.contains("entity") && e.contains("limit"), "{e}");
    assert_eq!(d, original);
    assert!(Arc::ptr_eq(d.sheet_sketch(1000).unwrap(), &record));
}

#[test]
fn sheet_sketch_validation_preserves_the_slot_work_allowance() {
    use solidcore_record::Vec2;
    use solidcraft_sketch::{Constraint, ConstraintKind as K, Sketch};
    let mut sk = Sketch::new();
    let line = sk.add_line(Vec2::new(20.0, 15.0), Vec2::new(60.0, 15.0), None, None, None).unwrap();
    let a = sk.add_arc(Vec2::new(20.0, 20.0), Vec2::new(20.0, 25.0), Vec2::new(20.0, 15.0), [None; 3], None).unwrap();
    let b = sk.add_arc(Vec2::new(60.0, 20.0), Vec2::new(60.0, 15.0), Vec2::new(60.0, 25.0), [None; 3], None).unwrap();
    // Stored redundant constraints are valid input. Bypass add_constraint's
    // current deduplication only in this hostile-size fixture, preserving the
    // original validation work at both sides of the production limit.
    for i in 0..703 {
        sk.constraints.push(Constraint { id: format!("budget_h{i}"), kind: K::Horizontal { l: line }, param: None, driven: false, text: None, old_join: false });
    }
    sk.add_constraint(K::Radius { c: a, value: 5.0 }, None).unwrap();
    sk.add_constraint(K::Radius { c: b, value: 5.0 }, None).unwrap();
    sk.add_constraint(K::SlotOverallLength { a, b, value: 50.0 }, None).unwrap();
    assert_eq!(sk.constraints.len(), 706);
    assert_eq!(4 * sk.constraints.len().pow(2) + sk.constraints.len() - 1, 1_994_449);
    let mut d = bare();
    d.sketches.push(Arc::new(SheetSketch { id: 41, owner: Owner::Sheet(d.sheets[0].id), name: "Slot".into(), sketch: sk }));
    // 705 ordinary charges plus 4*706^2 for the one slot: 1,994,449.
    d.validate().unwrap();
    let before = d.clone();
    let e = d
        .edit_sheet_sketch(&crate::Document::default(), 41, |sketch| {
            sketch.constraints.push(Constraint { id: "budget_h703".into(), kind: K::Horizontal { l: line }, param: None, driven: false, text: None, old_join: false });
            assert_eq!(sketch.constraints.len(), 707);
            assert_eq!(4 * sketch.constraints.len().pow(2) + sketch.constraints.len() - 1, 2_000_102);
            Ok(())
        })
        .unwrap_err()
        .to_string();
    // Adding one ordinary constraint makes the same conservative charge 2,000,102.
    assert!(e.contains("validation work"), "{e}");
    assert_eq!(d, before);
}

#[test]
fn staged_sheet_sketch_edit_refuses_an_invalid_original_before_running_the_callback() {
    let (doc, id) = sheet_edit_design();
    let mut d = (**doc.drawing(id).unwrap()).clone();
    Arc::make_mut(&mut d.sketches[0]).sketch.curves[0].kind = solidcraft_sketch::CurveKind::Line { a: 99_999, b: 1 };
    let before = d.clone();
    let mut called = false;
    assert!(
        d.edit_sheet_sketch(&doc, 41, |_| {
            called = true;
            Ok(())
        })
        .is_err()
    );
    assert!(!called, "unsafe original data must be refused before a callback sees it");
    assert_eq!(d, before);
}

fn w38d_record_fixture(hole: bool) -> (crate::Document, u64, crate::WholeProfileBoundary) {
    let (mut doc, id) = sheet_edit_design();
    let mut drawing = (**doc.drawing(id).unwrap()).clone();
    if hole {
        Arc::make_mut(&mut drawing.sketches[0]).sketch.add_circle(Vec2::new(35.0, 20.0), 3.0, None, Some("hole")).unwrap();
    }
    let boundary = drawing
        .sheet_fill_profiles(&doc, 41)
        .unwrap()
        .profiles
        .into_iter()
        .find(|profile| profile.boundary.holes.len() == usize::from(hole))
        .unwrap()
        .boundary;
    let region = drawing.sheet_fill_region(&doc, 41, &boundary).unwrap();
    drawing.patterns.push(Arc::new(PatternSnapshot { id: 71, pattern: solidcore_record::hatch::Pattern::lines_45() }));
    drawing.fills.push(Arc::new(SketchFill { id: 70, sketch: 41, boundary: boundary.clone(), paint: FillPaint::Solid, last_region: region }));
    drawing.validate().unwrap();
    doc.put_drawing(Arc::new(drawing));
    (doc, id, boundary)
}

#[test]
fn sheet_fill_records_keep_sorted_snapshots_global_ids_and_sheet_ownership() {
    let (doc, id, _) = w38d_record_fixture(true);
    let mut drawing = (**doc.drawing(id).unwrap()).clone();
    let mut second = (*drawing.fills[0]).clone();
    second.id = 73;
    second.paint = FillPaint::Hatch { pattern: 71, pen_width_mm: 0.25 };
    drawing.fills.insert(0, Arc::new(second));
    drawing.patterns.insert(0, Arc::new(PatternSnapshot { id: 72, pattern: drawing.patterns[0].pattern.clone() }));
    let value = file(&drawing);
    assert_eq!(value["fills"][0]["id"], ids::encode(70));
    assert_eq!(value["patterns"][0]["id"], ids::encode(71));
    assert_eq!(value["fills"][1]["paint"]["pattern"], ids::encode(71));
    let reopened = read(&value).unwrap();
    assert_eq!(reopened.fills.iter().map(|fill| fill.id).collect::<Vec<_>>(), [70, 73]);
    assert_eq!(reopened.patterns[0].pattern.name, reopened.patterns[1].pattern.name, "duplicate imported names have independent ids");
    assert!(reopened.object_ids().any(|id| id == 70) && reopened.object_ids().any(|id| id == 72));
    let mut collision = reopened.clone();
    Arc::make_mut(&mut collision.patterns[0]).id = 70;
    assert!(collision.validate().unwrap_err().to_string().contains("two objects have the id"));
    let sheet = reopened.sketch_sheet(&reopened.sketches[0]).unwrap();
    let mut removed = reopened.clone();
    let on = removed.remove_sheet(sheet);
    assert_eq!(on.fills, [70, 73]);
    assert!(removed.fills.is_empty() && removed.sketches.is_empty());
    assert_eq!(removed.patterns, reopened.patterns, "patterns belong to the drawing, not a removed sheet");
    removed.validate().unwrap();
    assert_eq!(file(&reopened), value);
}

/// A fill's region is in its sketch's units and goes on paper as it is. A view sketch's are model
/// millimetres in its view, so a fill on one is refused until fills are placed through their view.
#[test]
fn a_fill_on_a_view_sketch_is_refused_until_fills_go_through_their_view() {
    let (doc, id, _) = w38d_record_fixture(false);
    let mut drawing = (**doc.drawing(id).unwrap()).clone();
    let (view, _) = iso_view(&drawing);
    Arc::make_mut(&mut drawing.sketches[0]).owner = Owner::View(view);
    let why = drawing.validate().unwrap_err().to_string();
    assert!(why.contains("not supported yet: a fill on a view sketch"), "{why}");
    drawing.fills.clear();
    drawing.validate().unwrap();
}

#[test]
fn sheet_fill_deleted_hole_keeps_the_complete_old_region_instead_of_outer_only() {
    let (doc, id, boundary) = w38d_record_fixture(true);
    let mut drawing = (**doc.drawing(id).unwrap()).clone();
    let region = drawing.fills[0].last_region.clone();
    assert!((region.area() - (800.0 - std::f64::consts::PI * 9.0)).abs() < 1e-7);
    Arc::make_mut(&mut drawing.sketches[0]).sketch.remove_curves(&[4]);
    drawing.refresh_fill_regions(&doc, &std::collections::BTreeSet::from([41])).unwrap();
    let resolved = drawing.resolve_sheet_fill(&doc, 70).unwrap();
    assert!(matches!(resolved.status, FillRegionStatus::LostBoundary(_)));
    assert_eq!(resolved.region, region);
    assert_eq!(drawing.fills[0].last_region, region);
    let outer_only = crate::WholeProfileBoundary { outer: boundary.outer, holes: vec![] };
    assert!((drawing.sheet_fill_region(&doc, 41, &outer_only).unwrap().area() - 800.0).abs() < 1e-7);
    let reopened = Drawing::from_json(&drawing.to_json().unwrap()).unwrap();
    assert_eq!(reopened.resolve_sheet_fill(&doc, 70).unwrap().region, region);
    assert!(matches!(reopened.resolve_sheet_fill(&doc, 70).unwrap().status, FillRegionStatus::LostBoundary(_)));
}

#[test]
fn sheet_fill_source_refresh_uses_prospective_source_and_preserves_latest_region_on_loss() {
    let (before, id, _) = w38d_record_fixture(false);
    let original = before.drawing(id).unwrap();
    let mut after = before.clone();
    after.set_param("Width", "60", Some("mm"), None).unwrap();
    let mut prospective = (**original).clone();
    prospective.refresh_fill_regions_after_source_change(original, &before, &after).unwrap();
    assert!((prospective.fills[0].last_region.area() - 1200.0).abs() < 1e-7);
    assert!(
        (after.drawing(id).unwrap().fills[0].last_region.area() - 800.0).abs() < 1e-7,
        "the prospective Drawing must not be looked up through the old published record"
    );
    let newest = prospective.fills[0].last_region.clone();
    Arc::make_mut(&mut prospective.sketches[0]).sketch.remove_curves(&[0]);
    prospective.refresh_fill_regions(&after, &std::collections::BTreeSet::from([41])).unwrap();
    let reopened = Drawing::from_json(&prospective.to_json().unwrap()).unwrap();
    let retained = reopened.resolve_sheet_fill(&after, 70).unwrap();
    assert!(matches!(retained.status, FillRegionStatus::LostBoundary(_)));
    assert_eq!(retained.region, newest);
    assert_eq!(retained.region, reopened.fills[0].last_region);
}

#[test]
fn sheet_fill_source_failure_is_an_error_while_storage_keeps_the_retention_snapshot() {
    let (mut doc, id, _) = w38d_record_fixture(false);
    // A valid source exists before it disappears. set_param correctly refuses
    // unknown expressions; removing a parameter used only by a read-only
    // Drawing is the actual source-loss transition this consumer must handle.
    doc.set_param("missing_source_value", "Width", Some("mm"), None).unwrap();
    let mut record = (**doc.drawing(id).unwrap()).clone();
    let constraint = Arc::make_mut(&mut record.sketches[0]).sketch.constraints.iter_mut().find(|c| c.param.as_deref() == Some("Width")).unwrap();
    constraint.param = Some("missing_source_value".into());
    record.validate().unwrap();
    doc.put_drawing(Arc::new(record));
    let drawing = doc.drawing(id).unwrap().clone();
    let retained = drawing.fills[0].last_region.clone();
    let current = drawing.resolve_sheet_fill(&doc, 70).unwrap();
    assert!(matches!(current.status, FillRegionStatus::Current));
    assert_eq!(current.region, retained);
    doc.remove_param("missing_source_value").unwrap();
    assert!(doc.param("missing_source_value").is_none());
    assert!(drawing.resolve_sheet_fill(&doc, 70).unwrap_err().to_string().contains("missing_source_value"));
    let mut candidate = (*drawing).clone();
    candidate.refresh_fill_regions(&doc, &std::collections::BTreeSet::from([41])).unwrap();
    assert_eq!(candidate.fills[0].last_region, retained);
    assert!(Drawing::from_json(&candidate.to_json().unwrap()).unwrap().resolve_sheet_fill(&doc, 70).is_err());
}

#[test]
fn sheet_fill_raw_nested_caps_refuse_before_typed_pattern_and_identity_admission() {
    let (doc, id, _) = w38d_record_fixture(false);
    let base = file(doc.drawing(id).unwrap());
    let cases = [
        ("`fills`", {
            let mut value = base.clone();
            value["fills"] = json!(vec![base["fills"][0].clone(); 1001]);
            value
        }),
        ("`patterns`", {
            let mut value = base.clone();
            value["patterns"] = json!(vec![base["patterns"][0].clone(); 1001]);
            value
        }),
        ("families", {
            let mut value = base.clone();
            value["patterns"][0]["pattern"]["families"] = json!(vec![base["patterns"][0]["pattern"]["families"][0].clone(); 65]);
            value
        }),
        ("dashes", {
            let mut value = base.clone();
            value["patterns"][0]["pattern"]["families"][0]["dashes"] = json!([1, 1, 1, 1, 1, 1, 1]);
            value
        }),
        ("name", {
            let mut value = base.clone();
            value["patterns"][0]["pattern"]["name"] = json!("A".repeat(32));
            value
        }),
        ("description", {
            let mut value = base.clone();
            value["patterns"][0]["pattern"]["description"] = json!("🦀".repeat(81));
            value
        }),
        ("512 curve ids", {
            let mut value = base.clone();
            value["fills"][0]["boundary"]["outer"] = json!(vec!["l0"; 513]);
            value
        }),
        ("128 holes", {
            let mut value = base.clone();
            value["fills"][0]["boundary"]["holes"] = json!(vec![vec!["hole"]; 129]);
            value
        }),
        ("invalid curve id", {
            let mut value = base.clone();
            value["fills"][0]["boundary"]["outer"][0] = json!("x".repeat(513));
            value
        }),
        ("retained region", {
            let mut value = base.clone();
            value["fills"][0]["last_region"]["holes"] = json!(vec![json!({"segs":[]}); 129]);
            value
        }),
        ("8192", {
            let mut value = base.clone();
            value["fills"][0]["last_region"]["outer"]["segs"] = json!(vec![base["fills"][0]["last_region"]["outer"]["segs"][0].clone(); 8193]);
            value
        }),
    ];
    for (key, value) in cases {
        let error = read(&value).unwrap_err().to_string();
        assert!(error.contains("raw") && error.contains(key), "{key}: {error}");
    }
    let mut value = base.clone();
    let ids: Vec<_> = (0..512).map(|index| format!("curve{index:04}")).collect();
    value["fills"][0]["boundary"]["outer"] = json!(ids);
    value["fills"] = json!(vec![value["fills"][0].clone(); 40]);
    assert!(read(&value).unwrap_err().to_string().contains("raw boundaries exceed 20000"));
}

#[test]
fn sheet_fill_typed_caps_cover_zero_piece_holes_and_cumulative_identity_storage() {
    let (doc, id, _) = w38d_record_fixture(false);
    let original = doc.drawing(id).unwrap();
    let mut holes = (**original).clone();
    Arc::make_mut(&mut holes.fills[0]).last_region.holes = vec![solidcore_record::Loop2 { segs: vec![] }; 129];
    assert!(holes.validate().unwrap_err().to_string().contains("128 holes"));
    let mut families = (**original).clone();
    let family = families.patterns[0].pattern.families[0].clone();
    Arc::make_mut(&mut families.patterns[0]).pattern.families = vec![family; 65];
    assert!(families.validate().unwrap_err().to_string().contains("64 families"));
    let mut fills = (**original).clone();
    let template = (*fills.fills[0]).clone();
    fills.fills.clear();
    let mut fill_ids = IdSource::seeded(7062);
    for _ in 0..1000 {
        let id = fills.allocate_id(&mut fill_ids).unwrap();
        fills.fills.push(Arc::new(SketchFill { id, ..template.clone() }));
    }
    fills.validate().unwrap();
    let fill_id = fills.allocate_id(&mut fill_ids).unwrap();
    fills.fills.push(Arc::new(SketchFill { id: fill_id, ..template }));
    assert_eq!(fills.fills.len(), 1001);
    assert!(fills.validate().unwrap_err().to_string().contains("1000 fills"));
    let mut too_many = (**original).clone();
    let mut fill = (*too_many.fills[0]).clone();
    fill.boundary.outer = (0..512).map(|index| format!("curve{index:04}")).collect();
    too_many.fills = (1000..1040).map(|id| Arc::new(SketchFill { id, ..fill.clone() })).collect();
    assert!(too_many.validate().unwrap_err().to_string().contains("20000 total curve ids"));
    let mut pieces = (**original).clone();
    let first = pieces.fills[0].last_region.outer.segs[0];
    Arc::make_mut(&mut pieces.fills[0]).last_region.outer.segs = vec![first; 8193];
    assert!(pieces.validate().unwrap_err().to_string().contains("8192"));
    assert_eq!(doc.drawing(id).unwrap(), original);
}

#[test]
fn sheet_fill_profile_search_refuses_shared_conic_clamping_and_tiny_arc_widening() {
    use solidcraft_sketch::{CurveKind, Sketch};
    let (doc, id, _) = w38d_record_fixture(false);
    let original = doc.drawing(id).unwrap();
    for conic in [false, true] {
        let mut drawing = (**original).clone();
        let mut sketch = Sketch::new();
        let a = sketch.add_point(Vec2::new(1000.0, 0.0), Some("start")).unwrap();
        let b = sketch.add_point(Vec2::new(1000.0, 5e-10), Some("end")).unwrap();
        let apex = sketch.add_point(Vec2::new(1010.0, 10.0), Some("apex")).unwrap();
        let kind = if conic { CurveKind::Conic { a, b, apex, rho: 0.5 } } else { CurveKind::Arc { c: 0, a, b } };
        sketch.add_curve(kind, Some("unsafe_exact_curve")).unwrap();
        for point in &mut sketch.points {
            point.fixed = true;
        }
        sketch.curves[0].fixed = true;
        Arc::make_mut(&mut drawing.sketches[0]).sketch = sketch;
        drawing.validate().unwrap();
        let mut raw = file(&drawing);
        if conic {
            // The editing constructor deliberately admits a narrower rho
            // range. Persisted SheetSketch records admit 0<rho<1; exercise that
            // real file boundary before exact profile conversion refuses it.
            raw["sketches"][0]["sketch"]["curves"][0]["rho"] = json!(1.0 - 1e-10);
        }
        let reopened = read(&raw).unwrap();
        let source = &reopened.sketches[0].sketch;
        if conic {
            let CurveKind::Conic { rho, .. } = source.curves[0].kind else { panic!("missing authored conic") };
            assert!(rho > 0.0 && rho < 1.0 && 1.0 - rho < 1e-9);
            let pieces = source.exact_segs(0).unwrap();
            let solidcore_record::Seg2::Conic { w, .. } = pieces[0] else { panic!("missing shared conic") };
            assert!(w < rho / (1.0 - rho), "precondition: shared conversion clamps the actual authored weight");
        } else {
            let CurveKind::Arc { c, a, b } = source.curves[0].kind else { panic!("missing authored arc") };
            let (center, start, end) = (source.point(c).unwrap(), source.point(a).unwrap(), source.point(b).unwrap());
            assert_ne!(start, end);
            let authored = ((end - center).angle() - (start - center).angle()).rem_euclid(std::f64::consts::TAU);
            assert!(authored > 0.0 && authored <= 1e-12);
            let pieces = source.segs(0);
            let solidcore_record::Seg2::Arc { sweep, .. } = pieces[0] else { panic!("missing shared arc") };
            assert!(sweep >= std::f64::consts::TAU, "precondition: shared conversion widens the actual noncoincident tiny arc");
        }
        let why = reopened.sheet_fill_profiles(&doc, 41).unwrap_err().to_string();
        assert!(why.contains("unsafe_exact_curve") && why.contains(if conic { "conic rho" } else { "arc sweep" }), "{why}");
        assert!(reopened.resolve_sheet_fill(&doc, 70).is_err(), "normalization refusal must be Failed, never LostBoundary");
    }
}

#[test]
fn sheet_sketch_unsupported_equations_and_centerlines_are_refused_before_solving_or_saving() {
    for centerline in [false, true] {
        let mut drawing = read(&paper_sketch_file()).unwrap();
        let record = Arc::make_mut(&mut drawing.sketches[0]);
        let expected = if centerline {
            record.sketch.curves[0].centerline = true;
            "centerline"
        } else {
            record.sketch.equations.push(solidcraft_sketch::EquationCurve {
                curve: record.sketch.curves[0].id.clone(),
                x: None,
                y: "x".into(),
                start: "0".into(),
                end: "1".into(),
                tolerance: 0.01,
                variable_unit: solidcraft_sketch::VariableUnit::Number,
            });
            "equations"
        };
        assert!(drawing.validate().unwrap_err().to_string().contains(expected));
        assert!(drawing.to_json().unwrap_err().to_string().contains(expected));
        assert!(drawing.solve_sheet_sketch(&crate::Document::default(), 41).unwrap_err().to_string().contains(expected));
        let raw = serde_json::to_value(&drawing).unwrap();
        assert!(refusal(&raw).contains(expected));
    }
}

/// A drawing saved with a projection cell at the old smallest room (15.4 by 7.35 mm, which held the
/// 3.5 mm symbol before it grew to 5.5 h by 2.5 h) opens as before, and its cell holds the 2.5 mm symbol.
#[test]
fn a_drawing_saved_with_the_old_smallest_symbol_cell_opens() {
    let mut d = Drawing::new(
        "Old block",
        "en",
        "Sheet",
        drawing_sheet::Sheet::standard(PaperSize::A3, Orientation::Landscape),
        Some(DrawingBorderProfile::iso()),
        &BTreeSet::new(),
        &mut IdSource::seeded(SEED),
    )
    .unwrap();
    let mut block = solidcraft_geom::title_block::default_block();
    let cell = &mut block.cells[11];
    assert_eq!(cell.name, "projection");
    cell.label = Some(solidcraft_geom::title_block::Label { text: "Projection".into(), cap_mm: 4.125 });
    cell.rect_mm[0] = -17.925;
    Arc::make_mut(&mut d.sheets[0]).title_block = Some(block);
    let read = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    let layout = read.sheets[0].title_block_layout().unwrap().unwrap();
    assert_eq!(layout.cells.iter().filter_map(|c| c.symbol_mm).collect::<Vec<_>>(), [2.5]);
}
