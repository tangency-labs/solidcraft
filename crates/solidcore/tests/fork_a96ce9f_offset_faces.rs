//! WR-615 (the fork's tangency-labs/smlib a96ce9f, 607d617, 758829c): moving a face
//! along its normal, the tangent blends on it follow (`SmOffsetFacesExecutive`,
//! `offset::offset_faces::OffsetFacesExecutive`): the body is
//! blend(move(unblend(body))). The cases are the fork's capi tests
//! (`capi/tests/ops_kernel.cpp`):
//! - tangency-labs/solidcraft#3468: a 60 x 40 x 10 plate with a boss r 10, 10 high, at
//!   (30, 20), its foot rounded r 2, the wall moved out by 1 and in by 1 and 3:
//!   24000 + pi R^2 10 + 2 pi (R + d) A for R = 11, 9, 7, with A = r^2 (1 - pi/4) and
//!   d = r^3 (5/6 - pi/4) / A (Pappus), nine faces;
//! - a 60 x 40 x 20 block, its top front edge rounded r 2, the top up and down by 1:
//!   2400 (20 +- 1) - 60 A, seven faces.
//!
//! Pinned SMLib's face move (`FaceuseOffset` and `SmBrep::LocalOperation`, unchanged)
//! keeps the round where it was and builds a step on it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::PI;

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::offset::offset_faces::{OffsetFacesExecutive, OffsetFacesRefusal};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::surface::plane::Plane;
use solidcore::topology::face::p2 as face2;
use solidcore::topology::{edge, vertex, Brep, EdgeId, FaceId};
use solidcore::Kernel;

/// The volume at the integration tolerance 1e-9.
fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

fn near(got: f64, want: f64, rel: f64) -> bool {
    (got - want).abs() <= rel * want.abs()
}

/// The corner region of a round's section, r^2 (1 - pi/4), for r 2.
fn corner() -> f64 {
    4.0 * (1.0 - PI / 4.0)
}

/// The vertices of `f`'s edges.
fn face_points(b: &Brep, f: FaceId) -> Vec<Point3> {
    let mut out = Vec::new();
    for e in solidcore::topology::face::p4::edges(b, f, None, false, None).unwrap() {
        let (s, t) = edge::start_end_vertices(b, e).unwrap();
        for v in [s, t].into_iter().flatten() {
            out.push(vertex::point(b, v).unwrap());
        }
    }
    out
}

fn is_plane(b: &Brep, f: FaceId) -> bool {
    face2::surface(b, f)
        .unwrap()
        .unwrap()
        .is_kind_of(Plane::TYPE)
        .unwrap()
}

/// The edge of `b` whose two vertices satisfy `on`.
fn edge_where(b: &Brep, on: impl Fn(Point3) -> bool) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let (s, t) = edge::start_end_vertices(b, e).unwrap();
            [s, t]
                .into_iter()
                .flatten()
                .all(|v| on(vertex::point(b, v).unwrap()))
        })
        .unwrap()
}

/// The plate with the boss and its foot round, and the boss's wall.
fn boss_with_foot_round(k: &mut Kernel) -> (Brep, FaceId) {
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0).unwrap();
    let boss = create_cylinder(k, Vec3::new(30.0, 20.0, 10.0), 10.0, 10.0).unwrap();
    let mut b = boolean_union(k, plate, boss).unwrap();
    let foot = b
        .edges()
        .ids()
        .find(|&e| {
            let v = edge::start_vertex(&b, e).unwrap().unwrap();
            edge::is_closed(&b, e).unwrap() && (vertex::point(&b, v).unwrap().z - 10.0).abs() < 1e-9
        })
        .unwrap();
    fillet_edges(k, &mut b, &[foot], 2.0, 1, 1, 1.0).unwrap();
    // the wall: the curved face whose vertices are at radius 10 above z 11.9
    let wall = *b
        .face_list()
        .iter()
        .find(|&&f| {
            !is_plane(&b, f)
                && face_points(&b, f).iter().all(|p| {
                    ((p.x - 30.0).powi(2) + (p.y - 20.0).powi(2)).sqrt() > 9.99 && p.z > 11.9
                })
        })
        .unwrap();
    (b, wall)
}

/// 3468's body with the boss's wall moved by `d`: its volume and face count.
fn moved_wall(d: f64) -> (f64, usize) {
    let mut k = Kernel::default();
    let (mut b, wall) = boss_with_foot_round(&mut k);
    let mut ex = OffsetFacesExecutive::new();
    let r = ex.offset_faces(&mut k, &mut b, &[wall], d);
    assert!(r.is_ok(), "{r:?}: {:?} {}", ex.refusal(), ex.reason());
    assert_eq!(ex.followed_blends(), 1);
    assert!(brep_is_manifold_solid(&b).unwrap());
    (volume(&k, &mut b), b.face_list().len())
}

/// The plate and a boss of radius `r` with its r 2 foot round.
fn boss_of(r: f64) -> f64 {
    let centroid = 8.0 * (5.0 / 6.0 - PI / 4.0) / corner();
    24000.0 + PI * r * r * 10.0 + 2.0 * PI * (r + centroid) * corner()
}

#[test]
fn the_formula_is_the_forks() {
    assert!(near(boss_of(11.0), 27863.06545197909, 1e-12));
}

#[test]
fn the_foot_round_follows_the_wall_out() {
    let (v, faces) = moved_wall(1.0);
    assert!(
        near(v, boss_of(11.0), 1e-9),
        "volume {v}, expected {}",
        boss_of(11.0)
    );
    assert_eq!(faces, 9);
}

#[test]
fn the_foot_round_follows_the_wall_in() {
    for (d, r) in [(-1.0, 9.0), (-3.0, 7.0)] {
        let (v, faces) = moved_wall(d);
        assert!(
            near(v, boss_of(r), 1e-9),
            "d {d}: volume {v}, expected {}",
            boss_of(r)
        );
        assert_eq!(faces, 9, "d {d}");
    }
}

#[test]
fn the_top_front_round_follows_the_top() {
    for up in [1.0, -1.0] {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 20.0).unwrap();
        let e = edge_where(&b, |p| p.y.abs() < 1e-6 && (p.z - 20.0).abs() < 1e-6);
        fillet_edges(&mut k, &mut b, &[e], 2.0, 1, 1, 1.0).unwrap();
        let top = *b
            .face_list()
            .iter()
            .find(|&&f| {
                is_plane(&b, f) && face_points(&b, f).iter().all(|p| (p.z - 20.0).abs() < 1e-6)
            })
            .unwrap();
        let mut ex = OffsetFacesExecutive::new();
        let r = ex.offset_faces(&mut k, &mut b, &[top], up);
        assert!(
            r.is_ok(),
            "up {up}: {r:?}: {:?} {}",
            ex.refusal(),
            ex.reason()
        );
        assert_eq!(ex.followed_blends(), 1);
        let want = 2400.0 * (20.0 + up) - 60.0 * corner();
        let v = volume(&k, &mut b);
        assert!(near(v, want, 1e-9), "up {up}: volume {v}, expected {want}");
        assert_eq!(b.face_list().len(), 7, "up {up}");
    }
}

#[test]
fn a_face_without_blends_moves_as_the_local_operation_moves_it() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let top = *b
        .face_list()
        .iter()
        .find(|&&f| face_points(&b, f).iter().all(|p| (p.z - 10.0).abs() < 1e-6))
        .unwrap();
    let mut ex = OffsetFacesExecutive::new();
    ex.offset_faces(&mut k, &mut b, &[top], 2.0).unwrap();
    assert_eq!(ex.followed_blends(), 0);
    assert_eq!(ex.refusal(), OffsetFacesRefusal::None);
    assert!(near(volume(&k, &mut b), 1200.0, 1e-9));
    assert_eq!(b.face_list().len(), 6);
}

#[test]
fn no_faces_or_no_distance_is_invalid() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap();
    let f = b.face_list()[0];
    let mut ex = OffsetFacesExecutive::new();
    assert!(ex.offset_faces(&mut k, &mut b, &[], 1.0).is_err());
    assert!(ex.offset_faces(&mut k, &mut b, &[f], 0.0).is_err());
    assert!(ex.offset_faces(&mut k, &mut b, &[f], f64::NAN).is_err());
}
