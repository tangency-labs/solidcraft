//! `drawing.balloon.add`, `.edit`, `.repair` and `.inspect` (W26, #3874; B5, #4892): a balloon on a
//! saved view of the open drawing, pointing at a placed part by an edge of it, a new name for the
//! edge when it is lost, and where it stands with the model now. The edge is picked as a
//! dimension's is and saved by name ([`solidcraft_doc::Document::edge_reference`]); its path is the
//! part the balloon numbers, and a repair keeps it. Adding, editing and repairing are one undo step
//! of the drawing each, and none of the commands evaluates the model.
//!
//! The number it shows is its row's item in the parts list it reads
//! ([`solidcraft_doc::drawing::Drawing::parts_list_for`]), worked out on each read and never saved.
//! One list is made for a read, with each row's members, then one lookup per level of its path.

use std::sync::Arc;

use serde_json::Value;
use solidcraft_doc::bom::{self, Tree, Unlisted};
use solidcraft_doc::drawing::Drawing;
use solidcraft_doc::drawing::balloon::Balloon;
use solidcraft_doc::ids;

use super::CommandSpec;
use super::drawing_dimension::{Slot, repair_reference, status_words};
use super::drawing_view::{check_drawn, drawn_warning};
use crate::params::bad;
use crate::schema::drawing_balloon::{AddParams, BalloonResult, EditParams, EditResult, InspectParams, RepairParams};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const ADD: &str = "drawing.balloon.add";
const ADD_CONTRACT: TypedContract<AddParams> = TypedContract::new();
const EDIT: &str = "drawing.balloon.edit";
const EDIT_CONTRACT: TypedContract<EditParams> = TypedContract::new();
const INSPECT: &str = "drawing.balloon.inspect";
const INSPECT_CONTRACT: TypedContract<InspectParams> = TypedContract::new();
const REPAIR: &str = "drawing.balloon.repair";
const REPAIR_CONTRACT: TypedContract<RepairParams> = TypedContract::new();

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
             least one); at: [x, y], the balloon's centre on paper in mm from the view's centre; parts_list?: the table it reads \
             (default its sheet's only parts list, else the drawing's) → what drawing.balloon.inspect \
             answers for it. Saved by the edge's name, never the point picked; one on a part its view doesn't draw (its hide or \n             source leaves it out) is refused. One undo step; no model is evaluated.",
        ),
    CommandSpec::new(EDIT, "Edit Balloon", crate::checked!("drawing.balloon.edit", edit))
        .icon("edit")
        .in_drawing()
        .doc_only()
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<EditResult>)
        .params(
            "balloon: its id; set: {at?: [x, y], parts_list?: a table's id, or null to read the only list} → {balloon, changed: [fields]}. What it points at stays: to number another part, add \
             a new one. One undo step, none when nothing changes; no model is evaluated.",
        ),
    CommandSpec::new(REPAIR, "Repair Balloon", crate::checked!("drawing.balloon.repair", repair))
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
             row_status: ok, no_list, reference, no_row or not_found, warnings?: such as its view edited since to leave its part out}. A lost row never removes it. Read-only; no model is \
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
    let row = read_row(s, d, balloon);
    let mut warnings = if row.status == "reference" { vec!["it points at a reference, which no parts list counts".to_string()] } else { Vec::new() };
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
        parts_list: row.list.map(ids::encode),
        choices: row.choices.into_iter().map(ids::encode).collect(),
        item: row.item,
        row_status: row.status.to_string(),
        warnings,
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// A balloon's row in the parts list it reads.
#[derive(Default)]
struct RowRead {
    list: Option<u64>,
    choices: Vec<u64>,
    item: Option<String>,
    status: &'static str,
}

fn read_row(s: &Session, d: &Drawing, balloon: &Balloon) -> RowRead {
    let (list, parts_list) = match d.parts_list_for(balloon) {
        Ok(found) => found,
        Err(choices) => return RowRead { choices, status: "no_list", ..Default::default() },
    };
    let state = s.model.state();
    let mut tree = Tree::new(&s.doc, &state);
    // Its component deleted, or too large to list: nothing it can read.
    let Ok(listed) = bom::list_with(&mut tree, &parts_list.request(), true) else {
        return RowRead { list: Some(list), status: "not_found", ..Default::default() };
    };
    match listed.item_of(&tree, balloon.occurrence()) {
        Ok(item) => RowRead { list: Some(list), item: Some(item), status: "ok", ..Default::default() },
        Err(why) => {
            let status = match why {
                Unlisted::NotFound => "not_found",
                Unlisted::Reference => "reference",
                Unlisted::NoRow => "no_row",
            };
            RowRead { list: Some(list), status, ..Default::default() }
        }
    }
}

#[cfg(test)]
#[path = "drawing_balloon_tests.rs"]
mod tests;
