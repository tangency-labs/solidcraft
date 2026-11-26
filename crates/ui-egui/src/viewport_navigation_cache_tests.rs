use super::*;

fn app() -> SolveApp {
    let mut session = solidcraft_engine::Session::default();
    session.execute("solid.box", &json!({"length":40,"width":30,"height":20})).unwrap();
    let mut app = SolveApp::new(session, crate::Services::default());
    app.home.open = false;
    app.set_origin_shown(false);
    app.ui.show_grid = false;
    app.ui.ground_shadow = true;
    app.viewport.gpu =
        Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false, edges: false });
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

/// While a design opens, each picture holds the bodies meshed so far in the order they were
/// meshed, with the rest of the world's state: built from the picture before when nothing else
/// holds it (the new bodies added), copied when something does (which keeps its own bodies),
/// never by copying the whole world first.
#[test]
fn opening_shows_the_bodies_meshed_so_far_in_order_adding_to_the_picture_before() {
    let mut s = solidcraft_engine::Session::default();
    for k in 0..5 {
        s.execute("solid.box", &json!({"length":10,"width":10,"height":10,"corner":[20 * k, 0, 0],"operation":"new"})).unwrap();
    }
    let mut app = SolveApp::new(s, crate::Services::default());
    app.viewport.gpu =
        Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false, edges: false });
    let world = crate::open_meshing::session_world(&app.session);
    assert_eq!(world.bodies.len(), 5);
    crate::open_meshing::without_threads(|| crate::open_meshing::start(&mut app, None));
    let names = |st: &solidcraft_engine::doc::ModelState| st.bodies.iter().map(|b| b.name.clone()).collect::<Vec<_>>();
    let of = |order: &[usize]| order.iter().map(|&i| world.bodies[i].name.clone()).collect::<Vec<_>>();
    let mut now = 1e6;
    let mut show = |app: &mut SolveApp, order: &[usize]| {
        crate::open_meshing::set_ready(app, order);
        now += 1e6;
        crate::open_meshing::refresh(app, now);
        crate::open_meshing::shown_world(app)
    };
    let first = show(&mut app, &[3, 1]);
    assert_eq!(names(&first), of(&[3, 1]));
    assert_eq!(first.body_counter, world.body_counter, "the rest of the world's state with them");
    drop(first);
    let second = show(&mut app, &[3, 1, 0]);
    assert_eq!(names(&second), of(&[3, 1, 0]), "the new body added after the others");
    // Held elsewhere, the picture keeps its own bodies; the next is copied.
    let third = show(&mut app, &[3, 1, 0, 4, 2]);
    assert_eq!(names(&third), of(&[3, 1, 0, 4, 2]));
    assert_eq!(names(&second), of(&[3, 1, 0]), "the picture held before is as it was");
}

/// Opening a part placed many times makes, off the window's thread, the plan mesh its copies
/// are drawn from at a finer display quality (their part's, made once): the first frames would
/// otherwise plan every part there, which held a hundred-times assembly's window for most of a
/// minute.
#[test]
fn opening_makes_the_plan_mesh_of_a_part_placed_many_times_once_for_its_copies() {
    let mut s = solidcraft_engine::Session::default();
    let part = s.execute("component.create", &json!({"name":"Part","grounded":false})).unwrap()["component"].clone();
    s.execute("solid.cylinder", &json!({"base":[0,0,0],"radius":5,"height":20})).unwrap();
    s.execute("component.activate", &json!({"component":0})).unwrap();
    for k in 1..=3 {
        s.execute("occurrence.copy", &json!({"component":part,"translate":[0,40 * k,0]})).unwrap();
    }
    let world = s.world_state().unwrap();
    let repeated = crate::open_meshing::repeated_parts(&world);
    assert_eq!(repeated.len(), 1, "one part, defined once and placed three times more");
    let q = solidcraft_engine::doc::display_quality::DisplayQuality::Fine;
    let copies: Vec<_> = world.bodies.iter().filter(|b| b.placed_definition().is_some()).collect();
    assert_eq!(copies.len(), 3);
    assert!(copies.iter().all(|b| !b.plan_mesh_in_place_made(q)));
    crate::open_meshing::warm(copies[0], true, false, &repeated, q);
    assert!(copies.iter().all(|b| b.plan_mesh_in_place_made(q)), "every copy's is its part's, made once");
}

/// While a design opens, a copy of a part placed many times that is shown before the part's other
/// copies is still drawn as an instance, as opening counted it, and not planned on the window's
/// thread: counted over the bodies shown so far, it was the part's only copy.
#[test]
fn a_copy_shown_before_its_parts_other_copies_is_still_drawn_as_an_instance() {
    let mut s = solidcraft_engine::Session::default();
    let part = s.execute("component.create", &json!({"name":"Part","grounded":false})).unwrap()["component"].clone();
    s.execute("solid.cylinder", &json!({"base":[0,0,0],"radius":5,"height":20})).unwrap();
    s.execute("component.activate", &json!({"component":0})).unwrap();
    for k in 1..=3 {
        s.execute("occurrence.copy", &json!({"component":part,"translate":[0,40 * k,0]})).unwrap();
    }
    let mut app = SolveApp::new(s, crate::Services::default());
    let world = app.session.world_state().unwrap();
    let first = world.bodies.iter().position(|b| b.placed_definition().is_some()).unwrap();
    crate::open_meshing::start(&mut app, None);
    crate::open_meshing::show_only(&mut app, &[first]);
    let shown = crate::open_meshing::shown_world(&app);
    assert_eq!(shown.bodies.len(), 1, "one copy shown so far");
    assert!(repeated_copy(&app, &shown.bodies[0]), "drawn as an instance of its part, as opening counted it");
}

/// The mesher working ahead of the first frames also works out each body's box, which framing
/// the view reads for every body once they are all shown: on the window thread, a placed copy's box
/// moved every point of its part's mesh (a second for a hundred copies of a large assembly).
#[test]
fn opening_works_out_each_placed_copys_box_off_the_window_thread() {
    let mut s = solidcraft_engine::Session::default();
    let part = s.execute("component.create", &json!({"name":"Part","grounded":false})).unwrap()["component"].clone();
    s.execute("solid.cylinder", &json!({"base":[0,0,0],"radius":5,"height":20})).unwrap();
    s.execute("component.activate", &json!({"component":0})).unwrap();
    for k in 1..=3 {
        s.execute("occurrence.copy", &json!({"component":part,"translate":[0,40 * k,0]})).unwrap();
    }
    let world = s.world_state().unwrap();
    assert!(world.bodies.iter().all(|b| !b.mesh_bounds_made()));
    let repeated = crate::open_meshing::repeated_parts(&world);
    let q = solidcraft_engine::doc::display_quality::DisplayQuality::Normal;
    for b in &world.bodies {
        crate::open_meshing::warm(b, false, false, &repeated, q);
    }
    assert!(world.bodies.iter().all(|b| b.mesh_bounds_made()), "every body's box, ahead of framing");
    // A world state built again from the same placements keeps them.
    let again = s.world_state().unwrap();
    assert!(again.bodies.iter().all(|b| b.mesh_bounds_made()));
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

#[test]
fn orbit_skips_origin_occlusion_raycasts_and_recomputes_when_rested() {
    use std::sync::atomic::Ordering;
    let ctx = crate::fonts::test_context();
    let mut app = app();
    app.set_origin_shown(true);
    app.ui.show_grid = true;
    app.fit_view();
    // First resting frame computes exact origin occlusion once.
    frame(&mut app, &ctx);
    let after_first = app.viewport.origin_hidden_evals.load(Ordering::Relaxed);
    assert_eq!(after_first, 1, "first resting frame evaluates origin occlusion once");
    // Repeated still frames reuse the cached occlusion without re-running raycasts.
    frame(&mut app, &ctx);
    frame(&mut app, &ctx);
    assert_eq!(app.viewport.origin_hidden_evals.load(Ordering::Relaxed), after_first, "still frames reuse cached origin occlusion");
    // Actively orbiting with the right mouse button skips origin occlusion raycasts every frame.
    let rect = app.viewport.rect.unwrap();
    let at = rect.center();
    let none = egui::Modifiers::NONE;
    frame_events(&mut app, &ctx, vec![egui::Event::PointerMoved(at)]);
    frame_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed: true, modifiers: none }],
    );
    let yaw_before = app.cam.yaw;
    let evals_before_drag = app.viewport.origin_hidden_evals.load(Ordering::Relaxed);
    for i in 1..=10 {
        let p = at + vec2(i as f32 * 8.0, 0.0);
        frame_events(&mut app, &ctx, vec![egui::Event::PointerMoved(p)]);
    }
    assert_ne!(app.cam.yaw, yaw_before, "right drag orbited the camera");
    assert_eq!(
        app.viewport.origin_hidden_evals.load(Ordering::Relaxed),
        evals_before_drag,
        "orbiting frames must not raycast bodies for the origin dot"
    );
    // Releasing the mouse button ends navigation and immediately recomputes exact occlusion.
    let end = at + vec2(80.0, 0.0);
    frame_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerButton { pos: end, button: egui::PointerButton::Secondary, pressed: false, modifiers: none }],
    );
    assert_eq!(
        app.viewport.origin_hidden_evals.load(Ordering::Relaxed),
        evals_before_drag + 1,
        "releasing orbit recomputes exact origin occlusion once"
    );
    // Hovering without moving the camera still reuses the new cached result.
    frame_events(&mut app, &ctx, vec![egui::Event::PointerMoved(end + vec2(10.0, 5.0))]);
    assert_eq!(
        app.viewport.origin_hidden_evals.load(Ordering::Relaxed),
        evals_before_drag + 1,
        "pointer motion over a still view reuses cached origin occlusion"
    );
}

#[test]
fn origin_hidden_by_body_culls_bodies_behind_origin_and_keeps_front_occluders() {
    let mut s = solidcraft_engine::Session::default();
    // With yaw = 0, pitch = 0, Camera::back() is -Y, so the eye sits at (0, -80, 0) looking along +Y.
    // Place a body strictly behind the origin along the view ray (y in [30, 40]).
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "corner": [-5, 30, -5], "operation": "new"})).unwrap();
    let mut app = SolveApp::new(s, crate::Services::default());
    app.cam.target = Vec3::ZERO;
    app.cam.yaw = 0.0;
    app.cam.pitch = 0.0;
    app.cam.distance = 80.0;
    let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
    let proj = projection(&app, rect);
    assert!(!origin_hidden_by_body(&app, &proj, None), "body behind the origin does not hide it");
    // Now add a body strictly in front of the origin (y in [-30, -20], between eye at -80 and origin at 0).
    app.session.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "corner": [-5, -30, -5], "operation": "new"})).unwrap();
    let proj = projection(&app, rect);
    assert!(origin_hidden_by_body(&app, &proj, None), "body in front of the origin hides it");
}

#[test]
fn horizontal_orbit_with_grid_reuses_sketch_scene_and_rebuilds_on_grid_or_ground_change() {
    let ctx = crate::fonts::test_context();
    let mut app = app();
    app.ui.show_grid = true;
    app.ui.ground_shadow = true;
    app.ui.perspective = false;
    app.cam.target = Vec3::ZERO;
    app.cam.pitch = 0.6;
    app.cam.yaw = 0.4;
    app.cam.distance = 120.0;
    frame(&mut app, &ctx);
    let first = app.viewport.sketch_slot.lock().unwrap().take();
    assert!(first.is_some(), "initial frame publishes the grid and shadow scene");
    let first_key = app.viewport.sketch_key;
    // Horizontal yaw orbit with steady pitch and zoom keeps the snapped ground grid identical.
    for step in 1..=10 {
        app.cam.yaw += 0.15;
        frame(&mut app, &ctx);
        assert_eq!(app.viewport.sketch_key, first_key, "step {step}: horizontal orbit keeps sketch scene key stable");
        assert!(
            app.viewport.sketch_slot.lock().unwrap().take().is_none(),
            "step {step}: horizontal orbit must not rebuild or re-upload the sketch scene"
        );
    }
    // Zooming changes the grid extent/fade and rebuilds the sketch scene.
    app.cam.distance = 40.0;
    frame(&mut app, &ctx);
    assert_ne!(app.viewport.sketch_key, first_key, "zooming invalidates the grid key");
    assert!(app.viewport.sketch_slot.lock().unwrap().take().is_some(), "zooming publishes the updated grid");
    // Flipping below the ground hides the ground shadow and rebuilds the sketch scene.
    let before_flip = app.viewport.sketch_key;
    app.cam.pitch = -0.6;
    frame(&mut app, &ctx);
    assert_ne!(app.viewport.sketch_key, before_flip, "crossing the ground invalidates the sketch scene");
    let below = app.viewport.sketch_slot.lock().unwrap().take().expect("ground flip publishes updated shadow visibility");
    assert!(below.shadow.is_empty(), "ground shadow stays hidden from below");
}

#[test]
fn highlight_key_distinguishes_selections_and_stays_stable_during_orbit() {
    let mut app = app();
    let base = highlight_geometry_key(&app);
    let base_full = highlight_key(&app);
    app.cam.yaw += 0.5;
    app.cam.pitch += 0.2;
    assert_eq!(highlight_geometry_key(&app), base, "orbiting leaves highlight geometry key unchanged");
    assert_eq!(highlight_key(&app), base_full, "orbiting leaves highlight key unchanged when no sketches or long axes are active");

    app.session.selection = vec![Sel::Body { name: "Body1".into() }];
    let body_key = highlight_geometry_key(&app);
    assert_ne!(body_key, base, "selecting a body changes highlight key");

    app.session.selection = vec![Sel::Face { body: "Body1".into(), index: 0, point: Vec3::ZERO }];
    let face0_key = highlight_geometry_key(&app);
    assert_ne!(face0_key, body_key, "selecting a face differs from selecting its body");

    app.session.selection = vec![Sel::Face { body: "Body1".into(), index: 1, point: Vec3::ZERO }];
    let face1_key = highlight_geometry_key(&app);
    assert_ne!(face1_key, face0_key, "different face indices hash differently");

    app.session.selection = vec![Sel::Edge { body: "Body1".into(), index: 1, point: Vec3::ZERO }];
    let edge1_key = highlight_geometry_key(&app);
    assert_ne!(edge1_key, face1_key, "edge selection differs from face selection with same index");
}

#[test]
fn feature_names_cache_reuses_arc_across_frames_and_invalidates_on_edit() {
    let mut app = app();
    let first = FeatureNames::cached(&app);
    let again = FeatureNames::cached(&app);
    assert!(std::sync::Arc::ptr_eq(&first, &again), "unchanged document reuses cached FeatureNames");
    app.session.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "corner": [50, 0, 0], "operation": "new"})).unwrap();
    let after_edit = FeatureNames::cached(&app);
    assert!(!std::sync::Arc::ptr_eq(&first, &after_edit), "editing the document invalidates cached FeatureNames");
}
