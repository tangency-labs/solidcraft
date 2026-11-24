//! The point of a free curve nearest a ray, through ops on record curves: a pick through an
//! oblique view lands on the curve drawn under the pointer (a twisted space curve, a circle record), a ray
//! pointing away is nearest at its origin, and a direction of no length or not finite is refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use solidcore_ops::curve::{closest_point, closest_to_ray};
use solidcore_ops::curve_fit::ep_curve;
use solidcore_ops::Error;
use solidcore_record::brep::{Curve, Frame};

const TOL: f64 = 1e-6;

fn near(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < tol)
}

/// A curve that is not planar: a cubic fitted through points of the twisted cubic
/// (10 t, 10 t^2, 10 t^3), t from -1 to 1.
fn twisted() -> Curve {
    let points: Vec<[f64; 3]> = (0..=16)
        .map(|i| {
            let t = f64::from(i) / 8.0 - 1.0;
            [10.0 * t, 10.0 * t * t, 10.0 * t * t * t]
        })
        .collect();
    ep_curve(&points, 3).unwrap().0
}

/// Seen obliquely, the ray through a point on the space curve lands on that point: the curve's point
/// there, the ray's too, at no distance, with the curve's own parameter for it.
#[test]
fn a_ray_through_a_point_of_a_space_curve_lands_on_it() {
    let h = twisted();
    // a point on the helix, and a viewer far off along an oblique direction looking at it
    let on = closest_point(&h, [3.0, 1.0, 0.5], TOL).unwrap().unwrap();
    let look = [0.3, -0.7, -0.5];
    let origin = [
        on.point[0] - 200.0 * look[0],
        on.point[1] - 200.0 * look[1],
        on.point[2] - 200.0 * look[2],
    ];
    let hit = closest_to_ray(&h, origin, look, TOL).unwrap().unwrap();
    assert!(hit.distance < 1e-6, "{hit:?}");
    assert!(near(hit.point, on.point, 1e-5), "{hit:?} {on:?}");
    assert!(near(hit.on_ray, on.point, 1e-5), "{hit:?}");
    assert!(
        (hit.parameter - on.parameter).abs() < 1e-5,
        "{hit:?} {on:?}"
    );
}

/// A circle record seen obliquely: the ray through its point at 60 degrees lands there, the
/// parameter in radians.
#[test]
fn a_ray_through_a_circle_lands_on_it_in_radians() {
    let circle = Curve::Circle {
        frame: Frame {
            origin: [0.0; 3],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
        },
        radius: 10.0,
    };
    let p = [5.0, 8.660_254_037_844_386, 0.0];
    let look = [-0.2, 0.4, -1.0];
    let origin = [
        p[0] - 50.0 * look[0],
        p[1] - 50.0 * look[1],
        p[2] - 50.0 * look[2],
    ];
    let hit = closest_to_ray(&circle, origin, look, TOL).unwrap().unwrap();
    assert!(hit.distance < 1e-6, "{hit:?}");
    assert!(near(hit.point, p, 1e-6), "{hit:?}");
    assert!(
        (hit.parameter - std::f64::consts::FRAC_PI_3).abs() < 1e-7,
        "{hit:?}"
    );
}

/// A ray pointing away from the curve is nearest it at its origin.
#[test]
fn a_ray_pointing_away_is_nearest_at_its_origin() {
    let line = Curve::Line {
        start: [0.0, 0.0, 0.0],
        end: [10.0, 0.0, 0.0],
    };
    let origin = [5.0, 3.0, 0.0];
    let hit = closest_to_ray(&line, origin, [0.0, 1.0, 0.0], TOL)
        .unwrap()
        .unwrap();
    assert!(near(hit.on_ray, origin, 1e-9), "{hit:?}");
    assert!(near(hit.point, [5.0, 0.0, 0.0], 1e-9), "{hit:?}");
    assert!((hit.distance - 3.0).abs() < 1e-9, "{hit:?}");
    assert!((hit.parameter - 0.5).abs() < 1e-9, "{hit:?}");
}

/// A direction of no length or not finite, an origin not finite or a tolerance not positive is
/// refused.
#[test]
fn malformed_rays_are_refused() {
    let line = Curve::Line {
        start: [0.0, 0.0, 0.0],
        end: [10.0, 0.0, 0.0],
    };
    for d in [[0.0; 3], [f64::NAN, 1.0, 0.0]] {
        assert_eq!(
            closest_to_ray(&line, [0.0, 1.0, 0.0], d, TOL).err(),
            Some(Error::Argument)
        );
    }
    assert_eq!(
        closest_to_ray(&line, [f64::INFINITY, 1.0, 0.0], [0.0, -1.0, 0.0], TOL).err(),
        Some(Error::Argument)
    );
    assert_eq!(
        closest_to_ray(&line, [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], 0.0).err(),
        Some(Error::Argument)
    );
}
