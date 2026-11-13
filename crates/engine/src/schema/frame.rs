//! Contracts and result types for the commands in `cmd/frame.rs`: `frame.member`,
//! `frame.profile.list`, `frame.profile.properties` and `frame.profile.insert` (#12500); and
//! `frame.trim` (#4003).
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. Whether a sketch exists, a dimension evaluates
//! or a profile can be built stays the handler's to check. A key the handler refuses when it is
//! left out (`profile`, `sketch`) is optional in the type and required in the schema, so that the
//! handler's message, not the check's "is required", is what a caller sees. A call with several
//! problems may get a different one first in strict than with the check off (the check goes key
//! by key, the handlers in their own order).
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
use solidcraft_doc::frame::{self, Anchor, MAX_MEMBERS, Profile, ProfileSpec};

use super::check::{keyed, located, whole};
use super::motion::require;
use super::sketch_ends::SketchTarget;
use super::types::{
    AngleExpr, FeatureCommon, IdOrName, LengthExpr, Point2, ProfilesParam, SketchRef, StringList, feature_common, not_blank_pattern, present,
};
use crate::params;

/// The words `Anchor` is spelled with, as the handlers' message lists them.
const ANCHORS: [&str; 11] =
    ["origin", "center", "centroid", "top_left", "top", "top_right", "left", "right", "bottom_left", "bottom", "bottom_right"];

/// The longest name a member may be given, in bytes once trimmed, as the handler requires.
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
    /// The degrees of freedom left.
    pub dof: usize,
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

/// The name members are named after: text of 1 to 128 bytes once trimmed, as `frame.member` reads
/// `body_name`.
#[derive(Clone, Debug, PartialEq)]
pub struct MemberName(pub String);

impl<'de> Deserialize<'de> for MemberName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) if !s.trim().is_empty() && s.trim().len() <= MAX_NAME => Ok(MemberName(s)),
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
            "description": "A name: text that isn't blank, at most 128 bytes once trimmed of the whitespace around it.",
            "type": "string", "pattern": not_blank_pattern()
        })
    }
}

/// `names`: up to 1000 line ids, each with a name of 1 to 128 bytes once trimmed.
#[derive(Clone, Debug, PartialEq)]
pub struct MemberNames(pub BTreeMap<String, String>);

impl<'de> Deserialize<'de> for MemberNames {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Object(m) if m.len() <= MAX_MEMBERS => m
                .into_iter()
                .map(|(line, v)| match v.as_str().map(str::trim).filter(|n| !n.is_empty() && n.len() <= MAX_NAME) {
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
            "description": "Names to keep, by line id. A name is text that isn't blank, at most 128 bytes once trimmed.",
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

/// `sketch` is required and one of `profile` and `profile_sketch`, not both, as the schema says;
/// the type leaves them to the handler's messages.
fn member_required(schema: &mut Schema) {
    require(schema, "sketch");
    let only = |key: &str, other: &str| json!({"required": [key], "not": {"required": [other]}});
    schema.insert("anyOf".into(), json!([only("profile", "profile_sketch"), only("profile_sketch", "profile")]));
}

/// Lay structural members along the lines of a frame sketch: one body per line, from its start
/// to its end, its cross-section square to the line. The section is a built-in `profile` or the
/// profiles of a `profile_sketch`, one of them. Each member keeps its name when other lines are
/// added or deleted. Returns the feature and the bodies it made.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = member_required)]
pub struct FrameMemberKeys {
    // Optional in the type and required in the schema (`member_required`).
    /// The frame sketch, 2D or 3D, by id or name.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchRef")]
    pub sketch: Option<SketchRef>,
    /// The lines to lay members along, by curve id, one member each. Default: every straight
    /// line drawn in the sketch that isn't construction, projected or otherwise linked, or of no
    /// length.
    #[serde(default)]
    pub lines: Option<LineIds>,
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

// ---------------------------------------------------------------------------------------------
// Trim/Extend (`frame.trim`)

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
