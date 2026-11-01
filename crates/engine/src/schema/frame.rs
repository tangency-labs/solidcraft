//! Contracts for frame trim commands.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::types::{FeatureCommon, IdOrName, LengthExpr, feature_common};

/// A frame member body name, or its creating feature and sketch line.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Member {
    Body(String),
    Line { feature: IdOrName, line: String },
}

/// One member or a list of members.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Members {
    Body(String),
    List(Vec<Member>),
}

/// A profile side of a target member.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Right,
    Left,
    Top,
    Bottom,
}

/// A member, its profile side, or a plane/planar face.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Target {
    Side { member: Member, side: Option<Side> },
    Plane { plane: Value },
    SavedPlane { plane_ref: Value },
    Member(Member),
}

/// One target or a list of targets.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Targets {
    List(Vec<Target>),
    One(Target),
}

/// The end to move.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum End {
    Auto,
    Start,
    End,
}

/// Trim or extend frame members to member sides, planes or planar faces.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TrimParams {
    /// Members to rebuild, by body name or {feature, line}. Between 1 and 1000 members.
    pub members: Members,
    /// One or more member sides, {plane} picks or persisted {plane_ref} attachments.
    pub to: Targets,
    /// Which end moves. Default: auto, the end nearer each target.
    #[serde(default)]
    pub end: Option<End>,
    /// Weld gap in design length units or a length expression. Default: 0; at most 1 m.
    #[serde(default)]
    pub gap: Option<LengthExpr>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

#[cfg(test)]
#[path = "frame_tests.rs"]
mod tests;
