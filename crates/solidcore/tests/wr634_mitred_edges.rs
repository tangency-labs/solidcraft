//! WR-634 addendum (the fork's tangency-labs/smlib `9ba2b12`): the SMLib side of "rounds
//! mitred to one that overflows go first with it". When the executive stops for overflow
//! first (or classify only), it also notes the edges of each two blends mitred at a
//! face's concave corner (`SmFilletNx2Corner::IsMitre`, `GetFilletedEdgeuses`,
//! `SmFilletExecutive::GetMitredEdges`), two by two. The ordering policy that uses them
//! is the caller's (the fork's C API), as for the overflowing edges.
//!
//! The part is WR-642's pocketed plate (60 x 40 x 10, a 30 x 20 pocket 5 deep): its four
//! rim edges chamfered 3 meet in mitres at the pocket's four concave corners with
//! `fillet_mitre_concave_face_corners`; without it upstream sweeps a 3-sided face there
//! and no corner is a mitre.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeSet;

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_geom::p1::FilletErrorType;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn p(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z)
}

fn edge_between(k: &Kernel, b: &Brep, a: Point3, q: Point3) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            let s = c.evaluate_point(k, ivl.min()).unwrap();
            let t = c.evaluate_point(k, ivl.max()).unwrap();
            (s.distance(a) < 1e-9 && t.distance(q) < 1e-9)
                || (s.distance(q) < 1e-9 && t.distance(a) < 1e-9)
        })
        .expect("edge")
}

fn pocketed_plate(k: &mut Kernel) -> Brep {
    let plate = create_box(k, Point3::ZERO, 60.0, 40.0, 10.0).unwrap();
    let pocket = create_box(k, p(15.0, 10.0, 5.0), 30.0, 20.0, 6.0).unwrap();
    boolean_difference(k, plate, pocket).unwrap()
}

/// The rim's corners, in order round the pocket.
const RIM: [(f64, f64); 4] = [(15.0, 10.0), (45.0, 10.0), (45.0, 30.0), (15.0, 30.0)];

/// Chamfers the rim by 3 with classify only: the stop's code, the overflowing and the
/// mitred edges, and the rim's edges in `RIM` order.
fn classify(mitre: bool) -> (i32, Vec<EdgeId>, Vec<EdgeId>, Vec<EdgeId>) {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_rail_end_on_continued_surface = true;
    k.fixes.fillet_cliff_overflow = true;
    k.fixes.fillet_mitre_concave_face_corners = mitre;
    let mut b = pocketed_plate(&mut k);
    let rim: Vec<EdgeId> = (0..4)
        .map(|i| {
            let (a, q) = (RIM[i], RIM[(i + 1) % 4]);
            edge_between(&k, &b, p(a.0, a.1, 10.0), p(q.0, q.1, 10.0))
        })
        .collect();
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.set_classify_only(true);
    s.prepare_constant_radius_edges(&mut k, &rim, 3.0, 0, 1, 1.0)
        .unwrap();
    assert!(s.do_fillet(&mut k).is_err(), "classify only builds nothing");
    (
        s.error_info().code,
        s.overflow_edges(),
        s.mitred_edges(),
        rim,
    )
}

/// The mitred pairs as sets of two edges.
fn pairs(edges: &[EdgeId]) -> BTreeSet<BTreeSet<EdgeId>> {
    assert_eq!(edges.len() % 2, 0, "two by two");
    edges
        .chunks(2)
        .map(|pair| pair.iter().copied().collect())
        .collect()
}

#[test]
fn fork_9ba2b12_mitred_pairs_are_noted_at_the_stop() {
    let (code, overflow, mitred, rim) = classify(true);
    assert_eq!(code, FilletErrorType::OverflowFirst as i32);
    assert!(overflow.is_empty(), "chamfer 3 fits the 5 deep walls");
    // each corner of the pocket: the two rim edges meeting there
    let want: BTreeSet<BTreeSet<EdgeId>> = (0..4)
        .map(|i| [rim[i], rim[(i + 1) % 4]].into_iter().collect())
        .collect();
    assert_eq!(mitred.len(), 8, "{mitred:?}");
    assert_eq!(pairs(&mitred), want);
}

#[test]
fn fork_9ba2b12_no_mitred_pairs_without_mitres() {
    let (code, _, mitred, _) = classify(false);
    assert_eq!(code, FilletErrorType::OverflowFirst as i32);
    assert!(mitred.is_empty(), "{mitred:?}");
}
