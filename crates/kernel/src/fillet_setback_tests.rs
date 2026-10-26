use solidcraft_geom::Vec3;

use crate::*;

/// The 60 x 40 x 20 box with r5 rounds on the three edges at (60, 40, 20).
fn corner_case(setback: Option<f64>) -> Result<Body> {
    let b = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(60.0, 40.0, 20.0)).unwrap();
    let edges = [Vec3::new(30.0, 40.0, 20.0), Vec3::new(60.0, 20.0, 20.0), Vec3::new(60.0, 40.0, 10.0)];
    match setback {
        Some(s) => fillet_setback(&b, &edges, 5.0, &[(Vec3::new(60.0, 40.0, 20.0), s)]),
        None => fillet(&b, &edges, 5.0),
    }
}

/// Set back 8 along each edge: outside the corner cube [52, 60] x [32, 40] x [12, 20] the body
/// is the sphere corner's, and inside it the convex body holds the hull of its points on the
/// cube's faces (299.0201) and lies within the three rounds' prisms (512 - 225 (1 - pi/4) -
/// 125 (sqrt 2 - 1)); see capi/tests/ops_kernel.cpp, corner_setback. The patch is SMLib's two
/// quadrilaterals per side: 15 faces, 36 edges, 23 vertices; the box stays the box.
#[test]
fn a_corner_set_back_8_blends_through_a_larger_patch() {
    let pi = std::f64::consts::PI;
    let sphere = measure(&corner_case(None).unwrap()).unwrap().volume;
    let out = corner_case(Some(8.0)).unwrap();
    assert!(out.validity().is_empty(), "{:?}", out.validity());
    let m = measure(&out).unwrap();
    let in_c_sphere = 512.0 - 225.0 * (1.0 - pi / 4.0) - 125.0 * (1.0 - pi / 6.0);
    let prisms = 512.0 - 225.0 * (1.0 - pi / 4.0) - 125.0 * (2f64.sqrt() - 1.0);
    let outside = sphere - in_c_sphere;
    assert!(m.volume > outside + 299.0201 && m.volume < outside + prisms, "{} not in [{}, {}]", m.volume, outside + 299.0201, outside + prisms);
    assert_eq!((m.faces, m.edges, m.vertices), (15, 36, 23));
    assert!((m.bbox.min - Vec3::new(0.0, 0.0, 0.0)).len() < 1e-6 && (m.bbox.max - Vec3::new(60.0, 40.0, 20.0)).len() < 1e-6, "{:?}", m.bbox);
}

/// A setback point at no vertex, at a corner where no edge is rounded, or as long as an edge
/// there, is refused.
#[test]
fn a_setback_off_the_rounded_corner_is_refused() {
    let b = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(60.0, 40.0, 20.0)).unwrap();
    let edges = [Vec3::new(30.0, 40.0, 20.0), Vec3::new(60.0, 20.0, 20.0), Vec3::new(60.0, 40.0, 10.0)];
    assert!(fillet_setback(&b, &edges, 5.0, &[(Vec3::new(30.0, 20.0, 10.0), 8.0)]).is_err());
    assert!(fillet_setback(&b, &edges, 5.0, &[(Vec3::new(0.0, 0.0, 0.0), 8.0)]).is_err());
    assert!(matches!(corner_case(Some(20.0)), Err(KernelError::TooLarge(_))));
}
