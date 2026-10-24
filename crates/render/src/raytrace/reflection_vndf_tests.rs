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
