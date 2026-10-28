use serde_json::json;
use solidcraft_geom::drawing_sheet::{Margins, Orientation, PaperSize, SheetSize};

use super::*;

/// The seed every test drawing's ids come from.
const SEED: u64 = 3890;

/// A profile of its own: ISO 5457's filing margin and trimming marks, which the built-in leaves off.
fn own_border() -> DrawingBorderProfile {
    DrawingBorderProfile { filing_margin: true, trimming_mark_mm: Some([10.0, 5.0]), ..DrawingBorderProfile::iso() }
}

/// The design it is in, on two sheets: the first (A3, the ISO border) documents its component 7;
/// the second (A4 portrait, a border of its own, 600 dpi, left out of printing) the design itself.
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
    d.properties.set("author", json!("Jane Doe")).unwrap();
    d.units = Units::Fixed { unit: "in".into() };
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
        assert_eq!(units(json!({"fixed": {"unit": unit}})).is_ok(), kind == expr::Kind::Length, "{unit}");
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
    assert_eq!(custom(json!([5000.0, 0.5])), "read");
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
        |v| v["views"] = json!([]),
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
        v["views"] = json!([]);
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
    assert_eq!(refused(&d, |v| v["views"] = deep), "damaged drawing: nested too deeply");
    let deeper = format!("{{\"format\": \"{drawing_format}\", \"views\": {}1{}}}", "[".repeat(200), "]".repeat(200));
    assert!(Drawing::from_json(&deeper).unwrap_err().to_string().starts_with("not a SolidCraft drawing: "));
    let text = d.to_json().unwrap();
    let at_cap = text.clone() + &" ".repeat(MAX_DRAWING_BYTES - text.len());
    assert_eq!(Drawing::from_json(&at_cap).unwrap(), d);
    assert_eq!(Drawing::from_json(&(at_cap + " ")).unwrap_err().to_string(), "drawing too large (at most 16 MiB)");
}

/// A run never gives an id twice: not one in use, and not a removed object's. Only another run from
/// the same seed, which tests and replays start from the same file, can give a removed id again.
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

// W38a: original paper-mm payload, independent of any Drawing UI or model solver.
fn paper_sketch_file() -> Value {
    let mut d = file(&plate());
    let mut sketch = solidcraft_sketch::Sketch::new();
    let pts = [(15.0, 10.0), (55.0, 10.0), (55.0, 30.0), (15.0, 30.0)];
    for (i, (x, y)) in pts.into_iter().enumerate() {
        sketch.add_point(solidcraft_geom::Vec2::new(x, y), Some(&format!("p{i}"))).unwrap();
    }
    for i in 0..4 {
        sketch.add_line_pts(i + 1, (i + 1) % 4 + 1, Some(&format!("l{i}"))).unwrap();
    }
    sketch.add_constraint(solidcraft_sketch::ConstraintKind::Length { l: 0, value: 40.0 }, Some("Width".into())).unwrap();
    sketch.points[1].exprs = Some(Box::new(solidcraft_sketch::PointExprs { x: Some("Inset + 5 mm".into()), y: None }));
    d["sketches"] = json!([{"id": ids::encode(41), "sheet": d["sheets"][0]["id"], "name":"Detail outline", "sketch": sketch}]);
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
    second["sheet"] = input["sheets"][1]["id"].clone();
    input["sketches"].as_array_mut().unwrap().push(second);
    let d = read(&input).unwrap();
    assert_eq!(d.sketches.iter().map(|s| s.id).collect::<Vec<_>>(), [40, 41]);
    assert_eq!(file(&d)["sheets"], input["sheets"]);
    let mut reverse = d.clone();
    reverse.sketches.reverse();
    assert_eq!(reverse.to_json().unwrap(), d.to_json().unwrap());
    assert_eq!(reverse.sketches[0].id, 41, "saving must not mutate the live record");
    assert_eq!(d.sheet_sketch(41).unwrap().sheet, d.sheets[0].id);
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
    v["sketches"][0]["sheet"] = good["sources"][0]["id"].clone();
    assert!(refusal(&v).contains("owner does not exist"));
    v["sketches"][0]["sheet"] = json!(ids::encode(999));
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

#[test]
fn drawings_without_sketches_keep_their_old_shape_and_gain_only_the_pending_format() {
    let old = plate();
    let v = file(&old);
    assert!(v.get("sketches").is_none());
    assert!(read(&v).unwrap().sketches.is_empty());
    assert!(DRAWING.pending_names().iter().any(|n| n == "sheet-sketches"));
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
    for field in ["links", "patterns", "offsets", "freeform_offsets", "wires"] {
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
    assert_eq!(solved.sketch.points[1].pos, solidcraft_geom::Vec2::new(15.0, 10.0));
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
    d.sources.clear();
    for sheet in &mut d.sheets {
        Arc::make_mut(sheet).documents = None;
    }
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
    assert_eq!(staged.sheet_sketch(41).unwrap().sketch.points[0].pos, solidcraft_geom::Vec2::ZERO);
    assert_eq!(staged.sheet_sketch(41).unwrap().sheet, staged.sheets[0].id);
}

#[test]
fn staged_sheet_sketch_edit_checks_the_cumulative_entity_cap_before_solving() {
    use solidcraft_sketch::{MAX_CONSTRAINTS, MAX_CURVES, MAX_POINTS, Sketch};
    let mut d = bare();
    let mut payload = Sketch::new();
    for i in 1..80 {
        payload.add_point(solidcraft_geom::Vec2::new(i as f64, 10.0), Some(&format!("p{i}"))).unwrap();
    }
    for i in 0..1000 {
        d.sketches.push(Arc::new(SheetSketch { id: 1000 + i, sheet: d.sheets[0].id, name: format!("Sketch {i}"), sketch: payload.clone() }));
    }
    assert_eq!(d.sketches.len(), MAX_SHEET_SKETCHES);
    assert_eq!(d.sketches.iter().map(|s| s.sketch.points.len()).sum::<usize>(), MAX_POINTS + MAX_CURVES + MAX_CONSTRAINTS);
    d.validate().unwrap();
    let original = d.clone();
    let record = d.sheet_sketch(1000).unwrap().clone();
    let e = d
        .edit_sheet_sketch(&crate::Document::default(), 1000, |sketch| {
            sketch.add_point(solidcraft_geom::Vec2::new(3.0, 4.0), Some("over_cap"))?;
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
    use solidcraft_geom::Vec2;
    use solidcraft_sketch::{ConstraintKind as K, Sketch};
    let mut sk = Sketch::new();
    let line = sk.add_line(Vec2::new(20.0, 15.0), Vec2::new(60.0, 15.0), None, None, None).unwrap();
    let a = sk.add_arc(Vec2::new(20.0, 20.0), Vec2::new(20.0, 25.0), Vec2::new(20.0, 15.0), [None; 3], None).unwrap();
    let b = sk.add_arc(Vec2::new(60.0, 20.0), Vec2::new(60.0, 15.0), Vec2::new(60.0, 25.0), [None; 3], None).unwrap();
    for _ in 0..703 {
        sk.add_constraint(K::Horizontal { l: line }, None).unwrap();
    }
    sk.add_constraint(K::Radius { c: a, value: 5.0 }, None).unwrap();
    sk.add_constraint(K::Radius { c: b, value: 5.0 }, None).unwrap();
    sk.add_constraint(K::SlotOverallLength { a, b, value: 50.0 }, None).unwrap();
    let mut d = bare();
    d.sketches.push(Arc::new(SheetSketch { id: 41, sheet: d.sheets[0].id, name: "Slot".into(), sketch: sk }));
    // 705 ordinary charges plus 4*706^2 for the one slot: 1,994,449.
    d.validate().unwrap();
    let before = d.clone();
    let e = d
        .edit_sheet_sketch(&crate::Document::default(), 41, |sketch| {
            sketch.add_constraint(K::Horizontal { l: line }, None)?;
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
