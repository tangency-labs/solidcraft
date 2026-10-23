//! The handles' appearance and nothing else: colours, line widths, sizes, easing and the shapes
//! they are drawn as. A restyle touches only this file; `place_gizmo.rs` decides where handles
//! are and what a drag does.

use egui::{Color32, Pos2, Stroke, vec2};

use super::REACH;
use crate::theme::Tokens;

/// Radius of a drawn ball handle (pixels).
const DOT_PX: f32 = 6.0;
/// The handles are near-white, so the blue preview they sit on does not hide them; `EDGE` is the
/// darker casing drawn under every line and around every ball, so they also carry on a light body.
/// Hot (under the pointer or dragged) the core goes to `HOT`. Neutral in every theme and colour
/// vision mode: they carry no meaning, only contrast (listed in `colour_tokens_tests`).
pub(super) const HANDLE: Color32 = Color32::from_rgb(237, 240, 245);
const EDGE: Color32 = Color32::from_rgb(60, 68, 84);
pub(super) const HOT: Color32 = Color32::WHITE;

/// A line in `core`, cased in `EDGE` under it so it reads on the preview and on the body alike.
pub(super) fn cased_line(painter: &egui::Painter, a: Pos2, b: Pos2, w: f32, core: Color32) {
    painter.line_segment([a, b], Stroke::new(w + 2.5, EDGE));
    painter.line_segment([a, b], Stroke::new(w, core));
}

/// A closed outline in `core`, cased in `EDGE` under it.
pub(super) fn cased_closed(painter: &egui::Painter, pts: &[Pos2], w: f32, core: Color32) {
    for s in [Stroke::new(w + 2.0, EDGE), Stroke::new(w, core)] {
        painter.add(egui::Shape::closed_line(pts.to_vec(), s));
    }
}

/// A ball handle: near-white with the contrast edge. Hot (under the pointer or dragged) it
/// brightens to pure white, grows a little, gets a soft halo and an accent edge to say it is live.
pub(super) fn ball(painter: &egui::Painter, c: Pos2, hot: bool, t: &Tokens) {
    if hot {
        painter.circle_filled(c, REACH, t.accent.gamma_multiply(0.14));
    }
    let r = if hot { DOT_PX + 1.5 } else { DOT_PX };
    let fill = if hot { HOT } else { HANDLE };
    let edge = if hot { t.accent } else { EDGE };
    painter.circle(c, r, fill, Stroke::new(1.5, edge));
}

/// The move marker's look for a frame, interpolated between its three states so hover and drag
/// ease in and out rather than snap. `hot` eases 0 (resting) to 1 (pointer over or dragging);
/// `active` eases 0 to 1 while dragging.
pub(super) struct CrossStyle {
    pub(super) hot: f32,
    pub(super) active: f32,
}

/// The move marker: a small dot with four lines to the screen cardinals, a gap between the dot
/// and each line, a common way to mark a point you can drag. Near-white and cased in the contrast
/// edge, so it reads on the feature's blue preview and on a light body alike. Three states, eased:
/// resting (quiet, short dashes), hover (brighter, longer dashes) and active while dragging (solid
/// lines, a bigger dot with an accent edge and a soft halo, so it stays legible over the moving
/// ghost). The dash-to-solid switch is the one step that cannot tween, so it flips at drag start.
pub(super) fn crosshair(painter: &egui::Painter, c: Pos2, s: &CrossStyle, t: &Tokens) {
    let reach = 15.0 + 7.0 * s.hot + 2.0 * s.active;
    let gap = 7.0;
    let core = HANDLE.lerp_to_gamma(HOT, s.hot);
    let solid = s.active > 0.5;
    if s.active > 0.01 {
        painter.circle_filled(c, REACH + 4.0, t.accent.gamma_multiply(0.16 * s.active));
    }
    for d in [vec2(1.0, 0.0), vec2(-1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, -1.0)] {
        let (a, b) = (c + d * gap, c + d * reach);
        if solid {
            cased_line(painter, a, b, 1.8, core);
        } else {
            painter.add(egui::Shape::dashed_line(&[a, b], Stroke::new(3.5, EDGE), 3.0, 3.0));
            painter.add(egui::Shape::dashed_line(&[a, b], Stroke::new(1.5, core), 3.0, 3.0));
        }
    }
    let r = 3.0 + 0.5 * s.hot + 1.5 * s.active;
    let edge = EDGE.lerp_to_gamma(t.accent, s.active);
    painter.circle(c, r, core, Stroke::new(1.2 + 0.4 * s.active, edge));
}

/// The turn knob's angle readout beside the knob at `at`.
pub(super) fn angle_label(painter: &egui::Painter, at: Pos2, angle: f64, t: &Tokens) {
    let text = format!("{:.0}°", angle.to_degrees());
    painter.text(at + vec2(12.0, -12.0), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(t.font_body), t.text);
}
