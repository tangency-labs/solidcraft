//! A rounded solid outset of one face of `Boolean/extrudeHoles1.smb` far enough out
//! (22.75, 0.35 of its 65 bounding-box size) that `SmOffsetExecutive::PiecewiseMerge`'s
//! manifold boolean reaches `SmProtoTopologyManager::ResolveOverlappingProtoEdges`.
//! There `SetPointClassParameters` solves points on the operands' faces, which makes
//! each face's trimmed surface cache (`smsurf_GetSurfaceCache`) on its B-rep. The port
//! called the read-only classification there, so the solve stopped at
//! `Unported(query.face_p2)` ("a face's new trimmed cache from read-only topology");
//! the B-rep owner now lends the face's B-rep, as upstream edits it in place.
//!
//! The C++ reference builds a body of 47 faces, 100 edges and 53 vertices (difftest
//! `offset.offset_executive_p2` `overlap_pc_extrudeHoles1_f0_k0.35_out_rnd`). It is not
//! a solid: the checker finds 10 problems, the first a face with partially coincident
//! edges, and the body is not manifold (measured by
//! `offset_executive_p2::tests::moved_cases_results_measured`, #18927). It is built after
//! a group of overlapping intersection curves was taken out of the intersection and its
//! replacement failed (`CurveClassification::find_interval`: the parameter is before the
//! start), which upstream ignores. The offset is refused there now, with
//! `OverlapGroupNotReplaced` (#18927): the solve on the lent B-rep still runs (the
//! refusal comes after it, not as `Unported`). The solid is the upstream test file, read
//! by the Rust reader when the pinned upstream checkout is present (as the difftest SMS
//! tests do).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::Notifications;
use solidcore::geom::attribute::AttributeStore;
use solidcore::offset::offset_executive::p1::{OffsetExecutive, OffsetOperation};
use solidcore::offset::offset_executive::p2::SolidOffsetOptions;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, Kernel};

/// `Boolean/extrudeHoles1.smb` read by the Rust reader and arena-named, as the difftest
/// reader (`own_reader::read_file`) makes it.
fn extrude_holes(k: &mut Kernel) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Boolean/extrudeHoles1.smb");
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
    let key = solidcore::topology::loopuse::notify_ref;
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
    let Err(e) = result else {
        panic!("the overlap group's failed replacement is refused");
    };
    assert!(!e.is_unported_or_cancelled(), "{e}");
    assert_eq!(e.sm_status(), Some(1001), "{e}");
    let reasons: Vec<String> = std::iter::successors(Some(&e), |e| e.cause())
        .filter_map(|e| e.failure().map(|f| format!("{:?}", f.reason)))
        .collect();
    assert!(
        reasons
            .first()
            .is_some_and(|r| r.contains("OverlapGroupNotReplaced")),
        "{e}: {reasons:?}"
    );
    assert!(
        e.to_string().contains("find_interval"),
        "the failed step is the cause: {e}"
    );
}
