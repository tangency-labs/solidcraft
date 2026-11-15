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
//! port ran past 30 minutes). The C++ reference keeps 20 faces at 5.6 and 12 at 11.2.
//!
//! Face 2 bends with radius 3.96 towards the outset, so its offset folds at both
//! distances: the offset is refused before anything is built, naming that radius as the
//! largest wall that works (#2350 S1, D5), and neither the deletion nor the proto-edge
//! loop is reached from this part. The deletion stays covered by
//! `offset_shell_lamina_face_delete.rs`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::Notifications;
use solidcore::error::offset::OffsetFailure;
use solidcore::error::{ErrorKind, Reason};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::offset::offset_executive::p1::{OffsetExecutive, OffsetOperation};
use solidcore::offset::offset_executive::p2::SolidOffsetOptions;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::FaceId;
use solidcore::{Brep, EntityRef, Kernel, SolidError};

fn fillet2(k: &mut Kernel) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Fillet/brepForFillet2.smb");
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
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

/// `DoSolidOffset` of the part at `distance`, face 0 the shell face, and the part's
/// face 2; `None` (the test is skipped) without the pinned upstream checkout.
fn offset(distance: f64, skip_stitch_merge: bool) -> Option<(Result<Brep, SolidError>, FaceId)> {
    let mut k = Kernel::default();
    let mut part = fillet2(&mut k)?;
    let faces = part.face_list();
    let (face, face2) = (faces[0], faces[2]);
    let mut exec = OffsetExecutive::new(OffsetOperation::SolidOffset, distance, 1e-5);
    exec.set_inset(false);
    exec.set_extend_convex_edges(false);
    exec.set_merge_results(true);
    exec.set_shell_faces(&[face]);
    let mut notifications = Notifications::new(&mut part);
    let result = exec.do_solid_offset(
        &mut k,
        Some(&mut part),
        SolidOffsetOptions { skip_stitch_merge },
        &mut notifications.context(),
    );
    Some((result, face2))
}

#[test]
fn rounded_outsets_past_face_2s_radius_are_refused_naming_it() {
    for (distance, skip_stitch_merge) in [(5.6, true), (11.2, true), (11.2, false)] {
        let Some((result, face2)) = offset(distance, skip_stitch_merge) else {
            return;
        };
        let e = result.unwrap_err();
        assert_eq!(e.kind(), ErrorKind::OffsetFolds, "{e}");
        let Some(&Reason::Offset(OffsetFailure::Folds {
            face,
            distance: d,
            largest,
        })) = e.failure().map(|f| &f.reason)
        else {
            panic!("{e}");
        };
        assert_eq!(face, face2);
        assert_eq!(d, distance);
        // face 2's least radius of curvature on the outset side, measured: 3.9579840524301826
        assert!(
            (largest - 3.957_984_052_430_182_6).abs() < 1e-9,
            "largest {largest}"
        );
    }
}
