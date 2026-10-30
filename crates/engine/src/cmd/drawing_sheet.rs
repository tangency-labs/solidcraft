//! A drawing's sheets (#3875, W18 S4): add, change, reorder and delete them, each one undo step of
//! the drawing (none when nothing changes). A sheet is named by its id or, when only one has it,
//! its name.
//!
//! A new sheet takes the paper, border, title block and resolution of the sheet it follows, so a
//! drawing's sheets match unless told otherwise. Changing a sheet's paper regrows its border and zones,
//! which nothing on it is saved against, and moves nothing (S5): the answer lists the sheet's views
//! whose boundary the new paper leaves outside its drawing space. A sheet with anything on it is
//! deleted only with what is on it, when asked, in the same undo step.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;
use solidcraft_doc::drawing::fields::overridable;
use solidcraft_doc::drawing::view::ViewKind;
use solidcraft_doc::drawing::{Drawing, Excluded, OnSheet, Sheet, SheetComponent};
use solidcraft_doc::ids;
use solidcraft_geom::drawing_sheet::{Orientation, SheetSize};
use solidcraft_geom::title_block::TitleBlock;

use super::CommandSpec;
use crate::params::{bad, num, str_};
use crate::schema::drawing_sheet::{SheetAddParams, SheetDeleteParams, SheetName, SheetReorderParams, SheetSetParams, SheetsResult};
use crate::schema::drawing_view::OutsideResult;
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const ADD: TypedContract<SheetAddParams> = TypedContract::new();
const SET: TypedContract<SheetSetParams> = TypedContract::new();
const REORDER: TypedContract<SheetReorderParams> = TypedContract::new();
const DELETE: TypedContract<SheetDeleteParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("drawing.sheet.add", "Add Sheet", crate::checked!("drawing.sheet.add", add))
        .icon("new")
        .in_drawing()
        .doc_only()
        .contract(&ADD)
        .output(output_schema_for::<SheetsResult>)
        .params(
            "after?: a sheet (id or name; default the last); name?; size?: {standard: a3} or {custom: {sides_mm: [w, h]}}; \
             orientation?: landscape|portrait; title_block?: a definition or null; dpi?: 25..2400 (each defaulting to the sheet it              follows); documents?: {source: id, \
             component: id (0 the design)}; excluded?: {count?, print?}; title_block_overrides?: {cell: text} (none of these unless given) → sheet: the new sheet's id, sheets. \
             One undo step.",
        ),
    CommandSpec::new("drawing.sheet.set", "Sheet Settings", crate::checked!("drawing.sheet.set", set))
        .icon("settings")
        .in_drawing()
        .doc_only()
        .contract(&SET)
        .output(output_schema_for::<SheetsResult>)
        .params(
            "sheet: id or name; name?, size?, orientation?, title_block?: a definition or null (a new block keeps the overrides of the text cells it still has), dpi?; \
             documents?: {source: id, component: id (0 the design)} or null; excluded?: {count?, print?}; title_block_overrides?: {cell: text, shown in place of \
             its value, or null to drop it} or null to drop them all → the drawing's sheets, and after a change of paper or title block `outside`: the sheet's views whose \
             boundary leaves its drawing space (none is moved). One undo step, none when nothing changes.",
        ),
    CommandSpec::new("drawing.sheet.reorder", "Reorder Sheets", crate::checked!("drawing.sheet.reorder", reorder))
        .icon("reorder")
        .in_drawing()
        .doc_only()
        .contract(&REORDER)
        .output(output_schema_for::<SheetsResult>)
        .params("sheets: every sheet's id, in the new order → the drawing's sheets. One undo step, none for the same order."),
    CommandSpec::new("drawing.sheet.delete", "Delete Sheet", crate::checked!("drawing.sheet.delete", delete))
        .icon("delete")
        .in_drawing()
        .doc_only()
        .contract(&DELETE)
        .output(output_schema_for::<SheetsResult>)
        .params(
            "sheet: id or name (a drawing keeps at least one); contents?: delete, to delete the views on it, their dimensions and \
             balloons, and the images on it, with it (without it a sheet with anything on it is refused, naming what) → the drawing's sheets. One undo step.",
        ),
];

/// The drawing's sheets, as each command answers (`sheet`: the one `drawing.sheet.add` made;
/// `outside`: the views `drawing.sheet.set`'s paper leaves outside).
fn answer(s: &Session, cmd: &str, sheet: Option<u64>, outside: Vec<OutsideResult>) -> Result<Value> {
    let sheets = s.drawing(cmd)?.sheets.iter().map(|sheet| SheetName { id: ids::encode(sheet.id), name: sheet.name.clone() }).collect();
    let reply = SheetsResult { sheet: sheet.map(ids::encode), sheets, outside, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// At most this many ids of a kind in a message.
const NAMED: usize = 8;

/// Ids for a message: the first [`NAMED`], then how many more.
fn named(list: &[u64]) -> String {
    let mut named: Vec<String> = list.iter().take(NAMED).map(|id| format!("`{}`", ids::encode(*id))).collect();
    if list.len() > NAMED {
        named.push(format!("{} more", list.len() - NAMED));
    }
    named.join(", ")
}

/// What is on a sheet, for a message: `2 views (…), 1 dimension (…) and 3 balloons (…)`.
fn listed(on: &OnSheet) -> String {
    let kind = |what: &str, list: &[u64]| {
        let s = if list.len() == 1 { "" } else { "s" };
        (!list.is_empty()).then(|| format!("{} {what}{s} ({})", list.len(), named(list)))
    };
    let mut kinds: Vec<String> =
        [kind("view", &on.views), kind("dimension", &on.dimensions), kind("balloon", &on.balloons), kind("image", &on.images)]
            .into_iter()
            .flatten()
            .collect();
    let last = kinds.pop().unwrap_or_default();
    if kinds.is_empty() { last } else { format!("{} and {last}", kinds.join(", ")) }
}

/// The position of the sheet `p[key]` names ([`Drawing::sheet_index`]).
fn sheet_at(cmd: &str, d: &Drawing, p: &Value, key: &str) -> Result<usize> {
    let named = str_(p, key).ok_or_else(|| bad(cmd, format!("`{key}` must be a sheet's id or name")))?;
    d.sheet_index(named).map_err(|e| bad(cmd, e.to_string()))
}

/// The settings `p` gives a sheet, over the ones it has.
fn apply(cmd: &str, sheet: &mut Sheet, p: &Value) -> Result<()> {
    if let Some(name) = p.get("name") {
        sheet.name = name.as_str().ok_or_else(|| bad(cmd, "`name` must be text"))?.to_string();
    }
    if let Some(size) = p.get("size") {
        sheet.sheet.size = serde_json::from_value::<SheetSize>(size.clone())
            .map_err(|_| bad(cmd, "`size` is {\"standard\": \"a3\"} or {\"custom\": {\"sides_mm\": [w, h]}}"))?;
    }
    if let Some(orientation) = p.get("orientation") {
        sheet.sheet.orientation =
            serde_json::from_value::<Orientation>(orientation.clone()).map_err(|_| bad(cmd, "`orientation` is landscape or portrait"))?;
    }
    match p.get("title_block") {
        None => {}
        Some(Value::Null) => sheet.title_block = None,
        Some(block) => {
            let block = serde_json::from_value::<TitleBlock>(block.clone()).map_err(|e| bad(cmd, format!("`title_block`: {e}")))?;
            sheet.title_block = Some(block);
        }
    }
    if p.get("title_block").is_some() {
        let block = sheet.title_block.clone();
        sheet.title_block_overrides.retain(|name, _| overridable(block.as_ref(), name));
    }
    match p.get("title_block_overrides") {
        None => {}
        Some(Value::Null) => sheet.title_block_overrides.clear(),
        Some(Value::Object(cells)) => {
            for (cell, text) in cells {
                match text {
                    Value::Null => sheet.title_block_overrides.remove(cell),
                    Value::String(text) => sheet.title_block_overrides.insert(cell.clone(), text.clone()),
                    _ => return Err(bad(cmd, format!("`title_block_overrides.{cell}` is text or null"))),
                };
            }
        }
        Some(_) => return Err(bad(cmd, "`title_block_overrides` is {cell: text or null} or null")),
    }
    if p.get("dpi").is_some() {
        sheet.dpi = num(p, "dpi").ok_or_else(|| bad(cmd, "`dpi` must be a number"))?;
    }
    match p.get("documents") {
        None => {}
        Some(Value::Null) => sheet.documents = None,
        Some(documents) => {
            let source = str_(documents, "source").and_then(ids::decode).ok_or_else(|| bad(cmd, "`documents.source` must be a source's id"))?;
            let component =
                documents.get("component").map_or(Some(0), Value::as_u64).ok_or_else(|| bad(cmd, "`documents.component` must be a component id"))?;
            sheet.documents = Some(SheetComponent { source, component });
        }
    }
    if let Some(excluded) = p.get("excluded") {
        // An object of the two flags: anything else (a mistyped key) would change nothing unseen.
        let shape = || bad(cmd, "`excluded` is {count?: true|false, print?: true|false}");
        let excluded = excluded.as_object().filter(|o| o.keys().all(|k| matches!(k.as_str(), "count" | "print"))).ok_or_else(shape)?;
        let flag = |key: &str, was: bool| match excluded.get(key) {
            None => Ok(was),
            Some(Value::Bool(b)) => Ok(*b),
            Some(_) => Err(bad(cmd, format!("`excluded.{key}` is true or false"))),
        };
        sheet.excluded = Excluded { count: flag("count", sheet.excluded.count)?, print: flag("print", sheet.excluded.print)? };
    }
    Ok(())
}

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.sheet.add";
    let record = s.drawing(cmd)?.clone();
    let id = record.allocate_id(&mut s.ids).map_err(|e| bad(cmd, e.to_string()))?;
    s.edit_drawing(cmd, |d| {
        let after = if p.get("after").is_some() { sheet_at(cmd, d, p, "after")? } else { d.sheets.len().saturating_sub(1) };
        let model = d.sheets.get(after).ok_or_else(|| bad(cmd, "the drawing has no sheet to follow"))?;
        let free = (d.sheets.len() + 1..).map(|n| format!("Sheet {n}")).find(|name| d.sheets.iter().all(|s| s.name != *name)).unwrap_or_default();
        let mut sheet =
            Sheet { id, name: free, documents: None, excluded: Excluded::default(), title_block_overrides: BTreeMap::new(), ..(**model).clone() };
        apply(cmd, &mut sheet, p)?;
        d.sheets.insert(after + 1, Arc::new(sheet));
        Ok(())
    })?;
    answer(s, cmd, Some(id), Vec::new())
}

fn set(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.sheet.set";
    let (id, room) = s.edit_drawing(cmd, |d| {
        let at = sheet_at(cmd, d, p, "sheet")?;
        let sheet = Arc::make_mut(d.sheets.get_mut(at).ok_or_else(|| bad(cmd, "no such sheet"))?);
        let was = (sheet.sheet, sheet.title_block.clone());
        apply(cmd, sheet, p)?;
        Ok((sheet.id, (sheet.sheet, sheet.title_block.clone()) != was))
    })?;
    // Only a change of paper or block changes the room views have; each view measured once.
    let d = s.drawing(cmd)?;
    let on: Vec<u64> = if room { d.on_sheet(id).views } else { Vec::new() };
    let outside = super::drawing_view::outside_results(crate::drawing_view::outside(s, d, &on, cmd)?);
    answer(s, cmd, None, outside)
}

fn reorder(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.sheet.reorder";
    s.edit_drawing(cmd, |d| {
        let every = || bad(cmd, "`sheets` must list every sheet's id once");
        let order: Vec<u64> = p
            .get("sheets")
            .and_then(Value::as_array)
            .ok_or_else(every)?
            .iter()
            .map(|v| v.as_str().and_then(ids::decode))
            .collect::<Option<_>>()
            .ok_or_else(every)?;
        let mut sheets = Vec::with_capacity(order.len());
        for id in &order {
            let at = d.sheets.iter().position(|s| s.id == *id).ok_or_else(every)?;
            sheets.push(d.sheets.remove(at));
        }
        if !d.sheets.is_empty() {
            return Err(every());
        }
        d.sheets = sheets;
        Ok(())
    })?;
    answer(s, cmd, None, Vec::new())
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.sheet.delete";
    let with_contents = match p.get("contents") {
        None => false,
        Some(v) if v.as_str() == Some("delete") => true,
        Some(_) => return Err(bad(cmd, "`contents` is delete: the sheet goes with everything on it")),
    };
    s.edit_drawing(cmd, |d| {
        let at = sheet_at(cmd, d, p, "sheet")?;
        if d.sheets.len() <= 1 {
            return Err(bad(cmd, "a drawing keeps at least one sheet"));
        }
        let Some(sheet) = d.sheets.get(at).cloned() else { return Ok(()) };
        let on = d.on_sheet(sheet.id);
        if !on.is_empty() && !with_contents {
            let what = listed(&on);
            return Err(bad(cmd, format!("sheet `{}` has {what} on it: give `\"contents\": \"delete\"` to delete them with it", sheet.name)));
        }
        // Nothing on another sheet goes with it: a view there projected from one of its views would
        // be left without its parent.
        let elsewhere: Vec<u64> = d
            .views
            .iter()
            .filter(|v| v.sheet != sheet.id && matches!(v.kind, ViewKind::Projected { parent, .. } if on.views.binary_search(&parent).is_ok()))
            .map(|v| v.id)
            .collect();
        if !elsewhere.is_empty() {
            let views = named(&elsewhere);
            return Err(bad(
                cmd,
                format!("sheet `{}`'s views have views projected from them on other sheets ({views}): move or delete those first", sheet.name),
            ));
        }
        d.remove_sheet(sheet.id);
        Ok(())
    })?;
    answer(s, cmd, None, Vec::new())
}

#[cfg(test)]
#[path = "drawing_sheet_tests.rs"]
mod tests;
