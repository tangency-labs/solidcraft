use super::*;
use solidcraft_geom::MeshGeom;
use solidcraft_geom::{Aabb3, Mesh};
use solidcraft_render::ProductMesh;
use solidcraft_viewport::{Camera, StandardView, surface::OpaqueSurface};

fn executor() -> Option<Executor> {
    let (adapter, device, queue) = match open_headless() {
        Ok(device) => device,
        Err(e) => {
            // The path tracer runs on Vulkan or Metal only: where the backends in use leave both out
            // (`WGPU_BACKEND=dx12`), it is unavailable by design, not missing.
            let available = !solidcraft_viewport_gpu::select::path_backends().is_empty();
            assert!(!available || std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_none(), "required GPU unavailable: {e}");
            eprintln!("gpu-path device test skipped: {e}");
            return None;
        }
    };
    eprintln!("gpu-path tests on {} ({:?}, {:?})", adapter.get_info().name, adapter.get_info().device_type, adapter.get_info().backend);
    Some(Executor::new(device, queue).unwrap())
}
fn request(size: u32) -> ImageRequest {
    let mut camera = Camera { distance: 4.0, ..Default::default() };
    camera.set_view(StandardView::Top);
    ImageRequest {
        studio_weights: None,
        integrator: Default::default(),
        raytrace: None,
        textures: None,
        environment: None,
        camera,
        width: size,
        height: size,
        background: [10, 20, 30],
        transparent_background: false,
    }
}
fn sphere(surface: OpaqueSurface) -> ProductScene {
    sphere_grid(surface, 48, 96)
}
fn sphere_grid(surface: OpaqueSurface, rows: u32, columns: u32) -> ProductScene {
    sphere_section(surface, rows, columns, rows)
}
fn sphere_section(surface: OpaqueSurface, rows: u32, columns: u32, rings: u32) -> ProductScene {
    // Original unit sphere, deliberately triangulated here rather than imported test data.
    let mut mesh = Mesh::default();
    for i in 0..=rings {
        let theta = f64::from(i) * std::f64::consts::PI / f64::from(rows);
        for j in 0..=columns {
            let phi = f64::from(j) * std::f64::consts::TAU / f64::from(columns);
            let n = Vec3::new(theta.sin() * phi.cos(), theta.sin() * phi.sin(), theta.cos());
            mesh.positions.push(n);
            mesh.normals.push(n);
        }
    }
    for i in 0..rings {
        for j in 0..columns {
            let a = i * (columns + 1) + j;
            let b = a + columns + 1;
            mesh.triangles.extend([[a, b, a + 1], [a + 1, b, b + 1]]);
            mesh.tri_face.extend([0, 0]);
        }
    }
    let bounds = mesh.bounds();
    ProductScene {
        meshes: vec![ProductMesh {
            name: "original sphere".into(),
            mesh: Arc::new(mesh),
            color: [255; 3],
            opacity: 1.0,
            surface: Some(surface),
            transmission: None,
            faces: Vec::new(),
        }],
        bounds,
    }
}
fn white() -> OpaqueSurface {
    OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 0.0, 1.0, 1.0).unwrap()
}

fn clear_glass() -> solidcraft_render::transmission::Transmission {
    solidcraft_render::transmission::Transmission::new(Vec3::ZERO, None).unwrap()
}

#[test]
fn volume_glass_requires_closed_outward_homogeneous_geometry() {
    let mut glass = sphere(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap());
    glass.meshes[0].transmission = Some(clear_glass());
    assert!(scene::flatten(&glass, Vec3::ZERO, &CancelToken::default()).is_ok());
    let mut open = glass.clone();
    let mesh = Arc::make_mut(&mut open.meshes[0].mesh);
    mesh.triangles.remove(300);
    mesh.tri_face.remove(300);
    assert!(scene::flatten(&open, Vec3::ZERO, &CancelToken::default()).is_err());
    let mut inward = glass;
    for t in &mut Arc::make_mut(&mut inward.meshes[0].mesh).triangles {
        t.swap(1, 2);
    }
    assert!(scene::flatten(&inward, Vec3::ZERO, &CancelToken::default()).is_err());
    open.meshes[0].transmission = Some(solidcraft_render::transmission::Transmission::new(Vec3::ZERO, Some(0.1)).unwrap());
    assert!(scene::flatten(&open, Vec3::ZERO, &CancelToken::default()).is_ok());
}

#[test]
fn nested_and_intersecting_dielectric_meshes_are_rejected_without_nested_media() {
    let mut outer = sphere(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap());
    outer.meshes[0].transmission = Some(clear_glass());
    for shift in [0.0, 0.65] {
        let mut inner = outer.meshes[0].clone();
        for p in &mut Arc::make_mut(&mut inner.mesh).positions {
            *p = *p * 0.5 + Vec3::new(shift, 0.0, 0.0);
        }
        let mut scene = outer.clone();
        scene.meshes.push(inner);
        assert!(scene::flatten(&scene, Vec3::ZERO, &CancelToken::default()).is_err(), "shift={shift}");
        if shift == 0.0 {
            // The numerical longitude seam is welded for topology but remains
            // in uploaded coordinates. A +X ray from the inner pole escapes;
            // enabling nesting cannot make that ambiguous certificate valid.
            let error = scene::flatten_with_media(&scene, Vec3::ZERO, &CancelToken::default(), true).err().unwrap();
            assert!(error.message.contains("inconsistent optical containment"), "{}", error.message);
        }
    }
}

#[test]
fn multiple_closed_shells_in_one_dielectric_body_are_rejected() {
    let mut scene = sphere(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap());
    scene.meshes[0].transmission = Some(clear_glass());
    let second = scene.meshes[0].mesh.clone();
    let mesh = Arc::make_mut(&mut scene.meshes[0].mesh);
    let offset = mesh.positions.len() as u32;
    mesh.positions.extend(second.positions.iter().map(|p| *p + Vec3::new(4.0, 0.0, 0.0)));
    mesh.normals.extend(&second.normals);
    mesh.triangles.extend(second.triangles.iter().map(|t| t.map(|i| i + offset)));
    mesh.tri_face.extend(&second.tri_face);
    let error = scene::flatten(&scene, Vec3::ZERO, &CancelToken::default()).err().unwrap();
    assert!(error.to_string().contains("one connected shell"), "{error}");
}

#[test]
fn gpu_exact_dielectric_matches_f64_fresnel() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("dielectric_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    let mut task = Task::new(context.clone(), sphere(white()), request(1), JobOptions::default(), CancelToken::default()).unwrap();
    for (ni, nt) in [(1.0_f64, 1.0_f64), (1.0, 1.5), (1.5, 1.0), (1.0, 3.0)] {
        for cosine in [0.0, 0.01, 0.5, (nt / ni).atan().cos(), 1.0] {
            let mut params = vec![0; 64];
            scene::vector(&mut params, Vec3::new(cosine, ni, nt), 0.0);
            params.resize(PARAMETER_BYTES, 0);
            context.queue.write_buffer(&task.uniform, 0, &params);
            let mut encoder = context.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &task.bind, &[]);
                pass.dispatch_workgroups(1, 1, 1);
            }
            context.queue.submit([encoder.finish()]);
            task.sample = 1;
            let actual = task.read_linear().unwrap()[0];
            let expected = solidcraft_render::transmission::fresnel(cosine, ni, nt).unwrap();
            for (a, b) in actual.iter().zip([expected.s, expected.p, expected.transmitted_cosine.unwrap_or(0.0)]) {
                assert!((f64::from(*a) - b).abs() < 2e-5, "Fresnel {ni}->{nt} cos={cosine}: {a} != {b}");
            }
        }
    }
}

#[test]
fn gpu_sheet_grazing_and_zero_thickness_match_physical_path_limits() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let source = format!(
        "{SHADER}\n@compute @workgroup_size(1) fn sheet_limit_reference() {{
            var t=triangle(0u);t.parameters.w=u.up.w;t.optics=vec4<f32>(u.right.xyz,t.optics.w);
            let weights=sheet(t,dielectric(u.up.x,u.up.y,u.up.z));
            accumulation[0]=vec4<f32>(weights.reflection,1.0);
            accumulation[1]=vec4<f32>(weights.transmission,1.0);
        }}"
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
        entry_point: Some("sheet_limit_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    let mut task = Task::new(context.clone(), glass_slab(true), request(2), JobOptions::default(), CancelToken::default()).unwrap();
    for (ni, nt) in [(1.0, 1.0), (1.0, 1.5)] {
        for thickness in [0.0, 0.7] {
            for cosine in [0.0, 0.0001, 0.6, 1.0] {
                for sigma in [Vec3::ZERO, Vec3::new(0.5, 1.0, 0.0)] {
                    let mut params = vec![0; 48];
                    scene::vector(&mut params, sigma, 0.0);
                    scene::vector(&mut params, Vec3::new(cosine, ni, nt), thickness as f32);
                    params.resize(PARAMETER_BYTES, 0);
                    context.queue.write_buffer(&task.uniform, 0, &params);
                    context.queue.write_buffer(&task.failures, 0, &[0; FAILURE_BYTES as usize]);
                    let mut encoder = context.device.create_command_encoder(&Default::default());
                    {
                        let mut pass = encoder.begin_compute_pass(&Default::default());
                        pass.set_pipeline(&pipeline);
                        pass.set_bind_group(0, &task.bind, &[]);
                        pass.dispatch_workgroups(1, 1, 1);
                    }
                    context.queue.submit([encoder.finish()]);
                    task.sample = 1;
                    if ni == nt && thickness > 0.0 && cosine == 0.0 {
                        let error = task.read_linear().unwrap_err();
                        assert_eq!(error.code, ErrorCode::InvalidScene);
                        assert!(error.message.contains("grazing optical path"), "{error}");
                        continue;
                    }
                    let pixels = task.read_linear().unwrap();
                    let optics = solidcraft_render::transmission::Transmission::new(sigma, Some(thickness)).unwrap();
                    let (reflection, transmission) = solidcraft_render::transmission::slab(optics, thickness, cosine, ni, nt).unwrap();
                    for (pixel, expected) in pixels.iter().take(2).zip([reflection, transmission]) {
                        for (actual, expected) in pixel.iter().zip([expected.x, expected.y, expected.z]) {
                            assert!(
                                (f64::from(*actual) - expected).abs() < 3e-5,
                                "{ni}->{nt}, thickness={thickness}, cosine={cosine}: {pixel:?} != {expected}"
                            );
                        }
                    }
                    if ni == nt && thickness == 0.0 {
                        assert_eq!(pixels[0], [0.0, 0.0, 0.0, 1.0]);
                        assert_eq!(pixels[1], [1.0; 4], "no absorption path exists even with nonzero authored sigma");
                    }
                }
            }
        }
    }
}

#[test]
fn optional_glass_gpu_film_dispersion_and_visible_normals_match_the_f64_oracle() {
    use solidcraft_render::glass_model::{Dispersion, GlassModel, ThinFilm};
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let source = format!(
        "{SHADER}\n@compute @workgroup_size(1) fn glass_model_reference() {{
        var t=triangle(0u);t.glass=u.lighting;
        let f=glass_film(u.up.x,u.up.y,u.up.z,t.glass.w,t.glass.z,u.up.w);
        accumulation[0]=vec4<f32>(f,glass_ior(u.up.z,t.glass.y,u.up.w),1.0);
        var rng=u.sampling.x;
        let event=glass_interface(t,-unit(u.back.xyz),vec3<f32>(0.0,0.0,1.0),vec3<f32>(0.0,0.0,1.0),u.up.y,u.up.z,u.up.w,true,0u,1u,&rng);
        accumulation[1]=vec4<f32>(event.direction+vec3<f32>(2.0),select(0.0,1.0,event.valid));
        accumulation[2]=vec4<f32>(event.weight,select(0.0,1.0,event.transmitted));
        let bent=vec3<f32>(0.6,0.0,0.8);
        let normal=glass_macro_normal(vec3<f32>(0.0,0.0,-1.0),bent,vec3<f32>(0.0,0.0,1.0),1.0,1.5);
        accumulation[3]=vec4<f32>(normal,1.0);
    }}"
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
        entry_point: Some("glass_model_reference"),
        // HG-only jobs use this specialization with no active model or photons.
        compilation_options: wgpu::PipelineCompilationOptions { constants: &[("GLASS_DELTA_LIGHTS", 1.0)], ..Default::default() },
        cache: None,
    });
    let mut task = Task::new(context.clone(), glass_slab(true), request(2), JobOptions::default(), CancelToken::default()).unwrap();
    let random = |state: &mut u32| {
        *state = state.wrapping_mul(747796405).wrapping_add(2891336453);
        let word = ((*state >> ((*state >> 28) + 4)) ^ *state).wrapping_mul(277803737);
        f64::from((((word >> 22) ^ word) >> 8) as f32 * (1.0 / 16777216.0))
    };
    for model in [
        GlassModel::default(),
        GlassModel { roughness: 0.35, ..Default::default() },
        GlassModel { roughness: 0.35, thin_film: Some(ThinFilm { ior: 1.38, thickness_nm: 380.0 }), ..Default::default() },
        GlassModel { dispersion: Some(Dispersion { abbe_number: 25.0 }), ..Default::default() },
    ] {
        for (ni, nt) in [(1.0, 1.5), (1.5, 1.0), (1.5, 1.5)] {
            for cosine in [0.35_f64, 1.0] {
                for wavelength in [460.0, 550.0, 610.0] {
                    let view = Vec3::new(f64::from(((1.0 - cosine * cosine).sqrt()) as f32), 0.0, f64::from(cosine as f32));
                    let mut params = vec![0; PARAMETER_BYTES];
                    params[16..20].copy_from_slice(&71_u32.to_le_bytes());
                    let mut vector = Vec::new();
                    scene::vector(&mut vector, Vec3::new(cosine, ni, nt), wavelength as f32);
                    scene::vector(&mut vector, view, 0.0);
                    scene::vector(
                        &mut vector,
                        Vec3::new(model.roughness, model.dispersion.map_or(0.0, |d| d.abbe_number), model.thin_film.map_or(0.0, |f| f.thickness_nm)),
                        model.thin_film.map_or(0.0, |f| f.ior) as f32,
                    );
                    params[64..112].copy_from_slice(&vector);
                    context.queue.write_buffer(&task.uniform, 0, &params);
                    let mut encoder = context.device.create_command_encoder(&Default::default());
                    {
                        let mut pass = encoder.begin_compute_pass(&Default::default());
                        pass.set_pipeline(&pipeline);
                        pass.set_bind_group(0, &task.bind, &[]);
                        pass.dispatch_workgroups(1, 1, 1);
                    }
                    context.queue.submit([encoder.finish()]);
                    task.sample = 1;
                    let pixels = task.read_linear().unwrap();
                    assert_eq!(pixels[3], [0.6, 0.0, 0.8, 1.0], "HG-only lighting retains a valid bent shading normal");
                    let bare = model.fresnel(f64::from(cosine as f32), ni, nt, wavelength).unwrap();
                    for (actual, expected) in pixels[0].iter().zip([bare.s, bare.p, model.ior(nt, wavelength).unwrap()]) {
                        assert!(
                            (f64::from(*actual) - expected).abs() < 5e-4,
                            "model={model:?}, {ni}->{nt}, cosine={cosine}, wavelength={wavelength}: {pixels:?} != {expected}"
                        );
                    }
                    let rough = model.roughness > 0.0 && !(ni == nt && model.thin_film.is_none_or(|f| f.thickness_nm == 0.0 || f.ior == ni));
                    let mut state = 71_u32;
                    let uniforms = if rough { [random(&mut state), random(&mut state), random(&mut state)] } else { [0.5, 0.5, random(&mut state)] };
                    let expected = model.sample(-view, Vec3::Z, ni, nt, wavelength, uniforms).unwrap();
                    if let Some(expected) = expected {
                        assert_eq!(pixels[1][3], 1.0, "{model:?}: {pixels:?}");
                        for (actual, expected) in
                            pixels[1].iter().zip([expected.direction.x + 2.0, expected.direction.y + 2.0, expected.direction.z + 2.0])
                        {
                            assert!((f64::from(*actual) - expected).abs() < 8e-4, "sampled direction differs: {model:?}: {pixels:?} != {expected}");
                        }
                        assert!(
                            (f64::from(pixels[2][0]) - expected.weight).abs() < 8e-4,
                            "sample weight differs: {model:?}: {pixels:?} != {expected:?}"
                        );
                        assert_eq!(pixels[2][3] != 0.0, expected.crosses);
                    } else {
                        assert_eq!(pixels[1][3], 0.0, "a rejected f64 visible-facet sample must not be retried: {model:?}: {pixels:?}");
                    }
                }
            }
        }
    }
}

fn rough_glass_fixture(thin: bool, roughness: f64, absorption: Vec3, scattering: f64) -> ProductScene {
    let mut scene = glass_slab(thin);
    let mut transmission = solidcraft_render::transmission::Transmission::new(absorption, thin.then_some(0.7))
        .unwrap()
        .with_shadow_transmission(false)
        .with_model(solidcraft_render::glass_model::GlassModel { roughness, ..Default::default() })
        .unwrap();
    if scattering > 0.0 {
        transmission = transmission
            .with_subsurface(solidcraft_render::transmission::subsurface::Subsurface { scattering_per_mm: scattering, anisotropy: 0.3 })
            .unwrap();
    }
    scene.meshes[0].transmission = Some(transmission);
    scene
}

fn rough_glass_pixel(context: Arc<Context>, scene: ProductScene, options: JobOptions) -> [f32; 4] {
    let mut image = request(1);
    image.camera.fov = 0.000001;
    let expected_bytes = 16 * if options.adaptive.is_some() { 3 } else { 1 } + glass_tile_bytes(&scene, &image);
    let mut task = Task::new(context, scene, image, options, CancelToken::default()).unwrap();
    assert_eq!(task.accumulation.size(), expected_bytes, "optical scratch must be appended after exactly the historical film planes");
    for _ in 0..options.samples {
        task.complete_sample().unwrap();
    }
    task.read_linear().unwrap()[0]
}

#[test]
fn optical_hard_light_specialization_ignores_identity_models_and_refuses_nonfinite_f32_ggx_peaks() {
    assert!((1..=(1 << 24)).contains(&MAX_GPU_PATH_TRIANGLES), "captured parent references must remain exact f32 integers");
    let identity = rough_glass_fixture(false, 0.0, Vec3::ZERO, 0.0);
    assert!(!needs_glass_lighting(&identity));
    assert_eq!(glass_tile_bytes(&identity, &request(1024)), 0);
    let rough = rough_glass_fixture(false, 0.4, Vec3::ZERO, 0.0);
    assert_eq!(glass_tile_bytes(&rough, &request(1)), 128);
    assert_eq!(glass_tile_bytes(&rough, &request(128)), 128 * u64::from(dispatch::MAX_PATHS));
    assert_eq!(glass_tile_bytes(&rough, &request(1024)), 128 * u64::from(dispatch::MAX_PATHS));
    assert!(needs_glass_lighting(&rough_glass_fixture(false, 0.4, Vec3::ZERO, 0.0)));
    assert!(needs_glass_lighting(&rough_glass_fixture(false, 0.0, Vec3::ZERO, 0.5)));
    let representable = rough_glass_fixture(false, 1e-9, Vec3::ZERO, 0.0);
    assert!(scene::flatten(&representable, Vec3::ZERO, &CancelToken::default()).is_ok());
    for roughness in [1e-10, 3e-10] {
        let overflow = rough_glass_fixture(false, roughness, Vec3::ZERO, 0.0);
        let failure = scene::flatten(&overflow, Vec3::ZERO, &CancelToken::default()).err().unwrap();
        assert!(failure.message.contains("roughness"), "{failure}");
    }
    let mut thin = rough_glass_fixture(true, 0.4, Vec3::ZERO, 0.0);
    assert!(scene::flatten(&thin, Vec3::ZERO, &CancelToken::default()).is_ok());
    thin.meshes[0].transmission = thin.meshes[0].transmission.map(|material| material.with_shadow_transmission(true));
    assert!(
        scene::flatten(&thin, Vec3::ZERO, &CancelToken::default()).is_err(),
        "a straight shadow cannot stand in for independent rough interfaces"
    );
}

#[test]
fn rough_glass_hard_direction_point_spot_and_contained_transmission_match_analytic_projected_bsdf() {
    use solidcraft_render::job::{DirectionalLightOptions, DirectionalLights, PunctualLightOptions, PunctualLights, SpotConeOptions, StudioLighting};
    let Some(executor) = executor() else { return };
    let roughness = 0.5_f64;
    // At normal incidence D=1/(pi*roughness^4), G=1 and F=.04.
    let reflection = 0.04 / (4.0 * std::f64::consts::PI * roughness.powi(4));
    let directional = StudioLighting {
        directional_lights: DirectionalLights::new(&[DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 1.0 }])
            .unwrap(),
        ..Default::default()
    };
    for thin in [false, true] {
        for source in 0..3 {
            let cone = (source == 2).then_some(SpotConeOptions {
                emitted_axis: [0.0, 0.0, -1.0],
                inner_half_angle_radians: 0.1,
                outer_half_angle_radians: 0.4,
            });
            let lighting = if source == 0 {
                directional
            } else {
                StudioLighting {
                    punctual_lights: PunctualLights::new(&[PunctualLightOptions {
                        position_mm: [0.0, 0.0, 4.0],
                        color: [1.0; 3],
                        intensity: 1.0,
                        reference_distance_mm: 1.0,
                        minimum_distance_mm: 0.001,
                        cone,
                    }])
                    .unwrap(),
                    ..Default::default()
                }
            };
            let options = JobOptions { samples: 1, max_depth: 1, lighting: Some(lighting), ..Default::default() };
            let mut scene = rough_glass_fixture(thin, roughness, Vec3::ZERO, 0.0);
            if source == 0 {
                Arc::make_mut(&mut scene.meshes[0].mesh).normals.fill(Vec3::new(0.6, 0.0, 0.8));
            }
            let actual = rough_glass_pixel(executor.context.clone(), scene, options);
            let expected = reflection / if source == 0 { 1.0 } else { 3.5_f64.powi(2) };
            for value in actual.into_iter().take(3) {
                assert!((f64::from(value) - expected).abs() < expected * 0.0002, "thin={thin}, source={source}: {actual:?} != {expected}");
            }
            assert_eq!(actual[3], 1.0);
        }
    }
    // The point lies inside the slab, half a millimetre past its rough entry.
    // Walter's normal-incidence BTDF*cos is (1-F)*ni^2*D/(nt-ni)^2.
    let transmission = 0.96 / (0.5_f64.powi(2) * std::f64::consts::PI * roughness.powi(4));
    let lighting = StudioLighting {
        punctual_lights: PunctualLights::new(&[PunctualLightOptions {
            position_mm: [0.0; 3],
            color: [1.0; 3],
            intensity: 1.0,
            reference_distance_mm: 1.0,
            minimum_distance_mm: 0.001,
            cone: None,
        }])
        .unwrap(),
        ..Default::default()
    };
    for scattering in [0.0, 0.6] {
        let actual = rough_glass_pixel(
            executor.context.clone(),
            rough_glass_fixture(false, roughness, Vec3::new(0.2, 0.4, 0.8), scattering),
            JobOptions { samples: 1, max_depth: 1, lighting: Some(lighting), ..Default::default() },
        );
        for (value, absorption) in actual.into_iter().take(3).zip([0.2, 0.4, 0.8]) {
            let expected = transmission * 4.0 * (-0.5_f64 * (absorption + scattering)).exp();
            assert!((f64::from(value) - expected).abs() < expected * 0.0003, "contained BTDF/Beer/HG {actual:?} != {expected}");
        }
    }
    let inactive = rough_glass_pixel(
        executor.context.clone(),
        rough_glass_fixture(false, 0.0, Vec3::ZERO, 0.0),
        JobOptions { samples: 1, max_depth: 1, lighting: Some(directional), ..Default::default() },
    );
    assert_eq!(inactive, [0.0, 0.0, 0.0, 1.0], "smooth delta glass has no continuous hard-light lobe");
    for thin in [false, true] {
        let render = |zero_source: bool| {
            let directional_lights = if zero_source {
                DirectionalLights::new(&[DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 0.0 }]).unwrap()
            } else {
                DirectionalLights::default()
            };
            rough_glass_pixel(
                executor.context.clone(),
                rough_glass_fixture(thin, 0.5, Vec3::new(0.2, 0.4, 0.8), 0.0),
                JobOptions {
                    samples: 32,
                    max_depth: 16,
                    lighting: Some(StudioLighting { directional_lights, environment: [1.0; 3], ..Default::default() }),
                    ..Default::default()
                },
            )
        };
        let plain = render(false);
        assert!(plain[0] > 0.0);
        assert_eq!(plain, render(true), "offering zero-power NEE vertices must not advance the BSDF transport RNG: thin={thin}");
    }
    let mut image = request(1);
    image.camera.target = Vec3::new(0.17, 0.07, 0.0);
    image.camera.distance = 0.01;
    image.camera.fov = 0.000001;
    let options = JobOptions {
        samples: 512,
        max_depth: 1,
        seed: 43,
        nested_media: true,
        lighting: Some(StudioLighting {
            punctual_lights: PunctualLights::new(&[PunctualLightOptions {
                position_mm: [0.17, 0.07, 0.25],
                color: [1.0; 3],
                intensity: 1.0,
                reference_distance_mm: 1.0,
                minimum_distance_mm: 0.001,
                cone: None,
            }])
            .unwrap(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut expected = 0.0;
    let phase = 0.91 / (4.0 * std::f64::consts::PI * 1.69_f64.powf(1.5));
    for ordinal in 0..options.samples {
        let mut state = ordinal.wrapping_mul(0x9e3779b9) ^ options.seed;
        let mut random = || {
            state = state.wrapping_mul(747796405).wrapping_add(2891336453);
            let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277803737);
            f64::from((((word >> 22) ^ word) >> 8) as f32 * (1.0 / 16777216.0))
        };
        let _ = (random(), random()); // Independent primary AA dimensions.
        let travel = -(1.0 - random()).ln() / 2.0;
        if travel < 0.51 {
            let distance = 0.24 + travel;
            expected += phase * (-2.0 * distance).exp() / distance.powi(2) / f64::from(options.samples);
        }
    }
    let mut task =
        Task::new(executor.context.clone(), rough_glass_fixture(false, 0.0, Vec3::ZERO, 2.0), image, options, CancelToken::default()).unwrap();
    for _ in 0..options.samples {
        task.complete_sample().unwrap();
    }
    let actual = task.read_linear().unwrap()[0];
    for value in actual.into_iter().take(3) {
        assert!(
            (f64::from(value) - expected).abs() < expected * 0.0003,
            "primary selected HG must connect to its contained delta emitter: {actual:?} != {expected}"
        );
    }
}

#[test]
fn rough_optical_two_phase_tiles_preserve_adaptive_strata_and_converged_film() {
    use solidcraft_render::job::{DirectionalLightOptions, DirectionalLights, StudioLighting, adaptive::AdaptiveSampling};
    let Some(executor) = executor() else { return };
    let lighting = StudioLighting {
        directional_lights: DirectionalLights::new(&[DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 1.0 }])
            .unwrap(),
        ..Default::default()
    };
    let options = JobOptions {
        samples: 32,
        max_depth: 1,
        seed: 1948,
        transmission_samples: 4,
        adaptive: Some(AdaptiveSampling { pilot_samples: 16, min_samples: 16, ..Default::default() }),
        lighting: Some(lighting),
        ..Default::default()
    };
    let mut image = request(17);
    image.height = 19;
    image.camera.fov = 0.000001;
    let scene = rough_glass_fixture(false, 0.5, Vec3::ZERO, 0.0);
    let pixels = u64::from(image.width) * u64::from(image.height);
    let scratch = glass_tile_bytes(&scene, &image);
    let mut task = Task::new(executor.context, scene, image, options, CancelToken::default()).unwrap();
    assert_eq!(task.accumulation.size(), pixels * 48 + scratch);
    let mut offset_seen = false;
    while task.sample < 32 {
        task.dispatch().unwrap();
        offset_seen |= task.offset > 0;
    }
    assert!(offset_seen, "the physical control must exercise a nonzero tile offset");
    let converged = task.read_linear().unwrap();
    loop {
        if matches!(task.step(4096).unwrap(), TaskPoll::Complete(_)) {
            break;
        }
    }
    let actual = task.read_linear().unwrap();
    assert_eq!(actual, converged, "later maximum-budget iterations must skip both optical phases without touching converged film");
    let expected = 0.04 / (4.0 * std::f64::consts::PI * 0.5_f64.powi(4));
    for (pixel, value) in actual.iter().enumerate() {
        assert_eq!(value[3], 1.0, "coverage at pixel{pixel}");
        for channel in &value[..3] {
            assert!(
                (f64::from(*channel) - expected).abs() < expected * 0.0002,
                "pilot/stratum film normalization at pixel{pixel}: {value:?} != {expected}"
            );
        }
    }
    let report = task.sampling_report().unwrap();
    assert_eq!(report.pilot_samples_per_pixel, 16);
    assert_eq!((report.minimum_production_samples, report.maximum_production_samples), (16, 16));
    assert_eq!(report.production_samples, pixels * 16);
    assert_eq!(report.transmission_strata, 4);
    assert_eq!(report.primary_path_evaluations, pixels * (16 + 16 * 4));
    assert_eq!(report.pixels_at_cap, 0);
}

fn rough_shadow_overlap_scene(shadow_transmission: bool) -> ProductScene {
    let floor = ProductMesh {
        name: "original unit-Lambert floor".into(),
        mesh: Arc::new(Mesh {
            positions: vec![Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 0.0), Vec3::new(-10.0, 10.0, 0.0)],
            normals: vec![Vec3::Z; 4],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            tri_face: vec![0, 0],
            ..Default::default()
        }),
        color: [255; 3],
        opacity: 1.0,
        surface: Some(OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 0.0, 1.0, 1.0).unwrap()),
        transmission: None,
        faces: Vec::new(),
    };
    let mut glass = glass_slab(false).meshes.remove(0);
    glass.name = "original off-axis rough closed box".into();
    for point in &mut Arc::make_mut(&mut glass.mesh).positions {
        *point = Vec3::new(2.0 + point.x * 0.1, point.y * 0.1, 1.0 + point.z * 1.4);
    }
    glass.transmission = Some(
        solidcraft_render::transmission::Transmission::new(Vec3::ZERO, None)
            .unwrap()
            .with_shadow_transmission(shadow_transmission)
            .with_model(solidcraft_render::glass_model::GlassModel { roughness: 0.5, ..Default::default() })
            .unwrap(),
    );
    let bounds = floor.mesh.bounds().union(&glass.mesh.bounds());
    ProductScene { meshes: vec![floor, glass], bounds }
}

#[test]
fn rough_closed_shadow_approximation_replaces_only_the_transmitted_hard_source_family() {
    use solidcraft_render::job::{PunctualLightOptions, PunctualLights, StudioLighting};
    let Some(executor) = executor() else { return };
    let lighting = StudioLighting {
        punctual_lights: PunctualLights::new(&[PunctualLightOptions {
            position_mm: [2.0, 0.0, 1.0],
            color: [1.0; 3],
            intensity: 1.0,
            reference_distance_mm: 1.0,
            minimum_distance_mm: 0.001,
            cone: None,
        }])
        .unwrap(),
        ..Default::default()
    };
    let render = |shadow, depth| {
        let options = JobOptions { samples: 512, seed: 1948, max_depth: depth, lighting: Some(lighting), ..Default::default() };
        let mut image = request(1);
        image.camera.fov = 0.000001;
        let mut task = Task::new(executor.context.clone(), rough_shadow_overlap_scene(shadow), image, options, CancelToken::default()).unwrap();
        for _ in 0..options.samples {
            task.complete_sample().unwrap();
        }
        task.read_linear().unwrap()[0]
    };
    // The floor-to-point vector is (2,0,1), d=sqrt5, n_floor.l=1/sqrt5.
    // Its first glass crossing is x=1,z=.5, with incident cosine2/sqrt5.
    // Independent s/p Fresnel equations, then the existing straight-connection
    // inside-emitter radiance conversion1/nt^2; no production Fresnel helper.
    let cosine = 2.0 / 5.0_f64.sqrt();
    let transmitted_cosine = (1.0 - (1.0 - cosine * cosine) / 1.5_f64.powi(2)).sqrt();
    let rs = (cosine - 1.5 * transmitted_cosine) / (cosine + 1.5 * transmitted_cosine);
    let rp = (1.5 * cosine - transmitted_cosine) / (1.5 * cosine + transmitted_cosine);
    let reflection = 0.5 * (rs * rs + rp * rp);
    let expected = (1.0 - reflection) / (2.25 * std::f64::consts::PI * 5.0 * 5.0_f64.sqrt());
    let one = render(true, 1);
    let two = render(true, 2);
    for actual in [one, two] {
        assert_eq!(actual[3], 1.0);
        for channel in &actual[..3] {
            assert!((f64::from(*channel) - expected).abs() < expected * 0.0002, "{actual:?} != {expected}");
        }
    }
    assert_eq!(one, two, "another opaque->rough transmitted NEE must not duplicate the enabled straight approximation");
    let blocked = render(false, 1);
    assert_eq!(blocked, [0.0, 0.0, 0.0, 1.0]);
    let physical = render(false, 2);
    assert!(
        physical[..3].iter().all(|v| v.is_finite() && *v > 0.0),
        "a sampled rough hit must receive the contained hard point when its opaque straight connection is blocked: {physical:?}"
    );
    // The positive physical control above proves that this fixed stream really
    // reaches the rough box; increasing SPP is not a substitute for this check.
}

#[test]
fn reflection_only_closed_nee_packet_keeps_its_actual_inside_medium_beer() {
    use solidcraft_render::job::{PunctualLightOptions, PunctualLights, StudioLighting};
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let lighting = StudioLighting {
        punctual_lights: PunctualLights::new(&[PunctualLightOptions {
            position_mm: [0.13, -0.07, 0.0],
            color: [1.0; 3],
            intensity: 1.0,
            reference_distance_mm: 1.0,
            minimum_distance_mm: 0.001,
            cone: None,
        }])
        .unwrap(),
        ..Default::default()
    };
    let options = JobOptions { samples: 1, max_depth: 1, lighting: Some(lighting), ..Default::default() };
    let mut scene = rough_glass_fixture(false, 0.5, Vec3::new(0.2, 0.4, 0.8), 0.0);
    let optics = scene.meshes[0].transmission.unwrap();
    scene.meshes[0].transmission = Some(optics.with_shadow_transmission(true));
    let mut task = Task::new(context.clone(), scene, request(1), options, CancelToken::default()).unwrap();
    let source=format!("{SHADER}\n@compute @workgroup_size(1) fn closed_reflection_medium_reference() {{
        if u.sampling.z==0u {{return;}}
        let hit=trace_precise(vec3<f32>(0.13,-0.07,-2.0),vec3<f32>(0.0),vec3<f32>(0.0,0.0,1.0),false,0xffffffffu,0.0,false);
        var payload:array<vec4<u32>,6>;var count=0u;var selection_rng=81u;
        glass_offer_light(&payload,&count,&selection_rng,PrecisePoint(vec3<f32>(0.13,-0.07,-0.5),vec3<f32>(0.0)),vec3<f32>(0.0,0.0,-1.0),vec3<f32>(1.0),1.5,1.0,hit.triangle,4u,glass_vacuum());
        accumulation[0]=vec4<f32>(glass_resolve_light(&payload,count,0.0),1.0);
    }}");
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(source.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let constants = [("GLASS_DELTA_LIGHTS", 1.0)];
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("closed_reflection_medium_reference"),
        compilation_options: wgpu::PipelineCompilationOptions { constants: &constants, ..Default::default() },
        cache: None,
    });
    context.queue.write_buffer(&task.uniform, 0, &[0; PARAMETER_BYTES]);
    warm_up(&context.device, &context.queue, &pipeline, &task.bind, &context.healthy).unwrap();
    context.queue.write_buffer(&task.uniform, 0, &task.parameters(1).unwrap());
    let mut encoder = context.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &task.bind, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    context.queue.submit([encoder.finish()]);
    task.sample = 1;
    let actual = task.read_linear().unwrap()[0];
    let response = 0.04 / (4.0 * std::f64::consts::PI * 0.5_f64.powi(4));
    for (value, absorption) in actual.into_iter().take(3).zip([0.2, 0.4, 0.8]) {
        let expected = response * 4.0 * (-0.5_f64 * absorption).exp();
        assert!(
            (f64::from(value) - expected).abs() < expected * 0.0003,
            "inside reflection requires its actual medium, not sheet/vacuum metadata: {actual:?} != {expected}"
        );
    }
    assert_eq!(actual[3], 1.0);
}

#[test]
fn rough_closed_and_virtual_sheet_photon_partition_keeps_primary_caustics_and_avoids_double_counting() {
    use solidcraft_render::job::{CausticOptions, DirectionalLightOptions, DirectionalLights, StudioLighting};
    let Some(executor) = executor() else { return };
    if executor.context.device.limits().max_storage_buffers_per_shader_stage < 7 {
        return;
    }
    let lighting = StudioLighting {
        directional_lights: DirectionalLights::new(&[DirectionalLightOptions { direction: [0.0, 0.0, -1.0], color: [1.0; 3], intensity: 1.0 }])
            .unwrap(),
        ..Default::default()
    };
    for thin in [false, true] {
        let render = |map: bool, depth: u32| {
            rough_glass_pixel(
                executor.context.clone(),
                rough_glass_fixture(thin, 0.45, Vec3::ZERO, 0.0),
                JobOptions {
                    samples: 32,
                    max_depth: depth,
                    lighting: Some(lighting),
                    caustics: map.then_some(CausticOptions {
                        photons: 8192,
                        radius_mm: 2.0,
                        max_depth: 8,
                        seed: 81,
                        area_lights: false,
                        environment: false,
                        guided_emission: false,
                        refresh_per_pass: false,
                        power_weighted_sources: false,
                    }),
                    ..Default::default()
                },
            )
        };
        let one = render(false, 1);
        let two = render(false, 2);
        assert_eq!(one, [0.0, 0.0, 0.0, 1.0], "the first interface cannot connect directly to an exterior backlight");
        assert!(two[0] > 0.0 && two[0].is_finite(), "map-off virtual/closed exit must receive a hard backlight: thin={thin}, {two:?}");
        // Hold one immutable photon map fixed across both camera depths. Two
        // independent parallel prepasses can sum a cell's atomic-linked records
        // in different orders and differ by an ULP, unrelated to path partition.
        let mapped_options = JobOptions {
            samples: 32,
            seed: 1948,
            max_depth: 2,
            lighting: Some(lighting),
            caustics: Some(CausticOptions {
                photons: 8192,
                radius_mm: 2.0,
                max_depth: 8,
                seed: 81,
                area_lights: false,
                environment: false,
                guided_emission: false,
                refresh_per_pass: false,
                power_weighted_sources: false,
            }),
            ..Default::default()
        };
        let mut image = request(1);
        image.camera.fov = 0.000001;
        let mut mapped =
            Task::new(executor.context.clone(), rough_glass_fixture(thin, 0.45, Vec3::ZERO, 0.0), image, mapped_options, CancelToken::default())
                .unwrap();
        mapped.options.max_depth = 1;
        for _ in 0..mapped_options.samples {
            mapped.complete_sample().unwrap();
        }
        let mapped_one = mapped.read_linear().unwrap()[0];
        assert!(mapped.photons.as_ref().unwrap().ready());
        mapped.sample = 0;
        mapped.lane = 0;
        mapped.offset = 0;
        mapped.options.max_depth = 2;
        for _ in 0..mapped_options.samples {
            mapped.complete_sample().unwrap();
        }
        let mapped_two = mapped.read_linear().unwrap()[0];
        assert!(
            mapped_one[0] > 0.0 && mapped_one[0].is_finite(),
            "primary rough gather must retain transmitted caustics: thin={thin}, {mapped_one:?}"
        );
        assert_eq!(mapped_one, mapped_two, "a sampled continuous prefix cannot add the same photon-estimated suffix twice: thin={thin}");
        let maximum_depth = render(false, 16);
        assert!(
            maximum_depth[..3].iter().all(|value| value.is_finite() && *value > 0.0),
            "all admitted sheet/interface events fit the fixed request cap: {maximum_depth:?}"
        );
        for pixel in [two, mapped_one, mapped_two, maximum_depth] {
            for channel in 1..3 {
                assert!(
                    (pixel[channel] - pixel[0]).abs() <= pixel[0].abs() * 1e-5 + 1e-7,
                    "non-spectral white caustics must remain achromatic: {pixel:?}"
                );
            }
        }
    }
    let volume = |map: bool| {
        let options = JobOptions {
            samples: 64,
            max_depth: 1,
            nested_media: true,
            lighting: Some(lighting),
            caustics: map.then_some(CausticOptions {
                photons: 8192,
                radius_mm: 0.6,
                max_depth: 8,
                seed: 81,
                area_lights: false,
                environment: false,
                guided_emission: false,
                refresh_per_pass: false,
                power_weighted_sources: false,
            }),
            ..Default::default()
        };
        let mut image = request(1);
        image.camera.distance = 0.01;
        image.camera.fov = 0.000001;
        let mut task =
            Task::new(executor.context.clone(), rough_glass_fixture(false, 0.0, Vec3::ZERO, 2.0), image, options, CancelToken::default()).unwrap();
        for _ in 0..options.samples {
            task.complete_sample().unwrap();
        }
        task.read_linear().unwrap()[0]
    };
    assert_eq!(volume(false), [0.0, 0.0, 0.0, 1.0], "the hard source lies beyond a genuinely refracting closed boundary");
    let mapped = volume(true);
    assert!(
        mapped[0] > 0.0 && mapped[..3].iter().all(|value| value.is_finite()),
        "primary selected-HG must gather refracted volume caustics: {mapped:?}"
    );
}

#[test]
fn rough_volume_photon_kernels_and_hg_delta_connection_match_independent_measures() {
    use solidcraft_render::job::{CausticOptions, PunctualLightOptions, PunctualLights, StudioLighting};
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    if context.device.limits().max_storage_buffers_per_shader_stage < 7 {
        return;
    }
    let scene = rough_glass_fixture(false, 0.5, Vec3::new(0.2, 0.4, 0.8), 0.6);
    let options = JobOptions {
        max_depth: 16,
        lighting: Some(StudioLighting {
            punctual_lights: PunctualLights::new(&[PunctualLightOptions {
                position_mm: [0.0, 0.0, 0.25],
                color: [1.0; 3],
                intensity: 1.0,
                reference_distance_mm: 1.0,
                minimum_distance_mm: 0.001,
                cone: None,
            }])
            .unwrap(),
            ..Default::default()
        }),
        caustics: Some(CausticOptions { photons: 4, radius_mm: 1.0, ..Default::default() }),
        ..Default::default()
    };
    let mut task = Task::new(context.clone(), scene, request(5), options, CancelToken::default()).unwrap();
    assert_eq!(task.photons.as_ref().unwrap().progress().done, 0, "selected optical pipeline warm-up cannot advance photon work");
    task.sample = 1;
    assert!(task.read_linear().unwrap().iter().all(|pixel| *pixel == [0.0; 4]), "actual-binding optical warm-up must leave film untouched");
    task.sample = 0;
    let resources = task.photons.as_ref().unwrap();
    let source = format!(
        "{}\n@compute @workgroup_size(1) fn optical_density_reference() {{
        if u.sampling.z==0u {{return;}}
        let hit=trace_precise(vec3<f32>(0.1,0.0,2.0),vec3<f32>(0.0),vec3<f32>(0.0,0.0,-1.0),false,0xffffffffu,0.0,false);
        var t=triangle(hit.triangle);t.glass=vec4<f32>(0.5,0.0,0.0,0.0);
        let medium=Medium(t.optics.w,1.5,vec3<f32>(0.2,0.4,0.8),vec2<f32>(0.6,0.3));
        accumulation[0]=vec4<f32>(photon_gather_glass(t,vec3<f32>(0.0,0.0,0.5),vec3<f32>(0.0,0.0,1.0),vec3<f32>(0.0,0.0,1.0),vec3<f32>(0.0,0.0,-1.0),1.0,1.5,0.0,0),1.0);
        accumulation[1]=vec4<f32>(photon_gather_volume(vec3<f32>(0.0),vec3<f32>(0.0,0.0,-1.0),medium),1.0);
        var sum=vec3<f32>(0.0);
        for(var trial=0u;trial<4096u;trial+=1u) {{
            var reservoir:PhotonReservoir;reservoir.rng=trial^0x5b3d17u;
            photon_offer(&reservoir,PrecisePoint(vec3<f32>(0.0),vec3<f32>(0.0)),vec3<f32>(0.0,0.0,1.0),vec3<f32>(1.0,0.0,0.0),vec3<f32>(0.0),0u,1u,0);
            photon_offer(&reservoir,PrecisePoint(vec3<f32>(0.0),vec3<f32>(0.0)),vec3<f32>(0.0,0.0,1.0),vec3<f32>(0.0,1.0,0.0),vec3<f32>(0.0),0u,2u,0);
            photon_offer(&reservoir,PrecisePoint(vec3<f32>(0.0),vec3<f32>(0.0)),vec3<f32>(0.0,0.0,1.0),vec3<f32>(0.0,0.0,1.0),vec3<f32>(0.0),0u,3u,0);
            sum+=reservoir.flux*f32(reservoir.visits);
        }}
        accumulation[3]=vec4<f32>(sum/4096.0,1.0);
        var narrow=t;narrow.glass.x=0.001;
        accumulation[4]=vec4<f32>(glass_evaluate_cosine(narrow.glass,vec3<f32>(0.0,0.0,-1.0),unit(vec3<f32>(0.0002,0.0,1.0)),vec3<f32>(0.0,0.0,1.0),vec3<f32>(0.0,0.0,1.0),1.0,1.5,0.0),1.0);
        for(var case_index=0u;case_index<18u;case_index+=1u) {{
            let vertices=max(1u,case_index-min(case_index,1u));
            let trials=select(4096u,1u,case_index<3u);var total=vec3<f32>(0.0);
            for(var trial=0u;trial<trials;trial+=1u) {{
                var pending:array<vec4<u32>,6>;var pending_count=0u;var pending_rng=trial^(vertices*0x45d9f3bu)^0x6ef372d1u;
                if case_index==0u {{glass_offer_light(&pending,&pending_count,&pending_rng,PrecisePoint(vec3<f32>(0.0),vec3<f32>(0.0)),vec3<f32>(0.0,0.0,-1.0),vec3<f32>(1.0),1.5,1.5,0xffffffffu,1u,medium);}}
                else {{
                    for(var vertex=0u;vertex<vertices;vertex+=1u) {{
                        let value=f32(vertex+1u);var prefix=vec3<f32>(value,value*value,select(0.0,1.0,(vertex&1u)==0u));
                        if case_index==1u {{prefix=vec3<f32>(1.0);}}
                        glass_offer_light(&pending,&pending_count,&pending_rng,PrecisePoint(vec3<f32>(0.0,0.0,0.5),vec3<f32>(0.0)),vec3<f32>(0.0,0.0,-1.0),prefix,1.0,1.5,hit.triangle,0u,glass_vacuum());
                    }}
                }}
                total+=glass_resolve_light(&pending,pending_count,0.0);
            }}
            var output_index=select(2u,5u,case_index==1u);if case_index>=2u {{output_index=case_index+4u;}}
            accumulation[output_index]=vec4<f32>(total/f32(trials),1.0);
        }}
    }}
    @compute @workgroup_size(1) fn optical_event_limit_reference() {{
        if u.sampling.z==0u {{return;}}
        var pending:array<vec4<u32>,6>;var pending_count=u.sampling.y;var pending_rng=0u;
        glass_offer_light(&pending,&pending_count,&pending_rng,PrecisePoint(vec3<f32>(0.0),vec3<f32>(0.0)),vec3<f32>(0.0,0.0,-1.0),vec3<f32>(1.0),1.0,1.5,0u,0u,glass_vacuum());
    }}
    @compute @workgroup_size(1) fn optical_precision_reference() {{
        if u.sampling.z==0u {{return;}}
        var response=vec3<f32>(0.0);
        if u.image.z==0u {{
            response=glass_evaluate_cosine(vec4<f32>(1e-9,0.0,0.0,0.0),vec3<f32>(0.0,0.0,-1.0),vec3<f32>(0.0,0.0,-1.0),vec3<f32>(0.0,0.0,1.0),vec3<f32>(0.0,0.0,1.0),1.0,bitcast<f32>(0x3f800001u),0.0);
        }} else {{response=vec3<f32>(glass_response_product(1e30,1e30));}}
        accumulation[0]=vec4<f32>(response,1.0);
    }}",
        photons::shader_source().unwrap()
    );
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(source.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&resources.layout)],
        immediate_size: 0,
    });
    let constants = [("GLASS_DELTA_LIGHTS", 1.0)];
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("optical_density_reference"),
        compilation_options: wgpu::PipelineCompilationOptions { constants: &constants, ..Default::default() },
        cache: None,
    });
    warm_up(&context.device, &context.queue, &pipeline, &task.bind, &context.healthy).unwrap();
    // Every triangle belongs to the same closed body. Use triangle0 only as a
    // body identifier; the receiver's actual outward normal is explicitly +Z.
    let heads = 8_usize;
    let mut words = vec![0_u32; 16 + heads + 4 * 16];
    words[0] = 4;
    words[1] = heads as u32;
    words[2] = 1.0_f32.to_bits();
    words[6..9].fill((-10.0_f32).to_bits());
    words[16..16 + heads].fill(u32::MAX);
    let pi = std::f32::consts::PI;
    for (index, (position, incoming, flux, kind, receiver, side)) in [
        ([0.0_f32, 0.0, 0.59], [0.0, 0.0, 1.0], [pi, 2.0 * pi, 3.0 * pi], 2, 0, 0),
        ([0.0, 0.0, -0.1], [0.0, 0.0, -1.0], [pi, 2.0 * pi, 3.0 * pi], 3, 1.0_f32.to_bits(), 0),
        ([0.0, 0.0, 0.0], [0.0, 0.0, -1.0], [1e6; 3], 3, 2.0_f32.to_bits(), 0),
        ([0.0, 0.0, 0.5], [0.0, 0.0, 1.0], [1e6; 3], 2, 0, 1),
    ]
    .into_iter()
    .enumerate()
    {
        let cell = position.map(|value| (value + 10.0).floor() as u32);
        let hash = (cell[0].wrapping_mul(73856093) ^ cell[1].wrapping_mul(19349663) ^ cell[2].wrapping_mul(83492791)) as usize & (heads - 1);
        let record = 16 + heads + index * 16;
        for (at, vector) in [(0, position), (4, incoming), (8, flux), (12, [0.0, 0.0, 1.0])] {
            for (axis, value) in vector.into_iter().enumerate() {
                words[record + at + axis] = value.to_bits();
            }
        }
        words[record + 3] = words[16 + hash];
        words[16 + hash] = index as u32;
        words[record + 7] = receiver;
        words[record + 11] = kind;
        words[record + 15] = side | 4; // Punctual source0, plus the virtual-side bits.
    }
    let bytes: Vec<_> = words.into_iter().flat_map(u32::to_le_bytes).collect();
    context.queue.write_buffer(&resources.buffer, 0, &bytes);
    context.queue.write_buffer(&task.uniform, 0, &task.parameters(1).unwrap());
    let mut encoder = context.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &task.bind, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    context.queue.submit([encoder.finish()]);
    task.sample = 1;
    let actual = task.read_linear().unwrap();
    let reflection = 0.04 / (4.0 * std::f64::consts::PI * 0.5_f64.powi(4));
    let transmitted = 0.96 / (0.25 * std::f64::consts::PI * 0.5_f64.powi(4));
    let phase = |cosine: f64| 0.91 / (4.0 * std::f64::consts::PI * (1.09 - 0.6 * cosine).powf(1.5));
    let outgoing = Vec3::new(0.0002, 0.0, 1.0).normalized().unwrap();
    let narrow = solidcraft_render::glass_model::GlassModel { roughness: 0.001, ..Default::default() }
        .evaluate(-Vec3::Z, outgoing, Vec3::Z, 1.0, 1.5, 550.0)
        .unwrap()
        .value
        * outgoing.z;
    for (channel, absorption) in [0.2, 0.4, 0.8].into_iter().enumerate() {
        let scalar = (channel + 1) as f64;
        for (pixel, expected) in
            [reflection * scalar, phase(1.0) * 1.25 * scalar, phase(-1.0) * 16.0 * (-0.25_f64 * (absorption + 0.6)).exp()].into_iter().enumerate()
        {
            assert!(
                (f64::from(actual[pixel][channel]) - expected).abs() < expected * 0.0002,
                "kernel{pixel}/channel{channel}: {actual:?} != {expected}"
            );
        }
        assert!((actual[3][channel] - 1.0).abs() < 0.06, "one-record reservoir must preserve every eligible visit in expectation: {actual:?}");
        assert!(
            (f64::from(actual[4][channel]) - narrow).abs() < narrow * 0.0003,
            "representable facet sine must survive an f32 rounded-one cosine: {actual:?} != {narrow}"
        );
        let seam = transmitted * 16.0 * (-0.25_f64 * (absorption + 0.6)).exp();
        assert!(
            (f64::from(actual[5][channel]) - seam).abs() < seam * 0.0003,
            "an exact coplanar sibling at the producing seam cannot block the connection: {actual:?} != {seam}"
        );
    }
    for vertices in 1_usize..=16 {
        let count = vertices as f64;
        let sum = [count * (count + 1.0) / 2.0, count * (count + 1.0) * (2.0 * count + 1.0) / 6.0, vertices.div_ceil(2) as f64];
        let observed = actual[5 + vertices];
        for (channel, total) in sum.into_iter().enumerate() {
            let expected = f64::from(actual[5][channel]) * total;
            assert!(
                (f64::from(observed[channel]) - expected).abs() < expected * 0.06,
                "one camera light request must estimate the complete K={vertices} colored vertex sum: {observed:?} vs {expected}"
            );
        }
    }
    assert_eq!(actual[6], actual[5], "K=1 retains the exact deterministic direct contribution");
    // A finite-radius packet outside a source's omission sphere must not leak
    // into a receiver inside that sphere. Source metadata remains one packed word.
    let minimum_offset = PARAMETER_BYTES - PUNCTUAL_PARAMETER_BYTES as usize - 32 + 28;
    context.queue.write_buffer(&task.uniform, minimum_offset as u64, &0.3_f32.to_le_bytes());
    let mut encoder = context.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &task.bind, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    context.queue.submit([encoder.finish()]);
    let omitted = task.read_linear().unwrap();
    assert!(omitted[..3].iter().all(|pixel| *pixel == [0.0, 0.0, 0.0, 1.0]), "receiver-side point-source minimum is physical: {omitted:?}");
    let overflow = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("optical_event_limit_reference"),
        compilation_options: wgpu::PipelineCompilationOptions { constants: &constants, ..Default::default() },
        cache: None,
    });
    context.queue.write_buffer(&task.uniform, 0, &[0; PARAMETER_BYTES]);
    warm_up(&context.device, &context.queue, &overflow, &task.bind, &context.healthy).unwrap();
    context.queue.write_buffer(&task.uniform, 0, &task.parameters(1).unwrap());
    let mut encoder = context.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&overflow);
        pass.set_bind_group(0, &task.bind, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    context.queue.submit([encoder.finish()]);
    let failure = task.read_linear().unwrap_err();
    assert_eq!(failure.code, ErrorCode::InvalidScene);
    assert!(failure.message.contains("light-event"));
    let precision = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("optical_precision_reference"),
        compilation_options: wgpu::PipelineCompilationOptions { constants: &constants, ..Default::default() },
        cache: None,
    });
    context.queue.write_buffer(&task.uniform, 0, &[0; PARAMETER_BYTES]);
    warm_up(&context.device, &context.queue, &precision, &task.bind, &context.healthy).unwrap();
    for case in 0_u32..2 {
        context.queue.write_buffer(&task.failures, 0, &[0; FAILURE_BYTES as usize]);
        let mut parameters = task.parameters(1).unwrap();
        parameters[8..12].copy_from_slice(&case.to_le_bytes());
        context.queue.write_buffer(&task.uniform, 0, &parameters);
        let mut encoder = context.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&precision);
            pass.set_bind_group(0, &task.bind, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        context.queue.submit([encoder.finish()]);
        let failure = task.read_linear().unwrap_err();
        assert_eq!(failure.code, ErrorCode::InvalidScene);
        assert!(
            failure.message.contains("optical response"),
            "near-matched narrow BTDF and response times valid source must refuse arithmetic overflow, case{case}: {failure}"
        );
    }
}

#[test]
fn photon_density_kernel_counts_hash_collisions_once_and_respects_receiver_support() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    if context.device.limits().max_storage_buffers_per_shader_stage < 7 {
        return;
    }
    let mut scene = glass_slab(true);
    let material = OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 0.0, 0.5, 1.0).unwrap();
    scene.meshes[0].transmission = None;
    scene.meshes[0].surface = Some(material);
    let mut other = scene.meshes[0].clone();
    other.name = "other density receiver".into();
    for p in &mut Arc::make_mut(&mut other.mesh).positions {
        p.z += 0.02;
    }
    scene.meshes.push(other);
    scene.bounds = scene.bounds.union(&scene.meshes[1].mesh.bounds());
    let upload = scene::flatten(&scene, Vec3::ZERO, &CancelToken::default()).unwrap();
    let word = |index: usize| u32::from_le_bytes(upload.geometry[index * 4..index * 4 + 4].try_into().unwrap());
    let material_base = (word(2) & scene::MATERIAL_OFFSET_MASK) as usize;
    let receiver = |body: f32| {
        (0..word(0) as usize).find(|i| f32::from_bits(word(material_base + word(4 + i * 4 + 3) as usize * 20 + 11)) == body).unwrap() as u32
    };
    let first = receiver(1.0);
    let second = receiver(2.0);
    let options = JobOptions {
        caustics: Some(solidcraft_render::job::CausticOptions { photons: 4, radius_mm: 1.0, ..Default::default() }),
        ..Default::default()
    };
    let mut task = Task::new(context.clone(), scene, request(2), options, CancelToken::default()).unwrap();
    assert_eq!(task.photons.as_ref().unwrap().progress().done, 0, "setup warm-up cannot consume admitted photon work");
    task.sample = 1;
    assert!(task.read_linear().unwrap().iter().all(|pixel| *pixel == [0.0; 4]), "actual-binding zero-work warm-up cannot write camera film");
    task.sample = 0;
    let resources = task.photons.as_ref().unwrap();
    let cancelled = CancelToken::default();
    cancelled.cancel();
    assert_eq!(resources.warm_up_actual(context, &task.uniform, &task.bind, &cancelled).unwrap_err().code, ErrorCode::Cancelled);
    let source = format!(
        "{}\n@compute @workgroup_size(1) fn density_reference() {{
        let t=triangle(u32(u.eye.w));
        accumulation[0]=vec4<f32>(photon_gather(t,u.eye.xyz,vec3<f32>(0.0,0.0,1.0),vec3<f32>(0.0,0.0,1.0),vec3<f32>(0.0,0.0,1.0),64u),1.0);
    }}",
        photons::shader_source().unwrap()
    );
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(source.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&resources.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("density_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    let heads = 8_usize;
    let mut words = vec![0_u32; 16 + heads + 4 * 16];
    words[0] = 4;
    words[1] = heads as u32;
    words[2] = 1.0_f32.to_bits();
    words[6] = (-10.0_f32).to_bits();
    words[7] = (-10.0_f32).to_bits();
    words[8] = 0.5_f32.to_bits();
    words[16..16 + heads].fill(u32::MAX);
    let pi = std::f32::consts::PI;
    let fixtures = [
        ([0.0_f32, 0.0, 0.5], [pi, 2.0 * pi, 3.0 * pi], [0.0, 0.0, 1.0], first),
        ([0.1, 0.0, 0.52], [1000.0; 3], [0.0, 0.0, 1.0], second), // Different body.
        ([0.1, 0.1, 0.7], [1000.0; 3], [0.0, 0.0, 1.0], first),   // Outside plane band.
        ([0.2, 0.0, 0.5], [1000.0; 3], [0.0, 0.0, -1.0], first),  // Opposite normal.
    ];
    for (index, (point, flux, normal, body)) in fixtures.into_iter().enumerate() {
        let cell = [((point[0] + 10.0).floor()) as i32, ((point[1] + 10.0).floor()) as i32, (point[2] - 0.5).floor() as i32];
        let hash = ((cell[0] as u32).wrapping_mul(73856093) ^ (cell[1] as u32).wrapping_mul(19349663) ^ (cell[2] as u32).wrapping_mul(83492791))
            as usize
            & (heads - 1);
        let record = 16 + heads + index * 16;
        for (at, vector) in [(0, point), (4, [0.0, 0.0, 1.0]), (8, flux), (12, normal)] {
            for (axis, value) in vector.into_iter().enumerate() {
                words[record + at + axis] = value.to_bits();
            }
        }
        words[record + 3] = words[16 + hash];
        words[16 + hash] = index as u32;
        words[record + 7] = body;
        words[record + 11] = 1;
    }
    let bytes: Vec<_> = words.into_iter().flat_map(u32::to_le_bytes).collect();
    context.queue.write_buffer(&resources.buffer, 0, &bytes);
    let mut params = vec![0; 32];
    scene::vector(&mut params, Vec3::new(0.0, 0.0, 0.5), first as f32);
    params.resize(PARAMETER_BYTES, 0);
    context.queue.write_buffer(&task.uniform, 0, &params);
    let mut encoder = context.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &task.bind, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    context.queue.submit([encoder.finish()]);
    task.sample = 1;
    let actual = task.read_linear().unwrap()[0];
    // The one eligible packet has flux [pi,2pi,3pi], Lambert f=1/pi,
    // and the disk kernel is1/pi. Hash collisions must not multiply it.
    for (actual, expected) in actual.into_iter().zip([1.0 / pi, 2.0 / pi, 3.0 / pi, 1.0]) {
        assert!((actual - expected).abs() < 2e-5, "density kernel {actual} != {expected}");
    }
}

fn photon_receiver_fixture(scattering: f64) -> ProductScene {
    let mut source = glass_slab(false);
    source.meshes[0].surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.0).unwrap());
    let optics = clear_glass();
    source.meshes[0].transmission = Some(if scattering > 0.0 {
        optics.with_subsurface(solidcraft_render::transmission::subsurface::Subsurface { scattering_per_mm: scattering, anisotropy: 0.0 }).unwrap()
    } else {
        optics
    });
    let floor = Mesh {
        positions: vec![Vec3::new(-8.0, -8.0, -1.0), Vec3::new(8.0, -8.0, -1.0), Vec3::new(8.0, 8.0, -1.0), Vec3::new(-8.0, 8.0, -1.0)],
        normals: vec![Vec3::Z; 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        tri_face: vec![0, 0],
        ..Default::default()
    };
    source.bounds = source.bounds.union(&floor.bounds());
    source.meshes.push(ProductMesh {
        name: "photon Lambert receiver".into(),
        mesh: Arc::new(floor),
        color: [255; 3],
        opacity: 1.0,
        surface: Some(OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 0.0, 0.5, 1.0).unwrap()),
        transmission: None,
        faces: Vec::new(),
    });
    source
}

fn photon_map_probe(task: &mut Task, position: Vec3) -> [f32; 4] {
    let maximum = task.photons.as_ref().unwrap().maximum_quanta();
    for _ in 0..maximum {
        if task.photons.as_ref().unwrap().ready() {
            break;
        }
        task.dispatch().unwrap();
    }
    assert!(task.photons.as_ref().unwrap().ready(), "bounded photon prepass must finish");
    let context = &task.context;
    let resources = task.photons.as_ref().unwrap();
    let source=format!("{}\n@compute @workgroup_size(1) fn photon_map_reference() {{
        var receiver=triangle(0u);for(var i=0u;i<geometry[0];i+=1u) {{let candidate=triangle(i);if candidate.optics.w==2.0 {{receiver=candidate;break;}}}}
        accumulation[0]=vec4<f32>(photon_gather(receiver,u.eye.xyz,vec3<f32>(0.0,0.0,1.0),vec3<f32>(0.0,0.0,1.0),vec3<f32>(0.0,0.0,1.0),64u),1.0);
    }}",photons::shader_source().unwrap());
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(source.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&resources.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("photon_map_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    // Receiver-side source-minimum clipping needs the actual authored sources,
    // even though this numerical entry replaces only the probe's eye position.
    let mut params = task.parameters(1).unwrap();
    let mut eye = Vec::new();
    scene::vector(&mut eye, position, 0.0);
    params[32..48].copy_from_slice(&eye);
    context.queue.write_buffer(&task.uniform, 0, &params);
    let mut encoder = context.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &task.bind, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    context.queue.submit([encoder.finish()]);
    task.sample = 1;
    task.read_linear().unwrap()[0]
}

#[test]
fn delta_source_photon_power_matches_directional_and_point_disk_integrals() {
    use solidcraft_render::job::{
        CausticOptions, DirectionalLightOptions, DirectionalLights, PunctualLightOptions, PunctualLights, SpotConeOptions, StudioLighting,
    };
    let Some(executor) = executor() else { return };
    if executor.context.device.limits().max_storage_buffers_per_shader_stage < 7 {
        return;
    }
    let point = PunctualLightOptions {
        position_mm: [0.0, 0.0, 4.0],
        color: [1.0; 3],
        intensity: 25.0,
        reference_distance_mm: 1.0,
        minimum_distance_mm: 0.001,
        cone: None,
    };
    let directional = StudioLighting {
        sun: None,
        environment: [0.0; 3],
        directional_lights: DirectionalLights::new(&[DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 1.0 }])
            .unwrap(),
        ..Default::default()
    };
    let spot = PunctualLightOptions {
        cone: Some(SpotConeOptions {
            emitted_axis: [0.0, 0.0, -1.0],
            inner_half_angle_radians: 40_f64.to_radians(),
            outer_half_angle_radians: 60_f64.to_radians(),
        }),
        ..point
    };
    let radius = 3.0_f64;
    let point_integral = 2.0 * 25.0 / (radius * radius) * (1.0 - 5.0 / (25.0 + radius * radius).sqrt()) / std::f64::consts::PI;
    for (lighting, expected) in [
        (directional, 1.0 / std::f64::consts::PI),
        (
            StudioLighting {
                directional_lights: DirectionalLights::new(&[0.1, 0.9].map(|intensity| DirectionalLightOptions {
                    direction: [0.0, 0.0, 1.0],
                    color: [1.0; 3],
                    intensity,
                }))
                .unwrap(),
                ..directional
            },
            1.0 / std::f64::consts::PI,
        ),
        (
            StudioLighting { sun: None, environment: [0.0; 3], punctual_lights: PunctualLights::new(&[point]).unwrap(), ..Default::default() },
            point_integral,
        ),
        (
            StudioLighting { sun: None, environment: [0.0; 3], punctual_lights: PunctualLights::new(&[spot]).unwrap(), ..Default::default() },
            point_integral,
        ),
    ] {
        for power_weighted_sources in [false, true] {
            let options = JobOptions {
                lighting: Some(lighting),
                caustics: Some(CausticOptions {
                    photons: 16384,
                    radius_mm: radius,
                    max_depth: 4,
                    seed: 81,
                    area_lights: false,
                    environment: false,
                    guided_emission: false,
                    refresh_per_pass: false,
                    power_weighted_sources,
                }),
                ..Default::default()
            };
            let mut task = Task::new(executor.context.clone(), photon_receiver_fixture(0.0), request(1), options, CancelToken::default()).unwrap();
            let actual = photon_map_probe(&mut task, Vec3::new(0.0, 0.0, -1.0));
            for value in actual.into_iter().take(3) {
                assert!((f64::from(value) - expected).abs() < expected * 0.12, "delta source photon power {actual:?} != disk integral {expected}");
            }
        }
    }
}

#[test]
fn photon_refresh_keeps_pass_zero_reuses_storage_and_selects_distinct_complete_maps() {
    use solidcraft_render::job::{CausticOptions, DirectionalLightOptions, DirectionalLights, StudioLighting, adaptive::AdaptiveSampling};
    let Some(executor) = executor() else { return };
    if executor.context.device.limits().max_storage_buffers_per_shader_stage < 7 {
        return;
    }
    for power_weighted_sources in [false, true] {
        let scene = photon_receiver_fixture(0.0);
        let mut image = request(1);
        image.camera.target = -Vec3::Z;
        image.camera.distance = 0.1;
        let make = |samples, photons, refresh_per_pass, adaptive| {
            let options = JobOptions {
                samples,
                max_depth: 1,
                adaptive,
                transmission_samples: 2,
                lighting: Some(StudioLighting {
                    directional_lights: DirectionalLights::new(&[0.1, 0.9].map(|intensity| DirectionalLightOptions {
                        direction: [0.0, 0.0, 1.0],
                        color: [1.0; 3],
                        intensity,
                    }))
                    .unwrap(),
                    ..Default::default()
                }),
                caustics: Some(CausticOptions {
                    photons,
                    radius_mm: 3.0,
                    max_depth: 4,
                    refresh_per_pass,
                    power_weighted_sources,
                    guided_emission: power_weighted_sources,
                    ..Default::default()
                }),
                ..Default::default()
            };
            let admission = admission(&scene, &image, options).unwrap();
            let task = Task::new(executor.context.clone(), scene.clone(), image.clone(), options, CancelToken::default()).unwrap();
            (task, admission)
        };
        let run = |refresh| {
            let (mut task, admission) = make(3, 8192, refresh, None);
            let buffer = task.photons.as_ref().unwrap().buffer.clone();
            let mut maps = Vec::new();
            let mut films = Vec::new();
            for pass in 0..3 {
                task.complete_sample().unwrap();
                let resources = task.photons.as_ref().unwrap();
                assert_eq!(resources.buffer, buffer);
                assert_eq!(resources.test_epoch(), if refresh { pass } else { 0 });
                maps.push(resources.test_records(&task.context));
                films.push(task.read_linear().unwrap()[0]);
            }
            assert_eq!(admission.caustic_work.map(|work| work.emitted_paths), refresh.then_some(24_576));
            (maps, films, admission.preparation_bytes)
        };
        let ordinary = run(false);
        let refreshed = run(true);
        assert_eq!(ordinary.0[0], refreshed.0[0], "pass zero keeps every indexed photon bit");
        assert_eq!(ordinary.0[0], ordinary.0[1]);
        assert_ne!(refreshed.0[0], refreshed.0[1]);
        assert_ne!(refreshed.0[1], refreshed.0[2]);
        assert_eq!(refreshed.0, run(true).0, "the independent epoch keeps deterministic photon records");
        assert_eq!(ordinary.2, refreshed.2);
        let expected = 1.0 / std::f32::consts::PI;
        assert!((ordinary.1[0][0] - refreshed.1[0][0]).abs() < 1e-5);
        for value in refreshed.1 {
            assert!(value[..3].iter().all(|channel| (channel - expected).abs() < expected * 0.16), "caustic film {value:?}");
            assert_eq!(value[3], 1.0);
        }

        let (mut previewed, _) = make(2, 512, true, None);
        previewed.preview_enabled = true;
        let mut preview = None;
        for _ in 0..2048 {
            assert!(matches!(previewed.step(1).unwrap(), TaskPoll::Pending(_)));
            preview = previewed.take_preview();
            if preview.is_some() {
                break;
            }
        }
        assert_eq!(preview.unwrap().samples_per_pixel, 1);
        assert_eq!(previewed.photons.as_ref().unwrap().test_epoch(), 0);
        assert!(matches!(previewed.step(1).unwrap(), TaskPoll::Pending(Progress { phase: Phase::Running, done: 1, total: 2 })));
        assert_eq!(previewed.photons.as_ref().unwrap().test_epoch(), 1);
        assert!(!previewed.photons.as_ref().unwrap().ready());
        previewed.cancel.cancel();
        assert_eq!(previewed.step(1).err().unwrap().code, ErrorCode::Cancelled);

        let adaptive = AdaptiveSampling { pilot_samples: 16, min_samples: 16, ..Default::default() };
        let (mut adaptive_task, admission) = make(16, 128, true, Some(adaptive));
        assert_eq!(admission.caustic_work.unwrap().maps, 32);
        for pass in 0..=16 {
            adaptive_task.complete_sample().unwrap();
            assert_eq!(adaptive_task.photons.as_ref().unwrap().test_epoch(), pass);
        }
        assert_eq!(adaptive_task.sample, 17, "the first production pass has a new absolute photon key");
    }
}

#[test]
fn guided_photons_preserve_parallel_point_spot_and_area_power_at_a_translated_aperture() {
    use solidcraft_render::job::{
        AreaLight, AreaLights, CausticOptions, DirectionalLightOptions, DirectionalLights, PunctualLightOptions, PunctualLights, SpotConeOptions,
        StudioLighting,
    };
    let Some(executor) = executor() else { return };
    if executor.context.device.limits().max_storage_buffers_per_shader_stage < 7 {
        return;
    }
    let translation = Vec3::new(10_000.25, -8_000.5, 0.0);
    let mut scene = photon_receiver_fixture(0.0);
    for (index, body) in scene.meshes.iter_mut().enumerate() {
        for point in &mut Arc::make_mut(&mut body.mesh).positions {
            if index == 0 {
                point.x *= 0.2;
                point.y *= 0.2;
            }
            *point += translation;
        }
    }
    scene.bounds = scene.meshes.iter().fold(Aabb3::EMPTY, |bounds, body| bounds.union(&body.mesh.bounds()));
    let directional = StudioLighting {
        directional_lights: DirectionalLights::new(&[DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 1.0 }])
            .unwrap(),
        ..Default::default()
    };
    let point = PunctualLightOptions {
        position_mm: [translation.x, translation.y, 8.0],
        color: [1.0; 3],
        intensity: 25.0,
        reference_distance_mm: 1.0,
        minimum_distance_mm: 0.001,
        cone: None,
    };
    let punctual =
        |cone| StudioLighting { punctual_lights: PunctualLights::new(&[PunctualLightOptions { cone, ..point }]).unwrap(), ..Default::default() };
    let spot = |axis| {
        punctual(Some(SpotConeOptions {
            emitted_axis: axis,
            inner_half_angle_radians: 15_f64.to_radians(),
            outer_half_angle_radians: 25_f64.to_radians(),
        }))
    };
    let rectangle = AreaLight { center: point.position_mm, u: [0.25, 0.0, 0.0], v: [0.0, -0.25, 0.0], radiance: [1.0; 3] };
    let area = |light| StudioLighting { area_lights: AreaLights::new(&[light]).unwrap(), ..Default::default() };
    let radius = 0.5_f64;
    let height = 9.0_f64;
    let pi = std::f64::consts::PI;
    let point_integral = 2.0 * point.intensity * (1.0 - height / height.hypot(radius)) / (pi * radius * radius);
    let rectangle_integral = 0.25 / (pi * height * height);
    let probe = |scene, lighting| {
        let options = JobOptions {
            lighting: Some(lighting),
            caustics: Some(CausticOptions {
                photons: 32_768,
                radius_mm: radius,
                max_depth: 4,
                area_lights: true,
                guided_emission: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut task = Task::new(executor.context.clone(), scene, request(1), options, CancelToken::default()).unwrap();
        f64::from(photon_map_probe(&mut task, translation - Vec3::Z)[0])
    };
    for (lighting, expected) in
        [(directional, 1.0 / pi), (punctual(None), point_integral), (spot([0.0, 0.0, -1.0]), point_integral), (area(rectangle), rectangle_integral)]
    {
        let actual = probe(scene.clone(), lighting);
        assert!((actual - expected).abs() < expected * 0.15, "guided power: {actual} vs {expected}");
    }
    let back = AreaLight { v: [0.0, 0.25, 0.0], ..rectangle };
    assert_eq!(probe(scene.clone(), area(back)), 0.0);
    assert_eq!(probe(scene.clone(), spot([1.0, 0.0, 0.0])), 0.0);
    let mut blocker = scene.meshes[1].clone();
    blocker.name = "Opaque blocker before optical launch bounds".into();
    for point in &mut Arc::make_mut(&mut blocker.mesh).positions {
        point.z = 6.0;
    }
    scene.bounds = scene.bounds.union(&blocker.mesh.bounds());
    scene.meshes.push(blocker);
    assert_eq!(probe(scene, directional), 0.0);
}

#[test]
fn area_photon_caustics_match_rectangle_radiometry_and_partition_camera_depth() {
    use solidcraft_render::job::{AreaLight, AreaLights, CausticOptions, StudioLighting};
    let Some(executor) = executor() else { return };
    if executor.context.device.limits().max_storage_buffers_per_shader_stage < 7 {
        return;
    }
    // The parallel index-matched slab leaves straight geometry but needs two
    // closed interfaces. The camera starts below it and sees the floor first.
    let scene = photon_receiver_fixture(0.0);
    let mut image = request(8);
    image.camera.target = Vec3::new(0.0, 0.0, -1.0);
    image.camera.distance = 0.1;
    let light = AreaLight { center: [0.0, 0.0, 1.5], u: [0.75, 0.0, 0.0], v: [0.0, -0.75, 0.0], radiance: [1.0; 3] };
    let render = |scene: ProductScene, light: AreaLight, area_lights: bool, light_depth, max_depth| {
        let options = JobOptions {
            samples: 256,
            max_depth,
            seed: 71,
            lighting: Some(StudioLighting { area_lights: AreaLights::new(&[light]).unwrap(), ..Default::default() }),
            caustics: Some(CausticOptions {
                photons: 32768,
                radius_mm: 0.5,
                max_depth: light_depth,
                seed: 81,
                area_lights,
                environment: false,
                guided_emission: false,
                refresh_per_pass: false,
                power_weighted_sources: false,
            }),
            ..Default::default()
        };
        let mut task = Task::new(executor.context.clone(), scene, image.clone(), options, CancelToken::default()).unwrap();
        for _ in 0..options.samples {
            task.complete_sample().unwrap();
        }
        let pixels = task.read_linear().unwrap();
        assert!(pixels.iter().all(|pixel| pixel[3] == 1.0));
        pixels.iter().map(|pixel| f64::from(pixel[0])).sum::<f64>() / pixels.len() as f64
    };
    // Lambert outgoing radiance at the floor center from a parallel rectangle:
    // integral of H² / (pi * (H² + x² + y²)²) over its area. The 0.08 mm
    // camera crop and 0.5 mm photon kernel change this reference by under 6%.
    let n = 128;
    let step = 1.5 / f64::from(n);
    let mut expected = 0.0;
    for i in 0..n {
        for j in 0..n {
            let x = -0.75 + (f64::from(i) + 0.5) * step;
            let y = -0.75 + (f64::from(j) + 0.5) * step;
            expected += step * step * 6.25 / (std::f64::consts::PI * (6.25 + x * x + y * y).powi(2));
        }
    }
    let camera = render(scene.clone(), light, false, 3, 4);
    let photons = render(scene.clone(), light, true, 3, 4);
    eprintln!("area caustic reference={expected}, camera={camera}, photons={photons}");
    for actual in [camera, photons] {
        assert!((actual - expected).abs() < expected * 0.13, "rectangle radiance {actual} vs {expected}");
    }
    assert_eq!(render(scene.clone(), light, true, 2, 4), camera, "short light budget must retain the camera endpoint");
    assert_eq!(render(scene.clone(), light, true, 3, 3), 0.0, "receiver needs a camera slot beyond both glass events");
    let back = AreaLight { v: [0.0, 0.75, 0.0], ..light };
    assert_eq!(render(scene.clone(), back, true, 3, 4), 0.0, "rectangle emits on one side only");

    let mut direct = scene;
    direct.meshes.remove(0);
    direct.bounds = direct.meshes[0].mesh.bounds();
    let direct_camera = render(direct.clone(), light, false, 3, 4);
    assert!((direct_camera - expected).abs() < expected * 0.13);
    assert_eq!(render(direct, light, true, 3, 4), direct_camera, "unoccluded area paths belong to the camera estimator");
}

#[test]
fn environment_photon_caustics_preserve_sun_and_analytic_rectangle_visibility() {
    use solidcraft_render::job::{AreaLight, AreaLights, CausticOptions, StudioLighting, Sun};
    let Some(executor) = executor() else { return };
    if executor.context.device.limits().max_storage_buffers_per_shader_stage < 7 {
        return;
    }
    let mut scene = photon_receiver_fixture(0.0);
    scene.bounds = Aabb3::EMPTY;
    for body in &mut scene.meshes {
        let mesh = Arc::make_mut(&mut body.mesh);
        for point in &mut mesh.positions {
            point.x *= 0.25;
            point.y *= 0.25;
        }
        scene.bounds = scene.bounds.union(&mesh.bounds());
    }
    let mut image = request(8);
    image.camera.target = Vec3::new(0.0, 0.0, -1.0);
    image.camera.distance = 0.1;
    let sun = Sun { direction: [0.0, 0.0, 1.0], angular_radius_rad: 0.45, radiance: [0.0, 1.0, 0.0] };
    let lighting = StudioLighting { environment: [1.0, 0.0, 0.0], sun: Some(sun), ..Default::default() };
    let render_mode = |lighting, environment, light_depth, max_depth, guided_emission| {
        let options = JobOptions {
            samples: 256,
            max_depth,
            seed: 71,
            lighting: Some(lighting),
            caustics: Some(CausticOptions {
                photons: 65536,
                radius_mm: 0.65,
                max_depth: light_depth,
                seed: 81,
                environment,
                guided_emission,
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut task = Task::new(executor.context.clone(), scene.clone(), image.clone(), options, CancelToken::default()).unwrap();
        for _ in 0..options.samples {
            task.complete_sample().unwrap();
        }
        let pixels = task.read_linear().unwrap();
        let mut sum = [0.0; 3];
        for pixel in &pixels {
            assert_eq!(pixel[3], 1.0);
            for (total, channel) in sum.iter_mut().zip(pixel) {
                *total += f64::from(*channel);
            }
        }
        sum.map(|channel| channel / pixels.len() as f64)
    };
    let render = |lighting, environment, light_depth, max_depth| render_mode(lighting, environment, light_depth, max_depth, false);
    let camera = render(lighting, false, 3, 4);
    let photons = render(lighting, true, 3, 4);
    let guided = render_mode(lighting, true, 3, 4, true);
    eprintln!("environment caustic camera={camera:?}, photons={photons:?}");
    // A matched slab leaves a white Lambert receiver under a unit sky at one.
    // The green zenith cap integrates independently to sin(radius)^2.
    for red in [camera[0], photons[0], guided[0]] {
        assert!((red - 1.0).abs() < 0.15, "uniform environment radiance {red} vs 1");
    }
    let expected_sun = sun.angular_radius_rad.sin().powi(2);
    assert!((camera[1] - expected_sun).abs() < 0.02, "finite sun {camera:?} vs {expected_sun}");
    assert_eq!(photons[1], camera[1], "the environment partition cannot take the sun term");
    assert_eq!(guided[1], camera[1], "guided environment photons also leave the finite sun term intact");
    assert_eq!(render(lighting, true, 2, 4), camera, "a short light path cannot replace the full camera tail");
    assert_eq!(render(lighting, true, 3, 3), render(lighting, false, 3, 3), "camera depth excludes the unobservable environment endpoint");

    // This black rectangle is behind the entire scene projection disk. Every
    // upward path through the slab hits it, including from its nonemitting side.
    let blocker = AreaLight { center: [0.0, 0.0, 8.0], u: [100.0, 0.0, 0.0], v: [0.0, 100.0, 0.0], radiance: [0.0; 3] };
    let blocked = StudioLighting { area_lights: AreaLights::new(&[blocker]).unwrap(), ..lighting };
    let blocked_camera = render(blocked, false, 3, 4);
    assert!(blocked_camera[0] < 0.04);
    assert_eq!(render(blocked, true, 3, 4), blocked_camera, "photons cannot bypass the sky-side rectangle outside the captured bounds");
    assert_eq!(render_mode(blocked, true, 3, 4, true), blocked_camera, "guided photons retain the same sky-side rectangle visibility");
}

#[test]
fn photon_subsurface_flight_precedes_a_closer_analytic_emitter() {
    use solidcraft_render::job::{AreaLight, AreaLights, CausticOptions, PunctualLightOptions, PunctualLights, SpotConeOptions, StudioLighting};
    let Some(executor) = executor() else { return };
    if executor.context.device.limits().max_storage_buffers_per_shader_stage < 7 {
        return;
    }
    let lighting = StudioLighting {
        sun: None,
        environment: [0.0; 3],
        punctual_lights: PunctualLights::new(&[PunctualLightOptions {
            position_mm: [0.0, 0.0, 2.0],
            color: [1.0; 3],
            intensity: 1.0,
            reference_distance_mm: 1.0,
            minimum_distance_mm: 0.001,
            cone: Some(SpotConeOptions {
                emitted_axis: [0.0, 0.0, -1.0],
                inner_half_angle_radians: 3_f64.to_radians(),
                outer_half_angle_radians: 6_f64.to_radians(),
            }),
        }])
        .unwrap(),
        area_lights: AreaLights::new(&[AreaLight { center: [0.0; 3], u: [0.25, 0.0, 0.0], v: [0.0, 0.25, 0.0], radiance: [0.0; 3] }]).unwrap(),
        ..Default::default()
    };
    let options = JobOptions {
        lighting: Some(lighting),
        caustics: Some(CausticOptions {
            photons: 16384,
            radius_mm: 0.3,
            max_depth: 16,
            seed: 81,
            area_lights: false,
            environment: false,
            guided_emission: false,
            refresh_per_pass: false,
            power_weighted_sources: false,
        }),
        ..Default::default()
    };
    let probe = |scattering| {
        let mut task = Task::new(executor.context.clone(), photon_receiver_fixture(scattering), request(1), options, CancelToken::default()).unwrap();
        photon_map_probe(&mut task, Vec3::new(1.0, 0.0, -1.0))
    };
    assert_eq!(probe(0.0), [0.0, 0.0, 0.0, 1.0], "every unscattered narrow-cone photon meets the analytic blocker");
    let scattered = probe(2.0);
    assert!(
        scattered[0] > 1e-5 && scattered.iter().all(|v| v.is_finite()),
        "a free flight before that blocker must retain the off-axis scattering path: {scattered:?}"
    );
}

#[test]
fn contained_punctual_connections_apply_physical_beer_and_hg_survival_without_an_ior_jump() {
    use solidcraft_render::job::{PunctualLightOptions, PunctualLights, StudioLighting};
    let Some(executor) = executor() else { return };
    let sigma = Vec3::new(0.2, 0.4, 0.0);
    for scattering in [0.0, 0.4] {
        let mut source = photon_receiver_fixture(scattering);
        source.meshes[0].surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap());
        let mut optics = solidcraft_render::transmission::Transmission::new(sigma, None).unwrap();
        if scattering > 0.0 {
            optics = optics
                .with_subsurface(solidcraft_render::transmission::subsurface::Subsurface { scattering_per_mm: scattering, anisotropy: 0.0 })
                .unwrap();
        }
        source.meshes[0].transmission = Some(optics);
        for p in &mut Arc::make_mut(&mut source.meshes[1].mesh).positions {
            p.z = -0.25;
        }
        source.bounds = source.meshes[0].mesh.bounds();
        let lighting = StudioLighting {
            sun: None,
            environment: [0.0; 3],
            punctual_lights: PunctualLights::new(&[PunctualLightOptions {
                position_mm: [0.5, 0.0, 0.0],
                color: [1.0; 3],
                intensity: 0.0625,
                reference_distance_mm: 1.0,
                minimum_distance_mm: 0.001,
                cone: None,
            }])
            .unwrap(),
            ..Default::default()
        };
        let samples = if scattering > 0.0 { 2048 } else { 1 };
        let options = JobOptions { nested_media: true, samples, max_depth: 1, lighting: Some(lighting), ..Default::default() };
        let mut image = request(1);
        image.camera.target = Vec3::new(0.0, 0.0, -0.25);
        image.camera.distance = 0.5;
        image.camera.fov = 0.00001;
        let mut task = Task::new(executor.context.clone(), source, image, options, CancelToken::default()).unwrap();
        while task.sample < samples {
            task.complete_sample().unwrap();
        }
        let actual = task.read_linear().unwrap()[0];
        // The source is0.5mm off the eye→receiver segment, avoiding an ideal
        // point singularity on the camera ray. All endpoints remain inside IOR1.5
        // and no eta conversion applies. An HG collision now has hard-source NEE
        // at maxdepth1, so include its independent one-dimensional expectation.
        // The zero-collision atom has mass exp(-sigma_s*0.5); collision distance
        // has density sigma_s*exp(-sigma_s*s), for0<s<0.5. Integrate both first
        // and second moments to set the Monte Carlo tolerance from real variance.
        for (value, absorption) in actual.into_iter().take(3).zip([sigma.x, sigma.y, sigma.z]) {
            let receiver_distance = (0.5_f64.powi(2) + 0.25_f64.powi(2)).sqrt();
            let endpoint = 0.0625 * 0.25 / (std::f64::consts::PI * receiver_distance.powi(3))
                * (-absorption * 0.5 - (absorption + scattering) * receiver_distance).exp();
            let mass = (-scattering * 0.5).exp();
            let mut first = mass * endpoint;
            let mut second = mass * endpoint * endpoint;
            let intervals = 1024_u32;
            let step = 0.5 / f64::from(intervals);
            for index in 0..=intervals {
                let distance = f64::from(index) * step;
                let light_distance = (0.5_f64.powi(2) + (0.25 - distance).powi(2)).sqrt();
                let weight = 0.0625 / (4.0 * std::f64::consts::PI * light_distance.powi(2))
                    * (-absorption * distance - (absorption + scattering) * light_distance).exp();
                let density = scattering * (-scattering * distance).exp();
                let quadrature = if index == 0 || index == intervals {
                    1.0
                } else if index.is_multiple_of(2) {
                    2.0
                } else {
                    4.0
                };
                first += step / 3.0 * quadrature * density * weight;
                second += step / 3.0 * quadrature * density * weight * weight;
            }
            let standard_error = ((second - first * first).max(0.0) / f64::from(samples)).sqrt();
            let tolerance = 6.0 * standard_error + 2e-5;
            assert!(
                (f64::from(value) - first).abs() < tolerance,
                "contained Beer/HG={scattering}: {actual:?} != {first}, standard error{standard_error}"
            );
        }
    }
}

#[test]
fn gpu_embedded_receiver_keeps_the_legacy_straight_shadow_scale() {
    use solidcraft_render::{
        glass_model::GlassModel,
        job::{PunctualLightOptions, PunctualLights, StudioLighting},
    };
    let Some(executor) = executor() else { return };
    for (model, ior_scale) in [(None, 1.0), (Some(GlassModel::default()), 1.0), (Some(GlassModel { roughness: 0.35, ..Default::default() }), 2.25)] {
        let mut source = photon_receiver_fixture(0.0);
        source.meshes[0].surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap());
        let mut optics = solidcraft_render::transmission::Transmission::new(Vec3::ZERO, None).unwrap().with_shadow_transmission(true);
        if let Some(model) = model {
            optics = optics.with_model(model).unwrap();
        }
        source.meshes[0].transmission = Some(optics);
        for p in &mut Arc::make_mut(&mut source.meshes[1].mesh).positions {
            p.z = -0.25;
        }
        source.bounds = source.meshes[0].mesh.bounds();
        let lighting = StudioLighting {
            punctual_lights: PunctualLights::new(&[PunctualLightOptions {
                position_mm: [0.0, 0.0, 0.75],
                color: [1.0; 3],
                intensity: 1.0,
                reference_distance_mm: 1.0,
                minimum_distance_mm: 0.001,
                cone: None,
            }])
            .unwrap(),
            ..Default::default()
        };
        let options = JobOptions { nested_media: true, samples: 1, max_depth: 1, lighting: Some(lighting), ..Default::default() };
        let mut image = request(1);
        image.camera.target = Vec3::new(0.0, 0.0, -0.25);
        image.camera.distance = 0.5;
        image.camera.fov = 0.00001;
        let mut task = Task::new(executor.context.clone(), source, image, options, CancelToken::default()).unwrap();
        task.complete_sample().unwrap();
        let actual = task.read_linear().unwrap()[0];
        assert_eq!(actual[3], 1.0);
        // The inside eye sees a unit Lambert receiver. The point is 1 mm away
        // along the normal, across one IOR-1.5 interface with power T = 0.96.
        let expected = 0.96 * ior_scale / std::f64::consts::PI;
        for value in &actual[..3] {
            assert!((f64::from(*value) - expected).abs() < 2e-5, "model={model:?}: {actual:?} vs {expected}");
        }
    }
}

#[test]
fn gpu_nonabsorbing_glass_sphere_white_furnace_conserves_radiance() {
    let Some(executor) = executor() else { return };
    let mut scene = sphere(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap());
    scene.meshes[0].transmission = Some(clear_glass());
    scene.meshes[0].opacity = 0.2;
    let options = JobOptions { samples: 512, max_depth: 16, seed: 1948, ..Default::default() };
    let mut task = Task::new(executor.context.clone(), scene, request(16), options, CancelToken::default()).unwrap();
    task.environment = 1.0;
    task.studio = 0.0;
    for _ in 0..options.samples {
        task.complete_sample().unwrap();
    }
    for (index, pixel) in task.read_linear().unwrap().iter().enumerate() {
        assert!((pixel[0] - 1.0).abs() < 0.025, "glass furnace pixel {index}: {pixel:?}");
    }
}

#[test]
fn gpu_ball_lens_focus_uses_actual_mesh_intersections() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("lens_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    let mut scene = sphere_grid(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap(), 128, 256);
    scene.meshes[0].transmission = Some(clear_glass());
    let mut task = Task::new(context.clone(), scene, request(1), JobOptions::default(), CancelToken::default()).unwrap();
    for height in [0.01, 0.02, 0.03] {
        let mut params = vec![0; 32];
        scene::vector(&mut params, Vec3::new(height, 0.0, 3.0), 0.0);
        scene::vector(&mut params, Vec3::ZERO, 0.0);
        scene::vector(&mut params, Vec3::ZERO, 0.0);
        scene::vector(&mut params, -Vec3::Z, 1.5); // nR/(2(n-1)), measured from centre.
        scene::vector(&mut params, Vec3::new(f64::from(task.epsilon), 0.0, 0.0), 0.0);
        context.queue.write_buffer(&task.uniform, 0, &params);
        let mut encoder = context.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &task.bind, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        context.queue.submit([encoder.finish()]);
        task.sample = 1;
        let actual = task.read_linear().unwrap()[0];
        assert_eq!(actual[3], 1.0);
        assert!((actual[0] - 1.0).abs() < 5e-5 && (actual[1] - 1.0).abs() < 5e-5, "image-space focal spot at h={height}: {actual:?}");
    }
}

#[test]
fn gpu_camera_inside_volume_fails_without_publishing_fake_radiance() {
    let Some(executor) = executor() else { return };
    let mut scene = sphere(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap());
    scene.meshes[0].transmission = Some(clear_glass());
    let mut request = request(1);
    request.camera.distance = 0.1;
    let mut task = Task::new(executor.context.clone(), scene.clone(), request.clone(), JobOptions::default(), CancelToken::default()).unwrap();
    task.complete_sample().unwrap();
    assert_eq!(task.read_linear().unwrap_err().code, ErrorCode::InvalidScene);
    drop(task);
    // Seed just below the 32-bit wrap: the next real failed path must carry
    // into the high word, rather than clearing diagnostics and publishing black.
    let mut task = Task::new(executor.context.clone(), scene, request, JobOptions::default(), CancelToken::default()).unwrap();
    let mut failures = [0; FAILURE_BYTES as usize];
    failures[..4].copy_from_slice(&u32::MAX.to_le_bytes());
    task.context.queue.write_buffer(&task.failures, 0, &failures);
    task.complete_sample().unwrap();
    let error = task.read_linear().unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidScene);
    assert!(error.message.contains("4294967296 of 1 paths"), "{error}");
}

#[test]
fn engine_preflight_keeps_cpu_alpha_and_routes_physical_glass_to_the_native_host() {
    use serde_json::json;
    let Some(executor) = executor() else { return };
    let mut s = solidcraft_engine::Session::default();
    s.set_render_job_host(Some(solidcraft_render::job::CpuHost::cooperative())).unwrap();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":2,"body_name":"Glass"})).unwrap();
    s.execute("appearance.assign", &json!({"bodies":["Glass"],"color":"#c0e0ff","opacity":0.2})).unwrap();
    let cpu = s.execute("render.preflight", &json!({"settings":{"width":16,"height":16}})).unwrap();
    assert_eq!(cpu["refraction"], false);
    assert_eq!(cpu["transparency"], "sorted_alpha_approximation");
    assert!(s.execute("render.preflight", &json!({"progressive":true,"settings":{"width":16,"height":16}})).is_err());
    s.register_render_job_host(ImageHost::with_executor(Arc::new(executor)).unwrap()).unwrap();
    let snapshot = s.doc.to_json();
    let mut params = json!({"progressive":true,"settings":{"width":16,"height":16},
        "raytrace":{"dielectrics":[{"body_index":0,"body_name":"Glass","ior":1.5,"absorption_per_mm":[0.1,0.02,0],"boundary":"closed_box","thickness_mm":0}]}});
    let gpu = s.execute("render.preflight", &params).unwrap();
    assert_eq!(gpu["refraction"], true);
    assert_eq!(gpu["dielectric_boundaries"], json!(["thin_sheet", "closed_box", "closed_mesh"]));
    assert_eq!(gpu["transparency"], "physical_transmission");
    assert_eq!(gpu["resolved"]["executor"], "gpu-path");
    params["settings"]["camera"] = json!(Camera { distance: 40.0, fov: 0.7, ..Default::default() });
    params["lens"] = json!({"aperture_radius_mm":0.5,"focus_distance_mm":40});
    let lens = s.execute("render.preflight", &params).unwrap();
    assert_eq!(lens["options"]["lens"]["aperture_radius_mm"], 0.5);
    assert_eq!(snapshot, s.doc.to_json());
    params["progressive"] = json!(false);
    assert!(s.execute("render.preflight", &params).is_err());
    assert!(!solidcraft_engine::product_scene::extract(&s).unwrap().has_transmission());
}

#[test]
fn gpu_thin_lens_focus_and_defocus_match_geometric_oracle() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("camera_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    for (yaw, pitch, roll, fov) in [(0.0, 0.0, 0.0, 0.7), (-0.45, 0.5, 0.7, 0.7), (-0.45, 0.5, 0.7, 2.0)] {
        let mut request = request(2);
        request.camera = Camera { distance: 12.0, fov, yaw, pitch, roll, ..Default::default() };
        let camera = request.camera;
        let lens = solidcraft_render::job::ThinLens { aperture_radius_mm: 0.4, focus_distance_mm: 9.0 };
        let options = JobOptions { lens: Some(lens), ..Default::default() };
        let mut task = Task::new(context.clone(), sphere(white()), request, options, CancelToken::default()).unwrap();
        let (right, up, back) = camera.basis();
        let eye = back * camera.distance;
        for (ux, uy) in [(0.0_f64, 0.0_f64), (0.7, -0.6)] {
            let focal =
                eye + (right * ux + up * uy) * (camera.half_height() * lens.focus_distance_mm / camera.distance) - back * lens.focus_distance_mm;
            for (radial, azimuth) in [(0.0_f64, 0.0_f64), (0.25, 0.125), (0.75, 0.625), (0.999, 0.999)] {
                let mut params = task.parameters(1).unwrap();
                params.truncate(96);
                scene::vector(&mut params, Vec3::new(ux, uy, radial), azimuth as f32);
                context.queue.write_buffer(&task.uniform, 0, &params);
                let mut encoder = context.device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&pipeline);
                    pass.set_bind_group(0, &task.bind, &[]);
                    pass.dispatch_workgroups(1, 1, 1);
                }
                context.queue.submit([encoder.finish()]);
                task.sample = 1;
                let rays = task.read_linear().unwrap();
                let vec = |p: [f32; 4]| Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
                let origin = vec(rays[0]) - Vec3::new(16.0, 16.0, 16.0);
                let direction = vec(rays[1]) - Vec3::new(2.0, 2.0, 2.0);
                let disk = (right * (azimuth * std::f64::consts::TAU).cos() + up * (azimuth * std::f64::consts::TAU).sin())
                    * (radial.sqrt() * lens.aperture_radius_mm);
                // Differentiate independently constructed f64 rays with the lens
                // point fixed; the production helper keeps no camera live state.
                let direction_at = |x: f64, y: f64| {
                    ((right * x + up * y) * (camera.half_height() * lens.focus_distance_mm / camera.distance) - back * lens.focus_distance_mm - disk)
                        .normalized()
                        .unwrap()
                };
                let h = 1e-3;
                for (slot, dx, dy) in [(2, h, 0.), (3, 0., -h)] {
                    // request(2) makes one pixel exactly one unit of normalized UV.
                    let expected = (direction_at(ux + dx, uy + dy) - direction_at(ux - dx, uy - dy)) / (2.0 * h);
                    let actual = vec(rays[slot]) - Vec3::new(16., 16., 16.);
                    assert!((actual - expected).len() < 4e-6, "fov={fov},uv=({ux},{uy}),slot={slot}: {actual:?} vs {expected:?}");
                }
                assert!((origin - eye - disk).len() < 3e-6);
                let focus_hit = origin + direction * ((focal - origin).dot(back) / direction.dot(back));
                assert!((focus_hit - focal).len() < 5e-6, "focus {focus_hit:?} != {focal:?}");
                // At axial depth z, signed displacement from the pinhole hit is
                // disk*(1-z/focus). Check both sides, including the flip beyond focus.
                for depth in [4.5, 18.0] {
                    let centre = eye - back * depth;
                    let hit = origin + direction * ((centre - origin).dot(back) / direction.dot(back));
                    let pinhole = eye + (focal - eye) * (depth / lens.focus_distance_mm);
                    assert!((hit - pinhole - disk * (1.0 - depth / lens.focus_distance_mm)).len() < 1e-5);
                }
            }
        }
    }
}

#[test]
fn gpu_zero_aperture_is_bit_identical_and_lens_admission_is_explicit() {
    let Some(executor) = executor() else { return };
    let mut request = request(16);
    request.camera.fov = 0.7;
    let render = |lens, fov| {
        let options = JobOptions { samples: 8, lens, seed: 1948, ..Default::default() };
        let mut image = request.clone();
        image.camera.fov = fov;
        let mut task = Task::new(executor.context.clone(), sphere(white()), image, options, CancelToken::default()).unwrap();
        for _ in 0..options.samples {
            task.complete_sample().unwrap();
        }
        task.read_linear().unwrap()
    };
    for fov in [0.0, 0.7] {
        assert_eq!(render(None, fov), render(Some(solidcraft_render::job::ThinLens { aperture_radius_mm: 0.0, focus_distance_mm: 8.0 }), fov));
    }
    let lens = Some(solidcraft_render::job::ThinLens { aperture_radius_mm: 0.3, focus_distance_mm: 4.0 });
    assert_eq!(render(lens, 0.7), render(lens, 0.7));
    request.camera.fov = 0.0;
    assert!(admission(&sphere(white()), &request, JobOptions { lens, ..Default::default() }).is_err());
    request.camera.fov = 1e-100;
    assert!(admission(&sphere(white()), &request, JobOptions { lens, ..Default::default() }).is_err());
}

#[test]
#[ignore = "manual glass transport evidence; set SOLIDCRAFT_GLASS_EVIDENCE to a PNG path"]
fn glass_transport_evidence() {
    let Some(executor) = executor() else { return };
    let output = std::env::var("SOLIDCRAFT_GLASS_EVIDENCE").unwrap();
    let mut meshes = Vec::new();
    for (centre, radius, color) in
        [(Vec3::new(-1.3, 0.0, 1.5), 1.5, Vec3::new(0.75, 0.94, 1.0)), (Vec3::new(2.0, 1.0, 1.0), 1.0, Vec3::new(1.0, 0.7, 0.35))]
    {
        let mut scene = sphere_grid(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap(), 96, 192);
        let mut body = scene.meshes.remove(0);
        for p in &mut Arc::make_mut(&mut body.mesh).positions {
            *p = *p * radius + centre;
        }
        body.transmission =
            Some(solidcraft_render::transmission::Transmission::new(Vec3::new(-color.x.ln(), -color.y.ln(), -color.z.ln()) / 5.0, None).unwrap());
        meshes.push(body);
    }
    for y in -16..16 {
        for x in -16..16 {
            let color = if (x + y) % 2 == 0 { Vec3::new(0.65, 0.7, 0.75) } else { Vec3::new(0.035, 0.055, 0.08) };
            let x = f64::from(x) * 2.0;
            let y = f64::from(y) * 2.0;
            let mesh = Mesh {
                positions: vec![
                    Vec3::new(x, y, -0.01),
                    Vec3::new(x + 2.0, y, -0.01),
                    Vec3::new(x + 2.0, y + 2.0, -0.01),
                    Vec3::new(x, y + 2.0, -0.01),
                ],
                normals: vec![Vec3::Z; 4],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
                tri_face: vec![0, 0],
                ..Default::default()
            };
            meshes.push(ProductMesh {
                name: "original checker tile".into(),
                mesh: Arc::new(mesh),
                color: [200; 3],
                opacity: 1.0,
                surface: Some(OpaqueSurface::new(color, 0.0, 0.65, 1.5).unwrap()),
                transmission: None,
                faces: vec![],
            });
        }
    }
    let bounds = meshes.iter().fold(Aabb3::EMPTY, |a, m| a.union(&m.mesh.bounds()));
    let camera = Camera { target: Vec3::new(0.0, 0.0, 1.0), yaw: -0.45, pitch: 0.5, distance: 13.0, fov: 0.65, ..Default::default() };
    let request = ImageRequest {
        studio_weights: None,
        integrator: Default::default(),
        raytrace: None,
        textures: None,
        environment: None,
        camera,
        width: 960,
        height: 720,
        background: [236, 240, 245],
        transparent_background: false,
    };
    let lens =
        std::env::var_os("SOLIDCRAFT_GLASS_DOF").map(|_| solidcraft_render::job::ThinLens { aperture_radius_mm: 0.5, focus_distance_mm: 12.0 });
    let options = JobOptions { samples: 256, max_depth: 16, seed: 1948, lens, ..Default::default() };
    let mut task = Task::new(executor.context.clone(), ProductScene { meshes, bounds }, request, options, CancelToken::default()).unwrap();
    task.environment = 1.0;
    loop {
        if let TaskPoll::Complete(png) = task.step(4096).unwrap() {
            std::fs::write(&output, png).unwrap();
            break;
        }
    }
}

fn glass_slab(thin: bool) -> ProductScene {
    let mut mesh = Mesh::default();
    let faces = [
        ([[-10., -10., 0.5], [10., -10., 0.5], [10., 10., 0.5], [-10., 10., 0.5]], Vec3::Z),
        ([[-10., 10., -0.5], [10., 10., -0.5], [10., -10., -0.5], [-10., -10., -0.5]], -Vec3::Z),
        ([[10., -10., -0.5], [10., 10., -0.5], [10., 10., 0.5], [10., -10., 0.5]], Vec3::X),
        ([[-10., 10., -0.5], [-10., -10., -0.5], [-10., -10., 0.5], [-10., 10., 0.5]], -Vec3::X),
        ([[10., 10., -0.5], [-10., 10., -0.5], [-10., 10., 0.5], [10., 10., 0.5]], Vec3::Y),
        ([[-10., -10., -0.5], [10., -10., -0.5], [10., -10., 0.5], [-10., -10., 0.5]], -Vec3::Y),
    ];
    for (index, (positions, normal)) in faces.into_iter().enumerate() {
        if thin && index > 0 {
            break;
        }
        let offset = mesh.positions.len() as u32;
        for p in positions {
            mesh.positions.push(Vec3::new(p[0], p[1], p[2]));
            mesh.normals.push(normal);
        }
        mesh.triangles.extend([[offset, offset + 1, offset + 2], [offset, offset + 2, offset + 3]]);
        mesh.tri_face.extend([0, 0]);
    }
    let bounds = mesh.bounds();
    ProductScene {
        bounds,
        meshes: vec![ProductMesh {
            name: "original slab".into(),
            mesh: Arc::new(mesh),
            color: [255; 3],
            opacity: 1.0,
            surface: Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap()),
            transmission: Some(
                solidcraft_render::transmission::Transmission::new(Vec3::new(-0.5_f64.ln(), -0.8_f64.ln(), 0.0), thin.then_some(1.0)).unwrap(),
            ),
            faces: vec![],
        }],
    }
}

#[test]
fn gpu_solid_and_thin_slabs_match_absorbing_interreflection_oracle() {
    let Some(executor) = executor() else { return };
    for (thin, thickness) in [(false, 1.0), (true, 1.0), (true, 0.0)] {
        let mut scene = glass_slab(thin);
        if thin {
            let sigma = scene.meshes[0].transmission.unwrap().extinction();
            scene.meshes[0].transmission = Some(solidcraft_render::transmission::Transmission::new(sigma, Some(thickness)).unwrap());
        }
        let (_, expected) = solidcraft_render::transmission::slab(scene.meshes[0].transmission.unwrap(), thickness, 1.0, 1.0, 1.5).unwrap();
        let mut request = request(1);
        request.camera.fov = 0.0001;
        let options = JobOptions { samples: 4096, max_depth: 16, seed: 1948, ..Default::default() };
        let mut task = Task::new(executor.context.clone(), scene, request, options, CancelToken::default()).unwrap();
        task.environment = 1.0;
        task.environment_bottom_only = true;
        // Deliberately large offset: Beer uses unshifted geometric endpoints.
        task.epsilon = 0.05;
        task.studio = 0.0;
        for _ in 0..options.samples {
            task.complete_sample().unwrap();
        }
        let actual = task.read_linear().unwrap()[0];
        for (a, b) in actual.iter().zip([expected.x, expected.y, expected.z]) {
            assert!((f64::from(*a) - b).abs() < 0.015, "slab thin={thin} thickness={thickness}: {actual:?} vs {expected:?}");
        }
    }
}

#[test]
fn shader_validates_without_a_device() {
    for source in [SHADER, SINGLE_MEDIUM_SHADER] {
        let module = naga::front::wgsl::parse_str(source).unwrap();
        let parameters = module.types.iter().find(|(_, ty)| ty.name.as_deref() == Some("Params")).unwrap().1;
        let naga::TypeInner::Struct { span, .. } = &parameters.inner else { panic!("Params must be a struct") };
        assert_eq!(*span as usize, PARAMETER_BYTES);
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap();
        assert_eq!(module.entry_points.iter().find(|entry| entry.name == "main").unwrap().workgroup_size, [PATH_WORKGROUP_SIZE, 1, 1]);
    }
    let source = photons::shader_source().unwrap();
    let caustic = naga::front::wgsl::parse_str(&source).unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&caustic).unwrap();
}

/// #6705: native compilers inline every call. With 33 inlined BVH walks in `main` (three path
/// samples, a shadow walk per light source family) NVIDIA's Vulkan compiler crashed or ran out
/// of memory and never built the pipeline. Count the walks `main` inlines, so another call site
/// is a deliberate choice, checked on such a driver.
#[test]
fn main_inlines_few_bvh_walks() {
    fn callees(block: &naga::Block, out: &mut Vec<naga::Handle<naga::Function>>) {
        for statement in block.iter() {
            match statement {
                naga::Statement::Call { function, .. } => out.push(*function),
                naga::Statement::Block(inner) => callees(inner, out),
                naga::Statement::If { accept, reject, .. } => {
                    callees(accept, out);
                    callees(reject, out);
                }
                naga::Statement::Switch { cases, .. } => cases.iter().for_each(|case| callees(&case.body, out)),
                naga::Statement::Loop { body, continuing, .. } => {
                    callees(body, out);
                    callees(continuing, out);
                }
                _ => {}
            }
        }
    }
    fn walks(module: &naga::Module, function: &naga::Function) -> usize {
        let mut called = Vec::new();
        callees(&function.body, &mut called);
        called
            .into_iter()
            .map(|handle| {
                let callee = &module.functions[handle];
                let walk = matches!(callee.name.as_deref(), Some("trace_precise_within" | "trace_ordered_within" | "origin_boundary"));
                usize::from(walk) + walks(module, callee)
            })
            .sum()
    }
    let module = naga::front::wgsl::parse_str(SHADER).unwrap();
    let main = module.entry_points.iter().find(|entry| entry.name == "main").unwrap();
    let inlined = walks(&module, &main.function);
    assert!(inlined <= 5, "`main` inlines {inlined} BVH walks");
}

#[test]
fn geometry_materialization_lane_keeps_positive_zero_at_the_shader_offset() {
    let Some(executor) = executor() else { return };
    let module = naga::front::wgsl::parse_str(SHADER).unwrap();
    let params = module.types.iter().find(|(_, ty)| ty.name.as_deref() == Some("Params")).unwrap().1;
    let naga::TypeInner::Struct { members, .. } = &params.inner else { panic!("Params must be a struct") };
    let offset = members.iter().find(|member| member.name.as_deref() == Some("studio_weights")).unwrap().offset as usize + 12;
    let mut task = Task::new(
        executor.context,
        ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY },
        request(1),
        JobOptions::default(),
        CancelToken::default(),
    )
    .unwrap();
    for weights in [None, Some([0.0; 3]), Some([1.0, 0.5, 0.25])] {
        task.request.studio_weights = weights;
        for sample in [0, 1, 4095] {
            task.sample = sample;
            let bytes = task.parameters(1).unwrap();
            assert_eq!(&bytes[offset..offset + 4], &0u32.to_le_bytes(), "reserved arithmetic lane must remain positive zero");
        }
    }
    task.request.studio_weights = None;
    task.options.lighting = Some(solidcraft_render::job::StudioLighting { environment: [0.75; 3], ..Default::default() });
    assert_eq!(&task.parameters(1).unwrap()[offset..offset + 4], &0u32.to_le_bytes());
}

#[test]
fn render_lights_path_visibility_uniform_keeps_the_shader_contract() {
    shader_validates_without_a_device();
    let request = request(4);
    assert!(request.studio_weights.is_none());
}

/// #3217: the path tracer opens Vulkan adapters, or Metal on macOS, never D3D12 or GL.
#[test]
fn path_devices_are_vulkan_or_metal_only() {
    let picked = solidcraft_viewport_gpu::select::pick_path(solidcraft_viewport_gpu::select::Policy::Auto, |_| true);
    if let Some((adapter, _)) = &picked {
        assert!(matches!(adapter.get_info().backend, wgpu::Backend::Vulkan | wgpu::Backend::Metal));
    }
    if solidcraft_viewport_gpu::select::path_backends().is_empty() {
        assert!(picked.is_none());
    }
}

/// #6706: executors opened side by side never compile the path pipeline at the same time.
#[test]
fn path_pipelines_compile_one_at_a_time() {
    use std::sync::atomic::AtomicUsize;
    let inside = AtomicUsize::new(0);
    let most = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                let _compiling = one_compile_at_a_time();
                most.fetch_max(inside.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(5));
                inside.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    assert_eq!(most.load(Ordering::SeqCst), 1);
}

#[test]
fn viewport_path_backends_keep_windows_on_its_selected_backend() {
    for backend in [wgpu::Backend::Dx12, wgpu::Backend::Gl, wgpu::Backend::Vulkan] {
        let allowed = viewport_path_backends(backend, true);
        assert_eq!(allowed, wgpu::Backends::from(backend));
        assert_eq!(allowed.contains(wgpu::Backends::VULKAN), backend == wgpu::Backend::Vulkan);
    }
}

#[test]
fn viewport_path_backends_preserve_other_platforms() {
    for backend in [wgpu::Backend::Vulkan, wgpu::Backend::Gl, wgpu::Backend::Metal] {
        assert_eq!(viewport_path_backends(backend, false), solidcraft_viewport_gpu::select::path_backends());
    }
}

#[test]
fn admission_rejects_hostile_parameters_and_transparency() {
    let mut scene = sphere(white());
    let mut r = request(16);
    for size in [0, (GPU_PATH_LIMITS.max_dimension + 1) as u32, u32::MAX] {
        r.width = size;
        assert!(admission(&scene, &r, JobOptions::default()).is_err());
    }
    r.width = 16;
    for samples in [0, solidcraft_render::job::MAX_SAMPLES_PER_PIXEL + 1, u32::MAX] {
        assert!(admission(&scene, &r, JobOptions { samples, ..Default::default() }).is_err());
    }
    for depth in [0, u32::from(solidcraft_render::job::MAX_PATH_DEPTH) + 1, u32::MAX] {
        assert!(admission(&scene, &r, JobOptions { max_depth: depth, ..Default::default() }).is_err());
    }
    for ev in [f64::NAN, f64::INFINITY, 33.0] {
        assert!(admission(&scene, &r, JobOptions { exposure_ev: ev, ..Default::default() }).is_err());
    }
    scene.meshes[0].opacity = 0.5;
    assert!(admission(&scene, &r, JobOptions::default()).is_err());
    scene.meshes[0].opacity = 1.0;
    Arc::make_mut(&mut scene.meshes[0].mesh).triangles[0][0] = u32::MAX;
    assert!(scene::flatten(&scene, Vec3::ZERO, &CancelToken::default()).is_err());
}

#[test]
fn admission_ignores_the_cpu_raster_projected_pixel_limit() {
    let mesh = Mesh {
        positions: vec![Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 0.0)],
        normals: vec![Vec3::Z; 3],
        triangles: vec![[0, 1, 2]; 3201],
        tri_face: vec![0; 3201],
        ..Default::default()
    };
    let scene = ProductScene {
        bounds: mesh.bounds(),
        meshes: vec![ProductMesh {
            name: "overlapping triangles".into(),
            mesh: Arc::new(mesh),
            color: [255; 3],
            opacity: 1.0,
            surface: Some(white()),
            transmission: None,
            faces: Vec::new(),
        }],
    };
    let mut r = request(100);
    r.camera.distance = 1.0;
    r.camera.fov = 0.0;
    let raster_error = solidcraft_render::job::admit(&scene, &r).unwrap_err();
    assert_eq!(raster_error.code, ErrorCode::LimitExceeded);
    assert!(raster_error.message.contains("128000000 pixel tests"), "{raster_error:?}");
    // Exercise the actual GPU admission path without requiring a Vulkan device.
    let gpu = admission(&scene, &r, JobOptions::default()).unwrap();
    assert_eq!(gpu.projected_pixel_tests, None);
    assert!(gpu.estimated_total_bytes > gpu.geometry_bytes);
}

#[test]
fn admission_applies_the_gpu_estimate_instead_of_the_cpu_raster_budget() {
    // A library-bounded source (not an ordinary UI scene): about 100 MiB of face appearances
    // on a small mesh at the largest image. Its CPU raster estimate is above 384 MiB (covered
    // in the render crate); the GPU's own estimate stays within its 512 MiB job cap.
    let count = (100 * 1024 * 1024 / std::mem::size_of::<solidcraft_render::FaceAppearance>()).min(1_000_000);
    let mut scene = sphere(white());
    let appearance = solidcraft_render::FaceAppearance { face: 0, color: [255; 3], opacity: 1.0, surface: None, transmission: None };
    scene.meshes[0].faces = vec![appearance; count];
    let r = request(2048);
    let a = admission(&scene, &r, JobOptions::default()).unwrap();
    assert!(a.estimated_total_bytes <= 512 * 1024 * 1024, "{a:?}");
    assert_eq!(a.face_appearances, count as u64);
    // The CPU host still rejects the same source.
    assert_eq!(solidcraft_render::job::admit(&scene, &r).unwrap_err().code, ErrorCode::LimitExceeded);
}

#[test]
fn bvh_handles_empty_and_cancelled_scenes() {
    let empty = ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY };
    let upload = scene::flatten(&empty, Vec3::ZERO, &CancelToken::default()).unwrap();
    assert_eq!(upload.nodes.len(), 32);
    assert_eq!(&upload.nodes[12..16], &u32::MAX.to_le_bytes());
    let cancel = CancelToken::default();
    cancel.cancel();
    assert_eq!(scene::flatten(&sphere(white()), Vec3::ZERO, &cancel).err().unwrap().code, ErrorCode::Cancelled);
}

#[test]
fn gpu_brdf_matches_f64_surface_oracle() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("path BRDF reference"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    for metallic in [0.0, 0.5, 1.0] {
        for roughness in [0.045, 0.25, 1.0] {
            for ior in [1.0, 1.5, 3.0] {
                let surface = OpaqueSurface::new(Vec3::new(0.8, 0.5, 0.2), metallic, roughness, ior).unwrap();
                let mut task = Task::new(context.clone(), sphere(surface), request(1), JobOptions::default(), CancelToken::default()).unwrap();
                for (view, light) in [
                    (Vec3::Z, Vec3::Z),
                    (Vec3::new(0.5, 0.2, 1.0), Vec3::new(-0.4, 0.3, 0.8)),
                    (Vec3::new(1.0, 0.0, 0.02), Vec3::new(-1.0, 0.1, 0.02)),
                ] {
                    let view = view.normalized().unwrap();
                    let light = light.normalized().unwrap();
                    let mut params = vec![0; 48];
                    scene::vector(&mut params, light, 0.0);
                    scene::vector(&mut params, Vec3::Z, 0.0);
                    scene::vector(&mut params, view, 0.0);
                    scene::vector(&mut params, Vec3::ZERO, 0.0);
                    context.queue.write_buffer(&task.uniform, 0, &params);
                    let mut encoder = context.device.create_command_encoder(&Default::default());
                    {
                        let mut pass = encoder.begin_compute_pass(&Default::default());
                        pass.set_pipeline(&pipeline);
                        pass.set_bind_group(0, &task.bind, &[]);
                        pass.dispatch_workgroups(1, 1, 1);
                    }
                    context.queue.submit([encoder.finish()]);
                    task.sample = 1;
                    let actual = task.read_linear().unwrap()[0];
                    let expected = surface.brdf(Vec3::Z, view, light).unwrap();
                    if metallic == 0.0 && ior == 1.0 {
                        let diffuse = Vec3::new(0.8, 0.5, 0.2) / std::f64::consts::PI;
                        assert!((expected - diffuse).len() < 1e-12, "index-matched diffuse has no interface reflection");
                    }
                    for (a, b) in actual.iter().zip([expected.x, expected.y, expected.z]) {
                        assert!((f64::from(*a) - b).abs() <= 3e-3 * b.abs() + 2e-5, "m={metallic}, r={roughness}, ior={ior}: {a} vs {b}");
                    }
                }
            }
        }
    }
}

/// Assemble the same viewport declarations, lighting helpers and PBR implementation
/// as production, plus a compute entry point that calls the actual BRDF.
fn viewport_brdf_probe_source() -> String {
    format!(
        "{}\n{}\n{}",
        solidcraft_viewport_gpu::view::SHADER,
        solidcraft_viewport_gpu::pbr::SOURCE,
        r#"
struct Probe { base: vec4<f32>, parameters: vec4<f32>, view: vec4<f32>, light: vec4<f32>, };
// Bindings 0 and 1 belong to the production viewport uniform and palette.
@group(0) @binding(2) var<uniform> probe: Probe;
@group(0) @binding(3) var<storage, read_write> result: array<vec4<f32>>;
@compute @workgroup_size(1)
fn probe_pbr() {
    result[0] = vec4<f32>(pbr_brdf(probe.base.xyz, probe.parameters.xyz,
        vec3<f32>(0.0,0.0,1.0),probe.view.xyz,probe.light.xyz),1.0);
}
"#
    )
}

#[test]
fn viewport_brdf_probe_shader_is_valid() {
    let source = viewport_brdf_probe_source();
    let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
}

/// The viewport embeds a separate WGSL translation. Exercise its actual function
/// on the same native device as the path-tracer oracle, not a copied Rust formula.
#[test]
fn viewport_brdf_matches_f64_surface_oracle_at_index_match() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let source = viewport_brdf_probe_source();
    let creation_scopes = scopes(&context.device);
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("actual viewport PBR oracle"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = context.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let pipeline_layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some("probe_pbr"),
        compilation_options: Default::default(),
        cache: None,
    });
    let uniform = context.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let values = context.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = context.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind = context.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 2, resource: uniform.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: values.as_entire_binding() },
        ],
    });
    checked(creation_scopes).expect("viewport BRDF probe resource creation must validate");
    for metallic in [0.0, 0.5, 1.0] {
        for roughness in [0.045, 0.5, 1.0] {
            for ior in [1.0, 1.5, 3.0] {
                let base = Vec3::new(0.8, 0.5, 0.2);
                let surface = OpaqueSurface::new(base, metallic, roughness, ior).unwrap();
                for (view, light) in [
                    (Vec3::Z, Vec3::Z),
                    (Vec3::new(0.5, 0.0, 1.0).normalized().unwrap(), Vec3::new(-0.4, 0.3, 0.8).normalized().unwrap()),
                    (Vec3::new(1.0, 0.0, 0.02).normalized().unwrap(), Vec3::new(-1.0, 0.1, 0.02).normalized().unwrap()),
                ] {
                    let input = [
                        0.8_f32,
                        0.5,
                        0.2,
                        0.0,
                        metallic as f32,
                        roughness as f32,
                        ior as f32,
                        0.0,
                        view.x as f32,
                        view.y as f32,
                        view.z as f32,
                        0.0,
                        light.x as f32,
                        light.y as f32,
                        light.z as f32,
                        0.0,
                    ];
                    let bytes: Vec<u8> = input.iter().flat_map(|value| value.to_le_bytes()).collect();
                    let dispatch_scopes = scopes(&context.device);
                    context.queue.write_buffer(&uniform, 0, &bytes);
                    let mut encoder = context.device.create_command_encoder(&Default::default());
                    {
                        let mut pass = encoder.begin_compute_pass(&Default::default());
                        pass.set_pipeline(&pipeline);
                        pass.set_bind_group(0, &bind, &[]);
                        pass.dispatch_workgroups(1, 1, 1);
                    }
                    encoder.copy_buffer_to_buffer(&values, 0, &readback, 0, 16);
                    let submission = context.queue.submit([encoder.finish()]);
                    checked(dispatch_scopes).expect("viewport BRDF probe dispatch must validate");
                    let slice = readback.slice(..);
                    let (tx, rx) = std::sync::mpsc::channel();
                    slice.map_async(wgpu::MapMode::Read, move |result| {
                        let _ = tx.send(result);
                    });
                    context.wait(submission, TIMEOUT).unwrap();
                    rx.try_recv().unwrap().unwrap();
                    let data = slice.get_mapped_range().unwrap();
                    let actual: Vec<f32> = data.as_chunks::<4>().0.iter().map(|bytes| f32::from_le_bytes(*bytes)).collect();
                    drop(data);
                    readback.unmap();
                    let expected = surface.brdf(Vec3::Z, view, light).unwrap();
                    if metallic == 0.0 && ior == 1.0 {
                        assert!((expected - base / std::f64::consts::PI).len() < 1e-12);
                    }
                    for (a, b) in actual.iter().zip([expected.x, expected.y, expected.z]) {
                        assert!((f64::from(*a) - b).abs() < 2e-5 + 3e-3 * b.abs(), "viewport m={metallic}, r={roughness}, ior={ior}: {a} vs {b}");
                    }
                }
            }
        }
    }
}

#[test]
fn gpu_tiny_scene_matches_cpu_studio_with_face_appearance() {
    let Some(executor) = executor() else { return };
    let surface = OpaqueSurface::new(Vec3::new(0.7, 0.2, 0.1), 0.5, 0.3, 1.5).unwrap();
    let mesh = Mesh {
        positions: vec![Vec3::new(-3.0, -3.0, 0.0), Vec3::new(3.0, -3.0, 0.0), Vec3::new(3.0, 3.0, 0.0), Vec3::new(-3.0, 3.0, 0.0)],
        normals: vec![Vec3::Z; 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        tri_face: vec![7, 9],
        ..Default::default()
    };
    let bounds = mesh.bounds();
    let scene = ProductScene {
        meshes: vec![ProductMesh {
            name: "reference plane".into(),
            mesh: Arc::new(mesh),
            color: [255; 3],
            opacity: 1.0,
            surface: Some(white()),
            transmission: None,
            faces: [7, 9]
                .map(|face| solidcraft_render::FaceAppearance { face, color: [0; 3], opacity: 1.0, surface: Some(surface), transmission: None })
                .to_vec(),
        }],
        bounds,
    };
    let mut task =
        Task::new(executor.context.clone(), scene, request(4), JobOptions { samples: 4, max_depth: 1, ..Default::default() }, CancelToken::default())
            .unwrap();
    for _ in 0..4 {
        task.complete_sample().unwrap();
    }
    let actual = task.read_linear().unwrap();
    let expected = solidcraft_render::studio::shade(surface, Vec3::Z, Vec3::Z).unwrap();
    for pixel in actual {
        assert_eq!(pixel[3], 1.0);
        for (a, b) in pixel.iter().zip([expected.x, expected.y, expected.z]) {
            assert!((f64::from(*a) - b).abs() < 3e-3 * b.abs() + 2e-5, "{a} vs {b}");
        }
    }
}

#[test]
fn gpu_furnace_fixed_seed_and_cancellation() {
    let Some(executor) = executor() else { return };
    let options = JobOptions { samples: 4096, max_depth: 8, seed: 37, ..Default::default() };
    let mut task = Task::new(executor.context.clone(), sphere(white()), request(8), options, CancelToken::default()).unwrap();
    task.environment = 1.0;
    task.studio = 0.0;
    for _ in 0..options.samples {
        task.complete_sample().unwrap();
    }
    let result = task.read_linear().unwrap();
    eprintln!("furnace centre {:?}; BVH {:.3} ms; upload {} bytes / {:.3} ms", result[27], task.metrics.0, task.metrics.1, task.metrics.2);
    // The oracle includes Schlick entry/exit attenuation even at IOR=1, plus single-
    // scattering GGX. It is near-white, not an exactly energy-preserving Lambert model.
    for index in [27, 28, 35, 36] {
        assert!((result[index][0] - 1.0).abs() < 0.07, "furnace radiance: {:?}", result[index]);
    }
    let mut repeat = Task::new(executor.context.clone(), sphere(white()), request(8), options, CancelToken::default()).unwrap();
    repeat.environment = 1.0;
    repeat.studio = 0.0;
    for _ in 0..options.samples {
        repeat.complete_sample().unwrap();
    }
    assert_eq!(result, repeat.read_linear().unwrap(), "fixed sample indices and seed must repeat on the same adapter");
    let token = CancelToken::default();
    let mut cancelled = Task::new(executor.context.clone(), sphere(white()), request(16), options, token.clone()).unwrap();
    cancelled.dispatch().unwrap();
    token.cancel();
    assert_eq!(cancelled.step(4096).err().unwrap().code, ErrorCode::Cancelled);
}

#[test]
fn skinny_polar_triangles_do_not_shadow_themselves() {
    let Some(executor) = executor() else { return };
    let mut image = request(32);
    image.camera.distance = 16.0;
    image.camera.fov = 0.0005;
    let mut task = Task::new(
        executor.context.clone(),
        sphere_section(white(), 512, 1024, 1),
        image,
        JobOptions { samples: 1, max_depth: 1, ..Default::default() },
        CancelToken::default(),
    )
    .unwrap();
    task.complete_sample().unwrap();
    let pixels = task.read_linear().unwrap();
    for (index, pixel) in pixels.iter().enumerate() {
        assert_eq!(pixel[3], 1.0, "polar cap covers the image: {index} {pixel:?}");
        assert!(pixel[0] > 0.1, "outward-facing cap must receive unobstructed studio light: {index} {pixel:?}");
    }
}

#[test]
fn native_host_cancel_never_publishes_a_frame() {
    let Some(executor) = executor() else { return };
    let host = ImageHost::with_executor(Arc::new(executor)).unwrap();
    let job = host
        .submit_with_options(
            solidcraft_render::job::JobId { session: 1, serial: 1, generation: 1 },
            1,
            sphere(white()),
            request(64),
            JobOptions { samples: 4096, ..Default::default() },
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status = job.status().unwrap();
        if status.progress.phase == Phase::Running {
            break;
        }
        assert!(!status.progress.phase.terminal(), "{status:?}");
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    job.cancel().unwrap();
    assert_eq!(job.wait_timeout(Duration::from_secs(30)).unwrap().progress.phase, Phase::Cancelled);
    assert!(job.frame().unwrap().is_none());
}

/// Explicit benchmark: large allocation and sustained GPU load must not run in ordinary CI.
#[test]
#[ignore = "manual 1M-triangle 1920x1080 GPU performance measurement"]
fn path_performance() {
    let executor = executor().expect("benchmark requires an adapter");
    let samples = std::env::var("SOLIDCRAFT_PATH_BENCH_SAMPLES").ok().map(|s| s.parse::<u32>().unwrap()).unwrap_or(1);
    let options = JobOptions { samples, ..Default::default() };
    options.validate().unwrap();
    let surface = OpaqueSurface::new(Vec3::new(0.8, 0.4, 0.2), 0.25, 0.4, 1.5).unwrap();
    let scene = sphere_grid(surface, 512, 1024);
    let mut request = request(1920);
    request.height = 1080;
    let admitted = executor.admit(&scene, &request, options).unwrap();
    let mut task = Task::new(executor.context.clone(), scene, request, options, CancelToken::default()).unwrap();
    let started = Instant::now();
    while task.sample < samples {
        task.complete_sample().unwrap();
    }
    let seconds = started.elapsed().as_secs_f64();
    let linear = task.read_linear().unwrap();
    assert!(linear.iter().all(|p| p.iter().all(|v| v.is_finite())));
    let coverage = linear.iter().map(|p| f64::from(p[3])).sum::<f64>() / linear.len() as f64;
    assert!((0.16..0.19).contains(&coverage), "sphere must occupy the expected projected area: {coverage}");
    let centre = linear[540 * 1920 + 960];
    let lit = linear.iter().filter(|p| p[0] > 0.01).count();
    eprintln!(
        "PATH_BENCH_DIAGNOSTIC coverage={coverage:.6} centre={centre:?} lit_pixels={lit} nearby={:?}",
        [linear[500 * 1920 + 960], linear[540 * 1920 + 1000], linear[580 * 1920 + 960]]
    );
    assert_eq!(centre[3], 1.0);
    assert!(centre[0] > 0.01, "benchmark must actually shade the sphere: {centre:?}");
    eprintln!("PATH_BENCH_IMAGE coverage={coverage:.6} centre={centre:?}");
    eprintln!(
        "PATH_BENCH input_triangles={} resolution=1920x1080 spp={} bvh_ms={:.3} upload_bytes={} upload_ms={:.3} admitted_bytes={} render_seconds={:.6} ms_per_spp={:.3}",
        admitted.triangles,
        samples,
        task.metrics.0,
        task.metrics.1,
        task.metrics.2,
        admitted.estimated_total_bytes,
        seconds,
        seconds * 1000.0 / f64::from(samples)
    );
}

#[test]
fn admission_refuses_cpu_options_without_opening_a_device() {
    let scene = ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY };
    let mut input = request(16);
    input.integrator = solidcraft_render::job::Integrator::Raytrace;
    assert_eq!(admission(&scene, &input, JobOptions::default()).unwrap_err().code, ErrorCode::InvalidRequest);
    input.integrator = solidcraft_render::job::Integrator::Raster;
    input.raytrace = Some(solidcraft_render::job::RaytraceOptions::default());
    assert_eq!(admission(&scene, &input, JobOptions::default()).unwrap_err().code, ErrorCode::InvalidRequest);
}

#[test]
fn gpu_clear_coat_brdf_matches_f64_surface_oracle() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("path BRDF reference"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    for metallic in [0.0, 1.0] {
        for roughness in [0.045, 0.12, 0.5] {
            for ior in [1.5, 3.0] {
                let surface = OpaqueSurface::new(Vec3::new(0.8, 0.5, 0.2), metallic, 0.3, 1.5)
                    .unwrap()
                    .with_clear_coat(solidcraft_viewport::clear_coat::ClearCoat::new(1.0, roughness, ior).unwrap());
                let mut task = Task::new(context.clone(), sphere(surface), request(1), JobOptions::default(), CancelToken::default()).unwrap();
                for (view, light) in [
                    (Vec3::Z, Vec3::Z),
                    (Vec3::new(0.5, 0.2, 1.0), Vec3::new(-0.4, 0.3, 0.8)),
                    (Vec3::new(1.0, 0.0, 0.02), Vec3::new(-1.0, 0.1, 0.02)),
                ] {
                    let view = view.normalized().unwrap();
                    let light = light.normalized().unwrap();
                    let mut params = vec![0; 48];
                    scene::vector(&mut params, light, 0.0);
                    scene::vector(&mut params, Vec3::Z, 0.0);
                    scene::vector(&mut params, view, 0.0);
                    scene::vector(&mut params, Vec3::ZERO, 0.0);
                    context.queue.write_buffer(&task.uniform, 0, &params);
                    let mut encoder = context.device.create_command_encoder(&Default::default());
                    {
                        let mut pass = encoder.begin_compute_pass(&Default::default());
                        pass.set_pipeline(&pipeline);
                        pass.set_bind_group(0, &task.bind, &[]);
                        pass.dispatch_workgroups(1, 1, 1);
                    }
                    context.queue.submit([encoder.finish()]);
                    task.sample = 1;
                    let actual = task.read_linear().unwrap()[0];
                    let expected = surface.brdf(Vec3::Z, view, light).unwrap();
                    for (a, b) in actual.iter().zip([expected.x, expected.y, expected.z]) {
                        assert!((f64::from(*a) - b).abs() <= 3e-3 * b.abs() + 2e-5, "m={metallic}, r={roughness}, ior={ior}: {a} vs {b}");
                    }
                }
            }
        }
    }
}

#[test]
fn gpu_clear_coat_pdf_matches_f64_surface_oracle() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("path BRDF reference"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("pdf_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    for metallic in [0.0, 1.0] {
        for roughness in [0.045, 0.12, 0.5] {
            for ior in [1.5, 3.0] {
                let surface = OpaqueSurface::new(Vec3::new(0.8, 0.5, 0.2), metallic, 0.3, 1.5)
                    .unwrap()
                    .with_clear_coat(solidcraft_viewport::clear_coat::ClearCoat::new(1.0, roughness, ior).unwrap());
                let mut task = Task::new(context.clone(), sphere(surface), request(1), JobOptions::default(), CancelToken::default()).unwrap();
                for (view, light) in [
                    (Vec3::Z, Vec3::Z),
                    (Vec3::new(0.5, 0.2, 1.0), Vec3::new(-0.4, 0.3, 0.8)),
                    (Vec3::new(1.0, 0.0, 0.02), Vec3::new(-1.0, 0.1, 0.02)),
                ] {
                    let view = view.normalized().unwrap();
                    let light = light.normalized().unwrap();
                    let mut params = vec![0; 48];
                    scene::vector(&mut params, light, 0.0);
                    scene::vector(&mut params, Vec3::Z, 0.0);
                    scene::vector(&mut params, view, 0.0);
                    scene::vector(&mut params, Vec3::ZERO, 0.0);
                    context.queue.write_buffer(&task.uniform, 0, &params);
                    let mut encoder = context.device.create_command_encoder(&Default::default());
                    {
                        let mut pass = encoder.begin_compute_pass(&Default::default());
                        pass.set_pipeline(&pipeline);
                        pass.set_bind_group(0, &task.bind, &[]);
                        pass.dispatch_workgroups(1, 1, 1);
                    }
                    context.queue.submit([encoder.finish()]);
                    task.sample = 1;
                    let actual = task.read_linear().unwrap()[0];
                    let expected = solidcraft_viewport::clear_coat::surface_pdf(surface, Vec3::Z, view, light).unwrap();
                    for (a, b) in actual.iter().zip([expected, 0.0, 0.0]) {
                        assert!((f64::from(*a) - b).abs() <= 3e-3 * b.abs() + 2e-5, "m={metallic}, r={roughness}, ior={ior}: {a} vs {b}");
                    }
                }
            }
        }
    }
}

#[test]
fn gpu_clear_coat_samples_the_pdf_mixture_components() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("coat sampling oracle"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("surface_sample_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    for weight in [0.0, 0.4, 1.0] {
        let surface = OpaqueSurface::new(Vec3::new(0.02, 0.02, 0.02), 0.0, 0.45, 1.5)
            .unwrap()
            .with_clear_coat(solidcraft_viewport::clear_coat::ClearCoat::new(weight, 0.12, 1.5).unwrap());
        let mut task = Task::new(context.clone(), sphere(surface), request(1), JobOptions::default(), CancelToken::default()).unwrap();
        let p = weight * 0.5;
        // One variate strictly inside each nonempty mixture interval.
        for component in [(1.0 - p) * 0.25, (1.0 - p) * 0.75, 1.0 - p * 0.5] {
            if component >= 1.0 {
                continue;
            }
            for x in [0.125_f64, 0.75] {
                let mut params = vec![0; 48];
                scene::vector(&mut params, Vec3::ZERO, 0.0);
                scene::vector(&mut params, Vec3::Z, 0.0);
                scene::vector(&mut params, Vec3::Z, 0.0);
                scene::vector(&mut params, Vec3::new(component, x, 0.25), 0.0);
                context.queue.write_buffer(&task.uniform, 0, &params);
                let mut encoder = context.device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&pipeline);
                    pass.set_bind_group(0, &task.bind, &[]);
                    pass.dispatch_workgroups(1, 1, 1);
                }
                context.queue.submit([encoder.finish()]);
                task.sample = 1;
                let actual = task.read_linear().unwrap()[0];
                let expected = if component < (1.0 - p) * 0.5 {
                    Vec3::new(0.0, x.sqrt(), (1.0 - x).sqrt())
                } else {
                    let r = if component >= 1.0 - p { 0.12_f64 } else { 0.45_f64 };
                    let c = ((1.0 - x) / (1.0 + (r.powi(4) - 1.0) * x)).sqrt();
                    Vec3::new(2.0 * c * (1.0 - c * c).sqrt(), 0.0, 2.0 * c * c - 1.0)
                } + Vec3::new(2.0, 2.0, 2.0);
                for (a, b) in actual.iter().zip([expected.x, expected.y, expected.z]) {
                    assert!((f64::from(*a) - b).abs() < 2e-5, "weight={weight}, component={component}, x={x}: {actual:?}, {expected:?}");
                }
            }
        }
    }
}

#[test]
fn gpu_studio_area_mis_matches_independent_quadrature_and_distance_visibility() {
    use solidcraft_render::job::{AreaLight, AreaLights, StudioLighting};
    let Some(executor) = executor() else { return };
    let surface = white();
    let plane = |z: f64| {
        let mesh = Mesh {
            positions: vec![Vec3::new(-10.0, -10.0, z), Vec3::new(10.0, -10.0, z), Vec3::new(10.0, 10.0, z), Vec3::new(-10.0, 10.0, z)],
            normals: vec![Vec3::Z; 4],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            tri_face: vec![0, 0],
            ..Default::default()
        };
        ProductMesh {
            name: "original quadrature plane".into(),
            mesh: Arc::new(mesh),
            color: [255; 3],
            opacity: 1.0,
            surface: Some(surface),
            transmission: None,
            faces: Vec::new(),
        }
    };
    let light = AreaLight { center: [2.0, 0.0, 2.0], u: [1.0, 0.0, 0.0], v: [0.0, -1.0, 0.0], radiance: [3.0; 3] };
    let mut expected = 0.0;
    for y in 0..256 {
        for x in 0..256 {
            let delta = Vec3::new(1.0 + (f64::from(x) + 0.5) / 128.0, -1.0 + (f64::from(y) + 0.5) / 128.0, 2.0);
            let l = delta.normalized().unwrap();
            expected += surface.brdf(Vec3::Z, Vec3::Z, l).unwrap().x * 3.0 * l.z * l.z / delta.len2() * 4.0 / (256.0 * 256.0);
        }
    }
    for depth in [1, 2] {
        for blocker in [None, Some(4.0), Some(1.0)] {
            let mut scene = ProductScene { meshes: vec![plane(0.0)], bounds: Aabb3::EMPTY };
            if let Some(z) = blocker {
                scene.meshes.push(plane(z));
            }
            // Camera sits below any blocker and sees the receiver. Geometry behind
            // the emitter must not shadow it; the nearer blocker must.
            let mut image = request(1);
            image.camera.distance = 0.1;
            image.camera.fov = 0.001;
            let options = JobOptions {
                samples: 4096,
                max_depth: depth,
                seed: 37,
                lighting: Some(StudioLighting {
                    punctual_lights: Default::default(),
                    directional_lights: Default::default(),
                    sun: None,
                    environment: [0.0; 3],
                    area_lights: AreaLights::new(&[light]).unwrap(),
                }),
                ..Default::default()
            };
            let mut task = Task::new(executor.context.clone(), scene, image, options, CancelToken::default()).unwrap();
            assert_eq!(task.parameters(1).unwrap().len(), PARAMETER_BYTES);
            for _ in 0..options.samples {
                task.complete_sample().unwrap();
            }
            let value = f64::from(task.read_linear().unwrap()[0][0]);
            if blocker == Some(1.0) {
                assert!(value < 1e-6, "blocked depth{depth}: {value}");
            } else {
                assert!((value - expected).abs() < 0.04 * expected, "depth{depth}, blocker{blocker:?}: {value} vs {expected}");
            }
        }
    }
}

#[test]
fn gpu_studio_environment_mis_matches_furnace_at_last_and_continuing_bounces() {
    use solidcraft_render::job::StudioLighting;
    let Some(executor) = executor() else { return };
    for depth in [1, 2] {
        let options = JobOptions {
            samples: 4096,
            max_depth: depth,
            seed: 37,
            lighting: Some(StudioLighting { sun: None, environment: [1.0; 3], ..Default::default() }),
            ..Default::default()
        };
        let mut task = Task::new(executor.context.clone(), sphere(white()), request(8), options, CancelToken::default()).unwrap();
        for _ in 0..options.samples {
            task.complete_sample().unwrap();
        }
        let pixels = task.read_linear().unwrap();
        for index in [27, 28, 35, 36] {
            assert!((pixels[index][0] - 1.0).abs() < 0.07, "depth{depth}: {:?}", pixels[index]);
        }
    }
}

#[test]
fn gpu_emitter_inside_glass_applies_final_segment_absorption() {
    use solidcraft_render::job::{AreaLight, AreaLights, StudioLighting};
    let Some(executor) = executor() else { return };
    let mut glass = sphere(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.0).unwrap());
    glass.meshes[0].transmission = Some(solidcraft_render::transmission::Transmission::new(Vec3::new(0.7, 0.3, 0.0), None).unwrap());
    let light = AreaLight { center: [0.0; 3], u: [0.4, 0.0, 0.0], v: [0.0, 0.4, 0.0], radiance: [3.0; 3] };
    let options = JobOptions {
        samples: 16,
        max_depth: 2,
        lighting: Some(StudioLighting {
            punctual_lights: Default::default(),
            directional_lights: Default::default(),
            sun: None,
            environment: [0.0; 3],
            area_lights: AreaLights::new(&[light]).unwrap(),
        }),
        ..Default::default()
    };
    let mut image = request(2);
    image.camera.fov = 0.0001;
    let mut task = Task::new(executor.context.clone(), glass, image, options, CancelToken::default()).unwrap();
    for _ in 0..options.samples {
        task.complete_sample().unwrap();
    }
    for pixel in task.read_linear().unwrap() {
        for (value, sigma) in pixel.iter().zip([0.7_f64, 0.3, 0.0]) {
            assert!((f64::from(*value) - 3.0 * (-sigma).exp()).abs() < 0.002, "{pixel:?}");
        }
    }
}

fn environment_plane() -> ProductScene {
    let mesh = Mesh {
        positions: vec![Vec3::new(-3.0, -3.0, 0.0), Vec3::new(3.0, -3.0, 0.0), Vec3::new(3.0, 3.0, 0.0), Vec3::new(-3.0, 3.0, 0.0)],
        normals: vec![Vec3::Z; 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        tri_face: vec![0, 0],
        ..Default::default()
    };
    let bounds = mesh.bounds();
    ProductScene {
        meshes: vec![ProductMesh {
            name: "original environment test plane".into(),
            mesh: Arc::new(mesh),
            color: [255; 3],
            opacity: 1.0,
            surface: Some(white()),
            transmission: None,
            faces: Vec::new(),
        }],
        bounds,
    }
}
#[test]
fn gpu_rough_metal_furnace_restores_the_missing_energy() {
    use solidcraft_render::job::StudioLighting;
    let Some(executor) = executor() else { return };
    let mut scene = environment_plane();
    scene.meshes[0].surface = Some(OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 1.0, 1.0, 1.5).unwrap());
    let mut image = request(8);
    image.camera.fov = 0.0;
    for enabled in [false, true] {
        let options = JobOptions {
            samples: 256,
            max_depth: 1,
            seed: 37,
            metal_energy_compensation: enabled,
            lighting: Some(StudioLighting { environment: [1.0; 3], ..Default::default() }),
            ..Default::default()
        };
        let mut task = Task::new(executor.context.clone(), scene.clone(), image.clone(), options, CancelToken::default()).unwrap();
        for _ in 0..options.samples {
            task.complete_sample().unwrap();
        }
        let pixels = task.read_linear().unwrap();
        let mut sum = [0.0; 3];
        for pixel in &pixels {
            assert_eq!(pixel[3], 1.0);
            for (total, channel) in sum.iter_mut().zip(pixel) {
                *total += channel;
            }
        }
        let mean = sum.map(|channel| channel / pixels.len() as f32);
        // At alpha=1 and normal incidence, uncorrected separable Smith GGX
        // integrates to integral[0,1] mu/(1+mu) dmu = 1-ln(2).
        let expected = if enabled { 1.0 } else { 1.0 - 2.0_f32.ln() };
        eprintln!("metal compensation={enabled}: furnace={mean:?}");
        for channel in mean {
            assert!((channel - expected).abs() < 0.025, "enabled={enabled}: {mean:?} vs {expected}");
        }
    }
}

fn original_hdr() -> Vec<u8> {
    let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
    for _ in 0..8 {
        bytes.extend_from_slice(&[128, 128, 128, 129]);
    }
    bytes
}
#[test]
fn gpu_hdr_environment_matches_constant_furnace_and_resource_admission() {
    use solidcraft_render::lighting::environment::EnvironmentSource;
    let Some(executor) = executor() else { return };
    let source = Arc::new(EnvironmentSource::hdr(original_hdr(), 0.3, 1.0, false).unwrap());
    let mut image = request(8);
    image.environment = Some(source.clone());
    let options = JobOptions { samples: 2048, max_depth: 2, seed: 37, ..Default::default() };
    let plain = admission(&sphere(white()), &request(8), options).unwrap();
    let with_map = admission(&sphere(white()), &image, options).unwrap();
    assert_eq!(with_map.estimated_total_bytes - plain.estimated_total_bytes, source.admitted_bytes());
    let mut task = Task::new(executor.context.clone(), sphere(white()), image, options, CancelToken::default()).unwrap();
    for _ in 0..options.samples {
        task.complete_sample().unwrap();
    }
    let pixels = task.read_linear().unwrap();
    for index in [27, 28, 35, 36] {
        assert!((pixels[index][0] - 1.0).abs() < 0.07, "{:?}", pixels[index]);
    }
}
#[test]
fn gpu_finite_sun_matches_cone_quadrature_at_final_and_continuing_bounces() {
    use solidcraft_render::job::{StudioLighting, Sun};
    let Some(executor) = executor() else { return };
    let sun = Sun { direction: [0.0, 0.0, 1.0], angular_radius_rad: 0.4, radiance: [3.0; 3] };
    let mut expected = 0.0;
    for i in 0..4096 {
        let z = 1.0 - (f64::from(i) + 0.5) / 4096.0 * (1.0 - sun.angular_radius_rad.cos());
        let l = Vec3::new((1.0 - z * z).sqrt(), 0.0, z);
        expected += white().brdf(Vec3::Z, Vec3::Z, l).unwrap().x * z * 3.0 * sun.solid_angle() / 4096.0;
    }
    for max_depth in [1, 2] {
        let mut image = request(2);
        image.camera.distance = 0.1;
        image.camera.fov = 0.001;
        let options = JobOptions {
            samples: 2048,
            max_depth,
            seed: 123,
            lighting: Some(StudioLighting { sun: Some(sun), ..Default::default() }),
            ..Default::default()
        };
        let mut task = Task::new(executor.context.clone(), environment_plane(), image, options, CancelToken::default()).unwrap();
        for _ in 0..options.samples {
            task.complete_sample().unwrap();
        }
        for pixel in task.read_linear().unwrap() {
            assert!((f64::from(pixel[0]) - expected).abs() < 0.03 * expected, "depth{max_depth}: {pixel:?}, {expected}");
        }
    }
}

#[test]
fn gpu_environment_orientation_rotation_intensity_and_pdf_match_cpu() {
    use solidcraft_render::lighting::environment::EnvironmentSource;
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y 2 +X 4\n".to_vec();
    for i in 0..8u8 {
        bytes.extend_from_slice(&[16 * (i + 1), 32, 64, 129]);
    }
    let rotation = 0.7_f64;
    let intensity = 2.5;
    let source = Arc::new(EnvironmentSource::hdr(bytes, rotation, intensity, true).unwrap());
    let cpu = source.prepare(&CancelToken::default()).unwrap();
    let mut image = request(1);
    image.environment = Some(source);
    let mut task = Task::new(context.clone(), sphere(white()), image, JobOptions::default(), CancelToken::default()).unwrap();
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("environment_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    for row in 0..2 {
        for column in 0..4 {
            let theta = std::f64::consts::PI * (f64::from(row) + 0.5) / 2.0;
            let phi = std::f64::consts::TAU * (f64::from(column) + 0.5) / 4.0;
            let local = Vec3::new(theta.sin() * phi.cos(), theta.sin() * phi.sin(), theta.cos());
            let direction =
                Vec3::new(rotation.cos() * local.x - rotation.sin() * local.y, rotation.sin() * local.x + rotation.cos() * local.y, local.z);
            let mut params = task.parameters(1).unwrap();
            for (out, value) in params[48..60].as_chunks_mut::<4>().0.iter_mut().zip([direction.x, direction.y, direction.z]) {
                out.copy_from_slice(&(value as f32).to_le_bytes());
            }
            context.queue.write_buffer(&task.uniform, 0, &params);
            let mut encoder = context.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &task.bind, &[]);
                pass.dispatch_workgroups(1, 1, 1);
            }
            context.queue.submit([encoder.finish()]);
            task.sample = 1;
            let actual = task.read_linear().unwrap()[0];
            let (rgb, pdf) = cpu.evaluate(local).unwrap();
            for (a, b) in actual.iter().zip(rgb.into_iter().map(|v| v * intensity as f32).chain([pdf])) {
                assert!((a - b).abs() < 1e-5, "{actual:?} vs {rgb:?}/{pdf}");
            }
        }
    }
}

fn shadow_probe(context: &Arc<Context>, scene: ProductScene, origin: Vec3, direction: Vec3, limit: f64) -> [[f32; 4]; 2] {
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("shadow connection oracle"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("shadow_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    let mut task = Task::new(context.clone(), scene, request(2), JobOptions::default(), CancelToken::default()).unwrap();
    let mut params = vec![0; 32];
    scene::vector(&mut params, origin, 0.0);
    scene::vector(&mut params, Vec3::ZERO, 0.0);
    scene::vector(&mut params, Vec3::ZERO, 0.0);
    scene::vector(&mut params, direction, limit as f32);
    // Large offset catches Beer intervals incorrectly measured from shifted origins.
    scene::vector(&mut params, Vec3::new(0.01, 0.0, 0.0), 0.0);
    context.queue.write_buffer(&task.uniform, 0, &params);
    let mut encoder = context.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &task.bind, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    context.queue.submit([encoder.finish()]);
    task.sample = 1;
    let pixels = task.read_linear().unwrap();
    [pixels[0], pixels[1]]
}

#[test]
fn gpu_shadow_sheets_solids_endpoints_and_invalid_crossings_match_oracles() {
    use solidcraft_render::transmission::{slab, solid_shadow};
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    for thin in [true, false] {
        let mut scene = glass_slab(thin);
        let optics = scene.meshes[0].transmission.unwrap().with_shadow_transmission(true);
        scene.meshes[0].transmission = Some(optics);
        for cosine in [1.0_f64, 0.8] {
            let d = Vec3::new((1.0 - cosine * cosine).sqrt(), 0.0, -cosine);
            let actual = shadow_probe(context, scene.clone(), Vec3::new(0.0, 0.0, 2.0), d, 8.0);
            let expected =
                if thin { slab(optics, 1.0, cosine, 1.0, 1.5).unwrap().1 } else { solid_shadow(optics, 1.0 / cosine, cosine, cosine, 1.5).unwrap() };
            for (a, b) in actual[0].iter().zip([expected.x, expected.y, expected.z]) {
                assert!((f64::from(*a) - b).abs() < 2e-5, "thin={thin}, cosine={cosine}: {actual:?} vs {expected:?}");
            }
            assert_eq!(actual[1][1], if thin { 1.0 } else { 2.0 });
        }
        let before = shadow_probe(context, scene.clone(), Vec3::new(0.0, 0.0, 2.0), -Vec3::Z, 1.0);
        assert_eq!(before[0], [1.0; 4]);
        if !thin {
            let inside = shadow_probe(context, scene.clone(), Vec3::new(0.0, 0.0, 2.0), -Vec3::Z, 2.0);
            let expected = optics.attenuation(0.5).unwrap() * (0.96 / 2.25);
            for (a, b) in inside[0].iter().zip([expected.x, expected.y, expected.z]) {
                assert!((f64::from(*a) - b).abs() < 2e-5);
            }
            // Shadow rays start at shading points, which may touch or lie inside
            // glass. A first exit infers that medium and absorbs from the origin.
            let from_inside = shadow_probe(context, scene.clone(), Vec3::ZERO, Vec3::Z, 8.0);
            let expected = optics.attenuation(0.5).unwrap() * 0.96;
            for (a, b) in from_inside[0].iter().zip([expected.x, expected.y, expected.z]) {
                assert!((f64::from(*a) - b).abs() < 2e-5, "{from_inside:?} vs {expected:?}");
            }
            assert_eq!(from_inside[1][1], 1.0);
        }
        scene.meshes[0].transmission = Some(optics.with_shadow_transmission(false));
        assert_eq!(shadow_probe(context, scene, Vec3::new(0.0, 0.0, 2.0), -Vec3::Z, 8.0)[0], [0.0, 0.0, 0.0, 1.0]);
    }
    let mut opaque = glass_slab(true);
    opaque.meshes[0].transmission = None;
    assert_eq!(shadow_probe(context, opaque, Vec3::new(0.0, 0.0, 2.0), -Vec3::Z, 8.0)[0], [0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn gpu_shadow_interface_walk_is_bounded_and_stacked_sheet_pdf_matches() {
    let Some(executor) = executor() else { return };
    for count in [2_u32, 64, 65] {
        let mut scene = glass_slab(true);
        let base = scene.meshes[0].mesh.clone();
        let mut mesh = Mesh::default();
        for layer in 0..count {
            let offset = mesh.positions.len() as u32;
            mesh.positions.extend(base.positions.iter().map(|p| *p - Vec3::Z * f64::from(layer)));
            mesh.normals.extend_from_slice(&base.normals);
            mesh.triangles.extend(base.triangles.iter().map(|t| t.map(|i| i + offset)));
            mesh.tri_face.extend_from_slice(&base.tri_face);
        }
        scene.meshes[0].mesh = Arc::new(mesh);
        scene.meshes[0].transmission = Some(solidcraft_render::transmission::Transmission::new(Vec3::ZERO, Some(0.0)).unwrap());
        let actual = shadow_probe(&executor.context, scene, Vec3::new(0.0, 0.0, 2.0), -Vec3::Z, 100.0);
        if count == 65 {
            assert_eq!(actual[0], [0.0, 0.0, 0.0, 1.0]);
        } else {
            let expected = (0.96_f64 / 1.04).powi(count as i32);
            assert!((f64::from(actual[0][0]) - expected).abs() < 2e-5);
            assert!((f64::from(actual[1][0]) - expected).abs() < 2e-5);
            assert_eq!(actual[1][1], count as f32);
        }
    }
}

#[test]
fn gpu_shadow_mis_does_not_double_direct_energy_through_index_matched_glass() {
    use solidcraft_render::job::{AreaLight, AreaLights, StudioLighting};
    let Some(executor) = executor() else { return };
    let surface = white();
    let receiver = ProductMesh {
        name: "shadow MIS receiver".into(),
        mesh: Arc::new(Mesh {
            positions: vec![Vec3::new(-20.0, -20.0, -2.0), Vec3::new(20.0, -20.0, -2.0), Vec3::new(20.0, 20.0, -2.0), Vec3::new(-20.0, 20.0, -2.0)],
            normals: vec![Vec3::Z; 4],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            tri_face: vec![0, 0],
            ..Default::default()
        }),
        color: [255; 3],
        opacity: 1.0,
        surface: Some(surface),
        transmission: None,
        faces: vec![],
    };
    let light = AreaLight { center: [0.0, 0.0, 2.0], u: [4.0, 0.0, 0.0], v: [0.0, -4.0, 0.0], radiance: [1.0; 3] };
    let mut expected = 0.0;
    for y in 0..128 {
        for x in 0..128 {
            let delta = Vec3::new(-4.0 + (f64::from(x) + 0.5) / 16.0, -4.0 + (f64::from(y) + 0.5) / 16.0, 4.0);
            let l = delta.normalized().unwrap();
            expected += surface.brdf(Vec3::Z, Vec3::Z, l).unwrap().x * l.z * l.z / delta.len2() * 64.0 / 16384.0;
        }
    }
    for (thin, enabled, depth) in [(true, true, 2), (true, true, 4), (true, false, 4), (false, true, 1), (false, true, 4), (false, false, 4)] {
        let mut scene = glass_slab(thin);
        scene.meshes[0].surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.0).unwrap());
        scene.meshes[0].transmission =
            Some(solidcraft_render::transmission::Transmission::new(Vec3::ZERO, thin.then_some(0.0)).unwrap().with_shadow_transmission(enabled));
        scene.meshes.push(receiver.clone());
        let options = JobOptions {
            samples: 4096,
            max_depth: depth,
            seed: 37,
            lighting: Some(StudioLighting {
                punctual_lights: Default::default(),
                directional_lights: Default::default(),
                sun: None,
                environment: [0.0; 3],
                area_lights: AreaLights::new(&[light]).unwrap(),
            }),
            ..Default::default()
        };
        let mut image = request(1);
        image.camera.target = Vec3::new(0.0, 0.0, -2.0);
        image.camera.distance = 0.1;
        image.camera.fov = 0.001;
        let mut task = Task::new(executor.context.clone(), scene, image, options, CancelToken::default()).unwrap();
        for _ in 0..options.samples {
            task.complete_sample().unwrap();
        }
        let actual = f64::from(task.read_linear().unwrap()[0][0]);
        assert!((actual - expected).abs() < 0.08 * expected, "thin={thin}, enabled={enabled}, depth={depth}: {actual} vs {expected}");
    }
}

#[test]
fn gpu_absorbing_two_sheet_mis_matches_f64_quadrature() {
    use solidcraft_render::job::{AreaLight, AreaLights, StudioLighting};
    let Some(executor) = executor() else { return };
    let surface = white();
    // A tiny receiver excludes sheet-reflection returns from this direct-only oracle.
    // Its half-width still exceeds the primary ray footprint by two orders of magnitude.
    let receiver = ProductMesh {
        name: "shadow MIS receiver".into(),
        mesh: Arc::new(Mesh {
            positions: vec![Vec3::new(-0.01, -0.01, -2.0), Vec3::new(0.01, -0.01, -2.0), Vec3::new(0.01, 0.01, -2.0), Vec3::new(-0.01, 0.01, -2.0)],
            normals: vec![Vec3::Z; 4],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            tri_face: vec![0, 0],
            ..Default::default()
        }),
        color: [255; 3],
        opacity: 1.0,
        surface: Some(surface),
        transmission: None,
        faces: vec![],
    };
    let light = AreaLight { center: [0.0, 0.0, 2.0], u: [4.0, 0.0, 0.0], v: [0.0, -4.0, 0.0], radiance: [1.0; 3] };
    let optics = solidcraft_render::transmission::Transmission::new(Vec3::new(0.2, 0.1, 0.05), Some(0.7)).unwrap();
    let mut expected = 0.0;
    for y in 0..128 {
        for x in 0..128 {
            let delta = Vec3::new(-4.0 + (f64::from(x) + 0.5) / 16.0, -4.0 + (f64::from(y) + 0.5) / 16.0, 4.0);
            let l = delta.normalized().unwrap();
            let transmission = solidcraft_render::transmission::slab(optics, 0.7, l.z, 1.0, 1.5).unwrap().1.x;
            expected += transmission * transmission * surface.brdf(Vec3::Z, Vec3::Z, l).unwrap().x * l.z * l.z / delta.len2() * 64.0 / 16384.0;
        }
    }
    for (thin, enabled, depth) in [(true, true, 1), (true, true, 4), (true, false, 4)] {
        let mut scene = glass_slab(thin);
        scene.meshes[0].transmission = Some(optics.with_shadow_transmission(enabled));
        let mut second = scene.meshes[0].clone();
        second.name = "second absorbing sheet".into();
        for p in &mut Arc::make_mut(&mut second.mesh).positions {
            p.z -= 1.0;
        }
        scene.meshes.push(second);
        scene.meshes.push(receiver.clone());
        let options = JobOptions {
            samples: 4096,
            max_depth: depth,
            seed: 37,
            lighting: Some(StudioLighting {
                punctual_lights: Default::default(),
                directional_lights: Default::default(),
                sun: None,
                environment: [0.0; 3],
                area_lights: AreaLights::new(&[light]).unwrap(),
            }),
            ..Default::default()
        };
        let mut image = request(1);
        image.camera.target = Vec3::new(0.0, 0.0, -2.0);
        image.camera.distance = 0.1;
        image.camera.fov = 0.001;
        let mut task = Task::new(executor.context.clone(), scene, image, options, CancelToken::default()).unwrap();
        for _ in 0..options.samples {
            task.complete_sample().unwrap();
        }
        let actual = f64::from(task.read_linear().unwrap()[0][0]);
        assert!((actual - expected).abs() < 0.08 * expected, "thin={thin}, enabled={enabled}, depth={depth}: {actual} vs {expected}");
    }
}

#[test]
fn gpu_hdr_and_sun_shadow_mis_preserve_index_matched_energy() {
    use solidcraft_render::job::{StudioLighting, Sun};
    use solidcraft_render::lighting::environment::EnvironmentSource;
    let Some(executor) = executor() else { return };
    let sun = Sun { direction: [0.0, 0.0, 1.0], angular_radius_rad: 0.4, radiance: [3.0; 3] };
    let mut sun_expected = 0.0;
    for i in 0..4096 {
        let z = 1.0 - (f64::from(i) + 0.5) / 4096.0 * (1.0 - sun.angular_radius_rad.cos());
        let l = Vec3::new((1.0 - z * z).sqrt(), 0.0, z);
        sun_expected += white().brdf(Vec3::Z, Vec3::Z, l).unwrap().x * z * 3.0 * sun.solid_angle() / 4096.0;
    }
    // The original HDR has unit radiance everywhere. With IOR one the receiver
    // is pure diffuse; retain the independent directional-albedo quadrature.
    let mut hdr_expected = 0.0;
    for i in 0..256 {
        let z = (f64::from(i) + 0.5) / 256.0;
        for j in 0..256 {
            let phi = (f64::from(j) + 0.5) / 256.0 * std::f64::consts::TAU;
            let l = Vec3::new((1.0 - z * z).sqrt() * phi.cos(), (1.0 - z * z).sqrt() * phi.sin(), z);
            hdr_expected += white().brdf(Vec3::Z, Vec3::Z, l).unwrap().x * z * std::f64::consts::TAU / 65536.0;
        }
    }
    for solar in [false, true] {
        for (thin, depth) in [(true, 2), (true, 4), (false, 1), (false, 4)] {
            let mut scene = glass_slab(thin);
            scene.meshes[0].surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.0).unwrap());
            scene.meshes[0].transmission =
                Some(solidcraft_render::transmission::Transmission::new(Vec3::ZERO, thin.then_some(0.0)).unwrap().with_shadow_transmission(true));
            let mut receiver = environment_plane().meshes.remove(0);
            for p in &mut Arc::make_mut(&mut receiver.mesh).positions {
                p.z -= 2.0;
            }
            scene.meshes.push(receiver);
            scene.bounds = scene.meshes.iter().fold(Aabb3::EMPTY, |a, m| a.union(&m.mesh.bounds()));
            let mut image = request(1);
            image.camera.target = Vec3::new(0.0, 0.0, -2.0);
            image.camera.distance = 0.1;
            image.camera.fov = 0.001;
            if !solar {
                image.environment = Some(Arc::new(EnvironmentSource::hdr(original_hdr(), 0.3, 1.0, false).unwrap()));
            }
            let options = JobOptions {
                samples: 4096,
                max_depth: depth,
                seed: 37,
                lighting: Some(StudioLighting { sun: solar.then_some(sun), ..Default::default() }),
                ..Default::default()
            };
            let mut task = Task::new(executor.context.clone(), scene, image, options, CancelToken::default()).unwrap();
            for _ in 0..options.samples {
                task.complete_sample().unwrap();
            }
            let actual = f64::from(task.read_linear().unwrap()[0][0]);
            let expected = if solar { sun_expected } else { hdr_expected };
            assert!((actual - expected).abs() < 0.07 * expected, "sun={solar}, thin={thin}, depth={depth}: {actual} vs {expected}");
        }
    }
}

fn texture_fixture(frame: solidcraft_render::job::textures::PlanarFrame) -> Arc<solidcraft_render::job::textures::RuntimeTextures> {
    texture_fixture_filtering(frame, false)
}

fn texture_fixture_filtering(
    frame: solidcraft_render::job::textures::PlanarFrame,
    portable: bool,
) -> Arc<solidcraft_render::job::textures::RuntimeTextures> {
    use image::ImageEncoder;
    use solidcraft_render::{
        job::textures::{RuntimeTextures, TextureBinding, TextureMap},
        texture_input::MapRole,
    };
    let map = |pixels: &[u8], role| {
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes).write_image(pixels, 2, 2, image::ExtendedColorType::Rgba8).unwrap();
        if portable { TextureMap::new_portable(bytes, role).unwrap() } else { TextureMap::new(bytes, role).unwrap() }
    };
    Arc::new(
        RuntimeTextures::new(vec![TextureBinding {
            body_index: 0,
            body_name: "original sphere".into(),
            frame,
            normal_strength: 0.75,
            base_color: Some(map(&[128, 64, 255, 0, 255, 0, 32, 255, 64, 128, 192, 0, 0, 255, 128, 255], MapRole::BaseColor)),
            normal: Some(map(&[128, 128, 255, 0, 192, 128, 224, 255, 128, 192, 224, 0, 64, 96, 224, 255], MapRole::NormalOpenGl)),
            roughness: None,
            metallic: None,
            base_color_mode: Default::default(),
            flip_v: false,
        }])
        .unwrap(),
    )
}
#[test]
fn texture_admission_rejects_glass_and_stale_identity_and_honors_cancellation() {
    use solidcraft_render::job::textures::PlanarFrame;
    let source = texture_fixture(PlanarFrame { origin: [0.; 3], u: [1., 0., 0.], v: [0., 1., 0.] });
    let mut input = request(2);
    input.textures = Some(source.clone());
    let mut scene = sphere(white());
    let plain = admission(&scene, &request(2), JobOptions::default()).unwrap();
    let textured = admission(&scene, &input, JobOptions::default()).unwrap();
    assert_eq!(textured.preparation_bytes - plain.preparation_bytes, source.working_bytes());
    assert!(solidcraft_render::job::admit(&scene, &input).is_err());
    assert!(
        admission(&scene, &input, JobOptions { texture_filtering: true, ..Default::default() }).unwrap_err().message.contains("texture filtering"),
        "a requested filter may not silently use an LOD0 capture"
    );
    let mut filtered_input = input.clone();
    filtered_input.textures = Some(Arc::new((*source).clone().with_filtering(true)));
    assert!(
        admission(&scene, &filtered_input, JobOptions::default()).unwrap_err().message.contains("texture filtering"),
        "capture and request must agree in both directions"
    );
    let cancel = CancelToken::default();
    cancel.cancel();
    assert_eq!(textures::flatten(Some(&source), 1, Vec3::ZERO, &cancel).unwrap_err().code, ErrorCode::Cancelled);
    scene.meshes[0].name = "stale".into();
    assert!(admission(&scene, &input, JobOptions::default()).is_err());
    scene.meshes[0].name = "original sphere".into();
    scene.meshes[0].transmission = Some(clear_glass());
    assert!(admission(&scene, &input, JobOptions::default()).is_err());
}
#[test]
fn host_readiness_includes_unmapped_non_nested_glass() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    assert!(context.prepared_scene_pipeline(true, false, false).is_ok());
    assert!(context.prepared_scene_pipeline(false, false, false).is_ok(), "a ready host must already prepare ordinary unmapped glass");
    let plain = scene_pipeline_variant(false, false, false).unwrap();
    assert!(context.scene_pipelines[plain].get().is_some());
    for (opaque, maps, nested) in [(true, true, false), (false, false, true), (false, true, false), (false, true, true)] {
        let variant = scene_pipeline_variant(opaque, maps, nested).unwrap();
        assert!(context.scene_pipelines[variant].get().is_none(), "uncommon map/nesting programs stay lazy");
    }
    let mut glass = sphere(white());
    glass.meshes[0].transmission = Some(clear_glass());
    let options = JobOptions { samples: 1, max_depth: 3, ..Default::default() };
    let mut task = Task::new(context.clone(), glass, request(2), options, CancelToken::default()).unwrap();
    assert!(!task.opaque_scene && !task.has_textures);
    task.complete_sample().unwrap();
    let pixels = task.read_linear().unwrap();
    assert_eq!(pixels.len(), 4);
    assert!(pixels.iter().flatten().all(|value| value.is_finite()));
}

#[test]
fn opaque_scene_specialization_preserves_body_face_and_volume_media() {
    let mut variants = std::collections::BTreeSet::new();
    for opaque in [false, true] {
        for maps in [false, true] {
            for nested in [false, true] {
                let slot = scene_pipeline_variant(opaque, maps, nested);
                assert_eq!(slot.is_none(), opaque && !maps, "only opaque/no-map uses the eager base");
                assert!(slot.is_none_or(|index| index < SCENE_PIPELINE_CACHE_SLOTS));
                variants.insert(slot);
            }
            if opaque {
                assert_eq!(scene_pipeline_variant(opaque, maps, false), scene_pipeline_variant(opaque, maps, true));
            } else {
                assert_ne!(
                    scene_pipeline_variant(opaque, maps, false),
                    scene_pipeline_variant(opaque, maps, true),
                    "disjoint and nested options retain separate scene cache entries"
                );
            }
        }
    }
    assert_eq!(variants.len(), SCENE_PIPELINE_CACHE_SLOTS + 1);
    let mut scene = sphere(white());
    let options = JobOptions::default();
    assert!(use_opaque_pipeline(&scene, options));
    scene.meshes[0].transmission = Some(clear_glass());
    assert!(!use_opaque_pipeline(&scene, options));
    scene.meshes[0].transmission = None;
    scene.meshes[0].faces.push(solidcraft_render::FaceAppearance {
        face: 0,
        color: [255; 3],
        opacity: 1.0,
        surface: Some(white()),
        transmission: Some(clear_glass()),
    });
    assert!(!use_opaque_pipeline(&scene, options));
    scene.meshes[0].faces[0].transmission = None;
    assert!(use_opaque_pipeline(&scene, options));
    scene.meshes[0].faces[0].opacity = 0.5;
    assert!(!use_opaque_pipeline(&scene, options));
    scene.meshes[0].faces.clear();
    assert!(!use_opaque_pipeline(&scene, JobOptions { volume: Some(Default::default()), ..options }));
    assert!(use_opaque_pipeline(
        &scene,
        JobOptions { volume: Some(solidcraft_render::job::volume::HomogeneousVolume { extinction_per_mm: 0.0, ..Default::default() }), ..options }
    ));
}
#[test]
fn gpu_planar_color_and_normal_maps_match_cpu_sampling_oracle() {
    use solidcraft_render::job::textures::{PlanarFrame, sample_map};
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("texture_reference"),
        compilation_options: wgpu::PipelineCompilationOptions { constants: &[("HAS_TEXTURES", 1.0)], ..Default::default() },
        cache: None,
    });
    for handed in [1., -1.] {
        let frame = PlanarFrame { origin: [0.; 3], u: [2., 0., 0.], v: [0., 2. * handed, 0.] };
        let source = texture_fixture(frame);
        let binding = &source.bindings()[0];
        let base = binding.base_color.as_ref().unwrap().decode(&CancelToken::default()).unwrap();
        let normals = binding.normal.as_ref().unwrap().decode(&CancelToken::default()).unwrap();
        for point in [Vec3::new(0.5, 0.5, 0.), Vec3::new(-0.25, 1.125, 0.), Vec3::new(0., 0., 0.)] {
            let mut input = request(2);
            input.textures = Some(source.clone());
            let mut task = Task::new(context.clone(), sphere(white()), input, JobOptions::default(), CancelToken::default()).unwrap();
            let mut params = vec![0; 32];
            scene::vector(&mut params, point, 0.);
            scene::vector(&mut params, Vec3::ZERO, 0.);
            scene::vector(&mut params, Vec3::Z, 0.);
            scene::vector(&mut params, Vec3::Z, 0.);
            context.queue.write_buffer(&task.uniform, 0, &params);
            let mut encoder = context.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &task.bind, &[]);
                pass.dispatch_workgroups(1, 1, 1);
            }
            context.queue.submit([encoder.finish()]);
            task.sample = 1;
            let actual = task.read_linear().unwrap();
            let uv = frame.uv(point).unwrap();
            let expected = sample_map(&base, [2, 2], uv).unwrap();
            let mapped = frame.mapped_normal(Vec3::Z, Vec3::Z, sample_map(&normals, [2, 2], uv).unwrap(), 0.75).unwrap();
            for (a, b) in actual[0].iter().zip(expected) {
                assert!((f64::from(*a) - b).abs() < 2e-6, "color {actual:?} vs {expected:?}");
            }
            for (a, b) in actual[1].iter().zip([mapped.x + 2., mapped.y + 2., mapped.z + 2.]) {
                assert!((f64::from(*a) - b).abs() < 3e-6, "normal {actual:?} vs {mapped:?}");
            }
        }
    }
}

#[test]
fn gpu_quality_texture_preserves_minor_axis_detail_and_linear_mip_precision() {
    use image::ImageEncoder;
    use solidcraft_render::{
        job::textures::{PlanarFrame, RuntimeTextures, TextureMap},
        texture_input::MapRole,
    };
    use solidcraft_viewport::surface::srgb8_to_linear;
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let source = texture_fixture(PlanarFrame { origin: [0.0; 3], u: [2.0, 0.0, 0.0], v: [0.0, 2.0, 0.0] });
    let mut binding = source.bindings()[0].clone();
    let pixels: Vec<_> =
        (0..64).flat_map(|index| [if index % 2 == 0 { 0 } else { 255 }, if index / 8 % 2 == 0 { 0 } else { 255 }, 128, 255]).collect();
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded).write_image(&pixels, 8, 8, image::ExtendedColorType::Rgba8).unwrap();
    binding.base_color = Some(TextureMap::new(encoded, MapRole::BaseColor).unwrap());
    binding.normal = None;
    let filtered = Arc::new(RuntimeTextures::new(vec![binding]).unwrap().with_filtering(true));
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("texture_mip_reference"),
        compilation_options: wgpu::PipelineCompilationOptions { constants: &[("HAS_TEXTURES", 1.0)], ..Default::default() },
        cache: None,
    });
    for (dx, dy, red, green, lod) in [
        (Vec3::X, Vec3::Y * 0.03125, 0.5, 0.0, 0.0),
        (Vec3::X * 0.03125, Vec3::Y, 0.0, 0.5, 0.0),
        (Vec3::X * 4.0, Vec3::Y * 4.0, 0.5, 0.5, 3.0),
        (Vec3::X * 1e15, Vec3::Y * 1e15, 0.5, 0.5, 3.0),
    ] {
        let mut input = request(2);
        input.textures = Some(filtered.clone());
        let mut task =
            Task::new(context.clone(), sphere(white()), input, JobOptions { texture_filtering: true, ..Default::default() }, CancelToken::default())
                .unwrap();
        let mut params = vec![0; 32];
        scene::vector(&mut params, Vec3::new(-1.875, 0.125, 0.0), 0.0);
        scene::vector(&mut params, dx, 0.0);
        scene::vector(&mut params, dy, 0.0);
        scene::vector(&mut params, Vec3::Z, 0.0);
        context.queue.write_buffer(&task.uniform, 0, &params);
        let mut encoder = context.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &task.bind, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        context.queue.submit([encoder.finish()]);
        task.sample = 1;
        let actual = task.read_linear().unwrap();
        assert_eq!(actual[1][0], lod);
        for (actual, expected) in actual[0].iter().take(3).zip([red, green, srgb8_to_linear(128)]) {
            assert!((f64::from(*actual) - expected).abs() < 0.000008, "packed linear sample {actual} versus {expected}");
        }
    }
}

#[test]
fn gpu_quality_normal_mips_broaden_mapped_roughness_only_for_unresolved_detail() {
    use image::ImageEncoder;
    use solidcraft_render::{
        job::textures::{PlanarFrame, RuntimeTextures, TextureMap},
        texture_input::MapRole,
    };
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let source = format!(
        "{SHADER}\n@compute @workgroup_size(1) fn texture_variance_reference() {{
            let t=triangle(0u);
            let mapped=texture_surface(texture_record(t.optics.w),t.base.xyz,t.parameters.x,t.parameters.y,u.eye.xyz,TextureGradients(u.right.xy,u.up.xy),false);
            accumulation[0]=vec4<f32>(mapped.roughness,0.0,0.0,1.0);
        }}"
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
        entry_point: Some("texture_variance_reference"),
        compilation_options: wgpu::PipelineCompilationOptions { constants: &[("HAS_TEXTURES", 1.0)], ..Default::default() },
        cache: None,
    });
    let frame = PlanarFrame { origin: [0.0; 3], u: [2.0, 0.0, 0.0], v: [0.0, 2.0, 0.0] };
    let fixture = texture_fixture(frame);
    let png = |width, values: &[u8]| {
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes).write_image(values, width, 1, image::ExtendedColorType::Rgba8).unwrap();
        bytes
    };
    let roughness = 0.2_f64;
    let strength = 0.65_f64;
    let expected_variance = 205_f64.powi(2) / (2.0 * 205_f64.powi(2) + 1.0);
    let expected_roughness = (roughness.powi(4) + strength.powi(2) * expected_variance).powf(0.25);
    for (quality, mixed, normal_strength, gradient, expected) in [
        (true, true, strength, 4.0, expected_roughness),
        (true, true, strength, 0.0, roughness),
        (true, false, strength, 4.0, roughness),
        (true, true, 0.0, 4.0, roughness),
        (false, true, strength, 4.0, roughness),
    ] {
        let second = if mixed { 25 } else { 230 };
        let mut binding = fixture.bindings()[0].clone();
        binding.base_color = None;
        binding.normal = Some(TextureMap::new_portable(png(2, &[230, 128, 230, 0, second, 128, 230, 255]), MapRole::NormalOpenGl).unwrap());
        binding.roughness = Some(TextureMap::new(png(1, &[51, 51, 51, 255]), MapRole::Roughness).unwrap());
        binding.normal_strength = normal_strength;
        let mut input = request(2);
        input.textures = Some(Arc::new(RuntimeTextures::new(vec![binding]).unwrap().with_filtering(quality)));
        let options = JobOptions { texture_filtering: quality, ..Default::default() };
        let mut task = Task::new(context.clone(), sphere(white()), input, options, CancelToken::default()).unwrap();
        let mut params = vec![0; 32];
        scene::vector(&mut params, Vec3::new(1.0, 1.0, 0.0), 0.0);
        scene::vector(&mut params, Vec3::X * gradient, 0.0);
        scene::vector(&mut params, Vec3::Y * gradient, 0.0);
        scene::vector(&mut params, Vec3::Z, 0.0);
        context.queue.write_buffer(&task.uniform, 0, &params);
        let mut encoder = context.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &task.bind, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        context.queue.submit([encoder.finish()]);
        task.sample = 1;
        let actual = f64::from(task.read_linear().unwrap()[0][0]);
        assert!((actual - expected).abs() < 2e-5, "normal variance roughness {actual} versus {expected}");
    }
}

#[test]
fn gpu_finite_emitter_inside_solid_retains_entry_radiance_eta() {
    use solidcraft_render::job::{AreaLight, AreaLights, StudioLighting};
    let Some(executor) = executor() else { return };
    let surface = white();
    let receiver = ProductMesh {
        name: "shadow MIS receiver".into(),
        mesh: Arc::new(Mesh {
            positions: vec![Vec3::new(-0.01, -0.01, -2.0), Vec3::new(0.01, -0.01, -2.0), Vec3::new(0.01, 0.01, -2.0), Vec3::new(-0.01, 0.01, -2.0)],
            normals: vec![Vec3::Z; 4],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            tri_face: vec![0, 0],
            ..Default::default()
        }),
        color: [255; 3],
        opacity: 1.0,
        surface: Some(surface),
        transmission: None,
        faces: vec![],
    };
    let light = AreaLight { center: [0.0, 0.0, 0.0], u: [9.0, 0.0, 0.0], v: [0.0, -9.0, 0.0], radiance: [1.0; 3] };
    // Broad finite emitter makes edge/refraction disagreement small; the missing eta term is a 125% error.
    let mut expected = 0.0;
    for y in 0..128 {
        for x in 0..128 {
            let delta = Vec3::new(-9.0 + (f64::from(x) + 0.5) * 18.0 / 128.0, -9.0 + (f64::from(y) + 0.5) * 18.0 / 128.0, 2.0);
            let l = delta.normalized().unwrap();
            let r = solidcraft_render::transmission::fresnel(l.z, 1.0, 1.5).unwrap().reflectance();
            expected += (1.0 - r) / 2.25 * surface.brdf(Vec3::Z, Vec3::Z, l).unwrap().x * l.z * l.z / delta.len2() * 324.0 / 16384.0;
        }
    }
    for (thin, enabled, depth) in [(false, true, 1), (false, true, 4), (false, false, 4)] {
        let mut scene = glass_slab(thin);
        scene.meshes[0].surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap());
        scene.meshes[0].transmission =
            Some(solidcraft_render::transmission::Transmission::new(Vec3::ZERO, thin.then_some(0.0)).unwrap().with_shadow_transmission(enabled));
        scene.meshes.push(receiver.clone());
        let options = JobOptions {
            samples: 4096,
            max_depth: depth,
            seed: 37,
            lighting: Some(StudioLighting {
                punctual_lights: Default::default(),
                directional_lights: Default::default(),
                sun: None,
                environment: [0.0; 3],
                area_lights: AreaLights::new(&[light]).unwrap(),
            }),
            ..Default::default()
        };
        let mut image = request(1);
        image.camera.target = Vec3::new(0.0, 0.0, -2.0);
        image.camera.distance = 0.1;
        image.camera.fov = 0.001;
        let mut task = Task::new(executor.context.clone(), scene, image, options, CancelToken::default()).unwrap();
        for _ in 0..options.samples {
            task.complete_sample().unwrap();
        }
        let actual = f64::from(task.read_linear().unwrap()[0][0]);
        assert!((actual - expected).abs() < 0.08 * expected, "thin={thin}, enabled={enabled}, depth={depth}: {actual} vs {expected}");
    }
}

#[test]
fn path_device_limits_require_six_storage_buffers_without_changing_raster_defaults() {
    let mut supported = wgpu::Limits::downlevel_defaults();
    assert_eq!(supported.max_storage_buffers_per_shader_stage, 4);
    assert!(device_limits(supported.clone()).is_err());
    supported.max_storage_buffers_per_shader_stage = 6;
    assert_eq!(device_limits(supported.clone()).unwrap().max_storage_buffers_per_shader_stage, 6);
    supported.max_storage_buffers_per_shader_stage = 8;
    let requested = device_limits(supported.clone()).unwrap();
    let mut expected = wgpu::Limits::downlevel_defaults().using_resolution(supported);
    expected.max_storage_buffers_per_shader_stage = 7;
    assert_eq!(requested, expected);
}

#[test]
fn gpu_secondary_closed_corner_exits_are_not_skipped() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("closed corner boundary oracle"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("boundary_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    for case in 0..3 {
        let pyramid = case == 1;
        let mut scene = glass_slab(false);
        let (origin, direction) = if pyramid {
            let mut mesh = Mesh::default();
            let corners = [Vec3::new(-1., -1., 1.), Vec3::new(1., -1., 1.), Vec3::new(1., 1., 1.), Vec3::new(-1., 1., 1.)];
            let tip = Vec3::new(0., 0., -1.);
            let faces = [
                [corners[0], corners[1], corners[2]],
                [corners[0], corners[2], corners[3]],
                [corners[1], corners[0], tip],
                [corners[2], corners[1], tip],
                [corners[3], corners[2], tip],
                [corners[0], corners[3], tip],
            ];
            for face in faces {
                let n = (face[1] - face[0]).cross(face[2] - face[0]).normalized().unwrap();
                let first = mesh.positions.len() as u32;
                mesh.positions.extend(face);
                mesh.normals.extend([n; 3]);
                mesh.triangles.push([first, first + 1, first + 2]);
                mesh.tri_face.push(0);
            }
            scene.meshes[0].mesh = Arc::new(mesh);
            scene.bounds = scene.meshes[0].mesh.bounds();
            (Vec3::new(0.9999, 0., 2.), -Vec3::Z)
        } else {
            (Vec3::new(9.9999, if case == 2 { 9.9999 } else { 0. }, 2.), Vec3::new(1., 0., -1.).normalized().unwrap())
        };
        let mut task = Task::new(context.clone(), scene, request(2), JobOptions::default(), CancelToken::default()).unwrap();
        let mut params = vec![0; 32];
        scene::vector(&mut params, origin, 0.);
        scene::vector(&mut params, direction, 0.);
        scene::vector(&mut params, Vec3::ZERO, 0.);
        scene::vector(&mut params, -Vec3::Z, 0.);
        scene::vector(&mut params, Vec3::new(0.001, 0., 0.), 0.);
        context.queue.write_buffer(&task.uniform, 0, &params);
        let mut encoder = context.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &task.bind, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        context.queue.submit([encoder.finish()]);
        task.sample = 1;
        let values = task.read_linear().unwrap();
        let result = values[0];
        let expected_distance = if pyramid { 0.0002 } else { 0.0001 * 2.0_f32.sqrt() };
        assert!((values[1][0] - expected_distance).abs() < 3e-6, "case={case}: {:?}", values);
        assert_eq!(result, if pyramid { [0., 0., 1., 1.] } else { [0., 1., 1., 1.] }, "pyramid={pyramid}");
    }
}

#[test]
fn gpu_triangle_bound_away_rejects_rounded_behind_origin_hit() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("positive triangle oracle"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("positive_triangle_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    // Actual stored f32 tank-edge failure. Ray starts at the minimum x and moves
    // away, yet the projected distance numerator previously rounded positive.
    let vertices = [[218.38487_f32, -3.8715396, -86.], [220.27007, -2.9226143, 85.], [218.38487, -3.8715396, 85.]];
    let origin = [218.38487_f32, -3.871542, -67.5458];
    let direction = [-0.3121568_f32, -0.45083842, 0.8362433];
    for axis in 0..3 {
        for sign in [-1.0, 1.0] {
            let transform =
                |v: [f32; 3]| Vec3::new(f64::from(v[axis]) * sign, f64::from(v[(axis + 1) % 3]) * sign, f64::from(v[(axis + 2) % 3]) * sign);
            let points = vertices.map(transform);
            let d = transform(direction);
            let n = (points[1] - points[0]).cross(points[2] - points[0]).normalized().unwrap();
            let mesh = Mesh { positions: points.to_vec(), normals: vec![n; 3], triangles: vec![[0, 1, 2]], tri_face: vec![0], ..Default::default() };
            let mut scene = sphere_grid(white(), 2, 3);
            scene.bounds = mesh.bounds();
            scene.meshes[0].mesh = Arc::new(mesh);
            for positive in [false, true] {
                // Close positive intersections remain admitted; no epsilon is used.
                let o = if positive { (points[0] + points[1] + points[2]) / 3.0 - d * 0.001 } else { transform(origin) };
                let mut task = Task::new(context.clone(), scene.clone(), request(1), JobOptions::default(), CancelToken::default()).unwrap();
                let mut params = vec![0; 32];
                scene::vector(&mut params, o, 0.);
                scene::vector(&mut params, d, 0.);
                context.queue.write_buffer(&task.uniform, 0, &params);
                let mut encoder = context.device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&pipeline);
                    pass.set_bind_group(0, &task.bind, &[]);
                    pass.dispatch_workgroups(1, 1, 1);
                }
                context.queue.submit([encoder.finish()]);
                task.sample = 1;
                let values = task.read_linear().unwrap();
                assert_eq!(values[0][0], if positive { 1.0 } else { 0.0 }, "axis={axis} sign={sign} positive={positive}");
            }
        }
    }
}

#[test]
fn gpu_oblique_coplanar_grazing_reflection_does_not_rehit() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("coplanar boundary oracle"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("coplanar_boundary_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    let a = Vec3::new(f64::from(191.3_f32), f64::from(79.2_f32), f64::from(84.7_f32));
    let points = [a, a + Vec3::new(8., 0., 2.), a + Vec3::new(8., 8., 6.), a + Vec3::new(0., 8., 4.)];
    let n = Vec3::new(-0.25, -0.5, 1.).normalized().unwrap();
    let d = Vec3::new(f64::from(-0.69632584_f32), f64::from(0.69628024_f32), f64::from(0.17413844_f32));
    assert!(n.dot(d) > 0.0, "outward reflection");
    let mesh =
        Mesh { positions: points.to_vec(), normals: vec![n; 4], triangles: vec![[0, 1, 2], [0, 2, 3]], tri_face: vec![0, 0], ..Default::default() };
    let mut scene = sphere_grid(white(), 2, 3);
    scene.bounds = mesh.bounds();
    scene.meshes[0].mesh = Arc::new(mesh);
    let mut task = Task::new(context.clone(), scene, request(2), JobOptions::default(), CancelToken::default()).unwrap();
    let mut params = vec![0; 32];
    scene::vector(&mut params, Vec3::new(f64::from(1.734274e-6_f32), f64::from(0.23548841_f32), 0.), 0.);
    scene::vector(&mut params, d, 0.);
    context.queue.write_buffer(&task.uniform, 0, &params);
    let mut encoder = context.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &task.bind, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    context.queue.submit([encoder.finish()]);
    task.sample = 1;
    let values = task.read_linear().unwrap();
    // A source point on this exact stored plane and an outward ray cannot hit
    // its adjacent coplanar triangle at any strictly positive distance.
    assert_eq!(values[0][0], 0.0, "hit/distance={:?}, reconstructed={:?}", values[0], values[1]);
}

#[test]
fn gpu_shadow_closed_corner_keeps_short_exit_segment() {
    let Some(executor) = executor() else { return };
    let mut scene = glass_slab(false);
    let optics = scene.meshes[0].transmission.unwrap().with_shadow_transmission(true);
    scene.meshes[0].transmission = Some(optics);
    let cosine = std::f64::consts::FRAC_1_SQRT_2;
    let actual = shadow_probe(&executor.context, scene, Vec3::new(8.4999, 0., 2.), Vec3::new(cosine, 0., -cosine), 4.);
    let expected = solidcraft_render::transmission::solid_shadow(optics, 0.0001 * 2.0_f64.sqrt(), cosine, cosine, 1.5).unwrap();
    assert_eq!(actual[1][1], 2.0, "{actual:?}");
    for (a, b) in actual[0].iter().zip([expected.x, expected.y, expected.z]) {
        assert!((f64::from(*a) - b).abs() < 2e-5, "{actual:?} vs {expected:?}");
    }
}

#[test]
fn gpu_shadow_endpoint_near_interfaces_uses_true_distance() {
    let Some(executor) = executor() else { return };
    let mut scene = glass_slab(false);
    let optics = scene.meshes[0].transmission.unwrap().with_shadow_transmission(true);
    scene.meshes[0].transmission = Some(optics);
    // shadow_probe deliberately uses epsilon .01. Both offsets are smaller
    // than its old 2epsilon cutoff, and must still classify the actual endpoint.
    for (limit, length, factor, crossings) in
        [(1.495, 0.0, 1.0, 0.0), (1.505, 0.005, 0.96 / 2.25, 1.0), (2.495, 0.995, 0.96 / 2.25, 1.0), (2.505, 1.0, 0.96 * 0.96, 2.0)]
    {
        let actual = shadow_probe(&executor.context, scene.clone(), Vec3::new(0., 0., 2.), -Vec3::Z, limit);
        let expected = optics.attenuation(length).unwrap() * factor;
        assert_eq!(actual[1][1], crossings, "limit={limit}: {actual:?}");
        for (a, b) in actual[0].iter().zip([expected.x, expected.y, expected.z]) {
            assert!((f64::from(*a) - b).abs() < 2e-5, "limit={limit}: {actual:?} vs {expected:?}");
        }
    }
}

#[test]
fn gpu_long_ray_thin_corner_orders_entry_before_exit() {
    let Some(executor) = executor() else { return };
    let origin = Vec3::new(f64::from(f32::from_bits(0x43ad5626)), f64::from(f32::from_bits(0x44737912)), f64::from(f32::from_bits(0xc2dd2710)));
    let direction = Vec3::new(f64::from(f32::from_bits(0xbe341340)), f64::from(f32::from_bits(0xbf762fdb)), f64::from(f32::from_bits(0x3e577470)));
    // Captured stored-f32 tank side entry and top exit. Independent f64 plane
    // distances are 929.5224413721813 and 929.5225421330013 respectively.
    let side = [Vec3::new(243., 80., -86.), Vec3::new(157., 80., 85.), Vec3::new(243., 80., 85.)];
    let top = [Vec3::new(157., 80., 85.), Vec3::new(160., 77., 85.), Vec3::new(243., 80., 85.)];
    assert_entry_before_exit(&executor.context, origin, direction, side, top);
}

#[test]
fn gpu_camera_ray_sharp_rim_tie_orders_entry_before_exit() {
    let Some(executor) = executor() else { return };
    let origin = Vec3::new(f64::from(f32::from_bits(0xc409ad54)), f64::from(f32::from_bits(0xc45c5451)), f64::from(f32::from_bits(0x44160000)));
    let direction = Vec3::new(f64::from(f32::from_bits(0x3eac85fe)), f64::from(f32::from_bits(0x3f3d52db)), f64::from(f32::from_bits(0xbf152b54)));
    // Captured 64-spp coffee camera ray across the bin rim. Exact ray
    // parameters are 1124.0922606444 (entry) and 1124.0923198562 (exit):
    // closer than one f32 ULP, and both rounded to one scaled distance.
    let bottom = [Vec3::new(-152., -50., -55.), Vec3::new(-212., -50., -55.), Vec3::new(-182., -53., -55.)];
    let side = [Vec3::new(-152., -50., -55.), Vec3::new(-152., -50., -124.), Vec3::new(-212., -50., -55.)];
    assert_entry_before_exit(&executor.context, origin, direction, bottom, side);
}

#[test]
fn gpu_exact_rim_tie_prefers_entry_from_outside() {
    let Some(executor) = executor() else { return };
    // The ray crosses the shared rim edge itself: both faces report the same
    // exact depth, so only the medium-state tie preference can order them.
    let top = [Vec3::new(0., 0., 0.), Vec3::new(0.5, -1., 0.), Vec3::new(1., 0., 0.)];
    let side = [Vec3::new(0., 0., 0.), Vec3::new(1., 0., 0.), Vec3::new(0.5, 0., -1.)];
    assert_entry_before_exit(&executor.context, Vec3::new(0.25, -4., 4.), Vec3::new(0., 1., -1.), top, side);
}

fn assert_entry_before_exit(context: &Arc<Context>, origin: Vec3, direction: Vec3, entry: [Vec3; 3], exit: [Vec3; 3]) {
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("long-ray ordering oracle"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("ordering_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    for permutation in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
        let odd = (permutation[0] > permutation[1]) ^ (permutation[0] > permutation[2]) ^ (permutation[1] > permutation[2]);
        for reflection in 0..8_u32 {
            let sign = |axis: u32| if reflection & (1 << axis) == 0 { 1.0 } else { -1.0 };
            let transform = |v: Vec3| {
                let a = [v.x, v.y, v.z];
                Vec3::new(a[permutation[0]] * sign(0), a[permutation[1]] * sign(1), a[permutation[2]] * sign(2))
            };
            let invert = odd ^ (reflection.count_ones() % 2 != 0);
            for reverse in [false, true] {
                let mut mesh = Mesh::default();
                for face in if reverse { [exit, entry] } else { [entry, exit] } {
                    let mut face = face.map(transform);
                    if invert {
                        face.swap(1, 2);
                    }
                    let n = (face[1] - face[0]).cross(face[2] - face[0]).normalized().unwrap();
                    let i = mesh.positions.len() as u32;
                    mesh.positions.extend(face);
                    mesh.normals.extend([n; 3]);
                    mesh.triangles.push([i, i + 1, i + 2]);
                    mesh.tri_face.push(0);
                }
                let mut scene = sphere_grid(white(), 2, 3);
                scene.bounds = mesh.bounds();
                scene.meshes[0].mesh = Arc::new(mesh);
                let mut task = Task::new(context.clone(), scene, request(2), JobOptions::default(), CancelToken::default()).unwrap();
                let mut params = vec![0; 32];
                scene::vector(&mut params, transform(origin), 0.);
                scene::vector(&mut params, transform(direction), 0.);
                context.queue.write_buffer(&task.uniform, 0, &params);
                let mut encoder = context.device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&pipeline);
                    pass.set_bind_group(0, &task.bind, &[]);
                    pass.dispatch_workgroups(1, 1, 1);
                }
                context.queue.submit([encoder.finish()]);
                task.sample = 1;
                let values = task.read_linear().unwrap();
                assert_eq!(
                    values[0][0],
                    if reverse { 1.0 } else { 0.0 },
                    "permutation={permutation:?}, reflection={reflection}, reverse={reverse}, best/candidates={values:?}"
                );
                assert_eq!(values[1][1], 1.0, "candidate0 rejected: {values:?}");
                assert_eq!(values[2][1], 1.0, "candidate1 rejected: {values:?}");
            }
        }
    }
}
#[test]
fn gpu_far_rib_reentry_near_an_edge_is_not_lost_between_paths() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("rib re-entry oracle"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("continuation_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bits = |x: u32, y: u32, z: u32| Vec3::new(f64::from(f32::from_bits(x)), f64::from(f32::from_bits(y)), f64::from(f32::from_bits(z)));
    // Captured stored-f32 faces of the coffee bin's ribbed wall: the producing
    // face, its nearby neighbour, the far re-entry face sharing the neighbour's
    // edge, and the next exit. The ray leaves the producing face and truly
    // re-enters 2.615 mm later, inside the re-entry face but very close to the
    // shared edge; it then exits 17.529 mm along.
    let faces = [
        [bits(0x438a6e62, 0x4175ca91, 0xc2860000), bits(0x4389b018, 0x416145f8, 0x42c80000), bits(0x438a6e62, 0x4175ca91, 0x42c80000)],
        [bits(0x438b2463, 0x4185b9d0, 0xc2860000), bits(0x438a6e62, 0x4175ca91, 0x42c80000), bits(0x438b2463, 0x4185b9d0, 0x42c80000)],
        [bits(0x438b2463, 0x4185b9d0, 0xc2860000), bits(0x438b2463, 0x4185b9d0, 0x42c80000), bits(0x438bd1ab, 0x419119e4, 0xc2860000)],
        [bits(0x43906f8f, 0x41d4ebca, 0xc28e0000), bits(0x43906f8f, 0x41d4ebca, 0x42c80000), bits(0x438ff618, 0x41c5d6eb, 0xc28e0000)],
    ];
    let mut mesh = Mesh::default();
    for face in faces {
        let n = (face[1] - face[0]).cross(face[2] - face[0]).normalized().unwrap();
        let i = mesh.positions.len() as u32;
        mesh.positions.extend(face);
        mesh.normals.extend([n; 3]);
        mesh.triangles.push([i, i + 1, i + 2]);
        mesh.tri_face.push(0);
    }
    let mut scene = sphere_grid(white(), 2, 3);
    scene.bounds = mesh.bounds();
    scene.meshes[0].mesh = Arc::new(mesh);
    let mut task = Task::new(context.clone(), scene, request(2), JobOptions::default(), CancelToken::default()).unwrap();
    let mut params = vec![0; 32];
    scene::vector(&mut params, bits(0x438a6e62, 0x4175ca91, 0xc2860000), 0.);
    scene::vector(&mut params, bits(0xbedb21b8, 0xbebd05fd, 0x4295df13), 0.);
    scene::vector(&mut params, bits(0x3f3519ed, 0x3f28acac, 0xbe82f446), 0.);
    context.queue.write_buffer(&task.uniform, 0, &params);
    let mut encoder = context.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &task.bind, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    context.queue.submit([encoder.finish()]);
    task.sample = 1;
    let values = task.read_linear().unwrap();
    assert_eq!(values[0][0], 2.0, "hit/distance={:?}", values[0]);
    assert!((f64::from(values[0][1]) - 2.615).abs() < 0.01, "hit/distance={:?}", values[0]);
}
#[test]
fn gpu_entry_at_a_shared_edge_does_not_reenter_the_adjacent_face() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("edge entry oracle"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("continuation_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bits = |x: u32, y: u32, z: u32| Vec3::new(f64::from(f32::from_bits(x)), f64::from(f32::from_bits(y)), f64::from(f32::from_bits(z)));
    // Captured tumbler faces: a refracted ray entered the glass through the first
    // face 8e-9 (barycentric) from its edge with the second. Its reconstructed
    // start is 6.5e-9 mm outside both planes, so it met the second face from
    // outside 1.1e-8 mm later: an "entry" while already inside. A cone to an
    // apex behind both faces closes the captured pair into valid closed glass.
    let a = bits(0x43050000, 0x42095be6, 0x42140000);
    let b = bits(0x4304aef7, 0x4218aea0, 0x41b80000);
    let c = bits(0x4303bd40, 0x4205b1dd, 0x41b80000);
    let e = bits(0x4305c374, 0x421b114d, 0x42140000);
    let f = Vec3::new(140.75, 34.75, 29.5);
    let faces = [[c, a, b], [e, b, a], [a, c, f], [e, a, f], [b, e, f], [c, b, f]];
    let mut mesh = Mesh::default();
    for face in faces {
        let n = (face[1] - face[0]).cross(face[2] - face[0]).normalized().unwrap();
        let i = mesh.positions.len() as u32;
        mesh.positions.extend(face);
        mesh.normals.extend([n; 3]);
        mesh.triangles.push([i, i + 1, i + 2]);
        mesh.tri_face.push(0);
    }
    let mut scene = glass_slab(false);
    scene.bounds = mesh.bounds();
    scene.meshes[0].mesh = Arc::new(mesh);
    let mut task = Task::new(context.clone(), scene, request(2), JobOptions::default(), CancelToken::default()).unwrap();
    let mut params = vec![0; 32];
    scene::vector(&mut params, bits(0x43050000, 0x42095be6, 0x42140000), 1.);
    scene::vector(&mut params, bits(0xbda74978, 0x3f7d0fd4, 0xc06735c3), 0.);
    scene::vector(&mut params, bits(0x3f4ff9cf, 0x3f0c3c0f, 0xbe4c997d), 0.);
    context.queue.write_buffer(&task.uniform, 0, &params);
    let mut encoder = context.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &task.bind, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    context.queue.submit([encoder.finish()]);
    task.sample = 1;
    let values = task.read_linear().unwrap();
    // The ray must leave through one of the closing faces, at a real distance.
    assert!(values[0][0] >= 2.0 && values[0][0] < 6.0, "hit/distance={:?}", values[0]);
    assert!(values[0][1] > 1e-3, "hit/distance={:?}", values[0]);
}
#[test]
fn gpu_long_ray_near_corner_refraction_keeps_real_exit() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("refracted corner oracle"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("refracted_corner_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    let origin = Vec3::new(f64::from(f32::from_bits(0x43ad5626)), f64::from(f32::from_bits(0x44737912)), f64::from(f32::from_bits(0xc2dd2710)));
    let direction = Vec3::new(f64::from(f32::from_bits(0xbe341340)), f64::from(f32::from_bits(0xbf762fdb)), f64::from(f32::from_bits(0x3e577470)));
    let ior = f64::from(1.586_f32);
    let ni = direction.x / ior;
    let nk = direction.z / ior;
    let refracted = Vec3::new(ni, -(1.0 - ni * ni - nk * nk).sqrt(), nk);
    let entry_distance = (80.0 - origin.y) / direction.y;
    let entry_z = origin.z + direction.z * entry_distance;
    let exit_distance = (85.0 - entry_z) / refracted.z;
    // Independent f64 plane/refraction oracle: true entry clearance is ~21um.
    assert!(85.0 - entry_z > 0.00002 && 85.0 - entry_z < 0.000022);
    let faces = [
        [[157., 0., 85.], [243., 0., 85.], [243., 80., 85.], [157., 80., 85.]],
        [[157., 80., -86.], [243., 80., -86.], [243., 0., -86.], [157., 0., -86.]],
        [[243., 80., -86.], [157., 80., -86.], [157., 80., 85.], [243., 80., 85.]],
        [[157., 0., -86.], [243., 0., -86.], [243., 0., 85.], [157., 0., 85.]],
        [[243., 0., -86.], [243., 80., -86.], [243., 80., 85.], [243., 0., 85.]],
        [[157., 80., -86.], [157., 0., -86.], [157., 0., 85.], [157., 80., 85.]],
    ];
    for permutation in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
        let odd = (permutation[0] > permutation[1]) ^ (permutation[0] > permutation[2]) ^ (permutation[1] > permutation[2]);
        for reflection in 0..8_u32 {
            let sign = |axis: u32| if reflection & (1 << axis) == 0 { 1.0 } else { -1.0 };
            let transform = |v: Vec3| {
                let a = [v.x, v.y, v.z];
                Vec3::new(a[permutation[0]] * sign(0), a[permutation[1]] * sign(1), a[permutation[2]] * sign(2))
            };
            let invert = odd ^ (reflection.count_ones() % 2 != 0);
            let mut mesh = Mesh::default();
            for face in faces {
                for indices in [[0, 1, 2], [0, 2, 3]] {
                    let mut p = indices.map(|i| {
                        let a = face[i];
                        transform(Vec3::new(a[0], a[1], a[2]))
                    });
                    if invert {
                        p.swap(1, 2);
                    }
                    let n = (p[1] - p[0]).cross(p[2] - p[0]).normalized().unwrap();
                    let i = mesh.positions.len() as u32;
                    mesh.positions.extend(p);
                    mesh.normals.extend([n; 3]);
                    mesh.triangles.push([i, i + 1, i + 2]);
                    mesh.tri_face.push(0);
                }
            }
            let mut scene = glass_slab(false);
            scene.bounds = mesh.bounds();
            scene.meshes[0].mesh = Arc::new(mesh);
            scene.meshes[0].surface = Some(OpaqueSurface::new(Vec3::ZERO, 0., 0.1, ior).unwrap());
            scene.meshes[0].transmission = Some(solidcraft_render::transmission::Transmission::new(Vec3::ZERO, None).unwrap());
            let mut task = Task::new(context.clone(), scene, request(2), JobOptions::default(), CancelToken::default()).unwrap();
            let mut params = vec![0; 32];
            scene::vector(&mut params, transform(origin), 0.);
            scene::vector(&mut params, transform(direction), 0.);
            scene::vector(&mut params, Vec3::ZERO, 0.);
            scene::vector(&mut params, Vec3::ZERO, 0.);
            scene::vector(&mut params, Vec3::new(0.00084, 0., 0.), 0.);
            context.queue.write_buffer(&task.uniform, 0, &params);
            let mut encoder = context.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &task.bind, &[]);
                pass.dispatch_workgroups(1, 1, 1);
            }
            context.queue.submit([encoder.finish()]);
            task.sample = 1;
            let values = task.read_linear().unwrap();
            assert_eq!(values[0], [1.; 4], "permutation={permutation:?} reflection={reflection}: {values:?}");
            let z_axis = permutation.iter().position(|axis| *axis == 2).unwrap();
            assert!(values[2][z_axis] < 85.0, "rounded entry onto top edge: {values:?}");
            // Two position ULP at z85, propagated through the refracted angle.
            let tolerance = 2.0 * f64::from(f32::from_bits(85.0_f32.to_bits() + 1) - 85.0) / refracted.z;
            assert!(
                (f64::from(values[1][0]) - exit_distance).abs() < tolerance,
                "distance={:?} expected={exit_distance} tolerance={tolerance}",
                values[1]
            );
            let expected = transform(refracted);
            for (a, b) in values[3].iter().zip([expected.x, expected.y, expected.z]) {
                assert!((f64::from(*a) - 2.0 - b).abs() < 2e-6);
            }
        }
    }
}
#[test]
fn gpu_rib_total_internal_reflection_retains_origin_residual() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("rib residual oracle"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("rib_residual_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    let a = Vec3::new(-162., -53., -127.);
    let b = Vec3::new(f64::from(-163.7_f32), -55., -55.);
    let c = Vec3::new(f64::from(-163.7_f32), -55., -127.);
    let other = Vec3::new(-165.5, -53., -55.);
    let bary = Vec3::new(f64::from(f32::from_bits(0x369bafd4)), f64::from(f32::from_bits(0x3f59c40b)), f64::from(f32::from_bits(0x3e18ee9d)));
    let incoming = Vec3::new(f64::from(f32::from_bits(0xbeba12cc)), f64::from(f32::from_bits(0xbf6e1a19)), f64::from(f32::from_bits(0xbd5b2b1e)));
    // These are captured GPU barycentrics, not an idealized primary-ray hit.
    let point = b + (a - b) * bary.x + (c - b) * bary.z;
    let normal = (b - a).cross(c - a).normalized().unwrap();
    let outgoing = incoming - normal * (2.0 * incoming.dot(normal));
    let exit_normal = (other - b).cross(c - b).normalized().unwrap();
    let expected = -(point - b).dot(exit_normal) / outgoing.dot(exit_normal);
    assert!(expected > 1e-5 && expected < 1.5e-5);
    let cross_section = [[-162., -53.], [f64::from(-163.7_f32), -55.], [-165.5, -53.]];
    let mut faces = Vec::new();
    let lower = cross_section.map(|p| Vec3::new(p[0], p[1], -127.));
    let upper = cross_section.map(|p| Vec3::new(p[0], p[1], -55.));
    faces.push(lower);
    faces.push([upper[0], upper[2], upper[1]]);
    for i in 0..3 {
        let j = (i + 1) % 3;
        faces.push([lower[i], upper[i], upper[j]]);
        faces.push([lower[i], upper[j], lower[j]]);
    }
    for permutation in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
        let odd = (permutation[0] > permutation[1]) ^ (permutation[0] > permutation[2]) ^ (permutation[1] > permutation[2]);
        for reflection in 0..8_u32 {
            let sign = |axis: u32| if reflection & (1 << axis) == 0 { 1.0 } else { -1.0 };
            let transform = |v: Vec3| {
                let a = [v.x, v.y, v.z];
                Vec3::new(a[permutation[0]] * sign(0), a[permutation[1]] * sign(1), a[permutation[2]] * sign(2))
            };
            let invert = odd ^ (reflection.count_ones() % 2 != 0);
            let mut mesh = Mesh::default();
            for face in &faces {
                let mut p = face.map(transform);
                if invert {
                    p.swap(1, 2);
                }
                let n = (p[1] - p[0]).cross(p[2] - p[0]).normalized().unwrap();
                let i = mesh.positions.len() as u32;
                mesh.positions.extend(p);
                mesh.normals.extend([n; 3]);
                mesh.triangles.push([i, i + 1, i + 2]);
                mesh.tri_face.push(0);
            }
            let mut scene = glass_slab(false);
            scene.bounds = mesh.bounds();
            scene.meshes[0].mesh = Arc::new(mesh);
            scene.meshes[0].surface = Some(OpaqueSurface::new(Vec3::ZERO, 0., 0.1, f64::from(1.49_f32)).unwrap());
            scene.meshes[0].transmission = Some(solidcraft_render::transmission::Transmission::new(Vec3::ZERO, None).unwrap());
            let mut task = Task::new(context.clone(), scene, request(2), JobOptions::default(), CancelToken::default()).unwrap();
            let mut params = vec![0; 32];
            scene::vector(&mut params, transform(a), 0.);
            scene::vector(&mut params, transform(if invert { c } else { b }), 0.);
            scene::vector(&mut params, transform(if invert { b } else { c }), 0.);
            scene::vector(&mut params, transform(incoming), 0.);
            scene::vector(&mut params, Vec3::new(0.00084, 0., 0.), 0.);
            scene::vector(&mut params, if invert { Vec3::new(bary.x, bary.z, bary.y) } else { bary }, 0.);
            context.queue.write_buffer(&task.uniform, 0, &params);
            let mut encoder = context.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &task.bind, &[]);
                pass.dispatch_workgroups(1, 1, 1);
            }
            context.queue.submit([encoder.finish()]);
            task.sample = 1;
            let values = task.read_linear().unwrap();
            assert_eq!(values[0], [0., 1., 1., 1.], "permutation={permutation:?} reflection={reflection}: {values:?}");
            assert_eq!(values[1][1], 1.0, "must be TIR: {values:?}");
            assert!((f64::from(values[1][0]) - expected).abs() < 2e-7, "expected={expected}: {values:?}");
        }
    }
}
#[test]
fn gpu_split_products_reconstruct_exact_products_and_near_determinants() {
    use wgpu::util::DeviceExt;
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let unit = 0x3f800000u32;
    let c = unit | (13_264_529 - (1 << 23));
    let d = unit | (2351 * 4513 - (1 << 23));
    let mut cases =
        vec![[0x40000000, unit, c, d], [c, d, 0x40000000, unit], [unit, 0x40000000, 0x40000000, unit], [unit + 1, unit + 1, unit, unit + 2]];
    let mut state = 0x11c731u32;
    let mut next = || {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        state
    };
    while cases.len() < 4096 {
        let a = f32::from_bits(((110 + next() % 41) << 23) | (next() & 0x7fffff));
        let b = f32::from_bits(((110 + next() % 41) << 23) | (next() & 0x7fffff));
        let c = f32::from_bits(((110 + next() % 41) << 23) | (next() & 0x7fffff));
        let d = (f64::from(a) * f64::from(b) / f64::from(c)) as f32;
        cases.push([a.to_bits(), b.to_bits(), c.to_bits(), d.to_bits()]);
    }
    for [a, b, c, d] in cases.clone() {
        cases.push([a ^ 0x80000000, b, c ^ 0x80000000, d]);
    }
    assert_eq!(cases.len(), 8192);
    let mut bytes = Vec::with_capacity(4 + cases.len() * 16);
    bytes.extend((cases.len() as u32).to_le_bytes());
    for case in &cases {
        for word in case {
            bytes.extend(word.to_le_bytes());
        }
    }
    let input = context.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("split determinant inputs"),
        contents: &bytes,
        usage: wgpu::BufferUsages::STORAGE,
    });
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("split determinant oracle"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &shader,
        entry_point: Some("split_predicate_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    let mut task = Task::new(context.clone(), glass_slab(false), request(128), JobOptions::default(), CancelToken::default()).unwrap();
    context.queue.write_buffer(&task.uniform, 0, &task.parameters(0).unwrap());
    let bind = context.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("split determinant oracle"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: task.uniform.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: input.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: task.accumulation.as_entire_binding() },
        ],
    });
    let mut encoder = context.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups((cases.len() as u32).div_ceil(32), 1, 1);
    }
    context.queue.submit([encoder.finish()]);
    task.sample = 1;
    let values = task.read_linear().unwrap();
    let word = |low: f32, high: f32| low as u32 | ((high as u32) << 16);
    let value = |bits| f64::from(f32::from_bits(bits));
    let normalized = |bits| value((bits & 0x807fffff) | 0x3f800000);
    for (index, ([a, b, c, d], output)) in cases.iter().zip(values.as_chunks::<2>().0).enumerate() {
        let product = value(word(output[0][0], output[0][1]));
        let error = value(word(output[0][2], output[0][3]));
        assert_eq!(product + error, normalized(*a) * normalized(*b), "exact split product index={index}");
        let expected = (value(*a) * value(*b) - value(*c) * value(*d)) as f32;
        for got in [word(output[1][0], output[1][1]), word(output[1][2], output[1][3])] {
            if expected == 0.0 {
                assert_eq!(f32::from_bits(got), 0.0, "zero determinant index={index}");
            } else {
                assert_eq!(got, expected.to_bits(), "split determinant index={index}, operands={a:08x}/{b:08x}/{c:08x}/{d:08x}");
            }
        }
    }
}

#[test]
fn gpu_integer_binary32_add_sub_matches_hardware_bits() {
    use wgpu::util::DeviceExt;
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let mut pairs = Vec::new();
    let fractions = [0, 1, 0x3fffff, 0x400000, 0x400001, 0x7ffffe, 0x7fffff];
    for exponent in [0u32, 1, 2, 23, 24, 25, 31, 32, 63, 126, 127, 128, 253, 254, 255] {
        for gap in [0, 1, 23, 24, 25, 31, 32, 63, 254] {
            for a_fraction in fractions {
                for b_fraction in fractions {
                    let a = exponent << 23 | a_fraction;
                    let b = exponent.saturating_sub(gap) << 23 | b_fraction;
                    pairs.extend([[a, b], [a ^ 0x80000000, b], [b, a], [b, a ^ 0x80000000]]);
                }
            }
        }
    }
    pairs.extend([[0x80000000, 0x80000000], [0, 0x80000000], [1, 0x80000001]]);
    let mut state = 0x9e3779b9u32;
    while pairs.len() < 65536 {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        let a = state;
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        pairs.push([a, state]);
    }
    assert_eq!(pairs.len(), 65536);
    let mut bytes = Vec::with_capacity(4 + pairs.len() * 8);
    bytes.extend((pairs.len() as u32).to_le_bytes());
    for pair in &pairs {
        for word in pair {
            bytes.extend(word.to_le_bytes());
        }
    }
    let input = context.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("integer binary32 inputs"),
        contents: &bytes,
        usage: wgpu::BufferUsages::STORAGE,
    });
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("integer binary32 oracle"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &shader,
        entry_point: Some("integer_float_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    let mut task = Task::new(context.clone(), glass_slab(false), request(256), JobOptions::default(), CancelToken::default()).unwrap();
    let bind = context.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("integer binary32 oracle"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 1, resource: input.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: task.accumulation.as_entire_binding() },
        ],
    });
    let mut encoder = context.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(1024, 1, 1);
    }
    context.queue.submit([encoder.finish()]);
    task.sample = 1;
    let values = task.read_linear().unwrap();
    let bits = |value: f32| if value.is_nan() { 0x7fc00000 } else { value.to_bits() };
    for (i, ([a, b], value)) in pairs.iter().zip(values).enumerate() {
        let a_float = f32::from_bits(*a);
        let b_float = f32::from_bits(*b);
        let sum = value[0] as u32 | (value[1] as u32) << 16;
        let difference = value[2] as u32 | (value[3] as u32) << 16;
        assert_eq!(sum, bits(a_float + b_float), "sum index={i} a={a:08x} b={b:08x}");
        assert_eq!(difference, bits(a_float - b_float), "difference index={i} a={a:08x} b={b:08x}");
    }
}
#[test]
fn gpu_emitter_short_segment_beer_and_area_pdf() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("short emitter segment oracle"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("emitter_segment_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    let mut task = Task::new(context.clone(), glass_slab(false), request(1), JobOptions::default(), CancelToken::default()).unwrap();
    let mut encoder = context.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &task.bind, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    context.queue.submit([encoder.finish()]);
    task.sample = 1;
    let result = task.read_linear().unwrap()[0];
    let ray = Vec3::new(f64::from(0.6f32), 0., f64::from(0.8f32));
    let distance = f64::from(0.00001f32);
    let length = ray.len() * distance;
    let cosine = ray.z / ray.len();
    let expected = [length, (-5000. * length).exp(), length * length / (16. * cosine)];
    for i in 0..3 {
        assert!((f64::from(result[i]) / expected[i] - 1.).abs() < 2e-6, "channel={i} expected={expected:?} result={result:?}");
    }
}
#[test]
fn gpu_integer_relative_fast_paths_match_ordered_rounding() {
    use wgpu::util::DeviceExt;
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let mut quads = Vec::new();
    let fractions = [0, 1, 0x3fffff, 0x400000, 0x400001, 0x7ffffe, 0x7fffff];
    for exponent in [0u32, 1, 2, 23, 24, 25, 126, 127, 128, 253, 254, 255] {
        for fraction in fractions {
            let a = exponent << 23 | fraction;
            for b in [a, a ^ 1, a.wrapping_add(0x800000), a ^ 0x80000000, 0, 0x80000000] {
                for offset in [0, 0x80000000, 1, 0x80000001, 0x3f800000, 0xbf800000, a, b] {
                    quads.extend([[a, 0, b, offset], [a, offset, b, 0], [a, offset, a, b], [a, b, b, a]]);
                }
            }
        }
    }
    let mut state = 0x12345678u32;
    while quads.len() < 65536 {
        let mut next = || {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            state
        };
        let mut quad = [next(), next(), next(), next()];
        match quads.len() % 5 {
            0 => quad[2] = quad[0],
            1 => {
                quad[1] = 0;
                quad[3] = 0;
            }
            2 => quad[1] = 0,
            3 => {
                quad[0] = 0x3f800000 | (quad[0] & 0x7fffff);
                quad[2] = quad[0] ^ 1;
            }
            _ => {}
        }
        quads.push(quad);
    }
    assert_eq!(quads.len(), 65536);
    let mut bytes = Vec::with_capacity(4 + quads.len() * 16);
    bytes.extend((quads.len() as u32).to_le_bytes());
    for quad in &quads {
        for word in quad {
            bytes.extend(word.to_le_bytes());
        }
    }
    let input = context.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("relative coordinate inputs"),
        contents: &bytes,
        usage: wgpu::BufferUsages::STORAGE,
    });
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("relative coordinate oracle"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &shader,
        entry_point: Some("integer_relative_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    let mut task = Task::new(context.clone(), glass_slab(false), request(256), JobOptions::default(), CancelToken::default()).unwrap();
    let bind = context.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("relative coordinate oracle"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 1, resource: input.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: task.accumulation.as_entire_binding() },
        ],
    });
    let mut encoder = context.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(1024, 1, 1);
    }
    context.queue.submit([encoder.finish()]);
    task.sample = 1;
    let values = task.read_linear().unwrap();
    for (i, (quad, value)) in quads.iter().zip(values).enumerate() {
        let reference = value[0] as u32 | (value[1] as u32) << 16;
        let candidate = value[2] as u32 | (value[3] as u32) << 16;
        let [a, al, b, bl] = quad.map(f32::from_bits);
        let anchor = a - b;
        let offset = al - bl;
        let result = anchor + offset;
        let expected = if result.is_nan() { 0x7fc00000 } else { result.to_bits() };
        assert_eq!(reference, expected, "reference index={i} input={quad:08x?}");
        assert_eq!(candidate, expected, "candidate index={i} input={quad:08x?}");
    }
}

#[test]
fn medium_failures_are_tolerated_only_when_rare() {
    // One failure in one path (a camera inside glass) is never tolerated.
    assert!(!medium_failures_tolerated(1, 1));
    assert!(medium_failures_tolerated(0, 1));
    assert!(medium_failures_tolerated(1, 1_000_000));
    assert!(!medium_failures_tolerated(2, 1_000_000));
    // 1080p at 256 spp tolerates up to 530 isolated paths.
    assert!(medium_failures_tolerated(530, 1920 * 1080 * 256));
    assert!(!medium_failures_tolerated(531, 1920 * 1080 * 256));
    assert!(!medium_failures_tolerated(1_u64 << 32, 1_u64 << 32));
}

#[test]
fn identical_maps_pack_once_for_multiple_gpu_bodies() {
    use solidcraft_render::job::textures::{PlanarFrame, RuntimeTextures};
    let fixture = texture_fixture(PlanarFrame { origin: [0.0; 3], u: [2.0, 0.0, 0.0], v: [0.0, 2.0, 0.0] });
    let first = fixture.bindings()[0].clone();
    let mut second = first.clone();
    second.body_index = 1;
    second.body_name = "Second body".into();
    let input = RuntimeTextures::new(vec![first, second]).unwrap();
    let bytes = textures::flatten(Some(&input), 2, Vec3::ZERO, &CancelToken::default()).unwrap();
    let word = |i: usize| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()) as usize;
    let a = word(5);
    let b = word(6);
    assert_ne!(a, b, "bindings keep separate coordinate records");
    assert_eq!(word(a + 3), word(b + 3), "one color payload");
    assert_eq!(word(a + 12), word(b + 12), "one normal payload");
    assert_eq!(textures::packed_bytes(&input, 2), bytes.len() as u64, "admission does not charge unused mip tails or duplicate legacy maps");
    let quality = input.with_filtering(true);
    let bytes = textures::flatten(Some(&quality), 2, Vec3::ZERO, &CancelToken::default()).unwrap();
    assert_eq!(textures::packed_bytes(&quality, 2), bytes.len() as u64);
    let word = |i: usize| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()) as usize;
    let descriptor = word(word(5) + 3);
    assert_eq!(descriptor, word(word(6) + 3), "quality storage also deduplicates by content, role and policy");
    assert_eq!(word(descriptor), 2, "quality mode adds the full mip chain to legacy maps");
    assert_eq!(word(descriptor + 2), 0, "quality does not change legacy scalar-face precedence");
    let base = word(descriptor + 4);
    assert_eq!(word(descriptor + 7), 1, "sRGB8 base format");
    assert_eq!(word(descriptor + 11), 3, "linear RGBA16_UNORM tail format");
    assert_eq!(&bytes[base * 4..base * 4 + 16], &[128, 64, 255, 0, 255, 0, 32, 255, 64, 128, 192, 0, 0, 255, 128, 255]);
}

#[test]
fn packed_legacy_and_portable_gpu_maps_keep_distinct_policies_in_both_orders() {
    use image::ImageEncoder;
    use solidcraft_render::{
        job::textures::{PlanarFrame, RuntimeTextures, TextureMap},
        texture_input::MapRole,
    };
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded).write_image(&[128, 64, 32, 255].repeat(15), 5, 3, image::ExtendedColorType::Rgba8).unwrap();
    let legacy = TextureMap::new(encoded.clone(), MapRole::BaseColor).unwrap();
    let portable = TextureMap::new_portable(encoded, MapRole::BaseColor).unwrap();
    let original = texture_fixture(PlanarFrame { origin: [0.; 3], u: [1., 0., 0.], v: [0., 1., 0.] });
    for maps in [[legacy.clone(), portable.clone()], [portable.clone(), legacy.clone()]] {
        let bindings = maps
            .iter()
            .enumerate()
            .map(|(index, map)| {
                let mut binding = original.bindings()[0].clone();
                binding.body_index = index;
                binding.body_name = format!("Policy {index}");
                binding.base_color = Some(map.clone());
                binding.normal = None;
                binding
            })
            .collect();
        let input = RuntimeTextures::new(bindings).unwrap();
        let bytes = textures::flatten(Some(&input), 2, Vec3::ZERO, &CancelToken::default()).unwrap();
        let word = |i: usize| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()) as usize;
        let descriptors = [word(word(5) + 3), word(word(6) + 3)];
        assert_ne!(descriptors[0], descriptors[1], "same bytes and role retain separate filtering policies");
        // Header + two body records, then exactly one legacy base and one 5x3 -> 2x1 -> 1x1 chain.
        assert_eq!(bytes.len(), 8 * 4 + 2 * 24 * 4 + (8 * 4 + 15 * 16) + (16 * 4 + 18 * 16));
        assert_eq!(textures::packed_bytes(&input, 2), bytes.len() as u64);
        for (descriptor, map) in descriptors.into_iter().zip(maps) {
            assert_eq!(word(descriptor), if map.gpu_mip_filtering() { 3 } else { 1 });
            assert_eq!(word(descriptor + 2), usize::from(map.gpu_mip_filtering()));
            let base = word(descriptor + 4) * 4;
            let expected: Vec<_> = map.decode(&CancelToken::default()).unwrap().iter().flat_map(|value| value.to_le_bytes()).collect();
            assert_eq!(&bytes[base..base + expected.len()], expected, "level-zero encoded-role interpretation stays exact");
        }
    }
}

#[test]
fn packed_gpu_mips_preserve_odd_size_linear_color_and_data_means() {
    use image::ImageEncoder;
    use solidcraft_render::{
        job::textures::{PlanarFrame, RuntimeTextures, TextureMap},
        texture_input::MapRole,
    };
    let mut encoded = Vec::new();
    let pixels: Vec<u8> = (0..15).flat_map(|i| [if i & 1 == 0 { 64 } else { 192 }, 128, 255, 0]).collect();
    image::codecs::png::PngEncoder::new(&mut encoded).write_image(&pixels, 5, 3, image::ExtendedColorType::Rgba8).unwrap();
    let original = texture_fixture(PlanarFrame { origin: [0.; 3], u: [1., 0., 0.], v: [0., 1., 0.] });
    let mut binding = original.bindings()[0].clone();
    binding.base_color = Some(TextureMap::new_portable(encoded.clone(), MapRole::BaseColor).unwrap());
    binding.normal = None;
    binding.roughness = Some(TextureMap::new_portable(encoded, MapRole::Roughness).unwrap());
    let input = RuntimeTextures::new(vec![binding]).unwrap();
    let bytes = textures::flatten(Some(&input), 1, Vec3::ZERO, &CancelToken::default()).unwrap();
    let word = |i: usize| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()) as usize;
    let record = word(5);
    let color = word(record + 3);
    let data = word(record + 16);
    assert_ne!(color, data, "the same encoded source retains distinct role interpretation");
    for (descriptor, linear) in [(color, true), (data, false)] {
        assert_eq!(word(descriptor), 3, "5x3 -> 2x1 -> 1x1");
        let last = descriptor + 4 + 2 * 4;
        assert_eq!((word(last + 1), word(last + 2)), (1, 1));
        let expected = (0..15)
            .map(|i| {
                let value = if i & 1 == 0 { 64 } else { 192 };
                if linear { solidcraft_viewport::surface::srgb8_to_linear(value) } else { f64::from(value) / 255. }
            })
            .sum::<f64>()
            / 15.;
        let actual = f32::from_le_bytes(bytes[word(last) * 4..word(last) * 4 + 4].try_into().unwrap());
        assert!((f64::from(actual) - expected).abs() < 1e-7, "{actual} vs {expected}");
    }
    assert!((bytes.capacity() as u64) < input.working_bytes(), "the admitted preparation includes the full packed chain capacity");
}

#[test]
fn gpu_trilinear_minification_matches_singular_value_lod_and_repeat_oracle() {
    use solidcraft_render::{
        job::textures::PlanarFrame,
        texture_lod::select_isotropic_lod,
        texture_sampling::{
            TextureView,
            mips::{MAX_MIP_TAIL_BYTES, MipChain},
        },
    };
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("texture_mip_reference"),
        compilation_options: wgpu::PipelineCompilationOptions { constants: &[("HAS_TEXTURES", 1.0)], ..Default::default() },
        cache: None,
    });
    let frame = PlanarFrame { origin: [0.; 3], u: [2., 0., 0.], v: [0., 2., 0.] };
    let source = texture_fixture_filtering(frame, true);
    let pixels = source.bindings()[0].base_color.as_ref().unwrap().decode(&CancelToken::default()).unwrap();
    let base = TextureView::new(&pixels, [2, 2]).unwrap();
    let chain = MipChain::prepare(base, MAX_MIP_TAIL_BYTES, || false).unwrap();
    for portable in [true, false] {
        let source = texture_fixture_filtering(frame, portable);
        for (point, dx, dy) in [
            (Vec3::new(0.5, 0.5, 0.), [0., 0.], [0., 0.]),
            (Vec3::new(-0.01, 0.125, 0.), [0.75, 0.], [0., 0.25]),
            (Vec3::new(0.01, 0.125, 0.), [0.75, 0.], [0., 0.25]),
            (Vec3::new(-0.25, 1.125, 0.), [0.4, -0.5], [0.5, 0.25]),
            (Vec3::new(0., 0., 0.), [4., 0.], [0., 4.]),
            // Legacy sampling must bypass LOD arithmetic even when a finite gradient
            // would overflow its texel-space multiplication. This is not a portable-footprint case.
            (Vec3::new(0.5, 0.5, 0.), [3e38, 0.], [0., 3e38]),
        ] {
            let extreme = dx[0] > 1e30;
            if portable && extreme {
                continue;
            }
            let mut input = request(2);
            input.textures = Some(source.clone());
            let mut task = Task::new(context.clone(), sphere(white()), input, JobOptions::default(), CancelToken::default()).unwrap();
            let mut params = vec![0; 32];
            scene::vector(&mut params, point, 0.);
            scene::vector(&mut params, Vec3::new(dx[0], dx[1], 0.), 0.);
            scene::vector(&mut params, Vec3::new(dy[0], dy[1], 0.), 0.);
            scene::vector(&mut params, Vec3::Z, 0.);
            context.queue.write_buffer(&task.uniform, 0, &params);
            let mut encoder = context.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &task.bind, &[]);
                pass.dispatch_workgroups(1, 1, 1);
            }
            context.queue.submit([encoder.finish()]);
            task.sample = 1;
            let actual = task.read_linear().unwrap();
            let lod = if portable { select_isotropic_lod([2, 2], dx, dy).unwrap().lod } else { 0. };
            let expected = if portable {
                chain.sample_repeat_lod_opaque(frame.uv(point).unwrap(), lod).unwrap()
            } else {
                base.sample_repeat_opaque(frame.uv(point).unwrap()).unwrap()
            };
            assert!((f64::from(actual[1][0]) - lod).abs() < 1e-6, "GPU LOD {actual:?} vs {lod}");
            for (a, b) in actual[0].iter().zip(expected) {
                assert!((f64::from(*a) - b).abs() < 2e-6, "GPU portable={portable} {actual:?} vs {expected:?}");
            }
        }
    }
}

#[test]
fn gpu_scalar_faces_preserve_legacy_roles_and_suppress_only_portable_roles() {
    use image::ImageEncoder;
    use solidcraft_render::{
        FaceAppearance,
        job::textures::{PlanarFrame, RuntimeTextures, TextureMap, sample_map},
        texture_input::MapRole,
    };
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = |entry_point| {
        context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: Some(&layout),
            module: &shader,
            entry_point: Some(entry_point),
            compilation_options: wgpu::PipelineCompilationOptions { constants: &[("HAS_TEXTURES", 1.0)], ..Default::default() },
            cache: None,
        })
    };
    let color_normal_pipeline = pipeline("texture_reference");
    let scalar_pipeline = pipeline("texture_scalar_reference");
    let frame = PlanarFrame { origin: [0.; 3], u: [2., 0., 0.], v: [0., 2., 0.] };
    let point = Vec3::new(-0.25, 1.125, 0.);
    let uv = frame.uv(point).unwrap();
    let scalar = |value, role, portable| {
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes).write_image(&[value, value, value, 255], 1, 1, image::ExtendedColorType::Rgba8).unwrap();
        if portable { TextureMap::new_portable(bytes, role).unwrap() } else { TextureMap::new(bytes, role).unwrap() }
    };
    for color_portable in [false, true] {
        for normal_portable in [false, true] {
            let fixture = texture_fixture_filtering(frame, color_portable);
            let normal_fixture = texture_fixture_filtering(frame, normal_portable);
            let mut binding = fixture.bindings()[0].clone();
            binding.normal = normal_fixture.bindings()[0].normal.clone();
            binding.roughness = Some(scalar(192, MapRole::Roughness, color_portable));
            binding.metallic = Some(scalar(64, MapRole::Metallic, normal_portable));
            let color = sample_map(&binding.base_color.as_ref().unwrap().decode(&CancelToken::default()).unwrap(), [2, 2], uv).unwrap();
            let normal = sample_map(&binding.normal.as_ref().unwrap().decode(&CancelToken::default()).unwrap(), [2, 2], uv).unwrap();
            let mut input = request(2);
            input.textures = Some(Arc::new(RuntimeTextures::new(vec![binding]).unwrap()));
            let face_color = Vec3::new(0.2, 0.4, 0.6);
            let face = OpaqueSurface::new(face_color, 0.3, 0.2, 1.7).unwrap();
            let mut scene = sphere(white());
            scene.meshes[0].faces.push(FaceAppearance { face: 0, color: [255; 3], opacity: 1., surface: Some(face), transmission: None });
            let mut task = Task::new(context.clone(), scene, input, JobOptions::default(), CancelToken::default()).unwrap();
            let mut params = vec![0; 32];
            scene::vector(&mut params, point, 0.);
            scene::vector(&mut params, Vec3::ZERO, 0.);
            scene::vector(&mut params, Vec3::Z, 0.);
            scene::vector(&mut params, Vec3::Z, 0.);
            context.queue.write_buffer(&task.uniform, 0, &params);
            let run = |task: &mut Task, pipeline: &wgpu::ComputePipeline| {
                let mut encoder = context.device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, &task.bind, &[]);
                    pass.dispatch_workgroups(1, 1, 1);
                }
                context.queue.submit([encoder.finish()]);
                task.sample = 1;
                task.read_linear().unwrap()
            };
            let actual = run(&mut task, &color_normal_pipeline);
            let expected = [face_color.x, face_color.y, face_color.z];
            for (channel, value) in expected.into_iter().enumerate() {
                let expected = if color_portable { value } else { value * color[channel] };
                assert!((f64::from(actual[0][channel]) - expected).abs() < 2e-6, "mixed face color {color_portable}/{normal_portable}: {actual:?}");
            }
            let expected = if normal_portable { Vec3::Z } else { frame.mapped_normal(Vec3::Z, Vec3::Z, normal, 0.75).unwrap() };
            for (actual, expected) in actual[1].iter().zip([expected.x + 2., expected.y + 2., expected.z + 2.]) {
                assert!((f64::from(*actual) - expected).abs() < 3e-6, "mixed face normal {color_portable}/{normal_portable}");
            }
            let actual = run(&mut task, &scalar_pipeline);
            let expected = [if normal_portable { 0.3 } else { 64. / 255. }, if color_portable { 0.2 } else { 192. / 255. }, 1.7];
            for (actual, expected) in actual[0].iter().zip(expected) {
                assert!((f64::from(*actual) - expected).abs() < 2e-6, "mixed face scalars {color_portable}/{normal_portable}");
            }
        }
    }
}

#[test]
fn gpu_wood_minification_converges_to_mean_and_preserves_scalar_face_override() {
    use image::ImageEncoder;
    use solidcraft_render::{
        FaceAppearance,
        job::textures::{BaseColorMode, PlanarFrame, RuntimeTextures, TextureBinding, TextureMap},
        texture_input::MapRole,
    };
    let Some(executor) = executor() else { return };
    // Reuse the established closed slab fixture as an opaque wooden plank.
    let mut scene = glass_slab(false);
    scene.meshes[0].name = "wood diagnostic".into();
    scene.meshes[0].surface = Some(white());
    scene.meshes[0].transmission = None;
    let mesh = Arc::make_mut(&mut scene.meshes[0].mesh);
    for position in &mut mesh.positions {
        position.z -= 0.5;
    }
    mesh.tri_face = (0..mesh.triangles.len() as u32).collect();
    scene.bounds = mesh.bounds();
    let pixels: Vec<u8> = (0..64)
        .flat_map(|i| {
            let v = if i & 1 == 0 { 0 } else { 255 };
            [v, v, v, 255]
        })
        .collect();
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded).write_image(&pixels, 8, 8, image::ExtendedColorType::Rgba8).unwrap();
    let map = TextureMap::new_portable(encoded, MapRole::BaseColor).unwrap();
    let binding = |repeat| TextureBinding {
        body_index: 0,
        body_name: "wood diagnostic".into(),
        frame: PlanarFrame { origin: [0.; 3], u: [repeat, 0., 0.], v: [0., repeat, 0.] },
        normal_strength: 1.,
        base_color: Some(map.clone()),
        normal: None,
        roughness: None,
        metallic: None,
        base_color_mode: BaseColorMode::Replace,
        flip_v: false,
    };
    let render = |scene: ProductScene, source: Option<TextureBinding>, fov: f64, pitch: f64, lens: bool, weights: [f64; 3]| {
        let mut image = request(16);
        image.studio_weights = Some(weights);
        image.camera.fov = fov;
        image.camera.pitch = pitch;
        image.textures = source.map(|b| Arc::new(RuntimeTextures::new(vec![b]).unwrap()));
        let options = JobOptions {
            samples: 1,
            max_depth: 1,
            lens: lens.then_some(solidcraft_render::job::ThinLens { aperture_radius_mm: 0.15, focus_distance_mm: 4.0 }),
            ..Default::default()
        };
        let mut task = Task::new(executor.context.clone(), scene, image, options, CancelToken::default()).unwrap();
        task.complete_sample().unwrap();
        let pixels = task.read_linear().unwrap();
        assert!(pixels.iter().all(|pixel| pixel[3] == 1.0), "the plank must fill every output pixel");
        pixels
    };
    let mut flat = scene.clone();
    flat.meshes[0].surface = Some(OpaqueSurface::new(Vec3::new(0.5, 0.5, 0.5), 0., 1., 1.).unwrap());
    for (fov, pitch, lens) in [(0., std::f64::consts::FRAC_PI_2, false), (0.7, 0.65, false), (0.7, 0.65, true)] {
        for weights in [[1.; 3], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]] {
            let expected = render(flat.clone(), None, fov, pitch, lens, weights);
            if fov == 0.0 {
                // Normal-incidence Lambertian plane: independently sum E cos(theta)/pi.
                let analytic = 0.5 * (2.4 * weights[0] + 0.6 * weights[1] + 1.2 * weights[2]) / std::f64::consts::PI;
                assert!(expected.iter().all(|pixel| (f64::from(pixel[0]) - analytic).abs() < 2e-5));
            }
            let minified = render(scene.clone(), Some(binding(0.25)), fov, pitch, lens, weights);
            for (pixel, (actual, expected)) in minified.iter().zip(&expected).enumerate() {
                for (a, b) in actual.iter().take(3).zip(expected) {
                    assert!(
                        (a - b).abs() < 2e-5,
                        "weights={weights:?}, pixel={pixel}, fov={fov}, pitch={pitch},lens={lens}: minified {actual:?} vs {expected:?}"
                    );
                }
            }
        }
    }
    let close = render(scene.clone(), Some(binding(4.)), 0., std::f64::consts::FRAC_PI_2, false, [1.; 3]);
    let spread = close.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max) - close.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
    assert!(spread > 0.1, "magnified grain must retain contrast: {spread}");
    let mut overridden = scene;
    overridden.meshes[0].faces = vec![FaceAppearance {
        face: 0,
        color: [64, 128, 192],
        opacity: 1.,
        surface: Some(OpaqueSurface::new(Vec3::new(0.1, 0.2, 0.3), 0., 1., 1.).unwrap()),
        transmission: None,
    }];
    let scalar = render(overridden.clone(), None, 0., std::f64::consts::FRAC_PI_2, false, [1.; 3]);
    let mapped = render(overridden, Some(binding(0.25)), 0., std::f64::consts::FRAC_PI_2, false, [1.; 3]);
    let explicit: Vec<_> = scalar.iter().zip(&mapped).filter(|(a, _)| a[2] > a[0] * 2.).collect();
    assert!(explicit.len() > 32, "the diagnostic must cover explicit face pixels");
    for (a, b) in explicit {
        assert_eq!(a, b, "a scalar face suppresses inherited maps");
    }
}

#[test]
fn gpu_mapped_wood_preserves_index_matched_sheet_and_solid_transport() {
    use image::ImageEncoder;
    use solidcraft_render::{
        job::textures::{BaseColorMode, PlanarFrame, RuntimeTextures, TextureBinding, TextureMap},
        texture_input::MapRole,
    };
    let Some(executor) = executor() else { return };
    let mut board = glass_slab(false).meshes.remove(0);
    board.name = "mixed wood board".into();
    board.surface = Some(white());
    board.transmission = None;
    for position in &mut Arc::make_mut(&mut board.mesh).positions {
        position.z -= 0.5;
    }
    let pixels: Vec<u8> = (0..64)
        .flat_map(|i| {
            let c = if i & 1 == 0 { 0 } else { 255 };
            [c, c, c, 255]
        })
        .collect();
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded).write_image(&pixels, 8, 8, image::ExtendedColorType::Rgba8).unwrap();
    let resources = Arc::new(
        RuntimeTextures::new(vec![TextureBinding {
            body_index: 0,
            body_name: board.name.clone(),
            frame: PlanarFrame { origin: [0.; 3], u: [0.25, 0., 0.], v: [0., 0.25, 0.] },
            normal_strength: 1.,
            base_color: Some(TextureMap::new_portable(encoded, MapRole::BaseColor).unwrap()),
            normal: None,
            roughness: None,
            metallic: None,
            base_color_mode: BaseColorMode::Replace,
            flip_v: false,
        }])
        .unwrap(),
    );
    // At IOR1, zero extinction and no scatter, either glass boundary is the
    // exact identity. The mean map reflectance .5 receives three fixed lights.
    let expected = 0.5 * (3.0 * 0.8 + 1.0 * 0.6 + 1.5 * 0.8) / std::f64::consts::PI;
    for mapped in [false, true] {
        for (thin, active_optics) in [(true, false), (false, false), (true, true)] {
            if active_optics && !mapped {
                continue;
            }
            let mut glass = glass_slab(thin).meshes.remove(0);
            glass.name = "index matched glazing".into();
            for position in &mut Arc::make_mut(&mut glass.mesh).positions {
                position.z += 1.0;
                if active_optics {
                    // A vertex anchor lies outside the board. The actual hit
                    // stays over it after the barycentric displacement is added.
                    position.x *= 4.0;
                    position.y *= 4.0;
                }
            }
            glass.surface = Some(OpaqueSurface::new(Vec3::ZERO, 0., 1., 1.).unwrap());
            glass.transmission =
                Some(solidcraft_render::transmission::Transmission::new(Vec3::ZERO, thin.then_some(0.)).unwrap().with_shadow_transmission(true));
            let mut scene = ProductScene { bounds: board.mesh.bounds().union(&glass.mesh.bounds()), meshes: vec![board.clone(), glass] };
            if !mapped {
                scene.meshes[0].surface = Some(OpaqueSurface::new(Vec3::new(0.5, 0.5, 0.5), 0., 1., 1.).unwrap());
            }
            if active_optics {
                // A separate rough body selects the split optical producer
                // without changing the index-matched sheet's identity oracle.
                let mut rough = rough_glass_fixture(false, 0.35, Vec3::ZERO, 0.0).meshes.remove(0);
                rough.name = "offscreen rough glazing".into();
                for position in &mut Arc::make_mut(&mut rough.mesh).positions {
                    position.x += 100.0;
                }
                scene.bounds = scene.bounds.union(&rough.mesh.bounds());
                scene.meshes.push(rough);
            }
            let mut image = request(16);
            image.textures = mapped.then(|| resources.clone());
            let mut task = Task::new(
                executor.context.clone(),
                scene,
                image,
                JobOptions { samples: 1, max_depth: if thin { 2 } else { 3 }, ..Default::default() },
                CancelToken::default(),
            )
            .unwrap();
            assert!(!task.opaque_scene);
            assert_eq!(task.has_textures, mapped);
            assert_eq!(task.optical_resolve.is_some(), active_optics);
            task.complete_sample().unwrap_or_else(|error| panic!("thin={thin}, mapped={mapped}, active optics={active_optics}: {error:?}"));
            for (pixel, value) in task
                .read_linear()
                .unwrap_or_else(|error| panic!("thin={thin}, mapped={mapped}, active optics={active_optics}: {error:?}"))
                .iter()
                .enumerate()
            {
                assert_eq!(value[3], 1.0, "thin={thin}, mapped={mapped}, active optics={active_optics},pixel={pixel}");
                for actual in value.iter().take(3) {
                    assert!(
                        (f64::from(*actual) - expected).abs() < 2e-5,
                        "thin={thin}, mapped={mapped}, active optics={active_optics},pixel={pixel}: {value:?} vs {expected}"
                    );
                }
            }
        }
    }
}

#[test]
fn gpu_mapped_wood_keeps_fine_detail_behind_large_index_matched_solid_glass() {
    use image::ImageEncoder;
    use solidcraft_render::{
        job::textures::{BaseColorMode, PlanarFrame, RuntimeTextures, TextureBinding, TextureMap},
        texture_input::MapRole,
    };
    let Some(executor) = executor() else { return };
    let mut board = glass_slab(false).meshes.remove(0);
    board.name = "fine wood diagnostic".into();
    board.surface = Some(white());
    board.transmission = None;
    for position in &mut Arc::make_mut(&mut board.mesh).positions {
        position.z -= 2.0;
    }
    let mut glass = glass_slab(false).meshes.remove(0);
    glass.name = "large index matched glazing".into();
    for position in &mut Arc::make_mut(&mut glass.mesh).positions {
        position.x *= 50.0;
        position.y *= 50.0;
    }
    glass.surface = Some(OpaqueSurface::new(Vec3::ZERO, 0., 1., 1.).unwrap());
    glass.transmission = Some(solidcraft_render::transmission::Transmission::new(Vec3::ZERO, None).unwrap().with_shadow_transmission(true));
    // Each 2x2-texel block has a flat bilinear centre. The two camera patches
    // below see adjacent white/black details, not the texture's averaged mean.
    let pixels: Vec<u8> = (0..16)
        .flat_map(|y| {
            (0..16).flat_map(move |x| {
                let value = if (x / 2 + y / 2) & 1 == 0 { 255 } else { 0 };
                [value, value, value, 255]
            })
        })
        .collect();
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded).write_image(&pixels, 16, 16, image::ExtendedColorType::Rgba8).unwrap();
    let resources = Arc::new(
        RuntimeTextures::new(vec![TextureBinding {
            body_index: 0,
            body_name: board.name.clone(),
            frame: PlanarFrame { origin: [-0.125, -0.125, 0.], u: [2., 0., 0.], v: [0., 2., 0.] },
            normal_strength: 1.,
            base_color: Some(TextureMap::new_portable(encoded, MapRole::BaseColor).unwrap()),
            normal: None,
            roughness: None,
            metallic: None,
            base_color_mode: BaseColorMode::Replace,
            flip_v: false,
        }])
        .unwrap(),
    );
    let render = |with_glass, centre_x| {
        let mut scene = ProductScene { bounds: board.mesh.bounds(), meshes: vec![board.clone()] };
        if with_glass {
            scene.bounds = scene.bounds.union(&glass.mesh.bounds());
            scene.meshes.push(glass.clone());
        }
        let mut image = request(32);
        image.camera.target = Vec3::new(centre_x, 0., -1.5);
        image.camera.fov = 0.02;
        image.textures = Some(resources.clone());
        let mut task = Task::new(
            executor.context.clone(),
            scene,
            image,
            JobOptions { samples: 1, max_depth: if with_glass { 3 } else { 1 }, seed: 1599, ..Default::default() },
            CancelToken::default(),
        )
        .unwrap();
        assert_eq!(task.opaque_scene, !with_glass);
        task.complete_sample().unwrap();
        task.read_linear().unwrap()
    };
    // The camera-to-board path is about 4mm, including a 1mm glass/wood gap.
    // Closed-glass hit origins split a +/-500mm vertex anchor from the full
    // barycentric displacement. Ignoring that low component invents roughly
    // 1400mm of travel and selects coarse checker mips instead of these details.
    let unit_reflectance = (3.0 * 0.8 + 1.0 * 0.6 + 1.5 * 0.8) / std::f64::consts::PI;
    for (centre_x, reflectance) in [(0., 1.), (0.25, 0.)] {
        let direct = render(false, centre_x);
        let through_glass = render(true, centre_x);
        for (pixel, (actual, expected)) in through_glass.iter().zip(&direct).enumerate() {
            assert_eq!(expected[3], 1.0, "direct patch {centre_x}, pixel={pixel}");
            assert_eq!(actual[3], 1.0, "glazed patch {centre_x}, pixel={pixel}");
            for (actual, expected) in actual.iter().take(3).zip(expected) {
                assert!((f64::from(*expected) - reflectance * unit_reflectance).abs() < 2e-5, "direct patch {centre_x}, pixel={pixel}: {expected}");
                assert!((actual - expected).abs() < 2e-5, "glazed patch {centre_x}, pixel={pixel}: {actual} vs {expected}");
            }
        }
    }
}

#[test]
fn gpu_scalar_maps_match_linear_reference_without_touching_ior() {
    use image::ImageEncoder;
    use solidcraft_render::{
        job::textures::{PlanarFrame, RuntimeTextures, TextureMap},
        texture_input::MapRole,
    };
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let scalar = |value: u8, role| {
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes).write_image(&[value, value, value, 255], 1, 1, image::ExtendedColorType::Rgba8).unwrap();
        TextureMap::new(bytes, role).unwrap()
    };
    let fixture = texture_fixture(PlanarFrame { origin: [0.0; 3], u: [2.0, 0.0, 0.0], v: [0.0, 2.0, 0.0] });
    let mut binding = fixture.bindings()[0].clone();
    binding.base_color = None;
    binding.normal = None;
    binding.flip_v = true;
    binding.roughness = Some(scalar(192, MapRole::Roughness));
    binding.metallic = Some(scalar(64, MapRole::Metallic));
    let source = Arc::new(RuntimeTextures::new(vec![binding]).unwrap());
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("texture_scalar_reference"),
        compilation_options: wgpu::PipelineCompilationOptions { constants: &[("HAS_TEXTURES", 1.0)], ..Default::default() },
        cache: None,
    });
    let mut input = request(2);
    input.textures = Some(source);
    // An explicit non-default optical fixture catches accidental IOR rewrites.
    let original = OpaqueSurface::new(Vec3::new(0.2, 0.4, 0.6), 0.6, 0.2, 1.7).unwrap();
    let mut task = Task::new(context.clone(), sphere(original), input, JobOptions::default(), CancelToken::default()).unwrap();
    let mut params = vec![0; 32];
    scene::vector(&mut params, Vec3::ZERO, 0.0);
    scene::vector(&mut params, Vec3::ZERO, 0.0);
    scene::vector(&mut params, Vec3::Z, 0.0);
    scene::vector(&mut params, Vec3::Z, 0.0);
    context.queue.write_buffer(&task.uniform, 0, &params);
    let mut encoder = context.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &task.bind, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    context.queue.submit([encoder.finish()]);
    task.sample = 1;
    let actual = task.read_linear().unwrap();
    for (a, b) in actual[0].iter().zip([64.0 / 255.0, 192.0 / 255.0, original.dielectric_ior()]) {
        assert!((f64::from(*a) - b).abs() < 2e-6, "scalar map {actual:?}");
    }
}

#[test]
fn production_envelope_is_device_bounded_and_never_silently_resized() {
    let mut image = request(1);
    image.width = 3840;
    image.height = 2160;
    let source = sphere(white());
    let accepted = admission(&source, &image, JobOptions::default()).unwrap();
    assert!(accepted.estimated_total_bytes < GPU_PATH_LIMITS.max_scratch_bytes);
    assert_eq!(descriptor().unwrap().fit(3840, 2160), Some((3840, 2160)));
    image.width = 8193;
    assert!(admission(&source, &image, JobOptions::default()).is_err());
    image.width = 8192;
    image.height = 8192;
    assert!(admission(&source, &image, JobOptions::default()).is_err());
    let mut supported = wgpu::Limits::downlevel_defaults();
    supported.max_storage_buffers_per_shader_stage = 8;
    supported.max_storage_buffer_binding_size = 2 * 1024 * 1024 * 1024;
    supported.max_buffer_size = 2 * 1024 * 1024 * 1024;
    let limits = device_limits(supported.clone()).unwrap();
    assert_eq!(limits.max_storage_buffer_binding_size, supported.max_storage_buffer_binding_size);
    assert_eq!(limits.max_buffer_size, supported.max_buffer_size);
}

#[test]
fn disjoint_glass_count_does_not_relax_closed_topology_or_overlap_validation() {
    let template = sphere_grid(white(), 8, 16);
    let mut source = ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY };
    for i in 0..solidcraft_render::job::MAX_GPU_DIELECTRICS {
        let mut body = template.meshes[0].clone();
        body.name = format!("separate glass {i}");
        for p in &mut Arc::make_mut(&mut body.mesh).positions {
            p.x += i as f64 * 3.0;
        }
        body.transmission = Some(clear_glass());
        source.bounds = source.bounds.union(&body.mesh.bounds());
        source.meshes.push(body);
    }
    assert!(scene::flatten(&source, Vec3::ZERO, &CancelToken::default()).is_ok());
    source.meshes.push(source.meshes[0].clone());
    assert!(scene::flatten(&source, Vec3::ZERO, &CancelToken::default()).is_err());
}

#[test]
fn display_identity_matches_the_legacy_response_and_spatial_filters_ignore_chunking() {
    use solidcraft_render::job::finishing::{Backdrop, BackdropMode, Finishing};
    let cancel = CancelToken::default();
    let input: Vec<[f32; 4]> = (0..91)
        .map(|i| {
            [
                i as f32 * 0.15,
                0.07,
                0.8,
                if i % 3 == 0 {
                    0.0
                } else if i % 3 == 1 {
                    0.5
                } else {
                    1.0
                },
            ]
        })
        .collect();
    let display = Display {
        finishing: Finishing::default(),
        backdrop: None,
        width: 13,
        height: 7,
        exposure: 1.3,
        tone_map: ToneMap::Reinhard,
        background: [0.02, 0.05, 0.12],
        transparent: false,
    };
    let mut reference = Vec::new();
    for pixel in &input {
        let alpha = f64::from(pixel[3]);
        for (radiance, background) in pixel.iter().take(3).zip(display.background) {
            let c = if alpha > 0.0 { f64::from(*radiance) * display.exposure / alpha } else { 0.0 };
            let value = display.tone_map.map(c) * alpha + background * (1.0 - alpha);
            let encoded = if value <= 0.0031308 { value * 12.92 } else { 1.055 * value.powf(1.0 / 2.4) - 0.055 };
            reference.push((encoded.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
        reference.push(255);
    }
    let mut neutral = vec![0; input.len() * 4];
    display.encode(&input, &mut neutral, &cancel).unwrap();
    assert_eq!(neutral, reference);
    let changed = Display {
        finishing: Finishing { rgb_gain: [1.2, 0.9, 0.7], contrast: 1.2, saturation: 0.3, vignette: 0.45, ..Default::default() },
        backdrop: Some(Backdrop { mode: BackdropMode::Radial, ..Default::default() }),
        ..display
    };
    let mut all = vec![0; input.len() * 4];
    changed.encode(&input, &mut all, &cancel).unwrap();
    for chunk in [1, 5, 13, 90] {
        let mut split = vec![0; all.len()];
        for (index, (pixels, output)) in input.chunks(chunk).zip(split.chunks_mut(chunk * 4)).enumerate() {
            changed.encode_at(pixels, output, &cancel, index * chunk).unwrap();
        }
        assert_eq!(all, split, "chunk {chunk} must preserve absolute vignette/backdrop position");
    }
    assert_ne!(all, neutral);
}

fn complete_linear_fixture(context: Arc<Context>, options: JobOptions) -> (Vec<u8>, Option<Vec<u8>>) {
    // Use the public environment-lighting contract, not the test-only numerical
    // furnace path. Primary misses remain transparent black in the linear master.
    let options = JobOptions {
        lighting: Some(solidcraft_render::job::StudioLighting {
            punctual_lights: Default::default(),
            directional_lights: Default::default(),
            environment: [8.0; 3],
            sun: None,
            area_lights: Default::default(),
        }),
        ..options
    };
    let mut task = Task::new(context, sphere(white()), request(16), options, CancelToken::default()).unwrap();
    for _ in 0..5000 {
        match task.step(16384).unwrap() {
            TaskPoll::Pending(_) => {}
            TaskPoll::Complete(png) => {
                let exr = task.take_linear_exr();
                assert!(task.take_linear_exr().is_none(), "linear master moves exactly once");
                return (png, exr);
            }
        }
    }
    panic!("bounded linear-output fixture did not finish")
}
#[test]
fn gpu_raw_master_keeps_hdr_samples_before_denoise_gain_tone_and_camera_backdrop() {
    use solidcraft_render::job::finishing::{Backdrop, Finishing};
    let Some(executor) = executor() else {
        return;
    };
    let options = JobOptions { samples: 16, seed: 17, max_depth: 3, ..Default::default() };
    let (png, none) = complete_linear_fixture(executor.context.clone(), options);
    assert!(none.is_none());
    let (retained, raw) = complete_linear_fixture(executor.context.clone(), JobOptions { linear_exr: true, ..options });
    assert_eq!(png, retained, "opting into raw retention does not change display pixels");
    let raw = raw.unwrap();
    let (filtered, other) = complete_linear_fixture(
        executor.context.clone(),
        JobOptions {
            linear_exr: true,
            denoise: true,
            exposure_ev: -2.0,
            tone_map: ToneMap::AcesFitted,
            finishing: Finishing { rgb_gain: [1.4, 0.8, 0.6], saturation: 0.2, vignette: 0.4, ..Default::default() },
            backdrop: Some(Backdrop::default()),
            ..options
        },
    );
    assert_eq!(raw, other.unwrap(), "raw sample values precede every presentation adjustment");
    assert_ne!(png, filtered);
    // Parse attributes/offsets independently to check real HDR data > display white.
    let mut cursor = 8;
    while raw[cursor] != 0 {
        while raw[cursor] != 0 {
            cursor += 1;
        }
        cursor += 1;
        while raw[cursor] != 0 {
            cursor += 1;
        }
        cursor += 1;
        let size = i32::from_le_bytes(raw[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4 + size;
    }
    cursor += 1;
    let mut maximum = 0_f32;
    for y in 0..16 {
        let offset = u64::from_le_bytes(raw[cursor + y * 8..cursor + y * 8 + 8].try_into().unwrap()) as usize;
        for bytes in raw[offset + 8 + 64..offset + 8 + 256].as_chunks::<4>().0 {
            maximum = maximum.max(f32::from_le_bytes(*bytes));
        }
    }
    assert!(maximum > 2.0, "raw highlight radiance must not be clipped or reconstructed from PNG: {maximum}");
    // Optional fresh external evidence folder for an independent OpenEXR reader.
    if let Some(out) = std::env::var_os("SOLIDCRAFT_FINISHING_EVIDENCE_DIR") {
        let out = std::path::PathBuf::from(out);
        assert!(out.is_absolute() && !out.exists());
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("raw-radiance.exr"), &raw).unwrap();
        std::fs::write(out.join("neutral.png"), &png).unwrap();
        std::fs::write(out.join("filtered.png"), &filtered).unwrap();
        std::fs::write(
            out.join("expected.json"),
            serde_json::to_vec_pretty(&serde_json::json!({"width":16,"height":16,
            "maximum_raw_rgb":maximum,"raw_identical_before_after_finishing":true,"png_neutral_identity":true,
            "rgba_type":"FLOAT","encoding":"scene-linear sRGB/D65","alpha":"coverage-premultiplied"}))
            .unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn gpu_host_retains_linear_companion_under_the_same_exact_frame_lease() {
    use solidcraft_render::job::{JobId, Phase};
    let Some(executor) = executor() else {
        return;
    };
    let host = ImageHost::with_executor(Arc::new(executor)).unwrap();
    let id = JobId { session: 999, serial: 1, generation: 42 };
    let source = sphere(white());
    let req = request(8);
    let base = host.admit(&source, &req, JobOptions::default()).unwrap();
    let options = JobOptions { samples: 4, linear_exr: true, ..Default::default() };
    let extra = host.admit(&source, &req, options).unwrap();
    assert_eq!(extra.estimated_total_bytes - base.estimated_total_bytes, solidcraft_render::job::linear_exr::encoded_limit(8, 8).unwrap());
    let handle = host.submit_with_options(id, 7, source.clone(), req.clone(), options).unwrap();
    let status = handle.wait_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(status.progress.phase, Phase::Completed, "{:?}", status.error);
    let frame = handle.frame().unwrap().unwrap();
    let bytes = frame.linear_exr().unwrap();
    assert_eq!(frame.linear_master_info().unwrap().bytes, bytes.len() as u64);
    assert_eq!(status.linear_master.unwrap().sha256, frame.linear_master_info().unwrap().sha256);
    assert!(host.admitted_bytes() >= frame.png().len() as u64 + bytes.len() as u64);
    drop(handle);
    assert!(host.admitted_bytes() > 0);
    drop(frame);
    // Cancellation/completion notification and worker-owned resource destruction
    // are distinct events. Require the actual reservation to drain, without
    // forcing premature release or changing the host's asynchronous contract.
    let released_by = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while host.admitted_bytes() != 0 {
        assert!(std::time::Instant::now() < released_by, "worker did not release its completed/cancelled reservation");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(host.admitted_bytes(), 0);
    let handle = host.submit_with_options(JobId { serial: 2, ..id }, 7, source, req, JobOptions { samples: 4096, ..options }).unwrap();
    handle.cancel().unwrap();
    let status = handle.wait_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(status.progress.phase, Phase::Cancelled);
    assert!(handle.frame().unwrap().is_none());
    drop(handle);
    // Cancellation/completion notification and worker-owned resource destruction
    // are distinct events. Require the actual reservation to drain, without
    // forcing premature release or changing the host's asynchronous contract.
    let released_by = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while host.admitted_bytes() != 0 {
        assert!(std::time::Instant::now() < released_by, "worker did not release its completed/cancelled reservation");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(host.admitted_bytes(), 0);
}

#[test]
fn engine_exports_raw_master_from_the_exact_completed_job_without_png_conversion() {
    use serde_json::json;
    use solidcraft_render::job::{JobId, Phase};
    let Some(executor) = executor() else {
        return;
    };
    let host = ImageHost::with_executor(Arc::new(executor)).unwrap();
    let mut s = solidcraft_engine::Session::default();
    s.register_render_job_host(host.clone()).unwrap();
    s.execute("solid.box", &json!({"length":2,"width":2,"height":1,"body_name":"Original output fixture"})).unwrap();
    let before = s.doc.to_json();
    let p = json!({"progressive":true,"samples":4,"linear_exr":true,"settings":{"width":12,"height":8},
        "finishing":{"saturation":0.4},"backdrop":{"mode":"horizontal"}});
    let checked = s.execute("render.preflight", &p).unwrap();
    assert_eq!(checked["options"]["linear_exr"], true);
    let started = s.execute("render.start", &p).unwrap();
    let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let handle = s.render_job_handle(id).unwrap();
    assert_eq!(handle.wait_timeout(std::time::Duration::from_secs(30)).unwrap().progress.phase, Phase::Completed);
    let frame = handle.frame().unwrap().unwrap();
    let root = std::env::temp_dir().join(format!(
        "solidcraft-linear-export-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("raw.exr");
    let png = root.join("display.png");
    let output = s.execute("render.export", &json!({"job":id,"path":path,"format":"exr"})).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), frame.linear_exr().unwrap());
    assert_eq!(output["linear_master"]["sha256"], frame.linear_master_info().unwrap().sha256);
    assert_eq!(output["encoding"], "scene_linear_srgb_d65");
    assert_eq!(output["alpha"], "coverage_premultiplied");
    assert_eq!(output["linear_master"]["encoding"], output["encoding"]);
    s.execute("render.export", &json!({"job":id,"path":png})).unwrap();
    assert_eq!(std::fs::read(&png).unwrap(), frame.png());
    assert!(s.execute("render.export", &json!({"job":id,"path":path,"format":"exr"})).is_err());
    assert!(s.execute("render.export", &json!({"job":id,"path":path,"format":"png","overwrite":true})).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), frame.linear_exr().unwrap());
    assert!(s.execute("render.export", &json!({"job":JobId{session:id.session+1,..id},"path":root.join("wrong.exr"),"format":"exr"})).is_err());
    assert!(!root.join("wrong.exr").exists());
    assert_eq!(s.doc.to_json(), before);
    drop(s);
    drop(handle);
    assert!(host.admitted_bytes() > 0);
    drop(frame);
    // Completion notification precedes worker-owned resource destruction.
    // Require the actual reservation to drain after the final frame lease drops.
    let released_by = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while host.admitted_bytes() != 0 {
        assert!(std::time::Instant::now() < released_by, "worker did not release its exported frame reservation");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(host.admitted_bytes(), 0);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn gpu_bloom_changes_display_only_with_explicit_scratch_and_opaque_contract() {
    use solidcraft_render::job::{bloom::Bloom, finishing::Finishing};
    let Some(executor) = executor() else { return };
    let options = JobOptions { samples: 16, seed: 17, max_depth: 3, linear_exr: true, ..Default::default() };
    let (png, raw) = complete_linear_fixture(executor.context.clone(), options);
    let bloom = Bloom { strength: 0.4, threshold: 1.0, radius: 2 };
    let modified = JobOptions { finishing: Finishing { bloom: Some(bloom), ..Default::default() }, ..options };
    let a = admission(&sphere(white()), &request(16), options).unwrap();
    let b = admission(&sphere(white()), &request(16), modified).unwrap();
    assert_eq!(b.estimated_total_bytes - a.estimated_total_bytes, 16 * 16 * 24);
    assert_eq!(b.pixel_scratch_bytes - a.pixel_scratch_bytes, 16 * 16 * 24);
    let (glowing, untouched) = complete_linear_fixture(executor.context.clone(), modified);
    assert_eq!(raw, untouched, "bloom cannot alter the retained linear master or original estimator");
    assert_ne!(png, glowing);
    let (neutral, same) = complete_linear_fixture(
        executor.context.clone(),
        JobOptions { finishing: Finishing { bloom: Some(Bloom { strength: 0.0, ..bloom }), ..Default::default() }, ..options },
    );
    assert_eq!(png, neutral);
    assert_eq!(raw, same);
    let transparent = ImageRequest { transparent_background: true, ..request(16) };
    assert!(executor.admit(&sphere(white()), &transparent, modified).is_err());
    let decoded = image::load_from_memory(&glowing).unwrap().to_rgba8();
    assert!(decoded.pixels().all(|p| p.0[3] == 255));
}

#[path = "adaptive_tests.rs"]
mod adaptive_tests;

#[path = "regrade_tests.rs"]
mod regrade_tests;
#[path = "volume_tests.rs"]
mod volume_tests;

#[path = "resource_pack_tests.rs"]
mod resource_pack_tests;

#[path = "optical_media_tests.rs"]
mod optical_media_tests;

#[path = "effects_tests.rs"]
mod effects_tests;

#[test]
fn display_bloom_slices_refuse_invalid_ranges_without_panicking() {
    let pixels = [[1.0_f32, 2.0, 3.0]; 3];
    assert_eq!(bloom_chunk(Some(&pixels), 1, 2).unwrap().unwrap(), &pixels[1..3]);
    assert!(bloom_chunk(Some(&pixels), 3, 0).unwrap().unwrap().is_empty());
    for (offset, count) in [(0, 4), (2, 2), (4, 0), (usize::MAX, 1), (1, usize::MAX)] {
        let result = std::panic::catch_unwind(|| bloom_chunk(Some(&pixels), offset, count));
        assert!(result.expect("invalid retained image range must not panic").is_err());
    }
    assert!(bloom_chunk(None, 0, 3).unwrap().is_none());
}

/// Cancellation in fallible HDR preparation must not drop three wgpu scopes in
/// forward order. Exercise the real preparation routine inside the guarded path,
/// then prove that the same device still accepts a fresh nested scope stack.
#[test]
fn cancelled_environment_preparation_unwinds_gpu_error_scopes() {
    use solidcraft_render::lighting::environment::EnvironmentSource;
    let Ok((_, device, _queue)) = open_headless() else {
        let available = !solidcraft_viewport_gpu::select::path_backends().is_empty();
        assert!(!available || std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_none(), "required Vulkan device unavailable");
        return;
    };
    let mut image = request(1);
    image.environment = Some(Arc::new(EnvironmentSource::hdr(original_hdr(), 0.0, 1.0, false).unwrap()));
    let token = CancelToken::default();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        checked_creation(&device, || {
            token.cancel();
            prepare_environment(&image, &token)
        })
    }));
    let error = outcome.expect("ordinary cancellation must not panic").unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
    checked(scopes(&device)).unwrap();
    let bytes = prepare_environment(&image, &CancelToken::default()).unwrap().0;
    let info = image.environment.as_ref().unwrap().info();
    assert_eq!(bytes.len(), 16 + info.width as usize * info.height as usize * 32);
}

/// A denoiser allocation can fail after the outer resource scopes were opened.
/// Preserve its Result while draining every scope, not only the last one.
#[test]
fn failed_resource_creation_unwinds_gpu_error_scopes() {
    let Ok((_, device, _queue)) = open_headless() else {
        let available = !solidcraft_viewport_gpu::select::path_backends().is_empty();
        assert!(!available || std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_none(), "required Vulkan device unavailable");
        return;
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        checked_creation(&device, || -> Result<()> { Err(JobError::new(ErrorCode::LimitExceeded, "synthetic bounded allocation refusal")) })
    }));
    let error = outcome.expect("ordinary allocation refusal must not panic").unwrap_err();
    assert_eq!(error.code, ErrorCode::LimitExceeded);
    checked(scopes(&device)).unwrap();
}

#[test]
fn actual_first_gpu_quantum_is_small_and_cancellation_retains_no_completed_image() {
    let Some(executor) = executor() else { return };
    let token = CancelToken::default();
    let mut task = Task::new(executor.context.clone(), sphere(white()), request(64), JobOptions::default(), token.clone()).unwrap();
    task.dispatch().unwrap();
    assert_eq!(task.offset, dispatch::MIN_PATHS);
    assert_eq!(task.sample, 0);
    token.cancel();
    assert_eq!(task.step(4096).err().unwrap().code, ErrorCode::Cancelled);
    assert!(task.png.is_none());
}

#[test]
fn production_path_workgroup_matches_the_host_dispatch_contract() {
    let module = naga::front::wgsl::parse_str(SHADER).unwrap_or_else(|error| panic!("{}", error.emit_to_string(SHADER)));
    let main = module.entry_points.iter().find(|entry| entry.name == "main").unwrap();
    assert_eq!(main.workgroup_size, [PATH_WORKGROUP_SIZE, 1, 1]);
}

#[test]
fn actual_gpu_dispatch_visits_workgroup_tails_and_only_the_current_tile() {
    let Some(executor) = executor() else { return };
    for width in [1, 31, 32, 33, 63, 64, 65, 129] {
        let image = ImageRequest { width, height: 1, ..request(1) };
        let mut task = Task::new(
            executor.context.clone(),
            ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY },
            image,
            JobOptions { samples: 2, ..Default::default() },
            CancelToken::default(),
        )
        .unwrap();
        // The numerical furnace produces an exact nonzero value even on a miss. A skipped
        // invocation therefore cannot masquerade as a valid zero/background pixel.
        task.environment = 1.0;
        task.studio = 0.0;
        task.complete_sample().unwrap();
        assert_eq!(task.sample, 1);
        assert_eq!(task.offset, 0);
        assert_eq!(
            task.read_linear().unwrap(),
            vec![[1.0, 1.0, 1.0, 0.0]; width as usize],
            "every first-pass pixel, including a partial workgroup, must be written: width={width}"
        );
        if width == 129 {
            for prefix in [64, 128, 129] {
                task.dispatch_budget = dispatch::Budget::default();
                task.dispatch().unwrap();
                let actual = task.read_linear().unwrap();
                if prefix < width {
                    assert_eq!((task.sample, task.offset), (1, prefix));
                    for (index, pixel) in actual.iter().enumerate() {
                        let value = if index < prefix as usize { 2.0 } else { 1.0 };
                        assert_eq!(*pixel, [value, value, value, 0.0], "dispatch may update only its own tile: prefix={prefix}, pixel={index}");
                    }
                } else {
                    assert_eq!((task.sample, task.offset), (2, 0));
                    assert_eq!(actual, vec![[1.0, 1.0, 1.0, 0.0]; width as usize]);
                }
            }
        }
    }
}

#[test]
fn real_device_loss_retires_the_executor_and_the_sessions_cached_availability() {
    use solidcraft_engine::Session;
    let Some(executor) = executor() else { return };
    let context = executor.context.clone();
    let host = ImageHost::with_executor(Arc::new(executor)).unwrap();
    let mut session = Session::default();
    session.register_render_job_host(host.clone()).unwrap();
    assert!(matches!(host.descriptor().unwrap().availability, Availability::Available));
    context.device.destroy();
    let deadline = Instant::now() + Duration::from_secs(2);
    while context.healthy.load(Ordering::Acquire) && Instant::now() < deadline {
        let _ = context.device.poll(wgpu::PollType::Poll);
        std::thread::yield_now();
    }
    assert!(!context.healthy.load(Ordering::Acquire), "actual device-loss callback must retire the host");
    assert!(matches!(host.descriptor().unwrap().availability, Availability::Unavailable { .. }));
    let scene = sphere(white());
    assert_eq!(host.admit(&scene, &request(1), JobOptions::default()).unwrap_err().code, ErrorCode::HostUnavailable);
    let backend = session.render_backend();
    assert!(backend.executors.iter().any(|d| d.id.as_str() == "gpu-path" && matches!(d.availability, Availability::Unavailable { .. })));
}

#[test]
fn fixed_gpu_previews_match_complete_prefixes_with_finishing_and_final_exr() {
    let Some(executor) = executor() else { return };
    let scene = sphere_grid(white(), 8, 16);
    let request = request(8);
    let options = JobOptions {
        samples: 4,
        max_depth: 3,
        seed: 47,
        transmission_samples: 2,
        linear_exr: true,
        finishing: solidcraft_render::job::finishing::Finishing {
            rgb_gain: [1.2, 0.9, 1.1],
            contrast: 1.1,
            bloom: Some(solidcraft_render::job::bloom::Bloom { strength: 0.25, threshold: 0.1, radius: 1 }),
            photographic: solidcraft_render::job::photographic::Photographic { grain: 0.01, seed: 17, ..Default::default() },
            ..Default::default()
        },
        ..Default::default()
    };
    let admitted = executor.admit(&scene, &request, options).unwrap();
    // Host frame metadata now includes separate colour, raw-master and guide-mask identities.
    let slot_bytes = u64::from(request.width) * u64::from(request.height) * 8 + 65_536 + 2048;
    assert_eq!(admitted.encoded_limit + 2048, slot_bytes, "GPU encoder and shared preview slots use the same bound");
    let host = ImageHost::with_executor(Arc::new(Executor { context: executor.context.clone() })).unwrap();
    assert_eq!(host.admit(&scene, &request, options).unwrap().estimated_total_bytes, admitted.estimated_total_bytes + 2 * slot_bytes + 2048);
    let mut task = Task::new(executor.context.clone(), scene.clone(), request.clone(), options, CancelToken::default()).unwrap();
    task.preview_enabled = true;
    let mut previews = Vec::new();
    let mut final_png = None;
    for _ in 0..10_000 {
        let poll = task.step(4096).unwrap();
        if let Some(preview) = task.take_preview() {
            assert!(matches!(poll, TaskPoll::Pending(_)));
            assert!(task.exr.is_none(), "provisional output must not create a raw master");
            previews.push(preview);
        }
        if let TaskPoll::Complete(png) = poll {
            final_png = Some(png);
            break;
        }
    }
    assert_eq!(previews.iter().map(|p| p.samples_per_pixel).collect::<Vec<_>>(), [1, 2, 3]);
    for preview in previews {
        let mut prefix = Task::new(
            executor.context.clone(),
            scene.clone(),
            request.clone(),
            JobOptions { samples: preview.samples_per_pixel, ..options },
            CancelToken::default(),
        )
        .unwrap();
        let mut expected = None;
        for _ in 0..10_000 {
            if let TaskPoll::Complete(png) = prefix.step(4096).unwrap() {
                expected = Some(png);
                break;
            }
        }
        assert_eq!(Some(preview.png), expected, "provisional effects must match the same complete sample prefix");
    }
    assert!(final_png.is_some());
    let master = task.take_linear_exr().unwrap();
    let mut reference = Task::new(executor.context.clone(), scene, request, options, CancelToken::default()).unwrap();
    let mut reference_png = None;
    for _ in 0..10_000 {
        if let TaskPoll::Complete(png) = reference.step(4096).unwrap() {
            reference_png = Some(png);
            break;
        }
    }
    assert_eq!(final_png, reference_png, "publishing previews cannot change final presentation");
    assert_eq!(Some(master), reference.take_linear_exr(), "provisional effects cannot alter the raw master");
}

#[test]
fn adaptive_gpu_checkpoint_target_includes_pilots_without_disabling_final_rendering() {
    let Some(executor) = executor() else { return };
    let options = JobOptions {
        samples: 16,
        adaptive: Some(solidcraft_render::job::adaptive::AdaptiveSampling { pilot_samples: 16, min_samples: 16, ..Default::default() }),
        ..Default::default()
    };
    assert_eq!(executor.preview_samples(options), Some(32));
    assert!(executor.preview_unavailable_reason(options).is_none());
    let host = ImageHost::with_executor(Arc::new(executor)).unwrap();
    let job = host
        .submit_with_options(
            solidcraft_render::job::JobId { session: 9023, serial: 1, generation: 1 },
            57,
            ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY },
            request(2),
            options,
        )
        .unwrap();
    assert!(job.status().unwrap().preview_unavailable_reason.is_none());
    let status = job.wait_timeout(Duration::from_secs(30)).unwrap();
    assert_eq!(status.progress.phase, Phase::Completed, "{:?}", status.error);
    assert!(job.preview().unwrap().is_none());
    assert!(job.frame().unwrap().is_some());
}

#[test]
fn gpu_ui_box_coat_and_color_map_keep_every_raw_sample_finite() {
    use image::ImageEncoder;
    use serde_json::json;
    use solidcraft_render::{
        job::textures::{PlanarFrame, RuntimeTextures, TextureBinding, TextureMap},
        texture_input::MapRole,
    };
    use solidcraft_viewport::{clear_coat::ClearCoat, surface::srgb8_to_linear};
    let Some(executor) = executor() else { return };
    let mut session = solidcraft_engine::Session::default();
    session.execute("solid.box", &json!({"length":10,"width":20,"height":5})).unwrap();
    let captured = solidcraft_engine::product_scene::extract(&session).unwrap();
    assert_eq!(captured.meshes.len(), 1);
    let mut encoded_map = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded_map).write_image(&[80, 160, 220, 255], 1, 1, image::ExtendedColorType::Rgba8).unwrap();
    let color_map = TextureMap::new(encoded_map, MapRole::BaseColor).unwrap();
    // Retain the exact camera and request from the UI failure. This exercises
    // the production `main` entry point, not only the BRDF reference kernels.
    let camera = Camera {
        target: Vec3::new(10.525041892255036, 11.6661345783332, -1.3589073139218368),
        yaw: -std::f64::consts::FRAC_PI_4,
        pitch: 0.6154797086703874,
        roll: 0.0,
        distance: 61.06594959585294,
        fov: 0.0,
        up: solidcraft_viewport::UpAxis::Z,
    };
    let options: JobOptions = serde_json::from_value(json!({
        "samples":4,"max_depth":3,"seed":7,"exposure_ev":0.5,"tone_map":"aces_fitted",
        "lens":{"aperture_radius_mm":0.0,"focus_distance_mm":100.0},
        "lighting":{"environment":[0.75,0.75,0.75],"sun":null,"area_lights":[]}
    }))
    .unwrap();
    for (coated, mapped) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut source = captured.clone();
        if coated {
            let coat = ClearCoat::new(1.0, 0.12, 1.5).unwrap();
            let body = &mut source.meshes[0];
            let coat_surface = |color: [u8; 3], surface: Option<OpaqueSurface>| {
                let [r, g, b] = color.map(srgb8_to_linear);
                surface.unwrap_or_else(|| OpaqueSurface::new(Vec3::new(r, g, b), 0.0, 0.5, 1.5).unwrap()).with_clear_coat(coat)
            };
            body.surface = Some(coat_surface(body.color, body.surface));
            for face in &mut body.faces {
                face.surface = Some(coat_surface(face.color, face.surface));
            }
        }
        let textures = mapped.then(|| {
            Arc::new(
                RuntimeTextures::new(vec![TextureBinding {
                    body_index: 0,
                    body_name: source.meshes[0].name.clone(),
                    frame: PlanarFrame { origin: [0.0; 3], u: [10.0, 0.0, 0.0], v: [0.0, 10.0, 0.0] },
                    normal_strength: 1.0,
                    base_color: Some(color_map.clone()),
                    normal: None,
                    roughness: None,
                    metallic: None,
                    base_color_mode: Default::default(),
                    flip_v: false,
                }])
                .unwrap(),
            )
        });
        let input = ImageRequest { camera, background: [236, 240, 245], textures, ..request(16) };
        let mut task = Task::new(executor.context.clone(), source, input, options, CancelToken::default()).unwrap();
        for sample in 1..=options.samples {
            task.complete_sample().unwrap();
            let pixels = task.read_linear().unwrap_or_else(|error| panic!("coat={coated}, map={mapped}, sample={sample}: {error}"));
            assert_eq!(pixels.len(), 256);
            assert!(pixels.iter().flatten().all(|value| value.is_finite() && *value >= 0.0));
            assert!(pixels.iter().all(|pixel| pixel[3] <= 1.0));
            assert!(pixels.iter().any(|pixel| pixel[3] > 0.0 && pixel[..3].iter().any(|value| *value > 0.0)), "the fixture must shade the box");
            // A diagnostic fingerprint for comparing the same test with and
            // without shader instrumentation; not a cross-device golden.
            let fingerprint = pixels
                .iter()
                .flatten()
                .flat_map(|value| value.to_bits().to_le_bytes())
                .fold(0xcbf29ce484222325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3));
            eprintln!("UI box raw float fingerprint coat={coated} map={mapped} sample={sample}: {fingerprint:016x}");
        }
        let mut completed = None;
        for _ in 0..10_000 {
            if let TaskPoll::Complete(png) = task.step(4096).unwrap() {
                completed = Some(png);
                break;
            }
        }
        let image = image::load_from_memory(&completed.expect("bounded GPU box render must produce its final PNG")).unwrap().to_rgba8();
        assert_eq!(image.dimensions(), (16, 16));
        assert!(image.pixels().any(|pixel| pixel.0 != image.get_pixel(0, 0).0));
    }
}

fn directional_receiver() -> ProductScene {
    let mesh = Mesh {
        positions: vec![Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 0.0), Vec3::new(-10.0, 10.0, 0.0)],
        normals: vec![Vec3::Z; 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        tri_face: vec![0, 0],
        ..Default::default()
    };
    ProductScene {
        meshes: vec![ProductMesh {
            name: "directional receiver".into(),
            mesh: Arc::new(mesh),
            color: [255; 3],
            opacity: 1.0,
            surface: Some(white()),
            transmission: None,
            faces: Vec::new(),
        }],
        bounds: Aabb3::EMPTY,
    }
}

#[test]
fn gpu_hard_directional_rgb_sum_matches_f64_brdf_without_emitter_mis_or_sample_count_bias() {
    use solidcraft_render::job::{DirectionalLightOptions, DirectionalLights, StudioLighting};
    let Some(executor) = executor() else { return };
    let sources = [
        DirectionalLightOptions { direction: [0.0, 0.0, 2.0], color: [1.0, 0.0, 0.0], intensity: 2.0 },
        DirectionalLightOptions { direction: [3.0, 0.0, 4.0], color: [0.0, 1.0, 0.0], intensity: 3.0 },
        DirectionalLightOptions { direction: [0.0, -4.0, 3.0], color: [0.0, 0.0, 1.0], intensity: 4.0 },
    ];
    let mut expected = Vec3::ZERO;
    for light in sources {
        let [x, y, z] = light.direction;
        let l = Vec3::new(x, y, z).normalized().unwrap();
        let [r, g, b] = light.color;
        let response = white().brdf(Vec3::Z, Vec3::Z, l).unwrap();
        expected += Vec3::new(response.x * r, response.y * g, response.z * b) * (light.intensity * l.z);
    }
    for depth in [1, 2] {
        for samples in [1, 4] {
            let options = JobOptions {
                samples,
                max_depth: depth,
                seed: 43,
                lighting: Some(StudioLighting { directional_lights: DirectionalLights::new(&sources).unwrap(), ..Default::default() }),
                ..Default::default()
            };
            let mut task = Task::new(executor.context.clone(), directional_receiver(), request(1), options, CancelToken::default()).unwrap();
            while task.sample < samples {
                task.complete_sample().unwrap();
            }
            let actual = task.read_linear().unwrap()[0];
            for (a, b) in actual.iter().zip([expected.x, expected.y, expected.z]) {
                assert!((f64::from(*a) - b).abs() < 2e-6, "depth{depth} samples{samples}: {actual:?} vs {expected:?}");
            }
            assert_eq!(actual[3], 1.0);
        }
    }
}

#[test]
fn gpu_hard_directionals_preserve_zero_source_and_default_streams_and_cancel_without_publication() {
    use solidcraft_render::job::{DirectionalLightOptions, DirectionalLights, LightingPreset, StudioLighting};
    let Some(executor) = executor() else { return };
    let render = |lighting| {
        let options = JobOptions { samples: 4, max_depth: 3, seed: 71, lighting, ..Default::default() };
        let mut task = Task::new(executor.context.clone(), directional_receiver(), request(4), options, CancelToken::default()).unwrap();
        while task.sample < 4 {
            task.complete_sample().unwrap();
        }
        task.read_linear().unwrap()
    };
    let black = render(Some(StudioLighting::default()));
    let zero = DirectionalLightOptions { direction: [1.0, 0.0, 1.0], color: [1.0; 3], intensity: 0.0 };
    assert_eq!(black, render(Some(StudioLighting { directional_lights: DirectionalLights::new(&[zero; 8]).unwrap(), ..Default::default() })));
    let defaults = LightingPreset::Neutral.directional_lights();
    assert_eq!(
        render(None),
        render(Some(StudioLighting { directional_lights: DirectionalLights::new(&defaults).unwrap(), ..Default::default() })),
        "materializing the implicit three hard sources preserves their stream and values"
    );
    let token = CancelToken::default();
    let options = JobOptions {
        lighting: Some(StudioLighting { directional_lights: DirectionalLights::new(&[zero; 8]).unwrap(), ..Default::default() }),
        ..Default::default()
    };
    let mut task = Task::new(executor.context.clone(), directional_receiver(), request(4), options, token.clone()).unwrap();
    token.cancel();
    assert_eq!(task.step(1).err().unwrap().code, ErrorCode::Cancelled);
    assert!(std::mem::size_of::<Task>() <= 65_536, "inline sources must fit the unchanged fixed task reserve");
}

#[test]
fn gpu_hard_directional_uniform_layout_and_admission_keep_the_positive_zero_abi() {
    use solidcraft_render::job::{DirectionalLightOptions, DirectionalLights, StudioLighting};
    let Some(executor) = executor() else { return };
    shader_validates_without_a_device();
    main_inlines_few_bvh_walks();
    let module = naga::front::wgsl::parse_str(SHADER).unwrap();
    let params = module.types.iter().find(|(_, t)| t.name.as_deref() == Some("Params")).unwrap().1;
    let naga::TypeInner::Struct { members, .. } = &params.inner else { panic!("Params") };
    let offset = members.iter().find(|m| m.name.as_deref() == Some("directional_lights")).unwrap().offset as usize;
    let barrier = members.iter().find(|m| m.name.as_deref() == Some("studio_weights")).unwrap().offset as usize + 12;
    let page = members.iter().find(|m| m.name.as_deref() == Some("page")).unwrap().offset as usize;
    assert_eq!(barrier, PARAMETER_BYTES - 20);
    assert_eq!(page, barrier + 4);
    let mut lights = [DirectionalLightOptions { direction: [0.0, 0.0, 2.0], color: [0.25, 0.5, 1.0], intensity: 2.0 }; 8];
    lights[7].direction = [f64::MIN_POSITIVE, 0.0, 0.0];
    let options = JobOptions {
        lighting: Some(StudioLighting { directional_lights: DirectionalLights::new(&lights).unwrap(), ..Default::default() }),
        ..Default::default()
    };
    let task = Task::new(executor.context.clone(), directional_receiver(), request(1), options, CancelToken::default()).unwrap();
    let bytes = task.parameters(1).unwrap();
    assert_eq!(bytes.len(), PARAMETER_BYTES);
    let f = |i| f32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    assert_eq!([f(offset), f(offset + 4), f(offset + 8), f(offset + 12)], [0.0, 0.0, 1.0, 2.0]);
    assert_eq!([f(offset + 16), f(offset + 20), f(offset + 24), f(offset + 28)], [0.25, 0.5, 1.0, 0.0]);
    assert_eq!([f(offset + 7 * 32), f(offset + 7 * 32 + 4), f(offset + 7 * 32 + 8)], [1.0, 0.0, 0.0]);
    assert_eq!(&bytes[barrier..barrier + 4], &0u32.to_le_bytes());
    let word = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    assert_eq!([word(page), word(page + 4), word(page + 8), word(page + 12)], [0, 1, 0, 0]);
    assert_eq!(DIRECTIONAL_PARAMETER_BYTES, 256);
    let admitted = admission(&directional_receiver(), &request(1), options).unwrap();
    assert!(admitted.preparation_bytes >= 1_048_576 + 2 * DIRECTIONAL_PARAMETER_BYTES);
    assert_eq!(
        serde_json::to_value(admitted).unwrap(),
        serde_json::to_value(admission(&directional_receiver(), &request(1), JobOptions::default()).unwrap()).unwrap(),
        "fixed uniform billing is independent of list contents"
    );
}

#[test]
fn gpu_hard_directional_visibility_obeys_opaque_blockers_and_external_beer_attenuation() {
    use solidcraft_render::job::{DirectionalLightOptions, DirectionalLights, StudioLighting, volume::HomogeneousVolume};
    let Some(executor) = executor() else { return };
    let light = DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0, 0.5, 0.25], intensity: 2.0 };
    for (blocker, medium) in [(false, false), (true, false), (false, true)] {
        let mut scene = directional_receiver();
        if blocker {
            let mut plane = scene.meshes[0].clone();
            plane.name = "shadow blocker".into();
            for p in &mut Arc::make_mut(&mut plane.mesh).positions {
                p.z = 1.0;
            }
            scene.meshes.push(plane);
        }
        let options = JobOptions {
            samples: 4,
            max_depth: 1,
            lighting: Some(StudioLighting { directional_lights: DirectionalLights::new(&[light]).unwrap(), ..Default::default() }),
            volume: medium.then_some(HomogeneousVolume {
                min_mm: [-100.0, -100.0, 0.2],
                max_mm: [100.0, 100.0, 0.8],
                extinction_per_mm: 0.5,
                scattering_albedo: [0.0; 3],
                anisotropy: 0.0,
            }),
            ..Default::default()
        };
        let mut image = request(1);
        image.camera.distance = 0.1;
        image.camera.fov = 0.001;
        let mut task = Task::new(executor.context.clone(), scene, image, options, CancelToken::default()).unwrap();
        while task.sample < options.samples {
            task.complete_sample().unwrap();
        }
        let actual = task.read_linear().unwrap()[0];
        let visibility = if blocker {
            0.0
        } else if medium {
            (-0.3_f64).exp()
        } else {
            1.0
        };
        for (a, scale) in actual.iter().zip([1.0, 0.5, 0.25]) {
            let expected = 2.0 * scale * visibility / std::f64::consts::PI;
            assert!((f64::from(*a) - expected).abs() < 2e-6, "blocker{blocker} volume{medium}: {actual:?} vs{expected}");
        }
    }
}

fn punctual_options(sources: &[solidcraft_render::job::PunctualLightOptions], samples: u32, depth: u32) -> JobOptions {
    use solidcraft_render::job::{PunctualLights, StudioLighting};
    JobOptions {
        samples,
        max_depth: depth,
        seed: 43,
        lighting: Some(StudioLighting { punctual_lights: PunctualLights::new(sources).unwrap(), ..Default::default() }),
        ..Default::default()
    }
}
fn punctual_point() -> solidcraft_render::job::PunctualLightOptions {
    solidcraft_render::job::PunctualLightOptions {
        position_mm: [0.0, 0.0, 4.0],
        color: [0.25, 0.5, 1.0],
        intensity: 2.0,
        reference_distance_mm: 4.0,
        minimum_distance_mm: 0.001,
        cone: None,
    }
}
// Independent scalar law; does not call the CPU/GPU punctual sampling helpers.
fn punctual_oracle(source: solidcraft_render::job::PunctualLightOptions, position: Vec3) -> (Vec3, Vec3) {
    let [x, y, z] = source.position_mm;
    let delta = Vec3::new(x, y, z) - position;
    let distance = delta.len();
    let direction = delta / distance;
    let weight = source.cone.map_or(1.0, |cone| {
        let [x, y, z] = cone.emitted_axis;
        let axis = Vec3::new(x, y, z).normalized().unwrap();
        let t = ((axis.dot(-direction) - cone.outer_half_angle_radians.cos())
            / (cone.inner_half_angle_radians.cos() - cone.outer_half_angle_radians.cos()))
        .clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    });
    let [r, g, b] = source.color;
    (direction, Vec3::new(r, g, b) * (source.intensity * (source.reference_distance_mm / distance).powi(2) * weight))
}
fn punctual_primary_point(camera: Camera, ordinal: u32, seed: u32) -> Vec3 {
    let mut state = ordinal.wrapping_mul(0x9e3779b9) ^ seed;
    let mut random = || {
        state = state.wrapping_mul(747796405).wrapping_add(2891336453);
        let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277803737);
        f64::from((((word >> 22) ^ word) >> 8) as f32 * (1.0 / 16777216.0))
    };
    let x = random() * 2.0 - 1.0;
    let y = 1.0 - random() * 2.0;
    let (right, up, back) = camera.basis();
    let origin = camera.target + back * (camera.distance * 4.0) + (right * x + up * y) * camera.half_height();
    origin - back * (origin.z / back.z)
}
#[test]
fn gpu_point_and_spot_real_primary_rays_match_inverse_square_rgb_cone_and_endpoint_depth() {
    use solidcraft_render::job::SpotConeOptions;
    let Some(executor) = executor() else { return };
    for cone in [None, Some(SpotConeOptions { emitted_axis: [0.0, 0.0, -1.0], inner_half_angle_radians: 0.2, outer_half_angle_radians: 0.9 })] {
        let source = solidcraft_render::job::PunctualLightOptions { cone, ..punctual_point() };
        for depth in [1, 2] {
            let options = punctual_options(&[source], 16, depth);
            let image = request(1);
            let mut task = Task::new(executor.context.clone(), directional_receiver(), image.clone(), options, CancelToken::default()).unwrap();
            let mut expected = Vec3::ZERO;
            for ordinal in 0..16 {
                let position = punctual_primary_point(image.camera, ordinal, options.seed);
                let (direction, incident) = punctual_oracle(source, position);
                let brdf = white().brdf(Vec3::Z, image.camera.back(), direction).unwrap();
                expected += Vec3::new(brdf.x * incident.x, brdf.y * incident.y, brdf.z * incident.z) * direction.z;
                task.complete_sample().unwrap();
                let actual = task.read_linear().unwrap()[0];
                for (a, b) in actual.iter().zip([expected.x, expected.y, expected.z].map(|v| v / f64::from(ordinal + 1))) {
                    assert!((f64::from(*a) - b).abs() < 3e-6, "ordinal{ordinal} depth{depth} cone{cone:?}: {actual:?} vs {expected:?}");
                }
                assert_eq!(actual[3], 1.0);
            }
            assert_eq!(task.punctual_diagnostics(), Some(Default::default()));
        }
    }
}
#[test]
fn gpu_punctual_uniform_high_low_precision_and_admission_keep_studio_barrier_before_page_metadata() {
    use solidcraft_render::job::PunctualLightOptions;
    shader_validates_without_a_device();
    main_inlines_few_bvh_walks();
    let light = PunctualLightOptions { position_mm: [1e9 - 0.01, -1e9 + 0.02, 0.003], ..punctual_point() };
    let packed = punctual_parameters(&light, Vec3::ZERO);
    for (axis, expected) in light.position_mm.into_iter().enumerate() {
        let actual = f64::from(packed[0][axis]) + f64::from(packed[1][axis]);
        assert!((actual - expected).abs() < 1e-6, "axis{axis}: {actual} vs {expected}");
    }
    assert_eq!(packed[0][0], 1e9_f32);
    assert!(packed[1][0] < -0.009 && packed[1][0] > -0.011);
    let module = naga::front::wgsl::parse_str(SHADER).unwrap();
    let params = module.types.iter().find(|(_, t)| t.name.as_deref() == Some("Params")).unwrap().1;
    let naga::TypeInner::Struct { members, span } = &params.inner else { panic!("Params") };
    assert_eq!(*span as usize, PARAMETER_BYTES);
    let offset = members.iter().find(|m| m.name.as_deref() == Some("punctual_lights")).unwrap().offset;
    let barrier = members.iter().find(|m| m.name.as_deref() == Some("studio_weights")).unwrap().offset + 12;
    assert_eq!(offset as usize, PARAMETER_BYTES - PUNCTUAL_PARAMETER_BYTES as usize - 32);
    assert_eq!(barrier as usize, PARAMETER_BYTES - 20);
    let page = members.iter().find(|m| m.name.as_deref() == Some("page")).unwrap().offset;
    assert_eq!(page as usize, PARAMETER_BYTES - 16);
    assert_eq!(FAILURE_BYTES, 32 + PUNCTUAL_COUNTER_BYTES);
    let a = admission(&directional_receiver(), &request(1), JobOptions::default()).unwrap();
    let b = admission(&directional_receiver(), &request(1), punctual_options(&[light; 8], 1, 1)).unwrap();
    assert_eq!(serde_json::to_value(a).unwrap(), serde_json::to_value(b).unwrap());
    assert!(a.preparation_bytes >= 1_048_576 + 2 * (DIRECTIONAL_PARAMETER_BYTES + PUNCTUAL_PARAMETER_BYTES + PUNCTUAL_COUNTER_BYTES));
    assert_eq!(punctual_options(&[light; 8], 1, 1).retained_bytes().unwrap(), std::mem::size_of::<JobOptions>() as u64);
    assert!(std::mem::size_of::<Task>() <= 65_536);
}
#[test]
fn gpu_punctual_delta_sources_preserve_zero_default_stream_and_cancel_without_publication() {
    use solidcraft_render::job::{DirectionalLights, LightingPreset, StudioLighting};
    let Some(executor) = executor() else { return };
    let render = |options| {
        let mut task = Task::new(executor.context.clone(), directional_receiver(), request(4), options, CancelToken::default()).unwrap();
        for _ in 0..4 {
            task.complete_sample().unwrap();
        }
        task.read_linear().unwrap()
    };
    let zero = solidcraft_render::job::PunctualLightOptions { intensity: 0.0, ..punctual_point() };
    let old = JobOptions { samples: 4, max_depth: 3, seed: 71, ..Default::default() };
    let mut explicit = punctual_options(&[zero; 8], 4, 3);
    explicit.seed = old.seed;
    let lighting = explicit.lighting.as_mut().unwrap();
    lighting.directional_lights = DirectionalLights::new(&LightingPreset::Neutral.directional_lights()).unwrap();
    assert_eq!(render(old), render(explicit), "zero point/spot sources consume no random values and preserve implicit-three lighting");
    assert_eq!(
        render(JobOptions { lighting: Some(StudioLighting::default()), ..old }),
        render(JobOptions { lighting: punctual_options(&[zero; 8], 4, 3).lighting, ..old })
    );
    // Existing stochastic families draw exactly the same continuation stream
    // when a zero-strength delta family is appended after them.
    let mut mixed = old;
    mixed.lighting = Some(StudioLighting {
        sun: Some(solidcraft_render::job::Sun { direction: [0.0, 0.0, 1.0], angular_radius_rad: 0.2, radiance: [2.0; 3] }),
        directional_lights: DirectionalLights::new(&LightingPreset::Neutral.directional_lights()).unwrap(),
        environment: [0.2; 3],
        area_lights: solidcraft_render::job::AreaLights::new(&[solidcraft_render::job::AreaLight {
            center: [1.0, 0.0, 4.0],
            u: [1.0, 0.0, 0.0],
            v: [0.0, -1.0, 0.0],
            radiance: [1.0, 2.0, 3.0],
        }])
        .unwrap(),
        ..Default::default()
    });
    let mut with_zero = mixed;
    with_zero.lighting.as_mut().unwrap().punctual_lights = solidcraft_render::job::PunctualLights::new(&[zero; 8]).unwrap();
    assert_eq!(render(mixed), render(with_zero));
    let cancel = CancelToken::default();
    let mut task = Task::new(executor.context.clone(), directional_receiver(), request(4), explicit, cancel.clone()).unwrap();
    cancel.cancel();
    assert_eq!(task.step(1).err().unwrap().code, ErrorCode::Cancelled);
    assert!(task.take_preview().is_none() && task.take_linear_exr().is_none());
}
#[test]
fn gpu_punctual_finite_shadow_blocks_only_before_the_source_and_preserves_physical_falloff() {
    let Some(executor) = executor() else { return };
    let light = solidcraft_render::job::PunctualLightOptions { position_mm: [4.0, 0.0, 4.0], ..punctual_point() };
    for blocker_z in [None, Some(2.0), Some(4.0), Some(6.0)] {
        let mut scene = directional_receiver();
        if let Some(z) = blocker_z {
            let mut blocker = scene.meshes[0].clone();
            blocker.name = "finite shadow panel".into();
            let mesh = Arc::make_mut(&mut blocker.mesh);
            mesh.positions = vec![Vec3::new(z - 0.5, -1.0, z), Vec3::new(z + 0.5, -1.0, z), Vec3::new(z + 0.5, 1.0, z), Vec3::new(z - 0.5, 1.0, z)];
            scene.meshes.push(blocker);
        }
        let options = punctual_options(&[light], 4, 1);
        let mut image = request(1);
        image.camera.distance = 0.1;
        let mut task = Task::new(executor.context.clone(), scene, image.clone(), options, CancelToken::default()).unwrap();
        let mut expected = Vec3::ZERO;
        for ordinal in 0..4 {
            let p = punctual_primary_point(image.camera, ordinal, options.seed);
            let (l, incident) = punctual_oracle(light, p);
            let brdf = white().brdf(Vec3::Z, image.camera.back(), l).unwrap();
            if blocker_z != Some(2.0) {
                expected += Vec3::new(brdf.x * incident.x, brdf.y * incident.y, brdf.z * incident.z) * l.z;
            }
            task.complete_sample().unwrap();
        }
        let actual = task.read_linear().unwrap()[0];
        for (a, b) in actual.iter().zip([expected.x, expected.y, expected.z].map(|v| v / 4.0)) {
            assert!((f64::from(*a) - b).abs() < 3e-6, "blocker {blocker_z:?}: {actual:?} vs {expected:?}");
        }
    }
}

#[test]
fn gpu_punctual_samples_match_independent_cone_boundaries_midpoint_high_low_and_counter_carry() {
    use solidcraft_render::job::SpotConeOptions;
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    // A test-only entry calls the production sampler at an independently given
    // physical point; it does not launch a camera ray or copy the sampler math.
    let source = format!(
        "{SHADER}\n@compute @workgroup_size(1) fn punctual_reference() {{ let value=punctual_sample(u.punctual_lights[0],PrecisePoint(u.eye.xyz,u.right.xyz)); accumulation[0]=vec4<f32>(value.radiance,1.0); }}"
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
        entry_point: Some("punctual_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    let run = |light, point: Vec3, carry| {
        let mut task =
            Task::new(context.clone(), directional_receiver(), request(1), punctual_options(&[light], 1, 1), CancelToken::default()).unwrap();
        let mut params = task.parameters(1).unwrap();
        let hi = Vec3::new(f64::from(point.x as f32), f64::from(point.y as f32), f64::from(point.z as f32));
        let mut positions = Vec::new();
        scene::vector(&mut positions, hi, 0.0);
        scene::vector(&mut positions, point - hi, 0.0);
        params[32..64].copy_from_slice(&positions);
        context.queue.write_buffer(&task.uniform, 0, &params);
        if carry {
            let mut bytes = [0; FAILURE_BYTES as usize];
            bytes[8 * 4..9 * 4].copy_from_slice(&u32::MAX.to_le_bytes());
            context.queue.write_buffer(&task.failures, 0, &bytes);
        }
        let mut encoder = context.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &task.bind, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        context.queue.submit([encoder.finish()]);
        task.sample = 1;
        (task.read_linear().unwrap()[0], task.punctual_diagnostics().unwrap())
    };
    let cone = SpotConeOptions { emitted_axis: [0.0, 0.0, -3.0], inner_half_angle_radians: 0.2, outer_half_angle_radians: 0.8 };
    let light = solidcraft_render::job::PunctualLightOptions { cone: Some(cone), ..punctual_point() };
    let midpoint = ((cone.inner_half_angle_radians.cos() + cone.outer_half_angle_radians.cos()) * 0.5).acos();
    for angle in [0.0, cone.inner_half_angle_radians, midpoint, cone.outer_half_angle_radians, 1.0] {
        let point = Vec3::new(4.0 * angle.sin(), 0.0, 4.0 - 4.0 * angle.cos());
        let actual = run(light, point, false);
        let expected = punctual_oracle(light, point).1;
        for (a, b) in actual.0.iter().zip([expected.x, expected.y, expected.z]) {
            assert!((f64::from(*a) - b).abs() < 2e-6, "angle{angle}: {:?} vs {expected:?}", actual.0);
        }
        assert!(actual.1.is_empty());
    }
    let far = solidcraft_render::job::PunctualLightOptions { position_mm: [1e9 - 0.01, 0.0, 0.0], reference_distance_mm: 0.01, ..punctual_point() };
    let point = Vec3::new(1e9 - 0.02, 0.0, 0.0);
    let actual = run(far, point, false);
    let expected = punctual_oracle(far, point).1;
    for (a, b) in actual.0.iter().zip([expected.x, expected.y, expected.z]) {
        assert!((f64::from(*a) - b).abs() < 2e-6, "nearby large-world points {:?} vs {expected:?}", actual.0);
    }
    assert!(actual.1.is_empty());
    let near = solidcraft_render::job::PunctualLightOptions { position_mm: [0.0; 3], intensity: 0.0, ..punctual_point() };
    let actual = run(near, Vec3::ZERO, true);
    assert_eq!(actual.0, [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(actual.1.near_source, 1_u64 << 32, "near-source counting precedes zero-intensity omission and carries into the high word");
    let excessive = solidcraft_render::job::PunctualLightOptions {
        position_mm: [0.0, 0.0, 0.002],
        reference_distance_mm: 1e6,
        intensity: 64.0,
        ..punctual_point()
    };
    let actual = run(excessive, Vec3::ZERO, false);
    assert_eq!(actual.0, [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(actual.1.radiance_limit, 1);
    assert_eq!(actual.1.near_source, 0);
}
#[test]
fn gpu_punctual_shadow_offset_crossing_is_reported_without_reversing_or_infinite_connection() {
    let Some(executor) = executor() else { return };
    let light = solidcraft_render::job::PunctualLightOptions { position_mm: [0.0, 0.0, 0.01], ..punctual_point() };
    let mut image = request(1);
    image.camera.distance = 0.001;
    // Keep the original tiny orthographic pixel footprint. Move only its axial
    // framing target so the actual GPU primary origin is before the receiver:
    // target.z + 4*distance = 1.004, beyond the explicit0.1 primary minimum.
    // The former origin0.004 was clipped before any shadow connection existed.
    image.camera.target.z = 1.0;
    assert_eq!(image.camera.fov, 0.0);
    assert_eq!(image.camera.target.z + 4.0 * image.camera.distance, 1.004);
    let mut control =
        Task::new(executor.context.clone(), directional_receiver(), image.clone(), punctual_options(&[], 1, 1), CancelToken::default()).unwrap();
    control.epsilon = 0.1;
    assert!(image.camera.target.z + 4.0 * image.camera.distance > f64::from(control.epsilon));
    control.complete_sample().unwrap();
    assert_eq!(control.read_linear().unwrap()[0], [0.0, 0.0, 0.0, 1.0], "the actual primary sees opaque geometry without any light source");
    assert!(control.punctual_diagnostics().is_none(), "an authored empty punctual-light family has no diagnostic counters");
    drop(control);
    let mut task =
        Task::new(executor.context.clone(), directional_receiver(), image, punctual_options(&[light], 4, 1), CancelToken::default()).unwrap();
    task.epsilon = 0.1;
    for _ in 0..4 {
        task.complete_sample().unwrap();
    }
    assert_eq!(task.read_linear().unwrap()[0], [0.0, 0.0, 0.0, 1.0]);
    let counts = task.punctual_diagnostics().unwrap();
    assert_eq!(counts.shadow_offset_crossed, 4);
    assert_eq!(counts.near_source, 0);
    assert_eq!(counts.radiance_limit, 0);
}
#[test]
fn gpu_point_delta_visibility_keeps_glass_and_external_beer_without_competing_mis_bias() {
    let Some(executor) = executor() else { return };
    let light = solidcraft_render::job::PunctualLightOptions { position_mm: [4.0, 0.0, 4.0], ..punctual_point() };
    for glass in [false, true] {
        let mut scene = directional_receiver();
        let optics = solidcraft_render::transmission::Transmission::new(Vec3::new(0.1, 0.2, 0.3), Some(0.7)).unwrap().with_shadow_transmission(true);
        if glass {
            let mut panel = scene.meshes[0].clone();
            panel.name = "point connection glass".into();
            Arc::make_mut(&mut panel.mesh).positions =
                vec![Vec3::new(1.0, -2.0, 2.0), Vec3::new(3.0, -2.0, 2.0), Vec3::new(3.0, 2.0, 2.0), Vec3::new(1.0, 2.0, 2.0)];
            panel.surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap());
            panel.transmission = Some(optics);
            scene.meshes.push(panel);
        }
        let mut options = punctual_options(&[light], 4, 1);
        if !glass {
            options.volume = Some(solidcraft_render::job::volume::HomogeneousVolume {
                min_mm: [-10.0, -10.0, 1.0],
                max_mm: [10.0, 10.0, 3.0],
                extinction_per_mm: 0.1,
                scattering_albedo: [0.0; 3],
                anisotropy: 0.0,
            });
        }
        let mut image = request(1);
        image.camera.distance = 0.1;
        let mut task = Task::new(executor.context.clone(), scene, image.clone(), options, CancelToken::default()).unwrap();
        assert_eq!(options.seed, 43);
        assert_eq!(image.camera.fov, 0.0);
        let primary_fog_length = if let Some(volume) = options.volume {
            // Top retains a slight f64 tilt. The GPU uses the independently
            // packed f32 basis, and orthographic primary_ray does not normalize
            // its -back direction again. Verify those actual upload words.
            let (right, up, back) = image.camera.basis();
            let expected_vectors = [
                back * (4.0 * image.camera.distance),
                right * image.camera.half_height() * (f64::from(image.width) / f64::from(image.height)),
                up * image.camera.half_height(),
                back,
            ];
            let params = task.parameters(1).unwrap();
            let vectors: [[f64; 3]; 4] = std::array::from_fn(|vector| {
                std::array::from_fn(|axis| {
                    let at = 32 + vector * 16 + axis * 4;
                    let packed = f32::from_le_bytes(params[at..at + 4].try_into().unwrap());
                    let expected = expected_vectors[vector];
                    assert_eq!(packed.to_bits(), ([expected.x, expected.y, expected.z][axis] as f32).to_bits());
                    f64::from(packed)
                })
            });
            assert_eq!(&params[44..48], &0.0_f32.to_le_bytes(), "the actual GPU projection is orthographic");
            let [eye, right, up, back] = vectors;
            assert_eq!(back[2], f64::from(1.0_f32));
            assert_eq!(image.camera.target, Vec3::ZERO, "the box and plane use these same camera-relative coordinates");
            // For |uv|<=1, two products and two additions form each origin
            // component. 4*EPSILON bounds their gamma4 absolute rounding error
            // (also conservatively covers fused evaluation); no image tolerance
            // changes. This includes every jittered pixel, not just its centre.
            let origin_error: [f64; 3] =
                std::array::from_fn(|axis| 4.0 * f64::from(f32::EPSILON) * (eye[axis].abs() + right[axis].abs() + up[axis].abs()));
            let origin_min_z = eye[2] - right[2].abs() - up[2].abs() - origin_error[2];
            let origin_max_z = eye[2] + right[2].abs() + up[2].abs() + origin_error[2];
            assert!(
                origin_min_z > 0.0 && origin_max_z < volume.min_mm[2],
                "every geometry origin is above the receiver and below the complete fog slab"
            );
            assert!(origin_min_z / back[2] > f64::from(task.epsilon), "the original forward interval still reaches the receiver");
            let farthest_signed_entry = (volume.max_mm[2] - origin_min_z) / back[2];
            for axis in [0, 1] {
                let absolute_bound =
                    eye[axis].abs() + right[axis].abs() + up[axis].abs() + origin_error[axis] + back[axis].abs() * farthest_signed_entry;
                assert!(
                    -absolute_bound > volume.min_mm[axis] && absolute_bound < volume.max_mm[axis],
                    "the full signed chord remains inside the finite box laterally"
                );
            }
            // Origin displacement cancels between the two z-plane endpoints.
            let chord = (volume.max_mm[2] - volume.min_mm[2]) / back[2];
            assert_eq!(chord, 2.0);
            chord
        } else {
            0.0
        };
        let mut expected = Vec3::ZERO;
        let mut survivors = [true; 4];
        for ordinal in 0..4 {
            let p = punctual_primary_point(image.camera, ordinal, options.seed);
            let (direction, incident) = punctual_oracle(light, p);
            let brdf = white().brdf(Vec3::Z, image.camera.back(), direction).unwrap();
            let visibility = if glass {
                solidcraft_render::transmission::slab(optics, 0.7, direction.z, 1.0, 1.5).unwrap().1
            } else {
                Vec3::new(1.0, 1.0, 1.0) * (-0.1 * 2.0 / direction.z).exp()
            };
            if !glass {
                // Independent scalar PCG law: the first two draws belong to XY;
                // the third is the primary analog free flight. No renderer RNG
                // or volume helper defines this survival oracle.
                let mut state = ordinal.wrapping_mul(0x9e3779b9) ^ options.seed;
                let mut draws = [0.0_f64; 3];
                for draw in &mut draws {
                    state = state.wrapping_mul(747796405).wrapping_add(2891336453);
                    let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277803737);
                    *draw = f64::from((((word >> 22) ^ word) >> 8) as f32 * (1.0 / 16777216.0));
                }
                let travel = -(-draws[2]).ln_1p() / 0.1;
                let survives = travel >= primary_fog_length;
                assert_eq!(survives, ordinal < 3, "seed43 supplies three survived flights followed by one absorption");
                assert!((travel - primary_fog_length).abs() > 1.8, "the independent decision is separated from f32 rounding: {travel}");
                survivors[ordinal as usize] = survives;
            }
            // Finite source visibility keeps its own Beer/slab factor above.
            // Camera Beer is supplied by analog survival, not another exp factor.
            if survivors[ordinal as usize] {
                expected += Vec3::new(brdf.x * incident.x * visibility.x, brdf.y * incident.y * visibility.y, brdf.z * incident.z * visibility.z)
                    * direction.z;
            }
            task.complete_sample().unwrap();
            let actual = task.read_linear().unwrap()[0];
            for (a, b) in actual.iter().zip([expected.x, expected.y, expected.z].map(|v| v / f64::from(ordinal + 1))) {
                assert!((f64::from(*a) - b).abs() < 5e-6, "glass{glass} ordinal{ordinal}: {actual:?} vs prefix sum{expected:?}");
            }
            assert_eq!(actual[3], 1.0, "opaque receiver or absorbed foreground fog covers every prefix");
        }
        if !glass {
            assert_eq!(survivors, [true, true, true, false]);
        }
    }
}
#[test]
fn gpu_point_spot_complete_previews_match_prefixes_and_preserve_exact_final_png_raw_exr() {
    use solidcraft_render::job::SpotConeOptions;
    let Some(executor) = executor() else { return };
    let sources = [
        punctual_point(),
        solidcraft_render::job::PunctualLightOptions {
            position_mm: [2.0, 0.0, 4.0],
            cone: Some(SpotConeOptions { emitted_axis: [-2.0, 0.0, -4.0], inner_half_angle_radians: 0.2, outer_half_angle_radians: 0.8 }),
            ..punctual_point()
        },
    ];
    let options = JobOptions { linear_exr: true, ..punctual_options(&sources, 4, 2) };
    let render = |samples, previews, units| {
        let mut task =
            Task::new(executor.context.clone(), directional_receiver(), request(4), JobOptions { samples, ..options }, CancelToken::default())
                .unwrap();
        task.preview_enabled = previews;
        let mut held = Vec::new();
        for _ in 0..20_000 {
            let poll = task.step(units).unwrap();
            if let Some(preview) = task.take_preview() {
                assert!(task.exr.is_none());
                held.push(preview);
            }
            if let TaskPoll::Complete(png) = poll {
                return (held, png, task.take_linear_exr().unwrap(), task.punctual_diagnostics().unwrap());
            }
        }
        panic!("bounded point/spot output did not finish")
    };
    let (previews, png, exr, counts) = render(4, true, 1);
    assert_eq!(previews.iter().map(|p| p.samples_per_pixel).collect::<Vec<_>>(), [1, 2, 3]);
    assert!(counts.is_empty());
    for preview in previews {
        assert_eq!(preview.png, render(preview.samples_per_pixel, false, 4096).1);
    }
    let reference = render(4, false, 4096);
    assert_eq!(png, reference.1);
    assert_eq!(exr, reference.2);
    assert_eq!(counts, reference.3);
}

#[test]
fn gpu_punctual_volume_connection_matches_independent_hg_and_finite_beer_without_rng_consumption() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let source = format!(
        "{SHADER}\n@compute @workgroup_size(1) fn punctual_volume_reference() {{ var rng=123u; let value=volume_direct(u.eye.xyz,PrecisePoint(u.eye.xyz,vec3<f32>(0.0)),vec3<f32>(0.0,0.0,1.0),0u,&rng); accumulation[0]=vec4<f32>(value,f32(rng)); }}"
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
        entry_point: Some("punctual_volume_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    for cone in [
        None,
        Some(solidcraft_render::job::SpotConeOptions {
            emitted_axis: [-1.0, 0.0, -2.0],
            inner_half_angle_radians: 0.2,
            outer_half_angle_radians: 0.8,
        }),
    ] {
        let light = solidcraft_render::job::PunctualLightOptions { position_mm: [1.0, 0.0, 4.0], cone, ..punctual_point() };
        let options = JobOptions {
            volume: Some(solidcraft_render::job::volume::HomogeneousVolume {
                min_mm: [-10.0, -10.0, 1.0],
                max_mm: [10.0, 10.0, 3.0],
                extinction_per_mm: 0.1,
                scattering_albedo: [0.8; 3],
                anisotropy: 0.3,
            }),
            ..punctual_options(&[light], 1, 1)
        };
        let mut task = Task::new(context.clone(), directional_receiver(), request(1), options, CancelToken::default()).unwrap();
        let point = Vec3::new(0.0, 0.0, 2.0);
        let mut params = task.parameters(1).unwrap();
        let mut position = Vec::new();
        scene::vector(&mut position, point, 0.0);
        params[32..48].copy_from_slice(&position);
        context.queue.write_buffer(&task.uniform, 0, &params);
        let mut encoder = context.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &task.bind, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        context.queue.submit([encoder.finish()]);
        task.sample = 1;
        let actual = task.read_linear().unwrap()[0];
        let (l, incident) = punctual_oracle(light, point);
        let g = 0.3_f64;
        let denominator = 1.0 + g * g - 2.0 * g * l.z;
        let phase = (1.0 - g * g) / (4.0 * std::f64::consts::PI * denominator.powf(1.5));
        let expected = incident * ((-0.1 / l.z).exp() * phase);
        for (a, b) in actual.iter().zip([expected.x, expected.y, expected.z]) {
            assert!((f64::from(*a) - b).abs() < 2e-6, "{actual:?} vs {expected:?}");
        }
        assert_eq!(actual[3], 123.0, "point and spot delta connections consume no random values");
        assert!(task.punctual_diagnostics().unwrap().is_empty());
    }
}
#[test]
fn gpu_adaptive_denoised_point_spot_updates_preserve_every_pilot_production_pass_and_final_bytes() {
    use solidcraft_render::job::{SpotConeOptions, adaptive::AdaptiveSampling};
    let Some(executor) = executor() else { return };
    let light = solidcraft_render::job::PunctualLightOptions {
        cone: Some(SpotConeOptions { emitted_axis: [0.0, 0.0, -1.0], inner_half_angle_radians: 0.2, outer_half_angle_radians: 0.8 }),
        ..punctual_point()
    };
    for denoise in [false, true] {
        let options = JobOptions {
            denoise,
            linear_exr: true,
            adaptive: Some(AdaptiveSampling { pilot_samples: 16, min_samples: 16, ..Default::default() }),
            ..punctual_options(&[light], 16, 2)
        };
        let render = |preview_enabled| {
            let mut task = Task::new(executor.context.clone(), directional_receiver(), request(1), options, CancelToken::default()).unwrap();
            task.preview_enabled = preview_enabled;
            let mut sequences = Vec::new();
            for _ in 0..30_000 {
                let poll = task.step(4096).unwrap();
                if let Some(preview) = task.take_preview() {
                    sequences.push(preview.samples_per_pixel);
                    assert!(task.exr.is_none());
                }
                if let TaskPoll::Complete(png) = poll {
                    return (sequences, png, task.take_linear_exr().unwrap(), task.sampling_report().unwrap(), task.punctual_diagnostics().unwrap());
                }
            }
            panic!("bounded adaptive punctual fixture did not finish")
        };
        let previewed = render(true);
        let final_only = render(false);
        assert_eq!(previewed.0, (1..32).collect::<Vec<_>>());
        assert!(final_only.0.is_empty());
        assert_eq!(previewed.1, final_only.1);
        assert_eq!(previewed.2, final_only.2);
        assert_eq!(previewed.3, final_only.3);
        assert_eq!(previewed.4, final_only.4);
        assert!(previewed.4.is_empty());
        assert_eq!(previewed.3.primary_path_evaluations, 32);
    }
}

#[test]
fn gpu_punctual_offset_preserves_small_shift_after_large_local_barycentric_displacement() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let source = format!(
        "{SHADER}\n@compute @workgroup_size(1) fn punctual_offset_reference() {{ let original=PrecisePoint(vec3<f32>(1000000.0,0.0,0.0),vec3<f32>(-999999.0,0.0,0.0)); let shifted=punctual_offset(original,vec3<f32>(0.00001,0.0,0.0)); let canonical=punctual_offset(original,vec3<f32>(0.0)); let delta=precise_point_delta(shifted.hi,shifted.lo,canonical.hi,canonical.lo); accumulation[0]=vec4<f32>(delta,1.0); let light=punctual_sample(u.punctual_lights[0],original); accumulation[1]=vec4<f32>(light.radiance,1.0); }}"
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
        entry_point: Some("punctual_offset_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    let light = solidcraft_render::job::PunctualLightOptions { position_mm: [1.01, 0.0, 0.0], reference_distance_mm: 0.01, ..punctual_point() };
    let mut task = Task::new(context.clone(), directional_receiver(), request(2), punctual_options(&[light], 1, 1), CancelToken::default()).unwrap();
    context.queue.write_buffer(&task.uniform, 0, &task.parameters(1).unwrap());
    let mut encoder = context.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &task.bind, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    context.queue.submit([encoder.finish()]);
    task.sample = 1;
    let pixels = task.read_linear().unwrap();
    assert_eq!(pixels[0], [0.00001, 0.0, 0.0, 1.0]);
    let expected = punctual_oracle(light, Vec3::new(1.0, 0.0, 0.0)).1;
    for (actual, expected) in pixels[1].iter().zip([expected.x, expected.y, expected.z]) {
        assert!((f64::from(*actual) - expected).abs() < 2e-6, "the sampler must canonicalize large local displacement before source subtraction");
    }
    assert_eq!(-999999.0_f32 + 0.00001, -999999.0_f32, "the raw local addition would lose the real geometric shift");
}

#[test]
fn shared_bvh_node_walk_preserves_original_ordinary_and_compensated_hit_bits() {
    // Frozen f53 policy, not re-derived from the new implementation. Only its
    // test-local function name changes; it still calls the byte-identical old
    // ordered walker for zero low parts. This reference entry inlines three
    // real walks, rather than compiling the historical seven-walk main.
    let legacy = r#"fn legacy_precise_policy(o: vec3<f32>, o_low: vec3<f32>, d: vec3<f32>, any_hit: bool, excluded: u32, minimum: f32, inside: bool, limit: f32, blocker: f32) -> Hit {
    // Dispatch once before traversal. Ordinary rays keep the original lean BVH
    // loop, without precision-helper branches inside every candidate/node test.
    if all(o_low==vec3<f32>(0.0)) { return trace_ordered_within(o,d,any_hit,excluded,minimum,inside,limit,blocker); }
    var hit = Hit(limit, 0xffffffffu, vec3<f32>(0.0));
    // Project every candidate into the same ray-aligned 2D coordinate system.
    // Shared edges then use identical products with opposite signs, avoiding the
    // independently rounded barycentric comparisons of Moller-Trumbore.
    var kz = 0u;
    if abs(d.y)>abs(d.x) { kz=1u; }
    if abs(d.z)>abs(d[kz]) { kz=2u; }
    let kx=(kz+1u)%3u; let ky=(kx+1u)%3u;
    let shear=vec3<f32>(d[kx]/d[kz],d[ky]/d[kz],1.0/d[kz]);
    let order=select(-1.0,1.0,d[kz]>0.0);
    var best=FAR;
    // Only candidates near the origin need the integer-defined deltas. The
    // rounded origin is within a few ULPs of the true anchor plus offset, so
    // boxes grown by that bound reject conservatively: every point the true
    // ray reaches at depth t lies in the grown box along the rounded ray at
    // the same t. A triangle whose bounds are farther from the rounded origin
    // than the near radius uses ordinary arithmetic from it: that error cannot
    // reorder such a hit, but it can move the ray across an edge, so decisions
    // within the error bound of an edge or bound fall back to the exact path.
    let rounded=o+o_low;
    let scale=max(max(abs(rounded.x),max(abs(rounded.y),abs(rounded.z))),1.0);
    let origin_error=scale*(1.0/1048576.0);
    let pad=u.lighting.x+origin_error;
    var source: Triangle;
    if excluded!=0xffffffffu { source=triangle(excluded); }
    let near_radius=scale*(1.0/256.0);
    // Children are tested at their parent and visited near-first. The build
    // bounds tree depth to 30, so at most 31 nodes are pending.
    var stack: array<u32, 32>;
    var entry: array<f32, 32>;
    if nodes[0].first == 0xffffffffu { return hit; }
    entry[0] = box_entry_padded(nodes[0], rounded, d, hit.distance, pad);
    if entry[0] < 0.0 { return hit; }
    stack[0] = 0u;
    var size = 1u;
    loop {
        if size == 0u { break; }
        size -= 1u;
        if entry[size] > hit.distance { continue; }
        let node = nodes[stack[size]];
        if node.count == 0u {
            if size + 2u > 32u { break; }
            let left = box_entry_padded(nodes[node.first], rounded, d, hit.distance, pad);
            let right = box_entry_padded(nodes[node.first+1u], rounded, d, hit.distance, pad);
            var near_child = node.first; var near_entry = left;
            var far_child = node.first+1u; var far_entry = right;
            if right >= 0.0 && (left < 0.0 || right < left) {
                near_child = node.first+1u; near_entry = right;
                far_child = node.first; far_entry = left;
            }
            if far_entry >= 0.0 { stack[size] = far_child; entry[size] = far_entry; size += 1u; }
            if near_entry >= 0.0 { stack[size] = near_child; entry[size] = near_entry; size += 1u; }
            continue;
        }
        for (var i = node.first; i < node.first + node.count; i += 1u) {
            if i == excluded { continue; }
            // A straight continuation cannot revisit its producing exact plane.
            // IDs join only connected, exactly coplanar uploaded f32 triangles
            // of one body; curved facets and other bodies remain candidates.
            if excluded!=0xffffffffu && geometry[geometry[3]+i]==geometry[geometry[3]+excluded] { continue; }
            let t = triangle(i);
            let lo=min(t.p0.xyz,min(t.p1.xyz,t.p2.xyz));
            let hi=max(t.p0.xyz,max(t.p1.xyz,t.p2.xyz));
            let gap=max(lo-rounded,rounded-hi);
            if any(gap>vec3<f32>(near_radius)) {
                // The rounded origin is within origin_error of the true one on each
                // axis. Bound and edge decisions that close to their threshold could
                // flip, while a nearby neighbour across the edge is tested exactly:
                // a crossing would fall between them. Such candidates take the exact
                // path below.
                if any((rounded<=lo-vec3<f32>(origin_error))&(d<vec3<f32>(0.0))) || any((rounded>=hi+vec3<f32>(origin_error))&(d>vec3<f32>(0.0))) { continue; }
                if any((d==vec3<f32>(0.0))&((rounded<lo-vec3<f32>(origin_error))|(rounded>hi+vec3<f32>(origin_error)))) { continue; }
                let qa=t.p0.xyz-rounded; let qb=t.p1.xyz-rounded; let qc=t.p2.xyz-rounded;
                let fa=vec2<f32>(qa[kx]-shear.x*qa[kz],qa[ky]-shear.y*qa[kz]);
                let fb=vec2<f32>(qb[kx]-shear.x*qb[kz],qb[ky]-shear.y*qb[kz]);
                let fc=vec2<f32>(qc[kx]-shear.x*qc[kz],qc[ky]-shear.y*qc[kz]);
                let far_weights=vec3<f32>(difference_products(fb.x,fc.y,fb.y,fc.x),difference_products(fc.x,fa.y,fc.y,fa.x),difference_products(fa.x,fb.y,fa.y,fb.x));
                let edge_error=origin_error*vec3<f32>(length(fb-fc),length(fc-fa),length(fa-fb));
                if all(abs(far_weights)>edge_error) {
                    if any(far_weights<vec3<f32>(0.0)) && any(far_weights>vec3<f32>(0.0)) { continue; }
                    let far_det=far_weights.x+far_weights.y+far_weights.z;
                    let far_bary=far_weights/far_det;
                    let far_depth=surface_ray_depth(t,far_bary,rounded,kz);
                    let far_distance=far_depth*shear.z;
                    let far_key=far_depth*order;
                    if far_distance > minimum && far_distance < limit && (far_key < best || (far_key == best && tie_preferred(t,d,inside))) {
                        hit = Hit(far_distance,i,far_bary); best=far_key;
                        if any_hit || (far_distance < blocker && (t.base.w<=0.0 || t.coat.w<=0.0)) { return hit; }
                    }
                    continue;
                }
            }
            if !triangle_ahead_precise(t,o,o_low,d) { continue; }
            let pa=precise_point_delta(t.p0.xyz,vec3<f32>(0.0),o,o_low);
            let pb=precise_point_delta(t.p1.xyz,vec3<f32>(0.0),o,o_low);
            let pc=precise_point_delta(t.p2.xyz,vec3<f32>(0.0),o,o_low);
            let a=vec2<f32>(pa[kx]-shear.x*pa[kz],pa[ky]-shear.y*pa[kz]);
            let b=vec2<f32>(pb[kx]-shear.x*pb[kz],pb[ky]-shear.y*pb[kz]);
            let c=vec2<f32>(pc[kx]-shear.x*pc[kz],pc[ky]-shear.y*pc[kz]);
            let weights=vec3<f32>(difference_products(b.x,c.y,b.y,c.x),difference_products(c.x,a.y,c.y,a.x),difference_products(a.x,b.y,a.y,b.x));
            if any(weights<vec3<f32>(0.0)) && any(weights>vec3<f32>(0.0)) { continue; }
            let det=weights.x+weights.y+weights.z;
            if det==0.0 { continue; }
            // Retain all three directly evaluated weights: 1-u-v can erase a
            // small corner clearance when the other two weights sum to one.
            let bary=weights/det;
            let depth=surface_ray_depth_precise(t,bary,o,o_low,kz);
            let distance=depth*shear.z;
            let key=depth*order;
            if excluded!=0xffffffffu && repeated_crossing(t,source,d,inside,distance,origin_error) { continue; }
            if distance > minimum && distance < limit && (key < best || (key == best && tie_preferred(t,d,inside))) {
                hit = Hit(distance,i,bary); best=key;
                if any_hit || (distance < blocker && (t.base.w<=0.0 || t.coat.w<=0.0)) { return hit; }
            }
        }
    }
    return hit;
}
"#;
    let source = [
        SHADER,
        "\n",
        legacy,
        "\n",
        r#"
@compute @workgroup_size(1) fn shared_bvh_policy_reference() {
    let excluded=select(0xffffffffu,u32(max(u.back.w,0.0)),u.back.w>=0.0);
    let old=legacy_precise_policy(u.eye.xyz,u.right.xyz,u.up.xyz,u.back.z>0.5,excluded,u.back.x,u.back.y>0.5,u.environment.x,u.environment.y);
    let combined_hit=trace_precise_within(u.eye.xyz,u.right.xyz,u.up.xyz,u.back.z>0.5,excluded,u.back.x,u.back.y>0.5,u.environment.x,u.environment.y);
    accumulation[0]=vec4<f32>(select(0.0,1.0,old.triangle==combined_hit.triangle),
        select(0.0,1.0,bitcast<u32>(old.distance)==bitcast<u32>(combined_hit.distance)),
        select(0.0,1.0,all(bitcast<vec3<u32>>(old.bary)==bitcast<vec3<u32>>(combined_hit.bary))),1.0);
    accumulation[1]=vec4<f32>(old.distance,f32(old.triangle),old.bary.xy);
    accumulation[2]=vec4<f32>(combined_hit.distance,f32(combined_hit.triangle),combined_hit.bary.xy);
    accumulation[3]=vec4<f32>(old.bary.z,combined_hit.bary.z,0.0,1.0);
}
"#,
    ]
    .concat();
    // Check the generated reference before opening a device: a reserved test
    // identifier must report a parse error rather than silently retire its host.
    let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let pipeline = checked_creation(&context.device, || {
        let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shared BVH exact legacy policies"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&context.layout)],
            immediate_size: 0,
        });
        Ok(context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: Some(&layout),
            module: &shader,
            entry_point: Some("shared_bvh_policy_reference"),
            compilation_options: Default::default(),
            cache: None,
        }))
    })
    .unwrap();
    for scene in [directional_receiver(), glass_slab(false)] {
        let mut task =
            Task::new(context.clone(), scene, request(2), JobOptions { samples: 1, max_depth: 1, ..Default::default() }, CancelToken::default())
                .unwrap();
        let cases = [
            (Vec3::new(0.125, 0.25, 4.0), Vec3::ZERO, -Vec3::Z, 0.0, false, false, None, 1e30, -1.0),
            (Vec3::new(0.0, 0.0, 4.0), Vec3::ZERO, -Vec3::Z, 0.0, false, true, None, 1e30, -1.0),
            (
                Vec3::new(0.125, 0.25, 4.0),
                Vec3::new(1e-10, -2e-10, 0.0),
                Vec3::new(0.01, 0.02, -1.0).normalized().unwrap(),
                0.0,
                false,
                false,
                None,
                1e30,
                1.5,
            ),
            (Vec3::new(0.0, 0.125, 0.5), Vec3::new(0.0, 0.0, -1e-8), -Vec3::Z, 0.0, true, false, None, 1e30, -1.0),
            (Vec3::new(0.0, 0.125, 0.5), Vec3::new(0.0, 0.0, 1e-8), Vec3::Z, 0.0, false, false, None, 1e30, -1.0),
            (Vec3::new(30.0, 0.125, 4.0), Vec3::new(1e-5, 0.0, 0.0), -Vec3::Z, 0.0, false, true, None, 2.0, 1.0),
            (Vec3::new(0.25, 0.25, -0.5), Vec3::new(0.0, 0.0, 1e-8), Vec3::Z, 0.0, true, false, Some(0), 1e30, -1.0),
            (Vec3::new(-11.0, 0.125, 0.0), Vec3::new(1e-8, 0.0, 0.0), Vec3::X, 0.0, false, true, None, 1e30, -1.0),
        ];
        for (origin, low, direction, minimum, inside, any_hit, excluded, limit, blocker) in cases {
            let mut params = task.parameters(1).unwrap();
            let mut vectors = Vec::new();
            scene::vector(&mut vectors, origin, 0.0);
            scene::vector(&mut vectors, low, 0.0);
            scene::vector(&mut vectors, direction, 0.0);
            scene::vector(
                &mut vectors,
                Vec3::new(minimum, if inside { 1.0 } else { 0.0 }, if any_hit { 1.0 } else { 0.0 }),
                excluded.map_or(-1.0, |id| id as f32),
            );
            params[32..96].copy_from_slice(&vectors);
            vectors.clear();
            scene::vector(&mut vectors, Vec3::new(limit, blocker, 0.0), 0.0);
            params[112..128].copy_from_slice(&vectors);
            context.queue.write_buffer(&task.uniform, 0, &params);
            let mut encoder = context.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &task.bind, &[]);
                pass.dispatch_workgroups(1, 1, 1);
            }
            context.queue.submit([encoder.finish()]);
            task.sample = 1;
            let pixels = task.read_linear().unwrap();
            eprintln!(
                "shared-BVH policy origin={origin:?} low={low:?} direction={direction:?} minimum={minimum} inside={inside} any={any_hit} excluded={excluded:?} limit={limit} blocker={blocker}: old(distance,index,baryXY)={:?} combined={:?} baryZ={:?} exactFlags={:?} hitBits={:?}",
                pixels[1],
                pixels[2],
                pixels[3],
                pixels[0],
                [pixels[1].map(f32::to_bits), pixels[2].map(f32::to_bits), pixels[3].map(f32::to_bits)]
            );
            assert_eq!(
                pixels[0], [1.0; 4],
                "exact triangle/distance/barycentric bits: origin={origin:?} low={low:?} direction={direction:?} inside={inside} any={any_hit} limit={limit} blocker={blocker}"
            );
        }
    }
}

#[test]
fn photon_source_power_table_matches_shader_counts_and_packed_zero_environment() {
    use solidcraft_render::job::{CausticOptions, DirectionalLightOptions, DirectionalLights, StudioLighting};
    let mut bytes = Vec::new();
    for value in [-1.0_f32, -1.0, -1.0, 0.0, 1.0, 1.0, 1.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let image = request(1);
    let options = JobOptions {
        caustics: Some(CausticOptions { power_weighted_sources: true, environment: true, ..Default::default() }),
        lighting: Some(StudioLighting {
            directional_lights: DirectionalLights::new(&[DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 1.0 }])
                .unwrap(),
            environment: [1e-100; 3],
            ..Default::default()
        }),
        ..Default::default()
    };
    let table = photons::source_distribution(options, &image, &bytes, None, 0.0).unwrap().unwrap();
    assert_eq!(table.cumulative().len(), 1, "underflowed environment is absent on the shader");
    let mut hdr = image.clone();
    hdr.environment = Some(Arc::new(solidcraft_render::lighting::environment::EnvironmentSource::hdr(original_hdr(), 0.0, 1e-100, false).unwrap()));
    assert_eq!(photons::source_distribution(options, &hdr, &bytes, None, 1e-100).unwrap().unwrap().cumulative().len(), 1);
    let mut subnormal = options;
    subnormal.lighting.as_mut().unwrap().environment = [f64::from(f32::from_bits(1)); 3];
    assert_eq!(photons::source_distribution(subnormal, &image, &bytes, None, 0.0).unwrap().unwrap().cumulative().len(), 2);
    hdr.environment = Some(Arc::new(
        solidcraft_render::lighting::environment::EnvironmentSource::hdr(original_hdr(), 0.0, f64::from(f32::from_bits(1)), false).unwrap(),
    ));
    assert_eq!(photons::source_distribution(options, &hdr, &bytes, None, f64::from(f32::from_bits(1))).unwrap().unwrap().cumulative().len(), 2);
    let mut lit = options;
    lit.lighting.as_mut().unwrap().environment = [1.0; 3];
    let table = photons::source_distribution(lit, &image, &bytes, None, 0.0).unwrap().unwrap();
    assert_eq!(table.cumulative().len(), 2);
    let (_, probability) = table.select(0.99).unwrap();
    let expected = 0.95 * (4.0 * std::f64::consts::PI) / (1.0 + 4.0 * std::f64::consts::PI) + 0.025;
    assert!((probability - expected).abs() < 1.0 / 16777216.0);
    lit.caustics.as_mut().unwrap().max_depth = 1;
    assert_eq!(photons::source_distribution(lit, &image, &bytes, None, 0.0).unwrap().unwrap().cumulative().len(), 1);
    let mut off = options;
    off.caustics.as_mut().unwrap().power_weighted_sources = false;
    assert!(photons::source_distribution(off, &image, &[], None, f64::NAN).unwrap().is_none());
    let base = CausticOptions::default();
    let weighted = CausticOptions { power_weighted_sources: true, ..base };
    assert_eq!(photons::buffer_bytes(weighted).unwrap() - photons::buffer_bytes(base).unwrap(), (4 + 1 + 21) * 4);
    assert_eq!(photons::admission_bytes(Some(weighted)).unwrap() - photons::admission_bytes(Some(base)).unwrap(), 2 * (4 + 1 + 21) * 4);
}

#[test]
fn device_limits_use_adapter_capacity_with_a_finite_host_ceiling() {
    for (storage, buffer) in [(128 << 20, 256 << 20), (4_294_967_295, 4_294_967_292), (32 << 30, 16 << 30)] {
        let mut supported = wgpu::Limits::downlevel_defaults();
        supported.max_storage_buffers_per_shader_stage = 8;
        supported.max_storage_buffer_binding_size = storage;
        supported.max_buffer_size = buffer;
        let requested = device_limits(supported).unwrap();
        assert_eq!(requested.max_buffer_size, buffer.min(GPU_PATH_LIMITS.max_scratch_bytes));
        assert_eq!(requested.max_storage_buffer_binding_size, storage.min(buffer).min(GPU_PATH_LIMITS.max_scratch_bytes));
    }
}

fn maximum_paged_image_options(adaptive_photons: bool) -> JobOptions {
    use solidcraft_render::job::{
        CausticOptions, DenoiseSettings,
        adaptive::{AdaptiveSampling, MAX_PILOT_SAMPLES},
        bloom::Bloom,
        finishing::Finishing,
        photographic::{Flare, Photographic},
    };
    JobOptions {
        samples: 16_384,
        max_depth: 64,
        supersampling: 2,
        transmission_samples: 4,
        texture_filtering: true,
        metal_energy_compensation: true,
        denoise: true,
        denoise_settings: DenoiseSettings { passes: 3, strength: 1.0, firefly_suppression: true, optical_filter: true },
        denoise_diagnostics: true,
        linear_exr: true,
        adaptive: adaptive_photons.then_some(AdaptiveSampling { pilot_samples: MAX_PILOT_SAMPLES, min_samples: 16_384, ..Default::default() }),
        caustics: adaptive_photons.then_some(CausticOptions {
            photons: 1_048_576,
            max_depth: 64,
            area_lights: true,
            environment: true,
            guided_emission: true,
            refresh_per_pass: true,
            power_weighted_sources: true,
            ..Default::default()
        }),
        finishing: Finishing {
            bloom: Some(Bloom { strength: 2.0, threshold: 1e6, radius: 64 }),
            photographic: Photographic {
                flare: Flare { strength: 2.0, threshold: 1e6, ghosts: 4, halo: 2.0, streak: 2.0 },
                chromatic_pixels: 12.0,
                radial_distortion: 0.3,
                soft_focus: 1.0,
                sharpening: 1.0,
                grain: 0.15,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn eight_k_x2_maximum_controls_use_bounded_pages_without_allocating_images() {
    let scene = ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY };
    let image = ImageRequest { width: 7680, height: 4320, ..request(1) };
    for storage in [128 << 20, 1 << 30, 4_294_967_295] {
        let mut hardware = wgpu::Limits::downlevel_defaults();
        hardware.max_storage_buffers_per_shader_stage = 8;
        hardware.max_storage_buffer_binding_size = storage;
        hardware.max_buffer_size = 4_294_967_292;
        let limits = device_limits(hardware).unwrap();
        for adaptive_photons in [false, true] {
            let options = maximum_paged_image_options(adaptive_photons);
            let admitted = admission_with_device_limits(&scene, &image, options, &limits).unwrap();
            let trace = trace_request(&image, options).unwrap();
            admit_device_resources(&scene, &trace, options, &admitted, &limits).unwrap();
            let pages = image_pages::Layout::new(trace.width, trace.height, options, 0, &limits).unwrap();
            assert!(pages.is_paged());
            assert_eq!(pages.halo, 14);
            assert!(pages.buffer_bytes(pages.max_core_pixels()).unwrap() <= storage.min(image_pages::TARGET_BUFFER_BYTES));
            assert!(u64::from(pages.max_window_pixels()) * denoise::GUIDE_BYTES_PER_PIXEL <= storage.min(image_pages::TARGET_BUFFER_BYTES));
            assert!(pages.readback_bytes().unwrap() <= limits.max_buffer_size);
            assert!(admitted.estimated_total_bytes < GPU_PATH_LIMITS.max_scratch_bytes);
            assert!(admitted.pixel_scratch_bytes >= pages.filter_scratch_bytes().unwrap());
            assert_eq!(admitted.caustic_work.map(|work| work.maps), adaptive_photons.then_some(options.preview_target().unwrap()));
            let mut next = 0;
            for window in pages.windows() {
                assert_eq!(window.core_start, next);
                assert_eq!(window.start, window.core_start.saturating_sub(14 * trace.width));
                assert_eq!(window.start + window.pixels, (window.core_start + window.core_pixels + 14 * trace.width).min(trace.width * trace.height));
                assert!((window.core_start / trace.width).is_multiple_of(2));
                assert!((window.core_pixels / trace.width).is_multiple_of(2));
                next += window.core_pixels;
            }
            assert_eq!(next, trace.width * trace.height);
            println!(
                "nonallocating 8K X2 16384spp depth64 filter3 full-strength fireflies diagnostics raw bloom photographic texture-filtering metal-compensation: storage={storage} adaptive_photons={adaptive_photons} estimated={} pages={} core_rows={} halo_rows={} max_window_pixels={} readback={}",
                admitted.estimated_total_bytes,
                pages.page_count(),
                pages.rows,
                pages.halo,
                pages.max_window_pixels(),
                pages.readback_bytes().unwrap(),
            );
        }
    }
}

#[test]
fn image_page_capacity_prepays_optical_and_adaptive_planes_and_rejects_only_infeasible_rows() {
    let options = maximum_paged_image_options(true);
    let mut limits = wgpu::Limits::downlevel_defaults();
    limits.max_storage_buffer_binding_size = 128 << 20;
    limits.max_buffer_size = 256 << 20;
    let optical = u64::from(dispatch::MAX_PATHS) * 128;
    let layout = image_pages::Layout::new(15360, 8640, options, optical, &limits).unwrap();
    assert_eq!(layout.planes, 3);
    assert_eq!(layout.buffer_bytes(layout.max_core_pixels()).unwrap(), u64::from(layout.max_core_pixels()) * 48 + optical);
    assert_eq!(layout.optical_scratch_bytes().unwrap(), optical * u64::from(layout.page_count()));
    assert_eq!(layout.filter_scratch_bytes().unwrap(), u64::from(layout.max_window_pixels()) * (128 + 16 + 48));
    assert!(layout.readback_bytes().unwrap() >= u64::from(layout.max_core_pixels()) * 32 + FAILURE_BYTES);
    for window in layout.windows() {
        assert!(layout.buffer_bytes(window.core_pixels).unwrap() <= limits.max_storage_buffer_binding_size);
    }
    let ordinary = JobOptions { denoise: false, denoise_diagnostics: false, ..options };
    let no_filter = image_pages::Layout::new(15360, 8640, ordinary, optical, &limits).unwrap();
    assert!(no_filter.buffer_bytes(no_filter.max_core_pixels()).unwrap() <= limits.max_storage_buffer_binding_size);
    assert_eq!(no_filter.rows, (((limits.max_storage_buffer_binding_size - optical) / (15360 * 48)) as u32) / 2 * 2);
    limits.max_storage_buffer_binding_size = 1 << 20;
    assert_eq!(image_pages::Layout::new(15360, 8640, options, optical, &limits).unwrap_err().code, ErrorCode::LimitExceeded);
    for (width, height) in [(0, 1), (1, 0), (u32::MAX, 1), (16384, 16384)] {
        assert!(image_pages::Layout::new(width, height, options, optical, &limits).is_err());
    }
    limits.max_storage_buffer_binding_size = 128 << 20;
    assert!(image_pages::Layout::new(16, 16, options, u64::MAX, &limits).is_err());
    limits.max_buffer_size = FAILURE_BYTES - 1;
    assert!(image_pages::Layout::new(16, 16, options, 0, &limits).is_err());
}

#[test]
fn real_adapter_eight_k_x2_maximum_control_preflight_and_preview_accounting_allocate_no_images() {
    let Some(executor) = executor() else { return };
    let limits = executor.context.device.limits();
    let host = ImageHost::with_executor(Arc::new(Executor { context: executor.context.clone() })).unwrap();
    let scene = ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY };
    let image = ImageRequest { width: 7680, height: 4320, ..request(1) };
    for adaptive_photons in [false, true] {
        if adaptive_photons && limits.max_storage_buffers_per_shader_stage < 7 {
            continue;
        }
        let options = maximum_paged_image_options(adaptive_photons);
        let direct = executor.admit(&scene, &image, options).unwrap();
        let trace = trace_request(&image, options).unwrap();
        let pages = image_pages::Layout::new(trace.width, trace.height, options, 0, &limits).unwrap();
        println!(
            "actual adapter nonallocating maximum receipt: binding={} buffer={} storage_slots={} adaptive_photons={adaptive_photons} photographic=true filtering=true metal_compensation=true estimated={} pages={} core_rows={} halo_rows={} max_window_pixels={} readback={}",
            limits.max_storage_buffer_binding_size,
            limits.max_buffer_size,
            limits.max_storage_buffers_per_shader_stage,
            direct.estimated_total_bytes,
            pages.page_count(),
            pages.rows,
            pages.halo,
            pages.max_window_pixels(),
            pages.readback_bytes().unwrap(),
        );
        match host.admit(&scene, &image, options) {
            Ok(admitted) => {
                assert!(admitted.estimated_total_bytes >= direct.estimated_total_bytes + 2 * direct.encoded_limit);
                let memory = admitted.host_memory.expect("native maximum admission must include its actual RAM receipt");
                assert_eq!((memory.credited_bytes, memory.additional_bytes), (0, memory.required_bytes));
                assert!(memory.additional_bytes <= memory.available_bytes);
                assert_eq!(memory.available_bytes, memory.capacity_bytes.saturating_sub(memory.physical_debit_bytes));
                println!(
                    "actual host maximum including retained previews: adaptive_photons={adaptive_photons} estimated={} memory={memory:?}",
                    admitted.estimated_total_bytes,
                );
            }
            Err(error) => {
                assert_eq!(error.code, ErrorCode::LimitExceeded, "{error:?}");
                assert!(
                    error.message.contains("native render memory needs"),
                    "only actual host RAM capacity may refuse this admitted layout: {error:?}"
                );
                println!("actual host capacity refusal without allocation: adaptive_photons={adaptive_photons} {error}");
            }
        }
        assert_eq!(host.admitted_bytes(), 0, "preflight must never reserve image buffers or enqueue transport");
    }
}

#[test]
fn gpu_paged_optical_scratch_and_refreshed_photons_keep_global_sample_identity() {
    use solidcraft_render::job::{CausticOptions, DenoiseGuideClass, DenoiseSettings, DirectionalLightOptions, DirectionalLights, StudioLighting};
    let Some(executor) = executor() else { return };
    if executor.context.device.limits().max_storage_buffers_per_shader_stage < 7 {
        return;
    }
    let mut scene = photon_receiver_fixture(0.0);
    scene.meshes[0].surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap());
    scene.meshes[0].transmission = Some(
        clear_glass()
            .with_shadow_transmission(false)
            .with_model(solidcraft_render::glass_model::GlassModel { roughness: 0.4, ..Default::default() })
            .unwrap(),
    );
    assert!(needs_glass_lighting(&scene));
    let image = ImageRequest { width: 8, height: 16, ..request(8) };
    let optical_bytes = glass_tile_bytes(&scene, &image);
    assert!(optical_bytes > 0);
    let options = JobOptions {
        samples: 3,
        max_depth: 4,
        transmission_samples: 2,
        seed: 17,
        denoise: true,
        denoise_settings: DenoiseSettings { passes: 3, strength: 1.0, firefly_suppression: true, optical_filter: true },
        denoise_diagnostics: true,
        lighting: Some(StudioLighting {
            directional_lights: DirectionalLights::new(&[DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 1.0 }])
                .unwrap(),
            ..Default::default()
        }),
        caustics: Some(CausticOptions { photons: 512, radius_mm: 3.0, max_depth: 4, refresh_per_pass: true, ..Default::default() }),
        ..Default::default()
    };
    let render = |rows| {
        let mut task =
            Task::new_with_page_rows(executor.context.clone(), scene.clone(), image.clone(), options, CancelToken::default(), rows).unwrap();
        assert!(task.optical_resolve.is_some());
        assert_eq!(task.page_layout.optical_bytes, optical_bytes);
        assert_eq!(task.page_layout.optical_scratch_bytes().unwrap(), optical_bytes * task.pages.len() as u64);
        for page in &task.pages {
            assert_eq!(page.accumulation.size(), u64::from(page.pixels) * 16 + optical_bytes, "each page owns a complete optical dispatch suffix");
        }
        let mut maps = Vec::new();
        let mut films = Vec::new();
        for pass in 0..3 {
            task.complete_sample().unwrap();
            let resources = task.photons.as_ref().unwrap();
            assert_eq!(resources.test_epoch(), pass);
            let records = resources.test_records(&task.context);
            assert!(records.iter().any(|record| record[11] != 0), "the fixture must store actual caustic photons");
            maps.push(records);
            films.push(task.read_linear().unwrap());
        }
        assert_ne!(maps[0], maps[1], "refreshed maps must use independent indexed paths");
        let filter = task.denoiser.as_ref().unwrap();
        let filtered = filter.read_filtered(&task, false).unwrap();
        let diagnostics = filter.diagnostics(&task).unwrap();
        // Rough glass has no supported surface correspondence. Selecting
        // denoising must retain the established bypass, not invent smooth guides.
        assert_eq!(diagnostics.0.pixels(DenoiseGuideClass::UnsupportedMaterial), u64::from(image.width) * u64::from(image.height));
        assert_eq!(&filtered, films.last().unwrap(), "unsupported rough primary rays retain raw radiance");
        (maps, films, filtered, diagnostics)
    };
    let whole = render(None);
    let paged = render(Some(4));
    assert_eq!(whole.0, paged.0, "page boundaries cannot alter indexed photon records or refresh epochs");
    assert_eq!(whole.3, paged.3, "guide categories and masks remain exact");
    let mut maximum_difference = 0.0_f32;
    // Independent photon maps can link identical indexed records in different
    // atomic insertion orders. Only their floating-point gather sums allow the
    // same small tolerance as the existing rough-photon transport fixtures.
    for (a, b) in whole.1.iter().flatten().chain(&whole.2).zip(paged.1.iter().flatten().chain(&paged.2)) {
        assert_eq!(a[3].to_bits(), b[3].to_bits(), "coverage stays exact");
        for (a, b) in a[..3].iter().zip(&b[..3]) {
            assert!(a.is_finite() && b.is_finite());
            let difference = (a - b).abs();
            maximum_difference = maximum_difference.max(difference);
            assert!(difference <= 3e-6 * a.abs().max(b.abs()).max(1.0), "independent photon gather rounding: {a} versus {b}");
        }
    }
    println!("paged rough optical/photon parity: indexed records and masks exact; maximum independent-gather difference={maximum_difference}");
}
