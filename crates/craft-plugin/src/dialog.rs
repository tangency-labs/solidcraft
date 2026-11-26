//! Read-only field patches and file-aware importer updates, validated atomically.
use crate::{HostError, Result, conditions::InputConditions, invalid};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// A validated patch contains only known presentation keys and schema-compatible values.
pub type FieldPatch = Map<String, Value>;

#[derive(Clone, Debug, Default)]
pub struct Update {
    pub fields: BTreeMap<String, FieldPatch>,
    pub values: Value,
    /// A malformed reply is discarded whole. The caller shows/logs this quiet warning.
    pub warning: Option<String>,
}

fn bad(field: &str, reason: &str) -> HostError {
    HostError::InvalidInput { field: format!("params.{field}"), reason: reason.into() }
}

/// Declarative defaults and visibility first. A caller-owned key is never overwritten.
pub fn declarative(schema: &Value, values: &Value, protected: &BTreeSet<String>) -> Result<Value> {
    let mut values = values.as_object().cloned().ok_or_else(|| bad("", "expected an object"))?;
    let rules = InputConditions::parse(schema)?;
    for key in &rules.order {
        let Some(rule) = rules.fields.get(key) else { continue };
        let controls = rules.visible_values(&values)?;
        if !rule.visible(&controls) {
            values.remove(key);
        } else if !protected.contains(key)
            && let Some(value) = rule
                .default_value(&controls)
                .or_else(|| if values.contains_key(key) { None } else { schema.get("properties")?.get(key)?.get("default") })
        {
            values.insert(key.clone(), value.clone());
        }
    }
    // Unannotated schemas have no conditional order. Their ordinary defaults still
    // belong to the shared input preparation, without replacing caller-owned values.
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (key, property) in properties {
            if !rules.fields.contains_key(key)
                && !protected.contains(key)
                && !values.contains_key(key)
                && let Some(default) = property.get("default")
            {
                values.insert(key.clone(), default.clone());
            }
        }
    }
    Ok(Value::Object(values))
}

/// Apply one reply, or discard it whole with a warning. `complete` runs the full input check;
/// forms can validate partially filled input while retaining the same field constraints.
pub fn apply(schema: &Value, values: &Value, protected: &BTreeSet<String>, reply: Option<&str>, complete: bool) -> Result<Update> {
    if serde_json::to_vec(values).map_err(|_| bad("", "invalid JSON"))?.len() > crate::MAX_JSON_BYTES {
        return Err(bad("", "input exceeds 1 MiB"));
    }
    let baseline = declarative(schema, values, protected)?;
    let mut update = Update { values: baseline.clone(), ..Default::default() };
    if let Some(reply) = reply {
        match patch(schema, &baseline, protected, reply) {
            Ok((fields, values)) => {
                update.fields = fields;
                update.values = values;
            }
            Err(error) => update.warning = Some(format!("Dialog hook reply ignored: {error}")),
        }
    }
    let mut partial = schema.clone();
    if !complete && let Some(object) = partial.as_object_mut() {
        object.remove("required");
    }
    crate::check_input(&partial, &update.values)?;
    if complete {
        for (key, field) in &update.fields {
            if let (Some(choices), Some(value)) = (field.get("choices").and_then(Value::as_array), update.values.get(key))
                && !choices.iter().any(|choice| crate::conditions::scalar_equal(choice, value))
            {
                return Err(bad(key, &format!("`{key}` is outside the dialog hook's available choices")));
            }
        }
    }
    Ok(update)
}

fn patch(schema: &Value, baseline: &Value, protected: &BTreeSet<String>, reply: &str) -> Result<(BTreeMap<String, FieldPatch>, Value)> {
    if reply.len() > crate::MAX_JSON_BYTES {
        return Err(bad("", "hook reply exceeds 1 MiB"));
    }
    let reply: Value = serde_json::from_str(reply).map_err(|_| bad("", "hook reply must be JSON"))?;
    let reply = reply.as_object().ok_or_else(|| bad("", "hook reply must be an object"))?;
    let properties = schema.get("properties").and_then(Value::as_object).ok_or_else(|| bad("", "no schema fields"))?;
    if reply.len() > 1024 {
        return Err(bad("", "too many hook fields"));
    }
    let mut values = baseline.clone();
    let mut fields = BTreeMap::new();
    // Check all keys and literal values before mutating even the private candidate.
    for (key, value) in reply {
        let property = properties.get(key).ok_or_else(|| bad(key, "hook cannot add fields"))?;
        let field = value.as_object().ok_or_else(|| bad(key, "field patch must be an object"))?;
        for (name, value) in field {
            match name.as_str() {
                "visible" | "enabled" if value.is_boolean() => {}
                "hint" if value.as_str().is_some_and(|s| s.len() <= 1024 && !s.chars().any(char::is_control)) => {}
                "choices" if value.is_array() => {}
                "default" | "value" => crate::input::check_value(property, value).map_err(|_| bad(key, "hook value does not fit the schema"))?,
                _ => return Err(bad(key, "invalid hook patch key or type")),
            }
        }
        if !protected.contains(key)
            && let Some(value) = field.get("value").or_else(|| field.get("default"))
            && let Some(object) = values.as_object_mut()
        {
            object.insert(key.clone(), value.clone());
        }
        fields.insert(key.clone(), field.clone());
    }
    // A computed controller can expose another field or change its conditional default. Keep
    // explicit hook assignments and caller edits, then resolve the remaining defaults together.
    let assigned = protected
        .iter()
        .cloned()
        .chain(fields.iter().filter(|(_, field)| field.contains_key("value") || field.contains_key("default")).map(|(key, _)| key.clone()))
        .collect();
    values = declarative(schema, &values, &assigned)?;
    // Re-evaluate declarative constraints on the proposed values; the hook cannot relax them.
    let rules = InputConditions::parse(schema)?;
    let object = values.as_object().ok_or_else(|| bad("", "expected an object"))?;
    let controls = rules.visible_values(object)?;
    for (key, field) in &fields {
        let rule = rules.fields.get(key).cloned().unwrap_or_default();
        if field.get("visible") == Some(&Value::Bool(true)) && !rule.visible(&controls)
            || field.get("enabled") == Some(&Value::Bool(true)) && !rule.enabled(&controls)
        {
            return Err(bad(key, "hook cannot relax declarative conditions"));
        }
        if let Some(choices) = field.get("choices").and_then(Value::as_array) {
            let options = properties
                .get(key)
                .and_then(|p| p.get("oneOf"))
                .and_then(Value::as_array)
                .ok_or_else(|| bad(key, "hook choices need schema oneOf consts"))?;
            let mut next = 0;
            for choice in choices {
                let Some(index) = options
                    .iter()
                    .enumerate()
                    .skip(next)
                    .find_map(|(i, option)| option.get("const").filter(|c| crate::conditions::scalar_equal(c, choice)).map(|_| i))
                else {
                    return Err(bad(key, "hook choices must be an ordered subset of schema consts"));
                };
                if !rule.option_available(index, &controls) {
                    return Err(bad(key, "hook choice violates a declarative condition"));
                }
                next = index + 1;
            }
        }
        for value in [field.get("default"), field.get("value")].into_iter().flatten() {
            if let Some(options) = properties.get(key).and_then(|p| p.get("oneOf")).and_then(Value::as_array)
                && let Some(index) = options.iter().position(|option| option.get("const").is_some_and(|c| crate::conditions::scalar_equal(c, value)))
                && !rule.option_available(index, &controls)
            {
                return Err(bad(key, "hook value violates a declarative condition"));
            }
            if field.get("choices").and_then(Value::as_array).is_some_and(|c| !c.iter().any(|c| crate::conditions::scalar_equal(c, value))) {
                return Err(bad(key, "hook default/value is outside its choices"));
            }
        }
    }
    // Validate the whole candidate without required fields: an incomplete dialog is legitimate.
    let mut partial = schema.clone();
    if let Some(object) = partial.as_object_mut() {
        object.remove("required");
    }
    crate::check_input(&partial, &values)?;
    Ok((fields, values))
}

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
