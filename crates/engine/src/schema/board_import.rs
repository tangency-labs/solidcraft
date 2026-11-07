//! Contract and result types for `file.insert_gerber` in `cmd/board_import.rs` (#12797).

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::check::whole;
use super::types::StringList;

/// Insert a printed circuit board from its fabrication files into a new component named after
/// the board, in the active component: the board outline layer (Gerber, RS-274X) and any drill
/// files (Excellon) become a sketch `Outline` on XY with the outline and every drilled hole and
/// routed slot, and one extrude of the board by its thickness as the body `Board`. Closed loops
/// inside the outline are cut-outs. A hole or slot that is not wholly inside the board is skipped
/// with a warning; holes may overlap. The component's properties say which file was which layer
/// (`pcb.outline`, `pcb.drills`, `pcb.<layer>`) and the thickness (`pcb.thickness`).
///
/// Copper, solder-mask and silkscreen layers (Gerber files) are optional. Each is read exactly into
/// hidden sketches in the component (`Top copper`, `Bottom mask`...: the area the layer covers,
/// top layers on the top face, bottom ones on the bottom face, as seen from above), kept for later
/// work such as machining. The board is also given each side's printed look, as an image on the
/// face (`Top print`, `Bottom print`): solder mask, copper under it, bare copper in the chosen
/// finish and the print. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct InsertGerberParams {
    /// The board outline layer, a Gerber file: its path. Give this or `outline_text`.
    #[serde(default)]
    pub outline: Option<String>,
    /// The board outline layer's Gerber text, instead of a file. Give this or `outline`.
    #[serde(default)]
    pub outline_text: Option<String>,
    /// Excellon drill files, by path: one or a list of up to 16 (plated and non-plated holes
    /// often come in separate files). Default: none.
    #[serde(default)]
    pub drills: Option<StringList>,
    /// Excellon drill files' text, instead of or as well as `drills`: one or a list of up to 16.
    #[serde(default)]
    pub drills_text: Option<StringList>,
    /// The board's thickness, in mm: more than 0, at most 20. Default: 1.6.
    #[serde(default, deserialize_with = "thickness")]
    #[schemars(schema_with = "thickness_schema")]
    pub thickness: Option<f64>,
    /// The new component's name. Default: the outline file's name without its extension, or
    /// `Board` for `outline_text`.
    #[serde(default)]
    pub name: Option<String>,
    /// Where the board goes: `file` (the default) keeps the coordinates the files give; `corner`
    /// moves the board so the lower-left corner of the box around its outline is at the origin.
    #[serde(default, deserialize_with = "origin")]
    #[schemars(schema_with = "origin_schema")]
    pub origin: Option<String>,
    /// Copper, solder-mask and silkscreen Gerber files, by path. Default: none.
    #[serde(default)]
    pub layers: Option<Layers>,
    /// The same layers' Gerber text, instead of files; a layer is given once, here or in `layers`.
    #[serde(default)]
    pub layers_text: Option<Layers>,
    /// How copper the solder mask leaves bare looks: `hasl` (tin, silver-grey; the default) or
    /// `enig` (gold).
    #[serde(default, deserialize_with = "finish")]
    #[schemars(schema_with = "finish_schema")]
    pub finish: Option<String>,
}

/// A board's image layers, each optional: a Gerber file's path (or text, in `layers_text`).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct Layers {
    /// The top copper layer.
    #[serde(default)]
    pub top_copper: Option<String>,
    /// The bottom copper layer.
    #[serde(default)]
    pub bottom_copper: Option<String>,
    /// The top solder-mask layer (what it draws is where the mask is left open).
    #[serde(default)]
    pub top_mask: Option<String>,
    /// The bottom solder-mask layer.
    #[serde(default)]
    pub bottom_mask: Option<String>,
    /// The top silkscreen (legend) layer.
    #[serde(default)]
    pub top_silk: Option<String>,
    /// The bottom silkscreen layer.
    #[serde(default)]
    pub bottom_silk: Option<String>,
}

/// `hasl` or `enig`, with the handler's message; `null` is `hasl`.
fn finish<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::String(f) if f == "hasl" || f == "enig" => Ok(Some(f)),
        _ => Err(D::Error::custom(whole("`finish` must be \"hasl\" or \"enig\""))),
    }
}

fn finish_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"enum": ["hasl", "enig", null]})
}

/// The board's thickness as the handler reads it, with its message; `null` is the default.
fn thickness<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Number(n) if n.as_f64().is_some_and(|t| t > 0.0 && t <= 20.0) => Ok(n.as_f64()),
        _ => Err(D::Error::custom(whole("`thickness` must be a number of mm, more than 0 and at most 20"))),
    }
}

fn thickness_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["number", "null"], "exclusiveMinimum": 0, "maximum": 20})
}

/// `file` or `corner`, with the handler's message; `null` is `file`.
fn origin<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::String(o) if o == "file" || o == "corner" => Ok(Some(o)),
        _ => Err(D::Error::custom(whole("`origin` must be \"file\" or \"corner\""))),
    }
}

fn origin_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"enum": ["file", "corner", null]})
}

/// What `file.insert_gerber` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BoardImportResult {
    /// The new component's id: the board's sketch, extrude and body are in it.
    pub component: u64,
    /// The new component's name: the board's name.
    pub component_name: String,
    /// The new sketch's id (`Outline`): the outline, holes and slots.
    pub sketch: u64,
    /// The extrude's id.
    pub feature: u64,
    /// The board body's name.
    pub body: String,
    /// Every body the extrude made: one per separate board in the outline, usually just one.
    pub bodies: Vec<String>,
    /// How many drilled holes the board has (vias not counted).
    pub holes: usize,
    /// How many vias (holes the drill file marks as vias, or unmarked plated ones of 0.5 mm or
    /// less). They are still holes to drill, each at its size, in the hidden sketch `Vias`; the body
    /// only shows them on its look, not cut, so it stays quick to build.
    #[serde(default)]
    pub vias: usize,
    /// How many routed slots the board has.
    pub slots: usize,
    /// How many holes and slots were left out (outside the board, across its edge, or repeated).
    pub skipped: usize,
    /// The board's thickness, in mm.
    pub thickness: f64,
    /// What was added to the files' coordinates to place the board, [x, y] in mm: [0, 0] with
    /// `origin: file`.
    pub offset: [f64; 2],
    /// The layers' hidden sketches, by id, in the order the layers were given (a large layer
    /// fills more than one).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layer_sketches: Vec<u64>,
    /// The images laid on the board for its look (`Top print`, `Bottom print`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prints: Vec<String>,
    /// What the readers and the import noticed, worded for a person (things skipped or guessed).
    pub warnings: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "board_import_tests.rs"]
mod tests;
