//! Render capability discovery and persistent intent. No rendering or host probing.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_doc::render_settings::{RenderSettings, RenderSetup};

use super::CommandSpec;
use crate::params::bad;
use crate::{Result, Session};

mod clear_coat;
mod effects;
mod environment;
mod jobs;
mod library;
mod library_data;
#[cfg(test)]
mod light_union_tests;
mod lights;
mod optical_bindings;
mod physical_library;
mod resources;
mod shots;
mod staging;
mod studio_presets;
mod surface_bindings;
mod textures;
mod transmission;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("render.environment.set", "Set Render Environment", crate::checked!("render.environment.set", environment::set)).icon("settings").doc_only()
        .contract(&environment::CONTRACT)
        .output(crate::schema::output_schema_for::<crate::schema::render::EnvironmentResult>)
        .params("name; set?: numerical setup patch; rotation_deg?: finite degrees within +/-1000000, wrapped to [-180,180); intensity?: finite linear intensity 0..64 CPU or 0..100 GPU. Edits the existing HDR, sky or constant environment in one undo, without selecting a source, moving the camera or starting a render. Absent environments and raster refuse."),
    CommandSpec::new("render.lights.set", "Set Render Light", crate::checked!("render.lights.set", lights::set))
        .icon("render_scene").doc_only()
        .contract(&crate::schema::TypedContract::<crate::schema::render_lights::EditParams>::new())
        .output(crate::schema::output_schema_for::<crate::schema::render_lights::EditResult>)
        .params("name; id except add; settings?: full unsaved RenderSetup; expected_revision?:u64; set for set/add: kind directional/point/spot/area, intensity_ev, temperature_k, softness, azimuth_deg, elevation_deg, distance_mm, size_mm, aim_at, width_mm, height_mm, position_mm, direction, edge_u_mm, edge_v_mm, radiance, colour, reference_distance_mm, minimum_distance_mm. Validated finite light edit in one undo. Existing family caps and backend support apply; refusal is atomic. Names remain defaults."),
    CommandSpec::new("render.lights.add", "Add Light", crate::checked!("render.lights.add", lights::add))
        .icon("render_scene").doc_only()
        .contract(&crate::schema::TypedContract::<crate::schema::render_lights::EditParams>::new())
        .output(crate::schema::output_schema_for::<crate::schema::render_lights::EditResult>)
        .params("name; id except add; settings?: full unsaved RenderSetup; expected_revision?:u64; set for set/add: kind directional/point/spot/area, intensity_ev, temperature_k, softness, azimuth_deg, elevation_deg, distance_mm, size_mm, aim_at, width_mm, height_mm, position_mm, direction, edge_u_mm, edge_v_mm, radiance, colour, reference_distance_mm, minimum_distance_mm. Validated finite light edit in one undo. Existing family caps and backend support apply; refusal is atomic. Names remain defaults."),
    CommandSpec::new("render.lights.duplicate", "Duplicate Render Light", crate::checked!("render.lights.duplicate", lights::duplicate))
        .icon("render_scene").doc_only()
        .contract(&crate::schema::TypedContract::<crate::schema::render_lights::EditParams>::new())
        .output(crate::schema::output_schema_for::<crate::schema::render_lights::EditResult>)
        .params("name; id except add; settings?: full unsaved RenderSetup; expected_revision?:u64; set for set/add: kind directional/point/spot/area, intensity_ev, temperature_k, softness, azimuth_deg, elevation_deg, distance_mm, size_mm, aim_at, width_mm, height_mm, position_mm, direction, edge_u_mm, edge_v_mm, radiance, colour, reference_distance_mm, minimum_distance_mm. Validated finite light edit in one undo. Existing family caps and backend support apply; refusal is atomic. Names remain defaults."),
    CommandSpec::new("render.lights.remove", "Remove Render Light", crate::checked!("render.lights.remove", lights::remove))
        .icon("render_scene").doc_only()
        .contract(&crate::schema::TypedContract::<crate::schema::render_lights::EditParams>::new())
        .output(crate::schema::output_schema_for::<crate::schema::render_lights::EditResult>)
        .params("name; id except add; settings?: full unsaved RenderSetup; expected_revision?:u64; set for set/add: kind directional/point/spot/area, intensity_ev, temperature_k, softness, azimuth_deg, elevation_deg, distance_mm, size_mm, aim_at, width_mm, height_mm, position_mm, direction, edge_u_mm, edge_v_mm, radiance, colour, reference_distance_mm, minimum_distance_mm. Validated finite light edit in one undo. Existing family caps and backend support apply; refusal is atomic. Names remain defaults."),
    CommandSpec::new("render.lights.list", "List Render Lights", crate::checked!("render.lights.list", lights::list))
        .icon("list").noundo().doc_only()
        .contract(&crate::schema::TypedContract::<crate::schema::render_lights::ListParams>::new())
        .output(crate::schema::output_schema_for::<crate::schema::render_lights::ListResult>)
        .params("setup?: saved setup name OR settings?: complete RenderSetup snapshot; neither uses defaults. Returns {lights:[{id,kind,name,name_key,number,implicit,intensity,colour,...spatial fields}]}. IDs are kind:zero-based-source-slot, stable across numerical edits but not family changes/reordering/removal/rig replacement; point and spot share punctual slots. Includes implicit default/preset lights. GPU RGB radiance is factored as colour * intensity; area edges are full world-mm vectors. Read-only, no host, camera change, save, render, model evaluation or undo. See docs/render-settings.md."),
    CommandSpec::new("render.aging.describe","Layered Surface Aging",effects::aging).icon("appearance").noundo().doc_only()
        .params("Empty object. Read-only original Rust/patina/dust/grime/scratch/chip/fading presets and exact GPU aging contract. UI activation opens the shared Aging tab; edits remain a saved render draft, never engineering geometry or material-property mutation."),
    CommandSpec::new("render.effects.describe","Lens and Creative Effects",effects::presentation).icon("camera").noundo().doc_only()
        .params("Empty object. Read-only original photographic/creative filter defaults and constraints. UI activation opens shared Effects controls. Saved finishing and exact-master regrading preserve raw EXR bytes; no physical lens or branded-film calibration is implied."),
    CommandSpec::new("render.optics.bind","Save Optical Binding",optical_bindings::bind).icon("appearance")
        .params("name: GPU setup; set?: complete numerical setup patch; body_index and body_name: exact captured target; expected_revision; optics:{ior,absorption_per_mm:[r,g,b],boundary:closed_mesh|closed_box|thin_sheet,thickness_mm,shadow_transmission,subsurface?:{scattering_per_mm,anisotropy}}; replace?:false. Stores stable definition/feature/occurrence identity in one undo without modifying engineering geometry/materials. Nested media require separate job.options.nested_media and producer certification. Unresolved or incompatible targets refuse atomically."),
    CommandSpec::new("render.optics.unbind","Remove Optical Binding",optical_bindings::unbind).icon("appearance").doc_only()
        .params("name: saved setup; target: exact saved definition/feature/component/occurrence_path; expected_revision. Removes only that optical assignment in one undo without a device or file access."),
    CommandSpec::new("render.result.regrade", "Regrade Linear Master", jobs::regrade).icon("render_image").noundo().doc_only()
        .params("job?: exact completed JobId (default selected result); presentation: {exposure_ev?,tone_map?,finishing?,backdrop?,background?,transparent_background?}. Queues cancellable nondestructive PNG presentation from that retained native FLOAT32 master, not from PNG. Captured geometry/camera/lighting and source EXR remain unchanged; no path tracing, denoising, external EXR import or automatic file writes. Returns a distinct job with source identity and original-master digest. Existing frame retention and export rules still apply."),
    CommandSpec::new("render.camera.set", "Set Render Camera", set_camera).icon("camera").noundo().doc_only()
        .params("camera: complete Render Camera snapshot (target, yaw, pitch, distance, fov; roll defaults to 0 and up to z). Validates the same finite mm/radian bounds as saved render intent, preserves explicit roll/up and changes only transient view state. No model evaluation, setup save, renderer, dirty flag or undo step. Design view.apply retains its separate up-axis policy."),
    CommandSpec::new("render.camera.get", "Inspect Render Camera", get_camera).icon("camera").noundo()
        .params("No parameters. Returns the explicitly defined current view camera, or null if the session has no camera override. No camera is fitted, no job is started and no saved setup or engineering document is changed."),
CommandSpec::new("render.camera.fit", "Frame Render Subject", fit_camera).icon("camera").noundo()
    .params("aspect?: the picture's width over its height, 0.000001 to 1000000 (default 1). Fits the current camera to the visible product bounds through the shared Camera::fit contract, moving back for a picture taller than wide so its sides hold the product too; changes only view state, not a saved setup, rendering job, stage or engineering geometry."),
CommandSpec::new("render.preset.resolve", "Review Native Studio", studio_presets::resolve).icon("render_scene").noundo()
    .params("id: NATIVE-STUDIO-001..025; name; set?: typed GPU setup patch; expected_revision?:u64. Resolves original built-in HDR/rig/passive ground with conservative geometry clearance, returns all replacements and preserved fields without mutation. Unbound SCENE-* concepts are not accepted."),
CommandSpec::new("render.preset.apply", "Apply Native Studio", studio_presets::apply).icon("render_scene")
    .params("Same reviewed input as render.preset.resolve, expected_revision required, replace_lighting_and_environment:true and replace_stage:true when an existing stage would change. Re-resolves the complete candidate and publishes one undo or no mutation; camera, optical bindings, geometry and engineering materials are preserved."),
CommandSpec::new("render.quality.prepare", "Review Fixed Sampling Budget", studio_presets::quality).icon("settings").noundo().doc_only()
    .params("id: Q-*; variant:fixed_samples_png; acknowledge_omissions:true. Read-only bounded GPU still-budget projection, not full adaptive/convergence/linear-master quality acceptance. Returns dimensions/samples/depth/seed/denoise and explicit omissions. Never switches executor or mutates a shot."),
CommandSpec::new("render.shot.duplicate", "Duplicate Render Shot", shots::duplicate).icon("copy").doc_only()
    .params("name, new_name, expected_revision:u64. Copies exact saved setup and resource references in one undo. Existing destinations refuse; captured jobs and camera navigation are unchanged."),
CommandSpec::new("render.shot.rename", "Rename Render Shot", shots::rename).icon("settings").doc_only()
    .params("name, new_name, expected_revision:u64. Renames exact saved setup in one undo; an existing destination or stale review refuses."),
CommandSpec::new("render.shot.remove", "Remove Render Shot", shots::remove).icon("delete").doc_only()
    .params("name, expected_revision:u64. Removes only that saved setup in one undo; retains resources, geometry and captured results."),
CommandSpec::new("render.library.preview", "Inspect Original Surface Map", library::preview).icon("appearance").noundo().doc_only()
    .params("id: SURF-*. Returns bounded hash-verified original base-color PNG bytes and source-map fidelity label, not a rendered material thumbnail. No external paths, rendering or document edits."),
CommandSpec::new("render.library.physical.prepare", "Prepare Physical Reference", physical_library::prepare).icon("inspect").noundo().doc_only()
    .params("id: named built-in physical profile, name: proposed new material label, acknowledge_reference:true. Read-only loss-aware creation plan using native ValidatedCard admission and one point density; returns source receipts, extended fields and omissions. Does not execute material.create, assign bodies or infer effective-part physics."),
CommandSpec::new("render.stage.ground", "Save Render Ground", staging::ground).icon("render_scene")
    .params("name: typed raytrace setup, set?: setup patch, margin_fraction?:0.25, gap_mm?:0.1, reflectance_linear?:[0.18,0.18,0.18], replace_stage?:false. Fits one finite passive ground below the visible product once and saves its physical transforms in one undo. Product AABB separation is rechecked on every render."),
CommandSpec::new("render.stage.backdrop", "Save Physical Render Background", staging::backdrop).icon("render_scene").doc_only()
    .params("name: typed raytrace setup, set?: setup patch, kind: floor_wall or cyclorama, reflectance_linear?: [0.18,0.18,0.18], expected_revision: reviewed revision, replace_stage?:false. Fits a finite floor/wall or a 24-strip polygonal cyclorama behind the visible subject using the saved camera azimuth; world +Z up and a minimum one-mm stage envelope. Preserves subject geometry, lighting, output, pose and engineering materials; one undo and rechecked product separation. Not an infinite/analytically smooth/terrain background."),
CommandSpec::new("render.stage.clear", "Remove Render Stage", staging::clear).icon("render_scene").doc_only()
    .params("name: setup, set?: setup patch. Removes saved render-only panels atomically; CAD geometry and lighting are unchanged."),
CommandSpec::new("render.library.surface.bind", "Save Surface Binding", surface_bindings::bind).icon("appearance")
    .params("name: GPU setup; set?: setup patch; surface: same input as render.library.surface.prepare; replace?: false. Pins exact source hashes and definition/feature/occurrence identity with local-mm mapping in one undo. No paths, mesh ordinals or physical material changes."),
CommandSpec::new("render.library.surface.unbind", "Remove Surface Binding", surface_bindings::unbind).icon("appearance").doc_only()
    .params("name: setup, target: saved definition/feature/component/occurrence_path, set?: setup patch. Removes only that explicit binding in one undo; no device or current geometry required."),
    CommandSpec::new("render.result.pin", "Retain Displayed Render Result", jobs::pin).icon("inspect").noundo().doc_only()
        .params("job?: completed JobId (default selected result), or null to clear selection. Selects within the eight retained completed results; oldest is evicted first. Preserves exact export bytes and original host memory accounting. No design/undo change; preview or foreign jobs refuse."),
CommandSpec::new("render.library.targets", "Find Render Library Targets", library::targets).icon("list").noundo()
    .params("query?: exact-name substring; offset?:0; limit?:128 (1..128). Returns a bounded page of captured body ordinals/names, revision and scene bounds. Settles the model through ordinary command handling; never returns a large mesh payload."),
CommandSpec::new("render.library.list", "Browse Render Library", library::list).icon("list").noundo().doc_only()
    .params("kind?: surfaces, selene_appearances, hdr, rigs, scenes, materials, lighting, environments, quality, native_scenes, selene_shots or capabilities; query?: bounded search; availability?: all/usable/specification/blocked; offset?:0; limit?:48 (1..128). Returns source status and explicit implemented projections separately. No device, file IO or design change."),
CommandSpec::new("render.library.describe", "Inspect Render Library Resource", library::describe).icon("inspect").noundo().doc_only()
    .params("kind, id: exact authored resource. Returns original specification, runtime projection variants and explicit omissions. A concept is not an executable preset."),
CommandSpec::new("render.library.appearance.apply", "Apply Library Opaque Appearance", library::apply_appearance).icon("appearance")
    .params("id: SURF-* or SELENE-AP-*; variant: uniform_opaque (required); bodies: 1..256 exact body names. Applies the original scalar baseline through ordinary appearance assignment in one undo, preserving physical properties. Spatial maps and advanced target effects are explicitly excluded."),
CommandSpec::new("render.library.environment.apply", "Apply Built-in HDR Environment", library::apply_environment).icon("settings").doc_only()
    .params("id: HDR-*; name: saved setup; set?: typed CPU/GPU setup patch; rotation_rad?: authored rotation (native origin offset is explicit in the reply); intensity?:1; samples?:16 for CPU; replace_environment?:false. Hash-verifies and embeds original bytes, saves one atomic undo without external files or a render device. An HDR-owned sun cannot duplicate an analytic sun."),
CommandSpec::new("render.library.surface.prepare", "Prepare Authored Surface Maps", library::prepare_surface).icon("appearance").noundo()
    .params("id: SURF-*; body_index and body_name from render.scene; origin?:[0,0,0]; plane?:xy/yz/zx; scale?:1. Returns an explicit GPU planar map binding with source hashes, original millimetre scale, image-up normal orientation and absolute base-color semantics. No document change; height/displacement and true anisotropy are not included."),
CommandSpec::new("render.library.studio.prepare", "Prepare Authored Studio Lights", library::prepare_studio).icon("inspect").noundo().doc_only()
    .params("id: RIG-*; variant: complete for emitter-only rigs, emitters_only for rigs with passive cards; center_mm and subject_extent_mm required. Converts exact rectangular half-edges and solar direction/radius to GPU lighting. Blocked calibration and external-sun rigs refuse. Returns a geometry-clearance review requirement; does not create cards or modify the scene."),
CommandSpec::new("render.library.studio.apply", "Apply Authored Studio Lights", library::apply_studio).icon("settings")
    .params("rig: render.library.studio.prepare parameters; name; set?: typed GPU setup patch; replace_lighting_and_environment:true required. Saves the explicit rig projection in one undo, removing prior lighting/environment only with that confirmation. Subject clearance still needs review; passive cards are never silently omitted."),
CommandSpec::new("render.hdr.inspect", "Inspect HDR Source", resources::inspect).icon("inspect").noundo().doc_only()
    .params("path: scoped Radiance HDR file; validates the complete bounded source and returns sha256, width, height, encoded_bytes and primaries without saving or rendering"),
CommandSpec::new("render.settings.capture_hdr", "Save Render Settings with HDR", resources::capture_settings).icon("settings").doc_only()
    .params("name, set: typed CPU/GPU setup patch with hdr/environment unset; path: scoped HDR; expected_sha256?: digest returned by render.hdr.inspect (changed sources refuse); rotation_rad?:0, intensity?:1; samples?:1/4/16/64 for CPU only (default16); assume_linear_srgb?:false for GPU only. Captures immutable bytes and saves resource plus setup atomically in one undo, without a host; returns name/setup/resource metadata, never source bytes"),
CommandSpec::new("render.preflight", "Check Render Request", jobs::preflight).icon("inspect").noundo().doc_only()
    .params("setup?: saved name OR settings?: full RenderSetup. A typed job selects raster/cpu_raytrace/gpu_path and its numerical options plus embedded HDR before dispatch; only replace and transient studio_weights may accompany it. Without job, the legacy runtime controls below remain available. See docs/render-settings.md. studio_weights?: three finite 0..1 visibility weights for raster or implicit GPU studio sources only; never saved. integrator?: raster (default) or raytrace; progressive?: false (registered GPU path, CPU integrator/LED options are mutually exclusive); samples?: 1..4096; max_depth?: 1..16; seed?: u32 for progressive; raytrace?: bounded runtime progressive/primary_samples/lights/punctual_lights/area_lights/area_samples/seed/exposure_ev/shadows/reflections/anisotropic_faces/emissions/surface_effects/dielectrics/environment options, only with raytrace. CPU raytrace.budget?: compact (legacy default), studio or workstation; see docs/render-cpu-resource-profiles.md for explicit image/geometry/memory/work limits. CPU raytrace.progressive?: {samples_per_pixel:1..profile limit, seed:u32}; requires primary_samples:1 and selected reflections.samples:1; no area/LED, anisotropy or dielectrics in this initial mode. See docs/render-cpu-progressive.md; environment?: {path: scoped HDR, expected_sha256?: inspected digest, rotation_radians: 0, intensity: 1, samples: 16} OR {resource: saved HDR digest, rotation_radians, intensity, samples}; embedded resources cannot combine with path/hash; explicit sRGB/D65 HDR primaries required. Opaque CPU preview with selected mirror/rough-specular reflections and certified planar anisotropy. HDR with rough reflections or anisotropy requires fully metallic selected bodies and all effective faces; unsupported combinations refuse. No MIS or GI. CPU runtime dielectrics support only certified thin_sheet/closed_box boundaries with explicit IOR and Beer absorption; see docs/render-raytrace-dielectrics.md. CPU emissions are visible radiance only; surface_effects change roughness/shading normals without geometry changes. led_strip?: continuous world-mm LED rectangle recipe; led_samples?: 1/4/16/64 for the whole strip, default16. At most8directional+8punctual+4rectangle/LED sources; shared area N. See docs/render-light-union.md. transparent_background?: false; replace?: false. With progressive:true, raytrace instead accepts GPU dielectrics/clear_coats/textures; environment instead accepts tagged hdr/embedded_hdr/sky with rotation_rad/intensity; file hdr supports expected_sha256, embedded_hdr uses a document resource digest. See docs/render-hdr-resources.md. GPU lens, studio lighting, tone_map, denoise, bounded finishing/backdrop and linear_exr retention are also available. GPU adaptive?: {pilot_samples:16..256,min_samples:16..samples,relative_error:0..1,absolute_error:0.000001..1000000,coverage_error:0.001..0.25} allocates independent production budgets; pilots are discarded, not a certified error guarantee. transmission_samples?:1|2|4 stratifies the first Fresnel branch with extra path cost. volume?:{min_mm:[x,y,z],max_mm:[x,y,z],extinction_per_mm:0..1000,scattering_albedo:[r,g,b],anisotropy:-0.95..0.95} is one finite homogeneous world-mm medium; active media require explicit lighting/environment and refuse glass, denoise and multiple transmission strata. These fields also persist in GPU job.options. See docs/render-materials/TRANSPORT-MASTER-BATCH-2026-10-23.md; see docs/render-transmission.md, render-environment.md, render-runtime-textures.md, render-thin-lens.md, render-studio-lighting.md and render-denoise.md. Optional CPU raytracing requires a raytrace-cpu build and native host, checked before scene capture. Stale geometry is rejected. See docs/render-jobs.md"),
CommandSpec::new("render.start", "Render Image", jobs::start).key("R").noundo().doc_only().at("RENDER", "IMAGE").icon("render_image")
    .params("Same request as render.preflight. Captures an immutable product scene and queues a bounded image job. Returns its session-scoped job identity, captured scene_generation and progress. Does not change the design. See docs/render-jobs.md"),
CommandSpec::new("render.status", "Render Progress", jobs::status).icon("list").noundo().doc_only()
    .params("job?: {session, serial, generation}. Non-blocking progress, errors and captured frame metadata with queue:{running,waiting,job,shot,error}. Without job returns queue only. Advances an explicitly started render.batch through render.start as jobs finish; never pumps render work. Preview is separate from the exportable final image."),
CommandSpec::new("render.cancel", "Cancel Render", jobs::cancel).icon("delete").noundo().doc_only()
    .params("job?: {session, serial, generation}, default the active render.batch job. Requests cooperative cancellation and drops its waiting shots; completed jobs remain completed. Late cancelled frames cannot be published."),
CommandSpec::new("render.result.copy", "Copy Render Image", jobs::copy).icon("copy").noundo().doc_only()
    .params("job?: completed JobId (default selected result). Validates the retained PNG for app clipboard copying (max 4096 per side, 4194304 pixels); headless returns frame metadata only. Never copies a provisional image or starts a job."),
CommandSpec::new("render.export", "Export Render Image", jobs::export).icon("export").noundo().doc_only()
    .params("job?: {session, serial, generation} (default selected result), path: destination matching format?: png (default) or exr, overwrite?: false. EXR requires an opt-in raw linear master retained by that completed GPU job; never converts PNG pixels. Exports a retained completed image atomically; an existing destination is preserved on failure. See docs/render-jobs.md"),
CommandSpec::new("render.backends", "Render Backends", backends).icon("list")
    .noundo()
    .doc_only()
    .params("No parameters. Returns {schema_version: 1, default_backend: builtin, backends: [{id, label, availability, executors: [{id, availability, profiles, limits, scratch}]}]}. Metadata only; does not start rendering or probe a GPU. See docs/render-backends.md"),
CommandSpec::new("render.settings.get", "Render Settings", get_settings).icon("settings").noundo().doc_only()
    .params("name?: saved setup. Without name returns {setups, defaults}; with name returns {name, setup}. Does not evaluate the model or apply a camera. See docs/render-settings.md"),
CommandSpec::new("render.settings.set", "Save Render Settings", set_settings).icon("settings").doc_only()
    .params("name: setup name, set: object of fields to replace (backend, mode, camera, width, height, format, exposure_ev, environment_rotation, environment_intensity, background, job, surfaces, stage). job?: typed active raster/cpu_raytrace/gpu_path configuration with optional validated embedded HDR reference; omitted preserves, null clears, object replaces the complete job. Creates from defaults and saves without a host, validates atomically, one undo step; identical edits are no-ops. See docs/render-settings.md"),
CommandSpec::new("render.scene", "Inspect Render Scene", scene).noundo().at("RENDER", "SCENE").icon("render_scene")
    .params("No parameters. Authoritative product-scene metadata, without modelling overlays or mesh payloads. Explicitly settles stale geometry; does not render an image. See docs/render-workspace.md"),
CommandSpec::new("render.settings.apply", "Apply Render Camera", apply_settings).icon("camera").noundo().doc_only()
    .params("name: saved setup. Applies its existing Camera snapshot including roll/up as view state, without modifying the document or starting a render."),
];

fn set_camera(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "render.camera.set";
    if !params.as_object().is_some_and(|p| p.len() == 1 && p.contains_key("camera")) {
        return Err(bad(cmd, "expected exactly one camera snapshot"));
    }
    // Reuse strict saved-camera decoding and its numeric limits, without saving a setup.
    let setup: RenderSetup = serde_json::from_value(params.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    setup.validate().map_err(|e| bad(cmd, e.to_string()))?;
    s.camera = Some(setup.camera);
    Ok(json!({"camera":setup.camera,"document_changed":false}))
}

fn get_camera(s: &mut Session, params: &Value) -> Result<Value> {
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad("render.camera.get", "expected an empty parameter object"));
    }
    Ok(json!({"camera":s.camera,"explicit_view":s.camera.is_some(),"document_changed":false}))
}

fn fit_camera(s: &mut Session, params: &Value) -> Result<Value> {
    let aspect = match params.as_object() {
        Some(p) if p.is_empty() => 1.0,
        Some(p) if p.len() == 1 && p.contains_key("aspect") => match p["aspect"].as_f64() {
            // The range a preview frame can show (`frame_rect` in the Render workspace).
            Some(a) if (1e-6..=1e6).contains(&a) => a,
            _ => return Err(bad("render.camera.fit", "aspect must be a number from 0.000001 to 1000000: the picture's width over its height")),
        },
        _ => return Err(bad("render.camera.fit", "expected an empty parameter object or {aspect}")),
    };
    let scene = crate::product_scene::extract(s)?;
    let mut camera = s.camera.unwrap_or_default();
    // Publish only a valid fitted camera; the previous one stays otherwise.
    if !camera.fit_aspect(&scene.bounds, aspect) && !scene.bounds.is_empty() {
        return Err(bad("render.camera.fit", "the product cannot be framed at this aspect: the fitted camera is out of range"));
    }
    s.camera = Some(camera);
    Ok(json!({"camera":camera,"empty":scene.bounds.is_empty(),"document_changed":false}))
}

fn scene(s: &mut Session, params: &Value) -> Result<Value> {
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad("render.scene", "expected an empty parameter object"));
    }
    let scene = crate::product_scene::extract(s)?;
    let mut stable = std::collections::BTreeMap::new();
    for (target, name) in surface_bindings::targets(s, "render.scene")? {
        if stable.insert(name, target).is_some() {
            return Err(bad("render.scene", "ambiguous stable occurrence name"));
        }
    }
    let bodies: Vec<_> = scene
        .meshes
        .iter()
        .enumerate()
        .map(|(ordinal,m)| json!({"ordinal":ordinal,"name":m.name,"stable_target":stable.get(&m.name),"triangles":m.mesh.triangles.len(),"color":m.color,"opacity":m.opacity,"face_appearances":m.faces.len(),"bounds":{"min":m.mesh.bounds().min,"max":m.mesh.bounds().max}}))
        .collect();
    Ok(
        json!({"bodies":bodies,"bounds":if scene.bounds.is_empty() { Value::Null } else { json!({"min":scene.bounds.min,"max":scene.bounds.max}) },"overlays":false}),
    )
}

fn apply_settings(s: &mut Session, params: &Value) -> Result<Value> {
    let request: GetSettings = serde_json::from_value(params.clone()).map_err(|e| bad("render.settings.apply", e.to_string()))?;
    let name = request.name.ok_or_else(|| bad("render.settings.apply", "`name` is required"))?;
    RenderSettings::validate_name(&name).map_err(|e| bad("render.settings.apply", e.to_string()))?;
    let setup = s.doc.render.setups.get(&name).ok_or_else(|| bad("render.settings.apply", "no saved render setup with that name"))?;
    setup.validate().map_err(|e| bad("render.settings.apply", e.to_string()))?;
    s.camera = Some(setup.camera);
    Ok(json!({"name":name,"camera":setup.camera}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetSettings {
    name: Option<String>,
}

fn get_settings(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "render.settings.get";
    let request: GetSettings = serde_json::from_value(params.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    match request.name {
        None => Ok(json!({"setups": s.doc.render.setups, "defaults": RenderSetup::default()})),
        Some(name) => {
            RenderSettings::validate_name(&name).map_err(|e| bad(cmd, e.to_string()))?;
            let setup = s.doc.render.setups.get(&name).ok_or_else(|| bad(cmd, "no saved render setup with that name"))?;
            Ok(json!({"name": name, "setup": setup}))
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetSettings {
    name: String,
    set: Value,
}

fn set_settings(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "render.settings.set";
    let request: SetSettings = serde_json::from_value(params.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    let setup = patched_settings(s, &request.name, &request.set, cmd)?;
    let reply = json!({"name": request.name, "setup": setup});
    if s.doc.render.setups.get(&request.name) == Some(&setup) {
        return Ok(reply);
    }
    let mut settings = (*s.doc.render).clone();
    settings.setups.insert(request.name, setup);
    settings.validate().map_err(|e| bad(cmd, e.to_string()))?;
    s.doc_mut().render = Arc::new(settings);
    Ok(reply)
}

fn patched_settings(s: &Session, name: &str, set: &Value, cmd: &str) -> Result<RenderSetup> {
    RenderSettings::validate_name(name).map_err(|e| bad(cmd, e.to_string()))?;
    let patch = set.as_object().filter(|p| p.len() <= 13).ok_or_else(|| bad(cmd, "`set` must be an object with at most 13 fields"))?;
    let current = s.doc.render.setups.get(name).cloned().unwrap_or_default();
    let mut value = serde_json::to_value(&current).map_err(|e| bad(cmd, e.to_string()))?;
    let object = value.as_object_mut().ok_or_else(|| bad(cmd, "invalid render setup"))?;
    for (key, value) in patch {
        object.insert(key.clone(), value.clone());
    }
    let setup: RenderSetup = serde_json::from_value(value).map_err(|e| bad(cmd, e.to_string()))?;
    setup.validate().map_err(|e| bad(cmd, e.to_string()))?;
    Ok(setup)
}

fn backends(s: &mut Session, params: &Value) -> Result<Value> {
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad("render.backends", "expected an empty parameter object"));
    }
    Ok(json!({"schema_version": 1, "default_backend": "builtin", "backends": [s.render_backend()],
        "cpu_resource_profiles": solidcraft_render::job::CpuBudget::ALL.map(|budget| json!({"id":budget,"limits":budget.limits()}))}))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn assert_unchanged(s: &mut Session, params: Value) -> Result<Value> {
        let doc = s.doc.clone();
        let before = (s.is_dirty(), s.revision, s.evaluations, s.model.is_stale());
        let camera = s.camera;
        let undo = s.undo.clone();
        let redo = s.redo.clone();
        let result = s.execute("render.backends", &params);
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert_eq!(before, (s.is_dirty(), s.revision, s.evaluations, s.model.is_stale()));
        assert_eq!(camera, s.camera);
        for (before, after) in [(&undo, &s.undo), (&redo, &s.redo)] {
            assert_eq!(before.len(), after.len());
            for (a, b) in before.iter().zip(after.iter()) {
                assert!(Arc::ptr_eq(&a.doc, &b.doc));
                assert_eq!((&a.label, a.active_sketch, &a.pending_moves), (&b.label, b.active_sketch, &b.pending_moves));
            }
        }
        result
    }

    #[test]
    fn discovery_preserves_clean_dirty_and_undo_redo_state() {
        let mut s = Session::default();
        let r = assert_unchanged(&mut s, Value::Null).unwrap();
        assert_eq!(r["schema_version"], 1);
        assert_eq!(r["default_backend"], "builtin");
        assert_eq!(r["backends"].as_array().unwrap().len(), 1);
        let descriptor: crate::render::BackendDescriptor = serde_json::from_value(r["backends"][0].clone()).unwrap();
        assert_eq!(descriptor, crate::render::builtin_backend());
        s.execute("solid.box", &json!({"length": 10, "width": 20, "height": 5})).unwrap();
        s.execute("view.apply", &json!({"view": "top"})).unwrap();
        assert!(s.is_dirty());
        assert_unchanged(&mut s, json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(!s.redo.is_empty());
        assert_unchanged(&mut s, json!({})).unwrap();
        for p in [json!([]), json!(3), json!({"backend": "missing"}), json!({"width": "bad"})] {
            assert!(assert_unchanged(&mut s, p).is_err());
        }
    }

    #[test]
    fn cooperative_host_never_discovers_cpu_raytracing_or_accepts_its_reserved_viewport_id() {
        use solidcraft_render::job::CpuHost;
        let mut s = Session::default();
        s.set_render_job_host(Some(CpuHost::cooperative())).unwrap();
        let reply = assert_unchanged(&mut s, json!({})).unwrap();
        let backend: solidcraft_render::BackendDescriptor = serde_json::from_value(reply["backends"][0].clone()).unwrap();
        assert!(backend.executors.iter().any(|d| d.id.as_str() == "cpu-product"));
        assert!(!backend.executors.iter().any(|d| d.id.as_str() == "cpu-raytrace"));
        let mut viewport = CpuHost::cooperative().descriptor().unwrap();
        viewport.id = solidcraft_render::ExecutorId::new("cpu-raytrace").unwrap();
        viewport.profiles[0].operation = solidcraft_render::RenderOperation::Viewport;
        viewport.profiles[0].formats.clear();
        assert!(s.set_render_viewport(Some(viewport)).unwrap_err().to_string().contains("reserved"));
        assert_eq!(s.render_backend(), backend);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_cpu_discovery_matches_installed_features_and_preserves_raster_default() {
        use solidcraft_render::job::{CpuHost, Integrator};
        use solidcraft_render::{RenderFeature, RenderFormat, RenderMode, RenderOperation, RenderRequest};
        let host = CpuHost::for_platform().unwrap();
        // Query the host so the same test covers feature-off builds and workspace
        // feature unification without assuming the engine's own cfg is sufficient.
        let raytrace = host.validate_integrator(Integrator::Raytrace).is_ok();
        let mut s = Session::default();
        let doc = s.doc.clone();
        s.set_render_job_host(Some(host.clone())).unwrap();
        let reply = assert_unchanged(&mut s, json!({})).unwrap();
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert!(!s.is_dirty());
        assert!(s.undo.is_empty());
        let backend: solidcraft_render::BackendDescriptor = serde_json::from_value(reply["backends"][0].clone()).unwrap();
        let descriptors: Vec<_> = backend.executors.iter().filter(|d| d.id.as_str() == "cpu-raytrace").collect();
        assert_eq!(descriptors.len(), usize::from(raytrace));
        if let Some(descriptor) = descriptors.first() {
            assert_eq!(descriptor.availability, solidcraft_render::Availability::Available);
            assert_eq!(*descriptor, &host.descriptor_for(Integrator::Raytrace).unwrap());
            assert!(!descriptor.profiles.iter().any(|p| p.features.contains(&RenderFeature::Progressive)));
        }
        let request = RenderRequest {
            backend: None,
            mode: RenderMode::Preview,
            operation: RenderOperation::Image,
            width: 16,
            height: 16,
            format: Some(RenderFormat::Png),
            features: vec![RenderFeature::Pbr],
            policy: Default::default(),
        };
        assert_eq!(solidcraft_render::resolve_request(std::slice::from_ref(&backend), &request).unwrap().executor.as_str(), "cpu-product");
        assert_eq!(s.scratch().render_backend(), backend);
        s.set_render_job_host(Some(host)).unwrap();
        assert_eq!(s.render_backend(), backend, "reinstalling the same host cannot duplicate method descriptors");
        s.set_render_job_host(Some(CpuHost::cooperative())).unwrap();
        assert!(!s.render_backend().executors.iter().any(|d| d.id.as_str() == "cpu-raytrace"));
        s.set_render_job_host(None).unwrap();
        assert_eq!(s.render_backend(), solidcraft_render::builtin_backend());
        assert_unchanged(&mut s, json!({})).unwrap();
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert!(!s.is_dirty());
        assert!(s.undo.is_empty());
    }

    #[test]
    fn discovery_does_not_settle_a_live_drag_or_rebuild_geometry() {
        let mut s = Session::default();
        for (id, p) in [
            ("sketch.create", json!({"plane": "XY"})),
            ("sketch.rectangle.two_point", json!({"p0": [5, 5], "p1": [25, 20]})),
            ("sketch.dimension", json!({"entities": ["l1"], "value": 20})),
            ("sketch.dimension", json!({"entities": ["l2"], "value": 15})),
            ("sketch.finish", json!({})),
            ("solid.extrude", json!({"distance": 5})),
            ("sketch.edit", json!({"sketch": "Sketch1"})),
            ("sketch.drag", json!({"entity": "l1.start", "from": [5, 5], "to": [4, 5], "live": true})),
        ] {
            s.execute(id, &p).unwrap();
        }
        assert!(s.model.is_stale());
        s.evaluations = 0;
        let model = s.model.state();
        assert_unchanged(&mut s, json!({})).unwrap();
        assert_unchanged(&mut s, json!({"unexpected": true})).unwrap_err();
        assert!(Arc::ptr_eq(&model, &s.model.state()));
        assert_eq!(s.evaluations, 0);
    }
}

#[cfg(test)]
mod settings_tests;

#[cfg(test)]
mod job_tests;
