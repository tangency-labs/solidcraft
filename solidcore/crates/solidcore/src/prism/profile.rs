//! Planar profiles as the kernel holds them: closed loops of lines, circular arcs, conics and
//! Béziers in a frame's own coordinates, and regions (an outer loop less its holes). Plain data,
//! beside [`crate::eval::geometry`]; applications built on solidcore keep their own records of
//! these and copy them in at their boundary. The maths on them is here: a segment's points and
//! its homogeneous Bézier form, a loop's signed area with a proved bound on its rounding, the
//! even–odd hold of a point decided exactly or refused, the checks a profile must pass, the split
//! of a loop where it passes a point twice, and a segment made the kernel's curve on a frame.
//! For drawing sheets: a figure placed at its size, a regular outline's corners, and the tallest
//! label box inside a convex outline.
//!
//! Not upstream (SolidCraft's planar regions, moved into the kernel so the layers above compute
//! no geometry, docs/layers.md §1).
//!
//! References: Piegl & Tiller, *The NURBS Book* (2nd ed.), §1.4 (de Casteljau), §7.3 (the
//! rational quadratic arc, its middle weight the cosine of half its angle); Farin, *Curves and
//! Surfaces for CAGD* (5th ed.), §5.2 (Bernstein products), §12.4 (the area of a conic arc);
//! Higham, *Accuracy and Stability of Numerical Algorithms* (2nd ed.), §3.1 (`gamma_k`).
//!
//! Deviations: none (no upstream counterpart). No recursion.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests, kept as written

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use crate::curve::curve::Curve;
use crate::curve::ellipse::Ellipse;
use crate::curve::line::Line;
use crate::error::{Result, SolidError};
use crate::eval::Frame;
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::{Point2, Transform, Vec2};
use crate::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};

const OP_CURVE: &str = "prism::profile::curve_on_frame";
const OP_AREA: &str = "prism::profile::Loop2::signed_area";
const OP_FIGURE: &str = "prism::profile::place_figure_2d";
const OP_POLYGON: &str = "prism::profile::regular_polygon_2d";
const OP_FIT: &str = "prism::profile::box_fit_in_convex_polygon";

/// Highest degree of a [`Seg2::Bezier`].
pub const MAX_BEZIER_DEGREE: usize = 9;

/// Control points a [`Seg2::Bezier`] carries: its highest degree plus one (the tail unused).
pub const MAX_BEZIER_CTRL: usize = MAX_BEZIER_DEGREE + 1;

/// The most segments one call takes over all its loops.
pub const MAX_SEGMENTS: usize = 8192;

/// The largest coordinate a profile's point may have ([`check_regions`]), and so the largest a
/// sheet figure's corner or a fitted box's value may be: far inside `f64`, so sums and products
/// of two stay finite.
pub const MAX_PROFILE_COORD: f64 = 1e9;

/// Whether `p` is finite and within [`MAX_PROFILE_COORD`] on both axes.
fn within_profile(p: Point2) -> bool {
    finite2(p) && p.x.abs() < MAX_PROFILE_COORD && p.y.abs() < MAX_PROFILE_COORD
}

/// One boundary segment, in a frame's coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg2 {
    /// The line from `a` to `b`.
    Line {
        /// Its start.
        a: Point2,
        /// Its end.
        b: Point2,
    },
    /// The arc about `center` from angle `start` sweeping `sweep` radians (positive:
    /// counter-clockwise).
    Arc {
        /// Its centre.
        center: Point2,
        /// Its radius.
        radius: f64,
        /// Its start angle.
        start: f64,
        /// Its signed sweep.
        sweep: f64,
    },
    /// The cubic Bézier of `p0 .. p3`.
    Cubic {
        /// Its start.
        p0: Point2,
        /// The first inner control point.
        p1: Point2,
        /// The second inner control point.
        p2: Point2,
        /// Its end.
        p3: Point2,
    },
    /// The rational quadratic Bézier from `a` to `b` with control `apex` of weight `w` (end
    /// weights 1): an exact conic arc.
    Conic {
        /// Its start.
        a: Point2,
        /// The apex, where the end tangents meet.
        apex: Point2,
        /// Its end.
        b: Point2,
        /// The apex's weight.
        w: f64,
    },
    /// The Bézier of degree `n` (1 ..= [`MAX_BEZIER_DEGREE`]) over `p[0..=n]`.
    Bezier {
        /// Its degree.
        n: u8,
        /// Its control points (the tail past `n` unused).
        p: [Point2; MAX_BEZIER_CTRL],
    },
}

/// A closed loop of segments, each starting where the one before ends.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loop2 {
    /// The segments in order.
    pub segs: Vec<Seg2>,
}

/// A planar region: its outer loop (counter-clockwise) less its holes (clockwise).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Region2 {
    /// The outer loop.
    pub outer: Loop2,
    /// The holes.
    pub holes: Vec<Loop2>,
}

impl Region2 {
    /// The outer loop, then the holes.
    pub fn loops(&self) -> impl Iterator<Item = &Loop2> + Clone {
        std::iter::once(&self.outer).chain(&self.holes)
    }

    /// Its signed area, its loops' summed (holes running clockwise take theirs away), with a
    /// proved bound on its rounding.
    ///
    /// # Errors
    ///
    /// Those of [`Loop2::signed_area`].
    pub fn signed_area(&self) -> Result<Area> {
        let mut value = 0.0;
        let mut bound = 0.0;
        let mut size = 0.0_f64;
        for lp in self.loops() {
            let a = lp.signed_area()?;
            value += a.value;
            bound += a.bound;
            size += a.value.abs();
        }
        Ok(Area {
            value,
            bound: bound + gamma(self.holes.len() + 2) * size,
        })
    }
}

/// A homogeneous control point `[x w, y w, w]`.
pub type H2 = [f64; 3];

/// `gamma_k = k u / (1 - k u)` (Higham §3.1): the kernel's one copy.
pub(crate) use crate::nurbs::math::gamma;

pub(crate) fn cross2(a: Vec2, b: Vec2) -> f64 {
    a.x * b.y - a.y * b.x
}

fn unit(angle: f64) -> Vec2 {
    Vec2::new(angle.cos(), angle.sin())
}

fn finite2(p: Point2) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

fn h(p: Point2, w: f64) -> H2 {
    [p.x * w, p.y * w, w]
}

/// The point of a homogeneous control point.
pub(crate) fn point_of(p: H2) -> Point2 {
    Vec2::new(p[0] / p[2], p[1] / p[2])
}

/// The control points `p[0..=n]` of a Bézier segment, or `None` for a degree out of range.
fn bezier_points(n: u8, p: &[Point2; MAX_BEZIER_CTRL]) -> Option<&[Point2]> {
    let n = usize::from(n);
    if !(1..=MAX_BEZIER_DEGREE).contains(&n) {
        return None;
    }
    p.get(..=n)
}

impl Seg2 {
    /// Where it starts.
    #[must_use]
    pub fn start(&self) -> Point2 {
        match *self {
            Seg2::Line { a, .. } | Seg2::Conic { a, .. } => a,
            Seg2::Arc {
                center,
                radius,
                start,
                ..
            } => center + unit(start) * radius,
            Seg2::Cubic { p0, .. } => p0,
            Seg2::Bezier { p, .. } => p[0],
        }
    }

    /// Where it ends.
    #[must_use]
    pub fn end(&self) -> Point2 {
        match *self {
            Seg2::Line { b, .. } | Seg2::Conic { b, .. } => b,
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => center + unit(start + sweep) * radius,
            Seg2::Cubic { p3, .. } => p3,
            Seg2::Bezier { n, p } => p.get(usize::from(n)).copied().unwrap_or(Vec2::UNDEF),
        }
    }

    /// The same segment run the other way.
    #[must_use]
    pub fn reversed(&self) -> Seg2 {
        match *self {
            Seg2::Line { a, b } => Seg2::Line { a: b, b: a },
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => Seg2::Arc {
                center,
                radius,
                start: start + sweep,
                sweep: -sweep,
            },
            Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic {
                p0: p3,
                p1: p2,
                p2: p1,
                p3: p0,
            },
            Seg2::Conic { a, apex, b, w } => Seg2::Conic {
                a: b,
                apex,
                b: a,
                w,
            },
            Seg2::Bezier { n, mut p } => {
                if let Some(used) = p.get_mut(..=usize::from(n)) {
                    used.reverse();
                }
                Seg2::Bezier { n, p }
            }
        }
    }

    /// Whether it is a line or a circular arc.
    #[must_use]
    pub fn is_line_or_arc(&self) -> bool {
        matches!(self, Seg2::Line { .. } | Seg2::Arc { .. })
    }

    /// Its homogeneous Bézier spans, in the direction it runs: a line one span of degree 1, an
    /// arc rational quadratic spans of at most a quarter turn each (the middle weight the cosine
    /// of half the span's angle, Piegl & Tiller §7.3), the others their own control points.
    /// `None` for a malformed segment.
    #[must_use]
    pub fn spans(&self) -> Option<Vec<Vec<H2>>> {
        Some(match *self {
            Seg2::Line { a, b } => vec![vec![h(a, 1.0), h(b, 1.0)]],
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                if !(sweep.is_finite() && sweep.abs() <= 2.0 * TAU) {
                    return None;
                }
                // (at most eight quarter turns: the cast is of a small whole number)
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let n = (sweep.abs() / FRAC_PI_2).ceil().max(1.0) as usize;
                #[allow(clippy::cast_precision_loss)]
                let step = sweep / n as f64;
                let half = step * 0.5;
                let w = half.cos();
                (0..n)
                    .map(|i| {
                        #[allow(clippy::cast_precision_loss)]
                        let a = start + step * i as f64;
                        vec![
                            h(center + unit(a) * radius, 1.0),
                            h(center + unit(a + half) * (radius / w), w),
                            h(center + unit(a + step) * radius, 1.0),
                        ]
                    })
                    .collect()
            }
            Seg2::Cubic { p0, p1, p2, p3 } => {
                vec![vec![h(p0, 1.0), h(p1, 1.0), h(p2, 1.0), h(p3, 1.0)]]
            }
            Seg2::Conic { a, apex, b, w } => vec![vec![h(a, 1.0), h(apex, w), h(b, 1.0)]],
            Seg2::Bezier { n, ref p } => {
                vec![bezier_points(n, p)?.iter().map(|q| h(*q, 1.0)).collect()]
            }
        })
    }

    /// The point a fraction `t` of the way along its parameter (an arc's angle, a Bézier's own
    /// parameter): de Casteljau on the homogeneous points.
    #[must_use]
    pub fn point_at(&self, t: f64) -> Point2 {
        match *self {
            Seg2::Line { a, b } => a + (b - a) * t,
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => center + unit(start + sweep * t) * radius,
            _ => match self.spans().as_deref() {
                Some([span]) => point_of(casteljau(span, t)),
                _ => Vec2::UNDEF,
            },
        }
    }

    /// Its term of Green's integral, `1/2 ∫ x dy - y dx`, and a proved bound on the term's
    /// rounding: a loop's signed area is the sum of its segments' terms.
    #[must_use]
    pub fn area_term(&self) -> Option<(f64, f64)> {
        match *self {
            Seg2::Line { a, b } => {
                let (p, q) = (a.x * b.y, a.y * b.x);
                Some(((p - q) * 0.5, gamma(3) * (p.abs() + q.abs()) * 0.5))
            }
            Seg2::Arc {
                center: c,
                radius: r,
                start,
                sweep,
            } => {
                // P = c + r u(t), P x P' = r^2 + r (c.x cos t + c.y sin t)
                let end = start + sweep;
                let (s0, c0, s1, c1) = (start.sin(), start.cos(), end.sin(), end.cos());
                let terms = [r * r * sweep, r * c.x * (s1 - s0), -(r * c.y * (c1 - c0))];
                let size = r * r * sweep.abs() + 2.0 * r * (c.x.abs() + c.y.abs());
                // libm's sin and cos within two ulps; gamma covers the products and the sums
                let v = terms.iter().sum::<f64>() * 0.5;
                Some((v, gamma(16) * size * 0.5))
            }
            Seg2::Conic { a, apex, b, w } => {
                // Green of a conic arc = the chord's term + T f(w), T the signed area of the
                // control triangle and f(w) = 2 w ∫ t^2 / W^2 (the arc's segment, see
                // `conic_factor`)
                let (u, v) = (apex - a, b - a);
                let (p, q) = (a.x * b.y, a.y * b.x);
                let tri = cross2(u, v) * 0.5;
                let (f, fb) = conic_factor(w)?;
                let value = (p - q) * 0.5 + tri * f;
                let size = (p.abs() + q.abs()) * 0.5 + tri.abs() * f;
                let tri_err = gamma(6) * (u.x * v.y).abs().max((u.y * v.x).abs());
                Some((value, gamma(8) * size + tri.abs() * fb + tri_err * f))
            }
            Seg2::Cubic { .. } | Seg2::Bezier { .. } => {
                let span = self.spans()?.pop()?;
                let x: Vec<f64> = span.iter().map(|p| p[0]).collect();
                let y: Vec<f64> = span.iter().map(|p| p[1]).collect();
                let (xy, xb) = product(&x, &derivative(&y))?;
                let (yx, yb) = product(&y, &derivative(&x))?;
                #[allow(clippy::cast_precision_loss)]
                let m = xy.len() as f64;
                let value = (xy.iter().sum::<f64>() - yx.iter().sum::<f64>()) / m * 0.5;
                let size = xy.iter().chain(&yx).map(|c| c.abs()).sum::<f64>() / m * 0.5;
                #[allow(clippy::cast_precision_loss)]
                let k = 4 * x.len() + 16;
                Some((value, gamma(k) * size + (xb + yb) / m * 0.5))
            }
        }
    }
}

/// `f(w) = 2 w ∫_0^1 t^2 / W(t)^2 dt`, `W = 1 - 2 (1 - w) t (1 - t)`: the area between a
/// rational quadratic arc of middle weight `w` and its chord, over its control triangle's
/// (Farin §12.4), with a bound on its error. Near the parabola (`|1 - w| <= 1/2`) by its series
/// `1/W^2 = Σ (k + 1) (2 e τ)^k` (`e = 1 - w`, `τ = t (1 - t) <= 1/4`), whose terms integrate to
/// `B(k + 3, k + 1)`; the tail is bounded by a geometric series. Elsewhere in closed form: with
/// `w = cos φ` (an ellipse), `w (φ - w s) / s^3`, `s = sin φ`; with `w = cosh ψ` (a hyperbola),
/// `w (w s - ψ) / s^3`, `s = sinh ψ`.
fn conic_factor(w: f64) -> Option<(f64, f64)> {
    if !(w.is_finite() && w > 1e-9 && w < 1e9) {
        return None;
    }
    let e = 1.0 - w;
    if e.abs() <= 0.5 {
        // B(k + 3, k + 1) = (k + 2)! k! / (2k + 3)!, by its ratio from k to k + 1
        let mut beta = 1.0 / 3.0; // k = 0: 2! 0! / 3!
        let mut power = 1.0;
        let mut sum = 0.0;
        let mut size = 0.0;
        let mut term = 0.0;
        for k in 0..60u32 {
            let kf = f64::from(k);
            term = (kf + 1.0) * power * beta;
            sum += term;
            size += term.abs();
            power *= 2.0 * e;
            beta *= (kf + 3.0) * (kf + 1.0) / ((2.0 * kf + 4.0) * (2.0 * kf + 5.0));
        }
        // the ratio of successive terms is below |2 e| / 4 (k + 2)/(k + 1) <= 1/2 past k = 0
        let tail = term.abs() * 2.0;
        let f = 2.0 * w * sum;
        return Some((f, 2.0 * w * (tail + gamma(512) * size)));
    }
    let (f, terms) = if w < 1.0 {
        let phi = w.acos();
        let s = phi.sin();
        (
            w * (phi - w * s) / (s * s * s),
            w * (phi + w * s) / (s * s * s),
        )
    } else {
        let psi = w.acosh();
        let s = psi.sinh();
        (
            w * (w * s - psi) / (s * s * s),
            w * (w * s + psi) / (s * s * s),
        )
    };
    // |1 - w| > 1/2: the difference cancels at most a factor ~10; the functions within 4 ulps
    (f.is_finite()).then_some((f, gamma(64) * terms))
}

/// The Bernstein coefficients of the product of two Bernstein polynomials (Farin §5.2), and a
/// bound on their rounding.
pub(crate) fn product(a: &[f64], b: &[f64]) -> Option<(Vec<f64>, f64)> {
    let m = a.len().checked_sub(1)?;
    let n = b.len().checked_sub(1)?;
    let (cm, cn, ct) = (choose(m), choose(n), choose(m + n));
    let mut out = vec![0.0; m + n + 1];
    let mut size = 0.0_f64;
    for (i, ai) in a.iter().enumerate() {
        for (j, bj) in b.iter().enumerate() {
            let f = cm.get(i)? * cn.get(j)? / ct.get(i + j)?;
            let t = ai * bj * f;
            *out.get_mut(i + j)? += t;
            size = size.max(t.abs());
        }
    }
    let count = m + n + 2;
    out.iter()
        .all(|x| x.is_finite())
        .then_some((out, gamma(4 * count + 8) * size * (count as f64)))
}

/// `C(n, i)` for every `i`.
fn choose(n: usize) -> Vec<f64> {
    let mut v = vec![1.0; n + 1];
    for i in 1..=n {
        #[allow(clippy::cast_precision_loss)]
        let f = (n + 1 - i) as f64 / i as f64;
        let prev = v.get(i - 1).copied().unwrap_or(1.0);
        if let Some(x) = v.get_mut(i) {
            *x = prev * f;
        }
    }
    v
}

/// The Bernstein coefficients of a Bernstein polynomial's derivative.
pub(crate) fn derivative(a: &[f64]) -> Vec<f64> {
    #[allow(clippy::cast_precision_loss)]
    let n = a.len().saturating_sub(1) as f64;
    a.iter()
        .zip(a.iter().skip(1))
        .map(|(x, y)| (y - x) * n)
        .collect()
}

/// De Casteljau at `t` on homogeneous points.
pub(crate) fn casteljau(span: &[H2], t: f64) -> H2 {
    let mut q = span.to_vec();
    while q.len() > 1 {
        q = q
            .iter()
            .zip(q.iter().skip(1))
            .map(|(&[ax, ay, aw], &[bx, by, bw])| {
                [ax + (bx - ax) * t, ay + (by - ay) * t, aw + (bw - aw) * t]
            })
            .collect();
    }
    q.first().copied().unwrap_or([f64::NAN; 3])
}

/// A span split in half (de Casteljau at 1/2): the left and the right halves.
pub(crate) fn halves(c: &[H2]) -> (Vec<H2>, Vec<H2>) {
    let mut q = c.to_vec();
    let mut left: Vec<H2> = q.first().copied().into_iter().collect();
    let mut right: Vec<H2> = q.last().copied().into_iter().collect();
    while q.len() > 1 {
        q = q
            .iter()
            .zip(q.iter().skip(1))
            .map(|(&[ax, ay, aw], &[bx, by, bw])| {
                [(ax + bx) * 0.5, (ay + by) * 0.5, (aw + bw) * 0.5]
            })
            .collect();
        left.extend(q.first().copied());
        right.extend(q.last().copied());
    }
    right.reverse();
    (left, right)
}

/// A signed area with a proved bound on its error: the exact area is in
/// `[value - bound, value + bound]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    /// The area as computed.
    pub value: f64,
    /// The bound on its error.
    pub bound: f64,
}

impl Area {
    /// Its sign when the bound decides it: `Some(true)` positive, `Some(false)` negative.
    #[must_use]
    pub fn sign(&self) -> Option<bool> {
        if self.value > self.bound {
            Some(true)
        } else if self.value < -self.bound {
            Some(false)
        } else {
            None
        }
    }
}

impl Loop2 {
    /// The loop run the other way.
    #[must_use]
    pub fn reversed(&self) -> Loop2 {
        Loop2 {
            segs: self.segs.iter().rev().map(Seg2::reversed).collect(),
        }
    }

    /// Its signed area (positive counter-clockwise), the sum of its segments' Green terms, with a
    /// proved bound on its rounding.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a malformed segment.
    pub fn signed_area(&self) -> Result<Area> {
        let mut value = 0.0;
        let mut bound = 0.0;
        let mut size = 0.0_f64;
        for s in &self.segs {
            let (v, b) = s
                .area_term()
                .ok_or_else(|| SolidError::invalid_input(OP_AREA, "a malformed segment"))?;
            value += v;
            bound += b;
            size += v.abs();
        }
        bound += gamma(self.segs.len() + 1) * size;
        if !(value.is_finite() && bound.is_finite()) {
            return Err(SolidError::invalid_input(
                OP_AREA,
                "an area that is not finite",
            ));
        }
        Ok(Area { value, bound })
    }

    /// Whether each segment ends within `tol` of where the next starts, round the loop.
    #[must_use]
    pub fn is_closed(&self, tol: f64) -> bool {
        !self.segs.is_empty()
            && self
                .segs
                .iter()
                .zip(self.segs.iter().cycle().skip(1))
                .all(|(a, b)| a.end().distance(b.start()) <= tol)
    }

    /// Whether the loop holds `p` by the even–odd rule, decided exactly with the half-open
    /// crossing rule: a point is above the ray from `p` along +x when its `y > p.y`, and below
    /// it otherwise, so a stretch of the loop lying along the ray's line counts as below it and
    /// crosses nothing. Each segment's Bézier spans, and a straight join wherever a span ends
    /// away from where the next starts, are split until each is proved above, below, left of
    /// `p`, or right of it; a span right of `p` crosses the ray an odd number of times exactly
    /// when its ends lie on opposite sides (it is connected and stays right of `p`). `None` when
    /// a span cannot be decided (`p` on or within rounding of the loop).
    #[must_use]
    pub fn holds(&self, p: Point2) -> Option<bool> {
        let mut spans: Vec<Vec<H2>> = Vec::new();
        for s in &self.segs {
            spans.extend(s.spans()?);
        }
        let ends = |c: &[H2]| Some((point_of(*c.first()?), point_of(*c.last()?)));
        let mut odd = false;
        for (here, next) in spans.iter().zip(spans.iter().cycle().skip(1)) {
            odd ^= crossings_odd(here.clone(), p)?;
            let (_, end) = ends(here)?;
            let (start, _) = ends(next)?;
            if end != start {
                odd ^= crossings_odd(vec![[end.x, end.y, 1.0], [start.x, start.y, 1.0]], p)?;
            }
        }
        Some(odd)
    }
}

/// Whether the ray from `p` along +x crosses `span` an odd number of times by the half-open
/// rule ([`Loop2::holds`]): a point is above the ray when its `y > p.y`.
fn crossings_odd(span: Vec<H2>, p: Point2) -> Option<bool> {
    let mut odd = false;
    let mut todo = vec![(span, 0u32)];
    while let Some((c, depth)) = todo.pop() {
        if c.iter()
            .any(|q| !(q[2] > 0.0 && q.iter().all(|x| x.is_finite())))
        {
            return None;
        }
        let pts: Vec<Point2> = c.iter().map(|q| point_of(*q)).collect();
        let (first, last) = (*pts.first()?, *pts.last()?);
        let scale = pts.iter().fold(p.x.abs().max(p.y.abs()), |m, q| {
            m.max(q.x.abs()).max(q.y.abs())
        });
        // the rounding of the control points and of the hull they bound the span by
        let margin = gamma(32) * scale;
        // proved off the ray's line, or wholly left of p: no crossing on the ray
        if pts.iter().all(|q| q.y > p.y + margin)
            || pts.iter().all(|q| q.y < p.y - margin)
            || pts.iter().all(|q| q.x < p.x - margin)
        {
            continue;
        }
        // wholly right of p: the span is connected and never meets the line x = p.x, so its
        // crossings of the ray are its passages between above and below, odd exactly when its
        // ends are on opposite sides. The ends are the points the neighbouring spans and joins
        // share, so a level shared end is classified once and the count telescopes round the loop.
        if pts.iter().all(|q| q.x > p.x + margin) {
            odd ^= (first.y > p.y) != (last.y > p.y);
            continue;
        }
        if depth >= 48 {
            return None;
        }
        let (l, r) = halves(&c);
        todo.push((l, depth + 1));
        todo.push((r, depth + 1));
    }
    Some(odd)
}

/// Why a profile is refused ([`check_regions`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileRefusal {
    /// A loop without segments, not closed within its distance, or more segments than
    /// [`MAX_SEGMENTS`] in all.
    Budget,
    /// A line or arc not finite or of no length, an arc of no positive radius or turning more
    /// than once.
    LineOrArc,
    /// A free-form segment not finite or of no length, or a conic weight not positive.
    FreeForm,
}

/// The loops of `regions` checked: each closed within ten times `tol`, the segments within
/// [`MAX_SEGMENTS`], each segment finite and of some length (a line's ends apart, an arc of a
/// positive radius turning at most once, a conic's weight positive, a Bézier's ends apart), its
/// coordinates within [`MAX_PROFILE_COORD`].
///
/// # Errors
///
/// The [`ProfileRefusal`] that applies first.
pub fn check_regions<'a>(
    regions: impl Iterator<Item = &'a Region2>,
    tol: f64,
) -> std::result::Result<(), ProfileRefusal> {
    let mut count = 0usize;
    let finite = within_profile;
    let apart = |a: Point2, b: Point2| a.distance(b) > 1e-12;
    for lp in regions.flat_map(Region2::loops) {
        count = count.saturating_add(lp.segs.len());
        if count > MAX_SEGMENTS || lp.segs.is_empty() {
            return Err(ProfileRefusal::Budget);
        }
        for s in &lp.segs {
            let ok = match *s {
                Seg2::Line { a, b } => finite(a) && finite(b) && apart(a, b),
                Seg2::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                } => {
                    finite(center)
                        && radius.is_finite()
                        && radius > 0.0
                        && radius < 1e9
                        && start.is_finite()
                        && sweep.is_finite()
                        && sweep.abs() > 1e-12
                        && sweep.abs() <= TAU + 1e-9
                }
                Seg2::Cubic { p0, p1, p2, p3 } => {
                    [p0, p1, p2, p3].into_iter().all(finite) && apart(p0, p3)
                }
                Seg2::Conic { a, apex, b, w } => {
                    [a, apex, b].into_iter().all(finite)
                        && w.is_finite()
                        && w > 1e-9
                        && w < 1e9
                        && apart(a, b)
                }
                Seg2::Bezier { n, ref p } => bezier_points(n, p)
                    .is_some_and(|q| q.iter().copied().all(finite) && apart(s.start(), s.end())),
            };
            if !ok {
                return Err(if s.is_line_or_arc() {
                    ProfileRefusal::LineOrArc
                } else {
                    ProfileRefusal::FreeForm
                });
            }
        }
        if !lp.is_closed(tol * 10.0) {
            return Err(ProfileRefusal::Budget);
        }
    }
    Ok(())
}

/// A closed loop, each segment with a key for where it starts, split where it passes a point
/// twice (`same` keys for two of its segments) into loops that pass every point once. Each split
/// leaves two shorter loops, so the work is bounded by the segment count.
pub fn simple_loops<K: Clone>(segs: Vec<(K, Seg2)>, same: impl Fn(&K, &K) -> bool) -> Vec<Loop2> {
    let mut todo = vec![segs];
    let mut out = Vec::new();
    while let Some(segs) = todo.pop() {
        let repeat = segs.iter().enumerate().find_map(|(i, a)| {
            segs.iter()
                .enumerate()
                .skip(i + 1)
                .find(|(_, b)| same(&a.0, &b.0))
                .map(|(j, _)| (i, j))
        });
        match repeat {
            Some((i, j)) => {
                // segments i..j close a loop of their own; the rest, from j round to i, another
                let inner: Vec<(K, Seg2)> = segs.get(i..j).map(<[_]>::to_vec).unwrap_or_default();
                let rest: Vec<(K, Seg2)> = segs
                    .get(j..)
                    .into_iter()
                    .flatten()
                    .chain(segs.get(..i).into_iter().flatten())
                    .cloned()
                    .collect();
                todo.push(inner);
                todo.push(rest);
            }
            None => out.push(Loop2 {
                segs: segs.into_iter().map(|(_, s)| s).collect(),
            }),
        }
    }
    out
}

fn v3(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

/// A frame's origin, x and y axes and normal (`x × y`).
pub(crate) fn axes(frame: &Frame) -> (Point3, Vec3, Vec3, Vec3) {
    let (x, y) = (v3(frame.x_axis), v3(frame.y_axis));
    (v3(frame.origin), x, y, x.cross(y))
}

/// The point `(p, level)` of `frame` in the world: `origin + p.x x + p.y y + level n`.
#[must_use]
pub fn to_world(frame: &Frame, p: Point2, level: f64) -> Point3 {
    let (o, x, y, n) = axes(frame);
    o + x * p.x + y * p.y + n * level
}

/// `q` in `frame`'s coordinates: `((q - o).x, (q - o).y)`.
#[must_use]
pub fn to_local(frame: &Frame, q: Point3) -> Point2 {
    let (o, x, y, _) = axes(frame);
    let d = q - o;
    Vec2::new(d.dot(x), d.dot(y))
}

/// `q`'s height above `frame`: `(q - o).n`.
#[must_use]
pub fn height(frame: &Frame, q: Point3) -> f64 {
    let (o, _, _, n) = axes(frame);
    (q - o).dot(n)
}

/// Whether `frame`'s axes are unit and square to each other, to within 1e-12.
#[must_use]
pub fn frame_is_orthonormal(frame: &Frame) -> bool {
    let (o, x, y, _) = axes(frame);
    [o, x, y]
        .iter()
        .all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite())
        && (x.length() - 1.0).abs() <= 1e-12
        && (y.length() - 1.0).abs() <= 1e-12
        && x.dot(y).abs() <= 1e-12
}

/// `seg` as the kernel's curve on `frame` at `level`, in the direction it runs: a line segment,
/// a circular arc (the kernel's circle on the frame over the arc's angles; its net is proved on the arc where it is read back, [`super::read::planar_pieces`]; a clockwise arc
/// on the frame mirrored, so it runs as the segment does), a conic or Bézier as its exact NURBS.
///
/// # Errors
///
/// `InvalidInput` for a frame whose axes are not orthonormal, or a malformed segment; those of
/// the constructors.
pub fn curve_on_frame(k: &Kernel, frame: &Frame, level: f64, seg: &Seg2) -> Result<Curve> {
    if !frame_is_orthonormal(frame) || !level.is_finite() {
        return Err(SolidError::invalid_input(
            OP_CURVE,
            "a frame that is not orthonormal",
        ));
    }
    let at = |p: Point2| to_world(frame, p, level);
    match *seg {
        Seg2::Line { a, b } => Ok(Curve::from(Line::create_line_segment(
            3,
            at(a),
            at(b),
            None,
        )?)),
        Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            let (_, x, y, _) = axes(frame);
            let (y, s0) = if sweep >= 0.0 {
                (y, start)
            } else {
                (-y, -start)
            };
            let placement = Axis2Placement::new(at(center), x, y);
            let deg = Extent1d::from_min_max(s0.to_degrees(), (s0 + sweep.abs()).to_degrees());
            let mut e = Ellipse::create_canonical_circle(k, &placement, radius, Some(&deg), None)?;
            e.edit_parameterization(k, &deg, true, None)?;
            Ok(Curve::from(e))
        }
        _ => {
            let span = seg
                .spans()
                .and_then(|mut s| s.pop())
                .ok_or_else(|| SolidError::invalid_input(OP_CURVE, "a malformed segment"))?;
            let degree = span
                .len()
                .checked_sub(1)
                .ok_or_else(|| SolidError::invalid_input(OP_CURVE, "a malformed segment"))?;
            let mut knots = vec![0.0; degree + 1];
            knots.extend(std::iter::repeat_n(1.0, degree + 1));
            let homogeneous: Vec<[f64; 4]> = span
                .iter()
                .map(|q| {
                    let p = at(point_of(*q));
                    [p.x * q[2], p.y * q[2], p.z * q[2], q[2]]
                })
                .collect();
            rational_curve(&knots, &homogeneous)
        }
    }
}

/// A segment placed on a frame: its curve as a description, in world coordinates, for a caller
/// that keeps its own curve record ([`seg_on_frame`]).
#[derive(Clone, Debug, PartialEq)]
pub enum PlacedSeg {
    /// The line from `a` to `b`.
    Line {
        /// Its start.
        a: Point3,
        /// Its end.
        b: Point3,
    },
    /// The circular arc about `center` in the plane square to the unit `axis`, of `radius`,
    /// from angle `start` to `end` (radians, `start < end`) measured from the unit `reference`
    /// towards `axis × reference`: counter-clockwise about `axis`.
    Arc {
        /// Its centre.
        center: Point3,
        /// The normal it turns counter-clockwise about.
        axis: Vec3,
        /// Where its angles are measured from.
        reference: Vec3,
        /// Its radius.
        radius: f64,
        /// Its start angle.
        start: f64,
        /// Its end angle.
        end: f64,
    },
    /// The Bézier of `points` (`[x, y, z, weight]`, weights positive) on the clamped knots
    /// `knots`, of degree `degree`.
    Nurbs {
        /// Its degree.
        degree: usize,
        /// Its control points with their weights.
        points: Vec<[f64; 4]>,
        /// Its clamped knot vector, `0` and `1` each `degree + 1` times.
        knots: Vec<f64>,
    },
}

/// `seg` placed on `frame` at `level` as a [`PlacedSeg`], in the direction it runs: each point
/// by [`to_world`]; an arc about the frame's normal `x × y`, its angles from `x`, or, swept
/// clockwise, about the reversed normal with its angles negated so it still runs from its
/// start (the same arc [`curve_on_frame`] builds); a cubic, conic or Bézier with its control
/// points and weights. `None` for an arc of no sweep, a conic whose weight is not finite and
/// positive, or a Bézier whose degree is outside `1 ..=` [`MAX_BEZIER_DEGREE`].
#[must_use]
pub fn seg_on_frame(frame: &Frame, level: f64, seg: &Seg2) -> Option<PlacedSeg> {
    let at = |p: Point2| to_world(frame, p, level);
    let nurbs = |degree: usize, points: Vec<[f64; 4]>| {
        let mut knots = vec![0.0; degree + 1];
        knots.extend(std::iter::repeat_n(1.0, degree + 1));
        PlacedSeg::Nurbs {
            degree,
            points,
            knots,
        }
    };
    let weighted = |p: Point2, w: f64| {
        let q = at(p);
        [q.x, q.y, q.z, w]
    };
    match *seg {
        Seg2::Line { a, b } => Some(PlacedSeg::Line { a: at(a), b: at(b) }),
        Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            if sweep == 0.0 {
                return None;
            }
            let (_, x, _, normal) = axes(frame);
            let (axis, start, end) = if sweep < 0.0 {
                (-normal, -start, -start - sweep)
            } else {
                (normal, start, start + sweep)
            };
            Some(PlacedSeg::Arc {
                center: at(center),
                axis,
                reference: x,
                radius,
                start,
                end,
            })
        }
        Seg2::Cubic { p0, p1, p2, p3 } => Some(nurbs(
            3,
            [p0, p1, p2, p3].iter().map(|&p| weighted(p, 1.0)).collect(),
        )),
        Seg2::Conic { a, apex, b, w } => (w.is_finite() && w > 0.0).then(|| {
            nurbs(
                2,
                vec![weighted(a, 1.0), weighted(apex, w), weighted(b, 1.0)],
            )
        }),
        Seg2::Bezier { n, p } => {
            let degree = usize::from(n);
            (1..=MAX_BEZIER_DEGREE).contains(&degree).then(|| {
                nurbs(
                    degree,
                    p.iter()
                        .take(degree + 1)
                        .map(|&q| weighted(q, 1.0))
                        .collect(),
                )
            })
        }
    }
}

/// The B-spline curve of the knot vector `knots` (with its repeats) and the homogeneous control
/// points `[x w, y w, z w, w]` (all weights one: polynomial), by the kernel's canonical
/// constructor.
///
/// # Errors
///
/// `InvalidInput` for knots that do not fit the points or do not increase, a weight not
/// positive or numbers not finite; those of the constructor (a knot repeated past the degree).
pub fn rational_curve(knots: &[f64], homogeneous: &[[f64; 4]]) -> Result<Curve> {
    let bad = |reason: &'static str| SolidError::invalid_input(OP_CURVE, reason);
    let degree = knots
        .len()
        .checked_sub(
            homogeneous
                .len()
                .checked_add(1)
                .ok_or_else(|| bad("too many points"))?,
        )
        .ok_or_else(|| bad("the knots do not fit the points"))?;
    let mut unique: Vec<f64> = Vec::new();
    let mut mult: Vec<usize> = Vec::new();
    for &u in knots {
        if !u.is_finite() {
            return Err(bad("a knot is not finite"));
        }
        match (unique.last(), mult.last_mut()) {
            (Some(&prev), Some(m)) if u == prev => *m += 1,
            (Some(&prev), _) if u < prev => return Err(bad("the knots decrease")),
            _ => {
                unique.push(u);
                mult.push(1);
            }
        }
    }
    // a knot repeated past the degree inside the curve is a jump: refused, never repaired (the
    // canonical constructor would remove it and move the points)
    if mult.iter().skip(1).rev().skip(1).any(|m| *m > degree) {
        return Err(bad("an interior knot repeated past the degree"));
    }
    let mut points = Vec::new();
    let mut weights = Vec::new();
    for h in homogeneous {
        let [x, y, z, w] = *h;
        if !(w > 0.0 && w.is_finite() && x.is_finite() && y.is_finite() && z.is_finite()) {
            return Err(bad(
                "a control point is not finite or its weight not positive",
            ));
        }
        points.push(Point3::new(x / w, y / w, z / w));
        weights.push(w);
    }
    let rational = weights.iter().any(|w| *w != 1.0);
    let data = CanonicalData {
        dim: 3,
        degree,
        control_points: &points,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &mult,
        knots: &unique,
        knot_type: KnotType::Unspecified,
        weights: rational.then_some(weights.as_slice()),
        trim: None,
    };
    Ok(Curve::from(BSplineCurve::create_canonical(&data)?))
}

// Figures on a drawing sheet: a figure placed at its size, a regular outline's corners, and the
// tallest label box inside a convex outline.

/// Where a figure goes on its sheet ([`place_figure_2d`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FigurePlacement {
    /// Its size: the figure's units are `scale` long on the sheet.
    pub scale: f64,
    /// Its turn about its own origin, counter-clockwise, in radians.
    pub angle: f64,
    /// Where the figure's own origin goes.
    pub origin: Point2,
}

/// A figure placed on its sheet ([`place_figure_2d`]).
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedFigure {
    /// The figure's segments, in its order.
    pub segments: Vec<Seg2>,
    /// The points asked for (a text's anchor, say), in their order.
    pub points: Vec<Point2>,
    /// The unit direction the figure's `+x` now points along (a text's baseline).
    pub direction: Vec2,
}

/// `figure` and `points` scaled by `placement.scale`, turned by `placement.angle` about their own
/// origin and moved to `placement.origin`: each point `p` goes to `origin + scale R p`, where `R`
/// is the turn (counter-clockwise about `+z`). An arc's centre moves, its radius scales, its start
/// turns by the angle and its sweep stays; a conic keeps its weight. A drawing symbol is a table
/// of segments and text anchors in units of its size, placed by this one call.
///
/// A mirror is out of scope: a negative scale would also turn each arc's sweep, so it is refused.
///
/// # Errors
///
/// `InvalidInput` for a scale not above 0 or not finite, an angle or an origin not finite, more
/// than [`MAX_SEGMENTS`] segments or points, a Bézier degree outside `1 ..=`
/// [`MAX_BEZIER_DEGREE`], or a segment or point not finite before or after it is placed.
pub fn place_figure_2d(
    figure: &[Seg2],
    points: &[Point2],
    placement: FigurePlacement,
) -> Result<PlacedFigure> {
    let bad = |reason| SolidError::invalid_input(OP_FIGURE, reason);
    let FigurePlacement {
        scale,
        angle,
        origin,
    } = placement;
    if !(scale > 0.0 && scale.is_finite() && angle.is_finite() && finite2(origin)) {
        return Err(bad(
            "a scale not above 0 or not finite, or an angle or an origin not finite",
        ));
    }
    if figure.len() > MAX_SEGMENTS || points.len() > MAX_SEGMENTS {
        return Err(bad("more segments or points than MAX_SEGMENTS"));
    }
    // The turned axes are `x` and `x` turned a quarter more: one rounding per entry, so no turn is
    // exact. The kernel's affine map then scales them and adds the move.
    let x = unit(angle);
    let map = Transform::from_axes(
        Vec3::new(x.x, x.y, 0.0) * scale,
        Vec3::new(-x.y, x.x, 0.0) * scale,
        Vec3::Z,
        Point3::new(origin.x, origin.y, 0.0),
    );
    let at = |p: Point2| Vec2::from_xy(map.transform_point(Point3::new(p.x, p.y, 0.0)));
    let mut placed = Vec::with_capacity(figure.len());
    for seg in figure {
        let moved = match *seg {
            Seg2::Line { a, b } => Seg2::Line { a: at(a), b: at(b) },
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => Seg2::Arc {
                center: at(center),
                radius: radius * scale,
                start: start + angle,
                sweep,
            },
            Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic {
                p0: at(p0),
                p1: at(p1),
                p2: at(p2),
                p3: at(p3),
            },
            Seg2::Conic { a, apex, b, w } => Seg2::Conic {
                a: at(a),
                apex: at(apex),
                b: at(b),
                w,
            },
            Seg2::Bezier { n, mut p } => {
                let used = bezier_points(n, &p)
                    .ok_or_else(|| bad("a Bézier degree out of range"))?
                    .len();
                for q in p.iter_mut().take(used) {
                    *q = at(*q);
                }
                Seg2::Bezier { n, p }
            }
        };
        if !seg_finite(&moved) {
            return Err(bad("a segment not finite"));
        }
        placed.push(moved);
    }
    let points = points.iter().map(|&p| at(p)).collect::<Vec<_>>();
    if !points.iter().copied().all(finite2) {
        return Err(bad("a point not finite"));
    }
    Ok(PlacedFigure {
        segments: placed,
        points,
        direction: x,
    })
}

/// Every number `seg` uses is finite (a Bézier's tail past its degree is not used).
fn seg_finite(seg: &Seg2) -> bool {
    match *seg {
        Seg2::Line { a, b } => finite2(a) && finite2(b),
        Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } => finite2(center) && radius.is_finite() && start.is_finite() && sweep.is_finite(),
        Seg2::Cubic { p0, p1, p2, p3 } => [p0, p1, p2, p3].into_iter().all(finite2),
        Seg2::Conic { a, apex, b, w } => finite2(a) && finite2(apex) && finite2(b) && w.is_finite(),
        Seg2::Bezier { n, p } => {
            bezier_points(n, &p).is_some_and(|q| q.iter().copied().all(finite2))
        }
    }
}

/// The corners of the regular polygon with `sides` sides, each `side` long, about `centre`:
/// counter-clockwise, the first at the angle `first_angle` from `+x` (a triangle point up has
/// `first_angle = π/2`). The circumradius is `side / (2 sin(π / sides))`, and the centroid is
/// `centre`.
///
/// # Errors
///
/// `InvalidInput` for fewer than 3 sides or more than [`MAX_SEGMENTS`], a side not above 0, a
/// centre, side or angle not finite, or a corner that comes out not finite or beyond
/// [`MAX_PROFILE_COORD`] (the coordinates [`check_regions`] allows).
pub fn regular_polygon_2d(
    centre: Point2,
    sides: usize,
    side: f64,
    first_angle: f64,
) -> Result<Vec<Point2>> {
    let bad = |reason| SolidError::invalid_input(OP_POLYGON, reason);
    if !(3..=MAX_SEGMENTS).contains(&sides) {
        return Err(bad("fewer than 3 sides or more than MAX_SEGMENTS"));
    }
    if !(side > 0.0 && side.is_finite() && finite2(centre) && first_angle.is_finite()) {
        return Err(bad("a side not above 0, or a value not finite"));
    }
    let n = sides as f64;
    let radius = side / (2.0 * (PI / n).sin());
    let corners: Vec<Point2> = (0..sides)
        .map(|i| centre + unit(first_angle + TAU * i as f64 / n) * radius)
        .collect();
    if corners.iter().copied().all(within_profile) {
        Ok(corners)
    } else {
        Err(bad("a corner not finite or beyond MAX_PROFILE_COORD"))
    }
}

/// A box fitted inside a convex outline ([`box_fit_in_convex_polygon`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxFit {
    /// Its height (its width is the height times the aspect asked for).
    pub height: f64,
    /// Its lower-left corner.
    pub min: Point2,
}

/// The tallest box `width_per_height` times as wide as it is tall, centred across on the line
/// `x = centre_x`, kept `margin` clear of every edge of `polygon` (measured square to the edge)
/// and no taller than `max_height`; of the boxes of that height, the one with the lowest bottom.
/// `None` when no box fits, not even one of no height.
///
/// `polygon` is convex with its corners counter-clockwise. A box of height `h` with its bottom at
/// `y0` has its corners at `(centre_x ± width_per_height h / 2, y0)` and `(.., y0 + h)`. An edge
/// from `P` with inward unit normal `n` holds a corner `c` when `n · (c - P) ≥ margin`, and the
/// corner nearest the edge gives one condition, linear in `h` and `y0`:
/// `A h + B y0 ≥ C`, with `A = min(n.y, 0) - |n.x| width_per_height / 2`, `B = n.y` and
/// `C = margin + n · P - n.x centre_x`. As `A ≤ 0`, a box that fits still fits made shorter on the
/// same bottom, so the heights that fit run from 0 to the answer. Upright edges (`B = 0`) bound
/// the height alone; the others bound `y0` from below (`B > 0`) or above (`B < 0`) by lines in
/// `h`. From the tallest height allowed, each step goes down to where the highest lower line
/// meets the lowest upper line (the gap between the bounds is concave in `h`, so each step lands
/// on the next piece of it), until the lowest bottom meets every condition. The conditions are
/// distances, so they are met within a rounding allowance of `gamma(16)` times the problem's size
/// (Higham §3.1).
///
/// # Errors
///
/// `InvalidInput` for fewer than 3 corners or more than [`MAX_SEGMENTS`], an edge of no length,
/// an outline not convex and counter-clockwise, a negative aspect, margin or height, a value not
/// finite or beyond [`MAX_PROFILE_COORD`] (the coordinates [`check_regions`] allows), or a bound
/// that overflows (an edge too near upright to compare). `SM_ERR` if the steps do not settle
/// within one per edge (rounding only).
pub fn box_fit_in_convex_polygon(
    polygon: &[Point2],
    centre_x: f64,
    width_per_height: f64,
    margin: f64,
    max_height: f64,
) -> Result<Option<BoxFit>> {
    let bad = |reason| SolidError::invalid_input(OP_FIT, reason);
    if !(3..=MAX_SEGMENTS).contains(&polygon.len()) {
        return Err(bad("fewer than 3 corners or more than MAX_SEGMENTS"));
    }
    // Within the coordinates a profile may have (`check_regions`), so no product overflows.
    let fine = |v: f64| v.abs() < MAX_PROFILE_COORD;
    let values = [centre_x, width_per_height, margin, max_height];
    if !polygon.iter().all(|p| fine(p.x) && fine(p.y)) || !values.into_iter().all(fine) {
        return Err(bad("a value not finite or beyond MAX_PROFILE_COORD"));
    }
    if width_per_height < 0.0 || margin < 0.0 || max_height < 0.0 {
        return Err(bad("a negative aspect, margin or height"));
    }
    // Each edge's start and unit direction.
    let mut edges = Vec::with_capacity(polygon.len());
    for (p, q) in polygon.iter().zip(polygon.iter().cycle().skip(1)) {
        let d = (*q - *p)
            .unitize()
            .map_err(|_| bad("an edge of no length"))?;
        edges.push((*p, d));
    }
    // Convex and counter-clockwise: no turn to the right, and one turn in all.
    let mut turned = 0.0;
    for ((_, d), (_, e)) in edges.iter().zip(edges.iter().cycle().skip(1)) {
        let c = cross2(*d, *e);
        if c < 0.0 {
            return Err(bad("an outline not convex and counter-clockwise"));
        }
        turned += c.atan2(d.dot(*e));
    }
    if (turned - TAU).abs() > PI {
        return Err(bad("an outline not convex and counter-clockwise"));
    }
    let half = width_per_height / 2.0;
    let holds: Vec<Hold> = edges
        .iter()
        .map(|(p, d)| {
            let n = Vec2::new(-d.y, d.x);
            Hold {
                a: n.y.min(0.0) - n.x.abs() * half,
                b: n.y,
                c: margin + n.dot(*p) - n.x * centre_x,
            }
        })
        .collect();
    let reach = polygon
        .iter()
        .fold(0.0_f64, |m, p| m.max(p.x.abs()).max(p.y.abs()));
    let size = reach + centre_x.abs() + margin + max_height * (1.0 + width_per_height);
    let allowance = gamma(16) * size;
    // Upright edges bound the height alone; one the centre line is too near fits nothing.
    let mut h = max_height;
    for hold in holds.iter().filter(|e| e.b == 0.0) {
        if hold.c > allowance {
            return Ok(None);
        }
        if hold.a < 0.0 {
            h = h.min((hold.c / hold.a).max(0.0));
        }
    }
    let lowest = |h: f64| {
        let below = holds.iter().filter(|e| e.b > 0.0);
        below
            .map(|e| (e.bottom(h), *e))
            .max_by(|x, y| x.0.total_cmp(&y.0))
    };
    let highest = |h: f64| {
        let above = holds.iter().filter(|e| e.b < 0.0);
        above
            .map(|e| (e.bottom(h), *e))
            .min_by(|x, y| x.0.total_cmp(&y.0))
    };
    for _ in 0..=holds.len() {
        let (Some((y, low)), Some((_, high))) = (lowest(h), highest(h)) else {
            return Err(bad("an outline open above or below"));
        };
        if !y.is_finite() {
            return Err(bad("an edge too near upright to compare"));
        }
        if holds.iter().all(|e| e.inside(h, y) >= -allowance) {
            let min = Vec2::new(centre_x - half * h, y);
            return Ok(Some(BoxFit { height: h, min }));
        }
        if !(h > 0.0) {
            return Ok(None);
        }
        // At `h` the lowest bottom the edges below allow is above the highest one the edges above
        // allow. The gap between those bounds is concave in `h`, so no box taller than the height
        // where the two edges that set them meet fits: step down to it. If they meet at or above
        // `h`, or never, the gap stays shut all the way down and no box fits.
        let lower = (low.c * high.b - high.c * low.b) / (low.a * high.b - high.a * low.b);
        if !(lower < h) {
            return Ok(None);
        }
        h = lower.max(0.0);
    }
    Err(SolidError::sm_err(OP_FIT))
}

/// One edge's condition on a box of height `h` with its bottom at `y0`: `a h + b y0 ≥ c`
/// ([`box_fit_in_convex_polygon`]).
#[derive(Clone, Copy, Debug)]
struct Hold {
    a: f64,
    b: f64,
    c: f64,
}

impl Hold {
    /// The bottom at which a box of height `h` meets this condition exactly (`b` not 0).
    fn bottom(self, h: f64) -> f64 {
        (self.c - self.a * h) / self.b
    }

    /// How far the box of height `h` with its bottom at `y0` is inside this condition: its corner
    /// nearest the edge, measured square to the edge from the margin line.
    fn inside(self, h: f64, y0: f64) -> f64 {
        self.a * h + self.b * y0 - self.c
    }
}
