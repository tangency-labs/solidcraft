//! Sheet-owned sketch lifecycle on the embedded drawing receiver (W38a).
//! Geometry tools reuse cmd::sketch and cmd::sketch_store; there is no second sketcher.
//! A view's sketch may link to the model edges its view draws (#5425): they are read on the
//! session the view draws from ([`link_source`]) and follow each change in the command that made
//! it ([`follow_view_links`]), so every reader draws the curves the record holds.
use super::{CommandSpec, sketch_store};
use crate::drawing_view::ViewSessions;
use crate::params::bad;
use crate::schema::{drawing_sketch as contract, output_schema_for};
use crate::{Result, Session, SheetSketchEdit};
use serde::de::DeserializeOwned;
use serde_json::Value;
use solidcraft_doc::Document;
use solidcraft_doc::drawing::dimension::Status;
use solidcraft_doc::drawing::view::{View, ViewKind};
use solidcraft_doc::drawing::{Drawing, Owner, SheetSketch, ViewLinkState};
use solidcraft_doc::ids;
use solidcraft_sketch::Sketch;
use std::sync::Arc;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("drawing.sketch.create", "Create Sheet Sketch", crate::checked!("drawing.sketch.create", create))
        .in_drawing().doc_only().icon("sketch")
        .contract(&contract::CREATE).output(output_schema_for::<contract::Reading>)
        .params("owner: exact sheet or view id in the open drawing; name?: text. Creates and enters one sketch: a sheet's in paper XY millimetres, a view's in model millimetres in the view's plane; one drawing undo step."),
    CommandSpec::new("drawing.sketch.edit", "Edit Sheet Sketch", crate::checked!("drawing.sketch.edit", edit))
        .in_drawing().doc_only().noundo().icon("sketch")
        .contract(&contract::EDIT).output(output_schema_for::<contract::Reading>)
        .params("sketch: exact encoded sketch id in the open drawing. Enters that sketch (a sheet's in paper XY millimetres, a view's in model millimetres in the view's plane) without changing its data or the source design."),
    CommandSpec::new("drawing.sketch.inspect", "Inspect Sheet Sketch", crate::checked!("drawing.sketch.inspect", inspect))
        .in_drawing().doc_only().noundo().icon("inspect")
        .contract(&contract::INSPECT).output(output_schema_for::<contract::Reading>)
        .params("sketch?: exact encoded id, else the active sheet sketch. Reports resolved points in its owner's frame (paper millimetres for a sheet's sketch, model millimetres in the view's plane for a view's) and local entity ids without adopting the solution."),

];

fn parsed<T: DeserializeOwned>(cmd: &str, p: &Value) -> Result<T> {
    T::deserialize(p).map_err(|e| bad(cmd, e.to_string()))
}
fn id(cmd: &str, text: &str) -> Result<u64> {
    ids::decode(text).ok_or_else(|| bad(cmd, "an exact encoded drawing object id is required"))
}
fn no_entry(s: &Session, cmd: &str) -> Result<()> {
    if s.active_sheet_sketch.is_some() {
        return Err(bad(cmd, "finish or cancel the sheet sketch before entering another"));
    }
    Ok(())
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.sketch.create";
    let p: contract::Create = parsed(cmd, p)?;
    no_entry(s, cmd)?;
    let owner = s.drawing(cmd)?.clone();
    owner.validate().map_err(|e| bad(cmd, e.to_string()))?;
    // Ids are unique in a drawing, so the id says which kind of owner it is.
    let given = id(cmd, &p.owner)?;
    let place = if owner.sheets.iter().any(|record| record.id == given) {
        Owner::Sheet(given)
    } else if owner.view(given).is_some() {
        Owner::View(given)
    } else {
        return Err(bad(cmd, "the open drawing has no such sheet or view"));
    };
    let name = p.name.unwrap_or_else(|| "Sheet sketch".into());
    // Any object id held by drawing undo/redo must not name a new sketch either.
    let taken: std::collections::BTreeSet<u64> = std::iter::once(owner.as_ref())
        .chain(s.undo.iter().chain(&s.redo).filter_map(|step| step.doc.drawing(owner.id).map(AsRef::as_ref)))
        .flat_map(|drawing| drawing.object_ids())
        .collect();
    let sketch = s.ids.next(&taken).map_err(|e| bad(cmd, e.to_string()))?;
    s.edit_drawing(cmd, |drawing| {
        drawing.sketches.push(Arc::new(SheetSketch { id: sketch, owner: place, name, sketch: Sketch::new() }));
        Ok(())
    })?;
    s.active_sheet_sketch = Some(Arc::new(SheetSketchEdit { drawing: owner.id, owner: place, sketch, original: None }));
    s.selection.clear();
    reading(s, sketch, cmd)
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.sketch.edit";
    let p: contract::Edit = parsed(cmd, p)?;
    let sketch = id(cmd, &p.sketch)?;
    let owner = s.drawing(cmd)?.clone();
    if let Some(entry) = &s.active_sheet_sketch {
        if entry.drawing == owner.id && entry.sketch == sketch {
            return reading(s, sketch, cmd); // keep the original cancel boundary
        }
        return Err(bad(cmd, "finish or cancel the sheet sketch before entering another"));
    }
    // Solving first refuses a sketch that can't be edited, before the entry is made.
    s.doc.solve_sheet_sketch(owner.id, sketch)?;
    let original = owner.sheet_sketch(sketch).cloned().ok_or_else(|| bad(cmd, "the open drawing has no such sketch"))?;
    s.active_sheet_sketch = Some(Arc::new(SheetSketchEdit { drawing: owner.id, owner: original.owner, sketch, original: Some(original) }));
    s.selection.clear();
    s.revision += 1;
    reading(s, sketch, cmd)
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.sketch.inspect";
    let p: contract::Inspect = parsed(cmd, p)?;
    let sketch = match p.sketch {
        Some(text) => id(cmd, &text)?,
        None => sketch_store::sheet_target(s, &Value::Null, cmd)?.sketch,
    };
    reading(s, sketch, cmd)
}

fn reading(s: &Session, sketch: u64, cmd: &str) -> Result<Value> {
    let owner = s.drawing(cmd)?;
    let solved = s.doc.solve_sheet_sketch(owner.id, sketch)?;
    let record = owner.sheet_sketch(sketch).ok_or_else(|| bad(cmd, "the open drawing has no such sketch"))?;
    let reading = contract::Reading {
        drawing: ids::encode(owner.id),
        sheet: ids::encode(solved.sheet),
        view: record.owner.view().map(ids::encode),
        sketch: ids::encode(sketch),
        name: record.name.clone(),
        editing: s
            .active_sheet_sketch
            .as_ref()
            .is_some_and(|active| active.drawing == owner.id && active.owner == record.owner && active.sketch == sketch),
        solved: solved.report.keeps_orientation(),
        dof: solved.report.dof,
        points: solved
            .sketch
            .points
            .iter()
            .map(|point| contract::Point { id: point.id.clone(), at: [point.pos.x, point.pos.y], fixed: point.fixed })
            .collect(),
        curves: solved.sketch.curves.iter().map(|curve| curve.id.clone()).collect(),
        constraints: solved.sketch.constraints.iter().map(|constraint| constraint.id.clone()).collect(),
        links: link_states(s, owner, record)
            .into_iter()
            .map(|state| {
                let (status, why, candidates) = super::drawing_dimension::status_words(state.status);
                contract::LinkReading { link: state.link, status, why, candidates }
            })
            .collect(),
    };
    serde_json::to_value(reading).map_err(|e| bad(cmd, e.to_string()))
}

/// Why a sketch on `view` can't link to the model's edges, if it can't: its kind draws none
/// ([`ViewKind::no_model_edges`], the record's rule), or it is a section view, whose cut edges aren't
/// read yet.
fn no_links(view: &View) -> Option<&'static str> {
    let section = matches!(view.kind, ViewKind::Section { .. }).then_some("not supported yet: a link to a section view's edges");
    view.kind.no_model_edges().or(section)
}

/// The session view `view` of `d` draws from ([`ViewSessions::of`]): where its sketch's links read
/// their edges, so each follows its edge where the view draws it (an exploded, posed or configuration
/// view on its own copy, as its dimensions do). Or why they can't be read there: [`no_links`], a view
/// that doesn't resolve, a source that isn't this design ([`Session::drawn`]), or a copy that can't be
/// made.
pub(super) fn link_source<'a>(s: &Session, sessions: &'a mut ViewSessions<'_>, d: &Drawing, view: u64) -> std::result::Result<&'a Session, String> {
    let record = d.view(view).ok_or_else(|| format!("the drawing has no view {}", ids::encode(view)))?;
    if let Some(why) = no_links(record) {
        return Err(why.into());
    }
    let resolved = d.resolve_view(view).map_err(|e| e.to_string())?;
    s.drawn(d, resolved.source).map_err(|why| why.to_string())?;
    sessions.of(&resolved).map_err(|why| why.to_string())
}

/// Each link of `record`, a sketch of `d`, as the model has its edge now ([`link_source`]); none for a
/// sheet's sketch. Read only: nothing is kept.
fn link_states(s: &Session, d: &Drawing, record: &SheetSketch) -> Vec<ViewLinkState> {
    let Some(view) = record.owner.view().filter(|_| !record.sketch.links.is_empty()) else { return Vec::new() };
    let mut sketch = record.sketch.clone();
    let mut sessions = ViewSessions::new(s);
    let read = link_source(s, &mut sessions, d, view)
        .and_then(|from| from.doc.refresh_view_links(&from.model, d, view, &mut sketch).map_err(|e| e.to_string()));
    read.unwrap_or_else(|why| {
        let state =
            |link: &solidcraft_sketch::Link| ViewLinkState { link: link.id.clone(), status: Status::Unmeasurable(why.clone()), warning: None };
        record.sketch.links.iter().map(state).collect()
    })
}

/// The drawings of `s` whose view sketches' links follow a change from `before`: each link moved to its
/// edge where its view draws it now ([`link_source`]) and its sketch solved again from where it was, or
/// marked lost and kept where it was when its edge can't be read. A model change (`model`) reaches every
/// drawing's view sketches; otherwise only those whose record or view changed. Nothing is read for a
/// sketch with no links. The caller puts the drawings into the design in the same step: a change is
/// never a step of its own (`Session::refresh_sheet_fill_sources` does the same for fills). A sketch
/// that its links' new places leave unsolvable keeps its last solved state, and its next edit says why.
pub(crate) fn follow_view_links(s: &Session, before: &Document, model: bool) -> Vec<Arc<Drawing>> {
    let mut changed = Vec::new();
    for d in &s.doc.drawings {
        // Without a model change, a drawing as it was, and a sketch whose record and view are, read the same.
        let old = before.drawing(d.id).filter(|_| !model);
        if old.is_some_and(|old| Arc::ptr_eq(old, d)) {
            continue;
        }
        let mut sessions = ViewSessions::new(s);
        let mut next: Option<Drawing> = None;
        for record in &d.sketches {
            let Some(view) = record.owner.view().filter(|_| !record.sketch.links.is_empty()) else { continue };
            let as_it_was = |old: &Arc<Drawing>| {
                old.sheet_sketch(record.id).is_some_and(|was| Arc::ptr_eq(was, record)) && old.resolve_view(view).ok() == d.resolve_view(view).ok()
            };
            if old.is_some_and(as_it_was) {
                continue;
            }
            let mut moved = record.sketch.clone();
            match link_source(s, &mut sessions, d, view) {
                Ok(from) => {
                    // Only links that moved or were lost cost a solve.
                    if from.doc.refresh_view_links(&from.model, d, view, &mut moved).is_ok() && moved != record.sketch {
                        let target = next.get_or_insert_with(|| (**d).clone());
                        target.edit_sheet_sketch(&from.doc, Some(&from.model), record.id, |_| Ok(())).ok();
                    }
                }
                Err(_) => {
                    for link in &record.sketch.links {
                        moved.set_link_lost(&link.id, true);
                    }
                    if moved != record.sketch
                        && let Some(slot) = next.get_or_insert_with(|| (**d).clone()).sketches.iter_mut().find(|slot| slot.id == record.id)
                    {
                        Arc::make_mut(slot).sketch = moved;
                    }
                }
            }
        }
        if let Some(next) = next.filter(|next| next != &**d) {
            changed.push(Arc::new(next));
        }
    }
    changed
}

pub(super) fn finish(s: &mut Session, p: &Value) -> Result<Value> {
    let _: contract::Empty = parsed("sketch.finish", p)?;
    let entry = sketch_store::sheet_target(s, &Value::Null, "sketch.finish")?;
    let mut result = reading(s, entry.sketch, "sketch.finish")?;
    s.active_sheet_sketch = None;
    s.selection.clear();
    s.revision += 1;
    if let Some(object) = result.as_object_mut() {
        object.insert("editing".into(), Value::Bool(false));
    }
    Ok(result)
}

pub(super) fn cancel(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.cancel";
    let _: contract::Empty = parsed(cmd, p)?;
    let entry = sketch_store::sheet_target(s, &Value::Null, cmd)?;
    let removed = entry.original.is_none();
    // The sheet it is on, read before a new sketch is taken out again.
    let open = s.drawing(cmd)?;
    let sheet = open
        .sheet_sketch(entry.sketch)
        .and_then(|record| open.sketch_sheet(record))
        .ok_or_else(|| bad(cmd, "the edited sheet sketch no longer exists"))?;
    s.edit_drawing(cmd, |drawing| {
        let at = drawing
            .sketches
            .iter()
            .position(|record| record.id == entry.sketch)
            .ok_or_else(|| bad(cmd, "the edited sheet sketch no longer exists"))?;
        match &entry.original {
            Some(original) => {
                let slot = drawing.sketches.get_mut(at).ok_or_else(|| bad(cmd, "the sheet sketch changed while cancelling"))?;
                *slot = original.clone();
            }
            None => {
                drawing.sketches.remove(at);
            }
        }
        Ok(())
    })?;
    s.active_sheet_sketch = None;
    s.selection.clear();
    s.revision += 1;
    serde_json::to_value(contract::Cancelled {
        drawing: ids::encode(entry.drawing),
        sheet: ids::encode(sheet),
        view: entry.owner.view().map(ids::encode),
        sketch: ids::encode(entry.sketch),
        removed,
    })
    .map_err(|e| bad(cmd, e.to_string()))
}
