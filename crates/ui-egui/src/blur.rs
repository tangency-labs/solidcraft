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
    schedule_capped(radius, 5)
}

/// A transient quality cap changes pass count, retaining the requested physical radius.
pub fn schedule_capped(radius: f32, cap: usize) -> Option<(usize, f32)> {
    if validate_radius(radius).ok()? == 0.0 {
        return None;
    }
    let steps = (if radius <= 8.0 {
        2
    } else if radius <= 18.0 {
        3
    } else if radius <= 30.0 {
        4
    } else {
        5
    })
    .min(cap.clamp(2, 5));
    // Per-axis tap variance: down 1/2, up 4/3, scaled by input texel size.
    // Target tap sigma = radius / 2. Resampling adds its own measurable footprint.
    let variance = (35.0 / 18.0) * (4.0_f32.powi(steps as i32) - 1.0);
    Some((steps, radius / (2.0 * variance.sqrt())))
}

/// Evidence from the native pass owner's actual submission, never UI construction time.
#[derive(Clone, Copy, Debug)]
pub struct PaintedFrame {
    pub frame: u64,
    pub interval_ms: Option<f64>,
    pub continuous: bool,
}

#[derive(Debug)]
pub(crate) struct Guard {
    pub cap: usize,
    pub streak: u8,
    pub notice: bool,
    pub opted_out: bool,
    original: Option<usize>,
    announced: bool,
}

impl Default for Guard {
    fn default() -> Self {
        Self { cap: 5, streak: 0, notice: false, opted_out: false, original: None, announced: false }
    }
}

impl Guard {
    pub fn reset(&mut self) {
        self.streak = 0;
    }

    pub fn observe(&mut self, radius: f32, interval_ms: f64, eligible: bool) {
        let steps = schedule_capped(radius, self.cap).map_or(0, |s| s.0);
        if self.opted_out || !eligible || !interval_ms.is_finite() || interval_ms <= 22.0 || steps <= 2 {
            self.reset();
            return;
        }
        self.streak = self.streak.saturating_add(1);
        if self.streak == 60 {
            self.original.get_or_insert(self.cap);
            self.cap = steps - 1;
            self.reset();
            if !self.announced {
                self.notice = true;
                self.announced = true;
            }
        }
    }

    pub fn undo(&mut self) {
        if let Some(original) = self.original.take() {
            self.cap = original;
        }
        self.opted_out = true;
        self.notice = false;
        self.reset();
    }
}

/// Session-only state. Harness inputs never replace a real GPU capability with success.
#[derive(Default)]
pub(crate) struct Session {
    pub guard: Guard,
    pub harness_radius: Option<f32>,
    pub harness_unavailable: Option<&'static str>,
    pub evidence_fixture: Option<&'static str>,
    identity: Option<(u64, u32)>,
    frame: Option<u64>,
    painted: Option<u64>,
    dismissed: std::collections::VecDeque<u64>,
}

impl Session {
    pub fn sample(&mut self, ctx: &egui::Context, revision: u64, radius: f32, available: bool, paint: Option<PaintedFrame>) {
        let frame = ctx.cumulative_frame_nr();
        if self.frame == Some(frame) {
            return;
        } // egui layout re-passes do not count twice.
        self.frame = Some(frame);
        let identity = (revision, radius.to_bits());
        if self.identity.as_ref() != Some(&identity) {
            self.identity = Some(identity);
            self.painted = paint.map(|p| p.frame);
            self.guard.reset();
            return;
        }
        let eligible = available
            && radius > 0.0
            && ctx.requested_repaint_last_pass()
            && ctx.input(|i| i.viewport().visible() != Some(false))
            && paint.is_some_and(|p| p.frame.checked_add(1) == Some(frame) && self.painted != Some(p.frame) && p.continuous);
        self.painted = paint.map(|p| p.frame);
        self.guard.observe(radius, paint.and_then(|p| p.interval_ms).unwrap_or(0.0), eligible);
    }

    pub fn dismissed(&self, key: u64) -> bool {
        self.dismissed.contains(&key)
    }
    pub fn dismiss(&mut self, key: u64) {
        if !self.dismissed(key) {
            if self.dismissed.len() == 64 {
                self.dismissed.pop_front();
            }
            self.dismissed.push_back(key);
        }
    }
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
        pub fn set_available(&mut self, _: bool) {}
        pub fn set_tier_cap(&mut self, _: usize) {}
        pub fn software_adapter(&self) -> bool {
            false
        }
        pub fn painted_frame(&self) -> Option<PaintedFrame> {
            None
        }
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
