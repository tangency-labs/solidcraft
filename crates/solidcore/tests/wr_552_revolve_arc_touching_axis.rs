//! Regression test for WR-552 (the fork's tangency-labs/smlib@c4e378d, capi test
//! `sweeps tight_bend`): a profile in the XY plane revolved a whole turn about the Y
//! axis. An arc of the circle about (10, 0) of radius 10 + 1e-10 runs from the top of
//! the circle to where it meets the axis; a line along the axis and one across close the
//! profile. The circle reaches a hair past the axis, so the arc sweeps a spindle torus
//! whose minor circles cross the axis where the sweep collapses the arc's end to a
//! point, and the face's loop there comes out turned over. With
//! `revolve_arc_touching_axis_is_horn_torus` the torus is the horn one (its major radius
//! its minor) and the body passes the check (apart from `SmSurface` test #5, which the
//! torus fails by itself: see `revolve`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::surface::surface::SurfaceKind;
use solidcore::sweep::api_brep::create_rotational_sweep;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Kernel, Point3, Vec3};

const R: f64 = 10.0;
const DELTA: f64 = 1e-10;

/// The revolve: the torus faces' (major, minor) radii and SMLib's check lines.
fn revolve(horn: bool) -> (Vec<(f64, f64)>, Vec<String>) {
    let mut k = Kernel::default();
    k.fixes.revolve_analytic_profile_faces = true;
    k.fixes.revolve_arc_touching_axis_is_horn_torus = horn;
    let r = R + DELTA;
    let s = (r * r - R * R).sqrt();
    let a_deg = s.atan2(-R).to_degrees();
    let top = Point3::new(R, r, 0.0);
    let on_axis = Point3::new(0.0, s, 0.0);
    let cap = Point3::new(0.0, r, 0.0);
    let arc = Curve::from(create_arc(Point3::new(R, 0.0, 0.0), r, 90.0, a_deg).unwrap());
    let along = Curve::from(create_line_segment(on_axis, cap).unwrap());
    let across = Curve::from(create_line_segment(cap, top).unwrap());
    let curves = [Some(&arc), Some(&along), Some(&across)];
    let mut b = create_rotational_sweep(
        &mut k,
        &curves,
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        360.0,
        true,
    )
    .unwrap();
    let tori = b
        .face_list()
        .iter()
        .filter_map(
            |&f| match b.face(f).unwrap().surface.as_ref().unwrap().kind() {
                SurfaceKind::Torus(t) => Some((t.major_radius(), t.minor_radius())),
                _ => None,
            },
        )
        .collect();
    // SmSurface test #5 (an approximately singular boundary) is left out: the arc stops
    // short of the apex, at the axis point sqrt(2 R DELTA) above the torus's centre
    // plane, so even the exact horn torus's boundary circle there has a radius of about
    // DELTA, singular to the tolerance but not exactly; the test fails on that surface
    // by itself, snapped or not (`surface.surface_assert_valid` fixture
    // `wr552_horn_torus_*` compares it with SMLib's)
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines()
        .into_iter()
        .filter(|l| !l.starts_with("SmSurface::Test #5 "))
        .collect();
    (tori, lines)
}

#[test]
fn wr_552_arc_touching_the_axis_sweeps_a_horn_torus() {
    let (tori, lines) = revolve(true);
    assert_eq!(tori.len(), 1, "{tori:?}");
    // the face's torus is the snapped one up to rounding (its radii come back a few ulp
    // apart); the spindle torus it would otherwise be is DELTA apart
    let (major, minor) = tori[0];
    assert!(
        (major - minor).abs() <= DELTA / 1000.0,
        "a horn torus: {major} {minor}"
    );
    assert!(lines.is_empty(), "{lines:?}");
}

/// Without the opt-in the arc sweeps the spindle torus it lies on, and the face's loop
/// at the axis is turned over (a failure besides test #5).
#[test]
fn wr_552_without_the_opt_in_the_spindle_torus_fails_the_check() {
    let (tori, lines) = revolve(false);
    assert!(tori.iter().all(|&(major, minor)| major < minor), "{tori:?}");
    assert!(!lines.is_empty(), "{tori:?}");
}
