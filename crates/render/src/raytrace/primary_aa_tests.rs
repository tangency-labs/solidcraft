//! Spatial AA regressions: WRITTEN NOT RUN. No historical gate is transferred.
use super::*;
use crate::job::{ReflectionBody, ReflectionMode, ReflectionOptions};
use crate::{ProductMesh, StandardView};
use solidcraft_geom::{Aabb3, Mesh};
use std::sync::Arc;

fn progressive_fixture(samples: u32, seed: u32, left: Option<bool>, exposure: f64) -> ImageTask {
    // Reuse the analytic half-plane camera/geometry, not fixed-AA output.
    let fixed = fixture(1, 1, left);
    let mut options = fixed.options.clone();
    options.progressive = Some(job::CpuProgressiveOptions { samples_per_pixel: samples, seed });
    options.exposure_ev = exposure;
    let mut request = fixed.request.clone();
    request.raytrace = Some(options.clone());
    ImageTask::new(fixed.scene.clone(), request, CancelToken::default(), options).unwrap()
}

type ProgressiveOutput = (Vec<(u32, Vec<u8>)>, Vec<u8>, u64);
fn progressive_complete(mut task: ImageTask, quantum: usize) -> ProgressiveOutput {
    let mut previews = Vec::new();
    for _ in 0..200_000 {
        let result = task.step_with_previews(quantum).unwrap();
        let progress = task.progress();
        assert!(progress.done <= progress.total);
        match result {
            ImagePoll::Pending(_) => {}
            ImagePoll::Preview { png, samples_per_pixel } => previews.push((samples_per_pixel, png)),
            ImagePoll::Complete(png) => return (previews, png, task.work.used()),
        }
    }
    panic!("bounded progressive fixture did not finish");
}

#[test]
fn progressive_real_png_has_stable_prefixes_quanta_and_fresh_primary_samples() {
    let (previews, final_png, work) = progressive_complete(progressive_fixture(8, 123, Some(true), 0.0), 1);
    assert_eq!(previews.iter().map(|(samples, _)| *samples).collect::<Vec<_>>(), [1, 2, 4]);
    let (other, other_final, other_work) = progressive_complete(progressive_fixture(8, 123, Some(true), 0.0), 16_384);
    assert_eq!(other, previews);
    assert_eq!(other_final, final_png);
    assert_eq!(other_work, work, "quanta cannot reset or change the cumulative budget");
    for (samples, checkpoint) in &previews {
        let (_, prefix, _) = progressive_complete(progressive_fixture(*samples, 123, Some(true), 0.0), 3);
        assert_eq!(&prefix, checkpoint, "target SPP must not alter an earlier prefix");
    }
    let mut task = progressive_fixture(8, 123, Some(true), 0.0);
    let mut rays = Vec::new();
    for _ in 0..200_000 {
        let absent = task.pending.is_none();
        let result = task.step_with_previews(1).unwrap();
        if absent && let Some(pending) = task.pending.as_ref() {
            assert!(!pending.repeatable_primary, "jittered primary samples cannot use fixed-centre reuse");
            rays.push([pending.ray.origin().x.to_bits(), pending.ray.origin().y.to_bits()]);
        }
        if matches!(result, ImagePoll::Complete(_)) {
            break;
        }
    }
    assert_eq!(rays.len(), 8);
    rays.sort();
    rays.dedup();
    assert_eq!(rays.len(), 8, "each absolute ordinal traces a new camera sample");
}

#[test]
fn progressive_coverage_keeps_backplate_outside_exposure_and_tone_mapping() {
    for exposure in [-16.0, 0.0, 16.0] {
        let (_, background, _) = progressive_complete(progressive_fixture(5, 9, None, exposure), 4);
        assert_eq!(::image::load_from_memory(&background).unwrap().to_rgba8().get_pixel(0, 0).0, [255; 4]);
        let (_, half, _) = progressive_complete(progressive_fixture(64, 9, Some(true), exposure), 4);
        let covered = (0..64).filter(|ordinal| progressive::variates(9, 0, *ordinal, 0)[0] < 0.5).count();
        assert!(covered > 0 && covered < 64);
        let expected = encode8(1.0 - covered as f64 / 64.0);
        assert_eq!(::image::load_from_memory(&half).unwrap().to_rgba8().get_pixel(0, 0).0, [expected, expected, expected, 255]);
    }
    let (previews, _, _) = progressive_complete(progressive_fixture(5, 9, Some(true), 0.0), 1);
    assert_eq!(previews.iter().map(|(samples, _)| *samples).collect::<Vec<_>>(), [1, 2, 4], "non-power-of-two target still has an exact final pass");
}

#[test]
fn progressive_resolves_the_mean_radiance_once_instead_of_averaging_display_pixels() {
    let left = fixture(1, 1, Some(true));
    let right = fixture(1, 1, Some(false));
    let mut source = left.scene.clone();
    let mut other = right.scene.meshes[0].clone();
    other.name = "Bright half".into();
    source.bounds = source.bounds.union(&other.mesh.bounds());
    source.meshes.push(other);
    let mut options = left.options.clone();
    options.reflections = None;
    options.progressive = Some(job::CpuProgressiveOptions { samples_per_pixel: 64, seed: 9 });
    options.exposure_ev = 1.0;
    options.emissions = Some(vec![
        job::SurfaceEmission { body_index: 0, body: "Black half".into(), color: [1.0, 0.5, 0.25], strength: 1.0, sides: job::EmissionSides::Both },
        job::SurfaceEmission { body_index: 1, body: "Bright half".into(), color: [1.0, 0.5, 0.25], strength: 9.0, sides: job::EmissionSides::Both },
    ]);
    let request = ImageRequest { raytrace: Some(options.clone()), ..left.request.clone() };
    let task = ImageTask::new(source, request, CancelToken::default(), options).unwrap();
    let (_, png, _) = progressive_complete(task, 4);
    let left_count = (0..64).filter(|ordinal| progressive::variates(9, 0, *ordinal, 0)[0] < 0.5).count();
    let mean = (left_count as f64 + (64 - left_count) as f64 * 9.0) / 64.0;
    let expected = [1.0, 0.5, 0.25].map(|color| {
        let exposed = mean * color * 2.0;
        encode8(exposed / (1.0 + exposed))
    });
    let actual = ::image::load_from_memory(&png).unwrap().to_rgba8().get_pixel(0, 0).0;
    assert_eq!(actual, [expected[0], expected[1], expected[2], 255]);
    let fraction = left_count as f64 / 64.0;
    let prematurely_mapped = encode8(fraction * (2.0 / 3.0) + (1.0 - fraction) * (18.0 / 19.0));
    assert_ne!(actual[0], prematurely_mapped, "each sample must remain linear until the whole pass resolves");
}

#[test]
fn progressive_film_and_resolve_share_cancellation_and_one_cumulative_work_budget() {
    let mut task = progressive_fixture(4, 0, Some(true), 0.0);
    task.work = WorkBudget::new(1).unwrap();
    assert!(matches!(task.step_with_previews(1).unwrap(), ImagePoll::Pending(_)));
    assert_eq!(task.step_with_previews(1).err().unwrap().code, ErrorCode::LimitExceeded);
    for during_resolve in [false, true] {
        let mut task = progressive_fixture(4, 0, Some(true), 0.0);
        if during_resolve {
            for _ in 0..200_000 {
                if task.progressive.as_ref().is_some_and(|p| p.resolving) {
                    break;
                }
                assert!(matches!(task.step_with_previews(1).unwrap(), ImagePoll::Pending(_)));
            }
            assert!(task.progressive.as_ref().unwrap().resolving);
        }
        task.cancel.cancel();
        assert_eq!(task.step_with_previews(4).err().unwrap().code, ErrorCode::Cancelled);
        assert!(!task.done);
    }
}

fn fixture(samples: u8, r: u8, left: Option<bool>) -> ImageTask {
    let mut camera = crate::Camera { distance: 10.0, fov: 0.0, ..Default::default() };
    camera.set_view(StandardView::Top);
    let meshes = if let Some(left) = left {
        let (x0, x1) = if left { (0.0, 0.5) } else { (0.5, 1.0) };
        let point = |x, y| {
            let (o, _) = camera.ray(x, y, 1.0, 1.0);
            Vec3::new(o.x, o.y, 0.0)
        };
        vec![ProductMesh {
            transmission: None,
            name: "Black half".into(),
            mesh: Arc::new(Mesh {
                positions: vec![point(x0, 0.0), point(x1, 0.0), point(x1, 1.0), point(x0, 1.0)],
                normals: vec![Vec3::Z; 4],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
                tri_face: vec![0, 0],
                ..Default::default()
            }),
            color: [0; 3],
            opacity: 1.0,
            surface: Some(OpaqueSurface::new(Vec3::ZERO, 1.0, 1.0, 1.5).unwrap()),
            faces: Vec::new(),
        }]
    } else {
        Vec::new()
    };
    let bounds = meshes.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.mesh.bounds()));
    let options = RaytraceOptions {
        primary_samples: samples,
        lights: Some(Vec::new()),
        shadows: false,
        reflections: left.map(|_| ReflectionOptions {
            bodies: vec![ReflectionBody { body: "Black half".into(), mode: ReflectionMode::RoughSpecular }],
            samples: r,
            max_bounces: 2,
        }),
        ..Default::default()
    };
    let request = ImageRequest {
        studio_weights: None,
        environment: None,
        textures: None,
        integrator: job::Integrator::Raytrace,
        raytrace: Some(options.clone()),
        camera,
        width: 1,
        height: 1,
        background: [255; 3],
        transparent_background: false,
    };
    ImageTask::new(ProductScene { meshes, bounds }, request, CancelToken::default(), options).unwrap()
}
fn until_emit(task: &mut ImageTask) -> usize {
    let mut primaries = 0;
    let mut previous_primary = None;
    for _ in 0..200_000 {
        let absent = task.pending.is_none();
        let before = task.work.used();
        assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
        assert!(task.work.used() - before <= 4);
        if absent && task.pending.as_ref().is_some_and(|p| p.bounce == 0) {
            primaries += 1;
            let pending = task.pending.as_ref().unwrap();
            assert!(pending.repeatable_primary, "fixed generator must certify only intra-spatial R invariance");
            let o = pending.ray.origin();
            let d = pending.ray.direction();
            let ray = [o.x.to_bits(), o.y.to_bits(), o.z.to_bits(), d.x.to_bits(), d.y.to_bits(), d.z.to_bits()];
            if let Some((pixel, spatial, previous_ray)) = previous_primary {
                if (pixel, spatial) == (task.pixel, task.spatial) {
                    assert_eq!(ray, previous_ray, "fixed primary ray changed between R attempts");
                } else {
                    assert_ne!(ray, previous_ray, "distinct spatial samples must not reuse their primary ray");
                }
            }
            previous_primary = Some((task.pixel, task.spatial, ray));
        }
        if let Some(p) = task.pending.as_ref() {
            if p.bounce != 0 {
                assert!(!p.repeatable_primary, "secondary paths must revoke primary reuse");
            } else if let Some(hit) = p.hit {
                let shared =
                    PreparedScene { bvh: task.bvh.as_ref().unwrap(), primitives: &task.looks, source: &task.scene }.resolve_hit(p.ray, hit).unwrap();
                if task.options.reflections.as_ref().is_some_and(|r| r.bodies.iter().any(|b| b.body == shared.body_name)) {
                    assert!(!p.repeatable_primary, "selected primary hit must revoke reuse even when its reflection is null");
                }
            }
        }
        assert!(task.pixels.is_empty(), "early primary hit/miss emitted pixel");
        if task.progress().phase == Phase::Running {
            assert_eq!(task.progress().done, 0, "subsamples counted as completed pixels");
        }
        if task.emit_spatial_pixel {
            return primaries;
        }
    }
    panic!("bounded AA fixture did not reach emission");
}
fn complete(mut task: ImageTask, quantum: usize) -> (Vec<u8>, u64) {
    for _ in 0..200_000 {
        if let TaskPoll::Complete(png) = task.step(quantum).unwrap() {
            return (png, task.work.used());
        }
    }
    panic!("bounded AA fixture did not finish");
}

#[test]
fn transient_options_are_strict_default_one_and_do_not_serialize_a_new_default_field() {
    let omitted: RaytraceOptions = serde_json::from_str("{}").unwrap();
    let explicit: RaytraceOptions = serde_json::from_str(r#"{"primary_samples":1}"#).unwrap();
    assert_eq!(omitted, explicit);
    assert_eq!(omitted.primary_samples, 1);
    assert!(!serde_json::to_value(&omitted).unwrap().as_object().unwrap().contains_key("primary_samples"));
    for n in [0, 2, 8, 17, 255] {
        assert!(RaytraceOptions { primary_samples: n, ..Default::default() }.validate().is_err());
    }
    for raw in [
        r#"{"primary_samples":-1}"#,
        r#"{"primary_samples":1.5}"#,
        r#"{"primary_samples":null}"#,
        r#"{"primary_samples":"4"}"#,
        r#"{"primary_samples":4,"unknown":0}"#,
    ] {
        assert!(serde_json::from_str::<RaytraceOptions>(raw).is_err());
    }
}

#[test]
fn fixed_grids_have_independent_analytic_halfplane_and_triangle_coverage() {
    assert_eq!(primary_offset(1, 0).unwrap(), [0.5, 0.5]);
    assert_eq!(primary_offset(4, 0).unwrap(), [0.25, 0.25]);
    assert_eq!(primary_offset(4, 3).unwrap(), [0.75, 0.75]);
    for (s, half, triangle) in [(4, 2, 1), (16, 8, 3)] {
        let points: Vec<_> = (0..s).map(|i| primary_offset(s, i).unwrap()).collect();
        assert!(points.iter().flatten().all(|v| *v > 0.0 && *v < 1.0));
        assert_eq!(points.iter().filter(|p| p[0] < 0.5).count(), half);
        assert_eq!(points.iter().filter(|p| p[0] + p[1] < 5.0 / 8.0).count(), triangle);
        assert!(primary_offset(s, s).is_err());
    }
    assert!(primary_offset(8, 0).is_err());
    assert!(spatial_key(u64::MAX, 0, 16).is_err());
    assert_ne!(spatial_key(7, 0, 4).unwrap(), spatial_key(7, 1, 4).unwrap());
}

#[test]
fn compositor_keeps_conditional_scene_mean_then_display_linear_coverage() {
    // White L=1, Reinhard=1/2; half coverage over black gives1/4 display-linear.
    assert_eq!(composite_spatial(Vec3::new(0.5, 0.5, 0.5), 2, 4, [0; 3], 0.0).unwrap(), [encode8(0.25), encode8(0.25), encode8(0.25), 255]);
    assert_ne!(encode8(0.25), encode8(1.0 / 3.0));
    // Full coverage of L=0 and L=2 usesT(mean)=1/2, not mean(T)=1/3.
    assert_eq!(composite_spatial(Vec3::new(1.0, 1.0, 1.0), 4, 4, [255; 3], 0.0).unwrap(), [encode8(0.5), encode8(0.5), encode8(0.5), 255]);
    for ev in [-16.0, 0.0, 16.0] {
        assert_eq!(composite_spatial(Vec3::ZERO, 0, 16, [13, 27, 41], ev).unwrap(), [13, 27, 41, 255]);
    }
    let gray = srgb8_to_linear(128);
    assert_eq!(composite_spatial(Vec3::ZERO, 2, 4, [128; 3], 0.0).unwrap(), [encode8(gray * 0.5), encode8(gray * 0.5), encode8(gray * 0.5), 255]);
    assert!(composite_spatial(Vec3::ZERO, 5, 4, [0; 3], 0.0).is_err());
    assert!(composite_spatial(Vec3::X, 0, 4, [0; 3], 0.0).is_err());
}

#[test]
fn real_hit_first_and_miss_first_silhouettes_keep_all_attempts_and_null_coverage() {
    for s in [4, 16] {
        for r in [1, 4, 16, 64] {
            for left in [false, true] {
                let mut task = fixture(s, r, Some(left));
                let primaries = until_emit(&mut task);
                assert_eq!(primaries, usize::from(s) * usize::from(r));
                assert_eq!(task.primary_coverage, u16::from(s) * u16::from(r) / 2);
                let before = task.work.used();
                task.step(1).unwrap();
                assert_eq!(task.work.used(), before + 1);
                assert_eq!(task.pixels, [encode8(0.5), encode8(0.5), encode8(0.5), 255]);
                assert_eq!(task.pixel, 1);
            }
        }
    }
}

#[test]
fn miss_only_aa_retains_exact_background_and_quantum_independent_png() {
    for s in [4, 16] {
        let mut task = fixture(s, 1, None);
        assert_eq!(until_emit(&mut task), usize::from(s));
        assert_eq!(task.primary_coverage, 0);
        let small = complete(task, 1);
        let large = complete(fixture(s, 1, None), 16384);
        assert_eq!(small, large);
        let image = ::image::load_from_memory(&small.0).unwrap().to_rgba8();
        assert_eq!(image.get_pixel(0, 0).0, [255; 4]);
    }
}

#[test]
fn spatial_finalization_and_emission_observe_shared_work_and_cancellation() {
    for emit in [false, true] {
        for cancel in [false, true] {
            let mut task = fixture(16, 4, Some(false));
            for _ in 0..200_000 {
                if (emit && task.emit_spatial_pixel) || (!emit && task.finish_spatial) {
                    break;
                }
                task.step(1).unwrap();
            }
            assert!(if emit { task.emit_spatial_pixel } else { task.finish_spatial });
            let used = task.work.used();
            if cancel {
                task.cancel.cancel();
            } else {
                task.work.limit = used;
            }
            let code = task.step(1).err().unwrap().code;
            assert_eq!(code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
            assert_eq!(task.work.used(), used);
            assert!(!task.done && task.pixels.is_empty());
        }
    }
}

#[test]
fn legacy_single_primary_ignores_aa_state_and_preserves_default_bytes_work_and_order() {
    for left in [None, Some(true), Some(false)] {
        for r in [1, 4, 16, 64] {
            let control = fixture(1, r, left);
            let mut isolated = fixture(1, r, left);
            isolated.spatial = 16;
            isolated.primary_coverage = 1024;
            isolated.spatial_radiance = Vec3::new(f64::NAN, 0.0, 0.0);
            isolated.finish_spatial = true;
            isolated.emit_spatial_pixel = true;
            assert_eq!(complete(control, 1), complete(isolated, 16384));
        }
    }
    assert!(std::mem::size_of::<ImageTask>() <= 4096);
    assert_eq!(crate::raytrace::MAX_WORK, 1_000_000_000);
    assert_eq!(job::image::MAX_JOB_BYTES, 384 * 1024 * 1024);
    let legacy = fixture(1, 4, Some(false));
    let aa = fixture(16, 4, Some(false));
    assert_eq!(legacy.admission.estimated_total_bytes, aa.admission.estimated_total_bytes);
}

#[test]
fn full_coverage_black_plane_never_leaks_display_background() {
    for samples in [4, 16] {
        let mut initial = fixture(samples, 16, Some(true));
        let top = initial.request.camera.ray(1.0, 0.0, 1.0, 1.0).0;
        let bottom = initial.request.camera.ray(1.0, 1.0, 1.0, 1.0).0;
        let mesh = Arc::get_mut(&mut initial.scene.meshes[0].mesh).unwrap();
        mesh.positions[1] = Vec3::new(top.x, top.y, 0.0);
        mesh.positions[2] = Vec3::new(bottom.x, bottom.y, 0.0);
        initial.scene.bounds = mesh.bounds();
        let mut task = ImageTask::new(initial.scene, initial.request, initial.cancel, initial.options).unwrap();
        assert_eq!(until_emit(&mut task), usize::from(samples) * 16);
        assert_eq!(task.primary_coverage, u16::from(samples) * 16);
        task.step(1).unwrap();
        assert_eq!(task.pixels, [0, 0, 0, 255]);
    }
}

#[test]
fn later_spatial_attempts_do_not_reset_work_and_never_publish_when_interrupted() {
    for cancel in [false, true] {
        let mut task = fixture(16, 64, Some(true));
        for _ in 0..200_000 {
            if task.spatial >= 1 && task.sample >= 1 {
                break;
            }
            task.step(1).unwrap();
        }
        assert!(task.spatial >= 1 && task.sample >= 1);
        let used = task.work.used();
        assert!(used > 64, "no cumulative work across spatial samples");
        if cancel {
            task.cancel.cancel();
        } else {
            task.work.limit = used;
        }
        assert_eq!(task.step(1).err().unwrap().code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
        assert_eq!(task.work.used(), used);
        assert!(!task.done && task.pixels.is_empty());
    }
}

#[test]
fn actual_colored_lit_partial_coverage_wires_positive_radiance_exposure_and_background() {
    for samples in [4, 16] {
        for left in [false, true] {
            let mut initial = fixture(samples, 1, Some(left));
            initial.scene.meshes[0].surface = Some(OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 1.0, 1.0, 1.5).unwrap());
            initial.options.reflections = None;
            initial.options.lights = Some(vec![job::DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [0.25, 0.5, 1.0], intensity: 2.0 }]);
            initial.options.exposure_ev = 1.0;
            initial.request.background = [13, 73, 139];
            initial.request.raytrace = Some(initial.options.clone());
            let mut task = ImageTask::new(initial.scene, initial.request, initial.cancel, initial.options).unwrap();
            assert_eq!(until_emit(&mut task), usize::from(samples));
            assert_eq!(task.primary_coverage, u16::from(samples) / 2);
            // At normal incidence, unit-alpha white conductor: D=1/pi,
            // G1=F=1, f=1/(4pi), incoming radiance=2*colored light.
            let linear = [0.5, 1.0, 2.0].map(|v| v / (4.0 * std::f64::consts::PI));
            let expected_p = Vec3::new(linear[0], linear[1], linear[2]) * 0.5;
            assert!((task.spatial_radiance - expected_p).len() < 1e-12);
            let expected: [u8; 3] = std::array::from_fn(|i| {
                let exposed = linear[i] * 2.0;
                let display = 0.5 * exposed / (1.0 + exposed) + 0.5 * srgb8_to_linear([13, 73, 139][i]);
                encode8(display)
            });
            task.step(1).unwrap();
            assert_eq!(task.pixels, [expected[0], expected[1], expected[2], 255]);
        }
    }
}

#[test]
fn workstation_progressive_resolves_every_checkpoint_through_its_admitted_cap() {
    let fixed = fixture(1, 1, Some(true));
    let cap = job::CpuBudget::Workstation.limits().max_samples_per_pixel;
    assert_eq!(cap, 4096);
    for samples in [2048, cap] {
        let mut options = fixed.options.clone();
        options.budget = job::CpuBudget::Workstation;
        options.progressive = Some(job::CpuProgressiveOptions { samples_per_pixel: samples, seed: 123 });
        let request = ImageRequest { raytrace: Some(options.clone()), ..fixed.request.clone() };
        let task = ImageTask::new(fixed.scene.clone(), request, CancelToken::default(), options).unwrap();
        let (previews, final_png, _) = progressive_complete(task, 16_384);
        let expected: Vec<_> = (0..samples.ilog2()).map(|power| 1_u32 << power).collect();
        assert_eq!(previews.iter().map(|(n, _)| *n).collect::<Vec<_>>(), expected);
        for (_, png) in &previews {
            assert_eq!(::image::load_from_memory(png).unwrap().to_rgba8().dimensions(), (1, 1));
        }
        assert_eq!(::image::load_from_memory(&final_png).unwrap().to_rgba8().dimensions(), (1, 1));
    }
    assert!(composite_spatial(Vec3::ZERO, 0, 4097, [0; 3], 0.0).is_err());
}

#[test]
fn area_light_points_include_spatial_and_film_sample_identity() {
    let light = super::super::area::RectangleLight::new(Vec3::Z * 10.0, Vec3::X * 4.0, -Vec3::Y * 4.0, Vec3::new(1.0, 1.0, 1.0), 1.0).unwrap();
    let mut keys = std::collections::BTreeSet::new();
    let mut points = std::collections::BTreeSet::new();
    for film in [0, 1, 1023, 2047, 4095] {
        for spatial in 0..16 {
            let key = area_sample_key(41, spatial, 16, film).unwrap();
            assert!(keys.insert(key));
            let point = light.sample(Vec3::ZERO, key, 2, 0, 4, 73).unwrap().unwrap().position();
            assert!(points.insert((point.x.to_bits(), point.y.to_bits())));
            assert_eq!(light.sample(Vec3::ZERO, key, 2, 0, 4, 73).unwrap().unwrap().position(), point);
        }
    }
    assert_eq!(area_sample_key(41, 0, 1, 0).unwrap(), 41);
    assert!(area_sample_key(41, 0, 1, 4096).is_err());
    assert!(area_sample_key(41, 4, 4, 0).is_err());
    assert!(area_sample_key(u64::MAX, 0, 4, 0).is_err());
}
