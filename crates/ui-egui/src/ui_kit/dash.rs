//! Dashed lines for painters whose points may reach far outside the painted area.
use egui::{Pos2, Rect, Shape, Stroke, pos2};

/// [`Shape::dashed_line`] for points projected into the viewport, cut to `clip` (grown by two
/// dash periods, so the cuts lie off screen). epaint makes one shape per dash and steps along in
/// f32, so a line reaching far off screen made millions of shapes, or never ended: an Extrude
/// distance of 1e9 mm filled the memory within a second. A line inside the bounds is dashed as
/// before; a cut one keeps its dashes in phase along the whole line, so they don't crawl as the
/// view moves.
pub(crate) fn dashed_line(pts: &[Pos2], clip: Rect, stroke: Stroke, dash: f32, gap: f32) -> Vec<Shape> {
    let bounds = clip.expand(2.0 * (dash + gap));
    if pts.iter().all(|p| bounds.contains(*p)) {
        return Shape::dashed_line(pts, stroke, dash, gap);
    }
    let period = f64::from(dash + gap);
    let mut out = Vec::new();
    let mut run: Vec<Pos2> = Vec::new();
    let mut offset = 0.0;
    let mut flush = |run: &mut Vec<Pos2>, offset: f32| {
        if run.len() >= 2 && period > 0.0 {
            out.extend(Shape::dashed_line_with_offset(run, stroke, &[dash], &[gap], offset));
        }
        run.clear();
    };
    let mut along = 0.0_f64;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = f64::from(a.distance(b));
        match clip_segment(a, b, bounds).filter(|_| len.is_finite()) {
            Some((t0, t1)) => {
                // In f64: a cut a trillion points from `a` is lost in f32.
                let at = |t: f64| pos2((f64::from(a.x) + f64::from(b.x - a.x) * t) as f32, (f64::from(a.y) + f64::from(b.y - a.y) * t) as f32);
                let (from, to) = (at(t0), at(t1));
                if run.is_empty() || t0 > 0.0 {
                    flush(&mut run, offset);
                    // How far into its period the pattern is where the run begins: a cut run
                    // waits for the next dash (the wait is off screen), the line's start begins one.
                    let phase = (along + len * t0) % period;
                    offset = if t0 > 0.0 { ((period - phase) % period) as f32 } else { 0.0 };
                    run.push(from);
                }
                run.push(to);
                if t1 < 1.0 {
                    flush(&mut run, offset);
                }
            }
            None => flush(&mut run, offset),
        }
        if len.is_finite() {
            along += len;
        }
    }
    flush(&mut run, offset);
    out
}

/// The part of segment `a`–`b` inside `r`, as parameters `0 ≤ t0 ≤ t1 ≤ 1` along it; `None` when
/// it misses `r` or a coordinate is not finite (Liang–Barsky, in f64).
fn clip_segment(a: Pos2, b: Pos2, r: Rect) -> Option<(f64, f64)> {
    let (x0, y0, dx, dy) = (f64::from(a.x), f64::from(a.y), f64::from(b.x - a.x), f64::from(b.y - a.y));
    if ![x0, y0, dx, dy].iter().all(|v| v.is_finite()) {
        return None;
    }
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (p, q) in [(-dx, x0 - f64::from(r.min.x)), (dx, f64::from(r.max.x) - x0), (-dy, y0 - f64::from(r.min.y)), (dy, f64::from(r.max.y) - y0)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    (t0 <= t1).then_some((t0, t1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Color32, vec2};

    /// A dashed line reaching far off screen is cut to the view before it is dashed: epaint makes a
    /// shape per dash and steps along in f32, so one a trillion points long made billions of them,
    /// or never finished. A line in view is dashed exactly as before, and a cut one keeps its
    /// dashes in phase with its start.
    #[test]
    fn a_dashed_line_far_off_screen_is_cut_to_the_view() {
        let view = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
        let stroke = Stroke::new(1.0, Color32::WHITE);
        let near = [pos2(100.0, 100.0), pos2(700.0, 400.0), pos2(900.0, 100.0)];
        assert_eq!(dashed_line(&near, view, stroke, 5.0, 3.0), Shape::dashed_line(&near, stroke, 5.0, 3.0));
        let far = dashed_line(&[pos2(100.0, 500.0), pos2(1e12, 500.0)], view, stroke, 5.0, 3.0);
        assert!(!far.is_empty() && far.len() < 250, "{} dashes", far.len());
        for s in &far {
            let Shape::LineSegment { points, .. } = s else { panic!("{s:?}") };
            let periods = (points[0].x - 100.0) / 8.0;
            assert!((periods - periods.round()).abs() < 1e-3, "a dash starts at {points:?}, off the line's pattern");
        }
        let across = dashed_line(&[pos2(-1e12, 500.0), pos2(1e12, 500.0)], view, stroke, 5.0, 3.0);
        assert!(across.len() < 250, "{} dashes", across.len());
        assert!(
            across.iter().any(|s| matches!(s, Shape::LineSegment { points, .. } if (700.0..900.0).contains(&points[0].x))),
            "the line shows in view"
        );
        for missing in [[pos2(f32::NAN, 0.0), pos2(10.0, 10.0)], [pos2(f32::INFINITY, 0.0), pos2(10.0, 10.0)], [pos2(-1e9, -1e9), pos2(-1e9, 1e9)]] {
            assert!(dashed_line(&missing, view, stroke, 5.0, 3.0).is_empty(), "{missing:?}");
        }
    }
}
