use super::*;

fn app() -> SolveApp {
    let mut session = solidcraft_engine::Session::default();
    session.execute("solid.box", &json!({"length":40,"width":30,"height":20})).unwrap();
    let mut app = SolveApp::new(session, crate::Services::default());
    app.home.open = false;
    app.set_origin_shown(false);
    app.ui.show_grid = false;
    app.ui.ground_shadow = true;
    app.viewport.gpu = Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false });
    app.cam.pitch = 0.7;
    app
}

fn frame(app: &mut SolveApp, ctx: &egui::Context) {
    frame_events(app, ctx, vec![]);
}

fn frame_events(app: &mut SolveApp, ctx: &egui::Context, events: Vec<egui::Event>) {
    let mut output = ctx
        .run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))), events, ..Default::default() }, |ui| {
            show(app, ui)
        });
    output.textures_delta.clear();
}

#[test]
fn face_hover_and_clicks_follow_device_rotation_on_each_axis() {
    use crate::space_mouse::{DeviceMotion, Motion};
    for rotation in [[0.1, 0.0, 0.0], [0.0, 0.1, 0.0], [0.0, 0.0, 0.1]] {
        let ctx = crate::fonts::test_context();
        let mut app = app();
        app.cam.target = Vec3::new(20.0, 15.0, 10.0);
        frame(&mut app, &ctx);
        let p = projection(&app, app.viewport.rect.unwrap()).to_screen(app.cam.target).unwrap();
        let turn = |app: &mut SolveApp| {
            app.viewport.device_motion = Some(DeviceMotion { motion: Motion { translation: [0.0; 3], rotation }, seconds: 0.016 });
        };
        let before = app.cam;
        for pressed in [None, Some(true), Some(false)] {
            turn(&mut app);
            let mut events = vec![egui::Event::PointerMoved(p)];
            if let Some(pressed) = pressed {
                events.push(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
            }
            frame_events(&mut app, &ctx, events);
            let proj = projection(&app, app.viewport.rect.unwrap());
            let (expected, _) = candidate(&app, &pick(&app, &proj, p)).unwrap();
            assert!(matches!(expected, Hit::Face { .. }));
            assert_eq!(app.viewport.hover, Some(expected));
        }
        assert_ne!(app.cam, before);
        let proj = projection(&app, app.viewport.rect.unwrap());
        let (_, expected) = candidate(&app, &pick(&app, &proj, p)).unwrap();
        assert_eq!(app.session.selection, vec![expected]);
    }
}

/// Crossing the ground used to rebuild every body even with the shadow switched off.
#[test]
fn flips_reuse_the_model_and_only_switch_shadow_visibility() {
    for (shadow, ao) in [false, true].into_iter().flat_map(|shadow| [false, true].map(|ao| (shadow, ao))) {
        let ctx = crate::fonts::test_context();
        let mut app = app();
        app.ui.ground_shadow = shadow;
        app.preferences.ambient_occlusion = ao;
        frame(&mut app, &ctx);
        let model_key = app.viewport.key;
        let above = app.viewport.sketch_slot.lock().unwrap().take().unwrap();
        assert_eq!(!above.shadow.is_empty(), shadow);
        let geometry = shadow_geometry(&app, &app.session.world_state().unwrap());
        app.viewport.slot.lock().unwrap().take();
        app.cam.pitch = -0.7;
        frame(&mut app, &ctx);
        assert_eq!(app.viewport.key, model_key);
        assert!(app.viewport.slot.lock().unwrap().is_none(), "a camera flip must not rebuild or upload the model");
        let below = app.viewport.sketch_slot.lock().unwrap().take().unwrap();
        assert!(below.shadow.is_empty(), "the ground shadow is hidden from below");
        app.cam.pitch = 0.7;
        frame(&mut app, &ctx);
        assert!(app.viewport.slot.lock().unwrap().is_none());
        let again = app.viewport.sketch_slot.lock().unwrap().take().unwrap();
        if shadow {
            assert!(
                std::sync::Arc::ptr_eq(&geometry.bytes, &shadow_geometry(&app, &app.session.world_state().unwrap()).bytes),
                "returning above reuses the shadow geometry"
            );
            assert_eq!(above.shadow, again.shadow);
            assert_eq!(above.palette.entries(), again.palette.entries(), "cached shadow indices keep their colours");
            assert!(!again.palette.is_empty());
            assert!(!again.shadow.is_empty());
            assert!(again.merged().trans.is_empty(), "the shadow stays in its dedicated pass");
        }
        app.ui.ground_shadow = !shadow;
        frame(&mut app, &ctx);
        assert!(app.viewport.slot.lock().unwrap().is_none(), "toggling a shadow must not rebuild the model");
        let toggled = app.viewport.sketch_slot.lock().unwrap().take().unwrap();
        assert_eq!(!toggled.shadow.is_empty(), !shadow);
        app.preferences.ambient_occlusion = !ao;
        frame(&mut app, &ctx);
        assert!(app.viewport.slot.lock().unwrap().is_none(), "toggling AO must not rebuild model geometry");
    }
}

#[test]
fn shadow_cache_follows_visibility_and_world_up() {
    let mut app = app();
    let st = app.session.world_state().unwrap();
    let first = shadow_geometry(&app, &st);
    assert!(!first.bytes.is_empty());
    app.session.visibility.hidden_bodies.push("Body1".into());
    let hidden = shadow_geometry(&app, &st);
    assert_ne!(first.key, hidden.key);
    assert!(hidden.bytes.is_empty());
    app.session.visibility.hidden_bodies.clear();
    app.cam.up = solidcraft_viewport::UpAxis::Y;
    let turned = shadow_geometry(&app, &st);
    assert_ne!(first.key, turned.key);
    assert_ne!(first.bytes, turned.bytes);
}

#[test]
fn face_feedback_reuses_geometry_but_retains_fresh_pick_points() {
    let mut app = app();
    app.viewport.hover = Some(Hit::Face { body: "Body1".into(), index: 0, point: Vec3::new(10.0, 0.0, 5.0) });
    let key = highlight_geometry_key(&app);
    let first = build_highlight(&app);
    let fresh = Vec3::new(20.0, 0.0, 5.0);
    app.viewport.hover = Some(Hit::Face { body: "Body1".into(), index: 0, point: fresh });
    app.cam.pitch = -0.4;
    assert_eq!(highlight_geometry_key(&app), key);
    let again = build_highlight(&app);
    assert!(std::sync::Arc::ptr_eq(&first.blocks[0].bytes, &again.blocks[0].bytes));
    assert!(
        matches!(hit_sel(app.viewport.hover.as_ref().unwrap()),Some(Sel::Face { point,.. }) if point==fresh),
        "clicks keep the new intersection point"
    );
    app.viewport.hover = Some(Hit::Face { body: "Body1".into(), index: 1, point: fresh });
    assert_ne!(highlight_geometry_key(&app), key, "moving to another face invalidates feedback");
    app.viewport.hover = None;
    assert_ne!(highlight_geometry_key(&app), key, "leaving the model clears feedback");
}

/// A STEP opens with its assembly placement active. Re-expanding all descendants for every
/// new hover used to dominate the frame; cached membership must still follow real edits.
#[test]
fn active_assembly_membership_is_reused_and_follows_edits_and_activation() {
    let mut s = solidcraft_engine::Session::default();
    let a = s.execute("component.create", &json!({"name":"A"})).unwrap()["component"].as_u64().unwrap();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
    s.execute("component.create", &json!({"name":"Child"})).unwrap();
    s.execute("solid.box", &json!({"length":5,"width":5,"height":5})).unwrap();
    s.execute("component.activate", &json!({"component":0})).unwrap();
    let b = s.execute("component.create", &json!({"name":"B"})).unwrap()["component"].as_u64().unwrap();
    s.execute("solid.box", &json!({"length":4,"width":4,"height":4})).unwrap();
    let path = s.doc.first_path(a);
    s.execute("component.activate", &json!({"component":a,"path":path,"occurrence":path.last()})).unwrap();
    let mut app = SolveApp::new(s, crate::Services::default());
    let first = active_body_names(&app).unwrap();
    assert_eq!(first.len(), 2, "the child belongs to the active assembly too");
    app.cam.pitch = -0.5;
    let again = active_body_names(&app).unwrap();
    assert!(std::sync::Arc::ptr_eq(&first, &again));
    let world = app.session.world_state().unwrap();
    assert_eq!(world.bodies.iter().filter(|body| body_faded(&app, body, &Some(again.clone()))).count(), 1);
    app.session.execute("solid.box", &json!({"length":3,"width":3,"height":3})).unwrap();
    let edited = active_body_names(&app).unwrap();
    assert_eq!(edited.len(), 3);
    assert!(!std::sync::Arc::ptr_eq(&first, &edited));
    let path = app.session.doc.first_path(b);
    app.session.execute("component.activate", &json!({"component":b,"path":path,"occurrence":path.last()})).unwrap();
    let other = active_body_names(&app).unwrap();
    assert_eq!(other.len(), 1);
    assert!(other.is_disjoint(&edited));
    app.session.execute("component.activate", &json!({"component":0})).unwrap();
    assert!(active_body_names(&app).is_none());
    assert!(app.viewport.active_bodies.lock().unwrap().is_none());
}

/// Opening has a complete evaluated world, but only ready meshes belong to the picture.
#[test]
fn opening_does_not_draw_bodies_before_their_meshes_are_shown() {
    let mut app = app();
    assert!(!app.session.world_state().unwrap().bodies.is_empty());
    crate::open_meshing::start(&mut app, None);
    assert!(crate::open_meshing::shown_world(&app).bodies.is_empty());
    // No refresh has published any body yet, even if the background worker has finished.
    assert!(build_scene(&app).blocks.is_empty(), "the model uses the opening picture");
    let proj = projection(&app, Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)));
    assert!(build_sketch_scene(&app, &proj).shadow.is_empty(), "the shadow uses that same picture");
}

fn wireframe_cylinder() -> SolveApp {
    let mut s = solidcraft_engine::Session::default();
    s.execute("sketch.create", &json!({"plane":"XY"})).unwrap();
    s.execute("sketch.circle.center", &json!({"center":[0,0],"radius":10})).unwrap();
    s.execute("sketch.finish", &json!({})).unwrap();
    s.execute("solid.extrude", &json!({"distance":20})).unwrap();
    let mut app = SolveApp::new(s, crate::Services::default());
    app.set_origin_shown(false);
    app.ui.show_grid = false;
    app.ui.ground_shadow = false;
    app.ui.visual_style = 2;
    app.cam.pitch = 0.0;
    app.cam.yaw = 0.0;
    app.cam.fov = 0.0;
    app
}

fn settle_wireframe(app: &mut SolveApp) {
    for _ in 0..1000 {
        let proj = projection(app, Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)));
        refresh_sketch_scene(app, &proj);
        if !app.viewport.wireframe.busy() {
            return;
        }
    }
    panic!("wireframe work did not finish");
}

#[test]
fn wireframe_silhouettes_follow_orbit_without_changing_rims_or_model_buffers() {
    let mut app = wireframe_cylinder();
    let key = scene_key(&app);
    let rims = build_scene(&app).merged().lines;
    assert!(!rims.is_empty(), "circular rims were already drawn");
    settle_wireframe(&mut app);
    let sides = app.viewport.wireframe.segments().to_vec();
    let vertical_length: f64 = sides.iter().filter(|[a, b]| (a.z - b.z).abs() > 1e-6).map(|[a, b]| a.dist(*b)).sum();
    assert!((vertical_length - 40.0).abs() < 1e-6, "two 20 mm cylinder sides: {vertical_length}");
    let drawn = app.viewport.sketch_slot.lock().unwrap().as_ref().unwrap().clone().merged();
    assert_eq!(drawn.lines.len(), sides.len() * crate::gpu::LINE_SIZE);
    assert!(drawn.tris.is_empty(), "wireframe outlines must not restore shaded faces");
    app.cam.yaw += 0.7;
    settle_wireframe(&mut app);
    assert_ne!(app.viewport.wireframe.segments(), sides);
    assert_eq!(scene_key(&app), key, "orbit must not invalidate the model buffers");
    assert_eq!(build_scene(&app).merged().lines, rims, "authored rims are unchanged");
    app.session.visibility.hidden_bodies.push("Body1".into());
    settle_wireframe(&mut app);
    assert!(app.viewport.wireframe.segments().is_empty(), "hidden bodies contribute no silhouette");
    app.session.visibility.hidden_bodies.clear();
    settle_wireframe(&mut app);
    assert!(!app.viewport.wireframe.segments().is_empty());
    app.ui.visual_style = 0;
    settle_wireframe(&mut app);
    assert!(app.viewport.wireframe.model().is_none(), "leaving Wireframe releases its cache");
    assert!(app.viewport.wireframe.segments().is_empty());
}

#[test]
fn wireframe_silhouette_work_is_bounded_and_shared_placements_draw_in_world_space() {
    use crate::wireframe_silhouettes::{Outlines, Source};
    use solidcraft_engine::doc::IDENTITY;
    let app = wireframe_cylinder();
    let mesh = app.session.world_state().unwrap().bodies[0].mesh();
    let mut shifted = IDENTITY;
    shifted[3][0] = 100.0;
    let mut outlines = Outlines::default();
    outlines.set_model(Some(1), 2);
    let source = |index| Source::new(mesh.clone(), if index == 0 { IDENTITY } else { shifted });
    let mesh_owners = std::sync::Arc::strong_count(&mesh);
    assert_eq!(outlines.advance(&app.cam, 0, source), 0);
    assert!(outlines.busy());
    let mut frames = 0;
    while outlines.busy() {
        assert!(outlines.advance(&app.cam, 7, source) <= 7);
        frames += 1;
        assert!(frames < 100_000);
    }
    assert!(frames > 1, "the work must continue across frames");
    assert_eq!(std::sync::Arc::strong_count(&mesh), mesh_owners, "cached topology must not retain obsolete mesh buffers");
    let left: Vec<_> = outlines.segments().iter().filter(|s| s[0].x < 50.0).copied().collect();
    let right: Vec<_> = outlines.segments().iter().filter(|s| s[0].x > 50.0).map(|s| s.map(|p| p - Vec3::new(100.0, 0.0, 0.0))).collect();
    assert_eq!(left.len(), right.len());
    assert!(!left.is_empty());
    for (a, b) in left.iter().zip(right) {
        assert!(a.iter().zip(b).all(|(p, q)| p.dist(q) < 1e-12));
    }
    let generation = outlines.generation();
    assert_eq!(outlines.advance(&app.cam, 100, source), 0, "a stationary completed view does no work");
    assert_eq!(outlines.generation(), generation);
}

#[test]
fn wireframe_silhouette_preparation_survives_orbits_and_cancels_incrementally() {
    use crate::wireframe_silhouettes::{Outlines, Source};
    use solidcraft_engine::doc::IDENTITY;
    let mut app = wireframe_cylinder();
    let mesh = app.session.world_state().unwrap().bodies[0].mesh();
    let source = |_| Source::new(mesh.clone(), IDENTITY);
    let mut outlines = Outlines::default();
    outlines.set_model(Some(1), 1);
    for _ in 0..20 {
        app.cam.yaw += 0.1;
        assert!(outlines.advance(&app.cam, 3, source) <= 3);
    }
    let settle = |outlines: &mut Outlines| {
        for _ in 0..100_000 {
            assert!(outlines.advance(&app.cam, 7, source) <= 7);
            if !outlines.busy() {
                return;
            }
        }
        panic!("bounded silhouette job failed to settle");
    };
    settle(&mut outlines);
    let mut fresh = Outlines::default();
    fresh.set_model(Some(1), 1);
    settle(&mut fresh);
    assert_eq!(outlines.segments(), fresh.segments(), "orbits must not corrupt unfinished adjacency");
    let mut canceled = Outlines::default();
    canceled.set_model(Some(1), 1);
    canceled.advance(&app.cam, 10, source);
    canceled.set_model(None, 0);
    assert!(canceled.busy(), "retire unfinished adjacency over subsequent frames");
    settle(&mut canceled);
    assert!(canceled.segments().is_empty());
    let mut hidden = Outlines::default();
    hidden.set_model(Some(2), 100_000);
    let mut discovered = 0;
    hidden.advance(&app.cam, 7, |_| {
        discovered += 1;
        None
    });
    assert_eq!(discovered, 7, "hidden and empty bodies cannot bypass the discovery budget");
}
