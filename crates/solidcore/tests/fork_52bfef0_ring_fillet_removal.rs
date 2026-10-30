//! WR-605 (the fork's tangency-labs/smlib 52bfef0): `SmBrep::DeleteFilletFace` takes a
//! four-sided fillet only (`SmFilletRemoval::CollectFilletTopology`), so a round all the
//! way round a boss's foot, whose rails are closed edges with one vertex each and no
//! ends, cannot be removed. With `UpstreamFixes::fillet_removal_takes_ring_fillets` the
//! fork's `SmRingFilletRemoval` removes it: the sharp edge is the intersection of the
//! plate's top and the boss's wall (the circle r 10), and the body is the plate and the
//! boss again, 24000 + 1000 pi = 27141.592653590, eight faces, check clean. The fork's
//! probe gave the same. With `glue_vertices_joins_every_region` (WR-607, 661f79c) too,
//! the shells are joined by the glue instead of `JoinShellsAtVertex`, with the same
//! result.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::volume;
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_removal::delete_fillet_face;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::surface::plane::Plane;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p2 as face2;
use solidcore::topology::{edge, vertex, Brep, FaceId};
use solidcore::Kernel;

/// 24000 + 1000 pi: the 60 x 40 x 10 plate and the boss r 10, 10 high.
const PLATE_AND_BOSS: f64 = 24000.0 + 1000.0 * std::f64::consts::PI;

/// The plate with the boss on it at (30, 20), the boss's foot rounded r 2.
fn boss_with_foot_round(k: &mut Kernel) -> Brep {
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0).unwrap();
    let boss = create_cylinder(k, Vec3::new(30.0, 20.0, 10.0), 10.0, 10.0).unwrap();
    let mut b = boolean_union(k, plate, boss).unwrap();
    // the foot: the closed edge at z 10
    let foot = b
        .edges()
        .ids()
        .find(|&e| {
            let v = edge::start_vertex(&b, e).unwrap().unwrap();
            edge::is_closed(&b, e).unwrap() && (vertex::point(&b, v).unwrap().z - 10.0).abs() < 1e-9
        })
        .unwrap();
    fillet_edges(k, &mut b, &[foot], 2.0, 1, 1, 1.0).unwrap();
    b
}

/// The round: the face that is neither a plane nor on the boss's wall's cylinder
/// (radius 10 at its points at z 15).
fn round(b: &Brep) -> FaceId {
    let curved: Vec<FaceId> = b
        .face_list()
        .iter()
        .copied()
        .filter(|&f| {
            !face2::surface(b, f)
                .unwrap()
                .unwrap()
                .is_kind_of(Plane::TYPE)
                .unwrap()
        })
        .collect();
    // the wall reaches z 20, the round stays below 12
    *curved
        .iter()
        .find(|&&f| vertex_heights(b, f).iter().all(|&z| z < 12.0 + 1e-9))
        .unwrap()
}

/// The heights of the vertices of `f`'s edges.
fn vertex_heights(b: &Brep, f: FaceId) -> Vec<f64> {
    let edges = solidcore::topology::face::p4::edges(b, f, None, false, None).unwrap();
    let mut out = Vec::new();
    for e in edges {
        let (s, t) = edge::start_end_vertices(b, e).unwrap();
        for v in [s, t].into_iter().flatten() {
            out.push(vertex::point(b, v).unwrap().z);
        }
    }
    out
}

/// `DeleteFilletFace` of the boss's foot round, with the fixes given.
fn remove_round(ring: bool, every_region: bool) -> (Kernel, Brep, solidcore::Result<()>, usize) {
    let mut k = Kernel::default();
    k.fixes.fillet_removal_takes_ring_fillets = ring;
    k.fixes.glue_vertices_joins_every_region = every_region;
    let mut b = boss_with_foot_round(&mut k);
    let faces_before = b.face_list().len();
    let f = round(&b);
    b.set_editing_enabled(true);
    let r = delete_fillet_face(&mut k, &mut b, f, None);
    (k, b, r, faces_before)
}

/// The removal succeeded and gave the plate and the boss back.
fn assert_plate_and_boss(k: &mut Kernel, b: &mut Brep) {
    assert_eq!(b.face_list().len(), 8);
    assert!(brep_is_manifold_solid(b).unwrap());
    let v = volume(k, b).unwrap();
    assert!(
        (v - PLATE_AND_BOSS).abs() <= 1e-6 * PLATE_AND_BOSS,
        "volume {v}, expected {PLATE_AND_BOSS}"
    );
    let lines = check_brep(k, b, CheckLevel::Level2).unwrap().lines();
    assert!(lines.is_empty(), "{lines:?}");
}

#[test]
fn ring_fillet_removal_is_off_by_default() {
    let k = Kernel::default();
    assert!(!k.fixes.fillet_removal_takes_ring_fillets);
}

#[test]
fn upstream_refuses_a_ring_fillet() {
    let (_, b, r, faces_before) = remove_round(false, false);
    assert!(r.is_err(), "upstream removes four-sided fillets only");
    assert_eq!(b.face_list().len(), faces_before, "left as it was");
}

#[test]
fn ring_fillet_is_removed_to_the_sharp_edge() {
    let (mut k, mut b, r, _) = remove_round(true, false);
    r.unwrap();
    assert_plate_and_boss(&mut k, &mut b);
}

#[test]
fn ring_fillet_is_removed_with_every_region_glue() {
    let (mut k, mut b, r, _) = remove_round(true, true);
    r.unwrap();
    assert_plate_and_boss(&mut k, &mut b);
}
