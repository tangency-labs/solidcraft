//! tangency-labs/solidcore#1073 (WR-460): rounds on the four vertical edges of a
//! 14 x 6 x 10 post, each 6 mm face carrying two of them. Past r = 3 the two rails on a
//! face have crossed and SMLib builds an open body. The opt-in
//! `Kernel::fixes.fillet_refuse_crossing_rails` refuses it before the post is edited.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // test helpers
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::{edge, vertex};
use solidcore::{Brep, Kernel, Result};

/// Fillet the post's vertical edges with `r`; the post and whether it is a solid after.
fn round(fixed: bool, r: f64) -> (Brep, Result<()>, bool) {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_crossing_rails = fixed;
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 14.0, 6.0, 10.0).unwrap();
    let vertical: Vec<_> = b
        .edges()
        .ids()
        .filter(|&e| {
            let (s, t) = edge::start_end_vertices(&b, e).unwrap();
            let (p, q) = (
                vertex::point(&b, s.unwrap()).unwrap(),
                vertex::point(&b, t.unwrap()).unwrap(),
            );
            (p.x - q.x).abs() < 1e-9 && (p.y - q.y).abs() < 1e-9
        })
        .collect();
    assert_eq!(vertical.len(), 4);
    let faces_before = b.face_list().len();
    let result = fillet_edges(&mut k, &mut b, &vertical, r, 1, 1, 1.0);
    if result.is_err() {
        assert_eq!(b.face_list().len(), faces_before, "post left edited");
    }
    let solid = result.is_ok() && is_manifold_solid(&b).unwrap();
    (b, result, solid)
}

#[test]
fn issue_1073_flag_is_off_by_default() {
    assert!(!Kernel::default().fixes.fillet_refuse_crossing_rails);
}

#[test]
fn issue_1073_upstream_builds_an_open_body() {
    let (_, result, solid) = round(false, 3.5);
    result.unwrap();
    assert!(!solid);
}

#[test]
fn issue_1073_rounds_that_fit_are_kept() {
    for r in [2.9, 3.0] {
        let (_, result, solid) = round(true, r);
        result.unwrap();
        assert!(solid, "r = {r}");
    }
}

#[test]
fn issue_1073_rounds_past_the_face_are_refused() {
    for r in [3.03, 3.0625, 3.5, 4.0] {
        let (_, result, _) = round(true, r);
        assert!(result.is_err(), "r = {r}");
    }
}

#[test]
fn issue_1073_all_edges_that_fit_are_kept() {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_crossing_rails = true;
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 14.0, 6.0, 10.0).unwrap();
    let all: Vec<_> = b.edges().ids().collect();
    fillet_edges(&mut k, &mut b, &all, 2.5, 1, 1, 1.0).unwrap();
    assert!(is_manifold_solid(&b).unwrap());
}
