//! Commands for associative manual centre lines.

use super::CommandSpec;
use super::drawing_dimension::status_words;
use super::drawing_view::{centre_warning, check_drawn};
use crate::params::bad;
use crate::schema::drawing_centre_line::{AddParams, ChangeResult, EditParams, LineParams, LineResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};
use serde_json::Value;
use solidcraft_doc::drawing::centre_line::CentreLine;
use solidcraft_doc::drawing::dimension::Status;
use solidcraft_doc::ids;
use std::sync::Arc;

const ADD: &str = "drawing.center_line.add";
const EDIT: &str = "drawing.center_line.edit";
const INSPECT: &str = "drawing.center_line.inspect";
const REMOVE: &str = "drawing.center_line.remove";
const ADD_CONTRACT: TypedContract<AddParams> = TypedContract::new();
const EDIT_CONTRACT: TypedContract<EditParams> = TypedContract::new();
const LINE_CONTRACT: TypedContract<LineParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new(ADD, "Add Center Line", crate::checked!("drawing.center_line.add", add))
        .icon("new")
        .in_drawing()
        .doc_only()
        .contract(&ADD_CONTRACT)
        .output(output_schema_for::<LineResult>)
        .params("view: saved view id; a and b: model point picks; overshoot_mm: extension on paper at each end. The view must draw both parts and show the assembled design. One undo step."),
    CommandSpec::new(EDIT, "Edit Center Line", crate::checked!("drawing.center_line.edit", edit))
        .icon("edit")
        .in_drawing()
        .doc_only()
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<ChangeResult>)
        .params("line: id; overshoot_mm: extension on paper at each end. One undo step if changed."),
    CommandSpec::new(INSPECT, "Inspect Center Line", crate::checked!("drawing.center_line.inspect", inspect))
        .icon("inspect")
        .in_drawing()
        .noundo()
        .doc_only()
        .contract(&LINE_CONTRACT)
        .output(output_schema_for::<LineResult>)
        .params("line: id → saved record and current endpoints or reference status, with a warning if its view no longer draws a part or draws another configuration or position."),
    CommandSpec::new(REMOVE, "Remove Center Line", crate::checked!("drawing.center_line.remove", remove))
        .icon("delete")
        .in_drawing()
        .doc_only()
        .contract(&LINE_CONTRACT)
        .output(output_schema_for::<ChangeResult>)
        .params("line: id → removed line. One undo step."),
];

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let request: AddParams = crate::schema::read(ADD, p)?;
    let record = s.drawing(ADD)?.clone();
    let view = request.view.0;
    record.view(view).ok_or_else(|| bad(ADD, format!("the drawing has no view {}", ids::encode(view))))?;
    crate::drawing_view::check_centre_view(&record, view, "centre lines", ADD)?;
    let resolve = |pick: &crate::schema::drawing_dimension::CornerParam| {
        s.doc.corner_reference(&s.model, &pick.pick, &pick.path).map_err(|e| bad(ADD, e.to_string()))
    };
    let (a, b) = (resolve(&request.a)?, resolve(&request.b)?);
    check_drawn(&record, view, [a.path.as_slice(), b.path.as_slice()], ADD)?;
    let id = record.allocate_id(&mut s.ids).map_err(|e| bad(ADD, e.to_string()))?;
    let line = CentreLine { id, view, a, b, overshoot_mm: request.overshoot_mm };
    let mut probe = (*record).clone();
    probe.add_centre_lines([Arc::new(line.clone())]);
    probe.validate().map_err(|e| bad(ADD, e.to_string()))?;
    let result = s
        .doc
        .resolve_centre_lines(&s.model, &probe, &[id])
        .map_err(|e| bad(ADD, e.to_string()))?
        .into_iter()
        .next()
        .ok_or_else(|| bad(ADD, "the centre line could not be resolved"))?;
    if !matches!(&result.status, Status::Ok) {
        let (status, why, _) = status_words(result.status);
        return Err(bad(ADD, why.unwrap_or_else(|| format!("the reference is {status}"))));
    }
    s.edit_drawing(ADD, |d| {
        d.add_centre_lines([Arc::new(line)]);
        Ok(())
    })?;
    answer(s, ADD, id)
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let request: EditParams = crate::schema::read(EDIT, p)?;
    let id = request.line.0;
    let changed = s.edit_drawing(EDIT, |d| {
        let index = d.centre_line_index(id).ok_or_else(|| bad(EDIT, format!("no centre line {}", ids::encode(id))))?;
        let line = d.centre_lines.get_mut(index).map(Arc::make_mut).ok_or_else(|| bad(EDIT, "centre line no longer exists"))?;
        if line.overshoot_mm == request.overshoot_mm {
            return Ok(Vec::new());
        }
        line.overshoot_mm = request.overshoot_mm;
        Ok(vec!["overshoot_mm".into()])
    })?;
    change_reply(id, changed)
}

fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    let id = crate::schema::read::<LineParams>(REMOVE, p)?.line.0;
    s.edit_drawing(REMOVE, |d| {
        let index = d.centre_line_index(id).ok_or_else(|| bad(REMOVE, format!("no centre line {}", ids::encode(id))))?;
        d.centre_lines.remove(index);
        Ok(())
    })?;
    change_reply(id, vec!["removed".into()])
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let id = crate::schema::read::<LineParams>(INSPECT, p)?.line.0;
    answer(s, INSPECT, id)
}

fn change_reply(id: u64, changed: Vec<String>) -> Result<Value> {
    serde_json::to_value(ChangeResult { line: ids::encode(id), changed, param_warnings: Vec::new() }).map_err(EngineError::from)
}

fn answer(s: &Session, cmd: &str, id: u64) -> Result<Value> {
    let d = s.drawing(cmd)?;
    let line = d.centre_line(id).ok_or_else(|| bad(cmd, format!("no centre line {}", ids::encode(id))))?;
    let r = s
        .doc
        .resolve_centre_lines(&s.model, d, &[id])
        .map_err(|e| bad(cmd, e.to_string()))?
        .into_iter()
        .next()
        .ok_or_else(|| bad(cmd, "the centre line could not be resolved"))?;
    let (status, why, candidates) = status_words(r.status);
    let reply = LineResult {
        line: ids::encode(id),
        view: ids::encode(line.view),
        record: serde_json::to_value(line).map_err(EngineError::from)?,
        status: status.into(),
        why,
        points: r.points.map(|points| points.map(|p| [p.x, p.y, p.z])),
        exact: r.exact,
        candidates,
        warnings: centre_warning(d, line.view, [line.a.path.as_slice(), line.b.path.as_slice()]),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(EngineError::from)
}
