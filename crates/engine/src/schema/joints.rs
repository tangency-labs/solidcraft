//! Contracts and result types for the commands in `cmd/joints.rs`.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::types::{AngleExpr, IdOrName, LengthExpr, present};

/// A joint origin as joint.create takes it: a joint origin's name, or an object with the snap
/// (`occurrence`, then `face`, `circle`, `edge` or `point`, or a `frame`). Kept as given, for the
/// handler, which reads it as it reads joint.create's sides.
#[derive(Clone, Debug, PartialEq)]
pub struct JointOriginParam(pub Value);

impl<'de> Deserialize<'de> for JointOriginParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            v @ (Value::String(_) | Value::Object(_)) => Ok(JointOriginParam(v)),
            _ => Err(D::Error::custom("must be a joint origin's name or an object")),
        }
    }
}

impl JsonSchema for JointOriginParam {
    fn schema_name() -> Cow<'static, str> {
        "JointOrigin".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A joint origin, as joint.create takes a side: a joint origin's name, or an object with occurrence and face, circle, edge or point (world coordinates, or a face's name), with z?, x?, end?, on?, corner?, or with a frame {origin, z, x?}.",
            "anyOf": [
                {"type": "string", "minLength": 1},
                {"type": "object"}
            ]
        })
    }
}

/// Edit a joint in place. The sides `a` and `b` take a joint origin as joint.create does; a side
/// left out keeps its place, and the two must stay on different occurrences. The joint re-solves
/// and keeps its limits, its drive and the expressions it follows; a relation's values start
/// again where its parts are when its sides, type, offset, angle or flip change.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct EditJointParams {
    /// The joint, by id or name.
    pub joint: IdOrName,
    /// Side a: a joint origin, as joint.create takes it. Default: a keeps its place.
    #[serde(default, deserialize_with = "present")]
    pub a: Option<JointOriginParam>,
    /// Side b: a joint origin, as joint.create takes it. Default: b keeps its place.
    #[serde(default, deserialize_with = "present")]
    pub b: Option<JointOriginParam>,
    /// The joint's type, as joint.create takes it (rigid, revolute, slider, ...).
    #[serde(default, rename = "type", deserialize_with = "present")]
    pub kind: Option<String>,
    /// Whether b's z lies along a's instead of against it, as joint.create's `flip`.
    #[serde(default, deserialize_with = "present")]
    pub flip: Option<bool>,
    /// The distance from a's frame to b's along z: a length, or an expression the joint follows.
    #[serde(default, deserialize_with = "present")]
    pub offset: Option<LengthExpr>,
    /// The turn about that distance: an angle, or an expression the joint follows.
    #[serde(default, deserialize_with = "present")]
    pub angle: Option<AngleExpr>,
    /// Suppress the joint (true) or bring it back (false).
    #[serde(default, deserialize_with = "present")]
    pub suppressed: Option<bool>,
    /// The joint's new name.
    #[serde(default, deserialize_with = "present")]
    pub name: Option<String>,
    /// Leave out the degrees of freedom and the fixed occurrences in the reply.
    #[serde(default, deserialize_with = "present")]
    pub quiet: Option<bool>,
}

/// A joint the solve reports: its id, name and why.
#[derive(Debug, Serialize, JsonSchema)]
pub struct JointProblem {
    pub joint: u64,
    pub name: String,
    pub why: String,
}

/// The degrees of freedom one occurrence keeps (6 free, 0 fixed).
#[derive(Debug, Serialize, JsonSchema)]
pub struct OccurrenceDof {
    pub occurrence: u64,
    pub dof: usize,
}

/// What `joint.edit` returns: the joints that disagree with the solved placement, and those the
/// solve could not apply. Unless `quiet`, also each occurrence's degrees of freedom and the
/// occurrences fixed in place.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EditJointResult {
    pub conflicts: Vec<JointProblem>,
    pub errors: Vec<JointProblem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dof: Option<Vec<OccurrenceDof>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed: Option<Vec<u64>>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Show only the joints that attach one placement, with every body outside it faded. This is a
/// view: it is not saved and is not an undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct FocusJointsParams {
    /// The placement, as occurrence ids from the top of the design. Default: leave the view.
    #[serde(default)]
    pub path: Option<Vec<u64>>,
}

/// What `joint.focus` returns: the joints shown and the bodies left solid (both empty when the
/// view is left).
#[derive(Debug, Serialize, JsonSchema)]
pub struct FocusJointsResult {
    pub joints: Vec<u64>,
    pub bodies: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "joints_tests.rs"]
mod tests;
