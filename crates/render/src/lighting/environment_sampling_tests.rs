use super::*;
use crate::lighting::{HdrDecodeTask, HdrSource, HdrTaskPoll};

fn decoded(pixels: [[u8; 4]; 8], header: &str) -> DecodedHdr {
    let mut bytes = format!("#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n{header}\n-Y 2 +X 4\n").into_bytes();
    for pixel in pixels {
        bytes.extend_from_slice(&pixel);
    }
    let mut task = HdrDecodeTask::new(HdrSource::new(bytes).unwrap()).unwrap();
    for _ in 0..100 {
        if let HdrTaskPoll::Complete(decoded) = task.step(100, || false).unwrap() {
            return decoded;
        }
    }
    panic!("bounded fixture failed to decode");
}
fn prepared(decoded: &DecodedHdr, rotation: f64, intensity: f64) -> Environment<'_> {
    let mut task = EnvironmentPreparation::new(decoded, rotation, intensity).unwrap();
    while !task.step(1, || Ok(())).unwrap() {}
    task.finish().unwrap()
}

#[test]
fn constant_source_has_uniform_solid_angle_pdf_and_exact_radiance_integral() {
    let data = decoded([[128, 64, 32, 129]; 8], "");
    let env = prepared(&data, 0.0, 2.0);
    let expected = Vec3::new(2.0, 1.0, 0.5);
    let mut integral = Vec3::ZERO;
    for i in 0..64 {
        let sample = env.sample([(f64::from(i) + 0.5) / 64.0, 0.37]).unwrap();
        assert!((sample.direction.len() - 1.0).abs() < 1e-14);
        assert_eq!(sample.radiance_source_rgb, expected);
        assert_eq!(env.pdf_solid_angle(sample.direction).unwrap(), sample.pdf_solid_angle);
        integral += sample.radiance_source_rgb / sample.pdf_solid_angle / 64.0;
    }
    assert!((integral - expected * (4.0 * PI)).len() < 1e-12);
    assert!((env.pdf_solid_angle(Vec3::Z).unwrap() * 4.0 * PI - 1.0).abs() < 1e-15);
}

#[test]
fn z_up_uv_centres_seam_poles_and_positive_rotation_are_explicit() {
    let data = decoded([[128, 0, 0, 129]; 8], "");
    let env = prepared(&data, 0.0, 1.0);
    assert_eq!(env.uv(Vec3::X).unwrap(), [0.0, 0.5]);
    assert_eq!(env.uv(Vec3::Y).unwrap(), [0.25, 0.5]);
    assert_eq!(env.uv(Vec3::Z).unwrap(), [0.0, 0.0]);
    assert_eq!(env.uv(-Vec3::Z).unwrap(), [0.0, 1.0]);
    let rotated = prepared(&data, PI / 2.0, 1.0);
    assert_eq!(rotated.uv(Vec3::Y).unwrap(), [0.0, 0.5]);
    assert!(env.uv(Vec3::new(1.0, -1e-8, 0.0)).unwrap()[0] > 0.99);
    let theta = PI / 4.0;
    let phi = PI / 4.0;
    let centre = Vec3::new(theta.sin() * phi.cos(), theta.sin() * phi.sin(), theta.cos());
    let uv = env.uv(centre).unwrap();
    assert!((uv[0] - 0.125).abs() < 1e-14 && (uv[1] - 0.25).abs() < 1e-14);
}

#[test]
fn bright_texel_integral_matches_its_independent_spherical_cell_area() {
    let mut pixels = [[0; 4]; 8];
    pixels[0] = [128, 0, 0, 129];
    let data = decoded(pixels, "");
    let env = prepared(&data, 0.0, 1.0);
    let mut integral = 0.0;
    // 4x2: first cell has phi width pi/2 and z width 1, solid angle pi/2.
    for y in 0..8 {
        for x in 0..8 {
            let sample = env.sample([(f64::from(y) + 0.5) / 8.0, (f64::from(x) + 0.5) / 8.0]).unwrap();
            integral += sample.radiance_source_rgb.x / sample.pdf_solid_angle / 64.0;
        }
    }
    assert!((integral - PI / 2.0).abs() < 1e-14);
    let black = decoded([[0; 4]; 8], "");
    let env = prepared(&black, 0.0, 1.0);
    let sample = env.sample([0.3, 0.7]).unwrap();
    assert_eq!(sample.radiance_source_rgb, Vec3::ZERO);
    assert_eq!(sample.pdf_solid_angle, 1.0 / (4.0 * PI));
}

#[test]
fn caller_budget_cancellation_consumes_preparation_and_preserves_owner_capacity() {
    let data = decoded([[128, 0, 0, 129]; 8], "");
    let bytes = data.retained_pixel_bytes().unwrap();
    let mut task = EnvironmentPreparation::new(&data, 0.0, 1.0).unwrap();
    let mut charged = 0;
    assert!(
        !task
            .step(3, || {
                charged += 1;
                Ok(())
            })
            .unwrap()
    );
    assert_eq!(charged, 3);
    assert_eq!(task.step(1, || Err(HdrError::new(HdrErrorCode::Cancelled, "cancel"))).unwrap_err().code, HdrErrorCode::Cancelled);
    assert!(task.finish().is_err());
    assert_eq!(data.retained_pixel_bytes().unwrap(), bytes);
    let mut task = EnvironmentPreparation::new(&data, 0.0, 1.0).unwrap();
    assert_eq!(task.step(1, || Err(HdrError::new(HdrErrorCode::LimitExceeded, "shared work"))).unwrap_err().code, HdrErrorCode::LimitExceeded);
    assert!(task.finish().is_err());
}

#[test]
fn source_colour_is_not_assumed_srgb_and_decoder_corrections_are_not_reapplied() {
    let raw = decoded([[128, 64, 32, 129]; 8], "EXPOSURE=2\nCOLORCORR=2 1 1\n");
    let env = prepared(&raw, 0.0, 1.0);
    assert_eq!(env.radiance_source_rgb(Vec3::X).unwrap(), Vec3::new(0.25, 0.25, 0.125));
    assert!(env.radiance_linear_srgb(Vec3::X).is_err());
    for header in ["PRIMARIES=0.64 0.33 0.29 0.60 0.15 0.06 0.3333 0.3333\n", "PRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n"] {
        let data = decoded([[128, 0, 0, 129]; 8], header);
        let env = prepared(&data, 0.0, 1.0);
        assert_eq!(env.require_linear_srgb().is_ok(), data.info().primaries == Some([0.64, 0.33, 0.30, 0.60, 0.15, 0.06, 0.3127, 0.3290]));
    }
}

#[test]
fn hostile_options_variates_and_directions_refuse_without_panics() {
    let data = decoded([[128, 0, 0, 129]; 8], "");
    for (rotation, intensity) in [(f64::NAN, 1.0), (0.0, f64::INFINITY), (0.0, -1.0), (0.0, 65.0)] {
        assert!(EnvironmentPreparation::new(&data, rotation, intensity).is_err());
    }
    assert!(EnvironmentPreparation::new(&data, 0.0, 1.0).unwrap().finish().is_err());
    let env = prepared(&data, 0.0, 0.0);
    for u in [[0.0, 0.5], [1.0, 0.5], [f64::NAN, 0.5]] {
        assert!(env.sample(u).is_err());
    }
    for d in [Vec3::ZERO, Vec3::new(f64::INFINITY, 0.0, 0.0)] {
        assert!(env.uv(d).is_err());
        assert!(env.pdf_solid_angle(d).is_err());
    }
    assert_eq!(env.radiance_source_rgb(Vec3::new(f64::MAX, f64::MAX, 0.0)).unwrap(), Vec3::ZERO);
}

#[test]
fn direct_conductor_estimates_average_once_and_do_not_cross_geometry() {
    let data = decoded([[128, 64, 32, 129]; 8], "PRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n");
    let env = prepared(&data, 0.0, 1.0);
    let surface = solidcraft_viewport::surface::OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 1.0, 1.0, 1.5).unwrap();
    let mut mean = Vec3::ZERO;
    for i in 0..4096 {
        let u = [(f64::from(i) + 0.5) / 4096.0, 0.375];
        let sample = env.direct_sample_linear_srgb(u, surface, Vec3::Z, Vec3::Z, Vec3::Z).unwrap();
        mean += sample.contribution_linear_srgb / 4096.0;
    }
    // Independent white-conductor roughness=1 normal-view integral:
    // 2*pi*integral_0^1 z/(2*pi*(1+z)) dz = 1-ln(2).
    assert!((mean - Vec3::new(1.0, 0.5, 0.25) * (1.0 - 2.0_f64.ln())).len() < 3e-8);
    let blocked = env.direct_sample_linear_srgb([0.25, 0.375], surface, Vec3::Z, -Vec3::Z, Vec3::Z).unwrap();
    assert_eq!(blocked.contribution_linear_srgb, Vec3::ZERO);
    for view in [Vec3::new(0.99, 0.0, -0.1), Vec3::X] {
        let crossed = env.direct_sample_linear_srgb([0.25, 0.01], surface, Vec3::new(0.8, 0.0, 0.6), Vec3::Z, view).unwrap();
        assert_eq!(crossed.contribution_linear_srgb, Vec3::ZERO);
    }
    assert!(env.direct_sample_linear_srgb([0.25, 0.01], surface, Vec3::Z, Vec3::Z, Vec3::ZERO).is_err());
    let unknown = decoded([[128, 0, 0, 129]; 8], "");
    assert!(prepared(&unknown, 0.0, 1.0).direct_sample_linear_srgb([0.25, 0.375], surface, Vec3::Z, Vec3::Z, Vec3::Z).is_err());
}

fn importance(decoded: &DecodedHdr) -> super::super::environment::PreparedEnvironment {
    let mut builder = super::super::environment::EnvironmentBuilder::new(decoded.info().width, decoded.info().height).unwrap();
    while !builder.ready() {
        builder.step(decoded.pixels()).unwrap();
    }
    builder.finish().unwrap()
}

#[test]
fn hdr_importance_preserves_analytic_energy_and_reduces_bright_cell_variance() {
    let mut pixels = [[0; 4]; 8];
    pixels[2] = [255, 255, 255, 140];
    let data = decoded(pixels, "");
    let table = importance(&data);
    let legacy = prepared(&data, 0.0, 1.0);
    let guided = prepared(&data, 0.0, 1.0).with_importance(Some(&table)).unwrap();
    // One cell is pi/2 sr and RGBE(255,140) is 4080 in each source channel.
    let expected = 4080.0 * PI / 2.0;
    let mut legacy_mse = 0.0;
    let mut guided_mse = 0.0;
    let mut guided_mean = 0.0;
    let count = 16384;
    let mut rng = 1349u64;
    let mut random = || {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((rng >> 11) as f64 + 0.5) / 9007199254740992.0
    };
    for _ in 0..count {
        let u = [random().min(1.0_f64.next_down()), random().min(1.0_f64.next_down())];
        let baseline = legacy.sample(u).unwrap();
        let sample = guided.sample(u).unwrap();
        assert_eq!(guided.radiance_source_rgb(sample.direction).unwrap(), sample.radiance_source_rgb);
        assert_eq!(guided.pdf_solid_angle(sample.direction).unwrap(), sample.pdf_solid_angle);
        let value = sample.radiance_source_rgb.x / sample.pdf_solid_angle;
        guided_mean += value / f64::from(count);
        guided_mse += (value - expected).powi(2) / f64::from(count);
        legacy_mse += (baseline.radiance_source_rgb.x / baseline.pdf_solid_angle - expected).powi(2) / f64::from(count);
    }
    assert!((guided_mean - expected).abs() < expected * 1e-6);
    assert!(legacy_mse > expected * expected);
    assert!(guided_mse < expected * expected * 1e-12);
}

#[test]
fn hdr_importance_rotation_seams_poles_black_and_constant_cells_keep_matching_densities() {
    for pixels in [[[128, 64, 32, 129]; 8], [[0; 4]; 8], [[0; 4], [0; 4], [128, 64, 32, 129], [0; 4], [0; 4], [0; 4], [0; 4], [0; 4]]] {
        let data = decoded(pixels, "");
        let table = importance(&data);
        for rotation in [0.0, 0.55, PI / 2.0, TAU.next_down(), -0.3] {
            let env = prepared(&data, rotation, 2.0).with_importance(Some(&table)).unwrap();
            for u in [1.0 / 9007199254740992.0, 0.125, 0.25, 0.5, 0.875, 1.0_f64.next_down()] {
                for v in [1.0 / 9007199254740992.0, 0.25, 0.5, 0.875, 1.0_f64.next_down()] {
                    let sample = env.sample([u, v]).unwrap();
                    assert!(sample.pdf_solid_angle > 0.0 && sample.pdf_solid_angle.is_finite());
                    assert_eq!(env.radiance_source_rgb(sample.direction).unwrap(), sample.radiance_source_rgb, "rotation={rotation}, u={u}, v={v}");
                    assert_eq!(env.pdf_solid_angle(sample.direction).unwrap(), sample.pdf_solid_angle, "rotation={rotation}, u={u}, v={v}");
                }
            }
            for direction in [Vec3::Z, -Vec3::Z, Vec3::X, Vec3::new(1.0, -1e-12, 0.0)] {
                assert!(env.pdf_solid_angle(direction).unwrap().is_finite());
                assert!(env.radiance_source_rgb(direction).unwrap().is_finite());
            }
        }
    }
}

#[cfg(feature = "raytrace-cpu")]
#[test]
fn cpu_hdr_importance_preparation_is_admitted_cooperative_and_cancellable() {
    use crate::lighting::environment_input::EnvironmentInput;
    let input = || {
        let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
        for _ in 0..8 {
            bytes.extend_from_slice(&[128, 64, 32, 129]);
        }
        EnvironmentInput::new(HdrSource::new(bytes).unwrap(), 0.55, 2.0, 1).unwrap()
    };
    let bytes = input().required_bytes().unwrap();
    let table_bytes = super::super::environment::EnvironmentBuilder::admitted_bytes(4, 2).unwrap();
    assert_eq!(input().required_bytes_with_importance(true).unwrap(), bytes + table_bytes);
    let finish = |importance| {
        let mut state = input().begin(importance, false).unwrap();
        let mut polls = 0;
        while !state.ready() {
            assert!(state.view().is_err());
            state.prepare_one(|| false).unwrap();
            polls += 1;
            assert!(polls < 100);
        }
        (state, polls)
    };
    let (legacy, old_polls) = finish(false);
    let (guided, new_polls) = finish(true);
    assert!(new_polls >= old_polls + 16);
    let d = Vec3::new(0.3, 0.2, 0.8);
    assert_eq!(legacy.view().unwrap().radiance_linear_srgb(d).unwrap(), guided.view().unwrap().radiance_linear_srgb(d).unwrap());
    let mut cancelled = input().begin(true, false).unwrap();
    for _ in 0..old_polls {
        cancelled.prepare_one(|| false).unwrap();
    }
    assert!(!cancelled.ready());
    assert_eq!(cancelled.prepare_one(|| true).unwrap_err().code, HdrErrorCode::Cancelled);
    assert!(cancelled.view().is_err());
    let legacy_options = crate::job::RaytraceOptions::default();
    assert!(!legacy_options.environment_importance_sampling);
    assert!(serde_json::to_value(&legacy_options).unwrap().get("environment_importance_sampling").is_none());
}

#[test]
fn hdr_importance_extreme_dynamic_range_retains_the_exact_legacy_support() {
    let mut pixels = [[1, 1, 2, 1]; 8];
    pixels[0] = [128, 128, 128, 154];
    let data = decoded(pixels, "");
    let table = importance(&data);
    assert!(!table.preserves_positive_support());
    for rotation in [0.0, 0.55, -0.3] {
        let legacy = prepared(&data, rotation, 0.58);
        let fallback = prepared(&data, rotation, 0.58).with_importance(Some(&table)).unwrap();
        for u in [[0.125, 0.25], [0.5, 0.5], [0.75, 0.99], [1.0 / 9007199254740992.0, 1.0_f64.next_down()]] {
            let a = legacy.sample(u).unwrap();
            let b = fallback.sample(u).unwrap();
            assert_eq!(a.direction, b.direction);
            assert_eq!(a.radiance_source_rgb, b.radiance_source_rgb);
            assert_eq!(a.pdf_solid_angle, b.pdf_solid_angle);
            assert_eq!(fallback.pdf_solid_angle(b.direction).unwrap(), b.pdf_solid_angle);
        }
    }
}

#[cfg(feature = "raytrace-cpu")]
#[test]
fn photon_power_scan_is_cooperative_and_does_not_enable_hdr_importance() {
    use crate::lighting::environment_input::EnvironmentInput;
    let input = || {
        let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
        for _ in 0..8 {
            bytes.extend_from_slice(&[128, 128, 128, 129]);
        }
        EnvironmentInput::new(HdrSource::new(bytes).unwrap(), 0.55, 2.0, 1).unwrap()
    };
    let mut legacy = input().begin(false, false).unwrap();
    let mut weighted = input().begin(false, true).unwrap();
    let mut importance_state = input().begin(true, true).unwrap();
    let mut ticks = [0; 3];
    for (index, state) in [&mut legacy, &mut weighted, &mut importance_state].into_iter().enumerate() {
        while !state.ready() {
            state.prepare_one(|| false).unwrap();
            ticks[index] += 1;
        }
    }
    assert_eq!(ticks[1], ticks[0] + 8);
    assert!((weighted.luminance_integral().unwrap() - 8.0 * PI).abs() < 1e-12);
    assert_eq!(weighted.luminance_integral().unwrap(), importance_state.luminance_integral().unwrap());
    for sample in [[0.125, 0.25], [0.4, 0.7], [0.9, 0.1]] {
        let a = legacy.view().unwrap().sample(sample).unwrap();
        let b = weighted.view().unwrap().sample(sample).unwrap();
        assert_eq!(a.direction, b.direction);
        assert_eq!(a.radiance_source_rgb, b.radiance_source_rgb);
        assert_eq!(a.pdf_solid_angle, b.pdf_solid_angle);
    }
    let black = decoded([[0; 4]; 8], "");
    let table = importance(&black);
    assert_eq!(table.luminance_integral(), 0.0);
}
