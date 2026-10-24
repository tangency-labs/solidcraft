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
