//! Curves fitted to points: through points (edit points), closed, and rebuilt to a degree and span count.
//! The curve is the record's ([`Curve`]); the fit is the kernel's ([`interp`]: global
//! interpolation, The NURBS Book's §9.2.1). Ops checks the inputs and converts.

use solidcore::nurbs::crv_fit::p1::ParamMethod;
use solidcore::nurbs::crv_fit::p2::interp;
use solidcore::nurbs::knots::MAX_DEGREE;
use solidcore::Vec3;
use solidcore_record::brep::Curve;

use crate::curve_knots::record_of;
use crate::session::run;
use crate::sweep::{coord_ok, CURVE_MAX_POINTS};
use crate::{Error, Result};

/// The clamped B-spline of `degree` (at most one less than the number of points: two points
/// make a line, three a quadratic) through `points` in order, on `[0, 1]`, each point at its
/// share of the polygon's length (chord-length parameters) and the knots their averages.
///
/// # Errors
/// [`Error::Argument`] for fewer than two points, more than [`CURVE_MAX_POINTS`] (or than a
/// record holds), a coordinate that is not finite or past the coordinate bound, a point
/// repeating the one before it, or a degree of 0 or past the kernel's most; the kernel's status
/// otherwise (points so close the fit is singular).
pub fn ep_curve(points: &[[f64; 3]], degree: usize) -> Result<Curve> {
    if points.len() < 2 || points.len() > CURVE_MAX_POINTS || !points.iter().all(|p| coord_ok(p)) {
        return Err(Error::Argument);
    }
    if points.windows(2).any(|w| w.first() == w.get(1)) {
        return Err(Error::Argument);
    }
    if degree == 0 || degree > MAX_DEGREE {
        return Err(Error::Argument);
    }
    let degree = degree.min(points.len() - 1);
    let data: Vec<Vec3> = points.iter().map(|&[x, y, z]| Vec3::new(x, y, z)).collect();
    run("curve_fit::ep_curve", |_| {
        Ok(record_of(&interp(&data, degree, ParamMethod::ChordLength)?))
    })?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve_knots::nurb_of;
    use solidcore::nurbs::knots::Side;

    /// The point of `c` at `t`, by the kernel's evaluator.
    fn at(c: &Curve, t: f64) -> [f64; 3] {
        let p = nurb_of(c).unwrap().eval(t, Side::Left).unwrap();
        [p.x, p.y, p.z]
    }

    fn passes_through(c: &Curve, points: &[[f64; 3]], params: &[f64]) {
        for (p, t) in points.iter().zip(params) {
            let q = at(c, *t);
            assert!(
                p.iter().zip(q).all(|(a, b)| (a - b).abs() < 1e-9),
                "at {t}: {q:?}, not {p:?}"
            );
        }
    }

    #[test]
    fn a_cubic_passes_through_its_points_at_their_share_of_the_length() {
        // Chords of 5, 12 and 5: the points at 0, 5/22, 17/22 and 1 (uniform parameters would
        // put the second at 1/3).
        let points = [
            [0.0, 0.0, 0.0],
            [3.0, 4.0, 0.0],
            [3.0, 4.0, 12.0],
            [6.0, 8.0, 12.0],
        ];
        let c = ep_curve(&points, 3).unwrap();
        let Curve::Spline { knots, points: cvs } = &c else {
            panic!("a spline: {c:?}")
        };
        assert_eq!(knots, &vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
        assert_eq!(cvs.len(), 4);
        passes_through(&c, &points, &[0.0, 5.0 / 22.0, 17.0 / 22.0, 1.0]);
    }

    #[test]
    fn more_points_than_the_degree_make_interior_knots() {
        // Six points with chords of 5: parameters i / 5, interior knots the averages of three.
        let points = [
            [0.0, 0.0, 0.0],
            [3.0, 4.0, 0.0],
            [6.0, 0.0, 0.0],
            [9.0, 4.0, 0.0],
            [12.0, 0.0, 0.0],
            [15.0, 4.0, 0.0],
        ];
        let params: Vec<f64> = (0..6).map(|i| f64::from(i) / 5.0).collect();
        let c = ep_curve(&points, 3).unwrap();
        let Curve::Spline { knots, points: cvs } = &c else {
            panic!("a spline: {c:?}")
        };
        assert_eq!(cvs.len(), 6);
        assert_eq!(knots.len(), 10);
        for (k, want) in knots[4..6].iter().zip([0.4, 0.6]) {
            assert!((k - want).abs() < 1e-12, "{knots:?}");
        }
        passes_through(&c, &points, &params);

        let quadratic = ep_curve(&points, 2).unwrap();
        passes_through(&quadratic, &points, &params);
    }

    #[test]
    fn two_points_make_a_line() {
        let ends = [[1.0, 2.0, 3.0], [4.0, 6.0, 3.0]];
        assert_eq!(
            ep_curve(&ends, 3),
            Ok(Curve::Spline {
                knots: vec![0.0, 0.0, 1.0, 1.0],
                points: ends.to_vec(),
            })
        );
    }

    #[test]
    fn bad_points_are_refused() {
        let ends = [[0.0; 3], [1.0, 0.0, 0.0]];
        assert_eq!(ep_curve(&ends[..1], 3), Err(Error::Argument));
        assert_eq!(
            ep_curve(&[[0.0; 3], [f64::NAN, 0.0, 0.0]], 1),
            Err(Error::Argument)
        );
        assert_eq!(
            ep_curve(&[[0.0; 3], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0]], 2),
            Err(Error::Argument)
        );
        assert_eq!(ep_curve(&ends, 0), Err(Error::Argument));
        assert_eq!(ep_curve(&ends, MAX_DEGREE + 1), Err(Error::Argument));
    }
}
