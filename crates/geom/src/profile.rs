//! Planar profiles: closed loops of lines, circular arcs, cubic Béziers and conics (rational
//! quadratic Béziers), and regions (an outer loop minus holes). Sketches produce them; the
//! kernel turns them into faces.

use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};

use crate::{Aabb2, Ring, Vec2};

/// One boundary segment in sketch coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Seg2 {
    Line {
        a: Vec2,
        b: Vec2,
    },
    /// Arc from `start` angle sweeping `sweep` radians (positive = counter-clockwise).
    Arc {
        center: Vec2,
        radius: f64,
        start: f64,
        sweep: f64,
    },
    /// Cubic Bézier.
    Cubic {
        p0: Vec2,
        p1: Vec2,
        p2: Vec2,
        p3: Vec2,
    },
    /// Rational quadratic Bézier from `a` to `b` with control `apex` of weight `w` (end
    /// weights 1): exact conic arcs (ellipses, parabolas, hyperbolas).
    Conic {
        a: Vec2,
        apex: Vec2,
        b: Vec2,
        w: f64,
    },
    /// Bézier of degree `n` (4…[`MAX_BEZIER_DEGREE`]) over `p[0..=n]` (the rest unused): pieces of
    /// higher-degree splines.
    Bezier {
        n: u8,
        p: [Vec2; MAX_BEZIER_CTRL],
    },
}

/// Highest degree of [`Seg2::Bezier`]: a degree-9 piece is the single Bézier a G4 blend curve
/// (`sketch.blend_curve` with `continuity: "g4"`) is made of.
pub const MAX_BEZIER_DEGREE: usize = 9;

/// Control points a [`Seg2::Bezier`] carries: its highest degree plus one (the unused tail is
/// zero).
pub const MAX_BEZIER_CTRL: usize = MAX_BEZIER_DEGREE + 1;

/// Highest derivative order [`Seg2::derivatives`] hands out, and therefore the highest
/// continuity order a blend curve can be built for (4 = G4).
pub const MAX_DERIVATIVE_ORDER: usize = 4;

/// A Bézier segment over `pts` (2…[`MAX_BEZIER_CTRL`] points): a line, a cubic or a higher-degree
/// piece. A quadratic becomes a conic of weight 1.
pub fn bezier(pts: &[Vec2]) -> Option<Seg2> {
    Some(match *pts {
        [a, b] => Seg2::Line { a, b },
        [a, apex, b] => Seg2::Conic { a, apex, b, w: 1.0 },
        [p0, p1, p2, p3] => Seg2::Cubic { p0, p1, p2, p3 },
        _ if (2..=MAX_BEZIER_CTRL).contains(&pts.len()) => {
            let mut p = [Vec2::ZERO; MAX_BEZIER_CTRL];
            for (d, s) in p.iter_mut().zip(pts) {
                *d = *s;
            }
            Seg2::Bezier { n: (pts.len() - 1) as u8, p }
        }
        _ => return None,
    })
}

/// de Casteljau: the point at `t` and the two halves' control points.
fn casteljau(c: &[Vec2], t: f64) -> (Vec2, Vec<Vec2>, Vec<Vec2>) {
    let mut w = c.to_vec();
    let mut left = Vec::with_capacity(c.len());
    let mut right = Vec::with_capacity(c.len());
    while let Some(first) = w.first().copied() {
        left.push(first);
        if let Some(last) = w.last().copied() {
            right.push(last);
        }
        if w.len() == 1 {
            break;
        }
        w = w.windows(2).map(|x| x[0].lerp(x[1], t)).collect();
    }
    right.reverse();
    (left.last().copied().unwrap_or(Vec2::ZERO), left, right)
}

/// Binomial coefficient `C(n, k)` for the small orders the derivative rules need (0 when
/// `k > n`).
pub(crate) fn binom(n: usize, k: usize) -> f64 {
    if k > n {
        return 0.0;
    }
    (0..k.min(n - k)).fold(1.0, |v, i| v * (n - i) as f64 / (i + 1) as f64)
}

/// Derivatives 1…`order` of the Bézier over `c` (degree `c.len() - 1`) at `t`: the j-th forward
/// difference of the control polygon, scaled by `n!/(n − j)!` and evaluated by de Casteljau.
/// Exact in exact arithmetic, zero above the degree, `None` for non-finite input.
fn bezier_derivatives(c: &[Vec2], t: f64, order: usize) -> Option<Vec<Vec2>> {
    if !t.is_finite() || c.is_empty() || c.iter().any(|p| !p.is_finite()) {
        return None;
    }
    let n = c.len() - 1;
    let mut diffs = c.to_vec();
    let mut scale = 1.0;
    let mut out = Vec::with_capacity(order);
    for j in 1..=order {
        if diffs.len() < 2 {
            out.push(Vec2::ZERO);
            continue;
        }
        diffs = diffs.windows(2).map(|w| w[1] - w[0]).collect();
        scale *= (n + 1 - j) as f64;
        out.push(casteljau(&diffs, t).0 * scale);
    }
    Some(out)
}

/// Gauss–Legendre nodes and weights on [0, 1] (5 points).
const GL5: [(f64, f64); 5] = [
    (0.046_910_077_030_668, 0.118_463_442_528_095),
    (0.230_765_344_947_158, 0.239_314_335_249_683),
    (0.5, 0.284_444_444_444_444),
    (0.769_234_655_052_842, 0.239_314_335_249_683),
    (0.953_089_922_969_332, 0.118_463_442_528_095),
];

impl Seg2 {
    fn bez(n: u8, p: &[Vec2; MAX_BEZIER_CTRL]) -> &[Vec2] {
        &p[..(n as usize).clamp(1, MAX_BEZIER_DEGREE) + 1]
    }
    /// Derivative with respect to the parameter at `t` (Béziers and conics; lines and arcs too).
    pub fn derivative(&self, t: f64) -> Vec2 {
        match *self {
            Seg2::Line { a, b } => b - a,
            Seg2::Arc { radius, start, sweep, .. } => Vec2::from_angle(start + sweep * t).perp() * (radius * sweep),
            Seg2::Cubic { p0, p1, p2, p3 } => {
                let u = 1.0 - t;
                (p1 - p0) * (3.0 * u * u) + (p2 - p1) * (6.0 * u * t) + (p3 - p2) * (3.0 * t * t)
            }
            Seg2::Conic { a, apex, b, w } => {
                let u = 1.0 - t;
                let (n, d) = (a * (u * u) + apex * (2.0 * w * u * t) + b * (t * t), u * u + 2.0 * w * u * t + t * t);
                let (dn, dd) = (a * (-2.0 * u) + apex * (2.0 * w * (1.0 - 2.0 * t)) + b * (2.0 * t), -2.0 * u + 2.0 * w * (1.0 - 2.0 * t) + 2.0 * t);
                (dn * d - n * dd) / (d * d).max(1e-300)
            }
            Seg2::Bezier { n, ref p } => {
                let c = Self::bez(n, p);
                let diffs: Vec<Vec2> = c.windows(2).map(|w| (w[1] - w[0]) * (c.len() - 1) as f64).collect();
                casteljau(&diffs, t).0
            }
        }
    }
    /// Derivatives 1…`order` with respect to the segment's own parameter at `t`, exactly: a line
    /// and an arc from their analytic form (an arc's are the circular-motion recurrence, zero
    /// curvature terms included), a cubic and a higher-degree piece from the forward differences
    /// of their controls, a conic through the quotient rule of its rational form. A blend curve
    /// reads a reference curve's derivatives here to build a Hermite interpolation, so this is
    /// exact at both ends (`t` = 0 and 1) and `None` — never a wrong number — when `order` is
    /// above [`MAX_DERIVATIVE_ORDER`], `t` is not finite or a value is not finite, as with a
    /// degenerate rational piece.
    pub fn derivatives(&self, t: f64, order: usize) -> Option<Vec<Vec2>> {
        if !t.is_finite() || order > MAX_DERIVATIVE_ORDER {
            return None;
        }
        let mut d = match *self {
            Seg2::Line { a, b } => {
                if !a.is_finite() || !b.is_finite() {
                    return None;
                }
                let mut d = vec![b - a];
                d.resize(MAX_DERIVATIVE_ORDER, Vec2::ZERO);
                d
            }
            Seg2::Arc { center, radius, start, sweep } => {
                if !center.is_finite() || !radius.is_finite() || !start.is_finite() || !sweep.is_finite() {
                    return None;
                }
                // Differentiating the radial direction twice returns its negative, so the
                // directions of the successive derivatives are tangential, radial, tangential…
                // with alternating signs, each order picking up one more factor of the sweep.
                let mut v = Vec2::from_angle(start + sweep * t).perp();
                let mut d = Vec::with_capacity(MAX_DERIVATIVE_ORDER);
                for j in 1..=MAX_DERIVATIVE_ORDER {
                    d.push(v * (radius * sweep.powi(j as i32)));
                    v = v.perp();
                }
                d
            }
            Seg2::Cubic { p0, p1, p2, p3 } => bezier_derivatives(&[p0, p1, p2, p3], t, MAX_DERIVATIVE_ORDER)?,
            Seg2::Bezier { n, ref p } => bezier_derivatives(Self::bez(n, p), t, MAX_DERIVATIVE_ORDER)?,
            Seg2::Conic { a, apex, b, w } => {
                if !a.is_finite() || !apex.is_finite() || !b.is_finite() || !w.is_finite() {
                    return None;
                }
                // The conic is `N(t) / d(t)` with `N` the quadratic through `a`, `w·apex`, `b` and
                // `d` its weight polynomial; both are evaluated here in the power basis (at the
                // ends `d` = 1, so a join is never singular). The quotient rule in its
                // derivative-of-a-product form gives the higher derivatives: the numerator of
                // `x⁽ʲ⁾` is `N⁽ʲ⁾` minus the terms that already use `d` and a lower `x`.
                let (n0, n1, n2) = (a, apex * (2.0 * w) - a * 2.0, a - apex * (2.0 * w) + b);
                let (e1, e2) = (2.0 * (w - 1.0), 2.0 * (1.0 - w));
                let quotient = |j: usize| match j {
                    0 => n0 + n1 * t + n2 * (t * t),
                    1 => n1 + n2 * (2.0 * t),
                    2 => n2 * 2.0,
                    _ => Vec2::ZERO,
                };
                let weight = |j: usize| match j {
                    0 => 1.0 + e1 * t + e2 * (t * t),
                    1 => e1 + e2 * (2.0 * t),
                    2 => e2 * 2.0,
                    _ => 0.0,
                };
                let den = weight(0);
                if !(den.abs() > 1e-300) {
                    return None;
                }
                let mut x = vec![quotient(0) / den];
                for j in 1..=MAX_DERIVATIVE_ORDER {
                    let mut v = quotient(j);
                    for i in 1..=j {
                        v -= x[j - i] * (weight(i) * binom(j, i));
                    }
                    x.push(v / den);
                }
                x.remove(0);
                x
            }
        };
        d.truncate(order);
        d.iter().all(|q| q.is_finite()).then_some(d)
    }
    pub fn start(&self) -> Vec2 {
        match *self {
            Seg2::Line { a, .. } | Seg2::Conic { a, .. } => a,
            Seg2::Cubic { p0, .. } => p0,
            Seg2::Bezier { p, .. } => p[0],
            Seg2::Arc { center, radius, start, .. } => center + Vec2::from_angle(start) * radius,
        }
    }
    pub fn end(&self) -> Vec2 {
        match *self {
            Seg2::Line { b, .. } | Seg2::Conic { b, .. } => b,
            Seg2::Cubic { p3, .. } => p3,
            Seg2::Bezier { n, ref p } => Self::bez(n, p).last().copied().unwrap_or(p[0]),
            Seg2::Arc { center, radius, start, sweep } => center + Vec2::from_angle(start + sweep) * radius,
        }
    }
    pub fn point_at(&self, t: f64) -> Vec2 {
        match *self {
            Seg2::Line { a, b } => a.lerp(b, t),
            Seg2::Arc { center, radius, start, sweep } => center + Vec2::from_angle(start + sweep * t) * radius,
            Seg2::Cubic { p0, p1, p2, p3 } => {
                let u = 1.0 - t;
                p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t)
            }
            Seg2::Conic { a, apex, b, w } => {
                let u = 1.0 - t;
                let d = u * u + 2.0 * w * u * t + t * t;
                (a * (u * u) + apex * (2.0 * w * u * t) + b * (t * t)) / d.max(1e-300)
            }
            Seg2::Bezier { n, ref p } => casteljau(Self::bez(n, p), t).0,
        }
    }
    pub fn mid(&self) -> Vec2 {
        self.point_at(0.5)
    }
    /// Unit tangent at the start (direction of travel).
    pub fn start_tangent(&self) -> Vec2 {
        match *self {
            Seg2::Line { a, b } => (b - a).normalized().unwrap_or(Vec2::X),
            Seg2::Arc { start, sweep, .. } => Vec2::from_angle(start).perp() * sweep.signum(),
            Seg2::Cubic { p0, p1, p2, .. } => (p1 - p0).normalized().or_else(|| (p2 - p0).normalized()).unwrap_or(Vec2::X),
            Seg2::Conic { a, apex, b, .. } => (apex - a).normalized().or_else(|| (b - a).normalized()).unwrap_or(Vec2::X),
            Seg2::Bezier { n, ref p } => {
                let c = Self::bez(n, p);
                c.iter().skip(1).find_map(|q| (*q - c[0]).normalized()).unwrap_or(Vec2::X)
            }
        }
    }
    /// Unit tangent at the end (direction of travel).
    pub fn end_tangent(&self) -> Vec2 {
        match *self {
            Seg2::Line { a, b } => (b - a).normalized().unwrap_or(Vec2::X),
            Seg2::Arc { start, sweep, .. } => Vec2::from_angle(start + sweep).perp() * sweep.signum(),
            Seg2::Cubic { p1, p2, p3, .. } => (p3 - p2).normalized().or_else(|| (p3 - p1).normalized()).unwrap_or(Vec2::X),
            Seg2::Conic { a, apex, b, .. } => (b - apex).normalized().or_else(|| (b - a).normalized()).unwrap_or(Vec2::X),
            Seg2::Bezier { n, ref p } => {
                let c = Self::bez(n, p);
                let last = c.last().copied().unwrap_or(p[0]);
                c.iter().rev().skip(1).find_map(|q| (last - *q).normalized()).unwrap_or(Vec2::X)
            }
        }
    }
    pub fn reversed(&self) -> Seg2 {
        match *self {
            Seg2::Line { a, b } => Seg2::Line { a: b, b: a },
            Seg2::Arc { center, radius, start, sweep } => Seg2::Arc { center, radius, start: start + sweep, sweep: -sweep },
            Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic { p0: p3, p1: p2, p2: p1, p3: p0 },
            Seg2::Conic { a, apex, b, w } => Seg2::Conic { a: b, apex, b: a, w },
            Seg2::Bezier { n, ref p } => {
                let mut q = *p;
                let k = (n as usize).clamp(1, MAX_BEZIER_DEGREE);
                q[..=k].reverse();
                Seg2::Bezier { n, p: q }
            }
        }
    }
    pub fn length(&self) -> f64 {
        match *self {
            Seg2::Line { a, b } => a.dist(b),
            Seg2::Arc { radius, sweep, .. } => (radius * sweep).abs(),
            Seg2::Cubic { .. } | Seg2::Conic { .. } | Seg2::Bezier { .. } => self.integrate(|s, t| s.derivative(t).len()),
        }
    }
    /// The segment split at parameter `t` in (0, 1) (same kind, exact; conic halves are
    /// re-normalised so their end weights stay 1).
    pub fn split_at(&self, t: f64) -> (Seg2, Seg2) {
        let t = t.clamp(1e-9, 1.0 - 1e-9);
        match *self {
            Seg2::Line { a, b } => {
                let m = a.lerp(b, t);
                (Seg2::Line { a, b: m }, Seg2::Line { a: m, b })
            }
            Seg2::Arc { center, radius, start, sweep } => (
                Seg2::Arc { center, radius, start, sweep: sweep * t },
                Seg2::Arc { center, radius, start: start + sweep * t, sweep: sweep * (1.0 - t) },
            ),
            Seg2::Cubic { p0, p1, p2, p3 } => {
                let (a, b, c) = (p0.lerp(p1, t), p1.lerp(p2, t), p2.lerp(p3, t));
                let (d, e) = (a.lerp(b, t), b.lerp(c, t));
                let m = d.lerp(e, t);
                (Seg2::Cubic { p0, p1: a, p2: d, p3: m }, Seg2::Cubic { p0: m, p1: e, p2: c, p3 })
            }
            Seg2::Conic { a, apex, b, w } => {
                // Homogeneous de Casteljau: (a, 1), (w·apex, w), (b, 1).
                let h = |p: Vec2, wt: f64| (p * wt, wt);
                let lerp = |x: (Vec2, f64), y: (Vec2, f64)| (x.0 * (1.0 - t) + y.0 * t, x.1 * (1.0 - t) + y.1 * t);
                let (p0, p1, p2) = (h(a, 1.0), h(apex, w), h(b, 1.0));
                let (q0, q1) = (lerp(p0, p1), lerp(p1, p2));
                let r = lerp(q0, q1);
                let pt = |x: (Vec2, f64)| x.0 / x.1.max(1e-300);
                let m = pt(r);
                (
                    Seg2::Conic { a, apex: pt(q0), b: m, w: q0.1 / r.1.max(1e-300).sqrt() },
                    Seg2::Conic { a: m, apex: pt(q1), b, w: q1.1 / r.1.max(1e-300).sqrt() },
                )
            }
            Seg2::Bezier { n, ref p } => {
                let (_, l, r) = casteljau(Self::bez(n, p), t);
                let mk = |c: &[Vec2]| bezier(c).unwrap_or(*self);
                (mk(&l), mk(&r))
            }
        }
    }
    /// Parameter of the closest point: exact projection for lines,
    /// sampling followed by Newton for the remaining curves.
    pub fn closest_param(&self, p: Vec2) -> f64 {
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
            _ => {}
        }
        let n = 64;
        let mut t =
            (0..=n).map(|i| i as f64 / n as f64).min_by(|x, y| self.point_at(*x).dist(p).total_cmp(&self.point_at(*y).dist(p))).unwrap_or(0.0);
        for _ in 0..30 {
            let (q, d) = (self.point_at(t), self.derivative(t));
            let (d2, h) = ((self.derivative((t + 1e-6).min(1.0)) - self.derivative((t - 1e-6).max(0.0))) / 2e-6, q - p);
            let f = h.dot(d);
            let df = d.dot(d) + h.dot(d2);
            if df.abs() < 1e-300 {
                break;
            }
            let nt = (t - f / df).clamp(0.0, 1.0);
            if (nt - t).abs() < 1e-14 {
                t = nt;
                break;
            }
            t = nt;
        }
        t
    }
    /// The two halves of the segment (same kind, exact).
    pub fn split_half(&self) -> (Seg2, Seg2) {
        match *self {
            Seg2::Line { a, b } => {
                let m = a.lerp(b, 0.5);
                (Seg2::Line { a, b: m }, Seg2::Line { a: m, b })
            }
            Seg2::Arc { center, radius, start, sweep } => (
                Seg2::Arc { center, radius, start, sweep: sweep / 2.0 },
                Seg2::Arc { center, radius, start: start + sweep / 2.0, sweep: sweep / 2.0 },
            ),
            Seg2::Cubic { p0, p1, p2, p3 } => {
                let (a, b, c) = (p0.lerp(p1, 0.5), p1.lerp(p2, 0.5), p2.lerp(p3, 0.5));
                let (d, e) = (a.lerp(b, 0.5), b.lerp(c, 0.5));
                let m = d.lerp(e, 0.5);
                (Seg2::Cubic { p0, p1: a, p2: d, p3: m }, Seg2::Cubic { p0: m, p1: e, p2: c, p3 })
            }
            Seg2::Conic { a, apex, b, w } => {
                // Rational de Casteljau at the middle, weights normalised to 1 at the ends.
                let m = self.point_at(0.5);
                let w2 = ((1.0 + w) / 2.0).max(0.0).sqrt();
                let (l, r) = ((a + apex * w) / (1.0 + w), (apex * w + b) / (1.0 + w));
                (Seg2::Conic { a, apex: l, b: m, w: w2 }, Seg2::Conic { a: m, apex: r, b, w: w2 })
            }
            Seg2::Bezier { .. } => self.split_at(0.5),
        }
    }
    /// ∫₀¹ f(t) dt by composite Gauss–Legendre (16 pieces of 5 points).
    fn integrate(&self, f: impl Fn(&Seg2, f64) -> f64) -> f64 {
        let n = 16;
        let mut sum = 0.0;
        for k in 0..n {
            let (t0, h) = (k as f64 / n as f64, 1.0 / n as f64);
            for (x, w) in GL5 {
                sum += w * h * f(self, t0 + x * h);
            }
        }
        sum
    }
    /// Contribution to the loop's signed area (shoelace integral ½∮(x dy − y dx)).
    pub fn area_term(&self) -> f64 {
        match *self {
            Seg2::Line { a, b } => 0.5 * a.cross(b),
            Seg2::Arc { radius, sweep, .. } => {
                let (a, b) = (self.start(), self.end());
                // Chord term plus the circular segment between chord and arc.
                0.5 * a.cross(b) + 0.5 * radius * radius * (sweep - sweep.sin())
            }
            Seg2::Cubic { .. } | Seg2::Conic { .. } | Seg2::Bezier { .. } => self.integrate(|s, t| 0.5 * s.point_at(t).cross(s.derivative(t))),
        }
    }
    /// Polyline approximation (including both end points) with at most `tol` chord error.
    pub fn polyline(&self, tol: f64) -> Vec<Vec2> {
        match *self {
            Seg2::Line { a, b } => vec![a, b],
            Seg2::Arc { radius, sweep, .. } => {
                let tol = tol.max(radius.abs() * 1e-6).max(1e-9);
                let step = if tol < radius { 2.0 * (1.0 - tol / radius).clamp(-1.0, 1.0).acos() } else { TAU / 8.0 };
                let n = ((sweep.abs() / step.max(1e-6)).ceil() as usize).clamp(2, 4096);
                (0..=n).map(|i| self.point_at(i as f64 / n as f64)).collect()
            }
            Seg2::Cubic { p0, p1, p2, p3 } => {
                // Chord error of n pieces ≈ max second difference / (8 n²).
                let bow = (p0 - p1 * 2.0 + p2).len().max((p1 - p2 * 2.0 + p3).len()) * 6.0;
                let n = ((bow / (8.0 * tol.max(1e-9))).sqrt().ceil() as usize).clamp(2, 4096);
                (0..=n).map(|i| self.point_at(i as f64 / n as f64)).collect()
            }
            Seg2::Conic { a, apex, b, w } => {
                let bow = (a - apex * 2.0 + b).len() * 2.0 * w.max(1.0);
                let n = ((bow / (8.0 * tol.max(1e-9))).sqrt().ceil() as usize).clamp(2, 4096);
                (0..=n).map(|i| self.point_at(i as f64 / n as f64)).collect()
            }
            Seg2::Bezier { n, ref p } => {
                // Second derivative ≤ n(n−1) × the largest second difference of the controls.
                let c = Self::bez(n, p);
                let d2 = c.windows(3).map(|w| (w[0] - w[1] * 2.0 + w[2]).len()).fold(0.0, f64::max) * (n as f64) * (n as f64 - 1.0);
                let k = ((d2 / (8.0 * tol.max(1e-9))).sqrt().ceil() as usize).clamp(2, 4096);
                (0..=k).map(|i| self.point_at(i as f64 / k as f64)).collect()
            }
        }
    }
}

/// A closed loop of segments, each starting where the previous one ends.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Loop2 {
    pub segs: Vec<Seg2>,
}

/// An elliptical arc with a directed, unwrapped parameter interval. Its points are
/// `center + major * cos(t) + major.perp() * ratio * sin(t)`; `end < start` runs clockwise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EllipseArc {
    pub center: Vec2,
    pub major: Vec2,
    pub ratio: f64,
    pub start: f64,
    pub end: f64,
}

/// Recognize the ellipse supporting a rational quadratic with endpoint weights 1.
/// Weights close to 1 and degenerate/non-finite extractions return `None`; callers can
/// retain the original rational curve. The axis ratio is never clamped into range.
pub fn conic_ellipse(a: Vec2, apex: Vec2, b: Vec2, w: f64) -> Option<EllipseArc> {
    if !(a.is_finite() && apex.is_finite() && b.is_finite() && w > 1e-9 && w < 1.0 - 1e-6) {
        return None;
    }
    let m = (a + b) * 0.5;
    let center = m - (apex - m) * (w * w / (1.0 - w * w));
    let alpha = w.acos();
    let (u, v) = ((m - center) / w, (b - m) / alpha.sin());
    if !center.is_finite() || u.cross(v).abs() < 1e-18 {
        return None;
    }
    let (major, minor, ratio, t) = ellipse_axes(u, v)?;
    if major.len() <= 1e-12 {
        return None;
    }
    let (start, end) = if major.cross(minor) > 0.0 { (-alpha - t, alpha - t) } else { (t + alpha, t - alpha) };
    Some(EllipseArc { center, major, ratio, start, end })
}

fn ellipse_axes(u: Vec2, v: Vec2) -> Option<(Vec2, Vec2, f64, f64)> {
    let (uu, uv, vv) = (u.dot(u), u.dot(v), v.dot(v));
    let cross = u.cross(v);
    if !(u.is_finite() && v.is_finite() && uu.is_finite() && uv.is_finite() && vv.is_finite() && cross.is_finite()) || cross == 0.0 {
        return None;
    }
    let t = 0.5 * (2.0 * uv).atan2(uu - vv);
    if !(2.0 * uv).is_finite() || !(uu - vv).is_finite() {
        return None;
    }
    let major = u * t.cos() + v * t.sin();
    let minor = v * t.cos() - u * t.sin();
    let ratio = minor.len() / major.len();
    (major.is_finite() && minor.is_finite() && ratio.is_finite() && ratio > 0.0 && ratio <= 1.0).then_some((major, minor, ratio, t))
}

// Angular recognition uses the rounding cells of the stored parameters, not a spatial
// tolerance. Each +/- propagates operand radii and one rounding of its result. Radius
// additions round outward. This is a finite-precision recognition policy, not permission
// to snap path vertices or to accept an individual sweep outside [-TAU, TAU].
#[derive(Clone, Copy)]
struct AngleResolution {
    value: f64,
    radius: f64,
}

fn rounding_radius(value: f64) -> f64 {
    let spacing = (value.next_up() - value).max(value - value.next_down());
    // Half the minimum subnormal cannot be represented; enclose it outward instead.
    (spacing * 0.5).max(f64::from_bits(1))
}

// Error-free addition residual for finite operands whose sum does not overflow.
fn sum_roundoff(a: f64, b: f64) -> f64 {
    let sum = a + b;
    let virtual_b = sum - a;
    (a - (sum - virtual_b)) + (b - virtual_b)
}

impl AngleResolution {
    fn new(value: f64) -> Self {
        Self { value, radius: rounding_radius(value) }
    }

    fn add(self, rhs: Self) -> Self {
        let value = self.value + rhs.value;
        let radius = ((self.radius + rhs.radius).next_up() + rounding_radius(value)).next_up();
        Self { value, radius }
    }

    fn sub(self, rhs: Self) -> Self {
        self.add(Self { value: -rhs.value, radius: rhs.radius })
    }

    fn times_integer(self, factor: f64) -> Self {
        let value = self.value * factor;
        let radius = ((self.radius * factor.abs()).next_up() + rounding_radius(value)).next_up();
        Self { value, radius }
    }

    fn contains_zero(self) -> bool {
        self.value.is_finite() && self.radius.is_finite() && self.value.abs() <= self.radius
    }
}

// Fixed precision for whole-ellipse reconstruction. The exact expression is enclosed by
// hi + lo +/- radius; no operation is allowed to enlarge the geometric acceptance cell.
// Crate-private visibility lets the existing area tests inspect the arithmetic certificate.
#[derive(Clone, Copy, Debug)]
pub(super) struct EllipseScalar {
    pub(super) hi: f64,
    pub(super) lo: f64,
    pub(super) radius: f64,
}

fn ellipse_two_sum(a: f64, b: f64) -> Option<(f64, f64)> {
    let sum = a + b;
    let virtual_b = sum - a;
    let virtual_a = sum - virtual_b;
    let error_b = b - virtual_b;
    let error_a = a - virtual_a;
    let error = error_a + error_b;
    [sum, virtual_b, virtual_a, error_b, error_a, error].into_iter().all(f64::is_finite).then_some((sum, error))
}

// Arguments are nonnegative bounds. Preserve exact zero rather than inventing uncertainty
// for a TwoSum of exact inputs; nonzero products include underflow in their outward bound.
fn ellipse_bound_add(a: f64, b: f64) -> f64 {
    if a == 0.0 {
        b
    } else if b == 0.0 {
        a
    } else {
        (a + b).next_up()
    }
}

fn ellipse_bound_mul(a: f64, b: f64) -> f64 {
    if a == 0.0 || b == 0.0 { 0.0 } else { (a * b).next_up() }
}

impl EllipseScalar {
    const ZERO: Self = Self { hi: 0.0, lo: 0.0, radius: 0.0 };
    const ONE: Self = Self { hi: 1.0, lo: 0.0, radius: 0.0 };

    fn checked(hi: f64, lo: f64, radius: f64) -> Option<Self> {
        (hi.is_finite() && lo.is_finite() && radius.is_finite() && radius >= 0.0).then_some(Self { hi, lo, radius })
    }

    pub(super) fn exact(value: f64) -> Option<Self> {
        Self::checked(value, 0.0, 0.0)
    }

    pub(super) fn plus(self, rhs: Self) -> Option<Self> {
        let (s, e) = ellipse_two_sum(self.hi, rhs.hi)?;
        let (t, f) = ellipse_two_sum(self.lo, rhs.lo)?;
        let (u, g) = ellipse_two_sum(e, t)?;
        let (v, k) = ellipse_two_sum(s, u)?;
        let (l, r1) = ellipse_two_sum(k, g)?;
        let (l, r2) = ellipse_two_sum(l, f)?;
        let (hi, lo) = ellipse_two_sum(v, l)?;
        let radius = ellipse_bound_add(ellipse_bound_add(self.radius, rhs.radius), ellipse_bound_add(r1.abs(), r2.abs()));
        Self::checked(hi, lo, radius)
    }

    fn negated(self) -> Self {
        Self { hi: -self.hi, lo: -self.lo, radius: self.radius }
    }

    pub(super) fn minus(self, rhs: Self) -> Option<Self> {
        self.plus(rhs.negated())
    }

    pub(super) fn times(self, rhs: Self) -> Option<Self> {
        let mut product = Self::ZERO;
        for (a, b) in [(self.hi, rhs.hi), (self.hi, rhs.lo), (self.lo, rhs.hi), (self.lo, rhs.lo)] {
            let p = a * b;
            let e = a.mul_add(b, -p);
            let (hi, lo) = ellipse_two_sum(p, e)?;
            // FMA's residual can underflow: enclose its rounding instead of assuming an
            // exact TwoProduct, including when the stored residual happens to be zero.
            product = product.plus(Self::checked(hi, lo, rounding_radius(e))?)?;
        }
        let a_bound = ellipse_bound_add(self.hi.abs(), self.lo.abs());
        let b_bound = ellipse_bound_add(rhs.hi.abs(), rhs.lo.abs());
        let input_error = ellipse_bound_add(
            ellipse_bound_add(ellipse_bound_mul(a_bound, rhs.radius), ellipse_bound_mul(b_bound, self.radius)),
            ellipse_bound_mul(self.radius, rhs.radius),
        );
        Self::checked(product.hi, product.lo, ellipse_bound_add(product.radius, input_error))
    }

    fn lower(self) -> f64 {
        ((self.hi - self.lo.abs()).next_down() - self.radius).next_down()
    }

    pub(super) fn magnitude_bound(self) -> f64 {
        ellipse_bound_add(ellipse_bound_add(self.hi.abs(), self.lo.abs()), self.radius)
    }

    pub(super) fn sign(self) -> Option<i8> {
        if self.hi == 0.0 && self.lo == 0.0 && self.radius == 0.0 {
            Some(0)
        } else if self.lower() > 0.0 {
            Some(1)
        } else if self.negated().lower() > 0.0 {
            Some(-1)
        } else {
            None
        }
    }

    pub(super) fn divided_by(self, rhs: Self) -> Option<Self> {
        let denominator = ((rhs.hi.abs() - rhs.lo.abs()).next_down() - rhs.radius).next_down();
        if denominator <= 0.0 || !denominator.is_finite() {
            return None;
        }
        let q0 = Self::exact(self.hi / rhs.hi)?;
        let residual = self.minus(q0.times(rhs)?)?;
        let q1 = (residual.hi + residual.lo) / rhs.hi;
        let (hi, lo) = ellipse_two_sum(q0.hi, q1)?;
        let quotient = Self::checked(hi, lo, 0.0)?;
        // A/B-Q = (A-QB)/B: the predictor gets one correction, then a certificate.
        let residual = self.minus(quotient.times(rhs)?)?;
        Self::checked(hi, lo, (residual.magnitude_bound() / denominator).next_up())
    }

    pub(super) fn square_root(self) -> Option<Self> {
        if self.lower() <= 0.0 {
            return None;
        }
        let s0 = Self::exact((self.hi + self.lo).sqrt())?;
        let denominator = 2.0 * s0.hi;
        if denominator <= 0.0 || !denominator.is_finite() {
            return None;
        }
        let residual = self.minus(s0.times(s0)?)?;
        let s1 = (residual.hi + residual.lo) / denominator;
        let (hi, lo) = ellipse_two_sum(s0.hi, s1)?;
        let root = Self::checked(hi, lo, 0.0)?;
        let lower = root.lower();
        if lower <= 0.0 {
            return None;
        }
        // |sqrt(A)-Q| = |A-Q²|/(sqrt(A)+Q) <= |A-Q²|/lower(Q).
        let residual = self.minus(root.times(root)?)?;
        Self::checked(hi, lo, (residual.magnitude_bound() / lower).next_up())
    }

    fn exact_half(self) -> Option<Self> {
        let (hi, lo) = (self.hi * 0.5, self.lo * 0.5);
        if self.radius != 0.0 || hi * 2.0 != self.hi || lo * 2.0 != self.lo {
            return None;
        }
        let (hi, lo) = ellipse_two_sum(hi, lo)?;
        Self::checked(hi, lo, 0.0)
    }

    pub(super) fn fits(self, radius: f64) -> bool {
        radius.is_finite() && radius >= 0.0 && self.magnitude_bound() <= radius
    }

    pub(super) fn rounds_to(self, value: f64) -> Option<bool> {
        let value = Self::exact(value)?;
        let lower = value.plus(Self::exact(value.hi.next_down())?)?.exact_half()?;
        let upper = value.plus(Self::exact(value.hi.next_up())?)?.exact_half()?;
        // Strict inclusion conservatively refuses ties and every uncertain boundary.
        Some(self.minus(lower)?.sign() == Some(1) && upper.minus(self)?.sign() == Some(1))
    }
}

pub(super) fn ellipse_local_resolution(min: f64, max: f64) -> Option<f64> {
    let extent = EllipseScalar::exact(max)?.minus(EllipseScalar::exact(min)?)?.exact_half()?;
    let lower = extent.lower();
    if lower <= 0.0 || !lower.is_finite() {
        return None;
    }
    let gap = lower.next_up() - lower;
    let radius = gap * 0.5;
    // Choose the lower binade if the extent straddles a power of two. Underflow must
    // not invent a larger cell. Only original vertices contribute to this budget.
    (radius > 0.0 && radius.is_finite() && radius * 2.0 == gap).then_some(radius)
}

#[derive(Clone, Copy)]
struct EllipsePoint {
    x: EllipseScalar,
    y: EllipseScalar,
}

impl EllipsePoint {
    fn relative(point: Vec2, origin: Vec2) -> Option<Self> {
        Some(Self {
            x: EllipseScalar::exact(point.x)?.minus(EllipseScalar::exact(origin.x)?)?,
            y: EllipseScalar::exact(point.y)?.minus(EllipseScalar::exact(origin.y)?)?,
        })
    }

    fn plus(self, rhs: Self) -> Option<Self> {
        Some(Self { x: self.x.plus(rhs.x)?, y: self.y.plus(rhs.y)? })
    }

    fn minus(self, rhs: Self) -> Option<Self> {
        Some(Self { x: self.x.minus(rhs.x)?, y: self.y.minus(rhs.y)? })
    }

    fn divided_by(self, rhs: EllipseScalar) -> Option<Self> {
        Some(Self { x: self.x.divided_by(rhs)?, y: self.y.divided_by(rhs)? })
    }

    fn cross(self, rhs: Self) -> Option<EllipseScalar> {
        self.x.times(rhs.y)?.minus(self.y.times(rhs.x)?)
    }

    fn fits(self, radius: Vec2) -> bool {
        self.x.fits(radius.x) && self.y.fits(radius.y)
    }
}

#[derive(Clone, Copy)]
struct EllipseSupport {
    xx: EllipseScalar,
    xy: EllipseScalar,
    yy: EllipseScalar,
    determinant: EllipseScalar,
}

impl EllipseSupport {
    fn new(major: Vec2, ratio: f64) -> Option<Self> {
        let (x, y, r) = (EllipseScalar::exact(major.x)?, EllipseScalar::exact(major.y)?, EllipseScalar::exact(ratio)?);
        let (x2, y2, r2) = (x.times(x)?, y.times(y)?, r.times(r)?);
        let xx = x2.plus(r2.times(y2)?)?;
        let yy = y2.plus(r2.times(x2)?)?;
        let xy = x.times(y)?.times(EllipseScalar::ONE.minus(r2)?)?;
        let determinant = xx.times(yy)?.minus(xy.times(xy)?)?;
        (xx.sign() == Some(1) && yy.sign() == Some(1) && determinant.sign() == Some(1)).then_some(Self { xx, xy, yy, determinant })
    }

    fn inner(self, a: EllipsePoint, b: EllipsePoint) -> Option<EllipseScalar> {
        let diagonal = self.yy.times(a.x)?.times(b.x)?.plus(self.xx.times(a.y)?.times(b.y)?)?;
        let off_diagonal = self.xy.times(a.x.times(b.y)?.plus(a.y.times(b.x)?)?)?;
        diagonal.minus(off_diagonal)?.divided_by(self.determinant)
    }

    fn witness(self, original: EllipsePoint, radius: Vec2) -> Option<EllipsePoint> {
        let norm = self.inner(original, original)?.square_root()?;
        let witness = original.divided_by(norm)?;
        witness.minus(original)?.fits(radius).then_some(witness)
    }
}

#[derive(Clone, Copy)]
struct EllipseCandidate {
    a: Vec2,
    apex: Vec2,
    b: Vec2,
    w: f64,
    chord2: f64,
}

impl EllipseCandidate {
    fn new(a: Vec2, apex: Vec2, b: Vec2, w: f64) -> Option<Self> {
        if !(a.is_finite() && apex.is_finite() && b.is_finite() && w > 1e-9 && w < 1.0 - 1e-6) {
            return None;
        }
        let canonical = |p: Vec2| Vec2::new(if p.x == 0.0 { 0.0 } else { p.x }, if p.y == 0.0 { 0.0 } else { p.y });
        let (mut a, mut b, apex) = (canonical(a), canonical(b), canonical(apex));
        if (b.x, b.y) < (a.x, a.y) {
            std::mem::swap(&mut a, &mut b);
        }
        let delta = EllipsePoint::relative(b, a)?;
        let d = Vec2::new(delta.x.hi + delta.x.lo, delta.y.hi + delta.y.lo);
        let chord2 = d.len2();
        // This finite f64 ranking proposes ONE candidate; it is not a certificate of
        // the mathematically longest chord. Exact ties use the same canonical key.
        (chord2.is_finite() && chord2 > 0.0).then_some(Self { a, apex, b, w, chord2 })
    }

    fn key(self) -> [f64; 7] {
        [self.a.x, self.a.y, self.b.x, self.b.y, self.apex.x, self.apex.y, self.w]
    }

    fn ellipse(self, radius: Vec2) -> Option<(Vec2, Vec2, f64)> {
        let d = EllipsePoint::relative(self.b, self.a)?;
        let p = EllipsePoint::relative(self.apex, self.a)?;
        // Rounded local values are used only to propose a triple. Certification below
        // reconstructs from the original controls with their subtraction residuals.
        let d = Vec2::new(d.x.hi + d.x.lo, d.y.hi + d.y.lo);
        let p = Vec2::new(p.x.hi + p.x.lo, p.y.hi + p.y.lo);
        let m = d * 0.5;
        let h = p - m;
        let q = 1.0 - self.w * self.w;
        let k = self.w * self.w / q;
        let local_center = m - h * k;
        let (cx, error_x) = ellipse_two_sum(self.a.x, local_center.x)?;
        let (cy, error_y) = ellipse_two_sum(self.a.y, local_center.y)?;
        if error_x.abs() > radius.x || error_y.abs() > radius.y {
            return None;
        }
        let (along, chord) = (k / q, 0.25 / q);
        let xx = h.x * h.x * along + d.x * d.x * chord;
        let xy = h.x * h.y * along + d.x * d.y * chord;
        let yy = h.y * h.y * along + d.y * d.y * chord;
        if ![q, k, along, chord, xx, xy, yy].into_iter().all(f64::is_finite) || q <= 0.0 || xx <= 0.0 || yy <= 0.0 {
            return None;
        }
        let (mut major, ratio) = if xy == 0.0 {
            // Preserve coordinate axes exactly, including the isotropic case.
            if xx >= yy { (Vec2::new(xx.sqrt(), 0.0), (yy / xx).sqrt()) } else { (Vec2::new(0.0, yy.sqrt()), (xx / yy).sqrt()) }
        } else {
            let difference = xx - yy;
            let discriminant = (difference * difference + 4.0 * xy * xy).sqrt();
            let large = (xx + yy + discriminant) * 0.5;
            let small = (xx * yy - xy * xy) / large;
            if !discriminant.is_finite() || !large.is_finite() || !small.is_finite() || small <= 0.0 || large <= 0.0 {
                return None;
            }
            let axis = if xx >= yy { Vec2::new(large - yy, xy) } else { Vec2::new(xy, large - xx) };
            let length = axis.len();
            if !length.is_finite() || length <= 0.0 {
                return None;
            }
            (axis * (large.sqrt() / length), (small / large).sqrt())
        };
        if !major.is_finite() || !ratio.is_finite() || ratio <= 0.0 || ratio > 1.0 {
            return None;
        }
        if major.x < 0.0 || (major.x == 0.0 && major.y < 0.0) {
            major = -major;
        }
        Some((Vec2::new(cx, cy), major, ratio))
    }
}

impl Loop2 {
    /// Recognize one directed turn of circular arcs on exactly the same center and radius.
    /// Joins and the accumulated sweep must agree at the stored angles' floating-point
    /// resolution. No trigonometric endpoint or world-space tolerance is used. Ambiguous
    /// phases, gaps, mixed directions, repeated turns and non-arc pieces return `None`.
    pub fn as_circle(&self) -> Option<(Vec2, f64)> {
        let &Seg2::Arc { center, radius, start: first_start, sweep: first_sweep } = self.segs.first()? else { return None };
        if !center.is_finite() || !radius.is_finite() || radius <= 0.0 {
            return None;
        }
        let (mut anchor_index, mut anchor) = (0, first_start);
        for (index, seg) in self.segs.iter().enumerate() {
            let &Seg2::Arc { center: c, radius: r, start, sweep } = seg else { return None };
            if c != center
                || r != radius
                || !start.is_finite()
                || !sweep.is_finite()
                || sweep == 0.0
                || sweep.abs() > TAU
                || sweep.is_sign_positive() != first_sweep.is_sign_positive()
                || start + sweep == start
            {
                return None;
            }
            if start < anchor {
                (anchor_index, anchor) = (index, start);
            }
        }
        // Fix one stored angle instead of independently widening both sides of every
        // joint. All starts must fit this same accumulated phase. Choosing the smallest
        // start makes the anchor independent of cyclic permutations of the segment list.
        let anchor = AngleResolution { value: anchor, radius: 0.0 };
        let mut total = AngleResolution { value: 0.0, radius: 0.0 };
        for (index, seg) in self.segs.iter().cycle().skip(anchor_index).take(self.segs.len()).enumerate() {
            let &Seg2::Arc { start, sweep, .. } = seg else { return None };
            let actual = if index == 0 { AngleResolution { value: 0.0, radius: 0.0 } } else { AngleResolution::new(start).sub(anchor) };
            let gap = actual.sub(total);
            // Stored angles may use different turns. The selected integer must still
            // distinguish adjacent turns; its multiplication carries its own roundoff.
            let turns = (gap.value / TAU).round();
            if !turns.is_finite() || turns + 1.0 == turns || turns - 1.0 == turns {
                return None;
            }
            let residual = gap.sub(AngleResolution::new(TAU).times_integer(turns));
            if residual.radius >= sweep.abs() * 0.5 || !residual.contains_zero() {
                return None;
            }
            total = total.add(AngleResolution::new(sweep));
        }
        let residual = total.sub(AngleResolution::new(TAU.copysign(first_sweep)));
        (residual.radius < TAU * 0.5 && residual.contains_zero()).then_some((center, radius))
    }

    /// Conservatively recognize one turn of an ellipse from rational quadratic pieces.
    /// Four quarter pieces retain their common-parallelogram recognition rules. Other
    /// partitions require exact stored joins and reconstruct one fixed returned ellipse:
    /// shared vertices and tangent controls must fit componentwise cells no larger than
    /// half an ULP of the original vertices' local semiextent; required weights must lie
    /// strictly inside their stored rounding cells. This is local reconstruction, not
    /// exact identity of the original binary rational curves. No controls are changed.
    /// General weights outside `1e-9 < w < 1-1e-6`, unresolved arithmetic, gaps, overlaps
    /// and mixed/repeated turns return `None`, so callers can preserve the original pieces.
    pub fn as_ellipse(&self) -> Option<(Vec2, Vec2, f64)> {
        if self.segs.len() == 4 && self.segs.iter().all(|seg| matches!(seg, Seg2::Conic { w, .. } if *w == std::f64::consts::FRAC_1_SQRT_2)) {
            // Preserve both acceptance and rejection of this already-supported case.
            return self.as_quarter_ellipse();
        }
        self.as_general_ellipse()
    }

    fn as_general_ellipse(&self) -> Option<(Vec2, Vec2, f64)> {
        let &Seg2::Conic { a: first, apex, b, w } = self.segs.first()? else { return None };
        let mut candidate = EllipseCandidate::new(first, apex, b, w)?;
        let (mut min, mut max) = (first, first);
        for (seg, next) in self.joints() {
            let (&Seg2::Conic { a, apex, b, w }, &Seg2::Conic { a: next_a, .. }) = (seg, next) else { return None };
            let proposed = EllipseCandidate::new(a, apex, b, w)?;
            if b != next_a {
                return None;
            }
            if proposed.chord2 > candidate.chord2 || (proposed.chord2 == candidate.chord2 && proposed.key() < candidate.key()) {
                candidate = proposed;
            }
            min = Vec2::new(min.x.min(a.x), min.y.min(a.y));
            max = Vec2::new(max.x.max(a.x), max.y.max(a.y));
        }
        let radius = Vec2::new(ellipse_local_resolution(min.x, max.x)?, ellipse_local_resolution(min.y, max.y)?);
        let (center, major, ratio) = candidate.ellipse(radius)?;
        // Freeze the FINAL returned triple before any reconstruction. No candidate
        // retry, axis renormalization, per-piece support or adjustable error allowance.
        let support = EllipseSupport::new(major, ratio)?;
        let origin = candidate.a;
        let local_center = EllipsePoint::relative(center, origin)?;
        let first_y = EllipsePoint::relative(first, origin)?.minus(local_center)?;
        let first_x = support.witness(first_y, radius)?;
        let (mut from_y, mut from_x) = (first_y, first_x);
        let (mut direction, mut winding) = (None, 0_i32);
        for (index, seg) in self.segs.iter().enumerate() {
            let &Seg2::Conic { apex, b, w, .. } = seg else { return None };
            let (to_y, to_x) = if index == self.segs.len() - 1 {
                (first_y, first_x)
            } else {
                let y = EllipsePoint::relative(b, origin)?.minus(local_center)?;
                (y, support.witness(y, radius)?)
            };
            let apex_y = EllipsePoint::relative(apex, origin)?.minus(local_center)?;
            let sign = from_y.cross(to_y)?.sign()?;
            // These three signs put the rational numerator in one open cone of width
            // < pi and give its angular derivative a constant nonzero sign.
            if sign == 0
                || from_y.cross(apex_y)?.sign() != Some(sign)
                || apex_y.cross(to_y)?.sign() != Some(sign)
                || direction.is_some_and(|previous| previous != sign)
            {
                return None;
            }
            direction = Some(sign);
            let h = support.inner(from_x, to_x)?;
            let denominator = EllipseScalar::ONE.plus(h)?;
            if denominator.sign() != Some(1) || EllipseScalar::ONE.minus(h)?.sign() != Some(1) {
                return None;
            }
            // The witnesses, required apex and positive minor-arc weight satisfy all
            // five homogeneous conic identities on the SAME fixed support by construction.
            let required_apex = from_x.plus(to_x)?.divided_by(denominator)?;
            let required_weight = denominator.times(EllipseScalar::exact(0.5)?)?.square_root()?;
            if !required_apex.minus(apex_y)?.fits(radius) || !required_weight.rounds_to(w)? {
                return None;
            }
            let (from_sign, to_sign) = (from_y.y.sign()?, to_y.y.sign()?);
            if (from_sign <= 0 && to_sign > 0 && sign > 0) || (to_sign <= 0 && from_sign > 0 && sign < 0) {
                winding = winding.checked_add(i32::from(sign))?;
            }
            (from_y, from_x) = (to_y, to_x);
        }
        // Each source conic and its chord stay in the same open cone, so this half-open
        // polygon crossing count is also the curve's winding. No angle sum is needed.
        (winding == 1 || winding == -1).then_some((center, major, ratio))
    }

    fn as_quarter_ellipse(&self) -> Option<(Vec2, Vec2, f64)> {
        let [q0, q1, q2, q3] = self.segs.as_slice() else { return None };
        let (&Seg2::Conic { a: p0, .. }, &Seg2::Conic { a: p1, .. }, &Seg2::Conic { a: p2, .. }, &Seg2::Conic { a: p3, .. }) = (q0, q1, q2, q3)
        else {
            return None;
        };
        let center = (p0 + p2) * 0.5;
        if !center.is_finite() || center != (p1 + p3) * 0.5 {
            return None;
        }
        let offsets = [p0 - center, p1 - center, p2 - center, p3 - center];
        if offsets.iter().any(|d| !d.is_finite()) {
            return None;
        }
        let extent = offsets.iter().fold(Vec2::ZERO, |size, d| Vec2::new(size.x.max(d.x.abs()), size.y.max(d.y.abs())));
        // Do not lose a significant LOCAL displacement when averaging large world
        // coordinates. TwoSum recovers what the world-coordinate sum discarded; half
        // that residue must fit the rounding cell of the local extent in each axis.
        for (a, b) in [(p0, p2), (p1, p3)] {
            let error = Vec2::new(sum_roundoff(a.x, b.x), sum_roundoff(a.y, b.y)) * 0.5;
            if !error.is_finite() || error.x.abs() > rounding_radius(extent.x) || error.y.abs() > rounding_radius(extent.y) {
                return None;
            }
        }
        // A rounded world-coordinate sum can hide asymmetric endpoints. A displacement
        // from either endpoint must reconstruct BOTH original points. Trying both keeps
        // ordinary affine constructions whose inverse subtraction rounds differently.
        let opposite = |a: Vec2, b: Vec2| [a - center, center - b].into_iter().find(|&d| center + d == a && center - d == b);
        let (u, v) = (opposite(p0, p2)?, opposite(p1, p3)?);
        let axes = [u, v, -u, -v];
        let mut direction = None;
        for ((seg, next), (from, to)) in self.joints().zip(crate::chain::around(&axes)) {
            let (&Seg2::Conic { a, apex, b, w }, &Seg2::Conic { a: next_a, .. }) = (seg, next) else { return None };
            if !(a.is_finite() && apex.is_finite() && b.is_finite()) || w != std::f64::consts::FRAC_1_SQRT_2 || b != next_a {
                return None;
            }
            // Every endpoint and apex must use the SAME pair of accepted offsets.
            // The alternatives only change the association of this affine construction.
            if a != center + *from || b != center + *to || (apex != a + *to && apex != b + *from && apex != center + (*from + *to)) {
                return None;
            }
            let cross = from.cross(*to);
            if !cross.is_finite() || cross == 0.0 || direction.is_some_and(|positive| positive != cross.is_sign_positive()) {
                return None;
            }
            direction = Some(cross.is_sign_positive());
        }
        let (mut major, _, ratio, _) = ellipse_axes(u, v)?;
        if major.x < 0.0 || (major.x == 0.0 && major.y < 0.0) {
            major = -major;
        }
        Some((center, major, ratio))
    }

    /// The radius of the largest circle inside the loop, to about `tol` ([`Region2::inradius`]).
    pub fn inradius(&self, tol: f64) -> f64 {
        max_clearance(&[self.polyline(tol)])
    }
    /// Full circle as two half arcs (counter-clockwise).
    pub fn circle(center: Vec2, radius: f64) -> Loop2 {
        Loop2 {
            segs: vec![
                Seg2::Arc { center, radius, start: 0.0, sweep: std::f64::consts::PI },
                Seg2::Arc { center, radius, start: std::f64::consts::PI, sweep: std::f64::consts::PI },
            ],
        }
    }
    /// Full circle starting (and split) at angle `start`.
    pub fn circle_from(center: Vec2, radius: f64, start: f64) -> Loop2 {
        use std::f64::consts::PI;
        Loop2 { segs: vec![Seg2::Arc { center, radius, start, sweep: PI }, Seg2::Arc { center, radius, start: start + PI, sweep: PI }] }
    }
    pub fn polygon(pts: &[Vec2]) -> Loop2 {
        let n = pts.len();
        Loop2 { segs: (0..n).filter_map(|i| Some(Seg2::Line { a: *pts.get(i)?, b: *pts.get((i + 1) % n)? })).collect() }
    }
    /// Signed area: positive when counter-clockwise.
    pub fn signed_area(&self) -> f64 {
        self.segs.iter().map(Seg2::area_term).sum()
    }
    pub fn reversed(&self) -> Loop2 {
        Loop2 { segs: self.segs.iter().rev().map(Seg2::reversed).collect() }
    }
    /// Whether the loop runs counter-clockwise ([`Loop2::ccw`] keeps it as it is).
    pub fn is_ccw(&self) -> bool {
        self.signed_area() >= 0.0
    }
    /// Counter-clockwise copy.
    pub fn ccw(&self) -> Loop2 {
        if self.is_ccw() { self.clone() } else { self.reversed() }
    }
    /// Each segment with the next and the last with the first: the loop's joints, where one
    /// segment should end on the start of the next.
    pub fn joints(&self) -> impl Iterator<Item = (&Seg2, &Seg2)> + Clone {
        crate::chain::around(&self.segs)
    }
    /// The loop as a polyline within `tol`: each segment's points but its last, which the next
    /// segment starts on.
    pub fn polyline(&self, tol: f64) -> Ring<Vec2> {
        self.segs
            .iter()
            .flat_map(|s| {
                let p = s.polyline(tol);
                let n = p.len().saturating_sub(1);
                p.into_iter().take(n)
            })
            .collect()
    }
    /// Even-odd point containment against the polyline approximation.
    pub fn contains(&self, p: Vec2) -> bool {
        self.polyline(1e-3).contains(p)
    }
    /// Area-weighted centroid (of the polyline approximation).
    pub fn centroid(&self) -> Vec2 {
        let poly = self.polyline(1e-3);
        let n = poly.len();
        let (mut a2, mut c) = (0.0, Vec2::ZERO);
        for i in 0..n {
            let (Some(p), Some(q)) = (poly.get(i), poly.get((i + 1) % n)) else { continue };
            let k = p.cross(*q);
            a2 += k;
            c += (*p + *q) * k;
        }
        if a2.abs() < 1e-15 { poly.first().copied().unwrap_or_default() } else { c / (3.0 * a2) }
    }
    /// Closed within `tol`?
    pub fn is_closed(&self, tol: f64) -> bool {
        let n = self.segs.len();
        n > 0
            && (0..n).all(|i| match (self.segs.get(i), self.segs.get((i + 1) % n)) {
                (Some(a), Some(b)) => a.end().dist(b.start()) <= tol,
                _ => false,
            })
    }
}

/// A planar region: outer loop (counter-clockwise) minus holes (clockwise).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Region2 {
    pub outer: Loop2,
    pub holes: Vec<Loop2>,
}

impl Region2 {
    pub fn area(&self) -> f64 {
        self.outer.signed_area().abs() - self.holes.iter().map(|h| h.signed_area().abs()).sum::<f64>()
    }
    /// A point strictly inside the region (for picking and labels).
    pub fn interior_point(&self) -> Vec2 {
        let c = self.centroid();
        if self.contains(c) {
            return c;
        }
        // Scan a few horizontal lines for the midpoint of the widest inside span.
        let poly = self.outer.polyline(1e-2);
        let (mut y0, mut y1) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in &poly {
            y0 = y0.min(p.y);
            y1 = y1.max(p.y);
        }
        for k in 1..16 {
            let y = y0 + (y1 - y0) * k as f64 / 16.0;
            let mut xs: Vec<f64> = Vec::new();
            for l in std::iter::once(&self.outer).chain(&self.holes) {
                let pl = l.polyline(1e-2);
                let n = pl.len();
                for i in 0..n {
                    let (Some(a), Some(b)) = (pl.get(i), pl.get((i + 1) % n)) else { continue };
                    if (a.y > y) != (b.y > y) {
                        xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
                    }
                }
            }
            xs.sort_by(f64::total_cmp);
            for w in xs.chunks(2) {
                if let [a, b] = w {
                    let p = Vec2::new((a + b) * 0.5, y);
                    if self.contains(p) {
                        return p;
                    }
                }
            }
        }
        c
    }
    pub fn contains(&self, p: Vec2) -> bool {
        self.outer.contains(p) && !self.holes.iter().any(|h| h.contains(p))
    }
    /// The radius of the largest circle inside the region: how far its sides can move inwards
    /// before they meet (a taper going in closes it there). Found by a grid search refined
    /// around the best point, to about `tol`.
    pub fn inradius(&self, tol: f64) -> f64 {
        let loops: Vec<Ring<Vec2>> = std::iter::once(&self.outer).chain(&self.holes).map(|l| l.polyline(tol)).collect();
        max_clearance(&loops)
    }
}

/// The largest distance from a point inside `loops` (even-odd) to their sides, by a grid search
/// over their box refined around the best point. Zero for an empty or degenerate outline.
fn max_clearance(loops: &[Ring<Vec2>]) -> f64 {
    let sides: Vec<(Vec2, Vec2)> = loops.iter().flat_map(|pl| pl.edges().map(|(a, b)| (*a, *b))).filter(|(a, b)| a.dist(*b) > 0.0).collect();
    let corners: Aabb2 = sides.iter().map(|s| s.0).collect();
    if corners.is_empty() {
        return 0.0;
    }
    let Aabb2 { min: lo, max: hi } = corners;
    let inside = |p: Vec2| {
        let mut odd = false;
        for (a, b) in &sides {
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                odd = !odd;
            }
        }
        odd
    };
    let clearance = |p: Vec2| -> f64 {
        if !inside(p) {
            return 0.0;
        }
        sides
            .iter()
            .map(|(a, b)| {
                let ab = *b - *a;
                let t = ((p - *a).dot(ab) / ab.len2()).clamp(0.0, 1.0);
                p.dist(*a + ab * t)
            })
            .fold(f64::INFINITY, f64::min)
    };
    const N: usize = 32;
    let size = hi - lo;
    let mut best = (0.0, (lo + hi) * 0.5);
    for i in 0..=N {
        for j in 0..=N {
            let p = Vec2::new(lo.x + size.x * i as f64 / N as f64, lo.y + size.y * j as f64 / N as f64);
            let c = clearance(p);
            if c > best.0 {
                best = (c, p);
            }
        }
    }
    // Refine: try the eight neighbours, halving the step whenever none is better.
    let mut step = size.x.max(size.y) / N as f64;
    for _ in 0..60 {
        if step < size.x.max(size.y) * 1e-9 {
            break;
        }
        let around = [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0), (1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)];
        match around.iter().map(|(dx, dy)| best.1 + Vec2::new(dx * step, dy * step)).map(|p| (clearance(p), p)).max_by(|a, b| a.0.total_cmp(&b.0)) {
            Some(n) if n.0 > best.0 => best = n,
            _ => step *= 0.5,
        }
    }
    best.0
}

impl Region2 {
    /// Triangles covering the region (for filled display), by slabs between the vertex
    /// heights: inside each slab the boundary edges don't cross, so consecutive crossings pair
    /// up into trapezoids.
    pub fn triangulate(&self, tol: f64) -> Vec<[Vec2; 3]> {
        let mut edges: Vec<(Vec2, Vec2)> = Vec::new();
        for l in std::iter::once(&self.outer).chain(&self.holes) {
            let pl = l.polyline(tol);
            let n = pl.len();
            for i in 0..n {
                let (Some(a), Some(b)) = (pl.get(i), pl.get((i + 1) % n)) else { continue };
                if (a.y - b.y).abs() > 1e-12 {
                    edges.push(if a.y < b.y { (*a, *b) } else { (*b, *a) });
                }
            }
        }
        if edges.len() > 20_000 {
            return Vec::new();
        }
        let mut ys: Vec<f64> = edges.iter().flat_map(|(a, b)| [a.y, b.y]).collect();
        ys.sort_by(f64::total_cmp);
        ys.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
        let x_at = |e: &(Vec2, Vec2), y: f64| e.0.x + (y - e.0.y) / (e.1.y - e.0.y) * (e.1.x - e.0.x);
        // Sweep upward: an edge joins the slabs once its lower end is passed and leaves after
        // its upper end, so each slab sorts only the edges crossing it (a finely sampled ring
        // has hundreds of slabs and edges, but four edges in each slab).
        edges.sort_by(|a, b| a.0.y.total_cmp(&b.0.y));
        let mut next = 0;
        let mut span: Vec<&(Vec2, Vec2)> = Vec::new();
        let mut out = Vec::new();
        for w in ys.windows(2) {
            let (y0, y1) = (w[0], w[1]);
            let ym = (y0 + y1) * 0.5;
            while let Some(e) = edges.get(next)
                && e.0.y <= ym
            {
                span.push(e);
                next += 1;
            }
            span.retain(|e| e.1.y >= ym);
            span.sort_by(|a, b| x_at(a, ym).total_cmp(&x_at(b, ym)));
            for pair in span.chunks(2) {
                let [l, r] = pair else { continue };
                let (a, b) = (Vec2::new(x_at(l, y0), y0), Vec2::new(x_at(r, y0), y0));
                let (c, d) = (Vec2::new(x_at(r, y1), y1), Vec2::new(x_at(l, y1), y1));
                out.push([a, b, c]);
                out.push([a, c, d]);
            }
        }
        out
    }
    pub fn centroid(&self) -> Vec2 {
        let ao = self.outer.signed_area().abs();
        let mut c = self.outer.centroid() * ao;
        let mut a = ao;
        for h in &self.holes {
            let ah = h.signed_area().abs();
            c -= h.centroid() * ah;
            a -= ah;
        }
        if a.abs() < 1e-15 { self.outer.centroid() } else { c / a }
    }
}

/// Most boundary segment pairs [`regions_apart`] compares before it gives up (and says no).
const APART_BUDGET: usize = 50_000_000;

/// Whether no two of the regions meet or come within `gap` of each other (their boundaries
/// are farther apart than `gap` and none lies inside another; one inside another's hole is
/// apart). Tools made from them alike (extruded over one interval) are then apart too, and
/// can be cut as one. False when unsure (a degenerate region, too much to compare).
pub fn regions_apart(regions: &[Region2], gap: f64) -> bool {
    if !(gap.is_finite() && gap > 0.0) || regions.len() > 10_000 {
        return false;
    }
    // The polylines lie within `tol` of the curves: boundaries this much farther than the gap
    // apart are apart.
    let tol = gap / 4.0;
    let reach = gap + 2.0 * tol;
    struct Outline {
        loops: Vec<Ring<Vec2>>,
        bounds: Aabb2,
        inside: Vec2,
    }
    let mut outlines = Vec::with_capacity(regions.len());
    for r in regions {
        let loops: Vec<Ring<Vec2>> = std::iter::once(&r.outer).chain(&r.holes).map(|l| l.polyline(tol)).collect();
        let Some(outer) = loops.first().filter(|l| l.len() >= 2) else { return false };
        if outer.iter().any(|p| !p.is_finite()) {
            return false;
        }
        let bounds = outer.bounds();
        let Some(inside) = outer.first().copied() else { return false };
        outlines.push(Outline { loops, bounds, inside });
    }
    let mut order: Vec<usize> = (0..outlines.len()).collect();
    order.sort_by(|&i, &j| {
        let x = |k: usize| outlines.get(k).map(|o| o.bounds.min.x).unwrap_or(0.0);
        x(i).total_cmp(&x(j))
    });
    let mut work = 0usize;
    for (k, &i) in order.iter().enumerate() {
        let Some(a) = outlines.get(i) else { return false };
        for &j in order.iter().skip(k + 1) {
            let Some(b) = outlines.get(j) else { return false };
            let (a_box, b_box) = (a.bounds, b.bounds);
            if b_box.min.x > a_box.max.x + reach {
                break;
            }
            if b_box.min.y > a_box.max.y + reach || a_box.min.y > b_box.max.y + reach {
                continue;
            }
            // A boundary point of one inside the other: one holds the other (boundaries that do
            // not come near each other do not cross).
            let (Some(ra), Some(rb)) = (regions.get(i), regions.get(j)) else { return false };
            if ra.contains(b.inside) || rb.contains(a.inside) {
                return false;
            }
            for la in &a.loops {
                for lb in &b.loops {
                    work = work.saturating_add(la.len().saturating_mul(lb.len()));
                    if work > APART_BUDGET {
                        return false;
                    }
                    // (Each loop closed: its last point back to its first.)
                    for (p, q) in la.iter().zip(la.iter().skip(1).chain(la.first())) {
                        for (r, s) in lb.iter().zip(lb.iter().skip(1).chain(lb.first())) {
                            if segment_distance(*p, *q, *r, *s) <= reach {
                                return false;
                            }
                        }
                    }
                }
            }
        }
    }
    true
}

/// The distance between segments `ab` and `cd` (0 where they cross).
fn segment_distance(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> f64 {
    let side = |p: Vec2, q: Vec2, r: Vec2| (q - p).cross(r - p);
    let (d1, d2, d3, d4) = (side(c, d, a), side(c, d, b), side(a, b, c), side(a, b, d));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return 0.0;
    }
    let to = |p: Vec2, s: Vec2, e: Vec2| {
        let v = e - s;
        let len2 = v.dot(v);
        let t = if len2 > 0.0 { ((p - s).dot(v) / len2).clamp(0.0, 1.0) } else { 0.0 };
        p.dist(s + v * t)
    };
    to(a, c, d).min(to(b, c, d)).min(to(c, a, b)).min(to(d, a, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f64, y: f64, size: f64) -> Loop2 {
        Loop2::polygon(&[Vec2::new(x, y), Vec2::new(x + size, y), Vec2::new(x + size, y + size), Vec2::new(x, y + size)])
    }

    fn solid(l: Loop2) -> Region2 {
        Region2 { outer: l, holes: Vec::new() }
    }

    /// #1602: the largest circle inside a region, which bounds how far a taper can go.
    #[test]
    fn inradius_of_rectangles_circles_and_rings() {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-3 * b.max(1.0);
        let rect = Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 4.0), Vec2::new(0.0, 4.0)]);
        assert!(close(solid(rect.clone()).inradius(1e-3), 2.0), "{}", solid(rect.clone()).inradius(1e-3));
        assert!(close(rect.inradius(1e-3), 2.0));
        let disc = Loop2::circle(Vec2::new(3.0, -2.0), 5.0);
        assert!(close(solid(disc.clone()).inradius(1e-4), 5.0), "{}", solid(disc).inradius(1e-4));
        // A ring 10 to 6: the material is 4 wide (radius 2); the hole alone has radius 6.
        let ring = Region2 { outer: Loop2::circle(Vec2::ZERO, 10.0), holes: vec![Loop2::circle(Vec2::ZERO, 6.0).reversed()] };
        assert!(close(ring.inradius(1e-4), 2.0), "{}", ring.inradius(1e-4));
        assert!(close(ring.holes[0].inradius(1e-4), 6.0));
        assert_eq!(Region2::default().inradius(1e-3), 0.0);
    }

    #[test]
    fn regions_are_apart_only_when_nothing_meets_or_holds_another() {
        let gap = 1e-3;
        // Side by side, a millimetre apart.
        assert!(regions_apart(&[solid(square(0.0, 0.0, 5.0)), solid(square(6.0, 0.0, 5.0))], gap));
        // Touching along a side, or at a corner, or closer than the gap: not apart.
        assert!(!regions_apart(&[solid(square(0.0, 0.0, 5.0)), solid(square(5.0, 0.0, 5.0))], gap));
        assert!(!regions_apart(&[solid(square(0.0, 0.0, 5.0)), solid(square(5.0, 5.0, 5.0))], gap));
        assert!(!regions_apart(&[solid(square(0.0, 0.0, 5.0)), solid(square(5.0005, 0.0, 5.0))], gap));
        // Overlapping, or one inside another: not apart.
        assert!(!regions_apart(&[solid(square(0.0, 0.0, 5.0)), solid(square(3.0, 3.0, 5.0))], gap));
        assert!(!regions_apart(&[solid(square(0.0, 0.0, 10.0)), solid(square(2.0, 2.0, 2.0))], gap));
        // Inside another's hole, clear of its edge: apart (a letter in the middle of an O).
        let ring = Region2 { outer: square(0.0, 0.0, 10.0), holes: vec![square(2.0, 2.0, 6.0).reversed()] };
        assert!(regions_apart(&[ring.clone(), solid(square(4.0, 4.0, 2.0))], gap));
        // Filling the hole: its edge is the hole's.
        assert!(!regions_apart(&[ring, solid(square(2.0, 2.0, 6.0))], gap));
        // Curved boundaries: two circles a little apart, then touching.
        assert!(regions_apart(&[solid(Loop2::circle(Vec2::ZERO, 1.0)), solid(Loop2::circle(Vec2::new(2.01, 0.0), 1.0))], gap));
        assert!(!regions_apart(&[solid(Loop2::circle(Vec2::ZERO, 1.0)), solid(Loop2::circle(Vec2::new(2.0, 0.0), 1.0))], gap));
        // Nothing to compare, and nonsense gaps.
        assert!(regions_apart(&[], gap) && regions_apart(&[solid(square(0.0, 0.0, 1.0))], gap));
        assert!(!regions_apart(&[solid(square(0.0, 0.0, 1.0)), solid(square(5.0, 0.0, 1.0))], f64::NAN));
    }

    #[test]
    fn closest_line_projection_and_degeneracy() {
        let line = Seg2::Line { a: Vec2::new(-3.0, 4.0), b: Vec2::new(7.0, 4.0) };
        for (p, t) in [(Vec2::new(-9.0, 20.0), 0.0), (Vec2::new(2.0, -5.0), 0.5), (Vec2::new(15.0, 4.0), 1.0)] {
            assert_eq!(line.closest_param(p), t);
            assert_eq!(line.reversed().closest_param(p), 1.0 - t);
        }
        let a = Vec2::new(1e100, -1e100);
        let b = a + Vec2::new(1e90, 2e90);
        let large = Seg2::Line { a, b };
        assert!((large.closest_param(a.lerp(b, 0.5)) - 0.5).abs() < 1e-6);
        assert_eq!(Seg2::Line { a, b: a }.closest_param(Vec2::ZERO), 0.0);
        let tiny = Seg2::Line { a: Vec2::ZERO, b: Vec2::new(1e-12, 0.0) };
        assert_eq!(tiny.closest_param(Vec2::new(0.5e-12, 1.0)), 0.5);
        for line in [line, line.reversed(), tiny] {
            for k in 0..53 {
                let p = Vec2::new((k * 17 % 37) as f64 - 18.0, (k * 31 % 29) as f64 - 14.0);
                let closest = line.point_at(line.closest_param(p)).dist(p);
                let dense = (0..=8192).map(|i| line.point_at(i as f64 / 8192.0).dist(p)).fold(f64::INFINITY, f64::min);
                assert!(closest <= dense + 1e-12);
            }
        }
    }

    fn sampled_closest(s: &Seg2, p: Vec2) -> f64 {
        let mut t = (0..=64).map(|i| i as f64 / 64.0).min_by(|x, y| s.point_at(*x).dist(p).total_cmp(&s.point_at(*y).dist(p))).unwrap_or(0.0);
        for _ in 0..30 {
            let (q, d) = (s.point_at(t), s.derivative(t));
            let (d2, h) = ((s.derivative((t + 1e-6).min(1.0)) - s.derivative((t - 1e-6).max(0.0))) / 2e-6, q - p);
            let df = d.dot(d) + h.dot(d2);
            if df.abs() < 1e-300 {
                break;
            }
            let nt = (t - h.dot(d) / df).clamp(0.0, 1.0);
            if (nt - t).abs() < 1e-14 {
                t = nt;
                break;
            }
            t = nt;
        }
        t
    }

    #[test]
    fn closest_projection_preserves_clearance_tolerance_and_generic_fallback() {
        use std::f64::consts::TAU;
        let curves = [
            Seg2::Line { a: Vec2::new(-4.0, 7.0), b: Vec2::new(3.0, -2.0) },
            Seg2::Arc { center: Vec2::new(3.0, -2.0), radius: 6.0, start: 5.9, sweep: 1.3 },
            Seg2::Arc { center: Vec2::ZERO, radius: 2.0, start: 0.2, sweep: -TAU },
            Seg2::Arc { center: Vec2::ZERO, radius: 2.0, start: 0.0, sweep: TAU },
            Seg2::Arc { center: Vec2::ZERO, radius: 2.0, start: 0.0, sweep: 0.0 },
            Seg2::Arc { center: Vec2::ZERO, radius: 2.0, start: 0.0, sweep: 1e-12 },
            Seg2::Cubic { p0: Vec2::ZERO, p1: Vec2::new(2.0, 7.0), p2: Vec2::new(6.0, -3.0), p3: Vec2::new(8.0, 4.0) },
        ];
        for (i, s) in curves.iter().enumerate() {
            for k in 0..101 {
                let p = Vec2::new((k * 17 % 37) as f64 - 18.0, (k * 31 % 29) as f64 - 14.0);
                let old = sampled_closest(s, p);
                let new = s.closest_param(p);
                if i > 0 {
                    assert_eq!(new.to_bits(), old.to_bits());
                }
                assert!(s.point_at(new).dist(p) <= s.point_at(old).dist(p) + 1e-10);
                let on_curve = s.point_at(k as f64 / 100.0);
                assert!(s.point_at(s.closest_param(on_curve)).dist(on_curve) < 1e-9);
            }
        }
    }

    /// The sweep visits only the edges crossing each slab; it must give the triangles that
    /// testing every edge against every slab gives, for a finely sampled ring too (#164).
    #[test]
    fn triangulation_sweep_matches_every_edge_against_every_slab() {
        fn every_edge(r: &Region2, tol: f64) -> Vec<[Vec2; 3]> {
            let mut edges = Vec::new();
            for l in std::iter::once(&r.outer).chain(&r.holes) {
                let pl = l.polyline(tol);
                for i in 0..pl.len() {
                    let (a, b) = (pl[i], pl[(i + 1) % pl.len()]);
                    if (a.y - b.y).abs() > 1e-12 {
                        edges.push(if a.y < b.y { (a, b) } else { (b, a) });
                    }
                }
            }
            let mut ys: Vec<f64> = edges.iter().flat_map(|(a, b): &(Vec2, Vec2)| [a.y, b.y]).collect();
            ys.sort_by(f64::total_cmp);
            ys.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
            let x_at = |e: &(Vec2, Vec2), y: f64| e.0.x + (y - e.0.y) / (e.1.y - e.0.y) * (e.1.x - e.0.x);
            let mut out = Vec::new();
            for w in ys.windows(2) {
                let ym = (w[0] + w[1]) * 0.5;
                let mut span: Vec<_> = edges.iter().filter(|e| e.0.y <= ym && e.1.y >= ym).collect();
                span.sort_by(|a, b| x_at(a, ym).total_cmp(&x_at(b, ym)));
                for pair in span.chunks(2) {
                    let [l, r] = pair else { continue };
                    let (a, b) = (Vec2::new(x_at(l, w[0]), w[0]), Vec2::new(x_at(r, w[0]), w[0]));
                    let (c, d) = (Vec2::new(x_at(r, w[1]), w[1]), Vec2::new(x_at(l, w[1]), w[1]));
                    out.extend([[a, b, c], [a, c, d]]);
                }
            }
            out
        }
        let comb = Loop2::polygon(&[
            Vec2::new(0.0, 0.0),
            Vec2::new(9.0, 0.0),
            Vec2::new(9.0, 6.0),
            Vec2::new(7.0, 2.0),
            Vec2::new(5.0, 7.0),
            Vec2::new(3.0, 2.5),
            Vec2::new(1.0, 6.0),
        ]);
        let ring = Region2 { outer: Loop2::circle(Vec2::ZERO, 45.0), holes: vec![Loop2::circle(Vec2::ZERO, 40.0).reversed()] };
        let regions = [Region2 { outer: comb, holes: vec![Loop2::circle(Vec2::new(4.5, 1.2), 0.6).reversed()] }, ring];
        for (r, tol) in regions.iter().zip([1e-3, 1e-4]) {
            let tris = r.triangulate(tol);
            assert!(tris.len() > 100);
            assert_eq!(tris, every_edge(r, tol));
        }
    }

    #[test]
    fn triangulated_area() {
        let sq = Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 8.0), Vec2::new(5.0, 3.0), Vec2::new(0.0, 8.0)]);
        let r = Region2 { outer: sq, holes: vec![Loop2::circle(Vec2::new(3.0, 1.5), 1.0).reversed()] };
        let tris = r.triangulate(1e-3);
        let a: f64 = tris.iter().map(|[a, b, c]| (*b - *a).cross(*c - *a).abs() * 0.5).sum();
        // The circle is a polyline within the tolerance, so the areas agree to about that.
        assert!((a - r.area()).abs() < 1e-3 * r.area(), "{a} vs {}", r.area());
        for [a, b, c] in &tris {
            let m = (*a + *b + *c) / 3.0;
            assert!((*b - *a).cross(*c - *a).abs() < 1e-12 || r.contains(m), "{m:?}");
        }
    }

    #[test]
    fn areas() {
        let sq = Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 3.0), Vec2::new(0.0, 3.0)]);
        assert!((sq.signed_area() - 12.0).abs() < 1e-12);
        assert!((sq.reversed().signed_area() + 12.0).abs() < 1e-12);
        let c = Loop2::circle(Vec2::new(5.0, 5.0), 2.0);
        assert!((c.signed_area() - std::f64::consts::PI * 4.0).abs() < 1e-9);
        assert!(c.is_closed(1e-9));
        let r = Region2 { outer: Loop2::circle(Vec2::ZERO, 10.0), holes: vec![Loop2::circle(Vec2::ZERO, 5.0).reversed()] };
        assert!((r.area() - std::f64::consts::PI * 75.0).abs() < 1e-9);
        let ip = r.interior_point();
        assert!(r.contains(ip), "{ip:?}");
        assert!(sq.contains(Vec2::new(1.0, 1.0)) && !sq.contains(Vec2::new(5.0, 1.0)));
        assert!(sq.centroid().dist(Vec2::new(2.0, 1.5)) < 1e-9);
    }
}
