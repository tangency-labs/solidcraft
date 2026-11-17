//! A drawing's records pasted into another drawing, or into the same one (#13065): each record gets
//! a new id, and every link between them follows it through one old-to-new map
//! ([`Drawing::paste_records`]). What a copy keeps and what it drops is said here once, for every
//! kind of record a sheet holds:
//!
//! - a view keeps its settings and goes on its pasted sheet, or on the sheet pasted onto. A
//!   projected, pictorial or auxiliary view whose parent isn't pasted, or every one when the two
//!   drawings' projection angles differ (each is made from its parent by the angle), becomes a base
//!   view that looks as it did ([`View::as_base`]). A section keeps its cut, which is what it shows,
//!   in either angle: it cuts its parent's copy, or its parent itself in the drawing it came from,
//!   and else the paste is refused. A base or flat pattern view draws this drawing's design;
//! - a view needs a letter only as any view does: it keeps its own when no view of this drawing
//!   holds it, else gets the next free one ([`Drawing::give_letter`]);
//! - a dimension measures in its view's copy and shows no model dimension: the one it was copied
//!   from still does, and each shows once;
//! - a balloon names its parts list's copy when the list is pasted too, the list itself when this
//!   drawing has it (a paste into the drawing it came from), else none, so it reads the lists it
//!   can, as after a delete;
//! - a note goes on its sheet's copy, or on its view's copy;
//! - an image file with the same bytes as one this drawing has is used again;
//! - a sheet's sketches go on its copy and their fills on their sketches' copies; a fill's hatch
//!   pattern is one this drawing has that is equal, else a copy;
//! - a sheet keeps its name when no sheet has it, else gets the next free one, and names this
//!   drawing's design; a parts list too;
//! - revision tags are never pasted: each shows a row of its own drawing's revision record;
//! - each kind's copies keep their originals' order, which is their id order: the copies' new ids
//!   are in that order too, so they paint as the originals do.
//!
//! The model references stay as they were: moving them onto another part is the caller's.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use solidcore_record::Vec2;

use super::balloon::Balloon;
use super::dimension::Dimension;
use super::image::{ImageFile, SheetImage};
use super::note::{Anchor, Note};
use super::table::{Kind, SheetTable};
use super::view::{View, ViewKind, shifted};
use super::{Drawing, FillPaint, OnSheet, PatternSnapshot, Sheet, SheetComponent, SheetSketch, SketchFill};
use crate::ids::IdSource;
use crate::{DocError, Result, ids};

/// Where a paste puts what it copies.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    /// The sheet the views and tables copied without their sheet go on.
    pub onto: u64,
    /// How far every copied view moves on paper.
    pub by: Vec2,
    /// This drawing's source: the design a pasted base or flat pattern view draws, and a pasted
    /// sheet or parts list names ([`Drawing::design_source`]).
    pub source: u64,
}

/// What [`Drawing::paste_records`] made.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pasted {
    /// Each record copied, by its kind, the id it had and its copy's, in the order they were made.
    pub ids: Vec<(&'static str, u64, u64)>,
    /// The views' copies, in the copy's order.
    pub views: Vec<u64>,
    /// The base views among them, by the ids they were copied from.
    pub old_bases: BTreeSet<u64>,
    /// The same base views, by their own ids.
    pub bases: BTreeSet<u64>,
    /// The new sheets, in the copy's order.
    pub sheets: Vec<u64>,
    /// The new tables, in the copy's order.
    pub tables: Vec<u64>,
    /// How many projected views became base views because the projection angles differ.
    pub based_across: usize,
}

impl Drawing {
    /// Copies of `what` of drawing `from` (which may be this one, as it was), and of the sheets
    /// `sheets` of it whole, put in this drawing as `place` says, each with a new id from
    /// `id_source`: the rules are the module's. Every new id is allocated at once. Refused, with
    /// this drawing unchanged, when the copy names a view `from` doesn't have, or a projected view
    /// that can't be resolved; the caller checks the drawing's rules after.
    pub fn paste_records(&mut self, from: &Drawing, what: &OnSheet, sheets: &[u64], place: Placement, id_source: &mut IdSource) -> Result<Pasted> {
        // Every kind a sheet holds, with no `..`: a new kind fails to build here until it says how
        // it is pasted. Revision tags show a row of their own drawing's record: never pasted.
        let OnSheet { views, dimensions, balloons, images, tags: _, tables, notes, sketches, fills } = what;
        let files: BTreeSet<u64> = images.iter().filter_map(|id| from.sheet_images.by_id(*id)).map(|image| image.image).collect();
        let mut new: BTreeMap<u64, u64> = BTreeMap::new();
        for file in files.iter().filter_map(|id| from.images.by_id(*id)) {
            if let Some(same) = self.images.iter().find(|x| x.format == file.format && x.data == file.data) {
                new.insert(file.id, same.id);
            }
        }
        let fresh_files: Vec<u64> = files.iter().filter(|id| !new.contains_key(id)).copied().collect();
        let patterns: BTreeSet<u64> = fills
            .iter()
            .filter_map(|id| from.fills.iter().find(|fill| fill.id == *id))
            .filter_map(|fill| match fill.paint {
                FillPaint::Hatch { pattern, .. } => Some(pattern),
                FillPaint::Solid => None,
            })
            .collect();
        for snapshot in from.patterns.iter().filter(|snapshot| patterns.contains(&snapshot.id)) {
            if let Some(same) = self.patterns.iter().find(|x| x.pattern == snapshot.pattern) {
                new.insert(snapshot.id, same.id);
            }
        }
        let fresh_patterns: Vec<u64> = patterns.iter().filter(|id| !new.contains_key(id)).copied().collect();
        let kinds: [(&'static str, &[u64]); 11] = [
            ("sheet", sheets),
            ("view", views),
            ("dimension", dimensions),
            ("balloon", balloons),
            ("note", notes),
            ("image file", &fresh_files),
            ("image", images),
            ("table", tables),
            ("sheet sketch", sketches),
            ("pattern", &fresh_patterns),
            ("fill", fills),
        ];
        let count = kinds.iter().map(|(_, list)| list.len()).sum();
        // Given out in order, and each kind's list is in id order, so the copies keep their
        // originals' order: a drawing keeps its records, and paints them, in id order.
        let mut fresh = self.allocate_ids(count, id_source)?;
        fresh.sort_unstable();
        let mut fresh = fresh.into_iter();
        let mut made = Pasted::default();
        for (kind, old) in kinds.iter().flat_map(|(kind, list)| list.iter().map(move |old| (*kind, *old))) {
            let id = fresh.next().ok_or_else(|| DocError::Invalid("no free id".into()))?;
            new.insert(old, id);
            made.ids.push((kind, old, id));
        }
        let map = |old: u64| new.get(&old).copied();
        let sheet_of = |old: u64| map(old).unwrap_or(place.onto);

        // Views first, as the drawing still is: a refusal leaves it unchanged.
        let across = from.projection != self.projection;
        let same = from.id == self.id;
        let mut copies = Vec::with_capacity(views.len());
        for old in views {
            let missing = || DocError::Invalid(format!("the copy has no view {}", ids::encode(*old)));
            let (original, id) = from.view(*old).zip(map(*old)).ok_or_else(missing)?;
            let mut view = View { id, sheet: sheet_of(original.sheet), place: shifted(original, place.by), ..original.clone() };
            // Every kind, with no `_`: a new kind fails to build here until it says how it is pasted.
            match &original.kind {
                ViewKind::Base { view: direction, .. } => view.kind = ViewKind::Base { source: place.source, view: *direction },
                ViewKind::Flat { body, turn, .. } => view.kind = ViewKind::Flat { source: place.source, body: body.clone(), turn: *turn },
                ViewKind::Section { parent, .. } => match map(*parent) {
                    Some(copy) => view.kind = original.kind.with_parent(copy),
                    None if same && self.view(*parent).is_some() => {}
                    None => {
                        let (section, parent) = (ids::encode(*old), ids::encode(*parent));
                        return Err(DocError::Invalid(format!(
                            "view {section} is a section of view {parent}, which this drawing hasn't: copy the two together"
                        )));
                    }
                },
                ViewKind::Projected { parent, .. } | ViewKind::Pictorial { parent, .. } | ViewKind::Auxiliary { parent, .. } => {
                    match map(*parent).filter(|_| !across) {
                        Some(copy) => view.kind = original.kind.with_parent(copy),
                        None => {
                            let resolved = from.resolve_view(*old)?;
                            view = view.as_base(&resolved, place.source, from.view_centre(*old)? + place.by);
                            made.based_across += usize::from(across);
                        }
                    }
                }
            }
            if !view.needs_letter() {
                view.label = None;
            }
            if matches!(view.kind, ViewKind::Base { .. }) {
                made.old_bases.insert(*old);
                made.bases.insert(id);
            }
            made.views.push(id);
            copies.push(Arc::new(view));
        }
        // The parts lists this drawing had before the paste, for the balloons.
        let lists: BTreeSet<u64> = self.tables.iter().filter(|t| matches!(t.kind, Kind::PartsList(_))).map(|t| t.id).collect();
        for original in from.sheets.iter().filter(|sheet| sheets.contains(&sheet.id)) {
            let name = if self.sheets.iter().any(|x| x.name == original.name) { self.free_sheet_name() } else { original.name.clone() };
            let documents = original.documents.map(|c| SheetComponent { source: place.source, ..c });
            let id = sheet_of(original.id);
            self.sheets.push(Arc::new(Sheet { id, name, documents, ..Sheet::clone(original) }));
            made.sheets.push(id);
        }
        self.add_views(copies);
        for id in &made.views {
            self.give_letter(*id)?;
        }
        self.add_dimensions(dimensions.iter().filter_map(|old| {
            let original = from.dimension(*old)?;
            Some(Arc::new(Dimension { id: map(*old)?, view: map(original.view)?, from_model: None, ..original.clone() }))
        }));
        self.balloons.add(balloons.iter().filter_map(|old| {
            let original = from.balloons.by_id(*old)?;
            let parts_list = original.parts_list.and_then(|list| map(list).or_else(|| lists.contains(&list).then_some(list)));
            Some(Arc::new(Balloon { id: map(*old)?, view: map(original.view)?, parts_list, ..original.clone() }))
        }));
        self.notes.add(notes.iter().filter_map(|old| {
            let original = from.notes.by_id(*old)?;
            let anchor = match original.anchor {
                Anchor::Sheet(sheet) => Anchor::Sheet(sheet_of(sheet)),
                _ => original.anchor.on_view(map(original.anchor.view()?)?),
            };
            Some(Arc::new(Note { id: map(*old)?, anchor, ..original.clone() }))
        }));
        self.images.add(fresh_files.iter().filter_map(|old| Some(Arc::new(ImageFile { id: map(*old)?, ..from.images.by_id(*old)?.clone() }))));
        self.sheet_images.add(images.iter().filter_map(|old| {
            let original = from.sheet_images.by_id(*old)?;
            Some(Arc::new(SheetImage { id: map(*old)?, sheet: sheet_of(original.sheet), image: map(original.image)?, ..original.clone() }))
        }));
        let mut pasted_tables = Vec::with_capacity(tables.len());
        for old in tables {
            let Some((original, id)) = from.tables.by_id(*old).zip(map(*old)) else { continue };
            let mut table = SheetTable { id, sheet: sheet_of(original.sheet), ..original.clone() };
            if let Kind::PartsList(list) = &mut table.kind {
                list.of.source = place.source;
            }
            made.tables.push(id);
            pasted_tables.push(Arc::new(table));
        }
        self.tables.add(pasted_tables);
        self.sketches.extend(sketches.iter().filter_map(|old| {
            let original = from.sheet_sketch(*old)?;
            Some(Arc::new(SheetSketch { id: map(*old)?, sheet: sheet_of(original.sheet), ..SheetSketch::clone(original) }))
        }));
        self.patterns.extend(fresh_patterns.iter().filter_map(|old| {
            let original = from.patterns.iter().find(|snapshot| snapshot.id == *old)?;
            Some(Arc::new(PatternSnapshot { id: map(*old)?, ..PatternSnapshot::clone(original) }))
        }));
        self.fills.extend(fills.iter().filter_map(|old| {
            let original = from.fills.iter().find(|fill| fill.id == *old)?;
            let paint = match original.paint {
                FillPaint::Hatch { pattern, pen_width_mm } => FillPaint::Hatch { pattern: map(pattern)?, pen_width_mm },
                FillPaint::Solid => FillPaint::Solid,
            };
            Some(Arc::new(SketchFill { id: map(*old)?, sketch: map(original.sketch)?, paint, ..SketchFill::clone(original) }))
        }));
        // Kept by id, as a drawing read from its file keeps them.
        self.sketches.sort_by_key(|record| record.id);
        self.patterns.sort_by_key(|record| record.id);
        self.fills.sort_by_key(|record| record.id);
        Ok(made)
    }
}

#[cfg(test)]
#[path = "paste_tests.rs"]
mod tests;
