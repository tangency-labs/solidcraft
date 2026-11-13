//! The curve sweep kept upright (`CreateSweepSurface` with an up direction, the fork's z source
//! for `N_CreateSweepScale`): at every section the frame's z is the up direction made square to
//! the rail's tangent, so a section drawn in a plane holding the up direction stays in the
//! plane holding it and the rail's tangent, its top up, round a closed rail and over its slopes;
//! the sheet closes. A rail running along the up direction is refused.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::crv_fit::p1::ParamMethod;
use solidcore::SolidError;

/// A closed cubic through sixteen points of `(10 cos t, 10 sin t, lift cos 2t)`.
fn loop_rail(lift: f64) -> BSplineCurve {
    // the closed fit takes its points closed: the first repeated last (`a[0] == a[n]`)
    let mut points: Vec<Point3> = (0..16)
        .map(|i| {
            let t = std::f64::consts::TAU * f64::from(i) / 16.0;
            Point3::new(10.0 * t.cos(), 10.0 * t.sin(), lift * (2.0 * t).cos())
        })
        .collect();
    if let Some(&first) = points.first() {
        points.push(first);
    }
    BSplineCurve::interpolate_points(&points, None, 3, None, None, true, ParamMethod::ChordLength)
        .expect("closed rail")
}

/// A vertical segment 2 long, drawn at the rail's start: the section.
fn segment() -> BSplineCurve {
    BSplineCurve::interpolate_points(
        &[Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 2.0)],
        None,
        1,
        None,
        None,
        false,
        ParamMethod::ChordLength,
    )
    .expect("segment")
}

/// The swept sheet kept upright along `rail`, and where its first, middle and last sections
/// lie.
fn upright(rail: &BSplineCurve) -> solidcore::Result<[Axis2Placement; 3]> {
    let mut placed = [Axis2Placement::IDENTITY; 3];
    BSplineSurface::create_sweep_surface_traced(
        &segment(),
        rail,
        None,
        false,
        1e-4,
        Some(&mut placed),
        Some(Vec3::Z),
    )?;
    Ok(placed)
}

#[test]
fn kept_upright_each_section_stands_in_the_plane_of_the_up_direction_and_the_rail() {
    for lift in [0.0, 3.0] {
        let rail = loop_rail(lift);
        let placed = upright(&rail).unwrap_or_else(|e| panic!("lift {lift}: {e}"));
        // the rail's start tangent, which each placement carries onto the tangent at its station
        let d = rail
            .evaluate(rail.natural_interval().min(), 1, true, false)
            .expect("tangent");
        let x0 = d[1].unitize().expect("tangent");
        for (i, p) in placed.iter().enumerate() {
            let up = p.transform_vector(Vec3::Z);
            let x = p.transform_vector(x0);
            // square to the rail, in the plane of the up direction and the tangent, top up
            assert!(
                up.dot(x).abs() <= 1e-9,
                "lift {lift}, section {i}: {up:?} {x:?}"
            );
            assert!(
                up.dot(Vec3::Z.cross(x)).abs() <= 1e-9,
                "lift {lift}, section {i}: {up:?} leans out of the plane of Z and {x:?}"
            );
            assert!(up.z > 0.0, "lift {lift}, section {i}: upside down {up:?}");
        }
        // round the closed rail the last section lies on the first
        let near = |a: Vec3, b: Vec3| (a - b).length() <= 1e-9;
        let (a, b) = (&placed[0], &placed[2]);
        assert!(near(a.origin(), b.origin()) && near(a.x_axis(), b.x_axis()));
        assert!(near(a.y_axis(), b.y_axis()), "lift {lift}: {a:?} {b:?}");
    }
}

#[test]
fn a_rail_running_along_the_up_direction_is_refused() {
    // a closed loop standing on edge in the plane x = 0: vertical at two points
    let mut points: Vec<Point3> = (0..16)
        .map(|i| {
            let t = std::f64::consts::TAU * f64::from(i) / 16.0;
            Point3::new(0.0, 10.0 * t.cos(), 10.0 * t.sin())
        })
        .collect();
    if let Some(&first) = points.first() {
        points.push(first);
    }
    let rail = BSplineCurve::interpolate_points(
        &points,
        None,
        3,
        None,
        None,
        true,
        ParamMethod::ChordLength,
    )
    .expect("rail");
    assert!(matches!(
        upright(&rail),
        Err(SolidError::Unsupported { .. })
    ));
}
