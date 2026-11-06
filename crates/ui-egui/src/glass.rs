//! A theme's glass finish (`borders.glass` in a theme file): a sheen that fades down from the top
//! of every see-through surface, streaks of light across them, glossy push buttons, and glossy
//! bubbles or tiles behind the command, navigation and title bar buttons. How see-through the
//! surfaces are is the theme's Opacity option; the finish only adds light to them. Themes without
//! `borders.glass` paint exactly as before.

use egui::{Color32, Stroke};
use serde_json::{Map, Value, json};

use crate::theme::Tokens;

thread_local! {
    /// The user's own gloss (`set_gloss`), 0 to 1, over the theme's sheen; `None`: the theme's.
    static GLOSS: std::cell::Cell<Option<f32>> = const { std::cell::Cell::new(None) };
}

/// The user's own gloss over a glass theme's sheen and streaks (`prefs::apply`, every frame): how
/// strong they are, 0 (none) to 1; `None` keeps the theme's. Not a number: the theme's.
pub fn set_gloss(gloss: Option<f32>) {
    GLOSS.set(gloss.filter(|g| g.is_finite()).map(|g| g.clamp(0.0, 1.0)));
}

/// The glass finish of a theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glass {
    /// The sheen's colour at the top edge of a surface; it fades out over `sheen_height` points.
    /// Its alpha is also how bright the streaks are.
    pub sheen: Color32,
    pub sheen_height: f32,
    /// Command, navigation and title bar buttons sit on glossy bubbles of this shape in place of
    /// flat hover and pressed fills. `None`: flat fills.
    pub bubbles: Option<BubbleShape>,
}

/// The shape of a glass theme's bubbles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BubbleShape {
    /// Orbs round the icon.
    #[default]
    Round,
    /// Tiles over the whole button, with the theme's control corners.
    Square,
}

impl BubbleShape {
    pub fn id(self) -> &'static str {
        match self {
            BubbleShape::Round => "round",
            BubbleShape::Square => "square",
        }
    }
}

/// What a bubble shows of its button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bubble {
    Rest,
    Hover,
    /// Pressed, or on (a running command, a selected tool).
    On,
    Disabled,
}

impl Bubble {
    /// A button's bubble state: disabled, on (pressed, or `on`: running, selected, open), under
    /// the pointer or focused, or at rest.
    pub fn of(response: &egui::Response, enabled: bool, on: bool) -> Self {
        if !enabled {
            Bubble::Disabled
        } else if on || response.is_pointer_button_down_on() {
            Bubble::On
        } else if response.hovered() || response.has_focus() {
            Bubble::Hover
        } else {
            Bubble::Rest
        }
    }

    /// The body's tint, the rim's and the white shine of a bubble in this state.
    fn strengths(self) -> (f32, f32, f32) {
        match self {
            Bubble::Rest => (0.16, 0.45, 0.7),
            Bubble::Hover => (0.32, 0.75, 0.9),
            Bubble::On => (0.6, 1.0, 0.8),
            Bubble::Disabled => (0.08, 0.25, 0.35),
        }
    }
}

/// A bubble painted behind a button's icon (`Tokens::bubble`); `finish` adds an orb's gloss over
/// the icon once that is drawn.
#[derive(Clone, Copy, Debug)]
pub struct Bubbled {
    glass: Glass,
    center: egui::Pos2,
    radius: f32,
    state: Bubble,
}

impl Bubbled {
    pub fn finish(self, painter: &egui::Painter) {
        self.glass.paint_bubble_gloss(painter, self.center, self.radius, self.state);
    }
}

/// How far the light streaks lean: across per down.
const STREAK_SLANT: f32 = 0.9;
/// The share of a streak's width over which each edge fades in or out.
const STREAK_EDGE: f32 = 0.35;
/// The streaks: where across the window a band starts at its top, how wide it is and how bright.
const STREAKS: [(f32, f32, f32); 4] = [(0.18, 84.0, 0.5), (0.235, 30.0, 0.42), (0.52, 120.0, 0.36), (0.6, 40.0, 0.45)];
/// Bounds of a theme file's `sheen_height`, in points.
const MAX_SHEEN_HEIGHT: f64 = 128.0;

impl Glass {
    /// The sheen as drawn: the theme's colour, as strong as the user's gloss when they set one.
    fn lit(self) -> Color32 {
        match GLOSS.get() {
            Some(gloss) => {
                let [r, g, b, _] = self.sheen.to_srgba_unmultiplied();
                Color32::from_rgba_unmultiplied(r, g, b, (gloss * 255.0).round() as u8)
            }
            None => self.sheen,
        }
    }

    /// A glossy bubble of `radius` behind an icon at `center`, tinted with `tint`: a translucent
    /// body, a glow along its bottom and a rim. `paint_bubble_gloss` then covers the icon with the
    /// bubble's highlight. Paint only: no layout or input target. Too small or non-finite: nothing.
    pub fn paint_bubble(self, painter: &egui::Painter, center: egui::Pos2, radius: f32, tint: Color32, state: Bubble) {
        if !center.is_finite() || !radius.is_finite() || radius < 4.0 {
            return;
        }
        let (body, rim, shine) = state.strengths();
        // The glow: brightest at the bottom edge, gone above the middle.
        let glow = |p: egui::Pos2| Color32::WHITE.gamma_multiply(0.45 * shine * ((p.y - center.y) / radius).clamp(0.0, 1.0));
        match self.bubbles.unwrap_or_default() {
            BubbleShape::Round => {
                painter.circle_filled(center, radius, tint.gamma_multiply(body));
                fan(painter, center, Color32::TRANSPARENT, ellipse(center, egui::vec2(radius, radius)), glow);
                painter.circle_stroke(center, radius - 0.5, Stroke::new(1.0, tint.gamma_multiply(rim)));
                painter.circle_stroke(center, radius - 1.5, Stroke::new(1.0, Color32::WHITE.gamma_multiply(0.5 * shine)));
            }
            BubbleShape::Square => {
                let corner = (radius * 0.3).round().clamp(2.0, 12.0) as u8;
                let tile = egui::Rect::from_center_size(center, egui::vec2(radius * 2.0, radius * 2.0));
                self.paint_tile(painter, tile, egui::CornerRadius::same(corner), tint, state);
            }
        }
    }

    /// A glass tile filling `rect` with its own `corner`s (a button's whole face, or one segment
    /// of a joined group): a translucent body in `tint`, a glow along its bottom, a gloss over
    /// its top half that fades out at the middle, and a rim. All of it goes behind the icon and
    /// label the caller draws next, so the gloss never covers them. Too small: nothing.
    pub fn paint_tile(self, painter: &egui::Painter, rect: egui::Rect, corner: egui::CornerRadius, tint: Color32, state: Bubble) {
        if !rect.is_finite() || rect.width() < 6.0 || rect.height() < 6.0 {
            return;
        }
        let (body, rim, shine) = state.strengths();
        let center = rect.center();
        painter.rect_filled(rect, corner, tint.gamma_multiply(body));
        let half = rect.height() * 0.5;
        let glow = |p: egui::Pos2| Color32::WHITE.gamma_multiply(0.45 * shine * ((p.y - center.y) / half).clamp(0.0, 1.0));
        fan(painter, center, Color32::TRANSPARENT, outline(rect, corner), glow);
        // The gloss: the top half inside the rim, brightest at the top and gone at the middle.
        let face = rect.shrink(1.5);
        let inner_corner = shrunk(corner, 1);
        let fade = |y: f32| Color32::WHITE.gamma_multiply(0.6 * shine * (1.0 - (y - face.top()) / (center.y - face.top())).clamp(0.0, 1.0));
        let top = outline(face, inner_corner).into_iter().map(|p| egui::pos2(p.x, p.y.min(center.y))).collect();
        fan(painter, center, fade(center.y), top, |p| fade(p.y));
        if let Some(streaks) = self.streaks_mesh(painter.ctx().content_rect(), rect.shrink(1.0)) {
            painter.add(streaks);
        }
        painter.rect_stroke(rect, corner, Stroke::new(1.0, tint.gamma_multiply(rim)), egui::StrokeKind::Inside);
        painter.rect_stroke(rect.shrink(1.0), inner_corner, Stroke::new(1.0, Color32::WHITE.gamma_multiply(0.5 * shine)), egui::StrokeKind::Inside);
    }

    /// An orb's highlight, painted over its icon like a glass dome: an ellipse across the top
    /// half, fading downwards, light enough that the icon reads through it. Tiles have theirs
    /// under the icon already. The icon's own artwork is unchanged.
    pub fn paint_bubble_gloss(self, painter: &egui::Painter, center: egui::Pos2, radius: f32, state: Bubble) {
        if !center.is_finite() || !radius.is_finite() || radius < 4.0 || self.bubbles != Some(BubbleShape::Round) {
            return;
        }
        let shine = 0.6 * state.strengths().2;
        let top = egui::pos2(center.x, center.y - radius * 0.42);
        let size = egui::vec2(radius * 0.72, radius * 0.48);
        let fade = |p: egui::Pos2| Color32::WHITE.gamma_multiply(shine * ((top.y + size.y * 0.7 - p.y) / (size.y * 1.7)).clamp(0.0, 1.0));
        fan(painter, top, fade(top), ellipse(top, size), fade);
    }

    /// The sheen of `rect` and its `radius` corners as a shape: the sheen's colour along the top,
    /// fading out `sheen_height` points down. Too small, non-finite or no sheen: `None`.
    pub fn sheen_mesh(self, rect: egui::Rect, radius: impl Into<egui::CornerRadius>) -> Option<egui::Mesh> {
        let height = self.sheen_height.min(rect.height() * 0.5);
        if !rect.is_finite() || !height.is_finite() || height < 2.0 || rect.width() < 4.0 || self.lit().a() == 0 {
            return None;
        }
        let path = outline(rect, radius.into());
        let count = u32::try_from(path.len()).ok()?;
        // A fan from the point where the sheen has faded out; the outline is convex.
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(egui::pos2(rect.center().x, rect.top() + height), Color32::TRANSPARENT);
        for point in &path {
            mesh.colored_vertex(*point, self.lit().gamma_multiply((1.0 - (point.y - rect.top()) / height).clamp(0.0, 1.0)));
        }
        for i in 0..count {
            mesh.add_triangle(0, i + 1, (i + 1) % count + 1);
        }
        Some(mesh)
    }

    /// The streaks of light `rect` shows: one set of slanted bands across the whole window
    /// (`screen`), as if one light falls on all its glass, so they line up across every glass
    /// surface; each surface shows the part it covers, cut to its rectangle and brightest along its
    /// top. White light as strong as the sheen's alpha. `None`: nothing to show.
    pub fn streaks_mesh(self, screen: egui::Rect, rect: egui::Rect) -> Option<egui::Mesh> {
        if !rect.is_finite() || !screen.is_finite() || rect.width() < 4.0 || rect.height() < 4.0 || self.lit().a() == 0 {
            return None;
        }
        let light = Color32::WHITE.gamma_multiply(f32::from(self.lit().a()) / 255.0);
        let mut mesh = egui::Mesh::default();
        for (at, width, strength) in STREAKS {
            // The band leans: its left edge moves right as it goes down the window. It is soft:
            // its light rises across its first third, holds, and fades across its last third, each
            // part a strip of its own so the light changes evenly across it.
            let left = |y: f32| screen.left() + screen.width() * at + (y - screen.top()) * STREAK_SLANT;
            let profile = |p: egui::Pos2| {
                let across = ((p.x - left(p.y)) / width).clamp(0.0, 1.0);
                (across / STREAK_EDGE).min((1.0 - across) / STREAK_EDGE).min(1.0)
            };
            for (from, to) in [(0.0, STREAK_EDGE), (STREAK_EDGE, 1.0 - STREAK_EDGE), (1.0 - STREAK_EDGE, 1.0)] {
                let strip = [
                    egui::pos2(left(rect.top()) + width * from, rect.top()),
                    egui::pos2(left(rect.top()) + width * to, rect.top()),
                    egui::pos2(left(rect.bottom()) + width * to, rect.bottom()),
                    egui::pos2(left(rect.bottom()) + width * from, rect.bottom()),
                ];
                let piece = clip_x(&clip_x(&strip, rect.left(), true), rect.right(), false);
                let Ok(first) = u32::try_from(mesh.vertices.len()) else { return None };
                let Ok(count) = u32::try_from(piece.len()) else { continue };
                if count < 3 {
                    continue;
                }
                for p in &piece {
                    let down = ((p.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
                    mesh.colored_vertex(*p, light.gamma_multiply(strength * profile(*p) * (1.0 - 0.45 * down)));
                }
                for i in 1..count - 1 {
                    mesh.add_triangle(first, first + i, first + i + 1);
                }
            }
        }
        (!mesh.is_empty()).then_some(mesh)
    }

    /// The glossy face of a push button (`Tokens::button_bevel`): the top half brighter, fading
    /// towards a soft split across the middle, inside the button's corners. Dimmer while pressed
    /// or on, fainter when disabled. Paint only; too small or no sheen: nothing.
    pub fn paint_gloss(self, painter: &egui::Painter, rect: egui::Rect, radius: impl Into<egui::CornerRadius>, pressed: bool, enabled: bool) {
        if !rect.is_finite() || rect.height() < 8.0 || rect.width() < 8.0 || self.lit().a() == 0 {
            return;
        }
        let strength = match (enabled, pressed) {
            (false, _) => 0.25,
            (true, true) => 0.35,
            (true, false) => 0.75,
        };
        let split = rect.center().y;
        // The outline's lower half folds up onto the split: the top half's rounded outline.
        let face = |y: f32| self.lit().gamma_multiply(strength * (0.3 + 0.7 * ((split - y) / (split - rect.top())).clamp(0.0, 1.0)));
        let top = outline(rect, radius.into()).into_iter().map(|p| egui::pos2(p.x, p.y.min(split))).collect();
        fan(painter, egui::pos2(rect.center().x, split), face(split), top, |p| face(p.y));
    }

    /// A see-through surface's backing (`ui_kit::Container`): `shape` (its frame, or nothing when
    /// egui paints the frame itself), then the sheen and the streaks over its fill, all under its
    /// contents. An opaque or empty fill gets no light: only see-through glass catches it.
    pub fn backing(self, painter: &egui::Painter, frame: egui::Frame, content: egui::Rect, shape: egui::Shape) -> egui::Shape {
        if frame.fill.a() == 0 || frame.fill.a() == u8::MAX {
            return shape;
        }
        let widget = frame.widget_rect(content).shrink(frame.stroke.width);
        let sheen = self.sheen_mesh(widget, frame.corner_radius);
        let streaks = self.streaks_mesh(painter.ctx().content_rect(), widget.shrink(1.0));
        if sheen.is_none() && streaks.is_none() {
            return shape;
        }
        egui::Shape::Vec(std::iter::once(shape).chain(sheen.map(Into::into)).chain(streaks.map(Into::into)).collect())
    }
}

impl Tokens {
    /// A button's bubble in a glass theme with bubbles, behind the icon and label the caller draws
    /// next; then `Bubbled::finish`. A tile fills the whole of `rect` (icon and label) with the
    /// theme's control corners; an orb sits inside it. `None`: the theme has no bubbles, and the
    /// button keeps its own fill.
    pub fn bubble(&self, painter: &egui::Painter, rect: egui::Rect, tint: Color32, state: Bubble) -> Option<Bubbled> {
        let glass = self.borders.glass.filter(|glass| glass.bubbles.is_some())?;
        let bubbled = Bubbled { glass, center: rect.center(), radius: rect.width().min(rect.height()) * 0.5, state };
        match glass.bubbles {
            Some(BubbleShape::Square) => glass.paint_tile(painter, rect, egui::CornerRadius::same(self.control_radius), tint, state),
            _ => glass.paint_bubble(painter, bubbled.center, bubbled.radius, tint, state),
        }
        Some(bubbled)
    }

    /// Whether the theme's buttons are framed whole, icon and label (square bubbles), rather than
    /// an orb round the icon or a flat fill.
    pub fn tiles(&self) -> bool {
        self.borders.glass.is_some_and(|glass| glass.bubbles == Some(BubbleShape::Square))
    }

    /// One segment of a joined group of buttons (the window's caption buttons) in a theme with
    /// square bubbles: a tile with the `corner`s the group gives it, so the segments meet in one
    /// strip. Returns whether it painted; otherwise the caller uses `bubble` or its own fill.
    pub fn tile_segment(&self, painter: &egui::Painter, rect: egui::Rect, corner: egui::CornerRadius, tint: Color32, state: Bubble) -> bool {
        match self.borders.glass.filter(|glass| glass.bubbles == Some(BubbleShape::Square)) {
            Some(glass) => {
                glass.paint_tile(painter, rect, corner, tint, state);
                true
            }
            None => false,
        }
    }
}

/// Read a theme file's `borders.glass`: `sheen` (a colour, required), `sheen_height` (0 to 128
/// points, required) and `bubbles` (`"round"`, `"square"`, or `false` or absent for none).
pub(crate) fn parse(obj: &Map<String, Value>) -> Result<Glass, String> {
    if obj.keys().any(|k| !matches!(k.as_str(), "sheen" | "sheen_height" | "bubbles")) {
        return Err("`borders.glass` has an unknown setting".into());
    }
    let sheen = obj
        .get("sheen")
        .and_then(Value::as_str)
        .and_then(crate::theme_files::parse_color)
        .ok_or("`borders.glass.sheen` must be #rrggbb or #rrggbbaa")?;
    let sheen_height = obj
        .get("sheen_height")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && (0.0..=MAX_SHEEN_HEIGHT).contains(v))
        .ok_or("`borders.glass.sheen_height` must be a finite number from 0 to 128")? as f32;
    let bubbles = match obj.get("bubbles") {
        None | Some(Value::Bool(false)) => None,
        Some(Value::String(shape)) if shape == "round" => Some(BubbleShape::Round),
        Some(Value::String(shape)) if shape == "square" => Some(BubbleShape::Square),
        Some(_) => return Err("`borders.glass.bubbles` must be false, \"round\" or \"square\"".into()),
    };
    Ok(Glass { sheen, sheen_height, bubbles })
}

/// A glass finish as a theme file's `borders.glass`, which `parse` reads back unchanged.
pub(crate) fn export(glass: Glass) -> Value {
    json!({
        "sheen": crate::theme_files::format_color(glass.sheen),
        "sheen_height": glass.sheen_height,
        "bubbles": glass.bubbles.map_or(json!(false), |shape| json!(shape.id())),
    })
}

/// A convex polygon cut to one side of a vertical line (`keep_right`: keep `x >= at`).
fn clip_x(points: &[egui::Pos2], at: f32, keep_right: bool) -> Vec<egui::Pos2> {
    let inside = |p: egui::Pos2| if keep_right { p.x >= at } else { p.x <= at };
    let mut out = Vec::with_capacity(points.len() + 2);
    for (i, p) in points.iter().enumerate() {
        let Some(q) = points.get((i + 1) % points.len()) else { continue };
        if inside(*p) {
            out.push(*p);
        }
        if inside(*p) != inside(*q) && (q.x - p.x).abs() > f32::EPSILON {
            let t = (at - p.x) / (q.x - p.x);
            out.push(egui::pos2(at, p.y + (q.y - p.y) * t));
        }
    }
    out
}

/// Points round a rounded rectangle, for the gradients.
fn outline(rect: egui::Rect, corner: egui::CornerRadius) -> Vec<egui::Pos2> {
    let mut path = Vec::new();
    egui::epaint::tessellator::path::rounded_rectangle(&mut path, egui::epaint::RoundedRect::new(rect, corner));
    path
}

/// Corners `by` points tighter, for an outline inside another.
fn shrunk(corner: egui::CornerRadius, by: u8) -> egui::CornerRadius {
    egui::CornerRadius {
        nw: corner.nw.saturating_sub(by),
        ne: corner.ne.saturating_sub(by),
        sw: corner.sw.saturating_sub(by),
        se: corner.se.saturating_sub(by),
    }
}

/// Points round an ellipse, for the bubbles' gradients.
fn ellipse(center: egui::Pos2, radii: egui::Vec2) -> Vec<egui::Pos2> {
    const STEPS: usize = 40;
    (0..STEPS)
        .map(|i| {
            let a = std::f32::consts::TAU * i as f32 / STEPS as f32;
            egui::pos2(center.x + radii.x * a.cos(), center.y + radii.y * a.sin())
        })
        .collect()
}

/// A convex outline filled as a fan from `hub`, each outline point coloured by `color`.
fn fan(painter: &egui::Painter, hub: egui::Pos2, hub_color: Color32, outline: Vec<egui::Pos2>, color: impl Fn(egui::Pos2) -> Color32) {
    let Ok(count) = u32::try_from(outline.len()) else { return };
    if count < 3 {
        return;
    }
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(hub, hub_color);
    for point in outline {
        mesh.colored_vertex(point, color(point));
    }
    for i in 0..count {
        mesh.add_triangle(0, i + 1, (i + 1) % count + 1);
    }
    painter.add(mesh);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_glass_finish_reads_back_what_it_exports_and_refuses_bad_settings() {
        let glass = Glass { sheen: Color32::from_white_alpha(128), sheen_height: 30.0, bubbles: Some(BubbleShape::Square) };
        let exported = export(glass);
        assert_eq!(parse(exported.as_object().unwrap()), Ok(glass));
        for (key, value) in
            [("sheen", json!("white")), ("sheen_height", json!(-1)), ("sheen_height", json!(500)), ("bubbles", json!(true)), ("extra", json!(1))]
        {
            let mut bad = exported.clone();
            bad[key] = value;
            assert!(parse(bad.as_object().unwrap()).is_err(), "{key}");
        }
        let mut none = exported.as_object().unwrap().clone();
        none.remove("bubbles");
        assert_eq!(parse(&none).map(|g| g.bubbles), Ok(None));
    }

    #[test]
    fn only_see_through_surfaces_catch_the_light() {
        let ctx = egui::Context::default();
        let painter = egui::Painter::new(ctx, egui::LayerId::background(), egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)));
        let glass = Glass { sheen: Color32::from_white_alpha(166), sheen_height: 40.0, bubbles: None };
        let content = egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(300.0, 200.0));
        for (fill, lit) in
            [(Color32::from_rgba_unmultiplied(200, 220, 240, 170), true), (Color32::from_rgb(200, 220, 240), false), (Color32::TRANSPARENT, false)]
        {
            let frame = egui::Frame::NONE.fill(fill);
            let backed = glass.backing(&painter, frame, content, egui::Shape::Noop);
            assert_eq!(matches!(backed, egui::Shape::Vec(_)), lit, "{fill:?}");
        }
    }
}
