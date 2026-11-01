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
//! This pins that default (upstream) behaviour.
//!
//! The offset builds its faces' edges and vertices afresh at the B-rep's zone tolerance
//! (2.08e-5), not the sheet's own edge (4.15e-5) and vertex (8.31e-5) tolerances, so the
//! gaps (inside the sheet's own zone) fail against half the B-rep's. With the opt-in
//! `offset_edges_keep_source_tolerance` (WR-671) the offset edges and vertices keep the
//! original's, and the offsets by +-0.75, 1.0 and +-1.5 check clean with the area of the
//! default's. At d = 0.5 the offset's vertex/edge gaps (6.0e-5 to 6.8e-5) exceed even the
//! sheet's own tolerances: the flag clears `ValidateGeometry` there but the gaps remain.
//! `ValidateAndUpdateTolerances(FALSE)` after the offset covers them only with the opt-in
//! `validate_tolerances_fixes_reported_gaps` (WR-679): upstream grows a vertex for a gap
//! over its whole tolerance, so gaps between half of it and all of it stay reported.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)] // test helpers

use solidcore::geom::assert_array::AssertArray;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::mass::integrator::properties::brep_area;
use solidcore::offset::offset_executive::p2::{offset_brep_simple, OffsetOptions};
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p3::brep_copy;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep::validation::{
    check_brep, validate_and_update_tolerances, BrepCheck, CheckLevel,
};
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, EntityRef, Kernel};

const SHEET: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/topology.assert_valid/data/issue2111_patch_sheet.smb"
);

/// The sheet read from its file, copied, offset by `d` and checked at `Level2`.
fn offset_check(d: f64) -> BrepCheck {
    offset_checked(d, false).0
}

/// The sheet read from its file, copied, offset by `d` (with
/// `offset_edges_keep_source_tolerance` set to `keep`), checked at `Level2`, and the
/// offset's area.
fn offset_checked(d: f64, keep: bool) -> (BrepCheck, f64) {
    let (mut k, mut r) = offset(d, keep);
    let check = check_brep(&mut k, &mut r, CheckLevel::Level2).expect("check_brep answers");
    let area = brep_area(&k, &mut r, 1e-8).unwrap();
    (check, area)
}

/// The kernel and the sheet read from its file, copied and offset by `d` (with
/// `offset_edges_keep_source_tolerance` set to `keep`).
fn offset(d: f64, keep: bool) -> (Kernel, Brep) {
    let bytes = std::fs::read(SHEET).unwrap();
    let mut k = Kernel::default();
    k.fixes.offset_edges_keep_source_tolerance = keep;
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
    (k, r)
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

/// WR-671: with the offset edges and vertices keeping the sheet's own tolerances, the
/// offsets check clean (the default fails at +-0.75 and 1.0, above), and the tolerance
/// carry-over moves the offset by no more than a gap: its area is the default's.
#[test]
fn kept_source_tolerances_check_clean() {
    for d in [0.75, -0.75, 1.0, 1.5, -1.5] {
        let (c, area) = offset_checked(d, true);
        assert_eq!(c.assert_valid, Some(true), "d = {d}: {:?}", c.lines());
        assert_eq!(c.failures().count(), 0, "d = {d}: {:?}", c.lines());
        let (_, default_area) = offset_checked(d, false);
        println!("d = {d}: area {area} default {default_area}");
        assert!(
            (area - default_area).abs() <= 1e-6 * default_area,
            "d = {d}: area {area}, default {default_area}"
        );
    }
}

/// WR-671 at d = 0.5: the kept tolerances clear the two `ValidateGeometry` failures
/// (so `AssertValid` runs), but the offset's own vertex/edge gaps there, 6.0e-5 to
/// 6.8e-5, exceed half the sheet's vertex tolerance (4.15e-5) and are still reported.
#[test]
fn kept_source_tolerances_at_a_half_leave_the_larger_gaps() {
    let (c, _) = offset_checked(0.5, true);
    assert_eq!(c.assert_valid, Some(true), "{:?}", c.lines());
    assert_eq!(bad_gaps(&c), 4, "{:?}", c.lines());
    assert_eq!(c.failures().count(), 5, "{:?}", c.lines());
}

/// WR-679 at d = 0.5: after the WR-671 offset, `ValidateAndUpdateTolerances(FALSE)` grows
/// each vertex with a bad gap to twice that gap. Upstream grows a vertex only for a gap
/// over its whole tolerance, so these gaps (6.0e-5 to 6.8e-5, between half the vertex
/// tolerance 8.3e-5 and all of it) stay and are reported; with
/// `validate_tolerances_fixes_reported_gaps` the update covers them and the check is clean:
/// every vertex/edge and vertex/face gap is within half its vertex's tolerance and every
/// edge/face gap within its edge's.
#[test]
fn updated_tolerances_at_a_half_cover_the_gaps() {
    for fix in [false, true] {
        let (mut k, mut r) = offset(0.5, true);
        k.fixes.validate_tolerances_fixes_reported_gaps = fix;
        let mut reports = AssertArray::new();
        let gaps =
            validate_and_update_tolerances(&mut k, &mut r, false, Some(&mut reports), 0).unwrap();
        assert!(gaps.violated_or_updated);
        assert!(
            (6.0e-5..6.9e-5).contains(&gaps.max_vertex_edge_gap),
            "{}",
            gaps.max_vertex_edge_gap
        );
        let failed: Vec<_> = reports.reports().iter().filter(|r| !r.ok).collect();
        let c = check_brep(&mut k, &mut r, CheckLevel::Level2).unwrap();
        assert_eq!(c.assert_valid, Some(true), "{:?}", c.lines());
        if fix {
            assert!(failed.is_empty(), "{failed:?}");
            assert_eq!(bad_gaps(&c), 0, "{:?}", c.lines());
            // closed form: the largest vertex gap is held by a vertex of twice its size
            let vu = gaps.max_vertex_edge_use.unwrap();
            let v = r.vertexuse(vu).unwrap().vertex.unwrap();
            assert!(r.vertex(v).unwrap().tolerance >= 2.0 * gaps.max_vertex_edge_gap);
        } else {
            // upstream: the update's own report and the check keep the gaps
            assert_eq!(failed.len(), 4, "{failed:?}");
            assert_eq!(bad_gaps(&c), 4, "{:?}", c.lines());
        }
        println!("fix {fix}: {:?}", c.lines());
    }
}
