//! Contracts for the sheet metal feature commands in `cmd/sheet.rs`. They all return
//! [`FeatureResult`](super::features::FeatureResult).
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
use solidcraft_doc::FlangeEdge;
use solidcraft_geom::Vec3;

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
        json_schema!({
            "description": "Points in mm on the sheet's top or bottom edges, 1 to 1000: each [x, y, z] or {\"x\", \"y\", \"z\"}, or {\"point\": [x, y, z], \"angle\"?, \"height\"?} for an edge with its own angle and height (null: the feature's).",
            "type": "array",
            "items": {
                "anyOf": [
                    // An object with any of the edge's keys is an edge, never a bare point.
                    {"anyOf": [point], "not": {"type": "object", "anyOf": [{"required": ["point"]}, {"required": ["angle"]}, {"required": ["height"]}]}},
                    {
                        "type": "object",
                        "properties": {"point": point, "angle": {"anyOf": [angle, {"type": "null"}]}, "height": {"anyOf": [height, {"type": "null"}]}},
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
/// chain of sketch lines, or edge flanges on a sheet's edges. Thickness, K-factor, bend radius
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
    /// Contour: the open chain's lines (curve ids; arcs aren't supported yet), 1 to 1000.
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
    /// Edge: how far each flange stops short of the start of its edge, which runs the way its
    /// panel's outline does (counter-clockwise seen from the top face); the sheet is notched and
    /// relieved there. Default: 0. Refused on a base or contour flange.
    #[serde(default)]
    pub start_setback: Option<LengthExpr>,
    /// Edge: how far each flange stops short of the end of its edge, as `start_setback` does at
    /// the start. Default: 0. Refused on a base or contour flange.
    #[serde(default)]
    pub end_setback: Option<LengthExpr>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Add a field of round perforations over a whole flat sheet: holes in the flat pattern, the DXF
/// (layer PERFORATION) and the measured volume, not cut into the solid. `participants` changes
/// nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PerforateParams {
    /// The holes' diameter, more than 0.
    pub diameter: LengthExpr,
    /// The distance between hole centres, more than the diameter.
    pub pitch: LengthExpr,
    /// How far the holes stay clear of the sheet's edges, cut-outs and bends, at least 0.
    /// Default: the diameter.
    #[serde(default)]
    pub margin: Option<LengthExpr>,
    /// How the holes are laid out. Default: staggered.
    #[serde(default)]
    pub pattern: Option<PerforationPattern>,
    /// The sheet metal body. Default: the newest sheet.
    #[serde(default)]
    pub body: Option<String>,
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

#[cfg(test)]
#[path = "sheet_tests.rs"]
mod tests;
