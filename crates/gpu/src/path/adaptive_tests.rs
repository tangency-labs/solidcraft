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
    assert_eq!(report.budget_limited_pixels, 0);
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
    assert!(budget.budget_limited_pixels > 0 && budget.budget_limited_pixels <= budget.pixels_at_cap);
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
            optical_min_samples: 0,
            pilot_samples: 16,
            min_samples: 16,
            relative_error: 0.1,
            absolute_error: 0.01,
            coverage_error: 0.1,

            component_uncertainty: false,
            map_validation_batches: 0,
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
        assert_eq!(adaptive::preview_divisor(&options, 17, Some(&count.to_le_bytes())).unwrap_err().code, ErrorCode::Internal);
    }
    assert_eq!(adaptive::preview_divisor(&options, 17, None).unwrap_err().code, ErrorCode::Internal);
    assert_eq!(adaptive::preview_divisor(&options, 17, Some(&[0; 3])).unwrap_err().code, ErrorCode::Internal);
    assert_eq!(adaptive::preview_divisor(&options, 0, None).unwrap_err().code, ErrorCode::Internal);
    assert_eq!(adaptive::preview_divisor(&options, 81, Some(&16.0f32.to_le_bytes())).unwrap_err().code, ErrorCode::Internal);
    assert_eq!(adaptive::preview_divisor(&JobOptions::default(), 1, None).unwrap_err().code, ErrorCode::Internal);
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

#[test]
fn gpu_independent_map_assessment_restores_canonical_maps_and_complete_production() {
    use solidcraft_render::job::{CausticOptions, DirectionalLightOptions, DirectionalLights, StudioLighting, adaptive::PhotonMapIdentity};
    let Some(executor) = executor() else { return };
    if executor.context.device.limits().max_storage_buffers_per_shader_stage < 7 {
        return;
    }
    for (refresh, progressive_radius) in [(false, false), (true, false), (true, true)] {
        let run = |assessment| {
            let scene = photon_receiver_fixture(0.0);
            let mut image = request(1);
            image.camera.target = -Vec3::Z;
            image.camera.distance = 0.1;
            let policy = AdaptiveSampling {
                pilot_samples: 16,
                min_samples: 16,
                component_uncertainty: assessment,
                map_validation_batches: if assessment { 2 } else { 0 },
                ..Default::default()
            };
            let options = JobOptions {
                samples: 16,
                max_depth: 1,
                transmission_samples: 2,
                adaptive: Some(policy),
                lighting: Some(StudioLighting {
                    directional_lights: DirectionalLights::new(&[DirectionalLightOptions {
                        direction: [0.0, 0.0, 1.0],
                        color: [1.0; 3],
                        intensity: 1.0,
                    }])
                    .unwrap(),
                    ..Default::default()
                }),
                caustics: Some(CausticOptions {
                    photons: 128,
                    radius_mm: 3.0,
                    max_depth: 4,
                    refresh_per_pass: refresh,
                    progressive_radius,
                    ..Default::default()
                }),
                ..Default::default()
            };
            let admitted = admission(&scene, &image, options).unwrap();
            if assessment {
                assert_eq!(admitted.caustic_work.unwrap().maps, if refresh { 18 } else { 3 });
            }
            let mut task = Task::new(executor.context.clone(), scene, image, options, CancelToken::default()).unwrap();
            assert!(!radiance::enabled(options));
            let mut production_map = None;
            for pass in 0..32 {
                task.complete_sample().unwrap();
                let resources = task.photons.as_ref().unwrap();
                assert_eq!(resources.test_map_identity(), PhotonMapIdentity::for_pass(options.caustics.unwrap(), Some(policy), pass).unwrap());
                if pass == 16 {
                    production_map = Some(resources.test_records(&task.context));
                }
            }
            let report = adaptive::report(&task).unwrap();
            assert_eq!(report.production_samples, 16);
            assert_eq!(report.primary_path_evaluations, 48);
            if assessment {
                let evidence = report.pilot_evidence.unwrap();
                assert_eq!(evidence.map_batches, 2);
                assert_eq!(evidence.camera_observations_per_map, 8);
                assert_eq!(evidence.invalid_evidence_pixels, 0);
                assert!(evidence.photon_query_observations > 0);
            }
            (task.read_linear().unwrap(), production_map.unwrap())
        };
        let baseline = run(false);
        let assessed = run(true);
        assert_eq!(assessed.1, baseline.1, "assessment must restore indexed canonical production photon records");
        for (actual, expected) in assessed.0.iter().flatten().zip(baseline.0.iter().flatten()) {
            assert!((actual - expected).abs() <= 3e-6 * actual.abs().max(expected.abs()).max(1.0));
        }
    }
}

#[test]
fn gpu_pilot_component_scratch_is_admitted_and_preserves_paged_optical_production() {
    let Some(executor) = executor() else { return };
    let scene = rough_glass_fixture(true, 0.35, Vec3::ZERO, 0.0);
    assert!(scene.meshes[0].transmission.unwrap().thin_wall_thickness_mm().is_some(), "the open sheet must retain explicit thin-wall intent");
    assert!(needs_glass_lighting(&scene));
    let image = ImageRequest { width: 2, height: 4, ..request(2) };
    let run = |component_uncertainty, rows| {
        let options = JobOptions {
            samples: 16,
            max_depth: 4,
            transmission_samples: 2,
            adaptive: Some(AdaptiveSampling { pilot_samples: 16, min_samples: 16, component_uncertainty, ..Default::default() }),
            ..Default::default()
        };
        let mut task =
            Task::new_with_page_rows(executor.context.clone(), scene.clone(), image.clone(), options, CancelToken::default(), rows).unwrap();
        assert!(task.optical_resolve.is_some());
        assert!(!radiance::enabled(options));
        assert_eq!(task.page_layout.planes, 3 + if component_uncertainty { 23 } else { 0 });
        for page in &task.pages {
            assert_eq!(page.accumulation.size(), u64::from(page.pixels) * u64::from(task.page_layout.planes) * 16 + task.page_layout.optical_bytes);
        }
        for _ in 0..32 {
            task.complete_sample().unwrap();
        }
        let report = adaptive::report(&task).unwrap();
        if component_uncertainty {
            assert_eq!(report.pilot_evidence.as_ref().unwrap().invalid_evidence_pixels, 0);
        }
        (task.read_linear().unwrap(), report)
    };
    let baseline = run(false, None);
    let whole = run(true, None);
    let paged = run(true, Some(2));
    assert_eq!(whole.0, baseline.0, "pilot capture must not alter the complete production mean");
    assert_eq!(paged, whole, "page boundaries cannot change pilot component or deferred optical scratch ownership");
}

#[test]
fn gpu_pilot_evidence_matches_scalar_cases_and_marks_invalid_or_missing_support() {
    use solidcraft_render::job::CausticOptions;
    let Some(executor) = executor() else { return };
    if executor.context.device.limits().max_storage_buffers_per_shader_stage < 7 {
        return;
    }
    let context = &executor.context;
    let source = format!(
        "{SHADER}\n{}",
        r#"
@compute @workgroup_size(1)
fn pilot_evidence_reference() {
    radiance_reset();
    var value=vec3<f32>(1.0,0.0,0.0);
    if u.sampling.x==1u {value=vec3<f32>(select(0.0,1.0,u.image.z>=8u),0.0,0.0);}
    if u.sampling.x==2u {value=vec3<f32>(0.0);}
    if u.sampling.x==3u {value=vec3<f32>(1e20);}
    if u.sampling.x==4u {value=vec3<f32>(2.0*f32(u.image.z%2u+select(0u,1u,u.image.z>=8u)),0.0,0.0);}
    if u.image.z%2u==0u {radiance_components[2]=value;}else {radiance_components[4]=value;}
    adaptive_photon_begin();
    if u.sampling.x!=2u {adaptive_photon_contribution(vec3<f32>(1e30));adaptive_photon_contribution(vec3<f32>(2e30));}
    accumulate_path(0u,vec4<f32>(value,1.0));
}
"#
    );
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("independent pilot numerical cases"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let photon_layout = &photons::Pipelines::shared(context).unwrap().layout;
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(photon_layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("pilot_evidence_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    for (mode, refresh) in [(0, false), (1, false), (1, true), (2, false), (3, false), (4, false), (4, true)] {
        let policy = AdaptiveSampling {
            pilot_samples: 16,
            min_samples: 16,
            component_uncertainty: mode != 1 && mode != 4,
            map_validation_batches: if matches!(mode, 1 | 2 | 4) { 2 } else { 0 },
            relative_error: 0.0,
            absolute_error: if mode == 4 { 0.24 } else { 0.125 },
            ..Default::default()
        };
        let options = JobOptions {
            samples: 64,
            seed: mode,
            adaptive: Some(policy),
            clamp_indirect: Some(0.5),
            caustics: Some(CausticOptions { photons: 1, radius_mm: 3.0, max_depth: 4, refresh_per_pass: refresh, ..Default::default() }),
            ..Default::default()
        };
        let mut task = Task::new(context.clone(), photon_receiver_fixture(0.0), request(1), options, CancelToken::default()).unwrap();
        // The clamp and independent-map assessment share the page uniform but
        // have separate bit fields; neither may alter the other's semantics.
        let parameters = task.parameters(1).unwrap();
        assert_eq!(u32::from_le_bytes(parameters[PARAMETER_BYTES - 8..PARAMETER_BYTES - 4].try_into().unwrap()), 0.5_f32.to_bits());
        assert_eq!(
            u32::from_le_bytes(parameters[PARAMETER_BYTES - 4..PARAMETER_BYTES].try_into().unwrap()),
            (policy.map_validation_batches << 1) | u32::from(refresh)
        );
        for ordinal in 0..16 {
            task.sample = ordinal;
            context.queue.write_buffer(&task.uniform, 0, &task.parameters(1).unwrap());
            let mut encoder = context.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &task.bind, &[]);
                pass.dispatch_workgroups(1, 1, 1);
            }
            let submitted = context.queue.submit([encoder.finish()]);
            context.wait(submitted, TIMEOUT).unwrap();
        }
        task.sample = 16;
        // This reference reads the frozen pilot allocation directly; no
        // production observations are manufactured to support the statistics.
        let report = adaptive::report(&task).unwrap();
        let evidence = report.pilot_evidence.unwrap();
        match mode {
            0 => {
                assert_eq!(report.production_samples, 64);
                assert_eq!(evidence.component_budget_limited_pixels, 1);
            }
            1 => {
                if refresh {
                    assert!((32..=33).contains(&report.production_samples));
                } else {
                    assert_eq!(report.production_samples, 16);
                }
                assert!((evidence.maximum_map_scatter_to_tolerance - 0.5_f64.sqrt() / 0.125).abs() < 1e-5);
            }
            2 => {
                assert_eq!(report.production_samples, 16);
                assert_eq!(evidence.photon_empty_observations, 16);
                assert_eq!(evidence.minimum_query_effective_event_support, None);
            }
            4 => {
                assert_eq!(report.production_samples, if refresh { 53 } else { 20 });
                assert!((evidence.maximum_map_scatter_to_tolerance - (13.0_f64 / 7.0).sqrt() / 0.24).abs() < 1e-5);
            }
            _ => {
                assert_eq!(report.production_samples, 64);
                assert_eq!(evidence.invalid_evidence_pixels, 1);
            }
        }
        assert_eq!(evidence.photon_query_observations, 16);
        if mode != 2 {
            assert!((evidence.minimum_query_effective_event_support.unwrap() - 1.8).abs() < 1e-5);
        }
        if mode != 3 {
            assert_eq!(evidence.invalid_evidence_pixels, 0);
        }
        task.validate_failures(None).unwrap();
    }
}

#[test]
fn assessment_epoch_keys_cannot_alias_admitted_refreshed_map_keys() {
    // The actual shader's mix is a composition of bijective xor-shifts and odd
    // products. Audit its two epoch domains over every admitted logical pass;
    // the reserved high bit is separate from logical progress and radius epoch.
    fn mix(mut value: u32) -> u32 {
        value ^= value >> 16;
        value = value.wrapping_mul(0x7feb352d);
        value ^= value >> 15;
        value = value.wrapping_mul(0x846ca68b);
        value ^ (value >> 16)
    }
    let maximum = solidcraft_render::job::MAX_SAMPLES_PER_PIXEL + solidcraft_render::job::adaptive::MAX_PILOT_SAMPLES;
    for domain in [0x8da6b343_u32, 0x6b8b4567] {
        let production = (1..maximum).map(|epoch| mix(epoch.wrapping_add(domain))).collect::<std::collections::HashSet<_>>();
        for batch in 0..8 {
            assert!(!production.contains(&mix((0x8000_0000_u32 | batch).wrapping_add(domain))));
        }
    }
}

#[test]
fn gpu_smooth_split_matches_fresnel_eta_and_zero_child_oracles() {
    use solidcraft_render::glass_model::{Dispersion, GlassModel, WAVELENGTHS_NM};
    use solidcraft_render::sampling::Sampler;
    let Some(executor) = executor() else { return };
    for (ior, inside, tir, spectral) in
        [(1.0, false, false, false), (3.0, false, false, false), (1.5, true, false, false), (1.5, true, true, false), (1.5, false, false, true)]
    {
        let mut source = glass_slab(false);
        source.meshes[0].surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, ior).unwrap());
        let model = GlassModel { dispersion: spectral.then_some(Dispersion { abbe_number: 50.0 }), ..Default::default() };
        source.meshes[0].transmission =
            Some(solidcraft_render::transmission::Transmission::new(Vec3::ZERO, None).unwrap().with_model(model).unwrap());
        let mut image = request(2);
        image.camera.fov = if inside { 1e-6 } else { 0.0 };
        if inside {
            image.camera.distance = 1.0;
            if tir {
                image.camera.yaw = std::f64::consts::FRAC_PI_4;
                image.camera.pitch = (1.0 / 3.0_f64.sqrt()).asin();
            }
            image.camera.target = -image.camera.back();
        }
        let options = JobOptions {
            samples: 4,
            max_depth: 2,
            split_first_interface: true,
            sampler: Sampler::ScrambledSobol,
            nested_media: inside,
            radiance_components: true,
            ..Default::default()
        };
        let mut task = Task::new(executor.context.clone(), source, image, options, CancelToken::default()).unwrap();
        task.environment = 1.0;
        task.studio = 0.0;
        for _ in 0..options.samples {
            task.complete_sample().unwrap();
        }
        let pixels = task.read_linear().unwrap();
        for (pixel, actual) in pixels.iter().enumerate() {
            let mut expected = [0.0; 3];
            if spectral {
                for ordinal in 0..options.samples {
                    let hero = (solidcraft_render::sampling::scrambled_sobol_pair(0, pixel as u64, ordinal, 0x5741_5645)[0] * 3.0) as usize;
                    let index = model.ior(ior, WAVELENGTHS_NM[hero]).unwrap();
                    expected[hero] += 3.0 * ((index - 1.0) / (index + 1.0)).powi(2) / f64::from(options.samples);
                }
            } else {
                let fresnel = ((ior - 1.0) / (ior + 1.0)).powi(2);
                expected.fill(if tir {
                    0.0
                } else if inside {
                    (1.0 - fresnel) * ior * ior
                } else {
                    fresnel
                });
            }
            for (actual, expected) in actual[..3].iter().zip(expected) {
                assert!(
                    (f64::from(*actual) - expected).abs() < 2e-6,
                    "ior={ior}, inside={inside}, tir={tir}, spectral={spectral}: {actual} vs {expected}"
                );
            }
            assert_eq!(actual[3], 1.0);
        }
        let stats = radiance::read(&task).unwrap().report;
        assert_eq!((stats.observations, stats.minimum_samples, stats.maximum_samples), (16, 4, 4));
        assert!(stats.maximum_recombination_error < 1e-6);
        let report = adaptive::report(&task).unwrap();
        assert_eq!(
            (report.transmission_strata, report.production_lanes(), report.production_samples, report.primary_path_evaluations),
            (1, 2, 16, 32)
        );
        assert!(report.split_first_interface);
    }
}

#[test]
fn gpu_smooth_split_paging_adaptive_counts_and_partial_group_cancellation() {
    let Some(executor) = executor() else { return };
    let render = |rows| {
        let mut source = glass_slab(false);
        source.meshes[0].surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 3.0).unwrap());
        source.meshes[0].transmission = Some(solidcraft_render::transmission::Transmission::new(Vec3::ZERO, None).unwrap());
        let options = JobOptions {
            samples: 16,
            max_depth: 2,
            split_first_interface: true,
            radiance_components: true,
            adaptive: Some(AdaptiveSampling { pilot_samples: 16, min_samples: 16, component_uncertainty: true, ..Default::default() }),
            ..Default::default()
        };
        let mut image = request(2);
        image.camera.fov = 0.0;
        let mut task = Task::new_with_page_rows(executor.context.clone(), source, image, options, CancelToken::default(), rows).unwrap();
        task.environment = 1.0;
        task.studio = 0.0;
        for _ in 0..32 {
            task.complete_sample().unwrap();
        }
        let stats = radiance::read(&task).unwrap().report;
        let report = adaptive::report(&task).unwrap();
        assert_eq!((stats.observations, stats.minimum_samples, stats.maximum_samples), (64, 16, 16));
        assert_eq!((report.production_samples, report.primary_path_evaluations, report.pilot_samples_per_pixel), (64, 192, 16));
        assert_eq!(report.pilot_evidence.as_ref().unwrap().invalid_evidence_pixels, 0);
        let raw = task.read_linear().unwrap();
        assert!(raw.iter().all(|pixel| *pixel == [0.25, 0.25, 0.25, 1.0]));
        (raw, serde_json::to_value(stats).unwrap(), report)
    };
    assert_eq!(render(None), render(Some(1)));
    let cancel = CancelToken::default();
    let options = JobOptions { samples: 1, max_depth: 2, split_first_interface: true, radiance_components: true, ..Default::default() };
    let mut task = Task::new(executor.context, glass_slab(false), request(1), options, cancel.clone()).unwrap();
    task.dispatch().unwrap();
    assert_eq!((task.sample, task.lane), (0, 1));
    // A half group is not a final observation. The final reader must refuse
    // this one-pixel, one-observation task until both lanes complete.
    let incomplete = radiance::read(&task).err().unwrap();
    assert_eq!(incomplete.code, ErrorCode::HostUnavailable);
    assert_eq!(incomplete.message, "GPU radiance statistics are missing complete production observations");
    cancel.cancel();
    assert_eq!(task.step(1).err().unwrap().code, ErrorCode::Cancelled);
    assert!(task.take_linear_exr().is_none());
}

#[test]
fn gpu_smooth_split_waits_for_bare_closed_boundary_after_unsupported_prefixes() {
    use solidcraft_render::glass_model::{GlassModel, ThinFilm};
    use solidcraft_render::sampling::Sampler;
    let Some(executor) = executor() else { return };
    for kind in 0..3 {
        let mut source = glass_slab(false);
        source.meshes[0].surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 3.0).unwrap());
        source.meshes[0].transmission = Some(solidcraft_render::transmission::Transmission::new(Vec3::ZERO, None).unwrap());
        let mut prefix = glass_slab(kind == 0).meshes.remove(0);
        prefix.name = "unsupported split prefix".into();
        for point in &mut Arc::make_mut(&mut prefix.mesh).positions {
            point.z += 2.0;
        }
        prefix.surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.0).unwrap());
        let model = match kind {
            1 => GlassModel { roughness: 0.4, ..Default::default() },
            2 => GlassModel { thin_film: Some(ThinFilm { ior: 1.0, thickness_nm: 100.0 }), ..Default::default() },
            _ => GlassModel::default(),
        };
        prefix.transmission =
            Some(solidcraft_render::transmission::Transmission::new(Vec3::ZERO, (kind == 0).then_some(0.0)).unwrap().with_model(model).unwrap());
        source.meshes.push(prefix);
        let mut black = glass_slab(true).meshes.remove(0);
        black.name = "absorbing interior receiver".into();
        for point in &mut Arc::make_mut(&mut black.mesh).positions {
            point.z -= 0.5;
        }
        black.transmission = None;
        black.surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 1.0, 1.0).unwrap());
        source.meshes.push(black);
        source.bounds = source.meshes.iter().fold(Aabb3::EMPTY, |bounds, body| bounds.union(&body.mesh.bounds()));
        let mut image = request(1);
        image.camera.fov = 0.0;
        let options = JobOptions {
            samples: 1,
            max_depth: 7,
            split_first_interface: true,
            sampler: Sampler::ScrambledSobol,
            radiance_components: true,
            ..Default::default()
        };
        let mut task = Task::new(executor.context.clone(), source, image, options, CancelToken::default()).unwrap();
        task.environment = 1.0;
        task.studio = 0.0;
        task.complete_sample().unwrap();
        let pixel = task.read_linear().unwrap()[0];
        let mut expected = [0.25; 3];
        if kind == 2 {
            let hero = (solidcraft_render::sampling::scrambled_sobol_pair(0, 0, 0, 0x5741_5645)[0] * 3.0) as usize;
            expected = [0.0; 3];
            expected[hero] = 0.75;
        }
        for (actual, expected) in pixel[..3].iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-6, "unsupported prefix={kind}: {pixel:?}");
        }
        assert_eq!(radiance::read(&task).unwrap().report.observations, 1);
    }
}

#[test]
fn gpu_smooth_split_preserves_shared_deferred_light_prefix_before_and_without_eligible_boundary() {
    use solidcraft_render::job::{DirectionalLightOptions, DirectionalLights, StudioLighting};
    let Some(executor) = executor() else { return };
    for later_bare_boundary in [false, true] {
        let mut multiple_offers = false;
        let mut positive_prefix = false;
        for seed in 0..4 {
            let render = |split_first_interface| {
                let options = JobOptions {
                    samples: 1,
                    max_depth: if later_bare_boundary { 3 } else { 8 },
                    seed,
                    split_first_interface,
                    radiance_components: true,
                    lighting: Some(StudioLighting {
                        directional_lights: DirectionalLights::new(&[DirectionalLightOptions {
                            direction: [0.0, 0.0, 1.0],
                            color: [1.0, 0.7, 0.3],
                            intensity: 1.0,
                        }])
                        .unwrap(),
                        ..Default::default()
                    }),
                    ..Default::default()
                };
                let mut source = rough_glass_fixture(true, 0.45, Vec3::new(0.03, 0.15, 0.08), 0.0);
                if later_bare_boundary {
                    for point in &mut Arc::make_mut(&mut source.meshes[0].mesh).positions {
                        point.z += 2.0;
                    }
                    let mut bare = glass_slab(false).meshes.remove(0);
                    bare.name = "bare boundary after deferred prefix".into();
                    bare.surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 3.0).unwrap());
                    bare.transmission = Some(solidcraft_render::transmission::Transmission::new(Vec3::ZERO, None).unwrap());
                    source.meshes.push(bare);
                    source.bounds = source.meshes.iter().fold(Aabb3::EMPTY, |bounds, body| bounds.union(&body.mesh.bounds()));
                }
                let mut image = request(1);
                image.camera.fov = 0.000001;
                let mut task = Task::new(executor.context.clone(), source, image, options, CancelToken::default()).unwrap();
                assert!(task.optical_resolve.is_some());
                task.complete_sample().unwrap();
                let offset = (u64::from(task.page_layout.planes) + 6) * 16;
                let count = task
                    .readback_data(
                        16,
                        |encoder| {
                            encoder.copy_buffer_to_buffer(&task.accumulation, offset, &task.readback, 0, 16);
                            Ok(())
                        },
                        |bytes| Ok(f32::from_le_bytes(bytes[..4].try_into().unwrap())),
                    )
                    .unwrap();
                (task.read_linear().unwrap(), serde_json::to_value(radiance::read(&task).unwrap().report).unwrap(), count)
            };
            let ordinary = render(false);
            let split = render(true);
            multiple_offers |= ordinary.2 > 1.0;
            positive_prefix |= ordinary.0[0][0] > 0.0;
            // A three-event cap admits the two rough sheet interfaces and the
            // bare split, but no suffix endpoint. Deferred prefix energy and its
            // random reservoir choice must therefore match the unsplit result.
            assert_eq!(ordinary, split, "shared deferred-light prefix, later bare boundary={later_bare_boundary}");
        }
        assert!(multiple_offers && positive_prefix, "the fixture must select among multiple optical connections with nonzero prefix radiance");
    }
}

#[test]
fn gpu_smooth_split_photon_components_and_camera_probe_close_after_both_lanes() {
    use solidcraft_render::job::{CausticOptions, DirectionalLightOptions, DirectionalLights, StudioLighting};
    let Some(executor) = executor() else { return };
    for refreshed in [false, true] {
        let mut source = glass_slab(false);
        source.meshes[0].transmission =
            Some(solidcraft_render::transmission::Transmission::new(Vec3::ZERO, None).unwrap().with_shadow_transmission(false));
        let mut receiver = glass_slab(true).meshes.remove(0);
        receiver.name = "interior caustic receiver".into();
        for point in &mut Arc::make_mut(&mut receiver.mesh).positions {
            point.z -= 0.5;
        }
        receiver.transmission = None;
        receiver.surface = Some(OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 0.0, 1.0, 1.0).unwrap());
        source.meshes.push(receiver);
        let options = JobOptions {
            samples: 2,
            max_depth: 2,
            split_first_interface: true,
            radiance_components: true,
            lighting: Some(StudioLighting {
                directional_lights: DirectionalLights::new(&[DirectionalLightOptions {
                    direction: [0.0, 0.0, 1.0],
                    color: [1.0; 3],
                    intensity: 1.0,
                }])
                .unwrap(),
                ..Default::default()
            }),
            caustics: Some(CausticOptions {
                photons: 1024,
                radius_mm: 3.0,
                max_depth: 8,
                seed: 81,
                refresh_per_pass: refreshed,
                progressive_radius: refreshed,
                probe_pixel: Some([0, 0]),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut image = request(1);
        image.camera.fov = 0.000001;
        let mut task = Task::new(executor.context.clone(), source, image, options, CancelToken::default()).unwrap();
        for _ in 0..2 {
            task.complete_sample().unwrap();
        }
        let raw = task.read_linear().unwrap();
        assert!(raw[0][..3].iter().all(|value| *value > 0.0));
        let expected = raw[0].map(f64::from);
        task.finish_output(raw).unwrap();
        let probe = task.photon_diagnostics[0];
        assert!(probe.split_first_interface && probe.selected && probe.camera_contributing > 0);
        assert_eq!(probe.camera.selected_sample.unwrap().strata, 2);
        assert_eq!(probe.camera.trace_pixels[0].samples, 2);
        assert_eq!(probe.camera.output.unwrap().raw_rgba, expected);
        for entry in probe.largest_camera.iter().take(probe.camera_top_count as usize) {
            assert_eq!((entry.stratum, entry.fresnel_strata, entry.camera_interfaces), (1, 2, 1));
            assert!(entry.camera_prefix.into_iter().all(|value| (value - 2.0 * 0.96 / 2.25).abs() < 2e-6));
            assert_eq!(entry.final_output_radiance.unwrap(), entry.camera_radiance.map(|value| value / 4.0));
        }
        let stats = task.radiance_report.as_ref().unwrap();
        assert_eq!((stats.observations, stats.minimum_samples, stats.maximum_samples), (2, 2, 2));
        assert!(stats.maximum_recombination_error < 2e-5);
    }
}
