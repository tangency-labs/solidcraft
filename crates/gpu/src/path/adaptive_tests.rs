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
    assert_eq!(new.estimated_total_bytes - old.estimated_total_bytes, 16 * 16 * 32);
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
