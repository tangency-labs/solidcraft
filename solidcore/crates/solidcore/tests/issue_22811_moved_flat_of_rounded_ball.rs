//! #22811: moving the flat of a ball that was cut flat and had the flat's edge rounded
//! (`OffsetFacesExecutive::offset_faces`). The boolean leaves the flat's rim as two arcs (one
//! ends on the sphere's seam), so the round is two faces that meet at their ends: neither
//! comes off alone exactly, each one's end lying on the other's torus. They come off
//! together through the defeature, which takes them when every face round them continues
//! exactly: the flat's plane and the sphere, whose definition is the whole sphere. Both
//! pieces then come off onto the one sharp circle, which is rounded once again after the
//! move. Before, the sphere was refused as `OffsetFacesRefusal::NotExact`.
//!
//! The result is a body of revolution about Z: the ball of radius `R` up to where the
//! round of radius `r` touches it, then the round up to the flat at `h`. The round's centre
//! circle is at `zc = h - r`, radius `rho = sqrt((R - r)^2 - zc^2)`; it touches the sphere at
//! `zs = R zc / (R - r)`. The volume is `pi [R^2 z - z^3 / 3]` from `-R` to `zs` plus
//! `pi * integral (rho + sqrt(r^2 - u^2))^2 du` for `u` from `zs - zc` to `r`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::error::offset::OffsetFailure;
use solidcore::error::Reason;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::det;
use solidcore::math::{Point3, PI};
use solidcore::offset::offset_faces::OffsetFacesExecutive;
use solidcore::primitive::api_primitives::{create_box, create_sphere};
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::surface::plane::Plane;
use solidcore::topology::face::p2 as face2;
use solidcore::topology::{edge, vertex, Brep, EdgeId, FaceId};
use solidcore::Kernel;

/// The kernel SolidCraft runs (every fix on).
fn kernel() -> Kernel {
    Kernel {
        fixes: solidcore::kernel::UpstreamFixes::all(),
        ..Kernel::default()
    }
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

/// Whether every vertex of `e` is at height `z`.
fn edge_at(b: &Brep, e: EdgeId, z: f64) -> bool {
    let (s, t) = edge::start_end_vertices(b, e).unwrap();
    [s, t]
        .into_iter()
        .flatten()
        .all(|v| (vertex::point(b, v).unwrap().z - z).abs() < 1e-6)
}

/// The plane face whose edges are all at height `z`, if there is one.
fn flat(b: &Brep, z: f64) -> Option<FaceId> {
    b.face_list().iter().copied().find(|&f| {
        face2::surface(b, f)
            .unwrap()
            .unwrap()
            .is_kind_of(Plane::TYPE)
            .unwrap()
            && solidcore::topology::face::p4::edges(b, f, None, false, None)
                .unwrap()
                .into_iter()
                .all(|e| edge_at(b, e, z))
    })
}

/// A ball of radius `radius` about the origin cut flat at `h`, the flat's edge rounded
/// `round`.
fn rounded_ball(k: &mut Kernel, radius: f64, h: f64, round: f64) -> Brep {
    let ball = create_sphere(k, Point3::new(0.0, 0.0, 0.0), radius).unwrap();
    let side = 3.0 * radius;
    // (from the flat to past the north pole)
    let above = create_box(
        k,
        Point3::new(-side / 2.0, -side / 2.0, h),
        side,
        side,
        2.0 * radius,
    )
    .unwrap();
    let mut b = boolean_difference(k, ball, above).unwrap();
    let rim: Vec<EdgeId> = b.edges().ids().filter(|&e| edge_at(&b, e, h)).collect();
    // the boolean splits the rim in two
    assert_eq!(rim.len(), 2, "the flat's rim");
    fillet_edges(k, &mut b, &rim, round, 1, 1, 1.0).unwrap();
    b
}

/// The closed form above: the ball of radius `big` with its flat at `h` and that flat's
/// edge rounded `r`.
fn closed_form(big: f64, h: f64, r: f64) -> f64 {
    let zc = h - r;
    let rho = (det::powi(big - r, 2) - zc * zc).sqrt();
    let zs = big * zc / (big - r);
    let cap = |z: f64| big * big * z - z * z * z / 3.0;
    // the antiderivative of (rho + sqrt(r^2 - u^2))^2
    let round = |u: f64| {
        let w = (r * r - u * u).max(0.0).sqrt();
        rho * rho * u + rho * (u * w + r * r * det::asin(u / r)) + r * r * u - u * u * u / 3.0
    };
    PI * (cap(zs) - cap(-big)) + PI * (round(r) - round(zs - zc))
}

/// The ball with its flat at `h` rounded `r`, the flat moved `d`: checked against the
/// closed form at its flat's new height to `1e-9` of the volume.
fn moves(big: f64, h: f64, r: f64, d: f64) {
    let mut k = kernel();
    let mut b = rounded_ball(&mut k, big, h, r);
    let before = volume(&k, &mut b);
    let rel = |got: f64, want: f64| (got - want).abs() / want;
    // the body built is the closed form (the check of the check)
    assert!(rel(before, closed_form(big, h, r)) < 1e-9, "{before}");
    let top = flat(&b, h).unwrap();
    let mut ex = OffsetFacesExecutive::new();
    let res = ex.offset_faces(&mut k, &mut b, &[top], d);
    assert!(res.is_ok(), "{res:?}: {:?} {}", ex.refusal(), ex.reason());
    assert_eq!(ex.followed_blends(), 2, "both pieces of the round follow");
    assert!(brep_is_manifold_solid(&b).unwrap());
    assert!(flat(&b, h + d).is_some(), "the flat at its new height");
    let got = volume(&k, &mut b);
    let want = closed_form(big, h + d, r);
    assert!(rel(got, want) < 1e-9, "volume {got}, closed form {want}");
}

/// The issue's part: a 20 mm ball cut at 6, rounded 1, the flat moved up 1 (3933.1967 mm3).
#[test]
fn the_rounded_flat_of_a_ball_moves_up() {
    assert!((closed_form(10.0, 7.0, 1.0) - 3933.1967).abs() < 1e-4);
    moves(10.0, 6.0, 1.0, 1.0);
}

#[test]
fn the_rounded_flat_of_a_ball_moves_down() {
    moves(10.0, 6.0, 1.0, -1.0);
}

/// A hemisphere's flat, up and down.
#[test]
fn the_rounded_flat_of_a_hemisphere_moves() {
    moves(10.0, 0.0, 1.0, 1.0);
    moves(10.0, 0.0, 1.0, -1.0);
}

/// A flat below the equator, and a small ball (4 mm, cut at 1.2, rounded 0.16).
#[test]
fn the_rounded_flat_of_a_ball_moves_below_the_equator_and_small() {
    moves(10.0, -6.0, 1.0, 1.0);
    moves(2.0, 1.2, 0.16, 0.19);
    moves(2.0, 1.2, 0.16, -0.19);
}

/// A move that carries the flat past the north pole leaves no sphere for it to meet: still
/// refused before anything changes, naming how far the flat could go (to the pole, 4).
#[test]
fn the_rounded_flat_of_a_ball_moved_past_the_pole_is_refused() {
    let mut k = kernel();
    let mut b = rounded_ball(&mut k, 10.0, 6.0, 1.0);
    let top = flat(&b, 6.0).unwrap();
    let mut ex = OffsetFacesExecutive::new();
    let e = ex.offset_faces(&mut k, &mut b, &[top], 5.0).unwrap_err();
    let largest = std::iter::successors(Some(&e), |e| e.cause()).find_map(|e| {
        match e.failure().map(|f| &f.reason) {
            Some(&Reason::Offset(OffsetFailure::PastSide { largest, .. })) => Some(largest),
            _ => None,
        }
    });
    let largest = largest.unwrap_or_else(|| panic!("{e:?}"));
    assert!((largest - 4.0).abs() < 1e-6, "{largest}");
}
