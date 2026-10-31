//! #2111: the offset of a trimmed B-spline patch sheet (two lines and two R40 arcs at
//! +-14 on a plain B-spline surface, written by SolidCraft) checked at `Level2`.
//!
//! Before #2054 the check stopped on the `OffsetSurface` face's `Unported`; now it
//! runs and reports what the C++ reports for `SmApiBrepCopy` + `SmApiOffsetBrepFull`
//! (extended, merged) + the capi check (difftest `topology.assert_valid`,
//! `copy_offset_patch_sheet.json`, byte-identical lines): `SmBrep` list-2 Bad Gaps
//! (edge/vertex gap over half the vertex tolerance, `ValidateAndUpdateTolerances`)
//! at d = 0.75, -0.75 and 1.0, none at +-1.5, and at d = 0.5 two edges failing
//! `ValidateGeometry`, so `ValidatePointers` fails and `AssertValid` is skipped.
//! This pins that default (upstream) behaviour; an opt-in tolerance fix is to flip it
//! under its flag.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)] // test helpers

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::offset::offset_executive::p2::{offset_brep_simple, OffsetOptions};
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p3::brep_copy;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep::validation::{check_brep, BrepCheck, CheckLevel};
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, EntityRef, Kernel};

const SHEET: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/topology.assert_valid/data/issue2111_patch_sheet.smb"
);

/// The sheet read from its file, copied, offset by `d` and checked at `Level2`.
fn offset_check(d: f64) -> BrepCheck {
    let bytes = std::fs::read(SHEET).unwrap();
    let mut k = Kernel::default();
    let mut read = Brep::new_with_infinite_region().unwrap();
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
    read_from_file(&mut k, &mut read, bytes, FileType::Ascii, false, &mut env).unwrap();
    read.refresh_cached_topology();
    let mut b = brep_copy(&mut k, &read).unwrap();
    let mut r = offset_brep_simple(
        &mut k,
        &mut b,
        OffsetOptions {
            distance: d,
            extend_convex_edges: true,
            merge_results: true,
        },
    )
    .unwrap();
    r.refresh_cached_topology();
    check_brep(&mut k, &mut r, CheckLevel::Level2).expect("check_brep answers")
}

/// The failure lines that are edge/vertex Bad Gaps of the tolerance pass.
fn bad_gaps(c: &BrepCheck) -> usize {
    c.lines()
        .iter()
        .filter(|l| l.contains("Bad Gap: Edge/Vertex gap across Edgeuse"))
        .count()
}

#[test]
fn offset_reports_the_upstream_bad_gaps() {
    for (d, gaps) in [(0.75, 3), (-0.75, 4), (1.0, 1)] {
        let c = offset_check(d);
        assert_eq!(c.assert_valid, Some(true), "d = {d}: {:?}", c.lines());
        assert_eq!(c.failures().count(), gaps, "d = {d}: {:?}", c.lines());
        assert_eq!(bad_gaps(&c), gaps, "d = {d}: {:?}", c.lines());
    }
    // the issue's first gap: 3.7114381e-05 over half the vertex tolerance 2.0766567e-05
    let lines = offset_check(0.75).lines();
    assert!(
        lines[0].contains("offset=3.7114381e-05, tolerance=2.0766567e-05"),
        "{lines:?}"
    );
}

#[test]
fn offset_is_clean_at_one_and_a_half() {
    for d in [1.5, -1.5] {
        let c = offset_check(d);
        assert_eq!(c.assert_valid, Some(true), "d = {d}: {:?}", c.lines());
        assert_eq!(c.failures().count(), 0, "d = {d}: {:?}", c.lines());
    }
}

#[test]
fn offset_by_a_half_fails_validate_pointers() {
    let c = offset_check(0.5);
    assert_eq!(c.assert_valid, None, "{:?}", c.lines());
    assert_eq!(c.failures().count(), 2, "{:?}", c.lines());
}
