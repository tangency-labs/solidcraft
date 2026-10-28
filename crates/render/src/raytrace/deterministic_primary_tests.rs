use super::*;
use crate::job::{ReflectionBody, ReflectionMode, ReflectionOptions};
use crate::lighting::{HdrSource, environment_input::EnvironmentInput};
use solidcraft_geom::{Aabb3, Mesh};
use std::sync::Arc;

fn environment() -> EnvironmentInput {
    let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
    for _ in 0..8 {
        bytes.extend_from_slice(&[128, 64, 32, 129]);
    }
    EnvironmentInput::new(HdrSource::new(bytes).unwrap(), 0.0, 1.0, 4).unwrap()
}

fn scene(face_override: bool) -> ProductScene {
    let mut meshes = Vec::new();
    for (name, x) in [("Visible", 0.0), ("Hidden", 1000.0)] {
        let mesh = Mesh {
            positions: vec![
                Vec3::new(x - 20.0, -20.0, 0.0),
                Vec3::new(x + 20.0, -20.0, 0.0),
                Vec3::new(x + 20.0, 20.0, 0.0),
                Vec3::new(x - 20.0, 20.0, 0.0),
            ],
            normals: vec![Vec3::Z; 4],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            tri_face: vec![0, 0],
            ..Default::default()
        };
        meshes.push(crate::ProductMesh {
            transmission: None,
            name: name.into(),
            mesh: Arc::new(mesh),
            color: [255; 3],
            opacity: 1.0,
            surface: Some(OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 1.0, 1.0, 1.5).unwrap()),
            faces: Vec::new(),
        });
    }
    if face_override {
        meshes[0].faces.push(crate::FaceAppearance {
            transmission: None,
            face: 0,
            color: [0; 3],
            opacity: 1.0,
            surface: Some(OpaqueSurface::new(Vec3::ZERO, 1.0, 1.0, 1.5).unwrap()),
        });
    }
    let bounds = meshes.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.mesh.bounds()));
    ProductScene { meshes, bounds }
}

fn task(samples: u8, selected: bool, face_override: bool, shadows: bool) -> ImageTask {
    let mut camera = crate::Camera { distance: 50.0, ..Default::default() };
    camera.set_view(crate::StandardView::Top);
    let options = RaytraceOptions {
        lights: Some(Vec::new()),
        shadows,
        reflections: Some(ReflectionOptions {
            bodies: vec![ReflectionBody { body: if selected { "Visible" } else { "Hidden" }.into(), mode: ReflectionMode::RoughSpecular }],
            max_bounces: 2,
            samples,
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
        background: [13, 27, 41],
        transparent_background: false,
    };
    ImageTask::new_with_environment(scene(face_override), request, CancelToken::default(), options, Some(environment())).unwrap()
}

fn render(mut task: ImageTask, revoke_fixed_primary_proof: bool) -> (Vec<u8>, u64, usize) {
    let mut primaries = 0;
    for _ in 0..200_000 {
        // Test-only reference: revoking the generator proof keeps the original R
        // path/accumulation. No product switch, sampler, work limit or input changes.
        if revoke_fixed_primary_proof && let Some(p) = task.pending.as_mut() {
            p.repeatable_primary = false;
        }
        let absent = task.pending.is_none();
        match task.step(1).unwrap() {
            TaskPoll::Complete(png) => return (png, task.work.used(), primaries),
            TaskPoll::Pending(_) => {
                if absent && task.pending.is_some() {
                    primaries += 1;
                }
            }
        }
    }
    panic!("bounded single-pixel fixture did not finish");
}

#[test]
fn nonselected_fixed_primary_matches_analytic_color_and_legacy_r_with_less_work() {
    for samples in [1, 4, 16, 64] {
        for shadows in [false, true] {
            let (fast, work, primaries) = render(task(samples, false, false, shadows), false);
            let (legacy, legacy_work, legacy_primaries) = render(task(samples, false, false, shadows), true);
            assert_eq!(fast, legacy, "same radiance without repeated identical primary samples");
            assert_eq!(primaries, 1);
            assert_eq!(legacy_primaries, usize::from(samples));
            if samples == 1 {
                assert_eq!(work, legacy_work);
            } else {
                assert!(work < legacy_work);
            }
            if !shadows {
                // White roughness-one conductor, constant RGB(1,.5,.25), fixed
                // 2x2 solid-angle strata: two z=.5 contributions average to1/3.
                let expected = [1.0 / 3.0, 1.0 / 6.0, 1.0 / 12.0].map(|v| encode(v / (1.0 + v)));
                let image = ::image::load_from_memory(&fast).unwrap().to_rgba8();
                assert_eq!(image.get_pixel(0, 0).0, [expected[0], expected[1], expected[2], 255]);
            }
        }
    }
}

#[test]
fn selected_rough_primary_retains_every_sample_including_null_attempts() {
    for samples in [4, 16, 64] {
        let (actual, work, primaries) = render(task(samples, true, false, false), false);
        let (reference, reference_work, reference_primaries) = render(task(samples, true, false, false), true);
        assert_eq!(actual, reference);
        assert_eq!(work, reference_work);
        assert_eq!(primaries, usize::from(samples));
        assert_eq!(primaries, reference_primaries);
    }
}

#[test]
fn captured_face_override_is_resolved_before_nonselected_fast_path_finishes() {
    let (png, work, count) = render(task(64, false, true, false), false);
    let (reference, reference_work, reference_count) = render(task(64, false, true, false), true);
    assert_eq!(png, reference);
    assert_eq!(::image::load_from_memory(&png).unwrap().to_rgba8().get_pixel(0, 0).0, [0, 0, 0, 255]);
    assert!(work < reference_work);
    assert_eq!(count, 1);
    assert_eq!(reference_count, 64);
}

#[test]
fn nonselected_visibility_keeps_shared_cancel_and_work_limit_refusal() {
    for cancel_now in [false, true] {
        let mut task = task(64, false, false, true);
        for _ in 0..10000 {
            assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
            if task.pending.as_ref().is_some_and(|p| p.environment_cursor.is_some()) {
                break;
            }
        }
        assert!(task.pending.as_ref().is_some_and(|p| p.environment_cursor.is_some()));
        if cancel_now {
            task.cancel.cancel();
        } else {
            task.work.limit = task.work.used();
        }
        assert_eq!(task.step(1).err().unwrap().code, if cancel_now { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
        assert!(!task.done);
    }
}

#[test]
fn selected_sampling_cancellation_does_not_turn_partial_samples_into_png() {
    let mut task = task(64, true, false, false);
    for _ in 0..10000 {
        assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
        if task.sample >= 1 {
            break;
        }
    }
    assert!(task.sample >= 1 && task.sample < 64);
    task.cancel.cancel();
    assert_eq!(task.step(1).err().unwrap().code, ErrorCode::Cancelled);
    assert!(!task.done);
}

#[test]
fn reused_shading_preserves_original_per_add_radiance_bound() {
    let mut task = task(64, false, false, false);
    for _ in 0..10000 {
        assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
        if task.pending.as_ref().is_some_and(|p| p.hit.is_some() && p.repeatable_primary) {
            break;
        }
    }
    assert!(task.pending.as_ref().is_some_and(|p| p.hit.is_some() && p.repeatable_primary));
    let error = task.finish_sample(Vec3::new(1e12 / 32.0, 0.0, 0.0)).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidScene);
    assert_eq!(task.sample, 32);
    assert!(!task.done);
    assert!(task.pixels.is_empty());
}

#[test]
fn request_admission_and_task_slot_caps_are_not_expanded() {
    let fast = task(64, false, false, false);
    let legacy = task(64, false, false, false);
    assert_eq!(fast.options, legacy.options);
    assert_eq!(fast.admission.estimated_total_bytes, legacy.admission.estimated_total_bytes);
    assert_eq!(fast.options.reflections.as_ref().unwrap().samples, 64);
    assert!(std::mem::size_of::<ImageTask>() <= 4096);
    assert_eq!(crate::raytrace::MAX_WORK, 1_000_000_000);
    assert_eq!(job::image::MAX_JOB_BYTES, 384 * 1024 * 1024);
}

// Fixed spatial-generator reuse regressions. Source written; execution pending.

fn spatial_reuse_fixture(s: u8, r: u8, selected: bool, face_override: bool, shadows: bool) -> ImageTask {
    let mut camera = crate::Camera { distance: 50.0, ..Default::default() };
    camera.set_view(crate::StandardView::Top);
    let options = RaytraceOptions {
        primary_samples: s,
        lights: Some(Vec::new()),
        shadows,
        reflections: Some(ReflectionOptions {
            bodies: vec![ReflectionBody { body: if selected { "Visible" } else { "Hidden" }.into(), mode: ReflectionMode::RoughSpecular }],
            max_bounces: 2,
            samples: r,
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
        background: [13, 27, 41],
        transparent_background: false,
    };
    let mut scene = scene(face_override);
    // Explicit full coverage fixture; hidden selected body remains far off-axis.
    let visible = Arc::get_mut(&mut scene.meshes[0].mesh).unwrap();
    for p in &mut visible.positions {
        p.x *= 5.0;
        p.y *= 5.0;
    }
    scene.bounds = scene.meshes.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.mesh.bounds()));
    ImageTask::new_with_environment(scene, request, CancelToken::default(), options, Some(environment())).unwrap()
}

type SpatialBoundary = (u8, u8, u16, [u64; 6]);
fn spatial_reference_render(mut task: ImageTask, revoke: bool) -> (Vec<u8>, Vec<SpatialBoundary>) {
    let mut boundaries = Vec::new();
    for _ in 0..1_000_000 {
        // Exactly the established reference technique. Revoke before every step,
        // including BEFORE the first completed shade could retain a cache entry.
        if revoke && let Some(p) = task.pending.as_mut() {
            p.repeatable_primary = false;
        }
        let before = task.work.used();
        let previous = (task.pixel, task.spatial, task.sample);
        let poll = task.step(1).unwrap();
        assert!(task.work.used() - before <= 4);
        if revoke {
            assert!(
                task.pending.as_ref().is_none_or(|p| p.light <= task.lights.lights().len()),
                "revoked-proof reference retained a completed cache entry"
            );
        }
        if (task.pixel, task.spatial) == (previous.0, previous.1) && task.sample > previous.2 {
            let p = task.pixel_radiance;
            let s = task.spatial_radiance;
            boundaries.push((
                task.spatial,
                task.sample,
                task.primary_coverage,
                [p.x.to_bits(), p.y.to_bits(), p.z.to_bits(), s.x.to_bits(), s.y.to_bits(), s.z.to_bits()],
            ));
        }
        if let TaskPoll::Complete(png) = poll {
            return (png, boundaries);
        }
    }
    panic!("bounded actual-consumer spatial fixture did not finish");
}

#[test]
fn intra_spatial_nonselected_reuse_matches_uncached_ordered_sums_coverage_and_png() {
    for s in [4, 16] {
        for r in [1, 4, 16, 64] {
            for shadows in [false, true] {
                for face_override in [false, true] {
                    let fast = spatial_reference_render(spatial_reuse_fixture(s, r, false, face_override, shadows), false);
                    let reference = spatial_reference_render(spatial_reuse_fixture(s, r, false, face_override, shadows), true);
                    assert_eq!(fast, reference, "exact actual ImageTask PNG and per-S accumulator bits/coverage");
                    assert_eq!(fast.1.len(), usize::from(s) * usize::from(r));
                    for (ordinal, boundary) in fast.1.iter().enumerate() {
                        assert_eq!(boundary.0, (ordinal / usize::from(r)) as u8);
                        assert_eq!(boundary.1, (ordinal % usize::from(r)) as u8 + 1);
                        assert_eq!(boundary.2, ordinal as u16 + 1);
                    }
                    if !face_override {
                        assert!(
                            fast.1.iter().any(|b| b.3[..3].iter().any(|v| f64::from_bits(*v) > 0.0)),
                            "HDR actual-consumer comparison must include positive radiance"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn completed_spatial_reuse_checks_cancel_and_work_limit_before_any_mutation() {
    for cancel in [false, true] {
        for ordinal in [1, 2] {
            let mut task = spatial_reuse_fixture(16, 64, false, false, true);
            for _ in 0..1_000_000 {
                if task.sample == ordinal
                    && task
                        .pending
                        .as_ref()
                        .is_some_and(|p| p.repeatable_primary && p.bounce == 0 && p.hit.is_some() && p.light == task.lights.lights().len() + 1)
                {
                    break;
                }
                task.step(1).unwrap();
            }
            assert_eq!(task.sample, ordinal);
            assert!(task.pending.as_ref().is_some_and(|p| p.light == task.lights.lights().len() + 1));
            let before = (task.work.used(), task.sample, task.primary_coverage, task.pixel_radiance);
            if cancel {
                task.cancel.cancel();
            } else {
                task.work.limit = task.work.used();
            }
            assert_eq!(task.step(1).err().unwrap().code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
            assert_eq!((task.work.used(), task.sample, task.primary_coverage, task.pixel_radiance), before);
            assert!(!task.done && task.pixels.is_empty());
        }
    }
}
// Bits: directional, point, spot, rectangle, lowered LED, HDR. Every source can
// be isolated, preventing an all-black or one-source-only comparison from
// masquerading as combined-consumer coverage. Inputs are original fixtures.
fn spatial_lights_fixture(config: (u8, u8, u32, u32, bool), mask: u8, width: u32, partial: bool) -> ImageTask {
    let (s, r, n, seed, shadows) = config;
    let previous = spatial_reuse_fixture(s, r, false, false, shadows);
    let mut options = previous.options;
    options.exposure_ev = 1.0;
    options.lights = Some(if mask & 1 != 0 {
        vec![job::DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [0.25, 0.5, 1.0], intensity: 2.0 }]
    } else {
        Vec::new()
    });
    let point = job::PunctualLightOptions {
        position_mm: [0.0, 0.0, 40.0],
        color: [1.0, 0.5, 0.25],
        intensity: 1.0,
        reference_distance_mm: 40.0,
        minimum_distance_mm: 0.001,
        cone: None,
    };
    let mut punctual = Vec::new();
    if mask & 2 != 0 {
        punctual.push(point);
    }
    if mask & 4 != 0 {
        punctual.push(job::PunctualLightOptions {
            color: [0.25, 1.0, 0.5],
            cone: Some(job::SpotConeOptions { emitted_axis: [0.0, 0.0, -1.0], inner_half_angle_radians: 0.7, outer_half_angle_radians: 1.4 }),
            ..point
        });
    }
    options.punctual_lights = if punctual.is_empty() { None } else { Some(punctual) };
    if mask & 8 != 0 {
        options.area_lights = Some(vec![job::RectangleLightOptions {
            center_mm: [-5.0, 0.0, 50.0],
            edge_u_mm: [20.0, 0.0, 0.0],
            edge_v_mm: [0.0, -20.0, 0.0],
            color: [0.5, 0.25, 1.0],
            intensity: 3.0,
        }]);
        options.area_samples = Some(n);
    }
    if mask & 16 != 0 {
        job::led_strip::LedStripOptions {
            center_mm: [5.0, 0.0, 50.0],
            length_mm: 20.0,
            width_mm: 2.0,
            length_axis: [1.0, 0.0, 0.0],
            width_axis: [0.0, -1.0, 0.0],
            color: [1.0, 0.25, 0.5],
            intensity: 4.0,
        }
        .append_to(&mut options, n)
        .unwrap();
    }
    if options.area_lights.is_some() {
        options.seed = Some(seed);
    }
    let mut scene = previous.scene;
    if partial {
        let mesh = Arc::get_mut(&mut scene.meshes[0].mesh).unwrap();
        mesh.positions[0].x = 0.0;
        mesh.positions[3].x = 0.0;
        scene.bounds = scene.meshes.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.mesh.bounds()));
    }
    let request = ImageRequest { width, raytrace: Some(options.clone()), ..previous.request };
    ImageTask::new_with_environment(scene, request, previous.cancel, options, if mask & 32 != 0 { Some(environment()) } else { None }).unwrap()
}

#[test]
fn spatial_reuse_actual_nonzero_light_consumers_and_seeded_union_match_reference() {
    for mask in [1, 2, 4, 8, 16, 32, 63] {
        let config = (4, 4, 4, 7, false);
        let fast = spatial_reference_render(spatial_lights_fixture(config, mask, 1, false), false);
        let reference = spatial_reference_render(spatial_lights_fixture(config, mask, 1, false), true);
        assert_eq!(fast, reference, "actual isolated source mask {mask}");
        assert!(fast.1.iter().any(|b| b.3[..3].iter().any(|v| f64::from_bits(*v) > 0.0)), "source mask {mask} contributed no radiance");
    }
    // Finite pairwise matrix, not a timing benchmark. R64 is covered by the HDR
    // matrix above; every allowed rectangle N and two nonzero seeds are explicit.
    for (s, r, n) in [(4, 4, 1), (16, 4, 4), (4, 16, 16), (4, 4, 64)] {
        for seed in [7, 0xfedc_ba98] {
            for shadows in [false, true] {
                let config = (s, r, n, seed, shadows);
                assert_eq!(
                    spatial_reference_render(spatial_lights_fixture(config, 63, 1, false), false),
                    spatial_reference_render(spatial_lights_fixture(config, 63, 1, false), true)
                );
            }
        }
    }
}

#[test]
fn mixed_pixels_spatial_hits_misses_and_independent_jobs_do_not_share_reuse() {
    for s in [4, 16] {
        for shadows in [false, true] {
            let config = (s, 4, 4, 7, shadows);
            let fast = spatial_reference_render(spatial_lights_fixture(config, 63, 3, true), false);
            let reference = spatial_reference_render(spatial_lights_fixture(config, 63, 3, true), true);
            assert_eq!(fast, reference);
            assert_eq!(fast.1.len(), 3 * usize::from(s) * 4);
            assert!(fast.1.iter().any(|b| b.2 == 0), "fixture must include primary misses");
            assert!(fast.1.iter().any(|b| b.2 > 0), "fixture must include primary hits");
            assert!(fast.1.iter().any(|b| b.0 == s - 1 && b.1 == 4 && b.2 == u16::from(s) * 2), "middle pixel must retain half spatial coverage");
            // A new captured job with different sources/seed/coverage has its own
            // actual reference; prior jobs must not contribute state to it.
            let changed = (s, 4, 16, 0xfedc_ba98, shadows);
            assert_eq!(
                spatial_reference_render(spatial_lights_fixture(changed, 16, 2, false), false),
                spatial_reference_render(spatial_lights_fixture(changed, 16, 2, false), true)
            );
        }
    }
}

fn at_first_completed_spatial_primary() -> ImageTask {
    let mut task = spatial_reuse_fixture(4, 4, false, false, true);
    for _ in 0..1_000_000 {
        if task.sample == 1 && task.pending.as_ref().is_some_and(|p| p.light == task.lights.lights().len() + 1) {
            return task;
        }
        task.step(1).unwrap();
    }
    panic!("actual shading did not reach reusable completion");
}

#[test]
fn reused_r_addition_enforces_finite_bound_and_coverage_before_publication() {
    for corrupt in [0, 1, 2, 3] {
        let mut task = at_first_completed_spatial_primary();
        match corrupt {
            0 => {
                task.pixel_radiance = Vec3::new(1e12, 0.0, 0.0);
                task.pending.as_mut().unwrap().radiance = Vec3::new(1.0, 0.0, 0.0);
            }
            1 => {
                task.pending.as_mut().unwrap().radiance = Vec3::new(f64::NAN, 0.0, 0.0);
            }
            2 => {
                task.pending.as_mut().unwrap().radiance = Vec3::new(f64::INFINITY, 0.0, 0.0);
            }
            _ => {
                task.primary_coverage = 1024;
            }
        }
        assert_eq!(task.step(1).err().unwrap().code, ErrorCode::InvalidScene);
        assert!(!task.done && task.pixels.is_empty());
    }
}

#[test]
fn completed_marker_refuses_malformed_proof_sample_bounce_hit_and_shading_state() {
    for corrupt in 0..19 {
        let mut task = at_first_completed_spatial_primary();
        match corrupt {
            0 => task.pending.as_mut().unwrap().repeatable_primary = false,
            1 => task.pending.as_mut().unwrap().bounce = 1,
            2 => task.pending.as_mut().unwrap().hit = None,
            3 => task.sample = 0,
            4 => task.sample = task.sample_count(),
            5 => task.pending.as_mut().unwrap().punctual_shadow = true,
            6 => task.pending.as_mut().unwrap().area_ready = true,
            7 => task.pending.as_mut().unwrap().environment_sample = 0,
            8 => task.pending.as_mut().unwrap().area_light = 1,
            9 => task.options.reflections.as_mut().unwrap().bodies[0].body = "Visible".into(),
            10 => task.sample = task.sample_count() + 1,
            11 => task.pending.as_mut().unwrap().light = task.lights.lights().len() + 2,
            12 => task.finish_spatial = true,
            13 => task.emit_spatial_pixel = true,
            14 => task.spatial = task.options.primary_samples,
            15 => task.pending.as_mut().unwrap().emission_cursor = 1,
            16 => task.pending.as_mut().unwrap().surface_cursor = 1,
            17 => task.pending.as_mut().unwrap().surface_ready = false,
            _ => task.pending.as_mut().unwrap().surface_index = Some(0),
        }
        assert_eq!(task.step(1).err().unwrap().code, ErrorCode::InvalidScene);
        assert!(!task.done && task.pixels.is_empty());
    }
}
#[test]
fn spatial_reuse_observes_cancellation_and_limit_at_shading_finalization_and_emission() {
    for phase in 0..3 {
        for cancel in [false, true] {
            let mut task = spatial_reuse_fixture(4, 4, false, false, true);
            let reached = |task: &ImageTask| match phase {
                0 => task.sample == 0 && task.pending.as_ref().is_some_and(|p| p.hit.is_some() && p.environment_sample == 0),
                1 => task.finish_spatial,
                _ => task.emit_spatial_pixel,
            };
            for _ in 0..200_000 {
                if reached(&task) {
                    break;
                }
                task.step(1).unwrap();
            }
            assert!(reached(&task));
            let before = task.work.used();
            if cancel {
                task.cancel.cancel();
            } else {
                task.work.limit = before;
            }
            assert_eq!(task.step(1).err().unwrap().code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
            assert_eq!(task.work.used(), before);
            assert!(!task.done && task.pixels.is_empty());
        }
    }
}

fn spatial_quantum_png(mut task: ImageTask, quantum: usize) -> Vec<u8> {
    for _ in 0..1_000_000 {
        let used = task.work.used();
        let poll = task.step(quantum).unwrap();
        assert!(task.work.used() - used <= 4, "public work quantum exceeded");
        if let TaskPoll::Complete(png) = poll {
            return png;
        }
    }
    panic!("bounded spatial quantum fixture did not finish");
}

#[test]
fn spatial_reuse_quantum_partitions_preserve_complete_png_and_public_work_bound() {
    for s in [4, 16] {
        let fixture = || spatial_lights_fixture((s, 4, 4, 7, true), 63, 3, true);
        let reference = spatial_reference_render(fixture(), true).0;
        for quantum in [1, 4, 16384] {
            assert_eq!(spatial_quantum_png(fixture(), quantum), reference);
        }
    }
}

fn spatial_surface_fixture(s: u8, r: u8, selected: bool) -> ImageTask {
    let previous = spatial_lights_fixture((s, r, 4, 7, true), 63, 2, true);
    let mut options = previous.options;
    options.surface_effects = Some(vec![crate::surface_patterns::SurfaceEffect {
        body: "Visible".into(),
        frame: Default::default(),
        repeat_mm: [20.0, 27.0, 31.0],
        roughness_amplitude: 0.2,
        bump_amplitude_mm: 0.4,
    }]);
    if selected {
        options.reflections.as_mut().unwrap().bodies[0].body = "Visible".into();
    }
    let request = ImageRequest { raytrace: Some(options.clone()), ..previous.request };
    ImageTask::new_with_environment(previous.scene, request, previous.cancel, options, Some(environment())).unwrap()
}

#[test]
fn surface_shared_hit_preserves_aa_ordered_prefixes_coverage_and_completed_png() {
    for s in [4, 16] {
        for r in [1, 4, 16] {
            for selected in [false, true] {
                let expected = spatial_reference_render(spatial_surface_fixture(s, r, selected), true);
                let actual = spatial_reference_render(spatial_surface_fixture(s, r, selected), false);
                assert_eq!(actual, expected, "S={s}, R={r}, selected={selected}");
                for quantum in [4, 16384] {
                    assert_eq!(spatial_quantum_png(spatial_surface_fixture(s, r, selected), quantum), expected.0);
                }
            }
        }
    }
}
