//! A free curve made drawable: the points of a record curve over an interval, at a mesh
//! tolerance, by the same rules the mesher samples a body's edges with (each chord within the
//! tolerance of the curve, within the facet angle, never spanning more than a quarter turn).
//! For a curve that bounds no face: a wire, a sketch line, a curve being modelled.

use solidcore::Vec3;
use solidcore::eval::CurveEval;
use solidcore_record::brep::Curve;

use crate::mesher::edges::bisect;
use crate::mesher::size::Rules;
use crate::vec::VecExt as _;
use crate::{MeshTolerance, Result, TessError};

/// The points of `curve` over `interval` of its parameter, from its point at the start to its
/// point at the end, every chord within `tolerance` of the curve (its chord, and its facet angle
/// when it has one). A curve whose ends meet (within the chord tolerance) starts from four
/// segments, as a closed edge does.
///
/// # Errors
///
/// [`TessError::Invalid`] for an interval that is not finite and increasing; [`TessError::Failed`]
/// for a curve the evaluator does not take or that does not evaluate somewhere on the interval.
pub fn curve_points(
    curve: &Curve,
    interval: [f64; 2],
    tolerance: MeshTolerance,
) -> Result<Vec<[f64; 3]>> {
    let [t0, t1] = interval;
    if !(t0.is_finite() && t1.is_finite() && t1 > t0) {
        return Err(TessError::Invalid(format!(
            "curve points: interval [{t0}, {t1}]"
        )));
    }
    let fail = |why: String| TessError::Failed(format!("curve points: {why}"));
    let value = solidcore_convert::curve(curve).map_err(|e| fail(e.to_string()))?;
    let eval = CurveEval::new(&value).map_err(|e| fail(crate::eval_message(e)))?;
    let at = |t: f64| -> Result<Vec3> {
        eval.point(t)
            .filter(|p| p.is_finite())
            .ok_or_else(|| fail(format!("the curve does not evaluate at t = {t}")))
    };
    let (tol, turn) = tolerance.rules(0.0);
    let rules = Rules {
        tol,
        max_len: f64::INFINITY,
        turn,
    };
    let ends = [at(t0)?, at(t1)?];
    let [front, back] = ends;
    let initial = if front.dist(back) <= tol { 4 } else { 1 };
    let nodes = bisect(&at, interval, ends, initial, &rules, f64::INFINITY)?;
    Ok(nodes.points.iter().map(|p| [p.x, p.y, p.z]).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The quarter circle of radius 10 about the origin in z = 0 as a rational quadratic.
    fn quarter_circle() -> Curve {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        Curve::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: vec![
                [10.0, 0.0, 0.0, 1.0],
                [10.0 * w, 10.0 * w, 0.0, w],
                [0.0, 10.0, 0.0, 1.0],
            ],
        }
    }

    #[test]
    fn a_curves_points_stay_within_the_tolerance_and_end_on_its_ends() {
        for chord in [0.1, 0.01, 0.001] {
            let pts =
                curve_points(&quarter_circle(), [0.0, 1.0], MeshTolerance::chord(chord)).unwrap();
            assert!(pts.len() >= 3, "{chord}");
            assert_eq!(pts.first().copied(), Some([10.0, 0.0, 0.0]));
            let last = pts.last().copied().unwrap();
            assert!(
                (last[0] - 0.0).abs() < 1e-12 && (last[1] - 10.0).abs() < 1e-12,
                "{last:?}"
            );
            for w in pts.windows(2) {
                // On the circle, and every chord's middle sags at most the tolerance.
                for p in w {
                    assert!((p[0].hypot(p[1]) - 10.0).abs() < 1e-9);
                }
                let m = [(w[0][0] + w[1][0]) / 2.0, (w[0][1] + w[1][1]) / 2.0];
                assert!(10.0 - m[0].hypot(m[1]) <= chord * 1.0001, "sag at {chord}");
            }
        }
    }

    #[test]
    fn a_straight_spline_is_one_chord_and_a_bad_interval_is_refused() {
        let line = Curve::Spline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            points: vec![[0.0; 3], [5.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
        };
        assert_eq!(
            curve_points(&line, [0.0, 1.0], MeshTolerance::display(0.01)).unwrap(),
            vec![[0.0; 3], [10.0, 0.0, 0.0]]
        );
        assert!(matches!(
            curve_points(&line, [1.0, 0.0], MeshTolerance::chord(0.01)),
            Err(TessError::Invalid(_))
        ));
        assert!(matches!(
            curve_points(&line, [0.0, f64::NAN], MeshTolerance::chord(0.01)),
            Err(TessError::Invalid(_))
        ));
    }

    #[test]
    fn a_closed_curve_starts_from_four_segments() {
        // A full circle of radius 10 from four rational quadratic quarters; one coarse chord.
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let h = |x: f64, y: f64, wt: f64| [x * wt, y * wt, 0.0, wt];
        let circle = Curve::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0],
            homogeneous: vec![
                h(10.0, 0.0, 1.0),
                h(10.0, 10.0, w),
                h(0.0, 10.0, 1.0),
                h(-10.0, 10.0, w),
                h(-10.0, 0.0, 1.0),
                h(-10.0, -10.0, w),
                h(0.0, -10.0, 1.0),
                h(10.0, -10.0, w),
                h(10.0, 0.0, 1.0),
            ],
        };
        let pts = curve_points(&circle, [0.0, 4.0], MeshTolerance::chord(100.0)).unwrap();
        assert_eq!(pts.len(), 5, "{pts:?}");
    }
}
