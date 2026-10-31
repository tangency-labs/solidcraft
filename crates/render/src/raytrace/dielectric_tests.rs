use super::*;
use crate::{
    ProductMesh,
    job::{CancelToken, DielectricBoundary, ImageRequest, Integrator, RaytraceOptions, TaskPoll},
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
    let oversized = serde_json::json!({"dielectrics": vec![input(DielectricBoundary::ThinSheet);9]});
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
        dielectrics: vec![DielectricOptions { ior: 1.0, absorption_per_mm: [0.1, 0.0, 0.0], ..input(DielectricBoundary::ClosedBox) }],
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
            dielectrics: vec![DielectricOptions { ior, ..input(DielectricBoundary::ClosedBox) }],
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
        let material = DielectricOptions { ior: 1.0, absorption_per_mm, ..input(DielectricBoundary::ThinSheet) };
        let (optics, reflected, transmitted) = thin_sheet(Vec3::X, Vec3::Z, &material).unwrap();
        assert_eq!(optics.reflectance, 0.0);
        assert_eq!(optics.transmitted, Some(Vec3::X));
        assert_eq!(reflected, Vec3::ZERO);
        assert_eq!(transmitted, Vec3::new(1.0, 1.0, 1.0));
    }
}

#[test]
fn thin_sheet_positive_thickness_index_matched_grazing_refuses_without_tir() {
    let material = DielectricOptions { ior: 1.0, thickness_mm: 1.0, ..input(DielectricBoundary::ThinSheet) };
    let optics = interface(Vec3::X, Vec3::Z, 1.0, 1.0).unwrap();
    assert_eq!(optics.transmitted, Some(Vec3::X));
    assert_eq!(optics.reflectance, 0.0);
    assert!(matches!(thin_sheet(Vec3::X, Vec3::Z, &material), Err(TraceError::InvalidInput("grazing sheet thickness is unsupported"))));
}

#[test]
fn thin_sheet_zero_thickness_grazing_still_validates_absorption() {
    for sigma in [f64::NAN, f64::INFINITY, -1.0, 101.0] {
        let material = DielectricOptions { ior: 1.0, absorption_per_mm: [sigma, 0.0, 0.0], ..input(DielectricBoundary::ThinSheet) };
        assert!(matches!(thin_sheet(Vec3::X, Vec3::Z, &material), Err(TraceError::InvalidInput("dielectric absorption or distance"))));
    }
}

#[test]
fn thin_sheet_absorbing_brewster_keeps_separate_internal_returns() {
    let direction = Vec3::new(3.0, 0.0, -2.0) / 13.0f64.sqrt();
    let material = DielectricOptions {
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
