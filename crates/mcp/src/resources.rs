//! MCP resource templates (`resources/templates/list`, `resources/read`) and autocompletion
//! (`completion/complete`) over SolidCraft's active design, command catalog and bundled skills.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use solidcraft_mcp_server::{Contents, Reference};

use crate::backend::Backend;
use crate::skill_reference;

pub(crate) const BODY_TEMPLATE: &str = "solidcraft://body/{name}";
pub(crate) const SKETCH_TEMPLATE: &str = "solidcraft://sketch/{name}";
pub(crate) const FEATURE_TEMPLATE: &str = "solidcraft://feature/{id}";
pub(crate) const COMMAND_TEMPLATE: &str = "solidcraft://command/{id}";
pub(crate) const SKILL_TEMPLATE: &str = "solidcraft://skill/{skill}/{file}";

const COMMON_MATERIALS: &[&str] = &["6061 Aluminum", "304 Stainless Steel", "ABS", "PETG", "PLA", "Steel", "sheet metal"];

/// Resource templates advertised by `resources/templates/list`.
pub(crate) fn templates() -> Vec<Value> {
    vec![
        json!({
            "uriTemplate": BODY_TEMPLATE,
            "name": "Body",
            "description": "Focused geometry and physical properties of one body (bounds, face/edge counts, volume, mass, component)",
            "mimeType": "application/json",
        }),
        json!({
            "uriTemplate": SKETCH_TEMPLATE,
            "name": "Sketch",
            "description": "Focused inspection of one sketch by name or feature id (plane, dof, curves, points, constraints, profiles)",
            "mimeType": "application/json",
        }),
        json!({
            "uriTemplate": FEATURE_TEMPLATE,
            "name": "Feature",
            "description": "Focused inspection of one timeline feature by numeric id or name (definition, inputs, error, warning)",
            "mimeType": "application/json",
        }),
        json!({
            "uriTemplate": COMMAND_TEMPLATE,
            "name": "Command",
            "description": "Documentation and input/output JSON schemas for one SolidCraft engine command",
            "mimeType": "application/json",
        }),
        json!({
            "uriTemplate": SKILL_TEMPLATE,
            "name": "Skill document",
            "description": "Bundled SolidCraft skill guide or reference document",
            "mimeType": "text/markdown",
        }),
    ]
}

/// Read a URI matching one of SolidCraft's resource templates.
/// Returns `None` when the URI does not match a template or names a missing target (`-32002`).
pub(crate) fn read_template(backend: &mut dyn Backend, uri: &str) -> Option<Result<Contents, String>> {
    if let Some(raw_name) = uri.strip_prefix("solidcraft://body/") {
        return read_body(backend, raw_name);
    }
    if let Some(raw_name) = uri.strip_prefix("solidcraft://sketch/") {
        return read_sketch(backend, raw_name);
    }
    if let Some(raw_id) = uri.strip_prefix("solidcraft://feature/") {
        return read_feature(backend, raw_id);
    }
    if let Some(raw_id) = uri.strip_prefix("solidcraft://command/") {
        return read_command(backend, raw_id);
    }
    if let Some(rest) = uri.strip_prefix("solidcraft://skill/") {
        return read_skill(rest);
    }
    None
}

/// Suggest completions for a resource template parameter or prompt argument.
pub(crate) fn complete(backend: &mut dyn Backend, reference: Reference<'_>, argument: &str, value: &str) -> Vec<String> {
    match reference {
        Reference::Resource(uri) => match (uri, argument) {
            (BODY_TEMPLATE, "name") => filter_completions(design_body_names(backend), value),
            (SKETCH_TEMPLATE, "name") => filter_completions(design_sketch_names(backend), value),
            (FEATURE_TEMPLATE, "id") => filter_completions(design_feature_refs(backend), value),
            (COMMAND_TEMPLATE, "id") => filter_completions(command_ids(backend), value),
            (u, "skill") if u.starts_with("solidcraft://skill/") => filter_completions(skill_reference::skill_names(), value),
            (u, "file") if u.starts_with("solidcraft://skill/") => {
                let files = skill_from_template_uri(u).map_or_else(skill_reference::all_skill_files, |skill| skill_reference::skill_files(&skill));
                filter_completions(files, value)
            }
            _ => Vec::new(),
        },
        Reference::Prompt(name) => match (name, argument) {
            ("model-part", "material") => filter_completions(COMMON_MATERIALS.iter().map(|m| (*m).to_owned()).collect(), value),
            ("edit-design", "target") => filter_completions(design_targets(backend), value),
            ("check-parameters", "parameters") => complete_csv(design_user_params(backend), value),
            ("diagnose-feature", "feature") => filter_ordered(design_features_by_health(backend), value),
            ("assembly-clearance", "occurrences") => complete_csv(assembly_occurrences(backend), value),
            ("assembly-clearance", "joint") => filter_completions(assembly_joints(backend), value),
            _ => Vec::new(),
        },
    }
}

fn read_body(backend: &mut dyn Backend, raw_name: &str) -> Option<Result<Contents, String>> {
    let name = percent_decode(raw_name);
    if name.is_empty() {
        return None;
    }
    let measured = match backend.call("engine.execute", json!({"command": "inspect.measure", "params": {"bodies": [name]}})) {
        Ok(v) => v,
        Err(e) if is_body_not_found(&e) => return None,
        Err(e) => return Some(Err(e)),
    };
    let mut body = measured["bodies"].as_array().and_then(|list| list.first()).cloned()?;
    let definition = backend
        .call("engine.execute", json!({"command": "inspect.measure", "params": {"bodies": [name], "names_only": true}}))
        .ok()
        .and_then(|v| v["bodies"].as_array().and_then(|list| list.first()).and_then(|b| b["definition"].as_str()).map(str::to_owned));
    let lookup_name = definition.as_deref().unwrap_or(&name);
    let component = backend
        .call("engine.execute", json!({"command": "component.list", "params": {"compact": true, "bodies": true}}))
        .ok()
        .and_then(|v| find_body_component(&v, lookup_name, &name));
    if let Some(obj) = body.as_object_mut() {
        if let Some(comp) = component {
            obj.insert("component".into(), json!(comp));
        }
        if let Some(def) = definition
            && def != name
        {
            obj.insert("definition".into(), json!(def));
        }
    }
    Some(Ok(Contents::json(&body)))
}

fn find_body_component(components_reply: &Value, definition: &str, name: &str) -> Option<String> {
    components_reply["components"].as_array()?.iter().find_map(|comp| {
        let has_body = comp["bodies"].as_array()?.iter().filter_map(Value::as_str).any(|b| b == definition || b == name);
        if has_body { comp["name"].as_str().map(str::to_owned) } else { None }
    })
}

fn read_sketch(backend: &mut dyn Backend, raw_name: &str) -> Option<Result<Contents, String>> {
    let key = percent_decode(raw_name);
    if key.is_empty() {
        return None;
    }
    match backend.call("engine.execute", json!({"command": "sketch.inspect", "params": {"sketch": key}})) {
        Ok(v) => Some(Ok(Contents::json(&v))),
        Err(e) if is_sketch_not_found(&e) => None,
        Err(e) => Some(Err(e)),
    }
}

fn read_feature(backend: &mut dyn Backend, raw_id: &str) -> Option<Result<Contents, String>> {
    let key = percent_decode(raw_id);
    if key.is_empty() {
        return None;
    }
    let reply = if let Ok(num) = key.parse::<u64>() {
        match backend.call("engine.execute", json!({"command": "timeline.inspect", "params": {"feature": num}})) {
            Ok(v) => Ok(v),
            Err(e) if is_feature_not_found(&e) => backend.call("engine.execute", json!({"command": "timeline.inspect", "params": {"feature": key}})),
            Err(e) => Err(e),
        }
    } else {
        backend.call("engine.execute", json!({"command": "timeline.inspect", "params": {"feature": key}}))
    };
    match reply {
        Ok(v) => Some(Ok(Contents::json(&v))),
        Err(e) if is_feature_not_found(&e) => None,
        Err(e) => Some(Err(e)),
    }
}

fn read_command(backend: &mut dyn Backend, raw_id: &str) -> Option<Result<Contents, String>> {
    let id = percent_decode(raw_id);
    if id.is_empty() {
        return None;
    }
    match backend.call("engine.execute", json!({"command": "engine.describe", "params": {"command": id}})) {
        Ok(v) => Some(Ok(Contents::json(&v))),
        Err(e) if is_command_not_found(&e) => None,
        Err(e) => Some(Err(e)),
    }
}

fn read_skill(rest: &str) -> Option<Result<Contents, String>> {
    let (raw_skill, raw_file) = rest.split_once('/')?;
    let skill = percent_decode(raw_skill);
    let file = percent_decode(raw_file);
    if skill.is_empty() || file.is_empty() {
        return None;
    }
    let text = skill_reference::get(&skill, &file)?;
    Some(Ok(Contents::Text { mime_type: "text/markdown".into(), text: text.into() }))
}

fn is_body_not_found(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("unknown body") || lower.contains("no body ")
}

fn is_sketch_not_found(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("no sketch ") || lower.contains("is not a sketch")
}

fn is_feature_not_found(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("no feature ") || lower.contains("unknown feature")
}

fn is_command_not_found(err: &str) -> bool {
    err.to_ascii_lowercase().contains("unknown command")
}

fn hex_val(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(&hi) = bytes.get(i + 1)
            && let Some(&lo) = bytes.get(i + 2)
            && let (Some(h), Some(l)) = (hex_val(hi), hex_val(lo))
        {
            out.push((h << 4) | l);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_owned())
}

fn skill_from_template_uri(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("solidcraft://skill/")?;
    let (raw_skill, _) = rest.split_once('/')?;
    let skill = percent_decode(raw_skill);
    (!skill.is_empty() && !skill.contains('{') && !skill.contains('}')).then_some(skill)
}

fn filter_completions(candidates: Vec<String>, value: &str) -> Vec<String> {
    let unique: BTreeSet<String> = candidates.into_iter().filter(|c| !c.is_empty()).collect();
    let needle = value.to_ascii_lowercase();
    let prefix: Vec<String> = unique.iter().filter(|c| c.to_ascii_lowercase().starts_with(&needle)).cloned().collect();
    if !prefix.is_empty() || needle.is_empty() {
        return prefix;
    }
    unique.into_iter().filter(|c| c.to_ascii_lowercase().contains(&needle)).collect()
}

fn filter_ordered(candidates: Vec<String>, value: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let unique: Vec<String> = candidates.into_iter().filter(|c| !c.is_empty() && seen.insert(c.clone())).collect();
    let needle = value.to_ascii_lowercase();
    let prefix: Vec<String> = unique.iter().filter(|c| c.to_ascii_lowercase().starts_with(&needle)).cloned().collect();
    if !prefix.is_empty() || needle.is_empty() {
        return prefix;
    }
    unique.into_iter().filter(|c| c.to_ascii_lowercase().contains(&needle)).collect()
}

fn complete_csv(candidates: Vec<String>, value: &str) -> Vec<String> {
    let Some((head, tail)) = value.rsplit_once(',') else {
        return filter_completions(candidates, value);
    };
    let used: BTreeSet<String> = head.split(',').map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty()).collect();
    let remaining: Vec<String> = candidates.into_iter().filter(|c| !used.contains(&c.to_ascii_lowercase())).collect();
    let needle = tail.trim_start();
    let sep = if tail.starts_with(' ') || needle.is_empty() { ", " } else { "," };
    let prefix = head.trim_end();
    filter_completions(remaining, needle).into_iter().map(|item| format!("{prefix}{sep}{item}")).collect()
}

fn design_body_names(backend: &mut dyn Backend) -> Vec<String> {
    let Ok(doc) = backend.call("document.inspect", json!({"measure": false, "timeline": false, "bodies": "names"})) else {
        return Vec::new();
    };
    doc["bodies"].as_array().map(|list| list.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default()
}

fn design_sketch_names(backend: &mut dyn Backend) -> Vec<String> {
    let Ok(doc) = backend.call("document.inspect", json!({"measure": false, "timeline": false, "bodies": false})) else {
        return Vec::new();
    };
    doc["sketches"].as_array().map(|list| list.iter().filter_map(|s| s["name"].as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

fn design_feature_refs(backend: &mut dyn Backend) -> Vec<String> {
    let Ok(doc) = backend.call("document.inspect", json!({"measure": false, "timeline": "summary", "bodies": false})) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in doc["timeline"].as_array().into_iter().flatten() {
        if let Some(name) = entry["name"].as_str() {
            out.push(name.to_owned());
        }
        if let Some(id) = entry["id"].as_u64() {
            out.push(id.to_string());
        }
    }
    out
}

fn design_features_by_health(backend: &mut dyn Backend) -> Vec<String> {
    let Ok(doc) = backend.call("document.inspect", json!({"measure": false, "timeline": "summary", "bodies": false})) else {
        return Vec::new();
    };
    let mut troubled = Vec::new();
    let mut healthy = Vec::new();
    for entry in doc["timeline"].as_array().into_iter().flatten() {
        let has_issue = entry.get("error").is_some_and(|v| !v.is_null()) || entry.get("warning").is_some_and(|v| !v.is_null());
        let bucket = if has_issue { &mut troubled } else { &mut healthy };
        if let Some(name) = entry["name"].as_str() {
            bucket.push(name.to_owned());
        }
        if let Some(id) = entry["id"].as_u64() {
            bucket.push(id.to_string());
        }
    }
    troubled.extend(healthy);
    troubled
}

fn design_targets(backend: &mut dyn Backend) -> Vec<String> {
    let Ok(doc) = backend.call("document.inspect", json!({"measure": false, "timeline": "summary", "bodies": "names"})) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for b in doc["bodies"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        out.push(b.to_owned());
    }
    for s in doc["sketches"].as_array().into_iter().flatten() {
        if let Some(name) = s["name"].as_str() {
            out.push(name.to_owned());
        }
    }
    for f in doc["timeline"].as_array().into_iter().flatten() {
        if let Some(name) = f["name"].as_str() {
            out.push(name.to_owned());
        }
    }
    out
}

fn design_user_params(backend: &mut dyn Backend) -> Vec<String> {
    let Ok(doc) = backend.call("document.inspect", json!({"measure": false, "timeline": false, "bodies": false})) else {
        return Vec::new();
    };
    doc["params"]
        .as_array()
        .map(|list| list.iter().filter(|p| p["model"] != true).filter_map(|p| p["name"].as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

fn assembly_occurrences(backend: &mut dyn Backend) -> Vec<String> {
    let mut out = design_body_names(backend);
    if let Ok(list) = backend.call("engine.execute", json!({"command": "component.list", "params": {"compact": true}})) {
        for comp in list["components"].as_array().into_iter().flatten() {
            for occ in comp["occurrences"].as_array().into_iter().flatten() {
                if let Some(name) = occ["name"].as_str() {
                    out.push(name.to_owned());
                }
            }
        }
    }
    out
}

fn assembly_joints(backend: &mut dyn Backend) -> Vec<String> {
    let Ok(list) = backend.call("engine.execute", json!({"command": "joint.list", "params": {"summary": true}})) else {
        return Vec::new();
    };
    list["joints"].as_array().map(|items| items.iter().filter_map(|j| j["name"].as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

fn command_ids(backend: &mut dyn Backend) -> Vec<String> {
    let Ok(list) = backend.call("engine.commands", json!({})) else {
        return Vec::new();
    };
    list.as_array().map(|items| items.iter().filter_map(|c| c["id"].as_str().map(str::to_owned)).collect()).unwrap_or_default()
}
