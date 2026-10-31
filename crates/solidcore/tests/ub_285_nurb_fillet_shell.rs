//! UB-285: `SmApiShellBrepFull` of `Solids/nurb_fillet.smb` by -0.5 (extended convex
//! edges, merged results, no offset solid) with one face open crashes the C++ reference
//! for 19 of its 20 faces (all but face 4): `SmOffsetExecutive::GetCreateExtendedSurface`
//! calls through an extended surface that `DoExtendedOffset` deleted and left in
//! `m_vSurfExtSurf`. The port holds no such cache of raw pointers; this checks that it
//! returns, without a panic and within a time budget, with faces 0 and 1 open (both crash
//! the C++). In the port, at the time of writing, face 0's shell fails with `SM_ERR` and
//! face 1's builds a 22-face body. Every face returns in about 35 s in release (checked
//! once; only two run here, to keep the suite short).
//!
//! The solid is the upstream test file, read when the pinned upstream checkout is present
//! (the test is skipped otherwise, as in CI's test job).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, EntityRef, Kernel};

fn nurb_fillet(k: &mut Kernel) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Solids/nurb_fillet.smb");
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

/// Generous for the debug build on a loaded machine.
const BUDGET: std::time::Duration = std::time::Duration::from_secs(1800);

#[test]
#[ignore = "slow: UB-285 regression, run with --ignored"]
fn ub_285_nurb_fillet_shell_returns_for_every_open_face() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut k = Kernel::default();
        let Some(part) = nurb_fillet(&mut k) else {
            tx.send(None).unwrap();
            return;
        };
        let faces = part.face_list().to_vec();
        let mut outcomes = Vec::new();
        for (i, &face) in faces.iter().enumerate().take(2) {
            let mut copy = part.clone();
            let r = offset_fixture::api_shell(
                &mut k,
                &mut copy,
                ShellOptions {
                    offset: OffsetOptions {
                        distance: -0.5,
                        extend_convex_edges: true,
                        merge_results: true,
                    },
                    create_offset_solid: false,
                    merge_sheet_side_walls: false,
                },
                &[face],
            );
            outcomes.push(format!(
                "face {i}: {:?}",
                r.as_ref().map(|b| b.face_list().len())
            ));
        }
        tx.send(Some(outcomes)).unwrap();
    });
    match rx.recv_timeout(BUDGET) {
        Ok(Some(outcomes)) => {
            assert_eq!(outcomes.len(), 2, "{outcomes:?}");
            for o in outcomes {
                eprintln!("{o}");
            }
        }
        Ok(None) => {}
        Err(e) => panic!("the shells did not return within {BUDGET:?}: {e}"),
    }
}
