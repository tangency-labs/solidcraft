//! Global interpolation of points with a direction given at either end (not upstream): the
//! curve through the points whose tangent at each given end runs along that direction. A half
//! section whose end at a mirror plane runs square to it then joins its mirror smoothly.
//!
//! The direction becomes the end derivative the interpolation takes ([`interp_knots_and_derivs`],
//! [`interp_knots_and_deriv`]: The NURBS Book, §9.2.2), its length the data polygon's length,
//! the magnitude the book suggests for parameters on `[0, 1]` (what upstream's own estimate of a
//! missing end derivative scales by). The knots are upstream's for that system
//! ([`knot_vector_end_derivs`], [`knot_vector_deriv`]).
//!
//! An end asked to run square to a plane takes its direction from [`square_to_plane`], which
//! decides whether the points allow it: a curve through points that all lie in a plane square
//! to the normal cannot leave that plane square to it without turning out of it and back.
//!
//! [`approx_end_directions`] (not upstream) is the least-squares counterpart at given knots:
//! the curve through the first and the last point, as [`approx_knots`] (The NURBS Book,
//! §9.4.1), whose start leaves along a given direction and whose end arrives along another.
//! Only the direction is held: the second control point lies on the line from the first along
//! the start's direction, the last but one on the line back from the last along the end's,
//! and how far along each line (the reach) is an unknown of the fit. A curve refitted in pieces
//! then meets its neighbour without a corner, whatever the lengths of the two fits' ends.
//!
//! The fit solves its normal equations by eliminating the free control points first: the
//! banded matrix `NᵀN` over them, as [`approx_knots`] builds it, decomposed by the kernel's LU
//! without pivoting ([`RealMatrix::lu_decompose`]), one solve for the points and one for the
//! two scalar right sides of the reaches; then a 2 by 2 system for the reaches, by
//! elimination. Plain floating point in the order written, as [`approx_knots`]: a
//! least-squares solve is not a certificate, and a caller measures the result with
//! `crv_deviation`.

use crate::error::{nurbs::NurbsFailure, Result, SolidError};
use crate::math::Vec3;
use crate::nurbs::crv_basic::Curve;
use crate::nurbs::crv_fit::p1::{
    approx_knots, interp_knots_and_deriv, interp_knots_and_derivs, knot_vector_deriv,
    knot_vector_end_derivs, param_values, DerivEnd, ParamMethod,
};
use crate::nurbs::crv_fit::p2::interp;
use crate::nurbs::fit_arrays::{alloc, sg};
use crate::nurbs::knots::{KnotVector, Side};
use crate::nurbs::math::{CPoint, MatrixType, RealMatrix, LU_TOL};

const OP: &str = "nurbs::crv_fit::end_directions::interp_end_directions";

/// The curve of degree `p` through the points `a`, parametrised by `par`, whose tangent at the
/// start runs along `start` and at the end along `end` where they are given; with neither, the
/// plain interpolation ([`interp`]). A given direction is scaled to the data polygon's length
/// and taken as the end derivative.
///
/// # Errors
///
/// `InvalidInput` for a direction that is not finite or has no length, or points whose polygon
/// has no length; those of the parametrisation, the knot vectors and the interpolation (a
/// degree below 2 with a direction given, or too high for the points).
pub fn interp_end_directions(
    a: &[Vec3],
    p: usize,
    start: Option<Vec3>,
    end: Option<Vec3>,
    par: ParamMethod,
) -> Result<Curve> {
    if start.is_none() && end.is_none() {
        return interp(a, p, par);
    }
    let length: f64 = a
        .windows(2)
        .map(|w| match w {
            [x, y] => (*y - *x).length(),
            _ => 0.0,
        })
        .sum();
    if !(length.is_finite() && length > 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "points whose polygon has no length",
        ));
    }
    let derivative = |d: Vec3| -> Result<Vec3> {
        let l = d.length();
        if !(l.is_finite() && l > 0.0) {
            return Err(SolidError::invalid_input(OP, "a direction of no length"));
        }
        Ok(d * (length / l))
    };
    let u = param_values(a, par)?;
    match (start, end) {
        (Some(s), Some(e)) => {
            let knots = knot_vector_end_derivs(&u, p)?;
            let kv = KnotVector::from_slice(&knots)?;
            interp_knots_and_derivs(a, &u, &kv, p, derivative(s)?, derivative(e)?)
        }
        (Some(s), None) => {
            let knots = knot_vector_deriv(&u, p, DerivEnd::Start)?;
            let kv = KnotVector::from_slice(&knots)?;
            interp_knots_and_deriv(a, &u, &kv, p, derivative(s)?, DerivEnd::Start)
        }
        (None, Some(e)) => {
            let knots = knot_vector_deriv(&u, p, DerivEnd::End)?;
            let kv = KnotVector::from_slice(&knots)?;
            interp_knots_and_deriv(a, &u, &kv, p, derivative(e)?, DerivEnd::End)
        }
        (None, None) => interp(a, p, par),
    }
}

const SQUARE_OP: &str = "nurbs::crv_fit::end_directions::square_to_plane";

/// The direction the end `at` of a curve through the points `a`, in order, takes when held
/// square to a plane of normal `normal` (either sense, any length), as [`interp_end_directions`]
/// takes it: the unit normal in the sense the curve runs there. That is the side of the plane
/// through the end point the points first leave it to, walking from that end, more than `tol`
/// off it: the curve leaves the start toward it and reaches the end from it.
///
/// `None` when every point lies within `tol` of that plane: the curve lies in a plane square to
/// the normal and cannot leave it square to it (it would hook out of the plane and back), so
/// the end is left free.
///
/// # Errors
///
/// `InvalidInput` for no points, a coordinate or a distance that is not finite, a normal that
/// is not finite or is zero, or a tolerance not finite and positive.
pub fn square_to_plane(a: &[Vec3], normal: Vec3, at: DerivEnd, tol: f64) -> Result<Option<Vec3>> {
    if !(tol.is_finite() && tol > 0.0) {
        return Err(SolidError::invalid_input(
            SQUARE_OP,
            "a tolerance not finite and positive",
        ));
    }
    let Some((_, n)) = normal.length_and_unit() else {
        return Err(SolidError::invalid_input(
            SQUARE_OP,
            "a normal not finite or zero",
        ));
    };
    let end = match at {
        DerivEnd::Start => a.first(),
        DerivEnd::End => a.last(),
    }
    .copied()
    .ok_or_else(|| SolidError::invalid_input(SQUARE_OP, "no points"))?;
    // each point's signed distance from the plane through the end, from that end inward
    let mut distances: Vec<f64> = a.iter().map(|&q| n.dot(q - end)).collect();
    if !distances.iter().all(|d| d.is_finite()) {
        return Err(SolidError::invalid_input(
            SQUARE_OP,
            "a distance not finite",
        ));
    }
    if at == DerivEnd::End {
        distances.reverse();
    }
    let Some(&first_off) = distances.iter().find(|d| d.abs() > tol) else {
        return Ok(None);
    };
    // toward the points' side at the start; from it, so away from it, at the end
    let into = if first_off > 0.0 { n } else { -n };
    Ok(Some(match at {
        DerivEnd::Start => into,
        DerivEnd::End => -into,
    }))
}

const APPROX_OP: &str = "nurbs::crv_fit::end_directions::approx_end_directions";

/// A least-squares fit with a direction held at an end ([`approx_end_directions`]).
#[derive(Clone, Debug, PartialEq)]
pub struct EndDirectionFit {
    /// The curve.
    pub curve: Curve,
    /// How far along the start's direction the second control point lies from the first;
    /// `None` where no start direction was given.
    pub start_reach: Option<f64>,
    /// How far back along the end's direction the last control point but one lies from the
    /// last; `None` where no end direction was given.
    pub end_reach: Option<f64>,
}

/// The error of data that do not fix the fit (and of the shapes [`approx_knots`] refuses).
fn no_fit() -> SolidError {
    SolidError::nurbs(APPROX_OP, NurbsFailure::Input)
}

/// `len` copies of `v`, reserved fallibly.
fn filled<T: Clone>(len: usize, v: T) -> Result<Vec<T>> {
    alloc(sg(len, APPROX_OP)? - 1, v, APPROX_OP)
}

/// `Σ x_i y_i` in order.
fn dot(x: &[f64], y: &[f64]) -> f64 {
    x.iter().zip(y).map(|(a, b)| a * b).sum()
}

/// The least-squares curve of degree `p` with control points `0..=n` and the knots `knt`
/// (`n + p + 2` of them) through the first and last of the points `a[0..=m]`, taken at the
/// parameters `u`, as [`approx_knots`]; where `start` is given, the curve leaves the first
/// point along it, and where `end` is given, it reaches the last point running along it.
/// Each direction's length does not matter.
///
/// A held start puts the second control point at `P_0 + s T_0` (`T_0` the start's unit
/// direction), a held end the last but one at `P_n - e T_1`; the reaches `s` and `e` are
/// unknowns of the fit, with the free control points, and are returned as solved, whatever
/// their sign. A reach that is not positive means the points do not leave (or reach) that end
/// along the direction: the curve turns back there, and the caller decides what to do.
///
/// With neither direction the result is [`approx_knots`]'s curve itself and both reaches are
/// `None`.
///
/// Not upstream. Plain floating point, as [`approx_knots`]: the free control points are
/// eliminated through the banded normal matrix `NᵀN` (the kernel's LU without pivoting),
/// then a 2 by 2 system gives the reaches. The result is not certified.
///
/// # Errors
///
/// With neither direction, those of [`approx_knots`]. Otherwise: `InvalidInput` for a
/// direction that is not finite or has no length, a degree below 2, no points, or fewer
/// parameters than points; [`NurbsFailure::Input`] when `n < p`, `knt` does not have
/// `n + p + 2` knots, `m < n`, `n < 3` with both directions (`n < 2` with one), a basis span
/// past the control points, or data that do not fix the unknowns (a pivot of the normal
/// equations or of the 2 by 2 system below [`LU_TOL`]); those of
/// [`KnotVector::basis_eval`] and of the solve.
pub fn approx_end_directions(
    a: &[Vec3],
    u: &[f64],
    knt: &KnotVector<'_>,
    n: usize,
    p: usize,
    start: Option<Vec3>,
    end: Option<Vec3>,
) -> Result<EndDirectionFit> {
    const OP: &str = APPROX_OP;
    if start.is_none() && end.is_none() {
        return Ok(EndDirectionFit {
            curve: approx_knots(a, u, knt, n, p)?,
            start_reach: None,
            end_reach: None,
        });
    }
    let unit = |d: Option<Vec3>| -> Result<Option<Vec3>> {
        let Some(d) = d else {
            return Ok(None);
        };
        match d.length_and_unit() {
            Some((_, t)) => Ok(Some(t)),
            None => Err(SolidError::invalid_input(
                OP,
                "a direction not finite or zero",
            )),
        }
    };
    let t0 = unit(start)?;
    let t1 = unit(end)?;
    if p < 2 {
        return Err(SolidError::invalid_input(
            OP,
            "a degree below 2 with a direction",
        ));
    }
    let m = a
        .len()
        .checked_sub(1)
        .ok_or_else(|| SolidError::invalid_input(OP, "no data points"))?;
    if n < p {
        return Err(no_fit());
    }
    if n.checked_add(p).and_then(|x| x.checked_add(1)) != Some(knt.last_index()) {
        return Err(no_fit());
    }
    if m < n {
        return Err(no_fit());
    }
    let least = if t0.is_some() && t1.is_some() { 3 } else { 2 };
    if n < least {
        return Err(no_fit());
    }
    if u.len() < a.len() {
        return Err(SolidError::invalid_input(
            OP,
            "fewer parameters than points",
        ));
    }
    let (Some(&q0), Some(&qm)) = (a.first(), a.last()) else {
        return Err(SolidError::invalid_input(OP, "no data points"));
    };

    // The free control points lo..hi_end (hi_end exclusive; none when lo == hi_end), row
    // i - lo of the banded matrix, whose storage column is j - i + sbw.
    let lo = if t0.is_some() { 2 } else { 1 };
    let hi_end = if t1.is_some() { n - 1 } else { n };
    let nf = hi_end.saturating_sub(lo);
    let sbw = p;
    let bw = p
        .checked_mul(2)
        .and_then(|x| x.checked_add(1))
        .ok_or_else(no_fit)?;
    let mut cm = nf
        .checked_sub(1)
        .map(|top| RealMatrix::new(top, top, MatrixType::Banded, bw))
        .transpose()?;
    // N^T R per free control point, and N^T alpha, N^T beta
    let mut ntr = filled(nf, Vec3::ZERO)?;
    let mut g = filled(nf, 0.0)?;
    let mut h = filled(nf, 0.0)?;
    // alpha.alpha, beta.beta, alpha.beta, sum alpha_k R_k, sum beta_k R_k
    let (mut aa, mut bb, mut ab) = (0.0, 0.0, 0.0);
    let (mut ra, mut rb) = (Vec3::ZERO, Vec3::ZERO);

    // The interior data k = 1..m.
    for (&ak, &uk) in a.iter().zip(u).take(m).skip(1) {
        let b = knt.basis_eval(p, uk, Side::Left)?;
        let first = b.span.checked_sub(p).ok_or_else(no_fit)?;
        if b.span > n {
            return Err(no_fit());
        }
        let nv = |i: usize| -> f64 {
            i.checked_sub(first)
                .and_then(|j| b.values.get(j))
                .copied()
                .unwrap_or(0.0)
        };
        let alpha = if t0.is_some() { nv(1) } else { 0.0 };
        let beta = if t1.is_some() { nv(n - 1) } else { 0.0 };
        let mut fixed = q0 * nv(0) + qm * nv(n);
        if t0.is_some() {
            fixed += q0 * alpha;
        }
        if t1.is_some() {
            fixed += qm * beta;
        }
        let r = ak - fixed;
        aa += alpha * alpha;
        bb += beta * beta;
        ab += alpha * beta;
        ra += r * alpha;
        rb += r * beta;
        let Some(cm) = cm.as_mut() else {
            continue;
        };
        for (i, &ni) in (first..).zip(&b.values) {
            if !(lo..hi_end).contains(&i) {
                continue;
            }
            let row = i - lo;
            *ntr.get_mut(row).ok_or_else(no_fit)? += r * ni;
            *g.get_mut(row).ok_or_else(no_fit)? += ni * alpha;
            *h.get_mut(row).ok_or_else(no_fit)? += ni * beta;
            for (j, &nj) in (first..).zip(&b.values) {
                if !(lo..hi_end).contains(&j) {
                    continue;
                }
                let col = (j + sbw).checked_sub(i).ok_or_else(no_fit)?;
                let v = cm.get(row, col)? + ni * nj;
                cm.set(row, col, v)?;
            }
        }
    }

    // X_R = M^-1 N^T R; x_a, x_b = M^-1 N^T alpha, M^-1 N^T beta in one solve (x and y).
    let bad = |x: f64| x.is_nan() || x.abs() < LU_TOL;
    let (xr, xa, xb) = match cm {
        Some(mut cm) => {
            // The LU refuses a pivot below LU_TOL but for the last; check them all.
            cm.lu_decompose().map_err(|_| no_fit())?;
            for row in 0..nf {
                if bad(cm.get(row, sbw)?) {
                    return Err(no_fit());
                }
            }
            let xr: Vec<Vec3> = cm
                .for_back_points(&ntr)?
                .into_iter()
                .map(|c| Vec3::new(c.x, c.y, c.z))
                .collect();
            let gh: Vec<Vec3> = g
                .iter()
                .zip(&h)
                .map(|(&gi, &hi)| Vec3::new(gi, hi, 0.0))
                .collect();
            let (mut xa, mut xb) = (Vec::new(), Vec::new());
            for c in cm.for_back_points(&gh)? {
                xa.push(c.x);
                xb.push(c.y);
            }
            (xr, xa, xb)
        }
        None => (Vec::new(), Vec::new(), Vec::new()),
    };

    // The reaches: the normal equations of s and e with the free points eliminated.
    let a11 = aa - dot(&g, &xa);
    let a22 = bb - dot(&h, &xb);
    let a12 = ab - dot(&g, &xb);
    let mut gx = Vec3::ZERO;
    let mut hx = Vec3::ZERO;
    for ((&gi, &hi), &x) in g.iter().zip(&h).zip(&xr) {
        gx += x * gi;
        hx += x * hi;
    }
    let rho_a = gx - ra;
    let rho_b = hx - rb;
    let (s, e) = match (t0, t1) {
        (Some(t0), Some(t1)) => {
            // [A11, -c A12; c A12, -A22] (s, e) = (-T_0.rho_a, -T_1.rho_b), by elimination
            let c = t0.dot(t1);
            let b1 = -t0.dot(rho_a);
            let b2 = -t1.dot(rho_b);
            if bad(a11) {
                return Err(no_fit());
            }
            let l = c * a12 / a11;
            let u22 = l * (c * a12) - a22;
            if bad(u22) {
                return Err(no_fit());
            }
            let e = (b2 - l * b1) / u22;
            let s = (b1 + c * a12 * e) / a11;
            (Some(s), Some(e))
        }
        (Some(t0), None) => {
            if bad(a11) {
                return Err(no_fit());
            }
            (Some(-t0.dot(rho_a) / a11), None)
        }
        (None, Some(t1)) => {
            if bad(a22) {
                return Err(no_fit());
            }
            (None, Some(t1.dot(rho_b) / a22))
        }
        (None, None) => (None, None),
    };

    // P_1 = P_0 + s T_0, P_(n-1) = P_n - e T_1, P_i = X_R,i - s x_a,i T_0 + e x_b,i T_1.
    let p1 = t0.zip(s).map(|(t, s)| q0 + t * s);
    let pn1 = t1.zip(e).map(|(t, e)| qm - t * e);
    let mut pw = Vec::new();
    for i in 0..=n {
        let q = match (p1, pn1) {
            _ if i == 0 => q0,
            _ if i == n => qm,
            (Some(q), _) if i == 1 => q,
            (_, Some(q)) if i + 1 == n => q,
            _ => {
                let r = i.checked_sub(lo).ok_or_else(no_fit)?;
                let mut q = xr.get(r).copied().ok_or_else(no_fit)?;
                if let (Some(t0), Some(s)) = (t0, s) {
                    q -= t0 * (s * xa.get(r).copied().ok_or_else(no_fit)?);
                }
                if let (Some(t1), Some(e)) = (t1, e) {
                    q += t1 * (e * xb.get(r).copied().ok_or_else(no_fit)?);
                }
                q
            }
        };
        pw.push(CPoint::from_point(q));
    }
    Ok(EndDirectionFit {
        curve: Curve::new(pw, p, knt.knots().to_vec())?,
        start_reach: s,
        end_reach: e,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::knots::Side;

    fn pts() -> Vec<Vec3> {
        [
            [0.0, 0.0, 9.0],
            [0.0, 2.5, 8.8],
            [0.0, 5.2, 8.0],
            [0.0, 7.6, 6.4],
            [0.0, 8.8, 4.2],
        ]
        .iter()
        .map(|&[x, y, z]| Vec3::new(x, y, z))
        .collect()
    }

    #[test]
    fn the_curve_leaves_along_the_start_direction_through_every_point() {
        let a = pts();
        let c = interp_end_directions(
            &a,
            3,
            Some(Vec3::new(0.0, 3.0, 0.0)),
            None,
            ParamMethod::ChordLength,
        )
        .unwrap();
        let u = param_values(&a, ParamMethod::ChordLength).unwrap();
        for (q, t) in a.iter().zip(&u) {
            let p = c.eval(*t, Side::Left).unwrap();
            assert!((p - *q).length() < 1e-9, "{p:?} vs {q:?}");
        }
        let pw = c.cpts();
        let leg = Vec3::new(pw[1].x - pw[0].x, pw[1].y - pw[0].y, pw[1].z - pw[0].z);
        assert!(
            leg.x.abs() < 1e-12 && leg.z.abs() < 1e-12 && leg.y > 0.0,
            "{leg:?}"
        );
    }

    #[test]
    fn an_end_square_to_a_plane_runs_along_its_normal_the_way_the_points_leave_it() {
        // pts() runs from y = 0 out (+y) and falls (-z) at its end, all in x = 0.
        let a = pts();
        let start = square_to_plane(&a, Vec3::new(0.0, -2.0, 0.0), DerivEnd::Start, 1e-5);
        assert_eq!(start.unwrap(), Some(Vec3::new(0.0, 1.0, 0.0)));
        let end = square_to_plane(&a, Vec3::new(0.0, 0.0, 4.0), DerivEnd::End, 1e-5);
        assert_eq!(end.unwrap(), Some(Vec3::new(0.0, 0.0, -1.0)));
        // the points' own plane: no direction leaves it square to it
        for at in [DerivEnd::Start, DerivEnd::End] {
            let own = square_to_plane(&a, Vec3::new(1.0, 0.0, 0.0), at, 1e-5);
            assert_eq!(own.unwrap(), None);
        }
    }

    #[test]
    fn the_sense_skips_points_within_the_tolerance_of_the_ends_plane() {
        // the second point is 1e-6 above z = 0, inside the tolerance; the third is below it
        let a: Vec<Vec3> = [[0.0, 0.0, 0.0], [1.0, 0.0, 1e-6], [2.0, 0.0, -1.0]]
            .iter()
            .map(|&[x, y, z]| Vec3::new(x, y, z))
            .collect();
        let start = square_to_plane(&a, Vec3::new(0.0, 0.0, 1.0), DerivEnd::Start, 1e-5);
        assert_eq!(start.unwrap(), Some(Vec3::new(0.0, 0.0, -1.0)));
        // the end at z = -1: the second point (inward from it) is above, so the curve arrives
        // going down
        let end = square_to_plane(&a, Vec3::new(0.0, 0.0, 1.0), DerivEnd::End, 1e-5);
        assert_eq!(end.unwrap(), Some(Vec3::new(0.0, 0.0, -1.0)));
    }

    #[test]
    fn a_normal_of_any_finite_length_gives_the_same_direction() {
        // its squared length underflows to 0 at 1e-200 and overflows at 1e200
        let a = pts();
        for at in [DerivEnd::Start, DerivEnd::End] {
            let unit = square_to_plane(&a, Vec3::new(0.0, 0.0, 1.0), at, 1e-5).unwrap();
            assert!(unit.is_some(), "{at:?}");
            let subnormal = f64::from_bits(1);
            for z in [1e-200, 1e200, f64::MIN_POSITIVE, subnormal, f64::MAX] {
                let any = square_to_plane(&a, Vec3::new(0.0, 0.0, z), at, 1e-5);
                assert_eq!(any.unwrap(), unit, "{z:e} {at:?}");
            }
        }
    }

    #[test]
    fn square_to_plane_refuses_hostile_input() {
        let a = pts();
        let z = Vec3::new(0.0, 0.0, 1.0);
        for (points, normal, tol) in [
            (&a[..0], z, 1e-5),
            (&a[..], Vec3::ZERO, 1e-5),
            (&a[..], Vec3::new(f64::NAN, 0.0, 1.0), 1e-5),
            (&a[..], Vec3::new(0.0, f64::INFINITY, 1.0), 1e-5),
            (&a[..], z, 0.0),
            (&a[..], z, f64::NAN),
            (
                &[Vec3::ZERO, Vec3::new(f64::INFINITY, 0.0, 0.0)][..],
                z,
                1e-5,
            ),
        ] {
            assert!(
                matches!(
                    square_to_plane(points, normal, DerivEnd::Start, tol),
                    Err(SolidError::InvalidInput { .. })
                ),
                "{points:?} {normal:?} {tol}"
            );
        }
    }

    #[test]
    fn a_direction_of_no_length_is_refused() {
        assert!(matches!(
            interp_end_directions(&pts(), 3, Some(Vec3::ZERO), None, ParamMethod::ChordLength),
            Err(SolidError::InvalidInput { .. })
        ));
    }

    // ---- approx_end_directions ------------------------------------------------------------

    use crate::nurbs::crv_fit::p1::knot_vector_approx;

    /// A least-squares solve given data on its own model: the data carry the evaluation's
    /// rounding (`p + 1` products and sums, a few `EPSILON` times their size), and the normal
    /// equations amplify it by their condition; 2^10 allows a condition of about 2^7 over the
    /// few dozen roundings each entry takes.
    const SOLVE_ROUNDING: f64 = 1024.0 * f64::EPSILON;

    /// The normal equations' residual of a backward-stable solve: each sum carries about
    /// `2p + 1` roundings of the banded LU, the two solves, the 2 by 2 elimination, the
    /// combination of the free points and the evaluation of the residuals, each at most
    /// `EPSILON` times the size of the terms (`sum_k N_i(u_k) (sum_j N_j |P_j| + |a_k|)`); 2^8
    /// covers them with room.
    const NORMAL_ROUNDING: f64 = 256.0 * f64::EPSILON;

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    fn cpt(c: &Curve, i: usize) -> Vec3 {
        let q = c.cpts()[i];
        Vec3::new(q.x, q.y, q.z)
    }

    fn unit(d: Vec3) -> Vec3 {
        d.length_and_unit().unwrap().1
    }

    /// The cubic whose second control point is `p0 + s T_0` and last but one `pn - e T_1`.
    fn model(
        p0: Vec3,
        d0: Vec3,
        s: f64,
        inner: &[Vec3],
        pn: Vec3,
        d1: Vec3,
        e: f64,
        knots: &[f64],
    ) -> Curve {
        let mut c = vec![p0, p0 + unit(d0) * s];
        c.extend_from_slice(inner);
        c.push(pn - unit(d1) * e);
        c.push(pn);
        let pw = c.iter().map(|&q| CPoint::from_point(q)).collect();
        Curve::new(pw, 3, knots.to_vec()).unwrap()
    }

    /// `m + 1` even parameters on `[0, 1]` (the last exactly 1).
    fn even(m: usize) -> Vec<f64> {
        (0..=m).map(|k| k as f64 / m as f64).collect()
    }

    /// Points on the unit circle from angle 0 to 1.2 at the parameters of their angles, and
    /// the arc's tangents at its ends.
    fn arc(m: usize) -> (Vec<Vec3>, Vec<f64>, Vec3, Vec3) {
        let u = even(m);
        let a = u
            .iter()
            .map(|&t| v((1.2 * t).cos(), (1.2 * t).sin(), 0.0))
            .collect();
        let (s, c) = 1.2_f64.sin_cos();
        (a, u, v(0.0, 1.0, 0.0), v(-s, c, 0.0))
    }

    /// Points on the parabola `(t, t^2, 0.3 t)`, and its tangents at `t = 0` and `t = 1`.
    fn parabola(m: usize) -> (Vec<Vec3>, Vec<f64>, Vec3, Vec3) {
        let u = even(m);
        let a = u.iter().map(|&t| v(t, t * t, 0.3 * t)).collect();
        (a, u, v(1.0, 0.0, 0.3), v(1.0, 2.0, 0.3))
    }

    fn same_bits(x: &Curve, y: &Curve) -> bool {
        let points = |c: &Curve| -> Vec<[u64; 4]> {
            c.cpts()
                .iter()
                .map(|q| q.to_array().map(f64::to_bits))
                .collect()
        };
        let knots =
            |c: &Curve| -> Vec<u64> { c.knots().iter().copied().map(f64::to_bits).collect() };
        x.degree() == y.degree() && points(x) == points(y) && knots(x) == knots(y)
    }

    fn is_invalid(r: &Result<EndDirectionFit>) -> bool {
        matches!(r, Err(SolidError::InvalidInput { .. }))
    }

    fn is_no_fit(r: &Result<EndDirectionFit>) -> bool {
        matches!(r, Err(e) if matches!(e.nurbs_failure(), Some(NurbsFailure::Input)))
    }

    #[test]
    fn with_no_direction_the_fit_is_approx_knots_bit_for_bit() {
        for (a, u, _, _) in [arc(30), parabola(30)] {
            let kv = knot_vector_approx(&u, 6, 3).unwrap();
            let knt = KnotVector::from_slice(&kv).unwrap();
            let fit = approx_end_directions(&a, &u, &knt, 6, 3, None, None).unwrap();
            let plain = approx_knots(&a, &u, &knt, 6, 3).unwrap();
            assert!(same_bits(&fit.curve, &plain));
            assert_eq!((fit.start_reach, fit.end_reach), (None, None));
        }
    }

    /// Fits the model's own points at 21 even parameters with both directions, only the start
    /// and only the end: every control point and reach comes back.
    fn gives_back(model: &Curve, d0: Vec3, s: f64, d1: Vec3, e: f64) {
        let n = model.n();
        let u = even(20);
        let a: Vec<Vec3> = u
            .iter()
            .map(|&t| model.eval(t, Side::Left).unwrap())
            .collect();
        let knt = model.knot_vector().unwrap();
        let size = (0..=n)
            .map(|i| cpt(model, i).length())
            .fold(s.abs().max(e.abs()), f64::max);
        let tol = SOLVE_ROUNDING * size;
        for (start, end) in [(Some(d0), Some(d1)), (Some(d0), None), (None, Some(d1))] {
            let fit = approx_end_directions(&a, &u, &knt, n, 3, start, end).unwrap();
            assert_eq!(fit.curve.knots(), model.knots());
            for i in 0..=n {
                let off = (cpt(&fit.curve, i) - cpt(model, i)).length();
                assert!(off <= tol, "{start:?} {end:?} P{i}: {off:e} > {tol:e}");
            }
            match start {
                Some(_) => assert!((fit.start_reach.unwrap() - s).abs() <= tol),
                None => assert_eq!(fit.start_reach, None),
            }
            match end {
                Some(_) => assert!((fit.end_reach.unwrap() - e).abs() <= tol),
                None => assert_eq!(fit.end_reach, None),
            }
        }
    }

    #[test]
    fn points_of_a_cubic_bezier_on_the_two_lines_give_it_back() {
        let (d0, d1) = (v(1.0, 2.0, 0.5), v(1.0, -1.0, 0.25));
        let knots = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        let c = model(
            v(0.5, -1.0, 0.25),
            d0,
            1.5,
            &[],
            v(5.0, 1.0, 2.0),
            d1,
            2.0,
            &knots,
        );
        gives_back(&c, d0, 1.5, d1, 2.0);
    }

    #[test]
    fn points_of_a_cubic_b_spline_on_the_two_lines_give_it_back() {
        let (d0, d1) = (v(1.0, 2.0, 0.5), v(1.0, 1.0, -0.5));
        let knots = [0.0, 0.0, 0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0, 1.0, 1.0];
        let inner = [v(2.0, 2.0, 1.0), v(3.0, 0.0, 2.0), v(4.0, -1.0, 1.0)];
        let c = model(
            Vec3::ZERO,
            d0,
            0.8,
            &inner,
            v(6.0, 0.5, 0.0),
            d1,
            0.9,
            &knots,
        );
        gives_back(&c, d0, 0.8, d1, 0.9);
    }

    #[test]
    fn off_the_model_the_ends_hold_their_directions_and_the_normal_equations_hold() {
        let (n, p) = (6, 3);
        for (a, u, d0, d1) in [arc(30), parabola(30)] {
            let m = a.len() - 1;
            let kv = knot_vector_approx(&u, n, p).unwrap();
            let knt = KnotVector::from_slice(&kv).unwrap();
            for (start, end) in [(Some(d0), Some(d1)), (Some(d0), None), (None, Some(d1))] {
                let fit = approx_end_directions(&a, &u, &knt, n, p, start, end).unwrap();
                let c = &fit.curve;
                let k = c.knots();
                // through the end points exactly: the basis there is (1, 0, ..) and (.., 0, 1)
                assert_eq!(c.eval(u[0], Side::Left).unwrap(), a[0]);
                assert_eq!(c.eval(u[m], Side::Left).unwrap(), a[m]);
                // C'(u_0) = p / (U[p+1] - U[1]) (P_1 - P_0), P_1 - P_0 = s T_0 up to the
                // roundings of P_1, of the sum over the p + 1 basis derivatives and of T_0
                // itself: 16 EPSILON times the derivative's terms covers them (p = 3)
                if let Some(d) = start {
                    let dc = c.derivs(u[0], Side::Left, 1).unwrap()[1];
                    let slope = p as f64 / (k[p + 1] - k[1]);
                    let size: f64 = (0..=p).map(|i| cpt(c, i).length()).sum();
                    let off = dc.cross(unit(d)).length();
                    let tol = 16.0 * f64::EPSILON * slope * size;
                    assert!(off <= tol, "start {off:e} > {tol:e}");
                }
                if let Some(d) = end {
                    let dc = c.derivs(u[m], Side::Left, 1).unwrap()[1];
                    let slope = p as f64 / (k[n + p] - k[n]);
                    let size: f64 = (n - p..=n).map(|i| cpt(c, i).length()).sum();
                    let off = dc.cross(unit(d)).length();
                    let tol = 16.0 * f64::EPSILON * slope * size;
                    assert!(off <= tol, "end {off:e} > {tol:e}");
                }
                // the normal equations: sum_k N_i(u_k) e_k per control point, and its scale
                let mut sums = vec![Vec3::ZERO; n + 1];
                let mut scale = vec![0.0; n + 1];
                for (&uk, &ak) in u.iter().zip(&a).take(m).skip(1) {
                    let b = knt.basis_eval(p, uk, Side::Left).unwrap();
                    let first = b.span - p;
                    let ek = c.eval(uk, Side::Left).unwrap() - ak;
                    let size: f64 = b
                        .values
                        .iter()
                        .enumerate()
                        .map(|(r, nr)| nr * cpt(c, first + r).length())
                        .sum::<f64>()
                        + ak.length();
                    for (r, nr) in b.values.iter().enumerate() {
                        sums[first + r] += ek * *nr;
                        scale[first + r] += nr * size;
                    }
                }
                let lo = if start.is_some() { 2 } else { 1 };
                let hi_end = if end.is_some() { n - 1 } else { n };
                for (i, (sum, sc)) in sums.iter().zip(&scale).enumerate() {
                    let tol = NORMAL_ROUNDING * sc;
                    if (lo..hi_end).contains(&i) {
                        assert!(sum.length() <= tol, "P{i}: {:e} > {tol:e}", sum.length());
                    }
                    if let (1, Some(d)) = (i, start) {
                        let along = unit(d).dot(*sum).abs();
                        assert!(along <= tol, "start reach: {along:e} > {tol:e}");
                    }
                    if let (true, Some(d)) = (i + 1 == n, end) {
                        let along = unit(d).dot(*sum).abs();
                        assert!(along <= tol, "end reach: {along:e} > {tol:e}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_directions_length_does_not_change_the_fit() {
        // length_and_unit divides by the largest coordinate first; scaling a direction by a
        // power of two scales that coordinate and every other exactly, so the unit direction,
        // and with it the whole fit, is the same bit for bit
        for (a, u, d0, d1) in [arc(30), parabola(30)] {
            let kv = knot_vector_approx(&u, 6, 3).unwrap();
            let knt = KnotVector::from_slice(&kv).unwrap();
            for (start, end) in [(Some(d0), Some(d1)), (Some(d0), None), (None, Some(d1))] {
                let base = approx_end_directions(&a, &u, &knt, 6, 3, start, end).unwrap();
                for f in [8.0, 0.125, 1024.0] {
                    let scaled = approx_end_directions(
                        &a,
                        &u,
                        &knt,
                        6,
                        3,
                        start.map(|d| d * f),
                        end.map(|d| d * f),
                    )
                    .unwrap();
                    assert!(same_bits(&scaled.curve, &base.curve), "{f}");
                    assert_eq!(
                        (scaled.start_reach, scaled.end_reach),
                        (base.start_reach, base.end_reach)
                    );
                }
            }
        }
    }

    #[test]
    fn approx_end_directions_refuses_bad_directions_degrees_and_counts() {
        let (a, u, d0, d1) = parabola(30);
        let kv = knot_vector_approx(&u, 6, 3).unwrap();
        let knt = KnotVector::from_slice(&kv).unwrap();
        for bad in [
            Vec3::ZERO,
            v(f64::NAN, 0.0, 1.0),
            v(0.0, f64::INFINITY, 0.0),
        ] {
            for (s, e) in [(Some(bad), None), (None, Some(bad)), (Some(d0), Some(bad))] {
                let r = approx_end_directions(&a, &u, &knt, 6, 3, s, e);
                assert!(is_invalid(&r), "{s:?} {e:?}");
            }
        }
        // degree 1 with a direction
        let kv1 = knot_vector_approx(&u, 6, 1).unwrap();
        let knt1 = KnotVector::from_slice(&kv1).unwrap();
        let r = approx_end_directions(&a, &u, &knt1, 6, 1, Some(d0), None);
        assert!(is_invalid(&r));
        // n = 2 with both directions: the one inner control point cannot lie on both lines;
        // with one direction it is the reach's alone (no free control point)
        let kv2 = knot_vector_approx(&u, 2, 2).unwrap();
        let knt2 = KnotVector::from_slice(&kv2).unwrap();
        let r = approx_end_directions(&a, &u, &knt2, 2, 2, Some(d0), Some(d1));
        assert!(is_no_fit(&r));
        let one = approx_end_directions(&a, &u, &knt2, 2, 2, Some(d0), None).unwrap();
        assert!(one.start_reach.is_some() && one.end_reach.is_none());
        // too few data, knots of the wrong count, n < p
        let r = approx_end_directions(&a[..3], &u[..3], &knt, 6, 3, Some(d0), None);
        assert!(is_no_fit(&r));
        let r = approx_end_directions(&a, &u, &knt, 5, 3, Some(d0), None);
        assert!(is_no_fit(&r));
        let r = approx_end_directions(&a, &u, &knt, 2, 3, Some(d0), None);
        assert!(is_no_fit(&r));
        // no points, fewer parameters than points
        let r = approx_end_directions(&[], &u, &knt, 6, 3, Some(d0), None);
        assert!(is_invalid(&r));
        let r = approx_end_directions(&a, &u[..10], &knt, 6, 3, Some(d0), None);
        assert!(is_invalid(&r));
        // data that do not fix the free control points: every interior parameter the same
        let m = u.len() - 1;
        let same: Vec<f64> = u
            .iter()
            .enumerate()
            .map(|(k, &t)| if k == 0 || k == m { t } else { 0.5 })
            .collect();
        for (s, e) in [(Some(d0), Some(d1)), (Some(d0), None), (None, Some(d1))] {
            let r = approx_end_directions(&a, &same, &knt, 6, 3, s, e);
            assert!(is_no_fit(&r), "{s:?} {e:?}");
        }
    }

    #[test]
    fn approx_end_directions_never_panics_on_hostile_input() {
        let (a, u, d0, d1) = parabola(12);
        let kv = knot_vector_approx(&u, 6, 3).unwrap();
        let reversed: Vec<f64> = kv.iter().rev().copied().collect();
        let equal = vec![0.5; kv.len()];
        let nan_u = vec![f64::NAN; u.len()];
        let wild_u: Vec<f64> = u.iter().map(|t| t * 1e300 - 5.0).collect();
        let bad_a = vec![v(f64::INFINITY, 0.0, f64::NAN); a.len()];
        let counts = [
            (6, 3),
            (3, 2),
            (6, 0),
            (0, 0),
            (usize::MAX, 3),
            (6, usize::MAX),
            (usize::MAX, usize::MAX),
        ];
        let ends = [
            (Some(d0), Some(d1)),
            (Some(d0), None),
            (None, Some(d1)),
            (None, None),
        ];
        for knots in [&kv, &reversed, &equal] {
            let knt = KnotVector::from_slice(knots).unwrap();
            for (pts, par) in [(&a, &u), (&a, &nan_u), (&a, &wild_u), (&bad_a, &u)] {
                for &(n, p) in &counts {
                    for &(s, e) in &ends {
                        let _ = approx_end_directions(pts, par, &knt, n, p, s, e);
                    }
                }
            }
        }
    }
}
