use super::*;
use crate::lighting::{HdrSource, environment_input::EnvironmentInput};
use solidcraft_geom::{Aabb3, Mesh};
use std::sync::Arc;

fn input() -> EnvironmentInput {
    let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
    for _ in 0..8 {
        bytes.extend_from_slice(&[128, 64, 32, 129]);
    }
    EnvironmentInput::new(HdrSource::new(bytes).unwrap(), 0.0, 1.0, 64).unwrap()
}
fn scene() -> ProductScene {
    let mesh = Arc::new(Mesh {
        positions: vec![Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 0.0), Vec3::new(-10.0, 10.0, 0.0)],
        normals: vec![Vec3::Z; 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        tri_face: vec![0, 0],
        ..Default::default()
    });
    ProductScene {
        bounds: mesh.bounds(),
        meshes: vec![crate::ProductMesh {
            transmission: None,
            name: "Original conductor plate".into(),
            mesh,
            color: [255; 3],
            opacity: 1.0,
            surface: Some(OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 1.0, 1.0, 1.5).unwrap()),
            faces: Vec::new(),
        }],
    }
}
fn request() -> ImageRequest {
    let mut camera = crate::Camera { distance: 50.0, ..Default::default() };
    camera.set_view(crate::StandardView::Top);
    ImageRequest {
        studio_weights: None,
        environment: None,
        textures: None,
        integrator: job::Integrator::Raytrace,
        raytrace: Some(RaytraceOptions { lights: Some(Vec::new()), ..Default::default() }),
        camera,
        width: 9,
        height: 9,
        background: [13, 27, 41],
        transparent_background: false,
    }
}
fn png(source: ProductScene, environment: Option<EnvironmentInput>) -> Vec<u8> {
    let request = request();
    let options = request.raytrace.clone().unwrap();
    let mut task = ImageTask::new_with_environment(source, request, CancelToken::default(), options, environment).unwrap();
    for _ in 0..200_000 {
        if let TaskPoll::Complete(png) = task.step(16384).unwrap() {
            return png;
        }
    }
    panic!("bounded environment fixture did not complete");
}

#[test]
fn real_cpu_png_conductor_matches_independent_environment_quadrature_once() {
    let baseline = ::image::load_from_memory(&png(scene(), None)).unwrap().to_rgba8();
    let rendered = png(scene(), Some(input()));
    assert_eq!(rendered, png(scene(), Some(input())));
    let image = ::image::load_from_memory(&rendered).unwrap().to_rgba8();
    // Independent roughness-one white-conductor f*cos= z/(2*pi*(1+z)).
    // The fixed 8x8 solid-angle strata integrate 4 positive z rows, each 8 samples.
    let factor = [0.125, 0.375, 0.625, 0.875].into_iter().map(|z| z / (1.0 + z)).sum::<f64>() / 4.0;
    let expected = [factor, factor * 0.5, factor * 0.25].map(|r| encode8(r / (1.0 + r)));
    assert_eq!(image.get_pixel(4, 4).0, [expected[0], expected[1], expected[2], 255]);
    assert_eq!(baseline.get_pixel(4, 4).0, [0, 0, 0, 255]);
    let empty = ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY };
    let background = ::image::load_from_memory(&png(empty, Some(input()))).unwrap().to_rgba8();
    assert!(background.pixels().all(|p| p.0 == [13, 27, 41, 255]));
}

struct ProgressiveHdrResult {
    previews: Vec<(u32, Vec<u8>)>,
    png: Vec<u8>,
    work: u64,
}

fn progressive_hdr_png(samples_per_pixel: u32, seed: u32, quantum: usize, environment: Option<EnvironmentInput>) -> ProgressiveHdrResult {
    let mut request = request();
    let mut options = request.raytrace.clone().unwrap();
    options.progressive = Some(job::CpuProgressiveOptions { samples_per_pixel, seed });
    options.reflections = Some(job::ReflectionOptions {
        bodies: vec![job::ReflectionBody { body: "Original conductor plate".into(), mode: job::ReflectionMode::RoughSpecular }],
        samples: 1,
        max_bounces: 2,
    });
    request.raytrace = Some(options.clone());
    let mut task = ImageTask::new_with_environment(scene(), request, CancelToken::default(), options, environment).unwrap();
    let mut previews = Vec::new();
    for _ in 0..300_000 {
        match task.step_with_previews(quantum).unwrap() {
            ImagePoll::Pending(_) => {}
            ImagePoll::Preview { png, samples_per_pixel } => previews.push((samples_per_pixel, png)),
            ImagePoll::Complete(png) => return ProgressiveHdrResult { previews, png, work: task.work.used() },
        }
    }
    panic!("bounded progressive HDR fixture did not finish");
}

fn progressive_hdr_expected(samples: u32, seed: u32) -> [u8; 4] {
    // Independent normal-view, white-conductor reference at roughness one.
    // The centre pixel (40) stays inside the plate for every camera jitter.
    // GGX gives z = 1 - 2*u and weight = 2*z/(1+z) for z > 0; other
    // attempts are null events, not replacement samples. Only the variate
    // sequence is shared with production: no BSDF, PDF or film code is used.
    let sum: f64 = (0..samples)
        .map(|ordinal| {
            let z = 1.0 - 2.0 * progressive::variates(seed, 40, ordinal, 0x100)[0];
            if z > 0.0 { 2.0 * z / (1.0 + z) } else { 0.0 }
        })
        .sum();
    let mean = sum / f64::from(samples);
    // The original HDR is constant linear RGB (1, 0.5, 0.25). Average in
    // linear space over ALL attempts, then apply the display response once.
    let rgb = [mean, mean * 0.5, mean * 0.25].map(|r| encode8(r / (1.0 + r)));
    [rgb[0], rgb[1], rgb[2], 255]
}

fn centre(png: &[u8]) -> [u8; 4] {
    ::image::load_from_memory(png).unwrap().to_rgba8().get_pixel(4, 4).0
}

#[test]
fn progressive_hdr_and_rough_reflections_keep_the_same_absolute_prefix_at_every_quantum() {
    for (target, checkpoints) in [(3, vec![1, 2]), (7, vec![1, 2, 4])] {
        let result = progressive_hdr_png(target, 82, 1, Some(input()));
        assert_eq!(result.previews.iter().map(|(n, _)| *n).collect::<Vec<_>>(), checkpoints);
        let other = progressive_hdr_png(target, 82, 16384, Some(input()));
        assert_eq!(result.previews, other.previews);
        assert_eq!(result.png, other.png);
        assert_eq!(result.work, other.work);
        for (samples, checkpoint) in result.previews {
            assert_eq!(progressive_hdr_png(samples, 82, 3, Some(input())).png, checkpoint);
            assert_eq!(centre(&checkpoint), progressive_hdr_expected(samples, 82));
        }
        assert_eq!(centre(&result.png), progressive_hdr_expected(target, 82));
    }
}

#[test]
fn progressive_hdr_rough_reflections_count_null_events_in_the_linear_mean() {
    // Preserve the previously failing seed rather than picking a brighter one:
    // its first three attempts are all null, and later attempts receive HDR.
    assert_eq!(progressive_hdr_expected(3, 82), [0, 0, 0, 255]);
    assert_ne!(progressive_hdr_expected(7, 82), [0, 0, 0, 255]);
    for seed in [0, 82, u32::MAX] {
        for samples in [1, 3, 7] {
            let result = progressive_hdr_png(samples, seed, 37, Some(input()));
            assert_eq!(centre(&result.png), progressive_hdr_expected(samples, seed), "seed={seed}, samples={samples}");
            let image = ::image::load_from_memory(&result.png).unwrap().to_rgba8();
            assert_eq!(image.get_pixel(0, 0).0, [13, 27, 41, 255], "HDR must not replace the display background");
            let unlit = progressive_hdr_png(samples, seed, 37, None);
            assert_eq!(centre(&unlit.png), [0, 0, 0, 255], "no direct lights or environment means zero radiance");
        }
    }
}

#[test]
fn environment_decode_and_secondary_visibility_share_cancellation_and_work_budget() {
    for during_visibility in [false, true] {
        let request = request();
        let options = request.raytrace.clone().unwrap();
        let cancel = CancelToken::default();
        let mut task = ImageTask::new_with_environment(scene(), request, cancel.clone(), options, Some(input())).unwrap();
        if during_visibility {
            for _ in 0..100_000 {
                assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
                if task.pending.as_ref().is_some_and(|p| p.environment_cursor.is_some()) {
                    break;
                }
            }
            assert!(task.pending.as_ref().is_some_and(|p| p.environment_cursor.is_some()));
        }
        // Cancellation must win before any image publication, including decode preparation.
        cancel.cancel();
        assert_eq!(task.step(1).err().unwrap().code, ErrorCode::Cancelled);
    }
    let request = request();
    let options = request.raytrace.clone().unwrap();
    let mut task = ImageTask::new_with_environment(scene(), request, CancelToken::default(), options, Some(input())).unwrap();
    task.work = WorkBudget::new(1).unwrap();
    assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
    assert_eq!(task.step(1).err().unwrap().code, ErrorCode::LimitExceeded);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_host_captured_source_completes_and_releases_work_lease() {
    let host = job::CpuHost::for_platform().unwrap();
    assert!(host.descriptor_for(job::Integrator::Raytrace).unwrap().profiles[0].features.contains(&crate::RenderFeature::Hdri));
    assert!(!host.descriptor_for(job::Integrator::Raster).unwrap().profiles[0].features.contains(&crate::RenderFeature::Hdri));
    let handle = host.submit_with_environment(job::JobId { session: 1693, serial: 1, generation: 1 }, 1, scene(), request(), Some(input())).unwrap();
    assert_eq!(handle.wait_timeout(std::time::Duration::from_secs(60)).unwrap().progress.phase, Phase::Completed);
    let frame = handle.frame().unwrap().unwrap();
    assert!(host.admitted_bytes() >= frame.png().len() as u64 + 1024);
    drop(host);
    drop(frame);
    drop(handle);
}

#[test]
fn environment_visibility_respects_four_work_units_per_quantum_and_cancel() {
    let request = request();
    let options = request.raytrace.clone().unwrap();
    let cancel = CancelToken::default();
    let mut task = ImageTask::new_with_environment(scene(), request, cancel.clone(), options, Some(input())).unwrap();
    for _ in 0..100_000 {
        assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
        if task.pending.as_ref().is_some_and(|p| p.environment_cursor.is_some()) {
            break;
        }
    }
    assert!(task.pending.as_ref().is_some_and(|p| p.environment_cursor.is_some()));
    for _ in 0..32 {
        let before = task.work.used();
        assert!(matches!(task.step(4).unwrap(), TaskPoll::Pending(_)));
        assert!(task.work.used() - before <= 4, "visibility continuation double-charged shading and traversal");
    }
    cancel.cancel();
    assert_eq!(task.step(4).err().unwrap().code, ErrorCode::Cancelled);
}

#[test]
fn all_pixel_traversal_to_shading_transitions_debit_at_most_one_unit() {
    for (directional, shadows) in [(false, true), (true, true), (true, false)] {
        let mut request = request();
        let mut options = request.raytrace.clone().unwrap();
        options.shadows = shadows;
        if directional {
            options.lights = Some(vec![job::DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 1.0 }]);
        }
        request.raytrace = Some(options.clone());
        let cancel = CancelToken::default();
        let mut task = ImageTask::new_with_environment(scene(), request, cancel.clone(), options, Some(input())).unwrap();
        while task.bvh.is_none() {
            assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
        }
        let mut primary_transitions = 0;
        let mut directional_transitions = 0;
        for _ in 0..200_000 {
            if task.pixel == 81 {
                break;
            }
            let primary = task.pending.as_ref().is_some_and(|p| p.hit.is_none());
            let light_before = task.pending.as_ref().map(|p| p.light);
            let before = task.work.used();
            task.pixel_one().unwrap();
            assert!(task.work.used() - before <= 1, "traversal completion must yield before shading/sample debit");
            if primary && task.pending.as_ref().is_some_and(|p| p.hit.is_some()) {
                primary_transitions += 1;
            }
            if light_before == Some(0) && task.pending.as_ref().is_some_and(|p| p.light == 1) {
                directional_transitions += 1;
            }
        }
        assert_eq!(task.pixel, 81);
        assert!(primary_transitions > 0);
        if directional && shadows {
            assert!(directional_transitions > 0);
        }
        cancel.cancel();
        assert_eq!(task.step(4).err().unwrap().code, ErrorCode::Cancelled);
    }
}

#[test]
fn explicit_cpu_profiles_admit_large_images_without_allocating_a_film() {
    let empty = ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY };
    let mut request = request();
    request.width = 3840;
    request.height = 2160;
    let mut options = request.raytrace.clone().unwrap();
    options.progressive = Some(job::CpuProgressiveOptions { samples_per_pixel: 128, seed: 9 });
    request.raytrace = Some(options.clone());
    assert!(admit(&empty, &request, &options).is_err());
    if usize::BITS < 64 || cfg!(target_arch = "wasm32") {
        return;
    }
    options.budget = job::CpuBudget::Studio;
    request.raytrace = Some(options.clone());
    let studio = admit(&empty, &request, &options).unwrap();
    assert!(studio.estimated_total_bytes > 384 * 1024 * 1024);
    assert!(studio.estimated_total_bytes < options.budget.limits().max_job_bytes);
    request.width = 7680;
    request.height = 4320;
    assert!(admit(&empty, &request, &options).is_err());
    options.budget = job::CpuBudget::Workstation;
    request.raytrace = Some(options.clone());
    let workstation = admit(&empty, &request, &options).unwrap();
    assert!(workstation.estimated_total_bytes > studio.estimated_total_bytes);
    assert!(workstation.estimated_total_bytes < options.budget.limits().max_job_bytes);
    for budget in [job::CpuBudget::Studio, job::CpuBudget::Workstation] {
        let count = budget.limits().max_triangles as usize;
        let bytes = BvhBuilder::required_bytes(count, count, limits(budget)).unwrap();
        assert!(bytes < budget.limits().max_job_bytes);
    }
    assert!(BvhBuilder::required_bytes(2_000_000, 2_000_000, limits(job::CpuBudget::Compact)).is_err());
}
