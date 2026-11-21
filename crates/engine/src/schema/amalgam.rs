//! Command contracts for Amalgamate (#18799), the commands of `cmd/amalgam.rs`.
use super::align::{FrameSchema, frame_problem};
use super::component_link::{configuration, path_only_required, path_required, path_schema};
use super::config::RowRef;
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

/// Place a tool at seeds on this design: the bodies an Amalgam Tag labels in another design file,
/// read now and kept in this design. At every seed the tool's Remove bodies are cut from
/// `remove_from` first, and only then are its Add bodies joined into `add_to`. Whether the
/// bodies exist and the seeds resolve stays the handler's to check.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = path_only_required)]
pub struct AmalgamateParams {
    // Optional in the type and required in the schema (`path_only_required`).
    /// The tool design (`.sc`), by file path.
    #[serde(default, deserialize_with = "path_required", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "path_schema")]
    pub path: Option<String>,
    /// The tool design's configuration row, by name or stable id. Omitted or null: its saved state.
    #[serde(default, deserialize_with = "configuration")]
    pub configuration: Option<RowRef>,
    /// The tag to place, by its feature's name in the tool design. Default: the last tag there.
    #[serde(default)]
    #[schemars(length(min = 1, max = 256))]
    pub tag: Option<String>,
    /// Where the tool goes: frames on this design, each as `solid.align`'s `from_frame` takes one,
    /// found again whenever the design changes. A seed is told apart by what it is found by (a
    /// sketch point, a face, a circle or an edge), so a bare point or frame can't be one.
    #[serde(deserialize_with = "seeds")]
    #[schemars(with = "Vec<FrameSchema>", length(min = 1, max = 256))]
    pub seeds: Vec<Value>,
    /// Quarter turns about each seed's z, the same for every copy. Default: 0.
    #[serde(default)]
    #[schemars(range(max = 3))]
    pub turns: Option<u32>,
    /// Turn every copy over, its z against the seed's. Default: false.
    #[serde(default)]
    pub flip: Option<bool>,
    /// The bodies the tool's Remove bodies are cut from: required when it has any.
    #[serde(default)]
    pub remove_from: Option<StringList>,
    /// The bodies the tool's Add bodies are joined into: required when it has any.
    #[serde(default)]
    pub add_to: Option<StringList>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: ConstructionCommon,
}

/// `seeds`: 1 to 256 frames, each checked as Align checks one.
fn seeds<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Value>, D::Error> {
    let list = match Value::deserialize(d)? {
        Value::Array(list) if !list.is_empty() && list.len() <= 256 => list,
        _ => return Err(D::Error::custom("must be a list of 1 to 256 frames")),
    };
    for (i, v) in list.iter().enumerate() {
        if let Some(problem) = frame_problem(v, &format!("seeds[{i}]")) {
            return Err(D::Error::custom(problem));
        }
    }
    Ok(list)
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
