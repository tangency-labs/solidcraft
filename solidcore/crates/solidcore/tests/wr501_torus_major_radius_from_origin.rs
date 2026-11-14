//! WR-501 (tangency-labs/solidcore#1470, #1954): the public torus recogniser
//! `Torus::is_nurb_surface_torus` reports a torus's major radius from the NURB's first
//! sweep circle (upstream) or from the torus origin (`fixes.torus_major_radius_from_origin`).
//!
//! A whole-turn revolve of two half circles is a public torus construction whose torus
//! NURB runs its tube from -90 to 90 degrees, so the first sweep circle sits at the bottom
//! of the tube, off the equator. Upstream's radius then comes out sqrt(R^2 + r^2); the
//! opt-in measures R. Both readings are the public reader's own reported values on the
//! NURB of the face the revolve built (R 5, r 1).
//!
//! The recognition is at the direct level: the only library caller of the reader is the
//! exact-fillet quadric read, and the revolve operation
//! itself never calls it, so a public construction + the public reader is where the
//! guard is observable.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, NurbCircleParam};
use solidcore::surface::surface::SurfaceKind;
use solidcore::surface::torus::Torus;
use solidcore::sweep::api_brep::create_rotational_sweep;
use solidcore::{Kernel, Vec3};

const MAJOR: f64 = 5.0;
const MINOR: f64 = 1.0;

/// The torus faces of a whole turn about Z of the two half circles of radius 1 about
/// (5, 0, 0), the public profile construction of
/// `sweep::api_brep::a_whole_turn_of_a_profile_gives_each_curve_its_analytic_face`.
fn revolved_tori(k: &mut Kernel) -> Vec<Torus> {
    let arc = |c: Vec3, x: Vec3, y: Vec3| {
        Curve::from(
            BSplineCurve::create_circle_segment(
                3,
                &Axis2Placement::new(c, x, y),
                MINOR,
                0.0,
                180.0,
                NurbCircleParam::Quadratic,
            )
            .unwrap(),
        )
    };
    let outer = arc(Vec3::new(MAJOR, 0.0, 0.0), -Vec3::Z, Vec3::X);
    let inner = arc(Vec3::new(MAJOR, 0.0, 0.0), Vec3::Z, -Vec3::X);
    let b = create_rotational_sweep(
        k,
        &[Some(&outer), Some(&inner)],
        Vec3::ZERO,
        Vec3::Z,
        360.0,
        false,
    )
    .unwrap();
    b.face_list()
        .iter()
        .filter_map(
            |&f| match b.face(f).unwrap().surface.as_ref().unwrap().kind() {
                SurfaceKind::Torus(t) => Some((**t).clone()),
                _ => None,
            },
        )
        .collect()
}

#[test]
fn wr501_revolved_torus_reports_its_major_radius_from_the_torus_origin() {
    let mut build = Kernel::default();
    // the public option that makes the revolve build an analytic torus face
    let tori = revolved_tori(&mut build);
    assert_eq!(tori.len(), 2, "both half circles give a torus");

    // the reader kernels differ only in the WR-501 guard
    let off = Kernel::default();
    let mut on = Kernel::default();
    on.fixes.torus_major_radius_from_origin = true;

    let upstream_major = (MAJOR * MAJOR + MINOR * MINOR).sqrt();
    for (i, t) in tori.iter().enumerate() {
        // the torus the revolve constructed: R 5, r 1
        assert!(
            (t.major_radius() - MAJOR).abs() <= 1e-9 * MAJOR,
            "face {i}: constructed major {}",
            t.major_radius()
        );
        assert!(
            (t.minor_radius() - MINOR).abs() <= 1e-9 * MINOR,
            "face {i}: constructed minor {}",
            t.minor_radius()
        );

        // upstream: from the first sweep circle at the bottom of the tube, sqrt(R^2 + r^2)
        let read_off = Torus::is_nurb_surface_torus(&off, t.base().base(), 1.0)
            .unwrap()
            .expect("a torus");
        assert!(
            (read_off.major_radius() - upstream_major).abs() <= 1e-9 * upstream_major,
            "face {i}: upstream major {}",
            read_off.major_radius()
        );
        assert!(
            read_off.major_radius() - MAJOR > 0.05,
            "face {i}: the two readings differ materially"
        );
        assert!(
            (read_off.minor_radius() - MINOR).abs() <= 1e-9 * MINOR,
            "face {i}: minor {}",
            read_off.minor_radius()
        );
        assert!(!read_off.inside_out(), "face {i}");

        // with only the guard on: from the torus origin, R
        let read_on = Torus::is_nurb_surface_torus(&on, t.base().base(), 1.0)
            .unwrap()
            .expect("a torus");
        assert!(
            (read_on.major_radius() - MAJOR).abs() <= 1e-9 * MAJOR,
            "face {i}: opt-in major {}",
            read_on.major_radius()
        );
        assert!(
            upstream_major - read_on.major_radius() > 0.05,
            "face {i}: the two readings differ materially"
        );
        assert!(
            (read_on.minor_radius() - MINOR).abs() <= 1e-9 * MINOR,
            "face {i}: minor {}",
            read_on.minor_radius()
        );
        assert!(!read_on.inside_out(), "face {i}");

        // the same torus, axis and tube; only the major radius changes
        assert_eq!(
            read_off.base().position().z_axis(),
            read_on.base().position().z_axis(),
            "face {i}: same axis"
        );
    }
}
