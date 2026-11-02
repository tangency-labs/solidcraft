//! Motion for content that changes in place, such as a ribbon's tabs and their panels.
//!
//! [`CrossFade`]: the old content fades out while the content replacing it fades in, instead of
//! being swapped at once, optionally drifting a few points in the direction of travel. It works on
//! what was painted, not on the widgets: the caller lays out and paints the new content exactly as
//! it always does, and the fade only scales the opacity of those shapes (and offsets them while
//! they drift) and repaints a copy of the previous content's shapes over them, leaving. Layout
//! never changes during the change, only the new content is interactive, and a caller needs no
//! second "paint the old content" path.
//!
//! [`SlidingHighlight`]: a selection highlight (a tab's pill) that slides from the item it was on
//! to the one now selected, dimming while it travels and coming back to full as it lands.
//!
//! ```ignore
//! // Around code that paints into `ui`, keyed by what it shows:
//! let fade = CrossFade::default().begin(ui);
//! paint_page(ui, page);
//! fade.end(ui, "pages", page, region);
//!
//! // Pages in a row: the new page drifts in from the side it lies on.
//! let fade = CrossFade::default().drift(12.0).begin(ui);
//! paint_page(ui, page);
//! fade.end_ordered(ui, "pages", page, page_index as f32, region);
//!
//! // Or with a closure:
//! CrossFade::new(0.15).easing(Easing::Linear, Easing::Linear).show(ui, "pages", page, region, |ui| paint_page(ui, page));
//!
//! // A tab strip's highlight, painted under the labels:
//! let pill = SlidingHighlight::default().show(ui.ctx(), id, selected_tab, selected_rect, fill);
//! ui.painter().rect_filled(pill.rect, radius, pill.fill);
//! ```
//!
//! Only the caller's own layer fades; popups and menus the content opens on other layers do not.
//! A zero duration swaps at once and keeps no copy of the content, so turning the motion off
//! costs nothing.

use egui::epaint::ClippedShape;
use egui::layers::ShapeIdx;
use egui::{Color32, Id, LayerId, Rect, Shape, Vec2};

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
    /// Points the new content travels while it fades in, when the change has a direction
    /// ([`CrossFadeScope::end_ordered`]): it arrives from the side it lies on and settles on a
    /// decelerating curve, and the old content leaves half as far the other way. 0 (the default)
    /// fades in place.
    pub drift: f32,
}

impl Default for CrossFade {
    fn default() -> Self {
        Self { duration: CROSS_FADE_SECONDS, fade_in: Easing::Standard, fade_out: Easing::Standard, out_share: 0.5, drift: 0.0 }
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
    /// Where the content stands in its row, and which way the change goes (-1, 0 or 1).
    order: Option<f32>,
    direction: f32,
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

    /// How many points the content drifts in the direction of travel.
    pub fn drift(self, drift: f32) -> Self {
        Self { drift, ..self }
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

    /// Horizontal offsets of the new content and of the old, `elapsed` seconds into a change
    /// toward `direction` (1: the new content lies to the right of the old, -1: to the left).
    pub fn offsets(&self, elapsed: f32, direction: f32) -> (f32, f32) {
        let drift = if self.drift.is_finite() { self.drift.clamp(0.0, 200.0) } else { 0.0 };
        if !self.animates() || drift == 0.0 || !direction.is_finite() || direction == 0.0 {
            return (0.0, 0.0);
        }
        let t = elapsed / self.duration;
        let share = if self.out_share.is_finite() { self.out_share.clamp(0.05, 1.0) } else { 1.0 };
        let direction = direction.signum();
        (direction * drift * (1.0 - Easing::Decelerate.ease(t)), -direction * drift * 0.5 * self.fade_out.ease(t / share))
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
    /// content fades out and this content fades in. Both stay inside `region`. `id_salt` tells
    /// several fades in one `ui` apart.
    pub fn end(self, ui: &egui::Ui, id_salt: impl std::hash::Hash + std::fmt::Debug, key: impl std::hash::Hash + std::fmt::Debug, region: Rect) {
        self.end_ordered(ui, id_salt, key, 0.0, region);
    }

    /// [`Self::end`] for content in a row (tabs, pages): `order` is this content's place in it, so a
    /// change has a direction and, with [`CrossFade::drift`], the new content drifts in from the
    /// side it lies on.
    pub fn end_ordered(
        self,
        ui: &egui::Ui,
        id_salt: impl std::hash::Hash + std::fmt::Debug,
        key: impl std::hash::Hash + std::fmt::Debug,
        order: f32,
        region: Rect,
    ) {
        let Self { fade, layer, start } = self;
        let ctx = ui.ctx();
        let id = ui.id().with(("cross_fade", id_salt));
        let key = Id::new(key);
        let mut state = ctx.data_mut(|d| d.remove_temp::<State>(id)).unwrap_or_default();
        let order = if order.is_finite() { order } else { 0.0 };
        if !fade.animates() {
            // Nothing kept: switching the motion off frees the copy and costs nothing per pass.
            ctx.data_mut(|d| d.insert_temp(id, State { key: Some(key), order: Some(order), ..State::default() }));
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
            state.direction = state.order.map_or(0.0, |was| (order - was).signum() * f32::from(u8::from(order != was)));
        }
        state.key = Some(key);
        state.order = Some(order);
        state.last = painted;
        let opacities = state.since.map(|since| (elapsed(since), fade.opacities(elapsed(since), state.outgoing_from)));
        match opacities {
            Some((elapsed, (incoming, outgoing))) if elapsed < fade.duration => {
                let (dx_in, dx_out) = fade.offsets(elapsed, state.direction);
                ctx.graphics_mut(|g| {
                    let list = g.entry(layer);
                    let end = list.next_idx().0;
                    for idx in start.0..end {
                        list.mutate_shape(ShapeIdx(idx), |clipped| {
                            fade_shape(&mut clipped.shape, incoming);
                            if dx_in != 0.0 {
                                let by = Vec2::new(dx_in, 0.0);
                                clipped.shape.translate(by);
                                clipped.clip_rect = clipped.clip_rect.translate(by).intersect(region);
                            }
                        });
                    }
                    if outgoing > 0.0 {
                        let by = Vec2::new(dx_out, 0.0);
                        for clipped in &state.outgoing {
                            let clip = clipped.clip_rect.translate(by).intersect(region);
                            if clip.is_positive() {
                                let mut shape = clipped.shape.clone();
                                fade_shape(&mut shape, outgoing);
                                shape.translate(by);
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

/// Seconds the highlight takes to slide by default: a little longer than the cross-fade, so the
/// eye can follow it across a long row.
pub const SLIDE_SECONDS: f32 = 0.25;

/// A selection highlight that slides from the item it was on to the one now selected, taking the
/// new item's size and colour on the way. It dims while it travels and comes back to full
/// opacity as it lands, so the move reads as one gesture without the pill shouting mid-way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlidingHighlight {
    /// Seconds from the change until the highlight has landed; 0 (or less, or not a number) moves
    /// it at once.
    pub duration: f32,
    /// Curve of its position, size and colour.
    pub easing: Easing,
    /// How far its opacity dips while it travels: 0 not at all, 1 out of sight mid-way.
    pub dip: f32,
}

impl Default for SlidingHighlight {
    fn default() -> Self {
        Self { duration: SLIDE_SECONDS, easing: Easing::Decelerate, dip: 0.45 }
    }
}

/// Where to paint a [`SlidingHighlight`] this pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Highlight {
    pub rect: Rect,
    /// The fill, with the dip already applied.
    pub fill: Color32,
    /// Whether it has landed (no change under way).
    pub settled: bool,
}

/// What a highlight keeps between passes, per id.
#[derive(Clone)]
struct Slide {
    key: Option<Id>,
    /// Where it was and where it is going, with its colours there.
    from: Rect,
    from_fill: Color32,
    to: Rect,
    to_fill: Color32,
    /// When the slide started, while it is under way.
    since: Option<f64>,
    /// Share of the dip it had when a new slide interrupted it.
    carry: f32,
}

impl Default for Slide {
    fn default() -> Self {
        let (rect, fill) = (Rect::NOTHING, Color32::TRANSPARENT);
        Self { key: None, from: rect, from_fill: fill, to: rect, to_fill: fill, since: None, carry: 0.0 }
    }
}

impl SlidingHighlight {
    /// The default curve and dip with this duration in seconds.
    pub fn new(duration: f32) -> Self {
        Self { duration, ..Self::default() }
    }

    /// Whether this highlight moves at all.
    pub fn animates(&self) -> bool {
        self.duration.is_finite() && self.duration > 0.0
    }

    /// The rect and how dimmed (0..=1 of the dip) the highlight is at linear progress `t` of a
    /// slide from `from` to `to`, started while it was `carry` dimmed.
    pub fn at(&self, from: Rect, to: Rect, t: f32, carry: f32) -> (Rect, f32) {
        let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 1.0 };
        let e = self.easing.ease(t);
        let rect = Rect::from_min_max(from.min.lerp(to.min, e), from.max.lerp(to.max, e));
        // Flat-topped: it dims soon after leaving and brightens only as it lands.
        let bump = 1.0 - (2.0 * t - 1.0).powi(4);
        let carry = if carry.is_finite() { carry.clamp(0.0, 1.0) } else { 0.0 };
        (rect, bump.max(carry * (1.0 - t)).clamp(0.0, 1.0))
    }

    /// The highlight for the item `key`, whose own rect and fill are `target` and `fill`. When
    /// `key` differs from the previous pass's, the highlight slides there from where it was; while
    /// `key` stays, it follows `target` (a resized row) at once.
    pub fn show(&self, ctx: &egui::Context, id: Id, key: impl std::hash::Hash + std::fmt::Debug, target: Rect, fill: Color32) -> Highlight {
        let key = Id::new(key);
        let mut slide = ctx.data_mut(|d| d.remove_temp::<Slide>(id)).unwrap_or_default();
        let dip = if self.dip.is_finite() { self.dip.clamp(0.0, 1.0) } else { 0.0 };
        let settled = Highlight { rect: target, fill, settled: true };
        if !self.animates() || !target.is_finite() {
            ctx.data_mut(|d| d.insert_temp(id, Slide { key: Some(key), to: target, to_fill: fill, ..Slide::default() }));
            return settled;
        }
        let now = ctx.input(|i| i.time);
        let progress = |slide: &Slide| slide.since.map(|since| ((now - since).max(0.0) as f32) / self.duration);
        if slide.key.is_some_and(|k| k != key) {
            // From wherever it is now, mid-slide included, keeping the dim it had.
            let (rect, colour, dimmed) = match progress(&slide) {
                Some(t) if t < 1.0 => {
                    let (rect, dimmed) = self.at(slide.from, slide.to, t, slide.carry);
                    (rect, slide.from_fill.lerp_to_gamma(slide.to_fill, self.easing.ease(t)), dimmed)
                }
                _ => (slide.to, slide.to_fill, 0.0),
            };
            slide.from = rect;
            slide.from_fill = colour;
            slide.carry = dimmed;
            slide.since = Some(now);
        }
        slide.key = Some(key);
        slide.to = target;
        slide.to_fill = fill;
        let shown = match progress(&slide) {
            Some(t) if t < 1.0 => {
                let (rect, dimmed) = self.at(slide.from, slide.to, t, slide.carry);
                let colour = slide.from_fill.lerp_to_gamma(slide.to_fill, self.easing.ease(t));
                ctx.request_repaint();
                Highlight { rect, fill: colour.gamma_multiply(1.0 - dip * dimmed), settled: false }
            }
            _ => {
                slide.since = None;
                settled
            }
        };
        ctx.data_mut(|d| d.insert_temp(id, slide));
        shown
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
