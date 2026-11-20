//! A proved bound, over a whole span, of a variable round's skin's distance from the
//! interpolated ball family `R` through the solved sections about the span (#15256): not
//! from the round's true balls, and not a certificate of the round.
//!
//! Upstream: none (not in SMLib or SolidCraft).
//!
//! | Rust                     | Upstream |
//! |--------------------------|----------|
//! | [`BallFamily`]           | (none)   |
//! | [`BallFamily::through`]  | (none)   |
//! | [`span_bound`]           | (none)   |
//!
//! **What is bounded.** `FlushCurve` solves sections of the round exactly (to the solver's
//! convergence): the stations and the section in each span's middle, each a ball `(c, r)`
//! placed at the skin's own parameter where the skin passes the ball's touch circle.
//! [`BallFamily`] is the closed-form family `R` of balls through the solved balls about one
//! span: its two stations, its middle and, past a station where the round goes on smoothly
//! (not a corner of the radius law, not a radius of zero), the neighbouring span's middle.
//! Its centre and radius are their Lagrange interpolants (degree at most four) in the
//! centres' chord length `tau`, scaled to the span, over the nodes' range. The radius of a
//! ball is `|r(tau)|`; where the radius runs through zero, the nodes' radii take the signs
//! that make the law smoothest. The quantity bounded is `phi(x) = min_tau psi(tau; x)`,
//! `psi = |x - c(tau)| - |r(tau)|`, the signed distance from `x` to the union of `R`'s balls
//! (to their envelope where the nearest ball is inside the range), for every point
//! `x = S(u, v)` of the skin over the span `u in [t_a, t_b]` and its whole `v` range.
//! [`span_bound`] returns `B` with `|phi(S(u, v))| <= B` over that whole rectangle: no point
//! is sampled, every bound carries its rounding.
//!
//! **How.** With `F(tau; x) = |x - c|^2 - r^2 = psi (psi + 2 |r|)`:
//! - In the skin's homogeneous form about the middle ball's centre, `x = X / W` with `X`, `W`
//!   Bernstein polynomials on each Bézier piece (`crate::query::distance_bound`'s pieces,
//!   with the error they carry), and a correspondence `tau(s, v)` bilinear on the piece
//!   (through each corner's nearest ball, found by Newton's method; any correspondence
//!   serves: the bounds below hold for every `tau` in the range),
//!   `F W^2 = |X|^2 - 2 W X.c + W^2 (|c|^2 - r^2)` and `F_tau W^2` are polynomials in the
//!   piece's parameters, written in Bernstein form by products of Bernstein polynomials.
//! - The family's second derivative `F_tautau = 2 |c'|^2 - 2 (x - c).c'' - 2 r'^2 - 2 r r''`
//!   is enclosed in `[m, M]` over the whole range and the piece's box of points, on
//!   [`F2_CELLS`] cells: the part in `tau` alone as one Bernstein polynomial, `(x - c).c''`
//!   by intervals, widened for rounding. A piece with `m <= 0` is not proved (infinite).
//! - Lower side: for every `tau` in the range, `F(tau) >= F(u) + F_tau(u) (tau - u) +
//!   m (tau - u)^2 / 2 >= F(u) - F_tau(u)^2 / (2 m)` (`u` the correspondence), so
//!   `min_tau F >= (F W^2 . W^2 - (F_tau W^2)^2 / (2 m)) / W^4`, whose numerator is one
//!   polynomial: the least quotient of its Bernstein coefficients by `W^4`'s (in one
//!   degree, each with its error) bounds `min_tau F` from below on the piece (the rational
//!   convex hull property: the quotient is a mean of the coefficients' quotients with weights
//!   `w_i B_i >= 0`). Where that is negative, as `psi = sqrt(r^2 + F) - |r|` rises with `F`
//!   and, for `F < 0`, with `|r|`, `psi >= sqrt(r_lo^2 + F_min) - r_lo` (or
//!   `-sqrt(-F_min)` where `r_lo^2 + F_min < 0`), `r_lo` the least `|r|` over the `tau`
//!   where `F` can be negative (within `(|F_tau| + sqrt(F_tau^2 + 2 m |F|)) / m` of `u`).
//! - Upper side: with `tau* = u - F_tau(u) / M` inside the range,
//!   `F(tau*) <= F(u) - F_tau(u)^2 / (2 M)` (Taylor with `F_tautau <= M`), the same
//!   numerator with `M`; its largest coefficient bounds `F` at a ball of the family, and
//!   `psi <= sqrt(r_lo^2 + F) - r_lo` there (`psi` falls with `|r|` for `F > 0`); `F(u)`
//!   itself is the other candidate. At the correspondence also `psi <= max(F / (2 |r|), 0)`
//!   (for `F >= 0`, `|x - c| >= |r|`), as a quotient of polynomials where `r` is proved
//!   positive.
//! - A piece whose bound exceeds the tolerance is quartered (de Casteljau, with its error),
//!   or halved in `u` where the radius comes within the bound of zero on it (the skin closes
//!   to a point there), at most [`MAX_PIECES`] pieces per span. Every piece's bound is a
//!   bound, so the largest over the pieces left is returned, whatever stopped the
//!   subdivision.
//!
//! **The coefficients' error** (Higham, *Accuracy and Stability of Numerical Algorithms*, 2nd
//! ed., section 3.1, `gamma_n = n u / (1 - n u)`), one absolute bound per polynomial:
//! - the skin's pieces: their own, from `pieces_about` (the form's, the extraction's and the
//!   translation's), and each quartering's;
//! - the family's polynomials written on a piece or a cell: Horner's rule on Bernstein
//!   polynomials, each step a product and a sum as below;
//! - a product of Bernstein polynomials: each coefficient is a sum of at most
//!   `N = (min du + 1)(min dv + 1)` terms, each two coefficients scaled by four exact
//!   binomials, multiplied, and divided by a product of two, so within `gamma_(N + 8)` of the
//!   product of the two largest coefficients (the binomial weights sum to one, Vandermonde);
//!   the inputs' errors `ea`, `eb` add `ea |b| + |a| eb + ea eb`;
//! - a degree elevation is a product with ones; a sum `a + x b`, `gamma_2 (|a| + |x| |b|)`;
//! - the final quotients and roots, `gamma_8` of the bound.
//!
//! **What is not bounded** (ARGUED, said where it is used): that `R` is the true family of the
//! round's balls between its solved sections (it is their interpolant, exact at them), and
//! that the nearest true ball of each skin point lies inside the range. So a span kept on
//! this bound is not marked certified. A peak or kink of the radius law strictly inside a
//! span, narrower than about a quarter of it, is not in `R` (radius corners are found only
//! at stations). Enclosing the true family, by interval Newton on the section equations, is
//! #16446; the round as its certified definition is #15657. The interpolant's
//! coefficients define `R`: their own rounding is no error of the bound.
//!
//! Deviations: not upstream. No recursion: the pieces wait on a work list, at most
//! [`MAX_PIECES`] per span.

use crate::geom::certified::above;
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::math::{Point3, EFF_ZERO};
use crate::nurbs::math::gamma;
use crate::offset::construction::bernstein::{Bernstein2, Binomials};
use crate::query::distance_bound::{pieces_about, Piece};
use crate::surface::surface::Surface;
use crate::{Result, SolidError};

const OP: &str = "blend::fillet_intersector::skin_bound::span_bound";

/// The cells of the family's range on which its second derivative is enclosed.
const F2_CELLS: usize = 8;

/// The most pieces one span's bound subdivides its skin into.
pub(super) const MAX_PIECES: usize = 256;

/// A solved ball of the round: its centre, its radius and the skin's parameter where the
/// skin passes its touch circle.
#[derive(Debug, Clone, Copy)]
pub(super) struct Ball {
    /// The centre.
    pub(super) c: Point3,
    /// The radius.
    pub(super) r: f64,
    /// The skin's parameter.
    pub(super) at: f64,
}

/// A polynomial in `tau`, by its monomial coefficients (lowest first).
type Mono = Vec<f64>;

/// The closed-form family of balls through the solved balls about a span (its stations, its
/// middle and the neighbouring spans' middles where the round goes on smoothly past a
/// station): centre (about the middle ball's centre) and radius their Lagrange interpolants
/// in `tau = (t - t_m) / h` (degree at most four), over the nodes' range. The balls' radius
/// is `|r(tau)|` (`F` holds only `r^2`).
#[derive(Debug, Clone)]
pub(super) struct BallFamily {
    origin: Point3,
    /// Each node's skin parameter and family parameter, increasing: the start of the
    /// correspondence (piecewise linear between them).
    knots: Vec<(f64, f64)>,
    c: [Mono; 3],
    r: Mono,
    range: [f64; 2],
}

/// The Hermite interpolant through `(tau_i, y_i)` with slope `y'_i` where given (Newton's
/// divided differences on repeated nodes), in monomial form. It defines the family: its
/// rounding is not an error of anything.
fn hermite(data: &[(f64, f64, Option<f64>)]) -> Mono {
    let mut z = Vec::new();
    let mut y = Vec::new();
    let mut slope_at = Vec::new();
    for &(t, v, d) in data {
        z.push(t);
        y.push(v);
        slope_at.push(None);
        if let Some(d) = d {
            z.push(t);
            y.push(v);
            slope_at.push(Some(d));
        }
    }
    // the divided-difference table, column by column, keeping each column's head
    let mut heads = Vec::new();
    let mut column = y;
    let mut order = 0;
    while let Some(&head) = column.first() {
        heads.push(head);
        order += 1;
        let mut next = Vec::new();
        for (i, w) in column.windows(2).enumerate() {
            let (Some(&zi), Some(&zj)) = (z.get(i), z.get(i + order)) else {
                continue;
            };
            let [a, b] = [
                w.first().copied().unwrap_or(0.0),
                w.get(1).copied().unwrap_or(0.0),
            ];
            next.push(if zj == zi {
                slope_at.get(i + 1).copied().flatten().unwrap_or(0.0)
            } else {
                (b - a) / (zj - zi)
            });
        }
        column = next;
    }
    // Newton form to monomials: p = h0 + (t - z0)(h1 + (t - z1)(h2 + ...))
    let mut mono: Mono = Vec::new();
    for (k, &h) in heads.iter().enumerate().rev() {
        // mono = mono * (t - z_k) + h
        let zk = z.get(k).copied().unwrap_or(0.0);
        let mut next = vec![0.0; mono.len() + 1];
        for (i, &m) in mono.iter().enumerate() {
            if let Some(slot) = next.get_mut(i + 1) {
                *slot += m;
            }
            if let Some(slot) = next.get_mut(i) {
                *slot -= m * zk;
            }
        }
        if let Some(slot) = next.first_mut() {
            *slot += h;
        }
        mono = next;
    }
    mono
}

/// The signs of the nodes' radii (the first positive) whose second divided differences have
/// the least sum of squares: `|r|` folds where the law runs through zero, and the family is
/// fitted to the unfolded law. It only chooses the family, which the bounds do not trust.
fn smooth_signs(nodes: &[Ball]) -> Vec<f64> {
    let n = nodes.len();
    let mut best = (f64::INFINITY, vec![1.0; n]);
    if !(3..=8).contains(&n) {
        return best.1;
    }
    for mask in 0..(1_usize << (n - 1)) {
        let signs: Vec<f64> = (0..n)
            .map(|i| {
                if i > 0 && mask & (1 << (i - 1)) != 0 {
                    -1.0
                } else {
                    1.0
                }
            })
            .collect();
        let mut cost = 0.0;
        for (w, s) in nodes.windows(3).zip(signs.windows(3)) {
            if let ([a, b, c], [sa, sb, sc]) = (w, s) {
                let d1 = (sb * b.r - sa * a.r) / (b.at - a.at);
                let d2 = (sc * c.r - sb * b.r) / (c.at - b.at);
                let dd = (d2 - d1) / (c.at - a.at);
                cost += dd * dd;
            }
        }
        if cost < best.0 {
            best = (cost, signs);
        }
    }
    best.1
}

/// The value at the skin parameter `t` of the piecewise linear map through `(nodes[i].at,
/// values[i])` (extended by its end pieces).
fn interpolate(nodes: &[Ball], values: &[f64], t: f64) -> f64 {
    let pairs: Vec<(f64, f64)> = nodes
        .iter()
        .map(|q| q.at)
        .zip(values.iter().copied())
        .collect();
    linear_through(&pairs, t)
}

/// The piecewise linear map through `pairs` (increasing in their first) at `t`, extended
/// by its end pieces.
fn linear_through(pairs: &[(f64, f64)], t: f64) -> f64 {
    let n = pairs.len();
    let i = pairs
        .iter()
        .position(|&(x, _)| x > t)
        .unwrap_or(n)
        .clamp(1, n.saturating_sub(1).max(1));
    match (pairs.get(i - 1), pairs.get(i)) {
        (Some(&(x0, y0)), Some(&(x1, y1))) if x1 > x0 => y0 + (t - x0) * (y1 - y0) / (x1 - x0),
        (Some(&(_, y0)), _) => y0,
        _ => f64::NAN,
    }
}

/// The value of `p` at `t` (Horner).
fn eval(p: &[f64], t: f64) -> f64 {
    p.iter().rev().fold(0.0, |acc, &a| acc * t + a)
}

/// The derivative of `p`.
fn deriv(p: &[f64]) -> Mono {
    p.iter()
        .enumerate()
        .skip(1)
        .map(|(i, &a)| {
            #[allow(clippy::cast_precision_loss)]
            let k = i as f64;
            k * a
        })
        .collect()
}

/// The least `|x|` for `x` in `[lo, hi]` (NaN for a range that is not one).
fn least_size((lo, hi): (f64, f64)) -> f64 {
    if lo <= 0.0 && hi >= 0.0 {
        0.0
    } else if lo > 0.0 {
        lo
    } else if hi < 0.0 {
        -hi
    } else {
        f64::NAN
    }
}

/// The range of `a b` for `a`, `b` in their ranges.
fn product_range((a0, a1): (f64, f64), (b0, b1): (f64, f64)) -> (f64, f64) {
    let p = [a0 * b0, a0 * b1, a1 * b0, a1 * b1];
    (
        p.iter().copied().fold(f64::INFINITY, f64::min),
        p.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    )
}

/// A Bernstein polynomial and a bound on each coefficient's error.
#[derive(Debug, Clone)]
struct Poly {
    p: Bernstein2,
    e: f64,
}

impl Poly {
    fn largest(&self) -> f64 {
        self.p.max_abs_coefficient()
    }

    fn lo(&self) -> f64 {
        self.p.min_coefficient() - self.e
    }

    fn hi(&self) -> f64 {
        self.p.max_coefficient() + self.e
    }

    fn mul(&self, o: &Self, bi: &Binomials) -> Result<Self> {
        let ((au, av), (bu, bv)) = (self.p.degrees(), o.p.degrees());
        let terms = (au.min(bu) + 1) * (av.min(bv) + 1);
        let (ma, mb) = (self.largest(), o.largest());
        let e = gamma(terms + 8) * ma * mb + self.e * mb + ma * o.e + self.e * o.e;
        Ok(Self {
            p: self.p.mul(&o.p, bi)?,
            e,
        })
    }

    fn elevate(&self, du: usize, dv: usize, bi: &Binomials) -> Result<Self> {
        let (su, sv) = self.p.degrees();
        if (su, sv) == (du, dv) {
            return Ok(self.clone());
        }
        let terms = (su.min(du.saturating_sub(su)) + 1) * (sv.min(dv.saturating_sub(sv)) + 1);
        Ok(Self {
            p: self.p.elevate(du, dv, bi)?,
            e: self.e + gamma(terms + 8) * self.largest(),
        })
    }

    /// `self + x o`.
    fn add_scaled(&self, o: &Self, x: f64, bi: &Binomials) -> Result<Self> {
        let ((au, av), (bu, bv)) = (self.p.degrees(), o.p.degrees());
        let (du, dv) = (au.max(bu), av.max(bv));
        let a = self.elevate(du, dv, bi)?;
        let b = o.elevate(du, dv, bi)?;
        let e = a.e + x.abs() * b.e + gamma(2) * (a.largest() + x.abs() * b.largest());
        Ok(Self {
            p: a.p.add_scaled(&b.p, x, bi)?,
            e,
        })
    }
}

/// The polynomial `p` of the correspondence `tau` (a polynomial on the piece), by Horner's
/// rule on Bernstein polynomials, with its error.
fn mono_of(p: &[f64], tau: &Poly, bi: &Binomials) -> Result<Poly> {
    let constant = |x: f64| Poly {
        p: Bernstein2::constant(x),
        e: 0.0,
    };
    let mut acc = constant(0.0);
    for &a in p.iter().rev() {
        acc = constant(a).add_scaled(&acc.mul(tau, bi)?, 1.0, bi)?;
    }
    Ok(acc)
}

/// The range of `p` over `[lo, hi]`: the hull of its Bernstein coefficients there, with
/// their error (`None` where not finite).
fn mono_range(p: &[f64], lo: f64, hi: f64, bi: &Binomials) -> Result<Option<(f64, f64)>> {
    let tau = Poly {
        p: Bernstein2::new(1, 0, vec![lo, hi])?,
        e: 0.0,
    };
    let q = mono_of(p, &tau, bi)?;
    let (a, b) = (q.lo(), q.hi());
    Ok((a.is_finite() && b.is_finite()).then_some((a, b)))
}

/// The range of the quotient `n / d` of two polynomials over the square, `d` positive: with
/// both written in one degree, `n / d = sum n_i B_i / sum d_i B_i` is a mean of the
/// coefficients' quotients `n_i / d_i` weighted by `d_i B_i >= 0` (the rational convex hull
/// property), each taken with its coefficients' errors and rounded outward; `None` where a
/// coefficient of `d` is not proved positive.
fn quotient_range(n: &Poly, d: &Poly, bi: &Binomials) -> Result<Option<(f64, f64)>> {
    let ((nu, nv), (du, dv)) = (n.p.degrees(), d.p.degrees());
    let (u, v) = (nu.max(du), nv.max(dv));
    let (n, d) = (n.elevate(u, v, bi)?, d.elevate(u, v, bi)?);
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for (&a, &b) in n.p.coefficients().iter().zip(d.p.coefficients()) {
        let (b_lo, b_hi) = (b - d.e, b + d.e);
        if !(b_lo > 0.0 && b_hi.is_finite()) {
            return Ok(None);
        }
        let (a_lo, a_hi) = (a - n.e, a + n.e);
        // (`min`/`max` drop a NaN: a coefficient that is not finite proves nothing)
        if !(a_lo.is_finite() && a_hi.is_finite()) {
            return Ok(None);
        }
        let q_lo = if a_lo >= 0.0 {
            a_lo / b_hi
        } else {
            a_lo / b_lo
        };
        let q_hi = if a_hi >= 0.0 {
            a_hi / b_lo
        } else {
            a_hi / b_hi
        };
        lo = lo.min(q_lo - gamma(1) * q_lo.abs());
        hi = hi.max(q_hi + gamma(1) * q_hi.abs());
    }
    Ok((lo.is_finite() && hi.is_finite()).then_some((lo, hi)))
}

impl BallFamily {
    /// The family through `nodes` (distinct skin parameters, increasing), about the span
    /// `span` whose middle ball is at `t_m`, over the nodes' range; `None` for fewer than
    /// three nodes, nodes not finite or not increasing, or a span the range does not hold.
    pub(super) fn through(nodes: &[Ball], t_m: f64, span: [f64; 2]) -> Option<Self> {
        let [a, b] = span;
        let h = b - a;
        let finite = nodes.iter().all(|q| {
            q.c.x.is_finite()
                && q.c.y.is_finite()
                && q.c.z.is_finite()
                && q.r.is_finite()
                && q.at.is_finite()
        });
        let increasing = nodes.windows(2).all(|w| match w {
            [p, q] => p.at < q.at,
            _ => true,
        });
        let middle = nodes.iter().find(|q| q.at == t_m)?;
        let (first, last) = (nodes.first()?, nodes.last()?);
        if !(nodes.len() >= 3
            && finite
            && increasing
            && h > 0.0
            && h.is_finite()
            && first.at <= a
            && b <= last.at)
        {
            return None;
        }
        let o = middle.c;
        // the family's parameter: the centres' chord length, so that the centre moves at
        // about one speed and F is convex in it (the skin's parameter runs unevenly along
        // the centres); the skin's parameter where the centres do not move apart
        let mut sigma = Vec::with_capacity(nodes.len());
        let mut run = 0.0;
        for (i, q) in nodes.iter().enumerate() {
            if let Some(p) = i.checked_sub(1).and_then(|p| nodes.get(p)) {
                run += (q.c - p.c).length();
            }
            sigma.push(run);
        }
        let at_of = |t: f64| {
            nodes
                .iter()
                .zip(&sigma)
                .find(|(q, _)| q.at == t)
                .map(|(_, s)| *s)
        };
        let (sa, sb, sm) = (
            interpolate(nodes, &sigma, a),
            interpolate(nodes, &sigma, b),
            at_of(t_m)?,
        );
        let chord = sb - sa;
        let by_chord = chord > EFF_ZERO * (1.0 + run)
            && sigma.windows(2).all(|w| match w {
                [p, q] => p < q,
                _ => true,
            });
        let tau_of: Vec<f64> = if by_chord {
            sigma.iter().map(|s| (s - sm) / chord).collect()
        } else {
            nodes.iter().map(|q| (q.at - t_m) / h).collect()
        };
        let fit = |pick: &dyn Fn(&Ball) -> f64| {
            let data: Vec<(f64, f64, Option<f64>)> = nodes
                .iter()
                .zip(&tau_of)
                .map(|(q, t)| (*t, pick(q), None))
                .collect();
            hermite(&data)
        };
        let c = [
            fit(&|q| q.c.x - o.x),
            fit(&|q| q.c.y - o.y),
            fit(&|q| q.c.z - o.z),
        ];
        // (the balls' radius is |r|: where it runs through zero between the nodes, the
        // signs that make the radii's divided differences smoothest give the smooth law)
        let signs = smooth_signs(nodes);
        let signed: Vec<(f64, f64, Option<f64>)> = nodes
            .iter()
            .zip(&signs)
            .zip(&tau_of)
            .map(|((q, s), t)| (*t, s * q.r, None))
            .collect();
        let r = hermite(&signed);
        let coefficients_finite = c.iter().chain([&r]).flatten().all(|g| g.is_finite());
        let (Some(&lo), Some(&hi)) = (tau_of.first(), tau_of.last()) else {
            return None;
        };
        // (every `clamp(lo, hi)` below relies on this: neither NaN nor crossed)
        if !above(hi, lo) {
            return None;
        }
        coefficients_finite.then_some(Self {
            origin: o,
            knots: nodes
                .iter()
                .map(|q| q.at)
                .zip(tau_of.iter().copied())
                .collect(),
            c,
            r,
            range: [lo, hi],
        })
    }

    /// The range `[m, M]` of `F_tautau = 2 |c'|^2 - 2 (x - c).c'' - 2 r'^2 - 2 r r''` over
    /// `tau` in `[lo, hi]` and the box of points `[box_lo, box_hi]` (about the origin), on
    /// [`F2_CELLS`] cells of the range: on each, the part of `tau` alone as one Bernstein
    /// polynomial with its error, and `(x - c).c''` by intervals, widened for their rounding.
    /// `(-inf, inf)` where a range is not finite.
    fn second_derivative(
        &self,
        [lo, hi]: [f64; 2],
        box_lo: [f64; 3],
        box_hi: [f64; 3],
        bi: &Binomials,
    ) -> Result<(f64, f64)> {
        let (mut m, mut big_m) = (f64::INFINITY, f64::NEG_INFINITY);
        let unknown = Ok((f64::NEG_INFINITY, f64::INFINITY));
        #[allow(clippy::cast_precision_loss)]
        let cells = F2_CELLS as f64;
        for i in 0..F2_CELLS {
            #[allow(clippy::cast_precision_loss)]
            let (a, b) = (
                lo + (hi - lo) * (i as f64 / cells),
                // (the last cell ends at `hi` itself, not a rounding short of it)
                if i + 1 == F2_CELLS {
                    hi
                } else {
                    lo + (hi - lo) * ((i + 1) as f64 / cells)
                },
            );
            let tau = Poly {
                p: Bernstein2::new(1, 0, vec![a, b])?,
                e: 0.0,
            };
            let dr = deriv(&self.r);
            let r = mono_of(&self.r, &tau, bi)?;
            let rs = mono_of(&dr, &tau, bi)?;
            let rss = mono_of(&deriv(&dr), &tau, bi)?;
            let mut g = rs.mul(&rs, bi)?.add_scaled(&r.mul(&rss, bi)?, 1.0, bi)?;
            g = Poly {
                p: g.p.scale(-2.0),
                e: 2.0 * g.e,
            };
            let (mut dot_lo, mut dot_hi, mut mag) = (0.0_f64, 0.0_f64, 0.0_f64);
            for ((c, xl), xh) in self.c.iter().zip(box_lo).zip(box_hi) {
                let d = deriv(c);
                let cs = mono_of(&d, &tau, bi)?;
                g = g.add_scaled(&cs.mul(&cs, bi)?, 2.0, bi)?;
                let (Some((cl, ch)), Some(curve)) =
                    (mono_range(c, a, b, bi)?, mono_range(&deriv(&d), a, b, bi)?)
                else {
                    return unknown;
                };
                let off = product_range((xl - ch, xh - cl), curve);
                dot_lo += off.0;
                dot_hi += off.1;
                mag += off.0.abs().max(off.1.abs());
            }
            let slack = gamma(8) * 2.0 * (mag + g.largest());
            m = m.min(g.lo() - 2.0 * dot_hi - slack);
            big_m = big_m.max(g.hi() - 2.0 * dot_lo + slack);
        }
        if m.is_nan() || big_m.is_nan() {
            return unknown;
        }
        Ok((m, big_m))
    }

    /// Whether the family's radius comes within `bound` of zero over the piece (by the start
    /// of its correspondence): it only steers how the piece is cut.
    fn radius_vanishes_on(&self, piece: &Piece, bound: f64, bi: &Binomials) -> Result<bool> {
        let [lo, hi] = self.range;
        let [u0, u1, _, _] = piece.uv;
        let (a, b) = (
            self.start_at(u0).clamp(lo, hi),
            self.start_at(u1).clamp(lo, hi),
        );
        Ok(mono_range(&self.r, a.min(b), a.max(b), bi)?
            .is_some_and(|range| !above(least_size(range), bound)))
    }

    /// The family parameter the correspondence starts from at the skin parameter `u`.
    fn start_at(&self, u: f64) -> f64 {
        linear_through(&self.knots, u)
    }

    /// The family's parameter nearest the point `p` (about the origin) by `F`: Newton's
    /// method on `F_tau = 0` from `start`, kept inside the range; `start` where it does not
    /// settle. It only chooses the correspondence, which the bounds do not trust.
    fn nearest(&self, p: [f64; 3], start: f64) -> f64 {
        let [lo, hi] = self.range;
        let dc: Vec<Mono> = self.c.iter().map(|g| deriv(g)).collect();
        let ddc: Vec<Mono> = dc.iter().map(|g| deriv(g)).collect();
        let (dr, ddr) = (deriv(&self.r), deriv(&deriv(&self.r)));
        let mut t = start;
        for _ in 0..12 {
            let (mut f1, mut f2) = (0.0, 0.0);
            for (((g, d), dd), x) in self.c.iter().zip(&dc).zip(&ddc).zip(p) {
                let off = x - eval(g, t);
                let slope = eval(d, t);
                f1 -= 2.0 * off * slope;
                f2 += 2.0 * (slope * slope - off * eval(dd, t));
            }
            let (r, rs, rss) = (eval(&self.r, t), eval(&dr, t), eval(&ddr, t));
            f1 -= 2.0 * r * rs;
            f2 -= 2.0 * (rs * rs + r * rss);
            let next = (t - f1 / f2).clamp(lo, hi);
            if !next.is_finite() || !above(f2, 0.0) {
                return start;
            }
            t = next;
        }
        t
    }

    /// The bound of `|phi|` over one piece of the skin (module docs); infinite where it is
    /// not proved (a second derivative not proved positive, weights not proved positive, a
    /// radius not proved positive where it is used, or a value that is not finite).
    #[allow(clippy::too_many_lines)]
    fn piece_bound(&self, piece: &Piece, bi: &Binomials) -> Result<f64> {
        let [lo, hi] = self.range;
        let [x, y, z, w] = &piece.h;
        // the correspondence: bilinear in the piece's parameters through each corner's
        // nearest ball of the family (found by Newton's method, which only steers: any
        // correspondence inside the range serves, the bounds hold for every tau in it)
        let [u0, u1, _, _] = piece.uv;
        let corner_u = [u0, u0, u1, u1];
        let mut corners = [0.0; 4];
        let (du, dv) = w.degrees();
        let at = |p: &Bernstein2, i: usize, j: usize| {
            p.coefficients()
                .get(i * (dv + 1) + j)
                .copied()
                .unwrap_or(f64::NAN)
        };
        for ((slot, (i, j)), u) in corners
            .iter_mut()
            .zip([(0, 0), (0, dv), (du, 0), (du, dv)])
            .zip(corner_u)
        {
            let wc = at(w, i, j);
            let p = [at(x, i, j) / wc, at(y, i, j) / wc, at(z, i, j) / wc];
            *slot = self.nearest(p, self.start_at(u).clamp(lo, hi));
        }
        let [t00, t01, t10, t11] = corners;
        let tau = Poly {
            p: Bernstein2::new(1, 1, vec![t00, t01, t10, t11])?,
            e: 0.0,
        };
        let (tau0, tau1) = (
            corners.iter().copied().fold(f64::INFINITY, f64::min),
            corners.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        );
        let homog = |p: &Bernstein2| Poly {
            p: p.clone(),
            e: piece.err,
        };
        let (xs, w) = ([homog(x), homog(y), homog(z)], homog(w));
        if !(w.lo() > 0.0 && w.hi().is_finite()) {
            return Ok(f64::INFINITY);
        }

        // F W^2 and F_tau W^2 along the piece
        let [cx, cy, cz] = &self.c;
        let (dcx, dcy, dcz) = (deriv(cx), deriv(cy), deriv(cz));
        let cs = [
            mono_of(cx, &tau, bi)?,
            mono_of(cy, &tau, bi)?,
            mono_of(cz, &tau, bi)?,
        ];
        let ds = [
            mono_of(&dcx, &tau, bi)?,
            mono_of(&dcy, &tau, bi)?,
            mono_of(&dcz, &tau, bi)?,
        ];
        let rr = mono_of(&self.r, &tau, bi)?;
        let dr = mono_of(&deriv(&self.r), &tau, bi)?;
        // (scaling by -1 and -2 is exact)
        let r2 = rr.mul(&rr, bi)?;
        let mut q = Poly {
            p: r2.p.scale(-1.0),
            e: r2.e,
        };
        let rdr = rr.mul(&dr, bi)?;
        let mut dq = Poly {
            p: rdr.p.scale(-2.0),
            e: 2.0 * rdr.e,
        };
        let mut a = Poly {
            p: Bernstein2::constant(0.0),
            e: 0.0,
        };
        let mut wx_c = a.clone();
        let mut wx_d = a.clone();
        for ((xk, ck), dk) in xs.iter().zip(&cs).zip(&ds) {
            a = a.add_scaled(&xk.mul(xk, bi)?, 1.0, bi)?;
            q = q.add_scaled(&ck.mul(ck, bi)?, 1.0, bi)?;
            dq = dq.add_scaled(&ck.mul(dk, bi)?, 2.0, bi)?;
            let b = w.mul(xk, bi)?;
            wx_c = wx_c.add_scaled(&b.mul(ck, bi)?, 1.0, bi)?;
            wx_d = wx_d.add_scaled(&b.mul(dk, bi)?, 1.0, bi)?;
        }
        let c2 = w.mul(&w, bi)?;
        let fw = a
            .add_scaled(&wx_c, -2.0, bi)?
            .add_scaled(&c2.mul(&q, bi)?, 1.0, bi)?;
        let ftw = c2.mul(&dq, bi)?.add_scaled(&wx_d, -2.0, bi)?;

        // F and |F_tau| at the correspondence
        let w4 = c2.mul(&c2, bi)?;
        let (Some((f_lo, f_hi)), Some((ft_lo, ft_hi))) = (
            quotient_range(&fw, &c2, bi)?,
            quotient_range(&ftw, &c2, bi)?,
        ) else {
            return Ok(f64::INFINITY);
        };
        let ft_abs = ft_lo.abs().max(ft_hi.abs());
        let f_abs = f_lo.abs().max(f_hi.abs());

        // F_tautau over the range and the piece's box of points: `G = 2 |c'|^2 - 2 r'^2 -
        // 2 r r''` (of tau alone) as one Bernstein polynomial and `2 (x - c).c''` as an
        // interval, on each of `F2_CELLS` cells of the range
        let (box_lo, box_hi) = piece.widened_box()?;
        let (m, big_m) = self.second_derivative([lo, hi], box_lo, box_hi, bi)?;
        if !(m > 0.0 && big_m.is_finite()) {
            return Ok(f64::INFINITY);
        }

        // (F W^2) W^2 - (F_tau W^2)^2 x, for x = 1 / (2 kappa)
        let p1 = fw.mul(&c2, bi)?;
        let p2 = ftw.mul(&ftw, bi)?;
        // (rounded so that the kappa it stands for is at most m, at least M)
        let x_lo = (1.0 / (2.0 * m)) * (1.0 + gamma(3));
        let x_hi = (1.0 / (2.0 * big_m)) * (1.0 - gamma(3));
        let n_lo = p1.add_scaled(&p2, -x_lo, bi)?;
        let n_hi = p1.add_scaled(&p2, -x_hi, bi)?;

        // the lower side: min over the range of F, then of psi
        let Some((g_lo, _)) = quotient_range(&n_lo, &w4, bi)? else {
            return Ok(f64::INFINITY);
        };
        let phi_lo = if g_lo >= 0.0 {
            0.0
        } else {
            // (where F can be negative: within reach of the correspondence)
            let reach =
                (ft_abs + (ft_abs * ft_abs + 2.0 * m * f_abs).sqrt()) / m * (1.0 + gamma(8));
            let span = ((tau0 - reach).max(lo), (tau1 + reach).min(hi));
            let r_lo = mono_range(&self.r, span.0, span.1, bi)?.map_or(f64::NAN, least_size);
            // psi = sqrt(r^2 + F) - r rises with F and, for F < 0, with r
            let d = r_lo * r_lo + g_lo;
            if d >= 0.0 {
                g_lo / (d.sqrt() + r_lo)
            } else {
                -(-g_lo).sqrt()
            }
        };

        // the upper side: F at the correspondence, or at the Taylor step where it fits
        let step = ft_abs / big_m * (1.0 + gamma(4));
        let fits = tau0 - step >= lo && tau1 + step <= hi;
        let (f_up, r_span) = if fits {
            let Some((_, g_hi)) = quotient_range(&n_hi, &w4, bi)? else {
                return Ok(f64::INFINITY);
            };
            (f_hi.min(g_hi), (tau0 - step, tau1 + step))
        } else {
            (f_hi, (tau0, tau1))
        };
        let phi_hi = if f_up <= 0.0 {
            0.0
        } else {
            let r_lo = mono_range(&self.r, r_span.0, r_span.1, bi)?.map_or(f64::NAN, least_size);
            // psi = sqrt(r^2 + F) - r rises with F and, for F > 0, falls with r
            let by_root = f_up / ((r_lo * r_lo + f_up).sqrt() + r_lo);
            // and at the correspondence, psi = F / (rho + r) <= F / (2 r) where F >= 0 (so
            // rho >= r), psi < 0 elsewhere: max(F / (2 r), 0) bounds it, as a quotient of
            // polynomials where r is proved positive (where the radius runs to zero)
            let den = rr.mul(&c2, bi)?;
            let den = Poly {
                p: den.p.scale(2.0),
                e: 2.0 * den.e,
            };
            // (each candidate is a bound by itself; one that is NaN proves nothing, and
            // `min` must not let it stand in for the other)
            let by_root = not_proved(by_root);
            match quotient_range(&fw, &den, bi)? {
                Some((_, q)) => by_root.min(not_proved(q.max(0.0))),
                None => by_root,
            }
        };

        Ok(combine(phi_lo, phi_hi))
    }
}

/// `x`, or infinity for a NaN (a bound that is not proved).
fn not_proved(x: f64) -> f64 {
    if x.is_nan() {
        f64::INFINITY
    } else {
        x
    }
}

/// The bound of `|phi|` from its lower side `phi_lo` and upper side `phi_hi`, widened for
/// the final quotients and roots: infinite where either side is NaN or infinite (`f64::max`
/// would drop a NaN side and bound the piece by the other alone).
fn combine(phi_lo: f64, phi_hi: f64) -> f64 {
    let bound = not_proved(-phi_lo).max(not_proved(phi_hi)).max(0.0) * (1.0 + gamma(8));
    if bound.is_finite() {
        bound
    } else {
        f64::INFINITY
    }
}

/// The proved bound of `|phi|` (module docs) over the skin `skin` on the rectangle `u` x `v`
/// of its parameters, against the family `family`: every piece is bounded, and a piece over
/// `tol` is quartered while fewer than [`MAX_PIECES`] are made; the largest bound over the
/// pieces left. Infinite where a piece is not proved, or the skin has no Bézier pieces with
/// positive weights.
///
/// # Errors
///
/// A cancellation; those of the pieces' construction other than their being unsupported.
pub(super) fn span_bound(
    k: &Kernel,
    skin: &Surface,
    family: &BallFamily,
    u: [f64; 2],
    v: [f64; 2],
    tol: f64,
    bi: &Binomials,
) -> Result<f64> {
    let [u0, u1] = u;
    let [v0, v1] = v;
    let rect = Extent2d::from_bounds(u0, v0, u1, v1);
    let (mut work, slack) = match pieces_about(k, skin, Some(&rect), family.origin) {
        Ok(found) => found,
        Err(SolidError::Unsupported { .. }) => return Ok(f64::INFINITY),
        Err(e) => return Err(e),
    };
    if work.is_empty() {
        return Ok(f64::INFINITY);
    }
    let mut made = work.len();
    let mut worst = 0.0_f64;
    while let Some(piece) = work.pop() {
        k.check_cancelled(OP)?;
        let bound = match family.piece_bound(&piece, bi) {
            Ok(b) => b,
            Err(SolidError::Unsupported { .. }) => f64::INFINITY,
            Err(e) => return Err(e),
        };
        if bound <= tol || made + 4 > MAX_PIECES {
            // (NaN is not below anything: an unproved piece counts as infinite)
            worst = if bound <= f64::MAX {
                worst.max(bound)
            } else {
                f64::INFINITY
            };
            continue;
        }
        // (where the radius runs to zero on the piece the skin closes to a point along its
        // v, and only its length in u brings the bound down: halved in u; else quartered)
        if family.radius_vanishes_on(&piece, bound, bi)? {
            made += 2;
            work.extend(halves_in_u(&piece));
        } else {
            made += 4;
            work.extend(piece.quarters()?);
        }
    }
    Ok(worst + slack)
}

/// The two halves of a piece in its first parameter (de Casteljau at one half), each with the
/// error of one split level, as [`Piece::quarters`] charges it.
fn halves_in_u(piece: &Piece) -> [Piece; 2] {
    let largest = piece
        .h
        .iter()
        .map(Bernstein2::max_abs_coefficient)
        .fold(0.0_f64, f64::max);
    let [x, y, z, w] = &piece.h;
    let (du, dv) = w.degrees();
    let err = piece.err + gamma(2 * (du + dv) + 4) * (largest + piece.err);
    let ((xl, xr), (yl, yr), (zl, zr), (wl, wr)) = (
        x.split_u(0.5),
        y.split_u(0.5),
        z.split_u(0.5),
        w.split_u(0.5),
    );
    let [u0, u1, v0, v1] = piece.uv;
    let um = 0.5 * (u0 + u1);
    let half = |h: [Bernstein2; 4], uv: [f64; 4]| Piece {
        h,
        err,
        lower: 0.0,
        depth: piece.depth + 1,
        uv,
    };
    [
        half([xl, yl, zl, wl], [u0, um, v0, v1]),
        half([xr, yr, zr, wr], [um, u1, v0, v1]),
    ]
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::nurbs::b_spline_curve::KnotType;
    use crate::nurbs::b_spline_surface::{
        BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
    };

    /// A quarter of the tube about the x axis from `x = 0` to `x = 1`, over `u` in [0, 1]
    /// along x and `v` in [0, 1] from `(x, 0, rho)` to `(x, rho, 0)`, whose distance from the
    /// axis is `rho(u) = r0 + bump u (u - 1/2) (u - 1)`, in exact rational form: on balls of
    /// radius `r0` along the axis at `u = 0`, `1/2` and `1`, off them in between, most
    /// (`bump / (12 sqrt 3)`) at `u = 1/2 -+ 1/(2 sqrt 3)`.
    pub(in crate::blend) fn bulged_tube(r0: f64, bump: f64) -> Surface {
        // u (u - 1/2) (u - 1) in cubic Bernstein form: 0, 1/6, -1/6, 0
        let rho = [r0, r0 + bump / 6.0, r0 - bump / 6.0, r0];
        let xs = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
        let mut points = Vec::new();
        let mut weights = Vec::new();
        for (x, r) in xs.into_iter().zip(rho) {
            for (p, w) in [
                (Point3::new(x, 0.0, r), 1.0),
                (Point3::new(x, r, r), core::f64::consts::FRAC_1_SQRT_2),
                (Point3::new(x, r, 0.0), 1.0),
            ] {
                points.push(p);
                weights.push(w);
            }
        }
        Surface::from(
            BSplineSurface::create_canonical(&CanonicalSurfaceData {
                u_degree: 3,
                v_degree: 2,
                control_points: &points,
                form: BSplineSurfaceForm::Unspecified,
                u_multiplicities: &[4, 4],
                v_multiplicities: &[3, 3],
                u_knots: &[0.0, 1.0],
                v_knots: &[0.0, 1.0],
                knot_type: KnotType::Unspecified,
                weights: Some(&weights),
                uv_domain: None,
            })
            .unwrap(),
        )
    }

    fn ball(x: f64, r: f64) -> Ball {
        Ball {
            c: Point3::new(x, 0.0, 0.0),
            r,
            at: x,
        }
    }

    /// The family through balls of radius `r(x)` at `xs`, about the span `[0, 1]`.
    fn family(xs: &[f64], r: impl Fn(f64) -> f64) -> BallFamily {
        let nodes: Vec<Ball> = xs.iter().map(|&x| ball(x, r(x))).collect();
        BallFamily::through(&nodes, 0.5, [0.0, 1.0]).unwrap()
    }

    fn bound_of(skin: &Surface, family: &BallFamily, tol: f64) -> f64 {
        let k = &Kernel::default();
        span_bound(
            k,
            skin,
            family,
            [0.0, 1.0],
            [0.0, 1.0],
            tol,
            &Binomials::new(),
        )
        .unwrap()
    }

    /// A tube of radius `r0 + delta` about balls of radius `r0` is `delta` outside their
    /// envelope everywhere (closed form); one of `r0 - delta`, `delta` inside. The bound
    /// holds and is tight.
    #[test]
    fn the_bound_of_a_tube_off_its_balls_is_its_offset() {
        let r0 = 2.0;
        let family = family(&[0.0, 0.5, 1.0], |_| r0);
        for delta in [3e-5, -3e-5, 1e-3, -1e-3] {
            let skin = bulged_tube(r0 + delta, 0.0);
            let b = bound_of(&skin, &family, 4e-5);
            assert!(b >= delta.abs(), "{delta}: {b}");
            assert!(b <= delta.abs() * (1.0 + 1e-6) + 1e-12, "{delta}: {b}");
        }
        // on its balls: only rounding
        let b = bound_of(&bulged_tube(r0, 0.0), &family, 4e-5);
        assert!(b < 1e-12, "{b}");
    }

    /// A tube bulged between the three balls it passes through: the bound holds over the
    /// whole span against the closed form `|rho(u) - r0|` (largest `bump / (12 sqrt 3)`),
    /// and is tight enough to tell a bulge under the tolerance from one over it.
    #[test]
    fn the_bound_of_a_bulge_between_the_balls_covers_its_peak() {
        let r0 = 2.0;
        let family = family(&[-0.25, 0.0, 0.5, 1.0, 1.25], |_| r0);
        let peak = |bump: f64| bump.abs() / (12.0 * 3.0_f64.sqrt());
        for bump in [1e-4, -1e-4, 2e-3, 0.1] {
            let b = bound_of(&bulged_tube(r0, bump), &family, 4e-5);
            assert!(b >= peak(bump), "{bump}: {b} < {}", peak(bump));
            // (pieces are quartered only while over the tolerance)
            assert!(
                b <= (1.5 * peak(bump)).max(4e-5),
                "{bump}: {b} vs {}",
                peak(bump)
            );
        }
    }

    /// Balls whose radius outruns their centres (`|r'| >= |c'|`) have no envelope: the
    /// family's second derivative is not positive, and nothing is proved.
    #[test]
    fn nesting_balls_prove_nothing() {
        let family = family(&[0.0, 0.5, 1.0], |x| 1.0 + 2.0 * x);
        assert_eq!(
            bound_of(&bulged_tube(2.0, 0.0), &family, 4e-5),
            f64::INFINITY
        );
    }

    /// A side that is NaN proves nothing: the piece's bound is infinite, not the other
    /// side's alone (`f64::max` drops a NaN).
    #[test]
    fn a_side_that_is_nan_leaves_the_piece_unproved() {
        assert_eq!(combine(-1e-6, f64::NAN), f64::INFINITY);
        assert_eq!(combine(f64::NAN, 1e-6), f64::INFINITY);
        assert_eq!(combine(f64::NEG_INFINITY, 0.0), f64::INFINITY);
        let b = combine(-1e-6, 2e-6);
        assert!((2e-6..=2e-6 * (1.0 + 1e-12)).contains(&b), "{b}");
    }

    /// A coefficient that is not finite proves nothing: no range (`min`/`max` would drop a
    /// NaN quotient and range the others alone).
    #[test]
    fn a_quotient_with_a_coefficient_that_is_not_finite_has_no_range() {
        let bi = &Binomials::new();
        let poly = |c: Vec<f64>| Poly {
            p: Bernstein2::new(1, 0, c).unwrap(),
            e: 0.0,
        };
        let ones = poly(vec![1.0, 1.0]);
        assert!(quotient_range(&poly(vec![1.0, 2.0]), &ones, bi)
            .unwrap()
            .is_some());
        for bad in [f64::NAN, f64::INFINITY] {
            assert!(quotient_range(&poly(vec![1.0, bad]), &ones, bi)
                .unwrap()
                .is_none());
        }
    }

    #[test]
    fn a_family_needs_three_finite_balls_in_increasing_parameter_over_its_span() {
        let through = |nodes: &[Ball]| BallFamily::through(nodes, 0.5, [0.0, 1.0]);
        assert!(through(&[ball(0.0, 1.0), ball(0.5, 1.0), ball(1.0, 1.0)]).is_some());
        assert!(through(&[ball(0.0, 1.0), ball(1.0, 1.0), ball(0.5, 1.0)]).is_none());
        assert!(through(&[ball(0.0, 1.0), ball(0.5, f64::NAN), ball(1.0, 1.0)]).is_none());
        assert!(through(&[ball(0.0, 1.0), ball(0.5, 1.0)]).is_none());
        assert!(through(&[ball(0.0, 1.0), ball(0.5, 1.0), ball(0.9, 1.0)]).is_none());
    }
}
