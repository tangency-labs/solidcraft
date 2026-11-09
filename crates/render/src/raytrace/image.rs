//! Opt-in opaque direct image integration; immutable world geometry, one host reservation.
use super::anisotropic_faces::{self, PlanarTarget};
use super::bvh::TraceCursor;
use super::dielectric::{self, Certificate, MAX_BOUNDARIES, MAX_OVERRIDES, MAX_PATHS};
use super::lighting::{DirectionalLight, Lighting, StudioPreset, map_to_display};
use super::prepared::{PreparedPrimitive, PreparedScene, SharedHit};
#[cfg(test)]
use super::progressive;
use super::progressive::{CameraFilm, Progressive};
use super::reflection;
use super::{BuildLimits, Bvh, BvhBuilder, Ray, TraceError, Triangle, WorkBudget};
use crate::job::{self, Admission, CancelToken, ErrorCode, ImageRequest, JobError, Phase, Progress, RaytraceOptions, TaskPoll};
use crate::{
    ProductScene,
    surface::{OpaqueSurface, srgb8_to_linear},
};
use solidcore_record::Vec3;
use solidcraft_color::srgb::encode8;
use solidcraft_geom::prelude::*;

#[cfg(test)]
#[path = "environment_image_tests.rs"]
mod environment_image_tests;

#[cfg(test)]
#[path = "environment_reflection_tests.rs"]
mod environment_reflection_tests;

#[cfg(test)]
#[path = "anisotropic_reflection_image_tests.rs"]
mod anisotropic_reflection_tests;

pub(crate) fn error(e: TraceError) -> JobError {
    let code = match e {
        TraceError::Cancelled => ErrorCode::Cancelled,
        TraceError::MemoryLimit | TraceError::WorkLimit => ErrorCode::LimitExceeded,
        TraceError::AllocationFailed => ErrorCode::AllocationFailed,
        TraceError::InvalidInput(_) => ErrorCode::InvalidScene,
        _ => ErrorCode::Internal,
    };
    JobError::new(code, e.to_string())
}
pub(super) fn invalid(message: &'static str) -> JobError {
    JobError::new(ErrorCode::InvalidScene, message)
}
fn limits(budget: job::CpuBudget) -> BuildLimits {
    let limits = budget.limits();
    BuildLimits {
        max_triangles: limits.max_triangles as usize,
        // BVH preparation has its own bounded geometry envelope; the larger
        // whole-image budget also pays for films, guides, previews and workers.
        max_memory_bytes: limits.max_job_bytes.min(super::bvh::MAX_MEMORY_BYTES),
        max_work: limits.max_work_units,
        ..Default::default()
    }
}
fn lighting(options: &RaytraceOptions) -> job::Result<Lighting> {
    options.validate()?;
    if let Some(preset) = options.preset {
        let preset = match preset {
            job::LightingPreset::Neutral => StudioPreset::Neutral,
            job::LightingPreset::WarmCool => StudioPreset::WarmCool,
            job::LightingPreset::KeyRim => StudioPreset::KeyRim,
        };
        return Lighting::preset(preset).map_err(error);
    }
    match &options.lights {
        None => Ok(Lighting::studio_default()),
        Some(inputs) => {
            let mut lights = Vec::new();
            lights.try_reserve_exact(inputs.len()).map_err(|_| error(TraceError::AllocationFailed))?;
            for l in inputs {
                lights.push(
                    DirectionalLight::new(
                        Vec3::new(l.direction[0], l.direction[1], l.direction[2]),
                        Vec3::new(l.color[0], l.color[1], l.color[2]),
                        l.intensity,
                    )
                    .map_err(error)?,
                );
            }
            Lighting::new(lights).map_err(error)
        }
    }
}
type Look = PreparedPrimitive;

/// Sealed optical result transferred only with its owning captured scene/request.
/// The general cursor retains its exact certified f64 boundary and allocations;
/// moving it into ImageTask does not certify the same geometry a second time.
pub(crate) struct CertifiedOptics {
    legacy: Option<[Option<dielectric::Certificate>; MAX_OVERRIDES]>,
    general: Vec<super::glass_mesh::Task>,
    work_used: u64,
}
impl CertifiedOptics {
    pub(super) fn work_used(&self) -> u64 {
        self.work_used
    }
    pub(super) fn legacy(values: [Option<dielectric::Certificate>; MAX_OVERRIDES], work_used: u64) -> Self {
        Self { legacy: Some(values), general: Vec::new(), work_used }
    }
    fn into_legacy(self) -> job::Result<[Option<dielectric::Certificate>; MAX_OVERRIDES]> {
        if !self.general.is_empty() {
            return Err(invalid("unexpected general optical proof"));
        }
        self.legacy.ok_or_else(|| invalid("missing legacy optical proof"))
    }
}

pub(crate) fn certification_bytes(scene: &ProductScene, options: &RaytraceOptions) -> job::Result<u64> {
    if options.dielectrics.is_empty() {
        return Ok(0);
    }
    let bytes = if options.path.is_some() {
        super::glass_mesh::Task::required_bytes(scene, options)?
    } else {
        super::legacy_certificates::Task::required_bytes(scene, options).map_err(error)?
    };
    bytes.checked_add(std::mem::size_of::<CertifiedOptics>() as u64).ok_or_else(|| error(TraceError::MemoryLimit))
}

fn legacy_error(cause: TraceError) -> JobError {
    match cause {
        TraceError::InvalidInput(
            message @ ("nested, touching or overlapping dielectric volumes are unsupported"
            | "thin sheets touching or inside dielectric volumes are unsupported"
            | "thin-lens aperture sphere touches or enters a certified dielectric volume"),
        ) => invalid(message),
        other => error(other),
    }
}

pub(crate) fn certify_for_request(
    scene: &ProductScene,
    request: &ImageRequest,
    options: &RaytraceOptions,
    cancel: &CancelToken,
) -> job::Result<Option<CertifiedOptics>> {
    if options.dielectrics.is_empty() {
        cancel.check()?;
        return Ok(None);
    }
    cancel.check()?;
    if options.path.is_some() {
        let mut general = Vec::new();
        general.try_reserve_exact(1).map_err(|_| error(TraceError::AllocationFailed))?;
        if general.capacity() != 1 {
            return Err(error(TraceError::MemoryLimit));
        }
        let mut task = super::glass_mesh::Task::new(scene, options)?;
        let mut work = WorkBudget::new(options.budget.limits().max_work_units).map_err(error)?;
        while !task.ready() {
            task.step(scene, options, &mut work, cancel)?;
        }
        general.push(task);
        return Ok(Some(CertifiedOptics { legacy: None, general, work_used: work.used() }));
    }
    let mut task = super::legacy_certificates::Task::new(scene, request, options).map_err(error)?;
    let mut work = WorkBudget::new(options.budget.limits().max_work_units).map_err(error)?;
    while !task.ready() {
        task.step(scene, options, &mut work, cancel).map_err(legacy_error)?;
    }
    // The ordinary image continuation charges the same final proof store.
    work.tick(cancel).map_err(error)?;
    task.into_proof(work.used()).map(Some).map_err(error)
}

/// Strict blocking admission for standalone callers. Native UI preparation must
/// run it beside the window; a header check is never reported as full preflight.
pub fn admit(scene: &ProductScene, request: &ImageRequest, options: &RaytraceOptions) -> job::Result<Admission> {
    let admission = admit_layout(scene, request, options)?;
    if !options.dielectrics.is_empty() {
        certify_for_request(scene, request, options, &CancelToken::default())?;
    }
    Ok(admission)
}

fn selected_workers(options: &RaytraceOptions) -> u16 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        parallel::worker_count(options)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = options;
        1
    }
}

pub(crate) fn admit_layout(scene: &ProductScene, request: &ImageRequest, options: &RaytraceOptions) -> job::Result<Admission> {
    admit_layout_with_workers(scene, request, options, selected_workers(options))
}

fn admit_layout_with_workers(scene: &ProductScene, request: &ImageRequest, options: &RaytraceOptions, workers: u16) -> job::Result<Admission> {
    options.validate()?;
    if !(1..=job::MAX_CPU_THREADS).contains(&workers) {
        return Err(JobError::new(ErrorCode::InvalidRequest, "CPU worker admission requires 1..64 workers"));
    }
    #[cfg(not(target_arch = "wasm32"))]
    if options.threads != 0 && options.threads != workers {
        return Err(JobError::new(ErrorCode::InvalidRequest, "CPU worker admission differs from the explicit selection"));
    }
    #[cfg(target_arch = "wasm32")]
    if workers != 1 || options.threads > 1 {
        return Err(JobError::new(
            ErrorCode::HostUnavailable,
            "browser CPU execution requires Auto or Single; custom native workers are unavailable",
        ));
    }
    let (width, height) = if options.supersampling == 2 {
        job::supersampled_dimensions(request.width, request.height, options.supersampling)?
    } else {
        (request.width, request.height)
    };
    let linear_required = options.denoise
        || options.denoise_diagnostics
        || options.supersampling == 2
        || options.output.as_deref().is_some_and(|output| !output.identity_display() || output.linear_exr);
    if linear_required {
        job::linear_exr::encoded_limit(request.width, request.height)?;
    }

    if let Some(original) = &request.raytrace {
        original.validate()?;
    }
    if request.raytrace.as_ref().unwrap_or(&RaytraceOptions::default()) != options {
        return Err(JobError::new(ErrorCode::InvalidRequest, "raytrace options disagree with image request"));
    }
    if request.integrator != job::Integrator::Raytrace {
        return Err(JobError::new(ErrorCode::InvalidRequest, "raytrace task requires raytrace integrator"));
    }
    if options.path.is_some() && scene.has_transmission() {
        return Err(invalid(if options.volume.as_ref().is_some_and(|v| v.active()) {
            "CPU Atmosphere cannot combine with captured Glass or subsurface scattering"
        } else {
            "CPU opaque path transport cannot discard authored transmission; choose its existing optical transport"
        }));
    }
    if let Some(output) = &options.output {
        output.presentation(options.exposure_ev, request.background, request.transparent_background).validate()?;
    }
    let camera_sampling = options.camera_sampling()?;
    options.budget.validate_image(width, height, camera_sampling)?;
    if let Some(lens) = options.lens {
        lens.validate_camera(&request.camera)?;
    }
    if options.denoise || options.denoise_diagnostics {
        job::validate_cpu_denoise_image(width, height)?;
    }
    super::aging::validate_scene(scene, options)?;
    let map_bytes =
        request.textures.as_ref().map_or(Ok(0), |maps| job::textures::cpu_cache::CpuTextureCache::required_bytes(maps, scene, options))?;
    let mut a = job::image::admit_raytrace_source(scene, request, options.budget, (width, height))?;
    if let Some(caustics) = options.caustics {
        a.caustic_work = caustics.refresh_work(camera_sampling.map_or(0, |sampling| sampling.samples_per_pixel))?;
        if a.caustic_work.is_some_and(|work| work.max_light_path_events > options.budget.limits().max_work_units) {
            return Err(JobError::new(ErrorCode::LimitExceeded, "CPU photon refresh light-path events exceed the selected profile work limit"));
        }
    }
    if let Some(lens) = options.lens.filter(|lens| lens.aperture_radius_mm > 0.0) {
        // Bound every possible pupil/pixel ray against the same captured clip
        // interval used by ImageTask, before allocating or starting transport.
        let camera = &request.camera;
        let radius = if scene.meshes.is_empty() { 10.0 } else { scene.bounds.diagonal().max(10.0) + scene.bounds.center().dist(camera.target) };
        let clip = camera.clip(radius);
        let slope = (camera.half_height() / camera.distance) * (f64::from(width) / f64::from(height)).hypot(1.0)
            + lens.aperture_radius_mm / lens.focus_distance_mm;
        let scale = slope.hypot(1.0);
        let eye = camera.eye();
        let origin_bound = eye.x.abs().max(eye.y.abs()).max(eye.z.abs()) + lens.aperture_radius_mm + clip.0.abs() * scale;
        if !scale.is_finite()
            || !origin_bound.is_finite()
            || origin_bound > super::MAX_COORDINATE_MM
            || (clip.1 - clip.0) * scale > super::MAX_DISTANCE_MM
        {
            return Err(JobError::new(ErrorCode::InvalidRequest, "CPU thin-lens pupil exceeds the bounded ray coordinate or interval envelope"));
        }
    }
    let r = options.reflections.as_ref().filter(|r| !r.bodies.is_empty()).map_or(1, |r| r.samples);
    u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|n| n.checked_mul(u64::from(options.primary_samples)))
        .and_then(|n| n.checked_mul(u64::from(r)))
        .ok_or_else(|| invalid("spatial attempt count overflow"))?;
    for (index, body) in scene.meshes.iter().enumerate() {
        if body.opacity != 1.0 || body.faces.iter().any(|f| f.opacity != 1.0) {
            return Err(JobError::new(ErrorCode::InvalidScene, format!("raytrace requires opaque body and face appearances: {}", body.name)));
        }
        let has_coat = body.surface.is_some_and(|surface| surface.clear_coat().is_some())
            || body.faces.iter().any(|face| face.surface.is_some_and(|surface| surface.clear_coat().is_some()));
        let selected_transport = options.reflections.as_ref().is_some_and(|r| r.bodies.iter().any(|entry| entry.body == body.name))
            || options.anisotropic_faces.iter().flatten().any(|entry| entry.body_index == index)
            || options.dielectrics.iter().any(|entry| entry.body_index == index);
        if has_coat && selected_transport {
            return Err(invalid("CPU reflection, anisotropy and dielectric overrides do not support clear-coat transport"));
        }
    }
    if let Some(reflections) = &options.reflections {
        for override_ in &reflections.bodies {
            let count = scene.meshes.iter().filter(|body| body.name == override_.body).count();
            if count != 1 {
                return Err(JobError::new(ErrorCode::InvalidRequest, "reflection body must resolve to exactly one captured body"));
            }
        }
    }
    anisotropic_faces::prepare(scene, options, || Ok(()))?;
    for input in &options.dielectrics {
        if options.reflections.as_ref().is_some_and(|r| r.bodies.iter().any(|v| v.body == input.body_name))
            || options.anisotropic_faces.iter().flatten().any(|v| v.body_index == input.body_index)
            || options.emissions.iter().flatten().any(|v| v.body_index == input.body_index)
        {
            return Err(invalid("dielectric boundaries cannot also select opaque reflection, anisotropy or emission"));
        }
        if let Some(values) = &options.surface_effects {
            for value in values.iter().filter(|v| v.body == input.body_name) {
                if !value.prepare()?.is_identity() {
                    return Err(invalid("nonidentity surface effects on dielectric boundaries are unsupported"));
                }
            }
        }
    }
    if let Some(values) = &options.surface_effects {
        for value in values {
            let mut bodies = scene.meshes.iter().enumerate().filter(|(_, body)| body.name == value.body);
            let (index, _) = bodies.next().ok_or_else(|| JobError::new(ErrorCode::InvalidRequest, "surface effect body missing"))?;
            if bodies.next().is_some() {
                return Err(JobError::new(ErrorCode::InvalidRequest, "surface effect body must resolve uniquely"));
            }
            if !value.prepare()?.is_identity() && options.anisotropic_faces.iter().flatten().any(|target| target.body_index == index) {
                return Err(JobError::new(ErrorCode::InvalidRequest, "surface effects on a certified anisotropic body are not supported"));
            }
        }
    }
    if let Some(values) = &options.emissions {
        for value in values {
            if scene.meshes.get(value.body_index).is_none_or(|body| body.name != value.body) {
                return Err(invalid("surface emission target does not match captured body index and name"));
            }
        }
    }
    // Bill each task-owned options copy and the prepared emission vector in addition
    // to the unchanged baseline estimate. Record metadata has its independent lease.
    let emission_bytes = options
        .retained_emission_bytes()?
        .checked_add(request.raytrace.as_ref().map_or(Ok(0), RaytraceOptions::retained_emission_bytes)?)
        .and_then(|v| {
            v.checked_add(options.emissions.as_ref().map_or(0, Vec::len) as u64 * std::mem::size_of::<(usize, super::emission::Emission)>() as u64)
        })
        .ok_or_else(|| error(TraceError::MemoryLimit))?;
    // The task owns two independent options copies plus its prepared effect vector.
    // The original request Record remains billed by the same retained_bytes sum.
    let effect_bytes = options
        .retained_surface_effect_bytes()?
        .checked_add(request.raytrace.as_ref().map_or(Ok(0), RaytraceOptions::retained_surface_effect_bytes)?)
        .and_then(|v| {
            v.checked_add(
                options.surface_effects.as_ref().map_or(0, Vec::len) as u64
                    * std::mem::size_of::<(usize, crate::surface_patterns::PreparedEffect)>() as u64,
            )
        })
        .ok_or_else(|| error(TraceError::MemoryLimit))?;
    let legacy_bytes = if options.dielectrics.is_empty() || options.path.is_some() {
        0
    } else {
        certification_bytes(scene, options)?.checked_add(std::mem::size_of::<OpticalTask>() as u64).ok_or_else(|| error(TraceError::MemoryLimit))?
    };
    let optical_bytes = options
        .retained_dielectric_bytes()?
        .checked_add(request.raytrace.as_ref().map_or(Ok(0), RaytraceOptions::retained_dielectric_bytes)?)
        .and_then(|bytes| bytes.checked_add(legacy_bytes))
        .ok_or_else(|| error(TraceError::MemoryLimit))?;
    let aging_bytes = options
        .retained_aging_bytes()?
        .checked_add(request.raytrace.as_ref().map_or(Ok(0), RaytraceOptions::retained_aging_bytes)?)
        .and_then(|v| {
            v.checked_add(if options.aging.as_deref().is_some_and(job::aging::AgingStack::active) { super::aging::Task::required_bytes() } else { 0 })
        })
        .ok_or_else(|| error(TraceError::MemoryLimit))?;
    let adaptive_bytes = options
        .retained_adaptive_bytes()?
        .checked_add(request.raytrace.as_ref().map_or(Ok(0), RaytraceOptions::retained_adaptive_bytes)?)
        .and_then(|v| v.checked_add(if options.adaptive.is_some() { std::mem::size_of::<job::adaptive::SamplingReport>() as u64 } else { 0 }))
        .ok_or_else(|| error(TraceError::MemoryLimit))?;
    let celestial_bytes = options
        .retained_celestial_bytes()?
        .checked_add(request.raytrace.as_ref().map_or(Ok(0), RaytraceOptions::retained_celestial_bytes)?)
        .ok_or_else(|| error(TraceError::MemoryLimit))?;
    let n = usize::try_from(a.triangles).map_err(|_| error(TraceError::MemoryLimit))?;
    let film_bytes = if let Some(progressive) = options.progressive {
        let checkpoint = a.encoded_limit.checked_add(job::FRAME_METADATA_BYTES).ok_or_else(|| error(TraceError::MemoryLimit))?;
        CameraFilm::required_bytes(width, height, progressive, options.adaptive.as_deref(), options.budget)?
            .checked_add(u64::from(progressive.preview_slots()).checked_mul(checkpoint).ok_or_else(|| error(TraceError::MemoryLimit))?)
            .and_then(|v| v.checked_add(if progressive.preview_slots() == 0 { 0 } else { job::FRAME_METADATA_BYTES }))
            .and_then(|v| v.checked_add(std::mem::size_of::<Progressive>() as u64))
            .ok_or_else(|| error(TraceError::MemoryLimit))?
    } else {
        0
    };
    let output_bytes = options.output.as_deref().map_or(Ok(0), |output| {
        output
            .required_bytes(request.width, request.height)?
            .checked_add(2 * std::mem::size_of::<job::OutputOptions>() as u64)
            .ok_or_else(|| error(TraceError::MemoryLimit))
    })?;
    // Trace-grid film/guides/filtering are distinct from final-size finishing,
    // codec output and categorical diagnostics. Account overlapping owned planes.
    let pixels = u64::from(width).checked_mul(u64::from(height)).ok_or_else(|| error(TraceError::MemoryLimit))?;
    let output_pixels = u64::from(request.width).checked_mul(u64::from(request.height)).ok_or_else(|| error(TraceError::MemoryLimit))?;
    let plane_bytes = |count: u64, stride: u64| count.checked_mul(stride).ok_or_else(|| error(TraceError::MemoryLimit));
    let output_already_paid = options.output.as_deref().is_some_and(|output| !output.identity_display() || output.linear_exr);
    let mut denoise_bytes = if options.denoise || options.denoise_diagnostics {
        if options.denoise_diagnostics {
            super::denoise::Guides::required_bytes_with_diagnostics(width, height, true)?
        } else {
            super::denoise::Guides::required_bytes(width, height)?
        }
    } else {
        0
    };
    if linear_required {
        let converted = if output_already_paid { pixels.checked_sub(output_pixels).ok_or_else(|| error(TraceError::MemoryLimit))? } else { pixels };
        let converted_bytes = plane_bytes(converted, 16)?;
        let filter_bytes = if options.denoise { plane_bytes(pixels, 16)? } else { 0 };
        let resolve_bytes = if options.supersampling == 2 { plane_bytes(output_pixels, 17)? } else { 0 };
        denoise_bytes = denoise_bytes
            .checked_add(converted_bytes)
            .and_then(|n| n.checked_add(filter_bytes))
            .and_then(|n| n.checked_add(resolve_bytes))
            .and_then(|n| n.checked_add(if output_already_paid { 0 } else { std::mem::size_of::<job::output::OutputTask>() as u64 }))
            .ok_or_else(|| error(TraceError::MemoryLimit))?;
    }
    if options.denoise_diagnostics {
        denoise_bytes = denoise_bytes
            .checked_add(plane_bytes(output_pixels, 4)?)
            .and_then(|n| n.checked_add(a.encoded_limit))
            .ok_or_else(|| error(TraceError::MemoryLimit))?;
    }
    let general_glass_bytes = if options.path.is_some() && !options.dielectrics.is_empty() {
        certification_bytes(scene, options)?.checked_add(super::origin::Task::required_bytes()).ok_or_else(|| error(TraceError::MemoryLimit))?
    } else {
        0
    };
    let caustics_bytes = options.caustics.map_or(Ok(0), caustics::Task::required_bytes)?;
    #[cfg(not(target_arch = "wasm32"))]
    let parallel_bytes = parallel::Pool::required_bytes(options, request.textures.is_some(), workers)?;
    #[cfg(target_arch = "wasm32")]
    let parallel_bytes = 0;
    let extra = BvhBuilder::required_bytes(n, n, limits(options.budget))
        .map_err(error)?
        .checked_add(a.triangles.checked_mul(std::mem::size_of::<Look>() as u64).ok_or_else(|| error(TraceError::MemoryLimit))?)
        .and_then(|v| v.checked_add(emission_bytes))
        .and_then(|v| v.checked_add(effect_bytes))
        .and_then(|v| v.checked_add(map_bytes))
        .and_then(|v| v.checked_add(aging_bytes))
        .and_then(|v| v.checked_add(optical_bytes))
        .and_then(|v| v.checked_add(general_glass_bytes))
        .and_then(|v| v.checked_add(caustics_bytes))
        .and_then(|v| v.checked_add(parallel_bytes))
        .and_then(|v| v.checked_add(if options.path.is_some() { std::mem::size_of::<indirect_image::State>() as u64 } else { 0 }))
        .and_then(|v| v.checked_add(film_bytes))
        .and_then(|v| v.checked_add(adaptive_bytes))
        .and_then(|v| v.checked_add(celestial_bytes))
        .and_then(|v| v.checked_add(output_bytes))
        .and_then(|v| v.checked_add(denoise_bytes))
        // The resumable pixel owns two traversal cursors. Its fallible, fixed
        // heap slot is additional to the unchanged host/task-state allowance.
        .and_then(|v| v.checked_add(PendingPixelSlot::required_bytes()))
        .and_then(|v| {
            v.checked_add(8192 + if options.reflections.is_some() || options.anisotropic_faces.iter().any(Option::is_some) { 32768 } else { 0 })
        })
        .and_then(|v| {
            v.checked_add(options.area_lights.as_ref().map_or(0, Vec::capacity) as u64 * std::mem::size_of::<job::RectangleLightOptions>() as u64)
        })
        .and_then(|v| {
            v.checked_add(options.punctual_lights.as_ref().map_or(0, Vec::capacity) as u64 * std::mem::size_of::<job::PunctualLightOptions>() as u64)
        })
        .ok_or_else(|| error(TraceError::MemoryLimit))?;
    // Keep the conservative raster pixel/codec estimate; add every new owned buffer.
    a.preparation_bytes = a.preparation_bytes.checked_add(extra).ok_or_else(|| error(TraceError::MemoryLimit))?;
    a.estimated_total_bytes = a.estimated_total_bytes.checked_add(extra).ok_or_else(|| error(TraceError::MemoryLimit))?;
    if a.estimated_total_bytes > options.budget.limits().max_job_bytes {
        return Err(error(TraceError::MemoryLimit));
    }
    a.cpu_workers = Some(workers);
    Ok(a)
}

struct PendingPixel {
    surface_cursor: usize,
    surface_index: Option<usize>,
    surface_ready: bool,
    emission_cursor: usize,
    punctual_index: usize,
    punctual_shadow: bool,
    punctual_ready: bool,
    punctual_value: Vec3,
    area_light: usize,
    area_index: u32,
    area_shadow: bool,
    area_ready: bool,
    area_value: Vec3,

    /// Proof from the primary generator, revoked for any selected reflector.
    /// A jittered/time-varying primary generator MUST supply false.
    repeatable_primary: bool,
    environment_sample: u32,
    environment_sum: Vec3,
    environment_cursor: Option<TraceCursor>,
    environment_value: Vec3,
    ray: Ray,
    cursor: TraceCursor,
    hit: Option<super::Hit>,
    visibility: [bool; 8],
    light: usize,
    bounce: u8,
    throughput: Vec3,
    radiance: Vec3,
}

/// One reusable, fallibly allocated continuation. The Vec header stays in the
/// 4096-byte host slot; its exact single-element capacity is billed by admit.
/// Keeping an empty slot between samples avoids any per-ray allocation.
struct PendingPixelSlot(Vec<PendingPixel>);

impl PendingPixelSlot {
    fn required_bytes() -> u64 {
        std::mem::size_of::<PendingPixel>() as u64
    }

    fn new(cancel: &CancelToken) -> job::Result<Self> {
        cancel.check()?;
        let mut slot = Vec::new();
        slot.try_reserve_exact(1).map_err(|_| error(TraceError::AllocationFailed))?;
        if slot.capacity() != 1 {
            return Err(error(TraceError::MemoryLimit));
        }
        cancel.check()?;
        Ok(Self(slot))
    }

    fn as_ref(&self) -> Option<&PendingPixel> {
        self.0.first()
    }

    fn as_mut(&mut self) -> Option<&mut PendingPixel> {
        self.0.first_mut()
    }

    fn is_none(&self) -> bool {
        self.0.is_empty()
    }

    #[cfg(test)]
    fn is_some(&self) -> bool {
        !self.is_none()
    }

    fn insert(&mut self, value: PendingPixel) -> job::Result<()> {
        if self.0.capacity() != 1 || !self.0.is_empty() {
            return Err(invalid("pending pixel slot is unavailable"));
        }
        // Capacity is already admitted and reserved: this cannot allocate.
        self.0.push(value);
        Ok(())
    }

    fn clear(&mut self) {
        self.0.clear();
    }

    fn release(&mut self) {
        self.0 = Vec::new();
    }
}

#[derive(Clone, Copy)]
struct OpticalPath {
    ray: Ray,
    throughput: Vec3,
    medium: Option<usize>,
    /// Boundary or opaque reflection position, before the numerical spawn offset.
    segment_start: Vec3,
    boundaries: u8,
    /// Opaque reflection depth is independent of the dielectric interface count.
    bounce: u8,
}

struct DielectricPixel {
    path: OpticalPath,
    stack: [Option<OpticalPath>; MAX_BOUNDARIES as usize + 1],
    stack_len: usize,
    paths: u16,
    primary_check: usize,
    selector: usize,
    selected: Option<usize>,
    ready: bool,
}

/// One fallibly reserved optional slot. Branches do not copy PendingPixel, own
/// lighting state, encode pixels, or allocate outside the existing job reservation.
struct OpticalTask {
    certificates: [Option<Certificate>; MAX_OVERRIDES],
    pending: Option<DielectricPixel>,
    preparation: Vec<super::legacy_certificates::Task>,
}

impl DielectricPixel {
    fn new(ray: Ray) -> Self {
        Self {
            path: OpticalPath { ray, throughput: Vec3::new(1.0, 1.0, 1.0), medium: None, segment_start: ray.origin(), boundaries: 0, bounce: 0 },
            stack: [None; MAX_BOUNDARIES as usize + 1],
            stack_len: 0,
            paths: 1,
            primary_check: 0,
            selector: 0,
            selected: None,
            ready: false,
        }
    }

    fn push(&mut self, path: OpticalPath) -> job::Result<()> {
        if !bounded_radiance(path.throughput) {
            return Err(invalid("dielectric branch throughput exceeds finite bound"));
        }
        if path.throughput == Vec3::ZERO {
            return Ok(());
        }
        if self.paths >= MAX_PATHS {
            return Err(error(TraceError::WorkLimit));
        }
        *self.stack.get_mut(self.stack_len).ok_or_else(|| error(TraceError::MemoryLimit))? = Some(path);
        self.stack_len += 1;
        self.paths += 1;
        Ok(())
    }

    fn advance(&mut self) -> job::Result<bool> {
        if self.stack_len == 0 {
            return Ok(false);
        }
        self.stack_len -= 1;
        self.path = self.stack.get_mut(self.stack_len).and_then(Option::take).ok_or_else(|| invalid("optical path stack"))?;
        self.selector = 0;
        self.selected = None;
        self.ready = false;
        Ok(true)
    }
}

/// Reset only per-hit transport state; retain the shared sum across Fresnel leaves.
fn resume_optical_path(pending: &mut PendingPixel, path: OpticalPath) {
    pending.ray = path.ray;
    pending.throughput = path.throughput;
    pending.bounce = path.bounce;
    pending.repeatable_primary = false;
    pending.cursor = TraceCursor::new(path.ray, false);
    pending.hit = None;
    pending.surface_cursor = 0;
    pending.surface_index = None;
    pending.surface_ready = false;
    pending.emission_cursor = 0;
    pending.light = 0;
    pending.visibility = [true; 8];
    pending.punctual_index = 0;
    pending.punctual_shadow = false;
    pending.punctual_ready = false;
    pending.punctual_value = Vec3::ZERO;
    pending.area_light = 0;
    pending.area_index = 0;
    pending.area_shadow = false;
    pending.area_ready = false;
    pending.area_value = Vec3::ZERO;
    pending.environment_sample = 0;
    pending.environment_sum = Vec3::ZERO;
    pending.environment_cursor = None;
    pending.environment_value = Vec3::ZERO;
}

#[path = "image_caustics.rs"]
mod caustics;
#[path = "image_indirect.rs"]
mod indirect_image;
#[path = "image_transport.rs"]
mod transport;
use transport::TransportContext;
#[path = "image_direct.rs"]
mod direct;
#[cfg(not(target_arch = "wasm32"))]
#[path = "image_parallel.rs"]
mod parallel;

pub struct ImageTask {
    #[cfg(not(target_arch = "wasm32"))]
    parallel: Vec<parallel::Pool>,
    denoise_diagnostics: Option<job::DenoiseDiagnostics>,
    denoise_mask: Option<Vec<u8>>,
    output_width: u32,
    output_height: u32,
    maps: Vec<job::textures::cpu_cache::CpuTextureCache>,
    aging: Vec<super::aging::Task>,
    path: Vec<indirect_image::State>,
    caustics: Vec<caustics::Task>,
    punctual_diagnostics: job::PunctualDiagnostics,
    progressive: Option<Progressive>,
    sampling: Vec<job::adaptive::SamplingReport>,
    denoise: Vec<super::denoise::Guides>,
    optical: Vec<OpticalTask>,
    glass: Vec<super::glass_mesh::Task>,
    anisotropic_targets: [Option<PlanarTarget>; 2],
    environment: Option<crate::lighting::environment_input::EnvironmentState>,
    scene: ProductScene,
    request: ImageRequest,
    options: RaytraceOptions,
    cancel: CancelToken,
    admission: Admission,
    lights: Lighting,
    emissions: Vec<(usize, super::emission::Emission)>,
    effects: Vec<(usize, crate::surface_patterns::PreparedEffect)>,
    work: WorkBudget,
    clip: (f64, f64),
    pupil_sample: Option<[f64; 2]>,
    triangles: Vec<Triangle>,
    looks: Vec<Look>,
    body: usize,
    triangle: usize,
    /// The current body's face looks as (face, index into `faces`), sorted by
    /// face with only the first entry per face, so each triangle is one lookup.
    face_looks: Vec<(u32, usize)>,
    face_looks_ready: bool,
    builder: Option<BvhBuilder>,
    bvh: Option<Bvh>,
    build_used: u64,
    pixels: Vec<u8>,
    linear: Vec<[f32; 4]>,
    output: Vec<job::output::OutputTask>,
    linear_exr: Option<Vec<u8>>,
    pixel: usize,
    sample: u8,
    spatial: u8,
    primary_coverage: u32,
    spatial_radiance: Vec3,
    finish_spatial: bool,
    emit_spatial_pixel: bool,
    pixel_radiance: Vec3,
    pending: PendingPixelSlot,
    encoder: Option<job::encoding::PngEncoding>,
    done: bool,
}
// Same host slot: a future feature build must prove the actual target layout.
const _: [(); 1] = [(); (std::mem::size_of::<ImageTask>() <= 4096) as usize];

/// CPU continuations. Preview bytes are complete PNGs but acquire a transferable
/// host lease and identity only when the owning host publishes them.
pub enum ImagePoll {
    Pending(Progress),
    Preview { png: Vec<u8>, samples_per_pixel: u32 },
    Complete(Vec<u8>),
}

impl ImageTask {
    pub fn denoise_diagnostics(&self) -> Option<job::DenoiseDiagnostics> {
        self.denoise_diagnostics
    }
    pub fn take_denoise_mask(&mut self) -> Option<Vec<u8>> {
        self.denoise_mask.take()
    }
    pub fn sampling_report(&self) -> Option<job::adaptive::SamplingReport> {
        self.sampling.first().cloned()
    }
    pub fn punctual_diagnostics(&self) -> Option<job::PunctualDiagnostics> {
        (!self.punctual_diagnostics.is_empty()).then_some(self.punctual_diagnostics)
    }

    pub fn new(scene: ProductScene, request: ImageRequest, cancel: CancelToken, options: RaytraceOptions) -> job::Result<Self> {
        Self::new_with_environment(scene, request, cancel, options, None)
    }
    pub fn new_with_environment(
        scene: ProductScene,
        request: ImageRequest,
        cancel: CancelToken,
        options: RaytraceOptions,
        input: Option<crate::lighting::environment_input::EnvironmentInput>,
    ) -> job::Result<Self> {
        Self::new_prepared(scene, request, cancel, options, input, None)
    }
    pub(crate) fn new_prepared(
        scene: ProductScene,
        request: ImageRequest,
        cancel: CancelToken,
        options: RaytraceOptions,
        input: Option<crate::lighting::environment_input::EnvironmentInput>,
        proof: Option<CertifiedOptics>,
    ) -> job::Result<Self> {
        let workers = selected_workers(&options);
        Self::new_prepared_with_workers(scene, request, cancel, options, input, proof, workers)
    }

    pub(crate) fn new_prepared_with_workers(
        scene: ProductScene,
        request: ImageRequest,
        cancel: CancelToken,
        options: RaytraceOptions,
        input: Option<crate::lighting::environment_input::EnvironmentInput>,
        proof: Option<CertifiedOptics>,
        workers: u16,
    ) -> job::Result<Self> {
        cancel.check()?;
        let mut admission = admit_layout_with_workers(&scene, &request, &options, workers)?;
        let (output_width, output_height) = (request.width, request.height);
        let mut request = request;
        if options.supersampling == 2 {
            (request.width, request.height) = job::supersampled_dimensions(output_width, output_height, options.supersampling)?;
        }
        if options.dielectrics.is_empty() && proof.is_some() {
            return Err(invalid("unexpected optical proof without selected boundaries"));
        }
        let certified_work = proof.as_ref().map_or(0, CertifiedOptics::work_used);
        let (legacy_proof, mut glass) = if let Some(proof) = proof {
            if options.path.is_some() {
                if proof.legacy.is_some() || proof.general.len() != 1 || !proof.general.first().is_some_and(|task| task.ready()) {
                    return Err(invalid("unexpected or incomplete general optical proof"));
                }
                (None, proof.general)
            } else {
                (Some(proof.into_legacy()?), Vec::new())
            }
        } else {
            (None, Vec::new())
        };
        let mut optical = Vec::new();
        if !options.dielectrics.is_empty() && options.path.is_none() {
            let mut preparation = Vec::new();
            let certificates = if let Some(values) = legacy_proof {
                values
            } else {
                preparation.try_reserve_exact(1).map_err(|_| error(TraceError::AllocationFailed))?;
                if preparation.capacity() != 1 {
                    return Err(error(TraceError::MemoryLimit));
                }
                cancel.check()?;
                preparation.push(super::legacy_certificates::Task::new(&scene, &request, &options).map_err(error)?);
                [None; MAX_OVERRIDES]
            };
            optical.try_reserve_exact(1).map_err(|_| error(TraceError::AllocationFailed))?;
            if optical.capacity() != 1 {
                return Err(error(TraceError::MemoryLimit));
            }
            optical.push(OpticalTask { certificates, pending: None, preparation });
        } else if legacy_proof.is_some() {
            return Err(invalid("unexpected legacy optical proof"));
        }
        if options.path.is_some() && !options.dielectrics.is_empty() && glass.is_empty() {
            glass.try_reserve_exact(1).map_err(|_| error(TraceError::AllocationFailed))?;
            if glass.capacity() != 1 {
                return Err(error(TraceError::MemoryLimit));
            }
            glass.push(super::glass_mesh::Task::new(&scene, &options)?);
        }
        if input.is_some() {
            options.validate_environment_reflections(&scene)?;
        }
        let extra =
            input.as_ref().map_or(Ok(0), |e| e.required_bytes_with_importance(options.environment_importance_sampling)).map_err(environment_error)?;
        admission.estimated_total_bytes = admission
            .estimated_total_bytes
            .checked_add(extra)
            .filter(|v| *v <= options.budget.limits().max_job_bytes)
            .ok_or_else(|| error(TraceError::MemoryLimit))?;
        admission.preparation_bytes = admission.preparation_bytes.checked_add(extra).ok_or_else(|| error(TraceError::MemoryLimit))?;
        let environment = input
            .map(|e| e.begin(options.environment_importance_sampling, options.caustics.is_some_and(|c| c.power_weighted_sources && c.environment)))
            .transpose()
            .map_err(environment_error)?;
        let lights = lighting(&options)?;
        let mut work = WorkBudget::new(options.budget.limits().max_work_units).map_err(error)?;
        work.charge(certified_work, &cancel).map_err(error)?;
        let anisotropic_targets = anisotropic_faces::prepare(&scene, &options, || work.tick(&cancel).map_err(error))?;
        let mut effects = Vec::new();
        if let Some(values) = &options.surface_effects {
            effects.try_reserve_exact(values.len()).map_err(|_| error(TraceError::AllocationFailed))?;
            if effects.capacity() != values.len() {
                return Err(error(TraceError::MemoryLimit));
            }
            for value in values {
                work.tick(&cancel).map_err(error)?;
                let effect = value.prepare()?;
                let index = scene
                    .meshes
                    .iter()
                    .position(|body| body.name == value.body)
                    .ok_or_else(|| JobError::new(ErrorCode::InvalidRequest, "surface effect body missing"))?;
                // Identity is omitted to preserve the existing pixel path and bytes.
                if !effect.is_identity() {
                    effects.push((index, effect));
                }
            }
        }
        let mut emissions = Vec::new();
        if let Some(values) = &options.emissions {
            emissions.try_reserve_exact(values.len()).map_err(|_| error(TraceError::AllocationFailed))?;
            if emissions.capacity() != values.len() {
                return Err(error(TraceError::MemoryLimit));
            }
            for value in values {
                work.tick(&cancel).map_err(error)?;
                // Skip zero terms to preserve the baseline floating-point path exactly.
                if value.strength != 0.0 && value.color.iter().any(|v| *v != 0.0) {
                    emissions.push((value.body_index, super::emission::Emission::new(value)?));
                }
            }
        }
        let radius =
            if scene.meshes.is_empty() { 10.0 } else { scene.bounds.diagonal().max(10.0) + scene.bounds.center().dist(request.camera.target) };
        if !radius.is_finite() {
            return Err(invalid("invalid product bounds"));
        }
        let clip = request.camera.clip(radius);
        let mut triangles = Vec::new();
        let mut looks = Vec::new();
        let mut pixels = Vec::new();
        triangles.try_reserve_exact(admission.triangles as usize).map_err(|_| error(TraceError::AllocationFailed))?;
        looks.try_reserve_exact(admission.triangles as usize).map_err(|_| error(TraceError::AllocationFailed))?;
        pixels.try_reserve_exact(output_width as usize * output_height as usize * 4).map_err(|_| error(TraceError::AllocationFailed))?;
        // Allocator capacities are checked, not assumed equal to requested capacities.
        let actual = BvhBuilder::required_bytes(admission.triangles as usize, triangles.capacity(), limits(options.budget)).map_err(error)?;
        if actual != BvhBuilder::required_bytes(admission.triangles as usize, admission.triangles as usize, limits(options.budget)).map_err(error)?
            || looks.capacity() != admission.triangles as usize
            || pixels.capacity() != output_width as usize * output_height as usize * 4
        {
            return Err(error(TraceError::MemoryLimit));
        }
        let mut output = Vec::new();
        if options.denoise
            || options.denoise_diagnostics
            || options.supersampling == 2
            || options.output.as_deref().is_some_and(|settings| !settings.identity_display() || settings.linear_exr)
        {
            output.try_reserve_exact(1).map_err(|_| error(TraceError::AllocationFailed))?;
            if output.capacity() != 1 {
                return Err(error(TraceError::MemoryLimit));
            }
        }
        let mut path = Vec::new();
        if options.path.is_some() {
            cancel.check()?;
            path.try_reserve_exact(1).map_err(|_| error(TraceError::AllocationFailed))?;
            if path.capacity() != 1 {
                return Err(error(TraceError::MemoryLimit));
            }
            path.push(indirect_image::State::default());
        }
        let mut denoise = Vec::new();
        if options.denoise || options.denoise_diagnostics {
            denoise.try_reserve_exact(1).map_err(|_| error(TraceError::AllocationFailed))?;
            if denoise.capacity() != 1 {
                return Err(error(TraceError::MemoryLimit));
            }
            denoise.push(if options.denoise_diagnostics {
                super::denoise::Guides::new_with_diagnostics(request.width, request.height, true)?
            } else {
                super::denoise::Guides::new(request.width, request.height)?
            });
        }
        let mut maps = Vec::new();
        if let Some(resources) = &request.textures {
            maps.try_reserve_exact(1).map_err(|_| error(TraceError::AllocationFailed))?;
            if maps.capacity() != 1 {
                return Err(error(TraceError::MemoryLimit));
            }
            maps.push(job::textures::cpu_cache::CpuTextureCache::new(resources.clone(), &cancel)?);
        }
        let mut aging = Vec::new();
        if options.aging.as_deref().is_some_and(job::aging::AgingStack::active) {
            cancel.check()?;
            aging.try_reserve_exact(1).map_err(|_| error(TraceError::AllocationFailed))?;
            if aging.capacity() != 1 {
                return Err(error(TraceError::MemoryLimit));
            }
            aging.push(super::aging::Task::default());
        }
        let pending = PendingPixelSlot::new(&cancel)?;
        let budget = options.budget;
        let mut sampling = Vec::new();
        if options.adaptive.is_some() {
            cancel.check()?;
            sampling.try_reserve_exact(1).map_err(|_| error(TraceError::AllocationFailed))?;
            if sampling.capacity() != 1 {
                return Err(error(TraceError::MemoryLimit));
            }
        }
        let adaptive = options.adaptive.as_deref();
        let target = options.preview_target()?;
        let progressive = options
            .progressive
            .map(|options| -> job::Result<Progressive> {
                let bytes = CameraFilm::required_bytes(request.width, request.height, options, adaptive, budget)?;
                let film = CameraFilm::new(request.width, request.height, options, adaptive, budget, bytes, &cancel)?;
                Ok(Progressive { film, options, target, resolving: false, resolve_pixel: 0 })
            })
            .transpose()?;
        let mut caustics = Vec::new();
        if let Some(options) = options.caustics {
            caustics.try_reserve_exact(1).map_err(|_| error(TraceError::AllocationFailed))?;
            if caustics.capacity() != 1 {
                return Err(error(TraceError::MemoryLimit));
            }
            caustics.push(caustics::Task::new(options)?);
        }
        Ok(Self {
            #[cfg(not(target_arch = "wasm32"))]
            parallel: Vec::new(),
            denoise_diagnostics: None,
            denoise_mask: None,
            output_width,
            output_height,
            maps,
            aging,
            path,
            caustics,
            punctual_diagnostics: Default::default(),
            progressive,
            sampling,
            denoise,
            optical,
            glass,
            anisotropic_targets,
            environment,
            scene,
            request,
            options,
            cancel,
            admission,
            lights,
            emissions,
            effects,
            clip,
            pupil_sample: None,
            work,
            triangles,
            looks,
            body: 0,
            triangle: 0,
            face_looks: Vec::new(),
            face_looks_ready: false,
            builder: None,
            bvh: None,
            build_used: 0,
            pixels,
            linear: Vec::new(),
            output,
            linear_exr: None,
            pixel: 0,
            sample: 0,
            spatial: 0,
            primary_coverage: 0,
            spatial_radiance: Vec3::ZERO,
            finish_spatial: false,
            emit_spatial_pixel: false,
            pixel_radiance: Vec3::ZERO,
            pending,
            encoder: None,
            done: false,
        })
    }
    pub fn progress(&self) -> Progress {
        let (phase, done, total) = if self.optical.first().is_some_and(|optical| !optical.preparation.is_empty()) {
            (Phase::Preparing, 0, 1)
        } else if self.done {
            (Phase::Completed, 1, 1)
        } else if !self.output.is_empty() {
            (Phase::Encoding, 0, u64::from(self.request.width) * u64::from(self.request.height))
        } else if let Some(e) = &self.encoder {
            (Phase::Encoding, e.row() as u64, self.request.height as u64)
        } else if let Some(maps) = self.maps.first().filter(|maps| !maps.ready()) {
            let (done, total) = maps.preparation_rows();
            (Phase::Preparing, done, total)
        } else if self.glass.first().is_some_and(|glass| !glass.ready()) {
            (Phase::Preparing, 0, 1)
        } else if self.bvh.is_some() && self.caustics.first().is_some_and(caustics::Task::initial_preparation) {
            let (done, total) = self.caustics.first().map_or((0, 1), caustics::Task::progress);
            (Phase::Preparing, done, total)
        } else if self.bvh.is_some() && self.denoise.first().is_some_and(|guides| !guides.ready()) {
            (
                Phase::Preparing,
                self.denoise.first().map_or(0, |guides| guides.completed_pixels() as u64),
                u64::from(self.request.width) * u64::from(self.request.height),
            )
        } else if let Some(progressive) = &self.progressive {
            let count = u64::from(self.request.width) * u64::from(self.request.height);
            if !progressive.film.ready() {
                (Phase::Preparing, progressive.film.initialized() as u64, count)
            } else if progressive.film.resetting() {
                (Phase::Preparing, progressive.film.reset_progress() as u64, count)
            } else if progressive.resolving {
                (Phase::Encoding, progressive.resolve_pixel as u64, count)
            } else if self.bvh.is_some() {
                let partial = if self.pixel as u64 == count { 0 } else { self.pixel as u64 };
                (Phase::Running, u64::from(progressive.film.samples()) * count + partial, u64::from(progressive.target) * count)
            } else {
                (Phase::Preparing, self.triangles.len() as u64, self.admission.triangles)
            }
        } else if self.bvh.is_some() {
            (Phase::Running, self.pixel as u64, self.request.width as u64 * self.request.height as u64)
        } else {
            (Phase::Preparing, self.triangles.len() as u64, self.admission.triangles)
        };
        Progress { phase, done, total }
    }
    fn prepare_one(&mut self) -> job::Result<()> {
        self.work.tick(&self.cancel).map_err(error)?;
        let Some(body) = self.scene.meshes.get(self.body) else {
            let triangles = std::mem::take(&mut self.triangles);
            let expected = BvhBuilder::required_bytes(triangles.len(), triangles.capacity(), limits(self.options.budget)).map_err(error)?;
            let builder = BvhBuilder::new(triangles, limits(self.options.budget), &self.cancel).map_err(error)?;
            if builder.admitted_bytes() > expected {
                return Err(error(TraceError::MemoryLimit));
            }
            self.builder = Some(builder);
            return Ok(());
        };
        let Some(indices) = body.mesh.triangles.get(self.triangle) else {
            self.body += 1;
            self.triangle = 0;
            self.face_looks_ready = false;
            return Ok(());
        };
        if !self.face_looks_ready {
            // Built once per body instead of scanning `faces` for every triangle.
            // The first entry for a face wins, as the scan this replaces did.
            self.work.charge(body.faces.len() as u64, &self.cancel).map_err(error)?;
            self.face_looks.clear();
            self.face_looks.try_reserve_exact(body.faces.len()).map_err(|_| error(TraceError::AllocationFailed))?;
            self.face_looks.extend(body.faces.iter().enumerate().map(|(index, look)| (look.face, index)));
            self.face_looks.sort_unstable();
            self.face_looks.dedup_by_key(|entry| entry.0);
            self.face_looks_ready = true;
        }
        let mut vertices = [Vec3::ZERO; 3];
        let mut normals = [Vec3::ZERO; 3];
        for ((p, n), i) in vertices.iter_mut().zip(normals.iter_mut()).zip(indices) {
            *p = *body.mesh.positions.get(*i as usize).ok_or_else(|| invalid("triangle vertex index"))?;
            *n = body.mesh.normals.get(*i as usize).copied().unwrap_or(Vec3::ZERO);
        }
        let face = body.mesh.tri_face.get(self.triangle).copied();
        let mut color = body.color;
        let mut surface = body.surface;
        let look = face
            .and_then(|face| self.face_looks.binary_search_by_key(&face, |entry| entry.0).ok())
            .and_then(|at| self.face_looks.get(at))
            .and_then(|entry| body.faces.get(entry.1));
        if let Some(look) = look {
            color = look.color;
            surface = look.surface;
        }
        let surface = match surface {
            Some(s) => s,
            None => OpaqueSurface::new(Vec3::new(srgb8_to_linear(color[0]), srgb8_to_linear(color[1]), srgb8_to_linear(color[2])), 0.0, 0.7, 1.5)
                .map_err(|_| invalid("invalid scalar surface"))?,
        };
        self.triangles.push(Triangle::new(vertices).map_err(error)?);
        self.looks.push(Look {
            body_index: self.body,
            face,
            has_face_override: look.is_some(),
            surface: surface.with_metal_energy_compensation(self.options.metal_energy_compensation),
            normals,
            vertices,
        });
        self.triangle += 1;
        Ok(())
    }
    /// Move the final native raw companion once. Provisional passes never populate it.
    pub fn take_linear_exr(&mut self) -> Option<Vec<u8>> {
        self.linear_exr.take()
    }
    fn preserves_display(&self) -> bool {
        !self.options.denoise && self.options.supersampling == 1 && self.options.output.as_deref().is_none_or(|o| o.identity_display())
    }
    fn needs_linear_pass(&self) -> bool {
        self.options.denoise
            || self.options.supersampling == 2
            || self.options.denoise_diagnostics
            || self
                .options
                .output
                .as_deref()
                .is_some_and(|o| !o.identity_display() || (o.linear_exr && self.progressive.as_ref().is_none_or(|p| p.film.samples() == p.target)))
    }
    fn append_linear(&mut self, radiance: Vec3, covered: u32, samples: u32) -> job::Result<()> {
        if !self.needs_linear_pass() {
            return Ok(());
        }
        if samples == 0 || covered > samples || !bounded_radiance(radiance) || (covered == 0 && radiance != Vec3::ZERO) {
            return Err(invalid("invalid raw CPU radiance or coverage"));
        }
        self.work.tick(&self.cancel).map_err(error)?;
        let count = self.request.width as usize * self.request.height as usize;
        if self.linear.capacity() == 0 {
            self.linear.try_reserve_exact(count).map_err(|_| error(TraceError::AllocationFailed))?;
        }
        if self.linear.capacity() != count || self.linear.len() >= count {
            return Err(error(TraceError::MemoryLimit));
        }
        let pixel = [radiance.x as f32, radiance.y as f32, radiance.z as f32, (f64::from(covered) / f64::from(samples)) as f32];
        if pixel.iter().any(|v| !v.is_finite() || *v < 0.0) || pixel[3] > 1.0 {
            return Err(invalid("raw CPU float conversion exceeds finite bounds"));
        }
        self.cancel.check()?;
        self.linear.push(pixel);
        Ok(())
    }
    fn start_output(&mut self) -> job::Result<()> {
        if self.output.capacity() != 1 || !self.output.is_empty() {
            return Err(invalid("CPU output slot is not admitted or available"));
        }
        let settings = match self.options.output.as_deref() {
            Some(settings) => *settings,
            None if self.options.denoise || self.options.denoise_diagnostics || self.options.supersampling == 2 => Default::default(),
            None => return Err(invalid("missing CPU output settings")),
        };
        let final_pass = self.progressive.as_ref().is_none_or(|p| p.film.samples() == p.target);
        let coverage_samples = match &self.progressive {
            Some(progressive) => progressive.film.samples(),
            None if self.options.primary_samples == 1 => 1,
            None => u32::from(self.options.primary_samples)
                .checked_mul(u32::from(self.sample_count()))
                .ok_or_else(|| invalid("CPU spatial coverage sample count overflow"))?,
        };
        let presentation = settings.presentation(self.options.exposure_ev, self.request.background, self.request.transparent_background);
        let task = job::output::OutputTask::new_with_quality(
            self.output_width,
            self.output_height,
            self.admission.encoded_limit,
            std::mem::take(&mut self.linear),
            std::mem::take(&mut self.pixels),
            presentation,
            self.preserves_display(),
            final_pass && settings.linear_exr,
            self.options.denoise,
            self.options.denoise_settings,
            self.options.supersampling,
            final_pass && self.options.denoise_diagnostics,
            coverage_samples,
            self.cancel.clone(),
            &mut |units| self.work.charge(units, &self.cancel).map_err(error),
        )?;
        self.output.push(task);
        Ok(())
    }
    fn complete_png(&mut self, png: Vec<u8>, charged: bool) -> job::Result<ImagePoll> {
        self.cancel.check()?;
        if let Some(progressive) = &mut self.progressive {
            if !charged {
                self.work.charge(png.len() as u64, &self.cancel).map_err(error)?;
            }
            let samples_per_pixel = progressive.film.samples();
            if samples_per_pixel < progressive.target {
                progressive.resolving = false;
                progressive.resolve_pixel = 0;
                self.encoder = None;
                self.pixels.clear();
                self.pixel = 0;
                return Ok(ImagePoll::Preview { png, samples_per_pixel });
            }
        }
        if let Some(report) = self.progressive.as_ref().map(|p| p.film.report()).transpose()?.flatten() {
            if self.sampling.capacity() != 1 || !self.sampling.is_empty() {
                return Err(invalid("CPU adaptive report slot is not admitted or available"));
            }
            self.cancel.check()?;
            self.sampling.push(report);
        }
        self.release_completed_work()?;
        Ok(ImagePoll::Complete(png))
    }
    fn sample_count(&self) -> u8 {
        self.options.reflections.as_ref().filter(|r| !r.bodies.is_empty()).map_or(1, |r| r.samples)
    }
    fn finish_sample(&mut self, radiance: Vec3) -> job::Result<()> {
        direct::finish_sample(self, radiance)
    }
    fn pixel_one(&mut self) -> job::Result<()> {
        direct::step(self)
    }

    /// Legacy final-only callers keep their exact contract. Progressive callers
    /// must consume `step_with_previews`, so no checkpoint is silently discarded.
    pub fn step(&mut self, units: usize) -> job::Result<TaskPoll> {
        self.cancel.check()?;
        if self.progressive.is_some() {
            return Err(JobError::new(ErrorCode::InvalidRequest, "CPU progressive tasks require step_with_previews"));
        }
        match self.step_with_previews(units)? {
            ImagePoll::Pending(progress) => Ok(TaskPoll::Pending(progress)),
            ImagePoll::Complete(png) => Ok(TaskPoll::Complete(png)),
            ImagePoll::Preview { .. } => Err(invalid("fixed image produced a progressive checkpoint")),
        }
    }

    fn release_completed_work(&mut self) -> job::Result<()> {
        self.done = true;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.parallel = Vec::new();
        }
        self.progressive = None;
        self.anisotropic_targets = [None, None];
        self.options.anisotropic_faces = [None, None];
        self.environment = None;
        self.scene.meshes = Vec::new();
        self.looks = Vec::new();
        self.bvh = None;
        self.builder = None;
        self.triangles = Vec::new();
        self.pixels = Vec::new();
        self.linear = Vec::new();
        self.output = Vec::new();
        self.denoise = Vec::new();
        self.options.output = None;
        self.options.adaptive = None;
        self.options.volume = None;
        self.pending.release();
        self.path = Vec::new();
        self.caustics = Vec::new();
        self.request.raytrace = None;
        self.options.lights = None;
        self.options.emissions = None;
        self.emissions = Vec::new();
        self.options.surface_effects = None;
        self.maps = Vec::new();
        self.request.textures = None;
        self.options.aging = None;
        self.aging = Vec::new();
        self.options.dielectrics = Vec::new();
        self.options.environment = None;
        self.options.sun = None;
        self.options.sky = None;
        self.optical = Vec::new();
        self.glass = Vec::new();
        self.effects = Vec::new();
        self.options.punctual_lights = None;
        self.options.area_lights = None;
        self.options.reflections = None;
        self.lights = Lighting::new(Vec::new()).map_err(error)?;
        self.encoder = None;
        Ok(())
    }

    fn resolve_progressive_one(&mut self) -> job::Result<()> {
        self.work.tick(&self.cancel).map_err(error)?;
        let progressive = self.progressive.as_ref().ok_or_else(|| invalid("missing progressive resolve state"))?;
        let count = self.request.width as usize * self.request.height as usize;
        if progressive.resolve_pixel < count {
            let (rgb, covered, samples) = progressive.film.pixel(progressive.resolve_pixel)?;
            let radiance = Vec3::new(rgb[0], rgb[1], rgb[2]);
            self.append_linear(radiance, covered, samples)?;
            if self.preserves_display() {
                let rgba = composite_spatial_alpha(
                    radiance,
                    covered,
                    samples,
                    self.request.background,
                    self.options.exposure_ev,
                    self.request.transparent_background,
                )?;
                self.cancel.check()?;
                self.pixels.extend_from_slice(&rgba);
            }
            self.progressive.as_mut().ok_or_else(|| invalid("missing progressive resolve state"))?.resolve_pixel += 1;
        } else if self.needs_linear_pass() {
            self.start_output()?;
        } else {
            let capacity = usize::try_from(self.admission.encoded_limit).map_err(|_| error(TraceError::MemoryLimit))?;
            let mut output = Vec::new();
            output.try_reserve_exact(capacity).map_err(|_| error(TraceError::AllocationFailed))?;
            if output.capacity() != capacity {
                return Err(error(TraceError::MemoryLimit));
            }
            self.cancel.check()?;
            self.encoder = Some(job::encoding::PngEncoding::with_output(
                self.request.width,
                self.request.height,
                self.admission.encoded_limit,
                self.cancel.clone(),
                output,
            )?);
        }
        Ok(())
    }

    /// At most four continuations/encoding rows. One cumulative work budget spans
    /// geometry preparation, every absolute camera sample and every checkpoint.
    pub fn step_with_previews(&mut self, units: usize) -> job::Result<ImagePoll> {
        self.cancel.check()?;
        if units == 0 || units > 16384 || self.done {
            return Err(JobError::new(ErrorCode::InvalidRequest, "invalid raytrace step or completed task"));
        }
        for _ in 0..units.min(4) {
            if self.optical.first().is_some_and(|optical| !optical.preparation.is_empty()) {
                let optical = self.optical.first_mut().ok_or_else(|| invalid("missing optical preparation"))?;
                let task = optical.preparation.first_mut().ok_or_else(|| invalid("missing legacy certificate cursor"))?;
                if task.ready() {
                    self.work.tick(&self.cancel).map_err(error)?;
                    let proof = optical
                        .preparation
                        .pop()
                        .ok_or_else(|| invalid("missing completed legacy certificate"))?
                        .into_proof(self.work.used())
                        .map_err(error)?;
                    optical.certificates = proof.into_legacy()?;
                    optical.preparation = Vec::new();
                } else {
                    task.step(&self.scene, &self.options, &mut self.work, &self.cancel).map_err(legacy_error)?;
                }
            } else if self.progressive.as_ref().is_some_and(|p| !p.film.ready()) {
                self.progressive
                    .as_mut()
                    .ok_or_else(|| invalid("missing progressive initialization"))?
                    .film
                    .initialize_one(&self.cancel, &mut || self.work.tick(&self.cancel).map_err(error))?;
            } else if !self.output.is_empty() {
                let guides = self.denoise.first().map(super::denoise::Guides::pixels).transpose()?;
                let counts = self.progressive.as_ref().map(|p| p.film.counts()).transpose()?.flatten();
                let output = self.output.first_mut().ok_or_else(|| invalid("missing CPU output task"))?;
                let poll = if self.options.denoise_diagnostics {
                    let classes = self.denoise.first().map(super::denoise::Guides::classes).transpose()?;
                    output.step_with_diagnostics(guides, classes, counts, &mut |units| self.work.charge(units, &self.cancel).map_err(error))?
                } else {
                    output.step(guides, counts, &mut |units| self.work.charge(units, &self.cancel).map_err(error))?
                };
                match poll {
                    TaskPoll::Pending(_) => {}
                    TaskPoll::Complete(png) => {
                        let mut output = self.output.pop().ok_or_else(|| invalid("missing completed CPU output task"))?;
                        self.denoise_diagnostics = output.denoise_diagnostics();
                        self.denoise_mask = output.take_denoise_mask();
                        let (rgba, exr) = output.finish()?;
                        self.pixels = rgba;
                        self.linear_exr = exr;
                        return self.complete_png(png, true);
                    }
                }
            } else if let Some(encoder) = &mut self.encoder {
                if self.progressive.is_some() {
                    self.work
                        .charge(if encoder.row() < self.request.height as usize { u64::from(self.request.width) } else { 1 }, &self.cancel)
                        .map_err(error)?;
                }
                if let Some(png) = encoder.step(&self.pixels)? {
                    return self.complete_png(png, false);
                }
            } else if self.progressive.as_ref().is_some_and(|p| p.resolving) {
                self.resolve_progressive_one()?;
            } else if self.maps.first().is_some_and(|maps| !maps.ready()) {
                self.maps
                    .first_mut()
                    .ok_or_else(|| invalid("CPU map cache disappeared"))?
                    .prepare_one(&self.cancel, &mut |units| self.work.charge(units, &self.cancel).map_err(error))?;
            } else if self.pixel == 0 && self.progressive.as_ref().is_some_and(|p| p.film.pilot_complete()) {
                // The last pilot first takes the ordinary complete-pass resolve,
                // finishing and PNG handoff. Only complete_png resets pixel to0;
                // keep that pilot's film/counts available to finishing and host
                // preview metadata until the next cooperative step begins reset.
                self.progressive
                    .as_mut()
                    .ok_or_else(|| invalid("missing CPU pilot handoff"))?
                    .film
                    .begin_production(&self.cancel, &mut || self.work.tick(&self.cancel).map_err(error))?;
            } else if self.progressive.as_ref().is_some_and(|p| p.film.resetting()) {
                self.progressive
                    .as_mut()
                    .ok_or_else(|| invalid("missing CPU production reset"))?
                    .film
                    .reset_one(&self.cancel, &mut || self.work.tick(&self.cancel).map_err(error))?;
            } else if self.glass.first().is_some_and(|glass| !glass.ready()) {
                self.glass.first_mut().ok_or_else(|| invalid("CPU glass certificate disappeared"))?.step(
                    &self.scene,
                    &self.options,
                    &mut self.work,
                    &self.cancel,
                )?;
            } else if self.environment.as_ref().is_some_and(|e| !e.ready()) {
                self.work.tick(&self.cancel).map_err(error)?;
                self.environment
                    .as_mut()
                    .ok_or_else(|| invalid("missing environment preparation"))?
                    .prepare_one(|| self.cancel.is_cancelled())
                    .map_err(environment_error)?;
            } else if self.bvh.is_some()
                && self.caustics.first().is_some_and(|map| {
                    !map.ready() || (self.pixel == 0 && self.progressive.as_ref().is_some_and(|p| map.refresh_due(p.film.samples())))
                })
            {
                #[cfg(not(target_arch = "wasm32"))]
                let parallel_photons = parallel::admitted_workers(&self.admission)? > 1
                    && self.caustics.first().is_some_and(|map| map.parallel_ready() && map.parallel_sources(self))
                    && !self.scene.meshes.is_empty();
                #[cfg(target_arch = "wasm32")]
                let parallel_photons = false;
                if parallel_photons {
                    #[cfg(not(target_arch = "wasm32"))]
                    parallel::photons(self)?;
                } else {
                    caustics::step(self)?;
                }
            } else if self.bvh.is_some() && self.denoise.first().is_some_and(|guides| !guides.ready()) {
                let bvh = self.bvh.as_ref().ok_or_else(|| invalid("missing CPU guide BVH"))?;
                let glass = self
                    .glass
                    .first()
                    .map(super::denoise::GlassCertification::Path)
                    .or_else(|| self.optical.first().map(|optical| super::denoise::GlassCertification::Legacy(&optical.certificates)));
                self.denoise.first_mut().ok_or_else(|| invalid("missing CPU guide task"))?.step(
                    PreparedScene { bvh, primitives: &self.looks, source: &self.scene },
                    &self.request,
                    &self.options,
                    glass,
                    self.clip,
                    &self.effects,
                    self.maps.first_mut(),
                    self.aging.first_mut(),
                    &mut self.work,
                    &self.cancel,
                )?;
            } else if self.bvh.is_some() {
                if self.pixel == self.request.width as usize * self.request.height as usize {
                    if let Some(progressive) = &mut self.progressive {
                        self.work.tick(&self.cancel).map_err(error)?;
                        // Resolve every complete pass. The host keeps at most two
                        // immutable snapshots without changing sample order.
                        progressive.resolving = true;
                    } else if self.needs_linear_pass() {
                        self.start_output()?;
                    } else {
                        self.encoder = Some(job::encoding::PngEncoding::new(
                            self.request.width,
                            self.request.height,
                            self.admission.encoded_limit,
                            self.cancel.clone(),
                        )?);
                    }
                } else if self.progressive.as_ref().map(|p| p.film.identity(self.pixel, p.options.seed)).transpose()?.is_some_and(|i| i.is_none()) {
                    self.progressive.as_mut().ok_or_else(|| invalid("missing CPU stopped-pixel film"))?.film.skip(
                        self.pixel,
                        &self.cancel,
                        &mut || self.work.tick(&self.cancel).map_err(error),
                    )?;
                    self.pixel += 1;
                } else {
                    #[cfg(not(target_arch = "wasm32"))]
                    let parallel_camera = parallel::admitted_workers(&self.admission)? > 1;
                    #[cfg(target_arch = "wasm32")]
                    let parallel_camera = false;
                    if parallel_camera {
                        #[cfg(not(target_arch = "wasm32"))]
                        parallel::camera(self)?;
                    } else if self.options.path.is_some() {
                        indirect_image::step(self)?;
                    } else {
                        self.pixel_one()?;
                    }
                }
            } else if let Some(builder) = &mut self.builder {
                let progress = builder.step(1, &self.cancel).map_err(error)?;
                for _ in self.build_used..progress.work_used {
                    self.work.tick(&self.cancel).map_err(error)?;
                }
                self.build_used = progress.work_used;
                if progress.complete {
                    self.bvh = Some(self.builder.take().ok_or_else(|| invalid("missing builder"))?.finish().map_err(error)?);
                }
            } else {
                self.prepare_one()?;
            }
        }
        Ok(ImagePoll::Pending(self.progress()))
    }
}
/// Apply the already selected deterministic effect to the shared opaque hit.
/// The captured geometric normal, ray origin, visibility and topology are unchanged.
fn resolved_surface_hit<'a>(
    scene: &PreparedScene<'a>,
    pending: &PendingPixel,
    effects: &[(usize, crate::surface_patterns::PreparedEffect)],
    maps: Option<&job::textures::cpu_cache::CpuTextureCache>,
    aging: Option<&super::aging::Task>,
) -> job::Result<SharedHit<'a>> {
    if !pending.surface_ready {
        return Err(invalid("surface effect is not completely resolved"));
    }
    let mut shared = unaged_surface_hit(scene, pending, effects, maps)?;
    if let Some(aging) = aging {
        let changed = aging.surface()?;
        shared.surface = changed.surface;
        shared.shading_normal = changed.shading_normal;
    }
    Ok(shared)
}
fn unaged_surface_hit<'a>(
    scene: &PreparedScene<'a>,
    pending: &PendingPixel,
    effects: &[(usize, crate::surface_patterns::PreparedEffect)],
    maps: Option<&job::textures::cpu_cache::CpuTextureCache>,
) -> job::Result<SharedHit<'a>> {
    let mut shared = unmapped_surface_hit(scene, pending, effects)?;
    if let Some(maps) = maps {
        let mapped = maps.surface()?;
        shared.surface = mapped.surface;
        shared.shading_normal = mapped.shading_normal;
    }
    Ok(shared)
}
fn unmapped_surface_hit<'a>(
    scene: &PreparedScene<'a>,
    pending: &PendingPixel,
    effects: &[(usize, crate::surface_patterns::PreparedEffect)],
) -> job::Result<SharedHit<'a>> {
    if pending.surface_cursor != effects.len() {
        return Err(invalid("surface effect scan is incomplete"));
    }
    let mut shared = scene.resolve_hit(pending.ray, pending.hit.ok_or_else(|| invalid("missing prepared surface hit"))?).map_err(error)?;
    if let Some(index) = pending.surface_index {
        let (body_index, effect) = effects.get(index).ok_or_else(|| invalid("missing selected surface effect"))?;
        if *body_index != shared.body_index {
            return Err(invalid("selected surface effect belongs to another body"));
        }
        let changed = effect.apply(shared.surface, shared.shading_normal, shared.oriented_geometric_normal(), shared.position)?;
        shared.surface = changed.surface;
        shared.shading_normal = changed.shading_normal;
    }
    Ok(shared)
}

/// Adjacent shading-grid rays meet the same captured geometric plane. This is a
/// texture footprint only: no topology or visibility decision uses the plane.
/// A grazing footprint that crosses the horizon selects the coarsest mip.
fn primary_footprint(
    request: &ImageRequest,
    options: &RaytraceOptions,
    progressive: Option<&Progressive>,
    pixel: usize,
    spatial: u8,
    clip: (f64, f64),
    position: Vec3,
    normal: Vec3,
) -> job::Result<Option<[Vec3; 2]>> {
    let width = request.width as usize;
    let offset =
        if let Some(progressive) = progressive { progressive.variates(pixel, 0)? } else { primary_offset(options.primary_samples, spatial)? };
    let x = (pixel % width) as f64 + offset[0];
    let y = (pixel / width) as f64 + offset[1];
    let spacing = if progressive.is_some() { 1.0 } else { 1.0 / f64::from(options.primary_samples).sqrt() };
    if let Some(lens) = options.lens.filter(|lens| lens.aperture_radius_mm > 0.0) {
        // Counter-based lookup reproduces the primary ray's pupil without advancing
        // its sampler or adding a dimension. Hold that pupil for both neighbours.
        let progressive = progressive.ok_or_else(|| invalid("physical pupil requires progressive camera sampling"))?;
        let pupil = progressive.variates(pixel, 0x4c454e53)?;
        plane_footprint(position, normal, spacing, |dx, dy| lens_primary_ray(request, x + dx, y + dy, clip, lens, pupil).map_err(error))
    } else {
        camera_footprint(request, x, y, spacing, clip, position, normal)
    }
}

/// Same geometric-plane footprint for deterministic guide rays, without changing
/// their center sample or the progressive camera's sampling stream.
#[allow(clippy::too_many_arguments)]
pub(super) fn camera_footprint(
    request: &ImageRequest,
    x: f64,
    y: f64,
    spacing: f64,
    clip: (f64, f64),
    position: Vec3,
    normal: Vec3,
) -> job::Result<Option<[Vec3; 2]>> {
    plane_footprint(position, normal, spacing, |dx, dy| primary_ray(request, x + dx, y + dy, clip).map_err(error))
}

fn plane_footprint(
    position: Vec3,
    normal: Vec3,
    spacing: f64,
    mut neighbour: impl FnMut(f64, f64) -> job::Result<Ray>,
) -> job::Result<Option<[Vec3; 2]>> {
    let mut result = [Vec3::ZERO; 2];
    for (output, [dx, dy]) in result.iter_mut().zip([[spacing, 0.0], [0.0, spacing]]) {
        let ray = neighbour(dx, dy)?;
        let denominator = ray.direction().dot(normal);
        if !denominator.is_finite() || denominator.abs() < 1e-12 {
            return Ok(None);
        }
        let distance = (position - ray.origin()).dot(normal) / denominator;
        if !distance.is_finite() || distance < 0.0 {
            return Ok(None);
        }
        *output = ray.origin() + ray.direction() * distance - position;
        if !output.is_finite() {
            return Ok(None);
        }
    }
    Ok(Some(result))
}

fn bounded_radiance(v: Vec3) -> bool {
    v.is_finite() && [v.x, v.y, v.z].into_iter().all(|c| (0.0..=1e12).contains(&c))
}
fn primary_offset(samples: u8, ordinal: u8) -> job::Result<[f64; 2]> {
    let side = match samples {
        1 => 1,
        4 => 2,
        16 => 4,
        _ => return Err(invalid("invalid spatial sample count")),
    };
    if ordinal >= samples {
        return Err(invalid("invalid spatial sample ordinal"));
    }
    Ok([(f64::from(ordinal % side) + 0.5) / f64::from(side), (f64::from(ordinal / side) + 0.5) / f64::from(side)])
}
fn spatial_key(pixel: u64, ordinal: u8, samples: u8) -> job::Result<u64> {
    primary_offset(samples, ordinal)?;
    pixel
        .checked_mul(u64::from(samples))
        .and_then(|n| n.checked_add(u64::from(ordinal)))
        .map(|n| n ^ 0x4141_5052_494d_4152)
        .ok_or_else(|| invalid("spatial seed overflow"))
}
/// Distinguish film and spatial ordinals before sampling an emitter. The legacy
/// single-primary first pass keeps its original key and reference image bytes.
fn area_sample_key(pixel: u64, spatial: u8, spatial_samples: u8, film_sample: u32) -> job::Result<u64> {
    primary_offset(spatial_samples, spatial)?;
    if film_sample >= job::MAX_CPU_SAMPLES_PER_PIXEL {
        return Err(invalid("area-light film sample ordinal exceeds the admitted cap"));
    }
    if spatial_samples == 1 && film_sample == 0 {
        return Ok(pixel);
    }
    // Keep existing sample identities byte-for-byte. Added ordinals use a
    // distinct domain and a full-cap stride to avoid aliasing adjacent pixels.
    let (stride, domain) =
        if film_sample < 4096 { (4096 * 16, 0x4152_4541_5341_4d50) } else { (u64::from(job::MAX_CPU_SAMPLES_PER_PIXEL) * 16, 0xc152_4541_5341_4d50) };
    pixel
        .checked_mul(stride)
        .and_then(|key| key.checked_add(u64::from(film_sample) * 16 + u64::from(spatial)))
        .map(|key| key ^ domain)
        .ok_or_else(|| invalid("area-light sample identity overflow"))
}

/// Straight-alpha display from primary geometric coverage. Keep the original
/// opaque f64 route untouched, including its unexposed display backplate.
fn composite_spatial_alpha(
    premultiplied: Vec3,
    covered: u32,
    attempts: u32,
    background: [u8; 3],
    exposure: f64,
    transparent: bool,
) -> job::Result<[u8; 4]> {
    if !transparent {
        return composite_spatial(premultiplied, covered, attempts, background, exposure);
    }
    if attempts == 0 || attempts > job::MAX_CPU_SAMPLES_PER_PIXEL || covered > attempts || !bounded_radiance(premultiplied) {
        return Err(invalid("invalid transparent coverage/radiance"));
    }
    if covered == 0 {
        if premultiplied != Vec3::ZERO {
            return Err(invalid("uncovered transparent radiance"));
        }
        return Ok([0; 4]);
    }
    let coverage = f64::from(covered) / f64::from(attempts);
    let alpha = (coverage * 255.0).round() as u8;
    if alpha == 0 {
        return Ok([0; 4]);
    }
    let display = map_to_display(premultiplied / coverage, exposure).map_err(error)?;
    Ok([encode8(display.x), encode8(display.y), encode8(display.z), alpha])
}

fn composite_spatial(premultiplied: Vec3, covered: u32, attempts: u32, background: [u8; 3], exposure: f64) -> job::Result<[u8; 4]> {
    if attempts == 0 || attempts > job::MAX_CPU_SAMPLES_PER_PIXEL || covered > attempts || !bounded_radiance(premultiplied) {
        return Err(invalid("invalid spatial coverage/radiance"));
    }
    if covered == 0 {
        if premultiplied != Vec3::ZERO {
            return Err(invalid("uncovered spatial radiance"));
        }
        return Ok([background[0], background[1], background[2], 255]);
    }
    let coverage = f64::from(covered) / f64::from(attempts);
    let geometry = map_to_display(premultiplied / coverage, exposure).map_err(error)?;
    let display = if covered == attempts {
        geometry
    } else {
        let bg = Vec3::new(srgb8_to_linear(background[0]), srgb8_to_linear(background[1]), srgb8_to_linear(background[2]));
        geometry * coverage + bg * (1.0 - coverage)
    };
    Ok([encode8(display.x), encode8(display.y), encode8(display.z), 255])
}

/// One sample preparation, one traversal operation OR one visible-sum operation.
/// Reuses the existing primary/directional cursor: no second task or heap state.
fn punctual_one(
    pending: &mut PendingPixel,
    options: &RaytraceOptions,
    bvh: &Bvh,
    context: (super::prepared::SharedHit<'_>, Option<PlanarTarget>, Option<job::ReflectionMode>),
    work: &mut WorkBudget,
    cancel: &CancelToken,
    diagnostics: &mut job::PunctualDiagnostics,
) -> job::Result<()> {
    let (shared, target, mode) = context;
    if pending.punctual_shadow {
        if let Some(blocker) = pending.cursor.step(bvh, work, cancel).map_err(error)? {
            if blocker.is_some() {
                pending.punctual_value = Vec3::ZERO;
            }
            pending.punctual_shadow = false;
            pending.punctual_ready = true;
        }
        return Ok(());
    }
    work.tick(cancel).map_err(error)?;
    if pending.punctual_ready {
        add_punctual_radiance(pending, pending.punctual_value, diagnostics);
        pending.punctual_index += 1;
        pending.punctual_ready = false;
        return Ok(());
    }
    let inputs = options.punctual_lights.as_ref().ok_or_else(|| invalid("punctual options missing"))?;
    if mode == Some(job::ReflectionMode::Mirror) {
        pending.punctual_index = inputs.len();
        return Ok(());
    }
    let light = inputs.get(pending.punctual_index).copied().ok_or_else(|| invalid("punctual source index"))?.light().map_err(error)?;
    let sample = light.sample(shared.position).map_err(error)?;
    if let Some(reason) = sample.omitted {
        match reason {
            super::punctual::OmittedSample::NearSource => diagnostics.near_source = diagnostics.near_source.saturating_add(1),
            super::punctual::OmittedSample::RadianceLimit => diagnostics.radiance_limit = diagnostics.radiance_limit.saturating_add(1),
        }
        pending.punctual_index += 1;
        return Ok(());
    }
    let geometric = shared.oriented_geometric_normal();
    let cosine = if target.is_some() { geometric } else { shared.shading_normal }.dot(sample.direction).max(0.0);
    let contribution = if geometric.dot(sample.direction) <= 0.0 || cosine == 0.0 || sample.incident_linear_srgb == Vec3::ZERO {
        Vec3::ZERO
    } else {
        let view = -pending.ray.direction();
        let f = if let Some(target) = target {
            target.direct_brdf(shared.surface, geometric, view, sample.direction).map_err(error)?
        } else {
            shared.surface.brdf(shared.shading_normal, view, sample.direction).map_err(|_| invalid("punctual opaque response"))?
        };
        multiply(f, sample.incident_linear_srgb) * cosine
    };
    if super::lighting::validate_radiance(contribution).is_err() {
        diagnostics.radiance_limit = diagnostics.radiance_limit.saturating_add(1);
        pending.punctual_index += 1;
        return Ok(());
    }
    pending.punctual_value = contribution;
    if options.shadows && contribution != Vec3::ZERO {
        let Some(ray) = sample.shadow_ray(shared).map_err(error)? else {
            diagnostics.shadow_offset_crossed = diagnostics.shadow_offset_crossed.saturating_add(1);
            pending.punctual_index += 1;
            return Ok(());
        };
        pending.cursor = TraceCursor::new(ray, true);
        pending.punctual_shadow = true;
    } else {
        add_punctual_radiance(pending, contribution, diagnostics);
        pending.punctual_index += 1;
    }
    Ok(())
}

fn add_punctual_radiance(pending: &mut PendingPixel, contribution: Vec3, diagnostics: &mut job::PunctualDiagnostics) {
    let candidate = pending.radiance + multiply(pending.throughput, contribution);
    if super::lighting::validate_radiance(candidate).is_ok() {
        pending.radiance = candidate;
    } else {
        diagnostics.radiance_limit = diagnostics.radiance_limit.saturating_add(1);
    }
}

fn multiply(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x * b.x, a.y * b.y, a.z * b.z)
}
#[cfg(test)]
fn shadow_ray(ray: Ray, hit: super::Hit, light: Vec3, vertices: [Vec3; 3]) -> super::Result<Ray> {
    let mut n = hit.geometry.geometric_normal;
    if n.dot(light) < 0.0 {
        n = -n;
    }
    let [a, b, c] = hit.geometry.barycentric;
    let [va, vb, vc] = vertices;
    // Reconstruct from captured vertices, not a distant camera. Component-wise
    // rounding bounds avoid an unrelated large X translation displacing a Z plane.
    let p = va * a + vb * b + vc * c;
    let magnitude = |x: f64, y: f64, z: f64| x.abs() * a.abs() + y.abs() * b.abs() + z.abs() * c.abs();
    let e = Vec3::new(magnitude(va.x, vb.x, vc.x), magnitude(va.y, vb.y, vc.y), magnitude(va.z, vb.z, vc.z)) * (8.0 * f64::EPSILON);
    let distance = n.x.abs() * e.x + n.y.abs() * e.y + n.z.abs() * e.z;
    let shifted = p + n * distance;
    let outward = |v: f64, n: f64| {
        if n > 0.0 {
            v.next_up()
        } else if n < 0.0 {
            v.next_down()
        } else {
            v
        }
    };
    let _ = ray; // hit position comes from geometry, independent of camera distance.
    Ray::new(Vec3::new(outward(shifted.x, n.x), outward(shifted.y, n.y), outward(shifted.z, n.z)), light, 0.0_f64.next_up(), super::MAX_DISTANCE_MM)
}

/// A circular pupil in the eye plane, focused at an axial distance. The
/// original primary generator remains untouched for absent or zero aperture.
fn lens_primary_ray(request: &ImageRequest, x: f64, y: f64, clip: (f64, f64), lens: job::ThinLens, pupil: [f64; 2]) -> super::Result<Ray> {
    if lens.aperture_radius_mm == 0.0 {
        return primary_ray(request, x, y, clip);
    }
    if pupil.into_iter().any(|v| !v.is_finite() || !(0.0..1.0).contains(&v)) {
        return Err(TraceError::InvalidInput("thin-lens pupil sample"));
    }
    let camera = &request.camera;
    let (right, up, back) = camera.basis();
    let eye = camera.eye();
    let (_, pinhole) = camera.ray(x, y, request.width as f64, request.height as f64);
    let cosine = pinhole.dot(-back);
    if !cosine.is_finite() || cosine <= 0.0 {
        return Err(TraceError::InvalidInput("thin-lens camera direction"));
    }
    let radius = lens.aperture_radius_mm * pupil[0].sqrt();
    let phi = std::f64::consts::TAU * pupil[1];
    let displacement = (right * phi.cos() + up * phi.sin()) * radius;
    // Work in eye-relative coordinates so large world translations cannot erase
    // a small pupil before computing its aimed direction.
    let focused = pinhole * (lens.focus_distance_mm / cosine);
    let direction = super::unit(focused - displacement).ok_or(TraceError::InvalidInput("thin-lens focus direction"))?;
    let cosine = direction.dot(-back);
    if !cosine.is_finite() || cosine <= 0.0 {
        return Err(TraceError::InvalidInput("thin-lens forward interval"));
    }
    let origin = eye + displacement;
    let depth = (origin - eye).dot(-back);
    let start = (clip.0 - depth) / cosine;
    Ray::new(origin + direction * start, direction, 0.0, (clip.1 - clip.0) / cosine)
}

pub(super) fn primary_ray(request: &ImageRequest, x: f64, y: f64, clip: (f64, f64)) -> super::Result<Ray> {
    let camera = &request.camera;
    let (mut origin, direction) = camera.ray(x, y, request.width as f64, request.height as f64);
    if camera.fov == 0.0 {
        origin -= camera.back() * (camera.distance * 3.0);
    }
    let forward = -camera.back();
    let cosine = direction.dot(forward);
    if !cosine.is_finite() || cosine <= 0.0 {
        return Err(TraceError::InvalidInput("camera ray direction"));
    }
    let depth = (origin - camera.eye()).dot(forward);
    let start = (clip.0 - depth) / cosine;
    let length = (clip.1 - clip.0) / cosine;
    Ray::new(origin + direction * start, direction, 0.0, length)
}

#[cfg(test)]
#[path = "image_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "deterministic_primary_tests.rs"]
mod deterministic_primary_tests;

#[cfg(test)]
#[path = "reflection_image_tests.rs"]
mod reflection_tests;

fn environment_error(e: crate::lighting::HdrError) -> JobError {
    use crate::lighting::HdrErrorCode;
    let code = match e.code {
        HdrErrorCode::Cancelled => ErrorCode::Cancelled,
        HdrErrorCode::LimitExceeded => ErrorCode::LimitExceeded,
        HdrErrorCode::AllocationFailed => ErrorCode::AllocationFailed,
        _ => ErrorCode::InvalidRequest,
    };
    JobError::new(code, e.to_string())
}

#[cfg(test)]
#[path = "primary_aa_tests.rs"]
mod primary_aa_tests;

// One bounded continuation operation, using the same cursor, work accumulator and task.
fn area_one(
    pending: &mut PendingPixel,
    options: &RaytraceOptions,
    bvh: &Bvh,
    context: (super::prepared::SharedHit<'_>, Option<PlanarTarget>, Option<job::ReflectionMode>),
    work: &mut WorkBudget,
    cancel: &CancelToken,
    pixel: u64,
) -> job::Result<()> {
    let (shared, target, mode) = context;
    if pending.area_shadow {
        if let Some(blocker) = pending.cursor.step(bvh, work, cancel).map_err(error)? {
            if blocker.is_some() {
                pending.area_value = Vec3::ZERO;
            }
            pending.area_shadow = false;
            pending.area_ready = true;
        }
        return Ok(());
    }
    work.tick(cancel).map_err(error)?;
    if pending.area_ready {
        pending.radiance += multiply(pending.throughput, pending.area_value);
        super::lighting::validate_radiance(pending.radiance).map_err(error)?;
        pending.area_index += 1;
        pending.area_ready = false;
        return Ok(());
    }
    let inputs = options.area_lights.as_ref().ok_or_else(|| invalid("area options missing"))?;
    if mode == Some(job::ReflectionMode::Mirror) {
        pending.area_light = inputs.len();
        return Ok(());
    }
    let count = options.area_samples.unwrap_or(16);
    if pending.area_index == count {
        pending.area_light += 1;
        pending.area_index = 0;
        return Ok(());
    }
    let input = inputs.get(pending.area_light).ok_or_else(|| invalid("area source index"))?;
    let vec = |[x, y, z]: [f64; 3]| Vec3::new(x, y, z);
    let light = super::area::RectangleLight::new(vec(input.center_mm), vec(input.edge_u_mm), vec(input.edge_v_mm), vec(input.color), input.intensity)
        .map_err(error)?;
    let Some(sample) =
        light.sample(shared.position, pixel, pending.area_light as u32, pending.area_index, count, options.seed.unwrap_or(0)).map_err(error)?
    else {
        pending.area_index += 1;
        return Ok(());
    };
    let geometric = shared.oriented_geometric_normal();
    let cosine = if target.is_some() { geometric } else { shared.shading_normal }.dot(sample.direction()).max(0.0);
    let contribution = if geometric.dot(sample.direction()) <= 0.0 || cosine == 0.0 {
        Vec3::ZERO
    } else {
        let view = -pending.ray.direction();
        let f = if let Some(target) = target {
            target.direct_brdf(shared.surface, geometric, view, sample.direction()).map_err(error)?
        } else {
            shared.surface.brdf(shared.shading_normal, view, sample.direction()).map_err(|_| invalid("area opaque response"))?
        };
        multiply(f, sample.incident()) * cosine
    };
    super::lighting::validate_radiance(contribution).map_err(error)?;
    pending.area_value = contribution;
    if options.shadows && contribution != Vec3::ZERO {
        let origin = shared.spawn(sample.direction()).map_err(error)?.origin();
        pending.cursor = TraceCursor::new(sample.shadow_ray(origin).map_err(error)?, true);
        pending.area_shadow = true;
    } else {
        pending.radiance += multiply(pending.throughput, contribution);
        super::lighting::validate_radiance(pending.radiance).map_err(error)?;
        pending.area_index += 1;
    }
    Ok(())
}

#[cfg(test)]
#[path = "punctual_image_tests.rs"]
mod punctual_image_tests;
