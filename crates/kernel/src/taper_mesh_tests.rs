use super::*;

fn wedge() -> (Body, RadiusSpline) {
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2};
    let outer = Loop2::polygon(&[Vec2::new(0., 0.), Vec2::new(40., 0.), Vec2::new(0., 20.)]);
    let body = crate::extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], 0., 15.).unwrap().pop().unwrap();
    let at = Vec3::new(20., 0., 15.);
    let (filleted, tapers) = fillet_tapered(&body, &[at], 16.).unwrap();
    let taper = tapers.into_iter().flatten().next().unwrap();
    (filleted, taper.spline)
}

fn expected(law: &RadiusSpline) -> f64 {
    let n = 20_000;
    let integral = (0..n).map(|i| law.eval((i as f64 + 0.5) / n as f64).powi(2)).sum::<f64>() / n as f64;
    6000. - (1. - std::f64::consts::FRAC_PI_4) * 40. * integral
}

#[test]
fn collapsed_taper_mesh_stays_accurate_as_tolerance_decreases() {
    let (body, law) = wedge();
    let wanted = expected(&law);
    for tol in [0.002, 0.001, 0.0005, 0.0002] {
        let mesh = body.mesh(&crate::MeshTolerance::chord(tol)).unwrap();
        assert!(mesh.positions.iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()));
        let got = mesh.measure().volume;
        assert!((got - wanted).abs() / wanted < 1e-4, "tol={tol}: {got} vs {wanted}");
    }
}
