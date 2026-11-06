//! A built-in pattern behind the model (Preferences › Appearance › Background pattern), drawn in
//! code from the 3D view's two background colours over its gradient, so it follows the theme,
//! the tab colours and the flat background colour as the gradient does. A user's background
//! picture covers it. A new pattern is one row of [`PATTERNS`] and a painter.
//!
//! The pattern sits under a soft veil of the background's colour, strongest in the middle where
//! the model is worked on, so it never competes with the model.

use egui::{Color32, Mesh, Painter, Pos2, Rect, Stroke, pos2, vec2};

/// The built-in patterns: their id (the `background_pattern` preference) and name (a message id).
pub const PATTERNS: &[(&str, &str)] = &[("aurora", "prefs.pattern_aurora")];

/// How much of the background's colour veils a pattern in the middle of the 3D view.
const VIEW_VEIL: f32 = 0.55;

/// Whether `id` is a pattern's id, or empty for none (the `background_pattern` preference).
pub fn known(id: &str) -> bool {
    id.is_empty() || PATTERNS.iter().any(|(pattern, _)| *pattern == id)
}

/// Paint the pattern `id` into `rect`, from the background's `top` and `bottom` colours, over the
/// gradient already there. Empty, unknown or a degenerate `rect`: nothing.
pub fn paint(painter: &Painter, rect: Rect, top: Color32, bottom: Color32, id: &str) {
    if !rect.is_finite() || rect.width() < 1.0 || rect.height() < 1.0 {
        return;
    }
    let Some(pattern) = pattern(id) else { return };
    pattern(painter, rect, top, bottom);
    veil(painter, rect, top.lerp_to_gamma(bottom, 0.5));
}

/// The painter of a built-in pattern ([`PATTERNS`]), by its id.
fn pattern(id: &str) -> Option<fn(&Painter, Rect, Color32, Color32)> {
    match id {
        "aurora" => Some(aurora),
        _ => None,
    }
}

/// A soft wash of `color`, strongest in the middle of `rect` and gone at its edges.
fn veil(painter: &Painter, rect: Rect, color: Color32) {
    let center = rect.center();
    fan(painter, center, color.gamma_multiply(VIEW_VEIL), &ellipse(center, rect.size() * 0.62, 48), Color32::TRANSPARENT);
}

/// Aurora's layers and their colours, made from the background's two: a deep wash across the
/// top, light rays and fine strands in near-white tints of the background, a soft glow along the
/// ribbons, a white glow where they gather, and a warm, spring-green glow low on the left.
struct Layers {
    wash: Color32,
    ray: Color32,
    strand: Color32,
    ribbon: Color32,
    glow: Color32,
    warm: Color32,
}

impl Layers {
    fn of(top: Color32, bottom: Color32) -> Self {
        Layers {
            wash: top.lerp_to_gamma(Color32::BLACK, 0.22),
            ray: top.lerp_to_gamma(Color32::WHITE, 0.85),
            strand: bottom.lerp_to_gamma(Color32::WHITE, 0.75),
            ribbon: bottom.lerp_to_gamma(Color32::WHITE, 0.45),
            glow: Color32::WHITE,
            warm: bottom.lerp_to_gamma(Color32::from_rgb(0xe8, 0xff, 0x8c), 0.45),
        }
    }
}

/// "Aurora": soft rays falling from the top, and three ribbons of fine strands sweeping up from
/// the lower left across the view in long curves, glowing where they gather, all faint, in the
/// background's own colours ([`Layers`]).
fn aurora(painter: &Painter, rect: Rect, top: Color32, bottom: Color32) {
    let (w, h) = (rect.width(), rect.height());
    let at = |x: f32, y: f32| pos2(rect.left() + w * x, rect.top() + h * y);
    let layers = Layers::of(top, bottom);
    // A deeper wash across the upper right, so the light reads against it.
    fan(painter, at(0.8, 0.0), layers.wash.gamma_multiply(0.35), &ellipse(at(0.8, 0.0), vec2(w * 0.85, h * 0.7), 48), Color32::TRANSPARENT);
    // Rays: (where across the top, how wide, how far they lean, how bright)
    for (x, width, lean, strength) in
        [(0.05, 0.08, 0.1, 0.1), (0.15, 0.03, 0.08, 0.12), (0.28, 0.1, 0.06, 0.07), (0.4, 0.04, 0.05, 0.09), (0.58, 0.08, 0.02, 0.06)]
    {
        soft_band(painter, rect, at(x, 0.0).x, w * width, w * lean, h * 0.8, layers.ray.gamma_multiply(strength));
    }
    // Glows: warm low on the left, white where the ribbons gather, and a halo round it.
    let warm = at(0.12, 0.92);
    fan(painter, warm, layers.warm.gamma_multiply(0.28), &ellipse(warm, vec2(w * 0.42, h * 0.26), 48), Color32::TRANSPARENT);
    let gather = at(0.6, 0.86);
    fan(painter, gather, layers.ribbon.gamma_multiply(0.25), &ellipse(gather, vec2(w * 0.55, h * 0.2), 48), Color32::TRANSPARENT);
    fan(painter, gather, layers.glow.gamma_multiply(0.3), &ellipse(gather, vec2(w * 0.3, h * 0.07), 48), Color32::TRANSPARENT);
    // Ribbons: (the curve's four points, how far its strands spread, how many strands)
    let ribbons: [([f32; 8], f32, usize); 3] = [
        ([-0.1, 0.98, 0.25, 0.62, 0.6, 0.95, 1.1, 0.52], 0.1, 14),
        ([-0.1, 0.72, 0.35, 0.55, 0.65, 0.88, 1.1, 0.7], 0.08, 12),
        ([-0.1, 0.88, 0.4, 0.98, 0.75, 0.6, 1.1, 0.85], 0.06, 10),
    ];
    for (points, spread, strands) in ribbons {
        let curve = |offset: f32| {
            let p = |i: usize| {
                let (x, y) = (points.get(i).copied().unwrap_or(0.0), points.get(i + 1).copied().unwrap_or(0.0));
                // The strands fan out as the ribbon sweeps right.
                at(x, y + offset * (0.3 + 0.7 * x.clamp(0.0, 1.0)))
            };
            [p(0), p(2), p(4), p(6)]
        };
        let line = |offset: f32, width: f32, color: Color32| {
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(curve(offset), false, Color32::TRANSPARENT, Stroke::new(width, color)));
        };
        line(0.0, h * 0.12, layers.ribbon.gamma_multiply(0.04));
        line(0.0, h * 0.04, layers.ribbon.gamma_multiply(0.06));
        let last = strands.saturating_sub(1).max(1) as f32;
        for i in 0..strands {
            let along = i as f32 / last - 0.5;
            line(spread * along, 1.2, layers.strand.gamma_multiply(0.05 + 0.09 * (1.0 - along.abs() * 2.0)));
        }
    }
}

/// A soft, slanted band of light from the top of `rect`, fading in across its first third, out
/// across its last, and out towards its `length` down.
fn soft_band(painter: &Painter, rect: Rect, x: f32, width: f32, lean: f32, length: f32, color: Color32) {
    let top = rect.top();
    let bottom = top + length;
    let edge = 0.35;
    for (from, to, a, b) in [(0.0, edge, 0.0, 1.0), (edge, 1.0 - edge, 1.0, 1.0), (1.0 - edge, 1.0, 1.0, 0.0)] {
        let mut mesh = Mesh::default();
        let at = |across: f32, down: f32| pos2(x + width * across + lean * down, top + (bottom - top) * down);
        mesh.colored_vertex(at(from, 0.0), color.gamma_multiply(a));
        mesh.colored_vertex(at(to, 0.0), color.gamma_multiply(b));
        mesh.colored_vertex(at(to, 1.0), Color32::TRANSPARENT);
        mesh.colored_vertex(at(from, 1.0), Color32::TRANSPARENT);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(mesh);
    }
}

fn ellipse(center: Pos2, radii: egui::Vec2, steps: usize) -> Vec<Pos2> {
    (0..steps)
        .map(|i| {
            let a = std::f32::consts::TAU * i as f32 / steps as f32;
            pos2(center.x + radii.x * a.cos(), center.y + radii.y * a.sin())
        })
        .collect()
}

/// A convex outline filled from `hub` (in `inner`) to its edge (in `outer`).
fn fan(painter: &Painter, hub: Pos2, inner: Color32, outline: &[Pos2], outer: Color32) {
    let Ok(count) = u32::try_from(outline.len()) else { return };
    if count < 3 {
        return;
    }
    let mut mesh = Mesh::default();
    mesh.colored_vertex(hub, inner);
    for p in outline {
        mesh.colored_vertex(*p, outer);
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
    fn every_pattern_draws_and_none_or_an_unknown_one_draws_nothing() {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        let painter = Painter::new(ctx.clone(), egui::LayerId::background(), rect);
        for (id, _) in PATTERNS {
            assert!(known(id));
            assert!(pattern(id).is_some(), "{id}");
        }
        assert!(known("") && !known("plaid") && pattern("").is_none());
        // Hostile sizes draw nothing rather than failing.
        for bad in [Rect::NOTHING, Rect::from_min_size(Pos2::ZERO, vec2(f32::NAN, 1.0))] {
            paint(&painter, bad, Color32::WHITE, Color32::BLACK, "aurora");
        }
    }
}
