//! Sketch constraint glyph grouping, collision-free layout, badge expansion, and rendering.

mod actions;
mod grouping;
mod layout;
mod render;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Weak};

use egui::Pos2;
use solidcore_record::Vec2;
use solidcraft_engine::doc::ModelState;
use solidcraft_engine::sketch::Sketch;

pub use actions::{
    active_glyph_deletion, clear_clicked_constraint, click_group_badge, delete_glyph, detach_simulated_at, glyph_at, glyph_positions,
    note_clicked_constraint, over_constraint_ui, over_group_badge_or_expanded,
};
pub use grouping::{glyph_deletion, glyph_group};
pub use render::show;

/// Identifies the Design view window and viewport widget that published [`OVERLAY`] hit regions.
pub type ViewKey = (egui::ViewportId, egui::Id);

use grouping::{AnchorKey, ConstraintAnchorGroup, CurveRelGroup, CurveRelKind};

/// Glyph size and spacing (screen pixels) at the default `constraint_glyph_scale` of 1.
pub(crate) const GLYPH: f32 = 15.0;

/// The `constraint_glyph_scale` values Preferences › Sketch offers and the glyphs honour (#10794).
pub const GLYPH_SCALES: std::ops::RangeInclusive<f32> = 1.0..=3.0;

/// Accepted range for `constraint_group_threshold`: 1..=9 constraints, or 10 for Never.
pub const CONSTRAINT_GROUP_THRESHOLD_RANGE: std::ops::RangeInclusive<u8> = 1..=10;

/// Sentinel threshold value meaning never group constraint glyphs.
pub const CONSTRAINT_GROUP_NEVER: u8 = 10;

/// Screen pixels per millimetre of the sketch plane below which constraint glyphs hide when the
/// `hide_constraint_glyphs_zoomed_out` preference is on (#8980).
pub const MIN_GLYPH_PX_PER_MM: f32 = 2.0;

/// Below `MIN_GLYPH_PX_PER_MM`, glyphs still draw while the sketch spans at least this many
/// screen pixels (#10518).
pub const MIN_GLYPH_SPAN_PX: f32 = 20.0 * GLYPH;

/// Side of a constraint badge in screen pixels for the `constraint_glyph_scale` preference (#10794).
pub fn glyph_px(scale: f32) -> f32 {
    let scale = if scale.is_finite() { scale.clamp(*GLYPH_SCALES.start(), *GLYPH_SCALES.end()) } else { 1.0 };
    GLYPH * scale
}

/// Format a `constraint_group_threshold` slider value for display.
pub fn format_group_threshold(v: f64) -> String {
    if v.round() as u8 >= CONSTRAINT_GROUP_NEVER { solidcraft_ui_text::tr("prefs.constraint_group_never").to_owned() } else { format!("{:.0}", v) }
}

/// Whether constraint glyphs should draw at this sketch-plane scale (#8980, #10518).
pub fn constraint_glyphs_visible_at_scale(px_per_mm: f32, hide_when_zoomed_out: bool, span_px: impl FnOnce() -> f32) -> bool {
    !hide_when_zoomed_out || px_per_mm >= MIN_GLYPH_PX_PER_MM || span_px() >= MIN_GLYPH_SPAN_PX
}

pub(crate) const BADGE_HOVER_DELAY_SECS: f64 = 0.200;
pub(crate) const DETACH_HOVER_DELAY_SECS: f64 = 0.200;
pub(crate) const VC_HOVER_DELAY_SECS: f64 = 0.400;

/// Generic dwell-timer state shared by badge expansion and virtual-corner hover.
#[derive(Clone, Debug)]
pub(crate) struct HoverDelay<K: Copy + Eq> {
    pub(crate) pending: Option<(K, f64)>,
    pub(crate) active: Option<K>,
}

impl<K: Copy + Eq> HoverDelay<K> {
    pub(crate) const fn new() -> Self {
        Self { pending: None, active: None }
    }

    pub(crate) fn clear(&mut self) {
        self.pending = None;
        self.active = None;
    }

    /// Advance the dwell timer for `target` at timestamp `now`, requesting a repaint while pending.
    pub(crate) fn update(&mut self, target: Option<K>, now: f64, delay: f64, ctx: &egui::Context) -> Option<K> {
        let Some(key) = target else {
            self.clear();
            return None;
        };
        if self.active == Some(key) {
            self.pending = None;
            return self.active;
        }
        match self.pending {
            Some((k, since)) if k == key => {
                let elapsed = now - since;
                if elapsed >= delay {
                    self.active = Some(key);
                    self.pending = None;
                } else {
                    ctx.request_repaint_after(std::time::Duration::from_secs_f64((delay - elapsed).max(0.01)));
                }
            }
            _ => {
                self.pending = Some((key, now));
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(delay));
            }
        }
        self.active
    }
}

#[derive(Clone, Debug)]
pub(crate) struct DrawnRelInfo {
    pub(crate) id: String,
    pub(crate) center: Pos2,
    pub(crate) kind: CurveRelKind,
    pub(crate) group_index: usize,
    pub(crate) multi: bool,
    pub(crate) curve_id: String,
    pub(crate) all_constraint_ids: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct DetachItem {
    pub(crate) curve_id: String,
    pub(crate) point_id: String,
    pub(crate) glyph_rect: egui::Rect,
}

#[derive(Clone, Debug)]
pub(crate) struct DetachPreview {
    pub(crate) cluster_root: usize,
    pub(crate) selected_curve_id: Option<String>,
    pub(crate) point_rect: egui::Rect,
    pub(crate) box_rect: egui::Rect,
    pub(crate) items: Vec<DetachItem>,
    pub(crate) distance_mm: f64,
}

/// Cached per-sketch geometry analysis so mouse-move/hover/camera frames avoid recomputing
/// curve anchors, crossings, point clusters, and polylines.
pub(crate) struct SketchGlyphCache {
    pub(crate) world: Weak<ModelState>,
    pub(crate) sid: u64,
    pub(crate) fast_drag: bool,
    pub(crate) raw_groups: Vec<ConstraintAnchorGroup>,
    pub(crate) rel_groups: Vec<CurveRelGroup>,
    pub(crate) point_parent: Vec<usize>,
    pub(crate) vc_points: HashSet<usize>,
    pub(crate) visible_polylines: Vec<Vec<Vec2>>,
}

/// Cohesive per-frame & cross-frame state for sketch constraint overlays.
pub(crate) struct OverlayState {
    pub(crate) glyphs: Vec<(String, Pos2)>,
    pub(crate) drawn_rel: Vec<DrawnRelInfo>,
    pub(crate) picked: Option<String>,
    pub(crate) picked_rel: Option<(String, CurveRelKind, String, Vec<String>)>,
    pub(crate) collapsed_badges: Vec<(AnchorKey, egui::Rect)>,
    pub(crate) hit_rects: Vec<egui::Rect>,
    pub(crate) badge_expanded: Option<(AnchorKey, egui::Rect)>,
    pub(crate) badge_hover: HoverDelay<AnchorKey>,
    pub(crate) detach_hover: HoverDelay<usize>,
    pub(crate) vc_hover: HoverDelay<usize>,
    pub(crate) vc_boxes: Vec<(usize, egui::Rect)>,
    pub(crate) detach_preview: Option<DetachPreview>,
    pub(crate) prev_anchor_dirs: HashMap<AnchorKey, egui::Vec2>,
    pub(crate) sketch_cache: Option<Arc<SketchGlyphCache>>,
    pub(crate) glyph_px: f32,
    pub(crate) published_view: Option<ViewKey>,
}

impl OverlayState {
    fn new() -> Self {
        Self {
            glyphs: Vec::new(),
            drawn_rel: Vec::new(),
            picked: None,
            picked_rel: None,
            collapsed_badges: Vec::new(),
            hit_rects: Vec::new(),
            badge_expanded: None,
            badge_hover: HoverDelay::new(),
            detach_hover: HoverDelay::new(),
            vc_hover: HoverDelay::new(),
            vc_boxes: Vec::new(),
            detach_preview: None,
            prev_anchor_dirs: HashMap::new(),
            sketch_cache: None,
            glyph_px: GLYPH,
            published_view: None,
        }
    }

    pub(crate) fn clear_frame(&mut self) {
        self.glyphs.clear();
        self.drawn_rel.clear();
        self.picked_rel = None;
        self.collapsed_badges.clear();
        self.hit_rects.clear();
        self.badge_expanded = None;
        self.badge_hover.clear();
        self.detach_hover.clear();
        self.vc_hover.clear();
        self.vc_boxes.clear();
        self.detach_preview = None;
        self.prev_anchor_dirs.clear();
        self.published_view = None;
    }
}

thread_local! {
    pub(crate) static OVERLAY: RefCell<OverlayState> = RefCell::new(OverlayState::new());
}

#[cfg(test)]
pub(crate) fn push_test_glyph(id: String, center: Pos2) {
    OVERLAY.with(|cell| cell.borrow_mut().glyphs.push((id, center)));
}

pub(crate) fn pattern_icon_for(sk: &Sketch, id: &str) -> Option<&'static str> {
    sk.patterns.iter().find(|p| p.instances.first().and_then(|i| i.constraints.first()).map(String::as_str) == Some(id)).map(|p| {
        if matches!(p.kind, solidcraft_engine::sketch::SketchPatternKind::Circular { .. }) { "sketch_circ_pattern" } else { "sketch_rect_pattern" }
    })
}
