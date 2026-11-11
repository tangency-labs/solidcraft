//! `drawing.symbol.define`, `.edit` and `.delete` (#13063): a drawing's symbols of its own, each a
//! shape of strokes, fills and text made once to be placed many times; `.place`, `.placed.edit` and
//! `.placed.delete`: where each sits, anchored as a note is. The drawing's rules check each
//! definition and placement ([`solidcraft_doc::drawing::symbol`]). Each command is one undo step of
//! the drawing, none when nothing changes, and none evaluates the model. A placement is also deleted
//! as a drawing's other objects are (`selection.delete`).

use std::sync::Arc;

use serde_json::Value;
use solidcraft_doc::drawing::OnSheet;
use solidcraft_doc::drawing::symbol::{PlacedSymbol, Symbol};
use solidcraft_doc::ids;

use super::CommandSpec;
use super::drawing_note::{anchor, check_on_view};
use crate::params::bad;
use crate::schema::drawing_symbol::{
    DefineParams, DeleteParams, DeleteResult, EditParams, EditResult, PlaceParams, PlacedDeleteParams, PlacedDeleteResult, PlacedEditParams,
    PlacedEditResult, PlacedResult, SymbolResult,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const DEFINE: &str = "drawing.symbol.define";
const DEFINE_CONTRACT: TypedContract<DefineParams> = TypedContract::new();
const EDIT: &str = "drawing.symbol.edit";
const EDIT_CONTRACT: TypedContract<EditParams> = TypedContract::new();
const DELETE: &str = "drawing.symbol.delete";
const DELETE_CONTRACT: TypedContract<DeleteParams> = TypedContract::new();
const PLACE: &str = "drawing.symbol.place";
const PLACE_CONTRACT: TypedContract<PlaceParams> = TypedContract::new();
const PLACED_EDIT: &str = "drawing.symbol.placed.edit";
const PLACED_EDIT_CONTRACT: TypedContract<PlacedEditParams> = TypedContract::new();
const PLACED_DELETE: &str = "drawing.symbol.placed.delete";
const PLACED_DELETE_CONTRACT: TypedContract<PlacedDeleteParams> = TypedContract::new();

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
        .params(
            "symbol: its id; placements?: delete, to delete its placements with it (a placed symbol is refused without it, naming them) → \
             {symbol, placements: [the ids that went]}. One undo step; no model is evaluated.",
        ),
    CommandSpec::new(PLACE, "Place Symbol", crate::checked!("drawing.symbol.place", place))
        .icon("new")
        .in_drawing()
        .doc_only()
        .contract(&PLACE_CONTRACT)
        .output(output_schema_for::<PlacedResult>)
        .params(
            "symbol: its id; anchor: as drawing.note.add takes it ({sheet}, {view}, {edge: {view, edge}} or {corner: {view, corner}}); at: [x, y], \
             where its insertion point sits on paper in mm, from the sheet's lower-left corner or the view's centre; scale?: 0.01..100 (1); \
             angle?: degrees counter-clockwise about its insertion point (0); answers?: {prompt: text}, each one of its prompts, one line → \
             {placement, record (its angle in radians)}. Placed by id: a changed definition changes every placement. One undo step; no model is \
             evaluated.",
        ),
    CommandSpec::new(PLACED_EDIT, "Edit Placed Symbol", crate::checked!("drawing.symbol.placed.edit", placed_edit))
        .icon("edit")
        .in_drawing()
        .doc_only()
        .contract(&PLACED_EDIT_CONTRACT)
        .output(output_schema_for::<PlacedEditResult>)
        .params(
            "placement: its id; set: {at?: [x, y], scale?, angle?: degrees, answers?: all its answers} → {placement, changed: [fields]}. What it \
             places and belongs to stays: to change them, place it again. One undo step, none when nothing changes; no model is evaluated.",
        ),
    CommandSpec::new(PLACED_DELETE, "Delete Placed Symbol", crate::checked!("drawing.symbol.placed.delete", placed_delete))
        .icon("delete")
        .in_drawing()
        .doc_only()
        .contract(&PLACED_DELETE_CONTRACT)
        .output(output_schema_for::<PlacedDeleteResult>)
        .params("placement: its id → {placement}. Its symbol stays. One undo step; no model is evaluated."),
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
    let placements = s.edit_drawing(DELETE, |d| {
        d.symbols.by_id(id).ok_or_else(|| missing(DELETE, id))?;
        let placements = d.placements_of(id);
        if !placements.is_empty() && request.placements.is_none() {
            let named = placements.iter().map(|p| ids::encode(*p)).collect::<Vec<_>>().join(", ");
            return Err(bad(DELETE, format!("it is placed as {named}: give `\"placements\": \"delete\"` to delete them too")));
        }
        d.remove_objects(&OnSheet { placed_symbols: placements.clone(), ..OnSheet::default() });
        d.symbols.retain(|symbol| symbol.id != id);
        Ok(placements)
    })?;
    let placements = placements.iter().map(|p| ids::encode(*p)).collect();
    serde_json::to_value(DeleteResult { symbol: ids::encode(id), placements }).map_err(|e| EngineError::Other(e.to_string()))
}

fn place(s: &mut Session, p: &Value) -> Result<Value> {
    let request: PlaceParams = crate::schema::read(PLACE, p)?;
    let record = s.drawing(PLACE)?.clone();
    record.symbols.by_id(request.symbol.0).ok_or_else(|| missing(PLACE, request.symbol.0))?;
    let anchor = anchor(s, &record, request.anchor, PLACE)?;
    check_on_view(&record, &anchor, anchor.path(), PLACE)?;
    let id = record.allocate_id(&mut s.ids).map_err(|e| bad(PLACE, e.to_string()))?;
    let placed = PlacedSymbol {
        id,
        symbol: request.symbol.0,
        anchor,
        at: request.at,
        scale: request.scale,
        angle: request.angle.to_radians(),
        answers: request.answers,
    };
    let reply = placed_answer(&placed)?;
    s.edit_drawing(PLACE, |d| {
        d.placed_symbols.add([Arc::new(placed)]);
        Ok(())
    })?;
    Ok(reply)
}

fn placed_edit(s: &mut Session, p: &Value) -> Result<Value> {
    let request: PlacedEditParams = crate::schema::read(PLACED_EDIT, p)?;
    let id = request.placement.0;
    let set = request.set;
    let changed = s.edit_drawing(PLACED_EDIT, |d| {
        let at = d.placed_symbols.index_of(id).ok_or_else(|| placement_missing(PLACED_EDIT, id))?;
        let Some(placed) = d.placed_symbols.get_mut(at).map(Arc::make_mut) else { return Ok(Vec::new()) };
        let mut changed = Vec::new();
        if let Some(place) = set.at.filter(|place| *place != placed.at) {
            placed.at = place;
            changed.push("at".to_string());
        }
        if let Some(scale) = set.scale.filter(|scale| *scale != placed.scale) {
            placed.scale = scale;
            changed.push("scale".to_string());
        }
        if let Some(angle) = set.angle.map(f64::to_radians).filter(|angle| *angle != placed.angle) {
            placed.angle = angle;
            changed.push("angle".to_string());
        }
        if let Some(answers) = set.answers.filter(|answers| *answers != placed.answers) {
            placed.answers = answers;
            changed.push("answers".to_string());
        }
        Ok(changed)
    })?;
    let reply = PlacedEditResult { placement: ids::encode(id), changed, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

fn placed_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let request: PlacedDeleteParams = crate::schema::read(PLACED_DELETE, p)?;
    let id = request.placement.0;
    s.edit_drawing(PLACED_DELETE, |d| {
        d.placed_symbols.by_id(id).ok_or_else(|| placement_missing(PLACED_DELETE, id))?;
        d.remove_objects(&OnSheet { placed_symbols: vec![id], ..OnSheet::default() });
        Ok(())
    })?;
    serde_json::to_value(PlacedDeleteResult { placement: ids::encode(id) }).map_err(|e| EngineError::Other(e.to_string()))
}

fn missing(cmd: &str, id: u64) -> EngineError {
    bad(cmd, format!("the drawing has no symbol {}", ids::encode(id)))
}

fn placement_missing(cmd: &str, id: u64) -> EngineError {
    bad(cmd, format!("the drawing has no placed symbol {}", ids::encode(id)))
}

/// `placed` as the drawing now has it.
fn placed_answer(placed: &PlacedSymbol) -> Result<Value> {
    let record = serde_json::to_value(placed).map_err(|e| EngineError::Other(e.to_string()))?;
    let reply = PlacedResult { placement: ids::encode(placed.id), record, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
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
