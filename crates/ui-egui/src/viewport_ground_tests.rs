use super::*;
use solidcraft_engine::{Session, geom::Aabb3, render::UpAxis};

#[test]
fn ground_ao_follows_the_existing_plane_in_both_up_axes_without_origin_clamping() {
    for up in [UpAxis::Z, UpAxis::Y] {
        let cam = Camera { up, ..Default::default() };
        for bottom in [-20.0, 20.0] {
            // A 3 x 4 x 12 box has diagonal 13: the original ground offset is .014 mm.
            let bounds = Aabb3 { min: Vec3::new(-1.5, -2.0, bottom), max: Vec3::new(1.5, 2.0, bottom + 12.0) };
            let ground = solidcraft_gpu::view::GroundAo::new(bounds, ground_height(bounds), cam.up, 2.0).unwrap();
            let bytes = ground.vertices(None);
            assert_eq!(bytes.len(), 6 * crate::gpu::TRI_SIZE);
            for vertex in bytes.as_chunks::<{ crate::gpu::TRI_SIZE }>().0 {
                let read = |i| f64::from(f32::from_le_bytes(vertex[i..i + 4].try_into().unwrap()));
                let point = cam.to_frame(Vec3::new(read(0), read(4), read(8)));
                assert!((point.z - (bottom - 0.014)).abs() < 1e-5);
                assert!(point.x >= -5.5 && point.x <= 5.5 && point.y >= -6.0 && point.y <= 6.0);
                assert_eq!(cam.to_frame(Vec3::new(read(12), read(16), read(20))), Vec3::Z);
            }
        }
    }
    for (bounds, radius) in [(Aabb3::EMPTY, 2.0), (Aabb3 { min: Vec3::ZERO, max: Vec3::X }, f32::NAN)] {
        assert!(solidcraft_gpu::view::GroundAo::new(bounds, ground_height(bounds), Camera::default().up, radius).is_none());
    }
}

#[test]
fn ground_ao_is_independent_of_the_ellipse_and_respects_visibility_and_view_style() {
    let mut session = Session::default();
    session.execute("solid.box", &json!({"length":3,"width":4,"height":12,"corner":[0,0,-20],"body_name":"Foot"})).unwrap();
    let mut app = SolveApp::new(session, crate::Services::default());
    app.ui.ground_shadow = false;
    app.cam.pitch = 0.6;
    let key = scene_key(&app);
    let ground = build_scene(&app).ground_ao;
    assert!(ground.is_some(), "cache receiver data independently of its frame visibility");
    assert_eq!(ao_radius(&app), 0.0);
    let revision = app.session.revision;
    app.run("view.ambient_occlusion", json!({"enabled":true})).unwrap();
    assert_eq!(scene_key(&app), key, "AO only changes frame visibility, not model geometry");
    assert_eq!(app.session.revision, revision, "AO is a preference, not a design edit");
    assert_eq!(build_scene(&app).ground_ao, ground);
    assert_eq!(ground.unwrap().vertices(None).len(), 6 * crate::gpu::TRI_SIZE);
    app.ui.ground_shadow = true;
    assert_eq!(build_scene(&app).ground_ao, ground);
    app.cam.yaw += 0.5;
    app.cam.pitch += 0.1;
    assert_eq!(build_scene(&app).ground_ao, ground, "camera motion does not rebuild ground geometry");
    for style in [0, 1, 3] {
        app.ui.visual_style = style;
        assert!(build_scene(&app).ground_ao.is_some());
    }
    app.ui.visual_style = 2;
    assert!(build_scene(&app).ground_ao.is_none());
    app.ui.visual_style = 0;
    app.cam.pitch = -0.5;
    assert_eq!(build_scene(&app).ground_ao, ground, "the renderer hides the receiver below the ground");
    app.cam.pitch = 0.5;
    app.session.visibility.hidden_bodies.push("Foot".into());
    assert!(build_scene(&app).ground_ao.is_none());
}

#[test]
fn ground_ao_covers_a_live_preview_without_moving_the_existing_plane() {
    let mut session = Session::default();
    session.execute("solid.box", &json!({"length":3,"width":4,"height":12,"corner":[0,0,-20]})).unwrap();
    let mut app = SolveApp::new(session, crate::Services::default());
    app.preferences.ambient_occlusion = true;
    app.cam.pitch = 0.5;
    let before = scene_key(&app);
    app.preview.active = true;
    app.preview.key += 1;
    app.preview.bounds = Aabb3 { min: Vec3::new(100.0, 0.0, -40.0), max: Vec3::new(103.0, 4.0, -28.0) };
    assert_eq!(scene_key(&app), before, "a preview update must not re-upload unchanged model batches");
    let scene = build_scene(&app);
    let bytes = scene.ground_ao.unwrap().vertices(Some(app.preview.bounds));
    let points: Vec<Vec3> = bytes
        .as_chunks::<{ crate::gpu::TRI_SIZE }>()
        .0
        .iter()
        .map(|v| {
            let read = |i| f64::from(f32::from_le_bytes(v[i..i + 4].try_into().unwrap()));
            Vec3::new(read(0), read(4), read(8))
        })
        .collect();
    assert!(points.iter().all(|p| (p.z + 20.014).abs() < 1e-5));
    assert!(points.iter().any(|p| p.x > 103.0));
    app.preview.rolled_back = true;
    assert!(build_scene(&app).ground_ao.unwrap().vertices(None).len() == 6 * crate::gpu::TRI_SIZE);
}

/// Exercise both native geometry paths on a real adapter. White background pixels at the
/// foot gain contact shading; a stale receiver must draw nothing when AO is disabled.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn ground_ao_draws_contact_pixels_without_an_ellipse_and_disappears_when_disabled() {
    let mut session = Session::default();
    session.execute("solid.box", &json!({"length":10,"width":10,"height":20,"corner":[-5,-5,0]})).unwrap();
    let mut app = SolveApp::new(session, crate::Services::default());
    app.preferences.ambient_occlusion = true;
    app.ui.ground_shadow = false;
    app.cam = Camera { target: Vec3::new(0.0, 0.0, 5.0), distance: 70.0, pitch: 0.6, yaw: 0.5, fov: 0.0, ..Default::default() };
    let projection = projection(&app, Rect::from_min_size(Pos2::ZERO, vec2(256.0, 256.0)));
    let eye = app.cam.eye().to_f32();
    for tess in [false, true] {
        app.viewport.gpu = Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess, oit: false });
        let scene = build_scene(&app);
        let mut without_ground = scene.clone();
        without_ground.ground_ao = None;
        let render = |scene, radius| {
            crate::gpu::render_ao_offscreen(
                scene,
                projection.vp.to_f32(),
                app.cam.back().to_f32(),
                [eye[0], eye[1], eye[2], 0.0],
                256,
                tess.then_some(solidcraft_gpu::SHADER),
                radius,
            )
        };
        let Some(baseline) = render(without_ground.clone(), 0.0) else { return };
        assert_eq!(render(scene.clone(), 0.0).unwrap(), baseline, "AO off skips even a cached receiver");
        let shaded = render(scene, ao_radius(&app)).unwrap();
        let contacts = baseline.as_chunks::<4>().0.iter().zip(shaded.as_chunks::<4>().0).filter(|(a, b)| a[..3] == [255; 3] && b[0] < 253).count();
        assert!(contacts > 5, "no ground contacts on tess={tess}: {contacts} pixels");
        assert_eq!(&shaded[..4], &[255; 4], "a distant background pixel stays unchanged");
    }
}
