//! Optional viewport-only blur. Plain frames do not record markers or own effect resources.
use egui::{Color32, Frame, Painter, Rect, Shape};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Capability {
    Available,
    Unavailable(&'static str),
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Counters {
    pub textures_created: u64,
    pub buffers_created: u64,
    pub bind_groups_created: u64,
    pub pipelines_created: u64,
    pub source_passes: u64,
    pub blur_passes: u64,
    pub composition_passes: u64,
    pub present_passes: u64,
    pub markers_emitted: u64,
    pub markers_drawn: u64,
    pub cache_hits: u64,
    pub live_bytes: u64,
    pub live_textures: u64,
    pub live_buffers: u64,
    pub live_bind_groups: u64,
    pub live_pipelines: u64,
}

impl Counters {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn clear_live(&mut self) {
        self.live_bytes = 0;
        self.live_textures = 0;
        self.live_buffers = 0;
        self.live_bind_groups = 0;
        self.live_pipelines = 0;
    }
}

pub fn validate_radius(radius: f32) -> Result<f32, &'static str> {
    if radius.is_finite() && (0.0..=40.0).contains(&radius) {
        Ok(radius)
    } else {
        Err("blur radius must be finite and between 0 and 40 physical pixels")
    }
}

/// Original Kawase schedule. Offsets are in input texels; the first level is half sized.
pub fn schedule(radius: f32) -> Option<(usize, f32)> {
    if validate_radius(radius).ok()? == 0.0 {
        return None;
    }
    let steps = if radius <= 8.0 {
        2
    } else if radius <= 18.0 {
        3
    } else if radius <= 30.0 {
        4
    } else {
        5
    };
    // Per-axis tap variance: down 1/2, up 4/3, scaled by input texel size.
    // Target tap sigma = radius / 2. Resampling adds its own measurable footprint.
    let variance = (35.0 / 18.0) * (4.0_f32.powi(steps as i32) - 1.0);
    Some((steps, radius / (2.0 * variance.sqrt())))
}

#[cfg(not(target_arch = "wasm32"))]
#[path = "blur_native.rs"]
mod implementation;
#[cfg(not(target_arch = "wasm32"))]
pub use implementation::*;

#[cfg(target_arch = "wasm32")]
mod implementation {
    use super::*;
    #[derive(Default)]
    pub struct State {
        radius: f32,
    }
    impl State {
        pub fn install(&mut self, _: &egui_wgpu::RenderState, _: bool) {}
        pub fn set_radius(&mut self, radius: f32) -> Result<(), &'static str> {
            self.radius = validate_radius(radius)?;
            Ok(())
        }
        pub fn radius(&self) -> f32 {
            self.radius
        }
        pub fn capability(&self) -> Capability {
            Capability::Unavailable("web compositor integration unavailable")
        }
        pub fn capability_for(&self, _: egui::ViewportId) -> Capability {
            self.capability()
        }
        pub fn counters(&self) -> Counters {
            Counters::default()
        }
        pub fn begin(&mut self, _: &egui::Context, _: bool) {}
        pub fn set_surface_alpha(&mut self, _: Option<u8>) {}
    }
    pub fn enabled() -> bool {
        false
    }
    pub fn panel_fill(fill: Color32) -> Color32 {
        fill
    }
    pub fn decorate(_: &Painter, _: Frame, _: Rect, shape: Shape) -> Shape {
        shape
    }
    pub fn viewport(painter: &Painter, rect: Rect, cb: crate::gpu::ViewportCallback, _: Rect, _: [Color32; 2]) {
        painter.add(egui_wgpu::Callback::new_paint_callback(rect, cb));
    }
}
#[cfg(target_arch = "wasm32")]
pub use implementation::*;
