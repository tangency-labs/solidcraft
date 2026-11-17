//! WR-410 (tangency-labs/solidcore#1059): a boolean vertex first placed by one edge crossing
//! is placed again by a later crossing whose misplacement bound along the edge (gap over
//! sine) is smaller, only where a certified bound proves that the edge stretch the vertex
//! moves along stays within the vertex's tolerance of the other object and of the first
//! crossing's curve (`VertexDefinition::place_by_crossing`); otherwise the first placement
//! stays. Retiring `boolean_vertex_by_best_crossing` (tangency-labs/solidcraft#14246) moves
//! two `offset.offset_executive_p2` rows, both rounded solid offsets whose stitch merges the
//! offset faces with booleans:
//!
//! - `extrude_holes1_face0_inset24_7_rounded`: `Boolean/extrudeHoles1.smb` inset by 24.7
//!   keeping face 0 as a shell face (approximation tolerance 0.0059574935488984). Its vertex
//!   bounds move: lo.x, lo.y by about 1e-7, lo.z by 2.8e-13, hi.y by 1.0e-6, hi.z by 9.8e-7.
//! - `shell_file_problemShell10_face1_round`: `Shell/problemShell10.smb` inset by 12.7 keeping
//!   face 1 (approximation tolerance 0.001798855365225). Its hi.z moves by 1.03e-5.
//!
//! The counts stay. A vertex belongs on every face it bounds, and the faces' surfaces are the
//! exact offset surfaces while the edge curves are fits within the approximation tolerance, so
//! the measure is the vertex's largest gap to its faces' surfaces (`surface_closest_point`).
//! For each moved bound, measured with the first crossing (main before the retirement) and
//! with the certified move:
//!
//! | row, bound | vertex's largest face gap, first crossing | certified move |
//! |---|---|---|
//! | extrudeHoles1 lo.x, lo.y | 2.75e-7 | 2.1e-12 |
//! | extrudeHoles1 lo.z | 3.3e-13 | 1.5e-14 |
//! | extrudeHoles1 hi.y | 9.20e-7 | 3.66e-7 (its third face 9.20e-7 -> 1.9e-14) |
//! | extrudeHoles1 hi.z | 7.93e-7 | 7.93e-7 (its largest edge gap 4.70e-6 -> 2.85e-6) |
//! | problemShell10 hi.z | 9.73e-6 (its largest edge gap 1.03e-5, past the 1e-5 tolerance) | 1.9e-14 |
//!
//! The hi.y vertex's gap to one edge grows from 2.43e-6 to 2.62e-6 as it moves onto its
//! third face: it now lies on all three faces' surfaces (1.9e-14), their exact common point,
//! and that edge's curve, a fit of two of those surfaces' intersection, passes 2.62e-6 from
//! it (under the vertex tolerance 1e-5).
//!
//! In problemShell10 the two top vertices, (-50.8, -28.575) and (34.925, -28.575), move onto
//! their faces by certified moves (each edge stretch within 9.73e-6 and 9.36e-6 of the other
//! object and of the first crossing's curve, under the vertex tolerance 1.8e-3): 9.73e-6 ->
//! 1.9e-14 and 9.35e-6 -> 1.6e-14 off their faces, their largest edge gaps 1.03e-5 -> 9.73e-6
//! and 1.02e-5 -> 9.35e-6. The vertex (-50.8, -114.3) would move against a face on an offset
//! surface, which has no certified bound, so it keeps its first placement, 7.49e-7 off a
//! face. The tests below assert the moved gaps under bounds the first-crossing vertices
//! exceed.
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
use solidcore::math::Point3;
use solidcore::offset::offset_executive::p1::{OffsetExecutive, OffsetOperation};
use solidcore::offset::offset_executive::p2::SolidOffsetOptions;
use solidcore::query::api_queries::{edge_closest_point, surface_closest_point};
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::vertex;
use solidcore::{Brep, Kernel, VertexId};

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

/// The rounded inset (`DoSolidOffset`, stitched and merged) of `name` keeping face `face`
/// as a shell face, as the goldens row runs it.
fn rounded_inset(k: &mut Kernel, name: &str, face: usize, distance: f64, tol: f64) -> Option<Brep> {
    let mut part = upstream_part(k, name)?;
    let f = bp1::faces(&part, None).unwrap()[face];
    let mut executive = OffsetExecutive::new(OffsetOperation::SolidOffset, distance, tol);
    executive.set_inset(true);
    executive.set_extend_convex_edges(false);
    executive.set_merge_results(true);
    executive.set_create_offset_solid(false);
    executive.set_shell_faces(&[f]);
    let mut notifications = Notifications::new(&mut part);
    Some(
        executive
            .do_solid_offset(
                k,
                Some(&mut part),
                SolidOffsetOptions {
                    skip_stitch_merge: false,
                },
                &mut notifications.context(),
            )
            .unwrap(),
    )
}

fn coordinate(p: Point3, axis: usize) -> f64 {
    [p.x, p.y, p.z][axis]
}

/// The vertex attaining the body's least (`high` false) or greatest coordinate on `axis`.
fn extreme_vertex(b: &Brep, axis: usize, high: bool) -> VertexId {
    let mut best: Option<(VertexId, f64)> = None;
    for &v in b.vertex_list() {
        let c = coordinate(vertex::point(b, v).unwrap(), axis);
        let better = match best {
            None => true,
            Some((_, b)) => (high && c > b) || (!high && c < b),
        };
        if better {
            best = Some((v, c));
        }
    }
    best.unwrap().0
}

/// The vertex's largest gap to the surfaces of the faces it bounds.
fn face_gap(k: &Kernel, b: &Brep, v: VertexId) -> f64 {
    let p = vertex::point(b, v).unwrap();
    vertex::faces(b, v)
        .unwrap()
        .into_iter()
        .map(|f| {
            let s = b.face(f).unwrap().surface.as_ref().unwrap();
            surface_closest_point(k, s, p).unwrap().closest.distance
        })
        .fold(0.0, f64::max)
}

/// The vertex's largest gap to the curves of the edges it bounds.
fn edge_gap(k: &Kernel, b: &Brep, v: VertexId) -> f64 {
    let p = vertex::point(b, v).unwrap();
    vertex::edges(b, v, None)
        .unwrap()
        .into_iter()
        .map(|e| edge_closest_point(k, b, e, p).unwrap().closest.distance)
        .fold(0.0, f64::max)
}

fn counts(b: &Brep) -> (usize, usize, usize) {
    (
        b.face_list().len(),
        b.edge_list().len(),
        b.vertex_list().len(),
    )
}

#[test]
fn wr_410_extrude_holes_inset_vertices_meet_their_faces() {
    let mut k = Kernel::default();
    let Some(b) = rounded_inset(
        &mut k,
        "Boolean/extrudeHoles1.smb",
        0,
        24.7,
        0.005_957_493_548_898_4,
    ) else {
        return;
    };
    assert_eq!(counts(&b), (23, 45, 25), "faces, edges, vertices");
    // lo.x and lo.y: one vertex; first crossing 2.75e-7 off a face
    let v = extreme_vertex(&b, 0, false);
    assert_eq!(
        v,
        extreme_vertex(&b, 1, false),
        "lo.x and lo.y share a vertex"
    );
    let gap = face_gap(&k, &b, v);
    assert!(gap <= 1e-11, "lo.x/lo.y vertex {gap} off its faces");
    // lo.z: first crossing 3.3e-13
    let gap = face_gap(&k, &b, extreme_vertex(&b, 2, false));
    assert!(gap <= 1e-13, "lo.z vertex {gap} off its faces");
    // hi.y: first crossing 9.20e-7
    let gap = face_gap(&k, &b, extreme_vertex(&b, 1, true));
    assert!(gap <= 5e-7, "hi.y vertex {gap} off its faces");
    // hi.z: first crossing 7.93e-7 off its faces (unchanged) and 4.70e-6 off an edge
    let v = extreme_vertex(&b, 2, true);
    let gap = face_gap(&k, &b, v);
    assert!(gap <= 8e-7, "hi.z vertex {gap} off its faces");
    let gap = edge_gap(&k, &b, v);
    assert!(gap <= 3e-6, "hi.z vertex {gap} off its edges");
}

#[test]
fn wr_410_problem_shell_inset_vertices_meet_their_faces() {
    let mut k = Kernel::default();
    let Some(b) = rounded_inset(
        &mut k,
        "Shell/problemShell10.smb",
        1,
        12.7,
        0.001_798_855_365_225,
    ) else {
        return;
    };
    assert_eq!(counts(&b), (6, 12, 8), "faces, edges, vertices");
    // the top vertex (first crossing 9.73e-6 off a face, 1.03e-5 off an edge)
    let v = extreme_vertex(&b, 2, true);
    let p = vertex::point(&b, v).unwrap();
    assert!(
        (p.x + 50.8).abs() < 1e-9 && (p.y + 28.575).abs() < 1e-9,
        "top vertex {p:?}"
    );
    let gap = face_gap(&k, &b, v);
    assert!(gap <= 1e-12, "top vertex {gap} off its faces");
    let gap = edge_gap(&k, &b, v);
    assert!(gap < 1e-5, "top vertex {gap} off its edges");
    // every vertex lies within its tolerance of its faces; the one at (-50.8, -114.3)
    // keeps its first placement, 7.49e-7 off a face (its move is not certified)
    for &v in b.vertex_list() {
        let gap = face_gap(&k, &b, v);
        assert!(gap <= 1e-6, "vertex {v:?} {gap} off its faces");
    }
}
