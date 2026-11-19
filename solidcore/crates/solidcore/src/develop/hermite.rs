//! Following a curve carried into the flat by cubic pieces, each proved within a tolerance of it.
//! A piece runs from the carried curve's point and rate at one end of a step of its parameter to
//! its point and rate at the other (cubic Hermite interpolation), and over a step `h` it lies
//! within `h⁴/384 · max|f⁗|` of the carried curve `f` at the same parameter. The cubic's remainder
//! is `f[t₀, t₀, t₁, t₁, t] (t − t₀)² (t − t₁)²`, the divided difference an average of `f⁗ / 4!`
//! over the step (its weights a B-spline's, never below 0, adding to 1), so the bound holds for
//! the length of a vector as for each coordinate, and `(t − t₀)² (t − t₁)²` is at most `h⁴/16`.
//! `max|f⁗|` is bounded from bounds on the curve's own first four derivatives ([`smooth_pieces`])
//! and on the map's that carries it ([`fourth`]), never read off points of the curve; a step whose
//! bound passes the tolerance is halved ([`nodes`]).
//!
//! Every bound is rounded outward by [`ROUNDING`] of the sizes it is made of. A piece also says
//! how far its own form, as it is read, may lie from the curve the record holds
//! ([`Smooth::slack`]): a spline's Bézier pieces are made by knot insertion (NLib's segment
//! extraction and Bézier decomposition, as [`crate::eval::query`] reads a spline's pieces), whose
//! rounding the slack states. A curve that holds no derivatives to bound (a traced intersection)
//! has no smooth pieces here: its caller says what it does instead.
//!
//! Not upstream: SolidCraft's certified cut outlines (its #9245), written for the kernel with the
//! sheet development so that the application computes no geometry (docs/layers.md §1).
//!
//! Deviations: none (no upstream counterpart). No recursion: [`nodes`] halves on an explicit
//! stack, at most `MAX_PIECES` pieces.

use crate::eval::query::bezier_spans;
use crate::eval::vec::{v3, VecExt};
use crate::eval::{Curve, Frame};
use crate::math::{Vec2, Vec3};
use crate::prism::profile::Seg2;

/// The rounding a bound allows for: 64 units in the last place of the sizes it is made of, more
/// than the rounding of computing them (a few operations each, or a level of de Casteljau's or of
/// knot insertion).
pub const ROUNDING: f64 = 64.0 * f64::EPSILON;

/// Most cubic pieces one curve is followed by: far more than a curve of a sheet under 10⁹ mm
/// across needs at any tolerance it is read to.
const MAX_PIECES: usize = 1 << 14;

/// A piece of a curve's parameter over which the curve is smooth (no knot of its spline and no
/// corner of its polyline inside it), the curve's own form there, bounds on the lengths of its
/// first four derivatives anywhere on it, and how far its form, as it is read, may lie from the
/// curve the record holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Smooth {
    span: [f64; 2],
    form: Form,
    rates: [f64; 4],
    slack: f64,
}

/// A smooth piece's curve in closed form.
#[derive(Clone, Debug, PartialEq)]
enum Form {
    /// `a + d (t − from)`.
    Line { a: Vec3, d: Vec3, from: f64 },
    /// `o + x cos t + y sin t`.
    Conic { o: Vec3, x: Vec3, y: Vec3 },
    /// The rational Bézier piece over `over` of the parameter with these homogeneous control
    /// points (every weight above 0).
    Bezier { over: [f64; 2], ctrl: Vec<Hom> },
}

/// A homogeneous control point: its weighted coordinates `(x w, y w, z w)` and its weight.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Hom {
    xyz: Vec3,
    w: f64,
}

impl Hom {
    const ZERO: Hom = Hom {
        xyz: Vec3::ZERO,
        w: 0.0,
    };

    /// The record's `[x w, y w, z w, w]`.
    fn of([x, y, z, w]: [f64; 4]) -> Hom {
        Hom {
            xyz: Vec3::new(x, y, z),
            w,
        }
    }

    /// The point it weights, where its weight can be divided by and the point is finite.
    fn point(self) -> Option<Vec3> {
        let p = Vec3::new(
            self.xyz.x / self.w,
            self.xyz.y / self.w,
            self.xyz.z / self.w,
        );
        (self.w.abs() > f64::MIN_POSITIVE && p.is_finite()).then_some(p)
    }

    /// The point a fraction `u` of the way to `o`.
    fn lerp(self, o: Hom, u: f64) -> Hom {
        Hom {
            xyz: self.xyz * (1.0 - u) + o.xyz * u,
            w: self.w * (1.0 - u) + o.w * u,
        }
    }

    /// From this point to `o`, `k` times.
    fn toward(self, o: Hom, k: f64) -> Hom {
        Hom {
            xyz: (o.xyz - self.xyz) * k,
            w: (o.w - self.w) * k,
        }
    }
}

impl Smooth {
    /// The piece of the parameter.
    #[must_use]
    pub fn span(&self) -> [f64; 2] {
        self.span
    }

    /// At least the length of each of the curve's first four derivatives anywhere on the piece.
    #[must_use]
    pub fn rates(&self) -> [f64; 4] {
        self.rates
    }

    /// At most how far a point of the piece, as [`Smooth::at`] reads it, lies from the curve the
    /// record holds at the same parameter: the rounding of reading it, within [`ROUNDING`] of the
    /// sizes it is made of for a line, a circle, an ellipse or a polyline (whose form is the
    /// record's own numbers, in a few operations), and for a spline's Bézier piece the rounding
    /// of the knot insertion that made it and of de Casteljau's levels that read it (see
    /// [`smooth_pieces`]).
    #[must_use]
    pub fn slack(&self) -> f64 {
        self.slack
    }

    /// The curve's point and first derivative at `t`, from the piece's own form: at an end of the
    /// piece, the rate from its side (a polyline's corner or a spline's knot may turn there).
    #[must_use]
    pub fn at(&self, t: f64) -> Option<(Vec3, Vec3)> {
        let (p, d) = match &self.form {
            Form::Line { a, d, from } => (*a + *d * (t - *from), *d),
            Form::Conic { o, x, y } => {
                let (s, c) = t.sin_cos();
                (*o + *x * c + *y * s, *y * c - *x * s)
            }
            Form::Bezier { over, ctrl } => {
                let [s0, s1] = *over;
                let width = s1 - s0;
                let u = (t - s0) / width;
                let degree = ctrl.len().saturating_sub(1) as f64;
                // De Casteljau's levels: the last is the homogeneous point, and the two points of
                // the one before it give its rate.
                let (mut level, mut rate) = (ctrl.clone(), Hom::ZERO);
                while level.len() > 1 {
                    if let [first, second] = level.as_slice() {
                        rate = first.toward(*second, degree / width);
                    }
                    level = level
                        .iter()
                        .zip(level.iter().skip(1))
                        .map(|(a, b)| a.lerp(*b, u))
                        .collect();
                }
                let h = *level.first()?;
                if !(h.w > 0.0) {
                    return None;
                }
                // The curve is the homogeneous point's `xyz / w`, its rate
                // `(xyz′ w − xyz w′) / w²`.
                (
                    h.xyz * (1.0 / h.w),
                    (rate.xyz * h.w - h.xyz * rate.w) * (1.0 / (h.w * h.w)),
                )
            }
        };
        (p.is_finite() && d.is_finite()).then_some((p, d))
    }
}

/// The pieces of `curve` from parameter `a` to `b` over which it is smooth, in order, each with
/// bounds on its first four derivatives and its slack ([`Smooth::slack`]):
///
/// - a line's rate is its length, and it has no more;
/// - a circle's or an ellipse's every derivative is `±(x′ cos t + y′ sin t)` or
///   `±(y′ cos t − x′ sin t)`, `x′` and `y′` its axes scaled by its radii, so at most
///   `√(max(|x′|², |y′|²) + |x′ · y′|)` long: its larger radius, where its axes are unit vectors
///   square to each other;
/// - a polyline is a line on each of its segments, segment `i` from `i` to `i + 1`;
/// - a spline is cut into its Bézier pieces by knot insertion, each bounded from its control
///   points (see `bezier_rates`). A piece of a spline of degree `p` is made by at most `4 p`
///   levels of insertion (the segment's two ends and the piece's own two, each inserted up to `p`
///   times), and read by `p` levels of de Casteljau's: each level a point a fraction of the way
///   between two points of the level before, which never lies past the larger of them, rounded
///   within a few units in the last place of the largest coordinate it combines. So a point read
///   has its weighted coordinates `A` within `5 p ·` [`ROUNDING`] of the curve's longest
///   weighted point of the exact piece's (`|ΔA|`), and its weight `w` within `5 p ·`
///   [`ROUNDING`] of the curve's heaviest weight (`|Δw|`), and its point `A / w` lies within
///   `(|ΔA| + |A / w| |Δw|) / w` of the exact one: `|A / w|` at most the furthest of the piece's
///   control points from the origin, `w` at least its least weight.
///
/// `None` for a traced intersection (it holds no derivatives), a spline whose pieces can't be had
/// (not a valid B-spline, or a weight not above 0), and a parameter range the curve doesn't
/// cover.
#[must_use]
pub fn smooth_pieces(curve: &Curve, [a, b]: [f64; 2]) -> Option<Vec<Smooth>> {
    if !(a < b) {
        return None;
    }
    let out = match curve {
        Curve::Line { start, end } => {
            let p = v3(*start);
            vec![line(p, v3(*end) - p, 0.0, [a, b])]
        }
        Curve::Circle { frame, radius } => vec![conic(frame, [*radius; 2], [a, b])],
        Curve::Ellipse { frame, radii } => vec![conic(frame, *radii, [a, b])],
        Curve::Polyline { points } => points
            .iter()
            .zip(points.iter().skip(1))
            .enumerate()
            .filter_map(|(i, (p, q))| {
                let (lo, hi) = ((i as f64).max(a), ((i + 1) as f64).min(b));
                let p = v3(*p);
                (lo < hi).then(|| line(p, v3(*q) - p, i as f64, [lo, hi]))
            })
            .collect(),
        Curve::Spline { knots, points } => {
            let ctrl: Vec<[f64; 4]> = points.iter().map(|&[x, y, z]| [x, y, z, 1.0]).collect();
            bezier(knots, &ctrl, [a, b])?
        }
        Curve::RationalSpline { knots, homogeneous } => bezier(knots, homogeneous, [a, b])?,
        Curve::SurfaceIntersection { .. } => return None,
    };
    // The pieces run from `a` to `b`, each from where the one before ends.
    let covered = out.first().is_some_and(|s| s.span[0] == a)
        && out.last().is_some_and(|s| s.span[1] == b)
        && out
            .iter()
            .zip(out.iter().skip(1))
            .all(|(s, next)| s.span[1] == next.span[0]);
    let finite = |s: &Smooth| s.rates.iter().all(|r| r.is_finite()) && s.slack.is_finite();
    (covered && out.iter().all(finite)).then_some(out)
}

/// The line `p + d (t − from)` over `span`: its rate is `d`, and it has no more.
fn line(p: Vec3, d: Vec3, from: f64, span: [f64; 2]) -> Smooth {
    let [lo, hi] = span;
    let far = (lo - from).abs().max((hi - from).abs());
    Smooth {
        span,
        form: Form::Line { a: p, d, from },
        rates: [d.len() * (1.0 + ROUNDING), 0.0, 0.0, 0.0],
        slack: ROUNDING * (p.len() + d.len() * far),
    }
}

/// A circle or an ellipse in `frame`, of `radii` along its axes, over `span`.
fn conic(frame: &Frame, [rx, ry]: [f64; 2], span: [f64; 2]) -> Smooth {
    let (o, x, y) = (
        v3(frame.origin),
        v3(frame.x_axis) * rx,
        v3(frame.y_axis) * ry,
    );
    let r = (x.len2().max(y.len2()) + x.dot(y).abs()).sqrt() * (1.0 + ROUNDING);
    Smooth {
        span,
        form: Form::Conic { o, x, y },
        rates: [r; 4],
        slack: ROUNDING * (o.len() + r),
    }
}

/// The spline on `knots` of homogeneous control points `ctrl`, cut into its Bézier pieces within
/// `a` to `b`, each with its rates and its slack ([`smooth_pieces`]).
fn bezier(knots: &[f64], ctrl: &[[f64; 4]], [a, b]: [f64; 2]) -> Option<Vec<Smooth>> {
    let degree = knots.len().checked_sub(ctrl.len() + 1)?;
    // The sizes knot insertion rounds within: the curve's longest weighted point and heaviest
    // weight (a fraction of the way between two points never lies past the larger).
    let long = ctrl
        .iter()
        .map(|&h| Hom::of(h).xyz.len())
        .fold(0.0, f64::max);
    let heavy = ctrl.iter().map(|&[.., w]| w.abs()).fold(0.0, f64::max);
    let levels = 5.0 * degree as f64 * ROUNDING;
    let mut out = Vec::new();
    for (over, piece) in bezier_spans(knots, ctrl, [a, b])? {
        let [s0, s1] = over;
        let (lo, hi) = (s0.max(a), s1.min(b));
        if !(lo < hi) {
            continue;
        }
        let ctrl: Vec<Hom> = piece.into_iter().map(Hom::of).collect();
        let (rates, light, far) = bezier_rates(&ctrl, s1 - s0)?;
        let slack = levels * (long + far * heavy) / light * (1.0 + ROUNDING);
        out.push(Smooth {
            span: [lo, hi],
            form: Form::Bezier { over, ctrl },
            rates,
            slack,
        });
    }
    Some(out)
}

/// Bounds on the lengths of the first four derivatives of the rational Bézier piece whose
/// homogeneous control points are `ctrl`, over a step `width` of the parameter, with its least
/// weight (rounded down) and the furthest of its points from the origin. About its first point
/// `c`, the piece is `D = A / w`, `A` and `w` polynomials of degree `p`: the `k`-th derivative of
/// each is `p!/(p − k)!/widthᵏ` times a Bézier of its control points' `k`-th differences, so no
/// longer than the longest of them. `A = w D`, so `A⁽ᵏ⁾ = Σⱼ C(k, j) w⁽ʲ⁾ D⁽ᵏ⁻ʲ⁾` (Leibniz), and
/// `|D⁽ᵏ⁾| ≤ (|A⁽ᵏ⁾| + Σⱼ₌₁ᵏ C(k, j) |w⁽ʲ⁾| |D⁽ᵏ⁻ʲ⁾|) / min w`, with `|D|` at most the furthest of
/// its points from `c` (every weight above 0, the piece lies in their hull) and `w` at least its
/// least weight (the Bernstein weights are never below 0 and add to 1). `None` where a weight is
/// not above 0.
fn bezier_rates(ctrl: &[Hom], width: f64) -> Option<([f64; 4], f64, f64)> {
    let p = ctrl.len().checked_sub(1)?;
    let c = ctrl.first()?.point()?;
    let points: Vec<Vec3> = ctrl.iter().map(|h| h.point()).collect::<Option<_>>()?;
    let weighted: Vec<Vec3> = ctrl.iter().map(|h| h.xyz - c * h.w).collect();
    let weights: Vec<f64> = ctrl.iter().map(|h| h.w).collect();
    // The sizes the differences are made of, for their rounding.
    let size = ctrl
        .iter()
        .map(|h| h.xyz.len() + c.len() * h.w.abs())
        .fold(0.0, f64::max);
    let heavy = weights.iter().map(|w| w.abs()).fold(0.0, f64::max);
    let light = weights.iter().copied().fold(f64::INFINITY, f64::min) - ROUNDING * heavy;
    let far = points.iter().map(|q| q.len()).fold(0.0, f64::max);
    let reach = points.iter().map(|q| q.dist(c)).fold(0.0, f64::max) + ROUNDING * (c.len() + far);
    if !(light > 0.0 && width > 0.0) {
        return None;
    }
    // The bounds on the lengths of `A`'s and `w`'s first four derivatives.
    let (mut da, mut dw) = (weighted, weights);
    let (mut rate_a, mut rate_w) = ([0.0; 4], [0.0; 4]);
    let mut falling = 1.0;
    for (k, (ra, rw)) in (1..=4usize).zip(rate_a.iter_mut().zip(rate_w.iter_mut())) {
        // Past the degree, no derivative.
        if da.len() < 2 {
            break;
        }
        da = da
            .iter()
            .zip(da.iter().skip(1))
            .map(|(x, y)| *y - *x)
            .collect();
        dw = dw
            .iter()
            .zip(dw.iter().skip(1))
            .map(|(x, y)| y - x)
            .collect();
        falling *= (p + 1 - k) as f64;
        let scale = falling / width.powi(k as i32);
        let spread = f64::from(1u32 << k) * ROUNDING;
        *ra = scale * (da.iter().map(|v| v.len()).fold(0.0, f64::max) + spread * size);
        *rw = scale * (dw.iter().map(|v| v.abs()).fold(0.0, f64::max) + spread * heavy);
    }
    // `|D⁽ᵏ⁾|` from `k = 0`, each from those before it.
    let mut d = vec![reach];
    for (k, ra) in (1..=4usize).zip(rate_a) {
        let mut sum = ra;
        for ((j, rw), dj) in (1..=k).zip(rate_w).zip(d.iter().rev()) {
            sum += choose(k, j) * rw * dj;
        }
        d.push(sum / light * (1.0 + ROUNDING));
    }
    let [_, d1, d2, d3, d4] = d.as_slice() else {
        return None;
    };
    let out = [*d1, *d2, *d3, *d4];
    out.iter()
        .all(|x| x.is_finite())
        .then_some((out, light, far))
}

/// The binomial coefficient `C(k, j)`, `j ≤ k`.
fn choose(k: usize, j: usize) -> f64 {
    (0..j).fold(1.0, |c, i| c * (k - i) as f64 / (i + 1) as f64)
}

/// A bound on the length of the fourth derivative of a map `g` of a curve `c`, `g(c(t))`, from
/// bounds `c` on the lengths of the curve's first four derivatives and `g` on the map's (as
/// multilinear maps): `f⁗ = g′[c⁗] + g″(4 [c′, c‴] + 3 [c″, c″]) + 6 g‴[c′, c′, c″] + g⁗[c′, c′,
/// c′, c′]` (Faà di Bruno).
#[must_use]
pub fn fourth(c: [f64; 4], g: [f64; 4]) -> f64 {
    let ([c1, c2, c3, c4], [g1, g2, g3, g4]) = (c, g);
    (g1 * c4 + g2 * (4.0 * c1 * c3 + 3.0 * c2 * c2) + 6.0 * g3 * c1 * c1 * c2 + g4 * c1.powi(4))
        * (1.0 + ROUNDING)
}

/// How far a cubic piece over a step `h` lies at most from the carried curve it follows, where
/// the curve's fourth derivative is at most `fourth` long over the step (module docs).
#[must_use]
pub fn remainder(h: f64, fourth: f64) -> f64 {
    h.powi(4) / 384.0 * fourth * (1.0 + ROUNDING)
}

/// The parameters from `a` to `b`, in order, between which cubic pieces stand proved within `tol`
/// of the curve they follow: `off(t₀, t₁)` is a proven bound on how far the piece from `t₀` to
/// `t₁` lies from it (`None` where none is had over so long a step), and a step whose bound passes
/// `tol` is halved. `None` where a step is still over it at `MAX_PIECES` pieces, or can't be
/// halved any more.
pub fn nodes(
    off: impl Fn(f64, f64) -> Option<f64>,
    [a, b]: [f64; 2],
    tol: f64,
) -> Option<Vec<f64>> {
    let mut out = vec![a];
    // The last pushed is looked at first: pushing the right half before the left keeps the
    // output in order.
    let mut todo = vec![(a, b)];
    while let Some((t0, t1)) = todo.pop() {
        if off(t0, t1).is_some_and(|d| d <= tol) {
            out.push(t1);
            continue;
        }
        let m = 0.5 * (t0 + t1);
        if !(t0 < m && m < t1) || out.len() + todo.len() >= MAX_PIECES {
            return None;
        }
        todo.push((m, t1));
        todo.push((t0, m));
    }
    Some(out)
}

/// The cubic from `p0`, leaving it at the rate `d0`, to `p3`, reaching it at the rate `d3`, over a
/// step `h` of the parameter: at `u` of the way along it stands for the curve at `t₀ + u h`.
#[must_use]
pub fn cubic(p0: Vec2, d0: Vec2, p3: Vec2, d3: Vec2, h: f64) -> Seg2 {
    let third = h / 3.0;
    Seg2::Cubic {
        p0,
        p1: p0 + d0 * third,
        p2: p3 - d3 * third,
        p3,
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_1_SQRT_2, SQRT_2};

    use super::{cubic, fourth, nodes, remainder, smooth_pieces, ROUNDING};
    use crate::develop::plane::Planar;
    use crate::eval::{Curve, Frame, Surface};
    use crate::math::{Vec2, Vec3};
    use crate::prism::profile::Seg2;

    /// The quarter of the unit circle as its rational quadratic, (1, 0) and (0, 1) of weight 1
    /// and (1, 1) of weight √½ between.
    fn quarter() -> Curve {
        let s = FRAC_1_SQRT_2;
        Curve::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: vec![[1.0, 0.0, 0.0, 1.0], [s, s, 0.0, s], [0.0, 1.0, 0.0, 1.0]],
        }
    }

    /// A quarter of the unit circle as its rational quadratic, (1, 0) and (0, 1) of weight 1 and
    /// (1, 1) of weight √½ between, is followed by 32 cubic pieces, each proved within 7.21e-7 of
    /// it. About c = (1, 0), its weighted points are A = (0, 0), (0, 0.70711), (−1, 1) and its
    /// weights w = 1, 0.70711, 1. A′ is at most 2 · |(−1, 0.29289)| = 2.08402 long, A″ at most
    /// 2 · |(−1, −0.41421)| = 2.16478, and A‴ is 0; |w′| ≤ 2 · 0.29289 = 0.58579, |w″| ≤ 2 ·
    /// 0.58579 = 1.17157. The curve less c is at most √2 = 1.41421 long and w at least 0.70711, so
    /// |c′| ≤ (2.08402 + 0.58579 · 1.41421) / 0.70711 = 4.11882, |c″| ≤ (2.16478 + 2 · 0.58579 ·
    /// 4.11882 + 1.17157 · 1.41421) / 0.70711 = 12.2289, |c‴| ≤ (3 · 0.58579 · 12.2289 + 3 ·
    /// 1.17157 · 4.11882) / 0.70711 = 50.865 and |c⁗| ≤ (4 · 0.58579 · 50.865 + 6 · 1.17157 ·
    /// 12.2289) / 0.70711 = 290.12. Carried rigidly (the map's bounds 1, 0, 0, 0), a piece over a
    /// step h lies within h⁴/384 · 290.12 of the curve: 1.153e-5 at h = 1/16, over 1e-5, and
    /// 7.21e-7 at 1/32. Each piece is checked at seven points against the curve at the same
    /// parameter, and against the circle (its true |c⁗| is about 22.6, so it lies within about
    /// 5.6e-8 of both).
    #[test]
    fn a_quarter_circle_is_followed_by_cubic_pieces_within_their_bound() {
        let s = FRAC_1_SQRT_2;
        let curve = quarter();
        let pieces = smooth_pieces(&curve, [0.0, 1.0])
            .unwrap_or_else(|| panic!("a rational quadratic is smooth"));
        let [piece] = pieces.as_slice() else {
            panic!("one Bézier piece: {pieces:?}")
        };
        let rates = piece.rates();
        for (got, want) in rates.iter().zip([4.11882, 12.2289, 50.865, 290.12]) {
            assert!((got - want).abs() < 1e-4 * want, "{rates:?}");
        }
        let f4 = fourth(rates, [1.0, 0.0, 0.0, 0.0]);
        let bound = remainder(1.0 / 32.0, f4);
        assert!(
            (bound - 7.21e-7).abs() < 1e-9 && remainder(1.0 / 16.0, f4) > 1e-5,
            "{bound}"
        );
        let ts = nodes(|t0, t1| Some(remainder(t1 - t0, f4)), [0.0, 1.0], 1e-5)
            .unwrap_or_else(|| panic!("followed"));
        assert_eq!(ts.len(), 33, "{ts:?}");
        let exact = |t: f64| {
            let (u, v) = ((1.0 - t) * (1.0 - t), t * t);
            let m = 2.0 * t * (1.0 - t) * s;
            Vec2::new(u + m, m + v) * (1.0 / (u + m + v))
        };
        for w in ts.windows(2) {
            let [t0, t1] = [w[0], w[1]];
            let (Some((p0, d0)), Some((p3, d3))) = (piece.at(t0), piece.at(t1)) else {
                panic!("{t0} {t1}")
            };
            let flat = |p: Vec3| Vec2::new(p.x, p.y);
            let seg = cubic(flat(p0), flat(d0), flat(p3), flat(d3), t1 - t0);
            assert!(matches!(seg, Seg2::Cubic { .. }));
            for i in 1..8 {
                let u = f64::from(i) / 8.0;
                let q = seg.point_at(u);
                let off = q.dist(exact(t0 + u * (t1 - t0)));
                assert!(
                    off <= bound && (q.len() - 1.0).abs() <= bound,
                    "at {t0} + {u}: {off}, {}",
                    q.len()
                );
            }
        }
    }

    /// A traced intersection holds no derivatives: it has no smooth pieces to follow.
    #[test]
    fn a_traced_intersection_has_no_smooth_pieces() {
        let leader = Curve::Line {
            start: [0.0, 0.0, 0.0],
            end: [1.0, 0.0, 0.0],
        };
        let plane = |n: [f64; 3]| Surface::Plane {
            origin: [0.0; 3],
            u_axis: [1.0, 0.0, 0.0],
            v_axis: n,
        };
        let traced = Curve::SurfaceIntersection {
            surfaces: [
                Box::new(plane([0.0, 1.0, 0.0])),
                Box::new(plane([0.0, 0.0, 1.0])),
            ],
            leader: Box::new(leader),
            tolerance: 1e-6,
        };
        assert!(smooth_pieces(&traced, [0.0, 1.0]).is_none());
    }

    /// A piece's slack is the rounding of reading it. The line from (1, 2, 2) to (4, 6, 2),
    /// 3 from the origin at t = 0 and 5 long, over t from 0 to 1: ROUNDING · (3 + 5 · 1) =
    /// 8 ROUNDING. The circle of radius 2 about (0, 3, 4), 5 from the origin, its axes unit and
    /// square: every derivative at most 2 long, and ROUNDING · (5 + 2) = 7 ROUNDING. The quarter
    /// circle's one Bézier piece, of degree 2: its longest weighted point is 1 long, its heaviest
    /// weight 1, its furthest point (1, 1) √2 from the origin and its least weight √½, so
    /// 5 · 2 · ROUNDING · (1 + √2 · 1) / √½ = 10 ROUNDING (2 + √2) = 34.142 ROUNDING
    /// (4.8519e-13).
    #[test]
    fn a_pieces_slack_is_the_rounding_of_reading_it() {
        let near = |got: f64, want: f64| (got - want).abs() <= 1e-9 * want;
        let line = Curve::Line {
            start: [1.0, 2.0, 2.0],
            end: [4.0, 6.0, 2.0],
        };
        let pieces = smooth_pieces(&line, [0.0, 1.0]).unwrap_or_else(|| panic!("a line"));
        let [piece] = pieces.as_slice() else {
            panic!("one piece: {pieces:?}")
        };
        assert!(near(piece.slack(), 8.0 * ROUNDING), "{}", piece.slack());
        let circle = Curve::Circle {
            frame: Frame {
                origin: [0.0, 3.0, 4.0],
                x_axis: [1.0, 0.0, 0.0],
                y_axis: [0.0, 1.0, 0.0],
            },
            radius: 2.0,
        };
        let pieces = smooth_pieces(&circle, [0.0, 1.0]).unwrap_or_else(|| panic!("a circle"));
        let [piece] = pieces.as_slice() else {
            panic!("one piece: {pieces:?}")
        };
        assert!(near(piece.rates()[0], 2.0), "{:?}", piece.rates());
        assert!(near(piece.slack(), 7.0 * ROUNDING), "{}", piece.slack());
        let pieces = smooth_pieces(&quarter(), [0.0, 1.0]).unwrap_or_else(|| panic!("smooth"));
        let [piece] = pieces.as_slice() else {
            panic!("one piece: {pieces:?}")
        };
        let want = 10.0 * ROUNDING * (2.0 + SQRT_2);
        assert!(
            (piece.slack() - want).abs() <= 1e-3 * want,
            "{} {want}",
            piece.slack()
        );
    }
}
