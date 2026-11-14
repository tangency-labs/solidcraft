//! Contracts for sheet metal feature, rule, flat pattern, DXF export and bend table commands in
//! `cmd/sheet.rs`. Feature commands return [`FeatureResult`](super::features::FeatureResult); the
//! others have their own replies.
//!
//! A sheet metal rule sets the sheet's thickness, K-factor, bend radius and reliefs: the
//! commands refuse those keys with a pointer to `sheet.manage_rules`, so they are no keys of
//! these contracts (strict refuses them as unknown keys). The same goes for the nested `base`
//! object an old base flange call may carry, which the handler reads only to refuse rule keys
//! in it.
//!
//! The value types below are used by these commands only. Like the shared ones in
//! [`types`](super::types), each calls the handler's own reader (split out of `cmd/sheet.rs`)
//! and gives its message.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use solidcore_record::Vec3;
use solidcraft_doc::FlangeEdge;

use super::check::whole;
use super::types::{AngleExpr, FaceRef, FeatureCommon, IdOrName, LengthExpr, Point3, ProfilesParam, SketchRef, StringList, feature_common, present};

/// Points on sheet edges, as the `edges` of a hem and the `bends` of a conversion: 1 to 1000
/// points `[x, y, z]`, `{x, y, z}` or `{point}` (read by `sheet::sheet_points`).
#[derive(Clone, Debug, PartialEq)]
pub struct SheetPoints(pub Vec<Vec3>);

impl<'de> Deserialize<'de> for SheetPoints {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        crate::cmd::sheet_points(Some(&Value::deserialize(d)?)).map(SheetPoints).map_err(D::Error::custom)
    }
}

impl JsonSchema for SheetPoints {
    fn schema_name() -> Cow<'static, str> {
        "SheetPoints".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "Points in mm on sheet edges, 1 to 1000: each [x, y, z], {\"x\", \"y\", \"z\"} or {\"point\": [x, y, z]}.",
            "type": "array",
            "items": {"anyOf": [point, {"type": "object", "properties": {"point": point}, "required": ["point"]}]},
            "minItems": 1,
            "maxItems": 1000
        })
    }
}

/// The `edges` of an edge flange: 1 to 1000 points on sheet edges, each `[x, y, z]` (or
/// `{x, y, z}`) or `{point, angle?, height?}` for an edge with its own angle and height (read by
/// `sheet::flange_edge`). Whether the expressions suit the design is checked by the handler.
#[derive(Clone, Debug, PartialEq)]
pub struct FlangeEdges(pub Vec<FlangeEdge>);

impl<'de> Deserialize<'de> for FlangeEdges {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let items = crate::cmd::flange_edge_list(Some(&v)).map_err(|why| D::Error::custom(whole(why)))?;
        items
            .iter()
            .enumerate()
            .map(|(i, item)| crate::cmd::flange_edge(i, item).map_err(|why| D::Error::custom(whole(why))))
            .collect::<Result<_, _>>()
            .map(FlangeEdges)
    }
}

impl JsonSchema for FlangeEdges {
    fn schema_name() -> Cow<'static, str> {
        "FlangeEdges".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        let angle = generator.subschema_for::<AngleExpr>();
        let height = generator.subschema_for::<LengthExpr>();
        let segment = json_schema!({
            "description": "One stretch of the edge: from and to are fractions 0…1 of the stretch the sheet still has there (the first from is 0, the last to is 1, and each from is the to of the one before); angle, height and flip are its own where given.",
            "type": "object",
            "properties": {
                "from": {"type": "number", "minimum": 0, "maximum": 1},
                "to": {"type": "number", "minimum": 0, "maximum": 1},
                "angle": {"anyOf": [angle, {"type": "null"}]},
                "height": {"anyOf": [height, {"type": "null"}]},
                "flip": {"anyOf": [{"type": "boolean"}, {"type": "null"}]}
            },
            "required": ["from", "to"],
            "additionalProperties": false
        });
        json_schema!({
            "description": "Points in mm on the sheet's top or bottom edges, 1 to 1000: each [x, y, z] or {\"x\", \"y\", \"z\"}, or {point: [x, y, z], angle?, height?, segments?} for an edge with its own angle and height (null: the feature's), or split into 1 to 64 segments that tile it.",
            "type": "array",
            "items": {
                "anyOf": [
                    // An object with any of the edge's keys is an edge, never a bare point.
                    {"anyOf": [point], "not": {"type": "object", "anyOf": [{"required": ["point"]}, {"required": ["angle"]}, {"required": ["height"]}, {"required": ["segments"]}]}},
                    {
                        "type": "object",
                        "properties": {
                            "point": point,
                            "angle": {"anyOf": [angle, {"type": "null"}]},
                            "height": {"anyOf": [height, {"type": "null"}]},
                            "segments": {"anyOf": [{"type": "array", "items": segment, "minItems": 1, "maxItems": 64}, {"type": "null"}]}
                        },
                        "required": ["point"],
                        "additionalProperties": false
                    }
                ]
            },
            "minItems": 1,
            "maxItems": 1000
        })
    }
}

/// A fold line's two ends, `[[x, y, z], [x, y, z]]` (read by `sheet::fold_points`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FoldPoints(pub Vec3, pub Vec3);

impl<'de> Deserialize<'de> for FoldPoints {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        crate::cmd::fold_points(&Value::deserialize(d)?).map(|(a, b)| FoldPoints(a, b)).map_err(D::Error::custom)
    }
}

impl JsonSchema for FoldPoints {
    fn schema_name() -> Cow<'static, str> {
        "FoldPoints".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({"description": "The fold line's two ends, in mm.", "type": "array", "items": point, "minItems": 2, "maxItems": 2})
    }
}

/// A point in mm, `[x, y, z]` or `{x, y, z}`, with a fold's message for `fixed` (`Point3` says
/// "of numbers" after it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FixedPoint(pub Vec3);

impl<'de> Deserialize<'de> for FixedPoint {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        crate::params::vec3(&Value::deserialize(d)?).map(FixedPoint).ok_or_else(|| D::Error::custom("must be a point [x, y, z]"))
    }
}

impl JsonSchema for FixedPoint {
    fn schema_name() -> Cow<'static, str> {
        "FixedPoint".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        Point3::json_schema(generator)
    }
}

/// A flange's `type`, in any case (read by `sheet::flange_type`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlangeTypeParam(pub(crate) crate::cmd::FlangeType);

impl<'de> Deserialize<'de> for FlangeTypeParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let t = v.as_str().ok_or_else(|| D::Error::custom("must be base, edge or contour"))?;
        crate::cmd::flange_type(t).map(FlangeTypeParam).map_err(|why| D::Error::custom(whole(why)))
    }
}

impl JsonSchema for FlangeTypeParam {
    fn schema_name() -> Cow<'static, str> {
        "FlangeType".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "base, edge or contour, in any case.",
            "type": "string",
            "pattern": "^([Bb][Aa][Ss][Ee]|[Ee][Dd][Gg][Ee]|[Cc][Oo][Nn][Tt][Oo][Uu][Rr])$"
        })
    }
}

/// A word from a short list, in any case, read by a handler's reader that returns `None` for
/// any other text; `$message` is the handler's.
macro_rules! word {
    ($(#[$doc:meta])* $name:ident, $reader:path, $message:literal, $description:literal, [$($word:literal),+]) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name(pub String);

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Value::deserialize(d)?.as_str().and_then($reader).map($name).ok_or_else(|| D::Error::custom($message))
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                stringify!($name).into()
            }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                let any_case = |w: &str| w.chars().map(|c| format!("[{}{}]", c.to_ascii_uppercase(), c)).collect::<String>();
                let words: Vec<String> = [$($word),+].iter().map(|w| any_case(w)).collect();
                json_schema!({"description": $description, "type": "string", "pattern": format!("^({})$", words.join("|"))})
            }
        }
    };
}

word!(
    /// An edge flange's `position` (read by `sheet::flange_position`).
    FlangePosition,
    crate::cmd::flange_position,
    "must be inside, outside, middle, material_outside or tangent",
    "inside, outside (bend_outside is the same), middle, material_outside or tangent, in any case.",
    ["inside", "outside", "bend_outside", "middle", "material_outside", "tangent"]
);
word!(
    /// A fold's `position` (read by `sheet::fold_position`).
    FoldPosition,
    crate::cmd::fold_position,
    "must be centerline, start, end or mould",
    "centerline, start, end or mould (or mold), in any case.",
    ["centerline", "start", "end", "mould", "mold"]
);

/// A corner relief's shape: notch, round, rectangular or tear, in any case and with blanks
/// around it (read by `ReliefShape::parse`).
#[derive(Clone, Debug, PartialEq)]
pub struct ReliefShapeParam(pub solidcraft_doc::sheet::ReliefShape);

impl<'de> Deserialize<'de> for ReliefShapeParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?
            .as_str()
            .and_then(solidcraft_doc::sheet::ReliefShape::parse)
            .map(ReliefShapeParam)
            .ok_or_else(|| D::Error::custom("must be notch, round, rectangular or tear"))
    }
}

impl JsonSchema for ReliefShapeParam {
    fn schema_name() -> Cow<'static, str> {
        "ReliefShape".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "notch, round, rectangular or tear, in any case (blanks around it are ignored).",
            "type": "string",
            "pattern": format!("^{blank}({words}){blank}$", blank = blanks(), words = "[Nn][Oo][Tt][Cc][Hh]|[Rr][Oo][Uu][Nn][Dd]|[Rr][Ee][Cc][Tt][Aa][Nn][Gg][Uu][Ll][Aa][Rr]|[Tt][Ee][Aa][Rr]")
        })
    }
}

/// Where an edge flange's height is measured from: outer, inner or tangent, in any case and with
/// blanks around it (read by `HeightFrom::parse`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeightFromParam(pub solidcraft_doc::sheet::HeightFrom);

impl<'de> Deserialize<'de> for HeightFromParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?
            .as_str()
            .and_then(solidcraft_doc::sheet::HeightFrom::parse)
            .map(HeightFromParam)
            .ok_or_else(|| D::Error::custom("must be outer, inner or tangent"))
    }
}

impl JsonSchema for HeightFromParam {
    fn schema_name() -> Cow<'static, str> {
        "HeightFrom".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "outer, inner or tangent, in any case (blanks around it are ignored).",
            "type": "string",
            "pattern": format!("^{blank}({words}){blank}$", blank = blanks(), words = "[Oo][Uu][Tt][Ee][Rr]|[Ii][Nn][Nn][Ee][Rr]|[Tt][Aa][Nn][Gg][Ee][Nn][Tt]")
        })
    }
}

/// An edge flange's further bends (`then`): up to 16, each `{angle?, height, flip?}`, made on the
/// far edge of the flange before it (read by `sheet::then_list`). Whether the expressions suit
/// the design is checked by the handler. The handler reads an `angle` that is no expression as
/// left out (90°); this type refuses it.
#[derive(Clone, Debug, PartialEq)]
pub struct ThenSteps(pub Vec<crate::cmd::ThenStep>);

impl<'de> Deserialize<'de> for ThenSteps {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let steps = crate::cmd::then_list(Some(&v)).map_err(|why| D::Error::custom(whole(why)))?;
        let bends = v.as_array().map(Vec::as_slice).unwrap_or_default();
        let wrong = bends
            .iter()
            .enumerate()
            .find_map(|(i, b)| b.get("angle").filter(|a| !a.is_null() && crate::params::expr(b, "angle").is_none()).map(|a| (i, a)));
        match wrong {
            Some((i, a)) => Err(D::Error::custom(whole(format!("then[{i}]: `angle` {}", crate::params::expr_problem(Some(a)))))),
            None => Ok(ThenSteps(steps)),
        }
    }
}

impl JsonSchema for ThenSteps {
    fn schema_name() -> Cow<'static, str> {
        "ThenSteps".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let angle = generator.subschema_for::<AngleExpr>();
        let height = generator.subschema_for::<LengthExpr>();
        json_schema!({
            "description": "Further bends, at most 16, each a flange on the far edge of the one before: its height, its angle (default 90 deg) and flip (true turns that bend the other way; default: the flange's).",
            "type": "array",
            "items": {
                "type": "object",
                "properties": {"angle": {"anyOf": [angle, {"type": "null"}]}, "height": height, "flip": {"type": ["boolean", "null"]}},
                "required": ["height"],
                "additionalProperties": false
            },
            "maxItems": 16
        })
    }
}

/// Any run of the characters Rust's `trim` removes, as a regex.
fn blanks() -> String {
    let u = |c: char| format!("\\u{:04x}", c as u32);
    let class: String = super::types::WHITESPACE.iter().map(|&(a, b)| if a == b { u(a) } else { format!("{}-{}", u(a), u(b)) }).collect();
    format!("[{class}]*")
}

/// A perforation's `pattern` (read by `sheet::perforation_pattern`).
#[derive(Clone, Debug, PartialEq)]
pub struct PerforationPattern(pub String);

impl<'de> Deserialize<'de> for PerforationPattern {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let t = v.as_str().unwrap_or_default();
        crate::cmd::perforation_pattern(t).map(|t| PerforationPattern(t.to_string())).map_err(|why| D::Error::custom(whole(why)))
    }
}

impl JsonSchema for PerforationPattern {
    fn schema_name() -> Cow<'static, str> {
        "PerforationPattern".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "staggered (rows offset for a 60° pattern) or square, lower case.", "enum": ["staggered", "square"]})
    }
}

/// Add a sheet metal flange: a base flange from sketch profiles, a contour flange along an open
/// chain of sketch lines and tangent arcs, or edge flanges on a sheet's edges. Thickness, K-factor, bend radius
/// and reliefs come from the sheet metal rule (`sheet.manage_rules`), so those keys are refused;
/// an edge flange may set its own `radius` and corner relief. `participants` changes nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct FlangeParams {
    /// Which flange: base, edge or contour, in any case. Default: edge when `edges` is given,
    /// else contour when `curves` is given, else base.
    #[serde(default)]
    pub r#type: Option<FlangeTypeParam>,
    /// Base and contour: the sketch, by id or name. A base flange's default is the active sketch,
    /// else the last one; a contour flange needs it.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "IdOrName")]
    pub sketch: Option<IdOrName>,
    /// Base: which of the sketch's profiles become the sheet. Default: all of them.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ProfilesParam")]
    pub profiles: Option<ProfilesParam>,
    /// Base and contour: the sheet metal rule, by name. Default: the active rule. Ignored by an
    /// edge flange, which takes its sheet's.
    #[serde(default)]
    pub rule: Option<String>,
    /// Base: the thickness goes to the other side of the sketch plane. Contour: the material goes
    /// to the other side of the chain. Edge: the flanges bend the other way. Default: false.
    #[serde(default)]
    pub flip: Option<bool>,
    /// Contour: the open chain's lines and arcs (curve ids), 1 to 1000. An arc is a bend of its
    /// own radius; it must meet the curves beside it tangentially.
    #[serde(default)]
    pub curves: Option<StringList>,
    /// Contour: how far the flange runs from the sketch plane, positive.
    #[serde(default)]
    pub distance: Option<LengthExpr>,
    /// Contour: the flange runs from the sketch plane the other way. Default: false.
    #[serde(default)]
    pub reverse: Option<bool>,
    /// Edge: the edges that get a flange.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FlangeEdges")]
    pub edges: Option<FlangeEdges>,
    /// Edge: each flange's height, along the flange to its far edge, from where `height_from` says
    /// (an edge's own `height` replaces it). Required for an edge flange.
    #[serde(default)]
    pub height: Option<LengthExpr>,
    /// Edge: where the height is measured from: the outer virtual sharp (outer), the inner virtual
    /// sharp (inner), or past 90° the line square to the flange touching the bend's outside
    /// (tangent; up to 90° the outer sharp). Default: outer.
    #[serde(default)]
    pub height_from: Option<HeightFromParam>,
    /// Edge: the bend angle, more than 0 and at most 180° (an edge's own `angle` replaces it).
    /// Default: 90 deg.
    #[serde(default)]
    pub angle: Option<AngleExpr>,
    /// Edge: the inner bend radius. Default: the sheet's rule's.
    #[serde(default)]
    pub radius: Option<LengthExpr>,
    /// Edge: where the bend starts: inside (the outer virtual sharp on the edge), outside or
    /// bend_outside (the bend starting at the edge), middle (half-way), material_outside (the
    /// inner virtual sharp on the edge) or tangent (the bend's outer face tangent to the edge's
    /// face; below 90° the bend's end on that face). Default: inside.
    #[serde(default)]
    pub position: Option<FlangePosition>,
    /// Edge: the sheet metal body. Default: the sheet with an edge nearest the first point.
    #[serde(default)]
    pub body: Option<String>,
    /// Edge: the shape of the relief cut where these flanges meet at a corner, in place of the
    /// rule's. Refused on a base or contour flange.
    #[serde(default)]
    pub corner_relief_shape: Option<ReliefShapeParam>,
    /// Edge: the size across of a round or rectangular corner relief, in place of the rule's.
    /// Refused on a base or contour flange.
    #[serde(default)]
    pub corner_relief: Option<LengthExpr>,
    /// Edge: further bends, each a flange on the far edge of the one before (at most 16). Refused
    /// on a base or contour flange.
    #[serde(default)]
    pub then: Option<ThenSteps>,
    /// Edge: segments belong inside an `edges` item (`{point, segments}`), not here. A top-level
    /// `segments` is refused, naming where it goes, rather than ignored as an unknown key.
    #[serde(default)]
    pub segments: Option<Value>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Add a field of round perforations over a whole flat sheet, or inside a sketch's outline: holes
/// in the flat pattern, the DXF (layer PERFORATION) and the measured volume, not cut into the
/// solid. `participants` changes nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PerforateParams {
    /// The holes' diameter, more than 0.
    pub diameter: LengthExpr,
    /// The distance between hole centres, more than the diameter.
    pub pitch: LengthExpr,
    /// How far the holes stay clear of the sheet's edges, cut-outs and bends, and of the outline,
    /// at least 0. Default: the diameter.
    #[serde(default)]
    pub margin: Option<LengthExpr>,
    /// How the holes are laid out. Default: staggered.
    #[serde(default)]
    pub pattern: Option<PerforationPattern>,
    /// The sheet metal body. Default: the newest sheet.
    #[serde(default)]
    pub body: Option<String>,
    /// The field's outline: a sketch, by id or name, on a face of the sheet (or on the flat sheet
    /// while it is unfolded). Holes go only inside its closed profiles, out of their inner loops,
    /// on a grid from the outline's lower-left corner; on a folded sheet only in the panels the
    /// sketch lies on. Default: the whole sheet.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "IdOrName")]
    pub sketch: Option<IdOrName>,
    /// Which of the outline sketch's profiles bound the field. Needs `sketch`. Default: all of
    /// them.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ProfilesParam")]
    pub profiles: Option<ProfilesParam>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Add hems: the sheet folded back on itself along edges. `participants` changes nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct HemParams {
    /// The edges to hem.
    pub edges: SheetPoints,
    /// The hem's length, to its outer face.
    pub length: LengthExpr,
    /// The gap inside the hem (its inner radius). Default: the sheet's rule's hem gap.
    #[serde(default)]
    pub gap: Option<LengthExpr>,
    /// Hem to the other side. Default: false.
    #[serde(default)]
    pub flip: Option<bool>,
    /// The sheet metal body. Default: the sheet with an edge nearest the first point.
    #[serde(default)]
    pub body: Option<String>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Fold a sheet along a line on its top or bottom face: a sketch line (followed when the sketch
/// changes) or two points. The bend zone comes out of the sheet, so the flat pattern keeps its
/// size. `participants` changes nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct FoldParams {
    /// The fold line's two ends, instead of `sketch` and `curve`.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FoldPoints")]
    pub points: Option<FoldPoints>,
    /// The sketch holding the fold line, by id or name. Required unless `points` is given;
    /// ignored then.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchRef")]
    pub sketch: Option<SketchRef>,
    /// The fold line: a line of `sketch`, by id. Required unless `points` is given; ignored then.
    #[serde(default)]
    pub curve: Option<String>,
    /// The bend angle. Default: 90 deg.
    #[serde(default)]
    pub angle: Option<AngleExpr>,
    /// The inner bend radius. Default: the sheet's rule's.
    #[serde(default)]
    pub radius: Option<LengthExpr>,
    /// Where the line sits in the bend zone: its centre (centerline), its start or end, from the
    /// side that stays, or the mould line (mould: where the outer faces meet). Default:
    /// centerline.
    #[serde(default)]
    pub position: Option<FoldPosition>,
    /// A point on the side that stays, on the sheet. Default: the larger side (on a flange, the
    /// side toward its own bend).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FixedPoint")]
    pub fixed: Option<FixedPoint>,
    /// Fold the other way. Default: false.
    #[serde(default)]
    pub flip: Option<bool>,
    /// The sheet metal body. Default: the sheet with an edge nearest the line's middle.
    #[serde(default)]
    pub body: Option<String>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Unfold every bend of a sheet (`sheet.unfold`), or fold an unfolded sheet back
/// (`sheet.refold`). `participants` changes nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct UnfoldParams {
    /// The sheet metal body. Default: the last folded sheet (unfold) or the last unfolded one
    /// (refold).
    #[serde(default)]
    pub body: Option<String>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Turn a body into sheet metal. Without `bends`, a plate of even thickness: its large flat face
/// becomes the sheet, holes become cut-outs and recesses in one face up to half the thickness
/// deep (an engraved label) become pockets. With `bends`, a solid or shelled body: `face` stays
/// put and the flat faces reached from it across those edges become the sheet; a shell keeps its
/// thickness, a solid takes the rule's inward, and edges between two of those faces that are no
/// bends are cut open with the rule's gap. `participants` changes nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ConvertParams {
    /// The body to convert.
    pub body: String,
    /// Its large flat face (a plate), or the face that stays put (with `bends`).
    pub face: FaceRef,
    /// The sharp edges that become bends of the rule's radius at their virtual sharp. A point in
    /// the middle of a chamfer between two faces, or on a bend modelled with its radius (a
    /// cylinder tangent to two flat faces, either skin), makes it one bend. Default: none (a
    /// plate).
    #[serde(default)]
    pub bends: Option<SheetPoints>,
    /// The sheet metal rule, by name. Default: the active rule. A plate keeps its own thickness.
    #[serde(default)]
    pub rule: Option<String>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// List the design's sheet metal rules (the built-in steel rule if none are saved).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RulesParams {}

/// Create or edit a sheet metal rule, activate it, or delete it. Expressions may use design
/// parameters; `Thickness` inside a rule's other expressions is that rule's thickness.
/// The handler evaluates dimensions and ranges against the design after checking the contract.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ManageRulesParams {
    /// The rule's name (trimmed, nonempty, at most 128 bytes). A new rule inherits the active
    /// rule's fields; an existing rule keeps fields not given here.
    pub name: RuleName,
    /// Thickness, more than 0 and less than 10000 mm. Ignored when deleting.
    pub thickness: Option<LengthExpr>,
    /// Neutral-axis fraction, from 0 to 1. Ignored when deleting.
    pub k_factor: Option<super::types::NumberExpr>,
    /// Inner bend radius, nonnegative. Ignored when deleting.
    pub bend_radius: Option<LengthExpr>,
    /// Bend relief width. Ignored when deleting.
    pub relief_width: Option<LengthExpr>,
    /// Bend relief depth. Ignored when deleting.
    pub relief_depth: Option<LengthExpr>,
    /// Corner relief size across. Ignored when deleting.
    pub corner_relief: Option<LengthExpr>,
    /// Corner relief shape. Ignored when deleting.
    pub corner_relief_shape: Option<ReliefShapeParam>,
    /// Inner radius of flat hems. Ignored when deleting.
    pub hem_gap: Option<LengthExpr>,
    /// Gap between meeting flanges. Ignored when deleting.
    pub gap: Option<LengthExpr>,
    /// How DXF bend notes show angles: bend or included, ignoring case and surrounding blanks.
    /// Ignored when deleting.
    pub angle_display: Option<AngleDisplayParam>,
    /// Make this the active rule. Default: true if there is no saved active rule, otherwise false.
    /// False leaves the active choice unchanged; ignored when deleting.
    pub active: Option<bool>,
    /// Delete the named saved rule instead of editing it. Default: false. Deleting the active
    /// rule clears that choice; listing then uses the first remaining rule, else built-in steel.
    pub delete: Option<bool>,
}

/// A rule name, checked by the handler's reader.
#[derive(Debug)]
pub struct RuleName(pub String);

impl<'de> Deserialize<'de> for RuleName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        crate::cmd::sheet_rule_name(&v).map(|n| RuleName(n.to_string())).map_err(|why| D::Error::custom(whole(why)))
    }
}

impl JsonSchema for RuleName {
    fn schema_name() -> Cow<'static, str> {
        "SheetRuleName".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "description": "Trimmed, nonempty rule name, at most 128 bytes after trimming."})
    }
}

/// A rule's bend-note angle display, checked by the same parser as the handler.
#[derive(Debug)]
pub struct AngleDisplayParam(pub solidcraft_doc::sheet::AngleDisplay);

impl<'de> Deserialize<'de> for AngleDisplayParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?
            .as_str()
            .and_then(solidcraft_doc::sheet::AngleDisplay::parse)
            .map(AngleDisplayParam)
            .ok_or_else(|| D::Error::custom("must be bend or included"))
    }
}

impl JsonSchema for AngleDisplayParam {
    fn schema_name() -> Cow<'static, str> {
        "SheetAngleDisplay".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "description": "bend or included, ignoring case and surrounding blanks.",
            "pattern": format!("^{blank}([Bb][Ee][Nn][Dd]|[Ii][Nn][Cc][Ll][Uu][Dd][Ee][Dd]){blank}$", blank = blanks())})
    }
}

/// A saved rule's expressions, including the default bend-angle display which the saved
/// design omits. These are expressions, not evaluated values.
#[derive(Debug, serde::Serialize, JsonSchema)]
pub struct RuleReply {
    pub name: String,
    pub thickness: String,
    pub k_factor: String,
    pub bend_radius: String,
    pub relief_width: String,
    pub relief_depth: String,
    pub corner_relief: String,
    pub corner_relief_shape: String,
    pub hem_gap: String,
    pub gap: String,
    pub angle_display: String,
}

impl From<&solidcraft_doc::sheet::SheetRule> for RuleReply {
    fn from(r: &solidcraft_doc::sheet::SheetRule) -> Self {
        Self {
            name: r.name.clone(),
            thickness: r.thickness.clone(),
            k_factor: r.k_factor.clone(),
            bend_radius: r.bend_radius.clone(),
            relief_width: r.relief_width.clone(),
            relief_depth: r.relief_depth.clone(),
            corner_relief: r.corner_relief.clone(),
            corner_relief_shape: r.corner_relief_shape.clone(),
            hem_gap: r.hem_gap.clone(),
            gap: r.gap.clone(),
            angle_display: r.angle_display.clone(),
        }
    }
}

/// Evaluated rule values: lengths in millimetres, dimensionless K-factor and canonical choices.
#[derive(Debug, serde::Serialize, JsonSchema)]
pub struct RuleValuesReply {
    pub thickness: f64,
    pub k_factor: f64,
    pub bend_radius: f64,
    pub relief_width: f64,
    pub relief_depth: f64,
    pub corner_relief: f64,
    pub corner_relief_shape: String,
    pub hem_gap: f64,
    pub gap: f64,
    pub angle_display: String,
}

impl From<solidcraft_doc::sheet::RuleValues> for RuleValuesReply {
    fn from(r: solidcraft_doc::sheet::RuleValues) -> Self {
        Self {
            thickness: r.thickness,
            k_factor: r.k_factor,
            bend_radius: r.bend_radius,
            relief_width: r.relief_width,
            relief_depth: r.relief_depth,
            corner_relief: r.corner_relief,
            corner_relief_shape: r.corner_relief_shape.name().into(),
            hem_gap: r.hem_gap,
            gap: r.gap,
            angle_display: r.angle_display.name().into(),
        }
    }
}

/// A rule and its evaluated values, or null values if its expressions no longer evaluate.
#[derive(Debug, serde::Serialize, JsonSchema)]
pub struct ListedRule {
    pub rule: RuleReply,
    pub values: Option<RuleValuesReply>,
}

/// The rule list and effective active name.
#[derive(Debug, serde::Serialize, JsonSchema)]
pub struct RulesResult {
    pub rules: Vec<ListedRule>,
    pub active: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A feature that failed to rebuild after a rule was edited.
#[derive(Debug, serde::Serialize, JsonSchema)]
pub struct RuleFeatureError {
    pub feature: String,
    pub error: String,
}

/// A successful rule edit or deletion, preserving the existing reply alternatives.
#[derive(Debug, serde::Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ManageRulesResult {
    Changed {
        rule: Box<RuleReply>,
        values: RuleValuesReply,
        errors: Vec<RuleFeatureError>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        param_warnings: Vec<String>,
    },
    Deleted {
        deleted: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        param_warnings: Vec<String>,
    },
}

/// A point in mm on a sheet edge, `[x, y, z]` or `{x, y, z}`, with a path flange's message for
/// `edge`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SheetEdgePoint(pub Vec3);

impl<'de> Deserialize<'de> for SheetEdgePoint {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        crate::params::vec3(&Value::deserialize(d)?).map(SheetEdgePoint).ok_or_else(|| D::Error::custom("must be a point [x, y, z] on a sheet edge"))
    }
}

impl JsonSchema for SheetEdgePoint {
    fn schema_name() -> Cow<'static, str> {
        "SheetEdgePoint".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        Point3::json_schema(generator)
    }
}

/// Add a path flange: flanges along an open chain of sketch lines drawn on a plane across a
/// sheet edge, from the edge out. Each line is one flange as long as the line (to the outer face
/// from the virtual sharp) and each corner one bend, turning the way the lines turn. One
/// feature, one body. `participants` changes nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PathFlangeParams {
    /// A point on a top or bottom edge of the sheet, where the path starts.
    pub edge: SheetEdgePoint,
    /// The sketch holding the path, by id or name.
    pub sketch: SketchRef,
    /// The path's lines: 1 to 64 lines of `sketch`, by id, in one open chain, in any order. Arcs
    /// are refused.
    pub curves: StringList,
    /// The inner bend radius. Default: the sheet's rule's.
    #[serde(default)]
    pub radius: Option<LengthExpr>,
    /// The sheet metal body. Default: the sheet with an edge nearest `edge`.
    #[serde(default)]
    pub body: Option<String>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Read a sheet's flat pattern: its size, outline, cut-outs, pockets, bends and perforations, in
/// flat coordinates (mm, on the base panel's bottom face). Refused while a feature that is not
/// sheet metal (a fillet, a join) has changed the body.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct FlatPatternParams {
    /// The sheet metal body. Default: the last sheet.
    #[serde(default)]
    pub body: Option<String>,
}

/// Export a sheet's bend table: a row per bend, in mm and degrees, with the rule and the bend's
/// centre line in flat coordinates. A bend that cut-outs split has a row for each piece.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct BendTableParams {
    /// The sheet metal body. Default: the last sheet.
    #[serde(default)]
    pub body: Option<String>,
    /// csv (a header and a row per bend, quoted body and rule names, CRLF line ends) or json (the
    /// flat pattern that `sheet.flat_pattern` returns, as text). Default: csv.
    #[serde(default)]
    pub format: Option<BendTableFormatParam>,
    /// The file to write. Default (or blank): no file; the reply holds the table's text.
    #[serde(default)]
    pub path: Option<String>,
}

/// Export a sheet's flat pattern as a DXF drawing (AutoCAD 2000, mm): the outline, the cut-outs,
/// the letters, the bend lines and their notes, each on the layer for its role.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ExportDxfParams {
    /// The sheet metal body. Default: the last sheet. Not with `per_body`.
    #[serde(default)]
    pub body: Option<String>,
    /// The file to write. Default (or blank): no file, and the reply holds the DXF text. With
    /// `per_body`, one file per sheet body, `<path>-<body>.dxf`.
    #[serde(default)]
    pub path: Option<String>,
    /// exact: an ellipse as ELLIPSE, splines, text outlines and the other Béziers as SPLINE.
    /// arcs: tangent arcs and lines within 0.001 mm, for machines that read only those.
    /// Default: exact.
    #[serde(default)]
    pub curves: Option<DxfCurvesParam>,
    /// cut: the letters text cut through the sheet are cut contours on layer TEXT. etch: single
    /// contours on layer ETCH, and the sheet keeps its material there. Text machined to a depth
    /// is always on ETCH. Default: cut.
    #[serde(default)]
    pub text: Option<DxfTextParam>,
    /// A note beside each bend line on layer BEND_NOTES, `B1 UP 90° R1`. Default: false.
    #[serde(default)]
    pub bend_notes: Option<bool>,
    /// How high those notes' letters are, in mm. Default: 3; only a call with `bend_notes` uses
    /// it, but a value out of range is refused either way.
    #[serde(default)]
    pub note_height: Option<DxfNoteHeight>,
    /// The bend lines on layers BEND_UP and BEND_DOWN. Default: true; false leaves them out, for
    /// a cutter that would cut them.
    #[serde(default)]
    pub bend_lines: Option<bool>,
    /// roles: the layer for each role. single: every entity on layer 0. Default: roles.
    #[serde(default)]
    pub layers: Option<DxfLayersParam>,
    /// A file for every sheet body, or their DXF texts when there is no `path`. Not with `body`.
    /// Default: false.
    #[serde(default)]
    pub per_body: Option<bool>,
}

/// A DXF export's `curves`: exact or arcs, in any case (read by `sheet::dxf_curves`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DxfCurvesParam(pub(crate) crate::cmd::DxfCurves);

impl<'de> Deserialize<'de> for DxfCurvesParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        v.as_str().and_then(crate::cmd::dxf_curves).map(DxfCurvesParam).ok_or_else(|| D::Error::custom("must be exact or arcs"))
    }
}

impl JsonSchema for DxfCurvesParam {
    fn schema_name() -> Cow<'static, str> {
        "DxfCurves".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "exact or arcs, in any case.", "type": "string", "pattern": "^([Ee][Xx][Aa][Cc][Tt]|[Aa][Rr][Cc][Ss])$"})
    }
}

/// A DXF export's `text`: cut or etch, in any case (read by `sheet::dxf_text`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DxfTextParam(pub(crate) crate::cmd::DxfText);

impl<'de> Deserialize<'de> for DxfTextParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        v.as_str().and_then(crate::cmd::dxf_text).map(DxfTextParam).ok_or_else(|| D::Error::custom("must be cut or etch"))
    }
}

impl JsonSchema for DxfTextParam {
    fn schema_name() -> Cow<'static, str> {
        "DxfText".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "cut or etch, in any case.", "type": "string", "pattern": "^([Cc][Uu][Tt]|[Ee][Tt][Cc][Hh])$"})
    }
}

/// A DXF export's `layers`: false for the layer of each role, true for everything on layer 0, in
/// any case (read by `sheet::dxf_layers`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DxfLayersParam(pub(crate) bool);

impl<'de> Deserialize<'de> for DxfLayersParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        v.as_str().and_then(crate::cmd::dxf_layers).map(DxfLayersParam).ok_or_else(|| D::Error::custom("must be roles or single"))
    }
}

impl JsonSchema for DxfLayersParam {
    fn schema_name() -> Cow<'static, str> {
        "DxfLayers".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "roles or single, in any case.", "type": "string", "pattern": "^([Rr][Oo][Ll][Ee][Ss]|[Ss][Ii][Nn][Gg][Ll][Ee])$"})
    }
}

/// A DXF bend note's height: a number of mm from 0.1 to 100 (read by `sheet::dxf_note_height`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DxfNoteHeight(pub f64);

impl<'de> Deserialize<'de> for DxfNoteHeight {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        v.as_f64().and_then(crate::cmd::dxf_note_height).map(DxfNoteHeight).ok_or_else(|| D::Error::custom("must be a number of mm from 0.1 to 100"))
    }
}

impl JsonSchema for DxfNoteHeight {
    fn schema_name() -> Cow<'static, str> {
        "DxfNoteHeight".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "How high a bend note's letters are, in mm.",
            "type": "number",
            "minimum": 0.1,
            "maximum": 100.0
        })
    }
}

/// What `sheet.export_dxf` returns: the file it wrote, the DXF text, or one of each per sheet body.
#[derive(Debug, serde::Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ExportDxfResult {
    /// The DXF went to `path`.
    Written {
        path: String,
        /// How many bend lines the drawing has, which its notes and bend layers describe.
        bends: usize,
        /// In the warn parameter-check mode: what the call's parameters got wrong.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        param_warnings: Vec<String>,
    },
    /// No `path`: the DXF text.
    Text {
        dxf: String,
        /// How many bend lines the drawing has.
        bends: usize,
        /// In the warn parameter-check mode: what the call's parameters got wrong.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        param_warnings: Vec<String>,
    },
    /// `per_body`: one entry per sheet body, in the order the design lists them.
    Files {
        files: Vec<DxfFile>,
        /// In the warn parameter-check mode: what the call's parameters got wrong.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        param_warnings: Vec<String>,
    },
}

/// One sheet's DXF in a `per_body` reply: the file it wrote, or its text when there is no path.
#[derive(Debug, serde::Serialize, JsonSchema)]
pub struct DxfFile {
    /// The sheet metal body this drawing is for.
    pub body: String,
    /// The file this drawing went to. Left out when there is no `path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The DXF text. Left out when it went to a file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dxf: Option<String>,
    /// How many bend lines this drawing has.
    pub bends: usize,
}

/// The form of a bend table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BendTableFormat {
    Csv,
    Json,
}

/// A bend table's `format`, in any case (read by `sheet::bend_table_format`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BendTableFormatParam(pub BendTableFormat);

impl<'de> Deserialize<'de> for BendTableFormatParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        v.as_str().and_then(crate::cmd::bend_table_format).map(BendTableFormatParam).ok_or_else(|| D::Error::custom("must be csv or json"))
    }
}

impl JsonSchema for BendTableFormatParam {
    fn schema_name() -> Cow<'static, str> {
        "BendTableFormat".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "csv or json, in any case.", "type": "string", "pattern": "^([Cc][Ss][Vv]|[Jj][Ss][Oo][Nn])$"})
    }
}

/// A point `[x, y]` in flat coordinates, in mm.
pub type FlatPoint = [f64; 2];

/// One curve of a cut-out's exact shape.
#[derive(Debug, serde::Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FlatSegment {
    /// A straight line from its first point to its second.
    Line([FlatPoint; 2]),
    /// A circular arc.
    Arc {
        center: FlatPoint,
        radius: f64,
        /// Where the arc starts, in degrees counterclockwise from +X.
        start_deg: f64,
        /// How far it turns, in degrees: positive counterclockwise.
        sweep_deg: f64,
    },
    /// Any other curve (an ellipse, a spline), as points within 0.001 mm of it.
    Polyline(Vec<FlatPoint>),
}

/// A circle's centre and radius, in mm.
#[derive(Debug, serde::Serialize, JsonSchema)]
pub struct FlatCircle {
    pub center: FlatPoint,
    pub radius: f64,
}

/// A cut-out's exact shape: a whole circle, or its curves in order.
#[derive(Debug, serde::Serialize, JsonSchema)]
#[serde(untagged)]
pub enum FlatShape {
    Circle { circle: FlatCircle },
    Segments { segments: Vec<FlatSegment> },
}

/// Which side of the sheet a pocket is machined from.
#[derive(Debug, serde::Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PocketSide {
    /// From the flat's bottom face.
    Bottom,
    /// From its top face.
    Top,
    /// Inside the sheet, open to neither face.
    Internal,
}

/// A pocket: material machined to a depth, not through the sheet.
#[derive(Debug, serde::Serialize, JsonSchema)]
pub struct FlatPocket {
    /// Its outer contour, as points within 0.001 mm.
    pub outline: Vec<FlatPoint>,
    /// Islands inside it that keep their material, as points within 0.001 mm.
    pub cutouts: Vec<Vec<FlatPoint>>,
    /// The same islands exactly, in the same order.
    pub cutout_shapes: Vec<FlatShape>,
    pub side: PocketSide,
    /// `z1_mm` less `z0_mm`.
    pub depth_mm: f64,
    /// The pocket's bottom and top, measured from the flat's bottom face, in mm.
    pub z0_mm: f64,
    pub z1_mm: f64,
}

/// Which way a bend folds: up toward the flat's top face (+z), down toward its bottom.
#[derive(Debug, serde::Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BendDirection {
    Up,
    Down,
}

/// A bend of the flat pattern.
#[derive(Debug, serde::Serialize, JsonSchema)]
pub struct FlatBend {
    /// The bend's number, from 1. The stretches that cut-outs leave of one bend each have a row,
    /// with the same number.
    pub bend: usize,
    /// The bend angle, in degrees.
    pub angle_deg: f64,
    /// The angle left between the bend's sides, in degrees: 180 less `angle_deg`.
    pub included_angle_deg: f64,
    /// The inner bend radius, in mm.
    pub radius: f64,
    pub direction: BendDirection,
    /// The bend allowance (the bend zone's width in the flat), in mm.
    pub allowance: f64,
    /// The length of this stretch of the bend's centre line, in mm.
    pub length: f64,
    /// The bend's centre line, from `start` to `end`.
    pub start: FlatPoint,
    pub end: FlatPoint,
}

/// How a perforation field lays out its holes.
#[derive(Debug, serde::Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FlatPerforationPattern {
    /// Rows offset for a 60° pattern.
    Staggered,
    Square,
}

/// A perforation field: round holes in the flat pattern, not cut into the solid.
#[derive(Debug, serde::Serialize, JsonSchema)]
pub struct FlatPerforation {
    /// The `sheet.perforate` feature's id.
    pub feature: u64,
    /// The holes' diameter, the distance between their centres and how far they stay clear of
    /// edges, cut-outs and bends, in mm.
    pub diameter: f64,
    pub pitch: f64,
    pub margin: f64,
    pub pattern: FlatPerforationPattern,
    /// How many holes the field has.
    pub count: usize,
    /// The holes' area together, in mm².
    pub open_area_mm2: f64,
}

/// A hole drilled along a panel into its rim, drilled after cutting and not in the outline.
#[derive(Debug, serde::Serialize, JsonSchema)]
pub struct FlatBore {
    /// Where it starts, in flat coordinates with z up from the bottom face, in mm.
    pub start: [f64; 3],
    /// Its direction, a unit vector.
    pub axis: [f64; 3],
    /// Its diameter and length, in mm.
    pub diameter: f64,
    pub length: f64,
}

/// What `sheet.flat_pattern` returns: the sheet's flat pattern in flat coordinates (mm, on the
/// base panel's bottom face). The `json` form of `sheet.export_bend_table` is this as text.
#[derive(Debug, serde::Serialize, JsonSchema)]
pub struct FlatPatternResult {
    /// The sheet metal body.
    pub body: String,
    /// Its sheet metal rule, by name.
    pub rule: String,
    /// The sheet's thickness, in mm.
    pub thickness: f64,
    /// The K-factor its bends unfold with.
    pub k_factor: f64,
    /// The flat pattern's width and height (its bounding box) and its thickness, in mm.
    pub flat_size_mm: [f64; 3],
    /// The outer contours, one per piece, as points within 0.001 mm.
    pub outline: Vec<Vec<FlatPoint>>,
    /// The cut-outs through the sheet, as points within 0.001 mm.
    pub cutouts: Vec<Vec<FlatPoint>>,
    /// The same cut-outs exactly, in the same order.
    pub cutout_shapes: Vec<FlatShape>,
    pub pockets: Vec<FlatPocket>,
    pub bends: Vec<FlatBend>,
    /// Whether the sheet is unfolded now.
    pub unfolded: bool,
    pub perforations: Vec<FlatPerforation>,
    /// Each bend zone's bounding box, `[min x, max x, min y, max y]` in mm.
    pub bend_zones: Vec<[f64; 4]>,
    /// Left out when there are none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bores: Vec<FlatBore>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sheet.export_bend_table` returns: the file it wrote, or the table's text.
#[derive(Debug, serde::Serialize, JsonSchema)]
#[serde(untagged)]
pub enum BendTableResult {
    /// The table went to `path`.
    Written {
        path: String,
        format: BendTableFormat,
        /// How many rows the table has: one per bend, or per stretch of a bend that cut-outs split.
        bends: usize,
        /// In the warn parameter-check mode: what the call's parameters got wrong.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        param_warnings: Vec<String>,
    },
    /// No `path`: the table's text.
    Text {
        format: BendTableFormat,
        text: String,
        /// How many rows the table has: one per bend, or per stretch of a bend that cut-outs split.
        bends: usize,
        /// In the warn parameter-check mode: what the call's parameters got wrong.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        param_warnings: Vec<String>,
    },
}

#[cfg(test)]
#[path = "sheet_tests.rs"]
mod tests;
