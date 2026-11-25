//! Contracts and result types for `sheet.check_shop` (`cmd/sheet_check.rs`, #20113).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// Check a sheet metal part against a fabrication shop's file and design rules. Read-only.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct CheckShopParams {
    /// The sheet metal body. Default: the last sheet.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub body: Option<String>,
    /// The shop: a built-in profile's id (`generic`, `pcbway`, `jlccnc`, `sendcutsend`), the path
    /// of a profile file (ends in `.json`), or a profile object (`docs/formats/shop-rules.md`).
    pub shop: ShopParam,
    /// What the flat DXF is exported with, for the file rules (`file.dxf_single_layer`,
    /// `file.dxf_closed_contours`, `file.dxf_bend_lines`, `file.text_outlined`): the options
    /// `sheet.export_dxf` takes. Default: its defaults (layers by role, bend lines, no notes).
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "DxfCheckParams")]
    pub dxf: Option<DxfCheckParams>,
}

/// The options of the DXF the file rules are checked for, as `sheet.export_dxf` takes them.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct DxfCheckParams {
    /// roles: the layer for each role. single: everything on layer 0. Default: roles.
    #[serde(default)]
    pub layers: Option<super::sheet::DxfLayersParam>,
    /// The bend lines are drawn. Default: true.
    #[serde(default)]
    pub bend_lines: Option<bool>,
    /// A note beside each bend line, a text entity. Default: false.
    #[serde(default)]
    pub bend_notes: Option<bool>,
    /// A note beside each press-in hardware hole, a text entity; not with `layers: single`.
    /// Default: false.
    #[serde(default)]
    pub hardware_notes: Option<bool>,
}

/// A check's `shop`: a built-in id or a file's path, or a profile as an object. The command reads
/// and checks it, so its refusals name the rule and the value.
#[derive(Clone, Debug, PartialEq)]
pub enum ShopParam {
    Name(String),
    Profile(serde_json::Map<String, Value>),
}

impl<'de> Deserialize<'de> for ShopParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(name) if !name.trim().is_empty() => Ok(ShopParam::Name(name)),
            Value::Object(profile) => Ok(ShopParam::Profile(profile)),
            _ => Err(D::Error::custom("must be a shop profile's id, a profile file's path, or a profile object")),
        }
    }
}

impl JsonSchema for ShopParam {
    fn schema_name() -> Cow<'static, str> {
        "ShopProfile".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A built-in shop profile's id (generic, pcbway, jlccnc, sendcutsend), a profile file's path (ends in .json), or the profile itself: {id?, name, note?, rules: [{id, unit?, process?, limits?, stock?, wanted?, sources?, disagreement?}]}.",
            "anyOf": [
                {"type": "string", "minLength": 1},
                {"type": "object", "required": ["name", "rules"]}
            ]
        })
    }
}

/// How serious a finding is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShopSeverity {
    /// The part breaks a rule the shop states.
    Error,
    /// The shop's pages disagree, or the note is advice.
    Warning,
}

/// The shop checked against.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ShopInfo {
    pub id: String,
    pub name: String,
}

/// One rule the part breaks.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ShopFinding {
    /// The rule's id (`hole.min_diameter`).
    pub rule: String,
    pub severity: ShopSeverity,
    /// What is wrong and the number it broke, in words.
    pub message: String,
    /// The feature it names, when it names one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature: Option<u64>,
    /// Where, `[x, y]` in the flat as it is read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<[f64; 2]>,
    /// The part's value (mm), where the rule is a number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// The shop's limit (mm).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<f64>,
    /// The shop's pages the rule comes from.
    pub sources: Vec<String>,
}

/// A rule not looked at, or not wholly.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ShopNotChecked {
    pub rule: String,
    pub why: String,
}

/// What `sheet.check_shop` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CheckShopResult {
    pub shop: ShopInfo,
    /// The sheet's thickness (mm).
    pub thickness: f64,
    /// By rule, in the order of `checked`, then by hole.
    pub findings: Vec<ShopFinding>,
    /// The rules looked at.
    pub checked: Vec<String>,
    /// The rules the shop gives no value for, and what could not be measured, with the reason.
    pub not_checked: Vec<ShopNotChecked>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
