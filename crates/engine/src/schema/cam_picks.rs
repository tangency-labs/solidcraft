//! Picks: the pieces of the design a machining operation works on, stored by their persistent
//! names (`cmd/cam_picks.rs` finds them again each time the job is planned). docs/cam-jobs.md,
//! "Picks", describes them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A piece of the design, found again by its persistent name: a face or an edge of a body (with
/// a point on it, the fallback when the name no longer finds it).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Pick {
    /// The body the face or edge is on, by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// The face's persistent name (as `model.faces` gives it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<String>,
    /// The edge's persistent name (as `model.edges` gives it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<String>,
    /// A point on the face or edge, design coordinates (mm): where it is looked for when the name
    /// finds nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<[f64; 3]>,
}

/// An operation, by its place in the job (from 0) and its name.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct OpRef {
    /// Its place in `operations`, from 0.
    pub index: usize,
    pub name: String,
}

/// How one pick of an operation was found again.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct PickResolved {
    /// The operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<OpRef>,
    /// Which pick of the operation, from 0.
    pub pick: usize,
    /// `name` (by its persistent name), `point` (by the point stored with it) or `lost`.
    pub by: String,
    /// Its body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// The face found (as `model.faces` numbers it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<usize>,
    /// The edge found (as `model.edges` numbers it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<usize>,
    /// The `cam.features` id it belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature: Option<String>,
    /// What to look at: found by position rather than name, moved since it was picked, or not
    /// found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}
