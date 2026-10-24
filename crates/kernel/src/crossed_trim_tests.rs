//! A face whose trimming loop crosses itself is meshed as the region the loop winds around. A
//! boolean makes such a face when its cut runs along an edge of a face, a few micrometres outside
//! the edge for most of its length and inside it near one end: the sliver between them has a loop
//! that winds clockwise for most of its area. The mesher took a face without a counterclockwise
//! loop for a surface with holes only and meshed the whole surface around it, many times the
//! sliver's area; the volume of the shared part of two bodies read 6 % too high. A loop that winds
//! clockwise without crossing itself is still a hole in the surface around it.

use truck_meshalgo::prelude::*;
use truck_modeling as mt;

use crate::body::p3;
use solidcraft_geom::Vec3;

/// A flat B-spline patch over [0, 10] × [0, 10] in z = 0 (its parameters times 10 are x and y).
fn patch() -> mt::Surface {
    let k = mt::KnotVec::bezier_knot(1);
    let rows =
        vec![vec![p3(Vec3::new(0.0, 0.0, 0.0)), p3(Vec3::new(0.0, 10.0, 0.0))], vec![p3(Vec3::new(10.0, 0.0, 0.0)), p3(Vec3::new(10.0, 10.0, 0.0))]];
    mt::Surface::BSplineSurface(mt::BSplineSurface::new((k.clone(), k), rows))
}

/// The area of the mesh of a face of `surface` bounded by the closed polygon `corners`, each
/// side in 20 edges (as a boolean's cut is a polyline of many points).
fn meshed_area(surface: mt::Surface, corners: &[Vec3]) -> f64 {
    let n = corners.len();
    let points = (0..n).flat_map(|i| (0..20).map(move |k| corners[i] + (corners[(i + 1) % n] - corners[i]) * (k as f64 / 20.0)));
    let v: Vec<mt::Vertex> = points.map(|c| mt::builder::vertex(p3(c))).collect();
    let wire: mt::Wire = (0..v.len()).map(|i| mt::builder::line(&v[i], &v[(i + 1) % v.len()])).collect();
    let shell: mt::Shell = vec![mt::Face::new_unchecked(vec![wire], surface)].into();
    let meshed = shell.robust_triangulation(0.01);
    let mesh = meshed.face_iter().next().and_then(|f| f.surface()).expect("the face is meshed");
    let pos = mesh.positions();
    mesh.faces().triangle_iter().map(|t| (pos[t[1].pos] - pos[t[0].pos]).cross(pos[t[2].pos] - pos[t[0].pos]).magnitude() / 2.0).sum()
}

/// The face's edge runs along x = 9 from y = 2 to the patch's side at y = 10; the cut comes
/// back from (8.8, 10), crossing the edge at y = 10 − 5 × 0.2 / 0.7 and running outside it
/// through (9.5, 5). The loop winds counterclockwise only around the triangle above the
/// crossing, 0.2 wide and 10 − y tall; below it, clockwise around a strip of 1.64 mm².
#[test]
fn a_loop_crossing_itself_meshes_what_it_winds_around() {
    let loop_ = [Vec3::new(9.0, 2.0, 0.0), Vec3::new(9.0, 10.0, 0.0), Vec3::new(8.8, 10.0, 0.0), Vec3::new(9.5, 5.0, 0.0)];
    let crossing = 10.0 - 5.0 * 0.2 / 0.7;
    let want = 0.5 * 0.2 * (10.0 - crossing);
    let got = meshed_area(patch(), &loop_);
    // Not the patch's 100 mm² less the strip.
    assert!((got - want).abs() < 0.02 * want, "{got} mm² against {want}");
}

/// A face whose loops all wind counterclockwise is meshed as before: inside its loop.
#[test]
fn a_counterclockwise_loop_meshes_its_inside() {
    let square = [Vec3::new(2.0, 2.0, 0.0), Vec3::new(6.0, 2.0, 0.0), Vec3::new(6.0, 5.0, 0.0), Vec3::new(2.0, 5.0, 0.0)];
    let got = meshed_area(patch(), &square);
    assert!((got - 12.0).abs() < 1e-9, "{got}");
}

/// A face whose one loop winds clockwise without crossing itself is a hole: the patch around it.
#[test]
fn a_clockwise_loop_is_a_hole_in_the_patch() {
    let square = [Vec3::new(2.0, 2.0, 0.0), Vec3::new(2.0, 5.0, 0.0), Vec3::new(6.0, 5.0, 0.0), Vec3::new(6.0, 2.0, 0.0)];
    let got = meshed_area(patch(), &square);
    assert!((got - 88.0).abs() < 1e-9, "{got}");
}

/// On a sphere a face bounded by one loop is the cap inside the loop when the loop winds
/// counterclockwise, and the sphere around it when the loop winds clockwise, a hole.
#[test]
fn a_hole_alone_in_a_sphere_leaves_the_sphere_around_it() {
    let (r, d) = (10.0f64, 6.0f64);
    let pole = |z: f64| mt::builder::vertex(p3(Vec3::new(0.0, 0.0, z)));
    let meridian = mt::builder::circle_arc(&pole(r), &pole(-r), p3(Vec3::new(r, 0.0, 0.0)));
    let sphere = mt::Surface::RevolutedCurve(mt::Processor::new(mt::RevolutedCurve::by_revolution(
        meridian.oriented_curve(),
        p3(Vec3::ZERO),
        mt::Vector3::unit_z(),
    )));
    // The circle where the plane y = d cuts the sphere, in two halves.
    let c = (r * r - d * d).sqrt();
    let (a, b) = (mt::builder::vertex(p3(Vec3::new(c, d, 0.0))), mt::builder::vertex(p3(Vec3::new(-c, d, 0.0))));
    let one_way: mt::Wire =
        vec![mt::builder::circle_arc(&a, &b, p3(Vec3::new(0.0, d, c))), mt::builder::circle_arc(&b, &a, p3(Vec3::new(0.0, d, -c)))].into();
    let area = |wire: mt::Wire| {
        let shell: mt::Shell = vec![mt::Face::new_unchecked(vec![wire], sphere.clone())].into();
        let meshed = shell.robust_triangulation(0.001);
        let mesh = meshed.face_iter().next().and_then(|f| f.surface()).expect("the face is meshed");
        let pos = mesh.positions();
        mesh.faces().triangle_iter().map(|t| (pos[t[1].pos] - pos[t[0].pos]).cross(pos[t[2].pos] - pos[t[0].pos]).magnitude() / 2.0).sum::<f64>()
    };
    let cap = 2.0 * std::f64::consts::PI * r * (r - d);
    let sphere_area = 4.0 * std::f64::consts::PI * r * r;
    let mut got = [area(one_way.clone()), area(one_way.inverse())];
    got.sort_by(f64::total_cmp);
    assert!(
        (got[0] - cap).abs() < 1e-3 * cap && (got[1] - (sphere_area - cap)).abs() < 1e-3 * sphere_area,
        "{got:?}: {cap} and {}",
        sphere_area - cap
    );
}
