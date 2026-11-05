//! `drawing.note.add`, `.edit` and `.inspect` (W19e, #4896): a note on a sheet, a view, or an edge or
//! corner of a view's model, and where it stands with the model now. What it points at is picked as
//! a dimension's is and saved by name ([`solidcraft_doc::Document::edge_reference`]). Adding and
//! editing are one undo step of the drawing each, and none of the commands evaluates the model. A
//! note is deleted as a drawing's other objects are (`selection.delete`).

use std::sync::Arc;

use serde_json::Value;
use solidcraft_doc::drawing::note::{Anchor, Note};
use solidcraft_doc::ids;

use super::CommandSpec;
use super::drawing_dimension::status_words;
use crate::params::bad;
use crate::schema::drawing_note::{AddParams, AnchorParam, EditParams, EditResult, InspectParams, NoteResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const ADD: &str = "drawing.note.add";
const ADD_CONTRACT: TypedContract<AddParams> = TypedContract::new();
const EDIT: &str = "drawing.note.edit";
const EDIT_CONTRACT: TypedContract<EditParams> = TypedContract::new();
const INSPECT: &str = "drawing.note.inspect";
const INSPECT_CONTRACT: TypedContract<InspectParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new(ADD, "Add Note", crate::checked!("drawing.note.add", add))
        .icon("new")
        .in_drawing()
        .doc_only()
        .contract(&ADD_CONTRACT)
        .output(output_schema_for::<NoteResult>)
        .params(
            "anchor: {sheet: id or name}, {view: id}, {edge: {view, edge}} or {corner: {view, corner}} (an edge or a corner as drawing.dimension.add \
             takes it; its leader ends there); at: [x, y] on paper in mm, from the sheet's lower-left corner or the view's centre; content: text, \
             or [{text} or {field: {source: component|drawing|sheet|revision, key, format?}}]; cap_mm?: 0.5..100, its text height → what \
             drawing.note.inspect answers for it. Saved by name, never the point picked. One undo step; no model is evaluated.",
        ),
    CommandSpec::new(EDIT, "Edit Note", crate::checked!("drawing.note.edit", edit))
        .icon("edit")
        .in_drawing()
        .doc_only()
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<EditResult>)
        .params(
            "note: its id; set: {at?: [x, y], content?, cap_mm?: a height, or null for the drawing's} → {note, changed: [fields]}. What it \
             belongs to stays: to point at something else, add a new one. One undo step, none when nothing changes; no model is evaluated.",
        ),
    CommandSpec::new(INSPECT, "Inspect Note", crate::checked!("drawing.note.inspect", inspect))
        .icon("inspect")
        .in_drawing()
        .noundo()
        .doc_only()
        .contract(&INSPECT_CONTRACT)
        .output(output_schema_for::<NoteResult>)
        .params(
            "note: its id → {note, record: as saved, status: ok, ambiguous, suppressed or lost, why?, point?: where its leader ends, in the \
             design's frame, exact, candidates?: [{edge: {body, index}}]}. A lost reference keeps the note and its text in place, without a \
             leader. Read-only; no model is evaluated.",
        ),
];

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let request: AddParams = crate::schema::read(ADD, p)?;
    let record = s.drawing(ADD)?.clone();
    let on_view = |view: u64| record.view(view).map(|_| view).ok_or_else(|| bad(ADD, format!("the drawing has no view {}", ids::encode(view))));
    let anchor = match request.anchor {
        AnchorParam::Sheet(named) => Anchor::Sheet(crate::drawing_scene::sheet_id(&record, &named, ADD)?),
        AnchorParam::View(view) => Anchor::View(on_view(view.0)?),
        AnchorParam::Edge { view, edge } => Anchor::Edge {
            view: on_view(view.0)?,
            edge: s.doc.edge_reference(&s.model, &edge.pick, &edge.path).map_err(|why| bad(ADD, why.to_string()))?,
        },
        AnchorParam::Corner { view, corner } => Anchor::Corner {
            view: on_view(view.0)?,
            point: s.doc.corner_reference(&s.model, &corner.pick, &corner.path).map_err(|why| bad(ADD, why.to_string()))?,
        },
    };
    let id = record.allocate_id(&mut s.ids).map_err(|e| bad(ADD, e.to_string()))?;
    let note = Note { id, anchor, at: request.at, content: request.content.0, cap_mm: request.cap_mm };
    s.edit_drawing(ADD, |d| {
        d.notes.add([Arc::new(note)]);
        Ok(())
    })?;
    answer(s, ADD, id)
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let request: EditParams = crate::schema::read(EDIT, p)?;
    let id = request.note.0;
    let changed = s.edit_drawing(EDIT, |d| {
        let at = d.notes.index_of(id).ok_or_else(|| bad(EDIT, format!("the drawing has no note {}", ids::encode(id))))?;
        let Some(note) = d.notes.get_mut(at).map(Arc::make_mut) else { return Ok(Vec::new()) };
        let mut changed = Vec::new();
        if let Some(place) = request.set.at.filter(|place| *place != note.at) {
            note.at = place;
            changed.push("at".to_string());
        }
        if let Some(content) = request.set.content.map(|content| content.0).filter(|content| *content != note.content) {
            note.content = content;
            changed.push("content".to_string());
        }
        if let Some(cap) = request.set.cap_mm.filter(|cap| *cap != note.cap_mm) {
            note.cap_mm = cap;
            changed.push("cap_mm".to_string());
        }
        Ok(changed)
    })?;
    let reply = EditResult { note: ids::encode(id), changed, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let request: InspectParams = crate::schema::read(INSPECT, p)?;
    answer(s, INSPECT, request.note.0)
}

/// Note `id` of the open drawing as the model has it now.
fn answer(s: &Session, cmd: &str, id: u64) -> Result<Value> {
    let d = s.drawing(cmd)?;
    let note = d.notes.by_id(id).ok_or_else(|| bad(cmd, format!("the drawing has no note {}", ids::encode(id))))?;
    let resolved = s.doc.resolve_notes(&s.model, d, &[id]).map_err(|e| bad(cmd, e.to_string()))?;
    let r = resolved.into_iter().next().ok_or_else(|| bad(cmd, format!("note {} wasn't read", ids::encode(id))))?;
    let (status, why, candidates) = status_words(r.status);
    let reply = NoteResult {
        note: ids::encode(id),
        record: serde_json::to_value(note).map_err(|e| EngineError::Other(e.to_string()))?,
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
#[path = "drawing_note_tests.rs"]
mod tests;
