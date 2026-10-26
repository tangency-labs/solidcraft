//! tangency-labs/solidcore#528 (WR-254): `SM_BO_SLICE` by an oriented sheet lying in a
//! face of the solid. Upstream's steps 7.3e / 7.B give the slice the difference rule,
//! which never deletes a coincident pair against a sheet (void on both sides), so the
//! face lying in the sheet is kept in the part and the part is not a manifold solid.
//! That is the default (SMLib, confirmed on the C++ build by the `boolean.api_brep`
//! difftest cases `wr254_slice_box_by_its_top_face` and
//! `wr254_slice_slab_tower_in_slab_face`); the opt-in
//! `Kernel::fixes.slice_coincident_faces_sheet_side` keeps such a pair only on the side
//! the sheet's `SM_OT_SAME` faceuse bounds.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::{boolean, boolean_union};
use solidcore::geom::a_object::BooleanOperation;
use solidcore::primitive::api_primitives::{create_box, create_plane};
use solidcore::topology::brep::p1::num_faces;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::{Brep, Kernel, Point3};

/// A slab 40 x 40 x 5 with a tower 10 x 10 x 10 on its top, sliced by a sheet in the
/// slab's top face (z = 5).
fn slab_and_tower_sliced(k: &mut Kernel) -> Brep {
    let slab = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 40.0, 5.0).unwrap();
    let tower = create_box(k, Point3::new(15.0, 15.0, 5.0), 10.0, 10.0, 10.0).unwrap();
    let body = boolean_union(k, slab, tower).unwrap();
    let sheet = create_plane(k, Point3::new(-10.0, -10.0, 5.0), 60.0, 60.0).unwrap();
    boolean(k, body, sheet, BooleanOperation::Slice)
        .unwrap()
        .result
}

#[test]
fn issue_528_slice_in_a_face_keeps_a_manifold_part() {
    let mut k = Kernel::default();
    k.fixes.slice_coincident_faces_sheet_side = true;
    let part = slab_and_tower_sliced(&mut k);
    assert_eq!(num_faces(&part), 6);
    assert!(is_manifold_solid(&part).unwrap());
}

/// By default (SMLib) the slab's coincident top face survives in the part.
#[test]
fn issue_528_slice_in_a_face_keeps_the_coincident_face_by_default() {
    let mut k = Kernel::default();
    let part = slab_and_tower_sliced(&mut k);
    assert_eq!(num_faces(&part), 7);
    assert!(!is_manifold_solid(&part).unwrap());
}

/// A box 10^3 sliced by a 20 x 20 sheet in the plane of its top face: by default the top face is kept on its
/// own (one face, a lamina), as the C++ build gives; with the fix it is deleted too.
#[test]
fn issue_528_box_sliced_by_its_top_face() {
    for fix in [false, true] {
        let mut k = Kernel::default();
        k.fixes.slice_coincident_faces_sheet_side = fix;
        let b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
        let sheet = create_plane(&mut k, Point3::new(-5.0, -5.0, 10.0), 20.0, 20.0).unwrap();
        let part = boolean(&mut k, b, sheet, BooleanOperation::Slice)
            .unwrap()
            .result;
        assert_eq!(num_faces(&part), usize::from(!fix), "fix: {fix}");
        assert!(!is_manifold_solid(&part).unwrap(), "fix: {fix}");
    }
}
