//! Regression test for WR-596 (the fork's tangency-labs/smlib@0df62c8, @35008b1 and
//! @449c81c; SolidCraft's capi smoke test `smlib_make_box`): `SmApiCreateBox` of a box
//! far smaller than the tolerance (`smlib_fuzz` leak-000bffce). Pinned `CreateBox` drops
//! every face and stitch status and returns success with a 4-face body that fails its
//! own check; the fork returns the failing status (1001). With
//! `Kernel::fixes.box_face_failures_propagate` the port's `create_box` does the same.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::{Brep, Kernel, SolidError};

/// The fuzz case's origin and size (the record's hex floats).
fn tiny_box(fix: bool) -> solidcore::Result<Brep> {
    let mut k = Kernel::default();
    k.fixes.box_face_failures_propagate = fix;
    create_box(
        &mut k,
        Point3::new(
            7.041_528_834_472_482e-9,
            7.041_529_113_171_147e-9,
            7.041_529_113_171_147e-9,
        ),
        7.041_529_113_171_145_5e-9,
        3.040_193_435_271_801_5e-5,
        7.042_438_607_872_92e-9,
    )
}

#[test]
fn wr_596_tiny_box_is_sm_err_with_the_fix() {
    let r = tiny_box(true);
    assert!(
        matches!(r, Err(SolidError::Upstream { code: 1001, .. })),
        "{:?}",
        r.map(|b| b.face_list().len())
    );
}

#[test]
fn wr_596_tiny_box_is_a_four_face_success_by_default() {
    let b = tiny_box(false).unwrap();
    assert_eq!(b.face_list().len(), 4);
}

#[test]
fn wr_596_an_ordinary_box_is_unchanged_by_the_fix() {
    for fix in [false, true] {
        let mut k = Kernel::default();
        k.fixes.box_face_failures_propagate = fix;
        let b = create_box(&mut k, Point3::new(1.0, 2.0, 3.0), 4.0, 5.0, 6.0).unwrap();
        assert_eq!(b.face_list().len(), 6, "fix {fix}");
    }
}
