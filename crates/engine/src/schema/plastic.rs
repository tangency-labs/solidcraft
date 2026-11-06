//! Contracts and reply types for the commands in `cmd/plastic.rs`: the plastic rule commands
//! Manage, Assign and List Plastic Rules (#12313), and the plastic features Boss, Lip, Snap Fit
//! and Rest (#12622). A plastic rule holds a plastic part's design values, which the features
//! take as defaults.
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. Whether a rule, a body or a face exists, and
//! whether an expression evaluates and fits the part, stay the handlers' to check. Where a
//! handler ignores a key of the wrong type (a `delete` that isn't true or false, a rule value or
//! a feature's optional size that is neither a number nor text), the contract refuses it.

use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use solidcore_record::Vec3;
use solidcraft_doc::plastic::{PlasticRule, PlasticValues};
use solidcraft_geom::Vec3Geom as _;

use super::check::whole;
use super::config::NameList;
use super::types::{
    AngleExpr, FeatureCommon, LengthExpr, LipSideParam, LipTypeParam, NumberExpr, Point3, TouchingParam, feature_common, not_blank_pattern, present,
};

/// The longest rule name, in bytes once trimmed, as `plastic.manage_rules` reads it.
const MAX_RULE_NAME_BYTES: usize = 128;

/// `serde(default)` makes schemars publish `"default": null` on a key; drop it from a key the
/// schema requires.
fn drop_default(schema: &mut Schema) {
    schema.remove("default");
}

/// Adds `key` to the schema's `required` list: for a key the type has optional so that the
/// handler, not the check, refuses it with its own message when it's left out.
/// (`#[schemars(required)]` doesn't do this for a key with `serde(default)`.)
fn require(schema: &mut Schema, key: &str) {
    match schema.get_mut("required") {
        Some(Value::Array(required)) => {
            if !required.iter().any(|k| k == key) {
                required.push(key.into());
            }
        }
        _ => {
            schema.insert("required".into(), Value::Array(vec![key.into()]));
        }
    }
}

fn name_required(schema: &mut Schema) {
    require(schema, "name");
}

fn bodies_required(schema: &mut Schema) {
    require(schema, "bodies");
}

/// Make or edit a plastic rule, make it the active rule (the one for bodies without a rule of
/// their own), or delete it. A new rule starts from `from`; an existing one keeps the values not
/// given. Each value is a number or an expression, and may use the design's parameters; in a
/// rule's other values, `Thickness` is that rule's thickness.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = name_required)]
pub struct ManageRulesParams {
    /// The rule, by name: a library rule (to edit it in this design) or a new one. Trimmed,
    /// 1 to 128 bytes.
    // Optional in the type, required in the schema: left out, the handler refuses the call
    // with its own message.
    #[serde(default, deserialize_with = "rule_name")]
    #[schemars(schema_with = "rule_name_schema", transform = drop_default)]
    pub name: Option<String>,
    /// For a new rule: the rule it starts from. Default: "ABS (1.5mm)".
    pub from: Option<String>,
    /// The material the rule is for, as a note (at most 128 characters are kept).
    pub material: Option<String>,
    /// The nominal wall thickness.
    pub thickness: Option<LengthExpr>,
    /// The nominal radius of rounds.
    pub nominal_radius: Option<LengthExpr>,
    /// The clearance between mating parts: a groove's gap when it gives none.
    pub clearance: Option<LengthExpr>,
    pub knife_edge: Option<LengthExpr>,
    pub reveal_height: Option<LengthExpr>,
    /// How much a wall's thickness may vary.
    pub thickness_variation: Option<LengthExpr>,
    /// The draft of plastic features when they give none.
    pub draft: Option<AngleExpr>,
    /// The thickest wall allowed.
    pub max_thickness: Option<LengthExpr>,
    /// The thinnest wall allowed.
    pub min_thickness: Option<LengthExpr>,
    /// The least draft allowed.
    pub min_draft: Option<AngleExpr>,
    /// True makes this the active rule; false stops it being the active rule.
    pub active: Option<bool>,
    /// True deletes the rule from the design (a library rule goes back to the library's
    /// values). Default: false.
    pub delete: Option<bool>,
}

/// A rule's name as `plastic.manage_rules` reads it: text that isn't blank, at most 128 bytes
/// once trimmed, and the handler's message for anything else.
fn rule_name<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::String(name) if !name.trim().is_empty() && name.trim().len() <= MAX_RULE_NAME_BYTES => Ok(Some(name.trim().to_string())),
        _ => Err(D::Error::custom(whole("`name` is required"))),
    }
}

/// The pattern says "not blank"; at most 128 bytes once trimmed, which JSON Schema can't count,
/// is in the field's description and checked by [`rule_name`].
fn rule_name_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "minLength": 1, "pattern": not_blank_pattern()})
}

/// Assign a plastic rule to bodies, or take theirs away. Plastic features on those bodies take
/// the rule's draft and clearance as defaults.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = bodies_required)]
pub struct AssignRuleParams {
    /// The bodies, by name: 1 to 10 000.
    // Optional in the type, required in the schema: left out or empty, the handler refuses the
    // call with its own message.
    #[serde(default)]
    pub bodies: Option<NameList>,
    /// The rule, by name, as `plastic.rules` lists it. Empty or left out: the bodies have no
    /// rule of their own, so the active rule applies.
    pub rule: Option<String>,
}

/// List the plastic rules (the library's and the design's), the active rule and the rules
/// assigned to bodies. No parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RulesParams {}

/// A rule's values as written: numbers or expressions, as text.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RuleExpressions {
    pub thickness: String,
    pub nominal_radius: String,
    pub clearance: String,
    pub knife_edge: String,
    pub reveal_height: String,
    pub thickness_variation: String,
    pub draft: String,
    pub max_thickness: String,
    pub min_thickness: String,
    pub min_draft: String,
}

impl From<&PlasticRule> for RuleExpressions {
    fn from(r: &PlasticRule) -> Self {
        Self {
            thickness: r.thickness.clone(),
            nominal_radius: r.nominal_radius.clone(),
            clearance: r.clearance.clone(),
            knife_edge: r.knife_edge.clone(),
            reveal_height: r.reveal_height.clone(),
            thickness_variation: r.thickness_variation.clone(),
            draft: r.draft.clone(),
            max_thickness: r.max_thickness.clone(),
            min_thickness: r.min_thickness.clone(),
            min_draft: r.min_draft.clone(),
        }
    }
}

/// A rule's values, evaluated: lengths in mm, angles in degrees.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RuleValues {
    pub thickness: f64,
    pub nominal_radius: f64,
    pub clearance: f64,
    pub knife_edge: f64,
    pub reveal_height: f64,
    pub thickness_variation: f64,
    pub draft_deg: f64,
    pub max_thickness: f64,
    pub min_thickness: f64,
    pub min_draft_deg: f64,
}

impl From<PlasticValues> for RuleValues {
    fn from(v: PlasticValues) -> Self {
        Self {
            thickness: v.thickness,
            nominal_radius: v.nominal_radius,
            clearance: v.clearance,
            knife_edge: v.knife_edge,
            reveal_height: v.reveal_height,
            thickness_variation: v.thickness_variation,
            draft_deg: v.draft.to_degrees(),
            max_thickness: v.max_thickness,
            min_thickness: v.min_thickness,
            min_draft_deg: v.min_draft.to_degrees(),
        }
    }
}

/// A rule's evaluated values, or why they don't evaluate (an expression naming a parameter the
/// design no longer has, say).
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum RuleValuesOrError {
    Values(RuleValues),
    Error { error: String },
}

/// A plastic rule, as the rule commands give it back.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RuleInfo {
    pub name: String,
    /// The material the rule is for, as a note; may be empty.
    pub material: String,
    /// Whether the rule is one of the library's (edited in this design, or not).
    pub library: bool,
    pub expressions: RuleExpressions,
    pub values: RuleValuesOrError,
}

/// What `plastic.manage_rules` returns: the rule made or edited, with the active rule, or the
/// rule deleted.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ManageRulesResult {
    Changed {
        rule: Box<RuleInfo>,
        /// The active rule, if any.
        active: Option<String>,
        /// In the warn parameter-check mode: what the call's parameters got wrong.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        param_warnings: Vec<String>,
    },
    Deleted {
        deleted: String,
        /// In the warn parameter-check mode: what the call's parameters got wrong.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        param_warnings: Vec<String>,
    },
}

/// What `plastic.assign_rule` returns: the bodies and the rule they now have, or `null` for
/// none.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AssignRuleResult {
    pub bodies: Vec<String>,
    pub rule: Option<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `plastic.rules` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RulesResult {
    /// The library's rules, as this design has edited them, then the design's own.
    pub rules: Vec<RuleInfo>,
    /// The active rule, if any.
    pub active: Option<String>,
    /// Each body with a rule of its own: body name to rule name.
    pub assigned: BTreeMap<String, String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

// ---- Plastic features (#12622) ----
//
// Each feature joins the body it stands on (a groove cuts it) and replies as every feature
// command does, with `FeatureResult`. The common keys (`name`, `body_name`, `body_names`,
// `participants`) are `FeatureCommon`'s; `touching` is declared here because the features join.

/// `v` as `params::vec3` reads it. With `direction`, it must also have a length, since the
/// handlers make it a unit vector and refuse a zero one.
fn vector(v: &Value, direction: bool) -> Option<Vec3> {
    let p = crate::params::vec3(v)?;
    if direction { p.normalized() } else { Some(p) }
}

/// A point or direction key of the plastic features: read as [`vector`] reads it, refused with
/// the handler's whole message, published as a [`Point3`] with the key's own description.
macro_rules! plastic_vector {
    ($(#[$doc:meta])* $name:ident, direction: $direction:literal, $message:literal, $description:literal) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct $name(pub Vec3);

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                vector(&Value::deserialize(d)?, $direction).map($name).ok_or_else(|| D::Error::custom(whole($message)))
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
                let mut schema = generator.subschema_for::<Point3>();
                schema.insert("description".into(), $description.into());
                schema
            }
        }
    };
}

plastic_vector!(
    /// Boss, Snap Fit and Rest's `position`.
    FacePosition,
    direction: false,
    "`position` must be a point [x, y, z] on a face",
    "A point in mm on the face the feature stands on: [x, y, z] or {\"x\", \"y\", \"z\"}."
);
plastic_vector!(
    /// Boss, Snap Fit and Rest's `direction`.
    FaceDirection,
    direction: true,
    "`direction` must be a non-zero [x, y, z]",
    "The direction the feature stands out of the face, not zero. Default: square out of the face at `position`, which must then be planar."
);
plastic_vector!(
    /// Lip's `face`.
    RimFace,
    direction: false,
    "`face` must be a point [x, y, z] on the rim face",
    "A point in mm on the planar rim face of a shelled body, round its opening: [x, y, z] or {\"x\", \"y\", \"z\"}."
);
plastic_vector!(
    /// Snap Fit's `hook`.
    HookDirection,
    direction: true,
    "`hook` must be the catch's direction [x, y, z]",
    "The direction the catch sticks out of the arm: across `direction`, not zero."
);
plastic_vector!(
    /// Rest's `along`.
    AlongDirection,
    direction: true,
    "`along` must be a direction across the face",
    "The direction of a rectangular rest's `length`: across `direction`, not zero."
);

/// Add a boss: a round post standing on a face, for a screw, with an optional hole, draft, root
/// fillet and ribs. It joins the body it stands on. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct BossParams {
    pub position: FacePosition,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FaceDirection")]
    pub direction: Option<FaceDirection>,
    /// The post's diameter at the face, at least 0.01 mm.
    pub diameter: LengthExpr,
    /// How far the post stands out of the face, at least 0.01 mm.
    pub height: LengthExpr,
    /// The screw hole's diameter, down from the post's top. Default: no hole.
    #[serde(default)]
    pub hole_diameter: Option<LengthExpr>,
    /// How deep the hole goes from the post's top. Default: down to the face (the post's height).
    #[serde(default)]
    pub hole_depth: Option<LengthExpr>,
    /// The post's draft: it narrows towards its top by this angle. Default: the body's plastic
    /// rule's draft, or none.
    #[serde(default)]
    pub draft: Option<AngleExpr>,
    /// The radius of the round where the post meets the face. Default: none.
    #[serde(default)]
    pub fillet: Option<LengthExpr>,
    /// How many ribs brace the post, evenly round it: 0 to 64. Default: none.
    #[serde(default)]
    pub ribs: Option<NumberExpr>,
    /// Each rib's thickness. Default: 0.3 times the post's radius.
    #[serde(default)]
    pub rib_thickness: Option<LengthExpr>,
    /// How far each rib reaches out from the post's wall. Default: the post's radius.
    #[serde(default)]
    pub rib_length: Option<LengthExpr>,
    /// How far below the post's top the ribs stop. Default: 0, the ribs reach the top.
    #[serde(default)]
    pub rib_offset: Option<LengthExpr>,
    /// The body the boss joins, by name. Default: the body at `position`.
    #[serde(default)]
    pub body: Option<String>,
    /// What a join makes of pieces that share no face. Default: separate.
    #[serde(default)]
    pub touching: Option<TouchingParam>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Add a lip or a groove: a band along one edge of a shelled body's rim face, raised for a lip
/// and cut for a groove, so two halves of a case fit together. A lip joins the body and a groove
/// cuts it. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct LipParams {
    pub face: RimFace,
    /// The band's width across the rim.
    pub width: LengthExpr,
    /// A lip's height above the rim, or a groove's depth into it.
    pub height: LengthExpr,
    #[serde(default, rename = "type")]
    pub kind: Option<LipTypeParam>,
    /// A groove's clearance: it is this much wider than `width`, at least 0. Default: the body's
    /// plastic rule's clearance, or none. A lip doesn't use it.
    #[serde(default)]
    pub gap: Option<LengthExpr>,
    #[serde(default)]
    pub side: Option<LipSideParam>,
    /// True: the band runs along the outside edge, as `side: outside` does. Default: false.
    #[serde(default)]
    pub outside: Option<bool>,
    /// The shelled body, by name. Default: the body at `face`.
    #[serde(default)]
    pub body: Option<String>,
    /// What a lip's join makes of pieces that share no face. Default: separate.
    #[serde(default)]
    pub touching: Option<TouchingParam>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Add a snap fit: a cantilever arm standing on a face with a catch at its tip, which clips into
/// another part. It joins the body it stands on, or makes a body of its own. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SnapFitParams {
    pub position: FacePosition,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FaceDirection")]
    pub direction: Option<FaceDirection>,
    pub hook: HookDirection,
    /// The arm's length out of the face, at least 0.01 mm.
    pub length: LengthExpr,
    /// The arm's thickness, along `hook`, at least 0.01 mm.
    pub thickness: LengthExpr,
    /// The arm's width, across `hook`, at least 0.01 mm.
    pub width: LengthExpr,
    /// How far the catch sticks out of the arm, at least 0.01 mm.
    pub catch_depth: LengthExpr,
    /// The catch's length along the arm, at the arm's tip: at least 0.01 mm and shorter than
    /// `length`.
    pub catch_length: LengthExpr,
    /// How far the arm's base reaches below the face, into the wall, at least 0.01 mm. Default:
    /// none, the arm starts at the face.
    #[serde(default)]
    pub base_height: Option<LengthExpr>,
    /// True: the snap fit is a body of its own instead of joining the body. Default: false.
    #[serde(default)]
    pub new_body: Option<bool>,
    /// The body it stands on, by name. Default: the body at `position`.
    #[serde(default)]
    pub body: Option<String>,
    /// What a join makes of pieces that share no face. Default: separate.
    #[serde(default)]
    pub touching: Option<TouchingParam>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Add a rest: a pad standing on a face for another part to rest on, round or rectangular, solid
/// or hollow. It joins the body it stands on. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RestParams {
    pub position: FacePosition,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FaceDirection")]
    pub direction: Option<FaceDirection>,
    /// The pad's width at the face: a round rest's diameter.
    pub width: LengthExpr,
    /// A rectangular rest's length, along `along`. Default: a round rest.
    #[serde(default)]
    pub length: Option<LengthExpr>,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "AlongDirection")]
    pub along: Option<AlongDirection>,
    /// How far the pad stands out of the face, at least 0.01 mm.
    pub height: LengthExpr,
    /// The pad's draft, under 57°: it narrows towards its top by this angle. Default: the body's
    /// plastic rule's draft, or none.
    #[serde(default)]
    pub draft: Option<AngleExpr>,
    /// A hollow rest's wall thickness. Default: a solid rest.
    #[serde(default)]
    pub thickness: Option<LengthExpr>,
    /// The body it stands on, by name. Default: the body at `position`.
    #[serde(default)]
    pub body: Option<String>,
    /// What a join makes of pieces that share no face. Default: separate.
    #[serde(default)]
    pub touching: Option<TouchingParam>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

#[cfg(test)]
#[path = "plastic_tests.rs"]
mod tests;
