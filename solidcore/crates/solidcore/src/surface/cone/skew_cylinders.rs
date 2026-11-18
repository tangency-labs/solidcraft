//! The intersection of two circular cylinders whose axes are not parallel, in closed form.
//!
//! Upstream: none (not upstream; solidcore-native). Upstream solves equal cylinders with
//! meeting axes in closed form (two ellipses) and traces every other pair; the tracer steps
//! across the two branches of the curve where the cylinders nearly touch (two equal
//! cylinders whose axes pass a hair apart), so the pair is solved here instead.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`intersect_skew_cylinders`] | none |
//!
//! **Construction** (Levin's method; Levin 1976, with the branch classification of Wang,
//! Goldman and Tu, CAGD 20 (2003), and Dupont, Lazard, Lazard and Petitjean, JSC 43 (2008)).
//! A point of cylinder A is `P(θ, t) = o_A + r_A (cos θ x_A + sin θ y_A) + t z_A`. Put into
//! cylinder B's equation `|P − o_B|² − ((P − o_B)·z_B)² = r_B²`, it is a quadratic in the
//! height, `α t² + 2 b(θ) t + g(θ) = 0`, with `w = z_A − (z_A·z_B) z_B`, `α = w·w > 0` for
//! axes that are not parallel, `q(θ) = P(θ, 0) − o_B`, `b = q·w` and
//! `g = |q|² − (q·z_B)² − r_B²`. Its discriminant `F(θ) = b² − α g` is a trigonometric
//! polynomial of degree 2, and the curve is `t = (−b ± √F) / α` wherever `F ≥ 0`.
//! - **Folds.** A simple root of `F` is a point where the two heights meet: the curve turns
//!   back there. Between two consecutive simple roots `θ1 < θ2` with `F > 0` between them, the
//!   substitution `θ = m + h cos ψ` (`m`, `h` the middle and half-width) runs once round one
//!   closed loop: `ψ ∈ [0, π]` on one height and `[π, 2π]` on the other, with `sin ψ` giving
//!   the sign of the root, so the folds are ordinary points of the loop.
//! - **No root.** With `F > 0` everywhere, each sign is a closed curve of its own (a thin
//!   cylinder through a wide one); with `F < 0` everywhere the cylinders miss.
//! - **Double roots** are nodes or touches (the branches cross, as for meeting axes, or the
//!   cylinders are tangent at a point): this construction does not take them, and the caller
//!   gives the pair to the general intersector, as before.
//!
//! The roots are isolated with certified bounds: on an interval of half-width `w` round `c`,
//! `|F(c)| > L1 w` proves no root (`L1` bounds `|F'|` from the coefficients) and
//! `|F'(c)| > L2 w` proves at most one, found by bisection where the ends' signs differ.
//!
//! **The curve.** Each loop is cut to both faces' domains (by their STEP parameters at the
//! loop's points, each boundary crossing bisected), and each piece is fitted as a piecewise
//! cubic Hermite B-spline through exact points of the loop with the exact tangent
//! `n_A × n_B`, spans halved until the cubic stays within the approximation tolerance of
//! both cylinders and turns by less than [`MAX_TURN_RAD`]; its deviation is the largest
//! distance found. The trims are left to the drop, as for the ellipses.
//!
//! Deviations: none (no upstream counterpart). The root isolation and the span halving loop
//! with explicit bounds ([`MAX_ROOT_DEPTH`], [`MAX_SPAN_DEPTH`]), not by recursion.

use super::Cone;
use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::vector3d::{sqrt, Point3, Vec3};
use crate::intersect::surface_intersector::p1::flush_crv;
use crate::kernel::Kernel;
use crate::math::{EFF_ZERO, PI};
use crate::surface::surf_of_revolution::p3::Revolved;
use crate::surface::surface::p2::{SurfaceIntersection, TsectCurveType};

const OP: &str = "surface::cone::skew_cylinders::intersect_skew_cylinders";

/// The deepest the root isolation halves `[0, 2π)`: past 2π / 2⁶⁰ two roots are one in
/// double precision.
const MAX_ROOT_DEPTH: u32 = 60;
/// The deepest a span of a loop is halved.
const MAX_SPAN_DEPTH: u32 = 40;
/// The most a fitted span's tangent turns (the tracer's own span angle).
const MAX_TURN_RAD: f64 = 10.0 * PI / 180.0;
/// Spans per loop before halving: enough that every span sees less than a quarter turn of
/// either cylinder.
const START_SPANS: u32 = 32;
/// Samples per loop for the domain cut, before each crossing is bisected.
const CUT_SAMPLES: u32 = 256;

/// A cylinder's frame, radius and STEP trim domain.
struct Cyl {
    o: Point3,
    x: Vec3,
    y: Vec3,
    z: Vec3,
    r: f64,
    /// The face's STEP domain: angle (degrees), height.
    dom: Extent2d,
}

impl Cyl {
    fn new(k: &Kernel, c: &Cone, domain: &Extent2d) -> Result<Self> {
        let p = c.rev().position();
        Ok(Self {
            o: p.origin(),
            x: p.x_axis(),
            y: p.y_axis(),
            z: p.z_axis(),
            r: c.bot_radius(),
            dom: c.convert_domain_from_nurbs_to_step(k, domain)?,
        })
    }

    /// The point's distance from the cylinder.
    fn distance(&self, p: Point3) -> f64 {
        let d = p - self.o;
        let radial = d - self.z * d.dot(self.z);
        (radial.length() - self.r).abs()
    }

    /// The outward unit normal at a point on (or near) the cylinder.
    fn normal(&self, p: Point3) -> Result<Vec3> {
        let d = p - self.o;
        (d - self.z * d.dot(self.z)).unitize()
    }

    /// How far inside the face's STEP domain the point lies (negative outside): the least
    /// margin of its height and, for a face that does not go all round, of its angle.
    fn margin(&self, p: Point3) -> f64 {
        let d = p - self.o;
        let v = d.dot(self.z);
        let mut m = (v - self.dom.v_min()).min(self.dom.v_max() - v);
        let sweep = self.dom.u_max() - self.dom.u_min();
        if sweep < 360.0 - 1e-9 {
            // the angle's distance into the sweep, in arc length at this radius
            let u = d.dot(self.y).atan2(d.dot(self.x)) * 180.0 / PI;
            let a = (u - self.dom.u_min()).rem_euclid(360.0);
            let into = if a <= sweep { a.min(sweep - a) } else { -(a - sweep).min(360.0 - a) };
            m = m.min(into * PI / 180.0 * self.r);
        }
        m
    }
}

/// `F(θ) = c0 + c1 cos θ + s1 sin θ + c2 cos 2θ + s2 sin 2θ`, the discriminant.
#[derive(Clone, Copy)]
struct Trig2 {
    c0: f64,
    c1: f64,
    s1: f64,
    c2: f64,
    s2: f64,
}

impl Trig2 {
    /// The square of `u0 + u1 cos θ + u2 sin θ`.
    fn square(u0: f64, u1: f64, u2: f64) -> Self {
        Self {
            c0: u0 * u0 + (u1 * u1 + u2 * u2) / 2.0,
            c1: 2.0 * u0 * u1,
            s1: 2.0 * u0 * u2,
            c2: (u1 * u1 - u2 * u2) / 2.0,
            s2: u1 * u2,
        }
    }
    fn add(self, o: Self, f: f64) -> Self {
        Self { c0: self.c0 + f * o.c0, c1: self.c1 + f * o.c1, s1: self.s1 + f * o.s1, c2: self.c2 + f * o.c2, s2: self.s2 + f * o.s2 }
    }
    fn at(&self, t: f64) -> f64 {
        self.c0 + self.c1 * t.cos() + self.s1 * t.sin() + self.c2 * (2.0 * t).cos() + self.s2 * (2.0 * t).sin()
    }
    fn d1(&self, t: f64) -> f64 {
        -self.c1 * t.sin() + self.s1 * t.cos() - 2.0 * self.c2 * (2.0 * t).sin() + 2.0 * self.s2 * (2.0 * t).cos()
    }
    /// Bounds of `|F'|` and `|F''|` everywhere.
    fn lipschitz(&self) -> (f64, f64) {
        let one = self.c1.abs() + self.s1.abs();
        let two = self.c2.abs() + self.s2.abs();
        (one + 2.0 * two, one + 4.0 * two)
    }
}

/// The pair's quadratic in the height along A.
struct Levin<'a> {
    a: &'a Cyl,
    b: &'a Cyl,
    w: Vec3,
    alpha: f64,
    f: Trig2,
}

/// Where the discriminant's roots leave the construction.
enum Roots {
    /// Simple roots, increasing in `[0, 2π)`.
    Simple(Vec<f64>),
    /// A double root (a node or a touch), or roots closer than double precision tells apart.
    Degenerate,
}

impl<'a> Levin<'a> {
    fn new(a: &'a Cyl, b: &'a Cyl) -> Self {
        let w = a.z - b.z * a.z.dot(b.z);
        let alpha = w.dot(w);
        let d = a.o - b.o;
        let (e1, e2) = (a.x * a.r, a.y * a.r);
        // b(θ) = q·w and q·z_B, each u0 + u1 cos θ + u2 sin θ
        let bq = Trig2::square(d.dot(w), e1.dot(w), e2.dot(w));
        let nq = Trig2::square(d.dot(b.z), e1.dot(b.z), e2.dot(b.z));
        // |q|² − r_B² = |d|² + r_A² − r_B² + 2 cos θ d·e1 + 2 sin θ d·e2 (e1 ⊥ e2, |e| = r_A)
        let qq = Trig2 { c0: d.dot(d) + a.r * a.r - b.r * b.r, c1: 2.0 * d.dot(e1), s1: 2.0 * d.dot(e2), c2: 0.0, s2: 0.0 };
        // F = b² − α (|q|² − (q·z_B)² − r_B²)
        let f = bq.add(qq, -alpha).add(nq, alpha);
        Self { a, b, w, alpha, f }
    }

    /// The roots of `F` in `[0, 2π)`, isolated with certified bounds (module docs).
    fn roots(&self) -> Roots {
        let (l1, l2) = self.f.lipschitz();
        let mut found = Vec::new();
        let mut stack = vec![(0.0_f64, 2.0 * PI, 0_u32)];
        while let Some((lo, hi, depth)) = stack.pop() {
            let (c, w) = ((lo + hi) / 2.0, (hi - lo) / 2.0);
            if self.f.at(c).abs() > l1 * w {
                continue;
            }
            if self.f.d1(c).abs() > l2 * w {
                let (flo, fhi) = (self.f.at(lo), self.f.at(hi));
                if flo == 0.0 {
                    found.push(lo);
                } else if (flo < 0.0) != (fhi < 0.0) {
                    found.push(bisect(lo, hi, flo, |t| self.f.at(t)));
                }
                continue;
            }
            if depth >= MAX_ROOT_DEPTH {
                return Roots::Degenerate;
            }
            stack.push((c, hi, depth + 1));
            stack.push((lo, c, depth + 1));
        }
        found.sort_by(f64::total_cmp);
        found.dedup_by(|x, y| (*x - *y).abs() <= 4.0 * f64::EPSILON * 2.0 * PI);
        if found.len() % 2 == 1 {
            return Roots::Degenerate;
        }
        Roots::Simple(found)
    }

    /// The point of the curve at angle `θ` on A, on the height of sign `sign`.
    fn point(&self, theta: f64, sign: f64) -> Point3 {
        let (c, s) = (theta.cos(), theta.sin());
        let p0 = self.a.o + self.a.x * (self.a.r * c) + self.a.y * (self.a.r * s);
        let q = p0 - self.b.o;
        let bq = q.dot(self.w);
        let root = sqrt(self.f.at(theta).max(0.0));
        p0 + self.a.z * ((-bq + sign * root) / self.alpha)
    }
}

/// A closed loop of the curve, by its parameter `ψ ∈ [0, 2π]`.
#[derive(Clone, Copy)]
enum Loop {
    /// `θ = ψ`, on one height all round.
    Full { sign: f64 },
    /// `θ = m + h cos ψ` between two folds.
    Folds { m: f64, h: f64 },
}

impl Loop {
    fn point(self, l: &Levin<'_>, psi: f64) -> Point3 {
        match self {
            Self::Full { sign } => l.point(psi, sign),
            Self::Folds { m, h } => l.point(m + h * psi.cos(), if psi.sin() >= 0.0 { 1.0 } else { -1.0 }),
        }
    }
}

/// Bisection of a sign change of `f` on `[lo, hi]` (`f(lo) = flo`), to the end of the doubles.
fn bisect(mut lo: f64, mut hi: f64, flo: f64, f: impl Fn(f64) -> f64) -> f64 {
    let neg = flo < 0.0;
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        if mid <= lo || mid >= hi {
            break;
        }
        if (f(mid) < 0.0) == neg {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) / 2.0
}

/// The intersection of cylinders `a` and `b` (axes not parallel), over their face domains,
/// into `out`, curves approximated within `tol`. `Ok(false)` when it is solved here (with or
/// without curves); `Ok(true)` for a node or a touch, which the general intersector takes.
///
/// # Errors
///
/// A frame that does not unitize, or a span that does not fit within `tol` once halved
/// [`MAX_SPAN_DEPTH`] times; those of the domain conversions and the curve construction.
pub(crate) fn intersect_skew_cylinders(
    k: &Kernel,
    a: &Cone,
    a_domain: &Extent2d,
    b: &Cone,
    b_domain: &Extent2d,
    tol: f64,
    out: &mut SurfaceIntersection,
) -> Result<bool> {
    let (ca, cb) = (Cyl::new(k, a, a_domain)?, Cyl::new(k, b, b_domain)?);
    let levin = Levin::new(&ca, &cb);
    if !(levin.alpha > 0.0) {
        return Err(SolidError::invalid_input(OP, "the axes are parallel"));
    }
    let loops = match levin.roots() {
        Roots::Degenerate => return Ok(true),
        Roots::Simple(r) if r.is_empty() => {
            if levin.f.at(0.0) > 0.0 {
                vec![Loop::Full { sign: 1.0 }, Loop::Full { sign: -1.0 }]
            } else {
                Vec::new()
            }
        }
        Roots::Simple(r) => {
            let mut loops = Vec::new();
            for (i, &t1) in r.iter().enumerate() {
                let t2 = r.get(i + 1).copied().unwrap_or_else(|| r.first().copied().unwrap_or(t1) + 2.0 * PI);
                let m = (t1 + t2) / 2.0;
                if levin.f.at(m) > 0.0 {
                    loops.push(Loop::Folds { m, h: (t2 - t1) / 2.0 });
                }
            }
            loops
        }
    };
    for lp in loops {
        for (lo, hi) in cut_to_domains(&levin, lp) {
            let (curve, deviation) = fit(&levin, lp, lo, hi, tol)?;
            out.curves_3d.push(curve);
            out.surface1_uv_curves.push(None);
            out.surface2_uv_curves.push(None);
            out.curve_types.push(TsectCurveType::Crossing);
            out.deviations.push(deviation);
        }
    }
    Ok(false)
}

/// The pieces of the loop inside both faces' domains, as intervals of `ψ`: the whole loop
/// `[0, 2π]` when it never leaves them.
fn cut_to_domains(l: &Levin<'_>, lp: Loop) -> Vec<(f64, f64)> {
    let margin = |psi: f64| {
        let p = lp.point(l, psi);
        l.a.margin(p).min(l.b.margin(p))
    };
    let step = 2.0 * PI / f64::from(CUT_SAMPLES);
    let ms: Vec<f64> = (0..=CUT_SAMPLES).map(|i| margin(f64::from(i) * step)).collect();
    if ms.iter().all(|m| *m >= 0.0) {
        return vec![(0.0, 2.0 * PI)];
    }
    // the crossings, each where the margin changes sign
    let mut crossings = Vec::new();
    for (i, pair) in (0_u32..).zip(ms.windows(2)) {
        if let [m0, m1] = *pair {
            if (m0 < 0.0) != (m1 < 0.0) {
                let lo = f64::from(i) * step;
                crossings.push((bisect(lo, lo + step, m0, margin), m1 >= 0.0));
            }
        }
    }
    // each entering crossing to the next leaving one, round the loop
    let mut pieces = Vec::new();
    let n = crossings.len();
    for (i, &(at, entering)) in crossings.iter().enumerate() {
        if !entering {
            continue;
        }
        if let Some(&(next, _)) = crossings.iter().cycle().skip(i + 1).take(n).find(|c| !c.1) {
            let end = if next > at { next } else { next + 2.0 * PI };
            pieces.push((at, end));
        }
    }
    pieces
}

/// The loop between `ψ = lo` and `hi` as a piecewise cubic Hermite B-spline (module docs),
/// and its largest distance from the two cylinders.
fn fit(l: &Levin<'_>, lp: Loop, lo: f64, hi: f64, tol: f64) -> Result<(Curve, f64)> {
    let tangent = |p: Point3| -> Result<Vec3> { l.a.normal(p)?.cross(l.b.normal(p)?).unitize() };
    // spans as parameter intervals, halved until each fits
    let mut todo: Vec<(f64, f64, u32)> = (0..START_SPANS)
        .rev()
        .map(|i| {
            let f = (hi - lo) / f64::from(START_SPANS);
            (lo + f64::from(i) * f, lo + f64::from(i + 1) * f, 0)
        })
        .collect();
    let mut knots = Vec::new();
    let mut poly = Vec::new();
    let mut deviation = 0.0_f64;
    let mut length = 0.0;
    while let Some((s0, s1, depth)) = todo.pop() {
        let (p0, p1) = (lp.point(l, s0), lp.point(l, s1));
        let chord = p1 - p0;
        let h = chord.length();
        let (mut t0, mut t1) = (tangent(p0)?, tangent(p1)?);
        if t0.dot(chord) < 0.0 {
            t0 = -t0;
        }
        if t1.dot(chord) < 0.0 {
            t1 = -t1;
        }
        let (c1, c2) = (p0 + t0 * (h / 3.0), p1 - t1 * (h / 3.0));
        let mut worst = 0.0_f64;
        for j in 1..8 {
            let u = f64::from(j) / 8.0;
            let v = 1.0 - u;
            let q = p0 * (v * v * v) + c1 * (3.0 * v * v * u) + c2 * (3.0 * v * u * u) + p1 * (u * u * u);
            worst = worst.max(l.a.distance(q)).max(l.b.distance(q));
        }
        let turned = t0.angle_between(t1).unwrap_or(PI);
        let off = l.a.distance(p0).max(l.b.distance(p0));
        if (worst > tol || turned > MAX_TURN_RAD) && h > EFF_ZERO * (1.0 + p0.max_dimension()) {
            if depth >= MAX_SPAN_DEPTH {
                return Err(SolidError::intersect_failed(OP, "a span of the curve does not fit within the tolerance"));
            }
            let mid = (s0 + s1) / 2.0;
            todo.push((mid, s1, depth + 1));
            todo.push((s0, mid, depth + 1));
            continue;
        }
        deviation = deviation.max(worst).max(off);
        if poly.is_empty() {
            knots.push(0.0);
            poly.push(p0);
        }
        length += h.max(EFF_ZERO);
        knots.push(length);
        poly.extend_from_slice(&[c1, c2, p1]);
    }
    let mut curves = Vec::new();
    flush_crv(3, &poly, &knots, &mut curves)?;
    let curve = curves.pop().ok_or_else(|| SolidError::intersect_failed(OP, "no curve was made"))?;
    Ok((curve, deviation))
}
