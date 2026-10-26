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
            if task.stage() == stage {
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
    while task.stage() != Stage::Encode {
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

/// What an image request must be is checked on each way in, a registered executor's too.
#[test]
fn every_way_in_checks_what_an_image_request_must_be() {
    let scene = scene(vec![square("Square", [100; 3], 1.0, Vec3::ZERO)]);
    let budget = crate::job::CpuBudget::Compact;
    let limits = budget.limits();
    let refused = |r: &ImageRequest| {
        [
            admit_geometry(&scene, r, MAX_TRIANGLES).err().map(|e| e.code),
            admit_geometry_with_limits(&scene, r, MAX_TRIANGLES, limits.max_bodies, limits.max_geometry_bytes, budget.image_limits(false))
                .err()
                .map(|e| e.code),
        ]
    };
    let invalid = [Some(ErrorCode::InvalidRequest), Some(ErrorCode::InvalidRequest)];
    assert_eq!(refused(&request()), [None, None]);
    let mut camera = request();
    camera.camera.distance = f64::NAN;
    assert_eq!(refused(&camera), invalid, "a camera that isn't valid");
    for value in [-0.1, 1.1, f64::NAN] {
        let weights = ImageRequest { studio_weights: Some([value, 0.0, 0.0]), ..request() };
        assert_eq!(refused(&weights), invalid, "a weight of {value}");
    }
    let lit = ImageRequest { studio_weights: Some([1.0; 3]), ..request() };
    assert_eq!(refused(&lit), [None, None]);
    let traced = ImageRequest { integrator: crate::job::Integrator::Raytrace, ..lit };
    assert_eq!(refused(&traced), invalid, "studio weights on a ray traced image");
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
    while task.stage() != Stage::Clear {
        task.step(1).unwrap();
    }
    assert_eq!(task.raster.pixel_tests, 128_000_000);
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

/// Every tile of a picture, stepped to the end, stitched into the whole picture's rows.
fn picture(scene: ProductScene, request: PictureRequest) -> Vec<u8> {
    let (width, height) = request.projection.size();
    let mut task = PictureTask::new(scene, request, CancelToken::default()).unwrap();
    let mut rows = vec![0; width as usize * height as usize * 4];
    for _ in 0..1_000_000 {
        match task.step(4096).unwrap() {
            PicturePoll::Pending(p) => assert!(p.done <= p.total, "{p:?}"),
            PicturePoll::Tile(w, tile) => {
                assert_eq!(tile.len(), (w.x1 - w.x0) as usize * (w.y1 - w.y0) as usize * 4);
                for (y, row) in (w.y0..w.y1).zip(tile.chunks_exact((w.x1 - w.x0) as usize * 4)) {
                    let at = (y as usize * width as usize + w.x0 as usize) * 4;
                    rows[at..at + row.len()].copy_from_slice(row);
                }
            }
            PicturePoll::Complete => return rows,
        }
    }
    panic!("bounded picture did not finish");
}

/// `columns × rows` tiles of uneven sizes covering a `width × height` picture.
fn tiles(width: u32, height: u32, columns: u32, rows: u32) -> Vec<PixelWindow> {
    let cut = |n: u32, parts: u32, i: u32| if i == parts { n } else { (n * i / parts + i % 2).min(n) };
    let mut out = Vec::new();
    for j in 0..rows {
        for i in 0..columns {
            out.push(PixelWindow {
                x0: cut(width, columns, i),
                y0: cut(height, rows, j),
                x1: cut(width, columns, i + 1),
                y1: cut(height, rows, j + 1),
            });
        }
    }
    out
}

/// A front view: paper x along +X, paper y along +Z, seen from −Y; 1 mm pixels on a
/// 100 × 50 mm rectangle with its lower-left corner at the paper origin; the model's origin at
/// paper (50, 25).
fn front_view() -> (OrthoFrame, PixelGrid) {
    let frame = OrthoFrame { basis: Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z }, scale: 1.0, shift: Vec2::new(50.0, 25.0) };
    (frame, PixelGrid::new(0.0, 0.0, 100.0, 50.0, 25.4).unwrap())
}

fn opaque_and_glass() -> ProductScene {
    let mut glass = block("Glass", [40, 200, 120], Vec3::new(6.0, -14.0, 4.0), Vec3::new(9.0, 2.0, 9.0));
    glass.opacity = 0.4;
    scene(vec![
        block("Block", [180, 120, 60], Vec3::ZERO, Vec3::new(20.0, 12.0, 8.0)),
        block("Post", [60, 140, 200], Vec3::new(0.0, 0.0, 16.0), Vec3::new(4.0, 4.0, 8.0)),
        glass,
    ])
}

/// The tiles of a picture, put side by side, are the picture drawn as one tile, to the byte:
/// in the camera's view and an orthographic frame, at both samplings, with glass and with uneven
/// tiles that split triangles anywhere.
#[test]
fn tiles_side_by_side_are_the_picture_drawn_whole() {
    let mut iso = Camera { distance: 120.0, ..Default::default() };
    iso.set_view(StandardView::Iso);
    let (frame, grid) = front_view();
    for projection in [Projection::Camera { camera: Camera { fov: 0.6, ..iso }, width: 61, height: 37 }, Projection::Ortho { frame, grid }] {
        let (width, height) = projection.size();
        for supersample in [Supersample::One, Supersample::Two] {
            let request = |tiles: Vec<PixelWindow>| PictureRequest { projection, tiles, supersample, roles: Vec::new(), background: None };
            let whole = picture(opaque_and_glass(), request(tiles(width, height, 1, 1)));
            assert!(whole.as_chunks::<4>().0.iter().filter(|p| p[3] > 0).count() > 200, "{projection:?}: the scene is in the picture");
            for (columns, rows) in [(2, 2), (7, 3), (1, 9)] {
                let stitched = picture(opaque_and_glass(), request(tiles(width, height, columns, rows)));
                assert!(stitched == whole, "{projection:?} {supersample:?}: {columns} × {rows} tiles differ from the whole picture");
            }
        }
    }
}

/// The product image is the picture of its camera in one tile, 2 × 2 sampled, then encoded.
#[test]
fn the_product_image_is_its_cameras_picture() {
    let mut iso = Camera { distance: 120.0, ..Default::default() };
    iso.set_view(StandardView::Iso);
    for transparent in [true, false] {
        let request = ImageRequest { camera: iso, width: 48, height: 40, transparent_background: transparent, ..request() };
        let png = finish(ImageTask::new(opaque_and_glass(), request.clone(), CancelToken::default()).unwrap());
        let image = image::load_from_memory(&png).unwrap().to_rgba8().into_raw();
        let picture = picture(
            opaque_and_glass(),
            PictureRequest {
                projection: Projection::Camera { camera: iso, width: 48, height: 40 },
                tiles: tiles(48, 40, 3, 2),
                supersample: Supersample::Two,
                roles: Vec::new(),
                background: (!transparent).then_some(request.background),
            },
        );
        assert!(image == picture, "transparent {transparent}");
    }
}

/// An orthographic picture lands on its paper rectangle's pixel grid: a 20 × 10 mm face whose
/// edges lie on pixel edges covers exactly its 20 × 10 pixels sampled once at the centres (the
/// grid's rows run top-down from the rectangle's top); edges a little inside a pixel give it the
/// share of its 2 × 2 samples they cover, in the face's own colour (straight alpha).
#[test]
fn an_orthographic_picture_lands_on_its_pixel_grid() {
    let (frame, grid) = front_view();
    // A box seen from the front: paper x from `left` to 60, y from 20 to `top`.
    let face = |left: f64, top: f64| {
        let (x0, z1) = (left - 50.0, top - 25.0);
        let half = Vec3::new((10.0 - x0) / 2.0, 3.0, (z1 + 5.0) / 2.0);
        scene(vec![block("Face", [200, 100, 50], Vec3::new((x0 + 10.0) / 2.0, 0.0, (z1 - 5.0) / 2.0), half)])
    };
    let request = |supersample| PictureRequest {
        projection: Projection::Ortho { frame, grid },
        tiles: tiles(100, 50, 1, 1),
        supersample,
        roles: Vec::new(),
        background: None,
    };
    let px = |rows: &[u8], x: usize, y: usize| <[u8; 4]>::try_from(&rows[(y * 100 + x) * 4..(y * 100 + x) * 4 + 4]).unwrap();
    // Paper x 40..60, y 20..30: columns 40..60 and, counted from the top of the 50 px grid, rows 20..30.
    let rows = picture(face(40.0, 30.0), request(Supersample::One));
    for y in 0..50 {
        for x in 0..100 {
            let inside = (40..60).contains(&x) && (20..30).contains(&y);
            assert_eq!(px(&rows, x, y)[3], if inside { 255 } else { 0 }, "pixel ({x}, {y})");
        }
    }
    // The left edge at paper x 39.6 and the top at 30.4, sampled once: column 39's centre (39.5)
    // and row 19's (30.5) lie outside, so neither is drawn.
    let rows = picture(face(39.6, 30.4), request(Supersample::One));
    assert_eq!((px(&rows, 39, 25)[3], px(&rows, 50, 19)[3], px(&rows, 40, 20)[3]), (0, 0, 255));
    // Sampled 2 × 2: column 39 is half covered, row 19 half, their corner a quarter; each in the
    // face's colour.
    let rows = picture(face(39.6, 30.4), request(Supersample::Two));
    let inner = px(&rows, 50, 25);
    assert_eq!(inner[3], 255);
    for ((x, y), alpha) in [((39, 25), 128), ((50, 19), 128), ((39, 19), 64), ((38, 25), 0), ((50, 18), 0)] {
        let p = px(&rows, x, y);
        assert_eq!(p[3], alpha, "pixel ({x}, {y})");
        if alpha > 0 {
            assert_eq!(p[..3], inner[..3], "pixel ({x}, {y}) keeps the face's colour");
        }
    }
}

/// The orthographic map puts a point where its frame says it lies on paper, to 1e-9 mm, for a
/// turned frame, a scale and a shift: the guarantee that a picture and lines drawn from the same
/// frame register.
#[test]
fn the_orthographic_map_is_the_frames_paper_map() {
    let basis = Plane::new(Vec3::new(3.0, -2.0, 7.0), Vec3::new(0.8, 0.6, 0.0), Vec3::new(-0.3, 0.4, 0.9)).unwrap();
    let frame = OrthoFrame { basis, scale: 0.5, shift: Vec2::new(41.0, 17.5) };
    let grid = PixelGrid::new(12.5, 4.0, 60.0, 30.0, 300.0).unwrap();
    let scene = opaque_and_glass();
    let view = Projection::Ortho { frame, grid }.view(&scene, 2).unwrap();
    let (x0, y0, x1, y1) = grid.covers();
    for mesh in &scene.meshes {
        for p in &mesh.mesh.positions {
            let c = view.vp.apply(*p);
            assert_eq!(c[3], 1.0);
            let paper = Vec2::new(x0 + (c[0] + 1.0) / 2.0 * (x1 - x0), y0 + (c[1] + 1.0) / 2.0 * (y1 - y0));
            let want = frame.to_paper(*p);
            assert!((paper - want).len() < 1e-9, "{p:?}: {paper:?} against {want:?}");
            assert!((-1e-12..=1.0 + 1e-12).contains(&c[2]), "depth within the design's box: {c:?}");
        }
    }
    // Nearer the viewer (along the normal) is nearer in depth.
    let n = basis.normal();
    assert!(view.vp.apply(basis.origin + n)[2] < view.vp.apply(basis.origin)[2]);
}

/// A depth-only body hides what lies behind it and draws nothing itself, whichever is drawn
/// first; the painted body shows where it is not covered.
#[test]
fn a_depth_only_body_hides_what_is_behind_it() {
    let (frame, grid) = front_view();
    // Seen from −Y: the front block (y −10) covers paper x 40..50 of the back one (x 40..60).
    let front = block("Front", [20, 20, 220], Vec3::new(-5.0, -10.0, 0.0), Vec3::new(5.0, 2.0, 5.0));
    let back = block("Back", [220, 20, 20], Vec3::new(0.0, 10.0, 0.0), Vec3::new(10.0, 2.0, 5.0));
    for order in [[0, 1], [1, 0]] {
        let bodies = [front.clone(), back.clone()];
        let meshes: Vec<ProductMesh> = order.iter().map(|&i| bodies[i].clone()).collect();
        let roles: Vec<BodyRole> = order.iter().map(|&i| if i == 0 { BodyRole::DepthOnly } else { BodyRole::Painted }).collect();
        let request = PictureRequest {
            projection: Projection::Ortho { frame, grid },
            tiles: tiles(100, 50, 1, 1),
            supersample: Supersample::One,
            roles,
            background: None,
        };
        let rows = picture(scene(meshes), request);
        let px = |x: usize, y: usize| <[u8; 4]>::try_from(&rows[(y * 100 + x) * 4..(y * 100 + x) * 4 + 4]).unwrap();
        assert_eq!(px(45, 25), [0; 4], "order {order:?}: hidden behind the depth-only body, which draws nothing");
        let shown = px(55, 25);
        assert!(shown[3] == 255 && shown[0] > shown[2], "order {order:?}: the painted body beside it: {shown:?}");
    }
}

/// A picture's pixel work is admitted as a product image's is, by the raster's own projection:
/// under a cap that grows with its samples, and counting a depth-only body, which is never see-through.
#[test]
fn a_picture_is_admitted_under_its_own_pixel_work_cap() {
    let (frame, grid) = front_view();
    // Its faces drawn `times` over.
    let big = |times: usize| {
        let mut big = block("Big", [100; 3], Vec3::ZERO, Vec3::new(60.0, 2.0, 30.0));
        let mesh = Arc::make_mut(&mut big.mesh);
        mesh.triangles = mesh.triangles.repeat(times);
        mesh.tri_face = mesh.tri_face.repeat(times);
        scene(vec![big])
    };
    let small = PictureRequest {
        projection: Projection::Ortho { frame, grid },
        tiles: tiles(100, 50, 1, 1),
        supersample: Supersample::One,
        roles: Vec::new(),
        background: None,
    };
    let error = admit_picture(&big(8000), &small).unwrap_err();
    assert_eq!(error.code, ErrorCode::LimitExceeded);
    assert!(error.message.contains("128000000 pixel tests"), "{}", error.message);

    let sheet = OrthoFrame { shift: Vec2::new(594.0, 420.0), ..frame };
    let a0 = PictureRequest {
        projection: Projection::Ortho { frame: sheet, grid: PixelGrid::new(0.0, 0.0, 1189.0, 841.0, 1200.0).unwrap() },
        tiles: vec![PixelWindow { x0: 0, y0: 0, x1: 4096, y1: 64 }],
        ..small.clone()
    };
    let tests = admit_picture(&big(100), &a0).unwrap().projected_pixel_tests.unwrap();
    assert!(tests > MAX_PIXEL_TESTS, "{tests}");

    let mut unseen = block("Unseen", [100; 3], Vec3::ZERO, Vec3::new(10.0, 2.0, 5.0));
    unseen.opacity = 0.0;
    let count = |request: &PictureRequest| admit_picture(&scene(vec![unseen.clone()]), request).unwrap().projected_pixel_tests.unwrap();
    assert_eq!(count(&small), 0, "a painted body nothing can see draws nothing");
    assert!(count(&PictureRequest { roles: vec![BodyRole::DepthOnly], ..small.clone() }) > 0, "a depth-only one hides what is behind it");
}

/// A picture outside its bounds is refused before anything is allocated.
#[test]
fn hostile_pictures_are_refused() {
    let (frame, grid) = front_view();
    let good = PictureRequest {
        projection: Projection::Ortho { frame, grid },
        tiles: tiles(100, 50, 1, 1),
        supersample: Supersample::Two,
        roles: Vec::new(),
        background: None,
    };
    let code = |request: PictureRequest| PictureTask::new(opaque_and_glass(), request, CancelToken::default()).err().map(|e| e.code);
    assert_eq!(code(good.clone()), None);
    let bad = |f: &dyn Fn(&mut PictureRequest)| {
        let mut request = good.clone();
        f(&mut request);
        code(request)
    };
    let invalid = Some(ErrorCode::InvalidRequest);
    let one = vec![PixelWindow { x0: 0, y0: 0, x1: 1, y1: 1 }];
    assert_eq!(bad(&|r| r.tiles.clear()), invalid, "no tiles");
    assert_eq!(bad(&|r| r.tiles = vec![PixelWindow { x0: 0, y0: 0, x1: 101, y1: 10 }]), invalid, "a tile outside the picture");
    assert_eq!(bad(&|r| r.tiles = vec![PixelWindow { x0: 5, y0: 0, x1: 5, y1: 10 }]), invalid, "an empty tile");
    assert_eq!(bad(&|r| r.tiles = vec![one[0]; MAX_TILES + 1]), invalid, "too many tiles");
    assert_eq!(bad(&|r| r.roles = vec![BodyRole::Painted]), invalid, "a role for some bodies only");
    let mut skew = frame;
    skew.basis.y = Vec3::new(0.0, 0.3, 1.0);
    assert_eq!(bad(&|r| r.projection = Projection::Ortho { frame: skew, grid }), invalid, "a basis that isn't orthonormal");
    for scale in [0.0, -1.0, f64::NAN] {
        assert_eq!(bad(&|r| r.projection = Projection::Ortho { frame: OrthoFrame { scale, ..frame }, grid }), invalid, "scale {scale}");
    }
    let far = OrthoFrame { shift: Vec2::new(f64::INFINITY, 0.0), ..frame };
    assert_eq!(bad(&|r| r.projection = Projection::Ortho { frame: far, grid }), invalid, "an infinite shift");
    assert_eq!(bad(&|r| r.projection = Projection::Ortho { frame, grid: PixelGrid { pitch: f64::NAN, ..grid } }), invalid, "a NaN pixel");
    let limit = Some(ErrorCode::LimitExceeded);
    let wide = Projection::Camera { camera: Camera::default(), width: 65_537, height: 1 };
    assert_eq!(bad(&|r| (r.projection, r.tiles) = (wide, one.clone())), limit, "a side over 65,536 px");
    // A whole A0 sheet at 1200 dpi is accepted, drawn in strips; one tile that large is not.
    let a0 = Projection::Ortho { frame, grid: PixelGrid::new(0.0, 0.0, 1189.0, 841.0, 1200.0).unwrap() };
    assert_eq!(bad(&|r| (r.projection, r.tiles) = (a0, vec![PixelWindow { x0: 0, y0: 0, x1: 4096, y1: 64 }])), None);
    // As the product image refuses a size over the resolver's limits.
    assert_eq!(bad(&|r| (r.projection, r.tiles) = (a0, vec![PixelWindow { x0: 0, y0: 0, x1: 5000, y1: 5000 }])), invalid, "a tile over its limits");
}

/// A sphere of `bands` × `2 · bands` quads with smooth normals: `4 · bands²` triangles.
fn sphere(name: &str, colour: [u8; 3], centre: Vec3, radius: f64, bands: u32) -> ProductMesh {
    let mut mesh = Mesh::default();
    for i in 0..=bands {
        for j in 0..=2 * bands {
            let (polar, azimuth) = (std::f64::consts::PI * f64::from(i) / f64::from(bands), std::f64::consts::PI * f64::from(j) / f64::from(bands));
            let n = Vec3::new(polar.sin() * azimuth.cos(), polar.sin() * azimuth.sin(), polar.cos());
            mesh.positions.push(centre + n * radius);
            mesh.normals.push(n);
        }
    }
    let at = |i: u32, j: u32| i * (2 * bands + 1) + j;
    for i in 0..bands {
        for j in 0..2 * bands {
            mesh.triangles.extend([[at(i, j), at(i + 1, j), at(i + 1, j + 1)], [at(i, j), at(i + 1, j + 1), at(i, j + 1)]]);
            mesh.tri_face.extend([0, 0]);
        }
    }
    ProductMesh { name: name.into(), mesh: Arc::new(mesh), color: colour, opacity: 1.0, surface: None, transmission: None, faces: Vec::new() }
}

/// Timing, not a check (`cargo test --release -p solidcraft-render --lib picture_timing -- --ignored
/// --nocapture`): a 65,548-triangle front view drawn as the product image, as one picture at
/// 300 dpi, in 64-row strips at 300 dpi and in 64-row strips at 1200 dpi (4096 px wide at most).
#[test]
#[ignore]
fn picture_timing() {
    let scene = || {
        let mut glass = block("Glass", [40, 200, 120], Vec3::new(0.0, -60.0, 0.0), Vec3::new(30.0, 2.0, 20.0));
        glass.opacity = 0.4;
        let mut meshes = vec![
            sphere("Ball", [180, 120, 60], Vec3::ZERO, 40.0, 128),
            block("Base", [60, 140, 200], Vec3::new(0.0, 0.0, -45.0), Vec3::new(60.0, 30.0, 5.0)),
            glass,
        ];
        meshes.push(block("Post", [200, 200, 200], Vec3::new(50.0, 0.0, 0.0), Vec3::new(4.0, 4.0, 40.0)));
        scene(meshes)
    };
    let frame = OrthoFrame { basis: Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z }, scale: 1.0, shift: Vec2::new(70.0, 55.0) };
    let ortho = |dpi: f64| Projection::Ortho { frame, grid: PixelGrid::new(0.0, 0.0, 140.0, 110.0, dpi).unwrap() };
    let strips = |p: Projection| {
        let (width, height) = p.size();
        // A tile is at most 4096 px wide (the compact budget's side).
        let rows = (0..height.div_ceil(64)).map(|i| (i * 64, ((i + 1) * 64).min(height)));
        rows.flat_map(|(y0, y1)| (0..width.div_ceil(4096)).map(move |j| PixelWindow { x0: j * 4096, y0, x1: ((j + 1) * 4096).min(width), y1 }))
            .collect()
    };
    let time = |name: &str, run: &dyn Fn() -> usize| {
        let mut runs: Vec<f64> = (0..3)
            .map(|_| {
                let started = std::time::Instant::now();
                std::hint::black_box(run());
                started.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        runs.sort_by(f64::total_cmp);
        eprintln!("{name}: median {:.0} ms ({:.0} to {:.0})", runs[1], runs[0], runs[2]);
    };
    let mut front = Camera { distance: 300.0, ..Default::default() };
    front.set_view(StandardView::Front);
    let image = ImageRequest { camera: front, width: 1600, height: 1200, ..request() };
    time("product image 1600 × 1200, PNG", &|| finish(ImageTask::new(scene(), image.clone(), CancelToken::default()).unwrap()).len());
    for (name, projection, strip, supersample) in [
        ("300 dpi, one tile, 2 × 2", ortho(300.0), false, Supersample::Two),
        ("300 dpi, 64-row strips, 2 × 2", ortho(300.0), true, Supersample::Two),
        ("1200 dpi, 64-row strips, 1 × 1", ortho(1200.0), true, Supersample::One),
    ] {
        let (width, height) = projection.size();
        let tiles = if strip { strips(projection) } else { vec![PixelWindow { x0: 0, y0: 0, x1: width, y1: height }] };
        let request = PictureRequest { projection, tiles, supersample, roles: Vec::new(), background: None };
        time(&format!("{width} × {height} px, {name}"), &|| picture(scene(), request.clone()).len());
    }
}

/// The sRGB tables give exactly `geom::srgb`'s codes and values: at every code start and the
/// floating-point numbers around it, at the edges of the band where the formula decides, and
/// across a million values spread over 0 to 1 and outside it.
#[test]
fn srgb_tables_give_the_formulas_codes() {
    use solidcraft_geom::srgb;
    for code in 0..=255u8 {
        assert_eq!(decode(code).to_bits(), srgb::decode8(code).to_bits(), "code {code}");
    }
    let check = |v: f64| assert_eq!(encode(v), srgb::encode8(v), "{v:e}");
    for &start in CODE_STARTS.iter() {
        for steps in -64i64..=64 {
            check(f64::from_bits((start.to_bits() as i64 + steps) as u64));
        }
        for edge in [1.0 - 1e-9, 1.0 + 1e-9, 1.0 - 2e-9, 1.0 + 2e-9] {
            check(start * edge);
        }
    }
    for i in 0..=1_000_000 {
        check(f64::from(i) / 1_000_000.0);
        check(f64::from(i) * 1.3e-6 - 0.15);
    }
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0, f64::MIN_POSITIVE, 0.0031308, 0.0031308 + 1e-12] {
        check(v);
    }
    // The codes decode to round trip.
    for code in 0..=255u8 {
        assert_eq!(encode(decode(code)), code);
    }
}
