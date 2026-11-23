//! Regression test for WR-797: a union with a body that touches nothing. When the far
//! body's shell closes, every other shell of the region is sorted by a ray fired from a
//! point 20 edge tolerances into one of its faces. Upstream takes that point at the first
//! edge use of the shell's first face use only, and fails where the face is narrower than
//! the step. Geometry: a 10 mm cube whose edges around the first face use of each shell
//! have a tolerance of 1 mm (a step of 20 mm, across a 10 mm face), united with a 1 mm
//! cube 5 m away. No edge use of that face use gives a point, so the point comes from
//! the next face use.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::boolean_union;
use solidcore::error::topology::TopologyFailure;
use solidcore::mass::integrator::properties::brep_volume_default;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p1;
use solidcore::topology::edgeuse::p1::faceuse_edgeuses;
use solidcore::{Brep, Kernel, Point3, Result};

/// Which edges of the cube get the 1 mm tolerance.
#[derive(Clone, Copy)]
enum Loose {
    None,
    /// Every edge of the first face use of each shell.
    FirstFaceOfEachShell,
    All,
}

fn cube(k: &mut Kernel, loose: Loose) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let edges = match loose {
        Loose::None => Vec::new(),
        Loose::All => p1::edges(&b, None).unwrap(),
        Loose::FirstFaceOfEachShell => p1::regions(&b, None)
            .unwrap()
            .into_iter()
            .flat_map(|r| b.region(r).unwrap().shells.clone())
            .flat_map(|s| {
                let fu = b.shell(s).unwrap().first_faceuse().unwrap().unwrap();
                faceuse_edgeuses(&b, fu, false, None).unwrap()
            })
            .map(|eu| b.edgeuse(eu).unwrap().edge.unwrap())
            .collect(),
    };
    for e in edges {
        b.edge_mut(e).unwrap().tolerance = 1.0;
    }
    b
}

fn far_union(loose: Loose) -> (Kernel, Result<Brep>) {
    let mut k = Kernel::default();
    let near = cube(&mut k, loose);
    let far = create_box(&mut k, Point3::new(5000.0, 5000.0, 5000.0), 1.0, 1.0, 1.0).unwrap();
    let joined = boolean_union(&mut k, near, far);
    (k, joined)
}

/// The union holds both cubes as separate solids: the infinite region and two more,
/// 12 faces, the volumes summed.
fn assert_two_cubes(k: &Kernel, mut joined: Brep) {
    assert_eq!(p1::regions(&joined, None).unwrap().len(), 3);
    assert_eq!(p1::faces(&joined, None).unwrap().len(), 12);
    let volume = brep_volume_default(k, &mut joined, 1e-7).unwrap();
    assert!((volume - 1001.0).abs() < 1e-3, "volume {volume}");
}

#[test]
fn a_far_cube_joins_a_cube_whose_first_face_is_narrower_than_the_step() {
    let (k, joined) = far_union(Loose::FirstFaceOfEachShell);
    assert_two_cubes(&k, joined.unwrap());
}

#[test]
fn a_far_cube_joins_a_plain_cube_as_before() {
    let (k, joined) = far_union(Loose::None);
    assert_two_cubes(&k, joined.unwrap());
}

#[test]
fn a_shell_with_no_face_wide_enough_for_the_step_is_still_refused() {
    let (_, joined) = far_union(Loose::All);
    let err = joined.unwrap_err();
    assert_eq!(
        err.topology_failure(),
        Some(&TopologyFailure::SteppedOffFace),
        "{err:?}"
    );
}
