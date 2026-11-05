//! An extended solid offset of `Defeature/extrusion.smb` keeping face 0 as a shell face,
//! inset by 2 (the `offset.offset_executive_p2` case `defeature_extrusion_face0_inset2_extended`).
//!
//! Inset by 2, the part's convex round of radius 2 (face 5's corner at (2, 8)) offsets to
//! nothing: its offset surface collapses to a line. The edges `MapEdgeuses` lifts across
//! it are 1.69 off the surface, `CreateUVTrimCurve`'s 101-point drop raises their
//! tolerance to that, and every later trim curve built at the raised tolerance raises it
//! again (Rust: 3.38, 6.77, 5.05, 10, then the closing stitch 11.46 and 20). No body here is
//! sound:
//! - the C++ reference (11 faces, 34 edges, 14 vertices) carries edge tolerances 16.06 and
//!   6.77 on a B-rep of tolerance 1e-4, and edges whose ends lie about 3 from any vertex
//!   (its least vertex x of 4 comes from gluing the x = 2 wall's vertices away at those
//!   tolerances). The port on the fitted extension matched it only because that
//!   extension's knot removal moved the round off radius 2, so its offset did not
//!   collapse;
//! - on the exact extension (the round kept at radius 2) the port gives 11 faces, 32 edges,
//!   13 vertices, edge tolerances 11.46 and 20, and Level2 reports an edge/edge gap 4.14.
//!
//! A round whose radius is the offset distance needs the offset built as one construction
//! (#2350: exact offsets and mitres), not lifted edges and grown tolerances. Until then the
//! test is ignored; it is to assert a sound body (`check_brep` clean at level 2, the inner
//! walls exact: x = 2, y = 8 and the corner at (2, 8)).
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
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, EntityRef, Kernel};

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

#[test]
#[ignore = "the round of radius 2 collapses at inset 2: neither the C++ nor the port builds a sound body (#2350)"]
fn inset_extended_offset_of_a_collapsing_round_is_sound() {
    let mut k = Kernel::default();
    let Some(mut part) = extrusion(&mut k) else {
        return;
    };
    let face0 = bp1::faces(&part, None).unwrap()[0];
    let mut executive = OffsetExecutive::new(OffsetOperation::SolidOffset, 2.0, 1e-4);
    executive.set_inset(true);
    executive.set_extend_convex_edges(true);
    executive.set_merge_results(false);
    executive.set_create_offset_solid(false);
    executive.set_shell_faces(&[face0]);
    let mut notifications = Notifications::new(&mut part);
    let result = executive
        .do_solid_offset(
            &mut k,
            Some(&mut part),
            SolidOffsetOptions::default(),
            &mut notifications.context(),
        )
        .unwrap();

    // a sound body: check_brep clean at level 2
    let mut result = result;
    result.use_arena_naming().unwrap();
    let lines = check_brep(&mut k, &mut result, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    // the x = 0 wall inset by 2 is the x = 2 wall
    let min_x = bp1::vertices(&result, None)
        .unwrap()
        .iter()
        .map(|&v| result.vertex(v).unwrap().point.x)
        .fold(f64::INFINITY, f64::min);
    assert!((min_x - 2.0).abs() < 1e-9, "least vertex x {min_x}");
}
