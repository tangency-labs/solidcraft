//! `drawing.source.replace` (#13029): views of one part made to show another with the same
//! history (its Paste New copy, or one built the same way), keeping each annotation that still
//! finds what it points at on the new part.
//!
//! The two parts' features are paired by their timelines ([`FeaturePairs::between`]), each
//! reference is moved through the pairs, and an annotation whose every reference then finds the one
//! edge, corner or face it names on the new part is kept ([`solidcraft_doc::Document::finds_each`]).
//! Nothing reattaches to a nearby edge: an annotation that doesn't come across keeps its old
//! record, which names the old part, so its view leaves it undrawn and `drawing.status` says why.
//! One undo step; no model is evaluated.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::Value;
use solidcraft_doc::drawing::balloon::Balloon;
use solidcraft_doc::drawing::dimension::{Dimension, Reference};
use solidcraft_doc::drawing::note::Note;
use solidcraft_doc::drawing::table::Kind;
use solidcraft_doc::drawing::view::ViewKind;
use solidcraft_doc::drawing::{Drawing, SheetComponent};
use solidcraft_doc::feature_pairs::{FeaturePairs, Side};
use solidcraft_doc::{Document, ids};

use super::CommandSpec;
use crate::params::bad;
use crate::schema::drawing_source::{DroppedHideResult, KeptResult, LostResult, ReplaceParams, ReplaceResult, ToParam};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const REPLACE: &str = "drawing.source.replace";
const REPLACE_CONTRACT: TypedContract<ReplaceParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(REPLACE, "Replace Model", crate::checked!("drawing.source.replace", replace))
    .icon("edit")
    .in_drawing()
    .doc_only()
    .contract(&REPLACE_CONTRACT)
    .output(output_schema_for::<ReplaceResult>)
    .params(
        "exactly one of view: a base view's id (it and the views projected or cut from it), sheet: id or name (its base views, the \
         component it documents and its parts lists), drawing: true (every sheet); to: {component: id (0 the design), path?: the \
         occurrences it is placed through, from the top (default its only placement)}; dry_run?: true to answer without changing \
         anything → {views, kept: [{id, kind}], lost: [{id, kind, why}], dropped_hide: [{view, path}], sheets, parts_lists, \
         dry_run}. Each view must show one part, all of them the same one, and the new part has the same history: the two parts' \
         features pair in timeline order while their kinds match. An annotation whose every reference finds the one thing it \
         names on the new part is moved onto it; one that doesn't keeps its old record, and its view no longer draws it \
         (drawing.status lists it as not_drawn). A hide path under the old part is dropped, and a flat pattern view is left as it is. \
         Not supported yet: a view of more than one part. One undo step, none for a dry run; no model is evaluated.",
    )];

/// One part of the design: its component and the occurrences it is placed through.
#[derive(Clone, Debug, PartialEq)]
struct Part {
    component: u64,
    path: Vec<u64>,
}

fn replace(s: &mut Session, p: &Value) -> Result<Value> {
    let request: ReplaceParams = crate::schema::read(REPLACE, p)?;
    let record = s.drawing(REPLACE)?.clone();
    // Every record a drawing holds, with no `..`: a new kind fails to build here until it says
    // whether a replace moves it. Views, dimensions, notes and balloons name the model and move; a
    // sheet's documented component and a parts list's follow the part; the rest name paper, files,
    // revision rows or the drawing itself.
    let Drawing {
        sheets: _,
        views: _,
        dimensions: _,
        balloons: _,
        notes: _,
        tables: _,
        format: _,
        format_pending: _,
        id: _,
        name: _,
        sources: _,
        properties: _,
        units: _,
        projection: _,
        updates: _,
        language: _,
        font: _,
        line_group: _,
        images: _,
        sheet_images: _,
        revisions: _,
        newer_file: _,
        view_letters: _,
        symbols: _,
        sketches: _,
        fills: _,
        patterns: _,
    } = &*record;
    let (bases, sheets) = scope(&record, &request)?;
    let from = one_part(&s.doc, &record, &bases)?;
    let to = target(&s.doc, &request.to)?;
    if to == from {
        return Err(bad(REPLACE, "the views show that part already"));
    }
    let state = s.model.state();
    let side = |part: &Part| Side { doc: &s.doc, state: &state, component: part.component };
    let pairs = FeaturePairs::between(side(&from), side(&to));
    let mut d = Drawing::clone(&record);
    let views = record.with_projected(&bases);
    let mut reply = ReplaceResult {
        views: views.iter().map(|v| ids::encode(*v)).collect(),
        kept: Vec::new(),
        lost: Vec::new(),
        dropped_hide: Vec::new(),
        sheets: Vec::new(),
        parts_lists: Vec::new(),
        dry_run: request.dry_run.unwrap_or(false),
        param_warnings: Vec::new(),
    };
    for view in d.views.iter_mut().filter(|v| views.contains(&v.id)) {
        let view = Arc::make_mut(view);
        if bases.contains(&view.id) {
            view.settings.occurrences = Some(Arc::from(vec![to.path.clone()]));
        }
        // A copy's nested occurrences have other ids, so a hide path under the old part names
        // nothing in the new one.
        if let Some(hide) = view.settings.hide.take() {
            let (under, kept): (Vec<Vec<u64>>, Vec<Vec<u64>>) = hide.iter().cloned().partition(|path| path.starts_with(&from.path));
            reply.dropped_hide.extend(under.into_iter().map(|path| DroppedHideResult { view: ids::encode(view.id), path }));
            view.settings.hide = (!kept.is_empty()).then(|| Arc::from(kept));
        }
    }
    let moves = move_annotations(s, &mut d, &views, &pairs, (&from.path, &to.path));
    (reply.kept, reply.lost) = (moves.kept, moves.lost);
    // What a sheet documents and what its parts lists list follow the part, from the same design.
    let source = d.resolve_view(bases.first().copied().unwrap_or_default()).map_err(|e| bad(REPLACE, e.to_string()))?.source;
    let (old, new) = (SheetComponent { source, component: from.component }, SheetComponent { source, component: to.component });
    for sheet in d.sheets.iter_mut().filter(|sheet| sheets.contains(&sheet.id) && sheet.documents == Some(old)) {
        Arc::make_mut(sheet).documents = Some(new);
        reply.sheets.push(ids::encode(sheet.id));
    }
    for table in d.tables.iter_mut().filter(|table| sheets.contains(&table.sheet)) {
        let id = table.id;
        match &mut Arc::make_mut(table).kind {
            Kind::PartsList(list) if list.of == old => {
                list.of = new;
                reply.parts_lists.push(ids::encode(id));
            }
            Kind::PartsList(_) | Kind::Revision => {}
        }
    }
    if !reply.dry_run {
        s.edit_drawing(REPLACE, |live| {
            *live = d;
            Ok(())
        })?;
    }
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// The base views a request names, and the sheets whose documented component and parts lists
/// follow: none for a view.
fn scope(d: &Drawing, request: &ReplaceParams) -> Result<(BTreeSet<u64>, BTreeSet<u64>)> {
    let sheets: BTreeSet<u64> = match (&request.view, &request.sheet, request.drawing) {
        (Some(view), None, None) => {
            let found = d.view(view.0).ok_or_else(|| bad(REPLACE, format!("the drawing has no view {}", ids::encode(view.0))))?;
            let named = ids::encode(view.0);
            // Every kind, with no `_`: a new kind must say whether a replace starts from it.
            match &found.kind {
                ViewKind::Base { .. } => {}
                ViewKind::Projected { parent, .. }
                | ViewKind::Section { parent, .. }
                | ViewKind::Pictorial { parent, .. }
                | ViewKind::Auxiliary { parent, .. } => {
                    let (kind, parent) = (found.kind.name(), ids::encode(*parent));
                    return Err(bad(REPLACE, format!("view {named} is a {kind} view of view {parent}: name its base view")));
                }
                ViewKind::Flat { .. } => {
                    return Err(bad(REPLACE, format!("not supported yet: view {named} is a flat pattern view, which draws a body by name")));
                }
            }
            return Ok(([view.0].into(), BTreeSet::new()));
        }
        (None, Some(sheet), None) => {
            let at = d.sheet_index(sheet).map_err(|e| bad(REPLACE, e.to_string()))?;
            d.sheets.get(at).map(|sheet| sheet.id).into_iter().collect()
        }
        (None, None, Some(true)) => d.sheets.iter().map(|sheet| sheet.id).collect(),
        _ => return Err(bad(REPLACE, "give exactly one of view, sheet and drawing: true")),
    };
    let bases: BTreeSet<u64> =
        d.views.iter().filter(|v| sheets.contains(&v.sheet) && matches!(v.kind, ViewKind::Base { .. })).map(|v| v.id).collect();
    if bases.is_empty() {
        return Err(bad(REPLACE, "there are no views to replace there"));
    }
    Ok((bases, sheets))
}

/// The one part every view of `bases` shows ([`shown_part`]); refused when they show different
/// parts.
fn one_part(doc: &Document, d: &Drawing, bases: &BTreeSet<u64>) -> Result<Part> {
    let mut parts = bases.iter().map(|view| shown_part(doc, d, *view));
    let first = parts.next().unwrap_or_else(|| Err(bad(REPLACE, "there are no views to replace there")))?;
    for part in parts {
        if part? != first {
            return Err(bad(REPLACE, "the views there show different parts: replace each view on its own"));
        }
    }
    Ok(first)
}

/// The part base view `view` shows: the component at its one occurrence path, or the design's own
/// bodies (component 0, no path) when it draws them alone or the design places nothing else.
fn shown_part(doc: &Document, d: &Drawing, view: u64) -> Result<Part> {
    let resolved = d.resolve_view(view).map_err(|e| bad(REPLACE, e.to_string()))?;
    let several = || bad(REPLACE, format!("not supported yet: view {} shows more than one part", ids::encode(view)));
    let path = match resolved.occurrences.value.as_deref() {
        Some([path]) => path.clone(),
        None if doc.occurrences.is_empty() => Vec::new(),
        _ => return Err(several()),
    };
    let part = placed(doc, &path)
        .ok_or_else(|| bad(REPLACE, format!("view {} shows occurrence path {path:?}, which no longer exists", ids::encode(view))))?;
    if holds_parts(doc, &part) {
        return Err(several());
    }
    Ok(part)
}

/// The part the request's `to` names: refused when the design doesn't have it, it is placed more
/// than once and no path says which, or it holds other parts.
fn target(doc: &Document, to: &ToParam) -> Result<Part> {
    let path = match &to.path {
        Some(path) => path.clone(),
        None if to.component == 0 => Vec::new(),
        None => {
            let mut paths = doc.placements().into_iter().filter(|(c, _, _)| *c == to.component).map(|(_, path, _)| path);
            match (paths.next(), paths.next()) {
                (Some(path), None) => path,
                (Some(_), Some(_)) => return Err(bad(REPLACE, format!("component {} is placed more than once: give to.path", to.component))),
                (None, _) => return Err(bad(REPLACE, format!("the design has no placed component {}", to.component))),
            }
        }
    };
    let part = placed(doc, &path).filter(|part| part.component == to.component);
    let part = part.ok_or_else(|| bad(REPLACE, format!("occurrence path {path:?} doesn't place component {}", to.component)))?;
    if holds_parts(doc, &part) {
        return Err(bad(REPLACE, format!("not supported yet: component {} holds other parts", to.component)));
    }
    Ok(part)
}

/// The part occurrence path `path` places, if it still does.
fn placed(doc: &Document, path: &[u64]) -> Option<Part> {
    doc.path_placement(path).map(|(component, _)| Part { component, path: path.to_vec() })
}

/// Whether drawing `part` draws other parts too: those placed inside it. The design's own bodies
/// (no path) are drawn alone.
fn holds_parts(doc: &Document, part: &Part) -> bool {
    !part.path.is_empty() && doc.occurrences.iter().any(|o| o.parent == part.component)
}

/// Why an annotation doesn't come across: a feature or a body it names has no pair.
const NO_PAIR: &str = "a feature or body it names has no match on the new part";
/// Why an annotation doesn't come across: moved, it doesn't find its one thing.
const NOT_FOUND: &str = "what it points at isn't found as one thing on the new part";

/// What became of the annotations a replace moved: those that came across, and those that didn't,
/// with why.
#[derive(Default)]
struct Moves {
    kept: Vec<KeptResult>,
    lost: Vec<LostResult>,
}

impl Moves {
    /// Each of `moved` (an annotation's id, and its record moved onto the new part: `None` when a
    /// reference has no pair) kept when the next of `found` says every reference it has finds its
    /// one thing there, else lost with why. The kept records, to put in the drawing.
    fn sort<T>(&mut self, moved: Vec<(u64, Option<T>)>, kind: &'static str, found: &mut impl Iterator<Item = bool>) -> Vec<T> {
        let mut kept = Vec::new();
        for (id, record) in moved {
            let why = match record {
                None => NO_PAIR,
                Some(record) if found.next().unwrap_or(false) => {
                    kept.push(record);
                    self.kept.push(KeptResult { id: ids::encode(id), kind });
                    continue;
                }
                Some(_) => NOT_FOUND,
            };
            self.lost.push(LostResult { id: ids::encode(id), kind, why: why.to_string() });
        }
        kept
    }
}

/// Each dimension, note and balloon on `views` moved through `pairs` from occurrence path `from`
/// to `to` by its own record's rule ([`Dimension::moved`], [`Note::moved`], [`Balloon::moved`])
/// when every reference it has then finds its one thing, and listed as kept or lost. A note that
/// points at nothing stays as it is.
fn move_annotations(s: &Session, d: &mut Drawing, views: &BTreeSet<u64>, pairs: &FeaturePairs, (from, to): (&[u64], &[u64])) -> Moves {
    let dimensions: Vec<(u64, Option<Dimension>)> =
        d.dimensions.iter().filter(|r| views.contains(&r.view)).map(|r| (r.id, r.moved(pairs, from, to))).collect();
    let on_views = |n: &&Arc<Note>| n.on_model() && n.anchor.view().is_some_and(|v| views.contains(&v));
    let notes: Vec<(u64, Option<Note>)> = d.notes.iter().filter(on_views).map(|n| (n.id, n.moved(pairs, from, to))).collect();
    let balloons: Vec<(u64, Option<Balloon>)> =
        d.balloons.iter().filter(|b| views.contains(&b.view)).map(|b| (b.id, b.moved(pairs, from, to))).collect();
    // One look-up for every moved reference, each record's own list, in the order sorted below.
    let found: Vec<bool> = {
        let dimension = dimensions.iter().filter_map(|(_, r)| r.as_ref()).map(|r| r.kind.references());
        let note = notes.iter().filter_map(|(_, n)| n.as_ref()).map(|n| n.references().collect());
        let balloon = balloons.iter().filter_map(|(_, b)| b.as_ref()).map(|b| b.references().to_vec());
        s.doc.finds_each(&s.model, &dimension.chain(note).chain(balloon).collect::<Vec<Vec<Reference<'_>>>>())
    };
    let (mut found, mut moves) = (found.into_iter(), Moves::default());
    for record in moves.sort(dimensions, "dimension", &mut found) {
        if let Ok(at) = d.dimensions.binary_search_by_key(&record.id, |r| r.id)
            && let Some(slot) = d.dimensions.get_mut(at)
        {
            *slot = Arc::new(record);
        }
    }
    for record in moves.sort(notes, "note", &mut found) {
        if let Some(slot) = d.notes.index_of(record.id).and_then(|at| d.notes.get_mut(at)) {
            *slot = Arc::new(record);
        }
    }
    for record in moves.sort(balloons, "balloon", &mut found) {
        if let Some(slot) = d.balloons.index_of(record.id).and_then(|at| d.balloons.get_mut(at)) {
            *slot = Arc::new(record);
        }
    }
    moves
}

#[cfg(test)]
#[path = "drawing_source_tests.rs"]
mod tests;
