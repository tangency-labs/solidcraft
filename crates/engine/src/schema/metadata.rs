//! Contracts and result types for the commands in `cmd/metadata.rs`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Read the design's properties: its `metadata` object as saved, and SolidCraft's own design
/// properties (project, part number, revision…) as text.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MetadataParams {}

/// What `document.metadata` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MetadataResult {
    /// The design's metadata object as saved, other tools' keys included.
    pub metadata: Map<String, Value>,
    /// Each of SolidCraft's design properties, by key: its text, or null when it is absent or
    /// not text.
    pub properties: Map<String, Value>,
}

/// Change design properties in one undo step: `key` and `value`, or `values` for several at
/// once (all or nothing). A change that sets what is already there makes no undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MetadataSetParams {
    /// The property to set: one of SolidCraft's keys (the ones `document.metadata`'s
    /// `properties` lists), which take text, or another tool's reverse-DNS key such as
    /// `com.example.tool`, which takes an object. `bom_type` is a component's property and is
    /// refused here. Needs `value`; not with `values`.
    #[serde(default)]
    pub key: Option<String>,
    /// The value for `key`: text for SolidCraft's keys, an object for another tool's. `""` or
    /// null removes the key. Required with `key`.
    #[serde(default)]
    #[schemars(with = "Value")]
    pub value: Option<Value>,
    /// Several properties at once, `{key: value, …}`, at most 256 keys, each as `key` and
    /// `value` take them. Not with `key`.
    #[serde(default)]
    pub values: Option<Map<String, Value>>,
}

/// What `document.metadata.set` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MetadataSetResult {
    /// The design's metadata object after the change.
    pub metadata: Map<String, Value>,
    /// The keys whose value changed, in key order.
    pub changed: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Remove design properties in one undo step: `key`, or `keys` for several. Any key can go,
/// also one another tool wrote; a key that isn't there is left out of `removed`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MetadataRemoveParams {
    /// The property to remove. Not with `keys`.
    #[serde(default)]
    pub key: Option<String>,
    /// The properties to remove, at most 256. Not with `key`.
    #[serde(default)]
    pub keys: Option<Vec<String>>,
}

/// What `document.metadata.remove` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MetadataRemoveResult {
    /// The design's metadata object after the change.
    pub metadata: Map<String, Value>,
    /// The keys that were there and are removed, each once, in the order asked.
    pub removed: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "metadata_tests.rs"]
mod tests;
