//! Production wrapper checks, written but not run; NDF controls are named explicitly.
use super::*;

#[test]
fn isotropic_visible_wrapper_matches_independent_pdf_and_opaque_specular_bsdf() {
    let surface = OpaqueSurface::new(Vec3::new(0.8, 0.5, 0.2), 1.0, 0.3, 1.5).unwrap();
    let v = unit_direction(Vec3::new(0.7, 0.2, 1.0)).unwrap();
    let alpha = 0.3_f64.powi(2);
    let a2 = alpha * alpha;
    let mut accepted = 0;
    for i in 0..64 {
        let u = [(i as f64 + 0.5) / 64.0, 0.37];
        if let Some(sample) = rough_specular(surface, Vec3::Z, v, u).unwrap() {
            accepted += 1;
            let h = unit_direction(v + sample.direction).unwrap();
            let denominator = 1.0 - h.z * h.z + a2 * h.z * h.z;
            let d = a2 / (PI * denominator * denominator);
            let visibility = 2.0 / (v.z + (a2 + (1.0 - a2) * v.z * v.z).sqrt());
            let pdf = d * visibility * 0.25;
            assert!((sample.pdf - pdf).abs() <= 1e-10 * (1.0 + pdf));
            let expected = surface.brdf(Vec3::Z, v, sample.direction).unwrap() * (sample.direction.z / sample.pdf);
            assert!((sample.weight - expected).len() < 1e-10);
        }
    }
    assert!(accepted > 0);
}

#[test]
fn explicit_ndf_control_is_not_the_production_visible_sampler() {
    let s = OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 1.0, 1.0, 1.5).unwrap();
    let v = unit_direction(Vec3::new(1.0, 0.0, 0.2)).unwrap();
    let old = rough_specular_ndf(s, Vec3::Z, v, [0.0, 0.0]).unwrap().unwrap();
    let visible = rough_specular(s, Vec3::Z, v, [0.0, 0.0]).unwrap().unwrap();
    assert!((old.pdf - visible.pdf).abs() > 0.01);
    assert!((old.direction - visible.direction).len() > 0.01);
    // Mirror's explicit delta intent and existing Fresnel law are independent.
    let mirror_sample = mirror(s, Vec3::Z, Vec3::Z).unwrap().unwrap();
    assert_eq!(mirror_sample.direction, Vec3::Z);
    assert_eq!(mirror_sample.pdf, 1.0);
    assert_eq!(mirror_sample.weight, Vec3::new(1.0, 1.0, 1.0));
}

#[test]
fn authored_point_zero_two_vndf_matches_independent_normal_view_density_and_furnace() {
    let old = OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 1.0, 0.045, 1.5).unwrap();
    let finish = old.with_authored_finish(old.base_color(), 1.0, 0.02).unwrap();
    let a2 = 0.02_f64.powi(4);
    let mut sum = 0.0;
    let mut oracle = 0.0;
    for i in 0..4096 {
        let u = (f64::from(i) + 0.5) / 4096.0;
        let denominator = (1.0 - u) + a2 * u;
        let cosine = ((1.0 - u) - a2 * u) / denominator;
        let expected = 2.0 * cosine / (cosine + (a2 + (1.0 - a2) * cosine * cosine).sqrt());
        let sample = rough_specular(finish, Vec3::Z, Vec3::Z, [u, 0.375]).unwrap().unwrap();
        let pdf = denominator * denominator / (std::f64::consts::PI * a2 * 4.0);
        assert!((sample.direction.z - cosine).abs() < 1e-14);
        assert!((sample.pdf - pdf).abs() < 1e-12 * pdf.max(1.0));
        assert!((sample.weight - Vec3::new(expected, expected, expected)).len() < 1e-12);
        sum += sample.weight.x;
        oracle += expected;
    }
    assert!((sum - oracle).abs() < 1e-10);
    assert!((sum / 4096.0 - 1.0).abs() < 1e-8, "independent white-furnace bound at narrow finite GGX roughness");
    assert!(rough_specular(finish, Vec3::Z, Vec3::Z, [1.0 - a2 / 4.0, 0.375]).unwrap().is_none(), "a below-surface reflection retains its null mass");
    // The old constructor remains the production baseline for every old finish;
    // the optical adapter introduces no new arithmetic or variate consumption.
    let base = crate::anisotropic_surface::AnisotropicSurface::new(old.base_color(), 1.0, 0.045, 0.045, 1.5).unwrap();
    let frame = crate::anisotropic_surface::VisualFrame::new(Vec3::Z, -Vec3::Y).unwrap();
    for u in [[0.0, 0.0], [0.125, 0.375], [0.75, 0.5]] {
        let actual = rough_specular(old, Vec3::Z, Vec3::Z, u).unwrap().unwrap();
        let original = base.sample_visible_reflection(frame, Vec3::Z, Vec3::Z, u).unwrap().unwrap();
        assert_eq!((actual.direction, actual.pdf, actual.weight), (original.direction, original.pdf, original.weight));
    }
}
