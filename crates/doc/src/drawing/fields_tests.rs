use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::json;
use solidcraft_geom::cell::Format;
use solidcraft_geom::drawing_sheet::{self, DrawingBorderProfile, Orientation, PaperSize};
use solidcraft_geom::drawing_view::{NamedView, ProjectedSide};
use solidcraft_geom::title_block::default_block;

use super::*;
use crate::drawing::view::{View, ViewPlace};
use crate::drawing::{Sheet, Source as DrawingSource};
use crate::drawing_view::{ViewDirection, ViewOverrides};
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
    let component = component.map_or(Documented::Nothing, Documented::Present);
    Values { drawing: d, sheet, component }.text(&Content(parts))
}

/// The drawing's own fields: its name, its number (its part number property), its organization
/// and any property; a property taken away shows nothing, never the value it had.
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
    assert_eq!(shown(&d, 0, None, parts), "Bracket drawing / DRW-0042 / ");
    assert_eq!(shown(&d, 0, None, vec![field(Source::Drawing, "properties.acme.finish")]), "");
    // Without a number of its own, the drawing is numbered by the component its sheet documents,
    // by name; without either, nothing.
    let metadata = Metadata::default();
    let bracket = Component { name: "Bracket", metadata: &metadata, properties: None, material: None, mass: None };
    let number = || vec![field(Source::Drawing, "number")];
    assert_eq!(shown(&d, 0, Some(bracket), number()), "DRW-0042", "its own number first");
    Arc::make_mut(&mut d.properties).set(Owner::Design, "part_number", json!("")).unwrap();
    assert_eq!(shown(&d, 0, Some(bracket), number()), "Bracket");
    assert_eq!(shown(&d, 0, None, number()), "");
}

/// Sheets are numbered among the counted ones: the second of three is "2/3". A sheet left out of
/// the count shows neither, and the sheets after it close up.
#[test]
fn sheets_are_numbered_among_the_counted_ones() {
    let mut d = drawing();
    let sheet = || vec![field(Source::Sheet, "number"), Part::Text("/".into()), field(Source::Sheet, "count")];
    assert_eq!(shown(&d, 1, None, sheet()), "2/3");
    Arc::make_mut(&mut d.sheets[1]).excluded.count = true;
    assert_eq!([0, 1, 2].map(|at| shown(&d, at, None, sheet())), ["1/2", "/", "2/2"]);
    assert_eq!(shown(&d, 1, None, vec![field(Source::Sheet, "name")]), "Sheet 2");
}

/// The revision fields read the newest row; with none, nothing.
#[test]
fn revision_fields_read_the_newest_row() {
    let mut d = drawing();
    let date = || vec![field(Source::Revision, "revision"), Part::Text(" ".into()), field(Source::Revision, "date")];
    assert_eq!(shown(&d, 0, None, date()), " ");
    let revisions = Arc::make_mut(&mut d.revisions);
    for (id, day) in [(901, "2026-10-01"), (902, "2026-10-30")] {
        revisions.add(None, id).unwrap();
        revisions.set(id, Field::Date, day).unwrap();
    }
    assert_eq!(shown(&d, 0, None, date()), "2 2026-10-30");
}

/// The fields a cell shows nothing for, by source and key: a component's empty part number and a
/// prompt nobody answered, not its name, the sheet's number or literal text. A revision field is
/// one only once the drawing has a revision row, and then only while that row lacks it. A field
/// that reads lost or unknown has no value either.
#[test]
fn empty_fields_are_named_by_source_and_key() {
    let mut d = drawing();
    let metadata = Metadata::default();
    let bracket = Component { name: "Bracket", metadata: &metadata, properties: None, material: None, mass: None };
    let content = Content(vec![
        field(Source::Component, "name"),
        Part::Text(" ".into()),
        field(Source::Component, "part_number"),
        field(Source::Sheet, "number"),
        field(Source::Prompt, "checked_by"),
        field(Source::Revision, "date"),
    ]);
    let empty = |d: &Drawing| Values { drawing: d, sheet: 0, component: Documented::Present(bracket) }.empty_fields(&content);
    assert_eq!(empty(&d), ["component.part_number", "prompt.checked_by"]);
    Arc::make_mut(&mut d.revisions).add(None, 901).unwrap();
    assert_eq!(empty(&d), ["component.part_number", "prompt.checked_by", "revision.date"]);
    Arc::make_mut(&mut d.revisions).set(901, Field::Date, "2026-11-07").unwrap();
    assert_eq!(empty(&d), ["component.part_number", "prompt.checked_by"]);
    let lost = Values { drawing: &d, sheet: 0, component: Documented::Gone }.empty_fields(&content);
    assert_eq!(lost, ["component.name", "component.part_number", "prompt.checked_by"], "a component since deleted");
    let unknown = Content(vec![field(Source::Component, "weight"), field(Source::Sheet, "page")]);
    let present = Values { drawing: &d, sheet: 0, component: Documented::Present(bracket) };
    assert_eq!(present.empty_fields(&unknown), ["component.weight", "sheet.page"], "a drawing saved with fields its sources lack");
}

/// A component's fields: its name and part properties, its free-text properties, and the material
/// and mass the caller worked out; without a component, or a value, or with an empty one, nothing.
#[test]
fn the_components_fields() {
    let d = drawing();
    let mut metadata = Metadata::default();
    metadata.set(Owner::Component, "part_number", json!("BRK-7")).unwrap();
    let properties = BTreeMap::from([("finish".to_string(), "Zinc plated".to_string())]);
    let bracket = Component { name: "Bracket", metadata: &metadata, properties: Some(&properties), material: Some("Steel"), mass: Some("0.39 kg") };
    let keys = ["name", "part_number", "material", "mass", "properties.finish", "description"];
    let read = |component| keys.map(|key| shown(&d, 0, component, vec![field(Source::Component, key)]));
    assert_eq!(read(Some(bracket)), ["Bracket", "BRK-7", "Steel", "0.39 kg", "Zinc plated", ""]);
    let unweighed = Component { material: None, mass: None, properties: None, ..bracket };
    assert_eq!(read(Some(unweighed)), ["Bracket", "BRK-7", "", "", "", ""]);
    assert_eq!(read(None), [""; 6]);
    let blank = BTreeMap::from([("finish".to_string(), String::new())]);
    let finish = vec![field(Source::Component, "properties.finish")];
    assert_eq!(shown(&d, 0, Some(Component { properties: Some(&blank), ..bracket }), finish), "");
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

/// Each field reads shown, empty or lost: lost only for a component's field when the sheet
/// documents a component the design no longer has. The paper prints an empty and a lost field
/// alike, as nothing, as it always has.
#[test]
fn each_field_is_shown_empty_or_lost() {
    let d = drawing();
    let metadata = Metadata::default();
    let bracket = Component { name: "Bracket", metadata: &metadata, properties: None, material: None, mass: None };
    let blank = BTreeMap::from([("finish".to_string(), String::new())]);
    let state = |component, source, key| Values { drawing: &d, sheet: 0, component }.state(source, key);
    let present = Documented::Present(bracket);
    assert_eq!(state(present, Source::Component, "name"), FieldState::Shown("Bracket".into()));
    assert_eq!(state(present, Source::Component, "material"), FieldState::Empty);
    let finish = Documented::Present(Component { properties: Some(&blank), ..bracket });
    assert_eq!(state(finish, Source::Component, "properties.finish"), FieldState::Empty, "an empty value is empty");
    assert_eq!(state(Documented::Nothing, Source::Component, "name"), FieldState::Empty);
    assert_eq!(state(Documented::Gone, Source::Component, "name"), FieldState::Lost);
    assert_eq!(state(Documented::Gone, Source::Component, "mass"), FieldState::Lost);
    // Only the component's own fields are lost: the drawing's and the sheet's still read.
    assert_eq!(state(Documented::Gone, Source::Drawing, "name"), FieldState::Shown("Bracket drawing".into()));
    assert_eq!(state(Documented::Gone, Source::Drawing, "number"), FieldState::Empty);
    assert_eq!(state(Documented::Gone, Source::Sheet, "name"), FieldState::Shown("Sheet 1".into()));
    let parts = Content(vec![Part::Text("Material: ".into()), field(Source::Component, "material"), Part::Text(".".into())]);
    for component in [Documented::Gone, Documented::Nothing, present] {
        assert_eq!(Values { drawing: &d, sheet: 0, component }.text(&parts), "Material: .");
    }
}

/// A sheet's scale is the one its base views share, written as a view's scale is: a projected view's
/// own scale and another sheet's are not it. Base views at different scales give none, whichever
/// was placed first and whichever id is lower. A sheet without a base view shows the one scale its
/// views draw at (projected views of a base on another sheet), and none when they differ or it has
/// no views.
#[test]
fn a_sheets_scale_is_the_one_its_base_views_share() {
    let mut d = drawing();
    let scale = |d: &Drawing| [0, 1, 2].map(|at| shown(d, at, None, vec![field(Source::Sheet, "scale")]));
    assert_eq!(scale(&d), ["", "", ""]);
    let mut ids = IdSource::seeded(13059);
    let source = d.allocate_id(&mut ids).unwrap();
    d.sources.push(DrawingSource { id: source });
    let [side, front, other, top] = [(); 4].map(|()| d.allocate_id(&mut ids).unwrap());
    let (first, second) = (d.sheets[0].id, d.sheets[1].id);
    let view = |id, sheet, kind, scale: &str| {
        let settings = ViewOverrides { scale: Some(scale.parse().unwrap()), ..ViewOverrides::default() };
        Arc::new(View { id, sheet, kind, place: ViewPlace::At([150.0, 150.0]), settings, reference_shown_percent: 0.0, centre_marks: None })
    };
    let base = |named| ViewKind::Base { source, view: ViewDirection::Named(named) };
    d.add_views([
        view(side, first, ViewKind::Projected { parent: front, side: ProjectedSide::Right }, "5:1"),
        view(front, first, base(NamedView::Front), "1:2"),
        view(other, second, base(NamedView::Front), "1:5"),
        view(top, first, base(NamedView::Top), "2:1"),
    ]);
    d.validate().unwrap();
    // Front was placed first and Top last; the seed's ids put Top first. Neither decides.
    assert!(top < front, "the seed's ids disagree with the order the views were placed in");
    assert_eq!(scale(&d), ["", "1:5", ""], "1:2 and 2:1 on one sheet: no scale of the sheet's");
    let at = d.views.iter().position(|v| v.id == top).unwrap();
    let settings = ViewOverrides { scale: Some("1:2".parse().unwrap()), ..d.views[at].settings.clone() };
    d.views[at] = Arc::new(View { settings, ..(*d.views[at]).clone() });
    assert_eq!(scale(&d), ["1:2", "1:5", ""], "the scale both base views share; the projected view's 5:1 isn't");
    // The third sheet holds only projected views of the 1:2 front on the first, placed at points.
    let third = d.sheets[2].id;
    let [left, below] = [(); 2].map(|()| d.allocate_id(&mut ids).unwrap());
    let projected = |id, side| {
        let kind = ViewKind::Projected { parent: front, side };
        Arc::new(View {
            id,
            sheet: third,
            kind,
            place: ViewPlace::At([150.0, 150.0]),
            settings: ViewOverrides::default(),
            reference_shown_percent: 0.0,
            centre_marks: None,
        })
    };
    d.add_views([projected(left, ProjectedSide::Left), projected(below, ProjectedSide::Below)]);
    d.validate().unwrap();
    assert_eq!(scale(&d), ["1:2", "1:5", "1:2"], "a sheet of projected views only: the scale they draw at");
    let at = d.views.iter().position(|v| v.id == below).unwrap();
    let settings = ViewOverrides { scale: Some("2:1".parse().unwrap()), ..d.views[at].settings.clone() };
    d.views[at] = Arc::new(View { settings, ..(*d.views[at]).clone() });
    assert_eq!(scale(&d), ["1:2", "1:5", ""], "its views draw at 1:2 and 2:1: no scale of the sheet's");
}

/// A field its source doesn't have reads unknown, prints nothing, and says why: a note saved with
/// one still opens, and a block or a note given one is refused with the same words.
#[test]
fn a_field_its_source_lacks_reads_unknown() {
    let d = drawing();
    let metadata = Metadata::default();
    let bracket = Component { name: "Bracket", metadata: &metadata, properties: None, material: None, mass: None };
    let values = Values { drawing: &d, sheet: 0, component: Documented::Present(bracket) };
    assert_eq!(values.state(Source::Component, "weight"), FieldState::Unknown);
    assert_eq!(values.state(Source::Sheet, "scales"), FieldState::Unknown);
    assert_eq!(values.state(Source::Component, "name"), FieldState::Shown("Bracket".into()));
    let gone = Values { component: Documented::Gone, ..values };
    assert_eq!(gone.state(Source::Component, "weight"), FieldState::Unknown, "unknown before lost");
    let parts = Content(vec![field(Source::Component, "name"), field(Source::Component, "weight"), field(Source::Sheet, "page")]);
    assert_eq!(values.text(&parts), "Bracket");
    assert_eq!(no_field(Source::Component, "weight").as_deref(), Some("the component has no field `weight`"));
    assert_eq!(no_field(Source::Component, "mass"), None);
    assert_eq!(unknown_field(&parts).as_deref(), Some("the component has no field `weight`"), "the first one");
    assert_eq!(unknown_field(&Content(vec![field(Source::Sheet, "scale")])), None);
}
