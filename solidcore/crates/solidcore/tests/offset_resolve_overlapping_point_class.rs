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
//!
//! At 22.75 the outset's tube about edge 4 folds over itself (#13685): the body counted
//! there fails `check_brep` level 2 and is no manifold solid, and the offset is now refused,
//! naming the largest distance at which edge 4's tube is proved not to fold.
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
use solidcore::error::Reason;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::offset::offset_executive::p1::{OffsetExecutive, OffsetOperation};
use solidcore::offset::offset_executive::p2::SolidOffsetOptions;
use solidcore::topology::brep::p1 as brep_p1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, EntityRef, Kernel, SolidError};

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

/// The rounded, merged outset of the part by `distance`, its face 0 kept as the shell face.
fn outset(k: &mut Kernel, b: &mut Brep, distance: f64) -> Result<Brep, SolidError> {
    let face = b.face_list()[0];
    let mut executive = OffsetExecutive::new(
        OffsetOperation::SolidOffset,
        distance,
        0.005_957_493_548_898_4,
    );
    executive.set_inset(false);
    executive.set_extend_convex_edges(false);
    executive.set_merge_results(true);
    executive.set_shell_faces(&[face]);
    let mut notifications = Notifications::new(b);
    executive.do_solid_offset(
        k,
        Some(b),
        SolidOffsetOptions::default(),
        &mut notifications.context(),
    )
}

/// The radius edge 4's tube is refused at, by 22.75: that body (the C++ reference's 47 faces,
/// 100 edges, 53 vertices) fails `check_brep` level 2 and is no manifold solid (#13685).
fn edge_4s_largest(k: &mut Kernel, b: &mut Brep) -> f64 {
    let e = outset(k, b, 22.75).unwrap_err();
    let Some(&Reason::Offset(OffsetFailure::TubeFolds {
        edge,
        distance,
        largest,
    })) = e.failure().map(|f| &f.reason)
    else {
        panic!("{e}");
    };
    assert_eq!((edge.index(), distance), (4, 22.75), "{e}");
    let largest = largest.expect("a distance proved not to fold");
    assert!(largest < 22.75, "largest {largest}");
    largest
}

#[test]
fn the_outset_past_edge_4s_tube_is_refused_naming_it() {
    let mut k = Kernel::default();
    let Some(mut b) = extrude_holes(&mut k) else {
        return;
    };
    edge_4s_largest(&mut k, &mut b);
}

/// Gate data (#13685, `--ignored --nocapture`): the outset at edge 4's named largest, its
/// counts and soundness, to choose the distance the point-class mechanic is kept at.
#[test]
#[ignore = "data for #13685, not a check"]
#[allow(clippy::print_stderr)]
fn data_the_outset_at_edge_4s_largest() {
    let mut k = Kernel::default();
    let Some(mut b) = extrude_holes(&mut k) else {
        return;
    };
    let largest = edge_4s_largest(&mut k, &mut b);
    for f in [1.0, 0.99, 0.9, 0.75, 0.5] {
        let d = largest * f;
        match outset(&mut k, &mut b, d) {
            Ok(mut r) => {
                let counts = (
                    brep_p1::faces(&r, None).unwrap().len(),
                    brep_p1::edges(&r, None).unwrap().len(),
                    brep_p1::vertices(&r, None).unwrap().len(),
                );
                let s = solidcore::topology::brep::validation::soundness(
                    &mut Kernel::default(),
                    &mut r,
                )
                .unwrap();
                eprintln!(
                    "TUBEDATA point_class largest {largest} d {d} ok counts {counts:?} sound {} lines {} manifold {}",
                    s.sound(false),
                    s.check.lines().len(),
                    s.manifold
                );
                for line in s.check.lines() {
                    eprintln!("TUBEDATA point_class d {d} check: {line}");
                }
            }
            Err(e) => eprintln!("TUBEDATA point_class largest {largest} d {d} err {e}"),
        }
    }
}
