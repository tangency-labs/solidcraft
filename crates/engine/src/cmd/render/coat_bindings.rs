//! Stable opaque clear-coat bindings shared by CPU and GPU scene capture.
//! Lowering changes only already-sized captured surface fields, never render options.
use crate::{Result, Session, params::bad};
use serde_json::{Value, json};
use solidcraft_doc::{
    render_coat::{CoatBindings, SavedCoat},
    render_settings::RenderJob,
    render_surface::RenderBodyTarget,
};
use solidcraft_render::{ProductScene, job::RaytraceOptions};
use std::{collections::BTreeMap, sync::Arc};

/// No bindings is the original zero-allocation capture path. Even zero-weight
/// authored bindings require a current opaque target; they cannot mask Glass.
pub(super) fn apply(s: &Session, scene: &mut ProductScene, bindings: &CoatBindings, cpu: Option<&RaytraceOptions>, command: &str) -> Result<()> {
    if bindings.is_empty() {
        return Ok(());
    }
    bindings.validate().map_err(|e| bad(command, e.to_string()))?;
    let identities = super::surface_bindings::targets(s, command)?;
    // The shared resolver indexes authored source identity. This additional
    // captured-name index is bounded by the 64 requested targets, not scene size.
    let mut visible = BTreeMap::new();
    for binding in bindings.iter() {
        let name = identities.get(&binding.target).ok_or_else(|| bad(command, "saved clear-coat target changed; explicit rebinding required"))?;
        if visible.insert(name.as_str(), None).is_some() {
            return Err(bad(command, "ambiguous stable clear-coat world target"));
        }
    }
    for (index, body) in scene.meshes.iter().enumerate() {
        if let Some(slot) = visible.get_mut(body.name.as_str())
            && slot.replace(index).is_some()
        {
            return Err(bad(command, "ambiguous captured clear-coat target"));
        }
    }
    let mut resolved = Vec::new();
    resolved.try_reserve_exact(bindings.len()).map_err(|_| bad(command, "could not reserve clear-coat capture metadata"))?;
    if resolved.capacity() != bindings.len() {
        return Err(bad(command, "clear-coat capture capacity exceeds admission"));
    }
    for binding in bindings.iter() {
        let name = identities.get(&binding.target).ok_or_else(|| bad(command, "saved clear-coat target changed; explicit rebinding required"))?;
        let index = visible.get(name.as_str()).copied().flatten().ok_or_else(|| bad(command, "saved clear-coat target is hidden or absent"))?;
        let body = scene.meshes.get(index).ok_or_else(|| bad(command, "clear-coat target disappeared"))?;
        if body.transmission.is_some() || body.faces.iter().any(|face| face.transmission.is_some()) {
            return Err(bad(command, "saved clear coat requires an opaque target, including zero-weight authored coats"));
        }
        if cpu.is_some_and(|options| {
            options.dielectrics.iter().any(|entry| entry.body_index == index)
                || options.reflections.as_ref().is_some_and(|overrides| overrides.bodies.iter().any(|entry| entry.body == *name))
                || options.anisotropic_faces.iter().flatten().any(|entry| entry.body_index == index)
        }) {
            return Err(bad(command, "CPU clear-coat targets cannot share Glass, selected reflection or anisotropy overrides"));
        }
        resolved.push((index, name.as_str(), binding.coat.to_runtime().map_err(|e| bad(command, e.to_string()))?));
    }
    drop(visible);
    for (index, name, coat) in resolved {
        super::clear_coat::apply_one(scene, index, name, coat, command)?;
    }
    Ok(())
}

pub(super) fn bind(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.coat.bind";
    let input: crate::schema::render::CoatBindParams = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    let coat = input.coat.material();
    coat.validate().map_err(|e| bad(command, e.to_string()))?;
    if input.expected_revision != s.revision {
        return Err(bad(command, "clear-coat binding review is stale"));
    }
    let mut setup = super::patched_settings(s, &input.name, &input.set, command)?;
    if s.doc.render.setups.get(&input.name).is_some_and(|old| old.coats != setup.coats) {
        return Err(bad(command, "setup patch cannot replace saved clear coats; use explicit bind/unbind"));
    }
    if !matches!(setup.job, Some(RenderJob::CpuRaytrace { .. } | RenderJob::GpuPath { .. })) {
        return Err(bad(command, "saved clear coats require an explicit CPU or GPU raytrace setup"));
    }
    let mut scene = crate::product_scene::extract(s)?;
    let name = &scene
        .meshes
        .get(input.body_index)
        .filter(|body| body.name == input.body_name)
        .ok_or_else(|| bad(command, "clear-coat target ordinal and exact name do not match"))?
        .name;
    let identities = super::surface_bindings::targets(s, command)?;
    let mut candidates = identities.iter().filter(|(_, world)| *world == name);
    let target =
        candidates.next().map(|(target, _)| target.clone()).ok_or_else(|| bad(command, "clear-coat target has no stable source identity"))?;
    if candidates.next().is_some() {
        return Err(bad(command, "ambiguous clear-coat target identity"));
    }
    setup.coats.set(SavedCoat { target, coat }, input.replace).map_err(|e| bad(command, e.to_string()))?;
    setup.validate().map_err(|e| bad(command, e.to_string()))?;
    // Verify the actual opaque source even for inactive authored values. Runtime
    // options are not invented while saving an offline stable numerical setup.
    apply(s, &mut scene, &setup.coats, None, command)?;
    let mut settings = (*s.doc.render).clone();
    settings.setups.insert(input.name.clone(), setup.clone());
    settings.validate().map_err(|e| bad(command, e.to_string()))?;
    if settings != *s.doc.render {
        s.doc_mut().render = Arc::new(settings);
    }
    Ok(json!({"name":input.name,"setup":setup,"geometry_and_engineering_unchanged":true}))
}

pub(super) fn unbind(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.coat.unbind";
    let input: crate::schema::render::CoatUnbindParams = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    let target: RenderBodyTarget = serde_json::from_value(input.target).map_err(|e| bad(command, e.to_string()))?;
    target.validate().map_err(|e| bad(command, e.to_string()))?;
    if input.expected_revision != s.revision {
        return Err(bad(command, "clear-coat removal review is stale"));
    }
    let mut settings = (*s.doc.render).clone();
    let setup = settings.setups.get_mut(&input.name).ok_or_else(|| bad(command, "unknown clear-coat setup"))?;
    let removed = setup.coats.remove(&target);
    settings.validate().map_err(|e| bad(command, e.to_string()))?;
    if settings != *s.doc.render {
        s.doc_mut().render = Arc::new(settings);
    }
    Ok(json!({"name":input.name,"removed":removed}))
}
