//! Native progressive product rendering. Software BVH traversal needs no ray-query
//! features. Small measured quanta adapt to the device, then check cancellation.
use solidcore_record::Vec3;
use solidcraft_geom::Vec3Geom;
#[cfg(test)]
use solidcraft_render::job::ToneMap;
use solidcraft_render::{
    Availability, ExecutorDescriptor, ExecutorId, ProductScene, RenderFeature, RenderFormat, RenderMode, RenderOperation, RenderProfile,
    ScratchLayout,
    job::{
        Admission, CancelToken, ErrorCode, GPU_PATH_LIMITS, ImageExecutor, ImageHost, ImageRequest, ImageWork, JobError, JobOptions,
        MAX_GPU_PATH_BODIES, MAX_GPU_PATH_TRIANGLES, Phase, PngTask, Progress, Result, TaskPoll, admit_geometry_with_limits,
    },
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use wgpu::util::DeviceExt;

mod adaptive;
mod denoise;
mod dispatch;
mod media_hierarchy;
mod photons;
mod scene;
#[cfg(test)]
mod tests;
mod textures;
// Opaque producers specialize away dielectric traversal and use a single
// unused medium slot. Optical producers share the sixteen-medium shader.
macro_rules! path_shader {
    ($capacity:literal) => {
        concat!(
            "const MEDIUM_CAPACITY: u32 = ",
            $capacity,
            "u;\n",
            include_str!("path.wgsl"),
            "\n",
            include_str!("shadows.wgsl"),
            "\n",
            include_str!("textures.wgsl"),
            "\n",
            include_str!("volume.wgsl"),
            "\n",
            include_str!("aging.wgsl"),
            "\n",
            include_str!("punctual.wgsl"),
            "\n",
            include_str!("glass.wgsl"),
            "\n",
            include_str!("glass_lighting.wgsl"),
            "\n",
            include_str!("photons_disabled.wgsl")
        )
    };
}
const SHADER: &str = path_shader!(16);
const SINGLE_MEDIUM_SHADER: &str = path_shader!(1);
const MAX_TRIANGLES: u64 = MAX_GPU_PATH_TRIANGLES;
const PATH_WORKGROUP_SIZE: u32 = 32;
const SCENE_PIPELINE_CACHE_SLOTS: usize = 5;
const PARAMETER_BYTES: usize = 2080;
// Five vec4s per source; exact device+upload growth is additional to v2's budget.
const PUNCTUAL_PARAMETER_BYTES: u64 = 8 * 80;
// Eight inline 32-byte hard directions: device uniform plus host upload bytes.
const DIRECTIONAL_PARAMETER_BYTES: u64 = 8 * 32;
// Eight u32 words: count low, pixel, bounce, sample, code, count high, first-writer flag, spare.
const FAILURE_BYTES: u64 = 56;
// Original medium words0..7 remain unchanged; three low/high punctual counters
// occupy words8..13. Both device storage and complete-pass readback are prepaid.
const PUNCTUAL_COUNTER_BYTES: u64 = 24;
// A path that cannot classify its glass medium ends there. Isolated paths at the
// limits of f32 geometry are tolerated and logged; more than this fraction of all
// paths (any systematic error such as an ambiguous boundary origin) fails the job.
const FAILURE_FRACTION: f64 = 1e-6;
const TIMEOUT: Duration = Duration::from_secs(30);
// Software Vulkan drivers can compile a pipeline at its first dispatch, which on a cold
// shader cache exceeds the per-dispatch hang timeout. Setup dispatches once with no work
// and allows this longer wait, so jobs keep the short one.
const SETUP_TIMEOUT: Duration = Duration::from_secs(600);

fn gpu(e: impl std::fmt::Display) -> JobError {
    JobError::new(ErrorCode::HostUnavailable, e.to_string())
}

fn scopes(device: &wgpu::Device) -> [wgpu::ErrorScopeGuard; 3] {
    [wgpu::ErrorFilter::Validation, wgpu::ErrorFilter::OutOfMemory, wgpu::ErrorFilter::Internal].map(|f| device.push_error_scope(f))
}
fn checked(scopes: [wgpu::ErrorScopeGuard; 3]) -> Result<()> {
    let mut error = None;
    for scope in scopes.into_iter().rev() {
        if let Some(e) = pollster::block_on(scope.pop()) {
            error = Some(e);
        }
    }
    error.map_or(Ok(()), |e| Err(gpu(e)))
}

/// Drain every scope, in reverse order, even when resource creation returns an error.
/// The ordinary cancellation/error path must not drop an array of guards forwards.
fn checked_creation<T>(device: &wgpu::Device, create: impl FnOnce() -> Result<T>) -> Result<T> {
    let error_scopes = scopes(device);
    let result = create();
    checked(error_scopes)?;
    result
}

/// Open and validate a native device before advertising the executor. Failure leaves the
/// caller's CPU host usable. Respects SOLIDCRAFT_GPU, including the software adapter policy.
pub fn host() -> Result<ImageHost> {
    let (adapter, device, queue) = open_headless()?;
    log::info!("gpu-path adapter: {} ({:?})", adapter.get_info().name, adapter.get_info().device_type);
    ImageHost::with_executor(Arc::new(Executor::new(device, queue)?))
}

/// Open an independent job device on the viewport's selected adapter. In particular,
/// software fallback and safe mode must not rediscover a failing hardware driver.
/// An explicit incompatible SOLIDCRAFT_GPU policy leaves this executor unavailable.
/// The path tracer runs on Vulkan, or Metal on macOS. On Windows it must keep the viewport's
/// backend: opening Vulkan drivers that startup avoided can crash inside native driver code.
/// Elsewhere a viewport on another backend is reopened on the same GPU through one of them.
/// Without a usable adapter the executor is unavailable; CPU image methods remain usable.
pub fn host_on_adapter(adapter: &wgpu::Adapter) -> Result<ImageHost> {
    let policy = crate::select::Policy::from_env().map_err(gpu)?;
    let info = adapter.get_info();
    if !viewport_path_backends(info.backend, cfg!(target_os = "windows")).intersects(crate::select::path_backends()) {
        return Err(gpu(format!("{VULKAN_ONLY}; the viewport uses {:?}", info.backend)));
    }
    if !policy.admits(crate::select::tier_of(info.device_type)) {
        return Err(gpu(format!("viewport adapter does not satisfy SOLIDCRAFT_GPU={}", policy.name())));
    }
    if !matches!(info.backend, wgpu::Backend::Vulkan | wgpu::Backend::Metal) {
        let (picked, _) = crate::select::pick_path(policy, |i| i.vendor == info.vendor && i.device == info.device)
            .ok_or_else(|| gpu(format!("{VULKAN_ONLY}; the viewport uses {:?}", info.backend)))?;
        let (device, queue) = open_device(&picked)?;
        log::info!("gpu-path viewport GPU: {} through Vulkan or Metal", info.name);
        return ImageHost::with_executor(Arc::new(Executor::new(device, queue)?));
    }
    let (device, queue) = open_device(adapter)?;
    log::info!("gpu-path viewport adapter: {} ({:?})", info.name, info.device_type);
    ImageHost::with_executor(Arc::new(Executor::new(device, queue)?))
}

// D3D12 shader compilation is avoided; the user can select a CPU image method instead.
const VULKAN_ONLY: &str = "the GPU path tracer requires a Vulkan driver (Metal on macOS); select a CPU image method when none is available";

/// Backends a viewport's path device may open, before any driver is loaded.
fn viewport_path_backends(backend: wgpu::Backend, windows: bool) -> wgpu::Backends {
    if windows { wgpu::Backends::from(backend) } else { crate::select::path_backends() }
}

// Path resources need six storage buffers (geometry, BVH, accumulation, HDRI,
// textures, medium-failure counts). Request that limit only on this independent path device: raster and
// tessellation keep their existing four-buffer downlevel requirement.
const PATH_STORAGE_BUFFERS: u32 = 6;
fn device_limits(supported: wgpu::Limits) -> Result<wgpu::Limits> {
    if supported.max_storage_buffers_per_shader_stage < PATH_STORAGE_BUFFERS {
        return Err(gpu("gpu-path requires six storage buffers per shader stage"));
    }
    // Query first: never request unsupported storage or buffer sizes. The old
    // downlevel defaults unnecessarily capped large CAD geometry at 128 MiB.
    let storage = supported.max_storage_buffer_binding_size.min(1024 * 1024 * 1024);
    let buffer = supported.max_buffer_size.min(1024 * 1024 * 1024);
    let optional_photon_buffers = supported.max_storage_buffers_per_shader_stage.min(7);
    let mut limits = wgpu::Limits::downlevel_defaults().using_resolution(supported);
    limits.max_storage_buffer_binding_size = storage.min(buffer);
    limits.max_buffer_size = buffer;
    limits.max_storage_buffers_per_shader_stage = optional_photon_buffers;
    Ok(limits)
}
fn open_device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue)> {
    let required_limits = device_limits(adapter.limits())?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("solidcraft progressive path"),
        required_features: crate::depth_features(adapter),
        required_limits,
        ..Default::default()
    }))
    .map_err(gpu)
}
fn open_headless() -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    let policy = crate::select::Policy::from_env().map_err(gpu)?;
    let (adapter, _) = crate::select::pick_path(policy, |_| true)
        .ok_or_else(|| gpu(format!("{VULKAN_ONLY}; no Vulkan or Metal adapter satisfies SOLIDCRAFT_GPU={}", policy.name())))?;
    let (device, queue) = open_device(&adapter)?;
    Ok((adapter, device, queue))
}

struct Context {
    healthy: Arc<AtomicBool>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    // Plain opaque and non-nested glass are ready before the host is published.
    // The four map/nested variants are prepared lazily in the remaining slots.
    scene_pipelines: [std::sync::OnceLock<wgpu::ComputePipeline>; SCENE_PIPELINE_CACHE_SLOTS],
    // Built and warmed by the first denoised job, then shared by later ones.
    denoise: std::sync::OnceLock<denoise::Pipelines>,
    // Only authored caustics compile/retain the seven-storage-buffer variant.
    photons: std::sync::OnceLock<photons::Pipelines>,
    // Optical hard-light connections are specialized away in the legacy program.
    glass: std::sync::OnceLock<GlassPipelines>,
    photon_glass: std::sync::OnceLock<GlassPipelines>,
    layout: wgpu::BindGroupLayout,
}
impl Context {
    fn ensure_available(&self) -> Result<()> {
        if self.healthy.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(JobError::new(
                ErrorCode::HostUnavailable,
                "GPU path device was lost or failed to complete work; restart the renderer before retrying",
            ))
        }
    }
    fn wait(&self, submission: wgpu::SubmissionIndex, timeout: Duration) -> Result<()> {
        wait_device(&self.device, &self.healthy, submission, timeout)
    }
    #[cfg(test)]
    fn prepared_scene_pipeline(&self, opaque: bool, maps: bool, nested: bool) -> Result<&wgpu::ComputePipeline> {
        let Some(index) = scene_pipeline_variant(opaque, maps, nested) else { return Ok(&self.pipeline) };
        self.scene_pipelines.get(index).and_then(std::sync::OnceLock::get).ok_or_else(|| gpu("captured scene has no prepared pipeline"))
    }
    fn prepare_scene_pipeline(&self, opaque: bool, maps: bool, nested: bool) -> Result<&wgpu::ComputePipeline> {
        self.ensure_available()?;
        let Some(index) = scene_pipeline_variant(opaque, maps, nested) else { return Ok(&self.pipeline) };
        let slot = self.scene_pipelines.get(index).ok_or_else(|| gpu("scene pipeline variant exceeds its finite cache"))?;
        if let Some(pipeline) = slot.get() {
            return Ok(pipeline);
        }
        let _compiling = one_compile_at_a_time();
        if let Some(pipeline) = slot.get() {
            return Ok(pipeline);
        }
        let scopes = scopes(&self.device);
        let shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpu-path captured scene"),
            source: wgpu::ShaderSource::Wgsl(scene_shader(opaque).into()),
        });
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gpu-path captured scene"),
            bind_group_layouts: &[Some(&self.layout)],
            immediate_size: 0,
        });
        let pipeline = self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("gpu-path captured scene"),
            layout: Some(&layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &[("OPAQUE_SCENE", f64::from(u8::from(opaque))), ("HAS_TEXTURES", f64::from(u8::from(maps)))],
                ..Default::default()
            },
            cache: None,
        });
        checked(scopes)?;
        warm_up(&self.device, &self.queue, &pipeline, &placeholder_bind(&self.device, &self.layout, 7)?, &self.healthy)?;
        let _ = slot.set(pipeline);
        slot.get().ok_or_else(|| gpu("scene pipeline publication failed"))
    }
}

fn wait_device(device: &wgpu::Device, healthy: &AtomicBool, submission: wgpu::SubmissionIndex, timeout: Duration) -> Result<()> {
    if !healthy.load(Ordering::Acquire) {
        return Err(JobError::new(ErrorCode::HostUnavailable, "GPU path device is unavailable"));
    }
    if let Err(error) = device.poll(wgpu::PollType::Wait { submission_index: Some(submission), timeout: Some(timeout) }) {
        healthy.store(false, Ordering::Release);
        return Err(JobError::new(ErrorCode::HostUnavailable, format!("GPU path completion failed; executor retired: {error}")));
    }
    if !healthy.load(Ordering::Acquire) {
        return Err(JobError::new(ErrorCode::HostUnavailable, "GPU path device was lost while completing work"));
    }
    Ok(())
}

struct Executor {
    context: Arc<Context>,
}
impl Executor {
    fn new(device: wgpu::Device, queue: wgpu::Queue) -> Result<Self> {
        device_limits(device.limits())?;
        let healthy = Arc::new(AtomicBool::new(true));
        let uncaptured = healthy.clone();
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            uncaptured.store(false, Ordering::Release);
            log::error!("gpu-path executor retired after uncaptured error: {e}");
        }));
        let lost = healthy.clone();
        device.set_device_lost_callback(move |reason, message| {
            lost.store(false, Ordering::Release);
            log::warn!("gpu-path device lost ({reason:?}); executor unavailable: {message}");
        });
        let compiling = one_compile_at_a_time();
        let scopes = scopes(&device);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpu-path"),
            source: wgpu::ShaderSource::Wgsl(scene_shader(true).into()),
        });
        let entries: Vec<_> = (0..7)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 0 {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage { read_only: binding != 3 && binding != 6 }
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("gpu-path"), entries: &entries });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gpu-path"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("gpu-path"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions { constants: &[("OPAQUE_SCENE", 1.0)], ..Default::default() },
            cache: None,
        });
        checked(scopes)?;
        drop(compiling);
        warm_up(&device, &queue, &pipeline, &placeholder_bind(&device, &layout, 7)?, &healthy)?;
        let context = Arc::new(Context {
            healthy,
            device,
            queue,
            pipeline,
            layout,
            denoise: std::sync::OnceLock::new(),
            photons: std::sync::OnceLock::new(),
            glass: std::sync::OnceLock::new(),
            photon_glass: std::sync::OnceLock::new(),
            scene_pipelines: std::array::from_fn(|_| std::sync::OnceLock::new()),
        });
        // The original host prepared glass before advertising readiness. Keep the
        // common no-map/non-nested program out of the first image job's work.
        context.prepare_scene_pipeline(false, false, false)?;
        Ok(Self { context })
    }
}

fn needs_glass_lighting(scene: &ProductScene) -> bool {
    let active = |transmission: Option<solidcraft_render::transmission::Transmission>| {
        transmission.is_some_and(|material| {
            material.model().is_some_and(|model| model.roughness > 0.0) || material.subsurface().is_some_and(|medium| medium.active())
        })
    };
    scene.meshes.iter().any(|body| active(body.transmission) || body.faces.iter().any(|face| active(face.transmission)))
}

#[derive(Clone)]
struct GlassPipelines {
    camera: wgpu::ComputePipeline,
    resolve: wgpu::ComputePipeline,
}

// One request and camera value per path in the largest admitted tile, reusing
// the accumulation binding. No extra image plane or storage binding is needed.
fn glass_tile_bytes(scene: &ProductScene, request: &ImageRequest) -> u64 {
    if needs_glass_lighting(scene) { (u64::from(request.width) * u64::from(request.height)).min(u64::from(dispatch::MAX_PATHS)) * 128 } else { 0 }
}

fn glass_pipeline(context: &Context, layout: &wgpu::BindGroupLayout, caustics: bool, cancel: &CancelToken) -> Result<GlassPipelines> {
    cancel.check()?;
    let slot = if caustics { &context.photon_glass } else { &context.glass };
    if let Some(pipeline) = slot.get() {
        return Ok(pipeline.clone());
    }
    let source = if caustics { std::borrow::Cow::Owned(photons::shader_source()?) } else { std::borrow::Cow::Borrowed(SHADER) };
    let compiling = one_compile_at_a_time();
    cancel.check()?;
    let device = &context.device;
    let scopes = scopes(device);
    let module = device
        .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("gpu-path optical hard lights"), source: wgpu::ShaderSource::Wgsl(source) });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("gpu-path optical hard lights"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    let constants = [("GLASS_DELTA_LIGHTS", 1.0), ("OPAQUE_SCENE", 0.0), ("HAS_TEXTURES", 1.0)];
    let pipeline = |entry| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("gpu-path optical hard lights"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some(entry),
            compilation_options: wgpu::PipelineCompilationOptions { constants: &constants, ..Default::default() },
            cache: None,
        })
    };
    let camera = pipeline("main");
    checked(scopes)?;
    cancel.check()?;
    let resolve = checked_creation(device, || Ok(pipeline("optical_resolve")))?;
    let pipelines = GlassPipelines { camera, resolve };
    cancel.check()?;
    drop(compiling);
    let placeholder = placeholder_bind(device, layout, if caustics { 8 } else { 7 })?;
    warm_up(device, &context.queue, &pipelines.camera, &placeholder, &context.healthy)?;
    cancel.check()?;
    warm_up(device, &context.queue, &pipelines.resolve, &placeholder, &context.healthy)?;
    cancel.check()?;
    Ok(slot.get_or_init(|| pipelines).clone())
}

/// Native drivers compile the path pipeline for minutes and gigabytes on a cold shader cache.
/// Executors opened side by side (parallel tests, each on its own device) compiled it together
/// until memory ran out (#6706). One compile at a time per process: the first fills the driver's
/// cache, and the waiting ones then get it from there.
fn one_compile_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    static COMPILE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    COMPILE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

// Small zeroed bindings: binding 0 is a uniform whose zero work count makes every
// invocation return at once; the others are storage buffers.
fn placeholder_bind(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, count: u32) -> Result<wgpu::BindGroup> {
    let scopes = scopes(device);
    let buffers: Vec<_> = (0..count)
        .map(|binding| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("gpu-path warm-up"),
                size: if binding == 0 { PARAMETER_BYTES as u64 } else { 256 },
                usage: if binding == 0 { wgpu::BufferUsages::UNIFORM } else { wgpu::BufferUsages::STORAGE },
                mapped_at_creation: false,
            })
        })
        .collect();
    let entries: Vec<_> = buffers
        .iter()
        .enumerate()
        .map(|(binding, buffer)| wgpu::BindGroupEntry { binding: binding as u32, resource: buffer.as_entire_binding() })
        .collect();
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("gpu-path warm-up"), layout, entries: &entries });
    checked(scopes)?;
    Ok(bind)
}

fn warm_up(device: &wgpu::Device, queue: &wgpu::Queue, pipeline: &wgpu::ComputePipeline, bind: &wgpu::BindGroup, healthy: &AtomicBool) -> Result<()> {
    let scopes = scopes(device);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("gpu-path warm-up") });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("gpu-path warm-up"), timestamp_writes: None });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    let submission = queue.submit([encoder.finish()]);
    checked(scopes)?;
    wait_device(device, healthy, submission, SETUP_TIMEOUT)?;
    Ok(())
}

fn medium_failures_tolerated(failed: u64, paths: u64) -> bool {
    failed as f64 <= paths as f64 * FAILURE_FRACTION
}

fn descriptor() -> Result<ExecutorDescriptor> {
    Ok(ExecutorDescriptor {
        id: ExecutorId::new("gpu-path").map_err(gpu)?,
        availability: Availability::Available,
        profiles: vec![RenderProfile {
            mode: RenderMode::Preview,
            operation: RenderOperation::Image,
            formats: vec![RenderFormat::Png],
            features: vec![RenderFeature::Pbr, RenderFeature::Progressive, RenderFeature::Refraction, RenderFeature::Hdri],
        }],
        limits: GPU_PATH_LIMITS,
        scratch: ScratchLayout { sample_scale: 1, sample_bytes_per_pixel: 16, output_bytes_per_pixel: 16, encoding_bytes_per_pixel: 16 },
    })
}

// RADV failed to cover mapped thin-glass pixels with the combined one-medium
// producer. Keep the qualified full stack for optical transport. The nested
// media option still controls admission and the actual boundary transitions.
fn scene_shader(opaque: bool) -> &'static str {
    if opaque { SINGLE_MEDIUM_SHADER } else { SHADER }
}

fn scene_pipeline_variant(opaque: bool, maps: bool, nested: bool) -> Option<usize> {
    if opaque { maps.then_some(0) } else { Some(1 + 2 * usize::from(maps) + usize::from(nested)) }
}

fn use_opaque_pipeline(scene: &ProductScene, options: JobOptions) -> bool {
    !scene.has_transmission()
        && !options.volume.is_some_and(|volume| volume.active())
        && scene.meshes.iter().all(|body| body.opacity == 1.0 && body.faces.iter().all(|face| face.opacity == 1.0))
}

fn admission(scene: &ProductScene, request: &ImageRequest, options: JobOptions) -> Result<Admission> {
    if request.integrator != solidcraft_render::job::Integrator::Raster || request.raytrace.is_some() {
        return Err(JobError::new(ErrorCode::InvalidRequest, "GPU path rendering does not accept CPU raytrace options"));
    }
    options.validate()?;
    photons::validate_scene(scene, options)?;
    if request.studio_weights.is_some_and(|weights| weights.iter().any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))) {
        return Err(JobError::new(ErrorCode::InvalidRequest, "invalid studio visibility weights"));
    }
    if request.studio_weights.is_some() && (options.lighting.is_some() || request.environment.is_some()) {
        return Err(JobError::new(ErrorCode::InvalidRequest, "studio weights require implicit studio lighting"));
    }
    options.aging.validate_scene(scene)?;
    if options.finishing.photographic.active() && request.transparent_background {
        return Err(JobError::new(ErrorCode::InvalidRequest, "photographic effects require opaque output; choose it explicitly"));
    }
    if let Some(volume) = options.volume.filter(|v| v.active()) {
        volume.relative_bounds([request.camera.target.x, request.camera.target.y, request.camera.target.z])?;
        if options.lighting.is_none() && request.environment.is_none() {
            return Err(JobError::new(ErrorCode::InvalidRequest, "volume transport requires explicit studio lighting or an HDR/sky environment"));
        }
        if scene.meshes.iter().any(|body| body.transmission.is_some() || body.faces.iter().any(|face| face.transmission.is_some())) {
            return Err(JobError::new(
                ErrorCode::InvalidScene,
                "this homogeneous volume cannot combine with glass boundaries; disable either explicitly",
            ));
        }
    }
    if request.transparent_background && options.finishing.bloom.is_some_and(|b| b.active()) {
        return Err(JobError::new(ErrorCode::InvalidRequest, "display bloom requires opaque output; no implicit alpha expansion is supported"));
    }
    if options.backdrop.is_some() && request.transparent_background {
        return Err(JobError::new(ErrorCode::InvalidRequest, "choose a camera backdrop or transparent PNG, not both"));
    }
    if let Some(environment) = &request.environment {
        environment.validate()?;
    }
    let scattering = |t: Option<solidcraft_render::transmission::Transmission>| t.and_then(|v| v.subsurface()).is_some_and(|s| s.active());
    if scene.meshes.iter().any(|b| scattering(b.transmission) || b.faces.iter().any(|f| scattering(f.transmission)))
        && (options.denoise || options.transmission_samples != 1 || options.volume.is_some_and(|v| v.active()))
    {
        return Err(JobError::new(
            ErrorCode::InvalidRequest,
            "subsurface random walks require disabled surface denoising, one Fresnel stratum and no external volume",
        ));
    }
    let camera = request.camera;
    validate_punctual_origin(options, camera.target)?;
    if options.lens.is_some_and(|lens| lens.aperture_radius_mm > 0.0)
        && (camera.distance < 1e-6 || !(1e-6..=1e6).contains(&(camera.half_height() / camera.distance)) || camera.fov == 0.0)
    {
        return Err(JobError::new(
            ErrorCode::InvalidRequest,
            "thin-lens depth of field requires a bounded perspective camera (distance >= 0.000001 mm and tan(fov/2) in 0.000001..1000000)",
        ));
    }
    if !camera.fov.is_finite()
        || !(0.0..std::f64::consts::PI).contains(&camera.fov)
        || !camera.half_height().is_finite()
        || camera.half_height() > 1e12
        || camera.distance > 1e9
    {
        return Err(JobError::new(ErrorCode::InvalidRequest, "gpu-path requires a bounded finite camera"));
    }
    let mut admission = admit_geometry_with_limits(scene, request, MAX_TRIANGLES, MAX_GPU_PATH_BODIES, 2 * 1024 * 1024 * 1024, GPU_PATH_LIMITS)?;
    if scene
        .meshes
        .iter()
        .any(|m| (m.opacity != 1.0 && m.transmission.is_none()) || m.faces.iter().any(|f| f.opacity != 1.0 && f.transmission.is_none()))
    {
        return Err(JobError::new(ErrorCode::InvalidScene, "gpu-path requires explicit transmission for nonopaque preview appearances"));
    }
    // Peak preparation: f64 triangles + nodes + serialized upload. After preparation:
    // serialized + device geometry + accumulation/readback/RGBA/PNG. These phases don't overlap.
    let pixels = u64::from(request.width) * u64::from(request.height);
    let denoise_bytes = pixels * if options.denoise { denoise::BYTES_PER_PIXEL + if options.adaptive.is_some() { 16 } else { 0 } } else { 0 };
    let optical_tile_bytes = glass_tile_bytes(scene, request);
    admission.pixel_scratch_bytes = pixels * 64 + denoise_bytes;
    let vertex_bytes = scene.meshes.iter().map(|m| m.mesh.positions.len() as u64 * 32).sum::<u64>();
    let model_bytes = admission.triangles * scene::model_bytes_per_triangle(scene);
    admission.geometry_bytes += model_bytes;
    let triangle_preparation =
        (admission.triangles * 400_u64.max(scene::planar_preparation_bytes_per_triangle() as u64)).max(scene::medium_preparation_bytes(scene));
    admission.preparation_bytes = (triangle_preparation + vertex_bytes * 2 + admission.face_appearances * 8)
        .max(admission.triangles * 264 + vertex_bytes * 2 + pixels * 32 + denoise_bytes)
        .max(admission.triangles * 132 + vertex_bytes + admission.pixel_scratch_bytes)
        + 1_048_576
        + 2 * DIRECTIONAL_PARAMETER_BYTES
        + 2 * PUNCTUAL_PARAMETER_BYTES
        + 2 * PUNCTUAL_COUNTER_BYTES;
    // Optional material lane is retained in serialized/device geometry while
    // preparation also owns its serialization scratch. Legacy scenes add zero.
    admission.preparation_bytes += model_bytes * 2;
    admission.preparation_bytes += request.environment.as_ref().map_or(0, |source| source.admitted_bytes());
    if let Some(textures) = &request.textures {
        textures.validate_scene(scene)?;
        admission.preparation_bytes += textures.working_bytes();
    }
    if options.linear_exr {
        admission.preparation_bytes = admission
            .preparation_bytes
            .checked_add(solidcraft_render::job::linear_exr::encoded_limit(request.width, request.height)?)
            .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "linear master admission overflow"))?;
    }
    let bloom_bytes = options.finishing.bloom.map_or(Ok(0), |b| b.scratch_bytes(request.width, request.height))?;
    admission.pixel_scratch_bytes = admission
        .pixel_scratch_bytes
        .checked_add(bloom_bytes)
        .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "bloom pixel estimate overflow"))?;
    admission.preparation_bytes = admission
        .preparation_bytes
        .checked_add(bloom_bytes)
        .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "bloom preparation estimate overflow"))?;
    let photo_bytes = options.finishing.photographic.scratch_bytes(request.width, request.height)?;
    admission.pixel_scratch_bytes += photo_bytes;
    admission.preparation_bytes += photo_bytes;
    // Two pilot/budget planes plus a second readback plane. Adaptive denoising's
    // independent normalized preview input is included in denoise_bytes above.
    let adaptive_bytes = pixels * if options.adaptive.is_some() { 48 } else { 0 };
    admission.pixel_scratch_bytes += adaptive_bytes + optical_tile_bytes;
    admission.preparation_bytes += adaptive_bytes + optical_tile_bytes;
    admission.preparation_bytes = admission
        .preparation_bytes
        .checked_add(photons::admission_bytes(options.caustics)?)
        .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "caustic photon admission overflow"))?;
    admission.estimated_total_bytes = admission.geometry_bytes + admission.preparation_bytes;
    if admission.estimated_total_bytes > GPU_PATH_LIMITS.max_scratch_bytes {
        return Err(JobError::new(ErrorCode::LimitExceeded, "gpu-path job exceeds its bounded 8 GiB admission estimate"));
    }
    Ok(admission)
}

impl ImageExecutor for Executor {
    fn supports_glass_models(&self) -> bool {
        true
    }
    fn supports_caustics(&self) -> bool {
        true
    }
    fn preview_samples(&self, options: JobOptions) -> Option<u32> {
        options.preview_target().ok()
    }
    fn supports_aging(&self) -> bool {
        true
    }
    fn supports_photographic(&self) -> bool {
        true
    }
    fn supports_subsurface(&self) -> bool {
        true
    }
    fn supports_nested_media(&self) -> bool {
        true
    }
    fn supports_volume(&self) -> bool {
        true
    }
    fn supports_adaptive_sampling(&self) -> bool {
        true
    }
    fn supports_transmission_sampling(&self) -> bool {
        true
    }
    fn supports_bloom(&self) -> bool {
        true
    }
    fn supports_finishing(&self) -> bool {
        true
    }
    fn supports_linear_exr(&self) -> bool {
        true
    }
    fn memory_limit_bytes(&self) -> u64 {
        GPU_PATH_LIMITS.max_scratch_bytes
    }
    fn descriptor(&self) -> Result<ExecutorDescriptor> {
        let mut value = descriptor()?;
        if !self.context.healthy.load(Ordering::Acquire) {
            value.availability = Availability::Unavailable { reason: "GPU path device was lost or failed to complete work; restart renderer".into() };
        }
        Ok(value)
    }
    fn admit(&self, scene: &ProductScene, request: &ImageRequest, options: JobOptions) -> Result<Admission> {
        self.context.ensure_available()?;
        let a = admission(scene, request, options)?;
        let device_limits = self.context.device.limits();
        if let Some(caustics) = options.caustics {
            if device_limits.max_storage_buffers_per_shader_stage < 7 {
                return Err(JobError::new(
                    ErrorCode::HostUnavailable,
                    "caustics require seven GPU storage buffers; ordinary GPU images remain available",
                ));
            }
            let photon_bytes = photons::buffer_bytes(caustics)?;
            if photon_bytes > device_limits.max_storage_buffer_binding_size || photon_bytes > device_limits.max_buffer_size {
                return Err(JobError::new(ErrorCode::LimitExceeded, "caustic photon storage exceeds the selected GPU buffer limit"));
            }
        }
        if PARAMETER_BYTES as u64 > device_limits.max_uniform_buffer_binding_size
            || PARAMETER_BYTES as u64 > device_limits.max_buffer_size
            || FAILURE_BYTES > device_limits.max_storage_buffer_binding_size
        {
            return Err(JobError::new(ErrorCode::LimitExceeded, "GPU light parameters or diagnostic storage exceed device buffer limits"));
        }
        let limit = device_limits.max_storage_buffer_binding_size;
        let geometry_bytes = 16
            + a.triangles.max(1) * (100 + scene::model_bytes_per_triangle(scene))
            + scene.meshes.iter().map(|m| m.mesh.positions.len() as u64 * 32).sum::<u64>();
        let texture_bytes = request.textures.as_ref().map_or(16, |t| textures::packed_bytes(t, scene.meshes.len()));
        let environment_bytes =
            request.environment.as_ref().map_or(48, |source| 16 + u64::from(source.info().width) * u64::from(source.info().height) * 32);
        let node_bytes = a.triangles.max(1) * 64;
        let accumulation_bytes = u64::from(request.width) * u64::from(request.height) * if options.adaptive.is_some() { 48 } else { 16 }
            + glass_tile_bytes(scene, request);
        if node_bytes > limit
            || texture_bytes > limit
            || environment_bytes > limit
            || geometry_bytes > limit
            || accumulation_bytes > limit
            || (options.denoise && u64::from(request.width) * u64::from(request.height) * 48 > limit)
        {
            return Err(JobError::new(ErrorCode::LimitExceeded, "gpu-path storage buffer exceeds this device's limit"));
        }
        Ok(a)
    }
    fn begin(&self, scene: ProductScene, request: ImageRequest, options: JobOptions, cancel: CancelToken) -> Result<Box<dyn ImageWork>> {
        self.admit(&scene, &request, options)?;
        let mut task = Task::new(self.context.clone(), scene, request, options, cancel)?;
        task.preview_enabled = true;
        Ok(Box::new(task))
    }
}

/// Build all fallible CPU environment data before opening GPU error scopes.
fn prepare_environment(request: &ImageRequest, cancel: &CancelToken) -> Result<Vec<u8>> {
    cancel.check()?;
    let mut environment_bytes = Vec::new();
    if let Some(source) = &request.environment {
        let prepared = source.prepare(cancel)?;
        environment_bytes = scene::reserved(16 + prepared.texels().len() * 32)?;
        let [width, height] = prepared.dimensions();
        for x in [width, height, width * height, 0] {
            environment_bytes.extend_from_slice(&x.to_le_bytes());
        }
        for (index, texel) in prepared.texels().iter().enumerate() {
            if index % 1024 == 0 {
                cancel.check()?;
            }
            for x in texel.rgb.into_iter().chain([texel.pdf, texel.threshold]) {
                environment_bytes.extend_from_slice(&x.to_le_bytes());
            }
            environment_bytes.extend_from_slice(&texel.alias.to_le_bytes());
            environment_bytes.extend_from_slice(&[0; 8]);
        }
    } else {
        environment_bytes.resize(48, 0);
    }
    Ok(environment_bytes)
}

fn punctual_parameters(light: &solidcraft_render::job::PunctualLightOptions, origin: Vec3) -> [[f32; 4]; 5] {
    let p = Vec3::new(light.position_mm[0], light.position_mm[1], light.position_mm[2]) - origin;
    let hi = [p.x as f32, p.y as f32, p.z as f32];
    let lo = [(p.x - f64::from(hi[0])) as f32, (p.y - f64::from(hi[1])) as f32, (p.z - f64::from(hi[2])) as f32];
    let (axis, inner, outer, enabled) = if let Some(cone) = light.cone {
        let [x, y, z] = cone.emitted_axis;
        let scale = x.abs().max(y.abs()).max(z.abs());
        let v = Vec3::new(x / scale, y / scale, z / scale);
        let v = v / v.len();
        ([v.x as f32, v.y as f32, v.z as f32], cone.inner_half_angle_radians.cos() as f32, cone.outer_half_angle_radians.cos() as f32, 1.0)
    } else {
        ([0.0; 3], 0.0, 0.0, 0.0)
    };
    [
        [hi[0], hi[1], hi[2], light.reference_distance_mm as f32],
        [lo[0], lo[1], lo[2], light.minimum_distance_mm as f32],
        [light.color[0] as f32, light.color[1] as f32, light.color[2] as f32, light.intensity as f32],
        [axis[0], axis[1], axis[2], inner],
        [outer, enabled, 0.0, 0.0],
    ]
}
fn validate_punctual_origin(options: JobOptions, origin: Vec3) -> Result<()> {
    if let Some(lighting) = options.lighting {
        for light in lighting.punctual_lights.iter() {
            light.validate_gpu()?;
            if punctual_parameters(light, origin).iter().flatten().any(|v| !v.is_finite()) {
                return Err(JobError::new(ErrorCode::InvalidRequest, "point or spot position is outside the GPU camera-relative range"));
            }
        }
    }
    Ok(())
}

struct Task {
    dispatch_budget: dispatch::Budget,
    context: Arc<Context>,
    request: ImageRequest,
    options: JobOptions,
    #[cfg(test)]
    opaque_scene: bool,
    #[cfg(test)]
    has_textures: bool,
    cancel: CancelToken,
    uniform: wgpu::Buffer,
    accumulation: wgpu::Buffer,
    failures: wgpu::Buffer,
    readback: wgpu::Buffer,
    bind: wgpu::BindGroup,
    render_pipeline: wgpu::ComputePipeline,
    optical_resolve: Option<wgpu::ComputePipeline>,
    photons: Option<photons::Resources>,
    denoiser: Option<denoise::Resources>,
    sample: u32,
    lane: u32,
    sampling_report: Option<solidcraft_render::job::adaptive::SamplingReport>,
    punctual_diagnostics: std::cell::Cell<solidcraft_render::job::PunctualDiagnostics>,
    volume: [[f32; 4]; 3],
    aging: [[f32; 20]; 8],
    photographic: Option<solidcraft_render::job::photographic::PhotographicTask>,
    offset: u32,
    epsilon: f32,
    png: Option<PngTask>,
    encoding_preview: bool,
    preview_enabled: bool,
    preview: Option<solidcraft_render::job::ImageWorkPreview>,
    exr_work: Option<solidcraft_render::job::linear_exr::ExrTask>,
    bloom_work: Option<solidcraft_render::job::bloom::BloomTask>,
    exr: Option<Vec<u8>>,
    started: Instant,
    // Uniform environment is used only by the numerical furnace test, never advertised as HDRI.
    environment: f32,
    environment_bottom_only: bool,
    studio: f32,
    #[cfg(test)]
    metrics: (f64, u64, f64),
}
impl Task {
    fn new(context: Arc<Context>, scene: ProductScene, mut request: ImageRequest, options: JobOptions, cancel: CancelToken) -> Result<Self> {
        cancel.check()?;
        options.validate()?;
        validate_punctual_origin(options, request.camera.target)?;
        let opaque_scene = use_opaque_pipeline(&scene, options);
        let has_textures = request.textures.is_some();
        let start = Instant::now();
        let aging = options.aging.gpu_layers([request.camera.target.x, request.camera.target.y, request.camera.target.z])?;
        let mut volume = if let Some(v) = options.volume.filter(|v| v.active()) {
            let bounds = v.relative_bounds([request.camera.target.x, request.camera.target.y, request.camera.target.z])?;
            [
                [bounds[0][0], bounds[0][1], bounds[0][2], v.extinction_per_mm as f32],
                [bounds[1][0], bounds[1][1], bounds[1][2], v.anisotropy as f32],
                [v.scattering_albedo[0] as f32, v.scattering_albedo[1] as f32, v.scattering_albedo[2] as f32, 0.0],
            ]
        } else {
            [[0.0; 4]; 3]
        };
        if let Some(resources) = &request.textures {
            resources.validate_scene(&scene)?;
        }
        let texture_upload = textures::flatten(request.textures.as_deref(), scene.meshes.len(), request.camera.target, &cancel)?;
        request.textures = None; // GPU upload owns decoded pixels; release captured source bytes.
        let upload = scene::flatten_with_media(&scene, request.camera.target, &cancel, options.nested_media)?;
        // This uniform lane was spare. Keep the material offset mask that the
        // mapped opaque shader has qualified on RADV.
        volume[2][3] = if upload.active_glass_model { 1.0 } else { 0.0 };
        photons::validate_punctual_origins(&scene, request.camera.target, options, upload.epsilon, &cancel)?;
        let bvh_ms = start.elapsed().as_secs_f64() * 1000.0;
        let source_count = options.lighting.map_or_else(
            || if request.environment.is_none() { 3 } else { 0 },
            |lighting| lighting.directional_lights.iter().count() as u32 + lighting.punctual_lights.iter().count() as u32,
        );
        let photons = options.caustics.map(|options| photons::Resources::new(&context, options, &upload.nodes, source_count)).transpose()?;
        cancel.check()?;
        let glass_lighting = needs_glass_lighting(&scene);
        let optical_tile_bytes = glass_tile_bytes(&scene, &request);
        let optical_pipelines = if glass_lighting {
            Some(glass_pipeline(&context, photons.as_ref().map_or(&context.layout, |resources| &resources.layout), photons.is_some(), &cancel)?)
        } else {
            None
        };
        let render_pipeline = if let Some(pipelines) = &optical_pipelines {
            pipelines.camera.clone()
        } else if let Some(resources) = &photons {
            resources.render.clone()
        } else {
            context.prepare_scene_pipeline(opaque_scene, has_textures, options.nested_media)?.clone()
        };
        let optical_resolve = optical_pipelines.map(|pipelines| pipelines.resolve);
        cancel.check()?;
        let mut upload_bytes = (upload.geometry.len() + upload.nodes.len() + texture_upload.len()) as u64;
        let environment_bytes = prepare_environment(&request, &cancel)?;
        cancel.check()?;
        let uploading = Instant::now();
        drop(scene);
        let device = &context.device;
        let (uniform, accumulation, failures, readback, bind, denoiser) = checked_creation(device, || {
            let make = |label, bytes: &[u8], usage| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: bytes, usage })
            };
            upload_bytes += environment_bytes.len() as u64;
            let environment = make("path environment", &environment_bytes, wgpu::BufferUsages::STORAGE);
            drop(environment_bytes);
            let triangles = make("path triangles", &upload.geometry, wgpu::BufferUsages::STORAGE);
            let nodes = make("path BVH", &upload.nodes, wgpu::BufferUsages::STORAGE);
            let texture_buffer = make("path opaque textures", &texture_upload, wgpu::BufferUsages::STORAGE);
            let uniform = make("path parameters", &[0; PARAMETER_BYTES], wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
            let size = u64::from(request.width) * u64::from(request.height) * 16;
            let accumulation = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("path accumulation"),
                size: size * if options.adaptive.is_some() { 3 } else { 1 } + optical_tile_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            // Inconsistent-medium path count and the first such path's location (see read_linear).
            let failures = make(
                "path medium failures",
                &[0; FAILURE_BYTES as usize],
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            );
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("path readback"),
                size: size * if options.adaptive.is_some() { 2 } else { 1 } + FAILURE_BYTES,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut entries: Vec<_> = [&uniform, &triangles, &nodes, &accumulation, &environment]
                .iter()
                .enumerate()
                .map(|(binding, buffer)| wgpu::BindGroupEntry { binding: binding as u32, resource: buffer.as_entire_binding() })
                .collect();
            entries.push(wgpu::BindGroupEntry { binding: 5, resource: texture_buffer.as_entire_binding() });
            entries.push(wgpu::BindGroupEntry { binding: 6, resource: failures.as_entire_binding() });
            if let Some(resources) = &photons {
                entries.push(wgpu::BindGroupEntry { binding: 7, resource: resources.buffer.as_entire_binding() });
            }
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("gpu-path"),
                layout: photons.as_ref().map_or(&context.layout, |resources| &resources.layout),
                entries: &entries,
            });
            let denoiser = if options.denoise {
                Some(denoise::Resources::new(
                    &context,
                    &uniform,
                    [&triangles, &nodes, &environment, &texture_buffer, &failures],
                    &accumulation,
                    size,
                    options.adaptive.is_some(),
                )?)
            } else {
                None
            };
            Ok((uniform, accumulation, failures, readback, bind, denoiser))
        })?;
        let submission = context.queue.submit([]);
        context.wait(submission, TIMEOUT)?;
        let upload_ms = uploading.elapsed().as_secs_f64() * 1000.0;
        log::info!("gpu-path BVH {bvh_ms:.3} ms, upload {upload_bytes} bytes in {upload_ms:.3} ms");
        cancel.check()?;
        if let Some(resources) = &photons {
            resources.warm_up_actual(&context, &uniform, &bind, &cancel)?;
        }
        if glass_lighting {
            cancel.check()?;
            context.ensure_available()?;
            checked_creation(&context.device, || {
                context.queue.write_buffer(&uniform, 0, &[0; PARAMETER_BYTES]);
                Ok(())
            })?;
            warm_up(&context.device, &context.queue, &render_pipeline, &bind, &context.healthy)?;
            cancel.check()?;
            if let Some(resolve) = &optical_resolve {
                warm_up(&context.device, &context.queue, resolve, &bind, &context.healthy)?;
            };
            cancel.check()?;
        }
        Ok(Self {
            dispatch_budget: Default::default(),
            #[cfg(test)]
            opaque_scene,
            #[cfg(test)]
            has_textures,
            context,
            request,
            options,
            cancel,
            uniform,
            accumulation,
            failures,
            readback,
            bind,
            render_pipeline,
            optical_resolve,
            photons,
            denoiser,
            sample: 0,
            lane: 0,
            sampling_report: None,
            punctual_diagnostics: Default::default(),
            volume,
            aging,
            photographic: None,
            offset: 0,
            epsilon: upload.epsilon,
            png: None,
            encoding_preview: false,
            preview_enabled: false,
            preview: None,
            exr_work: None,
            bloom_work: None,
            exr: None,
            started: Instant::now(),
            environment: 0.0,
            environment_bottom_only: false,
            studio: 1.0,
            #[cfg(test)]
            metrics: (bvh_ms, upload_bytes, upload_ms),
        })
    }
    fn parameters(&self, count: u32) -> Result<Vec<u8>> {
        let mut bytes = scene::reserved(PARAMETER_BYTES)?;
        if bytes.capacity() != PARAMETER_BYTES {
            return Err(JobError::new(ErrorCode::LimitExceeded, "GPU parameter upload exceeds its prepaid capacity"));
        }
        for x in [
            self.request.width,
            self.request.height,
            self.sample,
            self.offset,
            self.options.seed,
            self.options.max_depth,
            count,
            u32::from(self.environment_bottom_only),
        ] {
            bytes.extend_from_slice(&x.to_le_bytes());
        }
        let camera = self.request.camera;
        let (right, up, back) = camera.basis();
        let perspective = camera.fov > 0.0;
        let eye = back * (camera.distance * if perspective { 1.0 } else { 4.0 });
        scene::vector(&mut bytes, eye, if perspective { 1.0 } else { 0.0 });
        scene::vector(
            &mut bytes,
            right * camera.half_height() * (f64::from(self.request.width) / f64::from(self.request.height)),
            self.options.lens.map_or(0.0, |l| l.aperture_radius_mm as f32),
        );
        scene::vector(&mut bytes, up * camera.half_height(), self.options.lens.map_or(0.0, |l| l.focus_distance_mm as f32));
        scene::vector(&mut bytes, back, camera.distance as f32);
        scene::vector(
            &mut bytes,
            Vec3::new(f64::from(self.epsilon), f64::from(self.environment), f64::from(self.studio)),
            f32::from(u8::from(self.options.nested_media)),
        );
        let lighting = self.options.lighting.unwrap_or_default();
        scene::vector(
            &mut bytes,
            Vec3::new(lighting.environment[0], lighting.environment[1], lighting.environment[2]),
            if self.options.lighting.is_some() || self.request.environment.is_some() { 1.0 } else { 0.0 },
        );
        scene::vector(
            &mut bytes,
            Vec3::new(lighting.directional_lights.iter().count() as f64, lighting.punctual_lights.iter().count() as f64, 0.0),
            lighting.area_lights.iter().count() as f32,
        );
        for light in lighting.area_lights.iter() {
            let vector = |[x, y, z]: [f64; 3]| Vec3::new(x, y, z);
            scene::vector(&mut bytes, vector(light.center) - camera.target, light.area() as f32);
            scene::vector(&mut bytes, vector(light.u), 0.0);
            scene::vector(&mut bytes, vector(light.v), 0.0);
            scene::vector(&mut bytes, vector(light.radiance), 0.0);
        }
        bytes.resize(400, 0);
        if let Some(source) = &self.request.environment {
            let info = source.info();
            scene::vector(&mut bytes, Vec3::new(info.rotation_rad.cos(), info.rotation_rad.sin(), info.intensity), 1.0);
        } else {
            scene::vector(&mut bytes, Vec3::ZERO, 0.0);
        }
        if let Some(sun) = lighting.sun {
            scene::vector(&mut bytes, sun.direction(), sun.angular_radius_rad.cos() as f32);
            let [r, g, b] = sun.radiance;
            scene::vector(&mut bytes, Vec3::new(r, g, b), (1.0 / sun.solid_angle()) as f32);
        } else {
            bytes.resize(448, 0);
        }

        let adaptive = self.options.adaptive;
        for value in
            [adaptive.map_or(0, |a| a.pilot_samples), adaptive.map_or(0, |a| a.min_samples), self.options.transmission_samples, self.options.samples]
        {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [
            adaptive.map_or(0.0, |a| a.relative_error as f32),
            adaptive.map_or(0.0, |a| a.absolute_error as f32),
            adaptive.map_or(0.0, |a| a.coverage_error as f32),
            self.lane as f32,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for vector in self.volume {
            for value in vector {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        for layer in self.aging {
            for value in layer {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        for light in lighting.directional_lights.iter() {
            // Validation rejects zero/nonfinite input; scaled normalization preserves
            // every bounded CPU directional intent, including tiny finite vectors.
            let [x, y, z] = light.direction;
            let scale = x.abs().max(y.abs()).max(z.abs());
            let direction = Vec3::new(x / scale, y / scale, z / scale);
            scene::vector(&mut bytes, direction / direction.len(), light.intensity as f32);
            let [r, g, b] = light.color;
            scene::vector(&mut bytes, Vec3::new(r, g, b), 0.0);
        }
        bytes.resize(PARAMETER_BYTES - PUNCTUAL_PARAMETER_BYTES as usize - 16, 0);
        for light in lighting.punctual_lights.iter() {
            for vector in punctual_parameters(light, camera.target) {
                for value in vector {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
        bytes.resize(PARAMETER_BYTES - 16, 0);
        let weights = self.request.studio_weights.unwrap_or([1.0; 3]);
        for value in [weights[0] as f32, weights[1] as f32, weights[2] as f32, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        Ok(bytes)
    }
    fn normalization_samples(&self) -> u32 {
        if self.options.adaptive.is_some() { 1 } else { self.sample }
    }
    fn target_samples(&self) -> u32 {
        self.options.samples + self.options.adaptive.map_or(0, |a| a.pilot_samples)
    }
    fn dispatch(&mut self) -> Result<()> {
        self.cancel.check()?;
        self.context.ensure_available()?;
        if let Some(resources) = self.photons.as_ref().filter(|resources| !resources.ready()) {
            let parameters = self.parameters(resources.count())?;
            if let Some(resources) = &mut self.photons {
                resources.step(&self.context, &self.uniform, &self.bind, &parameters, &self.cancel)?;
            }
            return Ok(());
        }
        let dispatch_started = Instant::now();
        let pixels = self.request.width * self.request.height;
        let count = self.dispatch_budget.count(pixels - self.offset);
        let context = &self.context;
        let scopes = scopes(&context.device);
        context.queue.write_buffer(&self.uniform, 0, &self.parameters(count)?);
        let mut encoder = context.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("path tile") });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("path tile"), timestamp_writes: None });
            pass.set_pipeline(&self.render_pipeline);
            pass.set_bind_group(0, &self.bind, &[]);
            pass.dispatch_workgroups(count.div_ceil(PATH_WORKGROUP_SIZE), 1, 1);
        }
        if let Some(resolve) = &self.optical_resolve {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("path optical resolve"), timestamp_writes: None });
            pass.set_pipeline(resolve);
            pass.set_bind_group(0, &self.bind, &[]);
            pass.dispatch_workgroups(count.div_ceil(PATH_WORKGROUP_SIZE), 1, 1);
        }
        let submission = context.queue.submit([encoder.finish()]);
        checked(scopes)?;
        context.wait(submission, TIMEOUT)?;
        self.dispatch_budget.observe(count, dispatch_started.elapsed());
        self.cancel.check()?;
        self.offset += count;
        if self.offset == pixels {
            self.offset = 0;
            let strata = if self.options.adaptive.is_some_and(|a| self.sample < a.pilot_samples) { 1 } else { self.options.transmission_samples };
            self.lane += 1;
            if self.lane == strata {
                self.sample += 1;
                self.lane = 0;
            }
        }
        Ok(())
    }
    #[cfg(test)]
    fn complete_sample(&mut self) -> Result<()> {
        let wanted = self.sample.checked_add(1).ok_or_else(|| gpu("sample counter overflow"))?;
        let maximum = (self.request.width * self.request.height)
            .div_ceil(dispatch::MIN_PATHS)
            .checked_mul(self.options.transmission_samples)
            .and_then(|count| count.checked_add(self.photons.as_ref().map_or(0, photons::Resources::maximum_quanta)))
            .ok_or_else(|| gpu("test sample wave count overflow"))?;
        for _ in 0..maximum {
            self.dispatch()?;
            if self.sample == wanted {
                return Ok(());
            }
        }
        Err(gpu("bounded full-sample test helper did not advance"))
    }
    fn read_linear(&self) -> Result<Vec<[f32; 4]>> {
        self.read_linear_buffer(&self.accumulation)
    }
    fn read_linear_buffer(&self, source: &wgpu::Buffer) -> Result<Vec<[f32; 4]>> {
        self.read_pixels(source, false)
    }
    fn read_checkpoint(&self) -> Result<Vec<[f32; 4]>> {
        self.read_pixels(&self.accumulation, self.options.adaptive.is_some())
    }
    fn read_pixels(&self, source: &wgpu::Buffer, adaptive_preview: bool) -> Result<Vec<[f32; 4]>> {
        self.cancel.check()?;
        let context = &self.context;
        let scopes = scopes(&context.device);
        let mut encoder = context.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("path readback") });
        let pixel_bytes = u64::from(self.request.width) * u64::from(self.request.height) * 16;
        let pilot = adaptive_preview && self.options.adaptive.is_some_and(|options| self.sample <= options.pilot_samples);
        encoder.copy_buffer_to_buffer(source, if pilot { pixel_bytes } else { 0 }, &self.readback, 0, pixel_bytes);
        if adaptive_preview && !pilot {
            encoder.copy_buffer_to_buffer(&self.accumulation, pixel_bytes * 2, &self.readback, pixel_bytes, pixel_bytes);
        }
        let failure_offset = pixel_bytes * if self.options.adaptive.is_some() { 2 } else { 1 };
        encoder.copy_buffer_to_buffer(&self.failures, 0, &self.readback, failure_offset, FAILURE_BYTES);
        let submission = context.queue.submit([encoder.finish()]);
        checked(scopes)?;
        let slice = self.readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        context.wait(submission, TIMEOUT)?;
        rx.try_recv().map_err(gpu)?.map_err(gpu)?;
        let data = slice.get_mapped_range().map_err(gpu)?;
        let result = (|| {
            let pixels = data.get(..pixel_bytes as usize).ok_or_else(|| gpu("missing image readback"))?;
            let failures = data.get(failure_offset as usize..).ok_or_else(|| gpu("missing failure readback"))?;
            let budgets = if adaptive_preview && !pilot {
                Some(data.get(pixel_bytes as usize..(pixel_bytes * 2) as usize).ok_or_else(|| gpu("missing adaptive preview budgets"))?)
            } else {
                None
            };
            let word = |i: usize| failures.as_chunks::<4>().0.get(i).map_or(0, |w| u32::from_le_bytes(*w));
            if word(7) & 16 != 0 {
                return Err(JobError::new(ErrorCode::InvalidScene, "rough optical response exceeds GPU arithmetic precision"));
            }
            if word(7) & 8 != 0 {
                return Err(JobError::new(ErrorCode::InvalidScene, "optical light-event count exceeds the admitted camera depth"));
            }
            if word(7) & 4 != 0 {
                return Err(JobError::new(ErrorCode::InvalidScene, "caustic photon medium origin or transition is ambiguous"));
            }
            if word(7) & 2 != 0 {
                return Err(JobError::new(ErrorCode::InvalidScene, "positive-thickness index-matched sheet has no finite grazing optical path"));
            }
            if word(7) != 0 {
                return Err(JobError::new(ErrorCode::Internal, "nonfinite or negative pilot/production path radiance"));
            }
            self.punctual_diagnostics.set(solidcraft_render::job::PunctualDiagnostics {
                near_source: u64::from(word(8)) | (u64::from(word(9)) << 32),
                radiance_limit: u64::from(word(10)) | (u64::from(word(11)) << 32),
                shadow_offset_crossed: u64::from(word(12)) | (u64::from(word(13)) << 32),
            });
            let failed = u64::from(word(0)) | (u64::from(word(5)) << 32);
            if failed > 0 {
                let pixels = u64::from(self.request.width) * u64::from(self.request.height);
                let paths = if adaptive_preview {
                    adaptive::preview_paths(self, budgets)?
                } else {
                    self.sampling_report.as_ref().map_or(
                        pixels * u64::from(self.sample.max(1)) * u64::from(self.options.transmission_samples),
                        |report| {
                            report.production_samples * u64::from(report.transmission_strata) + pixels * u64::from(report.pilot_samples_per_pixel)
                        },
                    )
                };
                let detail =
                    format!("{failed} of {paths} paths, first at pixel {}, bounce {}, sample {}, code {}", word(1), word(2), word(3), word(4));
                if !medium_failures_tolerated(failed, paths) {
                    return Err(JobError::new(
                        ErrorCode::InvalidScene,
                        format!(
                            "inconsistent glass medium paths ({detail}): ambiguous boundary origin or invalid containment; overlapping solids or more than 16 nested media are unsupported"
                        ),
                    ));
                }
                log::warn!("gpu-path ended {detail} at an unclassifiable glass medium");
            }
            let mut values = scene::reserved((self.request.width * self.request.height) as usize)?;
            for (index, pixel) in pixels.as_chunks::<16>().0.iter().enumerate() {
                if index % 4096 == 0 {
                    self.cancel.check()?;
                }
                let divisor = if adaptive_preview {
                    adaptive::preview_divisor(&self.options, self.sample, budgets.and_then(|values| values.get(index * 16..index * 16 + 4)))?
                } else {
                    self.normalization_samples() as f32
                };
                let mut value = [0.0; 4];
                for (v, bytes) in value.iter_mut().zip(pixel.as_chunks::<4>().0) {
                    *v = f32::from_le_bytes(*bytes) / divisor;
                    if !v.is_finite() || *v < 0.0 {
                        return Err(JobError::new(ErrorCode::Internal, "nonfinite or negative path radiance"));
                    }
                }
                values.push(value);
            }
            Ok(values)
        })();
        drop(data);
        self.readback.unmap();
        self.cancel.check()?;
        result
    }
}
use solidcraft_render::job::presentation::Display;

impl ImageWork for Task {
    fn punctual_diagnostics(&self) -> Option<solidcraft_render::job::PunctualDiagnostics> {
        self.options.lighting.filter(|l| !l.punctual_lights.is_empty()).map(|_| self.punctual_diagnostics.get())
    }

    fn take_preview(&mut self) -> Option<solidcraft_render::job::ImageWorkPreview> {
        self.preview.take()
    }
    fn sampling_report(&self) -> Option<solidcraft_render::job::adaptive::SamplingReport> {
        self.sampling_report.clone()
    }
    fn take_linear_exr(&mut self) -> Option<Vec<u8>> {
        self.exr.take()
    }
    fn step(&mut self, units: usize) -> Result<TaskPoll> {
        self.cancel.check()?;
        self.context.ensure_available()?;
        // Direct executor consumers may ignore previews; dropping their old slot
        // here never changes accumulated samples or final output. The image host
        // takes every publication immediately after this quantum.
        self.preview = None;
        if let Some(png) = &mut self.png {
            let poll = png.step(units)?;
            if self.encoding_preview
                && let TaskPoll::Complete(png) = poll
            {
                self.png = None;
                self.encoding_preview = false;
                self.preview = Some(solidcraft_render::job::ImageWorkPreview { png, samples_per_pixel: self.sample });
                return Ok(TaskPoll::Pending(Progress {
                    phase: Phase::Running,
                    done: u64::from(self.sample),
                    total: u64::from(self.target_samples()),
                }));
            }
            return Ok(poll);
        }
        if let Some(mut task) = self.photographic.take() {
            if !task.step(units)? {
                let (done, total) = task.progress();
                self.photographic = Some(task);
                return Ok(TaskPoll::Pending(Progress { phase: Phase::Encoding, done, total }));
            }
            self.png = Some(PngTask::new(&self.request, task.finish()?, self.cancel.clone())?);
            return Ok(TaskPoll::Pending(Progress { phase: Phase::Encoding, done: 0, total: u64::from(self.request.height) }));
        }
        if let Some(mut bloom) = self.bloom_work.take() {
            if !bloom.step(8)? {
                let done = bloom.completed_lines();
                let total = bloom.total_lines();
                self.bloom_work = Some(bloom);
                return Ok(TaskPoll::Pending(Progress { phase: Phase::Encoding, done, total }));
            }
            let (pixels, glow) = bloom.finish()?;
            return self.encode_output(pixels, Some(glow));
        }
        if self.sample < self.target_samples() {
            let before = self.sample;
            self.dispatch()?;
            if let Some(resources) = self.photons.as_ref().filter(|resources| !resources.ready()) {
                return Ok(TaskPoll::Pending(resources.progress()));
            }
            if self.preview_enabled && self.sample > before && self.sample < self.target_samples() {
                // sample advances only after every tile and Fresnel stratum of
                // the pass. Readback cannot observe a partially updated film.
                self.encoding_preview = true;
                let linear = self.read_checkpoint()?;
                return self.finish_output(linear);
            }
            return Ok(TaskPoll::Pending(Progress { phase: Phase::Running, done: u64::from(self.sample), total: u64::from(self.target_samples()) }));
        }
        if self.sampling_report.is_none() && (self.options.adaptive.is_some() || self.options.transmission_samples != 1) {
            self.sampling_report = Some(adaptive::report(self)?);
        }
        if self.exr_work.is_none() {
            log::info!("gpu-path {} spp, {:.3} ms/spp", self.sample, self.started.elapsed().as_secs_f64() * 1000.0 / f64::from(self.sample));
        }
        let linear = if let Some(mut work) = self.exr_work.take() {
            if !work.step(4)? {
                let done = work.completed_rows();
                self.exr_work = Some(work);
                return Ok(TaskPoll::Pending(Progress { phase: Phase::Encoding, done, total: u64::from(self.request.height) }));
            }
            let (bytes, pixels) = work.finish()?;
            self.exr = Some(bytes);
            pixels
        } else {
            let pixels = self.read_linear()?;
            if self.options.linear_exr {
                self.exr_work =
                    Some(solidcraft_render::job::linear_exr::ExrTask::new(self.request.width, self.request.height, pixels, self.cancel.clone())?);
                return Ok(TaskPoll::Pending(Progress { phase: Phase::Encoding, done: 0, total: u64::from(self.request.height) }));
            }
            pixels
        };
        self.finish_output(linear)
    }
}
impl Task {
    fn finish_output(&mut self, mut linear: Vec<[f32; 4]>) -> Result<TaskPoll> {
        if let Some(denoiser) = &self.denoiser {
            // Diagnose the raw path buffer before filtering: never hide invalid media
            // or nonfinite radiance by replacing a pixel with neighbouring values.
            drop(linear);
            if self.encoding_preview && self.options.adaptive.is_some() {
                denoiser.run_adaptive_preview(self)?;
            } else {
                denoiser.run(self)?;
            }
            linear = self.read_linear_buffer(denoiser.output())?;
        }
        if let Some(options) = self.options.finishing.bloom.filter(|options| options.active()) {
            self.bloom_work =
                Some(solidcraft_render::job::bloom::BloomTask::new(self.request.width, self.request.height, linear, options, self.cancel.clone())?);
            return Ok(TaskPoll::Pending(Progress {
                phase: Phase::Encoding,
                done: 0,
                total: u64::from(self.request.height) * 4 + u64::from(self.request.width) * 3,
            }));
        }
        self.encode_output(linear, None)
    }
    fn encode_output(&mut self, linear: Vec<[f32; 4]>, bloom: Option<Vec<[f32; 3]>>) -> Result<TaskPoll> {
        let encoded_len = linear.len().checked_mul(4).ok_or_else(|| gpu("display byte length overflow"))?;
        if bloom.as_ref().is_some_and(|pixels| pixels.len() != linear.len()) {
            return Err(gpu("bloom and display pixel counts differ"));
        }
        let mut rgba = scene::reserved(encoded_len)?;
        rgba.resize(encoded_len, 0);
        let display = Display {
            finishing: self.options.finishing,
            backdrop: self.options.backdrop,
            width: self.request.width,
            height: self.request.height,
            exposure: 2.0f64.powf(self.options.exposure_ev),
            tone_map: self.options.tone_map,
            background: self.request.background.map(solidcraft_render::surface::srgb8_to_linear),
            transparent: self.request.transparent_background,
        };
        // Pixels are independent: encode contiguous chunks on every core.
        let workers = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 16);
        let chunk = linear.len().div_ceil(workers).max(1);
        let output_chunk = chunk.checked_mul(4).ok_or_else(|| gpu("display chunk byte length overflow"))?;
        let bloom = bloom.as_deref();
        let cancel = &self.cancel;
        std::thread::scope(|scope| {
            let jobs: Vec<_> = linear
                .chunks(chunk)
                .zip(rgba.chunks_mut(output_chunk))
                .enumerate()
                .map(|(i, (input, output))| {
                    scope.spawn(move || {
                        let offset = i.checked_mul(chunk).ok_or_else(|| gpu("display chunk offset overflow"))?;
                        let glow = bloom_chunk(bloom, offset, input.len())?;
                        display.encode_bloom_at(input, output, cancel, offset, glow)
                    })
                })
                .collect();
            jobs.into_iter()
                .try_for_each(|job| job.join().unwrap_or_else(|_| Err(JobError::new(ErrorCode::Internal, "display encoding worker failed"))))
        })?;
        if self.options.finishing.photographic.active() {
            self.photographic = Some(solidcraft_render::job::photographic::PhotographicTask::new(
                self.request.width,
                self.request.height,
                rgba,
                linear,
                self.options.finishing.photographic,
                self.cancel.clone(),
            )?);
        } else {
            self.png = Some(PngTask::new(&self.request, rgba, self.cancel.clone())?);
        }
        Ok(TaskPoll::Pending(Progress { phase: Phase::Encoding, done: 0, total: u64::from(self.request.height) }))
    }
}

/// A malformed intermediate image is a job error, never an indexing panic in a
/// display worker. Also bound addition before constructing the range.
fn bloom_chunk(pixels: Option<&[[f32; 3]]>, offset: usize, count: usize) -> Result<Option<&[[f32; 3]]>> {
    let Some(pixels) = pixels else { return Ok(None) };
    let end = offset.checked_add(count).ok_or_else(|| gpu("bloom chunk range overflow"))?;
    pixels.get(offset..end).map(Some).ok_or_else(|| gpu("bloom chunk is outside the retained image"))
}
