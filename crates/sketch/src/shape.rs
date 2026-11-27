//! Analytic curve shapes for geometry operations (profiles, trim, extend, fillet…).

use solidcraft_geom::prelude::*;
use std::f64::consts::TAU;

use solidcore_record::{Seg2, Vec2};

use crate::model::{ConstraintKind, CurveKind, Sketch};

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

/// Dominant axis (`"horizontal"` or `"vertical"`) of the segment from `a` to `b`.
pub fn axis_mode_between(a: Vec2, b: Vec2) -> &'static str {
    let d = b - a;
    if d.x.abs() >= d.y.abs() { "horizontal" } else { "vertical" }
}

/// The ends of the segment from `a` to `b` turned to horizontal (`horizontal`) or vertical,
/// keeping its length and the way it ran along that axis: about its middle when both ends may
/// move, about the end that may not when one may not (`a_moves`, `b_moves`). `None` when neither
/// may move, or the segment has no length or is not finite.
pub fn turned_to_axis(a: Vec2, b: Vec2, horizontal: bool, a_moves: bool, b_moves: bool) -> Option<(Vec2, Vec2)> {
    let d = b - a;
    let length = d.len();
    if !(a.is_finite() && b.is_finite() && length.is_finite() && length > 0.0) {
        return None;
    }
    let sign = |v: f64| if v < 0.0 { -1.0 } else { 1.0 };
    let along = if horizontal { Vec2::new(length * sign(d.x), 0.0) } else { Vec2::new(0.0, length * sign(d.y)) };
    match (a_moves, b_moves) {
        (true, true) => {
            let mid = (a + b) * 0.5;
            Some((mid - along * 0.5, mid + along * 0.5))
        }
        (false, true) => Some((a, a + along)),
        (true, false) => Some((b - along, b)),
        (false, false) => None,
    }
}

/// Unit direction from `from` toward `to`, or `None` when points coincide or are non-finite.
pub fn unit_direction(from: Vec2, to: Vec2) -> Option<Vec2> {
    (from.is_finite() && to.is_finite()).then(|| (to - from).normalized()).flatten()
}

/// Intersection between two infinite 2D lines `(p0, d0)` and `(p1, d1)`.
pub fn intersect_infinite_lines(p0: Vec2, d0: Vec2, p1: Vec2, d1: Vec2) -> Option<Vec2> {
    if !(p0.is_finite() && d0.is_finite() && p1.is_finite() && d1.is_finite()) {
        return None;
    }
    let u0 = d0.normalized()?;
    let u1 = d1.normalized()?;
    let a = Shape::Line { a: p0, b: p0 + u0 };
    let b = Shape::Line { a: p1, b: p1 + u1 };
    intersections(&a, &b).into_iter().find(|q| q.is_finite())
}

/// Where extension ray `(ext_origin, ext_dir)` meets infinite line `(origin, dir)` ahead of
/// `ext_origin` within `max_dist` of `near`.
pub fn intersect_extension_ray(origin: Vec2, dir: Vec2, ext_origin: Vec2, ext_dir: Vec2, near: Vec2, max_dist: f64) -> Option<Vec2> {
    if !near.is_finite() || !max_dist.is_finite() || max_dist < 0.0 {
        return None;
    }
    let ext_u = ext_dir.normalized()?;
    let q = intersect_infinite_lines(origin, dir, ext_origin, ext_u)?;
    ((q - ext_origin).dot(ext_u) > 1e-6 && q.dist(near) <= max_dist).then_some(q)
}

/// Closest intersection between curve `ci` of `sk` and infinite line `(origin, dir)` near `near`
/// within `max_dist`.
pub fn intersect_curve_with_line(sk: &Sketch, ci: usize, origin: Vec2, dir: Vec2, near: Vec2, max_dist: f64) -> Option<Vec2> {
    if !(origin.is_finite() && dir.is_finite() && near.is_finite() && max_dist.is_finite() && max_dist >= 0.0) {
        return None;
    }
    let u = dir.normalized()?;
    let line = Shape::Line { a: origin, b: origin + u };
    let mut best: Option<(f64, Vec2)> = None;
    let mut offer = |q: Vec2| {
        if q.is_finite() {
            let d = q.dist(near);
            if d <= max_dist && best.is_none_or(|b| d < b.0) {
                best = Some((d, q));
            }
        }
    };
    if let Some(sh) = sk.shape(ci) {
        let infinite = sk.curves.get(ci).is_some_and(|c| c.infinite);
        for q in intersections_within(&line, &sh, 1e-6) {
            if snap_distance(&sh, infinite, q) <= ON_CURVE {
                offer(q);
            }
        }
    } else if sk.curves.get(ci).is_some_and(|c| c.kind.is_freeform()) {
        for w in sk.polyline(ci).windows(2) {
            let seg = Shape::Line { a: w[0], b: w[1] };
            for q in intersections(&line, &seg) {
                if seg.dist(q) <= ON_CURVE {
                    offer(q);
                }
            }
        }
    }
    best.map(|(_, q)| q)
}

/// Whether `pt` lies on the rim of circle or arc `curve_id` in `sk`.
pub fn is_on_circle_rim(sk: &Sketch, curve_id: &str, pt: Vec2) -> bool {
    if !pt.is_finite() {
        return false;
    }
    let Some(ci) = sk.curve_index(curve_id) else { return false };
    let (Some(o), Some(r)) = (sk.center(ci), sk.radius(ci)) else { return false };
    (pt.dist(o) - r).abs() <= 1e-6 * r.max(1.0)
}

/// Whether `p` projects onto segment `a..b` within `margin` past either endpoint.
pub fn point_on_segment_within(a: Vec2, b: Vec2, p: Vec2, margin: f64) -> bool {
    if !(a.is_finite() && b.is_finite() && p.is_finite() && margin.is_finite()) {
        return false;
    }
    let Some(dir) = (b - a).normalized() else { return false };
    let len = a.dist(b);
    let s = (p - a).dot(dir);
    s >= -margin && s <= len + margin
}

fn already_tangent(sk: &Sketch, c1: usize, c2: usize) -> bool {
    c1 == c2
        || sk
            .constraints
            .iter()
            .any(|c| !c.driven && matches!(c.kind, ConstraintKind::Tangent { a, b } if (a == c1 && b == c2) || (a == c2 && b == c1)))
}

/// Closest curve-to-curve tangent drag snap for a dragged line or resized circle/arc.
#[derive(Clone, Debug, PartialEq)]
pub struct TangentDragHit {
    pub target: String,
    pub at: Vec2,
    pub offset: Vec2,
    pub gap: f64,
}

/// Find the nearest curve-to-curve tangent snap for dragging `dragged_entity` (a line moved by
/// `delta`, or a circle/arc resized toward `cursor`), within `curve_tol` of tangency and
/// `cursor_tol` of `cursor`. Curves using any point in `exclude` or already tangent to
/// `dragged_entity` are skipped.
pub fn find_tangent_drag(
    sk: &Sketch,
    dragged_entity: &str,
    cursor: Vec2,
    delta: Vec2,
    exclude: &[usize],
    curve_tol: f64,
    cursor_tol: f64,
) -> Option<TangentDragHit> {
    if !(cursor.is_finite() && delta.is_finite() && curve_tol.is_finite() && curve_tol > 0.0 && cursor_tol.is_finite() && cursor_tol > 0.0) {
        return None;
    }
    let dragged_ci = sk.curve_index(dragged_entity)?;
    let dragged_curve = sk.curves.get(dragged_ci)?;
    let mut best: Option<TangentDragHit> = None;
    let mut offer = |hit: TangentDragHit| {
        if best.as_ref().is_none_or(|b| hit.gap < b.gap) {
            best = Some(hit);
        }
    };
    match dragged_curve.kind {
        CurveKind::Line { a, b } => {
            let (Some(pa), Some(pb)) = (sk.point(a), sk.point(b)) else { return None };
            let (a_m, b_m) = (pa + delta, pb + delta);
            let dir = (b_m - a_m).normalized()?;
            let n = dir.perp();
            for (c_idx, cu) in sk.curves.iter().enumerate() {
                if c_idx == dragged_ci
                    || !matches!(cu.kind, CurveKind::Circle { .. } | CurveKind::Arc { .. })
                    || exclude.iter().any(|&p| cu.kind.uses(p))
                    || already_tangent(sk, dragged_ci, c_idx)
                {
                    continue;
                }
                let (Some(o), Some(r)) = (sk.center(c_idx), sk.radius(c_idx)) else { continue };
                if r <= 1e-6 {
                    continue;
                }
                let sd = (o - a_m).dot(n);
                if sd.abs() <= 1e-6 {
                    continue;
                }
                let gap = (sd.abs() - r).abs();
                if gap >= curve_tol {
                    continue;
                }
                let tan_pt = o - n * (sd.signum() * r);
                let foot = o - n * sd;
                let on_arc = sk.shape(c_idx).is_none_or(|sh| sh.dist(tan_pt) < 1e-4);
                if !on_arc || tan_pt.dist(cursor) > cursor_tol {
                    continue;
                }
                let offset = tan_pt - foot;
                if !point_on_segment_within(a_m + offset, b_m + offset, tan_pt, curve_tol) {
                    continue;
                }
                offer(TangentDragHit { target: cu.id.clone(), at: tan_pt, offset, gap });
            }
        }
        CurveKind::Circle { .. } | CurveKind::Arc { .. } => {
            let o = sk.center(dragged_ci)?;
            let r_new = cursor.dist(o);
            let radial = (cursor - o).normalized()?;
            for (c_idx, cu) in sk.curves.iter().enumerate() {
                if c_idx == dragged_ci || exclude.iter().any(|&p| cu.kind.uses(p)) || already_tangent(sk, dragged_ci, c_idx) {
                    continue;
                }
                match cu.kind {
                    CurveKind::Line { a, b } => {
                        let (Some(pa), Some(pb)) = (sk.point(a), sk.point(b)) else { continue };
                        let Some(dir) = (pb - pa).normalized() else { continue };
                        let n = dir.perp();
                        let sd = (o - pa).dot(n);
                        let r_tan = sd.abs();
                        let gap = (r_new - r_tan).abs();
                        if r_tan <= 1e-6 || gap >= curve_tol {
                            continue;
                        }
                        let foot = o - n * sd;
                        if !point_on_segment_within(pa, pb, foot, curve_tol) || foot.dist(cursor) > cursor_tol {
                            continue;
                        }
                        let offset = radial * (r_tan - r_new);
                        offer(TangentDragHit { target: cu.id.clone(), at: foot, offset, gap });
                    }
                    CurveKind::Circle { .. } | CurveKind::Arc { .. } => {
                        let (Some(o2), Some(r2)) = (sk.center(c_idx), sk.radius(c_idx)) else { continue };
                        let d_c = o.dist(o2);
                        if r2 <= 1e-6 || d_c <= 1e-6 {
                            continue;
                        }
                        let u = (o2 - o) / d_c;
                        for (r_tan, tan_pt) in [(d_c - r2, o2 - u * r2), (d_c + r2, o2 + u * r2), (r2 - d_c, o2 - u * r2)] {
                            if r_tan <= 1e-6 {
                                continue;
                            }
                            let gap = (r_new - r_tan).abs();
                            if gap >= curve_tol || tan_pt.dist(cursor) > cursor_tol {
                                continue;
                            }
                            let on_arc = sk.shape(c_idx).is_none_or(|sh| sh.dist(tan_pt) < 1e-4);
                            if !on_arc {
                                continue;
                            }
                            let offset = radial * (r_tan - r_new);
                            offer(TangentDragHit { target: cu.id.clone(), at: tan_pt, offset, gap });
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    best
}

/// Whether two curves in `sk` (line–circle/arc or circle/arc–circle/arc) are geometrically tangent
/// within `tol`.
pub fn curves_tangent_within(sk: &Sketch, first: &str, second: &str, tol: f64) -> bool {
    if !tol.is_finite() || tol < 0.0 {
        return false;
    }
    let (Some(c1), Some(c2)) = (sk.curve_index(first), sk.curve_index(second)) else { return false };
    let (Some(cu1), Some(cu2)) = (sk.curves.get(c1), sk.curves.get(c2)) else { return false };
    let line_circle_ok = |line_c: usize, circ_c: usize| -> bool {
        let Some(CurveKind::Line { a, b }) = sk.curves.get(line_c).map(|c| &c.kind) else { return false };
        let (Some(pa), Some(pb), Some(o), Some(r)) = (sk.point(*a), sk.point(*b), sk.center(circ_c), sk.radius(circ_c)) else {
            return false;
        };
        let Some(dir) = (pb - pa).normalized() else { return false };
        let sd = (o - pa).dot(dir.perp()).abs();
        (sd - r).abs() <= tol
    };
    match (&cu1.kind, &cu2.kind) {
        (CurveKind::Line { .. }, CurveKind::Circle { .. } | CurveKind::Arc { .. }) => line_circle_ok(c1, c2),
        (CurveKind::Circle { .. } | CurveKind::Arc { .. }, CurveKind::Line { .. }) => line_circle_ok(c2, c1),
        (CurveKind::Circle { .. } | CurveKind::Arc { .. }, CurveKind::Circle { .. } | CurveKind::Arc { .. }) => {
            let (Some(o1), Some(r1), Some(o2), Some(r2)) = (sk.center(c1), sk.radius(c1), sk.center(c2), sk.radius(c2)) else {
                return false;
            };
            let d = o1.dist(o2);
            (d - (r1 + r2)).abs() <= tol || (d - (r1 - r2).abs()).abs() <= tol
        }
        _ => false,
    }
}

/// Kind of line-based drawing inference computed from existing sketch lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineInferenceKind {
    Parallel,
    Perpendicular,
    Extension,
}

/// Geometric result of a line direction or extension inference.
#[derive(Clone, Debug, PartialEq)]
pub struct LineInference {
    pub kind: LineInferenceKind,
    pub curve: String,
    pub at: Vec2,
    pub dist: f64,
    pub guide: (Vec2, Vec2),
}

fn line_ends(sk: &Sketch, c: usize) -> Option<(Vec2, Vec2)> {
    match sk.curves.get(c)?.kind {
        CurveKind::Line { a, b } => Some((sk.point(a)?, sk.point(b)?)),
        _ => None,
    }
}

/// Parallel or perpendicular direction inferred from existing non-axis lines in `sk`, starting at
/// `prev` with cursor `lp` within `tol`.
pub fn infer_line_direction(sk: &Sketch, prev: Vec2, lp: Vec2, tol: f64) -> Option<LineInference> {
    if !(prev.is_finite() && lp.is_finite() && tol.is_finite() && tol > 0.0) {
        return None;
    }
    let v = lp - prev;
    if v.len() < tol * 2.0 {
        return None;
    }
    let mut best: Option<LineInference> = None;
    let mut offer = |i: LineInference| {
        if best.as_ref().is_none_or(|b| i.dist < b.dist) {
            best = Some(i);
        }
    };
    for (i, c) in sk.curves.iter().enumerate() {
        if c.construction && c.link.is_none() {
            continue;
        }
        let Some((a, b)) = line_ends(sk, i) else { continue };
        let Some(dir) = (b - a).normalized() else { continue };
        // Axis-aligned directions are the H/V snaps' business.
        let axis = dir.x.abs() < 1e-9 || dir.y.abs() < 1e-9;
        let touches = a.dist(prev) < 1e-9 || b.dist(prev) < 1e-9;
        for (kind, d) in [(LineInferenceKind::Parallel, dir), (LineInferenceKind::Perpendicular, dir.perp())] {
            if axis || (kind == LineInferenceKind::Parallel && touches) {
                continue;
            }
            let t = v.dot(d);
            let at = prev + d * t;
            let e = at.dist(lp);
            if e < tol {
                offer(LineInference {
                    kind,
                    curve: c.id.clone(),
                    at,
                    dist: e,
                    guide: (prev - d * (t.signum() * tol * 6.0), at + d * (t.signum() * tol * 6.0)),
                });
            }
        }
    }
    best
}

/// Point locus along the infinite extension of an existing line in `sk` beyond its ends, when
/// drawing from `prev` toward `lp` within `tol`.
pub fn infer_line_extension(sk: &Sketch, prev: Vec2, lp: Vec2, tol: f64) -> Option<LineInference> {
    if !(prev.is_finite() && lp.is_finite() && tol.is_finite() && tol > 0.0) {
        return None;
    }
    let v = lp - prev;
    if v.len() < tol * 2.0 {
        return None;
    }
    let mut best: Option<LineInference> = None;
    for (i, c) in sk.curves.iter().enumerate() {
        if c.construction && c.link.is_none() {
            continue;
        }
        let Some((a, b)) = line_ends(sk, i) else { continue };
        let Some(dir) = (b - a).normalized() else { continue };
        let touches = a.dist(prev) < 1e-9 || b.dist(prev) < 1e-9;
        let s = (lp - a).dot(dir);
        let len = a.dist(b);
        if !(-tol..=len + tol).contains(&s) {
            let at = a + dir * s;
            let e = at.dist(lp);
            if e < tol * 0.8 && !touches {
                let from = if s < 0.0 { a } else { b };
                let dist = e * 0.9;
                if best.as_ref().is_none_or(|b| dist < b.dist) {
                    best = Some(LineInference { kind: LineInferenceKind::Extension, curve: c.id.clone(), at, dist, guide: (from, at) });
                }
            }
        }
    }
    best
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
mod turned_to_axis_tests {
    use super::*;

    /// A vertical, a steep and a reversed segment turned to horizontal, and the mirror cases
    /// turned to vertical: the length and the way along the axis stay, about the middle, or about
    /// the end that may not move.
    #[test]
    fn a_segment_turns_to_the_axis_keeping_its_length() {
        let v = Vec2::new;
        for (a, b, horizontal) in [
            (v(10.0, 10.0), v(10.0, 40.0), true),
            (v(10.0, 10.0), v(13.0, 40.0), true),
            (v(13.0, 40.0), v(10.0, 10.0), true),
            (v(10.0, 10.0), v(40.0, 10.0), false),
            (v(10.0, 10.0), v(40.0, 13.0), false),
        ] {
            let length = (b - a).len();
            for (a_moves, b_moves) in [(true, true), (false, true), (true, false)] {
                let (na, nb) = turned_to_axis(a, b, horizontal, a_moves, b_moves).unwrap();
                let d = nb - na;
                let case = format!("{a:?}-{b:?} horizontal {horizontal}, moves {a_moves} {b_moves}: {na:?} {nb:?}");
                assert!((d.len() - length).abs() < 1e-9, "{case}");
                let (across, along, was) = if horizontal { (d.y, d.x, (b - a).x) } else { (d.x, d.y, (b - a).y) };
                assert!(across.abs() < 1e-12 && along * was.signum() > 0.0, "{case}");
                match (a_moves, b_moves) {
                    (false, _) => assert_eq!(na, a, "{case}"),
                    (_, false) => assert_eq!(nb, b, "{case}"),
                    _ => assert!(((na + nb) * 0.5 - (a + b) * 0.5).len() < 1e-9, "{case}"),
                }
            }
        }
        assert!(turned_to_axis(v(0.0, 0.0), v(0.0, 5.0), true, false, false).is_none());
        assert!(turned_to_axis(v(1.0, 1.0), v(1.0, 1.0), true, true, true).is_none());
        assert!(turned_to_axis(v(f64::NAN, 0.0), v(0.0, 5.0), true, true, true).is_none());
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

    #[test]
    fn snap_line_intersection_and_direction_helpers_work() {
        assert_eq!(axis_mode_between(v(0.0, 0.0), v(10.0, 3.0)), "horizontal");
        assert_eq!(axis_mode_between(v(0.0, 0.0), v(2.0, 10.0)), "vertical");
        assert!(unit_direction(v(1.0, 1.0), v(1.0, 1.0)).is_none());
        let dir = unit_direction(v(0.0, 0.0), v(3.0, 4.0)).expect("unit dir");
        assert!((dir.x - 0.6).abs() < 1e-9 && (dir.y - 0.8).abs() < 1e-9);

        let hit = intersect_infinite_lines(v(0.0, 10.0), v(1.0, 0.0), v(25.0, 0.0), v(0.0, 1.0)).expect("crossing");
        assert!(hit.dist(v(25.0, 10.0)) < 1e-9);
        assert!(intersect_infinite_lines(v(0.0, 0.0), v(1.0, 0.0), v(0.0, 5.0), v(2.0, 0.0)).is_none());
        assert!(intersect_extension_ray(v(0.0, 10.0), v(1.0, 0.0), v(25.0, 0.0), v(0.0, 1.0), v(25.0, 10.0), 1.0).is_some());
        assert!(intersect_extension_ray(v(0.0, 10.0), v(1.0, 0.0), v(25.0, 20.0), v(0.0, 1.0), v(25.0, 10.0), 1.0).is_none());

        let mut sk = Sketch::new();
        let ci = sk.add_circle(v(0.0, 0.0), 10.0, None, None).unwrap();
        assert!(is_on_circle_rim(&sk, &sk.curves[ci].id, v(10.0, 0.0)));
        assert!(!is_on_circle_rim(&sk, &sk.curves[ci].id, v(5.0, 0.0)));
        let q = intersect_curve_with_line(&sk, ci, v(0.0, 0.0), v(1.0, 0.0), v(9.5, 0.2), 2.0).expect("circle hit");
        assert!(q.dist(v(10.0, 0.0)) < 1e-6);
    }

    #[test]
    fn tangent_drag_and_tangency_check_helpers_work() {
        assert!(point_on_segment_within(v(0.0, 0.0), v(10.0, 0.0), v(5.0, 2.0), 0.5));
        assert!(!point_on_segment_within(v(0.0, 0.0), v(10.0, 0.0), v(12.0, 0.0), 0.5));
        assert!(!point_on_segment_within(v(0.0, 0.0), v(0.0, 0.0), v(0.0, 0.0), 0.5));

        let mut sk = Sketch::new();
        let _ = sk.add_line(v(0.0, 10.0), v(40.0, 10.0), None, None, Some("l1"));
        let _ = sk.add_circle(v(20.0, 0.0), 10.0, None, Some("c1"));
        assert!(curves_tangent_within(&sk, "l1", "c1", 1e-6));

        let hit = find_tangent_drag(&sk, "l1", v(20.0, 10.3), v(0.0, 0.3), &[], 0.6, 2.4).expect("line tangent drag");
        assert_eq!(hit.target, "c1");
        assert!(hit.at.dist(v(20.0, 10.0)) < 1e-6);
        assert!((hit.offset.y + 0.3).abs() < 1e-6);
        assert!(find_tangent_drag(&sk, "l1", v(35.0, 10.3), v(0.0, 0.3), &[], 0.6, 2.4).is_none());

        let mut sk2 = Sketch::new();
        let _ = sk2.add_circle(v(0.0, 0.0), 8.0, None, Some("c1"));
        let _ = sk2.add_circle(v(20.0, 0.0), 10.0, None, Some("c2"));
        assert!(!curves_tangent_within(&sk2, "c1", "c2", 1e-3));
        let hit2 = find_tangent_drag(&sk2, "c1", v(9.8, 0.0), Vec2::ZERO, &[], 0.6, 2.4).expect("circle tangent drag");
        assert_eq!(hit2.target, "c2");
        assert!(hit2.at.dist(v(10.0, 0.0)) < 1e-6);
        assert!((hit2.offset.x - 0.2).abs() < 1e-6);
    }

    #[test]
    fn line_direction_and_extension_inference_helpers_work() {
        let mut sk = Sketch::new();
        let _ = sk.add_line(v(0.0, 0.0), v(10.0, 10.0), None, None, Some("l1"));

        let par = infer_line_direction(&sk, v(0.0, 20.0), v(10.2, 29.9), 0.5).expect("parallel");
        assert_eq!(par.kind, LineInferenceKind::Parallel);
        assert_eq!(par.curve, "l1");
        assert!((par.at - v(0.0, 20.0)).cross(v(1.0, 1.0)).abs() < 1e-9);

        let perp = infer_line_direction(&sk, v(0.0, 20.0), v(10.1, 10.0), 0.5).expect("perpendicular");
        assert_eq!(perp.kind, LineInferenceKind::Perpendicular);

        let ext = infer_line_extension(&sk, v(30.0, 15.0), v(20.1, 19.9), 0.5).expect("extension");
        assert_eq!(ext.kind, LineInferenceKind::Extension);
        assert!((ext.at.x - ext.at.y).abs() < 1e-9 && ext.at.x > 10.0);

        assert!(infer_line_direction(&sk, v(f64::NAN, 0.0), v(10.0, 10.0), 0.5).is_none());
        assert!(infer_line_extension(&sk, v(0.0, 0.0), v(20.0, 20.0), 0.0).is_none());
    }
}
