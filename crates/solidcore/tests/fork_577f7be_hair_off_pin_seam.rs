//! The fork's own regression test for WR-537 (tangency-labs/smlib@577f7be, capi test
//! `booleans` `hair_off_pin_at_a_seam`), ported: SolidCraft's inspect.interference. A pin
//! of radius 3 a hair (1e-6, inside the tolerance) off the axis of a hole of its own
//! radius through a 20 x 20 x 5 plate. Both cylinders are extruded from a circle whose
//! seam is at 0.6131 rad (SolidCraft's cylinders), so the two seams lie a micrometre
//! apart. Their walls are one surface to the tolerance: the plate and the pin share no
//! material. Upstream (and the port's default, as the pinned C++ 1.0.1 does) fails with
//! `SM_ERR` (1001), because a breakpoint on one seam solves to both ends of the other
//! wall's period and is dropped as unclassified. With `seam_point_solutions_are_one`
//! the intersection is empty, as the fork's test requires (`SMLIB_ERR_EMPTY` there).
//!
//! The fork's test also intersects a pin 0.01 off, expecting the crescent; that case needs
//! another fork change (the pinned C++ and the port fail it alike), so it is not here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::{boolean_difference, boolean_intersection};
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_box;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::{Brep, Kernel, SolidError};

/// The seam angle of SolidCraft's cylinders.
const SEAM: f64 = 0.6131;

/// A cylinder r 3 about (x, 0) from z = -2 up 9, extruded from a circle whose seam is at
/// `SEAM`.
fn pin(k: &mut Kernel, x: f64) -> Brep {
    let frame = Axis2Placement::new(
        Point3::new(x, 0.0, -2.0),
        Vec3::new(SEAM.cos(), SEAM.sin(), 0.0),
        Vec3::new(-SEAM.sin(), SEAM.cos(), 0.0),
    );
    let circle = Curve::from(Ellipse::create_canonical_circle(k, &frame, 3.0, None, None).unwrap());
    create_linear_sweep(k, &[Some(&circle)], Vec3::new(0.0, 0.0, 1.0), 9.0, true).unwrap()
}

/// The plate with its hole, intersected with the pin 1e-6 off, with WR-537's `fixed`.
fn shared(fixed: bool) -> solidcore::Result<Brep> {
    let mut k = Kernel::default();
    k.fixes.seam_point_solutions_are_one = fixed;
    let plate = create_box(&mut k, Point3::new(-10.0, -10.0, 0.0), 20.0, 20.0, 5.0).unwrap();
    let drill = pin(&mut k, 0.0);
    let holed = boolean_difference(&mut k, plate, drill).unwrap();
    let hair_off = pin(&mut k, 1e-6);
    boolean_intersection(&mut k, holed, hair_off)
}

#[test]
fn fork_577f7be_hair_off_pin_shares_nothing_with_the_fix() {
    let b = shared(true).unwrap();
    assert!(b.face_list().is_empty(), "{} faces", b.face_list().len());
}

/// Upstream (pinned C++ 1.0.1 alike): `SM_ERR` from the curve classification.
#[test]
fn fork_577f7be_hair_off_pin_fails_by_default() {
    let r = shared(false);
    assert!(
        matches!(r, Err(SolidError::Upstream { code: 1001, .. })),
        "{:?}",
        r.map(|b| b.face_list().len())
    );
}
