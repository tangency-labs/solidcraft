use super::*;
use crate::{
    ProductMesh,
    job::{self, CancelToken, DielectricBoundary, ImageRequest, Integrator, RaytraceOptions, TaskPoll},
};
use solidcraft_geom::Mesh;
use std::sync::Arc;

pub(in crate::raytrace) fn box_body(thickness: f64) -> ProductMesh {
    let positions = (0..8)
        .map(|i| Vec3::new(if i & 1 == 0 { -10.0 } else { 10.0 }, if i & 2 == 0 { -10.0 } else { 10.0 }, if i & 4 == 0 { 0.0 } else { thickness }))
        .collect();
    let triangles =
        vec![[0, 2, 3], [0, 3, 1], [4, 5, 7], [4, 7, 6], [0, 4, 6], [0, 6, 2], [1, 3, 7], [1, 7, 5], [0, 1, 5], [0, 5, 4], [2, 6, 7], [2, 7, 3]];
    ProductMesh {
        transmission: None,
        name: "captured slab@occurrence-2".into(),
        mesh: Arc::new(Mesh { positions, triangles, ..Default::default() }),
        color: [255; 3],
        opacity: 1.0,
        surface: None,
        faces: Vec::new(),
    }
}
pub(in crate::raytrace) fn scene(body: ProductMesh) -> ProductScene {
    let bounds = body.mesh.bounds();
    ProductScene { meshes: vec![body], bounds }
}
pub(in crate::raytrace) fn input(boundary: DielectricBoundary) -> DielectricOptions {
    DielectricOptions {
        model: None,
        shadow_transmission: false,
        subsurface: None,
        body_index: 0,
        body_name: "captured slab@occurrence-2".into(),
        boundary,
        ior: 1.5,
        absorption_per_mm: [0.0; 3],
        thickness_mm: 0.0,
    }
}
fn environment() -> Option<crate::job::RaytraceEnvironmentOptions> {
    Some(crate::job::RaytraceEnvironmentOptions { color: [1.0; 3], intensity: 1.0 })
}
pub(in crate::raytrace) fn request(options: RaytraceOptions) -> ImageRequest {
    let mut camera = crate::Camera { distance: 50.0, ..Default::default() };
    camera.set_view(crate::StandardView::Top);
    ImageRequest {
        studio_weights: None,
        environment: None,
        textures: None,
        integrator: Integrator::Raytrace,
        raytrace: Some(options),
        camera,
        width: 1,
        height: 1,
        background: [255; 3],
        transparent_background: false,
    }
}
fn render(scene: ProductScene, options: RaytraceOptions) -> [u8; 4] {
    let request = request(options.clone());
    let mut task = super::super::image::ImageTask::new(scene, request, CancelToken::default(), options).unwrap();
    for _ in 0..100_000 {
        if let TaskPoll::Complete(png) = task.step(16384).unwrap() {
            let image = ::image::load_from_memory(&png).unwrap().to_rgba8();
            return image.get_pixel(0, 0).0;
        }
    }
    panic!("bounded dielectric fixture failed to finish");
}

#[test]
fn normal_interface_has_four_percent_reflection() {
    let hit = interface(-Vec3::Z, Vec3::Z, 1.0, 1.5).unwrap();
    assert!((hit.reflectance - 0.04).abs() < 1e-14);
    assert_eq!(hit.transmitted, Some(-Vec3::Z));
    assert_eq!(hit.reflected, Vec3::Z);
}

#[test]
fn thirty_degree_snell_and_parallel_exit_are_reversible() {
    let incident = Vec3::new(0.5, 0.0, -(3.0f64).sqrt() / 2.0);
    let enter = interface(incident, Vec3::Z, 1.0, 1.5).unwrap();
    let inside = enter.transmitted.unwrap();
    assert!((inside.x - 1.0 / 3.0).abs() < 1e-14);
    let exit = interface(inside, Vec3::Z, 1.5, 1.0).unwrap();
    assert!((exit.transmitted.unwrap() - incident).len() < 1e-14);
    assert!((exit.reflectance - enter.reflectance).abs() < 1e-14);
}

#[test]
fn internal_grazing_ray_is_totally_reflected() {
    let hit = interface(Vec3::new(0.8, 0.0, -0.6), Vec3::Z, 1.5, 1.0).unwrap();
    assert_eq!(hit.transmitted, None);
    assert_eq!(hit.reflectance, 1.0);
}

#[test]
fn beer_lambert_uses_actual_millimetre_path() {
    let a = attenuation([0.1, 0.2, 0.0], 2.0).unwrap();
    let b = attenuation([0.1, 0.2, 0.0], 8.0).unwrap();
    assert!((b.x - a.x.powi(4)).abs() < 1e-14);
    assert!((b.y - a.y.powi(4)).abs() < 1e-14);
    assert_eq!(b.z, 1.0);
}

#[test]
fn hostile_optics_are_refused() {
    for v in [f64::NAN, f64::INFINITY, -1.0] {
        assert!(interface(-Vec3::Z, Vec3::Z, v, 1.5).is_err());
        assert!(attenuation([v, 0.0, 0.0], 1.0).is_err());
        assert!(attenuation([0.0; 3], v).is_err());
    }
    assert!(interface(Vec3::Z, Vec3::Z, 1.0, 1.5).is_err());
    assert_eq!(interface(Vec3::X, Vec3::Z, 1.0, 1.0).unwrap().reflectance, 0.0);
}

#[test]
fn closed_box_certification_requires_complete_outward_boundary() {
    let original = box_body(2.0);
    assert!(matches!(certify(&scene(original.clone()), &input(DielectricBoundary::ClosedBox)), Ok(Certificate::ClosedBox(_))));
    let mut body = original.clone();
    Arc::make_mut(&mut body.mesh).triangles.pop();
    assert!(certify(&scene(body), &input(DielectricBoundary::ClosedBox)).is_err());
    let mut body = original.clone();
    Arc::make_mut(&mut body.mesh).triangles[0].swap(0, 1);
    assert!(certify(&scene(body), &input(DielectricBoundary::ClosedBox)).is_err());
    let mut body = original;
    let mesh = Arc::make_mut(&mut body.mesh);
    mesh.triangles[1] = mesh.triangles[0];
    assert!(certify(&scene(body), &input(DielectricBoundary::ClosedBox)).is_err());
}

/// A box's boundary in more triangles than two a face (its top fanned from a point in its
/// middle, as a mesher with interior points makes it) is the same box; the fan's middle off the
/// face's plane, or a face covered twice, is not.
#[test]
fn closed_box_certification_takes_any_tiling_of_the_faces() {
    let fanned = |middle: Vec3| {
        let mut body = box_body(2.0);
        let mesh = Arc::make_mut(&mut body.mesh);
        mesh.positions.push(middle);
        mesh.triangles.retain(|t| *t != [4, 5, 7] && *t != [4, 7, 6]);
        mesh.triangles.extend([[8, 4, 5], [8, 5, 7], [8, 7, 6], [8, 6, 4]]);
        body
    };
    let Ok(Certificate::ClosedBox(bounds)) = certify(&scene(fanned(Vec3::new(1.0, -3.0, 2.0))), &input(DielectricBoundary::ClosedBox)) else {
        panic!("a fanned top is still the box")
    };
    assert_eq!((bounds.min(), bounds.max()), (Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 2.0)));
    assert!(certify(&scene(fanned(Vec3::new(1.0, -3.0, 2.0 - 1e-9))), &input(DielectricBoundary::ClosedBox)).is_err());
    // The top once more, split along its other diagonal: covered twice.
    let mut body = box_body(2.0);
    let mesh = Arc::make_mut(&mut body.mesh);
    mesh.triangles.extend([[4, 5, 6], [5, 7, 6]]);
    assert!(certify(&scene(body), &input(DielectricBoundary::ClosedBox)).is_err());
}

#[test]
fn captured_occurrence_identity_and_unsupported_boundary_are_refused() {
    let scene = scene(box_body(2.0));
    let mut material = input(DielectricBoundary::ClosedBox);
    material.body_name = "captured slab@occurrence-1".into();
    assert!(certify(&scene, &material).is_err());
    material = input(DielectricBoundary::ThinSheet);
    assert!(certify(&scene, &material).is_err());
}

#[test]
fn actual_png_closed_volume_absorption_uses_two_and_eight_mm() {
    for thickness in [2.0, 8.0] {
        let mut material = input(DielectricBoundary::ClosedBox);
        material.ior = 1.0;
        material.absorption_per_mm = [0.1, 0.0, 0.0];
        let options = RaytraceOptions {
            dielectrics: vec![material],
            environment: environment(),
            lights: Some(Vec::new()),
            shadows: false,
            ..Default::default()
        };
        let actual = render(scene(box_body(thickness)), options);
        let linear = Vec3::new((-0.1 * thickness).exp(), 1.0, 1.0);
        let mapped = super::super::lighting::map_to_display(linear, 0.0).unwrap();
        let encode = |v: f64| ((if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }) * 255.0).round() as u8;
        assert_eq!(actual, [encode(mapped.x), encode(mapped.y), encode(mapped.z), 255]);
    }
}

#[test]
fn actual_png_clear_volume_includes_multiple_fresnel_branches() {
    let options = RaytraceOptions {
        dielectrics: vec![input(DielectricBoundary::ClosedBox)],
        environment: environment(),
        lights: Some(Vec::new()),
        shadows: false,
        ..Default::default()
    };
    let actual = render(scene(box_body(2.0)), options);
    // Constant unit environment, no absorption: all branches sum to almost one.
    // Eight boundaries drop only a very small unresolved Fresnel tail.
    assert_eq!(actual, [188, 188, 188, 255]);
}

#[test]
fn overlapping_volumes_are_refused_before_pixel_work() {
    let mut scene = scene(box_body(2.0));
    scene.meshes.push(box_body(2.0));
    let first = input(DielectricBoundary::ClosedBox);
    let mut second = first.clone();
    second.body_index = 1;
    let options = RaytraceOptions { dielectrics: vec![first, second], ..Default::default() };
    assert!(super::super::image::admit(&scene, &request(options.clone()), &options).is_err());
}

#[test]
fn cancelled_dielectric_job_cannot_publish_png() {
    let options = RaytraceOptions { dielectrics: vec![input(DielectricBoundary::ClosedBox)], ..Default::default() };
    let cancel = CancelToken::default();
    let mut task = super::super::image::ImageTask::new(scene(box_body(2.0)), request(options.clone()), cancel.clone(), options).unwrap();
    task.step(1).unwrap();
    cancel.cancel();
    assert_eq!(task.step(1).err().unwrap().code, crate::job::ErrorCode::Cancelled);
}

#[test]
fn runtime_dielectric_inputs_are_bounded_and_default_off() {
    assert!(RaytraceOptions::default().dielectrics.is_empty());
    let options = RaytraceOptions { dielectrics: vec![input(DielectricBoundary::ClosedBox); 9], ..Default::default() };
    assert!(options.validate().is_err());
    let mut material = input(DielectricBoundary::ClosedBox);
    material.thickness_mm = 1.0;
    let options = RaytraceOptions { dielectrics: vec![material], ..Default::default() };
    assert!(options.validate().is_err());
    let oversized = serde_json::json!({"dielectrics": vec![input(DielectricBoundary::ThinSheet); job::MAX_OPTICAL_TARGETS + 1]});
    assert!(serde_json::from_value::<RaytraceOptions>(oversized).is_err());
    assert!(serde_json::from_value::<RaytraceOptions>(serde_json::json!({"dielectrics":[{"body_index":0,"body_name":"x".repeat(1025),"boundary":"closed_box","ior":1.5,"absorption_per_mm":[0,0,0],"thickness_mm":0}]})).is_err());
}

#[test]
fn oblique_parallel_slab_offset_scales_with_actual_thickness() {
    let direction = Vec3::new(0.5, 0.0, -(3.0f64).sqrt() / 2.0);
    let inside = interface(direction, Vec3::Z, 1.0, 1.5).unwrap().transmitted.unwrap();
    let mut offsets = Vec::new();
    for thickness in [2.0, 8.0] {
        let bottom = Triangle::new([Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 0.0), Vec3::new(10.0, -10.0, 0.0)]).unwrap();
        let ray = super::super::Ray::new(Vec3::new(0.0, 0.0, thickness), inside, 0.0, 100.0).unwrap();
        let hit = bottom.intersect(&ray).unwrap();
        let exit = ray.at(hit.distance).unwrap();
        assert!((hit.distance - thickness / -inside.z).abs() < 1e-13);
        let undeviated_x = thickness * direction.x / -direction.z;
        offsets.push(undeviated_x - exit.x);
    }
    assert!((offsets[1] - 4.0 * offsets[0]).abs() < 1e-13);
}

#[test]
fn thin_sheet_is_certified_separately_and_duplicate_faces_refuse() {
    let mut body = box_body(2.0);
    Arc::make_mut(&mut body.mesh).triangles = vec![[4, 5, 7], [4, 7, 6]];
    let mut material = input(DielectricBoundary::ThinSheet);
    material.ior = 1.0;
    material.thickness_mm = 8.0;
    material.absorption_per_mm = [0.1, 0.0, 0.0];
    assert!(matches!(certify(&scene(body.clone()), &material), Ok(Certificate::ThinSheet)));
    let options =
        RaytraceOptions { dielectrics: vec![material], environment: environment(), lights: Some(Vec::new()), shadows: false, ..Default::default() };
    let actual = render(scene(body.clone()), options);
    let closed = RaytraceOptions {
        environment: environment(),
        dielectrics: vec![DielectricOptions {
            shadow_transmission: false,
            subsurface: None,
            ior: 1.0,
            absorption_per_mm: [0.1, 0.0, 0.0],
            ..input(DielectricBoundary::ClosedBox)
        }],
        lights: Some(Vec::new()),
        shadows: false,
        ..Default::default()
    };
    assert_eq!(actual, render(scene(box_body(8.0)), closed));
    let mesh = Arc::make_mut(&mut body.mesh);
    mesh.triangles[1] = mesh.triangles[0];
    assert!(certify(&scene(body), &input(DielectricBoundary::ThinSheet)).is_err());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_dielectric_job_uses_existing_host_and_retained_png_lease() {
    use crate::job::{CpuHost, JobId, Phase};
    let host = CpuHost::for_platform().unwrap();
    let options = RaytraceOptions {
        dielectrics: vec![input(DielectricBoundary::ClosedBox)],
        environment: environment(),
        lights: Some(Vec::new()),
        shadows: false,
        ..Default::default()
    };
    let scene = scene(box_body(2.0));
    let weak = Arc::downgrade(&scene.meshes[0].mesh);
    let id = JobId { session: 1655, serial: 1, generation: 1 };
    let handle = host.submit(id, 1, scene, request(options)).unwrap();
    assert_eq!(handle.wait_timeout(std::time::Duration::from_secs(60)).unwrap().progress.phase, Phase::Completed);
    let frame = handle.frame().unwrap().unwrap();
    assert!(::image::load_from_memory(frame.png()).is_ok());
    assert!(weak.upgrade().is_none());
    assert!(host.admitted_bytes() > 0);
    drop(handle);
    assert!(host.admitted_bytes() > 0);
    drop(frame);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while host.admitted_bytes() != 0 {
        assert!(std::time::Instant::now() < deadline, "worker did not release terminal request record");
        std::thread::yield_now();
    }
}

#[test]
fn secondary_environment_is_explicit_and_primary_background_remains_display_srgb() {
    let options =
        RaytraceOptions { dielectrics: vec![input(DielectricBoundary::ClosedBox)], lights: Some(Vec::new()), shadows: false, ..Default::default() };
    assert_eq!(render(scene(box_body(2.0)), options.clone()), [0, 0, 0, 255]);
    let mut request = request(options.clone());
    request.camera.target.x = 50.0;
    request.background = [20, 30, 40];
    request.raytrace.as_mut().unwrap().exposure_ev = 4.0;
    let options = request.raytrace.clone().unwrap();
    let mut task = super::super::image::ImageTask::new(scene(box_body(2.0)), request, CancelToken::default(), options).unwrap();
    for _ in 0..100_000 {
        if let TaskPoll::Complete(png) = task.step(16384).unwrap() {
            assert_eq!(::image::load_from_memory(&png).unwrap().to_rgba8().get_pixel(0, 0).0, [20, 30, 40, 255]);
            return;
        }
    }
    panic!("primary background fixture failed to finish");
}

#[test]
fn typed_runtime_roundtrip_preserves_default_empty_and_explicit_lists() {
    for options in [
        RaytraceOptions::default(),
        RaytraceOptions { lights: Some(Vec::new()), ..Default::default() },
        RaytraceOptions { dielectrics: vec![input(DielectricBoundary::ClosedBox)], environment: environment(), ..Default::default() },
    ] {
        let encoded = serde_json::to_value(&options).unwrap();
        let decoded: RaytraceOptions = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, options);
        assert!(decoded.validate().is_ok());
    }
}

#[test]
fn actual_oblique_png_selects_the_snell_shifted_colour_target() {
    fn target(name: &str, y0: f64, y1: f64, color: Vec3) -> ProductMesh {
        ProductMesh {
            transmission: None,
            name: name.into(),
            mesh: Arc::new(Mesh {
                positions: vec![Vec3::new(-20.0, y0, -1.0), Vec3::new(20.0, y0, -1.0), Vec3::new(20.0, y1, -1.0), Vec3::new(-20.0, y1, -1.0)],
                normals: vec![Vec3::Z; 4],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
                ..Default::default()
            }),
            color: [255; 3],
            opacity: 1.0,
            surface: Some(crate::surface::OpaqueSurface::new(color, 0.0, 1.0, 1.0).unwrap()),
            faces: Vec::new(),
        }
    }
    let mut scene = scene(box_body(8.0));
    scene.meshes.push(target("near blue stripe", -20.0, 4.0, Vec3::Z));
    scene.meshes.push(target("far red stripe", 4.0, 20.0, Vec3::X));
    for body in &scene.meshes {
        scene.bounds = scene.bounds.union(&body.mesh.bounds());
    }
    // Entry at (0,0,8), incidence30deg. Independent Snell values give
    // y=8/sqrt(8)+1/sqrt(3)=3.4058 for glass and 9/sqrt(3)=5.1962
    // for index-matched air at target z=-1, on opposite sides of y=4.
    let shifted_y = 8.0 / 8.0f64.sqrt() + 1.0 / 3.0f64.sqrt();
    let straight_y = 9.0 / 3.0f64.sqrt();
    assert!(shifted_y < 4.0 && straight_y > 4.0);
    let mut results = Vec::new();
    for ior in [1.5, 1.0] {
        let options = RaytraceOptions {
            dielectrics: vec![DielectricOptions { shadow_transmission: false, subsurface: None, ior, ..input(DielectricBoundary::ClosedBox) }],
            lights: Some(vec![crate::job::DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 4.0 }]),
            shadows: false,
            ..Default::default()
        };
        let mut request = request(options.clone());
        request.camera.yaw = 0.0;
        request.camera.pitch = std::f64::consts::PI / 3.0;
        request.camera.target = Vec3::new(0.0, 0.0, 8.0);
        let mut task = super::super::image::ImageTask::new(scene.clone(), request, CancelToken::default(), options).unwrap();
        let mut pixel = None;
        for _ in 0..100_000 {
            if let TaskPoll::Complete(png) = task.step(16384).unwrap() {
                pixel = Some(::image::load_from_memory(&png).unwrap().to_rgba8().get_pixel(0, 0).0);
                break;
            }
        }
        results.push(pixel.expect("oblique slab fixture must finish"));
    }
    let glass = results[0];
    let matched = results[1];
    assert!(u16::from(glass[2]) > u16::from(glass[0]) + 50, "glass ray missed the predicted blue stripe: {glass:?}");
    assert!(u16::from(matched[0]) > u16::from(matched[2]) + 50, "index-matched ray missed the predicted red stripe: {matched:?}");
}

#[test]
fn thin_sheet_brewster_sums_polarizations_before_averaging() {
    // tan(theta_B) = 3/2, so single-interface Rs = 25/169 and Rp = 0.
    // The lossless slab returns Rs/(1+Rs) after the unpolarized average.
    let direction = Vec3::new(3.0, 0.0, -2.0) / 13.0f64.sqrt();
    let material = input(DielectricBoundary::ThinSheet);
    let (_, polarized) = polarized_interface(direction, Vec3::Z, 1.0, 1.5).unwrap();
    assert!((polarized[0] - 25.0 / 169.0).abs() < 1e-14);
    assert!(polarized[1].abs() < 1e-28);
    let (optics, reflected, transmitted) = thin_sheet(direction, Vec3::Z, &material).unwrap();
    let expected_r = 25.0 / 194.0;
    let expected_t = 169.0 / 194.0;
    assert!((reflected - Vec3::new(expected_r, expected_r, expected_r)).len() < 1e-14);
    assert!((transmitted - Vec3::new(expected_t, expected_t, expected_t)).len() < 1e-14);
    assert!((optics.transmitted.unwrap() - direction).len() < 1e-14);
    // The old average-before-series result differs by this exact rational.
    assert!((50.0 / 363.0 - reflected.x - 625.0 / 70422.0).abs() < 1e-14);
}

#[test]
fn thin_sheet_normal_incidence_keeps_the_lossless_slab_limit() {
    let material = input(DielectricBoundary::ThinSheet);
    let (_, reflected, transmitted) = thin_sheet(-Vec3::Z, Vec3::Z, &material).unwrap();
    // Rs = Rp = 1/25: the two-interface lossless slab has R = 1/13.
    let r = 1.0 / 13.0;
    let t = 12.0 / 13.0;
    assert!((reflected - Vec3::new(r, r, r)).len() < 1e-14);
    assert!((transmitted - Vec3::new(t, t, t)).len() < 1e-14);
}

#[test]
fn thin_sheet_matched_indices_keep_beer_lambert_transmission() {
    let material = DielectricOptions {
        shadow_transmission: false,
        subsurface: None,
        ior: 1.0,
        thickness_mm: 2.0,
        absorption_per_mm: [2.0f64.ln() / 2.0, 4.0f64.ln() / 2.0, 0.0],
        ..input(DielectricBoundary::ThinSheet)
    };
    let (optics, reflected, transmitted) = thin_sheet(-Vec3::Z, Vec3::Z, &material).unwrap();
    assert_eq!(reflected, Vec3::ZERO);
    assert!((transmitted - Vec3::new(0.5, 0.25, 1.0)).len() < 1e-14);
    assert_eq!(optics.transmitted, Some(-Vec3::Z));
}

#[test]
fn thin_sheet_zero_thickness_index_matched_grazing_is_transparent() {
    // Equal indices remove the interface; zero thickness means exp(-sigma * 0) = 1.
    for absorption_per_mm in [[0.0; 3], [100.0, 0.25, 1.0]] {
        let material =
            DielectricOptions { shadow_transmission: false, subsurface: None, ior: 1.0, absorption_per_mm, ..input(DielectricBoundary::ThinSheet) };
        let (optics, reflected, transmitted) = thin_sheet(Vec3::X, Vec3::Z, &material).unwrap();
        assert_eq!(optics.reflectance, 0.0);
        assert_eq!(optics.transmitted, Some(Vec3::X));
        assert_eq!(reflected, Vec3::ZERO);
        assert_eq!(transmitted, Vec3::new(1.0, 1.0, 1.0));
    }
}

#[test]
fn thin_sheet_positive_thickness_index_matched_grazing_refuses_without_tir() {
    let material =
        DielectricOptions { shadow_transmission: false, subsurface: None, ior: 1.0, thickness_mm: 1.0, ..input(DielectricBoundary::ThinSheet) };
    let optics = interface(Vec3::X, Vec3::Z, 1.0, 1.0).unwrap();
    assert_eq!(optics.transmitted, Some(Vec3::X));
    assert_eq!(optics.reflectance, 0.0);
    assert!(matches!(thin_sheet(Vec3::X, Vec3::Z, &material), Err(TraceError::InvalidInput("grazing sheet thickness is unsupported"))));
}

#[test]
fn thin_sheet_zero_thickness_grazing_still_validates_absorption() {
    for sigma in [f64::NAN, f64::INFINITY, -1.0, 101.0] {
        let material = DielectricOptions {
            shadow_transmission: false,
            subsurface: None,
            ior: 1.0,
            absorption_per_mm: [sigma, 0.0, 0.0],
            ..input(DielectricBoundary::ThinSheet)
        };
        assert!(matches!(thin_sheet(Vec3::X, Vec3::Z, &material), Err(TraceError::InvalidInput("dielectric absorption or distance"))));
    }
}

#[test]
fn thin_sheet_absorbing_brewster_keeps_separate_internal_returns() {
    let direction = Vec3::new(3.0, 0.0, -2.0) / 13.0f64.sqrt();
    let material = DielectricOptions {
        shadow_transmission: false,
        subsurface: None,
        // cos(theta_t) = 3/sqrt(13), hence the one-pass distance is 1 mm.
        thickness_mm: 3.0 / 13.0f64.sqrt(),
        absorption_per_mm: [2.0f64.ln(), 2.0f64.ln(), 0.0],
        ..input(DielectricBoundary::ThinSheet)
    };
    let (_, reflected, transmitted) = thin_sheet(direction, Vec3::Z, &material).unwrap();
    // Independent rational series with Rs = 25/169, Rp = 0, a = 1/2.
    let r = 6625.0 / 75746.0;
    let t = 65521.0 / 151492.0;
    assert!((reflected - Vec3::new(r, r, 25.0 / 194.0)).len() < 1e-14);
    assert!((transmitted - Vec3::new(t, t, 169.0 / 194.0)).len() < 1e-14);
    assert!(reflected.x + transmitted.x < 1.0);
}

#[test]
fn actual_png_brewster_thin_sheet_matches_reflected_energy() {
    let mut sheet = box_body(2.0);
    Arc::make_mut(&mut sheet.mesh).triangles.retain(|t| *t == [4, 5, 7] || *t == [4, 7, 6]);
    let mut scene = scene(sheet);
    let absorber = ProductMesh {
        transmission: None,
        name: "unlit opaque transmission target".into(),
        mesh: Arc::new(Mesh {
            positions: vec![Vec3::new(-20.0, -20.0, -1.0), Vec3::new(20.0, -20.0, -1.0), Vec3::new(20.0, 20.0, -1.0), Vec3::new(-20.0, 20.0, -1.0)],
            normals: vec![Vec3::Z; 4],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            ..Default::default()
        }),
        color: [0; 3],
        opacity: 1.0,
        surface: None,
        faces: Vec::new(),
    };
    scene.bounds = scene.bounds.union(&absorber.mesh.bounds());
    scene.meshes.push(absorber);
    let options = RaytraceOptions {
        dielectrics: vec![input(DielectricBoundary::ThinSheet)],
        environment: environment(),
        lights: Some(Vec::new()),
        shadows: false,
        ..Default::default()
    };
    let mut request = request(options.clone());
    request.camera.yaw = 0.0;
    request.camera.pitch = (2.0f64 / 3.0).atan();
    request.camera.target = Vec3::new(0.0, 0.0, 2.0);
    let (_, direction) = request.camera.ray(0.5, 0.5, 1.0, 1.0);
    assert!((-direction.z - 2.0 / 13.0f64.sqrt()).abs() < 1e-14);
    let mut task = super::super::image::ImageTask::new(scene, request, CancelToken::default(), options).unwrap();
    let mut pixel = None;
    for _ in 0..100_000 {
        if let TaskPoll::Complete(png) = task.step(16384).unwrap() {
            pixel = Some(::image::load_from_memory(&png).unwrap().to_rgba8().get_pixel(0, 0).0);
            break;
        }
    }
    // Only the reflected branch sees the unit white environment. R = 25/194
    // maps through Reinhard to 25/219, then sRGB to 95. The old R = 50/363
    // maps to 98, so this also detects a consumer still using the old series.
    assert_eq!(pixel.expect("Brewster sheet fixture must finish"), [95, 95, 95, 255]);
}

fn general_options() -> RaytraceOptions {
    RaytraceOptions {
        path: Some(crate::job::CpuPathOptions { max_depth: 16 }),
        progressive: Some(crate::job::CpuProgressiveOptions { samples_per_pixel: 4, seed: 9 }),
        dielectrics: vec![input(DielectricBoundary::ClosedMesh)],
        ..Default::default()
    }
}
fn general_certificate(scene: &ProductScene, options: &RaytraceOptions) -> crate::job::Result<super::super::glass_mesh::Task> {
    let mut certificate = super::super::glass_mesh::Task::new(scene, options)?;
    let cancel = CancelToken::default();
    let mut work = super::super::WorkBudget::new(1_000_000).unwrap();
    for _ in 0..1_000_000 {
        if certificate.ready() {
            return Ok(certificate);
        }
        let before = work.used();
        certificate.step(scene, options, &mut work, &cancel)?;
        assert_eq!(work.used() - before, 1, "every real certificate transition has one cumulative debit");
    }
    panic!("bounded exact certificate did not finish")
}
#[test]
fn general_glass_exact_triangle_contacts_preserve_only_real_shared_simplexes() {
    let a = [Vec3::ZERO, Vec3::X, Vec3::Y];
    let cross = [Vec3::new(0.25, 0.25, -1.0), Vec3::new(0.25, 0.25, 1.0), Vec3::new(1.0, 1.0, 1.0)];
    let overlap = [Vec3::new(0.25, 0.25, 0.0), Vec3::new(0.75, 0.25, 0.0), Vec3::new(0.25, 0.75, 0.0)];
    let adjacent = [Vec3::X, Vec3::ZERO, -Vec3::Y];
    let folded = [Vec3::X, Vec3::ZERO, Vec3::Z];
    let vertex = [Vec3::ZERO, -Vec3::X, -Vec3::Y];
    let apart = [Vec3::new(0.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 1.0), Vec3::new(0.0, 1.0, 1.0)];
    for b in [cross, overlap, a] {
        assert!(super::super::glass_mesh::forbidden_contact(a, b, true));
    }
    for b in [adjacent, folded, vertex, apart] {
        assert!(!super::super::glass_mesh::forbidden_contact(a, b, true));
    }
    for b in [adjacent, folded, vertex] {
        assert!(super::super::glass_mesh::forbidden_contact(a, b, false), "another body's contact is never a shared simplex");
    }
    let almost = apart.map(|p| Vec3::new(p.x, p.y, f64::from_bits((1023 - 100) << 52)));
    assert!(!super::super::glass_mesh::forbidden_contact(a, almost, false), "no epsilon turns a real gap into contact");
}
#[test]
fn general_glass_certifies_rotated_closed_mesh_and_rejects_open_inward_and_crossed_shells() {
    let mut body = box_body(2.0);
    for p in &mut Arc::make_mut(&mut body.mesh).positions {
        let (x, y) = (p.x, p.y);
        p.x = (x - y) * std::f64::consts::FRAC_1_SQRT_2;
        p.y = (x + y) * std::f64::consts::FRAC_1_SQRT_2;
    }
    assert!(certify(&scene(body.clone()), &input(DielectricBoundary::ClosedBox)).is_err());
    general_certificate(&scene(body.clone()), &general_options()).unwrap();
    let mut open = body.clone();
    Arc::make_mut(&mut open.mesh).triangles.pop();
    let error = general_certificate(&scene(open), &general_options()).err().unwrap();
    assert!(error.message.contains("closed edges") || error.message.contains("two oppositely"), "{error}");
    // Retain the actual independent winding and contact oracles, not only a broad failure.
    let mut inward = body;
    for t in &mut Arc::make_mut(&mut inward.mesh).triangles {
        t.swap(1, 2);
    }
    assert!(general_certificate(&scene(inward), &general_options()).err().unwrap().message.contains("outward"));
    let mut crossed = box_body(2.0);
    Arc::make_mut(&mut crossed.mesh).positions[7] = Vec3::new(0.0, 0.0, -1.0);
    assert!(general_certificate(&scene(crossed), &general_options()).err().unwrap().message.contains("intersect"));
}
#[test]
fn general_glass_vertex_link_refuses_a_pinch_even_with_two_faces_per_edge_and_one_shell() {
    let mut body = box_body(2.0);
    let mesh = Arc::make_mut(&mut body.mesh);
    let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3]];
    mesh.triangles.clear();
    for q in quads {
        let center = q.map(|i| mesh.positions[i]).into_iter().fold(Vec3::ZERO, |s, p| s + p) * 0.25;
        let n = mesh.positions.len() as u32;
        mesh.positions.push(center);
        for i in 0..4 {
            mesh.triangles.push([n, q[i] as u32, q[(i + 1) % 4] as u32]);
        }
    }
    // These opposite corners have disjoint edge neighbors in this triangulation.
    // Coinciding them preserves edge2 and global connectivity but creates two links.
    mesh.positions[7] = mesh.positions[0];
    let error = general_certificate(&scene(body), &general_options()).err().unwrap();
    assert!(error.message.contains("vertex link"), "the vertex certificate must catch the pinch before generic contact checks: {error}");
}
#[test]
fn general_glass_nested_parents_are_certified_not_inferred_from_overlapping_bounds() {
    let outer = box_body(4.0);
    let mut inner = box_body(2.0);
    inner.name = "inner".into();
    for p in &mut Arc::make_mut(&mut inner.mesh).positions {
        p.x *= 0.5;
        p.y *= 0.5;
        p.z += 1.0;
    }
    let mut scene = scene(outer);
    scene.meshes.push(inner);
    let mut options = general_options();
    let mut input = options.dielectrics[0].clone();
    input.body_index = 1;
    input.body_name = "inner".into();
    options.dielectrics.push(input);
    let certificate = general_certificate(&scene, &options).unwrap();
    assert_eq!(certificate.region(0).unwrap().parent, None);
    assert_eq!(certificate.region(1).unwrap().parent, Some(0));
    Arc::make_mut(&mut scene.meshes[1].mesh).positions.iter_mut().for_each(|p| p.z -= 1.0);
    let error = general_certificate(&scene, &options).err().unwrap();
    assert!(error.message.contains("touch") || error.message.contains("intersect"), "{error}");
}
#[test]
fn general_glass_certificate_cancel_and_exhaustion_never_publish_a_partial_proof() {
    let scene = scene(box_body(2.0));
    let options = general_options();
    let cancel = CancelToken::default();
    let mut certificate = super::super::glass_mesh::Task::new(&scene, &options).unwrap();
    let mut work = super::super::WorkBudget::new(1).unwrap();
    certificate.step(&scene, &options, &mut work, &cancel).unwrap();
    assert!(!certificate.ready());
    assert_eq!(certificate.step(&scene, &options, &mut work, &cancel).unwrap_err().code, crate::job::ErrorCode::LimitExceeded);
    assert!(!certificate.ready());
    cancel.cancel();
    assert_eq!(certificate.step(&scene, &options, &mut work, &cancel).unwrap_err().code, crate::job::ErrorCode::Cancelled);
    assert!(!certificate.ready());
}

#[test]
fn general_glass_uses_smooth_optical_normals_only_with_correct_geometric_crossings() {
    let geometric = Vec3::Z;
    let direction = -Vec3::Z;
    let smooth = unit(Vec3::new(0.2, 0.0, 0.96)).unwrap();
    let (actual, normal) = shaded_interface(direction, geometric, smooth, 1.0, 1.5).unwrap();
    assert!((normal - smooth).len() < 1e-14);
    assert!((actual.transmitted.unwrap() - interface(direction, smooth, 1.0, 1.5).unwrap().transmitted.unwrap()).len() < 1e-14);
    assert_ne!(actual.transmitted, interface(direction, geometric, 1.0, 1.5).unwrap().transmitted);
    let folded = unit(Vec3::new(0.99, 0.0, 0.01)).unwrap();
    let (actual, normal) = shaded_interface(direction, geometric, folded, 1.0, 1.5).unwrap();
    assert_eq!(normal, geometric);
    assert_eq!(actual.transmitted, Some(direction));
    for ni in [1.0, 1.5] {
        for nt in [1.0, 1.5] {
            let (i, _) = shaded_interface(direction, geometric, folded, ni, nt).unwrap();
            assert!(i.reflected.dot(geometric) > 0.0);
            if let Some(t) = i.transmitted {
                assert!(t.dot(geometric) < 0.0);
            }
        }
    }
}

#[test]
fn general_glass_exact_dyadic_volume_has_translation_and_winding_invariant_sign_at_cancellation_scales() {
    for offset in [Vec3::ZERO, Vec3::new(1e12 - 1024.0, -1e12 + 1024.0, 1e12 - 1024.0)] {
        let mut body = box_body(0.125);
        let mesh = Arc::make_mut(&mut body.mesh);
        for p in &mut mesh.positions {
            *p += offset;
        }
        for reverse in [false, true] {
            let mut signed = super::super::glass_volume::SignedVolume::default();
            for face in &mesh.triangles {
                let mut p = face.map(|i| mesh.positions[i as usize]);
                if reverse {
                    p.swap(1, 2);
                }
                for term in 0..6 {
                    signed.add_term(p, term).unwrap();
                }
            }
            assert_eq!(signed.positive(), !reverse, "a 0.125mm closed slab remains outward at large world coordinates");
        }
    }
    let mut exact = super::super::glass_volume::SignedVolume::default();
    let p = [Vec3::new(0.25, 0.5, 0.75), Vec3::new(1.0, 0.5, 0.75), Vec3::new(0.25, 1.0, 0.75)];
    for t in 0..6 {
        exact.add_term(p, t).unwrap();
    }
    let reversed = [p[0], p[2], p[1]];
    for t in 0..6 {
        exact.add_term(reversed, t).unwrap();
    }
    assert!(!exact.positive(), "exact opposite oriented terms cancel to zero, never a rounded positive shell");
}

#[test]
fn general_glass_absence_preserves_the_original_runtime_optics_wire_and_explicit_legacy_gate() {
    let old = input(DielectricBoundary::ClosedBox);
    assert_eq!(
        serde_json::to_string(&old).unwrap(),
        r#"{"body_index":0,"body_name":"captured slab@occurrence-2","boundary":"closed_box","ior":1.5,"absorption_per_mm":[0.0,0.0,0.0],"thickness_mm":0.0}"#
    );
    let mut options = RaytraceOptions { dielectrics: vec![old.clone()], ..Default::default() };
    options.validate().unwrap();
    let before = options.clone();
    options.dielectrics[0].boundary = DielectricBoundary::ClosedMesh;
    assert!(options.validate().unwrap_err().message.contains("Indirect lighting"));
    options.dielectrics[0] = old;
    assert_eq!(options, before);
}

#[test]
fn general_glass_origin_winding_is_half_open_at_projected_edges_vertices_and_silhouettes() {
    let body = box_body(2.0);
    let crossing = |p: Vec3| {
        body.mesh
            .triangles
            .iter()
            .map(|indices| super::super::glass_mesh::origin_crossing(indices.map(|i| body.mesh.positions[i as usize]), p))
            .try_fold(0_i32, |sum, n| n.map(|n| sum + i32::from(n)))
    };
    for p in [Vec3::new(0.0, 0.0, 1.0), Vec3::new(9.0, 9.0, 1.0), Vec3::new(-9.0, -9.0, 0.5)] {
        assert_eq!(crossing(p).unwrap(), 1, "inside {p:?}");
    }
    // Positive-X rays deliberately pass through an entire triangle diagonal,
    // a shared vertex line or a silhouette. No jitter/retry is permitted.
    for p in
        [Vec3::new(-20.0, 0.0, 1.0), Vec3::new(20.0, 0.0, 1.0), Vec3::new(-20.0, 10.0, 2.0), Vec3::new(-20.0, 10.0, 1.0), Vec3::new(0.0, 0.0, 3.0)]
    {
        assert_eq!(crossing(p).unwrap(), 0, "outside half-open query {p:?}");
    }
    for p in [Vec3::new(10.0, 0.0, 1.0), Vec3::new(10.0, 10.0, 1.0), Vec3::new(10.0, 10.0, 2.0)] {
        assert!(crossing(p).unwrap_err().message.contains("exactly on"), "boundary {p:?} must never be guessed as air");
    }
}

#[test]
fn general_glass_origin_segment_contact_includes_complete_crossings_touches_and_coplanar_motion() {
    let triangle = [Vec3::new(0.0, -2.0, -2.0), Vec3::new(0.0, 2.0, -2.0), Vec3::new(0.0, 0.0, 2.0)];
    let contact = |a, b| super::super::glass_mesh::origin_segment_contact(triangle, a, b).unwrap();
    assert!(contact(Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)), "both endpoints can be outside while the near segment crosses a boundary");
    assert!(contact(Vec3::new(-1.0, 0.0, 0.0), Vec3::ZERO), "a boundary at the clipped endpoint is included");
    assert!(contact(Vec3::new(0.0, -4.0, 0.0), Vec3::new(0.0, 4.0, 0.0)), "coplanar motion can touch or cross a boundary");
    assert!(!contact(Vec3::new(-1.0, 4.0, 0.0), Vec3::new(1.0, 4.0, 0.0)));
    assert!(!contact(Vec3::new(-2.0, 0.0, 0.0), Vec3::new(-1.0, 0.0, 0.0)));
}

#[test]
fn cpu_subsurface_runtime_intent_is_bounded_closed_path_only_and_inactive_values_stay_authored() {
    use crate::transmission::subsurface::Subsurface;
    let mut options = RaytraceOptions {
        path: Some(job::CpuPathOptions { max_depth: 4 }),
        progressive: Some(job::CpuProgressiveOptions { samples_per_pixel: 4, seed: 9 }),
        dielectrics: vec![input(DielectricBoundary::ClosedMesh)],
        ..Default::default()
    };
    options.dielectrics[0].subsurface = Some(Subsurface { scattering_per_mm: 0.0, anisotropy: 0.95 });
    options.denoise = true;
    options.validate().unwrap();
    let json = serde_json::to_value(&options).unwrap();
    assert_eq!(json["dielectrics"][0]["subsurface"]["scattering_per_mm"], 0.0);
    assert_eq!(serde_json::from_value::<RaytraceOptions>(json.clone()).unwrap(), options);
    options.dielectrics[0].subsurface.as_mut().unwrap().scattering_per_mm = 0.5;
    assert!(options.validate().unwrap_err().message.contains("denois"));
    options.denoise = false;
    options.validate().unwrap();
    options.path = None;
    assert!(options.validate().unwrap_err().message.contains("subsurface"));
    options.path = Some(job::CpuPathOptions { max_depth: 4 });
    options.dielectrics[0].boundary = DielectricBoundary::ThinSheet;
    assert!(options.validate().unwrap_err().message.contains("closed"));
    options.dielectrics[0].boundary = DielectricBoundary::ClosedMesh;
    for rate in [-1.0, f64::NAN, f64::INFINITY, 100.001] {
        options.dielectrics[0].subsurface.as_mut().unwrap().scattering_per_mm = rate;
        assert!(options.validate().is_err());
    }
    assert_eq!(json["dielectrics"][0]["subsurface"]["anisotropy"], 0.95);
}

/// A separate barycentric tiler. Constant coordinates stay exactly on their
/// analytic face; each replaced triangle is three non-overlapping triangles.
fn tiled_certificate_box(count: usize) -> ProductMesh {
    assert!(count >= 12 && count.is_multiple_of(2));
    let mut body = box_body(2.0);
    let mesh = Arc::make_mut(&mut body.mesh);
    while mesh.triangles.len() < count {
        let split = ((count - mesh.triangles.len()) / 2).min(mesh.triangles.len());
        let old = std::mem::take(&mut mesh.triangles);
        for (index, t) in old.into_iter().enumerate() {
            if index >= split {
                mesh.triangles.push(t);
                continue;
            }
            let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
            let centre = |a: f64, b: f64, c: f64| if a == b && b == c { a } else { (a + b + c) / 3.0 };
            let point = Vec3::new(centre(a.x, b.x, c.x), centre(a.y, b.y, c.y), centre(a.z, b.z, c.z));
            let at = mesh.positions.len() as u32;
            mesh.positions.push(point);
            mesh.triangles.extend([[t[0], t[1], at], [t[1], t[2], at], [t[2], t[0], at]]);
        }
    }
    assert_eq!(mesh.triangles.len(), count);
    body
}

fn bounded_box_certificate(body: &ProductMesh) -> super::super::Result<Bounds> {
    let mut task = super::super::box_certificate::BoxTask::new(body.mesh.triangles.len())?;
    assert_eq!(task.allocated_bytes(), super::super::box_certificate::BoxTask::required_bytes(body.mesh.triangles.len())?);
    let cancel = CancelToken::default();
    let mut work = super::super::WorkBudget::new(super::super::MAX_WORK)?;
    while task.result().is_err() {
        let before = work.used();
        task.step(&body.mesh, &mut work, &cancel)?;
        assert_eq!(work.used() - before, 1, "every real box continuation has one debit");
    }
    let n = body.mesh.triangles.len() as u64;
    let logarithm = u64::from((n * 3).next_power_of_two().ilog2());
    assert!(work.used() <= 6 * n * logarithm + 16 * n + 32, "bounded merge/search work cannot become an all-pairs proof");
    task.result()
}

#[test]
fn bounded_box_certificate_matches_original_main_oracle_tilings_rejections_and_raw_signed_zero() {
    let compare = |body: ProductMesh| {
        let original = certify(&scene(body.clone()), &input(DielectricBoundary::ClosedBox));
        let actual = bounded_box_certificate(&body);
        match (original, actual) {
            (Ok(Certificate::ClosedBox(a)), Ok(b)) => {
                for (a, b) in [a.min(), a.max()].into_iter().zip([b.min(), b.max()]) {
                    assert_eq!([a.x, a.y, a.z].map(f64::to_bits), [b.x, b.y, b.z].map(f64::to_bits));
                }
            }
            (Err(a), Err(b)) => assert_eq!(a, b, "same precise refusal, not a generic unsupported fallback"),
            _ => panic!("bounded and original strict box decisions differ"),
        }
    };
    for count in [12, 14, 36, 108, 324] {
        for offset in [Vec3::ZERO, Vec3::new(1e12 - 64.0, 1e12 - 64.0, 1e12 - 64.0), Vec3::new(-1e12 + 64.0, -1e12 + 64.0, -1e12 + 64.0)] {
            let mut body = tiled_certificate_box(count);
            Arc::make_mut(&mut body.mesh).positions.iter_mut().for_each(|p| *p += offset);
            compare(body.clone());
            let mut open = body.clone();
            Arc::make_mut(&mut open.mesh).triangles.pop();
            compare(open);
            let mut duplicate = body.clone();
            let mesh = Arc::make_mut(&mut duplicate.mesh);
            mesh.triangles.push(mesh.triangles[0]);
            compare(duplicate);
            let mut inward = body.clone();
            Arc::make_mut(&mut inward.mesh).triangles[0].swap(1, 2);
            compare(inward);
            let mut warped = body;
            Arc::make_mut(&mut warped.mesh).positions[0].z += 0.125;
            compare(warped);
        }
    }
    let mut negative = tiled_certificate_box(36);
    for p in &mut Arc::make_mut(&mut negative.mesh).positions {
        if p.z == 0.0 {
            p.z = -0.0;
        }
    }
    compare(negative.clone());
    let mesh = Arc::make_mut(&mut negative.mesh);
    let original = mesh.triangles[0][0] as usize;
    let mut changed = mesh.positions[original];
    changed.z = 0.0;
    let at = mesh.positions.len() as u32;
    mesh.positions.push(changed);
    mesh.triangles[0][0] = at;
    assert!(certify(&scene(negative.clone()), &input(DielectricBoundary::ClosedBox)).is_err(), "raw signed-zero seams are not silently welded");
    compare(negative);
}

#[test]
fn bounded_box_certificate_keeps_the_full_65536_triangle_domain_and_exact_scratch() {
    let body = tiled_certificate_box(MAX_BOX_TRIANGLES);
    let expected = certify(&scene(body.clone()), &input(DielectricBoundary::ClosedBox)).unwrap();
    let actual = bounded_box_certificate(&body).unwrap();
    let Certificate::ClosedBox(expected) = expected else { panic!("box oracle") };
    assert_eq!((actual.min(), actual.max()), (expected.min(), expected.max()));
    assert_eq!((actual.min(), actual.max()), (Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 2.0)));
    assert!(super::super::box_certificate::BoxTask::required_bytes(MAX_BOX_TRIANGLES + 1).is_err());
}

#[test]
fn bounded_box_certificate_cancel_and_work_exhaustion_freeze_every_phase_without_a_partial_proof() {
    let body = tiled_certificate_box(36);
    for phase in ["load", "edges", "sort", "duplicates", "reverse", "area"] {
        let mut task = super::super::box_certificate::BoxTask::new(36).unwrap();
        let cancel = CancelToken::default();
        let mut work = super::super::WorkBudget::new(super::super::MAX_WORK).unwrap();
        while task.phase() != phase {
            task.step(&body.mesh, &mut work, &cancel).unwrap();
        }
        let before = (task.fingerprint(), task.allocated_bytes(), work.used());
        let mut exhausted = super::super::WorkBudget::new(1).unwrap();
        exhausted.charge(1, &cancel).unwrap();
        assert_eq!(task.step(&body.mesh, &mut exhausted, &cancel).unwrap_err(), super::super::TraceError::WorkLimit);
        assert_eq!((task.fingerprint(), task.allocated_bytes(), work.used()), before);
        assert!(task.result().is_err());
        cancel.cancel();
        assert_eq!(task.step(&body.mesh, &mut work, &cancel).unwrap_err(), super::super::TraceError::Cancelled);
        assert_eq!((task.fingerprint(), task.allocated_bytes(), work.used()), before);
        assert!(task.result().is_err());
    }
}

#[test]
fn general_path_declared_tiled_box_reuses_complete_certificate_without_extra_triangle_scratch() {
    let mut options = general_options();
    let body = tiled_certificate_box(108);
    let mut captured = scene(body);
    let mesh_bytes = super::super::glass_mesh::Task::required_bytes(&captured, &options).unwrap();
    let mesh_proof = general_certificate(&captured, &options).unwrap();
    options.dielectrics[0].boundary = DielectricBoundary::ClosedBox;
    assert_eq!(
        super::super::glass_mesh::Task::required_bytes(&captured, &options).unwrap(),
        mesh_bytes,
        "the existing exact manifold/contact certificate pays the same scratch for a declared box"
    );
    let box_proof = general_certificate(&captured, &options).unwrap();
    assert_eq!(box_proof.region(0).unwrap().bounds.min(), mesh_proof.region(0).unwrap().bounds.min());
    assert_eq!(box_proof.region(0).unwrap().bounds.max(), mesh_proof.region(0).unwrap().bounds.max());
    // Captured ordinal+name is the identity. Another body carrying that same
    // name is permitted by main's certifier and must not introduce a new policy.
    let mut unrelated = box_body(2.0);
    Arc::make_mut(&mut unrelated.mesh).positions.iter_mut().for_each(|p| p.x += 100.0);
    captured.meshes.push(unrelated);
    general_certificate(&captured, &options).unwrap();
    let mut malformed = captured.clone();
    Arc::make_mut(&mut malformed.meshes[0].mesh).positions[0].z += 0.125;
    let error = general_certificate(&malformed, &options).err().unwrap();
    assert_eq!(error.message, "box face is not axis aligned");
}

#[test]
fn bounded_box_certificate_matches_main_for_unused_vertices_without_welding_raw_zero() {
    let mut body = tiled_certificate_box(36);
    // The certificate is a proof of referenced boundary triangles. Main does
    // not extend the box, reject, or weld from unused source vertex records.
    Arc::make_mut(&mut body.mesh).positions.extend([
        Vec3::new(1e13, -1e13, 1e13),
        Vec3::new(f64::NAN, f64::INFINITY, f64::NEG_INFINITY),
        Vec3::new(-0.0, 0.0, -0.0),
    ]);
    let original = certify(&scene(body.clone()), &input(DielectricBoundary::ClosedBox)).unwrap();
    let Certificate::ClosedBox(original) = original else { panic!("box oracle") };
    let actual = bounded_box_certificate(&body).unwrap();
    assert_eq!((actual.min(), actual.max()), (original.min(), original.max()));
    assert_eq!((actual.min(), actual.max()), (Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 2.0)));
    let mut options = general_options();
    options.dielectrics[0].boundary = DielectricBoundary::ClosedBox;
    let captured = scene(body.clone());
    let complete = general_certificate(&captured, &options).unwrap();
    let padded_min = Vec3::new(actual.min().x.next_down(), actual.min().y.next_down(), actual.min().z.next_down());
    let padded_max = Vec3::new(actual.max().x.next_up(), actual.max().y.next_up(), actual.max().z.next_up());
    assert_eq!((complete.region(0).unwrap().bounds.min(), complete.region(0).unwrap().bounds.max()), (padded_min, padded_max));
    assert!(padded_min.x < actual.min().x && padded_min.y < actual.min().y && padded_min.z < actual.min().z);
    assert!(padded_max.x > actual.max().x && padded_max.y > actual.max().y && padded_max.z > actual.max().z);
    let mut closed_mesh = options.clone();
    closed_mesh.dielectrics[0].boundary = DielectricBoundary::ClosedMesh;
    let prior_mesh_policy = general_certificate(&captured, &closed_mesh).unwrap();
    assert_eq!(
        (complete.region(0).unwrap().bounds.min(), complete.region(0).unwrap().bounds.max()),
        (prior_mesh_policy.region(0).unwrap().bounds.min(), prior_mesh_policy.region(0).unwrap().bounds.max()),
        "declaring Box preserves the existing padded general-mesh region ABI"
    );
    // Making an invalid previously-unused record part of the boundary must
    // still produce the original strict refusal; it is never sanitised.
    let mesh = Arc::make_mut(&mut body.mesh);
    let invalid = (mesh.positions.len() - 2) as u32;
    mesh.triangles[0][0] = invalid;
    let expected = certify(&scene(body.clone()), &input(DielectricBoundary::ClosedBox)).unwrap_err();
    assert_eq!(bounded_box_certificate(&body).unwrap_err(), expected);
}

#[test]
fn general_box_axis_planes_undo_only_single_ulp_padding_and_keep_raw_zero_endpoints() {
    use super::super::Triangle;
    // This tests Triangle's bounding arithmetic over finite IEEE values, not a
    // broader glass predicate domain. Subnormal referenced coordinates remain
    // outside GeneralPath's separate, unchanged exact-certificate domain.
    let minimum = f64::from_bits(1);
    for value in [-1e12, -1e-300, -minimum, -0.0, 0.0, minimum, 1e-300, 1e12] {
        let points = [Vec3::new(-1.0, -1.0, value), Vec3::new(1.0, -1.0, value), Vec3::new(0.0, 1.0, value)];
        let triangle = Triangle::new(points).unwrap();
        let padded = triangle.bounds();
        assert_eq!(padded.min().z.to_bits(), value.next_down().to_bits());
        assert_eq!(padded.max().z.to_bits(), value.next_up().to_bits());
        let low = padded.min().z.next_up();
        let high = padded.max().z.next_down();
        assert_eq!(low, value);
        assert_eq!(high, value);
        if value != 0.0 {
            assert_eq!(low.to_bits(), value.to_bits());
            assert_eq!(high.to_bits(), value.to_bits());
        }
        assert!(triangle.vertices().iter().all(|point| point.z.to_bits() == value.to_bits()), "bounds never rewrite signed endpoint bits");
    }
    for negative in [false, true] {
        let mut body = tiled_certificate_box(36);
        for point in &mut Arc::make_mut(&mut body.mesh).positions {
            if point.z == 0.0 {
                point.z = if negative { -0.0 } else { 0.0 };
            }
        }
        let captured = scene(body);
        let raw = certify(&captured, &input(DielectricBoundary::ClosedBox)).unwrap();
        let mut options = general_options();
        options.dielectrics[0].boundary = DielectricBoundary::ClosedBox;
        let complete = general_certificate(&captured, &options).unwrap();
        let Certificate::ClosedBox(raw) = raw else { panic!("raw box oracle") };
        assert_eq!(complete.region(0).unwrap().bounds.min().z, raw.min().z.next_down());
        assert_eq!(complete.region(0).unwrap().bounds.max().z, raw.max().z.next_up());
        let bits = if negative { (-0.0_f64).to_bits() } else { 0.0_f64.to_bits() };
        assert!(captured.meshes[0].mesh.positions.iter().filter(|point| point.z == 0.0).all(|point| point.z.to_bits() == bits));
    }
}

// Independent unit-square cell tilings: duplicate vertex indices deliberately
// retain exact coordinates so certification cannot assume indexed welding.
fn tiled_path_sheet(cells: &[(i32, i32)]) -> ProductMesh {
    let mut body = box_body(1.0);
    let mut mesh = Mesh::default();
    for &(x, y) in cells {
        let (x, y) = (f64::from(x), f64::from(y));
        let begin = mesh.positions.len() as u32;
        mesh.positions.extend([Vec3::new(x, y, 0.0), Vec3::new(x + 1.0, y, 0.0), Vec3::new(x + 1.0, y + 1.0, 0.0), Vec3::new(x, y + 1.0, 0.0)]);
        mesh.triangles.extend([[begin, begin + 1, begin + 2], [begin, begin + 2, begin + 3]]);
    }
    body.mesh = Arc::new(mesh);
    body
}
fn path_sheet_options() -> RaytraceOptions {
    let mut options = general_options();
    options.dielectrics[0].boundary = DielectricBoundary::ThinSheet;
    options.dielectrics[0].thickness_mm = 0.7;
    options
}
#[test]
fn general_path_sheets_accept_exact_tilings_concavity_holes_and_either_global_winding() {
    let rectangle = [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)];
    let concave = [(0, 0), (1, 0), (0, 1)];
    let ring = [(0, 0), (1, 0), (2, 0), (0, 1), (2, 1), (0, 2), (1, 2), (2, 2)];
    let options = path_sheet_options();
    for cells in [&rectangle[..], &concave[..], &ring[..]] {
        for reverse in [false, true] {
            let mut body = tiled_path_sheet(cells);
            let mesh = Arc::make_mut(&mut body.mesh);
            for p in &mut mesh.positions {
                // Integer oblique plane: exact coplanarity does not depend on a
                // normalized floating normal or on alignment with a world axis.
                p.z = p.x + p.y;
                if p.x == 0.0 {
                    p.x = -0.0;
                }
            }
            if reverse {
                for triangle in &mut mesh.triangles {
                    triangle.swap(1, 2);
                }
            }
            let expected = mesh.triangles.len();
            let captured = scene(body.clone());
            let proof = general_certificate(&captured, &options).unwrap();
            let region = proof.region(0).unwrap();
            assert_eq!(region.primitive_end - region.primitive_begin, expected);
            assert_eq!(region.parent, None);
            assert!(certify(&captured, &input(DielectricBoundary::ThinSheet)).is_err(), "legacy optics retain their original two-triangle domain");
        }
    }
}
#[test]
fn general_path_sheets_refuse_folded_disconnected_overlapping_and_ambiguous_tilings() {
    let cells = [(0, 0), (1, 0), (0, 1), (1, 1)];
    let options = path_sheet_options();
    let mut cases = vec![tiled_path_sheet(&[]), tiled_path_sheet(&[(0, 0), (2, 0)]), tiled_path_sheet(&[(0, 0), (1, 1)])];
    let mut folded = tiled_path_sheet(&cells);
    Arc::make_mut(&mut folded.mesh).positions[0].z = 0.125;
    cases.push(folded);
    let mut reversed = tiled_path_sheet(&cells);
    Arc::make_mut(&mut reversed.mesh).triangles[2].swap(1, 2);
    cases.push(reversed);
    let mut duplicated = tiled_path_sheet(&cells);
    let duplicate = duplicated.mesh.triangles[0];
    Arc::make_mut(&mut duplicated.mesh).triangles.push(duplicate);
    cases.push(duplicated);
    let mut overlap = tiled_path_sheet(&cells);
    let mesh = Arc::make_mut(&mut overlap.mesh);
    let begin = mesh.positions.len() as u32;
    mesh.positions.extend([Vec3::new(0.25, 0.25, 0.0), Vec3::new(1.5, 0.25, 0.0), Vec3::new(0.25, 1.5, 0.0)]);
    mesh.triangles.push([begin, begin + 1, begin + 2]);
    cases.push(overlap);
    // One cell ends with a complete edge, the adjacent cell has a vertex in its
    // interior. Geometric coincidence is never used to repair that T-junction.
    let mut junction = tiled_path_sheet(&[(0, 0)]);
    let mesh = Arc::make_mut(&mut junction.mesh);
    mesh.positions.extend([Vec3::new(1.0, 0.5, 0.0), Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 1.0, 0.0)]);
    mesh.triangles.extend([[1, 5, 4], [4, 5, 6], [4, 6, 2]]);
    cases.push(junction);
    for body in cases {
        assert!(general_certificate(&scene(body), &options).is_err(), "a valid sheet needs exact topology and nonoverlapping planar triangles");
    }
}
#[test]
fn general_path_sheet_preparation_is_prepaid_charged_and_cancellable_without_a_partial_proof() {
    let cells = [(0, 0), (1, 0), (2, 0), (0, 1), (2, 1), (0, 2), (1, 2), (2, 2)];
    let captured = scene(tiled_path_sheet(&cells));
    let options = path_sheet_options();
    let charged = super::super::glass_mesh::Task::required_bytes(&captured, &options).unwrap();
    let empty = RaytraceOptions { dielectrics: Vec::new(), ..options.clone() };
    let fixed = super::super::glass_mesh::Task::required_bytes(&captured, &empty).unwrap();
    assert!(charged > fixed, "sheet faces and edge/link scratch are included in exact admission");
    for prefix in [1, 17, 61, 127, 509] {
        let cancel = CancelToken::default();
        let mut task = super::super::glass_mesh::Task::new(&captured, &options).unwrap();
        let mut work = super::super::WorkBudget::new(prefix).unwrap();
        for _ in 0..prefix {
            let before = work.used();
            task.step(&captured, &options, &mut work, &cancel).unwrap();
            assert_eq!(work.used(), before + 1);
            assert!(!task.ready());
        }
        assert_eq!(task.step(&captured, &options, &mut work, &cancel).unwrap_err().code, job::ErrorCode::LimitExceeded);
        assert!(!task.ready());
        assert!(task.region(0).is_err());
        cancel.cancel();
        assert_eq!(task.step(&captured, &options, &mut work, &cancel).unwrap_err().code, job::ErrorCode::Cancelled);
        assert!(!task.ready());
    }
}

fn opus_concave_boundary() -> Mesh {
    let polygon = [(0.0, 0.0), (3.0, 0.0), (3.0, 1.0), (1.0, 1.0), (1.0, 3.0), (0.0, 3.0)];
    let mut mesh = Mesh::default();
    for z in [0.0, 1.0] {
        for (x, y) in polygon {
            mesh.positions.push(Vec3::new(x, y, z));
            mesh.normals.push(Vec3::Z);
        }
    }
    for [a, b, c] in [[0, 1, 2], [0, 2, 3], [0, 3, 5], [3, 4, 5]] {
        mesh.triangles.extend([[a, c, b], [a + 6, b + 6, c + 6]]);
    }
    for i in 0..6 {
        let j = (i + 1) % 6;
        mesh.triangles.extend([[i, j, j + 6], [i, j + 6, i + 6]]);
    }
    mesh.tri_face.resize(mesh.triangles.len(), 0);
    mesh
}

#[test]
fn general_glass_nonconvex_overlapping_bounds_distinguish_air_and_true_containment() {
    for contained in [false, true] {
        let mut outer = box_body(1.0);
        outer.mesh = Arc::new(opus_concave_boundary());
        let mut inner = box_body(1.0);
        inner.name = "notch or arm".into();
        for p in &mut Arc::make_mut(&mut inner.mesh).positions {
            p.x = (p.x + 10.0) * 0.03 + if contained { 0.2 } else { 1.5 };
            p.y = (p.y + 10.0) * 0.03 + if contained { 0.2 } else { 1.5 };
            p.z = p.z * 0.6 + 0.2;
        }
        let mut source = scene(outer);
        source.meshes.push(inner);
        let mut options = general_options();
        options.dielectrics.push(DielectricOptions { body_index: 1, body_name: "notch or arm".into(), ..options.dielectrics[0].clone() });
        for reverse in [false, true] {
            if reverse {
                source.meshes.reverse();
                for input in &mut options.dielectrics {
                    input.body_index = 1 - input.body_index;
                }
                options.dielectrics.reverse();
            }
            let certificate = general_certificate(&source, &options).unwrap();
            let child = usize::from(!reverse);
            let parent = usize::from(reverse);
            assert_eq!(certificate.region(child).unwrap().parent, contained.then_some(parent));
            assert_eq!(certificate.region(parent).unwrap().parent, None);
        }
    }
}
