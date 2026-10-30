//! A drawing's revision rows (#3874, W31): add, edit and delete them, each one undo step of the
//! drawing (none when nothing changes); `drawing.inspect` lists them. One revision record covers the whole drawing
//! ([`solidcraft_geom::revision`]): a new row gets the next revision, after the highest the record
//! has given, and people type the rest. The newest row is the drawing's revision, hidden or not.
//!
//! `drawing.revise` (#3874 R1) issues a revision: it appends a copy of a row to the issued history
//! with the design's version when there is one. The copy keeps what the row read, whatever becomes
//! of the row. It never fills in a date, a checker or an approval.

use std::sync::Arc;

use serde_json::Value;
use solidcraft_doc::ids;
use solidcraft_geom::revision::{Field, ModelVersion, RevisionRecord, RevisionRow, SkippedLetter};

use super::CommandSpec;
use crate::params::bad;
use crate::schema::drawing_revision::{
    ReviseParams, RevisionAddParams, RevisionDeleteParams, RevisionEditParams, RevisionsResult, RowResult, RowSet, VersionResult,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const ADD: &str = "drawing.revision.add";
const ADD_CONTRACT: TypedContract<RevisionAddParams> = TypedContract::new();
const EDIT: &str = "drawing.revision.edit";
const EDIT_CONTRACT: TypedContract<RevisionEditParams> = TypedContract::new();
const DELETE: &str = "drawing.revision.delete";
const DELETE_CONTRACT: TypedContract<RevisionDeleteParams> = TypedContract::new();
const REVISE: &str = "drawing.revise";
const REVISE_CONTRACT: TypedContract<ReviseParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new(ADD, "Add Revision", crate::checked!("drawing.revision.add", add))
        .icon("new")
        .in_drawing()
        .doc_only()
        .contract(&ADD_CONTRACT)
        .output(output_schema_for::<RevisionsResult>)
        .params("set?: {revision?, description?, designer?, checker?, date?, hidden?}: the new row's texts (the revision, when not given, the next one: after the highest the drawing has given) → {row: its id, rows, next}. One undo step."),
    CommandSpec::new(REVISE, "Issue Revision", crate::checked!("drawing.revise", revise))
        .icon("new")
        .in_drawing()
        .doc_only()
        .contract(&REVISE_CONTRACT)
        .output(output_schema_for::<RevisionsResult>)
        .params("row?: a row's id, issued as it reads; set?: without `row`, the texts of a new revision to add and issue (as drawing.revision.add takes them) → {row, rows, next, issued: the issued history, the oldest first, version?: {path, change}, what it was issued against}. One undo step. A row is issued once; its copy keeps what it read when the row is edited, hidden or deleted. Nothing is filled in for you: no date, checker or approval."),
    CommandSpec::new(EDIT, "Edit Revision", crate::checked!("drawing.revision.edit", edit))
        .icon("edit")
        .in_drawing()
        .doc_only()
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<RevisionsResult>)
        .params("row: a row's id; set: {revision?, description?, designer?, checker?, date?: each as typed (a revision is one line); hidden?: left out of the revision table} → {row, rows, next, changed: [fields], warnings?: a revision in letters with a letter the lettering skips}. One undo step, none when nothing changes."),
    CommandSpec::new(DELETE, "Delete Revision", crate::checked!("drawing.revision.delete", delete))
        .icon("delete")
        .in_drawing()
        .doc_only()
        .contract(&DELETE_CONTRACT)
        .output(output_schema_for::<RevisionsResult>)
        .params("row: a row's id → {row, rows, next, tags_lost?: the revision tags bound to it, which keep the revision they showed}. Its revision is not given out again. One undo step."),
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

fn revise(s: &mut Session, p: &Value) -> Result<Value> {
    let version = s.design_version();
    revise_against(s, p, version)
}

/// `drawing.revise`, issuing against `version`.
fn revise_against(s: &mut Session, p: &Value, version: Option<ModelVersion>) -> Result<Value> {
    let request: ReviseParams = crate::schema::read(REVISE, p)?;
    if request.row.is_some() && request.set.is_some() {
        return Err(bad(REVISE, "a row is issued as it reads: `set` is for a new revision, with no `row`"));
    }
    let new = match request.row {
        Some(_) => None,
        None => Some(s.drawing(REVISE)?.clone().allocate_id(&mut s.ids).map_err(|e| bad(REVISE, e.to_string()))?),
    };
    let reported = version.as_ref().map(|v| VersionResult { path: v.path.clone(), change: v.change.clone() });
    let (id, warnings) = s.edit_drawing(REVISE, |d| {
        let record = Arc::make_mut(&mut d.revisions);
        let (id, warnings) = match (request.row, new) {
            (Some(row), _) => (row.0, Vec::new()),
            (None, Some(id)) => {
                record.add(None, id).map_err(|e| bad(REVISE, e.to_string()))?;
                (id, apply(record, id, &request.set.unwrap_or_default(), REVISE)?)
            }
            (None, None) => return Err(bad(REVISE, "no row to issue")),
        };
        record.issue(id, version).map_err(|e| bad(REVISE, e.to_string()))?;
        Ok((id, warnings))
    })?;
    answer(s, REVISE, RevisionsResult { row: Some(ids::encode(id)), version: reported, warnings, ..Default::default() })
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let request: RevisionEditParams = crate::schema::read(EDIT, p)?;
    let id = request.row.0;
    let before = row(s.drawing(EDIT)?.revisions.as_ref(), id, EDIT)?.clone();
    let mut warnings = s.edit_drawing(EDIT, |d| apply(Arc::make_mut(&mut d.revisions), id, &request.set, EDIT))?;
    let record = s.drawing(EDIT)?.revisions.clone();
    let after = row(&record, id, EDIT)?;
    if *after != before && record.is_issued(id) {
        warnings.push("its issued copy keeps what it read".to_string());
    }
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
    let reply = RevisionsResult { rows: rows(record), next: record.next_revision(None), issued: issued(record), ..reply };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

fn row_result(r: &RevisionRow) -> RowResult {
    RowResult {
        row: ids::encode(r.id),
        revision: r.revision.clone(),
        description: r.description.clone(),
        designer: r.designer.clone(),
        checker: r.checker.clone(),
        date: r.date.clone(),
        hidden: r.hidden,
        version: r.model_version.as_ref().map(|v| VersionResult { path: v.path.clone(), change: v.change.clone() }),
    }
}

/// Every row of `record`, the oldest first: what the revision commands and `drawing.inspect` list.
pub(super) fn rows(record: &RevisionRecord) -> Vec<RowResult> {
    record.rows().iter().map(row_result).collect()
}

/// The issued history of `record`, the oldest first.
pub(super) fn issued(record: &RevisionRecord) -> Vec<RowResult> {
    record.issued().iter().map(row_result).collect()
}

#[cfg(test)]
#[path = "drawing_revision_tests.rs"]
mod tests;
