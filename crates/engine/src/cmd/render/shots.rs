//! Named saved-shot management through the existing RenderSettings authority.
//! No captured jobs, camera navigation, external files or engineering data are edited.
use crate::{Result, Session, params::bad};
use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_doc::render_settings::RenderSettings;
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Named {
    name: String,
    #[serde(default)]
    new_name: Option<String>,
    expected_revision: u64,
}
fn change(s: &mut Session, params: &Value, operation: &str) -> Result<Value> {
    let command = match operation {
        "duplicate" => "render.shot.duplicate",
        "rename" => "render.shot.rename",
        _ => "render.shot.remove",
    };
    let input: Named = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    if input.expected_revision != s.revision {
        return Err(bad(command, "saved shot changed after review; refresh before editing"));
    }
    RenderSettings::validate_name(&input.name).map_err(|e| bad(command, e.to_string()))?;
    let source = s.doc.render.setups.get(&input.name).cloned().ok_or_else(|| bad(command, "no saved shot with that name"))?;
    let mut settings = (*s.doc.render).clone();
    let output = if operation == "remove" {
        if input.new_name.is_some() {
            return Err(bad(command, "remove does not accept a destination name"));
        }
        settings.setups.remove(&input.name);
        Value::Null
    } else {
        let destination = input.new_name.as_ref().ok_or_else(|| bad(command, "new_name is required"))?;
        RenderSettings::validate_name(destination).map_err(|e| bad(command, e.to_string()))?;
        if settings.setups.contains_key(destination) {
            return Err(bad(command, "destination already exists; saved shots are never overwritten"));
        }
        settings.setups.insert(destination.clone(), source);
        if operation == "rename" {
            settings.setups.remove(&input.name);
        }
        json!(destination)
    };
    settings.validate().map_err(|e| bad(command, e.to_string()))?;
    s.doc_mut().render = Arc::new(settings);
    Ok(json!({"operation":operation,"source":input.name,"name":output,"resources_preserved":true,
        "camera_navigation_unchanged":true,"engineering_properties_unchanged":true,"captured_jobs_unchanged":true}))
}
pub(super) fn duplicate(s: &mut Session, params: &Value) -> Result<Value> {
    change(s, params, "duplicate")
}
pub(super) fn rename(s: &mut Session, params: &Value) -> Result<Value> {
    change(s, params, "rename")
}
pub(super) fn remove(s: &mut Session, params: &Value) -> Result<Value> {
    change(s, params, "remove")
}
