//! The fork's own regression test for WR-547 (tangency-labs/smlib@cae8618, capi test
//! `ops_kernel`, "the same 3x2 mixed corner in an L-shaped box"), ported: the 60 x 40 x 20
//! box less the notch (40, 20, 10)-(70, 50, 30), the notch floor's concave edge and its
//! convex side edge rounded together at r 2 and at r 5. The corner at (60, 20, 10) gets
//! one B-spline Coons patch. Upstream's patch averages the corners' twists and comes out
//! 38.5 degrees off its neighbours (the fork's record: 14.7 degrees off the convex round
//! and 38 off the side face). With `corner_blend_coons_patch_tangent_to_sides` every seam
//! of the patch is within 1 degree, as the fork's test requires.
//!
//! The fork's test measures the angle between the binormals of the two faces at 31
//! stations inside each of the patch's edges; this one compares the faces' normals there.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::math::Point3;
use solidcore::nurbs::knots::Side;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::surface_closest_point;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::face::p2 as face;
use solidcore::topology::face::p4 as face_topology;
use solidcore::topology::{edge, vertex, EdgeId};
use solidcore::{Brep, Kernel};

/// The edge of `b` passing nearest `p` (over 2001 points along each).
fn edge_through(k: &Kernel, b: &Brep, p: Point3) -> EdgeId {
    let distance = |e: EdgeId| {
        let c = edge::curve(b, e).unwrap().unwrap();
        let iv = edge::interval(b, e).unwrap();
        (0..=2000)
            .map(|i| {
                c.evaluate_point(k, iv.evaluate(f64::from(i) / 2000.0))
                    .unwrap()
                    .distance(p)
            })
            .fold(f64::INFINITY, f64::min)
    };
    let edges: Vec<EdgeId> = b.edges().ids().collect();
    *edges
        .iter()
        .min_by(|&&x, &&y| distance(x).total_cmp(&distance(y)))
        .unwrap()
}

/// The B-spline faces of `b` whose vertices all lie within `reach` of `corner`, and the
/// largest angle (degrees) between such a face and its neighbour at 31 stations inside
/// each of its edges.
fn corner_patch_seams(k: &Kernel, b: &Brep, corner: Point3, reach: f64) -> (usize, f64) {
    let normal = |s: &Surface, p: Point3| {
        let uv = surface_closest_point(k, s, p).unwrap().uvs[0];
        s.evaluate_normal(k, uv, Side::Left, Side::Left).unwrap()
    };
    let (mut patches, mut worst) = (0, 0.0_f64);
    for f in b.faces().ids() {
        let Some(s) = face::surface(b, f).unwrap() else {
            continue;
        };
        if !matches!(s.kind(), SurfaceKind::BSpline(_)) {
            continue;
        }
        let vertices = face_topology::vertices(b, f, None, false, None).unwrap();
        if vertices.is_empty()
            || !vertices
                .iter()
                .all(|&v| vertex::point(b, v).unwrap().distance(corner) <= reach)
        {
            continue;
        }
        patches += 1;
        for e in face_topology::edges(b, f, None, false, None).unwrap() {
            let faces = edge::faces(b, e, None).unwrap();
            let Some(&other) = faces.iter().find(|&&g| g != f) else {
                continue;
            };
            let s2 = face::surface(b, other).unwrap().unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            let iv = edge::interval(b, e).unwrap();
            for i in 1..32 {
                let p = c
                    .evaluate_point(k, iv.evaluate(f64::from(i) / 32.0))
                    .unwrap();
                let cos = normal(s, p).dot(normal(s2, p)).abs().min(1.0);
                worst = worst.max(cos.acos().to_degrees());
            }
        }
    }
    (patches, worst)
}

/// The L-shaped box's notch floor edges rounded `radius`, with WR-547's `fixed`: the
/// patches at the corner and their worst seam.
fn rounded_notch(fixed: bool, radius: f64) -> (usize, f64) {
    let mut k = Kernel::default();
    k.fixes.corner_blend_coons_patch_tangent_to_sides = fixed;
    let part = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 20.0).unwrap();
    let block = create_box(&mut k, Point3::new(40.0, 20.0, 10.0), 30.0, 30.0, 20.0).unwrap();
    let mut b = boolean_difference(&mut k, part, block).unwrap();
    let edges = [
        edge_through(&k, &b, Point3::new(50.0, 20.0, 10.0)),
        edge_through(&k, &b, Point3::new(60.0, 30.0, 10.0)),
    ];
    {
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &edges, radius, 1, 1, 1.0)
            .unwrap();
        session.do_fillet(&mut k).unwrap();
    }
    corner_patch_seams(&k, &b, Point3::new(60.0, 20.0, 10.0), 2.0 * radius)
}

#[test]
fn fork_cae8618_l_box_corner_patch_is_tangent_with_the_fix() {
    for radius in [2.0, 5.0] {
        let (patches, worst) = rounded_notch(true, radius);
        assert_eq!(patches, 1, "r {radius}");
        assert!(worst < 1.0, "r {radius}: a seam {worst} degrees off");
    }
}

/// Upstream's mean-twist patch: 38.5 degrees off at both radii.
#[test]
fn fork_cae8618_l_box_corner_patch_is_not_tangent_by_default() {
    for radius in [2.0, 5.0] {
        let (patches, worst) = rounded_notch(false, radius);
        assert_eq!(patches, 1, "r {radius}");
        assert!(worst > 10.0, "r {radius}: {worst} degrees");
    }
}
