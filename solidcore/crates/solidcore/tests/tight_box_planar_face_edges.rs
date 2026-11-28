//! The work of a B-rep's tight box, as a count: a planar face does not solve its curved
//! edges' tight boxes again.
//!
//! A planar face's tight box is its vertices' and its edges'. The B-rep's box has taken
//! those in its vertex and edge passes before it reaches the faces, so solving the face
//! only solves its curved edges a second time. On a plate with many round holes that was
//! half of the box's edge solves.
//! [`Kernel::work_counters`] counts each edge curve's tight box (`edge_tight_box_solves`),
//! the same on every machine.
#![allow(clippy::unwrap_used)] // test

use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::query::brep::calculate_tight_bounding_box;
use solidcore::Kernel;

/// A cylinder has two curved edges, its rims, each on a planar cap and on the wall. Its
/// vertices lie on the seam alone, so neither rim's loose box lies inside the box of the
/// vertices and the edge pass solves both: 2. The wall is curved, and its loose box
/// reaches past its surface, so it is solved, with both rims again: 2. The caps have
/// nothing to add, so they solve nothing (each would solve its rim: 6 in all).
#[test]
fn a_cylinder_s_caps_do_not_solve_its_rims_again() {
    let mut k = Kernel::default();
    let b = create_cylinder(&mut k, Point3::new(1.0, 2.0, 3.0), 5.0, 20.0).unwrap();

    k.work_counters.reset();
    let tight = calculate_tight_bounding_box(&k, &b).unwrap();

    // The box is the cylinder's: radius 5 about the axis through (1, 2), 20 up from z = 3.
    let (lo, hi) = (tight.min(), tight.max());
    for (got, want) in [lo.x, lo.y, lo.z, hi.x, hi.y, hi.z]
        .into_iter()
        .zip([-4.0, -3.0, 3.0, 6.0, 7.0, 23.0])
    {
        assert!((got - want).abs() <= 1e-9, "{got} for {want}");
    }
    assert_eq!(k.work_counters.edge_tight_box_solves(), 4);
}
