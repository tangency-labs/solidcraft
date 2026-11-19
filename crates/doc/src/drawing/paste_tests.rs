use std::collections::BTreeSet;
use std::sync::Arc;

use solidcore_record::Vec2;
use solidcraft_geom::cell::Projection;
use solidcraft_geom::drawing_sheet::{self, Orientation, PaperSize};
use solidcraft_geom::drawing_view::ProjectedSide;

use super::*;
use crate::drawing::FillPaint;
use crate::drawing::cutting_line::{ArrowSide, CuttingLine, SectionKind};
use crate::drawing::dimension::ModelDimension;
use crate::drawing::table::{MassFormat, PartsList};
use crate::drawing::tests::plate;
use crate::drawing::view::{Diagonal, QuarterTurn, ViewLabel, ViewPlace};
use crate::drawing::{SheetComponent, Source};
use crate::drawing_view::{Exploded, Pose, StudyStep, ViewConfiguration, ViewOverrides};

/// The plate drawing's front view and the right view projected from it.
fn front_and_right(d: &Drawing) -> (u64, u64) {
    let front = d.views.iter().find(|v| matches!(v.kind, ViewKind::Base { .. }) && v.sheet == d.sheets[0].id).unwrap().id;
    let right = d.views.iter().find(|v| v.kind == ViewKind::Projected { parent: front, side: ProjectedSide::Right }).unwrap().id;
    (front, right)
}

/// Onto `d`'s first sheet, where the copy was, for its own source.
fn here(d: &Drawing) -> Placement {
    Placement { onto: d.sheets[0].id, by: Vec2::ZERO, source: d.design_source().unwrap() }
}

/// A drawing of another design: one A3 sheet, no views.
fn other() -> Drawing {
    let mut id_source = IdSource::seeded(13066);
    let a3 = drawing_sheet::Sheet::standard(PaperSize::A3, Orientation::Landscape);
    let mut d = Drawing::new("Other", "en", "Sheet 1", a3, None, &BTreeSet::new(), &mut id_source).unwrap();
    let source = d.allocate_id(&mut id_source).unwrap();
    d.sources.push(Source { id: source, path: None });
    d
}

/// A parts list of `d`'s design on its first sheet, its id.
fn add_list(d: &mut Drawing, id_source: &mut IdSource) -> u64 {
    let id = d.allocate_id(id_source).unwrap();
    let of = SheetComponent { source: d.sources[0].id, component: 0 };
    let list =
        PartsList { of, view: Default::default(), group: Default::default(), fold: false, purchased_contents: false, mass: MassFormat::default() };
    let kind = Kind::PartsList(list);
    d.tables.add([Arc::new(SheetTable { id, sheet: d.sheets[0].id, table: kind.default_table(), place: kind.default_place(), kind })]);
    id
}

/// A pasted dimension measures in its view's copy and shows no model dimension: the one it was
/// copied from still shows it, so the model dimension shows once.
#[test]
fn a_pasted_dimension_shows_no_model_dimension() {
    let mut d = plate();
    let (front, _) = front_and_right(&d);
    let shown = ModelDimension::Feature { feature: 2, param: "Distance".into() };
    Arc::make_mut(&mut d.dimensions[0]).from_model = Some(shown.clone());
    let (old, from) = (d.dimensions[0].id, d.clone());
    let made = d.paste_records(&from, &from.with_contents(vec![front]), &[], here(&from), &mut IdSource::seeded(1)).unwrap();
    let copy = made.ids.iter().find(|(kind, was, _)| *kind == "dimension" && *was == old).map(|(_, _, id)| *id).unwrap();
    assert_eq!(d.dimension(copy).unwrap().from_model, None);
    assert_eq!(d.dimension(copy).unwrap().view, made.views[0]);
    assert_eq!(d.dimension(old).unwrap().from_model, Some(shown));
    d.validate().unwrap();
}

/// A balloon names its parts list's copy when the list is pasted too; the list itself when the
/// drawing it is pasted into has it; else none, as after a delete.
#[test]
fn a_pasted_balloon_keeps_the_parts_list_its_drawing_has() {
    let mut d = plate();
    let (front, _) = front_and_right(&d);
    let list = add_list(&mut d, &mut IdSource::seeded(2));
    Arc::make_mut(&mut d.balloons[0]).parts_list = Some(list);
    let from = d.clone();
    let views = from.with_contents(vec![front]);
    let balloon_of = |d: &Drawing, made: &Pasted| {
        let copy = made.ids.iter().find(|(kind, ..)| *kind == "balloon").map(|(_, _, id)| *id).unwrap();
        d.balloons.by_id(copy).unwrap().parts_list
    };

    let made = d.paste_records(&from, &views, &[], here(&from), &mut IdSource::seeded(3)).unwrap();
    assert_eq!(balloon_of(&d, &made), Some(list), "the same drawing keeps the list");

    let mut elsewhere = other();
    let place = here(&elsewhere);
    let made = elsewhere.paste_records(&from, &views, &[], place, &mut IdSource::seeded(4)).unwrap();
    assert_eq!(balloon_of(&elsewhere, &made), None, "another drawing hasn't the list");

    let mut elsewhere = other();
    let with_list = OnSheet { tables: vec![list], ..views.clone() };
    let place = here(&elsewhere);
    let made = elsewhere.paste_records(&from, &with_list, &[], place, &mut IdSource::seeded(5)).unwrap();
    assert_eq!(balloon_of(&elsewhere, &made), Some(made.tables[0]), "the list's copy");
    let Kind::PartsList(copied) = &elsewhere.tables.by_id(made.tables[0]).unwrap().kind else { panic!("a parts list") };
    assert_eq!(copied.of.source, elsewhere.sources[0].id);
}

/// A projected view pasted without its parent is a base view that looks as it did, and draws the
/// same state of the assembly: exploded, posed, a configuration's row, as its chain set them.
#[test]
fn a_projected_view_pasted_alone_keeps_its_assembly_state() {
    let mut d = plate();
    let (front, right) = front_and_right(&d);
    let state = ViewOverrides {
        exploded: Some(Exploded::View(9)),
        pose: Some(Pose::Study(StudyStep { id: 5, step: 2 })),
        configuration: Some(ViewConfiguration::Row(4)),
        ..d.view(front).unwrap().settings.clone()
    };
    Arc::make_mut(d.views.iter_mut().find(|v| v.id == front).unwrap()).settings = state;
    let from = d.clone();
    let before = from.resolve_view(right).unwrap();
    let by = Vec2::new(10.0, -5.0);
    let place = Placement { by, ..here(&from) };
    let made = d.paste_records(&from, &from.with_contents(vec![right]), &[], place, &mut IdSource::seeded(6)).unwrap();
    let copy = made.views[0];
    assert!(made.bases.contains(&copy) && made.old_bases.contains(&right) && made.based_across == 0);
    assert!(matches!(d.view(copy).unwrap().kind, ViewKind::Base { .. }));
    let after = d.resolve_view(copy).unwrap();
    assert_eq!(after.exploded.value, Exploded::View(9));
    assert_eq!(after.pose.value, Pose::Study(StudyStep { id: 5, step: 2 }));
    assert_eq!(after.configuration.value, ViewConfiguration::Row(4));
    assert_eq!((after.scale.value, after.visual_style.value), (before.scale.value, before.visual_style.value));
    assert_eq!(d.view(copy).unwrap().place, {
        let at = from.view_centre(right).unwrap() + by;
        ViewPlace::At([at.x, at.y])
    });
    d.validate().unwrap();
}

/// Every record pasted gets an id no record of the drawing had, each a different one, allocated
/// in one go; a copy naming a view its drawing hasn't is refused with the drawing unchanged.
#[test]
fn every_pasted_record_gets_a_new_id_and_a_refusal_changes_nothing() {
    let mut d = plate();
    let had: BTreeSet<u64> = d.ids().collect();
    let from = d.clone();
    let sheets: Vec<u64> = from.sheets.iter().map(|s| s.id).collect();
    let all = sheets.iter().fold(OnSheet::default(), |mut all, id| {
        let on = from.on_sheet(*id);
        all.views.extend(on.views);
        all.dimensions.extend(on.dimensions);
        all.balloons.extend(on.balloons);
        all.notes.extend(on.notes);
        all
    });
    let made = d.paste_records(&from, &all, &sheets, here(&from), &mut IdSource::seeded(7)).unwrap();
    let new: BTreeSet<u64> = made.ids.iter().map(|(_, _, id)| *id).collect();
    assert_eq!(new.len(), made.ids.len());
    assert!(new.is_disjoint(&had));
    assert_eq!(made.sheets.len(), 2);
    assert!(d.sheets.iter().filter(|s| made.sheets.contains(&s.id)).all(|s| s.name.starts_with("Sheet ")), "taken names move on");
    d.validate().unwrap();

    let mut d = from.clone();
    let missing = OnSheet { views: vec![u64::MAX], ..OnSheet::default() };
    assert!(
        d.paste_records(&from, &missing, &[], here(&from), &mut IdSource::seeded(8)).unwrap_err().to_string().starts_with("the copy has no view")
    );
    assert_eq!(d, from);
}

/// The copies keep their originals' order: a drawing keeps its records, and paints them, in id
/// order, so each kind's copies take their new ids in the order of the ids they copy. Ten views
/// (the sheet's three and seven more like its front view) keep that order by chance about once
/// in 3.6 million.
#[test]
fn copies_keep_their_originals_order() {
    let mut d = plate();
    let (front, _) = front_and_right(&d);
    let like_front = d.view(front).unwrap().clone();
    let more = d.allocate_ids(7, &mut IdSource::seeded(13065)).unwrap();
    d.add_views(more.into_iter().map(|id| Arc::new(View { id, ..like_front.clone() })));
    let from = d.clone();
    let views: Vec<u64> = from.views.iter().filter(|v| v.sheet == from.sheets[0].id).map(|v| v.id).collect();
    assert_eq!(views.len(), 10);
    let made = d.paste_records(&from, &from.with_contents(views), &[], here(&from), &mut IdSource::seeded(9)).unwrap();
    for kind in ["view", "dimension"] {
        let new: Vec<u64> = made.ids.iter().filter(|(k, _, _)| *k == kind).map(|(_, _, id)| *id).collect();
        assert!(made.ids.iter().filter(|(k, _, _)| *k == kind).map(|(_, old, _)| *old).is_sorted(), "{kind}: the copy lists by id");
        assert!(new.is_sorted(), "{kind}: the copies' ids are in their originals' order: {new:?}");
    }
}

/// A sheet pasted whole brings its sheet sketches and their fills: each sketch on the sheet's copy,
/// each fill on its sketch's copy. A fill's hatch pattern is used again where the drawing has an
/// equal one (always, in the drawing it came from), else copied once.
#[test]
fn a_pasted_sheet_brings_its_sketches_and_fills_and_reuses_equal_patterns() {
    let (doc, id, _) = crate::drawing::tests::w38d_record_fixture(false);
    let mut d = (**doc.drawing(id).unwrap()).clone();
    Arc::make_mut(&mut d.fills[0]).paint = FillPaint::Hatch { pattern: 71, pen_width_mm: 0.25 };
    d.validate().unwrap();
    let (from, sheet) = (d.clone(), d.sheets[0].id);
    let on = from.on_sheet(sheet);
    assert_eq!((on.sketches.as_slice(), on.fills.as_slice()), ([41].as_slice(), [70].as_slice()));
    let new = |made: &Pasted, kind: &str| made.ids.iter().find(|(k, ..)| *k == kind).map(|(_, _, id)| *id);

    let mut elsewhere = other();
    let made = elsewhere.paste_records(&from, &on, &[sheet], here(&elsewhere), &mut IdSource::seeded(10)).unwrap();
    let (sketch, fill, pattern) = (new(&made, "sheet sketch").unwrap(), new(&made, "fill").unwrap(), new(&made, "pattern").unwrap());
    assert_eq!(elsewhere.sheet_sketch(sketch).unwrap().sheet, made.sheets[0]);
    let pasted = elsewhere.fills.iter().find(|f| f.id == fill).unwrap();
    assert_eq!((pasted.sketch, &pasted.paint), (sketch, &FillPaint::Hatch { pattern, pen_width_mm: 0.25 }));
    assert_eq!(elsewhere.patterns.iter().map(|p| p.id).collect::<Vec<_>>(), [pattern], "copied once");
    elsewhere.validate().unwrap();

    let mut same = from.clone();
    let made = same.paste_records(&from, &on, &[sheet], here(&from), &mut IdSource::seeded(11)).unwrap();
    assert_eq!(new(&made, "pattern"), None);
    let fill = new(&made, "fill").unwrap();
    assert_eq!(same.fills.iter().find(|f| f.id == fill).unwrap().paint, FillPaint::Hatch { pattern: 71, pen_width_mm: 0.25 });
    assert_eq!(same.patterns.len(), 1, "the drawing's own pattern, used again");
    same.validate().unwrap();
}

/// The plate drawing with a section of its front view, lettered, at a point; its id.
fn with_section(d: &mut Drawing) -> u64 {
    let (front, _) = front_and_right(d);
    let id = d.allocate_id(&mut IdSource::seeded(12)).unwrap();
    let line = CuttingLine::new(SectionKind::Full, vec![Vec2::new(-60.0, 0.0), Vec2::new(60.0, 0.0)], ArrowSide::Left).unwrap();
    let kind = ViewKind::Section { parent: front, line };
    let sheet = d.sheets[0].id;
    let settings = ViewOverrides::default();
    d.add_views([Arc::new(View { id, sheet, kind, place: ViewPlace::At([300.0, 80.0]), settings, reference_shown_percent: 0.0, label: None })]);
    d.give_letter(id).unwrap();
    d.validate().unwrap();
    id
}

/// The copy of view `old` a paste made.
fn copy_of(made: &Pasted, old: u64) -> u64 {
    made.ids.iter().find(|(kind, was, _)| *kind == "view" && *was == old).map(|(_, _, id)| *id).unwrap()
}

/// A section pasted with its parent cuts the parent's copy. Pasted alone into the drawing it came
/// from, it still cuts its parent; into another drawing, which hasn't it, it is refused with that
/// drawing unchanged: a section's cut is what it shows, so it never becomes a base view.
#[test]
fn a_section_is_pasted_with_its_parent_or_refused_without_it() {
    let mut d = plate();
    let section = with_section(&mut d);
    let (front, _) = front_and_right(&d);
    let from = d.clone();
    let parent_of = |d: &Drawing, id: u64| d.view(id).unwrap().kind.parent();

    let mut elsewhere = other();
    let both = from.with_contents(vec![front, section]);
    let made = elsewhere.paste_records(&from, &both, &[], here(&elsewhere), &mut IdSource::seeded(13)).unwrap();
    assert_eq!(parent_of(&elsewhere, copy_of(&made, section)), Some(copy_of(&made, front)));
    assert!(matches!(elsewhere.view(copy_of(&made, section)).unwrap().kind, ViewKind::Section { .. }));
    elsewhere.validate().unwrap();

    let made = d.paste_records(&from, &from.with_contents(vec![section]), &[], here(&from), &mut IdSource::seeded(14)).unwrap();
    assert_eq!(parent_of(&d, made.views[0]), Some(front));
    d.validate().unwrap();

    let mut elsewhere = other();
    let before = elsewhere.clone();
    let alone = from.with_contents(vec![section]);
    let e = elsewhere.paste_records(&from, &alone, &[], here(&before), &mut IdSource::seeded(15)).unwrap_err().to_string();
    assert!(e.contains(&format!("view {} is a section of view {}", ids::encode(section), ids::encode(front))), "{e}");
    assert_eq!(elsewhere, before);
}

/// A pictorial or an auxiliary view pasted alone is a base view that looks as it did. With its
/// parent it follows the parent's copy, unless the two drawings' projection angles differ: each is
/// made from its parent by the angle, so then it is a base view too. A section's cut is the same
/// in either angle, so it keeps its parent.
#[test]
fn pictorial_and_auxiliary_views_become_base_views_alone_or_across_angles() {
    let mut d = plate();
    let section = with_section(&mut d);
    let (front, _) = front_and_right(&d);
    let sheet = d.sheets[0].id;
    let kinds = [ViewKind::Pictorial { parent: front, diagonal: Diagonal::AboveRight }, ViewKind::Auxiliary { parent: front, toward: [1.0, 0.0] }];
    let added = d.allocate_ids(2, &mut IdSource::seeded(16)).unwrap();
    for (id, kind) in added.iter().zip(kinds) {
        let (place, settings) = (ViewPlace::At([60.0, 60.0]), ViewOverrides::default());
        d.add_views([Arc::new(View { id: *id, sheet, kind, place, settings, reference_shown_percent: 0.0, label: None })]);
        d.give_letter(*id).unwrap();
    }
    d.validate().unwrap();
    let from = d.clone();
    for old in &added {
        let mut alone = other();
        let made = alone.paste_records(&from, &from.with_contents(vec![*old]), &[], here(&alone), &mut IdSource::seeded(17)).unwrap();
        let copy = made.views[0];
        assert!(matches!(alone.view(copy).unwrap().kind, ViewKind::Base { .. }));
        assert_eq!(alone.resolve_view(copy).unwrap().basis, from.resolve_view(*old).unwrap().basis);
        alone.validate().unwrap();
    }
    let mut across = other();
    across.projection = match from.projection {
        Projection::First => Projection::Third,
        Projection::Third => Projection::First,
    };
    let mut all = vec![front, section];
    all.extend(&added);
    let made = across.paste_records(&from, &from.with_contents(all), &[], here(&across), &mut IdSource::seeded(18)).unwrap();
    assert_eq!(made.based_across, 2, "the pictorial and the auxiliary view");
    let kinds: Vec<&str> = made.views.iter().map(|id| across.view(*id).unwrap().kind.name()).collect();
    assert_eq!(kinds.iter().filter(|k| **k == "base").count(), 3, "{kinds:?}");
    assert_eq!(kinds.iter().filter(|k| **k == "section").count(), 1, "{kinds:?}");
    across.validate().unwrap();
}

/// A pasted view needs a letter only as any view does; one that needs one keeps its letter when no
/// view of the drawing holds it, else gets the next free one. A base view has none.
#[test]
fn pasted_views_take_letters_only_as_they_need_them() {
    let mut d = plate();
    let section = with_section(&mut d);
    let (front, _) = front_and_right(&d);
    Arc::make_mut(d.views.iter_mut().find(|v| v.id == front).unwrap()).label = Some(ViewLabel::new("Q".into()));
    let from = d.clone();
    let letter = |d: &Drawing, id: u64| d.view(id).unwrap().label.as_ref().map(|l| l.letter.clone());
    let original = letter(&from, section).unwrap();
    let both = from.with_contents(vec![front, section]);

    let made = d.paste_records(&from, &both, &[], here(&from), &mut IdSource::seeded(19)).unwrap();
    assert_eq!(letter(&d, copy_of(&made, front)), None, "a base view needs no letter");
    assert!(letter(&d, copy_of(&made, section)).is_some_and(|l| l != original), "the original holds its letter");
    d.validate().unwrap();

    let mut elsewhere = other();
    let made = elsewhere.paste_records(&from, &both, &[], here(&elsewhere), &mut IdSource::seeded(20)).unwrap();
    assert_eq!(letter(&elsewhere, copy_of(&made, section)), Some(original), "free there, so kept");
}

/// Pasted base and flat pattern views draw the design the drawing is in, whatever order its
/// sources are listed in: a source with a path names another file.
#[test]
fn pasted_views_draw_the_design_the_drawing_is_in() {
    let mut d = plate();
    let (front, _) = front_and_right(&d);
    let flat = d.allocate_id(&mut IdSource::seeded(21)).unwrap();
    let (sheet, source) = (d.sheets[0].id, d.sources[0].id);
    let kind = ViewKind::Flat { source, body: "Plate".into(), turn: QuarterTurn::None };
    let settings = ViewOverrides { scale: Some("1:1".parse().unwrap()), ..ViewOverrides::default() };
    d.add_views([Arc::new(View { id: flat, sheet, kind, place: ViewPlace::At([60.0, 250.0]), settings, reference_shown_percent: 0.0, label: None })]);
    d.validate().unwrap();
    let from = d.clone();
    let mut elsewhere = other();
    let own = elsewhere.sources[0].id;
    elsewhere.sources.insert(0, Source { id: own.min(2) - 1, path: Some("Elsewhere/part.sc".into()) });
    assert_eq!(elsewhere.design_source(), Some(own));
    let place = here(&elsewhere);
    let made = elsewhere.paste_records(&from, &from.with_contents(vec![front, flat]), &[], place, &mut IdSource::seeded(22)).unwrap();
    for copy in &made.views {
        match &elsewhere.view(*copy).unwrap().kind {
            ViewKind::Base { source, .. } | ViewKind::Flat { source, .. } => assert_eq!(*source, own),
            kind => panic!("{kind:?}"),
        }
    }
    elsewhere.validate().unwrap();
}
