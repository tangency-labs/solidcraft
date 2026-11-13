//! `drawing.note.add`, `.edit` and `.inspect` (W19e, #4896): a note on a sheet, a view, or an edge or
//! corner of a view's model, and where it stands with the model now. What it points at is picked as
//! a dimension's is and saved by name ([`solidcraft_doc::Document::edge_reference`]). Adding and
//! editing are one undo step of the drawing each, and none of the commands evaluates the model. A
//! note is deleted as a drawing's other objects are (`selection.delete`).

use std::sync::Arc;

use serde_json::Value;
use solidcraft_doc::bom::Tree;
use solidcraft_doc::drawing::fields::{FieldState, Values, no_field};
use solidcraft_doc::drawing::note::{Anchor, Branch, End, Note, leaders_refused};
use solidcraft_doc::ids;
use solidcraft_geom::cell::{Content, Part};

use super::CommandSpec;
use super::drawing_dimension::status_words;
use super::drawing_view::{check_drawn, drawn_warning};
use crate::params::bad;
use crate::schema::drawing_note::{
    AddParams, AnchorParam, BranchParam, BranchResult, BranchesSet, EditParams, EditResult, FieldResult, InspectParams, NoteResult,
};
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
             or [{text} or {field: {source: component|drawing|sheet|revision, key, format?}}]; cap_mm?: 0.5..100, its text height; branches?: [{edge} or \
             {corner}], more leaders of a note on a view, 16 leaders in all, its anchor's included → what drawing.note.inspect answers \
             for it. Saved by name, never the point picked; an edge or a corner on a part its view doesn't \
             draw is refused. One undo step; no model is evaluated.",
        ),
    CommandSpec::new(EDIT, "Edit Note", crate::checked!("drawing.note.edit", edit))
        .icon("edit")
        .in_drawing()
        .doc_only()
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<EditResult>)
        .params(
            "note: its id; set: {at?: [x, y], content?, cap_mm?: a height, or null for the drawing's, branches?: {remove?: [numbers from 0 \
             as the list is now, each once], add?: [{edge} or {corner}], after those that stay}} → {note, changed: [fields]}. What it \
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
             design's frame, exact, candidates?: [{edge: {body, index}}], branches?: [{status, why?, point?, exact, candidates?}] \
             (a lost one loses only its own leader), text: what it says now, its fields read for its sheet, \
             fields?: [{source, key, state: shown, empty, lost (its sheet documents a component the design no longer has) or unknown \
             (its source has no such field), value?, why?}], \
             warnings?: its view edited since to leave out a part a leader points at}. A lost reference keeps the note and its text in place, without a \
             leader. Read-only; no model is evaluated.",
        ),
];

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let request: AddParams = crate::schema::read(ADD, p)?;
    // Counted before anything is looked up.
    let (on_view, own) = match request.anchor {
        AnchorParam::Sheet(_) => (false, 0),
        AnchorParam::View(_) => (true, 0),
        AnchorParam::Edge { .. } | AnchorParam::Corner { .. } => (true, 1),
    };
    if let Some(why) = leaders_refused(on_view, request.branches.len(), own + request.branches.len()) {
        return Err(bad(ADD, why));
    }
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
    let branches = request.branches.iter().map(|pick| branch(s, pick, ADD)).collect::<Result<Vec<_>>>()?;
    let mut note = Note { id: 0, anchor, at: request.at, content: request.content.0, cap_mm: request.cap_mm, branches };
    if let Some(view) = note.anchor.view() {
        check_drawn(&record, view, note.paths(), ADD)?;
    }
    note.id = record.allocate_id(&mut s.ids).map_err(|e| bad(ADD, e.to_string()))?;
    let id = note.id;
    s.edit_drawing(ADD, |d| {
        d.notes.add([Arc::new(note)]);
        Ok(())
    })?;
    answer(s, ADD, id)
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let request: EditParams = crate::schema::read(EDIT, p)?;
    let id = request.note.0;
    let branches = request.set.branches.map(|set| new_branches(s, id, set)).transpose()?;
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
        if let Some(branches) = branches.filter(|branches| *branches != note.branches) {
            note.branches = branches;
            changed.push("branches".to_string());
        }
        Ok(changed)
    })?;
    let reply = EditResult { note: ids::encode(id), changed, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// `pick` as a branch, saved by name as an anchor's edge or corner is.
fn branch(s: &Session, pick: &BranchParam, cmd: &str) -> Result<Branch> {
    match pick {
        BranchParam::Edge(edge) => s.doc.edge_reference(&s.model, &edge.pick, &edge.path).map(Branch::Edge),
        BranchParam::Corner(corner) => s.doc.corner_reference(&s.model, &corner.pick, &corner.path).map(Branch::Corner),
    }
    .map_err(|why| bad(cmd, why.to_string()))
}

/// Note `id`'s branches after `set`: those it takes out by number as the list is now, each once,
/// then those it adds, found as an add finds them. The numbers and the count are checked before
/// anything is looked up, and each branch it adds is on a part its view draws (one kept from before
/// may not be, and can be taken out).
fn new_branches(s: &Session, id: u64, set: BranchesSet) -> Result<Vec<Branch>> {
    let record = s.drawing(EDIT)?;
    let note = record.notes.by_id(id).ok_or_else(|| bad(EDIT, format!("the drawing has no note {}", ids::encode(id))))?;
    let mut gone = set.remove;
    gone.sort_unstable();
    if gone.windows(2).any(|pair| pair.first() == pair.get(1)) {
        return Err(bad(EDIT, "`branches.remove` names each branch once"));
    }
    if gone.last().is_some_and(|last| *last >= note.branches.len()) {
        return Err(bad(
            EDIT,
            match note.branches.len().checked_sub(1) {
                Some(last) => format!("`branches.remove` numbers this note's branches, 0 to {last}"),
                None => "`branches.remove`: this note has no branches".into(),
            },
        ));
    }
    let count = note.branches.len().saturating_sub(gone.len()).saturating_add(set.add.len());
    let own = usize::from(note.anchor.path().is_some());
    if let Some(why) = leaders_refused(note.anchor.view().is_some(), count, own.saturating_add(count)) {
        return Err(bad(EDIT, why));
    }
    let added = set.add.iter().map(|pick| branch(s, pick, EDIT)).collect::<Result<Vec<_>>>()?;
    if let Some(view) = note.anchor.view() {
        check_drawn(record, view, added.iter().map(Branch::path), EDIT)?;
    }
    let kept = note.branches.iter().enumerate().filter(|(at, _)| gone.binary_search(at).is_err()).map(|(_, branch)| branch.clone());
    Ok(kept.chain(added).collect())
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let request: InspectParams = crate::schema::read(INSPECT, p)?;
    answer(s, INSPECT, request.note.0)
}

/// Note `id` of the open drawing as the model has it now.
fn answer(s: &Session, cmd: &str, id: u64) -> Result<Value> {
    let d = s.drawing(cmd)?;
    let note = d.notes.by_id(id).ok_or_else(|| bad(cmd, format!("the drawing has no note {}", ids::encode(id))))?;
    let resolved = crate::drawing_view::ViewSessions::new(s)
        .read_on(d, &[(id, note.anchor.view().unwrap_or_default())], solidcraft_doc::Document::resolve_notes)
        .map_err(|e| bad(cmd, e.to_string()))?;
    let r = resolved.into_iter().next().ok_or_else(|| bad(cmd, format!("note {} wasn't read", ids::encode(id))))?;
    let (status, why, candidates) = status_words(r.anchor.status);
    let state = s.model.state();
    let read = |values: &Values| (values.text(&note.content), fields(values, &note.content));
    let at = d.sheet_of(&note.anchor).ok_or_else(|| bad(cmd, format!("note {} is on no sheet", ids::encode(id))))?;
    let (text, fields) = crate::title_block_text::with_values(&s.doc, &mut Tree::new(&s.doc, &state), d, at, std::iter::once(&note.content), read);
    let reply = NoteResult {
        note: ids::encode(id),
        record: serde_json::to_value(note).map_err(|e| EngineError::Other(e.to_string()))?,
        status: status.to_string(),
        why,
        point: r.anchor.point.map(|p| [p.x, p.y, p.z]),
        exact: r.anchor.exact,
        candidates,
        branches: r.branches.into_iter().map(branch_result).collect(),
        text,
        fields,
        warnings: note.anchor.view().map(|view| drawn_warning(d, view, note.paths())).unwrap_or_default(),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// A branch's end in a reply's words.
fn branch_result(end: End) -> BranchResult {
    let point = end.point.map(|p| [p.x, p.y, p.z]);
    let (status, why, candidates) = status_words(end.status);
    BranchResult { status: status.into(), why, point, exact: end.exact, candidates }
}

/// Each field `content` reads, in order, as `values` read it.
fn fields(values: &Values, content: &Content) -> Vec<FieldResult> {
    let field = |part: &Part| {
        let Part::Field { source, key, .. } = part else { return None };
        let (state, value) = match values.state(*source, key) {
            FieldState::Shown(value) => ("shown", Some(value)),
            FieldState::Empty => ("empty", None),
            FieldState::Lost => ("lost", None),
            FieldState::Unknown => ("unknown", None),
        };
        Some(FieldResult { source: *source, key: key.clone(), state: state.into(), value, why: no_field(*source, key) })
    };
    content.0.iter().filter_map(field).collect()
}

#[cfg(test)]
#[path = "drawing_note_tests.rs"]
mod tests;
