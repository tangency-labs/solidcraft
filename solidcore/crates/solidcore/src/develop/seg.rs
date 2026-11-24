//! The measures of a profile segment the development reads beyond the profile's own
//! ([`crate::prism::profile::Seg2`]'s points, ends, reversal and spans): its derivative and its
//! end tangents, its length, its split at a parameter, the parameter of its point nearest a given
//! one, and the points of a polyline that follows it; a loop made counter-clockwise, a polygon's
//! loop, and a region's area.
//!
//! Not upstream (SolidCraft's planar profiles, moved into the kernel with the sheet development,
//! docs/layers.md §1). A curve's length is a quadrature, its nearest point a sampled search
//! refined by Newton's method and its polyline sized from a bound on its second derivative: none
//! is a certificate, and the development uses them only where it compares within its own
//! tolerances (a hair of an edge, a point on an outline). A loop's turning and a region's area
//! are the profile's exact Green terms ([`Loop2::signed_area`]).
//!
//! Deviations: none (no upstream counterpart). No recursion.

use std::f64::consts::TAU;

use super::plane::{from_angle, Planar, X};
use crate::math::Vec2;
use crate::prism::profile::{Loop2, Region2, Seg2, MAX_BEZIER_CTRL, MAX_BEZIER_DEGREE};

/// Gauss–Legendre nodes and weights on [0, 1], five points.
const GL5: [(f64, f64); 5] = [
    (0.046_910_077_030_668, 0.118_463_442_528_095),
    (0.230_765_344_947_158, 0.239_314_335_249_683),
    (0.5, 0.284_444_444_444_444),
    (0.769_234_655_052_842, 0.239_314_335_249_683),
    (0.953_089_922_969_332, 0.118_463_442_528_095),
];

/// How far inside its ends [`SegMeasure::split_at`] splits a segment, so that neither part is
/// empty.
const SPLIT_MARGIN: f64 = 1e-9;

/// Samples [`SegMeasure::closest_param`] starts from.
const CLOSEST_SAMPLES: usize = 64;

/// Most points [`SegMeasure::polyline`] takes for one segment.
const MAX_POLYLINE: usize = 4096;

/// The control points `p[0..=n]` of a Bézier segment, its degree held to 1 through
/// [`MAX_BEZIER_DEGREE`].
fn bez(n: u8, p: &[Vec2; MAX_BEZIER_CTRL]) -> &[Vec2] {
    let k = match usize::from(n) {
        0 => 1,
        k => k.min(MAX_BEZIER_DEGREE),
    };
    p.get(..=k).unwrap_or(p.as_slice())
}

/// De Casteljau at `t`: the point, and the control points of the parts before and after it.
fn casteljau(c: &[Vec2], t: f64) -> (Vec2, Vec<Vec2>, Vec<Vec2>) {
    let mut w = c.to_vec();
    let (mut left, mut right) = (Vec::new(), Vec::new());
    while let Some(first) = w.first().copied() {
        left.push(first);
        if let Some(last) = w.last().copied() {
            right.push(last);
        }
        if w.len() == 1 {
            break;
        }
        w = w
            .iter()
            .zip(w.iter().skip(1))
            .map(|(a, b)| a.lerp(*b, t))
            .collect();
    }
    right.reverse();
    (left.last().copied().unwrap_or(Vec2::ZERO), left, right)
}

/// The segment of control points `pts`: a line, a conic of weight 1, a cubic or a Bézier.
fn bezier(pts: &[Vec2]) -> Option<Seg2> {
    Some(match *pts {
        [a, b] => Seg2::Line { a, b },
        [a, apex, b] => Seg2::Conic { a, apex, b, w: 1.0 },
        [p0, p1, p2, p3] => Seg2::Cubic { p0, p1, p2, p3 },
        _ if (2..=MAX_BEZIER_CTRL).contains(&pts.len()) => {
            let mut p = [Vec2::ZERO; MAX_BEZIER_CTRL];
            for (d, s) in p.iter_mut().zip(pts) {
                *d = *s;
            }
            Seg2::Bezier {
                n: u8::try_from(pts.len() - 1).ok()?,
                p,
            }
        }
        _ => return None,
    })
}

/// `∫₀¹ f(t) dt` by composite Gauss–Legendre (16 pieces of 5 points).
fn integrate(seg: &Seg2, f: impl Fn(&Seg2, f64) -> f64) -> f64 {
    let n = 16;
    let mut sum = 0.0;
    for k in 0..n {
        let (t0, h) = (f64::from(k) / f64::from(n), 1.0 / f64::from(n));
        for (x, w) in GL5 {
            sum += w * h * f(seg, t0 + x * h);
        }
    }
    sum
}

/// Newton's method for the parameter of the point nearest `p`, from `t`, within `[lo, hi]`: the
/// range closes in on the minimum, and a step that leaves it bisects.
fn newton_closest(seg: &Seg2, p: Vec2, mut t: f64, mut lo: f64, mut hi: f64) -> f64 {
    for _ in 0..30 {
        let (q, d) = (seg.point_at(t), seg.derivative(t));
        let d2 =
            (seg.derivative((t + 1e-6).min(1.0)) - seg.derivative((t - 1e-6).max(0.0))).over(2e-6);
        let h = q - p;
        let f = h.dot(d);
        let df = d.dot(d) + h.dot(d2);
        if f < 0.0 {
            lo = t;
        } else if f > 0.0 {
            hi = t;
        } else {
            break;
        }
        let step = t - f / df;
        // Arrived, even where the step rounds to the end of the range it has just closed.
        if (step - t).abs() < 1e-14 {
            break;
        }
        t = if step > lo && step < hi {
            step
        } else {
            0.5 * (lo + hi)
        };
    }
    t
}

/// The measures of a profile segment the development reads (module documentation).
pub(crate) trait SegMeasure {
    /// Its derivative at `t`.
    fn derivative(&self, t: f64) -> Vec2;
    /// The unit direction it starts in.
    fn start_tangent(&self) -> Vec2;
    /// The unit direction it ends in.
    fn end_tangent(&self) -> Vec2;
    /// Its length: exact for a line and an arc, a quadrature for any other curve.
    fn length(&self) -> f64;
    /// The parts before and after `t` (held a hair inside its ends).
    fn split_at(&self, t: f64) -> (Seg2, Seg2);
    /// The parameter of its point nearest `p`: exact for a line and an arc, a search refined by
    /// Newton's method for any other curve.
    fn closest_param(&self, p: Vec2) -> f64;
    /// Points along it from its start to its end whose chords keep about `tol` of it.
    fn polyline(&self, tol: f64) -> Vec<Vec2>;
}

impl SegMeasure for Seg2 {
    fn derivative(&self, t: f64) -> Vec2 {
        match *self {
            Seg2::Line { a, b } => b - a,
            Seg2::Arc {
                radius,
                start,
                sweep,
                ..
            } => from_angle(start + sweep * t).perp() * (radius * sweep),
            Seg2::Cubic { p0, p1, p2, p3 } => {
                let u = 1.0 - t;
                (p1 - p0) * (3.0 * u * u) + (p2 - p1) * (6.0 * u * t) + (p3 - p2) * (3.0 * t * t)
            }
            Seg2::Conic { a, apex, b, w } => {
                let u = 1.0 - t;
                let (n, d) = (
                    a * (u * u) + apex * (2.0 * w * u * t) + b * (t * t),
                    u * u + 2.0 * w * u * t + t * t,
                );
                let (dn, dd) = (
                    a * (-2.0 * u) + apex * (2.0 * w * (1.0 - 2.0 * t)) + b * (2.0 * t),
                    -2.0 * u + 2.0 * w * (1.0 - 2.0 * t) + 2.0 * t,
                );
                (dn * d - n * dd).over((d * d).max(1e-300))
            }
            Seg2::Bezier { n, ref p } => {
                let c = bez(n, p);
                let degree = (c.len() - 1) as f64;
                let diffs: Vec<Vec2> = c
                    .iter()
                    .zip(c.iter().skip(1))
                    .map(|(a, b)| (*b - *a) * degree)
                    .collect();
                casteljau(&diffs, t).0
            }
        }
    }

    fn start_tangent(&self) -> Vec2 {
        match *self {
            Seg2::Line { a, b } => (b - a).normalized().unwrap_or(X),
            Seg2::Arc { start, sweep, .. } => from_angle(start).perp() * sweep.signum(),
            Seg2::Cubic { p0, p1, p2, .. } => (p1 - p0)
                .normalized()
                .or_else(|| (p2 - p0).normalized())
                .unwrap_or(X),
            Seg2::Conic { a, apex, b, .. } => (apex - a)
                .normalized()
                .or_else(|| (b - a).normalized())
                .unwrap_or(X),
            Seg2::Bezier { n, ref p } => {
                let c = bez(n, p);
                let first = c.first().copied().unwrap_or(Vec2::ZERO);
                c.iter()
                    .skip(1)
                    .find_map(|q| (*q - first).normalized())
                    .unwrap_or(X)
            }
        }
    }

    fn end_tangent(&self) -> Vec2 {
        match *self {
            Seg2::Line { a, b } => (b - a).normalized().unwrap_or(X),
            Seg2::Arc { start, sweep, .. } => from_angle(start + sweep).perp() * sweep.signum(),
            Seg2::Cubic { p1, p2, p3, .. } => (p3 - p2)
                .normalized()
                .or_else(|| (p3 - p1).normalized())
                .unwrap_or(X),
            Seg2::Conic { a, apex, b, .. } => (b - apex)
                .normalized()
                .or_else(|| (b - a).normalized())
                .unwrap_or(X),
            Seg2::Bezier { n, ref p } => {
                let c = bez(n, p);
                let last = c.last().copied().unwrap_or(Vec2::ZERO);
                c.iter()
                    .rev()
                    .skip(1)
                    .find_map(|q| (last - *q).normalized())
                    .unwrap_or(X)
            }
        }
    }

    fn length(&self) -> f64 {
        match *self {
            Seg2::Line { a, b } => a.dist(b),
            Seg2::Arc { radius, sweep, .. } => (radius * sweep).abs(),
            Seg2::Cubic { .. } | Seg2::Conic { .. } | Seg2::Bezier { .. } => {
                integrate(self, |s, t| s.derivative(t).len())
            }
        }
    }

    // An if-chain, not `f64::clamp`: the panic-site check reads named bounds as unproven.
    #[allow(clippy::manual_clamp)]
    fn split_at(&self, t: f64) -> (Seg2, Seg2) {
        // Held a hair inside the ends (a parameter that is not a number stays one).
        let t = if t < SPLIT_MARGIN {
            SPLIT_MARGIN
        } else if t > 1.0 - SPLIT_MARGIN {
            1.0 - SPLIT_MARGIN
        } else {
            t
        };
        match *self {
            Seg2::Line { a, b } => {
                let m = a.lerp(b, t);
                (Seg2::Line { a, b: m }, Seg2::Line { a: m, b })
            }
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => (
                Seg2::Arc {
                    center,
                    radius,
                    start,
                    sweep: sweep * t,
                },
                Seg2::Arc {
                    center,
                    radius,
                    start: start + sweep * t,
                    sweep: sweep * (1.0 - t),
                },
            ),
            Seg2::Cubic { p0, p1, p2, p3 } => {
                let (a, b, c) = (p0.lerp(p1, t), p1.lerp(p2, t), p2.lerp(p3, t));
                let (d, e) = (a.lerp(b, t), b.lerp(c, t));
                let m = d.lerp(e, t);
                (
                    Seg2::Cubic {
                        p0,
                        p1: a,
                        p2: d,
                        p3: m,
                    },
                    Seg2::Cubic {
                        p0: m,
                        p1: e,
                        p2: c,
                        p3,
                    },
                )
            }
            Seg2::Conic { a, apex, b, w } => {
                // Homogeneous de Casteljau: (a, 1), (w·apex, w), (b, 1).
                let h = |p: Vec2, wt: f64| (p * wt, wt);
                let lerp = |x: (Vec2, f64), y: (Vec2, f64)| {
                    (x.0 * (1.0 - t) + y.0 * t, x.1 * (1.0 - t) + y.1 * t)
                };
                let (p0, p1, p2) = (h(a, 1.0), h(apex, w), h(b, 1.0));
                let (q0, q1) = (lerp(p0, p1), lerp(p1, p2));
                let r = lerp(q0, q1);
                let pt = |x: (Vec2, f64)| x.0.over(x.1.max(1e-300));
                let m = pt(r);
                (
                    Seg2::Conic {
                        a,
                        apex: pt(q0),
                        b: m,
                        w: q0.1 / r.1.max(1e-300).sqrt(),
                    },
                    Seg2::Conic {
                        a: m,
                        apex: pt(q1),
                        b,
                        w: q1.1 / r.1.max(1e-300).sqrt(),
                    },
                )
            }
            Seg2::Bezier { n, ref p } => {
                let (_, l, r) = casteljau(bez(n, p), t);
                let mk = |c: &[Vec2]| bezier(c).unwrap_or(*self);
                (mk(&l), mk(&r))
            }
        }
    }

    fn closest_param(&self, p: Vec2) -> f64 {
        let finite = |p: Vec2| p.x.is_finite() && p.y.is_finite();
        match *self {
            Seg2::Line { a, b } if finite(a) && finite(b) && finite(p) => {
                let d = b - a;
                if a == b {
                    return 0.0;
                }
                let denominator = d.len2();
                let t = (p - a).dot(d) / denominator;
                if denominator > 0.0 && denominator.is_finite() && t.is_finite() {
                    return t.clamp(0.0, 1.0);
                }
            }
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } if radius > 0.0 && sweep != 0.0 => {
                let angle = (p - center).angle();
                let along = if sweep > 0.0 {
                    (angle - start).rem_euclid(TAU)
                } else {
                    (start - angle).rem_euclid(TAU)
                };
                if along <= sweep.abs() {
                    return along / sweep.abs();
                }
                return if self.start().dist(p) <= self.end().dist(p) {
                    0.0
                } else {
                    1.0
                };
            }
            _ => {}
        }
        let n = CLOSEST_SAMPLES;
        let at = |i: usize| i as f64 / n as f64;
        let dist: Vec<f64> = (0..=n).map(|i| self.point_at(at(i)).dist(p)).collect();
        let d = |i: usize| dist.get(i).copied().unwrap_or(f64::INFINITY);
        // Refined from sample `i`, within its neighbours.
        let refine = |i: usize| {
            let t = newton_closest(self, p, at(i), at(i.saturating_sub(1)), at((i + 1).min(n)));
            (t, self.point_at(t).dist(p))
        };
        let lowest = dist
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.total_cmp(b.1))
            .map_or(0, |(i, _)| i);
        let (mut t, mut near) = refine(lowest);
        for i in (0..=n)
            .filter(|&i| i != lowest && (i == 0 || d(i) < d(i - 1)) && (i == n || d(i) <= d(i + 1)))
        {
            let (other, dd) = refine(i);
            // Clearly nearer only: two parts of a curve the same distance away keep the first answer.
            if dd + 1e-12 * (1.0 + dd) < near {
                (t, near) = (other, dd);
            }
        }
        t
    }

    fn polyline(&self, tol: f64) -> Vec<Vec2> {
        let pieces = |x: f64| {
            let n = x.ceil();
            if n >= MAX_POLYLINE as f64 {
                MAX_POLYLINE
            } else if n >= 2.0 {
                n as usize
            } else {
                2
            }
        };
        let points = |n: usize| {
            (0..=n)
                .map(|i| self.point_at(i as f64 / n as f64))
                .collect()
        };
        match *self {
            Seg2::Line { a, b } => vec![a, b],
            Seg2::Arc { radius, sweep, .. } => {
                let tol = tol.max(radius.abs() * 1e-6).max(1e-9);
                let step = if tol < radius {
                    2.0 * (1.0 - tol / radius).clamp(-1.0, 1.0).acos()
                } else {
                    TAU / 8.0
                };
                points(pieces(sweep.abs() / step.max(1e-6)))
            }
            Seg2::Cubic { p0, p1, p2, p3 } => {
                // Chord error of n pieces ≈ max second difference / (8 n²).
                let bow = (p0 - p1 * 2.0 + p2).len().max((p1 - p2 * 2.0 + p3).len()) * 6.0;
                points(pieces((bow / (8.0 * tol.max(1e-9))).sqrt()))
            }
            Seg2::Conic { a, apex, b, w } => {
                let bow = (a - apex * 2.0 + b).len() * 2.0 * w.max(1.0);
                points(pieces((bow / (8.0 * tol.max(1e-9))).sqrt()))
            }
            Seg2::Bezier { n, ref p } => {
                // Second derivative ≤ n(n−1) × the largest second difference of the controls.
                let c = bez(n, p);
                let n = f64::from(n);
                let d2 = c
                    .iter()
                    .zip(c.iter().skip(1))
                    .zip(c.iter().skip(2))
                    .map(|((a, b), d)| (*a - *b * 2.0 + *d).len())
                    .fold(0.0, f64::max)
                    * n
                    * (n - 1.0);
                points(pieces((d2 / (8.0 * tol.max(1e-9))).sqrt()))
            }
        }
    }
}

/// The loop run counter-clockwise: as it is where its signed area is 0 or more, else reversed
/// (a loop whose area cannot be had, as one with a coordinate that is not a number, too).
pub(crate) fn ccw(l: Loop2) -> Loop2 {
    match l.signed_area() {
        Ok(a) if a.value >= 0.0 => l,
        _ => l.reversed(),
    }
}

/// The closed polygon through `pts`, its last point joined back to its first.
pub(crate) fn polygon(pts: &[Vec2]) -> Loop2 {
    Loop2 {
        segs: pts
            .iter()
            .zip(pts.iter().cycle().skip(1))
            .map(|(a, b)| Seg2::Line { a: *a, b: *b })
            .collect(),
    }
}

/// A region's area: its outer loop's less its holes' (each loop's exact Green terms), not a
/// number where a loop's cannot be had.
pub(crate) fn area(r: &Region2) -> f64 {
    let loop_area = |l: &Loop2| l.signed_area().map_or(f64::NAN, |a| a.value.abs());
    loop_area(&r.outer) - r.holes.iter().map(loop_area).sum::<f64>()
}
