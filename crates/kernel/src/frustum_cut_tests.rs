use super::*;
use std::f64::consts::PI;

fn frustum() -> Body {
    let region = Region2 { outer: Loop2::circle(Vec2::ZERO, 1.), holes: vec![] };
    crate::extrude_tapered(&Plane::XY, &region, 10., 1., 0.4_f64.atan()).unwrap()
}

fn cutter(radius: f64, lo: f64, hi: f64) -> Body {
    crate::extrude(&Plane::XY, &[Region2 { outer: Loop2::circle(Vec2::ZERO, radius), holes: vec![] }], lo, hi).unwrap().pop().unwrap()
}

#[test]
fn inward_face_cut_trims_the_cone_at_its_exact_depth() {
    let a = frustum();
    for (lo, hi) in [(5.6, 10.), (5.6, 10.1), (2., 8.)] {
        let b = cutter(5., lo, hi);
        assert!(wider_tool(&a, &b).is_some());
        let (r, report) = crate::capture_boolean_diagnostics(|| crate::boolean(&a, &b, BoolOp::Cut));
        let r = r.unwrap().unwrap();
        assert!(report.events.iter().all(|e| !e.strategy.alters_result()), "{report:?}");
        assert!(r.validity().is_empty(), "{:?}", r.validity());
        // Integral of pi * (1 + 0.4 z)^2 dz over the retained heights.
        let volume_to = |z: f64| PI * (z + 0.4 * z * z + 0.16 / 3. * z * z * z);
        let want = volume_to(lo) + if hi < 10. { volume_to(10.) - volume_to(hi) } else { 0. };
        let m = r.tessellate(1e-5).unwrap();
        let got = m.measure().volume;
        assert!((got - want).abs() < 0.01, "{got} vs {want}");
        assert!(m.contains(Vec3::new(0., 0., lo - 0.1)));
        assert!(!m.contains(Vec3::new(0., 0., lo + 0.1)));
        if hi >= 10. {
            assert!((m.bounds().max.z - lo).abs() < 1e-8);
            assert_eq!(r.shell_count(), 1);
        } else {
            assert_eq!(r.shell_count(), 2);
            assert!(m.contains(Vec3::new(0., 0., 9.)));
        }
        let h = r.history().unwrap();
        h.check(&r, &[&a, &b]).unwrap();
        assert!(h.faces.iter().all(|f| !f.origins.is_empty()));
    }
}

#[test]
fn enclosure_is_certified_and_does_not_accept_seam_vertices_alone() {
    let a = frustum();
    assert!(wider_tool(&a, &cutter(4.99, 5.6, 10.)).is_none());
    let b = cutter(5., 5.6, 10.);
    let offset = crate::transform(&b, Vec3::new(0.01, 0., 0.), Vec3::ZERO, Vec3::Z, 0.).unwrap();
    assert!(wider_tool(&a, &offset).is_none());
    let scaled = crate::transform_matrix(&a, [[1.1, 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]]).unwrap();
    assert!(wider_tool(&scaled, &b).is_none());
    assert!(wider_tool(&crate::sphere(Vec3::ZERO, 5.).unwrap(), &b).is_none());
}

#[test]
fn transformed_cutter_metadata_does_not_certify_a_cylinder() {
    let a = frustum();
    let b = cutter(5., 5.6, 10.);
    for matrix in [
        [[0.5, 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]],
        [[0.5, 0., 0., 0.], [0., 0.5, 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]],
        [[1., 0., 0., 0.], [0., 1., 0., 0.], [0.1, 0., 1., 0.], [0., 0., 0., 1.]],
    ] {
        let changed = crate::transform_matrix(&b, matrix).unwrap();
        assert!(changed.face_sources().is_some(), "transforms retain the original extrusion metadata");
        assert!(wider_tool(&a, &changed).is_none(), "the actual cutter must be a coaxial circle extrusion");
    }
}

#[test]
fn enclosing_cut_follows_a_rigidly_placed_frustum() {
    let place =
        |body: &Body| crate::transform(body, Vec3::new(-6.7692028035233704, 15.405295625108298, 3.), Vec3::ZERO, Vec3::new(1., 2., 3.), 0.7).unwrap();
    let (a, b) = (place(&frustum()), place(&cutter(5., 5.6, 10.)));
    assert!(wider_tool(&a, &b).is_some());
    let r = crate::boolean(&a, &b, BoolOp::Cut).unwrap().unwrap();
    let want = PI * (5.6 + 0.4 * 5.6_f64.powi(2) + 0.16 / 3. * 5.6_f64.powi(3));
    assert!((r.tessellate(1e-5).unwrap().measure().volume - want).abs() < 0.01);
    r.history().unwrap().check(&r, &[&a, &b]).unwrap();
}

#[test]
fn circular_end_rows_do_not_certify_a_surface_bulging_between_them() {
    let w = 0.5_f64.sqrt();
    let net = [(1., 0., 1.), (1., 1., w), (0., 1., 1.)]
        .into_iter()
        .map(|(x, y, weight)| {
            [(1., 0.), (6., 5.), (5., 10.)].into_iter().map(|(r, z)| mt::Vector4::new(x * r * weight, y * r * weight, z * weight, weight)).collect()
        })
        .collect();
    let surface = mt::NurbsSurface::new(mt::BSplineSurface::try_new((mt::KnotVec::bezier_knot(2), mt::KnotVec::bezier_knot(2)), net).unwrap());
    assert!(surface_bound(&surface, Plane::XY, 5., 1e-9, &mut 2_000_000).is_none());
}
