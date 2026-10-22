//! A lip swept along the rim of a round body joins it, flush with its side or set in from it:
//! the lip's foot lies on the body's top, and its sides are round where the rim is.

use std::f64::consts::{FRAC_PI_2, PI};

use solidcraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};

use crate::*;

/// A 1 × 1 mm lip on the top (z = 10) of a round body of radius 10 about Z, its outer side
/// `inset` in from the body's side, swept a quarter turn from the XZ plane.
fn lip(inset: f64) -> Body {
    let plane = Plane::new(Vec3::ZERO, Vec3::X, Vec3::Z).unwrap();
    let (x0, x1) = (9.0 - inset, 10.0 - inset);
    let outer = Loop2::polygon(&[Vec2::new(x0, 10.0), Vec2::new(x1, 10.0), Vec2::new(x1, 11.0), Vec2::new(x0, 11.0)]);
    let path = [PathSeg::Arc { a: Vec3::new(10.0, 0.0, 10.0), center: Vec3::new(0.0, 0.0, 10.0), axis: Vec3::Z, angle: FRAC_PI_2 }];
    sweep(&plane, &Region2 { outer, holes: vec![] }, &path).unwrap()
}

#[test]
fn a_lip_swept_on_a_round_rim_joins_it() {
    let body = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 10.0).unwrap();
    for inset in [0.0, 0.5] {
        let (r0, r1) = (9.0 - inset, 10.0 - inset);
        // Measured as the result is (the round faces' volume from their meshes).
        let mesh_volume = |b: &Body| b.mesh(&crate::MeshTolerance::chord(1e-5)).unwrap().measure().volume;
        let want = mesh_volume(&body) + mesh_volume(&lip(inset));
        assert!((want - PI * 1000.0 - PI * (r1 * r1 - r0 * r0) / 4.0).abs() < 0.05);
        for (a, b) in [(&body, &lip(inset)), (&lip(inset), &body)] {
            let r = boolean(a, b, BoolOp::Union).unwrap().unwrap();
            assert_eq!(r.shell_count(), 1, "inset {inset}: one solid");
            assert!(r.validity().is_empty(), "inset {inset}: {:?}", r.validity());
            let v = mesh_volume(&r);
            assert!((v - want).abs() < 1e-3, "inset {inset}: volume {v} vs {want}");
        }
    }
}

/// The swept lip's foot over the turn is a plane, so it is found lying on the body's top.
#[test]
fn a_swept_lips_foot_is_a_plane() {
    let l = lip(0.0);
    // The faces lying in the plane z = 10 facing down: planar, normal -z, through z = 10.
    let foot: Vec<usize> = (0..l.face_count())
        .filter(|&i| l.planar_face(i).is_some_and(|p| p.normal().dot(-Vec3::Z) > 1.0 - 1e-9 && (p.origin.z - 10.0).abs() < 1e-6))
        .collect();
    let m = l.mesh(&crate::MeshTolerance::chord(1e-5)).unwrap();
    let area: f64 = m
        .triangles
        .iter()
        .zip(&m.tri_face)
        .filter(|(_, f)| foot.contains(&(**f as usize)))
        .filter_map(|(t, _)| m.tri(t))
        .map(|[a, b, c]| (b - a).cross(c - a).len() / 2.0)
        .sum();
    assert!((area - PI * (100.0 - 81.0) / 4.0).abs() < 1e-3, "{} faces, {area} mm²", foot.len());
}
