//! Command contracts for Amalgamate (#18799), the commands of `cmd/amalgam.rs`.
use super::align::{FrameSchema, frame_problem};
use super::{ConstructionCommon, StringList, feature_common};
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// Label bodies of this design as a tool that other designs place with Amalgamate. At least one
/// body in all; a body has one role. Whether each body exists, and that Remove and Add bodies
/// are closed solids, stays the feature's to check. The tag's name is the feature's `name`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AmalgamTagParams {
    /// Bodies cut from the bodies an Amalgamate names for Remove, before any Add is joined.
    /// Closed solids.
    #[serde(default)]
    pub remove: Option<StringList>,
    /// Bodies joined into the bodies an Amalgamate names for Add, after every Remove is cut.
    /// Closed solids.
    #[serde(default)]
    pub add: Option<StringList>,
    /// Bodies brought in as a part of their own.
    #[serde(default)]
    pub new: Option<StringList>,
    /// The tool's origin, which goes onto each place the tool is placed, its z on the place's z:
    /// a frame on this design, as `solid.align`'s `from_frame` takes one. Default: the design's
    /// origin.
    #[serde(default, deserialize_with = "seat")]
    #[schemars(with = "Option<FrameSchema>")]
    pub seat: Option<Value>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: ConstructionCommon,
}

/// `seat`: a frame, checked as Align checks one, or `null` (no seat).
fn seat<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v => frame_problem(&v, "seat").map_or(Ok(Some(v)), |problem| Err(D::Error::custom(problem))),
    }
}

#[cfg(test)]
#[path = "amalgam_tests.rs"]
mod tests;
