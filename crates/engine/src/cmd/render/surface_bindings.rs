//! Resolve portable surface intent into the existing captured GPU texture contract.
//! Definition/feature/occurrence identities are checked on every Check and Start.
use crate::schema::TypedContract;
use crate::schema::render_bindings::{SurfaceBindParams, SurfaceBindingSaved, SurfaceUnbindParams};
use crate::{Result, Session, params::bad};
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_doc::{
    render_resources::HdrId,
    render_settings::{RenderJob, RenderSetup},
    render_surface::{BuiltinSurfaceSource, RenderBodyTarget, SavedSurface, SurfaceBindings, SurfaceSource},
};
use solidcraft_render::{ProductScene, job::textures::PlanarFrame};
use std::{collections::BTreeMap, sync::Arc};

fn vec(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}
fn array(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}
pub(super) fn transform(frame: PlanarFrame, m: &solidcraft_doc::Mat, command: &str) -> Result<PlanarFrame> {
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
    let world = s.world_state()?;
    let world_by_name: BTreeMap<_, _> = world.bodies.iter().map(|body| (body.name.as_str(), body)).collect();
    let visible: BTreeMap<_, _> = scene.meshes.iter().enumerate().map(|(i, body)| (body.name.as_str(), i)).collect();
    let mut result = Vec::new();
    result.try_reserve_exact(bindings.len()).map_err(|_| bad(command, "could not reserve saved surface lowering"))?;
    for binding in bindings.iter() {
        if let SurfaceSource::Builtin(builtin) = &binding.source
            && *builtin != source(&builtin.id, command)?
        {
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
        let mut wire = json!({"body_index":ordinal,"body_name":name,"frame":frame,"normal_strength":binding.normal_strength,
            "base_color_mode":"replace"});
        match &binding.source {
            SurfaceSource::Builtin(source) => {
                let id = &source.id;
                wire["flip_v"] = json!(true);
                for (field, role) in [
                    ("base_color_srgb", "base_color"),
                    ("normal_opengl", "normal_open_gl"),
                    ("roughness_linear", "roughness"),
                    ("metallic_linear", "metallic"),
                ] {
                    wire[field] = json!(format!("builtin://{id}/{role}"));
                }
            }
            SurfaceSource::Embedded(source) => {
                wire["flip_v"] = json!(source.flip_v);
                for (field, id) in [
                    ("base_color_srgb", source.base_color),
                    ("normal_opengl", source.normal_open_gl),
                    ("roughness_linear", source.roughness),
                    ("metallic_linear", source.metallic),
                ] {
                    if let Some(id) = id {
                        s.doc.render.texture_resources.required(&id).map_err(|e| bad(command, e.to_string()))?;
                        wire[field] = json!(format!("embedded://{id}"));
                    }
                }
            }
        }
        result.push(serde_json::from_value(wire).map_err(|e| bad(command, e.to_string()))?);
    }
    Ok(result)
}
pub(super) const BIND_CONTRACT: TypedContract<SurfaceBindParams> = TypedContract::new();
pub(super) const UNBIND_CONTRACT: TypedContract<SurfaceUnbindParams> = TypedContract::new();

/// Save `setup` under `name` in one undo step (none when nothing changed) and reply with it.
fn publish(s: &mut Session, name: String, setup: RenderSetup, command: &str) -> Result<Value> {
    let mut settings = (*s.doc.render).clone();
    settings.setups.insert(name.clone(), setup.clone());
    settings.validate().map_err(|e| bad(command, e.to_string()))?;
    // Serialized before the document changes, so a setup that can't be written leaves it as it was.
    let setup = serde_json::to_value(&setup).map_err(|e| bad(command, e.to_string()))?;
    if settings != *s.doc.render {
        s.doc_mut().render = Arc::new(settings);
    }
    let reply = SurfaceBindingSaved {
        name,
        setup,
        mapping_space: "definition_local_mm".into(),
        geometry_and_physical_materials_unchanged: true,
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| bad(command, e.to_string()))
}
pub(super) fn bind(s: &mut Session, params: &Value) -> Result<Value> {
    super::library::require_bundled("render.library.surface.bind")?;
    let command = "render.library.surface.bind";
    let input: SurfaceBindParams = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    let mut setup = super::patched_settings(s, &input.name, &input.set, command)?;
    if s.doc.render.setups.get(&input.name).is_some_and(|old| old.surfaces != setup.surfaces) {
        return Err(bad(command, "setup patch cannot erase or replace saved surface bindings; use explicit bind/unbind commands"));
    }

    if !matches!(setup.job, Some(RenderJob::CpuRaytrace { .. } | RenderJob::GpuPath { .. })) {
        return Err(bad(command, "persistent authored maps require an explicit CPU raytrace or GPU path setup"));
    }
    if let Some(RenderJob::CpuRaytrace { options, .. }) = &setup.job {
        if !setup.optics.is_empty() && options.path.is_none() {
            return Err(bad(command, "CPU maps with Glass require Indirect lighting and separate targets"));
        }
        // Saving validates portable intent; capture later resolves stable aging
        // targets. The final setup validator still forbids maps on a Glass target.
        options.validate_surface_maps(setup.exposure_ev).map_err(|e| bad(command, e.to_string()))?;
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
    let world = s.world_state()?;
    let body = world.body(name).ok_or_else(|| bad(command, "surface target disappeared"))?;
    let inverse = solidcraft_doc::mat_inverse(&body.source_transform).ok_or_else(|| bad(command, "surface mapping transform is singular"))?;
    let frame: PlanarFrame = serde_json::from_value(prepared["texture"]["frame"].clone()).map_err(|e| bad(command, e.to_string()))?;
    setup
        .surfaces
        .set(
            SavedSurface { target, source: source(id, command)?.into(), local_frame: transform(frame, &inverse, command)?, normal_strength: 1.0 },
            input.replace,
        )
        .map_err(|e| bad(command, e.to_string()))?;
    // Validate actual opaque target and precision limits before the one document mutation.
    let scene = crate::product_scene::extract(s)?;
    let resolved = resolve(s, &scene, &setup.surfaces, command)?;
    super::textures::validate_targets(&scene, &resolved, command)?;
    publish(s, input.name, setup, command)
}
pub(super) fn unbind(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.library.surface.unbind";
    let input: SurfaceUnbindParams = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    input.target.validate().map_err(|e| bad(command, e.to_string()))?;
    let mut setup = super::patched_settings(s, &input.name, &input.set, command)?;
    setup.surfaces.remove(&input.target);
    publish(s, input.name, setup, command)
}

/// The image textures render setup `name` gives the design's bodies, for an export: one plan per
/// binding, with its maps' bytes. A binding that cannot be read (its target gone, a built-in
/// surface this build lacks or that changed, an image missing) is left out with a warning.
pub(in crate::cmd) fn export_plans(s: &Session, name: &str, command: &str, warnings: &mut Vec<String>) -> Result<Vec<solidcraft_io::SurfacePlan>> {
    use solidcraft_render::texture_input::MapRole;
    let setup = s.doc.render.setups.get(name).ok_or_else(|| bad(command, format!("there is no render setup named `{name}`")))?;
    if setup.surfaces.is_empty() {
        return Ok(Vec::new());
    }
    // A design whose bodies cannot be named for the setup (too many placed bodies for the
    // renderer, say) is still exported: its images are left out, with why.
    let identities = match targets(s, command) {
        Ok(identities) => identities,
        Err(e) => {
            warnings.push(format!("render setup `{name}`: its images are left out: {e}"));
            return Ok(Vec::new());
        }
    };
    let maximum = solidcraft_doc::render_texture_resources::MAX_TEXTURE_BYTES as u64;
    // Each image read once and shared by every binding that uses it.
    let mut read: BTreeMap<String, Arc<[u8]>> = BTreeMap::new();
    let mut map = |sha: String, bytes: &dyn Fn() -> std::result::Result<Vec<u8>, String>| -> std::result::Result<solidcraft_io::SurfaceMap, String> {
        let bytes = match read.get(&sha) {
            Some(b) => b.clone(),
            None => {
                let b: Arc<[u8]> = Arc::from(bytes()?);
                read.insert(sha.clone(), b.clone());
                b
            }
        };
        Ok(solidcraft_io::SurfaceMap { sha256: sha, bytes })
    };
    let mut plans = Vec::new();
    for binding in setup.surfaces.iter() {
        let Some(world) = identities.get(&binding.target) else {
            warnings.push(format!("render setup `{name}`: a surface's body is gone or changed: left out"));
            continue;
        };
        let mut plan = solidcraft_io::SurfacePlan {
            setup: name.to_string(),
            definition: binding.target.definition.clone(),
            component: binding.target.component,
            path: binding.target.occurrence_path.clone(),
            world: world.clone(),
            origin: binding.local_frame.origin,
            u: binding.local_frame.u,
            v: binding.local_frame.v,
            flip_v: true,
            normal_strength: binding.normal_strength,
            base_color: None,
            normal: None,
            roughness: None,
            metallic: None,
        };
        let outcome = match &binding.source {
            SurfaceSource::Builtin(builtin) => (|| {
                if !cfg!(feature = "bundled-render-library") {
                    return Err(format!("this build has no built-in surface images, so `{}` is written without them", builtin.id));
                }
                if source(&builtin.id, command).ok().as_ref() != Some(builtin) {
                    return Err(format!("built-in surface `{}` differs from the installed one: written without its images", builtin.id));
                }
                let bytes = |role: MapRole, file: &str| {
                    super::library::texture_bytes(&format!("{}/{file}", builtin.id), role, maximum, command).map_err(|e| e.to_string())
                };
                plan.base_color = Some(map(builtin.base_color_sha256.to_string(), &|| bytes(MapRole::BaseColor, "base_color"))?);
                plan.normal = Some(map(builtin.normal_sha256.to_string(), &|| bytes(MapRole::NormalOpenGl, "normal_open_gl"))?);
                plan.roughness = Some(map(builtin.roughness_sha256.to_string(), &|| bytes(MapRole::Roughness, "roughness"))?);
                plan.metallic = Some(map(builtin.metallic_sha256.to_string(), &|| bytes(MapRole::Metallic, "metallic"))?);
                Ok(())
            })(),
            SurfaceSource::Embedded(embedded) => (|| {
                plan.flip_v = embedded.flip_v;
                let resources = &s.doc.render.texture_resources;
                let mut bytes = |id: Option<HdrId>| -> std::result::Result<Option<solidcraft_io::SurfaceMap>, String> {
                    id.map(|id| map(id.to_string(), &|| resources.required(&id).map(|r| r.bytes().to_vec()).map_err(|e| e.to_string()))).transpose()
                };
                plan.base_color = bytes(embedded.base_color)?;
                plan.normal = bytes(embedded.normal_open_gl)?;
                plan.roughness = bytes(embedded.roughness)?;
                plan.metallic = bytes(embedded.metallic)?;
                Ok(())
            })(),
        };
        match outcome {
            Ok(()) => plans.push(plan),
            Err(e) => warnings.push(format!("{world}: render setup `{name}`: {e}")),
        }
    }
    Ok(plans)
}

/// The optics render setup `name` gives the design's bodies, for an export: one plan per binding.
/// What the export cannot write (a glass model, subsurface scattering, transmissive shadows) is a
/// warning naming the body; a binding whose body is gone is left out with a warning.
pub(in crate::cmd) fn optics_plans(s: &Session, name: &str, command: &str, warnings: &mut Vec<String>) -> Result<Vec<solidcraft_io::OpticsPlan>> {
    let Some(setup) = s.doc.render.setups.get(name) else { return Ok(Vec::new()) };
    if setup.optics.is_empty() {
        return Ok(Vec::new());
    }
    let identities = match targets(s, command) {
        Ok(identities) => identities,
        Err(e) => {
            warnings.push(format!("render setup `{name}`: its optics are not written: {e}"));
            return Ok(Vec::new());
        }
    };
    let mut plans = Vec::new();
    for binding in setup.optics.iter() {
        let Some(world) = identities.get(&binding.target) else {
            warnings.push(format!("render setup `{name}`: a body given optics is gone or changed: left out"));
            continue;
        };
        let optics = &binding.optics;
        for (unwritten, what) in [
            (optics.model.is_some(), "its glass model"),
            (optics.subsurface.is_some(), "its subsurface scattering"),
            (optics.shadow_transmission, "its transmissive shadows"),
        ] {
            if unwritten {
                warnings.push(format!("{world}: render setup `{name}`: {what} has no OpenUSD form here: left out"));
            }
        }
        plans.push(solidcraft_io::OpticsPlan {
            setup: name.to_string(),
            definition: binding.target.definition.clone(),
            component: binding.target.component,
            path: binding.target.occurrence_path.clone(),
            world: world.clone(),
            ior: optics.ior,
            absorption_per_mm: optics.absorption_per_mm,
            thin_sheet: optics.boundary == solidcraft_doc::render_optics::OpticalBoundary::ThinSheet,
        });
    }
    Ok(plans)
}
