use super::*;
use solidcraft_render::transmission::{Transmission, subsurface::Subsurface};

fn slab_body(name: &str, scale: Vec3, shift: Vec3, ior: f64, absorption: Vec3) -> ProductMesh {
    let mut body = glass_slab(false).meshes.remove(0);
    body.name = name.into();
    for p in &mut Arc::make_mut(&mut body.mesh).positions {
        *p = Vec3::new(p.x * scale.x, p.y * scale.y, p.z * scale.z) + shift;
    }
    body.surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, ior).unwrap());
    body.transmission = Some(Transmission::new(absorption, None).unwrap());
    body
}
fn nested() -> ProductScene {
    let outer = slab_body("Outer", Vec3::new(1.0, 1.0, 4.0), Vec3::ZERO, 1.0, Vec3::new(0.1, 0.2, 0.0));
    let inner = slab_body("Inner", Vec3::new(0.5, 0.5, 2.0), Vec3::ZERO, 1.0, Vec3::new(0.3, 0.4, 0.0));
    ProductScene { bounds: outer.mesh.bounds(), meshes: vec![outer, inner] }
}
#[test]
fn nested_certificate_is_opt_in_order_independent_and_rejects_touches_and_shadow_shortcuts() {
    let source = nested();
    let cancel = CancelToken::default();
    assert!(scene::flatten(&source, Vec3::ZERO, &cancel).is_err());
    assert!(scene::flatten_with_media(&source, Vec3::ZERO, &cancel, true).is_ok());
    let mut reversed = source.clone();
    reversed.meshes.reverse();
    assert!(scene::flatten_with_media(&reversed, Vec3::ZERO, &cancel, true).is_ok());
    for dx in [5.0, 7.0] {
        let mut overlapping = source.clone();
        for p in &mut Arc::make_mut(&mut overlapping.meshes[1].mesh).positions {
            p.x += dx;
        }
        assert!(scene::flatten_with_media(&overlapping, Vec3::ZERO, &cancel, true).is_err());
    }
    let mut shortcut = source.clone();
    shortcut.meshes[1].transmission = Some(clear_glass().with_shadow_transmission(true));
    assert!(scene::flatten_with_media(&shortcut, Vec3::ZERO, &cancel, true).err().unwrap().message.contains("shadow"));
    let mut deep = ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY };
    for i in 0..17 {
        deep.meshes.push(slab_body(
            &format!("layer {i}"),
            Vec3::new(1.0 / (1.0 + f64::from(i)), 1.0 / (1.0 + f64::from(i)), 4.0 / (1.0 + f64::from(i))),
            Vec3::ZERO,
            1.0,
            Vec3::ZERO,
        ));
    }
    deep.bounds = deep.meshes[0].mesh.bounds();
    assert!(scene::flatten_with_media(&deep, Vec3::ZERO, &cancel, true).err().unwrap().message.contains("16 nested"));
    cancel.cancel();
    assert!(scene::flatten_with_media(&source, Vec3::ZERO, &cancel, true).is_err());
}
#[test]
fn certified_nested_gpu_paths_use_each_true_medium_length_and_support_inside_camera() {
    let Some(executor) = executor() else { return };
    for (reverse, inside) in [(false, false), (true, false), (false, true)] {
        let mut source = nested();
        if reverse {
            source.meshes.reverse();
        }
        let mut req = request(1);
        req.camera.fov = 0.00001;
        if inside {
            req.camera.distance = 0.1;
            req.camera.target = Vec3::new(0.0, 0.0, -0.1);
        }
        let options = JobOptions { nested_media: true, samples: 32, max_depth: 12, seed: 71, ..Default::default() };
        let mut task = Task::new(executor.context.clone(), source, req, options, CancelToken::default()).unwrap();
        task.environment = 1.0;
        task.environment_bottom_only = true;
        task.studio = 0.0;
        for _ in 0..options.samples {
            task.dispatch().unwrap();
        }
        let value = task.read_linear().unwrap()[0];
        let lengths = if inside { [1.0, 1.0] } else { [2.0, 2.0] };
        let expected = [(-0.1_f64 * lengths[0] - 0.3 * lengths[1]).exp(), (-0.2_f64 * lengths[0] - 0.4 * lengths[1]).exp(), 1.0];
        for (a, b) in value.into_iter().zip(expected) {
            assert!((f64::from(a) - b).abs() < 2e-5, "reverse={reverse} inside={inside}: {value:?} vs {expected:?}");
        }
    }
    // Same-IOR nested interfaces must not invent glass/air Fresnel reflections.
    let mut source = nested();
    for body in &mut source.meshes {
        body.surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5).unwrap());
        body.transmission = Some(clear_glass());
    }
    let mut req = request(8);
    req.camera.fov = 0.001;
    let options = JobOptions { nested_media: true, samples: 512, max_depth: 16, seed: 192, ..Default::default() };
    let mut task = Task::new(executor.context, source, req, options, CancelToken::default()).unwrap();
    task.environment = 1.0;
    task.studio = 0.0;
    while task.sample < options.samples {
        task.dispatch().unwrap();
    }
    for value in task.read_linear().unwrap() {
        assert!((value[0] - 1.0).abs() < 0.025, "lossless nested furnace {value:?}");
    }
}
#[test]
fn subsurface_random_walk_preserves_zero_rate_and_matches_energy_and_absorption_bounds() {
    let Some(executor) = executor() else { return };
    let mut source =
        ProductScene { meshes: vec![slab_body("Scattering solid", Vec3::new(1.0, 1.0, 2.0), Vec3::ZERO, 1.0, Vec3::ZERO)], bounds: Aabb3::EMPTY };
    source.bounds = source.meshes[0].mesh.bounds();
    let mut req = request(8);
    req.camera.fov = 0.001;
    let options = JobOptions { samples: 512, max_depth: 16, seed: 1711, ..Default::default() };
    let render = |s: ProductScene| {
        let mut task = Task::new(executor.context.clone(), s, req.clone(), options, CancelToken::default()).unwrap();
        task.environment = 1.0;
        task.studio = 0.0;
        while task.sample < options.samples {
            task.dispatch().unwrap();
        }
        task.read_linear().unwrap()
    };
    let base = render(source.clone());
    let mut zero = source.clone();
    zero.meshes[0].transmission = Some(clear_glass().with_subsurface(Subsurface { scattering_per_mm: 0.0, anisotropy: 0.5 }).unwrap());
    assert_eq!(base, render(zero));
    let mut scatter = source.clone();
    scatter.meshes[0].transmission = Some(clear_glass().with_subsurface(Subsurface { scattering_per_mm: 0.3, anisotropy: 0.35 }).unwrap());
    let result = render(scatter.clone());
    let average = result.iter().map(|p| f64::from(p[0])).sum::<f64>() / result.len() as f64;
    eprintln!("nonabsorbing homogeneous interior furnace average={average}");
    assert!((average - 1.0).abs() < 0.02);
    assert!(result.iter().all(|p| p.iter().all(|c| c.is_finite())));
    assert!(admission(&scatter, &req, JobOptions { denoise: true, ..options }).is_err());
    let mut absorbing = scatter;
    absorbing.meshes[0].transmission = Some(
        Transmission::new(Vec3::new(0.2, 0.5, 1.0), None).unwrap().with_subsurface(Subsurface { scattering_per_mm: 0.3, anisotropy: 0.35 }).unwrap(),
    );
    let result = render(absorbing);
    let mean: [f64; 3] = std::array::from_fn(|c| result.iter().map(|p| f64::from(p[c])).sum::<f64>() / result.len() as f64);
    eprintln!("absorbing homogeneous interior RGB={mean:?}");
    assert!(mean[0] < average && mean[0] > mean[1] && mean[1] > mean[2] && mean[2] > 0.0);
}

#[test]
fn nested_media_camera_inside_before_opaque_geometry_attenuates_the_visible_emitter_from_the_actual_origin() {
    use solidcraft_render::job::{AreaLight, AreaLights, StudioLighting};
    let Some(executor) = executor() else { return };
    let radiance = [2.0, 3.0, 5.0];
    let render = |inside: bool, absorbing: bool, reverse: bool| {
        let outer_sigma = if absorbing { Vec3::new(2.0_f64.ln(), 2.0_f64.ln(), 0.0) } else { Vec3::ZERO };
        let inner_sigma = if absorbing { Vec3::new(2.0_f64.ln(), 4.0_f64.ln(), 0.0) } else { Vec3::ZERO };
        // Both index-matched media have no Fresnel/refraction uncertainty. The inner
        // medium replaces, rather than adds to, the outer absorption along its part.
        let outer = slab_body("Outer origin medium", Vec3::new(1.0, 1.0, 8.0), Vec3::ZERO, 1.0, outer_sigma);
        let inner = slab_body("Inner origin medium", Vec3::new(0.5, 0.5, 4.0), Vec3::ZERO, 1.0, inner_sigma);
        let opaque_mesh = Mesh {
            positions: vec![Vec3::new(-3.5, -3.5, -1.5), Vec3::new(3.5, -3.5, -1.5), Vec3::new(3.5, 3.5, -1.5), Vec3::new(-3.5, 3.5, -1.5)],
            normals: vec![Vec3::Z; 4],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            tri_face: vec![0, 0],
            ..Default::default()
        };
        let opaque = ProductMesh {
            name: "First inside geometry is opaque".into(),
            mesh: Arc::new(opaque_mesh),
            color: [0; 3],
            opacity: 1.0,
            surface: Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.5, 1.0).unwrap()),
            transmission: None,
            faces: Vec::new(),
        };
        let mut image = request(4);
        image.camera.fov = 0.0; // Orthographic rays all have the same exact axial lengths.
        image.camera.distance = if inside { 0.5 } else { 5.0 };
        image.camera.target = if inside { Vec3::new(0.0, 0.0, -2.0) } else { Vec3::ZERO };
        assert_eq!((outer.mesh.bounds().min.z, outer.mesh.bounds().max.z), (-4.0, 4.0));
        assert_eq!((inner.mesh.bounds().min.z, inner.mesh.bounds().max.z), (-2.0, 2.0));
        assert_eq!((opaque.mesh.bounds().min.z, opaque.mesh.bounds().max.z), (-1.5, -1.5));
        // In the inside case the geometry ray first meets the opaque plane at1.5mm,
        // before the inner exit at2mm. The front-facing analytic light at1mm must
        // still acquire Beer attenuation from the camera's actual inner medium.
        let light = AreaLight { center: [0.0, 0.0, -1.0], u: [3.0, 0.0, 0.0], v: [0.0, 3.0, 0.0], radiance };
        let bounds = outer.mesh.bounds();
        let mut source = ProductScene { bounds, meshes: vec![outer, inner, opaque] };
        if reverse {
            source.meshes.reverse();
        }
        assert!(
            scene::flatten_with_media(&source, Vec3::ZERO, &CancelToken::default(), true).is_ok(),
            "opaque backing inside two separated certified closed media is valid input"
        );
        let options = JobOptions {
            nested_media: true,
            samples: 4,
            max_depth: 4,
            seed: 71,
            lighting: Some(StudioLighting {
                sun: None,
                environment: [0.0; 3],
                area_lights: AreaLights::new(&[light]).unwrap(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut task = Task::new(executor.context.clone(), source, image, options, CancelToken::default()).unwrap();
        // Orthographic GPU launches use target + back*(4*distance), not Camera::eye.
        // Inspect the actual packed primary origin before measuring physical Beer.
        let packed = task.parameters(0).unwrap();
        let coordinate = |axis: usize| f64::from(f32::from_le_bytes(packed[32 + axis * 4..36 + axis * 4].try_into().unwrap()));
        let actual_origin = Vec3::new(coordinate(0), coordinate(1), coordinate(2)) + task.request.camera.target;
        assert_eq!(f32::from_le_bytes(packed[44..48].try_into().unwrap()), 0.0, "this fixture uses the actual orthographic GPU branch");
        assert!((actual_origin.z - if inside { 0.0 } else { 20.0 }).abs() < 1e-14, "actual packed GPU world origin: {actual_origin:?}");
        for _ in 0..options.samples {
            task.complete_sample().unwrap();
        }
        task.read_linear().unwrap()
    };
    // Controls run before the expected-red inside/absorbing cases. Outside rays
    // traverse outer4→2 (2mm), then inner2→-1 (3mm); inside rays see inner0→-1 (1mm).
    // Thus absorbing outside RGB is exactly[1/16,3/256,5], inside is[1,3/4,5].
    for (inside, absorbing, reverse) in
        [(false, false, false), (true, false, false), (false, true, false), (false, true, true), (true, true, false), (true, true, true)]
    {
        let expected: [f64; 3] = if !absorbing {
            radiance
        } else if inside {
            [1.0, 0.75, 5.0]
        } else {
            [1.0 / 16.0, 3.0 / 256.0, 5.0]
        };
        for (pixel, value) in render(inside, absorbing, reverse).into_iter().enumerate() {
            assert_eq!(value[3], 1.0, "visible analytic emitter has full coverage");
            if !absorbing {
                assert_eq!(value, [2.0, 3.0, 5.0, 1.0], "lossless same-IOR control must retain exact source radiance");
            }
            for channel in 0..3 {
                assert!(
                    (f64::from(value[channel]) - expected[channel]).abs() < 2e-5,
                    "inside={inside} absorbing={absorbing} reverse={reverse} pixel={pixel}: {value:?} vs independent Beer {expected:?}"
                );
            }
        }
    }
}

#[test]
fn gpu_origin_query_uses_only_closed_boundaries_and_refuses_surface_and_silhouette_ambiguity() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let source = [
        SHADER,
        r#"
@compute @workgroup_size(1) fn origin_reference() {
    let boundary=origin_boundary(u.eye.xyz,u.right.xyz);
    var media:array<Medium,16>;var count=0u;var valid=boundary.valid;
    if valid && boundary.hit.triangle!=0xffffffffu { valid=initialize_media(boundary.hit.triangle,u.right.xyz,&media,&count); }
    var sigma=vec3<f32>(0.0);var scatter=vec2<f32>(0.0);var outer=0.0;var top=0.0;
    if valid && count>0u { sigma=media[count-1u].sigma;scatter=media[count-1u].scatter;outer=media[0].ior;top=media[count-1u].ior; }
    accumulation[0]=vec4<f32>(sigma,select(0.0,1.0,valid));
    accumulation[1]=vec4<f32>(outer,top,f32(count),select(0.0,1.0,boundary.hit.triangle!=0xffffffffu));
    accumulation[2]=vec4<f32>(scatter,0.0,1.0);
}
"#,
    ]
    .concat();
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("actual origin classification"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
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
        entry_point: Some("origin_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    for reverse in [false, true] {
        let mut source = nested();
        source.meshes[0].surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.125).unwrap());
        source.meshes[1].surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.25).unwrap());
        source.meshes[0].transmission =
            Some(source.meshes[0].transmission.unwrap().with_subsurface(Subsurface { scattering_per_mm: 0.1, anisotropy: 0.2 }).unwrap());
        source.meshes[1].transmission =
            Some(source.meshes[1].transmission.unwrap().with_subsurface(Subsurface { scattering_per_mm: 0.3, anisotropy: 0.35 }).unwrap());
        let mut opaque = directional_receiver().meshes.remove(0);
        for p in &mut Arc::make_mut(&mut opaque.mesh).positions {
            p.z = -0.25;
        }
        opaque.name = "Opaque first hit".into();
        source.meshes.push(opaque);
        let mut thin = glass_slab(true).meshes.remove(0);
        thin.name = "Outside thin sheet".into();
        for p in &mut Arc::make_mut(&mut thin.mesh).positions {
            p.z = 3.0;
        }
        source.meshes.push(thin);
        if reverse {
            source.meshes.reverse();
        }
        // GPU12's original 1x1/seed71/sample4 ray rounds both projected outer
        // top-face triangles onto their shared diagonal. Prove the uploaded
        // topology and exact arithmetic independently before querying it.
        let upload = scene::flatten_with_media(&source, Vec3::ZERO, &CancelToken::default(), true).unwrap();
        let word = |at: usize| u32::from_le_bytes(upload.geometry[at * 4..at * 4 + 4].try_into().unwrap());
        let vertex = |index: u32| {
            let start = word(1) as usize + index as usize * 8;
            std::array::from_fn::<_, 3, _>(|axis| f32::from_bits(word(start + axis)))
        };
        let tops: Vec<_> = (0..word(0) as usize)
            .filter_map(|index| {
                let positions = [vertex(word(4 + index * 4)), vertex(word(5 + index * 4)), vertex(word(6 + index * 4))];
                positions.iter().all(|p| p[2] == 2.0).then_some((index, positions))
            })
            .collect();
        assert_eq!(tops.len(), 2);
        assert_eq!(word(word(3) as usize + tops[0].0), word(word(3) as usize + tops[1].0), "the actual uploaded face has one certified patch");
        let coefficients = |index: usize| {
            let material = word(2) as usize + word(7 + index * 4) as usize * 20;
            [6, 8, 9, 10, 11, 16, 17, 18, 19].map(|offset| word(material + offset))
        };
        assert_eq!(coefficients(tops[0].0), coefficients(tops[1].0), "actual IOR/absorption/medium/SSS/parent coefficients agree");
        let origin = [0.0_f32, f32::from_bits(0xb686_37bd), 4.0];
        let direction = [f32::from_bits(0xb69a_9748), f32::from_bits(0xb643_2e36), -1.0];
        // Products of two binary32 values fit exactly in binary64 here. This
        // independent determinant rounds their exact difference once to f32.
        let determinant = |a: f32, b: f32, c: f32, d: f32| (f64::from(a) * f64::from(b) - f64::from(c) * f64::from(d)) as f32;
        let edges: Vec<_> = tops
            .iter()
            .map(|(_, p)| {
                let q = p.map(|v| {
                    let relative = std::array::from_fn::<_, 3, _>(|axis| v[axis] - origin[axis]);
                    [
                        determinant(relative[0], direction[2], direction[0], relative[2]),
                        determinant(relative[1], direction[2], direction[1], relative[2]),
                    ]
                });
                let [a, b, c] = q;
                let weights = [determinant(b[0], c[1], b[1], c[0]), determinant(c[0], a[1], c[1], a[0]), determinant(a[0], b[1], a[1], b[0])];
                assert_eq!(weights.iter().filter(|&&weight| weight == 0.0).count(), 1, "only the internal edge is zero, not a vertex");
                let zero = weights.iter().position(|&weight| weight == 0.0).unwrap();
                [p[(zero + 1) % 3], p[(zero + 2) % 3]]
            })
            .collect();
        assert_eq!(edges[0], [edges[1][1], edges[1][0]], "the shared edge is exact and oppositely directed, not merely two same-patch hits");
        let mut task = Task::new(
            context.clone(),
            source,
            request(2),
            JobOptions { nested_media: true, samples: 1, ..Default::default() },
            CancelToken::default(),
        )
        .unwrap();
        let epsilon = f64::from(task.epsilon);
        let cases = [
            (Vec3::new(0.25, 0.125, 0.0), -Vec3::Z, Some((2, Vec3::new(0.3, 0.4, 0.0), 1.125, 1.25))),
            (Vec3::new(0.25, 0.125, 1.5), -Vec3::Z, Some((1, Vec3::new(0.1, 0.2, 0.0), 1.125, 1.125))),
            (Vec3::new(0.25, 0.125, 3.5), -Vec3::Z, Some((0, Vec3::ZERO, 0.0, 0.0))),
            (Vec3::ZERO, -Vec3::Z, Some((2, Vec3::new(0.3, 0.4, 0.0), 1.125, 1.25))),
            (Vec3::new(0.0, 0.0, 1.5), -Vec3::Z, Some((1, Vec3::new(0.1, 0.2, 0.0), 1.125, 1.125))),
            (Vec3::new(0.0, 0.0, 3.5), -Vec3::Z, Some((0, Vec3::ZERO, 0.0, 0.0))),
            (
                Vec3::new(f64::from(origin[0]), f64::from(origin[1]), f64::from(origin[2])),
                Vec3::new(f64::from(direction[0]), f64::from(direction[1]), f64::from(direction[2])),
                Some((0, Vec3::ZERO, 0.0, 0.0)),
            ),
            (Vec3::new(30.0, 0.125, 0.0), Vec3::X, Some((0, Vec3::ZERO, 0.0, 0.0))),
            (Vec3::new(0.25, 0.125, 1.0), -Vec3::Z, None),
            (Vec3::new(0.25, 0.125, 1.0), Vec3::Z, None),
            (Vec3::new(5.0, 5.0, 1.0), -Vec3::Z, None),
            (Vec3::new(10.0, 0.0, 3.5), -Vec3::Z, None),
            (Vec3::new(10.0, 10.0, 3.5), -Vec3::Z, None),
            (Vec3::new(0.25, 0.125, 1.0 + epsilon * 0.5), -Vec3::Z, None),
            (Vec3::new(0.25, 0.125, 1.0 - epsilon * 0.5), Vec3::Z, None),
            (Vec3::new(0.25, 0.125, 1_000_000_000.0), -Vec3::Z, None),
            (Vec3::new(-8.0, 5.0, 0.25), Vec3::X, None),
            (Vec3::new(-8.0, 2.0, 0.0), Vec3::new(1.0, 1.0, 0.0) / 2.0_f64.sqrt(), None),
        ];
        for (origin, direction, expected) in cases {
            let mut params = task.parameters(1).unwrap();
            let mut values = Vec::new();
            scene::vector(&mut values, origin, 0.0);
            scene::vector(&mut values, direction, 0.0);
            params[32..64].copy_from_slice(&values);
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
            let value = task.read_linear().unwrap();
            if let Some((count, sigma, outer, top)) = expected {
                assert_eq!(
                    value[0],
                    [sigma.x as f32, sigma.y as f32, sigma.z as f32, 1.0],
                    "reverse={reverse} origin={origin:?} direction={direction:?}"
                );
                assert_eq!(value[1][..3], [outer as f32, top as f32, count as f32]);
                let scatter = match count {
                    2 => [0.3_f32, 0.35],
                    1 => [0.1, 0.2],
                    _ => [0.0, 0.0],
                };
                assert_eq!(value[2][..2], scatter, "initial-medium classification preserves the actual homogeneous random-walk parameters");
            } else {
                assert_eq!(
                    value[0][3], 0.0,
                    "an actual boundary/silhouette cannot certify an arbitrary incident normal: reverse={reverse} origin={origin:?} direction={direction:?}"
                );
            }
        }
    }
}

fn origin_emitter_fixture() -> ProductScene {
    let mut glass = slab_body("Pupil medium", Vec3::new(0.5, 1.0, 4.0), Vec3::new(-5.0, 0.0, 0.0), 1.0, Vec3::new(2.0_f64.ln(), 4.0_f64.ln(), 0.0));
    // The exact uploaded box is x[-10,0], y[-10,10], z[-2,2].
    assert_eq!((glass.mesh.bounds().min.x, glass.mesh.bounds().max.x), (-10.0, 0.0));
    let mut opaque = directional_receiver().meshes.remove(0);
    opaque.name = "Opaque behind emitter".into();
    for p in &mut Arc::make_mut(&mut opaque.mesh).positions {
        p.z = -1.5;
    }
    opaque.surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.5, 1.0).unwrap());
    glass.transmission = Some(Transmission::new(Vec3::new(2.0_f64.ln(), 4.0_f64.ln(), 0.0), None).unwrap());
    ProductScene { bounds: glass.mesh.bounds(), meshes: vec![glass, opaque] }
}
#[test]
fn gpu_actual_sampled_pupils_normalize_initial_beer_before_visible_emission_without_new_variates() {
    use solidcraft_render::job::{AreaLight, AreaLights, StudioLighting};
    let Some(executor) = executor() else { return };
    let mut image = request(1);
    image.camera.target = Vec3::new(0.25, 0.0, -0.5);
    image.camera.distance = 0.5;
    image.camera.fov = 0.2;
    let samples = 16;
    let seed = 71;
    let lens = solidcraft_render::job::ThinLens { aperture_radius_mm: 1.0, focus_distance_mm: 2.0 };
    let radiance = [2.0, 3.0, 5.0];
    let light = AreaLight { center: [0.0, 0.0, -1.0], u: [3.0, 0.0, 0.0], v: [0.0, 3.0, 0.0], radiance };
    let options = JobOptions {
        nested_media: true,
        samples,
        seed,
        max_depth: 8,
        lens: Some(lens),
        lighting: Some(StudioLighting { sun: None, environment: [0.0; 3], area_lights: AreaLights::new(&[light]).unwrap(), ..Default::default() }),
        ..Default::default()
    };
    let mut expected = [0.0; 3];
    let mut inside = 0;
    let mut outside = 0;
    let (right, up, back) = image.camera.basis();
    for ordinal in 0..samples {
        let mut rng = ordinal.wrapping_mul(0x9e3779b9) ^ seed;
        let mut random = || {
            rng = rng.wrapping_mul(747796405).wrapping_add(2891336453);
            let word = ((rng >> ((rng >> 28) + 4)) ^ rng).wrapping_mul(277803737);
            f64::from(((word >> 22) ^ word) >> 8) / 16777216.0
        };
        let x = random();
        let y = random();
        let u = random();
        let v = random();
        let displacement =
            (right * (std::f64::consts::TAU * v).cos() + up * (std::f64::consts::TAU * v).sin()) * (lens.aperture_radius_mm * u.sqrt());
        let origin = image.camera.eye() + displacement;
        let pinhole = image.camera.ray(x, y, 1.0, 1.0).1;
        let aimed = pinhole * (lens.focus_distance_mm / pinhole.dot(-back)) - displacement;
        let direction = aimed / aimed.len();
        if origin.x < 0.0 {
            inside += 1;
        } else {
            outside += 1;
        }
        let end = (-1.0 - origin.z) / direction.z;
        assert!(end > 0.0);
        let mut enter = f64::NEG_INFINITY;
        let mut exit = f64::INFINITY;
        for (p, d, lo, hi) in [(origin.x, direction.x, -10.0, 0.0), (origin.y, direction.y, -10.0, 10.0), (origin.z, direction.z, -2.0, 2.0)] {
            if d == 0.0 {
                if p < lo || p > hi {
                    exit = f64::NEG_INFINITY;
                    break;
                }
            } else {
                let a = (lo - p) / d;
                let b = (hi - p) / d;
                enter = enter.max(a.min(b));
                exit = exit.min(a.max(b));
            }
        }
        let length = (exit.min(end) - enter.max(0.0)).max(0.0);
        for (channel, sigma) in [2.0_f64.ln(), 4.0_f64.ln(), 0.0].into_iter().enumerate() {
            expected[channel] += radiance[channel] * (-sigma * length).exp() / f64::from(samples);
        }
    }
    assert!(inside > 0 && outside > 0, "one actual pupil disk must exercise both initial media");
    let render = |absorbing: bool| {
        let mut source = origin_emitter_fixture();
        if !absorbing {
            source.meshes[0].transmission = Some(clear_glass());
        }
        let mut task = Task::new(executor.context.clone(), source, image.clone(), options, CancelToken::default()).unwrap();
        let packed = task.parameters(0).unwrap();
        let coordinate = |axis: usize| f64::from(f32::from_le_bytes(packed[32 + axis * 4..36 + axis * 4].try_into().unwrap()));
        let actual_eye = Vec3::new(coordinate(0), coordinate(1), coordinate(2)) + image.camera.target;
        assert_eq!(f32::from_le_bytes(packed[44..48].try_into().unwrap()), 1.0, "pupil fixture uses the actual perspective GPU branch");
        assert!(actual_eye.dist(image.camera.eye()) < 1e-12, "the independent per-pupil oracle starts at the actual packed perspective eye");
        while task.sample < samples {
            task.complete_sample().unwrap();
        }
        task.read_linear().unwrap()[0]
    };
    assert_eq!(render(false), [2.0, 3.0, 5.0, 1.0], "same-IOR lossless pupil paths preserve exact source radiance");
    let actual = render(true);
    assert_eq!(actual[3], 1.0);
    for (a, e) in actual[..3].iter().zip(expected) {
        assert!((f64::from(*a) - e).abs() < 2e-5, "actual per-pupil origin and unchanged RNG: {actual:?} vs{expected:?}");
    }
}
#[test]
fn gpu_boundary_origins_refuse_through_existing_diagnostics_and_non_nested_outside_pixels_stay_exact() {
    use solidcraft_render::job::{AreaLight, AreaLights, StudioLighting};
    let Some(executor) = executor() else { return };
    for z in [-2.0, 2.0] {
        let mut image = request(4);
        image.camera.fov = 0.0;
        image.camera.distance = 0.5;
        image.camera.target = Vec3::new(-5.0, 0.0, z - 2.0);
        let mut task = Task::new(
            executor.context.clone(),
            origin_emitter_fixture(),
            image,
            JobOptions { nested_media: true, samples: 1, ..Default::default() },
            CancelToken::default(),
        )
        .unwrap();
        let packed = task.parameters(0).unwrap();
        let actual_z = f64::from(f32::from_le_bytes(packed[40..44].try_into().unwrap())) + task.request.camera.target.z;
        assert_eq!(actual_z, z, "actual packed orthographic origin lies on the selected boundary");
        task.complete_sample().unwrap();
        let error = task.read_linear().unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidScene);
        assert!(error.message.contains("code 4"));
    }
    let light = AreaLight { center: [-5.0, 0.0, -1.0], u: [3.0, 0.0, 0.0], v: [0.0, 3.0, 0.0], radiance: [2.0, 3.0, 5.0] };
    let mut image = request(4);
    image.camera.fov = 0.0;
    image.camera.target = Vec3::new(-5.0, 0.0, 0.0);
    image.camera.distance = 5.0;
    let render = |nested_media| {
        let options = JobOptions {
            nested_media,
            samples: 4,
            max_depth: 4,
            seed: 71,
            lighting: Some(StudioLighting {
                sun: None,
                environment: [0.0; 3],
                area_lights: AreaLights::new(&[light]).unwrap(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut task = Task::new(executor.context.clone(), origin_emitter_fixture(), image.clone(), options, CancelToken::default()).unwrap();
        let packed = task.parameters(0).unwrap();
        assert_eq!(
            f64::from(f32::from_le_bytes(packed[40..44].try_into().unwrap())) + image.camera.target.z,
            20.0,
            "the established outside ortho origin is not generic Camera::eye"
        );
        while task.sample < options.samples {
            task.complete_sample().unwrap();
        }
        task.read_linear().unwrap()
    };
    let legacy = render(false);
    assert!(legacy.iter().all(|p| p[0] > 0.0 && p[3] == 1.0));
    assert_eq!(legacy, render(true), "the extra opt-in query consumes no RNG and leaves an already supported outside path byte-identical");
}
