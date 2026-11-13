//! Contracts and result types for the commands in `cmd/clipboard.rs` and `cmd/script.rs`:
//! `timeline.copy`, `timeline.paste`, `timeline.add` and `timeline.export_script` (#12502).
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message; where a reader is shared with the handler
//! (`copy_keys`, `component_path_form`, `step_params`) the type calls it. Whether a feature, a
//! component or a file exists stays the handler's to check. A key the handler refuses when it is
//! left out (`features`, `feature`) is optional in the type and required in the schema, so that the
//! handler's message, not the check's "is required", is what a caller sees. A call with several
//! problems may get a different one first in strict than with the check off (the check goes key by
//! key, the handlers in their own order).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value, json};
use solidcore_record::Vec3;

use super::check::whole;
use super::component_link::ComponentRef;
use super::motion::require;
use super::types::not_blank_pattern;
use crate::EngineError;
use crate::params;

/// The most features one `timeline.copy` lists, as the handler reads them.
const MAX_COPY: usize = 1000;
/// The most occurrence ids in a `component_path`, as the handler reads them.
const MAX_PATH: usize = 64;

/// The message of a handler's refusal, to pass on as it is.
fn handler_message(e: EngineError) -> String {
    match e {
        EngineError::BadParams { msg, .. } => whole(msg),
        other => whole(other),
    }
}

// ---- timeline.copy ----

/// The features `timeline.copy` lists: one name, or a list of names and ids, in any mix.
#[derive(Clone, Debug, PartialEq)]
pub struct FeatureKeys(pub Vec<Value>);

impl<'de> Deserialize<'de> for FeatureKeys {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let wrapped = json!({ "features": Value::deserialize(d)? });
        crate::cmd::copy_keys(&wrapped).map(|keys| FeatureKeys(keys.to_vec())).map_err(|e| D::Error::custom(handler_message(e)))
    }
}

impl JsonSchema for FeatureKeys {
    fn schema_name() -> Cow<'static, str> {
        "FeatureKeys".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let key = json!({"anyOf": [{"type": "string"}, {"type": "integer", "minimum": 0}]});
        json_schema!({
            "description": "Features by name or id: one name, or a list of names and ids in any mix.",
            "anyOf": [
                {"type": "string"},
                {"type": "array", "items": key, "minItems": 1, "maxItems": MAX_COPY}
            ]
        })
    }
}

/// Copy features to the session's clipboard, and a snapshot of them for pasting into another
/// design. The features stay where they are; the clipboard replaces what was copied before. Not an
/// undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = features_required)]
pub struct CopyParams {
    // Optional in the type and required in the schema (`features_required`).
    /// The features to copy, by name or id: one name, or 1 to 1000 names and ids. Pasted in
    /// timeline order whatever the order given.
    #[serde(default, deserialize_with = "features", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FeatureKeys")]
    pub features: Option<FeatureKeys>,
}

/// `features`, with the handler's message for anything else, `null` and an empty list included.
fn features<'de, D: Deserializer<'de>>(d: D) -> Result<Option<FeatureKeys>, D::Error> {
    FeatureKeys::deserialize(d).map(Some)
}

fn features_required(schema: &mut Schema) {
    require(schema, "features");
}

/// What `timeline.copy` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CopyResult {
    /// The names of the features copied, in timeline order.
    pub copied: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

// ---- timeline.paste ----

/// A `translate` as `timeline.paste` reads it: `[x, y, z]` or `{x, y, z}` of numbers in mm, each
/// under 1e9 in size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PasteTranslation(pub Vec3);

impl<'de> Deserialize<'de> for PasteTranslation {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        params::vec3(&Value::deserialize(d)?).map(PasteTranslation).ok_or_else(|| D::Error::custom(whole("`translate` must be [x, y, z]")))
    }
}

impl JsonSchema for PasteTranslation {
    fn schema_name() -> Cow<'static, str> {
        "PasteTranslation".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let c = json!({"type": "number", "exclusiveMinimum": -1e9, "exclusiveMaximum": 1e9});
        json_schema!({
            "description": "A distance in mm, as [x, y, z] or {\"x\", \"y\", \"z\"}.",
            "anyOf": [
                {"type": "array", "items": c, "minItems": 3, "maxItems": 3},
                {"type": "object", "properties": {"x": c, "y": c, "z": c}, "required": ["x", "y", "z"]}
            ]
        })
    }
}

/// A `component_path` as `timeline.paste` reads it: 0 to 64 occurrence ids.
#[derive(Clone, Debug, PartialEq)]
pub struct PastePath(pub Vec<u64>);

impl<'de> Deserialize<'de> for PastePath {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        crate::cmd::component_path_form(&Value::deserialize(d)?)
            .map(PastePath)
            .ok_or_else(|| D::Error::custom(whole("`component_path` must list 0..64 occurrence ids")))
    }
}

impl JsonSchema for PastePath {
    fn schema_name() -> Cow<'static, str> {
        "PastePath".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The occurrence ids from the top of the design down to a placement.",
            "type": "array",
            "items": {"type": "integer", "minimum": 0},
            "maxItems": MAX_PATH
        })
    }
}

fn paste_component<'de, D: Deserializer<'de>>(d: D) -> Result<Option<ComponentRef>, D::Error> {
    match Value::deserialize(d)? {
        Value::Number(n) => Ok(Some(ComponentRef(n.to_string()))),
        Value::String(s) => Ok(Some(ComponentRef(s))),
        _ => Err(D::Error::custom(whole("`component` must be a component id or name"))),
    }
}

fn paste_path<'de, D: Deserializer<'de>>(d: D) -> Result<Option<PastePath>, D::Error> {
    PastePath::deserialize(d).map(Some)
}

fn paste_translate<'de, D: Deserializer<'de>>(d: D) -> Result<Option<PasteTranslation>, D::Error> {
    PasteTranslation::deserialize(d).map(Some)
}

/// `from_design` is true or false; `null` is refused, as the handler does.
fn from_design<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    match Value::deserialize(d)? {
        Value::Bool(b) => Ok(Some(b)),
        _ => Err(D::Error::custom(whole("`from_design` must be true or false"))),
    }
}

/// Paste the copied features as independent copies at the timeline marker: sketches get their own
/// dimension parameters, features their own names, and references inside the copied set point at
/// the copies. Body transfers are left out. One undo step. With `from_design`, only sketch
/// features are pasted, from the snapshot another design's copy left. Every key reads the same
/// either way.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PasteParams {
    /// Move the copies' geometry by this much, in mm. Default: not moved.
    #[serde(default, deserialize_with = "paste_translate", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PasteTranslation")]
    pub translate: Option<PasteTranslation>,
    /// The component the copies go into, by id or name. Default: the active one.
    #[serde(default, deserialize_with = "paste_component", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ComponentRef")]
    pub component: Option<ComponentRef>,
    /// The placement of that component the copies are authored in, as occurrence ids. Default: the
    /// active placement for the active component, else the component's only placement.
    #[serde(default, deserialize_with = "paste_path", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PastePath")]
    pub component_path: Option<PastePath>,
    /// True pastes the sketch features of the clipboard snapshot of another design, with their
    /// parameters. Default: false, the features copied in this design.
    #[serde(default, deserialize_with = "from_design", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub from_design: Option<bool>,
}

/// A feature a paste made.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Pasted {
    /// The new feature, by id.
    pub id: u64,
    /// Its name.
    pub name: String,
    /// The name of the feature it is a copy of.
    pub from: String,
}

/// A pasted feature that did not rebuild.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PasteError {
    /// The pasted feature, by id.
    pub feature: u64,
    /// Why it did not rebuild.
    pub error: String,
}

/// What `timeline.paste` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PasteResult {
    /// The features made, in timeline order.
    pub pasted: Vec<Pasted>,
    /// The pasted features that did not rebuild.
    pub errors: Vec<PasteError>,
    /// What the paste left out or kept (body transfers, external face attachments).
    pub notices: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

// ---- timeline.add ----

/// The `feature` of `timeline.add`: an object, as a design file or `timeline.inspect` holds a
/// feature.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordedFeature(pub Map<String, Value>);

impl<'de> Deserialize<'de> for RecordedFeature {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Object(o) => Ok(RecordedFeature(o)),
            _ => Err(D::Error::custom(whole("`feature` must be a feature object, as a design file or timeline.inspect has it"))),
        }
    }
}

impl JsonSchema for RecordedFeature {
    fn schema_name() -> Cow<'static, str> {
        "RecordedFeature".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A feature as the design file and timeline.inspect hold it: {type, name?, id?, …}. \
                The name is kept even when another feature has it; the id is kept, and refused when another \
                feature has it. Whether the rest is a feature is the command's to check.",
            "type": "object"
        })
    }
}

/// The model parameters of `timeline.add`: read by the handler's own reader.
#[derive(Clone, Debug)]
pub struct StepParams(pub Vec<solidcraft_doc::Param>);

impl<'de> Deserialize<'de> for StepParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        crate::cmd::step_params("timeline.add", Some(&value)).map(StepParams).map_err(|e| D::Error::custom(handler_message(e)))
    }
}

impl JsonSchema for StepParams {
    fn schema_name() -> Cow<'static, str> {
        "StepParams".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let text = json!({"type": ["string", "null"]});
        json_schema!({
            "description": "Model parameters the feature's sketch dimensions use.",
            "type": "array",
            "maxItems": solidcraft_sketch::MAX_CONSTRAINTS,
            "items": {
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "The parameter's name."},
                    "expression": {
                        "description": "Its value: a number or an expression that is not blank.",
                        "anyOf": [{"type": "string", "pattern": not_blank_pattern()}, {"type": "number"}]
                    },
                    "unit": text.clone(),
                    "comment": text
                },
                "required": ["name", "expression"]
            }
        })
    }
}

fn recorded<'de, D: Deserializer<'de>>(d: D) -> Result<Option<RecordedFeature>, D::Error> {
    RecordedFeature::deserialize(d).map(Some)
}

fn feature_required(schema: &mut Schema) {
    require(schema, "feature");
}

/// Append one recorded feature to the timeline, as the design file holds it, so every feature
/// kind replays exactly: the same ids, names, parameter names and stored edge and face names (the
/// steps `timeline.export_script` writes). It goes at the timeline marker. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = feature_required)]
pub struct AddParams {
    // Optional in the type and required in the schema (`feature_required`).
    /// The feature, as a design file or `timeline.inspect` has it.
    #[serde(default, deserialize_with = "recorded", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "RecordedFeature")]
    pub feature: Option<RecordedFeature>,
    /// The model parameters its sketch dimensions use, each {name, expression, unit?, comment?}.
    /// Default: none.
    #[serde(default)]
    #[schemars(with = "StepParams")]
    pub params: Option<StepParams>,
}

/// What `timeline.add` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AddResult {
    /// The new feature, by id.
    pub feature: u64,
    /// Its name.
    pub name: String,
    /// Why it did not build, or null.
    pub error: Option<String>,
    /// A warning from building it, or null.
    pub warning: Option<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

// ---- timeline.export_script ----

fn script_path<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s)),
        _ => Err(D::Error::custom(whole("`path` must be a file path"))),
    }
}

/// Write the design's history as a command script that rebuilds it (`solidcraft-cli eval` and
/// `run` replay it): one step per setting and feature, and a list of what the script leaves out.
/// A design with components or joints is refused for now. The design is not changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ExportScriptParams {
    /// A file to write the script to, as JSON. Default: none, the script is only returned.
    #[serde(default, deserialize_with = "script_path", skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// One step of a command script.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ScriptStep {
    /// The command the step runs.
    pub command: String,
    /// Its parameters.
    pub params: Value,
}

/// Where a script came from and how to replay it. Fields stay in alphabetical order: a script file
/// is written from this type, and its keys have always been sorted.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ScriptAbout {
    /// The design's name.
    pub design: String,
    /// The design file format the `timeline.add` steps hold features in.
    pub format: String,
    /// How to replay the script.
    pub replay: String,
    /// The version of SolidCraft that wrote it.
    pub version: String,
}

/// A command script. Fields stay in alphabetical order (see [`ScriptAbout`]).
#[derive(Debug, Serialize, JsonSchema)]
pub struct Script {
    pub about: ScriptAbout,
    /// The steps that rebuild the design on an empty one.
    #[schemars(with = "Vec<ScriptStep>")]
    pub commands: Vec<Value>,
}

/// What `timeline.export_script` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ExportScriptResult {
    /// What the script leaves out (appearances, folders, views, configurations with face-appearance columns, print-face
    /// assignments…).
    pub omitted: Vec<String>,
    /// The file written, or null.
    pub path: Option<String>,
    /// The script.
    pub script: Script,
    /// How many steps it has.
    pub steps: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "timeline_clipboard_tests.rs"]
mod tests;
