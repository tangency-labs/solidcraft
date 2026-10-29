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

use crate::motion::Transition;
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
const CASE: f32 = 0.8;

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
/// How long a handle takes to step back and to come forward again, and how long the grip's axis
/// colour takes to come and go (seconds).
pub(super) const DIM_S: f32 = 0.16;

/// The marker answering the pointer arriving or leaving, and switching into and out of a drag.
pub(super) const MARKER_HOT: Transition = Transition::symmetric(0.22);
pub(super) const MARKER_ACTIVE: Transition = Transition::symmetric(0.36);
/// The marker's line width (a dash's and the shaft's).
const LINE: f32 = 2.0;
/// The dot's radius.
const DOT_R: f32 = 3.9;
/// Where an arm starts, from the centre.
const ARM_FROM: f32 = 6.0;
/// A dotted arm's dash and gap. At rest an arm is two dashes; the outer one is the shape that
/// becomes the arrowhead.
const DASH: f32 = 5.5;
const GAP: f32 = 2.3;
/// The arrowhead: a filled triangle this long, this wide either side of the arm, its base where
/// the outer dash began.
const HEAD_LEN: f32 = 8.7;
const HEAD_HALF: f32 = 3.55;
/// Where the outer dash, and so the head, begins.
const HEAD_BASE: f32 = ARM_FROM + DASH + GAP;
/// What the marker needs around its centre when fully grown, plus a gap, so another manipulator
/// (the dialog's drag arrow) can keep clear of it.
pub(super) const MARKER_CLEAR: f32 = HEAD_BASE + HEAD_LEN + 6.0;

/// The move marker: a dot with four arms to the screen cardinals. At rest each arm is two dashes,
/// a place that can be grabbed and nothing more. Pointed at, the outer dash becomes an arrowhead,
/// its far end narrowing to a point and its near end widening while it grows a little: the
/// feature moves straight in any direction along the face. While it drags, the inner dash runs
/// on into the head as a solid shaft. The dotted-to-solid switch is the one step that cannot
/// tween, so it flips halfway into the drag.
pub(super) fn crosshair(painter: &egui::Painter, c: Pos2, s: &CrossStyle, _t: &Tokens) {
    let solid = s.active > 0.5;
    let (core, case) = (core(s.hot > 0.5).gamma_multiply(s.alpha), casing().gamma_multiply(s.alpha));
    // The outer dash on its way to being the head.
    let len = DASH + (HEAD_LEN - DASH) * s.hot;
    let near = LINE / 2.0 + (HEAD_HALF - LINE / 2.0) * s.hot;
    let far = LINE / 2.0 * (1.0 - s.hot);
    let mut lines: Vec<Vec<Pos2>> = Vec::new();
    let mut heads: Vec<Vec<Pos2>> = Vec::new();
    for d in [vec2(1.0, 0.0), vec2(-1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, -1.0)] {
        // The inner dash, or the shaft up to the head's base.
        lines.push(vec![c + d * ARM_FROM, c + d * if solid { HEAD_BASE } else { ARM_FROM + DASH }]);
        let n: Vec2 = vec2(-d.y, d.x);
        let (base, tip) = (c + d * HEAD_BASE, c + d * (HEAD_BASE + len));
        let mut h = vec![base + n * near, tip + n * far, tip - n * far, base - n * near];
        if far < 0.05 {
            h.remove(2);
        }
        heads.push(h);
    }
    // Casings first, then every core over them, so a core never sits under a neighbour's casing;
    // a head's casing is the same shape grown by the casing's reach, as a line's is.
    for pts in &lines {
        painter.add(egui::Shape::line(pts.clone(), Stroke::new(LINE + CASE, case)));
    }
    for h in &heads {
        painter.add(egui::Shape::convex_polygon(grown(h, CASE / 2.0), case, Stroke::NONE));
    }
    for pts in &lines {
        painter.add(egui::Shape::line(pts.clone(), Stroke::new(LINE, core)));
    }
    for h in &heads {
        painter.add(egui::Shape::convex_polygon(h.clone(), core, Stroke::NONE));
    }
    painter.circle_filled(c, DOT_R + CASE / 2.0, case);
    painter.circle_filled(c, DOT_R, core);
}

/// A convex polygon with every edge moved outwards by `d` (mitred corners).
fn grown(pts: &[Pos2], d: f32) -> Vec<Pos2> {
    let n = pts.len();
    if n < 3 {
        return pts.to_vec();
    }
    let centre = pts.iter().fold(Vec2::ZERO, |a, p| a + p.to_vec2()) / n as f32;
    // Outward unit normal of the edge from pts[i] to pts[i + 1].
    let normal = |i: usize| {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let e = (b - a).normalized();
        let m = vec2(-e.y, e.x);
        let mid = (a.to_vec2() + b.to_vec2()) / 2.0;
        if m.dot(mid - centre) < 0.0 { -m } else { m }
    };
    (0..n)
        .map(|i| {
            let (n1, n2) = (normal((i + n - 1) % n), normal(i));
            let b = (n1 + n2).normalized();
            let cos = b.dot(n1).max(0.2);
            pts[i] + b * (d / cos)
        })
        .collect()
}

/// The turn ring's look for a frame. `hot` eases 0 (resting) to 1 (pointer over the grip or
/// turning), `active` 0 to 1 while turning. While `fading`, the pointer has left: the open ring
/// no longer folds back but fades out over the resting one (`hot` is then its opacity).
pub(super) struct RingStyle {
    pub(super) hot: f32,
    pub(super) active: f32,
    pub(super) fading: bool,
    /// 1 normally, [`DIM`] while the marker is dragged.
    pub(super) alpha: f32,
    /// The grip filled in this colour instead of white: the axis the feature is aligned with.
    pub(super) tint: Option<Color32>,
}

/// The ring's size on screen (radius, pixels): fixed, like the move triad's arrows, whatever the
/// zoom or the feature. It is a control of something on the screen, not a measure of the model.
pub(super) const RING_PX: f32 = 65.0;
/// The ring opening from its grip round the circle: eased in, slow off the grip and closing fast
/// at the far side; once the pointer has left, a quick fade, not the sweep played backwards.
pub(super) const RING_OPEN: Transition =
    Transition { on_s: 0.3, off_s: 0.15, ease_on: egui::emath::easing::quadratic_in, ease_off: egui::emath::easing::linear };
/// The solid arc and the compass coming up when a turn starts, and going.
pub(super) const RING_ACTIVE: Transition = Transition::symmetric(0.2);
/// Half of the arc shown at rest and of the solid arc while turning, either side of the grip (degrees).
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
/// The chevron's leg thickness across the base, and how far from the tip its hole's apex sits at
/// rest (the thickness along the legs' bisector).
const GRIP_LEG: f32 = 1.8;
const GRIP_APEX_IN: f32 = 3.8;
/// Angular step of the arc's polyline (degrees).
const ARC_STEP_DEG: f64 = 1.5;

/// The turn ring: a circle lying on the face about the move marker's centre, drawn in
/// perspective. At rest a short dotted arc with the grip, enough to say that something here
/// turns; pointed at, the dotted circle opens from the grip both ways round and the grip slides
/// in onto it and fills; while turning, a solid arc around the grip is laid over the circle. `on`
/// gives the screen point of the ring's plane at an angle about the face normal and offsets from
/// the circle (outwards, along the tangent) in screen points (none when behind the eye), so the
/// grip lies on the face like the ring; `grip` is the feature's turn, where the grip sits.
pub(super) fn ring(painter: &egui::Painter, on: &dyn Fn(f64, f32, f32) -> Option<Pos2>, grip: f64, s: &RingStyle) {
    if s.fading {
        // The resting ring underneath, the open one fading out over it.
        arc(painter, on, grip, ARC_HALF_DEG, false, 0.0, (1.0 - s.hot) * s.alpha, s.tint);
        arc(painter, on, grip, 180.0, false, 1.0, s.hot * s.alpha, s.tint);
    } else {
        let open = ARC_HALF_DEG + (180.0 - ARC_HALF_DEG) * f64::from(s.hot);
        arc(painter, on, grip, open, false, s.hot, s.alpha, s.tint);
    }
    if s.active > 0.01 {
        arc_only(painter, on, grip, ARC_HALF_DEG, true, s.active * s.alpha);
    }
}

/// One layer of the ring: the arc `half` degrees either side of the grip, dotted or solid, and the
/// grip `slid` of the way from its resting chevron outside the ring to the filled half-diamond on it.
#[allow(clippy::too_many_arguments)]
fn arc(
    painter: &egui::Painter,
    on: &dyn Fn(f64, f32, f32) -> Option<Pos2>,
    grip: f64,
    half: f64,
    solid: bool,
    slid: f32,
    alpha: f32,
    tint: Option<Color32>,
) {
    arc_only(painter, on, grip, half, solid, alpha);
    let (core, case) = (tint.unwrap_or_else(|| core(slid > 0.5)).gamma_multiply(alpha), casing().gamma_multiply(alpha));
    // The grip is a thick white chevron: a triangle with a triangular hole, cased along its outer
    // and its inner edge. As it slides in onto the ring the hole flattens, its apex sinking to the
    // base, until the inner edge is the base line of a full half-diamond and fades away.
    let out = GRIP_OUT * (1.0 - slid);
    let apex_in = GRIP_APEX_IN + (GRIP_LEN - GRIP_APEX_IN) * slid;
    let (Some(tip), Some(c1), Some(c2), Some(i1), Some(i2), Some(apex)) = (
        on(grip, out, 0.0),
        on(grip, out + GRIP_LEN, GRIP_HALF),
        on(grip, out + GRIP_LEN, -GRIP_HALF),
        on(grip, out + GRIP_LEN, GRIP_HALF - GRIP_LEG),
        on(grip, out + GRIP_LEN, -(GRIP_HALF - GRIP_LEG)),
        on(grip, out + apex_in, 0.0),
    ) else {
        return;
    };
    // Casing first, the white over it, so the casing shows by half its width outside every edge,
    // as a line's does: the outer edge and the legs' ends always; the inner edge gives way to the
    // base line as the hole closes.
    let closing = ((slid - 0.85) / 0.15).clamp(0.0, 1.0);
    let lines: Vec<(Vec<Pos2>, f32)> =
        vec![(vec![c1, tip, c2], 1.0), (vec![c1, i1], 1.0), (vec![c2, i2], 1.0), (vec![i1, apex, i2], 1.0 - closing), (vec![i1, i2], closing)];
    for (l, a) in &lines {
        if *a > 0.01 {
            painter.add(egui::Shape::line(l.clone(), Stroke::new(CASE, case.gamma_multiply(*a))));
        }
    }
    // The two legs, convex quads that meet at the tip; as the apex sinks they fill the triangle.
    for leg in [vec![c1, tip, apex, i1], vec![c2, tip, apex, i2]] {
        painter.add(egui::Shape::convex_polygon(leg, core, Stroke::NONE));
    }
}

/// The arc alone, `half` degrees either side of the grip, dotted or solid.
fn arc_only(painter: &egui::Painter, on: &dyn Fn(f64, f32, f32) -> Option<Pos2>, grip: f64, half: f64, solid: bool, alpha: f32) {
    let steps = (2.0 * half / ARC_STEP_DEG).ceil().max(2.0) as usize;
    let pts: Vec<Pos2> =
        (0..=steps).filter_map(|i| on(grip - half.to_radians() + 2.0 * half.to_radians() * i as f64 / steps as f64, 0.0, 0.0)).collect();
    let (core, case) = (core(true).gamma_multiply(alpha), casing().gamma_multiply(alpha));
    let pieces: Vec<Vec<Pos2>> = if solid { vec![pts] } else { dashed(&pts, RING_DASH, RING_GAP) };
    for piece in &pieces {
        painter.add(egui::Shape::line(piece.clone(), Stroke::new(RING_LINE + CASE, case)));
    }
    for piece in &pieces {
        painter.add(egui::Shape::line(piece.clone(), Stroke::new(RING_LINE, core)));
    }
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
/// ring's plane and the unturned frame, so it stays put while the ring turns, each in the colour
/// of the world axis it points along (the move triad's). Faded in by `alpha`.
pub(super) fn compass(painter: &egui::Painter, on: &dyn Fn(f64, f32, f32) -> Option<Pos2>, dirs: &[(f64, Color32)], alpha: f32) {
    let case = casing().gamma_multiply(alpha);
    for &(a, col) in dirs {
        let (Some(p), Some(q)) = (on(a, TICK_FROM, 0.0), on(a, TICK_FROM + TICK_LEN, 0.0)) else { continue };
        painter.line_segment([p, q], Stroke::new(RING_LINE + CASE, case));
        painter.line_segment([p, q], Stroke::new(RING_LINE, col.gamma_multiply(alpha)));
    }
}

/// A compass tick starts this far outside the ring and is this long.
const TICK_FROM: f32 = 4.0;
const TICK_LEN: f32 = 7.0;

/// The turn ring's readout while turning, beside the grip at `at`, away from the ring's `centre`:
/// the angle, and the compass point it was pulled onto ("90° · +X").
pub(super) fn angle_label(painter: &egui::Painter, at: Pos2, centre: Pos2, angle: f64, on: Option<(&str, Color32)>, t: &Tokens) {
    // The point's name in its axis colour, so the eye finds the axis before it reads the letter.
    let font = egui::FontId::proportional(t.font_body);
    let mut job = egui::text::LayoutJob::default();
    job.append(&format!("{:.0}°", angle.to_degrees()), 0.0, egui::TextFormat::simple(font.clone(), t.text));
    if let Some((name, col)) = on {
        job.append(" · ", 0.0, egui::TextFormat::simple(font.clone(), t.text));
        job.append(name, 0.0, egui::TextFormat::simple(font, col));
    }
    let out = (at - centre).normalized();
    let out = if out.is_finite() { out } else { vec2(1.0, 0.0) };
    let anchor = at + out * (GRIP_LEN + GRIP_OUT + 6.0);
    let align = if out.x >= 0.0 { egui::Align2::LEFT_CENTER } else { egui::Align2::RIGHT_CENTER };
    let galley = painter.layout_job(job);
    let rect = align.anchor_size(anchor, galley.size()).expand2(vec2(5.0, 2.0));
    painter.rect(rect, 4.0, t.panel.gamma_multiply(0.92), Stroke::new(1.0, t.manipulator), egui::StrokeKind::Inside);
    painter.galley(rect.shrink2(vec2(5.0, 2.0)).min, galley, t.text);
}
