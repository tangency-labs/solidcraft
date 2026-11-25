//! WR-606 (the fork's tangency-labs/smlib 5b0534b): `SmFilletRemoval` (`DeleteFilletFace`'s
//! removal of a four-sided fillet) runs a side edge that stops short of the new corner on
//! with a marched, fitted intersection of its two faces, and grows the faces past their
//! surfaces' domains with a natural extension: approximations except on lines and planes.
//! The fork's `SetExactOnly` makes the removal refuse instead, before the B-rep changes,
//! where a continuation would not be exact; `NotExact` tells. A side edge on a line runs
//! on along that line.
//!
//! A 20 mm cube with its top front edge rounded r 2: every side edge is a line and every
//! face a plane, so the exact-only removal gives the sharp cube back, the same as
//! upstream's. The refusals are tested on the upstream fixture solids, in the
//! `blend.fillet_removal` runner's `exact_only_tests`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_removal::FilletRemoval;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::surface::plane::Plane;
use solidcore::topology::face::p2 as face2;
use solidcore::topology::{edge, vertex, Brep, EdgeId, FaceId};
use solidcore::{Kernel, Result};

/// The edge of `b` whose two vertices satisfy `on`.
fn edge_where(b: &Brep, on: impl Fn(Point3) -> bool) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let (s, t) = edge::start_end_vertices(b, e).unwrap();
            on(vertex::point(b, s.unwrap()).unwrap()) && on(vertex::point(b, t.unwrap()).unwrap())
        })
        .unwrap()
}

/// The faces of `b` whose surface is not a plane.
fn curved_faces(b: &Brep) -> Vec<FaceId> {
    b.face_list()
        .iter()
        .copied()
        .filter(|&f| {
            !face2::surface(b, f)
                .unwrap()
                .unwrap()
                .is_kind_of(Plane::TYPE)
                .unwrap()
        })
        .collect()
}

/// Face, edge and vertex counts and the vertex points, sorted.
fn snapshot(b: &Brep) -> (usize, usize, Vec<[f64; 3]>) {
    let mut points: Vec<[f64; 3]> = b
        .vertex_list()
        .iter()
        .map(|&v| vertex::point(b, v).unwrap().to_array())
        .collect();
    points.sort_by(|p, q| p.partial_cmp(q).unwrap());
    (b.face_list().len(), b.edges().ids().count(), points)
}

/// `SmFilletRemoval(face, face tolerance)`, exact only or not, with editing enabled: the
/// status and `NotExact`.
fn remove(k: &mut Kernel, b: &mut Brep, f: FaceId, exact_only: bool) -> (Result<()>, bool) {
    b.set_editing_enabled(true);
    let tol = face2::tolerance(b, f).unwrap();
    let mut removal = FilletRemoval::new(f, tol);
    removal.set_exact_only(exact_only);
    let r = removal.remove_face(k, b);
    b.refresh_cached_topology();
    (r, removal.not_exact())
}

/// The 20 mm cube with its top front edge (y 0, z 20) rounded r 2.
fn rounded_cube(k: &mut Kernel) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 20.0).unwrap();
    let e = edge_where(&b, |p| p.y == 0.0 && p.z == 20.0);
    fillet_edges(k, &mut b, &[e], 2.0, 1, 1, 1.0).unwrap();
    b
}

#[test]
fn exact_only_is_off_by_default() {
    let mut k = Kernel::default();
    let mut b = rounded_cube(&mut k);
    let fillet = curved_faces(&b)[0];
    let (r, not_exact) = remove(&mut k, &mut b, fillet, false);
    r.unwrap();
    assert!(!not_exact);
}

#[test]
fn exact_only_removal_on_lines_and_planes_is_upstreams() {
    let mut k = Kernel::default();
    let mut upstream = rounded_cube(&mut k);
    let faces = curved_faces(&upstream);
    assert_eq!(faces.len(), 1, "one round");
    let (r, _) = remove(&mut k, &mut upstream, faces[0], false);
    r.unwrap();

    let mut k = Kernel::default();
    let mut exact = rounded_cube(&mut k);
    let faces = curved_faces(&exact);
    let (r, not_exact) = remove(&mut k, &mut exact, faces[0], true);
    r.unwrap();
    assert!(!not_exact);

    let (fu, eu, pu) = snapshot(&upstream);
    let (fe, ee, pe) = snapshot(&exact);
    assert_eq!(
        (fu, eu, pu.len()),
        (6, 12, 8),
        "upstream gives the sharp cube"
    );
    assert_eq!(
        (fe, ee, pe.len()),
        (6, 12, 8),
        "exact only gives the sharp cube"
    );
    for (p, q) in pu.iter().zip(&pe) {
        for i in 0..3 {
            assert!((p[i] - q[i]).abs() <= 1e-9, "{p:?} vs {q:?}");
        }
    }
}
