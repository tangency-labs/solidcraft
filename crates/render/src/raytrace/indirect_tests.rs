use super::*;
use solidcraft_viewport::clear_coat::ClearCoat;

fn white() -> OpaqueSurface {
    OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 0.0, 1.0, 1.0).unwrap()
}
fn rectangle() -> Rectangle {
    Rectangle::new(&RectangleLightOptions {
        center_mm: [0.0, 0.0, 2.0],
        edge_u_mm: [2.0, 0.0, 0.0],
        edge_v_mm: [0.0, -2.0, 0.0],
        color: [1.0; 3],
        intensity: 1.0,
    })
    .unwrap()
}

#[test]
fn guided_photon_mixture_integrates_point_and_one_sided_rectangle_power() {
    let count = 4096;
    let width: f64 = 0.125;
    let distance = 4.0;
    let radius = distance * (width * (2.0 - width)).sqrt();
    let cone = PhotonGuide { center: Vec3::Z * distance, radius }.cone(Vec3::ZERO, 2.0).unwrap();
    let mut point_power = 0.0;
    for i in 0..count {
        let u = (f64::from(i) + 0.5) / f64::from(count);
        let z = 1.0 - 2.0 * u;
        let baseline = Vec3::new((1.0 - z * z).sqrt(), 0.0, z);
        let guided = cone.sample([u, 0.0]).unwrap();
        let density = 1.0 / (4.0 * PI);
        point_power +=
            (1.0 - PhotonCone::MIX) / cone.mixture_pdf(baseline, density, false) + PhotonCone::MIX / cone.mixture_pdf(guided, density, true);
    }
    assert!((point_power / f64::from(count) - 4.0 * PI).abs() < 1e-11);

    let light = rectangle();
    // The chosen width gives an exact partition of this stratified cosine
    // domain. A guide behind the emitter must contribute null samples.
    for sign in [1.0, -1.0] {
        let guide = PhotonGuide { center: light.center + light.normal * (sign * distance), radius };
        let mut power = 0.0;
        for i in 0..count {
            let u = (f64::from(i) + 0.5) / f64::from(count);
            for (selector, probability) in [(0.0, PhotonCone::MIX), (0.9, 1.0 - PhotonCone::MIX)] {
                let (_, _, flux) = light.emit_photon([0.5, 0.5], [u, 0.25], Some((guide, selector))).unwrap();
                power += probability * flux.x;
            }
        }
        assert!((power / f64::from(count) - PI * light.area * light.radiance.x).abs() < 1e-6);
    }
    let baseline = light.emit_photon([0.5, 0.5], [0.1, 0.25], None).unwrap();
    for guide in [
        PhotonGuide { center: light.center, radius },
        PhotonGuide { center: light.center + light.normal * radius, radius },
        PhotonGuide { center: light.center + light.normal * 1e6, radius },
    ] {
        assert_eq!(light.emit_photon([0.5, 0.5], [0.1, 0.25], Some((guide, 0.0))).unwrap(), baseline);
    }
}

#[test]
fn opaque_path_mixture_has_analytic_lambert_and_matched_full_coat_density() {
    let count = 16384;
    let mut mean = Vec3::ZERO;
    for component in [0.25, 0.75] {
        for i in 0..count {
            if let Some(s) = sample(white(), Vec3::Z, Vec3::Z, Vec3::Z, [component, (f64::from(i) + 0.5) / f64::from(count), 0.37]).unwrap() {
                // For alpha=1 and normal view, NDF reflection density is 1/(4*pi).
                let expected = 0.5 * s.direction.z / PI + 0.5 / (4.0 * PI);
                assert!((s.pdf - expected).abs() < 1e-14);
                assert!((s.weight.x * s.pdf - s.direction.z / PI).abs() < 1e-14);
                mean += s.weight / (2.0 * f64::from(count));
            }
        }
    }
    assert!((mean - Vec3::new(1.0, 1.0, 1.0)).len() < 1e-6, "index-matched Lambert white furnace: {mean:?}");
    for metallic in [0.0, 0.5, 1.0] {
        let surface =
            OpaqueSurface::new(Vec3::new(0.7, 0.3, 0.1), metallic, 0.4, 1.5).unwrap().with_clear_coat(ClearCoat::new(0.8, 0.3, 1.4).unwrap());
        for i in 0..4096 {
            let u = [(f64::from(i) + 0.5) / 4096.0, ((i * 37 % 4096) as f64 + 0.5) / 4096.0, ((i * 101 % 4096) as f64 + 0.5) / 4096.0];
            if let Some(s) = sample(surface, Vec3::Z, Vec3::Z, Vec3::Z, u).unwrap() {
                let h = unit_direction(Vec3::Z + s.direction).unwrap();
                let ndf = |roughness: f64| {
                    let a2 = roughness.powi(4);
                    let denom = 1.0 - h.z * h.z + h.z * h.z * a2;
                    a2 / (PI * denom * denom) * h.z / (4.0 * h.z)
                };
                // The coat takes 0.4; the diffuse lobe takes 0.5 × (1 − metallic) of the rest.
                let diffuse = 0.5 * (1.0 - metallic);
                let expected = 0.6 * (diffuse * s.direction.z / PI + (1.0 - diffuse) * ndf(0.4)) + 0.4 * ndf(0.3);
                assert!((s.pdf - expected).abs() < 1e-12 * expected.max(1.0));
                let expected = surface.brdf(Vec3::Z, Vec3::Z, s.direction).unwrap() * s.direction.z;
                assert!((s.weight * s.pdf - expected).len() < 1e-11 * expected.len().max(1.0));
            }
        }
    }
    for bad in [f64::NAN, -0.1, 1.0] {
        assert!(sample(white(), Vec3::Z, Vec3::Z, Vec3::Z, [bad, 0.3, 0.7]).is_err());
    }
    assert!((power_weight(3.0, 4.0).unwrap() - 9.0 / 25.0).abs() < 1e-16);
    assert_eq!(power_weight(0.0, 0.0).unwrap(), 0.0);
    assert!(power_weight(f64::INFINITY, 1.0).is_err());
}

#[test]
fn opaque_path_rectangles_have_exact_facing_intervals_area_pdf_and_sample_count_weights() {
    let light = rectangle();
    let center = light.sample(Vec3::ZERO, [0.5, 0.5]).unwrap().unwrap();
    assert_eq!(center.position, Vec3::new(0.0, 0.0, 2.0));
    assert_eq!(center.pdf, 1.0); // distance² / (area*cos) = 4/4.
    let ray = Ray::new(Vec3::ZERO, Vec3::Z, 0.0, 3.0).unwrap();
    assert_eq!(light.intersect(ray).unwrap().unwrap(), (2.0, center.position));
    assert!(light.intersect(Ray::new(Vec3::ZERO, Vec3::Z, 0.0, 2.0_f64.next_down()).unwrap()).unwrap().is_none());
    assert!(light.intersect(Ray::new(Vec3::new(0.0, 0.0, 3.0), -Vec3::Z, 0.0, 4.0).unwrap()).unwrap().is_none());
    assert!(light.sample(Vec3::new(0.0, 0.0, 3.0), [0.5, 0.5]).unwrap().is_none());
    for samples in [1, 4, 16, 64] {
        let p = surface_pdf(white(), Vec3::Z, Vec3::Z, Vec3::Z).unwrap();
        let light_density = f64::from(samples) * center.pdf;
        let a = power_weight(light_density, p).unwrap();
        let b = power_weight(p, light_density).unwrap();
        assert!((a + b - 1.0).abs() < 2e-16, "sample count belongs to both MIS competitors");
    }
}

#[test]
fn opaque_path_rectangle_nee_and_bsdf_mis_means_match_the_analytic_projected_solid_angle() {
    let light = rectangle();
    // Independent projected solid angle of a parallel rectangle, half extents1, height2.
    let expected = 4.0 / 5.0_f64.sqrt() * (1.0 / 5.0_f64.sqrt()).atan() / PI;
    for samples in [1, 4, 16] {
        let mut nee = 0.0;
        let mut bsdf = 0.0;
        let side = 512;
        for y in 0..side {
            for x in 0..side {
                let u = [(f64::from(x) + 0.5) / f64::from(side), (f64::from(y) + 0.5) / f64::from(side)];
                let s = light.sample(Vec3::ZERO, u).unwrap().unwrap();
                let p = surface_pdf(white(), Vec3::Z, Vec3::Z, s.direction).unwrap();
                nee += s.direction.z / (PI * s.pdf) * power_weight(f64::from(samples) * s.pdf, p).unwrap();
                for component in [0.25, 0.75] {
                    if let Some(s) = sample(white(), Vec3::Z, Vec3::Z, Vec3::Z, [component, u[0], u[1]]).unwrap()
                        && let Some((_, point)) = light.intersect(Ray::new(Vec3::ZERO, s.direction, 0.0, 100.0).unwrap()).unwrap()
                    {
                        bsdf += 0.5 * s.weight.x * power_weight(s.pdf, f64::from(samples) * light.pdf(Vec3::ZERO, point).unwrap()).unwrap();
                    }
                }
            }
        }
        let actual = (nee + bsdf) / f64::from(side * side);
        // A fixed midpoint grid crosses the emitter boundary; its integration error is
        // bounded here at 0.002 absolute radiance, independent of production's sampler.
        assert!((actual - expected).abs() < 0.002, "N={samples}: {actual} vs {expected}");
    }
}

#[test]
fn opaque_path_oblique_and_grazing_coated_response_and_null_mass_match_independent_hemisphere_quadrature() {
    let side = 512;
    let cells = f64::from(side * side);
    for cosine in [0.4_f64, 0.02] {
        let view = Vec3::new((1.0 - cosine * cosine).sqrt(), 0.0, cosine);
        for metallic in [0.0, 0.5, 1.0] {
            let surface =
                OpaqueSurface::new(Vec3::new(0.7, 0.3, 0.1), metallic, 0.6, 1.5).unwrap().with_clear_coat(ClearCoat::new(0.8, 0.5, 1.4).unwrap());
            // The coat takes 0.4; the diffuse lobe takes 0.5 × (1 − metallic) of the other 0.6.
            // One variate inside each nonempty interval, with that interval's probability.
            let diffuse = 0.5 * (1.0 - metallic);
            let components = [(0.2, 0.4), (0.4 + 0.3 * diffuse, 0.6 * diffuse), (0.4 + 0.6 * diffuse + 0.3 * (1.0 - diffuse), 0.6 * (1.0 - diffuse))];
            let mut quadrature = Vec3::ZERO;
            let mut density = 0.0;
            let mut sampled = Vec3::ZERO;
            let mut mass = 0.0;
            let mut sampled_second_moment = 0.0;
            let mut ndf_second_moment = 0.0;
            for row in 0..side {
                let u = (f64::from(row) + 0.5) / f64::from(side);
                for col in 0..side {
                    let v = (f64::from(col) + 0.5) / f64::from(side);
                    // Independent integration over light-direction solid angle, not the
                    // proposal's half-vector coordinates. Equal-area cells have dω=2π/N².
                    let phi = TAU * v;
                    let l = Vec3::new((1.0 - u * u).sqrt() * phi.cos(), (1.0 - u * u).sqrt() * phi.sin(), u);
                    let half = unit_direction(view + l).unwrap();
                    let vndf = |roughness: f64| {
                        let a2 = roughness.powi(4);
                        let denominator = 1.0 - half.z * half.z + half.z * half.z * a2;
                        let lambda = 0.5 * ((1.0 + a2 * (1.0 - cosine * cosine) / (cosine * cosine)).sqrt() - 1.0);
                        a2 / (PI * denominator * denominator * (1.0 + lambda) * 4.0 * cosine)
                    };
                    let independent_pdf = 0.6 * (diffuse * u / PI + (1.0 - diffuse) * vndf(0.6)) + 0.4 * vndf(0.5);
                    density += independent_pdf * TAU / cells;
                    let integrand = surface.brdf(Vec3::Z, view, l).unwrap() * u;
                    quadrature += integrand * (TAU / cells);
                    let ndf = |roughness: f64| {
                        let a2 = roughness.powi(4);
                        let denominator = 1.0 - half.z * half.z + half.z * half.z * a2;
                        a2 * half.z / (PI * denominator * denominator * 4.0 * view.dot(half))
                    };
                    let ndf_pdf = 0.6 * (diffuse * u / PI + (1.0 - diffuse) * ndf(0.6)) + 0.4 * ndf(0.5);
                    ndf_second_moment += integrand.x.powi(2) * TAU / (cells * ndf_pdf);
                    // Enumerate mixture components with their original probability.
                    // Rejected directions contribute zero mass and zero radiance;
                    // no retry and no division by the number of accepted samples.
                    for (component, probability) in components.into_iter().filter(|&(_, probability)| probability > 0.0) {
                        if let Some(s) = sample(surface, Vec3::Z, Vec3::Z, view, [component, u, v]).unwrap() {
                            mass += probability / cells;
                            sampled += s.weight * (probability / cells);
                            sampled_second_moment += s.weight.x.powi(2) * (probability / cells);
                            assert!((s.pdf - surface_pdf(surface, Vec3::Z, view, s.direction).unwrap()).abs() < 1e-12 * s.pdf.max(1.0));
                        }
                    }
                }
            }
            assert!(mass < 1.0, "grazing null samples must remain in the estimator");
            if cosine == 0.02 && metallic == 1.0 {
                assert!(
                    sampled_second_moment < 0.8 * ndf_second_moment,
                    "visible normals second moment {sampled_second_moment}, full NDF {ndf_second_moment}"
                );
            }
            assert!((mass - density).abs() < 0.005, "nv={cosine}, metal={metallic}: accepted mass{mass}, independent PDF mass{density}");
            for (actual, expected) in [sampled.x, sampled.y, sampled.z].into_iter().zip([quadrature.x, quadrature.y, quadrature.z]) {
                assert!((actual - expected).abs() < 0.005, "nv={cosine}, metal={metallic}: sampled{actual}, independent hemisphere{expected}");
            }
        }
    }
}

#[test]
fn opaque_path_nearly_orthogonal_extreme_aspect_rectangles_round_trip_their_admitted_samples() {
    // The common light contract permits |cos(U,V)| <= 1e-10. A diagonal
    // projection would infer s≈22.5 for the first sample despite its true s=0.
    let light = Rectangle::new(&RectangleLightOptions {
        center_mm: [0.0, 0.0, 2.0],
        edge_u_mm: [1e-6, 0.0, 0.0],
        edge_v_mm: [9e-5, -1e6, 0.0],
        color: [1.0; 3],
        intensity: 1.0,
    })
    .unwrap();
    for uv in [[0.5, 0.75], [0.25, 0.125], [0.75, 0.625]] {
        let sample = light.sample(Vec3::ZERO, uv).unwrap().unwrap();
        let ray = Ray::new(Vec3::ZERO, sample.direction, 0.0, sample.position.len() * 2.0).unwrap();
        let (distance, point) = light.intersect(ray).unwrap().expect("every admitted interior sample belongs to the BSDF support");
        assert!((distance - sample.position.len()).abs() <= 1e-9 * distance);
        assert!((point - sample.position).len() <= 1e-9 * distance);
        assert!((light.pdf(Vec3::ZERO, point).unwrap() - sample.pdf).abs() <= 1e-9 * sample.pdf);
    }
}

#[test]
fn opaque_path_source_strata_are_open_at_both_floating_point_endpoints() {
    use crate::lighting::{HdrSource, environment_input::EnvironmentInput};
    let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
    for _ in 0..8 {
        bytes.extend_from_slice(&[128, 64, 32, 129]);
    }
    let mut input = EnvironmentInput::new(HdrSource::new(bytes).unwrap(), 0.0, 1.0, 64).unwrap().begin(false, false).unwrap();
    for _ in 0..10000 {
        if input.ready() {
            break;
        }
        input.prepare_one(|| false).unwrap();
    }
    assert!(input.ready());
    let environment = input.view().unwrap();
    for side in [1, 2, 4, 8] {
        for cell in 0..side {
            for raw in [0.0, 1.0_f64.next_down()] {
                let value = open_stratum(raw, cell, side).unwrap();
                assert!(value > 0.0 && value < 1.0, "N={}, cell={cell}, raw={raw}, value={value}", side * side);
                assert!(environment.sample([value, value]).is_ok());
                assert!(rectangle().sample(Vec3::ZERO, [value, value]).is_ok());
            }
        }
    }
    for (raw, cell, side) in [(1.0, 0, 1), (-0.1, 0, 1), (f64::NAN, 0, 1), (0.5, 1, 1), (0.5, 0, 3)] {
        assert!(open_stratum(raw, cell, side).is_err());
    }
}

#[test]
fn authored_point_zero_two_base_and_coated_path_pdf_use_the_exact_unclamped_finish() {
    let original = white();
    let finish = original.with_authored_finish(Vec3::new(1.0, 1.0, 1.0), 1.0, 0.02).unwrap();
    for coat_weight in [0.0, 0.6] {
        let coat = solidcraft_viewport::clear_coat::ClearCoat::new(coat_weight, 0.2, 1.5).unwrap();
        let surface = finish.with_clear_coat(coat);
        let a2 = 0.02_f64.powi(4);
        let ca2 = 0.2_f64.powi(4);
        let c = 0.8;
        let h2 = (1.0 + c) / 2.0;
        let density = |alpha2: f64| {
            let d = (1.0 - h2) + h2 * alpha2;
            alpha2 / (std::f64::consts::PI * d * d) / 4.0
        };
        let p = coat_weight / 2.0;
        // The authored finish is a pure metal, so the base spends nothing on its diffuse lobe.
        let expected = (1.0 - p) * density(a2) + p * density(ca2);
        let l = Vec3::new(0.6, 0.0, c);
        let actual = surface_pdf(surface, Vec3::Z, Vec3::Z, l).unwrap();
        assert!((actual - expected).abs() < 1e-14);
        for u in [[0.1, 0.2, 0.3], [0.8, 0.2, 0.3], [0.99, 0.9, 0.7]] {
            if let Some(sample) = sample(surface, Vec3::Z, Vec3::Z, Vec3::Z, u).unwrap() {
                assert!(sample.pdf.is_finite() && sample.pdf > 0.0);
                let pdf = surface_pdf(surface, Vec3::Z, Vec3::Z, sample.direction).unwrap();
                assert_eq!(sample.pdf, pdf);
                let brdf = surface.brdf(Vec3::Z, Vec3::Z, sample.direction).unwrap();
                assert_eq!(sample.weight, brdf * (sample.direction.z / pdf));
            }
        }
    }
}
