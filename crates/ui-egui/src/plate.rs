//! A theme's armour plates (`borders.plate` in a theme file), for a mechanical, heads-up-display
//! look: command, navigation and title bar buttons are plates with some corners cut away at 45
//! degrees, in one of a few shapes picked by where each sits so neighbours differ; highlighted rows
//! are lit bars with cut corners; and panes, windows and cards have their rounded corners cut the
//! same way. A plate is a light outline at rest and lights up in the accent under the pointer or
//! pressed; `floating` adds a detached strip under it and lifts its cut corner off as its own
//! piece. Themes without `borders.plate` paint exactly as before.

use std::cell::RefCell;

use egui::{Color32, Stroke};
use serde_json::{Map, Value, json};

use crate::theme::Tokens;

/// What a plate shows of its button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lit {
    Rest,
    Hover,
    /// Pressed, or on (a running command, a selected tool).
    On,
    Disabled,
}

impl Lit {
    /// A button's state: disabled, on (pressed, or `on`: running, selected, open), under the
    /// pointer or focused, or at rest.
    pub fn of(response: &egui::Response, enabled: bool, on: bool) -> Self {
        if !enabled {
            Lit::Disabled
        } else if on || response.is_pointer_button_down_on() {
            Lit::On
        } else if response.hovered() || response.has_focus() {
            Lit::Hover
        } else {
            Lit::Rest
        }
    }
}

/// A theme's plates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plate {
    /// How far a corner is cut back, in points (0 to 16); smaller on small buttons.
    pub cut: f32,
    pub floating: bool,
}

/// The plate shapes: how much of each corner (top left, top right, bottom right, bottom left) is
/// cut, as a share of the plate's cut.
const SHAPES: [[f32; 4]; 4] = [[1.0, 0.0, 1.0, 0.0], [0.0, 1.0, 0.0, 1.0], [1.0, 0.4, 0.0, 0.4], [0.4, 0.0, 1.0, 0.0]];
/// The largest cut a theme file may ask for, in points.
const MAX_CUT: f64 = 16.0;
/// At most this many panes are kept in a frame; more keep their own corners.
const MAX_PANES: usize = 4096;

thread_local! {
    /// The frame's panes, windows and cards in a plated theme ([`pane`]), by layer, to be cut
    /// ([`cut_panes`]).
    static PANES: RefCell<Vec<(egui::LayerId, egui::Rect)>> = const { RefCell::new(Vec::new()) };
}

impl Plate {
    /// The plate of a button filling `rect`, in `tint`, behind the icon and label the caller
    /// draws next. Paint only: no layout or input target. Too small or non-finite: nothing.
    pub fn paint(self, painter: &egui::Painter, rect: egui::Rect, tint: Color32, state: Lit) {
        if !rect.is_finite() || rect.width() < 8.0 || rect.height() < 8.0 {
            return;
        }
        let lit = matches!(state, Lit::Hover | Lit::On);
        // The body's tint, its edge's and the details'; at rest an outline without a face.
        let (body, edge, parts) = match state {
            Lit::Rest => (0.0, 0.5, 0.35),
            Lit::Hover => (0.18, 1.0, 0.8),
            Lit::On => (0.32, 1.0, 1.0),
            Lit::Disabled => (0.0, 0.18, 0.1),
        };
        let floating = self.floating && rect.height() >= 20.0;
        let (gap, strip) = (2.0, 2.5);
        let mut face = rect;
        if floating {
            face.max.y -= gap + strip;
        }
        let cut = self.cut.clamp(0.0, MAX_CUT as f32).min(face.width() * 0.3).min(face.height() * 0.3);
        let shape = SHAPES.get(shape_at(rect.min)).copied().unwrap_or([1.0, 0.0, 1.0, 0.0]);
        let cuts = shape.map(|share| (share * cut).round());
        let [nw, ne, se, sw] = cuts;
        let (l, t, r, b) = (face.left(), face.top(), face.right(), face.bottom());
        let (w, h) = (face.width(), face.height());
        let outline = cut_outline(face, cuts);
        if body > 0.0 {
            // The face, lit from the top and deeper towards it, like a display behind glass.
            let glow = |p: egui::Pos2| tint.gamma_multiply(body * (1.6 - 1.2 * ((p.y - t) / h).clamp(0.0, 1.0)));
            fan(painter, face.center(), tint.gamma_multiply(body), outline.clone(), glow);
        }
        painter.add(egui::Shape::closed_line(outline, Stroke::new(1.0, tint.gamma_multiply(edge))));
        // The details go by the more cut of the two top corners, mirrored when it is the right.
        let (corner, right) = if ne > nw { (ne, true) } else { (nw, false) };
        let x = |from_side: f32| if right { r - from_side } else { l + from_side };
        let detailed = w >= 24.0 && h >= 18.0;
        if detailed {
            // A heavier bar along the top edge, away from the cut corner.
            let far = if right { nw } else { ne };
            painter
                .line_segment([egui::pos2(x(w * 0.65), t + 1.0), egui::pos2(x(w - 1.0 - far), t + 1.0)], Stroke::new(2.0, tint.gamma_multiply(edge)));
        }
        if detailed && lit && corner > 0.0 {
            // A second line inside the cut corner and along part of the top and side.
            let (d, k) = (3.0, 3.0 * (std::f32::consts::SQRT_2 - 1.0));
            let inner = vec![
                egui::pos2(x(d), t + h * 0.55),
                egui::pos2(x(d), t + corner + k),
                egui::pos2(x(corner + k), t + d),
                egui::pos2(x(w * 0.5), t + d),
            ];
            painter.add(egui::Shape::line(inner, Stroke::new(1.0, tint.gamma_multiply(edge * 0.45))));
        }
        if detailed && lit {
            // Hatching low on the side away from the details' corner.
            let start = if right { l + sw + 4.0 } else { r - se - 16.0 };
            for i in 0..3 {
                let hx = start + i as f32 * 4.0;
                painter.line_segment([egui::pos2(hx, b - 3.0), egui::pos2(hx + 3.0, b - 7.0)], Stroke::new(1.5, tint.gamma_multiply(parts)));
            }
        }
        if !floating {
            return;
        }
        // The strip under half the plate, its ends slanted like the cuts.
        let (top, bottom) = (b + gap, rect.bottom());
        let (from, to) = if right { (l + sw, l + w * 0.55) } else { (l + w * 0.45, r - se) };
        if to - from > strip * 2.0 {
            let slant = bottom - top;
            let strip = vec![egui::pos2(from + slant, top), egui::pos2(to, top), egui::pos2(to - slant, bottom), egui::pos2(from, bottom)];
            painter.add(egui::Shape::convex_polygon(strip, tint.gamma_multiply(parts), Stroke::NONE));
        }
        // The cut corner, lifted off as its own piece while the plate is lit.
        let piece = corner - gap * 1.5;
        if lit && piece >= 2.0 {
            let piece = vec![egui::pos2(x(0.0), t), egui::pos2(x(piece), t), egui::pos2(x(0.0), t + piece)];
            painter.add(egui::Shape::convex_polygon(piece, tint.gamma_multiply(parts), Stroke::NONE));
        }
    }

    /// A highlighted row (under the pointer, selected, open) as a lit bar: its face in `fill`
    /// fading to the right, its top left corner cut a little and its bottom right more, an edge
    /// and a bar down its left in `accent`, and with `floating` the bottom right corner lifted
    /// off as its own piece. Too small or non-finite: a flat `fill`.
    pub fn paint_row(self, painter: &egui::Painter, rect: egui::Rect, fill: Color32, accent: Color32) {
        if !rect.is_finite() || rect.width() < 24.0 || rect.height() < 10.0 {
            painter.rect_filled(rect, 0.0, fill);
            return;
        }
        let (l, t, r, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
        let cut = self.cut.clamp(0.0, MAX_CUT as f32).min(rect.height() * 0.4).round();
        let small = (cut * 0.5).round();
        let outline = cut_outline(rect, [small, 0.0, cut, 0.0]);
        let across = |p: egui::Pos2| {
            let along = ((p.x - l) / rect.width()).clamp(0.0, 1.0);
            fill.lerp_to_gamma(accent, 0.18 * (1.0 - along)).gamma_multiply(1.0 - 0.55 * along)
        };
        fan(painter, rect.center(), across(rect.center()), outline.clone(), across);
        painter.add(egui::Shape::closed_line(outline, Stroke::new(1.0, accent.gamma_multiply(0.6))));
        let bar = vec![
            egui::pos2(l + 1.0, t + small + 1.0),
            egui::pos2(l + 4.0, t + small + 4.0),
            egui::pos2(l + 4.0, b - 2.0),
            egui::pos2(l + 1.0, b - 2.0),
        ];
        painter.add(egui::Shape::convex_polygon(bar, accent, Stroke::NONE));
        let piece = cut - 3.0;
        if self.floating && piece >= 2.0 {
            let piece = vec![egui::pos2(r, b - piece), egui::pos2(r, b), egui::pos2(r - piece, b)];
            painter.add(egui::Shape::convex_polygon(piece, accent.gamma_multiply(0.8), Stroke::NONE));
        }
    }
}

impl Tokens {
    /// A button's plate in a plated theme, filling `rect` (its icon and label) behind what the
    /// caller draws next. Returns whether it painted; otherwise the button keeps its own fill.
    pub fn plate_button(&self, painter: &egui::Painter, rect: egui::Rect, tint: Color32, state: Lit) -> bool {
        match self.borders.plate {
            Some(plate) => {
                plate.paint(painter, rect, tint, state);
                true
            }
            None => false,
        }
    }

    /// A highlighted row or item (under the pointer, selected, open): a flat `fill`, or in a plated
    /// theme a lit bar with cut corners (`Plate::paint_row`). Hover and selection fills of menus,
    /// trees, tables and the history paint through this, so a theme can restyle all of them.
    pub fn paint_highlight(&self, painter: &egui::Painter, rect: egui::Rect, radius: impl Into<egui::CornerRadius>, fill: Color32) {
        match self.borders.plate {
            Some(plate) => plate.paint_row(painter, rect, fill, self.accent),
            None => {
                painter.rect_filled(rect, radius, fill);
            }
        }
    }

    /// The tint of a button on the application bar: the accent, or in a plated theme the accent
    /// lightened or darkened towards the bar's text, so its plates stand out on the bar.
    pub fn app_bar_tint(&self) -> Color32 {
        if self.borders.plate.is_some() { self.accent.lerp_to_gamma(self.app_bar_text, 0.45) } else { self.accent }
    }
}

/// Note that a pane, window or card's frame fills `rect` in `painter`'s layer, so a plated theme
/// cuts its corners once the frame is drawn ([`cut_panes`]). Other themes: nothing.
pub fn pane(painter: &egui::Painter, rect: egui::Rect) {
    if Tokens::get().borders.plate.is_some() && rect.is_finite() && rect.is_positive() {
        PANES.with_borrow_mut(|panes| {
            if panes.len() < MAX_PANES {
                panes.push((painter.layer_id(), rect));
            }
        });
    }
}

/// After a frame is drawn, in a plated theme: each frame noted by [`pane`] has its rounded corners
/// cut back at 45 degrees instead, in one of the plate shapes, and a heavier bar in the accent
/// along its top. Corners the frame keeps square (joined to a dock edge or a neighbour) stay
/// square. Forgets the frame's panes, so each frame starts afresh.
pub fn cut_panes(ctx: &egui::Context) {
    let panes = PANES.take();
    let t = Tokens::get();
    let Some(plate) = t.borders.plate else { return };
    if panes.is_empty() {
        return;
    }
    let mut layers: Vec<egui::LayerId> = Vec::new();
    for (layer, _) in &panes {
        if !layers.contains(layer) {
            layers.push(*layer);
        }
    }
    ctx.graphics_mut(|graphics| {
        for layer in layers {
            let frames: Vec<egui::Rect> = panes.iter().filter(|(l, _)| *l == layer).map(|(_, rect)| *rect).collect();
            let Some(list) = graphics.get_mut(layer) else { continue };
            let count = list.all_entries().len();
            for i in 0..count {
                list.mutate_shape(egui::layers::ShapeIdx(i), |clipped| cut_frame(&mut clipped.shape, &frames, plate, t.accent));
            }
        }
    });
}

/// `shape`'s frames that fill one of `frames`, cut ([`cut_panes`]).
fn cut_frame(shape: &mut egui::Shape, frames: &[egui::Rect], plate: Plate, accent: Color32) {
    match shape {
        egui::Shape::Vec(shapes) => shapes.iter_mut().for_each(|s| cut_frame(s, frames, plate, accent)),
        egui::Shape::Rect(frame)
            if frame.blur_width <= 0.0
                && frames.iter().any(|f| (f.min - frame.rect.min).length() < 0.5 && (f.max - frame.rect.max).length() < 0.5) =>
        {
            let rect = frame.rect;
            let radius = frame.corner_radius;
            let cut = (plate.cut * 1.6).clamp(0.0, 24.0).min(rect.width() * 0.2).min(rect.height() * 0.2);
            let shares = SHAPES.get(shape_at(rect.min)).copied().unwrap_or([1.0, 0.0, 1.0, 0.0]);
            // Only a rounded corner is free to cut; a square one is joined to something.
            let free = [radius.nw, radius.ne, radius.se, radius.sw].map(|r| r > 0);
            let mut cuts = [0.0; 4];
            for ((cut_to, share), free) in cuts.iter_mut().zip(shares).zip(free) {
                *cut_to = if free { (share * cut).round() } else { 0.0 };
            }
            if cuts.iter().all(|c| *c <= 0.0) {
                return;
            }
            let width = frame.stroke.width;
            let line = match frame.stroke_kind {
                egui::StrokeKind::Inside => rect.shrink(width * 0.5),
                egui::StrokeKind::Outside => rect.expand(width * 0.5),
                egui::StrokeKind::Middle => rect,
            };
            let mut shapes = Vec::with_capacity(3);
            if frame.fill.a() > 0 {
                shapes.push(egui::Shape::convex_polygon(cut_outline(rect, cuts), frame.fill, Stroke::NONE));
            }
            if width > 0.0 && frame.stroke.color.a() > 0 {
                shapes.push(egui::Shape::closed_line(cut_outline(line, cuts), frame.stroke));
            }
            // A heavier bar along the top, from beside the top left corner, on panes big enough.
            if rect.width() >= 120.0 && rect.height() >= 60.0 {
                let [nw, ne, ..] = cuts;
                let from = rect.left() + nw + 6.0;
                let to = (from + rect.width() * 0.3).min(rect.right() - ne - 6.0);
                let y = line.top() + 1.0;
                shapes.push(egui::Shape::convex_polygon(
                    vec![egui::pos2(from, y - 1.0), egui::pos2(to, y - 1.0), egui::pos2(to - 2.0, y + 1.0), egui::pos2(from + 2.0, y + 1.0)],
                    accent.gamma_multiply(0.85),
                    Stroke::NONE,
                ));
            }
            *shape = egui::Shape::Vec(shapes);
        }
        _ => {}
    }
}

/// Which of the [`SHAPES`] a plate or pane at `corner` takes: the same one wherever it is drawn
/// from, and mostly different from its neighbours'.
fn shape_at(corner: egui::Pos2) -> usize {
    if !corner.is_finite() {
        return 0;
    }
    let (x, y) = ((corner.x / 4.0).round() as i32 as u32, (corner.y / 4.0).round() as i32 as u32);
    ((x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(2_246_822_519)) >> 28) as usize % SHAPES.len()
}

/// The outline of `rect` with its corners (top left, top right, bottom right, bottom left) cut
/// back by `cuts` at 45 degrees, clockwise from the top left; a 0 leaves a corner square.
fn cut_outline(rect: egui::Rect, cuts: [f32; 4]) -> Vec<egui::Pos2> {
    let (l, t, r, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let [nw, ne, se, sw] = cuts.map(|c| if c.is_finite() { c.max(0.0) } else { 0.0 });
    let mut points = Vec::with_capacity(8);
    let mut corner = |c: f32, square: egui::Pos2, before: egui::Pos2, after: egui::Pos2| {
        if c > 0.0 {
            points.extend([before, after]);
        } else {
            points.push(square);
        }
    };
    corner(nw, egui::pos2(l, t), egui::pos2(l, t + nw), egui::pos2(l + nw, t));
    corner(ne, egui::pos2(r, t), egui::pos2(r - ne, t), egui::pos2(r, t + ne));
    corner(se, egui::pos2(r, b), egui::pos2(r, b - se), egui::pos2(r - se, b));
    corner(sw, egui::pos2(l, b), egui::pos2(l + sw, b), egui::pos2(l, b - sw));
    points
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

/// Read a theme file's `borders.plate`: `cut` (0 to 16 points, required) and `floating`
/// (`true` or `false`, absent for false).
pub(crate) fn parse(obj: &Map<String, Value>) -> Result<Plate, String> {
    if obj.keys().any(|k| !matches!(k.as_str(), "cut" | "floating")) {
        return Err("`borders.plate` has an unknown setting".into());
    }
    let cut = obj
        .get("cut")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && (0.0..=MAX_CUT).contains(v))
        .ok_or("`borders.plate.cut` must be a finite number from 0 to 16")? as f32;
    let floating = match obj.get("floating") {
        None => false,
        Some(Value::Bool(floating)) => *floating,
        Some(_) => return Err("`borders.plate.floating` must be true or false".into()),
    };
    Ok(Plate { cut, floating })
}

/// Plates as a theme file's `borders.plate`, which `parse` reads back unchanged.
pub(crate) fn export(plate: Plate) -> Value {
    json!({"cut": plate.cut, "floating": plate.floating})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plates_read_back_what_they_export_and_refuse_bad_settings() {
        let plate = Plate { cut: 7.0, floating: true };
        let exported = export(plate);
        assert_eq!(parse(exported.as_object().unwrap()), Ok(plate));
        for (key, value) in [("cut", json!(-1)), ("cut", json!(40)), ("cut", json!("big")), ("floating", json!("yes")), ("extra", json!(1))] {
            let mut bad = exported.clone();
            bad[key] = value;
            assert!(parse(bad.as_object().unwrap()).is_err(), "{key}");
        }
    }

    #[test]
    fn a_cut_outline_keeps_square_corners_square_and_cuts_the_others_at_45_degrees() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 40.0));
        assert_eq!(cut_outline(rect, [0.0; 4]).len(), 4);
        let cut = cut_outline(rect, [6.0, 0.0, 6.0, 0.0]);
        assert_eq!(
            cut,
            vec![
                egui::pos2(0.0, 6.0),
                egui::pos2(6.0, 0.0),
                egui::pos2(100.0, 0.0),
                egui::pos2(100.0, 34.0),
                egui::pos2(94.0, 40.0),
                egui::pos2(0.0, 40.0)
            ]
        );
        assert_eq!(cut_outline(rect, [f32::NAN, -3.0, 0.0, 0.0]).len(), 4, "hostile cuts leave corners square");
    }

    #[test]
    fn a_plate_keeps_its_shape_and_neighbours_mostly_differ() {
        let at = |x: f32| shape_at(egui::pos2(x, 40.0));
        assert_eq!(at(100.0), at(100.0));
        let shapes: std::collections::BTreeSet<usize> = (0..12).map(|i| at(10.0 + i as f32 * 58.0)).collect();
        assert!(shapes.len() >= 2, "a row of buttons takes more than one shape");
        assert_eq!(shape_at(egui::pos2(f32::NAN, 0.0)), 0);
    }
}
