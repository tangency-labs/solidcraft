//! Regression test for WR-546 (#1377; the fork's tangency-labs/smlib@1072d1b): a box
//! 40 x 40 x 40 with every edge rounded r 4 and a setback of 7 at the corner (0, 0, 0),
//! the issue's case. The corner's six-sided patch is fitted to polynomial
//! approximations of the rounds' sections, and its faces are bounded by their
//! surfaces' isoparametric curves. Upstream's stitch keeps, of a section half and the
//! patch's edge, the one nearer the other's faces: two near-equal distances, so a
//! section half could come out as the approximation, 1.4e-5 off the round. With
//! `fillet_patch_keeps_fillet_edges` every edge between the patch and a round is the
//! round's own edge and lies on it. The setback itself needs
//! `fillet_setback_from_start_vertex` (WR-445).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::surface_closest_point;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::face::p2 as face;
use solidcore::topology::{edge, vertex, Brep};
use solidcore::{FaceId, Kernel, VertexId};

fn at_origin(b: &Brep, v: VertexId) -> bool {
    let p = vertex::point(b, v).unwrap();
    p.x.abs() + p.y.abs() + p.z.abs() < 1e-9
}

/// The distance from the origin of the face's surface point at the middle of its
/// UV domain.
fn face_middle_from_origin(k: &Kernel, b: &Brep, f: FaceId) -> f64 {
    let s = face::surface(b, f).unwrap().unwrap();
    let p = s
        .evaluate_point(k, face::uv_domain(b, f).unwrap().mid())
        .unwrap();
    (p.x * p.x + p.y * p.y + p.z * p.z).sqrt()
}

#[test]
fn wr_546_setback_patch_meets_the_rounds_on_their_own_edges() {
    let mut k = Kernel::default();
    k.fixes.fillet_setback_from_start_vertex = true;
    k.fixes.fillet_patch_keeps_fillet_edges = true;
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 40.0, 40.0).unwrap();
    let edges: Vec<_> = b.edges().ids().collect();
    assert_eq!(edges.len(), 12);
    {
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &edges, 4.0, 1, 1, 1.0)
            .unwrap();
        session.create_corners(&k, None).unwrap();
        for id in session.fillet_corners().unwrap() {
            if let Some(v) = session.corner(id).unwrap().vertex {
                if at_origin(session.target(), v) {
                    session.set_corner_setback(id, 7.0).unwrap();
                }
            }
        }
        session.do_filleting(&mut k, None).unwrap();
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();

    // The edges between a patch face (its middle within 12 of the corner) and a round
    // along one of the corner's edges (its middle beyond it) lie on the round.
    let mut checked = 0;
    for e in b.edges().ids() {
        let faces = edge::faces(&b, e, None).unwrap();
        let [f1, f2] = faces[..] else { continue };
        let (d1, d2) = (
            face_middle_from_origin(&k, &b, f1),
            face_middle_from_origin(&k, &b, f2),
        );
        let round = match (d1 < 12.0, d2 < 12.0) {
            (true, false) => f2,
            (false, true) => f1,
            _ => continue,
        };
        let round_surface = face::surface(&b, round).unwrap().unwrap();
        if round_surface
            .is_kind_of(solidcore::surface::plane::Plane::TYPE)
            .unwrap()
        {
            continue;
        }
        let c = edge::curve(&b, e).unwrap().unwrap();
        let iv = edge::interval(&b, e).unwrap();
        for i in 0..=10 {
            let p = c
                .evaluate_point(&k, iv.min() + iv.length() * f64::from(i) / 10.0)
                .unwrap();
            let gap = surface_closest_point(&k, round_surface, p)
                .unwrap()
                .closest
                .distance;
            assert!(gap < 1e-7, "edge {e:?} is {gap} off its round");
        }
        checked += 1;
    }
    assert!(checked >= 3, "{checked} patch/round edges");
}
