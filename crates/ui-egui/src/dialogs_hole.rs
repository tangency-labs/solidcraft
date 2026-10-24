//! The Hole dialog's section (#2171): a cut through the hole as its options make it (simple,
//! counterbore or countersink; to a depth, to an object or through all; a flat or an angled
//! tip; tapped), with each size typed in a field beside the dimension it sets. The picture is
//! schematic: its widths follow the diameters loosely, its heights line up with the fields.
//! Hovering a field, or typing in it, lights its dimension.

use egui::{Color32, Pos2, Rect, Shape, Stroke, pos2, vec2};
use solidcraft_engine::doc::expr::Kind as ValueKind;

use crate::dialogs::{HoleOpts, Kind};
use crate::theme::Tokens;
use crate::ui_kit::FigureSlot;

/// A size the section shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dim {
    /// The counterbore's or the countersink's diameter, across the top.
    TopDiameter,
    CbDepth,
    CsAngle,
    Depth,
    Diameter,
    TipAngle,
}

impl Dim {
    /// The field's name, as the rows had it (scenarios find `dialog:<name>`).
    pub(crate) fn label(self, kind: usize) -> &'static str {
        match self {
            Dim::TopDiameter if kind == 2 => "Countersink Ø",
            Dim::TopDiameter => "Counterbore Ø",
            Dim::CbDepth => "Counterbore depth",
            Dim::CsAngle => "Countersink angle",
            Dim::Depth => "Depth",
            Dim::Diameter => "Diameter",
            Dim::TipAngle => "Tip Angle",
        }
    }
}

/// The sizes a hole of type `kind` takes, top to bottom: a depth unless it goes through all or
/// to an object, a diameter unless it is tapped (the thread sets it), a tip angle for an angled
/// tip at the bottom of a blind simple hole.
pub(crate) fn dims(kind: usize, opts: &HoleOpts, to_object: bool) -> Vec<Dim> {
    let mut v = match kind {
        1 => vec![Dim::TopDiameter, Dim::CbDepth],
        2 => vec![Dim::TopDiameter, Dim::CsAngle],
        _ => Vec::new(),
    };
    if !opts.all && !to_object {
        v.push(Dim::Depth);
    }
    if !opts.tapped {
        v.push(Dim::Diameter);
    }
    if kind == 0 && opts.angled && !opts.all {
        v.push(Dim::TipAngle);
    }
    v
}

/// What the picture is drawn from.
struct Section {
    kind: usize,
    through: bool,
    to_object: bool,
    angled: bool,
    tapped: bool,
    /// The bore's diameter over the counterbore's or countersink's (1 for a simple hole).
    ratio: f32,
    /// Half the countersink's and the tip's angles (radians).
    cs_half: f32,
    tip_half: f32,
}

/// The section and its sizes (`ui_kit::figure_rows`): the picture in the label column, the
/// fields beside it. Returns whether Enter was accepted in a field.
pub(crate) fn rows(ui: &mut egui::Ui, k: &mut Kind, to_object: bool, eval: &dyn Fn(&str, ValueKind) -> Option<f64>) -> bool {
    let Kind::Hole { diameter, depth, kind, cb_diameter, cb_depth, cs_diameter, cs_angle, opts } = k else { return false };
    let kind = *kind;
    let dims = dims(kind, opts, to_object);
    let len = |e: &str| eval(e, ValueKind::Length).filter(|v| v.is_finite() && *v > 0.0);
    let angle = |e: &str, lo: f64, hi: f64, or: f64| eval(e, ValueKind::Angle).filter(|v| v.is_finite()).map_or(or, |v| (v / 2.0).clamp(lo, hi));
    let top = if kind == 2 { cs_diameter.as_str() } else { cb_diameter.as_str() };
    let ratio = match (len(diameter.as_str()), len(top)) {
        (Some(d), Some(t)) if kind != 0 => (d / t).clamp(0.3, 0.8) as f32,
        _ if kind != 0 => 0.55,
        _ => 1.0,
    };
    let section = Section {
        kind,
        through: opts.all,
        to_object,
        angled: kind == 0 && opts.angled,
        tapped: opts.tapped,
        ratio,
        cs_half: angle(cs_angle.as_str(), 0.35, 1.3, std::f64::consts::FRAC_PI_4) as f32,
        tip_half: angle(opts.tip_angle.as_str(), 0.5, 1.3, 1.03) as f32,
    };
    // The fields in `dims`' order, each value taken once.
    let mut fields: Vec<(&str, &mut String)> = Vec::with_capacity(dims.len());
    match kind {
        1 => {
            fields.push((Dim::TopDiameter.label(kind), cb_diameter));
            fields.push((Dim::CbDepth.label(kind), cb_depth));
        }
        2 => {
            fields.push((Dim::TopDiameter.label(kind), cs_diameter));
            fields.push((Dim::CsAngle.label(kind), cs_angle));
        }
        _ => {}
    }
    if dims.contains(&Dim::Depth) {
        fields.push((Dim::Depth.label(kind), depth));
    }
    if dims.contains(&Dim::Diameter) {
        fields.push((Dim::Diameter.label(kind), diameter));
    }
    if dims.contains(&Dim::TipAngle) {
        fields.push((Dim::TipAngle.label(kind), &mut opts.tip_angle));
    }
    crate::ui_kit::figure_rows(ui, "hole section", &mut fields, |painter, pic, slots| {
        let slots: Vec<(Dim, Rect, bool)> = dims.iter().zip(slots).map(|(d, s): (&Dim, &FigureSlot)| (*d, s.rect, s.hot)).collect();
        paint(painter, pic, &section, &slots);
    })
}

/// A small filled arrowhead at `tip`, pointing away from `from`.
fn head(p: &egui::Painter, tip: Pos2, from: Pos2, col: Color32) {
    let d = (tip - from).normalized();
    let n = vec2(-d.y, d.x);
    p.add(Shape::convex_polygon(vec![tip, tip - d * 6.0 + n * 2.5, tip - d * 6.0 - n * 2.5], col, Stroke::NONE));
}

/// A dimension line from `a` to `b` with arrowheads at both ends.
fn dimension(p: &egui::Painter, a: Pos2, b: Pos2, stroke: Stroke) {
    p.line_segment([a, b], stroke);
    head(p, a, b, stroke.color);
    head(p, b, a, stroke.color);
}

/// An arc about `c` of radius `r` between the directions at `from` and `to` (radians, screen y
/// down), with arrowheads at both ends; returns its middle.
fn angle_arc(p: &egui::Painter, c: Pos2, r: f32, from: f32, to: f32, stroke: Stroke) -> Pos2 {
    let at = |a: f32| c + vec2(a.cos(), a.sin()) * r;
    let pts: Vec<Pos2> = (0..=16).map(|k| at(from + (to - from) * k as f32 / 16.0)).collect();
    p.add(Shape::line(pts.clone(), stroke));
    if let (Some(a), Some(a2), Some(b), Some(b2)) = (pts.first(), pts.get(2), pts.last(), pts.get(14)) {
        head(p, *a, *a2, stroke.color);
        head(p, *b, *b2, stroke.color);
    }
    at((from + to) / 2.0)
}

/// Diagonal hatching across `r` (a cut through material).
fn hatch(p: &egui::Painter, r: Rect, stroke: Stroke) {
    let step = 7.0;
    let mut x = r.left() - r.height();
    while x < r.right() {
        // The line x + s, bottom - s for s in 0..height, clipped to the rectangle.
        let (s0, s1) = ((r.left() - x).max(0.0), (r.right() - x).min(r.height()));
        if s1 > s0 {
            p.line_segment([pos2(x + s0, r.bottom() - s0), pos2(x + s1, r.bottom() - s1)], stroke);
        }
        x += step;
    }
}

/// Draw the section into `pic`, its dimensions at the heights of the fields in `slots`, each
/// with a leader to its field.
fn paint(p: &egui::Painter, pic: Rect, s: &Section, slots: &[(Dim, Rect, bool)]) {
    let t = Tokens::get();
    let field = |d: Dim| slots.iter().find(|x| x.0 == d).map(|x| (x.1, x.2));
    let y_of = |d: Dim| field(d).map(|(r, _)| r.center().y);
    let ink = |d: Dim| {
        let hot = field(d).is_some_and(|(_, h)| h);
        if hot { Stroke::new(1.6, t.form_primary) } else { Stroke::new(1.0, t.form_dim) }
    };
    // A leader from a dimension's point to its field's left edge.
    let leader = |d: Dim, from: Pos2| {
        if let Some((r, _)) = field(d) {
            let to = pos2(r.left() - 3.0, r.center().y);
            let st = ink(d);
            p.line_segment([from, to], Stroke::new(st.width.min(1.2), st.color));
            p.circle_filled(from, 1.8, st.color);
        }
    };
    // Across: the axis, the widest half width, the material's sides; the vertical dimensions sit
    // right of the material, before the fields.
    let cx = pic.left() + 44.0;
    let wide = 24.0;
    let (left, right) = (pic.left() + 6.0, pic.left() + 76.0);
    let bottom = pic.bottom() - 3.0;
    let top_r = wide;
    let bore_r = if s.kind == 0 { wide * 0.55 } else { wide * s.ratio };
    // Down: the top face, the counterbore's floor or the countersink's end, the bore's bottom.
    let top = y_of(Dim::TopDiameter).map_or(pic.top() + 14.0, |y| y + 14.0);
    let step = match s.kind {
        1 => y_of(Dim::CbDepth).map_or(top + 16.0, |y| y + 9.0).max(top + 12.0),
        2 => top + ((top_r - bore_r) / s.cs_half.tan().max(0.2)).clamp(6.0, 28.0),
        _ => top,
    };
    let tip_h = if s.angled { (bore_r / s.tip_half.tan().max(0.2)).clamp(4.0, 16.0) } else { 0.0 };
    let shoulder = if s.through {
        bottom + 1.0
    } else {
        let mut y = step + 18.0;
        for d in [Dim::Depth, Dim::Diameter] {
            if let Some(v) = y_of(d) {
                y = y.max(v + 10.0);
            }
        }
        if let Some(v) = y_of(Dim::TipAngle) {
            y = y.max(v - tip_h - 2.0);
        }
        y.min(bottom - tip_h - 10.0).max(step + 8.0)
    };
    // The material, cut: hatched, with the hole's room cleared out of it.
    let block = Rect::from_min_max(pos2(left, top), pos2(right, bottom));
    p.rect_filled(block, 0.0, t.form_dim.gamma_multiply(0.16));
    hatch(p, block, Stroke::new(0.8, t.form_dim.gamma_multiply(0.45)));
    // The right half of the hole's outline, top to bottom (half widths), then mirrored.
    let mut half: Vec<(f32, f32)> = match s.kind {
        1 => vec![(top_r, top), (top_r, step), (bore_r, step), (bore_r, shoulder)],
        2 => vec![(top_r, top), (bore_r, step), (bore_r, shoulder)],
        _ => vec![(bore_r, top), (bore_r, shoulder)],
    };
    if s.through
        && let Some(last) = half.last_mut()
    {
        last.1 = bottom + 1.0;
    }
    let clear = t.dialog_body;
    for w in half.windows(2) {
        let &[(a, ya), (b, yb)] = w else { continue };
        if yb > ya {
            p.add(Shape::convex_polygon(vec![pos2(cx - a, ya), pos2(cx + a, ya), pos2(cx + b, yb), pos2(cx - b, yb)], clear, Stroke::NONE));
        }
    }
    let apex = pos2(cx, shoulder + tip_h);
    if s.angled && !s.through {
        p.add(Shape::convex_polygon(vec![pos2(cx - bore_r, shoulder), pos2(cx + bore_r, shoulder), apex], clear, Stroke::NONE));
    }
    // A tapped hole's thread: thin lines at the thread's major diameter, in the material.
    if s.tapped {
        let r = bore_r * 1.22;
        let (y0, y1) = (half.iter().rev().nth(1).map_or(top, |x| x.1), if s.through { bottom } else { shoulder - 3.0 });
        for x in [cx - r, cx + r] {
            p.line_segment([pos2(x, y0 + 2.0), pos2(x, y1)], Stroke::new(1.0, t.form_ink));
        }
    }
    // Outlines: the block's top and sides, the hole's walls and bottom.
    let edge = Stroke::new(1.4, t.form_ink);
    let wall = |sign: f32| -> Vec<Pos2> { half.iter().map(|(w, y)| pos2(cx + sign * w, *y)).collect() };
    let opening = half.first().map_or(bore_r, |x| x.0);
    p.add(Shape::line(vec![pos2(left, bottom), pos2(left, top), pos2(cx - opening, top)], edge));
    p.add(Shape::line(vec![pos2(cx + opening, top), pos2(right, top), pos2(right, bottom)], edge));
    p.add(Shape::line(wall(-1.0), edge));
    p.add(Shape::line(wall(1.0), edge));
    if s.through {
        p.line_segment([pos2(left, bottom), pos2(cx - bore_r, bottom)], edge);
        p.line_segment([pos2(cx + bore_r, bottom), pos2(right, bottom)], edge);
    } else {
        p.line_segment([pos2(left, bottom), pos2(right, bottom)], edge);
        if s.angled {
            p.add(Shape::line(vec![pos2(cx - bore_r, shoulder), apex, pos2(cx + bore_r, shoulder)], edge));
        } else {
            p.line_segment([pos2(cx - bore_r, shoulder), pos2(cx + bore_r, shoulder)], edge);
        }
    }
    // To an object: the face it stops at, dashed across the bottom of the hole.
    if s.to_object {
        let y = shoulder + tip_h + 3.0;
        p.add(Shape::dashed_line(&[pos2(left - 2.0, y), pos2(right + 2.0, y)], Stroke::new(1.2, t.form_primary), 4.0, 3.0));
    }
    // The hole's axis.
    p.add(Shape::dashed_line(&[pos2(cx, top - 6.0), pos2(cx, (shoulder + tip_h + 6.0).min(pic.bottom()))], Stroke::new(0.8, t.form_dim), 6.0, 3.0));

    // Dimensions, each lined up with its field.
    let thin = |d: Dim| {
        let st = ink(d);
        Stroke::new(0.8, st.color)
    };
    if let Some(y) = y_of(Dim::TopDiameter) {
        let (a, b) = (pos2(cx - top_r, y), pos2(cx + top_r, y));
        p.line_segment([pos2(a.x, top - 2.0), pos2(a.x, y - 3.0)], thin(Dim::TopDiameter));
        p.line_segment([pos2(b.x, top - 2.0), pos2(b.x, y - 3.0)], thin(Dim::TopDiameter));
        dimension(p, a, b, ink(Dim::TopDiameter));
        leader(Dim::TopDiameter, b);
    }
    // Depths stand right of the material: the counterbore's nearer, the hole's farther out.
    let mut x = right + 8.0;
    if let Some(y) = y_of(Dim::CbDepth) {
        p.line_segment([pos2(cx + top_r, step), pos2(x + 3.0, step)], thin(Dim::CbDepth));
        p.line_segment([pos2(right, top), pos2(x + 3.0, top)], thin(Dim::CbDepth));
        dimension(p, pos2(x, top), pos2(x, step), ink(Dim::CbDepth));
        leader(Dim::CbDepth, pos2(x, y.clamp(top + 2.0, step - 2.0)));
        x += 10.0;
    }
    if let Some(y) = y_of(Dim::Depth) {
        p.line_segment([pos2(cx + bore_r, shoulder), pos2(x + 3.0, shoulder)], thin(Dim::Depth));
        p.line_segment([pos2(right, top), pos2(x + 3.0, top)], thin(Dim::Depth));
        dimension(p, pos2(x, top), pos2(x, shoulder), ink(Dim::Depth));
        leader(Dim::Depth, pos2(x, y.clamp(top + 2.0, shoulder - 2.0)));
    }
    if let Some(y) = y_of(Dim::CsAngle) {
        // About the point where the countersink's sides meet, between them.
        let k = s.cs_half.tan().max(0.2);
        let meet = pos2(cx, top + top_r / k);
        let r = (meet.y - top) * 0.7;
        let up = -std::f32::consts::FRAC_PI_2;
        let mid = angle_arc(p, meet, r, up - s.cs_half, up + s.cs_half, ink(Dim::CsAngle));
        let end = meet + vec2((up + s.cs_half).cos(), (up + s.cs_half).sin()) * r;
        leader(Dim::CsAngle, if (end.y - y).abs() < (mid.y - y).abs() { end } else { mid });
    }
    if let Some(y) = y_of(Dim::Diameter) {
        let lo = if s.kind == 0 { top + 3.0 } else { step + 3.0 };
        let hi = if s.through { bottom - 3.0 } else { shoulder - 3.0 };
        let y = if hi > lo { y.clamp(lo, hi) } else { (lo + hi) / 2.0 };
        let (a, b) = (pos2(cx - bore_r, y), pos2(cx + bore_r, y));
        dimension(p, a, b, ink(Dim::Diameter));
        leader(Dim::Diameter, b);
    }
    if y_of(Dim::TipAngle).is_some() && s.angled && !s.through {
        // Most of the way along the tip's sides, so the arc spans them where they are clear.
        let r = (bore_r.hypot(tip_h) * 0.8).max(6.0);
        let up = -std::f32::consts::FRAC_PI_2;
        angle_arc(p, apex, r, up - s.tip_half, up + s.tip_half, ink(Dim::TipAngle));
        leader(Dim::TipAngle, apex + vec2((up + s.tip_half).cos(), (up + s.tip_half).sin()) * r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(all: bool, angled: bool) -> HoleOpts {
        HoleOpts { all, angled, ..HoleOpts::default() }
    }

    #[test]
    fn the_sizes_follow_the_options() {
        use Dim::*;
        assert_eq!(dims(0, &opts(false, true), false), vec![Depth, Diameter, TipAngle]);
        assert_eq!(dims(0, &opts(false, false), false), vec![Depth, Diameter]);
        assert_eq!(dims(0, &opts(true, true), false), vec![Diameter], "through all: no depth, no tip");
        assert_eq!(dims(0, &opts(false, true), true), vec![Diameter, TipAngle], "to an object: no depth");
        assert_eq!(dims(1, &opts(false, false), false), vec![TopDiameter, CbDepth, Depth, Diameter]);
        assert_eq!(dims(2, &opts(true, false), false), vec![TopDiameter, CsAngle, Diameter]);
        let tapped = HoleOpts { tapped: true, ..opts(false, true) };
        assert_eq!(dims(0, &tapped, false), vec![Depth, TipAngle], "tapped: the thread sets the diameter");
        assert_eq!(TopDiameter.label(1), "Counterbore Ø");
        assert_eq!(TopDiameter.label(2), "Countersink Ø");
    }
}
