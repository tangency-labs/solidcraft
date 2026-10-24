use super::*;

fn close(actual: Vec3, expected: Vec3) {
    assert!((actual - expected).len() < 1e-10, "{actual:?} != {expected:?}");
}

#[test]
fn physical_repeats_translation_negative_coordinates_and_manual_rotation() {
    // +90 degrees around local Z: grain +Y, across -X, depth +Z.
    let origin = Vec3::new(10.0, -20.0, 30.0);
    let mapping = VisualMapping::new(origin, [Vec3::Y, -Vec3::X, Vec3::Z], Vec3::new(100.0, 20.0, 5.0)).unwrap();
    close(mapping.coordinates(origin).unwrap(), Vec3::ZERO);
    close(mapping.coordinates(origin + Vec3::new(-10.0, 200.0, -15.0)).unwrap(), Vec3::new(2.0, 0.5, -3.0));
    // One repeat is a physical distance, independent of a body's dimensions.
    close(mapping.coordinates(origin + Vec3::Y * 100.0).unwrap(), Vec3::X);
    assert_eq!(mapping.origin_mm(), origin);
    assert_eq!(mapping.axes(), [Vec3::Y, -Vec3::X, Vec3::Z]);
    assert_eq!(mapping.repeat_mm(), Vec3::new(100.0, 20.0, 5.0));
}

#[test]
fn oblique_frame_has_analytic_coordinates_without_automatic_orthogonalization() {
    let a = std::f64::consts::FRAC_1_SQRT_2;
    let axes = [Vec3::new(a, a, 0.0), Vec3::new(-a, a, 0.0), Vec3::Z];
    let mapping = VisualMapping::new(Vec3::ZERO, axes, Vec3::new(2.0, 3.0, 4.0)).unwrap();
    close(mapping.coordinates(Vec3::new(2.0 * a, 2.0 * a, 0.0)).unwrap(), Vec3::X);
    close(mapping.coordinates(Vec3::new(-3.0 * a, 3.0 * a, 4.0)).unwrap(), Vec3::new(0.0, 1.0, 1.0));
    assert_eq!(mapping.axes(), axes);
}

#[test]
fn longest_local_axis_uses_xyz_ties_and_accepts_planar_bounds() {
    for (extent, expected) in [
        (Vec3::new(3.0, 3.0, 3.0), GrainAxis::X),
        (Vec3::new(2.0, 3.0, 3.0), GrainAxis::Y),
        (Vec3::new(1.0, 2.0, 3.0), GrainAxis::Z),
        (Vec3::new(0.0, 3.0, 0.0), GrainAxis::Y),
    ] {
        let min = Vec3::new(10.0, -4.0, 2.0);
        let axis = suggest_grain_axis(Aabb3 { min, max: min + extent }).unwrap();
        assert_eq!(axis, expected);
        assert!(VisualMapping::new(Vec3::ZERO, axis.axes(), Vec3::new(1.0, 1.0, 1.0)).is_ok());
    }
}

#[test]
fn accepted_frame_does_not_flip_after_a_different_resize_suggestion() {
    let first = suggest_grain_axis(Aabb3 { min: Vec3::ZERO, max: Vec3::new(100.0, 20.0, 10.0) }).unwrap();
    let mapping = VisualMapping::new(Vec3::ZERO, first.axes(), Vec3::new(10.0, 10.0, 10.0)).unwrap();
    let point = Vec3::new(25.0, 15.0, 5.0);
    let before = mapping.coordinates(point).unwrap();
    assert_eq!(suggest_grain_axis(Aabb3 { min: Vec3::ZERO, max: Vec3::new(100.0, 200.0, 10.0) }).unwrap(), GrainAxis::Y);
    assert_eq!(mapping.coordinates(point).unwrap(), before);
    close(before, Vec3::new(2.5, 1.5, 0.5));
}

#[test]
fn rigid_instances_share_definition_coordinates_after_explicit_inverse_placement() {
    let mapping = VisualMapping::new(Vec3::new(1.0, 2.0, 3.0), GrainAxis::Z.axes(), Vec3::new(5.0, 10.0, 20.0)).unwrap();
    let definition_point = Vec3::new(21.0, -8.0, 13.0);
    let expected = Vec3::new(2.0, 2.0, -0.5);
    close(mapping.coordinates(definition_point).unwrap(), expected);
    // Independent analytic instance transform: (x,y,z) -> (-y,x,z) + translation.
    let translation = Vec3::new(-300.0, 70.0, 12.0);
    let world = Vec3::new(-definition_point.y, definition_point.x, definition_point.z) + translation;
    let shifted = world - translation;
    let inverted = Vec3::new(shifted.y, -shifted.x, shifted.z);
    close(mapping.coordinates(inverted).unwrap(), expected);
    // Passing world points directly is deliberately not corrected by the utility.
    assert_ne!(mapping.coordinates(world).unwrap(), expected);
}

#[test]
fn malformed_frames_refuse_instead_of_repairing_or_reflecting() {
    for axes in [
        [Vec3::ZERO, Vec3::Y, Vec3::Z],
        [Vec3::X * 2.0, Vec3::Y, Vec3::Z],
        [Vec3::X, Vec3::X, Vec3::Z],
        [Vec3::X, Vec3::Y, -Vec3::Z],
        [Vec3::X, Vec3::new(0.01, 1.0, 0.0), Vec3::Z],
        [Vec3::new(f64::NAN, 0.0, 0.0), Vec3::Y, Vec3::Z],
        [Vec3::new(f64::MAX, 0.0, 0.0), Vec3::Y, Vec3::Z],
    ] {
        assert_eq!(VisualMapping::new(Vec3::ZERO, axes, Vec3::new(1.0, 1.0, 1.0)), Err(MappingError::Frame));
    }
}

#[test]
fn hostile_positions_sizes_bounds_and_excessive_coordinates_refuse() {
    let mapping = VisualMapping::new(Vec3::ZERO, GrainAxis::X.axes(), Vec3::new(1.0, 1.0, 1.0)).unwrap();
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX, MAX_LOCAL_MM * 2.0] {
        let point = Vec3::new(value, 0.0, 0.0);
        assert_eq!(VisualMapping::new(point, GrainAxis::X.axes(), Vec3::new(1.0, 1.0, 1.0)), Err(MappingError::Position));
        assert_eq!(mapping.coordinates(point), Err(MappingError::Position));
        assert_eq!(suggest_grain_axis(Aabb3 { min: Vec3::ZERO, max: point }), Err(MappingError::Bounds));
    }
    for value in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::MIN_POSITIVE, MIN_REPEAT_MM * 0.5, MAX_REPEAT_MM * 2.0] {
        assert_eq!(VisualMapping::new(Vec3::ZERO, GrainAxis::X.axes(), Vec3::new(1.0, value, 1.0)), Err(MappingError::RepeatSize));
    }
    for bounds in [Aabb3::EMPTY, Aabb3 { min: Vec3::X, max: Vec3::ZERO }, Aabb3 { min: Vec3::ZERO, max: Vec3::ZERO }] {
        assert_eq!(suggest_grain_axis(bounds), Err(MappingError::Bounds));
    }
    assert_eq!(mapping.coordinates(Vec3::new(MAX_REPEAT_COORDINATE + 1.0, 0.0, 0.0)), Err(MappingError::Coordinates));
}

#[test]
fn admitted_boundaries_remain_finite() {
    let mapping = VisualMapping::new(Vec3::new(MAX_LOCAL_MM, 0.0, 0.0), GrainAxis::X.axes(), Vec3::new(MAX_REPEAT_MM, MIN_REPEAT_MM, 1.0)).unwrap();
    close(mapping.coordinates(Vec3::new(-MAX_LOCAL_MM, 0.0, 0.0)).unwrap(), Vec3::new(-2000.0, 0.0, 0.0));
    let tiny = VisualMapping::new(Vec3::ZERO, GrainAxis::X.axes(), Vec3::new(MIN_REPEAT_MM, 1.0, 1.0)).unwrap();
    close(tiny.coordinates(Vec3::new(1000.0, 0.0, 0.0)).unwrap(), Vec3::new(MAX_REPEAT_COORDINATE, 0.0, 0.0));
}
