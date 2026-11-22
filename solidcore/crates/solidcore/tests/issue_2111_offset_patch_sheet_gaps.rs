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
//! offset checks clean, its edges and vertices keeping the sheet's own tolerances (WR-671),
//! and the tolerance updates (WR-679, WR-705) have nothing to grow.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)] // test helpers

use solidcore::geom::assert_array::AssertArray;
use solidcore::geom::attribute::AttributeStore;
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
use solidcore::{Brep, EdgeId, Kernel, VertexId};

const SHEET: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/topology.assert_valid/data/issue2111_patch_sheet.smb"
);

/// The accuracy the offsets' areas are integrated at: they are compared to 1e-6 relative,
/// and at 1e-7 each lands within 6e-8 relative of its value at 1e-8 (measured on these
/// offsets).
const AREA_ACCURACY: f64 = 1e-7;

/// The sheet read from its file, copied, offset by `d` and checked at `Level2`.
fn check_of(d: f64) -> (Kernel, Brep, BrepCheck) {
    let (mut k, mut r) = offset(d);
    let check = check_brep(&mut k, &mut r, CheckLevel::Level2).expect("check_brep answers");
    (k, r, check)
}

/// [`check_of`], and the offset's area.
fn offset_checked(d: f64) -> (BrepCheck, f64) {
    let (k, mut r, check) = check_of(d);
    let area = brep_area(&k, &mut r, AREA_ACCURACY).unwrap();
    (check, area)
}

/// The sheet read from its file.
fn sheet() -> (Kernel, Brep) {
    let bytes = std::fs::read(SHEET).unwrap();
    let mut k = Kernel::default();
    let mut read = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = solidcore::topology::loopuse::notify_ref;
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 40,
    };
    read_from_file(&mut k, &mut read, bytes, FileType::Ascii, false, &mut env).unwrap();
    read.refresh_cached_topology();
    (k, read)
}

/// The kernel and the sheet read from its file, copied and offset by `d`.
fn offset(d: f64) -> (Kernel, Brep) {
    let (mut k, read) = sheet();
    let r = offset_sheet(&mut k, &read, d);
    (k, r)
}

/// `read` copied and offset by `d`.
fn offset_sheet(k: &mut Kernel, read: &Brep, d: f64) -> Brep {
    let mut b = brep_copy(k, read).unwrap();
    let mut r = offset_brep_simple(
        k,
        &mut b,
        OffsetOptions {
            distance: d,
            extend_convex_edges: true,
            merge_results: true,
        },
    )
    .unwrap();
    r.refresh_cached_topology();
    r
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
        let (mut k, mut r, c) = check_of(d);
        assert_eq!(c.assert_valid, Some(true), "d = {d}: {:?}", c.lines());
        assert_eq!(c.failures().count(), 0, "d = {d}: {:?}", c.lines());
        let gap = largest_gap(&mut k, &mut r);
        assert!(gap <= 1e-12, "d = {d}: gap {gap}");
    }
}

/// WR-671: the offset edges and vertices keep the sheet's own tolerances (no offset
/// edge or vertex is tighter than the sheet's tightest), and the offsets check clean.
#[test]
fn kept_source_tolerances_check_clean() {
    let (_, s) = sheet();
    let tightest_edge = s
        .edges()
        .ids()
        .map(|e| s.edge(e).unwrap().tolerance)
        .fold(f64::INFINITY, f64::min);
    let tightest_vertex = s
        .vertices()
        .ids()
        .map(|v| s.vertex(v).unwrap().tolerance)
        .fold(f64::INFINITY, f64::min);
    assert!(tightest_edge > 2.08e-5 && tightest_vertex > 2.08e-5);
    for d in [0.5, 0.75, -0.75, 1.0, 1.5, -1.5] {
        let (_, r, c) = check_of(d);
        assert_eq!(c.assert_valid, Some(true), "d = {d}: {:?}", c.lines());
        assert_eq!(c.failures().count(), 0, "d = {d}: {:?}", c.lines());
        for e in r.edges().ids() {
            let t = r.edge(e).unwrap().tolerance;
            assert!(t >= tightest_edge, "d = {d}: edge {e:?} tolerance {t}");
        }
        for v in r.vertices().ids() {
            let t = r.vertex(v).unwrap().tolerance;
            assert!(t >= tightest_vertex, "d = {d}: vertex {v:?} tolerance {t}");
        }
    }
}

/// WR-671, entity by entity: the sheet's vertices and edges loosened each to its own
/// tolerance (vertices 1.0e-4, 1.1e-4, ..; edges 5.0e-5, 5.1e-5, ..), each offset vertex
/// keeps its own source vertex's and each offset edge its own source edge's, exactly.
/// An offset vertex's source is the one sheet vertex about `|d|` from it (within 1e-4, the
/// vertex tolerance scale: the offset surface is a fit, and its corners lie up to ~4e-6 off
/// `V + d n(V)`; every other sheet vertex is asserted more than 1e-3 farther), and an
/// edge's source is the sheet edge between its ends' sources.
#[test]
fn each_offset_entity_keeps_its_own_sources_tolerance() {
    let (mut k, mut s) = sheet();
    let vertices: Vec<VertexId> = s.vertices().ids().collect();
    let edges: Vec<EdgeId> = s.edges().ids().collect();
    assert_eq!((vertices.len(), edges.len()), (4, 4));
    for (i, &v) in vertices.iter().enumerate() {
        s.vertex_mut(v).unwrap().tolerance = 1e-4 + 1e-5 * i as f64;
    }
    for (i, &e) in edges.iter().enumerate() {
        s.edge_mut(e).unwrap().tolerance = 5e-5 + 1e-6 * i as f64;
    }
    let ends_of = |b: &Brep, e: EdgeId| -> Vec<VertexId> {
        let mut ends: Vec<VertexId> = solidcore::topology::edge::vertices(b, e, None)
            .unwrap()
            .into_iter()
            .map(Option::unwrap)
            .collect();
        ends.sort();
        ends
    };
    for d in [0.5, 0.75, -0.75, 1.0, 1.5, -1.5] {
        let r = offset_sheet(&mut k, &s, d);
        let source_of = |w: VertexId| -> VertexId {
            let p = r.vertex(w).unwrap().point;
            let mut by_distance: Vec<(f64, VertexId)> = vertices
                .iter()
                .map(|&v| (s.vertex(v).unwrap().point.distance(p), v))
                .collect();
            by_distance.sort_by(|a, b| a.0.total_cmp(&b.0));
            let (near, v) = by_distance[0];
            assert!(
                (near - d.abs()).abs() <= 1e-4,
                "d = {d}: {w:?} is {near} from {v:?}"
            );
            assert!(
                by_distance[1].0 > d.abs() + 1e-3,
                "d = {d}: {w:?} {by_distance:?}"
            );
            v
        };
        let made: Vec<VertexId> = r.vertices().ids().collect();
        assert_eq!(made.len(), 4, "d = {d}");
        for &w in &made {
            let own = s.vertex(source_of(w)).unwrap().tolerance;
            let t = r.vertex(w).unwrap().tolerance;
            assert_eq!(t, own, "d = {d}: vertex {w:?}");
        }
        let made_edges: Vec<EdgeId> = r.edges().ids().collect();
        assert_eq!(made_edges.len(), 4, "d = {d}");
        for e in made_edges {
            let mut sources: Vec<VertexId> = ends_of(&r, e).into_iter().map(source_of).collect();
            sources.sort();
            let between: Vec<EdgeId> = edges
                .iter()
                .copied()
                .filter(|&x| ends_of(&s, x) == sources)
                .collect();
            assert_eq!(between.len(), 1, "d = {d}: edge {e:?} {sources:?}");
            let own = s.edge(between[0]).unwrap().tolerance;
            let t = r.edge(e).unwrap().tolerance;
            assert_eq!(t, own, "d = {d}: edge {e:?}");
        }
    }
}

/// WR-679 at d = 0.5: `ValidateAndUpdateTolerances(FALSE)` after the WR-671 offset finds no
/// gap to cover and reports nothing.
#[test]
fn updated_tolerances_at_a_half_have_nothing_to_grow() {
    let (mut k, mut r) = offset(0.5);
    let mut reports = AssertArray::new();
    let gaps =
        validate_and_update_tolerances(&mut k, &mut r, false, Some(&mut reports), 0).unwrap();
    assert!(!gaps.violated_or_updated);
    assert!(
        gaps.max_vertex_edge_gap <= 1e-12,
        "{}",
        gaps.max_vertex_edge_gap
    );
    assert!(reports.reports().iter().all(|r| r.ok));
}

/// WR-679 on the sheet itself, as the golden case `update_patch_sheet_m6_i0_v0.0001`
/// (`topology.assert_valid`) builds it: vertex 0 moved 1e-4 along x, so its gaps straddle
/// its tolerance 8.3e-5 (one just under 1e-4, one just over). Upstream grows the vertex
/// past the first gap only, to twice it, and then reports the second (over half the grown
/// tolerance) as bad. `ValidateAndUpdateTolerances(FALSE)` grows it past half its
/// tolerance, so the vertex ends at exactly twice its largest gap and nothing is reported.
#[test]
fn update_grows_a_moved_vertex_to_twice_its_largest_gap() {
    let bytes = std::fs::read(SHEET).unwrap();
    let mut k = Kernel::default();
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
    read_from_file(&mut k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.use_arena_naming().unwrap();
    let v = VertexId::from_index(0);
    b.vertex_mut(v).unwrap().point.x += 1e-4;
    let mut reports = AssertArray::new();
    let gaps =
        validate_and_update_tolerances(&mut k, &mut b, false, Some(&mut reports), 0).unwrap();
    assert!(gaps.violated_or_updated);
    let g = gaps.max_vertex_edge_gap;
    assert!(g > 1e-4 && g < 1.0001e-4, "largest gap {g}");
    let tol = b.vertex(v).unwrap().tolerance;
    assert_eq!(
        tol.to_bits(),
        (2.0 * g).to_bits(),
        "tolerance {tol}, 2g {}",
        2.0 * g
    );
    assert!(reports.reports().is_empty(), "{:?}", reports.reports());
}

/// WR-705 at d = 0.5: with WR-671 and WR-679 the
/// offset's update of its own result moves no geometry and grows nothing: the result
/// checks clean, its gaps at rounding, its area the offset's without the update.
#[test]
fn offset_updates_its_result_tolerances() {
    let (mut k, mut r) = offset(0.5);
    let gap = largest_gap(&mut k, &mut r);
    assert!(gap <= 1e-12, "gap {gap}");
    let c = check_brep(&mut k, &mut r, CheckLevel::Level2).unwrap();
    assert_eq!(c.assert_valid, Some(true), "{:?}", c.lines());
    assert_eq!(c.failures().count(), 0, "{:?}", c.lines());
    let (_, area) = offset_checked(0.5);
    let updated = brep_area(&k, &mut r, AREA_ACCURACY).unwrap();
    assert!((updated - area).abs() <= 1e-6 * area, "{updated} {area}");
}
