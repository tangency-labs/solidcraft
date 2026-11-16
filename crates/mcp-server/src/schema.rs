//! Tool definitions (MCP `Tool`: name, title, description, input schema, annotations) and the
//! check of a call's arguments against the definition it was listed with.

use serde_json::{Value, json};

/// What a tool may do, for the client's approval prompts (MCP tool annotations). Clients that
/// auto-approve on these hints must not be told less than the truth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Only looks.
    ReadOnly,
    /// Changes the app's document in memory, undoably.
    Edits,
    /// Replaces the app's document in memory (unsaved changes are lost).
    Replaces,
    /// May read, write, overwrite or delete files on disk.
    Files,
}

/// A tool's definition, its annotations from `effect`.
pub fn tool(name: &str, title: &str, desc: &str, schema: Value, effect: Effect) -> Value {
    let (read_only, destructive, open_world) = match effect {
        Effect::ReadOnly => (true, false, false),
        Effect::Edits => (false, false, false),
        Effect::Replaces => (false, true, false),
        Effect::Files => (false, true, true),
    };
    json!({
        "name": name,
        "title": title,
        "description": desc,
        "inputSchema": schema,
        "annotations": {"title": title, "readOnlyHint": read_only, "destructiveHint": destructive, "idempotentHint": read_only, "openWorldHint": open_world},
    })
}

/// An object schema with these `props`, the `required` ones, and no other keys.
pub fn object(props: Value, required: &[&str]) -> Value {
    let mut o = json!({"type": "object", "properties": props, "additionalProperties": false});
    if !required.is_empty() {
        o["required"] = json!(required);
    }
    o
}

/// A string property described by `desc`.
pub fn string(desc: &str) -> Value {
    json!({"type": "string", "description": desc})
}

fn type_matches(t: &str, v: &Value) -> bool {
    match t {
        "string" => v.is_string(),
        "number" => v.is_number(),
        "integer" => v.is_i64() || v.is_u64(),
        "boolean" => v.is_boolean(),
        "array" => v.is_array(),
        "object" => v.is_object(),
        "null" => v.is_null(),
        _ => true,
    }
}

/// Check `args` against tool `name`'s input schema as `defs` list it (top level): unknown tool,
/// missing required arguments, unknown arguments, type and enum mismatches.
pub fn check_arguments(defs: &[Value], name: &str, args: &Value) -> Result<(), String> {
    let def = defs.iter().find(|d| d["name"] == name).ok_or_else(|| format!("unknown tool `{name}`"))?;
    let schema = &def["inputSchema"];
    let Some(a) = args.as_object() else { return Err(format!("{name}: arguments must be an object")) };
    for r in schema["required"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        if a.get(r).is_none_or(Value::is_null) {
            return Err(format!("{name}: missing required argument `{r}`"));
        }
    }
    let props = schema["properties"].as_object();
    for (k, v) in a {
        let Some(p) = props.and_then(|p| p.get(k)) else {
            let valid: Vec<&str> = props.map(|p| p.keys().map(String::as_str).collect()).unwrap_or_default();
            let valid = if valid.is_empty() { "none".to_string() } else { valid.join(", ") };
            let k: String = k.chars().take(64).collect();
            return Err(format!("{name}: unknown argument `{k}`; its arguments are: {valid}"));
        };
        if v.is_null() {
            continue;
        }
        // An argument of several types lists them as `anyOf` (Gemini refuses a list of types).
        let types: Vec<&Value> = match p["anyOf"].as_array() {
            Some(alts) => alts.iter().map(|alt| &alt["type"]).collect(),
            None => vec![&p["type"]],
        };
        let ok = types.iter().any(|t| match t {
            Value::String(t) => type_matches(t, v),
            Value::Array(ts) => ts.iter().filter_map(Value::as_str).any(|t| type_matches(t, v)),
            _ => true,
        });
        if !ok {
            let expected = if types.len() == 1 { types[0].clone() } else { Value::Array(types.into_iter().cloned().collect()) };
            return Err(format!("{name}: argument `{k}` must be of type {expected}"));
        }
        if let Some(allowed) = p["enum"].as_array()
            && !allowed.contains(v)
        {
            return Err(format!("{name}: `{k}` must be one of {}", Value::Array(allowed.clone())));
        }
    }
    Ok(())
}

/// Check `args` against prompt `name` as `defs` list it (MCP `Prompt`): unknown prompt, missing
/// required arguments, unknown arguments, values that are not strings.
pub(crate) fn check_prompt_arguments(defs: &[Value], name: &str, args: &Value) -> Result<(), String> {
    let def = defs.iter().find(|d| d["name"] == name).ok_or_else(|| format!("unknown prompt `{name}`"))?;
    let declared = def["arguments"].as_array().map(Vec::as_slice).unwrap_or_default();
    let Some(a) = args.as_object() else { return Err(format!("{name}: arguments must be an object")) };
    for r in declared.iter().filter(|d| d["required"] == true).filter_map(|d| d["name"].as_str()) {
        if !a.contains_key(r) {
            return Err(format!("{name}: missing required argument `{r}`"));
        }
    }
    for (k, v) in a {
        if !declared.iter().any(|d| d["name"] == k.as_str()) {
            let valid: Vec<&str> = declared.iter().filter_map(|d| d["name"].as_str()).collect();
            let valid = if valid.is_empty() { "none".to_string() } else { valid.join(", ") };
            let k: String = k.chars().take(64).collect();
            return Err(format!("{name}: unknown argument `{k}`; its arguments are: {valid}"));
        }
        if !v.is_string() {
            return Err(format!("{name}: argument `{k}` must be a string"));
        }
    }
    Ok(())
}
