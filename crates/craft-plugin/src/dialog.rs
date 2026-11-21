//! File-aware dialog data. Facts are supplied by the host, never by option values.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{Result, invalid};

pub const MAX_PREFIX_BYTES: u64 = 64 * 1024;
pub const MAX_UPDATE_BYTES: usize = 64 * 1024;
pub const DEADLINE_MS: u64 = 500;
pub const MAX_FUEL: u64 = 5_000_000;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileFacts {
    pub name: String,
    pub extension: String,
    pub size: u64,
    /// The namespaced importer entry matched by the host.
    pub format: String,
    pub generation: u64,
}
impl FileFacts {
    pub fn values(&self) -> Map<String, Value> {
        let mut facts = Map::new();
        facts.insert("name".into(), json!(self.name));
        facts.insert("extension".into(), json!(self.extension));
        facts.insert("size".into(), json!(self.size));
        facts.insert("format".into(), json!(self.format));
        facts
    }
}

/// Apply a bounded hook reply to declared fields only. A hook cannot add an input, change its
/// type, make a directory into a grant, or replace declarative visibility/choice rules.
pub fn apply_update(schema: &Value, update: &Value) -> Result<Value> {
    if update.to_string().len() > MAX_UPDATE_BYTES {
        return Err(invalid("dialog", "field update exceeds 64 KiB"));
    }
    let reply = update.as_object().filter(|o| o.len() == 1).ok_or_else(|| invalid("dialog", "reply needs fields"))?;
    let fields = reply
        .get("fields")
        .and_then(Value::as_object)
        .filter(|f| f.len() <= 256)
        .ok_or_else(|| invalid("dialog.fields", "requires at most 256 field updates"))?;
    let mut out = schema.clone();
    let properties = out.get_mut("properties").and_then(Value::as_object_mut).ok_or_else(|| invalid("dialog.fields", "no declared fields"))?;
    for (name, patch) in fields {
        let property = properties.get_mut(name).ok_or_else(|| invalid("dialog.fields", "unknown field"))?;
        let original = property.clone();
        let patch = patch.as_object().ok_or_else(|| invalid("dialog.fields", "an update must be an object"))?;
        if patch.keys().any(|k| !matches!(k.as_str(), "title" | "description" | "default" | "enum" | "oneOf"))
            || (patch.contains_key("enum") && patch.contains_key("oneOf"))
        {
            return Err(invalid("dialog.fields", "only title, description, default and choices may change"));
        }
        let target = property.as_object_mut().ok_or_else(|| invalid("dialog.fields", "not a property"))?;
        for key in ["title", "description"] {
            if let Some(value) = patch.get(key) {
                if !value.as_str().is_some_and(|s| s.len() <= 2048 && !s.chars().any(|c| c.is_control() && c != '\n')) {
                    return Err(invalid("dialog.fields", "text must be at most 2048 bytes"));
                }
                target.insert(key.into(), value.clone());
            }
        }
        for key in ["enum", "oneOf"] {
            if let Some(choices) = patch.get(key) {
                if original.get("type").and_then(Value::as_str) != Some("string")
                    || original.get("format").and_then(Value::as_str) == Some("directory")
                    || original
                        .get("oneOf")
                        .and_then(Value::as_array)
                        .is_some_and(|options| options.iter().any(|o| o.get(crate::conditions::OPTION_WHEN).is_some()))
                {
                    return Err(invalid("dialog.fields", "only unconditional string choices may be updated"));
                }
                let choices = choices
                    .as_array()
                    .filter(|c| !c.is_empty() && c.len() <= 256)
                    .ok_or_else(|| invalid("dialog.fields", "requires 1..256 choices"))?;
                for choice in choices {
                    let value = if key == "oneOf" {
                        let option = choice
                            .as_object()
                            .filter(|o| o.keys().all(|k| matches!(k.as_str(), "const" | "title" | "description")))
                            .ok_or_else(|| invalid("dialog.fields", "a choice needs const and optional title/description"))?;
                        for label in ["title", "description"] {
                            if option.get(label).is_some_and(|v| !v.as_str().is_some_and(|s| s.len() <= 2048)) {
                                return Err(invalid("dialog.fields", "invalid choice text"));
                            }
                        }
                        option.get("const").ok_or_else(|| invalid("dialog.fields", "a choice needs const"))?
                    } else {
                        choice
                    };
                    crate::input::check_value(&original, value).map_err(|_| invalid("dialog.fields", "choice does not fit the declared field"))?;
                }
                target.remove("enum");
                target.remove("oneOf");
                target.insert(key.into(), Value::Array(choices.clone()));
            }
        }
        if let Some(default) = patch.get("default") {
            crate::input::check_value(&Value::Object(target.clone()), default)
                .map_err(|_| invalid("dialog.fields", "default does not fit the updated field"))?;
            target.insert("default".into(), default.clone());
        }
    }
    crate::conditions::validate_conditions(&out)?;
    Ok(out)
}
