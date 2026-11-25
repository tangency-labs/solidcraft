//! `drawing.balloon.add`, `.edit`, `.delete`, `.repair` and `.inspect` (W26, #3874; B5, #4892): a balloon on a
//! saved view of the open drawing, pointing at a placed part by an edge of it, a new name for the
//! edge when it is lost, and where it stands with the model now. The edge is picked as a
//! dimension's is and saved by name ([`solidcraft_doc::Document::edge_reference`]); its path is the
//! part the balloon numbers, and a repair keeps it. Adding, editing and repairing are one undo step
//! of the drawing each, and none of the commands evaluates the model.
//!
//! The number it shows is its row's item in the parts list it reads
//! ([`solidcraft_doc::drawing::Drawing::parts_list_for`]), worked out on each read and never saved,
//! by the one rule the sheet draws it with ([`balloon_rows`]).

use std::sync::Arc;

use serde_json::Value;
use solidcraft_doc::bom::Unlisted;
use solidcraft_doc::drawing::OnSheet;
use solidcraft_doc::drawing::balloon::{Balloon, RowRead, balloon_rows};
use solidcraft_doc::ids;

use super::CommandSpec;
use super::drawing_dimension::{Slot, repair_reference, status_words};
use super::drawing_view::{check_drawn, drawn_warning};
use crate::params::bad;
use crate::schema::drawing_balloon::{AddParams, BalloonResult, DeleteParams, DeleteResult, EditParams, EditResult, InspectParams, RepairParams};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const ADD: &str = "drawing.balloon.add";
const ADD_CONTRACT: TypedContract<AddParams> = TypedContract::new();
const DELETE: &str = "drawing.balloon.delete";
const DELETE_CONTRACT: TypedContract<DeleteParams> = TypedContract::new();
const EDIT: &str = "drawing.balloon.edit";
const EDIT_CONTRACT: TypedContract<EditParams> = TypedContract::new();
const INSPECT: &str = "drawing.balloon.inspect";
const INSPECT_CONTRACT: TypedContract<InspectParams> = TypedContract::new();
const REPAIR: &str = "drawing.balloon.repair";
const REPAIR_CONTRACT: TypedContract<RepairParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new(ADD, "Add Balloon", crate::checked!("drawing.balloon.add", add)).begins_edit()
        .icon("new")
        .in_drawing()
        .doc_only()
        .contract(&ADD_CONTRACT)
        .output(output_schema_for::<BalloonResult>)
        .params(
            "view: a saved view's id; edge: the edge its arrow points at, a point on it [x, y, z] in the design's frame, {point} or \
             {body, index}, and `path` INSIDE it (not a top-level field), the occurrences its body is placed through, from the top (the part the \
             balloon numbers; at least one; a design of one part has none, so place it as a component first), e.g. \
             edge: {point: [40, 0, 0], path: [3]}; at: [x, y], the balloon's centre on paper in mm from the view's centre; parts_list?: the table it reads \
             (default its sheet's only parts list, else the drawing's) → what drawing.balloon.inspect \
             answers for it. Saved by the edge's name, never the point picked; one on a part its view doesn't draw (its hide or \n             source leaves it out) is refused. One undo step; no model is evaluated.",
        ),
    CommandSpec::new(DELETE, "Delete Balloon", crate::checked!("drawing.balloon.delete", delete)).begins_edit()
        .icon("delete")
        .in_drawing()
        .doc_only()
        .contract(&DELETE_CONTRACT)
        .output(output_schema_for::<DeleteResult>)
        .params("balloon: its id → {balloon}. The part it numbered keeps its row in the parts list. One undo step; no model is evaluated."),
    CommandSpec::new(EDIT, "Edit Balloon", crate::checked!("drawing.balloon.edit", edit)).begins_edit_when(crate::edit_start::changes_editor_fields)
        .icon("edit")
        .in_drawing()
        .doc_only()
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<EditResult>)
        .params(
            "balloon: its id; set: {at?: [x, y], parts_list?: a table's id, or null to read the only list} → {balloon, changed: [fields]}. What it points at stays: to number another part, add \
             a new one. One undo step, none when nothing changes; no model is evaluated.",
        ),
    CommandSpec::new(REPAIR, "Repair Balloon", crate::checked!("drawing.balloon.repair", repair)).begins_edit()
        .icon("edit")
        .in_drawing()
        .doc_only()
        .contract(&REPAIR_CONTRACT)
        .output(output_schema_for::<BalloonResult>)
        .params(
            "balloon: its id, one that reads lost or ambiguous; pick: {edge}, the edge it points at now, as add takes it but without \
             path (it keeps its own, the part it numbers); an edge candidate inspect lists passes back as it stands → what \
             drawing.balloon.inspect answers for it. Saved by the edge's name; its id, place and parts list stay. One undo step; no \
             model is evaluated.",
        ),
    CommandSpec::new(INSPECT, "Inspect Balloon", crate::checked!("drawing.balloon.inspect", inspect))
        .icon("inspect")
        .in_drawing()
        .noundo()
        .doc_only()
        .contract(&INSPECT_CONTRACT)
        .output(output_schema_for::<BalloonResult>)
        .params(
            "balloon: its id → {balloon, view, record: as saved, occurrence: the part it numbers, status: ok, ambiguous, suppressed, \
             failed or lost, why?, point?: where its arrow ends (its edge's middle, in the design's frame), exact, candidates?: [{edge: \
             {body, index}}] what an ambiguous edge's name finds, or a lost one's leads ({face: {body, index}} too), parts_list?: the \
             list it reads, choices?: with none to go by, those it could name, item?: its row's item (\"1.2\"), \
             row_status: ok, no_list, reference, no_row, not_found or unread (its list's design can't be read), warnings?: such as its view edited since to leave its part out}. A lost row never removes it. Read-only; no model is \
             evaluated.",
        ),
];

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let request: AddParams = crate::schema::read(ADD, p)?;
    let record = s.drawing(ADD)?.clone();
    let view = request.view.0;
    record.view(view).ok_or_else(|| bad(ADD, format!("the drawing has no view {}", ids::encode(view))))?;
    let edge = s.doc.edge_reference(&s.model, &request.edge.pick, &request.edge.path).map_err(|why| bad(ADD, why.to_string()))?;
    check_drawn(&record, view, [edge.path.as_slice()], ADD)?;
    let id = record.allocate_id(&mut s.ids).map_err(|e| bad(ADD, e.to_string()))?;
    let balloon = Balloon { id, view, edge, at: request.at, parts_list: request.parts_list.map(|list| list.0) };
    s.edit_drawing(ADD, |d| {
        d.balloons.add([Arc::new(balloon)]);
        Ok(())
    })?;
    answer(s, ADD, id)
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let request: DeleteParams = crate::schema::read(DELETE, p)?;
    let id = request.balloon.0;
    s.edit_drawing(DELETE, |d| {
        d.balloons.by_id(id).ok_or_else(|| bad(DELETE, format!("the drawing has no balloon {}", ids::encode(id))))?;
        d.remove_objects(&OnSheet { balloons: vec![id], ..Default::default() });
        Ok(())
    })?;
    serde_json::to_value(DeleteResult { balloon: ids::encode(id) }).map_err(|e| EngineError::Other(e.to_string()))
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let request: EditParams = crate::schema::read(EDIT, p)?;
    let id = request.balloon.0;
    let changed = s.edit_drawing(EDIT, |d| {
        let at = d.balloons.index_of(id).ok_or_else(|| bad(EDIT, format!("the drawing has no balloon {}", ids::encode(id))))?;
        let Some(balloon) = d.balloons.get_mut(at).map(Arc::make_mut) else { return Ok(Vec::new()) };
        let mut changed = Vec::new();
        if let Some(place) = request.set.at.filter(|place| *place != balloon.at) {
            balloon.at = place;
            changed.push("at".to_string());
        }
        if let Some(list) = request.set.parts_list.map(|list| list.map(|id| id.0)).filter(|list| *list != balloon.parts_list) {
            balloon.parts_list = list;
            changed.push("parts_list".to_string());
        }
        Ok(changed)
    })?;
    let reply = EditResult { balloon: ids::encode(id), changed, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

fn repair(s: &mut Session, p: &Value) -> Result<Value> {
    let request: RepairParams = crate::schema::read(REPAIR, p)?;
    let id = request.balloon.0;
    let unknown = || bad(REPAIR, format!("the drawing has no balloon {}", ids::encode(id)));
    let d = s.drawing(REPAIR)?;
    let view = d.balloons.by_id(id).ok_or_else(unknown)?.view;
    let read = crate::drawing_view::ViewSessions::new(s)
        .read_on(d, &[(id, view)], solidcraft_doc::Document::resolve_balloons)
        .map_err(|e| bad(REPAIR, e.to_string()))?;
    let now = read.into_iter().next().map(|r| r.status).ok_or_else(unknown)?;
    repair_reference(s, REPAIR, now, |s, copy| {
        let balloon = copy.balloons.index_of(id).and_then(|at| copy.balloons.get_mut(at)).map(Arc::make_mut).ok_or_else(unknown)?;
        Slot::Edge(&mut balloon.edge).take(s, REPAIR, &request.pick)
    })?;
    answer(s, REPAIR, id)
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let request: InspectParams = crate::schema::read(INSPECT, p)?;
    answer(s, INSPECT, request.balloon.0)
}

/// Balloon `id` of the open drawing as the model has it now.
fn answer(s: &Session, cmd: &str, id: u64) -> Result<Value> {
    let d = s.drawing(cmd)?;
    let balloon = d.balloons.by_id(id).ok_or_else(|| bad(cmd, format!("the drawing has no balloon {}", ids::encode(id))))?;
    let resolved = crate::drawing_view::ViewSessions::new(s)
        .read_on(d, &[(id, balloon.view)], solidcraft_doc::Document::resolve_balloons)
        .map_err(|e| bad(cmd, e.to_string()))?;
    let r = resolved.into_iter().next().ok_or_else(|| bad(cmd, format!("balloon {} wasn't read", ids::encode(id))))?;
    let (status, why, candidates) = status_words(r.status);
    let row = balloon_rows(d, &[balloon], |source| s.source_design(d, source)).pop().unwrap_or(RowRead::NoList(Vec::new()));
    // Why it has no number, in the sheet's words, when no edit to the balloon gives it one.
    let lasting = matches!(row, RowRead::Unlisted { why: Unlisted::Reference, .. } | RowRead::Unread { .. });
    let mut warnings: Vec<String> = if lasting { crate::drawing_scene::row_why(&row).into_iter().collect() } else { Vec::new() };
    warnings.extend(drawn_warning(d, balloon.view, [balloon.edge.path.as_slice()]));
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
        parts_list: row.list().map(ids::encode),
        row_status: row_word(&row).to_string(),
        item: row.item().map(str::to_string),
        choices: match row {
            RowRead::NoList(choices) => choices.into_iter().map(ids::encode).collect(),
            _ => Vec::new(),
        },
        warnings,
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// What a balloon reads of its parts list, in a reply's words: `ok`, `no_list`, `not_found`,
/// `reference`, `no_row` or `unread`.
pub(crate) fn row_word(row: &RowRead) -> &'static str {
    match row {
        RowRead::Item { .. } => "ok",
        RowRead::NoList(_) => "no_list",
        RowRead::Unlisted { why: Unlisted::NotFound, .. } => "not_found",
        RowRead::Unlisted { why: Unlisted::Reference, .. } => "reference",
        RowRead::Unlisted { why: Unlisted::NoRow, .. } => "no_row",
        RowRead::Unread { .. } => "unread",
    }
}

#[cfg(test)]
#[path = "drawing_balloon_tests.rs"]
mod tests;
