//! Plugin arguments checked against an entry's declared input schema before
//! plugin code runs: declared types, required fields, fixed values and ranges. Object schemas
//! default to accepting only declared properties unless they use unsupported schema composition;
//! `oneOf`/`anyOf` options do not implicitly close the object they constrain. The schema is the
//! plugin's, so the check is bounded in depth and work and ignores unsupported keywords.

use crate::{HostError, Result};
use serde_json::{Map, Value};

/// Dialog hint for choosing an installed font family; programmatic values remain strings.
pub const FONT_KEY: &str = "x-solidcraft-font";

/// Validate the optional font hint without looking at installed fonts or limiting values.
pub fn font_field(schema: &Value) -> Result<bool> {
    match schema.get(FONT_KEY) {
        None => Ok(false),
        Some(Value::Bool(true)) if schema.get("type").and_then(Value::as_str) == Some("string") => Ok(true),
        Some(_) => Err(bad(FONT_KEY, "must be true on a string property")),
    }
}

/// How deep a schema and value are followed.
const MAX_DEPTH: usize = 32;
/// The most schema nodes one check visits (`oneOf` inside `oneOf` multiplies).
const MAX_STEPS: usize = 100_000;

/// `Ok` when `value` fits `schema`; otherwise the first field that does not, and why.
pub fn check_input(schema: &Value, value: &Value) -> Result<()> {
    crate::conditions::validate_conditions(schema)?;
    let mut steps = 0;
    check(schema, value, "params", 0, &mut steps, true, true)
}

/// Validate and omit hidden fields before passing input to plugin code.
pub fn prepare_input(schema: &Value, value: &Value) -> Result<Value> {
    check_input(schema, value)?;
    // Select against the original input, before any scope removes keys. Load validation proves
    // conditional branches unambiguous; ordinary scalar choice filters remain field rules.
    fn scopes<'a>(schema: &'a Value, value: &Value, depth: usize, steps: &mut usize, out: &mut Vec<&'a Value>) -> Result<()> {
        *steps += 1;
        if depth > MAX_DEPTH || *steps > MAX_STEPS {
            return Err(bad("params", "is nested too deeply to check"));
        }
        out.push(schema);
        for key in ["oneOf", "anyOf"] {
            if let Some(options) = schema.get(key).and_then(Value::as_array) {
                for option in options.iter().take(256) {
                    if check(option, value, "params", depth + 1, steps, false, true).is_ok() {
                        scopes(option, value, depth + 1, steps, out)?;
                        break;
                    }
                    if *steps > MAX_STEPS {
                        return Err(bad("params", "is nested too deeply to check"));
                    }
                }
            }
        }
        Ok(())
    }
    fn visible(schemas: &[&Value], value: &Value, depth: usize, steps: &mut usize) -> Result<Value> {
        *steps += 1;
        if depth > MAX_DEPTH || *steps > MAX_STEPS {
            return Err(bad("params", "is nested too deeply to check"));
        }
        let mut applicable = Vec::new();
        for schema in schemas {
            scopes(schema, value, depth, steps, &mut applicable)?;
        }
        if let Some(object) = value.as_object() {
            let conditions = applicable.iter().map(|s| crate::conditions::InputConditions::parse(s)).collect::<Result<Vec<_>>>()?;
            let controls: Vec<_> = conditions.iter().map(|c| c.visible_values(object)).collect();
            let mut out = Map::new();
            for (key, value) in object {
                if conditions.iter().zip(&controls).any(|(c, values)| c.fields.get(key).is_some_and(|f| !f.visible(values))) {
                    continue;
                }
                let properties: Vec<_> = applicable
                    .iter()
                    .filter_map(|s| s.get("properties").and_then(|p| p.get(key)).or_else(|| s.get("additionalProperties")))
                    .collect();
                out.insert(key.clone(), visible(&properties, value, depth + 1, steps)?);
            }
            return Ok(Value::Object(out));
        }
        if let Some(values) = value.as_array() {
            let items: Vec<_> = applicable.iter().filter_map(|s| s.get("items")).collect();
            return values.iter().map(|v| visible(&items, v, depth + 1, steps)).collect::<Result<Vec<_>>>().map(Value::Array);
        }
        Ok(value.clone())
    }
    visible(&[schema], value, 0, &mut 0)
}

// The base schema constraints, without conditions (used to validate condition literals).
pub(crate) fn check_value(schema: &Value, value: &Value) -> Result<()> {
    check(schema, value, "value", 0, &mut 0, true, false)
}

/// The schema as a tool lists it, with the same default-closed object rules as [`check_input`].
pub fn strict_input_schema(schema: &Value) -> Value {
    fn strict(schema: &mut Value, depth: usize, closed: bool) {
        if depth > MAX_DEPTH {
            return;
        }
        let Some(o) = schema.as_object_mut() else { return };
        let object = o.get("type").and_then(Value::as_str) == Some("object") || o.get("properties").is_some_and(Value::is_object);
        if object && defaults_closed(o, closed) && !o.contains_key("additionalProperties") {
            o.insert("additionalProperties".into(), Value::Bool(false));
        }
        if let Some(Value::Object(properties)) = o.get_mut("properties") {
            for property in properties.values_mut() {
                strict(property, depth + 1, true);
            }
        }
        if let Some(extra @ Value::Object(_)) = o.get_mut("additionalProperties") {
            strict(extra, depth + 1, true);
        }
        if let Some(items) = o.get_mut("items") {
            strict(items, depth + 1, true);
        }
        for key in ["oneOf", "anyOf"] {
            if let Some(Value::Array(options)) = o.get_mut(key) {
                for option in options.iter_mut().take(256) {
                    strict(option, depth + 1, false);
                }
            }
        }
    }
    let mut schema = schema.clone();
    strict(&mut schema, 0, true);
    schema
}

// These keywords can describe fields outside `properties`, which this checker cannot resolve.
// Branches also constrain an existing object rather than declaring all its fields.
fn defaults_closed(schema: &Map<String, Value>, closed: bool) -> bool {
    const UNSUPPORTED: &[&str] = &[
        "allOf",
        "$ref",
        "$dynamicRef",
        "patternProperties",
        "dependentSchemas",
        "dependencies",
        "if",
        "then",
        "else",
        "unevaluatedProperties",
        "propertyNames",
    ];
    closed && !UNSUPPORTED.iter().any(|key| schema.contains_key(*key))
}

fn bad(field: &str, reason: impl Into<String>) -> HostError {
    HostError::InvalidInput { field: field.into(), reason: reason.into() }
}

fn type_matches(kind: &str, value: &Value) -> bool {
    match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        "number" => value.is_number(),
        "integer" => value.is_i64() || value.is_u64() || value.as_f64().is_some_and(|f| f.is_finite() && f.fract() == 0.0),
        // A type this check does not know refuses nothing.
        _ => true,
    }
}

fn shown(value: &Value) -> String {
    let text = value.to_string();
    if text.chars().count() > 80 { format!("{}…", text.chars().take(80).collect::<String>()) } else { text }
}

fn number(schema: &Map<String, Value>, key: &str) -> Option<f64> {
    schema.get(key).and_then(Value::as_f64)
}

fn count(schema: &Map<String, Value>, key: &str) -> Option<usize> {
    schema.get(key).and_then(Value::as_u64).map(|n| usize::try_from(n).unwrap_or(usize::MAX))
}

fn check(schema: &Value, value: &Value, field: &str, depth: usize, steps: &mut usize, closed: bool, conditional: bool) -> Result<()> {
    *steps += 1;
    if depth > MAX_DEPTH || *steps > MAX_STEPS {
        return Err(bad(field, "is nested too deeply to check"));
    }
    // `true`, `{}` or anything that is not a schema object: any value.
    let Some(schema) = schema.as_object() else {
        return if schema == &Value::Bool(false) { Err(bad(field, "is not allowed")) } else { Ok(()) };
    };
    match schema.get("type") {
        Some(Value::String(kind)) if !type_matches(kind, value) => return Err(bad(field, format!("must be of type {kind}"))),
        Some(Value::Array(kinds)) if !kinds.is_empty() && !kinds.iter().filter_map(Value::as_str).any(|kind| type_matches(kind, value)) => {
            let names: Vec<&str> = kinds.iter().filter_map(Value::as_str).collect();
            return Err(bad(field, format!("must be of type {}", names.join(" or "))));
        }
        _ => {}
    }
    if let Some(constant) = schema.get("const")
        && !crate::conditions::scalar_equal(constant, value)
    {
        return Err(bad(field, format!("must be {}", shown(constant))));
    }
    if let Some(Value::Array(options)) = schema.get("enum")
        && !options.iter().any(|option| crate::conditions::scalar_equal(option, value))
    {
        let names: Vec<String> = options.iter().take(12).map(shown).collect();
        return Err(bad(field, format!("must be one of {}", names.join(", "))));
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(Value::Array(options)) = schema.get(key)
            && !options.is_empty()
        {
            let mut matched = false;
            for option in options.iter().take(256) {
                if check(option, value, field, depth + 1, steps, false, conditional).is_ok() {
                    matched = true;
                    break;
                }
            }
            if !matched {
                let constants: Vec<String> = options.iter().filter_map(|o| o.get("const")).take(12).map(shown).collect();
                let reason = if constants.is_empty() {
                    "matches none of the declared options".to_string()
                } else {
                    format!("must be one of {}", constants.join(", "))
                };
                return Err(bad(field, reason));
            }
        }
    }
    if let Some(n) = value.as_f64() {
        if let Some(min) = number(schema, "minimum")
            && n < min
        {
            return Err(bad(field, format!("must be at least {min}")));
        }
        if let Some(max) = number(schema, "maximum")
            && n > max
        {
            return Err(bad(field, format!("must be at most {max}")));
        }
        if let Some(min) = number(schema, "exclusiveMinimum")
            && n <= min
        {
            return Err(bad(field, format!("must be more than {min}")));
        }
        if let Some(max) = number(schema, "exclusiveMaximum")
            && n >= max
        {
            return Err(bad(field, format!("must be less than {max}")));
        }
    }
    if let Some(text) = value.as_str() {
        let length = text.chars().count();
        if count(schema, "minLength").is_some_and(|min| length < min) {
            return Err(bad(field, "is too short"));
        }
        if count(schema, "maxLength").is_some_and(|max| length > max) {
            return Err(bad(field, "is too long"));
        }
    }
    if let Some(items) = value.as_array() {
        if count(schema, "minItems").is_some_and(|min| items.len() < min) {
            return Err(bad(field, "has too few items"));
        }
        if count(schema, "maxItems").is_some_and(|max| items.len() > max) {
            return Err(bad(field, "has too many items"));
        }
        if let Some(item) = schema.get("items") {
            for (i, value) in items.iter().enumerate() {
                check(item, value, &format!("{field}[{i}]"), depth + 1, steps, true, conditional)?;
            }
        }
    }
    if let Some(object) = value.as_object() {
        let properties = schema.get("properties").and_then(Value::as_object);
        let conditions = if conditional { crate::conditions::InputConditions::from_properties(properties)? } else { Default::default() };
        let controls = conditions.visible_values(object);
        if let Some(Value::Array(required)) = schema.get("required") {
            for name in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(name) && conditions.fields.get(name).is_none_or(|f| f.visible(&controls)) {
                    return Err(bad(&format!("{field}.{name}"), "is required"));
                }
            }
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        let additional = schema.get("additionalProperties");
        // An object schema that says nothing about other fields declares exactly its properties.
        let declared = defaults_closed(schema, closed) && (properties.is_some() || schema.get("type").and_then(Value::as_str) == Some("object"));
        for (name, value) in object {
            if conditions.fields.get(name).is_some_and(|f| !f.visible(&controls)) {
                continue;
            }
            let field = format!("{field}.{name}");
            match (properties.and_then(|p| p.get(name)), additional) {
                (Some(property), _) => {
                    if let (Some(rules), Some(options)) = (conditions.fields.get(name), property.get("oneOf").and_then(Value::as_array))
                        && rules.has_choice_filter()
                        && options.iter().take(256).enumerate().all(|(i, option)| {
                            !rules.option_available(i, &controls) || check(option, value, &field, depth + 1, steps, false, conditional).is_err()
                        })
                    {
                        return Err(bad(&field, "is not an available choice"));
                    }
                    check(property, value, &field, depth + 1, steps, true, conditional)?;
                }
                (None, Some(Value::Bool(false))) => return Err(bad(&field, "is not an input of this entry")),
                // Declared properties and nothing said about others: as strict as an app command.
                (None, None) if declared => return Err(bad(&field, "is not an input of this entry")),
                (None, Some(extra @ Value::Object(_))) => check(extra, value, &field, depth + 1, steps, true, conditional)?,
                (None, _) => {}
            }
        }
    }
    Ok(())
}
