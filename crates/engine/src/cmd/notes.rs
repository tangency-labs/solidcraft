//! Note commands (#1631): personal notes anchored to points in model space, saved with the
//! design and drawn in the viewport as screen-facing callouts. `note.add` anchors a note,
//! `note.update` edits its text, anchor, or hidden flag, `note.delete` removes it, and
//! `note.list` reads them back. Everything lives in the document, so notes are saved and
//! undone like any other design change.

use serde_json::{Value, json};

use solidcraft_doc::Note;

use super::CommandSpec;
use crate::params::{bad, bool_, req_vec3, str_};
use crate::schema::notes::{NoteAddParams, NoteDeleteParams, NoteDeleted, NoteId, NoteItem, NoteListParams, NoteUpdateParams, NotesList};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const ADD: TypedContract<NoteAddParams> = TypedContract::new();
const UPDATE: TypedContract<NoteUpdateParams> = TypedContract::new();
const DELETE: TypedContract<NoteDeleteParams> = TypedContract::new();
const LIST: TypedContract<NoteListParams> = TypedContract::new();

/// Notes a design may hold.
const MAX_NOTES: usize = 1000;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("note.add", "Add Note", crate::checked!("note.add", note_add))
        .icon("text")
        .contract(&ADD)
        .output(output_schema_for::<NoteId>)
        .params("anchor: [x,y,z] (the point in model space, mm); text: the note (may span lines); hidden?: bool (saved but not drawn) → note: id"),
    CommandSpec::new("note.update", "Edit Note", crate::checked!("note.update", note_update))
        .icon("edit")
        .contract(&UPDATE)
        .output(output_schema_for::<NoteId>)
        .params("note: id; text?: the new text; anchor?: [x,y,z] (move the anchor); hidden?: bool (show or hide) → note: id"),
    CommandSpec::new("note.delete", "Delete Note", crate::checked!("note.delete", note_delete))
        .icon("delete")
        .contract(&DELETE)
        .output(output_schema_for::<NoteDeleted>)
        .params("note: id → deleted: id"),
    CommandSpec::new("note.list", "List Notes", crate::checked!("note.list", note_list))
        .icon("list")
        .contract(&LIST)
        .output(output_schema_for::<NotesList>)
        .noundo()
        .doc_only()
        .params("→ notes: [{id, anchor: [x,y,z], text, hidden}] in creation order"),
];

fn note_add(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "note.add";
    let anchor = req_vec3(CMD, p, "anchor")?;
    let text = str_(p, "text").ok_or_else(|| bad(CMD, "`text` is required"))?;
    if !Note::text_is_valid(text) {
        return Err(bad(CMD, format!("`text` must not be empty and must be at most {} characters", solidcraft_doc::notes::MAX_NOTE_CHARS)));
    }
    if s.doc.notes.len() >= MAX_NOTES {
        return Err(bad(CMD, "too many notes"));
    }
    let id = s.doc_mut().allocate_id()?;
    let hidden = bool_(p, "hidden").unwrap_or(false);
    s.doc_mut().notes.push(Note { id, anchor, text: text.to_string(), hidden });
    Ok(json!({ "note": id }))
}

/// The index of a note by id.
fn note_index(s: &Session, p: &Value, cmd: &str) -> Result<usize> {
    let id = p.get("note").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "`note` must be a note id"))?;
    s.doc.notes.iter().position(|n| n.id == id).ok_or_else(|| bad(cmd, format!("no note {id}")))
}

fn note_update(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "note.update";
    let i = note_index(s, p, CMD)?;
    let text = match str_(p, "text") {
        Some(t) if Note::text_is_valid(t) => Some(t.to_string()),
        Some(_) => {
            return Err(bad(CMD, format!("`text` must not be empty and must be at most {} characters", solidcraft_doc::notes::MAX_NOTE_CHARS)));
        }
        None => None,
    };
    let anchor = match p.get("anchor") {
        Some(_) => Some(req_vec3(CMD, p, "anchor")?),
        None => None,
    };
    let hidden = bool_(p, "hidden");
    if text.is_none() && anchor.is_none() && hidden.is_none() {
        return Err(bad(CMD, "give at least one of `text`, `anchor`, `hidden`"));
    }
    let id = {
        let n = &mut s.doc_mut().notes[i];
        if let Some(t) = text {
            n.text = t;
        }
        if let Some(a) = anchor {
            n.anchor = a;
        }
        if let Some(h) = hidden {
            n.hidden = h;
        }
        n.id
    };
    Ok(json!({ "note": id }))
}

fn note_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let i = note_index(s, p, "note.delete")?;
    let n = s.doc_mut().notes.remove(i);
    Ok(json!({ "deleted": n.id }))
}

fn note_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let notes: Vec<NoteItem> = s
        .doc
        .notes
        .iter()
        .map(|n| NoteItem { id: n.id, anchor: [n.anchor.x, n.anchor.y, n.anchor.z], text: n.text.clone(), hidden: n.hidden })
        .collect();
    Ok(json!({ "notes": notes }))
}

#[cfg(test)]
#[path = "notes_tests.rs"]
mod tests;
