//! A rounded solid outset of one face of `Boolean/extrudeHoles1.smb` far enough out
//! (22.75, 0.35 of its 65 bounding-box size) that `SmOffsetExecutive::PiecewiseMerge`'s
//! manifold boolean reaches `SmProtoTopologyManager::ResolveOverlappingProtoEdges`.
//! There `SetPointClassParameters` solves points on the operands' faces, which makes
//! each face's trimmed surface cache (`smsurf_GetSurfaceCache`) on its B-rep. The port
//! called the read-only classification there, so the solve stopped at
//! `Unported(query.face_p2)` ("a face's new trimmed cache from read-only topology");
//! the B-rep owner now lends the face's B-rep, as upstream edits it in place.
//!
//! The expected counts are the C++ reference's (difftest
//! `offset.offset_executive_p2` `overlap_pc_extrudeHoles1_f0_k0.35_out_rnd`). The
//! solid is the upstream test file, read by the Rust reader when the pinned upstream
//! checkout is present (as the difftest SMS tests do).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::Notifications;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::offset::offset_executive::p1::{OffsetExecutive, OffsetOperation};
use solidcore::offset::offset_executive::p2::SolidOffsetOptions;
use solidcore::topology::brep::p1 as brep_p1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, EntityRef, Kernel};

/// `Boolean/extrudeHoles1.smb` read by the Rust reader and arena-named, as the difftest
/// reader (`own_reader::read_file`) makes it.
fn extrude_holes(k: &mut Kernel) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Boolean/extrudeHoles1.smb");
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("unavailable upstream test file: {}", path.display());
        return None;
    };
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = |_: EntityRef| ObjectRef {
        key: ObjectKey(0),
        type_code: 0,
    };
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.use_arena_naming().unwrap();
    Some(b)
}

#[test]
fn rounded_outset_sets_point_class_parameters_on_the_lent_brep() {
    let mut k = Kernel::default();
    let Some(mut b) = extrude_holes(&mut k) else {
        return;
    };
    let face = b.face_list()[0];
    let mut executive =
        OffsetExecutive::new(OffsetOperation::SolidOffset, 22.75, 0.005_957_493_548_898_4);
    executive.set_inset(false);
    executive.set_extend_convex_edges(false);
    executive.set_merge_results(true);
    executive.set_shell_faces(&[face]);
    let mut notifications = Notifications::new(&mut b);
    let result = executive.do_solid_offset(
        &mut k,
        Some(&mut b),
        SolidOffsetOptions::default(),
        &mut notifications.context(),
    );
    let result = match result {
        Ok(r) => r,
        Err(e) => panic!("solid offset failed: {e}"),
    };
    // the live lists, as the shim's GetFaces / GetEdges / GetVertices walk them
    let counts = (
        brep_p1::faces(&result, None).unwrap().len(),
        brep_p1::edges(&result, None).unwrap().len(),
        brep_p1::vertices(&result, None).unwrap().len(),
    );
    assert_eq!(counts, (47, 100, 53));
}
