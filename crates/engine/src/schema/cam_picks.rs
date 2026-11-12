//! Picks: the pieces of the design a machining operation works on, stored by their persistent
//! names (`cmd/cam_picks.rs` finds them again each time the job is planned), heights that follow
//! a face, and the reply of `cam.picks.resolve`. docs/cam-jobs.md, "Picks", describes them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A piece of the design, found again by its persistent name: a face or an edge of a body (with
/// a point on it, the fallback when the name no longer finds it), or a sketch's profile.
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
    /// A sketch, by id or name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sketch: Option<String>,
    /// With `sketch`: a profile of it, by index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<usize>,
}

/// What a height follows: a picked planar face, or one of `part_top`, `part_bottom`.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum HeightSource {
    Named(String),
    Face(Pick),
}

/// A height that follows the model: `{from, offset?}`.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeightRef {
    /// A planar face square to the tool axis (a pick), or `part_top`, `part_bottom`.
    pub from: HeightSource,
    /// Added to the height it follows, mm (negative: below it). Default: 0.
    #[serde(default)]
    pub offset: Option<f64>,
}

/// A height: a number (mm, in the operation's frame), or `{from, offset?}`, a height that
/// follows a face of the model (worked out each time the job is planned).
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum HeightValue {
    Number(f64),
    From(HeightRef),
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

/// Find picks again and say what they are and which operations suit them.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamPicksResolveParams {
    /// The picks. Give these or `from_selection`.
    #[serde(default)]
    pub picks: Option<Vec<Pick>>,
    /// Take the faces, edges, sketches and profiles selected in the design instead. Default:
    /// false.
    #[serde(default)]
    pub from_selection: Option<bool>,
    /// Say how this operation type reads them (`pocket` or `contour`). Default: every type.
    #[serde(default, rename = "for")]
    pub for_type: Option<String>,
}

/// One pick as `cam.picks.resolve` read it.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct PickRead {
    /// The pick (as an operation stores it).
    pub pick: Pick,
    /// `name`, `point` or `lost`.
    pub by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub face: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge: Option<usize>,
    /// `planar`, `cylinder`, `cone`, `sphere`, `torus`, `other`, `edge`, `circle`, `sketch`.
    pub kind: String,
    /// The `cam.features` id it belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature: Option<String>,
    /// What it is, in words: "pocket 16 x 10 mm, 5 deep".
    pub reads: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// An operation that suits the picks.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct Suit {
    /// The operation's `type`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Its label, as the Manufacture workspace shows it.
    pub label: String,
    /// Why it suits.
    pub why: String,
    /// The keys the picks give the operation (its `feature`, or its `sketch` and `profiles`).
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub set: Map<String, Value>,
}

/// What `cam.picks.resolve` returns.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct CamPicksResolveReply {
    pub picks: Vec<PickRead>,
    /// The operations the picks resolve for, best first.
    pub suits: Vec<Suit>,
    /// Those the picks do not resolve for, each `<label>: <why>`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub left_out: Vec<String>,
}
