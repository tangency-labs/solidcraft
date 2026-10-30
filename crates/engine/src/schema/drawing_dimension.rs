//! Contracts and result types for the commands in `cmd/drawing_dimension.rs` (#4896, W19d).
//!
//! What a dimension measures is given as a command picks it (a point, or an edge's index) and
//! saved by name ([`solidcraft_doc::Document::edge_reference`]); its kind, direction, reading and
//! text place are read by the saved record's own types, so the command and a file read the same
//! words.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use solidcraft_doc::MAX_PLACEMENT_DEPTH;
use solidcraft_doc::drawing::dimension::{CornerPick, EdgePick, TextPlace};
use solidcraft_geom::drawing_measure::{LinearDirection, Reading, SilhouetteSide};

use super::drawing_view::ViewIdParam;
use crate::params;

/// The most edges a corner given by its edges may list before it is refused unread.
const MAX_LISTED_EDGES: usize = 64;

const EDGE: &str = "an edge is a point on it [x, y, z], or {point} or {body, index}, with path?";
const FACE: &str = "a face is a point on it [x, y, z], or {point} or {body, index}, with path?";
const CORNER: &str = "a corner is a point [x, y, z], or {point} or {edges: [edges]} or {centre: a circle or an arc}, with path?";

/// An edge to measure: what picks it, and the occurrences its body is placed through.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeParam {
    pub pick: EdgePick,
    pub path: Vec<u64>,
}

/// A corner to measure, as [`EdgeParam`] an edge.
#[derive(Clone, Debug, PartialEq)]
pub struct CornerParam {
    pub pick: CornerPick,
    pub path: Vec<u64>,
}

/// A face to measure, picked as [`EdgeParam`] picks an edge (its index among the body's faces).
#[derive(Clone, Debug, PartialEq)]
pub struct FaceParam {
    pub pick: EdgePick,
    pub path: Vec<u64>,
}

/// One of a cylinder's silhouettes to measure from: a face on the cylinder, and which silhouette.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct SilhouetteParam {
    pub face: FaceParam,
    /// `near` (the one nearer the other cylinder on the view) or `far`.
    #[schemars(with = "String")]
    pub side: SilhouetteSide,
}

/// An object's keys, each one of `allowed`.
fn keys<'a>(v: &'a Value, allowed: &[&str], shape: &str) -> Result<&'a Map<String, Value>, String> {
    v.as_object().filter(|o| o.keys().all(|k| allowed.contains(&k.as_str()))).ok_or_else(|| shape.to_string())
}

/// The occurrences a body is placed through, from the top: none when not given.
fn path(v: Option<&Value>) -> Result<Vec<u64>, String> {
    let shape = || format!("`path` is the ids of at most {MAX_PLACEMENT_DEPTH} occurrences, from the top");
    let Some(v) = v else { return Ok(Vec::new()) };
    let list = v.as_array().filter(|l| l.len() <= MAX_PLACEMENT_DEPTH).ok_or_else(shape)?;
    list.iter().map(|id| id.as_u64().filter(|id| *id != 0)).collect::<Option<_>>().ok_or_else(shape)
}

/// An edge or a face (`what`) as a pick, without its path; `shape` says how one is given.
fn pick(v: &Value, what: &str, shape: &str) -> Result<EdgePick, String> {
    if let Some(p) = params::vec3(v) {
        return Ok(EdgePick::Point(p));
    }
    let o = keys(v, &["point", "body", "index", "path"], shape)?;
    match (o.get("point"), o.get("body"), o.get("index")) {
        (Some(p), None, None) => params::vec3(p).map(EdgePick::Point).ok_or_else(|| shape.to_string()),
        (None, Some(body), Some(index)) => {
            let body = body.as_str().ok_or_else(|| format!("{what}'s `body` is its name"))?.to_string();
            let index = index.as_u64().and_then(|i| usize::try_from(i).ok()).ok_or_else(|| format!("{what}'s `index` is a whole number"))?;
            Ok(EdgePick::Index { body, index })
        }
        _ => Err(shape.to_string()),
    }
}

/// An edge as a pick, without its path.
fn edge_pick(v: &Value) -> Result<EdgePick, String> {
    pick(v, "an edge", EDGE)
}

fn edge_param(v: &Value) -> Result<EdgeParam, String> {
    let path = path(v.get("path"))?;
    Ok(EdgeParam { pick: edge_pick(v)?, path })
}

fn corner_param(v: &Value) -> Result<CornerParam, String> {
    if let Some(p) = params::vec3(v) {
        return Ok(CornerParam { pick: CornerPick::Point(p), path: Vec::new() });
    }
    let o = keys(v, &["point", "edges", "centre", "path"], CORNER)?;
    let path = path(o.get("path"))?;
    let pick = match (o.get("point"), o.get("edges"), o.get("centre")) {
        (Some(p), None, None) => CornerPick::Point(params::vec3(p).ok_or(CORNER)?),
        // The centre's edge is on the point's body, so it has no path of its own.
        (None, None, Some(edge)) if edge.get("path").is_none() => CornerPick::Centre(edge_pick(edge)?),
        (None, Some(edges), None) => {
            let listed = || format!("a corner's `edges` lists 2 to {MAX_LISTED_EDGES} edges, each without a path of its own");
            let edges = edges.as_array().filter(|l| l.len() <= MAX_LISTED_EDGES).ok_or_else(listed)?;
            let own_path = edges.iter().any(|e| e.get("path").is_some());
            if own_path {
                return Err(listed());
            }
            CornerPick::Edges(edges.iter().map(edge_pick).collect::<Result<_, _>>()?)
        }
        _ => return Err(CORNER.to_string()),
    };
    Ok(CornerParam { pick, path })
}

impl<'de> Deserialize<'de> for EdgeParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        edge_param(&Value::deserialize(d)?).map_err(D::Error::custom)
    }
}

impl<'de> Deserialize<'de> for CornerParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        corner_param(&Value::deserialize(d)?).map_err(D::Error::custom)
    }
}

impl<'de> Deserialize<'de> for FaceParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let path = path(v.get("path")).map_err(D::Error::custom)?;
        Ok(FaceParam { pick: pick(&v, "a face", FACE).map_err(D::Error::custom)?, path })
    }
}

impl JsonSchema for EdgeParam {
    fn schema_name() -> Cow<'static, str> {
        "DrawingDimensionEdge".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "An edge: a point on it [x, y, z] in the design's frame, or {point} or {body, index} (its index among the body's edges as the model lists them), with path?: the ids of the occurrences its body is placed through, from the top.",
            "anyOf": [{"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}, {"type": "object"}]
        })
    }
}

impl JsonSchema for FaceParam {
    fn schema_name() -> Cow<'static, str> {
        "DrawingDimensionFace".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A face: a point on it [x, y, z] in the design's frame, or {point} or {body, index} (its index among the body's faces as the model lists them), with path? as an edge's.",
            "anyOf": [{"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}, {"type": "object"}]
        })
    }
}

impl JsonSchema for CornerParam {
    fn schema_name() -> Cow<'static, str> {
        "DrawingDimensionCorner".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A corner: a point [x, y, z] in the design's frame, or {point} or {edges: [2 to 16 edges that meet there]}, or {centre: an edge that is a circle or an arc} for its centre (a hole's), with path? as an edge's.",
            "anyOf": [{"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}, {"type": "object"}]
        })
    }
}

/// A dimension, by its id.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DimensionIdParam(pub u64);

impl<'de> Deserialize<'de> for DimensionIdParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) => solidcraft_doc::ids::decode(&s).map(DimensionIdParam).ok_or_else(|| D::Error::custom("must be a dimension's id")),
            _ => Err(D::Error::custom("must be a dimension's id")),
        }
    }
}

impl JsonSchema for DimensionIdParam {
    fn schema_name() -> Cow<'static, str> {
        "DrawingDimensionId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A dimension's id: 13 characters of lowercase Crockford base32.", "type": "string", "pattern": "^[0-9a-hjkmnp-tv-z]{13}$"})
    }
}

/// What a dimension measures, as `drawing.dimension.add` takes it: the saved record's kinds, with
/// picks for its names.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum KindParam {
    /// The distance between two corners.
    Linear {
        a: CornerParam,
        b: CornerParam,
        /// `aligned` (the default), `horizontal`, `vertical` or `{along: [x, y]}` on the view.
        // `skip_serializing_if` keeps schemars from writing `default: null`, which these fields' types refuse.
        #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
        #[schemars(with = "Value")]
        direction: Option<LinearDirection>,
        /// `projected` (the default) or `true` (measured aligned).
        #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
        #[schemars(with = "String")]
        reading: Option<Reading>,
    },
    /// A straight edge's length.
    Length {
        edge: EdgeParam,
        #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
        #[schemars(with = "Value")]
        direction: Option<LinearDirection>,
        #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
        #[schemars(with = "String")]
        reading: Option<Reading>,
    },
    /// The angle between two straight edges' lines.
    Angle {
        a: EdgeParam,
        b: EdgeParam,
        #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
        #[schemars(with = "String")]
        reading: Option<Reading>,
        /// π minus the smaller angle.
        #[serde(default)]
        other_side: bool,
    },
    /// A circle's or an arc's radius, on a view that sees it face on.
    Radial {
        edge: EdgeParam,
        /// Its diameter instead. By default, for a whole circle or an arc of half a turn or more.
        #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
        #[schemars(with = "bool")]
        diameter: Option<bool>,
    },
    /// A whole cylinder's diameter between its two silhouettes, on a view that doesn't look along
    /// its axis.
    Cylinder {
        /// A face on the cylinder.
        face: FaceParam,
    },
    /// The distance between a silhouette of one cylinder and a silhouette of another, as the view
    /// shows them; two of one cylinder make its `cylinder` dimension.
    Silhouettes {
        a: SilhouetteParam,
        b: SilhouetteParam,
        /// As a linear dimension's; `aligned` (the default) is square to the silhouettes.
        #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
        #[schemars(with = "Value")]
        direction: Option<LinearDirection>,
    },
    /// The distances from each point to the next, in order along one line as the view shows them.
    Chain {
        /// 3 to 64 corners.
        points: Vec<CornerParam>,
        /// As a linear dimension's; `aligned` (the default) is from the first point to the last.
        #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
        #[schemars(with = "Value")]
        direction: Option<LinearDirection>,
    },
}

/// What `drawing.dimension.add` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct AddParams {
    pub view: ViewIdParam,
    pub kind: KindParam,
    /// The dimension line's distance on paper from what it measures, in millimetres; the sign says
    /// on which side.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub offset_mm: Option<f64>,
    /// `centred` on the dimension line (the default), or `{at: [x, y]}` on paper from the view's
    /// centre.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Value")]
    pub text: Option<TextPlace>,
    /// 0 to 6.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u8")]
    pub decimals: Option<u8>,
    /// Shown instead of the number, which is still worked out.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub override_text: Option<String>,
}

/// What `drawing.dimension.inspect` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct InspectParams {
    pub dimension: DimensionIdParam,
}

/// What `drawing.dimension.edit` takes: the dimension, and what to change.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct EditParams {
    pub dimension: DimensionIdParam,
    pub set: DimensionSet,
}

/// What `drawing.dimension.edit` changes, each only when given. What a dimension measures isn't
/// one of them: to measure something else, add a new one.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct DimensionSet {
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub offset_mm: Option<f64>,
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Value")]
    pub text: Option<TextPlace>,
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u8")]
    pub decimals: Option<u8>,
    /// Text shown instead of the number, or `null` to show the number again.
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    pub override_text: Option<Option<String>>,
    /// A linear, length or chain dimension's.
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Value")]
    pub direction: Option<LinearDirection>,
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub reading: Option<Reading>,
    /// An angle's: π minus the smaller angle.
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub other_side: Option<bool>,
    /// A radial dimension's: its diameter, or its radius.
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub diameter: Option<bool>,
}

/// What `drawing.dimension.edit` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EditResult {
    pub dimension: String,
    /// The fields the edit changed.
    pub changed: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One of the things an ambiguous dimension's name finds.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CandidateResult {
    /// An edge, as `{body, index}` names it.
    Edge { body: String, index: usize },
    /// A point where the named edges meet, in the design's frame.
    Corner([f64; 3]),
    /// A face, as `{body, index}` names it.
    Face { body: String, index: usize },
}

/// One of a chain's members as the model has it now.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MemberResult {
    /// Model millimetres.
    pub value: f64,
    pub text: String,
}

/// A dimension as the model has it now.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DimensionResult {
    pub dimension: String,
    pub view: String,
    /// The record, as the drawing saves it.
    pub record: Value,
    /// `ok`, `overridden`, `ambiguous`, `suppressed`, `lost` or `unmeasurable`.
    pub status: String,
    /// Why it is lost or unmeasurable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// What it reads: model millimetres, or degrees for an angle; a chain's overall.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// Whether a measured line leaves the view's plane, so the view shows it shorter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreshortened: Option<bool>,
    /// What it shows: the text given, else the number in the drawing's units; none for a chain,
    /// which shows its members'.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// A chain's members, from its first point to its last.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<MemberResult>,
    /// When it has no value now: the last one it read, kept when the design is saved, as `value` gives it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_good: Option<f64>,
    /// Every name it measures was found by exact history.
    pub exact: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<CandidateResult>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "drawing_dimension_tests.rs"]
mod tests;
