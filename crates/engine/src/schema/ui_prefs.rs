//! Theme-option contracts for the UI-owned `ui.prefs` control method.
//! Other preferences remain owned and validated by the UI preference table.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ThemeOptionValue {
    Bool(bool),
    Choice(String),
    Num(f32),
}

/// Null resets one option; an empty inner map resets that theme.
pub type ThemeOptionsUpdate = BTreeMap<String, BTreeMap<String, Option<ThemeOptionValue>>>;

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct PrefsParams {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub theme_options: ThemeOptionsUpdate,
    #[serde(flatten)]
    pub settings: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct ThemeChoice {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ThemeOptionKind {
    Bool { default: bool },
    Choice { default: String, values: Vec<ThemeChoice> },
    Range { default: f32, min: f32, max: f32, step: f32, unit: Option<String> },
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct ThemeOptionDef {
    pub id: String,
    #[serde(flatten)]
    pub kind: ThemeOptionKind,
    pub label: String,
    pub help: String,
    pub parent: Option<String>,
    pub advanced: bool,
    pub section: Option<String>,
    pub standard: bool,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ThemeOptionNoticeKind {
    UnknownOption,
    WrongType,
    UnknownChoice,
    NonFinite,
    OutOfRange,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct ThemeOptionNotice {
    pub id: String,
    pub kind: ThemeOptionNoticeKind,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct ThemeOptionsReply {
    pub theme_option_defs: Vec<ThemeOptionDef>,
    pub theme_option_values: BTreeMap<String, ThemeOptionValue>,
    pub theme_option_notices: Vec<ThemeOptionNotice>,
}

/// Complete control reply: other preferences and dialog state share the outer object.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct PrefsReply {
    #[serde(flatten)]
    pub options: ThemeOptionsReply,
    #[serde(flatten)]
    pub settings: BTreeMap<String, Value>,
}
