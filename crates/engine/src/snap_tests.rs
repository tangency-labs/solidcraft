//! Snap points (#5881): the centres, middles and ends edges and faces offer. Every expected
//! point is worked out by hand from the part's dimensions.

use solidcraft_geom::prelude::*;
use std::f64::consts::{FRAC_1_SQRT_2, PI};

use serde_json::{Value, json};
use solidcore_record::Vec3;

use crate::snap::{SnapKind, SnapPoint, SnapSource, snap_points, snap_points_from};
use crate::{Sel, Session};

fn run(s: &mut Session, id: &str, p: Value) {
    if let Err(e) = s.execute(id, &p) {
        panic!("{id} {p}: {e}");
    }
}

/// A 60 × 40 × 10 plate with a Ø6 hole at (15, 20) and a Ø8 hole at (45, 20).
fn plate() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 60, "width": 40, "height": 10}));
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 12, "base": [15, 20, -1], "operation": "cut"}));
    run(&mut s, "solid.cylinder", json!({"radius": 4, "height": 12, "base": [45, 20, -1], "operation": "cut"}));
    s
}

fn sel(v: Value) -> Sel {
    serde_json::from_value(v).unwrap()
}

/// The first mesh edge all of whose points satisfy `on`, as a pick.
fn edge(s: &Session, on: impl Fn(Vec3) -> bool) -> Sel {
    let st = s.model.state();
    let b = st.bodies.first().unwrap();
    let m = b.mesh();
    let (i, e) = m.edges.iter().enumerate().find(|(_, e)| e.iter().all(|p| on(*p))).unwrap();
    sel(json!({"type": "edge", "body": b.name, "index": i, "point": e[e.len() / 2]}))
}

/// The face all of whose triangle corners satisfy `on`, as a pick.
fn face(s: &Session, on: impl Fn(Vec3) -> bool) -> Sel {
    let st = s.model.state();
    let b = st.bodies.first().unwrap();
    let m = b.mesh();
    let n = m.tri_face.iter().map(|f| *f as usize + 1).max().unwrap();
    let tris = |f: usize| -> Vec<[Vec3; 3]> {
        m.triangles.iter().zip(&m.tri_face).filter(|(_, tf)| **tf as usize == f).filter_map(|(t, _)| m.tri(t)).collect()
    };
    let f = (0..n).find(|&f| !tris(f).is_empty() && tris(f).iter().all(|t| t.iter().all(|p| on(*p)))).unwrap();
    sel(json!({"type": "face", "body": b.name, "index": f, "point": tris(f)[0][0]}))
}

/// The points of one kind, in the order given.
fn of(points: &[SnapPoint], kind: SnapKind) -> Vec<Vec3> {
    points.iter().filter(|p| p.kind == kind).map(|p| p.at).collect()
}

/// Whether `got` holds exactly the `want` points (any order), each within `tol`.
fn same(got: &[Vec3], want: &[Vec3], tol: f64) -> bool {
    got.len() == want.len() && want.iter().all(|w| got.iter().any(|g| g.dist(*w) <= tol))
}

#[test]
fn a_hole_offers_the_centres_of_its_ends_and_its_rim_its_centre() {
    let s = plate();
    let wall = face(&s, |p| ((p - Vec3::new(15.0, 20.0, p.z)).len() - 3.0).abs() < 1e-3);
    let got = snap_points(&s, &wall);
    assert!(same(&of(&got, SnapKind::Centre), &[Vec3::new(15.0, 20.0, 0.0), Vec3::new(15.0, 20.0, 10.0)], 1e-6), "{got:?}");
    assert_eq!(got.len(), 2, "only the two centres: {got:?}");

    let rim = edge(&s, |p| (p.z - 10.0).abs() < 1e-9 && ((p - Vec3::new(45.0, 20.0, 10.0)).len() - 4.0).abs() < 1e-3);
    let got = snap_points(&s, &rim);
    assert!(same(&of(&got, SnapKind::Centre), &[Vec3::new(45.0, 20.0, 10.0)], 1e-6), "{got:?}");
    assert_eq!(got.len(), 1, "a whole circle offers only its centre: {got:?}");
    assert!(got.iter().all(|p| p.body == "Body1"), "{got:?}");
}

#[test]
fn a_straight_edge_offers_its_ends_and_middle() {
    let s = plate();
    let front = edge(&s, |p| p.y.abs() < 1e-9 && (p.z - 10.0).abs() < 1e-9);
    let got = snap_points(&s, &front);
    assert!(same(&of(&got, SnapKind::End), &[Vec3::new(0.0, 0.0, 10.0), Vec3::new(60.0, 0.0, 10.0)], 1e-9), "{got:?}");
    assert!(same(&of(&got, SnapKind::Middle), &[Vec3::new(30.0, 0.0, 10.0)], 1e-9), "{got:?}");
}

#[test]
fn a_flat_face_offers_its_centre_corners_edge_middles_and_hole_centres() {
    let s = plate();
    let top = face(&s, |p| (p.z - 10.0).abs() < 1e-9);
    // The plate's area less the two holes, each holding its area at its centre.
    let (a, h6, h8) = (60.0 * 40.0, PI * 9.0, PI * 16.0);
    let x = (a * 30.0 - h6 * 15.0 - h8 * 45.0) / (a - h6 - h8);
    let got = snap_points(&s, &top);
    // The holes' circles are tessellated, so their areas are a little off.
    assert!(same(&of(&got, SnapKind::FaceCentre), &[Vec3::new(x, 20.0, 10.0)], 0.02), "want x = {x}: {got:?}");
    let at = |x: f64, y: f64| Vec3::new(x, y, 10.0);
    let corners = [at(0.0, 0.0), at(60.0, 0.0), at(60.0, 40.0), at(0.0, 40.0)];
    assert!(same(&of(&got, SnapKind::End), &corners, 1e-9), "the four corners, none on a hole's seam: {got:?}");
    let middles = [at(30.0, 0.0), at(60.0, 20.0), at(30.0, 40.0), at(0.0, 20.0)];
    assert!(same(&of(&got, SnapKind::Middle), &middles, 1e-9), "{got:?}");
    assert!(same(&of(&got, SnapKind::Centre), &[at(15.0, 20.0), at(45.0, 20.0)], 1e-9), "both holes' centres: {got:?}");
    assert_eq!(got.len(), 11, "{got:?}");
}

#[test]
fn a_flat_faces_points_say_which_face_and_edge_they_came_from() {
    let s = plate();
    let top = face(&s, |p| (p.z - 10.0).abs() < 1e-9);
    let Sel::Face { point: pick, .. } = top else { panic!("the top is a face pick") };
    let got = snap_points_from(&s, &top);
    assert!(got.iter().all(|(_, from)| from.face == Some(pick)), "{got:?}");
    assert!(got.iter().all(|(_, from)| from.normal.is_some_and(|n| n.dist(Vec3::Z) < 1e-12)), "out of the material: {got:?}");
    let source = |want: Vec3| got.iter().find(|(p, _)| p.at.dist(want) < 1e-9).map(|(_, from)| from.edge);
    // The face's centre belongs to no edge; a middle to its own edge; a corner to an edge that
    // starts there; a hole's centre to its rim.
    assert_eq!(got.iter().find(|(p, _)| p.kind == SnapKind::FaceCentre).map(|(_, from)| from.edge), Some(None));
    assert_eq!(source(Vec3::new(30.0, 0.0, 10.0)), Some(Some(Vec3::new(30.0, 0.0, 10.0))));
    let corner = source(Vec3::ZERO + Vec3::new(0.0, 0.0, 10.0)).flatten().unwrap();
    assert!([Vec3::new(30.0, 0.0, 10.0), Vec3::new(0.0, 20.0, 10.0)].iter().any(|m| m.dist(corner) < 1e-9), "{corner:?}");
    let rim = source(Vec3::new(45.0, 20.0, 10.0)).flatten().unwrap();
    assert!((rim.dist(Vec3::new(45.0, 20.0, 10.0)) - 4.0).abs() < 1e-9 && (rim.z - 10.0).abs() < 1e-9, "{rim:?}");

    let bottom = face(&s, |p| p.z.abs() < 1e-9);
    let got = snap_points_from(&s, &bottom);
    assert!(got.iter().all(|(_, from)| from.normal.is_some_and(|n| n.dist(-Vec3::Z) < 1e-12)), "{got:?}");

    let front = edge(&s, |p| p.y.abs() < 1e-9 && (p.z - 10.0).abs() < 1e-9);
    let Sel::Edge { point, .. } = front else { panic!("the front is an edge pick") };
    let got = snap_points_from(&s, &front);
    assert!(got.iter().all(|(_, from)| *from == SnapSource { edge: Some(point), ..SnapSource::default() }), "{got:?}");
}

#[test]
fn a_face_with_a_hundred_holes_offers_every_centre() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 110, "width": 110, "height": 5}));
    run(&mut s, "solid.cylinder", json!({"radius": 2, "height": 7, "base": [10, 10, -1], "operation": "cut", "name": "Hole"}));
    run(
        &mut s,
        "solid.pattern.rectangular",
        json!({"features": ["Hole"], "dir1": [1, 0, 0], "count1": 10, "spacing1": 10, "dir2": [0, 1, 0], "count2": 10, "spacing2": 10}),
    );
    let top = face(&s, |p| (p.z - 5.0).abs() < 1e-9);
    let got = snap_points(&s, &top);
    let want: Vec<Vec3> = (0..100).map(|k| Vec3::new(10.0 + 10.0 * f64::from(k % 10), 10.0 + 10.0 * f64::from(k / 10), 5.0)).collect();
    assert!(same(&of(&got, SnapKind::Centre), &want, 1e-9), "{} centres: {got:?}", of(&got, SnapKind::Centre).len());
    assert_eq!(of(&got, SnapKind::End).len(), 4, "the plate's corners only: {got:?}");
}

#[test]
fn a_rounded_outline_offers_its_tangent_corners_and_its_rounds_centres() {
    // A 40 × 40 tube end: 4 mm rounds outside, a sharp 34 × 34 opening inside.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 20}));
    run(&mut s, "solid.fillet", json!({"edges": [[0, 0, 10], [40, 0, 10], [40, 40, 10], [0, 40, 10]], "radius": 4}));
    run(&mut s, "solid.box", json!({"corner": [3, 3, -1], "length": 34, "width": 34, "height": 22, "operation": "cut"}));
    let top = face(&s, |p| (p.z - 20.0).abs() < 1e-9);
    let got = snap_points(&s, &top);
    let at = |x: f64, y: f64| Vec3::new(x, y, 20.0);
    let rounds = [at(4.0, 4.0), at(36.0, 4.0), at(36.0, 36.0), at(4.0, 36.0)];
    assert!(same(&of(&got, SnapKind::Centre), &rounds, 1e-9), "{got:?}");
    let mut ends: Vec<Vec3> = [(4.0, 0.0), (36.0, 0.0), (40.0, 4.0), (40.0, 36.0), (36.0, 40.0), (4.0, 40.0), (0.0, 36.0), (0.0, 4.0)]
        .into_iter()
        .map(|(x, y)| at(x, y))
        .collect();
    ends.extend([at(3.0, 3.0), at(37.0, 3.0), at(37.0, 37.0), at(3.0, 37.0)]);
    assert!(same(&of(&got, SnapKind::End), &ends, 1e-9), "where each round meets a side, and the opening's corners: {got:?}");
    let middles = [at(20.0, 0.0), at(40.0, 20.0), at(20.0, 40.0), at(0.0, 20.0), at(20.0, 3.0), at(37.0, 20.0), at(20.0, 37.0), at(3.0, 20.0)];
    assert!(same(&of(&got, SnapKind::Middle), &middles, 1e-9), "{got:?}");
}

#[test]
fn a_moved_parts_face_offers_its_points_where_the_part_stands() {
    let mut s = plate();
    run(&mut s, "component.create", json!({"name": "Plate", "parent": "root", "activate": false}));
    run(&mut s, "component.move_bodies", json!({"bodies": ["Body1"], "component": "Plate"}));
    run(&mut s, "occurrence.ground", json!({"occurrence": "Plate:1", "grounded": false}));
    run(&mut s, "occurrence.move", json!({"occurrence": "Plate:1", "translate": [100, 0, 30]}));
    let st = s.world_state().unwrap();
    let b = st.bodies.first().unwrap();
    let m = b.mesh();
    // The face on top, now at z = 40: the plate moved 100 along x and 30 up.
    let on_top = |f: &u32| {
        m.triangles
            .iter()
            .zip(&m.tri_face)
            .filter(|(_, tf)| *tf == f)
            .filter_map(|(t, _)| m.tri(t))
            .all(|t| t.iter().all(|p| (p.z - 40.0).abs() < 1e-9))
    };
    let f = m.tri_face.iter().copied().find(on_top).unwrap();
    let pick = m.triangles.iter().zip(&m.tri_face).find(|(_, tf)| **tf == f).and_then(|(t, _)| m.tri(t)).unwrap()[0];
    let top = sel(json!({"type": "face", "body": b.name, "index": f, "point": pick}));
    let got = snap_points_from(&s, &top);
    let placed = |x: f64, y: f64| Vec3::new(x + 100.0, y, 40.0);
    let corners = [placed(0.0, 0.0), placed(60.0, 0.0), placed(60.0, 40.0), placed(0.0, 40.0)];
    let points: Vec<_> = got.iter().map(|(p, _)| p.clone()).collect();
    assert!(same(&of(&points, SnapKind::End), &corners, 1e-9), "{got:?}");
    assert!(same(&of(&points, SnapKind::Centre), &[placed(15.0, 20.0), placed(45.0, 20.0)], 1e-9), "{got:?}");
    assert!(got.iter().all(|(_, from)| from.normal.is_some_and(|n| n.dist(Vec3::Z) < 1e-12)), "{got:?}");
}

#[test]
fn an_arc_offers_its_centre_ends_and_middle() {
    // A Ø20 cylinder cut at the corner (60, 40) leaves a quarter circle about it on the top.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 60, "width": 40, "height": 10}));
    run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 12, "base": [60, 40, -1], "operation": "cut"}));
    let c = Vec3::new(60.0, 40.0, 10.0);
    let arc = edge(&s, |p| (p.z - 10.0).abs() < 1e-9 && ((p - c).len() - 10.0).abs() < 1e-3);
    let got = snap_points(&s, &arc);
    assert!(same(&of(&got, SnapKind::Centre), &[c], 1e-6), "{got:?}");
    assert!(same(&of(&got, SnapKind::End), &[Vec3::new(50.0, 40.0, 10.0), Vec3::new(60.0, 30.0, 10.0)], 1e-6), "{got:?}");
    let mid = Vec3::new(60.0 - 10.0 * FRAC_1_SQRT_2, 40.0 - 10.0 * FRAC_1_SQRT_2, 10.0);
    assert!(same(&of(&got, SnapKind::Middle), &[mid], 1e-6), "the exact middle of the arc, not of its chord: {got:?}");
}

#[test]
fn a_sphere_offers_its_centre_and_a_vertex_itself() {
    let mut s = Session::default();
    run(&mut s, "solid.sphere", json!({"radius": 7, "center": [5, -3, 12]}));
    let ball = face(&s, |_| true);
    let got = snap_points(&s, &ball);
    assert_eq!(got.iter().map(|p| p.kind).collect::<Vec<_>>(), [SnapKind::Centre], "{got:?}");
    assert!(got[0].at.dist(Vec3::new(5.0, -3.0, 12.0)) < 1e-6, "{got:?}");

    let v = sel(json!({"type": "vertex", "body": "Body1", "point": [1, 2, 3]}));
    let got = snap_points(&s, &v);
    assert_eq!(got, [SnapPoint { at: Vec3::new(1.0, 2.0, 3.0), kind: SnapKind::Vertex, body: "Body1".into() }]);
}

#[test]
fn things_without_points_offer_none() {
    let s = plate();
    for v in [
        json!({"type": "body", "name": "Body1"}),
        json!({"type": "plane", "name": "XY"}),
        json!({"type": "edge", "body": "Nope", "index": 0, "point": [0, 0, 0]}),
    ] {
        assert!(snap_points(&s, &sel(v.clone())).is_empty(), "{v}");
    }
}

#[test]
fn a_round_faces_end_centres_point_out_of_their_ends() {
    let s = plate();
    let wall = face(&s, |p| ((p - Vec3::new(15.0, 20.0, p.z)).len() - 3.0).abs() < 1e-3);
    let got = snap_points_from(&s, &wall);
    let normal_at = |z: f64| got.iter().find(|(p, _)| p.at.dist(Vec3::new(15.0, 20.0, z)) < 1e-6).and_then(|(_, from)| from.normal);
    assert!(normal_at(0.0).is_some_and(|n| n.dist(-Vec3::Z) < 1e-9), "the bottom end, out of the plate's bottom: {got:?}");
    assert!(normal_at(10.0).is_some_and(|n| n.dist(Vec3::Z) < 1e-9), "the top end, out of its top: {got:?}");
}
