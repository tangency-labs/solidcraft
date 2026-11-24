//! Contracts and result types for the commands in `cmd/frame.rs`: `frame.member`,
//! `frame.profile.list`, `frame.profile.properties` and `frame.profile.insert` (#12500).
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. Whether a sketch exists, a dimension evaluates
//! or a profile can be built stays the handler's to check. A key the handler refuses when it is
//! left out (`profile`, and `sketch` when there are no `pairs`) is optional in the type and
//! required in the schema, so that the handler's message, not the check's "is required", is what
//! a caller sees. A call with several problems may get a different one first in strict than with
//! the check off (the check goes key by key, the handlers in their own order).
//!
//! `frame.member` reads some keys only in some cases. With `profile` (a built-in profile) it
//! never reads `profiles` or `exact_regions`, which pick among a section sketch's profiles; with
//! `profile_sketch` it never reads `profile`. Given both, or neither, it refuses the call before
//! it reads anything about the section, so none of the three is read then either.

use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value, json};
use solidcore_record::Vec2;
use solidcraft_doc::Expr;
use solidcraft_doc::frame::sizes::Family;
use solidcraft_doc::frame::{self, Anchor, CornerOverride, CornerType, MAX_MEMBERS, MemberRef, Profile, ProfileSpec};

use super::check::{keyed, located, parse_issue, whole};
use super::motion::require;
use super::sketch_ends::SketchTarget;
use super::types::{AngleExpr, GeoRefParam, LengthExpr, Point2, ProfilesParam, SketchRef, StringList, not_blank_pattern, present};
use crate::params;

/// The words `Anchor` is spelled with, as the handlers' message lists them.
const ANCHORS: [&str; 11] =
    ["origin", "center", "centroid", "top_left", "top", "top_right", "left", "right", "bottom_left", "bottom", "bottom_right"];

/// The longest name a member may be given, in characters once trimmed, as the handler requires.
const MAX_NAME: usize = 128;

/// `v` as the only key of an object, for the helpers that read `p.get(key)`.
fn wrap(key: &str, v: Value) -> Value {
    json!({ key: v })
}

// ---------------------------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------------------------

/// A built-in profile as the commands take it: `{kind, …dimensions}`, each dimension a number or
/// an expression, read by `ProfileSpec`'s own deserializer. Whether a dimension evaluates, and
/// whether the profile can be built from the values (a wall thicker than the tube), is the
/// handler's to check, as it has the design.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileParam(pub ProfileSpec);

impl<'de> Deserialize<'de> for ProfileParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // The handler's `` `profile`: {error} ``, with serde_json's own wording of the error.
        let v = Value::deserialize(d)?;
        if frame::size_with_dimensions(&v) {
            return Err(D::Error::custom(keyed(frame::SIZE_ALONE)));
        }
        serde_json::from_value::<ProfileSpec>(v).map(ProfileParam).map_err(|e| D::Error::custom(keyed(e)))
    }
}

/// The schema of a standard size given by name: `size`, and `kind` at most.
fn size_schema() -> Value {
    let kinds: Vec<&str> = frame::profile_kinds().iter().map(|k| k.kind).collect();
    json!({
        "type": "object",
        "properties": {
            "size": {"type": "string", "minLength": 1, "maxLength": frame::MAX_SIZE_NAME, "pattern": not_blank_pattern()},
            "kind": {"enum": kinds}
        },
        "required": ["size"],
        "additionalProperties": false
    })
}

/// The schema of one dimension, a number or an expression.
fn dimension_schema() -> Value {
    json!({
        "anyOf": [
            {"type": "number"},
            {"type": "string", "minLength": 1, "maxLength": 4096, "pattern": not_blank_pattern()}
        ]
    })
}

/// One schema per built-in profile kind: its `kind`, its required dimensions and its optional
/// ones, each as `dimension` says, and nothing else.
fn profile_kinds_schema(dimension: &Value) -> Vec<Value> {
    frame::profile_kinds()
        .into_iter()
        .map(|k| {
            let mut properties = Map::new();
            properties.insert("kind".into(), json!({"enum": [k.kind]}));
            for name in k.required.iter().chain(k.optional) {
                properties.insert((*name).into(), dimension.clone());
            }
            let required: Vec<&str> = std::iter::once("kind").chain(k.required.iter().copied()).collect();
            json!({"type": "object", "properties": properties, "required": required, "additionalProperties": false})
        })
        .collect()
}

impl JsonSchema for ProfileParam {
    fn schema_name() -> Cow<'static, str> {
        "ProfileParam".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A built-in profile: {\"kind\": bar | round_bar | rect_tube | round_tube | angle | channel | i_beam, …its dimensions}. Each dimension is a number in the design's units or a length expression over its parameters (\"tube_wall\", \"2 in\"); a channel's `flange_slope` is an angle, a number of degrees or an angle expression. `frame.profile.list` gives each kind's dimensions and an example. A dimension that is left out and isn't required takes a default worked out from the profile's size. Or a standard steel size by name, {\"size\": \"RSC 203x89\"}, alone: it brings its own dimensions (`frame.profile.list` lists the sizes).",
            "anyOf": profile_kinds_schema(&dimension_schema()).into_iter().chain([size_schema()]).collect::<Vec<_>>()
        })
    }
}

/// A built-in profile as a command reports it, every number in mm: serialized as the profile's
/// own JSON (`{kind, …dimensions}`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ProfileOut(pub Profile);

impl JsonSchema for ProfileOut {
    fn schema_name() -> Cow<'static, str> {
        "ProfileOut".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A built-in profile: {\"kind\", …its dimensions}, each in mm but a channel's `flange_slope`, in degrees.",
            "anyOf": profile_kinds_schema(&json!({"type": "number"}))
        })
    }
}

/// Where a profile is held to its placement point, as `Anchor` is spelled in JSON.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchorParam(pub Anchor);

impl<'de> Deserialize<'de> for AnchorParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        serde_json::from_value::<Anchor>(Value::deserialize(d)?).map(AnchorParam).map_err(|_| {
            D::Error::custom(whole(
                "`anchor` must be origin, center, centroid, top_left, top, top_right, left, right, bottom_left, bottom or bottom_right",
            ))
        })
    }
}

impl JsonSchema for AnchorParam {
    fn schema_name() -> Cow<'static, str> {
        "AnchorParam".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The profile point that is placed: its origin (the default; a built-in profile's centre), `center` (its bounding box's centre), `centroid`, or a corner or edge middle of its bounding box.",
            "type": "string",
            "enum": ANCHORS
        })
    }
}

// ---------------------------------------------------------------------------------------------
// frame.profile.list
// ---------------------------------------------------------------------------------------------

/// A family of standard sizes by its letters, as `frame.profile.list` reads `family`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FamilyParam(pub Family);

impl<'de> Deserialize<'de> for FamilyParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        v.as_str()
            .and_then(|f| Family::ALL.into_iter().find(|x| x.prefix() == f))
            .map(FamilyParam)
            .ok_or_else(|| D::Error::custom(whole("`family` must be RSC, PFC, RSA, SHS, RHS or CHS")))
    }
}

impl JsonSchema for FamilyParam {
    fn schema_name() -> Cow<'static, str> {
        "FamilyParam".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let families: Vec<&str> = Family::ALL.iter().map(|f| f.prefix()).collect();
        json_schema!({"description": "A family of standard steel sizes.", "type": "string", "enum": families})
    }
}

/// List the built-in profiles and the standard steel sizes.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ProfileListParams {
    /// List only this family's sizes. Default: every family.
    #[serde(default)]
    pub family: Option<FamilyParam>,
}

/// One built-in profile kind.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileKindInfo {
    /// The kind's id, as `profile.kind` takes it.
    pub kind: String,
    /// The kind's name for people.
    pub label: String,
    /// The dimensions it needs.
    pub required: Vec<String>,
    /// The dimensions that default when left out.
    pub optional: Vec<String>,
    /// A complete example profile.
    pub example: ProfileOut,
}

impl From<&frame::ProfileKind> for ProfileKindInfo {
    fn from(k: &frame::ProfileKind) -> Self {
        ProfileKindInfo {
            kind: k.kind.to_string(),
            label: k.label.to_string(),
            required: k.required.iter().map(|d| d.to_string()).collect(),
            optional: k.optional.iter().map(|d| d.to_string()).collect(),
            example: ProfileOut(k.example.clone()),
        }
    }
}

/// One family of standard steel sizes.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SizeFamilyInfo {
    /// The family's letters, which its sizes' names start with (`RSC`).
    pub family: String,
    /// The built-in profile kind its sizes are.
    pub kind: String,
    /// The standard the sizes come from.
    pub standard: String,
    /// The published table their dimensions were read from.
    pub source: String,
    /// Every size's name, as `profile.size` takes it (`RSC 203x89`).
    pub names: Vec<String>,
    /// The sizes the table adds to the standard's own range. Left out when there are none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional: Vec<String>,
}

/// What `frame.profile.list` returns: the built-in profiles, each SolidCraft's own parametric
/// definition, and the standard steel sizes by name.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileListResult {
    /// Every built-in profile kind.
    pub kinds: Vec<ProfileKindInfo>,
    /// The standard sizes, family by family (only `family`'s when it is given).
    pub sizes: Vec<SizeFamilyInfo>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// frame.profile.properties and the section both profile commands report
// ---------------------------------------------------------------------------------------------

/// A density in kg/m³: above 0 and at most 1e6, as `frame.profile.properties` requires.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Density(pub f64);

impl<'de> Deserialize<'de> for Density {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        params::num(&wrap("density", Value::deserialize(d)?), "density")
            .filter(|d| *d > 0.0 && *d <= 1e6)
            .map(Density)
            .ok_or_else(|| D::Error::custom(whole("`density` must be a positive number of kg/m³ (at most 1e6)")))
    }
}

impl JsonSchema for Density {
    fn schema_name() -> Cow<'static, str> {
        "Density".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A density in kg/m³: above 0 and at most 1e6.",
            "type": "number", "exclusiveMinimum": 0, "maximum": 1e6
        })
    }
}

fn profile_required(schema: &mut Schema) {
    require(schema, "profile");
}

/// Work out a built-in profile's section: its area, centroid and second moments, about axes
/// through the centroid, parallel to the profile's x and y.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = profile_required)]
pub struct ProfilePropertiesParams {
    // Optional in the type and required in the schema (`profile_required`).
    /// The profile, as `frame.profile.list` describes the kinds.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ProfileParam")]
    pub profile: Option<ProfileParam>,
    /// The material's density, which adds `mass_per_m`. Default: none, and no `mass_per_m`.
    #[serde(default)]
    pub density: Option<Density>,
}

/// A profile's section, as `frame.profile.properties` and `frame.profile.insert` report it.
/// Lengths are in mm, areas in mm² and second moments in mm⁴.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileSection {
    /// The profile with every default filled in.
    pub profile: ProfileOut,
    /// The standard size it is, by name, when it was given as one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    /// The section's area, mm².
    pub area: f64,
    /// The centroid [x, y], mm, in the profile's own plane (x across, y up) from the centre of its
    /// bounding box.
    pub centroid: [f64; 2],
    /// ∫ y² dA about the centroid: bending about the x axis, mm⁴.
    pub ixx: f64,
    /// ∫ x² dA about the centroid: bending about the y axis, mm⁴.
    pub iyy: f64,
    /// ∫ x·y dA about the centroid, mm⁴.
    pub ixy: f64,
    /// The bounding box's width, mm.
    pub width: f64,
    /// The bounding box's height, mm.
    pub height: f64,
    /// With a `density`: the mass of a metre of the profile, kg/m.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mass_per_m: Option<f64>,
}

/// What `frame.profile.properties` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfilePropertiesResult {
    #[serde(flatten)]
    pub section: ProfileSection,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// frame.remove_member
// ---------------------------------------------------------------------------------------------

/// `members` as `frame.remove_member` reads it (the handler and the contract both): 1 to 1000
/// `{feature, line}`, each the Frame Member by id or name and the member's line or pair id.
/// Whether the feature and the member exist is the handler's to check.
pub fn removed_members(v: Option<&Value>) -> Result<Vec<(Value, String)>, String> {
    let list = match v {
        Some(Value::Array(a)) if !a.is_empty() && a.len() <= MAX_MEMBERS => a,
        _ => return Err(format!("`members` must list 1…{MAX_MEMBERS} tubes: [{{feature, line}}]")),
    };
    list.iter()
        .enumerate()
        .map(|(i, m)| match (m.get("feature"), m.get("line")) {
            (Some(f @ (Value::Number(_) | Value::String(_))), Some(Value::String(line))) if m.as_object().is_some_and(|o| o.len() == 2) => {
                Ok((f.clone(), line.clone()))
            }
            _ => Err(format!("`members[{i}]` must be {{feature, line}}: the Frame Member (id or name) and the tube's line or pair id")),
        })
        .collect()
}

/// `members`: the tubes to take out, read by [`removed_members`].
#[derive(Debug)]
pub struct RemovedMembers(pub Vec<(Value, String)>);

impl<'de> Deserialize<'de> for RemovedMembers {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        removed_members(Some(&Value::deserialize(d)?)).map(RemovedMembers).map_err(|e| D::Error::custom(whole(e)))
    }
}

impl JsonSchema for RemovedMembers {
    fn schema_name() -> Cow<'static, str> {
        "RemovedMembers".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The tubes to take out, all of one Frame Member: the feature by id or name, and the tube's line or pair id.",
            "type": "array",
            "items": {
                "type": "object",
                "properties": {"feature": {"type": ["integer", "string"], "minimum": 0}, "line": {"type": "string"}},
                "required": ["feature", "line"],
                "additionalProperties": false
            },
            "minItems": 1, "maxItems": MAX_MEMBERS
        })
    }
}

/// Take tubes out of a Frame Member, as one undo step: a pair goes from its `pairs`, a sketch
/// line from its `lines` (the line stays in its sketch). Taking out its last tube deletes the
/// Frame Member. Returns the edited feature, or `{deleted}` with the deleted feature's name.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = members_required)]
pub struct RemoveMemberParams {
    // Required in the schema (`members_required`), optional in the type.
    /// The tubes to take out.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "RemovedMembers")]
    pub members: Option<RemovedMembers>,
}

fn members_required(schema: &mut Schema) {
    require(schema, "members");
}

/// What `frame.remove_member` returns when its last tube went with the feature.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MemberFeatureDeleted {
    /// The name of the Frame Member deleted with its last tube.
    pub deleted: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `frame.remove_member` returns: the edited feature, or the deleted one's name.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum RemoveMemberResult {
    Edited(Box<super::features::FeatureResult>),
    Deleted(MemberFeatureDeleted),
}

// ---------------------------------------------------------------------------------------------
// frame.multi_trim
// ---------------------------------------------------------------------------------------------

/// What `frame.multi_trim`'s `members` must be.
pub const TRIM_MEMBERS: &str = "`members` must list 2…1000 members that meet: line or pair ids, or {feature?, line}";

/// `members` as `frame.multi_trim` reads it (the handler and the contract both): 2 to 1000 members, each a line or
/// pair id or `{feature?, line}`. Whether they are the frame's and meet is the handler's to check.
pub fn trim_members(v: Option<&Value>) -> Result<Vec<MemberRef>, String> {
    match v {
        Some(Value::Array(a)) if (2..=MAX_MEMBERS).contains(&a.len()) => a
            .iter()
            .map(|m| match m {
                Value::String(line) => Some(MemberRef { feature: None, line: line.clone() }),
                Value::Object(_) => serde_json::from_value(m.clone()).ok(),
                _ => None,
            })
            .collect::<Option<_>>()
            .ok_or_else(|| TRIM_MEMBERS.to_string()),
        _ => Err(TRIM_MEMBERS.to_string()),
    }
}

/// `members`: the members of the corner, read by [`trim_members`].
#[derive(Debug)]
pub struct TrimMembers(pub Vec<MemberRef>);

impl<'de> Deserialize<'de> for TrimMembers {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        trim_members(Some(&Value::deserialize(d)?)).map(TrimMembers).map_err(|e| D::Error::custom(whole(e)))
    }
}

impl JsonSchema for TrimMembers {
    fn schema_name() -> Cow<'static, str> {
        "TrimMembers".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The members that meet at the corner, crossing or overlap, in order (the first runs through, or stays whole): line or pair ids, or {feature?, line} (`feature` only the frame's own name).",
            "type": "array", "minItems": 2, "maxItems": MAX_MEMBERS,
            "items": {"anyOf": [
                {"type": "string"},
                {"type": "object", "properties": {"feature": {"type": "string"}, "line": {"type": "string"}}, "required": ["line"]}
            ]}
        })
    }
}

/// What `frame.multi_trim`'s `kind` must be.
pub const TRIM_KIND: &str = "`kind` must be auto, none, mitre or butt";

/// `kind` as `frame.multi_trim` reads it (`coped_butt` is not taken: Coped is the frame's).
pub fn trim_kind(v: &Value) -> Option<CornerType> {
    serde_json::from_value(v.clone()).ok().filter(|k| *k != CornerType::CopedButt)
}

/// `kind`, read by [`trim_kind`].
#[derive(Debug)]
pub struct TrimKind(pub CornerType);

impl<'de> Deserialize<'de> for TrimKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        trim_kind(&Value::deserialize(d)?).map(TrimKind).ok_or_else(|| D::Error::custom(whole(TRIM_KIND)))
    }
}

impl JsonSchema for TrimKind {
    fn schema_name() -> Cow<'static, str> {
        "TrimKind".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The corner's type: auto (the frame's own type, with this order), none, mitre or butt.",
            "type": "string",
            "enum": ["auto", "none", "mitre", "butt"]
        })
    }
}

/// Set, change or remove one corner override of a Frame Member, as one undo step: the corner, crossing or overlap
/// `members` meet at finished its own way. Replaces the override of the same members, or adds one. Returns the
/// edited feature as `frame.member` does.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = trim_required)]
pub struct MultiTrimParams {
    // `feature` and `members` are required in the schema (`trim_required`), optional in the type.
    /// The Frame Member, by id or name.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "super::edit::FeatureRef")]
    pub feature: Option<super::edit::FeatureRef>,
    /// The members that meet there, in order.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "TrimMembers")]
    pub members: Option<TrimMembers>,
    /// The corner's type. Default: auto.
    #[serde(default)]
    pub kind: Option<TrimKind>,
    /// The first two members as today's rules order them, swapped: the other one runs through. Without it the
    /// members are saved in the order given.
    #[serde(default)]
    pub flip: Option<bool>,
    /// The corner's own weld gap (0 to 1000 mm, a length or an expression). Default: the frame's. An overlap takes
    /// none.
    #[serde(default)]
    pub gap: Option<MemberGap>,
    /// Take the override of these members away (the corner goes back to the frame's own rules); no `kind`, `flip`
    /// or `gap` with it.
    #[serde(default)]
    pub remove: Option<bool>,
}

fn trim_required(schema: &mut Schema) {
    require(schema, "feature");
    require(schema, "members");
}

// ---------------------------------------------------------------------------------------------
// frame.profile.insert
// ---------------------------------------------------------------------------------------------

/// A point of the sketch in mm: `[x, y]` or `{x, y}`, with `frame.profile.insert`'s message.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InsertAt(pub Vec2);

impl<'de> Deserialize<'de> for InsertAt {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        params::vec2(&Value::deserialize(d)?).map(InsertAt).ok_or_else(|| D::Error::custom(whole("`at` must be [x, y] in sketch millimetres")))
    }
}

impl JsonSchema for InsertAt {
    fn schema_name() -> Cow<'static, str> {
        "InsertAt".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        generator.subschema_for::<Point2>()
    }
}

/// An angle in radians as `frame.profile.insert` reads it: a number, at most 1e6 in size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Radians(pub f64);

impl<'de> Deserialize<'de> for Radians {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        params::num(&wrap("angle", Value::deserialize(d)?), "angle")
            .filter(|a| a.abs() <= 1e6)
            .map(Radians)
            .ok_or_else(|| D::Error::custom(whole("`angle` must be a number of radians")))
    }
}

impl JsonSchema for Radians {
    fn schema_name() -> Cow<'static, str> {
        "Radians".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "An angle in radians, counter-clockwise: a number of at most 1e6 in size.", "type": "number", "minimum": -1e6, "maximum": 1e6})
    }
}

/// Draw a built-in profile into a sketch as lines and arcs, placed by its anchor and turned about
/// it. A tube's inside is drawn too, so the wall is one profile. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = profile_required)]
pub struct ProfileInsertParams {
    // Optional in the type and required in the schema (`profile_required`).
    /// The profile, as `frame.profile.list` describes the kinds.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ProfileParam")]
    pub profile: Option<ProfileParam>,
    /// Where the anchor goes, in sketch mm. Default: the origin.
    #[serde(default)]
    pub at: Option<InsertAt>,
    /// The profile point that is placed on `at`. Default: origin.
    #[serde(default)]
    pub anchor: Option<AnchorParam>,
    /// How far the profile turns about the anchor, in radians counter-clockwise (not degrees).
    /// Default: 0.
    #[serde(default)]
    pub angle: Option<Radians>,
    /// Lock the curves, so that solving the sketch keeps the shape. Default: true.
    #[serde(default)]
    pub fixed: Option<bool>,
    /// The sketch to draw in, by id or name. Default: the sketch being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// The ids of what a sketch edit added, stable and never reused.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CreatedIds {
    /// The points added.
    pub points: Vec<String>,
    /// The curves added.
    pub curves: Vec<String>,
    /// The constraints added, dimensions apart.
    pub constraints: Vec<String>,
    /// The dimensions added.
    pub dimensions: Vec<String>,
    /// The wires added. Left out when there are none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wires: Vec<String>,
}

/// The sketch's solve after an edit.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SketchSolve {
    /// Whether the constraints solved, keeping the sketch's orientation.
    pub solved: bool,
    /// The degrees of freedom left, a sketch's drawn 3D curves' included (#10608).
    pub dof: usize,
    /// For a sketch with drawn 3D curves: the planar sketch's own degrees of freedom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planar_dof: Option<usize>,
    /// For a sketch with drawn 3D curves: whether its 3D constraints hold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wires_solved: Option<bool>,
}

/// What `frame.profile.insert` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileInsertResult {
    /// What the insertion added to the sketch.
    pub created: CreatedIds,
    /// The inserted profile's section, as `frame.profile.properties` reports it.
    pub section: ProfileSection,
    /// The sketch's solve, or null when it was not solved again.
    pub sketch: Option<SketchSolve>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// frame.member
// ---------------------------------------------------------------------------------------------

/// How two members are finished where they meet, as `CornerType` is spelled in JSON.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CornerParam(pub CornerType);

impl<'de> Deserialize<'de> for CornerParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        serde_json::from_value::<CornerType>(Value::deserialize(d)?)
            .map(CornerParam)
            .map_err(|_| D::Error::custom(whole("`corner` must be auto, none, mitre or butt")))
    }
}

impl JsonSchema for CornerParam {
    fn schema_name() -> Cow<'static, str> {
        "CornerParam".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "How members ending at one point are cut.",
            "type": "string",
            "enum": ["auto", "none", "mitre", "butt"]
        })
    }
}

/// `corners`: up to 1000 corner overrides, as `frame.member` reads them. Whether the members are
/// the feature's and meet at a corner is the handler's to check.
#[derive(Clone, Debug, PartialEq)]
pub struct CornersParam(pub Vec<CornerOverride>);

/// What a `corners` entry must be.
pub const CORNER_ENTRY: &str = "`corners` entries must be {members: [2 to 1000 line or pair ids, or {line}], kind, gap?}";

/// A `corners` entry, either shape (`corner` for `kind`, members as bare ids), and its `flip` when it gives one (an
/// older script's, or the dialog's: the handler saves it as an order). `None` for anything else.
pub fn corner_entry(v: &Value) -> Option<(CornerOverride, Option<bool>)> {
    let flip = match v.get("flip") {
        None | Some(Value::Null) => None,
        Some(f) => Some(f.as_bool()?),
    };
    // `gap` as any length parameter: a number or an expression.
    let gap = match v.get("gap") {
        None | Some(Value::Null) => None,
        Some(_) => Some(params::expr(v, "gap")?),
    };
    let mut v = v.clone();
    v.as_object_mut()?.remove("gap");
    let entry: CornerOverride = serde_json::from_value(v).ok()?;
    Some((CornerOverride { gap, ..entry }, flip))
}

impl<'de> Deserialize<'de> for CornersParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let fail = || D::Error::custom(whole(format!("`corners` must be a list of up to {MAX_MEMBERS} corner overrides")));
        let Value::Array(a) = Value::deserialize(d)? else { return Err(fail()) };
        if a.len() > MAX_MEMBERS {
            return Err(fail());
        }
        a.iter()
            .map(|v| corner_entry(v).map(|e| e.0).ok_or_else(|| D::Error::custom(whole(CORNER_ENTRY))))
            .collect::<Result<_, _>>()
            .map(CornersParam)
    }
}

impl JsonSchema for CornersParam {
    fn schema_name() -> Cow<'static, str> {
        "CornersParam".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Corners, crossings and overlaps finished their own way: the 2 to 1000 members that meet there (lines' or pairs' ids), in order (the first runs through, or stays whole), and the corner type. An older script's `corner` reads as `kind`, and its `flip` is saved as today's order with the first two swapped.",
            "type": "array", "maxItems": MAX_MEMBERS,
            "items": {
                "type": "object",
                "properties": {
                    "members": {"type": "array", "minItems": 2, "maxItems": MAX_MEMBERS, "items": {"anyOf": [
                        {"type": "string"},
                        {"type": "object", "properties": {"feature": {"type": "string"}, "line": {"type": "string"}}, "required": ["line"]}
                    ]}},
                    "kind": {"type": "string", "enum": ["auto", "none", "mitre", "butt"]},
                    "corner": {"type": "string", "enum": ["auto", "none", "mitre", "butt"]},
                    "flip": {"type": "boolean"},
                    "gap": {"type": ["string", "number"]}
                },
                // Its type as `kind` or an older `corner`, one of them, as the handler reads it.
                "required": ["members"],
                "anyOf": [
                    {"required": ["kind"], "not": {"required": ["corner"]}},
                    {"required": ["corner"], "not": {"required": ["kind"]}}
                ]
            }
        })
    }
}

/// `lines`: one to 1000 curve ids, none twice, as `frame.member` reads them (a single id isn't
/// taken).
#[derive(Clone, Debug, PartialEq)]
pub struct LineIds(pub Vec<String>);

impl<'de> Deserialize<'de> for LineIds {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let fail = || D::Error::custom(whole(format!("`lines` must list 1…{MAX_MEMBERS} curve ids")));
        let Value::Array(a) = Value::deserialize(d)? else { return Err(fail()) };
        if a.is_empty() || a.len() > MAX_MEMBERS {
            return Err(fail());
        }
        let lines = a.into_iter().map(|v| if let Value::String(s) = v { Some(s) } else { None }).collect::<Option<Vec<_>>>().ok_or_else(fail)?;
        let mut seen = std::collections::HashSet::new();
        match lines.iter().find(|l| !seen.insert(l.as_str())) {
            Some(twice) => Err(D::Error::custom(whole(format!("line `{twice}` is listed twice")))),
            None => Ok(LineIds(lines)),
        }
    }
}

impl JsonSchema for LineIds {
    fn schema_name() -> Cow<'static, str> {
        "LineIds".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Curve ids of the frame sketch's lines, each once.",
            "type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": MAX_MEMBERS, "uniqueItems": true
        })
    }
}

/// The name members are named after: text of 1 to 128 characters once trimmed, as `frame.member` reads
/// `body_name`.
#[derive(Clone, Debug, PartialEq)]
pub struct MemberName(pub String);

impl<'de> Deserialize<'de> for MemberName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) if !s.trim().is_empty() && s.trim().chars().count() <= MAX_NAME => Ok(MemberName(s)),
            _ => Err(D::Error::custom(whole(format!("`body_name` must be 1…{MAX_NAME} characters")))),
        }
    }
}

impl JsonSchema for MemberName {
    fn schema_name() -> Cow<'static, str> {
        "MemberName".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A name: text that isn't blank, at most 128 characters once trimmed of the whitespace around it.",
            "type": "string", "pattern": not_blank_pattern()
        })
    }
}

/// `names`: up to 1000 line ids, each with a name of 1 to 128 characters once trimmed.
#[derive(Clone, Debug, PartialEq)]
pub struct MemberNames(pub BTreeMap<String, String>);

impl<'de> Deserialize<'de> for MemberNames {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Object(m) if m.len() <= MAX_MEMBERS => m
                .into_iter()
                .map(|(line, v)| match v.as_str().map(str::trim).filter(|n| !n.is_empty() && n.chars().count() <= MAX_NAME) {
                    Some(name) => Ok((line, name.to_string())),
                    None => Err(D::Error::custom(whole(format!("`names`: the name of line `{line}` must be 1…{MAX_NAME} characters")))),
                })
                .collect::<Result<_, _>>()
                .map(MemberNames),
            _ => Err(D::Error::custom(whole(format!("`names` must map up to {MAX_MEMBERS} line ids to names")))),
        }
    }
}

impl JsonSchema for MemberNames {
    fn schema_name() -> Cow<'static, str> {
        "MemberNames".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Names to keep, by line id. A name is text that isn't blank, at most 128 characters once trimmed.",
            "type": "object",
            "additionalProperties": {"type": "string", "pattern": not_blank_pattern()},
            "maxProperties": MAX_MEMBERS
        })
    }
}

/// `angle` as `frame.member` reads it: a number of degrees or an expression, with its message. Whether
/// the expression evaluates as an angle is the handler's to check.
#[derive(Clone, Debug, PartialEq)]
pub struct MemberAngle(pub Expr);

impl<'de> Deserialize<'de> for MemberAngle {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        params::expr(&wrap("angle", Value::deserialize(d)?), "angle")
            .map(MemberAngle)
            .ok_or_else(|| D::Error::custom(whole("`angle` must be an angle expression")))
    }
}

impl JsonSchema for MemberAngle {
    fn schema_name() -> Cow<'static, str> {
        "MemberAngle".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        generator.subschema_for::<AngleExpr>()
    }
}

/// `gap` as `frame.member` reads it: a length or an expression, with its message. Whether the expression evaluates as
/// a length, and within 0…1000 mm, is the handler's to check.
#[derive(Clone, Debug, PartialEq)]
pub struct MemberGap(pub Expr);

impl<'de> Deserialize<'de> for MemberGap {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        params::expr(&wrap("gap", Value::deserialize(d)?), "gap")
            .map(MemberGap)
            .ok_or_else(|| D::Error::custom(whole("`gap` must be a length expression")))
    }
}

impl JsonSchema for MemberGap {
    fn schema_name() -> Cow<'static, str> {
        "MemberGap".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        generator.subschema_for::<LengthExpr>()
    }
}

/// A group's located profile point, with the same bounds and errors as the member handler.
fn member_locate<'de, D: Deserializer<'de>>(d: D) -> Result<Option<[f64; 2]>, D::Error> {
    let value = Value::deserialize(d)?;
    if value.is_null() {
        return Ok(None);
    }
    let Some([x, y]) = value.as_array().map(Vec::as_slice) else {
        return Err(D::Error::custom(whole("`locate` must be [x, y] in the profile's own coordinates")));
    };
    let x = x.as_f64().filter(|v| v.is_finite()).ok_or_else(|| D::Error::custom(whole("`locate` x must be a finite length")))?;
    let y = y.as_f64().filter(|v| v.is_finite()).ok_or_else(|| D::Error::custom(whole("`locate` y must be a finite length")))?;
    if x.abs() > frame::MAX_PROFILE_SIZE || y.abs() > frame::MAX_PROFILE_SIZE {
        return Err(D::Error::custom(whole("`locate` is outside the largest profile")));
    }
    Ok(Some([x, y]))
}

fn member_locate_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"anyOf": [
        {"type": "null"},
        {"type": "array", "minItems": 2, "maxItems": 2,
         "items": {"type": "number", "minimum": -frame::MAX_PROFILE_SIZE, "maximum": frame::MAX_PROFILE_SIZE}}
    ]})
}

/// One group's explicit lines and independent placement, in the feature's shared section.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MemberGroupParams {
    /// The frame sketch's line ids, each once; a group must list its lines.
    pub lines: LineIds,
    /// The profile point on each line. Default: origin.
    #[serde(default)]
    pub anchor: Option<AnchorParam>,
    /// The profile point on each line, [x, y] in mm from the profile's own origin. Takes
    /// precedence over `anchor`.
    #[serde(default, deserialize_with = "member_locate")]
    #[schemars(schema_with = "member_locate_schema")]
    pub locate: Option<[f64; 2]>,
    /// The profile's rotation about each line. Default: zero.
    #[serde(default)]
    pub angle: Option<MemberAngle>,
    /// Mirror the section before turning it. Default: false.
    #[serde(default)]
    pub mirror: Option<bool>,
    /// The base name of this group's members. Default: the profile's description.
    #[serde(default)]
    pub body_name: Option<MemberName>,
}

/// A bounded list of groups. Cross-group line ownership and the total member limit are checked
/// by the handler, along with sketch references and angle expressions.
#[derive(Debug)]
pub struct MemberGroups(pub Vec<MemberGroupParams>);

impl<'de> Deserialize<'de> for MemberGroups {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        let groups = match value.as_array() {
            Some(groups) if !groups.is_empty() && groups.len() <= MAX_MEMBERS && groups.iter().all(Value::is_object) => groups,
            _ => {
                return Err(D::Error::custom(whole(format!(
                    "`groups` must list 1…{MAX_MEMBERS} groups, each {{lines, anchor?, locate?, angle?, mirror?, body_name?}}"
                ))));
            }
        };
        groups
            .iter()
            .enumerate()
            .map(|(k, group)| {
                if group.get("lines").is_none_or(Value::is_null) {
                    return Err(D::Error::custom(whole(format!("group {}: `lines` must list 1…{MAX_MEMBERS} curve ids", k + 1))));
                }
                serde_path_to_error::deserialize(group).map_err(|e| D::Error::custom(whole(format!("group {}: {}", k + 1, parse_issue(&e).message))))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(MemberGroups)
    }
}

impl JsonSchema for MemberGroups {
    fn schema_name() -> Cow<'static, str> {
        "MemberGroups".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = Vec::<MemberGroupParams>::json_schema(generator);
        schema.insert("minItems".into(), json!(1));
        schema.insert("maxItems".into(), json!(MAX_MEMBERS));
        schema
    }
}

/// `participants`, read as `features::add_feature` reads it: a name or a list of names, with the
/// message of `params::string_list`.
#[derive(Clone, Debug, PartialEq)]
pub struct Participants(pub Vec<String>);

impl<'de> Deserialize<'de> for Participants {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match params::string_list("", &wrap("participants", Value::deserialize(d)?), "participants") {
            Ok(names) => Ok(Participants(names)),
            Err(crate::EngineError::BadParams { msg, .. }) => Err(D::Error::custom(whole(msg))),
            Err(e) => Err(D::Error::custom(whole(e))),
        }
    }
}

impl JsonSchema for Participants {
    fn schema_name() -> Cow<'static, str> {
        "Participants".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        generator.subschema_for::<StringList>()
    }
}

/// One member between two points: from `a` to `b`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MemberPairParam {
    /// The pair's id (`pair<N>`, below `next_pair`) as the feature has it, which Edit Feature
    /// gives back for each pair it keeps. Default: the next free one.
    #[serde(default)]
    pub id: Option<String>,
    /// Where the member starts: a vertex, a sketch point (a 3D sketch node as `wire:index`), a
    /// construction point, a snap point or the origin. Not a fixed point.
    pub a: GeoRefParam,
    /// Where it ends, somewhere else; a point as `a` is.
    pub b: GeoRefParam,
}

/// `pairs`: one to 1000 pairs of points, as `frame.member` reads them.
#[derive(Debug)]
pub struct MemberPairs(pub Vec<MemberPairParam>);

impl<'de> Deserialize<'de> for MemberPairs {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let fail = || D::Error::custom(whole(format!("`pairs` must list one pair of points or more, at most {MAX_MEMBERS}: [{{a, b}}]")));
        match Value::deserialize(d)? {
            Value::Array(a) if !a.is_empty() && a.len() <= MAX_MEMBERS => {
                serde_json::from_value(Value::Array(a)).map(MemberPairs).map_err(|e| D::Error::custom(whole(format!("`pairs`: {e}"))))
            }
            _ => Err(fail()),
        }
    }
}

impl JsonSchema for MemberPairs {
    fn schema_name() -> Cow<'static, str> {
        "MemberPairs".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let pair = generator.subschema_for::<MemberPairParam>();
        json_schema!({
            "description": "Pairs of points, one member each, from `a` to `b`.",
            "type": "array", "items": pair, "minItems": 1, "maxItems": MAX_MEMBERS
        })
    }
}

/// `next_pair`: the number the next new pair's id takes, a whole number, as `frame.member` reads
/// it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NextPair(pub u32);

impl<'de> Deserialize<'de> for NextPair {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .map(NextPair)
            .ok_or_else(|| D::Error::custom(whole("`next_pair` must be a whole number (the number the next new pair's id takes)")))
    }
}

impl JsonSchema for NextPair {
    fn schema_name() -> Cow<'static, str> {
        "NextPair".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The number the next new pair's id takes (`pair<N>`).",
            "type": "integer", "minimum": 0, "maximum": u32::MAX
        })
    }
}

/// `sketch` or `pairs` (or both), and one of `profile` and `profile_sketch`, not both, as the
/// schema says; the type leaves them to the handler's messages.
fn member_required(schema: &mut Schema) {
    let only = |key: &str, other: &str| json!({"required": [key], "not": {"required": [other]}});
    schema.insert(
        "allOf".into(),
        json!([
            {"anyOf": [{"required": ["sketch"]}, {"required": ["pairs"]}]},
            {"anyOf": [only("profile", "profile_sketch"), only("profile_sketch", "profile")]},
            // With groups, placement keys live inside each group, never beside them.
            {"anyOf": [
                {"properties": {"groups": {"type": "null"}}},
                {"required": ["sketch"], "properties": {
                    "lines": {"type": "null"}, "anchor": {"type": "null"}, "locate": {"type": "null"}, "angle": {"type": "null"},
                    "mirror": {"type": "null"}, "body_name": {"type": "null"}, "continuous": {"type": "null"}
                }}
            ]},
            // One group is canonical flat form. Several currently stay independent and square-ended.
            {"anyOf": [
                {"properties": {"groups": {"maxItems": 1}}},
                {"properties": {
                    "pairs": {"type": "null"}, "corner": {"enum": ["none", null]}, "corners": {"maxItems": 0},
                    "flip": {"enum": [false, null]}, "fit_ends": {"enum": [false, null]},
                    "coped": {"enum": [false, null]}, "gap": {"type": "null"}
                }}
            ]}
        ]),
    );
}

/// Lay structural members along the lines of a frame sketch and between pairs of points: one
/// body per line or pair, from its start to its end, its cross-section square to it. The section
/// is a built-in `profile` or the profiles of a `profile_sketch`, one of them. Each member keeps
/// its name when other lines or pairs are added or deleted. Returns the feature and the bodies it
/// made.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = member_required)]
pub struct FrameMemberKeys {
    // `sketch` or `pairs` is required in the schema (`member_required`).
    /// The frame sketch, 2D or 3D, by id or name. Give it, `pairs` or both.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchRef")]
    pub sketch: Option<SketchRef>,
    /// The lines or arcs to lay members along, by curve id. One member each, unless continuous. Default: every straight
    /// line drawn in the sketch that isn't construction, projected or otherwise linked, or of no
    /// length.
    #[serde(default)]
    pub lines: Option<LineIds>,
    /// Independently placed groups sharing the profile. Multi-group frames currently need
    /// sketch lines with corner none and no corner overrides, point pairs or finishing options.
    #[serde(default)]
    pub groups: Option<MemberGroups>,
    /// One member along explicit, ordered tangent lines and circular arcs. Default: false.
    /// Requires corner None without overrides, fitting, coping, Flip, gap or point pairs.
    #[serde(default)]
    pub continuous: Option<bool>,
    /// Members between two points, one each, after the lines' members; each point is found
    /// again whenever the design changes, so the member follows it. Give it, `sketch` or both.
    #[serde(default)]
    pub pairs: Option<MemberPairs>,
    /// The number the next new pair's id takes, which Edit Feature gives back so that a removed
    /// pair's id is never given again. Default: one past the highest id given.
    #[serde(default)]
    pub next_pair: Option<NextPair>,
    /// The section as a built-in profile. Give it or `profile_sketch`, not both.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ProfileParam")]
    pub profile: Option<ProfileParam>,
    /// The section as a sketch: its profiles, in the sketch's own x and y around its origin. It
    /// can't be the frame sketch. Give it or `profile`, not both.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchRef")]
    pub profile_sketch: Option<SketchRef>,
    /// With `profile_sketch`: which of its profiles are the section, as `solid.extrude` takes
    /// them. Default: its material, nested loops alternating, so two circles make a tube. Not
    /// used with `profile`.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ProfilesParam")]
    pub profiles: Option<ProfilesParam>,
    /// With `profile_sketch`: each index or point pick in `profiles` is exactly the region it
    /// names, where it would otherwise take the whole drawn profile a face's projected outline
    /// cuts into regions. Default: false. Not used with `profile`.
    #[serde(default)]
    pub exact_regions: Option<bool>,
    /// The profile point that sits on each line. Default: origin.
    #[serde(default)]
    pub anchor: Option<AnchorParam>,
    /// The profile point that sits on each line, [x, y] in mm from the profile's own origin (a
    /// bar's or an angle's bottom-left corner, a channel's web back at mid-height, the centre of
    /// other profiles). Takes precedence over `anchor`; the member stays attached at that point
    /// when the profile's size changes.
    #[serde(default)]
    pub locate: Option<[f64; 2]>,
    /// How two members ending at one point are cut: `none` (ends square to their lines),
    /// `mitre`, `butt` or `auto` (each corner by its angle). Default: none. An older script's
    /// `coped_butt` reads as `butt` with `coped`.
    #[serde(default)]
    pub corner: Option<CornerParam>,
    /// With a butt: the other member runs through. Default: false.
    #[serde(default)]
    pub flip: Option<bool>,
    /// Free ends are fitted to the tubes of earlier Frame Members. Default: false.
    #[serde(default)]
    pub fit_ends: Option<bool>,
    /// Every butt joint cut to the other tube's real shape (its outline, radii kept, hole filled):
    /// butts, ends on a tube's middle, crossings, overlaps and fitted ends. Mitres stay mitres; not
    /// under `none`. Default: false.
    #[serde(default)]
    pub coped: Option<bool>,
    /// Corners finished their own way, each named by its members' (2 to 1000) line or pair ids.
    #[serde(default)]
    pub corners: Option<CornersParam>,
    /// The weld gap left at every welded end (0 to 1000 mm, a length or an expression): each mitre plane moves half
    /// of it back along its member, a butting end stops this far off the member it meets, and every fitted or coped
    /// end, a crossing's pieces' included, stands this far off; through members keep their length, and tubes notched
    /// where they overlap take none. A corner's own `gap` takes precedence. Default: 0.
    #[serde(default)]
    pub gap: Option<MemberGap>,
    /// How far the profile turns about the line, a number of degrees or an expression. Default:
    /// 0, which puts its y as near world up as the line allows (a vertical member's x along
    /// world X).
    #[serde(default)]
    pub angle: Option<MemberAngle>,
    /// The profile mirrored across its y axis, before it turns. Default: false.
    #[serde(default)]
    pub mirror: Option<bool>,
    /// The feature's name in the timeline.
    #[serde(default)]
    pub name: Option<String>,
    /// What the members are named after, then numbered: `Rail`, `Rail (2)`… Default: the
    /// standard size's name (`RSC 203x89`), the profile's description (`Rect Tube 40x40x3`), or
    /// the profile sketch's name.
    #[serde(default)]
    pub body_name: Option<MemberName>,
    /// Names to keep, by line id, as Edit Feature keeps them. A line not listed gets a name from
    /// `body_name`.
    #[serde(default)]
    pub names: Option<MemberNames>,
    /// The bodies taking part, which every feature records; each an existing body or `*`. Members
    /// are new bodies, whatever this lists.
    #[serde(default)]
    pub participants: Option<Participants>,
}

/// `frame.member`'s parameters: [`FrameMemberKeys`], reading the section's keys only as the
/// handler does (see the module docs).
#[derive(Debug)]
pub struct FrameMemberParams(pub FrameMemberKeys);

impl<'de> Deserialize<'de> for FrameMemberParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut value = Value::deserialize(d)?;
        if let Some(keys) = value.as_object_mut() {
            if keys.get("groups").is_some_and(|groups| !groups.is_null())
                && let Some(key) = ["lines", "anchor", "locate", "angle", "mirror", "body_name", "continuous"]
                    .iter()
                    .find(|key| keys.get(**key).is_some_and(|v| !v.is_null()))
            {
                return Err(D::Error::custom(whole(format!("give `{key}` in each of `groups`, not beside them"))));
            }

            // A key counts as given when it is there, `null` included, as the handler tests it.
            match (keys.contains_key("profile"), keys.contains_key("profile_sketch")) {
                // A built-in profile: the sketch's profile picks are never read.
                (true, false) => {
                    keys.remove("profiles");
                    keys.remove("exact_regions");
                }
                (false, true) => {}
                // Both or neither: the handler refuses before it reads any of them.
                _ => {
                    for key in ["profile", "profile_sketch", "profiles", "exact_regions"] {
                        keys.remove(key);
                    }
                }
            }
        }
        serde_path_to_error::deserialize(&value).map(FrameMemberParams).map_err(|e| D::Error::custom(located(&e)))
    }
}

impl JsonSchema for FrameMemberParams {
    fn schema_name() -> Cow<'static, str> {
        "FrameMemberParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        FrameMemberKeys::json_schema(generator)
    }
}

#[cfg(test)]
#[path = "frame_tests.rs"]
mod tests;
