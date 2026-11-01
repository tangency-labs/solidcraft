use crate::{Result, Session, params::bad, render_jobs::job_error};
use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_doc::render_resources::{CpuHdrEnvironment, GpuHdrEnvironment};
use solidcraft_doc::render_settings::{RenderJob, RenderSetup};
use solidcraft_render::{
    RenderOperation, RenderRequest,
    job::{ImageRequest, Integrator, JobId, RaytraceOptions},
};

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Start {
    studio_weights: Option<[f64; 3]>,
    progressive: bool,
    denoise: bool,
    samples: Option<u32>,
    max_depth: Option<u32>,
    seed: Option<u32>,
    integrator: Integrator,
    environment: Option<EnvironmentRequest>,
    raytrace: Option<RaytraceOptions>,
    led_strip: Option<solidcraft_render::job::led_strip::LedStripOptions>,
    led_samples: Option<u32>,
    setup: Option<String>,
    settings: Option<RenderSetup>,
    transparent_background: bool,
    replace: bool,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct EnvironmentRequest {
    path: String,
    resource: Option<solidcraft_doc::render_resources::HdrId>,
    expected_sha256: Option<solidcraft_doc::render_resources::HdrId>,
    rotation_radians: f64,
    intensity: f64,
    samples: u32,
}
impl Default for EnvironmentRequest {
    fn default() -> Self {
        Self { path: String::new(), resource: None, expected_sha256: None, rotation_radians: 0.0, intensity: 1.0, samples: 16 }
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ProgressiveStart {
    studio_weights: Option<[f64; 3]>,
    aging: solidcraft_doc::render_aging::SavedAgingStack,
    #[serde(default)]
    nested_media: bool,
    volume: Option<solidcraft_render::job::volume::HomogeneousVolume>,
    adaptive: Option<solidcraft_render::job::adaptive::AdaptiveSampling>,
    transmission_samples: Option<u32>,
    finishing: solidcraft_render::job::finishing::Finishing,
    backdrop: Option<solidcraft_render::job::finishing::Backdrop>,
    linear_exr: bool,
    integrator: Integrator,
    setup: Option<String>,
    settings: Option<RenderSetup>,
    transparent_background: bool,
    replace: bool,
    progressive: bool,
    samples: Option<u32>,
    max_depth: Option<u32>,
    seed: Option<u32>,
    raytrace: super::transmission::Raytrace,
    lens: Option<solidcraft_render::job::ThinLens>,
    lighting: Option<solidcraft_render::job::StudioLighting>,
    tone_map: Option<solidcraft_render::job::ToneMap>,
    environment: Option<super::environment::Input>,
    denoise: bool,
}

struct Prepared {
    replace: bool,
    scene: solidcraft_render::ProductScene,
    request: ImageRequest,
    reply: Value,
    executor: solidcraft_render::ExecutorId,
    options: solidcraft_render::job::JobOptions,
    environment: Option<solidcraft_render::lighting::environment_input::EnvironmentInput>,
}

fn lower_led(input: &mut Start, command: &str) -> Result<()> {
    if input.led_strip.is_none() && input.led_samples.is_some() {
        return Err(bad(command, "led_samples requires led_strip"));
    }
    if let Some(strip) = input.led_strip {
        if input.integrator != Integrator::Raytrace {
            return Err(bad(command, "led_strip requires integrator: raytrace"));
        }
        strip.append_to(input.raytrace.get_or_insert_default(), input.led_samples.unwrap_or(16)).map_err(job_error)?;
        input.led_strip = None;
        input.led_samples = None;
    }
    Ok(())
}

#[cfg(test)]
mod union_lowering_tests {
    use super::*;
    #[test]
    fn led_lowering_keeps_hdr_reflection_aa_and_anisotropy_inputs_without_reading_hdr() {
        let wire = json!({"integrator":"raytrace","environment":{"path":"unopened.hdr","rotation_radians":0.75,"intensity":2,"samples":64},"raytrace":{"primary_samples":4,"reflections":{"bodies":[{"body":"receiver","mode":"rough_specular"}],"samples":16,"max_bounces":3},"anisotropic_faces":[{"body_index":0,"body_name":"receiver","face":0,"tangent":[1,0,0],"orientation":"quarter_turn","roughness_x":0.2,"roughness_y":0.5},null],"punctual_lights":[{"position_mm":[0,0,20],"color":[1,1,1],"intensity":1,"reference_distance_mm":20,"minimum_distance_mm":0.001}]},"led_strip":{"center_mm":[0,0,10],"length_mm":8,"width_mm":0.5,"length_axis":[1,0,0],"width_axis":[0,-1,0],"color":[1,0.5,0.25],"intensity":2},"led_samples":64});
        let mut input: Start = serde_json::from_value(wire.clone()).unwrap();
        let before = input.raytrace.clone().unwrap();
        lower_led(&mut input, "render.preflight").unwrap();
        let after = input.raytrace.unwrap();
        assert_eq!(before.primary_samples, after.primary_samples);
        assert_eq!(before.reflections, after.reflections);
        assert_eq!(before.anisotropic_faces, after.anisotropic_faces);
        assert_eq!(before.punctual_lights, after.punctual_lights);
        assert_eq!(after.area_lights.as_ref().unwrap().len(), 1);
        assert_eq!(after.area_samples, Some(64));
        let environment = input.environment.unwrap();
        assert_eq!(environment.path, "unopened.hdr");
        assert_eq!(environment.rotation_radians, 0.75);
        assert_eq!(environment.intensity, 2.0);
        assert_eq!(environment.samples, 64);
        assert!(input.led_strip.is_none() && input.led_samples.is_none());
        assert_eq!(wire["environment"]["path"], json!("unopened.hdr"));
    }
}

/// Resolve portable intent before choosing an executor. Legacy requests are left intact;
/// complete saved jobs cannot be silently overridden by a second set of runtime controls.
enum SavedHdr {
    Cpu(CpuHdrEnvironment),
    Gpu(GpuHdrEnvironment),
}

struct Normalized {
    params: Value,
    hdr: Option<SavedHdr>,
    surfaces: solidcraft_doc::render_surface::SurfaceBindings,
    optics: solidcraft_doc::render_optics::OpticalBindings,
}

fn normalize_saved_job(s: &Session, params: &Value, command: &str) -> Result<Option<Normalized>> {
    let Some(object) = params.as_object() else {
        return Ok(None);
    };
    let named = object.get("setup").filter(|value| !value.is_null());
    let inline = object.get("settings").filter(|value| !value.is_null());
    if named.is_some() && inline.is_some() {
        return Err(bad(command, "choose either setup or settings"));
    }
    let mut settings = if let Some(value) = inline {
        if value.get("job").is_none_or(Value::is_null) {
            return Ok(None);
        }
        serde_json::from_value::<RenderSetup>(value.clone()).map_err(|e| bad(command, e.to_string()))?
    } else if let Some(settings) = named.and_then(Value::as_str).and_then(|name| s.doc.render.setups.get(name)) {
        if settings.job.is_none() {
            return Ok(None);
        }
        settings.clone()
    } else {
        return Ok(None);
    };
    if let Some(key) = object.keys().find(|key| !matches!(key.as_str(), "setup" | "settings" | "replace" | "studio_weights")) {
        return Err(bad(
            command,
            format!(
                "saved job already defines the active method and its options; runtime `{key}` cannot override it (use job: null for a runtime request)"
            ),
        ));
    }
    settings.validate().map_err(|e| bad(command, e.to_string()))?;
    s.doc.render.validate_resources(&settings).map_err(|e| bad(command, e.to_string()))?;
    let Some(job) = settings.job.take() else {
        return Ok(None);
    };
    let optics = std::mem::take(&mut settings.optics);
    let surfaces = std::mem::take(&mut settings.surfaces);
    let mut hdr = None;
    let mut lowered = match job {
        RenderJob::Raster { transparent_background } => json!({"transparent_background":transparent_background}),
        RenderJob::CpuRaytrace { options } => {
            hdr = options.hdr.map(SavedHdr::Cpu);
            let raytrace = options.to_runtime(settings.exposure_ev).map_err(|e| bad(command, e.to_string()))?;
            // The legacy CPU path reads EV from RaytraceOptions. There is one saved authority.
            settings.exposure_ev = 0.0;
            json!({"integrator":"raytrace","raytrace":raytrace})
        }
        RenderJob::GpuPath { transparent_background, options } => {
            hdr = options.hdr.map(SavedHdr::Gpu);
            json!({
            "progressive":true,
            "transparent_background":transparent_background,
            "samples":options.samples,"max_depth":options.max_depth,"seed":options.seed,
            "lens":options.lens,"lighting":options.lighting,"tone_map":options.tone_map,
            "denoise":options.denoise,"environment":options.environment,
            "finishing":options.finishing,"backdrop":options.backdrop,"linear_exr":options.linear_exr,
            "adaptive":options.adaptive,"transmission_samples":options.transmission_samples,"volume":options.volume,"nested_media":options.nested_media,"aging":options.aging
            })
        }
    };
    lowered["settings"] = serde_json::to_value(settings).map_err(|e| bad(command, e.to_string()))?;
    if let Some(weights) = object.get("studio_weights") {
        lowered["studio_weights"] = weights.clone();
    }
    if let Some(replace) = object.get("replace") {
        lowered["replace"] = replace.clone();
    }
    Ok(Some(Normalized { params: lowered, hdr, surfaces, optics }))
}

fn prepare(s: &Session, params: &Value, command: &str) -> Result<Prepared> {
    let normalized = normalize_saved_job(s, params, command)?;
    let saved_hdr = normalized.as_ref().and_then(|normalized| normalized.hdr.as_ref());
    let params = normalized.as_ref().map_or(params, |normalized| &normalized.params);
    if params.get("progressive").and_then(Value::as_bool) == Some(true) {
        let hdr = match saved_hdr {
            Some(SavedHdr::Gpu(hdr)) => Some(*hdr),
            Some(SavedHdr::Cpu(_)) => return Err(bad(command, "saved CPU HDR cannot enter a GPU request")),
            None => None,
        };
        return prepare_progressive(
            s,
            params,
            command,
            normalized.is_some(),
            hdr,
            normalized.as_ref().map(|n| &n.surfaces),
            normalized.as_ref().map(|n| &n.optics),
        );
    }
    let gpu_raytrace =
        params.get("raytrace").and_then(Value::as_object).is_some_and(|o| ["clear_coats", "textures"].iter().any(|key| o.contains_key(*key)));
    if gpu_raytrace
        || params.get("environment").is_some_and(|e| e.get("kind").is_some())
        || ["lens", "lighting", "tone_map"].iter().any(|key| params.get(*key).is_some())
        || params.get("denoise").is_some_and(|value| value != &json!(false))
    {
        return Err(bad(
            command,
            "runtime GPU transmission, texture, lens, environment, lighting, tone-map and denoise controls require progressive rendering",
        ));
    }
    let mut input: Start = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    validate_studio_weights(input.studio_weights, command)?;
    if input.integrator == Integrator::Raytrace && input.studio_weights.is_some() {
        return Err(bad(command, "studio_weights apply to raster or the implicit GPU studio; use explicit CPU light intensities"));
    }
    if input.progressive {
        return prepare_progressive(s, params, command, false, None, None, None);
    }
    if input.denoise {
        return Err(bad(command, "denoise requires GPU progressive rendering"));
    }
    if input.samples.is_some() || input.max_depth.is_some() || input.seed.is_some() {
        return Err(bad(command, "sampling controls require progressive rendering"));
    }
    if input.setup.is_some() && input.settings.is_some() {
        return Err(bad(command, "choose either setup or settings"));
    }
    // Reject unavailable execution before capturing authoritative product meshes.
    // Runtime services are independent of the document and can be checked cold.
    let host = s.render_job_host().ok_or_else(|| bad(command, "no image host is installed"))?;
    if !host.is_available() {
        return Err(bad(command, "image host is unavailable"));
    }
    host.validate_integrator(input.integrator).map_err(job_error)?;
    lower_led(&mut input, command)?;
    if input.integrator == Integrator::Raster && input.raytrace.is_some() {
        return Err(bad(command, "raytrace options require integrator: raytrace"));
    }
    if input.integrator == Integrator::Raytrace && input.transparent_background {
        return Err(bad(command, "raytrace supports opaque backgrounds only"));
    }
    if let Some(options) = &input.raytrace {
        options.retained_bytes().map_err(job_error)?;
        options.budget.validate_execution().map_err(job_error)?;
        // Check the complete image envelope before any source file is read.
        let dimensions = if let Some(name) = &input.setup {
            let settings = s.doc.render.setups.get(name).ok_or_else(|| bad(command, "no saved render setup with that name"))?;
            (settings.width, settings.height)
        } else if let Some(settings) = &input.settings {
            (settings.width, settings.height)
        } else {
            let defaults = RenderSetup::default();
            (defaults.width, defaults.height)
        };
        options.budget.validate_image(dimensions.0, dimensions.1, options.progressive).map_err(job_error)?;
    }
    let environment = if let Some(saved_hdr) = saved_hdr {
        let SavedHdr::Cpu(hdr) = saved_hdr else {
            return Err(bad(command, "saved GPU HDR cannot enter a CPU request"));
        };
        if input.integrator != Integrator::Raytrace || input.environment.is_some() {
            return Err(bad(command, "saved CPU HDR requires raytrace without a runtime environment override"));
        }
        let resource = s.doc.render.resources.required(&hdr.resource).map_err(|e| bad(command, e.to_string()))?;
        Some(
            solidcraft_render::lighting::environment_input::EnvironmentInput::new(resource.source(), hdr.rotation_rad, hdr.intensity, hdr.samples)
                .map_err(|e| bad(command, e.to_string()))?,
        )
    } else if let Some(environment) = &input.environment {
        if input.integrator != Integrator::Raytrace {
            return Err(bad(command, "environment requires raytrace"));
        }
        let source = if let Some(id) = environment.resource {
            if !environment.path.is_empty() || environment.expected_sha256.is_some() {
                return Err(bad(command, "choose an embedded HDR resource or a file path with optional expected_sha256"));
            }
            s.doc.render.resources.required(&id).map_err(|e| bad(command, e.to_string()))?.source()
        } else {
            if environment.path.is_empty()
                || environment.path.len() > 4096
                || environment.path.capacity() > 4096
                || environment.path.chars().any(char::is_control)
            {
                return Err(bad(command, "environment requires a bounded HDR input path"));
            }
            let bytes =
                solidcraft_io::vfs::read(&environment.path, solidcraft_render::lighting::MAX_HDR_BYTES).map_err(|e| bad(command, e.to_string()))?;
            if environment.expected_sha256.is_some_and(|id| id != solidcraft_doc::render_resources::HdrId::of(&bytes)) {
                return Err(bad(command, "HDR file changed since selection; inspect and select it again"));
            }
            solidcraft_render::lighting::HdrSource::new(bytes).map_err(|e| bad(command, e.to_string()))?
        };
        Some(
            solidcraft_render::lighting::environment_input::EnvironmentInput::new(
                source,
                environment.rotation_radians,
                environment.intensity,
                environment.samples,
            )
            .map_err(|e| bad(command, e.to_string()))?,
        )
    } else {
        None
    };
    let mut scene = crate::product_scene::extract(s)?;
    if environment.is_some() {
        input.raytrace.as_ref().unwrap_or(&RaytraceOptions::default()).validate_environment_reflections(&scene).map_err(job_error)?;
    }
    let settings = if let Some(name) = &input.setup {
        s.doc.render.setups.get(name).cloned().ok_or_else(|| bad(command, "no saved render setup with that name"))?
    } else if let Some(settings) = &input.settings {
        settings.clone()
    } else {
        let mut settings = RenderSetup::default();
        if let Some(camera) = s.camera {
            settings.camera = camera;
        } else {
            settings.camera.fit(&scene.bounds);
        }
        settings
    };
    if input.integrator == Integrator::Raster && !settings.stage.is_empty() {
        return Err(bad(command, "passive stages require a raytrace executor"));
    }
    super::staging::apply(&mut scene, &settings, command)?;
    let budget = input.raytrace.as_ref().map_or(solidcraft_render::job::CpuBudget::Compact, |o| o.budget);
    if input.integrator == Integrator::Raytrace {
        settings.validate_for_cpu(budget).map_err(|e| bad(command, e.to_string()))?;
        budget.validate_execution().map_err(job_error)?;
        budget.validate_image(settings.width, settings.height, input.raytrace.as_ref().and_then(|o| o.progressive)).map_err(job_error)?;
    } else {
        settings.validate().map_err(|e| bad(command, e.to_string()))?;
    }
    // The raster image executor has no exposure or environment controls. A setup saved with them
    // (the render workspace and the other executors use them) still renders here, and the reply
    // names what this render leaves out (#5089). The raytrace path takes them only through a
    // saved job, so a raytrace request with them is still refused.
    let ignored = if input.integrator == Integrator::Raster { ignored_controls(&settings) } else { Vec::new() };
    if input.integrator != Integrator::Raster
        && (settings.exposure_ev != 0.0 || settings.environment_rotation != 0.0 || settings.environment_intensity != 1.0)
    {
        return Err(bad(command, "exposure and environment controls are not supported by the R4a CPU image executor"));
    }
    let mut descriptor = host.descriptor_for(input.integrator).map_err(job_error)?;
    if input.integrator == Integrator::Raytrace {
        descriptor.limits = budget.image_limits(input.raytrace.as_ref().is_some_and(|o| o.progressive.is_some()));
    }
    let expected_executor = descriptor.id.clone();
    let mut backend = s.render_backend();
    backend.executors = vec![descriptor];
    let mut features = scene.required_features();
    if environment.is_some() {
        features.push(solidcraft_render::RenderFeature::Hdri);
    }
    let refraction = input.raytrace.as_ref().is_some_and(|options| !options.dielectrics.is_empty());
    if refraction && !features.contains(&solidcraft_render::RenderFeature::Refraction) {
        features.push(solidcraft_render::RenderFeature::Refraction);
    }
    let environment_info = environment.as_ref().map(|e| e.metadata());
    let resolved = solidcraft_render::resolve_request(
        &[backend],
        &RenderRequest {
            backend: Some(settings.backend.clone()),
            mode: settings.mode,
            operation: RenderOperation::Image,
            width: u64::from(settings.width),
            height: u64::from(settings.height),
            format: Some(settings.format),
            features,
            policy: Default::default(),
        },
    )
    .map_err(|e| bad(command, e.to_string()))?;
    if resolved.executor != expected_executor {
        return Err(bad(command, "resolved executor is not installed as an image job host"));
    }
    let request = ImageRequest {
        studio_weights: validate_studio_weights(input.studio_weights, command)?,
        environment: None,
        textures: None,
        integrator: input.integrator,
        raytrace: input.raytrace.take(),
        camera: settings.camera,
        width: settings.width,
        height: settings.height,
        background: settings.background,
        transparent_background: input.transparent_background,
    };
    let mut admission = solidcraft_render::job::admit(&scene, &request).map_err(job_error)?;
    let environment_bytes = environment.as_ref().map_or(Ok(0), |e| e.required_bytes()).map_err(|e| bad(command, e.to_string()))?;
    let retained_lights = request.raytrace.as_ref().map_or(Ok(0), |o| o.retained_bytes()).map_err(job_error)?;
    let retained_environment =
        if environment_info.is_some() { std::mem::size_of::<solidcraft_render::lighting::environment_input::EnvironmentInfo>() as u64 } else { 0 };
    let retained_request_bytes = retained_lights.checked_add(retained_environment).ok_or_else(|| bad(command, "request metadata overflow"))?;

    admission.estimated_total_bytes = admission
        .estimated_total_bytes
        .checked_add(environment_bytes)
        .and_then(|v| v.checked_add(retained_request_bytes))
        .filter(|v| *v <= budget.limits().max_job_bytes)
        .ok_or_else(|| bad(command, "environment job exceeds per-job memory limit"))?;
    admission.preparation_bytes =
        admission.preparation_bytes.checked_add(environment_bytes).ok_or_else(|| bad(command, "environment admission overflow"))?;
    let transparency = match input.integrator {
        Integrator::Raster => "sorted_alpha_approximation",
        Integrator::Raytrace if refraction => "physical_transmission",
        Integrator::Raytrace => "opaque_only",
    };
    let reply = json!({"ignored_controls":ignored,"stage_panels":settings.stage.len(),"stage_transport":"CPU direct/specular only; no diffuse interreflection", "cpu_budget":budget,"cpu_limits":budget.limits(),"environment":environment_info,"retained_request_bytes":retained_request_bytes,"resolved":resolved,"admission":admission,"request":request,"scene_generation":s.revision,"host":host.kind(),"transparency":transparency,"refraction":refraction,"dielectric_boundaries":if refraction {vec!["thin_sheet","closed_box"]} else {Vec::<&str>::new()}});
    Ok(Prepared { replace: input.replace, scene, request, reply, environment, executor: resolved.executor, options: Default::default() })
}

fn prepare_progressive(
    s: &Session,
    params: &Value,
    command: &str,
    require_gpu_path: bool,
    saved_hdr: Option<GpuHdrEnvironment>,
    saved_surfaces: Option<&solidcraft_doc::render_surface::SurfaceBindings>,
    saved_optics: Option<&solidcraft_doc::render_optics::OpticalBindings>,
) -> Result<Prepared> {
    let cpu_raytrace = params
        .get("raytrace")
        .and_then(Value::as_object)
        .is_some_and(|o| o.keys().any(|key| !matches!(key.as_str(), "dielectrics" | "clear_coats" | "textures")));
    if params.get("integrator").is_some_and(|v| v.as_str() != Some("raster"))
        || cpu_raytrace
        || params.get("led_strip").is_some()
        || params.get("led_samples").is_some()
        || params.get("environment").is_some_and(|e| e.is_object() && e.get("kind").is_none())
    {
        return Err(bad(command, "progressive rendering does not accept CPU raytrace, environment or LED options"));
    }
    let mut input: ProgressiveStart = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    validate_studio_weights(input.studio_weights, command)?;
    if input.studio_weights.is_some() && (input.lighting.is_some() || input.environment.is_some() || saved_hdr.is_some()) {
        return Err(bad(command, "studio_weights require the implicit GPU studio without environment lighting"));
    }
    if input.integrator != Integrator::Raster || !input.progressive {
        return Err(bad(command, "progressive rendering requires the registered GPU request method"));
    }
    if input.setup.is_some() && input.settings.is_some() {
        return Err(bad(command, "choose either setup or settings"));
    }
    let mut scene = crate::product_scene::extract(s)?;
    if !input.progressive
        && (!input.raytrace.dielectrics.is_empty()
            || !input.raytrace.clear_coats.is_empty()
            || !input.raytrace.textures.is_empty()
            || input.lens.is_some()
            || input.lighting.is_some()
            || input.tone_map.is_some()
            || input.environment.is_some()
            || input.denoise)
    {
        return Err(bad(
            command,
            "runtime transmission, clear-coat, texture, lens, environment, lighting, tone-map and denoise controls require progressive rendering",
        ));
    }
    if let Some(bindings) = saved_surfaces.filter(|bindings| !bindings.is_empty()) {
        if !input.raytrace.textures.is_empty() {
            return Err(bad(command, "saved surfaces cannot be combined with runtime texture overrides"));
        }
        input.raytrace.textures = super::surface_bindings::resolve(s, &scene, bindings, command)?;
    }
    if let Some(bindings) = saved_optics.filter(|b| !b.is_empty()) {
        if !input.raytrace.dielectrics.is_empty() || !input.raytrace.clear_coats.is_empty() {
            return Err(bad(command, "saved optical bindings cannot be mixed with transient optical overrides"));
        }
        input.raytrace.dielectrics = super::optical_bindings::resolve(s, &scene, bindings, command)?;
    }
    super::transmission::apply(&mut scene, &input.raytrace.dielectrics, command)?;
    super::clear_coat::apply(&mut scene, &input.raytrace.clear_coats, command)?;
    let settings = if let Some(name) = &input.setup {
        s.doc.render.setups.get(name).cloned().ok_or_else(|| bad(command, "no saved render setup with that name"))?
    } else if let Some(settings) = &input.settings {
        settings.clone()
    } else {
        let mut settings = RenderSetup::default();
        if let Some(camera) = s.camera {
            settings.camera = camera;
        } else {
            settings.camera.fit(&scene.bounds);
        }
        settings
    };
    settings.validate_for_gpu().map_err(|e| bad(command, e.to_string()))?;
    if settings.environment_rotation != 0.0 || settings.environment_intensity != 1.0 {
        return Err(bad(command, "environment controls are not supported by the installed image executors"));
    }
    if !input.progressive && (input.samples.is_some() || input.max_depth.is_some() || input.seed.is_some() || settings.exposure_ev != 0.0) {
        return Err(bad(command, "sampling and exposure controls require progressive rendering"));
    }
    super::staging::apply(&mut scene, &settings, command)?;
    let mut options = solidcraft_render::job::JobOptions::default();
    if let Some(samples) = input.samples {
        options.samples = samples;
    }
    if let Some(depth) = input.max_depth {
        options.max_depth = depth;
    }
    if let Some(seed) = input.seed {
        options.seed = seed;
    }
    options.exposure_ev = settings.exposure_ev;
    options.lens = input.lens;
    options.lighting = input.lighting;
    options.tone_map = input.tone_map.unwrap_or_default();
    options.denoise = input.denoise;
    options.finishing = input.finishing;
    options.backdrop = input.backdrop;
    options.linear_exr = input.linear_exr;
    options.aging = super::effects::resolve_aging(s, &scene, &input.aging, command)?;
    options.nested_media = input.nested_media;
    options.volume = input.volume;
    options.adaptive = input.adaptive;
    options.transmission_samples = input.transmission_samples.unwrap_or(1);
    options.validate().map_err(job_error)?;
    if let Some(super::environment::Input::BuiltinHdr { id, .. }) = &input.environment
        && options.lighting.as_ref().is_some_and(|lighting| lighting.sun.is_some())
        && super::library::owns_sun(id, command)?
    {
        return Err(bad(command, "built-in HDR and analytic lighting cannot both own the sun"));
    }
    let environment = if let Some(hdr) = saved_hdr {
        if input.environment.is_some() {
            return Err(bad(command, "saved GPU HDR cannot have a runtime environment override"));
        }
        let resource = s.doc.render.resources.required(&hdr.resource).map_err(|e| bad(command, e.to_string()))?;
        Some(std::sync::Arc::new(
            solidcraft_render::lighting::environment::EnvironmentSource::from_hdr(
                resource.source(),
                hdr.rotation_rad,
                hdr.intensity,
                hdr.assume_linear_srgb,
            )
            .map_err(|e| bad(command, e.to_string()))?,
        ))
    } else {
        input.environment.as_ref().map(|source| source.capture(&s.doc.render.resources, command)).transpose()?
    };
    let mut features = scene.required_features();
    if environment.is_some() {
        features.push(solidcraft_render::RenderFeature::Hdri);
    }
    if input.progressive {
        features.push(solidcraft_render::RenderFeature::Progressive);
        if scene.has_transmission() {
            features.push(solidcraft_render::RenderFeature::Refraction);
        }
    }
    let mut backend = s.render_backend();
    if require_gpu_path {
        backend.executors.retain(|executor| executor.id.as_str() == "gpu-path");
        if backend.executors.is_empty() {
            return Err(bad(command, "saved gpu_path method requires an installed gpu-path executor"));
        }
    }
    let resolved = solidcraft_render::resolve_request(
        &[backend],
        &RenderRequest {
            backend: Some(settings.backend.clone()),
            mode: settings.mode,
            operation: RenderOperation::Image,
            width: u64::from(settings.width),
            height: u64::from(settings.height),
            format: Some(settings.format),
            features,
            policy: Default::default(),
        },
    )
    .map_err(|e| bad(command, e.to_string()))?;
    if require_gpu_path && resolved.executor.as_str() != "gpu-path" {
        return Err(bad(command, "saved gpu_path method requires the gpu-path executor; another progressive executor cannot substitute for it"));
    }
    if options.denoise && resolved.executor.as_str() != "gpu-path" {
        return Err(bad(command, "denoise requires the native gpu-path executor"));
    }
    let host = s.render_jobs.host_for(&resolved.executor).ok_or_else(|| bad(command, "resolved executor is not installed as an image job host"))?;
    if !host.is_available() {
        return Err(bad(command, "image host is unavailable"));
    }
    let request = ImageRequest {
        studio_weights: validate_studio_weights(input.studio_weights, command)?,
        integrator: Default::default(),
        raytrace: None,
        environment,
        camera: settings.camera,
        width: settings.width,
        height: settings.height,
        background: settings.background,
        transparent_background: input.transparent_background,
        textures: super::textures::capture(&scene, &input.raytrace.textures, command)?,
    };
    let mut admission = host.admit(&scene, &request, options).map_err(job_error)?;
    admission.estimated_total_bytes = admission
        .estimated_total_bytes
        .checked_add(request.retained_resource_metadata_bytes())
        .filter(|bytes| *bytes <= host.job_limit_bytes())
        .ok_or_else(|| bad(command, "GPU work and retained metadata exceed the per-job limit"))?;
    let refraction = input.progressive && scene.has_transmission();
    let reply = json!({"stage_panels":settings.stage.len(),"resolved":resolved,"admission":admission,"request":request,"textures":request.textures.as_ref().map(|resources|resources.summary()),"scene_generation":s.revision,"host":host.kind(),"transparency":if refraction {"physical_transmission"} else if input.progressive {"opaque_only"} else {"sorted_alpha_approximation"},"options":options,"environment":request.environment.as_ref().map(|source|source.info()),"refraction":refraction,"dielectric_boundaries": if resolved.executor.as_str() == "gpu-path" {vec!["thin_sheet","closed_box","closed_mesh"]} else {Vec::<&str>::new()}});
    let mut reply = reply;
    if options.denoise {
        reply["denoising"] = json!({
            "method":"edge_aware_atrous", "passes":3, "steps":[1,2,4],
            "guides":"deterministic_center_ray_albedo_normal_position_depth",
            "bypass":"glass, smooth metal/coat, emitter, background and partial coverage first hits",
            "limitation":"first-hit guides do not reconstruct reflection/refraction or depth-of-field correspondences",
            "raw_linear_accumulator_preserved":true, "output_stage":"linear_before_tone_map"
        });
    }
    Ok(Prepared { replace: input.replace, scene, request, reply, executor: resolved.executor, options, environment: None })
}

/// The setup's controls the raster image executor doesn't apply: those not at their neutral value.
fn ignored_controls(settings: &RenderSetup) -> Vec<&'static str> {
    let mut ignored = Vec::new();
    if settings.exposure_ev != 0.0 {
        ignored.push("exposure_ev");
    }
    if settings.environment_rotation != 0.0 {
        ignored.push("environment_rotation");
    }
    if settings.environment_intensity != 1.0 {
        ignored.push("environment_intensity");
    }
    ignored
}

pub(super) fn preflight(s: &mut Session, params: &Value) -> Result<Value> {
    prepare(s, params, "render.preflight").map(|p| p.reply)
}

pub(super) fn start(s: &mut Session, params: &Value) -> Result<Value> {
    let p = prepare(s, params, "render.start")?;
    let status = s
        .render_jobs
        .start_with_environment(
            s.revision,
            p.scene,
            p.request,
            p.replace,
            crate::render_jobs::Execution { executor: p.executor, options: p.options, environment: p.environment },
        )
        .map_err(job_error)?;
    let mut status = json!(status);
    if let (Some(out), Some(ignored)) = (status.as_object_mut(), p.reply.get("ignored_controls")) {
        out.insert("ignored_controls".into(), ignored.clone());
    }
    Ok(status)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Regrade {
    job: JobId,
    presentation: solidcraft_render::job::presentation::Presentation,
}
pub(super) fn regrade(s: &mut Session, params: &Value) -> Result<Value> {
    let input: Regrade = serde_json::from_value(params.clone()).map_err(|e| bad("render.result.regrade", e.to_string()))?;
    input.presentation.validate().map_err(job_error)?;
    Ok(json!(s.render_jobs.regrade(input.job, input.presentation).map_err(job_error)?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Job {
    job: JobId,
}

pub(super) fn status(s: &mut Session, params: &Value) -> Result<Value> {
    let input: Job = serde_json::from_value(params.clone()).map_err(|e| bad("render.status", e.to_string()))?;
    Ok(json!(s.render_jobs.status(input.job).map_err(job_error)?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    job: Option<JobId>,
}
pub(super) fn pin(s: &mut Session, params: &Value) -> Result<Value> {
    if params.get("job").is_none() {
        return Err(bad("render.result.pin", "job or explicit null is required"));
    }
    let input: Pin = serde_json::from_value(params.clone()).map_err(|e| bad("render.result.pin", e.to_string()))?;
    Ok(json!({"frame":s.render_jobs.pin(input.job).map_err(job_error)?}))
}

pub(super) fn copy(s: &mut Session, params: &Value) -> Result<Value> {
    let input: Job = serde_json::from_value(params.clone()).map_err(|e| bad("render.result.copy", e.to_string()))?;
    let frame = s
        .render_jobs
        .handle(input.job)
        .map_err(job_error)?
        .frame()
        .map_err(job_error)?
        .ok_or_else(|| bad("render.result.copy", "render has no completed image"))?;
    let size = (u64::from(frame.info.width), u64::from(frame.info.height));
    if solidcraft_render::RenderLimits::default().fit(size.0, size.1, None) != Some(size) {
        return Err(bad("render.result.copy", "clipboard image exceeds 4096 per side or 4194304 pixels; export the PNG instead"));
    }
    Ok(json!({"frame":frame.info}))
}

pub(super) fn cancel(s: &mut Session, params: &Value) -> Result<Value> {
    let input: Job = serde_json::from_value(params.clone()).map_err(|e| bad("render.cancel", e.to_string()))?;
    let status = s.render_jobs.status(input.job).map_err(job_error)?;
    if status.progress.phase.terminal() {
        return Ok(json!(status));
    }
    Ok(json!(s.render_jobs.handle(input.job).map_err(job_error)?.cancel().map_err(job_error)?))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ExportFormat {
    #[default]
    Png,
    Exr,
}
impl ExportFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Exr => "exr",
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Export {
    #[serde(default)]
    format: ExportFormat,
    job: JobId,
    path: String,
    #[serde(default)]
    overwrite: bool,
}

pub(super) fn export(s: &mut Session, params: &Value) -> Result<Value> {
    let input: Export = serde_json::from_value(params.clone()).map_err(|e| bad("render.export", e.to_string()))?;
    let path = std::path::Path::new(&input.path);
    if input.path.len() > 4096
        || input.path.chars().any(char::is_control)
        || !path.extension().is_some_and(|x| x.eq_ignore_ascii_case(input.format.extension()))
    {
        return Err(bad("render.export", "a bounded destination with the explicitly selected png/exr extension is required"));
    }
    // Atomic native writers are also used for the app's own files and do not
    // apply a request's path scope. Check the caller's destination explicitly.
    solidcraft_io::vfs::check(&input.path).map_err(|e| bad("render.export", e.to_string()))?;
    let frame = s.render_jobs.frame(input.job).map_err(job_error)?;
    let bytes = match input.format {
        ExportFormat::Png => frame.png(),
        ExportFormat::Exr => frame
            .linear_exr()
            .ok_or_else(|| bad("render.export", "this completed job has no raw linear master; enable retention before rendering"))?,
    };
    // Replace atomically without reading an arbitrarily large previous target into memory.
    // Design saves retain their independent backup policy.
    let result =
        if input.overwrite { solidcraft_io::write_atomic(path, bytes, false) } else { solidcraft_io::safe_write::create_atomic(path, bytes) };
    result.map_err(|e| bad("render.export", format!("could not export completed {}: {e}", input.format.extension())))?;
    Ok(
        json!({"job":input.job,"path":input.path,"bytes":bytes.len(),"format":input.format.extension(),"encoding":if input.format==ExportFormat::Exr {"scene_linear_srgb_d65"} else {"srgb"},"alpha":if input.format==ExportFormat::Exr {"coverage_premultiplied"} else {"straight"},"frame":frame.info,"linear_master":frame.linear_master_info(),"presentation":frame.presentation()}),
    )
}

fn validate_studio_weights(weights: Option<[f64; 3]>, command: &str) -> Result<Option<[f64; 3]>> {
    if weights.is_some_and(|weights| weights.iter().any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))) {
        return Err(bad(command, "studio visibility weights must be finite in 0..1"));
    }
    Ok(weights)
}
