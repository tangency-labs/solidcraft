//! Read-only light inspection. No host, geometry evaluation, camera or design mutation.

use serde_json::Value;
use solidcraft_doc::render_settings::{RenderSettings, RenderSetup};

use crate::params::bad;
use crate::schema::render_lights::{EditParams, EditResult, Geometry, Light};
use crate::schema::render_lights::{ListParams, ListResult};
use crate::{Result, Session};

pub(super) fn list(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "render.lights.list";
    if !params.is_object() {
        return Err(bad(cmd, "expected an object"));
    }
    let request: ListParams = serde_json::from_value(params.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    if params.get("setup").is_some_and(Value::is_null) || params.get("settings").is_some_and(Value::is_null) {
        return Err(bad(cmd, "setup and settings cannot be null"));
    }
    let setup = match (request.setup, request.settings) {
        (Some(_), Some(_)) => return Err(bad(cmd, "choose setup or settings, not both")),
        (Some(name), None) => {
            RenderSettings::validate_name(&name).map_err(|e| bad(cmd, e.to_string()))?;
            s.doc.render.setups.get(&name).cloned().ok_or_else(|| bad(cmd, "no saved render setup with that name"))?
        }
        (None, Some(settings)) => {
            if !settings.is_object() {
                return Err(bad(cmd, "settings must be an object"));
            }
            serde_json::from_value(settings).map_err(|e| bad(cmd, e.to_string()))?
        }
        (None, None) => RenderSetup::default(),
    };
    setup.validate().map_err(|e| bad(cmd, e.to_string()))?;
    serde_json::to_value(ListResult { lights: crate::render_lights::inspect(&setup).map_err(|e| bad(cmd, e))? }).map_err(|e| bad(cmd, e.to_string()))
}

fn change(s: &mut Session, params: &Value, operation: &str) -> Result<Value> {
    let command = format!("render.lights.{operation}");
    if !params.is_object() {
        return Err(bad(&command, "expected an object"));
    }
    let input: EditParams = serde_json::from_value(params.clone()).map_err(|e| bad(&command, e.to_string()))?;
    RenderSettings::validate_name(&input.name).map_err(|e| bad(&command, e.to_string()))?;
    if input.expected_revision.is_some_and(|revision| revision != s.revision) {
        return Err(bad(&command, "light settings changed; refresh before editing"));
    }
    let mut setup = match input.settings {
        Some(value) if value.is_object() => serde_json::from_value::<RenderSetup>(value).map_err(|e| bad(&command, e.to_string()))?,
        Some(_) => return Err(bad(&command, "settings must be an object")),
        None => s.doc.render.setups.get(&input.name).cloned().unwrap_or_default(),
    };
    let mut lights = crate::render_lights::inspect(&setup).map_err(|e| bad(&command, e))?;
    let target = [setup.camera.target.x, setup.camera.target.y, setup.camera.target.z];
    let mut selected = None;
    match operation {
        "add" => {
            if input.id.is_some() {
                return Err(bad(&command, "add does not accept an id"));
            }
            let mut patch = input.set.unwrap_or_default();
            if patch.kind.is_none() {
                patch.kind = Some("area".into());
            }
            let [x, y, z] = target;
            let mut light = Light {
                id: String::new(),
                name: String::new(),
                name_key: String::new(),
                number: 0,
                implicit: false,
                intensity: 1.0,
                colour: [1.0; 3],
                geometry: Geometry::Area { position_mm: [x, y - 100.0, z + 100.0], edge_u_mm: [10.0, 0.0, 0.0], edge_v_mm: [0.0, -10.0, 0.0] },
            };
            crate::render_lights::patch(&mut light, &patch, target, setup.camera.distance).map_err(|e| bad(&command, e))?;
            lights.push(light);
            selected = lights.len().checked_sub(1);
        }
        "set" | "duplicate" | "remove" => {
            let id = input.id.as_deref().ok_or_else(|| bad(&command, "id is required"))?;
            crate::render_lights::parse_id(id).map_err(|e| bad(&command, e))?;
            let index = lights.iter().position(|light| light.id == id).ok_or_else(|| bad(&command, "light no longer exists"))?;
            if operation == "remove" {
                if input.set.is_some() {
                    return Err(bad(&command, "remove does not accept set"));
                }
                lights.retain(|light| light.id != id);
            } else if operation == "duplicate" {
                if input.set.is_some() {
                    return Err(bad(&command, "duplicate does not accept set"));
                }
                let light = lights.get(index).cloned().ok_or_else(|| bad(&command, "missing light"))?;
                lights.push(light);
                selected = lights.len().checked_sub(1);
            } else {
                let patch = input.set.ok_or_else(|| bad(&command, "set is required"))?;
                let light = lights.get_mut(index).ok_or_else(|| bad(&command, "missing light"))?;
                if matches!(setup.job, Some(solidcraft_doc::render_settings::RenderJob::CpuRaytrace { .. }))
                    && light.geometry.kind() == "directional"
                    && patch.softness.is_some()
                {
                    return Err(bad(&command, "CPU directional lights have hard shadows; softness is not supported"));
                }
                crate::render_lights::patch(light, &patch, target, setup.camera.distance).map_err(|e| bad(&command, e))?;
                selected = Some(index);
            }
        }
        _ => return Err(bad(&command, "unknown light operation")),
    }
    let id = selected.and_then(|index| {
        lights.get(index).map(|light| {
            let kind = light.geometry.kind();
            let slot = lights
                .iter()
                .take(index)
                .filter(|prior| {
                    if matches!(kind, "point" | "spot") { matches!(prior.geometry.kind(), "point" | "spot") } else { prior.geometry.kind() == kind }
                })
                .count();
            format!("{kind}:{slot}")
        })
    });
    crate::render_lights::write(&mut setup, &lights).map_err(|e| bad(&command, e))?;
    let mut settings = (*s.doc.render).clone();
    settings.setups.insert(input.name.clone(), setup.clone());
    settings.validate().map_err(|e| bad(&command, e.to_string()))?;
    if *s.doc.render != settings {
        s.doc_mut().render = std::sync::Arc::new(settings);
    }
    serde_json::to_value(EditResult { id, name: input.name, setup: serde_json::to_value(setup).map_err(|e| bad(&command, e.to_string()))? })
        .map_err(|e| bad(&command, e.to_string()))
}

pub(super) fn set(s: &mut Session, p: &Value) -> Result<Value> {
    change(s, p, "set")
}
pub(super) fn add(s: &mut Session, p: &Value) -> Result<Value> {
    change(s, p, "add")
}
pub(super) fn duplicate(s: &mut Session, p: &Value) -> Result<Value> {
    change(s, p, "duplicate")
}
pub(super) fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    change(s, p, "remove")
}
