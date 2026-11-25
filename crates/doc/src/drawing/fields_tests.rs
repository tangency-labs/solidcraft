use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::json;
use solidcraft_geom::cell::Format;
use solidcraft_geom::drawing_sheet::{self, DrawingBorderProfile, Orientation, PaperSize};
use solidcraft_geom::drawing_view::{NamedView, ProjectedSide};
use solidcraft_geom::title_block::default_block;

use super::*;
use crate::drawing::view::{View, ViewKind, ViewPlace};
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
    let bracket = Component { name: "Bracket", metadata: &metadata, properties: None, material: None, mass: None, sheet_metal: None };
    let number = || vec![field(Source::Drawing, "number")];
    assert_eq!(shown(&d, 0, Some(bracket), number()), "DRW-0042", "its own number first");
    Arc::make_mut(&mut d.properties).set(Owner::Design, "part_number", json!("")).unwrap();
    assert_eq!(shown(&d, 0, Some(bracket), number()), "Bracket");
    assert_eq!(shown(&d, 0, None, number()), "");
}

/// The drawing's language is a field for the ISO 7200 language cell (#20121): its code in capitals,
/// whatever case or region it was saved with.
#[test]
fn the_drawings_language_reads_as_its_code_in_capitals() {
    let mut d = drawing();
    let language = || vec![field(Source::Drawing, "language")];
    assert!(has_key(Source::Drawing, "language") && !has_key(Source::Sheet, "language"));
    assert_eq!(shown(&d, 0, None, language()), "EN");
    d.language = "pt-br".into();
    assert_eq!(shown(&d, 0, None, language()), "PT-BR");
    assert!(Values { drawing: &d, sheet: 0, component: Documented::Nothing }.empty_fields(&Content(language())).is_empty());
}

/// The built-in block's new cells read what the drawing knows: the sheet's scale, the drawing's
/// language, the newest revision row's revision; the shop's own three read nothing.
#[test]
fn the_built_in_blocks_iso_7200_cells_follow_the_drawing() {
    let d = drawing();
    let block = d.sheets[0].title_block.clone().unwrap();
    let read = |name: &str| {
        let cell = block.cells.iter().find(|c| c.name == name).unwrap();
        let CellKind::Text(text) = &cell.kind else { panic!("{name} is text") };
        shown(&d, 0, None, text.content.0.clone())
    };
    assert_eq!(read("language"), "EN");
    for own in ["document_type", "document_status", "department"] {
        assert_eq!(read(own), "", "{own}");
    }
    // No base view and no revision row yet: nothing to show, and no row to blame.
    assert_eq!((read("scale").as_str(), read("revision").as_str()), ("", ""));
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
    let bracket = Component { name: "Bracket", metadata: &metadata, properties: None, material: None, mass: None, sheet_metal: None };
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
    let bracket = Component {
        name: "Bracket",
        metadata: &metadata,
        properties: Some(&properties),
        material: Some("Steel"),
        mass: Some("0.39 kg"),
        sheet_metal: None,
    };
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

/// A 2 mm sheet made with rule `AL2`, K-factor 0.44, bent at 1 mm, whose flat is 120.5 by
/// 300.25 mm along its axes.
fn al2() -> SheetBody<'static> {
    SheetBody { rule: "AL2", thickness: 2.0, k_factor: 0.44, bend_radius: Some(1.0), flat: Some([120.5, 300.25]) }
}

/// The six sheet metal fields of a component with one sheet body: lengths in the drawing's units
/// with two decimals (inches keep a fourth where it means something, as a dimension does: 2 mm is
/// 0.07874 in), the flat's longer side its length whichever axis it runs along, and the K-factor
/// without trailing zeros.
#[test]
fn a_sheet_metal_parts_fields() {
    let d = drawing();
    let metadata = Metadata::default();
    let read = |sheet: &SheetMetal| {
        let bracket = Component { name: "Bracket", metadata: &metadata, properties: None, material: None, mass: None, sheet_metal: Some(sheet) };
        SHEET_METAL.map(|key| shown(&d, 0, Some(bracket), vec![field(Source::Component, key)]))
    };
    // In SHEET_METAL's order: bend_radius, flat_length, flat_width, k_factor, sheet_rule, thickness.
    assert_eq!(read(&SheetMetal::of(&[al2()], "mm")), ["1.00 mm", "300.25 mm", "120.50 mm", "0.44", "AL2", "2.00 mm"]);
    assert_eq!(read(&SheetMetal::of(&[al2()], "in")), ["0.0394 in", "11.8209 in", "4.7441 in", "0.44", "AL2", "0.0787 in"]);
    let turned = SheetBody { flat: Some([300.25, 120.5]), ..al2() };
    assert_eq!(read(&SheetMetal::of(&[turned], "mm"))[1..3], ["300.25 mm", "120.50 mm"], "length is the longer side");
    let k = |k_factor| SheetMetal::of(&[SheetBody { k_factor, ..al2() }], "mm").k_factor;
    assert_eq!([k(0.5), k(1.0 / 3.0), k(0.0), k(f64::NAN)], [Some("0.5".into()), Some("0.333".into()), Some("0".into()), None]);
    // A note reads them as text around them.
    let sheet = SheetMetal::of(&[al2()], "mm");
    let bracket = Component { name: "Bracket", metadata: &metadata, properties: None, material: None, mass: None, sheet_metal: Some(&sheet) };
    let note =
        vec![Part::Text("t = ".into()), field(Source::Component, "thickness"), Part::Text(", ".into()), field(Source::Component, "sheet_rule")];
    assert_eq!(shown(&d, 0, Some(bracket), note), "t = 2.00 mm, AL2");
}

/// A component's sheet metal field shows only what every one of its sheet bodies gives: two bodies
/// cut from one rule agree on everything but what differs between them; a body whose bends differ
/// or whose flat doesn't describe it has no bend radius or flat size, so the component has none;
/// a component with no sheet body has none at all, and never a value from another part.
#[test]
fn sheet_metal_fields_show_only_what_every_sheet_body_agrees_on() {
    let both = SheetMetal::of(&[al2(), SheetBody { flat: Some([300.25, 120.5]), ..al2() }], "mm");
    let one = SheetMetal::of(&[al2()], "mm");
    assert_eq!(both, one, "the same flat turned round is the same size");
    let thicker = SheetBody { thickness: 3.0, bend_radius: Some(1.5), flat: Some([120.0, 300.25]), ..al2() };
    let differ = SheetMetal::of(&[al2(), thicker], "mm");
    let expected = SheetMetal {
        thickness: None,
        sheet_rule: Some("AL2".into()),
        k_factor: Some("0.44".into()),
        bend_radius: None,
        flat_length: Some("300.25 mm".into()),
        flat_width: None,
    };
    assert_eq!(differ, expected);
    let altered = SheetBody { bend_radius: None, flat: None, ..al2() };
    let altered = SheetMetal::of(&[altered], "mm");
    assert_eq!([&altered.bend_radius, &altered.flat_length, &altered.flat_width], [&None, &None, &None]);
    assert_eq!(altered.thickness.as_deref(), Some("2.00 mm"), "the flat doesn't change the sheet's thickness");
    let nan = SheetMetal::of(&[SheetBody { thickness: f64::NAN, flat: Some([f64::INFINITY, 1.0]), ..al2() }], "mm");
    assert_eq!([&nan.thickness, &nan.flat_length, &nan.flat_width], [&None, &None, &None], "never a number that isn't one");
    assert_eq!(SheetMetal::of(&[], "mm"), SheetMetal::default(), "no sheet body");
    // Read through a sheet: empty for a component that isn't sheet metal, lost for one deleted.
    let d = drawing();
    let metadata = Metadata::default();
    let plain = Component { name: "Bracket", metadata: &metadata, properties: None, material: None, mass: None, sheet_metal: None };
    let content = Content(vec![field(Source::Component, "name"), field(Source::Component, "thickness"), field(Source::Component, "flat_length")]);
    let values = |component| Values { drawing: &d, sheet: 0, component };
    assert_eq!(values(Documented::Present(plain)).state(Source::Component, "thickness"), FieldState::Empty);
    assert_eq!(values(Documented::Present(plain)).empty_fields(&content), ["component.thickness", "component.flat_length"]);
    let sheet = Component { sheet_metal: Some(&differ), ..plain };
    assert_eq!(values(Documented::Present(sheet)).empty_fields(&content), ["component.thickness"]);
    assert_eq!(values(Documented::Gone).state(Source::Component, "k_factor"), FieldState::Lost);
    assert_eq!(no_field(Source::Component, "flat_width"), None);
    assert_eq!(no_field(Source::Drawing, "thickness").as_deref(), Some("the drawing has no field `thickness`"), "a component's field only");
}

/// The `finish` field reads the part's finish property (a design's own too: its block's component 0);
/// a part without one shows its free property of that name, as the built-in block's Finish cell did, and
/// the `properties.finish` field still reads the free property alone.
#[test]
fn the_finish_field_reads_the_finish_property_then_the_free_one() {
    fn part<'a>(metadata: &'a Metadata, properties: Option<&'a BTreeMap<String, String>>) -> Option<Component<'a>> {
        Some(Component { name: "Bracket", metadata, properties, material: None, mass: None, sheet_metal: None })
    }
    let d = drawing();
    let free = BTreeMap::from([("finish".to_string(), "Zinc plated".to_string())]);
    // A fn, not a closure: each call borrows the component for itself.
    fn read(d: &Drawing, component: Option<Component>, key: &str) -> String {
        shown(d, 0, component, vec![field(Source::Component, key)])
    }
    assert!(has_key(Source::Component, "finish") && has_key(Source::Component, "properties.finish"));
    let mut metadata = Metadata::default();
    assert_eq!(read(&d, part(&metadata, None), "finish"), "", "nothing set");
    assert_eq!(read(&d, part(&metadata, Some(&free)), "finish"), "Zinc plated", "the free property shows as before");
    metadata.set(Owner::Component, "finish", json!("Powder coat, RAL 9005, matt")).unwrap();
    assert_eq!(read(&d, part(&metadata, Some(&free)), "finish"), "Powder coat, RAL 9005, matt", "the part property wins");
    assert_eq!(read(&d, part(&metadata, None), "finish"), "Powder coat, RAL 9005, matt", "the design has no free properties");
    assert_eq!(read(&d, part(&metadata, Some(&free)), "properties.finish"), "Zinc plated", "that field is unchanged");
    assert_eq!(read(&d, None, "finish"), "");
    let blank = Metadata(serde_json::from_value(json!({"finish": ""})).unwrap());
    assert_eq!(read(&d, part(&blank, Some(&free)), "finish"), "Zinc plated", "an empty property is none");
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
    let bracket = Component { name: "Bracket", metadata: &metadata, properties: None, material: None, mass: None, sheet_metal: None };
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
    d.sources.push(DrawingSource { id: source, path: None });
    let [side, front, other, top] = [(); 4].map(|()| d.allocate_id(&mut ids).unwrap());
    let (first, second) = (d.sheets[0].id, d.sheets[1].id);
    let view = |id, sheet, kind, scale: &str| {
        let settings = ViewOverrides { scale: Some(scale.parse().unwrap()), ..ViewOverrides::default() };
        Arc::new(View {
            id,
            sheet,
            kind,
            place: ViewPlace::At([150.0, 150.0]),
            settings,
            reference_shown_percent: 0.0,
            centre_marks: None,
            label: None,
        })
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
            label: None,
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
    let bracket = Component { name: "Bracket", metadata: &metadata, properties: None, material: None, mass: None, sheet_metal: None };
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
