use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::json;
use solidcraft_geom::cell::Format;
use solidcraft_geom::drawing_sheet::{self, DrawingBorderProfile, Orientation, PaperSize};
use solidcraft_geom::title_block::default_block;

use super::*;
use crate::drawing::Sheet;
use crate::ids::IdSource;
use crate::metadata::Owner;

/// A drawing of three A3 sheets, each with the built-in block.
fn drawing() -> Drawing {
    let mut ids = IdSource::seeded(3875);
    let a3 = drawing_sheet::Sheet::standard(PaperSize::A3, Orientation::Landscape);
    let mut d = Drawing::new("Bracket drawing", "en", "Sheet 1", a3, Some(DrawingBorderProfile::iso()), &BTreeSet::new(), &mut ids).unwrap();
    Arc::make_mut(&mut d.sheets[0]).title_block = Some(default_block());
    for n in 2..=3 {
        let id = d.allocate_id(&mut ids).unwrap();
        d.sheets.push(Arc::new(Sheet { id, name: format!("Sheet {n}"), ..(*d.sheets[0]).clone() }));
    }
    d.validate().unwrap();
    d
}

fn field(source: Source, key: &str) -> Part {
    Part::Field { source, key: key.into(), format: Format::default() }
}

fn shown(d: &Drawing, sheet: usize, component: Option<Component>, parts: Vec<Part>) -> String {
    Values { drawing: d, sheet, component }.text(&Content(parts))
}

/// The drawing's own fields: its name, its number (its part number property), its organization
/// and any property; a property taken away shows "-", never the value it had.
#[test]
fn the_drawings_fields() {
    let mut d = drawing();
    let properties = Arc::make_mut(&mut d.properties);
    properties.set(Owner::Design, "part_number", json!("DRW-0042")).unwrap();
    properties.set(Owner::Design, "organization", json!("Tangency Labs")).unwrap();
    let parts = vec![
        field(Source::Drawing, "name"),
        Part::Text(" / ".into()),
        field(Source::Drawing, "number"),
        Part::Text(" / ".into()),
        field(Source::Drawing, "organization"),
    ];
    assert_eq!(shown(&d, 0, None, parts.clone()), "Bracket drawing / DRW-0042 / Tangency Labs");
    Arc::make_mut(&mut d.properties).remove("organization");
    assert_eq!(shown(&d, 0, None, parts), "Bracket drawing / DRW-0042 / -");
    assert_eq!(shown(&d, 0, None, vec![field(Source::Drawing, "properties.acme.finish")]), "-");
}

/// Sheets are numbered among the counted ones: the second of three is "2/3". A sheet left out of
/// the count shows neither, and the sheets after it close up.
#[test]
fn sheets_are_numbered_among_the_counted_ones() {
    let mut d = drawing();
    let sheet = || vec![field(Source::Sheet, "number"), Part::Text("/".into()), field(Source::Sheet, "count")];
    assert_eq!(shown(&d, 1, None, sheet()), "2/3");
    Arc::make_mut(&mut d.sheets[1]).excluded.count = true;
    assert_eq!([0, 1, 2].map(|at| shown(&d, at, None, sheet())), ["1/2", "-/-", "2/2"]);
    assert_eq!(shown(&d, 1, None, vec![field(Source::Sheet, "name")]), "Sheet 2");
}

/// The revision fields read the newest row; with none, "-".
#[test]
fn revision_fields_read_the_newest_row() {
    let mut d = drawing();
    let date = || vec![field(Source::Revision, "revision"), Part::Text(" ".into()), field(Source::Revision, "date")];
    assert_eq!(shown(&d, 0, None, date()), "- -");
    let revisions = Arc::make_mut(&mut d.revisions);
    for (id, day) in [(901, "2026-10-01"), (902, "2026-10-30")] {
        revisions.add(None, id).unwrap();
        revisions.set(id, Field::Date, day).unwrap();
    }
    assert_eq!(shown(&d, 0, None, date()), "2 2026-10-30");
}

/// A component's fields: its name and part properties, its free-text properties, and the material
/// and mass the caller worked out; without a component, or a value, or with an empty one, "-".
#[test]
fn the_components_fields() {
    let d = drawing();
    let mut metadata = Metadata::default();
    metadata.set(Owner::Component, "part_number", json!("BRK-7")).unwrap();
    let properties = BTreeMap::from([("finish".to_string(), "Zinc plated".to_string())]);
    let bracket = Component { name: "Bracket", metadata: &metadata, properties: Some(&properties), material: Some("Steel"), mass: Some("0.39 kg") };
    let keys = ["name", "part_number", "material", "mass", "properties.finish", "description"];
    let read = |component| keys.map(|key| shown(&d, 0, component, vec![field(Source::Component, key)]));
    assert_eq!(read(Some(bracket)), ["Bracket", "BRK-7", "Steel", "0.39 kg", "Zinc plated", "-"]);
    let unweighed = Component { material: None, mass: None, properties: None, ..bracket };
    assert_eq!(read(Some(unweighed)), ["Bracket", "BRK-7", "-", "-", "-", "-"]);
    assert_eq!(read(None), ["-"; 6]);
    let blank = BTreeMap::from([("finish".to_string(), String::new())]);
    let finish = vec![field(Source::Component, "properties.finish")];
    assert_eq!(shown(&d, 0, Some(Component { properties: Some(&blank), ..bracket }), finish), "-");
}

/// A field its source doesn't have is refused when a block is given and when a drawing is read,
/// naming the cell; so is an override of a cell the block doesn't have or that shows no text, or
/// one with a control character. A sheet without overrides saves none.
#[test]
fn unknown_fields_and_overrides_are_refused() {
    let mut block = default_block();
    let CellKind::Text(text) = &mut block.cells[8].kind else { panic!("the mass cell has text") };
    text.content = Content(vec![field(Source::Component, "weight")]);
    assert_eq!(check_block(&block).unwrap_err().to_string(), "cell `mass`: the component has no field `weight`");
    let mut d = drawing();
    let saved = d.to_json().unwrap();
    assert!(!saved.contains("title_block_overrides"));
    let mut v: serde_json::Value = serde_json::from_str(&saved).unwrap();
    v["sheets"][0]["title_block"] = serde_json::to_value(&block).unwrap();
    let refusal = Drawing::from_json(&v.to_string()).unwrap_err().to_string();
    assert!(refusal.contains("cell `mass`: the component has no field `weight`"), "{refusal}");
    // Overrides: a cell's name, a cell's text.
    Arc::make_mut(&mut d.sheets[0]).title_block_overrides.insert("material".into(), "See note 3".into());
    let saved = d.to_json().unwrap();
    assert_eq!(Drawing::from_json(&saved).unwrap(), d);
    let refusals =
        [("weight", "x", "no text cell `weight`"), ("projection", "x", "no text cell `projection`"), ("material", "a\u{7}", "control character")];
    for (cell, text, why) in refusals {
        let mut wrong = d.clone();
        Arc::make_mut(&mut wrong.sheets[0]).title_block_overrides.insert(cell.into(), text.into());
        let refusal = wrong.validate().unwrap_err().to_string();
        assert!(refusal.contains(why), "{refusal}");
    }
}
