//! Contracts and result types for the commands in `cmd/sketch_inspect.rs`: curvature combs,
//! minimum radius and isocurves on sketch curves, model edges and faces, and the centre of mass
//! of bodies (#10949).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use solidcraft_geom::Vec3;

use super::check::whole;
use super::types::{Point3, SketchRef, StringList, present};
use crate::params;

/// Points that pick edges or faces, read as the handler reads them: a list whose first `$limit`
/// items are each a point `params::vec3` reads (the handler looks at no more), with its message
/// for anything else and, where it refuses one, for an empty list.
macro_rules! point_list {
    ($(#[$doc:meta])* $name:ident, limit: $limit:literal, bad: $bad:literal, empty: $empty:expr) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name(pub Vec<Vec3>);

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let v = Value::deserialize(d)?;
                let points = v.as_array().ok_or_else(|| D::Error::custom(whole($bad)))?;
                let empty: Option<&str> = $empty;
                if let (true, Some(message)) = (points.is_empty(), empty) {
                    return Err(D::Error::custom(whole(message)));
                }
                points.iter().take($limit).map(params::vec3).collect::<Option<Vec<_>>>().map($name).ok_or_else(|| D::Error::custom(whole($bad)))
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
                let point = generator.subschema_for::<Point3>();
                let empty: Option<&str> = $empty;
                json_schema!({"type": "array", "items": point, "minItems": u32::from(empty.is_some())})
            }
        }
    };
}

point_list!(
    /// Points on model edges, each picking the nearest edge of any body (the first 1000 are used).
    EdgePoints,
    limit: 1000,
    bad: "`edges` must be points [x,y,z]",
    empty: None
);
point_list!(
    /// Points on faces, for combs along each face's isoparametric curves (the first 100 are used).
    CombFaces,
    limit: 100,
    bad: "`faces` must list points [x,y,z]",
    empty: Some("`faces` must list points [x,y,z]")
);
point_list!(
    /// Points on faces, each picking the nearest face of any body (the first 1000 are used).
    RadiusFaces,
    limit: 1000,
    bad: "`faces` must be points [x,y,z]",
    empty: None
);
/// The most faces one isocurve call shows.
pub const MAX_ISO_FACES: usize = 100;

const ISO_FACES: &str = "`faces` must list points [x,y,z] on faces";

/// Points on faces, each picking the nearest face of any body: 1 to [`MAX_ISO_FACES`] of them,
/// with the handler's messages.
#[derive(Clone, Debug, PartialEq)]
pub struct IsoFaces(pub Vec<Vec3>);

impl<'de> Deserialize<'de> for IsoFaces {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let points = v.as_array().ok_or_else(|| D::Error::custom(whole(ISO_FACES)))?;
        if points.is_empty() || points.len() > MAX_ISO_FACES {
            return Err(D::Error::custom(whole(format!("`faces` must list 1…{MAX_ISO_FACES} points [x,y,z] on faces"))));
        }
        points.iter().map(params::vec3).collect::<Option<Vec<_>>>().map(IsoFaces).ok_or_else(|| D::Error::custom(whole(ISO_FACES)))
    }
}

impl JsonSchema for IsoFaces {
    fn schema_name() -> Cow<'static, str> {
        "IsoFaces".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({"type": "array", "items": point, "minItems": 1, "maxItems": MAX_ISO_FACES})
    }
}

/// `true`, `false` or null, with the handler's message for anything else.
fn flag<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Bool(b) => Ok(Some(b)),
        _ => Err(D::Error::custom("must be true or false")),
    }
}

/// The comb's length multiplier, 0.01 to 100, with the handler's message for anything else;
/// `null` is none.
fn multiplier<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v => v
            .as_f64()
            .filter(|x| (0.01..=100.0).contains(x))
            .map(Some)
            .ok_or_else(|| D::Error::custom(whole("`length` must be a multiplier 0.01…100"))),
    }
}

fn multiplier_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["number", "null"], "minimum": 0.01, "maximum": 100})
}

/// Isocurves each way, a whole number 1 to `MAX_ISO_COUNT`, with the handler's message for
/// anything else; `null` is none.
fn iso_count<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    let max = crate::kernel::MAX_ISO_COUNT;
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v => v
            .as_f64()
            .filter(|c| c.fract() == 0.0 && (1.0..=max as f64).contains(c))
            .map(Some)
            .ok_or_else(|| D::Error::custom(whole(format!("`count` must be a whole number of curves each way, 1…{max}")))),
    }
}

fn iso_count_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["integer", "null"], "minimum": 1, "maximum": crate::kernel::MAX_ISO_COUNT})
}

/// A number, as `params::num` reads one (finite, under 1e9 in size), with the curvature comb's
/// message for anything else; `null` is none.
fn number<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v => v.as_f64().and_then(params::finite).map(Some).ok_or_else(|| D::Error::custom("must be a number")),
    }
}

fn number_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["number", "null"], "exclusiveMinimum": -1e9, "exclusiveMaximum": 1e9})
}

/// Curvature combs: teeth along each curve, edge or face isocurve, each as long as the curvature
/// there times `scale`, standing out on the convex side. Give `curves`, `edges` or `faces`, or
/// more than one.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CurvatureCombParams {
    /// Sketch curve ids (the first 1000 are used).
    #[serde(default)]
    pub curves: Option<StringList>,
    /// The sketch the curves are in (default: the one being edited). Read only with `curves`.
    #[serde(default)]
    pub sketch: Option<SketchRef>,
    #[serde(default)]
    pub edges: Option<EdgePoints>,
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "CombFaces")]
    pub faces: Option<CombFaces>,
    /// With `faces`: how many isoparametric curves each way (default 4; 1 to 50).
    #[serde(default, deserialize_with = "number")]
    #[schemars(schema_with = "number_schema")]
    pub count: Option<f64>,
    /// Teeth per curve (default 40; 2 to 2000).
    #[serde(default, deserialize_with = "number")]
    #[schemars(schema_with = "number_schema")]
    pub density: Option<f64>,
    /// Tooth length per unit of curvature (mm²). Default, or when not above 0: the longest tooth
    /// is a fifth of the size of everything combed.
    #[serde(default, deserialize_with = "number")]
    #[schemars(schema_with = "number_schema")]
    pub scale: Option<f64>,
    /// A multiplier on the automatic scale (default 1; 0.01 to 100). Ignored with `scale`.
    #[serde(default, deserialize_with = "multiplier")]
    #[schemars(schema_with = "multiplier_schema")]
    pub length: Option<f64>,
    /// Keep the combs shown and add these, every comb again at one scale. With nothing picked it
    /// draws those shown again with this call's options.
    #[serde(default, deserialize_with = "flag")]
    pub add: Option<bool>,
    /// Turn the analysis off.
    #[serde(default, deserialize_with = "flag")]
    pub clear: Option<bool>,
}

/// The smallest radius of curvature of each sketch curve, edge or face, and where it is. With
/// none of `curves`, `edges` and `faces`: every face of every body.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MinimumRadiusParams {
    /// Sketch curve ids (the first 1000 are used).
    #[serde(default)]
    pub curves: Option<StringList>,
    /// The sketch the curves are in (default: the one being edited). Read only with `curves`.
    #[serde(default)]
    pub sketch: Option<SketchRef>,
    #[serde(default)]
    pub edges: Option<EdgePoints>,
    #[serde(default)]
    pub faces: Option<RadiusFaces>,
}

/// The isoparametric curves of faces, u = const and v = const of each face's surface, cut to its
/// trims; `faces` unless `clear`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct IsocurveParams {
    #[serde(default)]
    pub faces: Option<IsoFaces>,
    /// How many curves each way (default 8; 1 to 100).
    #[serde(default, deserialize_with = "iso_count")]
    #[schemars(schema_with = "iso_count_schema")]
    pub count: Option<f64>,
    /// Keep the faces already shown.
    #[serde(default, deserialize_with = "flag")]
    pub add: Option<bool>,
    /// Turn the analysis off.
    #[serde(default, deserialize_with = "flag")]
    pub clear: Option<bool>,
}

/// The centre of mass of bodies, at uniform density, each and all together, where the bodies
/// are placed in the assembly.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CenterOfMassParams {
    /// The bodies, by name (default: all). A body whose volume can't be measured is left out.
    #[serde(default)]
    pub bodies: Option<StringList>,
}

/// What a view analysis's `clear` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AnalysisCleared {
    /// The analysis turned off.
    pub analysis: String,
    pub cleared: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `inspect.curvature_comb` returns: the combs, or the analysis turned off.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum CurvatureCombReply {
    Combs(CurvatureCombs),
    Cleared(AnalysisCleared),
}

/// The combs `inspect.curvature_comb` shows.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CurvatureCombs {
    /// One comb per curve, edge and face isocurve (those shown, with `add`).
    pub combs: Vec<Comb>,
    /// The tooth length per unit of curvature used for every comb (mm²).
    pub scale: f64,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One curvature comb.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Comb {
    /// What it combs: a sketch curve's id, `edge@x,y,z` or `face@x,y,z#k` (the point given, and
    /// the face's isocurve).
    pub curve: String,
    /// Each tooth, from the curve out to its tip.
    pub teeth: Vec<[[f64; 3]; 2]>,
    /// The teeth's tips, in order.
    pub envelope: Vec<[f64; 3]>,
    /// The largest curvature along it (1/mm).
    pub max_curvature: f64,
    /// Its smallest radius of curvature (mm); `null` when it is straight.
    pub min_radius: Option<f64>,
}

/// What `inspect.minimum_radius` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MinimumRadii {
    /// Each curve, edge or face looked at.
    pub items: Vec<RadiusItem>,
    /// The smallest of them all (mm); `null` when everything is straight or flat.
    pub min_radius: Option<f64>,
    /// Where that is; `null` with it.
    pub at: Option<[f64; 3]>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One curve, edge or face's smallest radius of curvature.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RadiusItem {
    /// A sketch curve's id, `edge@x,y,z`, `face@x,y,z` (the point given) or `<body>:face<n>`.
    pub item: String,
    /// Its smallest radius of curvature (mm); `null` when it is straight or flat.
    pub min_radius: Option<f64>,
    /// Where that is; only with a radius.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<[f64; 3]>,
}

/// What `inspect.isocurve` returns: the curves, or the analysis turned off.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum IsocurveReply {
    Curves(Isocurves),
    Cleared(AnalysisCleared),
}

/// The isocurves of the faces `inspect.isocurve` picked.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Isocurves {
    /// Each isoparametric curve as points along it, `count` each way per face.
    pub curves: Vec<Vec<[f64; 3]>>,
    /// Each face picked.
    pub faces: Vec<IsoFace>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One face whose isocurves are shown.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IsoFace {
    /// The body, by name.
    pub body: String,
    /// The face's index in the body.
    pub face: usize,
    /// Curves asked for each way.
    pub count: usize,
    /// Curves along u (v = const) and along v (u = const), as cut to the face.
    pub u_curves: usize,
    pub v_curves: usize,
}

/// What `inspect.center_of_mass` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CentersOfMass {
    /// The centre of mass of the bodies together.
    pub center: [f64; 3],
    /// Their volume together (mm³).
    pub volume: f64,
    /// Each body measured.
    pub bodies: Vec<BodyCenter>,
    /// The bodies left out: those that enclose no volume or could not be measured.
    pub skipped: Vec<SkippedBody>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One body's centre of mass.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BodyCenter {
    /// Its name, in the assembly.
    pub body: String,
    /// Its centre of mass.
    pub center: [f64; 3],
    /// Its volume (mm³).
    pub volume: f64,
}

/// A body the centre of mass leaves out, and why.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SkippedBody {
    pub body: String,
    pub reason: String,
}

#[cfg(test)]
#[path = "sketch_inspect_tests.rs"]
mod tests;
