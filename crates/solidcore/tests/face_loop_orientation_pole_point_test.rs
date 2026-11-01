//! `sm_AreConnected` tests the ends of a UV gap for a singularity with
//! `IsSingularity(uv, dir, tol, bPtTestOnly = TRUE)`; the port passed `FALSE`, which also
//! runs the surface tests. On a sphere whose seam vertex near the pole was moved by 1e-4
//! and grown to 2e-4 (as `ValidateAndUpdateTolerances(FALSE)` grows it), the pinned C++
//! does not join the trims across the pole there (`ComputeLoopOrientation` gives
//! `SM_OT_UNKNOWN`, and `ValidatePointers` reports the loop's bad orientation and zero
//! area); the port joined them and gave `Same` (difftest `topology.assert_valid`,
//! `update_tolerances.json`, `update_sphere_m6_*`;
//! `plan/findings/topology.face_p3--moved-sphere-vertex-loop-orientation.md`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)] // test helpers

use solidcore::geom::assert_array::AssertArray;
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::object::ObjectRef;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::topology::brep::validation::validate_pointers;
use solidcore::topology::VertexId;
use solidcore::{EntityRef, Kernel};

#[test]
fn moved_grown_pole_vertex_does_not_join_the_trims() {
    let mut k = Kernel::default();
    let mut b =
        solidcore::primitive::api_primitives::create_sphere(&mut k, Vec3::new(0.0, 0.0, 0.0), 1.0)
            .unwrap();
    let v = b.vertex_mut(VertexId::from_index(0)).unwrap();
    v.point.x += 1e-4;
    v.tolerance = 2e-4;
    let faces: Vec<_> = b.faces().ids().collect();
    assert_eq!(faces.len(), 1);
    let f = faces[0];
    let loops = solidcore::topology::face::p2::loops(&b, f, false, None).unwrap();
    assert_eq!(loops.len(), 1);
    let o = solidcore::topology::face::p3::compute_loop_orientation(
        &k, &mut b, f, loops[0], None, false,
    )
    .unwrap();
    assert_eq!(o, OrientType::Unknown);

    // and the pointer check reports the loop as the C++ does
    let mut reports = AssertArray::new();
    let key = |_: EntityRef| ObjectRef {
        key: ObjectKey(0),
        type_code: 0,
    };
    validate_pointers(&k, &mut b, Some(&mut reports), 2, false, &key).unwrap();
    let failed: Vec<String> = reports
        .reports()
        .iter()
        .filter(|r| !r.ok)
        .map(solidcore::geom::assert_array::AssertReport::format_log_message)
        .collect();
    assert!(
        failed
            .iter()
            .any(|m| m.contains("Face Loop has bad orientation")),
        "{failed:?}"
    );
}
