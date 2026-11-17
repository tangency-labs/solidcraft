//! Splits traced crossing curves where they pass through a singular point of the
//! intersection (WR-77).
//!
//! Upstream: the SolidCraft fork's `source/SMLib/src/SmSurfaceIntersector.cpp`
//! (tangency-labs/smlib@3c9d1d3, replayed on 1.0.1 as 1ff9264; unchanged at 7ac037e);
//! not in the pinned SMLib.
//!
//! | Rust                               | Upstream (fork)                    |
//! |------------------------------------|------------------------------------|
//! | [`split_curves_at_singular_points`] | `sm_SplitCurvesAtSingularPoints`  |
//! | `singular_sine` (private)          | `sm_SingularSine`                  |
//! | `piece_deviation` (private)        | (solidcore's: certified piece deviation) |
//!
//! Deviations: the curve arrays are `Vec`s whose UV curves are never `NULL` (the fork's
//! `NULL` skip is the arrays' length check). A failed evaluation in `sm_SingularSine`
//! (`-1`) still propagates [`SolidError::Unported`]; so does a 3D `CreateBySplitCurve`,
//! whose other failures skip that split as the fork's do. The tails of one curve are
//! collected and appended after its splits, in the fork's order (nothing reads the
//! arrays in between). No recursion; the golden-section loop is bounded at 80 steps as
//! the fork's is. Each piece's deviation is certified against its parent
//! (`piece_deviation`) instead of copied: it grows by the bound on the move of `Trim`'s
//! end-span cleanup, and stays the parent's when that move is at most
//! `EFF_ZERO (1 + scale)` (rounding, or a cleanup below that size); a curve whose
//! reshaped piece cannot be bounded is left unsplit.

use crate::blend::fillet_section::interval::Ivl;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::{ignored, Result, SolidError};
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::sqrt;
use crate::kernel::Kernel;
use crate::math::EFF_ZERO;
use crate::nurbs::b_spline_curve_n_lib::piece_moves_at_most;
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

/// Not in the fork: the deviation of `piece`, a piece split from the 3D curve `parent`
/// of deviation `deviation`. The split ([`Curve::create_by_split_curve`], `Trim`) may drop
/// an end span shorter than the zone tolerance (`N_CrvRemoveDegenSegs`), which reshapes
/// the piece; [`piece_moves_at_most`] bounds, at every parameter, how far the piece lies
/// from the parent, and the deviation grows by that bound, rounded up. A move up to
/// `EFF_ZERO (1 + max |coordinate|)` is not recorded, and the deviation stays the parent's
/// (bit for bit, as the fork copies it): that is the split's rounding, but it can also be
/// a real cleanup of an end span shorter than that (a cut within about `1e-13` of a knot),
/// so the recorded deviation may understate the move by up to that amount. `None` when
/// the piece cannot be bounded (it is rational or not a B-spline) and it lacks a knot of
/// the parent inside its interval: it may be reshaped by an unknown amount. A piece that
/// keeps every parent knot inside its interval keeps the parent's deviation without a
/// certificate. That is argued from `N_CrvRemoveDegenSegs`, not checked: the cleanup
/// rescales or drops the interior knot bounding the span it drops, so a reshaped piece
/// loses a parent knot value, and the knot-end trim is a pure restriction.
///
/// The deviation is the 3D curve's only: the UV curves' move under the same cleanup is
/// not recorded (as for the joins of `join_through_tangent_points`, #17210).
fn piece_deviation(parent: &Curve, piece: &Curve, deviation: f64) -> Option<f64> {
    let (CurveKind::BSpline(whole), CurveKind::BSpline(part)) = (parent.kind(), piece.kind())
    else {
        return None;
    };
    let whole_nurb = whole.nurb.as_ref()?;
    let scale = whole_nurb.cpts().iter().fold(0.0_f64, |m, c| {
        m.max(c.x.abs()).max(c.y.abs()).max(c.z.abs())
    });
    match piece_moves_at_most(whole, part) {
        Some(moved) if moved <= EFF_ZERO * (1.0 + scale) => Some(deviation),
        Some(moved) => {
            let grown = (Ivl::from(deviation) + Ivl::from(moved)).hi;
            grown.is_finite().then_some(grown)
        }
        None => {
            let part_nurb = part.nurb.as_ref()?;
            let (a, b) = (*part_nurb.knots().first()?, *part_nurb.knots().last()?);
            let count = |u: &[f64], t: f64| u.iter().filter(|&&x| x == t).count();
            let keeps_all = whole_nurb
                .knots()
                .iter()
                .filter(|&&t| a < t && t < b)
                .all(|&t| count(part_nurb.knots(), t) >= count(whole_nurb.knots(), t));
            keeps_all.then_some(deviation)
        }
    }
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
/// stays in place; each tail is appended to the arrays with the curve's type. Every
/// piece's deviation is `piece_deviation`'s: the parent's, grown by the certified move of
/// any end-span cleanup (3D only; the UV curves' move is not recorded).
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
        if splits.is_empty() {
            continue;
        }
        // the parents stay as they are, for the pieces' deviations and to be put back; the
        // splits work on exact copies (`clone_for_evaluation`: no copy-constructor knot
        // fix-up, so the copies are the parents bit for bit)
        let (Some(c3), Some(uv1), Some(uv2)) = (
            curves_3d.get_mut(ii),
            uv1_curves.get_mut(ii),
            uv2_curves.get_mut(ii),
        ) else {
            continue;
        };
        let (work_3, work_uv1, work_uv2) = (
            c3.clone_for_evaluation()?,
            uv1.clone_for_evaluation()?,
            uv2.clone_for_evaluation()?,
        );
        let parent_3 = core::mem::replace(c3, work_3);
        let parent_uv1 = core::mem::replace(uv1, work_uv1);
        let parent_uv2 = core::mem::replace(uv2, work_uv2);
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
        // each piece's deviation, certified against its parent (solidcore's, not the
        // fork's copy): `None` undoes this curve's splits
        let lead_dev = curves_3d
            .get(ii)
            .and_then(|lead| piece_deviation(&parent_3, lead, deviation));
        let tail_devs: Option<Vec<f64>> = tails
            .iter()
            .map(|(tail_3, _, _)| piece_deviation(&parent_3, tail_3, deviation))
            .collect();
        let (Some(lead_dev), Some(tail_devs)) = (lead_dev, tail_devs) else {
            if let (Some(c3), Some(uv1), Some(uv2)) = (
                curves_3d.get_mut(ii),
                uv1_curves.get_mut(ii),
                uv2_curves.get_mut(ii),
            ) {
                *c3 = parent_3;
                *uv1 = parent_uv1;
                *uv2 = parent_uv2;
            }
            continue;
        };
        if let Some(d) = deviations.get_mut(ii) {
            *d = lead_dev;
        }
        for ((tail_3, tail_uv1, tail_uv2), tail_dev) in tails.into_iter().zip(tail_devs) {
            curves_3d.push(tail_3);
            uv1_curves.push(tail_uv1);
            uv2_curves.push(tail_uv2);
            curve_types.push(curve_type);
            deviations.push(tail_dev);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::b_spline_curve::BSplineCurve;
    use crate::nurbs::crv_basic::Curve as Nurb;
    use crate::nurbs::math::{CPoint, NOW, NOZ};

    /// A cubic with one interior knot, at 1; rational (one weight 2) when asked.
    fn parent(rational: bool) -> Curve {
        let w = if rational { 1.0 } else { NOW };
        let pw = vec![
            CPoint::new(0.0, 0.0, 0.0, w),
            CPoint::new(1.0, 2.0, 0.5, w),
            if rational {
                // homogeneous: (3, 3, -1) at weight 2
                CPoint::new(6.0, 6.0, -2.0, 2.0)
            } else {
                CPoint::new(3.0, 3.0, -1.0, w)
            },
            CPoint::new(4.0, 1.0, 2.0, w),
            CPoint::new(6.0, 0.0, 1.0, w),
        ];
        let u = vec![0.0, 0.0, 0.0, 0.0, 1.0, 2.5, 2.5, 2.5, 2.5];
        Curve::from(BSplineCurve::from_gw_nurb(3, &Nurb::new(pw, 3, u).unwrap()))
    }

    fn knots_of(c: &Curve) -> Vec<f64> {
        let CurveKind::BSpline(b) = c.kind() else {
            panic!("not a B-spline")
        };
        b.nurb.as_ref().unwrap().knots().to_vec()
    }

    /// The largest distance, at 2001 parameters of the piece, between it and its parent.
    fn sampled(k: &Kernel, whole: &Curve, piece: &Curve) -> f64 {
        let u = knots_of(piece);
        let (a, b) = (u[0], u[u.len() - 1]);
        (0..=2000)
            .map(|j| {
                let t = a + (b - a) * f64::from(j) / 2000.0;
                let p = whole.evaluate_point(k, t).unwrap();
                p.distance(piece.evaluate_point(k, t).unwrap())
            })
            .fold(0.0_f64, f64::max)
    }

    /// Pieces cut away from a knot are the parent's restriction to rounding and keep its
    /// deviation bit for bit; a piece whose end span (shorter than the zone tolerance)
    /// the split's `Trim` removes is reshaped, and its deviation grows by at least the
    /// move.
    #[test]
    fn a_split_piece_s_deviation_covers_its_move_from_the_parent() {
        let k = Kernel::default();
        let dev = 0.25;
        let mut lead = parent(false);
        let tail = lead.create_by_split_curve(&k, 1.7).unwrap();
        let whole = parent(false);
        assert_eq!(piece_deviation(&whole, &lead, dev), Some(dev));
        assert_eq!(piece_deviation(&whole, &tail, dev), Some(dev));

        let mut lead = parent(false);
        let tail = lead.create_by_split_curve(&k, 1.0 - 1e-7).unwrap();
        // the cleanup ran: the tail lost the knot at 1
        assert!(!knots_of(&tail).contains(&1.0), "{:?}", knots_of(&tail));
        let moved = sampled(&k, &whole, &tail);
        assert!(moved > 0.0, "the tail did not move");
        let got = piece_deviation(&whole, &tail, dev).unwrap();
        assert!(
            got >= dev + moved && got <= dev + 1e-4,
            "{got} vs {dev} + {moved}"
        );
    }

    /// A rational piece is not bounded: it keeps the parent's deviation when it holds every
    /// parent knot inside its interval, and has none (its curve stays unsplit) when the
    /// split's cleanup could have dropped one (it does not run on this rational tail, so the
    /// dropped-knot piece is built directly).
    #[test]
    fn a_rational_piece_is_kept_only_as_the_parent_s_restriction() {
        let k = Kernel::default();
        let whole = parent(true);
        let mut lead = parent(true);
        let tail = lead.create_by_split_curve(&k, 1.7).unwrap();
        assert_eq!(piece_deviation(&whole, &lead, 0.25), Some(0.25));
        assert_eq!(piece_deviation(&whole, &tail, 0.25), Some(0.25));
        // a rational piece over [1 - 1e-7, 2.5] without the parent's knot at 1, as a
        // cleanup would leave it
        let a = 1.0 - 1e-7;
        let pw = vec![
            CPoint::new(2.0, 2.0, 0.0, 1.0),
            CPoint::new(6.0, 6.0, -2.0, 2.0),
            CPoint::new(4.0, 1.0, 2.0, 1.0),
            CPoint::new(6.0, 0.0, 1.0, 1.0),
        ];
        let u = vec![a, a, a, a, 2.5, 2.5, 2.5, 2.5];
        let cleaned = Curve::from(BSplineCurve::from_gw_nurb(3, &Nurb::new(pw, 3, u).unwrap()));
        assert_eq!(piece_deviation(&whole, &cleaned, 0.25), None);
    }

    /// A non-rational bilinear patch over `[0, 1]^2`: `x = u`, `y = v`, `z` from the rows
    /// `zs` (degree `zs.len() - 1` in `u`).
    fn patch(zs: &[f64]) -> Surface {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::srf_basic::Surface as Patch;
        let p = zs.len() - 1;
        let net = zs
            .iter()
            .enumerate()
            .map(|(i, &z)| {
                let x = i as f64 / p as f64;
                vec![CPoint::new(x, 0.0, z, NOW), CPoint::new(x, 1.0, z, NOW)]
            })
            .collect();
        let mut u = vec![0.0; p + 1];
        u.extend(vec![1.0; p + 1]);
        let net = Patch::new(net, p, 1, u, vec![0.0, 0.0, 1.0, 1.0]).unwrap();
        Surface::from(BSplineSurface::from_gw_nurb(&net))
    }

    /// The straight UV curve from `a` to `b` over `[0, 2.5]`.
    fn uv_line(a: (f64, f64), b: (f64, f64)) -> Curve {
        let pw = [
            CPoint::new(a.0, a.1, NOZ, NOW),
            CPoint::new(b.0, b.1, NOZ, NOW),
        ];
        Curve::from(BSplineCurve::from_data(2, 1, 3, 1, &pw, &[0.0, 0.0, 2.5, 2.5]).unwrap())
    }

    /// A refused split puts the parents back exactly. The plane z = 0 and the trough
    /// `z = (2u - 1)^2` have parallel normals at u = 1/2, which the second UV curve
    /// reaches at t = 1.25, so the crossing is split there. Its 3D curve is rational
    /// (unit weights) and its first span [0, 1] collapses to a pole at the start, so the
    /// split's `Trim` cleans that span off the lead piece and drops the parent's knot
    /// at 1. A rational piece cannot be bounded (`piece_deviation` is `None`) and the curve
    /// is left unsplit: the three parents must come back with their knots, control
    /// points, owner and recorded edge tolerance. A `Clone` (the copy constructor) drops
    /// the owner and the tolerance and merges the first UV curve's knots 1e-14 apart,
    /// so this fails if the parents are kept as `Clone`s. The same curve non-rational is
    /// bounded, and its split at 1.25 stands.
    #[test]
    fn a_refused_singular_split_restores_the_parents_exactly() {
        let k = Kernel::default();
        let plane = patch(&[0.0, 0.0]);
        let trough = patch(&[1.0, -1.0, 1.0]);
        // P0..P3 lie within 1e-7: the span [0, 1] collapses to a pole at the start
        let pw = vec![
            CPoint::new(0.0, 0.0, 0.0, 1.0),
            CPoint::new(1e-8, 2e-8, 0.5e-8, 1.0),
            CPoint::new(3e-8, 3e-8, -1e-8, 1.0),
            CPoint::new(4e-8, 1e-8, 2e-8, 1.0),
            CPoint::new(6.0, 0.0, 1.0, 1.0),
        ];
        let u = vec![0.0, 0.0, 0.0, 0.0, 1.0, 2.5, 2.5, 2.5, 2.5];
        let mut c3 = Curve::from(BSplineCurve::from_gw_nurb(3, &Nurb::new(pw, 3, u).unwrap()));
        let edge = crate::geom::object::ObjectRef {
            type_code: 16016, // SM_TYPE of an edge
            key: crate::geom::attribute::ObjectKey(7),
        };
        let _ = c3.set_owner(Some(edge), None);
        c3.set_owner_zone_tol_3d(Some(3e-6));
        // a knot pair 1e-14 apart, which the copy constructor (`Clone`) would merge
        let mut b = BSplineCurve::from_data(
            2,
            3,
            5,
            1,
            &[0.0, 1.0, 1.0 + 1e-14, 2.5].map(|t| {
                let x = 0.2 + 0.6 * t / 2.5;
                CPoint::new(x, x, NOZ, NOW)
            }),
            &[0.0, 0.0, 1.0, 1.0, 2.5, 2.5],
        )
        .unwrap();
        b.get_or_create_gw_nurb()
            .unwrap()
            .knots_mut()
            .copy_from_slice(&[0.0, 0.0, 1.0, 1.0 + 1e-14, 2.5, 2.5]);
        let mut uv1 = Curve::from(b);
        let _ = uv1.set_owner(Some(edge), None);
        assert_ne!(knots_of(&uv1.clone()), knots_of(&uv1));
        let uv2 = uv_line((0.0, 0.5), (1.0, 0.5));

        let mut curves_3d = vec![c3.clone_for_evaluation().unwrap()];
        let mut uv1_curves = vec![uv1.clone_for_evaluation().unwrap()];
        let mut uv2_curves = vec![uv2.clone_for_evaluation().unwrap()];
        let mut curve_types = vec![TsectCurveType::Crossing];
        let mut deviations = vec![0.25];
        split_curves_at_singular_points(
            &k,
            &plane,
            &trough,
            1e-12,
            &mut curves_3d,
            &mut uv1_curves,
            &mut uv2_curves,
            &mut curve_types,
            &mut deviations,
        )
        .unwrap();

        // the split at 1.25 makes a lead piece without the knot at 1, which is refused
        let mut lead = c3.clone_for_evaluation().unwrap();
        let tail = lead.create_by_split_curve(&k, 1.25).unwrap();
        assert!(!knots_of(&lead).contains(&1.0), "{:?}", knots_of(&lead));
        assert_eq!(piece_deviation(&c3, &lead, 0.25), None);
        assert_eq!(piece_deviation(&c3, &tail, 0.25), Some(0.25));

        assert_eq!(
            (curves_3d.len(), uv1_curves.len(), uv2_curves.len()),
            (1, 1, 1)
        );
        assert_eq!(
            (curve_types, deviations),
            (vec![TsectCurveType::Crossing], vec![0.25])
        );
        for (got, want) in [
            (&curves_3d[0], &c3),
            (&uv1_curves[0], &uv1),
            (&uv2_curves[0], &uv2),
        ] {
            let (CurveKind::BSpline(g), CurveKind::BSpline(w)) = (got.kind(), want.kind()) else {
                panic!("not a B-spline")
            };
            let (g, w) = (g.nurb.as_ref().unwrap(), w.nurb.as_ref().unwrap());
            assert_eq!(g.knots(), w.knots());
            assert_eq!(g.cpts(), w.cpts());
            assert_eq!(got.owner(), want.owner());
            assert_eq!(got.owner_zone_tol_3d(), want.owner_zone_tol_3d());
        }
        assert_eq!(curves_3d[0].owner(), Some(edge));
        assert_eq!(curves_3d[0].owner_zone_tol_3d(), Some(3e-6));
        assert_eq!(knots_of(&uv1_curves[0])[3], 1.0 + 1e-14);

        // the same curve, non-rational: its pieces are bounded and the split stands
        let CurveKind::BSpline(b) = c3.kind() else {
            panic!("not a B-spline")
        };
        let pw = b.nurb.as_ref().unwrap().cpts().iter();
        let pw = pw.map(|c| CPoint::new(c.x, c.y, c.z, NOW)).collect();
        let u = knots_of(&c3);
        let polynomial = Curve::from(BSplineCurve::from_gw_nurb(3, &Nurb::new(pw, 3, u).unwrap()));
        let mut curves_3d = vec![polynomial];
        let mut uv1_curves = vec![uv1.clone_for_evaluation().unwrap()];
        let mut uv2_curves = vec![uv2];
        let mut curve_types = vec![TsectCurveType::Crossing];
        let mut deviations = vec![0.25];
        split_curves_at_singular_points(
            &k,
            &plane,
            &trough,
            1e-12,
            &mut curves_3d,
            &mut uv1_curves,
            &mut uv2_curves,
            &mut curve_types,
            &mut deviations,
        )
        .unwrap();
        assert_eq!(curves_3d.len(), 2);
        let (lead, tail) = (knots_of(&curves_3d[0]), knots_of(&curves_3d[1]));
        assert!((lead[lead.len() - 1] - 1.25).abs() < 1e-6, "{lead:?}");
        assert!((tail[0] - 1.25).abs() < 1e-6, "{tail:?}");
    }
}
