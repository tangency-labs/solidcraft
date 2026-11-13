//! A B-spline curve's speed `|C'|` on one knot span, as an integrand (not upstream).
//!
//! [`BSplineCurve::span_speed`] evaluates the speed in difference form,
//! `C' = sum_i R'_i (P_i - P_0)` (the `R'_i` sum to zero, so this is `C'` exactly), so that
//! its rounding scales with the span's own size `max |P_i - P_0|`, not with its distance
//! from the origin, and carries the bound on that rounding
//! ([`SpanSpeed::rounding_bound`]), which a length integral's step control reads
//! ([`crate::mass::integrator::OdeOptions::rounding`]). A span whose control points are one
//! point has no speed to evaluate: its length is 0, exactly.

use super::BSplineCurve;
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::mass::integrator::gamma;
use crate::nurbs::math::{CPoint, NOW, NOZ};

const OP: &str = "nurbs::b_spline_curve::speed";

/// The speed of a B-spline curve on one knot span `[t_j, t_(j+1)]`, of degree `p`, in
/// difference form ([`BSplineCurve::span_speed`]).
#[derive(Debug, Clone)]
pub struct SpanSpeed {
    /// The knots `t_(j-p+1) ..= t_(j+p)` (local index `k` is `t_(j-p+1+k)`).
    knots: Vec<f64>,
    degree: usize,
    /// The span's ends `t_j`, `t_(j+1)`.
    lo: f64,
    hi: f64,
    /// The control vectors of the derivative, `q_s = p (g_(s+1) - g_s) / (t_(j+1+s) -
    /// t_(j-p+1+s))` for `s = 0 .. p`: `g_s = P_s - P_0` (polynomial, formed as `P_(s+1) -
    /// P_s`), or the homogeneous `w_s (P_s - P_0)` (rational). Local index `s` is the
    /// span's control point `j-p+s`.
    q: Vec<[f64; 3]>,
    rational: Option<Rational>,
    rounding: f64,
    /// The span's first control point `P_0`, Euclidean.
    first: [f64; 3],
}

/// The rational part of a [`SpanSpeed`].
#[derive(Debug, Clone)]
struct Rational {
    /// The homogeneous translated points `g_s = w_s (P_s - P_0)`.
    g: Vec<[f64; 3]>,
    /// The weights `w_s`.
    w: Vec<f64>,
    /// The weights' derivative control values, `p (w_(s+1) - w_s) / (t_(j+1+s) -
    /// t_(j-p+1+s))`.
    qw: Vec<f64>,
}

impl BSplineCurve {
    /// The speed of the curve on the knot span `[t_j, t_(j+1)]` holding the middle of
    /// `interval` (which lies within one knot span), or `None` when the curve is a
    /// single point there: all of that span's control points are the same Euclidean
    /// point, exactly. For a rational span (homogeneous `(x w, y w, z w, w)`) that is
    /// `h_i w_0 = h_0 w_i` for every coordinate, compared as exact products (each `a b`
    /// as `fl(a b)` plus its exact remainder, which is unique for a real value): every
    /// weight then multiplies the same point, and `C = sum R_i P_i` is that point
    /// whatever the weights.
    ///
    /// # Errors
    ///
    /// `SM_ERR` without a NURB; `InvalidInput` when `interval` is outside the knot spans,
    /// for a knot span of zero length, for a span whose points are not all rational or
    /// all polynomial, or for a rational span with a weight that is not positive (the
    /// rounding bound needs `w >= w_min > 0`).
    pub fn span_speed(&self, interval: &Extent1d) -> Result<Option<SpanSpeed>> {
        let cur = self.nurb_ref()?;
        let degree = cur.degree();
        let knots = cur.knots();
        let points = cur.cpts();
        let mid = 0.5 * (interval.min() + interval.max());
        let missing = || SolidError::invalid_input(OP, "interval outside the knot spans");
        // the last knot at or below the middle, at most the last control point's index
        let j = knots
            .iter()
            .rposition(|&t| t <= mid)
            .ok_or_else(missing)?
            .min(points.len().checked_sub(1).ok_or_else(missing)?);
        let first = j.checked_sub(degree).ok_or_else(missing)?;
        let span_points = points.get(first..=j).ok_or_else(missing)?;
        let local_knots = (j + 1)
            .checked_sub(degree)
            .and_then(|a| knots.get(a..=j + degree))
            .ok_or_else(missing)?;
        let (Some(&lo), Some(&hi)) = (knots.get(j), knots.get(j + 1)) else {
            return Err(missing());
        };
        if degree == 0 || hi.partial_cmp(&lo) != Some(core::cmp::Ordering::Greater) {
            return Err(SolidError::invalid_input(OP, "degenerate knot span"));
        }
        if is_point(span_points) {
            return Ok(None);
        }
        let Some(&p0) = span_points.first() else {
            return Err(missing());
        };
        let polynomial = p0.w == NOW;
        if span_points.iter().any(|p| (p.w == NOW) != polynomial) {
            return Err(SolidError::invalid_input(
                OP,
                "rational and polynomial control points",
            ));
        }
        let p = degree as f64;
        // the derivative's knot differences t_(j+1+s) - t_(j-p+1+s), s = 0 .. p
        let deltas: Vec<f64> = local_knots
            .iter()
            .zip(local_knots.iter().skip(degree))
            .map(|(a, b)| b - a)
            .collect();
        let differences = |values: &[[f64; 3]]| -> Vec<[f64; 3]> {
            values
                .iter()
                .zip(values.iter().skip(1))
                .zip(&deltas)
                .map(|((a, b), d)| {
                    [
                        p * (b[0] - a[0]) / d,
                        p * (b[1] - a[1]) / d,
                        p * (b[2] - a[2]) / d,
                    ]
                })
                .collect()
        };
        let first = if polynomial {
            coords(p0)
        } else {
            coords(p0).map(|h| h / p0.w)
        };
        let (q, rational, size, ratio) = if polynomial {
            let euclid: Vec<[f64; 3]> = span_points.iter().map(|&c| coords(c)).collect();
            let origin = coords(p0);
            let size = euclid
                .iter()
                .map(|e| norm([e[0] - origin[0], e[1] - origin[1], e[2] - origin[2]]))
                .fold(0.0_f64, f64::max);
            (differences(&euclid), None, size, 1.0)
        } else {
            let (w_min, w_max) = span_points
                .iter()
                .fold((f64::INFINITY, 0.0_f64), |(lo, hi), c| {
                    (lo.min(c.w), hi.max(c.w))
                });
            if w_min.is_nan() || w_min <= 0.0 {
                return Err(SolidError::invalid_input(OP, "a weight is not positive"));
            }
            let h0 = coords(p0);
            // g_s = (h_s w_0 - h_0 w_s) / w_0, the difference of products to 2u relative
            let g: Vec<[f64; 3]> = span_points
                .iter()
                .map(|&c| {
                    let h = coords(c);
                    [
                        difference_of_products(h[0], p0.w, h0[0], c.w) / p0.w,
                        difference_of_products(h[1], p0.w, h0[1], c.w) / p0.w,
                        difference_of_products(h[2], p0.w, h0[2], c.w) / p0.w,
                    ]
                })
                .collect();
            let w: Vec<f64> = span_points.iter().map(|c| c.w).collect();
            let size = g
                .iter()
                .zip(&w)
                .map(|(g, w)| norm(*g) / w)
                .fold(0.0_f64, f64::max);
            let qw = w
                .iter()
                .zip(w.iter().skip(1))
                .zip(&deltas)
                .map(|((a, b), d)| p * (b - a) / d)
                .collect();
            let q = differences(&g);
            (q, Some(Rational { g, w, qw }), size, w_max / w_min)
        };
        // the bound (see `SpanSpeed::rounding_bound`)
        let (n, factor) = if polynomial {
            (
                degree.checked_mul(7).and_then(|d| d.checked_add(12)),
                2.0 * p,
            )
        } else {
            (
                degree.checked_mul(7).and_then(|d| d.checked_add(24)),
                11.0 * p * ratio,
            )
        };
        let n = n
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| SolidError::invalid_input(OP, "degree out of range"))?;
        let rounding = gamma(n) * factor * size / (hi - lo);
        if !rounding.is_finite() {
            return Err(SolidError::invalid_input(OP, "degenerate knot span"));
        }
        Ok(Some(SpanSpeed {
            knots: local_knots.to_vec(),
            degree,
            lo,
            hi,
            q,
            rational,
            rounding,
            first,
        }))
    }
}

impl SpanSpeed {
    /// The computed speed `|C'(t)|` for `t` in the span `[t_j, t_(j+1)]`, its end
    /// included: the span's own polynomial (or rational) piece, whatever lies above
    /// `t_(j+1)`.
    ///
    /// `C' = sum_s N_(s,p-1)(t) q_s` (polynomial; the basis of degree `p - 1` by the
    /// triangular recurrence, Piegl and Tiller, "The NURBS Book", 2nd ed., A2.2), and
    /// rationally `C' = (A' - w' A / w) / w` with `A = sum_s N_(s,p) g_s`,
    /// `w = sum_s N_(s,p) w_s` and `A'`, `w'` from `q_s` and the weights' differences, so
    /// that `A / w = C - P_0`.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for `t` outside the span.
    pub fn speed(&self, t: f64) -> Result<f64> {
        if !(self.lo <= t && t <= self.hi) {
            return Err(SolidError::invalid_input(
                OP,
                "parameter outside the knot span",
            ));
        }
        let (lower, full) = basis(&self.knots, self.degree, t)?;
        let derivative = weighted(&lower, &self.q);
        let Some(r) = &self.rational else {
            return Ok(norm(derivative));
        };
        let a = weighted(&full, &r.g);
        let w: f64 = full.iter().zip(&r.w).fold(0.0, |s, (n, w)| s + n * w);
        let dw: f64 = lower.iter().zip(&r.qw).fold(0.0, |s, (n, q)| s + n * q);
        let mut d = [0.0; 3];
        for ((d, da), a) in d.iter_mut().zip(derivative).zip(a) {
            *d = (da - dw * (a / w)) / w;
        }
        Ok(norm(d))
    }

    /// The span's first control point `P_0`, Euclidean (for a rational span, its
    /// homogeneous coordinates divided by its weight, rounded).
    #[must_use]
    pub const fn first_point(&self) -> [f64; 3] {
        self.first
    }

    /// `C(t) - P_0` for `t` in a rational span, its end included, in difference form:
    /// `A / w` (see [`SpanSpeed::speed`]), whose rounding scales with the span's own size
    /// `max |P_i - P_0|`, not with its distance from the origin; `None` for a polynomial
    /// span.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for `t` outside the span.
    pub fn from_first_point(&self, t: f64) -> Result<Option<[f64; 3]>> {
        if !(self.lo <= t && t <= self.hi) {
            return Err(SolidError::invalid_input(
                OP,
                "parameter outside the knot span",
            ));
        }
        let Some(r) = &self.rational else {
            return Ok(None);
        };
        let (_, full) = basis(&self.knots, self.degree, t)?;
        let a = weighted(&full, &r.g);
        let w: f64 = full.iter().zip(&r.w).fold(0.0, |s, (n, w)| s + n * w);
        Ok(Some(a.map(|a| a / w)))
    }

    /// A bound `delta` on the absolute rounding error of [`SpanSpeed::speed`]'s `C'` on
    /// the span (length `D`, degree `p`), with `M = max_i |P_i - P_0|` over its control
    /// points and `rho` the ratio of its largest to its smallest weight:
    ///
    /// `delta = gamma_(7p+12) 2p M / D` (polynomial),
    /// `delta = gamma_(7p+24) 11 p rho M / D` (rational).
    ///
    /// Counted as Higham, "Accuracy and Stability of Numerical Algorithms", 2nd ed., §3.1
    /// (`theta_a theta_b = theta_(a+b)`; a sum of `m` terms adds `m - 1`):
    /// - Basis: each level of the triangular recurrence forms sums of
    ///   `left (N / (right + left))`, every term non-negative for `t` in the span: one
    ///   rounding each for `left`, `right`, their sum, the quotient, the product and the
    ///   sum, so each computed `N_(s,r)` is within `theta_(6r)` of the exact one; the
    ///   exact ones are non-negative and sum to 1.
    /// - Polynomial: each `q_s` coordinate is within `theta_4` (the point difference, the
    ///   knot difference, the product by `p` and the quotient), and
    ///   `|q_s| <= p |P_(s+1) - P_s| / D <= 2p M / D` (every knot difference spans the
    ///   span). The product with `N_(s,p-1)` and the sum of `p` terms give
    ///   `theta_(6(p-1)+4+1+(p-1)) = theta_(7p-2)` relative to
    ///   `sum_s N_s |q_s| <= 2p M / D`.
    /// - Rational: `g_s` is within `theta_3` (Kahan's difference of products, `2u`
    ///   relative: Jeannerod, Louvet and Muller, Math. Comp. 82 (2013); then the quotient
    ///   by `w_0`), `|g_s| <= w_max M`, `sum_s N_s |g_s| <= w M` and `|A / w| <= M` (a
    ///   convex combination of the `P_s - P_0`). Then `A'` is within `gamma_(7p+1) 2p
    ///   w_max M / D`; `w'` (`|w'| <= p (w_max - w_min) / D`) within `gamma_(7p-2)` of
    ///   that; `A / w` within `2 gamma_(7p+4) M`; `w` within `gamma_(7p+1)` relative; the
    ///   last product, difference and quotient add `gamma_3` relative to `(|A'| + |w'|
    ///   |A / w|) / w <= p (3 rho - 1) M / D`. Over `w >= w_min` they sum to at most
    ///   `gamma_(7p+4) p M / D (2 rho + 2 (rho - 1) + (rho - 1) + 2 (3 rho - 1)) <=
    ///   gamma_(7p+4) 11 p rho M / D`.
    /// - The bound's own evaluation (`M`, `D`, the products) and the second-order terms
    ///   are covered by the margin of `n` (`gamma_(n+k) (1 - gamma_k) >= gamma_n`): 14
    ///   (polynomial) and 20 (rational) beyond the counts above.
    ///
    /// The norm's own roundings are relative to the sample, counted by the integrator's
    /// estimate ([`crate::mass::integrator::OdeOptions::rounding`]); the norm is
    /// 1-Lipschitz, so the computed `|C'|` is within `delta` too, up to those. The bound
    /// scales with the span's own size `M`, not with its distance from the origin.
    #[must_use]
    pub const fn rounding_bound(&self) -> f64 {
        self.rounding
    }
}

/// Whether `points` are one Euclidean point, exactly ([`BSplineCurve::span_speed`]).
fn is_point(points: &[CPoint]) -> bool {
    let Some(&p0) = points.first() else {
        return false;
    };
    points.iter().all(|&p| {
        if p.w == NOW || p0.w == NOW {
            return p.w == p0.w && coords(p) == coords(p0);
        }
        coords(p)
            .into_iter()
            .zip(coords(p0))
            .all(|(h, h0)| exact_product(h, p0.w) == exact_product(h0, p.w))
    })
}

/// The stored coordinates of a control point, a missing `z` as 0.
fn coords(p: CPoint) -> [f64; 3] {
    [p.x, p.y, if p.z == NOZ { 0.0 } else { p.z }]
}

fn norm([x, y, z]: [f64; 3]) -> f64 {
    (x * x + y * y + z * z).sqrt()
}

/// `sum_s n_s v_s`, coordinate by coordinate, in order.
fn weighted(n: &[f64], v: &[[f64; 3]]) -> [f64; 3] {
    n.iter().zip(v).fold([0.0; 3], |[a, b, c], (n, [x, y, z])| {
        [a + n * x, b + n * y, c + n * z]
    })
}

/// The B-spline basis of degrees `degree - 1` and `degree` on the span `[k_(p-1), k_p]`
/// of the local knots `k` (`p = degree`) at `t` (The NURBS Book, A2.2).
fn basis(knots: &[f64], degree: usize, t: f64) -> Result<(Vec<f64>, Vec<f64>)> {
    let knot = |i: Option<usize>| -> Result<f64> {
        i.and_then(|i| knots.get(i))
            .copied()
            .ok_or_else(|| SolidError::invalid_input(OP, "knot missing"))
    };
    let mut n = vec![1.0];
    let mut lower = n.clone();
    for r in 1..=degree {
        if r == degree {
            lower.clone_from(&n);
        }
        let mut next = vec![0.0; r + 1];
        let mut saved = 0.0;
        for (s, (&ns, slot)) in n.iter().zip(next.iter_mut()).enumerate() {
            let right = knot(degree.checked_add(s))? - t;
            let left = t - knot(degree.checked_add(s).and_then(|i| i.checked_sub(r)))?;
            let temp = ns / (right + left);
            *slot = saved + right * temp;
            saved = left * temp;
        }
        if let Some(last) = next.last_mut() {
            *last = saved;
        }
        n = next;
    }
    Ok((lower, n))
}

/// `a b - c d` by Kahan's algorithm: within `2u` relative of the exact value, barring
/// underflow (Jeannerod, Louvet and Muller, "Further analysis of Kahan's algorithm for
/// the accurate computation of 2x2 determinants", Math. Comp. 82 (2013)).
fn difference_of_products(a: f64, b: f64, c: f64, d: f64) -> f64 {
    let cd = c * d;
    let error = c.mul_add(d, -cd);
    a.mul_add(b, -cd) - error
}

/// `a b` exactly, as `fl(a b)` and the remainder `a b - fl(a b)` (exact by a fused
/// multiply-add, barring underflow).
fn exact_product(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    (p, a.mul_add(b, -p))
}
