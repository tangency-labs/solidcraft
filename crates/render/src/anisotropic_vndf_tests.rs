//! VNDF proposal tests, written but not executed. Legacy NDF assertions stay intact.
use super::*;

fn frame() -> VisualFrame {
    VisualFrame::new(Vec3::Z, Vec3::X).unwrap()
}
fn white(rx: f64, ry: f64) -> AnisotropicSurface {
    AnisotropicSurface::new(Vec3::new(1.0, 1.0, 1.0), 1.0, rx, ry, 1.5).unwrap()
}
fn close(a: f64, b: f64, tolerance: f64) {
    assert!((a - b).abs() <= tolerance * (1.0 + a.abs().max(b.abs())), "{a} != {b}");
}
fn rotate(v: Vec3) -> Vec3 {
    Vec3::new(-v.y, v.x, v.z)
}

#[test]
fn vndf_normal_incidence_has_analytic_mean_second_moment_and_half_null_mass() {
    let surface = white(1.0, 1.0);
    let (mut sum, mut second, mut nulls) = (0.0, 0.0, 0);
    const N: usize = 4096;
    for i in 0..N {
        let u = (i as f64 + 0.5) / N as f64;
        let value = match surface.sample_visible_reflection(frame(), Vec3::Z, Vec3::Z, [u, 0.3]).unwrap() {
            Some(s) => {
                close(s.direction.z, 1.0 - 2.0 * u, 1e-13);
                close(s.pdf, 1.0 / (4.0 * PI), 1e-13);
                close(s.weight.x, 2.0 * s.direction.z / (1.0 + s.direction.z), 1e-13);
                s.weight.x
            }
            None => {
                nulls += 1;
                0.0
            }
        };
        sum += value;
        second += value * value;
    }
    assert_eq!(nulls, N / 2);
    // Independent integrals of (1-2u)/(1-u) and its square on [0,1/2].
    close(sum / N as f64, 1.0 - 2.0_f64.ln(), 1e-6);
    close(second / N as f64, 3.0 - 4.0 * 2.0_f64.ln(), 1e-6);
}

#[test]
fn unit_roughness_visible_cap_has_analytic_grazing_pdf_and_acceptance_mass() {
    const N: usize = 2048;
    let s = white(1.0, 1.0);
    for cosine in [1.0_f64, 0.2, 1e-12] {
        let view = Vec3::new(((1.0 - cosine) * (1.0 + cosine)).sqrt(), 0.0, cosine);
        let mut accepted = 0;
        for i in 0..N {
            let u = (i as f64 + 0.5) / N as f64;
            if let Some(sample) = s.sample_visible_reflection(frame(), Vec3::Z, view, [u, 0.37]).unwrap() {
                accepted += 1;
                close(sample.pdf, 1.0 / (2.0 * PI * (1.0 + cosine)), 1e-10);
                close(sample.weight.x, 2.0 * sample.direction.z / (1.0 + sample.direction.z), 1e-10);
            }
        }
        close(accepted as f64 / N as f64, 1.0 / (1.0 + cosine), 1.0 / N as f64);
    }
}

#[test]
fn anisotropic_visible_samples_match_unchanged_specular_bsdf_and_pdf() {
    let s = AnisotropicSurface::new(Vec3::new(0.8, 0.5, 0.2), 0.4, 0.4, 0.8, 1.5).unwrap();
    let v = unit_direction(Vec3::new(0.2, 0.3, 1.0)).unwrap();
    let (mut accepted, mut nulls) = (0, 0);
    for i in 0..32 {
        for j in 0..32 {
            let uv = [(i as f64 + 0.5) / 32.0, (j as f64 + 0.5) / 32.0];
            match s.sample_visible_reflection(frame(), Vec3::Z, v, uv).unwrap() {
                Some(sample) => {
                    accepted += 1;
                    let pdf = s.visible_reflection_pdf(frame(), Vec3::Z, v, sample.direction).unwrap();
                    close(sample.pdf, pdf, 1e-13);
                    let expected = s.specular_brdf(frame(), Vec3::Z, v, sample.direction).unwrap() * (sample.direction.z / pdf);
                    assert!((sample.weight - expected).len() < 1e-10);
                    assert!([sample.weight.x, sample.weight.y, sample.weight.z].into_iter().all(|w| (0.0..=1.0 + 1e-12).contains(&w)));
                }
                None => nulls += 1,
            }
        }
    }
    assert!(accepted > 0 && nulls > 0);
    assert_eq!(accepted + nulls, 1024);
}

#[test]
fn anisotropic_visible_mapping_is_covariant_under_visual_frame_rotation() {
    let s = white(0.2, 0.8);
    let v = unit_direction(Vec3::new(0.4, 0.2, 1.0)).unwrap();
    let turned = VisualFrame::new(Vec3::Z, Vec3::Y).unwrap();
    for u in [[0.0, 0.0], [0.2, 0.3], [0.8, 0.7], [1.0_f64.next_down(), 0.5]] {
        let a = s.sample_visible_normal(frame(), v, u).unwrap().unwrap();
        let b = s.sample_visible_normal(turned, rotate(v), u).unwrap().unwrap();
        assert!((rotate(a) - b).len() < 1e-12);
        close(s.visible_normal_pdf(frame(), v, a).unwrap(), s.visible_normal_pdf(turned, rotate(v), b).unwrap(), 1e-11);
        let a = s.sample_visible_reflection(frame(), Vec3::Z, v, u).unwrap();
        let b = s.sample_visible_reflection(turned, Vec3::Z, rotate(v), u).unwrap();
        match (a, b) {
            (Some(a), Some(b)) => {
                assert!((rotate(a.direction) - b.direction).len() < 1e-12);
                assert!((a.weight - b.weight).len() < 1e-12);
                close(a.pdf, b.pdf, 1e-11);
            }
            (None, None) => {}
            _ => panic!("rotation changed null support"),
        }
    }
}

#[test]
fn visible_mapping_retains_finite_open_endpoints_and_grazing_limits() {
    for (rx, ry) in [(MIN_ROUGHNESS, 1.0), (1.0, MIN_ROUGHNESS), (MIN_ROUGHNESS, MIN_ROUGHNESS), (1.0, 1.0)] {
        let s = white(rx, ry);
        for view in [Vec3::Z, Vec3::new(1.0, 0.0, 1e-300), Vec3::new(1.0, 0.0, f64::from_bits(1))] {
            for u in [0.0, f64::from_bits(1), 0.5, 1.0_f64.next_down()] {
                for phi in [0.0, 0.5, 1.0_f64.next_down()] {
                    let h = s.sample_visible_normal(frame(), view, [u, phi]).unwrap().unwrap();
                    assert!(h.is_finite() && h.z > 0.0);
                    close(h.len(), 1.0, 1e-13);
                    assert!(s.visible_normal_pdf(frame(), view, h).unwrap().is_finite());
                    if let Some(sample) = s.sample_visible_reflection(frame(), Vec3::Z, view, [u, phi]).unwrap() {
                        assert!(sample.pdf.is_finite() && sample.pdf > 0.0 && sample.weight.is_finite());
                    }
                }
            }
        }
        // This endpoint was an overflowing NDF-density control; VNDF cancels
        // the tiny view cosine analytically instead of clamping that old PDF.
        let sample = s.sample_visible_reflection(frame(), Vec3::Z, Vec3::new(1.0, 0.0, f64::from_bits(1)), [0.0, 0.0]).unwrap().unwrap();
        assert!(sample.pdf.is_finite() && sample.weight.is_finite());
    }
}

#[test]
fn visible_density_has_correct_support_and_rejects_hostile_variates() {
    let s = white(0.4, 0.8);
    for bad in [-0.1, 1.0, f64::NAN, f64::INFINITY] {
        assert!(s.sample_visible_normal(frame(), Vec3::Z, [bad, 0.2]).is_err());
        assert!(s.sample_visible_reflection(frame(), Vec3::Z, Vec3::Z, [0.2, bad]).is_err());
    }
    assert!(s.sample_visible_normal(frame(), Vec3::ZERO, [0.2, 0.2]).is_err());
    assert_eq!(s.visible_normal_pdf(frame(), Vec3::Z, -Vec3::Z).unwrap(), 0.0);
    assert_eq!(s.visible_normal_pdf(frame(), Vec3::Z, Vec3::X).unwrap(), 0.0);
    assert_eq!(s.visible_normal_pdf(frame(), Vec3::new(1.0, 0.0, 0.01), Vec3::new(-1.0, 0.0, 0.1)).unwrap(), 0.0);
    assert_eq!(s.visible_reflection_pdf(frame(), Vec3::Z, Vec3::Z, -Vec3::Z).unwrap(), 0.0);
    assert!(s.sample_visible_reflection(frame(), Vec3::Z, -Vec3::Z, [0.2, 0.2]).unwrap().is_none());
    let tilted = VisualFrame::new(Vec3::new(0.8, 0.0, 0.6), Vec3::Y).unwrap();
    assert_eq!(s.visible_reflection_pdf(tilted, Vec3::Z, Vec3::Z, Vec3::new(0.99, 0.0, -0.1)).unwrap(), 0.0);
}

#[test]
fn unit_roughness_visible_normal_density_integrates_to_one() {
    const N: usize = 256;
    let s = white(1.0, 1.0);
    for view in [Vec3::Z, unit_direction(Vec3::new(1.0, 0.0, 0.2)).unwrap()] {
        let mut mass = 0.0;
        for i in 0..N {
            let z = (i as f64 + 0.5) / N as f64;
            let radius = ((1.0 - z) * (1.0 + z)).sqrt();
            for j in 0..N {
                let phi = TAU * (j as f64 + 0.5) / N as f64;
                mass += s.visible_normal_pdf(frame(), view, Vec3::new(radius * phi.cos(), radius * phi.sin(), z)).unwrap();
            }
        }
        close(mass * TAU / (N * N) as f64, 1.0, 0.002);
    }
}

fn oblique_rotation(v: Vec3) -> Vec3 {
    // Original Rodrigues construction about an axis with all three components.
    let axis = unit_direction(Vec3::new(1.0, 2.0, 3.0)).unwrap();
    let (sine, cosine) = 0.71_f64.sin_cos();
    v * cosine + axis.cross(v) * sine + axis * (axis.dot(v) * (1.0 - cosine))
}

#[test]
fn visible_samples_and_densities_follow_an_arbitrarily_tilted_visual_frame() {
    let s = white(0.65, 0.9);
    let tilted = VisualFrame::new(oblique_rotation(Vec3::Z), oblique_rotation(Vec3::X)).unwrap();
    let v = unit_direction(Vec3::new(0.3, -0.2, 1.0)).unwrap();
    let geometric = unit_direction(Vec3::new(0.1, 0.0, 1.0)).unwrap();
    for uv in [[0.0, 0.0], [0.1, 0.3], [0.5, 0.7], [0.9, 0.2]] {
        let h = s.sample_visible_normal(frame(), v, uv).unwrap().unwrap();
        let rotated_h = s.sample_visible_normal(tilted, oblique_rotation(v), uv).unwrap().unwrap();
        assert!((oblique_rotation(h) - rotated_h).len() < 1e-11);
        close(s.visible_normal_pdf(frame(), v, h).unwrap(), s.visible_normal_pdf(tilted, oblique_rotation(v), rotated_h).unwrap(), 1e-10);
        match (
            s.sample_visible_reflection(frame(), geometric, v, uv).unwrap(),
            s.sample_visible_reflection(tilted, oblique_rotation(geometric), oblique_rotation(v), uv).unwrap(),
        ) {
            (Some(a), Some(b)) => {
                assert!((oblique_rotation(a.direction) - b.direction).len() < 1e-10);
                assert!((a.weight - b.weight).len() < 1e-10);
                close(a.pdf, b.pdf, 1e-10);
            }
            (None, None) => {}
            _ => panic!("tilting changed support away from a hemisphere boundary"),
        }
    }
}

#[test]
fn tilted_grazing_and_cap_opposition_are_finite_with_explicit_null_support() {
    let tilted = VisualFrame::new(oblique_rotation(Vec3::Z), oblique_rotation(Vec3::X)).unwrap();
    // A resolved 1e-6/1e-9 elevation, rather than an unrepresentable subnormal
    // added to O(1) world components. The existing frame-Z subnormal test stays.
    for elevation in [1e-6, 1e-9] {
        let local_view = unit_direction(Vec3::new(1.0, 0.0, elevation)).unwrap();
        let view = oblique_rotation(local_view);
        for (rx, ry) in [(0.65, 0.9), (0.9, 0.65), (MIN_ROUGHNESS, 1.0)] {
            let s = white(rx, ry);
            for radial in [0.0, 0.5, 1.0_f64.next_down()] {
                // Azimuth 1/2 opposes the stretched view's positive X component;
                // radial→1 approaches the lower cap boundary (w+c cancellation).
                for azimuth in [0.5_f64.next_down(), 0.5, 0.5_f64.next_up()] {
                    let uv = [radial, azimuth];
                    let h = s.sample_visible_normal(tilted, view, uv).unwrap().unwrap();
                    assert!(h.is_finite());
                    close(h.len(), 1.0, 1e-13);
                    let pdf_h = s.visible_normal_pdf(tilted, view, h).unwrap();
                    assert!(pdf_h.is_finite() && pdf_h >= 0.0);
                    if let Some(sample) = s.sample_visible_reflection(tilted, tilted.normal(), view, uv).unwrap() {
                        assert!(sample.direction.dot(tilted.normal()) > 0.0);
                        assert!(sample.pdf.is_finite() && sample.pdf > 0.0 && sample.weight.is_finite());
                        close(sample.pdf, s.visible_reflection_pdf(tilted, tilted.normal(), view, sample.direction).unwrap(), 1e-12);
                    }
                }
            }
            // Both sides of the shading horizon have explicit support. This
            // avoids inferring a sign from an exactly tangent, rounded dot.
            assert!(s.sample_visible_normal(tilted, oblique_rotation(Vec3::new(1.0, 0.0, -elevation)), [0.2, 0.3]).unwrap().is_none());
            assert_eq!(s.visible_normal_pdf(tilted, oblique_rotation(Vec3::new(1.0, 0.0, -elevation)), tilted.normal()).unwrap(), 0.0);
        }
    }
}

fn independent_visible_density(rx: f64, ry: f64, v: Vec3, h: Vec3) -> f64 {
    if v.z <= 0.0 || h.z <= 0.0 || v.dot(h) <= 0.0 {
        return 0.0;
    }
    let (ax, ay) = (rx * rx, ry * ry);
    let q = (h.x / ax).powi(2) + (h.y / ay).powi(2) + h.z * h.z;
    let distribution = 1.0 / (PI * ax * ay * q * q);
    let masking_over_cosine = 2.0 / (v.z + (v.z * v.z + (ax * v.x).powi(2) + (ay * v.y).powi(2)).sqrt());
    distribution * masking_over_cosine * v.dot(h)
}

#[test]
fn unequal_axes_normal_view_has_analytic_acceptance_and_unconditional_pdf_mass() {
    const N: usize = 256;
    for (rx, ry) in [(0.65_f64, 0.9_f64), (0.9, 0.65)] {
        let s = white(rx, ry);
        let (ax, ay) = (rx * rx, ry * ry);
        // At v=n, tan²(theta_h)=u/(1-u)*(ax² cos²(phi)+ay² sin²(phi)).
        // l.z>0 iff u<1/(1+ax² cos²(phi)+ay² sin²(phi)); integrating phi
        // gives 1/sqrt((1+ax²)*(1+ay²)), not a density normalized to success.
        let acceptance = 1.0 / ((1.0 + ax * ax) * (1.0 + ay * ay)).sqrt();
        let (mut accepted, mut nulls, mut pdf_mass, mut normal_mass) = (0, 0, 0.0, 0.0);
        for i in 0..N {
            let u = (i as f64 + 0.5) / N as f64;
            let radius = ((1.0 - u) * (1.0 + u)).sqrt();
            for j in 0..N {
                let t = (j as f64 + 0.5) / N as f64;
                let phi = TAU * t;
                let direction = Vec3::new(radius * phi.cos(), radius * phi.sin(), u);
                let half = unit_direction(Vec3::Z + direction).unwrap();
                let expected_pdf = independent_visible_density(rx, ry, Vec3::Z, half) / (4.0 * half.z);
                close(s.visible_reflection_pdf(frame(), Vec3::Z, Vec3::Z, direction).unwrap(), expected_pdf, 1e-12);
                pdf_mass += expected_pdf;
                normal_mass += independent_visible_density(rx, ry, Vec3::Z, direction);
                if s.sample_visible_reflection(frame(), Vec3::Z, Vec3::Z, [u, t]).unwrap().is_some() {
                    accepted += 1;
                } else {
                    nulls += 1;
                }
            }
        }
        assert_eq!(accepted + nulls, N * N);
        assert!(nulls > 0);
        // Midpoint radial acceptance has <=1/N counting error per azimuth;
        // smooth-azimuth and solid-angle quadrature remain separate checks.
        close(accepted as f64 / (N * N) as f64, acceptance, 1.0 / N as f64);
        close(pdf_mass * TAU / (N * N) as f64, acceptance, 0.002);
        close(normal_mass * TAU / (N * N) as f64, 1.0, 0.002);
        assert!(pdf_mass * TAU / ((N * N) as f64) < 1.0);
    }
}

#[test]
fn unequal_axes_grazing_pdf_mass_matches_attempts_and_geometric_rejection() {
    const N: usize = 256;
    let s = white(0.65, 0.9);
    let view = unit_direction(Vec3::new(1.0, 0.2, 0.1)).unwrap();
    let geometric = unit_direction(Vec3::new(-0.05, 0.0, 1.0)).unwrap();
    let (mut accepted, mut nulls, mut mass, mut normal_mass) = (0, 0, 0.0, 0.0);
    for i in 0..N {
        let u = (i as f64 + 0.5) / N as f64;
        let radius = ((1.0 - u) * (1.0 + u)).sqrt();
        for j in 0..N {
            let t = (j as f64 + 0.5) / N as f64;
            let phi = TAU * t;
            let light = Vec3::new(radius * phi.cos(), radius * phi.sin(), u);
            let half = unit_direction(view + light).unwrap();
            let expected = if geometric.dot(light) > 0.0 { independent_visible_density(0.65, 0.9, view, half) / (4.0 * view.dot(half)) } else { 0.0 };
            close(s.visible_reflection_pdf(frame(), geometric, view, light).unwrap(), expected, 1e-11);
            mass += expected;
            normal_mass += independent_visible_density(0.65, 0.9, view, light);
            if s.sample_visible_reflection(frame(), geometric, view, [u, t]).unwrap().is_some() {
                accepted += 1;
            } else {
                nulls += 1;
            }
        }
    }
    assert_eq!(accepted + nulls, N * N);
    assert!(accepted > 0 && nulls > 0);
    let integrated = mass * TAU / (N * N) as f64;
    // A discontinuous geometric-hemisphere cut and two independently parameterized
    // midpoint grids have a finite 0.01 agreement gate; no universal error bound.
    close(integrated, accepted as f64 / (N * N) as f64, 0.01);
    close(normal_mass * TAU / (N * N) as f64, 1.0, 0.002);
    assert!(integrated > 0.0 && integrated < 1.0);
}
