//! Regression test for WR-536 (the fork's tangency-labs/smlib@67b8a1e, capi test
//! `ops_kernel` four disks on a 3 m hole pattern): two r 2 disks 1.5 m apart, joined
//! with `boolean_2d`. The second disk is rebuilt on a copy of the first disk's plane,
//! which must first be grown to cover it; a plane's search domain of 100 widths (a 6 mm
//! plane) does not reach 1.5 m. With `boolean2d_plane_covers_far_faces` the plane
//! grows to the far disk and the edges keep their tolerance.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::{boolean_2d, Boolean2d, Boolean2dOutcome};
use solidcore::primitive::api_primitives::create_planar_circle;
use solidcore::{Kernel, Vec3};

#[test]
fn far_disks_join_on_one_plane_with_the_fix() {
    let mut k = Kernel::default();
    k.fixes.boolean2d_plane_covers_far_faces = true;
    let near = create_planar_circle(&mut k, Vec3::new(0.0, 0.0, 0.0), 2.0).unwrap();
    let far = create_planar_circle(&mut k, Vec3::new(1500.0, 0.0, 0.0), 2.0).unwrap();
    let joined = match boolean_2d(&mut k, near, far, Boolean2d::Union).unwrap() {
        Boolean2dOutcome::Result(b) => b,
        Boolean2dOutcome::Unassigned { .. } => panic!("2D boolean left its operands"),
    };
    assert_eq!(joined.face_list().len(), 2);
    let worst = joined
        .edge_list()
        .iter()
        .map(|&e| joined.edge(e).unwrap().tolerance)
        .fold(0.0_f64, f64::max);
    assert!(worst < 1e-3, "an edge tolerance of {worst}");
}
