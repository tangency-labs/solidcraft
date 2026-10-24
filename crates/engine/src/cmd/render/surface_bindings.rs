//! Resolve portable surface intent into the existing captured GPU texture contract.
//! Definition/feature/occurrence identities are checked on every Check and Start.
use crate::{Result, Session, params::bad};
use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_doc::{
    render_resources::HdrId,
    render_settings::{RenderJob, RenderSetup},
    render_surface::{BuiltinSurfaceSource, RenderBodyTarget, SavedSurface, SurfaceBindings},
};
use solidcraft_geom::Vec3;
use solidcraft_render::{ProductScene, job::textures::PlanarFrame};
use std::{collections::BTreeMap, sync::Arc};

fn vec(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}
fn array(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}
fn transform(frame: PlanarFrame, m: &solidcraft_doc::Mat, command: &str) -> Result<PlanarFrame> {
    let result = PlanarFrame {
        origin: array(solidcraft_doc::apply_point(m, vec(frame.origin))),
        u: array(solidcraft_doc::apply_vector(m, vec(frame.u))),
        v: array(solidcraft_doc::apply_vector(m, vec(frame.v))),
    };
    result.validate().map_err(|e| bad(command, e.to_string()))?;
    Ok(result)
}

fn source(id: &str, command: &str) -> Result<BuiltinSurfaceSource> {
    let item = super::library::item("surface_manifest", id, command)?;
    let digest =
        |role: &str| -> Result<HdrId> { serde_json::from_value(item["maps"][role]["sha256"].clone()).map_err(|e| bad(command, e.to_string())) };
    let result = BuiltinSurfaceSource {
        id: id.into(),
        version: item["resource_version"].as_str().ok_or_else(|| bad(command, "surface resource version missing"))?.into(),
        base_color_sha256: digest("base_color")?,
        normal_sha256: digest("normal_open_gl")?,
        roughness_sha256: digest("roughness")?,
        metallic_sha256: digest("metallic")?,
    };
    result.validate().map_err(|e| bad(command, e.to_string()))?;
    Ok(result)
}

/// Build an exact identity index once, not an O(bindings * visible bodies) scan.
pub(super) fn targets(s: &Session, command: &str) -> Result<BTreeMap<RenderBodyTarget, String>> {
    if s.model.is_stale() {
        return Err(bad(command, "surface bindings require settled model geometry"));
    }
    let state = s.model.state();
    let mut definitions = BTreeMap::new();
    for body in &state.bodies {
        let key = (body.definition_name().to_string(), s.doc.model_body_component(body));
        if definitions.insert(key, body.feature).is_some() {
            return Err(bad(command, "ambiguous surface definition identity"));
        }
    }
    let frames = s.doc.body_frames_with_source(&state);
    if frames.len() > solidcraft_render::job::MAX_CPU_BODIES {
        return Err(bad(command, "too many placed surface targets"));
    }
    let mut result = BTreeMap::new();
    for (world, definition, component, path, _) in frames {
        let feature = *definitions.get(&(definition.clone(), component)).ok_or_else(|| bad(command, "surface definition disappeared"))?;
        let target = RenderBodyTarget { definition, feature, component, occurrence_path: path };
        target.validate().map_err(|e| bad(command, e.to_string()))?;
        if result.insert(target, world).is_some() {
            return Err(bad(command, "ambiguous render surface occurrence identity"));
        }
    }
    Ok(result)
}

pub(super) fn resolve(s: &Session, scene: &ProductScene, bindings: &SurfaceBindings, command: &str) -> Result<Vec<super::textures::Override>> {
    bindings.validate().map_err(|e| bad(command, e.to_string()))?;
    if bindings.is_empty() {
        return Ok(Vec::new());
    }
    let identities = targets(s, command)?;
    let world = s.world_state();
    let world_by_name: BTreeMap<_, _> = world.bodies.iter().map(|body| (body.name.as_str(), body)).collect();
    let visible: BTreeMap<_, _> = scene.meshes.iter().enumerate().map(|(i, body)| (body.name.as_str(), i)).collect();
    let mut result = Vec::new();
    result.try_reserve_exact(bindings.len()).map_err(|_| bad(command, "could not reserve saved surface lowering"))?;
    for binding in bindings.iter() {
        if binding.source != source(&binding.source.id, command)? {
            return Err(bad(command, "saved surface version or SHA-256 differs from installed source; explicit rebinding required"));
        }
        let name =
            identities.get(&binding.target).ok_or_else(|| bad(command, "saved surface target is missing or changed; explicit rebinding required"))?;
        let ordinal = *visible.get(name.as_str()).ok_or_else(|| bad(command, "saved surface target is hidden or absent from this render"))?;
        let body = world_by_name.get(name.as_str()).ok_or_else(|| bad(command, "saved surface world target disappeared"))?;
        if body.definition_name() != binding.target.definition {
            return Err(bad(command, "surface definition/world mapping disagrees"));
        }
        let frame = transform(binding.local_frame, &body.source_transform, command)?;
        let id = &binding.source.id;
        let wire = json!({"body_index":ordinal,"body_name":name,"frame":frame,"normal_strength":binding.normal_strength,
            "flip_v":true,"base_color_mode":"replace","base_color_srgb":format!("builtin://{id}/base_color"),
            "normal_opengl":format!("builtin://{id}/normal_open_gl"),"roughness_linear":format!("builtin://{id}/roughness"),"metallic_linear":format!("builtin://{id}/metallic")});
        result.push(serde_json::from_value(wire).map_err(|e| bad(command, e.to_string()))?);
    }
    Ok(result)
}
fn empty() -> Value {
    json!({})
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bind {
    name: String,
    surface: Value,
    #[serde(default = "empty")]
    set: Value,
    #[serde(default)]
    replace: bool,
}
fn publish(s: &mut Session, name: String, setup: RenderSetup, command: &str) -> Result<Value> {
    let mut settings = (*s.doc.render).clone();
    settings.setups.insert(name.clone(), setup.clone());
    settings.validate().map_err(|e| bad(command, e.to_string()))?;
    if settings != *s.doc.render {
        s.doc_mut().render = Arc::new(settings);
    }
    Ok(json!({"name":name,"setup":setup,"mapping_space":"definition_local_mm","geometry_and_physical_materials_unchanged":true}))
}
pub(super) fn bind(s: &mut Session, params: &Value) -> Result<Value> {
    super::library::require_bundled("render.library.surface.bind")?;
    let command = "render.library.surface.bind";
    let input: Bind = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    let mut setup = super::patched_settings(s, &input.name, &input.set, command)?;
    if s.doc.render.setups.get(&input.name).is_some_and(|old| old.surfaces != setup.surfaces) {
        return Err(bad(command, "setup patch cannot erase or replace saved surface bindings; use explicit bind/unbind commands"));
    }

    if !matches!(setup.job, Some(RenderJob::GpuPath { .. })) {
        return Err(bad(command, "persistent authored maps require an explicit GPU setup"));
    }
    let prepared = super::library::prepare_surface(s, &input.surface)?;
    let id = prepared["id"].as_str().ok_or_else(|| bad(command, "missing prepared surface identifier"))?;
    let name = prepared["texture"]["body_name"].as_str().ok_or_else(|| bad(command, "missing prepared body identity"))?;
    let identities = targets(s, command)?;
    let mut matches = identities.iter().filter(|(_, world)| world.as_str() == name);
    let target = matches.next().map(|(key, _)| key.clone()).ok_or_else(|| bad(command, "surface target has no stable occurrence identity"))?;
    if matches.next().is_some() {
        return Err(bad(command, "surface target is ambiguous"));
    }
    let world = s.world_state();
    let body = world.body(name).ok_or_else(|| bad(command, "surface target disappeared"))?;
    let inverse = solidcraft_doc::mat_inverse(&body.source_transform).ok_or_else(|| bad(command, "surface mapping transform is singular"))?;
    let frame: PlanarFrame = serde_json::from_value(prepared["texture"]["frame"].clone()).map_err(|e| bad(command, e.to_string()))?;
    setup
        .surfaces
        .set(
            SavedSurface { target, source: source(id, command)?, local_frame: transform(frame, &inverse, command)?, normal_strength: 1.0 },
            input.replace,
        )
        .map_err(|e| bad(command, e.to_string()))?;
    // Validate actual opaque target and precision limits before the one document mutation.
    let scene = crate::product_scene::extract(s)?;
    let resolved = resolve(s, &scene, &setup.surfaces, command)?;
    super::textures::validate_targets(&scene, &resolved, command)?;
    publish(s, input.name, setup, command)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Unbind {
    name: String,
    target: RenderBodyTarget,
    #[serde(default = "empty")]
    set: Value,
}
pub(super) fn unbind(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.library.surface.unbind";
    let input: Unbind = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    input.target.validate().map_err(|e| bad(command, e.to_string()))?;
    let mut setup = super::patched_settings(s, &input.name, &input.set, command)?;
    setup.surfaces.remove(&input.target);
    publish(s, input.name, setup, command)
}
