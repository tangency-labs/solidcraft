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

use crate::theme::Tokens;

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

/// The feature's footprint on the face while the marker drags it: a filled shape in the preview's
/// own blue with a thin darker edge, so white stays reserved for the handles. It shows where the
/// feature is going before the recomputed preview gets there.
pub(super) fn footprint(painter: &egui::Painter, pts: &[Pos2], t: &Tokens) {
    for shape in crate::viewport::projected_polygon(pts, t.preview_add.gamma_multiply(0.38), Stroke::new(1.2, t.manipulator)) {
        painter.add(shape);
    }
}

/// The move marker's look for a frame, interpolated between its three states so hover and drag
/// grow in and out rather than snap. `hot` eases 0 (resting) to 1 (pointer over or dragging);
/// `active` eases 0 to 1 while dragging.
pub(super) struct CrossStyle {
    pub(super) hot: f32,
    pub(super) active: f32,
    /// 1 normally, [`DIM`] while the other handle is in use.
    pub(super) alpha: f32,
}

/// How much of itself the handle not in use keeps while the other one is dragged.
pub(super) const DIM: f32 = 0.25;
/// How long a handle takes to step back and to come forward again (seconds).
pub(super) const DIM_S: f32 = 0.16;

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
    let (core, case) = (core(s.hot > 0.5).gamma_multiply(s.alpha), casing().gamma_multiply(s.alpha));
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

/// The turn ring's look for a frame. `hot` eases 0 (resting) to 1 (pointer over the grip or
/// turning), `active` 0 to 1 while turning.
pub(super) struct RingStyle {
    pub(super) hot: f32,
    pub(super) active: f32,
    /// 1 normally, [`DIM`] while the marker is dragged.
    pub(super) alpha: f32,
}

/// The ring's size on screen (radius, pixels): fixed, like the move triad's arrows, whatever the
/// zoom or the feature. It is a control of something on the screen, not a measure of the model.
pub(super) const RING_PX: f32 = 65.0;
/// How long the ring takes to open from its grip round the circle, and to fold back (seconds).
pub(super) const SWEEP_S: f32 = 0.6;
/// Half of the arc shown at rest and while turning, either side of the grip (degrees).
const ARC_HALF_DEG: f64 = 50.0;
/// The ring's line width, its dash and gap along the arc.
const RING_LINE: f32 = 1.6;
const RING_DASH: f32 = 4.5;
const RING_GAP: f32 = 4.5;
/// The grip: a half-diamond pointing in at the centre, this long and this wide either side; at
/// rest an open chevron this far outside the ring, sliding in onto it as the ring opens.
const GRIP_LEN: f32 = 12.0;
const GRIP_HALF: f32 = 6.0;
const GRIP_OUT: f32 = 8.0;
/// Angular step of the arc's polyline (degrees).
const ARC_STEP_DEG: f64 = 1.5;

/// The turn ring: a circle lying on the face about the move marker's centre, drawn in
/// perspective. At rest a short dotted arc with the grip, enough to say that something here
/// turns; pointed at, the full dotted circle opens from the grip both ways round; while turning,
/// a solid arc around the grip. `at` gives the screen point on the circle at an angle about the
/// face normal (none when behind the eye); `grip` is the feature's turn, where the grip sits;
/// `centre` is the marker's screen point, which the grip points at.
pub(super) fn ring(painter: &egui::Painter, at: &dyn Fn(f64) -> Option<Pos2>, grip: f64, centre: Pos2, s: &RingStyle) {
    let half = ARC_HALF_DEG + (180.0 - ARC_HALF_DEG) * f64::from(s.hot) * f64::from(1.0 - s.active);
    let solid = s.active > 0.5;
    let steps = (2.0 * half / ARC_STEP_DEG).ceil().max(2.0) as usize;
    let pts: Vec<Pos2> = (0..=steps).filter_map(|i| at(grip - half.to_radians() + 2.0 * half.to_radians() * i as f64 / steps as f64)).collect();
    let (core, case) = (core(s.hot > 0.5).gamma_multiply(s.alpha), casing().gamma_multiply(s.alpha));
    let pieces: Vec<Vec<Pos2>> = if solid { vec![pts] } else { dashed(&pts, RING_DASH, RING_GAP) };
    for piece in &pieces {
        painter.add(egui::Shape::line(piece.clone(), Stroke::new(RING_LINE + CASE, case)));
    }
    for piece in &pieces {
        painter.add(egui::Shape::line(piece.clone(), Stroke::new(RING_LINE, core)));
    }
    // The grip: an open chevron outside the ring at rest, a filled half-diamond on it when hot.
    let Some(g) = at(grip) else { return };
    let inward = (centre - g).normalized();
    if !inward.is_finite() {
        return;
    }
    let n = vec2(-inward.y, inward.x);
    let tip = g - inward * (GRIP_OUT * (1.0 - s.hot));
    let (c1, c2) = (tip - inward * GRIP_LEN + n * GRIP_HALF, tip - inward * GRIP_LEN - n * GRIP_HALF);
    if s.hot > 0.01 {
        painter.add(egui::Shape::convex_polygon(vec![c1, tip, c2], core.gamma_multiply(s.hot), Stroke::NONE));
    }
    painter.add(egui::Shape::line(vec![c1, tip, c2], Stroke::new(RING_LINE + CASE, case)));
    painter.add(egui::Shape::line(vec![c1, tip, c2], Stroke::new(RING_LINE, core)));
}

/// `pts` cut into dashes of `dash` screen length with `gap` between them, measured along the line.
fn dashed(pts: &[Pos2], dash: f32, gap: f32) -> Vec<Vec<Pos2>> {
    let mut out: Vec<Vec<Pos2>> = Vec::new();
    let mut cur: Vec<Pos2> = Vec::new();
    // Where we are in the dash cycle: on for `dash`, off for `gap`.
    let (mut phase, mut on) = (0.0f32, true);
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = (b - a).length();
        let mut t0 = 0.0f32;
        while t0 < len {
            let span = (if on { dash } else { gap }) - phase;
            let t1 = (t0 + span).min(len);
            if on {
                if cur.is_empty() {
                    cur.push(a + (b - a) * (t0 / len));
                }
                cur.push(a + (b - a) * (t1 / len));
            }
            if t1 < len || (t1 - t0) >= span {
                // the piece ended inside this segment (or exactly at its end): switch
                if on && !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                on = !on;
                phase = 0.0;
            } else {
                phase += t1 - t0;
            }
            t0 = t1;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The compass: a short tick just outside the ring at each of the face's compass points, in the
/// unturned frame, so it stays put while the ring turns. Faded in by `alpha`.
pub(super) fn compass(painter: &egui::Painter, at: &dyn Fn(f64) -> Option<Pos2>, centre: Pos2, dirs: &[f64], alpha: f32) {
    let (core, case) = (core(true).gamma_multiply(alpha), casing().gamma_multiply(alpha));
    for &a in dirs {
        let Some(g) = at(a) else { continue };
        let out = (g - centre).normalized();
        if !out.is_finite() {
            continue;
        }
        let (p, q) = (g + out * TICK_FROM, g + out * (TICK_FROM + TICK_LEN));
        painter.line_segment([p, q], Stroke::new(RING_LINE + CASE, case));
        painter.line_segment([p, q], Stroke::new(RING_LINE, core));
    }
}

/// A compass tick starts this far outside the ring and is this long.
const TICK_FROM: f32 = 4.0;
const TICK_LEN: f32 = 7.0;

/// The turn ring's readout while turning, beside the grip at `at`, away from the ring's `centre`:
/// the angle, and the compass point it was pulled onto ("90° · +X").
pub(super) fn angle_label(painter: &egui::Painter, at: Pos2, centre: Pos2, angle: f64, on: Option<&str>, t: &Tokens) {
    let text = match on {
        Some(name) => format!("{:.0}° · {name}", angle.to_degrees()),
        None => format!("{:.0}°", angle.to_degrees()),
    };
    let out = (at - centre).normalized();
    let out = if out.is_finite() { out } else { vec2(1.0, 0.0) };
    let anchor = at + out * (GRIP_LEN + GRIP_OUT + 6.0);
    let align = if out.x >= 0.0 { egui::Align2::LEFT_CENTER } else { egui::Align2::RIGHT_CENTER };
    let font = egui::FontId::proportional(t.font_body);
    let galley = painter.layout_no_wrap(text, font, t.text);
    let rect = align.anchor_size(anchor, galley.size()).expand2(vec2(5.0, 2.0));
    painter.rect(rect, 4.0, t.panel.gamma_multiply(0.92), Stroke::new(1.0, t.manipulator), egui::StrokeKind::Inside);
    painter.galley(rect.shrink2(vec2(5.0, 2.0)).min, galley, t.text);
}
