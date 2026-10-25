//! The handles' appearance and nothing else: colours, line widths, sizes, easing and the shapes
//! they are drawn as. A restyle touches only this file; `place_gizmo.rs` decides where handles
//! are and what a drag does.
//!
//! The rules the handles are drawn by (worked out in a browser prototype first, see the notes on
//! #4384): white with a dark casing marks what can be grabbed, and only handles are drawn so; a
//! dotted line is a proposal (what a drag would do), a solid line a drag under way; at rest a handle
//! shows only enough to be noticed and pointing at it shows the whole action; the shape of a tip
//! names the motion (an arrowhead for a straight move). The sizes below are the prototype's, in
//! screen points.

use egui::{Color32, Pos2, Stroke, Vec2, vec2};

use super::REACH;
use crate::theme::Tokens;

/// Radius of a drawn ball handle (pixels).
const DOT_PX: f32 = 6.0;

/// The handles' core: the icon highlight (white in every theme), so the blue preview they sit on
/// does not hide them. They carry no meaning, only contrast, so it is the same hot or not.
pub(super) fn core(_hot: bool) -> Color32 {
    Tokens::get().icons.highlight
}

/// The casing drawn under every line and around every dot: the marker halo (dark, translucent,
/// per theme), so the white core also carries on a light body.
fn casing() -> Color32 {
    Tokens::get().icons.marker_halo
}

/// How much wider than its core a casing is (both sides together).
const CASE: f32 = 1.6;

/// A line in `core`, cased under it so it reads on the preview and on the body alike.
pub(super) fn cased_line(painter: &egui::Painter, a: Pos2, b: Pos2, w: f32, core: Color32) {
    painter.line_segment([a, b], Stroke::new(w + CASE, casing()));
    painter.line_segment([a, b], Stroke::new(w, core));
}

/// A closed outline in `core`, cased under it.
pub(super) fn cased_closed(painter: &egui::Painter, pts: &[Pos2], w: f32, core: Color32) {
    for s in [Stroke::new(w + CASE, casing()), Stroke::new(w, core)] {
        painter.add(egui::Shape::closed_line(pts.to_vec(), s));
    }
}

/// A ball handle: white with the casing. Hot (under the pointer or dragged) it grows a little,
/// gets a soft halo and an accent edge to say it is live.
pub(super) fn ball(painter: &egui::Painter, c: Pos2, hot: bool, t: &Tokens) {
    if hot {
        painter.circle_filled(c, REACH, t.accent.gamma_multiply(0.14));
    }
    let r = if hot { DOT_PX + 1.5 } else { DOT_PX };
    let edge = if hot { t.accent } else { casing() };
    painter.circle(c, r, core(hot), Stroke::new(1.5, edge));
}

/// The move marker's look for a frame, interpolated between its three states so hover and drag
/// grow in and out rather than snap. `hot` eases 0 (resting) to 1 (pointer over or dragging);
/// `active` eases 0 to 1 while dragging.
pub(super) struct CrossStyle {
    pub(super) hot: f32,
    pub(super) active: f32,
}

/// How long the marker's states take to grow into each other (seconds).
pub(super) const GROW_S: f32 = 0.36;
/// The marker's line width.
const LINE: f32 = 2.0;
/// The dot's radius at rest and while dragging.
const DOT_REST: f32 = 3.9;
const DOT_DRAG: f32 = 4.3;
/// Where an arm starts, from the centre.
const ARM_FROM: f32 = 6.0;
/// How far an arm reaches at rest, pointed at and while dragging (the arrowhead's tip).
const REACH_REST: f32 = 27.0;
const REACH_HOT: f32 = 34.0;
const REACH_DRAG: f32 = 42.0;
/// A dotted arm's dash and gap.
const DASH: f32 = 5.5;
const GAP: f32 = 2.3;
/// What the marker needs around its centre when fully grown, plus a gap, so another manipulator
/// (the dialog's drag arrow) can keep clear of it.
pub(super) const MARKER_CLEAR: f32 = REACH_DRAG + 6.0;
/// The arrowhead: an open chevron this long, this wide either side of the arm.
const HEAD_LEN: f32 = 12.6;
const HEAD_HALF: f32 = 7.3;

/// The move marker: a dot with four arms to the screen cardinals. At rest the arms are short and
/// dotted, a place that can be grabbed and nothing more. Pointed at, they grow and end in
/// arrowheads: the feature moves straight in any direction along the face. While it drags, the
/// arms turn solid. Each state grows out of the previous one (`GROW_S`); the dotted-to-solid
/// switch is the one step that cannot tween, so it flips halfway into the drag.
pub(super) fn crosshair(painter: &egui::Painter, c: Pos2, s: &CrossStyle, _t: &Tokens) {
    let reach = REACH_REST + (REACH_HOT - REACH_REST) * s.hot + (REACH_DRAG - REACH_HOT) * s.active;
    let solid = s.active > 0.5;
    let (core, case) = (core(s.hot > 0.5), casing());
    let (core_w, case_w) = (LINE, LINE + CASE);
    // The arrowhead grows with the arm it ends: scaled and faded in by `hot`.
    let head = (HEAD_LEN * s.hot, HEAD_HALF * s.hot, s.hot);
    let mut shapes: Vec<(Vec<Pos2>, f32, Color32)> = Vec::new();
    for d in [vec2(1.0, 0.0), vec2(-1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, -1.0)] {
        let tip = c + d * reach;
        // The shaft: solid to the tip, or dotted up to the head's base.
        let to = if solid { reach } else { reach - head.0 };
        if solid {
            shapes.push((vec![c + d * ARM_FROM, tip], 1.0, core));
        } else {
            let mut at = ARM_FROM;
            while at < to {
                let end = (at + DASH).min(to);
                shapes.push((vec![c + d * at, c + d * end], 1.0, core));
                at += DASH + GAP;
            }
        }
        if head.2 > 0.01 {
            let n: Vec2 = vec2(-d.y, d.x);
            let base = tip - d * head.0;
            shapes.push((vec![base + n * head.1, tip, base - n * head.1], head.2, core));
        }
    }
    // Casings first, then every core over them, so a core never sits under a neighbour's casing.
    for (pts, alpha, _) in &shapes {
        painter.add(egui::Shape::line(pts.clone(), Stroke::new(case_w, case.gamma_multiply(*alpha))));
    }
    for (pts, alpha, col) in &shapes {
        painter.add(egui::Shape::line(pts.clone(), Stroke::new(core_w, col.gamma_multiply(*alpha))));
    }
    let r = DOT_REST + (DOT_DRAG - DOT_REST) * s.active;
    painter.circle_filled(c, r + CASE / 2.0, case);
    painter.circle_filled(c, r, core);
}

/// The turn knob's angle readout beside the knob at `at`.
pub(super) fn angle_label(painter: &egui::Painter, at: Pos2, angle: f64, t: &Tokens) {
    let text = format!("{:.0}°", angle.to_degrees());
    painter.text(at + vec2(12.0, -12.0), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(t.font_body), t.text);
}
