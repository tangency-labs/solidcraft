//! `drawing.table.add`, `.edit` and `.delete` (#3874): a parts list or a revision table saved on a
//! sheet of the open drawing. Its definition is the record's ([`solidcraft_doc::drawing::table`]).
//! Each is one undo step of the drawing, and none evaluates the model.
//!
//! Placing a parts list saves the item numbers its view shows, once for each view, so what is
//! printed doesn't move. Numbers are the design's, not the drawing's: undoing the placing puts back
//! only the drawing, as every drawing step does, and the numbers stay, which are the ones the view
//! showed anyway.

use std::sync::Arc;

use serde_json::Value;
use solidcraft_doc::drawing::table::{Kind, MassFormat, PartsList, SheetTable};
use solidcraft_doc::drawing::{Drawing, OnSheet, SheetComponent};
use solidcraft_doc::ids;

use super::CommandSpec;
use crate::drawing_scene::sheet_id;
use crate::params::bad;
use crate::schema::drawing_table::{AddParams, DeleteParams, DeleteResult, EditParams, KindParam, ListSet, TableResult, TableSet};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const ADD: &str = "drawing.table.add";
const ADD_CONTRACT: TypedContract<AddParams> = TypedContract::new();
const EDIT: &str = "drawing.table.edit";
const EDIT_CONTRACT: TypedContract<EditParams> = TypedContract::new();
const DELETE: &str = "drawing.table.delete";
const DELETE_CONTRACT: TypedContract<DeleteParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new(ADD, "Add Table", crate::checked!("drawing.table.add", add))
        .icon("new")
        .in_drawing()
        .doc_only()
        .contract(&ADD_CONTRACT)
        .output(output_schema_for::<TableResult>)
        .params(
            "sheet: id or name; kind: parts_list (at the top right; lists the component its sheet documents, else the design) or \
             revision (at the bottom left); set?: as drawing.table.edit → {table, sheet, record: as saved, warnings?}. A parts \
             list saves the numbers its view shows, if it has none saved, so what is printed doesn't move. One undo step; no \
             model is evaluated.",
        ),
    CommandSpec::new(EDIT, "Edit Table", crate::checked!("drawing.table.edit", edit))
        .icon("edit")
        .in_drawing()
        .doc_only()
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<TableResult>)
        .params(
            "table: its id; set: {sheet?, list?: {component?, levels?, group_by?, group_children?, show_purchased_contents?, mass?: \
             {unit: g|kg|lb, decimals}}, columns?, heading?: top|bottom, row_height_mm?, cap_mm?, filter?, overrides?, place?: \
             {anchor: {corner, offset_mm?}} or {at: [x, y]}}, columns, filter and overrides as a drawing saves them → as \
             drawing.table.add. One undo step, none when nothing changes.",
        ),
    CommandSpec::new(DELETE, "Delete Table", crate::checked!("drawing.table.delete", delete))
        .icon("delete")
        .in_drawing()
        .doc_only()
        .contract(&DELETE_CONTRACT)
        .output(output_schema_for::<DeleteResult>)
        .params("table: its id → {table, balloons?: those that named it, which name no list now}. A sheet's delete counts the tables on it and takes them with `contents: delete`. One undo step."),
];

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let request: AddParams = crate::schema::read(ADD, p)?;
    let record = s.drawing(ADD)?.clone();
    let sheet = sheet_id(&record, &request.sheet, ADD)?;
    let kind = match request.kind {
        KindParam::Revision => Kind::Revision,
        KindParam::PartsList => Kind::PartsList(new_list(&record, sheet)?),
    };
    let id = record.allocate_id(&mut s.ids).map_err(|e| bad(ADD, e.to_string()))?;
    let mut table = SheetTable { id, sheet, table: kind.default_table(), place: kind.default_place(), kind };
    apply(s, &record, &mut table, request.set, ADD)?;
    let list = listed(&table);
    s.edit_drawing(ADD, |d| {
        d.tables.add([Arc::new(table)]);
        Ok(())
    })?;
    answer(s, ADD, id, list)
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let request: EditParams = crate::schema::read(EDIT, p)?;
    let id = request.table.0;
    let record = s.drawing(EDIT)?.clone();
    let mut table = record.tables.by_id(id).ok_or_else(|| bad(EDIT, format!("the drawing has no table {}", ids::encode(id))))?.clone();
    let before = listed(&table);
    apply(s, &record, &mut table, request.set, EDIT)?;
    let list = listed(&table).filter(|list| Some(*list) != before);
    s.edit_drawing(EDIT, |d| {
        let at = d.tables.index_of(id).ok_or_else(|| bad(EDIT, format!("the drawing has no table {}", ids::encode(id))))?;
        if let Some(old) = d.tables.get_mut(at) {
            *old = Arc::new(table);
        }
        Ok(())
    })?;
    answer(s, EDIT, id, list)
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let request: DeleteParams = crate::schema::read(DELETE, p)?;
    let id = request.table.0;
    let balloons = s.edit_drawing(DELETE, |d| {
        d.tables.by_id(id).ok_or_else(|| bad(DELETE, format!("the drawing has no table {}", ids::encode(id))))?;
        let named = d.balloons.iter().filter(|b| b.parts_list == Some(id)).map(|b| ids::encode(b.id)).collect();
        d.remove_objects(&OnSheet { tables: vec![id], ..Default::default() });
        Ok(named)
    })?;
    serde_json::to_value(DeleteResult { table: ids::encode(id), balloons }).map_err(|e| EngineError::Other(e.to_string()))
}

/// The list a parts list on `sheet` starts with: what its sheet documents, else the design.
fn new_list(d: &Drawing, sheet: u64) -> Result<PartsList> {
    let documents = d.sheets.iter().find(|s| s.id == sheet).and_then(|s| s.documents);
    let design = || d.sources.first().map(|source| SheetComponent { source: source.id, component: 0 });
    let of = documents.or_else(design).ok_or_else(|| bad(ADD, "the drawing draws no design to list"))?;
    Ok(PartsList { of, view: Default::default(), group: Default::default(), fold: false, purchased_contents: false, mass: MassFormat::default() })
}

fn listed(table: &SheetTable) -> Option<PartsList> {
    match table.kind {
        Kind::PartsList(list) => Some(list),
        Kind::Revision => None,
    }
}

/// `set` on `table`, a record not in `d` yet, which `Drawing::validate` checks when it goes in.
fn apply(s: &Session, d: &Drawing, table: &mut SheetTable, set: TableSet, cmd: &str) -> Result<()> {
    if let Some(named) = &set.sheet {
        table.sheet = sheet_id(d, named, cmd)?;
    }
    if let Some(list) = set.list {
        let Kind::PartsList(old) = &mut table.kind else { return Err(bad(cmd, "`list` is for a parts list")) };
        apply_list(s, old, list, cmd)?;
    }
    if let Some(place) = set.place {
        table.place = serde_json::from_value(place).map_err(|e| bad(cmd, format!("`place`: {e}")))?;
    }
    table.table = merged(
        &table.table,
        [
            ("columns", set.columns),
            ("heading", set.heading),
            ("row_height_mm", set.row_height_mm),
            ("cap_mm", set.cap_mm),
            ("filter", set.filter),
            ("overrides", set.overrides),
        ],
        cmd,
    )?;
    Ok(())
}

/// `set` on a parts list, in the words it saves them in.
fn apply_list(s: &Session, list: &mut PartsList, set: ListSet, cmd: &str) -> Result<()> {
    if let Some(component) = set.component {
        let key = match &component {
            Value::String(key) => key.clone(),
            Value::Number(id) => id.to_string(),
            _ => return Err(bad(cmd, "`component` must be a component id or name")),
        };
        list.of.component = s.doc.find_component(&key).ok_or_else(|| bad(cmd, format!("no component `{key}`")))?;
    }
    *list = merged(
        list,
        [
            ("levels", set.levels),
            ("group_by", set.group_by),
            ("group_children", set.group_children),
            ("show_purchased_contents", set.show_purchased_contents),
            ("mass", set.mass),
        ],
        cmd,
    )?;
    Ok(())
}

/// `record` with `fields`, those given, in place of its own: the record's own reader judges them.
fn merged<T: serde::Serialize + serde::de::DeserializeOwned, const N: usize>(record: &T, fields: [(&str, Option<Value>); N], cmd: &str) -> Result<T> {
    let mut v = serde_json::to_value(record).map_err(|e| EngineError::Other(e.to_string()))?;
    if let Some(saved) = v.as_object_mut() {
        saved.extend(fields.into_iter().filter_map(|(key, value)| Some((key.to_string(), value?))));
    }
    serde_json::from_value(v).map_err(|e| bad(cmd, e.to_string()))
}

/// Table `id` of the open drawing, with `list`'s view numbered when it is new here.
fn answer(s: &mut Session, cmd: &str, id: u64, list: Option<PartsList>) -> Result<Value> {
    let mut warnings = Vec::new();
    if let Some(list) = list
        && let Err(why) = super::bom::keep_shown_numbers(s, &list.request())
    {
        warnings.push(format!("its item numbers can still move: {why}"));
    }
    let d = s.drawing(cmd)?;
    let table = d.tables.by_id(id).ok_or_else(|| bad(cmd, format!("the drawing has no table {}", ids::encode(id))))?;
    let reply = TableResult {
        table: ids::encode(id),
        sheet: ids::encode(table.sheet),
        record: serde_json::to_value(table).map_err(|e| EngineError::Other(e.to_string()))?,
        warnings,
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

#[cfg(test)]
#[path = "drawing_table_tests.rs"]
mod tests;
