//! WR-502: the four equal rounds of a square pyramid's apex (SolidCraft oracle case 37).
//! SMLib's `CreateSphereFromArcs` only takes three arcs, so the four-sided corner gets an
//! approximating blend whose top is 0.2 mm low; with
//! `Kernel::fixes.fillet_corner_sphere_from_four_arcs` the corner is the sphere the four
//! rounds lie on, and the apex ends at the sphere's top.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_pyramid;
use solidcore::query::brep::{calculate_bounding_box, BoundingBoxOptions};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::Kernel;

/// A pyramid on a 40 square, 30 high, every edge rounded R 2: the apex's height, whether the
/// result is a manifold solid.
fn rounded_pyramid(k: &mut Kernel) -> (f64, bool) {
    let mut b = create_pyramid(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0).unwrap();
    let edges = b.edge_list().to_vec();
    {
        let mut session = FilletSession::new(k, &mut b).unwrap();
        session
            .prepare_constant_radius_edges(k, &edges, 2.0, 1, 1, 1.0)
            .unwrap();
        session.do_fillet(k).unwrap();
    }
    stitch_and_orient(k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    let top = calculate_bounding_box(k, &mut b, BoundingBoxOptions::default())
        .unwrap()
        .max_at(2);
    (top, is_manifold_solid(&b).unwrap())
}

#[test]
fn four_equal_rounds_at_an_apex_meet_in_a_sphere_only_with_the_fix() {
    // The ball touching the four faces has its centre at z = (600 - 2 sqrt(1300)) / 20.
    let sphere_top = (600.0 - 2.0 * 1300.0_f64.sqrt()) / 20.0 + 2.0;
    let mut k = Kernel::default();
    let (top, manifold) = rounded_pyramid(&mut k);
    assert!(manifold && top < sphere_top - 0.1, "{top} {sphere_top}");
    let mut k = Kernel::default();
    k.fixes.fillet_corner_sphere_from_four_arcs = true;
    let (top, manifold) = rounded_pyramid(&mut k);
    assert!(
        manifold && (top - sphere_top).abs() < 1e-3,
        "{top} {sphere_top}"
    );
}
