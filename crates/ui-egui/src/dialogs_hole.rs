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
    ThreadDepth,
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
            Dim::ThreadDepth => "Thread depth",
            Dim::Diameter => "Diameter",
            Dim::TipAngle => "Tip Angle",
        }
    }
}

/// The sizes a hole of type `kind` takes, top to bottom: a depth unless it goes through all or
/// to an object, a diameter unless tapped, and a tip angle for an angled bottom
/// independently of the entry recess.
pub(crate) fn dims(kind: usize, opts: &HoleOpts, to_object: bool) -> Vec<Dim> {
    let mut v = match kind {
        1 => vec![Dim::TopDiameter, Dim::CbDepth],
        2 => vec![Dim::TopDiameter, Dim::CsAngle],
        _ => Vec::new(),
    };
    if !opts.all && !to_object {
        v.push(Dim::Depth);
    }
    if opts.tapped && !opts.thread_full_depth {
        v.push(Dim::ThreadDepth);
    }
    if !opts.tapped {
        v.push(Dim::Diameter);
    }
    if opts.angled && !opts.all {
        v.push(Dim::TipAngle);
    }
    v
}

/// Tapped bores use the engine's catalogued cut-tap drill, including Unified and parallel pipe sizes.
/// A custom draft with an error must not illustrate its stale, last valid designation.
fn bore_diameter(diameter: &str, opts: &HoleOpts, eval: &dyn Fn(&str, ValueKind) -> Option<f64>) -> Option<f64> {
    if opts.tapped {
        if opts.custom_thread.as_ref().is_some_and(|draft| draft.error.is_some()) {
            return None;
        }
        solidcraft_engine::doc::parse_tapped_thread(&opts.thread).map(|size| size.tap_drill)
    } else {
        eval(diameter, ValueKind::Length).filter(|v| v.is_finite() && *v > 0.0)
    }
}

/// What the picture is drawn from.
struct Section {
    kind: usize,
    through: bool,
    to_object: bool,
    angled: bool,
    tapped: bool,
    /// A schematic fraction: known blind extents use the entered lengths. To Object and
    /// Through All have no typed drill length, so partial tapping is shown as a shorter span.
    thread_fraction: f32,
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
    let bore = bore_diameter(diameter, opts, eval);
    let ratio = match (bore, len(top)) {
        (Some(d), Some(t)) if kind != 0 => (d / t).clamp(0.3, 0.8) as f32,
        _ if kind != 0 => 0.55,
        _ => 1.0,
    };
    let section = Section {
        kind,
        through: opts.all,
        to_object,
        angled: opts.angled,
        tapped: opts.tapped,
        thread_fraction: if opts.thread_full_depth {
            1.0
        } else if !opts.all && !to_object {
            let recess = match kind {
                1 => len(cb_depth.as_str()).unwrap_or(0.0),
                2 => match (bore, len(cs_diameter.as_str())) {
                    (Some(d), Some(top)) => (top - d).max(0.0) / (2.0 * angle(cs_angle.as_str(), 0.35, 1.3, std::f64::consts::FRAC_PI_4).tan()),
                    _ => 0.0,
                },
                _ => 0.0,
            };
            match (len(opts.thread_depth.as_str()), len(depth.as_str()).filter(|d| *d > recess)) {
                (Some(thread), Some(drill)) => (thread / (drill - recess)).clamp(0.05, 1.0) as f32,
                _ => 0.55,
            }
        } else {
            0.55
        },
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
    if dims.contains(&Dim::ThreadDepth) {
        fields.push((Dim::ThreadDepth.label(kind), &mut opts.thread_depth));
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
    if a.distance(b) < 14.0 {
        // Short partial depths need outward arrows rather than overlapping heads.
        let d = (b - a).normalized();
        head(p, a, a - d * 8.0, stroke.color);
        head(p, b, b + d * 8.0, stroke.color);
    } else {
        head(p, a, b, stroke.color);
        head(p, b, a, stroke.color);
    }
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
    // Leave three separate depth lanes inside the picture even on a narrow form.
    let cx = pic.left() + pic.width() * 0.40;
    let wide = pic.width() * 0.20;
    let (left, right) = (pic.left() + pic.width() * 0.04, pic.left() + pic.width() * 0.68);
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
        for d in [Dim::Depth, Dim::ThreadDepth, Dim::Diameter] {
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
    let thread_start = step + 2.0;
    let thread_end = thread_start + ((if s.through { bottom } else { shoulder - 3.0 }) - thread_start).max(0.0) * s.thread_fraction;
    if s.tapped {
        let r = bore_r * 1.22;
        for x in [cx - r, cx + r] {
            p.line_segment([pos2(x, thread_start), pos2(x, thread_end)], Stroke::new(1.0, t.form_ink));
        }
        p.line_segment([pos2(cx - r, thread_end), pos2(cx - bore_r, thread_end)], Stroke::new(1.0, t.form_ink));
        p.line_segment([pos2(cx + bore_r, thread_end), pos2(cx + r, thread_end)], Stroke::new(1.0, t.form_ink));
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
    let mut x = right + pic.width() * 0.07;
    if let Some(y) = y_of(Dim::CbDepth) {
        p.line_segment([pos2(cx + top_r, step), pos2(x + 3.0, step)], thin(Dim::CbDepth));
        p.line_segment([pos2(right, top), pos2(x + 3.0, top)], thin(Dim::CbDepth));
        dimension(p, pos2(x, top), pos2(x, step), ink(Dim::CbDepth));
        leader(Dim::CbDepth, pos2(x, y.clamp(top + 2.0, step - 2.0)));
        x += pic.width() * 0.10;
    }
    if let Some(y) = y_of(Dim::ThreadDepth) {
        p.line_segment([pos2(cx + bore_r * 1.22, thread_start), pos2(x + 3.0, thread_start)], thin(Dim::ThreadDepth));
        p.line_segment([pos2(cx + bore_r * 1.22, thread_end), pos2(x + 3.0, thread_end)], thin(Dim::ThreadDepth));
        dimension(p, pos2(x, thread_start), pos2(x, thread_end), ink(Dim::ThreadDepth));
        leader(Dim::ThreadDepth, pos2(x, y.clamp(thread_start, thread_end.max(thread_start))));
        x += pic.width() * 0.10;
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

    #[test]
    fn recessed_drill_point_total_depth_dimension_is_outside_partial_tapping() {
        for kind in [1, 2] {
            for width in [80.0, 120.0] {
                for fraction in [0.05, 0.5] {
                    let ctx = crate::fonts::test_context();
                    let pic = Rect::from_min_size(pos2(10.0, 10.0), vec2(width, 200.0));
                    let section = Section {
                        kind,
                        through: false,
                        to_object: false,
                        angled: false,
                        tapped: true,
                        thread_fraction: fraction,
                        ratio: 0.5,
                        cs_half: std::f32::consts::FRAC_PI_4,
                        tip_half: 1.03,
                    };
                    let slots = [
                        (Dim::TopDiameter, 25.0),
                        (if kind == 1 { Dim::CbDepth } else { Dim::CsAngle }, 60.0),
                        (Dim::Depth, 105.0),
                        (Dim::ThreadDepth, 145.0),
                    ]
                    .map(|(dim, y)| (dim, Rect::from_min_size(pos2(150.0, y), vec2(100.0, 20.0)), false));
                    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| paint(ui.painter(), pic, &section, &slots));
                    output.textures_delta.clear();
                    let mut dimensions: Vec<_> = output
                        .shapes
                        .iter()
                        .filter_map(|clipped| match &clipped.shape {
                            Shape::LineSegment { points, stroke }
                                if (stroke.width - 1.0).abs() < 1e-4
                                    && (points[0].x - points[1].x).abs() < 1e-4
                                    && points[0].x > pic.left() + width * 0.68
                                    && (points[0].y - points[1].y).abs() > 0.5 =>
                            {
                                Some((points[0].x, (points[0].y - points[1].y).abs()))
                            }
                            _ => None,
                        })
                        .collect();
                    assert!(dimensions.len() >= 2, "both total and partial depths must be drawn");
                    dimensions.sort_by(|a, b| b.1.total_cmp(&a.1));
                    let total = dimensions[0];
                    assert!(dimensions.iter().skip(1).all(|shorter| total.0 > shorter.0), "longest drill depth must be outside: {dimensions:?}");
                }
            }
        }
    }

    #[test]
    fn tapped_section_uses_the_current_nominal_size_and_pitch() {
        let mut opts = HoleOpts { tapped: true, thread: "M8".into(), ..HoleOpts::default() };
        let stale_diameter = |_: &str, _: ValueKind| Some(4.0);
        assert_eq!(bore_diameter("4 mm", &opts, &stale_diameter), Some(6.75));
        opts.thread = "M8x1.2".into();
        assert_eq!(bore_diameter("4 mm", &opts, &stale_diameter), Some(6.8));
        for (designation, drill) in [("G 1/4", 11.75), ("Rp 1/4", 11.4), ("1/4-20 UNC", 5.1054)] {
            opts.thread = designation.into();
            assert_eq!(bore_diameter("4 mm", &opts, &stale_diameter), Some(drill), "{designation}");
        }
        opts.thread = "invalid".into();
        assert_eq!(bore_diameter("4 mm", &opts, &stale_diameter), None);
        opts.tapped = false;
        assert_eq!(bore_diameter("4 mm", &opts, &stale_diameter), Some(4.0));
    }

    #[test]
    fn partial_tapping_has_an_independent_dimension_for_every_extent() {
        for kind in 0..=2 {
            for through in [false, true] {
                let mut opts = HoleOpts { tapped: true, thread_full_depth: false, all: through, ..HoleOpts::default() };
                for to_object in [false, true] {
                    let labels: Vec<_> = dims(kind, &opts, to_object).into_iter().map(|d| d.label(kind)).collect();
                    assert!(labels.contains(&"Thread depth"), "partial tapping needs its own dimension: {labels:?}");
                }
                opts.thread_full_depth = true;
                assert!(!dims(kind, &opts, false).into_iter().any(|d| d.label(kind) == "Thread depth"));
                opts.thread_full_depth = false;
                opts.tapped = false;
                assert!(!dims(kind, &opts, false).into_iter().any(|d| d.label(kind) == "Thread depth"));
            }
        }
    }

    #[test]
    fn partial_tapping_fields_do_not_overlap_on_narrow_forms() {
        for width in [220.0, 280.0, 360.0] {
            let ctx = crate::fonts::test_context();
            let app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
            let mut kind = crate::dialogs::Dialog::for_command(&app, "solid.hole").unwrap().kind;
            if let Kind::Hole { kind, opts, .. } = &mut kind {
                *kind = 1;
                opts.tapped = true;
                opts.thread_full_depth = false;
            }
            for _ in 0..3 {
                let mut output = ctx.run_ui(
                    egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width + 20.0, 700.0))), ..Default::default() },
                    |ui| {
                        ui.set_width(width);
                        crate::ui_kit::scope(ui, |ui| rows(ui, &mut kind, false, &|_, _| Some(10.0)));
                    },
                );
                output.textures_delta.clear();
            }
            let figure = crate::scenario::rect_of("dialog:figure:hole section").unwrap();
            let picture = crate::scenario::rect_of("dialog:figure_picture:hole section").unwrap();
            let mut previous_bottom = figure.top();
            assert!(!dims(1, &HoleOpts { tapped: true, ..HoleOpts::default() }, false).contains(&Dim::Diameter));
            for label in ["Counterbore Ø", "Counterbore depth", "Depth", "Thread depth"] {
                let field = crate::scenario::rect_of(&format!("dialog:widget:{label}")).unwrap();
                assert!(figure.contains_rect(field), "{label} is outside the {width} pt form: {field:?}");
                assert!(field.left() > picture.right() && field.top() >= previous_bottom, "{label} overlaps another element: {field:?}");
                previous_bottom = field.bottom();
            }
        }
    }

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
