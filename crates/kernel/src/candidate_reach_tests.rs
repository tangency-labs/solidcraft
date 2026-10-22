//! A boolean candidate with a trim loop that runs off its surface is discarded fast (#3020).
//!
//! In the design of #3020 a coarse candidate of a nudged union came out with a face on a cone
//! (a revolved line: its profile parameter bounded to [0, 1], its angle periodic) whose trim
//! loop left the cone and ran thousands of profile lengths along its continuation, kilometres
//! from the part. Meshing that face to measure the candidate never finished. Here such a face
//! is made on purpose: the check finds its edge outside the operands without meshing it.

use std::time::{Duration, Instant};

use solidcraft_geom::Vec3;
use truck_modeling::{self as mt, ParametricSurface, builder};

use super::{MARGIN, Reach};
use crate::body::Body;

/// The cone of the report: a profile line 2 mm out and 4 mm up from radius 20 mm (its length,
/// √20 mm, is the |∂S/∂u| measured on the runaway face), turned about Z.
fn cone() -> mt::Surface {
    let profile = mt::Curve::Line(mt::Line(mt::Point3::new(20.0, 0.0, 0.0), mt::Point3::new(22.0, 0.0, 4.0)));
    mt::Surface::RevolutedCurve(mt::Processor::new(mt::RevolutedCurve::by_revolution(profile, mt::Point3::new(0.0, 0.0, 0.0), mt::Vector3::unit_z())))
}

/// A solid of one face of `surface` bounded by straight edges through `corners`, given in the
/// surface's parameters and moved `off` mm off it along its normal.
fn face_body(surface: &mt::Surface, corners: &[(f64, f64, f64)]) -> Body {
    use mt::ParametricSurface3D;
    let vertices: Vec<mt::Vertex> = corners.iter().map(|&(u, v, off)| builder::vertex(surface.subs(u, v) + surface.normal(u, v) * off)).collect();
    let edges: Vec<mt::Edge> = (0..vertices.len()).map(|i| builder::line(&vertices[i], &vertices[(i + 1) % vertices.len()])).collect();
    let face = mt::Face::new(vec![edges.into_iter().collect()], surface.clone());
    let solid = mt::Solid::new_unchecked(vec![vec![face].into()]);
    Body::new(solid).unwrap()
}

/// The operands: a plate and the cone's own patch, which spans u in [0, 1] (radius 20-22 mm,
/// 4 mm high) over a quarter turn.
fn operands() -> (Body, Body) {
    let plate = crate::box_solid(Vec3::new(-30.0, -30.0, -6.0), Vec3::new(30.0, 30.0, 0.0)).unwrap();
    let patch = face_body(&cone(), &[(0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (1.0, 1.5, 0.0), (0.0, 1.5, 0.0)]);
    (plate, patch)
}

fn reach(a: &Body, b: &Body) -> Reach {
    let size = a.size().max(b.size());
    let meshes = (a.mesh(&crate::MeshTolerance::chord(size * 5e-4)).unwrap(), b.mesh(&crate::MeshTolerance::chord(size * 5e-4)).unwrap());
    Reach::new(a, b, (Some(&meshes.0), Some(&meshes.1)), size).unwrap()
}

/// The runaway face: on the cone, its loop leaves the patch at u = 0 and follows the cone's
/// continuation out to u = -2450 (as the logged loop did, -2452 … 1349), 1.5 mm off it, and
/// back.
fn runaway() -> Body {
    face_body(&cone(), &[(1.0, 0.2, 0.0), (1.0, 1.3, 0.0), (0.0, 1.3, 0.0), (-2450.0, 0.9, 1.5), (-2450.0, 0.6, 1.5), (0.0, 0.2, 0.0)])
}

#[test]
fn a_candidate_with_a_trim_loop_off_its_surface_is_rejected_fast() {
    let (a, b) = operands();
    let reach = reach(&a, &b);
    let candidate = runaway();
    let start = Instant::now();
    let reason = reach.runaway(&candidate);
    let took = start.elapsed();
    let reason = reason.expect("an edge kilometres outside the operands");
    assert!(reason.contains("outside the operands"), "{reason}");
    assert!(took < Duration::from_secs(1), "{took:?}");
}

#[test]
fn sound_candidates_stay_within_reach() {
    let (a, b) = operands();
    let reach = reach(&a, &b);
    // The operands themselves, and a union of the plate with a cylinder standing on it.
    assert_eq!(reach.runaway(&a), None);
    assert_eq!(reach.runaway(&b), None);
    let post = crate::cylinder(Vec3::new(0.0, 0.0, -1.0), Vec3::Z, 5.0, 20.0).unwrap();
    let reach = self::reach(&a, &post);
    let union = crate::boolean(&a, &post, crate::BoolOp::Union).unwrap().unwrap();
    assert_eq!(reach.runaway(&union), None);
}

#[test]
fn an_edge_just_past_the_margin_is_out_of_reach() {
    let (a, b) = operands();
    let size = a.size().max(b.size());
    let reach = reach(&a, &b);
    // A box reaching past the plate's side by twice the margin, and one within it.
    let top = Vec3::new(30.0, 30.0, 0.0);
    let past = crate::box_solid(Vec3::new(0.0, 0.0, -6.0), top + Vec3::X * (2.0 * MARGIN * size)).unwrap();
    let within = crate::box_solid(Vec3::new(0.0, 0.0, -6.0), top + Vec3::X * (0.5 * MARGIN * size)).unwrap();
    assert!(reach.runaway(&past).is_some());
    assert_eq!(reach.runaway(&within), None);
}

/// The bound on a loop's span is in lengths of a bounded parameter's range: a plane's unit box
/// is only a chart, so a plate thousands of millimetres across (its loops thousands of units
/// wide) still meshes, as does a long cylinder.
#[test]
fn large_faces_still_mesh() {
    let plate = crate::box_solid(Vec3::new(-2500.0, -2500.0, 0.0), Vec3::new(2500.0, 2500.0, 3.0)).unwrap();
    let rod = crate::cylinder(Vec3::ZERO, Vec3::Z, 0.5, 3000.0).unwrap();
    for body in [plate, rod] {
        assert!(!body.mesh(&crate::MeshTolerance::chord(0.05)).unwrap().triangles.is_empty());
        assert_eq!(body.unmeshed_faces(), 0);
    }
}
