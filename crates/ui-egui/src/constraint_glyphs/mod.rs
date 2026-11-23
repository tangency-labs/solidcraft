//! Sketch constraint glyph grouping, collision-free layout, hover/click interaction, and rendering.

mod actions;
mod grouping;
mod layout;
mod render;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::collections::HashMap;

use egui::Pos2;
use solidcore_record::Vec2;
use solidcraft_engine::sketch::Sketch;

pub use actions::{
    HistoryConstraintSnapshot, activate_tool_at, active_glyph_deletion, break_symmetry_at, clear_clicked_constraint, click_group_badge, delete_glyph,
    delete_glyph_at, detach_simulated_at, glyph_at, glyph_positions, note_clicked_constraint, over_constraint_ui, over_group_badge_or_expanded,
    record_history_constraint_diff, snapshot_active_constraints,
};
pub use grouping::{glyph_deletion, glyph_group};
pub use render::show;

/// Identifies the Design view window and viewport widget that published [`OVERLAY`] hit regions.
pub type ViewKey = (egui::ViewportId, egui::Id);

use grouping::{AnchorKey, CurveRelKind, HoveredGeom, SelectedGeomKey};

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
pub(crate) fn glyph_px(scale: f32) -> f32 {
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
pub(crate) const GEOM_HOVER_DELAY_SECS: f64 = 0.400;
pub(crate) const HISTORY_FLASH_MS: f64 = 700.0;
pub(crate) const HISTORY_FADE_MS: f64 = 300.0;

/// Generic dwell-timer state shared by badge expansion, geometry hover, and virtual-corner hover.
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
pub(crate) struct DetachPreview {
    pub(crate) curve_id: String,
    pub(crate) point_id: String,
    pub(crate) point_rect: egui::Rect,
    pub(crate) glyph_rect: egui::Rect,
    pub(crate) distance_mm: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct SymmetryBreakPreview {
    pub(crate) point_id: String,
    pub(crate) constraint_id: String,
    pub(crate) axis_curve: usize,
    pub(crate) other_point: usize,
    pub(crate) glyph_rect: egui::Rect,
    pub(crate) prev_dir: egui::Vec2,
}

#[derive(Clone, Debug)]
pub(crate) struct ConstraintFlashGhost {
    pub(crate) id: String,
    pub(crate) kind_name: &'static str,
    pub(crate) pattern_icon: Option<&'static str>,
    pub(crate) at: Vec2,
    pub(crate) dir: Option<Vec2>,
    pub(crate) is_added: bool,
    pub(crate) expires_ms: f64,
}

pub(crate) struct HistoryFlashState {
    pub(crate) sketch: Option<u64>,
    pub(crate) added: HashMap<String, f64>,
    pub(crate) ghosts: Vec<ConstraintFlashGhost>,
}

impl HistoryFlashState {
    pub(crate) fn new() -> Self {
        Self { sketch: None, added: HashMap::new(), ghosts: Vec::new() }
    }
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
    pub(crate) geom_hover: HoverDelay<HoveredGeom>,
    pub(crate) geom_expanded: Vec<(AnchorKey, egui::Rect)>,
    pub(crate) geom_pinned: Option<(HoveredGeom, AnchorKey, egui::Rect)>,
    pub(crate) locked_sel: Option<SelectedGeomKey>,
    pub(crate) vc_hover: HoverDelay<usize>,
    pub(crate) vc_boxes: Vec<(usize, egui::Rect)>,
    pub(crate) detach_preview: Option<DetachPreview>,
    pub(crate) sym_break_previews: Vec<SymmetryBreakPreview>,
    pub(crate) prev_anchor_dirs: HashMap<AnchorKey, egui::Vec2>,
    pub(crate) history_flash: HistoryFlashState,
    pub(crate) glyph_px: f32,
    pub(crate) last_px_per_mm: f32,
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
            geom_hover: HoverDelay::new(),
            geom_expanded: Vec::new(),
            geom_pinned: None,
            locked_sel: None,
            vc_hover: HoverDelay::new(),
            vc_boxes: Vec::new(),
            detach_preview: None,
            sym_break_previews: Vec::new(),
            prev_anchor_dirs: HashMap::new(),
            history_flash: HistoryFlashState::new(),
            glyph_px: GLYPH,
            last_px_per_mm: 1.0,
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
        self.geom_hover.clear();
        self.geom_expanded.clear();
        self.geom_pinned = None;
        self.locked_sel = None;
        self.vc_hover.clear();
        self.vc_boxes.clear();
        self.detach_preview = None;
        self.sym_break_previews.clear();
        self.prev_anchor_dirs.clear();
        self.published_view = None;
    }
}

thread_local! {
    pub(crate) static OVERLAY: RefCell<OverlayState> = RefCell::new(OverlayState::new());
}

pub(crate) fn pattern_icon_for(sk: &Sketch, id: &str) -> Option<&'static str> {
    sk.patterns.iter().find(|p| p.instances.first().and_then(|i| i.constraints.first()).map(String::as_str) == Some(id)).map(|p| {
        if matches!(p.kind, solidcraft_engine::sketch::SketchPatternKind::Circular { .. }) { "sketch_circ_pattern" } else { "sketch_rect_pattern" }
    })
}
