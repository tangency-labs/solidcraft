//! #1038: a small hole on a large curved face cuts cleanly, its rim on the face an intersection
//! edge.

use solidcraft_geom::brep::Curve;
use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

use crate::{Body, BoolOp, build::extrude, cylinder, ops::boolean};

/// A 200 × 200 block whose top is one cubic wave across x (a single B-spline face), with a
/// Ø1 mm hole through it at the middle.
fn holed() -> (Body, Body) {
    let plane = Plane::new(Vec3::new(0.0, 200.0, 0.0), Vec3::X, Vec3::Z).unwrap();
    let (a, b, c, d) = (Vec2::new(0.0, 0.0), Vec2::new(200.0, 0.0), Vec2::new(200.0, 40.0), Vec2::new(0.0, 40.0));
    let top = Seg2::Cubic { p0: c, p1: Vec2::new(133.0, 70.0), p2: Vec2::new(67.0, 10.0), p3: d };
    let outer = Loop2 { segs: vec![Seg2::Line { a, b }, Seg2::Line { a: b, b: c }, top, Seg2::Line { a: d, b: a }] };
    let block = extrude(&plane, &[Region2 { outer, holes: vec![] }], 0.0, 200.0).unwrap().pop().unwrap();
    let tool = cylinder(Vec3::new(100.0, 100.0, -10.0), Vec3::Z, 0.5, 100.0).unwrap();
    let cut = boolean(&block, &tool, BoolOp::Cut).unwrap().unwrap();
    (block, cut)
}

/// The edges that are intersection curves: neither lines nor conics (a hole's rim on a spline
/// face is a spline).
fn intersection_curves(body: &Body) -> Vec<Curve> {
    body.record()
        .edges
        .iter()
        .map(|e| e.curve.clone())
        .filter(|c| matches!(c, Curve::Spline { .. } | Curve::RationalSpline { .. } | Curve::SurfaceIntersection { .. }))
        .collect()
}

#[test]
fn a_small_hole_in_a_large_spline_face_cuts_cleanly() {
    let (block, cut) = holed();
    assert!(cut.validity().is_empty(), "{:?}", cut.validity());
    // The hole adds its wall (one cylinder, closed along its seam) and splits nothing: the
    // bottom and the wave each gain a loop.
    assert_eq!(cut.face_count(), block.face_count() + 1);
    let curves = intersection_curves(&cut);
    assert!(!curves.is_empty(), "the hole's rim on the wave is an intersection edge");
    // The wave is within a few tenths of a millimetre of flat across a Ø1 hole, so the hole
    // removes a cylinder as tall as the wave at its axis (the cubic's middle: z = 40).
    let (v0, v1) = (block.volume_at(0.001).unwrap(), cut.volume_at(0.001).unwrap());
    let removed = std::f64::consts::PI * 0.25 * 40.0;
    assert!((v0 - v1 - removed).abs() < 0.05 * removed, "removed {} of {removed}", v0 - v1);
}
