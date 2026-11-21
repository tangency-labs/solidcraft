//! Runtime-option and admission regressions; no render timing assumptions.

use super::*;
use crate::ProductScene;
use solidcraft_geom::Aabb3;

fn request(integrator: Integrator) -> ImageRequest {
    ImageRequest {
        studio_weights: None,
        environment: None,
        textures: None,
        integrator,
        raytrace: None,
        camera: Default::default(),
        width: 16,
        height: 16,
        background: [0; 3],
        transparent_background: false,
    }
}

#[test]
fn options_reject_retained_capacity_even_when_no_lights_are_active() {
    let options = RaytraceOptions { lights: Some(Vec::with_capacity(64)), ..Default::default() };
    assert_eq!(options.validate().unwrap_err().code, ErrorCode::InvalidRequest);
    let serialized = serde_json::to_value(&options).unwrap();
    let decoded: RaytraceOptions = serde_json::from_value(serialized).unwrap();
    assert!(decoded.validate().is_ok(), "wire input retains no oversized source allocation");
}

#[cfg(feature = "raytrace-cpu")]
#[test]
fn raytrace_admission_validates_retained_options_before_cloning() {
    let scene = ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY };
    let mut request = request(Integrator::Raytrace);
    request.raytrace = Some(RaytraceOptions { lights: Some(Vec::with_capacity(64)), ..Default::default() });
    let original = request.raytrace.as_ref().unwrap();
    assert_eq!(original.validate().unwrap_err().code, ErrorCode::InvalidRequest);
    assert!(original.clone().validate().is_ok(), "cloning an empty Vec discards its retained capacity");
    let error = admit(&scene, &request).err().unwrap();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(request.raytrace.as_ref().unwrap().lights.as_ref().unwrap().capacity(), 64);
}

#[test]
fn options_preserve_the_eight_light_boundary_and_refuse_the_ninth() {
    let light = serde_json::json!({"direction": [0.0, 0.0, -1.0], "color": [1.0, 1.0, 1.0], "intensity": 1.0});
    let accepted: RaytraceOptions = serde_json::from_value(serde_json::json!({"lights": vec![light.clone(); 8]})).unwrap();
    assert!(accepted.validate().is_ok());
    assert!(serde_json::from_value::<RaytraceOptions>(serde_json::json!({"lights": vec![light; 9]})).is_err());
}

#[test]
fn exposure_refuses_nonfinite_and_out_of_range_values() {
    for exposure_ev in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -16.01, 16.01] {
        let options = RaytraceOptions { exposure_ev, ..Default::default() };
        assert_eq!(options.validate().unwrap_err().code, ErrorCode::InvalidRequest);
    }
    for exposure_ev in [-16.0, 16.0] {
        assert!(RaytraceOptions { exposure_ev, ..Default::default() }.validate().is_ok());
    }
}

#[test]
fn rejected_raytrace_submission_leaves_existing_raster_reservation_intact() {
    let host = CpuHost::cooperative();
    let scene = || ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY };
    let raster = host.submit(JobId { session: 1, serial: 1, generation: 1 }, 1, scene(), request(Integrator::Raster)).unwrap();
    let before = host.admitted_bytes();
    assert!(before > 0);
    let error = host.submit(JobId { session: 1, serial: 2, generation: 2 }, 1, scene(), request(Integrator::Raytrace)).err().unwrap();
    assert_eq!(error.code, ErrorCode::HostUnavailable);
    assert_eq!(host.admitted_bytes(), before);
    assert_eq!(raster.status().unwrap().progress.phase, Phase::Queued);
    assert!(raster.frame().unwrap().is_none());
    assert_eq!(raster.cancel().unwrap().progress.phase, Phase::Cancelled);
    // Pump only to discard cancelled queued input; it must not produce a frame.
    assert!(!host.pump(1).unwrap());
    assert!(raster.work_released());
    assert_eq!(host.admitted_bytes(), ImageRequest::fixed_cpu_metadata_bytes(), "the live handle retains only its exact fixed request header");
    assert!(raster.frame().unwrap().is_none());
    drop(raster);
    assert_eq!(host.admitted_bytes(), 0, "the last handle releases that header without any frame");
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn native_integrators_share_one_host_and_retained_frame_budget() {
    for frames_first in [false, true] {
        let mut host = CpuHost::for_platform().unwrap();
        let scene = || ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY };
        let raster_id = JobId { session: 10, serial: 1, generation: 1 };
        let raytrace_id = JobId { session: 20, serial: 1, generation: 1 };
        let raster = host.submit(raster_id, 100, scene(), request(Integrator::Raster)).unwrap();
        let raytrace = host.submit(raytrace_id, 200, scene(), request(Integrator::Raytrace)).unwrap();
        // Deadlock watchdog only; there is no rendering-speed assertion.
        for job in [&raster, &raytrace] {
            assert_eq!(job.wait_timeout(std::time::Duration::from_secs(60)).unwrap().progress.phase, Phase::Completed);
        }
        host.join_worker_for_test().unwrap();
        let raster_frame = raster.frame().unwrap().unwrap();
        let raytrace_frame = raytrace.frame().unwrap().unwrap();
        assert_eq!(raster_frame.info.job, raster_id);
        assert_eq!(raytrace_frame.info.job, raytrace_id);
        assert_eq!(raster_frame.info.scene_generation, 100);
        assert_eq!(raytrace_frame.info.scene_generation, 200);
        assert_eq!(raster_frame.info.executor.as_str(), "cpu-product");
        assert_eq!(raytrace_frame.info.executor.as_str(), "cpu-raytrace");
        let fixed = ImageRequest::fixed_cpu_metadata_bytes();
        let both = host.admitted_bytes();
        assert!(both > 2 * fixed);
        if frames_first {
            drop(raster_frame);
            drop(raytrace_frame);
            assert_eq!(host.admitted_bytes(), both, "held Records still own both final images and both fixed request headers");
            drop(raster);
            assert!(host.admitted_bytes() > fixed && host.admitted_bytes() < both - fixed, "the remaining Record keeps its image and exact header");
            drop(raytrace);
        } else {
            drop(raster);
            assert_eq!(host.admitted_bytes(), both - fixed);
            drop(raytrace);
            let frames = both - 2 * fixed;
            assert_eq!(host.admitted_bytes(), frames, "external frames retain image leases, not the dropped Records' request headers");
            drop(raster_frame);
            assert!(host.admitted_bytes() > 0 && host.admitted_bytes() < frames);
            drop(raytrace_frame);
        }
        assert_eq!(host.admitted_bytes(), 0);
    }
}

#[cfg(feature = "raytrace-cpu")]
#[test]
fn cancelled_raytrace_task_never_returns_a_frame() {
    let cancel = CancelToken::default();
    let scene = ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY };
    let mut task = crate::raytrace::image::ImageTask::new(scene, request(Integrator::Raytrace), cancel.clone(), RaytraceOptions::default()).unwrap();
    cancel.cancel();
    assert_eq!(task.step(4).err().unwrap().code, ErrorCode::Cancelled);
}

#[cfg(feature = "raytrace-cpu")]
#[test]
fn completed_raytrace_task_releases_captured_mesh_before_returning_png() {
    let mesh = Arc::new(solidcraft_geom::Mesh::default());
    let captured = Arc::downgrade(&mesh);
    let scene = ProductScene {
        meshes: vec![crate::ProductMesh {
            transmission: None,
            name: "Lease probe".into(),
            mesh,
            color: [0; 3],
            opacity: 1.0,
            surface: None,
            faces: Vec::new(),
        }],
        bounds: Aabb3::EMPTY,
    };
    let mut task =
        crate::raytrace::image::ImageTask::new(scene, request(Integrator::Raytrace), CancelToken::default(), RaytraceOptions::default()).unwrap();
    assert!(captured.upgrade().is_some());
    let mut completed = false;
    for _ in 0..10_000 {
        if let TaskPoll::Complete(png) = task.step(4).unwrap() {
            assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
            completed = true;
            break;
        }
    }
    assert!(completed, "bounded empty-scene fixture must complete");
    assert!(captured.upgrade().is_none(), "the host may shrink the lease as soon as Complete is returned, even while the task remains alive");
    assert_eq!(task.progress().phase, Phase::Completed);
}
#[test]
fn emission_runtime_json_is_bounded_explicit_and_roundtrips() {
    use super::{EmissionSides, RaytraceOptions, SurfaceEmission};
    let emitter = SurfaceEmission { body_index: 0, body: "fixture".into(), color: [1.0, 0.5, 0.0], strength: 2.0, sides: EmissionSides::Front };
    let options = RaytraceOptions { emissions: Some(vec![emitter.clone()]), ..Default::default() };
    let encoded = serde_json::to_value(&options).unwrap();
    assert_eq!(serde_json::from_value::<RaytraceOptions>(encoded).unwrap(), options);
    assert!(serde_json::from_str::<RaytraceOptions>("{\"emissions\":null}").is_err());
    assert!(serde_json::from_value::<RaytraceOptions>(serde_json::json!({"emissions": vec![emitter; 33]})).is_err());
    assert!(
        serde_json::from_value::<RaytraceOptions>(
            serde_json::json!({"emissions":[{"body_index":0,"body":"x".repeat(513),"color":[1,1,1],"strength":1}]})
        )
        .is_err()
    );
    // Any body name a rename takes (128 characters, up to 512 bytes) is admitted.
    let long: RaytraceOptions =
        serde_json::from_value(serde_json::json!({"emissions":[{"body_index":0,"body":"中".repeat(128),"color":[1,1,1],"strength":1}]})).unwrap();
    assert!(long.emissions.as_ref().is_some_and(|e| e.iter().all(|emitter| emitter.validate().is_ok())), "{long:?}");
    let reflections =
        ReflectionOptions { bodies: vec![ReflectionBody { body: "中".repeat(128), mode: ReflectionMode::Mirror }], samples: 1, max_bounces: 1 };
    assert!(reflections.validate().is_ok());
    assert_eq!(serde_json::from_str::<RaytraceOptions>("{}").unwrap().emissions, None);
    assert_eq!(serde_json::from_str::<RaytraceOptions>("{\"emissions\":[]}").unwrap().emissions, Some(Vec::new()));
}

#[test]
fn opus_quality_limits_are_explicit_finite_and_keep_transport_defaults() {
    assert_eq!(JobOptions::default().samples, 64);
    assert_eq!(JobOptions::default().max_depth, 8);
    let options = JobOptions {
        samples: MAX_SAMPLES_PER_PIXEL,
        max_depth: u32::from(MAX_PATH_DEPTH),
        caustics: Some(CausticOptions { photons: MAX_CAUSTIC_PHOTONS, max_depth: MAX_PATH_DEPTH, ..Default::default() }),
        ..Default::default()
    };
    options.validate().unwrap();
    assert!(JobOptions { samples: MAX_SAMPLES_PER_PIXEL + 1, ..options }.validate().is_err());
    assert!(JobOptions { max_depth: u32::from(MAX_PATH_DEPTH) + 1, ..options }.validate().is_err());
    assert!(CpuPathOptions { max_depth: MAX_PATH_DEPTH }.validate().is_ok());
    assert!(CpuPathOptions { max_depth: MAX_PATH_DEPTH + 1 }.validate().is_err());
    assert!(
        CpuBudget::Workstation.validate_image(3840, 2160, Some(CpuProgressiveOptions { samples_per_pixel: MAX_SAMPLES_PER_PIXEL, seed: 81 })).is_ok()
    );
    assert!(CpuBudget::Studio.validate_samples(MAX_SAMPLES_PER_PIXEL).is_err());
}

#[test]
fn quality_controls_preserve_authored_values_and_validate_finite_envelopes() {
    let cpu: RaytraceOptions = serde_json::from_value(serde_json::json!({
        "threads":4,"supersampling":2,"denoise":true,"denoise_diagnostics":true,
        "denoise_settings":{"passes":2,"strength":0.5,"firefly_suppression":false,"optical_filter":false}
    }))
    .unwrap();
    assert_eq!(cpu.threads, 4);
    assert_eq!(cpu.supersampling, 2);
    assert!(cpu.denoise_diagnostics);
    assert!(!cpu.denoise_settings.optical_filter);
    cpu.validate().unwrap();
    let default_cpu: RaytraceOptions = serde_json::from_value(serde_json::json!({"denoise":true})).unwrap();
    let default_gpu: JobOptions = serde_json::from_value(serde_json::json!({"denoise":true})).unwrap();
    assert!(default_cpu.denoise_settings.optical_filter && default_gpu.denoise_settings.optical_filter);
    assert!(serde_json::to_value(&default_cpu).unwrap().get("denoise_settings").is_none());
    assert!(serde_json::to_value(default_gpu).unwrap().get("denoise_settings").is_none());
    for invalid in [serde_json::json!(null), serde_json::json!("false"), serde_json::json!(0)] {
        let wire = serde_json::json!({"denoise_settings":{"optical_filter":invalid}});
        assert!(serde_json::from_value::<RaytraceOptions>(wire.clone()).is_err());
        assert!(serde_json::from_value::<JobOptions>(wire).is_err());
    }
    assert_eq!(serde_json::from_value::<RaytraceOptions>(serde_json::to_value(&cpu).unwrap()).unwrap(), cpu);
    for threads in [0, 1, MAX_CPU_THREADS] {
        assert!(RaytraceOptions { threads, ..Default::default() }.validate().is_ok());
    }
    assert!(RaytraceOptions { threads: MAX_CPU_THREADS + 1, ..Default::default() }.validate().is_err());
    for supersampling in [0, 3, u32::MAX] {
        assert!(RaytraceOptions { supersampling, ..Default::default() }.validate().is_err());
        assert!(JobOptions { supersampling, ..Default::default() }.validate().is_err());
    }
    for settings in [
        DenoiseSettings { passes: 0, ..Default::default() },
        DenoiseSettings { passes: 4, ..Default::default() },
        DenoiseSettings { strength: f64::NAN, ..Default::default() },
        DenoiseSettings { strength: -0.01, ..Default::default() },
        DenoiseSettings { strength: 1.01, ..Default::default() },
    ] {
        assert!(settings.validate().is_err());
    }
    for options in [
        JobOptions { supersampling: 2, ..Default::default() },
        JobOptions { denoise_diagnostics: true, ..Default::default() },
        JobOptions { denoise_settings: DenoiseSettings { strength: 0.5, ..Default::default() }, ..Default::default() },
        JobOptions { denoise_settings: DenoiseSettings { optical_filter: false, ..Default::default() }, ..Default::default() },
    ] {
        assert!(!options.is_structural_default());
        let wire = serde_json::to_value(options).unwrap();
        assert_eq!(serde_json::from_value::<JobOptions>(wire.clone()).unwrap(), options);
        if !options.denoise_settings.optical_filter {
            assert_eq!(wire["denoise_settings"]["optical_filter"], false);
        }
        assert_eq!(options.retained_bytes().unwrap(), std::mem::size_of::<JobOptions>() as u64);
    }
}

#[test]
fn denoiser_serde_keeps_legacy_defaults_and_explicit_method_quality_intent() {
    let defaults = DenoiseSettings::default();
    assert_eq!(defaults.method, DenoiseMethod::Spatial);
    assert_eq!(defaults.oidn_quality, OidnQuality::High);
    assert_eq!((defaults.passes, defaults.strength, defaults.firefly_suppression, defaults.optical_filter), (3, 1.0, false, true));
    for old in [serde_json::json!({}), serde_json::json!({"passes":3,"strength":1.0,"firefly_suppression":false,"optical_filter":true})] {
        assert_eq!(serde_json::from_value::<DenoiseSettings>(old.clone()).unwrap(), defaults);
        let wire = serde_json::json!({"denoise":true,"denoise_settings":old});
        assert_eq!(serde_json::from_value::<RaytraceOptions>(wire.clone()).unwrap().denoise_settings, defaults);
        assert_eq!(serde_json::from_value::<JobOptions>(wire).unwrap().denoise_settings, defaults);
    }
    let legacy: DenoiseSettings = serde_json::from_value(serde_json::json!({
        "passes":2,"strength":0.25,"firefly_suppression":true,"optical_filter":false
    }))
    .unwrap();
    assert_eq!(legacy, DenoiseSettings { passes: 2, strength: 0.25, firefly_suppression: true, optical_filter: false, ..defaults });
    assert_eq!(
        serde_json::to_value(legacy).unwrap(),
        serde_json::json!({"passes":2,"strength":0.25,"firefly_suppression":true,"optical_filter":false}),
        "legacy Spatial replies do not gain default method or unused quality fields",
    );
    let method_only: DenoiseSettings = serde_json::from_value(serde_json::json!({"method":"oidn"})).unwrap();
    assert_eq!(method_only, DenoiseSettings { method: DenoiseMethod::Oidn, ..defaults });
    let quality_only: DenoiseSettings = serde_json::from_value(serde_json::json!({"oidn_quality":"fast"})).unwrap();
    assert_eq!(quality_only, DenoiseSettings { oidn_quality: OidnQuality::Fast, ..defaults });

    for (method, method_name) in [(DenoiseMethod::Spatial, "spatial"), (DenoiseMethod::Oidn, "oidn")] {
        for (quality, quality_name) in [(OidnQuality::Fast, "fast"), (OidnQuality::Balanced, "balanced"), (OidnQuality::High, "high")] {
            let settings = DenoiseSettings { method, oidn_quality: quality, ..legacy };
            settings.validate().unwrap();
            let wire = serde_json::to_value(settings).unwrap();
            assert_eq!(wire.get("method"), (method != DenoiseMethod::Spatial).then(|| serde_json::json!(method_name)).as_ref());
            assert_eq!(wire.get("oidn_quality"), (quality != OidnQuality::High).then(|| serde_json::json!(quality_name)).as_ref());
            assert_eq!(serde_json::from_value::<DenoiseSettings>(wire).unwrap(), settings);
            // Disabled filtering still retains a future method/quality choice, even without OIDN.
            for denoise in [false, true] {
                let cpu = RaytraceOptions { denoise, denoise_settings: settings, ..Default::default() };
                let gpu = JobOptions { denoise, denoise_settings: settings, ..Default::default() };
                cpu.validate().unwrap();
                gpu.validate().unwrap();
                assert_eq!(serde_json::from_value::<RaytraceOptions>(serde_json::to_value(&cpu).unwrap()).unwrap(), cpu);
                assert_eq!(serde_json::from_value::<JobOptions>(serde_json::to_value(gpu).unwrap()).unwrap(), gpu);
            }
        }
    }
}

#[test]
fn denoiser_serde_rejects_unknown_variants_types_and_fields_without_fallback() {
    for (field, invalid) in [
        ("method", serde_json::json!("unknown")),
        ("method", serde_json::json!("Oidn")),
        ("method", serde_json::json!(null)),
        ("method", serde_json::json!(false)),
        ("method", serde_json::json!(0)),
        ("oidn_quality", serde_json::json!("unknown")),
        ("oidn_quality", serde_json::json!("High")),
        ("oidn_quality", serde_json::json!(null)),
        ("oidn_quality", serde_json::json!(false)),
        ("oidn_quality", serde_json::json!(0)),
        ("unknown", serde_json::json!(true)),
    ] {
        let mut settings = serde_json::json!({});
        settings[field] = invalid;
        assert!(serde_json::from_value::<DenoiseSettings>(settings.clone()).is_err(), "{settings}");
        let wire = serde_json::json!({"denoise_settings":settings});
        assert!(serde_json::from_value::<RaytraceOptions>(wire.clone()).is_err(), "{wire}");
        assert!(serde_json::from_value::<JobOptions>(wire.clone()).is_err(), "{wire}");
    }
}

#[test]
fn oidn_admission_bounds_and_supersampling_bytes_are_checked_without_allocations() {
    for (width, height) in [(0, 1), (1, 0), (MAX_TRACE_DIMENSION + 1, 1), (MAX_TRACE_DIMENSION, MAX_TRACE_DIMENSION), (u32::MAX, u32::MAX)] {
        assert!(denoise_oidn::required_bytes(width, height).is_err(), "{width}x{height}");
    }
    let native = denoise_oidn::required_bytes(8, 4).unwrap();
    let (width, height) = supersampled_dimensions(8, 4, 2).unwrap();
    let doubled = denoise_oidn::required_bytes(width, height).unwrap();
    assert!(native > 8 * 4 * 24, "native workspace is charged in addition to RGB planes");
    assert_eq!(doubled - native, (16 * 8 - 8 * 4) * 24, "X2 charges four trace pixels per output pixel");
    assert!(denoise_oidn::required_bytes(MAX_TRACE_DIMENSION, MAX_TRACE_DIMENSION / 2).is_ok());
    assert_eq!(denoise_oidn::available(), cfg!(all(feature = "denoise-oidn", not(target_arch = "wasm32"))));
    assert_eq!(
        denoise_oidn::validate_request(1, 1, true, false).unwrap_err().code,
        if denoise_oidn::available() { ErrorCode::InvalidRequest } else { ErrorCode::HostUnavailable },
    );
}

fn not_native(failure: denoise_oidn::OidnFailure) -> JobError {
    let mut native_failed = true;
    let error = failure.record(&mut native_failed);
    assert!(!native_failed, "preflight, bad source data or cancellation cannot authorize raw recovery");
    error
}

#[cfg(not(all(feature = "denoise-oidn", not(target_arch = "wasm32"))))]
#[test]
fn unavailable_oidn_refuses_execution_without_rewriting_parseable_intent() {
    let settings: DenoiseSettings = serde_json::from_value(serde_json::json!({"method":"oidn","oidn_quality":"balanced"})).unwrap();
    settings.validate().unwrap();
    assert_eq!(denoise_oidn::validate_request(1, 1, false, false).unwrap_err().code, ErrorCode::HostUnavailable);
    let error = denoise_oidn::OidnTask::new(1, 1, vec![[0.5, 0.25, 0.1, 1.0]], settings, CancelToken::default()).err().map(not_native).unwrap();
    assert_eq!(error.code, ErrorCode::HostUnavailable);
    assert_eq!(settings.method, DenoiseMethod::Oidn, "unavailable OIDN never silently runs Spatial");
    assert_eq!(settings.oidn_quality, OidnQuality::Balanced);
}

#[cfg(all(feature = "denoise-oidn", not(target_arch = "wasm32")))]
#[test]
fn oidn_cancelled_poll_keeps_active_worker_pending_until_retirement() {
    let (release, wait) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        wait.recv().unwrap();
        Ok(vec![[1.0, 2.0, 3.0, 1.0]])
    });
    let cancel = CancelToken::default();
    let mut task = denoise_oidn::OidnTask::from_worker_for_test(worker, cancel.clone());
    cancel.cancel();
    let first = task.poll();
    // Release before asserting so the old early-error implementation cannot leave
    // the deliberately blocked worker joined by Drop during assertion unwinding.
    release.send(()).unwrap();
    assert!(matches!(first, Ok(None)), "cancellation cannot return an error while native work is active: {first:?}");
    assert_eq!(not_native(task.wait().unwrap_err()).code, ErrorCode::Cancelled);
}

#[cfg(all(feature = "denoise-oidn", not(target_arch = "wasm32")))]
#[test]
fn oidn_adapter_filters_noisy_rgb_and_keeps_internal_coverage_bits_without_mutating_source() {
    // Partial/zero coverage is possible internally even with opaque final output.
    // This tests buffer ownership/coverage only, not transparent-export image quality.
    let source: Vec<[f32; 4]> = (0..32 * 32)
        .map(|index| {
            let x = index % 32;
            let y = index / 32;
            let alpha = match x {
                0 => 0.0,
                1 => 0.25,
                2 => 0.5,
                3 => 0.75,
                _ => 1.0,
            };
            let checker = if (x + y) % 2 == 0 { 0.1 } else { 1.5 };
            [alpha * checker, alpha * (0.2 + checker * 0.3), alpha * (0.4 + checker * 0.2), alpha]
        })
        .collect();
    let before: Vec<_> = source.iter().map(|pixel| pixel.map(f32::to_bits)).collect();
    let mut task = denoise_oidn::OidnTask::new(
        32,
        32,
        source.clone(),
        DenoiseSettings { method: DenoiseMethod::Oidn, ..Default::default() },
        CancelToken::default(),
    )
    .unwrap();
    let filtered = task.wait().unwrap();
    assert_eq!(filtered.len(), 32 * 32);
    assert!(filtered.iter().flatten().all(|value| value.is_finite() && *value >= 0.0));
    assert!(filtered.iter().zip(&source).all(|(filtered, original)| filtered[3].to_bits() == original[3].to_bits()));
    assert!(filtered.iter().filter(|pixel| pixel[3] == 0.0).all(|pixel| pixel[..3] == [0.0; 3]));
    assert!(
        filtered.iter().zip(&source).any(|(filtered, original)| filtered[..3] != original[..3]),
        "the native filter must run, not return raw RGB"
    );
    assert_eq!(source.iter().map(|pixel| pixel.map(f32::to_bits)).collect::<Vec<_>>(), before, "the caller's raw image is unchanged");
    assert!(task.poll().is_err(), "a completed task must not return a second output");
}

#[cfg(all(feature = "denoise-oidn", not(target_arch = "wasm32")))]
#[test]
fn oidn_adapter_rejects_invalid_radiance_and_coverage_without_output() {
    for pixel in [
        [f32::NAN, 0.0, 0.0, 1.0],
        [0.0, f32::INFINITY, 0.0, 1.0],
        [-0.1, 0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0, -0.1],
        [0.0, 0.0, 0.0, 1.1],
        [0.0, 0.0, 0.0, f32::NAN],
        [0.1, 0.0, 0.0, 0.0],
    ] {
        let mut task = denoise_oidn::OidnTask::new(
            1,
            1,
            vec![pixel],
            DenoiseSettings { method: DenoiseMethod::Oidn, ..Default::default() },
            CancelToken::default(),
        )
        .unwrap();
        let error = not_native(task.wait().unwrap_err());
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{pixel:?}: {error}");
        assert!(error.message.contains("invalid radiance or coverage"), "{error}");
        assert!(task.poll().is_err(), "invalid input cannot later produce output");
    }
}

#[cfg(all(feature = "denoise-oidn", not(target_arch = "wasm32")))]
#[test]
fn oidn_adapter_refuses_bad_shape_and_already_cancelled_work_before_starting() {
    let settings = DenoiseSettings { method: DenoiseMethod::Oidn, ..Default::default() };
    let error = denoise_oidn::OidnTask::new(3, 1, vec![[0.0; 4]; 2], settings, CancelToken::default()).err().map(not_native).unwrap();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(error.message.contains("trace grid"));
    let cancel = CancelToken::default();
    cancel.cancel();
    let error = denoise_oidn::OidnTask::new(1, 1, vec![[0.5, 0.25, 0.1, 1.0]], settings, cancel).err().map(not_native).unwrap();
    assert_eq!(error.code, ErrorCode::Cancelled);
}

#[test]
fn cooperative_cpu_oidn_refuses_before_any_work_reservation() {
    let host = CpuHost::cooperative();
    let scene = || ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY };
    let mut image = request(Integrator::Raytrace);
    image.raytrace = Some(RaytraceOptions {
        denoise: true,
        denoise_settings: DenoiseSettings { method: DenoiseMethod::Oidn, ..Default::default() },
        ..Default::default()
    });
    let error = host.submit(JobId { session: 16902, serial: 1, generation: 1 }, 1, scene(), image).err().unwrap();
    assert_eq!(error.code, ErrorCode::HostUnavailable);
    assert!(error.message.contains("background image host"), "{error}");
    assert_eq!(host.admitted_bytes(), 0);
    assert!(!host.pump(1).unwrap());
}

#[test]
fn output_and_internal_sampling_bounds_are_checked_without_allocating_images() {
    assert_eq!(supersampled_dimensions(7680, 4320, 2).unwrap(), (15360, 8640));
    assert_eq!(supersampled_dimensions(8192, 4096, 2).unwrap(), (16384, 8192));
    assert!(supersampled_dimensions(8193, 1, 2).is_err());
    assert!(supersampled_dimensions(8192, 4097, 2).is_err());
    assert!(supersampled_dimensions(u32::MAX, u32::MAX, 2).is_err());
    assert!(linear_exr::encoded_limit(7680, 4320).is_ok());
    assert!(linear_exr::encoded_limit(8192, 4096).is_ok());
    assert!(linear_exr::encoded_limit(8192, 4097).is_err());
    assert!(validate_cpu_denoise_image(15360, 8640).is_ok());
    assert!(validate_cpu_denoise_image(16384, 8192).is_ok());
    assert!(validate_cpu_denoise_image(16384, 8193).is_err());
    assert!(CpuBudget::Workstation.validate_image(15360, 8640, Some(CpuProgressiveOptions { samples_per_pixel: 128, seed: 0 })).is_ok());
    assert!(CpuBudget::Compact.validate_samples(128).is_err(), "Compact keeps its explicit resource envelope");
    assert!(CpuBudget::Studio.validate_samples(128).is_ok());
    assert!(CpuBudget::Workstation.validate_image(16384, 1, None).is_ok(), "scale1 native backend dimensions remain compatible");
}

#[test]
fn diagnostic_masks_keep_exact_categories_and_reports_use_readable_keys() {
    use DenoiseGuideClass::*;
    assert_eq!(resolve_denoise_class([Surface; 4]), Surface);
    assert_eq!(resolve_denoise_class([Optical; 4]), Optical);
    assert_eq!(resolve_denoise_class([Surface, Optical]), Unavailable);
    assert_eq!(resolve_denoise_class([Surface, Optical, Background, Emitter]), Background);
    let mut report = DenoiseDiagnostics::new(2, 2);
    for class in [Surface, Optical, PartialCoverage, Background] {
        report.record(class);
    }
    let json = serde_json::to_value(report).unwrap();
    assert_eq!(json["counts"]["partial_coverage"], 1);
    assert_eq!(json["counts"]["surface"], 1);
    assert_eq!(report.counts.iter().sum::<u64>(), 4);
    for class in DenoiseGuideClass::ALL {
        assert_eq!(DenoiseGuideClass::from_code(class.code()), Some(*class));
        assert_eq!(class.color()[3], 255);
    }
}
