use super::*;
use solidcraft_render::job::adaptive::AdaptiveSampling;

#[test]
fn independent_pilots_never_enter_the_final_master_and_admission_counts_auxiliary_storage() {
    let Some(executor) = executor() else { return };
    let fixed = JobOptions { samples: 16, seed: 91, max_depth: 4, linear_exr: true, ..Default::default() };
    let adaptive = JobOptions { adaptive: Some(AdaptiveSampling { pilot_samples: 16, min_samples: 16, ..Default::default() }), ..fixed };
    let source = sphere(white());
    let req = request(16);
    let old = admission(&source, &req, fixed).unwrap();
    let new = admission(&source, &req, adaptive).unwrap();
    assert_eq!(new.estimated_total_bytes - old.estimated_total_bytes, 16 * 16 * 48);
    let old_filtered = admission(&source, &req, JobOptions { denoise: true, ..fixed }).unwrap();
    let new_filtered = admission(&source, &req, JobOptions { denoise: true, ..adaptive }).unwrap();
    assert_eq!(
        new_filtered.pixel_scratch_bytes - old_filtered.pixel_scratch_bytes,
        16 * 16 * 64,
        "the normalized denoiser input has its own prepaid float4 plane"
    );
    // The sphere's geometry preparation can dominate both peak estimates, hiding
    // a larger pixel stage beneath that already prepaid peak. With no geometry,
    // pixel storage determines the peak and must include the normalized plane too.
    let empty = ProductScene { meshes: Vec::new(), bounds: solidcraft_geom::Aabb3::EMPTY };
    let old_pixel_peak = admission(&empty, &req, JobOptions { denoise: true, ..fixed }).unwrap();
    let new_pixel_peak = admission(&empty, &req, JobOptions { denoise: true, ..adaptive }).unwrap();
    assert_eq!(new_pixel_peak.estimated_total_bytes - old_pixel_peak.estimated_total_bytes, 16 * 16 * 64);
    let (a, exr_a) = complete_linear_fixture(executor.context.clone(), fixed);
    let (b, exr_b) = complete_linear_fixture(executor.context.clone(), adaptive);
    assert_eq!(a, b, "fixed production budget must retain the old production sample stream");
    assert_eq!(exr_a, exr_b, "discard pilots rather than mixing an adaptively selected prefix into the master");
}

#[test]
fn zero_variance_pixels_skip_path_work_and_report_real_counts_without_hiding_capped_pixels() {
    let Some(executor) = executor() else { return };
    let options = JobOptions {
        samples: 64,
        adaptive: Some(AdaptiveSampling { pilot_samples: 16, min_samples: 16, ..Default::default() }),
        denoise: true,
        linear_exr: true,
        ..Default::default()
    };
    let source = ProductScene { meshes: vec![], bounds: Aabb3::EMPTY };
    let mut task = Task::new(executor.context.clone(), source, request(8), options, CancelToken::default()).unwrap();
    loop {
        if matches!(task.step(4096).unwrap(), TaskPoll::Complete(_)) {
            break;
        }
    }
    let report = task.sampling_report().unwrap();
    assert_eq!(report.pilot_samples_per_pixel, 16);
    assert_eq!((report.minimum_production_samples, report.maximum_production_samples), (16, 16));
    assert_eq!(report.production_samples, 8 * 8 * 16);
    assert_eq!(report.pixels_at_cap, 0);
    assert_eq!(report.primary_path_evaluations, 8 * 8 * 32);
    assert!(task.take_linear_exr().is_some());
    assert_eq!(task.read_linear().unwrap(), vec![[0.0; 4]; 64]);
    let mut mixed = Task::new(executor.context.clone(), sphere(white()), request(16), options, CancelToken::default()).unwrap();
    loop {
        if matches!(mixed.step(4096).unwrap(), TaskPoll::Complete(_)) {
            break;
        }
    }
    let budget = mixed.sampling_report().unwrap();
    eprintln!("mixed scene allocation: {budget:?}");
    assert_eq!((budget.minimum_production_samples, budget.maximum_production_samples), (16, 64));
    assert!(budget.pixels_at_cap > 0 && budget.pixels_at_cap < 256);
    assert!(budget.production_samples < 256 * 64);
    let cancel = CancelToken::default();
    let mut cancelled = Task::new(executor.context, sphere(white()), request(8), options, cancel.clone()).unwrap();
    cancelled.dispatch().unwrap();
    cancel.cancel();
    assert!(matches!(cancelled.step(1), Err(error) if error.code == ErrorCode::Cancelled));
    assert!(cancelled.take_linear_exr().is_none());
}

#[test]
fn transmission_strata_preserve_opaque_stream_and_reduce_absorbing_sheet_error_at_equal_path_budget() {
    let Some(executor) = executor() else { return };
    let ordinary = JobOptions { samples: 16, seed: 1948, max_depth: 4, linear_exr: true, ..Default::default() };
    let (a, _) = complete_linear_fixture(executor.context.clone(), ordinary);
    let (b, _) = complete_linear_fixture(executor.context.clone(), JobOptions { transmission_samples: 4, ..ordinary });
    let a = image::load_from_memory(&a).unwrap().into_rgba8();
    let b = image::load_from_memory(&b).unwrap().into_rgba8();
    assert!(a.as_raw().iter().zip(b.as_raw()).all(|(a, b)| a.abs_diff(*b) <= 1), "strata preserve opaque radiance within accumulation rounding");
    let mut source = glass_slab(true);
    let transmission = solidcraft_render::transmission::Transmission::new(Vec3::new(3.0, 3.0, 3.0), Some(1.0)).unwrap();
    source.meshes[0].transmission = Some(transmission);
    let (_, oracle) = solidcraft_render::transmission::slab(transmission, 1.0, 1.0, 1.0, 1.5).unwrap();
    let mut squared_error = [0.0; 2];
    let mut mean = [0.0; 2];
    for seed in 0..32 {
        for (column, (samples, strata)) in [(64, 1), (16, 4)].into_iter().enumerate() {
            let mut req = request(1);
            req.camera.fov = 0.0001;
            let options = JobOptions { samples, transmission_samples: strata, seed: seed * 7919 + 19, max_depth: 4, ..Default::default() };
            let mut task = Task::new(executor.context.clone(), source.clone(), req, options, CancelToken::default()).unwrap();
            task.environment = 1.0;
            task.environment_bottom_only = true;
            task.studio = 0.0;
            while task.sample < samples {
                task.dispatch().unwrap();
            }
            let actual = f64::from(task.read_linear().unwrap()[0][0]);
            squared_error[column] += (actual - oracle.x).powi(2);
            mean[column] += actual / 32.0;
        }
    }
    eprintln!(
        "absorbing-sheet equal path budget: fixed/stratified MSE {:?}, means {:?}, analytic {}",
        squared_error.map(|v| v / 32.0),
        mean,
        oracle.x
    );
    assert!((mean[0] - oracle.x).abs() < 0.005 && (mean[1] - oracle.x).abs() < 0.005);
    assert!(squared_error[1] < squared_error[0] * 0.6, "the declared pilot must show a quantitative benefit, not just a changed image");
}

/// Adaptive reports must not touch a device already retired by the current
/// renderer health policy, even when the numerical sample budget is unchanged.
#[test]
fn adaptive_report_refuses_retired_device_before_readback() {
    let Some(executor) = executor() else { return };
    let options = JobOptions {
        samples: 16,
        adaptive: Some(solidcraft_render::job::adaptive::AdaptiveSampling {
            pilot_samples: 16,
            min_samples: 16,
            relative_error: 0.1,
            absolute_error: 0.01,
            coverage_error: 0.1,
        }),
        ..Default::default()
    };
    let task = Task::new(executor.context.clone(), sphere(white()), request(1), options, CancelToken::default()).unwrap();
    executor.context.healthy.store(false, Ordering::Release);
    let error = adaptive::report(&task).unwrap_err();
    assert_eq!(error.code, ErrorCode::HostUnavailable);
}

#[test]
fn adaptive_preview_divides_only_unfinished_production_and_rejects_invalid_budgets() {
    let options = JobOptions {
        samples: 64,
        adaptive: Some(AdaptiveSampling { pilot_samples: 16, min_samples: 16, ..Default::default() }),
        ..Default::default()
    };
    for completed in 1..=16 {
        assert_eq!(adaptive::preview_divisor(&options, completed, None).unwrap(), 1.0);
    }
    for count in [16, 32, 64] {
        let budget = (count as f32).to_le_bytes();
        for production in 1..=64 {
            let actual = adaptive::preview_divisor(&options, 16 + production, Some(&budget)).unwrap();
            assert_eq!(actual, if production < count { production as f32 } else { 1.0 });
        }
    }
    for count in [f32::NAN, f32::INFINITY, 0.0, 15.0, 16.5, 65.0] {
        assert!(adaptive::preview_divisor(&options, 17, Some(&count.to_le_bytes())).is_err());
    }
    assert!(adaptive::preview_divisor(&options, 17, None).is_err());
    assert!(adaptive::preview_divisor(&options, 0, None).is_err());
    assert!(adaptive::preview_divisor(&options, 81, Some(&16.0f32.to_le_bytes())).is_err());
    let shader = include_str!("adaptive_preview.wgsl");
    let module = naga::front::wgsl::parse_str(shader).unwrap_or_else(|error| panic!("{}", error.emit_to_string(shader)));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap();
}

#[test]
fn actual_adaptive_pilot_and_production_readback_never_mutates_accumulation() {
    let Some(executor) = executor() else { return };
    let options = JobOptions {
        samples: 32,
        adaptive: Some(AdaptiveSampling { pilot_samples: 16, min_samples: 16, ..Default::default() }),
        ..Default::default()
    };
    let mut task =
        Task::new(executor.context.clone(), ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY }, request(3), options, CancelToken::default())
            .unwrap();
    task.environment = 1.0;
    task.studio = 0.0;
    for ordinal in 1..=options.preview_target().unwrap() {
        task.complete_sample().unwrap();
        let before = task.read_linear().unwrap();
        let preview = task.read_checkpoint().unwrap();
        assert_eq!(
            preview,
            vec![[1.0, 1.0, 1.0, 0.0]; 9],
            "pilot means and stopped production means have the same physical normalization at iteration {ordinal}"
        );
        assert_eq!(task.read_linear().unwrap(), before, "preview readback cannot write the accumulation");
    }
}

type AdaptiveOutputs = (Vec<(u32, Vec<u8>)>, Vec<u8>, Option<Vec<u8>>);
fn adaptive_output(context: Arc<Context>, options: JobOptions, preview: bool) -> AdaptiveOutputs {
    let mut task = Task::new(context, sphere_grid(white(), 8, 16), request(8), options, CancelToken::default()).unwrap();
    task.preview_enabled = preview;
    let mut images = Vec::new();
    for _ in 0..200_000 {
        let poll = task.step(4096).unwrap();
        if let Some(image) = task.take_preview() {
            assert!(task.take_linear_exr().is_none(), "pilot/production preview never exposes an export master");
            images.push((image.samples_per_pixel, image.png));
        }
        if let TaskPoll::Complete(png) = poll {
            return (images, png, task.take_linear_exr());
        }
    }
    panic!("adaptive presentation must complete within bounded test work");
}

#[test]
fn every_adaptive_prefix_is_displayed_and_preview_work_keeps_final_png_exr_identical_with_denoise() {
    let Some(executor) = executor() else { return };
    for denoise in [false, true] {
        let options = JobOptions {
            samples: 32,
            max_depth: 3,
            seed: 47,
            linear_exr: true,
            denoise,
            adaptive: Some(AdaptiveSampling { pilot_samples: 16, min_samples: 16, ..Default::default() }),
            finishing: solidcraft_render::job::finishing::Finishing {
                rgb_gain: [1.2, 0.9, 1.1],
                contrast: 1.1,
                bloom: Some(solidcraft_render::job::bloom::Bloom { strength: 0.2, threshold: 0.1, radius: 1 }),
                photographic: solidcraft_render::job::photographic::Photographic { grain: 0.01, seed: 17, ..Default::default() },
                ..Default::default()
            },
            ..Default::default()
        };
        let baseline = adaptive_output(executor.context.clone(), options, false);
        let shown = adaptive_output(executor.context.clone(), options, true);
        assert_eq!(
            shown.0.iter().map(|image| image.0).collect::<Vec<_>>(),
            (1..options.preview_target().unwrap()).collect::<Vec<_>>(),
            "every complete pilot and production iteration is a real image"
        );
        assert_eq!(shown.1, baseline.1, "display readbacks, normalization, effects and denoise cannot change final PNG bytes");
        assert_eq!(shown.2, baseline.2, "raw final EXR must be byte-identical with preview work enabled");
        assert!(shown.2.is_some());
        for production in [16, 31] {
            let prefix = adaptive_output(executor.context.clone(), JobOptions { samples: production, ..options }, false);
            let provisional = &shown.0.iter().find(|image| image.0 == 16 + production).unwrap().1;
            let a = image::load_from_memory(provisional).unwrap().into_rgba8();
            let b = image::load_from_memory(&prefix.1).unwrap().into_rgba8();
            // In-place GPU final division and the display-only copy may differ by one
            // display rounding unit; both represent this exact production prefix.
            assert!(
                a.as_raw().iter().zip(b.as_raw()).all(|(a, b)| a.abs_diff(*b) <= 1),
                "the normalized production prefix must match a standalone capped final, denoise={denoise}, production={production}"
            );
        }
    }
}

#[test]
fn cancellation_during_adaptive_preview_encoding_cannot_publish_a_preview_or_master() {
    let Some(executor) = executor() else { return };
    let options = JobOptions {
        samples: 16,
        denoise: true,
        linear_exr: true,
        adaptive: Some(AdaptiveSampling { pilot_samples: 16, min_samples: 16, ..Default::default() }),
        ..Default::default()
    };
    let cancel = CancelToken::default();
    let mut task = Task::new(executor.context.clone(), sphere_grid(white(), 8, 16), request(4), options, cancel.clone()).unwrap();
    task.preview_enabled = true;
    for _ in 0..100_000 {
        task.step(1).unwrap();
        if task.encoding_preview && task.png.is_some() {
            break;
        }
    }
    assert!(task.encoding_preview && task.png.is_some());
    cancel.cancel();
    assert_eq!(task.step(1).err().unwrap().code, ErrorCode::Cancelled);
    assert!(task.take_preview().is_none());
    assert!(task.take_linear_exr().is_none());
}
