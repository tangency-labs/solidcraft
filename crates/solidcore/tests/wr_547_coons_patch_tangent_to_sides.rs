//! Regression test for WR-547 (the fork's tangency-labs/smlib@cae8618): an L-shaped
//! box (20 x 20 x 10 less the quarter x 10..20, y 10..20) with the three edges at its
//! notch's top corner (10, 10, 10) rounded r 2: the concave vertical edge and the two
//! convex top edges. The corner patch meets a concave and a convex round of different
//! curvature; the bicubic Coons patch's mean twist left them at an angle along whole
//! sides (14.7 degrees in the fork's L-shaped box). With
//! `corner_blend_coons_patch_tangent_to_sides`, every seam between two curved faces is
//! within 1 degree.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::math::Point3;
use solidcore::nurbs::knots::Side;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::surface_closest_point;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::face::p2 as face;
use solidcore::topology::{edge, vertex};
use solidcore::Kernel;

#[test]
fn wr_547_notch_corner_patch_is_tangent_to_its_rounds() {
    let mut k = Kernel::default();
    k.fixes.corner_blend_coons_patch_tangent_to_sides = true;
    let block = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 10.0).unwrap();
    let notch = create_box(&mut k, Point3::new(10.0, 10.0, -1.0), 20.0, 20.0, 12.0).unwrap();
    let mut b = boolean_difference(&mut k, block, notch).unwrap();
    b.refresh_cached_topology();

    let corner = Point3::new(10.0, 10.0, 10.0);
    let mut edges = Vec::new();
    for e in b.edges().ids() {
        let (s, t) = edge::start_end_vertices(&b, e).unwrap();
        if [s, t]
            .into_iter()
            .flatten()
            .any(|v| vertex::point(&b, v).unwrap().distance(corner) < 1e-9)
        {
            edges.push(e);
        }
    }
    assert_eq!(edges.len(), 3, "the edges at the notch's top corner");
    fillet_edges(&mut k, &mut b, &edges, 2.0, 1, 1, 1.0).unwrap();

    let normal = |s: &Surface, p: Point3| {
        let uv = surface_closest_point(&k, s, p).unwrap().uvs[0];
        s.evaluate_normal(&k, uv, Side::Left, Side::Left).unwrap()
    };
    let mut worst: f64 = 0.0;
    let mut seams = 0;
    for e in b.edges().ids() {
        let faces = edge::faces(&b, e, None).unwrap();
        let [f1, f2] = faces[..] else { continue };
        let (Some(s1), Some(s2)) = (
            face::surface(&b, f1).unwrap(),
            face::surface(&b, f2).unwrap(),
        ) else {
            continue;
        };
        if s1.is_kind_of(Plane::TYPE).unwrap() || s2.is_kind_of(Plane::TYPE).unwrap() {
            continue;
        }
        seams += 1;
        let c = edge::curve(&b, e).unwrap().unwrap();
        let iv = edge::interval(&b, e).unwrap();
        for i in 1..20 {
            let p = c
                .evaluate_point(&k, iv.min() + iv.length() * f64::from(i) / 20.0)
                .unwrap();
            let cos = normal(s1, p).dot(normal(s2, p)).abs().min(1.0);
            worst = worst.max(cos.acos().to_degrees());
        }
    }
    assert!(seams > 0);
    assert!(
        worst < 1.0,
        "a corner-patch seam is {worst} degrees off tangent"
    );
}
