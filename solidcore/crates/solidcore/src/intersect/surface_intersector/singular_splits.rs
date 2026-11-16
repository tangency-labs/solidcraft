//! Splits traced crossing curves where they pass through a singular point of the
//! intersection (opt-in, WR-77).
//!
//! Upstream: the SolidCraft fork's `source/SMLib/src/SmSurfaceIntersector.cpp`
//! (tangency-labs/smlib@3c9d1d3, replayed on 1.0.1 as 1ff9264; unchanged at 7ac037e);
//! not in the pinned SMLib.
//!
//! | Rust                               | Upstream (fork)                    |
//! |------------------------------------|------------------------------------|
//! | [`split_curves_at_singular_points`] | `sm_SplitCurvesAtSingularPoints`  |
//! | `singular_sine` (private)          | `sm_SingularSine`                  |
//!
//! Deviations: the curve arrays are `Vec`s whose UV curves are never `NULL` (the fork's
//! `NULL` skip is the arrays' length check). A failed evaluation in `sm_SingularSine`
//! (`-1`) still propagates [`SolidError::Unported`]; so does a 3D `CreateBySplitCurve`,
//! whose other failures skip that split as the fork's do. The tails of one curve are
//! collected and appended after its splits, in the fork's order (nothing reads the
//! arrays in between). No recursion; the golden-section loop is bounded at 80 steps as
//! the fork's is.

use crate::curve::curve::{Curve, CurveKind};
use crate::error::{ignored, Result, SolidError};
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::sqrt;
use crate::kernel::Kernel;
use crate::math::EFF_ZERO;
use crate::nurbs::knots::Side;
use crate::surface::surface::p2::TsectCurveType;
use crate::surface::surface::Surface;

const OP: &str = "intersect::surface_intersector::split_curves_at_singular_points";

/// `sm_SingularSine`: `|n1 x n2|` (the sine of the angle between the unit surface
/// normals) at the parameter `t` the 3D and UV curves share; `-1` when an evaluation
/// fails or a normal is shorter than `SM_EFF_ZERO`.
fn singular_sine(
    k: &Kernel,
    srf1: &Surface,
    srf2: &Surface,
    uv1: &Curve,
    uv2: &Curve,
    t: f64,
) -> Result<f64> {
    let Some(p1) = ignored(uv1.evaluate_point(k, t))? else {
        return Ok(-1.0);
    };
    let Some(p2) = ignored(uv2.evaluate_point(k, t))? else {
        return Ok(-1.0);
    };
    let Some(n1) =
        ignored(srf1.evaluate_normal(k, Point2::new(p1.x, p1.y), Side::Left, Side::Left))?
    else {
        return Ok(-1.0);
    };
    let Some(n2) =
        ignored(srf2.evaluate_normal(k, Point2::new(p2.x, p2.y), Side::Left, Side::Left))?
    else {
        return Ok(-1.0);
    };
    let (l1, l2) = (n1.length(), n2.length());
    if l1 < EFF_ZERO || l2 < EFF_ZERO {
        return Ok(-1.0);
    }
    Ok(n1.cross(n2).length() / (l1 * l2))
}

fn at(v: &[f64], i: usize) -> Result<f64> {
    v.get(i)
        .copied()
        .ok_or_else(|| SolidError::invalid_input(OP, "sample index out of range"))
}

/// `sm_SplitCurvesAtSingularPoints`: splits every crossing or near-tangent B-spline
/// curve (and its two UV curves) where the surface normals turn parallel inside it:
/// `s(t) = |n1 x n2|` is sampled 8 times per knot span, each sharp local minimum is
/// refined by golden section, and the curve is split where `s < 1e-6`, the angle rises
/// above `1e-3` again both between the previous split (or the start) and the point and
/// after it, and the point lies more than `10 * tol_3d` from both ends. The leading part
/// stays in place; each tail is appended to the arrays with the curve's type and
/// deviation.
///
/// # Errors
///
/// Those of `GetKnots`, the curve evaluations, `IsDegenerate` and the UV curves'
/// `CreateBySplitCurve` (`SER`); [`SolidError::Unported`] from any evaluation.
#[allow(clippy::too_many_arguments)] // the fork's five curve arrays
pub(crate) fn split_curves_at_singular_points(
    k: &Kernel,
    srf1: &Surface,
    srf2: &Surface,
    tol_3d: f64,
    curves_3d: &mut Vec<Curve>,
    uv1_curves: &mut Vec<Curve>,
    uv2_curves: &mut Vec<Curve>,
    curve_types: &mut Vec<TsectCurveType>,
    deviations: &mut Vec<f64>,
) -> Result<()> {
    const SAMPLES_PER_SPAN: usize = 8;
    let golden = 0.5 * (sqrt(5.0) - 1.0);

    let count = curves_3d.len();
    for ii in 0..count {
        let (Some(c3), Some(uv1), Some(uv2)) =
            (curves_3d.get(ii), uv1_curves.get(ii), uv2_curves.get(ii))
        else {
            continue;
        };
        let curve_type = curve_types
            .get(ii)
            .copied()
            .ok_or_else(|| SolidError::invalid_input(OP, "curve types shorter than curves"))?;
        if curve_type != TsectCurveType::Crossing && curve_type != TsectCurveType::NearTangent {
            continue;
        }
        if !matches!(c3.kind(), CurveKind::BSpline(_)) || c3.is_degenerate(k, EFF_ZERO, None)? {
            continue;
        }

        // sample s(t) on every knot span
        let (knots, _) = c3.knots(k, None)?;
        if knots.len() < 2 {
            continue;
        }
        let mut ts = Vec::new();
        let mut ss = Vec::new();
        for span in knots.windows(2) {
            let (k0, k1) = (at(span, 0)?, at(span, 1)?);
            for jj in 0..SAMPLES_PER_SPAN {
                let t = k0 + (k1 - k0) * (jj as f64) / (SAMPLES_PER_SPAN as f64);
                ts.push(t);
                ss.push(singular_sine(k, srf1, srf2, uv1, uv2, t)?);
            }
        }
        let last_knot = at(&knots, knots.len() - 1)?;
        ts.push(last_knot);
        ss.push(singular_sine(k, srf1, srf2, uv1, uv2, last_knot)?);

        let start = c3.evaluate_point(k, at(&ts, 0)?)?;
        let end = c3.evaluate_point(k, at(&ts, ts.len() - 1)?)?;

        let mut splits = Vec::new();
        let mut last_split_index = 0; // sample index of the previous split (or the curve start)
        for kk in 1..ss.len().saturating_sub(1) {
            let (s_prev, s_k, s_next) = (at(&ss, kk - 1)?, at(&ss, kk)?, at(&ss, kk + 1)?);
            if s_k < 0.0 || s_prev < 0.0 || s_next < 0.0 {
                continue;
            }
            if !(s_k <= s_prev && s_k < s_next) {
                continue;
            }
            // golden section refine on [t(k-1), t(k+1)]
            let (mut a, mut b) = (at(&ts, kk - 1)?, at(&ts, kk + 1)?);
            let mut c = b - golden * (b - a);
            let mut d = a + golden * (b - a);
            let mut fc = singular_sine(k, srf1, srf2, uv1, uv2, c)?;
            let mut fd = singular_sine(k, srf1, srf2, uv1, uv2, d)?;
            let mut it = 0;
            while it < 80 && b - a > EFF_ZERO * (1.0 + a.abs()) {
                if fc < fd {
                    b = d;
                    d = c;
                    fd = fc;
                    c = b - golden * (b - a);
                    fc = singular_sine(k, srf1, srf2, uv1, uv2, c)?;
                } else {
                    a = c;
                    c = d;
                    fc = fd;
                    d = a + golden * (b - a);
                    fd = singular_sine(k, srf1, srf2, uv1, uv2, d)?;
                }
                it += 1;
            }
            let t_star = 0.5 * (a + b);
            let s_star = singular_sine(k, srf1, srf2, uv1, uv2, t_star)?;
            let p = c3.evaluate_point(k, t_star)?;
            // the curve must really cross the point: the intersection angle has to rise
            // again between the previous split (or the curve start) and this point and
            // between this point and the curve end
            let rise = |range: core::ops::Range<usize>| {
                ss.get(range)
                    .unwrap_or_default()
                    .iter()
                    .fold(0.0_f64, |m, &s| if s > m { s } else { m })
            };
            let rise_before = rise(last_split_index..kk);
            let rise_after = rise(kk + 1..ss.len());
            // parallel to numerical precision, a true crossing on both sides, away from
            // the curve ends
            if (0.0..1.0e-6).contains(&s_star)
                && rise_before > 1.0e-3
                && rise_after > 1.0e-3
                && p.distance(start) > 10.0 * tol_3d
                && p.distance(end) > 10.0 * tol_3d
            {
                splits.push(t_star);
                last_split_index = kk;
            }
        }

        // split from the last parameter down: the curve keeps the leading part, each new
        // curve is a tail
        let mut tails = Vec::new();
        for &split in splits.iter().rev() {
            let (Some(c3), Some(uv1), Some(uv2)) = (
                curves_3d.get_mut(ii),
                uv1_curves.get_mut(ii),
                uv2_curves.get_mut(ii),
            ) else {
                break;
            };
            let Some(tail_3) = ignored(c3.create_by_split_curve(k, split))? else {
                continue;
            };
            let tail_uv1 = uv1.create_by_split_curve(k, split)?;
            let tail_uv2 = uv2.create_by_split_curve(k, split)?;
            tails.push((tail_3, tail_uv1, tail_uv2));
        }
        if tails.is_empty() {
            continue;
        }
        let deviation = deviations
            .get(ii)
            .copied()
            .ok_or_else(|| SolidError::invalid_input(OP, "deviations shorter than curves"))?;
        for (tail_3, tail_uv1, tail_uv2) in tails {
            curves_3d.push(tail_3);
            uv1_curves.push(tail_uv1);
            uv2_curves.push(tail_uv2);
            curve_types.push(curve_type);
            deviations.push(deviation);
        }
    }
    Ok(())
}
