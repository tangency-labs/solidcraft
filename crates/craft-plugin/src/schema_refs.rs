//! Shared field definitions: an entry's `input_schema` may say `{"$ref": "#/$defs/name"}` for a
//! definition in its own `$defs` or in the manifest's top-level `$defs`. They are inlined once when
//! the manifest loads, so the dialog, tool schemas, conditions and input checks only ever see the
//! expanded schema. Keys beside a `$ref` (a `default`, a `description`) override the definition's.

use crate::{Result, invalid};
use serde_json::{Map, Value};

/// How many definitions one `$ref` may pass through (refs inside definitions).
const MAX_DEPTH: usize = 16;
/// The most `$ref`s one manifest may expand, so nested shared definitions can't multiply its size.
const MAX_EXPANSIONS: usize = 10_000;

/// Inline every `$ref` in the entries' input schemas and drop the `$defs` they came from.
/// Returns whether the manifest had any `$defs` or `$ref`.
pub(crate) fn expand(manifest: &mut Value) -> Result<bool> {
    let Some(object) = manifest.as_object_mut() else { return Ok(false) };
    let shared = definitions(object.remove("$defs"), "$defs")?;
    let mut changed = shared.is_some();
    let mut expansions = 0;
    let entries = object.get_mut("runtime").and_then(|r| r.get_mut("entries")).and_then(Value::as_array_mut);
    for entry in entries.into_iter().flatten() {
        let name = entry.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
        let Some(schema) = entry.get_mut("input_schema").filter(|s| s.is_object()) else { continue };
        let field = format!("runtime.entries.{name}.input_schema");
        let local = definitions(schema.as_object_mut().and_then(|s| s.remove("$defs")), &format!("{field}.$defs"))?;
        changed |= local.is_some();
        let scope = Scope { local: local.as_ref(), shared: shared.as_ref(), field: &field };
        // The whole schema, so a `$ref` at its root expands too.
        changed |= scope.resolve(schema, false, &mut Vec::new(), &mut expansions)?;
    }
    Ok(changed)
}

fn definitions(value: Option<Value>, field: &str) -> Result<Option<Map<String, Value>>> {
    match value {
        None => Ok(None),
        Some(Value::Object(map)) => Ok(Some(map)),
        Some(_) => Err(invalid(field, "must be an object of named schemas")),
    }
}

struct Scope<'a> {
    local: Option<&'a Map<String, Value>>,
    shared: Option<&'a Map<String, Value>>,
    field: &'a str,
}

impl Scope<'_> {
    /// Inline the refs in `value`; `trail` names the definitions being expanded, to find cycles.
    /// `names` is true for a map of names to schemas (`properties`), whose keys are not keywords.
    fn resolve(&self, value: &mut Value, names: bool, trail: &mut Vec<String>, expansions: &mut usize) -> Result<bool> {
        let mut changed = false;
        match value {
            Value::Array(items) => {
                for item in items {
                    changed |= self.resolve(item, false, trail, expansions)?;
                }
            }
            Value::Object(object) if names => {
                for child in object.values_mut() {
                    changed |= self.resolve(child, false, trail, expansions)?;
                }
            }
            Value::Object(object) => {
                if let Some(Value::String(reference)) = object.get("$ref") {
                    let reference = reference.clone();
                    object.remove("$ref");
                    let mut resolved = self.definition(&reference, trail, expansions)?;
                    // The keys beside `$ref` win over the shared definition's.
                    resolved.append(object);
                    *object = resolved;
                    changed = true;
                }
                for (key, child) in object.iter_mut() {
                    // Literal values are data, not schemas, even when they hold a "$ref" key.
                    if matches!(key.as_str(), "const" | "default" | "enum" | "examples" | "x-solidcraft-defaults") {
                        continue;
                    }
                    let names = matches!(key.as_str(), "properties" | "patternProperties" | "dependentSchemas" | "$defs" | "definitions");
                    changed |= self.resolve(child, names, trail, expansions)?;
                }
            }
            _ => {}
        }
        Ok(changed)
    }

    fn definition(&self, reference: &str, trail: &mut Vec<String>, expansions: &mut usize) -> Result<Map<String, Value>> {
        let Some(name) = reference.strip_prefix("#/$defs/").filter(|n| !n.is_empty() && !n.contains('/')) else {
            return Err(invalid(self.field, format!("\"$ref\": \"{reference}\" must name a definition as \"#/$defs/<name>\"")));
        };
        if trail.iter().any(|t| t == name) {
            return Err(invalid(self.field, format!("\"$ref\": \"{reference}\" refers back to itself through {}", trail.join(" → "))));
        }
        *expansions += 1;
        if trail.len() >= MAX_DEPTH || *expansions > MAX_EXPANSIONS {
            return Err(invalid(self.field, format!("\"$ref\": \"{reference}\" nests or repeats definitions too much to expand")));
        }
        let found = self.local.and_then(|d| d.get(name)).or_else(|| self.shared.and_then(|d| d.get(name)));
        let Some(Value::Object(definition)) = found else {
            return Err(invalid(self.field, format!("\"$ref\": \"{reference}\" names no schema in the entry's or the manifest's \"$defs\"")));
        };
        let mut definition = Value::Object(definition.clone());
        trail.push(name.to_string());
        self.resolve(&mut definition, false, trail, expansions)?;
        trail.pop();
        Ok(match definition {
            Value::Object(definition) => definition,
            _ => Map::new(),
        })
    }
}
