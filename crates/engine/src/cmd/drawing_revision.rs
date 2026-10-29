//! A drawing's revision rows (#3874, W31): add, edit and delete them, each one undo step of the
//! drawing (none when nothing changes), and list them. One revision record covers the whole drawing
//! ([`solidcraft_geom::revision`]): a new row gets the next revision, after the highest the record
//! has given, and people type the rest. The newest row is the drawing's revision, hidden or not.

use std::sync::Arc;

use serde_json::Value;
use solidcraft_doc::ids;
use solidcraft_geom::revision::{Field, RevisionRecord, RevisionRow, SkippedLetter};

use super::CommandSpec;
use crate::params::bad;
use crate::schema::drawing_revision::{RevisionAddParams, RevisionDeleteParams, RevisionEditParams, RevisionsResult, RowResult, RowSet};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const ADD: &str = "drawing.revision.add";
const ADD_CONTRACT: TypedContract<RevisionAddParams> = TypedContract::new();
const EDIT: &str = "drawing.revision.edit";
const EDIT_CONTRACT: TypedContract<RevisionEditParams> = TypedContract::new();
const DELETE: &str = "drawing.revision.delete";
const DELETE_CONTRACT: TypedContract<RevisionDeleteParams> = TypedContract::new();
const LIST: &str = "drawing.revision.list";

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new(ADD, "Add Revision", crate::checked!("drawing.revision.add", ADD_CONTRACT, add))
        .icon("new")
        .in_drawing()
        .doc_only()
        .contract(&ADD_CONTRACT)
        .output(output_schema_for::<RevisionsResult>)
        .params("set?: {revision?, description?, designer?, checker?, date?, hidden?}: the new row's texts (the revision, when not given, the next one: after the highest the drawing has given) → {row: its id, rows, next}. One undo step."),
    CommandSpec::new(EDIT, "Edit Revision", crate::checked!("drawing.revision.edit", EDIT_CONTRACT, edit))
        .icon("edit")
        .in_drawing()
        .doc_only()
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<RevisionsResult>)
        .params("row: a row's id; set: {revision?, description?, designer?, checker?, date?: each as typed (a revision is one line); hidden?: left out of the revision table} → {row, rows, next, changed: [fields], warnings?: a revision in letters with a letter the lettering skips}. One undo step, none when nothing changes."),
    CommandSpec::new(DELETE, "Delete Revision", crate::checked!("drawing.revision.delete", DELETE_CONTRACT, delete))
        .icon("delete")
        .in_drawing()
        .doc_only()
        .contract(&DELETE_CONTRACT)
        .output(output_schema_for::<RevisionsResult>)
        .params("row: a row's id → {row, rows, next, tags_lost?: the revision tags bound to it, which keep the revision they showed}. Its revision is not given out again. One undo step."),
    CommandSpec::new(LIST, "List Revisions", list).icon("inspect").in_drawing().noundo().doc_only().output(output_schema_for::<RevisionsResult>).params(
        "→ {rows: [{row, revision, description, designer, checker, date, hidden}], the oldest first, next: the revision a new row gets}. Read-only.",
    ),
];

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let request: RevisionAddParams = crate::schema::read(ADD, p)?;
    let id = s.drawing(ADD)?.clone().allocate_id(&mut s.ids).map_err(|e| bad(ADD, e.to_string()))?;
    let warnings = s.edit_drawing(ADD, |d| {
        let record = Arc::make_mut(&mut d.revisions);
        record.add(None, id).map_err(|e| bad(ADD, e.to_string()))?;
        apply(record, id, &request.set, ADD)
    })?;
    answer(s, ADD, RevisionsResult { row: Some(ids::encode(id)), warnings, ..Default::default() })
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let request: RevisionEditParams = crate::schema::read(EDIT, p)?;
    let id = request.row.0;
    let before = row(s.drawing(EDIT)?.revisions.as_ref(), id, EDIT)?.clone();
    let warnings = s.edit_drawing(EDIT, |d| apply(Arc::make_mut(&mut d.revisions), id, &request.set, EDIT))?;
    let after = row(s.drawing(EDIT)?.revisions.as_ref(), id, EDIT)?;
    let mut changed: Vec<String> =
        Field::ALL.into_iter().filter(|f| before.field(f.key()) != after.field(f.key())).map(|f| f.key().to_string()).collect();
    if before.hidden != after.hidden {
        changed.push("hidden".into());
    }
    answer(s, EDIT, RevisionsResult { row: Some(ids::encode(id)), changed, warnings, ..Default::default() })
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let request: RevisionDeleteParams = crate::schema::read(DELETE, p)?;
    let id = request.row.0;
    let lost = s.edit_drawing(DELETE, |d| Arc::make_mut(&mut d.revisions).delete(id).map_err(|e| bad(DELETE, e.to_string())))?;
    answer(s, DELETE, RevisionsResult { row: Some(ids::encode(id)), tags_lost: lost.into_iter().map(ids::encode).collect(), ..Default::default() })
}

fn list(s: &mut Session, _p: &Value) -> Result<Value> {
    answer(s, LIST, RevisionsResult::default())
}

/// Sets what `set` gives on row `id`: the warnings for a revision in letters with a letter the
/// lettering skips.
fn apply(record: &mut RevisionRecord, id: u64, set: &RowSet, cmd: &str) -> Result<Vec<String>> {
    let texts = [
        (Field::Revision, &set.revision),
        (Field::Description, &set.description),
        (Field::Designer, &set.designer),
        (Field::Checker, &set.checker),
        (Field::Date, &set.date),
    ];
    let mut warnings = Vec::new();
    for (field, text) in texts {
        let Some(text) = text else { continue };
        if let Some(SkippedLetter(letter)) = record.set(id, field, text).map_err(|e| bad(cmd, e.to_string()))? {
            warnings.push(format!("the revisions' lettering skips {letter}: kept as typed"));
        }
    }
    if let Some(hidden) = set.hidden {
        record.hide(id, hidden).map_err(|e| bad(cmd, e.to_string()))?;
    }
    Ok(warnings)
}

fn row<'a>(record: &'a RevisionRecord, id: u64, cmd: &str) -> Result<&'a RevisionRow> {
    record.rows().iter().find(|r| r.id == id).ok_or_else(|| bad(cmd, format!("no revision row {}", ids::encode(id))))
}

/// `reply` with the drawing's rows and its next revision, as every revision command answers.
fn answer(s: &Session, cmd: &str, reply: RevisionsResult) -> Result<Value> {
    let record = &s.drawing(cmd)?.revisions;
    let rows = record
        .rows()
        .iter()
        .map(|r| RowResult {
            row: ids::encode(r.id),
            revision: r.revision.clone(),
            description: r.description.clone(),
            designer: r.designer.clone(),
            checker: r.checker.clone(),
            date: r.date.clone(),
            hidden: r.hidden,
        })
        .collect();
    let reply = RevisionsResult { rows, next: record.next_revision(None), ..reply };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

#[cfg(test)]
#[path = "drawing_revision_tests.rs"]
mod tests;
