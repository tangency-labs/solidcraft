//! `drawing.symbol.define`, `.edit` and `.delete` (#13063): a drawing's symbols of its own, each a
//! shape of strokes, fills and text made once to be placed many times. The drawing's rules check
//! each definition ([`solidcraft_doc::drawing::symbol`]). Each command is one undo step of the
//! drawing, none when nothing changes, and none evaluates the model.

use std::sync::Arc;

use serde_json::Value;
use solidcraft_doc::drawing::symbol::Symbol;
use solidcraft_doc::ids;

use super::CommandSpec;
use crate::params::bad;
use crate::schema::drawing_symbol::{DefineParams, DeleteParams, DeleteResult, EditParams, EditResult, SymbolResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const DEFINE: &str = "drawing.symbol.define";
const DEFINE_CONTRACT: TypedContract<DefineParams> = TypedContract::new();
const EDIT: &str = "drawing.symbol.edit";
const EDIT_CONTRACT: TypedContract<EditParams> = TypedContract::new();
const DELETE: &str = "drawing.symbol.delete";
const DELETE_CONTRACT: TypedContract<DeleteParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new(DEFINE, "Define Symbol", crate::checked!("drawing.symbol.define", define))
        .icon("new")
        .in_drawing()
        .doc_only()
        .contract(&DEFINE_CONTRACT)
        .output(output_schema_for::<SymbolResult>)
        .params(
            "name: its own among the drawing's symbols; pieces: [{stroke: {path: [segments], closed?, line: visible|hidden|centre|thin}}, {fill: \
             {path}} or {text: {at: [x, y], content: [{text} or {field: {source: drawing|sheet|component|revision|prompt, key}}], cap_mm?: 0.5..100, \
             align?: left|centre|right}}], 1 to 1000 of them, at most 64 texts, in mm from its insertion point (at most 5000 mm from it); a segment \
             is {type: line, a, b} or {type: arc, center, radius, start, sweep}, as a drawing saves it → {symbol, record, prompts: [keys] each \
             placement answers}. A stroke's width and dashes come from the sheet's line group. One undo step; no model is evaluated.",
        ),
    CommandSpec::new(EDIT, "Edit Symbol", crate::checked!("drawing.symbol.edit", edit))
        .icon("edit")
        .in_drawing()
        .doc_only()
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<EditResult>)
        .params(
            "symbol: its id; set: {name?, pieces?: all its pieces, as drawing.symbol.define takes them} → {symbol, changed: [fields]}. One undo step, \
             none when nothing changes; no model is evaluated.",
        ),
    CommandSpec::new(DELETE, "Delete Symbol", crate::checked!("drawing.symbol.delete", delete))
        .icon("delete")
        .in_drawing()
        .doc_only()
        .contract(&DELETE_CONTRACT)
        .output(output_schema_for::<DeleteResult>)
        .params("symbol: its id → {symbol}. One undo step; no model is evaluated."),
];

fn define(s: &mut Session, p: &Value) -> Result<Value> {
    let request: DefineParams = crate::schema::read(DEFINE, p)?;
    let id = Arc::clone(s.drawing(DEFINE)?).allocate_id(&mut s.ids).map_err(|e| bad(DEFINE, e.to_string()))?;
    let symbol = Symbol { id, name: request.name, pieces: request.pieces.0 };
    let reply = answer(&symbol)?;
    s.edit_drawing(DEFINE, |d| {
        d.symbols.add([Arc::new(symbol)]);
        Ok(())
    })?;
    Ok(reply)
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let request: EditParams = crate::schema::read(EDIT, p)?;
    let id = request.symbol.0;
    let changed = s.edit_drawing(EDIT, |d| {
        let at = d.symbols.index_of(id).ok_or_else(|| missing(EDIT, id))?;
        let Some(symbol) = d.symbols.get_mut(at).map(Arc::make_mut) else { return Ok(Vec::new()) };
        let mut changed = Vec::new();
        if let Some(name) = request.set.name.filter(|name| *name != symbol.name) {
            symbol.name = name;
            changed.push("name".to_string());
        }
        if let Some(pieces) = request.set.pieces.map(|pieces| pieces.0).filter(|pieces| *pieces != symbol.pieces) {
            symbol.pieces = pieces;
            changed.push("pieces".to_string());
        }
        Ok(changed)
    })?;
    let reply = EditResult { symbol: ids::encode(id), changed, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let request: DeleteParams = crate::schema::read(DELETE, p)?;
    let id = request.symbol.0;
    s.edit_drawing(DELETE, |d| {
        d.symbols.by_id(id).ok_or_else(|| missing(DELETE, id))?;
        d.symbols.retain(|symbol| symbol.id != id);
        Ok(())
    })?;
    serde_json::to_value(DeleteResult { symbol: ids::encode(id) }).map_err(|e| EngineError::Other(e.to_string()))
}

fn missing(cmd: &str, id: u64) -> EngineError {
    bad(cmd, format!("the drawing has no symbol {}", ids::encode(id)))
}

/// `symbol` as the drawing now has it.
fn answer(symbol: &Symbol) -> Result<Value> {
    let reply = SymbolResult {
        symbol: ids::encode(symbol.id),
        record: serde_json::to_value(symbol).map_err(|e| EngineError::Other(e.to_string()))?,
        prompts: symbol.prompts().into_iter().map(str::to_string).collect(),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

#[cfg(test)]
#[path = "drawing_symbol_tests.rs"]
mod tests;
