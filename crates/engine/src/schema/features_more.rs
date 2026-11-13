//! Contracts for five commands in `cmd/features_more.rs` (#12655, #2829): Rib, Web, Remove,
//! Boundary Fill and Replace Face. Each adds a feature and returns `add_feature`'s reply, a
//! [`super::features::FeatureResult`].
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. Whether a sketch, a curve or a body exists, and
//! whether a value evaluates and fits the part, stay the handlers' to check. Where a handler
//! ignores a key of the wrong type (a `flip` that isn't true or false, an item of `cells` that
//! isn't a point, which Boundary Fill skips), the contract refuses it.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use super::check::whole;
use super::config::NameList;
use super::motion::require;
use super::types::{
    AngleExpr, AxisDir, CoilSectionParam, CoilSectionPositionParam, EmbossFacePoint, EmbossModeParam, FaceRef, FeatureCommon, IdOrName, LengthExpr,
    NumberExpr, OperationParam, PlaneParam, Point3, PointExpr3, ProfilesParam, RibDirectionParam, SketchRef, StringList, TouchingParam,
    feature_common, present,
};
use crate::params::{self, MAX_STRING_LIST};

/// The most curves a rib or web follows, and the most cells Boundary Fill fills, as
/// `cmd/features_more.rs` reads them.
const MAX_CURVES: usize = 1000;
const MAX_CELLS: usize = 1000;
/// The most tool bodies Boundary Fill takes.
const MAX_TOOLS: usize = 64;
/// The most faces Replace Face moves.
const MAX_FACES: usize = 1000;

/// A list of names read as `params::string_list` reads it (through [`NameList`], with its
/// messages), then held to `range` items with the handler's whole `message` for any other count.
/// `string_list` reads `null` as no names, so `null` gets that message too.
fn names_in<'de, D: Deserializer<'de>>(d: D, range: std::ops::RangeInclusive<usize>, message: &str) -> Result<Option<NameList>, D::Error> {
    let v = Value::deserialize(d)?;
    let names = if v.is_null() { NameList::default() } else { NameList::deserialize(v).map_err(D::Error::custom)? };
    if range.contains(&names.0.len()) { Ok(Some(names)) } else { Err(D::Error::custom(whole(message))) }
}

/// A list of 1 to `max` names: one name as text, or a list.
fn names_schema(description: &str, max: usize) -> Schema {
    json_schema!({
        "description": description,
        "anyOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": max}]
    })
}

/// Rib's and Web's `curves`: 1 to 1000 sketch curve ids.
fn rib_curves<'de, D: Deserializer<'de>>(d: D) -> Result<Option<NameList>, D::Error> {
    names_in(d, 1..=MAX_CURVES, "`curves` must list 1…1000 sketch curve ids")
}

fn rib_curves_schema(_: &mut SchemaGenerator) -> Schema {
    names_schema("The sketch's open curves, by id: 1 to 1000.", MAX_CURVES)
}

fn curves_required(schema: &mut Schema) {
    require(schema, "curves");
}

/// Add a rib or a web, thin walls from open sketch curves grown until they meet the part. A rib
/// (`solid.rib`) is one wall along a chain of curves, in order; a web (`solid.web`) is one wall
/// along each curve. It joins the part. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = curves_required)]
pub struct RibParams {
    /// The sketch, by id or name. Default: the active sketch, else the last one.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "IdOrName")]
    pub sketch: Option<IdOrName>,
    // Optional in the type, required in the schema: left out, `null` or empty, the handler
    // refuses the call with its own message.
    #[serde(default, deserialize_with = "rib_curves")]
    #[schemars(schema_with = "rib_curves_schema")]
    pub curves: Option<NameList>,
    /// The wall's thickness: across the sketch plane, or across the curves with `direction:
    /// normal`.
    pub thickness: LengthExpr,
    /// How far the wall grows. Default: until it meets the part.
    #[serde(default)]
    pub depth: Option<LengthExpr>,
    /// True: the wall grows to the other side of the curves. Default: false.
    #[serde(default)]
    pub flip: Option<bool>,
    #[serde(default)]
    pub direction: Option<RibDirectionParam>,
    /// What the join makes of pieces that share no face. Default: separate.
    #[serde(default)]
    pub touching: Option<TouchingParam>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Replace Face's `faces`, read as the handler reads them, with its messages.
#[derive(Debug)]
pub struct ReplaceFaces(pub Vec<Value>);

impl<'de> Deserialize<'de> for ReplaceFaces {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let items = v.as_array().ok_or_else(|| D::Error::custom(whole("`faces` must list points [x, y, z] or face names")))?;
        if items.is_empty() || items.len() > MAX_FACES {
            return Err(D::Error::custom(whole("`faces` must list 1…1000 points")));
        }
        for item in items {
            crate::cmd::face_ref_form(item).map_err(|why| D::Error::custom(whole(why)))?;
        }
        Ok(ReplaceFaces(items.clone()))
    }
}

impl JsonSchema for ReplaceFaces {
    fn schema_name() -> Cow<'static, str> {
        "ReplaceFaces".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let face = generator.subschema_for::<FaceRef>();
        json_schema!({
            "description": "The planar faces to move: 1 to 1000.",
            "type": "array",
            "items": face,
            "minItems": 1,
            "maxItems": MAX_FACES
        })
    }
}

fn faces_and_target(schema: &mut Schema) {
    require(schema, "faces");
    schema.insert("anyOf".into(), json!([{"required": ["target"]}, {"required": ["target_face"]}]));
}

/// Move planar faces onto a plane, or onto the plane of a parallel planar face. `target_face` is
/// read before `target`. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = faces_and_target)]
pub struct ReplaceFaceParams {
    // Optional in the type, required in the schema: left out, the handler refuses the call with
    // its own message.
    #[serde(default)]
    pub faces: Option<ReplaceFaces>,
    /// The plane to move the faces onto.
    #[serde(default)]
    pub target: Option<PlaneParam>,
    /// A point on a planar face, whose plane the faces move onto.
    #[serde(default)]
    pub target_face: Option<Point3>,
    /// The body the faces are on. Default: found from the faces.
    #[serde(default)]
    pub body: Option<String>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Remove's `bodies`: 1 to 10 000 names.
fn remove_bodies<'de, D: Deserializer<'de>>(d: D) -> Result<Option<NameList>, D::Error> {
    names_in(d, 1..=MAX_STRING_LIST, "`bodies` must list bodies")
}

fn remove_bodies_schema(_: &mut SchemaGenerator) -> Schema {
    names_schema("The bodies, by name: one, or a list of 1 to 10 000.", MAX_STRING_LIST)
}

fn bodies_required(schema: &mut Schema) {
    require(schema, "bodies");
}

/// Remove bodies from the design, from this point in the timeline on. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = bodies_required)]
pub struct RemoveParams {
    // Required in the schema (`bodies_required`), optional in the type, as Rib's `curves`.
    #[serde(default, deserialize_with = "remove_bodies")]
    #[schemars(schema_with = "remove_bodies_schema")]
    pub bodies: Option<NameList>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// The handler's messages for Boundary Fill's `tools` and `cells`.
const TOOLS_MESSAGE: &str = "`tools` must list 1…64 bodies";
const CELLS_MESSAGE: &str = "`cells` must list 1…1000 points [x, y, z], one inside each cell";

/// Boundary Fill's `tools`: 1 to 64 body names.
fn fill_tools<'de, D: Deserializer<'de>>(d: D) -> Result<Option<NameList>, D::Error> {
    names_in(d, 1..=MAX_TOOLS, TOOLS_MESSAGE)
}

fn fill_tools_schema(_: &mut SchemaGenerator) -> Schema {
    names_schema("The tool bodies whose surfaces bound the cells, by name: 1 to 64.", MAX_TOOLS)
}

/// Boundary Fill's `cells`: 1 to 1000 points, each inside one cell to fill. The handler skips an
/// item that isn't a point and refuses the call only when none is left; the contract refuses
/// any such item.
#[derive(Clone, Debug, PartialEq)]
pub struct FillCells(pub Vec<Value>);

impl<'de> Deserialize<'de> for FillCells {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Array(points) if (1..=MAX_CELLS).contains(&points.len()) && points.iter().all(|p| params::vec3(p).is_some()) => {
                Ok(FillCells(points))
            }
            _ => Err(D::Error::custom(whole(CELLS_MESSAGE))),
        }
    }
}

impl JsonSchema for FillCells {
    fn schema_name() -> Cow<'static, str> {
        "FillCells".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A point inside each cell to fill, 1 to 1000. A cell is a region inside some of the tools and outside the rest.",
            "type": "array",
            "items": generator.subschema_for::<Point3>(),
            "minItems": 1,
            "maxItems": MAX_CELLS
        })
    }
}

fn tools_and_cells_required(schema: &mut Schema) {
    require(schema, "tools");
    require(schema, "cells");
}

/// Add a solid made of the cells that tool bodies enclose, each picked by a point inside it, and
/// combine it with the design's bodies. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = tools_and_cells_required)]
pub struct BoundaryFillParams {
    // Required in the schema (`tools_and_cells_required`), optional in the type: left out, the
    // handler refuses the call with its own message, which the contract gives `null` and an
    // empty list (`present`: a `null` goes to `FillCells`, which refuses it).
    #[serde(default, deserialize_with = "fill_tools")]
    #[schemars(schema_with = "fill_tools_schema")]
    pub tools: Option<NameList>,
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "FillCells")]
    pub cells: Option<FillCells>,
    /// How the filled solid combines with the design's bodies. Default: new.
    #[serde(default)]
    pub operation: Option<OperationParam>,
    /// True: the tool bodies are removed once the cells are filled. Default: false.
    #[serde(default)]
    pub remove_tools: Option<bool>,
    /// What a join makes of pieces that share no face. Default: separate.
    #[serde(default)]
    pub touching: Option<TouchingParam>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Add a coil: a section swept along a helix. Give two of `revolutions` (or `turns`), `height` and
/// `pitch`. The section is either made from `diameter` and `section_size`, or taken from `sketch`.
/// One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CoilParams {
    /// The helix's diameter, greater than zero. Required without `sketch`; refused with it.
    #[serde(default)]
    pub diameter: Option<LengthExpr>,
    /// The number of turns, greater than zero; it may be fractional.
    #[serde(default)]
    pub revolutions: Option<NumberExpr>,
    /// The number of turns, when `revolutions` is not given.
    #[serde(default)]
    pub turns: Option<NumberExpr>,
    /// The height of the helix along `axis`. Not used when `revolutions` (or `turns`) and `pitch`
    /// are both given.
    #[serde(default)]
    pub height: Option<LengthExpr>,
    /// The distance along `axis` between one turn and the next.
    #[serde(default)]
    pub pitch: Option<LengthExpr>,
    /// The section's diameter when circular, or its side when square, greater than zero. The
    /// section must stay clear of the coil's axis. Required without `sketch`; refused with it.
    #[serde(default)]
    pub section_size: Option<LengthExpr>,
    /// Refused with `sketch`.
    #[serde(default)]
    pub section: Option<CoilSectionParam>,
    /// Refused with `sketch`.
    #[serde(default)]
    pub section_position: Option<CoilSectionPositionParam>,
    /// A point on the helix's axis, where it starts. Default: the origin.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointExpr3")]
    pub base: Option<PointExpr3>,
    /// The direction the helix climbs. Default: Z.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "AxisDir")]
    pub axis: Option<AxisDir>,
    /// Where around the axis the section starts. Default: 0. Refused with `sketch`.
    #[serde(default)]
    pub start_angle: Option<AngleExpr>,
    /// A left-handed coil, turning the other way while still climbing along `axis`. Default: false.
    #[serde(default)]
    pub clockwise: Option<bool>,
    /// A sketch whose profile is the section. Its plane is the section plane, so its placement
    /// sets the radius and the start phase.
    #[serde(default)]
    pub sketch: Option<SketchRef>,
    /// Which profile of `sketch` to sweep: exactly one closed profile without holes must result.
    /// Default: all of them. Needs `sketch`.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ProfilesParam")]
    pub profiles: Option<ProfilesParam>,
    /// How the coil combines with existing bodies. Default: new.
    #[serde(default)]
    pub operation: Option<OperationParam>,
    /// The bodies a join, cut or intersect works on. Default: every body in the active component.
    /// Ignored when `participants` lists bodies.
    #[serde(default)]
    pub targets: Option<StringList>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Raise a sketch's profiles from a face of the part, or sink them into it. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct EmbossParams {
    /// The sketch, by id or name. Default: the active sketch, else the last one.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "IdOrName")]
    pub sketch: Option<IdOrName>,
    /// Which profiles of the sketch to use. Default: all of them.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ProfilesParam")]
    pub profiles: Option<ProfilesParam>,
    /// How far the profiles are raised or sunk.
    pub depth: LengthExpr,
    #[serde(default)]
    pub mode: Option<EmbossModeParam>,
    /// The bodies to emboss, by name. With `face`, only these are searched for it.
    #[serde(default)]
    pub targets: Option<NameList>,
    /// A world point selecting a planar face or a complete exterior cylindrical wall. Default:
    /// inferred when exactly one supported tangent cylinder matches the selected profiles.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "EmbossFacePoint")]
    pub face: Option<EmbossFacePoint>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

#[cfg(test)]
#[path = "features_more_tests.rs"]
mod tests;
