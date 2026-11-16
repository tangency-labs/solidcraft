use super::*;

fn hdr(extra: &str, pixels: &[[u8; 4]]) -> Vec<u8> {
    let mut bytes = format!("#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n{extra}\n-Y 2 +X 4\n").into_bytes();
    for pixel in pixels {
        bytes.extend_from_slice(pixel);
    }
    bytes
}
#[test]
fn captured_hdr_requires_explicit_color_policy_and_prepares_without_clipping() {
    let bytes = hdr("", &[[128, 64, 32, 130]; 8]);
    assert!(EnvironmentSource::hdr(bytes.clone(), 0.0, 1.0, false).is_err());
    let source = EnvironmentSource::hdr(bytes, 0.3, 2.0, true).unwrap();
    assert_eq!(source.info().source_sha256.len(), 64);
    assert_eq!(source.info().color_policy, "explicit_assumed_linear_srgb_d65");
    let prepared = source.prepare(&CancelToken::default()).unwrap();
    assert_eq!(prepared.dimensions(), [4, 2]);
    assert!(prepared.texels().iter().all(|t| t.rgb == [2.0, 1.0, 0.5]));
    let wrong = hdr("PRIMARIES=0.67 0.33 0.21 0.71 0.14 0.08 0.31 0.33\n", &[[128, 128, 128, 129]; 8]);
    assert!(EnvironmentSource::hdr(wrong, 0.0, 1.0, true).is_err());
    let declared = hdr("PRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n", &[[128, 128, 128, 129]; 8]);
    let shared = HdrSource::new(declared.clone()).unwrap();
    let resource = EnvironmentSource::from_hdr(shared.clone(), 0.0, 1.0, false).unwrap();
    let Source::Hdr(retained) = &resource.source else { panic!("HDR fixture") };
    assert_eq!(shared.encoded_bytes().as_ptr(), retained.encoded_bytes().as_ptr());
    let captured = EnvironmentSource::hdr(shared.encoded_bytes().to_vec(), 0.0, 1.0, false).unwrap();
    assert_eq!(resource.info().source_sha256, captured.info().source_sha256);
    assert_eq!(resource.admitted_bytes(), captured.admitted_bytes(), "sharing does not bypass the job budget");
    assert!(EnvironmentSource::sky(SkyGradient::default(), f64::NAN, 1.0).is_err());
    assert!(EnvironmentSource::sky(SkyGradient::default(), 0.0, 101.0).is_err());
    let token = CancelToken::default();
    token.cancel();
    assert_eq!(source.prepare(&token).err().unwrap().code, ErrorCode::Cancelled);
}
#[test]
fn exact_solid_angle_distribution_handles_poles_black_and_white_maps() {
    for color in [[0.0; 3], [1.0; 3]] {
        let map = PreparedEnvironment::new(64, 32, vec![color; 2048], &CancelToken::default()).unwrap();
        let total: f64 = map.texels().iter().enumerate().map(|(i, t)| f64::from(t.pdf) * row_solid_angle(64, 32, i / 64)).sum();
        assert!((total - 1.0).abs() < 1e-7);
        for t in map.texels() {
            assert!((f64::from(t.pdf) - 1.0 / (4.0 * PI)).abs() < 1e-8);
        }
        for d in [Vec3::Z, -Vec3::Z, Vec3::X, Vec3::new(1.0, -1e-12, 0.0)] {
            let (value, pdf) = map.evaluate(d).unwrap();
            assert_eq!(value, color);
            assert!((f64::from(pdf) - 1.0 / (4.0 * PI)).abs() < 1e-8);
        }
    }
}
#[test]
fn alias_samples_match_unequal_texel_probabilities_and_lookup_pdf() {
    let mut pixels = vec![[0.0; 3]; 32];
    pixels[2] = [1.0; 3];
    pixels[11] = [4.0; 3];
    pixels[29] = [2.0; 3];
    let map = PreparedEnvironment::new(8, 4, pixels, &CancelToken::default()).unwrap();
    let mut counts = [0usize; 32];
    let mut rng = 17u64;
    let mut random = || {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        (rng >> 11) as f64 / (1u64 << 53) as f64
    };
    let count = 200_000;
    for _ in 0..count {
        let (direction, rgb, pdf) = map.sample(random(), random(), random(), random()).unwrap();
        let (actual, actual_pdf) = map.evaluate(direction).unwrap();
        assert_eq!(rgb, actual);
        assert_eq!(pdf, actual_pdf);
        let index = if rgb == [1.0; 3] {
            2
        } else if rgb == [4.0; 3] {
            11
        } else {
            assert_eq!(rgb, [2.0; 3]);
            29
        };
        counts[index] += 1;
    }
    for i in [2, 11, 29] {
        let expected = f64::from(map.texels()[i].pdf) * row_solid_angle(8, 4, i / 8);
        assert!((counts[i] as f64 / f64::from(count) - expected).abs() < 0.004);
    }
}
#[test]
fn original_sky_is_bounded_cancellable_and_reproducible() {
    let source = EnvironmentSource::sky(SkyGradient::default(), 0.0, 1.0).unwrap();
    let a = source.prepare(&CancelToken::default()).unwrap();
    assert_eq!(a.dimensions(), [512, 256]);
    let (zenith, _) = a.evaluate(Vec3::Z).unwrap();
    let (horizon, _) = a.evaluate(Vec3::X).unwrap();
    let (ground, _) = a.evaluate(-Vec3::Z).unwrap();
    assert!(zenith[2] > zenith[0]);
    assert!(horizon[0] > zenith[0]);
    assert!(ground[0] > ground[2]);
    assert!(source.admitted_bytes() > a.texels().len() as u64 * 64);
    assert!(EnvironmentSource::sky(SkyGradient { zenith: [f32::INFINITY; 3], ..Default::default() }, 0.0, 1.0).is_err());
}

#[test]
fn hdr_admission_counts_retained_capacity_not_just_file_length() {
    let bytes = hdr("", &[[128, 128, 128, 129]; 8]);
    let mut overallocated = Vec::with_capacity(super::super::MAX_HDR_BYTES);
    overallocated.extend_from_slice(&bytes);
    let retained = overallocated.capacity() as u64;
    let source = EnvironmentSource::hdr(overallocated, 0.0, 1.0, true).unwrap();
    assert_eq!(source.info().encoded_bytes, bytes.len() as u64);
    assert_eq!(source.admitted_bytes(), retained + 8 * 160 + 65536);
}

#[test]
fn cooperative_alias_builder_preserves_original_gpu_table_bits() {
    let colors = [0.0, 0.0, 1.0, 4.0, 9.0, 0.0, 2.0, 8.0];
    let pixels: Vec<_> = colors.into_iter().map(|v| [v; 3]).collect();
    // Pre-refactor table retained as a bitwise compatibility oracle.
    let expected = [
        (0x00000000, 0x00000000, 4),
        (0x00000000, 0x00000000, 4),
        (0x3cd94caf, 0x3eaaaaab, 7),
        (0x3dd94caf, 0x3f800000, 3),
        (0x3e747645, 0x3f2aaaab, 3),
        (0x00000000, 0x00000000, 7),
        (0x3d594caf, 0x3f2aaaab, 7),
        (0x3e594caf, 0x3f2aaaab, 4),
    ];
    let mut builder = EnvironmentBuilder::new(4, 2).unwrap();
    assert!(EnvironmentBuilder::new(4, 2).unwrap().finish().is_err());
    let mut steps = 0;
    while !builder.ready() {
        builder.step(&pixels).unwrap();
        steps += 1;
    }
    assert!((16..=25).contains(&steps), "bounded mass/entry/alias operations: {steps}");
    let map = builder.finish().unwrap();
    for ((entry, color), (pdf, threshold, alias)) in map.texels().iter().zip(colors).zip(expected) {
        assert_eq!(entry.rgb, [color; 3]);
        assert_eq!((entry.pdf.to_bits(), entry.threshold.to_bits(), entry.alias), (pdf, threshold, alias));
    }
}

#[test]
fn alias_pair_has_independent_spherical_cell_and_within_cell_distribution() {
    let colors = [0.0, 0.0, 1.0, 4.0, 9.0, 0.0, 2.0, 8.0];
    let map = PreparedEnvironment::new(4, 2, colors.into_iter().map(|v| [v; 3]).collect(), &CancelToken::default()).unwrap();
    let mut counts = [[0usize; 16]; 8];
    for i in 0..4096 {
        for j in 0..16 {
            let (d, rgb, pdf) = map.sample_pair([(f64::from(i) + 0.5) / 4096.0, (f64::from(j) + 0.5) / 16.0]).unwrap();
            let index = colors.iter().position(|value| [*value; 3] == rgb).unwrap();
            assert!(rgb[0] > 0.0);
            assert_eq!(map.evaluate(d).unwrap(), (rgb, pdf));
            let vertical = if index < 4 { 1.0 - d.z } else { -d.z };
            let azimuth = d.y.atan2(d.x).rem_euclid(TAU) / (TAU / 4.0) - (index % 4) as f64;
            counts[index][((vertical * 4.0) as usize).min(3) * 4 + ((azimuth * 4.0) as usize).min(3)] += 1;
        }
    }
    for (index, bins) in counts.into_iter().enumerate().filter(|(i, _)| colors[*i] > 0.0) {
        let expected = f64::from(colors[index]) / 24.0 / 16.0;
        for count in bins {
            assert!((count as f64 / 65536.0 - expected).abs() < 0.0003);
        }
    }
}

#[test]
fn underflowing_positive_hdr_mass_uses_cpu_uniform_support_without_changing_gpu_entries() {
    let mut pixels = vec![[1e-38; 3]; 8];
    pixels[0] = [1e8; 3];
    let map = PreparedEnvironment::new(4, 2, pixels, &CancelToken::default()).unwrap();
    assert_eq!(map.texels()[7].pdf, 0.0);
    assert!(!map.preserves_positive_support);
    let (d, rgb, pdf) = map.sample_pair([0.75, 0.875]).unwrap();
    assert_eq!(rgb, [1e-38; 3]);
    assert_eq!(pdf, (1.0 / (4.0 * PI)) as f32);
    let u = d.y.atan2(d.x).rem_euclid(TAU) / TAU;
    let v = d.z.acos() / PI;
    assert_eq!(map.pdf_at_uv([u, v]).unwrap(), pdf);
}

#[test]
fn alias_builder_rejects_hostile_dimensions_before_admission_or_allocation() {
    for (width, height) in [(0, 1), (2, 0), (1, 2_097_152), (2_097_152, 1), (2049, 1), (2, 1025), (4, 4), (u32::MAX, u32::MAX)] {
        assert!(EnvironmentBuilder::admitted_bytes(width, height).is_err());
        assert!(EnvironmentBuilder::new(width, height).is_err());
    }
}

#[test]
fn subnormal_positive_alias_probabilities_keep_gpu_bytes_and_use_cpu_uniform_support() {
    let mut pixels = vec![[1e-39; 3]; 8];
    pixels[0] = [1.0; 3];
    let map = PreparedEnvironment::new(4, 2, pixels, &CancelToken::default()).unwrap();
    assert!(map.texels()[7].pdf.is_subnormal());
    assert!(map.texels()[7].threshold.is_subnormal());
    assert!(!map.preserves_positive_support);
    let (_, rgb, pdf) = map.sample_pair([0.75, 0.875]).unwrap();
    assert_eq!(rgb, [1e-39; 3]);
    assert_eq!(pdf, (1.0 / (4.0 * PI)) as f32);
}
