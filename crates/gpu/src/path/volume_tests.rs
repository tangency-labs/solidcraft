use super::*;
use solidcraft_render::job::{StudioLighting, volume::HomogeneousVolume};

fn empty() -> ProductScene {
    ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY }
}
fn slab() -> HomogeneousVolume {
    HomogeneousVolume {
        min_mm: [-10000.0, -10000.0, -1.0],
        max_mm: [10000.0, 10000.0, 1.0],
        extinction_per_mm: 0.5,
        scattering_albedo: [0.8; 3],
        anisotropy: 0.0,
    }
}
fn settings(volume: HomogeneousVolume) -> JobOptions {
    JobOptions {
        volume: Some(volume),
        lighting: Some(StudioLighting { environment: [1.0; 3], sun: None, area_lights: Default::default() }),
        samples: 1024,
        max_depth: 1,
        seed: 3951,
        ..Default::default()
    }
}
fn camera() -> ImageRequest {
    let mut r = request(8);
    r.camera = solidcraft_render::Camera { distance: 4.0, pitch: std::f64::consts::FRAC_PI_2, yaw: 0.0, fov: 0.001, ..Default::default() };
    r
}

#[test]
fn volume_admission_refuses_glass_surface_denoising_legacy_lighting_and_lost_position_precision() {
    let options = settings(slab());
    assert!(admission(&empty(), &camera(), options).is_ok());
    assert!(admission(&glass_slab(true), &camera(), options).unwrap_err().message.contains("glass"));
    assert!(admission(&empty(), &camera(), JobOptions { denoise: true, ..options }).unwrap_err().message.contains("denois"));
    assert!(admission(&empty(), &camera(), JobOptions { lighting: None, ..options }).is_err());
    let too_dense = HomogeneousVolume { min_mm: [1e8; 3], max_mm: [1e8 + 100.0; 3], extinction_per_mm: 1000.0, ..slab() };
    assert!(admission(&empty(), &camera(), JobOptions { volume: Some(too_dense), ..options }).is_err());
    let base = JobOptions { volume: None, ..options };
    let a = admission(&empty(), &camera(), base).unwrap();
    let b = admission(&empty(), &camera(), options).unwrap();
    assert_eq!(a.estimated_total_bytes, b.estimated_total_bytes, "homogeneous metadata fits the existing task allowance, no unbilled voxel buffers");
}

#[test]
fn gpu_volume_beer_coverage_and_single_scatter_match_independent_slab_quadrature() {
    let Some(executor) = executor() else { return };
    let medium = slab();
    // Independent integration over collision depth and the isotropic outgoing
    // cosine. The finite 20-m lateral bounds are optically opaque at grazing
    // angles, making the infinite-slab formula indistinguishable at this tolerance.
    let mut expected = 0.0;
    for z in 0..512 {
        let depth = (f64::from(z) + 0.5) * 2.0 / 512.0;
        let mut angular = 0.0;
        for i in 0..1024 {
            let cosine = -1.0 + (f64::from(i) + 0.5) * 2.0 / 1024.0;
            let exit = if cosine > 0.0 { depth / cosine } else { (2.0 - depth) / (-cosine) };
            angular += (-medium.extinction_per_mm * exit).exp() / 1024.0;
        }
        expected += medium.extinction_per_mm * (-medium.extinction_per_mm * depth).exp() * 0.8 * angular * 2.0 / 512.0;
    }
    for scattering in [false, true] {
        let v = if scattering { medium } else { HomogeneousVolume { scattering_albedo: [0.0; 3], ..medium } };
        let options = settings(v);
        let mut task = Task::new(executor.context.clone(), empty(), camera(), options, CancelToken::default()).unwrap();
        while task.sample < options.samples {
            task.dispatch().unwrap();
        }
        let pixels = task.read_linear().unwrap();
        let alpha = pixels.iter().map(|p| f64::from(p[3])).sum::<f64>() / pixels.len() as f64;
        let rgb = pixels.iter().map(|p| f64::from(p[0])).sum::<f64>() / pixels.len() as f64;
        eprintln!("homogeneous slab scattering={scattering}: alpha={alpha}, RGB={rgb}, quadrature={expected}");
        assert!((alpha - (1.0 - (-1.0_f64).exp())).abs() < 0.012);
        if scattering {
            assert!((rgb - expected).abs() < 0.012);
        } else {
            assert_eq!(rgb, 0.0);
        }
    }
    let options = JobOptions { samples: 16, lighting: None, volume: None, ..Default::default() };
    let (base, raw) = complete_linear_fixture(executor.context.clone(), options);
    let (zero, same) =
        complete_linear_fixture(executor.context, JobOptions { volume: Some(HomogeneousVolume { extinction_per_mm: 0.0, ..medium }), ..options });
    assert_eq!((base, raw), (zero, same), "zero extinction must not consume extra variates or change old pixels");
}

#[test]
fn gpu_hg_inverse_cdf_and_phase_match_f64_including_nearly_isotropic_media() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let source = format!(
        "{SHADER}\n{}",
        r#"
@compute @workgroup_size(64)
fn phase_reference(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x>=4096u {return;}
    let cosine=volume_cosine((f32(gid.x)+0.5)/4096.0);
    accumulation[gid.x]=vec4<f32>(volume_phase(cosine),cosine+1.0,0.0,1.0);
}
"#
    );
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(source.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("phase_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    for g in [-0.8, -0.00005, 0.0, 0.00005, 0.8] {
        let medium = HomogeneousVolume { anisotropy: g, ..slab() };
        let mut task = Task::new(context.clone(), empty(), request(64), settings(medium), CancelToken::default()).unwrap();
        context.queue.write_buffer(&task.uniform, 0, &task.parameters(4096));
        let mut encoder = context.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &task.bind, &[]);
            pass.dispatch_workgroups(64, 1, 1);
        }
        context.queue.submit([encoder.finish()]);
        task.sample = 1;
        let pixels = task.read_linear().unwrap();
        let mean = pixels.iter().map(|p| f64::from(p[1]) - 1.0).sum::<f64>() / 4096.0;
        assert!((mean - g).abs() < 1e-5, "g={g} mean={mean}");
        for p in pixels {
            let cosine = (f64::from(p[1]) - 1.0).clamp(-1.0, 1.0);
            let oracle = medium.phase(cosine).unwrap();
            assert!((f64::from(p[0]) - oracle).abs() < oracle * 0.0001 + 1e-6);
        }
    }
}
