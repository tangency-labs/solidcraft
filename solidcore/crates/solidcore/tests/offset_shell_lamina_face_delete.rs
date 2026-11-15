//! A rounded solid offset deletes an offset face whose lamina edge lies closer to the
//! original surface than the offset distance (`BuildOffsetOfFaceuse`, cpp:3730-3759).
//!
//! `Boolean/extrudeHoles1.smb` outset by 24.7 keeping face 0 as a shell face (the
//! `offset.offset_executive_p2` case `pi_extrudeHoles1_f0_k0.38_out_rnd`): one offset face
//! has a lamina edge point nearer the original surface than 24.7 less twice the
//! approximation tolerance, so upstream deletes it with
//! `SmTopologyIntersector::DeleteFaces`. The port ran that deletion through the kernel
//! context while the face builder held its notification callbacks, so the deletion
//! failed as a nested edit, its status was ignored and the face was kept: the
//! unstitched offset had one face and four edges more than the C++ reference.
//!
//! The expected counts are the pinned C++ reference's. `Defeature/extrusion.smb` inset by
//! 2.5 deleted its face 2 the same way; that face's offset folds (radius 1.53), so the
//! offset is now refused before it is built (`offset_shell_extrusion_extend.rs`).
//!
//! At 24.7 the outset's tube about edge 4 folds over itself (#13685), so this outset is now
//! refused too, naming the largest distance at which edge 4's tube is proved not to fold.
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
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, EntityRef, Kernel, SolidError};

/// The upstream test file, read as the difftest runner reads it; `None` (the test is
/// skipped) without the pinned upstream checkout.
fn upstream_part(k: &mut Kernel, name: &str) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(name);
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

/// The unstitched rounded offset (`DoSolidOffset` with `bSkipStitchMerge`) of `name`
/// keeping face 0 as a shell face (`None` without the upstream checkout).
fn unstitched(
    name: &str,
    distance: f64,
    tol: f64,
    inset: bool,
) -> Option<Result<Brep, SolidError>> {
    let mut k = Kernel::default();
    let mut part = upstream_part(&mut k, name)?;
    let face0 = bp1::faces(&part, None).unwrap()[0];
    let mut executive = OffsetExecutive::new(OffsetOperation::SolidOffset, distance, tol);
    executive.set_inset(inset);
    executive.set_extend_convex_edges(false);
    executive.set_merge_results(true);
    executive.set_create_offset_solid(false);
    executive.set_shell_faces(&[face0]);
    let mut notifications = Notifications::new(&mut part);
    Some(executive.do_solid_offset(
        &mut k,
        Some(&mut part),
        SolidOffsetOptions {
            skip_stitch_merge: true,
        },
        &mut notifications.context(),
    ))
}

const TOL: f64 = 0.005_957_493_548_898_4;

/// At 24.7 the outset's tube about edge 4 folds over itself (#13685): the unstitched body
/// the C++ reference counts (24 faces, 84 edges) fails `check_brep` level 2, its first line
/// a control net that doubles back on itself; the offset is refused, naming the largest
/// distance at which edge 4's tube is proved not to fold. Returns it.
fn edge_4s_largest() -> Option<f64> {
    let e = unstitched("Boolean/extrudeHoles1.smb", 24.7, TOL, false)?.unwrap_err();
    let Some(&Reason::Offset(OffsetFailure::TubeFolds {
        edge,
        distance,
        largest,
    })) = e.failure().map(|f| &f.reason)
    else {
        panic!("{e}");
    };
    assert_eq!((edge.index(), distance), (4, 24.7), "{e}");
    let largest = largest.expect("a distance proved not to fold");
    assert!(largest < 24.7, "largest {largest}");
    Some(largest)
}

#[test]
fn extrude_holes_outset_past_edge_4s_tube_is_refused_naming_it() {
    let _ = edge_4s_largest();
}

/// Gate data (#13685, `--ignored --nocapture`): the unstitched outset at edge 4's named
/// largest, its counts, to choose the distance the face deletion is kept at.
#[test]
#[ignore = "data for #13685, not a check"]
#[allow(clippy::print_stderr)]
fn data_the_unstitched_outset_at_edge_4s_largest() {
    let Some(largest) = edge_4s_largest() else {
        return;
    };
    for f in [1.0, 0.99, 0.9, 0.75, 0.5] {
        let d = largest * f;
        match unstitched("Boolean/extrudeHoles1.smb", d, TOL, false).unwrap() {
            Ok(r) => eprintln!(
                "TUBEDATA lamina largest {largest} d {d} ok faces {} edges {}",
                bp1::faces(&r, None).unwrap().len(),
                bp1::edges(&r, None).unwrap().len()
            ),
            Err(e) => eprintln!("TUBEDATA lamina largest {largest} d {d} err {e}"),
        }
    }
}
