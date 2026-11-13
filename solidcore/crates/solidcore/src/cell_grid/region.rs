//! Plane geometry for the cell grid planner: straight pieces and circular arcs, loops welded at
//! their joints, regions (fields minus keep-outs) and convex polygons.
//!
//! Containment counts ray crossings half-open in y. That is only consistent when both sides of
//! every joint agree on its coordinates exactly, so a loop welds its joints to one shared point
//! and an arc keeps the end points it was given. (A circle's seam computed as sin 0 on one side
//! and sin 2π on the other would otherwise lose a row of cells.)

use std::collections::BTreeMap;
use std::f64::consts::{FRAC_PI_2, TAU};

use crate::math::Vec2;

use super::vec::{ring_next, ring_prev, Vec2Ext};
use crate::prism::profile::cross2;

/// Boundary tolerance (mm): geometry closer than this to a boundary counts as on it.
pub(crate) const EPS: f64 = 1e-6;
/// Joints closer than this (mm) are welded to one exact point.
pub(crate) const JOINT_TOL: f64 = 1e-6;
/// Most boundary pieces a region holds; more is refused before anything is planned.
pub(crate) const MAX_PRIMS: usize = 100_000;
/// Most loops a field holds.
pub(crate) const MAX_LOOPS: usize = 2_000;

/// The rectangle `x0..x1` by `y0..y1`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Bounds {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Bounds {
    pub fn of_points(points: impl IntoIterator<Item = Vec2>) -> Option<Bounds> {
        let mut it = points.into_iter();
        let first = it.next()?;
        let mut b = Bounds {
            x0: first.x,
            y0: first.y,
            x1: first.x,
            y1: first.y,
        };
        for p in it {
            b.x0 = b.x0.min(p.x);
            b.y0 = b.y0.min(p.y);
            b.x1 = b.x1.max(p.x);
            b.y1 = b.y1.max(p.y);
        }
        Some(b)
    }

    pub fn union(self, o: Bounds) -> Bounds {
        Bounds {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }

    pub fn overlaps(&self, o: &Bounds, tol: f64) -> bool {
        self.x0 <= o.x1 + tol
            && o.x0 <= self.x1 + tol
            && self.y0 <= o.y1 + tol
            && o.y0 <= self.y1 + tol
    }

    pub fn grown(self, by: f64) -> Bounds {
        Bounds {
            x0: self.x0 - by,
            y0: self.y0 - by,
            x1: self.x1 + by,
            y1: self.y1 + by,
        }
    }
}

// --- convex polygons ---

/// The part of the convex polygon `poly` with `nrm . x >= c` (Sutherland-Hodgman).
pub(crate) fn clip_halfplane(poly: &[Vec2], nrm: Vec2, c: f64) -> Vec<Vec2> {
    let mut out = Vec::new();
    let n = poly.len();
    for k in 0..n {
        let (Some(&p), Some(&q)) = (poly.get(k), poly.get(ring_next(k, n))) else {
            continue;
        };
        let (fp, fq) = (nrm.dot(p) - c, nrm.dot(q) - c);
        if fp >= 0.0 {
            out.push(p);
        }
        if (fp >= 0.0) != (fq >= 0.0) {
            let t = fp / (fp - fq);
            out.push(Vec2::new(p.x + t * (q.x - p.x), p.y + t * (q.y - p.y)));
        }
    }
    dedupe(out)
}

/// The points with neighbours closer than 1e-12 (the last against the first too) dropped.
pub(crate) fn dedupe(poly: Vec<Vec2>) -> Vec<Vec2> {
    let tol = 1e-12;
    let mut out: Vec<Vec2> = Vec::new();
    for p in poly {
        if out.last().is_none_or(|q| p.distance(*q) > tol) {
            out.push(p);
        }
    }
    if out.len() > 1 {
        if let (Some(f), Some(l)) = (out.first(), out.last()) {
            if f.distance(*l) <= tol {
                out.pop();
            }
        }
    }
    out
}

/// Area of the counter-clockwise polygon.
///
/// Summed about the first corner: about the origin, a small loop far from it loses its area to
/// cancellation.
pub(crate) fn poly_area(poly: &[Vec2]) -> f64 {
    let n = poly.len();
    let o = poly.first().copied().unwrap_or(Vec2::new(0.0, 0.0));
    let mut a = 0.0;
    for k in 0..n {
        if let (Some(&p), Some(&q)) = (poly.get(k), poly.get(ring_next(k, n))) {
            a += cross2(p - o, q - o);
        }
    }
    0.5 * a
}

/// Centroid of the polygon (the mean of the corners when it has no area).
pub(crate) fn poly_centroid(poly: &[Vec2]) -> Vec2 {
    let n = poly.len();
    let o = poly.first().copied().unwrap_or(Vec2::new(0.0, 0.0));
    let (mut a, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for k in 0..n {
        if let (Some(&p0), Some(&q0)) = (poly.get(k), poly.get(ring_next(k, n))) {
            let (p, q) = (p0 - o, q0 - o);
            let w = cross2(p, q);
            a += w;
            cx += (p.x + q.x) * w;
            cy += (p.y + q.y) * w;
        }
    }
    if a.abs() < 1e-300 {
        let m = n.max(1) as f64;
        return Vec2::new(
            poly.iter().map(|p| p.x).sum::<f64>() / m,
            poly.iter().map(|p| p.y).sum::<f64>() / m,
        );
    }
    o + Vec2::new(cx / (3.0 * a), cy / (3.0 * a))
}

/// The point where the lines `n1 . x = c1` and `n2 . x = c2` meet (`None` when parallel).
pub(crate) fn line_intersection(n1: Vec2, c1: f64, n2: Vec2, c2: f64) -> Option<Vec2> {
    let det = n1.x * n2.y - n1.y * n2.x;
    if det.abs() < 1e-12 {
        return None;
    }
    Some(Vec2::new(
        (c1 * n2.y - c2 * n1.y) / det,
        (n1.x * c2 - n2.x * c1) / det,
    ))
}

/// A convex polygon (counter-clockwise) with its inward half-planes `n . x >= c`.
#[derive(Clone, Debug)]
pub(crate) struct Convex {
    pub pts: Vec<Vec2>,
    hp: Vec<(Vec2, f64)>,
    pub bounds: Bounds,
}

impl Convex {
    /// `None` for fewer than three corners, a repeated corner or a corner that is not finite.
    pub fn new(pts: Vec<Vec2>) -> Option<Convex> {
        let n = pts.len();
        if n < 3 {
            return None;
        }
        let mut hp = Vec::new();
        for k in 0..n {
            let (a, b) = (*pts.get(k)?, *pts.get(ring_next(k, n))?);
            let d = b - a;
            let ln = d.length();
            if !(ln > 0.0 && ln.is_finite()) {
                return None;
            }
            let nrm = Vec2::new(-d.y / ln, d.x / ln);
            hp.push((nrm, nrm.dot(a)));
        }
        let bounds = Bounds::of_points(pts.iter().copied())?;
        Some(Convex { pts, hp, bounds })
    }

    /// The polygon moved inwards by `eps` on every side (itself when that is not possible).
    pub fn shrunk(&self, eps: f64) -> Convex {
        let n = self.hp.len();
        let mut pts = Vec::new();
        for k in 0..n {
            let (Some(&(n1, c1)), Some(&(n2, c2))) = (self.hp.get(ring_prev(k, n)), self.hp.get(k))
            else {
                return self.clone();
            };
            match line_intersection(n1, c1 + eps, n2, c2 + eps) {
                Some(q) if q.is_finite() => pts.push(q),
                _ => return self.clone(),
            }
        }
        Convex::new(pts).unwrap_or_else(|| self.clone())
    }

    pub fn contains(&self, p: Vec2, eps: f64) -> bool {
        self.hp.iter().all(|&(n, c)| n.dot(p) >= c - eps)
    }

    /// Whether the segment `ab` meets the closed polygon (Cyrus-Beck).
    pub fn meets_segment(&self, a: Vec2, b: Vec2) -> bool {
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        for &(nrm, c) in &self.hp {
            let (fa, fb) = (nrm.dot(a) - c, nrm.dot(b) - c);
            if fa < 0.0 && fb < 0.0 {
                return false;
            }
            if fa >= 0.0 && fb >= 0.0 {
                continue;
            }
            let t = fa / (fa - fb);
            if fa < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
        true
    }
}

/// Points where the circle (`c`, `r`) meets the closed segment `ab`, up to two.
pub(crate) fn circle_segment_points(c: Vec2, r: f64, a: Vec2, b: Vec2) -> ([Vec2; 2], usize) {
    let mut out = [Vec2::ZERO; 2];
    let mut n = 0;
    let d = b - a;
    let f = a - c;
    let qa = d.dot(d);
    if qa == 0.0 {
        return (out, 0);
    }
    let qb = 2.0 * f.dot(d);
    let qc = f.dot(f) - r * r;
    let disc = qb * qb - 4.0 * qa * qc;
    if !(disc >= 0.0) {
        return (out, 0);
    }
    let sq = disc.sqrt();
    for t in [(-qb - sq) / (2.0 * qa), (-qb + sq) / (2.0 * qa)] {
        if (-1e-12..=1.0 + 1e-12).contains(&t) {
            if let Some(slot) = out.get_mut(n) {
                *slot = Vec2::new(a.x + t * d.x, a.y + t * d.y);
                n += 1;
            }
        }
    }
    (out, n)
}

// --- boundary pieces ---

/// One piece of a boundary: a straight segment, or a circular arc from angle `a0` sweeping
/// `sweep` radians (signed) about `c`, whose end points `p0` and `p1` are kept as given.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Prim {
    /// `lattice` marks a segment that lies on a lattice line.
    Line { a: Vec2, b: Vec2, lattice: bool },
    Arc {
        c: Vec2,
        r: f64,
        a0: f64,
        sweep: f64,
        p0: Vec2,
        p1: Vec2,
    },
}

fn arc_point(c: Vec2, r: f64, a: f64) -> Vec2 {
    Vec2::new(c.x + r * a.cos(), c.y + r * a.sin())
}

/// Angles `base + k * pi/2` strictly inside the open sweep `lo..hi`, up to 8 of them.
fn quarter_angles(lo: f64, hi: f64, base: f64) -> ([f64; 8], usize) {
    let mut out = [0.0; 8];
    let mut n = 0;
    if !(lo.is_finite() && hi.is_finite()) {
        return (out, 0);
    }
    let k0 = ((lo - base) / FRAC_PI_2).ceil();
    for step in 0..9 {
        let a = base + (k0 + f64::from(step)) * FRAC_PI_2;
        if a >= hi {
            break;
        }
        if a > lo {
            if let Some(slot) = out.get_mut(n) {
                *slot = a;
                n += 1;
            }
        }
    }
    (out, n)
}

impl Prim {
    pub fn line(a: Vec2, b: Vec2) -> Prim {
        Prim::Line {
            a,
            b,
            lattice: false,
        }
    }

    /// An arc with its end points worked out from the angles.
    pub fn arc(c: Vec2, r: f64, a0: f64, sweep: f64) -> Prim {
        Prim::Arc {
            c,
            r,
            a0,
            sweep,
            p0: arc_point(c, r, a0),
            p1: arc_point(c, r, a0 + sweep),
        }
    }

    pub fn start(&self) -> Vec2 {
        match *self {
            Prim::Line { a, .. } => a,
            Prim::Arc { p0, .. } => p0,
        }
    }

    pub fn end(&self) -> Vec2 {
        match *self {
            Prim::Line { b, .. } => b,
            Prim::Arc { p1, .. } => p1,
        }
    }

    pub fn is_lattice(&self) -> bool {
        matches!(self, Prim::Line { lattice: true, .. })
    }

    pub fn with_start(&self, p: Vec2) -> Prim {
        match *self {
            Prim::Line { b, lattice, .. } => Prim::Line { a: p, b, lattice },
            Prim::Arc {
                c,
                r,
                a0,
                sweep,
                p1,
                ..
            } => Prim::Arc {
                c,
                r,
                a0,
                sweep,
                p0: p,
                p1,
            },
        }
    }

    pub fn reversed(&self) -> Prim {
        match *self {
            Prim::Line { a, b, lattice } => Prim::Line {
                a: b,
                b: a,
                lattice,
            },
            Prim::Arc {
                c,
                r,
                a0,
                sweep,
                p0,
                p1,
            } => Prim::Arc {
                c,
                r,
                a0: a0 + sweep,
                sweep: -sweep,
                p0: p1,
                p1: p0,
            },
        }
    }

    /// The piece moved by the rotation of `angle` radians about the origin and then by `shift`.
    pub fn moved(&self, angle: f64, shift: Vec2) -> Prim {
        let (s, c) = angle.sin_cos();
        let f = |p: Vec2| Vec2::new(c * p.x - s * p.y, s * p.x + c * p.y) + shift;
        match *self {
            Prim::Line { a, b, lattice } => Prim::Line {
                a: f(a),
                b: f(b),
                lattice,
            },
            Prim::Arc {
                c: ctr,
                r,
                a0,
                sweep,
                p0,
                p1,
            } => Prim::Arc {
                c: f(ctr),
                r,
                a0: a0 + angle,
                sweep,
                p0: f(p0),
                p1: f(p1),
            },
        }
    }

    pub fn length(&self) -> f64 {
        match *self {
            Prim::Line { a, b, .. } => a.distance(b),
            Prim::Arc { r, sweep, .. } => r * sweep.abs(),
        }
    }

    pub fn bounds(&self) -> Bounds {
        match *self {
            Prim::Line { a, b, .. } => Bounds {
                x0: a.x.min(b.x),
                y0: a.y.min(b.y),
                x1: a.x.max(b.x),
                y1: a.y.max(b.y),
            },
            Prim::Arc {
                c,
                r,
                a0,
                sweep,
                p0,
                p1,
            } => {
                let mut b = Bounds {
                    x0: p0.x.min(p1.x),
                    y0: p0.y.min(p1.y),
                    x1: p0.x.max(p1.x),
                    y1: p0.y.max(p1.y),
                };
                let (lo, hi) = if sweep >= 0.0 {
                    (a0, a0 + sweep)
                } else {
                    (a0 + sweep, a0)
                };
                let (angles, n) = quarter_angles(lo, hi, 0.0);
                for &a in angles.iter().take(n) {
                    let q = arc_point(c, r, a);
                    b.x0 = b.x0.min(q.x);
                    b.y0 = b.y0.min(q.y);
                    b.x1 = b.x1.max(q.x);
                    b.y1 = b.y1.max(q.y);
                }
                b
            }
        }
    }

    /// This piece's term of the loop's signed area, taken about `o` (any point of the loop: the
    /// sum is the same, but about a far origin a small loop's terms cancel to nothing).
    pub fn area_term(&self, o: Vec2) -> f64 {
        match *self {
            Prim::Line { a, b, .. } => 0.5 * cross2(a - o, b - o),
            Prim::Arc {
                r, sweep, p0, p1, ..
            } => 0.5 * cross2(p0 - o, p1 - o) + 0.5 * r * r * (sweep - sweep.sin()),
        }
    }

    /// Whether direction `a` (radians) lies on the arc (always false for a segment).
    pub fn contains_angle(&self, a: f64, tol: f64) -> bool {
        match *self {
            Prim::Line { .. } => false,
            Prim::Arc { a0, sweep, .. } => {
                if sweep >= 0.0 {
                    let d = (a - a0).rem_euclid(TAU);
                    d <= sweep + tol || d >= TAU - tol
                } else {
                    let d = (a0 - a).rem_euclid(TAU);
                    d <= -sweep + tol || d >= TAU - tol
                }
            }
        }
    }

    /// Pieces monotone in y, cut at the top and bottom of the circle, as
    /// `(angle a, angle b, y at a, y at b)`. The outer ends take their y from the stored end
    /// points, so they agree exactly with the neighbouring pieces.
    fn monotone_pieces(&self) -> ([(f64, f64, f64, f64); 5], usize) {
        let mut out = [(0.0, 0.0, 0.0, 0.0); 5];
        let Prim::Arc {
            c,
            r,
            a0,
            sweep,
            p0,
            p1,
        } = *self
        else {
            return (out, 0);
        };
        let (lo, hi) = if sweep >= 0.0 {
            (a0, a0 + sweep)
        } else {
            (a0 + sweep, a0)
        };
        let (quarters, nq) = quarter_angles(lo, hi, FRAC_PI_2);
        let mut cuts = [0.0f64; 8];
        let mut nc = 0;
        for &a in quarters.iter().take(nq) {
            if a.sin().abs() > 0.5 {
                if let Some(slot) = cuts.get_mut(nc) {
                    *slot = a;
                    nc += 1;
                }
            }
        }
        let Some(cuts) = cuts.get_mut(..nc) else {
            return (out, 0);
        };
        if sweep < 0.0 {
            cuts.reverse();
        }
        // Angles and heights along the arc: the start, the cuts, the end.
        let mut angles = [0.0f64; 7];
        let mut ys = [0.0f64; 7];
        angles[0] = a0;
        ys[0] = p0.y;
        let mut n = 1;
        for &a in cuts.iter() {
            if let (Some(sa), Some(sy)) = (angles.get_mut(n), ys.get_mut(n)) {
                *sa = a;
                *sy = c.y + if a.sin() > 0.0 { r } else { -r };
                n += 1;
            }
        }
        if let (Some(sa), Some(sy)) = (angles.get_mut(n), ys.get_mut(n)) {
            *sa = a0 + sweep;
            *sy = p1.y;
            n += 1;
        }
        let mut m = 0;
        for k in 0..n.saturating_sub(1) {
            if let (Some(&a), Some(&b), Some(&ya), Some(&yb), Some(slot)) = (
                angles.get(k),
                angles.get(k + 1),
                ys.get(k),
                ys.get(k + 1),
                out.get_mut(m),
            ) {
                *slot = (a, b, ya, yb);
                m += 1;
            }
        }
        (out, m)
    }

    /// Crossings of the ray from `p` towards +x (half-open in y).
    pub fn crossings(&self, p: Vec2) -> u32 {
        match *self {
            Prim::Line { a, b, .. } => {
                if (a.y > p.y) != (b.y > p.y) {
                    let x = a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y);
                    if x > p.x {
                        return 1;
                    }
                }
                0
            }
            Prim::Arc { c, r, .. } => {
                let (pieces, n) = self.monotone_pieces();
                let mut count = 0;
                for &(a, b, ya, yb) in pieces.iter().take(n) {
                    if (ya > p.y) != (yb > p.y) {
                        let dy = p.y - c.y;
                        let dx = (r * r - dy * dy).max(0.0).sqrt();
                        let x = c.x + if (0.5 * (a + b)).cos() > 0.0 { dx } else { -dx };
                        if x > p.x {
                            count += 1;
                        }
                    }
                }
                count
            }
        }
    }

    /// Whether this piece meets the closed convex polygon.
    pub fn meets_convex(&self, poly: &Convex) -> bool {
        match *self {
            Prim::Line { a, b, .. } => poly.meets_segment(a, b),
            Prim::Arc { c, r, p0, p1, .. } => {
                if poly.contains(p0, 0.0) || poly.contains(p1, 0.0) {
                    return true;
                }
                let n = poly.pts.len();
                for k in 0..n {
                    let (Some(&a), Some(&b)) = (poly.pts.get(k), poly.pts.get(ring_next(k, n)))
                    else {
                        continue;
                    };
                    let (pts, m) = circle_segment_points(c, r, a, b);
                    for q in pts.iter().take(m) {
                        if self.contains_angle((q.y - c.y).atan2(q.x - c.x), 1e-9) {
                            return true;
                        }
                    }
                }
                false
            }
        }
    }

    /// The midpoint along the piece.
    pub fn mid(&self) -> Vec2 {
        match *self {
            Prim::Line { a, b, .. } => (a + b) * 0.5,
            Prim::Arc {
                c, r, a0, sweep, ..
            } => arc_point(c, r, a0 + sweep * 0.5),
        }
    }

    /// The unit direction of travel at parameter fraction `t` along the piece (zero when degenerate).
    pub fn direction_at(&self, p: Vec2) -> Vec2 {
        match *self {
            Prim::Line { a, b, .. } => (b - a).normalized().unwrap_or(Vec2::ZERO),
            Prim::Arc { c, sweep, .. } => {
                let s = if sweep >= 0.0 { 1.0 } else { -1.0 };
                let v = p - c;
                Vec2::new(-v.y * s, v.x * s)
                    .normalized()
                    .unwrap_or(Vec2::ZERO)
            }
        }
    }

    /// The distance from `p` to the piece.
    pub fn distance(&self, p: Vec2) -> f64 {
        match *self {
            Prim::Line { a, b, .. } => {
                let d = b - a;
                let l = d.dot(d);
                let t = if l > 0.0 {
                    ((p - a).dot(d) / l).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                p.distance(a + d * t)
            }
            Prim::Arc { c, r, p0, p1, .. } => {
                let v = p - c;
                if v.length() > 0.0 && self.contains_angle(v.y.atan2(v.x), 0.0) {
                    (v.length() - r).abs()
                } else {
                    p.distance(p0).min(p.distance(p1))
                }
            }
        }
    }
}

// --- loops ---

/// A closed loop of pieces, chained end to start, with its joints welded.
///
/// `curved` marks a loop that is not made of straight segments (arcs, or curves that were
/// flattened before they got here): such a loop never snaps.
#[derive(Clone, Debug)]
pub(crate) struct Loop {
    pub prims: Vec<Prim>,
    pub curved: bool,
    bounds: Bounds,
}

impl Loop {
    /// `None` for no pieces or one with a coordinate that is not finite.
    pub fn new(mut prims: Vec<Prim>, curved: Option<bool>) -> Option<Loop> {
        let n = prims.len();
        if n == 0 {
            return None;
        }
        for k in 0..n {
            let end = prims.get(ring_prev(k, n))?.end();
            let cur = prims.get_mut(k)?;
            let start = cur.start();
            if start != end && start.distance(end) <= JOINT_TOL {
                *cur = cur.with_start(end);
            }
        }
        let curved =
            curved.unwrap_or_else(|| prims.iter().any(|p| !matches!(p, Prim::Line { .. })));
        let mut bounds = prims.first()?.bounds();
        for p in prims.iter().skip(1) {
            bounds = bounds.union(p.bounds());
        }
        if ![bounds.x0, bounds.y0, bounds.x1, bounds.y1]
            .iter()
            .all(|v| v.is_finite())
        {
            return None;
        }
        Some(Loop {
            prims,
            curved,
            bounds,
        })
    }

    pub fn signed_area(&self) -> f64 {
        let o = self.prims.first().map_or(Vec2::new(0.0, 0.0), Prim::start);
        self.prims.iter().map(|p| p.area_term(o)).sum()
    }

    pub fn reversed(&self) -> Loop {
        let prims = self.prims.iter().rev().map(Prim::reversed).collect();
        Loop {
            prims,
            curved: self.curved,
            bounds: self.bounds,
        }
    }

    pub fn oriented(self, ccw: bool) -> Loop {
        if (self.signed_area() > 0.0) == ccw {
            self
        } else {
            self.reversed()
        }
    }

    pub fn bounds(&self) -> Bounds {
        self.bounds
    }

    pub fn contains(&self, p: Vec2) -> bool {
        let b = self.bounds;
        if p.x < b.x0 || p.x > b.x1 || p.y < b.y0 || p.y > b.y1 {
            return false;
        }
        self.prims.iter().map(|q| q.crossings(p)).sum::<u32>() % 2 == 1
    }

    /// A point just inside the loop, to test its nesting against other loops.
    fn interior_probe(&self) -> Vec2 {
        let ccw = if self.signed_area() > 0.0 {
            self.clone()
        } else {
            self.reversed()
        };
        let Some(p) = ccw.prims.first() else {
            return Vec2::ZERO;
        };
        let (m, d) = match *p {
            Prim::Line { a, b, .. } => ((a + b) * 0.5, b - a),
            Prim::Arc {
                c, r, a0, sweep, ..
            } => {
                let a = a0 + sweep * 0.5;
                let s = if sweep > 0.0 { 1.0 } else { -1.0 };
                (arc_point(c, r, a), Vec2::new(-a.sin() * s, a.cos() * s))
            }
        };
        let ln = d.length();
        if !(ln > 0.0) {
            return m;
        }
        let off = 1e-4;
        Vec2::new(m.x - d.y / ln * off, m.y + d.x / ln * off)
    }
}

// --- regions ---

/// Fields (outer loops, counter-clockwise) each minus their keep-outs (holes, clockwise).
///
/// Built from loops in any orientation by nesting depth: an even depth is a field, an odd depth
/// a keep-out of the loop that immediately contains it.
pub(crate) struct Region {
    pub parts: Vec<(Loop, Vec<Loop>)>,
    pub prims: Vec<Prim>,
    boxes: Vec<Bounds>,
    /// Pieces by horizontal band, so a cell only meets the pieces near it.
    rows: Vec<Vec<u32>>,
    row_y0: f64,
    row_h: f64,
}

impl Region {
    pub fn from_loops(loops: Vec<Loop>, bucket: f64) -> Region {
        let loops: Vec<Loop> = loops
            .into_iter()
            .filter(|l| l.signed_area().abs() > EPS * EPS)
            .collect();
        let areas: Vec<f64> = loops.iter().map(|l| l.signed_area().abs()).collect();
        let probes: Vec<Vec2> = loops.iter().map(Loop::interior_probe).collect();
        let mut containers: Vec<Vec<usize>> = Vec::new();
        for (i, &q) in probes.iter().enumerate() {
            let ai = areas.get(i).copied().unwrap_or(0.0);
            containers.push(
                loops
                    .iter()
                    .enumerate()
                    .filter(|&(j, other)| {
                        j != i && areas.get(j).copied().unwrap_or(0.0) > ai && other.contains(q)
                    })
                    .map(|(j, _)| j)
                    .collect(),
            );
        }
        let mut parts: BTreeMap<usize, (Loop, Vec<Loop>)> = BTreeMap::new();
        for (i, lp) in loops.iter().enumerate() {
            if containers.get(i).is_some_and(|c| c.len() % 2 == 0) {
                parts.insert(i, (lp.clone().oriented(true), Vec::new()));
            }
        }
        for (i, lp) in loops.iter().enumerate() {
            let Some(cs) = containers.get(i) else {
                continue;
            };
            if cs.len() % 2 == 1 {
                // The smallest field containing it (overlapping keep-outs are not a valid sketch,
                // but they must not crash).
                let fields = cs.iter().copied().filter(|j| parts.contains_key(j));
                let smallest = fields.min_by(|&x, &y| {
                    areas
                        .get(x)
                        .copied()
                        .unwrap_or(0.0)
                        .total_cmp(&areas.get(y).copied().unwrap_or(0.0))
                });
                if let Some(part) = smallest.and_then(|j| parts.get_mut(&j)) {
                    part.1.push(lp.clone().oriented(false));
                }
            }
        }
        Region::new(parts.into_values().collect(), bucket)
    }

    pub fn new(parts: Vec<(Loop, Vec<Loop>)>, bucket: f64) -> Region {
        let prims: Vec<Prim> = parts
            .iter()
            .flat_map(|(o, hs)| {
                std::iter::once(o)
                    .chain(hs)
                    .flat_map(|l| l.prims.iter().copied())
            })
            .collect();
        let boxes: Vec<Bounds> = prims.iter().map(Prim::bounds).collect();
        let all = boxes.iter().copied().reduce(Bounds::union);
        let (mut rows, mut row_y0, mut row_h) = (vec![Vec::new()], 0.0, 1.0);
        if let Some(b) = all {
            let span = (b.y1 - b.y0).max(0.0);
            let by_size = (span / bucket.max(1e-9)).ceil().clamp(1.0, 4096.0);
            let by_budget = (4_000_000.0 / (prims.len().max(1) as f64)).floor().max(1.0);
            let n = by_size.min(by_budget) as usize;
            row_y0 = b.y0;
            row_h = if span > 0.0 { span / n as f64 } else { 1.0 };
            rows = vec![Vec::new(); n];
            for (idx, pb) in boxes.iter().enumerate() {
                let (r0, r1) = (band(pb.y0, row_y0, row_h, n), band(pb.y1, row_y0, row_h, n));
                for r in r0..=r1 {
                    if let Some(row) = rows.get_mut(r) {
                        row.push(idx as u32);
                    }
                }
            }
        }
        Region {
            parts,
            prims,
            boxes,
            rows,
            row_y0,
            row_h,
        }
    }

    pub fn contains(&self, p: Vec2) -> bool {
        self.parts
            .iter()
            .any(|(o, hs)| o.contains(p) && !hs.iter().any(|h| h.contains(p)))
    }

    pub fn bounds(&self) -> Option<Bounds> {
        self.parts
            .iter()
            .map(|(o, _)| o.bounds())
            .reduce(Bounds::union)
    }

    /// Indices of the pieces whose box meets `b` (a superset of those that touch it).
    pub fn near(&self, b: &Bounds) -> Vec<usize> {
        let n = self.rows.len();
        let (r0, r1) = (
            band(b.y0, self.row_y0, self.row_h, n),
            band(b.y1, self.row_y0, self.row_h, n),
        );
        let mut out: Vec<usize> = Vec::new();
        for r in r0..=r1 {
            if let Some(row) = self.rows.get(r) {
                out.extend(row.iter().map(|&i| i as usize));
            }
        }
        out.sort_unstable();
        out.dedup();
        out.retain(|&i| self.boxes.get(i).is_some_and(|pb| pb.overlaps(b, 0.0)));
        out
    }

    /// The pieces that meet the interior of the convex polygon (shrunk by [`EPS`]).
    pub fn hits(&self, poly: &Convex) -> Vec<usize> {
        let sp = poly.shrunk(EPS);
        let mut out = self.near(&sp.bounds);
        out.retain(|&i| self.prims.get(i).is_some_and(|p| p.meets_convex(&sp)));
        out
    }

    /// Whether the convex polygon (counter-clockwise) lies inside the region.
    pub fn holds(&self, poly: Vec<Vec2>) -> bool {
        let Some(cv) = Convex::new(poly) else {
            return false;
        };
        self.hits(&cv).is_empty() && self.contains(poly_centroid(&cv.pts))
    }
}

fn band(y: f64, y0: f64, h: f64, n: usize) -> usize {
    let r = ((y - y0) / h).floor();
    if r > 0.0 {
        (r as usize).min(n.saturating_sub(1))
    } else {
        0
    }
}

// --- unions of tiles ---

/// A point snapped to a grid of 1e-9, as integers.
type GridKey = (i64, i64);
/// A directed edge by its end points' keys.
type EdgeKey = (GridKey, GridKey);

/// The outline loops of a union of non-overlapping counter-clockwise convex tiles that share
/// edges: outer loops counter-clockwise, holes clockwise, collinear points removed.
pub(crate) fn union_tiles(tiles: &[Vec<Vec2>]) -> Vec<Vec<Vec2>> {
    let tol = 1e-9;
    let key = |p: Vec2| ((p.x / tol).round() as i64, (p.y / tol).round() as i64);
    let pts: Vec<Vec2> = tiles.iter().flatten().copied().collect();
    // Directed edges still unpaired: an edge and its reverse (a shared side) cancel.
    let mut order: Vec<EdgeKey> = Vec::new();
    let mut edges: BTreeMap<EdgeKey, (Vec2, Vec2)> = BTreeMap::new();
    for t in tiles {
        let n = t.len();
        for k in 0..n {
            let (Some(&a), Some(&b)) = (t.get(k), t.get(ring_next(k, n))) else {
                continue;
            };
            let d = b - a;
            let l2 = d.dot(d);
            if !(l2 > 0.0) {
                continue;
            }
            let mut inner: Vec<(f64, Vec2)> = Vec::new();
            for &q in &pts {
                let s = (q - a).dot(d) / l2;
                if 1e-9 < s && s < 1.0 - 1e-9 && cross2(d, q - a).abs() / l2.sqrt() < tol * 10.0 {
                    inner.push((s, q));
                }
            }
            inner.sort_by(|x, y| x.0.total_cmp(&y.0));
            let mut chain = vec![a];
            chain.extend(inner.into_iter().map(|(_, q)| q));
            chain.push(b);
            for w in chain.windows(2) {
                let (Some(&u), Some(&v)) = (w.first(), w.get(1)) else {
                    continue;
                };
                let (ku, kv) = (key(u), key(v));
                if ku == kv {
                    continue;
                }
                if edges.remove(&(kv, ku)).is_none() {
                    edges.insert((ku, kv), (u, v));
                    order.push((ku, kv));
                }
            }
        }
    }
    let mut next: BTreeMap<GridKey, Vec<(GridKey, Vec2)>> = BTreeMap::new();
    for e in &order {
        if let Some(&(u, _)) = edges.get(e) {
            next.entry(e.0).or_default().push((e.1, u));
        }
    }
    let mut loops = Vec::new();
    while let Some((&start, _)) = next.iter().next() {
        let mut lp: Vec<Vec2> = Vec::new();
        let mut k = start;
        while let Some(outs) = next.get_mut(&k) {
            let Some((kv, u)) = outs.pop() else {
                next.remove(&k);
                break;
            };
            if outs.is_empty() {
                next.remove(&k);
            }
            lp.push(u);
            k = kv;
            if k == start {
                break;
            }
        }
        if lp.len() >= 3 {
            loops.push(drop_collinear(&lp));
        }
    }
    loops.retain(|l| l.len() >= 3);
    loops
}

fn drop_collinear(lp: &[Vec2]) -> Vec<Vec2> {
    let n = lp.len();
    let mut out = Vec::new();
    for k in 0..n {
        let (Some(&a), Some(&b), Some(&c)) =
            (lp.get(ring_prev(k, n)), lp.get(k), lp.get(ring_next(k, n)))
        else {
            continue;
        };
        if cross2(b - a, c - b).abs() > 1e-12 * (a.distance(b) + b.distance(c)).powi(2) {
            out.push(b);
        }
    }
    out
}

/// Rotation by `angle` of the vector `v`.
pub(crate) fn rotate(v: Vec2, angle: f64) -> Vec2 {
    let (s, c) = angle.sin_cos();
    Vec2::new(c * v.x - s * v.y, s * v.x + c * v.y)
}
