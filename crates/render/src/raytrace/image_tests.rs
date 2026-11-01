use super::*;
use solidcraft_geom::Aabb3;
fn emission_fixture(reverse: bool) -> (ProductScene, ImageRequest, RaytraceOptions) {
    use std::sync::Arc;
    let mut meshes = Vec::new();
    for (name, lo, hi) in [("emitter", -20.0, 0.0), ("receiver", 0.0, 20.0)] {
        let mesh = solidcraft_geom::Mesh {
            positions: vec![Vec3::new(lo, -10.0, 0.0), Vec3::new(hi, -10.0, 0.0), Vec3::new(hi, 10.0, 0.0), Vec3::new(lo, 10.0, 0.0)],
            normals: vec![Vec3::Z; 4],
            triangles: if reverse { vec![[0, 2, 1], [0, 3, 2]] } else { vec![[0, 1, 2], [0, 2, 3]] },
            tri_face: vec![0; 2],
            ..Default::default()
        };
        meshes.push(crate::ProductMesh {
            transmission: None,
            name: name.into(),
            mesh: Arc::new(mesh),
            color: [255; 3],
            opacity: 1.0,
            surface: None,
            faces: Vec::new(),
        });
    }
    let bounds = meshes.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.mesh.bounds()));
    let mut request = request();
    request.width = 2;
    request.height = 1;
    request.camera.distance = 25.0;
    request.camera.fov = 0.0;
    request.camera.set_view(crate::StandardView::Top);
    let options = RaytraceOptions {
        lights: Some(Vec::new()),
        emissions: Some(vec![job::SurfaceEmission {
            body_index: 0,
            body: "emitter".into(),
            color: [0.25, 0.5, 1.0],
            strength: 1.0,
            sides: job::EmissionSides::Front,
        }]),
        ..Default::default()
    };
    (ProductScene { meshes, bounds }, request, options)
}
fn emission_png(scene: ProductScene, mut request: ImageRequest, options: RaytraceOptions) -> Vec<u8> {
    request.raytrace = Some(options.clone());
    let mut task = ImageTask::new(scene, request, CancelToken::default(), options).unwrap();
    for _ in 0..10_000 {
        if let TaskPoll::Complete(png) = task.step(16384).unwrap() {
            return png;
        }
    }
    panic!("emission fixture did not complete");
}
#[test]
fn emission_actual_png_adds_linear_radiance_once_and_never_lights_neighbor() {
    let (scene, request, mut options) = emission_fixture(false);
    for ev in [0.0, 1.0] {
        options.exposure_ev = ev;
        let image = ::image::load_from_memory(&emission_png(scene.clone(), request.clone(), options.clone())).unwrap().to_rgba8();
        let expected = [0.25, 0.5, 1.0].map(|c| {
            let v = c * 2.0_f64.powf(ev);
            encode8(v / (1.0 + v))
        });
        assert_eq!(image.get_pixel(0, 0).0, [expected[0], expected[1], expected[2], 255]);
        assert_eq!(image.get_pixel(1, 0).0, [0, 0, 0, 255]);
    }
}
#[test]
fn emission_zero_default_png_equivalence_and_geometric_sidedness() {
    let (scene, request, mut options) = emission_fixture(false);
    options.emissions.as_mut().unwrap()[0].strength = 0.0;
    let zero = emission_png(scene.clone(), request.clone(), options.clone());
    options.emissions = None;
    assert_eq!(zero, emission_png(scene, request, options));
    let (scene, request, mut options) = emission_fixture(true);
    let image = ::image::load_from_memory(&emission_png(scene.clone(), request.clone(), options.clone())).unwrap().to_rgba8();
    assert_eq!(image.get_pixel(0, 0).0, [0, 0, 0, 255], "shading normals cannot turn a back face into an emitter");
    options.emissions.as_mut().unwrap()[0].sides = job::EmissionSides::Both;
    let image = ::image::load_from_memory(&emission_png(scene, request, options)).unwrap().to_rgba8();
    assert!(image.get_pixel(0, 0).0[0] > 0);
}
#[test]
fn emission_stale_duplicate_and_capacity_targets_refuse_before_work() {
    let (scene, mut request, mut options) = emission_fixture(false);
    options.emissions.as_mut().unwrap()[0].body = "renamed".into();
    request.raytrace = Some(options.clone());
    assert_eq!(admit(&scene, &request, &options).unwrap_err().code, ErrorCode::InvalidScene);
    let (_, _, mut options) = emission_fixture(false);
    let first = options.emissions.as_ref().unwrap()[0].clone();
    options.emissions.as_mut().unwrap().push(first);
    assert!(options.validate().is_err());
    options.emissions = Some(Vec::with_capacity(64));
    assert!(options.validate().is_err());
}
#[test]
fn emission_survives_face_reflectance_override_without_changing_captured_geometry() {
    let (mut scene, request, options) = emission_fixture(false);
    let original = scene.meshes[0].mesh.clone();
    let baseline = emission_png(scene.clone(), request.clone(), options.clone());
    scene.meshes[0].faces.push(crate::FaceAppearance { transmission: None, face: 0, color: [0; 3], opacity: 1.0, surface: None });
    assert_eq!(baseline, emission_png(scene.clone(), request, options));
    assert!(std::sync::Arc::ptr_eq(&original, &scene.meshes[0].mesh));
    assert_eq!(scene.meshes[0].faces[0].color, [0; 3]);
}
#[test]
fn emission_scan_observes_cancel_and_exhausts_shared_work_without_png() {
    let (scene, mut request, options) = emission_fixture(false);
    request.raytrace = Some(options.clone());
    let cancel = CancelToken::default();
    let mut task = ImageTask::new(scene, request, cancel.clone(), options).unwrap();
    for _ in 0..10_000 {
        if task.pending.as_ref().is_some_and(|p| p.hit.is_some() && p.light == task.lights.lights().len() && p.emission_cursor == 0) {
            task.work.limit = task.work.used() + 1;
            assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
            assert_eq!(task.step(1).err().unwrap().code, ErrorCode::LimitExceeded);
            assert!(!task.done);
            assert!(task.pixels.is_empty());
            cancel.cancel();
            assert_eq!(task.step(1).err().unwrap().code, ErrorCode::Cancelled);
            return;
        }
        assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
    }
    panic!("fixture never reached shading");
}
#[test]
fn thirty_two_emission_bindings_resume_with_four_work_units_and_cancel_without_publication() {
    for cancel_midway in [false, true] {
        let (mut scene, mut request, mut options) = emission_fixture(false);
        while scene.meshes.len() < 32 {
            let mut mesh = scene.meshes[1].clone();
            mesh.name = format!("receiver {}", scene.meshes.len());
            scene.meshes.push(mesh);
        }
        let base = options.emissions.as_ref().unwrap()[0].clone();
        let mut values = Vec::with_capacity(32);
        for index in (1..32).chain(std::iter::once(0)) {
            values.push(job::SurfaceEmission { body_index: index, body: scene.meshes[index].name.clone(), ..base.clone() });
        }
        options.emissions = Some(values);
        request.raytrace = Some(options.clone());
        let cancel = CancelToken::default();
        let mut task = ImageTask::new(scene, request, cancel.clone(), options).unwrap();
        for _ in 0..100_000 {
            if task.pending.as_ref().is_some_and(|p| p.hit.is_some() && p.light == task.lights.lights().len() && p.emission_cursor == 0) {
                break;
            }
            assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
        }
        assert_eq!(task.pending.as_ref().unwrap().emission_cursor, 0);
        assert_eq!(task.pending.as_ref().unwrap().radiance, Vec3::ZERO);
        for expected_cursor in (4..=32).step_by(4) {
            let before = task.work.used();
            assert!(matches!(task.step(16384).unwrap(), TaskPoll::Pending(_)));
            assert_eq!(task.work.used() - before, 4);
            let pending = task.pending.as_ref().unwrap();
            assert_eq!(pending.emission_cursor, expected_cursor);
            assert_eq!(pending.radiance, if expected_cursor == 32 { Vec3::new(0.25, 0.5, 1.0) } else { Vec3::ZERO });
            assert!(task.pixels.is_empty());
            if cancel_midway && expected_cursor == 16 {
                cancel.cancel();
                assert_eq!(task.step(4).err().unwrap().code, ErrorCode::Cancelled);
                assert_eq!(task.work.used() - before, 4);
                assert!(task.pixels.is_empty());
                break;
            }
        }
        if !cancel_midway {
            let before = task.work.used();
            assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
            assert_eq!(task.work.used() - before, 1);
            assert_eq!(task.pixel, 1);
            assert_eq!(task.pixels, [encode8(0.25 / 1.25), encode8(0.5 / 1.5), encode8(0.5), 255]);
        }
    }
}
fn request() -> ImageRequest {
    ImageRequest {
        studio_weights: None,
        environment: None,
        textures: None,
        integrator: job::Integrator::Raytrace,
        raytrace: None,
        camera: Default::default(),
        width: 8,
        height: 8,
        background: [10, 20, 30],
        transparent_background: false,
    }
}
#[test]
fn image_work_exhaustion_is_typed_and_never_publishes_partial_png() {
    let mut task =
        ImageTask::new(ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY }, request(), CancelToken::default(), Default::default()).unwrap();
    task.work = WorkBudget::new(2).unwrap();
    let mut previous = 0;
    for _ in 0..20 {
        match task.step(1) {
            Ok(TaskPoll::Pending(_)) => {
                assert!(task.work.used() >= previous);
                previous = task.work.used();
            }
            Ok(TaskPoll::Complete(_)) => panic!("exhausted task published a frame"),
            Err(e) => {
                assert_eq!(e.code, ErrorCode::LimitExceeded);
                assert_eq!(task.work.used(), 2);
                return;
            }
        }
    }
    panic!("tiny budget did not stop image");
}
#[test]
fn thin_shadow_occluder_survives_large_unrelated_translation() {
    for x in [0.0, 1.0e12 - 100.0] {
        let vertices = [Vec3::new(x, 0.0, 0.0), Vec3::new(x + 20.0, 0.0, 0.0), Vec3::new(x, 20.0, 0.0)];
        let receiver = Triangle::new(vertices).unwrap();
        let primary = Ray::new(Vec3::new(x + 5.0, 5.0, 10.0), -Vec3::Z, 0.0, 20.0).unwrap();
        let geometry = receiver.intersect(&primary).unwrap();
        let shadow = shadow_ray(primary, super::super::Hit { primitive_index: 0, geometry }, Vec3::Z, vertices).unwrap();
        assert!(shadow.interval()[0] > 0.0, "secondary rays exclude signed-zero distance after underflow");
        assert!(receiver.intersect(&shadow).is_none(), "self-shadow at translation {x}");
        let occluder = Triangle::new(vertices.map(|v| v + Vec3::Z * 0.005)).unwrap();
        assert!(occluder.intersect(&shadow).is_some(), "thin blocker was skipped at translation {x}");
    }
}
#[test]
fn primary_intervals_follow_camera_clip_planes_for_both_projections_and_up_axes() {
    for fov in [0.0, 0.7] {
        for up in [crate::UpAxis::Z, crate::UpAxis::Y] {
            let mut request = request();
            request.camera.fov = fov;
            request.camera.up = up;
            let clip = request.camera.clip(20.0);
            let ray = primary_ray(&request, 4.0, 4.0, clip).unwrap();
            let forward = -request.camera.back();
            let near_depth = (ray.origin() - request.camera.eye()).dot(forward);
            let far_depth = (ray.at(ray.interval()[1]).unwrap() - request.camera.eye()).dot(forward);
            assert!((near_depth - clip.0).abs() < 1e-9);
            assert!((far_depth - clip.1).abs() < 1e-9);
        }
    }
}
#[test]
fn resumable_cursor_matches_original_visibility_and_charges_each_unit() {
    let cancel = CancelToken::default();
    let triangle = Triangle::new([Vec3::ZERO, Vec3::X, Vec3::Y]).unwrap();
    let mut builder = BvhBuilder::new(vec![triangle], limits(job::CpuBudget::Compact), &cancel).unwrap();
    while !builder.step(1, &cancel).unwrap().complete {}
    let bvh = builder.finish().unwrap();
    let ray = Ray::new(Vec3::new(0.2, 0.2, 1.0), -Vec3::Z, 0.0, 2.0).unwrap();
    let expected = bvh.closest_hit(&ray, &mut WorkBudget::new(100).unwrap(), &cancel).unwrap();
    let mut cursor = TraceCursor::new(ray, false);
    let mut work = WorkBudget::new(100).unwrap();
    for _ in 0..100 {
        let before = work.used();
        let result = cursor.step(&bvh, &mut work, &cancel).unwrap();
        assert_eq!(work.used(), before + 1);
        if let Some(actual) = result {
            assert_eq!(actual, expected);
            return;
        }
    }
    panic!("cursor failed to finish");
}
#[test]
fn actual_png_has_analytic_lit_and_cast_shadow_pixels() {
    use crate::ProductMesh;
    use solidcraft_geom::Mesh;
    use std::sync::Arc;
    fn plane(z: f64, x0: f64, x1: f64) -> ProductMesh {
        let mesh = Mesh {
            positions: vec![Vec3::new(x0, -10.0, z), Vec3::new(x1, -10.0, z), Vec3::new(x1, 10.0, z), Vec3::new(x0, 10.0, z)],
            normals: vec![Vec3::Z; 4],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            tri_face: vec![0, 0],
            ..Default::default()
        };
        ProductMesh {
            transmission: None,
            name: format!("plane {z}"),
            mesh: Arc::new(mesh),
            color: [255; 3],
            opacity: 1.0,
            surface: Some(OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 1.0, 1.0, 1.5).unwrap()),
            faces: Vec::new(),
        }
    }
    let meshes = vec![plane(0.0, -20.0, 20.0), plane(5.0, -5.0, 5.0)];
    let bounds = meshes.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.mesh.bounds()));
    let scene = ProductScene { meshes, bounds };
    let mut request = request();
    request.width = 40;
    request.height = 40;
    request.camera.distance = 50.0;
    request.camera.set_view(crate::StandardView::Top);
    let options = RaytraceOptions {
        lights: Some(vec![job::DirectionalLightOptions { direction: [-1.0, 0.0, 1.0], color: [1.0; 3], intensity: 1.0 }]),
        ..Default::default()
    };
    fn render(scene: ProductScene, mut request: ImageRequest, options: RaytraceOptions) -> ::image::RgbaImage {
        request.raytrace = Some(options.clone());
        let mut task = ImageTask::new(scene, request, CancelToken::default(), options).unwrap();
        for _ in 0..100_000 {
            if let TaskPoll::Complete(png) = task.step(16384).unwrap() {
                return ::image::load_from_memory(&png).unwrap().to_rgba8();
            }
        }
        panic!("small analytic render did not complete");
    }
    let shadowed = render(scene.clone(), request.clone(), options.clone());
    let lit = render(scene, request, RaytraceOptions { shadows: false, ..options });
    // Top view: screen x maps world x, width40 spans[-20,20]. Blocker[-5,5]
    // at z5 casts a footprint[0,10] for the light toward(-1,0,+1).
    let shadow = shadowed.get_pixel(27, 20).0;
    let unshadowed = lit.get_pixel(27, 20).0;
    assert_eq!(shadow, [0, 0, 0, 255]);
    // White fully rough conductor: F=1, D=1/pi, Smith visibility
    // at normal view yields BRDF=1/[2*pi*(1+cosL)].
    let cosine = (0.5_f64).sqrt();
    let radiance = cosine / (2.0 * std::f64::consts::PI * (1.0 + cosine));
    let expected = encode8(radiance / (1.0 + radiance));
    assert_eq!(unshadowed, [expected, expected, expected, 255]);
    assert_eq!(shadowed.get_pixel(7, 20), lit.get_pixel(7, 20), "unoccluded receiver changes when toggling shadows");
}
fn triangle_task(cancel: CancelToken) -> ImageTask {
    use std::sync::Arc;
    let mesh = solidcraft_geom::Mesh {
        positions: vec![Vec3::new(-20.0, -20.0, 0.0), Vec3::new(20.0, -20.0, 0.0), Vec3::new(0.0, 20.0, 0.0)],
        normals: vec![Vec3::Z; 3],
        triangles: vec![[0, 1, 2]],
        tri_face: vec![0],
        ..Default::default()
    };
    let bounds = mesh.bounds();
    let scene = ProductScene {
        meshes: vec![crate::ProductMesh {
            transmission: None,
            name: "work fixture".into(),
            mesh: Arc::new(mesh),
            color: [220; 3],
            opacity: 1.0,
            surface: None,
            faces: Vec::new(),
        }],
        bounds,
    };
    let mut request = request();
    request.camera.distance = 50.0;
    request.camera.set_view(crate::StandardView::Top);
    ImageTask::new(scene, request, cancel, Default::default()).unwrap()
}
#[test]
fn build_primary_and_shadows_share_cumulative_work_and_exhaust_atomically() {
    let mut task = triangle_task(CancelToken::default());
    let mut previous = 0;
    let mut built = false;
    for _ in 0..10_000 {
        assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
        assert!(task.work.used() >= previous);
        previous = task.work.used();
        built |= task.builder.is_some();
        if task.pending.as_ref().is_some_and(|p| p.hit.is_some() && p.light < task.lights.lights().len()) {
            assert!(built);
            assert!(task.build_used > 0);
            assert!(task.work.used() > task.build_used);
            task.work.limit = task.work.used() + 1;
            assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
            assert_eq!(task.step(1).err().unwrap().code, ErrorCode::LimitExceeded);
            return;
        }
    }
    panic!("triangle fixture never reached shadow traversal");
}
#[test]
fn cancellation_is_observed_in_every_ray_image_phase() {
    for phase in 0..5 {
        let cancel = CancelToken::default();
        let mut task = triangle_task(cancel.clone());
        let mut reached = false;
        for _ in 0..100_000 {
            let current = match phase {
                0 => task.builder.is_none() && task.bvh.is_none(),
                1 => task.builder.is_some(),
                2 => task.pending.as_ref().is_some_and(|p| p.hit.is_none()),
                3 => task.pending.as_ref().is_some_and(|p| p.hit.is_some()),
                _ => task.encoder.is_some(),
            };
            if current {
                reached = true;
                break;
            }
            assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
        }
        assert!(reached, "phase {phase}");
        cancel.cancel();
        assert_eq!(task.step(4).err().unwrap().code, ErrorCode::Cancelled);
    }
}
#[test]
fn runtime_presets_reach_the_lighting_consumer() {
    for (runtime, expected) in [
        (job::LightingPreset::Neutral, StudioPreset::Neutral),
        (job::LightingPreset::WarmCool, StudioPreset::WarmCool),
        (job::LightingPreset::KeyRim, StudioPreset::KeyRim),
    ] {
        let options = RaytraceOptions { preset: Some(runtime), ..Default::default() };
        assert_eq!(lighting(&options).unwrap(), Lighting::preset(expected).unwrap());
    }
    let conflict = RaytraceOptions { preset: Some(job::LightingPreset::Neutral), lights: Some(Vec::new()), ..Default::default() };
    assert_eq!(lighting(&conflict).err().unwrap().code, ErrorCode::InvalidRequest);
}
#[test]
fn warm_cool_preset_changes_actual_triangle_png() {
    fn render(preset: job::LightingPreset) -> Vec<u8> {
        let initial = triangle_task(CancelToken::default());
        let options = RaytraceOptions { preset: Some(preset), ..Default::default() };
        let mut request = initial.request.clone();
        request.raytrace = Some(options.clone());
        let mut task = ImageTask::new(initial.scene.clone(), request, CancelToken::default(), options).unwrap();
        for _ in 0..100_000 {
            if let TaskPoll::Complete(bytes) = task.step(16384).unwrap() {
                return bytes;
            }
        }
        panic!("preset render did not complete");
    }
    let neutral = ::image::load_from_memory(&render(job::LightingPreset::Neutral)).unwrap().to_rgba8();
    let warm = ::image::load_from_memory(&render(job::LightingPreset::WarmCool)).unwrap().to_rgba8();
    assert_ne!(neutral.get_pixel(4, 4), warm.get_pixel(4, 4));
    assert_eq!(neutral.get_pixel(0, 0), warm.get_pixel(0, 0), "background must remain independent of lighting preset");
}

#[test]
fn emission_primary_aa_and_reflection_attempts_preserve_single_linear_value() {
    for s in [1, 4, 16] {
        for r in [1, 4, 16] {
            let (mut scene, request, mut options) = emission_fixture(false);
            for body in &mut scene.meshes {
                let mesh = std::sync::Arc::make_mut(&mut body.mesh);
                for p in &mut mesh.positions {
                    p.x *= 100.0;
                    p.y *= 100.0;
                }
            }
            scene.bounds = scene.meshes.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.mesh.bounds()));
            options.primary_samples = s;
            options.reflections = Some(job::ReflectionOptions {
                bodies: vec![job::ReflectionBody { body: "receiver".into(), mode: job::ReflectionMode::Mirror }],
                max_bounces: 1,
                samples: r,
            });
            let image = ::image::load_from_memory(&emission_png(scene, request, options)).unwrap().to_rgba8();
            assert_eq!(image.get_pixel(0, 0).0, [encode8(0.25 / 1.25), encode8(0.5 / 1.5), encode8(0.5), 255], "S{s}/R{r}");
            assert_eq!(image.get_pixel(1, 0).0, [0, 0, 0, 255], "emission does not illuminate the neighboring reflector");
        }
    }
}

#[test]
fn emission_and_directional_point_area_terms_share_one_linear_output_transform() {
    let (mut scene, mut request, mut options) = emission_fixture(false);
    scene.meshes.truncate(1);
    let body = &mut scene.meshes[0];
    for p in &mut std::sync::Arc::make_mut(&mut body.mesh).positions {
        p.x += 10.0;
    }
    body.surface = Some(OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 1.0, 1.0, 1.5).unwrap());
    scene.bounds = body.mesh.bounds();
    request.width = 1;
    request.height = 1;
    options.shadows = false;
    options.lights = Some(vec![job::DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 2.0 }]);
    options.punctual_lights = Some(vec![job::PunctualLightOptions {
        position_mm: [0.0, 0.0, 10.0],
        color: [1.0; 3],
        intensity: 3.0,
        reference_distance_mm: 10.0,
        minimum_distance_mm: 0.001,
        cone: None,
    }]);
    options.area_lights = Some(vec![job::RectangleLightOptions {
        center_mm: [0.0, 0.0, 10.0],
        edge_u_mm: [2.0, 0.0, 0.0],
        edge_v_mm: [0.0, -2.0, 0.0],
        color: [1.0; 3],
        intensity: 5.0,
    }]);
    options.area_samples = Some(1);
    let image = ::image::load_from_memory(&emission_png(scene, request, options)).unwrap().to_rgba8();
    // At normal incidence a white fully rough conductor has BRDF 1/(4*pi).
    // Point reference distance is 10 mm; the 4 mm² rectangle contributes 5*4/100.
    let direct = (2.0 + 3.0 + 5.0 * 4.0 / 100.0) / (4.0 * std::f64::consts::PI);
    let expected = [0.25, 0.5, 1.0].map(|e| encode8((e + direct) / (1.0 + e + direct)));
    assert_eq!(image.get_pixel(0, 0).0, [expected[0], expected[1], expected[2], 255]);
}

#[test]
fn reflected_hit_adds_surface_emission_with_the_current_path_throughput() {
    use std::sync::Arc;
    let normal = Vec3::new(1.0, 0.0, 1.0) / 2.0_f64.sqrt();
    let mirror_mesh = solidcraft_geom::Mesh {
        positions: vec![Vec3::new(-20.0, -20.0, 20.0), Vec3::new(20.0, -20.0, -20.0), Vec3::new(20.0, 20.0, -20.0), Vec3::new(-20.0, 20.0, 20.0)],
        normals: vec![normal; 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        tri_face: vec![0; 2],
        ..Default::default()
    };
    let emitter_mesh = solidcraft_geom::Mesh {
        positions: vec![Vec3::new(10.0, -4.0, -2.0), Vec3::new(10.0, -4.0, 2.0), Vec3::new(10.0, 4.0, 2.0), Vec3::new(10.0, 4.0, -2.0)],
        normals: vec![-Vec3::X; 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        tri_face: vec![0; 2],
        ..Default::default()
    };
    let base_color = Vec3::new(0.2, 0.5, 0.75);
    let meshes = vec![
        crate::ProductMesh {
            transmission: None,
            name: "mirror".into(),
            mesh: Arc::new(mirror_mesh),
            color: [255; 3],
            opacity: 1.0,
            surface: Some(OpaqueSurface::new(base_color, 1.0, 0.1, 1.5).unwrap()),
            faces: Vec::new(),
        },
        crate::ProductMesh {
            transmission: None,
            name: "emitter".into(),
            mesh: Arc::new(emitter_mesh),
            color: [0; 3],
            opacity: 1.0,
            surface: None,
            faces: Vec::new(),
        },
    ];
    let bounds = meshes.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.mesh.bounds()));
    let mut request = request();
    request.width = 1;
    request.height = 1;
    request.camera.distance = 50.0;
    request.camera.fov = 0.0;
    request.camera.set_view(crate::StandardView::Top);
    let options = RaytraceOptions {
        lights: Some(Vec::new()),
        shadows: false,
        emissions: Some(vec![job::SurfaceEmission {
            body_index: 1,
            body: "emitter".into(),
            color: [0.25, 0.5, 1.0],
            strength: 2.0,
            sides: job::EmissionSides::Front,
        }]),
        reflections: Some(job::ReflectionOptions {
            bodies: vec![job::ReflectionBody { body: "mirror".into(), mode: job::ReflectionMode::Mirror }],
            max_bounces: 1,
            samples: 1,
        }),
        ..Default::default()
    };
    let image = ::image::load_from_memory(&emission_png(ProductScene { meshes, bounds }, request, options)).unwrap().to_rgba8();
    let schlick = (1.0 - 0.5_f64.sqrt()).powi(5);
    let expected = [(0.2, 0.5), (0.5, 1.0), (0.75, 2.0)].map(|(f0, emitted)| {
        let radiance = (f0 + (1.0 - f0) * schlick) * emitted;
        encode8(radiance / (1.0 + radiance))
    });
    assert_eq!(image.get_pixel(0, 0).0, [expected[0], expected[1], expected[2], 255]);
}

#[test]
fn procedural_effects_change_completed_pngs_independently_and_zero_is_byte_identical() {
    use crate::{ProductMesh, surface_patterns::SurfaceEffect};
    use solidcraft_geom::Mesh;
    use std::sync::Arc;
    let mesh = Arc::new(Mesh {
        positions: vec![Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 0.0), Vec3::new(-10.0, 10.0, 0.0)],
        normals: vec![Vec3::Z; 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        tri_face: vec![0, 0],
        ..Default::default()
    });
    let original = mesh.positions.clone();
    let scene = ProductScene {
        bounds: mesh.bounds(),
        meshes: vec![ProductMesh {
            transmission: None,
            name: "Coupon".into(),
            mesh: mesh.clone(),
            color: [180; 3],
            opacity: 1.0,
            surface: Some(OpaqueSurface::new(Vec3::new(0.7, 0.5, 0.3), 1.0, 0.4, 1.5).unwrap()),
            faces: Vec::new(),
        }],
    };
    let effect = SurfaceEffect {
        body: "Coupon".into(),
        frame: Default::default(),
        repeat_mm: [12.0, 17.0, 20.0],
        roughness_amplitude: 0.0,
        bump_amplitude_mm: 0.0,
    };
    let render = |effect: Option<SurfaceEffect>| {
        let mut request = request();
        request.width = 32;
        request.height = 32;
        request.camera.distance = 40.0;
        request.camera.set_view(crate::StandardView::Top);
        let options = RaytraceOptions { surface_effects: effect.map(|e| vec![e]), shadows: false, ..Default::default() };
        request.raytrace = Some(options.clone());
        let mut task = ImageTask::new(scene.clone(), request, CancelToken::default(), options).unwrap();
        for _ in 0..100_000 {
            if let TaskPoll::Complete(png) = task.step(16384).unwrap() {
                return png;
            }
        }
        panic!("small surface fixture did not complete");
    };
    let baseline = render(None);
    assert_eq!(baseline, render(Some(effect.clone())));
    let rough = render(Some(SurfaceEffect { roughness_amplitude: 0.3, ..effect.clone() }));
    let bump = render(Some(SurfaceEffect { bump_amplitude_mm: 0.3, ..effect }));
    let base = ::image::load_from_memory(&baseline).unwrap().to_rgba8();
    for changed in [&rough, &bump] {
        let decoded = ::image::load_from_memory(changed).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), base.dimensions());
        assert_ne!(decoded, base);
        for (a, b) in decoded.pixels().zip(base.pixels()) {
            assert_eq!(a[3], b[3]);
        }
    }
    assert_ne!(rough, bump);
    assert_eq!(mesh.positions, original);
}

#[test]
fn surface_target_refusals_are_atomic_before_pixels_or_bvh_allocation() {
    use crate::surface_patterns::SurfaceEffect;
    let effect =
        SurfaceEffect { body: "missing".into(), frame: Default::default(), repeat_mm: [20.0; 3], roughness_amplitude: 0.2, bump_amplitude_mm: 0.0 };
    let scene = ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY };
    for effects in [vec![effect.clone()], vec![effect.clone(), effect]] {
        let options = RaytraceOptions { surface_effects: Some(effects), ..Default::default() };
        let mut request = request();
        request.raytrace = Some(options.clone());
        assert_eq!(admit(&scene, &request, &options).unwrap_err().code, ErrorCode::InvalidRequest);
    }
}

fn surface_consumer_fixture() -> (ProductScene, ImageRequest, RaytraceOptions) {
    use crate::{ProductMesh, surface_patterns::SurfaceEffect};
    use solidcraft_geom::Mesh;
    use std::sync::Arc;
    let mesh = Arc::new(Mesh {
        positions: vec![Vec3::new(-20.0, -20.0, 0.0), Vec3::new(20.0, -20.0, 0.0), Vec3::new(20.0, 20.0, 0.0), Vec3::new(-20.0, 20.0, 0.0)],
        normals: vec![Vec3::Z; 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        tri_face: vec![0, 0],
        ..Default::default()
    });
    let scene = ProductScene {
        bounds: mesh.bounds(),
        meshes: vec![ProductMesh {
            transmission: None,
            name: "Coupon".into(),
            mesh,
            color: [180; 3],
            opacity: 1.0,
            surface: Some(OpaqueSurface::new(Vec3::new(0.7, 0.5, 0.3), 1.0, 0.4, 1.5).unwrap()),
            faces: Vec::new(),
        }],
    };
    let options = RaytraceOptions {
        surface_effects: Some(vec![SurfaceEffect {
            body: "Coupon".into(),
            frame: Default::default(),
            repeat_mm: [12.0, 17.0, 20.0],
            roughness_amplitude: 0.2,
            bump_amplitude_mm: 0.3,
        }]),
        lights: Some(Vec::new()),
        shadows: false,
        ..Default::default()
    };
    let mut request = request();
    request.width = 1;
    request.height = 1;
    request.camera.distance = 40.0;
    request.camera.set_view(crate::StandardView::Top);
    request.raytrace = Some(options.clone());
    (scene, request, options)
}

fn surface_consumer_png(mut task: ImageTask, quantum: usize) -> Vec<u8> {
    for _ in 0..200_000 {
        let used = task.work.used();
        let poll = task.step(quantum).unwrap();
        assert!(task.work.used() - used <= 4, "surface consumer exceeded public work quantum");
        if let TaskPoll::Complete(png) = poll {
            return png;
        }
    }
    panic!("bounded surface consumer did not complete");
}

fn surface_constant_environment() -> crate::lighting::environment_input::EnvironmentInput {
    use crate::lighting::{HdrSource, environment_input::EnvironmentInput};
    let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
    for _ in 0..8 {
        bytes.extend_from_slice(&[128, 64, 32, 129]);
    }
    EnvironmentInput::new(HdrSource::new(bytes).unwrap(), 0.0, 1.0, 4).unwrap()
}

#[test]
fn surface_effect_shared_hit_reaches_directional_point_area_and_hdr_consumers() {
    use crate::job::{DirectionalLightOptions, PunctualLightOptions, RectangleLightOptions};
    use std::sync::Arc;
    for mask in 1..16 {
        for shadows in [false, true] {
            let (scene, mut request, mut options) = surface_consumer_fixture();
            options.shadows = shadows;
            if mask & 1 != 0 {
                options.lights = Some(vec![DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 2.0 }]);
            }
            if mask & 2 != 0 {
                options.punctual_lights = Some(vec![PunctualLightOptions {
                    position_mm: [0.0, 0.0, 10.0],
                    color: [1.0; 3],
                    intensity: 3.0,
                    reference_distance_mm: 10.0,
                    minimum_distance_mm: 0.001,
                    cone: None,
                }]);
            }
            if mask & 4 != 0 {
                options.area_lights = Some(vec![RectangleLightOptions {
                    center_mm: [0.0, 0.0, 10.0],
                    edge_u_mm: [2.0, 0.0, 0.0],
                    edge_v_mm: [0.0, -2.0, 0.0],
                    color: [1.0; 3],
                    intensity: 5.0,
                }]);
                options.area_samples = Some(1);
            }
            let effect = options.surface_effects.as_ref().unwrap()[0].prepare().unwrap();
            let changed = effect.apply(scene.meshes[0].surface.unwrap(), Vec3::Z, Vec3::Z, Vec3::ZERO).unwrap();
            let mut reference_scene = scene.clone();
            reference_scene.meshes[0].surface = Some(changed.surface);
            Arc::make_mut(&mut reference_scene.meshes[0].mesh).normals.fill(changed.shading_normal);
            let mut reference_options = options.clone();
            reference_options.surface_effects = None;
            let reference_request = ImageRequest { raytrace: Some(reference_options.clone()), ..request.clone() };
            request.raytrace = Some(options.clone());
            let environment = || if mask & 8 != 0 { Some(surface_constant_environment()) } else { None };
            let expected = surface_consumer_png(
                ImageTask::new_with_environment(reference_scene, reference_request, CancelToken::default(), reference_options, environment())
                    .unwrap(),
                1,
            );
            for quantum in [1, 4, 16384] {
                let actual = surface_consumer_png(
                    ImageTask::new_with_environment(scene.clone(), request.clone(), CancelToken::default(), options.clone(), environment()).unwrap(),
                    quantum,
                );
                assert_eq!(actual, expected, "mask={mask}, shadows={shadows}, quantum={quantum}");
            }
        }
    }
}

fn surface_at_thirty_two_binding_scan() -> ImageTask {
    use crate::{ProductMesh, surface_patterns::SurfaceEffect};
    use solidcraft_geom::Mesh;
    use std::sync::Arc;
    let (mut scene, mut request, mut options) = surface_consumer_fixture();
    let visible = scene.meshes.pop().unwrap();
    let mut effects = Vec::with_capacity(32);
    for index in 0..31 {
        let name = format!("unused-{index}");
        scene.meshes.push(ProductMesh {
            transmission: None,
            name: name.clone(),
            mesh: Arc::new(Mesh::default()),
            color: [255; 3],
            opacity: 1.0,
            surface: None,
            faces: Vec::new(),
        });
        effects.push(SurfaceEffect { body: name, frame: Default::default(), repeat_mm: [20.0; 3], roughness_amplitude: 0.1, bump_amplitude_mm: 0.0 });
    }
    scene.meshes.push(visible);
    effects.push(options.surface_effects.take().unwrap().remove(0));
    options.surface_effects = Some(effects);
    request.raytrace = Some(options.clone());
    let mut task = ImageTask::new(scene, request, CancelToken::default(), options).unwrap();
    for _ in 0..20_000 {
        if task.pending.as_ref().is_some_and(|p| p.hit.is_some() && !p.surface_ready && p.surface_cursor == 0) {
            return task;
        }
        task.step(1).unwrap();
    }
    panic!("did not reach actual unresolved surface scan");
}

#[test]
fn thirty_two_surface_bindings_resume_and_refuse_cancel_or_work_exhaustion_without_png() {
    let mut task = surface_at_thirty_two_binding_scan();
    for expected_cursor in [4, 8, 12, 16, 20, 24, 28, 32] {
        let before = task.work.used();
        assert!(matches!(task.step(16384).unwrap(), TaskPoll::Pending(_)));
        assert_eq!(task.work.used() - before, 4);
        let pending = task.pending.as_ref().unwrap();
        assert_eq!(pending.surface_cursor, expected_cursor);
        assert!(!pending.surface_ready);
        assert_eq!(pending.surface_index, if expected_cursor == 32 { Some(31) } else { None });
        assert!(task.pixels.is_empty() && !task.done);
    }
    let png = surface_consumer_png(task, 4);
    assert_eq!(::image::load_from_memory(&png).unwrap().width(), 1);
    for cancel in [false, true] {
        let mut task = surface_at_thirty_two_binding_scan();
        let before = task.work.used();
        if cancel {
            task.cancel.cancel();
        } else {
            task.work.limit = before;
        }
        assert_eq!(task.step(16384).err().unwrap().code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
        assert_eq!(task.work.used(), before);
        assert_eq!(task.pending.as_ref().unwrap().surface_cursor, 0);
        assert!(task.pixels.is_empty() && !task.done);
    }
}

#[test]
fn surface_effects_reject_ambiguous_targets_and_active_anisotropic_overlap() {
    use crate::job::{AnisotropicFaceOptions, AnisotropicOrientation};
    let (scene, mut request, mut options) = surface_consumer_fixture();
    let mut ambiguous = scene.clone();
    ambiguous.meshes.push(scene.meshes[0].clone());
    assert_eq!(admit(&ambiguous, &request, &options).unwrap_err().code, ErrorCode::InvalidRequest);
    options.anisotropic_faces[0] = Some(AnisotropicFaceOptions {
        body_index: 0,
        body_name: "Coupon".into(),
        face: 0,
        tangent: [1.0, 0.0, 0.0],
        orientation: AnisotropicOrientation::Zero,
        roughness_x: 0.2,
        roughness_y: 0.7,
    });
    request.raytrace = Some(options.clone());
    assert_eq!(admit(&scene, &request, &options).unwrap_err().code, ErrorCode::InvalidRequest);
    let identity = options.surface_effects.as_mut().unwrap().first_mut().unwrap();
    identity.roughness_amplitude = 0.0;
    identity.bump_amplitude_mm = 0.0;
    request.raytrace = Some(options.clone());
    assert!(admit(&scene, &request, &options).is_ok());
}

#[test]
fn reflected_surface_effect_uses_changed_shading_and_resets_before_emitting_secondary_hit() {
    use crate::surface_patterns::SurfaceEffect;
    use std::sync::Arc;
    let normal = Vec3::new(1.0, 0.0, 1.0) / 2.0_f64.sqrt();
    let mirror_mesh = solidcraft_geom::Mesh {
        positions: vec![Vec3::new(-20.0, -20.0, 20.0), Vec3::new(20.0, -20.0, -20.0), Vec3::new(20.0, 20.0, -20.0), Vec3::new(-20.0, 20.0, 20.0)],
        normals: vec![normal; 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        tri_face: vec![0; 2],
        ..Default::default()
    };
    let emitter_mesh = solidcraft_geom::Mesh {
        positions: vec![Vec3::new(10.0, -20.0, -20.0), Vec3::new(10.0, -20.0, 20.0), Vec3::new(10.0, 20.0, 20.0), Vec3::new(10.0, 20.0, -20.0)],
        normals: vec![-Vec3::X; 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        tri_face: vec![0; 2],
        ..Default::default()
    };
    let meshes = vec![
        crate::ProductMesh {
            transmission: None,
            name: "mirror".into(),
            mesh: Arc::new(mirror_mesh),
            color: [255; 3],
            opacity: 1.0,
            surface: Some(OpaqueSurface::new(Vec3::new(0.2, 0.5, 0.75), 1.0, 0.1, 1.5).unwrap()),
            faces: Vec::new(),
        },
        crate::ProductMesh {
            transmission: None,
            name: "emitter".into(),
            mesh: Arc::new(emitter_mesh),
            color: [0; 3],
            opacity: 1.0,
            surface: None,
            faces: Vec::new(),
        },
    ];
    let bounds = meshes.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.mesh.bounds()));
    let scene = ProductScene { meshes, bounds };
    let effect = SurfaceEffect {
        body: "mirror".into(),
        frame: Default::default(),
        repeat_mm: [12.0, 17.0, 20.0],
        roughness_amplitude: 0.2,
        bump_amplitude_mm: 0.3,
    };
    let changed = effect.prepare().unwrap().apply(scene.meshes[0].surface.unwrap(), normal, normal, Vec3::ZERO).unwrap();
    let mut reference_scene = scene.clone();
    reference_scene.meshes[0].surface = Some(changed.surface);
    Arc::make_mut(&mut reference_scene.meshes[0].mesh).normals.fill(changed.shading_normal);
    let mut request = request();
    request.width = 1;
    request.height = 1;
    request.camera.distance = 50.0;
    request.camera.fov = 0.0;
    request.camera.set_view(crate::StandardView::Top);
    for mode in [job::ReflectionMode::Mirror, job::ReflectionMode::RoughSpecular] {
        let options = RaytraceOptions {
            surface_effects: Some(vec![effect.clone()]),
            lights: Some(Vec::new()),
            shadows: false,
            emissions: Some(vec![job::SurfaceEmission {
                body_index: 1,
                body: "emitter".into(),
                color: [0.25, 0.5, 1.0],
                strength: 2.0,
                sides: job::EmissionSides::Front,
            }]),
            reflections: Some(job::ReflectionOptions {
                bodies: vec![job::ReflectionBody { body: "mirror".into(), mode }],
                max_bounces: 2,
                samples: 16,
            }),
            ..Default::default()
        };
        let mut reference_options = options.clone();
        reference_options.surface_effects = None;
        let expected = emission_png(reference_scene.clone(), request.clone(), reference_options);
        assert_eq!(emission_png(scene.clone(), request.clone(), options), expected, "mode={mode:?}");
        let decoded = ::image::load_from_memory(&expected).unwrap().to_rgba8();
        assert!(decoded.get_pixel(0, 0).0[..3].iter().any(|value| *value > 0));
    }
}

fn dielectric_triangle_task(cancel: CancelToken) -> ImageTask {
    let task = triangle_task(cancel.clone());
    let mut request = task.request;
    let options = RaytraceOptions {
        dielectrics: vec![job::DielectricOptions {
            body_index: 0,
            body_name: "work fixture".into(),
            boundary: job::DielectricBoundary::ThinSheet,
            ior: 1.5,
            absorption_per_mm: [0.0; 3],
            thickness_mm: 0.0,
        }],
        shadows: false,
        ..Default::default()
    };
    request.raytrace = Some(options.clone());
    ImageTask::new(task.scene, request, cancel, options).unwrap()
}

#[test]
fn dielectric_branch_traversal_exhausts_the_same_job_work_budget() {
    let mut task = dielectric_triangle_task(CancelToken::default());
    for _ in 0..100_000 {
        assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
        if task.optical.first().and_then(|optical| optical.pending.as_ref()).is_some_and(|state| state.paths > 1) {
            assert!(task.work.used() > task.build_used);
            task.work.limit = task.work.used();
            assert_eq!(task.step(1).err().unwrap().code, ErrorCode::LimitExceeded);
            assert!(!task.done);
            return;
        }
    }
    panic!("dielectric branch not reached");
}

#[test]
fn completed_dielectric_png_releases_capture_and_path_state() {
    let mut task = dielectric_triangle_task(CancelToken::default());
    let weak = std::sync::Arc::downgrade(&task.scene.meshes[0].mesh);
    for _ in 0..100_000 {
        if let TaskPoll::Complete(png) = task.step(16384).unwrap() {
            assert!(::image::load_from_memory(&png).is_ok());
            assert!(weak.upgrade().is_none());
            assert!(task.optical.is_empty());
            assert!(task.options.dielectrics.is_empty());
            assert!(task.request.raytrace.is_none());
            assert!(task.bvh.is_none());
            return;
        }
    }
    panic!("dielectric fixture did not complete");
}

#[test]
fn dielectric_fixed_stack_and_path_caps_return_typed_failures() {
    let ray = Ray::new(Vec3::Z, -Vec3::Z, 0.0, 2.0).unwrap();
    let mut state = DielectricPixel::new(ray);
    state.paths = MAX_PATHS;
    assert_eq!(state.push(state.path).err().unwrap().code, ErrorCode::LimitExceeded);
    state.paths = 1;
    state.stack_len = state.stack.len();
    assert_eq!(state.push(state.path).err().unwrap().code, ErrorCode::LimitExceeded);
}

#[test]
fn dielectric_image_path_applies_radiance_eta_squared_at_each_interface() {
    let options = RaytraceOptions {
        dielectrics: vec![dielectric::tests::input(job::DielectricBoundary::ClosedBox)],
        lights: Some(Vec::new()),
        shadows: false,
        ..Default::default()
    };
    let mut request = dielectric::tests::request(options.clone());
    request.camera.pitch = std::f64::consts::FRAC_PI_2;
    let mut task = ImageTask::new(dielectric::tests::scene(dielectric::tests::box_body(2.0)), request, CancelToken::default(), options).unwrap();
    let mut entry = None;
    for _ in 0..100_000 {
        assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
        if let Some(state) = task.optical.first().and_then(|optical| optical.pending.as_ref()) {
            let path = state.path;
            if path.boundaries == 1 && path.medium == Some(0) {
                // Air -> glass at normal incidence: (1-F0)/n^2 = .96/2.25.
                let expected = 0.96 / 2.25;
                assert!((path.throughput.x - expected).abs() < 1e-13);
                assert!((path.throughput.y - expected).abs() < 1e-13);
                assert!((path.throughput.z - expected).abs() < 1e-13);
                entry = Some(path.throughput.x);
            }
            if path.boundaries == 2 && path.medium.is_none() {
                // Glass -> air independently multiplies by .96*n^2 = 2.16.
                let ratio = path.throughput.x / entry.expect("entry checked before exit");
                assert!((ratio - 2.16).abs() < 1e-13);
                return;
            }
        }
    }
    panic!("slab's first entry and exit were not observed");
}

fn optical_sheet(name: &str, x: f64, z: f64) -> crate::ProductMesh {
    use std::sync::Arc;
    crate::ProductMesh {
        transmission: None,
        name: name.into(),
        mesh: Arc::new(solidcraft_geom::Mesh {
            positions: vec![Vec3::new(x - 20.0, -20.0, z), Vec3::new(x + 20.0, -20.0, z), Vec3::new(x + 20.0, 20.0, z), Vec3::new(x - 20.0, 20.0, z)],
            normals: vec![Vec3::Z; 4],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            ..Default::default()
        }),
        color: [255; 3],
        opacity: 1.0,
        surface: None,
        faces: Vec::new(),
    }
}

fn optical_input(body_index: usize, body_name: &str, ior: f64) -> job::DielectricOptions {
    job::DielectricOptions {
        body_index,
        body_name: body_name.into(),
        boundary: job::DielectricBoundary::ThinSheet,
        ior,
        absorption_per_mm: [0.0; 3],
        thickness_mm: 0.0,
    }
}

fn optical_png(mut task: ImageTask, quantum: usize) -> Vec<u8> {
    for _ in 0..2_000_000 {
        let before = task.work.used();
        let poll = task.step(quantum).unwrap();
        assert!(task.work.used() - before <= 4, "an optical continuation exceeded the shared work quantum");
        if let TaskPoll::Complete(png) = poll {
            return png;
        }
    }
    panic!("bounded optical fixture did not finish");
}

#[test]
fn dielectric_branches_sum_inside_each_spatial_and_reflection_attempt() {
    let mut scene = dielectric::tests::scene(dielectric::tests::box_body(2.0));
    scene.meshes.push(optical_sheet("unused reflector", 100.0, 0.0));
    scene.bounds = scene.meshes.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.mesh.bounds()));
    let mut baseline = None;
    for spatial in [1, 4, 16] {
        for attempts in [1, 4, 16] {
            let options = RaytraceOptions {
                primary_samples: spatial,
                dielectrics: vec![dielectric::tests::input(job::DielectricBoundary::ClosedBox)],
                environment: Some(job::RaytraceEnvironmentOptions { color: [1.0; 3], intensity: 1.0 }),
                reflections: Some(job::ReflectionOptions {
                    bodies: vec![job::ReflectionBody { body: "unused reflector".into(), mode: job::ReflectionMode::Mirror }],
                    samples: attempts,
                    max_bounces: 1,
                }),
                lights: Some(Vec::new()),
                shadows: false,
                ..Default::default()
            };
            let mut request = dielectric::tests::request(options.clone());
            request.camera.distance = 10.0;
            request.camera.fov = 0.0;
            for quantum in [1, 4, 16384] {
                let mut task = ImageTask::new(scene.clone(), request.clone(), CancelToken::default(), options.clone()).unwrap();
                let mut saw_complete_coverage = false;
                let mut done = None;
                for _ in 0..2_000_000 {
                    let before = task.work.used();
                    let poll = task.step(quantum).unwrap();
                    assert!(task.work.used() - before <= 4);
                    assert!(task.primary_coverage <= u16::from(spatial) * u16::from(attempts));
                    if task.emit_spatial_pixel {
                        assert_eq!(task.primary_coverage, u16::from(spatial) * u16::from(attempts), "branches are not coverage samples");
                        saw_complete_coverage = true;
                    }
                    if let Some(state) = task.optical.first().and_then(|o| o.pending.as_ref())
                        && state.path.boundaries > 0
                    {
                        assert!(!task.pending.as_ref().unwrap().repeatable_primary, "selected optical paths have no opaque reuse proof");
                    }
                    if let TaskPoll::Complete(png) = poll {
                        done = Some(png);
                        break;
                    }
                }
                let png = done.expect("bounded optical AA fixture completes");
                // A four-unit call may commit/reset coverage before yielding. The
                // one-unit run observes that state; every quantum checks final bytes.
                if spatial != 1 && quantum == 1 {
                    assert!(saw_complete_coverage);
                }
                let rgba = ::image::load_from_memory(&png).unwrap().to_rgba8();
                let pixel = rgba.get_pixel(0, 0).0;
                assert!(pixel[0] > 150 && pixel[0] == pixel[1] && pixel[1] == pixel[2]);
                if let Some(expected) = &baseline {
                    assert_eq!(&png, expected, "S={spatial}, R={attempts}, quantum={quantum}; branches sum, S/R average");
                } else {
                    baseline = Some(png);
                }
            }
        }
    }
}

#[test]
fn unit_ior_sheet_rejoins_surface_emission_and_every_existing_light_consumer() {
    use job::{DirectionalLightOptions, PunctualLightOptions, RectangleLightOptions};
    for mask in 1..16 {
        let (scene, mut request, mut options) = surface_consumer_fixture();
        request.camera.fov = 0.0;
        request.camera.distance = 10.0;
        // Shadows are deliberately off: this preview's shadow rays conservatively
        // treat every dielectric boundary as an opaque blocker.
        options.shadows = false;
        if mask & 1 != 0 {
            options.lights = Some(vec![DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 2.0 }]);
        }
        if mask & 2 != 0 {
            options.punctual_lights = Some(vec![PunctualLightOptions {
                position_mm: [0.0, 0.0, 10.0],
                color: [1.0; 3],
                intensity: 3.0,
                reference_distance_mm: 10.0,
                minimum_distance_mm: 0.001,
                cone: None,
            }]);
        }
        if mask & 4 != 0 {
            options.area_lights = Some(vec![RectangleLightOptions {
                center_mm: [0.0, 0.0, 10.0],
                edge_u_mm: [2.0, 0.0, 0.0],
                edge_v_mm: [0.0, -2.0, 0.0],
                color: [1.0; 3],
                intensity: 5.0,
            }]);
            options.area_samples = Some(4);
        }
        options.emissions = Some(vec![job::SurfaceEmission {
            body_index: 0,
            body: "Coupon".into(),
            color: [0.1, 0.2, 0.3],
            strength: 0.5,
            sides: job::EmissionSides::Front,
        }]);
        request.raytrace = Some(options.clone());
        let environment = || if mask & 8 != 0 { Some(surface_constant_environment()) } else { None };
        let expected = optical_png(
            ImageTask::new_with_environment(scene.clone(), request.clone(), CancelToken::default(), options.clone(), environment()).unwrap(),
            1,
        );
        let mut transmitted_scene = scene;
        transmitted_scene.meshes.push(optical_sheet("sheet", 0.0, 5.0));
        transmitted_scene.bounds = transmitted_scene.meshes.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.mesh.bounds()));
        options.dielectrics = vec![optical_input(1, "sheet", 1.0)];
        request.raytrace = Some(options.clone());
        for quantum in [1, 4, 16384] {
            let actual = optical_png(
                ImageTask::new_with_environment(transmitted_scene.clone(), request.clone(), CancelToken::default(), options.clone(), environment())
                    .unwrap(),
                quantum,
            );
            assert_eq!(actual, expected, "the same opaque light/surface/emission consumer after transmission; mask={mask}");
        }
    }
}

fn optical_at_eight_selector_scan() -> ImageTask {
    let (mut scene, mut request, mut options) = surface_consumer_fixture();
    for i in 0..8 {
        let name = format!("unused sheet {i}");
        let index = scene.meshes.len();
        scene.meshes.push(optical_sheet(&name, 100.0 + 50.0 * f64::from(i), 5.0));
        options.dielectrics.push(optical_input(index, &name, 1.5));
    }
    scene.bounds = scene.meshes.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.mesh.bounds()));
    request.raytrace = Some(options.clone());
    let mut task = ImageTask::new(scene, request, CancelToken::default(), options).unwrap();
    for _ in 0..200_000 {
        if task.pending.as_ref().is_some_and(|p| p.hit.is_some())
            && task.optical.first().and_then(|o| o.pending.as_ref()).is_some_and(|p| !p.ready && p.selector == 0)
        {
            return task;
        }
        task.step(1).unwrap();
    }
    panic!("actual optical selector scan not reached");
}

#[test]
fn eight_optical_selectors_resume_with_cancel_and_work_checks_before_mutation() {
    let mut task = optical_at_eight_selector_scan();
    for expected in [4, 8] {
        let before = task.work.used();
        assert!(matches!(task.step(16384).unwrap(), TaskPoll::Pending(_)));
        assert_eq!(task.work.used() - before, 4);
        let state = task.optical[0].pending.as_ref().unwrap();
        assert_eq!(state.selector, expected);
        assert!(!state.ready);
        assert!(task.pixels.is_empty());
    }
    assert!(!optical_png(task, 4).is_empty());
    for cancelled in [false, true] {
        let mut task = optical_at_eight_selector_scan();
        let before = task.work.used();
        if cancelled {
            task.cancel.cancel();
        } else {
            task.work.limit = before;
        }
        assert_eq!(task.step(4).err().unwrap().code, if cancelled { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
        assert_eq!(task.work.used(), before);
        assert_eq!(task.optical[0].pending.as_ref().unwrap().selector, 0);
        assert!(task.pixels.is_empty() && task.encoder.is_none() && !task.done);
    }
}

#[test]
fn optical_reuse_rejects_an_undrained_branch_and_a_selected_primary() {
    fn reusable() -> ImageTask {
        let mut task = optical_at_eight_selector_scan();
        // Start a fresh coherent request with an unreachable ordinary reflector to
        // request R=4, and a spatial grid whose visible receiver remains opaque.
        let mut scene = task.scene.clone();
        let mut request = task.request.clone();
        let mut options = task.options.clone();
        scene.meshes.push(optical_sheet("unused reflector", 1000.0, 0.0));
        scene.bounds = scene.meshes.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.mesh.bounds()));
        options.primary_samples = 4;
        options.reflections = Some(job::ReflectionOptions {
            bodies: vec![job::ReflectionBody { body: "unused reflector".into(), mode: job::ReflectionMode::Mirror }],
            max_bounces: 1,
            samples: 4,
        });
        request.camera.distance = 10.0;
        request.camera.fov = 0.0;
        request.raytrace = Some(options.clone());
        task = ImageTask::new(scene, request, CancelToken::default(), options).unwrap();
        for _ in 0..200_000 {
            if task.pending.as_ref().is_some_and(|p| p.light == task.lights.lights().len() + 1) {
                assert!(task.optical[0].pending.is_none(), "only drained optics reach the completed opaque marker");
                return task;
            }
            task.step(1).unwrap();
        }
        panic!("completed nonselected primary marker not reached");
    }
    for pending_branch in [false, true] {
        let mut task = reusable();
        if pending_branch {
            task.optical[0].pending = Some(DielectricPixel::new(task.pending.as_ref().unwrap().ray));
        } else {
            task.options.dielectrics[0].body_index = 0;
            task.options.dielectrics[0].body_name = "Coupon".into();
        }
        let before = task.primary_coverage;
        assert_eq!(task.step(1).err().unwrap().code, ErrorCode::InvalidScene);
        assert_eq!(task.primary_coverage, before);
        assert!(task.pixels.is_empty());
    }
}

#[test]
fn optical_admission_bills_capacity_and_refuses_ambiguous_effect_and_environment_contracts() {
    let scene = dielectric::tests::scene(dielectric::tests::box_body(2.0));
    let mut options = RaytraceOptions { dielectrics: vec![dielectric::tests::input(job::DielectricBoundary::ClosedBox)], ..Default::default() };
    let name = scene.meshes[0].name.clone();
    let bytes = options.retained_bytes().unwrap();
    assert_eq!(
        bytes,
        options.dielectrics.capacity() as u64 * std::mem::size_of::<job::DielectricOptions>() as u64
            + options.dielectrics[0].body_name.capacity() as u64
    );
    let mut request = dielectric::tests::request(options.clone());
    let with_optics = admit(&scene, &request, &options).unwrap();
    let plain = RaytraceOptions::default();
    let plain_request = ImageRequest { raytrace: Some(plain.clone()), ..request.clone() };
    let without_optics = admit(&scene, &plain_request, &plain).unwrap();
    assert_eq!(
        with_optics.estimated_total_bytes - without_optics.estimated_total_bytes,
        bytes + request.raytrace.as_ref().unwrap().retained_dielectric_bytes().unwrap() + std::mem::size_of::<OpticalTask>() as u64
    );
    options.surface_effects = Some(vec![crate::surface_patterns::SurfaceEffect {
        body: name,
        frame: Default::default(),
        repeat_mm: [10.0; 3],
        roughness_amplitude: 0.1,
        bump_amplitude_mm: 0.0,
    }]);
    request.raytrace = Some(options.clone());
    assert_eq!(admit(&scene, &request, &options).unwrap_err().code, ErrorCode::InvalidScene);
    options.surface_effects.as_mut().unwrap()[0].roughness_amplitude = 0.0;
    request.raytrace = Some(options.clone());
    assert!(admit(&scene, &request, &options).is_ok(), "an identity opaque effect does not change the geometric boundary");
    options.environment = Some(job::RaytraceEnvironmentOptions { color: [1.0; 3], intensity: 1.0 });
    request.raytrace = Some(options.clone());
    assert_eq!(
        options.validate_environment_reflections(&scene).unwrap_err().code,
        ErrorCode::InvalidRequest,
        "the same guard is called by engine preflight and host admission"
    );
    assert_eq!(
        ImageTask::new_with_environment(
            scene.clone(),
            request.clone(),
            CancelToken::default(),
            options.clone(),
            Some(surface_constant_environment())
        )
        .err()
        .unwrap()
        .code,
        ErrorCode::InvalidRequest
    );
    options.dielectrics = Vec::with_capacity(9);
    assert_eq!(options.validate().unwrap_err().code, ErrorCode::InvalidRequest);
}

#[test]
fn captured_clear_coat_keeps_direct_shading_but_refuses_unmatched_cpu_transport() {
    let (mut scene, mut request, mut options) = emission_fixture(false);
    let coat = crate::clear_coat::ClearCoat::new(0.8, 0.2, 1.5).unwrap();
    let surface = OpaqueSurface::new(Vec3::new(0.4, 0.3, 0.2), 0.0, 0.4, 1.5).unwrap().with_clear_coat(coat);
    scene.meshes[0].surface = Some(surface);
    request.raytrace = Some(options.clone());
    admit(&scene, &request, &options).unwrap();
    options.reflections = Some(job::ReflectionOptions {
        bodies: vec![job::ReflectionBody { body: "emitter".into(), mode: job::ReflectionMode::RoughSpecular }],
        max_bounces: 1,
        samples: 1,
    });
    request.raytrace = Some(options.clone());
    assert!(admit(&scene, &request, &options).unwrap_err().message.contains("clear-coat transport"));
    scene.meshes[0].surface = None;
    scene.meshes[0].faces.push(crate::FaceAppearance { face: 0, color: [255; 3], opacity: 1.0, surface: Some(surface), transmission: None });
    assert!(admit(&scene, &request, &options).unwrap_err().message.contains("clear-coat transport"));
}

#[test]
fn pending_pixel_heap_slot_stays_single_and_reuses_storage_until_completion() {
    let mut task = triangle_task(CancelToken::default());
    let initial = task.pending.0.as_ptr();
    assert_eq!(task.pending.0.capacity(), 1);
    assert!(task.pending.is_none());
    assert!(std::mem::size_of::<ImageTask>() <= 4096);
    let expected = admit(&task.scene, &task.request, &task.options).unwrap();
    assert_eq!(task.admission.estimated_total_bytes, expected.estimated_total_bytes);
    assert!(expected.preparation_bytes >= PendingPixelSlot::required_bytes());
    let mut seen = false;
    let mut completed = false;
    for _ in 0..100_000 {
        match task.step(1).unwrap() {
            TaskPoll::Pending(_) => {
                assert_eq!(task.pending.0.capacity(), 1);
                assert_eq!(task.pending.0.as_ptr(), initial, "samples must reuse the one admitted continuation slot");
                assert!(task.pending.0.len() <= 1);
                seen |= task.pending.is_some();
            }
            TaskPoll::Complete(png) => {
                assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
                assert_eq!(task.pending.0.capacity(), 0, "completed task releases continuation storage before its lease is shrunk");
                assert!(task.pending.is_none());
                completed = true;
                break;
            }
        }
    }
    assert!(completed, "fixture must complete within its bounded test work");
    assert!(seen, "fixture must exercise a live resumable pixel");
}

#[test]
fn cancelled_pending_pixel_allocation_is_refused_before_any_continuation() {
    let cancel = CancelToken::default();
    cancel.cancel();
    assert_eq!(PendingPixelSlot::new(&cancel).err().unwrap().code, ErrorCode::Cancelled);
}
