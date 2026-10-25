use super::*;
use crate::{Camera, ProductMesh, StandardView};
use solidcraft_geom::{Aabb3, Mesh};
use std::sync::Arc;

fn request() -> ImageRequest {
    let mut camera = Camera { distance: 50.0, ..Default::default() };
    camera.set_view(StandardView::Front);
    ImageRequest {
        studio_weights: None,
        environment: None,
        textures: None,
        integrator: Default::default(),
        raytrace: None,
        camera,
        width: 64,
        height: 64,
        background: [20, 40, 60],
        transparent_background: true,
    }
}

#[test]
fn render_lights_raster_mute_is_captured_in_request_and_darkens_only_the_product() {
    let part = scene(vec![square("part", [200, 180, 160], 1.0, Vec3::ZERO)]);
    let mut input = request();
    input.transparent_background = false;
    let normal = finish(ImageTask::new(part.clone(), input.clone(), super::CancelToken::default()).unwrap());
    input.studio_weights = Some([0.0; 3]);
    let dark = finish(ImageTask::new(part, input, super::CancelToken::default()).unwrap());
    assert_ne!(normal, dark);
    let image = image::load_from_memory(&dark).unwrap().to_rgba8();
    assert_eq!(image.get_pixel(32, 32).0, [0, 0, 0, 255]);
    assert_eq!(image.get_pixel(0, 0).0, [20, 40, 60, 255]);
}

fn square(name: &str, colour: [u8; 3], opacity: f64, offset: Vec3) -> ProductMesh {
    let mesh = Mesh {
        positions: vec![
            Vec3::new(-10.0, 0.0, -10.0) + offset,
            Vec3::new(10.0, 0.0, -10.0) + offset,
            Vec3::new(10.0, 0.0, 10.0) + offset,
            Vec3::new(-10.0, 0.0, 10.0) + offset,
        ],
        normals: vec![Vec3::new(0.0, -1.0, 0.0); 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        tri_face: vec![0, 1],
        ..Default::default()
    };
    ProductMesh { name: name.into(), mesh: Arc::new(mesh), color: colour, opacity, surface: None, transmission: None, faces: Vec::new() }
}

fn scene(meshes: Vec<ProductMesh>) -> ProductScene {
    let bounds = meshes.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.mesh.bounds()));
    ProductScene { meshes, bounds }
}

fn finish(mut task: ImageTask) -> Vec<u8> {
    for _ in 0..100_000 {
        match task.step(4096).unwrap() {
            TaskPoll::Complete(png) => return png,
            TaskPoll::Pending(p) => assert!(p.done <= p.total, "{p:?}"),
        }
    }
    panic!("bounded fixture did not finish");
}

#[test]
fn transparent_png_has_straight_srgb_colour_without_a_shared_edge_seam() {
    let scene = scene(vec![square("Glass", [220, 40, 20], 0.5, Vec3::ZERO)]);
    let png = finish(ImageTask::new(scene, request(), CancelToken::default()).unwrap());
    let image = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(image.get_pixel(0, 0).0, [0; 4]);
    for (x, y) in [(32, 32), (31, 31), (30, 32), (32, 30)] {
        let p = image.get_pixel(x, y).0;
        assert!((i16::from(p[3]) - 128).abs() <= 1, "a shared diagonal must not double-blend: {p:?}");
        assert!(p[0] > 180 && p[0] > p[1] + 60, "RGB is straight, not multiplied by 0.5: {p:?}");
    }
    let reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
    assert_eq!(reader.info().srgb, Some(png::SrgbRenderingIntent::Perceptual));
}

#[test]
fn glass_is_sorted_back_to_front_independently_of_body_order() {
    let request = request();
    let far = square("Far", [240, 20, 20], 0.5, request.camera.back() * -5.0);
    let near = square("Near", [20, 20, 240], 0.5, request.camera.back() * 5.0);
    let a = finish(ImageTask::new(scene(vec![far.clone(), near.clone()]), request.clone(), CancelToken::default()).unwrap());
    let b = finish(ImageTask::new(scene(vec![near, far]), request, CancelToken::default()).unwrap());
    assert_eq!(a, b);
    let image = image::load_from_memory(&a).unwrap().to_rgba8();
    let p = image.get_pixel(32, 32).0;
    assert!((i16::from(p[3]) - 192).abs() <= 1, "two half-transparent layers: {p:?}");
    assert!(p[2] > p[0] + 30, "the near blue layer contributes more: {p:?}");
}

#[test]
fn physical_transmission_keeps_the_exact_scalar_cpu_preview() {
    let original = square("Glass", [80, 180, 140], 0.25, Vec3::ZERO);
    let mut glass = original.clone();
    glass.surface = Some(crate::surface::OpaqueSurface::new(Vec3::new(0.2, 0.4, 0.3), 0.0, 0.1, 1.5).unwrap());
    glass.transmission = Some(crate::transmission::Transmission::new(Vec3::new(-0.7_f64.ln(), -0.9_f64.ln(), 0.0) / 10.0, None).unwrap());
    let expected = finish(ImageTask::new(scene(vec![original]), request(), CancelToken::default()).unwrap());
    let actual = finish(ImageTask::new(scene(vec![glass]), request(), CancelToken::default()).unwrap());
    assert_eq!(actual, expected);
}

#[test]
fn cpu_raytrace_refuses_physical_transmission_before_preparation() {
    let mut glass = square("Glass", [200; 3], 1.0, Vec3::ZERO);
    glass.transmission = Some(crate::transmission::Transmission::new(Vec3::ZERO, Some(1.0)).unwrap());
    let mut input = request();
    input.integrator = super::super::Integrator::Raytrace;
    input.transparent_background = false;
    let error = admit(&scene(vec![glass]), &input).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(error.message.contains("physical transmission"));
}

#[test]
fn cancellation_is_observed_during_preparation_sort_pixels_and_encoding() {
    for stage in [Stage::Looks, Stage::Prepare, Stage::Sort, Stage::Clear, Stage::Glass, Stage::Downsample, Stage::Encode] {
        let cancel = CancelToken::default();
        let mut task = ImageTask::new(scene(vec![square("Glass", [40, 180, 100], 0.4, Vec3::ZERO)]), request(), cancel.clone()).unwrap();
        let mut reached = false;
        for _ in 0..100_000 {
            if task.stage == stage {
                reached = true;
                break;
            }
            assert!(matches!(task.step(1).unwrap(), TaskPoll::Pending(_)));
        }
        assert!(reached);
        cancel.cancel();
        assert_eq!(task.step(4096).err().unwrap().code, ErrorCode::Cancelled);
    }
}

#[test]
fn encoding_yields_between_rows_and_completed_result_is_consumed_once() {
    let mut task = ImageTask::new(scene(Vec::new()), request(), CancelToken::default()).unwrap();
    while task.stage != Stage::Encode {
        assert!(matches!(task.step(64).unwrap(), TaskPoll::Pending(_)));
    }
    let before = task.encoder.as_ref().unwrap().row();
    assert!(matches!(task.step(usize::MAX).unwrap(), TaskPoll::Pending(_)));
    assert!(task.encoder.as_ref().unwrap().row() - before <= 4);
    loop {
        if let TaskPoll::Complete(bytes) = task.step(4096).unwrap() {
            assert_eq!(image::load_from_memory(&bytes).unwrap().width(), 64);
            break;
        }
    }
    assert_eq!(task.step(1).err().unwrap().code, ErrorCode::NotCompleted);
}

#[test]
fn invalid_sizes_indices_opacity_and_large_work_lists_fail_without_a_frame() {
    let body = square("Product", [100, 120, 140], 1.0, Vec3::ZERO);
    let mut bad_request = request();
    bad_request.width = u32::MAX;
    assert!(ImageTask::new(scene(vec![body.clone()]), bad_request, CancelToken::default()).is_err());
    for bad in [0, 1] {
        let mut body = body.clone();
        if bad == 0 {
            Arc::make_mut(&mut body.mesh).triangles[0][0] = u32::MAX;
        } else {
            body.faces.push(FaceAppearance { face: 0, color: [0; 3], opacity: f64::NAN, surface: None, transmission: None });
        }
        let error = ImageTask::new(scene(vec![body]), request(), CancelToken::default()).err().unwrap();
        assert_eq!(error.code, ErrorCode::InvalidScene);
    }
    let mut body = body;
    Arc::make_mut(&mut body.mesh).triangles.resize(MAX_TRIANGLES as usize + 1, [0, 1, 2]);
    assert_eq!(ImageTask::new(scene(vec![body]), request(), CancelToken::default()).err().unwrap().code, ErrorCode::LimitExceeded);
    assert_eq!(std::mem::size_of::<Draw>(), 16, "admission accounts for each of the three lists");
}

#[test]
fn registered_executor_geometry_admission_excludes_the_cpu_raster_estimate() {
    // A library-bounded source: about 100 MiB of face appearances on a small mesh at the
    // largest image. Its CPU raster estimate is above 384 MiB at a registered executor's cap.
    let count = (100 * 1024 * 1024 / std::mem::size_of::<FaceAppearance>()).min(1_000_000);
    let mut body = square("appearances", [200; 3], 1.0, Vec3::ZERO);
    body.faces = vec![FaceAppearance { transmission: None, face: 0, color: [200; 3], opacity: 1.0, surface: None }; count];
    let source = scene(vec![body]);
    let mut r = request();
    r.width = 2048;
    r.height = 2048;
    let cpu = admit_raster(admit_source(&source, &r, 1_048_576).unwrap()).unwrap_err();
    assert_eq!(cpu.code, ErrorCode::LimitExceeded);
    assert!(cpu.message.contains("384 MiB"), "{}", cpu.message);
    let geometry = admit_geometry(&source, &r, 1_048_576).unwrap();
    assert_eq!((geometry.pixel_scratch_bytes, geometry.preparation_bytes), (0, 0));
    assert_eq!(geometry.estimated_total_bytes, geometry.geometry_bytes);
    assert_eq!(geometry.face_appearances, count as u64);
    // The CPU host itself keeps rejecting the source.
    assert_eq!(admit(&source, &r).unwrap_err().code, ErrorCode::LimitExceeded);
}

#[test]
fn admission_rejects_projected_pixel_work_before_starting_a_job() {
    let mut body = square("Overlapping glass", [100, 120, 140], 0.55, Vec3::ZERO);
    let mesh = Arc::make_mut(&mut body.mesh);
    mesh.triangles = vec![[0, 1, 2]; 1024];
    mesh.tri_face = vec![0; 1024];
    let scene = scene(vec![body]);
    let request = ImageRequest { width: 512, height: 512, ..request() };
    let error = admit(&scene, &request).expect_err("preflight must reject the same pixel-work limit as the rasterizer");
    assert_eq!(error.code, ErrorCode::LimitExceeded);
    assert!(error.message.contains("pixel tests"));
    let request = ImageRequest { width: 64, height: 64, ..request };
    let admission = admit(&scene, &request).unwrap();
    assert!(admission.projected_pixel_tests.unwrap() > 0);
    assert!(admission.projected_pixel_tests.unwrap() <= MAX_PIXEL_TESTS);
    let png = finish(ImageTask::new(scene, request, CancelToken::default()).unwrap());
    assert_eq!(image::load_from_memory(&png).unwrap().width(), 64);
}

#[test]
fn pixel_work_admission_includes_the_limit_and_uses_clipped_sample_bounds() {
    let mut body = square("Full viewport", [100; 3], 1.0, Vec3::ZERO);
    let mesh = Arc::make_mut(&mut body.mesh);
    // Each triangle's bounds cover all 200 * 200 supersampled pixels.
    mesh.triangles = vec![[0, 1, 2]; 3200];
    mesh.tri_face = vec![0; 3200];
    let mut request = ImageRequest { width: 100, height: 100, ..request() };
    request.camera.distance = 1.0;
    request.camera.fov = 0.0;
    let mut scene = scene(vec![body]);
    assert_eq!(admit(&scene, &request).unwrap().projected_pixel_tests, Some(128_000_000));
    let mut task = ImageTask::new(scene.clone(), request.clone(), CancelToken::default()).unwrap();
    while task.stage != Stage::Clear {
        task.step(1).unwrap();
    }
    assert_eq!(task.pixel_tests, 128_000_000);
    Arc::make_mut(&mut scene.meshes[0].mesh).triangles.push([0, 1, 2]);
    assert_eq!(admit(&scene, &request).unwrap_err().code, ErrorCode::LimitExceeded);
    // Registered executors budget their own work, independently of the raster projection.
    let geometry = admit_geometry(&scene, &request, MAX_TRIANGLES).unwrap();
    assert_eq!(geometry.projected_pixel_tests, None);
    assert!(serde_json::to_value(geometry).unwrap().get("projected_pixel_tests").is_none());
}

#[test]
fn pixel_work_ignores_invisible_triangles_and_respects_the_last_face_override() {
    let mut body = square("Hidden", [100; 3], 0.0, Vec3::ZERO);
    let request = request();
    assert_eq!(admit(&scene(vec![body.clone()]), &request).unwrap().projected_pixel_tests, Some(0));
    body.faces.push(FaceAppearance { face: 0, color: [100; 3], opacity: 0.55, surface: None, transmission: None });
    assert!(admit(&scene(vec![body.clone()]), &request).unwrap().projected_pixel_tests.unwrap() > 0);
    body.faces.push(FaceAppearance { face: 0, color: [100; 3], opacity: 0.0, surface: None, transmission: None });
    assert_eq!(admit(&scene(vec![body]), &request).unwrap().projected_pixel_tests, Some(0));

    let offscreen = square("Offscreen", [100; 3], 1.0, Vec3::new(10_000.0, 0.0, 0.0));
    assert_eq!(admit(&scene(vec![offscreen]), &request).unwrap().projected_pixel_tests, Some(0));
    let mut degenerate = square("Degenerate", [100; 3], 1.0, Vec3::ZERO);
    Arc::make_mut(&mut degenerate.mesh).triangles = vec![[0, 0, 0]];
    assert_eq!(admit(&scene(vec![degenerate]), &request).unwrap().projected_pixel_tests, Some(0));
}

/// A box with a flat normal per face, so each of its six faces shades in one tone.
fn block(name: &str, colour: [u8; 3], centre: Vec3, half: Vec3) -> ProductMesh {
    let mut mesh = Mesh::default();
    for (n, u, v) in [(Vec3::X, Vec3::Y, Vec3::Z), (Vec3::Y, Vec3::Z, Vec3::X), (Vec3::Z, Vec3::X, Vec3::Y)] {
        for side in [1.0, -1.0] {
            let n = n * side;
            let (u, v) = if side > 0.0 { (u, v) } else { (v, u) };
            let base = mesh.positions.len() as u32;
            let face = mesh.triangles.len() as u32 / 2;
            for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let p = n + u * a + v * b;
                mesh.positions.push(centre + Vec3::new(p.x * half.x, p.y * half.y, p.z * half.z));
                mesh.normals.push(n);
            }
            mesh.triangles.extend([[base, base + 1, base + 2], [base, base + 2, base + 3]]);
            mesh.tri_face.extend([face, face]);
        }
    }
    ProductMesh { name: name.into(), mesh: Arc::new(mesh), color: colour, opacity: 1.0, surface: None, transmission: None, faces: Vec::new() }
}

/// The product render's output, pinned before W17b S1 reshaped it into prepare, tile and sink
/// stages (#5624): the decoded rows of each scene, a change in which is a change in what is drawn.
/// Not the PNG bytes: their compression depends on the deflate backend the build unifies (`flate2`
/// with zlib-rs when the UI crate is built alongside, miniz_oxide without), not on this code.
#[test]
fn product_render_output_is_pinned() {
    use sha2::{Digest, Sha256};
    let mut iso = Camera { distance: 120.0, ..Default::default() };
    iso.set_view(StandardView::Iso);
    let perspective = Camera { fov: 0.6, ..iso };
    let opaque = || {
        scene(vec![
            block("Block", [180, 120, 60], Vec3::ZERO, Vec3::new(20.0, 12.0, 8.0)),
            block("Post", [60, 140, 200], Vec3::new(0.0, 0.0, 16.0), Vec3::new(4.0, 4.0, 8.0)),
        ])
    };
    let mut glossy = opaque();
    glossy.meshes[0].surface = Some(crate::surface::OpaqueSurface::new(Vec3::new(0.6, 0.3, 0.1), 1.0, 0.3, 1.5).unwrap());
    let front = request().camera;
    let glass = scene(vec![square("Far", [240, 20, 20], 0.5, front.back() * -5.0), square("Near", [20, 20, 240], 0.5, front.back() * 5.0)]);
    // Larger than the view: triangles cross every edge of the picture.
    let crossing = scene(vec![block("Wall", [90, 160, 90], Vec3::ZERO, Vec3::new(80.0, 3.0, 80.0))]);
    let with = |camera: Camera, width: u32, height: u32, transparent: bool| ImageRequest {
        camera,
        width,
        height,
        transparent_background: transparent,
        ..request()
    };
    let cases = [
        ("orthographic, transparent", opaque(), with(iso, 96, 64, true)),
        ("perspective, solid background", opaque(), with(perspective, 96, 64, false)),
        ("PBR surface", glossy, with(iso, 80, 60, true)),
        ("two glass layers", glass, with(front, 64, 64, true)),
        ("crossing the picture's edges", crossing, with(perspective, 72, 40, false)),
    ];
    let mut rows = Vec::new();
    for (name, scene, request) in cases {
        let png = finish(ImageTask::new(scene, request, CancelToken::default()).unwrap());
        let rgba = image::load_from_memory(&png).unwrap().to_rgba8();
        rows.push((name, format!("{:x}", Sha256::digest(rgba.as_raw()))));
    }
    const ROWS: [&str; 5] = [
        "1677d1d51ff34f35993c9235933bb1e3770230ac6b419909b3c92d7aff7670aa",
        "41b29088a0e538394731bfef48428d64fff670d79108107b8624f5e109252242",
        "9c839ffab7945eb49cb5285401a46c6542ea1d8c0cbd21b5c040b9a536cd29a2",
        "7029581e13f0953175421a069fa459d18ceec61b2864870f0da70200e320ef55",
        "56fa250228f06a3fd4f300b3f31fcdc1d6d88b85549699cf9380601f48c2cdeb",
    ];
    for ((name, got), want) in rows.iter().zip(ROWS) {
        assert_eq!(got, want, "{name}: the rows changed");
    }
}
