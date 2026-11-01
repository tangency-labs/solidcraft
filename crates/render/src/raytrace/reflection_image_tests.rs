use super::*;
use crate::ProductMesh;
use crate::job::{ReflectionBody, ReflectionMode, ReflectionOptions};
use solidcraft_geom::Mesh;
use std::sync::Arc;

fn visible_second_body_fixture(spatial: u8, samples: u8) -> ImageTask {
    let mut source = scene();
    source.meshes[0].surface = Some(OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 1.0, 1.0, 1.5).unwrap());
    source.meshes[0].mesh = Arc::new(Mesh {
        positions: vec![
            Vec3::new(-100.0, -100.0, -100.0),
            Vec3::new(100.0, -100.0, -100.0),
            Vec3::new(100.0, 100.0, 100.0),
            Vec3::new(-100.0, 100.0, 100.0),
        ],
        normals: vec![Vec3::new(0.0, -1.0, 1.0); 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        tri_face: vec![0, 0],
        ..Default::default()
    });
    // Camera and reflector lie INSIDE this separate inward-facing box. A
    // primary hits the nearer reflector first; every valid secondary leaving
    // it eventually intersects body1. No emitter or test-only transport hook.
    let mut target = Mesh::default();
    for normal in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z] {
        let tangent = if normal.x == 0.0 { Vec3::X } else { Vec3::Y };
        let bitangent = normal.cross(tangent);
        let center = -normal * 10_000.0;
        let first = target.positions.len() as u32;
        for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            target.positions.push(center + tangent * (x * 10_000.0) + bitangent * (y * 10_000.0));
            target.normals.push(normal);
        }
        target.triangles.extend([[first, first + 1, first + 2], [first, first + 2, first + 3]]);
        target.tri_face.extend([0, 0]);
    }
    source.meshes[1].mesh = Arc::new(target);
    source.bounds = source.meshes.iter().fold(solidcraft_geom::Aabb3::EMPTY, |bounds, body| bounds.union(&body.mesh.bounds()));
    let mut req = request(Some(reflection_options(ReflectionMode::RoughSpecular)));
    req.width = 1;
    req.height = 1;
    // Perspective near clipping starts inside the box. Orthographic near
    // clipping includes geometry behind the eye and starts outside this box.
    // tan(0.25) < 1/3: the whole film hits |x|, |y|=|z| < 25, within
    // the reflector extent100 and before any target box wall10000.
    req.camera.fov = 0.5;
    let options = req.raytrace.as_mut().unwrap();
    options.primary_samples = spatial;
    options.reflections.as_mut().unwrap().samples = samples;
    options.reflections.as_mut().unwrap().max_bounces = 1;
    let options = options.clone();
    ImageTask::new(source, req, CancelToken::default(), options).unwrap()
}

#[test]
fn visible_transport_hits_a_real_second_body_and_completes_all_spatial_reflection_attempts() {
    for spatial in [1_u8, 4, 16] {
        for samples in [1_u8, 4, 16, 64] {
            let mut task = visible_second_body_fixture(spatial, samples);
            let mut primary_attempts = std::collections::BTreeSet::new();
            let mut secondary_spawns = std::collections::BTreeSet::new();
            let mut second_body_hits = std::collections::BTreeSet::new();
            let expected = usize::from(spatial) * usize::from(samples);
            let mut completed = None;
            for _ in 0..200_000 {
                let absent = task.pending.is_none();
                let work = task.work.used();
                let poll = task.step(1).unwrap();
                assert!(task.work.used() - work <= 4);
                if let Some(pending) = task.pending.as_ref() {
                    let key = (task.spatial, task.sample);
                    if absent && pending.bounce == 0 {
                        assert!(primary_attempts.insert(key), "a fixed S,R attempt was retried");
                    }
                    if pending.bounce == 0
                        && let Some(hit) = pending.hit
                    {
                        assert_eq!(task.looks[hit.primitive_index].body_index, 0, "every primary must reach the selected reflector first");
                    }
                    if pending.bounce == 1 {
                        secondary_spawns.insert(key);
                        if let Some(hit) = pending.hit {
                            assert_eq!(task.looks[hit.primitive_index].body_index, 1, "spawn alone is insufficient: resolve body identity");
                            second_body_hits.insert(key);
                        }
                    }
                }
                if !task.pixels.is_empty() {
                    assert_eq!(primary_attempts.len(), expected, "pixel published before every S*R attempt, including nulls");
                }
                if let TaskPoll::Complete(png) = poll {
                    completed = Some(png);
                    break;
                }
            }
            assert_eq!(task.pixel, 1);
            assert!(task.done);
            assert_eq!(primary_attempts.len(), expected);
            assert_eq!(second_body_hits, secondary_spawns, "every accepted secondary must finish tracing its actual second body");
            if spatial == 1 {
                assert!(!second_body_hits.is_empty(), "the centre fixture must actually hit another body");
                if samples == 64 {
                    assert!(secondary_spawns.len() < expected, "unit roughness must also exercise null attempts without retry/renormalization");
                }
            }
            let decoded = ::image::load_from_memory(&completed.expect("bounded full S*R traversal did not complete")).unwrap().to_rgba8();
            assert_eq!(decoded.dimensions(), (1, 1));
            assert_eq!(decoded.get_pixel(0, 0).0[3], 255);
        }
    }
}

#[test]
fn visible_rough_transport_keeps_cancellation_and_work_caps_at_sample_count_extremes() {
    for samples in [1, 4, 16, 64] {
        for cancel_now in [false, true] {
            let mut reflection = reflection_options(ReflectionMode::RoughSpecular);
            reflection.samples = samples;
            let request = request(Some(reflection));
            let options = request.raytrace.clone().unwrap();
            let cancel = CancelToken::default();
            let mut task = ImageTask::new(scene(), request, cancel.clone(), options).unwrap();
            let mut reached = false;
            for _ in 0..100_000 {
                let before = task.work.used();
                assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
                assert!(task.work.used() - before <= 4, "public quantum expanded");
                if task.pending.as_ref().is_some_and(|p| p.bounce > 0) {
                    reached = true;
                    break;
                }
            }
            assert!(reached, "actual VNDF rough continuation was not exercised");
            if cancel_now {
                cancel.cancel();
                assert_eq!(task.step(1).err().unwrap().code, ErrorCode::Cancelled);
            } else {
                task.work.limit = task.work.used();
                assert_eq!(task.step(1).err().unwrap().code, ErrorCode::LimitExceeded);
            }
        }
    }
}

fn scene() -> ProductScene {
    fn quad(name: &str, positions: Vec<Vec3>, normal: Vec3, color: [u8; 3], surface: Option<OpaqueSurface>) -> ProductMesh {
        ProductMesh {
            transmission: None,
            name: name.into(),
            mesh: Arc::new(Mesh {
                positions,
                normals: vec![normal; 4],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
                tri_face: vec![0, 0],
                ..Default::default()
            }),
            color,
            opacity: 1.0,
            surface,
            faces: Vec::new(),
        }
    }
    let mirror = quad(
        "Mirror",
        vec![Vec3::new(-20.0, -20.0, -20.0), Vec3::new(20.0, -20.0, -20.0), Vec3::new(20.0, 20.0, 20.0), Vec3::new(-20.0, 20.0, 20.0)],
        Vec3::new(0.0, -1.0, 1.0),
        [255; 3],
        Some(OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 1.0, 0.2, 1.5).unwrap()),
    );
    let object = quad(
        "Orange target",
        vec![Vec3::new(-8.0, -8.0, 10.0), Vec3::new(-8.0, 8.0, 10.0), Vec3::new(8.0, 8.0, 10.0), Vec3::new(8.0, -8.0, 10.0)],
        -Vec3::Z,
        [240, 80, 20],
        None,
    );
    let meshes = vec![mirror, object];
    let bounds = meshes.iter().fold(solidcraft_geom::Aabb3::EMPTY, |b, m| b.union(&m.mesh.bounds()));
    ProductScene { meshes, bounds }
}
fn request(reflections: Option<ReflectionOptions>) -> ImageRequest {
    let mut camera = crate::Camera { distance: 50.0, ..Default::default() };
    camera.set_view(crate::StandardView::Front);
    ImageRequest {
        studio_weights: None,
        environment: None,
        textures: None,
        integrator: job::Integrator::Raytrace,
        raytrace: Some(RaytraceOptions {
            reflections,
            shadows: false,
            lights: Some(vec![job::DirectionalLightOptions { direction: [0.0, 0.0, -1.0], color: [1.0; 3], intensity: 2.0 }]),
            ..Default::default()
        }),
        camera,
        width: 9,
        height: 9,
        background: [13, 27, 41],
        transparent_background: false,
    }
}
fn reflection_options(mode: ReflectionMode) -> ReflectionOptions {
    ReflectionOptions { bodies: vec![ReflectionBody { body: "Mirror".into(), mode }], max_bounces: 2, samples: 16 }
}
fn render(request: ImageRequest) -> Vec<u8> {
    let options = request.raytrace.clone().unwrap();
    let mut task = ImageTask::new(scene(), request, CancelToken::default(), options).unwrap();
    for _ in 0..200_000 {
        if let TaskPoll::Complete(png) = task.step(16384).unwrap() {
            return png;
        }
    }
    panic!("bounded reflection fixture did not complete");
}
#[test]
fn mirror_png_contains_other_body_and_rough_transport_is_reproducible() {
    let baseline = ::image::load_from_memory(&render(request(None))).unwrap().to_rgba8();
    let mirrored = ::image::load_from_memory(&render(request(Some(reflection_options(ReflectionMode::Mirror))))).unwrap().to_rgba8();
    let b = baseline.get_pixel(4, 4).0;
    let p = mirrored.get_pixel(4, 4).0;
    assert!(p[0] > p[1] && p[1] > p[2], "actual orange object must be reflected: {p:?}");
    assert_ne!(p, b, "reflection must change the scene image, not only metadata");
    let rough = request(Some(reflection_options(ReflectionMode::RoughSpecular)));
    assert_eq!(render(rough.clone()), render(rough));
}
#[test]
fn explicit_empty_overrides_preserve_baseline_bytes_and_validate_names_atomically() {
    assert_eq!(render(request(None)), render(request(Some(ReflectionOptions::default()))));
    let mut options = reflection_options(ReflectionMode::Mirror);
    options.bodies[0].body = "missing".into();
    let request = request(Some(options));
    assert_eq!(admit(&scene(), &request, request.raytrace.as_ref().unwrap()).err().unwrap().code, ErrorCode::InvalidRequest);
    let hostile = ReflectionOptions { bodies: Vec::with_capacity(65), ..ReflectionOptions::default() };
    assert!(hostile.validate().is_err());
}
#[test]
fn reflection_cancel_and_work_exhaustion_never_publish_partial_frame() {
    for mode in [ReflectionMode::Mirror, ReflectionMode::RoughSpecular] {
        for cancel_now in [false, true] {
            let request = request(Some(reflection_options(mode)));
            let options = request.raytrace.clone().unwrap();
            let cancel = CancelToken::default();
            let mut task = ImageTask::new(scene(), request, cancel.clone(), options).unwrap();
            let mut reached = false;
            for _ in 0..100_000 {
                assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
                if task.pending.as_ref().is_some_and(|p| p.bounce > 0) {
                    reached = true;
                    break;
                }
            }
            assert!(reached);
            if cancel_now {
                cancel.cancel();
                assert_eq!(task.step(1).err().unwrap().code, ErrorCode::Cancelled);
            } else {
                task.work.limit = task.work.used();
                assert_eq!(task.step(1).err().unwrap().code, ErrorCode::LimitExceeded);
            }
        }
    }
}
#[test]
fn prepared_hit_preserves_original_winding_identity_and_never_certifies_glass() {
    let source = scene();
    let primitive = &source.meshes[0];
    let indices = primitive.mesh.triangles[0];
    let vertices = indices.map(|i| primitive.mesh.positions[i as usize]);
    let triangle = Triangle::new(vertices).unwrap();
    let ray = Ray::new(Vec3::new(2.0, -50.0, 0.0), Vec3::Y, 0.0, 100.0).unwrap();
    let geometry = triangle.intersect(&ray).unwrap();
    let cancel = CancelToken::default();
    let mut builder = BvhBuilder::new(vec![triangle], limits(job::CpuBudget::Compact), &cancel).unwrap();
    while !builder.step(1, &cancel).unwrap().complete {}
    let bvh = builder.finish().unwrap();
    let primitives =
        [PreparedPrimitive { body_index: 0, face: Some(0), vertices, normals: [geometry.geometric_normal; 3], surface: primitive.surface.unwrap() }];
    let prepared = PreparedScene { bvh: &bvh, primitives: &primitives, source: &source };
    let shared = prepared.resolve_hit(ray, super::super::Hit { primitive_index: 0, geometry }).unwrap();
    assert_eq!(shared.body_name, "Mirror");
    assert_eq!(shared.geometric_normal, geometry.geometric_normal);
    assert_eq!(shared.front_face, geometry.front_face);
    assert_eq!(prepared.boundary_status(0), super::super::prepared::BoundaryStatus::Unknown);
}
#[test]
fn shared_spawn_excludes_signed_zero_self_hit_without_skipping_thin_blockers() {
    for x in [0.0, 1e12 - 100.0] {
        let vertices = [Vec3::new(x, 0.0, 0.0), Vec3::new(x + 20.0, 0.0, 0.0), Vec3::new(x, 20.0, 0.0)];
        let triangle = Triangle::new(vertices).unwrap();
        let ray = Ray::new(Vec3::new(x + 5.0, 5.0, 10.0), -Vec3::Z, 0.0, 20.0).unwrap();
        let geometry = triangle.intersect(&ray).unwrap();
        let primitive = PreparedPrimitive {
            body_index: 0,
            face: Some(0),
            vertices,
            normals: [Vec3::Z; 3],
            surface: OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 0.0, 0.7, 1.5).unwrap(),
        };
        let source = ProductScene {
            meshes: vec![ProductMesh {
                transmission: None,
                name: "Plane".into(),
                mesh: Arc::new(Mesh::default()),
                color: [255; 3],
                opacity: 1.0,
                surface: None,
                faces: Vec::new(),
            }],
            bounds: Default::default(),
        };
        let cancel = CancelToken::default();
        let mut builder = BvhBuilder::new(vec![triangle], limits(job::CpuBudget::Compact), &cancel).unwrap();
        while !builder.step(1, &cancel).unwrap().complete {}
        let bvh = builder.finish().unwrap();
        let primitives = [primitive];
        let prepared = PreparedScene { bvh: &bvh, primitives: &primitives, source: &source };
        let hit = prepared.resolve_hit(ray, super::super::Hit { primitive_index: 0, geometry }).unwrap();
        let secondary = hit.spawn(Vec3::Z).unwrap();
        assert!(secondary.interval()[0] > 0.0);
        assert!(triangle.intersect(&secondary).is_none());
        assert!(Triangle::new(vertices.map(|v| v + Vec3::Z * 0.005)).unwrap().intersect(&secondary).is_some());
    }
}
