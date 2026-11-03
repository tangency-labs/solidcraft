//! Named saved-shot management through the existing RenderSettings authority.
//! No captured jobs, camera navigation, external files or engineering data are edited.
use crate::schema::TypedContract;
use crate::schema::render_shots::{ShotChange, ShotOperation, ShotParams};
use crate::{Result, Session, params::bad};
use serde_json::Value;
use solidcraft_doc::render_settings::RenderSettings;
use std::sync::Arc;

pub(super) const CONTRACT: TypedContract<ShotParams> = TypedContract::new();

fn change(s: &mut Session, params: &Value, operation: ShotOperation) -> Result<Value> {
    let command = match operation {
        ShotOperation::Duplicate => "render.shot.duplicate",
        ShotOperation::Rename => "render.shot.rename",
        ShotOperation::Remove => "render.shot.remove",
    };
    let input: ShotParams = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    if input.expected_revision != s.revision {
        return Err(bad(command, "saved shot changed after review; refresh before editing"));
    }
    RenderSettings::validate_name(&input.name).map_err(|e| bad(command, e.to_string()))?;
    let source = s.doc.render.setups.get(&input.name).cloned().ok_or_else(|| bad(command, "no saved shot with that name"))?;
    let mut settings = (*s.doc.render).clone();
    let output = if operation == ShotOperation::Remove {
        if input.new_name.is_some() {
            return Err(bad(command, "remove does not accept a destination name"));
        }
        settings.setups.remove(&input.name);
        None
    } else {
        let destination = input.new_name.ok_or_else(|| bad(command, "new_name is required"))?;
        RenderSettings::validate_name(&destination).map_err(|e| bad(command, e.to_string()))?;
        if settings.setups.contains_key(&destination) {
            return Err(bad(command, "destination already exists; saved shots are never overwritten"));
        }
        settings.setups.insert(destination.clone(), source);
        if operation == ShotOperation::Rename {
            settings.setups.remove(&input.name);
        }
        Some(destination)
    };
    settings.validate().map_err(|e| bad(command, e.to_string()))?;
    s.doc_mut().render = Arc::new(settings);
    let reply = ShotChange {
        operation,
        source: input.name,
        name: output,
        resources_preserved: true,
        camera_navigation_unchanged: true,
        engineering_properties_unchanged: true,
        captured_jobs_unchanged: true,
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| bad(command, e.to_string()))
}
pub(super) fn duplicate(s: &mut Session, params: &Value) -> Result<Value> {
    change(s, params, ShotOperation::Duplicate)
}
pub(super) fn rename(s: &mut Session, params: &Value) -> Result<Value> {
    change(s, params, ShotOperation::Rename)
}
pub(super) fn remove(s: &mut Session, params: &Value) -> Result<Value> {
    change(s, params, ShotOperation::Remove)
}
