//! Regression test for WR-528 (the fork's tangency-labs/smlib@e881cdb, capi test
//! `booleans crescent_on_rim_join`): a crescent prism standing against a cylinder,
//! its r 10 wall on the cylinder's wall, united with it. With
//! `Kernel::fixes.close_loop_relates_only_coincident_edges`, the cylinder's rim
//! split at the crescent's corners is no longer related to the crescent's r 6 arc,
//! and the join is a valid solid of the exact volume, both across the cylinder's
//! seam and clear of it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::primitive::api_curves::create_arc;
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::Brep;
use solidcore::{Kernel, Vec3};

const R: f64 = 10.0;
const R_CRESCENT: f64 = 6.0;
const HEIGHT: f64 = 10.0;

/// The prism of the crescent outside the cylinder's disk and inside the r 6 circle
/// centred on the cylinder's rim at `angle_deg`, `HEIGHT` tall.
fn crescent_prism(k: &mut Kernel, angle_deg: f64) -> Brep {
    // the circles meet at x = 8.2, y = +-sqrt(32.76) in the frame of the crescent
    let y = (R * R - 8.2 * 8.2).sqrt();
    let big = y.atan2(8.2).to_degrees();
    let small = y.atan2(8.2 - R).to_degrees();
    let centre = Vec3::new(
        R * angle_deg.to_radians().cos(),
        R * angle_deg.to_radians().sin(),
        0.0,
    );
    let inner = Curve::from(
        create_arc(
            Vec3::new(0.0, 0.0, 0.0),
            R,
            angle_deg - big,
            angle_deg + big,
        )
        .unwrap(),
    );
    let outer =
        Curve::from(create_arc(centre, R_CRESCENT, angle_deg - small, angle_deg + small).unwrap());
    let curves = [Some(&inner), Some(&outer)];
    create_linear_sweep(k, &curves, Vec3::new(0.0, 0.0, 1.0), HEIGHT, true).unwrap()
}

/// The volume of the cylinder and the crescent prism.
fn exact_volume() -> f64 {
    let (d, r) = (R, R_CRESCENT);
    let lens = r * r * ((d * d + r * r - R * R) / (2.0 * d * r)).acos()
        + R * R * ((d * d + R * R - r * r) / (2.0 * d * R)).acos()
        - 0.5 * ((-d + r + R) * (d + r - R) * (d - r + R) * (d + r + R)).sqrt();
    let crescent = std::f64::consts::PI * r * r - lens;
    (std::f64::consts::PI * R * R + crescent) * HEIGHT
}

#[test]
fn crescent_on_rim_join_is_exact_with_the_fix() {
    let exact = exact_volume();
    // across the cylinder's seam, and clear of it
    for angle in [0.0, 90.0] {
        let mut k = Kernel::default();
        k.fixes.close_loop_relates_only_coincident_edges = true;
        let cylinder = create_cylinder(&mut k, Vec3::new(0.0, 0.0, 0.0), R, HEIGHT).unwrap();
        let prism = crescent_prism(&mut k, angle);
        let mut joined = boolean_union(&mut k, cylinder, prism).unwrap();
        let c = check_brep(&mut k, &mut joined, CheckLevel::Level2).unwrap();
        assert_eq!(c.failure_count(), 0, "angle {angle}: {:?}", c.lines());
        let store = AttributeStore::default();
        let key_of = |_| ObjectKey(0);
        let attributes = PropertyAttributes {
            store: &store,
            key_of: &key_of,
        };
        let volume = brep_volume(&k, &mut joined, 1e-6, &attributes).unwrap();
        assert!(
            (volume - exact).abs() <= 1e-6 * exact,
            "angle {angle}: volume {volume}, expected {exact}"
        );
    }
}
