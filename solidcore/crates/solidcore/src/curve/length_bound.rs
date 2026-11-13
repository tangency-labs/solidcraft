//! (not upstream) A curve's arc length as a certified enclosure: the exact length of the
//! curve as defined lies in the returned [`Bracket`], rounding included.
//!
//! Upstream: none (#3184). [`Curve::length`] integrates the speed with an adaptive
//! quadrature whose steps are accepted on an estimate, so it has no error bound; a caller
//! that refuses on a length compares against this bracket instead.
//!
//! | Rust                    | Kind                                                     |
//! |-------------------------|----------------------------------------------------------|
//! | [`Curve::length_bound`] | a line, a circular arc, a B-spline (rational or not)     |
//!
//! **A line**: the distance between its ends, each known to `gamma_3` of its size (`P0 + t D`).
//!
//! **A circular arc** (an ellipse whose radii agree): `r` times its angle, the angle summed
//! over pieces between its knots, halved, each less than a quarter turn, as `2 asin(c / 2r)`
//! of the piece's chord `c`.
//!
//! **A B-spline** (and an ellipse whose radii differ, a parabola, a hyperbola, by their
//! B-spline), piece by piece over sub-intervals of its knot spans. A piece's homogeneous
//! Bezier control points are read off the span by blossoming on the piece's own interval
//! (not subdivided from the span's, so their rounding does not grow with the subdivision).
//! Its length is at least its chord `c`, the shortest path between its ends, and at most
//! `c / cos phi`: the curve's tangent `C' = (A' W - A W') / W^2` points into the cone of the
//! Bernstein coefficients of `A' W - A W'` (`W > 0`, the basis non-negative), so every
//! element of length is at most `phi` from the chord's direction, `phi` the widest angle
//! between the chord and a coefficient, and its projection on the chord, `c` in all, is at
//! least `cos phi` of it. The cone is taken from the Bernstein coefficients of
//! `A' W - A W'`, whose direction is the tangent's since `W^2 > 0`. A piece whose cone
//! reaches a quarter turn from its chord is never bounded above: it is split. The piece whose bracket is widest is halved until the brackets'
//! widths sum to the width asked or the pieces reach their count ([`MAX_PIECES`]); the
//! bracket is then returned as it stands, wider than asked but still certified. A piece's
//! `phi` falls with its length, so `c / cos phi - c` falls with its cube and the total width
//! with the square of the piece length; each piece also carries its ends' rounding, which
//! halving does not narrow, so the halving stops where the widest piece's narrowable part
//! is no more than its rounding (measured: a bracket 4.9e-9 wide, 16384 pieces, on a
//! rational arc of length 4 at the origin; 5.6e-9 on a cubic).
//!
//! **Rounding.** The points are taken in difference form, `w_s (P_s - P_0)` on a span (to
//! `2u` relative by Kahan's products), so that their rounding scales with the span's size,
//! not with its distance from the origin. A level of the blossom is `z = (1 - a) x + a y`,
//! `a = (u - l) / (h - l)` with `u` a piece's end and `l <= u <= h` knots (#14434): a piece
//! lies within its knot span `[t_j, t_(j+1)]` (refused, named, if not), and every level's
//! knots `l <= t_j`, `h >= t_(j+1)`, so `a` lies in `[0, 1]`. `u`, `l` and `h` are doubles,
//! exact, so each of the two differences and the quotient rounds once relative to its own
//! result: the computed `a` is within `gamma_3 a` of `a`, and `1 - a`, rounded once more,
//! within `gamma_3 a (1 + u) + u (1 - a)` of `1 - a` (`u` here the unit roundoff). The two
//! products and the sum add `gamma_2` of `|x| (1 - a) + |y| a`, so with `M` at least `|x|`
//! and `|y|` a level with exact inputs is within `M (2 gamma_3 + u gamma_3) + gamma_2 M (1 +
//! gamma_7) <= gamma_9 M` of `z` (`gamma_a + gamma_b + gamma_a gamma_b <= gamma_(a+b)`). Its
//! inputs' errors `E` carry through the combination, whose coefficients are at most 1 and sum
//! to 1, unamplified: `E_r <= (1 + gamma_9) E_(r-1) + gamma_9 M`, and `M` stays the span's
//! largest point or weight (every level is a convex combination of the last). From the
//! difference form's `E_0 = gamma_3 M`, the `p` levels give `E_p <= gamma_(9p+3) M` for a
//! point, and from exact weights `gamma_(9p) w_max` for a weight, a polynomial span's
//! (weights 1) too, as its points are divided by the weights as computed. (No term in the
//! knots' size: no difference of them is formed from a rounded value.) A blossomed weight is
//! then at least `w_min`, the span's least less that rounding, and a Euclidean end is known
//! to `(e + |X| e_w) / w_min` plus `gamma_2` of it; a chord to twice that times `sqrt3` plus `gamma_4` of
//! itself, and its direction to the `asin` of that over its length. A coefficient of the
//! cone is a combination of weight 1 of products of the points' differences and weights:
//! its inputs' rounding carries in through the products, its own arithmetic to
//! `gamma_(4p + 8)` of its terms, and its direction to the `asin` of that over its length.
//! The arc's ends are the curve's evaluation, known to `gamma_(16 (p + 2)) |P|` (a few ulps
//! for each of the rational evaluation's operations), and `asin` and the
//! products to `gamma_8` of the result; the sums to `gamma_n` of their size.
//!
//! Deviations: not a port. Other kinds (offset, composite, iso, projected and
//! curve-on-surface curves, a Hermite curve) are refused `Unsupported`: no estimate stands
//! in for a bound. No recursion; the pieces are a queue bounded by [`MAX_PIECES`].

use crate::curve::curve::{Curve, CurveKind};
use crate::error::curve::CurveFailure;
use crate::error::{Failure, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::vector3d::Vec3;
use crate::kernel::Kernel;
use crate::math::PI;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::math::{gamma, CPoint, NOW, NOZ};
use crate::surface::side::Bracket;

const OP: &str = "curve::length_bound";

/// The most pieces a B-spline's bracket is halved into before it is returned as it stands.
pub const MAX_PIECES: usize = 1 << 16;

impl Curve {
    /// The arc length over `interval`, certified: the exact length lies in the bracket.
    /// `width` is the widest bracket asked: a B-spline's pieces are halved until the
    /// bracket is that narrow, or until no halving narrows it (the widest piece is mostly
    /// its own rounding), or they reach [`MAX_PIECES`]. A bracket that cannot be as narrow
    /// as asked because of its rounding (a line's or an arc's as found, a B-spline's at
    /// that stop) is refused, naming the narrowest it proves; one still wider than asked at
    /// [`MAX_PIECES`] is refused, naming the width it reached. A returned bracket is always
    /// at most `width` wide.
    ///
    /// # Errors
    ///
    /// [`CurveFailure::LengthBelowFloor`] (a [`SolidError::Failed`]) for a width below the
    /// narrowest its rounding allows, [`CurveFailure::LengthPastPieceCap`] for one the
    /// curve would need more than [`MAX_PIECES`] for; `Unsupported` for a kind it does not
    /// enclose (module docs); `InvalidInput` for an interval or width that is not finite,
    /// or a malformed curve; those of the evaluations and the parameter conversions.
    pub fn length_bound(&self, k: &Kernel, interval: &Extent1d, width: f64) -> Result<Bracket> {
        let (a, b) = (interval.min(), interval.max());
        if !(a.is_finite() && b.is_finite() && width.is_finite() && width >= 0.0) {
            return Err(SolidError::invalid_input(
                OP,
                "an interval or width not finite",
            ));
        }
        if b - a <= 0.0 {
            return Ok(Bracket {
                lower: 0.0,
                upper: 0.0,
            });
        }
        match self.kind() {
            CurveKind::Line(_) => {
                let (p, q) = (self.evaluate_point(k, a)?, self.evaluate_point(k, b)?);
                // a line's point is `P0 + t D`: one product and one sum, so each end is within
                // gamma_3 of its size of the exact point
                let e = gamma(3) * (1.0 + p.max_dimension().max(q.max_dimension()));
                let chord = (q - p).length();
                let slack = 2.0 * 3.0_f64.sqrt() * e + gamma(4) * chord;
                at_floor(
                    Bracket {
                        lower: (chord - slack).max(0.0),
                        upper: chord + slack,
                    },
                    width,
                    1,
                )
            }
            CurveKind::Ellipse(el) if el.is_circle() => {
                let (bracket, pieces) =
                    arc_bound(k, self, el.base(), el.x_radius().max(el.y_radius()), a, b)?;
                at_floor(bracket, width, pieces)
            }
            CurveKind::Ellipse(el) => spline_bound(el.base(), a, b, width),
            CurveKind::BSpline(c) => spline_bound(c, a, b, width),
            CurveKind::Parabola(c) => spline_bound(c.base(), a, b, width),
            CurveKind::Hyperbola(c) => spline_bound(c.base(), a, b, width),
            _ => Err(SolidError::Unsupported {
                operation: OP,
                reason: "no certified length for this curve kind",
            }),
        }
    }
}

/// How far an evaluated point of a curve of degree `p` and size `size` can be from the
/// exact one (module docs).
fn evaluation_rounding(p: usize, size: f64) -> f64 {
    gamma(16 * (p + 2)) * (1.0 + size)
}

/// The unique knots of `c` strictly inside `(a, b)`, with `a` and `b`: the pieces lie in
/// one knot span each.
fn breaks(c: &BSplineCurve, a: f64, b: f64) -> Result<Vec<f64>> {
    let (unique, _) = c.knots(None)?;
    let mut out = vec![a];
    out.extend(unique.into_iter().filter(|&t| t > a && t < b));
    out.push(b);
    Ok(out)
}

/// A circular arc of radius `r` over `[a, b]` of `curve`, whose B-spline is `base`: its
/// pieces between knots, halved, each less than a quarter turn, summed as `2 r asin(c /
/// 2r)` of their chords (module docs), with the count of those pieces.
fn arc_bound(
    k: &Kernel,
    curve: &Curve,
    base: &BSplineCurve,
    r: f64,
    a: f64,
    b: f64,
) -> Result<(Bracket, usize)> {
    if !(r.is_finite() && r > 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "a radius not finite and positive",
        ));
    }
    let p = base.degree()?;
    let mut ends = Vec::new();
    for w in breaks(base, a, b)?.windows(2) {
        let [lo, hi] = w else { continue };
        ends.push(*lo);
        ends.push(0.5 * (lo + hi));
    }
    ends.push(b);
    let points = ends
        .iter()
        .map(|&t| curve.evaluate_point(k, t))
        .collect::<Result<Vec<_>>>()?;
    let size = points.iter().fold(0.0_f64, |m, x| m.max(x.max_dimension()));
    let e = evaluation_rounding(p, size);
    let (mut lower, mut upper) = (0.0_f64, 0.0_f64);
    for w in points.windows(2) {
        let [x, y] = w else { continue };
        let c = (*y - *x).length();
        // the chord to its points' rounding and its own
        let slack = 2.0 * 3.0_f64.sqrt() * e + gamma(4) * c;
        let angle = |c: f64| 2.0 * (c / (2.0 * r)).clamp(0.0, 1.0).asin();
        let (lo, hi) = (angle((c - slack).max(0.0)), angle(c + slack));
        if hi >= 0.5 * PI {
            // a piece of a quarter turn or more: the halving did not reach the knots'
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "an arc piece of a quarter turn or more",
            });
        }
        lower += r * lo;
        upper += r * hi;
    }
    let n = points.len() + 8;
    Ok((
        Bracket {
            lower: (lower * (1.0 - gamma(n))).max(0.0),
            upper: upper * (1.0 + gamma(n)),
        },
        points.len().saturating_sub(1),
    ))
}

/// One piece `[a, b]` of a knot span, with its bracket.
struct Piece {
    a: f64,
    b: f64,
    lower: f64,
    upper: f64,
    /// The part of the width halving narrows, `c / cos phi - c` (infinite without an
    /// upper bound); the rest is rounding, which halving does not narrow.
    geometric: f64,
}

/// A homogeneous point `(x w, y w, z w, w)` in difference form.
#[derive(Clone, Copy)]
struct H {
    x: Vec3,
    w: f64,
}

/// The knot span `j` of a B-spline of degree `p` (rational or not): its control points in
/// homogeneous difference form, `w_s (P_s - P_0)` and `w_s`, its local knots, and the
/// rounding of the points and weights read off it by blossoming.
struct Span {
    p: usize,
    /// The knots `t_(j-p+1) ..= t_(j+p)`.
    knots: Vec<f64>,
    /// The knot span `[t_j, t_(j+1)]`: a piece within it is blossomed to the rounding bound.
    start: f64,
    end: f64,
    points: Vec<H>,
    /// The rounding of a homogeneous difference point read off the span (module docs).
    rounding: f64,
    /// The rounding of a weight read off the span.
    rounding_w: f64,
    /// A lower bound of the weights blossomed on the span: its least weight (every exact
    /// weight there is a convex combination of its weights) less their rounding.
    w_min: f64,
}

/// `a b - c d` by Kahan's algorithm: within `2u` relative of the exact value, barring
/// underflow (as `nurbs::b_spline_curve::speed` forms the span's difference points).
fn difference_of_products(a: f64, b: f64, c: f64, d: f64) -> f64 {
    let cd = c * d;
    let error = c.mul_add(d, -cd);
    a.mul_add(b, -cd) - error
}

impl Span {
    fn of(c: &BSplineCurve, mid: f64) -> Result<Self> {
        let p = c.degree()?;
        let cpts = c.control_points()?;
        let (unique, mult) = c.knots(None)?;
        let mut knots = Vec::new();
        for (t, m) in unique.iter().zip(&mult) {
            for _ in 0..*m {
                knots.push(*t);
            }
        }
        let bad = || SolidError::invalid_input(OP, "a malformed B-spline");
        let last = cpts.len().checked_sub(1).ok_or_else(bad)?;
        let j = knots
            .iter()
            .rposition(|&t| t <= mid)
            .ok_or_else(bad)?
            .min(last);
        let first = j.checked_sub(p).ok_or_else(bad)?;
        let span = cpts.get(first..=j).ok_or_else(bad)?;
        let c0 = *span.first().ok_or_else(bad)?;
        let rational = c0.w != NOW;
        if span.iter().any(|c| (c.w != NOW) != rational) {
            return Err(bad());
        }
        let z = |c: &CPoint| if c.z == NOZ { 0.0 } else { c.z };
        let w0 = if rational { c0.w } else { 1.0 };
        let mut points = Vec::new();
        for c in span {
            let w = if rational { c.w } else { 1.0 };
            if !(w.is_finite() && w > 0.0) {
                return Err(SolidError::invalid_input(OP, "a weight is not positive"));
            }
            // w_s (P_s - P_0) = (h_s w_0 - h_0 w_s) / w_0, to 2u relative
            let x = Vec3::new(
                difference_of_products(c.x, w0, c0.x, w) / w0,
                difference_of_products(c.y, w0, c0.y, w) / w0,
                difference_of_products(z(c), w0, z(&c0), w) / w0,
            );
            points.push(H { x, w });
        }
        let local = (j + 1)
            .checked_sub(p)
            .and_then(|s| knots.get(s..=j + p))
            .ok_or_else(bad)?
            .to_vec();
        let size = points.iter().fold(0.0_f64, |m, h| m.max(h.x.length()));
        let (w_min, w_max) = points.iter().fold((f64::INFINITY, 0.0_f64), |(lo, hi), h| {
            (lo.min(h.w), hi.max(h.w))
        });
        // the knot span [t_j, t_(j+1)]: every level's knots l <= t_j and h >= t_(j+1)
        let (Some(&start), Some(&end)) = (local.get(p.wrapping_sub(1)), local.get(p)) else {
            return Err(bad());
        };
        if !(start.is_finite() && end.is_finite() && end > start) {
            return Err(bad());
        }
        // a level moves a coordinate by at most gamma_9 of the span's largest, p levels
        // gamma_(9p), from the difference form's gamma_3 (module docs, #14434)
        let levels = p.checked_mul(9).ok_or_else(bad)?;
        let rounding = gamma(levels.checked_add(3).ok_or_else(bad)?) * size;
        // (a polynomial span's weights are 1, but blossomed to the same bound: its points
        // are divided by them as computed)
        let rounding_w = gamma(levels) * w_max;
        if !(rounding.is_finite() && rounding_w.is_finite()) {
            return Err(bad());
        }
        let w_min = w_min - rounding_w;
        if !(w_min.is_finite() && w_min > 0.0) {
            return Err(bad());
        }
        Ok(Self {
            p,
            knots: local,
            start,
            end,
            points,
            rounding,
            rounding_w,
            w_min,
        })
    }

    /// The blossom `b(u_1, .., u_p)` on this span (de Boor's levels with one parameter
    /// each), homogeneous.
    fn blossom(&self, u: &[f64]) -> H {
        let p = self.p;
        let mut d = self.points.clone();
        for (r, &ur) in (1..=p).zip(u) {
            for i in (r..=p).rev() {
                // global i is j - p + i; its knots t_(j-p+i) and t_(j+1+i-r)
                let (Some(&lo), Some(&hi)) = (self.knots.get(i - 1), self.knots.get(i + p - r))
                else {
                    continue;
                };
                let alpha = if hi > lo { (ur - lo) / (hi - lo) } else { 0.0 };
                let (Some(&x), Some(&y)) = (d.get(i - 1), d.get(i)) else {
                    continue;
                };
                if let Some(slot) = d.get_mut(i) {
                    *slot = H {
                        x: x.x * (1.0 - alpha) + y.x * alpha,
                        w: x.w * (1.0 - alpha) + y.w * alpha,
                    };
                }
            }
        }
        d.get(p).copied().unwrap_or(H {
            x: Vec3::ZERO,
            w: 1.0,
        })
    }

    /// The homogeneous Bezier control points of the curve on `[a, b]` within this span.
    fn bezier(&self, a: f64, b: f64) -> Vec<H> {
        (0..=self.p)
            .map(|i| {
                let u: Vec<f64> = (0..self.p)
                    .map(|s| if s < self.p - i { a } else { b })
                    .collect();
                self.blossom(&u)
            })
            .collect()
    }

    /// The bracket of the piece `[a, b]` (module docs): its chord `c`, and `c / cos phi`,
    /// `phi` the widest angle between the chord and the hodograph's cone. A piece outside
    /// this span is refused, named: the rounding bound holds within it (module docs).
    fn piece(&self, a: f64, b: f64) -> Result<Piece> {
        if !(self.start <= a && a < b && b <= self.end) {
            return Err(Failure::new(
                OP,
                CurveFailure::PieceOutsideSpan {
                    a,
                    b,
                    span_start: self.start,
                    span_end: self.end,
                },
            )
            .into_error());
        }
        let open = |lower: f64| Piece {
            a,
            b,
            lower,
            upper: f64::INFINITY,
            geometric: f64::INFINITY,
        };
        let pts = self.bezier(a, b);
        let (Some(&h0), Some(&h1)) = (pts.first(), pts.last()) else {
            return Ok(open(0.0));
        };
        // the ends, Euclidean (from P_0), and their rounding
        let (e, ew, wl) = (self.rounding, self.rounding_w, self.w_min);
        let size = pts.iter().fold(0.0_f64, |m, h| m.max(h.x.length() / h.w));
        let e_point = (e + size * ew) / wl + gamma(2) * size;
        let (x0, x1) = (h0.x * (1.0 / h0.w), h1.x * (1.0 / h1.w));
        let chord = x1 - x0;
        let c = chord.length();
        let slack = 2.0 * 3.0_f64.sqrt() * e_point + gamma(4) * c;
        let lower = (c - slack).max(0.0);
        if c <= slack {
            return Ok(open(lower));
        }
        let unit = chord * (1.0 / c);
        // the chord's own direction to its rounding
        let du = (slack / c).min(1.0).asin();
        // the hodograph's numerator N = A' W - A W', degree 2p - 1, in Bernstein form with
        // the binomials normalised (Vandermonde: each coefficient a combination of weight 1)
        let p = self.p;
        let pf = p as f64;
        let n = 2 * p - 1;
        let binom = |n: usize, k: usize| -> f64 {
            let mut v = 1.0_f64;
            for i in 0..k.min(n - k) {
                // (multiplied, then divided: each step an integer, so exact)
                v *= (n - i) as f64;
                v /= (i + 1) as f64;
            }
            v
        };
        let dh: Vec<Vec3> = pts
            .windows(2)
            .map(|w| match w {
                [x, y] => (y.x - x.x) * pf,
                _ => Vec3::ZERO,
            })
            .collect();
        let dw: Vec<f64> = pts
            .windows(2)
            .map(|w| match w {
                [x, y] => (y.w - x.w) * pf,
                _ => 0.0,
            })
            .collect();
        let dh_max = dh.iter().fold(0.0_f64, |m, v| m.max(v.length()));
        let dw_max = dw.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
        let h_max = pts.iter().fold(0.0_f64, |m, h| m.max(h.x.length()));
        let w_max = pts.iter().fold(0.0_f64, |m, h| m.max(h.w));
        // the inputs' rounding carried into a coefficient
        let input = pf * ((2.0 * e * w_max + dh_max * ew) + (e * dw_max + h_max * 2.0 * ew));
        let mut cos_phi = f64::INFINITY;
        for kk in 0..=n {
            let norm = binom(n, kk);
            let mut v = Vec3::ZERO;
            let mut terms = 0.0_f64;
            for i in 0..p {
                let Some(jj) = kk.checked_sub(i).filter(|&jj| jj <= p) else {
                    continue;
                };
                let (Some(d), Some(h)) = (dh.get(i), pts.get(jj)) else {
                    continue;
                };
                let lam = binom(p - 1, i) * binom(p, jj) / norm;
                v += *d * (lam * h.w);
                terms += lam * d.length() * h.w;
            }
            for i in 0..=p {
                let Some(jj) = kk.checked_sub(i).filter(|&jj| jj < p) else {
                    continue;
                };
                let (Some(h), Some(dwj)) = (pts.get(i), dw.get(jj)) else {
                    continue;
                };
                let mu = binom(p, i) * binom(p - 1, jj) / norm;
                v -= h.x * (mu * dwj);
                terms += mu * h.x.length() * dwj.abs();
            }
            let len = v.length();
            let err = input + gamma(4 * p + 8) * terms;
            if len <= err {
                return Ok(open(lower));
            }
            let along = unit.dot(v);
            if along <= 0.0 {
                return Ok(open(lower));
            }
            // the angle to the chord as atan2 of its sine and cosine parts (an acos near 1
            // knows a small angle only to sqrt(eps)), widened by the coefficient's and the
            // chord's rounding
            let phi = unit.cross(v).length().atan2(along) + (err / len).min(1.0).asin() + du;
            if phi >= 0.5 * PI {
                return Ok(open(lower));
            }
            cos_phi = cos_phi.min(phi.cos());
        }
        if !(cos_phi.is_finite() && cos_phi > 0.0) {
            return Ok(open(lower));
        }
        Ok(Piece {
            a,
            b,
            lower,
            upper: (c + slack) / cos_phi * (1.0 + gamma(8)),
            geometric: c / cos_phi - c,
        })
    }
}

/// A piece in the queue, ordered by the width of its bracket.
struct Queued(usize, Piece);

impl Queued {
    /// The order: the piece whose width halving narrows most first.
    fn key(&self) -> f64 {
        self.1.geometric
    }
}

impl PartialEq for Queued {
    fn eq(&self, other: &Self) -> bool {
        self.key().total_cmp(&other.key()).is_eq()
    }
}

impl Eq for Queued {}

impl PartialOrd for Queued {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Queued {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.key().total_cmp(&other.key())
    }
}

/// A B-spline's length over `[a, b]`, its pieces halved down to `width` (module docs).
/// The piece whose width halving narrows most is halved first, and the halving stops
/// where that part is no more than the piece's rounding, which halving does not narrow
/// (each piece carries its own): past that the bracket would widen. The result depends on
/// the widths alone.
fn spline_bound(c: &BSplineCurve, a: f64, b: f64, width: f64) -> Result<Bracket> {
    if c.degree()? == 0 {
        return Err(SolidError::invalid_input(OP, "a B-spline of degree 0"));
    }
    let mut spans: Vec<Span> = Vec::new();
    let mut queue = std::collections::BinaryHeap::new();
    for w in breaks(c, a, b)?.windows(2) {
        let [lo, hi] = w else { continue };
        if hi <= lo {
            continue;
        }
        let span = Span::of(c, 0.5 * (lo + hi))?;
        queue.push(Queued(spans.len(), span.piece(*lo, *hi)?));
        spans.push(span);
    }
    let mut done: Vec<Piece> = Vec::new();
    // the width of the bracket the pieces give, its sums' rounding included
    let total = |q: &std::collections::BinaryHeap<Queued>, d: &[Piece]| -> f64 {
        let pieces = q.iter().map(|x| &x.1).chain(d.iter());
        summed(pieces, q.len() + d.len()).map_or(f64::INFINITY, |b| b.upper - b.lower)
    };
    let mut pieces = queue.len();
    // (the total is summed afresh every 64 halvings: a running sum of widths that start
    // infinite cannot be kept)
    let mut halvings = 0usize;
    // whether the halving stopped because the widest piece is mostly rounding
    let mut floored = false;
    while pieces < MAX_PIECES {
        if halvings % 64 == 0 && total(&queue, &done) <= width {
            break;
        }
        let Some(Queued(s, piece)) = queue.pop() else {
            break;
        };
        // the widest piece is mostly rounding: halving it (or any other) widens the bracket
        if piece.geometric <= (piece.upper - piece.lower) - piece.geometric {
            done.push(piece);
            floored = true;
            break;
        }
        let Some(span) = spans.get(s) else {
            return Err(SolidError::invalid_input(OP, "a piece without its span"));
        };
        let m = 0.5 * (piece.a + piece.b);
        if !(m > piece.a && m < piece.b) {
            // halved to the parameter's resolution: kept as it is
            done.push(piece);
            continue;
        }
        queue.push(Queued(s, span.piece(piece.a, m)?));
        queue.push(Queued(s, span.piece(m, piece.b)?));
        pieces += 1;
        halvings += 1;
    }
    done.extend(queue.into_iter().map(|q| q.1));
    let Some(bracket) = summed(done.iter(), done.len()) else {
        return Err(SolidError::invalid_input(OP, "no piece"));
    };
    // stopped at the rounding (or every piece halved to the parameter's resolution): no
    // halving narrows it, so a narrower width is refused
    let reached = bracket.upper - bracket.lower;
    if floored || (reached > width && done.len() < MAX_PIECES) {
        return at_floor(bracket, width, done.len());
    }
    // stopped at the cap still wider than asked: refused, naming the width reached
    if reached > width {
        return Err(Failure::new(
            OP,
            CurveFailure::LengthPastPieceCap {
                asked: width,
                reached,
                pieces: done.len(),
            },
        )
        .into_error());
    }
    Ok(bracket)
}

/// The bracket the `n` pieces give: their sides summed, each sum to `gamma_(n+1)` of itself.
fn summed<'p>(pieces: impl Iterator<Item = &'p Piece>, n: usize) -> Option<Bracket> {
    let (mut lower, mut upper) = (0.0_f64, 0.0_f64);
    for p in pieces {
        lower += p.lower;
        upper += p.upper;
    }
    let g = gamma(n + 1);
    (n > 0).then(|| Bracket {
        lower: (lower * (1.0 - g)).max(0.0),
        upper: upper * (1.0 + g),
    })
}

/// `bracket`, the narrowest the `pieces` it is summed over prove (each piece's derived
/// rounding bound and the sums'), or the refusal naming it when `asked` is narrower.
fn at_floor(bracket: Bracket, asked: f64, pieces: usize) -> Result<Bracket> {
    let floor = bracket.upper - bracket.lower;
    if floor <= asked {
        return Ok(bracket);
    }
    Err(Failure::new(
        OP,
        CurveFailure::LengthBelowFloor {
            asked,
            floor,
            pieces,
        },
    )
    .into_error())
}
