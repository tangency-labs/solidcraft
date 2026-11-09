//! Regression test for WR-537 (the fork's tangency-labs/smlib@577f7be, capi test
//! `booleans hair_off_pin_at_a_seam`): a plate with an r 3 hole intersected with a pin
//! of the hole's radius a micrometre off its axis, both cylinders with their seams at
//! the same angle, so the two seams lie 1e-6 apart. A point on the pin's seam has its
//! face parameters at both ends of the period; upstream's `ComputePointParameters`
//! finds no single solution, drops the point to `SM_PC_UNKNOWN`, and the boolean fails
//! in `Homogenize` (`SM_ERR`). With `seam_point_solutions_are_one` the walls are one
//! surface to the tolerance and the intersection is empty.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::{boolean_difference, boolean_intersection};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::{Kernel, Vec3};

fn intersect(fixed: bool) -> solidcore::Result<solidcore::Brep> {
    let mut k = Kernel::default();
    k.fixes.seam_point_solutions_are_one = fixed;
    let plate = create_box(&mut k, Vec3::new(0.0, 0.0, 0.0), 40.0, 40.0, 10.0).unwrap();
    let hole = create_cylinder(&mut k, Vec3::new(20.0, 20.0, -1.0), 3.0, 12.0).unwrap();
    let holed = boolean_difference(&mut k, plate, hole).unwrap();
    let pin = create_cylinder(&mut k, Vec3::new(20.0 + 1e-6, 20.0, -5.0), 3.0, 20.0).unwrap();
    boolean_intersection(&mut k, holed, pin)
}

#[test]
fn wr_537_hair_off_pin_is_empty_with_the_fix() {
    let b = intersect(true).unwrap();
    assert!(b.face_list().is_empty(), "{} faces", b.face_list().len());
}
