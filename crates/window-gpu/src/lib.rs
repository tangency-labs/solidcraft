//! SolidCraft's own window painter and runner on a stock egui-wgpu and egui-winit (no eframe).
//!
//! eframe paints every window through egui-wgpu's `winit::Painter`, configured from one setting and
//! with no way to take over a frame. Two things SolidCraft needs don't fit that: a window other
//! than the main one (a pop-out) presenting without waiting for vsync, so it can't halve the main
//! window's frame rate (#9118), and the blur behind panels, which composes the frame itself
//! (#8876). [`painter::Painter`] is egui-wgpu 0.37's painter with those (and two small fixes, its
//! docs list them), and [`runner`] runs the windows on it. The app chooses this runner only when
//! asked to; eframe stays the default.

pub use egui_wgpu;
pub use egui_wgpu::wgpu;

pub mod painter;
#[cfg(not(target_arch = "wasm32"))]
pub mod runner;

/// The [`FrameCompositor`] the painter hands whole frames to, kept in egui's renderer's callback
/// resources by whatever composes them (`renderer.callback_resources.insert(Compositor(..))`, and
/// `remove` to take frames back).
pub struct Compositor(pub std::sync::Arc<dyn FrameCompositor>);

/// Draw `jobs`, a run of a frame whose meshes were all uploaded with one `update_buffers`, into
/// `pass`: `mesh_offset` counts every mesh of the frame before the run (also those with an empty
/// clip rectangle; callbacks take no mesh). Stock egui-wgpu draws a frame from its first mesh, so
/// the run is led by `mesh_offset` empty meshes clipped to nothing, which only advance its place
/// in the uploaded buffers.
pub fn render_run(
    renderer: &egui_wgpu::Renderer,
    pass: &mut wgpu::RenderPass<'static>,
    jobs: &[egui::ClippedPrimitive],
    screen: &egui_wgpu::ScreenDescriptor,
    mesh_offset: usize,
) {
    let skipped = || egui::ClippedPrimitive { clip_rect: egui::Rect::ZERO, primitive: egui::epaint::Primitive::Mesh(egui::Mesh::default()) };
    let mut run = Vec::with_capacity(mesh_offset + jobs.len());
    run.extend(std::iter::repeat_with(skipped).take(mesh_offset));
    run.extend(jobs.iter().cloned());
    renderer.render(pass, &run, screen);
}

/// Optional ownership of a complete frame, outside the egui primitive loop (the blur behind
/// panels). [`FrameCompositor::render`] returning false keeps the ordinary pass.
pub trait FrameCompositor: Send + Sync {
    /// select the native viewport before any effect preparation/painting.
    fn begin_viewport(&self, _viewport: u64) {}
    /// actual acquired target geometry, before callback preparation.
    fn screen(&self, _screen: &egui_wgpu::ScreenDescriptor, _resources: &mut egui_wgpu::CallbackResources) {}
    /// release target-dependent state on zero size or a lost device.
    fn suspend(&self) {}
    fn render(
        &self,
        renderer: &egui_wgpu::Renderer,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        jobs: &[egui::ClippedPrimitive],
        screen: &egui_wgpu::ScreenDescriptor,
        clear: [f32; 4],
    ) -> bool;

    /// Called only after the commands producing cached pixels have been submitted.
    fn submitted(&self) {}
}

/// the present mode a viewport's surface is configured with. The main window keeps
/// `configured`. Another window (the agent chat popped out into its own window) is an immediate
/// viewport: eframe draws and presents it inside the main window's frame, one surface after the
/// other. With vsync on both, every frame waited for the display twice, and on a compositor that
/// sends no frame callbacks to a window it isn't showing (Wayland: on another workspace, scrolled
/// away, covered) the main window stalled behind the other one's present. So another window
/// presents without waiting where its surface can (Mailbox, which doesn't tear, before
/// Immediate); the main window's vsync still paces both. A `configured` mode without vsync is
/// kept everywhere.
pub fn viewport_present_mode(root: bool, configured: wgpu::PresentMode, supported: &[wgpu::PresentMode]) -> wgpu::PresentMode {
    use wgpu::PresentMode;
    let waits = matches!(configured, PresentMode::AutoVsync | PresentMode::Fifo | PresentMode::FifoRelaxed);
    if root || !waits {
        return configured;
    }
    [PresentMode::Mailbox, PresentMode::Immediate].into_iter().find(|mode| supported.contains(mode)).unwrap_or(configured)
}

/// the surface size for a window of `width` x `height` pixels on a device whose
/// textures are at most `max_side` on a side, and the scale it is drawn at. A window that fits is
/// unchanged (scale 1); a larger one shrinks uniformly, keeping its aspect ratio, so the image
/// the compositor stretches back over the window is not distorted. Never zero on a side.
pub fn clamp_surface_extent(width: u32, height: u32, max_side: u32) -> (u32, u32, f32) {
    let max_side = max_side.max(1);
    let (width, height) = (width.max(1), height.max(1));
    let longest = width.max(height);
    if longest <= max_side {
        return (width, height, 1.0);
    }
    let scale = f64::from(max_side) / f64::from(longest);
    let fit = |side: u32| -> u32 {
        let scaled = (f64::from(side) * scale).floor() as u32;
        scaled.clamp(1, max_side)
    };
    let scale_f32 = scale as f32;
    (fit(width), fit(height), scale_f32)
}

#[cfg(test)]
mod tests {
    use super::{clamp_surface_extent, viewport_present_mode};
    use crate::wgpu::PresentMode;

    #[test]
    fn a_window_that_fits_is_unchanged() {
        assert_eq!(clamp_surface_extent(3840, 2160, 8192), (3840, 2160, 1.0));
        assert_eq!(clamp_surface_extent(8192, 8192, 8192), (8192, 8192, 1.0));
    }

    #[test]
    fn the_reported_window_is_clamped_to_the_device_limit() {
        // #869: 11512 x 6808 requested against a maximum of 8192.
        let (w, h, s) = clamp_surface_extent(11512, 6808, 8192);
        assert_eq!(w, 8192);
        assert!((s - 8192.0 / 11512.0).abs() < 1e-6);
        let expected_h = 6808.0 * 8192.0 / 11512.0;
        assert!((f64::from(h) - expected_h).abs() <= 1.0, "{h} vs {expected_h}");
    }

    #[test]
    fn extreme_sizes_never_give_zero_or_exceed_the_limit() {
        for (w, h, m) in [(u32::MAX, 1, 8192), (1, u32::MAX, 8192), (u32::MAX, u32::MAX, 16384), (0, 0, 8192), (5000, 3000, 0), (100_000, 3, 2048)] {
            let (cw, ch, s) = clamp_surface_extent(w, h, m);
            assert!(cw >= 1 && ch >= 1, "{w}x{h} max {m} gave {cw}x{ch}");
            assert!(cw <= m.max(1) && ch <= m.max(1), "{w}x{h} max {m} gave {cw}x{ch}");
            assert!(s > 0.0 && s <= 1.0);
        }
    }

    #[test]
    fn a_popped_out_window_presents_without_waiting_for_the_display() {
        let wayland = [PresentMode::Fifo, PresentMode::Mailbox, PresentMode::Immediate];
        assert_eq!(viewport_present_mode(false, PresentMode::AutoVsync, &wayland), PresentMode::Mailbox);
        assert_eq!(viewport_present_mode(false, PresentMode::Fifo, &[PresentMode::Fifo, PresentMode::Immediate]), PresentMode::Immediate);
        // The main window keeps its vsync: it paces both.
        assert_eq!(viewport_present_mode(true, PresentMode::AutoVsync, &wayland), PresentMode::AutoVsync);
    }

    #[test]
    fn a_surface_that_only_waits_and_a_chosen_mode_without_vsync_are_kept() {
        assert_eq!(viewport_present_mode(false, PresentMode::AutoVsync, &[PresentMode::Fifo]), PresentMode::AutoVsync);
        assert_eq!(viewport_present_mode(false, PresentMode::Fifo, &[]), PresentMode::Fifo);
        for configured in [PresentMode::AutoNoVsync, PresentMode::Immediate, PresentMode::Mailbox] {
            assert_eq!(viewport_present_mode(false, configured, &[PresentMode::Fifo, PresentMode::Mailbox]), configured);
            assert_eq!(viewport_present_mode(true, configured, &[PresentMode::Fifo]), configured);
        }
    }
}
