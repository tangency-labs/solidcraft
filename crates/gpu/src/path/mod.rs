//! Native progressive product rendering. Software BVH traversal needs no ray-query
//! features. Small measured quanta adapt to the device, then check cancellation.
use solidcraft_geom::Vec3;
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
mod scene;
#[cfg(test)]
mod tests;
mod textures;
const SHADER: &str = concat!(
    include_str!("path.wgsl"),
    "\n",
    include_str!("shadows.wgsl"),
    "\n",
    include_str!("textures.wgsl"),
    "
",
    include_str!("volume.wgsl"),
    "
",
    include_str!("aging.wgsl")
);
const MAX_TRIANGLES: u64 = MAX_GPU_PATH_TRIANGLES;
const PARAMETER_BYTES: usize = 1168;
// Eight u32 words: count low, pixel, bounce, sample, code, count high, first-writer flag, spare.
const FAILURE_BYTES: u64 = 32;
// A path that cannot classify its glass medium ends there. Isolated paths at the
// limits of f32 geometry are tolerated and logged; more than this fraction of all
// paths (any systematic error such as a camera inside glass) fails the job.
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
/// The path tracer runs on Vulkan only: a viewport on another backend is reopened on the same
/// GPU through Vulkan, and without one the executor is unavailable and images render on the CPU.
pub fn host_on_adapter(adapter: &wgpu::Adapter) -> Result<ImageHost> {
    let policy = crate::select::Policy::from_env().map_err(gpu)?;
    let info = adapter.get_info();
    if !policy.admits(crate::select::tier_of(info.device_type)) {
        return Err(gpu(format!("viewport adapter does not satisfy SOLIDCRAFT_GPU={}", policy.name())));
    }
    if info.backend != wgpu::Backend::Vulkan {
        let (vulkan, _) = crate::select::pick_vulkan(policy, |i| i.vendor == info.vendor && i.device == info.device)
            .ok_or_else(|| gpu(format!("{VULKAN_ONLY}; the viewport uses {:?}", info.backend)))?;
        let (device, queue) = open_device(&vulkan)?;
        log::info!("gpu-path viewport GPU: {} through Vulkan", info.name);
        return ImageHost::with_executor(Arc::new(Executor::new(device, queue)?));
    }
    let (device, queue) = open_device(adapter)?;
    log::info!("gpu-path viewport adapter: {} ({:?})", info.name, info.device_type);
    ImageHost::with_executor(Arc::new(Executor::new(device, queue)?))
}

// D3D12 shader compilation is avoided; the user can select a CPU image method instead.
const VULKAN_ONLY: &str = "the GPU path tracer requires a Vulkan driver; select a CPU image method when Vulkan is unavailable";

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
    let mut limits = wgpu::Limits::downlevel_defaults().using_resolution(supported);
    limits.max_storage_buffer_binding_size = storage.min(buffer);
    limits.max_buffer_size = buffer;
    limits.max_storage_buffers_per_shader_stage = PATH_STORAGE_BUFFERS;
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
    let (adapter, _) = crate::select::pick_vulkan(policy, |_| true)
        .ok_or_else(|| gpu(format!("{VULKAN_ONLY}; no Vulkan adapter satisfies SOLIDCRAFT_GPU={}", policy.name())))?;
    let (device, queue) = open_device(&adapter)?;
    Ok((adapter, device, queue))
}

struct Context {
    healthy: Arc<AtomicBool>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    // Built and warmed by the first denoised job, then shared by later ones.
    denoise: std::sync::OnceLock<denoise::Pipelines>,
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
        let scopes = scopes(&device);
        let shader =
            device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("gpu-path"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
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
            compilation_options: Default::default(),
            cache: None,
        });
        checked(scopes)?;
        warm_up(&device, &queue, &pipeline, &placeholder_bind(&device, &layout, 7)?, &healthy)?;
        Ok(Self { context: Arc::new(Context { healthy, device, queue, pipeline, layout, denoise: std::sync::OnceLock::new() }) })
    }
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

fn admission(scene: &ProductScene, request: &ImageRequest, options: JobOptions) -> Result<Admission> {
    if request.integrator != solidcraft_render::job::Integrator::Raster || request.raytrace.is_some() {
        return Err(JobError::new(ErrorCode::InvalidRequest, "GPU path rendering does not accept CPU raytrace options"));
    }
    options.validate()?;
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
    let denoise_bytes = pixels * if options.denoise { denoise::BYTES_PER_PIXEL } else { 0 };
    admission.pixel_scratch_bytes = pixels * 64 + denoise_bytes;
    let vertex_bytes = scene.meshes.iter().map(|m| m.mesh.positions.len() as u64 * 32).sum::<u64>();
    let triangle_preparation =
        (admission.triangles * 400_u64.max(scene::planar_preparation_bytes_per_triangle() as u64)).max(scene::medium_preparation_bytes(scene));
    admission.preparation_bytes = (triangle_preparation + vertex_bytes * 2 + admission.face_appearances * 8)
        .max(admission.triangles * 264 + vertex_bytes * 2 + pixels * 32 + denoise_bytes)
        .max(admission.triangles * 132 + vertex_bytes + admission.pixel_scratch_bytes)
        + 1_048_576;
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
    let adaptive_bytes = pixels * if options.adaptive.is_some() { 32 } else { 0 };
    admission.pixel_scratch_bytes += adaptive_bytes;
    admission.preparation_bytes += adaptive_bytes;
    admission.estimated_total_bytes = admission.geometry_bytes + admission.preparation_bytes;
    if admission.estimated_total_bytes > GPU_PATH_LIMITS.max_scratch_bytes {
        return Err(JobError::new(ErrorCode::LimitExceeded, "gpu-path job exceeds its bounded 8 GiB admission estimate"));
    }
    Ok(admission)
}

impl ImageExecutor for Executor {
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
        let limit = self.context.device.limits().max_storage_buffer_binding_size;
        let geometry_bytes = 16 + a.triangles.max(1) * 100 + scene.meshes.iter().map(|m| m.mesh.positions.len() as u64 * 32).sum::<u64>();
        let texture_bytes = request
            .textures
            .as_ref()
            .map_or(16, |t| t.summary().linear_pixel_bytes + 4 * (5 + scene.meshes.len() as u64 + t.bindings().len() as u64 * 24));
        let environment_bytes =
            request.environment.as_ref().map_or(48, |source| 16 + u64::from(source.info().width) * u64::from(source.info().height) * 32);
        let node_bytes = a.triangles.max(1) * 64;
        if node_bytes > limit
            || texture_bytes > limit
            || environment_bytes > limit
            || geometry_bytes > limit
            || u64::from(request.width) * u64::from(request.height) * if options.adaptive.is_some() { 48 } else { 16 } > limit
            || (options.denoise && u64::from(request.width) * u64::from(request.height) * 48 > limit)
        {
            return Err(JobError::new(ErrorCode::LimitExceeded, "gpu-path storage buffer exceeds this device's limit"));
        }
        Ok(a)
    }
    fn begin(&self, scene: ProductScene, request: ImageRequest, options: JobOptions, cancel: CancelToken) -> Result<Box<dyn ImageWork>> {
        self.admit(&scene, &request, options)?;
        Ok(Box::new(Task::new(self.context.clone(), scene, request, options, cancel)?))
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

struct Task {
    dispatch_budget: dispatch::Budget,
    context: Arc<Context>,
    request: ImageRequest,
    options: JobOptions,
    cancel: CancelToken,
    uniform: wgpu::Buffer,
    accumulation: wgpu::Buffer,
    failures: wgpu::Buffer,
    readback: wgpu::Buffer,
    bind: wgpu::BindGroup,
    denoiser: Option<denoise::Resources>,
    sample: u32,
    lane: u32,
    sampling_report: Option<solidcraft_render::job::adaptive::SamplingReport>,
    volume: [[f32; 4]; 3],
    aging: [[f32; 20]; 8],
    photographic: Option<solidcraft_render::job::photographic::PhotographicTask>,
    offset: u32,
    epsilon: f32,
    png: Option<PngTask>,
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
        let start = Instant::now();
        let aging = options.aging.gpu_layers([request.camera.target.x, request.camera.target.y, request.camera.target.z])?;
        let volume = if let Some(v) = options.volume.filter(|v| v.active()) {
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
        let bvh_ms = start.elapsed().as_secs_f64() * 1000.0;
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
                size: size * if options.adaptive.is_some() { 3 } else { 1 },
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
                size: size + FAILURE_BYTES,
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
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("gpu-path"), layout: &context.layout, entries: &entries });
            let denoiser = if options.denoise {
                Some(denoise::Resources::new(
                    &context,
                    &uniform,
                    [&triangles, &nodes, &environment, &texture_buffer, &failures],
                    &accumulation,
                    size,
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
        Ok(Self {
            dispatch_budget: Default::default(),
            context,
            request,
            options,
            cancel,
            uniform,
            accumulation,
            failures,
            readback,
            bind,
            denoiser,
            sample: 0,
            lane: 0,
            sampling_report: None,
            volume,
            aging,
            photographic: None,
            offset: 0,
            epsilon: upload.epsilon,
            png: None,
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
    fn parameters(&self, count: u32) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(PARAMETER_BYTES);
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
        scene::vector(&mut bytes, Vec3::ZERO, lighting.area_lights.iter().count() as f32);
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
        bytes
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
        let dispatch_started = Instant::now();
        let pixels = self.request.width * self.request.height;
        let count = self.dispatch_budget.count(pixels - self.offset);
        let context = &self.context;
        let scopes = scopes(&context.device);
        context.queue.write_buffer(&self.uniform, 0, &self.parameters(count));
        let mut encoder = context.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("path tile") });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("path tile"), timestamp_writes: None });
            pass.set_pipeline(&context.pipeline);
            pass.set_bind_group(0, &self.bind, &[]);
            pass.dispatch_workgroups(count.div_ceil(64), 1, 1);
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
        self.cancel.check()?;
        let context = &self.context;
        let scopes = scopes(&context.device);
        let mut encoder = context.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("path readback") });
        let pixel_bytes = u64::from(self.request.width) * u64::from(self.request.height) * 16;
        encoder.copy_buffer_to_buffer(source, 0, &self.readback, 0, pixel_bytes);
        encoder.copy_buffer_to_buffer(&self.failures, 0, &self.readback, pixel_bytes, FAILURE_BYTES);
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
            let (pixels, failures) = data.split_at(data.len().saturating_sub(FAILURE_BYTES as usize));
            let word = |i: usize| failures.as_chunks::<4>().0.get(i).map_or(0, |w| u32::from_le_bytes(*w));
            if word(7) != 0 {
                return Err(JobError::new(ErrorCode::Internal, "nonfinite or negative pilot/production path radiance"));
            }
            let failed = u64::from(word(0)) | (u64::from(word(5)) << 32);
            if failed > 0 {
                let pixels = u64::from(self.request.width) * u64::from(self.request.height);
                let paths = self
                    .sampling_report
                    .as_ref()
                    .map_or(pixels * u64::from(self.sample.max(1)) * u64::from(self.options.transmission_samples), |report| {
                        report.production_samples * u64::from(report.transmission_strata) + pixels * u64::from(report.pilot_samples_per_pixel)
                    });
                let detail =
                    format!("{failed} of {paths} paths, first at pixel {}, bounce {}, sample {}, code {}", word(1), word(2), word(3), word(4));
                if !medium_failures_tolerated(failed, paths) {
                    return Err(JobError::new(
                        ErrorCode::InvalidScene,
                        format!(
                            "inconsistent glass medium paths ({detail}): camera must start outside volumes; overlapping solids or more than 16 nested media are unsupported"
                        ),
                    ));
                }
                log::warn!("gpu-path ended {detail} at an unclassifiable glass medium");
            }
            let mut values = scene::reserved((self.request.width * self.request.height) as usize)?;
            for pixel in pixels.as_chunks::<16>().0 {
                let mut value = [0.0; 4];
                for (v, bytes) in value.iter_mut().zip(pixel.as_chunks::<4>().0) {
                    *v = f32::from_le_bytes(*bytes) / self.normalization_samples() as f32;
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
    fn sampling_report(&self) -> Option<solidcraft_render::job::adaptive::SamplingReport> {
        self.sampling_report.clone()
    }
    fn take_linear_exr(&mut self) -> Option<Vec<u8>> {
        self.exr.take()
    }
    fn step(&mut self, units: usize) -> Result<TaskPoll> {
        self.cancel.check()?;
        self.context.ensure_available()?;
        if let Some(png) = &mut self.png {
            return png.step(units);
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
        if self.sample < self.target_samples() {
            self.dispatch()?;
            return Ok(TaskPoll::Pending(Progress { phase: Phase::Running, done: u64::from(self.sample), total: u64::from(self.target_samples()) }));
        }
        if self.sampling_report.is_none() && (self.options.adaptive.is_some() || self.options.transmission_samples != 1) {
            self.sampling_report = Some(adaptive::report(self)?);
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
        if self.exr_work.is_none() {
            log::info!("gpu-path {} spp, {:.3} ms/spp", self.sample, self.started.elapsed().as_secs_f64() * 1000.0 / f64::from(self.sample));
        }
        let mut linear = if let Some(mut work) = self.exr_work.take() {
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
        if let Some(denoiser) = &self.denoiser {
            // Diagnose the raw path buffer before filtering: never hide invalid media
            // or nonfinite radiance by replacing a pixel with neighbouring values.
            drop(linear);
            denoiser.run(self)?;
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
}
impl Task {
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
