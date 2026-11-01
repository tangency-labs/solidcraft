use serde_json::json;
use solidcraft_geom::drawing_measure::{LinearDirection, Reading};
use solidcraft_geom::drawing_sheet::{Margins, Orientation, PaperSize, SheetSize};
use solidcraft_geom::drawing_view::{NamedView, ProjectedSide};

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
fn plate() -> Drawing {
    let mut id_source = IdSource::seeded(SEED);
    let a3 = drawing_sheet::Sheet::standard(PaperSize::A3, Orientation::Landscape);
    let mut d = Drawing::new("Plate assembly", "en-GB", "Assembly", a3, Some(DrawingBorderProfile::iso()), &BTreeSet::new(), &mut id_source).unwrap();
    let id = d.allocate_id(&mut id_source).unwrap();
    d.sources.push(Source { id });
    let plate_id = d.sources[0].id;
    let id = d.allocate_id(&mut id_source).unwrap();
    d.sheets.push(Arc::new(Sheet {
        id,
        name: "Bolt".into(),
        sheet: drawing_sheet::Sheet::standard(PaperSize::A4, Orientation::Portrait),
        border: Some(own_border()),
        dpi: 600.0,
        documents: Some(SheetComponent { source: plate_id, component: 0 }),
        excluded: Excluded { count: false, print: true },
    }));
    Arc::make_mut(&mut d.sheets[0]).documents = Some(SheetComponent { source: plate_id, component: 7 });
    Arc::make_mut(&mut d.properties).set(crate::metadata::Owner::Design, "author", json!("Jane Doe")).unwrap();
    d.units = Units::Fixed { unit: "in".into() };
    let (assembly, bolt_sheet) = (d.sheets[0].id, d.sheets[1].id);
    let mut add = |d: &mut Drawing, sheet, kind, place, settings, reference_shown_percent| {
        let id = d.allocate_id(&mut id_source).unwrap();
        d.add_views([Arc::new(View { id, sheet, kind, place, settings, reference_shown_percent })]);
        id
    };
    let half = ViewOverrides { scale: Some("1:2".parse().unwrap()), ..ViewOverrides::default() };
    let front = ViewKind::Base { source: plate_id, view: ViewDirection::Named(NamedView::Front) };
    let front = add(&mut d, assembly, front, ViewPlace::At([150.0, 150.0]), half, 0.0);
    let hidden = ViewOverrides { display_style: Some(VisualStyle::VisibleHidden), ..ViewOverrides::default() };
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
        d.add_dimensions([Arc::new(Dimension { id, view: front, kind, offset_mm: -10.0, text, decimals: 1, override_text, last_good: None })]);
    }
    let id = d.allocate_id(&mut id_source).unwrap();
    d.add_balloons([Arc::new(Balloon { id, view: front, edge: edge("F1:front|F1:top"), at: [60.0, 45.0] })]);
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
    // Sources from id 1: the drawing's and the sheet's ids are the seed's, far from them. A drawing
    // draws the design it is in, by an id alone: a path is a field this build doesn't know.
    let sources = |n: u64| Value::Array((1..=n).map(|id| json!({"id": ids::encode(id)})).collect());
    for n in [0, 1] {
        assert_eq!(read(&with("sources", sources(n))).unwrap().sources.len(), n as usize);
    }
    assert_eq!(refusal(&with("sources", sources(2))), "damaged drawing: a drawing draws only the design it is in");
    let with_path = refusal(&with("sources", json!([{"id": ids::encode(1), "path": "plate.solidcraft"}])));
    assert!(with_path.starts_with("damaged drawing: unknown field `path`"), "{with_path}");
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
    // A newer build's file is refused as one before anything in it is read: a field this build
    // doesn't know, or a number out of its range.
    let (current, newer) = (DRAWING.current(), DRAWING.current() + 1);
    let why = refused(&d, |v| {
        v["format"] = json!(DRAWING.format_string(newer));
        v["annotations"] = json!([]);
        v["sheets"][0]["dpi"] = json!(1e10);
    });
    assert_eq!(
        why,
        format!(
            "this drawing was saved by a newer SolidCraft (file format {newer}; this version reads formats up to {current}): update SolidCraft to open it"
        )
    );
    // A list of pending changes means nothing beside a format made of none; beside the pending
    // number, a change this build doesn't know is a newer build's.
    let why = refused(&d, |v| v["format_pending"] = json!(["a-later-change"]));
    if DRAWING.pending_names().is_empty() {
        assert_eq!(why, "read");
    } else {
        assert!(why.contains("newer SolidCraft"), "{why}");
    }
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
    let top = moved.views.iter().position(|v| v.sheet == assembly && matches!((v.kind, v.place), (ViewKind::Projected { .. }, ViewPlace::At(_))));
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
    assert!(v["format_pending"].as_array().is_some_and(|names| names.contains(&json!("drawings"))), "{}", v["format_pending"]);
    let empty: Value = serde_json::from_str(&crate::Document::new("Plate").to_json()).unwrap();
    assert_eq!(empty.get("drawings"), None);
}

/// What a design refuses in its drawings, each time the whole design, never by dropping a drawing:
/// one a newer build saved (a newer number, or a change this build doesn't know), a damaged one,
/// `drawings` that isn't a list, two with one id, and more than a design may hold, counted before
/// any is read. A drawing's numbers are its own family's to check: other tools' data in its
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
    let (current, newer) = (DRAWING.current(), DRAWING.current() + 1);
    assert_eq!(
        refused(&|v| v["drawings"][0]["format"] = json!(DRAWING.format_string(newer))),
        format!(
            "drawing 1: this drawing was saved by a newer SolidCraft (file format {newer}; this version reads formats up to {current}): update SolidCraft to open it"
        )
    );
    let later = refused(&|v| v["drawings"][0]["format_pending"] = json!(["a-later-change"]));
    assert!(if DRAWING.pending_names().is_empty() { later == "read 1" } else { later.contains("newer SolidCraft") }, "{later}");
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
