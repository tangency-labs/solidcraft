//! Contracts and result types for the commands in `cmd/sketch_project.rs`: `sketch.project`,
//! `sketch.intersect`, `sketch.include_3d`, `sketch.project_to_surface`,
//! `sketch.intersection_curve`, `sketch.spun_profile`, `sketch.isoparametric_curve`,
//! `sketch.fit_curves_to_section`, `sketch.break_link` and `sketch.auto_project`.
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. What only the design can tell (a body, sketch or
//! curve that exists, an axis name, whether a reference projects to anything) stays the handler's
//! to check: a reference given as text, or as an axis name, passes here and is looked up there, and
//! so does a reference to a sketch, whose `curve` or `point` the handler checks once it has found
//! the sketch.
//! A call with several problems may get a different one first in strict than with the check off
//! (the check goes key by key, the handlers in their own order).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value, json};

use super::check::whole;
use super::motion::require;
use super::types::Point3;
use crate::cmd::{RefForm, ref_form};
use crate::params;

/// What a reference to model geometry may look like, for the descriptions.
const REF_FORMS: &str = "a reference: `{\"edge\": [x, y, z], \"body\"?}` (the edge through that point), `{\"face\": [x, y, z], \"body\"?}` (the edges of the face), `{\"vertex\": [x, y, z], \"body\"?}`, `{\"body\": name}` (its silhouette or outline), `{\"sketch\": id or name, \"curve\": id}` or `{\"sketch\": id or name, \"point\": id}` (a curve or point of another sketch), `\"origin\"` or `{\"origin\": …}`, `{\"axis\": \"X\"|\"Y\"|\"Z\"|a construction axis's name}`, `{\"plane\": \"XY\"|\"XZ\"|\"YZ\"|a plane's name}`; text alone is `origin`, X, Y, Z (any case) or a body's name";

/// The most references one `refs` takes into account: the handler reads no more.
const MAX_REFS: usize = 1000;

/// Whether text is what `cmd/sketch_project.rs` reads as the origin or an origin axis, which is
/// settled without the design; any other text may be a body's name.
fn is_origin_or_axis(name: &str) -> bool {
    ["origin", "x", "y", "z"].iter().any(|word| name.eq_ignore_ascii_case(word))
}

/// The handler's message for a reference that says nothing a design could be asked about, in its
/// own words, or `None` when the form is fine. A sketch reference with neither `curve` nor `point`
/// is fine here: the handler refuses it only once it has looked the sketch up, and says "no sketch"
/// first when there is none.
fn ref_problem(v: &Value) -> Option<String> {
    ref_form(v).err()
}

/// One reference of `sketch.project`, `sketch.intersect` and `sketch.include_3d`, kept as given
/// for the handler.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectRef(pub Value);

impl<'de> Deserialize<'de> for ProjectRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        match ref_problem(&v) {
            Some(why) => Err(D::Error::custom(whole(why))),
            None => Ok(ProjectRef(v)),
        }
    }
}

impl JsonSchema for ProjectRef {
    fn schema_name() -> Cow<'static, str> {
        "ProjectRef".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        let with_body = |key: &str| {
            json!({
                "type": "object",
                "properties": {key: point, "body": {"type": "string", "description": "The body's name. Default: any body. Text only: the handler reads anything else as left out, which the schema does not take."}},
                "required": [key]
            })
        };
        json_schema!({
            "description": REF_FORMS,
            "anyOf": [
                {"type": "string"},
                with_body("edge"),
                with_body("face"),
                with_body("vertex"),
                {
                    "type": "object",
                    "properties": {"sketch": {"type": ["number", "string"]}, "curve": {"type": "string"}},
                    "required": ["sketch", "curve"]
                },
                {
                    "type": "object",
                    "properties": {"sketch": {"type": ["number", "string"]}, "point": {"type": "string"}},
                    "required": ["sketch", "point"]
                },
                {"type": "object", "properties": {"body": {"type": "string"}}, "required": ["body"]},
                {"type": "object", "properties": {"axis": {"type": "string"}}, "required": ["axis"]},
                {"type": "object", "properties": {"plane": {"type": "string"}}, "required": ["plane"]},
                {"type": "object", "required": ["origin"]}
            ]
        })
    }
}

/// The references of `refs` (or `ref`): a list, or one reference. Only the first 1000 of a list
/// count, as the handler reads no more.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectRefs(pub Vec<Value>);

impl ProjectRefs {
    /// The references `v` lists, or the handler's message for them. A value that is not a list is
    /// one reference.
    fn read(v: &Value) -> Result<ProjectRefs, String> {
        let list: Vec<&Value> = match v {
            Value::Array(a) => a.iter().take(MAX_REFS).collect(),
            one => vec![one],
        };
        if list.is_empty() {
            return Err("`refs` is empty".into());
        }
        if let Some(why) = list.iter().find_map(|r| ref_problem(r)) {
            return Err(why);
        }
        Ok(ProjectRefs(list.into_iter().cloned().collect()))
    }
}

impl JsonSchema for ProjectRefs {
    fn schema_name() -> Cow<'static, str> {
        "ProjectRefs".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let one = generator.subschema_for::<ProjectRef>();
        json_schema!({
            "description": "One reference or a non-empty list of them (the first 1000 count).",
            "anyOf": [one, {"type": "array", "items": one, "minItems": 1}]
        })
    }
}

/// `refs`, or `ref` when `refs` is absent (the handler reads `ref` only then, `null` included).
fn read_refs(o: &Map<String, Value>) -> Result<(Option<ProjectRefs>, Option<ProjectRefs>), String> {
    match (o.get("refs"), o.get("ref")) {
        (Some(v), _) => Ok((Some(ProjectRefs::read(v)?), None)),
        (None, Some(v)) => Ok((None, Some(ProjectRefs::read(v)?))),
        (None, None) => Ok((None, None)),
    }
}

/// `projection_link`: a boolean, and `null` is refused too (the message is the same for both
/// commands that take it).
fn read_link_flag(o: &Map<String, Value>) -> Result<Option<bool>, String> {
    match o.get("projection_link") {
        None => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err("`projection_link` must be a boolean".into()),
    }
}

/// `construction`: a boolean (`null` is refused too); left out, the session's preference decides.
fn read_construction(o: &Map<String, Value>) -> Result<Option<bool>, String> {
    match o.get("construction") {
        None => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err("`construction` must be a boolean".into()),
    }
}

/// `refs` and `ref` are one of them required: left out, the handler refuses with its own message.
fn refs_required(schema: &mut Schema) {
    schema.insert("anyOf".into(), json!([{"required": ["refs"]}, {"required": ["ref"]}]));
}

/// Bring model edges, faces, bodies, other sketches' curves and points, the origin, an axis or a
/// plane into the active sketch as linked reference geometry: fixed, drawn purple and updated
/// when the upstream geometry changes. What the sketch already shows (the border of its face, an
/// edge projected before) is skipped and listed in `already_projected`; a call that adds nothing is
/// refused. One undo step.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = refs_required)]
pub struct ProjectParams {
    /// What to project: one reference or a list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ProjectRefs")]
    pub refs: Option<ProjectRefs>,
    /// The same as `refs`, used only when `refs` is not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(rename = "ref", with = "ProjectRefs")]
    pub reference: Option<ProjectRefs>,
    /// Keep the geometry linked to its source (true), or make it normal sketch geometry at once
    /// (false: the replies' `link` is then null). Default: true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub projection_link: Option<bool>,
    /// Make the created curves construction geometry (dashed, left out of profiles; they stay
    /// linked). Default: the "Project references as construction geometry" preference, off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub construction: Option<bool>,
}

impl<'de> Deserialize<'de> for ProjectParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let o = Map::<String, Value>::deserialize(d)?;
        // The handler reads `projection_link` before the references.
        let projection_link = read_link_flag(&o).map_err(|why| D::Error::custom(whole(why)))?;
        let (refs, reference) = read_refs(&o).map_err(|why| D::Error::custom(whole(why)))?;
        let construction = read_construction(&o).map_err(|why| D::Error::custom(whole(why)))?;
        Ok(ProjectParams { refs, reference, projection_link, construction })
    }
}

/// Intersect the sketch plane with model bodies, faces, edges and other sketches' curves, and
/// bring the section curves (or points) into the active sketch as linked reference geometry. One
/// undo step.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = refs_required)]
pub struct IntersectParams {
    /// What to intersect with the sketch plane: one reference or a list (`{body}`, `{face}`,
    /// `{edge}` or `{sketch, curve}` are the forms that make sense here).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ProjectRefs")]
    pub refs: Option<ProjectRefs>,
    /// The same as `refs`, used only when `refs` is not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(rename = "ref", with = "ProjectRefs")]
    pub reference: Option<ProjectRefs>,
    /// Keep the geometry linked to its source (true), or make it normal sketch geometry at once
    /// (false: the reply's `link` is then null). Default: true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub projection_link: Option<bool>,
    /// Make the created curves construction geometry (dashed, left out of profiles; they stay
    /// linked). Default: the "Project references as construction geometry" preference, off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub construction: Option<bool>,
}

impl<'de> Deserialize<'de> for IntersectParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let o = Map::<String, Value>::deserialize(d)?;
        let projection_link = read_link_flag(&o).map_err(|why| D::Error::custom(whole(why)))?;
        let (refs, reference) = read_refs(&o).map_err(|why| D::Error::custom(whole(why)))?;
        let construction = read_construction(&o).map_err(|why| D::Error::custom(whole(why)))?;
        Ok(IntersectParams { refs, reference, projection_link, construction })
    }
}

/// Include model edges, vertices, and other sketches' curves and points in the active sketch as
/// linked geometry; curves out of the sketch plane are flattened onto it. Takes the references of
/// `sketch.project`, and always keeps the link. One undo step.
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields, transform = refs_required)]
pub struct Include3dParams {
    /// What to include: one reference or a list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ProjectRefs")]
    pub refs: Option<ProjectRefs>,
    /// The same as `refs`, used only when `refs` is not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(rename = "ref", with = "ProjectRefs")]
    pub reference: Option<ProjectRefs>,
    /// Make the created curves construction geometry (dashed, left out of profiles; they stay
    /// linked). Default: the "Project references as construction geometry" preference, off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub construction: Option<bool>,
}

impl<'de> Deserialize<'de> for Include3dParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let o = Map::<String, Value>::deserialize(d)?;
        let (refs, reference) = read_refs(&o).map_err(|why| D::Error::custom(whole(why)))?;
        let construction = read_construction(&o).map_err(|why| D::Error::custom(whole(why)))?;
        Ok(Include3dParams { refs, reference, construction })
    }
}

/// A key that, once given (`null` included), must parse as `T`, as the handlers read it.
fn some<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

/// A point `[x, y, z]` or `{x, y, z}` with the handler's own message.
macro_rules! point_with_message {
    ($(#[$doc:meta])* $name:ident, $message:literal) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct $name(pub solidcore_record::Vec3);

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                params::vec3(&Value::deserialize(d)?).map($name).ok_or_else(|| D::Error::custom(whole($message)))
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                stringify!($name).into()
            }
            fn inline_schema() -> bool {
                true
            }
            fn json_schema(generator: &mut SchemaGenerator) -> Schema {
                generator.subschema_for::<Point3>()
            }
        }
    };
}

point_with_message!(
    /// `sketch.project_to_surface`'s `face`, with the handler's message.
    TargetFacePoint,
    "`face` must be a point [x,y,z] on the target face"
);
point_with_message!(
    /// `sketch.isoparametric_curve`'s `face`, with the handler's message.
    IsoFacePoint,
    "`face` must be a point [x,y,z] on the face"
);

/// A curve to project onto a surface: `{sketch, curve}`, read as the handler reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceCurve(pub Value);

impl SurfaceCurve {
    fn problem(v: &Value) -> Option<String> {
        const EACH: &str = "each entry must be {sketch, curve}";
        match ref_form(v) {
            Err(why) => Some(why),
            // What the handler says of a sketch reference comes after it has found the sketch.
            Ok(RefForm::Sketch { .. }) => None,
            // Whether text names a body, or an axis a construction axis, only the design knows;
            // either way it is not a curve.
            Ok(RefForm::Name(name)) if !is_origin_or_axis(name) => None,
            Ok(RefForm::Axis(name)) if !is_origin_or_axis(name) => None,
            Ok(_) => Some(EACH.into()),
        }
    }
}

/// The `curves` of `sketch.project_to_surface`: 1 to 500 of them.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceCurves(pub Vec<Value>);

impl<'de> Deserialize<'de> for SurfaceCurves {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let list = match Value::deserialize(d)? {
            Value::Array(list) => list,
            _ => return Err(D::Error::custom(whole("`curves` must list {sketch, curve}"))),
        };
        if list.is_empty() || list.len() > 500 {
            return Err(D::Error::custom(whole("`curves` must list 1…500 curves")));
        }
        if let Some(why) = list.iter().find_map(SurfaceCurve::problem) {
            return Err(D::Error::custom(whole(why)));
        }
        Ok(SurfaceCurves(list))
    }
}

impl JsonSchema for SurfaceCurves {
    fn schema_name() -> Cow<'static, str> {
        "SurfaceCurves".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "From 1 to 500 curves of other sketches, each {\"sketch\": id or name, \"curve\": curve id}.",
            "type": "array",
            "minItems": 1,
            "maxItems": 500,
            "items": {
                "type": "object",
                "properties": {"sketch": {"type": ["number", "string"]}, "curve": {"type": "string"}},
                "required": ["sketch", "curve"]
            }
        })
    }
}

/// Project curves of other sketches along the active sketch's normal onto a face of the model,
/// as linked 3D curves. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = surface_required)]
pub struct ProjectToSurfaceParams {
    // Optional in the type and required in the schema (`surface_required`): left out, the handler
    // refuses it with its own message, not the check's "is required".
    /// The curves to project: 1 to 500 of `{"sketch": id or name, "curve": id}`, curves of other
    /// sketches.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SurfaceCurves")]
    pub curves: Option<SurfaceCurves>,
    /// A point [x, y, z] in mm on the target face.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "TargetFacePoint")]
    pub face: Option<TargetFacePoint>,
    /// The face's body. Default: any body the point is on. Text only; anything else is read as
    /// left out.
    #[serde(default)]
    pub body: Option<String>,
}

fn surface_required(schema: &mut Schema) {
    require(schema, "curves");
    require(schema, "face");
}

/// One side of `sketch.intersection_curve`: a body, a face, or a curve of another sketch.
macro_rules! intersection_side {
    ($(#[$doc:meta])* $name:ident, $key:literal) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name(pub Value);

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let v = Value::deserialize(d)?;
                let wrong = || D::Error::custom(whole(concat!("`", $key, "` must be a body, a face or a curve of another sketch")));
                match ref_form(&v) {
                    Err(why) => Err(D::Error::custom(whole(why))),
                    // A sketch reference is judged by the handler once it has found the sketch.
                    Ok(RefForm::Body(_) | RefForm::Face { .. } | RefForm::Sketch { .. }) => Ok($name(v)),
                    // Text is a body's name, or the origin or an axis; an axis name is looked up.
                    Ok(RefForm::Name(name)) if !is_origin_or_axis(name) => Ok($name(v)),
                    Ok(RefForm::Axis(name)) if !is_origin_or_axis(name) => Ok($name(v)),
                    Ok(_) => Err(wrong()),
                }
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                stringify!($name).into()
            }
            fn json_schema(generator: &mut SchemaGenerator) -> Schema {
                let point = generator.subschema_for::<Point3>();
                json_schema!({
                    "description": "A body `{\"body\": name}` (or its name as text), a face `{\"face\": [x, y, z], \"body\"?}` (the face through that point), or a curve of another sketch `{\"sketch\": id or name, \"curve\": id}`, which stands for its extrusion along that sketch's normal.",
                    "anyOf": [
                        {"type": "string"},
                        {"type": "object", "properties": {"body": {"type": "string"}}, "required": ["body"]},
                        {
                            "type": "object",
                            "properties": {"face": point, "body": {"type": "string"}},
                            "required": ["face"]
                        },
                        {
                            "type": "object",
                            "properties": {"sketch": {"type": ["number", "string"]}, "curve": {"type": "string"}},
                            "required": ["sketch", "curve"]
                        }
                    ]
                })
            }
        }
    };
}

intersection_side!(
    /// `sketch.intersection_curve`'s `a`.
    IntersectionSideA,
    "a"
);
intersection_side!(
    /// `sketch.intersection_curve`'s `b`.
    IntersectionSideB,
    "b"
);

/// Add where two pieces of model geometry meet to the active sketch, as a linked 3D curve. One
/// undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = sides_required)]
pub struct IntersectionCurveParams {
    // Optional in the type and required in the schema, as `curves` above.
    /// The first side: a body, a face or a curve of another sketch.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "IntersectionSideA")]
    pub a: Option<IntersectionSideA>,
    /// The second side, of the same forms.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "IntersectionSideB")]
    pub b: Option<IntersectionSideB>,
}

fn sides_required(schema: &mut Schema) {
    require(schema, "a");
    require(schema, "b");
}

/// A body's name, with the handler's message for text it is not given as.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyName(pub String);

impl<'de> Deserialize<'de> for BodyName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(name) => Ok(BodyName(name)),
            _ => Err(D::Error::custom(whole("`body` must name a body"))),
        }
    }
}

impl JsonSchema for BodyName {
    fn schema_name() -> Cow<'static, str> {
        "BodyName".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A body's name.", "type": "string"})
    }
}

/// `sketch.spun_profile`'s `axis`: X, Y or Z (any case), or the id of a line of the active sketch.
#[derive(Clone, Debug, PartialEq)]
pub struct SpunAxis(pub String);

impl<'de> Deserialize<'de> for SpunAxis {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(axis) => Ok(SpunAxis(axis)),
            _ => Err(D::Error::custom(whole("`axis` must be a sketch line or X|Y|Z"))),
        }
    }
}

impl JsonSchema for SpunAxis {
    fn schema_name() -> Cow<'static, str> {
        "SpunAxis".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The axis to revolve about: X, Y or Z (any case: the origin's axis), or the id of a line of the active sketch.",
            "type": "string"
        })
    }
}

/// Add the outline of a body, revolved about an axis, to the active sketch as a linked 3D curve
/// in the sketch plane. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = spun_required)]
pub struct SpunProfileParams {
    // Optional in the type and required in the schema, as `curves` above.
    /// The body to revolve.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "BodyName")]
    pub body: Option<BodyName>,
    /// The axis to revolve about: X, Y or Z, or the id of a line of the active sketch.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SpunAxis")]
    pub axis: Option<SpunAxis>,
}

fn spun_required(schema: &mut Schema) {
    require(schema, "body");
    require(schema, "axis");
}

/// `sketch.isoparametric_curve`'s `direction`: `u` or `v` (any case), or a direction [x, y, z].
#[derive(Clone, Debug, PartialEq)]
pub struct IsoDirection(pub Value);

impl<'de> Deserialize<'de> for IsoDirection {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let word = matches!(&v, Value::String(d) if d.eq_ignore_ascii_case("u") || d.eq_ignore_ascii_case("v"));
        if word || params::vec3(&v).is_some() { Ok(IsoDirection(v)) } else { Err(D::Error::custom(whole("`direction` must be u, v or [x,y,z]"))) }
    }
}

impl JsonSchema for IsoDirection {
    fn schema_name() -> Cow<'static, str> {
        "IsoDirection".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "u or v (any case): along the face's parametric direction; or a direction [x, y, z].",
            "anyOf": [{"type": "string", "enum": ["u", "v", "U", "V"]}, point]
        })
    }
}

/// Add the curve of constant u or v through a point of a face to the active sketch, as a linked
/// 3D curve. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = face_required)]
pub struct IsoCurveParams {
    // Optional in the type and required in the schema, as `curves` above.
    /// A point [x, y, z] in mm on the face: the curve passes through it.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "IsoFacePoint")]
    pub face: Option<IsoFacePoint>,
    /// The face's body. Default: any body the point is on. Text only; anything else is read as
    /// left out.
    #[serde(default)]
    pub body: Option<String>,
    /// Which way the curve runs: u or v, or a direction [x, y, z]. Default: u.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "IsoDirection")]
    pub direction: Option<IsoDirection>,
}

fn face_required(schema: &mut Schema) {
    require(schema, "face");
}

/// Fit lines and arcs to the section of a mesh or solid body with the sketch plane, and add them
/// to the active sketch as normal (unlinked) geometry. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = body_required)]
pub struct FitCurvesParams {
    // Optional in the type and required in the schema, as `curves` above.
    /// The body to cut with the sketch plane.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "BodyName")]
    pub body: Option<BodyName>,
}

fn body_required(schema: &mut Schema) {
    require(schema, "body");
}

/// `sketch.break_link`'s `entities`: one id or a list of ids, as `params::string_list` reads them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EntityIds(pub Vec<String>);

impl<'de> Deserialize<'de> for EntityIds {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        match params::string_list("", &json!({"entities": v}), "entities") {
            Ok(ids) => Ok(EntityIds(ids)),
            Err(crate::EngineError::BadParams { msg, .. }) => Err(D::Error::custom(whole(msg))),
            Err(other) => Err(D::Error::custom(whole(other))),
        }
    }
}

impl JsonSchema for EntityIds {
    fn schema_name() -> Cow<'static, str> {
        "EntityIds".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The id of a curve or point, or a list of ids (at most 10000).",
            "anyOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}, "maxItems": 10000}]
        })
    }
}

/// Turn linked sketch geometry into normal sketch geometry, by its link or by any of its curves
/// and points. Give `link` or `entities`. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = link_or_entities)]
pub struct SketchBreakLinkParams {
    /// A link's id (`j1`). Text only; anything else is read as left out.
    #[serde(default)]
    pub link: Option<String>,
    /// The ids of linked curves or points: each one's whole link is broken. Default: none.
    #[serde(default)]
    pub entities: Option<EntityIds>,
}

/// The handler refuses a call with neither key (or an empty list), with its own message.
fn link_or_entities(schema: &mut Schema) {
    schema.insert("anyOf".into(), json!([{"required": ["link"]}, {"required": ["entities"]}]));
}

/// Choose whether a sketch created on a face projects that face's edges into it at once, or
/// toggle the choice. A setting the session keeps: not an undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AutoProjectParams {
    /// True to project, false not to. Default: the opposite of what it is now. Anything but true
    /// or false is read as left out.
    #[serde(default)]
    pub value: Option<bool>,
}

/// One link a projection made, and what it brought into the sketch.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectedLink {
    /// The link's id (`j1`); null when `projection_link` was false: the geometry is normal sketch
    /// geometry then.
    pub link: Option<String>,
    /// The ids of the curves it made.
    pub curves: Vec<String>,
    /// The ids of the points it made.
    pub points: Vec<String>,
}

/// What `sketch.project`, `sketch.intersect` and `sketch.include_3d` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectResult {
    /// One entry for each reference that added something, in order.
    pub links: Vec<ProjectedLink>,
    /// What was left out because the sketch shows it already (`sketch.project` only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub already_projected: Vec<String>,
    /// What the command added to the sketch, as every sketch command reports it (the engine adds
    /// it): `{points, curves, constraints, dimensions}`, and `wires` when there are any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The link one command made: what `sketch.intersection_curve`, `sketch.spun_profile` and
/// `sketch.isoparametric_curve` return, and each of the `links` of `sketch.project_to_surface`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LinkMade {
    /// The link's id (`j1`).
    pub link: String,
    /// The ids of the curves it made.
    pub curves: Vec<String>,
    /// The ids of the points it made.
    pub points: Vec<String>,
    /// The ids of the wires it made.
    pub wires: Vec<String>,
}

/// What `sketch.intersection_curve`, `sketch.spun_profile` and `sketch.isoparametric_curve`
/// return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SketchLinkResult {
    #[serde(flatten)]
    pub made: LinkMade,
    /// What the command added to the sketch, as every sketch command reports it (the engine adds
    /// it): `{points, curves, constraints, dimensions}`, and `wires` when there are any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.project_to_surface` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectToSurfaceResult {
    /// One link for each curve, in order.
    pub links: Vec<LinkMade>,
    /// What the command added to the sketch, as every sketch command reports it (the engine adds
    /// it): `{points, curves, constraints, dimensions}`, and `wires` when there are any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.fit_curves_to_section` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FitSectionResult {
    /// The ids of the curves added.
    pub curves: Vec<String>,
    /// What the command added to the sketch, as every sketch command reports it (the engine adds
    /// it): `{points, curves, constraints, dimensions}`, and `wires` when there are any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.break_link` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SketchBreakLinkResult {
    /// The ids of the links that were broken.
    pub broken: Vec<String>,
    /// What the command added to the sketch, as every sketch command reports it (the engine adds
    /// it): `{points, curves, constraints, dimensions}`, and `wires` when there are any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.auto_project` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AutoProjectResult {
    /// Whether a sketch created on a face projects its edges now.
    pub auto_project: bool,
    /// What the command added to the sketch, as every sketch command reports it (the engine adds
    /// it): `{points, curves, constraints, dimensions}`, and `wires` when there are any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "sketch_project_tests.rs"]
mod tests;
