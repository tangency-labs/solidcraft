//! Original native studio compositions. Independent of unbound SCENE-* concepts.
//! Resolution is read-only; publication validates a complete candidate before one undo.
use crate::{Result, Session, params::bad};
use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_doc::{
    render_resources::GpuHdrEnvironment,
    render_settings::{RenderJob, RenderSettings, RenderSetup},
};
use solidcraft_render::{
    job::{AreaLights, StudioLighting},
    stage::{Stage, StagePanel},
};
use std::sync::Arc;

pub(super) fn entries() -> Value {
    json!([
        {"id":"NATIVE-STUDIO-001","name":"Neutral product studio","description":"Original side-softbox HDR and a fitted passive ground. Preserves the camera, all appearances and native geometry.","hdr":"HDR-001","intensity":1.0,"exposure_ev":-0.9,"ground":[0.32,0.34,0.38],"rig":null},
        {"id":"NATIVE-STUDIO-002","name":"Soft enveloping studio","description":"Original broad white HDR and a neutral ground for product inspection. No automatic appearance assignment.","hdr":"HDR-002","intensity":1.0,"exposure_ev":-2.0,"ground":[0.18,0.19,0.21],"rig":null},
        {"id":"NATIVE-STUDIO-003","name":"Electronics inspection studio","description":"Original electronics-bench HDR, neutral passive ground and explicit exposure. Retains board and component appearances.","hdr":"HDR-014","intensity":1.0,"exposure_ev":-1.0,"ground":[0.22,0.24,0.27],"rig":null},
        {"id":"NATIVE-STUDIO-004","name":"Three-light product studio","description":"Original RIG-002 with an explicit 1/16 creative radiance gain, HDR-002 fill and passive ground. This is not the unmodified reference calibration.","hdr":"HDR-002","intensity":0.12,"exposure_ev":-1.1,"ground":[0.32,0.34,0.38],"rig":"RIG-002"},
        {"id":"NATIVE-STUDIO-005","name":"Dark metal reflection studio","description":"Original black-strip HDR for readable metal reflections, with a dark diffuse ground. Preserves all product optics.","hdr":"HDR-003","intensity":1.0,"exposure_ev":-0.9,"ground":[0.025,0.03,0.04],"rig":null},
        {"id":"NATIVE-STUDIO-006","name":"Warm DIY workbench studio","description":"Original home-workbench window radiance and a warm diffuse ground. Not a textured workshop reconstruction.","hdr":"HDR-013","intensity":1.0,"exposure_ev":-1.0,"ground":[0.27,0.22,0.16],"rig":null},
        {"id":"NATIVE-STUDIO-007","name":"Spacecraft inspection bay","description":"Original clean-bay HDR and pale neutral ground. A terrestrial inspection composition, not lunar illumination or vacuum physics.","hdr":"HDR-023","intensity":1.0,"exposure_ev":-2.0,"ground":[0.42,0.44,0.48],"rig":null},
        {"id":"NATIVE-STUDIO-008","name":"Woodshop soft-window studio","description":"Original broad-window woodshop HDR with a warm neutral ground. Does not automatically assign a wood material or geometry.","hdr":"HDR-010","intensity":1.0,"exposure_ev":-1.0,"ground":[0.30,0.25,0.20],"rig":null},
        {"id":"NATIVE-STUDIO-009","name":"Aerospace hangar inspection","description":"Hangar skylight reflections for airframe hardware. This is a finite product stage, not a reconstructed hangar.","hdr":"HDR-004","intensity":1.0,"exposure_ev":-1.2,"ground":[0.25,0.28,0.32],"rig":null},
        {"id":"NATIVE-STUDIO-010","name":"Neutral comparison studio","description":"Neutral comparison field and an 18-percent diffuse ground. Relative authored radiance, not a colorimetric calibration.","hdr":"HDR-005","intensity":1.0,"exposure_ev":-1.0,"ground":[0.18,0.18,0.18],"rig":null},
        {"id":"NATIVE-STUDIO-011","name":"Warm tabletop presentation","description":"Warm table-window reflections and a muted warm ground for household and DIY objects.","hdr":"HDR-006","intensity":1.0,"exposure_ev":-1.2,"ground":[0.26,0.22,0.16],"rig":null},
        {"id":"NATIVE-STUDIO-012","name":"Automotive showroom ribbons","description":"Long showroom reflection bands with a dark passive ground. Paint optics and material assignments are preserved.","hdr":"HDR-007","intensity":1.0,"exposure_ev":-0.9,"ground":[0.04,0.05,0.07],"rig":null},
        {"id":"NATIVE-STUDIO-013","name":"Mechanical service bay","description":"Mechanical-bay window reflections for industrial assemblies and workshop hardware.","hdr":"HDR-008","intensity":1.0,"exposure_ev":-1.0,"ground":[0.21,0.23,0.25],"rig":null},
        {"id":"NATIVE-STUDIO-014","name":"CNC tooling overhead studio","description":"Overhead lane reflections for machined parts. Does not add manufacturing marks or simulate a cutting process.","hdr":"HDR-009","intensity":1.0,"exposure_ev":-1.1,"ground":[0.25,0.27,0.28],"rig":null},
        {"id":"NATIVE-STUDIO-015","name":"Garage doorway product studio","description":"Broad garage-doorway reflections and a neutral dark ground for automotive and DIY components.","hdr":"HDR-011","intensity":1.0,"exposure_ev":-1.0,"ground":[0.16,0.17,0.18],"rig":null},
        {"id":"NATIVE-STUDIO-016","name":"Makerspace worklight studio","description":"Authored worklight environment for printed fixtures, enclosures and tools. Retains the actual source geometry.","hdr":"HDR-012","intensity":1.0,"exposure_ev":-1.0,"ground":[0.27,0.26,0.23],"rig":null},
        {"id":"NATIVE-STUDIO-017","name":"Sun-free overcast inspection","description":"Diffuse overcast environment without a solar disc. No extra sun or atmospheric medium is introduced.","hdr":"HDR-015","intensity":1.0,"exposure_ev":-1.0,"ground":[0.3,0.32,0.34],"rig":null},
        {"id":"NATIVE-STUDIO-018","name":"Automotive turntable presentation","description":"Turntable-style reflections on a stationary finite ground. This composition does not animate the subject.","hdr":"HDR-016","intensity":1.0,"exposure_ev":-1.0,"ground":[0.045,0.05,0.06],"rig":null},
        {"id":"NATIVE-STUDIO-019","name":"Paint reflection-band review","description":"Authored inspection bands reveal the currently assigned surface. Flakes, spectral paint and coating thickness are not added.","hdr":"HDR-017","intensity":1.0,"exposure_ev":-1.0,"ground":[0.05,0.055,0.065],"rig":null},
        {"id":"NATIVE-STUDIO-020","name":"Assembly bench soft studio","description":"Soft assembly-bench lighting for mechanisms, bearings and electronic enclosures; engineering materials remain unchanged.","hdr":"HDR-018","intensity":1.0,"exposure_ev":-1.0,"ground":[0.24,0.25,0.26],"rig":null},
        {"id":"NATIVE-STUDIO-021","name":"Electronics rework reflection studio","description":"Offset strips for solder and component inspection. No physical PCB or solder geometry is synthesized.","hdr":"HDR-019","intensity":1.0,"exposure_ev":-1.0,"ground":[0.12,0.14,0.16],"rig":null},
        {"id":"NATIVE-STUDIO-022","name":"FDM surface inspection","description":"Neutral authored lighting for real printed-part surface detail. Layer geometry is not invented by this preset.","hdr":"HDR-020","intensity":1.0,"exposure_ev":-1.0,"ground":[0.28,0.29,0.3],"rig":null},
        {"id":"NATIVE-STUDIO-023","name":"Printed-part photography","description":"Broad photographic environment and a pale ground for printed housings and DIY products.","hdr":"HDR-021","intensity":1.0,"exposure_ev":-1.0,"ground":[0.45,0.46,0.48],"rig":null},
        {"id":"NATIVE-STUDIO-024","name":"Robot LED editorial studio","description":"Colored editorial environment for robotics. Does not assign emitters, diode photometry or a physical lunar atmosphere.","hdr":"HDR-022","intensity":1.0,"exposure_ev":-1.4,"ground":[0.025,0.027,0.035],"rig":null},
        {"id":"NATIVE-STUDIO-025","name":"Black-environment three-light studio","description":"Black environment with an explicit three-light rig and dark ground. Not solar or lunar illumination.","hdr":"HDR-024","intensity":1.0,"exposure_ev":-1.1,"ground":[0.08,0.08,0.09],"rig":"RIG-002"}
    ])
}
fn recipe(id: &str, command: &str) -> Result<Value> {
    entries()
        .as_array()
        .and_then(|items| items.iter().find(|item| item["id"].as_str() == Some(id)).cloned())
        .ok_or_else(|| bad(command, "unknown native studio identity; SCENE-* specifications are not executable compositions"))
}
fn patch() -> Value {
    json!({})
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    id: String,
    name: String,
    #[serde(default = "patch")]
    set: Value,
    #[serde(default)]
    expected_revision: Option<u64>,
    #[serde(default)]
    replace_lighting_and_environment: bool,
    #[serde(default)]
    replace_stage: bool,
}
struct Candidate {
    settings: RenderSettings,
    setup: RenderSetup,
    report: Value,
}
fn candidate(s: &Session, input: &Input, command: &str) -> Result<Candidate> {
    if input.expected_revision.is_some_and(|revision| revision != s.revision) {
        return Err(bad(command, "scene or saved settings changed after review; resolve the composition again"));
    }
    let source = recipe(&input.id, command)?;
    let mut setup = super::patched_settings(s, &input.name, &input.set, command)?;
    if !matches!(&setup.job, Some(RenderJob::GpuPath { .. })) {
        return Err(bad(command, "native studio compositions require an explicitly selected GPU path setup; no executor is substituted"));
    }
    let old = s.doc.render.setups.get(&input.name);
    let stage_replacement = !setup.stage.is_empty() || old.is_some_and(|saved| !saved.stage.is_empty());
    let mut product = crate::product_scene::extract(s)?;
    if product.bounds.is_empty() {
        return Err(bad(command, "a native studio requires a nonempty visible subject"));
    }
    let bounds = product.bounds;
    let size = bounds.max - bounds.min;
    let center = bounds.center();
    let extent = size.x.max(size.y).max(size.z);
    if !extent.is_finite() || !(0.001..=1e6).contains(&extent) {
        return Err(bad(command, "subject extent is outside the bounded millimeter studio range"));
    }
    let hdr = source["hdr"].as_str().ok_or_else(|| bad(command, "native composition has no HDR identity"))?;
    let resource = super::library::hdr_resource(hdr, command)?;
    let resource_id = resource.id();
    let mut lighting = StudioLighting { environment: [0.0; 3], sun: None, area_lights: AreaLights::default() };
    let mut panels = Vec::new();
    let mut projection = Value::Null;
    if let Some(rig) = source["rig"].as_str() {
        let input = super::library::RigInput {
            id: rig.into(),
            variant: "complete".into(),
            center_mm: [center.x, center.y, center.z],
            subject_extent_mm: extent,
        };
        let (mut prepared, details) = super::library::rig_lighting(&input, command)?;
        let areas: Vec<_> = prepared
            .area_lights
            .iter()
            .map(|light| {
                let mut light = *light;
                light.radiance = light.radiance.map(|value| value / 16.0);
                light
            })
            .collect();
        prepared.area_lights = AreaLights::new(&areas).map_err(|e| bad(command, e.to_string()))?;
        if prepared.sun.is_some() {
            return Err(bad(command, "this studio projection cannot introduce a solar emitter"));
        }
        panels.extend(super::library::rig_stage(&input, command)?.iter().cloned());
        lighting = prepared;
        projection = details;
    }
    let reflectance: [f64; 3] = serde_json::from_value(source["ground"].clone()).map_err(|e| bad(command, e.to_string()))?;
    panels.push(StagePanel {
        id: "studio_ground".into(),
        center_mm: [center.x, center.y, bounds.min.z - 0.1],
        half_u_mm: [(size.x * 0.5).max(0.001) + extent, 0.0, 0.0],
        half_v_mm: [0.0, (size.y * 0.5).max(0.001) + extent, 0.0],
        reflectance_linear: reflectance,
    });
    setup.stage = Stage::new(panels).map_err(|e| bad(command, e.to_string()))?;
    setup.exposure_ev = source["exposure_ev"].as_f64().ok_or_else(|| bad(command, "native composition exposure missing"))?;
    if let Some(RenderJob::GpuPath { options, .. }) = &mut setup.job {
        options.environment = None;
        options.hdr = Some(GpuHdrEnvironment {
            resource: resource_id,
            rotation_rad: super::library::hdr_rotation(0.35, command)?,
            intensity: source["intensity"].as_f64().ok_or_else(|| bad(command, "native composition intensity missing"))?,
            assume_linear_srgb: false,
        });
        options.lighting = Some(lighting);
    }
    setup.validate().map_err(|e| bad(command, e.to_string()))?;
    super::staging::apply(&mut product, &setup, command)?;
    super::library::rig_clearance(&product, &lighting, command)?;
    let mut settings = (*s.doc.render).clone();
    settings.resources.insert(resource).map_err(|e| bad(command, e.to_string()))?;
    settings.setups.insert(input.name.clone(), setup.clone());
    settings.validate().map_err(|e| bad(command, e.to_string()))?;
    let report = json!({"id":input.id,"name":input.name,"revision":s.revision,"source":source,"setup":setup,
        "resource_sha256":resource_id,"rig_projection":projection,"status":"resolved_native_projection",
        "document_changed":false,"executor":"gpu-path","requires_lighting_consent":true,"requires_stage_consent":stage_replacement,
        "replaces":["lighting","environment","passive_stage","exposure"],
        "preserves":["camera","output_dimensions","sampling","optical_bindings","geometry","engineering_materials","pose","visibility"],
        "clearance":"conservative subject/stage/emitter AABB separation; not exact contact certification",
        "limitations":["Authored relative RGB radiance, not measured photometric calibration.","No automatic material or semantic-role assignment.","This composition preserves existing sampling, master retention and medium intent; it does not certify convergence or add terrain, motion or nested-media support."]});
    Ok(Candidate { settings, setup, report })
}
pub(super) fn resolve(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.preset.resolve";
    let input: Input = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    Ok(candidate(s, &input, command)?.report)
}
pub(super) fn apply(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.preset.apply";
    let input: Input = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    if input.expected_revision.is_none() || !input.replace_lighting_and_environment {
        return Err(bad(command, "resolve first, then supply expected_revision and explicit lighting/environment/exposure replacement consent"));
    }
    let mut ready = candidate(s, &input, command)?;
    if ready.report["requires_stage_consent"] == true && !input.replace_stage {
        return Err(bad(command, "existing stage requires explicit replace_stage consent"));
    }
    let changed = ready.settings != *s.doc.render;
    if changed {
        s.doc_mut().render = Arc::new(ready.settings);
    }
    ready.report["document_changed"] = json!(changed);
    ready.report["setup"] = json!(ready.setup);
    Ok(ready.report)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Quality {
    id: String,
    variant: String,
    acknowledge_omissions: bool,
}
pub(super) fn quality(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.quality.prepare";
    let input: Quality = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    if input.variant != "fixed_samples_png" || !input.acknowledge_omissions {
        return Err(bad(
            command,
            "explicit fixed_samples_png projection and omission acknowledgement are required; full Q-* requirements remain unbound",
        ));
    }
    let source = super::library::item("quality", &input.id, command)?;
    let number = |path: &[&str]| -> Result<u32> {
        let mut field = source;
        for key in path {
            field = field.get(*key).ok_or_else(|| bad(command, "quality field missing"))?;
        }
        field.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| bad(command, "quality field is not a bounded unsigned integer"))
    };
    let (width, height) = (number(&["resolution", "width"])?, number(&["resolution", "height"])?);
    let (samples, depth) = (number(&["sampling", "maximum_samples"])?, number(&["transport", "maximum_depth"])?);
    let seed = number(&["seed"])?;
    if solidcraft_render::job::GPU_PATH_LIMITS.fit(u64::from(width), u64::from(height), None) != Some((u64::from(width), u64::from(height)))
        || samples == 0
        || samples > 4096
        || depth == 0
        || depth > 16
        || source["motion"]["enabled"] != false
    {
        return Err(bad(
            command,
            "this quality concept exceeds current fixed-sample still-image capabilities; no dimensions, samples or motion are silently reduced",
        ));
    }
    Ok(json!({"id":input.id,"name":source["name"],"variant":input.variant,"status":"fixed_budget_projection_not_full_quality_profile",
        "revision":s.revision,"document_changed":false,"source_unchanged":true,
        "settings":{"width":width,"height":height},"gpu_options":{"samples":samples,"max_depth":depth,"seed":seed,"denoise":source["denoising"]["enabled"]},
        "preserves":["executor","camera","lighting","environment","stage","optical_bindings","exposure","tone_map"],
        "omitted":["adaptive_stopping_and_error_target","independent_transmission_depth","authored_russian_roulette_rule","preserved_raw_linear_output","EXR_master","convergence_certification"],
        "source":source}))
}

#[cfg(test)]
#[path = "studio_presets_tests.rs"]
mod tests;
