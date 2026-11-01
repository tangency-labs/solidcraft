//! The exact UV curve of a curve lying on a cylinder: the opt-in replacement
//! ([`UpstreamFixes::cylinder_plane_section_trims_exact`](crate::kernel::UpstreamFixes::cylinder_plane_section_trims_exact),
//! issue #2109, WR-680) of the traced fit `SmSurface::DropCurve` makes for it.
//!
//! Upstream: none. `SmSurface::DropCurve` (`source/SMLib/src/SmSurfaceDropLib.cpp`,
//! [`Surface::drop_curve_base`]) traces every curve on a surface other than a plane and
//! fits a piecewise cubic to it within the drop tolerance.
//!
//! | Rust                     | Upstream                                                |
//! |--------------------------|---------------------------------------------------------|
//! | `exact_uv_curve` (crate) | none: the opt-in fix's exact UV curve, tried before the trace |
//!
//! **The construction.** The cylinder's NURB sweeps a rational quadratic circle along
//! its axis with degree 1. On one Bezier segment of the circle, control points `q0`,
//! `q1`, `q2` with weights `w0`, `w1`, `w2`, the point at segment parameter `s` has the
//! barycentric coordinates `b0 = w0 (1-s)^2 / D`, `b1 = 2 w1 s (1-s) / D`,
//! `b2 = w2 s^2 / D` in the triangle `q0 q1 q2`, so
//!
//! ```text
//! s / (1 - s) = (w0 b1 + 2 w1 b2) / (2 w1 b0 + w2 b1)
//! ```
//!
//! a ratio of affine functions of the point whose two terms are not negative on the
//! arc. Projected along the axis, a rational curve `C(t) = N(t) / W(t)` of degree `p`
//! that lies on the cylinder therefore has `s(t) = n(t) / d(t)`, `n` and `d`
//! polynomials of degree `p` whose Bernstein coefficients are those affine maps of the
//! curve's homogeneous control points, and its height along the axis, so the linear
//! NURB parameter, is a polynomial over `W(t)`. Over the common denominator
//! `d(t) W(t)` the UV curve is a rational Bezier curve of degree `2p` in the curve's
//! own parameter on every piece of the curve between its knots and the points where
//! it crosses a joint of the circle's segments. A plane section of the cylinder (an
//! ellipse, circle or line) is such a curve, and so is an exact round's rail on it.
//!
//! Deviations: not a port. The result is used only where it checks out: the curve is a
//! B-spline kind with its NURB (B-spline, line, ellipse or circle, parabola, hyperbola)
//! over the interval; it does not cross the cylinder's seam or leave the domain; it is
//! not an iso-parameter line (`DropIsoCurve` already gives those exactly); every
//! piece's denominator has positive Bezier weights (a piece is halved up to
//! [`MAX_HALVINGS`] times to get there) and the weights fit NLib's range; and the UV
//! curve lifted onto the surface is within [`ON_SURFACE_REL`] (relative to the model's
//! size) of the curve at sampled parameters. Otherwise the answer is `None` and the
//! drop traces as upstream. No recursion: the pieces wait on an explicit list, bounded
//! by the halvings.

use crate::curve::curve::{Curve, CurveKind};
use crate::error::{ignored, Result};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::EFF_ZERO_PARAM;
use crate::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::crv_basic::Curve as NurbCurve;
use crate::nurbs::math::{CPoint, NOW, NOZ};
use crate::nurbs::srf_basic::{WEIGHT_MAX, WEIGHT_MIN};
use crate::surface::cone::Cone;
use crate::surface::surface::{Surface, SurfaceKind};

/// How far, relative to `1 +` the model's size (the largest sampled point's distance
/// from the origin plus the radius), the UV curve lifted onto the cylinder may be from
/// the curve at a sampled parameter: round-off, far below any drop tolerance.
pub const ON_SURFACE_REL: f64 = 1e-10;

/// The most times a piece whose denominator has a non-positive Bezier weight is halved.
pub const MAX_HALVINGS: usize = 8;

/// Samples per piece of the curve at which a crossing of a segment joint is looked for.
const CROSSING_SAMPLES: usize = 32;

/// Samples per piece at which the UV curve is checked against the curve.
const CHECK_SAMPLES: usize = 8;

/// A crossing this close (as a fraction of its piece) to a piece's end is that end.
const CROSSING_END: f64 = 1e-9;

/// Bisection steps for a crossing (the bracket halves to round-off well before).
const BISECTIONS: usize = 80;

/// A homogeneous point `(w x, w y, w z, w)`.
type H4 = [f64; 4];

/// The homogeneous point of an NLib control point (`None` for a 2D one).
fn homogeneous(c: CPoint) -> Option<H4> {
    if c.z == NOZ {
        return None;
    }
    if c.w == NOW {
        Some([c.x, c.y, c.z, 1.0])
    } else {
        Some([c.x, c.y, c.z, c.w])
    }
}

/// `(1 - a) x + a y`.
fn lerp(a: f64, x: H4, y: H4) -> H4 {
    let [x0, x1, x2, x3] = x;
    let [y0, y1, y2, y3] = y;
    let b = 1.0 - a;
    [
        b * x0 + a * y0,
        b * x1 + a * y1,
        b * x2 + a * y2,
        b * x3 + a * y3,
    ]
}

/// The blossom of the B-spline of degree `p` with control points `pw` and `knots` at
/// the `p` values `args`, all in knot span `span` (`knots[span] < knots[span + 1]`):
/// de Boor's algorithm with the `r`-th argument at level `r`.
fn blossom(pw: &[H4], knots: &[f64], p: usize, span: usize, args: &[f64]) -> Option<H4> {
    if args.len() != p {
        return None;
    }
    let first = span.checked_sub(p)?;
    let mut d: Vec<H4> = pw.get(first..=span)?.to_vec();
    for (r, &t) in (1..=p).zip(args) {
        for i in (r..=p).rev() {
            let lo = *knots.get(first + i)?;
            let hi = *knots.get(first + i + p + 1 - r)?;
            if hi <= lo {
                return None;
            }
            let a = (t - lo) / (hi - lo);
            let prev = *d.get(i - 1)?;
            let cur = d.get_mut(i)?;
            *cur = lerp(a, prev, *cur);
        }
    }
    d.get(p).copied()
}

/// The homogeneous Bezier control points of the B-spline over `[a, b]` inside knot
/// span `span`.
fn bezier(pw: &[H4], knots: &[f64], p: usize, span: usize, a: f64, b: f64) -> Option<Vec<H4>> {
    (0..=p)
        .map(|r| {
            let args: Vec<f64> = (0..p).map(|i| if i < p - r { a } else { b }).collect();
            blossom(pw, knots, p, span, &args)
        })
        .collect()
}

/// The knot span `s` (`p <= s <= n`, `knots[s] < knots[s + 1]`) holding `[a, b]`.
fn span_of(knots: &[f64], p: usize, n: usize, a: f64, b: f64) -> Option<usize> {
    let mid = 0.5 * (a + b);
    (p..=n).find(|&s| match (knots.get(s), knots.get(s + 1)) {
        (Some(&lo), Some(&hi)) => lo < hi && lo <= mid && mid <= hi,
        _ => false,
    })
}

/// The value at `s` of the polynomial with Bernstein coefficients `c` (de Casteljau).
fn casteljau(c: &[f64], s: f64) -> Option<f64> {
    let mut b = c.to_vec();
    for r in 1..b.len() {
        for i in 0..b.len() - r {
            let next = *b.get(i + 1)?;
            let cur = b.get_mut(i)?;
            *cur = (1.0 - s) * *cur + s * next;
        }
    }
    b.first().copied()
}

/// `n` choose `k`.
fn binomial(n: usize, k: usize) -> f64 {
    (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64)
}

/// The Bernstein coefficients (degree `2p`) of the product of two polynomials of degree
/// `p` given by theirs.
fn product(a: &[f64], b: &[f64]) -> Option<Vec<f64>> {
    let p = a.len().checked_sub(1)?;
    if b.len() != a.len() {
        return None;
    }
    let q = 2 * p;
    let mut out = vec![0.0; q + 1];
    for (i, ai) in a.iter().enumerate() {
        for (j, bj) in b.iter().enumerate() {
            let f = binomial(p, i) * binomial(p, j) / binomial(q, i + j);
            *out.get_mut(i + j)? += f * ai * bj;
        }
    }
    Some(out)
}

/// One rational quadratic Bezier segment of the cylinder's circle.
struct Segment {
    /// Its NURB parameter range on the circle.
    knots: [f64; 2],
    /// Its weights `w0`, `w1`, `w2`.
    w: [f64; 3],
    /// The barycentric coordinates in its control triangle as affine maps of the
    /// point `(x, y)` of the section plane: `b_i = g_i.0 x + g_i.1 y + h_i`.
    g: [[f64; 2]; 3],
    /// The constant terms `h_i`.
    h: [f64; 3],
}

impl Segment {
    /// The segment of control points `q` (in the section plane) and weights `w` over
    /// `knots`; `None` for a flat triangle.
    fn new(knots: [f64; 2], q: [[f64; 2]; 3], w: [f64; 3]) -> Option<Self> {
        let [q0, q1, q2] = q;
        let cross = |a: [f64; 2], b: [f64; 2]| a[0] * b[1] - a[1] * b[0];
        let area = cross(
            [q1[0] - q0[0], q1[1] - q0[1]],
            [q2[0] - q0[0], q2[1] - q0[1]],
        );
        if area.abs() <= f64::EPSILON * (q0[0].abs() + q0[1].abs() + 1.0).powi(2) {
            return None;
        }
        // b_i = cross(q_{i+1} - X, q_{i+2} - X) / area
        let map = |b: [f64; 2], c: [f64; 2]| {
            (
                [(b[1] - c[1]) / area, (c[0] - b[0]) / area],
                cross(b, c) / area,
            )
        };
        let (g0, h0) = map(q1, q2);
        let (g1, h1) = map(q2, q0);
        let (g2, h2) = map(q0, q1);
        Some(Self {
            knots,
            w,
            g: [g0, g1, g2],
            h: [h0, h1, h2],
        })
    }

    /// The barycentric coordinate `b1` of `(x, y)`: not negative exactly on the
    /// segment's arc, for a point of the circle.
    fn b1(&self, x: f64, y: f64) -> f64 {
        let [_, g, _] = self.g;
        let [_, h, _] = self.h;
        g[0] * x + g[1] * y + h
    }

    /// The Bernstein coefficients of `b_i W` from the homogeneous section-plane
    /// coordinates `(X, Y, W)` of the curve's Bezier control points.
    fn weighted_bary(&self, xyw: &[[f64; 3]], i: usize) -> Option<Vec<f64>> {
        let g = *self.g.get(i)?;
        let h = *self.h.get(i)?;
        Some(
            xyw.iter()
                .map(|&[x, y, w]| g[0] * x + g[1] * y + h * w)
                .collect(),
        )
    }
}

/// The cylinder's NURB in the form the construction reads.
struct Section {
    /// A point of the axis.
    origin: Point3,
    /// The unit axis and two unit directions across it.
    axis: Vec3,
    e1: Vec3,
    e2: Vec3,
    /// The circle's radius.
    radius: f64,
    /// The circle's segments, in order.
    segments: Vec<Segment>,
    /// The joints between the segments and the circle's ends, in the section plane.
    joints: Vec<[f64; 2]>,
    /// The linear NURB parameter is `v0 + (h - h0) * dv` at height `h` along the axis.
    v0: f64,
    h0: f64,
    dv: f64,
    /// The NURB's v is the circle's direction.
    swap: bool,
}

impl Section {
    /// The coordinates `(x, y, h)` across and along the axis of a homogeneous point,
    /// homogeneous: `(w x, w y, w h, w)`.
    fn local(&self, p: H4) -> [f64; 4] {
        let [x, y, z, w] = p;
        let d = Vec3::new(x, y, z) - self.origin * w;
        [d.dot(self.e1), d.dot(self.e2), d.dot(self.axis), w]
    }

    /// The section of a cylinder's NURB: its circle direction of degree 2, its linear
    /// direction of degree 1 with two rows of the same weights, the rows at two
    /// heights; `None` for any other.
    fn of(cone: &Cone) -> Option<Self> {
        if !cone.is_cylinder() {
            return None;
        }
        let rev = cone.rev();
        let nurb = rev.base().gw_nurb()?;
        let swap = rev.swap_uv();
        let position = rev.position();
        let axis = position.z_axis().unitize().ok()?;
        let x = position.x_axis();
        let e1 = (x - axis * x.dot(axis)).unitize().ok()?;
        let e2 = axis.cross(e1);
        let origin = position.origin();

        let (p, q) = nurb.degrees();
        let (circle_degree, linear_degree, circle_knots, linear_knots, n_circle, n_linear) = if swap
        {
            (q, p, nurb.v_knots(), nurb.u_knots(), nurb.m(), nurb.n())
        } else {
            (p, q, nurb.u_knots(), nurb.v_knots(), nurb.n(), nurb.m())
        };
        if circle_degree != 2 || linear_degree != 1 || n_linear != 1 {
            return None;
        }
        let at = |c: usize, l: usize| {
            let cpt = if swap { nurb.cpt(l, c) } else { nurb.cpt(c, l) };
            homogeneous(cpt.ok()?)
        };
        let row0: Vec<H4> = (0..=n_circle).map(|c| at(c, 0)).collect::<Option<_>>()?;
        let row1: Vec<H4> = (0..=n_circle).map(|c| at(c, 1)).collect::<Option<_>>()?;
        // the linear direction must be linear in the height: the same weights
        for (a, b) in row0.iter().zip(&row1) {
            if (a[3] - b[3]).abs() > 1e-12 * a[3].abs() || a[3] <= 0.0 {
                return None;
            }
        }
        let mut section = Self {
            origin,
            axis,
            e1,
            e2,
            radius: 0.0,
            segments: Vec::new(),
            joints: Vec::new(),
            v0: 0.0,
            h0: 0.0,
            dv: 0.0,
            swap,
        };
        let height = |p: H4| {
            let [_, _, h, w] = section.local(p);
            h / w
        };
        let h0 = height(*row0.first()?);
        let h1 = height(*row1.first()?);
        let v0 = *linear_knots.get(1)?;
        let v1 = *linear_knots.get(2)?;
        if h1 == h0 || v1 <= v0 {
            return None;
        }

        // the circle's Bezier segments in the section plane
        let mut segments = Vec::new();
        let mut joints: Vec<[f64; 2]> = Vec::new();
        let in_plane = |p: H4| {
            let [x, y, _, w] = section.local(p);
            [x / w, y / w]
        };
        for s in 2..=n_circle {
            let (lo, hi) = (*circle_knots.get(s)?, *circle_knots.get(s + 1)?);
            if lo >= hi {
                continue;
            }
            let b = bezier(&row0, circle_knots, 2, s, lo, hi)?;
            let [b0, b1, b2] = <[H4; 3]>::try_from(b).ok()?;
            let q = [in_plane(b0), in_plane(b1), in_plane(b2)];
            let w = [b0[3], b1[3], b2[3]];
            if w.iter().any(|&w| w <= 0.0) {
                return None;
            }
            joints.push(q[0]);
            segments.push(Segment::new([lo, hi], q, w)?);
            if s == n_circle {
                joints.push(q[2]);
            }
        }
        let first = *joints.first()?;
        let radius = first[0].hypot(first[1]);
        if segments.is_empty() || radius <= 0.0 {
            return None;
        }
        // the circle's two ends are one joint (the seam) when it is whole
        joints.dedup_by(|a, b| (a[0] - b[0]).hypot(a[1] - b[1]) <= 1e-12 * radius);
        if joints.len() > 1 {
            if let (Some(a), Some(b)) = (joints.first(), joints.last()) {
                if (a[0] - b[0]).hypot(a[1] - b[1]) <= 1e-12 * radius {
                    joints.pop();
                }
            }
        }
        section.radius = radius;
        section.segments = segments;
        section.joints = joints;
        section.v0 = v0;
        section.h0 = h0;
        section.dv = (v1 - v0) / (h1 - h0);
        Some(section)
    }

    /// The parameters `t` in `(a, b)` where the curve's Bezier piece over `[a, b]`
    /// (local homogeneous control points `xyw`, see [`Section::local`]) crosses the ray
    /// from the axis through a joint.
    fn crossings(&self, xyw: &[[f64; 3]], a: f64, b: f64) -> Option<Vec<f64>> {
        let mut out = Vec::new();
        for &[jx, jy] in &self.joints {
            let across: Vec<f64> = xyw.iter().map(|&[x, y, _]| jx * y - jy * x).collect();
            let along: Vec<f64> = xyw.iter().map(|&[x, y, _]| jx * x + jy * y).collect();
            let at = |s: usize| s as f64 / CROSSING_SAMPLES as f64;
            let mut prev = casteljau(&across, 0.0)?;
            for m in 1..=CROSSING_SAMPLES {
                let f = casteljau(&across, at(m))?;
                let root = if prev == 0.0 {
                    None
                } else if f == 0.0 {
                    (m < CROSSING_SAMPLES).then(|| at(m))
                } else if (f < 0.0) != (prev < 0.0) {
                    // bisect the sign change
                    let (mut lo, mut hi, mut f_lo) = (at(m - 1), at(m), prev);
                    for _ in 0..BISECTIONS {
                        let mid = 0.5 * (lo + hi);
                        if mid <= lo || mid >= hi {
                            break;
                        }
                        let f_mid = casteljau(&across, mid)?;
                        if f_mid == 0.0 {
                            lo = mid;
                            hi = mid;
                            break;
                        }
                        if (f_mid < 0.0) == (f_lo < 0.0) {
                            lo = mid;
                            f_lo = f_mid;
                        } else {
                            hi = mid;
                        }
                    }
                    Some(0.5 * (lo + hi))
                } else {
                    None
                };
                prev = f;
                let Some(s) = root else { continue };
                if s > CROSSING_END && s < 1.0 - CROSSING_END && casteljau(&along, s)? > 0.0 {
                    out.push(a + s * (b - a));
                }
            }
        }
        Some(out)
    }

    /// The homogeneous UV control points (in the NURB's `(u, v)` order, then the
    /// weight) of the curve's Bezier piece with homogeneous control points `bez`, on
    /// the circle segment its middle lies on (whose index is returned too); `None`
    /// when the middle is on no segment or a weight is not positive.
    fn uv_piece(&self, bez: &[H4]) -> Option<(usize, Vec<[f64; 3]>)> {
        let local: Vec<[f64; 4]> = bez.iter().map(|&p| self.local(p)).collect();
        let xyw: Vec<[f64; 3]> = local.iter().map(|&[x, y, _, w]| [x, y, w]).collect();
        let coef = |f: fn(&[f64; 4]) -> f64| -> Vec<f64> { local.iter().map(f).collect() };
        let (xs, ys, ws) = (coef(|l| l[0]), coef(|l| l[1]), coef(|l| l[3]));
        let w_mid = casteljau(&ws, 0.5)?;
        let (x_mid, y_mid) = (casteljau(&xs, 0.5)? / w_mid, casteljau(&ys, 0.5)? / w_mid);
        let (index, segment) = self
            .segments
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.b1(x_mid, y_mid).total_cmp(&b.b1(x_mid, y_mid)))?;
        if segment.b1(x_mid, y_mid) < -1e-9 {
            return None;
        }
        let [w0, w1, w2] = segment.w;
        let b0 = segment.weighted_bary(&xyw, 0)?;
        let b1 = segment.weighted_bary(&xyw, 1)?;
        let b2 = segment.weighted_bary(&xyw, 2)?;
        // s = n / d on the segment
        let n: Vec<f64> = b1
            .iter()
            .zip(&b2)
            .map(|(b1, b2)| w0 * b1 + 2.0 * w1 * b2)
            .collect();
        let d: Vec<f64> = n
            .iter()
            .zip(b0.iter().zip(&b1))
            .map(|(n, (b0, b1))| n + 2.0 * w1 * b0 + w2 * b1)
            .collect();
        let [k0, k1] = segment.knots;
        let circle: Vec<f64> = d
            .iter()
            .zip(&n)
            .map(|(d, n)| k0 * d + (k1 - k0) * n)
            .collect();
        let linear: Vec<f64> = local
            .iter()
            .map(|&[_, _, h, w]| self.v0 * w + (h - self.h0 * w) * self.dv)
            .collect();
        // over the common denominator d W
        let weight = product(&d, &ws)?;
        let circle = product(&circle, &ws)?;
        let linear = product(&linear, &d)?;
        if weight.iter().any(|&w| !w.is_finite() || w <= 0.0) {
            return None;
        }
        let ctrl = weight
            .iter()
            .zip(circle.iter().zip(&linear))
            .map(|(&w, (&c, &l))| if self.swap { [l, c, w] } else { [c, l, w] })
            .collect();
        Some((index, ctrl))
    }
}

/// The NURB of a curve of a B-spline kind.
fn curve_nurb(curve: &Curve) -> Option<&NurbCurve> {
    let base = match curve.kind() {
        CurveKind::BSpline(c) => c,
        CurveKind::Line(c) => c.base(),
        CurveKind::Ellipse(c) => c.base(),
        CurveKind::Parabola(c) => c.base(),
        CurveKind::Hyperbola(c) => c.base(),
        _ => return None,
    };
    base.gw_nurb()
}

/// A piece of the UV curve: its parameter range, the circle segment it lies on and its
/// homogeneous Bezier control points `(w u, w v, w)`.
struct Piece {
    t: [f64; 2],
    segment: usize,
    ctrl: Vec<[f64; 3]>,
}

/// The UV pieces of the NURB `nurb` over `interval` on `section`.
fn pieces(section: &Section, nurb: &NurbCurve, interval: &Extent1d) -> Option<Vec<Piece>> {
    let pw: Vec<H4> = nurb
        .cpts()
        .iter()
        .map(|&c| homogeneous(c))
        .collect::<Option<_>>()?;
    if pw.iter().any(|p| p[3] <= 0.0) {
        return None;
    }
    let knots = nurb.knots();
    let (p, n) = (nurb.degree(), nurb.n());
    let (lo, hi) = (*knots.get(p)?, *knots.get(n + 1)?);
    let (t0, t1) = (interval.min(), interval.max());
    let slack = 1e-12 * (1.0 + (hi - lo).abs());
    if t0.is_nan() || t1.is_nan() || t1 <= t0 || t0 < lo - slack || t1 > hi + slack {
        return None;
    }
    let (t0, t1) = (t0.max(lo), t1.min(hi));
    let near = 1e-12 * (t1 - t0);

    // the curve's knots inside the interval, then the joint crossings of each span
    let mut breaks = vec![t0];
    for &u in knots {
        if u > t0 + near && u < t1 - near && breaks.last().is_some_and(|&l| u > l + near) {
            breaks.push(u);
        }
    }
    breaks.push(t1);
    let mut crossings = Vec::new();
    for w in breaks.windows(2) {
        let &[a, b] = w else { return None };
        let bez = bezier(&pw, knots, p, span_of(knots, p, n, a, b)?, a, b)?;
        let xyw: Vec<[f64; 3]> = bez
            .iter()
            .map(|&q| {
                let [x, y, _, w] = section.local(q);
                [x, y, w]
            })
            .collect();
        crossings.extend(section.crossings(&xyw, a, b)?);
    }
    breaks.extend(crossings);
    breaks.sort_by(f64::total_cmp);
    breaks.dedup_by(|a, b| (*a - *b).abs() <= near);

    // the pieces, halved where a weight is not positive (in order: the left half first)
    let mut todo: Vec<([f64; 2], usize)> = breaks
        .windows(2)
        .rev()
        .filter_map(|w| match *w {
            [a, b] => Some(([a, b], 0)),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    while let Some(([a, b], halvings)) = todo.pop() {
        let bez = bezier(&pw, knots, p, span_of(knots, p, n, a, b)?, a, b)?;
        if let Some((segment, ctrl)) = section.uv_piece(&bez) {
            out.push(Piece {
                t: [a, b],
                segment,
                ctrl,
            });
        } else if halvings < MAX_HALVINGS {
            let m = 0.5 * (a + b);
            todo.push(([m, b], halvings + 1));
            todo.push(([a, m], halvings + 1));
        } else {
            return None;
        }
    }
    Some(out)
}

/// The UV curve's B-spline data: knots, multiplicities, Euclidean control points and
/// weights.
struct UvData {
    knots: Vec<f64>,
    multiplicities: Vec<usize>,
    points: Vec<Point3>,
    weights: Vec<f64>,
    degree: usize,
}

/// Joins the pieces into one B-spline (continuous at the piece ends, each end's
/// control point shared): `None` when the pieces jump across the seam, a curve point
/// is outside `uv_domain` (by more than `uv_tol`, else it is moved onto it), the
/// curve is an iso-parameter line or the weights do not fit NLib's range.
fn join(pieces: &[Piece], uv_domain: &Extent2d, uv_tol: f64) -> Option<UvData> {
    let degree = pieces.first()?.ctrl.len().checked_sub(1)?;
    let mut data = UvData {
        knots: vec![pieces.first()?.t[0]],
        multiplicities: vec![degree + 1],
        points: Vec::new(),
        weights: Vec::new(),
        degree,
    };
    let mut prev: Option<(usize, [f64; 3])> = None;
    for piece in pieces {
        if piece.ctrl.len() != degree + 1 {
            return None;
        }
        let first = *piece.ctrl.first()?;
        let scale = match prev {
            None => 1.0,
            Some((segment, last)) => {
                // across the seam the circle's parameter jumps
                if segment.abs_diff(piece.segment) > 1 {
                    return None;
                }
                last[2] / first[2]
            }
        };
        let skip = usize::from(prev.is_some());
        for &[u, v, w] in piece.ctrl.iter().skip(skip) {
            let w = w * scale;
            data.points
                .push(Point3::new(u * scale / w, v * scale / w, 0.0));
            data.weights.push(w);
        }
        let last = *piece.ctrl.last()?;
        prev = Some((
            piece.segment,
            [last[0] * scale, last[1] * scale, last[2] * scale],
        ));
        if let Some(m) = data.multiplicities.last_mut() {
            if data.knots.len() > 1 {
                *m = degree;
            }
        }
        data.knots.push(piece.t[1]);
        data.multiplicities.push(degree + 1);
    }

    // the curve's points at the piece ends: inside the domain
    let (u_ivl, v_ivl) = (uv_domain.u_interval(), uv_domain.v_interval());
    for p in data.points.iter_mut().step_by(degree) {
        for (c, ivl) in [(&mut p.x, u_ivl), (&mut p.y, v_ivl)] {
            if *c < ivl.min() - uv_tol || *c > ivl.max() + uv_tol {
                return None;
            }
            *c = c.clamp(ivl.min(), ivl.max());
        }
    }
    // an iso-parameter line is the drop's own
    let range = |f: fn(&Point3) -> f64| {
        data.points
            .iter()
            .map(f)
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| {
                (lo.min(x), hi.max(x))
            })
    };
    let (u_lo, u_hi) = range(|p| p.x);
    let (v_lo, v_hi) = range(|p| p.y);
    if u_hi - u_lo <= uv_tol || v_hi - v_lo <= uv_tol {
        return None;
    }
    // weights in NLib's range
    let w_max = data.weights.iter().copied().fold(0.0_f64, f64::max);
    if !w_max.is_finite() || w_max <= 0.0 {
        return None;
    }
    for w in &mut data.weights {
        *w /= w_max;
        if *w < WEIGHT_MIN || *w > WEIGHT_MAX {
            return None;
        }
    }
    Some(data)
}

/// The exact UV curve on the cylinder `surface` of `curve` over `interval` (see the
/// module notes), within `uv_domain`; `None` where it does not apply or does not
/// check out, and the drop traces.
///
/// # Errors
///
/// [`crate::error::SolidError::Unported`] from the evaluations only.
pub(crate) fn exact_uv_curve(
    k: &Kernel,
    surface: &Surface,
    uv_domain: &Extent2d,
    curve: &Curve,
    interval: &Extent1d,
) -> Result<Option<BSplineCurve>> {
    let SurfaceKind::Cone(cone) = surface.kind() else {
        return Ok(None);
    };
    let Some(section) = Section::of(cone) else {
        return Ok(None);
    };
    let Some(nurb) = curve_nurb(curve) else {
        return Ok(None);
    };
    let Some(pieces) = pieces(&section, nurb, interval) else {
        return Ok(None);
    };
    let uv_tol = EFF_ZERO_PARAM
        * (1.0
            + uv_domain
                .u_interval()
                .length()
                .min(uv_domain.v_interval().length()));
    let Some(data) = join(&pieces, uv_domain, uv_tol) else {
        return Ok(None);
    };
    let Some(uv_curve) = ignored(BSplineCurve::create_canonical(&CanonicalData {
        dim: 2,
        degree: data.degree,
        control_points: &data.points,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &data.multiplicities,
        knots: &data.knots,
        knot_type: KnotType::Unspecified,
        weights: Some(&data.weights),
        trim: None,
    }))?
    else {
        return Ok(None);
    };

    // the check: lifted onto the surface, the UV curve is on the curve
    let mut worst = 0.0_f64;
    let mut size = section.radius;
    for piece in &pieces {
        let [a, b] = piece.t;
        for i in 0..=CHECK_SAMPLES {
            let t = a + (b - a) * (i as f64 / CHECK_SAMPLES as f64);
            let Some(uv) = ignored(uv_curve.evaluate_point(t))? else {
                return Ok(None);
            };
            let uv = Point2::new(uv.x, uv.y);
            if !uv_domain.contains_point(uv, uv_tol) {
                return Ok(None);
            }
            let Some(on_surface) = ignored(surface.evaluate_point(k, uv))? else {
                return Ok(None);
            };
            let Some(on_curve) = ignored(curve.evaluate_point(k, t))? else {
                return Ok(None);
            };
            let gap = on_surface.distance(on_curve);
            if !gap.is_finite() {
                return Ok(None);
            }
            worst = worst.max(gap);
            size = size.max(on_curve.length() + section.radius);
        }
    }
    if worst > ON_SURFACE_REL * (1.0 + size) {
        return Ok(None);
    }
    Ok(Some(uv_curve))
}
