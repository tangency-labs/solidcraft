//! The viewport in egui: its pipelines ([`solidcraft_viewport_gpu::view`]) drawn in `egui_wgpu` paint
//! callbacks, on the host's device and render pass, over a background the host paints first. The
//! same pipelines draw headless pictures ([`solidcraft_viewport_gpu::capture`]), so a window and a picture
//! shade alike.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::sync::Arc;

use solidcraft_viewport_gpu::view::{DrawStatsHandle, Frame, GpuTarget, Renderer};

/// A frame of the viewport as an egui paint callback.
pub struct ViewportCallback(pub Frame);
/// A transparent, depth-correct selection/hover layer over live product pixels.
pub struct ProductHighlightCallback(pub Frame);

/// The view's background: a vertical gradient from `top` at the top edge to `bottom` at the
/// bottom edge. The viewport callback composites transparently onto this egui mesh, so the host
/// passes the colours it shows (its theme's, as they animate); no background colour belongs to a
/// cached model scene or GPU upload.
pub fn paint_background(painter: &egui::Painter, rect: egui::Rect, top: egui::Color32, bottom: egui::Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// The GPU plan evaluation compiling beside the window: there once it is ready ([`install`]).
pub type TessReady = Arc<std::sync::OnceLock<solidcraft_viewport_gpu::TessGpu>>;

/// Create the pipelines in `rs`'s callback resources. Call once with the host's render state, the
/// scene target's depth/stencil/MSAA settings, and the target of the pass the picture is shown in.
/// Without a stencil the section cap does not hide the inside of cut bodies. Without
/// `gpu_meshing` (safe mode) bodies are meshed on the CPU, and with `force_cpu` (the host's
/// choice) they are on any device. When the two targets differ, the scene is drawn offscreen and
/// blitted. GPU errors no error scope catches go to the log and to `report`
/// ([`log_uncaptured_errors`]). Also returns the handle to what each frame drew, and the GPU plan
/// evaluation still compiling (native: the target says `tess: false` until it is ready).
/// [`build`] and [`adopt`] are its two halves, for a host that compiles beside its window.
pub fn install(
    rs: &egui_wgpu::RenderState,
    scene: GpuTarget,
    window: GpuTarget,
    gpu_meshing: bool,
    force_cpu: bool,
    report: impl Fn(&wgpu::Error) + Send + Sync + 'static,
) -> (GpuTarget, DrawStatsHandle, Option<TessReady>) {
    adopt(rs, build(rs, scene, window, gpu_meshing, force_cpu, report))
}

/// The viewport's pipelines, compiled but not yet in a render state's callback resources
/// ([`build`], [`adopt`]).
pub struct Built {
    /// The scene's target as the renderer draws it (order-independent transparency where it can).
    pub target: GpuTarget,
    /// The pipelines, for checks a host runs before [`adopt`] (the anti-aliasing readback).
    pub renderer: Renderer,
    stats: DrawStatsHandle,
    ready: Option<TessReady>,
}

/// [`install`]'s slow half, the shader compile (seconds on some devices): it only needs the
/// device, so a host can run it on a thread of its own while its window keeps drawing (#1210).
pub fn build(
    rs: &egui_wgpu::RenderState,
    scene: GpuTarget,
    window: GpuTarget,
    gpu_meshing: bool,
    force_cpu: bool,
    report: impl Fn(&wgpu::Error) + Send + Sync + 'static,
) -> Built {
    log_uncaptured_errors(&rs.device, report);
    // The evaluation shader compiles beside the window, which does not wait for it: on D3D12 it
    // takes seconds, most of the start (#4860). Bodies are built on the CPU until it is ready.
    #[cfg(not(target_arch = "wasm32"))]
    let later = gpu_meshing
        .then(|| solidcraft_viewport_gpu::view::TessLater::start(&rs.adapter, &rs.device, &rs.queue, solidcraft_viewport_gpu::SHADER, force_cpu));
    #[cfg(target_arch = "wasm32")]
    let tess = if gpu_meshing {
        solidcraft_viewport_gpu::view::tessellation(&rs.adapter, &rs.device, &rs.queue, solidcraft_viewport_gpu::SHADER, force_cpu)
    } else {
        None
    };
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
    let target = res.target();
    let stats = res.stats();
    Built { target, renderer: res, stats, ready }
}

/// [`install`]'s other half, on the window's thread: the pipelines [`build`] made go into `rs`'s
/// callback resources. Returns the scene's target, the handle to what each frame draws, and the
/// GPU plan evaluation still compiling.
pub fn adopt(rs: &egui_wgpu::RenderState, built: Built) -> (GpuTarget, DrawStatsHandle, Option<TessReady>) {
    rs.renderer.write().callback_resources.insert(built.renderer);
    (built.target, built.stats, built.ready)
}

/// A GPU error no error scope caught is logged and handed to `report`, never a panic: a device's
/// default handler panics with "wgpu error: ...", which would close the app with the open design
/// (#5486 reported "wgpu error: Out of Memory" from such a panic). The headless picture devices do
/// the same (`Capturer::headless`). Errors the code expects are caught under error scopes where
/// they happen, and those take priority over this handler. Errors before [`install`] (egui's own
/// pipelines at startup) still reach the default handler, so a startup failure stays a crash
/// report with its startup step. Only the first few are reported: an invalid resource repeats its
/// error on every frame.
pub fn log_uncaptured_errors(device: &wgpu::Device, report: impl Fn(&wgpu::Error) + Send + Sync + 'static) {
    use std::sync::atomic::{AtomicU32, Ordering};
    let reported = Arc::new(AtomicU32::new(0));
    device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
        if reported.fetch_add(1, Ordering::Relaxed) < 8 {
            log::error!("GPU: {e}");
            report(&e);
        }
    }));
}

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

impl egui_wgpu::CallbackTrait for ProductHighlightCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(renderer) = resources.get_mut::<Renderer>() {
            renderer.prepare_product_highlights(device, queue, encoder, &self.0);
        }
        Vec::new()
    }
    fn paint(&self, info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, resources: &egui_wgpu::CallbackResources) {
        let Some(renderer) = resources.get::<Renderer>() else { return };
        let viewport = info.viewport_in_pixels();
        renderer.paint_product_highlights(
            &self.0,
            pass,
            [viewport.left_px as f32, viewport.top_px as f32, viewport.width_px as f32, viewport.height_px as f32],
        );
    }
}

#[cfg(test)]
mod tests {
    use egui::{Rect, pos2, vec2};

    /// The background is drawn from the two colours it is given, top edge then bottom edge, on a
    /// bare context: no theme is published or read.
    #[test]
    fn the_background_is_a_gradient_of_the_colours_given() {
        let (top, bottom) = (egui::Color32::from_rgb(10, 20, 30), egui::Color32::from_rgb(200, 150, 100));
        let rect = Rect::from_min_size(pos2(5.0, 7.0), vec2(300.0, 200.0));
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| super::paint_background(ui.painter(), rect, top, bottom));
        // The first frame of a bare context uploads the font atlas; nothing displays it here.
        out.textures_delta.clear();
        let meshes: Vec<_> = out
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) => Some(mesh),
                _ => None,
            })
            .collect();
        assert_eq!(meshes.len(), 1);
        let corners: Vec<_> = meshes[0].vertices.iter().map(|v| (v.pos, v.color)).collect();
        let want = [(rect.left_top(), top), (rect.right_top(), top), (rect.right_bottom(), bottom), (rect.left_bottom(), bottom)];
        assert_eq!(corners, want);
        assert_eq!(meshes[0].indices, [0, 1, 2, 0, 2, 3]);
    }
}
