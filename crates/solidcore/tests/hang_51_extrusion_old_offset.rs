//! HANG-51: `SmProtoTopologyManager::ResolveOverlappingProtoEdges` does not advance its
//! index after it replaces an overlapping proto edge group, and on
//! `Solids/extrusion_old.smb` offset outward by 2.5 (rounded, merged) the replacement of
//! a two-edge group forms a new two-edge group at the same index, forever. The C++
//! reference never returns there; neither did the port. The port now fails once one call
//! has replaced more groups than its bound allows (module deviations of
//! `topology::proto_topology::p3`).
//!
//! The solid is the upstream test file, read when the pinned upstream checkout is present
//! (the test is skipped otherwise). The offset runs on its own thread: without the bound
//! it never ends, and the test fails at the time budget instead of hanging the suite.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::Notifications;
use solidcore::error::SolidError;
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

/// Generous for the debug build on a loaded machine (the bounded call takes about 100 s
/// there and 3 s in release).
const BUDGET: std::time::Duration = std::time::Duration::from_secs(1200);

#[test]
fn hang_51_extrusion_old_rounded_outset_returns() {
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
        let mut notifications = Notifications::new(&mut part);
        let result = exec
            .do_solid_offset(
                &mut k,
                Some(&mut part),
                SolidOffsetOptions::default(),
                &mut notifications.context(),
            )
            .map(|b| b.face_list().len());
        tx.send(Some(result)).unwrap();
    });
    let result = match rx.recv_timeout(BUDGET) {
        Ok(None) => return,
        Ok(Some(result)) => result,
        Err(e) => panic!("the offset did not return within {BUDGET:?} (HANG-51): {e}"),
    };
    // the C++ has no result to compare with; the bound ends the call with its error
    match result {
        Err(SolidError::InvalidInput { reason, .. }) => {
            assert!(reason.contains("HANG-51"), "{reason}");
        }
        other => panic!("expected the HANG-51 bound's error, got {other:?}"),
    }
}
