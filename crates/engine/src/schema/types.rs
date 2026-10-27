//! Shared parameter types. Each one decides acceptance with the helper the handlers already
//! use, applied to the raw JSON value, so a contract can never accept a value its handler's own
//! reader would refuse. Their schemas describe the accepted forms.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use solidcraft_doc::{Expr, Operation, ThinSide};
use solidcraft_geom::{Vec2, Vec3};

use super::check::whole;
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
fn whitespace_class() -> String {
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
const OPERATIONS: [&str; 13] = [
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
            &OPERATIONS,
            "How the feature combines with existing bodies: new, join, cut or intersect. The others are aliases. Matching ignores case, spaces, '_' and '-'.",
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
        values.extend(OPERATIONS);
        enum_schema(
            &values,
            "How the extrusion combines with existing bodies: auto (cut into a body, otherwise join; new in an empty component), new, join, cut or intersect. The others are aliases. Matching ignores case, spaces, '_' and '-' (only case for auto).",
        )
    }
}

const DIRECTIONS: [&str; 6] = ["positive", "negative", "symmetric", "one_side", "reverse", "flip"];

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
            &DIRECTIONS,
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
    /// `face.delete`, as `features::face_points` reads them. (Extrude's `faces` is a
    /// [`FacesParam`].)
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

/// Deserializes a flattened [`FeatureCommon`] from the keys the parent doesn't take, and reports
/// a problem as if its fields were the parent's own, with the key in the message and in the
/// issue's path (serde's flatten otherwise loses the key). serde reads a flattened field after
/// all the parent's own, so its problems come after theirs, including a missing key.
pub fn feature_common<'de, D: Deserializer<'de>>(d: D) -> Result<FeatureCommon, D::Error> {
    let v = Value::deserialize(d)?;
    serde_path_to_error::deserialize(&v).map_err(|e| D::Error::custom(super::check::located(&e)))
}
