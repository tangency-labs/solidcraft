//! Cross-fade: content that changes in place (a ribbon tab's panels, a page of a card) fades out
//! while the content replacing it fades in, instead of being swapped at once.
//!
//! The fade works on what was painted, not on the widgets: the caller lays out and paints the new
//! content exactly as it always does, and the fade only scales the opacity of those shapes and
//! repaints a copy of the previous content's shapes over them, fading out. Nothing moves or
//! changes size during the change, only the new content is interactive, and a caller needs no
//! second "paint the old content" path.
//!
//! ```ignore
//! // Around code that paints into `ui`, keyed by what it shows:
//! let fade = CrossFade::default().begin(ui);
//! paint_page(ui, page);
//! fade.end(ui, "pages", page, region);
//!
//! // Or with a closure:
//! CrossFade::new(0.15).easing(Easing::Linear, Easing::Linear).show(ui, "pages", page, region, |ui| paint_page(ui, page));
//! ```
//!
//! Only the caller's own layer fades; popups and menus the content opens on other layers do not.
//! A zero duration swaps at once and keeps no copy of the content, so turning the motion off
//! costs nothing.

use egui::epaint::ClippedShape;
use egui::layers::ShapeIdx;
use egui::{Color32, Id, LayerId, Rect, Shape};

/// Seconds a change takes by default: short enough to keep up with quick clicking, long enough to
/// read as one motion.
pub const CROSS_FADE_SECONDS: f32 = 0.2;

/// A timing curve: maps linear progress in 0..=1 to eased progress in 0..=1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Easing {
    Linear,
    /// Moves quickly at first, then settles: for content arriving.
    Decelerate,
    /// Starts slowly, then leaves quickly.
    Accelerate,
    /// Gentle at both ends: for content leaving.
    Standard,
    /// A cubic Bézier through (0, 0), (x1, y1), (x2, y2) and (1, 1), as in CSS
    /// `cubic-bezier(x1, y1, x2, y2)`. The x values are kept within 0..=1.
    CubicBezier(f32, f32, f32, f32),
}

impl Easing {
    /// Eased progress for linear progress `t`; values outside 0..=1 are clamped, and a value that
    /// is not a number counts as finished.
    pub fn ease(self, t: f32) -> f32 {
        if !t.is_finite() {
            return 1.0;
        }
        if t <= 0.0 {
            return 0.0;
        }
        if t >= 1.0 {
            return 1.0;
        }
        let (x1, y1, x2, y2) = match self {
            Self::Linear => return t,
            Self::Decelerate => (0.1, 0.9, 0.2, 1.0),
            Self::Accelerate => (0.4, 0.0, 1.0, 1.0),
            Self::Standard => (0.33, 0.0, 0.67, 1.0),
            Self::CubicBezier(x1, y1, x2, y2) => (x1, y1, x2, y2),
        };
        if ![x1, y1, x2, y2].iter().all(|v| v.is_finite()) {
            return t;
        }
        cubic_bezier(x1.clamp(0.0, 1.0), y1, x2.clamp(0.0, 1.0), y2, t)
    }
}

/// One coordinate of the Bézier with end points 0 and 1 and control values `a`, `b`, at `s`.
fn bezier(a: f32, b: f32, s: f32) -> f32 {
    let r = 1.0 - s;
    3.0 * r * r * s * a + 3.0 * r * s * s * b + s * s * s
}

/// y of the curve where x is `x`: x(s) rises monotonically when both x controls are in 0..=1, so
/// halving the interval always finds s (24 halvings are below f32 precision).
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if bezier(x1, x2, mid) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bezier(y1, y2, 0.5 * (lo + hi))
}

/// How a change fades: its length and the curves of the two halves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CrossFade {
    /// Seconds from the change until the new content is fully shown; 0 (or less, or not a
    /// number) swaps at once.
    pub duration: f32,
    /// Curve of the new content's opacity, from 0 to 1 over the whole duration.
    pub fade_in: Easing,
    /// Curve of the old content's fade, from its opacity at the change to 0.
    pub fade_out: Easing,
    /// Share of the duration the old content takes to disappear (0.05..=1). Below 1 the old
    /// content has mostly gone before the new one is much shown, so the two never read as one
    /// muddle at half opacity.
    pub out_share: f32,
}

impl Default for CrossFade {
    fn default() -> Self {
        Self { duration: CROSS_FADE_SECONDS, fade_in: Easing::Standard, fade_out: Easing::Standard, out_share: 0.5 }
    }
}

/// What a fade keeps between passes, per id.
#[derive(Clone, Default)]
struct State {
    /// What the content showed last pass.
    key: Option<Id>,
    /// What it painted then, before any fading.
    last: Vec<ClippedShape>,
    /// When the content last changed, while the change is still fading.
    since: Option<f64>,
    /// The content being replaced, and its opacity when it started to leave.
    outgoing: Vec<ClippedShape>,
    outgoing_from: f32,
}

/// An open fade: what [`CrossFade::begin`] noted before the content was painted.
#[must_use = "call `end` after painting the content"]
pub struct CrossFadeScope {
    fade: CrossFade,
    layer: LayerId,
    start: ShapeIdx,
}

impl CrossFade {
    /// The default curves with this duration in seconds.
    pub fn new(duration: f32) -> Self {
        Self { duration, ..Self::default() }
    }

    /// The curves of the incoming and of the outgoing content.
    pub fn easing(self, fade_in: Easing, fade_out: Easing) -> Self {
        Self { fade_in, fade_out, ..self }
    }

    /// The share of the duration the outgoing content takes to disappear.
    pub fn out_share(self, out_share: f32) -> Self {
        Self { out_share, ..self }
    }

    /// Whether this fade animates at all.
    pub fn animates(&self) -> bool {
        self.duration.is_finite() && self.duration > 0.0
    }

    /// Opacity of the new content and of the old, `elapsed` seconds into a change that started
    /// with the old content at opacity `from`.
    pub fn opacities(&self, elapsed: f32, from: f32) -> (f32, f32) {
        if !self.animates() {
            return (1.0, 0.0);
        }
        let t = elapsed / self.duration;
        let share = if self.out_share.is_finite() { self.out_share.clamp(0.05, 1.0) } else { 1.0 };
        let from = if from.is_finite() { from.clamp(0.0, 1.0) } else { 1.0 };
        (self.fade_in.ease(t), from * (1.0 - self.fade_out.ease(t / share)))
    }

    /// How selected the item `id` is (0..=1) while a selection moves, as a tab's highlight: it
    /// rises on the fade-in curve and falls on the fade-out curve, over the duration, so the old
    /// highlight leaves as the new one arrives. Without motion it is 0 or 1.
    pub fn selected(&self, ctx: &egui::Context, id: Id, selected: bool) -> f32 {
        if !self.animates() {
            return f32::from(u8::from(selected));
        }
        let shown = ctx.animate_bool_with_time(id, selected, self.duration);
        if selected { self.fade_in.ease(shown) } else { 1.0 - self.fade_out.ease(1.0 - shown) }
    }

    /// Notes where `ui`'s shapes stand; paint the content next, then call [`CrossFadeScope::end`].
    pub fn begin(self, ui: &egui::Ui) -> CrossFadeScope {
        let layer = ui.layer_id();
        let start = ui.ctx().graphics(|g| g.get(layer).map_or(ShapeIdx(0), |list| list.next_idx()));
        CrossFadeScope { fade: self, layer, start }
    }

    /// Paints `add_contents` and fades it in when `key` differs from the previous pass's (see
    /// [`CrossFadeScope::end`]).
    pub fn show<R>(
        self,
        ui: &mut egui::Ui,
        id_salt: impl std::hash::Hash + std::fmt::Debug,
        key: impl std::hash::Hash + std::fmt::Debug,
        region: Rect,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        let scope = self.begin(ui);
        let inner = add_contents(ui);
        scope.end(ui, id_salt, key, region);
        inner
    }
}

impl CrossFadeScope {
    /// Ends the fade around what was painted since [`CrossFade::begin`]. `key` names what the
    /// content shows (the tab, the page); when it differs from the previous pass's, the old
    /// content fades out and this content fades in. The old content stays inside `region`.
    /// `id_salt` tells several fades in one `ui` apart.
    pub fn end(self, ui: &egui::Ui, id_salt: impl std::hash::Hash + std::fmt::Debug, key: impl std::hash::Hash + std::fmt::Debug, region: Rect) {
        let Self { fade, layer, start } = self;
        let ctx = ui.ctx();
        let id = ui.id().with(("cross_fade", id_salt));
        let key = Id::new(key);
        let mut state = ctx.data_mut(|d| d.remove_temp::<State>(id)).unwrap_or_default();
        if !fade.animates() {
            // Nothing kept: switching the motion off frees the copy and costs nothing per pass.
            ctx.data_mut(|d| d.insert_temp(id, State { key: Some(key), ..State::default() }));
            return;
        }
        let now = ctx.input(|i| i.time);
        let elapsed = |since: f64| (now - since).max(0.0) as f32;
        let painted: Vec<ClippedShape> =
            ctx.graphics(|g| g.get(layer).map(|list| list.all_entries().skip(start.0).cloned().collect()).unwrap_or_default());
        if state.key.is_some_and(|k| k != key) {
            // Changed again before the last change finished: the content leaving starts from the
            // opacity it had reached, so nothing jumps.
            let from = state.since.map_or(1.0, |since| fade.opacities(elapsed(since), 1.0).0);
            state.outgoing = std::mem::take(&mut state.last);
            state.outgoing_from = from;
            state.since = Some(now);
        }
        state.key = Some(key);
        state.last = painted;
        let opacities = state.since.map(|since| (elapsed(since), fade.opacities(elapsed(since), state.outgoing_from)));
        match opacities {
            Some((elapsed, (incoming, outgoing))) if elapsed < fade.duration => {
                ctx.graphics_mut(|g| {
                    let list = g.entry(layer);
                    let end = list.next_idx().0;
                    for idx in start.0..end {
                        list.mutate_shape(ShapeIdx(idx), |clipped| fade_shape(&mut clipped.shape, incoming));
                    }
                    if outgoing > 0.0 {
                        for clipped in &state.outgoing {
                            let clip = clipped.clip_rect.intersect(region);
                            if clip.is_positive() {
                                let mut shape = clipped.shape.clone();
                                fade_shape(&mut shape, outgoing);
                                list.add(clip, shape);
                            }
                        }
                    }
                });
                ctx.request_repaint();
            }
            _ => {
                state.since = None;
                state.outgoing = Vec::new();
            }
        }
        ctx.data_mut(|d| d.insert_temp(id, state));
    }
}

/// Multiplies a shape's opacity by `opacity`. Text keeps its laid-out glyphs and changes only its
/// opacity factor, so fading text copies nothing.
fn fade_shape(shape: &mut Shape, opacity: f32) {
    if opacity >= 1.0 {
        return;
    }
    match shape {
        Shape::Noop => {}
        Shape::Vec(shapes) => shapes.iter_mut().for_each(|shape| fade_shape(shape, opacity)),
        Shape::Text(text) => text.opacity_factor *= opacity.max(0.0),
        shape => egui::epaint::shape_transform::adjust_colors(shape, move |color| {
            if *color != Color32::PLACEHOLDER {
                *color = color.gamma_multiply(opacity.max(0.0));
            }
        }),
    }
}

#[cfg(test)]
#[path = "cross_fade_tests.rs"]
mod tests;
