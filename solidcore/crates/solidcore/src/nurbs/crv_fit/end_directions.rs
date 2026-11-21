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

use crate::error::{Result, SolidError};
use crate::math::Vec3;
use crate::nurbs::crv_basic::Curve;
use crate::nurbs::crv_fit::p1::{
    interp_knots_and_deriv, interp_knots_and_derivs, knot_vector_deriv, knot_vector_end_derivs,
    param_values, DerivEnd, ParamMethod,
};
use crate::nurbs::crv_fit::p2::interp;
use crate::nurbs::knots::KnotVector;

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
}
