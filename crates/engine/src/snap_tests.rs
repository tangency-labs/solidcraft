//! Snap points (#5881): the centres, middles and ends edges and faces offer. Every expected
//! point is worked out by hand from the part's dimensions.

use std::f64::consts::{FRAC_1_SQRT_2, PI};

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use crate::snap::{SnapKind, SnapPoint, snap_points};
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
fn a_flat_face_offers_the_centre_of_its_area() {
    let s = plate();
    let top = face(&s, |p| (p.z - 10.0).abs() < 1e-9);
    // The plate's area less the two holes, each holding its area at its centre.
    let (a, h6, h8) = (60.0 * 40.0, PI * 9.0, PI * 16.0);
    let x = (a * 30.0 - h6 * 15.0 - h8 * 45.0) / (a - h6 - h8);
    let got = snap_points(&s, &top);
    // The holes' circles are tessellated, so their areas are a little off.
    assert!(same(&of(&got, SnapKind::FaceCentre), &[Vec3::new(x, 20.0, 10.0)], 0.02), "want x = {x}: {got:?}");
    assert_eq!(got.len(), 1, "{got:?}");
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
