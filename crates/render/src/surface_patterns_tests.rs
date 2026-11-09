use super::*;

#[test]
fn runtime_effects_roundtrip_absent_empty_and_bounded_lists_without_null() {
    use crate::job::RaytraceOptions;
    for effects in [None, Some(Vec::new()), Some(vec![effect()])] {
        let options = RaytraceOptions { surface_effects: effects.clone(), ..Default::default() };
        let value = serde_json::to_value(&options).unwrap();
        match &effects {
            None => assert!(value.get("surface_effects").is_none()),
            Some(effects) if effects.is_empty() => assert_eq!(value["surface_effects"], serde_json::json!([])),
            Some(_) => {}
        }
        let decoded: RaytraceOptions = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(decoded, options);
        decoded.validate().unwrap();
        let mut invalid = value;
        invalid["surface_effects"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<RaytraceOptions>(invalid).is_err());
    }
}

fn effect() -> SurfaceEffect {
    SurfaceEffect { body: "Coupon".into(), frame: Default::default(), repeat_mm: [20.0; 3], roughness_amplitude: 0.0, bump_amplitude_mm: 0.0 }
}
fn surface() -> OpaqueSurface {
    OpaqueSurface::new(Vec3::new(0.4, 0.3, 0.2), 0.0, 0.4, 1.5).unwrap()
}

#[test]
fn zero_amplitude_is_exact_and_roughness_does_not_modify_normals() {
    let original = surface();
    let normal = Vec3::new(0.0, 0.0, 2.0);
    assert_eq!(
        effect().prepare().unwrap().apply(original, normal, Vec3::new(0.0, 0.0, 1.0), Vec3::ZERO).unwrap(),
        ShadingSurface { surface: original, shading_normal: normal }
    );
    let mut input = effect();
    input.roughness_amplitude = 0.2;
    let changed = input.prepare().unwrap().apply(original, normal, Vec3::new(0.0, 0.0, 1.0), Vec3::ZERO).unwrap();
    assert_eq!(changed.shading_normal, normal);
    assert!((changed.surface.roughness() - 0.6).abs() < 1.0e-12);
}

#[test]
fn analytic_height_gradient_matches_central_difference_in_rotated_translated_frame() {
    let mut input = effect();
    input.bump_amplitude_mm = 0.3;
    input.frame.origin_mm = [17.0, 3.0, 5.0];
    input.frame.axes = [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    input.repeat_mm = [12.0, 17.0, 23.0];
    let prepared = input.prepare().unwrap();
    let point = Vec3::new(19.1, 6.3, 8.7);
    let (_, gradient, _) = prepared.field(point).unwrap();
    let epsilon = 1.0e-5;
    for (axis, expected) in [(Vec3::new(1.0, 0.0, 0.0), gradient.x), (Vec3::new(0.0, 1.0, 0.0), gradient.y), (Vec3::new(0.0, 0.0, 1.0), gradient.z)] {
        let derivative = (prepared.field(point + axis * epsilon).unwrap().0 - prepared.field(point - axis * epsilon).unwrap().0) / (2.0 * epsilon);
        assert!((derivative - expected).abs() < 1.0e-8, "{derivative} != {expected}");
    }
}

#[test]
fn bump_changes_only_shading_and_remains_in_geometric_hemisphere() {
    let mut input = effect();
    input.bump_amplitude_mm = 0.5;
    let normal = Vec3::new(0.0, 0.0, 1.0);
    let changed = input.prepare().unwrap().apply(surface(), normal, normal, Vec3::ZERO).unwrap();
    assert_eq!(changed.surface, surface());
    assert_ne!(changed.shading_normal, normal);
    assert!(changed.shading_normal.dot(normal) > 0.0);
    assert!((changed.shading_normal.len() - 1.0).abs() < 1.0e-12);
}

#[test]
fn hostile_frames_periods_amplitudes_and_precision_are_refused() {
    for repeat in [0.0, -1.0, f64::NAN, f64::INFINITY, 0.0001] {
        let mut input = effect();
        input.repeat_mm = [repeat; 3];
        assert!(input.prepare().is_err());
    }
    let mut input = effect();
    input.frame.axes[2] = [0.0, 0.0, -1.0];
    assert!(input.prepare().is_err());
    let mut input = effect();
    input.bump_amplitude_mm = 1000.0;
    assert!(input.prepare().is_err());
    let mut input = effect();
    input.roughness_amplitude = f64::NAN;
    assert!(input.prepare().is_err());
    assert!(effect().prepare().unwrap().field(Vec3::new(f64::INFINITY, 0.0, 0.0)).is_err());
}

#[test]
fn surface_names_and_reserved_selector_capacities_are_bounded_before_admission() {
    use crate::job::RaytraceOptions;
    for name in [String::new(), "x".repeat(257), "body\nname".into()] {
        let mut value = serde_json::to_value(effect()).unwrap();
        value["body"] = name.into();
        assert!(serde_json::from_value::<SurfaceEffect>(value).is_err());
    }
    let mut body = String::with_capacity(512);
    body.push_str("Coupon");
    assert!(SurfaceEffect { body, ..effect() }.prepare().is_err());
    let mut values = Vec::with_capacity(64);
    values.push(effect());
    assert!(RaytraceOptions { surface_effects: Some(values), ..Default::default() }.validate().is_err());
}

#[test]
fn surface_dynamic_lease_sum_preserves_every_existing_options_capacity() {
    use crate::job::{DirectionalLightOptions, EmissionSides, RaytraceOptions, SurfaceEmission};
    let mut effects = Vec::with_capacity(32);
    let mut name = String::with_capacity(256);
    name.push_str("Coupon");
    effects.push(SurfaceEffect { body: name, ..effect() });
    let mut lights = Vec::with_capacity(8);
    lights.push(DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 1.0 });
    let mut emissions = Vec::with_capacity(32);
    emissions.push(SurfaceEmission { body_index: 0, body: "Coupon".into(), color: [1.0; 3], strength: 1.0, sides: EmissionSides::Front });
    let baseline = RaytraceOptions { lights: Some(lights), emissions: Some(emissions), ..Default::default() };
    let original = baseline.retained_bytes().unwrap();
    let options = RaytraceOptions { surface_effects: Some(effects), ..baseline };
    assert_eq!(options.retained_bytes().unwrap(), original + 32 * std::mem::size_of::<SurfaceEffect>() as u64 + 256);
}

#[test]
fn procedural_roughness_preserves_the_captured_clear_coat() {
    let coat = crate::clear_coat::ClearCoat::new(0.8, 0.2, 1.5).unwrap();
    let original = surface().with_clear_coat(coat);
    let mut input = effect();
    input.roughness_amplitude = 0.2;
    let changed = input.prepare().unwrap().apply(original, Vec3::Z, Vec3::Z, Vec3::ZERO).unwrap();
    assert_eq!(changed.surface.clear_coat(), Some(coat));
    assert!((changed.surface.roughness() - 0.6).abs() < 1.0e-12);
    let expected = OpaqueSurface::new(original.base_color(), original.metallic(), 0.6, original.dielectric_ior()).unwrap().with_clear_coat(coat);
    assert!((changed.surface.brdf(Vec3::Z, Vec3::Z, Vec3::Z).unwrap() - expected.brdf(Vec3::Z, Vec3::Z, Vec3::Z).unwrap()).len() < 1.0e-12);
}
