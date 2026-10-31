//! Regression for `offset.offset_executive_p2` fuzz00087: the temporary B-rep's
//! tolerance passes through C++'s `SmZoneTol3d` conversion, which takes fabs.
#![allow(clippy::unwrap_used)]

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::Notifications;
use solidcore::math::Point3;
use solidcore::offset::offset_executive::p1::{OffsetExecutive, OffsetOperation};
use solidcore::offset::offset_executive::p2::SolidOffsetOptions;
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::Kernel;

#[test]
fn fuzz00087_temporary_brep_tolerance_is_positive() {
    let mut kernel = Kernel::default();
    let mut original = create_cylinder(&mut kernel, Point3::new(1.0, -2.0, 0.5), 3.0, 5.0).unwrap();
    original.set_tolerance(1e300);
    let mut notifications = Notifications::new(&mut original);
    let mut executive = OffsetExecutive::new(OffsetOperation::SolidOffset, 0.5, -1e-300);
    executive.set_inset(true);
    executive.set_extend_convex_edges(true);
    executive.set_merge_results(false);
    let result = executive
        .do_solid_offset(
            &mut kernel,
            Some(&mut original),
            SolidOffsetOptions::default(),
            &mut notifications.context(),
        )
        .unwrap();
    assert_eq!(result.tolerance(), 1e-300);
    assert_eq!(executive.approximation_tol_3d(), -1e-300);
}
