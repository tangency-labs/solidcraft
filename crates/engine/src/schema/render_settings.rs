//! Contracts and reply types for Render Settings and Save Render Settings in `cmd/render.rs`
//! (#12783), and the render setup fields a `set` patch may change, which Save Render Settings
//! with HDR (`schema/render_hdr.rs`) shares.
//!
//! The handlers read their parameters with serde, so each key here reads as the handler reads
//! it, with serde's message (`invalid type: …`) or the setup name rules'. Whether a setup exists,
//! and whether the patched setup is valid, stay the handlers' to check.

use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use solidcraft_doc::render_settings::RenderSettings;

use super::check::whole;
use super::render_camera::{setup_name, setup_name_schema};

/// The fields of a render setup (`solidcraft_doc::render_settings::RenderSetup`), as the render
/// settings commands give and take them. `cmd/render.rs`'s `patched_settings` takes a patch of at
/// most this many fields, so the published schemas and the handlers share one list and one bound;
/// a test checks the list against `RenderSetup`'s own fields.
pub(crate) const SETUP_FIELDS: &[&str] = &[
    "backend",
    "mode",
    "camera",
    "width",
    "height",
    "format",
    "exposure_ev",
    "environment_rotation",
    "environment_intensity",
    "background",
    "job",
    "surfaces",
    "optics",
    "coats",
    "stage",
];

/// `patched_settings`' message for a `set` that isn't an object of at most [`SETUP_FIELDS`] keys.
pub(crate) fn set_message() -> String {
    format!("`set` must be an object with at most {} fields", SETUP_FIELDS.len())
}

/// The schema of a `set` patch: an object of setup fields, each without its own schema
/// (`patched_settings` reads the patched setup as a `RenderSetup`, which checks the values and
/// refuses any other key).
pub(super) fn setup_patch_schema(description: &str) -> Schema {
    let properties: Map<String, Value> = SETUP_FIELDS.iter().map(|field| (field.to_string(), Value::Object(Map::new()))).collect();
    json_schema!({
        "description": description,
        "type": "object",
        "properties": properties,
        "additionalProperties": false,
        "maxProperties": SETUP_FIELDS.len()
    })
}

/// Give every saved render setup and the defaults a new one starts from, or one setup by name.
/// Nothing is changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct GetParams {
    /// The saved setup, by name: trimmed, at most 128 bytes, with no control characters. Left out
    /// or `null`: every setup and the defaults.
    #[serde(default, deserialize_with = "setup_name")]
    #[schemars(schema_with = "optional_setup_name_schema")]
    pub name: Option<String>,
}

fn optional_setup_name_schema(generator: &mut SchemaGenerator) -> Schema {
    json_schema!({"anyOf": [setup_name_schema(generator), {"type": "null"}]})
}

/// Change a render setup's fields, or make a new setup from the defaults with them; the fields
/// left out keep their values. One undo step, none when nothing changes.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SetParams {
    // Both keys are required, so the check refuses a call without one ("`name` is required")
    // before the handler runs, as for the other required keys. The handler's own words for it are
    // serde's ("missing field `name`").
    /// The setup, by name: trimmed, at most 128 bytes, with no control characters.
    #[serde(deserialize_with = "saved_name")]
    #[schemars(schema_with = "setup_name_schema")]
    pub name: String,
    #[serde(deserialize_with = "setup_patch")]
    #[schemars(schema_with = "set_schema")]
    pub set: Value,
}

/// Save Render Settings' `name`: text, as the handler's serde struct reads it (its message for
/// anything else, `null` included), and a valid setup name.
fn saved_name<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let name = String::deserialize(Value::deserialize(d)?).map_err(|e| D::Error::custom(whole(e)))?;
    RenderSettings::validate_name(&name).map_err(|e| D::Error::custom(whole(e)))?;
    Ok(name)
}

/// `set`: an object of at most [`SETUP_FIELDS`] keys, with `patched_settings`' message for
/// anything else, `null` included. Which keys and values a setup takes stays the handler's to
/// check, with serde's messages.
fn setup_patch<'de, D: Deserializer<'de>>(d: D) -> Result<Value, D::Error> {
    match Value::deserialize(d)? {
        Value::Object(patch) if patch.len() <= SETUP_FIELDS.len() => Ok(Value::Object(patch)),
        _ => Err(D::Error::custom(whole(set_message()))),
    }
}

fn set_schema(_: &mut SchemaGenerator) -> Schema {
    setup_patch_schema(&format!(
        "The fields to change: at most {} of the setup's fields, as `render.settings.get` gives them. `job`: `null` clears it, an object replaces it whole.",
        SETUP_FIELDS.len()
    ))
}

/// A render setup's fields, as `RenderSetup` saves them.
pub type SetupFields = Map<String, Value>;

/// The saved setups, or one of them.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum RenderSettingsInfo {
    /// Without `name`: every saved setup, by name, and the defaults a new setup starts from.
    All { setups: BTreeMap<String, SetupFields>, defaults: SetupFields },
    /// With `name`: that setup.
    One { name: String, setup: SetupFields },
}

/// What `render.settings.get` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RenderSettingsGot {
    #[serde(flatten)]
    pub info: RenderSettingsInfo,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `render.settings.set` returns: the setup as saved.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RenderSettingsSaved {
    pub name: String,
    pub setup: SetupFields,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "render_settings_tests.rs"]
mod tests;
