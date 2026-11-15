//! #2111: the offset of a trimmed B-spline patch sheet (two lines and two R40 arcs at
//! +-14 on a plain B-spline surface, written by SolidCraft) checked at `Level2`.
//!
//! Upstream (the pinned C++, difftest `topology.assert_valid`, `copy_offset_patch_sheet.json`)
//! reports `SmBrep` list-2 Bad Gaps (edge/vertex gap over half the vertex tolerance) at d =
//! 0.75, -0.75 and 1.0, and at d = 0.5 two edges failing `ValidateGeometry`. Their cause
//! was the lamina corner: `TrimExtendedEdge` ended each free edge's offset where the two
//! edges' UV trim curves cross and read that UV parameter on the lifted 3D curve, which
//! does not keep it, so the line edges' offsets stopped up to 8.5e-5 short of the arcs'
//! (WR-736, #2611). The offset now ends each lamina edge where it passes nearest the
//! vertex's image `V + d n(V)` (the only path, #2769): the offset face is the exact offset
//! surface, every edge curve lies on it, and every vertex/edge gap is at rounding, so every
//! offset checks clean at the B-rep's own zone tolerance, with or without the tolerance
//! opt-ins (WR-671, WR-679, WR-705), which then have nothing to grow.

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

/// The accuracy the offsets' areas are integrated at: they are compared to 1e-6 relative,
/// and at 1e-7 each lands within 6e-8 relative of its value at 1e-8 (measured on these
/// offsets).
const AREA_ACCURACY: f64 = 1e-7;

/// The sheet read from its file, copied, offset by `d` (with
/// `offset_edges_keep_source_tolerance` set to `keep`) and checked at `Level2`.
fn check_of(d: f64, keep: bool) -> (Kernel, Brep, BrepCheck) {
    let (mut k, mut r) = offset(d, keep);
    let check = check_brep(&mut k, &mut r, CheckLevel::Level2).expect("check_brep answers");
    (k, r, check)
}

/// [`check_of`], and the offset's area.
fn offset_checked(d: f64, keep: bool) -> (BrepCheck, f64) {
    let (k, mut r, check) = check_of(d, keep);
    let area = brep_area(&k, &mut r, AREA_ACCURACY).unwrap();
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

/// The largest vertex/edge gap of `r` (`ValidateAndUpdateTolerances`, validate only).
fn largest_gap(k: &mut Kernel, r: &mut Brep) -> f64 {
    validate_and_update_tolerances(k, r, true, None, 0)
        .unwrap()
        .max_vertex_edge_gap
}

/// Every offset checks clean at the defaults, its vertex/edge gaps at rounding.
#[test]
fn offset_checks_clean_at_every_distance() {
    for d in [0.5, 0.75, -0.75, 1.0, 1.5, -1.5] {
        let (mut k, mut r, c) = check_of(d, false);
        assert_eq!(c.assert_valid, Some(true), "d = {d}: {:?}", c.lines());
        assert_eq!(c.failures().count(), 0, "d = {d}: {:?}", c.lines());
        let gap = largest_gap(&mut k, &mut r);
        assert!(gap <= 1e-12, "d = {d}: gap {gap}");
    }
}

/// WR-671: with the offset edges and vertices keeping the sheet's own tolerances the
/// offsets check clean too, and the tolerance carry-over moves no geometry: the area is
/// the default's.
#[test]
fn kept_source_tolerances_check_clean() {
    for d in [0.5, 0.75, -0.75, 1.0, 1.5, -1.5] {
        let (c, area) = offset_checked(d, true);
        assert_eq!(c.assert_valid, Some(true), "d = {d}: {:?}", c.lines());
        assert_eq!(c.failures().count(), 0, "d = {d}: {:?}", c.lines());
        let (_, default_area) = offset_checked(d, false);
        assert!(
            (area - default_area).abs() <= 1e-6 * default_area,
            "d = {d}: area {area}, default {default_area}"
        );
    }
}

/// WR-679 at d = 0.5: `ValidateAndUpdateTolerances(FALSE)` after the WR-671 offset finds no
/// gap to cover, with or without the fix, and reports nothing.
#[test]
fn updated_tolerances_at_a_half_have_nothing_to_grow() {
    for fix in [false, true] {
        let (mut k, mut r) = offset(0.5, true);
        k.fixes.validate_tolerances_fixes_reported_gaps = fix;
        let mut reports = AssertArray::new();
        let gaps =
            validate_and_update_tolerances(&mut k, &mut r, false, Some(&mut reports), 0).unwrap();
        assert!(!gaps.violated_or_updated, "fix {fix}");
        assert!(
            gaps.max_vertex_edge_gap <= 1e-12,
            "{}",
            gaps.max_vertex_edge_gap
        );
        assert!(reports.reports().iter().all(|r| r.ok), "fix {fix}");
    }
}

/// The sheet read from its file, copied and offset by `d` with the given flags set
/// before the offset.
fn offset_with(d: f64, flags: &[&str]) -> (Kernel, Brep) {
    let bytes = std::fs::read(SHEET).unwrap();
    let mut k = Kernel::default();
    for name in flags {
        *k.fixes.flag_mut(name).unwrap() = true;
    }
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

/// WR-705 at d = 0.5: with WR-671 and WR-679 the
/// offset's update of its own result moves no geometry and grows nothing: the result
/// checks clean, its gaps at rounding, its area the offset's without the update.
#[test]
fn offset_updates_its_result_tolerances() {
    let flags = [
        "offset_edges_keep_source_tolerance",
        "validate_tolerances_fixes_reported_gaps",
    ];
    let (mut k, mut r) = offset_with(0.5, &flags);
    let gap = largest_gap(&mut k, &mut r);
    assert!(gap <= 1e-12, "gap {gap}");
    let c = check_brep(&mut k, &mut r, CheckLevel::Level2).unwrap();
    assert_eq!(c.assert_valid, Some(true), "{:?}", c.lines());
    assert_eq!(c.failures().count(), 0, "{:?}", c.lines());
    let (_, area) = offset_checked(0.5, true);
    let updated = brep_area(&k, &mut r, AREA_ACCURACY).unwrap();
    assert!((updated - area).abs() <= 1e-6 * area, "{updated} {area}");
}
