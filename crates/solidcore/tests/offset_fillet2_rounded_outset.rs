//! Regression test: rounded solid outsets of `Fillet/brepForFillet2.smb` with face 0
//! kept as the shell face (approximation tolerance 1e-5, merged results).
//!
//! `BuildOffsetOfFaceuse` deletes a new offset face when a point of one of its lamina
//! edges lies closer than the offset distance to the original surface
//! (`SmOffsetExecutive.cpp:3790-3835`, `SmTopologyIntersector::DeleteFaces`). The
//! port ran that deletion inside the offset's lent notification context, so it failed
//! as a nested edit and, its status ignored, kept the faces: three extra offset faces
//! at distance 5.6, five at 11.2. At 11.2 the extra faces made coincident proto edges
//! that `ResolveOverlappingProtoEdges` rebuilt forever (C++ finishes in seconds; the
//! port ran past 30 minutes). The face counts are the C++ reference's (difftest unit
//! `offset.offset_executive_p2`); in release the full offset takes well under a second.
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
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, EntityRef, Kernel};

fn fillet2(k: &mut Kernel) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Fillet/brepForFillet2.smb");
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
        next_key: 1 << 40,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.use_arena_naming().unwrap();
    Some(b)
}

/// `DoSolidOffset` of the part at `distance`, face 0 the shell face.
fn offset(distance: f64, skip_stitch_merge: bool) -> Option<Brep> {
    let mut k = Kernel::default();
    let mut part = fillet2(&mut k)?;
    let face = part.face_list()[0];
    let mut exec = OffsetExecutive::new(OffsetOperation::SolidOffset, distance, 1e-5);
    exec.set_inset(false);
    exec.set_extend_convex_edges(false);
    exec.set_merge_results(true);
    exec.set_shell_faces(&[face]);
    let mut notifications = Notifications::new(&mut part);
    Some(
        exec.do_solid_offset(
            &mut k,
            Some(&mut part),
            SolidOffsetOptions { skip_stitch_merge },
            &mut notifications.context(),
        )
        .unwrap(),
    )
}

#[test]
fn offsets_deleted_by_lamina_edge_test_are_gone_before_the_merge() {
    // C++: 20 faces at 5.6 and 12 at 11.2 (the port kept 23 and 17)
    for (distance, faces) in [(5.6, 20), (11.2, 12)] {
        let Some(b) = offset(distance, true) else {
            return;
        };
        assert_eq!(b.face_list().len(), faces, "distance {distance}");
    }
}

#[test]
fn rounded_outset_at_11_2_finishes() {
    let Some(b) = offset(11.2, false) else {
        return;
    };
    assert_eq!(
        (b.face_list().len(), b.edges().len(), b.vertices().len()),
        (13, 40, 29)
    );
}
