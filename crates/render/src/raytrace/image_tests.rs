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
fn face_looks_cost_one_unit_per_triangle_and_the_first_entry_wins() {
    use std::sync::Arc;
    let (columns, rows) = (8u32, 4u32);
    let mut positions = Vec::new();
    for y in 0..=rows {
        for x in 0..=columns {
            positions.push(Vec3::new(f64::from(x) * 5.0 - 20.0, f64::from(y) * 5.0 - 10.0, 0.0));
        }
    }
    let (mut triangles, mut tri_face) = (Vec::new(), Vec::new());
    for y in 0..rows {
        for x in 0..columns {
            let a = y * (columns + 1) + x;
            triangles.extend([[a, a + 1, a + columns + 2], [a, a + columns + 2, a + columns + 1]]);
            tri_face.extend([y * columns + x; 2]);
        }
    }
    let count = positions.len();
    let mesh = solidcraft_geom::Mesh { positions, normals: vec![Vec3::Z; count], triangles, tri_face, ..Default::default() };
    let look = |face: u32, color: [u8; 3]| crate::FaceAppearance { face, color, opacity: 1.0, surface: None, transmission: None };
    let mut faces: Vec<_> = (0..columns * rows).step_by(2).map(|face| look(face, [face as u8 * 8, 100, 200])).collect();
    // Later duplicates never replace the first entry for a face.
    faces.extend([look(2, [1, 2, 3]), look(4, [4, 5, 6])]);
    let entries = faces.len() as u64;
    let bounds = mesh.bounds();
    let scene = ProductScene {
        meshes: vec![crate::ProductMesh {
            transmission: None,
            name: "face look fixture".into(),
            mesh: Arc::new(mesh),
            color: [220; 3],
            opacity: 1.0,
            surface: None,
            faces,
        }],
        bounds,
    };
    let mut request = request();
    request.camera.distance = 60.0;
    request.camera.set_view(crate::StandardView::Top);
    let mut task = ImageTask::new(scene, request, CancelToken::default(), Default::default()).unwrap();
    let before = task.work.used();
    for _ in 0..10_000 {
        if task.builder.is_some() || task.bvh.is_some() {
            break;
        }
        assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
    }
    assert!(task.builder.is_some() || task.bvh.is_some(), "preparation did not finish");
    let triangles = u64::from(2 * columns * rows);
    // One unit per triangle and per face entry, plus the end of the body and the
    // hand-over to the BVH builder; scanning the entries per triangle cost ~T*F.
    assert_eq!(task.work.used() - before, triangles + entries + 2);
    let surface =
        |c: [u8; 3]| OpaqueSurface::new(Vec3::new(srgb8_to_linear(c[0]), srgb8_to_linear(c[1]), srgb8_to_linear(c[2])), 0.0, 0.7, 1.5).unwrap();
    assert_eq!(task.looks.len() as u64, triangles);
    for look in &task.looks {
        let face = look.face.unwrap();
        let expected = if face % 2 == 0 { [face as u8 * 8, 100, 200] } else { [220; 3] };
        assert_eq!(look.surface, surface(expected), "face {face}");
    }
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
            let environment = || {
                if mask & 8 != 0 { Some(surface_constant_environment()) } else { None }
            };
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
            shadow_transmission: false,
            subsurface: None,
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
        shadow_transmission: false,
        subsurface: None,
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
        let environment = || {
            if mask & 8 != 0 { Some(surface_constant_environment()) } else { None }
        };
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
        bytes
            + request.raytrace.as_ref().unwrap().retained_dielectric_bytes().unwrap()
            + std::mem::size_of::<OpticalTask>() as u64
            + super::super::legacy_certificates::Task::required_bytes(&scene, &options).unwrap()
            + std::mem::size_of::<super::CertifiedOptics>() as u64
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

fn progressive_optics_fixture(samples: u32, thin: bool, ior: f64, cancel: CancelToken) -> ImageTask {
    let mut body = dielectric::tests::box_body(8.0);
    let boundary = if thin {
        std::sync::Arc::make_mut(&mut body.mesh).triangles = vec![[4, 5, 7], [4, 7, 6]];
        job::DielectricBoundary::ThinSheet
    } else {
        job::DielectricBoundary::ClosedBox
    };
    let mut material = dielectric::tests::input(boundary);
    material.ior = ior;
    material.absorption_per_mm = [0.1, 0.2, 0.0];
    material.thickness_mm = if thin { 8.0 } else { 0.0 };
    let options = RaytraceOptions {
        progressive: Some(job::CpuProgressiveOptions { samples_per_pixel: samples, seed: 83 }),
        dielectrics: vec![material],
        environment: Some(job::RaytraceEnvironmentOptions { color: [1.0; 3], intensity: 1.0 }),
        lights: Some(Vec::new()),
        shadows: false,
        ..Default::default()
    };
    let mut request = dielectric::tests::request(options.clone());
    // A small parallel camera keeps every independently jittered sample well inside
    // the certified slab; this analytic fixture does not sample a boundary edge.
    request.camera.distance = 10.0;
    request.camera.fov = 0.0;
    ImageTask::new(dielectric::tests::scene(body), request, cancel, options).unwrap()
}

fn progressive_optics_complete(mut task: ImageTask, quantum: usize) -> (Vec<(u32, Vec<u8>)>, Vec<u8>, u64) {
    let mut previews = Vec::new();
    let mut branches = 0;
    for _ in 0..2_000_000 {
        let used = task.work.used();
        let transport = task.progress().phase == Phase::Running;
        let poll = task.step_with_previews(quantum).unwrap();
        if quantum == 1 && transport {
            assert!(task.work.used() - used <= 4, "an optical continuation exceeded the shared work quantum");
        }
        assert!(task.primary_coverage <= 1, "Fresnel branches cannot become additional camera coverage samples");
        if let Some(state) = task.optical.first().and_then(|o| o.pending.as_ref())
            && state.path.boundaries > 0
        {
            branches += 1;
            assert!(!task.pending.as_ref().unwrap().repeatable_primary);
        }
        match poll {
            ImagePoll::Pending(_) => {}
            ImagePoll::Preview { png, samples_per_pixel } => {
                assert!(task.optical.first().is_some_and(|o| o.pending.is_none()), "a complete pass must drain every optical branch");
                previews.push((samples_per_pixel, png));
            }
            ImagePoll::Complete(png) => {
                assert!(branches > 0, "the actual image producer must consume optical interfaces");
                assert!(task.optical.is_empty());
                return (previews, png, task.work.used());
            }
        }
    }
    panic!("bounded progressive optical fixture did not finish");
}

#[test]
fn progressive_certified_optics_keep_beer_lambert_energy_prefixes_and_shared_work() {
    for thin in [false, true] {
        for ior in [1.0, 1.5] {
            let actual = progressive_optics_complete(progressive_optics_fixture(5, thin, ior, CancelToken::default()), 1);
            assert_eq!(actual.0.iter().map(|p| p.0).collect::<Vec<_>>(), [1, 2, 3, 4]);
            assert_eq!(actual, progressive_optics_complete(progressive_optics_fixture(5, thin, ior, CancelToken::default()), 16_384));
            for (samples, png) in &actual.0 {
                let prefix = progressive_optics_complete(progressive_optics_fixture(*samples, thin, ior, CancelToken::default()), 3);
                assert_eq!(*png, prefix.1, "target sample count must not change an optical prefix");
            }
            if ior == 1.0 {
                // Index-matched slab/sheet: no reflection, exactly eight millimetres
                // of Beer attenuation in each independent camera sample.
                let linear = Vec3::new((-0.8f64).exp(), (-1.6f64).exp(), 1.0);
                let mapped = map_to_display(linear, 0.0).unwrap();
                let expected = [encode8(mapped.x), encode8(mapped.y), encode8(mapped.z), 255];
                assert_eq!(::image::load_from_memory(&actual.1).unwrap().to_rgba8().get_pixel(0, 0).0, expected);
            }
        }
    }
}

#[test]
fn progressive_certified_optical_branches_cancel_and_exhaust_before_any_late_pass() {
    for exhausted in [false, true] {
        let cancel = CancelToken::default();
        let mut task = progressive_optics_fixture(5, false, 1.5, cancel.clone());
        let mut stopped = false;
        for _ in 0..200_000 {
            assert!(matches!(task.step_with_previews(1).unwrap(), ImagePoll::Pending(_)));
            if task.optical.first().and_then(|o| o.pending.as_ref()).is_some_and(|state| state.path.boundaries > 0) {
                let used = task.work.used();
                if exhausted {
                    task.work.limit = used;
                } else {
                    cancel.cancel();
                }
                assert_eq!(task.step_with_previews(1).err().unwrap().code, if exhausted { ErrorCode::LimitExceeded } else { ErrorCode::Cancelled });
                assert_eq!(task.work.used(), used);
                assert!(!task.done);
                stopped = true;
                break;
            }
        }
        assert!(stopped, "test must stop inside a real Fresnel branch");
    }
}

type DenoiseOracleGuide = [[f64; 4]; 3];
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(x, y)| x * y).sum()
}
fn diff(a: [f64; 4], b: [f64; 4]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn xyz(a: [f64; 4]) -> [f64; 3] {
    [a[0], a[1], a[2]]
}

// Independent f64 gather oracle: all inputs are averaged linear values, so the
// GPU's accumulated-sample scaling is absent from this numerical implementation.
fn cpu_denoise_reference(width: usize, input: &[[f64; 4]], guides: &[DenoiseOracleGuide], step: usize) -> Vec<[f64; 4]> {
    let height = input.len() / width;
    let kernel = [1.0, 4.0, 6.0, 4.0, 1.0];
    input
        .iter()
        .enumerate()
        .map(|(i, &center)| {
            let [a, n, p] = guides[i];
            if a[3] == 0.0 || center[3] < 0.999 {
                return center;
            }
            let mut result = [0.0; 4];
            let mut total = 0.0;
            for (ky, &wy) in kernel.iter().enumerate() {
                for (kx, &wx) in kernel.iter().enumerate() {
                    let x = (i % width) as isize + (kx as isize - 2) * step as isize;
                    let y = (i / width) as isize + (ky as isize - 2) * step as isize;
                    if x < 0 || y < 0 || x >= width as isize || y >= height as isize {
                        continue;
                    }
                    let j = y as usize * width + x as usize;
                    let [b, m, q] = guides[j];
                    let value = input[j];
                    if a[3] != b[3] || (value[3] - center[3]).abs() > 0.01 {
                        continue;
                    }
                    let albedo = diff(a, b);
                    let position = diff(q, p);
                    let plane = dot(position, xyz(n)).abs() + dot(position, xyz(m)).abs();
                    let scale = (0.05 * p[3].max(q[3]) * step as f64).max(1e-6);
                    let depth = (n[3] - m[3]).abs() / (8.0 * p[3].max(q[3]) * step as f64).max(1e-6);
                    let color = diff(center, value);
                    let maximum = |v: [f64; 3]| v.into_iter().fold(0.0, f64::max);
                    let color_scale = 0.25 + 0.5 * maximum(xyz(center)).max(maximum(xyz(value)));
                    let weight = wx
                        * wy
                        * (-dot(albedo, albedo) / 0.04 - plane / scale - depth - maximum(color.map(f64::abs)) / color_scale).exp()
                        * dot(xyz(n), xyz(m)).max(0.0).powi(32);
                    for k in 0..3 {
                        result[k] += value[k] * weight;
                    }
                    total += weight;
                }
            }
            for v in result.iter_mut().take(3) {
                *v /= total;
            }
            result[3] = center[3];
            result
        })
        .collect()
}

fn denoise_test_guide(x: usize, y: usize) -> job::denoise::Guide {
    job::denoise::Guide { albedo: [0.5; 3], body: 1, normal: [0.0, 0.0, 1.0], depth: 4.0, position: [x as f32, y as f32, 0.0], footprint: 1.0 }
}
fn oracle_guides(guides: &[job::denoise::Guide]) -> Vec<DenoiseOracleGuide> {
    guides
        .iter()
        .map(|g| {
            [
                [f64::from(g.albedo[0]), f64::from(g.albedo[1]), f64::from(g.albedo[2]), f64::from(g.body)],
                [f64::from(g.normal[0]), f64::from(g.normal[1]), f64::from(g.normal[2]), f64::from(g.depth)],
                [f64::from(g.position[0]), f64::from(g.position[1]), f64::from(g.position[2]), f64::from(g.footprint)],
            ]
        })
        .collect()
}
fn complete_denoise(width: u32, pixels: Vec<[f32; 4]>, guides: &[job::denoise::Guide]) -> (Vec<[f32; 4]>, usize) {
    let mut task = job::denoise::DenoiseTask::new(width, pixels.len() as u32 / width, pixels).unwrap();
    let mut visits = 0;
    for _ in 0..1000 {
        let count = task.next_pixels();
        assert!(count <= job::denoise::MAX_STEP_PIXELS);
        visits += count;
        if task.step(guides, &CancelToken::default()).unwrap() {
            return (task.finish().unwrap(), visits);
        }
    }
    panic!("bounded filter did not finish")
}
#[test]
fn cpu_atrous_matches_independent_f64_gather_preserves_hdr_coverage_and_edges() {
    let width = 17u32;
    let count = (width * width) as usize;
    let guides: Vec<_> = (0..count).map(|i| denoise_test_guide(i % 17, i / 17)).collect();
    let pixels: Vec<_> = (0..count)
        .map(|i| {
            let hash = (i as u32).wrapping_mul(2654435761);
            let value = 0.4 + 0.2 * (hash >> 8) as f32 / 16777216.0;
            [value, value * 0.7, value * 1.3, 1.0]
        })
        .collect();
    let original = pixels.clone();
    let saved = guides.clone();
    let (actual, visits) = complete_denoise(width, pixels, &guides);
    let mut expected: Vec<_> = original.iter().map(|p| p.map(f64::from)).collect();
    for step in [1, 2, 4] {
        expected = cpu_denoise_reference(17, &expected, &oracle_guides(&guides), step);
    }
    assert_eq!(visits, 3 * count, "bounded pixel visits, not an invented scalar-FLOP budget");
    for (a, b) in actual.iter().zip(expected) {
        for c in 0..4 {
            assert!((f64::from(a[c]) - b[c]).abs() < 2e-6)
        }
    }
    let variance = |v: &[[f32; 4]]| {
        let mean = v.iter().map(|p| f64::from(p[0])).sum::<f64>() / v.len() as f64;
        v.iter().map(|p| (f64::from(p[0]) - mean).powi(2)).sum::<f64>() / v.len() as f64
    };
    assert!(variance(&actual) < variance(&original) / 4.0);
    assert_eq!(guides.iter().map(|g| g.position).collect::<Vec<_>>(), saved.iter().map(|g| g.position).collect::<Vec<_>>());
    for constant in [[0.75, 0.25, 0.5, 1.0], [1e30, 1e20, 1e10, 1.0], [0.7, 0.3, 0.1, 0.5]] {
        assert_eq!(complete_denoise(width, vec![constant; count], &guides).0, vec![constant; count]);
    }
    for edge in 0..6 {
        let mut g = guides.clone();
        let mut p = vec![[0.0, 0.0, 0.0, 1.0]; count];
        for (i, value) in p.iter_mut().enumerate() {
            if i % 17 >= 8 {
                *value = [1.0; 4];
                match edge {
                    0 => g[i].body = 2,
                    1 => g[i].normal = [1.0, 0.0, 0.0],
                    2 => g[i].position[2] = 10.0,
                    3 => g[i].albedo = [1.0; 3],
                    4 => g[i].depth = 1004.0,
                    _ => g[i].body = 0,
                }
            } else if edge == 3 {
                g[i].albedo = [0.0; 3]
            }
        }
        let filtered = complete_denoise(width, p.clone(), &g).0;
        assert!(filtered.iter().zip(p).all(|(a, b)| (a[0] - b[0]).abs() < 1e-12), "edge {edge}");
    }
    let bypass = vec![job::denoise::Guide::default(); count];
    assert_eq!(complete_denoise(width, original.clone(), &bypass).0, original);
}

fn denoised_plane(samples: u32, denoise: bool, output: Option<job::OutputOptions>) -> ImageTask {
    let (scene, mut request, _) = emission_fixture(false);
    request.width = 13;
    request.height = 13;
    let options = RaytraceOptions {
        denoise,
        output: output.map(Box::new),
        progressive: Some(job::CpuProgressiveOptions { samples_per_pixel: samples, seed: 7 }),
        lights: Some(Vec::new()),
        area_lights: Some(vec![job::RectangleLightOptions {
            center_mm: [0.0, 0.0, 10.0],
            edge_u_mm: [14.0, 0.0, 0.0],
            edge_v_mm: [0.0, -14.0, 0.0],
            color: [1.0, 0.7, 0.3],
            intensity: 10.0,
        }]),
        area_samples: Some(1),
        seed: Some(31),
        ..Default::default()
    };
    request.raytrace = Some(options.clone());
    ImageTask::new(scene, request, CancelToken::default(), options).unwrap()
}
type DenoisedImageOutput = (Vec<(u32, Vec<u8>)>, Vec<u8>, Option<Vec<u8>>, u64);
fn finish_denoise_image(mut task: ImageTask, units: usize) -> DenoisedImageOutput {
    let mut previews = Vec::new();
    for _ in 0..300000 {
        match task.step_with_previews(units).unwrap() {
            ImagePoll::Pending(_) => {}
            ImagePoll::Preview { png, samples_per_pixel } => {
                assert!(task.take_linear_exr().is_none());
                previews.push((samples_per_pixel, png));
            }
            ImagePoll::Complete(png) => {
                assert_eq!(task.denoise.capacity(), 0, "guides are work-owned, never retained with the final master");
                return (previews, png, task.take_linear_exr(), task.work.used());
            }
        }
    }
    panic!("bounded denoised image did not finish")
}
#[test]
fn cpu_denoising_filters_actual_complete_prefixes_without_changing_raw_film_or_exr() {
    let output = job::OutputOptions { linear_exr: true, ..Default::default() };
    let first = finish_denoise_image(denoised_plane(4, true, Some(output)), 1);
    assert_eq!(first.0.iter().map(|p| p.0).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(first, finish_denoise_image(denoised_plane(4, true, Some(output)), 16384));
    for (samples, png) in &first.0 {
        assert_eq!(png, &finish_denoise_image(denoised_plane(*samples, true, Some(output)), 3).1)
    }
    let raw = finish_denoise_image(denoised_plane(4, false, Some(output)), 1);
    assert_eq!(first.2, raw.2, "EXR is encoded before filtering from identical absolute Monte Carlo samples");
    assert_ne!(first.1, raw.1, "the actual CPU soft-light picture must pass through the filter");
    let mut task = denoised_plane(2, true, None);
    for _ in 0..300000 {
        if task.linear.len() == 169 && task.output.is_empty() {
            break;
        }
        assert!(!matches!(task.step_with_previews(1).unwrap(), ImagePoll::Complete(_)));
    }
    assert_eq!(task.linear.len(), 169);
    let guides = task.denoise.first().unwrap().pixels().unwrap().to_vec();
    assert!(guides.iter().all(|g| g.body > 0));
    assert!(guides.iter().all(|g| (g.normal[2] - 1.0).abs() < 1e-6));
    let film: Vec<_> = (0..169).map(|i| task.progressive.as_ref().unwrap().film.pixel(i).unwrap()).collect();
    let mut expected: Vec<_> = task.linear.iter().map(|p| p.map(f64::from)).collect();
    for step in [1, 2, 4] {
        expected = cpu_denoise_reference(13, &expected, &oracle_guides(&guides), step)
            .into_iter()
            .map(|pixel| pixel.map(|value| f64::from(value as f32)))
            .collect()
    }
    let linear: Vec<_> = expected.iter().map(|p| p.map(|v| v as f32)).collect();
    let mut rgba = vec![0; 169 * 4];
    job::presentation::Presentation { background: task.request.background, ..Default::default() }
        .display(13, 13)
        .unwrap()
        .encode(&linear, &mut rgba, &CancelToken::default())
        .unwrap();
    let png = loop {
        match task.step_with_previews(1).unwrap() {
            ImagePoll::Preview { png, .. } => break png,
            ImagePoll::Pending(_) => {}
            ImagePoll::Complete(_) => panic!("first prefix was lost"),
        }
    };
    assert_eq!(::image::load_from_memory(&png).unwrap().to_rgba8().as_raw(), &rgba);
    assert_eq!((0..169).map(|i| task.progressive.as_ref().unwrap().film.pixel(i).unwrap()).collect::<Vec<_>>(), film);
}
#[test]
fn cpu_denoising_admits_actual_guides_and_colour_scratch_only_when_enabled() {
    let plain = denoised_plane(4, false, None);
    let filtered = denoised_plane(4, true, None);
    assert_eq!(plain.denoise.capacity(), 0);
    assert_eq!(filtered.denoise.capacity(), 1);
    let expected = 169 * 80 + std::mem::size_of::<super::super::denoise::Guides>() as u64 + std::mem::size_of::<job::output::OutputTask>() as u64;
    assert_eq!(filtered.admission.estimated_total_bytes - plain.admission.estimated_total_bytes, expected);
    let output = Some(job::OutputOptions { linear_exr: true, ..Default::default() });
    let plain = denoised_plane(4, false, output);
    let filtered = denoised_plane(4, true, output);
    assert_eq!(
        filtered.admission.estimated_total_bytes - plain.admission.estimated_total_bytes,
        169 * 64 + std::mem::size_of::<super::super::denoise::Guides>() as u64
    );
    let mut request = plain.request.clone();
    request.width = 8193;
    request.height = 1;
    let options = RaytraceOptions { denoise: true, budget: job::CpuBudget::Workstation, ..Default::default() };
    request.raytrace = Some(options.clone());
    assert!(admit(&plain.scene, &request, &options).unwrap_err().message.contains("CPU denoising"));
}
#[test]
fn cpu_denoising_guides_and_filter_keep_cancellation_and_cumulative_work() {
    for guide_stage in [true, false] {
        for cancel in [true, false] {
            let mut task = denoised_plane(2, true, Some(job::OutputOptions { linear_exr: true, ..Default::default() }));
            for _ in 0..300000 {
                let reached = if guide_stage {
                    task.bvh.is_some() && task.denoise.first().is_some_and(|g| !g.ready())
                } else {
                    task.output.first().is_some_and(|o| o.stage_name() == "denoise")
                };
                if reached {
                    break;
                }
                assert!(!matches!(task.step_with_previews(1).unwrap(), ImagePoll::Complete(_)));
            }
            if guide_stage {
                assert!(!task.denoise.first().unwrap().ready())
            } else {
                assert_eq!(task.output.first().unwrap().stage_name(), "denoise")
            }
            if cancel {
                task.cancel.cancel()
            } else {
                task.work.limit = task.work.used()
            }
            assert_eq!(task.step_with_previews(1).err().unwrap().code, if cancel { ErrorCode::Cancelled } else { ErrorCode::LimitExceeded });
            assert!(!task.done);
            assert!(task.take_linear_exr().is_none());
        }
    }
}

fn actual_denoise_guides(mut task: ImageTask) -> Vec<job::denoise::Guide> {
    for _ in 0..300000 {
        if task.denoise.first().is_some_and(|guides| guides.ready()) {
            assert_eq!(task.pixel, 0, "guide construction cannot consume a Monte Carlo sample");
            assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
            return task.denoise.first().unwrap().pixels().unwrap().to_vec();
        }
        assert!(!matches!(task.step_with_previews(1).unwrap(), ImagePoll::Complete(_)));
    }
    panic!("bounded production guides did not finish")
}
#[test]
fn cpu_denoise_production_guides_use_exact_body_and_material_bypass_policy() {
    for case in 0..7 {
        let base = denoised_plane(2, true, None);
        let mut source = base.scene.clone();
        let mut options = base.options.clone();
        match case {
            0 | 1 => {
                for body in &mut source.meshes {
                    body.surface = Some(OpaqueSurface::new(Vec3::new(0.4, 0.6, 0.8), 1.0, if case == 0 { 0.2 } else { 0.3 }, 1.5).unwrap());
                }
            }
            2 | 3 => {
                for body in &mut source.meshes {
                    body.surface = Some(
                        OpaqueSurface::new(Vec3::new(0.4, 0.6, 0.8), 0.0, 0.5, 1.5)
                            .unwrap()
                            .with_clear_coat(crate::clear_coat::ClearCoat::new(1.0, if case == 2 { 0.045 } else { 0.1 }, 1.5).unwrap()),
                    );
                }
            }
            4 => {
                options.dielectrics = source
                    .meshes
                    .iter()
                    .enumerate()
                    .map(|(body_index, body)| job::DielectricOptions {
                        shadow_transmission: false,
                        subsurface: None,
                        body_index,
                        body_name: body.name.clone(),
                        boundary: job::DielectricBoundary::ThinSheet,
                        ior: 1.5,
                        absorption_per_mm: [0.0; 3],
                        thickness_mm: 0.0,
                    })
                    .collect();
            }
            5 => {
                options.reflections = Some(job::ReflectionOptions {
                    samples: 1,
                    bodies: source
                        .meshes
                        .iter()
                        .map(|body| job::ReflectionBody { body: body.name.clone(), mode: job::ReflectionMode::Mirror })
                        .collect(),
                    ..Default::default()
                })
            }
            _ => {
                options.emissions = Some(
                    source
                        .meshes
                        .iter()
                        .enumerate()
                        .map(|(body_index, body)| job::SurfaceEmission {
                            body_index,
                            body: body.name.clone(),
                            color: [1.0; 3],
                            strength: 1.0,
                            sides: job::EmissionSides::Both,
                        })
                        .collect(),
                )
            }
        }
        let request = ImageRequest { raytrace: Some(options.clone()), ..base.request.clone() };
        let guides = actual_denoise_guides(ImageTask::new(source, request, CancelToken::default(), options).unwrap());
        let eligible = matches!(case, 1 | 3);
        assert!(guides.iter().all(|g| (g.body != 0) == eligible), "case {case}");
        if eligible {
            assert!(guides.iter().all(|g| g.body == 1 || g.body == 2));
            assert!(guides.iter().all(|g| g.albedo == [0.4, 0.6, 0.8]));
        }
    }
}
#[test]
fn cpu_denoise_center_guides_are_translation_stable_and_independent_of_pupil_sampling() {
    let base = denoised_plane(2, true, None);
    let expected = actual_denoise_guides(denoised_plane(2, true, None));
    let shift = Vec3::new(9e8, -8e8, 7e8);
    let mut scene = base.scene.clone();
    for body in &mut scene.meshes {
        let mut mesh = (*body.mesh).clone();
        for position in &mut mesh.positions {
            *position += shift;
        }
        body.mesh = std::sync::Arc::new(mesh);
    }
    scene.bounds = scene.meshes.iter().fold(Aabb3::EMPTY, |bounds, body| bounds.union(&body.mesh.bounds()));
    let mut request = base.request.clone();
    request.camera.target += shift;
    let translated = actual_denoise_guides(ImageTask::new(scene, request, CancelToken::default(), base.options.clone()).unwrap());
    for (a, b) in expected.iter().zip(translated) {
        assert_eq!((a.body, a.albedo, a.normal), (b.body, b.albedo, b.normal));
        for i in 0..3 {
            assert!((a.position[i] - b.position[i]).abs() < 1e-4);
        }
        assert!((a.depth - b.depth).abs() < 1e-4);
        assert_eq!(a.footprint, b.footprint);
    }
    let mut request = base.request.clone();
    request.camera.fov = 0.8;
    let no_lens = actual_denoise_guides(ImageTask::new(base.scene.clone(), request.clone(), CancelToken::default(), base.options.clone()).unwrap());
    let mut options = base.options.clone();
    options.lens = Some(job::ThinLens { aperture_radius_mm: 0.2, focus_distance_mm: 25.0 });
    request.raytrace = Some(options.clone());
    let lens = actual_denoise_guides(ImageTask::new(base.scene.clone(), request, CancelToken::default(), options).unwrap());
    for (a, b) in no_lens.iter().zip(lens) {
        assert_eq!((a.body, a.albedo, a.normal, a.position, a.depth, a.footprint), (b.body, b.albedo, b.normal, b.position, b.depth, b.footprint));
    }
}
#[test]
fn cpu_denoising_rejects_invalid_raw_radiance_and_never_masks_it_with_bypass() {
    for body in [0, 1] {
        let mut guide = denoise_test_guide(0, 0);
        guide.body = body;
        for pixel in [[f32::NAN, 0.0, 0.0, 1.0], [f32::INFINITY, 0.0, 0.0, 1.0], [-1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 1.1]] {
            let mut task = job::denoise::DenoiseTask::new(1, 1, vec![pixel]).unwrap();
            assert!(task.step(&[guide], &CancelToken::default()).is_err());
        }
    }
    let cancel = CancelToken::default();
    cancel.cancel();
    let mut task = job::denoise::DenoiseTask::new(1, 1, vec![[1.0; 4]]).unwrap();
    assert_eq!(task.step(&[denoise_test_guide(0, 0)], &cancel).unwrap_err().code, ErrorCode::Cancelled);
}

#[test]
fn cpu_denoise_hit_and_miss_guide_continuations_charge_one_unit_before_each_store() {
    for miss in [false, true] {
        for exhaust_at_store in [false, true] {
            let base = denoised_plane(2, true, None);
            let source = if miss { ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY } } else { base.scene.clone() };
            let mut task = ImageTask::new(source, base.request.clone(), CancelToken::default(), base.options.clone()).unwrap();
            let mut guide_calls = 0;
            let mut tested_store = false;
            for _ in 0..300000 {
                if task.bvh.is_some() && task.denoise.first().is_some_and(|guides| !guides.ready()) {
                    let guide = task.denoise.first().unwrap();
                    let stored = guide.completed_pixels();
                    let at_store = guide.ready_to_store(&task.options);
                    let before = task.work.used();
                    if exhaust_at_store && at_store {
                        task.work.limit = before;
                        assert_eq!(task.step_with_previews(1).err().unwrap().code, ErrorCode::LimitExceeded);
                        assert_eq!(task.work.used(), before);
                        let guide = task.denoise.first().unwrap();
                        assert_eq!(guide.completed_pixels(), stored);
                        assert!(guide.ready_to_store(&task.options), "an exhausted debit cannot consume the pending guide");
                        assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
                        assert!(task.take_linear_exr().is_none());
                        tested_store = true;
                        break;
                    }
                    assert!(matches!(task.step_with_previews(1).unwrap(), ImagePoll::Pending(_)));
                    assert_eq!(task.work.used() - before, 1, "one guide call; miss={miss}, store={at_store}");
                    guide_calls += 1;
                    if task.denoise.first().unwrap().ready() {
                        break;
                    }
                } else {
                    assert!(!matches!(task.step_with_previews(1).unwrap(), ImagePoll::Complete(_)));
                }
            }
            assert!(guide_calls > 0);
            if exhaust_at_store {
                assert!(tested_store);
            } else {
                let guides = task.denoise.first().unwrap();
                assert!(guides.ready());
                assert_eq!(guides.completed_pixels(), 169);
                assert!(guides.pixels().unwrap().iter().all(|guide| (guide.body == 0) == miss));
                assert_eq!(task.progressive.as_ref().unwrap().film.samples(), 0);
            }
        }
    }
}

#[test]
fn adaptive_certified_optics_drain_pilot_branches_and_keep_exact_independent_production_beer_energy() {
    use crate::job::adaptive::AdaptiveSampling;
    for thin in [false, true] {
        for ior in [1.0, 1.5] {
            let build = || {
                let original = progressive_optics_fixture(16, thin, ior, CancelToken::default());
                let options = RaytraceOptions {
                    adaptive: Some(Box::new(AdaptiveSampling { pilot_samples: 16, min_samples: 16, ..Default::default() })),
                    ..original.options
                };
                let request = ImageRequest { raytrace: Some(options.clone()), ..original.request };
                ImageTask::new(original.scene, request, CancelToken::default(), options).unwrap()
            };
            let actual = progressive_optics_complete(build(), 1);
            assert_eq!(actual, progressive_optics_complete(build(), 16384));
            assert_eq!(actual.0.iter().map(|p| p.0).collect::<Vec<_>>(), (1..32).collect::<Vec<_>>());
            let production = progressive_optics_complete(progressive_optics_fixture(16, thin, ior, CancelToken::default()), 3);
            assert_eq!(actual.1, production.1, "discarded pilots cannot alter the existing16-sample production optical estimator");
            if ior == 1.0 {
                let mapped = map_to_display(Vec3::new((-0.8_f64).exp(), (-1.6_f64).exp(), 1.0), 0.0).unwrap();
                let expected = [encode8(mapped.x), encode8(mapped.y), encode8(mapped.z), 255];
                for (_, png) in actual.0.iter().chain(std::iter::once(&(32, actual.1.clone()))) {
                    assert_eq!(
                        ::image::load_from_memory(png).unwrap().to_rgba8().get_pixel(0, 0).0,
                        expected,
                        "all pilot/production prefixes retain the independent8mm Beer oracle"
                    );
                }
            }
        }
    }
}

#[test]
fn prepaid_legacy_certificate_and_standalone_cursor_share_cumulative_work_and_exact_output() {
    let scene = dielectric::tests::scene(dielectric::tests::box_body(2.0));
    let options = RaytraceOptions {
        dielectrics: vec![dielectric::tests::input(job::DielectricBoundary::ClosedBox)],
        lights: Some(Vec::new()),
        environment: Some(job::RaytraceEnvironmentOptions { color: [1.0; 3], intensity: 1.0 }),
        output: Some(Box::new(job::OutputOptions { linear_exr: true, ..Default::default() })),
        ..Default::default()
    };
    let request = dielectric::tests::request(options.clone());
    let cancel = CancelToken::default();
    let proof = certify_for_request(&scene, &request, &options, &cancel).unwrap().unwrap();
    let prepared_work = proof.work_used();
    assert!(prepared_work > 12, "the complete tiled-boundary proof is real work");
    let mut prepared = ImageTask::new_prepared(scene.clone(), request.clone(), cancel, options.clone(), None, Some(proof)).unwrap();
    let mut standalone = ImageTask::new(scene, request, CancelToken::default(), options).unwrap();
    assert!(prepared.optical[0].preparation.is_empty(), "the factory consumes the proof instead of certifying again");
    assert!(!standalone.optical[0].preparation.is_empty());
    assert_eq!(prepared.work.used(), prepared_work);
    while !standalone.optical[0].preparation.is_empty() {
        let before = standalone.work.used();
        assert!(matches!(standalone.step(1).unwrap(), TaskPoll::Pending(_)));
        assert_eq!(standalone.work.used() - before, 1);
        assert!(standalone.triangles.is_empty() && standalone.pixels.is_empty(), "proof precedes geometry/pixels");
    }
    assert_eq!(standalone.work.used(), prepared.work.used(), "background proof/store credit transfers into the one cumulative budget");
    let complete = |task: &mut ImageTask| {
        for _ in 0..100_000 {
            if let TaskPoll::Complete(png) = task.step(1).unwrap() {
                return png;
            }
        }
        panic!("tiny certified image did not complete");
    };
    let a = complete(&mut prepared);
    let b = complete(&mut standalone);
    assert_eq!(a, b);
    let raw = prepared.take_linear_exr().expect("final raw EXR");
    assert_eq!(Some(raw.as_slice()), standalone.take_linear_exr().as_deref(), "prepared proof does not alter raw master bytes");
    assert_eq!(prepared.work.used(), standalone.work.used());
    // Independent native uncompressed FLOAT EXR scanline: one1×1 chunk stores
    // the four A/B/G/R channels, all exactly1 for this lossless white furnace.
    assert_eq!(&raw[..8], &[0x76, 0x2f, 0x31, 0x01, 2, 0, 0, 0]);
    let mut header = 8;
    while raw[header] != 0 {
        header += raw[header..].iter().position(|byte| *byte == 0).unwrap() + 1;
        header += raw[header..].iter().position(|byte| *byte == 0).unwrap() + 1;
        let count = u32::from_le_bytes(raw[header..header + 4].try_into().unwrap()) as usize;
        header += 4 + count;
    }
    header += 1;
    let chunk = u64::from_le_bytes(raw[header..header + 8].try_into().unwrap()) as usize;
    assert_eq!(chunk, header + 8);
    assert_eq!(i32::from_le_bytes(raw[chunk..chunk + 4].try_into().unwrap()), 0);
    assert_eq!(u32::from_le_bytes(raw[chunk + 4..chunk + 8].try_into().unwrap()), 16);
    let channels: [f32; 4] = std::array::from_fn(|i| f32::from_le_bytes(raw[chunk + 8 + i * 4..chunk + 12 + i * 4].try_into().unwrap()));
    assert_eq!(channels, [1.0; 4], "lossless raw radiance/coverage remains exactly1 before presentation");
    // Default Reinhard maps linear1 to.5; standard sRGB encoding rounds to188.
    assert_eq!(
        ::image::load_from_memory(&a).unwrap().to_rgba8().get_pixel(0, 0).0,
        [188, 188, 188, 255],
        "white furnace follows the existing presentation contract"
    );
}

#[test]
fn standalone_legacy_preparation_cancellation_never_advances_or_allocates_pixels() {
    let scene = dielectric::tests::scene(dielectric::tests::box_body(2.0));
    let options = RaytraceOptions { dielectrics: vec![dielectric::tests::input(job::DielectricBoundary::ClosedBox)], ..Default::default() };
    let request = dielectric::tests::request(options.clone());
    let cancel = CancelToken::default();
    let mut task = ImageTask::new(scene, request, cancel.clone(), options).unwrap();
    assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
    let before = (task.work.used(), task.optical[0].preparation.len(), task.triangles.len(), task.pixels.len());
    cancel.cancel();
    assert_eq!(task.step(1).err().unwrap().code, ErrorCode::Cancelled);
    assert_eq!((task.work.used(), task.optical[0].preparation.len(), task.triangles.len(), task.pixels.len()), before);
}

fn mapped_wood_fixture() -> (ProductScene, ImageRequest, RaytraceOptions) {
    use crate::job::textures::{BaseColorMode, PlanarFrame, RuntimeTextures, TextureBinding, TextureMap};
    use image::ImageEncoder;
    use std::sync::Arc;
    let mesh = solidcraft_geom::Mesh {
        positions: vec![Vec3::new(-40., -40., 0.), Vec3::new(40., -40., 0.), Vec3::new(40., 40., 0.), Vec3::new(-40., 40., 0.)],
        normals: vec![Vec3::Z; 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        tri_face: vec![0, 0],
        ..Default::default()
    };
    let bounds = mesh.bounds();
    let surface = OpaqueSurface::new(Vec3::new(1., 1., 1.), 0., 0.7, 1.).unwrap();
    let scene = ProductScene {
        bounds,
        meshes: vec![crate::ProductMesh {
            name: "Wood board".into(),
            mesh: Arc::new(mesh),
            color: [255; 3],
            opacity: 1.,
            surface: Some(surface),
            transmission: None,
            faces: vec![],
        }],
    };
    // Original diagnostic grain: alternating opaque dark/light fibres with known mean.
    let mut encoded = Vec::new();
    let pixels: Vec<u8> = (0..64)
        .flat_map(|i| {
            let value = if i % 8 % 2 == 0 { 0 } else { 255 };
            [value, value, value, 255]
        })
        .collect();
    image::codecs::png::PngEncoder::new(&mut encoded).write_image(&pixels, 8, 8, image::ExtendedColorType::Rgba8).unwrap();
    let textures = RuntimeTextures::new(vec![TextureBinding {
        body_index: 0,
        body_name: "Wood board".into(),
        frame: PlanarFrame { origin: [0.; 3], u: [1., 0., 0.], v: [0., 1., 0.] },
        normal_strength: 0.4,
        base_color: Some(TextureMap::new(encoded, crate::texture_input::MapRole::BaseColor).unwrap()),
        normal: None,
        roughness: None,
        metallic: None,
        base_color_mode: BaseColorMode::Replace,
        flip_v: false,
    }])
    .unwrap();
    let mut request = request();
    request.camera.set_view(crate::StandardView::Top);
    request.camera.distance = 1.25;
    request.textures = Some(Arc::new(textures));
    let options = RaytraceOptions {
        lights: Some(vec![job::DirectionalLightOptions { direction: [0., 0., 1.], color: [1.; 3], intensity: std::f64::consts::PI }]),
        shadows: false,
        ..Default::default()
    };
    request.raytrace = Some(options.clone());
    (scene, request, options)
}
fn mapped_png(scene: ProductScene, request: ImageRequest, options: RaytraceOptions, quantum: usize) -> Vec<u8> {
    let mut task = ImageTask::new(scene, request, CancelToken::default(), options).unwrap();
    for _ in 0..300_000 {
        let before = task.work.used();
        let result = task.step(quantum).unwrap();
        // Four bounded continuations; a PNG row keeps its legacy explicit row cost.
        if let TaskPoll::Complete(png) = result {
            return png;
        }
        assert!(task.work.used() >= before);
    }
    panic!("mapped image did not finish");
}
#[test]
fn cpu_wood_keeps_close_fibres_and_minifies_distant_fibres_to_the_linear_mean() {
    let (scene, mut request, options) = mapped_wood_fixture();
    let mesh = scene.meshes[0].mesh.clone();
    let close = mapped_png(scene.clone(), request.clone(), options.clone(), 1);
    assert_eq!(close, mapped_png(scene.clone(), request.clone(), options.clone(), 7));
    let image = image::load_from_memory(&close).unwrap().to_rgba8();
    let values: Vec<_> = image.pixels().map(|p| p[0]).collect();
    assert!(values.iter().max().unwrap() - values.iter().min().unwrap() > 150, "magnified fibres must remain distinct: {values:?}");
    request.camera.distance = 25.;
    let far = mapped_png(scene.clone(), request.clone(), options.clone(), 7);
    let image = image::load_from_memory(&far).unwrap().to_rgba8();
    // Linear mean reflectance is 0.5. Unit irradiance/pi yields 0.5 radiance,
    // then the existing Reinhard presentation maps 0.5 to 1/3 before sRGB.
    let expected = encode8(1. / 3.);
    assert!(image.pixels().all(|p| p.0 == [expected, expected, expected, 255]), "distant grain must converge to linear-light mean");
    assert!(std::sync::Arc::ptr_eq(&mesh, &scene.meshes[0].mesh));
    assert_eq!(scene.meshes[0].surface.unwrap().dielectric_ior(), 1.);
}
#[test]
fn cpu_wood_scalar_face_override_suppresses_inherited_body_maps() {
    let (mut scene, mut request, options) = mapped_wood_fixture();
    scene.meshes[0].faces.push(crate::FaceAppearance {
        face: 0,
        color: [64, 96, 128],
        opacity: 1.,
        surface: Some(OpaqueSurface::new(Vec3::new(0.2, 0.3, 0.4), 0., 0.7, 1.).unwrap()),
        transmission: None,
    });
    let mapped = mapped_png(scene.clone(), request.clone(), options.clone(), 1);
    request.textures = None;
    assert_eq!(mapped, mapped_png(scene, request, options, 7));
}
#[test]
fn cpu_wood_footprint_uses_unwrapped_physical_spacing_and_applies_supersampling_once() {
    let (_, request, mut options) = mapped_wood_fixture();
    let clip = request.camera.clip(100.);
    let ray = primary_ray(&request, 0.5, 0.5, clip).unwrap();
    let point = ray.origin() + ray.direction() * (-ray.origin().z / ray.direction().z);
    let first = primary_footprint(&request, &options, None, 0, 0, clip, point, Vec3::Z).unwrap().unwrap();
    assert!((first[0].len() - 0.125).abs() < 1e-12);
    assert!((first[1].len() - 0.125).abs() < 1e-12);
    for samples in [4, 16] {
        options.primary_samples = samples;
        let offset = primary_offset(samples, 0).unwrap();
        let ray = primary_ray(&request, offset[0], offset[1], clip).unwrap();
        let p = ray.origin() + ray.direction() * (-ray.origin().z / ray.direction().z);
        let footprint = primary_footprint(&request, &options, None, 0, 0, clip, p, Vec3::Z).unwrap().unwrap();
        for delta in footprint {
            assert!((delta.len() - 0.125 / f64::from(samples).sqrt()).abs() < 1e-12);
        }
    }
}
#[test]
fn cpu_wood_preparation_cancel_and_role_work_limit_never_publish_partial_pixels() {
    let (scene, request, options) = mapped_wood_fixture();
    let cancel = CancelToken::default();
    let mut task = ImageTask::new(scene, request, cancel.clone(), options).unwrap();
    let cache = task.maps.first().unwrap();
    assert!(!cache.ready());
    assert_eq!(cache.prepared_maps(), 0);
    assert_eq!(cache.preparation_phase(), None);
    for _ in 0..100_000 {
        if task.maps.first().is_some_and(|cache| cache.hit_role() == Some(1)) {
            break;
        }
        assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
    }
    assert_eq!(task.maps.first().unwrap().prepared_maps(), 1);
    assert_eq!(task.maps.first().unwrap().hit_role(), Some(1));
    assert!(task.pixels.is_empty());
    task.work.limit = task.work.used();
    assert_eq!(task.step(1).err().unwrap().code, ErrorCode::LimitExceeded);
    assert_eq!(task.maps.first().unwrap().hit_role(), Some(1));
    cancel.cancel();
    assert_eq!(task.step(1).err().unwrap().code, ErrorCode::Cancelled);
    assert!(!task.done);
}
