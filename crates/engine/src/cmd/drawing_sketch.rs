//! Sheet-owned sketch lifecycle on the embedded drawing receiver (W38a).
//! Geometry tools reuse cmd::sketch and cmd::sketch_store; there is no second sketcher.
use super::{CommandSpec, sketch_store};
use crate::params::bad;
use crate::schema::{drawing_sketch as contract, output_schema_for};
use crate::{Result, Session, SheetSketchEdit};
use serde::de::DeserializeOwned;
use serde_json::Value;
use solidcraft_doc::drawing::SheetSketch;
use solidcraft_doc::ids::{self, IdSource};
use solidcraft_sketch::Sketch;
use std::sync::Arc;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("drawing.sketch.create", "Create Sheet Sketch", crate::checked!("drawing.sketch.create", contract::CREATE, create))
        .in_drawing().doc_only().icon("sketch")
        .contract(&contract::CREATE).output(output_schema_for::<contract::Reading>)
        .params("owner: exact sheet id in the open drawing; name?: text. Creates and enters one sheet sketch in paper XY millimetres; one drawing undo step."),
    CommandSpec::new("drawing.sketch.edit", "Edit Sheet Sketch", crate::checked!("drawing.sketch.edit", contract::EDIT, edit))
        .in_drawing().doc_only().noundo().icon("sketch")
        .contract(&contract::EDIT).output(output_schema_for::<contract::Reading>)
        .params("sketch: exact encoded sketch id in the open drawing. Enters that sheet sketch without changing its data or the source design."),
    CommandSpec::new("drawing.sketch.inspect", "Inspect Sheet Sketch", crate::checked!("drawing.sketch.inspect", contract::INSPECT, inspect))
        .in_drawing().doc_only().noundo().icon("inspect")
        .contract(&contract::INSPECT).output(output_schema_for::<contract::Reading>)
        .params("sketch?: exact encoded id, else the active sheet sketch. Reports resolved paper-mm points and local entity ids without adopting the solution."),

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
    let sheet = id(cmd, &p.owner)?;
    if !owner.sheets.iter().any(|record| record.id == sheet) {
        return Err(bad(cmd, "the open drawing has no such sheet"));
    }
    let name = p.name.unwrap_or_else(|| "Sheet sketch".into());
    // Any object id held by drawing undo/redo must not name a new sketch either.
    let retired: std::collections::BTreeSet<u64> = s
        .undo
        .iter()
        .chain(&s.redo)
        .filter_map(|step| step.doc.drawing(owner.id))
        .flat_map(|drawing| {
            std::iter::once(drawing.id)
                .chain(drawing.sources.iter().map(|record| record.id))
                .chain(drawing.sheets.iter().map(|record| record.id))
                .chain(drawing.sketches.iter().map(|record| record.id))
        })
        .collect();
    let mut source = IdSource::from_entropy();
    let mut fresh = None;
    for _ in 0..64 {
        let next = owner.allocate_id(&mut source)?;
        if !retired.contains(&next) {
            fresh = Some(next);
            break;
        }
    }
    let sketch = fresh.ok_or_else(|| bad(cmd, "could not allocate an unused sheet sketch id"))?;
    s.edit_drawing(cmd, |drawing| {
        drawing.sketches.push(Arc::new(SheetSketch { id: sketch, sheet, name, sketch: Sketch::new() }));
        Ok(())
    })?;
    s.active_sheet_sketch = Some(Arc::new(SheetSketchEdit { drawing: owner.id, sheet, sketch, original: None }));
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
    let solved = s.doc.solve_sheet_sketch(owner.id, sketch)?;
    let original = owner.sheet_sketch(sketch).cloned().ok_or_else(|| bad(cmd, "the open drawing has no such sketch"))?;
    s.active_sheet_sketch = Some(Arc::new(SheetSketchEdit { drawing: owner.id, sheet: solved.sheet, sketch, original: Some(original) }));
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
        sketch: ids::encode(sketch),
        name: record.name.clone(),
        editing: s
            .active_sheet_sketch
            .as_ref()
            .is_some_and(|active| active.drawing == owner.id && active.sheet == solved.sheet && active.sketch == sketch),
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
        sheet: ids::encode(entry.sheet),
        sketch: ids::encode(entry.sketch),
        removed,
    })
    .map_err(|e| bad(cmd, e.to_string()))
}
