//! The view compass's axis ends: SolidCraft's [`Glyphs::axis_tip`](solidcraft_ui_kit::icons::glyphs::Glyphs::axis_tip)
//! and its pointer target.
use egui::{Pos2, Stroke, pos2};
use solidcraft_theme::Tokens;

/// The pointer target around a compass end: larger than the drawn disc, so it is easy to hit.
pub const AXIS_RADIUS: f32 = 15.0;

/// A compass axis end at a uniform scale, including its lettering and strokes.
#[allow(clippy::too_many_arguments)]
pub fn axis_tip_scaled(painter: &egui::Painter, c: Pos2, name: &str, d: [f64; 3], at: Pos2, depth: f64, hovered: bool, t: Tokens, scale: f32) {
    let col = if d[0] != 0.0 {
        t.axis_x
    } else if d[1] != 0.0 {
        t.axis_y
    } else {
        t.axis_z
    };
    let positive = !name.starts_with('-');
    let light = t.panel.r() > 128;
    // Positive shafts are the full axis colour; negative ones are a dimmed tint (and any end
    // pointing away from the viewer dims further).
    let ink = if !positive || depth < -0.01 { col.gamma_multiply(0.55) } else { col };
    let radius = if positive { 11.0 } else { 4.8 } * scale;
    let direction = (at - c).normalized();
    let width = 3.6 * scale;
    // A negative shaft stops at the outer edge of its ring; a positive one runs under its disc.
    let end = if positive { at - direction * radius } else { at - direction * (radius + 1.2 * scale) };
    painter.line_segment([c, end], Stroke::new(width, ink));
    // The round cap sits at the origin; the other end is covered by the disc or ring.
    painter.circle_filled(c, width / 2.0, ink);
    if positive {
        painter.circle_filled(at, radius, col);
    }
    // Negative tips are hollow: neither the axis shaft nor a fill covers their
    // centres, so the translucent compass background shows through. A hovered tip of either
    // kind turns fully white.
    if hovered {
        // Same outer size as the resting tip: a ring's outline is half its stroke outside.
        painter.circle_filled(at, if positive { radius } else { radius + 1.2 * scale }, t.on_accent);
        // On the light theme a white disc vanishes into the compass, so it carries an outline in
        // the axis colour, drawn inside the disc so it does not grow.
        if light && positive {
            painter.circle_stroke(at, radius - scale, Stroke::new(2.0 * scale, col));
        }
    }
    if positive {
        // On a hovered (white) disc the letter takes the axis colour.
        let letter = if hovered { col } else { t.on_accent };
        let label = name.to_uppercase();
        let galley = painter.layout_no_wrap(label, solidcraft_ui_kit::type_face::bold(solidcraft_theme::Tokens::get().font_compass * scale), letter);
        let origin = at - galley.size() / 2.0;
        let ppp = painter.ctx().pixels_per_point().max(0.5);
        let origin = pos2((origin.x * ppp).round() / ppp, (origin.y * ppp).round() / ppp);
        painter.galley(origin, galley, letter);
    }
    if !positive {
        painter.circle_stroke(
            at,
            radius,
            Stroke::new(
                2.4 * scale,
                if !hovered {
                    ink
                } else if light {
                    col
                } else {
                    t.on_accent
                },
            ),
        );
    }
}
