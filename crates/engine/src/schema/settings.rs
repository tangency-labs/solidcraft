//! Contracts and reply types for the settings profile commands in `cmd/settings.rs` (#6237).
//!
//! The engine keeps no preferences and no profiles: each handler only checks its parameters and
//! echoes what it read, and the desktop and web apps answer the command themselves from that echo
//! (`settings_profiles::command` in the UI crate). The reply types here are therefore the echo,
//! which is what a call without an app (the CLI, a script, a test) gets; what the app answers
//! instead is what the command's `params` text describes.
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message: [`ProfileName`], [`ProfilePath`] and the
//! either-or of `settings.profile.import` call the handler's own readers and share its wording
//! (`profile_name`, `name_message`, `path_value`, `path_message`, `EITHER_MESSAGE` in
//! `cmd/settings.rs`), so it can't drift. A key the handler refuses when it is left out is
//! optional in the type and required in the schema, so that the handler's message, not the
//! check's "is required", is what a caller sees. `settings.profile.import` reads `name` only when
//! it is there and not `null`, so its `name` is not required and `null` reads as left out.
//!
//! Which name exists, how many profiles there are, what a profile applies and what a profile file
//! holds stay the app's to answer.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};

use super::check::whole;
use super::motion::require;
use crate::cmd::settings::{self, MAX_PATH, MAX_PROFILE_NAME};

/// A settings profile's name, as `cmd/settings.rs` reads it: 1 to [`MAX_PROFILE_NAME`] characters
/// without control characters, with surrounding whitespace trimmed away. The value here is the
/// trimmed name, which is what the handler echoes and what the app keeps the profile under.
#[derive(Debug)]
pub struct ProfileName(pub String);

impl JsonSchema for ProfileName {
    fn schema_name() -> Cow<'static, str> {
        "ProfileName".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": format!(
                "A settings profile's name: 1 to {MAX_PROFILE_NAME} characters, without control characters. \
                 Surrounding whitespace is trimmed away, so the name that is kept can be shorter.",
            ),
            "type": "string",
        })
    }
}

/// A profile file's path, as `cmd/settings.rs` reads it: text that is not blank and at most
/// [`MAX_PATH`] bytes.
#[derive(Debug)]
pub struct ProfilePath(pub String);

impl JsonSchema for ProfilePath {
    fn schema_name() -> Cow<'static, str> {
        "ProfilePath".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": format!("A file path: not blank, at most {MAX_PATH} bytes."),
            "type": "string",
        })
    }
}

/// The name in `v`, as the handler reads it; `None` for anything it refuses.
fn name_of(v: &Value) -> Option<ProfileName> {
    match v {
        Value::String(text) => settings::profile_name(text).map(|name| ProfileName(name.to_string())),
        _ => None,
    }
}

/// `name`, required: the handler's message is what a call that leaves it out, or gives `null` or a
/// name it refuses, hears.
fn name_field<'de, D: Deserializer<'de>>(d: D) -> Result<Option<ProfileName>, D::Error> {
    let v = Value::deserialize(d)?;
    name_of(&v).map(Some).ok_or_else(|| D::Error::custom(whole(settings::name_message("name"))))
}

/// `to` of `settings.profile.rename`, read and refused as `name` is, under the key's own name.
fn to_field<'de, D: Deserializer<'de>>(d: D) -> Result<Option<ProfileName>, D::Error> {
    let v = Value::deserialize(d)?;
    name_of(&v).map(Some).ok_or_else(|| D::Error::custom(whole(settings::name_message("to"))))
}

/// `name` of `settings.profile.import`: the handler reads it only when it is there and not `null`,
/// so `null` is the same as leaving it out and neither is refused.
fn import_name_field<'de, D: Deserializer<'de>>(d: D) -> Result<Option<ProfileName>, D::Error> {
    let v = Value::deserialize(d)?;
    if v.is_null() {
        return Ok(None);
    }
    name_of(&v).map(Some).ok_or_else(|| D::Error::custom(whole(settings::name_message("name"))))
}

/// `path`, optional: `null` is the same as leaving it out, as the handler reads it; anything else
/// it can't read gets the handler's message.
fn path_field<'de, D: Deserializer<'de>>(d: D) -> Result<Option<ProfilePath>, D::Error> {
    let v = Value::deserialize(d)?;
    if v.is_null() {
        return Ok(None);
    }
    match settings::path_value(&v) {
        Some(path) => Ok(Some(ProfilePath(path.to_string()))),
        None => Err(D::Error::custom(whole(settings::path_message()))),
    }
}

/// `profile` of `settings.profile.import`: an object, or `null` to read it as left out. Anything
/// else is what the handler's either-or refuses, so that is the message.
fn profile_field<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    let v = Value::deserialize(d)?;
    match v {
        Value::Null => Ok(None),
        Value::Object(_) => Ok(Some(v)),
        _ => Err(D::Error::custom(whole(settings::EITHER_MESSAGE))),
    }
}

/// A settings profile document, as `settings.profile.export` answers with one. What it holds is the
/// app's to check; the engine only passes it on.
fn profile_document_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "A settings profile document: an object with `format: \"solidcraft-profile/1\"` and the settings it holds.",
        "type": "object",
    })
}

/// The same, where `null` reads as the key being left out (`settings.profile.import`).
fn profile_or_nothing_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The settings profile document to add: an object with `format: \"solidcraft-profile/1\"` and the \
                        settings it holds. `null` is the same as leaving it out.",
        "type": ["object", "null"],
    })
}

/// Requires `name`, which the type leaves optional so that the handler's message for a name that
/// is missing is what a caller sees.
fn name_required(schema: &mut Schema) {
    require(schema, "name");
}

/// Requires `name` and `to`, for `settings.profile.rename`.
fn names_required(schema: &mut Schema) {
    require(schema, "name");
    require(schema, "to");
}

/// Says that one of `path` or `profile` is needed. Which one, and both being one too many, stays
/// the handler's to refuse, in its own words.
fn path_or_profile_required(schema: &mut Schema) {
    schema.insert("anyOf".into(), json!([{"required": ["path"]}, {"required": ["profile"]}]));
}

/// `settings.profile.list`: no parameters. The app answers it; without an app the call does nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ProfileListParams {}

/// What `settings.profile.list` returns: the echo, which is empty. The app answers with
/// `profiles` and `active` instead, as its `params` text says.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileListResult {
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `settings.profile.save`: keep the current input and interface settings under a name, and make
/// that profile the active one.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = name_required)]
pub struct ProfileSaveParams {
    // Optional in the type and required in the schema (`name_required`).
    /// The profile to save under, replacing one of that name.
    #[serde(default, deserialize_with = "name_field")]
    #[schemars(with = "ProfileName")]
    pub name: Option<ProfileName>,
}

/// What `settings.profile.save` returns: the name as it is kept.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileSaveResult {
    /// The profile's name, with surrounding whitespace trimmed away.
    pub name: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `settings.profile.apply`: switch to a profile, changing every setting it holds at once.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = name_required)]
pub struct ProfileApplyParams {
    // Optional in the type and required in the schema (`name_required`).
    /// The profile to apply.
    #[serde(default, deserialize_with = "name_field")]
    #[schemars(with = "ProfileName")]
    pub name: Option<ProfileName>,
}

/// What `settings.profile.apply` returns: the echo. The app answers with `applied` and `skipped`
/// too, as its `params` text says.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileApplyResult {
    /// The profile's name, with surrounding whitespace trimmed away.
    pub name: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `settings.profile.rename`: call a profile something else.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = names_required)]
pub struct ProfileRenameParams {
    // Both are optional in the type and required in the schema (`names_required`), so that the
    // handler's message names the key that is missing.
    /// The profile to rename.
    #[serde(default, deserialize_with = "name_field")]
    #[schemars(with = "ProfileName")]
    pub name: Option<ProfileName>,
    /// The name to call it.
    #[serde(default, deserialize_with = "to_field")]
    #[schemars(with = "ProfileName")]
    pub to: Option<ProfileName>,
}

/// What `settings.profile.rename` returns: the old name and the new one. The app answers with the
/// new name under `name`, as its `params` text says.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileRenameResult {
    /// The profile's name as it was given.
    pub name: String,
    /// The name it has now.
    pub to: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `settings.profile.delete`: remove a profile.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = name_required)]
pub struct ProfileDeleteParams {
    // Optional in the type and required in the schema (`name_required`).
    /// The profile to remove.
    #[serde(default, deserialize_with = "name_field")]
    #[schemars(with = "ProfileName")]
    pub name: Option<ProfileName>,
}

/// What `settings.profile.delete` returns: the echo. The app answers with `deleted` instead, as its
/// `params` text says.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileDeleteResult {
    /// The profile's name, with surrounding whitespace trimmed away.
    pub name: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `settings.profile.export`: a profile's document, written to a file when one is given.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = name_required)]
pub struct ProfileExportParams {
    // Optional in the type and required in the schema (`name_required`).
    /// The profile to export.
    #[serde(default, deserialize_with = "name_field")]
    #[schemars(with = "ProfileName")]
    pub name: Option<ProfileName>,
    /// Where to write it. Left out, nothing is written; the app answers with the document itself.
    #[serde(default, deserialize_with = "path_field")]
    #[schemars(with = "ProfilePath")]
    pub path: Option<ProfilePath>,
}

/// What `settings.profile.export` returns: the name, and the path when one was given. The app
/// answers with the `profile` document too, as its `params` text says.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileExportResult {
    /// The profile's name, with surrounding whitespace trimmed away.
    pub name: String,
    /// The file it was written to, when a path was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `settings.profile.import`: add a profile from a file or a document, without switching to it.
/// One of `path` or `profile` is needed; the handler refuses giving both, in its own words.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = path_or_profile_required)]
pub struct ProfileImportParams {
    /// The file to read the profile from. Give this or `profile`.
    #[serde(default, deserialize_with = "path_field")]
    #[schemars(with = "ProfilePath")]
    pub path: Option<ProfilePath>,
    /// The profile to add, as `settings.profile.export` answers with one. Give this or `path`.
    #[serde(default, deserialize_with = "profile_field")]
    #[schemars(schema_with = "profile_or_nothing_schema")]
    pub profile: Option<Value>,
    /// What to call the new profile. The app makes the name it uses unique.
    /// Default: the name the profile document carries.
    #[serde(default, deserialize_with = "import_name_field")]
    #[schemars(with = "ProfileName")]
    pub name: Option<ProfileName>,
}

/// What `settings.profile.import` returns: the source it read, and the name when one was given.
/// The app answers with the name it added the profile under, as its `params` text says.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileImportResult {
    /// The file the profile was read from, when `path` was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The document that was added, when `profile` was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "profile_document_schema")]
    pub profile: Option<Value>,
    /// The name the profile is added under, when the call gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
