use super::*;

fn wedge() -> Body {
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2};
    let outer = Loop2::polygon(&[Vec2::new(0., 0.), Vec2::new(40., 0.), Vec2::new(0., 20.)]);
    let body = crate::extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], 0., 15.).unwrap().pop().unwrap();
    let at = Vec3::new(20., 0., 15.);
    // The room's taper, closing to zero at the wedge's tip, given as a spline fillet: R16 itself
    // is built over the wedge's cliffs (the blend-overflow decision), so a tapered fillet no
    // longer tapers it.
    let t = taper(&body, at, 16.).unwrap();
    fillet_variable_spline(&body, at, t.start, &t.spline).unwrap()
}

/// The display mesh of a fillet tapering to zero at a wedge's tip converges to the B-rep: each
/// mesh lies within its chord tolerance of the faces, so its volume is within the body's area
/// times that tolerance of the exact one (SMLib's, integrated on the surfaces, to 1e-8). Only
/// the mesher is under test here; the taper's own law is `taper_follows_the_room_along_the_edge`'s.
#[test]
fn collapsed_taper_mesh_stays_accurate_as_tolerance_decreases() {
    let body = wedge();
    let exact = crate::measure(&body).unwrap();
    for tol in [0.002, 0.001, 0.0005, 0.0002] {
        let mesh = body.mesh(&crate::MeshTolerance::chord(tol)).unwrap();
        assert!(mesh.positions.iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()));
        let got = mesh.measure().volume;
        let bound = exact.area * tol;
        assert!((got - exact.volume).abs() <= bound, "tol={tol}: mesh {got} vs exact {} (within {bound})", exact.volume);
    }
}
