//! Sheet-owned sketch lifecycle on the embedded drawing receiver (W38a).
//! Geometry tools reuse cmd::sketch and cmd::sketch_store; there is no second sketcher.
use super::{CommandSpec, sketch_store};
use crate::params::bad;
use crate::schema::{drawing_sketch as contract, output_schema_for};
use crate::{Result, Session, SheetSketchEdit};
use serde::de::DeserializeOwned;
use serde_json::Value;
use solidcraft_doc::drawing::{Owner, SheetSketch};
use solidcraft_doc::ids;
use solidcraft_sketch::Sketch;
use std::sync::Arc;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("drawing.sketch.create", "Create Sheet Sketch", crate::checked!("drawing.sketch.create", create)).begins_edit()
        .in_drawing().doc_only().icon("sketch")
        .contract(&contract::CREATE).output(output_schema_for::<contract::Reading>)
        .params("owner: exact sheet or drawing-view id in the open drawing; name?: text; view?: open Sheet tile displaying the owner's sheet. Hosted calls default to the focused or most recent matching Sheet tile and return sketching_view; headless calls omit view and have no tile owner. Creates and enters one sketch: a sheet's in paper XY millimetres, a drawing view's in model millimetres in its plane; one drawing undo step."),
    CommandSpec::new("drawing.sketch.edit", "Edit Sheet Sketch", crate::checked!("drawing.sketch.edit", edit)).begins_edit()
        .in_drawing().doc_only().noundo().icon("sketch")
        .contract(&contract::EDIT).output(output_schema_for::<contract::Reading>)
        .params("sketch: exact encoded sketch id in the open drawing; view?: matching open Sheet tile (headless calls omit it). Returns sketching_view for hosted edits, distinct from the encoded drawing view in the reply's view field. Enters that sketch (a sheet's in paper XY millimetres, a drawing view's in model millimetres in its plane) without changing its data or the source design."),
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
    let sheet = match place {
        Owner::Sheet(sheet) => sheet,
        Owner::View(view) => owner.view(view).map(|record| record.sheet).ok_or_else(|| bad(cmd, "the drawing view's sheet no longer exists"))?,
    };
    let view = s.requested_sheet_view(owner.id, sheet, p.view, cmd)?;
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
    s.active_sheet_sketch = Some(Arc::new(SheetSketchEdit { drawing: owner.id, owner: place, view, sketch, original: None }));
    s.selection.clear();
    reading(s, sketch, cmd)
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.sketch.edit";
    let p: contract::Edit = parsed(cmd, p)?;
    let sketch = id(cmd, &p.sketch)?;
    let owner = s.drawing(cmd)?.clone();
    let original = owner.sheet_sketch(sketch).cloned().ok_or_else(|| bad(cmd, "the open drawing has no such sketch"))?;
    let sheet = owner.sketch_sheet(&original).ok_or_else(|| bad(cmd, "the sketch's sheet no longer exists"))?;
    let requested = p
        .view
        .or_else(|| s.active_sheet_sketch.as_ref().filter(|entry| entry.drawing == owner.id && entry.sketch == sketch).and_then(|entry| entry.view));
    let view = s.requested_sheet_view(owner.id, sheet, requested, cmd)?;
    if let Some(entry) = &s.active_sheet_sketch {
        if entry.drawing == owner.id && entry.sketch == sketch && entry.view == view {
            return reading(s, sketch, cmd); // keep the original cancel boundary
        }
        return Err(bad(cmd, "finish or cancel the sheet sketch before entering another"));
    }
    // Solving first refuses a sketch that can't be edited, before the entry is made.
    s.doc.solve_sheet_sketch(owner.id, sketch)?;
    s.active_sheet_sketch = Some(Arc::new(SheetSketchEdit { drawing: owner.id, owner: original.owner, view, sketch, original: Some(original) }));
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
        sketching_view: s
            .active_sheet_sketch
            .as_ref()
            .filter(|entry| entry.drawing == owner.id && entry.sketch == sketch)
            .and_then(|entry| entry.view),
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
    };
    serde_json::to_value(reading).map_err(|e| bad(cmd, e.to_string()))
}

pub(super) fn finish(s: &mut Session, p: &Value) -> Result<Value> {
    let _: contract::Empty = parsed("sketch.finish", p)?;
    let entry = sketch_store::sheet_target(s, &Value::Null, "sketch.finish")?;
    let mut result = reading(s, entry.sketch, "sketch.finish")?;
    if s.doc
        .drawing(entry.drawing)
        .and_then(|drawing| drawing.sheet_sketch(entry.sketch))
        .is_some_and(|record| crate::edit_start::empty_geometry(&record.sketch))
    {
        s.edit_drawing("sketch.finish", |drawing| {
            drawing.sketches.retain(|record| record.id != entry.sketch);
            Ok(())
        })?;
    }
    s.active_sheet_sketch = None;
    s.selection.clear();
    s.revision += 1;
    if let Some(object) = result.as_object_mut() {
        object.insert("editing".into(), Value::Bool(false));
        object.remove("sketching_view");
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
