//! The curve sweep (`CreateSweepSurface`, NLib's `N_CreateSweepScale`) carries its section on
//! the projection-normal frame, close to rotation-minimising. Round a closed rail that is not
//! planar that frame comes back turned about the rail (its holonomy), so the last section does
//! not lie on the first and the sheet does not close. The closing turn is spread along the rail:
//! the last section's frame is the first's.
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

/// A closed cubic through sixteen points of the loop `(10 cos t, 10 sin t, lift cos 2t)`: a saddle
/// loop (planar at `lift` 0), smooth at its seam.
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

/// A vertical segment 2 long: the section.
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

fn axes_near(a: &Axis2Placement, b: &Axis2Placement, tol: f64) -> bool {
    let near = |p: Vec3, q: Vec3| (p - q).length() <= tol;
    near(a.x_axis(), b.x_axis()) && near(a.y_axis(), b.y_axis()) && near(a.z_axis(), b.z_axis())
}

#[test]
fn round_a_closed_saddle_loop_the_last_section_lies_on_the_first() {
    let (_, placed) = BSplineSurface::create_sweep_surface_with_placements(
        &segment(),
        &loop_rail(3.0),
        None,
        false,
        1e-4,
    )
    .expect("swept");
    assert!(
        (placed[0].origin() - placed[2].origin()).length() <= 1e-9,
        "{:?} {:?}",
        placed[0].origin(),
        placed[2].origin()
    );
    assert!(
        axes_near(&placed[0], &placed[2], 1e-9),
        "the last section's frame is turned from the first's: {:?} against {:?}",
        placed[2],
        placed[0]
    );
}
