//! Shared parameter types. Each one decides acceptance with the helper the handlers already
//! use, applied to the raw JSON value, so a contract can never accept a value its handler's own
//! reader would refuse. Their schemas describe the accepted forms.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use solidcore_record::{Vec2, Vec3};
use solidcraft_doc::{Expr, Operation, ThinSide, Touching};

use super::check::{keyed, whole};
use crate::params;

/// For optional fields whose key may be absent but not `null`: absent is `None` (with
/// `#[serde(default)]`), a present value, `null` included, goes to `T`'s deserializer. Pair it
/// with `#[schemars(with = "T")]` so the schema has no `null` branch.
pub fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

/// A coordinate's schema: a number below 1e9 in size, as `params::finite` requires.
fn coordinate() -> Value {
    json!({"type": "number", "exclusiveMinimum": -1e9, "exclusiveMaximum": 1e9})
}

/// The characters Rust's `trim` removes (Unicode `White_Space`), as ranges. A regex's `\s`
/// differs (it has U+FEFF and lacks U+0085), so the expression schema spells them out.
pub(crate) const WHITESPACE: [(char, char); 10] = [
    ('\u{9}', '\u{d}'),
    (' ', ' '),
    ('\u{85}', '\u{85}'),
    ('\u{a0}', '\u{a0}'),
    ('\u{1680}', '\u{1680}'),
    ('\u{2000}', '\u{200a}'),
    ('\u{2028}', '\u{2029}'),
    ('\u{202f}', '\u{202f}'),
    ('\u{205f}', '\u{205f}'),
    ('\u{3000}', '\u{3000}'),
];

/// [`WHITESPACE`] as the inside of a regex character class.
pub(crate) fn whitespace_class() -> String {
    let u = |c: char| format!("\\u{:04x}", c as u32);
    WHITESPACE.iter().map(|&(a, b)| if a == b { u(a) } else { format!("{}-{}", u(a), u(b)) }).collect()
}

/// A pattern for text that isn't blank once trimmed: a character outside [`WHITESPACE`].
pub(crate) fn not_blank_pattern() -> String {
    format!("[^{}]", whitespace_class())
}

/// `v` as the only key of an object, for the helpers that read `p.get(key)`.
fn wrap(v: Value) -> Value {
    json!({ "v": v })
}

macro_rules! expr_type {
    ($(#[$doc:meta])* $name:ident, $title:literal, $description:literal) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name(pub Expr);

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let v = wrap(Value::deserialize(d)?);
                params::expr(&v, "v").map($name).ok_or_else(|| D::Error::custom(params::expr_problem(v.get("v"))))
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                $title.into()
            }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                json_schema!({
                    "description": $description,
                    "anyOf": [
                        {"type": "number", "exclusiveMinimum": -1e9, "exclusiveMaximum": 1e9},
                        {"type": "string", "minLength": 1, "maxLength": 4096, "pattern": not_blank_pattern()}
                    ]
                })
            }
        }
    };
}

expr_type!(
    /// A length: a number, read in the design's length unit, or an expression such as
    /// `"w / 2"` or `"30 mm"`.
    LengthExpr,
    "LengthExpr",
    "A length: a number in the design's length unit, or an expression such as \"w / 2\" or \"30 mm\"."
);
expr_type!(
    /// An angle: a number of degrees, or an expression such as `"30 deg"` or `"a / 2"`.
    AngleExpr,
    "AngleExpr",
    "An angle: a number of degrees, or an expression such as \"30 deg\" or \"a / 2\"."
);
expr_type!(
    /// A plain number, or an expression such as `"k / 2"`: a ratio or a factor, with no unit.
    NumberExpr,
    "NumberExpr",
    "A number with no unit, or an expression such as \"k / 2\"."
);

/// Scale's `factors`: three factors, along X, Y and Z, each a [`NumberExpr`]. The handler takes
/// any three values, reading an item that isn't text as its JSON text (`true`, `null`), and
/// reports one that doesn't evaluate as a bad expression. Strict refuses, with this type's
/// message, a blank item, a number of 1e9 or more in size, text over 4 KB, or one that isn't a
/// number or an expression.
#[derive(Clone, Debug, PartialEq)]
pub struct ScaleFactors(pub [NumberExpr; 3]);

impl<'de> Deserialize<'de> for ScaleFactors {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Array(a) if a.len() == 3 => {
                let factors = a.into_iter().map(|v| params::expr(&wrap(v), "v").map(NumberExpr)).collect::<Option<Vec<_>>>();
                factors
                    .and_then(|f| <[NumberExpr; 3]>::try_from(f).ok())
                    .map(ScaleFactors)
                    .ok_or_else(|| D::Error::custom("must be three numbers or expressions"))
            }
            Value::Array(_) => Err(D::Error::custom(whole("`factors` must have three values"))),
            _ => Err(D::Error::custom("must be three numbers or expressions")),
        }
    }
}

impl JsonSchema for ScaleFactors {
    fn schema_name() -> Cow<'static, str> {
        "ScaleFactors".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let f = generator.subschema_for::<NumberExpr>();
        json_schema!({
            "description": "Factors along X, Y and Z: three numbers or expressions.",
            "type": "array",
            "items": f,
            "minItems": 3,
            "maxItems": 3
        })
    }
}

/// A point in mm: `[x, y, z]` or `{x, y, z}` of finite numbers (read by `params::vec3`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point3(pub Vec3);

impl<'de> Deserialize<'de> for Point3 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        params::vec3(&Value::deserialize(d)?).map(Point3).ok_or_else(|| D::Error::custom("must be a point [x, y, z] of numbers"))
    }
}

impl JsonSchema for Point3 {
    fn schema_name() -> Cow<'static, str> {
        "Point3".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let c = coordinate();
        json_schema!({
            "description": "A point in mm: [x, y, z] or {\"x\", \"y\", \"z\"}.",
            "anyOf": [
                {"type": "array", "items": c, "minItems": 3, "maxItems": 3},
                {"type": "object", "properties": {"x": c, "y": c, "z": c}, "required": ["x", "y", "z"]}
            ]
        })
    }
}

macro_rules! target_point {
    ($(#[$doc:meta])* $name:ident, $message:literal) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct $name(pub Vec3);

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                params::vec3(&Value::deserialize(d)?).map($name).ok_or_else(|| D::Error::custom($message))
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

target_point!(
    /// Extrude's `to`: a [`Point3`], with the handler's message.
    ToPoint,
    "must be a point [x, y, z] on the face, vertex or point to extrude to"
);
target_point!(
    /// Extrude's `from`: a [`Point3`], with the handler's message.
    FromPoint,
    "must be a point [x, y, z] on the face or point to start from"
);

target_point!(
    /// Revolve's `to`: a [`Point3`], with the handler's message.
    RevolveToPoint,
    "must be a point [x, y, z] to turn to"
);

/// Extrude's `faces`: 1 to 1024 points on coplanar faces of one body, read as the handler reads
/// them, with its messages.
#[derive(Clone, Debug, PartialEq)]
pub struct FacesParam(pub Vec<Vec3>);

impl<'de> Deserialize<'de> for FacesParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let faces = v
            .as_array()
            .filter(|faces| !faces.is_empty() && faces.len() <= 1024)
            .ok_or_else(|| D::Error::custom(whole("faces must contain between 1 and 1024 points")))?;
        faces
            .iter()
            .map(params::vec3)
            .collect::<Option<Vec<_>>>()
            .map(FacesParam)
            .ok_or_else(|| D::Error::custom(whole("faces must be points [x,y,z]")))
    }
}

impl JsonSchema for FacesParam {
    fn schema_name() -> Cow<'static, str> {
        "Faces".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "Points in mm, one on each of the coplanar faces of one body to extrude together.",
            "type": "array", "items": point, "minItems": 1, "maxItems": 1024
        })
    }
}

/// A direction given as an axis, as `features::axis_dir_param` reads it: X, Y or Z (trimmed, any
/// case) or a non-zero `[x, y, z]` (or `{x, y, z}`), with its messages.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisDir(pub Vec3);

impl<'de> Deserialize<'de> for AxisDir {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        crate::cmd::axis_dir_form(&Value::deserialize(d)?).map(AxisDir).map_err(|why| D::Error::custom(whole(why)))
    }
}

impl JsonSchema for AxisDir {
    fn schema_name() -> Cow<'static, str> {
        "AxisDir".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "An axis: X, Y or Z (any case), or a direction [x, y, z] that isn't zero.",
            "anyOf": [{"type": "string", "pattern": format!("^[{ws}]*[XYZxyz][{ws}]*$", ws = whitespace_class())}, point]
        })
    }
}

/// A camera, as the view commands read it (`cmd::view_navigation::camera_form`): the render
/// `Camera`, with finite fields, a positive distance and a field of view in [0, π), and the
/// handlers' message, "invalid camera", for anything else (#10898).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraParam(pub solidcraft_viewport::Camera);

impl<'de> Deserialize<'de> for CameraParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        crate::cmd::camera_form(&Value::deserialize(d)?).map(CameraParam).ok_or_else(|| D::Error::custom(whole("invalid camera")))
    }
}

impl JsonSchema for CameraParam {
    fn schema_name() -> Cow<'static, str> {
        "Camera".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A camera orbiting a target point, as the view commands give it back. Angles are in radians, measured in the frame of the up axis.",
            "type": "object",
            "properties": {
                "target": {"description": "The point the camera looks at, in mm: [x, y, z].", "type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3},
                "yaw": {"description": "Turn about the up axis; 0 looks along +Y, from the front.", "type": "number"},
                "pitch": {"description": "Elevation above the ground plane, from -π/2 (looking up) to π/2 (looking down).", "type": "number"},
                "roll": {"description": "Turn about the viewing direction; 0 keeps the up axis upright. Default: 0.", "type": "number", "default": 0.0},
                "distance": {"description": "From the target to the eye, in mm.", "type": "number", "exclusiveMinimum": 0},
                "fov": {"description": "Vertical field of view; 0 is orthographic. Default: 0.", "type": "number", "minimum": 0, "exclusiveMaximum": std::f64::consts::PI, "default": 0.0},
                "up": {"description": "The world's up axis. Default: z.", "enum": ["z", "y"], "default": "z"}
            },
            "required": ["target", "yaw", "pitch", "distance"]
        })
    }
}

/// A sketch point in mm: `[x, y]` or `{x, y}` of finite numbers (read by `params::vec2`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point2(pub Vec2);

impl<'de> Deserialize<'de> for Point2 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        params::vec2(&Value::deserialize(d)?).map(Point2).ok_or_else(|| D::Error::custom("must be a point [x, y] of numbers"))
    }
}

impl JsonSchema for Point2 {
    fn schema_name() -> Cow<'static, str> {
        "Point2".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let c = coordinate();
        json_schema!({
            "description": "A sketch point in mm: [x, y] or {\"x\", \"y\"}.",
            "anyOf": [
                {"type": "array", "items": c, "minItems": 2, "maxItems": 2},
                {"type": "object", "properties": {"x": c, "y": c}, "required": ["x", "y"]}
            ]
        })
    }
}

/// A placement point: `[x, y, z]` or `{x, y, z}`, each a number (mm) or a length expression,
/// as `features::point_param` reads it. Whether the expressions evaluate is checked by the
/// handler, which has the design.
#[derive(Clone, Debug, PartialEq)]
pub struct PointExpr3(pub [Expr; 3]);

impl<'de> Deserialize<'de> for PointExpr3 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let coords: Option<Vec<&Value>> = match &v {
            Value::Array(a) if a.len() == 3 => Some(a.iter().collect()),
            Value::Object(o) => ["x", "y", "z"].iter().map(|k| o.get(*k)).collect(),
            _ => None,
        };
        coords
            .and_then(|c| c.into_iter().map(|c| params::expr(&wrap(c.clone()), "v")).collect::<Option<Vec<Expr>>>())
            .and_then(|c| <[Expr; 3]>::try_from(c).ok())
            .map(PointExpr3)
            .ok_or_else(|| D::Error::custom("must be a point [x, y, z] of numbers or length expressions"))
    }
}

impl JsonSchema for PointExpr3 {
    fn schema_name() -> Cow<'static, str> {
        "PointExpr3".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let c = generator.subschema_for::<LengthExpr>();
        json_schema!({
            "description": "A point: [x, y, z] or {\"x\", \"y\", \"z\"}, each a number in mm or a length expression.",
            "anyOf": [
                {"type": "array", "items": c, "minItems": 3, "maxItems": 3},
                {"type": "object", "properties": {"x": c, "y": c, "z": c}, "required": ["x", "y", "z"]}
            ]
        })
    }
}

/// A feature referred to by its id (a number) or its name (a string).
#[derive(Clone, Debug, PartialEq)]
pub struct IdOrName(pub String);

impl<'de> Deserialize<'de> for IdOrName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Number(n) => Ok(IdOrName(n.to_string())),
            Value::String(s) => Ok(IdOrName(s)),
            _ => Err(D::Error::custom("must be an id or name")),
        }
    }
}

impl JsonSchema for IdOrName {
    fn schema_name() -> Cow<'static, str> {
        "IdOrName".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A feature's id (a number) or its name.", "type": ["number", "string"]})
    }
}

/// One name or a list of names, at most 10 000.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StringList(pub Vec<String>);

impl<'de> Deserialize<'de> for StringList {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let fail = || D::Error::custom("must be a name or a list of names");
        match Value::deserialize(d)? {
            Value::String(s) => Ok(StringList(vec![s])),
            Value::Array(a) if a.len() > 10_000 => Err(D::Error::custom("must have at most 10000 items")),
            Value::Array(a) => a
                .into_iter()
                .map(|v| if let Value::String(s) = v { Some(s) } else { None })
                .collect::<Option<Vec<_>>>()
                .map(StringList)
                .ok_or_else(fail),
            _ => Err(fail()),
        }
    }
}

impl JsonSchema for StringList {
    fn schema_name() -> Cow<'static, str> {
        "StringList".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A name, or a list of names.",
            "anyOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}, "maxItems": 10000}]
        })
    }
}

/// The spellings `Operation::parse` accepts, canonical ones first (it also ignores case, spaces,
/// '_' and '-', which an enum can't say).
const OPERATIONS: &[&str] = &[
    "new",
    "join",
    "cut",
    "intersect",
    "new_body",
    "newbody",
    "new_component",
    "newcomponent",
    "union",
    "add",
    "subtract",
    "difference",
    "intersection",
];

/// A string `ok` accepts. A string it refuses gets `unknown(s)` (the handler's own message,
/// marked [`whole`]); anything else, which the handlers ignore, "must be one of `expected`".
fn string_matching<'de, D: Deserializer<'de>>(
    d: D,
    ok: impl Fn(&str) -> bool,
    expected: &str,
    unknown: impl Fn(&str) -> String,
) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) if ok(&s) => Ok(s),
        Value::String(s) => Err(D::Error::custom(unknown(&s))),
        _ => Err(D::Error::custom(format!("must be one of {expected}"))),
    }
}

/// `features::operation`'s message for an operation it doesn't know.
fn unknown_operation(s: &str) -> String {
    whole(format!("unknown operation `{s}` (new, join, cut, intersect)"))
}

fn enum_schema(values: &[&str], description: &str) -> Schema {
    json_schema!({"description": description, "type": "string", "enum": values})
}

/// A keyword a handler reads with `str_` and lowercases, refusing any other word with `unknown
/// <what> `<word>` (<expected>)`; the lowercased word is kept for the handler.
macro_rules! keyword_param {
    ($(#[$doc:meta])* $name:ident, $what:literal, $values:expr, $expected:literal, $description:literal) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name(pub String);

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let ok = |s: &str| $values.contains(&s.to_ascii_lowercase().as_str());
                let unknown = |s: &str| whole(format!(concat!("unknown ", $what, " `{}` (", $expected, ")"), s.to_ascii_lowercase()));
                string_matching(d, ok, $expected, unknown).map(|s| $name(s.to_ascii_lowercase()))
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                stringify!($name).into()
            }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                enum_schema($values, $description)
            }
        }
    };
}

keyword_param!(
    /// `solid.rib`'s and `solid.web`'s `direction`.
    RibDirectionParam,
    "direction",
    &["in_plane", "plane", "normal"],
    "in_plane or normal",
    "Which way the wall grows from the curves: in_plane (the default; plane is an alias), across the sketch plane, or normal, along the sketch plane's normal. Matching ignores case."
);
keyword_param!(
    /// `solid.fillet`'s `type`.
    FilletTypeParam,
    "fillet type",
    &["constant", "rolling_ball", "chord", "chord_length", "curvature", "g2", "variable", "spline"],
    "constant, chord, variable, spline or curvature",
    "The round's section: constant (the default; rolling_ball is an alias), chord (`radius` is the width across; alias chord_length), curvature (alias g2: curvature-continuous between planar faces along straight edges, `radius` the equivalent circular setback), variable (`radius` at the end nearest `start`, `radius2` at the other) or spline (the radius a clamped B-spline from `start`: `radius`, then `radii`, over `knots`). Matching ignores case."
);
keyword_param!(
    /// `solid.fillet`'s `fit`.
    FilletFitParam,
    "fit",
    &["exact", "reduce", "taper"],
    "exact, reduce or taper",
    "What a radius too large for the geometry around the edges does: exact (the default) refuses it naming the largest that fits, reduce reduces it where it doesn't fit, taper follows the room along straight edges between planar faces. reduce and taper take type: constant. Matching ignores case."
);
keyword_param!(
    /// `solid.fillet`'s `corner`.
    FilletCornerParam,
    "corner",
    &["rolling", "g2"],
    "rolling or g2",
    "What fills a corner where three rounded edges meet: rolling (the default), the rolling ball's sphere, or g2, a set-back patch meeting the rounds to the second order (type: constant, fit: exact only). Matching ignores case."
);
keyword_param!(
    /// `solid.fillet`'s `turn`.
    FilletTurnParam,
    "turn",
    &["mitre", "swept"],
    "mitre or swept",
    "How two rounds turn round a sharp, straight, convex side edge square to both on a plane (the rounds along a boss's base, at its upright edges): mitre (the default) runs them on to meet in a mitre; swept ends them at the side edge and fills the corner with their section revolved about it, a torus. swept takes type: constant, corner: rolling, no setbacks and fit: exact or reduce, and is refused with the reason where the rounds are unequal or the side edge is concave, curved or tangent. Matching ignores case."
);
keyword_param!(
    /// `plastic.lip`'s `type`.
    LipTypeParam,
    "type",
    &["lip", "groove"],
    "lip or groove",
    "What the feature makes along the rim: lip (the default), a raised band, or groove, a band cut into the rim for another part's lip. Matching ignores case."
);
keyword_param!(
    /// `plastic.lip`'s `side`.
    LipSideParam,
    "side",
    &["inside", "outside"],
    "inside or outside",
    "Which edge of the rim face the band runs along: inside (the default) or outside. Matching ignores case."
);

/// How a feature combines with existing bodies, as `features::operation` reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct OperationParam(pub Operation);

impl<'de> Deserialize<'de> for OperationParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = string_matching(d, |s| Operation::parse(s).is_some(), "new, join, cut or intersect", unknown_operation)?;
        Operation::parse(&s).map(OperationParam).ok_or_else(|| D::Error::custom("must be one of new, join, cut or intersect"))
    }
}

impl JsonSchema for OperationParam {
    fn schema_name() -> Cow<'static, str> {
        "Operation".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        enum_schema(
            OPERATIONS,
            "How the feature combines with existing bodies: new, join, cut or intersect. The others are aliases. Matching ignores case, spaces, '_' and '-'.",
        )
    }
}

/// What a join makes of pieces that share no face, as `Touching::parse` reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct TouchingParam(pub Touching);

/// The feature commands' message for a `touching` they don't know.
const TOUCHING_MESSAGE: &str = "`touching` must be separate or one_body";

impl<'de> Deserialize<'de> for TouchingParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) => Touching::parse(&s).map(TouchingParam).ok_or_else(|| D::Error::custom(whole(TOUCHING_MESSAGE))),
            _ => Err(D::Error::custom(whole(TOUCHING_MESSAGE))),
        }
    }
}

impl JsonSchema for TouchingParam {
    fn schema_name() -> Cow<'static, str> {
        "Touching".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        enum_schema(
            &["separate", "one_body"],
            "What a join makes of pieces that share no face: separate (the default) makes each piece that touches another only along an edge or at a point a body of its own and keeps pieces that touch nothing in one body; one_body keeps every piece in one body.",
        )
    }
}

/// Extrude's operation: the above or `auto` (matched with `eq_ignore_ascii_case`, as the
/// handler does).
#[derive(Clone, Debug, PartialEq)]
pub struct ExtrudeOperation(pub String);

impl<'de> Deserialize<'de> for ExtrudeOperation {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let ok = |s: &str| s.eq_ignore_ascii_case("auto") || Operation::parse(s).is_some();
        string_matching(d, ok, "new, join, cut, intersect or auto", unknown_operation).map(ExtrudeOperation)
    }
}

impl JsonSchema for ExtrudeOperation {
    fn schema_name() -> Cow<'static, str> {
        "ExtrudeOperation".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let mut values = vec!["auto"];
        values.extend_from_slice(OPERATIONS);
        enum_schema(
            &values,
            "How the extrusion combines with existing bodies: auto (cut into a body, otherwise join; new in an empty component), new, join, cut or intersect. The others are aliases. Matching ignores case, spaces, '_' and '-' (only case for auto).",
        )
    }
}

/// What Combine's tools do to its target, as `features::combine` reads `operation`: a spelling
/// of join, cut or intersect that `Operation::parse` accepts. A spelling of new, or any other
/// word, gets the handler's message.
#[derive(Clone, Debug, PartialEq)]
pub struct CombineOperation(pub Operation);

/// The handler's message for an operation Combine doesn't take.
const COMBINE_OPERATION_MESSAGE: &str = "operation must be join, cut or intersect";

/// Combine's operation for `s`: `Operation::parse`'s, but not new.
fn combine_operation(s: &str) -> Option<Operation> {
    Operation::parse(s).filter(|o| *o != Operation::NewBody)
}

impl<'de> Deserialize<'de> for CombineOperation {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let ok = |s: &str| combine_operation(s).is_some();
        let s = string_matching(d, ok, "join, cut or intersect", |_| whole(COMBINE_OPERATION_MESSAGE))?;
        combine_operation(&s).map(CombineOperation).ok_or_else(|| D::Error::custom(whole(COMBINE_OPERATION_MESSAGE)))
    }
}

impl JsonSchema for CombineOperation {
    fn schema_name() -> Cow<'static, str> {
        "CombineOperation".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let values: Vec<&str> = OPERATIONS.iter().copied().filter(|s| combine_operation(s).is_some()).collect();
        enum_schema(
            &values,
            "What the tools do to the target: join (the default), cut or intersect. The others are aliases. Matching ignores case, spaces, '_' and '-'.",
        )
    }
}

const DIRECTIONS: &[&str] = &["positive", "negative", "symmetric", "one_side", "reverse", "flip"];

/// Extrude's direction, as `extent_param` reads it (lowercased, not trimmed).
#[derive(Clone, Debug, PartialEq)]
pub struct DirectionParam(pub String);

impl<'de> Deserialize<'de> for DirectionParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let ok = |s: &str| DIRECTIONS.contains(&s.to_ascii_lowercase().as_str());
        // The handler names the direction lowercased.
        let unknown = |s: &str| whole(format!("unknown direction `{}`", s.to_ascii_lowercase()));
        string_matching(d, ok, "positive, negative or symmetric", unknown).map(DirectionParam)
    }
}

impl JsonSchema for DirectionParam {
    fn schema_name() -> Cow<'static, str> {
        "Direction".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        enum_schema(
            DIRECTIONS,
            "Which way to extrude: positive (the default), negative, or symmetric (the distance is the total thickness, split evenly around the start plane). one_side is an alias of positive; reverse and flip of negative. Matching ignores case.",
        )
    }
}

/// The `extent` keyword: only `through_all` (ignoring case) is meaningful.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtentKeyword;

impl<'de> Deserialize<'de> for ExtentKeyword {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) if s.eq_ignore_ascii_case("through_all") => Ok(ExtentKeyword),
            _ => Err(D::Error::custom("must be through_all")),
        }
    }
}

impl JsonSchema for ExtentKeyword {
    fn schema_name() -> Cow<'static, str> {
        "ExtentKeyword".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        enum_schema(&["through_all"], "\"through_all\" (any case) is the same as through_all: true.")
    }
}

/// Extrude's distance convention, as `extent_param` reads it (serde, case-sensitive).
#[derive(Clone, Debug, PartialEq)]
pub struct ConventionParam(pub solidcraft_doc::ExtrudeConvention);

impl<'de> Deserialize<'de> for ConventionParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        serde_json::from_value(Value::deserialize(d)?).map(ConventionParam).map_err(|_| D::Error::custom("must be total_distance or legacy_per_side"))
    }
}

impl JsonSchema for ConventionParam {
    fn schema_name() -> Cow<'static, str> {
        "ExtrudeConvention".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        enum_schema(
            &["total_distance", "legacy_per_side"],
            "total_distance (the default): a symmetric distance is the total thickness. legacy_per_side keeps older designs' per-side meaning.",
        )
    }
}

/// A thin wall's side, as `ThinSide::parse` reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct ThinSideParam(pub ThinSide);

impl<'de> Deserialize<'de> for ThinSideParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = string_matching(
            d,
            |s| ThinSide::parse(s).is_some(),
            "center, inside, outside or two_sides",
            |s| whole(format!("unknown side `{s}` (center, inside, outside or two_sides)")),
        )?;
        ThinSide::parse(&s).map(ThinSideParam).ok_or_else(|| D::Error::custom("must be one of center, inside, outside or two_sides"))
    }
}

impl JsonSchema for ThinSideParam {
    fn schema_name() -> Cow<'static, str> {
        "ThinSide".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        enum_schema(
            &["center", "inside", "outside", "two_sides", "centre", "centered", "centred", "symmetric", "side1", "side_1", "side2", "side_2", "both"],
            "Which side of the curves the wall lies on: center (the default), inside, outside or two_sides. The others are aliases. Matching ignores case and surrounding spaces, and reads ' ' and '-' as '_'.",
        )
    }
}

/// Which sketch profiles a feature uses. Accepted exactly when `features::profiles` accepts
/// it; kept as the raw value for the handler.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfilesParam(pub Value);

impl<'de> Deserialize<'de> for ProfilesParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        match crate::cmd::feature_profiles(&json!({ "profiles": v }), "profiles") {
            Ok(_) => Ok(ProfilesParam(v)),
            // The helper's messages as they are, apart from two terse ones, which get the
            // contract's `must be` wording.
            Err(crate::EngineError::BadParams { msg, .. }) => Err(D::Error::custom(match msg.as_str() {
                "profile index" => "must be \"all\", an index or a list".to_string(),
                "too many profiles" => "must have at most 10000 items".to_string(),
                _ => whole(msg),
            })),
            Err(e) => Err(D::Error::custom(e.to_string())),
        }
    }
}

impl JsonSchema for ProfilesParam {
    fn schema_name() -> Cow<'static, str> {
        "Profiles".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point2>();
        let curves = json!({"type": "array", "items": {"type": "string"}, "minItems": 1});
        let named = json!({"anyOf": [{"type": "string"}, curves]});
        let list = |items: Value| json!({"type": "array", "minItems": 1, "maxItems": 10000, "items": items});
        // An index is what the helper reads as a u64.
        let index = json!({"type": "integer", "minimum": 0, "maximum": u64::MAX});
        let loops = json!({"type": "array", "minItems": 1, "items": {
            "type": "object", "properties": {"outer": {"type": "boolean"}, "curves": named}, "required": ["curves"]
        }});
        let associations = json!({
            "type": "array", "minItems": 1, "maxItems": 128, "uniqueItems": true,
            "items": {"type": "string", "minLength": 1, "maxLength": 512}
        });
        // A list is all indices, all points, or all curve selections, as the helper reads it;
        // the `not`s give its precedence (a list whose items all have `point` is points, an
        // item's `curves` wins over its `loops`; `type: whole_boundaries` wins over an
        // `offset_boundaries` key, which wins over `type: offset_boundaries`). Narrower than
        // the helper in one way: every curve id and every loop must be well formed, where the
        // helper drops non-text ids and reads only the outer loop.
        let curve_object = json!({"type": "object", "properties": {"curves": named}, "required": ["curves"]});
        let loops_object = json!({
            "type": "object", "properties": {"loops": loops}, "required": ["loops"], "not": {"required": ["curves"]}
        });
        let curve_list = json!({
            "type": "array", "minItems": 1, "maxItems": 10000, "items": {"anyOf": [curves, curve_object, loops_object]},
            "not": {"items": {"type": "object", "required": ["point"]}}
        });
        json_schema!({
            "description": "Which sketch profiles to use: \"all\" (the default, any case), an index, or a list of indices, of {\"point\": [x, y]} inside a profile, or of curve selections (a list of curve ids, {\"curves\": [ids]} or {\"loops\": [...]}). Picks by index or point are stored by the curves around the profile.",
            "anyOf": [
                {"type": "string", "pattern": "^[Aa][Ll][Ll]$"},
                index.clone(),
                list(index),
                list(json!({"type": "object", "properties": {"point": point}, "required": ["point"]})),
                curve_list,
                {
                    "description": "Internal selections written by the app.",
                    "anyOf": [
                        {"type": "object", "properties": {"type": {"enum": ["whole_boundaries"]}}, "required": ["type"]},
                        {
                            "type": "object", "properties": {"offset_boundaries": associations}, "required": ["offset_boundaries"],
                            "not": {"properties": {"type": {"enum": ["whole_boundaries"]}}, "required": ["type"]}
                        },
                        {
                            "type": "object", "properties": {"type": {"enum": ["offset_boundaries"]}, "associations": associations},
                            "required": ["type", "associations"], "not": {"required": ["offset_boundaries"]}
                        }
                    ]
                }
            ]
        })
    }
}

/// A face given to a command, as `face::face_ref` reads it: a point on the face (`[x, y, z]`,
/// `{x, y, z}` or `{point}`) or its name as `model.faces` lists it (1 to 512 bytes; the
/// schema's `maxLength` counts characters, so it can't say bytes). Whether it finds a face is
/// checked by the handler, which has the design. (Not `solidcraft_doc::FacePick`, the stored
/// pick.)
#[derive(Clone, Debug, PartialEq)]
pub enum FaceRef {
    Point(Vec3),
    Name(String),
}

impl<'de> Deserialize<'de> for FaceRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match crate::cmd::face_ref_form(&Value::deserialize(d)?) {
            Ok(crate::cmd::FaceRefForm::Point(p)) => Ok(FaceRef::Point(p)),
            Ok(crate::cmd::FaceRefForm::Name(n)) => Ok(FaceRef::Name(n.to_string())),
            Err(why) => Err(D::Error::custom(whole(why))),
        }
    }
}

impl JsonSchema for FaceRef {
    fn schema_name() -> Cow<'static, str> {
        "FaceRef".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "A face: a point in mm on it ([x, y, z], {\"x\", \"y\", \"z\"} or {\"point\": [x, y, z]}), or its name as model.faces lists it (at most 512 bytes of UTF-8).",
            "anyOf": [
                point,
                {"type": "object", "properties": {"point": point}, "required": ["point"]},
                {"type": "string", "minLength": 1, "maxLength": 512}
            ]
        })
    }
}

/// An edge given to a fillet or chamfer, as `features::edge_points` reads it: a point on the
/// edge (`[x, y, z]`, `{x, y, z}` or `{point}`), else `{body, index}` in the body's edge order.
/// The index is whole-number JSON, as `Value::as_u64` reads it: JSON Schema also counts `1.0`
/// as an integer, which the reader refuses. Whether it finds an edge is checked by the handler,
/// which has the design. (Not `solidcraft_doc::EdgeRef`, the stored edge position.)
#[derive(Clone, Debug, PartialEq)]
pub enum EdgeRef {
    Point(Vec3),
    Index { body: String, index: u64 },
}

impl<'de> Deserialize<'de> for EdgeRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match crate::cmd::edge_ref_form(&Value::deserialize(d)?) {
            Ok(crate::cmd::EdgeRefForm::Point(p)) => Ok(EdgeRef::Point(p)),
            Ok(crate::cmd::EdgeRefForm::Index { body, index }) => Ok(EdgeRef::Index { body: body.to_string(), index }),
            Err(why) => Err(D::Error::custom(whole(why))),
        }
    }
}

impl JsonSchema for EdgeRef {
    fn schema_name() -> Cow<'static, str> {
        "EdgeRef".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "An edge: a point in mm on it ([x, y, z], {\"x\", \"y\", \"z\"} or {\"point\": [x, y, z]}), or {\"body\", \"index\"} in the body's edge order (an index written without a fraction).",
            "anyOf": [
                point,
                {"type": "object", "properties": {"point": point}, "required": ["point"]},
                {
                    "type": "object",
                    "properties": {"body": {"type": "string"}, "index": {"type": "integer", "minimum": 0, "maximum": u64::MAX}},
                    "required": ["body", "index"]
                }
            ]
        })
    }
}

/// A list of 1 to 1000 picks, with the handler's messages for the list itself. Like the handler,
/// it refuses `null`: an optional field of this type takes `present`.
///
/// The handler looks each pick up in the design before reading the next, so a call with a pick
/// that finds nothing and a later malformed one fails today on the first and in strict on the
/// second.
macro_rules! pick_list {
    ($(#[$doc:meta])* $name:ident, $item:ty, $not_list:literal, $count:literal, $description:literal) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name(pub Vec<$item>);

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let v = Value::deserialize(d)?;
                let items = v.as_array().ok_or_else(|| D::Error::custom(whole($not_list)))?;
                if items.is_empty() || items.len() > 1000 {
                    return Err(D::Error::custom(whole($count)));
                }
                items.iter().map(|item| <$item>::deserialize(item).map_err(D::Error::custom)).collect::<Result<_, _>>().map($name)
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                stringify!($name).into()
            }
            fn json_schema(generator: &mut SchemaGenerator) -> Schema {
                let item = generator.subschema_for::<$item>();
                json_schema!({"description": $description, "type": "array", "items": item, "minItems": 1, "maxItems": 1000})
            }
        }
    };
}

pick_list!(
    /// The `faces` of `solid.shell`, `solid.draft`, `solid.offset_face`, `solid.split_face` and
    /// `face.delete`, as `features::face_points` reads them; `solid.shell` also takes an empty
    /// list (a closed hollow body). (Extrude's `faces` is a [`FacesParam`].)
    FaceRefs,
    FaceRef,
    "`faces` must be a list of points or face names",
    "select 1…1000 faces",
    "The faces, 1 to 1000."
);
pick_list!(
    /// The `edges` of `solid.fillet` and `solid.chamfer`, as `features::edge_points` reads them.
    EdgeRefs,
    EdgeRef,
    "`edges` must be a list",
    "select 1…1000 edges",
    "The edges, 1 to 1000."
);

/// A face of a body whose surface, extended, cuts like a plane: the `tool` of `solid.split_body`,
/// `solid.split_face` and `surface.trim`, which read `tool.body` and then `tool.point` with these
/// messages. Other keys in it are ignored, as the handlers ignore them. Whether the body exists
/// and the point is on one of its faces is the handler's to check.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolFace {
    pub body: String,
    pub point: Vec3,
}

impl<'de> Deserialize<'de> for ToolFace {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        // In the handlers' order: the body (anything but an object has none), then the point.
        let body = v.get("body").and_then(Value::as_str).ok_or_else(|| D::Error::custom(whole("`tool.body` must name a body")))?;
        let point =
            v.get("point").and_then(params::vec3).ok_or_else(|| D::Error::custom(whole("`tool.point` must be [x, y, z] on the tool face")))?;
        Ok(ToolFace { body: body.to_string(), point })
    }
}

impl JsonSchema for ToolFace {
    fn schema_name() -> Cow<'static, str> {
        "ToolFace".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "A face of a body, its surface extended: the body's name and a point in mm on the face.",
            "type": "object",
            "properties": {"body": {"type": "string"}, "point": point},
            "required": ["body", "point"]
        })
    }
}

/// A sketch referred to by its feature id (a number) or its name, as `features::sketch_id` reads
/// it, with its message for a top-level key (`path_sketch`, `solid.split_face`'s `sketch`…);
/// for a nested one (a section's `sketch`) the message names the key as `sections[0].sketch`
/// where the handler says `sections[].sketch`. The `sketch` that `features::feature_sketch`
/// reads (extrude's, revolve's…) has another message and is an [`IdOrName`]. Where the reader
/// refuses `null`, an optional field of this type takes `present`. Whether a sketch has that id
/// or name is checked by the handler.
#[derive(Clone, Debug, PartialEq)]
pub struct SketchRef(pub String);

impl<'de> Deserialize<'de> for SketchRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        crate::cmd::sketch_key(Some(&Value::deserialize(d)?)).map(SketchRef).ok_or_else(|| D::Error::custom("must be a sketch id or name"))
    }
}

impl JsonSchema for SketchRef {
    fn schema_name() -> Cow<'static, str> {
        "SketchRef".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A sketch's feature id (a number) or its name.", "type": ["number", "string"]})
    }
}

/// A sweep's `orientation`, as `features::sweep_orientation` reads it, with its messages.
/// `null` is the default, as the handler reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SweepOrientationParam(pub solidcraft_doc::SweepOrientation);

impl<'de> Deserialize<'de> for SweepOrientationParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        crate::cmd::sweep_orientation(&Value::deserialize(d)?).map(SweepOrientationParam).map_err(|e| D::Error::custom(whole(e)))
    }
}

impl JsonSchema for SweepOrientationParam {
    fn schema_name() -> Cow<'static, str> {
        "SweepOrientation".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "How the profile turns as it follows the path: perpendicular (the default), square to the path and turning with it; parallel, as drawn, moving without turning (the path must keep crossing the profile's plane); or a direction, [x, y, z], {\"x\", \"y\", \"z\"} or {\"direction\": [x, y, z]}, not zero: square to the path, its turn about the path fixed so that the direction's projection onto the profile stays where it was at the start. Matching ignores case.",
            "anyOf": [
                {"type": "string", "enum": ["perpendicular", "parallel"]},
                point,
                {"type": "object", "properties": {"direction": point}, "required": ["direction"]}
            ]
        })
    }
}

/// A sweep's `twist`: an expression as [`AngleExpr`] reads one, with the handler's message for
/// anything else. Whether it evaluates, and to within four turns, is the handler's to check.
#[derive(Clone, Debug, PartialEq)]
pub struct SweepTwist(pub Expr);

impl<'de> Deserialize<'de> for SweepTwist {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        params::expr(&wrap(Value::deserialize(d)?), "v")
            .map(SweepTwist)
            .ok_or_else(|| D::Error::custom(whole("`twist` must be an angle or an expression")))
    }
}

/// Guide rails' `rail_fit`, as `features::rail_fit_form` reads it, with its message.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RailFitParam(pub solidcraft_doc::RailFit);

impl<'de> Deserialize<'de> for RailFitParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        crate::cmd::rail_fit_form(&Value::deserialize(d)?).map(RailFitParam).map_err(|e| D::Error::custom(whole(e)))
    }
}

impl JsonSchema for RailFitParam {
    fn schema_name() -> Cow<'static, str> {
        "RailFit".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        enum_schema(
            &["scale", "stretch"],
            "How guide rails reshape the profile: scale (the default) grows and shrinks it evenly with the distance from the path to the rail (between the rails); stretch only across to the rail (between the rails). Matching ignores case.",
        )
    }
}

/// What `features::guide_rails` says about a sweep's or a loft's (`what`) `rails`, as given (not
/// `null`), before it looks a sketch up: its message, or `None`. `default_sketch`: a rail may
/// leave its `sketch` out. A key a rail doesn't take, which the handler ignores, is refused too.
fn guide_rails_problem(v: &Value, what: &str, default_sketch: bool) -> Option<String> {
    let Value::Array(given) = v else { return Some(whole("`rails` must list one or two rails: [{sketch, curves: [ids]}]")) };
    if !(1..=2).contains(&given.len()) {
        return Some(whole(format!("a {what} takes one or two guide rails")));
    }
    for (i, rail) in given.iter().enumerate() {
        match rail.get("sketch").filter(|v| !v.is_null()) {
            None if !default_sketch => return Some(whole("each rail needs the `sketch` its curves are in")),
            Some(sketch) if crate::cmd::sketch_key(Some(sketch)).is_none() => return Some(whole("`rails[].sketch` must be a sketch id or name")),
            _ => {}
        }
        let curves = match params::string_list(params::UNKNOWN_COMMAND, rail, "curves") {
            Ok(curves) => curves,
            Err(crate::EngineError::BadParams { msg, .. }) => return Some(whole(msg)),
            Err(e) => return Some(whole(e)),
        };
        if curves.is_empty() || curves.len() > 1000 {
            return Some(whole("each rail's `curves` must list 1…1000 curve ids in order"));
        }
        if let Some(key) = rail.as_object().and_then(|o| o.keys().find(|k| !matches!(k.as_str(), "sketch" | "curves"))) {
            let key = super::check::truncate(key, 64);
            return Some(whole(format!("unknown key `{key}` in `rails[{i}]`: a rail takes `sketch` and `curves`")));
        }
    }
    None
}

/// A sweep's guide `rails`, as `features::guide_rails` reads them before it looks the sketches up,
/// with its messages: one or two `{sketch?, curves}`, each rail's sketch the path's when it
/// names none. Kept as given, for the handler. Whether the sketches and curves exist, and that
/// `orientation`, `twist` and `scale` are left out, is the handler's to check.
#[derive(Clone, Debug, PartialEq)]
pub struct SweepRails(pub Value);

impl<'de> Deserialize<'de> for SweepRails {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        match guide_rails_problem(&v, "sweep", true) {
            Some(why) => Err(D::Error::custom(why)),
            None => Ok(SweepRails(v)),
        }
    }
}

impl JsonSchema for SweepRails {
    fn schema_name() -> Cow<'static, str> {
        "SweepRails".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let sketch = generator.subschema_for::<SketchRef>();
        json_schema!({
            "description": "One or two guide rails, each {\"sketch\"?: the sketch its curves are in (default: the path's sketch), \"curves\": its curve ids in order, 1 to 1000 (one id may be given as text)}.",
            "type": "array",
            "minItems": 1,
            "maxItems": 2,
            "items": {
                "type": "object",
                "properties": {
                    "sketch": {"anyOf": [sketch, {"type": "null"}]},
                    "curves": {"anyOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": 1000}]}
                },
                "required": ["curves"],
                "additionalProperties": false
            }
        })
    }
}

/// What `features::plane_param` says about `v` as a plane before it looks anything up, as a
/// complete message, or `None`. Names and ids are the handler's to look up.
fn plane_problem(v: &Value) -> Option<String> {
    let forms = || whole(format!("`plane` must be {}", crate::cmd::PLANE_FORMS));
    match v {
        Value::String(_) => None,
        Value::Number(n) => n.as_u64().map_or_else(|| Some(forms()), |_| None),
        Value::Object(o) => {
            // The handler evaluates each coordinate after reading it; an expression that doesn't
            // evaluate is its to report.
            if o.get("origin").is_some_and(|origin| PointExpr3::deserialize(origin).is_err()) {
                return Some(whole("`origin` must be a point [x, y, z] of numbers or length expressions"));
            }
            crate::cmd::explicit_plane(v, Vec3::ZERO).map_or_else(|| Some(whole("plane needs x_dir and y_dir, or normal")), |_| None)
        }
        _ => Some(forms()),
    }
}

/// A plane, as `features::plane_param` reads it: an origin plane's name or alias, a construction
/// plane's name or feature id, or an explicit plane object. Kept as given, for the handler.
/// Whether a name or id names a plane is checked by the handler, which has the design.
#[derive(Clone, Debug, PartialEq)]
pub struct PlaneParam(pub Value);

impl<'de> Deserialize<'de> for PlaneParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        match plane_problem(&v) {
            Some(why) => Err(D::Error::custom(why)),
            None => Ok(PlaneParam(v)),
        }
    }
}

impl JsonSchema for PlaneParam {
    fn schema_name() -> Cow<'static, str> {
        "Plane".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        let origin = generator.subschema_for::<PointExpr3>();
        json_schema!({
            "description": "A plane: XY, XZ or YZ (or Top, Front or Right; any case), a construction plane's name or feature id, or an explicit plane {\"origin\"?, \"x_dir\", \"y_dir\"} or {\"origin\"?, \"normal\"}: directions [x, y, z] (two that aren't parallel, or a normal that isn't zero; the two directions win over normal when both are points) and an origin in mm, each coordinate a number or a length expression (default: the origin).",
            "anyOf": [
                {"type": "string"},
                {"type": "integer", "minimum": 0, "maximum": u64::MAX},
                {
                    "type": "object",
                    "properties": {"origin": origin},
                    "anyOf": [
                        {"properties": {"x_dir": point, "y_dir": point}, "required": ["x_dir", "y_dir"]},
                        {"properties": {"normal": point}, "required": ["normal"]}
                    ]
                }
            ]
        })
    }
}

/// What `construct::geo` says about `v` as a reference before it looks anything up, in the
/// contract's style (the key goes in front), or `None`. Sketches, axes and construction points
/// are the handler's to look up.
fn geo_problem(v: &Value) -> Option<String> {
    use crate::cmd::{GeoForm, GeoFormError};
    match crate::cmd::geo_form(v) {
        Ok(GeoForm::Sketch { sketch, part }) => {
            if crate::cmd::sketch_key(Some(sketch)).is_none() {
                Some("must be a sketch id or name".into())
            } else {
                part.is_none().then(|| crate::cmd::SKETCH_PART.into())
            }
        }
        Ok(GeoForm::Plane(plane)) => plane_problem(plane),
        Ok(_) => None,
        Err(GeoFormError::Saved(why)) => Some(keyed(why)),
        Err(GeoFormError::Unknown) => Some(format!("must be {}", crate::cmd::GEO_REF_FORMS)),
    }
}

/// The reference types a saved reference's `type` names (`solidcraft_doc::construct::GeoRef`).
const SAVED_REFS: [&str; 11] =
    ["point", "origin", "vertex", "edge", "face", "plane", "world", "axis", "construct_point", "sketch_point", "sketch_curve"];

/// `{key: point}`, unless it has `type` (then it is a saved reference).
fn picked_at(key: &str, point: &Schema) -> Value {
    json!({"type": "object", "properties": {key: point}, "required": [key], "not": {"required": ["type"]}})
}

/// A reference to geometry for a construction axis, point or plane, as `construct::geo` reads
/// it. Kept as given, for the handler. What the reference must be (a plane, a line…) depends on
/// the key and is checked when the feature is evaluated; whether a sketch, axis or construction
/// point it names exists is checked by the handler.
#[derive(Clone, Debug, PartialEq)]
pub struct GeoRefParam(pub Value);

impl<'de> Deserialize<'de> for GeoRefParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        match geo_problem(&v) {
            Some(why) => Err(D::Error::custom(why)),
            None => Ok(GeoRefParam(v)),
        }
    }
}

impl JsonSchema for GeoRefParam {
    fn schema_name() -> Cow<'static, str> {
        "GeoRef".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        let plane = generator.subschema_for::<PlaneParam>();
        let sketch = generator.subschema_for::<SketchRef>();
        let text = json!({"type": "string"});
        // The reader takes the first form that fits, in the order below; the `not`s keep a later
        // form from taking an object an earlier one refuses.
        let earlier = |keys: &[&str]| json!({"anyOf": keys.iter().map(|k| json!({"required": [k]})).collect::<Vec<_>>()});
        json_schema!({
            "description": "Geometry to build from, the first of these forms that fits: a reference as saved ({\"type\": …}, as timeline.inspect gives one back); a point in mm on a face, edge or vertex of a body ({\"face\": [x, y, z]}, {\"edge\": …} or {\"vertex\": …}), kept by its persistent name; a sketch's point or curve ({\"sketch\": id or name, \"point\" or \"curve\": its id}); a fixed point {\"point\": [x, y, z]}; a plane {\"plane\": …}; an axis {\"axis\": X, Y, Z or a construction axis's name}; a construction point {\"construct_point\": name}; or a plane's name.",
            "anyOf": [
                {"type": "object", "properties": {"type": {"enum": SAVED_REFS}}, "required": ["type"]},
                picked_at("face", &point),
                picked_at("edge", &point),
                picked_at("vertex", &point),
                {
                    "type": "object",
                    "properties": {"sketch": sketch},
                    "required": ["sketch"],
                    "anyOf": [{"properties": {"point": text}, "required": ["point"]}, {"properties": {"curve": text}, "required": ["curve"]}],
                    "not": earlier(&["type"])
                },
                {"type": "object", "properties": {"point": point}, "required": ["point"], "not": earlier(&["type", "sketch"])},
                {"type": "object", "properties": {"plane": plane}, "required": ["plane"], "not": earlier(&["type", "sketch"])},
                {"type": "object", "properties": {"axis": text}, "required": ["axis"], "not": earlier(&["type", "sketch", "plane"])},
                {"type": "object", "properties": {"construct_point": text}, "required": ["construct_point"], "not": earlier(&["type", "sketch", "plane"])},
                text
            ]
        })
    }
}

/// A construction plane's `base`, as `construct.plane.offset` and `construct.plane.at_angle`
/// read it: a planar face of a body when it has `face` (as `sketch::face::capture` reads it),
/// otherwise a [`PlaneParam`]. Kept as given, for the handler.
#[derive(Clone, Debug, PartialEq)]
pub struct PlaneOrFace(pub Value);

impl<'de> Deserialize<'de> for PlaneOrFace {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let problem = match v.get("face") {
            Some(_) => crate::cmd::face_pick_form(&v).err().map(whole),
            None => plane_problem(&v),
        };
        match problem {
            Some(why) => Err(D::Error::custom(why)),
            None => Ok(PlaneOrFace(v)),
        }
    }
}

impl JsonSchema for PlaneOrFace {
    fn schema_name() -> Cow<'static, str> {
        "PlaneOrFace".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        let plane = generator.subschema_for::<PlaneParam>();
        let index_max = u64::try_from(usize::MAX).unwrap_or(u64::MAX);
        json_schema!({
            "description": "A plane, or a planar face of a body {\"face\": [x, y, z] in world coordinates, \"body\"?, \"index\"?, \"path\"?, \"fixed\"?}, which the plane follows as the model changes (\"fixed\": true keeps it where the face is now). \"body\" (the picked body's world name) and \"index\" (the face's index in it) pick the face exactly and go together; \"path\" is the occurrence path of the body's placement, at most 64 ids.",
            "anyOf": [
                {
                    "type": "object",
                    "properties": {
                        "face": point,
                        "fixed": {"type": "boolean"},
                        "body": {"type": "string", "minLength": 1},
                        "index": {"type": "integer", "minimum": 0, "maximum": index_max},
                        "path": {"type": "array", "maxItems": 64, "items": {"type": "integer", "minimum": 1, "maximum": u64::MAX}}
                    },
                    "required": ["face"],
                    "anyOf": [{"required": ["body", "index"]}, {"not": {"anyOf": [{"required": ["body"]}, {"required": ["index"]}]}}]
                },
                {"anyOf": [plane], "not": {"type": "object", "required": ["face"]}}
            ]
        })
    }
}

/// What `construct::model_axis_param` finds wrong with an edge or face axis before it looks
/// anything up in the design: a reference that isn't a picked or saved face or edge.
fn model_axis_problem(v: &Value) -> Option<String> {
    use crate::cmd::GeoForm;
    use solidcraft_doc::construct::GeoRef;
    geo_problem(v).or_else(|| match crate::cmd::geo_form(v) {
        Ok(GeoForm::Face(_) | GeoForm::Edge(_) | GeoForm::Saved(GeoRef::Face { .. } | GeoRef::Edge { .. })) => None,
        _ => Some(solidcraft_doc::construct::MODEL_AXIS_REFS.to_string()),
    })
}

/// The line `construct.plane.at_angle` turns about, as `features::axis_line` reads it: an edge or
/// face of the model (as `construct::model_axis_param` reads it), else X, Y, Z, a construction
/// axis's name or `{origin?, dir}`. Kept as given, for the handler. Whether a picked edge or face
/// gives a line, or a name names an axis, is checked by the handler, which has the design.
#[derive(Clone, Debug, PartialEq)]
pub struct AxisLine(pub Value);

impl<'de> Deserialize<'de> for AxisLine {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let problem = if crate::cmd::is_model_axis(&v) {
            model_axis_problem(&v)
        } else {
            match &v {
                Value::String(_) => None,
                Value::Object(_) => crate::cmd::axis_line_object(&v).err().map(whole),
                _ => Some(whole("`axis` is required")),
            }
        };
        match problem {
            Some(why) => Err(D::Error::custom(why)),
            None => Ok(AxisLine(v)),
        }
    }
}

impl JsonSchema for AxisLine {
    fn schema_name() -> Cow<'static, str> {
        "AxisLine".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        let saved = json!({"required": ["type"], "properties": {"type": {"enum": ["edge", "face"]}}});
        json_schema!({
            "description": "An axis: X, Y or Z (any case), a construction axis's name (where it is now), {\"origin\"?: [x, y, z] in mm (default: the origin), \"dir\": [x, y, z] that isn't zero}, or a straight or circular edge, or a cylinder, cone or torus face, of a body ({\"edge\": [x, y, z]} or {\"face\": [x, y, z]}, a point on it, or as saved: {\"type\": \"edge\" or \"face\", \"at\", \"name\"?}), whose axis the plane follows as the model changes.",
            "anyOf": [
                {"type": "string"},
                {
                    "type": "object",
                    "properties": {"origin": point, "dir": point},
                    "required": ["dir"],
                    "not": {"anyOf": [{"required": ["edge"]}, {"required": ["face"]}, saved]}
                },
                picked_at("face", &point),
                picked_at("edge", &point),
                {"type": "object", "properties": {"type": {"enum": ["edge", "face"]}}, "required": ["type"]}
            ]
        })
    }
}

/// Circular Pattern's `axis`, as `features::pattern_axis` reads it: an [`AxisLine`], except that
/// the coordinates of `{origin, dir}`'s origin may be expressions. Kept as given, for the handler.
#[derive(Clone, Debug, PartialEq)]
pub struct PatternAxis(pub Value);

impl<'de> Deserialize<'de> for PatternAxis {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let Some(origin) = v.get("origin").filter(|_| v.is_object() && !crate::cmd::is_model_axis(&v)) else {
            return AxisLine::deserialize(v).map(|axis| PatternAxis(axis.0)).map_err(D::Error::custom);
        };
        if serde_json::from_value::<PointExpr3>(origin.clone()).is_err() {
            return Err(D::Error::custom(whole("`axis.origin` must be a point [x, y, z] of numbers or length expressions")));
        }
        if v.get("dir").and_then(params::vec3).and_then(solidcraft_geom::Vec3Geom::normalized).is_none() {
            return Err(D::Error::custom(whole("axis needs a non-zero `dir`")));
        }
        Ok(PatternAxis(v))
    }
}

impl JsonSchema for PatternAxis {
    fn schema_name() -> Cow<'static, str> {
        "PatternAxis".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let origin = generator.subschema_for::<PointExpr3>();
        let mut schema = AxisLine::json_schema(generator);
        let slot = schema.as_object_mut().and_then(|o| o.get_mut("anyOf")).and_then(|forms| forms.pointer_mut("/1/properties/origin"));
        if let Some(slot) = slot {
            *slot = json!(origin);
        }
        schema
    }
}

/// What `features::axis_param` says to an `axis` that is none of its forms.
const REVOLVE_AXIS_MESSAGE: &str =
    "`axis` must be a sketch line id, x, y (sketch axes), X, Y, Z, a construction axis, {origin, dir} or {edge|face: [x,y,z]}";

/// What `features::axis_param` finds wrong with an object axis that isn't an edge or face of the
/// model, before it looks anything up in the design.
fn revolve_axis_object_problem(o: &Value) -> Option<String> {
    if o.get("type").and_then(Value::as_str) == Some("world") {
        let ok = o.get("axis").and_then(Value::as_str).is_some_and(|a| matches!(a, "X" | "Y" | "Z"));
        return (!ok).then(|| whole("world axis requires `axis` to be X, Y or Z"));
    }
    if o.get("curve").and_then(Value::as_str).is_some() {
        // A line of a named sketch (#6548): {sketch?, curve}, also saved as {type: "sketch_line", …}.
        // Whether that sketch and line exist is the handler's, which has the design.
        return None;
    }
    if let Some(origin) = o.get("origin")
        && serde_json::from_value::<PointExpr3>(origin.clone()).is_err()
    {
        return Some(whole("`origin` must be a point [x, y, z] of numbers or length expressions"));
    }
    match o.get("dir") {
        None => Some(whole("axis needs `dir`")),
        Some(dir) => params::vec3(dir).is_none().then(|| whole("axis `dir` must be a direction [x, y, z] of numbers")),
    }
}

/// Revolve's `axis`, as `features::axis_param` reads it before it looks anything up: a sketch
/// line's id or name, `x` or `y`, X, Y or Z, a construction axis's name (any text), `{type: "world",
/// axis}`, `{sketch?, curve}` (a line of another sketch, #6548), `{origin?, dir}`, or an edge or
/// face of the model (as `construct::model_axis_param`
/// reads it). Kept as given, for the handler. Whether a line or axis exists, and whether a picked
/// edge or face gives an axis, is checked by the handler, which has the design.
#[derive(Clone, Debug, PartialEq)]
pub struct RevolveAxis(pub Value);

impl<'de> Deserialize<'de> for RevolveAxis {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let problem = if crate::cmd::is_model_axis(&v) {
            model_axis_problem(&v)
        } else {
            match &v {
                Value::String(_) => None,
                Value::Object(_) => revolve_axis_object_problem(&v),
                _ => Some(whole(REVOLVE_AXIS_MESSAGE)),
            }
        };
        match problem {
            Some(why) => Err(D::Error::custom(why)),
            None => Ok(RevolveAxis(v)),
        }
    }
}

impl JsonSchema for RevolveAxis {
    fn schema_name() -> Cow<'static, str> {
        "RevolveAxis".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        let origin = generator.subschema_for::<PointExpr3>();
        let id_or_name = generator.subschema_for::<IdOrName>();
        let model = json!({"anyOf": [{"required": ["edge"]}, {"required": ["face"]}, {"required": ["type"], "properties": {"type": {"enum": ["edge", "face"]}}}]});
        json_schema!({
            "description": "The axis to turn about: a sketch line's id or name (a line of the profile's sketch wins over a construction axis of the same name); {\"sketch\"?: an id or name, \"curve\": an id} — a line of that sketch, which the revolve follows when that sketch is edited; x or y, the sketch's own axes; X, Y or Z, the world axes (any other case is a name); a construction axis's name, which the revolve follows when it is edited; {\"type\": \"world\", \"axis\": X, Y or Z}, as timeline.inspect gives one back; {\"origin\"?: [x, y, z] (numbers or length expressions; default: the world origin), \"dir\": [x, y, z]}, which the revolve follows when parameters change; or an edge or face of a body ({\"edge\": [x, y, z]} or {\"face\": [x, y, z]}, a point on a straight or circular edge, or on a cylinder, cone or torus face, or as saved: {\"type\": \"edge\" or \"face\", \"at\", \"name\"?}), about whose axis it turns, followed as the model changes.",
            "anyOf": [
                {"type": "string"},
                {"type": "object", "properties": {"type": {"enum": ["world"]}, "axis": {"enum": ["X", "Y", "Z"]}}, "required": ["type", "axis"]},
                {"type": "object", "properties": {"sketch": id_or_name, "curve": {"type": "string"}}, "required": ["curve"]},
                {
                    "type": "object",
                    "properties": {"origin": origin, "dir": point},
                    "required": ["dir"],
                    "not": {"anyOf": [model, {"required": ["type"], "properties": {"type": {"enum": ["world"]}}}]}
                },
                picked_at("face", &point),
                picked_at("edge", &point),
                {"type": "object", "properties": {"type": {"enum": ["edge", "face"]}}, "required": ["type"]}
            ]
        })
    }
}

/// Revolve's `direction`: only `symmetric` (any case, not trimmed) does anything; the handler
/// ignores every other value.
#[derive(Clone, Debug, PartialEq)]
pub struct RevolveDirection;

impl<'de> Deserialize<'de> for RevolveDirection {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) if s.eq_ignore_ascii_case("symmetric") => Ok(RevolveDirection),
            _ => Err(D::Error::custom("must be symmetric")),
        }
    }
}

impl JsonSchema for RevolveDirection {
    fn schema_name() -> Cow<'static, str> {
        "RevolveDirection".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        enum_schema(
            &["symmetric"],
            "symmetric: the angle is split either side of the sketch plane, whichever way the axis points. The same as `symmetric: true`. Matching ignores case.",
        )
    }
}

/// Revolve's angular start, as the handler reads `convention` (serde, case-sensitive).
#[derive(Clone, Debug, PartialEq)]
pub struct RevolveConventionParam(pub solidcraft_doc::RevolveConvention);

impl<'de> Deserialize<'de> for RevolveConventionParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        serde_json::from_value(Value::deserialize(d)?)
            .map(RevolveConventionParam)
            .map_err(|_| D::Error::custom("must be sketch_normal or legacy_axis_start"))
    }
}

impl JsonSchema for RevolveConventionParam {
    fn schema_name() -> Cow<'static, str> {
        "RevolveConvention".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        enum_schema(
            &["sketch_normal", "legacy_axis_start"],
            "sketch_normal (the default): the angle is measured from the sketch plane, turning about the axis as the sketch's normal sweeps. legacy_axis_start keeps the angular start older designs had.",
        )
    }
}

/// A construction plane's name, which `features::check_plane_name` refuses when it names an
/// origin plane. Strict gives the same message but names the command that was called, where the
/// handler (`add_feature`, which isn't given it) says `construct.plane`.
#[derive(Clone, Debug, PartialEq)]
pub struct PlaneName(pub String);

impl<'de> Deserialize<'de> for PlaneName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(name) => match crate::cmd::plane_name_problem(&name) {
                Some(why) => Err(D::Error::custom(whole(why))),
                None => Ok(PlaneName(name)),
            },
            _ => Err(D::Error::custom("must be text")),
        }
    }
}

impl JsonSchema for PlaneName {
    fn schema_name() -> Cow<'static, str> {
        "PlaneName".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        // The schema can't say which names are refused: a pattern would need a lookahead, which
        // not every validator has.
        json_schema!({
            "description": "The plane's name in the timeline: not XY, XZ, YZ, Top, Front or Right (any case, with or without spaces around it), which name the origin planes.",
            "type": "string"
        })
    }
}

/// The keys `features::add_feature` reads that a construction axis or point takes besides its
/// references, flattened into its parameters with [`feature_common`]. Not [`FeatureCommon`]: it
/// makes no body, so `body_name` and `body_names` would do nothing. `participants` stays because
/// `timeline.redefine` passes a feature's own back.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
pub struct ConstructionCommon {
    /// The feature's name in the timeline.
    #[serde(default)]
    pub name: Option<String>,
    /// The bodies taking part, which every feature records; each an existing body or `*`. A
    /// construction feature changes no body, whatever this lists.
    #[serde(default)]
    pub participants: Option<StringList>,
}

/// [`ConstructionCommon`] for a construction plane, whose name may not name an origin plane.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
pub struct PlaneCommon {
    #[serde(default)]
    pub name: Option<PlaneName>,
    /// The bodies taking part, which every feature records; each an existing body or `*`. A
    /// construction feature changes no body, whatever this lists.
    #[serde(default)]
    pub participants: Option<StringList>,
}

/// The keys `features::add_feature` reads for every feature command, whatever else it takes:
/// the feature's name, the new bodies' names and the bodies taking part. A feature command's
/// parameters include it with
///
/// ```ignore
/// #[serde(flatten, deserialize_with = "feature_common")]
/// pub common: FeatureCommon,
/// ```
///
/// Two keys `add_feature` also reads aren't here. `targets` it reads only for primitives; the
/// other commands that read it are the ones that combine bodies. `operation` it reads for every
/// feature (a cut where the active component has no bodies takes part in all of them), but only
/// the commands that combine bodies take one; those declare it with their own type
/// ([`OperationParam`] or [`ExtrudeOperation`]), and for the rest it is a caller's mistake.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
pub struct FeatureCommon {
    /// The feature's name in the timeline.
    #[serde(default)]
    pub name: Option<String>,
    /// The new body's name.
    #[serde(default)]
    pub body_name: Option<String>,
    /// Names for the new bodies, in order.
    #[serde(default)]
    pub body_names: Option<StringList>,
    /// The bodies taking part, each an existing body or `*`; what that changes depends on the
    /// feature.
    #[serde(default)]
    pub participants: Option<StringList>,
}

/// Deserializes a flattened [`FeatureCommon`] (or [`ConstructionCommon`], [`PlaneCommon`]) from
/// the keys the parent doesn't take, and reports a problem as if its fields were the parent's
/// own, with the key in the message and in the issue's path (serde's flatten otherwise loses the
/// key). serde reads a flattened field after all the parent's own, so its problems come after
/// theirs, including a missing key.
pub fn feature_common<'de, D: Deserializer<'de>, T: serde::de::DeserializeOwned>(d: D) -> Result<T, D::Error> {
    let v = Value::deserialize(d)?;
    serde_path_to_error::deserialize(&v).map_err(|e| D::Error::custom(super::check::located(&e)))
}
