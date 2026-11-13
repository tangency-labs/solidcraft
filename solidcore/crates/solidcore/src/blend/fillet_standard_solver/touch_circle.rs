//! A variable round as the union of its balls (opt-in, not upstream): the knots where a
//! round's curves turn.
//!
//! Not in the pinned upstream: the SolidCraft fork's `source/SMLib/src/SmFilletSection.cpp`
//! (`sm_FilletCurveCorners`, from tangency-labs/smlib@6eecf04), called by
//! `SmGnSectionFSG::CircleSurface`, the fork's circular section (WR-685, #2250).
//!
//! | Rust                      | Fork (`SmFilletSection.cpp`) |
//! |---------------------------|------------------------------|
//! | [`fillet_curve_corners`]  | `sm_FilletCurveCorners`      |
//!
//! Deviations: the out-param `rCorners` is a return value. Every loop is bounded by the
//! rails' knots.

use crate::error::{Result, SolidError};
use crate::math::{Vec3, EFF_ZERO_SQ, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve::BSplineCurve;

const OP_CORNERS: &str = "blend::fillet_standard_solver::touch_circle::fillet_curve_corners";

/// Position and first derivative of `c` at `t`.
fn pv(c: &BSplineCurve, t: f64, from_left: bool) -> Result<(Vec3, Vec3)> {
    let v = c.evaluate(t, 1, from_left, false)?;
    match *v.as_slice() {
        [p, d, ..] => Ok((p, d)),
        _ => Err(SolidError::blend_failed(
            OP_CORNERS,
            "the curve has no first derivative",
        )),
    }
}

/// `sm_FilletCurveCorners`: the interior knots of the first rail, ascending, where the
/// centre line or a rail turns, its tangents before and after the knot differing by more
/// than `ang_tol_rad`. The three curves share their knots; a turn comes from a corner in
/// the radius.
///
/// # Errors
///
/// Those of `GetKnots`, `Evaluate` and `AngleBetween`.
pub fn fillet_curve_corners(
    center_line: &BSplineCurve,
    rail1: &BSplineCurve,
    rail2: &BSplineCurve,
    ang_tol_rad: f64,
) -> Result<Vec<f64>> {
    let mut corners = Vec::new();
    let (knots, _) = rail1.knots(None)?;
    let ivl = rail1.natural_interval();
    let tol = EFF_ZERO_SQRT * (1.0 + ivl.min().abs() + ivl.max().abs());
    let curves = [center_line, rail1, rail2];
    let mut previous: Option<f64> = None;
    for &knot in &knots {
        let repeated = previous == Some(knot);
        previous = Some(knot);
        if repeated || knot < ivl.min() + tol || knot > ivl.max() - tol {
            continue;
        }
        let mut corner = false;
        for c in curves {
            if corner {
                break;
            }
            if !c.natural_interval().contains_value(knot, 0.0) {
                continue;
            }
            let (_, before) = pv(c, knot, false)?;
            let (_, after) = pv(c, knot, true)?;
            if before.length_squared() < EFF_ZERO_SQ || after.length_squared() < EFF_ZERO_SQ {
                continue;
            }
            if before.angle_between(after)? > ang_tol_rad {
                corner = true;
            }
        }
        if corner {
            corners.push(knot);
        }
    }
    Ok(corners)
}

/// The angle above which a knot is a corner in `SmGnSectionFSG::CircleSurface`:
/// `smos_Min(GetThisAngTolRad(), SM_DEG2RAD(SM_CONTINUITY_ANGLE))`.
#[must_use]
pub fn corner_angle(ang_tol_rad: f64) -> f64 {
    let continuity = crate::geom::tol::ANG_TOL_DEG * (crate::math::PI / 180.0);
    if continuity < ang_tol_rad {
        continuity
    } else {
        ang_tol_rad
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corner_angle_is_the_smaller() {
        assert_eq!(corner_angle(1.0), 0.5 * (crate::math::PI / 180.0));
        assert_eq!(corner_angle(1e-3), 1e-3);
    }
}
