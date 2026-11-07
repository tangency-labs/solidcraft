//! Contracts and reply types for the plastic rule commands in `cmd/plastic.rs` (#12313): Manage,
//! Assign and List Plastic Rules. A plastic rule holds a plastic part's design values, which
//! Boss, Lip, Snap Fit and Rest take as defaults.
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. Whether a rule or a body exists, and whether
//! an expression evaluates, stay the handlers' to check. Where a handler ignores a key of the
//! wrong type (a `delete` that isn't true or false, a rule value that is neither a number nor
//! text), the contract refuses it.

use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use solidcraft_doc::plastic::{PlasticRule, PlasticValues};

use super::check::whole;
use super::config::NameList;
use super::types::{AngleExpr, LengthExpr, not_blank_pattern};

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
    // Optional in the type, required in the schema: left out, `null` or empty, the handler
    // refuses the call with its own message, so the schema allows none of those.
    #[serde(default)]
    #[schemars(schema_with = "bodies_schema")]
    pub bodies: Option<NameList>,
    /// The rule, by name, as `plastic.rules` lists it. Empty or left out: the bodies have no
    /// rule of their own, so the active rule applies.
    pub rule: Option<String>,
}

/// `bodies` as `plastic.assign_rule` takes it: a name, or a list of 1 to 10 000 names. Not
/// `NameList`'s own schema, which allows the empty list the handler refuses.
fn bodies_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "anyOf": [
            {"type": "string"},
            {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": crate::params::MAX_STRING_LIST}
        ]
    })
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

#[cfg(test)]
#[path = "plastic_tests.rs"]
mod tests;
