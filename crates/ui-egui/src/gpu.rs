//! The GPU viewport in egui: the viewport's pipelines ([`solidcraft_viewport_gpu::view`]) drawn in an
//! `egui_wgpu` paint callback. Everything that draws lives in the GPU crate, so headless pictures
//! (`solidcraft-cli snapshot`, MCP screenshots, `ui.render`) use the same pipelines; this is the
//! glue that hands them egui's device and render pass.

#[cfg(all(test, not(target_arch = "wasm32")))]
use std::sync::{Arc, Mutex};

use egui_wgpu::wgpu;
pub use solidcraft_viewport_gpu::view::{
    AccessMap, BlockBuilder, BlockBytes, BlockTris, DrawStats, DrawStatsHandle, Frame, GpuImage, GpuScene, GpuTarget, INSTANCE_SIZE, InstanceLook,
    InstancedMesh, LINE_SIZE, LOD_PX, LitEdges, MAX_MOTIONS, MAX_PALETTE, Material, MotionRows, Motions, MovingLayer, PBR_SIZE, Palette,
    PaletteIndex, PlannedBody, Renderer, SHARED_SIZE, SceneBlock, SceneSlot, SharedEdges, SharedTris, TRI_SIZE, TriClass, UNMOVED_SLOT, UploadError,
    extend_runs, level_for, lifted_width, lit_line_style, placement_scale, runs_within, supersampled_size,
};

/// The part of a picture (as texture coordinates) that `rect` shows in a window `window`: the
/// picture covers the window from its centre, so a narrower window keeps the middle of it, and
/// panels and the view's own size don't move it.
fn background_uv(image: egui::Vec2, rect: egui::Rect, window: egui::Rect) -> egui::Rect {
    let scale = (window.width() / image.x).max(window.height() / image.y).max(f32::EPSILON);
    let uv = |p: egui::Pos2| ((p - window.center()) / scale + image / 2.0) / image;
    egui::Rect::from_min_max(uv(rect.min).to_pos2(), uv(rect.max).to_pos2())
}

/// A picture's pixels, at most `limit` on a side (what the GPU takes).
fn decode_background(bytes: &[u8], limit: u32) -> Result<egui::ColorImage, String> {
    // Upright, as every viewer shows it: a photo stored turned by its EXIF orientation (#19138).
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?;
    let (mut image, _) = solidcraft_engine::upright_image::decode_upright(reader).map_err(|e| e.to_string())?;
    if image.width().max(image.height()) > limit {
        image = image.resize(limit, limit, image::imageops::FilterType::Triangle);
    }
    let rgba = image.into_rgba8();
    Ok(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

#[derive(Clone)]
struct BackgroundPicture {
    path: String,
    decoded: std::sync::Arc<std::sync::Mutex<Option<Result<egui::ColorImage, String>>>>,
    texture: Option<egui::TextureHandle>,
}

/// The background picture over the view's background (the Background picture preference), read
/// off the window thread. Until it is ready, and if it can't be read, the view's background stays.
pub(crate) fn paint_background_picture(app: &mut crate::SolveApp, painter: &egui::Painter, rect: egui::Rect) {
    let path = &app.preferences.background_image;
    if path.is_empty() {
        return;
    }
    let ctx = painter.ctx().clone();
    let id = egui::Id::unique("sc_background_picture");
    let mut picture = ctx.data(|d| d.get_temp::<BackgroundPicture>(id)).filter(|p| p.path == *path).unwrap_or_else(|| {
        let picture = BackgroundPicture { path: path.clone(), decoded: Default::default(), texture: None };
        let (path, slot, repaint) = (path.clone(), picture.decoded.clone(), ctx.clone());
        let limit = ctx.input(|i| i.max_texture_side) as u32;
        std::thread::spawn(move || {
            let result = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| decode_background(&bytes, limit));
            *slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(result);
            repaint.request_repaint();
        });
        picture
    });
    let done = picture.decoded.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
    match done {
        Some(Ok(image)) => {
            let options = egui::TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..egui::TextureOptions::LINEAR };
            picture.texture = Some(ctx.load_texture("background-picture", image, options));
        }
        Some(Err(error)) => app.set_status(format!("{}: {error}", solidcraft_ui_text::tr("prefs.background_unreadable")), true),
        None => {}
    }
    if let Some(texture) = &picture.texture {
        painter.image(texture.id(), rect, background_uv(texture.size_vec2(), rect, ctx.content_rect()), egui::Color32::WHITE);
    }
    ctx.data_mut(|d| d.insert_temp(id, picture));
}

/// Draw bodies from their plans on the GPU from the frame the evaluation shader has compiled
/// ([`solidcraft_viewport_egui::install`]); until then, and where it never does, they are built on the CPU.
pub(crate) fn tick_tessellation(app: &mut crate::SolveApp) {
    if app.viewport.tess_ready.as_ref().is_some_and(|ready| ready.get().is_some()) {
        app.viewport.tess_ready = None;
        if let Some(gpu) = app.viewport.gpu.as_mut() {
            gpu.tess = true;
            #[cfg(not(target_arch = "wasm32"))]
            solidcraft_engine::note!("SolidCraft graphics: GPU tessellation ready; bodies are evaluated on the GPU from now on");
        }
    }
}

/// SolidCraft's report of a GPU error no error scope caught
/// ([`solidcraft_viewport_egui::log_uncaptured_errors`] logs it too): a line on stderr, which a
/// terminal or crash report shows beside the app's own lines.
pub(crate) fn report_gpu_error(e: &wgpu::Error) {
    #[cfg(not(target_arch = "wasm32"))]
    solidcraft_engine::note!("SolidCraft graphics: {e}");
    #[cfg(target_arch = "wasm32")]
    let _ = e;
}

/// The viewport scene's sample count, for the desktop and the web app alike. Only the offscreen
/// scene is multisampled; the window's own pass is 1x.
/// - `chosen` is the Anti-aliasing preference.
/// - `supported` lists the counts the device draws the scene's colour and depth formats with;
///   empty means unknown, and `chosen` is taken.
///
/// The backend's policy comes first: OpenGL and WebGL draw 1x (`effective_msaa_samples`). A
/// count the device can't draw falls back to the largest supported count up to the default (4x),
/// not to 1x, so an 8x chosen on another device still antialiases here.
pub fn scene_samples(backend: wgpu::Backend, chosen: u32, supported: &[u32]) -> u32 {
    let requested = egui_wgpu::effective_msaa_samples(backend, chosen.max(1));
    if requested <= 1 || supported.is_empty() || supported.contains(&requested) {
        requested
    } else {
        supported.iter().copied().filter(|&n| n <= crate::prefs::Prefs::default().msaa).max().unwrap_or(1)
    }
}

/// The anti-aliasing choices Graphics offers: the counts in `supported` (as for [`scene_samples`])
/// that the backend draws as such. A choice the backend would draw at 1x (OpenGL and WebGL) then
/// shows as unavailable, instead of being offered and doing nothing.
pub fn scene_sample_choices(backend: wgpu::Backend, supported: Vec<u32>) -> Vec<u32> {
    supported.into_iter().filter(|&n| egui_wgpu::effective_msaa_samples(backend, n) == n).collect()
}

/// Draw this process's headless pictures (`ui.render`, the agent's view image, thumbnails of
/// designs and saves) on the app's own device, with the viewport's pipelines (native only: a
/// browser cannot wait for the GPU to hand a picture back); where the device cannot, the CPU
/// rasterizer draws them.
#[cfg(not(target_arch = "wasm32"))]
pub fn install_picture_renderer(rs: &egui_wgpu::RenderState) {
    match solidcraft_viewport_gpu::capture::Capturer::new(&rs.adapter, rs.device.clone(), rs.queue.clone()) {
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
fn install_capturer(capturer: solidcraft_viewport_gpu::capture::Capturer) {
    let capturer = std::sync::Arc::new(capturer);
    solidcraft_engine::capture::install(std::sync::Arc::new(move |scene, cam, w, h| picture(&capturer, scene, cam, w, h)));
}

/// `scene` drawn by `capturer`, or by the CPU rasterizer if the device is lost or the picture is invisible.
#[cfg(not(target_arch = "wasm32"))]
fn picture(
    capturer: &solidcraft_viewport_gpu::capture::Capturer,
    scene: &solidcraft_viewport::Scene,
    cam: &solidcraft_viewport::Camera,
    w: u32,
    h: u32,
) -> Result<solidcraft_engine::capture::Picture, String> {
    picture_result(capturer.rgba(scene, cam, w, h), capturer.renderer(), || cpu_picture(scene, cam, w, h))
}

/// Keep a captured picture or draw its CPU replacement when the device is lost or returns no visible pixels.
#[cfg(not(target_arch = "wasm32"))]
fn picture_result(
    captured: solidcraft_viewport_gpu::Result<Vec<u8>>,
    renderer: &'static str,
    cpu: impl FnOnce() -> solidcraft_engine::capture::Picture,
) -> Result<solidcraft_engine::capture::Picture, String> {
    match captured {
        // Capturer draws an opaque backdrop before the model. An entirely transparent picture
        // cannot be a successful capture, even when the readback reports no error (#15986).
        Ok(rgba) if !rgba.is_empty() && rgba.len().is_multiple_of(4) && rgba.iter().skip(3).step_by(4).all(|&alpha| alpha == 0) => {
            log::warn!("GPU picture was fully transparent; drawing it with the CPU rasterizer");
            Ok(cpu())
        }
        Ok(rgba) => Ok(solidcraft_engine::capture::Picture { rgba, renderer }),
        Err(solidcraft_viewport_gpu::GpuError::DeviceLost) => Ok(cpu()),
        Err(e) => Err(e.to_string()),
    }
}

/// The last tier: draw this process's headless pictures with the CPU rasterizer, and every
/// picture says so. The app uses it when its device cannot draw pictures; the web app always
/// does, since a browser cannot wait for the GPU to hand a picture back.
pub fn install_cpu_picture_renderer() {
    solidcraft_engine::capture::install(std::sync::Arc::new(|scene, cam, w, h| Ok(cpu_picture(scene, cam, w, h))));
}

fn cpu_picture(scene: &solidcraft_viewport::Scene, cam: &solidcraft_viewport::Camera, w: u32, h: u32) -> solidcraft_engine::capture::Picture {
    let canvas = solidcraft_viewport::render(scene, cam, w as usize, h as usize);
    solidcraft_engine::capture::Picture { rgba: canvas.rgba, renderer: "cpu" }
}

#[cfg(test)]
mod test_support {
    /// Paint real egui textures and callbacks into a readable window target.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) struct EguiFixture {
        device: super::wgpu::Device,
        queue: super::wgpu::Queue,
        renderer: std::sync::Arc<egui::epaint::mutex::RwLock<egui_wgpu::Renderer>>,
        output: super::wgpu::Texture,
        size: [u32; 2],
        pub(crate) product_highlight_callbacks: usize,
        pub(crate) viewport_callbacks: usize,
    }

    #[cfg(not(target_arch = "wasm32"))]
    impl EguiFixture {
        pub(crate) fn new(app: &mut crate::SolveApp, size: [u32; 2]) -> Option<Self> {
            use super::*;
            let Some((_, device, queue)) = offscreen_device() else {
                assert!(std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_none(), "GPU adapter required for egui fixture");
                eprintln!("skipped egui fixture: no admitted adapter/device");
                return None;
            };
            let format = wgpu::TextureFormat::Rgba8UnormSrgb;
            let renderer = Arc::new(egui::epaint::mutex::RwLock::new(egui_wgpu::Renderer::new(&device, format, Default::default())));
            let window = GpuTarget { format, depth: None, samples: 1, tess: false, oit: false, edges: false };
            let scene = GpuTarget { depth: Some(solidcraft_viewport_gpu::depth_format(&device, true)), ..window };
            let view = Renderer::with_window_target(&device, scene, window, None);
            let installed = view.target();
            app.viewport.gpu = Some(installed);
            app.viewport.draw_stats = Some(view.stats());
            renderer.write().callback_resources.insert(view);
            app.render_workspace.host = Some(
                crate::render_workspace::host_executor(device.limits().max_texture_dimension_2d, installed.depth.is_some(), installed.samples.max(1))
                    .unwrap(),
            );
            crate::render_workspace::sync_backend(app);
            let output = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("full egui fixture"),
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            Some(Self { device, queue, renderer, output, size, product_highlight_callbacks: 0, viewport_callbacks: 0 })
        }

        pub(crate) fn frame(&mut self, ctx: &egui::Context, events: Vec<egui::Event>, add: impl FnMut(&mut egui::Ui)) -> Vec<u8> {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(self.size[0] as f32, self.size[1] as f32))),
                time: Some(1.0),
                focused: true,
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, add);
            self.product_highlight_callbacks = 0;
            self.viewport_callbacks = 0;
            for clipped in &out.shapes {
                crate::test_support::each_shape(&clipped.shape, &mut |shape| {
                    if let egui::Shape::Callback(callback) = shape
                        && let Some(callback) = callback.callback.downcast_ref::<egui_wgpu::Callback>()
                    {
                        self.product_highlight_callbacks +=
                            usize::from(callback.downcast_ref::<solidcraft_viewport_egui::ProductHighlightCallback>().is_some());
                        self.viewport_callbacks += usize::from(callback.downcast_ref::<solidcraft_viewport_egui::ViewportCallback>().is_some());
                    }
                });
            }
            paint_egui_output(ctx, &mut out, &self.renderer, &self.device, &self.queue, &self.output);
            read_egui_pixels(&self.device, &self.queue, &self.output)
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn paint_egui_output(
        ctx: &egui::Context,
        out: &mut egui::FullOutput,
        renderer: &std::sync::Arc<egui::epaint::mutex::RwLock<egui_wgpu::Renderer>>,
        device: &super::wgpu::Device,
        queue: &super::wgpu::Queue,
        output: &super::wgpu::Texture,
    ) -> usize {
        paint_egui_output_in(egui::ViewportId::ROOT, ctx, out, renderer, device, queue, output)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn paint_egui_output_in(
        viewport: egui::ViewportId,
        ctx: &egui::Context,
        out: &mut egui::FullOutput,
        renderer: &std::sync::Arc<egui::epaint::mutex::RwLock<egui_wgpu::Renderer>>,
        device: &super::wgpu::Device,
        queue: &super::wgpu::Queue,
        output: &super::wgpu::Texture,
    ) -> usize {
        use super::*;
        let jobs = ctx.tessellate(std::mem::take(&mut out.shapes), out.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [output.width(), output.height()], pixels_per_point: out.pixels_per_point };
        let mut encoder = device.create_command_encoder(&Default::default());
        let mut renderer = renderer.write();
        for (id, deltas) in out.textures_delta.set.drain() {
            for delta in deltas {
                renderer.update_texture(device, queue, id, &delta);
            }
        }
        if let Some(effect) = &renderer.frame_compositor {
            effect.begin_viewport(viewport.0.value());
        }
        let extra = renderer.update_buffers(device, queue, &mut encoder, &jobs, &screen);
        let target = output.create_view(&Default::default());
        let composed = renderer
            .frame_compositor
            .as_ref()
            .is_some_and(|effect| effect.render(&renderer, &mut encoder, &target, &jobs, &screen, [0.0, 0.0, 0.0, 1.0]));
        if !composed {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("fixture ordinary UI"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            renderer.render(&mut pass, &jobs, &screen);
        }
        queue.submit(extra.into_iter().chain([encoder.finish()]));
        if let Some(effect) = &renderer.frame_compositor {
            effect.submitted();
        }
        for id in out.textures_delta.free.drain() {
            renderer.free_texture(&id);
        }
        drop(renderer);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        jobs.len()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn read_egui_pixels(device: &super::wgpu::Device, queue: &super::wgpu::Queue, texture: &super::wgpu::Texture) -> Vec<u8> {
        use super::*;
        let size = texture.size();
        let stride = (size.width * 4).div_ceil(256) * 256;
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("full egui readback"),
            size: u64::from(stride) * u64::from(size.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(stride), rows_per_image: Some(size.height) },
            },
            size,
        );
        queue.submit([encoder.finish()]);
        let slice = read.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let bytes = slice.get_mapped_range().unwrap();
        bytes.chunks_exact(stride as usize).flat_map(|row| row[..size.width as usize * 4].iter().copied()).collect()
    }

    /// For tests that look at pictures: install the headless GPU renderer (the CLI's). `false`, with
    /// the reason printed, on a machine without a GPU adapter, and the test skips.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn test_picture_renderer() -> bool {
        match solidcraft_viewport_gpu::capture::shared(solidcraft_gpu::env::policy().unwrap_or_else(|e| panic!("{e}"))) {
            Ok(c) => {
                solidcraft_engine::capture::install(std::sync::Arc::new(move |scene, cam, w, h| {
                    c.rgba(scene, cam, w, h)
                        .map(|rgba| solidcraft_engine::capture::Picture { rgba, renderer: c.renderer() })
                        .map_err(|e| e.to_string())
                }));
                true
            }
            Err(e @ solidcraft_viewport_gpu::GpuError::NoAdapter(_)) if std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_none() => {
                eprintln!("skipping: {e}");
                false
            }
            Err(e) => panic!("{e}"),
        }
    }
}
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) use test_support::{EguiFixture, test_picture_renderer};

/// The shader's analysis colours and flags for the theme's colours.
pub fn analysis_colors(c: &solidcraft_color::AnalysisColors) -> [f32; 36] {
    solidcraft_viewport_gpu::view::analysis_colors(c)
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
        let default = solidcraft_viewport_gpu::instance_descriptor_from_env().backends;
        if !cfg!(target_os = "windows") || std::env::var("WGPU_BACKEND").is_ok_and(|v| !v.trim().is_empty()) {
            return default;
        }
        let hardware = |backends: wgpu::Backends| {
            let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
            desc.backends = backends;
            desc.backend_options.dx12.shader_compiler = solidcraft_viewport_gpu::dx12_compiler();
            let adapters = pollster::block_on(solidcraft_viewport_gpu::select::start_instance(desc).enumerate_adapters(backends));
            adapters.iter().any(|a| a.get_info().device_type != wgpu::DeviceType::Cpu)
        };
        [wgpu::Backends::DX12, wgpu::Backends::VULKAN, wgpu::Backends::GL].into_iter().find(|b| hardware(*b)).unwrap_or(wgpu::Backends::DX12)
    })
}

/// Held while a test starts an instance, adapter or device: starting an instance scans the Vulkan
/// drivers, and tests doing that side by side crashed the test process inside the Vulkan loader
/// (`loader_icd_scan`). Rendering on the devices still runs side by side. The instance itself
/// starts through `select::start_instance`, so it also waits for an instance that
/// `solidcraft-viewport-gpu` is starting in the same test process (#10232).
#[cfg(all(test, not(target_arch = "wasm32")))]
fn offscreen_starting() -> std::sync::MutexGuard<'static, ()> {
    static STARTING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    STARTING.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The adapter an offscreen test opens under `policy` (`SOLIDCRAFT_GPU`, #9040), on
/// [`offscreen_backends`]; call it holding [`offscreen_starting`]. Under `auto` (the variable
/// unset) wgpu's default adapter, as before; under `hardware` or `software` the one
/// `solidcraft_viewport_gpu::select` picks for headless pictures of that tier, so `software` keeps the
/// tests off the hardware GPU; under `cpu`, as for pictures, none. `None` when none qualifies.
#[cfg(all(test, not(target_arch = "wasm32")))]
fn offscreen_adapter(policy: solidcraft_viewport_gpu::select::Policy) -> Option<wgpu::Adapter> {
    use solidcraft_viewport_gpu::select::Policy;
    let mut desc = solidcraft_viewport_gpu::instance_descriptor_from_env();
    desc.backends = offscreen_backends();
    let instance = solidcraft_viewport_gpu::select::start_instance(desc);
    match policy {
        Policy::Auto => pollster::block_on(instance.request_adapter(&Default::default())).ok(),
        Policy::Hardware | Policy::Software => {
            let found = pollster::block_on(instance.enumerate_adapters(offscreen_backends()));
            solidcraft_viewport_gpu::select::pick_among(found, policy).map(|(adapter, _)| adapter)
        }
        Policy::Cpu => None,
    }
}

/// The test process's offscreen device, for an offscreen render or check: started once, on
/// [`offscreen_own_device`], and shared by every test after it (#13395). A new device for every
/// render also compiled the viewport's pipelines anew: 28 renders took 55 s, on one device 4 s.
/// `None` when [`offscreen_own_device`] is; only a device is kept, so after a failed start (a GPU
/// briefly out of memory) the next test tries again rather than every later one skipping. A test
/// that loses its device, or changes how the device reports errors or its loss, takes
/// [`offscreen_own_device`] instead.
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn offscreen_device() -> Option<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    static SHARED: std::sync::Mutex<Option<(wgpu::Adapter, wgpu::Device, wgpu::Queue)>> = std::sync::Mutex::new(None);
    let mut shared = SHARED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if shared.is_none() {
        *shared = offscreen_own_device();
    }
    shared.clone()
}

/// A device of its own for an offscreen test (`None` without a GPU adapter the `SOLIDCRAFT_GPU`
/// policy admits, or when the variable names no policy: [`solidcraft_gpu::env`]), on [`offscreen_backends`]. One test at a
/// time starts its instance, adapter and device ([`offscreen_starting`]).
#[cfg(all(test, not(target_arch = "wasm32")))]
// Test-only (`allow-print-in-tests` does not see a `cfg(all(test, ..))`).
#[allow(clippy::print_stderr)]
pub(crate) fn offscreen_own_device() -> Option<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    let policy = solidcraft_gpu::env::policy().map_err(|e| eprintln!("no offscreen device: {e}")).ok()?;
    let _one_at_a_time = offscreen_starting();
    let adapter = offscreen_adapter(policy)?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: solidcraft_viewport_gpu::depth_features(&adapter),
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
    Some(solidcraft_viewport_gpu::view::tessellation(&adapter, &device, &queue, shader, solidcraft_gpu::env::force_cpu_tessellation()).is_some())
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
            product_lights: None,
            product: false,
            srgb: false,
            analysis,
            analysis_colors: colors,
            ssaa: 1.0,
            ao_radius: 0.0,
            ao_samples: solidcraft_viewport_gpu::view::AO_SAMPLES,
            preview: None,
            moving: None,
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
    product_lights: Option<Vec<[f32; 16]>>,
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
            product_lights,
            product: true,
            srgb,
            analysis: [0.0; 8],
            analysis_colors: [0.0; 36],
            ssaa: 1.0,
            ao_radius: 0.0,
            ao_samples: solidcraft_viewport_gpu::view::AO_SAMPLES,
            preview: None,
            moving: None,
        },
    )
}

#[cfg(all(test, not(target_arch = "wasm32")))]
struct Offscreen<'a> {
    tess: Option<&'a str>,
    /// The Render workspace's analytic preview lights (`Frame::product_lights`); `None` draws the
    /// product without them.
    product_lights: Option<Vec<[f32; 16]>>,
    product: bool,
    srgb: bool,
    analysis: [f32; 8],
    analysis_colors: [f32; 36],
    ssaa: f32,
    ao_radius: f32,
    ao_samples: u32,
    /// The live preview's scene (`Frame::pv_slot`); `None`: no preview.
    preview: Option<GpuScene>,
    /// The frame's moving layer (`Frame::moving`): a scene and where it is drawn, column-major.
    moving: Option<(GpuScene, [[f32; 4]; 4])>,
}

/// [`render_offscreen`] of a scene with a live preview over it: `preview` as the preview's
/// scene, and `moving` as the frame's moving layer (a scene and the motion it is drawn with,
/// column-major), as a preview that only moves bodies draws them.
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn render_offscreen_preview(
    scene: GpuScene,
    preview: GpuScene,
    moving: Option<(GpuScene, [[f32; 4]; 4])>,
    view_proj: [[f32; 4]; 4],
    back: [f32; 3],
    eye: [f32; 4],
    size: u32,
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
            product_lights: None,
            product: false,
            srgb: false,
            analysis: [0.0; 8],
            analysis_colors: [0.0; 36],
            ssaa: 1.0,
            ao_radius: 0.0,
            ao_samples: solidcraft_viewport_gpu::view::AO_SAMPLES,
            preview: Some(preview),
            moving,
        },
    )
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn render_ao_offscreen(
    scene: GpuScene,
    view_proj: [[f32; 4]; 4],
    back: [f32; 3],
    eye: [f32; 4],
    size: u32,
    tess: Option<&str>,
    ao_radius: f32,
) -> Option<Vec<u8>> {
    offscreen(
        scene,
        None,
        view_proj,
        back,
        eye,
        size,
        Offscreen {
            tess,
            product_lights: None,
            product: false,
            srgb: false,
            analysis: [0.0; 8],
            analysis_colors: [0.0; 36],
            ssaa: 1.0,
            ao_radius,
            ao_samples: solidcraft_viewport_gpu::view::AO_SAMPLES,
            preview: None,
            moving: None,
        },
    )
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
            product_lights: None,
            product: false,
            srgb: false,
            analysis: [0.0; 8],
            analysis_colors: [0.0; 36],
            ssaa,
            ao_radius: 0.0,
            ao_samples: solidcraft_viewport_gpu::view::AO_SAMPLES,
            preview: None,
            moving: None,
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
        Offscreen {
            tess: None,
            product_lights: None,
            product: false,
            srgb: false,
            analysis: [0.0; 8],
            analysis_colors: [0.0; 36],
            ssaa: 1.0,
            ao_radius,
            ao_samples,
            preview: None,
            moving: None,
        },
    )
}

#[cfg(all(test, not(target_arch = "wasm32")))]
// Test-only (`allow-print-in-tests` does not see a `cfg(all(test, ..))`).
#[allow(clippy::print_stderr)]
fn offscreen(
    scene: GpuScene,
    highlight: Option<GpuScene>,
    view_proj: [[f32; 4]; 4],
    back: [f32; 3],
    eye: [f32; 4],
    size: u32,
    options: Offscreen<'_>,
) -> Option<Vec<u8>> {
    let Offscreen { tess, product_lights, product, srgb, analysis, analysis_colors, ssaa, ao_radius, ao_samples, preview, moving } = options;
    let (adapter, device, queue) = offscreen_device()?;
    eprintln!("offscreen adapter: {:?}", adapter.get_info().backend);
    let format = if srgb { wgpu::TextureFormat::Rgba8UnormSrgb } else { wgpu::TextureFormat::Rgba8Unorm };
    let depth = solidcraft_viewport_gpu::depth_format(&device, true);
    let compute = tess.and_then(|shader| {
        solidcraft_viewport_gpu::view::tessellation(&adapter, &device, &queue, shader, solidcraft_gpu::env::force_cpu_tessellation())
    });
    let target = GpuTarget { format, depth: Some(depth), samples: 1, tess: compute.is_some(), oit: false, edges: false };
    let mut resources = egui_wgpu::CallbackResources::default();
    resources.insert(Renderer::new(&device, target, compute));
    let none = || Arc::new(Mutex::new(None));
    let cb = solidcraft_viewport_egui::ViewportCallback(Frame {
        // A preview frame with no session: no whole-body selection.
        selection_all: false,
        product_lights,
        product,
        upload_error: None,
        key: 1,
        slot: Arc::new(Mutex::new(Some(scene))),
        sketch_key: 0,
        sketch_slot: none(),
        hl_key: u64::from(highlight.is_some()),
        hl_slot: Arc::new(Mutex::new(highlight)),
        pv_key: u64::from(preview.is_some()),
        pv_slot: Arc::new(Mutex::new(preview)),
        pv_hide: false,
        preview_bounds: None,
        view_proj,
        turned_depth: None,
        back,
        size_px: [size as f32; 2],
        clip: [[0.0; 4]; solidcraft_viewport_gpu::view::MAX_SECTION_PLANES],
        clip_hatch: [0; solidcraft_viewport_gpu::view::MAX_SECTION_PLANES],
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
        edge_px: solidcraft_viewport_gpu::view::EDGE_PX,
        ssaa,
        sky: [0.0; 4],
        hover_tint: [0.0; 3],
        selection_tint: [0.0; 3],
        hatch: Default::default(),
        moving: moving.map(|(scene, transform)| MovingLayer { key: 1, slot: Arc::new(Mutex::new(Some(scene))), transform }),
        motions: Default::default(),
        ground: None,
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
        let depth = solidcraft_viewport_gpu::depth_format(&device, true);
        let compute = if tess {
            solidcraft_viewport_gpu::view::tessellation(
                &adapter,
                &device,
                &queue,
                solidcraft_viewport_gpu::SHADER,
                solidcraft_gpu::env::force_cpu_tessellation(),
            )
        } else {
            None
        };
        let target = GpuTarget { format, depth: Some(depth), samples: 1, tess: compute.is_some(), oit: false, edges: false };
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
        let [prepare, paint, wait] = self.frame_parts(frame);
        prepare + paint + wait
    }

    /// [`Self::frame`] in its parts, in milliseconds: the renderer's `prepare` (uploads, culling),
    /// recording the draws (`paint`), and the GPU's work until it has finished.
    pub(crate) fn frame_parts(&mut self, frame: Frame) -> [f64; 3] {
        let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1000.0;
        let started = std::time::Instant::now();
        let cb = solidcraft_viewport_egui::ViewportCallback(frame);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [self.size, self.size], pixels_per_point: 1.0 };
        let extra = egui_wgpu::CallbackTrait::prepare(&cb, &self.device, &self.queue, &screen, &mut encoder, &mut self.resources);
        let prepare = ms(started);
        let painted = std::time::Instant::now();
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
        let paint = ms(painted);
        let waited = std::time::Instant::now();
        self.queue.submit(extra.into_iter().chain([encoder.finish()]));
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        [prepare, paint, ms(waited)]
    }

    /// What the model scene's last change uploaded.
    pub(crate) fn uploads(&self) -> solidcraft_viewport_gpu::view::UploadStats {
        self.resources.get::<Renderer>().map(Renderer::uploads).unwrap_or_default()
    }

    /// What the highlight scene's last change uploaded.
    pub(crate) fn highlight_uploads(&self) -> solidcraft_viewport_gpu::view::UploadStats {
        self.resources.get::<Renderer>().map(Renderer::highlight_uploads).unwrap_or_default()
    }

    /// What the last frame drew.
    pub(crate) fn stats(&self) -> DrawStats {
        self.resources.get::<Renderer>().and_then(|r| r.stats().lock().ok().map(|s| *s)).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    /// The web app drew its scene at 1x whatever Graphics said (#11622). It now takes its sample
    /// count by the desktop's rule: the saved choice where the browser's device draws it, else
    /// the default it supports, and 1x on WebGL, where Graphics offers 1x alone.
    #[test]
    fn the_scene_takes_the_chosen_antialiasing_where_the_backend_draws_it() {
        use egui_wgpu::wgpu::Backend;
        // WebGPU guarantees 1x and 4x for the scene's formats.
        assert_eq!(super::scene_samples(Backend::BrowserWebGpu, 4, &[1, 4]), 4, "the default");
        assert_eq!(super::scene_samples(Backend::BrowserWebGpu, 1, &[1, 4]), 1, "Off");
        assert_eq!(super::scene_samples(Backend::BrowserWebGpu, 8, &[1, 4]), 4, "a desktop choice the browser lacks");
        assert_eq!(super::scene_samples(Backend::Gl, 4, &[1, 4]), 1, "WebGL");
        assert_eq!(super::scene_sample_choices(Backend::BrowserWebGpu, vec![1, 4]), [1, 4]);
        assert_eq!(super::scene_sample_choices(Backend::Gl, vec![1, 4]), [1]);
        assert_eq!(super::scene_sample_choices(Backend::Vulkan, vec![1, 2, 4, 8]), [1, 2, 4, 8]);
    }

    #[test]
    fn blur_radius_schedule_and_hostile_values() {
        use crate::blur::{schedule, validate_radius};
        for radius in [f32::NAN, f32::INFINITY, -1.0, 60.01] {
            assert!(validate_radius(radius).is_err());
            assert!(schedule(radius).is_none());
        }
        assert!(schedule(0.0).is_none());
        for (radius, steps) in [(1.0, 2), (8.0, 2), (9.0, 3), (18.0, 3), (19.0, 4), (30.0, 4), (31.0, 5), (40.0, 5), (60.0, 5)] {
            let (actual, offset) = schedule(radius).unwrap();
            assert_eq!(actual, steps);
            assert!(offset.is_finite() && offset > 0.0);
            let variance = (35.0 / 18.0) * (4.0_f32.powi(steps as i32) - 1.0);
            assert!((offset * variance.sqrt() - radius / 2.0).abs() < 0.0001);
        }
        for cap in 2..=5 {
            let (steps, offset) = crate::blur::schedule_capped(40.0, cap).unwrap();
            assert_eq!(steps, cap);
            let variance = (35.0 / 18.0) * (4.0_f32.powi(steps as i32) - 1.0);
            assert!((offset * variance.sqrt() - 20.0).abs() < 0.0001, "tier cap must preserve target radius");
        }
    }

    #[test]
    fn blur_guard_threshold_idle_minimum_and_session_undo() {
        let mut guard = crate::blur::Guard::default();
        let radius = 40.0;
        for _ in 0..59 {
            guard.observe(radius, 23.0, true);
        }
        assert_eq!((guard.cap, guard.streak, guard.notice), (5, 59, false));
        guard.observe(radius, 23.0, false); // idle, unavailable, hidden or no actual paint
        assert_eq!((guard.cap, guard.streak), (5, 0));
        for _ in 0..59 {
            guard.observe(radius, 23.0, true);
        }
        guard.observe(radius, 22.0, true); // strictly over budget
        assert_eq!((guard.cap, guard.streak), (5, 0));
        for _ in 0..59 {
            guard.observe(radius, 23.0, true);
        }
        assert_eq!(guard.cap, 5);
        guard.observe(radius, 23.0, true);
        assert_eq!((guard.cap, guard.notice), (4, true));
        guard.notice = false; // Dismiss does not stop later reductions or reannounce.
        for _ in 0..180 {
            guard.observe(radius, 23.0, true);
        }
        assert_eq!((guard.cap, guard.streak, guard.notice), (2, 0, false));
        assert_eq!(crate::blur::schedule_capped(radius, guard.cap).unwrap().0, 2);
        assert_eq!(radius, 40.0, "quality guard never turns radius down or off");
        guard.undo();
        assert_eq!((guard.cap, guard.notice, guard.opted_out), (5, false, true));
        guard.reset(); // theme/radius changes do not revoke the session opt-out
        for _ in 0..120 {
            guard.observe(18.0, 23.0, true);
        }
        assert_eq!(guard.cap, 5);
        for radius in [0.0, 1.0, 8.0] {
            let mut guard = crate::blur::Guard::default();
            for _ in 0..120 {
                guard.observe(radius, 23.0, true);
            }
            assert_eq!((guard.cap, guard.notice), (5, false));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn blur_guard_requires_consecutive_visible_actual_paints() {
        let ctx = egui::Context::default();
        let mut session = crate::blur::Session::default();
        let mut time = 0.0;
        let mut feed =
            |session: &mut crate::blur::Session, rendered: bool, continuous: bool, visible: bool, available: bool, revision: u64, radius: f32| {
                time += 0.023;
                let mut raw = egui::RawInput { time: Some(time), ..Default::default() };
                raw.viewports.entry(egui::ViewportId::ROOT).or_default().occluded = Some(!visible);
                ctx.run_ui(raw, |ui| {
                    ui.ctx().request_repaint(); // Deterministic harness only; product never does this for sampling.
                    let paint = rendered.then(|| crate::blur::PaintedFrame {
                        frame: ui.ctx().cumulative_frame_nr().saturating_sub(1),
                        interval_ms: Some(23.0),
                        continuous,
                    });
                    session.sample(ui.ctx(), revision, radius, available, paint);
                    session.sample(ui.ctx(), revision, radius, available, paint); // egui re-pass duplicate
                })
                .textures_delta
                .clear();
            };
        feed(&mut session, false, true, true, true, 1, 40.0);
        feed(&mut session, false, true, true, true, 1, 40.0);
        for _ in 0..59 {
            feed(&mut session, true, true, true, true, 1, 40.0);
        }
        assert_eq!((session.guard.cap, session.guard.streak), (5, 59));
        for (rendered, continuous, visible, available) in
            [(false, true, true, true), (true, false, true, true), (true, true, false, true), (true, true, true, false)]
        {
            feed(&mut session, rendered, continuous, visible, available, 1, 40.0);
            assert_eq!((session.guard.cap, session.guard.streak), (5, 0));
            feed(&mut session, true, true, true, true, 1, 40.0);
            assert_eq!(session.guard.streak, 1);
        }
        feed(&mut session, true, true, true, true, 2, 40.0); // theme contents changed
        assert_eq!(session.guard.streak, 0);
        feed(&mut session, true, true, true, true, 2, 18.0); // radius changed
        assert_eq!(session.guard.streak, 0);
        for _ in 0..59 {
            feed(&mut session, true, true, true, true, 2, 18.0);
        }
        assert_eq!((session.guard.cap, session.guard.streak), (5, 59));
        feed(&mut session, true, true, true, true, 2, 18.0);
        assert_eq!((session.guard.cap, session.guard.notice), (2, true));
        session.guard.undo();
        feed(&mut session, true, true, true, true, 3, 40.0);
        for _ in 0..100 {
            feed(&mut session, true, true, true, true, 3, 40.0);
        }
        assert_eq!((session.guard.cap, session.guard.opted_out), (5, true));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn blur_target_budget_rejects_empty_and_excessive_sizes() {
        for size in [[0, 1], [1, 0], [u32::MAX, 2], [16384, 16384]] {
            assert!(crate::blur::fixture_budget(size, 5).is_none());
        }
        for steps in 2..=5 {
            assert!(crate::blur::fixture_budget([1, 1], steps).is_some());
            assert!(crate::blur::fixture_budget([800, 600], steps).is_some());
        }
    }

    /// Full egui GPU harness: actual app UI, callbacks, fonts, marker slots and pass owner.
    /// This is intentionally distinct from model-only `ui.render`/OffscreenFrames.
    #[cfg(not(target_arch = "wasm32"))]
    struct BlurFixture {
        app: crate::SolveApp,
        ctx: egui::Context,
        renderer: std::sync::Arc<egui::epaint::mutex::RwLock<egui_wgpu::Renderer>>,
        adapter: egui_wgpu::wgpu::Adapter,
        device: egui_wgpu::wgpu::Device,
        queue: egui_wgpu::wgpu::Queue,
        output: egui_wgpu::wgpu::Texture,
        size: [u32; 2],
        jobs: usize,
        root_transform: egui::emath::TSTransform,
        root_layer: Option<egui::LayerId>,
    }

    #[cfg(not(target_arch = "wasm32"))]
    impl BlurFixture {
        fn radius(&mut self, radius: f32) {
            // Explicit PR5 rendering harness activation, independent of theme declarations.
            self.app.blur_session.harness_radius = Some(radius);
            self.app.blur.set_radius(radius).unwrap();
        }
        fn new(srgb: bool, policy: crate::blur::FixturePolicy) -> Option<Self> {
            use super::*;
            let Some((adapter, device, queue)) = offscreen_device() else {
                assert!(std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_none(), "GPU adapter required for blur fixture");
                eprintln!("skipped blur fixture: no admitted adapter/device");
                return None;
            };
            let format = if srgb { wgpu::TextureFormat::Rgba8UnormSrgb } else { wgpu::TextureFormat::Rgba8Unorm };
            let renderer = Arc::new(egui::epaint::mutex::RwLock::new(egui_wgpu::Renderer::new(&device, format, Default::default())));
            let mut app = crate::test_support::app_with_script(serde_json::json!({"commands":[
                {"command":"solid.box", "params":{"length":40,"width":30,"height":25}}
            ]}));
            app.home.open = false;
            app.preferences.ambient_occlusion = false;
            app.cpu_meshing = true;
            // Refinement uses wall time and can change the scene between fixed-time frames.
            // These fixtures check blur caching against stable display geometry.
            app.viewport.suspend_refinement = true;
            app.cam.distance = 110.0;
            app.cam.target = solidcore_record::Vec3::new(20.0, 15.0, 12.5);
            let window = GpuTarget { format, depth: None, samples: 1, tess: false, oit: false, edges: false };
            let scene = GpuTarget { depth: Some(solidcraft_viewport_gpu::depth_format(&device, true)), ..window };
            let view = Renderer::with_window_target(&device, scene, window, None);
            app.viewport.gpu = Some(view.target());
            app.viewport.draw_stats = Some(view.stats());
            renderer.write().callback_resources.insert(view);
            app.blur = crate::blur::fixture_state(&adapter, &device, &queue, &renderer, format, policy);
            assert_eq!(
                app.blur.software_adapter(),
                adapter.get_info().device_type == wgpu::DeviceType::Cpu,
                "fixture must retain real adapter identity"
            );
            if policy == crate::blur::FixturePolicy::Rendering {
                assert_eq!(app.blur.capability(), crate::blur::Capability::Available, "blur fixture requires real rendering capabilities");
            }
            let size = [800, 600];
            let output = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("full egui fixture"),
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let ctx = crate::fonts::test_context();
            Some(Self {
                app,
                ctx,
                renderer,
                adapter,
                device,
                queue,
                output,
                size,
                jobs: 0,
                root_transform: egui::emath::TSTransform::IDENTITY,
                root_layer: None,
            })
        }

        fn frame(&mut self) -> f64 {
            let started = std::time::Instant::now();
            // Keep local layout/input stationary, then transform only after emission.
            if let Some(layer) = self.root_layer {
                self.ctx.set_transform_layer(layer, egui::emath::TSTransform::IDENTITY);
            }
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(self.size[0] as f32, self.size[1] as f32))),
                time: Some(1.0),
                focused: true,
                ..Default::default()
            };
            let app = &mut self.app;
            let transform = self.root_transform;
            let mut root_layer = None;
            let mut out = self.ctx.run_ui(raw, |ui| {
                root_layer = Some(ui.layer_id());
                app.logic(ui.ctx());
                app.ui(ui);
                ui.ctx().set_transform_layer(ui.layer_id(), transform);
            });
            self.root_layer = root_layer;
            self.jobs = super::test_support::paint_egui_output(&self.ctx, &mut out, &self.renderer, &self.device, &self.queue, &self.output);
            started.elapsed().as_secs_f64() * 1000.0
        }

        fn pixels(&self) -> Vec<u8> {
            let texture = self.output.clone();
            super::test_support::read_egui_pixels(&self.device, &self.queue, &texture)
        }

        fn evidence(&self, name: &str) {
            let directory = std::env::var_os("SOLIDCRAFT_BLUR_EVIDENCE");
            let goldens = std::env::var_os("SOLIDCRAFT_BLUR_GOLDENS");
            if directory.is_none() && goldens.is_none() {
                return;
            }
            let image = image::RgbaImage::from_raw(self.size[0], self.size[1], self.pixels()).unwrap();
            if let Some(dir) = directory {
                let path = std::path::PathBuf::from(dir);
                std::fs::create_dir_all(&path).unwrap();
                image.save(path.join(format!("{name}.png"))).unwrap();
                std::fs::write(path.join(format!("{name}-counters.json")), serde_json::to_vec_pretty(&self.app.blur.counters()).unwrap()).unwrap();
            }
            if let Some(goldens) = goldens {
                let expected = image::open(std::path::PathBuf::from(goldens).join(format!("{name}.png"))).unwrap().to_rgba8();
                assert_eq!(image.as_raw(), expected.as_raw(), "golden {name}");
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn blur_production_policy_rejects_software_adapter() {
        use crate::blur::{Capability, FixturePolicy};
        let Some(mut f) = BlurFixture::new(false, FixturePolicy::Production) else { return };
        if solidcraft_viewport_gpu::select::tier_of(f.adapter.get_info().device_type) != solidcraft_viewport_gpu::select::Tier::Software {
            eprintln!("skipped software policy assertion: selected adapter is not software");
            return;
        }
        let unavailable = Capability::Unavailable("software adapter (gpu-software)");
        assert_eq!(f.app.blur.capability(), unavailable);
        f.radius(18.0);
        for _ in 0..6 {
            f.frame();
            assert_eq!(f.app.blur.radius(), 18.0, "each prepared fallback frame retains the requested radius");
        }
        assert_eq!(f.app.blur.capability(), unavailable);
        assert_eq!(f.app.blur.radius(), 18.0, "production fallback preserves the actual requested radius through prepare_blur");
        assert_eq!(f.app.blur_session.harness_radius, Some(18.0));
        assert_eq!(crate::prefs::blur_availability(&f.app), crate::theme_options::Availability::Unavailable("prefs.blur_software"));
        let counts = f.app.blur.counters();
        assert_eq!((counts.textures_created, counts.buffers_created, counts.bind_groups_created, counts.pipelines_created), (0, 0, 0, 0));
        assert_eq!((counts.markers_emitted, counts.markers_drawn), (0, 0));
        assert_eq!(counts.live_bytes, 0);
        assert!(f.renderer.read().frame_compositor.is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn blur_offered_zero_has_no_effect_work() {
        let Some(mut f) = BlurFixture::new(false, crate::blur::FixturePolicy::Production) else { return };
        assert!(f.app.blur_session.harness_radius.is_none(), "zero must come from the declared theme option");
        crate::prefs::control(
            &mut f.app,
            &serde_json::json!({"theme_import":{
                "format":"solidcraft-theme/1", "name":"Zero blur", "base":"dark",
                "options":[{"id":"blur","type":"range","default":0}]
            }}),
        )
        .unwrap();
        for _ in 0..6 {
            f.frame();
        }
        assert!(crate::theme::blur_intent(&f.ctx).offered);
        assert_eq!(crate::theme::blur_intent(&f.ctx).radius, 0.0);
        assert_eq!(f.app.blur.radius(), 0.0);
        assert!(f.app.blur_session.harness_radius.is_none());
        let counters = serde_json::to_value(f.app.blur.counters()).unwrap();
        for (name, count) in counters.as_object().unwrap() {
            assert_eq!(count, 0, "{name}");
        }
        assert!(f.renderer.read().frame_compositor.is_none());
        assert!(f.app.blur.painted_frame().is_none());
    }

    /// A surface translucent only to be seen through or because it fades (#10395) records no
    /// blur marker, while the same glass fill at full opacity does.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn blur_faded_and_see_through_surfaces_emit_no_marker() {
        use crate::ui_kit::{Container, LayoutMode, Surface};
        let Some(mut f) = BlurFixture::new(false, crate::blur::FixturePolicy::Rendering) else { return };
        f.radius(18.0);
        f.frame();
        let _scope = f.app.blur.begin(&f.ctx);
        assert!(crate::blur::enabled());
        let fill = egui::Color32::from_rgba_unmultiplied(40, 80, 100, 180);
        let frame = egui::Frame::NONE.fill(fill);
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(120.0, 80.0));
        let painter = f.ctx.layer_painter(egui::LayerId::background());
        let markers = |app: &crate::SolveApp| app.blur.counters().markers_emitted;
        let before = markers(&f.app);
        let _ = crate::blur::decorate(&painter, frame, rect, egui::Shape::Noop);
        assert_eq!(markers(&f.app), before + 1, "a glass panel at full opacity still blurs");
        let mut faded = painter.clone();
        faded.multiply_opacity(0.5);
        let _ = crate::blur::decorate(&faded, frame, rect, egui::Shape::Noop);
        let _ = crate::blur::decorate(&faded, egui::Frame::NONE.fill(egui::Color32::from_rgb(40, 80, 100)), rect, egui::Shape::Noop);
        assert_eq!(markers(&f.app), before + 1, "a faded surface, opaque fill included, blurs nothing");
        let host = Container::for_layout(LayoutMode::Floating, Surface::Panel);
        host.see_through().paint_background(&painter, frame, rect);
        assert_eq!(markers(&f.app), before + 1, "a see-through host blurs nothing");
        host.paint_background(&painter, frame, rect);
        assert_eq!(markers(&f.app), before + 2, "the ordinary host beside it still blurs");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn blur_search_picker_popup_and_tooltips_keep_native_backing() {
        use crate::ui_kit::{SearchChoice, SearchPicker};
        let Some(mut f) = BlurFixture::new(false, crate::blur::FixturePolicy::Rendering) else { return };
        f.radius(18.0);
        let heading = "Long provider heading ".repeat(4);
        let note = "Long discovery note ".repeat(20);
        for target in ["choice", "group", "heading", "note"] {
            let ctx = crate::fonts::test_context();
            ctx.global_style_mut(|s| {
                s.visuals.window_fill = egui::Color32::from_rgba_unmultiplied(40, 60, 80, 180);
                s.interaction.tooltip_delay = 0.0;
                s.interaction.show_tooltips_only_when_still = false;
                s.animation_time = 0.0;
            });
            let choices = [
                SearchChoice { value: "one", label: "One", tip: "First choice detail", group: Some(&heading) },
                SearchChoice { value: "two", label: "Two", tip: "Second choice detail", group: (target != "heading").then_some("Other") },
            ];
            let mut widget = egui::Id::NULL;
            let mut time = 0.0;
            let mut frame = |events| {
                // Native tooltips suppress movement just after a click for 100 ms.
                // Advance deterministic input time beyond that grace period; never
                // depend on the machine's speed to make a hover eligible.
                time += 0.25;
                crate::scenario::clear_handles();
                crate::test_support::frame(
                    &ctx,
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
                        events,
                        time: Some(time),
                        focused: true,
                        ..Default::default()
                    },
                    |ui| {
                        let _blur = f.app.blur.begin(ui.ctx());
                        ui.set_width(300.0);
                        crate::ui_kit::scope(ui, |ui| {
                            let result = crate::ui_kit::search_picker(
                                ui,
                                SearchPicker {
                                    id: "blur_picker",
                                    handles: "blur:picker",
                                    current: "One",
                                    current_value: "one",
                                    hint: "Search",
                                    choices: &choices,
                                    custom: false,
                                    note: Some(&note),
                                    open_group: None,
                                    enabled: true,
                                },
                            );
                            widget = result.response.id;
                            assert!(result.inner.is_none(), "hovering must not pick a value");
                        });
                    },
                )
            };
            frame(vec![]);
            let at = crate::scenario::handle_at("blur:picker").unwrap();
            frame(crate::test_support::pointer(at, true));
            frame(crate::test_support::pointer(at, false));
            let mut output = frame(vec![]);
            let at = match target {
                "choice" => crate::scenario::handle_at("blur:picker:one").unwrap(),
                "group" => crate::scenario::handle_at(&format!("blur:picker:group:{heading}")).unwrap(),
                _ => output
                    .shapes
                    .iter()
                    .find_map(|s| match &s.shape {
                        egui::Shape::Text(t) if t.galley.job.text.starts_with(if target == "note" { "Long discovery" } else { "Long provider" }) => {
                            Some(t.pos + t.galley.rect.center().to_vec2())
                        }
                        _ => None,
                    })
                    .expect("picker heading or note has visible text"),
            };
            for _ in 0..4 {
                output = frame(vec![egui::Event::PointerMoved(at)]);
            }
            let mut leaves = Vec::new();
            for clipped in &output.shapes {
                crate::test_support::each_shape(&clipped.shape, &mut |shape| leaves.push(shape.clone()));
            }
            let markers: Vec<_> = leaves
                .iter()
                .enumerate()
                .filter_map(|(i, shape)| match shape {
                    egui::Shape::Callback(cb) => Some((i, cb.rect)),
                    _ => None,
                })
                .collect();
            assert_eq!(markers.len(), 2, "{target}: popup and tooltip both have backing markers");
            let popup = ctx.read_response(widget.with("popup")).unwrap().rect;
            assert_eq!(markers[0].1, popup, "the marker covers popup padding and scrollbars");
            assert!(
                leaves
                    .iter()
                    .skip(markers[0].0 + 1)
                    .find(|shape| !matches!(shape, egui::Shape::Noop))
                    .is_some_and(|shape| matches!(shape, egui::Shape::Rect(_))),
                "backing precedes native popup chrome"
            );
            assert!(ctx.memory(|m| m.has_focus(egui::Id::unique("blur_picker").with("search"))), "native search focus survives hover");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn assert_backdrop_passes(before: crate::blur::Counters, after: crate::blur::Counters, steps: usize) {
        let markers = after.markers_drawn - before.markers_drawn;
        assert!(markers > 0, "the fixture must paint a visible backing boundary");
        let runs = after.blur_runs - before.blur_runs;
        assert!((1..=markers).contains(&runs), "markers that share a snapshot share one blur: {runs} runs for {markers} markers");
        assert_eq!(after.blur_passes - before.blur_passes, 2 * steps as u64 * runs, "each run filters the preceding UI once");
    }

    /// The blur of a marker's padded region is the whole-frame blur inside the marker (#19213).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn blur_region_matches_whole_frame_blur() {
        for srgb in [false, true] {
            let Some(mut f) = BlurFixture::new(srgb, crate::blur::FixturePolicy::Rendering) else { return };
            f.app.blur.set_surface_alpha(Some(180));
            for radius in [8.0, 18.0, 40.0] {
                f.radius(radius);
                f.app.blur.set_whole_frame_blur(false);
                for _ in 0..4 {
                    f.frame();
                }
                let before = f.app.blur.counters();
                f.frame();
                let regions = f.app.blur.counters();
                let region_pixels = f.pixels();
                f.app.blur.set_whole_frame_blur(true);
                for _ in 0..4 {
                    f.frame();
                }
                let before_whole = f.app.blur.counters();
                f.frame();
                let whole = f.app.blur.counters();
                let whole_pixels = f.pixels();
                let worst = region_pixels.iter().zip(&whole_pixels).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
                assert!(worst <= 1, "srgb {srgb} radius {radius}: region blur differs from whole-frame blur by {worst}/255");
                assert_eq!(regions.markers_drawn - before.markers_drawn, whole.markers_drawn - before_whole.markers_drawn);
                let (region_work, whole_work) = (regions.blur_pixels - before.blur_pixels, whole.blur_pixels - before_whole.blur_pixels);
                assert!(
                    region_work > 0 && region_work < whole_work,
                    "radius {radius}: {region_work} pixels in regions, {whole_work} for the whole frame"
                );
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn blur_full_egui_composition() {
        for srgb in [false, true] {
            let Some(mut f) = BlurFixture::new(srgb, crate::blur::FixturePolicy::Rendering) else { return };
            for _ in 0..6 {
                f.frame();
            }
            assert_eq!(f.app.blur.capability(), crate::blur::Capability::Available);
            let plain = f.pixels();
            let plain_jobs = f.jobs;
            let original_camera = f.app.cam;
            let original_background = f.app.preferences.viewport_background;
            let original_theme = f.app.preferences.theme.clone();
            let original_dark = f.app.ui.dark;
            assert_eq!(f.app.blur.counters().textures_created, 0);
            for dark in [false, true] {
                f.app.preferences.theme = if dark { "dark" } else { "light" }.into();
                f.app.ui.dark = dark;
                f.app.blur.set_surface_alpha(Some(180));
                f.app.cam = original_camera;
                f.app.preferences.viewport_background = original_background;
                f.radius(0.0);
                for _ in 0..3 {
                    f.frame();
                }
                // The zero reference uses the same camera, view settings and panel alpha.
                let zero_reference = f.pixels();
                f.frame();
                assert_eq!(zero_reference, f.pixels(), "stable zero reference");
                f.evidence(&format!("blur-{}-0-{}", if dark { "dark" } else { "light" }, if srgb { "srgb" } else { "unorm" }));
                for radius in [8.0, 18.0, 40.0] {
                    f.app.cam = original_camera;
                    f.app.preferences.viewport_background = original_background;
                    f.radius(radius);
                    for _ in 0..3 {
                        f.frame();
                    }
                    let before = f.app.blur.counters();
                    assert_eq!(
                        crate::prefs::blur_availability(&f.app),
                        crate::theme_options::Availability::Available,
                        "positive rendering fixtures must pass the shared app-aware capability policy"
                    );
                    assert_eq!(f.app.blur.radius(), radius);
                    let panels = &f.app.universal_panel_frame().unwrap().layout.sections;
                    assert!(!panels.is_empty());
                    assert!(panels.iter().all(|panel| panel.paint_alpha == Some(180)), "the shared panels keep the ribbon's blur tint alpha");
                    assert!(f.app.blur.painted_frame().is_some(), "guard evidence comes from actual submitted visible marker work");
                    let resting = f.pixels();
                    f.frame();
                    let after = f.app.blur.counters();
                    assert_backdrop_passes(before, after, crate::blur::schedule(radius).unwrap().0);
                    assert_eq!(f.pixels(), resting, "resting ordered composition is stable");
                    f.app.blur.set_surface_alpha(Some(120));
                    f.frame();
                    f.app.blur.set_surface_alpha(Some(180));
                    f.frame();
                    let positive = f.pixels();
                    let changed_pixels = zero_reference
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .zip(positive.as_chunks::<4>().0.iter())
                        .filter(|(zero, blurred)| (0..3).any(|channel| zero[channel].abs_diff(blurred[channel]) > 1))
                        .count();
                    assert!(changed_pixels > 0, "radius {radius} must change RGB pixels with the same camera, alpha and theme");
                    let name = format!("blur-{}-{}-{}", if dark { "dark" } else { "light" }, radius as u32, if srgb { "srgb" } else { "unorm" });
                    f.evidence(&name);
                    // There is no source to rebuild: a moved camera is only another frame to blur.
                    let before = f.app.blur.counters();
                    f.app.cam.yaw += 0.2;
                    f.frame();
                    let after = f.app.blur.counters();
                    assert_backdrop_passes(before, after, crate::blur::schedule(radius).unwrap().0);
                    assert_eq!(after.textures_created, before.textures_created, "a moved camera reuses storage");
                    f.app.cam = original_camera;
                    f.frame();
                    f.app.preferences.viewport_background = Some([3, 37, 93]);
                    let before = f.app.blur.counters();
                    f.frame();
                    let after = f.app.blur.counters();
                    assert_eq!(after.textures_created, before.textures_created, "background changes reuse storage");
                    f.app.preferences.viewport_background = original_background;
                    f.frame();
                }
                let before = f.app.blur.counters();
                f.app.blur_session.guard.cap = 3; // Explicit deterministic quality input in this GPU fixture.
                f.frame();
                let reduced = f.app.blur.counters();
                assert_eq!(f.app.blur.radius(), 40.0);
                assert_backdrop_passes(before, reduced, 3);
                f.frame();
                let cached = f.app.blur.counters();
                assert_backdrop_passes(reduced, cached, 3);
                f.app.blur_session.guard.cap = 5;
                f.frame();
                assert_backdrop_passes(cached, f.app.blur.counters(), 5);
                f.radius(0.0);
                for _ in 0..2 {
                    f.frame();
                }
                assert_eq!(zero_reference, f.pixels(), "on -> off preserves the fixed scene and tint");
            }
            f.app.blur.set_surface_alpha(None);
            f.app.cam = original_camera;
            f.app.preferences.theme = original_theme;
            f.app.ui.dark = original_dark;
            // Compare two stable zero frames after resources are dropped.
            for _ in 0..3 {
                f.frame();
            }
            let zero = f.pixels();
            let before = f.app.blur.counters();
            for _ in 0..3 {
                f.frame();
            }
            assert_eq!(zero, f.pixels());
            assert_eq!(plain, zero, "on -> off restores the original decoded pixels");
            assert_eq!(plain_jobs, f.jobs, "plain primitive count is restored");
            assert_eq!(before.textures_created, f.app.blur.counters().textures_created);
            assert_eq!(before.markers_emitted, f.app.blur.counters().markers_emitted);
            assert_eq!(f.app.blur.counters().live_bytes, 0);
            assert!(f.renderer.read().frame_compositor.is_none());
            assert!(plain.iter().any(|v| *v != 0));
            assert!(plain_jobs > 0);
        }
    }

    /// A frame with only egui-painted content (no viewport callback anywhere): stripes under an
    /// optional panel with a translucent fill, drawn as window `viewport`.
    #[cfg(not(target_arch = "wasm32"))]
    fn plain_frame(f: &mut BlurFixture, ctx: &egui::Context, viewport: egui::ViewportId, translucent: bool) {
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(f.size[0] as f32, f.size[1] as f32))),
            time: Some(1.0),
            focused: true,
            viewport_id: viewport,
            ..Default::default()
        };
        raw.viewports.insert(viewport, Default::default());
        let app = &mut f.app;
        let mut out = ctx.run_ui(raw, |ui| {
            let _blur = app.blur.begin(ui.ctx());
            for i in 0..40 {
                let x = i as f32 * 20.0;
                let color = if i % 2 == 0 { egui::Color32::from_rgb(230, 230, 230) } else { egui::Color32::from_rgb(20, 20, 20) };
                ui.painter().rect_filled(egui::Rect::from_min_size(egui::pos2(x, 0.0), egui::vec2(20.0, 600.0)), 0.0, color);
            }
            let alpha = if translucent { 120 } else { 255 };
            let frame = egui::Frame::new().fill(egui::Color32::from_rgba_unmultiplied(20, 30, 60, alpha)).corner_radius(8).inner_margin(12);
            crate::ui_kit::Container::show_backing(ui, frame, |ui| {
                ui.set_min_size(egui::vec2(300.0, 200.0));
            });
        });
        super::test_support::paint_egui_output_in(viewport, ctx, &mut out, &f.renderer, &f.device, &f.queue, &f.output);
    }

    /// With nothing to blur, a frame is the ordinary pass: no marker, no texture, no extra pass,
    /// and the pixels of blur off (#19213).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn blur_frame_without_a_marker_takes_the_ordinary_pass() {
        let Some(mut f) = BlurFixture::new(false, crate::blur::FixturePolicy::Rendering) else { return };
        let ctx = crate::fonts::test_context();
        for _ in 0..3 {
            plain_frame(&mut f, &ctx, egui::ViewportId::ROOT, false);
        }
        let plain = f.pixels();
        // Radius on, but the only panel is opaque: nothing asks for a backdrop.
        f.radius(18.0);
        for _ in 0..3 {
            plain_frame(&mut f, &ctx, egui::ViewportId::ROOT, false);
        }
        let counters = f.app.blur.counters();
        assert_eq!((counters.markers_emitted, counters.markers_drawn), (0, 0), "no surface is translucent");
        assert_eq!((counters.composition_passes, counters.present_passes, counters.blur_passes, counters.blur_pixels), (0, 0, 0, 0));
        assert_eq!((counters.textures_created, counters.pipelines_created, counters.live_bytes), (0, 0, 0));
        assert_eq!(f.pixels(), plain, "the ordinary pass draws the frame as with blur off");
    }

    /// The blur needs nothing from any view: a translucent panel over plain egui painting, with no
    /// viewport callback in the frame, blurs what is under it (#19213).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn blur_works_with_no_viewport_callback_in_the_frame() {
        let Some(mut f) = BlurFixture::new(false, crate::blur::FixturePolicy::Rendering) else { return };
        let ctx = crate::fonts::test_context();
        for _ in 0..3 {
            plain_frame(&mut f, &ctx, egui::ViewportId::ROOT, true);
        }
        let zero = f.pixels();
        f.evidence("no-viewport-panel-0");
        f.radius(18.0);
        for _ in 0..3 {
            plain_frame(&mut f, &ctx, egui::ViewportId::ROOT, true);
        }
        f.evidence("no-viewport-panel-18");
        let counters = f.app.blur.counters();
        assert!(counters.markers_drawn > 0 && counters.blur_runs > 0, "the panel asked for and drew a backdrop");
        assert_eq!(f.app.blur.capability(), crate::blur::Capability::Available);
        assert_ne!(f.pixels(), zero, "the stripes under the translucent panel are blurred");
    }

    /// A native pop-out window blurs behind its own translucent panels (#19213).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn blur_pop_out_window_blurs_its_own_panels() {
        let Some(mut f) = BlurFixture::new(false, crate::blur::FixturePolicy::Rendering) else { return };
        f.radius(18.0);
        let popout = egui::ViewportId::from_hash_of("blur_popout_fixture");
        let ctx = crate::fonts::test_context();
        for _ in 0..4 {
            let mut raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(f.size[0] as f32, f.size[1] as f32))),
                time: Some(1.0),
                focused: true,
                viewport_id: popout,
                ..Default::default()
            };
            raw.viewports.insert(popout, Default::default());
            let app = &mut f.app;
            let mut out = ctx.run_ui(raw, |ui| {
                let _blur = app.blur.begin(ui.ctx());
                ui.painter().rect_filled(ui.max_rect(), 0.0, egui::Color32::from_rgb(200, 40, 40));
                let frame = egui::Frame::new().fill(egui::Color32::from_rgba_unmultiplied(20, 30, 60, 120)).corner_radius(8).inner_margin(12);
                crate::ui_kit::Container::show_backing(ui, frame, |ui| {
                    ui.label("pop-out panel");
                });
                // A surface that is not a container host asks for its own backdrop.
                let own = egui::Rect::from_min_size(egui::pos2(300.0, 300.0), egui::vec2(120.0, 80.0));
                ui.painter().add(crate::blur::Backdrop::new(own).corners(6).shape(ui.painter()));
            });
            super::test_support::paint_egui_output_in(popout, &ctx, &mut out, &f.renderer, &f.device, &f.queue, &f.output);
        }
        f.evidence("pop-out-panel-18");
        let counters = f.app.blur.counters();
        assert!(counters.markers_drawn > 0, "the pop-out panel drew its backdrop");
        assert!(counters.blur_runs > 0);
        assert!(counters.markers_emitted >= 8, "the container panel and the Backdrop each emitted a marker per frame");
        f.radius(0.0);
        let ctx = crate::fonts::test_context();
        let shape = crate::blur::Backdrop::new(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(10.0, 10.0)))
            .shape(&ctx.layer_painter(egui::LayerId::background()));
        assert!(matches!(shape, egui::Shape::Noop), "blur off: a Backdrop is nothing");
        assert_eq!(f.app.blur.capability_for(popout), crate::blur::Capability::Available);
    }

    /// A layer transform applied after the UI was emitted moves the markers with it: the blur
    /// follows the transformed pixels (#19213).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn blur_late_layer_transform_keeps_blurring_placed_panels() {
        let Some(mut f) = BlurFixture::new(false, crate::blur::FixturePolicy::Rendering) else { return };
        f.radius(18.0);
        f.app.blur.set_surface_alpha(Some(180));
        for _ in 0..4 {
            f.frame();
        }
        f.root_transform = egui::emath::TSTransform { translation: egui::vec2(-23.0, 17.0), scaling: 0.75 };
        f.frame();
        let before = f.app.blur.counters();
        f.frame();
        assert_backdrop_passes(before, f.app.blur.counters(), 3);
    }

    /// Bodies are drawn from their plans on the GPU from the frame the evaluation shader has
    /// compiled, and built on the CPU until then (#4860).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn plans_are_drawn_on_the_gpu_once_the_evaluation_has_compiled() {
        let Ok(tess) = solidcraft_gpu::env::headless_tessellation() else {
            eprintln!("skipped: no GPU adapter that builds the evaluation");
            return;
        };
        let mut app = crate::SolveApp::new(Default::default(), Default::default());
        app.viewport.gpu = Some(super::GpuTarget {
            format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm,
            depth: None,
            samples: 1,
            tess: false,
            oit: false,
            edges: false,
        });
        let ready = solidcraft_viewport_egui::TessReady::default();
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
        use solidcraft_viewport_gpu::select::Policy;
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

    /// Captures draw an opaque backdrop, so an invisible result is a failed picture even if
    /// the GPU reports success (#15986). RGB bytes do not make a pixel visible without alpha.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_fully_transparent_gpu_picture_falls_back_to_the_cpu() {
        let (scene, cam) = (solidcraft_viewport::Scene::default(), solidcraft_viewport::Camera::default());
        let expected = super::cpu_picture(&scene, &cam, 4, 2);
        for pixel in [[0, 0, 0, 0], [120, 180, 240, 0]] {
            let calls = std::cell::Cell::new(0);
            let picture = super::picture_result(Ok(pixel.repeat(8)), "gpu", || {
                calls.set(calls.get() + 1);
                super::cpu_picture(&scene, &cam, 4, 2)
            })
            .unwrap();
            assert_eq!(calls.get(), 1, "an invisible capture must be redrawn");
            assert_eq!(picture.renderer, "cpu");
            assert_eq!(picture.rgba, expected.rgba);
            assert!(picture.rgba.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 255));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_visible_gpu_picture_keeps_its_pixels_and_renderer() {
        for renderer in ["gpu", "gpu-software"] {
            for rgba in [vec![10, 20, 30, 255, 40, 50, 60, 255], vec![0, 0, 0, 0, 40, 50, 60, 1]] {
                let picture = super::picture_result(Ok(rgba.clone()), renderer, || panic!("a visible picture needs no CPU replacement")).unwrap();
                assert_eq!(picture.renderer, renderer);
                assert_eq!(picture.rgba, rgba);
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn picture_errors_keep_device_loss_recovery_and_other_errors() {
        use solidcraft_viewport_gpu::GpuError;
        let (scene, cam) = (solidcraft_viewport::Scene::default(), solidcraft_viewport::Camera::default());
        let calls = std::cell::Cell::new(0);
        let picture = super::picture_result(Err(GpuError::DeviceLost), "gpu", || {
            calls.set(calls.get() + 1);
            super::cpu_picture(&scene, &cam, 4, 2)
        })
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(picture.renderer, "cpu");
        assert_eq!(picture.rgba, super::cpu_picture(&scene, &cam, 4, 2).rgba);
        let error = GpuError::Render("capture readback failed".into());
        let expected = error.to_string();
        let actual = super::picture_result(Err(error), "gpu", || panic!("other capture errors must be reported")).err().unwrap();
        assert_eq!(actual, expected);
    }

    /// A device the driver loses (#4172): its lost callback marks it, the painter sees the mark
    /// before the next frame's uploads, those uploads no longer reach the device (they panicked
    /// inside wgpu when they were the first to meet the loss), and pictures (thumbnails of saves)
    /// come from the CPU rasterizer. `destroy` loses the device as a driver reset does; wgpu calls
    /// the lost callback of a destroyed device once it has polled.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_lost_device_stops_drawing_and_pictures_fall_back_to_the_cpu() {
        use egui_wgpu::wgpu;
        let Some((adapter, device, queue)) = super::offscreen_own_device() else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let lost = egui_wgpu::DeviceLost::watch(&device);
        let capturer = solidcraft_viewport_gpu::capture::Capturer::new(&adapter, device.clone(), queue.clone()).unwrap().watching(lost.flag());
        let (scene, cam) = (solidcraft_viewport::Scene::default(), solidcraft_viewport::Camera::default());
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

    /// The probe runs after a panicked frame and before every capture, so on a working device it
    /// must cost nothing visible: no error for any scope or handler, and the uploads after it go
    /// through. On a reset D3D12 device an empty submission went through and the upload after it
    /// panicked inside wgpu (#6528); the probe now opens a command list, the driver call that
    /// upload's queue write would make first. A driver reset can't be made here: `destroy` loses the
    /// device in wgpu itself, before any driver call.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_lost_device_probe_is_clean_on_a_working_device() {
        use egui_wgpu::wgpu;
        let Some((adapter, device, queue)) = super::offscreen_own_device() else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let lost = egui_wgpu::DeviceLost::watch(&device);
        let capturer = solidcraft_viewport_gpu::capture::Capturer::new(&adapter, device.clone(), queue.clone()).unwrap().watching(lost.flag());
        let mut renderer = egui_wgpu::Renderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, Default::default());
        let image = egui::ImageData::Color(std::sync::Arc::new(egui::ColorImage::filled([4, 4], egui::Color32::WHITE)));
        let delta = egui::epaint::ImageDelta::full(image, egui::TextureOptions::LINEAR);
        let (scene, cam) = (solidcraft_viewport::Scene::default(), solidcraft_viewport::Camera::default());
        for frame in 0..3 {
            // The probe, then a queue write, as a frame after a survived panic makes them.
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

    /// A driver reset that lands mid-frame is met by a queue write (or the present),
    /// which panics inside wgpu (#10240). The painter survives that panic once the device turns
    /// out lost, and draws nothing more; on a working device a panic is a bug and carries on. A
    /// driver reset can't be made here: `destroy` loses the device in wgpu itself, so the probe
    /// after the panic finds it already marked.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_frame_that_meets_a_lost_device_is_survived() {
        use egui_wgpu::wgpu;
        let Some((_, device, queue)) = super::offscreen_own_device() else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let lost = egui_wgpu::DeviceLost::watch(&device);
        let met = || -> f32 { std::panic::panic_any(String::from("called `Result::unwrap()` on an `Err` value: Lost")) };
        assert_eq!(lost.survive(&device, &queue, || 0.5), Some(0.5));
        let bug = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lost.survive(&device, &queue, met)));
        assert!(bug.is_err(), "a panic on a working device carries on");
        assert!(!lost.is_lost());

        device.destroy();
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        assert_eq!(lost.survive(&device, &queue, met), None, "the frame's panic was the loss");
        assert!(lost.is_lost());
    }
}

#[cfg(test)]
mod background_picture_tests {
    use egui::{Rect, pos2, vec2};

    /// A window as wide as the picture shows all of it; a narrower one keeps its middle, at scale.
    #[test]
    fn a_picture_covers_the_window_from_its_centre() {
        let image = vec2(5120.0, 1440.0);
        let wide = Rect::from_min_size(pos2(0.0, 0.0), image);
        let all = super::background_uv(image, wide, wide);
        assert!(all.min.distance(pos2(0.0, 0.0)) < 1e-4 && all.max.distance(pos2(1.0, 1.0)) < 1e-4, "{all:?}");
        let small = Rect::from_min_size(pos2(0.0, 0.0), vec2(2560.0, 1440.0));
        let middle = super::background_uv(image, small, small);
        assert!(middle.min.distance(pos2(0.25, 0.0)) < 1e-4 && middle.max.distance(pos2(0.75, 1.0)) < 1e-4, "{middle:?}");
        // The view inside the window shows its own part, so the panels don't move the picture.
        let view = Rect::from_min_size(pos2(1280.0, 0.0), vec2(1280.0, 720.0));
        let part = super::background_uv(image, view, small);
        assert!(part.min.distance(pos2(0.5, 0.0)) < 1e-4 && part.max.distance(pos2(0.75, 0.5)) < 1e-4, "{part:?}");
        // A tall window keeps the picture's middle the other way.
        let tall = Rect::from_min_size(pos2(0.0, 0.0), vec2(500.0, 1000.0));
        let side = super::background_uv(vec2(100.0, 100.0), tall, tall);
        assert!(side.min.distance(pos2(0.25, 0.0)) < 1e-4 && side.max.distance(pos2(0.75, 1.0)) < 1e-4, "{side:?}");
    }

    #[test]
    fn a_big_picture_is_scaled_to_the_limit_and_junk_says_so() {
        assert!(super::decode_background(b"not a picture", 8192).is_err());
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(40, 20).write_to(&mut png, image::ImageFormat::Png).unwrap();
        assert_eq!(super::decode_background(png.get_ref(), 10).unwrap().size, [10, 5]);
    }

    /// #19138: a photo stored turned (EXIF orientation 6) is the background picture upright: a
    /// 60 x 40 file is shown 40 x 60, as it is when it carries no tag.
    #[test]
    fn a_background_photo_with_an_exif_orientation_stands_upright() {
        let plain = super::decode_background(&crate::test_pictures::jpeg(40, 60, None), 8192).unwrap();
        let turned = super::decode_background(&crate::test_pictures::jpeg(60, 40, Some(6)), 8192).unwrap();
        assert_eq!((plain.size, turned.size), ([40, 60], [40, 60]));
    }
}
