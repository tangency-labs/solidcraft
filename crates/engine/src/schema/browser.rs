//! Contracts and result types for the commands in `cmd/browser.rs`.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::types::{FeatureCommon, LengthExpr, feature_common};

/// Repair a part the modelling kernel refuses, or an open part that should be a solid: faces
/// SMLib refuses on their own are rebuilt on their own surfaces (geometry unchanged), free edges
/// that pair up within `gap` are sewn (each sewn edge carries its own gap as its tolerance; the
/// body's and the other edges' stay), and with `patch_holes` the loops of free edges left (missing
/// faces) are patched and sewn in. One undoable feature; refused, with why, when nothing applies.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RepairParams {
    /// The part (body name).
    pub body: String,
    /// The widest gap between two free edges that is sewn, more than 0 and at most 1 mm.
    /// Default: 0.2 mm.
    #[serde(default)]
    pub gap: Option<LengthExpr>,
    /// Patch each loop of free edges no pair explains (a missing face) and sew the patch in: the
    /// plane of the faces around it where they are all that plane, else a patch tangent to them
    /// (G1). Default: false (the reply offers the patches).
    #[serde(default)]
    pub patch_holes: Option<bool>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// A face rebuilt on its own surface, or one SMLib still refuses.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RepairFace {
    /// The face's index in the part's face order before the repair (from 0).
    pub face: usize,
    /// Its surface: plane, cylinder, cone, sphere, torus or freeform.
    pub surface: String,
    /// What SMLib said about it.
    pub why: String,
}

/// Two free edges sewn into one.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RepairSewn {
    /// The faces on either side, by index before the repair.
    pub faces: [usize; 2],
    /// The widest distance between the two edges, mm.
    pub gap: f64,
    /// A point halfway along the gap, [x, y, z].
    pub at: [f64; 3],
}

/// A loop of free edges no pair explains: a missing face.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RepairHole {
    /// The faces around it, by index before the repair.
    pub faces: Vec<usize>,
    /// g0 (a patch in the plane of the faces around it) or g1 (tangent to curved neighbours).
    pub continuity: String,
    /// The patch was made and sewn in.
    pub patched: bool,
    /// Why it was not, when `patch_holes` asked for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The Patch that fills it: `{command: "surface.patch", params}`.
    pub patch: Value,
}

/// What the repair did, face by face.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RepairDone {
    pub rebuilt: Vec<RepairFace>,
    /// Faces SMLib refuses that could not be rebuilt (the part may still be taken as a whole).
    pub refused: Vec<RepairFace>,
    pub sewn: Vec<RepairSewn>,
    pub holes: Vec<RepairHole>,
    /// Faces before and after.
    pub faces: [usize; 2],
    /// The largest edge tolerance before and after, mm.
    pub largest_edge_tolerance: [f64; 2],
    /// The part is a closed solid now.
    pub closed: bool,
    /// The modelling kernel takes the part now.
    pub modelling: bool,
    /// Its volume, mm³, when it is a closed solid.
    pub volume: Option<f64>,
}

/// What `body.repair` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RepairResult {
    /// The new feature's id.
    pub feature: u64,
    /// The new feature's name.
    pub name: String,
    /// Every body after the feature.
    pub bodies: Vec<String>,
    /// The bodies the feature made.
    pub created_bodies: Vec<String>,
    /// Bodies that were there before and now have other geometry.
    pub changed_bodies: Vec<String>,
    /// Bodies that are gone.
    pub removed_bodies: Vec<String>,
    /// A warning from the evaluation (a hole left open), or `null`.
    pub warning: Option<String>,
    /// How long the feature took to evaluate, in milliseconds.
    pub ms: Option<f64>,
    pub repair: RepairDone,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The units a design can show lengths in.
pub const DESIGN_UNITS: [&str; 5] = ["mm", "cm", "m", "in", "ft"];

/// One of [`DESIGN_UNITS`], with `document.units`' message for anything else.
#[derive(Clone, Debug, PartialEq)]
pub struct DesignUnits(pub String);

impl<'de> Deserialize<'de> for DesignUnits {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(u) if DESIGN_UNITS.contains(&u.as_str()) => Ok(DesignUnits(u)),
            _ => Err(D::Error::custom("must be mm, cm, m, in or ft")),
        }
    }
}

impl JsonSchema for DesignUnits {
    fn schema_name() -> Cow<'static, str> {
        "DesignUnits".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "The units the design shows lengths in.", "type": "string", "enum": DESIGN_UNITS})
    }
}

/// Change the units the design shows lengths in, and reads a bare number in. Values keep their
/// meaning: a unit-less parameter holding a bare number that lengths read is written with the
/// old unit (`10` becomes `10 mm`) when that keeps those lengths where they were. One undo
/// step; the same units again changes nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct DocumentUnitsParams {
    /// The design's new units: mm, cm, m, in or ft.
    pub units: DesignUnits,
}

/// What `document.units` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DocumentUnitsResult {
    /// The design's units now.
    pub units: String,
    /// The unit-less parameters now written with the old unit, so the lengths that read them
    /// stay where they were. Empty when nothing needed it, or the units didn't change.
    pub kept: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "browser_tests.rs"]
mod tests;
