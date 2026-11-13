//! The point of a free curve nearest a given point, through ops on record curves: on a circle,
//! a line, past a line's end and on a spline; a point that is not finite refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use solidcore_ops::curve::{circle, closest_point, curve_comb, cv_curve};
use solidcore_ops::measure::point_distance;
use solidcore_ops::Error;

const TOL: f64 = 1e-6;

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < 1e-8)
}

/// From (12, 16, 0) the circle of radius 10 about the origin is nearest at (6, 8, 0), 10 away.
#[test]
fn a_circle_is_nearest_along_the_radius() {
    let c = circle([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 10.0).unwrap();
    let r = closest_point(&c, [12.0, 16.0, 0.0], TOL).unwrap().unwrap();
    assert!(near(r.point, [6.0, 8.0, 0.0]), "{r:?}");
    assert!((r.distance - 10.0).abs() < 1e-9, "{r:?}");
    assert!((0.0..=1.0).contains(&r.parameter), "{r:?}");
}

/// The line from the origin to (10, 0, 0) on [0, 1]: from (4, 3, 0) the foot of the
/// perpendicular, (4, 0, 0) at 0.4, 3 away; from (15, 3, 0), past its end, the end itself.
#[test]
fn a_line_is_nearest_at_the_foot_of_the_perpendicular_or_its_end() {
    let line = cv_curve(&[[0.0; 3], [10.0, 0.0, 0.0]], 1).unwrap();
    let r = closest_point(&line, [4.0, 3.0, 0.0], TOL).unwrap().unwrap();
    assert!(near(r.point, [4.0, 0.0, 0.0]), "{r:?}");
    assert!((r.parameter - 0.4).abs() < 1e-8, "{r:?}");
    assert!((r.distance - 3.0).abs() < 1e-9, "{r:?}");

    let past = closest_point(&line, [15.0, 3.0, 0.0], TOL)
        .unwrap()
        .unwrap();
    assert!(near(past.point, [10.0, 0.0, 0.0]), "{past:?}");
    assert!((past.parameter - 1.0).abs() < 1e-8, "{past:?}");
    let to_end = point_distance([15.0, 3.0, 0.0], [10.0, 0.0, 0.0]).unwrap();
    assert!((past.distance - to_end.value).abs() < 1e-9, "{past:?}");
}

/// On a wavy cubic no point the kernel samples along it is nearer than the point found, and
/// the distance given is the point's.
#[test]
fn a_spline_has_no_sampled_point_nearer() {
    let c = cv_curve(
        &[
            [0.0, 0.0, 0.0],
            [10.0, 20.0, 0.0],
            [20.0, -20.0, 5.0],
            [30.0, 20.0, -5.0],
            [40.0, -10.0, 0.0],
            [50.0, 0.0, 0.0],
        ],
        3,
    )
    .unwrap();
    for p in [
        [25.0, 15.0, 3.0],
        [-5.0, 5.0, 0.0],
        [12.0, -30.0, 10.0],
        [55.0, 2.0, -1.0],
    ] {
        let r = closest_point(&c, p, TOL).unwrap().unwrap();
        let own = point_distance(p, r.point).unwrap();
        assert!((own.value - r.distance).abs() < 1e-9, "{p:?} {r:?}");
        let samples = curve_comb(&c, [0.0, 1.0], 401).unwrap();
        for s in samples {
            let d = point_distance(p, s.at).unwrap();
            assert!(r.distance <= d.value + 1e-9, "{p:?} {r:?} {s:?}");
        }
    }
}

/// A point or tolerance that is not finite, or a tolerance that is not positive, is refused.
#[test]
fn a_point_that_is_not_finite_is_refused() {
    let line = cv_curve(&[[0.0; 3], [10.0, 0.0, 0.0]], 1).unwrap();
    for p in [
        [f64::NAN, 0.0, 0.0],
        [0.0, f64::INFINITY, 0.0],
        [0.0, 0.0, f64::NEG_INFINITY],
    ] {
        assert!(
            matches!(closest_point(&line, p, TOL), Err(Error::Argument)),
            "{p:?}"
        );
    }
    for tol in [f64::NAN, 0.0, -1.0] {
        assert!(
            matches!(closest_point(&line, [1.0; 3], tol), Err(Error::Argument)),
            "{tol}"
        );
    }
}
