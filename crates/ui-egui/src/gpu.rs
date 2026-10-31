//! The GPU viewport in egui: the viewport's pipelines ([`solidcraft_gpu::view`]) drawn in an
//! `egui_wgpu` paint callback. Everything that draws lives in the GPU crate, so headless pictures
//! (`solidcraft-cli snapshot`, MCP screenshots, `ui.render`) use the same pipelines; this is the
//! glue that hands them egui's device and render pass.

#[cfg(all(test, not(target_arch = "wasm32")))]
use std::sync::{Arc, Mutex};

use egui_wgpu::wgpu;
pub use solidcraft_gpu::view::{
    AccessMap, BlockBuilder, BlockBytes, BlockTris, DrawStats, DrawStatsHandle, Frame, GpuImage, GpuScene, GpuTarget, INSTANCE_SIZE, InstanceLook,
    InstancedMesh, LINE_SIZE, LOD_PX, MAX_PALETTE, PBR_SIZE, Palette, PaletteIndex, PlannedBody, Renderer, SHARED_SIZE, SceneBlock, SceneSlot,
    SharedTris, TRI_SIZE, TriClass, UploadError, extend_runs, level_for, lifted_width, placement_scale, runs_within, supersampled_size,
};

/// A frame of the viewport as an egui paint callback.
pub struct ViewportCallback(pub Frame);

/// The viewport callback composites transparently onto this egui GPU mesh. Both its clear
/// surface and gradient therefore use the same published/animated tokens as the panels; no
/// background colour belongs to a cached model scene or GPU upload.
pub(crate) fn paint_background(painter: &egui::Painter, rect: egui::Rect, tokens: &crate::theme::Tokens) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), tokens.viewport_top);
    mesh.colored_vertex(rect.right_top(), tokens.viewport_top);
    mesh.colored_vertex(rect.right_bottom(), tokens.viewport_bottom);
    mesh.colored_vertex(rect.left_bottom(), tokens.viewport_bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// The GPU plan evaluation compiling beside the window: there once it is ready ([`tick_tessellation`]).
pub type TessReady = std::sync::Arc<std::sync::OnceLock<solidcraft_gpu::TessGpu>>;

/// Create the pipelines. Call once with the app's render state, the scene target's depth/stencil/
/// MSAA settings, and the target of the pass the picture is shown in. Without a stencil the
/// section cap does not hide the inside of cut bodies. Without `gpu_meshing` (safe mode) bodies
/// are meshed on the CPU. When the two targets differ, the scene is drawn offscreen and blitted.
/// Also returns the handle to what each frame drew, and the GPU plan evaluation still compiling
/// (native: the target says `tess: false` until it is ready).
pub fn install(
    rs: &egui_wgpu::RenderState,
    scene: GpuTarget,
    window: GpuTarget,
    gpu_meshing: bool,
) -> (GpuTarget, DrawStatsHandle, Option<TessReady>) {
    log_uncaptured_errors(&rs.device);
    // The evaluation shader compiles beside the window, which does not wait for it: on D3D12 it
    // takes seconds, most of the start (#4860). Bodies are built on the CPU until it is ready.
    #[cfg(not(target_arch = "wasm32"))]
    let later = gpu_meshing.then(|| solidcraft_gpu::view::TessLater::start(&rs.adapter, &rs.device, &rs.queue, solidcraft_gpu::SHADER));
    #[cfg(target_arch = "wasm32")]
    let tess = if gpu_meshing { solidcraft_gpu::view::tessellation(&rs.adapter, &rs.device, &rs.queue, solidcraft_gpu::SHADER) } else { None };
    #[cfg(not(target_arch = "wasm32"))]
    let tess = None;
    let (scene, window) = (GpuTarget { tess: tess.is_some(), ..scene }, GpuTarget { tess: tess.is_some(), ..window });
    #[allow(unused_mut)] // Only native hosts hand the renderer a later evaluation.
    let mut res = Renderer::with_window_target(&rs.device, scene, window, tess);
    #[cfg(not(target_arch = "wasm32"))]
    let ready = later.map(|later| {
        let ready = later.ready();
        res.tessellate_later(later);
        ready
    });
    #[cfg(target_arch = "wasm32")]
    let ready = None;
    // What the renderer can do (order-independent transparency), not only what was asked for.
    let scene = res.target();
    let stats = res.stats();
    rs.renderer.write().callback_resources.insert(res);
    (scene, stats, ready)
}

/// Draw bodies from their plans on the GPU from the frame the evaluation shader has compiled
/// ([`install`]); until then, and where it never does, they are built on the CPU.
pub(crate) fn tick_tessellation(app: &mut crate::SolveApp) {
    if app.viewport.tess_ready.as_ref().is_some_and(|ready| ready.get().is_some()) {
        app.viewport.tess_ready = None;
        if let Some(gpu) = app.viewport.gpu.as_mut() {
            gpu.tess = true;
            #[cfg(not(target_arch = "wasm32"))]
            eprintln!("SolidCraft graphics: GPU tessellation ready; bodies are evaluated on the GPU from now on");
        }
    }
}

/// A GPU error no error scope caught is reported, never a panic: a device's default handler
/// panics with "wgpu error: ...", which would close the app with the open design (#5486 reported
/// "wgpu error: Out of Memory" from such a panic). The headless picture devices do the same
/// (`Capturer::headless`). Errors the code expects are caught under error scopes where they
/// happen, and those take priority over this handler. Errors before [`install`] (egui's own
/// pipelines at startup) still reach the default handler, so a startup failure stays a crash
/// report with its startup step.
pub fn log_uncaptured_errors(device: &wgpu::Device) {
    use std::sync::atomic::{AtomicU32, Ordering};
    let reported = std::sync::Arc::new(AtomicU32::new(0));
    device.on_uncaptured_error(std::sync::Arc::new(move |e: wgpu::Error| {
        // The first few only: an invalid resource repeats its error on every frame.
        if reported.fetch_add(1, Ordering::Relaxed) < 8 {
            log::error!("GPU: {e}");
            #[cfg(not(target_arch = "wasm32"))]
            eprintln!("SolidCraft graphics: {e}");
        }
    }));
}

/// Draw this process's headless pictures (`ui.render`, the agent's view image, thumbnails of
/// designs and saves) on the app's own device, with the viewport's pipelines (native only: a
/// browser cannot wait for the GPU to hand a picture back); where the device cannot, the CPU
/// rasterizer draws them.
#[cfg(not(target_arch = "wasm32"))]
pub fn install_picture_renderer(rs: &egui_wgpu::RenderState) {
    match solidcraft_gpu::capture::Capturer::new(&rs.adapter, rs.device.clone(), rs.queue.clone()) {
        Ok(capturer) => install_capturer(capturer.watching(rs.device_lost.flag())),
        Err(e) => {
            log::warn!("pictures of the model are drawn by the CPU rasterizer: the GPU cannot draw them: {e}");
            install_cpu_picture_renderer();
        }
    }
}

/// Draw this process's headless pictures with `capturer`; once its device is lost (the graphics
/// driver was reset, #4172) the CPU rasterizer draws them, so saving still writes a thumbnail.
#[cfg(not(target_arch = "wasm32"))]
fn install_capturer(capturer: solidcraft_gpu::capture::Capturer) {
    let capturer = std::sync::Arc::new(capturer);
    solidcraft_engine::capture::install(std::sync::Arc::new(move |scene, cam, w, h| picture(&capturer, scene, cam, w, h)));
}

/// `scene` drawn by `capturer`, or by the CPU rasterizer once its device is lost.
#[cfg(not(target_arch = "wasm32"))]
fn picture(
    capturer: &solidcraft_gpu::capture::Capturer,
    scene: &solidcraft_engine::render::Scene,
    cam: &solidcraft_engine::render::Camera,
    w: u32,
    h: u32,
) -> Result<solidcraft_engine::capture::Picture, String> {
    match capturer.rgba(scene, cam, w, h) {
        Ok(rgba) => Ok(solidcraft_engine::capture::Picture { rgba, renderer: capturer.renderer() }),
        Err(solidcraft_gpu::GpuError::DeviceLost) => Ok(cpu_picture(scene, cam, w, h)),
        Err(e) => Err(e.to_string()),
    }
}

/// The last tier: draw this process's headless pictures with the CPU rasterizer, and every
/// picture says so. The app uses it when its device cannot draw pictures; the web app always
/// does, since a browser cannot wait for the GPU to hand a picture back.
pub fn install_cpu_picture_renderer() {
    solidcraft_engine::capture::install(std::sync::Arc::new(|scene, cam, w, h| Ok(cpu_picture(scene, cam, w, h))));
}

fn cpu_picture(
    scene: &solidcraft_engine::render::Scene,
    cam: &solidcraft_engine::render::Camera,
    w: u32,
    h: u32,
) -> solidcraft_engine::capture::Picture {
    let canvas = solidcraft_engine::render::render(scene, cam, w as usize, h as usize);
    solidcraft_engine::capture::Picture { rgba: canvas.rgba, renderer: "cpu" }
}

#[cfg(test)]
mod test_support {
    /// For tests that look at pictures: install the headless GPU renderer (the CLI's). `false`, with
    /// the reason printed, on a machine without a GPU adapter, and the test skips.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn test_picture_renderer() -> bool {
        match solidcraft_gpu::capture::shared() {
            Ok(c) => {
                solidcraft_engine::capture::install(std::sync::Arc::new(move |scene, cam, w, h| {
                    c.rgba(scene, cam, w, h)
                        .map(|rgba| solidcraft_engine::capture::Picture { rgba, renderer: c.renderer() })
                        .map_err(|e| e.to_string())
                }));
                true
            }
            Err(e @ solidcraft_gpu::GpuError::NoAdapter(_)) if std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_none() => {
                eprintln!("skipping: {e}");
                false
            }
            Err(e) => panic!("{e}"),
        }
    }
}
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) use test_support::test_picture_renderer;

impl egui_wgpu::CallbackTrait for ViewportCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(res) = resources.get_mut::<Renderer>() {
            res.prepare(device, queue, encoder, &self.0);
        }
        Vec::new()
    }

    fn paint(&self, info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, resources: &egui_wgpu::CallbackResources) {
        let Some(res) = resources.get::<Renderer>() else { return };
        let vp = info.viewport_in_pixels();
        res.paint(&self.0, pass, [vp.left_px as f32, vp.top_px as f32, vp.width_px as f32, vp.height_px as f32]);
    }
}

/// The shader's analysis colours and flags for the theme's colours.
pub fn analysis_colors(c: &crate::theme::AnalysisColors) -> [f32; 36] {
    solidcraft_gpu::view::analysis_colors(c)
}

/// The backends the offscreen tests start, chosen once per test process as the app chooses them
/// (`apps/solidcraft/src/startup.rs`, `choose_backends`): what `WGPU_BACKEND` names; on Windows one
/// backend, the first of D3D12, Vulkan and OpenGL with a hardware adapter (else D3D12); elsewhere
/// wgpu's default. wgpu's default on Windows starts Vulkan, D3D12 and OpenGL together, and tests
/// starting them side by side hung or crashed the test process inside the Vulkan driver.
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn offscreen_backends() -> wgpu::Backends {
    static CHOSEN: std::sync::OnceLock<wgpu::Backends> = std::sync::OnceLock::new();
    *CHOSEN.get_or_init(|| {
        let default = solidcraft_gpu::instance_descriptor_from_env().backends;
        if !cfg!(target_os = "windows") || std::env::var("WGPU_BACKEND").is_ok_and(|v| !v.trim().is_empty()) {
            return default;
        }
        let hardware = |backends: wgpu::Backends| {
            let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
            desc.backends = backends;
            desc.backend_options.dx12.shader_compiler = solidcraft_gpu::dx12_compiler();
            let adapters = pollster::block_on(wgpu::Instance::new(desc).enumerate_adapters(backends));
            adapters.iter().any(|a| a.get_info().device_type != wgpu::DeviceType::Cpu)
        };
        [wgpu::Backends::DX12, wgpu::Backends::VULKAN, wgpu::Backends::GL].into_iter().find(|b| hardware(*b)).unwrap_or(wgpu::Backends::DX12)
    })
}

/// Held while a test starts an instance, adapter or device: starting an instance scans the Vulkan
/// drivers, and tests doing that side by side crashed the test process inside the Vulkan loader
/// (`loader_icd_scan`). Rendering on the devices still runs side by side.
#[cfg(all(test, not(target_arch = "wasm32")))]
fn offscreen_starting() -> std::sync::MutexGuard<'static, ()> {
    static STARTING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    STARTING.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The adapter an offscreen test opens under `policy` (`SOLIDCRAFT_GPU`, #9040), on
/// [`offscreen_backends`]; call it holding [`offscreen_starting`]. Under `auto` (the variable
/// unset) wgpu's default adapter, as before; under `hardware` or `software` the one
/// `solidcraft_gpu::select` picks for headless pictures of that tier, so `software` keeps the
/// tests off the hardware GPU; under `cpu`, as for pictures, none. `None` when none qualifies.
#[cfg(all(test, not(target_arch = "wasm32")))]
fn offscreen_adapter(policy: solidcraft_gpu::select::Policy) -> Option<wgpu::Adapter> {
    use solidcraft_gpu::select::Policy;
    let mut desc = solidcraft_gpu::instance_descriptor_from_env();
    desc.backends = offscreen_backends();
    let instance = wgpu::Instance::new(desc);
    match policy {
        Policy::Auto => pollster::block_on(instance.request_adapter(&Default::default())).ok(),
        Policy::Hardware | Policy::Software => {
            let found = pollster::block_on(instance.enumerate_adapters(offscreen_backends()));
            solidcraft_gpu::select::pick_among(found, policy).map(|(adapter, _)| adapter)
        }
        Policy::Cpu => None,
    }
}

/// A device of its own for an offscreen render or check (`None` without a GPU adapter the
/// `SOLIDCRAFT_GPU` policy admits, or when the variable names no policy), on [`offscreen_backends`]. One test at a time starts its
/// instance, adapter and device ([`offscreen_starting`]).
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn offscreen_device() -> Option<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    let policy = solidcraft_gpu::select::Policy::from_env().map_err(|e| eprintln!("no offscreen device: {e}")).ok()?;
    let _one_at_a_time = offscreen_starting();
    let adapter = offscreen_adapter(policy)?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: solidcraft_gpu::depth_features(&adapter),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        ..Default::default()
    }))
    .ok()?;
    Some((adapter, device, queue))
}

/// Whether [`install`] would evaluate plans on the GPU with `shader`, on an offscreen device
/// (`None` without a GPU adapter).
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn offscreen_tessellates(shader: &str) -> Option<bool> {
    let (adapter, device, queue) = offscreen_device()?;
    Some(solidcraft_gpu::view::tessellation(&adapter, &device, &queue, shader).is_some())
}

/// Render `scene` offscreen on a device of its own, through the viewport's real resources and
/// frame callback (`prepare`, then `paint`), and read the pixels back: straight RGBA, `size` ×
/// `size`. `None` without a GPU adapter. With `tess` (the tessellation shader), planned bodies
/// are evaluated on the GPU where the device builds that pipeline, as [`install`] sets it up.
/// `highlight` is drawn as the viewport's highlight scene (hover and selection). `analysis` is the
/// surface analysis uniform (`viewport::analysis_uniform`; zeros for none) and `colors` its
/// colours (`analysis_colors`).
#[cfg(all(test, not(target_arch = "wasm32")))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_offscreen(
    scene: GpuScene,
    highlight: Option<GpuScene>,
    view_proj: [[f32; 4]; 4],
    back: [f32; 3],
    eye: [f32; 4],
    size: u32,
    tess: Option<&str>,
    analysis: [f32; 8],
    colors: [f32; 36],
) -> Option<Vec<u8>> {
    offscreen(
        scene,
        highlight,
        view_proj,
        back,
        eye,
        size,
        Offscreen {
            tess,
            product: false,
            srgb: false,
            analysis,
            analysis_colors: colors,
            ssaa: 1.0,
            ao_radius: 0.0,
            ao_samples: solidcraft_gpu::view::AO_SAMPLES,
        },
    )
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn render_product_offscreen(
    scene: GpuScene,
    highlight: Option<GpuScene>,
    view_proj: [[f32; 4]; 4],
    back: [f32; 3],
    eye: [f32; 4],
    size: u32,
    srgb: bool,
) -> Option<Vec<u8>> {
    offscreen(
        scene,
        highlight,
        view_proj,
        back,
        eye,
        size,
        Offscreen {
            tess: None,
            product: true,
            srgb,
            analysis: [0.0; 8],
            analysis_colors: [0.0; 36],
            ssaa: 1.0,
            ao_radius: 0.0,
            ao_samples: solidcraft_gpu::view::AO_SAMPLES,
        },
    )
}

#[cfg(all(test, not(target_arch = "wasm32")))]
struct Offscreen<'a> {
    tess: Option<&'a str>,
    product: bool,
    srgb: bool,
    analysis: [f32; 8],
    analysis_colors: [f32; 36],
    ssaa: f32,
    ao_radius: f32,
    ao_samples: u32,
}

/// [`render_offscreen`] of a scene supersampled `ssaa` times (1: off).
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn render_offscreen_ssaa(
    scene: GpuScene,
    view_proj: [[f32; 4]; 4],
    back: [f32; 3],
    eye: [f32; 4],
    size: u32,
    ssaa: f32,
) -> Option<Vec<u8>> {
    offscreen(
        scene,
        None,
        view_proj,
        back,
        eye,
        size,
        Offscreen {
            tess: None,
            product: false,
            srgb: false,
            analysis: [0.0; 8],
            analysis_colors: [0.0; 36],
            ssaa,
            ao_radius: 0.0,
            ao_samples: solidcraft_gpu::view::AO_SAMPLES,
        },
    )
}

/// [`render_offscreen`] of a scene with ambient occlusion of `ao_radius` (world units; 0: off),
/// `ao_samples` samples per pixel.
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn render_offscreen_ao(
    scene: GpuScene,
    view_proj: [[f32; 4]; 4],
    back: [f32; 3],
    eye: [f32; 4],
    size: u32,
    ao_radius: f32,
    ao_samples: u32,
) -> Option<Vec<u8>> {
    offscreen(
        scene,
        None,
        view_proj,
        back,
        eye,
        size,
        Offscreen { tess: None, product: false, srgb: false, analysis: [0.0; 8], analysis_colors: [0.0; 36], ssaa: 1.0, ao_radius, ao_samples },
    )
}

#[cfg(all(test, not(target_arch = "wasm32")))]
fn offscreen(
    scene: GpuScene,
    highlight: Option<GpuScene>,
    view_proj: [[f32; 4]; 4],
    back: [f32; 3],
    eye: [f32; 4],
    size: u32,
    options: Offscreen<'_>,
) -> Option<Vec<u8>> {
    let Offscreen { tess, product, srgb, analysis, analysis_colors, ssaa, ao_radius, ao_samples } = options;
    let (adapter, device, queue) = offscreen_device()?;
    eprintln!("offscreen adapter: {:?}", adapter.get_info().backend);
    let format = if srgb { wgpu::TextureFormat::Rgba8UnormSrgb } else { wgpu::TextureFormat::Rgba8Unorm };
    let depth = solidcraft_gpu::depth_format(&device, true);
    let compute = tess.and_then(|shader| solidcraft_gpu::view::tessellation(&adapter, &device, &queue, shader));
    let target = GpuTarget { format, depth: Some(depth), samples: 1, tess: compute.is_some(), oit: false };
    let mut resources = egui_wgpu::CallbackResources::default();
    resources.insert(Renderer::new(&device, target, compute));
    let none = || Arc::new(Mutex::new(None));
    let cb = ViewportCallback(Frame {
        product_lights: None,
        product,
        upload_error: None,
        key: 1,
        slot: Arc::new(Mutex::new(Some(scene))),
        sketch_key: 0,
        sketch_slot: none(),
        hl_key: u64::from(highlight.is_some()),
        hl_slot: Arc::new(Mutex::new(highlight)),
        pv_key: 0,
        pv_slot: none(),
        pv_hide: false,
        view_proj,
        back,
        size_px: [size as f32; 2],
        clip: [0.0; 4],
        cap: [0.0; 4],
        images: Vec::new(),
        clouds: Vec::new(),
        analysis,
        analysis_colors,
        access: None,
        outline: false,
        eye,
        ao_radius,
        ao_samples,
        ssaa,
        hover_tint: [0.0; 3],
        selection_tint: [0.0; 3],
    });
    let texture = |format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let color = texture(format, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
    let depth_tex = texture(depth, wgpu::TextureUsages::RENDER_ATTACHMENT);
    let (color_view, depth_view) = (color.create_view(&Default::default()), depth_tex.create_view(&Default::default()));
    let mut encoder = device.create_command_encoder(&Default::default());
    let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [size, size], pixels_per_point: 1.0 };
    let extra = egui_wgpu::CallbackTrait::prepare(&cb, &device, &queue, &screen, &mut encoder, &mut resources);
    {
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("offscreen"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Store }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
            .forget_lifetime();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size as f32, size as f32));
        let info = egui::PaintCallbackInfo { viewport: rect, clip_rect: rect, pixels_per_point: 1.0, screen_size_px: [size, size] };
        egui_wgpu::CallbackTrait::paint(&cb, info, &mut pass, &resources);
    }
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("offscreen read"),
        size: u64::from(size * size * 4),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        color.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &read,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size * 4), rows_per_image: Some(size) },
        },
        wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
    );
    queue.submit(extra.into_iter().chain([encoder.finish()]));
    let slice = read.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
    let bytes = slice.get_mapped_range().ok()?.to_vec();
    Some(bytes)
}

/// A viewport renderer kept on an offscreen device of its own, for timing frames (prepare, paint,
/// and the GPU finishing them) as the window would draw them. Scale measurements only.
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) struct OffscreenFrames {
    device: wgpu::Device,
    queue: wgpu::Queue,
    resources: egui_wgpu::CallbackResources,
    color: wgpu::TextureView,
    depth: wgpu::TextureView,
    size: u32,
}

#[cfg(all(test, not(target_arch = "wasm32")))]
impl OffscreenFrames {
    /// `None` without a GPU adapter. With `tess`, planned bodies are evaluated on the GPU.
    pub(crate) fn new(size: u32, tess: bool) -> Option<OffscreenFrames> {
        let (adapter, device, queue) = offscreen_device()?;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let depth = solidcraft_gpu::depth_format(&device, true);
        let compute = if tess { solidcraft_gpu::view::tessellation(&adapter, &device, &queue, solidcraft_gpu::SHADER) } else { None };
        let target = GpuTarget { format, depth: Some(depth), samples: 1, tess: compute.is_some(), oit: false };
        let mut resources = egui_wgpu::CallbackResources::default();
        resources.insert(Renderer::new(&device, target, compute));
        let texture = |format, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("offscreen frames"),
                    size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let color = texture(format, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let depth = texture(depth, wgpu::TextureUsages::RENDER_ATTACHMENT);
        Some(OffscreenFrames { device, queue, resources, color, depth, size })
    }

    /// What the renderer can do, as the viewport is told when the window's renderer is
    /// installed (plans evaluated, order-independent transparency).
    pub(crate) fn target(&self) -> Option<GpuTarget> {
        self.resources.get::<Renderer>().map(Renderer::target)
    }

    /// Whether the device evaluates plans (so scenes should be built with `tess`).
    pub(crate) fn tessellates(&self) -> bool {
        self.resources.get::<Renderer>().is_some_and(|r| r.target().tess)
    }

    /// Draw one frame and wait for the GPU: milliseconds from `prepare` to the work finishing.
    pub(crate) fn frame(&mut self, frame: Frame) -> f64 {
        let started = std::time::Instant::now();
        let cb = ViewportCallback(frame);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [self.size, self.size], pixels_per_point: 1.0 };
        let extra = egui_wgpu::CallbackTrait::prepare(&cb, &self.device, &self.queue, &screen, &mut encoder, &mut self.resources);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("offscreen frames"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.color,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.depth,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                        stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Store }),
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(self.size as f32, self.size as f32));
            let info = egui::PaintCallbackInfo { viewport: rect, clip_rect: rect, pixels_per_point: 1.0, screen_size_px: [self.size, self.size] };
            egui_wgpu::CallbackTrait::paint(&cb, info, &mut pass, &self.resources);
        }
        self.queue.submit(extra.into_iter().chain([encoder.finish()]));
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        started.elapsed().as_secs_f64() * 1000.0
    }

    /// What the model scene's last change uploaded.
    pub(crate) fn uploads(&self) -> solidcraft_gpu::view::UploadStats {
        self.resources.get::<Renderer>().map(Renderer::uploads).unwrap_or_default()
    }

    /// What the last frame drew.
    pub(crate) fn stats(&self) -> DrawStats {
        self.resources.get::<Renderer>().and_then(|r| r.stats().lock().ok().map(|s| *s)).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    /// Bodies are drawn from their plans on the GPU from the frame the evaluation shader has
    /// compiled, and built on the CPU until then (#4860).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn plans_are_drawn_on_the_gpu_once_the_evaluation_has_compiled() {
        let Ok(tess) = solidcraft_gpu::TessGpu::headless() else {
            eprintln!("skipped: no GPU adapter that builds the evaluation");
            return;
        };
        let mut app = crate::SolveApp::new(Default::default(), Default::default());
        app.viewport.gpu =
            Some(super::GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false });
        let ready = super::TessReady::default();
        app.viewport.tess_ready = Some(ready.clone());
        super::tick_tessellation(&mut app);
        assert!(app.viewport.gpu.is_some_and(|g| !g.tess), "still compiling: built on the CPU");
        assert!(ready.set(tess).is_ok());
        super::tick_tessellation(&mut app);
        assert!(app.viewport.gpu.is_some_and(|g| g.tess), "compiled: drawn from plans");
        assert!(app.viewport.tess_ready.is_none());
    }

    /// The offscreen tests start the backends the app would: on Windows without `WGPU_BACKEND`
    /// exactly one, from the app's order (D3D12, Vulkan, OpenGL); otherwise wgpu's default.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn offscreen_tests_start_the_backend_the_app_would() {
        use egui_wgpu::wgpu::Backends;
        let chosen = super::offscreen_backends();
        let env = std::env::var("WGPU_BACKEND").is_ok_and(|v| !v.trim().is_empty());
        if cfg!(target_os = "windows") && !env {
            assert!([Backends::DX12, Backends::VULKAN, Backends::GL].contains(&chosen), "one backend, not {chosen:?}");
        } else {
            assert_eq!(chosen, egui_wgpu::wgpu::InstanceDescriptor::new_without_display_handle_from_env().backends);
        }
        assert_eq!(super::offscreen_backends(), chosen, "chosen once per test process");
        // And the offscreen device is made on them (skipped without a GPU adapter).
        if let Some((adapter, _, _)) = super::offscreen_device() {
            let used = Backends::from(adapter.get_info().backend);
            assert!(chosen.contains(used), "the offscreen device runs on {used:?}, not on {chosen:?}");
        }
    }
    /// `SOLIDCRAFT_GPU` picks the offscreen tests' adapter as it picks headless pictures' (#9040):
    /// `software` never a hardware GPU (it did, and the tests crashed one), `hardware` never a
    /// software rasterizer, `cpu` none. A tier without an adapter here is skipped. Only adapters
    /// are picked: no device is opened.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn offscreen_tests_pick_their_adapter_by_the_gpu_policy() {
        use solidcraft_gpu::select::Policy;
        let _one_at_a_time = super::offscreen_starting();
        assert!(super::offscreen_adapter(Policy::Cpu).is_none(), "cpu opens no adapter");
        for (policy, software) in [(Policy::Software, true), (Policy::Hardware, false)] {
            match super::offscreen_adapter(policy) {
                Some(adapter) => {
                    let info = adapter.get_info();
                    assert_eq!(
                        info.device_type == egui_wgpu::wgpu::DeviceType::Cpu,
                        software,
                        "{policy:?} picked {} ({:?})",
                        info.name,
                        info.device_type
                    );
                }
                None => eprintln!("skipping {policy:?}: no adapter of that tier"),
            }
        }
    }

    /// A device the driver loses (#4172): its lost callback marks it, the painter's probe sees it
    /// before the next frame's uploads, those uploads no longer reach the device (they panicked
    /// inside wgpu when they were the first to meet the loss), and pictures (thumbnails of saves)
    /// come from the CPU rasterizer. `destroy` loses the device as a driver reset does; wgpu calls
    /// the lost callback of a destroyed device once it has polled.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_lost_device_stops_drawing_and_pictures_fall_back_to_the_cpu() {
        use egui_wgpu::wgpu;
        let Some((adapter, device, queue)) = super::offscreen_device() else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let lost = egui_wgpu::DeviceLost::watch(&device);
        let capturer = solidcraft_gpu::capture::Capturer::new(&adapter, device.clone(), queue.clone()).unwrap().watching(lost.flag());
        let (scene, cam) = (solidcraft_engine::render::Scene::default(), solidcraft_engine::render::Camera::default());
        assert_ne!(super::picture(&capturer, &scene, &cam, 16, 16).unwrap().renderer, "cpu");
        assert!(!lost.probe(&device, &queue), "a working device isn't lost");

        // egui's renderer is made with the window, before any loss (made on a lost device, its
        // uniform buffer's `create_buffer_init` panics).
        let mut renderer = egui_wgpu::Renderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, Default::default());

        device.destroy();
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        assert!(lost.is_lost() && lost.probe(&device, &queue), "the lost callback marked the device");
        assert!(lost.reason().is_some_and(|r| r.starts_with("Destroyed")), "{:?}", lost.reason());

        // The next frame's uploads, as the painter would make them were it not skipping: wgpu
        // knows the device is lost, so they fail quietly instead of panicking.
        let image = egui::ImageData::Color(std::sync::Arc::new(egui::ColorImage::filled([4, 4], egui::Color32::WHITE)));
        let delta = egui::epaint::ImageDelta::full(image, egui::TextureOptions::LINEAR);
        renderer.update_texture(&device, &queue, egui::TextureId::Managed(0), &delta);

        // A save's thumbnail still comes: from the CPU rasterizer.
        let picture = super::picture(&capturer, &scene, &cam, 16, 16).unwrap();
        assert_eq!((picture.renderer, picture.rgba.len()), ("cpu", 16 * 16 * 4));
    }

    /// The probe runs before every frame's uploads, so on a working device it must cost nothing
    /// visible: no error for any scope or handler, and the uploads after it go through. On a
    /// reset D3D12 device an empty submission went through and the upload after it panicked
    /// inside wgpu (#6528); the probe now opens a command list, the driver call that upload's
    /// queue write would make first. A driver reset can't be made here: `destroy` loses the
    /// device in wgpu itself, before any driver call.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_lost_device_probe_is_clean_on_a_working_device() {
        use egui_wgpu::wgpu;
        let Some((adapter, device, queue)) = super::offscreen_device() else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let lost = egui_wgpu::DeviceLost::watch(&device);
        let capturer = solidcraft_gpu::capture::Capturer::new(&adapter, device.clone(), queue.clone()).unwrap().watching(lost.flag());
        let mut renderer = egui_wgpu::Renderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, Default::default());
        let image = egui::ImageData::Color(std::sync::Arc::new(egui::ColorImage::filled([4, 4], egui::Color32::WHITE)));
        let delta = egui::epaint::ImageDelta::full(image, egui::TextureOptions::LINEAR);
        let (scene, cam) = (solidcraft_engine::render::Scene::default(), solidcraft_engine::render::Camera::default());
        for frame in 0..3 {
            // As the painter: the probe, then the frame's first queue write.
            let (was_lost, error) = egui_wgpu::capture_errors(&device, || {
                let was_lost = lost.probe(&device, &queue);
                renderer.update_texture(&device, &queue, egui::TextureId::Managed(0), &delta);
                was_lost
            });
            assert!(!was_lost && error.is_none(), "frame {frame}: lost {was_lost}, {error:?}");
            // A save's thumbnail, which probes too, still comes from the GPU.
            assert_ne!(super::picture(&capturer, &scene, &cam, 16, 16).unwrap().renderer, "cpu");
        }
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        assert!(!lost.is_lost() && lost.reason().is_none(), "{:?}", lost.reason());
    }
}
