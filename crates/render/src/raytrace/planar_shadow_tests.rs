//! Analytic shadow-ray depth regressions for exact and nearly coordinate-planar faces.
use super::*;
use prepared::SharedHit;

fn floor() -> [Vec3; 3] {
    [Vec3::new(-180.0, -180.0, 0.0), Vec3::new(180.0, -180.0, 0.0), Vec3::new(180.0, 180.0, 0.0)]
}
fn permute(v: Vec3, axis: usize) -> Vec3 {
    match axis {
        0 => Vec3::new(v.z, v.x, v.y),
        1 => Vec3::new(v.y, v.z, v.x),
        _ => v,
    }
}
fn spawned(triangle: Triangle, point: Vec3, scale: f64, direction: Vec3) -> Ray {
    let normal = triangle.geometric_normal().unwrap();
    let primary = Ray::new(point + normal * (10.0 * scale), -normal, 0.0, 20.0 * scale).unwrap();
    let hit = triangle.intersect(&primary).unwrap();
    let [a, b, c] = hit.barycentric;
    let [va, vb, vc] = triangle.vertices();
    SharedHit {
        body_index: 0,
        body_name: "Floor",
        primitive_index: 0,
        distance_mm: hit.distance,
        position: va * a + vb * b + vc * c,
        geometric_normal: normal,
        front_face: hit.front_face,
        shading_normal: normal,
        surface: crate::surface::OpaqueSurface::new(Vec3::new(0.5, 0.5, 0.5), 0.0, 0.7, 1.5).unwrap(),
        vertices: triangle.vertices(),
        barycentric: hit.barycentric,
    }
    .spawn(direction)
    .unwrap()
}
fn build(triangles: Vec<Triangle>, leaf_size: usize) -> Bvh {
    let cancel = CancelToken::default();
    let mut builder = BvhBuilder::new(triangles, BuildLimits { leaf_size, ..Default::default() }, &cancel).unwrap();
    while !builder.step(1, &cancel).unwrap().complete {}
    builder.finish().unwrap()
}

#[test]
fn zero_plane_outgoing_ray_never_gets_positive_depth_from_shear_cancellation() {
    let triangle = Triangle::new(floor()).unwrap();
    let ray = Ray::new(Vec3::new(73.17, -21.19, 0.0_f64.next_up()), Vec3::new(0.7, -1.0, 0.2), 0.0_f64.next_up(), MAX_DISTANCE_MM).unwrap();
    // Exact plane equation has negative depth: the ray starts above and leaves the plane.
    // The previous dominant-Y weighted-depth sum can instead yield +1.4210854715202004e-14.
    assert!(triangle.intersect(&ray).is_none());
}

#[test]
fn mixed_leaf_and_input_order_cannot_turn_the_departed_floor_into_an_occluder() {
    let floor = Triangle::new(floor()).unwrap();
    let far = Triangle::new([Vec3::new(-1200.0, -1200.0, 1.0), Vec3::new(-1000.0, -1200.0, 1.0), Vec3::new(-1000.0, -1000.0, 1.0)]).unwrap();
    let ray = Ray::new(Vec3::new(73.17, -21.19, 0.0_f64.next_up()), Vec3::new(0.7, -1.0, 0.2), 0.0_f64.next_up(), MAX_DISTANCE_MM).unwrap();
    for leaf_size in [1, 2, 8] {
        for triangles in [vec![floor, far], vec![far, floor]] {
            let bvh = build(triangles, leaf_size);
            let cancel = CancelToken::default();
            assert!(bvh.closest_hit(&ray, &mut WorkBudget::new(MAX_WORK).unwrap(), &cancel).unwrap().is_none());
            let mut cursor = bvh::TraceCursor::new(ray, true);
            let mut work = WorkBudget::new(MAX_WORK).unwrap();
            loop {
                if let Some(hit) = cursor.step(&bvh, &mut work, &cancel).unwrap() {
                    assert!(hit.is_none(), "any-hit traversal must agree for the mixed leaf");
                    break;
                }
            }
        }
    }
}

#[test]
fn shared_spawn_preserves_close_real_blockers_across_axes_scales_placement_and_winding() {
    for axis in 0..3 {
        for scale in [1e-6, 1.0, 1e6] {
            for translated in [false, true] {
                let translation = if translated { Vec3::new(1e8, -2e8, 11.0 * scale) } else { Vec3::ZERO };
                let transform = |v| permute(v * scale + translation, axis);
                let original = floor().map(transform);
                for reverse in [false, true] {
                    let [a, b, c] = original;
                    let triangle = Triangle::new(if reverse { [a, c, b] } else { [a, b, c] }).unwrap();
                    for direction in [Vec3::new(0.7, -1.0, 0.2), Vec3::new(-1.0, 0.3, 1e-6)] {
                        for sign in [-1.0, 1.0] {
                            let ray = spawned(triangle, transform(Vec3::new(73.17, -21.19, 0.0)), scale, permute(direction * sign, axis));
                            assert!(triangle.intersect(&ray).is_none());
                            // The blocker is placed at the actual ray's positive plane crossing,
                            // so grazing direction does not make a finite fixture miss its target.
                            let height = 0.005 * scale;
                            let component = component(ray.direction(), axis);
                            let distance = height / component.abs();
                            let center = ray.at(distance).unwrap();
                            let [a, b, c] = [Vec3::new(-180.0, -180.0, 0.0), Vec3::new(180.0, -180.0, 0.0), Vec3::new(0.0, 180.0, 0.0)]
                                .map(|v| permute(v * scale, axis) + center);
                            let blocker = Triangle::new([a, b, c]).unwrap();
                            assert!(blocker.intersect(&ray).is_some(), "a different close triangle remains visible");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn exact_coordinate_plane_depth_keeps_closed_intervals_signed_zero_and_oblique_path() {
    let plane = Triangle::new(floor().map(|v| v + Vec3::Z * 4.0)).unwrap();
    let origin = Vec3::new(73.17, -21.19, 0.0);
    assert_eq!(plane.intersect(&Ray::new(origin, Vec3::Z, 4.0, 4.0).unwrap()).unwrap().distance, 4.0);
    assert!(plane.intersect(&Ray::new(origin, Vec3::Z, 4.0_f64.next_up(), 8.0).unwrap()).is_none());
    assert!(plane.intersect(&Ray::new(origin, Vec3::Z, 0.0, 4.0_f64.next_down()).unwrap()).is_none());
    let point = origin + Vec3::Z * 4.0;
    for z in [0.0, -0.0] {
        assert!(plane.intersect(&Ray::new(point, Vec3::new(1.0, 0.3, z), 0.0, 8.0).unwrap()).is_none());
    }
    for direction in [Vec3::Z, -Vec3::Z] {
        assert_eq!(plane.intersect(&Ray::new(point, direction, 0.0, 8.0).unwrap()).unwrap().distance, 0.0);
        assert!(plane.intersect(&Ray::new(point, direction, 0.0_f64.next_up(), 8.0).unwrap()).is_none());
    }
    let oblique = Triangle::new([Vec3::new(-4.0, -4.0, -4.0), Vec3::new(4.0, -4.0, 4.0), Vec3::new(4.0, 4.0, 4.0)]).unwrap();
    assert_eq!(oblique.intersect(&Ray::new(Vec3::new(1.0, 1.0, 9.0), -Vec3::Z, 0.0, 10.0).unwrap()).unwrap().distance, 8.0);
}

#[test]
fn native_sample_plate_front_face_does_not_shadow_its_outgoing_rim_ray() {
    // These are the two front-side triangles of the built-in Sample Plate,
    // exported at standard quality. Tiny evaluated coordinate differences make
    // this a real slanted plane, not an exactly axis-aligned special case.
    let fixtures = [
        (
            [
                Vec3::new(5.9999999999993445, -1.7763568393612512e-15, 0.0),
                Vec3::new(74.00000000000065, 1.77635683943593e-15, 8.0),
                Vec3::new(5.999999999999999, -1.7763568394002505e-15, 8.0),
            ],
            Vec3::new(12.935950282461624, -1.413982641678367e-15, 5.4093378813347),
        ),
        (
            [
                Vec3::new(73.99999999999999, 1.7763568394002296e-15, 0.0),
                Vec3::new(74.00000000000065, 1.77635683943593e-15, 8.0),
                Vec3::new(5.9999999999993445, -1.7763568393612512e-15, 0.0),
            ],
            Vec3::new(42.44826731118756, 1.2791165835596725e-16, 3.8240400501773513),
        ),
    ];
    for (vertices, origin) in fixtures {
        let triangle = Triangle::new(vertices).unwrap();
        let ray = Ray::new(origin, Vec3::new(0.0, -0.6, 0.8), 0.0_f64.next_up(), MAX_DISTANCE_MM).unwrap();
        let normal = triangle.geometric_normal().unwrap();
        // Independent plane-side oracle: the origin is outside and the ray
        // travels farther outside. No positive plane intersection can exist.
        assert!((origin - vertices[0]).dot(normal) > 0.0);
        assert!(ray.direction().dot(normal) > 0.0);
        assert!(triangle.intersect(&ray).is_none(), "the departed Sample Plate face cannot occlude its own rim light");
        // A distinct surface only 5 micrometres along this same ray must remain
        // visible. The repair must not introduce a near-distance rejection floor.
        let center = ray.at(0.005).unwrap();
        let blocker =
            Triangle::new([center + Vec3::new(-1.0, 0.0, -1.0), center + Vec3::new(1.0, 0.0, -1.0), center + Vec3::new(0.0, 0.0, 1.0)]).unwrap();
        assert!(blocker.intersect(&ray).is_some());
    }
}
