//! Analytic curve shapes for geometry operations (profiles, trim, extend, fillet…).

use std::f64::consts::TAU;

use solidcraft_geom::{Seg2, Vec2};

use crate::model::{CurveKind, Sketch};

/// A curve of the sketch as an analytic shape for splitting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Line {
        a: Vec2,
        b: Vec2,
    },
    /// Circle (full when `sweep` is 2π) from `start` angle sweeping counter-clockwise.
    Round {
        c: Vec2,
        r: f64,
        start: f64,
        sweep: f64,
    },
}

impl Shape {
    /// Curve parameter of `p` if it lies on the curve's interior (line t in (0,1); round:
    /// angle offset from start in (0, sweep)).
    pub fn param_on(&self, p: Vec2, tol: f64) -> Option<f64> {
        match *self {
            Shape::Line { a, b } => {
                let d = b - a;
                let l2 = d.len2();
                if l2 < 1e-24 {
                    return None;
                }
                let t = (p - a).dot(d) / l2;
                let q = a + d * t;
                (q.dist(p) <= tol && t * l2.sqrt() > tol && (1.0 - t) * l2.sqrt() > tol).then_some(t)
            }
            Shape::Round { c, r, start, sweep } => {
                if (p.dist(c) - r).abs() > tol {
                    return None;
                }
                let off = ((p - c).angle() - start).rem_euclid(TAU);
                let ang_tol = tol / r.max(1e-12);
                let full = sweep >= TAU - 1e-12;
                if full {
                    return Some(off);
                }
                (off > ang_tol && off < sweep - ang_tol).then_some(off)
            }
        }
    }
    pub fn point(&self, t: f64) -> Vec2 {
        match *self {
            Shape::Line { a, b } => a.lerp(b, t),
            Shape::Round { c, r, start, .. } => c + Vec2::from_angle(start + t) * r,
        }
    }
}

/// Intersection points of two shapes (as circles/infinite lines; callers filter by range).
pub fn intersections(x: &Shape, y: &Shape) -> Vec<Vec2> {
    match (*x, *y) {
        (Shape::Line { a, b }, Shape::Line { a: c, b: d }) => {
            let (r, s) = (b - a, d - c);
            let den = r.cross(s);
            if den.abs() < 1e-12 * r.len() * s.len() {
                return Vec::new();
            }
            let t = (c - a).cross(s) / den;
            vec![a + r * t]
        }
        (Shape::Line { a, b }, Shape::Round { c, r, .. }) | (Shape::Round { c, r, .. }, Shape::Line { a, b }) => {
            let d = b - a;
            let f = a - c;
            let (qa, qb, qc) = (d.len2(), 2.0 * f.dot(d), f.len2() - r * r);
            let disc = qb * qb - 4.0 * qa * qc;
            if qa < 1e-24 || disc < 0.0 {
                return Vec::new();
            }
            let sq = disc.sqrt();
            vec![a + d * ((-qb - sq) / (2.0 * qa)), a + d * ((-qb + sq) / (2.0 * qa))]
        }
        (Shape::Round { c: c1, r: r1, .. }, Shape::Round { c: c2, r: r2, .. }) => {
            let dv = c2 - c1;
            let d = dv.len();
            if d < 1e-12 || d > r1 + r2 || d < (r1 - r2).abs() {
                return Vec::new();
            }
            let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
            let h = (r1 * r1 - a * a).max(0.0).sqrt();
            let m = c1 + dv * (a / d);
            let n = dv.perp() / d;
            vec![m + n * h, m - n * h]
        }
    }
}

/// Where two unbounded curves meet, missing by no more than `tol`: a line or circle that touches
/// a circle within `tol` (outside or inside) meets it where it comes closest, as a double root.
/// [`intersections`] takes the roots of the quadratic as they come, and at a tangency these are
/// rounding noise: a line made tangent to a circle by the solver misses it whenever the noise
/// falls outside, so the regions it closes off are never found (#588). Here the root's square
/// root is clamped instead, and the touch comes back as two equal points (which profile
/// detection merges). Circles nearly on one another still cross where they cross. Computed in
/// the circle's own frame (no cancellation on long lines or small circles).
pub fn intersections_within(x: &Shape, y: &Shape, tol: f64) -> Vec<Vec2> {
    match (*x, *y) {
        (Shape::Line { a, b }, Shape::Round { c, r, .. }) | (Shape::Round { c, r, .. }, Shape::Line { a, b }) => {
            let Some(direction) = (b - a).normalized() else { return Vec::new() };
            let foot = a + direction * (c - a).dot(direction);
            let distance = foot.dist(c);
            if distance > r + tol {
                return Vec::new();
            }
            let half = ((r - distance).max(0.0) * (r + distance)).sqrt();
            vec![foot - direction * half, foot + direction * half]
        }
        (Shape::Round { c: c1, r: r1, .. }, Shape::Round { c: c2, r: r2, .. }) => {
            let ((small, r), (large, radius)) = if r1 <= r2 { ((c1, r1), (c2, r2)) } else { ((c2, r2), (c1, r1)) };
            let delta = small - large;
            let distance = delta.len();
            if distance < 1e-12 || distance > radius + r + tol || distance < radius - r - tol {
                return Vec::new();
            }
            let u = delta / distance;
            // Along the line of centres from the large circle; factoring d² - r² avoids the
            // cancellation of R² - a² for a small circle crossing a huge one.
            let along = ((distance - r) * (distance + r) + radius * radius) / (2.0 * distance);
            let height = ((radius - along).max(0.0) * (radius + along).max(0.0)).sqrt();
            let foot = large + u * along.clamp(-radius, radius);
            vec![foot - u.perp() * height, foot + u.perp() * height]
        }
        _ => intersections(x, y),
    }
}

/// How close a computed crossing must lie to both bounded curves to count as on them.
const ON_CURVE: f64 = 1e-4;
/// Curves considered for a snap, so a hostile sketch cannot stall the pointer.
const MAX_INTERSECTION_CANDIDATES: usize = 256;

/// Infinite construction lines use their full support; all other curves keep their bounds.
fn snap_distance(shape: &Shape, infinite: bool, p: Vec2) -> f64 {
    if infinite && matches!(shape, Shape::Line { .. }) { p.dist(shape.project(p)) } else { shape.dist(p) }
}

/// The crossing of `x` and `y` nearest `near` that lies on both bounded curves.
pub fn nearest_crossing(x: &Shape, y: &Shape, near: Vec2) -> Option<Vec2> {
    nearest_crossing_with_extents(x, y, near, [false; 2])
}

fn nearest_crossing_with_extents(x: &Shape, y: &Shape, near: Vec2, infinite: [bool; 2]) -> Option<Vec2> {
    if !near.is_finite() {
        return None;
    }
    let mut best: Option<(f64, Vec2)> = None;
    let [infinite_x, infinite_y] = infinite;
    for q in intersections_within(x, y, 1e-6) {
        if !(q.is_finite() && snap_distance(x, infinite_x, q) <= ON_CURVE && snap_distance(y, infinite_y, q) <= ON_CURVE) {
            continue;
        }
        let d = q.dist(near);
        if best.is_none_or(|prev| d < prev.0) {
            best = Some((d, q));
        }
    }
    best.map(|(_, q)| q)
}

/// The nearest crossing of two lines, circles or arcs of `sk` to `p` within `tol`.
///
/// Skips a crossing that already has a sketch point. Curves farther than `tol` from `p`
/// cannot meet near the cursor and are ignored. Infinite lines use their full support;
/// ordinary segments and arcs keep their bounds.
pub fn nearest_curve_intersection(sk: &Sketch, p: Vec2, tol: f64) -> Option<(Vec2, usize, usize)> {
    if !p.is_finite() || !tol.is_finite() || tol <= 0.0 {
        return None;
    }
    let mut candidates: Vec<(usize, Shape, bool)> = Vec::new();
    for (i, curve) in sk.curves.iter().enumerate() {
        if candidates.len() >= MAX_INTERSECTION_CANDIDATES {
            break;
        }
        let Some(sh) = sk.shape(i) else { continue };
        if snap_distance(&sh, curve.infinite, p) <= tol {
            candidates.push((i, sh, curve.infinite));
        }
    }
    let mut best: Option<(f64, Vec2, usize, usize)> = None;
    for a in 0..candidates.len() {
        for b in a + 1..candidates.len() {
            let Some((ia, sa, infinite_a)) = candidates.get(a) else { continue };
            let Some((ib, sb, infinite_b)) = candidates.get(b) else { continue };
            let Some(q) = nearest_crossing_with_extents(sa, sb, p, [*infinite_a, *infinite_b]) else { continue };
            let d = q.dist(p);
            if d > tol {
                continue;
            }
            if sk.points.iter().any(|pt| pt.pos.dist(q) <= ON_CURVE) {
                continue;
            }
            if best.is_none_or(|prev| d < prev.0) {
                best = Some((d, q, *ia, *ib));
            }
        }
    }
    best.map(|(_, q, i, j)| (q, i, j))
}

impl Shape {
    /// Unbounded curve parameter of `p` (line: t along a→b; round: angle offset from start in
    /// [0, 2π)).
    pub fn param(&self, p: Vec2) -> f64 {
        match *self {
            Shape::Line { a, b } => {
                let d = b - a;
                let l2 = d.len2();
                if l2 < 1e-24 { 0.0 } else { (p - a).dot(d) / l2 }
            }
            Shape::Round { c, start, .. } => ((p - c).angle() - start).rem_euclid(TAU),
        }
    }
    /// Parameter range of the bounded curve.
    pub fn range(&self) -> (f64, f64) {
        match *self {
            Shape::Line { .. } => (0.0, 1.0),
            Shape::Round { sweep, .. } => (0.0, sweep),
        }
    }
    pub fn is_full(&self) -> bool {
        matches!(*self, Shape::Round { sweep, .. } if sweep >= TAU - 1e-12)
    }
    /// Unit tangent at parameter `t` in the direction of increasing `t`.
    pub fn tangent(&self, t: f64) -> Vec2 {
        match *self {
            Shape::Line { a, b } => (b - a).normalized().unwrap_or(Vec2::X),
            Shape::Round { start, .. } => Vec2::from_angle(start + t).perp(),
        }
    }
    /// Closest point on the unbounded curve.
    pub fn project(&self, p: Vec2) -> Vec2 {
        match *self {
            Shape::Line { a, b } => a.lerp(b, self.param(p)),
            Shape::Round { c, r, .. } => c + (p - c).normalized().unwrap_or(Vec2::X) * r,
        }
    }
    /// Distance from `p` to the bounded curve.
    pub fn dist(&self, p: Vec2) -> f64 {
        let (t0, t1) = self.range();
        let t = self.param(p);
        match *self {
            Shape::Line { .. } => p.dist(self.point(t.clamp(t0, t1))),
            Shape::Round { c, r, sweep, .. } => {
                if sweep >= TAU - 1e-12 || t <= sweep {
                    (p.dist(c) - r).abs()
                } else {
                    p.dist(self.point(0.0)).min(p.dist(self.point(sweep)))
                }
            }
        }
    }
}

impl Sketch {
    /// The nearest crossing of two analytic curves, respecting infinite construction lines.
    pub fn nearest_crossing(&self, a: usize, b: usize, near: Vec2) -> Option<Vec2> {
        let infinite = [self.curves.get(a)?.infinite, self.curves.get(b)?.infinite];
        nearest_crossing_with_extents(&self.shape(a)?, &self.shape(b)?, near, infinite)
    }

    /// The analytic shape of a curve (arcs counter-clockwise from their stored start).
    pub fn shape(&self, ci: usize) -> Option<Shape> {
        match self.curves.get(ci)?.kind {
            CurveKind::Line { a, b } => Some(Shape::Line { a: self.point(a)?, b: self.point(b)? }),
            CurveKind::Circle { c, r } => Some(Shape::Round { c: self.point(c)?, r, start: 0.0, sweep: TAU }),
            CurveKind::Arc { .. } => match self.segs(ci).into_iter().next()? {
                Seg2::Arc { center, radius, start, sweep } => Some(Shape::Round { c: center, r: radius, start, sweep }),
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod intersection_snap_tests {
    use super::*;
    use crate::model::Sketch;

    fn v(x: f64, y: f64) -> Vec2 {
        Vec2::new(x, y)
    }

    #[test]
    fn nearest_curve_intersection_finds_crossing_lines() {
        let mut sk = Sketch::new();
        let a = sk.add_line(v(0.0, 0.0), v(20.0, 20.0), None, None, None).unwrap();
        let b = sk.add_line(v(0.0, 20.0), v(20.0, 0.0), None, None, None).unwrap();
        let (p, i, j) = nearest_curve_intersection(&sk, v(10.2, 9.8), 1.0).expect("crossing");
        assert!(p.dist(v(10.0, 10.0)) < 1e-9, "{p:?}");
        assert_eq!([i.min(j), i.max(j)], [a.min(b), a.max(b)]);
    }

    #[test]
    fn infinite_line_crossing_snaps_beyond_both_defining_segments() {
        for reverse in [false, true] {
            let mut sk = Sketch::new();
            let mut lines = [(v(20.0, 40.0), v(20.0, 50.0)), (v(40.0, 15.0), v(50.0, 15.0))];
            if reverse {
                lines.reverse();
            }
            for (a, b) in lines {
                let i = sk.add_line(a, b, None, None, None).unwrap();
                sk.curves[i].construction = true;
            }
            let near = v(20.4, 15.4);
            assert!(nearest_curve_intersection(&sk, near, 1.0).is_none(), "finite construction segments do not cross");
            for curve in &mut sk.curves {
                curve.infinite = true;
            }
            let (at, a, b) = nearest_curve_intersection(&sk, near, 1.0).expect("infinite lines cross");
            assert!(at.dist(v(20.0, 15.0)) < 1e-9, "{at:?}");
            assert_ne!(a, b);
        }
    }

    #[test]
    fn infinite_line_crossing_keeps_the_other_curves_bounds() {
        let mut sk = Sketch::new();
        let line = sk.add_line(v(-6.0, 30.0), v(-6.0, 40.0), None, None, None).unwrap();
        sk.curves[line].infinite = true;
        sk.curves[line].construction = true;
        sk.add_arc(v(0.0, 0.0), v(10.0, 0.0), v(0.0, 10.0), [None; 3], None).unwrap();
        assert!(nearest_curve_intersection(&sk, v(-6.0, 8.0), 1.0).is_none(), "the missing part of the arc is not a crossing");
        let CurveKind::Line { a, b } = sk.curves[line].kind else { panic!("line") };
        sk.points[a].pos.x = 6.0;
        sk.points[b].pos.x = 6.0;
        let (at, _, _) = nearest_curve_intersection(&sk, v(6.2, 8.2), 1.0).expect("the drawn arc crosses the infinite line");
        assert!(at.dist(v(6.0, 8.0)) < 1e-9, "{at:?}");

        let mut sk = Sketch::new();
        let vertical = sk.add_line(v(20.0, 40.0), v(20.0, 50.0), None, None, None).unwrap();
        sk.curves[vertical].infinite = true;
        sk.curves[vertical].construction = true;
        sk.add_line(v(40.0, 15.0), v(50.0, 15.0), None, None, None).unwrap();
        assert!(nearest_curve_intersection(&sk, v(20.4, 15.4), 1.0).is_none(), "the finite line must not extend");
    }

    #[test]
    fn nearest_curve_intersection_skips_a_crossing_that_already_has_a_point() {
        let mut sk = Sketch::new();
        sk.add_line(v(0.0, 0.0), v(20.0, 20.0), None, None, None).unwrap();
        sk.add_line(v(0.0, 20.0), v(20.0, 0.0), None, None, None).unwrap();
        sk.add_point(v(10.0, 10.0), None).unwrap();
        assert!(nearest_curve_intersection(&sk, v(10.1, 10.0), 1.0).is_none());
    }

    #[test]
    fn nearest_curve_intersection_ignores_parallel_lines_and_extensions() {
        let mut sk = Sketch::new();
        sk.add_line(v(0.0, 0.0), v(10.0, 0.0), None, None, None).unwrap();
        sk.add_line(v(0.0, 5.0), v(10.0, 5.0), None, None, None).unwrap();
        assert!(nearest_curve_intersection(&sk, v(5.0, 2.5), 3.0).is_none());
        let mut far = Sketch::new();
        far.add_line(v(0.0, 0.0), v(10.0, 0.0), None, None, None).unwrap();
        far.add_line(v(0.0, 10.0), v(0.0, 20.0), None, None, None).unwrap();
        assert!(nearest_curve_intersection(&far, v(0.0, 0.0), 1.0).is_none(), "the unbounded lines meet at an endpoint off the second segment");
    }

    #[test]
    fn nearest_curve_intersection_picks_the_nearer_circle_crossing() {
        let mut sk = Sketch::new();
        sk.add_circle(v(0.0, 0.0), 10.0, None, None).unwrap();
        sk.add_circle(v(10.0, 0.0), 10.0, None, None).unwrap();
        let hi = v(5.0, 5.0 * 3.0f64.sqrt());
        let (p, _, _) = nearest_curve_intersection(&sk, v(5.0, 8.0), 2.0).expect("upper crossing");
        assert!(p.dist(hi) < 1e-6, "{p:?}");
    }

    #[test]
    fn nearest_curve_intersection_uses_an_arc_s_bounded_sweep() {
        let mut sk = Sketch::new();
        sk.add_arc(v(0.0, 0.0), v(10.0, 0.0), v(0.0, 10.0), [None; 3], None).unwrap();
        sk.add_line(v(-15.0, 0.0), v(-5.0, 0.0), None, None, None).unwrap();
        assert!(nearest_curve_intersection(&sk, v(-10.0, 0.0), 1.0).is_none(), "the missing half of the circle is not a snap");
        let mut on = Sketch::new();
        on.add_arc(v(0.0, 0.0), v(10.0, 0.0), v(0.0, 10.0), [None; 3], None).unwrap();
        on.add_line(v(0.0, 0.0), v(20.0, 20.0), None, None, None).unwrap();
        let (p, _, _) = nearest_curve_intersection(&on, v(7.0, 7.0), 2.0).expect("on the quarter");
        assert!((p.len() - 10.0).abs() < 1e-6 && p.x > 0.0 && p.y > 0.0, "{p:?}");
    }

    #[test]
    fn nearest_curve_intersection_refuses_hostile_cursor_and_tolerance() {
        let mut sk = Sketch::new();
        sk.add_line(v(0.0, 0.0), v(10.0, 0.0), None, None, None).unwrap();
        sk.add_line(v(0.0, 0.0), v(0.0, 10.0), None, None, None).unwrap();
        assert!(nearest_curve_intersection(&sk, Vec2::new(f64::NAN, 0.0), 1.0).is_none());
        assert!(nearest_curve_intersection(&sk, v(0.0, 0.0), 0.0).is_none());
        assert!(nearest_curve_intersection(&sk, v(0.0, 0.0), f64::INFINITY).is_none());
    }
}
