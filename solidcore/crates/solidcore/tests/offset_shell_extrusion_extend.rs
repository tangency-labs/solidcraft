//! An extended solid offset of `Defeature/extrusion.smb` keeping face 0 as a shell face,
//! inset by 2 (the `offset.offset_executive_p2` case `defeature_extrusion_face0_inset2_extended`).
//!
//! Inset by 2, the part's convex round of radius 2 (face 5's corner at (2, 8)) offsets onto
//! a line: its offset collapses. Built on regardless, the edges `MapEdgeuses` lifts across it
//! are 1.69 off its offset surface, and every later trim curve raises the tolerances again,
//! to 11.46 and 20. No body built that way is sound:
//! - the C++ reference (11 faces, 34 edges, 14 vertices) carries edge tolerances 16.06 and
//!   6.77 on a B-rep of tolerance 1e-4, and edges whose ends lie about 3 from any vertex;
//! - on the exact extension the port gave 11 faces, 32 edges, 13 vertices, and Level2
//!   reported an edge/edge gap 4.14.
//!
//! So the offset refuses it, naming why (#2350; owner ruling): a face whose offset is proved
//! to collapse is refused before anything is built (`OffsetFailure::Collapses`, radius 2).
//! Face 2's profile also bends with radius 1.53 towards the inset (at u = 2.68 of 0..4.37),
//! so its offset folds, and face 2 comes before face 5: kept as a shell face only face 0,
//! the inset is refused at face 2 (`OffsetFailure::Folds`, the largest wall 1.53, D5);
//! kept with face 2 as well, at face 5's collapse.
//!
//! When #2350 builds the offset as one construction (exact offsets and mitres: the faces
//! beside a collapsed round meet across it), this becomes the sound-body assertion it
//! replaces: `check_brep` clean at level 2, the inner walls exact (x = 2, y = 8 and the
//! corner at (2, 8)), least vertex x 2.
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
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::FaceId;
use solidcore::{Brep, EntityRef, Kernel, SolidError};

/// The upstream test file, read as the difftest runner reads it; `None` (the test is
/// skipped) without the pinned upstream checkout.
fn extrusion(k: &mut Kernel) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Defeature/extrusion.smb");
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
        next_key: 1 << 50,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.use_arena_naming().unwrap();
    Some(b)
}

/// The extended solid offset of the part, inset by 2, keeping `kept` as shell faces.
fn inset_by_two(k: &mut Kernel, part: &mut Brep, kept: &[FaceId]) -> Result<Brep, SolidError> {
    let mut executive = OffsetExecutive::new(OffsetOperation::SolidOffset, 2.0, 1e-4);
    executive.set_inset(true);
    executive.set_extend_convex_edges(true);
    executive.set_merge_results(false);
    executive.set_create_offset_solid(false);
    executive.set_shell_faces(kept);
    let mut notifications = Notifications::new(part);
    executive.do_solid_offset(
        k,
        Some(part),
        SolidOffsetOptions::default(),
        &mut notifications.context(),
    )
}

/// The offset failure `e` carries.
fn offset_failure(e: &SolidError) -> OffsetFailure {
    match e.failure().map(|f| &f.reason) {
        Some(Reason::Offset(failure)) => *failure,
        other => panic!("not an offset refusal: {other:?} ({e})"),
    }
}

#[test]
fn inset_by_the_rounds_radius_is_refused_naming_why() {
    let mut k = Kernel::default();
    let Some(mut part) = extrusion(&mut k) else {
        return;
    };
    let faces = bp1::faces(&part, None).unwrap();
    let e = inset_by_two(&mut k, &mut part, &[faces[0], faces[2]]).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::OffsetCollapses, "{e}");
    let OffsetFailure::Collapses {
        face,
        distance,
        radius,
    } = offset_failure(&e)
    else {
        panic!("{e}");
    };
    assert_eq!(face, faces[5]);
    assert_eq!(distance.abs(), 2.0);
    assert!((radius - 2.0).abs() < 1e-9, "radius {radius}");
    assert!(e.to_string().contains("collapses"), "{e}");
}

#[test]
fn inset_past_face_2s_radius_is_refused_naming_its_largest_wall() {
    let mut k = Kernel::default();
    let Some(mut part) = extrusion(&mut k) else {
        return;
    };
    let faces = bp1::faces(&part, None).unwrap();
    let e = inset_by_two(&mut k, &mut part, &[faces[0]]).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::OffsetFolds, "{e}");
    let OffsetFailure::Folds {
        face,
        distance,
        largest,
    } = offset_failure(&e)
    else {
        panic!("{e}");
    };
    assert_eq!(face, faces[2]);
    assert_eq!(distance.abs(), 2.0);
    // face 2's least radius of curvature on the inset side, measured: 1.5331125921683275
    assert!((largest - 1.533_112_592_168_327_5).abs() < 1e-9, "largest {largest}");
}
