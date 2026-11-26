//! Contracts and result types for the commands in `cmd/sketch_block.rs`.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::config::NameList;
use super::sketch_ends::SketchTarget;

/// Whether a block keeps its size: `fixed` or `free`, with the handler's message.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScaleParam(pub solidcraft_sketch::BlockScale);

/// The handler's reading of `scale`.
pub(crate) fn scale_of(v: &Value) -> Option<solidcraft_sketch::BlockScale> {
    match v.as_str()?.to_ascii_lowercase().as_str() {
        "fixed" => Some(solidcraft_sketch::BlockScale::Fixed),
        "free" => Some(solidcraft_sketch::BlockScale::Free),
        _ => None,
    }
}

/// The message for a `scale` that is neither, after its key.
pub(crate) const SCALE_MESSAGE: &str = "must be \"fixed\" or \"free\"";

impl<'de> Deserialize<'de> for ScaleParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        scale_of(&Value::deserialize(d)?).map(ScaleParam).ok_or_else(|| D::Error::custom(SCALE_MESSAGE))
    }
}

impl JsonSchema for ScaleParam {
    fn schema_name() -> Cow<'static, str> {
        "BlockScale".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "fixed: the block keeps its size (3 degrees of freedom); free: it keeps its proportions, and one length dimension sets its size (4 degrees of freedom).",
            "type": "string",
            "enum": ["fixed", "free"]
        })
    }
}

/// Make a block of sketch curves: they keep their shape relative to each other while the block
/// moves and turns as one (and scales, with scale free). A construction line, the block's axis,
/// is added: its start is the block origin. Coincident on the origin places the block; parallel,
/// perpendicular or an angle on the axis turns it. Nothing moves. The constraints wholly inside
/// the curves are held: kept, not solved, until the block is exploded.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct BlockParams {
    /// The sketch: its feature id or name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// The curves of the block, by id; a member spline brings its guide lines. Refused: curves
    /// already in a block, projected curves, text, offset outputs and pattern geometry.
    #[serde(default)]
    pub curves: Option<NameList>,
    /// fixed (default) or free.
    #[serde(default)]
    pub scale: Option<ScaleParam>,
    /// A point the block origin goes on, by id: a member point, or another point, which the
    /// origin is then made coincident with. Default: the centre of the curves' bounding box. The
    /// axis runs along sketch +X for half the box's diagonal, rounded to 1, 2 or 5 times a power
    /// of ten.
    #[serde(default)]
    pub origin: Option<String>,
    /// The block's name, at most 200 characters. Default: none (it is named by its id).
    #[serde(default)]
    pub name: Option<String>,
}

/// What `sketch.block` and `sketch.block_edit` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BlockResult {
    /// The block's id.
    pub block: String,
    /// The id of the block's axis line.
    pub axis: String,
    /// The id of the block origin, the axis's start point.
    pub origin: String,
    /// The ids of the constraints the block holds (kept, not solved).
    pub held: Vec<String>,
    /// `sketch.block_edit`: the ids of the constraints it no longer holds that did not hold
    /// where the geometry is, and were removed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dropped: Vec<String>,
    /// The sketch's degrees of freedom after the command.
    pub dof: usize,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Turn a block back into plain geometry, its axis staying as a plain construction line. Its
/// held constraints that hold where the geometry is now drive again, a free block's dimensions
/// that are plain numbers first scaled to its size; the others are removed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ExplodeBlockParams {
    /// The sketch: its feature id or name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// The block's id.
    #[serde(default)]
    pub block: Option<String>,
}

/// What `sketch.explode_block` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ExplodeBlockResult {
    /// The ids of the curves that were the block's members.
    pub curves: Vec<String>,
    /// The ids of the held constraints that drive again: they hold where the geometry is now.
    pub restored: Vec<String>,
    /// The ids of the held constraints that did not hold where the geometry is now (a turned
    /// block's horizontals, a resized block's dimensions driven by parameters), and were removed.
    pub dropped: Vec<String>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Change a block and capture its shape again from where its curves are now: its scale, its
/// name and its members. The constraints wholly inside the new members are held, except those
/// that drove while the block existed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct BlockEditParams {
    /// The sketch: its feature id or name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// The block's id.
    #[serde(default)]
    pub block: Option<String>,
    /// fixed or free. Default: as it is.
    #[serde(default)]
    pub scale: Option<ScaleParam>,
    /// The block's new name; empty text removes it. Default: as it is.
    #[serde(default)]
    pub name: Option<String>,
    /// Curves to add to the block, by id.
    #[serde(default)]
    pub add: Option<NameList>,
    /// Members to take out of the block, by id.
    #[serde(default)]
    pub remove: Option<NameList>,
}

#[cfg(test)]
#[path = "sketch_block_tests.rs"]
mod tests;
