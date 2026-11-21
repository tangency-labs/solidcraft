//! Cross sections reparametrised so that a skin through them is G1 across their joints
//! (not upstream: SolidCraft #15859).
//!
//! Upstream: none. [`super::p5::create_skin_srf_boundary_continuity`] calls
//! [`match_joint_ratios`] once its cross sections are compatible.
//!
//! A skin interpolates the compatible sections' control points across the sections, so
//! along a knot `u*` of the sections its u-derivative is `S_u(u*, v) = sum_k L_k(v) C_k'(u*)`
//! with `L_k` the interpolation's own functions. A section that is G1 but not C1 at `u*`
//! (a joint of two pieces of a fit-point spline, each on a unit interval) has
//! `C_k'(u*+) = r_k C_k'(u*-)` with `r_k > 0`. Where every section has the same ratio
//! `r`, `S_u(u*+, v) = r S_u(u*-, v)` and the surface is G1 across the knot. Where the
//! ratios differ, the surface is G1 only at the sections themselves: between them its
//! normal jumps across the knot (#15859 measured up to 4.4e-4 rad), so it has creases
//! inside one face, and an exact offset of the face parts there by `|d| |n- - n+|`.
//!
//! The sections are reparametrised so that they all have one ratio at every such joint,
//! `tau` (the geometric mean of theirs). Section `k`'s new parametrisation `phi_k` keeps
//! every knot where it is (so the sections still meet the skin's knots, and the trace of
//! their segments, [`super::super::crv_tool::p2::KnotMap`], holds) and has slope
//! `sqrt(tau / r_k)` just after the joint and `sqrt(r_k / tau)` just before it: then
//! `C_k(phi_k)` has ratio `r_k sqrt(tau / r_k) / sqrt(r_k / tau) = tau`. On each span
//! `phi_k` is a C1 quadratic spline (its slope piecewise linear) through both ends, so
//! `C_k(phi_k)` is the same curve, exactly, with degree `2p`; its new knots inside the
//! span are C1 for every section. The sections are not approximated and not moved.
//!
//! Decided on the sections' own data before anything is built:
//! - A joint is matched only where every section is G1 there in homogeneous space (the
//!   homogeneous derivatives on both sides point the same way within the kernel's angular
//!   zero, [`EFF_ZERO_RAD`]): a reparametrisation scales a derivative, it cannot turn one,
//!   so a corner stays a corner.
//! - A joint whose new slopes would all be 1 within the kernel's parametric zero
//!   ([`EFF_ZERO_PARAM`]) is left: its reparametrisation would be the identity at that
//!   resolution (sections that are scaled copies of one another, or C1 sections).
//! - Where nothing is left to match, or the sections are not compatible, or `2p` is past
//!   NLib's largest degree ([`MAX_DEGREE`]), or a span is too short to hold its new knots
//!   as distinct values, the sections stay as they are.
//!
//! Deviations: not upstream (the whole module).

use crate::error::{nurbs::NurbsFailure, Result, SolidError};
use crate::math::{EFF_ZERO_PARAM, EFF_ZERO_RAD};
use crate::nurbs::crv_basic::Curve;
use crate::nurbs::knots::MAX_DEGREE;
use crate::nurbs::math::CPoint;

const OP: &str = "nurbs::srf_adv::joint_ratios::match_joint_ratios";

fn oob() -> SolidError {
    SolidError::nurbs(OP, NurbsFailure::Input)
}

/// The derivative ratio `r > 0` of `right = r left` (homogeneous first derivatives), when the
/// two point the same way within [`EFF_ZERO_RAD`]; `None` otherwise (a corner, a cusp, or a
/// vanishing derivative).
fn g1_ratio(left: CPoint, right: CPoint) -> Option<f64> {
    let ll = CPoint::dot_4d(left, left);
    let rr = CPoint::dot_4d(right, right);
    let lr = CPoint::dot_4d(left, right);
    if !(ll > 0.0 && rr > 0.0 && lr > 0.0) {
        return None;
    }
    // sin^2 of the angle between them, by Lagrange's identity
    let sin2 = 1.0 - (lr / ll) * (lr / rr);
    let limit = EFF_ZERO_RAD.sin();
    if sin2 > limit * limit {
        return None;
    }
    let r = (rr / ll).sqrt();
    r.is_finite().then_some(r)
}

/// `n` choose `k`, as `f64` (exact for the degrees NLib has).
fn binomial(n: usize, k: usize) -> f64 {
    let Some(rest) = n.checked_sub(k) else {
        return 0.0;
    };
    let k = k.min(rest);
    let mut b = 1.0;
    for i in 0..k {
        b = b * (n - i) as f64 / (i + 1) as f64;
    }
    b
}

/// The Bernstein coefficients of the product of two polynomials in Bernstein form.
fn bernstein_product(a: &[f64], b: &[f64]) -> Vec<f64> {
    let (Some(m), Some(n)) = (a.len().checked_sub(1), b.len().checked_sub(1)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for k in 0..=m + n {
        let mut c = 0.0;
        for (i, ai) in a.iter().enumerate() {
            let Some(j) = k.checked_sub(i) else { break };
            if let Some(bj) = b.get(j) {
                c += binomial(m, i) * binomial(n, j) * ai * bj;
            }
        }
        out.push(c / binomial(m + n, k));
    }
    out
}

/// The control points (degree `2p`) of the Bezier curve `bez` (degree `p`, on `[0, 1]`)
/// composed with the quadratic `g` (its Bernstein coefficients, values in `[0, 1]`):
/// `bez(g(s)) = sum_i bez_i C(p, i) g^i (1 - g)^(p - i)`, each product in Bernstein form.
fn compose(bez: &[CPoint], g: [f64; 3]) -> Result<Vec<CPoint>> {
    let p = bez.len().checked_sub(1).ok_or_else(oob)?;
    let h = [1.0 - g[0], 1.0 - g[1], 1.0 - g[2]];
    let mut g_pow = vec![vec![1.0]];
    let mut h_pow = vec![vec![1.0]];
    for i in 0..p {
        let next_g = bernstein_product(g_pow.get(i).ok_or_else(oob)?, &g);
        let next_h = bernstein_product(h_pow.get(i).ok_or_else(oob)?, &h);
        g_pow.push(next_g);
        h_pow.push(next_h);
    }
    let mut terms = Vec::new();
    for i in 0..=p {
        let gi = g_pow.get(i).ok_or_else(oob)?;
        let hi = h_pow.get(p - i).ok_or_else(oob)?;
        let c = binomial(p, i);
        terms.push(
            bernstein_product(gi, hi)
                .into_iter()
                .map(|x| c * x)
                .collect::<Vec<_>>(),
        );
    }
    let mut out = Vec::new();
    for l in 0..=2 * p {
        let mut acc: Option<CPoint> = None;
        for (b, t) in bez.iter().zip(&terms) {
            let w = *t.get(l).ok_or_else(oob)?;
            acc = Some(match acc {
                None => b.scale(w),
                Some(a) => CPoint::combine2(1.0, a, w, *b),
            });
        }
        out.push(acc.ok_or_else(oob)?);
    }
    Ok(out)
}

/// One span's reparametrisation: the quadratic pieces (Bernstein coefficients on the
/// span's normalised parameter) and where the new knots fall (fractions of the span).
/// `start` and `end` are the slopes at its ends, `lambda` the span's shared fraction.
fn span_pieces(start: f64, end: f64, lambda: f64) -> Vec<[f64; 3]> {
    if start == 1.0 && end == 1.0 {
        // the identity, as a quadratic: the span degree elevated
        return vec![[0.0, 0.5, 1.0]];
    }
    // The slope runs linearly from `start` to `gamma` over [0, lambda], stays `gamma` to
    // 1 - lambda and runs to `end` over the last lambda; the slopes' integral is 1, so
    // gamma = (1 - lambda (start + end) / 2) / (1 - lambda) > 0 for lambda <= 1 / (start + end).
    let gamma = (1.0 - 0.5 * lambda * (start + end)) / (1.0 - lambda);
    let fa = 0.5 * lambda * (start + gamma);
    let fb = fa + (1.0 - 2.0 * lambda) * gamma;
    let first = [0.0, 0.5 * lambda * start, fa];
    let last = [fb, 1.0 - 0.5 * lambda * end, 1.0];
    if lambda >= 0.5 {
        vec![first, [fa, 1.0 - 0.5 * lambda * end, 1.0]]
    } else {
        vec![first, [fa, 0.5 * (fa + fb), fb], last]
    }
}

/// Reparametrises the compatible cross sections `cur` (one degree, one knot vector) so that
/// at every joint where they are all G1 they all have the same derivative ratio, keeping
/// every section the same curve (see the module docs). Sections it has nothing to match in
/// stay as they are, bit for bit.
///
/// # Errors
///
/// Those of the Bezier decomposition and the curve construction.
pub(crate) fn match_joint_ratios(cur: &mut [Curve]) -> Result<()> {
    let Some(first) = cur.first() else {
        return Ok(());
    };
    let p = first.degree();
    let knots: Vec<f64> = first.knots().to_vec();
    if cur.len() < 2 || p == 0 || 2 * p > MAX_DEGREE {
        return Ok(());
    }
    let same = |c: &Curve| {
        c.degree() == p
            && c.knots().len() == knots.len()
            && c.knots()
                .iter()
                .zip(&knots)
                .all(|(a, b)| a.to_bits() == b.to_bits())
    };
    if !cur.iter().all(same) {
        return Ok(());
    }
    // the distinct knots, bit for bit: span j runs from distinct[j] to distinct[j + 1]
    let mut distinct: Vec<f64> = Vec::new();
    for &u in &knots {
        if distinct.last().is_none_or(|l| l.to_bits() != u.to_bits()) {
            distinct.push(u);
        }
    }
    let spans: Vec<Vec<Curve>> = cur
        .iter()
        .map(Curve::decompose_bez)
        .collect::<Result<_>>()?;
    let n = distinct.len().saturating_sub(1);
    if n == 0 || spans.iter().any(|s| s.len() != n) {
        return Ok(());
    }
    let width = |j: usize| -> Result<f64> {
        Ok(*distinct.get(j + 1).ok_or_else(oob)? - *distinct.get(j).ok_or_else(oob)?)
    };

    // The joints: (span before, span after). The seam of closed sections is one too.
    let mut joints: Vec<(usize, usize)> = (1..n).map(|j| (j - 1, j)).collect();
    if cur.iter().all(Curve::is_closed) {
        joints.push((n - 1, 0));
    }

    // Each section's slope at the start and end of each span (as many entries as the
    // sections' Bezier spans, which are already in memory).
    let mut start = vec![vec![1.0; n]; cur.len()];
    let mut end = vec![vec![1.0; n]; cur.len()];
    let mut any = false;
    for &(a, b) in &joints {
        let (ha, hb) = (width(a)?, width(b)?);
        let mut ratios = Vec::new();
        for s in &spans {
            let left = s.get(a).ok_or_else(oob)?.cpts();
            let right = s.get(b).ok_or_else(oob)?.cpts();
            let dl = CPoint::diff(
                *left.get(p).ok_or_else(oob)?,
                *left.get(p - 1).ok_or_else(oob)?,
            )
            .scale(1.0 / ha);
            let dr = CPoint::diff(
                *right.get(1).ok_or_else(oob)?,
                *right.first().ok_or_else(oob)?,
            )
            .scale(1.0 / hb);
            match g1_ratio(dl, dr) {
                Some(r) => ratios.push(r),
                None => break,
            }
        }
        if ratios.len() != cur.len() {
            continue;
        }
        let tau = (ratios.iter().map(|r| r.ln()).sum::<f64>() / ratios.len() as f64).exp();
        let slopes: Vec<f64> = ratios.iter().map(|r| (tau / r).sqrt()).collect();
        if slopes.iter().all(|s| (s - 1.0).abs() <= EFF_ZERO_PARAM) {
            continue;
        }
        for (k, &s) in slopes.iter().enumerate() {
            *start
                .get_mut(k)
                .and_then(|v| v.get_mut(b))
                .ok_or_else(oob)? = s;
            *end.get_mut(k).and_then(|v| v.get_mut(a)).ok_or_else(oob)? = 1.0 / s;
        }
        any = true;
    }
    if !any {
        return Ok(());
    }

    // Each span's shared fraction (every section's breaks inside a span at the same knots),
    // and the new knot vector's breaks.
    let mut lambdas = Vec::new();
    let mut breaks = vec![*distinct.first().ok_or_else(oob)?];
    for j in 0..n {
        let touched = (0..cur.len()).any(|k| {
            start.get(k).and_then(|v| v.get(j)) != Some(&1.0)
                || end.get(k).and_then(|v| v.get(j)) != Some(&1.0)
        });
        let (u0, u1) = (
            *distinct.get(j).ok_or_else(oob)?,
            *distinct.get(j + 1).ok_or_else(oob)?,
        );
        let mut lambda = 0.5;
        if touched {
            for k in 0..cur.len() {
                let s = *start.get(k).and_then(|v| v.get(j)).ok_or_else(oob)?;
                let e = *end.get(k).and_then(|v| v.get(j)).ok_or_else(oob)?;
                lambda = f64::min(lambda, 1.0 / (s + e));
            }
            let h = u1 - u0;
            let inner = if lambda >= 0.5 {
                vec![u0 + 0.5 * h]
            } else {
                vec![u0 + lambda * h, u1 - lambda * h]
            };
            for x in inner {
                if !(x > *breaks.last().ok_or_else(oob)? && x < u1) {
                    // too short a span for distinct knots: leave the sections as they are
                    return Ok(());
                }
                breaks.push(x);
            }
        }
        breaks.push(u1);
        lambdas.push((touched, lambda));
    }
    let q = 2 * p;
    let mut new_knots = Vec::new();
    for (i, &b) in breaks.iter().enumerate() {
        let mult = if i == 0 || i + 1 == breaks.len() {
            q + 1
        } else {
            q
        };
        new_knots.extend((0..mult).map(|_| b));
    }

    for (k, c) in cur.iter_mut().enumerate() {
        let mut pts: Vec<CPoint> = Vec::new();
        for (j, &(touched, lambda)) in lambdas.iter().enumerate() {
            let bez = spans.get(k).and_then(|s| s.get(j)).ok_or_else(oob)?.cpts();
            let bez = bez.get(..=p).ok_or_else(oob)?;
            let pieces = if touched {
                let s = *start.get(k).and_then(|v| v.get(j)).ok_or_else(oob)?;
                let e = *end.get(k).and_then(|v| v.get(j)).ok_or_else(oob)?;
                span_pieces(s, e, lambda)
            } else {
                span_pieces(1.0, 1.0, lambda)
            };
            for g in pieces {
                let piece = compose(bez, g)?;
                // each piece starts where the last one ended
                let skip = usize::from(!pts.is_empty());
                pts.extend(piece.into_iter().skip(skip));
            }
        }
        *c = Curve::new(pts, q, new_knots.clone())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composing_with_the_identity_elevates_the_degree() {
        let bez = [
            CPoint::new(0.0, 0.0, 0.0, crate::nurbs::math::NOW),
            CPoint::new(1.0, 2.0, 0.0, crate::nurbs::math::NOW),
            CPoint::new(3.0, 2.0, 1.0, crate::nurbs::math::NOW),
            CPoint::new(4.0, 0.0, 1.0, crate::nurbs::math::NOW),
        ];
        let out = compose(&bez, [0.0, 0.5, 1.0]).unwrap();
        assert_eq!(out.len(), 7);
        let c = Curve::new(out, 6, [vec![0.0; 7], vec![1.0; 7]].concat()).unwrap();
        let d = Curve::new(bez.to_vec(), 3, [vec![0.0; 4], vec![1.0; 4]].concat()).unwrap();
        for i in 0..=8 {
            let t = f64::from(i) / 8.0;
            let a = c.eval(t, crate::nurbs::knots::Side::Left).unwrap();
            let b = d.eval(t, crate::nurbs::knots::Side::Left).unwrap();
            assert!((a - b).length() < 1e-14, "{t}");
        }
    }

    #[test]
    fn a_span_reparametrisation_is_c1_with_the_given_end_slopes() {
        for (s, e) in [(1.2, 0.9), (3.0, 2.5), (0.2, 4.0)] {
            let lambda = f64::min(0.5, 1.0 / (s + e));
            let pieces = span_pieces(s, e, lambda);
            let widths: Vec<f64> = if pieces.len() == 2 {
                vec![0.5, 0.5]
            } else {
                vec![lambda, 1.0 - 2.0 * lambda, lambda]
            };
            let first = pieces.first().unwrap();
            let last = pieces.last().unwrap();
            assert!((2.0 * (first[1] - first[0]) / widths[0] - s).abs() < 1e-14);
            assert!((2.0 * (last[2] - last[1]) / widths[widths.len() - 1] - e).abs() < 1e-14);
            for (i, w) in pieces.windows(2).enumerate() {
                let out = 2.0 * (w[0][2] - w[0][1]) / widths[i];
                let inn = 2.0 * (w[1][1] - w[1][0]) / widths[i + 1];
                assert!((out - inn).abs() < 1e-14, "{s} {e}: {out} vs {inn}");
                assert!(w[0][2].to_bits() == w[1][0].to_bits());
                assert!(out > 0.0);
            }
            assert!((last[2] - 1.0).abs() < 1e-15);
        }
    }
}
