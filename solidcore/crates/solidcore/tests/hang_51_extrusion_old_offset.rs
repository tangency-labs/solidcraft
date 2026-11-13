//! HANG-51: `SmProtoTopologyManager::ResolveOverlappingProtoEdges` does not advance its
//! index after it replaces an overlapping proto edge group, and on
//! `Solids/extrusion_old.smb` offset outward by 2.5 (rounded, merged) the replacement of
//! a two-edge group forms a new two-edge group at the same index, forever. The C++
//! reference never returns there; neither did the port. The port now fails once one call
//! has replaced more groups than its bound allows (module deviations of
//! `topology::proto_topology::p3`).
//!
//! Face 7 of the part bends with radius 0.97 towards the outset, so its offset folds at
//! 2.5: the offset is now refused before anything is built, naming that radius as the
//! largest wall that works (#2350 S1, D5), and the loop is no longer reached from this
//! part. The bound stays (a hang fix is unconditional); no other known input reaches it.
//!
//! The solid is the upstream test file, read when the pinned upstream checkout is present
//! (the test is skipped otherwise). The offset runs on its own thread, so a regression
//! that hangs fails at the time budget instead of hanging the suite.
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
use solidcore::{Brep, EntityRef, Kernel};

fn extrusion_old(k: &mut Kernel) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Solids/extrusion_old.smb");
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

/// Generous for the debug build on a loaded machine (the bounded call took about 100 s
/// there and 3 s in release; the refusal comes well before).
const BUDGET: std::time::Duration = std::time::Duration::from_secs(1200);

#[test]
fn hang_51_extrusion_old_rounded_outset_is_refused_at_its_fold() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut k = Kernel::default();
        let Some(mut part) = extrusion_old(&mut k) else {
            tx.send(None).unwrap();
            return;
        };
        // `OffsetExecutive` solid offset, outset 2.5, rounded (no convex extension),
        // merged results, member approximation tolerance 1e-4
        let mut exec = OffsetExecutive::new(OffsetOperation::SolidOffset, 2.5, 1e-4);
        exec.set_inset(false);
        exec.set_extend_convex_edges(false);
        exec.set_merge_results(true);
        let face7 = part.face_list()[7];
        let mut notifications = Notifications::new(&mut part);
        let result = exec
            .do_solid_offset(
                &mut k,
                Some(&mut part),
                SolidOffsetOptions::default(),
                &mut notifications.context(),
            )
            .map(|b| b.face_list().len());
        tx.send(Some((result, face7))).unwrap();
    });
    let result = match rx.recv_timeout(BUDGET) {
        Ok(None) => return,
        Ok(Some(result)) => result,
        Err(e) => panic!("the offset did not return within {BUDGET:?} (HANG-51): {e}"),
    };
    let (result, face7) = result;
    let e = result.unwrap_err();
    assert_eq!(e.kind(), ErrorKind::OffsetFolds, "{e}");
    let Some(&Reason::Offset(OffsetFailure::Folds {
        face,
        distance,
        largest,
    })) = e.failure().map(|f| &f.reason)
    else {
        panic!("{e}");
    };
    assert_eq!(face, face7);
    assert_eq!(distance, 2.5);
    // face 7's least radius of curvature on the outset side, measured: 0.9689470076317032
    assert!(
        (largest - 0.968_947_007_631_703_2).abs() < 1e-9,
        "largest {largest}"
    );
}
