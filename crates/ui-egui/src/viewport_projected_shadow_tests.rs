use super::*;

#[test]
fn projected_ground_shadow_preserves_gaps_and_display_opacity() {
    let mut session = solidcraft_engine::Session::default();
    // A connected rectangular frame still has one ellipse after main's per-group shadow
    // change. Its opening distinguishes that ellipse from the projected silhouette.
    for (x, y, length, width) in [(-20, -20, 4, 40), (16, -20, 4, 40), (-16, -20, 32, 4), (-16, 16, 32, 4)] {
        session.execute("solid.box", &json!({"length":length,"width":width,"height":8,"corner":[x,y,0],"operation":"new"})).unwrap();
    }
    let mut app = SolveApp::new(session, crate::Services::default());
    app.ui.show_grid = false;
    app.set_origin_shown(false);
    app.cam =
        Camera { target: Vec3::new(0.0, 0.0, 4.0), distance: 90.0, pitch: std::f64::consts::FRAC_PI_2, yaw: 0.0, fov: 0.0, ..Default::default() };
    let projection = projection(&app, Rect::from_min_size(Pos2::ZERO, vec2(256.0, 256.0)));
    let eye = app.cam.eye().to_f32();
    for tess in [false, true] {
        app.viewport.gpu = Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess, oit: false });
        let mut render = |mode| {
            app.run("view.ground_shadow", json!({"mode":mode})).unwrap();
            crate::gpu::render_offscreen_layers(
                build_scene(&app),
                build_sketch_scene(&app, &projection),
                projection.vp.to_f32(),
                app.cam.back().to_f32(),
                [eye[0], eye[1], eye[2], 0.0],
                256,
                tess.then_some(solidcraft_gpu::SHADER),
                [0.0; 8],
                [0.0; 36],
            )
        };
        let Some(ellipse) = render("ellipse") else { return };
        let projected = render("projected").unwrap();
        let disabled = render("off").unwrap();
        if !tess && let Ok(dir) = std::env::var("SOLIDCRAFT_GROUND_SHADOW_EVIDENCE") {
            std::fs::create_dir_all(&dir).unwrap();
            for (name, pixels) in [("ellipse", &ellipse), ("projected", &projected), ("off", &disabled)] {
                image::save_buffer(std::path::Path::new(&dir).join(format!("{name}.png")), pixels, 256, 256, image::ColorType::Rgba8).unwrap();
            }
        }
        let centre = (128 * 256 + 128) * 4;
        assert!(ellipse[centre] < 240, "the ellipse covers the centre gap");
        assert_eq!(&projected[centre..centre + 4], &[255; 4], "the projection leaves the gap clear");
        let changed = projected.as_chunks::<4>().0.iter().zip(disabled.as_chunks::<4>().0).filter(|(a, b)| a != b).count();
        assert!(changed > 20, "projection produced no visible contact halo: tess={tess}, {changed}");
    }
    let names: Vec<_> = crate::open_meshing::session_world(&app.session).bodies.iter().map(|b| b.name.clone()).collect();
    app.run("appearance.assign", json!({"bodies":names,"color":"#bec3cd","opacity":0.0})).unwrap();
    let mut render = |mode| {
        app.run("view.ground_shadow", json!({"mode":mode})).unwrap();
        crate::gpu::render_offscreen_layers(
            build_scene(&app),
            build_sketch_scene(&app, &projection),
            projection.vp.to_f32(),
            app.cam.back().to_f32(),
            [eye[0], eye[1], eye[2], 0.0],
            256,
            Some(solidcraft_gpu::SHADER),
            [0.0; 8],
            [0.0; 36],
        )
        .unwrap()
    };
    let projected = render("projected");
    let disabled = render("off");
    // The existing viewport keeps transparent faces at a minimum display opacity of 5%.
    // Reusing its buffers must preserve that faint coverage, without accumulating all faces.
    let darkening = projected.iter().zip(disabled).map(|(a, b)| b.saturating_sub(*a)).max().unwrap_or(0);
    // Dark palette strength is 150/255 * 0.8: at 13/255 display alpha it darkens a white
    // pixel by at most 7 levels, plus one level of render/filter rounding. Light is weaker.
    assert!((1..=8).contains(&darkening), "shadow must follow the existing 5% display opacity: {darkening}");
}

#[test]
fn projected_shadow_keeps_the_grouped_ellipse_fallback_and_ground_preference() {
    use solidcraft_engine::{geom::Aabb3, render::UpAxis};

    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    for up in [UpAxis::Z, UpAxis::Y] {
        app.cam.up = up;
        for bottom in [-20.0_f64, 20.0] {
            let boxes = vec![
                Aabb3 { min: Vec3::new(-1.5, -2.0, bottom), max: Vec3::new(1.5, 2.0, bottom + 12.0) },
                Aabb3 { min: Vec3::new(28.5, -2.0, bottom), max: Vec3::new(31.5, 2.0, bottom + 12.0) },
            ];
            for follows_model in [false, true] {
                app.preferences.ground_shadow_follows_model = follows_model;
                app.preferences.ground_shadow_style = crate::prefs::GroundShadowStyle::Ellipse;
                let mut ellipse = GpuScene::default();
                ground_shadow(&mut ellipse, &app, boxes.clone());
                app.preferences.ground_shadow_style = crate::prefs::GroundShadowStyle::Projected;
                let mut projected = GpuScene::default();
                ground_shadow(&mut projected, &app, boxes.clone());
                let fallback = &projected.projected_shadow.as_ref().unwrap().fallback;
                assert!(projected.shadow.is_empty());
                assert!(!fallback.is_empty());
                assert_eq!(fallback, &ellipse.shadow, "unsupported adapters retain main's grouped ellipses");
                let plane = if follows_model { bottom } else { bottom.min(0.0) };
                let expected = plane - 1e-3 * (1.0 + (33.0_f64.powi(2) + 4.0_f64.powi(2) + 12.0_f64.powi(2)).sqrt());
                for vertex in fallback.as_chunks::<{ crate::gpu::TRI_SIZE }>().0 {
                    let read = |i| f64::from(f32::from_le_bytes(vertex[i..i + 4].try_into().unwrap()));
                    let point = app.cam.to_frame(Vec3::new(read(0), read(4), read(8)));
                    assert!((point.z - expected).abs() < 1e-5, "fallback must share the selected ground plane");
                }
            }
        }
    }
}
