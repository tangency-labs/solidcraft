//! Where two free curves cross, through ops on record curves: two lines crossing in space, two
//! lines drawn in different planes crossing as seen along a direction, a circle crossed twice by
//! a line (its parameters in radians), a spline crossing a line, curves that do not cross, and
//! the refusals.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use solidcore_ops::curve::{closest_point, crossings, cv_curve};
use solidcore_ops::Error;
use solidcore_record::brep::{Curve, Frame};

const TOL: f64 = 1e-6;

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < 1e-7)
}

fn line(start: [f64; 3], end: [f64; 3]) -> Curve {
    Curve::Line { start, end }
}

/// The x axis from 0 to 10 and the line x = 5 from y = -5 to 5 cross at (5, 0, 0), half way
/// along each.
#[test]
fn two_lines_cross_half_way_along_each() {
    let a = line([0.0; 3], [10.0, 0.0, 0.0]);
    let b = line([5.0, -5.0, 0.0], [5.0, 5.0, 0.0]);
    let found = crossings(&a, &b, None, TOL).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    let c = found[0];
    assert!(
        (c.a - 0.5).abs() < 1e-9 && (c.b - 0.5).abs() < 1e-9,
        "{c:?}"
    );
    assert!(
        near(c.on_a, [5.0, 0.0, 0.0]) && near(c.on_b, [5.0, 0.0, 0.0]),
        "{c:?}"
    );
}

/// A line in the plane z = 0 and one in z = 3 do not meet in space; seen from above (along z)
/// they cross where the first is at (5, 0, 0) and the second, above it, at (5, 0, 3).
#[test]
fn lines_in_different_planes_cross_as_seen_along_a_direction() {
    let a = line([0.0; 3], [10.0, 0.0, 0.0]);
    let b = line([5.0, -5.0, 3.0], [5.0, 5.0, 3.0]);
    assert!(crossings(&a, &b, None, TOL).unwrap().is_empty());
    let found = crossings(&a, &b, Some([0.0, 0.0, -2.0]), TOL).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    let c = found[0];
    assert!(
        (c.a - 0.5).abs() < 1e-9 && (c.b - 0.5).abs() < 1e-9,
        "{c:?}"
    );
    assert!(near(c.on_a, [5.0, 0.0, 0.0]), "{c:?}");
    assert!(near(c.on_b, [5.0, 0.0, 3.0]), "{c:?}");
}

/// The circle of radius 5 about the origin is crossed by the x axis at angles 0 and pi, in
/// order along the circle; the line's parameters there are 0.75 and 0.25.
#[test]
fn a_line_through_a_circle_crosses_it_twice_at_its_angles() {
    let circle = Curve::Circle {
        frame: Frame {
            origin: [0.0; 3],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
        },
        radius: 5.0,
    };
    let axis = line([-10.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
    let found = crossings(&circle, &axis, None, TOL).unwrap();
    assert_eq!(found.len(), 2, "{found:?}");
    let pi = std::f64::consts::PI;
    let angles: Vec<f64> = found.iter().map(|c| c.a.rem_euclid(2.0 * pi)).collect();
    assert!(
        angles[0].abs() < 1e-7 || (angles[0] - 2.0 * pi).abs() < 1e-7,
        "{found:?}"
    );
    assert!((angles[1] - pi).abs() < 1e-7, "{found:?}");
    assert!(
        (found[0].b - 0.75).abs() < 1e-9 && (found[1].b - 0.25).abs() < 1e-9,
        "{found:?}"
    );
    assert!(near(found[0].on_a, [5.0, 0.0, 0.0]), "{found:?}");
    assert!(near(found[1].on_a, [-5.0, 0.0, 0.0]), "{found:?}");
}

/// A cubic spline arching over the x axis crosses the line y = 1 twice; at each crossing both
/// curves' points are on y = 1, and each parameter is the one the curve's nearest point to the
/// crossing has.
#[test]
fn a_spline_crosses_a_line_where_both_are() {
    let s = cv_curve(
        &[
            [0.0, 0.0, 0.0],
            [3.0, 4.0, 0.0],
            [7.0, 4.0, 0.0],
            [10.0, 0.0, 0.0],
        ],
        3,
    )
    .unwrap();
    let l = line([-1.0, 1.0, 0.0], [11.0, 1.0, 0.0]);
    let found = crossings(&s, &l, None, TOL).unwrap();
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].a < found[1].a, "{found:?}");
    for c in &found {
        assert!(
            (c.on_a[1] - 1.0).abs() < 1e-6 && (c.on_b[1] - 1.0).abs() < 1e-6,
            "{c:?}"
        );
        let near_s = closest_point(&s, c.on_a, TOL).unwrap().unwrap();
        assert!((near_s.parameter - c.a).abs() < 1e-6, "{c:?} {near_s:?}");
    }
}

/// Two parallel lines give no crossing.
#[test]
fn curves_that_do_not_cross_give_none() {
    let a = line([0.0; 3], [10.0, 0.0, 0.0]);
    let b = line([0.0, 1.0, 0.0], [10.0, 1.0, 0.0]);
    assert!(crossings(&a, &b, None, TOL).unwrap().is_empty());
}

/// A tolerance not positive or a direction not finite or of no length is refused.
#[test]
fn malformed_crossings_are_refused() {
    let a = line([0.0; 3], [10.0, 0.0, 0.0]);
    let b = line([5.0, -5.0, 0.0], [5.0, 5.0, 0.0]);
    for bad in [0.0, -1.0, f64::NAN] {
        assert_eq!(crossings(&a, &b, None, bad).err(), Some(Error::Argument));
    }
    for d in [[0.0; 3], [f64::NAN, 0.0, 1.0]] {
        assert_eq!(crossings(&a, &b, Some(d), TOL).err(), Some(Error::Argument));
    }
}
