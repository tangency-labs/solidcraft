//! Global interpolation of points with a direction given at either end (not upstream): the
//! curve through the points whose tangent at each given end runs along that direction. A half
//! section whose end at a mirror plane runs square to it then joins its mirror smoothly.
//!
//! The direction becomes the end derivative the interpolation takes ([`interp_knots_and_derivs`],
//! [`interp_knots_and_deriv`]: The NURBS Book, §9.2.2), its length the data polygon's length,
//! the magnitude the book suggests for parameters on `[0, 1]` (what upstream's own estimate of a
//! missing end derivative scales by). The knots are upstream's for that system
//! ([`knot_vector_end_derivs`], [`knot_vector_deriv`]).

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
    fn a_direction_of_no_length_is_refused() {
        assert!(matches!(
            interp_end_directions(&pts(), 3, Some(Vec3::ZERO), None, ParamMethod::ChordLength),
            Err(SolidError::InvalidInput { .. })
        ));
    }
}
