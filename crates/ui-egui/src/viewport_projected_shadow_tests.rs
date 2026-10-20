use super::*;

#[test]
fn projected_ground_shadow_preserves_gaps_and_display_opacity() {
    let mut session = solidcraft_engine::Session::default();
    for x in [-20, 16] {
        for y in [-20, 16] {
            session.execute("solid.box", &json!({"length":4,"width":4,"height":8,"corner":[x,y,0],"operation":"new"})).unwrap();
        }
    }
    let mut app = SolveApp::new(session, crate::Services::default());
    app.cam =
        Camera { target: Vec3::new(0.0, 0.0, 4.0), distance: 90.0, pitch: std::f64::consts::FRAC_PI_2, yaw: 0.0, fov: 0.0, ..Default::default() };
    let projection = projection(&app, Rect::from_min_size(Pos2::ZERO, vec2(256.0, 256.0)));
    let eye = app.cam.eye().to_f32();
    for tess in [false, true] {
        app.viewport.gpu = Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess, oit: false });
        let mut render = |mode| {
            app.run("view.ground_shadow", json!({"mode":mode})).unwrap();
            crate::gpu::render_offscreen(
                build_scene(&app),
                None,
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
        crate::gpu::render_offscreen(
            build_scene(&app),
            None,
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
