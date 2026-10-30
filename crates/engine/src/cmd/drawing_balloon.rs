//! `drawing.balloon.add`, `.edit` and `.inspect` (W26, #3874): a balloon on a saved view of the
//! open drawing, pointing at a placed part by an edge of it, and where it stands with the model
//! now. The edge is picked as a dimension's is and saved by name
//! ([`solidcraft_doc::Document::edge_reference`]); its path is the part the balloon numbers.
//! Adding and editing are one undo step of the drawing each, and none of the commands evaluates
//! the model.

use std::sync::Arc;

use serde_json::Value;
use solidcraft_doc::drawing::balloon::Balloon;
use solidcraft_doc::ids;

use super::CommandSpec;
use super::drawing_dimension::status_words;
use crate::params::bad;
use crate::schema::drawing_balloon::{AddParams, BalloonResult, EditParams, EditResult, InspectParams};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const ADD: &str = "drawing.balloon.add";
const ADD_CONTRACT: TypedContract<AddParams> = TypedContract::new();
const EDIT: &str = "drawing.balloon.edit";
const EDIT_CONTRACT: TypedContract<EditParams> = TypedContract::new();
const INSPECT: &str = "drawing.balloon.inspect";
const INSPECT_CONTRACT: TypedContract<InspectParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new(ADD, "Add Balloon", crate::checked!("drawing.balloon.add", add))
        .icon("new")
        .in_drawing()
        .doc_only()
        .contract(&ADD_CONTRACT)
        .output(output_schema_for::<BalloonResult>)
        .params(
            "view: a saved view's id; edge: the edge its arrow points at, a point on it [x, y, z] in the design's frame, {point} or \
             {body, index}, with path: the occurrences its body is placed through, from the top (the part the balloon numbers; at \
             least one); at: [x, y], the balloon's centre on paper in mm from the view's centre → what drawing.balloon.inspect \
             answers for it. Saved by the edge's name, never the point picked. One undo step; no model is evaluated.",
        ),
    CommandSpec::new(EDIT, "Edit Balloon", crate::checked!("drawing.balloon.edit", edit))
        .icon("edit")
        .in_drawing()
        .doc_only()
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<EditResult>)
        .params(
            "balloon: its id; set: {at?: [x, y]} → {balloon, changed: [fields]}. What it points at stays: to number another part, add \
             a new one. One undo step, none when nothing changes; no model is evaluated.",
        ),
    CommandSpec::new(INSPECT, "Inspect Balloon", crate::checked!("drawing.balloon.inspect", inspect))
        .icon("inspect")
        .in_drawing()
        .noundo()
        .doc_only()
        .contract(&INSPECT_CONTRACT)
        .output(output_schema_for::<BalloonResult>)
        .params(
            "balloon: its id → {balloon, view, record: as saved, occurrence: the part it numbers, status: ok, ambiguous, suppressed or \
             lost, why?, point?: where its arrow ends (its edge's middle, in the design's frame), exact, candidates?: [{edge: {body, \
             index}}]}. Read-only; no model is evaluated.",
        ),
];

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let request: AddParams = crate::schema::read(ADD, p)?;
    let record = s.drawing(ADD)?.clone();
    let view = request.view.0;
    record.view(view).ok_or_else(|| bad(ADD, format!("the drawing has no view {}", ids::encode(view))))?;
    let edge = s.doc.edge_reference(&s.model, &request.edge.pick, &request.edge.path).map_err(|why| bad(ADD, why.to_string()))?;
    let id = record.allocate_id(&mut s.ids).map_err(|e| bad(ADD, e.to_string()))?;
    let balloon = Balloon { id, view, edge, at: request.at };
    s.edit_drawing(ADD, |d| {
        d.balloons.add([Arc::new(balloon)]);
        Ok(())
    })?;
    answer(s, ADD, id)
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let request: EditParams = crate::schema::read(EDIT, p)?;
    let id = request.balloon.0;
    let changed = s.edit_drawing(EDIT, |d| {
        let at = d.balloons.index_of(id).ok_or_else(|| bad(EDIT, format!("the drawing has no balloon {}", ids::encode(id))))?;
        let Some(balloon) = d.balloons.get_mut(at).map(Arc::make_mut) else { return Ok(Vec::new()) };
        let moved = request.set.at.filter(|place| *place != balloon.at);
        if let Some(place) = moved {
            balloon.at = place;
        }
        Ok(moved.map(|_| vec!["at".to_string()]).unwrap_or_default())
    })?;
    let reply = EditResult { balloon: ids::encode(id), changed, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let request: InspectParams = crate::schema::read(INSPECT, p)?;
    answer(s, INSPECT, request.balloon.0)
}

/// Balloon `id` of the open drawing as the model has it now.
fn answer(s: &Session, cmd: &str, id: u64) -> Result<Value> {
    let d = s.drawing(cmd)?;
    let balloon = d.balloons.by_id(id).ok_or_else(|| bad(cmd, format!("the drawing has no balloon {}", ids::encode(id))))?;
    let resolved = s.doc.resolve_balloons(&s.model, d, &[id]).map_err(|e| bad(cmd, e.to_string()))?;
    let r = resolved.into_iter().next().ok_or_else(|| bad(cmd, format!("balloon {} wasn't read", ids::encode(id))))?;
    let (status, why, candidates) = status_words(r.status);
    let reply = BalloonResult {
        balloon: ids::encode(id),
        view: ids::encode(balloon.view),
        record: serde_json::to_value(balloon).map_err(|e| EngineError::Other(e.to_string()))?,
        occurrence: balloon.occurrence().to_vec(),
        status: status.to_string(),
        why,
        point: r.point.map(|p| [p.x, p.y, p.z]),
        exact: r.exact,
        candidates,
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

#[cfg(test)]
#[path = "drawing_balloon_tests.rs"]
mod tests;
