//! Regression test for WR-545 (the fork's tangency-labs/smlib@cd85d34): a 40 x 30 x 20
//! box with its four vertical edges rounded R6, shelled 2 inward open at the top. The
//! inner quarter circles of the rim (R4, where the rounds' offsets meet the top face)
//! come from marched intersections. With `shell_edges_on_analytics_exact`, every curved
//! edge of the rim is a circle: four R6 outer arcs and four R4 inner arcs.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, shell};
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::curve::curve::p2::CurveKind;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::Kernel;

#[test]
fn wr_545_shelled_rounded_box_rim_is_arcs() {
    let mut k = Kernel::default();
    k.fixes.shell_edges_on_analytics_exact = true;
    let mut part = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let uprights: Vec<_> = part
        .edges()
        .iter()
        .filter(|(_, e)| {
            let c = e.curve.as_ref().unwrap();
            let p = c.evaluate_point(&k, e.interval.min()).unwrap();
            let q = c.evaluate_point(&k, e.interval.max()).unwrap();
            (p.x - q.x).abs() < 1e-9 && (p.y - q.y).abs() < 1e-9
        })
        .map(|(id, _)| id)
        .collect();
    assert_eq!(uprights.len(), 4, "the four vertical edges");
    fillet_edges(&mut k, &mut part, &uprights, 6.0, 1, 1, 1.0).unwrap();
    part.refresh_cached_topology();
    let top = planar_faces_at(&k, &part, 2, 20.0);
    assert_eq!(top.len(), 1);

    let shelled = shell(&mut k, part, -2.0, &top).unwrap();
    assert!(brep_is_manifold_solid(&shelled).unwrap());
    // the rim's curved edges: both ends and the middle at z = 20, not a line
    let mut kinds = Vec::new();
    for &id in shelled.edge_list() {
        let e = shelled.edge(id).unwrap();
        let c = e.curve.as_ref().unwrap();
        let on_top = [0.0, 0.5, 1.0].iter().all(|&s| {
            (c.evaluate_point(&k, e.interval.evaluate(s)).unwrap().z - 20.0).abs() < 1e-6
        });
        if on_top && !matches!(c.kind(), CurveKind::Line(_)) {
            kinds.push(matches!(c.kind(), CurveKind::Ellipse(_)));
        }
    }
    assert_eq!(kinds.len(), 8, "four outer and four inner arcs");
    assert!(kinds.iter().all(|&circle| circle), "{kinds:?}");
}
