//! Free curves: a B-spline from the control points a designer places, one of its control points
//! moved, and its curvature comb. The curve is the record's ([`Curve`]); the knots and the
//! curvature are the kernel's ([`solidcore::nurbs::knots::clamped_uniform`],
//! [`solidcore::query::continuity::curve_comb`]). Ops checks the inputs and converts.

use solidcore::curve::from_parameters::from_parameters;
use solidcore::nurbs::knots::{clamped_uniform, MAX_DEGREE};
use solidcore::query::continuity;
use solidcore::SolidError;
use solidcore_record::brep::Curve;

use crate::continuity::{tooth, CombTooth, MAX_COMB_TEETH, MIN_COMB_TEETH};
use crate::session::run;
use crate::sweep::{coord_ok, CURVE_MAX_POINTS};
use crate::{Error, Result};

/// The clamped B-spline with `points` as its control points, of `degree` (at most one less than
/// the number of points: two points make a line, three a quadratic), on `[0, 1]` with the
/// kernel's uniform knots. It passes through its first and last point.
///
/// # Errors
/// [`Error::Argument`] for fewer than two points, more than [`CURVE_MAX_POINTS`], a coordinate
/// that is not finite or past the coordinate bound, or a degree of 0 or past the kernel's most.
pub fn cv_curve(points: &[[f64; 3]], degree: usize) -> Result<Curve> {
    if points.len() < 2 || points.len() > CURVE_MAX_POINTS || !points.iter().all(|p| coord_ok(p)) {
        return Err(Error::Argument);
    }
    if degree == 0 || degree > MAX_DEGREE {
        return Err(Error::Argument);
    }
    let degree = degree.min(points.len() - 1);
    let knots = clamped_uniform(points.len(), degree).map_err(|_| Error::Argument)?;
    Ok(Curve::Spline {
        knots,
        points: points.to_vec(),
    })
}

/// `curve` with its control point `index` at `to`: its knots, degree and every other point as
/// they were. A spline's or a polyline's point.
///
/// # Errors
/// [`Error::Argument`] for another kind of curve (a rational spline's point carries its weight),
/// an index past its points, or a point that is not finite or past the coordinate bound.
pub fn move_cv(curve: &Curve, index: usize, to: [f64; 3]) -> Result<Curve> {
    if !coord_ok(&to) {
        return Err(Error::Argument);
    }
    let mut moved = curve.clone();
    let (Curve::Spline { points, .. } | Curve::Polyline { points }) = &mut moved else {
        return Err(Error::Argument);
    };
    *points.get_mut(index).ok_or(Error::Argument)? = to;
    Ok(moved)
}

/// The curvature comb of `curve` over `interval` of its parameter: the kernel's curvature vector
/// at `teeth` even steps, both ends included. Stations where the curve has no tangent are left
/// out.
///
/// # Errors
/// [`Error::Argument`] for a tooth count outside [`MIN_COMB_TEETH`]..=[`MAX_COMB_TEETH`], or a
/// curve or interval the kernel refuses; the kernel's status otherwise.
pub fn curve_comb(curve: &Curve, interval: [f64; 2], teeth: usize) -> Result<Vec<CombTooth>> {
    if !(MIN_COMB_TEETH..=MAX_COMB_TEETH).contains(&teeth)
        || !interval.iter().all(|t| t.is_finite())
    {
        return Err(Error::Argument);
    }
    let record = solidcore_convert::curve(curve).map_err(|_| Error::Argument)?;
    run("curve_comb", |k| {
        let made = match from_parameters(k, &record, interval) {
            Ok(made) => made,
            Err(SolidError::InvalidInput { .. }) => return Ok(Err(Error::Argument)),
            Err(e) => return Err(e),
        };
        Ok(Ok(continuity::curve_comb(
            k,
            &made.curve,
            made.interval,
            teeth,
            false,
        )?
        .into_iter()
        .map(tooth)
        .collect()))
    })?
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUARE: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
    ];

    #[test]
    fn a_cubic_through_four_points_is_clamped_on_its_end_points() {
        let c = cv_curve(&SQUARE, 3).unwrap();
        let Curve::Spline { knots, points } = &c else {
            panic!("a spline")
        };
        assert_eq!(knots, &vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
        assert_eq!(points, &SQUARE.to_vec());
        // Clamped: the curve starts and ends on its first and last points (the comb's end teeth
        // stand on the curve's ends).
        let ends = curve_comb(&c, [0.0, 1.0], 2).unwrap();
        assert_eq!(ends.len(), 2);
        for (tooth, cv) in ends.iter().zip([SQUARE[0], SQUARE[3]]) {
            assert!(
                tooth.at.iter().zip(cv).all(|(a, b)| (a - b).abs() < 1e-12),
                "{:?}",
                tooth.at
            );
        }
        // Fewer points lower the degree: two points make a line, three a quadratic.
        let Curve::Spline { knots, .. } = cv_curve(&SQUARE[..2], 3).unwrap() else {
            panic!()
        };
        assert_eq!(knots, vec![0.0, 0.0, 1.0, 1.0]);
        let Curve::Spline { knots, .. } = cv_curve(&SQUARE[..3], 5).unwrap() else {
            panic!()
        };
        assert_eq!(knots, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn bad_control_points_are_refused() {
        assert_eq!(cv_curve(&SQUARE[..1], 3), Err(Error::Argument));
        assert_eq!(
            cv_curve(&[[0.0; 3], [f64::NAN, 0.0, 0.0]], 1),
            Err(Error::Argument)
        );
        assert_eq!(cv_curve(&SQUARE, 0), Err(Error::Argument));
        assert_eq!(cv_curve(&SQUARE, MAX_DEGREE + 1), Err(Error::Argument));
    }

    #[test]
    fn moving_a_cv_changes_only_that_point() {
        let c = cv_curve(&SQUARE, 3).unwrap();
        let moved = move_cv(&c, 2, [12.0, 14.0, 3.0]).unwrap();
        let (
            Curve::Spline {
                knots: k0,
                points: p0,
            },
            Curve::Spline {
                knots: k1,
                points: p1,
            },
        ) = (&c, &moved)
        else {
            panic!()
        };
        assert_eq!(k0, k1);
        assert_eq!(p1[2], [12.0, 14.0, 3.0]);
        for i in [0, 1, 3] {
            assert_eq!(p0[i], p1[i]);
        }
        assert_eq!(move_cv(&c, 4, [0.0; 3]), Err(Error::Argument));
        assert_eq!(
            move_cv(&c, 1, [f64::INFINITY, 0.0, 0.0]),
            Err(Error::Argument)
        );
        assert_eq!(
            move_cv(
                &Curve::Line {
                    start: [0.0; 3],
                    end: [1.0; 3]
                },
                0,
                [0.0; 3]
            ),
            Err(Error::Argument)
        );
    }

    #[test]
    fn an_arcs_comb_points_at_its_centre_with_the_inverse_radius() {
        // A quarter circle of radius 10 about (1, 2, 3) in its z plane, as a rational quadratic.
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let (cx, cy, cz) = (1.0, 2.0, 3.0);
        let arc = Curve::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: vec![
                [cx + 10.0, cy, cz, 1.0],
                [(cx + 10.0) * w, (cy + 10.0) * w, cz * w, w],
                [cx, cy + 10.0, cz, 1.0],
            ],
        };
        let comb = curve_comb(&arc, [0.0, 1.0], 9).unwrap();
        assert_eq!(comb.len(), 9);
        for t in &comb {
            let to_centre = [cx - t.at[0], cy - t.at[1], cz - t.at[2]];
            let r2 = to_centre.iter().map(|x| x * x).sum::<f64>();
            assert!((r2 - 100.0).abs() < 1e-7, "on the arc: r² = {r2}");
            for (k, d) in t.curvature.iter().zip(to_centre) {
                assert!(
                    (k - d / 100.0).abs() < 1e-9,
                    "κ N = (centre - p) / r²: {:?} at {:?}",
                    t.curvature,
                    t.at
                );
            }
        }
    }

    #[test]
    fn a_straight_splines_comb_is_zero_and_bad_counts_are_refused() {
        let line = cv_curve(&[[0.0; 3], [5.0, 5.0, 0.0], [10.0, 10.0, 0.0]], 2).unwrap();
        let comb = curve_comb(&line, [0.0, 1.0], 5).unwrap();
        assert_eq!(comb.len(), 5);
        assert!(
            comb.iter()
                .all(|t| t.curvature.iter().all(|k| k.abs() < 1e-12)),
            "{comb:?}"
        );
        assert_eq!(
            curve_comb(&line, [0.0, 1.0], MIN_COMB_TEETH - 1),
            Err(Error::Argument)
        );
        assert_eq!(
            curve_comb(&line, [0.0, 1.0], MAX_COMB_TEETH + 1),
            Err(Error::Argument)
        );
        assert_eq!(curve_comb(&line, [0.0, f64::NAN], 5), Err(Error::Argument));
    }
}
