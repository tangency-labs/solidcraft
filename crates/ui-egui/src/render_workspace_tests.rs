use super::*;
use crate::scenario::Harness;
use egui::Color32;
use solidcraft_engine::Session;

fn click_render_check(h: &mut Harness) {
    h.app.run("view.render_select", json!({"kind":"check"})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:check"}})).unwrap();
    h.app.run("view.render_select", json!({"kind":"output"})).unwrap();
    h.frames(3);
}

fn environment_editor_frame(h: &mut Harness, events: Vec<egui::Event>) {
    let mut output = h.ctx.run_ui(
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 1800.0))), events, ..Default::default() },
        |ui| {
            ui.set_width(308.0);
            environment::editor(&mut h.app, ui);
        },
    );
    output.textures_delta.clear();
}

fn environment_editor_fixture() -> Harness {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5})).unwrap();
    let name = h.app.render_workspace.view.name.clone();
    h.app.run("render.settings.set", json!({"name":name,"set":{"job":{"method":"gpu_path","options":{"environment":{"kind":"sky"}}}}})).unwrap();
    setups::load(&mut h.app);
    h
}

#[test]
fn environment_editor_e4_variants_keep_all_five_sections_in_order() {
    for variant in ["embedded", "sky", "file"] {
        let mut h = environment_editor_fixture();
        if variant == "embedded" {
            let name = h.app.render_workspace.view.name.clone();
            h.app.run("render.library.environment.apply", json!({"id":"HDR-001","name":name,"replace_environment":true})).unwrap();
            setups::load(&mut h.app);
        } else if variant == "file" {
            h.app.render_workspace.view.studio.environment = studio::Environment::Hdr;
            h.app.render_workspace.view.studio.hdr = hdr::Input::FilePending { path: "C:/hdr/studio.hdr".into() };
        } else {
            h.app.render_workspace.view.studio.backdrop_enabled = true;
        }
        let before = h.app.session.doc.clone();
        let mut output = h.ctx.run_ui(
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(308.0, 3000.0))), ..Default::default() },
            |ui| environment::editor(&mut h.app, ui),
        );
        let mut previous = -1.0;
        let source_row = crate::scenario::rect_of("dialog:control:Source").unwrap();
        let change = crate::scenario::rect_of("render:environment:change").unwrap();
        assert!(source_row.contains_rect(change), "{variant}: Change must fit beside Source");
        for key in [
            "render.env_source",
            "render.environment.orientation",
            "render.env_backdrop",
            "render.environment.ground",
            "render.environment.atmosphere",
        ] {
            let rect = crate::scenario::rect_of(&format!("dialog:section:{}", h.app.tr(key))).unwrap();
            assert!(rect.top() > previous, "{variant}: {key} out of order");
            previous = rect.top();
        }
        let text = crate::test_support::text_shapes(&output).map(|shape| shape.galley.job.text.clone()).collect::<Vec<_>>().join("\n");
        for key in [
            "render.environment.replace",
            "render.environment.fit",
            "render.environment.floor_wall",
            "render.environment.cyclorama",
            "render.environment.medium",
            "render.environment.phase",
        ] {
            assert!(text.contains(h.app.tr(key)), "{variant}: missing {key}: {text}");
        }
        for key in match variant {
            "embedded" => &["render.environment.embedded", "render.environment.builtin"][..],
            "file" => &["render.environment.choose", "render.environment.check"][..],
            _ => &[
                "render.environment.zenith",
                "render.environment.horizon",
                "render.environment.sky_ground",
                "render.environment.start",
                "render.environment.end",
            ][..],
        } {
            assert!(text.contains(h.app.tr(key)), "{variant}: missing {key}");
        }
        assert!(Arc::ptr_eq(&before, &h.app.session.doc), "opening {variant} must never save");
        output.textures_delta.clear();
    }
}

#[test]
fn environment_editor_unsupported_methods_keep_controls_and_reasons_without_switching() {
    for method in [ImageMethod::Raster, ImageMethod::CpuRaytrace] {
        let mut h = environment_editor_fixture();
        h.app.render_workspace.view.method = method;
        let before = h.app.session.doc.clone();
        let mut output = h.ctx.run_ui(
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(308.0, 3000.0))), ..Default::default() },
            |ui| environment::editor(&mut h.app, ui),
        );
        let text = crate::test_support::text_shapes(&output).map(|shape| shape.galley.job.text.clone()).collect::<Vec<_>>().join("\n");
        for key in [
            "render.environment.blur_pending",
            "render.environment.world_pending",
            "render.environment.medium",
            "render.environment.min",
            "render.environment.max",
        ] {
            assert!(text.contains(h.app.tr(key)), "missing {key}: {text}");
        }
        if method == ImageMethod::Raster {
            assert!(text.contains(h.app.tr("render.environment.source_raytrace")));
            assert!(text.contains(h.app.tr("render.output_raytrace_only")));
        } else {
            assert!(!text.contains(h.app.tr("render.environment.source_raytrace")), "CPU environment sources are supported");
            assert!(crate::scenario::rect_of("render:environment:source").is_some());
            assert!(text.contains(h.app.tr("render.cpu_volume_help")));
            assert!(text.contains(h.app.tr("render.cpu_volume_path_required")));
            assert!(crate::scenario::rect_of("render:volume-enable-path").is_some());
        }
        output.textures_delta.clear();
        let at = crate::scenario::rect_of("render:volume-enable").unwrap().center();
        for pressed in [true, false] {
            environment_editor_frame(
                &mut h,
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() },
                ],
            );
        }
        assert!(h.app.render_workspace.view.studio.volume.is_none());
        assert_eq!(h.app.render_workspace.view.method, method);
        assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    }
}

#[test]
fn environment_editor_gradient_and_atmosphere_refuse_hostile_values_atomically() {
    let mut h = environment_editor_fixture();
    let before = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    for value in [f64::NAN, f64::INFINITY, -1.0, 1e300] {
        let mut gradient = h.app.render_workspace.view.studio.backdrop;
        gradient.radius = value;
        assert!(environment::commit_gradient(&mut h.app, true, gradient).is_err());
        h.app.render_workspace.view.studio.volume =
            Some(solidcraft_engine::render::job::volume::HomogeneousVolume { extinction_per_mm: value, ..Default::default() });
        assert!(setups::save(&mut h.app).is_err());
        h.app.render_workspace.view.studio.volume = None;
    }
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    environment::commit_gradient(&mut h.app, true, Default::default()).unwrap();
    assert!(h.app.render_workspace.view.studio.backdrop_enabled);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc, before);
}

#[test]
fn environment_editor_stage_removal_requires_confirm_and_rejects_stale_answers() {
    for stale in [false, true] {
        let mut h = environment_editor_fixture();
        environment::commit_ground(&mut h.app, true, environment::Ground { margin: 1.0, gap: 0.1, color: [0.18; 3] }).unwrap();
        let before = h.app.session.doc.clone();
        environment::request_stage_removal(&h.app, &h.ctx);
        for _ in 0..3 {
            environment_editor_frame(&mut h, vec![]);
        }
        assert!(Arc::ptr_eq(&before, &h.app.session.doc));
        assert!(crate::scenario::rect_of("dialog:ok").is_some());
        if stale {
            h.app.run("render.settings.set", json!({"name":"Other","set":{"width":42}})).unwrap();
        }
        let undo = h.app.session.undo.len();
        let at = crate::scenario::rect_of("dialog:ok").unwrap().center();
        for pressed in [true, false] {
            environment_editor_frame(
                &mut h,
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() },
                ],
            );
        }
        assert_eq!(h.app.session.undo.len(), undo + usize::from(!stale));
        assert_eq!(h.app.render_workspace.view.draft.stage.is_empty(), !stale);
    }
}

#[test]
fn environment_editor_checked_file_embeds_bytes_and_keeps_the_recheck_banner() {
    let mut h = environment_editor_fixture();
    let path = h.dir.join("environment-editor.hdr");
    let bytes = original_hdr_bytes(128, true);
    std::fs::write(&path, &bytes).unwrap();
    h.app.render_workspace.view.studio.environment = studio::Environment::Hdr;
    hdr::inspect(&mut h.app, hdr::Target::Gpu, path.to_string_lossy().into_owned()).unwrap();
    let undo = h.app.session.undo.len();
    environment::persist(&mut h.app);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.doc.render.resources.len(), 1);
    assert!(matches!(h.app.render_workspace.view.studio.hdr, hdr::Input::FileCaptured { .. }));
    let mut output = h.ctx.run_ui(
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(308.0, 3000.0))), ..Default::default() },
        |ui| environment::editor(&mut h.app, ui),
    );
    assert!(crate::test_support::text_shapes(&output).any(|shape| shape.galley.job.text.contains(h.app.tr("render.environment.checked"))));
    output.textures_delta.clear();
    let before = h.app.session.doc.clone();
    std::fs::write(&path, original_hdr_bytes(64, true)).unwrap();
    environment::persist(&mut h.app);
    assert!(Arc::ptr_eq(&before, &h.app.session.doc), "changed bytes require rechecking");
    assert_eq!(h.app.session.undo.len(), undo + 1);
}

#[test]
fn environment_editor_medium_toggle_saves_one_command_without_a_gpu_host() {
    let mut h = environment_editor_fixture();
    let name = h.app.render_workspace.view.name.clone();
    super::inspector::activate_saved_shot(&mut h.app, name).unwrap();
    let saved_camera = h.app.session.doc.render.setups[&h.app.render_workspace.view.name].camera;
    h.app.cam.yaw += 0.2;
    setups::refresh_dirty(&mut h.app);
    assert!(h.app.render_workspace.view.dirty, "navigation is a real draft after activating the saved shot");
    let live_camera = h.app.cam;
    let before = h.app.session.doc.clone();
    environment_editor_frame(&mut h, vec![]);
    let at = crate::scenario::rect_of("render:volume-enable").unwrap().center();
    let undo = h.app.session.undo.len();
    for pressed in [true, false] {
        environment_editor_frame(
            &mut h,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() },
            ],
        );
    }
    assert!(h.app.render_workspace.view.studio.volume.is_some());
    assert_eq!(h.app.session.undo.len(), undo + 1);
    let mut expected = setups::snapshot(&h.app).unwrap();
    // Numerical atmosphere edits preserve the saved shot while navigation remains a draft.
    expected.camera = saved_camera;
    assert_eq!(h.app.session.doc.render.setups[&h.app.render_workspace.view.name], expected);
    assert_eq!(h.app.cam, live_camera);
    assert_eq!(h.app.render_workspace.view.clean_setup.as_ref().unwrap().camera, saved_camera);
    h.app.run("edit.undo", json!({})).unwrap();
    setups::load(&mut h.app);
    assert!(h.app.render_workspace.view.studio.volume.is_none());
    assert_eq!(h.app.session.doc, before, "Undo restores the exact pre-gesture document");
    assert_eq!(h.app.cam, live_camera);
}

#[test]
fn environment_ring_angles_snap_wrap_and_refuse_hostile_geometry() {
    let center = egui::pos2(100.0, 200.0);
    for (point, expected) in
        [(egui::pos2(150.0, 200.0), 0.0), (egui::pos2(100.0, 250.0), 90.0), (egui::pos2(50.0, 200.0), 180.0), (egui::pos2(100.0, 150.0), -90.0)]
    {
        assert_eq!(environment::pointer_angle(point, center, 50.0), Some(expected));
    }
    assert_eq!(environment::drag_rotation(170.0, 170.0, -170.0, false), Some(-170.0));
    assert_eq!(environment::drag_rotation(170.0, 0.0, 30.0, true), Some(-165.0));
    assert_eq!(environment::drag_rotation(-170.0, 0.0, -30.0, true), Some(165.0));
    assert_eq!(environment::drag_rotation(0.0, 0.0, 180.0, false), Some(-180.0));
    assert_eq!(environment::pointer_angle(center, center, 50.0), None);
    for radius in [0.0, -1.0, f32::NAN, f32::INFINITY, 1e9] {
        assert_eq!(environment::pointer_angle(egui::pos2(150.0, 200.0), center, radius), None);
    }
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(environment::pointer_angle(egui::pos2(value, 200.0), center, 50.0), None);
        assert_eq!(environment::pointer_angle(center, egui::pos2(100.0, value), 50.0), None);
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300] {
        assert_eq!(environment::drag_rotation(value, 0.0, 20.0, false), None);
        assert_eq!(environment::drag_rotation(0.0, value, 20.0, true), None);
        assert_eq!(environment::drag_rotation(0.0, 0.0, value, false), None);
    }
}

fn environment_ring_frame(h: &mut Harness, mods: egui::Modifiers, events: Vec<egui::Event>) {
    let mut output = h.ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
            focused: true,
            events: std::iter::once(egui::Event::ModifiersChanged(mods)).chain(events).collect(),
            ..Default::default()
        },
        |ui| {
            let viewport = ui.max_rect();
            show(&mut h.app, ui);
            environment_overlay(&mut h.app, ui, viewport);
        },
    );
    output.textures_delta.clear();
}

fn environment_ring_fixture() -> Harness {
    let mut h = environment_editor_fixture();
    enter(&mut h.app, true).unwrap();
    inspector::select_editor(&mut h.app, inspector::Editor::Environment);
    h.app.preferences.nav = crate::prefs::OPENSCAD_NAV.into();
    h.app.cam.fit(&solidcraft_engine::product_scene::extract(&h.app.session).unwrap().bounds);
    environment_ring_frame(&mut h, egui::Modifiers::NONE, vec![]);
    h
}

#[test]
fn render_workspace_frame_rect_fits_centres_and_refuses_hostile_sizes() {
    let viewport = egui::Rect::from_min_size(egui::pos2(100.0, 40.0), egui::vec2(800.0, 600.0));
    for aspect in [16.0 / 9.0, 1.5, 1.0, 0.8, 9.0 / 16.0, 0.01, 100.0] {
        let frame = shot::frame_rect(aspect, viewport).unwrap();
        assert!(viewport.contains_rect(frame));
        assert_eq!(frame.center(), viewport.center());
        assert!((f64::from(frame.width() / frame.height()) - aspect).abs() < 0.001);
        assert!(frame.width() == viewport.width() || frame.height() == viewport.height());
    }
    for aspect in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::MAX, 1e-300] {
        assert!(shot::frame_rect(aspect, viewport).is_none());
    }
    for size in [egui::Vec2::ZERO, egui::vec2(0.0, 400.0), egui::vec2(f32::NAN, 10.0), egui::vec2(f32::MAX, f32::MAX), egui::vec2(-10.0, 20.0)] {
        assert!(shot::frame_rect(1.5, egui::Rect::from_min_size(egui::Pos2::ZERO, size)).is_none());
    }
}

fn frame_editor_fixture() -> Harness {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1440,"height":1200}));
    h.app.run("render.settings.set", json!({"name":"Hero","set":{"width":800,"height":600,"camera":h.app.cam}})).unwrap();
    enter(&mut h.app, true).unwrap();
    h.app.run("view.render_select", json!({"kind":"shot","id":"Hero"})).unwrap();
    h.app.run("view.render_stage", json!({"stage":"frame"})).unwrap();
    h
}

#[test]
fn environment_ring_drag_is_one_undo_snaps_and_never_navigates_the_camera() {
    use egui::{Event, Modifiers, PointerButton};
    let mut h = environment_ring_fixture();
    environment::commit_environment(&mut h.app, json!({"rotation_deg":170})).unwrap();
    environment_ring_frame(&mut h, Modifiers::NONE, vec![]);
    let ring = h.app.render_workspace.view.environment_ring.geometry.unwrap();
    let at = |degrees: f32| ring.center + egui::vec2(degrees.to_radians().cos(), degrees.to_radians().sin()) * ring.radius;
    let press = at(0.0);
    let before = h.app.session.doc.clone();
    let camera = h.app.cam;
    let undo = h.app.session.undo.len();
    let selection = h.app.session.selection.clone();
    for pressed in [true, false] {
        environment_ring_frame(
            &mut h,
            Modifiers::SHIFT,
            vec![
                Event::PointerMoved(press),
                Event::PointerButton { pos: press, button: PointerButton::Primary, pressed, modifiers: Modifiers::SHIFT },
            ],
        );
    }
    assert!(Arc::ptr_eq(&before, &h.app.session.doc), "a Shift click without a drag does not snap or save");
    environment_ring_frame(
        &mut h,
        Modifiers::NONE,
        vec![
            Event::PointerMoved(press),
            Event::PointerButton { pos: press, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        ],
    );
    for angle in [10.0, 20.0, 30.0] {
        environment_ring_frame(&mut h, Modifiers::SHIFT, vec![Event::PointerMoved(at(angle))]);
        assert!(Arc::ptr_eq(&before, &h.app.session.doc), "drag is temporary");
        assert_eq!(h.app.session.undo.len(), undo);
        assert_eq!(h.app.cam, camera, "OpenSCAD left-drag must not orbit through the ring");
    }
    let end = at(30.0);
    environment_ring_frame(
        &mut h,
        Modifiers::SHIFT,
        vec![Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::SHIFT }],
    );
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!((h.app.render_workspace.view.studio.rotation_rad.to_degrees() + 165.0).abs() < 1e-6);
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.session.selection, selection);
    assert!(crate::scenario::rect_of("render:environment:ring-degrees").is_some());
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc, before);
}

#[test]
fn environment_ring_cancel_or_stale_revision_discards_the_drag() {
    use egui::{Event, Modifiers, PointerButton};
    for stale in [false, true] {
        let mut h = environment_ring_fixture();
        let ring = h.app.render_workspace.view.environment_ring.geometry.unwrap();
        let start = ring.center + egui::vec2(ring.radius, 0.0);
        let end = ring.center + egui::vec2(0.0, ring.radius);
        environment_ring_frame(
            &mut h,
            Modifiers::NONE,
            vec![
                Event::PointerMoved(start),
                Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
            ],
        );
        environment_ring_frame(&mut h, Modifiers::NONE, vec![Event::PointerMoved(end)]);
        if stale {
            h.app.run("render.settings.set", json!({"name":"Studio","set":{"width":640}})).unwrap();
        }
        let before = h.app.session.doc.clone();
        let undo = h.app.session.undo.len();
        let camera = h.app.cam;
        let cancel = if stale {
            vec![]
        } else {
            vec![Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]
        };
        environment_ring_frame(&mut h, Modifiers::NONE, cancel);
        environment_ring_frame(&mut h, Modifiers::NONE, vec![Event::PointerMoved(start)]);
        environment_ring_frame(
            &mut h,
            Modifiers::NONE,
            vec![Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }],
        );
        assert!(Arc::ptr_eq(&before, &h.app.session.doc));
        assert_eq!(h.app.session.undo.len(), undo);
        assert_eq!(h.app.cam, camera);
    }
}

#[test]
fn environment_ring_visibility_follows_editor_target_and_live_canvas() {
    let mut h = environment_ring_fixture();
    inspector::select_editor(&mut h.app, inspector::Editor::Lighting);
    h.app.render_workspace.view.selection = RenderSelection::Environment;
    environment_ring_frame(&mut h, egui::Modifiers::NONE, vec![]);
    assert!(h.app.render_workspace.view.environment_ring.geometry.is_some());
    h.app.render_workspace.view.selection = RenderSelection::Nothing;
    environment_ring_frame(&mut h, egui::Modifiers::NONE, vec![]);
    assert!(h.app.render_workspace.view.environment_ring.geometry.is_none());
    inspector::select_editor(&mut h.app, inspector::Editor::Environment);
    h.app.render_workspace.view.result = true;
    environment_ring_frame(&mut h, egui::Modifiers::NONE, vec![]);
    assert!(h.app.render_workspace.view.environment_ring.geometry.is_none());
}

#[test]
fn environment_editor_opening_pages_and_sources_preserves_design_and_history() {
    let mut h = environment_editor_fixture();
    let before = h.app.session.doc.clone();
    let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    {
        let page = inspector::Editor::Environment;
        inspector::select_editor(&mut h.app, page);
        environment_editor_frame(&mut h, vec![]);
        assert!(crate::scenario::rect_of("render:environment:rotation-slider").is_some());
        h.app.ui.show_browser = false;
        environment::open_sources(&mut h.app);
        assert!(h.app.ui.show_browser);
        assert_eq!(h.app.render_workspace.view.stage, Stage::Light);
        assert_eq!(inspector::selected_editor(&h.app), page);
        assert!(Arc::ptr_eq(&before, &h.app.session.doc));
        assert_eq!(history, (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()));
    }
}

#[test]
fn environment_editor_controls_commit_one_undo_each_and_refuse_hostile_edits() {
    let mut h = environment_editor_fixture();
    for request in [json!({"rotation_deg":215}), json!({"intensity":2.5})] {
        let before = h.app.session.doc.clone();
        let undo = h.app.session.undo.len();
        environment::commit_environment(&mut h.app, request).unwrap();
        assert_eq!(h.app.session.undo.len(), undo + 1);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc, before);
        h.app.run("edit.redo", json!({})).unwrap();
    }
    for (color, transparent) in [([40, 80, 120], false), ([40, 80, 120], true), ([40, 80, 120], false)] {
        let undo = h.app.session.undo.len();
        environment::commit_backdrop(&mut h.app, color, transparent).unwrap();
        assert_eq!(h.app.session.undo.len(), undo + 1);
    }
    let mut ground = environment::Ground { margin: 0.25, gap: 0.1, color: [0.18; 3] };
    for edit in 0..5 {
        match edit {
            1 => ground.margin = 1.0,
            2 => ground.gap = 0.25,
            3 => ground.color = [0.1, 0.3, 0.5],
            _ => {}
        }
        let undo = h.app.session.undo.len();
        environment::commit_ground(&mut h.app, edit != 4, ground.clone()).unwrap();
        assert_eq!(h.app.session.undo.len(), undo + 1);
    }
    let before = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    for request in [json!({"rotation_deg":1e300}), json!({"intensity":-1}), json!({"rotation_deg":"NaN"}), json!(null), json!([])] {
        assert!(environment::commit_environment(&mut h.app, request).is_err());
    }
    assert!(environment::commit_ground(&mut h.app, true, environment::Ground { gap: f64::NAN, ..ground }).is_err());
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
fn environment_editor_slider_drag_commits_only_on_release() {
    let mut h = environment_editor_fixture();
    environment_editor_frame(&mut h, vec![]);
    let rect = crate::scenario::rect_of("render:environment:rotation-slider").unwrap();
    let start = rect.center();
    let end = egui::pos2(rect.right() - 20.0, start.y);
    let undo = h.app.session.undo.len();
    environment_editor_frame(
        &mut h,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton { pos: start, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
        ],
    );
    environment_editor_frame(&mut h, vec![egui::Event::PointerMoved(end)]);
    assert_eq!(h.app.session.undo.len(), undo, "dragging is temporary");
    environment_editor_frame(
        &mut h,
        vec![egui::Event::PointerButton { pos: end, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }],
    );
    assert_eq!(h.app.session.undo.len(), undo + 1, "release saves one edit");
    environment_editor_frame(&mut h, vec![]);
    assert_eq!(h.app.session.undo.len(), undo + 1);
}

#[test]
fn environment_editor_hdr_header_reads_catalog_without_applying_or_saving() {
    let mut h = environment_editor_fixture();
    let name = h.app.render_workspace.view.name.clone();
    h.app.run("render.library.environment.apply", json!({"id":"HDR-001","name":name,"replace_environment":true})).unwrap();
    setups::load(&mut h.app);
    let before = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let mut output = h.ctx.run_ui(
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(305.0, 1600.0))), ..Default::default() },
        |ui| environment::editor(&mut h.app, ui),
    );
    let named = crate::test_support::text_shapes(&output).any(|text| text.galley.job.text.contains("Neutral side softbox"));
    output.textures_delta.clear();
    assert!(named);
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    for request in [json!({"rotation_deg":540}), json!({"intensity":0})] {
        let undo = h.app.session.undo.len();
        environment::commit_environment(&mut h.app, request).unwrap();
        assert_eq!(h.app.session.undo.len(), undo + 1);
    }
}

#[test]
fn render_workspace_frame_opening_editor_and_presets_never_mutates_or_starts_a_job() {
    let mut h = frame_editor_fixture();
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    let camera = h.app.cam;
    h.frames(4);
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(h.app.render_workspace.view.job.is_none());
    assert!(crate::scenario::rect_of("render:shot:name").is_some());
    assert!(crate::scenario::rect_of("render:frame:aspect:16:9").is_some());
    assert!(crate::scenario::rect_of("render:frame:overlay").is_some());
    assert_eq!(shot::selected(&h.app), Some("Hero"));
    h.step(&json!({"click":{"handle":"render:frame:thirds"}})).unwrap();
    assert!(h.app.render_workspace.view.frame.guides.thirds);
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.session.undo.len(), undo);
    let mut h = frame_editor_fixture();
    let camera = Camera { fov: 175f64.to_radians(), ..h.app.cam };
    inspector::apply_camera_draft(&mut h.app, camera).unwrap();
    h.frames(4);
    assert_eq!(h.app.cam, camera, "opening must preserve valid FOVs outside the slider's usual editing range");
}

#[test]
fn render_workspace_frame_guides_are_transient_and_overlay_visibility_is_contextual() {
    let mut h = frame_editor_fixture();
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    h.app.run("view.render_frame", json!({"aspect":"16:9","thirds":true,"safe_area":true,"centre_mark":true})).unwrap();
    assert!(shot::overlay_visible(&h.app));
    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
    let frame = shot::preview_rect(&h.app, viewport).unwrap();
    assert!((frame.width() / frame.height() - 16.0 / 9.0).abs() < 0.001);
    h.app.run("view.render_stage", json!({"stage":"look"})).unwrap();
    assert!(shot::overlay_visible(&h.app), "a stage change keeps the selected Shot");
    h.app.run("view.render_select", json!({"kind":"nothing"})).unwrap();
    assert!(!shot::overlay_visible(&h.app));
    h.app.run("view.render_select", json!({"kind":"shot","id":"Hero"})).unwrap();
    assert!(shot::overlay_visible(&h.app));
    h.app.render_workspace.view.result = true;
    assert!(!shot::overlay_visible(&h.app));
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.session.doc.render.setups["Hero"].width, 800);
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(h.app.render_workspace.view.frame.guides.thirds);
    assert!(h.app.render_workspace.view.frame.guides.safe_area);
    assert!(h.app.render_workspace.view.frame.guides.centre_mark);
}

#[test]
fn render_workspace_frame_exposure_drag_is_one_saved_edit_and_undoes() {
    let mut h = frame_editor_fixture();
    shot::edit_settings(&mut h.app, "Hero", json!({"job":{"method":"cpu_raytrace"}})).unwrap();
    h.call("ui.resize", json!({"width":1600,"height":1800}));
    h.frames(4);
    let track = crate::scenario::rect_of("dialog:slider_track:Exposure").unwrap();
    assert!(track.width() > 20.0, "exposure track: {track:?}");
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    let start = track.center();
    let end = egui::pos2(track.left() + track.width() * 0.75, track.center().y);
    h.step(&json!({"drag":[[start.x,start.y],[end.x,end.y]],"steps":8})).unwrap();
    assert_ne!(
        h.app.session.doc.render.setups["Hero"].exposure_ev, 0.0,
        "track: {track:?}; state: {:?}; status: {:?}",
        h.app.render_workspace.view.frame, h.app.status
    );
    assert_eq!(h.app.session.undo.len(), undo + 1);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before);
}

#[test]
fn render_workspace_scene_render_shot_captures_current_size_and_live_camera_without_saving() {
    let mut h = frame_editor_fixture();
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5})).unwrap();
    shot::edit_settings(&mut h.app, "Hero", json!({"width":16,"height":12})).unwrap();
    h.app.render_workspace.image_host = Some(solidcraft_engine::render::job::CpuHost::cooperative());
    sync_backend(&mut h.app);
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    let camera = Camera { distance: 85.0, fov: 35f64.to_radians(), ..h.app.cam };
    inspector::apply_camera_draft(&mut h.app, camera).unwrap();
    h.app.run("view.render_frame", json!({"thirds":true,"safe_area":true,"centre_mark":true})).unwrap();
    h.app.render_workspace.view.name = "Hero".into();
    setups::load(&mut h.app);
    ribbon::perform_job(&mut h.app, "render.start").unwrap();
    let job = h.app.render_workspace.view.job.unwrap();
    let handle = h.app.session.render_job_handle(job).unwrap();
    assert_eq!(h.app.render_workspace.view.job_shot.as_deref(), Some("Hero"));
    let host = h.app.session.render_job_host().unwrap().clone();
    for _ in 0..128 {
        host.pump(16384).unwrap();
        if handle.frame().unwrap().is_some() {
            break;
        }
    }
    let frame = handle.frame().unwrap().unwrap();
    assert_eq!((frame.info.width, frame.info.height), (16, 12));
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
fn render_workspace_frame_aspect_click_edits_only_selected_shot_size_once_and_undoes() {
    let mut h = frame_editor_fixture();
    h.frames(4);
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":"render:frame:aspect:16:9"}})).unwrap();
    let saved = &h.app.session.doc.render.setups["Hero"];
    assert_eq!((saved.width, saved.height), (1920, 1080));
    assert_eq!(h.app.session.undo.len(), undo + 1);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before);
    for (index, expected) in [(1, (1620, 1080)), (2, (1080, 1080)), (3, (864, 1080)), (4, (1080, 1920))] {
        let undo = h.app.session.undo.len();
        shot::choose_aspect(&mut h.app, "Hero", index).unwrap();
        let saved = &h.app.session.doc.render.setups["Hero"];
        assert_eq!((saved.width, saved.height), expected);
        assert_eq!(h.app.session.undo.len(), undo + 1);
    }
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    shot::choose_aspect(&mut h.app, "Hero", 5).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(shot::choose_aspect(&mut h.app, "Hero", usize::MAX).is_err());
}

#[test]
fn render_workspace_frame_shot_edits_revert_duplicate_and_rename_use_existing_commands() {
    let mut h = frame_editor_fixture();
    let before = h.app.session.doc.render.setups["Hero"].clone();
    let camera = Camera { distance: before.camera.distance * 2.0, ..before.camera };
    inspector::apply_camera_draft(&mut h.app, camera).unwrap();
    let undo = h.app.session.undo.len();
    assert!(shots::camera_modified(&h.app, "Hero"));
    shot::edit_settings(&mut h.app, "Hero", json!({"camera":camera})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(!shots::camera_modified(&h.app, "Hero"));
    h.app.run("edit.undo", json!({})).unwrap();
    shot::revert(&mut h.app, "Hero").unwrap();
    assert_eq!(h.app.cam, before.camera);
    shot::edit_settings(&mut h.app, "Hero", json!({"exposure_ev":1.5})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    shot::duplicate(&mut h.app, "Hero").unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 2);
    let copied = h.app.session.doc.render.setups.iter().find(|(name, _)| name.as_str() != "Hero").unwrap().1;
    assert_eq!(copied, &h.app.session.doc.render.setups["Hero"]);
    shot::rename(&mut h.app, "Hero", "Renamed").unwrap();
    assert_eq!(shot::selected(&h.app), Some("Renamed"));
    assert_eq!(h.app.session.undo.len(), undo + 3);
    let before = h.app.session.doc.to_json();
    for patch in [json!({"width":0}), json!({"width":u32::MAX}), json!({"height":1e300}), json!({"exposure_ev":33}), json!({"exposure_ev":null})] {
        assert!(shot::edit_settings(&mut h.app, "Renamed", patch).is_err());
        assert_eq!(h.app.session.doc.to_json(), before);
    }
}

fn render_lights_fixture() -> Harness {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1440,"height":1400}));
    h.app
        .run(
            "render.settings.set",
            json!({"name":"Studio","set":{"job":{"method":"cpu_raytrace","options":{"lights":[
                {"direction":[0.6,0,0.8],"color":[1,0.5,0.25],"intensity":3},
                {"direction":[-0.8,0,0.6],"color":[0.25,0.5,1],"intensity":2}
            ]}}}}),
        )
        .unwrap();
    enter(&mut h.app, true).unwrap();
    setups::load(&mut h.app);
    h.app.pending_fit = None;
    h
}

#[test]
fn render_lights_solo_and_mute_filter_preview_and_render_copies_without_saving_or_history() {
    let mut h = render_lights_fixture();
    let original = setups::snapshot(&h.app).unwrap();
    let document = h.app.session.doc.clone();
    let json = document.to_json();
    let before = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision, h.app.session.is_dirty());
    h.app.run("render.lights.solo", json!({"id":"directional:1","solo":true})).unwrap();
    assert_eq!(light::preview(&h.app).unwrap().len(), 1);
    let request = image_request(&h.app).unwrap();
    assert_eq!(request["settings"]["job"]["options"]["lights"][0]["intensity"], 0.0);
    assert_eq!(request["settings"]["job"]["options"]["lights"][1]["intensity"], 2.0);
    h.app.run("render.lights.mute", json!({"id":"directional:1","mute":true})).unwrap();
    assert!(light::preview(&h.app).unwrap().is_empty());
    assert_eq!(image_request(&h.app).unwrap()["settings"]["job"]["options"]["lights"][1]["intensity"], 0.0);
    assert_eq!(setups::snapshot(&h.app).unwrap(), original);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(h.app.session.doc.to_json(), json);
    assert_eq!(before, (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision, h.app.session.is_dirty()));
    let visibility = h.app.render_workspace.view.lights.visibility.clone();
    assert!(h.app.run("render.lights.mute", json!({"id":"area:0","mute":true})).is_err());
    assert_eq!(h.app.render_workspace.view.lights.visibility, visibility);
    crate::documents::open_tab(&mut h.app, Session::default()).unwrap();
    assert!(!h.app.render_workspace.view.lights.visibility.active());
}

#[test]
fn render_lights_editor_opening_is_read_only_and_gpu_point_conversion_is_one_undoable_edit() {
    let mut h = render_lights_fixture();
    h.app
        .run(
            "render.settings.set",
            json!({"name":"Studio","set":{"job":{"method":"gpu_path","options":{"lighting":{"area_lights":[
                {"center":[0,0,100],"u":[1000000,0,0],"v":[0,-1000000,0],"radiance":[0,0,0]}
            ]}}}}}),
        )
        .unwrap();
    setups::load(&mut h.app);
    h.app.run("view.render_select", json!({"kind":"light","id":"area:0"})).unwrap();
    let document = h.app.session.doc.clone();
    let before = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision, h.app.session.is_dirty());
    h.frames(3);
    assert!(crate::scenario::rect_of("render:light:editor").is_some());
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(before, (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision, h.app.session.is_dirty()));
    assert!(h.app.render_workspace.view.job.is_none());
    assert!(h.app.dialog.is_none());
    assert_eq!(light::rows(&h.app).unwrap()[0].geometry.kind(), "area");
    let camera = h.app.cam;
    h.step(&json!({"click":{"handle":"render:light:type:point"}})).unwrap();
    assert_eq!(h.app.session.undo.len(), before.0 + 1);
    assert_eq!(h.app.session.doc.features, document.features);
    assert_eq!(h.app.session.doc.materials, document.materials);
    assert_eq!(h.app.cam, camera);
    assert_eq!(light::rows(&h.app).unwrap()[0].geometry.kind(), "point");
    assert!(h.app.render_workspace.view.job.is_none());
    assert!(h.app.dialog.is_none());
    h.app.run("edit.undo", json!({})).unwrap();
    h.frames(3);
    assert_eq!(*h.app.session.doc, *document);
    assert_eq!(h.app.session.undo.len(), before.0);
    assert!(h.app.render_workspace.view.job.is_none());
    assert!(h.app.dialog.is_none());
    assert_eq!(light::rows(&h.app).unwrap()[0].geometry.kind(), "area");
}

#[test]
fn render_lights_edit_commands_refresh_selected_editor_and_keep_visibility_separate_from_undo() {
    let mut h = render_lights_fixture();
    h.app.run("render.lights.mute", json!({"id":"directional:0","mute":true})).unwrap();
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    light::edit(&mut h.app, "render.lights.set", Some("directional:0"), Some(json!({"intensity_ev":2}))).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Light("directional:0".into()));
    assert_eq!(light::rows(&h.app).unwrap()[0].intensity, 4.0);
    assert!(!h.app.render_workspace.view.lights.visibility.visible("directional:0"));
    h.app.run("edit.undo", json!({})).unwrap();
    setups::load(&mut h.app);
    assert_eq!(h.app.session.doc, document);
    assert!(!h.app.render_workspace.view.lights.visibility.visible("directional:0"));
    assert_eq!(light::rows(&h.app).unwrap()[0].intensity, 3.0);
    light::edit(&mut h.app, "render.lights.duplicate", Some("directional:0"), None).unwrap();
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Light("directional:2".into()));
    light::edit(&mut h.app, "render.lights.remove", Some("directional:2"), None).unwrap();
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Nothing);
}

#[test]
fn render_lights_fixed_studio_requests_carry_transient_weights_without_saving_them() {
    let mut h = Harness::new();
    enter(&mut h.app, true).unwrap();
    let document = h.app.session.doc.clone();
    let history = h.app.session.undo.len();
    h.app.run("render.lights.solo", json!({"id":"directional:1","solo":true})).unwrap();
    let raster = image_request(&h.app).unwrap();
    assert_eq!(raster["studio_weights"], json!([0.0, 1.0, 0.0]));
    h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
    assert_eq!(image_request(&h.app).unwrap()["studio_weights"], json!([0.0, 1.0, 0.0]));
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), history);
    assert!(serde_json::to_value(setups::snapshot(&h.app).unwrap()).unwrap().get("studio_weights").is_none());
}

#[test]
fn render_lights_list_inspects_unsaved_ui_draft_without_applying_or_opening_anything() {
    let mut h = Harness::new();
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.method = ImageMethod::CpuRaytrace;
    h.app.render_workspace.view.raytrace.lights =
        Some(vec![solidcraft_engine::render::job::DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0, 0.5, 0.25], intensity: 7.0 }]);
    h.app.run("view.render_select", json!({"kind":"environment"})).unwrap();
    let document = h.app.session.doc.clone();
    let before = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision, h.app.session.is_dirty());
    let camera = h.app.cam;
    let page = inspector::selected_editor(&h.app);
    let snapshot = setups::snapshot(&h.app).unwrap();
    let reply = h.app.run("render.lights.list", json!({"settings":snapshot})).unwrap();
    assert_eq!(reply["lights"].as_array().unwrap().len(), 1);
    assert_eq!(reply["lights"][0]["id"], "directional:0");
    assert_eq!(reply["lights"][0]["name"], "Key");
    assert_eq!(reply["lights"][0]["intensity"], 7.0);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(before, (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision, h.app.session.is_dirty()));
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Environment);
    assert_eq!(inspector::selected_editor(&h.app), page);
    assert_eq!(setups::snapshot(&h.app).unwrap(), snapshot);
    assert!(h.app.render_workspace.view.job.is_none());
    assert!(h.app.dialog.is_none() && h.app.tool.is_none());
}

#[cfg(not(target_arch = "wasm32"))]
mod async_path_host {
    use super::*;
    use solidcraft_engine::render::job;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// Advertise the path capability without opening a device or rendering anything.
    struct TestExecutor;
    impl job::ImageExecutor for TestExecutor {
        fn descriptor(&self) -> job::Result<ExecutorDescriptor> {
            let mut descriptor = job::CpuHost::cooperative().descriptor()?;
            descriptor.id = ExecutorId::new("test-path").unwrap();
            descriptor.profiles[0].features.push(solidcraft_engine::render::RenderFeature::Progressive);
            Ok(descriptor)
        }
        fn admit(&self, _: &ProductScene, _: &job::ImageRequest, _: job::JobOptions) -> job::Result<job::Admission> {
            panic!("setup tests must not render")
        }
        fn begin(&self, _: ProductScene, _: job::ImageRequest, _: job::JobOptions, _: job::CancelToken) -> job::Result<Box<dyn job::ImageWork>> {
            panic!("setup tests must not render")
        }
    }

    fn panel_fixture() -> Harness {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1600,"height":1600}));
        enter(&mut h.app, true).unwrap();
        inspector::select_editor(&mut h.app, inspector::Editor::Output);
        h.frames(3);
        h
    }

    fn finish_setup(h: &mut Harness) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while matches!(h.app.render_workspace.path_host, PathHost::Preparing(..)) {
            assert!(Instant::now() < deadline, "test setup worker did not finish");
            h.frames(1);
            std::thread::sleep(Duration::from_millis(1));
        }
        h.frames(3);
    }

    /// Choose the GPU path image method, which is what starts the deferred setup.
    fn choose_gpu_path(h: &mut Harness) {
        select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
        h.frames(1);
    }

    /// Entering Render and moving its view with the Raster (or CPU) image method never compiles
    /// the GPU path tracer: on Vulkan drivers that compile takes gigabytes (#9041). Choosing the
    /// GPU path method starts it, once.
    #[test]
    fn render_with_another_image_method_never_starts_the_path_setup() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1200,"height":900}));
        h.app.render_workspace.defer_path_host("selected viewport".into(), move || {
            observed.fetch_add(1, Ordering::SeqCst);
            job::ImageHost::with_executor(Arc::new(TestExecutor)).map_err(|e| e.to_string())
        });
        enter(&mut h.app, true).unwrap();
        assert_eq!(h.app.render_workspace.view.method, ImageMethod::Raster);
        for step in 0..20 {
            h.call("ui.camera", json!({"yaw": f64::from(step) * 0.1, "pitch": 0.3}));
            h.frames(1);
        }
        select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
        h.frames(5);
        enter(&mut h.app, false).unwrap();
        enter(&mut h.app, true).unwrap();
        h.frames(5);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(matches!(h.app.render_workspace.path_host, PathHost::NotStarted));
        assert!(h.app.render_workspace.pending_setup.is_some());
        choose_gpu_path(&mut h);
        finish_setup(&mut h);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(matches!(h.app.render_workspace.path_host, PathHost::Ready(_)));
        // The GPU method chosen in Design (a loaded setup) waits for Render.
        let mut h = Harness::new();
        h.app.render_workspace.defer_path_host("selected viewport".into(), || Err("not opened".into()));
        select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
        h.frames(5);
        assert!(matches!(h.app.render_workspace.path_host, PathHost::NotStarted));
    }

    #[test]
    fn design_frames_and_document_reset_never_start_the_deferred_path_host() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let mut h = Harness::new();
        h.app.render_workspace.defer_path_host("selected viewport".into(), move || {
            observed.fetch_add(1, Ordering::SeqCst);
            job::ImageHost::with_executor(Arc::new(TestExecutor)).map_err(|e| e.to_string())
        });
        h.frames(5);
        crate::documents::adopt(&mut h.app, Session::default());
        h.frames(5);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(matches!(h.app.render_workspace.path_host, PathHost::NotStarted));
        assert!(h.app.render_workspace.pending_setup.is_some());
        enter(&mut h.app, true).unwrap();
        choose_gpu_path(&mut h);
        finish_setup(&mut h);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(matches!(h.app.render_workspace.path_host, PathHost::Ready(_)));
        enter(&mut h.app, false).unwrap();
        enter(&mut h.app, true).unwrap();
        h.frames(5);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn first_render_before_adapter_install_starts_once_when_the_adapter_arrives() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let mut h = panel_fixture();
        assert!(matches!(h.app.render_workspace.path_host, PathHost::NotStarted));
        h.app.render_workspace.defer_path_host("late selected viewport".into(), move || {
            observed.fetch_add(1, Ordering::SeqCst);
            Err("deliberate device refusal".into())
        });
        sync_backend(&mut h.app);
        assert!(matches!(h.app.render_workspace.path_host, PathHost::NotStarted));
        choose_gpu_path(&mut h);
        finish_setup(&mut h);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(matches!(h.app.render_workspace.path_host, PathHost::Failed(_)));
        h.app.render_workspace.defer_path_host("must not retry".into(), || panic!("failed setup retried"));
        enter(&mut h.app, false).unwrap();
        enter(&mut h.app, true).unwrap();
        h.frames(5);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_fallback_viewport_replaces_an_unstarted_factory_without_opening_the_old_device() {
        let mut h = Harness::new();
        h.app.render_workspace.defer_path_host("old hardware adapter".into(), || panic!("superseded device opened"));
        h.app.render_workspace.defer_path_host("selected safe-mode adapter".into(), || Err("selected adapter unavailable".into()));
        h.frames(5);
        assert_eq!(h.app.render_workspace.setup_key.as_deref(), Some("selected safe-mode adapter"));
        enter(&mut h.app, true).unwrap();
        choose_gpu_path(&mut h);
        finish_setup(&mut h);
        let PathHost::Failed(message) = &h.app.render_workspace.path_host else { panic!("expected selected adapter result") };
        assert_eq!(message, "selected adapter unavailable");
    }

    #[test]
    fn preparing_keeps_frames_responsive_then_registers_ready_host() {
        let mut h = panel_fixture();
        let cpu_host = h.app.session.render_job_host().unwrap().clone();
        let (release, wait) = mpsc::channel();
        let (started, thread) = mpsc::channel();
        h.app.render_workspace.path_host.start(move || {
            started.send(std::thread::current().id()).unwrap();
            wait.recv_timeout(Duration::from_secs(10)).unwrap();
            job::ImageHost::with_executor(Arc::new(TestExecutor)).map_err(|e| e.to_string())
        });
        assert_ne!(thread.recv_timeout(Duration::from_secs(10)).unwrap(), std::thread::current().id());
        // A second viewport installation cannot replace or duplicate setup.
        h.app.render_workspace.path_host.start(|| panic!("setup started twice"));
        h.frames(3);
        assert!(h.widget("GPU path tracer: compiling shaders…").is_some());
        assert!(image_host(&h.app, ImageMethod::GpuRaytrace).is_err());
        assert!(!h.app.session.render_backend().executors.iter().any(|e| e.id.as_str() == "test-path"));
        let transparent = h.app.tr("render.image_transparent").to_string();
        h.step(&json!({"widget":transparent})).unwrap();
        assert!(h.app.render_workspace.view.transparent);
        // Replacing the current document must retain the receiver while setup waits.
        crate::documents::adopt(&mut h.app, Session::default());
        assert!(matches!(h.app.render_workspace.path_host, PathHost::Preparing(..)));
        enter(&mut h.app, true).unwrap();
        inspector::select_editor(&mut h.app, inspector::Editor::Output);
        h.frames(3);
        assert!(h.widget("GPU path tracer: compiling shaders…").is_some());
        enter(&mut h.app, false).unwrap();
        release.send(()).unwrap();
        finish_setup(&mut h);
        assert!(matches!(h.app.render_workspace.path_host, PathHost::Ready(_)));
        enter(&mut h.app, true).unwrap();
        inspector::select_editor(&mut h.app, inspector::Editor::Output);
        h.frames(3);
        assert!(h.widget("GPU path tracer: compiling shaders…").is_none());
        assert!(image_host(&h.app, ImageMethod::GpuRaytrace).is_ok());
        assert_eq!(h.app.session.render_backend().executors.iter().filter(|e| e.id.as_str() == "test-path").count(), 1);
        assert!(h.app.session.render_job_host().unwrap().same_host(&cpu_host));
        h.step(&json!({"click":{"handle":"render:method"}})).unwrap();
        h.step(&json!({"click":{"handle":"render:method-gpu"}})).unwrap();
        assert_eq!(h.app.render_workspace.view.method, ImageMethod::GpuRaytrace);
        crate::documents::adopt(&mut h.app, Session::default());
        assert!(matches!(h.app.render_workspace.path_host, PathHost::Ready(_)));
        assert_eq!(h.app.session.render_backend().executors.iter().filter(|e| e.id.as_str() == "test-path").count(), 1);
    }

    /// The first setup on an adapter shows a spinner, the time so far and a note; a later one
    /// a bar estimated from the time the first took, which the preferences keep.
    #[test]
    fn setup_progress_is_estimated_from_the_last_setup_on_the_adapter() {
        let mut h = panel_fixture();
        h.app.render_workspace.setup_key = Some("Test GPU | Vulkan | 1.0".into());
        let (release, wait) = mpsc::channel::<()>();
        h.app.render_workspace.path_host.start(move || {
            wait.recv_timeout(Duration::from_secs(10)).unwrap();
            job::ImageHost::with_executor(Arc::new(TestExecutor)).map_err(|e| e.to_string())
        });
        h.frames(3);
        let first = h.app.tr("render.gpu_path_first_setup").to_string();
        assert!(h.widget("GPU path tracer: compiling shaders…").is_some());
        assert!(h.widget(&first).is_some(), "the first setup explains the wait");
        assert!(h.widget("0:00").is_some(), "and counts the time");
        release.send(()).unwrap();
        finish_setup(&mut h);
        let took = h.app.render_workspace.setup_seconds.get("Test GPU | Vulkan | 1.0").copied();
        assert!(took.is_some_and(|s| (0.0..10.0).contains(&s)), "{took:?}");

        // The next start on the same adapter has an estimate, kept in the preferences.
        let mut h = panel_fixture();
        h.app.load_prefs(&json!({"gpu_path_setup_seconds": {"Test GPU | Vulkan | 1.0": 120.0}}).to_string());
        h.app.render_workspace.setup_key = Some("Test GPU | Vulkan | 1.0".into());
        let (release, wait) = mpsc::channel::<()>();
        h.app.render_workspace.path_host.start(move || {
            wait.recv_timeout(Duration::from_secs(10)).unwrap();
            job::ImageHost::with_executor(Arc::new(TestExecutor)).map_err(|e| e.to_string())
        });
        h.frames(3);
        assert!(h.widget(&first).is_none());
        assert!(h.widget("0:00 / ~2:00").is_some(), "a bar with the estimate");
        release.send(()).unwrap();
        finish_setup(&mut h);
        let prefs: serde_json::Value = serde_json::from_str(&h.app.prefs()).unwrap();
        assert!(prefs["gpu_path_setup_seconds"]["Test GPU | Vulkan | 1.0"].as_f64().is_some_and(|s| s < 10.0));
    }

    #[test]
    fn hostile_setup_times_are_ignored() {
        let times = load_setup_seconds(&json!({"a": -1.0, "b": 1e300, "c": "slow", "d": 30.0}));
        assert_eq!(times.into_iter().collect::<Vec<_>>(), [("d".to_string(), 30.0)]);
        let many: serde_json::Map<String, serde_json::Value> = (0..100).map(|i| (format!("gpu {i}"), json!(1.0))).collect();
        assert_eq!(load_setup_seconds(&serde_json::Value::Object(many)).len(), MAX_SETUP_TIMES);
    }

    #[test]
    fn setup_error_is_visible_and_survives_document_reset() {
        let mut h = panel_fixture();
        h.app.render_workspace.path_host.start(|| Err("test driver refused the device".into()));
        finish_setup(&mut h);
        let message = "GPU path tracer unavailable: test driver refused the device";
        assert!(h.widget(message).is_some());
        assert!(h.widget("GPU path tracer: compiling shaders…").is_none());
        assert!(image_host(&h.app, ImageMethod::GpuRaytrace).is_err());
        assert!(h.app.session.render_job_host().is_some());
        completed_preview(&mut h, [200, 210, 220]);
        crate::documents::adopt(&mut h.app, Session::default());
        enter(&mut h.app, true).unwrap();
        inspector::select_editor(&mut h.app, inspector::Editor::Output);
        h.frames(3);
        assert!(h.widget(message).is_some());
        h.app.render_workspace.path_host.start(|| panic!("failed setup retried on viewport install"));
        assert!(matches!(h.app.render_workspace.path_host, PathHost::Failed(_)));
    }

    #[test]
    fn disconnected_setup_is_reported_in_the_panel() {
        let mut h = panel_fixture();
        let (tx, rx) = mpsc::channel();
        h.app.render_workspace.path_host = PathHost::Preparing(rx, Instant::now());
        drop(tx);
        h.frames(3);
        assert!(h.widget("GPU path tracer unavailable: setup worker stopped without returning a host").is_some());
    }

    #[test]
    fn panicking_setup_is_reported_in_the_panel() {
        let mut h = panel_fixture();
        h.app.render_workspace.path_host.start(|| panic!("simulated driver panic"));
        finish_setup(&mut h);
        assert!(h.widget("GPU path tracer unavailable: GPU path tracer setup panicked").is_some());
    }

    #[test]
    fn registration_failure_is_reported_in_the_panel() {
        let mut h = panel_fixture();
        // A different host with the same CPU descriptor cannot be registered twice.
        h.app.render_workspace.path_host.start(|| Ok(job::CpuHost::cooperative()));
        finish_setup(&mut h);
        let PathHost::Failed(message) = &h.app.render_workspace.path_host else { panic!("duplicate executor must fail") };
        assert!(h.widget(&format!("GPU path tracer unavailable: {message}")).is_some());
        assert!(image_host(&h.app, ImageMethod::GpuRaytrace).is_err());
    }

    fn marker_adapter_in(path: &std::path::Path) -> Option<String> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str::<serde_json::Value>(&text).ok()?["adapter"].as_str().map(str::to_owned)
    }

    /// A driver that takes the process down while it compiles the path tracer (#9236) can't be
    /// caught: the marker it leaves keeps the next launch from running that setup by itself.
    #[test]
    fn a_setup_that_stopped_the_app_is_not_run_again_until_asked() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let mut h = panel_fixture();
        let marker = h.dir.join(SETUP_MARKER);
        let key = "AMD Radeon RX 7600 (RADV NAVI33) | Vulkan | radv Mesa 25.2.8";
        std::fs::write(&marker, json!({"adapter": key}).to_string()).unwrap();
        h.app.render_workspace.watch_setup(Some(marker.clone()));
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        h.app.render_workspace.defer_path_host(key.into(), move || {
            observed.fetch_add(1, Ordering::SeqCst);
            job::ImageHost::with_executor(Arc::new(TestExecutor)).map_err(|e| e.to_string())
        });
        h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
        sync_backend(&mut h.app);
        h.frames(5);
        assert_eq!(calls.load(Ordering::SeqCst), 0, "the setup that stopped the app must not run by itself");
        let PathHost::Failed(message) = &h.app.render_workspace.path_host else { panic!("expected the crash to be reported") };
        assert_eq!(message, h.app.tr("render.gpu_path_crashed"));
        assert!(h.widget(&format!("GPU path tracer unavailable: {message}")).is_some());
        assert!(image_host(&h.app, ImageMethod::GpuRaytrace).is_err());
        // The other image methods stay available.
        assert!(h.app.session.render_job_host().is_some());
        assert_eq!(marker_adapter_in(&marker).as_deref(), Some(key), "the marker stays until the setup answers");

        // Opening another design keeps both the crash and the offer to try again.
        crate::documents::adopt(&mut h.app, Session::default());
        enter(&mut h.app, true).unwrap();
        inspector::select_editor(&mut h.app, inspector::Editor::Output);
        h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
        h.frames(3);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(h.app.render_workspace.setup_crashed());
        let retry = h.app.tr("render.gpu_path_retry").to_string();
        assert!(h.widget(&retry).is_some(), "the panel offers to try again");
        h.step(&json!({"widget": retry})).unwrap();
        finish_setup(&mut h);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(matches!(h.app.render_workspace.path_host, PathHost::Ready(_)));
        assert!(!marker.exists(), "a setup that answered removes its marker");
        assert!(!h.app.render_workspace.setup_crashed());
    }

    #[test]
    fn a_running_setup_keeps_a_marker_until_it_answers() {
        for answer in [true, false] {
            let mut h = panel_fixture();
            let marker = h.dir.join("settings").join(SETUP_MARKER);
            h.app.render_workspace.watch_setup(Some(marker.clone()));
            let (release, wait) = mpsc::channel::<()>();
            h.app.render_workspace.defer_path_host("Test GPU | Vulkan | 1.0".into(), move || {
                wait.recv_timeout(Duration::from_secs(10)).unwrap();
                if answer { job::ImageHost::with_executor(Arc::new(TestExecutor)).map_err(|e| e.to_string()) } else { Err("refused".into()) }
            });
            // Raster does not ask for the setup (#9041), so nothing runs and no marker is kept.
            sync_backend(&mut h.app);
            assert!(matches!(h.app.render_workspace.path_host, PathHost::NotStarted));
            assert!(!marker.exists());
            h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
            sync_backend(&mut h.app);
            assert!(matches!(h.app.render_workspace.path_host, PathHost::Preparing(..)));
            assert_eq!(marker_adapter_in(&marker).as_deref(), Some("Test GPU | Vulkan | 1.0"));
            release.send(()).unwrap();
            finish_setup(&mut h);
            assert_eq!(matches!(h.app.render_workspace.path_host, PathHost::Ready(_)), answer);
            assert!(!marker.exists(), "answer {answer}: the process survived, so the marker goes");
        }
    }

    #[test]
    fn closing_during_setup_is_not_a_crash() {
        let mut h = panel_fixture();
        let marker = h.dir.join(SETUP_MARKER);
        h.app.render_workspace.watch_setup(Some(marker.clone()));
        let (release, wait) = mpsc::channel::<()>();
        h.app.render_workspace.defer_path_host("Test GPU | Vulkan | 1.0".into(), move || {
            let _ = wait.recv_timeout(Duration::from_secs(10));
            Err("closed".into())
        });
        h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
        sync_backend(&mut h.app);
        assert!(marker.exists());
        close(&mut h.app);
        assert!(!marker.exists());
        release.send(()).unwrap();
    }

    #[test]
    fn markers_for_other_adapters_or_unreadable_ones_never_block_the_setup() {
        let other = || json!({"adapter": "Another GPU | Vulkan | 1.0"}).to_string().into_bytes();
        let hostile: [Vec<u8>; 6] = [
            other(),
            b"\xff\xfe not text".to_vec(),
            b"{\"adapter\": 42}".to_vec(),
            b"{\"adapter\": \"\"}".to_vec(),
            b"[\"Test GPU | Vulkan | 1.0\"]".to_vec(),
            format!("{{\"adapter\": \"Test GPU | Vulkan | 1.0\", \"pad\": \"{}\"}}", "x".repeat(70_000)).into_bytes(),
        ];
        for bytes in hostile {
            let mut h = panel_fixture();
            let marker = h.dir.join(SETUP_MARKER);
            std::fs::write(&marker, &bytes).unwrap();
            h.app.render_workspace.watch_setup(Some(marker.clone()));
            h.app.render_workspace.defer_path_host("Test GPU | Vulkan | 1.0".into(), || {
                job::ImageHost::with_executor(Arc::new(TestExecutor)).map_err(|e| e.to_string())
            });
            h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
            sync_backend(&mut h.app);
            finish_setup(&mut h);
            assert!(matches!(h.app.render_workspace.path_host, PathHost::Ready(_)), "{:?}", String::from_utf8_lossy(&bytes[..bytes.len().min(40)]));
            assert!(!marker.exists());
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn opaque_pbr_product_gpu_and_cpu_share_color_and_material_semantics() {
    use solidcraft_engine::render::{
        StandardView,
        job::{CancelToken, ImageRequest, ImageTask, TaskPoll},
    };
    let mut s = Session::default();
    s.execute("solid.sphere", &json!({"radius":10,"body_name":"Original sphere"})).unwrap();
    const SIZE: u32 = 256;
    let mut previous = None;
    for (metallic, roughness) in [(0.0, 0.2), (0.0, 0.65), (1.0, 0.2), (1.0, 0.65)] {
        s.execute(
            "appearance.assign",
            &json!({"bodies":["Original sphere"],"color":"#b47846",
            "pbr":{"metallic":metallic,"roughness":roughness,"dielectric_ior":1.5}}),
        )
        .unwrap();
        let scene = solidcraft_engine::product_scene::extract(&s).unwrap();
        let (key, gpu) = gpu_scene(&scene).unwrap();
        assert!(!gpu.pbr.is_empty() && gpu.tris.is_empty());
        assert_ne!(previous, Some(key));
        previous = Some(key);
        let mut camera = Camera { fov: 0.0, ..Default::default() };
        camera.set_view(StandardView::Iso);
        camera.fit(&scene.bounds);
        let radius = scene.bounds.diagonal().max(10.0) + scene.bounds.center().dist(camera.target);
        let eye = camera.eye().to_f32();
        let mut task = ImageTask::new(
            scene,
            ImageRequest {
                studio_weights: None,
                environment: None,
                textures: None,
                camera,
                width: SIZE,
                height: SIZE,
                background: [255; 3],
                transparent_background: false,
                integrator: Default::default(),
                raytrace: None,
            },
            CancelToken::default(),
        )
        .unwrap();
        let mut png = None;
        for _ in 0..100_000 {
            if let TaskPoll::Complete(bytes) = task.step(4096).unwrap() {
                png = Some(bytes);
                break;
            }
        }
        let cpu = image::load_from_memory(&png.expect("bounded original fixture completes")).unwrap().to_rgba8();
        assert_eq!(cpu.dimensions(), (SIZE, SIZE));
        let cpu = cpu.into_raw();
        for srgb in [false, true] {
            let Some(gpu) = crate::gpu::render_product_offscreen(
                gpu.clone(),
                None,
                camera.view_proj(1.0, radius).to_f32(),
                camera.back().to_f32(),
                [eye[0], eye[1], eye[2], 0.0],
                SIZE,
                srgb,
            ) else {
                eprintln!("skipping: no GPU adapter");
                return;
            };
            let at = |pixels: &[u8], x: usize, y: usize| {
                let i = (y * SIZE as usize + x) * 4;
                [pixels[i], pixels[i + 1], pixels[i + 2]]
            };
            let mut core = 0usize;
            let mut error = 0usize;
            let mut outliers = 0usize;
            for y in 2..SIZE as usize - 2 {
                for x in 2..SIZE as usize - 2 {
                    // Exclude antialiased silhouettes: CPU uses 2x supersampling, GPU uses 1x.
                    if (y - 1..=y + 1).any(|ny| (x - 1..=x + 1).any(|nx| at(&cpu, nx, ny) == [255; 3] || at(&gpu, nx, ny) == [255; 3])) {
                        continue;
                    }
                    let delta = at(&cpu, x, y).into_iter().zip(at(&gpu, x, y)).map(|(a, b)| usize::from(a.abs_diff(b))).collect::<Vec<_>>();
                    error += delta.iter().sum::<usize>();
                    outliers += usize::from(delta.iter().any(|d| *d > 12));
                    core += 1;
                }
            }
            assert!(core > 1000);
            let mean = error as f64 / (3 * core) as f64;
            eprintln!("PBR m={metallic} r={roughness} srgb={srgb}: core={core}, mean={mean}, outliers={outliers}");
            assert!(mean < 2.0 && outliers * 100 < core * 2, "CPU/GPU color or material interpretation differs");
        }
    }
}

#[test]
fn control_render_export_respects_upstream_file_scope() {
    let mut h = Harness::new();
    let root = h.dir.join("allowed");
    let outside = h.dir.join("outside");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let roots = json!([root.to_string_lossy()]);
    let response = h.call("ui.workspace", json!({"workspace":"render","allow_paths":roots}));
    assert_eq!(response["ok"], true, "{response}");
    let response = h.call("engine.execute", json!({"command":"render.start","params":{"settings":{"width":16,"height":16}},"allow_paths":roots}));
    assert_eq!(response["ok"], true, "{response}");
    let job = response["result"]["job"].clone();
    for _ in 0..50 {
        h.frames(1);
    }
    assert_eq!(h.app.session.execute("render.status", &json!({"job":job})).unwrap()["progress"]["phase"], "completed");
    for path in [outside.join("denied.png"), root.join("../outside/traversal.png")] {
        for overwrite in [false, true] {
            let response = h.call(
                "engine.execute",
                json!({"command":"render.export","params":{"job":job,"path":path,"overwrite":overwrite},"allow_paths":roots}),
            );
            assert_eq!(response["ok"], false, "{response}");
            assert!(response.to_string().contains("outside the folders this agent may use"), "{response}");
        }
    }
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    let path = root.join("allowed.png");
    let response = h.call("engine.execute", json!({"command":"render.export","params":{"job":job,"path":path},"allow_paths":roots}));
    assert_eq!(response["ok"], true, "{response}");
    assert!(path.is_file());
    let original = std::fs::read(&path).unwrap();
    let response = h.call("engine.execute", json!({"command":"render.export","params":{"job":job,"path":path,"overwrite":true},"allow_paths":roots}));
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn adopting_into_blank_document_resets_render_draft_and_preserves_hosts() {
    let mut h = Harness::new();
    h.app.render_workspace.host = Some(host_executor(2048, true, 4).unwrap());
    sync_backend(&mut h.app);
    h.app.render_workspace.view.name = "Old draft".into();
    enter(&mut h.app, true).unwrap();
    inspector::select_editor(&mut h.app, inspector::Editor::Output);
    let mut incoming = Session::default();
    incoming.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
    crate::documents::adopt(&mut h.app, incoming);
    assert!(!active(&h.app));
    assert_eq!(h.app.render_workspace.view.name, "Studio");
    assert_eq!(h.app.session.render_backend().executors.len(), 3);
    assert!(h.app.session.render_job_host().is_some());
    assert_eq!(h.app.session.model.state().bodies.len(), 1);
}

#[test]
fn clicked_image_controls_complete_cancel_and_export_without_dirtying_the_design() {
    for (language, legacy) in [("en", false), ("de", false), ("ko", false), ("ru", false), ("en", true)] {
        let mut h = Harness::new();
        h.app.preferences.language = language.into();
        h.app.preferences.render_legacy_image_defaults = legacy;
        h.frames(1);

        let export = studio::export_label(&h.app).to_string();
        h.call("ui.resize", json!({"width":1600,"height":1400}));
        enter(&mut h.app, true).unwrap();
        inspector::select_editor(&mut h.app, inspector::Editor::Output);
        h.app.render_workspace.view.draft.width = 16;
        h.app.render_workspace.view.draft.height = 16;
        h.app.render_workspace.view.dirty = true;
        h.frames(3);
        let doc = h.app.session.doc.clone();
        let undo = h.app.session.undo.len();
        click_render_check(&mut h);
        h.step(&json!({"click":{"handle":"render:stage:render.start"}})).unwrap();
        assert_eq!(h.app.render_workspace.view.result, !legacy, "Render must make its image visible unless the previous workflow is requested");
        for _ in 0..50 {
            h.frames(1);
            if h.app.render_workspace.view.completed.is_some() {
                break;
            }
        }
        let good = h.app.render_workspace.view.completed.unwrap();
        h.frames(2);
        if !legacy {
            assert_eq!(displayed_image(&h.app).unwrap().info.job, good);
        }
        h.step(&json!({"widget":export})).unwrap();
        assert!(h.dir.join("render.png").is_file());
        h.app.render_workspace.view.draft.width = 1024;
        h.app.render_workspace.view.draft.height = 768;
        h.step(&json!({"click":{"handle":"render:stage:render.start"}})).unwrap();
        assert!(job_ready(&h.app, "render.cancel").is_ok());
        h.step(&json!({"click":{"handle":"render:stage:render.cancel"}})).unwrap();
        h.frames(5);
        assert_eq!(h.app.render_workspace.view.completed, Some(good));
        assert_eq!(
            h.app.session.execute("render.status", &json!({"job":h.app.render_workspace.view.job})).unwrap()["progress"]["phase"],
            "cancelled"
        );
        assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
        assert_eq!(h.app.session.undo.len(), undo);
        assert!(!h.app.session.is_dirty());
    }
}

#[test]
fn saved_camera_survives_frames_and_product_fit_ignores_selection() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5,"body_name":"Product"})).unwrap();
    h.app.run("solid.box", json!({"corner":[100,0,0],"length":10,"width":20,"height":5,"body_name":"Other"})).unwrap();
    let camera = Camera { up: solidcraft_engine::render::UpAxis::Y, roll: 0.2, ..Default::default() };
    h.app.run("render.settings.set", json!({"name":"Studio","set":{"camera":camera,"background":[21,43,65]}})).unwrap();
    h.frames(3);
    let design = h.app.cam;
    enter(&mut h.app, true).unwrap();
    inspector::select_editor(&mut h.app, inspector::Editor::Output);
    h.frames(3);
    h.app.run("view.render_select", json!({"kind":"shot","id":"Studio"})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:shot:revert"}})).unwrap();
    h.frames(3);
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.render_workspace.view.draft.background, [21, 43, 65]);
    h.app.session.selection = vec![solidcraft_engine::Sel::Body { name: "Product".into() }];
    {
        let selection = object_selection(&h.app);
        selection_editor(&mut h.app, &selection);
    }
    h.call("engine.execute", json!({"command":"view.fit","params":{}}));
    h.frames(3);
    let selected_fit = h.app.cam;
    assert_product_fits_canvas(&h.app);
    h.app.session.selection.clear();
    h.app.cam = camera;
    h.call("engine.execute", json!({"command":"view.fit","params":{}}));
    h.frames(3);
    assert_eq!(h.app.cam, selected_fit, "Fit Product ignores the selected subset");
    enter(&mut h.app, false).unwrap();
    assert_eq!(h.app.cam, design);
}

/// Inspect the mesh submitted by the actual Render canvas, including its GPU-unavailable path.
/// This is a headless colour handoff assertion, not a rendered-pixel or screenshot comparison.
fn render_preview_background_frame(h: &mut Harness, time: f64) -> [Color32; 4] {
    let app = &mut h.app;
    let mut viewport = egui::Rect::NOTHING;
    let mut output = h.ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
            time: Some(time),
            ..Default::default()
        },
        |ui| {
            app.logic(ui.ctx());
            viewport = ui.max_rect();
            show(app, ui);
        },
    );
    let corners = [viewport.left_top(), viewport.right_top(), viewport.right_bottom(), viewport.left_bottom()];
    let colors = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Mesh(mesh) if mesh.vertices.len() == 4 && mesh.vertices.iter().zip(corners).all(|(vertex, corner)| vertex.pos == corner) => {
                Some(std::array::from_fn(|i| mesh.vertices[i].color))
            }
            _ => None,
        })
        .expect("Render must paint a background mesh over the full viewport");
    output.textures_delta.clear();
    colors
}

#[test]
fn render_preview_tab_surfaces_use_published_gradient_through_fade() {
    for base in ["dark", "light"] {
        let mut h = Harness::new();
        let file = crate::theme_files::tab_fixture(base);
        crate::prefs::control(&mut h.app, &json!({"theme_import":crate::theme_files::export_file(&file)})).unwrap();
        let app = &mut h.app;
        h.ctx.run_ui(egui::RawInput { time: Some(1.0), ..Default::default() }, |ui| app.logic(ui.ctx())).textures_delta.clear();
        let model = Tokens::get();
        enter(&mut h.app, true).unwrap();
        assert!(h.app.session.doc.render.setups.is_empty());
        let document = h.app.session.doc.clone();
        let request = image_request(&h.app).unwrap();
        let history = (h.app.session.revision, h.app.session.undo.len());
        let end = crate::theme_files::resolve_for_tab(&file, &Default::default(), Some("RENDER"));
        let expected_start = [model.viewport_top, model.viewport_top, model.viewport_bottom, model.viewport_bottom];
        let expected_end = [end.viewport_top, end.viewport_top, end.viewport_bottom, end.viewport_bottom];
        let mut previous = expected_start;
        for progress in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let colors = render_preview_background_frame(&mut h, 2.0 + 0.24 * progress);
            let shown = Tokens::get();
            assert_eq!(colors, [shown.viewport_top, shown.viewport_top, shown.viewport_bottom, shown.viewport_bottom]);
            if progress == 0.0 {
                assert_eq!(colors, expected_start);
            } else if progress == 1.0 {
                assert_eq!(colors, expected_end);
            } else if progress <= 0.5 {
                assert_ne!(colors, expected_start, "the Render canvas must leave the starting colours at {progress}");
                assert_ne!(colors, expected_end, "the Render canvas must receive intermediate fade colours at {progress}");
                assert_ne!(colors, previous, "the Render canvas fade must advance between early samples at {progress}");
            }
            // At 75% elapsed time, ease-out cubic is already 98.4375% of the way to the
            // destination. Byte quantization can legitimately produce the exact final colours.
            previous = colors;
            assert!(Arc::ptr_eq(&document, &h.app.session.doc));
            assert_eq!((h.app.session.revision, h.app.session.undo.len()), history);
            assert_eq!(image_request(&h.app).unwrap(), request, "UI themes never modify rendered image settings");
        }
        assert_eq!(render_preview_background_frame(&mut h, 3.0), expected_end);
    }
}

#[test]
fn render_preview_tab_surfaces_preserve_scene_backplates_and_viewport_overrides() {
    for base in ["dark", "light"] {
        let mut h = Harness::new();
        let file = crate::theme_files::tab_fixture(base);
        crate::prefs::control(
            &mut h.app,
            &json!({
                "theme_import":crate::theme_files::export_file(&file),
                "theme_options":{&file.id:{"tab_fade_ms":0}}
            }),
        )
        .unwrap();
        enter(&mut h.app, true).unwrap();
        let scene_rgb = h.app.render_workspace.view.draft.background;
        let scene = [Color32::from_rgb(scene_rgb[0], scene_rgb[1], scene_rgb[2]); 4];
        let gradient = render_preview_background_frame(&mut h, 1.0);
        assert_ne!(gradient, scene);

        crate::prefs::control(&mut h.app, &json!({"theme_options":{&file.id:{"tab_colours":false}}})).unwrap();
        assert_eq!(render_preview_background_frame(&mut h, 2.0), scene);
        crate::prefs::control(
            &mut h.app,
            &json!({"theme_options":{&file.id:{"tab_colours":true}},"viewport_theme":if base == "dark" {"light"} else {"dark"}}),
        )
        .unwrap();
        let opposite = if base == "dark" { Tokens::light() } else { Tokens::dark() };
        assert_eq!(
            render_preview_background_frame(&mut h, 3.0),
            [opposite.viewport_top, opposite.viewport_top, opposite.viewport_bottom, opposite.viewport_bottom]
        );
        crate::prefs::control(&mut h.app, &json!({"viewport_theme":"theme","viewport_background":[3,7,11]})).unwrap();
        assert_eq!(render_preview_background_frame(&mut h, 4.0), [Color32::from_rgb(3, 7, 11); 4]);

        // Authored scene intent takes priority even over a viewport preference. Test temporary
        // drafts as well as saved setups, without persisting any theme colour into the scene.
        let original = h.app.render_workspace.view.clone();
        for case in 0..6 {
            h.app.render_workspace.view = original.clone();
            match case {
                0 => h.app.render_workspace.view.draft.background = [21, 43, 65],
                1 => h.app.render_workspace.view.transparent = true,
                2 => h.app.render_workspace.view.studio.backdrop_enabled = true,
                3 => h.app.render_workspace.view.studio.environment = studio::Environment::Sky,
                4 => h.app.render_workspace.view.cpu.environment = cpu::Environment::Constant,
                _ => {
                    h.app.render_workspace.view.raytrace.environment =
                        Some(solidcraft_engine::render::job::RaytraceEnvironmentOptions { color: [0.2, 0.3, 0.4], intensity: 1.0 })
                }
            }
            let expected = if case == 0 { [Color32::from_rgb(21, 43, 65); 4] } else { scene };
            assert_eq!(render_preview_background_frame(&mut h, 5.0 + f64::from(case)), expected);
        }
        h.app.render_workspace.view = original;
        h.app.run("render.settings.set", json!({"name":"Studio","set":{"camera":h.app.cam}})).unwrap();
        setups::load(&mut h.app);
        let authored = h.app.session.doc.clone();
        assert_eq!(render_preview_background_frame(&mut h, 11.0), scene, "even a saved default-colour backplate is scene-owned");
        assert!(Arc::ptr_eq(&authored, &h.app.session.doc));

        // A theme without tabs keeps the legacy Render background, as do built-in themes.
        h.app.render_workspace.view.name = "Unsaved".into();
        setups::load(&mut h.app);
        crate::prefs::control(&mut h.app, &json!({"theme_import":{"format":"solidcraft-theme/1","name":"No tabs","base":base}})).unwrap();
        assert_eq!(render_preview_background_frame(&mut h, 12.0), scene);
        crate::prefs::control(&mut h.app, &json!({"theme":base})).unwrap();
        assert_eq!(render_preview_background_frame(&mut h, 13.0), scene);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
/// Exercise the registered GPU host through the UI, including mapped output and glass.
fn progressive_image_controls_route_to_the_registered_host() {
    use solidcraft_engine::render::job::Phase;
    use std::time::{Duration, Instant};

    let host = solidcraft_gpu::select::Policy::from_env()
        .and_then(|policy| solidcraft_gpu::select::pick(policy).ok_or_else(|| "no qualifying adapter".to_string()))
        .and_then(|(adapter, _)| solidcraft_gpu::path::host_on_adapter(&adapter).map_err(|e| e.to_string()));
    let host = match host {
        Ok(host) => host,
        Err(e) => {
            let expected = !solidcraft_gpu::select::path_backends().is_empty();
            assert!(!expected || std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_none(), "required GPU unavailable: {e}");
            eprintln!("skipping progressive UI device test: {e}");
            return;
        }
    };
    let mut h = Harness::new();
    h.app.render_workspace.path_host = PathHost::Ready(host);
    sync_backend(&mut h.app);
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5})).unwrap();
    h.app.session.mark_saved();
    h.call("ui.resize", json!({"width":1600,"height":1600}));
    enter(&mut h.app, true).unwrap();
    inspector::select_editor(&mut h.app, inspector::Editor::Output);
    h.app.render_workspace.view.draft.width = 16;
    h.app.render_workspace.view.draft.height = 16;
    h.app.render_workspace.view.samples = 4;
    h.app.render_workspace.view.max_depth = 3;
    h.app.render_workspace.view.seed = 7;
    h.app.render_workspace.view.draft.exposure_ev = 0.5;
    h.app.render_workspace.view.studio.tone_map = solidcraft_engine::render::job::ToneMap::AcesFitted;
    h.app.render_workspace.view.studio.lens_enabled = true;
    h.app.render_workspace.view.studio.lens.aperture_radius_mm = 0.0;
    h.app.render_workspace.view.studio.lighting_enabled = true;
    h.app.render_workspace.view.studio.ambient = [0.75; 3];
    h.frames(3);
    let engineering = h.app.session.doc.features.clone();
    h.step(&json!({"click":{"handle":"render:method"}})).unwrap();
    h.step(&json!({"click":{"handle":"render:method-gpu"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::GpuRaytrace);
    assert_eq!(h.app.session.doc.features, engineering);
    assert!(matches!(
        h.app.session.doc.render.setups[&h.app.render_workspace.view.name].job,
        Some(solidcraft_engine::doc::render_settings::RenderJob::GpuPath { .. })
    ));
    // The UI request reaches the real registered host with a coat and captured map.
    let target: materials::Target = serde_json::from_value(h.app.run("render.scene", json!({})).unwrap()["bodies"][0].clone()).unwrap();
    let mut material = materials::Binding::new(target, &h.app);
    material.coat_enabled = true;
    material.maps_enabled = true;
    let map_path = h.dir.join("original-color.png");
    image::RgbaImage::from_pixel(1, 1, image::Rgba([80, 160, 220, 255])).save(&map_path).unwrap();
    material.maps.color = map_path.to_string_lossy().into_owned();
    h.app.render_workspace.view.materials.enabled = true;
    h.app.render_workspace.view.materials.bindings.push(material);
    h.app.session.mark_saved();
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let checked = image_params(&h.app).unwrap();
    assert_eq!(checked["settings"]["coats"][0]["coat"]["weight"], 1.0);
    assert_eq!(checked["raytrace"]["textures"][0]["base_color_srgb"], map_path.to_string_lossy().as_ref());
    h.app.run("render.preflight", checked.clone()).unwrap();
    assert_eq!(image_params(&h.app).unwrap(), checked);

    h.step(&json!({"click":{"handle":"render:stage:render.start"}})).unwrap();
    let job = h.app.render_workspace.view.job.unwrap();
    let handle = h.app.session.render_job_handle(job).unwrap();
    // The first mapped render compiles a separate pipeline on a cold Vulkan cache.
    // Allow the GPU host's setup budget, then keep the short deadline after preparation.
    let submitted = Instant::now();
    let queue_deadline = submitted + Duration::from_secs(30);
    let setup_deadline = submitted + Duration::from_secs(600);
    let mut render_deadline = None;
    while h.app.render_workspace.view.completed.is_none() {
        let status = handle.status().unwrap();
        let now = Instant::now();
        let deadline = match status.progress.phase {
            Phase::Queued => queue_deadline,
            Phase::Preparing => setup_deadline,
            Phase::Failed | Phase::Cancelled => panic!("progressive UI job ended: {status:?}"),
            _ => *render_deadline.get_or_insert(now + Duration::from_secs(30)),
        };
        assert!(now < deadline, "progressive UI job did not complete: {status:?}");
        std::thread::sleep(Duration::from_millis(5));
        h.frames(1);
    }
    let status = h.app.session.execute("render.status", &json!({"job":job})).unwrap();
    assert_eq!(status["frame"]["executor"], "gpu-path", "{status}");
    assert_eq!(status["options"]["samples"], 4);
    assert_eq!(status["options"]["max_depth"], 3);
    assert_eq!(status["options"]["seed"], 7);
    assert_eq!(status["options"]["exposure_ev"], 0.5);
    assert_eq!(status["options"]["tone_map"], "aces_fitted");
    assert_eq!(status["options"]["lens"]["aperture_radius_mm"], 0.0);
    assert_eq!(status["options"]["lighting"]["environment"], json!([0.75, 0.75, 0.75]));
    assert_eq!(status["textures"]["bodies"], 1);
    let export = studio::export_label(&h.app).to_string();
    h.step(&json!({"widget":export})).unwrap();
    assert!(h.dir.join("render.png").is_file());
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(!h.app.session.is_dirty());
    // A pack glass recipe follows the same production request and passes actual
    // closed-mesh preparation. It never calls scalar appearance assignment.
    let material = &mut h.app.render_workspace.view.materials.bindings[0];
    material.coat_enabled = false;
    material.maps_enabled = false;
    material.glass_enabled = true;
    material.glass = materials::Glass::recipe("starter.glass.clear-soda-lime").unwrap();
    let checked = image_params(&h.app).unwrap();
    h.app.run("render.preflight", checked.clone()).unwrap();
    assert_eq!(image_params(&h.app).unwrap(), checked);
    let started = h.app.run("render.start", checked).unwrap();
    let job = serde_json::from_value(started["job"].clone()).unwrap();
    let status = h.app.session.render_job_handle(job).unwrap().wait_timeout(Duration::from_secs(30)).unwrap();
    assert_eq!(status.progress.phase, Phase::Completed, "{:?}", status.error);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(!h.app.session.is_dirty());
}

#[test]
fn clicked_shot_editor_saves_camera_and_restores_design_view() {
    for language in ["en", "de", "ko", "ru"] {
        let mut h = Harness::new();
        h.app.preferences.language = language.into();
        h.app.run("solid.box", json!({"length":10,"width":20,"height":5})).unwrap();
        h.app.session.mark_saved();
        h.frames(3);
        let camera = h.app.cam;
        let flags = (h.app.ui.show_grid, h.app.origin_shown(), h.app.ui.visual_style, h.app.ui.perspective);
        let undo = h.app.session.undo.len();
        let at = crate::scenario::rect_of("workspace_switcher").unwrap().center();
        h.step(&json!({"click":[at.x,at.y]})).unwrap();
        h.step(&json!({"click":{"handle":"menu:ui.workspace.render"}})).unwrap();
        assert!(active(&h.app));
        assert!(h.widget(h.app.tr("render.page_scene")).is_some(), "{language}");
        h.step(&json!({"click":{"handle":"render:shots:save"}})).unwrap();
        h.app.render_workspace.view.save_shot_name = Some("Studio".into());
        h.frames(3);
        h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.app.run("view.render_select", json!({"kind":"shot","id":"Studio"})).unwrap();
        h.frames(3);
        assert_eq!(inspector::selected_editor(&h.app), inspector::Editor::Camera);
        h.step(&json!({"widget":h.app.tr("render.perspective")})).unwrap();
        assert_eq!(h.app.cam.fov, 35f64.to_radians(), "{language}: keep the current Shot editor's projection behavior");
        assert_eq!(h.app.session.undo.len(), undo + 2);
        assert_eq!(h.app.session.doc.render.setups["Studio"].camera, h.app.cam);
        let at = crate::scenario::rect_of("workspace_switcher").unwrap().center();
        h.step(&json!({"click":[at.x,at.y]})).unwrap();
        h.step(&json!({"click":{"handle":"menu:ui.workspace.design"}})).unwrap();
        assert!(!active(&h.app));
        assert_eq!(h.app.cam, camera);
        assert_eq!(flags, (h.app.ui.show_grid, h.app.origin_shown(), h.app.ui.visual_style, h.app.ui.perspective));
        for _ in 0..2 {
            h.app.run("edit.undo", json!({})).unwrap();
        }
        assert!(!h.app.session.is_dirty());
    }
}

#[test]
fn render_drafts_and_capabilities_are_isolated_across_document_switches() {
    let mut h = Harness::new();
    h.app.render_workspace.host = Some(host_executor(2048, true, 4).unwrap());
    sync_backend(&mut h.app);
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.render_workspace.view.name = "First draft".into();
    h.app.render_workspace.view.draft.background = [20, 40, 60];
    h.app.render_workspace.view.dirty = true;
    enter(&mut h.app, true).unwrap();
    inspector::select_editor(&mut h.app, inspector::Editor::Output);
    crate::documents::open_tab(&mut h.app, Session::default()).unwrap();
    assert!(!active(&h.app));
    assert_eq!(h.app.render_workspace.view.name, "Studio");
    assert_eq!(h.app.session.render_backend().executors.len(), 3);
    crate::documents::switch(&mut h.app, 0);
    assert_eq!(h.app.render_workspace.view.name, "First draft");
    assert_eq!(h.app.render_workspace.view.draft.background, [20, 40, 60]);
    assert!(h.app.render_workspace.view.dirty);
    assert_eq!(h.app.session.render_backend().executors.len(), 3);
    h.app.run("file.new", json!({})).unwrap();
    assert_eq!(h.app.render_workspace.view.name, "Studio");
    assert_eq!(h.app.session.render_backend().executors.len(), 3);
}

#[test]
fn product_gpu_buffers_preserve_opacity_and_never_contain_overlays() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10,"body_name":"Glass"})).unwrap();
    s.execute("appearance.assign", &json!({"bodies":["Glass"],"color":"#336699","opacity":0.4})).unwrap();
    let scene = solidcraft_engine::product_scene::extract(&s).unwrap();
    let (key, gpu) = gpu_scene(&scene).unwrap();
    assert!(gpu.tris.is_empty());
    assert!(!gpu.glass.is_empty());
    let color = |v: &[u8]| {
        let i = v.get(crate::gpu::TRI_SIZE - 4..).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes)?;
        gpu.palette.get(crate::gpu::PaletteIndex(i))
    };
    assert!(gpu.glass.as_chunks::<{ crate::gpu::TRI_SIZE }>().0.iter().all(|v| color(v) == Some([51, 102, 153, 102])));
    assert!(gpu.lines.is_empty() && gpu.overlay.is_empty() && gpu.under.is_empty() && gpu.hidden.is_empty() && gpu.ghost.is_empty());
    s.selection = vec![solidcraft_engine::Sel::Body { name: "Glass".into() }];
    s.section = Some((solidcore_record::Vec3::ZERO, solidcore_record::Vec3::X));
    let after = solidcraft_engine::product_scene::extract(&s).unwrap();
    assert_eq!(gpu_scene(&after).unwrap().0, key);
    s.execute("appearance.assign", &json!({"bodies":["Glass"],"color":"#336699","opacity":0.0})).unwrap();
    let invisible = solidcraft_engine::product_scene::extract(&s).unwrap();
    assert_eq!(invisible.meshes[0].opacity, 0.0);
    let (_, empty) = gpu_scene(&invisible).unwrap();
    assert!(empty.tris.is_empty() && empty.glass.is_empty());
}

#[test]
fn transmission_uses_legacy_preview_buffers_and_invalidates_the_opaque_cache() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10,"body_name":"Glass"})).unwrap();
    let appearance = json!({"bodies":["Glass"],"color":"#336699",
        "pbr":{"metallic":0,"roughness":0.1,"dielectric_ior":1.5}});
    s.execute("appearance.assign", &appearance).unwrap();
    let (opaque_key, opaque) = gpu_scene(&solidcraft_engine::product_scene::extract(&s).unwrap()).unwrap();
    assert!(!opaque.pbr.is_empty());
    let mut scene = solidcraft_engine::product_scene::extract(&s).unwrap();
    scene.meshes[0].transmission = Some(solidcraft_engine::render::transmission::Transmission::new(solidcore_record::Vec3::ZERO, None).unwrap());
    let (glass_key, glass) = gpu_scene(&scene).unwrap();
    assert_ne!(opaque_key, glass_key);
    assert!(glass.pbr.is_empty());
    assert!(!glass.tris.is_empty());
    scene.meshes[0].opacity = 0.4;
    let (_, transparent) = gpu_scene(&scene).unwrap();
    assert!(transparent.pbr.is_empty());
    assert!(!transparent.glass.is_empty());
}

#[test]
fn host_profiles_advertise_only_installed_opaque_pbr_not_hdri_refraction_or_final() {
    use solidcraft_engine::render::*;
    let host = host_executor(512, true, 4).unwrap();
    assert_eq!(host.limits.max_dimension, 512);
    assert_eq!(host.profiles.len(), 1);
    assert_eq!(host.profiles[0].features, vec![RenderFeature::Pbr]);
    assert!(host.profiles[0].formats.is_empty());
    let mut s = Session::default();
    s.set_render_viewport(Some(host)).unwrap();
    let mut request = RenderRequest {
        backend: None,
        mode: RenderMode::Preview,
        operation: RenderOperation::Viewport,
        width: 512,
        height: 512,
        format: None,
        features: Vec::new(),
        policy: Default::default(),
    };
    assert_eq!(resolve_request(&[s.render_backend()], &request).unwrap().executor.as_str(), "gpu-viewport");
    request.features = vec![RenderFeature::Pbr];
    assert!(resolve_request(&[s.render_backend()], &request).is_ok());
    for unsupported in [RenderFeature::Hdri, RenderFeature::Refraction, RenderFeature::Progressive] {
        request.features = vec![unsupported];
        assert!(resolve_request(&[s.render_backend()], &request).is_err());
    }
    request.features.clear();
    request.width = 513;
    assert!(resolve_request(&[s.render_backend()], &request).is_err());
    request.width = 512;
    request.mode = RenderMode::Final;
    assert!(resolve_request(&[s.render_backend()], &request).is_err());
    assert!(host_executor(1024, false, 1).unwrap().availability != Availability::Available);
    assert!(host_executor(1024, true, 0).is_err());
}

fn completed_preview(h: &mut Harness, background: [u8; 3]) -> solidcraft_engine::render::job::JobId {
    let reply = h.app.session.execute("render.start", &json!({"settings":{"width":16,"height":16,"background":background}})).unwrap();
    let job = serde_json::from_value(reply["job"].clone()).unwrap();
    let host = h.app.session.render_job_host().unwrap().clone();
    for _ in 0..100 {
        host.pump(16384).unwrap();
        if h.app.session.execute("render.status", &json!({"job":job})).unwrap()["progress"]["phase"] == "completed" {
            return job;
        }
    }
    panic!("small cooperative image did not complete within the work budget");
}

fn image_panel_frame(app: &mut SolveApp, ctx: &egui::Context) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(285.0, 1200.0))), ..Default::default() },
        |ui| image_controls(app, ui),
    )
}

#[test]
fn completed_image_is_painted_once_available_and_upload_is_cached() {
    let mut h = Harness::new();
    let ctx = egui::Context::default();
    crate::fonts::setup_bundled_fonts(&ctx);
    image_panel_frame(&mut h.app, &ctx).textures_delta.clear();
    assert!(h.app.render_workspace.image.is_none());
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let job = completed_preview(&mut h, [20, 40, 60]);
    h.app.render_workspace.view.job = Some(job);
    let mut output = image_panel_frame(&mut h.app, &ctx);
    let (cached_job, texture) = h.app.render_workspace.image.as_ref().unwrap();
    let texture = texture.as_ref().unwrap();
    let id = texture.id();
    assert_eq!(*cached_job, job);
    assert_eq!(texture.size(), [16, 16]);
    let uploaded = &output.textures_delta.set.get(&id).unwrap().first().unwrap().image;
    let egui::ImageData::Color(pixels) = uploaded;
    assert!(pixels.pixels.iter().all(|p| *p == Color32::from_rgb(20, 40, 60)));
    // egui requires frame texture deltas to be applied or cleared.
    output.textures_delta.clear();
    assert!(
        ctx.tessellate(output.shapes, output.pixels_per_point)
            .iter()
            .any(|shape| { matches!(&shape.primitive, egui::epaint::Primitive::Mesh(mesh) if mesh.texture_id == id && !mesh.indices.is_empty()) })
    );
    let mut output = image_panel_frame(&mut h.app, &ctx);
    assert!(!output.textures_delta.set.contains_key(&id), "repaint must reuse the texture");
    // egui requires frame texture deltas to be applied or cleared.
    output.textures_delta.clear();
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    let path = h.dir.join("displayed.png");
    h.app.run("render.export", json!({"job":job,"path":path})).unwrap();
    let frame = h.app.session.render_job_handle(job).unwrap().frame().unwrap().unwrap();
    assert_eq!(std::fs::read(path).unwrap(), frame.png(), "displaying the image must not change export bytes");
}

#[test]
fn completed_image_keeps_last_export_during_work_and_updates_for_the_next_job() {
    let mut h = Harness::new();
    let ctx = egui::Context::default();
    crate::fonts::setup_bundled_fonts(&ctx);
    let first = completed_preview(&mut h, [20, 40, 60]);
    h.app.render_workspace.view.job = Some(first);
    image_panel_frame(&mut h.app, &ctx).textures_delta.clear();
    let old_texture = h.app.render_workspace.image.as_ref().unwrap().1.as_ref().unwrap().id();
    let reply = h.app.run("render.start", json!({"settings":{"width":16,"height":16}})).unwrap();
    let pending = serde_json::from_value(reply["job"].clone()).unwrap();
    h.app.render_workspace.view.job = Some(pending);
    image_panel_frame(&mut h.app, &ctx).textures_delta.clear();
    assert_eq!(h.app.render_workspace.image.as_ref().unwrap().0, first);
    h.app.run("render.cancel", json!({"job":pending})).unwrap();
    image_panel_frame(&mut h.app, &ctx).textures_delta.clear();
    assert_eq!(h.app.render_workspace.image.as_ref().unwrap().0, first);
    let second = completed_preview(&mut h, [80, 100, 120]);
    h.app.render_workspace.view.job = Some(second);
    let mut output = image_panel_frame(&mut h.app, &ctx);
    let (cached_job, texture) = h.app.render_workspace.image.as_ref().unwrap();
    assert_eq!(*cached_job, second);
    assert_ne!(texture.as_ref().unwrap().id(), old_texture);
    assert!(output.textures_delta.free.contains(&old_texture));
    // egui requires frame texture deltas to be applied or cleared.
    output.textures_delta.clear();
    h.app.render_workspace.view.result = true;
    h.app.render_workspace.view.result_scale = ResultScale::Free;
    h.app.render_workspace.view.result_camera.distance = 42.0;
    let framing = h.app.render_workspace.view.result_camera;
    crate::documents::open_tab(&mut h.app, Session::default()).unwrap();
    image_panel_frame(&mut h.app, &ctx).textures_delta.clear();
    assert!(h.app.render_workspace.image.is_none(), "another document must not show this image");
    assert!(!h.app.render_workspace.view.result);
    crate::documents::switch(&mut h.app, 0);
    image_panel_frame(&mut h.app, &ctx).textures_delta.clear();
    assert_eq!(h.app.render_workspace.image.as_ref().unwrap().0, second);
    assert!(h.app.render_workspace.view.result);
    assert_eq!(h.app.render_workspace.view.result_camera, framing);
    assert_eq!(h.app.render_workspace.view.result_scale, ResultScale::Free);
    reset_document(&mut h.app);
    assert!(h.app.render_workspace.image.is_none());
    assert!(!h.app.render_workspace.view.result);
}

#[test]
fn completed_image_decoder_preserves_alpha_and_rejects_invalid_sizes_and_pngs() {
    let mut png = std::io::Cursor::new(Vec::new());
    let pixels = image::RgbaImage::from_pixel(2, 1, image::Rgba([80, 120, 160, 128]));
    pixels.write_to(&mut png, image::ImageFormat::Png).unwrap();
    let decoded = decode_image(png.get_ref(), 2, 1).unwrap();
    assert_eq!(decoded.size, [2, 1]);
    assert_eq!(decoded.pixels, vec![Color32::from_rgba_unmultiplied(80, 120, 160, 128); 2]);
    for (width, height) in [(0, 1), (u32::MAX, 1), (4096, 4096), (1, 1), (3, 1)] {
        assert!(decode_image(png.get_ref(), width, height).is_err(), "{width}x{height}");
    }
    assert!(decode_image(b"invalid PNG", 2, 1).is_err());
}

#[test]
fn cpu_checkpoints_replace_only_the_provisional_texture_and_keep_final_export_and_framing() {
    let mut h = Harness::new();
    let ctx = egui::Context::default();
    let first = completed_preview(&mut h, [20, 40, 60]);
    h.app.render_workspace.view.completed = Some(first);
    h.app.render_workspace.view.job = Some(first);
    cache_render_images(&mut h.app, &ctx);
    let final_texture = h.app.render_workspace.image.as_ref().unwrap().1.as_ref().unwrap().id();
    let second = completed_preview(&mut h, [80, 100, 120]);
    let frame = h.app.session.render_job_handle(second).unwrap().frame().unwrap().unwrap();
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.app.render_workspace.view.job = Some(second);
    let mut info = PreviewInfo { frame_sequence: 1, samples_per_pixel: 1, adaptive: None, frame: frame.info.clone() };
    cache_preview_frame(&mut h.app, &ctx, &info, frame.png());
    remember_displayed_job(&mut h.app, false);
    assert_eq!(displayed_image(&h.app).unwrap().info.job, second);
    assert!(displayed_image(&h.app).unwrap().preview.is_some());
    h.app.render_workspace.view.result_scale = ResultScale::Free;
    h.app.render_workspace.view.result_camera.distance = 123.0;
    h.app.render_workspace.view.result_camera.target.x = 5.0;
    let camera = h.app.render_workspace.view.result_camera;
    let first_checkpoint = h.app.render_workspace.preview_image.as_ref().unwrap().image.as_ref().unwrap().id();
    // Repeated or older metadata never re-decodes bytes, even if passed bad data.
    cache_preview_frame(&mut h.app, &ctx, &info, b"must not decode again");
    assert_eq!(h.app.render_workspace.preview_image.as_ref().unwrap().image.as_ref().unwrap().id(), first_checkpoint);
    info.frame_sequence = 2;
    info.samples_per_pixel = 2;
    cache_preview_frame(&mut h.app, &ctx, &info, frame.png());
    let newer = h.app.render_workspace.preview_image.as_ref().unwrap().image.as_ref().unwrap().id();
    assert_ne!(newer, first_checkpoint);
    info.frame_sequence = 1;
    cache_preview_frame(&mut h.app, &ctx, &info, b"out of order");
    remember_displayed_job(&mut h.app, false);
    assert_eq!(h.app.render_workspace.preview_image.as_ref().unwrap().info.frame_sequence, 2);
    assert_eq!(h.app.render_workspace.preview_image.as_ref().unwrap().image.as_ref().unwrap().id(), newer);
    assert_eq!(h.app.render_workspace.view.result_camera, camera);
    assert_eq!(h.app.render_workspace.view.result_scale, ResultScale::Free);
    assert_eq!(h.app.render_workspace.image.as_ref().unwrap().1.as_ref().unwrap().id(), final_texture);
    assert_eq!(h.app.render_workspace.view.completed, Some(first));
    let path = h.dir.join("checkpoint-export-is-final.png");
    h.app.run("render.export", json!({"job":h.app.render_workspace.view.completed,"path":path})).unwrap();
    let retained = h.app.session.render_job_handle(first).unwrap().frame().unwrap().unwrap();
    assert_eq!(std::fs::read(path).unwrap(), retained.png());
    // A final can arrive after the panel read a still-running status. The cached
    // completed ID deliberately remains the old job until publication is sampled.
    cache_render_images(&mut h.app, &ctx);
    assert!(h.app.render_workspace.preview_image.is_none());
    assert!(displayed_image(&h.app).unwrap().preview.is_none());
    assert_eq!(h.app.render_workspace.image_info.as_ref().unwrap().job, second);
    assert_eq!(h.app.render_workspace.view.completed, Some(second));
    assert_eq!(h.app.render_workspace.view.result_camera, camera);
    assert_eq!(h.app.render_workspace.view.result_scale, ResultScale::Free);
    cache_render_images(&mut h.app, &ctx);
    assert_eq!(h.app.render_workspace.view.result_camera, camera);
    assert_eq!(h.app.render_workspace.view.result_scale, ResultScale::Free, "a subsequent status refresh must not refit the new final");
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    ctx.run_ui(Default::default(), |_| {}).textures_delta.clear();
}

#[test]
fn cancelling_a_preview_returns_to_the_final_image_without_resetting_pan_or_export() {
    let mut h = Harness::new();
    let ctx = egui::Context::default();
    let final_job = completed_preview(&mut h, [30, 50, 70]);
    h.app.render_workspace.view.completed = Some(final_job);
    h.app.render_workspace.view.job = Some(final_job);
    cache_render_images(&mut h.app, &ctx);
    let frame = h.app.session.render_job_handle(final_job).unwrap().frame().unwrap().unwrap();
    let reply = h.app.run("render.start", json!({"settings":{"width":16,"height":16}})).unwrap();
    let pending = serde_json::from_value(reply["job"].clone()).unwrap();
    h.app.render_workspace.view.job = Some(pending);
    let mut info = PreviewInfo { frame_sequence: 1, samples_per_pixel: 1, adaptive: None, frame: frame.info.clone() };
    cache_preview_frame(&mut h.app, &ctx, &info, frame.png());
    assert!(h.app.render_workspace.preview_image.is_none(), "a different job's checkpoint must be ignored");
    info.frame.job = pending;
    cache_preview_frame(&mut h.app, &ctx, &info, frame.png());
    remember_displayed_job(&mut h.app, false);
    h.app.render_workspace.view.result_scale = ResultScale::Free;
    h.app.render_workspace.view.result_camera.target.x = 12.0;
    h.app.render_workspace.view.result_camera.distance = 87.0;
    let camera = h.app.render_workspace.view.result_camera;
    h.app.run("render.cancel", json!({"job":pending})).unwrap();
    cache_render_images(&mut h.app, &ctx);
    assert!(h.app.render_workspace.preview_image.is_none());
    assert_eq!(displayed_image(&h.app).unwrap().info.job, final_job);
    assert_eq!(h.app.render_workspace.view.result_camera, camera);
    assert_eq!(h.app.render_workspace.view.result_scale, ResultScale::Free);
    assert_eq!(h.app.render_workspace.view.completed, Some(final_job));
    assert!(h.app.run("render.export", json!({"job":pending,"path":h.dir.join("not-final.png")})).is_err());
    let path = h.dir.join("after-preview-cancel.png");
    h.app.run("render.export", json!({"job":final_job,"path":path})).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), frame.png());
    // Document adoption must also release a provisional texture without leaking it into a tab.
    cache_preview_frame(&mut h.app, &ctx, &info, frame.png());
    assert!(h.app.render_workspace.preview_image.is_some());
    h.app.render_workspace.view.raytrace.progressive = Some(Default::default());
    crate::documents::open_tab(&mut h.app, Session::default()).unwrap();
    assert!(displayed_image(&h.app).is_none());
    assert!(h.app.render_workspace.preview_image.is_none());
    assert!(h.app.render_workspace.view.raytrace.progressive.is_none());
    ctx.run_ui(Default::default(), |_| {}).textures_delta.clear();
}

#[test]
fn result_framing_fits_both_axes_and_actual_size_respects_display_pixels() {
    let canvas = egui::Rect::from_min_size(egui::pos2(40.0, 80.0), egui::vec2(800.0, 500.0));
    for size in [[1920, 1080], [100, 4000], [16, 16]] {
        for dpi in [1.0, 1.5, 2.0] {
            let fit = result_camera(size, canvas, dpi, ResultScale::Fit);
            assert!(fit.is_valid());
            let fitted = result_rect(fit, size, canvas);
            assert!(canvas.contains_rect(fitted), "{size:?} at {dpi}: {fitted:?}");
            assert_eq!(fitted.center(), canvas.center());
            let actual = result_rect(result_camera(size, canvas, dpi, ResultScale::Actual), size, canvas);
            assert!((actual.width() * dpi - size[0] as f32).abs() < 0.01);
            assert!((actual.height() * dpi - size[1] as f32).abs() < 0.01);
        }
    }
}

#[test]
fn result_canvas_paints_large_capture_reports_staleness_and_preserves_export() {
    let mut h = Harness::new();
    let ctx = crate::fonts::test_context();
    let job = completed_preview(&mut h, [20, 40, 60]);
    h.app.render_workspace.view.job = Some(job);
    h.app.render_workspace.view.completed = Some(job);
    h.app.render_workspace.view.result = true;
    enter(&mut h.app, true).unwrap();
    inspector::select_editor(&mut h.app, inspector::Editor::Output);
    let preview = h.app.cam;
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let frame = h.app.session.render_job_handle(job).unwrap().frame().unwrap().unwrap();
    let draw = |app: &mut SolveApp| {
        let mut out = ctx.run_ui(
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))), ..Default::default() },
            |ui| show(app, ui),
        );
        out.textures_delta.clear();
        out
    };
    let out = draw(&mut h.app);
    let id = h.app.render_workspace.image.as_ref().unwrap().1.as_ref().unwrap().id();
    let current = h.app.tr("render.result_current");
    assert!(crate::test_support::text_shapes(&out).any(|t| t.galley.text() == current));
    let identity = format!("{}:{}:{}", job.session, job.serial, job.generation);
    assert!(crate::test_support::text_shapes(&out).any(|t| t.galley.text().contains(&identity)));
    let image_bounds = ctx
        .tessellate(out.shapes, out.pixels_per_point)
        .iter()
        .filter_map(|p| match &p.primitive {
            egui::epaint::Primitive::Mesh(mesh) if mesh.texture_id == id => {
                Some(mesh.vertices.iter().fold(egui::Rect::NOTHING, |r, v| r.union(egui::Rect::from_min_max(v.pos, v.pos))))
            }
            _ => None,
        })
        .fold(egui::Rect::NOTHING, |a, b| a.union(b));
    assert!(image_bounds.width() > 240.0 && image_bounds.height() > 240.0, "{image_bounds:?}");
    assert_eq!(h.app.cam, preview);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    h.app.session.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
    let out = draw(&mut h.app);
    assert!(crate::test_support::text_shapes(&out).any(|t| t.galley.text() == h.app.tr("render.image_stale")));
    assert_eq!(h.app.render_workspace.image_info.as_ref().unwrap().scene_generation, frame.info.scene_generation);
    assert_eq!(h.app.render_workspace.image.as_ref().unwrap().1.as_ref().unwrap().id(), id);
    let path = h.dir.join("result-canvas.png");
    h.app.run("render.export", json!({"job":job,"path":path})).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), frame.png());
}

#[test]
fn result_navigation_uses_shared_pan_zoom_and_leaves_product_camera_unchanged() {
    use egui::{Event, Key, Modifiers, PointerButton, vec2};
    let mut h = Harness::new();
    let ctx = crate::fonts::test_context();
    let job = completed_preview(&mut h, [20, 40, 60]);
    h.app.render_workspace.view.completed = Some(job);
    h.app.render_workspace.view.result = true;
    enter(&mut h.app, true).unwrap();
    inspector::select_editor(&mut h.app, inspector::Editor::Output);
    h.app.ui.perspective = true;
    h.app.preferences.smooth_wheel_zoom = false;
    h.app.preferences.zoom_reverse = false;
    let preview = h.app.cam;
    let draft = serde_json::to_value(&h.app.render_workspace.view.draft).unwrap();
    let dirty = h.app.render_workspace.view.dirty;
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let mut time = 0.0;
    nav_frame(&mut h.app, &ctx, true, &mut time, Modifiers::NONE, vec![]);
    let fitted = h.app.render_workspace.view.result_camera;
    let p = h.app.viewport.rect.unwrap().center() + vec2(80.0, 20.0);
    nav_frame(&mut h.app, &ctx, true, &mut time, Modifiers::NONE, vec![Event::PointerMoved(p)]);
    nav_frame(
        &mut h.app,
        &ctx,
        true,
        &mut time,
        Modifiers::NONE,
        vec![Event::MouseWheel {
            source: egui::MouseWheelSource::Unknown,
            unit: egui::MouseWheelUnit::Line,
            delta: vec2(0.0, 3.0),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        }],
    );
    assert!(h.app.render_workspace.view.result_camera.distance < fitted.distance);
    assert_eq!(h.app.render_workspace.view.result_scale, ResultScale::Free);
    let before_pan = h.app.render_workspace.view.result_camera.target;
    let space = |pressed| Event::Key { key: Key::Space, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
    nav_frame(
        &mut h.app,
        &ctx,
        true,
        &mut time,
        Modifiers::NONE,
        vec![space(true), Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }],
    );
    nav_frame(&mut h.app, &ctx, true, &mut time, Modifiers::NONE, vec![Event::PointerMoved(p + vec2(40.0, -20.0))]);
    nav_frame(
        &mut h.app,
        &ctx,
        true,
        &mut time,
        Modifiers::NONE,
        vec![
            Event::PointerButton { pos: p + vec2(40.0, -20.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
            space(false),
        ],
    );
    assert_ne!(h.app.render_workspace.view.result_camera.target, before_pan);
    assert_eq!(h.app.render_workspace.view.result_camera.pitch, fitted.pitch);
    assert_eq!(h.app.render_workspace.view.result_camera.yaw, fitted.yaw);
    let before_key = h.app.render_workspace.view.result_camera.target;
    assert!(crate::navigation::view_key(&mut h.app, &ctx, Key::ArrowRight, Modifiers::CTRL, false));
    assert_ne!(h.app.render_workspace.view.result_camera.target, before_key);
    assert!(!crate::navigation::view_key(&mut h.app, &ctx, Key::ArrowRight, Modifiers::NONE, false));
    for command in ["view.fit", "view.home"] {
        h.app.render_workspace.view.result_camera.target.x += 20.0;
        h.app.run(command, json!({})).unwrap();
        nav_frame(&mut h.app, &ctx, true, &mut time, Modifiers::NONE, vec![]);
        assert_eq!(h.app.render_workspace.view.result_camera, fitted);
    }
    assert_eq!(h.app.cam, preview);
    assert!(h.app.ui.perspective);
    assert_eq!(serde_json::to_value(&h.app.render_workspace.view.draft).unwrap(), draft);
    assert_eq!(h.app.render_workspace.view.dirty, dirty);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    assert_eq!(image_params(&h.app).unwrap()["settings"]["camera"], json!(preview));
}

#[test]
fn clicked_result_controls_switch_canvas_and_frame_without_changing_the_setup() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":1400}));
    let job = completed_preview(&mut h, [20, 40, 60]);
    h.app.render_workspace.view.job = Some(job);
    enter(&mut h.app, true).unwrap();
    inspector::select_editor(&mut h.app, inspector::Editor::Output);
    h.frames(3);
    let camera = h.app.cam;
    let doc = h.app.session.doc.clone();
    let result = h.app.tr("render.output_view_image").to_string();
    h.step(&json!({"widget":result})).unwrap();
    h.frames(2);
    assert!(h.app.render_workspace.view.result);
    h.step(&json!({"widget":h.app.tr("render.result_actual")})).unwrap();
    h.frames(2);
    assert_eq!(h.app.render_workspace.view.result_scale, ResultScale::Actual);
    let fit = h.app.tr("render.output_fit").to_string();
    h.step(&json!({"widget":fit})).unwrap();
    h.frames(2);
    assert_eq!(h.app.render_workspace.view.result_scale, ResultScale::Fit);
    let preview = h.app.tr("render.state_model").to_string();
    h.step(&json!({"widget":preview})).unwrap();
    assert!(!h.app.render_workspace.view.result);
    assert_eq!(h.app.cam, camera);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
}

#[test]
fn image_method_requests_never_fall_back_or_mix_cpu_and_gpu_options() {
    let mut h = Harness::new();
    enter(&mut h.app, true).unwrap();
    inspector::select_editor(&mut h.app, inspector::Editor::Output);
    h.app.render_workspace.view.result = true;
    h.app.render_workspace.view.result_camera.distance = h.app.cam.distance * 3.0;
    h.app.render_workspace.view.raytrace.exposure_ev = f64::NAN;
    h.app.render_workspace.view.samples = 256;
    h.app.render_workspace.view.max_depth = 12;
    h.app.render_workspace.view.seed = 42;
    h.app.render_workspace.view.draft.exposure_ev = 0.0;
    h.app.render_workspace.view.studio.environment = studio::Environment::Hdr;
    h.app.render_workspace.view.studio.hdr = hdr::Input::FilePending { path: "never-opened.hdr".into() };
    h.app.render_workspace.view.studio.denoise = true;
    h.app.render_workspace.view.studio.lens_enabled = true;
    h.app.render_workspace.view.cpu.environment = cpu::Environment::Hdr;
    h.app.render_workspace.view.cpu.hdr = hdr::Input::FilePending { path: "never-opened-cpu.hdr".into() };
    let params = image_params(&h.app).unwrap();
    assert_eq!(params["settings"]["camera"], json!(h.app.cam));
    assert_eq!(params["settings"]["exposure_ev"], 0.0);
    for key in ["progressive", "samples", "max_depth", "seed", "integrator", "raytrace", "environment", "lighting", "tone_map", "lens", "denoise"] {
        assert!(params.get(key).is_none(), "raster must omit {key}");
    }
    assert!(image_host(&h.app, ImageMethod::GpuRaytrace).is_err());
    select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::GpuRaytrace);
    assert!(image_params(&h.app).is_err(), "missing GPU cannot silently render on CPU");
    h.app.render_workspace.view.method = ImageMethod::CpuRaytrace;
    assert!(image_params(&h.app).is_err(), "unsupported CPU options or host must be refused");
    h.app.render_workspace.view.method = ImageMethod::Raster;
    h.app.session.set_render_job_host(None).unwrap();
    assert!(image_params(&h.app).is_err());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn new_render_setups_use_available_cpu_progressive_without_saving_or_starting() {
    use solidcraft_engine::render::job::{CpuHost, CpuProgressiveOptions};
    let mut h = Harness::new();
    h.app.session.set_render_job_host(Some(CpuHost::for_platform().unwrap())).unwrap();
    let available = image_host(&h.app, ImageMethod::CpuRaytrace).is_ok();
    let before = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let size = (h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height);
    setups::load(&mut h.app);
    assert_eq!(h.app.render_workspace.view.method, if available { ImageMethod::CpuRaytrace } else { ImageMethod::Raster });
    assert_eq!(h.app.render_workspace.view.raytrace.progressive, available.then(CpuProgressiveOptions::default));
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), size);
    assert_eq!(h.app.render_workspace.view.raytrace.budget, solidcraft_engine::render::job::CpuBudget::Compact);
    assert!(h.app.render_workspace.view.job.is_none());
    assert!(!h.app.render_workspace.view.dirty);
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);

    // The preference preserves the previous defaults even with a supported native host.
    h.app.preferences.render_legacy_image_defaults = true;
    setups::load(&mut h.app);
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::Raster);
    assert!(h.app.render_workspace.view.raytrace.progressive.is_none());
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn unsaved_render_defaults_survive_model_edits_and_preserve_local_drafts() {
    use solidcraft_engine::render::job::{CpuHost, CpuProgressiveOptions};
    let mut h = Harness::new();
    h.app.session.set_render_job_host(Some(CpuHost::for_platform().unwrap())).unwrap();
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5,"body_name":"Subject"})).unwrap();
    let available = image_host(&h.app, ImageMethod::CpuRaytrace).is_ok();
    enter(&mut h.app, true).unwrap();
    let expected_method = if available { ImageMethod::CpuRaytrace } else { ImageMethod::Raster };
    let expected_sampling = available.then(CpuProgressiveOptions::default);
    assert_eq!(h.app.render_workspace.view.method, expected_method);
    assert_eq!(h.app.render_workspace.view.raytrace.progressive, expected_sampling);

    let before = h.app.session.undo.len();
    h.app.run("appearance.assign", json!({"bodies":["Subject"],"color":"#123456"})).unwrap();
    let edited = h.app.session.doc.clone();
    setups::sync_revision(&mut h.app);
    assert_eq!(h.app.render_workspace.view.method, expected_method, "a model edit must not replace the unsaved CPU baseline with Raster");
    assert_eq!(h.app.render_workspace.view.raytrace.progressive, expected_sampling);
    assert!(h.app.session.doc.render.setups.is_empty());
    assert!(Arc::ptr_eq(&edited, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), before + 1, "only the appearance edit creates undo history");

    // Defaults apply when opening a new setup, never retrospectively to this unsaved choice.
    h.app.preferences.render_legacy_image_defaults = true;
    h.app.session.set_render_job_host(None).unwrap();
    h.app.render_workspace.view.draft.exposure_ev = 1.25;
    let sampling = expected_sampling.map(|options| CpuProgressiveOptions { samples_per_pixel: 24, ..options });
    h.app.render_workspace.view.raytrace.progressive = sampling;
    setups::refresh_dirty(&mut h.app);
    assert!(h.app.render_workspace.view.dirty);
    h.app.run("appearance.assign", json!({"bodies":["Subject"],"color":"#654321"})).unwrap();
    let edited = h.app.session.doc.clone();
    setups::sync_revision(&mut h.app);
    assert_eq!(h.app.render_workspace.view.method, expected_method);
    assert_eq!(h.app.render_workspace.view.raytrace.progressive, sampling);
    assert_eq!(h.app.render_workspace.view.draft.exposure_ev, 1.25);
    assert!(h.app.render_workspace.view.dirty);
    assert!(h.app.session.doc.render.setups.is_empty());
    assert!(Arc::ptr_eq(&edited, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), before + 2);
    assert!(h.app.render_workspace.view.job.is_none());
    setups::load(&mut h.app);
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::Raster, "the next explicit new load follows the changed defaults");
    assert!(h.app.render_workspace.view.raytrace.progressive.is_none());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn first_numeric_edit_persists_the_unsaved_image_route_once_without_saving_navigation() {
    use solidcraft_engine::doc::render_settings::RenderSetup;
    use solidcraft_engine::render::job::CpuHost;
    let mut h = Harness::new();
    h.app.session.set_render_job_host(Some(CpuHost::for_platform().unwrap())).unwrap();
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5,"body_name":"Subject"})).unwrap();
    let available = image_host(&h.app, ImageMethod::CpuRaytrace).is_ok();
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.draft.width = 32;
    h.app.render_workspace.view.draft.height = 24;
    h.app.render_workspace.view.draft.background = [3, 4, 5];
    let before = setups::draft(&h.app).unwrap();
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let camera = h.app.cam;
    setups::persist_editor(&mut h.app, &before).unwrap();
    assert!(Arc::ptr_eq(&document, &h.app.session.doc), "opening an editor never creates a setup");
    assert_eq!(h.app.session.undo.len(), undo);
    if available {
        h.app.render_workspace.view.draft.exposure_ev = 1.25;
    } else {
        h.app.render_workspace.view.draft.width = 128;
    }
    let expected = setups::draft(&h.app).unwrap();
    setups::persist_editor(&mut h.app, &before).unwrap();
    let saved = h.app.session.doc.render.setups["Studio"].clone();
    assert_eq!(saved.job, expected.job, "the first numerical patch must carry the intended CPU/progressive route");
    assert_eq!(saved.exposure_ev, expected.exposure_ev);
    assert_eq!((saved.width, saved.height), (expected.width, expected.height));
    assert_eq!(saved.background, [3, 4, 5], "first creation preserves the visible baseline, not engine defaults");
    assert_eq!(saved.camera, RenderSetup::default().camera, "numeric edits retain the explicit Update shot camera policy");
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(h.app.render_workspace.view.job.is_none());
    setups::sync_revision(&mut h.app);
    setups::load(&mut h.app);
    assert_eq!(h.app.render_workspace.view.draft, saved);
    assert_eq!(
        h.app.render_workspace.view.raytrace.progressive,
        expected.job.and_then(|job| match job {
            solidcraft_engine::doc::render_settings::RenderJob::CpuRaytrace { options, .. } => options.progressive,
            _ => None,
        })
    );
    assert_eq!(h.app.session.undo.len(), undo + 1);

    // Undo of an actual saved setup is still a removal, not an active unsaved baseline.
    h.app.preferences.render_legacy_image_defaults = true;
    h.app.run("edit.undo", json!({})).unwrap();
    setups::sync_revision(&mut h.app);
    assert!(h.app.session.doc.render.setups.is_empty());
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::Raster);
    assert!(h.app.render_workspace.view.raytrace.progressive.is_none());
}

#[test]
fn render_image_defaults_preserve_offline_and_saved_setups_and_explicit_sampling() {
    use solidcraft_engine::doc::render_settings::{CpuJob, RenderJob, RenderSetup};
    use solidcraft_engine::render::job::{CpuHost, CpuProgressiveOptions};
    let mut h = Harness::new();
    for host in [None, Some(CpuHost::cooperative())] {
        h.app.session.set_render_job_host(host).unwrap();
        setups::load(&mut h.app);
        assert_eq!(h.app.render_workspace.view.method, ImageMethod::Raster);
        assert!(h.app.render_workspace.view.raytrace.progressive.is_none());
    }
    let progressive = CpuProgressiveOptions { samples_per_pixel: 32, seed: 81 };
    for job in [
        None,
        Some(RenderJob::Raster { transparent_background: true }),
        Some(RenderJob::CpuRaytrace { transparent_background: false, options: Box::new(CpuJob { primary_samples: 4, ..Default::default() }) }),
        Some(RenderJob::CpuRaytrace {
            transparent_background: false,
            options: Box::new(CpuJob { progressive: Some(progressive), ..Default::default() }),
        }),
    ] {
        let setup = RenderSetup { width: 32, height: 24, job, ..Default::default() };
        h.app.run("render.settings.set", json!({"name":"Saved","set":setup})).unwrap();
        h.app.render_workspace.view.name = "Saved".into();
        let before = h.app.session.doc.clone();
        let undo = h.app.session.undo.len();
        setups::load(&mut h.app);
        assert_eq!(h.app.render_workspace.view.draft, setup);
        assert!(Arc::ptr_eq(&before, &h.app.session.doc));
        assert_eq!(h.app.session.undo.len(), undo);
        assert!(h.app.render_workspace.view.job.is_none());
        if let Some(RenderJob::CpuRaytrace { options, .. }) = setup.job {
            assert_eq!(h.app.render_workspace.view.raytrace.progressive, options.progressive);
            assert_eq!(h.app.render_workspace.view.raytrace.primary_samples, options.primary_samples);
        } else {
            assert_eq!(h.app.render_workspace.view.method, ImageMethod::Raster);
        }
    }

    // Selecting the CPU method is an explicit new choice. It supplies sampling but retains
    // existing progressive intent; legacy preferences retain the fixed route and spatial AA.
    let mut h = Harness::new();
    h.app.render_workspace.view.raytrace.progressive = None;
    h.app.render_workspace.view.raytrace.primary_samples = 16;
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    assert_eq!(h.app.render_workspace.view.raytrace.progressive, Some(CpuProgressiveOptions::default()));
    assert_eq!(h.app.render_workspace.view.raytrace.primary_samples, 1);
    h.app.render_workspace.view.raytrace.progressive = Some(progressive);
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    assert_eq!(h.app.render_workspace.view.raytrace.progressive, Some(progressive));
    h.app.preferences.render_legacy_image_defaults = true;
    h.app.render_workspace.view.raytrace.progressive = None;
    h.app.render_workspace.view.raytrace.primary_samples = 16;
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    assert!(h.app.render_workspace.view.raytrace.progressive.is_none());
    assert_eq!(h.app.render_workspace.view.raytrace.primary_samples, 16);
}

fn sampling_policy_fixture(fixed: bool) -> Harness {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2600}));
    enter(&mut h.app, true).unwrap();
    if fixed {
        h.app
            .run(
                "render.settings.set",
                json!({"name":"Sampling intent","set":{"width":64,"height":32,"exposure_ev":1.5,
            "job":{"method":"cpu_raytrace","options":{"primary_samples":4,"shadows":false,"lights":[]}}}}),
            )
            .unwrap();
        h.app.render_workspace.view.name = "Sampling intent".into();
        setups::load(&mut h.app);
    } else {
        select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
        h.app.render_workspace.view.draft.width = 64;
        h.app.render_workspace.view.draft.height = 32;
    }
    h.app.render_workspace.view.live_paused = true;
    inspector::select_editor(&mut h.app, inspector::Editor::Sampling);
    h.frames(3);
    if crate::scenario::rect_of("render:cpu-spp").is_none() {
        h.step(&json!({"click":{"handle":format!("dialog:section:{}",h.app.tr("render.children.cpu"))}})).unwrap();
    }
    h.frames(2);
    h
}

#[test]
fn normal_cpu_progressive_sampling_is_status_without_an_off_selector_or_paint_mutation() {
    let mut h = sampling_policy_fixture(false);
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let before = h.app.render_workspace.view.raytrace.clone();
    h.frames(3);
    assert!(crate::scenario::rect_of("render:cpu-sampling").is_some());
    assert!(crate::scenario::rect_of("render:cpu-spp-field").is_some());
    assert!(crate::scenario::rect_of("render:cpu-fixed").is_none());
    assert!(crate::scenario::rect_of("render:cpu-adopt-progressive").is_none());
    assert!(crate::scenario::rect_of("render:aa").is_none());
    assert_eq!(h.app.render_workspace.view.raytrace, before);
    assert!(before.progressive.is_some());
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
fn saved_fixed_cpu_sampling_requires_explicit_adoption_and_keeps_other_intent() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    let mut h = sampling_policy_fixture(true);
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let before = setups::snapshot(&h.app).unwrap();
    let camera = h.app.cam;
    h.frames(3);
    assert!(h.app.render_workspace.view.raytrace.progressive.is_none());
    assert_eq!(setups::snapshot(&h.app).unwrap(), before);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert!(crate::scenario::rect_of("render:cpu-fixed").is_none());
    h.step(&json!({"click":{"handle":"render:cpu-adopt-progressive"}})).unwrap();
    h.frames(3);
    let saved = &h.app.session.doc.render.setups["Sampling intent"];
    assert_eq!((saved.width, saved.height, saved.exposure_ev), (64, 32, 1.5));
    let Some(RenderJob::CpuRaytrace { options, .. }) = &saved.job else { panic!("CPU method must stay selected") };
    assert_eq!(options.primary_samples, 1);
    assert!(options.progressive.is_some());
    assert!(!options.shadows);
    assert_eq!(options.lights, Some(Vec::new()));
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::CpuRaytrace);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(h.app.render_workspace.view.job.is_none());
}

#[test]
fn fixed_cpu_adoption_refuses_reflection_or_size_changes_instead_of_rewriting_them() {
    use solidcraft_engine::render::job::{ReflectionBody, ReflectionMode, ReflectionOptions};
    let mut h = sampling_policy_fixture(true);
    let reflection =
        ReflectionOptions { bodies: vec![ReflectionBody { body: "Body".into(), mode: ReflectionMode::RoughSpecular }], samples: 64, max_bounces: 2 };
    h.app.render_workspace.view.raytrace.reflections = Some(reflection.clone());
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:cpu-adopt-progressive"}})).unwrap();
    assert!(h.app.render_workspace.view.raytrace.progressive.is_none());
    assert_eq!(h.app.render_workspace.view.raytrace.primary_samples, 4);
    assert_eq!(h.app.render_workspace.view.raytrace.reflections, Some(reflection));
    h.app.render_workspace.view.raytrace.reflections = None;
    h.app.render_workspace.view.draft.width = 2048;
    h.app.render_workspace.view.draft.height = 2048;
    h.frames(3);
    let document = h.app.session.doc.clone();
    h.step(&json!({"click":{"handle":"render:cpu-adopt-progressive"}})).unwrap();
    assert!(h.app.render_workspace.view.raytrace.progressive.is_none());
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (2048, 2048));
    assert_eq!(h.app.render_workspace.view.raytrace.primary_samples, 4);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
}

#[test]
fn explicit_fixed_cpu_sampling_survives_method_switches_reselection_and_saved_reload() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2600}));
    enter(&mut h.app, true).unwrap();
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    assert!(h.app.render_workspace.view.raytrace.progressive.is_some());
    inspector::select_editor(&mut h.app, inspector::Editor::Sampling);
    h.app.preferences.render_legacy_image_defaults = true;
    h.frames(3);
    h.step(&json!({"click":{"handle":format!("dialog:section:{}",h.app.tr("render.children.cpu"))}})).unwrap();
    h.step(&json!({"click":{"handle":"render:cpu-fixed"}})).unwrap();
    assert!(h.app.render_workspace.view.raytrace.progressive.is_none());
    h.app.render_workspace.view.raytrace.primary_samples = 4;
    for method in [ImageMethod::CpuRaytrace, ImageMethod::GpuRaytrace, ImageMethod::CpuRaytrace, ImageMethod::Raster, ImageMethod::CpuRaytrace] {
        select_image_method(&mut h.app, method);
        assert!(h.app.render_workspace.view.raytrace.progressive.is_none(), "switching methods must preserve an explicit fixed choice");
        assert_eq!(h.app.render_workspace.view.raytrace.primary_samples, 4);
    }
    setups::save(&mut h.app).unwrap();
    setups::load(&mut h.app);
    for method in [ImageMethod::CpuRaytrace, ImageMethod::GpuRaytrace, ImageMethod::CpuRaytrace] {
        select_image_method(&mut h.app, method);
        assert!(h.app.render_workspace.view.raytrace.progressive.is_none(), "a saved fixed setup must not be converted on reselect");
        assert_eq!(h.app.render_workspace.view.raytrace.primary_samples, 4);
    }
    assert!(h.app.render_workspace.view.job.is_none());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_cpu_controls_check_and_start_the_same_capture_and_keep_it_exportable() {
    use solidcraft_engine::render::job::{CpuHost, Phase};
    let mut h = Harness::new();
    h.app.preferences.render_legacy_image_defaults = true;
    let host = CpuHost::for_platform().unwrap();
    h.app.session.set_render_job_host(Some(host.clone())).unwrap();
    h.app.render_workspace.image_host = Some(host);
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5})).unwrap();
    h.call("ui.resize", json!({"width":1600,"height":1800}));
    enter(&mut h.app, true).unwrap();
    inspector::select_editor(&mut h.app, inspector::Editor::Output);
    // Keep the small capture size in the saved setup across editor autosaves.
    let name = h.app.render_workspace.view.name.clone();
    h.app.run("render.settings.set", json!({"name":name,"set":{"width":16,"height":16}})).unwrap();
    h.frames(3);
    let available = image_host(&h.app, ImageMethod::CpuRaytrace).is_ok();
    h.step(&json!({"click":{"handle":"render:method"}})).unwrap();
    h.step(&json!({"click":{"handle":"render:method-cpu"}})).unwrap();
    if !available {
        assert_eq!(h.app.render_workspace.view.method, ImageMethod::CpuRaytrace);
        assert!(image_params(&h.app).is_err());
        return;
    }
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::CpuRaytrace);
    h.app.run("view.render_select", json!({"kind":"sampling"})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":format!("dialog:section:{}", h.app.tr("render.children.cpu"))}})).unwrap();
    h.step(&json!({"click":{"handle":"render:aa"}})).unwrap();
    h.step(&json!({"click":{"handle":"render:aa-4"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.raytrace.primary_samples, 4);
    h.app.render_workspace.view.raytrace.preset = Some(LightingPreset::WarmCool);
    h.app.render_workspace.view.draft.exposure_ev = 1.0;
    let numeric = image_params(&h.app).unwrap();
    assert_eq!(numeric["settings"]["job"]["method"], "cpu_raytrace");
    assert!(numeric.get("raytrace").is_none());

    click_render_check(&mut h);
    assert_eq!(h.app.render_workspace.view.cpu.checked.as_ref().unwrap().request, numeric);
    let reflected = crate::open_meshing::session_world(&h.app.session).bodies.first().unwrap().name.clone();
    h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body { name: reflected }]})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":format!("dialog:section:{}", h.app.tr("render.appearance_editor.reflections"))}})).unwrap();
    h.step(&json!({"widget":h.app.tr("render.cpu_reflection_mirror")})).unwrap();
    assert_eq!(
        h.app.render_workspace.view.raytrace.reflections.as_ref().unwrap().bodies[0].mode,
        solidcraft_engine::render::job::ReflectionMode::Mirror
    );
    h.app.run("view.render_select", json!({"kind":"output"})).unwrap();
    h.frames(3);
    let hdr_path = h.dir.join("cpu-captured.hdr");
    let mut hdr = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 1 +X 2\n".to_vec();
    hdr.extend_from_slice(&[128, 128, 128, 129, 128, 128, 128, 129]);
    std::fs::write(&hdr_path, &hdr).unwrap();
    h.app.render_workspace.view.cpu.environment = cpu::Environment::Hdr;
    hdr::inspect(&mut h.app, hdr::Target::Cpu, hdr_path.to_string_lossy().into_owned()).unwrap();
    h.app.render_workspace.view.cpu.samples = 4;
    h.app.render_workspace.view.result = true;
    h.app.render_workspace.view.result_camera.distance = 42.0;
    h.frames(2);
    let camera = h.app.cam;
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let params = image_params(&h.app).unwrap();
    let preflight = h.app.session.execute("render.preflight", &params).unwrap();
    assert_eq!(preflight["resolved"]["executor"], "cpu-raytrace");

    click_render_check(&mut h);
    assert_eq!(image_params(&h.app).unwrap(), params);
    let checked = h.app.render_workspace.view.cpu.checked.as_ref().unwrap();
    assert_eq!(checked.request, params);
    assert_eq!(checked.environment, preflight["environment"]);

    h.step(&json!({"click":{"handle":"render:stage:render.start"}})).unwrap();
    let id = h.app.render_workspace.view.job.unwrap();
    let handle = h.app.session.render_job_handle(id).unwrap();
    // The job owns the source bytes captured by Start, even if the file changes.
    std::fs::write(&hdr_path, b"changed after capture").unwrap();
    let status = handle.wait_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(json!(status.environment), preflight["environment"]);
    assert_eq!(status.progress.phase, Phase::Completed);
    assert_eq!(status.request.integrator, Integrator::Raytrace);
    assert_eq!(json!(status.request.camera), params["settings"]["camera"]);
    assert_eq!(json!(status.request.raytrace), params["raytrace"]);
    assert_eq!(status.request.width, 16);
    assert_eq!(status.request.height, 16);
    h.frames(3);
    assert_eq!(h.app.render_workspace.view.completed, Some(id));
    assert_eq!(h.app.cam, camera);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    let frame = handle.frame().unwrap().unwrap();
    assert_eq!(frame.info.executor.as_str(), "cpu-raytrace");
    let path = h.dir.join("cpu-result-canvas.png");
    h.app.run("render.export", json!({"job":id,"path":path})).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), frame.png());
    h.app.render_workspace.view.transparent = true;
    assert_eq!(image_params(&h.app).unwrap()["transparent_background"], true);
    h.app.render_workspace.view.transparent = false;
    h.app.render_workspace.view.raytrace.primary_samples = 3;
    assert!(image_params(&h.app).is_err());
    h.app.render_workspace.view.raytrace.primary_samples = 4;
    h.app.render_workspace.view.raytrace.area_lights = Some(Vec::new());
    assert_eq!(image_params(&h.app).unwrap()["raytrace"]["area_lights"], json!([]));
}

fn async_export_fixture() -> (Harness, solidcraft_engine::render::job::JobId) {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5})).unwrap();
    h.app.session.mark_saved();
    let job = completed_preview(&mut h, [200, 210, 220]);
    (h, job)
}

#[test]
fn delayed_image_export_holds_its_document_without_changing_history() {
    use crate::picker_test_support::{Slot, save};
    let (mut h, job) = async_export_fixture();
    let path = h.dir.join("delayed.png");
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let ctx = crate::fonts::test_context();
    let slot: Slot = Default::default();
    save(&mut h.app, &slot);
    export_image(&mut h.app, job, false);
    crate::file_picker::tick(&mut h.app, &ctx);
    assert!(h.app.file_picker_busy() && !path.exists());
    assert_eq!(h.call("ui.inspect", json!({}))["result"]["file_dialog_open"], true);
    crate::documents::new_design(&mut h.app);
    let other = h.app.session.doc.clone();
    *slot.borrow_mut() = Some(Some(path.to_string_lossy().into_owned()));
    crate::file_picker::tick(&mut h.app, &ctx);
    assert_eq!(h.app.docs.active, 0, "upstream holds tab changes while a picker is pending");
    assert_eq!(h.call("ui.inspect", json!({}))["result"]["file_dialog_open"], false);
    assert!(Arc::ptr_eq(&other, &h.app.session.doc));
    assert_eq!(&std::fs::read(&path).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
    crate::documents::switch(&mut h.app, 0);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(!h.app.session.is_dirty());
}

#[test]
fn delayed_image_export_cancel_and_overwrite_choice_preserve_existing_files() {
    use crate::picker_test_support::{Slot, save};
    let (mut h, job) = async_export_fixture();
    let path = h.dir.join("existing.png");
    std::fs::write(&path, b"existing contents").unwrap();
    let doc = h.app.session.doc.clone();
    let ctx = crate::fonts::test_context();
    let slot: Slot = Default::default();
    save(&mut h.app, &slot);
    export_image(&mut h.app, job, false);
    crate::file_picker::tick(&mut h.app, &ctx);
    *slot.borrow_mut() = Some(None);
    crate::file_picker::tick(&mut h.app, &ctx);
    assert!(!h.app.file_picker_busy());
    assert_eq!(std::fs::read(&path).unwrap(), b"existing contents");
    export_image(&mut h.app, job, false);
    crate::file_picker::tick(&mut h.app, &ctx);
    h.app.render_workspace.view.overwrite = true;
    *slot.borrow_mut() = Some(Some(path.to_string_lossy().into_owned()));
    crate::file_picker::tick(&mut h.app, &ctx);
    assert_eq!(std::fs::read(&path).unwrap(), b"existing contents", "a later checkbox change is not overwrite consent for the pending export");
    assert!(matches!(&h.app.status, Some((_, _, true))));
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.execute("render.status", &json!({"job":job})).unwrap()["progress"]["phase"], "completed");
}

#[test]
fn delayed_image_export_never_substitutes_a_newer_retained_image() {
    use crate::picker_test_support::{Slot, save};
    let (mut h, job) = async_export_fixture();
    let path = h.dir.join("old-image.png");
    let bytes = h.app.session.render_job_handle(job).unwrap().frame().unwrap().unwrap().png().to_vec();
    let ctx = crate::fonts::test_context();
    let slot: Slot = Default::default();
    save(&mut h.app, &slot);
    export_image(&mut h.app, job, false);
    crate::file_picker::tick(&mut h.app, &ctx);
    let newer = completed_preview(&mut h, [20, 30, 40]);
    h.app.render_workspace.view.completed = Some(newer);
    *slot.borrow_mut() = Some(Some(path.to_string_lossy().into_owned()));
    crate::file_picker::tick(&mut h.app, &ctx);
    assert_eq!(std::fs::read(&path).unwrap(), bytes, "an older retained image remains the exact pending export source");
    assert_eq!(h.app.render_workspace.view.completed, Some(newer));
    let evicted = h.dir.join("evicted.png");
    export_image(&mut h.app, job, false);
    crate::file_picker::tick(&mut h.app, &ctx);
    for _ in 0..7 {
        completed_preview(&mut h, [50, 60, 70]);
    }
    *slot.borrow_mut() = Some(Some(evicted.to_string_lossy().into_owned()));
    crate::file_picker::tick(&mut h.app, &ctx);
    assert!(!evicted.exists(), "an evicted requested image fails without substituting a newer image");
    assert!(matches!(&h.app.status, Some((_, _, true))));
    let newer_path = h.dir.join("new-image.png");
    h.app.run("render.export", json!({"job":newer,"path":newer_path})).unwrap();
    assert!(newer_path.is_file());
}

#[test]
fn delayed_image_export_holds_close_and_new_document_until_cancel() {
    use crate::picker_test_support::{Slot, save};
    let (mut h, job) = async_export_fixture();
    let path = h.dir.join("closed.png");
    let ctx = crate::fonts::test_context();
    let slot: Slot = Default::default();
    save(&mut h.app, &slot);
    export_image(&mut h.app, job, false);
    crate::file_picker::tick(&mut h.app, &ctx);
    crate::documents::new_design(&mut h.app);
    crate::documents::close(&mut h.app, 0, true);
    let other = h.app.session.doc.clone();
    assert_eq!(h.app.docs.count(), 1);
    assert!(h.app.file_picker_busy());
    *slot.borrow_mut() = Some(None);
    crate::file_picker::tick(&mut h.app, &ctx);
    assert!(!path.exists() && !h.app.file_picker_busy());
    assert!(Arc::ptr_eq(&other, &h.app.session.doc));
}

#[test]
fn delayed_image_export_refuses_an_internally_replaced_document_even_with_overwrite() {
    use crate::picker_test_support::{Slot, save};
    for overwrite in [false, true] {
        let (mut h, job) = async_export_fixture();
        let path = h.dir.join("keep.png");
        std::fs::write(&path, b"keep this file").unwrap();
        let slot: Slot = Default::default();
        save(&mut h.app, &slot);
        export_image(&mut h.app, job, overwrite);
        assert!(h.app.run("appearance.assign", json!({"bodies":["Body1"],"color":[1,2,3]})).is_err());
        // Internal host/session changes are outside the input hold; never export them by accident.
        h.app.session.execute("appearance.assign", &json!({"bodies":["Body1"],"color":[1,2,3]})).unwrap();
        let changed = h.app.session.doc.clone();
        let history = (h.app.session.undo.len(), h.app.session.revision);
        *slot.borrow_mut() = Some(Some(path.to_string_lossy().into_owned()));
        crate::file_picker::tick(&mut h.app, &crate::fonts::test_context());
        assert_eq!(std::fs::read(path).unwrap(), b"keep this file");
        assert!(Arc::ptr_eq(&changed, &h.app.session.doc));
        assert_eq!((h.app.session.undo.len(), h.app.session.revision), history);
        assert!(matches!(&h.app.status, Some((_, _, true))));
    }
}

/// One frame of either the modelling viewport or the Render preview, over the same rectangle.
fn nav_frame(app: &mut crate::SolveApp, ctx: &egui::Context, render: bool, time: &mut f64, mods: egui::Modifiers, events: Vec<egui::Event>) {
    *time += 1.0 / 60.0;
    let events = std::iter::once(egui::Event::ModifiersChanged(mods)).chain(events).collect();
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
        time: Some(*time),
        focused: true,
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        if render {
            show(app, ui);
        } else {
            crate::viewport::show(app, ui);
        }
    });
    out.textures_delta.clear();
}

fn nav_app(render: bool, preset: &str, around: crate::prefs::OrbitAround) -> crate::SolveApp {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":40,"width":30,"height":20,"body_name":"Product"})).unwrap();
    let mut app = crate::SolveApp::new(s, crate::Services::default());
    if render {
        enter(&mut app, true).unwrap();
        assert!(active(&app));
    }
    app.preferences.nav = preset.into();
    app.preferences.orbit_around = around;
    app.preferences.zoom_to_cursor = true;
    app.ui.perspective = true;
    let mut cam = Camera { fov: 0.6, ..Default::default() };
    cam.set_view(solidcraft_engine::render::StandardView::Iso);
    cam.fit(&solidcraft_engine::product_scene::extract(&app.session).unwrap().bounds);
    app.cam = cam;
    app
}

/// #1454: the same mouse, wheel and modifier input moves the Render preview's camera exactly as
/// it moves the modelling viewport's, under every navigation preset and Orbit around setting.
#[test]
fn render_preview_navigates_like_the_modelling_viewport_in_every_preset() {
    use egui::{Event, Modifiers, PointerButton, pos2, vec2};
    let none = Modifiers::NONE;
    let chords = [none, Modifiers::SHIFT, Modifiers::CTRL, Modifiers::ALT, Modifiers::SHIFT | Modifiers::ALT];
    let arounds = [crate::prefs::OrbitAround::ViewCentre, crate::prefs::OrbitAround::Cursor, crate::prefs::OrbitAround::Origin];
    for preset in crate::prefs::NAV_PRESETS.iter().map(|p| p.id) {
        for around in arounds {
            let (design_ctx, render_ctx) = (crate::fonts::test_context(), crate::fonts::test_context());
            let mut design = nav_app(false, preset, around);
            let mut render = nav_app(true, preset, around);
            let (mut design_time, mut render_time) = (0.0, 0.0);
            let mut both = |events: Vec<Event>, mods: Modifiers, design: &mut crate::SolveApp, render: &mut crate::SolveApp| {
                nav_frame(design, &design_ctx, false, &mut design_time, mods, events.clone());
                nav_frame(render, &render_ctx, true, &mut render_time, mods, events);
            };
            both(vec![], none, &mut design, &mut render);
            assert_eq!(design.viewport.rect, render.viewport.rect);
            // Presses start on empty space in both workspaces, and the wheel
            // turns over the model, so zoom to the cursor anchors on its surface.
            let empty = pos2(90.0, 520.0);
            let proj = crate::viewport::projection(&design, design.viewport.rect.unwrap());
            assert!(crate::viewport::pick(&design, &proj, empty).is_empty());
            let model = proj.to_screen(design.cam.target).unwrap();
            assert!(!crate::viewport::pick(&design, &proj, model).is_empty());
            let mut moved = 0;
            for button in [PointerButton::Primary, PointerButton::Middle, PointerButton::Secondary] {
                for mods in chords {
                    let before = design.cam;
                    both(vec![Event::PointerMoved(empty)], mods, &mut design, &mut render);
                    both(vec![Event::PointerButton { pos: empty, button, pressed: true, modifiers: mods }], mods, &mut design, &mut render);
                    let mut at = empty;
                    for _ in 0..5 {
                        at += vec2(14.0, -9.0);
                        both(vec![Event::PointerMoved(at)], mods, &mut design, &mut render);
                    }
                    both(vec![Event::PointerButton { pos: at, button, pressed: false, modifiers: mods }], mods, &mut design, &mut render);
                    for _ in 0..30 {
                        both(vec![], none, &mut design, &mut render);
                    }
                    assert_eq!(design.cam, render.cam, "{preset} {around:?}: {button:?} drag with {mods:?}");
                    moved += usize::from(design.cam != before);
                    // Let any snap settle and start the next gesture from the same view.
                    design.cam = render.cam;
                }
            }
            assert!(moved >= 2, "{preset}: the preset's drags move the camera ({moved})");
            // Wheel over the model, then modifier-only motion (Touchpad: Shift pans, Alt orbits).
            let before = design.cam;
            both(vec![Event::PointerMoved(model)], none, &mut design, &mut render);
            let wheel = Event::MouseWheel {
                source: egui::MouseWheelSource::Unknown,
                unit: egui::MouseWheelUnit::Line,
                delta: vec2(0.0, 3.0),
                modifiers: none,
                phase: egui::TouchPhase::Move,
            };
            both(vec![wheel], none, &mut design, &mut render);
            for _ in 0..30 {
                both(vec![], none, &mut design, &mut render);
            }
            assert_ne!(design.cam, before, "{preset}: the wheel zooms");
            assert_eq!(design.cam, render.cam, "{preset} {around:?}: wheel zoom to the cursor");
            for mods in [Modifiers::SHIFT, Modifiers::ALT] {
                both(vec![Event::PointerMoved(empty)], mods, &mut design, &mut render);
                both(vec![Event::PointerMoved(empty + vec2(25.0, 15.0))], mods, &mut design, &mut render);
                both(vec![Event::PointerMoved(empty)], none, &mut design, &mut render);
                assert_eq!(design.cam, render.cam, "{preset} {around:?}: modifier motion with {mods:?}");
            }
            assert!(render.session.selection.is_empty(), "camera navigation must not select");
        }
    }
}

/// #1454: Fit (F6), a middle double-click, and the axis view's and navigation cube's Home and
/// view controls all work in the Render workspace, framing the product scene.
#[test]
fn render_workspace_view_keys_double_middle_click_and_navigators_work() {
    for style in [crate::prefs::NavigatorStyle::Axes, crate::prefs::NavigatorStyle::Cube] {
        let mut h = Harness::new();
        h.app.preferences.navigator_style = style;
        h.app.preferences.look_at_anim = false;
        h.app.run("solid.box", json!({"length":40,"width":30,"height":20,"body_name":"Product"})).unwrap();
        enter(&mut h.app, true).unwrap();
        inspector::select_editor(&mut h.app, inspector::Editor::Output);
        h.frames(3);
        let mut home = h.app.cam;
        home.set_view(solidcraft_engine::render::StandardView::Iso);
        let off = |h: &mut Harness| {
            h.app.cam.yaw += 0.7;
            h.app.cam.pitch = 0.1;
            h.app.cam.distance *= 3.0;
            h.app.cam.target.x += 25.0;
            h.frames(2);
        };
        // F6 fits, keeping the direction.
        off(&mut h);
        let before = h.app.cam;
        h.step(&json!({"key":"F6"})).unwrap();
        h.settle();
        assert_eq!(
            (h.app.cam.yaw, h.app.cam.pitch, h.app.cam.roll, h.app.cam.up),
            (before.yaw, before.pitch, before.roll, before.up),
            "{style:?}: F6 retains orientation"
        );
        assert!(h.app.cam.distance < before.distance, "{style:?}: F6 frames the product");
        assert_product_fits_canvas(&h.app);
        // A middle double-click goes Home.
        off(&mut h);
        let centre = h.app.viewport.rect.unwrap().center();
        h.step(&json!({"click":[centre.x, centre.y], "double":true, "button":"middle"})).unwrap();
        h.settle();
        assert_eq!(
            (h.app.cam.yaw, h.app.cam.pitch, h.app.cam.roll, h.app.cam.up),
            (home.yaw, home.pitch, home.roll, home.up),
            "{style:?}: middle double-click"
        );
        assert_product_fits_canvas(&h.app);
        // The navigator's Home control.
        off(&mut h);
        let handle = match style {
            crate::prefs::NavigatorStyle::Axes => "axis_view:home",
            // Custom draws the cube until a body is chosen.
            crate::prefs::NavigatorStyle::Cube | crate::prefs::NavigatorStyle::Custom => "navigation_cube:control:home",
        };
        h.step(&json!({"click":{"handle":handle}})).unwrap();
        h.settle();
        assert!(
            (h.app.cam.yaw - home.yaw).abs() < 1e-6 && (h.app.cam.pitch - home.pitch).abs() < 1e-6,
            "{style:?}: navigator Home turns to the home view"
        );
        // A standard view from the navigator.
        h.step(&json!({"click":{"handle":"axis_view:z"}})).unwrap();
        h.settle();
        assert!(h.app.cam.back().dot(solidcore_record::Vec3::Z) > 0.999, "{style:?}: view from +Z, got {:?}", h.app.cam.back());
        assert!(h.app.session.selection.is_empty());
    }
}

/// #1464: a maximised 4K or 5K window is past the preview's pixel budget. The preview draws
/// smaller at the same aspect instead of refusing every frame.
#[test]
fn four_and_five_k_viewports_preview_downscaled_within_the_limits() {
    use solidcraft_engine::render::*;
    for samples in [1, 4, 16] {
        let host = host_executor(16384, true, samples).unwrap();
        let mut s = Session::default();
        s.set_render_viewport(Some(host.clone())).unwrap();
        let request = |width, height| RenderRequest {
            backend: None,
            mode: RenderMode::Preview,
            operation: RenderOperation::Viewport,
            width,
            height,
            format: None,
            features: vec![RenderFeature::Pbr],
            policy: Default::default(),
        };
        if samples <= 4 {
            assert_eq!(preview_size(s.render_viewport(), (1920, 1080)), (1920, 1080), "fits: drawn at full size");
        }
        // 16× MSAA's scratch cost brings even 1080p under the budget.
        let sizes: &[(u64, u64)] = if samples == 16 { &[(1920, 1080), (3840, 2160), (5120, 2880)] } else { &[(3840, 2160), (5120, 2880)] };
        for &full in sizes {
            assert!(resolve_request(&[s.render_backend()], &request(full.0, full.1)).is_err(), "{full:?} at full size is over the limit");
            let (w, h) = preview_size(s.render_viewport(), full);
            assert!(w < full.0 && h < full.1 && w * h <= host.limits.max_pixels, "{full:?} -> {w}x{h}");
            assert!((w as f64 / h as f64 - 16.0 / 9.0).abs() < 0.01, "{w}x{h} keeps the aspect");
            let resolved = resolve_request(&[s.render_backend()], &request(w, h)).unwrap();
            assert_eq!(resolved.executor.as_str(), "gpu-viewport");
            assert!(resolved.estimated_scratch_bytes <= host.limits.max_scratch_bytes);
        }
    }
    assert_eq!(preview_size(None, (5120, 2880)), (5120, 2880), "no executor: resolution reports it");
}

/// Backend failures must remain readable in the fixed-width Render panel, not
/// disappear into an ellipsis before the reason and recovery guidance are shown.
#[test]
fn backend_diagnostics_wrap_without_elision_in_bounded_image_controls() {
    fn collect<'a>(shape: &'a egui::Shape, texts: &mut Vec<&'a egui::epaint::TextShape>) {
        match shape {
            egui::Shape::Text(text) => texts.push(text),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, texts)),
            _ => {}
        }
    }
    for mode in ["floating", "compact"] {
        for dark in [false, true] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.ui.dark = dark;
            h.call("ui.resize", json!({"width":1400,"height":1800}));
            h.app.run("view.layout", json!({"mode":mode})).unwrap();
            enter(&mut h.app, true).unwrap();
            let reason = "The selected viewport adapter has no compatible Vulkan path device. Images remain available on the CPU; keep the original design and review the driver before retrying.";
            h.app.render_workspace.path_host = PathHost::Failed(reason.into());
            h.frames(3);
            let expected = format!("{}: {reason}", h.app.tr("render.gpu_path_failed"));
            let mut out = h.ctx.run_ui(
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 1800.0))), ..Default::default() },
                |ui| {
                    h.app.logic(ui.ctx());
                    ui.set_max_width(285.0);
                    image_controls(&mut h.app, ui);
                },
            );
            out.textures_delta.clear();
            let mut texts = Vec::new();
            out.shapes.iter().for_each(|shape| collect(&shape.shape, &mut texts));
            {
                let message = &expected;
                let text = texts.iter().find(|text| text.galley.job.text == *message).expect("the complete diagnostic is painted");
                assert!(!text.galley.elided, "{mode}, dark={dark}: {message}");
                assert!(text.galley.rows.len() > 1, "long messages must wrap in the narrow panel");
                assert!(text.galley.rect.width() <= 285.0, "the message must not expand the panel");
            }
        }
    }
}

#[test]
fn gpu_material_recipes_use_exact_linear_absorption_without_changing_saved_appearance() {
    use solidcraft_engine::doc::appearance_pack::{GlassBoundary, Recipe, STARTER_PACK};
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":3,"body_name":"Optics"})).unwrap();
    h.app.run("appearance.assign", json!({"bodies":["Optics"],"color":"#a0c0e0","opacity":0.25})).unwrap();
    h.app.session.mark_saved();
    let mut target: materials::Target = serde_json::from_value(h.app.run("render.scene", json!({})).unwrap()["bodies"][0].clone()).unwrap();
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    // Exercise the retained ordinal-only adapter; stable Glass capture is covered separately.
    target.stable_target = None;
    h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
    h.app.render_workspace.view.materials.enabled = true;
    let mut count = 0;
    for preset in STARTER_PACK {
        let Recipe::Glass(recipe) = preset.recipe else { continue };
        let mut binding = materials::Binding::new(target.clone(), &h.app);
        binding.glass_enabled = true;
        binding.glass = materials::Glass::recipe(preset.id).unwrap();
        h.app.render_workspace.view.materials.bindings = vec![binding];
        let mut params = json!({"progressive":true});
        materials::params(&h.app, &mut params).unwrap();
        let wire = &params["raytrace"]["dielectrics"][0];
        assert_eq!(wire["body_name"], "Optics");
        assert_eq!(wire["ior"], recipe.dielectric_ior);
        let [r, g, b]: [f64; 3] = serde_json::from_value(wire["absorption_per_mm"].clone()).unwrap();
        let medium = solidcraft_engine::render::transmission::Transmission::new(solidcore_record::Vec3::new(r, g, b), None).unwrap();
        let transmitted = medium.attenuation(recipe.reference_path_mm).unwrap();
        for (actual, expected) in [transmitted.x, transmitted.y, transmitted.z].into_iter().zip(recipe.transmission_linear_rgb) {
            assert!((actual - expected).abs() < 1e-12, "the actual optical consumer must reproduce the recipe's linear transmittance");
        }
        match recipe.boundary {
            GlassBoundary::Closed => {
                assert_eq!(wire["boundary"], "closed_mesh");
                assert_eq!(wire["thickness_mm"], 0.0);
            }
            GlassBoundary::Thin { thickness_mm } => {
                assert_eq!(wire["boundary"], "thin_sheet");
                assert_eq!(wire["thickness_mm"], thickness_mm);
            }
        }
        assert!(wire.get("opacity").is_none());
        count += 1;
    }
    assert_eq!(count, 6);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(!h.app.session.is_dirty());
    h.app.render_workspace.view.method = ImageMethod::Raster;
    assert!(materials::params(&h.app, &mut json!({})).is_err());
    h.app.render_workspace.view.materials.enabled = false;
    let mut raster = json!({});
    materials::params(&h.app, &mut raster).unwrap();
    assert!(raster.get("raytrace").is_none());
}

#[test]
fn numeric_editor_save_refreshes_only_unchanged_runtime_binding_sources() {
    use solidcraft_engine::render::job::ReflectionMode;
    for (method, change_geometry) in
        [(ImageMethod::GpuRaytrace, false), (ImageMethod::GpuRaytrace, true), (ImageMethod::CpuRaytrace, false), (ImageMethod::CpuRaytrace, true)]
    {
        let mut h = Harness::new();
        // This fixture exercises changing fixed AA/reflection attempts, independently of
        // progressive sampling's one-attempt-per-camera-sample contract.
        h.app.preferences.render_legacy_image_defaults = true;
        h.app.run("solid.box", json!({"length":10,"width":10,"height":3,"body_name":"Target"})).unwrap();
        enter(&mut h.app, true).unwrap();
        select_image_method(&mut h.app, method);
        setups::save(&mut h.app).unwrap();
        if method == ImageMethod::GpuRaytrace {
            let target = materials::read_targets(&mut h.app).unwrap().remove(0);
            let mut binding = materials::Binding::new(target, &h.app);
            binding.maps_enabled = true;
            binding.maps.color = "unchanged-source.png".into();
            h.app.render_workspace.view.materials.enabled = true;
            h.app.render_workspace.view.materials.bindings.push(binding);
        } else {
            cpu::set_reflection(&mut h.app, "Target", Some(ReflectionMode::Mirror)).unwrap();
        }
        let runtime = image_request(&h.app).unwrap();
        let features = h.app.session.doc.features.clone();
        let undo = h.app.session.undo.len();
        let before = setups::draft(&h.app).unwrap();
        if method == ImageMethod::GpuRaytrace {
            h.app.render_workspace.view.samples += 1;
        } else {
            h.app.render_workspace.view.raytrace.primary_samples = 4;
        }
        setups::persist_editor(&mut h.app, &before).unwrap();
        let after = image_request(&h.app).expect("render-only persistence must retain current bindings");
        assert_eq!(after["raytrace"]["textures"], runtime["raytrace"]["textures"]);
        if method == ImageMethod::GpuRaytrace {
            assert_eq!(after["raytrace"]["textures"][0]["base_color_srgb"], "unchanged-source.png");
        } else {
            assert!(after["raytrace"]["reflections"]["bodies"].as_array().is_some_and(|bodies| bodies.len() == 1));
        }
        assert_eq!(after["raytrace"]["reflections"], runtime["raytrace"]["reflections"]);
        assert_eq!(h.app.session.doc.features, features);
        assert_eq!(h.app.session.undo.len(), undo + 1);
        if change_geometry {
            h.app.run("solid.box", json!({"length":2,"width":2,"height":2,"corner":[20,0,0],"body_name":"Later","operation":"new"})).unwrap();
        } else {
            h.app.run("appearance.assign", json!({"bodies":["Target"],"color":"#123456"})).unwrap();
        }
        assert!(image_request(&h.app).is_err());
        let before = setups::draft(&h.app).unwrap();
        h.app.render_workspace.view.draft.exposure_ev += 0.25;
        setups::persist_editor(&mut h.app, &before).unwrap();
        assert!(image_request(&h.app).is_err(), "a render save must never bless a changed material source");
        h.app.run("browser.visibility", json!({"items":["Target"],"visible":false})).unwrap();
        let before = setups::draft(&h.app).unwrap();
        h.app.render_workspace.view.draft.exposure_ev += 0.25;
        setups::persist_editor(&mut h.app, &before).unwrap();
        assert!(image_request(&h.app).is_err(), "hidden or moved targets must remain stale");
    }
}

#[test]
fn undo_numeric_editor_sync_preserves_runtime_inputs_and_camera_draft() {
    use solidcraft_engine::render::job::ReflectionMode;
    let mut h = Harness::new();
    h.app.preferences.render_legacy_image_defaults = true;
    h.call("ui.resize", json!({"width":1600,"height":2600}));
    h.app.run("solid.box", json!({"length":10,"width":10,"height":3,"body_name":"Target"})).unwrap();
    enter(&mut h.app, true).unwrap();
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    h.app.render_workspace.view.raytrace.primary_samples = 1;
    setups::save(&mut h.app).unwrap();
    h.app.run("view.render_select", json!({"kind":"sampling"})).unwrap();
    cpu::set_reflection(&mut h.app, "Target", Some(ReflectionMode::Mirror)).unwrap();
    h.app.cancel_view_animation();
    h.app.cam.yaw += 0.25;
    let camera = h.app.cam;
    let saved = h.app.session.doc.render.setups["Studio"].clone();
    let request = image_request(&h.app).unwrap();
    let undo = h.app.session.undo.len();
    h.frames(3);
    h.step(&json!({"click":{"handle":format!("dialog:section:{}", h.app.tr("render.children.cpu"))}})).unwrap();
    h.step(&json!({"click":{"handle":"render:aa"}})).unwrap();
    h.step(&json!({"click":{"handle":"render:aa-4"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.raytrace.primary_samples, 4);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    h.frames(3);
    h.app.run("edit.undo", json!({})).unwrap();
    h.frames(3);
    assert_eq!(h.app.session.doc.render.setups["Studio"], saved);
    assert_eq!(h.app.render_workspace.view.raytrace.primary_samples, 1, "Undo must reach the actual Sampling editor");
    assert!(h.widget("1×").is_some(), "the visible Sampling control must show the undone value");
    assert_eq!(h.app.cam, camera);
    assert_eq!(image_request(&h.app).unwrap(), request, "Undo must preserve valid temporary reflection bindings");
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(h.app.render_workspace.view.dirty, "the camera and reflection drafts remain unsaved");
    h.app.run("edit.redo", json!({})).unwrap();
    h.frames(3);
    assert_eq!(h.app.render_workspace.view.raytrace.primary_samples, 4);
    assert!(h.widget("4×").is_some());
    assert_eq!(h.app.cam, camera);
    assert_eq!(image_request(&h.app).unwrap()["raytrace"]["reflections"], request["raytrace"]["reflections"]);
}

#[test]
fn gpu_material_identity_requires_explicit_revalidation_and_never_retargets() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":3,"body_name":"Optics"})).unwrap();
    let target: materials::Target = serde_json::from_value(h.app.run("render.scene", json!({})).unwrap()["bodies"][0].clone()).unwrap();
    // Glass revalidation keeps the original ordinal identity contract, even
    // with a stable capture target. Coat ordinal shifts have their own test.
    let mut binding = materials::Binding::new(target, &h.app);
    binding.glass_enabled = true;
    h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
    h.app.render_workspace.view.materials.enabled = true;
    h.app.render_workspace.view.materials.bindings = vec![binding.clone()];
    let capture = |app: &SolveApp| materials::params(app, &mut json!({"settings":setups::draft(app).unwrap()}));
    capture(&h.app).unwrap();
    h.app.run("appearance.assign", json!({"bodies":["Optics"],"color":"#123456"})).unwrap();
    assert!(capture(&h.app).is_err());
    materials::revalidate(&mut h.app, &mut binding).unwrap();
    h.app.render_workspace.view.materials.bindings = vec![binding.clone()];
    capture(&h.app).unwrap();
    h.app.run("solid.box", json!({"length":3,"width":3,"height":3,"corner":[20,0,0],"body_name":"Other","operation":"new"})).unwrap();
    let scene = h.app.run("render.scene", json!({})).unwrap();
    let target: materials::Target =
        serde_json::from_value(scene["bodies"].as_array().unwrap().iter().find(|body| body["name"] == "Other").unwrap().clone()).unwrap();
    let mut moved = materials::Binding::new(target.clone(), &h.app);
    moved.glass_enabled = true;
    h.app.render_workspace.view.materials.bindings = vec![moved.clone()];
    h.app.run("browser.visibility", json!({"items":["Optics"],"visible":false})).unwrap();
    assert!(capture(&h.app).is_err());
    assert!(materials::revalidate(&mut h.app, &mut moved).is_err(), "a surviving body with a new ordinal must be selected explicitly");
    assert_eq!(moved.target, target);
    let scene = h.app.run("render.scene", json!({})).unwrap();
    assert_eq!(scene["bodies"][0]["name"], "Other");
    assert_ne!(scene["bodies"][0]["ordinal"], target.ordinal);
    binding.target.name = "Different body".into();
    assert!(materials::revalidate(&mut h.app, &mut binding).is_err());
    assert_eq!(binding.target.name, "Different body");
    reset_document(&mut h.app);
    assert!(h.app.render_workspace.view.materials.bindings.is_empty());
}

#[test]
fn clicked_gpu_glass_recipe_stays_temporary_and_preserves_cpu_certified_numbers() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2200}));
    h.app.run("solid.box", json!({"length":10,"width":10,"height":3,"body_name":"Optics"})).unwrap();
    h.app.session.mark_saved();
    enter(&mut h.app, true).unwrap();
    inspector::select_editor(&mut h.app, inspector::Editor::Output);
    h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
    h.frames(3);
    let doc = h.app.session.doc.clone();
    h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body { name: "Optics".into() }]})).unwrap();
    h.frames(3);
    let glass = h.app.tr("render.appearance_editor.glass").to_string();
    h.step(&json!({"click":{"handle":format!("dialog:section:{glass}")}})).unwrap();
    h.step(&json!({"widget":h.app.tr("render.material_custom")})).unwrap();
    h.step(&json!({"click":{"handle":"render:glass:starter.glass.amber-tint"}})).unwrap();
    let binding = &h.app.render_workspace.view.materials.bindings[0];
    assert!(binding.glass_enabled && !binding.coat_enabled && !binding.maps_enabled);
    assert_eq!(binding.glass.recipe, Some("starter.glass.amber-tint"));
    assert_eq!(binding.target.name, "Optics");
    let glass = binding.glass.clone();
    h.app.render_workspace.view.method = ImageMethod::CpuRaytrace;
    let cpu = image_request(&h.app).unwrap();
    let authored = &cpu["settings"]["optics"][0]["optics"];
    assert_eq!(authored["boundary"], "closed_mesh");
    assert_eq!(authored["ior"], glass.ior);
    assert_eq!(authored["absorption_per_mm"], json!(glass.absorption));
    let optical: solidcraft_engine::doc::render_optics::OpticalMaterial = serde_json::from_value(authored.clone()).unwrap();
    let lowered = optical.to_cpu(h.app.render_workspace.view.materials.bindings[0].target.ordinal, "Optics").unwrap();
    assert_eq!(lowered.boundary, solidcraft_engine::render::job::DielectricBoundary::ClosedBox);
    assert_eq!(lowered.ior, glass.ior);
    assert_eq!(lowered.absorption_per_mm, glass.absorption);
    assert_eq!(
        h.app.render_workspace.view.materials.bindings[0].glass.boundary,
        materials::Boundary::Closed,
        "CPU certification never downgrades the authored preset boundary"
    );
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert!(!h.app.session.is_dirty());
}

#[test]
fn clicked_gpu_material_at_assignment_limit_edits_existing_but_cannot_add_another_body() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2200}));
    h.app.run("solid.box", json!({"length":1,"width":1,"height":1,"body_name":"Optics"})).unwrap();
    h.app.run("solid.pattern.rectangular", json!({"bodies":["Optics"],"dir1":[1,0,0],"count1":129,"spacing1":3})).unwrap();
    let targets: Vec<materials::Target> = serde_json::from_value(h.app.run("render.scene", json!({})).unwrap()["bodies"].clone()).unwrap();
    assert_eq!(targets.len(), 129);
    h.app.session.mark_saved();
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    enter(&mut h.app, true).unwrap();
    inspector::select_editor(&mut h.app, inspector::Editor::Output);
    h.app.render_workspace.view.materials.bindings = targets.iter().skip(1).cloned().map(|target| materials::Binding::new(target, &h.app)).collect();
    h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
    h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body { name: targets[1].name.clone() }]})).unwrap();
    h.frames(3);
    let glass = h.app.tr("render.appearance_editor.glass").to_string();
    h.step(&json!({"click":{"handle":format!("dialog:section:{glass}")}})).unwrap();
    h.step(&json!({"widget":h.app.tr("render.appearance_editor.glass_enable")})).unwrap();
    assert!(h.app.render_workspace.view.materials.bindings[0].glass_enabled, "existing assignments remain editable at the limit");
    assert!(h.app.render_workspace.view.materials.bindings.iter().skip(1).all(|binding| !binding.glass_enabled));
    assert_eq!(h.app.render_workspace.view.materials.bindings.len(), 128);

    // The first body has no assignment: editing cannot allocate a 129th entry.
    h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body { name: targets[0].name.clone() }]})).unwrap();
    h.frames(3);
    h.step(&json!({"widget":h.app.tr("render.appearance_editor.glass_enable")})).unwrap();
    assert_eq!(h.app.render_workspace.view.materials.bindings.len(), 128);
    assert!(h.app.render_workspace.view.materials.bindings.iter().all(|binding| binding.target != targets[0]));
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(!h.app.session.is_dirty());
}

#[test]
fn new_cpu_reflection_matches_sampling_without_rewriting_explicit_attempts() {
    use solidcraft_engine::render::job::ReflectionMode;
    for legacy in [false, true] {
        let mut h = Harness::new();
        h.app.preferences.render_legacy_image_defaults = legacy;
        h.app.run("solid.box", json!({"length":10,"width":20,"height":5,"body_name":"Reflector"})).unwrap();
        enter(&mut h.app, true).unwrap();
        select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
        let before = h.app.session.doc.clone();
        let undo = h.app.session.undo.len();
        cpu::set_reflection(&mut h.app, "Reflector", Some(ReflectionMode::Mirror)).unwrap();
        let expected = if legacy { 4 } else { 1 };
        assert_eq!(h.app.render_workspace.view.raytrace.reflections.as_ref().unwrap().samples, expected);
        let request = image_request(&h.app).expect("adding the first mirror must leave a renderable request");
        assert_eq!(request["raytrace"]["reflections"]["samples"], expected);
        assert_eq!(request["raytrace"]["reflections"]["bodies"][0]["body"], "Reflector");
        let mut params = json!({});
        cpu::params(&h.app, &mut params).unwrap();
        assert_eq!(params["raytrace"]["reflections"]["samples"], expected);
        assert!(Arc::ptr_eq(&before, &h.app.session.doc));
        assert_eq!(h.app.session.undo.len(), undo);
        assert!(h.app.render_workspace.view.job.is_none());

        // Existing numerical intent is never silently reduced when a body mode changes.
        h.app.render_workspace.view.raytrace.reflections.as_mut().unwrap().samples = 4;
        cpu::set_reflection(&mut h.app, "Reflector", Some(ReflectionMode::RoughSpecular)).unwrap();
        assert_eq!(h.app.render_workspace.view.raytrace.reflections.as_ref().unwrap().samples, 4);
        assert_eq!(image_request(&h.app).is_ok(), legacy);
        inspector::select_editor(&mut h.app, inspector::Editor::Sampling);
        h.call("ui.resize", json!({"width":1600,"height":2600}));
        h.frames(3);
        h.step(&json!({"click":{"handle":format!("dialog:section:{}",h.app.tr("render.children.cpu"))}})).unwrap();
        h.frames(3);
        assert_eq!(
            h.app.render_workspace.view.raytrace.reflections.as_ref().unwrap().samples,
            4,
            "showing the field must preserve unsupported existing attempts"
        );
        assert_eq!(image_request(&h.app).is_ok(), legacy);
    }
}

#[test]
fn cpu_reflection_controls_preserve_exact_targets_and_engine_bounds() {
    use solidcraft_engine::render::job::ReflectionMode;
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5,"body_name":"Reflector"})).unwrap();
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    cpu::set_reflection(&mut h.app, "Reflector", Some(ReflectionMode::Mirror)).unwrap();
    cpu::set_reflection(&mut h.app, "Reflector", Some(ReflectionMode::RoughSpecular)).unwrap();
    let reflections = h.app.render_workspace.view.raytrace.reflections.as_mut().unwrap();
    assert_eq!(reflections.bodies.len(), 1);
    reflections.max_bounces = 8;
    reflections.samples = 64;
    let mut params = json!({});
    cpu::params(&h.app, &mut params).unwrap();
    assert_eq!(params["raytrace"]["reflections"]["bodies"], json!([{"body":"Reflector","mode":"rough_specular"}]));
    assert_eq!(params["raytrace"]["reflections"]["max_bounces"], 8);
    assert_eq!(params["raytrace"]["reflections"]["samples"], 64);
    h.app.render_workspace.view.raytrace.reflections.as_mut().unwrap().max_bounces = 9;
    assert!(cpu::params(&h.app, &mut json!({})).is_err());
    h.app.render_workspace.view.raytrace.reflections.as_mut().unwrap().max_bounces = 8;
    h.app.render_workspace.view.raytrace.reflections.as_mut().unwrap().samples = 65;
    assert!(cpu::params(&h.app, &mut json!({})).is_err());
    h.app.render_workspace.view.raytrace.reflections.as_mut().unwrap().samples = 64;
    h.app.session.visibility.hidden_bodies.push("Reflector".into());
    assert!(cpu::params(&h.app, &mut json!({})).unwrap_err().contains(h.app.tr("render.cpu_body_missing")));
    cpu::set_reflection(&mut h.app, "Reflector", None).unwrap();
    assert!(h.app.render_workspace.view.raytrace.reflections.is_none());
    assert!(cpu::set_reflection(&mut h.app, "Missing", Some(ReflectionMode::Mirror)).is_err());
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
fn cpu_reflection_binding_requires_confirmation_and_never_follows_a_reused_name() {
    use solidcraft_engine::render::job::ReflectionMode;
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5,"body_name":"Reflector"})).unwrap();
    cpu::set_reflection(&mut h.app, "Reflector", Some(ReflectionMode::Mirror)).unwrap();
    h.app.run("solid.box", json!({"length":5,"width":5,"height":5,"body_name":"Other"})).unwrap();
    assert!(cpu::params(&h.app, &mut json!({})).unwrap_err().contains(h.app.tr("render.cpu_binding_stale")));
    assert!(
        cpu::set_reflection(&mut h.app, "Reflector", Some(ReflectionMode::RoughSpecular)).unwrap_err().contains(h.app.tr("render.cpu_binding_stale"))
    );
    cpu::revalidate(&mut h.app, "Reflector").unwrap();
    cpu::params(&h.app, &mut json!({})).unwrap();
    h.app.run("body.rename", json!({"body":"Reflector","name":"Original"})).unwrap();
    h.app.run("body.rename", json!({"body":"Other","name":"Reflector"})).unwrap();
    assert!(cpu::params(&h.app, &mut json!({})).unwrap_err().contains(h.app.tr("render.cpu_binding_stale")));
    assert!(cpu::revalidate(&mut h.app, "Reflector").is_err(), "confirmation cannot move a binding to a different feature");
    let before = json!(h.app.render_workspace.view.raytrace.reflections);
    for mode in [ReflectionMode::Mirror, ReflectionMode::RoughSpecular] {
        assert!(cpu::set_reflection(&mut h.app, "Reflector", Some(mode)).unwrap_err().contains(h.app.tr("render.cpu_binding_stale")));
        assert_eq!(json!(h.app.render_workspace.view.raytrace.reflections), before, "a stale row's mode cannot rebind its body");
        assert!(cpu::params(&h.app, &mut json!({})).is_err());
    }
    cpu::set_reflection(&mut h.app, "Reflector", None).unwrap();
    cpu::set_reflection(&mut h.app, "Reflector", Some(ReflectionMode::RoughSpecular)).unwrap();
    cpu::params(&h.app, &mut json!({})).unwrap();
    // Isolate document identity: the cloned document has the same bodies, IDs and revision.
    let previous_doc = h.app.session.doc.clone();
    let previous_revision = h.app.session.revision;
    h.app.session = Session::new((*previous_doc).clone());
    h.app.session.revision = previous_revision;
    assert!(!Arc::ptr_eq(&previous_doc, &h.app.session.doc));
    assert!(cpu::params(&h.app, &mut json!({})).unwrap_err().contains(h.app.tr("render.cpu_binding_stale")));
}

#[test]
fn cpu_environment_controls_keep_wire_schemas_separate_without_reading_files() {
    let mut h = Harness::new();
    h.app.render_workspace.view.cpu.environment = cpu::Environment::Hdr;
    h.app.render_workspace.view.cpu.hdr = captured_hdr_input("not-opened-by-ui.hdr");
    h.app.render_workspace.view.cpu.rotation_rad = std::f64::consts::FRAC_PI_2;
    h.app.render_workspace.view.cpu.intensity = 2.0;
    h.app.render_workspace.view.cpu.samples = 64;
    let mut hdr = json!({});
    cpu::params(&h.app, &mut hdr).unwrap();
    assert_eq!(
        hdr["environment"],
        json!({"path":"not-opened-by-ui.hdr","expected_sha256":h.app.render_workspace.view.cpu.hdr.resource().unwrap(),"rotation_radians":std::f64::consts::FRAC_PI_2,"intensity":2.0,"samples":64})
    );
    assert!(hdr["raytrace"].get("environment").is_none());
    for gpu_key in ["progressive", "tone_map", "denoise", "lighting", "lens"] {
        assert!(hdr.get(gpu_key).is_none());
    }
    h.app.render_workspace.view.cpu.environment = cpu::Environment::Constant;
    h.app.render_workspace.view.cpu.color = [0.2, 0.4, 0.8];
    let mut constant = json!({});
    cpu::params(&h.app, &mut constant).unwrap();
    assert!(constant.get("environment").is_none());
    assert_eq!(constant["raytrace"]["environment"], json!({"color":[0.2,0.4,0.8],"intensity":2.0}));
    assert!(h.app.render_workspace.view.raytrace.environment.is_none(), "request building must not mutate the editable base");
    h.app.render_workspace.view.cpu.color[0] = f64::NAN;
    assert!(cpu::params(&h.app, &mut json!({})).is_err());
    h.app.render_workspace.view.cpu.environment = cpu::Environment::Hdr;
    h.app.render_workspace.view.cpu.samples = 3;
    assert!(cpu::params(&h.app, &mut json!({})).is_err());
    h.app.render_workspace.view.cpu.samples = 16;
    h.app.render_workspace.view.cpu.hdr = captured_hdr_input("bad\npath.hdr");
    assert!(cpu::params(&h.app, &mut json!({})).is_err());
    h.app.render_workspace.view.cpu.environment = cpu::Environment::None;
    let mut none = json!({});
    cpu::params(&h.app, &mut none).unwrap();
    assert!(none.get("environment").is_none());
    assert!(none["raytrace"].get("environment").is_none());
}

#[test]
fn cpu_light_controls_preserve_hidden_spot_fields_and_validate_runtime_bounds() {
    use solidcraft_engine::render::job::{DirectionalLightOptions, PunctualLightOptions, SpotConeOptions};
    let mut h = Harness::new();
    h.app.render_workspace.view.raytrace.lights = Some(vec![DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 2.0 }]);
    h.app.render_workspace.view.raytrace.punctual_lights = Some(vec![PunctualLightOptions {
        position_mm: [0.0, 0.0, 100.0],
        color: [1.0, 0.5, 0.25],
        intensity: 4.0,
        reference_distance_mm: 100.0,
        minimum_distance_mm: 0.01,
        cone: None,
    }]);
    let mut params = json!({});
    cpu::params(&h.app, &mut params).unwrap();
    assert_eq!(params["raytrace"]["lights"][0]["intensity"], 2.0);
    assert_eq!(params["raytrace"]["punctual_lights"][0]["reference_distance_mm"], 100.0);
    assert!(params.get("lighting").is_none(), "CPU must not emit GPU studio lighting");
    h.app.render_workspace.view.raytrace.lights.as_mut().unwrap()[0].direction = [0.0; 3];
    assert!(cpu::params(&h.app, &mut json!({})).is_err());
    h.app.render_workspace.view.raytrace.lights.as_mut().unwrap()[0].direction = [0.0, 0.0, 1.0];
    h.app.render_workspace.view.raytrace.punctual_lights.as_mut().unwrap()[0].minimum_distance_mm = 101.0;
    assert!(cpu::params(&h.app, &mut json!({})).is_err());
    h.app.render_workspace.view.raytrace.punctual_lights.as_mut().unwrap()[0].minimum_distance_mm = 0.01;
    h.app.render_workspace.view.raytrace.punctual_lights.as_mut().unwrap()[0].cone =
        Some(SpotConeOptions { emitted_axis: [0.0, 0.0, -1.0], inner_half_angle_radians: 0.1, outer_half_angle_radians: 0.2 });
    let mut params = json!({});
    cpu::params(&h.app, &mut params).unwrap();
    assert_eq!(
        params["raytrace"]["punctual_lights"][0]["cone"],
        json!(h.app.render_workspace.view.raytrace.punctual_lights.as_ref().unwrap()[0].cone)
    );
}

#[test]
fn delayed_cpu_hdr_picker_cannot_write_into_another_document_or_revision() {
    use crate::picker_test_support::{Slot, open};
    for other_document in [false, true] {
        let mut h = Harness::new();
        let slot: Slot = Default::default();
        open(&mut h.app, &slot);
        h.app.render_workspace.view.cpu.hdr = hdr::Input::FilePending { path: "original.hdr".into() };
        hdr::pick(&mut h.app, hdr::Target::Cpu);
        let ctx = crate::fonts::test_context();
        crate::file_picker::tick(&mut h.app, &ctx);
        assert!(h.app.file_picker_busy());
        if other_document {
            h.app.session = Session::default();
        } else {
            h.app.session.execute("solid.box", &json!({"length":10,"width":20,"height":5})).unwrap();
        }
        *slot.borrow_mut() = Some(Some("late.hdr".into()));
        crate::file_picker::tick(&mut h.app, &ctx);
        assert_eq!(h.app.render_workspace.view.cpu.hdr, hdr::Input::FilePending { path: "original.hdr".into() });
    }
}

#[test]
fn clicked_setup_save_reload_and_switch_keep_gpu_numeric_intent_without_a_host() {
    use solidcraft_engine::doc::render_settings::{GpuJob, RenderJob, SkyEnvironment};
    use solidcraft_engine::render::job::{AreaLight, AreaLights, StudioLighting, Sun, ThinLens, ToneMap};
    use solidcraft_engine::render::lighting::environment::SkyGradient;
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2200}));
    enter(&mut h.app, true).unwrap();
    h.app.run("view.render_stage", json!({"stage":"output"})).unwrap();
    h.frames(3);
    h.app.session.set_render_job_host(None).unwrap();
    assert!(image_host(&h.app, ImageMethod::GpuRaytrace).is_err());
    h.step(&json!({"click":{"handle":"render:method"}})).unwrap();
    h.step(&json!({"click":{"handle":"render:method-gpu"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::GpuRaytrace);
    h.app.cam.fov = 0.7;
    h.app.ui.perspective = true;
    // The positive UI fixture uses the admitted document boundary. Coordinates
    // beyond it must still refuse before a setup that cannot reopen is created.
    for x in [-2e9, 2e9, 1e9_f64.next_up()] {
        assert!(
            AreaLights::new(&[AreaLight { center: [x, 0.0, 100.0], u: [2.0, 0.0, 0.0], v: [0.0, -3.0, 0.0], radiance: [3.0, 5.0, 7.0] }]).is_err()
        );
    }
    let expected = RenderSetup {
        camera: h.app.cam,
        exposure_ev: 1.25,
        width: 48,
        height: 32,
        job: Some(RenderJob::GpuPath {
            transparent_background: true,
            options: Box::new(GpuJob {
                caustics: None,
                aging: Default::default(),
                nested_media: true,
                volume: Some(solidcraft_engine::render::job::volume::HomogeneousVolume { extinction_per_mm: 0.0, ..Default::default() }),
                adaptive: Some(solidcraft_engine::render::job::adaptive::AdaptiveSampling {
                    pilot_samples: 16,
                    min_samples: 16,
                    relative_error: 0.1,
                    absolute_error: 0.003,
                    coverage_error: 0.02,
                }),
                transmission_samples: 4,
                finishing: Default::default(),
                backdrop: None,
                linear_exr: false,
                samples: 37,
                max_depth: 11,
                seed: 1977,
                tone_map: ToneMap::AcesFitted,
                denoise: true,
                lens: Some(ThinLens { aperture_radius_mm: 0.5, focus_distance_mm: 123.0 }),
                lighting: Some(StudioLighting {
                    punctual_lights: Default::default(),
                    directional_lights: solidcraft_engine::render::job::DirectionalLights::new(&[
                        solidcraft_engine::render::job::DirectionalLightOptions {
                            direction: [0.0, 0.0, 2.0],
                            color: [0.25, 0.5, 1.0],
                            intensity: 0.0,
                        },
                        solidcraft_engine::render::job::DirectionalLightOptions {
                            direction: [3.0, -4.0, 5.0],
                            color: [0.6, 0.4, 0.2],
                            intensity: 2.5,
                        },
                    ])
                    .unwrap(),
                    environment: [0.25, 0.5, 0.75],
                    sun: Some(Sun { direction: [2.0, 3.0, 4.0], angular_radius_rad: 0.0123456789, radiance: [11.0, 22.0, 33.0] }),
                    area_lights: AreaLights::new(&[AreaLight {
                        center: [1e9, 0.0, 100.0],
                        u: [2.0, 0.0, 0.0],
                        v: [0.0, -3.0, 0.0],
                        radiance: [3.0, 5.0, 7.0],
                    }])
                    .unwrap(),
                }),
                environment: Some(SkyEnvironment::Sky {
                    gradient: SkyGradient { zenith: [0.3, 0.4, 0.5], horizon: [0.6, 0.7, 0.8], ground: [0.1, 0.2, 0.3] },
                    rotation_rad: std::f64::consts::PI,
                    intensity: 1.75,
                }),
                hdr: None,
            }),
        }),
        ..Default::default()
    };
    h.app.run("render.settings.set", json!({"name":"GPU saved","set":expected})).unwrap();
    h.app.render_workspace.view.name = "GPU saved".into();
    setups::load(&mut h.app);
    let completed = h.app.render_workspace.view.completed;
    let result_camera = h.app.render_workspace.view.result_camera;
    h.frames(3);
    h.app.run("view.render_select", json!({"kind":"sampling"})).unwrap();
    h.frames(3);
    h.app.run("view.render_select", json!({"kind":"effects"})).unwrap();
    h.frames(3);
    h.app.run("view.render_select", json!({"kind":"nothing"})).unwrap();
    h.app.run("view.render_stage", json!({"stage":"light"})).unwrap();
    h.frames(3);
    h.app.run("view.render_select", json!({"kind":"lighting"})).unwrap();
    h.frames(3);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    h.app.run("view.render_select", json!({"kind":"light","id":"area:0"})).unwrap();
    h.frames(3);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    assert!(!h.app.render_workspace.view.dirty);
    assert!(image_params(&h.app).is_err(), "offline editing must not claim an executable GPU");
    assert_eq!(image_request(&h.app).unwrap(), json!({"settings":expected}));
    h.app.render_workspace.view.name = "GPU copy".into();
    h.app.render_workspace.view.dirty = true;
    assert_eq!(h.app.render_workspace.view.name, "GPU copy");
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected, "typing Save As must not reset the settings");
    assert!(h.app.render_workspace.view.dirty);
    let undo = h.app.session.undo.len();
    setups::save(&mut h.app).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.doc.render.setups["GPU copy"], expected);
    h.app.render_workspace.view.seed = 8;
    h.app.render_workspace.view.studio.ambient[1] = 0.125;
    setups::refresh_dirty(&mut h.app);
    assert!(h.app.render_workspace.view.dirty);
    setups::load(&mut h.app);
    h.frames(3);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    assert!(!h.app.render_workspace.view.dirty);
    select_image_method(&mut h.app, ImageMethod::Raster);
    h.app.render_workspace.view.name = "Raster saved".into();
    setups::save(&mut h.app).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:shots:setup:GPU copy"}})).unwrap();
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    assert_eq!(h.app.render_workspace.view.completed, completed);
    assert_eq!(h.app.render_workspace.view.result_camera, result_camera);
    let reopened = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
    assert_eq!(reopened.render.setups["GPU copy"], expected, "the boundary setup must reopen from JSON");
    let native = solidcraft_engine::io::write_design_container(&h.app.session.doc).unwrap();
    let reopened = solidcraft_engine::io::read_design(&native).unwrap();
    assert_eq!(reopened.render.setups["GPU copy"], expected, "the native container keeps exact numeric intent");
}

#[test]
fn default_cpu_progressive_request_preserves_area_and_led_lighting_without_saving() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    use solidcraft_engine::render::job::{CpuProgressiveOptions, RectangleLightOptions, led_strip::LedStripOptions};
    let mut h = Harness::new();
    let before = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    assert_eq!(h.app.render_workspace.view.raytrace.progressive, Some(CpuProgressiveOptions::default()));
    let lights = vec![
        RectangleLightOptions {
            center_mm: [0.0, 0.0, 100.0],
            edge_u_mm: [20.0, 0.0, 0.0],
            edge_v_mm: [0.0, -30.0, 0.0],
            color: [0.3, 0.5, 0.7],
            intensity: 5.0,
        },
        LedStripOptions {
            center_mm: [10.0, 20.0, 80.0],
            length_mm: 40.0,
            width_mm: 2.0,
            length_axis: [1.0, 0.0, 0.0],
            width_axis: [0.0, -1.0, 0.0],
            color: [1.0, 0.68, 0.38],
            intensity: 3.0,
        }
        .rectangle()
        .unwrap(),
    ];
    h.app.render_workspace.view.raytrace.area_lights = Some(lights.clone());
    h.app.render_workspace.view.raytrace.area_samples = Some(64);
    h.app.render_workspace.view.raytrace.seed = Some(9123);
    let request = image_request(&h.app).expect("default progressive sampling must accept the complete area/LED setup");
    let setup: RenderSetup = serde_json::from_value(request["settings"].clone()).unwrap();
    setup.validate().unwrap();
    let Some(RenderJob::CpuRaytrace { options, .. }) = setup.job else { panic!("CPU method must be preserved") };
    assert_eq!(options.progressive, Some(CpuProgressiveOptions::default()));
    assert_eq!(options.area_lights, Some(lights));
    assert_eq!((options.area_samples, options.seed), (Some(64), Some(9123)));
    assert!(request.get("progressive").is_none(), "CPU sampling must not select the GPU executor");
    assert!(h.app.render_workspace.view.job.is_none());
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
fn cpu_setup_reload_preserves_unedited_numeric_options_and_explicit_empty_values() {
    use solidcraft_engine::doc::render_settings::{CpuJob, RenderJob};
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2400}));
    enter(&mut h.app, true).unwrap();
    inspector::select_editor(&mut h.app, inspector::Editor::Output);
    let options: CpuJob = serde_json::from_value(json!({
        "primary_samples":4,"shadows":false,
        "lights":[{"direction":[2.0,-4.0,8.0],"color":[0.2,0.4,0.6],"intensity":3.0}],
        "punctual_lights":[{"position_mm":[10.0,20.0,30.0],"color":[1.0,0.5,0.25],"intensity":4.0,
            "reference_distance_mm":100.0,"minimum_distance_mm":0.01,
            "cone":{"emitted_axis":[0.0,0.0,-2.0],"inner_half_angle_radians":0.1,"outer_half_angle_radians":0.3}}],
        "area_lights":[{"center_mm":[0.0,0.0,100.0],"edge_u_mm":[20.0,0.0,0.0],"edge_v_mm":[0.0,-30.0,0.0],"color":[0.3,0.5,0.7],"intensity":5.0}],
        "area_samples":16,"seed":9123,"environment":{"color":[0.0,0.0,0.0],"intensity":2.0}
    }))
    .unwrap();
    let mut expected = RenderSetup {
        camera: h.app.cam,
        exposure_ev: 1.5,
        job: Some(RenderJob::CpuRaytrace { transparent_background: false, options: Box::new(options) }),
        ..Default::default()
    };
    h.app.run("render.settings.set", json!({"name":"CPU numeric","set":expected})).unwrap();
    h.app.render_workspace.view.name = "CPU numeric".into();
    setups::load(&mut h.app);
    h.frames(3);
    h.app.run("view.render_select", json!({"kind":"lighting"})).unwrap();
    h.frames(3);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    h.app.run("view.render_select", json!({"kind":"light","id":"directional:0"})).unwrap();
    h.frames(3);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    assert_eq!(image_request(&h.app).unwrap(), json!({"settings":expected}));
    assert_eq!(h.app.render_workspace.view.raytrace.exposure_ev, 0.0, "the UI has one exposure authority");
    h.app.render_workspace.view.draft.background = [2, 4, 8];
    setups::refresh_dirty(&mut h.app);
    assert!(h.app.render_workspace.view.dirty);
    expected.background = [2, 4, 8];
    setups::save(&mut h.app).unwrap();
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    assert!(json!(expected)["job"]["options"].get("exposure_ev").is_none());
    for value in [json!({}), json!({"lights":[],"punctual_lights":[],"area_lights":[],"environment":{"color":[0.0,0.0,0.0],"intensity":0.0}})] {
        let options: CpuJob = serde_json::from_value(value).unwrap();
        expected.job = Some(RenderJob::CpuRaytrace { transparent_background: false, options: Box::new(options) });
        h.app.run("render.settings.set", json!({"name":"CPU numeric","set":expected})).unwrap();
        setups::load(&mut h.app);
        assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    }
}

#[test]
fn cpu_progressive_controls_resize_only_on_request_and_save_reload_without_a_host() {
    use solidcraft_engine::render::job::CpuProgressiveOptions;
    let mut h = Harness::new();
    // Exercise the retained fixed-to-progressive control, rather than the new method default.
    h.app.preferences.render_legacy_image_defaults = true;
    h.call("ui.resize", json!({"width":1600,"height":2600}));
    enter(&mut h.app, true).unwrap();
    inspector::select_editor(&mut h.app, inspector::Editor::Sampling);
    h.app.session.set_render_job_host(None).unwrap();
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    h.app.render_workspace.view.raytrace.primary_samples = 16;
    h.app.render_workspace.view.draft.width = 1024;
    h.app.render_workspace.view.draft.height = 1024;
    h.frames(3);
    h.step(&json!({"click":{"handle":format!("dialog:section:{}",h.app.tr("render.children.cpu"))}})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:cpu-sampling"}})).unwrap();
    h.step(&json!({"click":{"handle":"render:cpu-progressive"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.raytrace.primary_samples, 1);
    assert_eq!(h.app.render_workspace.view.raytrace.progressive, Some(CpuProgressiveOptions::default()));
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (1024, 1024));
    h.app.render_workspace.view.raytrace.progressive = Some(CpuProgressiveOptions { samples_per_pixel: 64, seed: u32::MAX });
    h.app.render_workspace.view.draft.exposure_ev = 1.25;
    h.app.render_workspace.view.name = "Progressive CPU".into();
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    assert!(image_request(&h.app).is_err());
    assert!(setups::save(&mut h.app).is_err());
    h.frames(3);
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (1024, 1024));
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    h.app.run("view.render_select", json!({"kind":"output"})).unwrap();
    h.frames(3);
    let resize = crate::i18n::format("render.output_largest", &[("width", "512"), ("height", "512")]);
    assert!(h.widget(&resize).is_some(), "the proposed dimensions must be visible before accepting them");
    h.step(&json!({"widget":resize})).unwrap();
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (512, 512));
    let expected = setups::snapshot(&h.app).unwrap();
    assert_eq!(image_request(&h.app).unwrap(), json!({"settings":expected}));
    assert!(image_request(&h.app).unwrap().get("progressive").is_none(), "CPU sampling must not select the GPU executor");
    setups::save(&mut h.app).unwrap();
    assert_eq!(h.app.session.doc.render.setups["Progressive CPU"], expected);
    h.app.run("view.render_select", json!({"kind":"sampling"})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:cpu-sampling"}})).unwrap();
    h.step(&json!({"click":{"handle":"render:cpu-fixed"}})).unwrap();
    assert!(h.app.render_workspace.view.raytrace.progressive.is_none());
    assert!(!h.app.render_workspace.view.dirty, "Sampling saves its numerical edit immediately");
    h.app.run("edit.undo", json!({})).unwrap();
    h.frames(3);
    assert_eq!(h.app.session.doc.render.setups["Progressive CPU"], expected);
    assert!(h.app.render_workspace.view.raytrace.progressive.is_some());
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    assert!(!h.app.render_workspace.view.dirty);
    h.app.render_workspace.view.name = "Progressive CPU copy".into();
    setups::save(&mut h.app).unwrap();
    let reopened = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
    h.app.session = Session::new(reopened);
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    assert_eq!(h.app.render_workspace.view.raytrace.exposure_ev, 0.0);
    assert_eq!(h.app.render_workspace.view.draft.exposure_ev, 1.25);
}

#[test]
fn cpu_progressive_setup_preserves_hidden_values_empty_lists_and_one_exposure() {
    use solidcraft_engine::doc::render_settings::{CpuJob, RenderJob};
    let mut h = Harness::new();
    h.app.session.set_render_job_host(None).unwrap();
    let options: CpuJob = serde_json::from_value(json!({
        "primary_samples":1,"progressive":{"samples_per_pixel":7,"seed":321},"preset":null,
        "lights":[],"punctual_lights":null,"area_lights":[],"area_samples":16,"seed":9123,
        "shadows":false,"environment":{"color":[0.0,0.0,0.0],"intensity":0.0}
    }))
    .unwrap();
    let expected = RenderSetup {
        camera: h.app.cam,
        width: 32,
        height: 16,
        exposure_ev: -1.5,
        job: Some(RenderJob::CpuRaytrace { transparent_background: false, options: Box::new(options) }),
        ..Default::default()
    };
    h.app.run("render.settings.set", json!({"name":"Progressive hidden","set":expected})).unwrap();
    h.app.render_workspace.view.name = "Progressive hidden".into();
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    assert_eq!(image_request(&h.app).unwrap()["settings"]["job"]["method"], "gpu_path");
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    assert_eq!(image_request(&h.app).unwrap(), json!({"settings":expected}));
    h.app.render_workspace.view.raytrace.progressive.as_mut().unwrap().seed = 654;
    setups::refresh_dirty(&mut h.app);
    assert!(h.app.render_workspace.view.dirty);
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    let undo = h.app.session.undo.len();
    setups::save(&mut h.app).unwrap();
    assert_eq!(h.app.session.undo.len(), undo, "a no-op Save must preserve all nonedited intent");
}

#[test]
fn setup_save_refuses_nonportable_state_atomically_and_tracks_invalid_legacy_edits() {
    use solidcraft_engine::render::job::ReflectionMode;
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5,"body_name":"Target"})).unwrap();
    enter(&mut h.app, true).unwrap();
    inspector::select_editor(&mut h.app, inspector::Editor::Output);
    setups::load(&mut h.app);
    select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    setups::save(&mut h.app).unwrap();
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.app.render_workspace.view.studio.environment = studio::Environment::Hdr;
    h.app.render_workspace.view.studio.hdr = hdr::Input::FilePending { path: "must-not-be-read.hdr".into() };
    setups::refresh_dirty(&mut h.app);
    assert!(h.app.render_workspace.view.dirty);
    assert!(setups::save(&mut h.app).unwrap_err().contains(h.app.tr("render.hdr_inspect_required")));
    h.app.render_workspace.view.studio.environment = studio::Environment::None;
    let target: materials::Target = serde_json::from_value(h.app.run("render.scene", json!({})).unwrap()["bodies"][0].clone()).unwrap();
    let binding = materials::Binding::new(target, &h.app);
    h.app.render_workspace.view.materials.bindings.push(binding);
    h.app.render_workspace.view.materials.enabled = false;
    assert!(setups::save(&mut h.app).is_err(), "disabled assignments must not be silently lost either");
    h.app.render_workspace.view.materials.bindings.clear();
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    cpu::set_reflection(&mut h.app, "Target", Some(ReflectionMode::Mirror)).unwrap();
    assert!(setups::save(&mut h.app).is_err());
    assert!(h.app.render_workspace.view.raytrace.reflections.is_some());
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    h.app.run("render.settings.set", json!({"name":"Legacy","set":{"job":null,"environment_rotation":0.25,"environment_intensity":2.0}})).unwrap();
    h.app.render_workspace.view.name = "Legacy".into();
    setups::load(&mut h.app);
    assert!(!h.app.render_workspace.view.dirty);
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.app.render_workspace.view.draft.background = [1, 3, 5];
    setups::refresh_dirty(&mut h.app);
    assert!(h.app.render_workspace.view.dirty, "invalid saved intent still needs complete dirty tracking");
    assert!(setups::save(&mut h.app).unwrap_err().contains(h.app.tr("render.setup_legacy_environment")));
    assert_eq!(h.app.render_workspace.view.draft.environment_rotation, 0.25);
    assert_eq!(h.app.render_workspace.view.draft.environment_intensity, 2.0);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
}

fn original_hdr_bytes(value: u8, declared_primaries: bool) -> Vec<u8> {
    let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n".to_vec();
    if declared_primaries {
        bytes.extend_from_slice(b"PRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n");
    }
    bytes.extend_from_slice(b"\n-Y 1 +X 2\n");
    bytes.extend_from_slice(&[value, 128, 128, 129, 128, value, 128, 129]);
    bytes
}

/// A verified selector for pure request tests. No file exists or is read by the builder.
fn captured_hdr_input(path: &str) -> hdr::Input {
    let bytes = original_hdr_bytes(128, true);
    hdr::Input::FileCaptured {
        path: path.into(),
        metadata: hdr::Metadata {
            sha256: solidcraft_engine::doc::render_resources::HdrId::of(&bytes),
            width: 2,
            height: 1,
            encoded_bytes: bytes.len() as u64,
            primaries: Some(solidcraft_engine::doc::render_resources::SRGB_D65_PRIMARIES),
        },
    }
}

#[test]
fn hdr_ui_save_embeds_once_without_a_host_and_reload_never_reopens_the_source_path() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"body_name":"HDR target","length":10,"width":20,"height":5})).unwrap();
    h.call("ui.resize", json!({"width":1600,"height":2200}));
    enter(&mut h.app, true).unwrap();
    inspector::select_editor(&mut h.app, inspector::Editor::Output);
    h.app.session.set_render_job_host(None).unwrap();
    let path = h.dir.join("original-embedded.hdr");
    let bytes = original_hdr_bytes(128, true);
    std::fs::write(&path, &bytes).unwrap();
    let resource = solidcraft_engine::doc::render_resources::HdrId::of(&bytes);
    select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    h.app.render_workspace.view.name = "HDR GPU".into();
    h.app.render_workspace.view.draft.width = 16;
    h.app.render_workspace.view.draft.height = 16;
    h.app.render_workspace.view.studio.environment = studio::Environment::Hdr;
    h.app.render_workspace.view.studio.rotation_rad = 1234.567890123;
    h.app.render_workspace.view.studio.intensity = 1.75;
    h.app.render_workspace.view.samples = 37;
    let before = (h.app.session.revision, h.app.session.undo.len());
    hdr::inspect(&mut h.app, hdr::Target::Gpu, path.to_string_lossy().into_owned()).unwrap();
    assert_eq!((h.app.session.revision, h.app.session.undo.len()), before);
    assert!(h.app.session.doc.render.resources.is_empty());
    let unsaved = image_request(&h.app).unwrap();
    assert_eq!(unsaved["environment"]["expected_sha256"], json!(resource));
    h.frames(3);
    setups::save(&mut h.app).unwrap();
    assert_eq!(h.app.session.undo.len(), before.1 + 1);
    assert_eq!(h.app.render_workspace.view.studio.hdr, hdr::Input::EmbeddedResource { resource });
    assert_eq!(h.app.session.doc.render.resources.len(), 1);
    let gpu = h.app.session.doc.render.setups.get("HDR GPU").unwrap().clone();
    assert!(matches!(&gpu.job, Some(RenderJob::GpuPath { options, .. }) if options.hdr.unwrap().resource == resource && options.samples == 37));
    assert_eq!(image_request(&h.app).unwrap(), json!({"settings":gpu}));
    assert!(!h.app.render_workspace.view.dirty);

    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    h.app.render_workspace.view.name = "HDR CPU".into();
    h.app.render_workspace.view.raytrace.progressive = Some(solidcraft_engine::render::job::CpuProgressiveOptions { samples_per_pixel: 7, seed: 84 });
    h.app.render_workspace.view.draft.exposure_ev = 1.75;
    h.app.render_workspace.view.cpu.environment = cpu::Environment::Hdr;
    h.app.render_workspace.view.cpu.rotation_rad = -1234.567890123;
    h.app.render_workspace.view.cpu.intensity = 2.5;
    h.app.render_workspace.view.cpu.samples = 64;
    hdr::inspect(&mut h.app, hdr::Target::Cpu, path.to_string_lossy().into_owned()).unwrap();
    let undo = h.app.session.undo.len();
    h.frames(2);
    setups::save(&mut h.app).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.doc.render.resources.len(), 1, "both setups share one source payload");
    assert_eq!(h.app.session.doc.render.resources.total_bytes(), bytes.len());
    let cpu = h.app.session.doc.render.setups.get("HDR CPU").unwrap().clone();
    assert!(matches!(&cpu.job, Some(RenderJob::CpuRaytrace { options, .. })
        if options.hdr.unwrap().samples == 64 && options.progressive.unwrap().samples_per_pixel == 7 && options.progressive.unwrap().seed == 84));
    let encoded = h.app.session.doc.to_json();
    assert!(!encoded.contains("original-embedded.hdr"), "the document never stores the source path");
    std::fs::remove_file(&path).unwrap();

    for (name, expected, target) in [("HDR GPU", gpu, hdr::Target::Gpu), ("HDR CPU", cpu, hdr::Target::Cpu)] {
        let loaded = solidcraft_engine::doc::Document::from_json(&encoded).unwrap();
        h.app.session = Session::new(loaded);
        h.app.render_workspace.view.name = name.into();
        setups::load(&mut h.app);
        let camera = expected.camera;
        h.app.cam = camera;
        assert_eq!(hdr::input(&h.app, target), &hdr::Input::EmbeddedResource { resource });
        assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
        assert_eq!(image_request(&h.app).unwrap(), json!({"settings":expected}));
        let before = (h.app.session.revision, h.app.session.undo.len());
        h.app.run("view.render_select", json!({"kind":"environment"})).unwrap();
        h.frames(3);
        assert!(crate::scenario::rect_of("render:environment:intensity").is_some());
        h.frames(2);
        assert_eq!(setups::snapshot(&h.app).unwrap(), expected, "displaying degrees must preserve saved radians");
        setups::save(&mut h.app).unwrap();
        assert_eq!((h.app.session.revision, h.app.session.undo.len()), before);
        assert_eq!(h.app.cam, camera);
    }
}

#[test]
fn changed_hdr_bytes_reject_save_atomically_and_check_start_keep_the_expected_digest() {
    let mut h = Harness::new();
    enter(&mut h.app, true).unwrap();
    inspector::select_editor(&mut h.app, inspector::Editor::Output);
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    h.app.render_workspace.view.name = "Pinned HDR".into();
    h.app.render_workspace.view.cpu.environment = cpu::Environment::Hdr;
    let path = h.dir.join("changing.hdr");
    std::fs::write(&path, original_hdr_bytes(128, true)).unwrap();
    hdr::inspect(&mut h.app, hdr::Target::Cpu, path.to_string_lossy().into_owned()).unwrap();
    let input = h.app.render_workspace.view.cpu.hdr.clone();
    let request = image_request(&h.app).unwrap();
    let doc = h.app.session.doc.clone();
    let draft = h.app.render_workspace.view.draft.clone();
    let before = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    std::fs::write(&path, original_hdr_bytes(64, true)).unwrap();
    assert!(setups::save(&mut h.app).is_err());
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), before);
    assert_eq!(h.app.render_workspace.view.cpu.hdr, input);
    assert_eq!(h.app.render_workspace.view.draft, draft);
    assert_eq!(image_request(&h.app).unwrap(), request, "both buttons keep the verified digest, never silently accept new bytes");
    assert!(h.app.session.doc.render.resources.is_empty());
    hdr::inspect(&mut h.app, hdr::Target::Cpu, path.to_string_lossy().into_owned()).unwrap();
    assert_ne!(h.app.render_workspace.view.cpu.hdr.resource(), input.resource());
    setups::save(&mut h.app).unwrap();
    assert!(h.app.render_workspace.view.cpu.hdr.embedded());
    assert_eq!(h.app.session.doc.render.resources.len(), 1);
}

#[test]
fn hdr_picker_cancel_and_late_setup_method_or_input_answers_preserve_the_selection() {
    use crate::picker_test_support::{Slot, open};
    for target in [hdr::Target::Cpu, hdr::Target::Gpu] {
        for change in ["cancel", "setup", "method", "input"] {
            let mut h = Harness::new();
            h.app.render_workspace.view.method = if target == hdr::Target::Cpu { ImageMethod::CpuRaytrace } else { ImageMethod::GpuRaytrace };
            h.app.render_workspace.view.cpu.environment = cpu::Environment::Hdr;
            h.app.render_workspace.view.studio.environment = studio::Environment::Hdr;
            h.app.render_workspace.view.cpu.hdr = captured_hdr_input("original.hdr");
            h.app.render_workspace.view.studio.hdr = captured_hdr_input("original.hdr");
            let slot: Slot = Default::default();
            open(&mut h.app, &slot);
            hdr::pick(&mut h.app, target);
            let ctx = crate::fonts::test_context();
            crate::file_picker::tick(&mut h.app, &ctx);
            match change {
                "setup" => h.app.render_workspace.view.name = "another setup".into(),
                "method" => h.app.render_workspace.view.method = ImageMethod::Raster,
                "input" => {
                    h.app.render_workspace.view.cpu.hdr = captured_hdr_input("newer.hdr");
                    h.app.render_workspace.view.studio.hdr = captured_hdr_input("newer.hdr");
                }
                _ => {}
            }
            let before = hdr::input(&h.app, target).clone();
            let doc = h.app.session.doc.clone();
            let status = h.app.status.clone();
            *slot.borrow_mut() = Some(if change == "cancel" { None } else { Some("never-read-late.hdr".into()) });
            crate::file_picker::tick(&mut h.app, &ctx);
            assert_eq!(hdr::input(&h.app, target), &before, "{target:?}: {change}");
            assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
            assert_eq!(h.app.status, status, "the stale path must be discarded before trying to inspect it");
        }
    }
}

#[test]
fn embedded_hdr_runtime_requests_preserve_temporary_bindings_without_serializing_them() {
    use solidcraft_engine::render::job::ReflectionMode;
    for target in [hdr::Target::Cpu, hdr::Target::Gpu] {
        let mut h = Harness::new();
        h.app.preferences.render_legacy_image_defaults = true;
        h.app.run("solid.box", json!({"body_name":"Target","length":10,"width":20,"height":5})).unwrap();
        enter(&mut h.app, true).unwrap();
        inspector::select_editor(&mut h.app, inspector::Editor::Output);
        select_image_method(&mut h.app, if target == hdr::Target::Cpu { ImageMethod::CpuRaytrace } else { ImageMethod::GpuRaytrace });
        h.app.render_workspace.view.cpu.environment = cpu::Environment::Hdr;
        h.app.render_workspace.view.studio.environment = studio::Environment::Hdr;
        let path = h.dir.join("bindings.hdr");
        std::fs::write(&path, original_hdr_bytes(128, true)).unwrap();
        hdr::inspect(&mut h.app, target, path.to_string_lossy().into_owned()).unwrap();
        setups::save(&mut h.app).unwrap();
        let resource = hdr::input(&h.app, target).resource().unwrap();
        std::fs::remove_file(&path).unwrap();
        if target == hdr::Target::Cpu {
            cpu::set_reflection(&mut h.app, "Target", Some(ReflectionMode::Mirror)).unwrap();
        } else {
            let target: materials::Target = serde_json::from_value(h.app.run("render.scene", json!({})).unwrap()["bodies"][0].clone()).unwrap();
            let binding = materials::Binding::new(target, &h.app);
            h.app.render_workspace.view.materials.bindings.push(binding);
            // Even disabled, this assignment must not be lost on Save. It still
            // selects the runtime path and exercises the embedded HDR selector.
            h.app.render_workspace.view.materials.enabled = false;
        }
        let params = image_request(&h.app).unwrap();
        assert_eq!(params["environment"]["resource"], json!(resource));
        assert!(params["environment"].get("path").is_none());
        assert!(params["environment"].get("expected_sha256").is_none());
        if target == hdr::Target::Cpu {
            assert!(params["environment"].get("kind").is_none());
            assert_eq!(params["raytrace"]["reflections"]["bodies"][0]["body"], "Target");
        } else {
            assert_eq!(params["environment"]["kind"], "embedded_hdr");
        }
        let doc = h.app.session.doc.clone();
        let undo = h.app.session.undo.len();
        assert!(setups::save(&mut h.app).is_err());
        assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
        assert_eq!(h.app.session.undo.len(), undo);
        assert_eq!(image_request(&h.app).unwrap(), params);
    }
}

#[test]
fn cpu_progressive_sample_editor_uses_the_selected_profile_without_rewriting_existing_intent() {
    use solidcraft_engine::render::job::{CpuBudget, CpuProgressiveOptions};
    for (budget, maximum) in [(CpuBudget::Compact, 64), (CpuBudget::Studio, 1024), (CpuBudget::Workstation, 4096)] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1600,"height":2600}));
        enter(&mut h.app, true).unwrap();
        inspector::select_editor(&mut h.app, inspector::Editor::Sampling);
        h.app.session.set_render_job_host(None).unwrap();
        select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
        h.app.render_workspace.view.draft.width = 8;
        h.app.render_workspace.view.draft.height = 8;
        h.app.render_workspace.view.raytrace.budget = budget;
        h.app.render_workspace.view.raytrace.progressive = Some(CpuProgressiveOptions { samples_per_pixel: maximum + 1, seed: 82 });
        h.frames(3);
        h.step(&json!({"click":{"handle":format!("dialog:section:{}",h.app.tr("render.children.cpu"))}})).unwrap();
        h.frames(3);

        // Merely painting the constrained field must retain and refuse an incompatible draft.
        assert_eq!(h.app.render_workspace.view.raytrace.progressive.unwrap().samples_per_pixel, maximum + 1);
        assert!(image_request(&h.app).is_err());
        assert!(h.app.render_workspace.view.job.is_none());
        let before = h.app.session.doc.clone();
        let undo = h.app.session.undo.len();
        assert!(setups::save(&mut h.app).is_err());
        assert!(Arc::ptr_eq(&before, &h.app.session.doc));
        assert_eq!(h.app.session.undo.len(), undo);

        // A real edit respects the chosen profile, rather than offering the Workstation cap
        // under Compact and failing later when the person tries to save or start rendering.
        for _ in 0..8 {
            let field = crate::scenario::rect_of("render:cpu-spp-field").unwrap();
            let inspector = crate::scenario::rect_of("render:inspector").unwrap();
            let top = crate::scenario::rect_of("render:inspector:header").unwrap().bottom() + Tokens::get().space_8;
            let bottom = inspector.bottom() - Tokens::get().space_8;
            if field.top() >= top && field.bottom() <= bottom {
                break;
            }
            let delta = field.center().y.clamp(top + field.height() / 2.0, bottom - field.height() / 2.0) - field.center().y;
            h.call("ui.scroll", json!({"x":inspector.center().x,"y":(top+bottom)/2.0,"delta":delta}));
            h.frames(20);
        }
        let field = crate::scenario::rect_of("render:cpu-spp-field").unwrap();
        let inspector = crate::scenario::rect_of("render:inspector").unwrap();
        let top = crate::scenario::rect_of("render:inspector:header").unwrap().bottom() + Tokens::get().space_8;
        assert!(
            inspector.contains_rect(field) && field.top() >= top && field.bottom() <= inspector.bottom() - Tokens::get().space_8,
            "the sample editor must be visible before typing"
        );
        h.step(&json!({"click":[field.center().x,field.center().y],"double":true})).unwrap();
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        h.step(&json!({"text":"4097"})).unwrap();
        h.step(&json!({"key":"Enter"})).unwrap();
        h.frames(3);
        assert_eq!(h.app.render_workspace.view.raytrace.progressive, Some(CpuProgressiveOptions { samples_per_pixel: maximum, seed: 82 }));
        assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (8, 8));
        assert!(image_request(&h.app).is_ok());
        assert!(h.app.render_workspace.view.job.is_none(), "editing samples must never start a job");
    }
}

#[test]
fn cpu_resource_profile_edits_preserve_resolution_samples_and_saved_intent() {
    use solidcraft_engine::render::job::{CpuBudget, CpuProgressiveOptions};
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2600}));
    enter(&mut h.app, true).unwrap();
    inspector::select_editor(&mut h.app, inspector::Editor::Sampling);
    h.app.session.set_render_job_host(None).unwrap();
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    h.app.render_workspace.view.name = "Studio output".into();
    h.app.render_workspace.view.draft.width = 3840;
    h.app.render_workspace.view.draft.height = 2160;
    h.app.render_workspace.view.raytrace.budget = CpuBudget::Studio;
    h.app.render_workspace.view.raytrace.progressive = Some(CpuProgressiveOptions { samples_per_pixel: 128, seed: 82 });
    h.frames(3);
    h.step(&json!({"click":{"handle":format!("dialog:section:{}",h.app.tr("render.children.cpu"))}})).unwrap();
    h.frames(3);
    let expected = setups::snapshot(&h.app).unwrap();
    setups::save(&mut h.app).unwrap();
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);

    let saved = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":"render.cpu_budget_compact"}})).unwrap();
    h.frames(3);
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (3840, 2160));
    assert_eq!(h.app.render_workspace.view.raytrace.progressive.unwrap().samples_per_pixel, 128);
    assert_eq!(h.app.render_workspace.view.raytrace.budget, CpuBudget::Studio, "an invalid saved edit is refused without resizing");
    assert_eq!(h.app.session.doc, saved);
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(h.app.status.is_some(), "the refused budget needs its reason");
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
}

#[test]
fn large_image_display_uses_captured_profile_and_keeps_export_pixels_unchanged() {
    use solidcraft_engine::render::job::{CpuBudget, ImageRequest, RaytraceOptions};
    let request = ImageRequest {
        studio_weights: None,
        integrator: Integrator::Raytrace,
        raytrace: Some(RaytraceOptions { budget: CpuBudget::Studio, ..Default::default() }),
        camera: Camera::default(),
        width: 8,
        height: 8,
        background: [0; 3],
        transparent_background: false,
        environment: None,
        textures: None,
    };
    let limits = captured_image_limits(&request);
    assert_eq!(limits, CpuBudget::Studio.image_limits(false));
    assert_eq!(display_dimensions(7680, 4320, 4096).unwrap(), (4096, 2304));
    assert_eq!(display_dimensions(1920, 1080, 4096).unwrap(), (1920, 1080));
    assert!(display_dimensions(u32::MAX, 1, 0).is_err());
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(8200, 2, image::Rgba([42, 88, 123, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
    let encoded = png.into_inner();
    let before = encoded.clone();
    assert!(decode_image(&encoded, 8200, 2).is_err(), "legacy default remains compact");
    let display = decode_render_image(&encoded, 8200, 2, CpuBudget::Workstation.image_limits(false), 1024).unwrap();
    assert!(display.size[0] <= 1024 && display.size[1] >= 1);
    assert_eq!(encoded, before, "the retained image is not rewritten for its display cache");
    assert!(decode_render_image(&encoded, 8199, 2, CpuBudget::Workstation.image_limits(false), 1024).is_err());
}

#[test]
fn stage_actions_and_workspace_return_stay_visible_with_long_render_controls() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1280,"height":900}));
    enter(&mut h.app, true).unwrap();
    inspector::select_editor(&mut h.app, inspector::Editor::Sampling);
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    h.frames(3);
    let bar = crate::scenario::rect_of("render:stage_bar").unwrap();
    for id in ["render:stage:render.start", "render:stage:render.cancel"] {
        assert!(bar.contains_rect(crate::scenario::rect_of(id).unwrap()));
    }
    for key in ["render.save_setup", "render.reload_settings"] {
        assert!(h.widget(h.app.tr(key)).is_none(), "removed studio action {key}");
    }
    let at = crate::scenario::rect_of("workspace_switcher").unwrap().center();
    h.step(&json!({"click":[at.x,at.y]})).unwrap();
    h.step(&json!({"click":{"handle":"menu:ui.workspace.design"}})).unwrap();
    assert!(!active(&h.app));
}

#[test]
fn camera_draft_preserves_sampling_resource_picker_hit_targets() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2600}));
    enter(&mut h.app, true).unwrap();
    inspector::select_editor(&mut h.app, inspector::Editor::Sampling);
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    setups::save(&mut h.app).unwrap();
    setups::load(&mut h.app);
    let saved = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.frames(3);
    h.step(&json!({"click":{"handle":format!("dialog:section:{}",h.app.tr("render.children.cpu"))}})).unwrap();
    h.frames(3);
    let clean_picker = crate::scenario::rect_of("render:cpu-budget").unwrap();
    h.app.cam.yaw += 0.25;
    setups::refresh_dirty(&mut h.app);
    assert!(h.app.render_workspace.view.dirty);
    h.frames(3);
    assert_eq!(crate::scenario::rect_of("render:cpu-budget").unwrap(), clean_picker);
    assert_eq!(h.app.session.doc, saved, "navigation keeps the saved camera until Update shot");
    assert_eq!(h.app.session.undo.len(), undo);
    inspector::restore_camera(&mut h.app).unwrap();
    assert_eq!(h.app.cam, saved.render.setups["Studio"].camera);
}

#[test]
fn product_preview_exact_payload_preserves_mixed_surfaces_and_refuses_overflow() {
    let mut session = Session::default();
    for (name, x) in [("Paint", 0), ("Legacy", 20), ("Glass", 40)] {
        session.execute("solid.box", &json!({"base":[x,0,0],"length":10,"width":10,"height":10,"body_name":name})).unwrap();
    }
    session
        .execute("appearance.assign", &json!({"bodies":["Paint"],"color":"#c07030","pbr":{"metallic":0,"roughness":0.4,"dielectric_ior":1.5}}))
        .unwrap();
    session.execute("appearance.assign", &json!({"bodies":["Glass"],"color":"#336699","opacity":0.4})).unwrap();
    let original = session.doc.to_json();
    let scene = solidcraft_engine::product_scene::extract(&session).unwrap();
    let plan = preview_upload_plan(&scene, MAX_GEOMETRY_BYTES).unwrap();
    let (_, gpu) = gpu_scene(&scene).unwrap();
    assert_eq!((plan.pbr, plan.tris, plan.glass), (gpu.pbr.len(), gpu.tris.len(), gpu.glass.len()));
    assert!(plan.pbr > 0 && plan.tris > 0 && plan.glass > 0);
    assert_eq!(plan.source_triangles, scene.meshes.iter().map(|b| b.mesh.triangles.len()).sum::<usize>());
    assert!(preview_upload_plan(&scene, plan.bytes().unwrap() - 1).is_err());
    assert!(PreviewUploadPlan::default().add(true, false, usize::MAX, MAX_GEOMETRY_BYTES).is_err());
    assert_eq!(session.doc.to_json(), original);
    let mut invalid = scene.clone();
    let source = invalid.meshes[0].mesh.clone();
    Arc::make_mut(&mut invalid.meshes[0].mesh).triangles[0][0] = u32::MAX;
    assert!(gpu_scene(&invalid).is_err());
    assert!(gpu_scene(&scene).is_ok());
    assert!(Arc::ptr_eq(&scene.meshes[0].mesh, &source));
}

#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
#[test]
fn full_detail_preview_source_can_exceed_legacy_128_mib_without_mesh_reduction() {
    use solidcore_record::Vec3;
    use solidcraft_engine::geom::Mesh;
    use solidcraft_engine::render::{ProductMesh, surface::OpaqueSurface};
    // One immutable original triangle repeated in the index list; this tests
    // payload admission only. Actual SELENE upload is separate native evidence.
    let count = (128 * 1024 * 1024) / (3 * crate::gpu::PBR_SIZE) + 1;
    let mesh = Arc::new(Mesh {
        positions: vec![Vec3::ZERO, Vec3::X, Vec3::Y],
        normals: vec![Vec3::Z; 3],
        triangles: vec![[0, 1, 2]; count],
        ..Default::default()
    });
    let body = ProductMesh {
        name: "Original admission fixture".into(),
        mesh: mesh.clone(),
        color: [200, 100, 50],
        opacity: 1.0,
        surface: Some(OpaqueSurface::new(Vec3::new(0.4, 0.2, 0.1), 0.0, 0.4, 1.5).unwrap()),
        transmission: None,
        faces: Vec::new(),
    };
    let scene = ProductScene { meshes: vec![body], bounds: mesh.bounds() };
    assert!(preview_upload_plan(&scene, 128 * 1024 * 1024).is_err());
    let plan = preview_upload_plan(&scene, MAX_GEOMETRY_BYTES).unwrap();
    assert_eq!(plan.source_triangles, count);
    assert_eq!(plan.bytes().unwrap(), count * 3 * crate::gpu::PBR_SIZE);
    assert!(Arc::ptr_eq(&scene.meshes[0].mesh, &mesh));
    let mut maximum = PreviewUploadPlan::default();
    assert!(maximum.add(true, false, MAX_GEOMETRY_BYTES / (3 * crate::gpu::PBR_SIZE) + 1, MAX_GEOMETRY_BYTES).is_err());
}

/// The product scene is extracted again only when what it shows can change: a selection or a
/// view change keeps its signature (they extracted every body and built every vertex again),
/// an appearance edit, hiding a body or a geometry edit changes it.
#[test]
fn product_preview_signature_follows_what_the_preview_shows() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    let body = crate::open_meshing::session_world(&h.app.session).bodies[0].name.clone();
    let first = preview_signature(&mut h.app);
    assert_eq!(preview_signature(&mut h.app), first, "nothing changed");
    h.app.run("select.set", json!({"items": [{"type": "body", "name": &body}]})).unwrap();
    assert_eq!(preview_signature(&mut h.app), first, "a selection");
    h.app.run("select.clear", json!({})).unwrap();
    let _ = h.call("ui.view", json!({"view": "top"}));
    assert_eq!(preview_signature(&mut h.app), first, "a view change");
    h.app.run("appearance.assign", json!({"bodies": [&body], "color": "#336699"})).unwrap();
    let painted = preview_signature(&mut h.app);
    assert_ne!(painted, first, "an appearance edit");
    h.app.run("browser.visibility", json!({"items": [&body], "visible": false})).unwrap();
    assert_ne!(preview_signature(&mut h.app), painted, "hiding a body");
    h.app.run("browser.visibility", json!({"items": [&body], "visible": true})).unwrap();
    h.app.run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [20, 0, 0], "operation": "new"})).unwrap();
    assert_ne!(preview_signature(&mut h.app), painted, "a geometry edit");
}

#[test]
fn refused_product_capture_is_cached_until_identity_changes_or_explicit_retry() {
    let mut h = Harness::new();
    let calls = std::cell::Cell::new(0);
    let signature = (7, 123, 99, MAX_GEOMETRY_BYTES as u64);
    for _ in 0..8 {
        assert_eq!(
            h.app
                .render_workspace
                .prepare_preview(signature, || {
                    calls.set(calls.get() + 1);
                    Err("original bounded refusal".into())
                })
                .unwrap_err(),
            "original bounded refusal"
        );
    }
    assert_eq!(calls.get(), 1, "same refused source is not rebuilt every frame");
    assert!(h.app.render_workspace.scene.is_none() && h.app.render_workspace.key.is_none());
    assert!(h.app.render_workspace.slot.lock().unwrap().is_none());
    assert!(
        h.app
            .render_workspace
            .prepare_preview((8, 123, 99, MAX_GEOMETRY_BYTES as u64), || {
                calls.set(calls.get() + 1);
                Err("new revision".into())
            })
            .is_err()
    );
    assert_eq!(calls.get(), 2);
    invalidate_scene(&mut h.app);
    assert!(h.app.render_workspace.scene_signature.is_none() && h.app.render_workspace.scene_error.is_none());
    let mut session = Session::default();
    session.execute("solid.box", &json!({"length":1,"width":1,"height":1})).unwrap();
    h.app.render_workspace.prepare_preview(signature, || solidcraft_engine::product_scene::extract(&session).map_err(|e| e.to_string())).unwrap();
    assert!(h.app.render_workspace.scene_error.is_none());
    assert!(h.app.render_workspace.preview_upload_bytes.unwrap() > 0);
    assert!(h.app.render_workspace.key.is_some() && h.app.render_workspace.scene.is_some());
}

#[test]
fn product_preview_error_uses_shared_wrapped_surface_and_explicit_retry_target() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1280,"height":800}));
    enter(&mut h.app, true).unwrap();
    h.frames(3);
    let rect = egui::Rect::from_min_size(egui::pos2(260.0, 200.0), egui::vec2(550.0, 360.0));
    let message = "The original captured product geometry exceeds its bounded preview admission. The renderer must preserve all exact body identities and cannot silently simplify or drop geometry to paint this view.";
    let mut output = h.ctx.run_ui(
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))), ..Default::default() },
        |ui| preview_error(&mut h.app, ui, rect, message),
    );
    output.textures_delta.clear();
    let note = crate::scenario::rect_of("dialog:report:render.preview_failure").unwrap();
    let retry = crate::scenario::rect_of("render:preview-retry").unwrap();
    assert!(rect.contains_rect(note) && rect.contains_rect(retry));
    assert!(note.height() > Tokens::get().font_body, "the detailed failure wraps instead of escaping the viewport");
    assert!(crate::theme::contrast_ratio(Tokens::get().form_error, Tokens::get().form_error_fill) >= 3.0);
}

#[test]
fn finishing_backdrop_and_raw_retention_save_reload_without_a_gpu_or_hidden_transport_edits() {
    use solidcraft_engine::render::job::finishing::{Backdrop, BackdropMode, Finishing};
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2200}));
    enter(&mut h.app, true).unwrap();
    h.app.session.set_render_job_host(None).unwrap();
    select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    h.app.run("view.render_stage", json!({"stage":"output"})).unwrap();
    h.app.render_workspace.view.name = "Linear studio".into();
    h.app.render_workspace.view.studio.finishing =
        Finishing { rgb_gain: [1.3, 0.8, 0.9], saturation: 0.4, contrast: 1.2, vignette: 0.3, ..Default::default() };
    h.app.render_workspace.view.studio.backdrop_enabled = true;
    h.app.render_workspace.view.studio.backdrop = Backdrop { mode: BackdropMode::Radial, center: [0.25, 0.7], ..Default::default() };
    h.app.render_workspace.view.studio.linear_exr = true;
    let expected = setups::snapshot(&h.app).unwrap();
    setups::save(&mut h.app).unwrap();
    h.app.render_workspace.view.studio = studio::View::default();
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    assert!(!h.app.render_workspace.view.dirty);
    assert!(!studio::linear_export_available(&h.app));
    let request = image_request(&h.app).unwrap();
    assert_eq!(request["settings"]["job"]["options"]["linear_exr"], true);
    assert_eq!(request["settings"]["job"]["options"]["finishing"]["rgb_gain"], json!([1.3, 0.8, 0.9]));
    assert!(image_params(&h.app).is_err(), "offline editing is not a claim of executable GPU capabilities");
    h.app.render_workspace.view.transparent = true;
    assert!(setups::snapshot(&h.app).is_err(), "gradient cannot silently disappear behind alpha mode");
    h.app.render_workspace.view.transparent = false;
    let before = h.app.session.doc.to_json();
    h.frames(3);
    h.app.run("view.render_select", json!({"kind":"effects"})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:post-reset"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.studio.finishing, Finishing::default());
    assert!(h.app.render_workspace.view.studio.linear_exr);
    assert!(h.app.render_workspace.view.studio.backdrop_enabled);
    assert!(!h.app.render_workspace.view.dirty, "Effects already saved the numerical edit");
    assert_ne!(h.app.session.doc.to_json(), before, "Effects saves its job patch through the shared editor");
    let reset = setups::snapshot(&h.app).unwrap();
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), reset);
}

#[test]
fn missing_linear_master_is_never_reconstructed_from_a_finished_png() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":1800}));
    enter(&mut h.app, true).unwrap();
    let id = completed_preview(&mut h, [21, 37, 55]);
    let bytes = h.app.session.render_job_handle(id).unwrap().frame().unwrap().unwrap().png().to_vec();
    h.app.render_workspace.view.completed = Some(id);
    h.app.render_workspace.view.studio.export_exr = true;
    assert!(!studio::linear_export_available(&h.app));
    let temp = std::env::temp_dir().join(format!("solidcraft-missing-linear-{}.exr", std::process::id()));
    assert!(h.app.session.execute("render.export", &json!({"job":id,"path":temp,"format":"exr"})).is_err());
    assert!(!temp.exists());
    assert_eq!(h.app.session.render_job_handle(id).unwrap().frame().unwrap().unwrap().png(), bytes);
}

#[cfg(not(target_arch = "wasm32"))]
mod linear_format_choices {
    use super::*;
    use solidcraft_engine::render::job;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    struct Executor(Arc<AtomicBool>);
    struct Work {
        ready: Arc<AtomicBool>,
        preview: Option<job::ImageWorkPreview>,
        png: Option<Vec<u8>>,
        exr: Option<Vec<u8>>,
    }
    impl job::ImageWork for Work {
        fn step(&mut self, _: usize) -> job::Result<job::TaskPoll> {
            if self.preview.is_some() || !self.ready.load(Ordering::Acquire) {
                return Ok(job::TaskPoll::Pending(job::Progress { phase: job::Phase::Running, done: 1, total: 2 }));
            }
            Ok(job::TaskPoll::Complete(self.png.take().unwrap()))
        }
        fn take_preview(&mut self) -> Option<job::ImageWorkPreview> {
            self.preview.take()
        }
        fn take_linear_exr(&mut self) -> Option<Vec<u8>> {
            self.exr.take()
        }
    }
    impl job::ImageExecutor for Executor {
        fn descriptor(&self) -> job::Result<ExecutorDescriptor> {
            let mut descriptor = job::CpuHost::cooperative().descriptor()?;
            descriptor.id = ExecutorId::new("test-linear-master").unwrap();
            descriptor.profiles[0].features.push(solidcraft_engine::render::RenderFeature::Progressive);
            Ok(descriptor)
        }
        fn preview_samples(&self, _: job::JobOptions) -> Option<u32> {
            Some(2)
        }
        fn supports_linear_exr(&self) -> bool {
            true
        }
        fn admit(&self, scene: &ProductScene, request: &job::ImageRequest, _: job::JobOptions) -> job::Result<job::Admission> {
            let mut admission = job::admit(scene, request)?;
            admission.estimated_total_bytes += job::linear_exr::encoded_limit(request.width, request.height)? + 16 * 4;
            Ok(admission)
        }
        fn begin(
            &self,
            _: ProductScene,
            request: job::ImageRequest,
            options: job::JobOptions,
            cancel: job::CancelToken,
        ) -> job::Result<Box<dyn job::ImageWork>> {
            use job::ImageWork;
            assert_eq!((request.width, request.height), (2, 2));
            let mut png = job::PngTask::new(&request, [200, 30, 50, 255].repeat(4), cancel.clone())?;
            let job::TaskPoll::Complete(png) = png.step(4)? else { panic!("the two-row PNG must finish") };
            let exr = if options.linear_exr {
                let mut task = job::linear_exr::ExrTask::new(2, 2, vec![[8.0, 0.25, 0.5, 1.0]; 4], cancel)?;
                assert!(task.step(2)?);
                Some(task.finish()?.0)
            } else {
                None
            };
            Ok(Box::new(Work {
                ready: self.0.clone(),
                preview: Some(job::ImageWorkPreview { png: png.clone(), samples_per_pixel: 1 }),
                png: Some(png),
                exr,
            }))
        }
    }

    fn frame(h: &mut Harness, events: Vec<egui::Event>) {
        let mut output = h.ctx.run_ui(
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 100.0))), events, ..Default::default() },
            |ui| {
                studio::format_choices(&mut h.app, ui);
            },
        );
        output.textures_delta.clear();
    }
    pub(super) fn choose(h: &mut Harness, target: &str) {
        frame(h, vec![]);
        let at = crate::scenario::rect_of(target).unwrap().center();
        for pressed in [true, false] {
            frame(
                h,
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() },
                ],
            );
        }
    }

    #[test]
    fn exr_choices_use_the_actual_retained_final_master_and_keep_selection_during_previews() {
        let mut h = Harness::new();
        h.app.render_workspace.view.studio.linear_exr = true;
        choose(&mut h, "render:format-exr");
        assert!(!h.app.render_workspace.view.studio.export_exr, "a future retention flag is not an exportable master");
        let ready = Arc::new(AtomicBool::new(true));
        let host = job::ImageHost::with_executor(Arc::new(Executor(ready.clone()))).unwrap();
        h.app.session.register_render_job_host(host).unwrap();
        let request = json!({"progressive":true,"samples":2,"linear_exr":true,"settings":{"width":2,"height":2}});
        let reply = h.app.session.execute("render.start", &request).unwrap();
        let completed = serde_json::from_value(reply["job"].clone()).unwrap();
        let completed_handle = h.app.session.render_job_handle(completed).unwrap();
        assert_eq!(completed_handle.wait_timeout(Duration::from_secs(10)).unwrap().progress.phase, job::Phase::Completed);
        let master = completed_handle.frame().unwrap().unwrap();
        let raw = master.linear_exr().expect("the real host must retain the producer's valid native master");
        assert_eq!(&raw[..4], &20_000_630_u32.to_le_bytes());
        h.app.render_workspace.view.completed = Some(completed);
        h.app.render_workspace.view.method = ImageMethod::Raster;
        h.app.render_workspace.view.studio.linear_exr = false;
        assert!(studio::linear_export_available(&h.app));
        choose(&mut h, "render:format-exr");
        assert!(h.app.render_workspace.view.studio.export_exr, "completed data determines export, independently of the current draft");
        frame(&mut h, vec![]);
        assert!(h.app.render_workspace.view.studio.export_exr, "painting cannot reset an explicit EXR choice");
        let path = h.dir.join("retained-master.exr");
        h.app.session.execute("render.export", &json!({"job":completed,"path":path,"format":"exr"})).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), raw);

        ready.store(false, Ordering::Release);
        let reply = h.app.session.execute("render.start", &request).unwrap();
        let current = serde_json::from_value(reply["job"].clone()).unwrap();
        let current_handle = h.app.session.render_job_handle(current).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while current_handle.preview().unwrap().is_none() {
            assert!(Instant::now() < deadline, "the deterministic producer must publish its first pass");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(current_handle.frame().unwrap().is_none());
        h.app.render_workspace.view.job = Some(current);
        frame(&mut h, vec![]);
        assert!(studio::linear_export_available(&h.app), "the current provisional image must not hide the previous final's master");
        assert!(h.app.render_workspace.view.studio.export_exr);
        h.app.render_workspace.view.completed = None;
        assert!(!studio::linear_export_available(&h.app), "a provisional PNG cannot enable raw export");
        h.app.render_workspace.view.studio.export_exr = false;
        choose(&mut h, "render:format-exr");
        assert!(!h.app.render_workspace.view.studio.export_exr);
        h.app.render_workspace.view.completed = Some(completed);
        choose(&mut h, "render:format-exr");
        assert!(h.app.render_workspace.view.studio.export_exr);
        current_handle.cancel().unwrap();
        h.app.session.set_render_job_host(Some(job::CpuHost::cooperative())).unwrap();
        assert!(h.app.session.render_job_handle(completed).is_err(), "clearing retained session ownership removes that export source");
        assert!(!studio::linear_export_available(&h.app));
        choose(&mut h, "render:format-png");
        assert!(!h.app.render_workspace.view.studio.export_exr, "PNG remains selectable after the master disappears");
    }
}

#[test]
fn render_workspace_stage_commands_keep_registered_actions_without_a_ribbon() {
    let mut h = Harness::new();
    enter(&mut h.app, true).unwrap();
    assert!(crate::workspace::visible_tabs(&h.app).is_empty());
    for command in solidcraft_engine::command_specs()
        .into_iter()
        .filter(|c| c.id.starts_with("render.") || c.tab.starts_with("RENDER") || c.id == "view.shots_strip")
    {
        assert!(solidcraft_engine::find_command(command.id).is_some());
        assert!(crate::workspace::destination(command).is_none(), "{} has a ribbon destination", command.id);
    }
    for tab in ["RENDER", "RENDER APPEARANCE", "RENDER SCENE", "RENDER CAMERA"] {
        assert!(crate::workspace::panels(tab).is_empty());
    }
    h.frames(3);
    assert!(crate::scenario::rect_of("ribbon:card").is_none());
    assert!(crate::scenario::rect_of("tab:RENDER").is_none());
    assert!(crate::scenario::rect_of("render:hud").is_none());
    assert_eq!(h.app.render_workspace.view.stage, Stage::Look);
    assert_eq!(inspector::selected_editor(&h.app), inspector::Editor::Scene);
}

#[test]
fn render_workspace_stage_chips_dispatch_without_mutations_and_remember_each_document() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1440,"height":1000}));
    enter(&mut h.app, true).unwrap();
    let document = h.app.session.doc.clone();
    let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    for stage in Stage::ALL {
        let selection = h.app.render_workspace.view.selection.clone();
        h.frames(3);
        h.step(&json!({"click":{"handle":format!("render:stage:{}",stage.key())}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.render_workspace.view.stage, stage);
        assert_eq!(h.app.render_workspace.view.selection, selection);
        assert_eq!(crate::scenario::rect_of("render:pick").is_some(), stage == Stage::Look);
        assert_eq!(crate::scenario::rect_of("render:preview_note").is_some(), stage == Stage::Look);
        assert!(Arc::ptr_eq(&h.app.session.doc, &document));
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
        assert!(h.app.render_workspace.view.job.is_none());
        assert!(h.app.render_workspace.view.completed.is_none());
        // Registered editor navigation remains available independently of the stage.
        for command in ["render.status", "render.backends"] {
            h.app.start(command);
            h.frames(2);
            assert_eq!(inspector::selected_editor(&h.app), inspector::Editor::Check);
        }
    }
    h.app.run("view.render_stage", json!({"stage":"frame"})).unwrap();
    crate::documents::open_tab(&mut h.app, Session::default()).unwrap();
    assert_eq!(h.app.render_workspace.view.stage, Stage::Look);
    h.app.run("view.render_stage", json!({"stage":"light"})).unwrap();
    crate::documents::switch(&mut h.app, 0);
    enter(&mut h.app, true).unwrap();
    assert_eq!(h.app.render_workspace.view.stage, Stage::Frame);
    crate::documents::switch(&mut h.app, 1);
    enter(&mut h.app, true).unwrap();
    assert_eq!(h.app.render_workspace.view.stage, Stage::Light);
}

#[test]
fn render_workspace_stage_shell_degrades_without_overlap_at_small_window_sizes() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut h = Harness::new();
        enter(&mut h.app, true).unwrap();
        h.app.ui.layout = mode;
        for (width, height) in [
            (1440, 900),
            (1400, 900),
            (1100, 700),
            (900, 700),
            (800, 600),
            (700, 600),
            (640, 480),
            (320, 240),
            (240, 160),
            (120, 80),
            (32, 32),
            (1, 1),
        ] {
            h.call("ui.resize", json!({"width":width,"height":height}));
            for stage in Stage::ALL {
                h.app.run("view.render_stage", json!({"stage":stage.key()})).unwrap();
                h.frames(3);
                let bar = crate::scenario::rect_of("render:stage_bar").unwrap();
                let workspace = crate::scenario::rect_of("ribbon:workspace").unwrap();
                assert!(bar.is_finite() && workspace.is_finite());
                assert_eq!(bar.top(), workspace.top());
                assert_eq!(bar.height(), workspace.height());
                assert!(!bar.intersect(workspace).is_positive());
                if width >= 1100 {
                    assert_eq!(bar.height(), 88.0);
                    assert_eq!(workspace.width(), 104.0);
                    assert_eq!(bar.left() - workspace.right(), 8.0);
                    let header = crate::scenario::rect_of("render:stage_header").unwrap();
                    let tools = crate::scenario::rect_of("render:stage_tools").unwrap();
                    assert_eq!(header.height(), 36.0);
                    assert_eq!(tools.height(), 52.0);
                    assert_eq!(header.bottom(), tools.top());
                    assert_eq!(header.top(), bar.top());
                    assert_eq!(tools.bottom(), bar.bottom());
                }
                let docks: Vec<_> =
                    ["render:browser", "render:inspector"].iter().filter_map(|id| h.app.shell.workspace_panels.get(id).map(|(r, _)| *r)).collect();
                if let [browser, inspector] = docks.as_slice() {
                    assert!((browser.top() - inspector.top()).abs() < 1.0);
                    assert!(browser.bottom() < inspector.bottom());
                    assert!(!browser.intersect(*inspector).is_positive());
                    if width >= 800 {
                        let compact = crate::render_ui::compact(&h.app, width as f32);
                        assert_eq!(browser.width(), if compact { crate::frame::DIALOG_MIN_W } else { 266.0 });
                        assert_eq!(inspector.width(), if compact { crate::frame::DIALOG_MIN_W } else { 305.0 });
                        assert_eq!(browser.top() - bar.bottom(), 8.0);
                        let (strip, _) = h.app.shell.workspace_panels.get("render:shots_strip").unwrap();
                        let library = h.app.shell.workspace_panels["render:appearances"].0;
                        assert_eq!(strip.top() - library.bottom(), 8.0);
                        assert_eq!(library.top() - browser.bottom(), 8.0);
                        assert_eq!(strip.height(), 48.0);
                    }
                }
                if (640..800).contains(&width) {
                    assert!(crate::scenario::rect_of("render:browser:show").is_some());
                    assert!(!h.app.shell.workspace_panels.contains_key("render:browser"));
                }
                if width >= 700 {
                    for dock in &docks {
                        assert!(!dock.x_range().contains(h.app.viewport.rect.unwrap().center().x), "{width}: {dock:?}");
                    }
                }
                let mut docks = docks;
                if let Some((panel, _)) = h.app.shell.workspace_panels.get("render:appearances") {
                    for dock in &docks {
                        assert!(!panel.intersect(*dock).is_positive());
                    }
                    docks.push(*panel);
                }
                for dock in docks {
                    assert!(!dock.intersect(bar).is_positive(), "{width}×{height}: {dock:?} {bar:?}");
                    if let Some((strip, _)) = h.app.shell.workspace_panels.get("render:shots_strip") {
                        assert!(!dock.intersect(*strip).is_positive(), "{width}×{height}: {dock:?} {strip:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn render_workspace_focus_restores_docks_without_design_history_or_camera_changes() {
    for (side, folded, shots) in [("left", false, true), ("right", true, false)] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":900}));
        enter(&mut h.app, true).unwrap();
        h.app.preferences.render_browser_side = side.into();
        h.app.preferences.render_inspector_side = if side == "left" { "right" } else { "left" }.into();
        h.app.preferences.render_browser_width = 350.0;
        h.app.preferences.render_inspector_width = 390.0;
        h.app.preferences.render_shots_strip = shots;
        h.app.preferences.browser_collapsed = folded;
        h.app.shell.render_browser_open = folded;
        h.frames(3);
        let dock_preferences = h.app.prefs();
        let docks = h.app.shell.workspace_panels.clone();
        let camera = (h.app.cam, h.app.session.camera);
        let document = h.app.session.doc.clone();
        let state = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.is_dirty());
        let before = h.app.viewport.layout.as_ref().unwrap().content;
        h.app.run("view.render_focus", json!({"on":true})).unwrap();
        h.frames(3);
        assert!(crate::render_ui::focused(&h.app));
        crate::shell::register_panel(&mut h.app, "render:stage_panel", Some(before), Some(crate::ui_kit::Edge::Left));
        assert!(!h.app.shell.workspace_panels.contains_key("render:stage_panel"));
        for id in ["render:browser", "render:appearances", "render:inspector", "render:shots_strip", "render:shots:show"] {
            assert!(!h.app.shell.workspace_panels.contains_key(id));
            assert!(crate::scenario::rect_of(id).is_none());
        }
        for id in ["render:focus:exit", "render:stage_bar", "ribbon:workspace"] {
            assert!(crate::scenario::rect_of(id).is_some(), "{id}");
        }
        let layout = h.app.viewport.layout.as_ref().unwrap();
        assert!(layout.content.width() > before.width());
        assert!(layout.axis_view.is_positive() && layout.navigation.is_positive());
        assert_eq!(h.app.prefs(), dock_preferences);
        h.step(&json!({"click":{"handle":"render:focus:exit"}})).unwrap();
        h.frames(3);
        assert!(!h.app.shell.render_focus);
        assert_eq!(h.app.shell.workspace_panels.len(), docks.len());
        for (id, (rect, edge)) in docks {
            let (restored_rect, restored_edge) = h.app.shell.workspace_panels.get(id).unwrap();
            assert_eq!(*restored_rect, rect);
            assert_eq!(restored_edge.as_ref().map(std::mem::discriminant), edge.as_ref().map(std::mem::discriminant));
        }
        assert_eq!(h.app.prefs(), dock_preferences);
        assert_eq!(h.app.preferences.browser_collapsed, folded);
        assert_eq!(h.app.shell.render_browser_open, folded);
        assert_eq!((h.app.cam, h.app.session.camera), camera);
        assert!(Arc::ptr_eq(&h.app.session.doc, &document));
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.is_dirty()), state);
        assert!(!Harness::new().app.shell.render_focus);
    }
}

#[test]
fn render_workspace_focus_tab_only_toggles_render_without_text_focus() {
    let mut h = Harness::new();
    h.step(&json!({"key":"Tab"})).unwrap();
    assert!(!h.app.shell.render_focus);
    enter(&mut h.app, true).unwrap();
    h.frames(3);
    h.ctx.memory_mut(|memory| {
        if let Some(id) = memory.focused() {
            memory.surrender_focus(id);
        }
    });
    h.step(&json!({"key":"Tab"})).unwrap();
    assert!(h.app.shell.render_focus);
    h.step(&json!({"key":"Tab"})).unwrap();
    assert!(!h.app.shell.render_focus);
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:library-search"}})).unwrap();
    assert!(h.ctx.egui_wants_keyboard_input());
    h.step(&json!({"key":"Tab"})).unwrap();
    assert!(!h.app.shell.render_focus);
}

#[test]
fn render_workspace_compact_browser_card_and_focus_preserve_remembered_widths() {
    for side in ["left", "right"] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":900}));
        enter(&mut h.app, true).unwrap();
        h.app.preferences.render_browser_side = side.into();
        h.app.preferences.render_inspector_side = if side == "left" { "right" } else { "left" }.into();
        h.app.preferences.render_browser_width = 320.0;
        h.app.preferences.render_inspector_width = 360.0;
        h.frames(3);
        for width in [1100, 900, 700] {
            h.call("ui.resize", json!({"width":width,"height":700}));
            h.frames(3);
            if width == 700 {
                h.step(&json!({"click":{"handle":"render:browser:show"}})).unwrap();
                h.frames(3);
                let (browser, reservation) = h.app.shell.workspace_panels["render:browser"];
                let (inspector, _) = h.app.shell.workspace_panels["render:inspector"];
                assert!(reservation.is_none(), "narrow Browser overlays the canvas");
                assert!(!browser.intersect(inspector).is_positive());
                h.app.run("view.render_focus", json!({"on":true})).unwrap();
                h.frames(3);
                h.app.run("view.render_focus", json!({"on":false})).unwrap();
                h.frames(3);
                assert!(h.app.shell.render_browser_open);
                assert!(h.app.shell.workspace_panels.contains_key("render:browser"));
                h.step(&json!({"click":{"handle":"render:browser:hide"}})).unwrap();
                h.frames(3);
                assert!(!h.app.shell.workspace_panels.contains_key("render:browser"));
            }
            assert_eq!(h.app.preferences.render_browser_width, 320.0);
            assert_eq!(h.app.preferences.render_inspector_width, 360.0);
        }
        h.call("ui.resize", json!({"width":1400,"height":900}));
        h.frames(3);
        assert_eq!(h.app.shell.workspace_panels["render:browser"].0.width(), 320.0);
        assert_eq!(h.app.shell.workspace_panels["render:inspector"].0.width(), 360.0);
    }
}

#[test]
fn render_workspace_focus_messages_have_five_complete_locales() {
    let keys = [
        "cmd.view.render_focus",
        "cmd.view.render_browser",
        "render.exit_focus",
        "render.fold_browser",
        "render.spot_cone",
        "render.emitted_axis",
        "render.inner_half_angle",
        "render.outer_half_angle",
        "render.layer_off_suffix",
        "render.tone_reinhard",
        "render.tone_aces_fitted",
        "render.size_hd",
        "render.size_full_hd",
        "render.size_4k_uhd",
        "render.result_actual_size",
        "render.property_unknown",
        "render.property_fallback",
        "render.property_source",
        "render.property_qualifier",
        "render.property_conditions",
        "render.property_uncertainty",
        "render.error_setup_panicked",
        "render.error_setup_worker",
        "render.error_setup_stopped",
        "render.error_gpu_samples",
        "render.error_preview_device",
        "render.error_finish_operation",
        "render.error_no_completed_image",
        "render.error_provisional_identity",
        "render.error_checkpoint_dimensions",
        "render.error_display_dimensions",
        "render.error_display_fit",
        "render.error_display_width",
        "render.error_display_height",
        "render.error_image_profile",
        "render.error_decode_budget",
        "render.error_image_dimensions",
        "render.error_upload_slot",
        "render.error_export_document",
        "render.error_geometry_size",
        "render.error_vertex_envelope",
        "render.error_body_bound",
        "render.error_triangle_count",
        "render.error_triangle_bound",
        "render.error_face_cap",
        "render.error_opacity",
        "render.error_duplicate_face",
        "render.error_pbr_opacity",
        "render.error_vertex_allocation",
        "render.error_product_triangle",
        "render.error_surface_normals",
        "render.error_vertex_packing",
        "render.error_gpu_preview",
        "render.error_fixed_preview",
        "render.error_finish_gesture",
        "render.error_no_scene",
        "render.error_no_geometry",
        "render.error_visible_solid",
        "render.error_area_samples",
        "render.error_quality_method",
        "render.error_quality_projection",
        "render.error_denoise_setting",
        "render.error_quality_bounds",
        "render.error_library_reply",
        "render.error_source_preview",
        "render.error_glass_recipe",
        "render.error_physical_glass",
        "render.error_scene_bodies",
        "render.error_scene_body_limit",
        "render.error_scene_identity",
        "render.error_material_binding",
        "render.error_surface_reply",
        "render.error_binding_allocation",
        "render.error_job_identity",
        "render.error_regrade_identity",
        "render.error_job_action",
        "render.error_interactive_command",
    ];
    for (language, source) in [
        ("en", include_str!(concat!(env!("OUT_DIR"), "/locales/en.json"))),
        ("ja", include_str!(concat!(env!("OUT_DIR"), "/locales/ja.json"))),
        ("zh", include_str!(concat!(env!("OUT_DIR"), "/locales/zh.json"))),
        ("de", include_str!(concat!(env!("OUT_DIR"), "/locales/de.json"))),
        ("ko", include_str!(concat!(env!("OUT_DIR"), "/locales/ko.json"))),
    ] {
        let translated: std::collections::BTreeMap<String, String> = serde_json::from_str(source).unwrap();
        let locale = crate::i18n::Locale::for_language(language);
        for key in keys {
            let value = translated.get(key).unwrap_or_else(|| panic!("{language}: missing {key}"));
            assert!(!value.is_empty());
            assert_eq!(locale.text(key), value);
        }
        let sentence = locale.format("render.property_source", &[("id", "A"), ("locator", "B")]);
        assert!(sentence.contains('A') && sentence.contains('B') && !sentence.contains('{'));
    }
}

#[test]
fn render_workspace_chrome_uses_tokens_instead_of_literal_colours() {
    use syn::visit::{self, Visit};
    #[derive(Default)]
    struct Colours(Vec<String>);
    impl<'ast> Visit<'ast> for Colours {
        fn visit_expr_path(&mut self, expr: &'ast syn::ExprPath) {
            let names: Vec<_> = expr.path.segments.iter().map(|s| s.ident.to_string()).collect();
            if names.iter().any(|s| s == "Color32") && names.last().is_some_and(|s| matches!(s.as_str(), "WHITE" | "BLACK" | "GRAY")) {
                self.0.push(names.join("::"));
            }
            visit::visit_expr_path(self, expr);
        }
        fn visit_expr_call(&mut self, expr: &'ast syn::ExprCall) {
            if let syn::Expr::Path(path) = expr.func.as_ref() {
                let names: Vec<_> = path.path.segments.iter().map(|s| s.ident.to_string()).collect();
                if names.iter().any(|s| s == "Color32")
                    && names.last().is_some_and(|s| s.starts_with("from_rgb") || s == "from_gray")
                    && expr.args.iter().all(|arg| matches!(arg, syn::Expr::Lit(_)))
                {
                    self.0.push(names.join("::"));
                }
            }
            visit::visit_expr_call(self, expr);
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            name.starts_with("render_workspace") && name.ends_with(".rs") && !name.contains("tests")
        })
        .collect();
    let mut folders = vec![root.join("render_ui")];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push(path);
            }
        }
    }
    for path in files {
        let source = std::fs::read_to_string(&path).unwrap();
        let mut colours = Colours::default();
        colours.visit_file(&syn::parse_file(&source).unwrap());
        assert!(colours.0.is_empty(), "{}: {:?}", path.display(), colours.0);
    }
}

#[test]
fn render_workspace_shots_toggle_persists_and_design_roundtrip_preserves_state() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1440,"height":1000}));
    h.app.run("solid.box", json!({"length":20,"width":15,"height":10,"body_name":"Subject"})).unwrap();
    h.app.session.selection = vec![solidcraft_engine::Sel::Body { name: "Subject".into() }];
    {
        let selection = object_selection(&h.app);
        selection_editor(&mut h.app, &selection);
    }
    h.app.cam.yaw = 0.71;
    h.app.cam.pitch = 0.23;
    h.app.cam.roll = 0.17;
    let camera = h.app.cam;
    let selection = h.app.session.selection.clone();
    let document = h.app.session.doc.clone();
    let state = (
        h.app.session.undo.len(),
        h.app.session.redo.len(),
        h.app.session.revision,
        h.app.ui.show_timeline,
        h.app.preferences.timeline_left,
        h.app.preferences.timeline_collapsed,
    );
    enter(&mut h.app, true).unwrap();
    h.frames(3);
    let strip = crate::scenario::rect_of("render:shots_strip").unwrap();
    assert!((strip.height() - 48.0).abs() <= 1.0, "{strip:?}");
    h.step(&json!({"click":{"handle":"render:shots:hide"}})).unwrap();
    assert!(!h.app.preferences.render_shots_strip);
    enter(&mut h.app, false).unwrap();
    h.frames(3);
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.session.selection, selection);
    assert_eq!(h.app.session.doc, document);
    assert_eq!(
        state,
        (
            h.app.session.undo.len(),
            h.app.session.redo.len(),
            h.app.session.revision,
            h.app.ui.show_timeline,
            h.app.preferences.timeline_left,
            h.app.preferences.timeline_collapsed
        )
    );
    enter(&mut h.app, true).unwrap();
    h.frames(3);
    assert!(crate::scenario::rect_of("render:shots_strip").is_none());
    assert!(crate::scenario::rect_of("render:shots:show").is_some());
    h.step(&json!({"click":{"handle":"render:shots:show"}})).unwrap();
    assert!(h.app.preferences.render_shots_strip);
    h.step(&json!({"click":{"handle":"render:shots:hide"}})).unwrap();
    assert!(!h.app.preferences.render_shots_strip);
    let saved = h.app.prefs();
    let mut next = Harness::new();
    next.app.load_prefs(&saved);
    assert!(!next.app.preferences.render_shots_strip);
    h.frames(2);
    h.app.start("view.shots_strip");
    assert!(h.app.preferences.render_shots_strip);
    h.frames(3);
    assert!(crate::scenario::rect_of("render:shots_strip").is_some());
    assert_eq!(h.app.session.doc, document);
    assert_eq!(h.app.session.undo.len(), state.0);
}

#[test]
fn render_workspace_canvas_controls_and_fit_respect_panels_and_shots_in_both_layouts() {
    for layout in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut h = Harness::new();
        h.app.ui.layout = layout;
        h.call("ui.resize", json!({"width":1600,"height":1000}));
        h.app.run("solid.box", json!({"length":60,"width":40,"height":30})).unwrap();
        enter(&mut h.app, true).unwrap();
        h.frames(4);
        h.app.cam.fov = 0.9;
        h.app.start("render.camera.fit");
        h.frames(4);
        assert_eq!(h.app.cam.fov, 0.9);
        let free = crate::shell::content_rect(&h.app, h.app.viewport.rect.unwrap());
        let layout = h.app.viewport.layout.as_ref().unwrap();
        assert!(free.contains_rect(layout.navigation), "{free:?} {layout:?}");
        assert!((layout.navigation.center().x - free.center().x).abs() < 1.0);
        assert_product_fits_canvas(&h.app);
    }
}

#[test]
fn render_workspace_old_personal_pins_migrate_once_to_task_tabs() {
    let mut custom = crate::toolbar_custom::ToolbarCustom::default();
    custom.load(&json!({"version":2,"panels":{
        "DESIGN/RENDER/SCENE":["render.settings.get","render.preset.resolve","solid.box"],
        "DESIGN/RENDER/LIGHTING":["render.library.studio.apply"],
        "DESIGN/RENDER/CAMERA":["render.settings.apply","render.camera.fit"],
        "DESIGN/RENDER/IMAGE":["render.export","render.start"],
        "DESIGN/RENDER/OUTPUT":["render.preflight"]
    }}));
    assert_eq!(custom.panels["DESIGN/RENDER CAMERA/SHOTS"], ["render.settings.get", "solid.box"]);
    assert_eq!(custom.panels["DESIGN/RENDER SCENE/STUDIO"], ["render.preset.resolve", "render.library.studio.apply"]);
    assert_eq!(custom.panels["DESIGN/RENDER CAMERA/VIEW"], ["render.settings.apply"]);
    assert_eq!(custom.panels["DESIGN/RENDER/RESULT"], ["render.export"]);
    assert_eq!(custom.panels["DESIGN/RENDER/RENDER"], ["render.start", "render.preflight"]);
    let saved = custom.prefs();
    custom.load(&saved);
    assert_eq!(custom.prefs(), saved);
}

#[test]
fn render_workspace_scene_vocabulary_migrates_previous_shell_pins_once() {
    let mut custom = crate::toolbar_custom::ToolbarCustom::default();
    custom.load(&json!({"version":2,"panels":{
        "DESIGN/RENDER LIGHTING/LIGHTING":["render.library.studio.apply","render.lights.solo"],
        "DESIGN/RENDER LIGHTING/ENVIRONMENT":["render.library.environment.apply","render.hdr.inspect"],
        "DESIGN/RENDER/OUTPUT":["render.preflight","render.quality.prepare"],
        "DESIGN/RENDER/RESULTS":["render.export","render.effects.describe"]
    }}));
    assert_eq!(custom.panels["DESIGN/RENDER SCENE/STUDIO"], ["render.library.studio.apply", "render.library.environment.apply"]);
    assert_eq!(custom.panels["DESIGN/RENDER SCENE/LIGHTS"], ["render.lights.solo"]);
    assert_eq!(custom.panels["DESIGN/RENDER SCENE/STAGE"], ["render.hdr.inspect"]);
    assert_eq!(custom.panels["DESIGN/RENDER/IMAGE"], ["render.quality.prepare", "render.effects.describe"]);
    assert_eq!(custom.panels["DESIGN/RENDER/RENDER"], ["render.preflight"]);
    assert_eq!(custom.panels["DESIGN/RENDER/RESULT"], ["render.export"]);
    let saved = custom.prefs();
    custom.load(&saved);
    assert_eq!(custom.prefs(), saved);
}

fn assert_product_fits_canvas(app: &SolveApp) {
    let layout = app.viewport.layout.as_ref().unwrap();
    let free = crate::shell::content_rect(app, layout.viewport);
    assert!(free.contains_rect(layout.drawable), "fit region {:?} outside canvas {free:?}", layout.drawable);
    let bounds = solidcraft_engine::product_scene::extract(&app.session).unwrap().bounds;
    for x in [bounds.min.x, bounds.max.x] {
        for y in [bounds.min.y, bounds.max.y] {
            for z in [bounds.min.z, bounds.max.z] {
                let (sx, sy, _) = app
                    .cam
                    .to_screen(
                        solidcore_record::Vec3::new(x, y, z),
                        f64::from(layout.viewport.width()),
                        f64::from(layout.viewport.height()),
                        bounds.diagonal().max(10.0),
                    )
                    .unwrap();
                let at = layout.viewport.min + egui::vec2(sx as f32, sy as f32);
                assert!(layout.drawable.contains(at), "product corner {at:?} outside {:?}", layout.drawable);
            }
        }
    }
}

#[test]
fn render_workspace_hosts_float_and_shield_input_in_both_ribbon_layouts() {
    for (layout, page) in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked]
        .into_iter()
        .flat_map(|layout| [inspector::Editor::Scene, inspector::Editor::Appearance].map(|page| (layout, page)))
    {
        let mut h = Harness::new();
        h.app.ui.layout = layout;
        h.call("ui.resize", json!({"width":1600,"height":1000}));
        h.app.run("solid.box", json!({"length":30,"width":20,"height":10})).unwrap();
        h.frames(4);

        enter(&mut h.app, true).unwrap();
        inspector::select_editor(&mut h.app, page);
        h.frames(4);
        let viewport = h.app.viewport.rect.unwrap();
        let history = crate::timeline::floating_rect(viewport);
        let shots = crate::scenario::rect_of("render:shots_strip").unwrap();
        assert!((shots.height() - 48.0).abs() <= 1.0, "{layout:?}: {shots:?}");
        assert!(
            (shots.left() - history.left()).abs() < 1.0
                && (shots.right() - history.right()).abs() < 1.0
                && (shots.bottom() - history.bottom()).abs() < 1.0,
            "Shots must reuse History's floating placement: {shots:?} {history:?}"
        );
        for key in ["render:inspector", "render:browser", "render:appearances", "render:shots_strip"] {
            let (card, _) = h.app.shell.workspace_panels.get(key).unwrap();
            assert!(viewport.contains_rect(*card), "{layout:?} {key}: {card:?} outside {viewport:?}");
            assert!(crate::shell::covered(&h.app, card.center()), "{key} must shield the viewport");
        }
        let inspector = h.app.shell.workspace_panels["render:inspector"].0;
        let browser = h.app.shell.workspace_panels["render:browser"].0;
        let appearances = h.app.shell.workspace_panels["render:appearances"].0;
        let ribbon_bottom = h.app.shell.cards.iter().map(|r| r.bottom()).fold(0.0, f32::max);
        assert!((browser.top() - inspector.top()).abs() < 1.0);
        assert!((appearances.bottom() - inspector.bottom()).abs() < 1.0);
        assert!(browser.height() <= 330.0, "Browser must stay within the fixed tree height: {browser:?}");
        for key in ["render:browser", "render:inspector"] {
            let card = h.app.shell.workspace_panels[key].0;
            assert!(
                (card.top() - ribbon_bottom - Tokens::get().space_8).abs() < 1.0,
                "{layout:?} {key}: gap {} must be 8 pt",
                card.top() - ribbon_bottom
            );
        }
        for key in ["render:browser", "render:appearances", "render:inspector"] {
            let card = h.app.shell.workspace_panels[key].0;
            let header = crate::scenario::rect_of(&format!("{key}:header")).unwrap();
            assert!((header.height() - crate::ui_kit::header_height()).abs() < 0.1, "{key}: {header:?}");
            assert!((header.top() - card.top()).abs() <= 1.0, "header must have no outer content padding");
            let control = crate::scenario::rect_of(if key == "render:browser" {
                "render:tree:by-look"
            } else if key == "render:appearances" {
                "render:library-search"
            } else if page == inspector::Editor::Appearance {
                "dialog:editor_footer"
            } else {
                "render:scene:look"
            })
            .unwrap();
            if key == "render:browser" {
                assert!(header.contains_rect(control), "Scene lens belongs in the title band");
            } else {
                assert!(control.top() >= header.bottom(), "editor controls must be below the standard header: {control:?} {header:?}");
            }
        }

        assert!((browser.width() - appearances.width()).abs() < 1.0);
        assert!((appearances.top() - browser.bottom() - Tokens::get().space_8).abs() < 1.0);
        assert!(appearances.bottom() < shots.top());
        assert!(browser.left() > viewport.left() && inspector.right() < viewport.right());
        assert!(inspector.bottom() < shots.top() && browser.bottom() < shots.top());
        let camera = h.app.cam;
        h.step(
            &json!({"drag":[[inspector.center().x,inspector.center().y],[inspector.center().x-30.0,inspector.center().y+20.0]],"button":"middle"}),
        )
        .unwrap();
        assert_eq!(h.app.cam, camera, "middle dragging over the card must not reach the model");
        h.step(&json!({"click":{"handle":"render:shots:hide"}})).unwrap();
        h.frames(3);
        assert_eq!(h.app.viewport.rect, Some(viewport), "floating strips must preserve the viewport projection");
        let pill = h.app.shell.workspace_panels["render:shots:show"].0;
        assert!(crate::shell::covered(&h.app, pill.center()));
        enter(&mut h.app, false).unwrap();
        h.frames(3);
        assert!(h.app.shell.workspace_panels.is_empty(), "Render card reservations must not leak into Design");
    }
}

#[test]
fn render_workspace_assignment_footer_fits_stacked_card_with_shots_shown_and_hidden() {
    for layout in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut h = Harness::new();
        h.app.ui.layout = layout;
        h.call("ui.resize", json!({"width":1600,"height":1000}));
        enter(&mut h.app, true).unwrap();
        for shown in [true, false] {
            h.app.run("view.shots_strip", json!({"visible":shown})).unwrap();
            h.frames(4);
            let browser = h.app.shell.workspace_panels["render:browser"].0;
            let appearances = h.app.shell.workspace_panels["render:appearances"].0;
            // Assignment help is the shared footer's tooltip; its visible anchor stays in the card.
            let footer = crate::scenario::rect_of("catalog:footer").unwrap();
            assert!(appearances.contains_rect(footer), "{layout:?}, Shots {shown}: footer {footer:?} outside {appearances:?}");
            let gap = Tokens::get().space_8;
            assert!((appearances.top() - browser.bottom() - gap).abs() < 1.0);
            let bounds = crate::shell::panel_bounds(&h.app, h.app.viewport.rect.unwrap());
            assert!((bounds.bottom() - appearances.bottom() - gap).abs() < 1.0, "{layout:?}, Shots {shown}: {appearances:?}, {bounds:?}");
        }
    }
}

#[test]
fn render_workspace_card_grips_swap_sides_and_resize_without_design_edits() {
    for layout in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut h = Harness::new();
        h.app.ui.layout = layout;
        h.call("ui.resize", json!({"width":1600,"height":1000}));
        enter(&mut h.app, true).unwrap();
        h.frames(4);
        let document = h.app.session.doc.clone();
        let history = (h.app.session.undo.len(), h.app.session.redo.len());
        let camera = h.app.cam;
        h.step(&json!({"drag":[{"handle":"render:inspector:grip"},[100.0,350.0]],"steps":8})).unwrap();
        h.frames(3);
        assert_eq!(h.app.preferences.render_inspector_side, "left");
        assert_eq!(h.app.preferences.render_browser_side, "right");
        assert!(h.app.shell.workspace_panels["render:inspector"].0.right() < h.app.shell.workspace_panels["render:browser"].0.left());
        let width = h.app.preferences.render_inspector_width;
        let card_width = h.app.shell.workspace_panels["render:inspector"].0.width();
        let rail = crate::scenario::handle_at("render:inspector:resize").unwrap();
        h.step(&json!({"drag":[[rail.x,rail.y],[rail.x+50.0,rail.y]],"steps":8})).unwrap();
        h.frames(3);
        // A compact dock keeps its minimum width and leaves the remembered width alone.
        if layout == crate::shell::LayoutMode::Docked {
            assert_eq!(h.app.preferences.render_inspector_width, width);
            assert_eq!(h.app.shell.workspace_panels["render:inspector"].0.width(), card_width);
        } else {
            assert!(h.app.preferences.render_inspector_width > width);
            assert!(h.app.shell.workspace_panels["render:inspector"].0.width() > card_width);
        }
        h.step(&json!({"click":{"handle":"render:tree:by-look"}})).unwrap();
        h.frames(3);
        h.step(&json!({"drag":[{"handle":"render:browser:grip"},[100.0,350.0]],"steps":8})).unwrap();
        h.frames(3);
        assert_eq!(h.app.preferences.render_browser_side, "left");
        assert_eq!(h.app.preferences.render_inspector_side, "right");
        assert!(h.app.shell.workspace_panels["render:browser"].0.right() < h.app.shell.workspace_panels["render:inspector"].0.left());
        assert_eq!(h.app.cam, camera, "card gestures must not move the camera");
        assert_eq!(h.app.session.doc, document);
        assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
    }
}

/// A small public-safe model, built through the sample recipe inside an occurrence.
fn render_pick_sample() -> (Harness, u64) {
    let mut h = Harness::new();
    let part = h.app.run("component.create", json!({"name":"Sample","grounded":false})).unwrap();
    let occurrence = part["occurrence"].as_u64().unwrap();
    h.app.session.run_script(&solidcraft_engine::sample::script()).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    enter(&mut h.app, true).unwrap();
    h.frames(4);
    h.app.start("render.camera.fit");
    h.settle();
    (h, occurrence)
}

fn product_pick_point(h: &Harness) -> egui::Pos2 {
    let rect = h.app.viewport.layout.as_ref().unwrap().drawable;
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    for y in 2..18 {
        for x in 2..18 {
            let at = rect.min + egui::vec2(rect.width() * x as f32 / 20.0, rect.height() * y as f32 / 20.0);
            if !crate::shell::covered(&h.app, at)
                && crate::viewport::pick(&h.app, &proj, at).iter().any(|h| matches!(h, crate::viewport::Hit::Face { .. }))
            {
                return at;
            }
        }
    }
    panic!("sample must offer a visible product face outside the floating panels");
}

#[test]
fn render_workspace_pick_stage_tools_filter_sample_clicks_with_shots_shown_and_hidden() {
    use solidcraft_engine::Sel;
    let (mut h, occurrence) = render_pick_sample();
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    for shots in [true, false] {
        h.app.run("view.shots_strip", json!({"visible":shots})).unwrap();
        h.frames(3);
        let tools = crate::scenario::rect_of("render:stage_tools").unwrap();
        assert!(tools.contains_rect(crate::scenario::rect_of("render:pick").unwrap()));
        assert!(tools.contains_rect(crate::scenario::rect_of("render:preview_note").unwrap()));
        assert!(crate::scenario::rect_of("render:hud").is_none());
        for mode in ["body", "face", "component"] {
            assert!(crate::scenario::handle_at(&format!("render:pick:{mode}")).is_some(), "Pick must be enabled");
            h.step(&json!({"click":{"handle":format!("render:pick:{mode}")}})).unwrap();
            h.frames(2);
            let at = product_pick_point(&h);
            h.step(&json!({"move":[at.x,at.y]})).unwrap();
            assert!(matches!(h.app.viewport.hover, Some(crate::viewport::Hit::Face { .. })));
            h.step(&json!({"click":[at.x,at.y]})).unwrap();
            match h.app.session.selection.as_slice() {
                [Sel::Body { name }] if mode == "body" => assert_eq!(name, "Plate"),
                [Sel::Face { body, .. }] if mode == "face" => assert_eq!(body, "Plate"),
                [Sel::Occurrence { path }] if mode == "component" => assert_eq!(path, &[occurrence]),
                other => panic!("{mode}, Shots {shots}: wrong selection {other:?}"),
            }
            let selected = h.app.session.selection.clone();
            enter(&mut h.app, false).unwrap();
            h.frames(3);
            assert_eq!(h.app.session.selection, selected);
            enter(&mut h.app, true).unwrap();
            h.settle();
            assert_eq!(h.app.session.selection, selected);
        }
    }
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(h.app.tool.is_none() && h.app.dialog.is_none());
}

/// A mode Render's filter has no button for picks there as the filter shows it: Features as Body,
/// Sketch geometry as Face, Components as Component. Leaving Render, the mode is what it was.
#[test]
fn render_picks_as_its_filter_shows_and_keeps_the_mode() {
    use solidcraft_engine::{PickMode, Sel};
    let (mut h, occurrence) = render_pick_sample();
    for (mode, filter) in [(PickMode::Features, "body"), (PickMode::Sketch, "face"), (PickMode::Components, "component")] {
        enter(&mut h.app, false).unwrap();
        h.frames(2);
        crate::selection_mode::set(&mut h.app, mode);
        enter(&mut h.app, true).unwrap();
        h.settle();
        assert_eq!(crate::selection_mode::render_filter(h.app.ui.pick_mode), filter);
        let at = product_pick_point(&h);
        h.step(&json!({"move":[at.x,at.y]})).unwrap();
        h.step(&json!({"click":[at.x,at.y]})).unwrap();
        match (filter, h.app.session.selection.as_slice()) {
            ("body", [Sel::Body { name }]) => assert_eq!(name, "Plate"),
            ("face", [Sel::Face { body, .. }]) => assert_eq!(body, "Plate"),
            ("component", [Sel::Occurrence { path }]) => assert_eq!(path, &[occurrence]),
            (_, other) => panic!("{mode:?}: the filter shows {filter}, a click picked {other:?}"),
        }
        enter(&mut h.app, false).unwrap();
        h.frames(2);
        assert_eq!(h.app.ui.pick_mode, mode, "Render leaves the setting as it was");
        enter(&mut h.app, true).unwrap();
        h.settle();
    }
}

#[test]
fn modelling_select_panel_shows_and_clears_renders_component_pick() {
    use egui::accesskit::{Role, Toggled};
    const BODY: &str = "Select bodies (a face picks its body)";
    const COMPONENT: &str = "Select components (a face picks its component)";
    let (mut h, _) = render_pick_sample();
    h.step(&json!({"click":{"handle":"render:pick:component"}})).unwrap();
    enter(&mut h.app, false).unwrap();
    h.settle();
    h.step(&json!({"click":{"handle":"tab:VIEW"}})).unwrap();
    h.step(&json!({"click":{"handle":"panel:VIEW:SELECT"}})).unwrap();

    let check = |h: &Harness, body: bool, component: bool| {
        assert_eq!(
            (h.app.ui.pick_mode == solidcraft_engine::PickMode::Bodies, h.app.ui.pick_mode == solidcraft_engine::PickMode::Parts),
            (body, component)
        );
        let tree = h.output.accesskit_update.as_ref().unwrap();
        for (label, on) in [(BODY, body), (COMPONENT, component)] {
            let node = tree.nodes.iter().find(|(_, n)| n.role() == Role::CheckBox && n.label() == Some(label));
            assert_eq!(node.and_then(|(_, n)| n.toggled()), Some(if on { Toggled::True } else { Toggled::False }), "{label}");
        }
    };
    check(&h, false, true);
    h.step(&json!({"widget":"Faces"})).unwrap();
    for (label, body, component) in [(COMPONENT, false, false), (BODY, true, false), (COMPONENT, false, true), (BODY, true, false)] {
        h.step(&json!({"widget":label})).unwrap();
        check(&h, body, component);
        assert!(h.app.ui.pick_off.iter().any(|kind| kind == "faces"), "changing pick mode must preserve the panel's Faces filter");
    }
}

#[test]
fn render_workspace_click_modifiers_clear_and_browser_share_selection() {
    use solidcraft_engine::Sel;
    let mut h = Harness::new();
    for (name, x) in [("Left", -30), ("Right", 30)] {
        h.app.run("solid.box", json!({"corner":[x,0,0],"length":20,"width":20,"height":20,"body_name":name})).unwrap();
    }
    enter(&mut h.app, true).unwrap();
    h.settle();
    h.app.run("select.filter", json!({"mode":"body"})).unwrap();
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let left = proj.to_screen(solidcore_record::Vec3::new(-20.0, 10.0, 20.0)).unwrap();
    let right = proj.to_screen(solidcore_record::Vec3::new(40.0, 10.0, 20.0)).unwrap();
    h.step(&json!({"click":[left.x,left.y]})).unwrap();
    assert_eq!(h.app.session.selection, [Sel::Body { name: "Left".into() }]);
    h.step(&json!({"click":[right.x,right.y],"shift":true})).unwrap();
    assert_eq!(h.app.session.selection.len(), 2);
    h.step(&json!({"click":[left.x,left.y],"ctrl":true})).unwrap();
    assert_eq!(h.app.session.selection, [Sel::Body { name: "Right".into() }]);
    h.app.preferences.selection_shift_add_only = true;
    h.step(&json!({"click":[right.x,right.y],"shift":true})).unwrap();
    assert_eq!(h.app.session.selection, [Sel::Body { name: "Right".into() }]);
    // A Browser body row writes the same selection and immediately feeds the product overlay.
    h.step(&json!({"click":{"handle":"row:Left"}})).unwrap();
    assert_eq!(h.app.session.selection, [Sel::Body { name: "Left".into() }]);
    assert!(!crate::viewport::build_highlight(&h.app).merged().tris.is_empty());
    let empty = h.app.viewport.layout.as_ref().unwrap().drawable.left_bottom() + egui::vec2(10.0, -10.0);
    h.step(&json!({"click":[empty.x,empty.y]})).unwrap();
    assert!(h.app.session.selection.is_empty());
    assert!(h.app.viewport.hover.is_none());
}

#[test]
fn render_workspace_selection_overlay_never_changes_started_frames_png_or_retained_results() {
    use solidcraft_engine::Sel;
    let (mut h, _) = render_pick_sample();
    let at = product_pick_point(&h);
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let hit = crate::viewport::pick(&h.app, &proj, at).into_iter().next().unwrap();
    let face = crate::viewport::hit_sel(&hit).unwrap();
    let first = completed_preview(&mut h, [230, 230, 230]);
    let clean = h.app.session.render_job_handle(first).unwrap().frame().unwrap().unwrap();
    let clean_scene = gpu_scene(&solidcraft_engine::product_scene::extract(&h.app.session).unwrap()).unwrap();
    h.app.run("select.set", json!({"items":[face]})).unwrap();
    let face_overlay = crate::viewport::build_highlight(&h.app).merged();
    assert!(!face_overlay.tris.is_empty());
    h.app.run("select.set", json!({"items":[Sel::Body { name:"Plate".into() }]})).unwrap();
    let body_overlay = crate::viewport::build_highlight(&h.app).merged();
    assert!(body_overlay.tris.len() > face_overlay.tris.len(), "Face must only tint the selected face");
    h.app.viewport.hover = Some(hit);
    let second = completed_preview(&mut h, [230, 230, 230]);
    let selected = h.app.session.render_job_handle(second).unwrap().frame().unwrap().unwrap();
    assert_eq!(clean.png(), selected.png(), "started image job must never capture selection or hover pixels");
    let after = gpu_scene(&solidcraft_engine::product_scene::extract(&h.app.session).unwrap()).unwrap();
    assert_eq!(clean_scene.0, after.0);
    assert_eq!(clean_scene.1.tris, after.1.tris);
    h.app.render_workspace.view.completed = Some(second);
    h.app.render_workspace.view.result = true;
    h.frames(3);
    h.app.run("select.filter", json!({"mode":"face"})).unwrap();
    h.step(&json!({"click":{"handle":"render:pick:body"}})).unwrap();
    assert_eq!(h.app.ui.pick_mode, solidcraft_engine::PickMode::Faces, "result canvas must disable the stage's Pick control");
    let path = h.dir.join("overlay-free.png");
    h.app.run("render.export", json!({"job":second,"path":path})).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), clean.png());
    assert_eq!(h.app.session.render_job_handle(second).unwrap().frame().unwrap().unwrap().png(), clean.png());
}

#[test]
fn render_workspace_pick_filter_rejects_invalid_requests_atomically() {
    let mut h = Harness::new();
    let doc = h.app.session.doc.clone();
    // The filter sets the one selection mode (#4255): a component is the innermost placement,
    // the Parts mode, as Render picked before; and the mode reads back as the same filter.
    for (mode, pick) in [
        ("body", solidcraft_engine::PickMode::Bodies),
        ("face", solidcraft_engine::PickMode::Faces),
        ("component", solidcraft_engine::PickMode::Parts),
    ] {
        h.app.ui.pick_off = vec!["faces".into()];
        assert_eq!(h.app.run("select.filter", json!({"mode":mode})).unwrap()["mode"], mode);
        assert_eq!(h.app.ui.pick_mode, pick);
        assert!(h.app.ui.pick_off.is_empty(), "every filter picks through faces");
        assert_eq!(crate::selection_mode::render_filter(h.app.ui.pick_mode), mode);
    }
    for request in [json!({}), json!({"mode":"edge"}), json!({"mode":"body","extra":true})] {
        assert!(h.app.run("select.filter", request).is_err());
        assert_eq!(h.app.ui.pick_mode, solidcraft_engine::PickMode::Parts);
    }
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert!(h.app.session.undo.is_empty());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn render_workspace_product_pass_draws_only_requested_face_or_body_overlay() {
    let mut app = nav_app(true, "fusion", crate::prefs::OrbitAround::ViewCentre);
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(256.0, 256.0));
    app.viewport.rect = Some(rect);
    let proj = crate::viewport::projection(&app, rect);
    let hit = crate::viewport::pick(&app, &proj, rect.center()).into_iter().find(|h| matches!(h, crate::viewport::Hit::Face { .. })).unwrap();
    let face = crate::viewport::hit_sel(&hit).unwrap();
    let scene = solidcraft_engine::product_scene::extract(&app.session).unwrap();
    let gpu = gpu_scene(&scene).unwrap().1;
    let radius = scene.bounds.diagonal().max(10.0) + scene.bounds.center().dist(app.cam.target);
    let eye = app.cam.eye().to_f32();
    let camera = app.cam;
    let draw = |overlay| {
        crate::gpu::render_product_offscreen(
            gpu.clone(),
            overlay,
            camera.view_proj(1.0, radius).to_f32(),
            camera.back().to_f32(),
            [eye[0], eye[1], eye[2], 1.0],
            256,
            false,
        )
    };
    let Some(clean) = draw(None) else {
        eprintln!("skipping: no GPU adapter for product overlay pixels");
        return;
    };
    app.run("select.set", json!({"items":[face]})).unwrap();
    let face_pixels = draw(Some(crate::viewport::build_highlight(&app))).unwrap();
    app.run("select.set", json!({"items":[{"type":"body","name":"Product"}]})).unwrap();
    let body_pixels = draw(Some(crate::viewport::build_highlight(&app))).unwrap();
    let changed = |image: &[u8]| clean.as_chunks::<4>().0.iter().zip(image.as_chunks::<4>().0).filter(|(a, b)| a != b).count();
    assert!(changed(&face_pixels) > 100, "Face overlay must reach the product pass");
    assert!(changed(&body_pixels) > changed(&face_pixels) + 100, "Face must leave the other faces untouched");
}

#[test]
fn render_workspace_pick_projection_matches_orthographic_and_custom_perspective_camera() {
    use egui::{Event, Modifiers, PointerButton};
    use solidcore_record::Vec3;
    for fov in [0.0, 0.6, 1.1] {
        let mut app = nav_app(true, "fusion", crate::prefs::OrbitAround::ViewCentre);
        app.cam.fov = fov;
        let ctx = crate::fonts::test_context();
        let mut time = 0.0;
        nav_frame(&mut app, &ctx, true, &mut time, Modifiers::NONE, vec![]);
        let rect = app.viewport.rect.unwrap();
        let proj = crate::viewport::projection(&app, rect);
        assert_eq!(proj.cam.fov, fov);
        let (x, y, _) = app.cam.to_screen(Vec3::new(20.0, 15.0, 20.0), f64::from(rect.width()), f64::from(rect.height()), 100.0).unwrap();
        let pos = rect.min + egui::vec2(x as f32, y as f32);
        nav_frame(&mut app, &ctx, true, &mut time, Modifiers::NONE, vec![Event::PointerMoved(pos)]);
        assert!(matches!(app.viewport.hover, Some(crate::viewport::Hit::Face { .. })), "FOV {fov}");
        for pressed in [true, false] {
            nav_frame(
                &mut app,
                &ctx,
                true,
                &mut time,
                Modifiers::NONE,
                vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }],
            );
        }
        assert!(matches!(app.session.selection.as_slice(), [solidcraft_engine::Sel::Face { body, .. }] if body == "Product"), "FOV {fov}");
    }
}

#[test]
fn render_workspace_component_filter_keeps_nested_repeated_occurrences_distinct() {
    use crate::viewport::{Hit, candidate};
    use solidcore_record::Vec3;
    use solidcraft_engine::Sel;
    let mut h = Harness::new();
    let parent = h.app.run("component.create", json!({"name":"Parent"})).unwrap();
    let child = h.app.run("component.create", json!({"name":"Part"})).unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Solid"})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    let second = h.app.run("occurrence.copy", json!({"occurrence":parent["occurrence"],"translate":[50,0,0]})).unwrap();
    enter(&mut h.app, true).unwrap();
    h.app.run("select.filter", json!({"mode":"component"})).unwrap();
    for top in [parent["occurrence"].as_u64().unwrap(), second["occurrence"].as_u64().unwrap()] {
        let path = vec![top, child["occurrence"].as_u64().unwrap()];
        let names = h.app.session.occurrence_bodies(&path, true).unwrap();
        assert_eq!(names.len(), 1);
        let hit = Hit::Face { body: names[0].clone(), index: 0, point: Vec3::ZERO };
        assert_eq!(candidate(&h.app, &[hit]).unwrap().1, Sel::Occurrence { path });
    }
}

#[test]
fn navigation_bar_render_canvases_filter_slots_and_keep_product_fit() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5})).unwrap();
    enter(&mut h.app, true).unwrap();
    h.app.preferences.look_at_anim = false;
    h.settle();
    assert_eq!(crate::navigation_bar::canvas_caps(&h.app), crate::navigation::NavCaps::RENDER);
    // The product preview picks (#6426), so the slots that need a pick are on the bar there too.
    for slot in ["orbit", "pan", "zoom", "fit", "look_at", "home", "visual_style", "display"] {
        assert!(crate::scenario::handle_at(&format!("nav:slot:{slot}")).is_some(), "{slot}");
    }
    h.app.cam.pan(100.0, 50.0, 600.0);
    let panned = h.app.cam;
    h.step(&json!({"click":{"handle":"nav:slot:fit"}})).unwrap();
    assert_ne!(h.app.cam.target, panned.target, "Fit recentres the product");
    assert_product_fits_canvas(&h.app);
    let mut iso = h.app.cam;
    iso.set_view(solidcraft_engine::render::StandardView::Iso);
    h.app.cam.orbit(0.4, 0.2);
    h.step(&json!({"click":{"handle":"nav:slot:home"}})).unwrap();
    assert_eq!((h.app.cam.yaw, h.app.cam.pitch), (iso.yaw, iso.pitch), "Home returns to the isometric view");
    assert_product_fits_canvas(&h.app);

    let job = completed_preview(&mut h, [20, 40, 60]);
    h.app.render_workspace.view.job = Some(job);
    h.app.render_workspace.view.result = true;
    h.settle();
    assert_eq!(crate::navigation_bar::canvas_caps(&h.app), crate::navigation::NavCaps { orbit: false, pick: false });
    for slot in ["orbit", "look_at", "visual_style"] {
        assert!(crate::scenario::handle_at(&format!("nav:slot:{slot}")).is_none(), "{slot}");
    }
    for slot in ["pan", "zoom", "fit", "home", "display"] {
        assert!(crate::scenario::handle_at(&format!("nav:slot:{slot}")).is_some(), "{slot}");
    }
}

#[test]
fn render_lights_browser_sections_and_lights_describe_current_job_entries() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1440,"height":1100}));
    enter(&mut h.app, true).unwrap();
    h.frames(3);
    let rows: Vec<_> = crate::browser::drawn_rows().into_iter().map(|r| r.0).collect();
    for section in ["Environment", "Lights", "Shots", "Objects"] {
        assert!(rows.iter().any(|label| label == section), "{section}: {rows:?}");
    }
    assert_eq!(scene::light_types(&h.app), vec!["render.tree_directional"; 3]);
    assert_eq!(scene::environment(&h.app).0, "Fixed studio");
    h.app.render_workspace.view.method = ImageMethod::CpuRaytrace;
    h.app.render_workspace.view.raytrace = serde_json::from_value(json!({
        "lights":[{"direction":[0,0,-1],"color":[1,1,1],"intensity":1}],
        "punctual_lights":[
            {"position_mm":[0,0,10],"color":[1,1,1],"intensity":1,"reference_distance_mm":10,"minimum_distance_mm":0.1},
            {"position_mm":[0,0,10],"color":[1,1,1],"intensity":1,"reference_distance_mm":10,"minimum_distance_mm":0.1,
             "cone":{"emitted_axis":[0,0,-1],"inner_half_angle_radians":0.1,"outer_half_angle_radians":0.2}}
        ],
        "area_lights":[{"center_mm":[0,0,10],"edge_u_mm":[1,0,0],"edge_v_mm":[0,-1,0],"color":[1,1,1],"intensity":1}]
    }))
    .unwrap();
    assert_eq!(scene::light_types(&h.app), ["render.tree_directional", "render.tree_point", "render.tree_spot", "render.tree_rectangle"]);
    h.app.render_workspace.view.raytrace.lights = Some(vec![]);
    h.app.render_workspace.view.raytrace.punctual_lights = None;
    h.app.render_workspace.view.raytrace.area_lights = None;
    assert!(scene::light_types(&h.app).is_empty(), "empty explicit rig must stay empty");
    h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
    let studio = &mut h.app.render_workspace.view.studio;
    studio.sun_enabled = true;
    assert_eq!(scene::light_types(&h.app), vec!["render.tree_directional"; 3], "implicit studio sources are listed");
    h.app.render_workspace.view.studio.lighting_enabled = true;
    h.app
        .render_workspace
        .view
        .studio
        .area_lights
        .push(serde_json::from_value(json!({"center":[0,0,10],"u":[1,0,0],"v":[0,-1,0],"radiance":[1,1,1]})).unwrap());
    assert_eq!(scene::light_types(&h.app), ["render.tree_sun", "render.tree_rectangle"]);
    h.app.render_workspace.view.studio.environment = studio::Environment::Sky;
    h.app.render_workspace.view.studio.rotation_rad = std::f64::consts::FRAC_PI_2;
    h.app.render_workspace.view.studio.intensity = 2.5;
    assert_eq!(scene::environment(&h.app), ("Sky".into(), "90° · 2.50×".into()));
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:tree:environment:current"}})).unwrap();
    assert_eq!(inspector::selected_editor(&h.app), inspector::Editor::Environment);
    h.step(&json!({"click":{"browser":"Key"}})).unwrap();
    assert_eq!(inspector::selected_editor(&h.app), inspector::Editor::Light);
}

#[test]
fn render_workspace_browser_look_lens_counts_distinct_objects_and_selects_them() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1440,"height":1400}));
    for (name, x) in [("A", 0), ("B", 20), ("C", 40)] {
        h.app.run("solid.box", json!({"corner":[x,0,0],"length":10,"width":10,"height":10,"body_name":name})).unwrap();
    }
    h.app.run("appearance.assign", json!({"bodies":["A","B"],"appearance":"ABS - Red"})).unwrap();
    h.app.run("appearance.assign", json!({"bodies":["C"],"appearance":"ABS - Blue"})).unwrap();
    // Two faces on C use the same look as A and B: still only one additional user.
    h.app.run("appearance.assign", json!({"body":"C","faces":[[40,5,5],[45,0,5]],"appearance":"ABS - Red"})).unwrap();
    enter(&mut h.app, true).unwrap();
    let groups = scene::groups(&h.app);
    let red = groups.iter().position(|g| g.look.as_ref().is_some_and(|l| l.name == "ABS - Red")).unwrap();
    assert_eq!(groups[red].objects, ["A", "B", "C"]);
    assert_eq!(groups.len(), 2);
    h.app.render_workspace.view.by_look = true;
    // Keep the object rows inside the ten-row Browser viewport while testing their hover.
    crate::browser::toggle(&mut h.app, "render:tree:lights", true);
    h.frames(3);
    let at = crate::scenario::handle_at("row:A").unwrap();
    h.step(&json!({"move":[at.x,at.y]})).unwrap();
    h.frames(2);
    assert_eq!(h.app.viewport.hover_bodies, ["A"], "By look rows retain Browser hover outlines");
    h.step(&json!({"move":[1000.0,800.0]})).unwrap();
    h.frames(2);
    assert!(h.app.viewport.hover_bodies.is_empty(), "leaving the row clears its hover outline");
    h.step(&json!({"click":{"handle":format!("render:tree:look:{red}")}})).unwrap();
    assert_eq!(h.app.session.selection.len(), 3);
    for name in ["A", "B", "C"] {
        assert!(h.app.session.selection.contains(&solidcraft_engine::Sel::Body { name: name.into() }));
    }
    h.step(&json!({"click":{"handle":"eye:A"}})).unwrap();
    assert!(h.app.session.visibility.hidden_bodies.contains(&"A".into()));
    let menu = crate::context_menu::items(&h.app, &crate::context_menu::Target::Body { name: "A".into() });
    for id in ["select.set", "select.same_look", "ui.isolate", "ui.show"] {
        assert!(menu.iter().any(|i| i.id == id));
    }
}

#[test]
fn render_workspace_dock_command_swaps_sides_without_design_edits() {
    for layout in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut h = Harness::new();
        h.app.ui.layout = layout;
        h.call("ui.resize", json!({"width":1600,"height":1000}));
        enter(&mut h.app, true).unwrap();
        h.frames(4);
        let document = h.app.session.doc.clone();
        let history = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
        let camera = h.app.cam;
        let product_camera = h.app.render_workspace.view.draft.camera;
        let widths = (h.app.preferences.render_browser_width, h.app.preferences.render_inspector_width);
        assert!(crate::shortcut_box::search(&h.app, "view.render_dock").iter().any(|row| row.0 == "view.render_dock"));
        for (panel, side, inspector, browser) in [
            ("inspector", "left", "left", "right"),
            ("browser", "left", "right", "left"),
            ("inspector", "right", "right", "left"),
            ("browser", "right", "left", "right"),
        ] {
            h.app.run("view.render_dock", json!({"panel":panel,"side":side})).unwrap();
            h.frames(3);
            assert_eq!(h.app.preferences.render_inspector_side, inspector);
            assert_eq!(h.app.preferences.render_browser_side, browser);
            let a = h.app.shell.workspace_panels["render:inspector"].0;
            let b = h.app.shell.workspace_panels["render:browser"].0;
            assert_eq!(a.center().x < b.center().x, inspector == "left");
            let view = h.app.viewport.layout.as_ref().unwrap();
            assert!(view.content.contains_rect(view.axis_view));
            let edge = if inspector == "left" { view.axis_view.left() - view.content.left() } else { view.content.right() - view.axis_view.right() };
            assert!(edge.abs() < 0.1);
            assert!(!view.navigation.intersects(a));
            assert!(!view.navigation.intersects(b));
            assert_eq!(h.app.cam, camera);
            assert_eq!(h.app.render_workspace.view.draft.camera, product_camera);
            assert_eq!(h.app.session.doc, document);
            assert_eq!((h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision), history);
            assert_eq!((h.app.preferences.render_browser_width, h.app.preferences.render_inspector_width), widths);
        }
    }
}

#[test]
fn render_workspace_dock_grip_menus_dispatch_the_other_side() {
    for layout in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut h = Harness::new();
        h.app.ui.layout = layout;
        h.call("ui.resize", json!({"width":1600,"height":1000}));
        enter(&mut h.app, true).unwrap();
        h.frames(4);
        let document = h.app.session.doc.clone();
        let history = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
        let camera = h.app.cam;
        for panel in ["inspector", "inspector", "browser", "browser"] {
            let before = h.app.preferences.render_inspector_side.clone();
            let at = crate::scenario::handle_at(&format!("render:{panel}:grip")).unwrap();
            for pressed in [true, false] {
                h.frame_with(vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE },
                ]);
            }
            h.frames(2);
            // Opening the menu must not move either panel.
            assert_eq!(h.app.preferences.render_inspector_side, before);
            h.step(&json!({"click":{"handle":format!("render:{panel}:move_side")}})).unwrap();
            h.frames(3);
            assert_ne!(h.app.preferences.render_inspector_side, before);
            assert_ne!(h.app.preferences.render_inspector_side, h.app.preferences.render_browser_side);
            assert_eq!(h.app.cam, camera);
            assert_eq!(h.app.session.doc, document);
            assert_eq!((h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision), history);
        }
    }
}

#[test]
fn render_workspace_browser_shot_switch_preserves_saved_edits_without_a_draft_prompt() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1440,"height":1100}));
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.name = "A".into();
    setups::save(&mut h.app).unwrap();
    h.app.run("render.settings.set", json!({"name":"B","set":{"width":321,"job":{"method":"raster"}}})).unwrap();
    let before = setups::draft(&h.app).unwrap();
    h.app.render_workspace.view.draft.width = 123;
    setups::persist_editor(&mut h.app, &before).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:tree:shot:B"}})).unwrap();
    assert!(!inspector::confirmation_open(&h.app));
    assert_eq!(h.app.render_workspace.view.name, "B");
    assert_eq!(h.app.render_workspace.view.draft.width, 321);
    assert_eq!(h.app.session.doc.render.setups["A"].width, 123);
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Shot("B".into()));
    assert!(!h.app.render_workspace.view.dirty);
}

#[test]
fn render_lights_browser_focus_covers_each_kind_without_design_edits() {
    use solidcraft_engine::Sel;
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1440,"height":1400}));
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Object"})).unwrap();
    h.app.run("render.settings.set", json!({"name":"Shot","set":{"job":{"method":"raster"}}})).unwrap();
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
    h.app.render_workspace.view.studio.lighting_enabled = true;
    h.app.render_workspace.view.studio.sun_enabled = true;
    h.frames(3);
    let doc = h.app.session.doc.clone();
    let before = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.is_dirty());
    for label in ["Environment", "Lights", "Shots", "Objects", "Output"] {
        assert!(crate::browser::drawn_rows().iter().any(|r| r.0 == label), "{label}");
    }
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Nothing);
    h.step(&json!({"click":{"handle":"row:Object"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Object(vec![Sel::Body { name: "Object".into() }]));
    h.step(&json!({"click":{"handle":"render:tree:environment"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Environment);
    assert_eq!(inspector::selected_editor(&h.app), inspector::Editor::Environment);
    assert!(crate::scenario::rect_of("dialog:section:Source").is_some());
    assert!(h.app.session.selection.is_empty());
    let focused_revision = h.app.session.revision;
    h.step(&json!({"click":{"handle":"render:tree:light:0"}})).unwrap();
    assert!(matches!(&h.app.render_workspace.view.selection, RenderSelection::Light(id) if id=="directional:0"));
    setups::load(&mut h.app);
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:tree:shot:Shot"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Shot("Shot".into()));
    // The browser is height-capped even in a tall window. Output is its last row,
    // and published targets can lie below the scroll clip as more rows are added.
    let browser = crate::scenario::rect_of("browser:card").unwrap();
    let header = crate::scenario::rect_of("render:browser:header").unwrap();
    h.call("ui.scroll", json!({"x":browser.center().x,"y":(header.bottom()+browser.bottom())/2.0,"delta":-2000}));
    h.frames(20);
    let browser = crate::scenario::rect_of("browser:card").unwrap();
    let header = crate::scenario::rect_of("render:browser:header").unwrap();
    let output = crate::scenario::handle_at("render:tree:output").unwrap();
    assert!(
        browser.shrink(Tokens::get().space_8).contains(output) && output.y > header.bottom(),
        "Output click must be inside the visible browser body"
    );
    h.step(&json!({"click":{"handle":"render:tree:output"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Output);
    h.app.run("view.render_select", json!({"kind":"nothing"})).unwrap();
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Nothing);
    assert_eq!(inspector::selected_editor(&h.app), inspector::Editor::Scene);
    assert_eq!(h.app.session.revision, focused_revision, "semantic selection only refreshes the view when clearing object picks");
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(before, (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.is_dirty()));
    assert!(h.app.dialog.is_none() && h.app.tool.is_none());
}

#[test]
fn render_lights_browser_eye_and_solo_are_view_state_and_editor_opening_is_read_only() {
    let mut h = render_lights_fixture();
    h.frames(3);
    assert!(crate::browser::drawn_rows().iter().any(|row| row.0 == "Key"));
    assert!(crate::browser::drawn_rows().iter().any(|row| row.0 == "Fill"));
    let document = h.app.session.doc.clone();
    let before = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision, h.app.session.is_dirty());
    h.step(&json!({"click":{"handle":"eye:Key"}})).unwrap();
    assert!(!h.app.render_workspace.view.lights.visibility.visible("directional:0"));
    h.step(&json!({"click":{"handle":"render:tree:light:1:solo"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.lights.visibility.solo.as_deref(), Some("directional:1"));
    h.step(&json!({"click":{"handle":"render:tree:light:1"}})).unwrap();
    h.frames(3);
    assert_eq!(inspector::selected_editor(&h.app), inspector::Editor::Light);
    assert!(crate::scenario::rect_of("render:light:editor").is_some());
    assert!(crate::scenario::handle_at("render:tree:shots:add").is_some());
    h.step(&json!({"click":{"handle":"render:tree:shots:add"}})).unwrap();
    assert!(inspector::confirmation_open(&h.app));
    assert_eq!(inspector::selected_editor(&h.app), inspector::Editor::Light);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(before, (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision, h.app.session.is_dirty()));
    assert!(h.app.render_workspace.view.job.is_none());
    assert!(h.app.dialog.is_none());
}

#[test]
fn render_workspace_browser_stage_expansion_never_locks_rows_or_changes_focus() {
    let mut h = Harness::new();
    enter(&mut h.app, true).unwrap();
    h.app.run("view.render_select", json!({"kind":"output"})).unwrap();
    let doc = h.app.session.doc.clone();
    let sections = ["objects", "lights", "environment", "shots"];
    for (stage, expanded) in [("look", vec!["objects"]), ("light", vec!["lights", "environment"]), ("frame", vec!["shots"]), ("output", vec![])] {
        for section in sections {
            h.app.tree.collapsed.insert(format!("render:tree:{section}"));
        }
        h.app.run("view.render_stage", json!({"stage":stage})).unwrap();
        for section in sections {
            assert_eq!(crate::browser::is_open(&h.app, &format!("render:tree:{section}"), true), expanded.contains(&section), "{stage}/{section}");
            crate::browser::toggle(&mut h.app, &format!("render:tree:{section}"), true);
            assert_eq!(crate::browser::is_open(&h.app, &format!("render:tree:{section}"), true), !expanded.contains(&section));
        }
        assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Output);
    }
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
}

#[test]
fn render_workspace_browser_semantic_focus_survives_clearing_object_picks() {
    use solidcraft_engine::Sel;
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Object"})).unwrap();
    enter(&mut h.app, true).unwrap();
    let doc = h.app.session.doc.clone();
    let evaluated = crate::open_meshing::session_world(&h.app.session);
    let before = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.is_dirty());
    for (request, expected) in [
        (json!({"kind":"environment"}), RenderSelection::Environment),
        (json!({"kind":"lighting"}), RenderSelection::Lighting),
        (json!({"kind":"light","id":"sun"}), RenderSelection::Light("sun".into())),
        (json!({"kind":"shot","id":"Shot"}), RenderSelection::Shot("Shot".into())),
        (json!({"kind":"output"}), RenderSelection::Output),
        (json!({"kind":"nothing"}), RenderSelection::Nothing),
    ] {
        h.app.run("select.set", json!({"items":[Sel::Body { name:"Object".into() }]})).unwrap();
        assert!(matches!(h.app.render_workspace.view.selection, RenderSelection::Object(_)));
        h.app.run("view.render_select", request.clone()).unwrap();
        h.frames(2);
        assert_eq!(h.app.render_workspace.view.selection, expected, "{request}");
        assert!(h.app.session.selection.is_empty());
    }
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert!(Arc::ptr_eq(&evaluated, &crate::open_meshing::session_world(&h.app.session)));
    assert_eq!(before, (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.is_dirty()));
}

#[test]
fn render_workspace_browser_focus_is_per_document_and_invalid_selection_is_atomic() {
    let mut h = Harness::new();
    h.app.run("view.render_select", json!({"kind":"environment"})).unwrap();
    let doc = h.app.session.doc.clone();
    for request in [
        json!({}),
        json!({"kind":"object"}),
        json!({"kind":"light"}),
        json!({"kind":"shot","id":""}),
        json!({"kind":"lighting","id":"extra"}),
        json!({"kind":"output","id":"x"}),
        json!({"kind":"shot","id":"x".repeat(1025)}),
        json!({"kind":"nothing","extra":1}),
    ] {
        assert!(h.app.run("view.render_select", request).is_err());
        assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Environment);
    }
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    crate::documents::open_tab(&mut h.app, Session::default()).unwrap();
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Nothing);
    h.app.run("view.render_select", json!({"kind":"output"})).unwrap();
    crate::documents::switch(&mut h.app, 0);
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Environment);
    crate::documents::switch(&mut h.app, 1);
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Output);
}

#[test]
fn render_workspace_wear_tree_selects_the_scene_stack_and_counts_layers_in_both_lenses() {
    use solidcraft_engine::render::job::aging::{AgingKind, AgingLayer};
    fn has_text(shape: &egui::Shape, needle: &str) -> bool {
        match shape {
            egui::Shape::Text(text) => text.galley.text().contains(needle),
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| has_text(shape, needle)),
            _ => false,
        }
    }
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1440,"height":1400}));
    h.app.run("solid.box", json!({"length":20,"width":15,"height":10,"body_name":"Subject"})).unwrap();
    enter(&mut h.app, true).unwrap();
    h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body { name: "Subject".into() }]})).unwrap();
    let document = h.app.session.doc.clone();
    assert_eq!(scene::wear_subtitle(&h.app), h.app.tr("render.wear_no_layers"));
    h.frames(3);
    assert!(h.shapes.iter().any(|s| has_text(&s.shape, h.app.tr("render.wear_no_layers"))));
    h.step(&json!({"click":{"handle":"render:tree:wear"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Wear);
    assert!(h.app.session.selection.is_empty());
    assert!(crate::scenario::rect_of("render:wear:controls").is_some());
    assert!(!crate::icon_svg::resolve("appearance").placeholder);
    for count in 1..=2 {
        h.app.render_workspace.view.studio.aging.push(AgingLayer::preset(AgingKind::Dust)).unwrap();
        h.app.render_workspace.view.dirty = true;
        let label =
            crate::i18n::format(if count == 1 { "render.wear_layer_one" } else { "render.wear_layer_count" }, &[("count", &count.to_string())]);
        assert_eq!(scene::wear_subtitle(&h.app), label);
        for by_look in [false, true] {
            h.app.run("view.render_browser", json!({"by_look":by_look})).unwrap();
            h.frames(3);
            assert!(crate::scenario::rect_of("render:tree:wear").is_some());
            assert!(h.shapes.iter().any(|s| has_text(&s.shape, &label)));
        }
    }
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert!(h.app.dialog.is_none() && h.app.tool.is_none());
}

/// Actual application tree at the approved card width, in both themes and both task states.
#[test]
fn render_workspace_browser_compact_geometry_search_and_theme_evidence() {
    let previous = crate::theme::is_dark();
    let mut themes = Vec::new();
    for dark in [false, true] {
        let mut h = Harness::new();
        for (name, x, appearance) in [
            ("Plate", 0, "Aluminum - Satin"),
            ("Boss", 20, "Aluminum - Anodized Blue"),
            ("Knob", 40, "Copper"),
            ("Cover", 60, "Glass - Clear"),
            ("Spacer", 80, "Aluminum - Satin"),
            ("Bolt", 100, "Aluminum - Satin"),
            ("Nut", 120, "Aluminum - Satin"),
        ] {
            h.app.run("solid.box", json!({"corner":[x,0,0],"length":10,"width":10,"height":10,"body_name":name})).unwrap();
            h.app.run("appearance.assign", json!({"bodies":[name],"appearance":appearance})).unwrap();
        }
        let group =
            h.app.run("browser.group", json!({"folder":"bodies","name":"Spacer, fasteners","items":["Spacer","Bolt","Nut"]})).unwrap()["group"]
                .as_u64()
                .unwrap();
        h.app.tree.collapsed.insert(format!("c0/g{group}"));
        for name in ["Hero", "Front", "Detail"] {
            h.app.run("render.settings.set", json!({"name":name,"set":{"width":1920,"height":1080}})).unwrap();
        }
        enter(&mut h.app, true).unwrap();
        h.app.preferences.render_browser_width = 266.0;
        h.app.render_workspace.view.draft.width = 1920;
        h.app.render_workspace.view.draft.height = 1080;
        h.app.ui.dark = dark;
        h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body {name:"Boss".into()}]})).unwrap();
        let ctx = crate::fonts::test_context();
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0));
        let mut textures = std::collections::HashMap::new();
        let mut images = Vec::new();
        for stage in ["look", "light"] {
            h.app.run("view.render_stage", json!({"stage":stage})).unwrap();
            h.app.render_workspace.view.method = ImageMethod::CpuRaytrace;
            h.app.render_workspace.view.raytrace.lights = Some(serde_json::from_value(json!([{"direction":[0,0,-1],"color":[1,1,1],"intensity":1},{"direction":[1,0,0],"color":[1,1,1],"intensity":1},{"direction":[0,1,0],"color":[1,1,1],"intensity":1}])).unwrap());
            for section in ["lights", "shots", "objects"] {
                if (stage == "look" && section == "objects") || (stage == "light" && section == "lights") {
                    h.app.tree.collapsed.remove(&format!("render:tree:{section}"));
                } else {
                    h.app.tree.collapsed.insert(format!("render:tree:{section}"));
                }
            }
            if stage == "light" {
                h.app
                    .run(
                        "view.render_select",
                        json!({"kind":"light","id":format!("{}:render.tree_directional:0",h.app.render_workspace.view.method.label())}),
                    )
                    .unwrap();
            }
            let mut image = None;
            for frame in 0..12 {
                crate::scenario::clear_handles();
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(bounds),
                        time: Some(ctx.cumulative_frame_nr() as f64 / 60.0),
                        focused: true,
                        ..Default::default()
                    },
                    |ui| {
                        h.app.logic(ui.ctx());
                        h.app.ui(ui);
                    },
                );
                crate::ui_kit::test_textures_update(&mut textures, &output.textures_delta);
                output.textures_delta.clear();
                if frame == 11 {
                    let card = crate::scenario::rect_of("browser:card").unwrap();
                    assert!((card.width() - 266.0).abs() < 0.1);
                    let inspector = crate::scenario::rect_of("render:inspector").unwrap();
                    let stage_panel = h.app.shell.workspace_panels["render:appearances"].0;
                    assert!((card.top() - inspector.top()).abs() < 0.1);
                    assert!((stage_panel.bottom() - inspector.bottom()).abs() < 0.1);
                    assert!((stage_panel.top() - card.bottom() - Tokens::get().space_8).abs() < 0.1);
                    assert!(card.height() <= 330.0, "Browser must stay within the fixed tree height: {card:?}");
                    let header = crate::scenario::rect_of("render:browser:header").unwrap();
                    assert!(header.contains_rect(crate::scenario::rect_of("render:tree:by-look").unwrap()));
                    assert!(crate::scenario::handle_at("render:browser-model").is_none());
                    assert!(crate::scenario::handle_at("render:browser-library").is_none());
                    for (name, row, _, _) in crate::browser::drawn_rows() {
                        assert!((row.height() - 28.0).abs() < 0.1, "{name}: {row:?}");
                    }
                    if stage == "look" {
                        let row = crate::browser::drawn_rows().into_iter().find(|r| r.0 == "Boss").unwrap().1;
                        assert!((row.width() - (card.width() - 2.0)).abs() < 0.1);
                        assert!(crate::scenario::handle_at("eye:Boss").unwrap().x > row.right() - 24.0);
                    }
                    if std::env::var_os("SOLIDCRAFT_BROWSER_EVIDENCE").is_some() {
                        let full = crate::ui_kit::test_raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures);
                        image = Some(
                            image::imageops::crop_imm(&full, card.left() as u32, card.top() as u32, card.width() as u32, card.height() as u32)
                                .to_image(),
                        );
                    }
                }
            }
            if let (Some(image), Ok(dir)) = (image, std::env::var("SOLIDCRAFT_BROWSER_EVIDENCE")) {
                std::fs::create_dir_all(&dir).unwrap();
                image.save(std::path::Path::new(&dir).join(format!("scene-{stage}-{}.png", if dark { "dark" } else { "light" }))).unwrap();
                images.push(image);
            }
        }
        if let (Some(look), Some(light), Ok(dir)) = (images.first(), images.get(1), std::env::var("SOLIDCRAFT_BROWSER_EVIDENCE")) {
            let mut pair = image::RgbaImage::new(look.width() + light.width(), look.height().max(light.height()));
            image::imageops::overlay(&mut pair, look, 0, 0);
            image::imageops::overlay(&mut pair, light, 266, 0);
            pair.save(std::path::Path::new(&dir).join(format!("scene-stages-{}.png", if dark { "dark" } else { "light" }))).unwrap();
        }
        themes.push(images);
        h.app.run("view.render_browser", json!({"search":true,"query":"Copper"})).unwrap();
        h.app.tree.collapsed.remove("render:tree:objects");
        h.frames(4);
        let rows = crate::browser::drawn_rows();
        assert!(rows.iter().any(|r| r.0 == "Knob"));
        assert!(!rows.iter().any(|r| r.0 == "Boss"));
        h.app.run("view.render_browser", json!({"search":false})).unwrap();
        h.frames(3);
        assert!(crate::browser::drawn_rows().iter().any(|r| r.0 == "Boss"));
        for hidden in [true, false] {
            h.step(&json!({"click":{"handle":"eye:Boss"}})).unwrap();
            assert_eq!(h.app.session.visibility.hidden_bodies.iter().any(|name| name == "Boss"), hidden);
        }
    }
    if let (Some(light), Some(dark), Ok(dir)) = (themes.first(), themes.get(1), std::env::var("SOLIDCRAFT_BROWSER_EVIDENCE")) {
        for (index, stage) in ["look", "light"].iter().enumerate() {
            let mut pair = image::RgbaImage::new(light[index].width() + dark[index].width(), light[index].height().max(dark[index].height()));
            image::imageops::overlay(&mut pair, &light[index], 0, 0);
            image::imageops::overlay(&mut pair, &dark[index], 266, 0);
            pair.save(std::path::Path::new(&dir).join(format!("scene-{stage}-themes.png"))).unwrap();
        }
    }
    crate::theme::set_dark(previous);
}

#[test]
fn render_workspace_shots_modified_dot_update_and_revert_follow_only_camera() {
    let mut h = Harness::new();
    enter(&mut h.app, true).unwrap();
    h.frames(3);
    setups::save(&mut h.app).unwrap();
    inspector::select_editor(&mut h.app, inspector::Editor::Camera);
    h.frames(3);
    assert!(!shots::camera_modified(&h.app, "Studio"));
    assert!(crate::scenario::rect_of("dialog:list:shot:Studio:modified").is_none());
    let saved = h.app.session.doc.render.setups["Studio"].clone();
    h.app.render_workspace.view.draft.width = 317;
    h.frames(2);
    assert!(!shots::camera_modified(&h.app, "Studio"), "output settings must not mark the camera");
    h.app.cam.yaw += 0.3;
    h.frames(3);
    assert!(crate::scenario::rect_of("dialog:list:shot:Studio:modified").is_some());
    assert!(crate::scenario::handle_at("render:shot:update").is_some());
    h.step(&json!({"click":{"handle":"render:shot:update"}})).unwrap();
    h.frames(2);
    let mut expected = saved.clone();
    expected.camera = h.app.cam;
    assert_eq!(h.app.session.doc.render.setups["Studio"], expected, "Update must preserve saved output and lighting");
    assert_eq!(h.app.render_workspace.view.draft.width, 317, "Update must preserve the editor draft");
    assert!(!shots::camera_modified(&h.app, "Studio"));
    assert!(crate::scenario::rect_of("dialog:list:shot:Studio:modified").is_none());
    let tree = h.output.accesskit_update.as_ref().unwrap();
    let update = tree.nodes.iter().find(|(_, node)| node.label() == Some(h.app.tr("render.shots_update"))).unwrap();
    assert!(update.1.is_disabled(), "Update stays visible but inactive once the camera is saved");
    h.app.cam.roll += 0.2;
    h.frames(3);
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":"render:shot:revert"}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.cam, expected.camera);
    assert!(!shots::camera_modified(&h.app, "Studio"));
    assert_eq!(h.app.session.doc, document);
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
fn render_workspace_shots_save_prompt_and_size_dropdown_use_existing_commands() {
    let mut h = Harness::new();
    enter(&mut h.app, true).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:shots:save"}})).unwrap();
    assert!(inspector::confirmation_open(&h.app));
    h.app.render_workspace.view.save_shot_name = Some("Hero".into());
    h.frames(3);
    assert_eq!(crate::scenario::count_of("dialog:ok:enabled"), Some(1.0));
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    assert!(h.app.session.doc.render.setups.contains_key("Hero"));
    assert_eq!(h.app.render_workspace.view.name, "Hero");
    assert!(!inspector::confirmation_open(&h.app));
    assert_eq!(h.app.session.doc.render.setups["Hero"].camera, h.app.cam);
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":"render:shots:size"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:list:Full HD"}})).unwrap();
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (1920, 1080));
    assert_eq!(h.app.session.doc, doc);
    assert_eq!(h.app.session.undo.len(), undo);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn render_workspace_shots_thumbnails_bound_invalidate_and_never_replace_results_or_history() {
    use std::time::{Duration, Instant};
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":20,"width":15,"height":10})).unwrap();
    enter(&mut h.app, true).unwrap();
    for index in 0..20 {
        h.app.run("render.settings.set", json!({"name":format!("Shot {index:02}"),"set":{"camera":h.app.cam}})).unwrap();
    }
    // A real completed frame stays available, including its exact PNG and retained texture.
    let id = completed_preview(&mut h, [20, 40, 60]);
    h.app.render_workspace.view.job = Some(id);
    cache_render_images(&mut h.app, &h.ctx);
    h.app.render_workspace.view.job = None;
    let png = h.app.session.render_job_handle(id).unwrap().frame().unwrap().unwrap().png().to_vec();
    let retained = h.app.render_workspace.image.as_ref().unwrap().1.as_ref().unwrap().id();
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let deadline = Instant::now() + Duration::from_secs(20);
    while h.app.render_workspace.thumbnails.entries.len() < thumbnails::LIMIT {
        assert!(Instant::now() < deadline, "thumbnail worker did not complete");
        thumbnails::refresh(&mut h.app, &h.ctx);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(h.app.render_workspace.thumbnails.entries.len(), 16);
    assert!(h.app.render_workspace.thumbnails.entries.values().all(|texture| texture.as_ref().is_some_and(|texture| texture.size() == [104, 60])));
    assert_eq!(h.app.render_workspace.image.as_ref().unwrap().1.as_ref().unwrap().id(), retained);
    assert_eq!(h.app.render_workspace.view.completed, Some(id));
    assert_eq!(h.app.session.render_job_handle(id).unwrap().frame().unwrap().unwrap().png(), png);
    assert!(h.app.render_workspace.view.job.is_none());
    assert_eq!(h.app.session.doc, doc);
    assert_eq!(h.app.session.undo.len(), undo);
    // Later shots must get previews on selection/scrolling rather than keeping the first 16 forever.
    h.app.render_workspace.view.name = "Shot 19".into();
    h.app.render_workspace.thumbnails.show(vec!["Shot 18".into(), "Shot 19".into()], &h.ctx);
    thumbnails::refresh(&mut h.app, &h.ctx);
    while h.app.render_workspace.thumbnails.texture("Shot 19").is_none() || h.app.render_workspace.thumbnails.texture("Shot 18").is_none() {
        assert!(Instant::now() < deadline, "visible thumbnail worker did not complete");
        thumbnails::refresh(&mut h.app, &h.ctx);
        assert!(h.app.render_workspace.thumbnails.entries.len() <= 16);
        std::thread::sleep(Duration::from_millis(2));
    }
    let old = h.app.render_workspace.thumbnails.entries.keys().cloned().collect::<Vec<_>>();
    // Camera, scene revision and visibility invalidate stale previews.
    let camera = Camera { yaw: 0.7, ..h.app.cam };
    h.app.run("render.settings.set", json!({"name":"Shot 00","set":{"camera":camera}})).unwrap();
    thumbnails::refresh(&mut h.app, &h.ctx);
    assert!(h.app.render_workspace.thumbnails.entries.keys().all(|key| !old.contains(key)));
    h.app.session.visibility.hidden_bodies.push("Body1".into());
    thumbnails::refresh(&mut h.app, &h.ctx);
    invalidate_scene(&mut h.app);
    assert!(h.app.render_workspace.thumbnails.entries.is_empty());
    thumbnails::refresh(&mut h.app, &h.ctx);
    reset_document(&mut h.app);
    assert!(h.app.render_workspace.thumbnails.entries.is_empty());
}

#[test]
fn render_workspace_shots_running_ring_stays_with_the_submitted_shot() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":20,"width":15,"height":10})).unwrap();
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.name = "A".into();
    setups::save(&mut h.app).unwrap();
    h.app.run("render.settings.set", json!({"name":"B","set":{}})).unwrap();
    h.app.start("render.start");
    assert_eq!(h.app.render_workspace.view.job_shot.as_deref(), Some("A"));
    h.app.render_workspace.view.name = "B".into();
    h.frames(2);
    assert!(crate::scenario::rect_of("dialog:list:shot:A:progress").is_some());
    assert!(crate::scenario::rect_of("dialog:list:shot:B:progress").is_none());
    h.app.start("render.cancel");
}

#[test]
fn render_workspace_shots_thumbnail_keys_include_name_scene_revision_and_complete_camera() {
    let camera = Camera::default();
    let key = thumbnails::Key::new("Hero".into(), 1, 2, camera);
    assert_eq!(key, thumbnails::Key::new("Hero".into(), 1, 2, camera));
    assert_ne!(key, thumbnails::Key::new("Front".into(), 1, 2, camera));
    assert_ne!(key, thumbnails::Key::new("Hero".into(), 2, 2, camera));
    assert_ne!(key, thumbnails::Key::new("Hero".into(), 1, 3, camera));
    for changed in [
        Camera { yaw: camera.yaw + 0.1, ..camera },
        Camera { roll: camera.roll + 0.1, ..camera },
        Camera { fov: 0.7, ..camera },
        Camera { up: solidcraft_engine::render::UpAxis::Y, ..camera },
    ] {
        assert_ne!(key, thumbnails::Key::new("Hero".into(), 1, 2, changed));
    }
}

#[test]
fn render_workspace_shots_heading_matches_design_history_typography_and_strip_baseline() {
    fn heading(h: &mut Harness, title: &str, strip: &str) -> (egui::FontId, Color32) {
        let mut output = h.ctx.run_ui(
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 1000.0))), ..Default::default() },
            |ui| {
                h.app.logic(ui.ctx());
                h.app.ui(ui);
            },
        );
        let rect = crate::scenario::rect_of(strip).unwrap();
        let text = crate::test_support::text_shapes(&output).find(|text| text.galley.text() == title && rect.contains(text.pos)).unwrap();
        let format = &text.galley.job.sections.first().unwrap().format;
        assert!((text.pos.y + text.galley.size().y / 2.0 - rect.center().y).abs() <= 1.0, "{title} centred on its strip");
        let appearance = (format.font_id.clone(), format.color);
        output.textures_delta.clear();
        appearance
    }
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.ui.layout = crate::shell::LayoutMode::Docked;
    h.app.preferences.timeline_left = false;
    h.app.ui.show_timeline = true;
    h.frames(3);
    let history = heading(&mut h, "History", "history:strip");
    enter(&mut h.app, true).unwrap();
    h.frames(3);
    assert_eq!(heading(&mut h, "Shots", "render:shots_strip"), history);
}

#[test]
fn render_workspace_frame_save_shot_dispatches_without_a_ribbon_or_document_edits() {
    let mut h = Harness::new();
    enter(&mut h.app, true).unwrap();
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.frames(3);
    assert!(crate::scenario::handle_at("render:stage:save_shot").is_none());
    h.app.run("view.render_stage", json!({"stage":"frame"})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:stage:save_shot"}})).unwrap();
    assert!(inspector::confirmation_open(&h.app));
    assert_eq!(h.app.session.doc, document);
    assert_eq!(h.app.session.undo.len(), undo);
    h.app.render_workspace.view.save_shot_name = Some("Frame shot".into());
    h.frames(3);
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    assert_eq!(h.app.session.doc.render.setups["Frame shot"].camera, h.app.cam);
    h.app.render_workspace.view.save_shot_name = None;
    assert!(h.app.run("render.settings.set", json!({})).is_err());
    assert!(!inspector::confirmation_open(&h.app));
    assert!(crate::workspace::destination(solidcraft_engine::find_command("view.shots_strip").unwrap()).is_none());
    assert!(crate::workspace::RIBBON_LABELS.iter().all(|(id, _)| *id != "view.shots_strip"));
}

#[test]
fn render_workspace_shots_strip_geometry_and_theme_evidence() {
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":20,"width":15,"height":10})).unwrap();
        enter(&mut h.app, true).unwrap();
        h.app.pending_fit = None;
        for (name, yaw) in [("Hero", 0.5), ("Front", 0.0), ("Detail", 0.8)] {
            h.app.run("render.settings.set", json!({"name":name,"set":{"camera":Camera {yaw,..h.app.cam}}})).unwrap();
        }
        switch_shot(&mut h.app, "Hero".into());
        h.app.render_workspace.view.draft.width = 1920;
        h.app.render_workspace.view.draft.height = 1080;
        let reply = h.app.session.execute("render.start", &json!({"settings":{"width":1920,"height":1080}})).unwrap();
        let id = serde_json::from_value(reply["job"].clone()).unwrap();
        let host = h.app.session.render_job_host().unwrap().clone();
        for _ in 0..10_000 {
            host.pump(16_384).unwrap();
            if h.app.session.render_job_handle(id).unwrap().frame().unwrap().is_some() {
                break;
            }
        }
        assert!(h.app.session.render_job_handle(id).unwrap().frame().unwrap().is_some());
        h.app.render_workspace.view.job = Some(id);
        cache_render_images(&mut h.app, &h.ctx);
        h.app.render_workspace.view.job = None;
        h.app.ui.dark = dark;
        let ctx = crate::fonts::test_context();
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0));
        let mut textures = std::collections::HashMap::new();
        for frame in 0..12 {
            crate::scenario::clear_handles();
            let mut output = ctx.run_ui(
                egui::RawInput { screen_rect: Some(bounds), time: Some(ctx.cumulative_frame_nr() as f64 / 60.0), ..Default::default() },
                |ui| {
                    h.app.logic(ui.ctx());
                    h.app.ui(ui);
                },
            );
            crate::ui_kit::test_textures_update(&mut textures, &output.textures_delta);
            output.textures_delta.clear();
            if frame == 11 {
                let strip = crate::scenario::rect_of("render:shots_strip").unwrap();
                assert!((strip.height() - 48.0).abs() < 1.0, "{strip:?}");
                let mut last = crate::scenario::rect_of("render:shots:heading").unwrap();
                for target in [
                    "render:shots:save",
                    "render:shots:setup:Detail",
                    "render:shots:setup:Front",
                    "render:shots:setup:Hero",
                    "render:shots:status",
                    "render:shots:size",
                    "render:shots:render",
                    "render:shots:hide",
                ] {
                    let rect = crate::scenario::rect_of(target).unwrap();
                    assert!(strip.contains_rect(rect), "{target}: {rect:?} in {strip:?}");
                    assert!(rect.left() >= last.right(), "{target} overlaps preceding control");
                    last = rect;
                }
                if let Ok(dir) = std::env::var("SOLIDCRAFT_SHOTS_EVIDENCE") {
                    std::fs::create_dir_all(&dir).unwrap();
                    let image = crate::ui_kit::test_raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures);
                    image.save(std::path::Path::new(&dir).join(format!("shots-{}.png", if dark { "dark" } else { "light" }))).unwrap();
                }
            }
        }
    }
    crate::theme::set_dark(previous);
}

#[test]
fn appearance_editor_hostile_optics_stay_finite_and_never_panic() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":3,"body_name":"Optics"})).unwrap();
    let mut target = materials::read_targets(&mut h.app).unwrap().remove(0);
    // Exercise the retained ordinal-only adapter; stable Glass capture is covered separately.
    target.stable_target = None;
    h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
    h.app.render_workspace.view.materials.enabled = true;
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 1e300] {
        let mut binding = materials::Binding::new(target.clone(), &h.app);
        binding.glass_enabled = true;
        binding.glass.ior = value;
        h.app.render_workspace.view.materials.bindings = vec![binding];
        assert!(materials::params(&h.app, &mut json!({})).is_err(), "hostile IOR {value}");
    }
    let mut binding = materials::Binding::new(target, &h.app);
    binding.glass_enabled = true;
    binding.glass.boundary = materials::Boundary::Box;
    binding.glass.subsurface = Some(solidcraft_engine::render::transmission::subsurface::Subsurface { scattering_per_mm: 0.1, anisotropy: 0.0 });
    h.app.render_workspace.view.materials.bindings = vec![binding];
    assert!(materials::params(&h.app, &mut json!({})).is_ok(), "box boundaries are closed dielectrics too");
}

fn output_viewer_fixture() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.session.set_render_job_host(Some(solidcraft_engine::render::job::CpuHost::cooperative())).unwrap();
    enter(&mut h.app, true).unwrap();
    h.app.run("view.render_stage", json!({"stage":"output"})).unwrap();
    h.app.run("view.render_select", json!({"kind":"output"})).unwrap();
    h.app.render_workspace.view.draft.width = 16;
    h.app.render_workspace.view.draft.height = 16;
    h.frames(3);
    h
}

#[test]
fn output_viewer_size_chips_obey_limits_and_largest_upscales_at_the_current_aspect() {
    let mut h = output_viewer_fixture();
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    for (key, expected) in [("720", (1280, 720)), ("1080", (1920, 1080))] {
        h.step(&json!({"click":{"handle":format!("render:preset:{key}")}})).unwrap();
        let draft = &h.app.render_workspace.view.draft;
        assert_eq!((draft.width, draft.height), expected);
    }
    let largest = inspector::largest_output(&h.app).unwrap();
    assert!(largest.0 > 1920);
    assert_eq!(cpu::image_limits(&h.app).fit(largest.0, largest.1, None), Some(largest));
    assert!((largest.0 as f64 / largest.1 as f64 - 16.0 / 9.0).abs() < 0.002);
    h.step(&json!({"click":{"handle":"render:size:largest"}})).unwrap();
    assert_eq!((u64::from(h.app.render_workspace.view.draft.width), u64::from(h.app.render_workspace.view.draft.height)), largest);
    let before = h.app.render_workspace.view.draft.clone();
    assert!(h.app.run("render.output.size", json!({"width":4096,"height":4096})).is_err());
    assert_eq!(h.app.render_workspace.view.draft, before);
    assert_eq!(h.app.session.doc.features, document.features);
    assert_eq!(undo + 3, h.app.session.undo.len());
    assert!(h.app.render_workspace.view.job.is_none());
}

#[test]
fn output_viewer_check_lists_preflight_results_without_starting_and_marks_edits_stale() {
    let mut h = output_viewer_fixture();
    let host = h.app.session.render_job_host().unwrap().clone();
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":"render:stage:check"}})).unwrap();
    assert!(h.app.render_workspace.view.inspector.checked.is_none());
    h.step(&json!({"click":{"handle":"render:check"}})).unwrap();
    assert!(h.app.render_workspace.view.job.is_none());
    assert_eq!(host.admitted_bytes(), 0);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(undo, h.app.session.undo.len());
    for key in ["ready", "size", "transparency"] {
        assert!(crate::scenario::rect_of(&format!("render:check:{key}")).is_some(), "{key}");
    }
    h.app.run("render.output.size", json!({"width":32,"height":16})).unwrap();
    h.frames(2);
    assert!(crate::scenario::rect_of("render:check:stale").is_some());
    assert!(h.app.render_workspace.view.job.is_none());
}

#[test]
fn output_viewer_copy_uses_retained_final_pixels_and_refuses_missing_or_oversized_images() {
    let mut h = output_viewer_fixture();
    assert!(ribbon::perform_job(&mut h.app, "render.result.copy").is_err());
    assert!(h.app.render_workspace.clipboard_image.is_none());
    assert!(decode_render_image(&[], 4097, 1, RenderLimits::default(), 4096).unwrap_err().contains("resource profile"));
    assert!(decode_render_image(&[], 4096, 4096, RenderLimits::default(), 4096).unwrap_err().contains("resource profile"));
    let first = completed_preview(&mut h, [12, 34, 56]);
    h.app.render_workspace.view.completed = Some(first);
    // A new queued render must not become the source for Copy.
    ribbon::perform_job(&mut h.app, "render.start").unwrap();
    ribbon::perform_job(&mut h.app, "render.result.copy").unwrap();
    let image = h.app.render_workspace.clipboard_image.as_ref().unwrap();
    assert_eq!(image.size, [16, 16]);
    assert_eq!(image.pixels[0], egui::Color32::from_rgb(12, 34, 56));
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.frame();
    assert!(h.app.render_workspace.clipboard_image.is_none());
    assert!(h.output.commands.iter().any(|command| matches!(command, egui::OutputCommand::CopyImage(image) if image.size == [16,16])));
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(undo, h.app.session.undo.len());
}

/// #11222: during a render, Cancel keeps its whole label below and from 10 %. A name that fits
/// is shown whole as the percentage grows; a long one is cut instead, and the row stays in the canvas.
/// In a window too narrow for the row, Cancel still shows whole.
#[test]
fn progress_pill_cancel_keeps_its_label_and_a_long_name_gives_way() {
    for image in [false, true] {
        let mut h = output_viewer_fixture();
        h.app.render_workspace.view.draft.width = 512;
        h.app.render_workspace.view.draft.height = 512;
        h.app.render_workspace.view.name = "A".into();
        ribbon::perform_job(&mut h.app, "render.start").unwrap();
        let job = h.app.render_workspace.view.job.unwrap();
        assert!(h.app.render_workspace.view.result, "Render opens its image by default");
        if !image {
            h.app.run("render.result.view", json!({"view":"model"})).unwrap();
        }
        h.frames(3);
        let whole = crate::scenario::rect_of("render:progress:cancel").unwrap().width();
        let fraction = |h: &mut Harness| {
            let status = h.app.session.execute("render.status", &json!({"job":job})).unwrap();
            status["progress"]["done"].as_f64().unwrap() / status["progress"]["total"].as_f64().unwrap().max(1.0)
        };
        assert!(fraction(&mut h) < 0.1);
        let long = "A very long shot name that will not fit beside the progress meter ".repeat(4);
        let host = h.app.session.render_job_host().unwrap().clone();
        for below in [true, false] {
            if !below {
                for _ in 0..100_000 {
                    if fraction(&mut h) >= 0.1 {
                        break;
                    }
                    host.pump(64).unwrap();
                }
                let reached = fraction(&mut h);
                assert!((0.1..1.0).contains(&reached), "{reached}");
            }
            for name in ["Bracket front view", long.as_str()] {
                h.app.render_workspace.view.job_shot = Some(name.into());
                h.frames(3);
                let cancel = crate::scenario::rect_of("render:progress:cancel").unwrap();
                let pill = crate::scenario::rect_of("render:progress-pill").unwrap();
                let shown = crate::scenario::rect_of("render:progress:name").unwrap();
                let case = format!("image: {image}, below 10 %: {below}, {} characters", name.len());
                assert!((cancel.width() - whole).abs() < 0.5, "{case}: Cancel {} wide, whole {whole}", cancel.width());
                assert!(pill.contains_rect(cancel), "{case}: {cancel:?} outside {pill:?}");
                assert!(shown.right() < cancel.left(), "{case}");
                let cut = crate::scenario::count_of("render:progress:name:cut");
                assert_eq!(cut, Some(if name == long { 1.0 } else { 0.0 }), "{case}");
                // The shared outer canvas owns the progress row in both views. The image
                // viewer reserves that row and its header above the smaller pan/zoom viewport.
                let canvas = h.app.shell.content.unwrap();
                let viewport = h.app.viewport.rect.unwrap();
                assert!(canvas.contains_rect(pill), "{case}: {pill:?} outside canvas {canvas:?}");
                for (panel, edge) in h.app.shell.workspace_panels.values() {
                    if edge.is_some() {
                        assert!(!pill.intersect(*panel).is_positive(), "{case}: {pill:?} overlaps reserved panel {panel:?}");
                    }
                }
                if image {
                    assert!(canvas.contains_rect(viewport), "{case}: viewport {viewport:?} outside canvas {canvas:?}");
                    assert!(!pill.intersect(viewport).is_positive(), "{case}: {pill:?} covers image viewport {viewport:?}");
                    for target in [
                        "render:result:view:model",
                        "render:result:view:fit",
                        "render:result:view:actual",
                        "render:result-copy",
                        "render:result-export",
                        "render:result-close",
                    ] {
                        let header = crate::scenario::rect_of(target).unwrap();
                        assert!(canvas.contains_rect(header), "{case}: header {target} {header:?} outside canvas {canvas:?}");
                        assert!(!pill.intersect(header).is_positive(), "{case}: {pill:?} covers header {target} {header:?}");
                    }
                } else {
                    assert!(viewport.contains_rect(pill), "{case}: {pill:?} outside viewport {viewport:?}");
                }
            }
        }
        // A window too narrow for the whole row: the name is cut and Cancel still shows whole.
        h.call("ui.resize", json!({"width": 480, "height": 600}));
        h.frames(3);
        let cancel = crate::scenario::rect_of("render:progress:cancel").unwrap();
        assert!((cancel.width() - whole).abs() < 0.5, "narrow window: Cancel {} wide, whole {whole}", cancel.width());
        assert_eq!(crate::scenario::count_of("render:progress:name:cut"), Some(1.0));
    }
}

#[test]
fn output_viewer_progress_keeps_stage_tools_and_cancel_cancels_without_closing_viewer() {
    let mut h = output_viewer_fixture();
    h.app.render_workspace.view.result = true;
    ribbon::perform_job(&mut h.app, "render.start").unwrap();
    let job = h.app.render_workspace.view.job.unwrap();
    h.frames(3);
    assert!(crate::scenario::rect_of("render:progress-pill").is_some());
    assert!(crate::scenario::rect_of("render:stage:size").is_some());
    assert!(crate::scenario::rect_of("render:hud").is_none());
    h.step(&json!({"click":{"handle":"render:progress-cancel"}})).unwrap();
    h.app.session.render_job_host().unwrap().pump(1024).unwrap();
    assert_eq!(h.app.session.execute("render.status", &json!({"job":job})).unwrap()["progress"]["phase"], "cancelled");
    assert!(h.app.render_workspace.view.result);
    h.frames(3);
    assert!(crate::scenario::rect_of("render:progress-pill").is_none());
    h.app.run("view.render_stage", json!({"stage":"look"})).unwrap();
    h.frames(3);
    assert!(crate::scenario::handle_at("render:pick").is_some());
}

#[test]
fn auto_opened_image_cancel_returns_to_model_and_preserves_previous_or_explicit_views() {
    for case in ["first", "previous", "explicit_before", "explicit_after", "comparison", "legacy"] {
        let mut h = output_viewer_fixture();
        let previous = if matches!(case, "previous" | "comparison") {
            let job = completed_preview(&mut h, [20, 40, 60]);
            sync_results(&mut h.app);
            Some(job)
        } else {
            None
        };
        if case == "explicit_before" {
            h.app.run("render.result.view", json!({"view":"image"})).unwrap();
        } else if case == "comparison" {
            h.app.run("render.compare", json!({"left":"live","right":0})).unwrap();
        } else if case == "legacy" {
            h.app.preferences.render_legacy_image_defaults = true;
        }
        let document = h.app.session.doc.clone();
        let history = (h.app.session.undo.len(), h.app.session.redo.len());
        ribbon::perform_job(&mut h.app, "render.start").unwrap();
        let job = h.app.render_workspace.view.job.unwrap();
        assert_eq!(
            h.app.render_workspace.view.auto_result_job.as_ref().map(|auto| auto.job),
            matches!(case, "first" | "previous" | "explicit_after").then_some(job),
            "{case}"
        );
        if case == "explicit_after" {
            h.app.run("render.result.view", json!({"view":"image"})).unwrap();
            assert!(h.app.render_workspace.view.auto_result_job.is_none());
        }
        ribbon::perform_job(&mut h.app, "render.cancel").unwrap();
        let host = h.app.session.render_job_host().unwrap().clone();
        for _ in 0..100 {
            host.pump(1024).unwrap();
            if h.app.session.execute("render.status", &json!({"job":job})).unwrap()["progress"]["phase"] == "cancelled" {
                break;
            }
        }
        assert_eq!(h.app.session.execute("render.status", &json!({"job":job})).unwrap()["progress"]["phase"], "cancelled");
        cache_render_images(&mut h.app, &h.ctx);
        assert_eq!(h.app.render_workspace.view.result, !matches!(case, "first" | "legacy"), "{case}");
        assert!(h.app.render_workspace.view.auto_result_job.is_none());
        assert_eq!(h.app.render_workspace.view.completed, previous, "{case}: cancelling must not change the retained image");
        assert_eq!(h.app.session.render_selected_result(), previous);
        assert_eq!(h.app.render_workspace.view.compare.is_some(), case == "comparison");
        if let Some(previous) = previous {
            assert_eq!(displayed_image(&h.app).unwrap().info.job, previous);
        }
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
/// Hold a GPU job in preparation to verify the window, shared keys and failure cleanup.
fn gpu_preparation_shows_shader_warning_then_reports_job_failure() {
    use solidcraft_engine::render::job;
    use std::sync::{Mutex, mpsc};
    use std::time::Duration;
    struct Failed(mpsc::Sender<()>, Mutex<mpsc::Receiver<()>>);
    impl job::ImageExecutor for Failed {
        fn descriptor(&self) -> job::Result<ExecutorDescriptor> {
            let mut descriptor = job::CpuHost::cooperative().descriptor()?;
            descriptor.id = ExecutorId::new("gpu-path").unwrap();
            descriptor.profiles[0].features.push(solidcraft_engine::render::RenderFeature::Progressive);
            Ok(descriptor)
        }
        fn admit(&self, scene: &ProductScene, request: &job::ImageRequest, _: job::JobOptions) -> job::Result<job::Admission> {
            job::admit(scene, request)
        }
        fn begin(&self, _: ProductScene, _: job::ImageRequest, _: job::JobOptions, _: job::CancelToken) -> job::Result<Box<dyn job::ImageWork>> {
            self.0.send(()).unwrap();
            let _ = self.1.lock().unwrap().recv_timeout(Duration::from_secs(10));
            Err(job::JobError::new(job::ErrorCode::Internal, "controlled image failure"))
        }
    }
    let mut h = output_viewer_fixture();
    let (entered, preparing) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let host = job::ImageHost::with_executor(Arc::new(Failed(entered, Mutex::new(wait)))).unwrap();
    h.app.session.register_render_job_host(host.clone()).unwrap();
    h.app.render_workspace.path_host = PathHost::Ready(host);
    h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
    ribbon::perform_job(&mut h.app, "render.start").unwrap();
    let job = h.app.render_workspace.view.job.unwrap();
    assert_eq!(h.app.render_workspace.view.auto_result_job.as_ref().map(|auto| auto.job), Some(job));
    let handle = h.app.session.render_job_handle(job).unwrap();
    preparing.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(handle.status().unwrap().progress.phase, job::Phase::Preparing);
    let keys = [egui::Key::Enter, egui::Key::Escape];
    let events = keys
        .into_iter()
        .map(|key| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE })
        .collect();
    h.ctx
        .run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
            assert!(show_shader_preparation(&mut h.app, ui.ctx()));
            for key in keys {
                assert!(ui.ctx().input_mut(|input| input.consume_key(egui::Modifiers::NONE, key)), "{key:?} must be available to other windows");
            }
            assert!(progress_pill(&mut h.app, ui, "render:progress"));
        })
        .textures_delta
        .clear();
    assert!(crate::scenario::rect_of("render:progress:name").is_some());
    assert!(crate::scenario::rect_of("dialog:progress:render:progress").is_some());
    for control in ["warning", "spinner", "cancel"] {
        assert!(crate::scenario::rect_of(&format!("render:shader-preparation:{control}")).is_some());
    }
    release.send(()).unwrap();
    assert_eq!(handle.wait_timeout(Duration::from_secs(10)).unwrap().progress.phase, job::Phase::Failed);
    assert!(!show_shader_preparation(&mut h.app, &h.ctx));
    cache_render_images(&mut h.app, &h.ctx);
    assert!(!h.app.render_workspace.view.result);
    assert!(h.app.render_workspace.view.auto_result_job.is_none());
    assert!(h.app.status.as_ref().is_some_and(|status| status.0.contains("controlled image failure")));
    assert!(h.app.session.render_retained_results().unwrap().is_empty());
}

#[test]
fn output_viewer_r_starts_active_setup_and_escape_returns_to_model_without_cancelling() {
    let mut h = output_viewer_fixture();
    h.app.render_workspace.view.name = "Active shot".into();
    h.app.render_workspace.view.draft.width = 32;
    h.app.render_workspace.view.draft.height = 24;
    assert_eq!(crate::keymap::command_for(&h.app, "R"), Some("render.start"));
    h.step(&json!({"key":"R"})).unwrap();
    let job = h.app.render_workspace.view.job.unwrap();
    assert_eq!(h.app.render_workspace.view.job_shot.as_deref(), Some("Active shot"));
    let status = h.app.session.execute("render.status", &json!({"job":job})).unwrap();
    assert_eq!(status["request"]["width"], 32);
    assert_eq!(status["request"]["height"], 24);
    // A repeated R while busy cannot fall through to Rectangle/Create Sketch.
    h.step(&json!({"key":"R"})).unwrap();
    assert_eq!(h.app.render_workspace.view.job, Some(job));
    assert!(h.app.dialog.is_none() && h.app.tool.is_none());
    h.app.run("render.result.view", json!({"view":"image"})).unwrap();
    h.frames(2);
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(!h.app.render_workspace.view.result);
    assert!(ribbon::busy(&h.app), "closing the image never cancels the job");
    h.app.run("render.result.view", json!({"view":"image"})).unwrap();
    h.app.viewport.nav = Some(crate::viewport::NavMode::Pan);
    crate::keymap::press(&mut h.app, "Escape", false);
    assert!(!h.app.render_workspace.view.result);
    assert!(h.app.viewport.nav.is_none(), "closing the image releases its navigation capture");
    enter(&mut h.app, false).unwrap();
    assert_ne!(crate::keymap::command_for(&h.app, "R"), Some("render.start"));
}

#[test]
fn retained_results_picker_and_discard_dispatch_the_selected_index() {
    let mut h = output_viewer_fixture();
    let first = completed_preview(&mut h, [20, 40, 60]);
    let second = completed_preview(&mut h, [80, 100, 120]);
    let document = h.app.session.doc.clone();
    let before = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
    h.app.run("render.result.select", json!({"index":1})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:results"}})).unwrap();
    h.frames(2);
    h.step(&json!({"click":{"handle":"render:result:select:0"}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.render_workspace.view.completed, Some(first));
    h.step(&json!({"click":{"handle":"render:result-discard"}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.render_workspace.view.completed, Some(second));
    assert_eq!(h.app.render_workspace.view.retained.len(), 1);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(before, (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision));
}

#[test]
fn retained_results_select_discard_copy_export_and_repaint_keep_exact_selected_pixels() {
    let mut h = output_viewer_fixture();
    let ctx = crate::fonts::test_context();
    let document = h.app.session.doc.clone();
    let before = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
    let mut jobs = Vec::new();
    for shade in 0..10 {
        let job = completed_preview(&mut h, [shade * 20, 40, 60]);
        h.app.render_workspace.view.job = Some(job);
        h.app.render_workspace.view.job_shot = Some(format!("Shot {shade}"));
        cache_render_images(&mut h.app, &ctx);
        jobs.push(job);
    }
    assert_eq!(h.app.render_workspace.view.retained.len(), 8);
    assert_eq!(h.app.render_workspace.view.retained.iter().map(|result| result.frame.job).collect::<Vec<_>>(), jobs[2..]);
    assert_eq!(h.app.render_workspace.view.retained[0].shot.as_deref(), Some("Shot 2"));
    h.app.run("render.result.select", json!({"index":0})).unwrap();
    assert!(h.app.render_workspace.view.result);
    for _ in 0..3 {
        cache_render_images(&mut h.app, &ctx);
        assert_eq!(h.app.render_workspace.view.completed, Some(jobs[2]));
        assert_eq!(h.app.render_workspace.image_info.as_ref().unwrap().job, jobs[2]);
    }
    h.app.run("render.result.copy", json!({})).unwrap();
    assert!(h.app.render_workspace.clipboard_image.as_ref().unwrap().pixels.iter().all(|pixel| *pixel == Color32::from_rgb(40, 40, 60)));
    let frame = h.app.session.render_job_handle(jobs[2]).unwrap().frame().unwrap().unwrap();
    let path = h.dir.join("selected-result.png");
    h.app.run("render.export", json!({"path":path})).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), frame.png());
    h.app.run("render.result.discard", json!({"index":0})).unwrap();
    cache_render_images(&mut h.app, &ctx);
    assert_eq!(h.app.render_workspace.view.completed, Some(jobs[9]));
    assert!(h.app.session.render_job_handle(jobs[2]).is_err());
    assert!(h.app.run("render.result.select", json!({"index":7})).is_err());
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(before, (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision));
    ctx.run_ui(Default::default(), |_| {}).textures_delta.clear();
}

#[test]
fn retained_results_and_selection_stay_with_the_document_tab_and_clear_on_reset() {
    let mut h = output_viewer_fixture();
    let first = completed_preview(&mut h, [20, 40, 60]);
    let second = completed_preview(&mut h, [80, 100, 120]);
    h.app.render_workspace.view.job = Some(second);
    cache_render_images(&mut h.app, &h.ctx);
    h.app.run("render.result.select", json!({"index":0})).unwrap();
    h.app.run("solid.box", json!({"length":1,"width":1,"height":1})).unwrap();
    cache_render_images(&mut h.app, &h.ctx);
    assert!(h.app.render_workspace.view.retained.iter().all(|result| result.stale));
    crate::documents::open_tab(&mut h.app, Session::default()).unwrap();
    assert!(h.app.render_workspace.view.retained.is_empty());
    assert!(h.app.run("render.result.select", json!({"index":0})).is_err());
    crate::documents::switch(&mut h.app, 0);
    cache_render_images(&mut h.app, &h.ctx);
    assert_eq!(h.app.render_workspace.view.completed, Some(first));
    assert_eq!(h.app.render_workspace.view.retained.len(), 2);
    h.app.run("render.result.discard", json!({"index":0})).unwrap();
    h.app.run("render.result.discard", json!({"index":0})).unwrap();
    cache_render_images(&mut h.app, &h.ctx);
    assert!(h.app.render_workspace.view.completed.is_none());
    assert!(displayed_image(&h.app).is_none(), "polling the discarded active job cannot resurrect its image");
    reset_document(&mut h.app);
    assert!(h.app.render_workspace.view.retained.is_empty());
}

#[test]
fn retained_results_capture_the_draft_shot_name_before_the_user_switches_shots() {
    let mut h = output_viewer_fixture();
    h.app.render_workspace.view.name = "Hero".into();
    let reply = h.app.run("render.start", json!({"settings":{"width":8,"height":8}})).unwrap();
    let job = serde_json::from_value(reply["job"].clone()).unwrap();
    let host = h.app.session.render_job_host().unwrap().clone();
    for _ in 0..100 {
        if !host.pump(16384).unwrap() {
            break;
        }
    }
    let duration = h.app.session.render_job_handle(job).unwrap().completed_duration().unwrap().unwrap().as_secs_f64();
    h.app.render_workspace.view.name = "Front".into();
    h.app.run("render.start", json!({"settings":{"width":8,"height":8}})).unwrap();
    sync_results(&mut h.app);
    assert_eq!(h.app.render_workspace.view.retained[0].shot.as_deref(), Some("Hero"));
    assert_eq!(h.app.render_workspace.view.retained[0].seconds, duration);
    assert_eq!(h.app.render_workspace.view.completed, Some(job));
    assert!(h.app.dialog.is_none());
}

/// Actual app controls and retained pixels, rasterized from egui without a window.
#[test]
fn output_viewer_evidence() {
    let Some(dir) = std::env::var_os("SOLIDCRAFT_OUTPUT_EVIDENCE") else { return };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        let mut h = output_viewer_fixture();
        h.app.run("solid.box", json!({"length":24,"width":18,"height":12})).unwrap();
        inspector::fit_subject(&mut h.app).unwrap();
        h.app.pending_fit = None;
        h.app.cancel_view_animation();
        h.app.preferences.render_inspector_width = 305.0;
        h.app.run("render.output.size", json!({"width":1280,"height":720})).unwrap();
        ribbon::perform_job(&mut h.app, "render.preflight").unwrap();
        h.ctx = crate::fonts::test_context();
        h.app.preferences.theme = if dark { "dark" } else { "light" }.into();
        h.app.ui.dark = dark;
        crate::theme::set_dark(dark);
        crate::theme::apply(&h.ctx);
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0));
        let mut textures = std::collections::HashMap::new();
        for (index, state) in ["settings", "check", "effects", "running", "completed"].into_iter().enumerate() {
            if state == "check" {
                h.app.run("view.render_select", json!({"kind":"check"})).unwrap();
                h.app.cancel_view_animation();
                ribbon::perform_job(&mut h.app, "render.preflight").unwrap();
            }
            if state == "effects" {
                select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
                h.app.start("render.effects.describe");
            }
            if state == "running" {
                select_image_method(&mut h.app, ImageMethod::Raster);
                h.app.run("view.render_select", json!({"kind":"output"})).unwrap();
                // Retain a real first image while the next job makes bounded progress.
                ribbon::perform_job(&mut h.app, "render.start").unwrap();
                let host = h.app.session.render_job_host().unwrap().clone();
                for _ in 0..1000 {
                    if !host.pump(65536).unwrap() {
                        break;
                    }
                }
                cache_render_images(&mut h.app, &h.ctx);
                assert!(h.app.render_workspace.view.completed.is_some());
                h.app.run("render.result.view", json!({"view":"image"})).unwrap();
                ribbon::perform_job(&mut h.app, "render.start").unwrap();
                host.pump(327680).unwrap();
                assert!(ribbon::busy(&h.app));
            }
            if state == "completed" {
                let host = h.app.session.render_job_host().unwrap().clone();
                for _ in 0..1000 {
                    if !host.pump(65536).unwrap() {
                        break;
                    }
                }
            }
            for frame in 0..10 {
                let mut events = Vec::new();
                if frame == 0 {
                    events.push(egui::Event::PointerMoved(egui::pos2(700.0, 500.0)));
                }
                let mut input = egui::RawInput {
                    screen_rect: Some(bounds),
                    focused: true,
                    time: Some((index * 10 + frame) as f64 / 60.0),
                    events,
                    ..Default::default()
                };
                h.app.raw_input_hook(&mut input);
                let mut output = h.ctx.run_ui(input, |ui| {
                    h.app.logic(ui.ctx());
                    if state == "check" && frame == 9 {
                        // Check the settled, scrolled view immediately before capturing it.
                        ribbon::perform_job(&mut h.app, "render.preflight").unwrap();
                    }
                    h.app.ui(ui);
                });
                crate::ui_kit::update_test_textures(&mut textures, &output.textures_delta);
                if frame == 9 {
                    if state == "check" {
                        for key in ["ready", "size", "transparency"] {
                            assert!(crate::scenario::rect_of(&format!("render:check:{key}")).is_some(), "{key}");
                        }
                        assert!(h.app.render_workspace.view.job.is_none());
                    }
                    crate::ui_kit::raster_ui(&h.ctx, output.shapes.clone(), output.pixels_per_point, bounds, &textures)
                        .save(dir.join(format!("output-{state}-{}.png", if dark { "dark" } else { "light" })))
                        .unwrap();
                }
                output.textures_delta.clear();
            }
        }
    }
    crate::theme::set_dark(previous);
}

#[test]
fn output_viewer_radial_defaults_enable_lights_and_compare_requires_a_result() {
    let mut h = render_lights_fixture();
    h.app.session.set_render_job_host(Some(solidcraft_engine::render::job::CpuHost::cooperative())).unwrap();
    let expected = [
        "render.start",
        "appearance.assign",
        "render.lights.add",
        "render.settings.set",
        "render.camera.fit",
        "render.compare",
        "ui.workspace.design",
        "undo",
    ];
    assert_eq!(crate::mouse_gesture::Studio::Render.defaults(), expected);
    assert!(solidcraft_engine::find_command("render.lights.add").unwrap().planned.is_none());
    assert!(crate::mouse_gesture::item(&h.app, "render.lights.add").unwrap().enabled);
    assert!(!crate::mouse_gesture::item(&h.app, "render.compare").unwrap().enabled);
    completed_preview(&mut h, [20, 40, 60]);
    assert!(crate::mouse_gesture::item(&h.app, "render.compare").unwrap().enabled);
    assert!(crate::mouse_gesture::item(&h.app, "render.lights.add").unwrap().enabled);
}

#[test]
fn output_viewer_presets_are_commands_with_limits_and_custom_keeps_the_size() {
    let mut h = output_viewer_fixture();
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.frames(6);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc), "opening Size presets does not apply anything");
    for (index, (key, expected)) in [("720", (1280, 720)), ("1080", (1920, 1080)), ("square", (2048, 2048))].into_iter().enumerate() {
        h.step(&json!({"click":{"handle":format!("render:preset:{key}")}})).unwrap();
        assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), expected);
        assert_eq!(h.app.session.undo.len(), undo + index + 1);
        let saved = &h.app.session.doc.render.setups[&h.app.render_workspace.view.name];
        assert_eq!((saved.width, saved.height), expected);
    }
    h.step(&json!({"click":{"handle":"render:preset:4k"}})).unwrap();
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (2048, 2048));
    h.step(&json!({"click":{"handle":"render:preset:custom"}})).unwrap();
    assert!(h.app.render_workspace.view.inspector.resolution.custom);
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (2048, 2048));
    assert_eq!(document.features, h.app.session.doc.features);
    assert_eq!(undo + 3, h.app.session.undo.len());
    assert!(h.app.render_workspace.view.job.is_none());
}

#[test]
fn output_viewer_has_sections_and_shared_footer_without_pages_and_selected_row_routes_to_output() {
    for dark in [false, true] {
        let mut h = output_viewer_fixture();
        h.app.preferences.theme = if dark { "dark" } else { "light" }.into();
        h.app.ui.dark = dark;
        h.call("ui.resize", json!({"width":1440,"height":900}));
        h.frames(3);
        for key in ["render.children.renderer", "render.output_image", "render.output_destination", "render.children.render"] {
            assert!(crate::scenario::rect_of(&format!("dialog:section:{}", h.app.tr(key))).is_some(), "{key}");
        }
        assert!(crate::scenario::rect_of("render:job-footer").is_none());
        let footer = crate::scenario::rect_of("render:output-footer").unwrap();
        for target in ["render:output:render", "render:output:all-shots"] {
            assert!(footer.contains_rect(crate::scenario::rect_of(target).unwrap()));
        }
        for page in ["output", "effects", "all"] {
            assert!(crate::scenario::rect_of(&format!("render:page:{page}")).is_none());
        }
        let tools = crate::scenario::rect_of("render:stage_tools").unwrap();
        for target in ["render:stage:size", "render:stage:check"] {
            assert!(tools.contains_rect(crate::scenario::rect_of(target).unwrap()));
        }
        h.app.run("view.render_stage", json!({"stage":"look"})).unwrap();
        h.frames(3);
        assert!(crate::scenario::rect_of("render:output-width").is_some(), "selected Output keeps its editor when the stage changes");
    }
}

#[test]
fn render_assign_gestures_selection_sizes_are_atomic_and_undoable() {
    use solidcraft_engine::Sel;
    let (mut h, occurrence) = render_pick_sample();
    for mode in ["body", "face", "component"] {
        h.app.run("select.filter", json!({"mode":mode})).unwrap();
        let at = product_pick_point(&h);
        h.step(&json!({"click":[at.x,at.y]})).unwrap();
        let before = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        assign::tile(&mut h.app, json!({"appearance":"ABS - Blue"}), false).unwrap();
        assert_eq!(h.app.session.undo.len(), undo + 1);
        match mode {
            "body" => assert_eq!(h.app.session.doc.appearances.bodies["Plate"].name, "ABS - Blue"),
            "face" => assert!(h.app.session.doc.appearances.faces.iter().any(|f| f.look.name == "ABS - Blue")),
            _ => {
                let id = h.app.session.doc.path_placement(&[occurrence]).unwrap().0;
                assert_eq!(h.app.session.doc.appearances.components[&id].name, "ABS - Blue");
            }
        }
        assert!(h.app.status_undo);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), before);
    }
    h.app.run("select.filter", json!({"mode":"body"})).unwrap();
    h.app.session.selection = vec![Sel::Body { name: "Plate".into() }, Sel::Body { name: "missing".into() }];
    {
        let selection = object_selection(&h.app);
        selection_editor(&mut h.app, &selection);
    }
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    assert!(assign::tile(&mut h.app, json!({"appearance":"ABS - Red"}), false).is_err());
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
fn render_assign_gestures_paint_sample_copy_paste_escape_and_failed_pick() {
    let (mut h, _) = render_pick_sample();
    h.app.run("select.filter", json!({"mode":"body"})).unwrap();
    h.app.run("select.clear", json!({})).unwrap();
    let before = h.app.session.doc.clone();
    assign::tile(&mut h.app, json!({"appearance":"ABS - Red"}), false).unwrap();
    assert!(h.app.render_workspace.view.assign.painting);
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    h.frames(3);
    let at = product_pick_point(&h);
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":[at.x,at.y]})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    h.step(&json!({"click":[at.x,at.y]})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 2);
    h.step(&json!({"widget":"Undo"})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1, "toast Undo must not paint through to the canvas");
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(!h.app.render_workspace.view.assign.painting);
    h.app.run("appearance.assign", json!({"body":"Plate","faces":[[20,20,8]],"appearance":"ABS - Blue"})).ok();
    h.app.start("appearance.pick_up");
    let candidate = crate::viewport::pick(&h.app, &crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap()), at)
        .into_iter()
        .find(|hit| matches!(hit, crate::viewport::Hit::Face { .. }))
        .unwrap();
    let source = match candidate {
        crate::viewport::Hit::Face { body, index, .. } => Some((body, index)),
        _ => None,
    };
    let (body, index) = source.unwrap();
    let expected = h.app.run("appearance.pick_up", json!({"target":{"body":body,"face":index}})).unwrap()["look"].clone();
    h.step(&json!({"click":[at.x,at.y]})).unwrap();
    assert!(h.app.render_workspace.view.assign.painting);
    assert_eq!(h.app.render_workspace.view.assign.look, Some(expected.clone()));
    h.step(&json!({"widget":"Done"})).unwrap();
    assert!(!h.app.render_workspace.view.assign.painting);
    h.app.start("appearance.paint");
    h.step(&json!({"key":"Escape"})).unwrap();
    h.app.session.selection = vec![solidcraft_engine::Sel::Body { name: "Plate".into() }];
    {
        let selection = object_selection(&h.app);
        selection_editor(&mut h.app, &selection);
    }
    h.app.start("appearance.copy");
    h.app.run("appearance.assign", json!({"bodies":["Plate"],"appearance":"ABS - Yellow"})).unwrap();
    let undo = h.app.session.undo.len();
    h.app.start("appearance.paste");
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.doc.appearances.bodies["Plate"].name, "ABS - Red");
    h.app.start("appearance.pick_up");
    let before = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    // Empty view: a failed pick neither paints nor exits sampling.
    let free = crate::render_ui::canvas(&h.app, h.app.viewport.rect.unwrap());
    let empty = egui::pos2(free.right() - 20.0, free.bottom() - 50.0);
    h.step(&json!({"click":[empty.x,empty.y]})).unwrap();
    assert!(h.app.render_workspace.view.assign.sampling);
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(!h.app.render_workspace.view.assign.sampling);
}

#[test]
fn render_assign_gestures_drag_preview_and_release_use_no_document_draft() {
    let (mut h, _) = render_pick_sample();
    h.app.run("select.filter", json!({"mode":"body"})).unwrap();
    let at = product_pick_point(&h);
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    assign::tile(&mut h.app, json!({"appearance":"ABS - Blue"}), true).unwrap();
    h.frame_with(vec![
        egui::Event::PointerMoved(at),
        egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE },
    ]);
    h.frames(2);
    assert!(assign::preview(&h.app).is_some());
    let mut scene = solidcraft_engine::product_scene::extract(&h.app.session).unwrap();
    let before = scene.meshes.iter().map(|m| m.color).collect::<Vec<_>>();
    assign::preview_scene(&mut scene, assign::preview(&h.app).as_ref()).unwrap();
    assert_ne!(scene.meshes.iter().map(|m| m.color).collect::<Vec<_>>(), before);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    h.frame_with(vec![egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    h.frames(2);
    assert!(assign::preview(&h.app).is_none());
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.doc.appearances.bodies["Plate"].name, "ABS - Blue");
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), document.to_json());
}

#[test]
fn render_assign_gestures_shortcuts_are_scoped_and_commands_are_not_stubs() {
    let mut h = Harness::new();
    assert_eq!(crate::keymap::command_for(&h.app, "I"), Some("inspect.measure"));
    enter(&mut h.app, true).unwrap();
    assert_eq!(crate::keymap::command_for(&h.app, "I"), Some("appearance.pick_up"));
    assert_eq!(crate::keymap::command_for(&h.app, "B"), Some("appearance.paint"));
    for (id, key) in [("appearance.copy", "Ctrl+Alt+C"), ("appearance.paste", "Ctrl+Alt+V")] {
        assert_eq!(crate::keymap::command_for(&h.app, key), Some(id));
    }
    for id in ["appearance.paint", "appearance.pick_up", "appearance.copy", "appearance.paste"] {
        assert!(solidcraft_engine::find_command(id).unwrap().planned.is_none());
    }
    let before = h.app.session.doc.clone();
    h.app.run("appearance.paint", json!({"appearance":"ABS - Red"})).unwrap();
    h.app.run("appearance.pick_up", json!({})).unwrap();
    assert!(!h.app.render_workspace.view.assign.painting && !h.app.render_workspace.view.assign.sampling);
    assert!(h.app.dialog.is_none());
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
}

#[test]
fn render_assign_gestures_grid_drag_modifiers_cancel_and_context_menu() {
    let (mut h, occurrence) = render_pick_sample();
    h.app.run("appearance.assign", json!({"bodies":["Plate"],"name":"Gesture blue","color":"#1234dd"})).unwrap();
    h.app.run("select.filter", json!({"mode":"body"})).unwrap();
    h.app.run("select.clear", json!({})).unwrap();
    h.step(&json!({"click":{"handle":"render:library-scope:in-design"}})).unwrap();
    h.frames(3);
    let id = json!(["Gesture blue", "#1234dd", 1.0, null]).to_string();
    let source = crate::scenario::rect_of(&format!("render:library:{id}")).unwrap().center();
    let at = product_pick_point(&h);
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.frame_with(vec![egui::Event::PointerMoved(source)]);
    h.frame_with(vec![egui::Event::PointerButton {
        pos: source,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    }]);
    h.frame_with(vec![egui::Event::PointerMoved(source + egui::vec2(20.0, 0.0))]);
    h.frame_with(vec![egui::Event::PointerMoved(at)]);
    h.frames(2);
    assert!(assign::preview(&h.app).is_some(), "grid drag must arm a live preview over geometry");
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    for mods in [egui::Modifiers::SHIFT, egui::Modifiers::CTRL, egui::Modifiers::COMMAND] {
        h.frame_with(vec![egui::Event::ModifiersChanged(mods)]);
        h.frames(2);
        let hover = assign::hover(&h.app).unwrap();
        if mods.shift {
            assert!(matches!(hover, solidcraft_engine::Sel::Face { .. }));
        } else {
            assert_eq!(hover, &solidcraft_engine::Sel::Occurrence { path: vec![occurrence] });
        }
        assert!(assign::preview(&h.app).is_some());
    }
    h.frame_with(vec![
        egui::Event::ModifiersChanged(egui::Modifiers::SHIFT),
        egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::SHIFT },
    ]);
    h.frames(2);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.doc.appearances.faces.last().unwrap().look.name, "Gesture blue");
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), doc.to_json());
    assign::tile(&mut h.app, json!({"appearance":"ABS - Red"}), true).unwrap();
    // The undo left no walk of the placements; the drag's start makes the one a Ctrl-drop needs.
    assert_eq!(crate::selection_mode::part_of(&h.app, "Plate"), solidcraft_engine::Sel::Occurrence { path: vec![occurrence] });
    h.step(&json!({"move":[at.x,at.y]})).unwrap();
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(assign::preview(&h.app).is_none());
    assert_eq!(h.app.session.doc.to_json(), doc.to_json());
    let menu = assign::menu(&h.app, "Plate");
    for id in ["appearance.library", "appearance.pick_up", "appearance.copy", "appearance.paste", "appearance.assign"] {
        assert!(menu.children.iter().any(|item| item.id == id), "missing menu action {id}");
    }
    let browse = menu.children.iter().find(|item| item.id == "appearance.library").unwrap();
    crate::context_menu::run_item(&mut h.app, browse, at);
    assert!(h.app.dialog.is_none());
    assert!(!h.app.render_workspace.view.assign.painting);
}

#[test]
fn render_assign_gestures_labels_translate_in_all_five_catalogs() {
    for language in ["en", "ja", "zh", "de", "ko"] {
        let locale = crate::i18n::Locale::for_language(language);
        for id in [
            "cmd.appearance_copy",
            "cmd.appearance_paste",
            "render.assign_help",
            "render.browse_looks",
            "render.choose_look",
            "render.look_applied",
            "render.painting_with",
            "render.pick_up_prompt",
            "render.recent_looks",
            "render.remove_override",
            "cmd.appearance_favorite",
            "render.replace_all",
            "render.replace_with",
            "render.look_material",
            "render.hover_look",
        ] {
            assert_ne!(locale.text(id), id, "{language}: {id}");
        }
        assert!(locale.format("render.painting_with", &[("name", "Sample")]).contains("Sample"));
    }
}

#[test]
fn render_lighting_group_selection_opens_ordered_editor_and_plus_still_adds() {
    let mut h = render_lights_fixture();
    h.call("ui.resize", json!({"width":1440,"height":2400}));
    let before = h.app.session.doc.clone();
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:tree:lights"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Lighting);
    assert_eq!(inspector::selected_editor(&h.app), inspector::Editor::Lighting);
    assert!(crate::scenario::rect_of("render:lighting:editor").is_some());
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    let sections: Vec<_> = ["render.lighting_source_section", "render.lighting_shadows_section", "render.lighting_budget"]
        .iter()
        .map(|key| crate::scenario::rect_of(&format!("dialog:section:{}", h.app.tr(key))).unwrap())
        .collect();
    assert!(sections.windows(2).all(|pair| pair[0].bottom() <= pair[1].top()));
    h.step(&json!({"click":{"handle":"render:tree:lights:add"}})).unwrap();
    assert!(light::rows(&h.app).unwrap().iter().any(|l| l.geometry.kind() == "area"));
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Light("area:0".into()));
    assert!(crate::scenario::rect_of("render:light:editor").is_some());
}

#[test]
fn render_lighting_methods_show_reasons_and_budgets_without_changing_method_or_design() {
    for method in [ImageMethod::Raster, ImageMethod::CpuRaytrace, ImageMethod::GpuRaytrace] {
        let mut h = render_lights_fixture();
        h.call("ui.resize", json!({"width":1440,"height":2400}));
        if method == ImageMethod::GpuRaytrace {
            h.app.run("render.settings.set", json!({"name":"Studio","set":{"job":{"method":"gpu_path","options":{"lighting":{}}}}})).unwrap();
            setups::load(&mut h.app);
        } else {
            h.app.render_workspace.view.method = method;
        }
        h.app.run("view.render_select", json!({"kind":"lighting"})).unwrap();
        let before = h.app.session.doc.clone();
        let revision = h.app.session.revision;
        h.frames(3);
        for section in ["render.lighting_source_section", "render.lighting_shadows_section", "render.lighting_budget"] {
            assert!(crate::scenario::rect_of(&format!("dialog:section:{}", h.app.tr(section))).is_some());
        }
        let reason = match method {
            ImageMethod::Raster => "render.method_limit_raster",
            ImageMethod::CpuRaytrace => "render.lighting_samples_need_area",
            ImageMethod::GpuRaytrace => "render.lighting_shadow_cpu",
        };
        assert!(crate::scenario::rect_of(&format!("dialog:note:{reason}")).is_some());
        if method == ImageMethod::GpuRaytrace {
            assert!(crate::scenario::rect_of("dialog:note:render.lighting_shadow_cpu").is_some());
            assert!(crate::scenario::rect_of("dialog:note:render.lighting_samples_cpu").is_some());
        }
        assert!(h.widget(h.app.tr("render.lighting_budget_help")).is_some());
        assert_eq!(h.app.render_workspace.view.method, method);
        assert_eq!(h.app.session.revision, revision);
        assert!(Arc::ptr_eq(&before, &h.app.session.doc));
        assert!(h.app.dialog.is_none() && h.app.render_workspace.view.job.is_none());
    }
}

#[test]
fn render_assign_gestures_stage_tools_and_selection_actions_use_commands() {
    let (mut h, _) = render_pick_sample();
    h.app.run("appearance.assign", json!({"bodies":["Plate"],"appearance":"ABS - Blue"})).unwrap();
    h.app.run("select.filter", json!({"mode":"body"})).unwrap();
    h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body {name:"Plate".into()}]})).unwrap();
    h.frames(4);
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let tools = crate::scenario::rect_of("render:stage_tools").unwrap();
    for id in ["appearance.paint", "appearance.pick_up"] {
        assert!(tools.contains_rect(crate::scenario::rect_of(&format!("render:stage:{id}")).unwrap()));
        assert!(crate::workspace::destination(solidcraft_engine::find_command(id).unwrap()).is_none());
    }
    let pill = crate::scenario::rect_of("render:selection").unwrap();
    assert!((pill.height() - 34.0).abs() < 1.0, "{pill:?}");
    assert!(pill.width() < 200.0, "{pill:?}");
    for index in 0..4 {
        assert!(pill.contains_rect(crate::scenario::rect_of(&format!("selection:action:{index}")).unwrap()));
    }
    h.step(&json!({"click":{"handle":"render:stage:appearance.pick_up"}})).unwrap();
    assert!(h.app.render_workspace.view.assign.sampling);
    h.step(&json!({"key":"Escape"})).unwrap();
    h.step(&json!({"click":{"handle":"selection:action:0"}})).unwrap();
    assert!(crate::scenario::rect_of("render:appearance:chain-menu").is_some());
    h.step(&json!({"click":{"handle":"selection:action:2"}})).unwrap();
    assert_eq!(h.app.preferences.render_library_favorites.len(), 1);
    h.step(&json!({"click":{"handle":"selection:action:2"}})).unwrap();
    assert!(h.app.preferences.render_library_favorites.is_empty());
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    h.step(&json!({"click":{"handle":"selection:action:1"}})).unwrap();
    assert!(inspector::confirmation_open(&h.app));
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    h.step(&json!({"click":{"handle":"dialog:Replace with"}})).unwrap();
    h.step(&json!({"widget":"Copper"})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
    assert!(!inspector::confirmation_open(&h.app));
    assert_eq!(h.app.session.doc.appearances.bodies["Plate"].name, "Copper");
    assert_eq!(h.app.session.undo.len(), undo + 1);
    h.app.run("edit.undo", json!({})).unwrap();
    h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body {name:"Plate".into()}]})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"selection:action:3"}})).unwrap();
    assert!(!h.app.session.body_visible("Plate"));
    h.app.run("edit.undo", json!({})).unwrap();
    h.app.run("view.render_stage", json!({"stage":"light"})).unwrap();
    h.frames(3);
    assert!(crate::scenario::rect_of("render:selection").is_none());
    assert!(crate::scenario::rect_of("render:stage:appearance.paint").is_none());
}

#[test]
fn render_assign_gestures_replacement_draft_cannot_cross_a_document_edit() {
    let (mut h, _) = render_pick_sample();
    h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body {name:"Plate".into()}]})).unwrap();
    h.app.start("appearance.replace");
    assert!(assign::replacement_open(&h.app));
    h.app.run("appearance.assign", json!({"bodies":["Plate"],"appearance":"ABS - Red"})).unwrap();
    h.frames(3);
    assert!(!assign::replacement_open(&h.app));
    assert_eq!(h.app.session.doc.appearances.bodies["Plate"].name, "ABS - Red");
}

#[test]
fn render_assign_gestures_geometry_and_theme_evidence() {
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        let mut h = Harness::new();
        h.app.run("solid.cylinder", json!({"body_name":"Boss","radius":10,"height":20})).unwrap();
        h.app.run("appearance.assign", json!({"bodies":["Boss"],"appearance":"Aluminum - Anodized Blue"})).unwrap();
        h.app.run("solid.sphere", json!({"body_name":"Knob","center":[0,0,28],"radius":8})).unwrap();
        h.app.run("appearance.assign", json!({"bodies":["Knob"],"appearance":"Copper"})).unwrap();
        enter(&mut h.app, true).unwrap();
        h.app.run("select.filter", json!({"mode":"body"})).unwrap();
        h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body {name:"Boss".into()}]})).unwrap();
        h.app.pending_fit = None;
        h.app.ui.dark = dark;
        let ctx = crate::fonts::test_context();
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0));
        let mut textures = std::collections::HashMap::new();
        for frame in 0..48 {
            crate::scenario::clear_handles();
            let events = h
                .app
                .viewport
                .rect
                .and_then(|rect| crate::viewport::projection(&h.app, rect).to_screen(solidcore_record::Vec3::new(0.0, 0.0, 28.0)))
                .filter(|_| frame == 1)
                .map(|at| vec![egui::Event::PointerMoved(at)])
                .unwrap_or_default();
            let mut output =
                ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), time: Some(frame as f64 / 60.0), events, ..Default::default() }, |ui| {
                    h.app.logic(ui.ctx());
                    h.app.ui(ui);
                });
            crate::ui_kit::test_textures_update(&mut textures, &output.textures_delta);
            output.textures_delta.clear();
            if frame == 47 {
                assert!(
                    crate::scenario::rect_of("kit:tooltip").is_some(),
                    "the model hover uses the standard kit tooltip; hover: {:?}, pointer: {:?}",
                    h.app.viewport.hover,
                    ctx.pointer_hover_pos()
                );
                let tooltip = crate::scenario::rect_of("kit:tooltip").unwrap();
                assert!(tooltip.distance_to_pos(ctx.pointer_hover_pos().unwrap()) < Tokens::get().control_height, "{tooltip:?}");
                let pill = crate::scenario::rect_of("render:selection").unwrap();
                assert!((pill.height() - 34.0).abs() < 1.0, "{pill:?}");
                assert!((150.0..170.0).contains(&pill.width()), "{pill:?}");
                let tools = crate::scenario::rect_of("render:stage_tools").unwrap();
                for id in ["appearance.paint", "appearance.pick_up"] {
                    assert!(tools.contains_rect(crate::scenario::rect_of(&format!("render:stage:{id}")).unwrap()));
                }
                if let Ok(dir) = std::env::var("SOLIDCRAFT_GESTURES_EVIDENCE") {
                    std::fs::create_dir_all(&dir).unwrap();
                    let image = crate::ui_kit::test_raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures);
                    image.save(std::path::Path::new(&dir).join(format!("look-controls-{}.png", if dark { "dark" } else { "light" }))).unwrap();
                }
            }
        }
    }
    crate::theme::set_dark(previous);
}

#[test]
fn render_lights_each_kind_contains_design_sections_and_real_parameter_fields_in_order() {
    for (method, kind) in
        [("cpu_raytrace", "area"), ("cpu_raytrace", "point"), ("cpu_raytrace", "directional"), ("gpu_path", "area"), ("gpu_path", "directional")]
    {
        let mut h = render_lights_fixture();
        h.call("ui.resize", json!({"width":1440,"height":3000}));
        let options = if method == "gpu_path" { json!({"lighting":{}}) } else { json!({"lights":[],"punctual_lights":[],"area_lights":[]}) };
        h.app.run("render.settings.set", json!({"name":"Studio","set":{"job":{"method":method,"options":options}}})).unwrap();
        setups::load(&mut h.app);
        light::edit(&mut h.app, "render.lights.add", None, Some(json!({"kind":kind}))).unwrap();
        let before = h.app.session.doc.clone();
        h.frames(3);
        assert!(crate::scenario::rect_of("render:light:editor").is_some());
        let mut sections = vec!["render.light_position"];
        if kind == "area" {
            sections.push("render.lighting_shape");
        }
        if kind == "point" {
            sections.push("render.lighting_falloff");
        }
        sections.push("render.light_behaviour");
        let rects: Vec<_> = sections.iter().map(|key| crate::scenario::rect_of(&format!("dialog:section:{}", h.app.tr(key))).unwrap()).collect();
        assert!(rects.windows(2).all(|pair| pair[0].bottom() <= pair[1].top()), "{method}/{kind}");
        let fields: &[&str] = match kind {
            "area" => &["width_mm", "height_mm", "radiance:0", "edge_u_mm:0", "edge_v_mm:0", "position_mm:0"],
            "point" => &["reference_distance_mm", "minimum_distance_mm", "position_mm:0"],
            _ => &["direction:0", "direction:1", "direction:2"],
        };
        for field in fields {
            assert!(crate::scenario::rect_of(&format!("render:light:{field}")).is_some(), "{method}/{kind}: {field}");
        }
        for key in ["render.lighting_solo", "render.lighting_mute", "render.light_duplicate", "render.light_delete"] {
            assert!(h.widget(h.app.tr(key)).is_some(), "{method}/{kind}: {key}");
        }
        assert!(crate::scenario::rect_of("dialog:note:render.light_place_pending").is_some());
        assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    }
}

fn render_results_compare_fixture() -> (Harness, solidcraft_engine::render::job::JobId, solidcraft_engine::render::job::JobId) {
    let mut h = output_viewer_fixture();
    h.call("ui.resize", json!({"width":1440,"height":900}));
    let first = completed_preview(&mut h, [20, 40, 60]);
    h.app.render_workspace.view.job = Some(first);
    h.app.render_workspace.view.job_shot = Some("Front".into());
    cache_render_images(&mut h.app, &h.ctx);
    let second = completed_preview(&mut h, [80, 100, 120]);
    h.app.render_workspace.view.job = Some(second);
    h.app.render_workspace.view.job_shot = Some("Hero".into());
    cache_render_images(&mut h.app, &h.ctx);
    (h, first, second)
}

#[test]
fn render_results_compare_live_two_renders_swap_labels_and_common_framing_use_the_shared_controls() {
    let (mut h, first, second) = render_results_compare_fixture();
    let document = h.app.session.doc.clone();
    let before = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
    let camera = h.app.cam;
    h.app.run("render.compare", json!({"left":"live","right":1})).unwrap();
    h.frames(3);
    let view = h.app.render_workspace.view.compare.as_ref().unwrap();
    assert_eq!((view.left, view.right, view.reference), (None, Some(second), second));
    let image = crate::scenario::rect_of("render:compare:image").unwrap();
    let canvas = crate::scenario::rect_of("render:compare:canvas").unwrap();
    for target in ["render:compare:left-label", "render:compare:right-label", "render:compare:divider"] {
        assert!(canvas.contains_rect(crate::scenario::rect_of(target).unwrap()), "{target}");
    }
    assert!(
        h.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == h.app.tr("render.compare_live_preview")))
    );
    h.step(&json!({"click":{"handle":"render:compare:swap"}})).unwrap();
    h.frames(2);
    let view = h.app.render_workspace.view.compare.as_ref().unwrap();
    assert_eq!((view.left, view.right, view.reference), (Some(second), None, second));
    assert_eq!(crate::scenario::rect_of("render:compare:image"), Some(image));
    h.app.run("render.compare", json!({"left":0,"right":1})).unwrap();
    h.frames(2);
    let first_texture = h.app.render_workspace.comparison_images.iter().find(|(job, _)| *job == first).unwrap().1.as_ref().unwrap().id();
    let second_texture = h.app.render_workspace.image.as_ref().unwrap().1.as_ref().unwrap().id();
    let vertices = |texture| {
        h.shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id == texture => Some(mesh.vertices.iter().map(|vertex| vertex.pos).collect::<Vec<_>>()),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(vertices(first_texture), vertices(second_texture), "both rendered sources use the same image framing");
    h.step(&json!({"click":{"handle":"render:compare:swap"}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.render_workspace.image.as_ref().unwrap().1.as_ref().unwrap().id(), first_texture);
    assert!(h.app.render_workspace.comparison_images.iter().any(|(job, image)| *job == second && image.as_ref().unwrap().id() == second_texture));
    assert!(h.app.render_workspace.comparison_images.len() <= 2);
    assert_eq!(h.app.cam, camera);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(before, (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision));
    h.app.run("render.compare", json!({"off":true})).unwrap();
    assert!(h.app.render_workspace.view.compare.is_none());
    assert!(h.app.render_workspace.comparison_images.is_empty());
}

#[test]
fn render_results_compare_divider_drag_clamps_and_blocks_all_camera_input() {
    use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, vec2};
    let (mut h, _, _) = render_results_compare_fixture();
    h.app.run("render.compare", json!({"left":0,"right":1})).unwrap();
    let pending = h.app.run("render.start", json!({"settings":{"width":16,"height":16}})).unwrap()["job"].clone();
    assert!(!h.app.render_workspace.view.follow_job, "new job previews must not replace comparison framing");
    h.frames(3);
    h.app.preferences.nav = "maya".into();
    h.app.render_workspace.view.result_scale = ResultScale::Free;
    let product = h.app.cam;
    let result = h.app.render_workspace.view.result_camera;
    let document = h.app.session.doc.clone();
    let before = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
    let canvas = crate::scenario::rect_of("render:compare:canvas").unwrap();
    let start = crate::scenario::rect_of("render:compare:handle").unwrap().center();
    h.frame_with(vec![
        Event::ModifiersChanged(Modifiers::ALT),
        Event::PointerMoved(start),
        Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::ALT },
    ]);
    let end = egui::pos2(canvas.right() + 100.0, start.y);
    h.frame_with(vec![
        Event::PointerMoved(end),
        Event::MouseWheel {
            source: egui::MouseWheelSource::Unknown,
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, -300.0),
            modifiers: Modifiers::ALT,
            phase: egui::TouchPhase::Move,
        },
    ]);
    assert!(
        (h.app.render_workspace.view.compare.as_ref().unwrap().position - 0.95).abs() < 1e-6,
        "divider position {:?}, canvas {canvas:?}, press {start:?}, pointer {end:?}, captured {:?}",
        h.app.render_workspace.view.compare.as_ref().unwrap().position,
        h.ctx.dragged_id()
    );
    assert_eq!(h.app.render_workspace.view.result_camera, result);
    assert_eq!(h.app.cam, product);
    let end = egui::pos2(canvas.left() - 100.0, start.y);
    h.frame_with(vec![Event::PointerMoved(end)]);
    assert!((h.app.render_workspace.view.compare.as_ref().unwrap().position - 0.05).abs() < 1e-6);
    assert_eq!(h.app.render_workspace.view.result_camera, result);
    assert_eq!(h.app.cam, product);
    h.frame_with(vec![
        Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::ALT },
        Event::ModifiersChanged(Modifiers::NONE),
    ]);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(before, (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision));
    h.app.run("render.cancel", json!({"job":pending})).unwrap();
}

#[test]
fn render_results_compare_live_projection_matches_image_fit_zoom_and_pan_without_an_oversized_gpu_viewport() {
    use solidcore_record::Vec3;
    let canvas = egui::Rect::from_min_size(egui::pos2(100.0, 200.0), egui::vec2(800.0, 600.0));
    for fov in [0.0, 0.6] {
        let camera = Camera { fov, roll: 0.25, ..Default::default() };
        for image in [
            egui::Rect::from_center_size(canvas.center(), egui::vec2(400.0, 400.0)),
            egui::Rect::from_center_size(canvas.center(), egui::vec2(1600.0, 1200.0)),
            egui::Rect::from_min_size(egui::pos2(-200.0, 350.0), egui::vec2(1200.0, 900.0)),
        ] {
            let projection = product_projection(camera, 100.0, canvas, Some(image));
            for point in [camera.target, camera.target + Vec3::X * 5.0, camera.target + Vec3::Z * 10.0] {
                let (x, y, _) = camera.to_screen(point, f64::from(image.width()), f64::from(image.height()), 100.0).unwrap();
                let expected = egui::pos2(image.left() + x as f32, image.top() + y as f32);
                let clip = projection.apply(point);
                let actual = egui::pos2(
                    canvas.left() + ((clip[0] / clip[3] + 1.0) * 0.5) as f32 * canvas.width(),
                    canvas.top() + ((1.0 - clip[1] / clip[3]) * 0.5) as f32 * canvas.height(),
                );
                assert!(actual.distance(expected) < 0.001, "{actual:?} != {expected:?}");
            }
        }
        assert_eq!(product_projection(camera, 100.0, canvas, None), camera.view_proj(f64::from(canvas.width() / canvas.height()), 100.0));
    }
}

#[test]
fn render_lighting_preset_materialises_values_and_replacement_requires_review_consent() {
    let mut h = render_lights_fixture();
    h.call("ui.resize", json!({"width":1440,"height":2400}));
    h.app.run("view.render_select", json!({"kind":"lighting"})).unwrap();
    h.frames(3);
    let before = h.app.session.doc.clone();
    h.step(&json!({"click":{"handle":"render:lighting:preset"}})).unwrap();
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    // Confirm's primary action is disabled until its explicit acknowledgement.
    h.step(&json!({"widget":h.app.tr("render.lighting_consent")})).unwrap();
    h.step(&json!({"widget":h.app.tr("render.lighting_apply")})).unwrap();
    assert!(h.app.render_workspace.view.raytrace.preset.is_some());
    assert!(h.app.render_workspace.view.raytrace.lights.is_none());
    let lights = light::rows(&h.app).unwrap();
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":"render:lighting:custom"}})).unwrap();
    assert!(h.app.render_workspace.view.raytrace.preset.is_none());
    assert_eq!(h.app.render_workspace.view.raytrace.lights.as_ref().unwrap().len(), lights.len());
    let materialised = light::rows(&h.app).unwrap();
    for (before, after) in lights.iter().zip(materialised) {
        assert_eq!(before.colour, after.colour);
        assert_eq!(before.intensity, after.intensity);
    }
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Lighting);
}

#[test]
fn render_lighting_hostile_numeric_edits_refuse_without_panics_or_partial_state() {
    let mut h = render_lights_fixture();
    light::edit(&mut h.app, "render.lights.add", None, Some(json!({"kind":"point"}))).unwrap();
    let before = h.app.session.doc.clone();
    let history = h.app.session.undo.len();
    for set in
        [json!({"reference_distance_mm":1e300}), json!({"minimum_distance_mm":-1}), json!({"position_mm":[1e300,0,0]}), json!({"direction":[0,0,0]})]
    {
        assert!(light::edit(&mut h.app, "render.lights.set", Some("point:0"), Some(set)).is_err());
        assert!(Arc::ptr_eq(&before, &h.app.session.doc));
        assert_eq!(h.app.session.undo.len(), history);
    }
    h.frames(3);
    assert!(crate::scenario::rect_of("render:light:editor").is_some());
}

#[test]
fn render_lighting_shadow_edits_are_undoable_and_preset_reviews_reject_changed_drafts() {
    let mut h = render_lights_fixture();
    h.call("ui.resize", json!({"width":1440,"height":2400}));
    h.app.run("view.render_select", json!({"kind":"lighting"})).unwrap();
    h.frames(3);
    let before = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.step(&json!({"widget":h.app.tr("render.lighting_shadows")})).unwrap();
    assert!(!h.app.render_workspace.view.raytrace.shadows);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(matches!(&h.app.session.doc.render.setups["Studio"].job,
        Some(solidcraft_engine::doc::render_settings::RenderJob::CpuRaytrace { options, .. }) if !options.shadows));
    h.app.run("edit.undo", json!({})).unwrap();
    setups::load(&mut h.app);
    assert_eq!(h.app.session.doc, before);
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:lighting:preset"}})).unwrap();
    assert!(h.widget(h.app.tr("render.lighting_consent")).is_some());
    h.app.render_workspace.view.draft.width = 1200;
    h.frames(3);
    assert!(h.widget(h.app.tr("render.lighting_consent")).is_none());
    assert_eq!(h.app.session.doc, before);
    assert_eq!(h.app.render_workspace.view.draft.width, 1200);
}

#[test]
fn geometry_budget_follows_the_device_buffer_limit_and_keeps_the_web_cap() {
    // The standard 256 MiB device buffers: 2 GiB, far past the old fixed 128 MiB.
    assert_eq!(geometry_budget(256 << 20, false), 2 << 30);
    assert!(admit_geometry(129 << 20, geometry_budget(256 << 20, false)).is_ok());
    // A smaller device gets less, a larger one no more than the ceiling.
    assert_eq!(geometry_budget(64 << 20, false), 512 << 20);
    assert_eq!(geometry_budget(u64::MAX, false), DESKTOP_GEOMETRY_CEILING);
    for device in [0, 256 << 20, u64::MAX] {
        let budget = geometry_budget(device, true);
        assert_eq!(budget, 128 << 20);
        assert!(admit_geometry(budget, budget).is_ok());
        assert!(admit_geometry(budget + 1, budget).is_err());
    }
}

#[test]
fn geometry_refusal_reports_scene_size_limit_and_recovery() {
    use solidcraft_engine::{
        geom::{Aabb3, Mesh},
        render::ProductMesh,
    };
    let scene = ProductScene {
        meshes: vec![ProductMesh {
            name: "Synthetic assembly".into(),
            mesh: Arc::new(Mesh { triangles: vec![[0; 3]; 1 << 20], ..Default::default() }),
            color: [128; 3],
            opacity: 1.0,
            surface: Some(solidcraft_engine::render::surface::OpaqueSurface::new(solidcore_record::Vec3::new(0.5, 0.5, 0.5), 0.0, 0.5, 1.5).unwrap()),
            transmission: None,
            faces: Vec::new(),
        }],
        bounds: Aabb3::EMPTY,
    };
    let error = gpu_scene_with_budget(&scene, WEB_GEOMETRY_BYTES).err().unwrap();
    assert!(error.contains("144.0 MiB"), "{error}");
    assert!(error.contains("128.0 MiB"), "{error}");
    assert!(error.contains("Hide bodies in Design"), "{error}");
    let refused = upload_refusal(144 << 20, "Out of Memory");
    assert!(refused.contains("144.0 MiB") && refused.contains("Out of Memory") && refused.contains("Hide bodies in Design"), "{refused}");
}

#[test]
fn previous_geometry_budget_restores_the_fixed_limit() {
    for web in [false, true] {
        for device in [64 << 20, 256 << 20, u64::MAX] {
            let device_budget = geometry_budget(device, web);
            assert_eq!(preferred_geometry_budget(device_budget, false), device_budget);
            let previous = preferred_geometry_budget(device_budget, true);
            assert_eq!(previous, MAX_GEOMETRY_BYTES as u64);
            assert!(admit_geometry(previous, previous).is_ok());
            assert!(admit_geometry(previous + 1, previous).is_err());
        }
    }
}

#[test]
fn geometry_budget_change_retries_cached_admission_without_document_changes() {
    let mut session = Session::default();
    session.execute("solid.box", &json!({"length":1,"width":1,"height":1})).unwrap();
    let scene = solidcraft_engine::product_scene::extract(&session).unwrap();
    let bytes = preview_upload_plan(&scene, usize::MAX).unwrap().bytes().unwrap() as u64;
    let mut state = State::default();
    assert!(state.prepare_preview((7, 123, 99, bytes - 1), || Ok(scene.clone())).is_err());
    state.prepare_preview((7, 123, 99, bytes), || Ok(scene)).unwrap();
    assert_eq!(state.preview_upload_bytes, Some(bytes as usize));
    assert!(state.scene_error.is_none());
}

/// The Field of view preference (#5364) rescales the design view only: navigating the result image
/// turns Perspective view off and on around it without a rescale, and an angle changed while Render
/// is open leaves Render's camera alone and reaches the design view when it's back.
#[test]
fn the_field_of_view_rescales_only_the_design_view() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5})).unwrap();
    h.call("ui.set", json!({"perspective": true}));
    h.frames(3);
    let design = h.app.cam;
    let size = |cam: Camera, fov: f64| Camera { fov, ..cam }.half_height();
    enter(&mut h.app, true).unwrap();
    h.frames(2);
    with_result_camera(&mut h.app, |app| app.cam.distance *= 1.0);
    h.frames(2);
    let render = h.app.cam;
    h.call("ui.prefs", json!({"perspective_fov": 1.2}));
    h.frames(2);
    assert_eq!(h.app.cam, render, "Render's camera is its own");
    enter(&mut h.app, false).unwrap();
    h.frames(2);
    assert!(h.app.ui.perspective);
    assert!((size(h.app.cam, 1.2) / size(design, 0.6) - 1.0).abs() < 1e-9, "the design view keeps its size: {:?}", h.app.cam);
}

/// Fit Product keeps the whole product inside the image, whatever its aspect: a picture
/// taller than wide moves the camera back so the sides hold it too.
#[test]
fn fit_aspect_keeps_the_product_inside_a_tall_image() {
    use solidcore_record::Vec3;
    use solidcraft_engine::geom::Aabb3;
    let bounds = Aabb3 { min: Vec3::new(-30.0, -30.0, -30.0), max: Vec3::new(30.0, 30.0, 30.0) };
    let radius = bounds.diagonal();
    for fov in [0.0, std::f64::consts::FRAC_PI_4] {
        let mut landscape = Camera { fov, ..Default::default() };
        assert!(landscape.fit_aspect(&bounds, 2.0));
        let mut plain = Camera { fov, ..Default::default() };
        plain.fit(&bounds);
        assert_eq!(landscape, plain, "a wide picture keeps the standard fit");
        for aspect in [0.5, 0.25] {
            let mut cam = Camera { fov, ..Default::default() };
            assert!(cam.fit_aspect(&bounds, aspect));
            for p in corners(&bounds) {
                let q = cam.view_proj(aspect, radius).apply(p);
                let (x, y) = (q[0] / q[3], q[1] / q[3]);
                assert!(x.abs() <= 1.0 && y.abs() <= 1.0, "fov {fov}, aspect {aspect}: corner {p:?} projects to ({x}, {y}), outside the picture");
            }
        }
        // Nothing to fit leaves the camera alone.
        let mut cam = Camera { fov, distance: 123.0, ..Default::default() };
        assert!(!cam.fit_aspect(&Aabb3::EMPTY, 0.5));
        assert_eq!(cam.distance, 123.0);
    }
}

fn corners(b: &solidcraft_engine::geom::Aabb3) -> Vec<solidcore_record::Vec3> {
    let mut out = Vec::new();
    for x in [b.min.x, b.max.x] {
        for y in [b.min.y, b.max.y] {
            for z in [b.min.z, b.max.z] {
                out.push(solidcore_record::Vec3::new(x, y, z));
            }
        }
    }
    out
}

fn cold_render_entry_fixture() -> Harness {
    let document = {
        let mut author = Harness::new();
        author.app.run("solid.box", json!({"length":60,"width":20,"height":20})).unwrap();
        let saved_camera = Camera { distance: 321.0, ..Default::default() };
        author
            .app
            .run("render.settings.set", json!({"name":"Studio","set":{"width":1024,"height":4096,"camera":saved_camera,"job":{"method":"raster"}}}))
            .unwrap();
        author.app.run("render.settings.set", json!({"name":"Other","set":{"width":321,"job":{"method":"raster"}}})).unwrap();
        (*author.app.session.doc).clone()
    };
    let mut h = Harness::new();
    crate::documents::open_tab(&mut h.app, Session::opened(document, None)).unwrap();
    h.call("ui.resize", json!({"width":980,"height":720}));
    h.app.render_workspace.view.live_paused = true;
    assert!(!h.app.session.is_dirty());
    h
}

#[test]
fn cold_render_entry_fit_keeps_shot_switch_clean_without_updating_saved_cameras() {
    for legacy in [false, true] {
        let mut h = cold_render_entry_fixture();
        h.app.preferences.render_legacy_image_defaults = legacy;
        let document = h.app.session.doc.clone();
        let camera = h.app.cam;
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        enter(&mut h.app, true).unwrap();
        if legacy {
            assert!(h.app.pending_fit.is_some());
            assert!(h.app.render_workspace.entry_fit.is_some());
        } else {
            assert!(h.app.pending_fit.is_none());
            assert!(h.app.render_workspace.entry_fit.is_none());
            assert_eq!(h.app.cam, document.render.setups["Studio"].camera, "a composed saved shot is an exact camera, not a new fit");
        }
        h.frames(4);
        assert!(h.app.pending_fit.is_none());
        assert!(h.app.render_workspace.entry_fit.is_none());
        assert_ne!(h.app.cam, camera, "the cold automatic fit must actually move the camera");
        setups::refresh_dirty(&mut h.app);
        assert!(!h.app.render_workspace.view.dirty);
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert!(!h.app.session.is_dirty());
        switch_shot(&mut h.app, "Other".into());
        assert!(!inspector::confirmation_open(&h.app));
        assert_eq!(h.app.render_workspace.view.name, "Other");
        assert_eq!(h.app.render_workspace.view.draft.width, 321);
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
    }
}

#[test]
fn render_entry_fit_preserves_real_camera_and_numeric_draft_confirmation() {
    for kind in ["camera", "numeric"] {
        for timing in ["before-entry", "pending-fit", "settled-fit"] {
            let mut h = cold_render_entry_fixture();
            // Exercise entry Fit rather than the separate authored-camera activation policy.
            h.app.run("render.settings.set", json!({"name":"Studio","set":{"camera":Camera::default()}})).unwrap();
            if timing == "before-entry" {
                setups::load(&mut h.app);
            } else {
                enter(&mut h.app, true).unwrap();
                assert!(h.app.pending_fit.is_some());
                if timing == "settled-fit" {
                    h.frames(4);
                }
            }
            if kind == "camera" {
                h.app.cancel_view_animation();
                h.app.cam.yaw += 0.25;
            } else {
                h.app.render_workspace.view.draft.width = 205;
            }
            if timing == "before-entry" {
                enter(&mut h.app, true).unwrap();
                assert!(h.app.render_workspace.entry_fit.is_none());
            }
            let document = h.app.session.doc.clone();
            let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
            h.frames(4);
            setups::refresh_dirty(&mut h.app);
            assert!(h.app.render_workspace.view.dirty, "{kind} {timing}");
            switch_shot(&mut h.app, "Other".into());
            assert!(inspector::confirmation_open(&h.app), "{kind} {timing}");
            assert_eq!(h.app.render_workspace.view.name, "Studio");
            assert!(Arc::ptr_eq(&document, &h.app.session.doc));
            assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
        }
    }
}

#[test]
fn cold_cpu_entry_after_resize_uses_the_settled_render_layout_for_its_fit() {
    for fov in [0.0, 0.8] {
        for warm in [(1600, 1000), (700, 480)] {
            let mut h = cold_render_entry_fixture();
            h.app
                .run(
                    "render.settings.set",
                    json!({"name":"Studio","set":{"width":1024,"height":768,"camera":Camera::default(),
                "job":{"method":"cpu_raytrace","options":{"progressive":{"samples_per_pixel":16,"seed":0}}}}}),
                )
                .unwrap();
            let session = Session::opened((*h.app.session.doc).clone(), None);
            crate::documents::open_tab(&mut h.app, session).unwrap();
            h.app.render_workspace.view.live_paused = true;
            h.call("ui.resize", json!({"width":warm.0,"height":warm.1}));
            h.frames(4);
            h.app.ui.perspective = fov > 0.0;
            h.app.cam.fov = fov;
            let source = h.app.cam;
            let document = h.app.session.doc.clone();
            h.call("ui.resize", json!({"width":1100,"height":760}));
            enter(&mut h.app, true).unwrap();
            h.frames(6);
            let scene = solidcraft_engine::product_scene::extract(&h.app.session).unwrap();
            let expected = h.app.try_fitted_bounds(source, &scene.bounds).expect("the final measured Render layout must admit a fit");
            assert_eq!(h.app.cam, expected, "warm {warm:?}, fov {fov}: an automatic fit must settle in the current Render layout");
            assert!(h.app.pending_fit.is_none());
            assert!(!h.app.render_workspace.view.dirty);
            assert!(Arc::ptr_eq(&document, &h.app.session.doc));
            assert!(!h.app.session.is_dirty());
        }
    }
}

/// Entering Render fits the product to the setup in effect, not to the draft that setup
/// replaces: in a cold workspace whose saved Studio setup is portrait (1024 × 4096) the fit
/// waits for the setup to load, so with Frame open the product sits inside the final 1:4
/// picture, in orthographic and in perspective, and leaving Render gives the Design camera
/// back.
#[test]
fn entering_render_fits_a_saved_portrait_setup_inside_its_final_frame() {
    for fov in [0.0, 0.8] {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":60,"width":20,"height":20})).unwrap();
        let name = h.app.render_workspace.view.name.clone();
        h.app.run("render.settings.set", json!({"name":name,"set":{"width":1024,"height":4096}})).unwrap();
        h.app.ui.perspective = fov > 0.0;
        h.app.cam.fov = fov;
        h.frames(2);
        let design = (h.app.cam, h.app.ui.perspective);
        let draft = &h.app.render_workspace.view.draft;
        assert_ne!((draft.width, draft.height), (1024, 4096), "the cold draft is still the default");
        h.app.render_workspace.view.stage = Stage::Frame;
        enter(&mut h.app, true).unwrap();
        h.frames(3);
        assert!(shot::overlay_visible(&h.app));
        let draft = &h.app.render_workspace.view.draft;
        assert_eq!((draft.width, draft.height), (1024, 4096), "the saved setup is in effect");
        let frame = shot::preview_rect(&h.app, h.app.viewport.rect.unwrap()).unwrap();
        assert!((frame.width() / frame.height() - 0.25).abs() < 1e-3, "{frame:?}");
        let scene = solidcraft_engine::product_scene::extract(&h.app.session).unwrap();
        let cam = h.app.cam;
        let radius = scene.bounds.diagonal().max(10.0) + scene.bounds.center().dist(cam.target);
        for p in corners(&scene.bounds) {
            let (x, y, _) = cam.to_screen(p, f64::from(frame.width()), f64::from(frame.height()), radius).unwrap();
            assert!(
                (0.0..=f64::from(frame.width())).contains(&x) && (0.0..=f64::from(frame.height())).contains(&y),
                "fov {fov}: corner {p:?} at ({x}, {y}) outside the {frame:?} picture"
            );
        }
        enter(&mut h.app, false).unwrap();
        assert_eq!((h.app.cam, h.app.ui.perspective), design, "fov {fov}: leaving Render restores the Design camera");
    }
}

#[test]
fn render_results_compare_segments_work_from_live_and_close_safely_when_a_source_is_discarded() {
    let (mut h, first, second) = render_results_compare_fixture();
    h.app.run("render.result.view", json!({"view":"model"})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:compare:result:0"}})).unwrap();
    h.frames(2);
    let view = h.app.render_workspace.view.compare.as_ref().unwrap();
    assert_eq!((view.left, view.right), (None, Some(first)));
    h.step(&json!({"click":{"handle":"render:compare:result:1"}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.render_workspace.view.compare.as_ref().unwrap().right, Some(second));
    h.step(&json!({"click":{"handle":"render:compare:live"}})).unwrap();
    h.frames(2);
    assert!(h.app.render_workspace.view.compare.is_none());
    assert_eq!(h.app.render_workspace.view.completed, Some(second));
    h.app.run("render.compare", json!({"left":0,"right":1})).unwrap();
    h.frames(2);
    h.app.run("render.result.discard", json!({"index":0})).unwrap();
    h.frames(2);
    assert!(h.app.render_workspace.view.compare.is_none());
    assert_eq!(h.app.render_workspace.view.completed, Some(second));
    let third = completed_preview(&mut h, [100, 120, 140]);
    let fourth = completed_preview(&mut h, [140, 160, 180]);
    h.app.run("render.compare", json!({"left":1,"right":2})).unwrap();
    h.app.run("render.result.discard", json!({"index":0})).unwrap();
    h.frames(2);
    let view = h.app.render_workspace.view.compare.as_ref().unwrap();
    assert_eq!((view.left, view.right), (Some(third), Some(fourth)), "discarding an unrelated earlier row must not retarget comparison");
    assert_eq!(h.app.session.render_comparison(), Some([Some(third), Some(fourth)]));
    assert!(h.app.run("render.compare", json!({"left":"live","right":99})).is_err());
}

fn render_results_batch_fixture() -> Harness {
    let mut h = output_viewer_fixture();
    h.call("ui.resize", json!({"width":1440,"height":900}));
    for (index, name) in ["Front", "Hero", "Top"].iter().enumerate() {
        let camera = Camera { yaw: index as f64 * 0.3, ..Default::default() };
        h.app.run("render.settings.set", json!({"name":name,"set":{"camera":camera,"width":8,"height":8}})).unwrap();
    }
    h.frames(3);
    h
}

#[test]
fn scene_and_output_section_all_shots_use_pointer_queue_actions_and_keep_saved_camera_policy() {
    for (kind, target) in [("nothing", "render:scene:all-shots"), ("output", "render:output:section-all-shots")] {
        let mut h = render_results_batch_fixture();
        h.call("ui.resize", json!({"width":1600,"height":2200}));
        h.app.render_workspace.view.live_paused = true;
        h.app.run("view.render_select", json!({"kind":kind})).unwrap();
        h.app.render_workspace.view.draft.width = 128;
        h.app.render_workspace.view.draft.height = 128;
        h.app.render_workspace.view.draft.background = [31, 51, 71];
        h.app.render_workspace.view.dirty = true;
        h.frames(3);
        assert_eq!((h.app.session.render_queue_state().running, h.app.session.render_queue_state().waiting), (0, 0));
        assert!(h.app.render_workspace.view.job.is_none(), "painting {kind} must not submit a batch");
        let document = h.app.session.doc.clone();
        let history = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
        let saved_camera = h.app.session.doc.render.setups.values().next().unwrap().camera;
        let navigation = h.app.cam;
        h.step(&json!({"click":{"handle":target}})).unwrap();
        h.frames(1);
        let queue = h.app.session.render_queue_state();
        assert_eq!((queue.running, queue.waiting), (1, 2), "{kind}");
        let handle = h.app.session.render_job_handle(queue.job.unwrap()).unwrap();
        let status = handle.status().unwrap();
        assert_eq!((status.request.width, status.request.height), (128, 128));
        assert_eq!(status.request.background, [31, 51, 71]);
        assert_eq!(status.request.camera, saved_camera, "the existing batch policy replaces only each saved camera");
        assert_eq!(h.app.session.doc.render.setups.values().next().unwrap().width, 8);
        assert!(ribbon::perform_job(&mut h.app, "render.batch").is_err(), "an active queue cannot be duplicated");
        h.step(&json!({"click":{"handle":"render:stage:render.cancel"}})).unwrap();
        h.frames(3);
        assert_eq!((h.app.session.render_queue_state().running, h.app.session.render_queue_state().waiting), (0, 0));
        assert!(h.app.session.render_retained_results().unwrap().is_empty());
        assert_eq!(h.app.cam, navigation);
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!((h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision), history);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn all_shots_batch_waits_for_live_cleanup_and_keeps_the_captured_current_settings() {
    let Some(mut h) = live_cpu_fixture() else { return };
    for (index, name) in ["A", "B"].iter().enumerate() {
        let camera = Camera { yaw: 0.2 + index as f64 * 0.3, ..Default::default() };
        h.app.run("render.settings.set", json!({"name":name,"set":{"camera":camera,"width":8,"height":8,"job":{"method":"raster"}}})).unwrap();
    }
    h.app.render_workspace.view.draft.width = 7;
    h.app.render_workspace.view.draft.height = 5;
    live::advance(&mut h.app, &h.ctx, 1.0);
    live::advance(&mut h.app, &h.ctx, 1.3);
    assert_eq!(live::inspect(&h.app)["pending"], true);
    let expected = image_request(&h.app).unwrap();
    let document = h.app.session.doc.clone();
    let history = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
    ribbon::perform_job(&mut h.app, "render.batch").unwrap();
    assert!(live::final_pending(&h.app));
    assert_eq!((h.app.session.render_queue_state().running, h.app.session.render_queue_state().waiting), (0, 0));
    assert!(ribbon::perform_job(&mut h.app, "render.batch").is_err());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while h.app.session.render_queue_state().job.is_none() {
        live::advance(&mut h.app, &h.ctx, 1.4);
        assert!(std::time::Instant::now() < deadline, "batch did not leave live cleanup: {}", live::inspect(&h.app));
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(!live::final_pending(&h.app));
    assert_eq!(live::inspect(&h.app)["draining"], 0);
    let job = h.app.session.render_queue_state().job.unwrap();
    let handle = h.app.session.render_job_handle(job).unwrap();
    let status = handle.status().unwrap();
    assert_eq!((status.request.width, status.request.height), (7, 5));
    assert_eq!(status.request.camera, h.app.session.doc.render.setups["A"].camera);
    assert_eq!(json!(status.request.raytrace.as_ref().unwrap().progressive), expected["settings"]["job"]["options"]["progressive"]);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!((h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision), history);
    let _ = h.app.run("render.cancel", json!({"job":job}));
}

#[test]
fn render_results_batch_output_footer_only_starts_on_click_and_cancel_drops_waiting_shots() {
    let mut h = render_results_batch_fixture();
    assert!(h.app.render_workspace.view.job.is_none());
    assert_eq!((h.app.session.render_queue_state().running, h.app.session.render_queue_state().waiting), (0, 0));
    h.app.render_workspace.view.draft.width = 128;
    h.app.render_workspace.view.draft.height = 128;
    h.app.render_workspace.view.dirty = true;
    h.frames(2);
    let document = h.app.session.doc.clone();
    let before = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
    let camera = h.app.cam;
    h.step(&json!({"click":{"handle":"render:output:all-shots"}})).unwrap();
    h.frames(1);
    let queue = h.app.session.render_queue_state();
    assert_eq!((queue.running, queue.waiting), (1, 2));
    assert_eq!(h.app.render_workspace.view.job, queue.job);
    let status = crate::i18n::format("render.batch_status", &[("running", "1"), ("waiting", "2")]);
    assert!(h.shapes.iter().any(|shape| matches!(&shape.shape,egui::Shape::Text(text) if text.galley.text()==status)));
    assert!(crate::scenario::rect_of("render:shots:queue").is_some());
    h.step(&json!({"click":{"handle":"render:stage:render.cancel"}})).unwrap();
    h.frames(3);
    assert_eq!((h.app.session.render_queue_state().running, h.app.session.render_queue_state().waiting), (0, 0));
    assert!(h.app.session.render_retained_results().unwrap().is_empty());
    assert_eq!(h.app.cam, camera);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(before, (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision));
}

#[cfg(not(target_arch = "wasm32"))]
fn live_cpu_fixture() -> Option<Harness> {
    use solidcraft_engine::render::job::{CpuHost, CpuProgressiveOptions};
    let mut h = Harness::new();
    let host = CpuHost::for_platform().unwrap();
    h.app.session.set_render_job_host(Some(host.clone())).unwrap();
    h.app.render_workspace.image_host = Some(host);
    if image_host(&h.app, ImageMethod::CpuRaytrace).is_err() {
        eprintln!("native CPU live tests require raytrace-cpu");
        return None;
    }
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5,"body_name":"Subject"})).unwrap();
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.raytrace.progressive = Some(CpuProgressiveOptions { samples_per_pixel: 3, seed: 81 });
    h.app.viewport.rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(8.0, 8.0)));
    Some(h)
}

#[cfg(not(target_arch = "wasm32"))]
fn wait_live_complete(h: &mut Harness, now: f64) -> serde_json::Value {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        live::advance(&mut h.app, &h.ctx, now);
        let state = live::inspect(&h.app);
        assert!(state["error"].is_null(), "{state}");
        if state["complete"] == true {
            return state;
        }
        assert!(std::time::Instant::now() < deadline, "live image did not finish: {state}");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn automatic_live_cpu_pixels_refresh_without_replacing_final_export_or_saved_settings() {
    use solidcraft_engine::render::job::{JobId, Phase};
    let Some(mut h) = live_cpu_fixture() else { return };
    let reply = h
        .app
        .session
        .execute(
            "render.start",
            &json!({"settings":{"width":8,"height":8,"job":{"method":"cpu_raytrace","options":{"progressive":{"samples_per_pixel":3,"seed":82}}}}}),
        )
        .unwrap();
    let final_job: JobId = serde_json::from_value(reply["job"].clone()).unwrap();
    let final_handle = h.app.session.render_job_handle(final_job).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while final_handle.status().unwrap().progress.phase != Phase::Completed {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let bytes = final_handle.frame().unwrap().unwrap().png().to_vec();
    assert_eq!(h.app.session.render_retained_results().unwrap().len(), 1);
    let before = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let settings = setups::draft(&h.app).unwrap();
    live::advance(&mut h.app, &h.ctx, 10.0);
    assert_eq!(live::inspect(&h.app)["pending"], false, "visual edits are grouped before capture");
    let first = wait_live_complete(&mut h, 10.3);
    let live_job: JobId = serde_json::from_value(first["job"].clone()).unwrap();
    assert_ne!(live_job.session, final_job.session, "automatic pixels belong to an isolated session");
    assert_eq!(first["samples"], 3);
    assert!(first["hash"].as_str().is_some_and(|hash| !hash.is_empty()));
    assert_eq!(setups::draft(&h.app).unwrap(), settings, "live resolution must never change final settings");
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(h.app.render_workspace.view.job.is_none());
    assert_eq!(h.app.session.render_retained_results().unwrap().len(), 1);
    assert_eq!(h.app.session.render_selected_result(), Some(final_job));
    assert_eq!(final_handle.frame().unwrap().unwrap().png(), bytes);
    let path = h.dir.join("automatic-preview.png");
    assert!(h.app.session.execute("render.export", &json!({"job":live_job,"path":path.to_string_lossy()})).is_err());
    assert!(!path.exists(), "automatic output cannot become the final export source");

    h.app.run("appearance.assign", json!({"bodies":["Subject"],"color":"#204080"})).unwrap();
    live::advance(&mut h.app, &h.ctx, 11.0);
    assert!(!live::picking_current(&h.app), "old pixels must not authorize picking a changed scene");
    let second = wait_live_complete(&mut h, 11.3);
    assert_ne!(second["hash"], first["hash"], "the actual image must reflect the material edit");
    assert!(live::picking_current(&h.app));
    assert_eq!(h.app.session.render_selected_result(), Some(final_job));
    assert_eq!(final_handle.frame().unwrap().unwrap().png(), bytes);

    h.app.cam.yaw += 0.3;
    assert!(!live::picking_current(&h.app), "same-frame navigation cannot pick or highlight against the previous camera's pixels");
    assert_eq!(live::inspect(&h.app)["complete"], false);
    live::advance(&mut h.app, &h.ctx, 12.0);
    let third = wait_live_complete(&mut h, 12.3);
    assert_ne!(third["generation"], second["generation"]);
    assert_ne!(third["hash"], second["hash"], "camera movement must refresh real live pixels");
    assert_eq!(h.app.session.render_retained_results().unwrap().len(), 1);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cancelled_first_manual_cpu_checkpoint_resumes_live_without_retaining_or_exporting_it() {
    use solidcraft_engine::render::job::Phase;
    let Some(mut h) = live_cpu_fixture() else { return };
    h.app.render_workspace.view.draft.width = 64;
    h.app.render_workspace.view.draft.height = 64;
    h.app.render_workspace.view.raytrace.progressive.as_mut().unwrap().samples_per_pixel = 64;
    let document = h.app.session.doc.clone();
    let history = (h.app.session.undo.len(), h.app.session.redo.len());
    ribbon::perform_job(&mut h.app, "render.start").unwrap();
    let job = h.app.render_workspace.view.job.unwrap();
    let handle = h.app.session.render_job_handle(job).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        cache_render_images(&mut h.app, &h.ctx);
        if displayed_image(&h.app).is_some_and(|image| image.preview.is_some()) {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "actual CPU checkpoint was not published: {:?}", handle.status().unwrap());
        assert_ne!(handle.status().unwrap().progress.phase, Phase::Completed, "the fixture must cancel while a real checkpoint is displayed");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(h.app.render_workspace.view.result);
    ribbon::perform_job(&mut h.app, "render.cancel").unwrap();
    assert_eq!(handle.wait_timeout(std::time::Duration::from_secs(10)).unwrap().progress.phase, Phase::Cancelled);
    cache_render_images(&mut h.app, &h.ctx);
    assert!(!h.app.render_workspace.view.result);
    assert!(displayed_image(&h.app).is_none());
    assert!(h.app.session.render_retained_results().unwrap().is_empty());
    live::advance(&mut h.app, &h.ctx, 1.0);
    let resumed = wait_live_complete(&mut h, 1.3);
    assert_eq!(resumed["suspended"], false);
    assert_eq!(resumed["is_current"], true);
    assert!(h.app.render_workspace.view.completed.is_none());
    assert!(ribbon::action_ready(&h.app, "render.export").is_err());
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn live_edits_coalesce_and_pause_drains_late_capture_without_document_or_result_changes() {
    let Some(mut h) = live_cpu_fixture() else { return };
    let before = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    live::advance(&mut h.app, &h.ctx, 0.0);
    h.app.render_workspace.view.draft.exposure_ev = 0.1;
    live::advance(&mut h.app, &h.ctx, 0.1);
    h.app.render_workspace.view.draft.exposure_ev = 0.2;
    live::advance(&mut h.app, &h.ctx, 0.2);
    live::advance(&mut h.app, &h.ctx, 0.39);
    assert_eq!(live::inspect(&h.app)["pending"], false);
    let generation = live::inspect(&h.app)["generation"].clone();
    live::advance(&mut h.app, &h.ctx, 0.41);
    assert_eq!(live::inspect(&h.app)["pending"], true);
    assert_eq!(live::inspect(&h.app)["generation"], generation);
    let revision = h.app.session.revision;
    h.app.session.pending_moves.insert(99, solidcraft_engine::doc::IDENTITY);
    live::advance(&mut h.app, &h.ctx, 0.42);
    assert_ne!(live::inspect(&h.app)["generation"], generation, "placement previews invalidate even without a document revision");
    assert_eq!(h.app.session.revision, revision);
    h.app.run("view.render_live", json!({"paused":true})).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        live::advance(&mut h.app, &h.ctx, 1.0);
        let state = live::inspect(&h.app);
        if state["pending"] == false && state["draining"] == 0 {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "late live capture did not drain: {state}");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(live::inspect(&h.app)["job"].is_null());
    assert!(h.app.session.render_retained_results().unwrap().is_empty());
    assert_eq!(h.app.session.render_selected_result(), None);
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    assert_eq!(h.app.session.render_job_host().unwrap().admitted_bytes(), 0);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn current_live_pixels_use_only_the_depth_correct_feedback_callback_and_stale_pixels_have_none() {
    let Some(mut h) = live_cpu_fixture() else { return };
    live::advance(&mut h.app, &h.ctx, 1.0);
    wait_live_complete(&mut h, 1.3);
    h.app.viewport.gpu = Some(crate::gpu::GpuTarget {
        format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm,
        depth: Some(egui_wgpu::wgpu::TextureFormat::Depth24PlusStencil8),
        samples: 1,
        tess: false,
        oit: false,
    });
    h.app.render_workspace.host = Some(host_executor(4096, true, 1).unwrap());
    sync_backend(&mut h.app);
    h.app.session.selection = vec![solidcraft_engine::Sel::Body { name: "Subject".into() }];
    let count = |shapes: &[egui::epaint::ClippedShape]| {
        let mut feedback = 0;
        let mut model = 0;
        for shape in shapes {
            if let egui::Shape::Callback(callback) = &shape.shape
                && let Some(callback) = callback.callback.downcast_ref::<egui_wgpu::Callback>()
            {
                feedback += usize::from(callback.downcast_ref::<crate::gpu::ProductHighlightCallback>().is_some());
                model += usize::from(callback.downcast_ref::<crate::gpu::ViewportCallback>().is_some());
            }
        }
        (feedback, model)
    };
    let rect = h.app.viewport.rect.unwrap();
    let mut painted = h.ctx.run_ui(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ui| show(&mut h.app, ui));
    painted.textures_delta.clear();
    assert_eq!(count(&painted.shapes), (1, 0), "live pixels receive one feedback pass and no shaded product redraw");
    let scene = h.app.render_workspace.scene.as_ref().unwrap();
    let radius = scene.bounds.diagonal().max(10.0) + scene.bounds.center().dist(h.app.cam.target);
    let expected = product_projection(h.app.cam, radius, live::projection_rect(&h.app, shot::render_rect(&h.app, rect)), None).to_f32();
    let feedback = painted
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Callback(callback) = &shape.shape {
                callback
                    .callback
                    .downcast_ref::<egui_wgpu::Callback>()
                    .and_then(|callback| callback.downcast_ref::<crate::gpu::ProductHighlightCallback>())
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(feedback.0.view_proj, expected);
    assert!(
        feedback.0.hl_slot.lock().unwrap().as_ref().is_some_and(|scene| !scene.tris.is_empty() || !scene.blocks.is_empty()),
        "the selected target reuses existing highlight geometry"
    );
    h.app.cam.yaw += 0.3;
    let mut stale = h.ctx.run_ui(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ui| show(&mut h.app, ui));
    stale.textures_delta.clear();
    assert_eq!(count(&stale.shapes), (0, 0), "old-camera pixels cannot receive current-camera target feedback");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn live_image_target_outlines_keep_new_material_and_lighting_visible_without_clearing_selection() {
    use solidcore_record::Vec3;
    use solidcraft_engine::Sel;
    let Some(mut h) = live_cpu_fixture() else { return };
    h.app.viewport.rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(64.0, 64.0)));
    let world = h.app.session.world_state().unwrap();
    let body = &world.body("Subject").unwrap().body;
    // Select the analytic top face; tessellation is only target feedback.
    let face = (0..body.face_count())
        .find(|index| body.planar_face(*index).is_some_and(|plane| plane.origin.z == 5.0 && plane.normal() == Vec3::Z))
        .unwrap();
    let point = Vec3::new(5.0, 10.0, 5.0);
    h.app.session.selection = vec![Sel::Face { body: "Subject".into(), index: face, point }];
    h.app.viewport.hover = Some(crate::viewport::Hit::Face { body: "Subject".into(), index: face, point });
    let selected = h.app.session.selection.clone();
    live::advance(&mut h.app, &h.ctx, 1.0);
    let before = wait_live_complete(&mut h, 1.3);
    let outlines = |app: &SolveApp| {
        let feedback = crate::viewport::build_highlight(app).merged();
        assert!(
            feedback.tris.is_empty() && feedback.pbr.is_empty() && feedback.trans.is_empty(),
            "target feedback cannot replace image colour or shadows"
        );
        assert!(!feedback.lines.is_empty(), "depth-tested visible outlines must identify the target");
        assert!(feedback.overlay.is_empty() && feedback.xray.is_empty(), "target outlines cannot bypass product depth");
    };
    outlines(&h.app);
    h.app.run("appearance.assign", json!({"faces":[[5,10,5]],"color":"#ff0000"})).unwrap();
    assert_eq!(h.app.session.selection, selected, "applying a look keeps the appearance target selected");
    live::advance(&mut h.app, &h.ctx, 2.0);
    let after = wait_live_complete(&mut h, 2.3);
    assert_ne!(after["hash"], before["hash"], "the selected face's actual image must show its new material");
    assert!(live::picking_current(&h.app));
    outlines(&h.app);
    let document = h.app.session.doc.clone();
    let history = (h.app.session.undo.len(), h.app.session.redo.len());
    // Whole-body and unselected hovered faces preserve the same image response.
    h.app.session.selection = vec![Sel::Body { name: "Subject".into() }];
    outlines(&h.app);
    h.app.session.selection.clear();
    outlines(&h.app);
    h.app.viewport.hover = None;
    h.app.viewport.hover_bodies = vec!["Subject".into()];
    outlines(&h.app);
    h.app.viewport.hover_bodies.clear();
    h.app.session.selection = selected.clone();
    let outline_key = crate::viewport::highlight_key(&h.app);
    h.app.preferences.render_legacy_image_defaults = true;
    assert_ne!(crate::viewport::highlight_key(&h.app), outline_key, "the cached feedback must follow the compatibility preference");
    assert!(!crate::viewport::build_highlight(&h.app).merged().tris.is_empty(), "legacy Render retains its opaque selected face");
    h.app.preferences.render_legacy_image_defaults = false;
    outlines(&h.app);
    assert_eq!(h.app.session.selection, selected);
    assert!(Arc::ptr_eq(&h.app.session.doc, &document));
    assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
    enter(&mut h.app, false).unwrap();
    assert!(!crate::viewport::build_highlight(&h.app).merged().tris.is_empty(), "Design keeps its existing selected face fill");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn live_image_imported_mesh_keeps_selected_and_hovered_face_and_body_feedback() {
    use solidcore_record::Vec3;
    use solidcraft_engine::{
        Sel,
        doc::{Document, FeatureKind, MeshData},
        render::job::{CpuHost, CpuProgressiveOptions},
    };

    // Original synthetic triangle geometry follows the real reference-mesh path;
    // its display outlines must never become selectable modeling edges.
    let mut doc = Document::default();
    doc.add_feature(
        FeatureKind::MeshImport {
            file: "feedback-plane.stl".into(),
            meshes: vec![MeshData {
                name: "Reference".into(),
                positions: vec![-2., -2., 0., 2., -2., 0., 2., 2., 0., -2., 2., 0.].into(),
                triangles: vec![0, 1, 2, 0, 2, 3].into(),
                color: None,
            }],
        },
        Some("Reference"),
    )
    .unwrap();
    let mut h = Harness::new();
    h.app = SolveApp::new(Session::new(doc), crate::Services::default());
    // This context's texture deltas must reach the real GPU renderer, rather than
    // being discarded by the scenario harness's renderer-free frames.
    h.ctx = crate::fonts::test_context();
    h.app.run("solid.box", json!({"length":2,"width":2,"height":2,"corner":[9,-1,0],"operation":"new","body_name":"Solid"})).unwrap();
    h.app.run("solid.box", json!({"length":2,"width":4,"height":1,"corner":[0,-2,0],"operation":"new","body_name":"Occluder"})).unwrap();
    h.app.ui.show_grid = false;
    h.app.ui.ground_shadow = false;
    h.app.set_origin_shown(false);
    h.app.cam.set_view(solidcraft_engine::render::StandardView::Top);
    h.app.fit_view();
    // The real workflow draws Design before entering Render. Keep its shared
    // reference triangles so whole-body feedback uses the actual instance path.
    let design_scene = crate::viewport::build_scene(&h.app);
    let reference_shared = design_scene.instanced.iter().find(|draw| draw.tris.bounds == [[-2.0, -2.0, 0.0], [2.0, 2.0, 0.0]]).unwrap();
    assert_eq!(reference_shared.look, crate::gpu::InstanceLook::Opaque);
    assert_eq!(reference_shared.count(), 1, "Design must draw the reference as a real shared-mesh placement");
    let reference_shared = reference_shared.tris.clone();
    let host = CpuHost::for_platform().unwrap();
    h.app.session.set_render_job_host(Some(host.clone())).unwrap();
    h.app.render_workspace.image_host = Some(host);
    if image_host(&h.app, ImageMethod::CpuRaytrace).is_err() {
        eprintln!("live imported-mesh feedback test requires raytrace-cpu");
        return;
    }
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.raytrace.progressive = Some(CpuProgressiveOptions { samples_per_pixel: 3, seed: 81 });
    h.app.viewport.rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(256., 256.)));
    live::advance(&mut h.app, &h.ctx, 1.0);
    let picture = wait_live_complete(&mut h, 1.3);
    assert_eq!(picture["complete"], true);
    assert!(live::has_picture(&h.app) && live::picking_current(&h.app));

    let world = h.app.session.world_state().unwrap();
    let reference = world.body("Reference").unwrap();
    let mesh = reference.mesh();
    assert!(mesh.display_only_edges && !mesh.edges.is_empty());
    assert!((0..mesh.edges.len()).all(|edge| !mesh.edge_pickable(edge)));
    assert_eq!(reference.body.face_count(), 1);
    let expected_vertices = mesh.triangles.len() * 3;
    let solid = world.body("Solid").unwrap();
    assert!((0..solid.mesh().edges.len()).any(|edge| solid.mesh().edge_pickable(edge)));
    let solid_face = (0..solid.body.face_count()).find(|face| solid.body.planar_face(*face).is_some_and(|plane| plane.normal() == Vec3::Z)).unwrap();
    let document = h.app.session.doc.clone();
    let history = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
    let tokens = crate::theme::Tokens::get();
    let hover_color = [
        tokens.body.r().saturating_add(tokens.hover_face_lift),
        tokens.body.g().saturating_add(tokens.hover_face_lift),
        tokens.body.b().saturating_add(tokens.hover_face_lift),
        255,
    ];

    // Expand actual highlighted placements too: an empty CPU triangle list alone
    // does not mean that a whole-body GPU highlight is missing.
    let filled_vertices = |app: &SolveApp, expected_color: [u8; 4]| {
        let feedback = crate::viewport::build_highlight(app).merged();
        assert!(feedback.overlay.is_empty() && feedback.xray.is_empty(), "feedback must remain depth-tested");
        assert!(feedback.selected_planned.is_empty() && feedback.hover_planned.is_empty());
        assert!(feedback.selected_blocks.is_empty() && feedback.hover_blocks.is_empty() && feedback.recoloured.is_empty());
        let mut tris = feedback.tris.clone();
        for mesh in feedback.selected_instanced.iter().chain(&feedback.hover_instanced) {
            mesh.expand_into(&mut tris);
        }
        let vertices = tris.as_chunks::<{ crate::gpu::TRI_SIZE }>().0;
        for vertex in vertices {
            let color = crate::gpu::PaletteIndex(u32::from_le_bytes(vertex[24..28].try_into().unwrap()));
            assert_eq!(feedback.palette.get(color), Some(expected_color), "target fill keeps its existing theme color");
        }
        vertices.len()
    };
    let target = |app: &mut SolveApp, body: &str, face: usize, case: usize| {
        app.session.selection.clear();
        app.viewport.hover = None;
        app.viewport.hover_bodies.clear();
        app.ui.pick_mode = if case == 3 { solidcraft_engine::PickMode::Bodies } else { solidcraft_engine::PickMode::Faces };
        let point = Vec3::ZERO;
        match case {
            0 => app.session.selection = vec![Sel::Face { body: body.into(), index: face, point }],
            1 => app.session.selection = vec![Sel::Body { name: body.into() }],
            _ => {
                let hit = crate::viewport::Hit::Face { body: body.into(), index: face, point };
                let picked = crate::viewport::candidate(app, std::slice::from_ref(&hit)).unwrap().1;
                assert_eq!(picked, if case == 3 { Sel::Body { name: body.into() } } else { Sel::Face { body: body.into(), index: face, point } });
                app.viewport.hover = Some(hit);
            }
        }
    };
    let mut missing = Vec::new();
    for (case, label) in ["selected face", "selected body", "hovered face", "hovered body"].into_iter().enumerate() {
        let color = match case {
            0 => crate::viewport::c4(tokens.sel_face),
            1 => crate::viewport::c4(tokens.body_selected),
            _ => hover_color,
        };
        target(&mut h.app, "Reference", 0, case);
        let selected = h.app.session.selection.clone();
        h.app.preferences.render_legacy_image_defaults = true;
        assert_eq!(filled_vertices(&h.app, color), expected_vertices, "legacy {label} fixture must show every triangle once");
        h.app.preferences.render_legacy_image_defaults = false;
        assert!(live::has_picture(&h.app));
        if filled_vertices(&h.app, color) != expected_vertices {
            missing.push(label);
        }
        assert_eq!(h.app.session.selection, selected, "painting cannot replace the selected target");

        // B-rep targets retain outline-only feedback over the same live picture.
        target(&mut h.app, "Solid", solid_face, case);
        let outlines = crate::viewport::build_highlight(&h.app).merged();
        assert!(outlines.tris.is_empty() && outlines.selected_instanced.is_empty() && outlines.hover_instanced.is_empty());
        assert!(!outlines.lines.is_empty(), "{label}: real B-rep edges retain live-image outlines");
    }
    let mut missing_pixels = Vec::new();
    if let Some(mut gpu) = crate::gpu::EguiFixture::new(&mut h.app, [256, 256]) {
        let draw = |gpu: &mut crate::gpu::EguiFixture, h: &mut Harness, pointer| {
            let pixels = gpu.frame(&h.ctx, vec![pointer], |ui| show(&mut h.app, ui));
            assert_eq!(
                (gpu.product_highlight_callbacks, gpu.viewport_callbacks),
                (1, 0),
                "every measured live-image frame must emit its feedback callback without a shaded-product redraw"
            );
            pixels
        };
        h.app.session.selection.clear();
        h.app.viewport.hover = None;
        h.app.viewport.hover_bodies.clear();
        draw(&mut gpu, &mut h, egui::Event::PointerGone);
        let clean = draw(&mut gpu, &mut h, egui::Event::PointerGone);
        assert!(live::has_picture(&h.app) && live::picking_current(&h.app), "pixel fixture must paint the actual current live image");
        let projection = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        let visible = projection.to_screen(Vec3::new(-1.0, 0.0, 0.0)).unwrap();
        let covered = projection.to_screen(Vec3::new(1.0, 0.0, 0.0)).unwrap();
        let solid_inside = projection.to_screen(Vec3::new(10.0, 0.0, 2.0)).unwrap();
        let solid_bounds = egui::Rect::from_two_pos(
            projection.to_screen(Vec3::new(9.0, -1.0, 2.0)).unwrap(),
            projection.to_screen(Vec3::new(11.0, 1.0, 2.0)).unwrap(),
        );
        let patch = |pixels: &[u8], at: egui::Pos2| {
            let (x, y) = (at.x.round() as usize, at.y.round() as usize);
            assert!((3..253).contains(&x) && (3..253).contains(&y), "interior patch must stay inside the image: {at:?}");
            (y - 2..=y + 2)
                .flat_map(|row| {
                    (x - 2..=x + 2).flat_map(move |column| pixels[(row * 256 + column) * 4..(row * 256 + column) * 4 + 4].iter().copied())
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(clean, draw(&mut gpu, &mut h, egui::Event::PointerGone), "the unselected photo must be stable before pixel comparisons");
        for (case, label) in ["selected face", "selected body", "hovered face", "hovered body"].into_iter().enumerate() {
            target(&mut h.app, "Reference", 0, case);
            let pointer = if case < 2 { egui::Event::PointerGone } else { egui::Event::PointerMoved(visible) };
            draw(&mut gpu, &mut h, pointer.clone());
            if case == 1 || case == 3 {
                let feedback = crate::viewport::build_highlight(&h.app);
                let instances = if case == 1 { &feedback.selected_instanced } else { &feedback.hover_instanced };
                assert!(
                    instances.iter().any(|draw| Arc::ptr_eq(&draw.tris, &reference_shared) && draw.count() == 1),
                    "{label}: the actual callback must receive the Reference's shared-triangle instance stream"
                );
                assert!(feedback.tris.is_empty(), "{label}: a CPU triangle fallback would miss the native regression");
            }
            let pixels = draw(&mut gpu, &mut h, pointer);
            if case >= 2 {
                let hit = h.app.viewport.hover.as_ref().unwrap();
                assert!(
                    matches!(hit, crate::viewport::Hit::Face { body, .. } if body == "Reference"),
                    "{label}: actual pointer must hit the reference face"
                );
                let picked = crate::viewport::candidate(&h.app, std::slice::from_ref(hit)).unwrap().1;
                assert!(
                    matches!(&picked, Sel::Body { name } if case == 3 && name == "Reference")
                        || matches!(&picked, Sel::Face { body, index: 0, .. } if case == 2 && body == "Reference"),
                    "the real pointer must use the requested face/body filter: {picked:?}"
                );
            }
            if patch(&pixels, visible) == patch(&clean, visible) {
                missing_pixels.push(label);
            }
            assert_eq!(patch(&pixels, covered), patch(&clean, covered), "{label}: the front body must occlude reference feedback");
            assert_eq!(patch(&pixels, solid_inside), patch(&clean, solid_inside), "{label}: unrelated product pixels must retain the live photo");

            target(&mut h.app, "Solid", solid_face, case);
            let pointer = if case < 2 { egui::Event::PointerGone } else { egui::Event::PointerMoved(solid_inside) };
            draw(&mut gpu, &mut h, pointer.clone());
            let outlines = draw(&mut gpu, &mut h, pointer);
            assert_eq!(
                patch(&outlines, solid_inside),
                patch(&clean, solid_inside),
                "{label}: ordinary B-rep feedback must not fill its photo interior"
            );
            assert_eq!(patch(&outlines, visible), patch(&clean, visible), "{label}: B-rep feedback must leave the reference photo unchanged");
            let changed_edges = clean
                .as_chunks::<4>()
                .0
                .iter()
                .zip(outlines.as_chunks::<4>().0)
                .enumerate()
                .filter(|(index, (before, after))| {
                    let point = egui::pos2((index % 256) as f32 + 0.5, (index / 256) as f32 + 0.5);
                    before != after && solid_bounds.expand(3.0).contains(point) && !solid_bounds.shrink(4.0).contains(point)
                })
                .count();
            assert!(changed_edges > 0, "{label}: normal B-rep outlines must reach actual displayed pixels");
        }
    }
    target(&mut h.app, "Reference", 0, 0);
    enter(&mut h.app, false).unwrap();
    assert_eq!(filled_vertices(&h.app, crate::viewport::c4(tokens.sel_face)), expected_vertices, "Design retains its selected reference face fill");
    assert!((0..mesh.edges.len()).all(|edge| !mesh.edge_pickable(edge)), "feedback must not promote display-only edges to modeling picks");
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!((h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision), history);
    assert!(h.app.session.render_retained_results().unwrap().is_empty());
    assert!(missing.is_empty(), "live-image reference mesh lost target feedback: {missing:?}");
    assert!(missing_pixels.is_empty(), "live-image reference mesh feedback never reached displayed pixels: {missing_pixels:?}");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn live_capture_aspect_matches_product_navigation_and_frame_without_changing_output_size() {
    let Some(mut h) = live_cpu_fixture() else { return };
    let output = [h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height];
    for stage in [Stage::Look, Stage::Frame] {
        select_stage(&mut h.app, stage.key());
        for dimensions in [egui::vec2(1200.0, 600.0), egui::vec2(900.0, 1400.0)] {
            let viewport = egui::Rect::from_min_size(egui::pos2(20.0, 30.0), dimensions);
            h.app.viewport.rect = Some(viewport);
            live::advance(&mut h.app, &h.ctx, 20.0);
            let state = live::inspect(&h.app);
            assert!(state["error"].is_null(), "{state}");
            let request = &state["request"];
            let width = request["settings"]["width"].as_u64().unwrap();
            let height = request["settings"]["height"].as_u64().unwrap();
            assert!(width <= 512 && height <= 512);
            let canvas = shot::render_rect(&h.app, viewport);
            let projected = live::projection_rect(&h.app, canvas);
            assert!((f64::from(projected.width() / projected.height()) - width as f64 / height as f64).abs() < 1e-5);
            assert!(canvas.contains_rect(projected));
            assert_eq!(request["settings"]["camera"], json!(h.app.cam));
            let centre = crate::viewport::projection(&h.app, projected).to_screen(h.app.cam.target).unwrap();
            assert!(centre.distance(projected.center()) < 0.01);
            assert_eq!([h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height], output);
        }
    }
    h.app.render_workspace.view.raytrace.progressive.as_mut().unwrap().samples_per_pixel = u32::MAX;
    live::advance(&mut h.app, &h.ctx, 20.0);
    assert!(!live::inspect(&h.app)["error"].is_null());
    for width in [360.0, 600.0] {
        let bar = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(width, crate::ui_kit::StageBar::HEIGHT));
        let mut painted = h.ctx.run_ui(
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 300.0))), ..Default::default() },
            |ui| crate::render_ui::stage_bar(&mut h.app, ui, bar),
        );
        painted.textures_delta.clear();
        let pause = crate::scenario::rect_of("render:live:pause").unwrap();
        let note = crate::scenario::rect_of("render:live:status").unwrap();
        assert!(bar.contains_rect(pause), "Pause remains visible even with a long refusal in a narrow bar");
        assert!(pause.right() < note.left());
        assert!(note.width() <= crate::theme::Tokens::get().control_height * 10.0);
        assert!(bar.contains_rect(note));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn final_image_waits_for_live_capture_cleanup_and_keeps_the_click_time_request() {
    let Some(mut h) = live_cpu_fixture() else { return };
    h.app.render_workspace.view.draft.width = 7;
    h.app.render_workspace.view.draft.height = 5;
    live::advance(&mut h.app, &h.ctx, 1.0);
    live::advance(&mut h.app, &h.ctx, 1.3);
    assert_eq!(live::inspect(&h.app)["pending"], true);
    let expected = image_request(&h.app).unwrap();
    ribbon::perform_job(&mut h.app, "render.start").unwrap();
    assert!(live::final_pending(&h.app));
    assert!(ribbon::busy(&h.app));
    assert!(ribbon::perform_job(&mut h.app, "render.start").is_err(), "waiting final requests cannot be duplicated");
    assert!(h.app.render_workspace.view.job.is_none());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while h.app.render_workspace.view.job.is_none() {
        live::advance(&mut h.app, &h.ctx, 1.4);
        assert!(std::time::Instant::now() < deadline, "final request did not leave live cleanup: {}", live::inspect(&h.app));
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(!live::final_pending(&h.app));
    assert_eq!(live::inspect(&h.app)["draining"], 0);
    let id = h.app.render_workspace.view.job.unwrap();
    let handle = h.app.session.render_job_handle(id).unwrap();
    let status = handle.status().unwrap();
    assert_eq!((status.request.width, status.request.height), (7, 5));
    let sampling = status.request.raytrace.as_ref().unwrap().progressive.unwrap();
    assert_eq!(json!(sampling), expected["settings"]["job"]["options"]["progressive"]);
    assert_eq!(json!(status.request.camera), expected["settings"]["camera"]);
    assert!(h.app.render_workspace.view.result && h.app.render_workspace.view.follow_job);
    let _ = handle.cancel();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn waiting_final_can_be_cancelled_and_refuses_changed_source_instead_of_rendering_a_new_scene() {
    for changed in [false, true] {
        let Some(mut h) = live_cpu_fixture() else { return };
        h.app.render_workspace.view.draft.width = 8;
        h.app.render_workspace.view.draft.height = 8;
        live::advance(&mut h.app, &h.ctx, 1.0);
        live::advance(&mut h.app, &h.ctx, 1.3);
        ribbon::perform_job(&mut h.app, "render.start").unwrap();
        assert!(live::final_pending(&h.app));
        if changed {
            h.app.run("appearance.assign", json!({"bodies":["Subject"],"color":"#204080"})).unwrap();
        } else {
            ribbon::perform_job(&mut h.app, "render.cancel").unwrap();
            assert!(!live::final_pending(&h.app));
        }
        h.app.run("view.render_live", json!({"paused":true})).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            live::advance(&mut h.app, &h.ctx, 1.4);
            let state = live::inspect(&h.app);
            if state["pending"] == false && state["draining"] == 0 && !live::final_pending(&h.app) {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "waiting final did not drain: {state}");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(h.app.render_workspace.view.job.is_none());
        assert!(h.app.session.render_retained_results().unwrap().is_empty());
        assert_eq!(h.app.session.render_job_host().unwrap().admitted_bytes(), 0);
        if changed {
            assert!(h.app.status.as_ref().is_some_and(|status| status.0 == h.app.tr("render.live.final_changed")));
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fixed_cpu_live_sampling_is_a_separate_progressive_copy_and_inspection_does_not_launch_it() {
    let Some(mut h) = live_cpu_fixture() else { return };
    h.app.render_workspace.view.raytrace.progressive = None;
    h.app.render_workspace.view.raytrace.primary_samples = 4;
    let document = h.app.session.doc.clone();
    let before = setups::draft(&h.app).unwrap();
    let initial = inspect_state(&h.app);
    assert_eq!(initial["live"]["pending"], false);
    assert_eq!(initial, inspect_state(&h.app), "inspection only reads existing metadata");
    assert_eq!(h.app.session.render_job_host().unwrap().admitted_bytes(), 0);
    live::advance(&mut h.app, &h.ctx, 1.0);
    let request = live::inspect(&h.app)["request"].clone();
    assert_eq!(request["settings"]["job"]["options"]["progressive"]["samples_per_pixel"], 16);
    assert!(request["settings"]["job"]["options"]["primary_samples"].is_null() || request["settings"]["job"]["options"]["primary_samples"] == 1);
    assert_eq!(setups::draft(&h.app).unwrap(), before);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert!(h.app.render_workspace.view.raytrace.progressive.is_none());
    assert_eq!(h.app.render_workspace.view.raytrace.primary_samples, 4);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fixed_cpu_live_reflection_attempts_use_the_film_without_changing_final_sampling() {
    use solidcraft_engine::render::job::ReflectionMode;
    let Some(mut h) = live_cpu_fixture() else { return };
    h.app.render_workspace.view.raytrace.progressive = None;
    h.app.render_workspace.view.raytrace.primary_samples = 4;
    cpu::set_reflection(&mut h.app, "Subject", Some(ReflectionMode::Mirror)).unwrap();
    let reflections = h.app.render_workspace.view.raytrace.reflections.as_mut().unwrap();
    reflections.samples = 4;
    reflections.max_bounces = 1;
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    live::advance(&mut h.app, &h.ctx, 1.0);
    let state = live::inspect(&h.app);
    assert!(state["error"].is_null(), "{state}");
    let options = &state["request"]["raytrace"];
    assert_eq!(options["progressive"]["samples_per_pixel"], 16);
    assert_eq!(options["reflections"]["samples"], 1);
    assert_eq!(options["primary_samples"].as_u64().unwrap_or(1), 1);
    let pixels = wait_live_complete(&mut h, 1.3);
    assert_eq!(pixels["samples"], 16);
    assert!(pixels["hash"].as_str().is_some_and(|hash| !hash.is_empty()));
    assert!(h.app.render_workspace.view.raytrace.progressive.is_none());
    assert_eq!(h.app.render_workspace.view.raytrace.primary_samples, 4);
    assert_eq!(h.app.render_workspace.view.raytrace.reflections.as_ref().unwrap().samples, 4);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(h.app.session.doc.render.setups.is_empty());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn new_document_final_keeps_the_old_live_capture_cleanup_barrier() {
    let Some(mut h) = live_cpu_fixture() else { return };
    live::advance(&mut h.app, &h.ctx, 1.0);
    live::advance(&mut h.app, &h.ctx, 1.3);
    assert_eq!(live::inspect(&h.app)["pending"], true);
    let host = h.app.session.render_job_host().unwrap().clone();
    h.app.session = Session::default();
    h.app.session.set_render_job_host(Some(host)).unwrap();
    h.app.run("solid.box", json!({"length":4,"width":5,"height":6,"body_name":"New document"})).unwrap();
    reset_document(&mut h.app);
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.draft.width = 8;
    h.app.render_workspace.view.draft.height = 8;
    ribbon::perform_job(&mut h.app, "render.start").unwrap();
    assert!(live::final_pending(&h.app), "reset must retain a late old-document capture until its leases are released");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while h.app.render_workspace.view.job.is_none() {
        live::advance(&mut h.app, &h.ctx, 1.4);
        assert!(std::time::Instant::now() < deadline, "new-document final did not drain: {}", live::inspect(&h.app));
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(live::inspect(&h.app)["draining"], 0);
    let id = h.app.render_workspace.view.job.unwrap();
    let handle = h.app.session.render_job_handle(id).unwrap();
    assert_eq!(handle.status().unwrap().scene_generation, h.app.session.revision);
    let _ = handle.cancel();
}

#[test]
fn cpu_presentation_saves_reopens_and_reaches_runtime_bindings_with_one_exposure() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    use solidcraft_engine::render::job::{
        OutputOptions, ReflectionOptions, ToneMap,
        finishing::{Backdrop, BackdropMode, Finishing},
        photographic::Photographic,
    };
    let mut h = Harness::new();
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    h.app.session.set_render_job_host(None).unwrap();
    h.app.render_workspace.view.name = "CPU presentation".into();
    h.app.render_workspace.view.draft.width = 64;
    h.app.render_workspace.view.draft.height = 32;
    h.app.render_workspace.view.draft.exposure_ev = 1.5;
    h.app.render_workspace.view.studio.tone_map = ToneMap::AcesFitted;
    h.app.render_workspace.view.studio.finishing =
        Finishing { rgb_gain: [1.3, 0.8, 1.1], photographic: Photographic { grain: 0.03, seed: 42, ..Default::default() }, ..Default::default() };
    h.app.render_workspace.view.studio.backdrop_enabled = true;
    h.app.render_workspace.view.studio.backdrop = Backdrop { mode: BackdropMode::Radial, ..Default::default() };
    h.app.render_workspace.view.studio.linear_exr = true;
    let expected = setups::snapshot(&h.app).unwrap();
    let Some(RenderJob::CpuRaytrace { options, .. }) = &expected.job else { panic!("CPU setup expected") };
    let output = options.output.as_deref().copied().unwrap();
    assert_eq!(output.tone_map, ToneMap::AcesFitted);
    assert!(output.linear_exr);
    assert!(studio::cpu_output(&h.app.render_workspace.view).is_some());
    setups::save(&mut h.app).unwrap();
    h.app.render_workspace.view.studio = studio::View::default();
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    assert_eq!(image_request(&h.app).unwrap(), json!({"settings":expected}));
    let reopened = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
    assert_eq!(reopened.render.setups["CPU presentation"], expected);
    let container = solidcraft_engine::io::write_design_container(&h.app.session.doc).unwrap();
    let reopened = solidcraft_engine::io::read_design(&container).unwrap();
    assert_eq!(reopened.render.setups["CPU presentation"], expected);
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    // An empty temporary reflection block selects the runtime route without a saved body binding.
    h.app.render_workspace.view.raytrace.reflections = Some(ReflectionOptions { bodies: Vec::new(), samples: 1, max_bounces: 1 });
    let runtime = image_request(&h.app).unwrap();
    assert_eq!(runtime["integrator"], "raytrace");
    assert_eq!(runtime["raytrace"]["output"], json!(output));
    assert_eq!(runtime["raytrace"]["exposure_ev"], 1.5);
    assert_eq!(runtime["settings"]["exposure_ev"], 0.0);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    h.app.render_workspace.view.raytrace.reflections = None;
    // An explicit saved identity object also survives an unedited load/snapshot.
    let mut identity = expected;
    if let Some(RenderJob::CpuRaytrace { options, .. }) = &mut identity.job {
        options.output = Some(Box::new(OutputOptions::default()));
    }
    h.app.run("render.settings.set", json!({"name":"CPU presentation","set":identity})).unwrap();
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), identity);
}

#[test]
fn first_cpu_effect_edit_preserves_visible_baseline_and_undoes_once() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2200}));
    enter(&mut h.app, true).unwrap();
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    h.app.render_workspace.view.name = "First CPU effect".into();
    h.app.render_workspace.view.draft.width = 64;
    h.app.render_workspace.view.draft.height = 32;
    h.app.render_workspace.view.draft.exposure_ev = 1.5;
    h.app.render_workspace.view.draft.background = [3, 7, 11];
    h.app.run("view.render_select", json!({"kind":"effects"})).unwrap();
    h.frames(3);
    assert!(!h.app.session.doc.render.setups.contains_key("First CPU effect"));
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":"render:bloom-enable"}})).unwrap();
    h.frames(3);
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::CpuRaytrace);
    let saved = &h.app.session.doc.render.setups["First CPU effect"];
    assert_eq!((saved.width, saved.height, saved.exposure_ev, saved.background), (64, 32, 1.5, [3, 7, 11]));
    let Some(RenderJob::CpuRaytrace { options, .. }) = &saved.job else { panic!("CPU output must not select a GPU job") };
    assert!(options.output.as_ref().unwrap().finishing.bloom.is_some());
    assert_eq!(h.app.session.undo.len(), undo + 1);
    h.app.run("edit.undo", json!({})).unwrap();
    assert!(!h.app.session.doc.render.setups.contains_key("First CPU effect"));
    assert!(h.app.render_workspace.view.job.is_none());
}

fn cpu_output_hint_frame(h: &mut Harness, events: Vec<egui::Event>) -> String {
    let mut output = h.ctx.run_ui(
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(308.0, 2000.0))), events, ..Default::default() },
        |ui| inspector::output_editor(&mut h.app, ui),
    );
    let text = crate::test_support::text_shapes(&output).map(|shape| shape.galley.job.text.clone()).collect::<Vec<_>>().join("\n");
    output.textures_delta.clear();
    text
}

#[test]
fn cpu_alpha_help_and_checkbox_are_available_without_mutating_intent_on_paint() {
    let mut h = Harness::new();
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let text = cpu_output_hint_frame(&mut h, vec![]);
    assert!(text.contains(h.app.tr("render.cpu_transparent_help")));
    assert!(!h.app.render_workspace.view.transparent);
    for expected in [true, false] {
        let at = crate::scenario::rect_of("render:transparent-background").unwrap().center();
        for pressed in [true, false] {
            cpu_output_hint_frame(
                &mut h,
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() },
                ],
            );
        }
        assert_eq!(h.app.render_workspace.view.transparent, expected, "CPU alpha remains an explicit reversible choice");
        assert_eq!(h.app.render_workspace.view.method, ImageMethod::CpuRaytrace);
    }
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
fn cpu_gradient_backdrop_and_retained_master_controls_preserve_the_cpu_method() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    use solidcraft_engine::render::job::finishing::{Backdrop, BackdropMode};
    let mut h = Harness::new();
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    h.app.render_workspace.view.name = "CPU backdrop".into();
    let gradient = Backdrop { mode: BackdropMode::Radial, ..Default::default() };
    environment::commit_gradient(&mut h.app, true, gradient).unwrap();
    let saved = setups::snapshot(&h.app).unwrap();
    let Some(RenderJob::CpuRaytrace { options, .. }) = &saved.job else { panic!("CPU job expected") };
    assert_eq!(options.output.as_ref().unwrap().backdrop, Some(gradient));
    environment::commit_backdrop(&mut h.app, [21, 37, 55], false).unwrap();
    let cleared = setups::snapshot(&h.app).unwrap();
    let Some(RenderJob::CpuRaytrace { options, .. }) = &cleared.job else { panic!("CPU job expected") };
    assert!(options.output.as_ref().unwrap().backdrop.is_none());
    assert_eq!(cleared.background, [21, 37, 55]);
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::CpuRaytrace);
    assert!(h.app.render_workspace.view.job.is_none());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_cpu_output_ui_capture_keeps_common_effects_and_exports_its_actual_raw_master() {
    use solidcraft_engine::render::job::{JobId, Phase};
    let Some(mut h) = live_cpu_fixture() else { return };
    h.app.render_workspace.view.live_paused = true;
    h.call("ui.resize", json!({"width":1600,"height":2200}));
    h.app
        .run(
            "render.settings.set",
            json!({"name":"CPU output capture","set":{"camera":h.app.cam,"width":8,"height":8,"exposure_ev":1.5,
        "job":{"method":"cpu_raytrace","options":{"progressive":{"samples_per_pixel":3,"seed":81},
            "output":{"tone_map":"aces_fitted","finishing":{"rgb_gain":[1.2,0.8,1.1],"photographic":{"grain":0.03,"seed":42},
                "bloom":{"strength":0.2,"threshold":0.5,"radius":1}},"backdrop":{"mode":"radial"},"linear_exr":true}}}}}),
        )
        .unwrap();
    h.app.render_workspace.view.name = "CPU output capture".into();
    setups::load(&mut h.app);
    h.app.run("view.render_select", json!({"kind":"output"})).unwrap();
    h.frames(3);
    let checked = image_params(&h.app).unwrap();
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":"render:stage:render.start"}})).unwrap();
    let job: JobId = h.app.render_workspace.view.job.expect("the real UI Render action must capture the CPU setup");
    let handle = h.app.session.render_job_handle(job).unwrap();
    let status = handle.wait_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(status.progress.phase, Phase::Completed, "{:?}", status.error);
    assert!(status.request.raytrace.as_ref().unwrap().output.as_ref().unwrap().linear_exr);
    assert_eq!(json!(status.request.raytrace.as_ref().unwrap().output), checked["settings"]["job"]["options"]["output"]);
    h.frames(3);
    assert_eq!(h.app.render_workspace.view.completed, Some(job));
    assert!(studio::linear_export_available(&h.app));
    linear_format_choices::choose(&mut h, "render:format-exr");
    assert!(h.app.render_workspace.view.studio.export_exr);
    let frame = handle.frame().unwrap().unwrap();
    let path = h.dir.join("actual-cpu-output.exr");
    h.app.session.execute("render.export", &json!({"job":job,"path":path,"format":"exr"})).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), frame.linear_exr().unwrap());
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
fn cpu_output_identity_presence_tracks_external_edits_undo_and_local_saved_changes() {
    use solidcraft_engine::render::job::OutputOptions;
    let mut h = Harness::new();
    let camera = h.app.cam;
    h.app
        .run("render.settings.set", json!({"name":"CPU identity","set":{"camera":camera,"width":64,"height":32,"job":{"method":"cpu_raytrace"}}}))
        .unwrap();
    h.app.render_workspace.view.name = "CPU identity".into();
    setups::load(&mut h.app);
    for output in [json!(OutputOptions::default()), serde_json::Value::Null] {
        let before = setups::snapshot(&h.app).unwrap();
        h.app.run("render.settings.set", json!({"name":"CPU identity","set":{"job":{"method":"cpu_raytrace","options":{"output":output}}}})).unwrap();
        setups::sync_revision(&mut h.app);
        assert_eq!(setups::snapshot(&h.app).unwrap(), h.app.session.doc.render.setups["CPU identity"]);
        assert_eq!(h.app.cam, camera);
        assert!(!h.app.render_workspace.view.dirty);
        h.app.run("edit.undo", json!({})).unwrap();
        setups::sync_revision(&mut h.app);
        assert_eq!(setups::snapshot(&h.app).unwrap(), before);
        assert!(!h.app.render_workspace.view.dirty);
        h.app.run("edit.redo", json!({})).unwrap();
        setups::sync_revision(&mut h.app);
        assert_eq!(setups::snapshot(&h.app).unwrap(), h.app.session.doc.render.setups["CPU identity"]);
    }
    // A local save must refresh the marker too; subsequent removal is not a local override.
    h.app.render_workspace.view.raytrace.output = Some(Box::new(OutputOptions::default()));
    h.app.render_workspace.view.studio.finishing.rgb_gain = [1.2, 0.8, 1.1];
    setups::save(&mut h.app).unwrap();
    let before = setups::snapshot(&h.app).unwrap();
    h.app.run("render.settings.set", json!({"name":"CPU identity","set":{"job":{"method":"cpu_raytrace","options":{"output":null}}}})).unwrap();
    setups::sync_revision(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), h.app.session.doc.render.setups["CPU identity"]);
    assert!(!h.app.render_workspace.view.dirty);
    h.app.run("edit.undo", json!({})).unwrap();
    setups::sync_revision(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), before);
    assert!(!h.app.render_workspace.view.dirty);
}

#[test]
fn cpu_gradient_and_colour_edits_preserve_the_saved_shot_camera_and_live_navigation() {
    use solidcraft_engine::render::job::finishing::{Backdrop, BackdropMode};
    let mut h = Harness::new();
    let saved_camera = Camera { yaw: 0.2, distance: 80.0, ..Default::default() };
    h.app
        .run(
            "render.settings.set",
            json!({"name":"CPU camera","set":{"camera":saved_camera,"width":64,"height":32,"exposure_ev":1.5,
        "job":{"method":"cpu_raytrace","options":{"progressive":{"samples_per_pixel":3,"seed":81}}}}}),
        )
        .unwrap();
    h.app.render_workspace.view.name = "CPU camera".into();
    setups::load(&mut h.app);
    let live_camera = Camera { yaw: 1.3, distance: 120.0, ..saved_camera };
    h.app.cam = live_camera;
    let undo = h.app.session.undo.len();
    environment::commit_gradient(&mut h.app, true, Backdrop { mode: BackdropMode::Radial, ..Default::default() }).unwrap();
    let gradient = h.app.session.doc.render.setups["CPU camera"].clone();
    assert_eq!(gradient.camera, saved_camera);
    assert_eq!(h.app.cam, live_camera);
    assert_eq!((gradient.width, gradient.height, gradient.exposure_ev), (64, 32, 1.5));
    assert_eq!(h.app.session.undo.len(), undo + 1);
    environment::commit_backdrop(&mut h.app, [21, 37, 55], false).unwrap();
    assert_eq!(h.app.session.doc.render.setups["CPU camera"].camera, saved_camera);
    assert_eq!(h.app.cam, live_camera);
    assert_eq!(h.app.session.undo.len(), undo + 2);
    h.app.run("edit.undo", json!({})).unwrap();
    setups::sync_revision(&mut h.app);
    assert_eq!(h.app.session.doc.render.setups["CPU camera"], gradient);
    assert_eq!(h.app.cam, live_camera);
}

#[test]
fn gpu_sample_editor_keeps_exact_requested_pass_count_and_saved_intent() {
    for (legacy, entered, expected) in [(false, 17, 17), (false, 31, 31), (true, 17, 32)] {
        let mut h = Harness::new();
        h.app.preferences.render_legacy_image_defaults = legacy;
        h.call("ui.resize", json!({"width":1600,"height":1600}));
        enter(&mut h.app, true).unwrap();
        h.app.render_workspace.view.live_paused = true;
        select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
        h.app.render_workspace.view.draft.width = 64;
        h.app.render_workspace.view.draft.height = 32;
        h.app.render_workspace.view.samples = 64;
        h.app.render_workspace.view.seed = 71;
        h.app.render_workspace.view.max_depth = 7;
        h.app.render_workspace.view.draft.exposure_ev = 1.25;
        setups::save(&mut h.app).unwrap();
        inspector::select_editor(&mut h.app, inspector::Editor::Sampling);
        h.frames(3);
        if crate::scenario::rect_of("render:gpu-samples").is_none() {
            h.step(&json!({"click":{"handle":"dialog:section:Path tracing"}})).unwrap();
            h.frames(3);
        }
        let field = crate::scenario::rect_of("render:gpu-samples").unwrap();
        let at = egui::pos2(field.right() - 10.0, field.center().y);
        h.step(&json!({"click":[at.x,at.y],"double":true})).unwrap();
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        h.step(&json!({"text":entered.to_string()})).unwrap();
        h.step(&json!({"key":"Enter"})).unwrap();
        h.frames(3);
        assert_eq!(h.app.render_workspace.view.samples, expected, "legacy={legacy}, entered={entered}");
        let request = image_request(&h.app).unwrap();
        let settings = &request["settings"];
        let options = &settings["job"]["options"];
        assert_eq!(options["samples"], expected);
        assert_eq!(options["seed"], 71);
        assert_eq!(options["max_depth"], 7);
        assert_eq!(settings["exposure_ev"], 1.25);
        assert_eq!((settings["width"].as_u64(), settings["height"].as_u64()), (Some(64), Some(32)));
        assert!(h.app.render_workspace.view.job.is_none(), "editing a pass count does not start a final render");
        setups::save(&mut h.app).unwrap();
        setups::load(&mut h.app);
        assert_eq!(h.app.render_workspace.view.samples, expected);
        assert_eq!(image_request(&h.app).unwrap(), request);
    }
}

fn frame_lens_edit_keeps_job_draft_unsaved(method: &str) {
    use solidcraft_engine::doc::render_settings::RenderJob;
    use solidcraft_engine::render::job::{CpuProgressiveOptions, ThinLens};
    let other_method = if method == "cpu_raytrace" { "gpu_path" } else { "cpu_raytrace" };
    for draft_method in [method, other_method] {
        for moved_camera in [false, true] {
            let mut h = Harness::new();
            let saved_camera = Camera { fov: 0.7, distance: 20.0, ..Default::default() };
            let options = if method == "cpu_raytrace" {
                json!({"progressive":{"samples_per_pixel":8,"seed":81}})
            } else {
                json!({"samples":8,"max_depth":4,"seed":81})
            };
            h.app
                .run(
                    "render.settings.set",
                    json!({"name":"Lens isolation","set":{"width":64,"height":32,"camera":saved_camera,
                    "job":{"method":method,"options":options}}}),
                )
                .unwrap();
            h.app.render_workspace.view.name = "Lens isolation".into();
            h.app.cam = if moved_camera { Camera { distance: 30.0, roll: 0.3, ..saved_camera } } else { saved_camera };
            setups::load(&mut h.app);
            let draft_kind = if draft_method == "cpu_raytrace" { ImageMethod::CpuRaytrace } else { ImageMethod::GpuRaytrace };
            if draft_method != method {
                // The Scene inspector changes the active method without saving it. Opening
                // Frame and editing DOF must not commit that separate renderer choice.
                select_image_method(&mut h.app, draft_kind);
            }
            if draft_method == "cpu_raytrace" {
                h.app.render_workspace.view.raytrace.progressive = Some(CpuProgressiveOptions { samples_per_pixel: 16, seed: 93 });
                h.app.render_workspace.view.raytrace.path = Some(serde_json::from_value(json!({"max_depth":3})).unwrap());
                h.app.render_workspace.view.raytrace.lights = Some(Vec::new());
            } else {
                h.app.render_workspace.view.samples = 17;
                h.app.render_workspace.view.max_depth = 5;
                h.app.render_workspace.view.seed = 93;
            }
            setups::refresh_dirty(&mut h.app);
            assert!(h.app.render_workspace.view.dirty);
            let live_camera = h.app.cam;
            let before = h.app.session.doc.clone();
            let model = h.app.session.model.state();
            let undo = h.app.session.undo.len();
            let saved = before.render.setups["Lens isolation"].clone();
            let mut expected_saved = saved.clone();
            let mut expected_draft = setups::snapshot(&h.app).unwrap();
            assert_ne!(expected_draft.job, saved.job, "the fixture must have an independent unsaved job draft");
            let lens = ThinLens { aperture_radius_mm: 0.5, focus_distance_mm: 20.0 };
            for setup in [&mut expected_saved, &mut expected_draft] {
                match setup.job.as_mut().unwrap() {
                    RenderJob::CpuRaytrace { options, .. } => options.lens = Some(lens),
                    RenderJob::GpuPath { options, .. } => options.lens = Some(lens),
                    RenderJob::Raster { .. } => panic!("the fixture must use an image job"),
                }
            }
            shot::set_lens(&mut h.app, "Lens isolation", Some(lens)).unwrap();
            assert!(h.app.render_workspace.view.dirty, "{method} saved/{draft_method} draft: a lens edit must leave unrelated job intent unsaved");
            assert_eq!(h.app.session.doc.render.setups["Lens isolation"], expected_saved, "{method} saved/{draft_method} draft: save only the lens");
            assert_eq!(
                setups::draft(&h.app).unwrap(),
                expected_draft,
                "{method} saved/{draft_method} draft: preserve the independent active job draft"
            );
            assert_eq!(h.app.render_workspace.view.method, draft_kind);
            assert_eq!(h.app.cam, live_camera);
            assert_eq!(h.app.session.doc.render.setups["Lens isolation"].camera, saved_camera);
            assert!(Arc::ptr_eq(&model, &h.app.session.model.state()));
            assert_eq!(h.app.session.undo.len(), undo + 1);
            assert!(h.app.render_workspace.view.job.is_none());
            h.app.run("edit.undo", json!({})).unwrap();
            assert_eq!(h.app.session.doc, before);
            assert_eq!(h.app.cam, live_camera);
            h.app.run("edit.redo", json!({})).unwrap();
            assert_eq!(h.app.session.doc.render.setups["Lens isolation"], expected_saved);
            assert_eq!(h.app.cam, live_camera);
        }
    }
}

#[test]
fn cpu_frame_lens_edit_keeps_unrelated_job_draft_unsaved() {
    frame_lens_edit_keeps_job_draft_unsaved("cpu_raytrace");
}

#[test]
fn gpu_frame_lens_edit_keeps_unrelated_job_draft_unsaved() {
    frame_lens_edit_keeps_job_draft_unsaved("gpu_path");
}

#[test]
fn cpu_frame_lens_edits_roundtrip_offline_undo_once_and_preserve_saved_and_live_cameras() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    use solidcraft_engine::render::job::ThinLens;
    for legacy in [false, true] {
        let mut h = Harness::new();
        h.app.preferences.render_legacy_image_defaults = legacy;
        let saved_camera = Camera { fov: 0.7, distance: 20.0, ..Default::default() };
        h.app
            .run(
                "render.settings.set",
                json!({"name":"Focus","set":{"width":64,"height":32,"camera":saved_camera,"exposure_ev":1.5,
            "job":{"method":"cpu_raytrace","options":{"progressive":{"samples_per_pixel":8,"seed":81},"lights":[],
                "output":{"tone_map":"aces_fitted","linear_exr":true}}}}}),
            )
            .unwrap();
        h.app.render_workspace.view.name = "Focus".into();
        h.app.cam = Camera { distance: 30.0, roll: 0.3, ..saved_camera };
        setups::load(&mut h.app);
        let live_camera = h.app.cam;
        let before = h.app.session.doc.clone();
        let history = (h.app.session.undo.len(), h.app.session.redo.len());
        let saved = h.app.session.doc.render.setups["Focus"].clone();
        let mut painted = h.ctx.run_ui(
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 2400.0))), ..Default::default() },
            |ui| shot::editor(&mut h.app, ui, "Focus"),
        );
        painted.textures_delta.clear();
        assert!(crate::scenario::rect_of("render:shot:dof").is_some(), "CPU uses the existing Frame lens editor");
        assert!(Arc::ptr_eq(&before, &h.app.session.doc));
        assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
        assert!(h.app.render_workspace.view.job.is_none());
        let lens = ThinLens { aperture_radius_mm: 0.5, focus_distance_mm: 20.0 };
        shot::set_lens(&mut h.app, "Focus", Some(lens)).unwrap();
        assert_eq!(h.app.session.undo.len(), history.0 + 1);
        let changed = &h.app.session.doc.render.setups["Focus"];
        assert_eq!(changed.camera, saved_camera);
        assert_eq!(h.app.cam, live_camera);
        assert_eq!((changed.width, changed.height, changed.exposure_ev), (64, 32, 1.5));
        let RenderJob::CpuRaytrace { options, .. } = changed.job.as_ref().unwrap() else { panic!("CPU method changed") };
        assert_eq!(options.lens, Some(lens));
        assert_eq!(options.progressive.unwrap().samples_per_pixel, 8);
        assert!(options.output.as_ref().unwrap().linear_exr);
        assert_eq!(options.output.as_ref().unwrap().tone_map, solidcraft_engine::render::job::ToneMap::AcesFitted);
        assert_eq!(h.app.render_workspace.view.raytrace.lens, Some(lens));
        let mut runtime = json!({"settings":{"camera":live_camera}});
        cpu::params(&h.app, &mut runtime).unwrap();
        assert_eq!(runtime["raytrace"]["lens"], json!(lens));
        assert_eq!(runtime["settings"]["camera"], json!(live_camera));
        assert_eq!(h.app.session.undo.len(), history.0 + 1, "building a lens request is read-only");
        let reopened = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
        assert_eq!(&reopened.render.setups["Focus"], changed);
        h.app.run("edit.undo", json!({})).unwrap();
        setups::sync_revision(&mut h.app);
        assert_eq!(h.app.session.doc.render.setups["Focus"], saved);
        assert!(!h.app.render_workspace.view.studio.lens_enabled);
        assert_eq!(h.app.cam, live_camera);
        h.app.run("edit.redo", json!({})).unwrap();
        setups::sync_revision(&mut h.app);
        assert!(h.app.render_workspace.view.studio.lens_enabled);
        assert_eq!(h.app.render_workspace.view.studio.lens, lens);
        assert_eq!(h.app.cam, live_camera);
        assert!(h.app.render_workspace.view.job.is_none());
        h.app.cam.fov = 0.0;
        shot::set_lens(&mut h.app, "Focus", None).unwrap();
        assert_eq!(h.app.session.doc.render.setups["Focus"].camera, saved_camera);
        assert_eq!(h.app.cam.fov, 0.0, "removing an active lens never adopts navigation");
        let RenderJob::CpuRaytrace { options, .. } = h.app.session.doc.render.setups["Focus"].job.as_ref().unwrap() else {
            panic!("CPU method changed")
        };
        assert!(options.lens.is_none());
        assert_eq!(options.progressive.unwrap().samples_per_pixel, 8);
    }
}

#[test]
fn cpu_fixed_lens_intent_requires_explicit_progressive_adoption_and_zero_can_be_removed() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    use solidcraft_engine::render::job::ThinLens;
    let mut h = Harness::new();
    let camera = Camera { fov: 0.7, distance: 20.0, ..Default::default() };
    h.app
        .run(
            "render.settings.set",
            json!({"name":"Fixed","set":{"width":64,"height":32,"camera":camera,
        "job":{"method":"cpu_raytrace","options":{"primary_samples":4}}}}),
        )
        .unwrap();
    h.app.render_workspace.view.name = "Fixed".into();
    setups::load(&mut h.app);
    let before = h.app.session.doc.clone();
    let history = (h.app.session.undo.len(), h.app.session.redo.len());
    let positive = ThinLens { aperture_radius_mm: 0.5, focus_distance_mm: 20.0 };
    assert!(shot::set_lens(&mut h.app, "Fixed", Some(positive)).unwrap_err().contains("progressive"));
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
    let zero = ThinLens { aperture_radius_mm: 0.0, ..positive };
    shot::set_lens(&mut h.app, "Fixed", Some(zero)).unwrap();
    let saved = h.app.session.doc.render.setups["Fixed"].clone();
    let RenderJob::CpuRaytrace { options, .. } = saved.job.as_ref().unwrap() else { panic!("CPU method changed") };
    assert_eq!(options.lens, Some(zero));
    assert!(options.progressive.is_none());
    assert_eq!(options.primary_samples, 4);
    h.app.cam.fov = 0.0;
    shot::set_lens(&mut h.app, "Fixed", None).unwrap();
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.render.setups["Fixed"], saved);
    assert_eq!(h.app.cam.fov, 0.0, "removing a lens never navigates or saves the current camera");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fresh_cpu_default_frame_perspective_then_first_dof_edit_saves_visible_job_once_offline() {
    use egui::{Event, Modifiers, PointerButton};
    use solidcraft_engine::doc::render_settings::RenderJob;
    use solidcraft_engine::render::job::CpuHost;
    let mut h = Harness::new();
    let host = CpuHost::for_platform().unwrap();
    h.app.session.set_render_job_host(Some(host.clone())).unwrap();
    h.app.render_workspace.image_host = Some(host);
    if image_host(&h.app, ImageMethod::CpuRaytrace).is_err() {
        eprintln!("fresh CPU lens default test requires raytrace-cpu");
        return;
    }
    h.app.preferences.render_legacy_image_defaults = false;
    h.app.render_workspace.view.live_paused = true;
    let original = h.app.session.doc.clone();
    let original_undo = h.app.session.undo.len();
    enter(&mut h.app, true).unwrap();
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::CpuRaytrace);
    assert_eq!(h.app.render_workspace.view.raytrace.progressive.unwrap().samples_per_pixel, 16);
    assert!(Arc::ptr_eq(&original, &h.app.session.doc), "opening the fresh CPU route is read-only");
    assert_eq!(h.app.session.undo.len(), original_undo);
    assert!(!h.app.session.doc.render.setups.contains_key("Studio"));
    let camera = Camera { fov: 0.7, ..h.app.cam };
    shot::set_camera(&mut h.app, "Studio", camera).unwrap();
    assert!(h.app.session.doc.render.setups["Studio"].job.is_none(), "the explicit Perspective edit saves only its camera");
    assert_eq!(h.app.session.undo.len(), original_undo + 1);
    let visible = setups::snapshot(&h.app).unwrap();
    let before = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.app.session.set_render_job_host(None).unwrap();
    h.app.render_workspace.image_host = None;
    h.app.render_workspace.host = None;
    assert!(image_host(&h.app, ImageMethod::CpuRaytrace).is_err());
    let paint = |h: &mut Harness, events: Vec<Event>| {
        let mut output = h.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 2400.0))),
                events,
                ..Default::default()
            },
            |ui| shot::editor(&mut h.app, ui, "Studio"),
        );
        output.textures_delta.clear();
    };
    paint(&mut h, Vec::new());
    paint(&mut h, Vec::new());
    let at = crate::scenario::rect_of("render:shot:dof").unwrap().center();
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo, "painting Frame never autosaves the transient CPU job");
    assert!(h.app.render_workspace.view.job.is_none());
    for pressed in [true, false] {
        paint(
            &mut h,
            vec![Event::PointerMoved(at), Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }],
        );
    }
    assert_eq!(h.app.session.undo.len(), undo + 1, "the actual first DOF checkbox click saves one complete numerical job");
    let saved = &h.app.session.doc.render.setups["Studio"];
    assert_eq!(saved.camera, camera);
    assert_eq!(h.app.cam, camera);
    assert_eq!((saved.width, saved.height, saved.exposure_ev), (visible.width, visible.height, visible.exposure_ev));
    let RenderJob::CpuRaytrace { options, .. } = saved.job.as_ref().unwrap() else { panic!("visible CPU method was not persisted") };
    assert_eq!(options.progressive.unwrap().samples_per_pixel, 16);
    assert_eq!(options.primary_samples, 1);
    assert_eq!(options.lens, Some(h.app.render_workspace.view.studio.lens));
    assert!(options.lens.unwrap().aperture_radius_mm > 0.0);
    assert!(h.app.render_workspace.view.job.is_none());
    assert!(h.app.session.render_retained_results().unwrap().is_empty());
    let after = h.app.session.doc.clone();
    paint(&mut h, Vec::new());
    assert!(Arc::ptr_eq(&after, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo + 1);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(*h.app.session.doc, *before);
    assert_eq!(h.app.cam, camera);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn clicked_cpu_alpha_saves_reopens_and_exports_exact_straight_png_with_unchanged_raw_master() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    use solidcraft_engine::render::job::{CpuHost, Phase};
    for legacy in [false, true] {
        let mut h = Harness::new();
        h.app.preferences.render_legacy_image_defaults = legacy;
        let host = CpuHost::for_platform().unwrap();
        h.app.session.set_render_job_host(Some(host.clone())).unwrap();
        h.app.render_workspace.image_host = Some(host.clone());
        let available = image_host(&h.app, ImageMethod::CpuRaytrace).is_ok();
        h.app.render_workspace.view.live_paused = true;
        h.call("ui.resize", json!({"width":1600,"height":2200}));
        let path = h.dir.join("Half.obj");
        std::fs::write(&path, "o Half\nv -4 -4 0\nv 0 -4 0\nv 0 4 0\nv -4 4 0\nf 1 2 3\nf 1 3 4\n").unwrap();
        h.app.run("file.insert_mesh", json!({"path":path})).unwrap();
        let mut camera = Camera { distance: 10.0, ..Default::default() };
        camera.set_view(solidcraft_engine::render::StandardView::Top);
        h.app
            .run(
                "render.settings.set",
                json!({"name":"Cutout","set":{"camera":camera,"width":1,"height":1,"background":[255,255,255],
            "job":{"method":"cpu_raytrace","options":{"primary_samples":4,"lights":[],"output":{"linear_exr":true}}}}}),
            )
            .unwrap();
        h.app.render_workspace.view.name = "Cutout".into();
        enter(&mut h.app, true).unwrap();
        h.frames(3);
        inspector::apply_camera_draft(&mut h.app, camera).unwrap();
        inspector::select_editor(&mut h.app, inspector::Editor::Output);
        h.frames(3);
        let original = h.app.session.doc.clone();
        let undo = h.app.session.undo.len();
        h.step(&json!({"click":{"handle":"render:transparent-background"}})).unwrap();
        assert!(h.app.render_workspace.view.transparent);
        assert_eq!(h.app.session.undo.len(), undo + 1);
        assert!(matches!(&h.app.session.doc.render.setups["Cutout"].job, Some(RenderJob::CpuRaytrace { transparent_background: true, .. })));
        assert_eq!(h.app.session.doc.features, original.features);
        assert_eq!(h.app.session.doc.materials, original.materials);
        assert!(h.app.render_workspace.view.job.is_none());
        h.app.run("edit.undo", json!({})).unwrap();
        setups::sync_revision(&mut h.app);
        assert!(!h.app.render_workspace.view.transparent);
        h.app.run("edit.redo", json!({})).unwrap();
        setups::sync_revision(&mut h.app);
        assert!(h.app.render_workspace.view.transparent);
        let reopened = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
        h.app.session = Session::new(reopened);
        h.app.session.set_render_job_host(Some(host)).unwrap();
        reset_document(&mut h.app);
        h.app.render_workspace.view.live_paused = true;
        h.app.render_workspace.view.name = "Cutout".into();
        setups::load(&mut h.app);
        enter(&mut h.app, true).unwrap();
        h.frames(3);
        inspector::apply_camera_draft(&mut h.app, camera).unwrap();
        assert!(h.app.render_workspace.view.transparent);
        if !available {
            continue;
        }
        let doc = h.app.session.doc.clone();
        let request = image_request(&h.app).unwrap();
        let checked = h.app.session.execute("render.preflight", &request).unwrap();
        assert_eq!(checked["request"]["transparent_background"], true);
        assert_eq!(checked["transparency"], "coverage_alpha");
        let runtime=h.app.session.execute("render.preflight",&json!({"integrator":"raytrace","settings":{"width":1,"height":1,"camera":camera,"background":[255,255,255]},"transparent_background":true,"raytrace":checked["request"]["raytrace"]})).unwrap();
        assert_eq!(checked["request"], runtime["request"]);
        h.frames(2);
        h.step(&json!({"click":{"handle":"render:stage:render.start"}})).unwrap();
        let job = h.app.render_workspace.view.job.unwrap();
        let handle = h.app.session.render_job_handle(job).unwrap();
        let status = handle.wait_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert_eq!(status.progress.phase, Phase::Completed, "{:?}", status.error);
        let frame = handle.frame().unwrap().unwrap();
        let png = image::load_from_memory(frame.png()).unwrap().to_rgba8();
        assert_eq!(png.get_pixel(0, 0).0, [0, 0, 0, 128]);
        let path = h.dir.join(format!("cpu-cutout-{legacy}.png"));
        h.app.session.execute("render.export", &json!({"job":job,"path":path})).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), frame.png());
        let mut opaque = request["settings"].clone();
        opaque["job"]["transparent_background"] = json!(false);
        let started = h.app.session.execute("render.start", &json!({"settings":opaque})).unwrap();
        let opaque_id = serde_json::from_value(started["job"].clone()).unwrap();
        let opaque_handle = h.app.session.render_job_handle(opaque_id).unwrap();
        assert_eq!(opaque_handle.wait_timeout(std::time::Duration::from_secs(10)).unwrap().progress.phase, Phase::Completed);
        assert_eq!(opaque_handle.frame().unwrap().unwrap().linear_exr(), frame.linear_exr());
        assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    }
}

#[test]
fn cpu_denoise_effect_control_saves_once_reopens_offline_and_tracks_undo_without_launch() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2200}));
    h.app.render_workspace.view.live_paused = true;
    h.app
        .run(
            "render.settings.set",
            json!({"name":"CPU filter","set":{"camera":h.app.cam,"width":16,"height":8,
        "job":{"method":"cpu_raytrace","transparent_background":true,"options":{"progressive":{"samples_per_pixel":3,"seed":81}}}}}),
        )
        .unwrap();
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.live_paused = true;
    h.app.render_workspace.view.name = "CPU filter".into();
    setups::load(&mut h.app);
    h.app.run("view.render_select", json!({"kind":"effects"})).unwrap();
    h.frames(3);
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let camera = h.app.cam;
    h.frames(3);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert!(h.app.render_workspace.view.job.is_none());
    h.step(&json!({"click":{"handle":"render:denoise-enable"}})).unwrap();
    h.frames(3);
    let saved = &h.app.session.doc.render.setups["CPU filter"];
    let Some(RenderJob::CpuRaytrace { options, .. }) = &saved.job else { panic!("CPU job expected") };
    assert!(options.denoise);
    assert!(h.app.render_workspace.view.transparent);
    assert_eq!(image_request(&h.app).unwrap()["settings"]["job"]["transparent_background"], true);
    assert_eq!((saved.width, saved.height), (16, 8));
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(h.app.render_workspace.view.job.is_none());
    assert_eq!(image_request(&h.app).unwrap()["settings"]["job"]["options"]["denoise"], true);
    h.app.run("edit.undo", json!({})).unwrap();
    setups::sync_revision(&mut h.app);
    assert!(!h.app.render_workspace.view.studio.denoise);
    h.app.run("edit.redo", json!({})).unwrap();
    setups::sync_revision(&mut h.app);
    assert!(h.app.render_workspace.view.studio.denoise);
    let reopened = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
    h.app.session = Session::new(reopened);
    reset_document(&mut h.app);
    h.app.render_workspace.view.live_paused = true;
    h.app.render_workspace.view.name = "CPU filter".into();
    setups::load(&mut h.app);
    assert!(h.app.render_workspace.view.studio.denoise);
    assert!(h.app.render_workspace.view.raytrace.denoise);
    assert!(h.app.render_workspace.view.transparent);
    let mut runtime = json!({"settings":{"camera":h.app.cam}});
    cpu::params(&h.app, &mut runtime).unwrap();
    assert_eq!(runtime["raytrace"]["denoise"], true);
    assert!(runtime.get("denoise").is_none(), "the CPU filter must not enter the GPU runtime schema");
}

#[test]
fn cpu_common_glass_controls_preserve_unsupported_effects_and_all_cpu_options() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":3,"body_name":"Optics"})).unwrap();
    let target = materials::read_targets(&mut h.app).unwrap().remove(0);
    h.app.render_workspace.view.method = ImageMethod::CpuRaytrace;
    h.app.render_workspace.view.raytrace.progressive = Some(Default::default());
    h.app.render_workspace.view.draft.width = 8;
    h.app.render_workspace.view.draft.height = 8;
    h.app.render_workspace.view.draft.exposure_ev = 1.5;
    let mut binding = materials::Binding::new(target, &h.app);
    binding.glass_enabled = true;
    binding.glass = materials::Glass::recipe("starter.glass.amber-tint").unwrap();
    h.app.render_workspace.view.materials.enabled = true;
    // Keep the direct ordinal adapter's all-options preservation oracle; the
    // subsequent shared controls retain their real stable target.
    let mut ordinal = binding.clone();
    ordinal.target.stable_target = None;
    h.app.render_workspace.view.materials.bindings = vec![ordinal];
    let mut params = json!({"settings":{"camera":h.app.cam}});
    cpu::params(&h.app, &mut params).unwrap();
    let previous = params["raytrace"].clone();
    materials::params(&h.app, &mut params).unwrap();
    for (key, value) in previous.as_object().unwrap() {
        if key != "dielectrics" {
            assert_eq!(&params["raytrace"][key], value, "shared Glass must preserve CPU {key}");
        }
    }
    for (coat, maps, scatter, shadows) in
        [(true, false, false, false), (false, true, false, false), (false, false, true, false), (false, false, false, true)]
    {
        let mut unsupported = binding.clone();
        unsupported.glass_enabled = !(coat || maps);
        unsupported.coat_enabled = coat;
        unsupported.maps_enabled = maps;
        if scatter {
            unsupported.glass.subsurface =
                Some(solidcraft_engine::render::transmission::subsurface::Subsurface { scattering_per_mm: 0.1, anisotropy: 0.0 });
        }
        unsupported.glass.shadows = shadows;
        h.app.render_workspace.view.materials.bindings = vec![unsupported];
        if coat {
            let request = image_request(&h.app).unwrap();
            assert_eq!(request["settings"]["coats"][0]["coat"], json!({"weight":1.0,"roughness":0.12,"ior":1.5}));
            assert_eq!(request["settings"]["job"]["method"], "cpu_raytrace");
            assert_eq!(request["settings"]["exposure_ev"], 1.5);
        } else {
            assert!(image_request(&h.app).is_err());
        }
        let retained = &h.app.render_workspace.view.materials.bindings[0];
        assert_eq!((retained.coat_enabled, retained.maps_enabled, retained.glass.shadows), (coat, maps, shadows));
        assert_eq!(retained.glass.subsurface.is_some(), scatter);
        assert_eq!(retained.glass.recipe, binding.glass.recipe);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn clicked_cpu_glass_render_save_reopen_hydrates_and_previews_edits_without_changing_mass() {
    cpu_glass_ui_save_reopen_workflow(false);
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn clicked_cpu_general_closed_glass_path_render_save_reopen_keeps_geometry_mass_and_reviewed_numbers() {
    cpu_glass_ui_save_reopen_workflow(true);
}
#[cfg(not(target_arch = "wasm32"))]
fn cpu_glass_ui_save_reopen_workflow(general: bool) {
    use solidcraft_engine::render::job::{CpuHost, Phase};
    let mut h = Harness::new();
    let host = CpuHost::for_platform().unwrap();
    h.app.session.set_render_job_host(Some(host.clone())).unwrap();
    h.app.render_workspace.image_host = Some(host.clone());
    if image_host(&h.app, ImageMethod::CpuRaytrace).is_err() {
        eprintln!("CPU Glass UI test requires raytrace-cpu");
        return;
    }
    h.app.render_workspace.view.live_paused = true;
    h.call("ui.resize", json!({"width":1600,"height":2200}));
    if general {
        h.app.run("solid.cylinder", json!({"radius":5,"height":3,"body_name":"Optics"})).unwrap();
    } else {
        h.app.run("solid.box", json!({"length":10,"width":10,"height":3,"body_name":"Optics"})).unwrap();
    }
    h.app.run("material.assign", json!({"bodies":["Optics"],"material":"Steel"})).unwrap();
    let physical = h.app.run("component.properties", json!({"component":"root"})).unwrap();
    assert!(physical["total"]["mass_g"].as_f64().unwrap() > 0.0);
    let camera = Camera {
        target: solidcore_record::Vec3::new(if general { 0.0 } else { 5.0 }, if general { 0.0 } else { 5.0 }, 1.5),
        distance: 20.0,
        yaw: 0.3,
        pitch: 0.4,
        fov: 0.5,
        ..Default::default()
    };
    h.app
        .run(
            "render.settings.set",
            json!({"name":"Studio","set":{"camera":camera,"width":8,"height":8,"background":[255,255,255],"job":{"method":"cpu_raytrace","options":{
        "path":general.then_some(json!({"max_depth":12})),"lights":[],"environment":{"color":[1,1,1],"intensity":1},"progressive":{"samples_per_pixel":3,"seed":81}}}}}),
        )
        .unwrap();
    enter(&mut h.app, true).unwrap();
    h.frames(3);
    inspector::apply_camera_draft(&mut h.app, camera).unwrap();
    setups::load(&mut h.app);
    h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body {name:"Optics".into()}]})).unwrap();
    h.frames(3);
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let glass = h.app.tr("render.appearance_editor.glass").to_string();
    h.step(&json!({"click":{"handle":format!("dialog:section:{glass}")}})).unwrap();
    h.step(&json!({"widget":h.app.tr("render.material_custom")})).unwrap();
    h.step(&json!({"click":{"handle":"render:glass:starter.glass.amber-tint"}})).unwrap();
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    let temporary = image_request(&h.app).unwrap();
    assert_eq!(temporary["settings"]["optics"][0]["optics"]["boundary"], "closed_mesh");
    if general {
        assert_eq!(temporary["settings"]["job"]["options"]["path"]["max_depth"], 12);
    }
    assert_eq!(temporary["settings"]["optics"][0]["optics"]["ior"], 1.51);
    let checked = h.app.session.execute("render.preflight", &temporary).unwrap();
    assert_eq!(checked["request"]["raytrace"]["dielectrics"][0]["boundary"], if general { "closed_mesh" } else { "closed_box" });
    assert_eq!(checked["request"]["raytrace"]["dielectrics"][0]["ior"], 1.51);
    h.step(&json!({"click":{"handle":"render:stage:render.start"}})).unwrap();
    let job = h.app.render_workspace.view.job.unwrap();
    let handle = h.app.session.render_job_handle(job).unwrap();
    let terminal = handle.wait_timeout(std::time::Duration::from_secs(10)).unwrap();
    assert_eq!(terminal.progress.phase, Phase::Completed, "{:?}", terminal.error);
    let png = handle.frame().unwrap().unwrap().png().to_vec();
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    h.frames(2);
    h.app.run("render.result.view", json!({"view":"model"})).unwrap();
    h.frames(2);
    let saved = h.app.tr("render.appearance_editor.saved").to_string();
    h.step(&json!({"click":{"handle":format!("dialog:section:{saved}")}})).unwrap();
    h.step(&json!({"click":{"handle":"render:optics-save"}})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.doc.features, doc.features);
    assert_eq!(h.app.session.doc.materials, doc.materials);
    assert_eq!(h.app.run("component.properties", json!({"component":"root"})).unwrap(), physical);
    assert_eq!(h.app.session.doc.render.setups["Studio"].optics.len(), 1);
    let captured = h.app.session.execute("render.preflight", &image_request(&h.app).unwrap()).unwrap();
    assert_eq!(captured["request"], checked["request"]);
    let reopened = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
    h.app.session = Session::new(reopened);
    h.app.session.set_render_job_host(Some(host)).unwrap();
    reset_document(&mut h.app);
    h.app.render_workspace.view.live_paused = true;
    setups::load(&mut h.app);
    enter(&mut h.app, true).unwrap();
    h.frames(3);
    inspector::apply_camera_draft(&mut h.app, camera).unwrap();
    h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body {name:"Optics".into()}]})).unwrap();
    let reopened_doc = h.app.session.doc.clone();
    h.frames(3);
    assert!(Arc::ptr_eq(&reopened_doc, &h.app.session.doc));
    assert!(h.app.render_workspace.view.materials.bindings.is_empty(), "hydrated saved controls are not a temporary override on paint");
    if crate::scenario::rect_of("render:glass:ior-field").is_none() {
        h.step(&json!({"click":{"handle":format!("dialog:section:{glass}")}})).unwrap();
    }
    h.frames(2);
    assert!(h.widget("1.51").is_some(), "the Glass IOR field displays the saved numerical value");
    let same = h.app.session.execute("render.preflight", &image_request(&h.app).unwrap()).unwrap();
    assert_eq!(same["request"], checked["request"]);
    let result = h.app.session.execute("render.start", &image_request(&h.app).unwrap()).unwrap();
    let id = serde_json::from_value(result["job"].clone()).unwrap();
    let complete = h.app.session.render_job_handle(id).unwrap();
    assert_eq!(complete.wait_timeout(std::time::Duration::from_secs(10)).unwrap().progress.phase, Phase::Completed);
    assert_eq!(complete.frame().unwrap().unwrap().png(), png);
    let field = crate::scenario::rect_of("render:glass:ior-field").unwrap().center();
    h.step(&json!({"click":[field.x,field.y],"double":true})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"1.6"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    h.frames(2);
    let prospective = image_request(&h.app).unwrap();
    assert_eq!(prospective["settings"]["optics"][0]["optics"]["ior"], 1.6, "ordinary image/live capture sees the edited saved binding immediately");
    assert_eq!(h.app.session.doc.render.setups["Studio"].optics.iter().next().unwrap().optics.ior, 1.51);
    assert!(Arc::ptr_eq(&reopened_doc, &h.app.session.doc));
    h.app.session.execute("render.preflight", &prospective).unwrap();
    h.step(&json!({"click":{"handle":"render:glass:enable"}})).unwrap();
    assert!(
        image_request(&h.app).unwrap()["settings"]["optics"].as_array().is_none_or(Vec::is_empty),
        "disabling saved Glass removes only its prospective row"
    );
    assert_eq!(h.app.session.doc.render.setups["Studio"].optics.len(), 1);
    h.frames(1);
    if crate::scenario::rect_of("render:optics-discard-edit").is_none() {
        h.step(&json!({"click":{"handle":format!("dialog:section:{saved}")}})).unwrap();
    }
    h.step(&json!({"click":{"handle":"render:optics-discard-edit"}})).unwrap();
    h.frames(2);
    assert_eq!(image_request(&h.app).unwrap()["settings"]["optics"][0]["optics"]["ior"], 1.51);
    assert!(h.app.render_workspace.view.materials.bindings.is_empty());
    assert!(Arc::ptr_eq(&reopened_doc, &h.app.session.doc));
}

#[test]
fn saved_glass_prospective_overlay_keeps_other_targets_and_refuses_stale_sources() {
    let mut h = Harness::new();
    for (name, corner) in [("First", [0, 0, 0]), ("Second", [20, 0, 0])] {
        h.app.run("solid.box", json!({"length":10,"width":10,"height":3,"body_name":name,"corner":corner,"operation":"new"})).unwrap();
    }
    h.app.run("render.settings.set", json!({"name":"Studio","set":{"width":8,"height":8,"job":{"method":"cpu_raytrace"}}})).unwrap();
    for (index, name, ior) in [(0, "First", 1.4), (1, "Second", 1.6)] {
        h.app
            .run(
                "render.optics.bind",
                json!({"name":"Studio","body_index":index,"body_name":name,"expected_revision":h.app.session.revision,"optics":{"ior":ior}}),
            )
            .unwrap();
    }
    setups::load(&mut h.app);
    let target = materials::read_targets(&mut h.app).unwrap().remove(0);
    let first = h.app.render_workspace.view.draft.optics.iter().find(|value| target.stable_target.as_ref() == Some(&value.target)).unwrap().clone();
    let second = h.app.render_workspace.view.draft.optics.iter().find(|value| value.target != first.target).unwrap().clone();
    let mut binding = materials::Binding::from_saved(target, &h.app, first.optics);
    binding.glass.ior = 1.7;
    h.app.render_workspace.view.materials.enabled = true;
    h.app.render_workspace.view.materials.bindings = vec![binding];
    let doc = h.app.session.doc.clone();
    let candidate: RenderSetup = serde_json::from_value(image_request(&h.app).unwrap()["settings"].clone()).unwrap();
    assert_eq!(candidate.optics.len(), 2);
    assert_eq!(candidate.optics.iter().find(|value| value.target == first.target).unwrap().optics.ior, 1.7);
    assert_eq!(candidate.optics.iter().find(|value| value.target == second.target).unwrap(), &second);
    assert!(setups::snapshot(&h.app).is_err(), "an ordinary shot save cannot quietly persist the old saved optical values");
    h.app.render_workspace.view.materials.bindings[0].glass_enabled = false;
    let off: RenderSetup = serde_json::from_value(image_request(&h.app).unwrap()["settings"].clone()).unwrap();
    assert_eq!(off.optics.iter().collect::<Vec<_>>(), vec![&second]);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    h.app.run("solid.box", json!({"length":1,"width":1,"height":1,"corner":[50,0,0],"operation":"new"})).unwrap();
    assert!(image_request(&h.app).is_err(), "a stale pending edit must be reviewed rather than attached to the next body");
    assert!(!h.app.render_workspace.view.materials.bindings[0].glass_enabled, "refusal retains the exact prospective draft");
}

#[test]
fn saved_zero_thickness_glass_hydration_never_creates_a_prospective_edit_on_paint() {
    for method in ["cpu_raytrace", "gpu_path"] {
        let mut h = Harness::new();
        h.app.render_workspace.view.live_paused = true;
        h.call("ui.resize", json!({"width":1600,"height":2200}));
        let path = h.dir.join("ZeroSheet.obj");
        std::fs::write(&path, "o ZeroSheet\nv -10 -10 0\nv 10 -10 0\nv 10 10 0\nv -10 10 0\nf 1 2 3\nf 1 3 4\n").unwrap();
        h.app.run("file.insert_mesh", json!({"path":path})).unwrap();
        let body = h.app.run("render.scene", json!({})).unwrap()["bodies"][0]["name"].as_str().unwrap().to_owned();
        let mut camera = Camera { distance: 10.0, ..Default::default() };
        camera.set_view(solidcraft_engine::render::StandardView::Top);
        h.app.run("render.settings.set", json!({"name":"Studio","set":{"camera":camera,"width":8,"height":8,"job":{"method":method}}})).unwrap();
        h.app
            .run(
                "render.optics.bind",
                json!({"name":"Studio","body_index":0,"body_name":body,"expected_revision":h.app.session.revision,
            "optics":{"boundary":"thin_sheet","thickness_mm":0.0,"ior":1.51,"absorption_per_mm":[0.1,0.2,0.0],"shadow_transmission":false}}),
            )
            .unwrap();
        enter(&mut h.app, true).unwrap();
        h.frames(3);
        inspector::apply_camera_draft(&mut h.app, camera).unwrap();
        setups::load(&mut h.app);
        h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body {name:body}]})).unwrap();
        h.frames(2);
        let before = image_request(&h.app).unwrap();
        assert_eq!(before["settings"]["optics"][0]["optics"]["thickness_mm"], 0.0);
        let document = h.app.session.doc.clone();
        let model = h.app.session.model.state();
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        assert!(h.app.render_workspace.view.materials.bindings.is_empty());
        let glass = h.app.tr("render.appearance_editor.glass").to_string();
        h.step(&json!({"click":{"handle":format!("dialog:section:{glass}")}})).unwrap();
        h.frames(3);
        assert!(crate::scenario::rect_of("render:glass:thickness-field").is_some(), "the actual hydrated thickness editor was painted");
        assert!(h.app.render_workspace.view.materials.bindings.is_empty(), "valid zero thickness is not an unsaved optical edit: {method}");
        assert_eq!(image_request(&h.app).unwrap(), before, "painting the opened Glass section preserves exact captured intent: {method}");
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert!(Arc::ptr_eq(&model, &h.app.session.model.state()));
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
        assert!(h.app.render_workspace.view.job.is_none());
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cold_saved_cpu_lens_hydrates_its_camera_and_render_reentry_preserves_the_dirty_render_view() {
    use solidcraft_engine::render::job::{CpuHost, CpuProgressiveOptions, ThinLens};
    let mut h = Harness::new();
    let host = CpuHost::for_platform().unwrap();
    h.app.session.set_render_job_host(Some(host.clone())).unwrap();
    h.app.render_workspace.image_host = Some(host.clone());
    if image_host(&h.app, ImageMethod::CpuRaytrace).is_err() {
        eprintln!("native CPU lens hydration test requires raytrace-cpu");
        return;
    }
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5,"body_name":"Subject"})).unwrap();
    let saved_camera = Camera { fov: 35f64.to_radians(), distance: 168.6, target: solidcore_record::Vec3::new(5.0, 10.0, 2.5), ..Default::default() };
    let lens = ThinLens { aperture_radius_mm: 1.0, focus_distance_mm: 168.6 };
    let progressive = CpuProgressiveOptions { samples_per_pixel: 3, seed: 81 };
    h.app
        .run(
            "render.settings.set",
            json!({"name":"Lens","set":{"camera":saved_camera,"width":8,"height":8,
        "job":{"method":"cpu_raytrace","options":{"lens":lens,"progressive":progressive}}}}),
        )
        .unwrap();
    let design = (*h.app.session.doc).clone();
    let reopened = solidcraft_engine::doc::Document::from_json(&design.to_json()).unwrap();
    crate::documents::open_tab(&mut h.app, Session::opened(reopened, None)).unwrap();
    h.app.session.set_render_job_host(Some(host.clone())).unwrap();
    h.app.render_workspace.image_host = Some(host);
    h.app.render_workspace.view.name = "Lens".into();
    h.app.cancel_view_animation();
    h.app.cam = Camera { distance: 211.0, target: solidcore_record::Vec3::new(-12.0, 7.0, 9.0), ..Default::default() };
    h.app.ui.perspective = false;
    let original_design_camera = h.app.cam;
    let document = h.app.session.doc.clone();
    let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    enter(&mut h.app, true).unwrap();
    assert_eq!(h.app.cam, saved_camera);
    assert!(h.app.ui.perspective);
    assert!(h.app.pending_fit.is_none() && h.app.render_workspace.entry_fit.is_none());
    assert_eq!(setups::snapshot(&h.app).unwrap(), document.render.setups["Lens"]);
    assert!(!h.app.render_workspace.view.dirty);
    h.app.viewport.rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(8.0, 8.0)));
    live::advance(&mut h.app, &h.ctx, 1.0);
    let completed = wait_live_complete(&mut h, 1.3);
    assert!(completed["error"].is_null());
    assert_eq!(completed["request"]["settings"]["camera"], json!(saved_camera));
    assert_eq!(completed["request"]["settings"]["job"]["options"]["lens"], json!(lens));
    assert!(completed["hash"].as_str().is_some_and(|hash| !hash.is_empty()));
    assert!(h.app.session.render_retained_results().unwrap().is_empty());

    h.app.cam.yaw += 0.3;
    h.app.render_workspace.view.draft.width = 7;
    setups::refresh_dirty(&mut h.app);
    let render_camera = h.app.cam;
    assert!(h.app.render_workspace.view.dirty);
    enter(&mut h.app, false).unwrap();
    assert_eq!(h.app.cam, original_design_camera);
    assert!(!h.app.ui.perspective);
    let changed_design = Camera { yaw: 0.4, distance: 250.0, ..original_design_camera };
    h.app.cam = changed_design;
    enter(&mut h.app, true).unwrap();
    assert_eq!(h.app.cam, render_camera);
    assert!(h.app.ui.perspective);
    assert_eq!(h.app.render_workspace.view.draft.width, 7);
    assert!(h.app.render_workspace.view.dirty);
    assert!(h.app.pending_fit.is_none());
    assert_eq!(image_request(&h.app).unwrap()["settings"]["camera"], json!(render_camera));
    enter(&mut h.app, false).unwrap();
    assert_eq!(h.app.cam, changed_design);
    assert!(!h.app.ui.perspective);
    assert!(Arc::ptr_eq(&h.app.session.doc, &document));
    assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
    assert!(!h.app.session.is_dirty());
    reset_document(&mut h.app);
    assert!(h.app.render_workspace.view.render_camera.is_none(), "new documents cannot inherit a retired Render camera");
}

#[test]
fn legacy_entry_keeps_the_previous_fit_and_does_not_remember_a_render_camera() {
    let mut h = cold_render_entry_fixture();
    h.app.preferences.render_legacy_image_defaults = true;
    let design = h.app.cam;
    enter(&mut h.app, true).unwrap();
    assert!(h.app.pending_fit.is_some());
    h.frames(4);
    let render_camera = h.app.cam;
    assert_ne!(render_camera, h.app.session.doc.render.setups["Studio"].camera);
    enter(&mut h.app, false).unwrap();
    assert_eq!(h.app.cam, design);
    assert!(h.app.render_workspace.view.render_camera.is_none());
}

#[test]
fn live_source_labels_use_captured_executor_names_instead_of_the_selected_method() {
    let mut h = Harness::new();
    h.app.render_workspace.view.method = ImageMethod::CpuRaytrace;
    for (executor, key) in [("cpu-raytrace", "render.method_cpu"), ("gpu-path", "render.method_gpu")] {
        for current in [true, false] {
            let expected =
                crate::i18n::format(if current { "render.live.source_current" } else { "render.live.source_outdated" }, &[("method", h.app.tr(key))]);
            assert_eq!(live::source_text(&h.app, executor, current), expected);
        }
    }
    let expected = crate::i18n::format("render.live.source_outdated", &[("method", "test-image-executor")]);
    assert_eq!(live::source_text(&h.app, "test-image-executor", false), expected, "unrecognised captures cannot claim CPU or GPU transport");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn live_source_badge_follows_real_pixels_and_its_settings_gear_opens_output_without_editing() {
    fn draw(h: &mut Harness, events: Vec<egui::Event>, width: f32) -> Vec<egui::epaint::ClippedShape> {
        crate::scenario::clear_handles();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, crate::ui_kit::StageBar::HEIGHT));
        let mut output = h.ctx.run_ui(egui::RawInput { screen_rect: Some(rect.expand(20.0)), events, ..Default::default() }, |ui| {
            crate::render_ui::stage_bar(&mut h.app, ui, rect)
        });
        output.textures_delta.clear();
        output.shapes
    }
    let Some(mut h) = live_cpu_fixture() else { return };
    live::advance(&mut h.app, &h.ctx, 1.0);
    wait_live_complete(&mut h, 1.3);
    let current = crate::i18n::format("render.live.source_current", &[("method", h.app.tr("render.method_cpu"))]);
    assert_eq!(live::source_note(&h.app), (current.clone(), "render.live.quality_help"));
    let document = h.app.session.doc.clone();
    let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    let camera = h.app.cam;
    let settings = setups::draft(&h.app).unwrap();
    let shapes = draw(&mut h, vec![], 1440.0);
    assert!(shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == current)));
    assert!(
        !shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == h.app.tr("render.stage_preview_note")))
    );
    let gear = crate::scenario::rect_of("render:preview:settings").unwrap();
    assert!(crate::scenario::rect_of("render:stage_tools").unwrap().contains_rect(gear));
    for pressed in [true, false] {
        draw(
            &mut h,
            vec![
                egui::Event::PointerMoved(gear.center()),
                egui::Event::PointerButton { pos: gear.center(), button: egui::PointerButton::Primary, pressed, modifiers: Default::default() },
            ],
            1440.0,
        );
    }
    assert_eq!(inspector::selected_editor(&h.app), inspector::Editor::Output);
    assert_eq!(h.app.cam, camera);
    assert_eq!(setups::draft(&h.app).unwrap(), settings);
    assert!(Arc::ptr_eq(&h.app.session.doc, &document));
    assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
    assert!(h.app.render_workspace.view.job.is_none());
    assert!(h.app.session.render_retained_results().unwrap().is_empty());

    for width in [1100.0, 800.0] {
        for stale in [false, true] {
            h.app.cam = camera;
            if stale {
                h.app.cam.yaw += 0.2;
            }
            h.app.run("view.render_select", json!({"kind":"nothing"})).unwrap();
            draw(&mut h, vec![], width);
            let tools = crate::scenario::rect_of("render:stage_tools").unwrap();
            let bar = crate::scenario::rect_of("render:stage_bar").unwrap();
            let gear = crate::scenario::rect_of("render:preview:settings").unwrap();
            assert!(gear.is_positive() && tools.contains_rect(gear) && bar.contains_rect(gear), "{width}/{stale}: gear {gear:?} outside {tools:?}");
            for pressed in [true, false] {
                draw(
                    &mut h,
                    vec![
                        egui::Event::PointerMoved(gear.center()),
                        egui::Event::PointerButton {
                            pos: gear.center(),
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                    width,
                );
            }
            assert_eq!(inspector::selected_editor(&h.app), inspector::Editor::Output);
            assert!(h.app.render_workspace.view.job.is_none());
        }
    }
    h.app.cam = camera;
    assert!(Arc::ptr_eq(&h.app.session.doc, &document));
    assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);

    // A method change before the next lifecycle tick cannot rename an existing CPU image.
    h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
    let previous = crate::i18n::format("render.live.source_outdated", &[("method", h.app.tr("render.method_cpu"))]);
    assert_eq!(live::source_note(&h.app), (previous.clone(), "render.live.source_outdated_help"));
    live::advance(&mut h.app, &h.ctx, 2.0);
    assert_eq!(live::source_note(&h.app), (previous, "render.live.source_outdated_help"), "a refused update retains the captured source name");
    h.app.render_workspace.view.method = ImageMethod::Raster;
    assert_eq!(live::source_note(&h.app), (h.app.tr("render.stage_preview_note").to_string(), "render.preview_lighting_help"));
    h.app.render_workspace.view.method = ImageMethod::CpuRaytrace;
    h.app.preferences.render_legacy_image_defaults = true;
    assert_eq!(live::source_note(&h.app), (h.app.tr("render.stage_preview_note").to_string(), "render.preview_lighting_help"));
    h.app.preferences.render_legacy_image_defaults = false;
    h.app.render_workspace.view.result = true;
    draw(&mut h, vec![], 1440.0);
    assert!(crate::scenario::rect_of("render:preview_note").is_none(), "the result viewer has its own captured-image information");
    assert!(crate::scenario::rect_of("render:preview:settings").is_none());
}

#[test]
fn result_fit_and_header_actions_stay_inside_the_free_canvas_with_long_retained_shot_names() {
    use solidcraft_engine::render::job::{JobId, Phase};
    for side in ["left", "right"] {
        for dpi in [1.0, 2.0] {
            let mut h = output_viewer_fixture();
            h.app.render_workspace.view.live_paused = true;
            h.call("ui.resize", json!({"width":1100,"height":760}));
            h.ctx.set_pixels_per_point(dpi);
            h.app.run("view.render_dock", json!({"panel":"inspector","side":side})).unwrap();
            let host = h.app.session.render_job_host().unwrap().clone();
            let mut jobs = Vec::new();
            for (name, background) in [
                ("CPU lens acceptance with a long captured shot name for the retained image", [20, 40, 60]),
                ("GPU lens acceptance with a second long captured shot name for the result", [90, 70, 50]),
            ] {
                let reply = h.app.session.execute("render.start", &json!({"settings":{"width":96,"height":54,"background":background}})).unwrap();
                let job: JobId = serde_json::from_value(reply["job"].clone()).unwrap();
                for _ in 0..100 {
                    host.pump(16384).unwrap();
                    if h.app.session.render_job_handle(job).unwrap().status().unwrap().progress.phase == Phase::Completed {
                        break;
                    }
                }
                assert_eq!(h.app.session.render_job_handle(job).unwrap().status().unwrap().progress.phase, Phase::Completed);
                h.app.session.label_render_job(job, name).unwrap();
                jobs.push(job);
            }
            h.app.run("render.result.select", json!({"index":1})).unwrap();
            h.app.run("render.result.view", json!({"view":"fit"})).unwrap();
            h.frames(6);
            let document = h.app.session.doc.clone();
            let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
            let camera = h.app.cam;
            let frame = h.app.session.render_job_handle(jobs[1]).unwrap().frame().unwrap().unwrap();
            let free = h.app.shell.content.unwrap();
            let canvas = h.app.viewport.rect.unwrap();
            assert!(free.contains_rect(canvas), "{side}/{dpi}: canvas {canvas:?} escaped original free {free:?}");
            let fitted = result_rect(h.app.render_workspace.view.result_camera, [96, 54], canvas);
            assert!(canvas.contains_rect(fitted), "{side}/{dpi}: fitted image {fitted:?} escapes canvas {canvas:?}");
            assert!(free.contains_rect(fitted));
            for target in [
                "render:compare:live",
                "render:compare:result:0",
                "render:compare:result:1",
                "render:results",
                "render:result-discard",
                "render:result:view:model",
                "render:result:view:fit",
                "render:result:view:actual",
                "render:result-copy",
                "render:result-export",
                "render:result-close",
            ] {
                let rect = crate::scenario::rect_of(target).unwrap();
                assert!(rect.is_positive() && free.contains_rect(rect), "{side}/{dpi}: header {target} {rect:?} is covered outside {free:?}");
            }
            // The actual popup must offer the retained source inside the same free
            // canvas, and pointer selection must use that source before discarding.
            for index in [0, 1] {
                h.step(&json!({"click":{"handle":"render:results"}})).unwrap();
                h.frames(2);
                for option in [0, 1] {
                    let rect = crate::scenario::rect_of(&format!("render:result:select:{option}")).unwrap();
                    assert!(rect.is_positive() && free.contains_rect(rect), "{side}/{dpi}: popup source{option} {rect:?} escapes {free:?}");
                }
                h.step(&json!({"click":{"handle":format!("render:result:select:{index}")}})).unwrap();
                h.frames(2);
                assert_eq!(h.app.session.render_selected_result(), Some(jobs[index]));
                assert_eq!(h.app.render_workspace.view.completed, Some(jobs[index]));
            }
            h.app.render_workspace.view.result_scale = ResultScale::Free;
            h.app.render_workspace.view.result_camera.distance *= 0.5;
            h.frames(2);
            h.step(&json!({"click":{"handle":"render:result:view:fit"}})).unwrap();
            h.frames(2);
            assert_eq!(h.app.render_workspace.view.result_scale, ResultScale::Fit);
            let canvas = h.app.viewport.rect.unwrap();
            assert!(h.app.shell.content.unwrap().contains_rect(result_rect(h.app.render_workspace.view.result_camera, [96, 54], canvas)));
            let path = h.dir.join("bounded-result.png");
            h.app.session.execute("render.export", &json!({"job":jobs[1],"path":path})).unwrap();
            assert_eq!(std::fs::read(path).unwrap(), frame.png());
            assert_eq!(h.app.cam, camera);
            assert!(Arc::ptr_eq(&h.app.session.doc, &document));
            assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
            assert_eq!(h.app.session.render_selected_result(), Some(jobs[1]));
        }
    }
}

#[test]
fn cpu_path_checkbox_adopts_visible_sampling_in_one_undo_and_reopens_offline() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    for legacy in [false, true] {
        let mut h = sampling_policy_fixture(false);
        h.app.preferences.render_legacy_image_defaults = legacy;
        let document = h.app.session.doc.clone();
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        let camera = h.app.cam;
        let before = setups::draft(&h.app).unwrap();
        h.frames(3);
        assert!(h.app.render_workspace.view.raytrace.path.is_none());
        assert!(Arc::ptr_eq(&h.app.session.doc, &document));
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
        h.step(&json!({"click":{"handle":"render:cpu-path"}})).unwrap();
        h.frames(3);
        let saved = &h.app.session.doc.render.setups[&h.app.render_workspace.view.name];
        assert_eq!((saved.width, saved.height, saved.exposure_ev), (before.width, before.height, before.exposure_ev));
        let Some(RenderJob::CpuRaytrace { options, .. }) = &saved.job else { panic!("explicit CPU path edit must keep CPU") };
        assert_eq!(options.path, Some(Default::default()));
        assert_eq!(
            options.progressive,
            before.job.as_ref().and_then(|job| match job {
                RenderJob::CpuRaytrace { options, .. } => options.progressive,
                _ => None,
            })
        );
        assert_eq!(h.app.session.undo.len(), history.1 + 1);
        assert_eq!(h.app.cam, camera);
        assert!(h.app.render_workspace.view.job.is_none());
        let bytes = serde_json::to_vec(&*h.app.session.doc).unwrap();
        let reopened: solidcraft_engine::doc::Document = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(reopened, *h.app.session.doc);
        h.app.run("edit.undo", json!({})).unwrap();
        setups::sync_revision(&mut h.app);
        assert!(h.app.render_workspace.view.raytrace.path.is_none());
        h.app.run("edit.redo", json!({})).unwrap();
        setups::sync_revision(&mut h.app);
        assert!(h.app.render_workspace.view.raytrace.path.is_some());
        h.app.render_workspace.view.raytrace.path = None;
        setups::load(&mut h.app);
        assert!(h.app.render_workspace.view.raytrace.path.is_some());
    }
}
#[test]
fn cpu_path_adoption_keeps_fixed_and_selected_optical_intent_when_unsupported() {
    use solidcraft_engine::render::job::{ReflectionBody, ReflectionMode, ReflectionOptions};
    let mut h = sampling_policy_fixture(true);
    let document = h.app.session.doc.clone();
    let options = h.app.render_workspace.view.raytrace.clone();
    h.step(&json!({"click":{"handle":"render:cpu-path"}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.render_workspace.view.raytrace, options);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    let mut h = sampling_policy_fixture(false);
    let reflection = ReflectionOptions {
        bodies: vec![ReflectionBody { body: "Kept reflector".into(), mode: ReflectionMode::Mirror }],
        samples: 1,
        max_bounces: 2,
    };
    h.app.render_workspace.view.raytrace.reflections = Some(reflection.clone());
    h.frames(2);
    let document = h.app.session.doc.clone();
    h.step(&json!({"click":{"handle":"render:cpu-path"}})).unwrap();
    h.frames(2);
    assert!(h.app.render_workspace.view.raytrace.path.is_none());
    assert_eq!(h.app.render_workspace.view.raytrace.reflections, Some(reflection));
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn inspector_focus_never_changes_the_saved_output_camera_live_aspect_or_normalized_picking_frame() {
    use solidcraft_engine::render::job::{CpuProgressiveOptions, ThinLens};
    let Some(mut h) = live_cpu_fixture() else { return };
    h.app.preferences.render_legacy_image_defaults = false;
    h.app.render_workspace.view.live_paused = true;
    h.call("ui.resize", json!({"width":1100,"height":760}));
    h.frames(5);
    let camera = Camera { fov: 35f64.to_radians(), distance: 168.6, target: solidcore_record::Vec3::new(5.0, 10.0, 2.5), ..h.app.cam };
    let lens = ThinLens { aperture_radius_mm: 1.0, focus_distance_mm: 168.6 };
    h.app
        .run(
            "render.settings.set",
            json!({"name":"Framing","set":{"camera":camera,"width":160,"height":120,
        "job":{"method":"cpu_raytrace","options":{"lens":lens,"progressive":CpuProgressiveOptions { samples_per_pixel:3,seed:81 }}}}}),
        )
        .unwrap();
    h.app.render_workspace.view.name = "Framing".into();
    setups::load(&mut h.app);
    inspector::apply_camera_draft(&mut h.app, camera).unwrap();
    setups::refresh_dirty(&mut h.app);
    h.app.run("view.render_frame", json!({"aspect":"16:9","thirds":true})).unwrap();
    select_stage(&mut h.app, "look");
    h.frames(4);
    let document = h.app.session.doc.clone();
    let history = (h.app.session.undo.len(), h.app.session.redo.len());
    let dirty = h.app.render_workspace.view.dirty;
    let output = setups::draft(&h.app).unwrap();
    let scene = solidcraft_engine::product_scene::extract(&h.app.session).unwrap();
    let radius = scene.bounds.diagonal().max(10.0) + scene.bounds.center().dist(camera.target);
    for side in ["left", "right"] {
        for dpi in [1.0, 2.0] {
            for [width, height] in [[1100, 760], [1440, 1600]] {
                h.ctx.set_pixels_per_point(dpi);
                h.call("ui.resize", json!({"width":width,"height":height}));
                h.app.run("view.render_dock", json!({"panel":"inspector","side":side})).unwrap();
                for kind in ["shot", "sampling", "effects", "output"] {
                    let selection = if kind == "shot" { json!({"kind":kind,"id":"Framing"}) } else { json!({"kind":kind}) };
                    h.app.run("view.render_select", selection).unwrap();
                    h.frames(4);
                    live::advance(&mut h.app, &h.ctx, 200.0);
                    let state = live::inspect(&h.app);
                    let request = &state["request"];
                    let w = request["settings"]["width"].as_u64().unwrap();
                    let hpx = request["settings"]["height"].as_u64().unwrap();
                    // This is the original 1073 native failure oracle, independent of any
                    // new frame helper/handle: Sampling yielded 471×512 for 160×120 output.
                    assert_eq!(w * 120, hpx * 160, "{side}/{dpi}/{width}x{height}/{kind}: live {w}x{hpx} changed authored 4:3 framing: {state}");
                    assert!(w > 0 && hpx > 0 && w <= 512 && hpx <= 512);
                    assert_eq!(request["settings"]["camera"], json!(camera));
                    assert_eq!(request["settings"]["job"]["options"]["lens"], json!(lens));
                    let viewport = h.app.viewport.rect.unwrap();
                    let frame = crate::scenario::rect_of("render:product:frame").unwrap();
                    let free = crate::shell::workspace_content(&h.app, crate::render_ui::canvas(&h.app, viewport));
                    assert!(frame.is_positive() && free.contains_rect(frame), "{kind}: rendered frame must fit the actual unobscured canvas");
                    assert!((f64::from(frame.width() / frame.height()) - w as f64 / hpx as f64).abs() < 1e-5);
                    let projection = crate::viewport::projection(&h.app, frame);
                    for point in corners(&scene.bounds) {
                        let actual = projection.to_screen(point).unwrap();
                        let normalized = (actual - frame.min) / frame.size();
                        let (x, y, _) = camera.to_screen(point, 160.0, 120.0, radius).unwrap();
                        assert!(
                            (f64::from(normalized.x) - x / 160.0).abs() < 1e-5 && (f64::from(normalized.y) - y / 120.0).abs() < 1e-5,
                            "{side}/{dpi}/{width}x{height}/{kind}: normalized pick projection changed for {point:?}"
                        );
                    }
                    assert_eq!(shot::overlay_visible(&h.app), kind == "shot");
                    assert_eq!(crate::scenario::rect_of("render:frame:overlay").is_some(), kind == "shot");
                    if kind == "shot" {
                        let guide = shot::preview_rect(&h.app, viewport).unwrap();
                        assert!(
                            (guide.width() / guide.height() - 16.0 / 9.0).abs() < 1e-5,
                            "the transient guide aspect remains contextual and independent"
                        );
                    }
                    assert_eq!(h.app.cam, camera);
                    assert_eq!(setups::draft(&h.app).unwrap(), output);
                    assert_eq!(h.app.render_workspace.view.dirty, dirty);
                    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
                    assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
                    assert!(h.app.render_workspace.view.job.is_none());
                }
            }
        }
    }
}

#[test]
fn rendered_output_frame_ignores_transient_guides_but_legacy_keeps_its_contextual_projection() {
    let mut h = frame_editor_fixture();
    h.app.render_workspace.view.live_paused = true;
    h.app.preferences.render_legacy_image_defaults = false;
    h.app.render_workspace.view.draft.width = 160;
    h.app.render_workspace.view.draft.height = 120;
    select_stage(&mut h.app, "look");
    let viewport = egui::Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(1100.0, 760.0));
    let document = h.app.session.doc.clone();
    let history = (h.app.session.undo.len(), h.app.session.redo.len());
    for aspect in ["16:9", "1:1", "9:16"] {
        h.app.run("view.render_frame", json!({"aspect":aspect,"thirds":true,"safe_area":true})).unwrap();
        h.app.run("view.render_select", json!({"kind":"shot","id":"Hero"})).unwrap();
        let frame = shot::render_rect(&h.app, viewport);
        assert!((frame.width() / frame.height() - 4.0 / 3.0).abs() < 1e-5);
        let guide = shot::preview_rect(&h.app, viewport).unwrap();
        assert_ne!(frame, guide);
        h.app.run("view.render_select", json!({"kind":"sampling"})).unwrap();
        assert!(!shot::overlay_visible(&h.app));
        assert_eq!(shot::render_rect(&h.app, viewport), frame);
        h.app.preferences.render_legacy_image_defaults = true;
        assert_eq!(shot::render_rect(&h.app, viewport), viewport);
        h.app.run("view.render_select", json!({"kind":"shot","id":"Hero"})).unwrap();
        assert_eq!(shot::render_rect(&h.app, viewport), shot::preview_rect(&h.app, viewport).unwrap());
        h.app.preferences.render_legacy_image_defaults = false;
    }
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn live_framing_rounds_unrepresentable_coprime_sizes_within_the_same_ceiling_and_refuses_invalid_authored_size() {
    let Some(mut h) = live_cpu_fixture() else { return };
    h.app.render_workspace.view.live_paused = true;
    h.app.render_workspace.view.draft.width = 997;
    h.app.render_workspace.view.draft.height = 563;
    h.app.render_workspace.view.raytrace.progressive.as_mut().unwrap().samples_per_pixel = 3;
    let document = h.app.session.doc.clone();
    let history = (h.app.session.undo.len(), h.app.session.redo.len());
    for dimensions in [egui::vec2(1100.0, 760.0), egui::vec2(1440.0, 1600.0)] {
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, dimensions);
        h.app.viewport.rect = Some(viewport);
        live::advance(&mut h.app, &h.ctx, 300.0);
        let state = live::inspect(&h.app);
        let width = state["request"]["settings"]["width"].as_u64().unwrap();
        let height = state["request"]["settings"]["height"].as_u64().unwrap();
        assert!(width > 0 && height > 0 && width <= 512 && height <= 512);
        assert!((width * 563).abs_diff(height * 997) <= 997, "nearest-pixel fallback must remain within one source-aspect pixel: {state}");
        let frame = live::projection_rect(&h.app, shot::render_rect(&h.app, viewport));
        assert!((f64::from(frame.width() / frame.height()) - width as f64 / height as f64).abs() < 1e-5);
        assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (997, 563));
    }
    for [width, height] in [[0, 120], [160, 0], [u32::MAX, 120], [160, u32::MAX]] {
        h.app.render_workspace.view.draft.width = width;
        h.app.render_workspace.view.draft.height = height;
        live::advance(&mut h.app, &h.ctx, 301.0);
        let state = live::inspect(&h.app);
        assert!(state["request"]["error"].as_str().is_some(), "a small live copy cannot make invalid output {width}x{height} valid: {state}");
        assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (width, height));
        assert_eq!(state["pending"], false);
        assert_eq!(state["running"], false);
    }
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
}

#[test]
fn cpu_aging_saved_and_runtime_requests_retain_the_same_stack_camera_and_other_effect_intent() {
    use solidcraft_engine::render::job::aging::{AgingKind, AgingLayer};
    let mut h = Harness::new();
    h.app.render_workspace.view.live_paused = true;
    h.app.preferences.render_legacy_image_defaults = false;
    h.app.run("solid.box", json!({"body_name":"Aged body","length":10,"width":10,"height":2})).unwrap();
    h.app.run("render.settings.set", json!({"name":"CPU aging","set":{"width":8,"height":8,"camera":h.app.cam,
        "job":{"method":"cpu_raytrace","transparent_background":true,"options":{"progressive":{"samples_per_pixel":3,"seed":81},"denoise":true,"path":{"max_depth":2},"output":{"linear_exr":true}}}}})).unwrap();
    h.app.render_workspace.view.name = "CPU aging".into();
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.live_paused = true;
    setups::load(&mut h.app);
    let targets = materials::read_targets(&mut h.app).unwrap();
    let identity = targets.iter().find(|target| target.name == "Aged body").unwrap().stable_target.clone().unwrap();
    h.app.render_workspace.view.studio.aging.push(AgingLayer::preset(AgingKind::Dust)).unwrap();
    h.app.render_workspace.view.studio.aging.get_mut(0).unwrap().target = Some(identity.clone());
    let camera = h.app.cam;
    let before = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    setups::save(&mut h.app).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.doc.features, before.features);
    assert_eq!(h.app.session.doc.materials, before.materials);
    let expected = image_request(&h.app).unwrap();
    assert_eq!(expected["settings"]["job"]["options"]["aging"][0]["target"]["definition"], "Aged body");
    assert!(expected["settings"]["job"]["options"]["aging"][0]["target"].get("body_index").is_none());
    assert_eq!(expected["settings"]["job"]["options"]["path"]["max_depth"], 2);
    assert_eq!(expected["settings"]["job"]["options"]["denoise"], true);
    assert_eq!(expected["settings"]["job"]["transparent_background"], true);
    assert_eq!(expected["settings"]["job"]["options"]["output"]["linear_exr"], true);
    let reopened = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
    h.app.session = Session::new(reopened);
    reset_document(&mut h.app);
    h.app.render_workspace.view.live_paused = true;
    h.app.render_workspace.view.name = "CPU aging".into();
    setups::load(&mut h.app);
    assert_eq!(h.app.cam, camera, "numeric reload never navigates the camera");
    assert_eq!(image_request(&h.app).unwrap(), expected);
    assert_eq!(h.app.render_workspace.view.studio.aging.get(0).unwrap().target.as_ref(), Some(&identity));
    // A temporary reflection binding forces the runtime route. The stable aging
    // intent must travel beside it, rather than being lost with the typed job.
    h.app.render_workspace.view.raytrace.path = None;
    h.app.session.execute("render.scene", &json!({})).unwrap();
    cpu::set_reflection(&mut h.app, "Aged body", Some(solidcraft_engine::render::job::ReflectionMode::RoughSpecular)).unwrap();
    let runtime = image_request(&h.app).unwrap();
    assert_eq!(runtime["aging"], expected["settings"]["job"]["options"]["aging"]);
    assert_eq!(runtime["raytrace"]["denoise"], true);
    assert_eq!(runtime["transparent_background"], true);
    assert_eq!(runtime["raytrace"]["output"]["linear_exr"], true);
    assert!(runtime["settings"].get("job").is_none_or(serde_json::Value::is_null));
    assert_eq!(h.app.cam, camera);
    assert!(h.app.render_workspace.view.job.is_none());
}

// Dropdown IDs are egui state salts, not scenario handles. Find the actual
// current visible Add Light control inside the selected lighting editor.
fn click_gpu_light_add(h: &mut Harness, kind: &str) {
    h.frames(3);
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::GpuRaytrace);
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Lighting);
    assert_eq!(inspector::selected_editor(&h.app), inspector::Editor::Lighting);
    let editor = crate::scenario::rect_of("render:lighting:editor").unwrap();
    fn painted_label(shape: &egui::Shape, clip: egui::Rect, control: egui::Rect, label: &str) -> bool {
        match shape {
            egui::Shape::Text(text) => text.galley.text() == label && clip.contains(text.pos) && control.contains(text.pos),
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| painted_label(shape, clip, control, label)),
            _ => false,
        }
    }
    let label = h.app.tr("render.tree_add_light").to_string();
    let tree = h.output.accesskit_update.as_ref().unwrap();
    let controls: Vec<_> = tree
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == egui::accesskit::Role::ComboBox && !node.is_disabled())
        .filter_map(|(_, node)| node.bounds())
        .map(|b| egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32)))
        .filter(|rect| {
            editor.contains(rect.center())
                && h.ctx.content_rect().contains_rect(*rect)
                && h.shapes.iter().any(|shape| painted_label(&shape.shape, shape.clip_rect, *rect, &label))
        })
        .collect();
    assert_eq!(controls.len(), 1, "one current fully visible Add Light dropdown inside the lighting editor");
    let point = controls[0].center();
    h.step(&json!({"click":[point.x,point.y]})).unwrap();
    h.frames(2);
    let target = format!("render:lighting:add:{kind}");
    let row = crate::scenario::rect_of(&target).unwrap();
    assert!(h.ctx.content_rect().contains_rect(row), "the fresh popup row is fully visible");
    h.step(&json!({"click":{"handle":target}})).unwrap();
}

// These portable GPU requests carry lighting in the typed saved-job contract.
// Do not accept a missing top-level runtime field as a numeric light value.
fn gpu_lighting_request(request: &serde_json::Value) -> &serde_json::Value {
    assert!(request.get("lighting").is_none());
    assert!(request.get("progressive").is_none());
    assert_eq!(request["settings"]["job"]["method"], "gpu_path");
    let lighting = &request["settings"]["job"]["options"]["lighting"];
    assert!(lighting.is_object());
    lighting
}

fn edit_gpu_light_number(h: &mut Harness, key: &str, text: &str) {
    h.frames(3);
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::GpuRaytrace);
    assert!(matches!(h.app.render_workspace.view.selection, RenderSelection::Light(_)));
    let target = format!("render:light:{key}");
    let field = crate::scenario::rect_of(&target).unwrap();
    assert!(h.ctx.content_rect().contains_rect(field), "edit only a current fully visible numeric control: {key}");
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":target},"double":true})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":text})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    h.frames(3);
    assert_eq!(h.app.session.undo.len(), undo + 1, "one authored edit for {key}");
}

#[test]
fn gpu_hard_directional_controls_materialize_three_defaults_and_keep_sun_separate() {
    use solidcraft_engine::schema::render_lights::Geometry;
    let mut h = render_lights_fixture();
    h.call("ui.resize", json!({"width":1440,"height":3000}));
    h.app.run("render.settings.set", json!({"name":"Studio","set":{"job":{"method":"gpu_path","options":{}}}})).unwrap();
    setups::load(&mut h.app);
    let camera = h.app.cam;
    let initial = light::rows(&h.app).unwrap();
    assert_eq!(initial.len(), 3);
    assert!(initial.iter().all(|l| l.implicit));
    h.app.run("view.render_select", json!({"kind":"light","id":"directional:1"})).unwrap();
    let original = h.app.session.doc.clone();
    h.frames(3);
    assert!(Arc::ptr_eq(&original, &h.app.session.doc), "opening a zero-softness hard source never normalizes it into a Sun");
    h.step(&json!({"widget":h.app.tr("render.light_duplicate").to_string()})).unwrap();
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Light("directional:3".into()));
    let duplicated = light::rows(&h.app).unwrap();
    assert_eq!(duplicated.len(), 4);
    for (a, b) in duplicated.iter().zip(&initial) {
        assert_eq!((a.colour, a.intensity), (b.colour, b.intensity));
        let (Geometry::Directional { direction: a, angular_radius_rad: None }, Geometry::Directional { direction: b, angular_radius_rad: None }) =
            (a.geometry, b.geometry)
        else {
            panic!("hard sources")
        };
        assert_eq!(a, b);
    }
    h.app.run("view.render_select", json!({"kind":"lighting"})).unwrap();
    h.frames(2);
    click_gpu_light_add(&mut h, "sun");
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Light("directional:0".into()));
    let sun = light::rows(&h.app).unwrap()[0].clone();
    let Geometry::Directional { angular_radius_rad: Some(radius), .. } = sun.geometry else { panic!("finite Sun") };
    assert_eq!(radius, 0.02);
    h.frames(2);
    let before = h.app.session.doc.clone();
    h.step(&json!({"widget":h.app.tr("render.light_duplicate").to_string()})).unwrap();
    assert!(Arc::ptr_eq(&before, &h.app.session.doc), "a finite Sun cannot silently become a second hard source");
    h.app.run("view.render_select", json!({"kind":"lighting"})).unwrap();
    h.frames(2);
    click_gpu_light_add(&mut h, "directional");
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Light("directional:5".into()));
    let before = h.app.session.doc.clone();
    h.frames(3);
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    assert_eq!(h.app.render_workspace.view.studio.directional_lights.iter().count(), 5);
    // Literal world axis, independent of the angle/position adapter and camera.
    for (key, value) in [("direction:0", "1"), ("direction:1", "0"), ("direction:2", "0")] {
        edit_gpu_light_number(&mut h, key, value);
    }
    assert_eq!(light::rows(&h.app).unwrap().last().unwrap().geometry, Geometry::Directional { direction: [1.0, 0.0, 0.0], angular_radius_rad: None });
    assert_eq!(h.app.cam, camera);
    let request = image_request(&h.app).unwrap();
    assert_eq!(gpu_lighting_request(&request)["directional_lights"].as_array().unwrap().len(), 5);
    assert_eq!(gpu_lighting_request(&request)["directional_lights"][4]["direction"], json!([1.0, 0.0, 0.0]));
    let before = h.app.session.doc.clone();
    h.step(&json!({"widget":h.app.tr("render.lighting_solo").to_string()})).unwrap();
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    assert!(h.app.render_workspace.view.lights.visibility.active());
    let visible = image_request(&h.app).unwrap();
    let sources = gpu_lighting_request(&visible)["directional_lights"].as_array().unwrap();
    assert!(sources.iter().take(4).all(|l| l["intensity"] == 0.0));
    assert!(sources[4]["intensity"].as_f64().unwrap() > 0.0);
    h.step(&json!({"widget":h.app.tr("render.lighting_solo").to_string()})).unwrap();
    h.step(&json!({"widget":h.app.tr("render.lighting_mute").to_string()})).unwrap();
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    let muted = image_request(&h.app).unwrap();
    assert_eq!(gpu_lighting_request(&muted)["directional_lights"][4]["intensity"].as_f64(), Some(0.0));
    h.step(&json!({"widget":h.app.tr("render.lighting_mute").to_string()})).unwrap();
    h.step(&json!({"widget":h.app.tr("render.light_delete").to_string()})).unwrap();
    assert_eq!(light::rows(&h.app).unwrap().len(), 5);
    h.app.run("edit.undo", json!({})).unwrap();
    setups::load(&mut h.app);
    assert_eq!(light::rows(&h.app).unwrap().len(), 6);
    assert_eq!(solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap(), *h.app.session.doc);
}

#[test]
fn gpu_sun_insert_and_remove_remap_existing_hard_source_visibility_selection_and_editor_draft() {
    use solidcraft_engine::schema::render_lights::Geometry;
    for solo in [false, true] {
        let mut h = render_lights_fixture();
        h.call("ui.resize", json!({"width":1440,"height":3000}));
        h.app.run("render.settings.set", json!({"name":"Studio","set":{"job":{"method":"gpu_path","options":{}}}})).unwrap();
        setups::load(&mut h.app);
        h.frames(3);
        h.step(&json!({"click":{"handle":"render:tree:light:0"}})).unwrap();
        let original = light::rows(&h.app).unwrap()[0].clone();
        assert_eq!(original.name_key, "render.light_default_key");
        let toggle = if solo { "render.lighting_solo" } else { "render.lighting_mute" };
        h.step(&json!({"widget":h.app.tr(toggle).to_string()})).unwrap();
        h.frames(2);
        assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Light("directional:0".into()));
        let draft = light::editor_draft(&h.app).unwrap();
        assert_eq!(draft.0, "directional:0");
        assert_eq!(draft.2, 3.0_f64.log2());
        assert_eq!(draft.3, 0.0);
        h.step(&json!({"click":{"handle":"render:tree:lights"}})).unwrap();
        click_gpu_light_add(&mut h, "sun");
        h.frames(2);
        assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Light("directional:0".into()), "Add selects the actual new Sun");
        let sun_draft = light::editor_draft(&h.app).unwrap();
        assert_eq!(sun_draft.2, 0.0, "the new Sun must not reuse the old hard source's EV draft at the same old address");
        assert_eq!(sun_draft.3, 0.02_f64.to_degrees());
        let current = light::rows(&h.app).unwrap();
        assert!(matches!(current[0].geometry, Geometry::Directional { angular_radius_rad: Some(_), .. }));
        assert_eq!(current[1].geometry, original.geometry);
        assert_eq!((current[1].colour, current[1].intensity), (original.colour, original.intensity));
        assert_eq!(current[1].name_key, "render.light_default_fill", "display-only fallback names follow their current ordinal");
        assert_eq!(h.app.render_workspace.view.lights.visibility.solo.as_deref(), solo.then_some("directional:1"));
        assert_eq!(h.app.render_workspace.view.lights.visibility.muted.contains("directional:1"), !solo);
        assert!(!h.app.render_workspace.view.lights.visibility.muted.contains("directional:0"));
        let request = image_request(&h.app).unwrap();
        assert_eq!(gpu_lighting_request(&request)["directional_lights"][0]["intensity"].as_f64(), Some(if solo { 3.0 } else { 0.0 }));
        assert_eq!(gpu_lighting_request(&request)["sun"]["radiance"][0].as_f64(), Some(if solo { 0.0 } else { 1.0 }));
        h.step(&json!({"click":{"handle":"render:tree:light:1"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Light("directional:1".into()));
        assert_eq!(light::editor_draft(&h.app).unwrap().2, 3.0_f64.log2());
        let document = h.app.session.doc.clone();
        let visibility = h.app.render_workspace.view.lights.visibility.clone();
        let selected = h.app.render_workspace.view.selection.clone();
        let draft = light::editor_draft(&h.app);
        assert!(light::edit(&mut h.app, "render.lights.add", None, Some(json!({"kind":"directional","softness":0.5}))).is_err());
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!(h.app.render_workspace.view.lights.visibility, visibility);
        assert_eq!(h.app.render_workspace.view.selection, selected);
        assert_eq!(light::editor_draft(&h.app), draft, "a rejected edit applies no identity map or cache invalidation");
        if solo {
            // Programmatic removal of a different light preserves the selected hard source.
            light::edit(&mut h.app, "render.lights.remove", Some("directional:0"), None).unwrap();
            assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Light("directional:0".into()));
        } else {
            h.step(&json!({"click":{"handle":"render:tree:light:0"}})).unwrap();
            h.step(&json!({"widget":h.app.tr("render.light_delete").to_string()})).unwrap();
            assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Nothing);
            h.step(&json!({"click":{"handle":"render:tree:light:0"}})).unwrap();
        }
        h.frames(2);
        let actual = light::rows(&h.app).unwrap()[0].clone();
        assert_eq!(actual.geometry, original.geometry);
        assert_eq!(actual.name_key, original.name_key);
        assert_eq!(h.app.render_workspace.view.lights.visibility.solo.as_deref(), solo.then_some("directional:0"));
        assert_eq!(h.app.render_workspace.view.lights.visibility.muted.contains("directional:0"), !solo);
        assert_eq!(light::editor_draft(&h.app).unwrap().0, "directional:0");
        let request = image_request(&h.app).unwrap();
        assert!(gpu_lighting_request(&request)["sun"].is_null());
        assert_eq!(gpu_lighting_request(&request)["directional_lights"][0]["intensity"].as_f64(), Some(if solo { 3.0 } else { 0.0 }));
    }
}

#[test]
fn unrelated_gpu_settings_override_cannot_inherit_old_light_identity_state() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    use solidcraft_engine::render::job::{DirectionalLightOptions, DirectionalLights, StudioLighting};
    let mut h = render_lights_fixture();
    h.app.run("render.settings.set", json!({"name":"Studio","set":{"job":{"method":"gpu_path","options":{}}}})).unwrap();
    setups::load(&mut h.app);
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:tree:light:0"}})).unwrap();
    h.step(&json!({"widget":h.app.tr("render.lighting_solo").to_string()})).unwrap();
    h.app.run("render.lights.mute", json!({"id":"directional:1","mute":true})).unwrap();
    let mut unrelated = setups::snapshot(&h.app).unwrap();
    let Some(RenderJob::GpuPath { options, .. }) = &mut unrelated.job else { panic!("gpu") };
    options.lighting = Some(StudioLighting {
        directional_lights: DirectionalLights::new(&[DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0, 0.0, 0.0], intensity: 9.0 }])
            .unwrap(),
        ..Default::default()
    });
    h.app.run("render.lights.add", json!({"name":"Studio","settings":unrelated,"set":{"kind":"directional"}})).unwrap();
    assert!(!h.app.render_workspace.view.lights.visibility.active(), "verified mapping belongs to the supplied override, not the old UI rows");
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Light("directional:1".into()));
    let request = image_request(&h.app).unwrap();
    assert_eq!(gpu_lighting_request(&request)["directional_lights"][0]["intensity"].as_f64(), Some(9.0));
    assert!(gpu_lighting_request(&request)["directional_lights"][1]["intensity"].as_f64().unwrap() > 0.0);
}

#[test]
fn unverified_sun_undo_redo_clears_transient_light_identity_instead_of_reusing_ordinals() {
    for solo in [false, true] {
        let mut h = render_lights_fixture();
        h.call("ui.resize", json!({"width":1440,"height":3000}));
        h.app.run("render.settings.set", json!({"name":"Studio","set":{"job":{"method":"gpu_path","options":{}}}})).unwrap();
        setups::load(&mut h.app);
        h.frames(3);
        h.step(&json!({"click":{"handle":"render:tree:light:0"}})).unwrap();
        let toggle = if solo { "render.lighting_solo" } else { "render.lighting_mute" };
        h.step(&json!({"widget":h.app.tr(toggle).to_string()})).unwrap();
        h.step(&json!({"click":{"handle":"render:tree:lights"}})).unwrap();
        click_gpu_light_add(&mut h, "sun");
        assert_eq!(h.app.render_workspace.view.lights.visibility.solo.as_deref(), solo.then_some("directional:1"));
        assert_eq!(h.app.render_workspace.view.lights.visibility.muted.contains("directional:1"), !solo);
        h.app.run("edit.undo", json!({})).unwrap();
        h.frames(3);
        assert!(!h.app.render_workspace.view.lights.visibility.active());
        assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Nothing);
        assert!(light::editor_draft(&h.app).is_none());
        assert_eq!(light::rows(&h.app).unwrap().len(), 3);
        h.step(&json!({"click":{"handle":"render:tree:light:0"}})).unwrap();
        h.step(&json!({"widget":h.app.tr(toggle).to_string()})).unwrap();
        assert!(h.app.render_workspace.view.lights.visibility.active());
        h.app.run("edit.redo", json!({})).unwrap();
        h.frames(3);
        assert!(!h.app.render_workspace.view.lights.visibility.active(), "a new override after Undo must not move onto the restored Sun");
        assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Nothing);
        assert!(light::editor_draft(&h.app).is_none());
        let request = image_request(&h.app).unwrap();
        assert_eq!(gpu_lighting_request(&request)["sun"]["radiance"][0].as_f64(), Some(1.0));
        assert_eq!(gpu_lighting_request(&request)["directional_lights"][0]["intensity"].as_f64(), Some(3.0));
    }
}

#[test]
fn verified_numerical_light_undo_redo_preserves_same_source_visibility_and_selection() {
    let mut h = render_lights_fixture();
    h.app.run("render.settings.set", json!({"name":"Studio","set":{"job":{"method":"gpu_path","options":{}}}})).unwrap();
    setups::load(&mut h.app);
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:tree:light:0"}})).unwrap();
    h.step(&json!({"widget":h.app.tr("render.lighting_mute").to_string()})).unwrap();
    light::edit(&mut h.app, "render.lights.set", Some("directional:0"), Some(json!({"intensity_ev":2}))).unwrap();
    assert_eq!(light::rows(&h.app).unwrap()[0].intensity, 4.0);
    for (command, intensity) in [("edit.undo", 3.0_f64), ("edit.redo", 4.0)] {
        h.app.run(command, json!({})).unwrap();
        h.frames(3);
        assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Light("directional:0".into()));
        assert!(h.app.render_workspace.view.lights.visibility.muted.contains("directional:0"));
        assert_eq!(light::rows(&h.app).unwrap()[0].intensity, intensity);
        assert_eq!(light::editor_draft(&h.app).unwrap().2, intensity.log2());
    }
}

#[test]
fn same_shape_gpu_light_replacement_or_reorder_has_no_ordinal_identity_proof() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    use solidcraft_engine::render::job::{DirectionalLightOptions, DirectionalLights};
    for reorder in [false, true] {
        let mut h = render_lights_fixture();
        h.app
            .run(
                "render.settings.set",
                json!({"name":"Studio","set":{"job":{"method":"gpu_path","options":{"lighting":{"directional_lights":[
                    {"direction":[0,0,1],"color":[1,0,0],"intensity":2},
                    {"direction":[1,0,1],"color":[0,0,1],"intensity":4}
                ]}}}}}),
            )
            .unwrap();
        setups::load(&mut h.app);
        h.frames(3);
        h.step(&json!({"click":{"handle":"render:tree:light:0"}})).unwrap();
        h.step(&json!({"widget":h.app.tr("render.lighting_solo").to_string()})).unwrap();
        h.app.run("render.lights.mute", json!({"id":"directional:1","mute":true})).unwrap();
        let mut replacement = setups::snapshot(&h.app).unwrap();
        let Some(RenderJob::GpuPath { options, .. }) = &mut replacement.job else { panic!("gpu") };
        let lighting = options.lighting.as_mut().unwrap();
        let mut entries: Vec<_> = lighting.directional_lights.iter().copied().collect();
        if reorder {
            entries.reverse();
        } else {
            entries[0] = DirectionalLightOptions { direction: [0.0, 1.0, 1.0], color: [0.0, 1.0, 0.0], intensity: 9.0 };
        }
        lighting.directional_lights = DirectionalLights::new(&entries).unwrap();
        h.app.run("render.settings.set", json!({"name":"Studio","set":replacement})).unwrap();
        h.frames(3);
        assert!(!h.app.render_workspace.view.lights.visibility.active(), "same kind/count is not evidence of surviving source identity");
        assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Nothing);
        assert!(light::editor_draft(&h.app).is_none());
        let request = image_request(&h.app).unwrap();
        assert_eq!(gpu_lighting_request(&request)["directional_lights"][0]["intensity"].as_f64(), Some(if reorder { 4.0 } else { 9.0 }));
        assert_eq!(gpu_lighting_request(&request)["directional_lights"][1]["intensity"].as_f64(), Some(if reorder { 2.0 } else { 4.0 }));
    }
}

#[test]
fn gpu_point_spot_real_controls_save_degree_cones_ranges_undo_and_reopen_exact_runtime_capture() {
    use solidcraft_engine::schema::render_lights::Geometry;
    let mut h = render_lights_fixture();
    h.call("ui.resize", json!({"width":1440,"height":3000}));
    h.app.run("render.settings.set", json!({"name":"Studio","set":{"job":{"method":"gpu_path","options":{}}}})).unwrap();
    setups::load(&mut h.app);
    h.frames(3);
    let camera = h.app.cam;
    let implicit = light::rows(&h.app).unwrap();
    h.step(&json!({"click":{"handle":"render:tree:lights"}})).unwrap();
    click_gpu_light_add(&mut h, "spot");
    h.frames(3);
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Light("spot:0".into()));
    for (a, b) in light::rows(&h.app).unwrap().iter().take(3).zip(&implicit) {
        assert_eq!((a.geometry, a.intensity, a.colour), (b.geometry, b.intensity, b.colour));
        assert!(!a.implicit);
    }
    let before = h.app.session.doc.clone();
    let revision = h.app.session.revision;
    let undo = h.app.session.undo.len();
    h.frames(5);
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    assert_eq!((h.app.session.revision, h.app.session.undo.len()), (revision, undo));
    for (key, text) in [
        ("position_mm:0", "-3.25"),
        ("position_mm:1", "7.5"),
        ("position_mm:2", "120.125"),
        ("emitted_axis:1", "2"),
        ("emitted_axis:0", "1"),
        ("emitted_axis:2", "-2"),
        ("inner_half_angle_radians", "15.125"),
        ("outer_half_angle_radians", "42.5"),
        ("reference_distance_mm", "128.25"),
        ("minimum_distance_mm", "0.02"),
    ] {
        edit_gpu_light_number(&mut h, key, text);
    }
    let source = light::rows(&h.app).unwrap().into_iter().find(|l| l.id == "spot:0").unwrap();
    let Geometry::Spot { position_mm, emitted_axis, inner_half_angle_radians, outer_half_angle_radians, reference_distance_mm, minimum_distance_mm } =
        source.geometry
    else {
        panic!("spot")
    };
    assert_eq!(position_mm, [-3.25, 7.5, 120.125]);
    assert_eq!(emitted_axis, [1.0, 2.0, -2.0]);
    assert_eq!((inner_half_angle_radians, outer_half_angle_radians), (15.125_f64.to_radians(), 42.5_f64.to_radians()));
    assert_eq!((reference_distance_mm, minimum_distance_mm), (128.25, 0.02));
    let expected = setups::snapshot(&h.app).unwrap();
    let request = image_request(&h.app).unwrap();
    assert_eq!(gpu_lighting_request(&request)["punctual_lights"][0]["position_mm"], json!([-3.25, 7.5, 120.125]));
    assert_eq!(gpu_lighting_request(&request)["punctual_lights"][0]["cone"]["emitted_axis"], json!([1.0, 2.0, -2.0]));
    assert_eq!(gpu_lighting_request(&request)["punctual_lights"][0]["cone"]["inner_half_angle_radians"], json!(inner_half_angle_radians));
    assert_eq!(gpu_lighting_request(&request)["punctual_lights"][0]["reference_distance_mm"], 128.25);
    h.app.run("edit.undo", json!({})).unwrap();
    h.frames(3);
    h.app.run("edit.redo", json!({})).unwrap();
    h.frames(3);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    let reopened = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
    h.app.session = solidcraft_engine::Session::new(reopened);
    setups::load(&mut h.app);
    h.frames(3);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    assert_eq!(image_request(&h.app).unwrap(), request);
    assert_eq!(h.app.cam, camera, "editing lights never navigates the product camera");
    h.step(&json!({"click":{"handle":"render:tree:lights"}})).unwrap();
    click_gpu_light_add(&mut h, "point");
    h.frames(3);
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Light("point:1".into()), "Point and Spot share source slots");
    h.step(&json!({"widget":h.app.tr("render.lighting_solo").to_string()})).unwrap();
    let filtered = image_request(&h.app).unwrap();
    assert_eq!(gpu_lighting_request(&filtered)["punctual_lights"][0]["intensity"], 0.0);
    assert_eq!(gpu_lighting_request(&filtered)["punctual_lights"][1]["intensity"], 1.0);
    let saved = solidcraft_engine::render_lights::inspect(&h.app.session.doc.render.setups["Studio"]).unwrap();
    assert_eq!(saved.iter().find(|l| l.id == "spot:0").unwrap(), &source, "solo affects only the runtime copy");
}

#[test]
fn cpu_adaptive_actual_sampling_control_preserves_saved_flags_and_reopens_with_one_undo() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    use solidcraft_engine::render::job::adaptive::AdaptiveSampling;
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2600}));
    h.app.cam.fov = 35.0_f64.to_radians();
    h.app.session.set_render_job_host(None).unwrap();
    h.app
        .run(
            "render.settings.set",
            json!({"name":"Adaptive CPU","set":{"width":64,"height":32,"camera":h.app.cam,"exposure_ev":1.25,
        "job":{"method":"cpu_raytrace","transparent_background":true,"options":{"progressive":{"samples_per_pixel":48,"seed":81},
            "path":{"max_depth":3},"denoise":true,"lens":{"aperture_radius_mm":0.1,"focus_distance_mm":100.0},
            "lights":[],"output":{"tone_map":"aces_fitted","linear_exr":true}}}}}),
        )
        .unwrap();
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.live_paused = true;
    h.app.render_workspace.view.name = "Adaptive CPU".into();
    setups::load(&mut h.app);
    inspector::select_editor(&mut h.app, inspector::Editor::Sampling);
    h.frames(3);
    if crate::scenario::rect_of("render:adaptive-enable").is_none() {
        h.step(&json!({"click":{"handle":format!("dialog:section:{}",h.app.tr("render.children.adaptive"))}})).unwrap();
    }
    h.frames(3);
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let camera = h.app.cam;
    let mut expected = h.app.session.doc.render.setups["Adaptive CPU"].clone();
    let policy = AdaptiveSampling { pilot_samples: 16, min_samples: 16, ..Default::default() };
    let Some(RenderJob::CpuRaytrace { options, .. }) = expected.job.as_mut() else { panic!("CPU job expected") };
    options.adaptive = Some(policy);
    h.frames(3);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    h.step(&json!({"click":{"handle":"render:adaptive-enable"}})).unwrap();
    h.frames(3);
    assert_eq!(
        h.app.session.doc.render.setups["Adaptive CPU"], expected,
        "one actual edit must retain all lens/path/filter/alpha/raw/output/sample/camera intent"
    );
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.cam, camera);
    assert!(h.app.render_workspace.view.job.is_none());
    let saved = image_request(&h.app).unwrap();
    assert_eq!(saved["settings"]["job"]["options"]["adaptive"], json!(policy));
    let mut runtime = json!({"settings":{"camera":camera}});
    cpu::params(&h.app, &mut runtime).unwrap();
    assert_eq!(runtime["raytrace"]["adaptive"], json!(policy));
    assert_eq!(runtime["raytrace"]["progressive"], json!({"samples_per_pixel":48,"seed":81}));
    assert!(runtime.get("adaptive").is_none(), "CPU intent must not enter the top-level GPU adaptive lane");
    h.app.run("edit.undo", json!({})).unwrap();
    setups::sync_revision(&mut h.app);
    assert!(h.app.render_workspace.view.studio.adaptive.is_none());
    h.app.run("edit.redo", json!({})).unwrap();
    setups::sync_revision(&mut h.app);
    assert_eq!(h.app.render_workspace.view.studio.adaptive, Some(policy));
    let container = solidcraft_engine::io::write_design_container(&h.app.session.doc).unwrap();
    let reopened = solidcraft_engine::io::read_design(&container).unwrap();
    assert_eq!(reopened.render.setups["Adaptive CPU"], expected);
    h.app.session = Session::new(reopened);
    reset_document(&mut h.app);
    h.app.render_workspace.view.live_paused = true;
    h.app.render_workspace.view.name = "Adaptive CPU".into();
    setups::load(&mut h.app);
    assert_eq!(h.app.render_workspace.view.studio.adaptive, Some(policy));
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
}

#[test]
fn cpu_adaptive_invalid_draft_is_visible_and_refused_without_silent_size_profile_or_sampling_changes() {
    use solidcraft_engine::render::job::{CpuBudget, CpuProgressiveOptions, adaptive::AdaptiveSampling};
    let mut h = sampling_policy_fixture(false);
    h.app.render_workspace.view.raytrace.progressive = Some(CpuProgressiveOptions { samples_per_pixel: 49, seed: 81 });
    h.app.render_workspace.view.studio.adaptive = Some(AdaptiveSampling { pilot_samples: 16, min_samples: 16, ..Default::default() });
    let before = h.app.render_workspace.view.clone();
    let document = h.app.session.doc.clone();
    let history = (h.app.session.undo.len(), h.app.session.redo.len());
    h.frames(3);
    assert!(cpu::validate_progressive_image(&h.app).is_err());
    assert!(image_request(&h.app).is_err());
    let after = &h.app.render_workspace.view;
    assert_eq!(after.raytrace.progressive, before.raytrace.progressive);
    assert_eq!(after.studio.adaptive, before.studio.adaptive);
    assert_eq!((after.draft.width, after.draft.height, after.raytrace.budget), (before.draft.width, before.draft.height, CpuBudget::Compact));
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(history, (h.app.session.undo.len(), h.app.session.redo.len()));
    assert!(after.job.is_none());
    assert!(
        cpu::profile_for_size(&h.app, u64::from(after.draft.width), u64::from(after.draft.height)).is_some(),
        "an explicit larger profile can be offered without silently selecting it"
    );
}

#[test]
fn cpu_general_glass_path_availability_uses_reviewed_unsaved_saved_optics_without_committing_them() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":3,"body_name":"Glass"})).unwrap();
    h.app.run("render.settings.set", json!({"name":"Reviewed Glass","set":{"job":{"method":"gpu_path","options":{"denoise":true}}}})).unwrap();
    h.app
        .run(
            "render.optics.bind",
            json!({"name":"Reviewed Glass","body_index":0,"body_name":"Glass","expected_revision":h.app.session.revision,
        "optics":{"ior":1.51,"boundary":"closed_mesh","subsurface":{"scattering_per_mm":0.1,"anisotropy":0}}}),
        )
        .unwrap();
    h.app.render_workspace.view.name = "Reviewed Glass".into();
    setups::load(&mut h.app);
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    h.app.render_workspace.view.raytrace.progressive = Some(Default::default());
    h.app.render_workspace.view.raytrace.primary_samples = 1;
    let target = materials::read_targets(&mut h.app).unwrap().remove(0);
    let source = h.app.render_workspace.view.draft.optics.iter().next().unwrap().optics;
    assert!(materials::path_compatibility(&h.app).unwrap_err().contains("subsurface"));
    let document = h.app.session.doc.clone();
    let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    let mut edited = materials::Binding::from_saved(target, &h.app, source);
    edited.glass.subsurface = None;
    h.app.render_workspace.view.materials.enabled = true;
    h.app.render_workspace.view.materials.bindings = vec![edited];
    materials::path_compatibility(&h.app).unwrap();
    assert!(cpu::path_reason(&h.app).is_none());
    assert!(h.app.render_workspace.view.raytrace.path.is_none(), "availability is not adoption");
    assert!(h.app.session.doc.render.setups["Reviewed Glass"].optics.iter().next().unwrap().optics.subsurface.is_some());
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
    // Discarding the review restores the actual saved incompatibility.
    h.app.render_workspace.view.materials.bindings.clear();
    assert!(materials::path_compatibility(&h.app).unwrap_err().contains("subsurface"));
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
}

#[test]
fn render_ui_at_world_points_hit_the_painted_surface_and_letterbox_does_not_select() {
    use solidcore_record::Vec3;
    use solidcraft_engine::Sel;
    for layout in [crate::shell::LayoutMode::Docked, crate::shell::LayoutMode::Floating] {
        for side in ["left", "right"] {
            let mut h = Harness::new();
            h.app.run("solid.box", json!({"corner":[-10,-10,-10],"length":20,"width":20,"height":20,"body_name":"Pick target"})).unwrap();
            enter(&mut h.app, true).unwrap();
            h.app.ui.layout = layout;
            h.app.preferences.render_inspector_side = side.into();
            h.app.preferences.render_browser_side = if side == "left" { "right" } else { "left" }.into();
            h.app.render_workspace.view.draft.width = 160;
            h.app.render_workspace.view.draft.height = 120;
            h.app.cam_anim = None;
            h.app.cam.target = Vec3::ZERO;
            h.app.cam.yaw = 0.0;
            h.app.cam.pitch = std::f64::consts::FRAC_PI_2;
            h.app.cam.roll = 0.0;
            h.app.cam.distance = 100.0;
            h.app.cam.fov = 0.0;
            h.app.run("select.filter", json!({"mode":"body"})).unwrap();
            h.app.run("view.render_select", json!({"kind":"output"})).unwrap();
            for ([width, height], dpi, shots) in [([1100, 760], 1.0, false), ([1440, 1000], 2.0, true)] {
                h.ctx.set_pixels_per_point(dpi);
                h.app.preferences.render_shots_strip = shots;
                h.call("ui.resize", json!({"width":width,"height":height}));
                h.frames(4);
                let camera = h.app.cam;
                let document = h.app.session.doc.clone();
                let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
                let model = h.app.session.model.state();
                let request = image_request(&h.app).unwrap();
                let outer = h.app.viewport.rect.unwrap();
                let frame = crate::scenario::rect_of("render:product:frame").unwrap();
                let world = Vec3::new(5.0, 2.0, 10.0);
                // Independent camera maths in the original authored 160x120 output:
                // no viewport::projection/camera_rect helper participates in this oracle.
                let (x, y, _) = camera.to_screen(world, 160.0, 120.0, 40.0).unwrap();
                let expected = frame.min + frame.size() * egui::vec2((x / 160.0) as f32, (y / 120.0) as f32);
                let at = h.at(&json!({"world":[world.x,world.y,world.z]})).unwrap();
                let actual = egui::pos2(at[0] as f32, at[1] as f32);
                assert!(actual.distance(expected) < 0.01, "ui.at must use the painted output frame: {actual:?} != {expected:?}");
                let generic = crate::viewport::projection(&h.app, outer);
                let explicit = crate::viewport::projection(&h.app, frame);
                assert_eq!(generic.rect, frame);
                assert_eq!(explicit.rect, frame, "an explicit render frame must not be framed twice");
                assert!(generic.to_screen(world).unwrap().distance(expected) < 0.01);
                assert!(explicit.to_screen(world).unwrap().distance(expected) < 0.01);
                assert!(!crate::shell::covered(&h.app, actual));
                h.step(&json!({"move":at})).unwrap();
                assert!(matches!(&h.app.viewport.hover, Some(crate::viewport::Hit::Face { body, .. }) if body == "Pick target"));
                h.step(&json!({"click":at})).unwrap();
                assert_eq!(h.app.session.selection, [Sel::Body { name: "Pick target".into() }]);
                // select.set always refreshes display once, including repeat selection;
                // it does not change the document or create a history entry.
                let selected_history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
                assert_eq!(selected_history, (history.0 + 1, history.1, history.2));
                assert!(Arc::ptr_eq(&h.app.session.model.state(), &model));
                assert!(Arc::ptr_eq(&h.app.session.doc, &document));
                let free = crate::shell::workspace_content(&h.app, crate::render_ui::canvas(&h.app, outer));
                let outside = if frame.left() - free.left() > 2.0 {
                    egui::pos2((free.left() + frame.left()) * 0.5, frame.center().y)
                } else {
                    assert!(frame.top() - free.top() > 2.0);
                    egui::pos2(frame.center().x, (free.top() + frame.top()) * 0.5)
                };
                assert!(free.contains(outside) && !frame.contains(outside));
                h.step(&json!({"click":[outside.x,outside.y]})).unwrap();
                assert_eq!(h.app.session.selection, [Sel::Body { name: "Pick target".into() }], "letterbox cannot select or clear scene geometry");
                assert_eq!(h.app.cam, camera);
                assert_eq!(image_request(&h.app).unwrap(), request);
                assert!(Arc::ptr_eq(&h.app.session.doc, &document));
                assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), selected_history);
                assert!(Arc::ptr_eq(&h.app.session.model.state(), &model), "gray click cannot rebuild geometry");
                assert!(h.app.render_workspace.view.job.is_none());
            }
        }
    }
}

#[test]
fn rendered_camera_layout_cache_and_fit_share_the_exact_frame_without_moving_outer_controls() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        for side in ["left", "right"] {
            let mut h = Harness::new();
            h.app.ui.layout = mode;
            h.call("ui.resize", json!({"width":1440,"height":1600}));
            h.app.run("solid.box", json!({"length":40,"width":30,"height":20,"body_name":"Framed product"})).unwrap();
            enter(&mut h.app, true).unwrap();
            h.app.render_workspace.view.live_paused = true;
            h.app.preferences.render_inspector_side = side.into();
            h.app.preferences.render_browser_side = if side == "left" { "right" } else { "left" }.into();
            h.app.render_workspace.view.draft.width = 160;
            h.app.render_workspace.view.draft.height = 120;
            h.app.cam.fov = 0.6;
            h.app.cam_anim = None;
            setups::save(&mut h.app).unwrap();
            h.frames(4);
            let document = h.app.session.doc.clone();
            let history = (h.app.session.undo.len(), h.app.session.redo.len());
            let fov = h.app.cam.fov;
            for kind in ["shot", "sampling", "effects", "output"] {
                let selection = if kind == "shot" { json!({"kind":kind,"id":h.app.render_workspace.view.name}) } else { json!({"kind":kind}) };
                h.app.run("view.render_select", selection).unwrap();
                h.frames(4);
                let outer = h.app.viewport.rect.unwrap();
                let frame = crate::scenario::rect_of("render:product:frame").unwrap();
                let layout = h.app.viewport.layout.as_ref().unwrap();
                assert_eq!(layout.viewport, frame, "the cached projection must describe painted pixels");
                assert!(frame.contains_rect(layout.drawable));
                assert!(frame.contains_rect(layout.middle));
                assert_eq!(picture_layout(&h.app, layout), *layout, "Fit cannot frame an already-framed cache twice");
                assert_eq!(camera_rect(&h.app, outer), frame);
                assert_eq!(camera_rect(&h.app, frame), frame);
                assert_eq!(crate::viewport::axis_rect(&h.app, outer), layout.axis_view, "outer control calls retain the reserved compass");
                if layout.axis_view.is_positive() {
                    assert!(outer.contains_rect(layout.axis_view));
                }
                if layout.navigation.is_positive() {
                    assert!(outer.contains_rect(layout.navigation));
                }
                h.step(&json!({"key":"F6"})).unwrap();
                h.settle();
                assert_product_fits_canvas(&h.app); // Preserve the preexisting actual-camera/corner oracle.
                assert_eq!(h.app.cam.fov, fov);
                assert!(Arc::ptr_eq(&document, &h.app.session.doc));
                assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
            }
        }
    }
}

#[test]
fn gray_letterbox_gestures_use_the_real_camera_scale_and_never_select_or_assign_geometry() {
    use solidcore_record::Vec3;
    use solidcraft_engine::Sel;
    for fov in [0.0, 0.6] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1440,"height":1600}));
        h.app.run("solid.box", json!({"corner":[-10,-10,-10],"length":20,"width":20,"height":20,"body_name":"Navigation target"})).unwrap();
        enter(&mut h.app, true).unwrap();
        h.app.render_workspace.view.live_paused = true;
        h.app.render_workspace.view.draft.width = 160;
        h.app.render_workspace.view.draft.height = 120;
        h.app.cam = Camera { target: Vec3::ZERO, distance: 100.0, fov, yaw: 0.0, pitch: std::f64::consts::FRAC_PI_2, ..h.app.cam };
        h.app.cancel_view_animation();
        h.app.viewport.nav = Some(crate::viewport::NavMode::Pan);
        h.app.session.selection = vec![Sel::Body { name: "Navigation target".into() }];
        h.frames(4);
        let outer = h.app.viewport.rect.unwrap();
        let frame = crate::scenario::rect_of("render:product:frame").unwrap();
        let free = crate::shell::workspace_content(&h.app, crate::render_ui::canvas(&h.app, outer));
        let from = egui::pos2(frame.center().x, (frame.bottom() + free.bottom()) * 0.5);
        let delta = egui::vec2(30.0, -18.0);
        assert!(free.contains(from) && !frame.contains(from) && !crate::shell::covered(&h.app, from));
        assert!(free.contains(from + delta) && !frame.contains(from + delta));
        let project = |camera: Camera| {
            let (x, y, _) = camera.to_screen(Vec3::ZERO, 160.0, 120.0, 40.0).unwrap();
            frame.min + frame.size() * egui::vec2((x / 160.0) as f32, (y / 120.0) as f32)
        };
        let before = h.app.cam;
        let at = project(before);
        let document = h.app.session.doc.clone();
        let history = (h.app.session.undo.len(), h.app.session.redo.len());
        let selection = h.app.session.selection.clone();
        h.step(&json!({"drag":[[from.x,from.y],[(from+delta).x,(from+delta).y]],"steps":1})).unwrap();
        h.frames(2);
        assert_ne!(h.app.cam.target, before.target, "gray free canvas must accept the shared Pan gesture");
        assert_eq!((h.app.cam.yaw, h.app.cam.pitch, h.app.cam.distance, h.app.cam.fov), (before.yaw, before.pitch, before.distance, before.fov));
        assert!(
            project(h.app.cam).distance(at + delta) < 0.02,
            "world point must follow logical pixels in the actual framed projection, not the outer canvas scale"
        );
        assert_eq!(h.app.session.selection, selection);
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
        assert!(h.app.render_workspace.view.job.is_none());
    }
}

#[test]
fn gray_canvas_empty_space_orbit_ignores_cropped_geometry_and_its_hidden_surface_pivot() {
    use solidcore_record::Vec3;
    use solidcraft_engine::Sel;
    for (preset, ctrl) in [("fusion", true), (crate::prefs::OPENSCAD_NAV, false)] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1440,"height":1600}));
        h.app.run("solid.box", json!({"corner":[-1000,-1000,-10],"length":2000,"width":2000,"height":20,"body_name":"Cropped body"})).unwrap();
        enter(&mut h.app, true).unwrap();
        h.app.render_workspace.view.live_paused = true;
        h.app.render_workspace.view.draft.width = 160;
        h.app.render_workspace.view.draft.height = 120;
        h.app.preferences.nav = preset.into();
        h.app.preferences.orbit_around = crate::prefs::OrbitAround::Cursor;
        h.app.viewport.nav = None;
        h.app.cancel_view_animation();
        h.app.cam = Camera { target: Vec3::ZERO, distance: 100.0, fov: 0.0, yaw: 0.0, pitch: std::f64::consts::FRAC_PI_2, ..h.app.cam };
        h.app.session.selection = vec![Sel::Body { name: "Cropped body".into() }];
        h.frames(4);
        let outer = h.app.viewport.rect.unwrap();
        let frame = crate::scenario::rect_of("render:product:frame").unwrap();
        let free = crate::shell::workspace_content(&h.app, crate::render_ui::canvas(&h.app, outer));
        let from = egui::pos2(frame.center().x, (frame.bottom() + free.bottom()) * 0.5);
        let to = from + egui::vec2(36.0, -24.0);
        assert!(free.contains(from) && !frame.contains(from) && !crate::shell::covered(&h.app, from));
        assert!(free.contains(to) && !frame.contains(to));
        let proj = crate::viewport::projection(&h.app, outer);
        let (origin, direction) = proj.ray(from);
        let distance = (10.0 - origin.z) / direction.z;
        let extrapolated = origin + direction * distance;
        assert!(
            distance > 0.0 && extrapolated.x.abs() < 1000.0 && extrapolated.y.abs() < 1000.0,
            "the invisible continuation really crosses the cropped body's top face"
        );
        assert!(!crate::viewport::pick(&h.app, &proj, frame.center()).is_empty(), "visible geometry remains pickable");
        assert!(crate::viewport::pick(&h.app, &proj, from).is_empty(), "letterbox has no hidden surface");
        assert!(crate::viewport::pick_all(&h.app, &proj, from).is_empty(), "Select Other cannot enumerate invisible surfaces");
        assert!(crate::viewport::pick_for_tool(&h.app, &proj, from).is_empty(), "tool picks obey the same image boundary");
        assert!(crate::select_other::rows(&h.app, &proj, from).is_empty());
        assert!(crate::viewport::pick_cached(&mut h.app, &proj, from).is_empty(), "surface zoom uses the same clipped candidates");
        assert_eq!(crate::prefs::pivot_under(&h.app, &proj, from), Vec3::ZERO, "gray cursor orbit uses the visible body's centre");
        let before = h.app.cam;
        let document = h.app.session.doc.clone();
        let history = (h.app.session.undo.len(), h.app.session.redo.len());
        let selection = h.app.session.selection.clone();
        h.step(&json!({"drag":[[from.x,from.y],[to.x,to.y]],"ctrl":ctrl,"steps":1})).unwrap();
        h.frames(2);
        assert_ne!((h.app.cam.yaw, h.app.cam.pitch), (before.yaw, before.pitch), "{preset}: empty gray canvas accepts the preset's orbit gesture");
        assert!(h.app.cam.target.len() < 1e-9, "the hidden extrapolated surface cannot become the orbit pivot");
        assert_eq!((h.app.cam.distance, h.app.cam.fov), (before.distance, before.fov));
        assert_eq!(h.app.session.selection, selection);
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
        assert!(h.app.render_workspace.view.job.is_none());
    }
}

#[test]
fn light_stage_describes_world_coordinates_without_inert_frame_settings_or_edits() {
    fn draw(h: &mut Harness, width: f32, events: Vec<egui::Event>) -> egui::FullOutput {
        crate::scenario::clear_handles();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, crate::ui_kit::StageBar::HEIGHT));
        let mut output = h.ctx.run_ui(egui::RawInput { screen_rect: Some(rect.expand(20.0)), events, ..Default::default() }, |ui| {
            crate::render_ui::stage_bar(&mut h.app, ui, rect)
        });
        // This fixture inspects shapes and handles without a texture backend.
        output.textures_delta.clear();
        output
    }
    for method in [ImageMethod::Raster, ImageMethod::CpuRaytrace, ImageMethod::GpuRaytrace] {
        for custom in [false, true] {
            let mut h = Harness::new();
            h.app.run("solid.box", json!({"length":10,"width":20,"height":5})).unwrap();
            enter(&mut h.app, true).unwrap();
            h.app.render_workspace.view.method = method;
            h.app.render_workspace.view.draft.width = 32;
            h.app.render_workspace.view.draft.height = 24;
            if custom {
                h.app.render_workspace.view.raytrace.preset = None;
                h.app.render_workspace.view.raytrace.lights =
                    Some(serde_json::from_value(json!([{"direction":[0.0,1.0,0.0],"color":[0.7,1.0,0.4],"intensity":2.0}])).unwrap());
                h.app.render_workspace.view.studio.lighting_enabled = true;
                h.app.render_workspace.view.studio.sun_enabled = true;
                h.app.render_workspace.view.studio.sun.direction = [0.0, 1.0, 0.0];
                if method == ImageMethod::CpuRaytrace {
                    // The read-only header fixture explicitly authors the valid
                    // transport required by its finite Sun, before snapshots.
                    h.app.render_workspace.view.raytrace.path = Some(Default::default());
                    h.app.render_workspace.view.raytrace.progressive = Some(Default::default());
                    h.app.render_workspace.view.raytrace.primary_samples = 1;
                    h.app.render_workspace.view.raytrace.shadows = true;
                }
            }
            h.app.run("view.render_stage", json!({"stage":"light"})).unwrap();
            let document = h.app.session.doc.clone();
            let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
            let camera = h.app.cam;
            let settings = setups::draft(&h.app).unwrap();
            let request = image_request(&h.app).unwrap();
            let lights = light::rows(&h.app).unwrap();
            let preview = light::preview(&h.app).unwrap();
            assert!(!lights.is_empty());
            let lights = serde_json::to_value(lights).unwrap();
            for layout in [crate::shell::LayoutMode::Docked, crate::shell::LayoutMode::Floating] {
                h.app.ui.layout = layout;
                for width in [800.0, 1440.0] {
                    let output = draw(&mut h, width, vec![]);
                    let text = crate::test_support::text_shapes(&output).map(|shape| shape.galley.job.text.clone()).collect::<Vec<_>>();
                    assert!(text.iter().any(|s| s == h.app.tr("render.light_space_world")));
                    for obsolete in ["render.fixed_camera", "render.fixed_world", "render.light_anchor_pending"] {
                        assert!(!text.iter().any(|s| s == h.app.tr(obsolete)), "no inert coordinate-frame control: {obsolete}");
                    }
                    let note = crate::scenario::rect_of("render:light_space_note").unwrap();
                    let bar = crate::scenario::rect_of("render:stage_bar").unwrap();
                    assert!(bar.contains_rect(note), "world-coordinate note fits the shared toolbar");
                    for pressed in [true, false] {
                        draw(
                            &mut h,
                            width,
                            vec![
                                egui::Event::PointerMoved(note.center()),
                                egui::Event::PointerButton {
                                    pos: note.center(),
                                    button: egui::PointerButton::Primary,
                                    pressed,
                                    modifiers: Default::default(),
                                },
                            ],
                        );
                    }
                    assert_eq!(h.app.cam, camera);
                    assert_eq!(setups::draft(&h.app).unwrap(), settings);
                    assert_eq!(image_request(&h.app).unwrap(), request);
                    assert!(std::sync::Arc::ptr_eq(&h.app.session.doc, &document));
                    assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
                    assert!(h.app.render_workspace.view.job.is_none());
                }
            }
            h.app.cam.yaw += 0.7;
            h.app.cam.pitch -= 0.2;
            assert_eq!(serde_json::to_value(light::rows(&h.app).unwrap()).unwrap(), lights, "orbit does not reauthor world-space light geometry");
            assert_eq!(light::preview(&h.app).unwrap(), preview, "preview receives unchanged world-space light values");
            assert!(std::sync::Arc::ptr_eq(&h.app.session.doc, &document));
            assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cpu_subsurface_real_glass_pointer_enable_render_save_undo_and_cold_hydration_keep_optical_intent() {
    use solidcraft_engine::render::job::{CpuHost, Integrator, Phase};
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":3000}));
    h.app.preferences.render_legacy_image_defaults = false;
    let host = CpuHost::for_platform().unwrap();
    let available = host.descriptor_for(Integrator::Raytrace).is_ok();
    h.app.session.set_render_job_host(Some(host.clone())).unwrap();
    h.app.render_workspace.image_host = Some(host.clone());
    h.app.run("solid.box", json!({"length":10,"width":10,"height":2,"body_name":"Scattering"})).unwrap();
    h.frames(3);
    let camera = Camera {
        target: solidcore_record::Vec3::new(5.0, 5.0, 0.5),
        distance: 0.5,
        yaw: 0.0,
        pitch: std::f64::consts::FRAC_PI_2,
        fov: 0.6,
        ..Default::default()
    };
    h.app
        .run(
            "render.settings.set",
            json!({"name":"Studio","set":{"camera":camera,"width":8,"height":8,"job":{"method":"cpu_raytrace","options":{
                "path":{"max_depth":16},"progressive":{"samples_per_pixel":3,"seed":9},"lights":[],
                "environment":{"color":[1,1,1],"intensity":1},"output":{"linear_exr":true}
            }}}}),
        )
        .unwrap();
    h.app
        .run(
            "render.optics.bind",
            json!({"name":"Studio","body_index":0,"body_name":"Scattering","expected_revision":h.app.session.revision,
        "optics":{"ior":1.0,"boundary":"closed_mesh","absorption_per_mm":[0.1,0.2,0.0]}}),
        )
        .unwrap();
    h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body{name:"Scattering".into()}]})).unwrap();
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.live_paused = true;
    h.frames(3);
    assert_eq!(h.app.cam, camera, "opening the authored shot must hydrate its camera");
    let before = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let glass = h.app.tr("render.appearance_editor.glass").to_string();
    h.step(&json!({"click":{"handle":format!("dialog:section:{glass}")}})).unwrap();
    h.step(&json!({"click":{"handle":"render:glass:subsurface-enable"}})).unwrap();
    h.frames(2);
    for (handle, text) in [("render:glass:subsurface-rate", "0.3"), ("render:glass:subsurface-g", "0.37")] {
        let field = crate::scenario::rect_of(handle).unwrap();
        assert!(crate::scenario::rect_of("render:inspector").unwrap().contains_rect(field), "type only into a visible actual numeric field");
        h.step(&json!({"click":[field.center().x,field.center().y],"double":true})).unwrap();
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        h.step(&json!({"text":text})).unwrap();
        h.step(&json!({"key":"Enter"})).unwrap();
        h.frames(2);
    }
    let temporary = image_request(&h.app).unwrap();
    assert_eq!(temporary["settings"]["optics"][0]["optics"]["subsurface"]["scattering_per_mm"], 0.3);
    assert_eq!(temporary["settings"]["optics"][0]["optics"]["subsurface"]["anisotropy"], 0.37);
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    let optics = temporary["settings"]["optics"].clone();
    let target = h.app.render_workspace.view.materials.bindings[0].target.clone();
    select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    h.frames(2);
    let gpu = image_request(&h.app).unwrap();
    assert_eq!(gpu["settings"]["optics"], optics, "method switches preserve full rate, phase bias and stable optical identity");
    assert_eq!(gpu["settings"]["camera"], json!(camera));
    assert_eq!(h.app.render_workspace.view.materials.bindings[0].target, target);
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    h.frames(2);
    assert_eq!(image_request(&h.app).unwrap(), temporary);
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    let old_pixels = if available {
        h.app.session.execute("render.preflight", &temporary).unwrap();
        h.step(&json!({"click":{"handle":"render:stage:render.start"}})).unwrap();
        let handle = h.app.session.render_job_handle(h.app.render_workspace.view.job.unwrap()).unwrap();
        let status = handle.wait_timeout(std::time::Duration::from_secs(60)).unwrap();
        assert_eq!(status.progress.phase, Phase::Completed, "{:?}", status.error);
        let frame = handle.frame().unwrap().unwrap();
        Some((frame.png().to_vec(), frame.linear_exr().unwrap().to_vec()))
    } else {
        assert!(h.app.session.execute("render.preflight", &temporary).is_err(), "offline editing cannot claim a producer");
        None
    };
    h.frames(2);
    h.app.run("render.result.view", json!({"view":"model"})).unwrap();
    h.frames(2);
    let saved_section = h.app.tr("render.appearance_editor.saved").to_string();
    h.step(&json!({"click":{"handle":format!("dialog:section:{saved_section}")}})).unwrap();
    h.step(&json!({"widget":h.app.tr("render.appearance_editor.replace")})).unwrap();
    h.step(&json!({"click":{"handle":"render:optics-save"}})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    let saved = h.app.session.doc.clone();
    assert_eq!(saved.render.setups["Studio"].optics.iter().next().unwrap().optics.subsurface.unwrap().scattering_per_mm, 0.3);
    assert_eq!(saved.features, before.features);
    assert_eq!(saved.materials, before.materials);
    assert_eq!(saved.appearances, before.appearances);
    assert_eq!(saved.render.setups["Studio"].camera, before.render.setups["Studio"].camera);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc, before);
    h.app.run("edit.redo", json!({})).unwrap();
    assert_eq!(h.app.session.doc, saved);
    h.app.session = Session::new(solidcraft_engine::doc::Document::from_json(&saved.to_json()).unwrap());
    h.app.session.set_render_job_host(Some(host)).unwrap();
    reset_document(&mut h.app);
    setups::load(&mut h.app);
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.live_paused = true;
    h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body{name:"Scattering".into()}]})).unwrap();
    h.frames(3);
    assert_eq!(h.app.cam, camera, "cold reopening must retain the shot camera without a helper override");
    let document = h.app.session.doc.clone();
    let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    assert_eq!(image_request(&h.app).unwrap()["settings"]["optics"], temporary["settings"]["optics"]);
    assert!(h.app.render_workspace.view.materials.bindings.is_empty(), "hydration is not a new temporary override");
    if let Some(expected) = old_pixels {
        h.step(&json!({"click":{"handle":"render:stage:render.start"}})).unwrap();
        let handle = h.app.session.render_job_handle(h.app.render_workspace.view.job.unwrap()).unwrap();
        let status = handle.wait_timeout(std::time::Duration::from_secs(60)).unwrap();
        assert_eq!(status.progress.phase, Phase::Completed, "{:?}", status.error);
        let frame = handle.frame().unwrap().unwrap();
        assert_eq!((frame.png().to_vec(), frame.linear_exr().unwrap().to_vec()), expected);
    }
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(history, (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()));
}

fn celestial_editor_fixture(gpu: bool) -> Harness {
    let mut h = Harness::new();
    let camera = solidcraft_engine::render::Camera { distance: 30.0, fov: 0.7, ..Default::default() };
    let sun = json!({"direction":[0.2,-0.3,0.9],"angular_radius_rad":0.02,"radiance":[301.25,202.5,103.75]});
    let sky = json!({"kind":"sky","gradient":{"zenith":[0.25,0.5,1.0],"horizon":[0.75,0.5,0.25],"ground":[0.125,0.25,0.5]},"rotation_rad":0.731,"intensity":1.75});
    let job = if gpu {
        json!({"method":"gpu_path","options":{"samples":32,"max_depth":4,"seed":81,"denoise":true,
        "lighting":{"sun":sun,"environment":[0,0,0]},"environment":sky,"linear_exr":true,
        "lens":{"aperture_radius_mm":0.01,"focus_distance_mm":30}}})
    } else {
        json!({"method":"cpu_raytrace","options":{
        "path":{"max_depth":4},"progressive":{"samples_per_pixel":3,"seed":81},"lights":[],"sun":sun,"sky":sky,"denoise":true,
        "output":{"linear_exr":true},"lens":{"aperture_radius_mm":0.01,"focus_distance_mm":30}}})
    };
    h.app.run("render.settings.set", json!({"name":"Studio","set":{"width":16,"height":12,"camera":camera,"job":job}})).unwrap();
    h.app.cam = camera;
    setups::load(&mut h.app);
    h
}
#[test]
fn cpu_celestial_load_snapshot_undo_and_environment_edit_keep_all_authored_intent() {
    let mut h = celestial_editor_fixture(false);
    let before = h.app.session.doc.render.setups["Studio"].clone();
    let camera = h.app.cam;
    assert_eq!(setups::snapshot(&h.app).unwrap(), before);
    assert_eq!(h.app.render_workspace.view.cpu.environment, cpu::Environment::Sky);
    let document = h.app.session.doc.clone();
    let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    environment_editor_frame(&mut h, vec![]);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(history, (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()));
    let captured = image_request(&h.app).unwrap();
    assert_eq!(captured["settings"]["job"], json!(before.job));
    let mut snapshot = json!({"settings":{}});
    cpu::params(&h.app, &mut snapshot).unwrap();
    assert_eq!(snapshot["raytrace"]["sun"], json!({"direction":[0.2,-0.3,0.9],"angular_radius_rad":0.02,"radiance":[301.25,202.5,103.75]}));
    assert_eq!(snapshot["raytrace"]["sky"]["rotation_rad"], 0.731);
    environment::commit_environment(&mut h.app, json!({"intensity":75.0})).unwrap();
    let changed = h.app.session.doc.render.setups["Studio"].clone();
    let mut expected = before.clone();
    if let Some(solidcraft_engine::doc::render_settings::RenderJob::CpuRaytrace { options, .. }) = &mut expected.job {
        let Some(solidcraft_engine::doc::render_settings::SkyEnvironment::Sky { intensity, .. }) = &mut options.sky else { panic!("Sky expected") };
        *intensity = 75.0;
    }
    assert_eq!(changed, expected);
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.session.undo.len(), history.1 + 1);
    h.app.run("edit.undo", json!({})).unwrap();
    setups::sync_revision(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), before);
    h.app.run("edit.redo", json!({})).unwrap();
    setups::sync_revision(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), changed);
    let reopened = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
    assert_eq!(reopened.render.setups["Studio"], changed);
}
fn celestial_method_frame(h: &mut Harness, events: Vec<egui::Event>) {
    let mut output = h.ctx.run_ui(
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(308.0, 600.0))), events, ..Default::default() },
        |ui| {
            method_dropdown(&mut h.app, ui);
        },
    );
    output.textures_delta.clear();
}
fn celestial_method_click(h: &mut Harness, handle: &str) {
    let at = crate::scenario::handle_at(handle).unwrap();
    for pressed in [true, false] {
        celestial_method_frame(
            h,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() },
            ],
        );
    }
}
#[test]
fn gpu_sky_to_cpu_has_reachable_explicit_indirect_switch_and_one_undo_without_dropping_fields() {
    let mut h = celestial_editor_fixture(true);
    let saved = h.app.session.doc.render.setups["Studio"].clone();
    let camera = h.app.cam;
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.app.run("render.lights.mute", json!({"id":"directional:0","mute":true})).unwrap();
    assert!(switch_celestial_method(&mut h.app, ImageMethod::CpuRaytrace, false).unwrap_err().contains(h.app.tr("render.cpu_celestial_switch")));
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::GpuRaytrace);
    for _ in 0..3 {
        celestial_method_frame(&mut h, vec![]);
    }
    celestial_method_click(&mut h, "render:method");
    celestial_method_frame(&mut h, vec![]);
    let at = crate::scenario::handle_at("render:method-cpu-indirect").unwrap();
    assert!((0.0..=308.0).contains(&at.x));
    celestial_method_click(&mut h, "render:method-cpu-indirect");
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::CpuRaytrace);
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(h.app.render_workspace.view.lights.visibility.muted.contains("directional:0"));
    let filtered = image_request(&h.app).unwrap();
    assert_eq!(filtered["settings"]["job"]["options"]["sun"]["radiance"], json!([0.0, 0.0, 0.0]));
    let cpu = setups::snapshot(&h.app).unwrap();
    cpu.validate().unwrap();
    let Some(solidcraft_engine::doc::render_settings::RenderJob::CpuRaytrace { options, .. }) = &cpu.job else { panic!("CPU expected") };
    let Some(solidcraft_engine::doc::render_settings::RenderJob::GpuPath { options: gpu, .. }) = &saved.job else { panic!("GPU expected") };
    assert_eq!(options.sun, gpu.lighting.unwrap().sun);
    assert_eq!(options.sky, gpu.environment);
    assert_eq!(options.lens, gpu.lens);
    assert_eq!(options.denoise, gpu.denoise);
    assert!(options.output.as_ref().unwrap().linear_exr);
    assert_eq!(options.path.unwrap().max_depth, 4);
    assert_eq!(options.progressive.unwrap(), Default::default());
    assert!(options.shadows);
    h.app.run("edit.undo", json!({})).unwrap();
    setups::sync_revision(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), saved);
    h.app.run("edit.redo", json!({})).unwrap();
    setups::sync_revision(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), cpu);
    switch_celestial_method(&mut h.app, ImageMethod::GpuRaytrace, false).unwrap();
    let roundtrip = setups::snapshot(&h.app).unwrap();
    let Some(solidcraft_engine::doc::render_settings::RenderJob::GpuPath { options: again, .. }) = &roundtrip.job else { panic!("GPU expected") };
    assert_eq!(again.environment, gpu.environment);
    assert_eq!(again.lighting, gpu.lighting);
    assert_eq!(h.app.cam, camera);
}
#[test]
fn cpu_celestial_switch_refuses_incompatible_depth_or_sampling_atomically() {
    for incompatible in ["depth", "adaptive", "transmission"] {
        let mut h = celestial_editor_fixture(true);
        if incompatible == "depth" {
            h.app.render_workspace.view.max_depth = 17;
        } else if incompatible == "transmission" {
            h.app.render_workspace.view.studio.transmission_samples = 4;
        } else {
            h.app.render_workspace.view.raytrace.progressive =
                Some(solidcraft_engine::render::job::CpuProgressiveOptions { samples_per_pixel: 64, seed: 81 });
            h.app.render_workspace.view.studio.adaptive =
                Some(solidcraft_engine::render::job::adaptive::AdaptiveSampling { pilot_samples: 16, min_samples: 16, ..Default::default() });
        }
        let before = setups::draft(&h.app).unwrap();
        let dormant = h.app.render_workspace.view.raytrace.clone();
        let adaptive = h.app.render_workspace.view.studio.adaptive;
        let document = h.app.session.doc.clone();
        let camera = h.app.cam;
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        assert!(switch_celestial_method(&mut h.app, ImageMethod::CpuRaytrace, true).is_err(), "{incompatible}");
        assert_eq!(setups::draft(&h.app).unwrap(), before);
        assert_eq!(h.app.render_workspace.view.raytrace, dormant);
        assert_eq!(h.app.render_workspace.view.studio.adaptive, adaptive);
        assert_eq!(h.app.cam, camera);
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!(history, (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()));
    }
}

#[test]
fn raw_unbound_cpu_celestial_options_cannot_be_lost_by_setup_snapshot_or_runtime_lowering() {
    for sky in [false, true] {
        let mut h = Harness::new();
        h.app.render_workspace.view.method = ImageMethod::CpuRaytrace;
        h.app.render_workspace.view.raytrace.path = Some(Default::default());
        h.app.render_workspace.view.raytrace.progressive = Some(Default::default());
        if sky {
            h.app.render_workspace.view.raytrace.sky = Some(Box::default());
        } else {
            h.app.render_workspace.view.raytrace.sun = Some(Box::new(h.app.render_workspace.view.studio.sun));
        }
        let before = h.app.render_workspace.view.raytrace.clone();
        let document = h.app.session.doc.clone();
        let undo = h.app.session.undo.len();
        assert!(setups::snapshot(&h.app).is_err());
        let mut request = json!({"settings":{}});
        assert!(cpu::params(&h.app, &mut request).is_err());
        assert_eq!(h.app.render_workspace.view.raytrace, before);
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!(h.app.session.undo.len(), undo);
    }
}

#[test]
fn celestial_environment_numerical_edits_keep_saved_camera_and_real_navigation_dirty_even_when_capturing_hdr() {
    for gpu in [false, true] {
        for edit in ["gradient", "intensity", "hdr"] {
            let mut h = celestial_editor_fixture(gpu);
            let saved = h.app.session.doc.render.setups["Studio"].clone();
            let saved_camera = saved.camera;
            h.app.cam.yaw += 0.2;
            let live_camera = h.app.cam;
            setups::refresh_dirty(&mut h.app);
            assert!(h.app.render_workspace.view.dirty);
            let undo = h.app.session.undo.len();
            match edit {
                "gradient" => {
                    if gpu {
                        h.app.render_workspace.view.studio.gradient.zenith[0] = 0.375;
                    } else {
                        h.app.render_workspace.view.cpu.gradient.zenith[0] = 0.375;
                    }
                    environment::persist(&mut h.app);
                }
                "intensity" => environment::commit_environment(&mut h.app, json!({"intensity":75})).unwrap(),
                _ => {
                    let path = h.dir.join("celestial-camera-preserved.hdr");
                    std::fs::write(&path, original_hdr_bytes(128, true)).unwrap();
                    let target = if gpu { hdr::Target::Gpu } else { hdr::Target::Cpu };
                    hdr::inspect(&mut h.app, target, path.to_string_lossy().into_owned()).unwrap();
                    if gpu {
                        h.app.render_workspace.view.studio.environment = studio::Environment::Hdr;
                    } else {
                        h.app.render_workspace.view.cpu.environment = cpu::Environment::Hdr;
                    }
                    environment::persist(&mut h.app);
                    assert_eq!(h.app.session.doc.render.resources.len(), 1);
                }
            }
            let changed = h.app.session.doc.render.setups["Studio"].clone();
            assert_eq!(changed.camera, saved_camera, "GPU={gpu}/{edit}: numerical commits cannot silently Update shot");
            assert_eq!(h.app.cam, live_camera);
            assert_eq!(h.app.session.undo.len(), undo + 1, "one atomic numerical/source transaction");
            setups::refresh_dirty(&mut h.app);
            assert!(h.app.render_workspace.view.dirty, "navigation remains a real draft after {edit}");
            assert_eq!(h.app.render_workspace.view.clean_setup.as_ref().unwrap().camera, saved_camera);
            let old_sun = serde_json::to_value(&saved).unwrap();
            let new_sun = serde_json::to_value(&changed).unwrap();
            let sun_path = if gpu { "lighting" } else { "sun" };
            assert_eq!(old_sun["job"]["options"][sun_path], new_sun["job"]["options"][sun_path], "Sun/rig survives every source edit");
            h.app.run("edit.undo", json!({})).unwrap();
            setups::sync_revision(&mut h.app);
            assert_eq!(h.app.session.doc.render.setups["Studio"], saved);
            assert_eq!(h.app.cam, live_camera);
            h.app.run("edit.redo", json!({})).unwrap();
            setups::sync_revision(&mut h.app);
            assert_eq!(h.app.session.doc.render.setups["Studio"], changed);
            assert_eq!(h.app.cam, live_camera);
        }
    }
}

#[test]
fn celestial_lighting_metadata_uses_actual_profile_without_inventing_a_default_sized_image() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    use solidcraft_engine::render::job::{CpuBudget, CpuProgressiveOptions};
    let mut h = celestial_editor_fixture(false);
    let mut setup = h.app.session.doc.render.setups["Studio"].clone();
    if let Some(RenderJob::CpuRaytrace { options, .. }) = &mut setup.job {
        options.budget = CpuBudget::Studio;
        options.progressive = Some(CpuProgressiveOptions { samples_per_pixel: 128, seed: 81 });
        options.punctual_lights = Some(vec![]);
        options.area_lights = Some(vec![]);
    }
    h.app.run("render.settings.set", json!({"name":"Studio","set":setup})).unwrap();
    setups::load(&mut h.app);
    let camera = h.app.cam;
    let before = h.app.session.doc.clone();
    let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    let rows = light::rows(&h.app).unwrap();
    assert_eq!(rows.len(), 1);
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    assert_eq!(history, (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()));
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (16, 12));
    switch_celestial_method(&mut h.app, ImageMethod::GpuRaytrace, false).unwrap();
    switch_celestial_method(&mut h.app, ImageMethod::CpuRaytrace, false).unwrap();
    assert_eq!(setups::snapshot(&h.app).unwrap(), setup, "only the method transaction's actual authoring size/profile may govern admission");
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.session.undo.len(), history.1 + 2);
}
#[test]
fn celestial_metadata_success_does_not_bypass_exact_full_request_adoption_limits() {
    use solidcraft_engine::render::job::{CpuBudget, CpuProgressiveOptions};
    let mut h = celestial_editor_fixture(true);
    h.app.run("render.settings.set", json!({"name":"Studio","set":{"width":1024,"height":768}})).unwrap();
    setups::load(&mut h.app);
    h.app.render_workspace.view.raytrace.progressive = Some(CpuProgressiveOptions { samples_per_pixel: 64, seed: 81 });
    h.app.render_workspace.view.raytrace.budget = CpuBudget::Compact;
    let mut metadata = h.app.render_workspace.view.clone();
    metadata.raytrace.path = Some(Default::default());
    metadata.raytrace.shadows = true;
    metadata.raytrace.primary_samples = 1;
    light::celestial_method(&h.app, &mut metadata, ImageMethod::CpuRaytrace).unwrap();
    assert_eq!((metadata.draft.width, metadata.draft.height), (1024, 768), "lighting-only validation cannot rewrite the authored image");
    let before = setups::draft(&h.app).unwrap();
    let document = h.app.session.doc.clone();
    let camera = h.app.cam;
    let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    let dormant = h.app.render_workspace.view.raytrace.clone();
    assert!(switch_celestial_method(&mut h.app, ImageMethod::CpuRaytrace, true).is_err(), "1024×768×64 exceeds Compact's unchanged camera budget");
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(setups::draft(&h.app).unwrap(), before);
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.render_workspace.view.raytrace, dormant);
    assert_eq!(history, (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()));
}

fn volume_editor_frame(h: &mut Harness, events: Vec<egui::Event>) {
    let mut output = h.ctx.run_ui(
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(308.0, 1200.0))), events, ..Default::default() },
        |ui| environment::atmosphere(&mut h.app, ui),
    );
    output.textures_delta.clear();
}

fn volume_editor_click(h: &mut Harness, handle: &str) {
    volume_editor_frame(h, vec![]);
    let point = crate::scenario::rect_of(handle).expect("actual visible Atmosphere control").center();
    for pressed in [true, false] {
        volume_editor_frame(
            h,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() },
            ],
        );
    }
}

fn volume_editor_number(h: &mut Harness, handle: &str, text: &str) {
    // DragValue's real double-click text editor, never a guessed neighbouring field.
    volume_editor_click(h, handle);
    volume_editor_click(h, handle);
    let command = egui::Modifiers { ctrl: true, command: true, ..Default::default() };
    volume_editor_frame(h, vec![egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: command }]);
    volume_editor_frame(h, vec![egui::Event::Text(text.into())]);
    volume_editor_frame(
        h,
        vec![egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }],
    );
    volume_editor_frame(h, vec![]);
}

fn cpu_volume_editor_fixture(path: bool, extinction: Option<f64>) -> Harness {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":3,"body_name":"Volume subject"})).unwrap();
    let camera = Camera { target: solidcore_record::Vec3::new(5.0, 5.0, 1.5), distance: 30.0, fov: 0.7, ..Default::default() };
    let volume = extinction.map(|extinction| {
        json!({"min_mm":[-20,-20,-20],"max_mm":[20,20,20],"extinction_per_mm":extinction,
        "scattering_albedo":[0.2,0.4,0.6],"anisotropy":0.3})
    });
    h.app
        .run(
            "render.settings.set",
            json!({"name":"Studio","set":{"camera":camera,"width":16,"height":12,"job":{"method":"cpu_raytrace","options":{
        "primary_samples":if path {1} else {4},"path":path.then_some(json!({"max_depth":4})),
        "progressive":path.then_some(json!({"samples_per_pixel":3,"seed":81})),"lights":[],
        "environment":{"color":[0.5,0.75,1.0],"intensity":0.25},"output":{"linear_exr":true},"volume":volume}}}}),
        )
        .unwrap();
    h.app.cam = camera;
    setups::load(&mut h.app);
    h
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cpu_atmosphere_pointer_fields_preserve_saved_camera_and_roundtrip_with_one_undo_each() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    let mut h = cpu_volume_editor_fixture(true, None);
    let host = solidcraft_engine::render::job::CpuHost::for_platform().unwrap();
    h.app.session.set_render_job_host(Some(host)).unwrap();
    if image_host(&h.app, ImageMethod::CpuRaytrace).is_err() {
        eprintln!("CPU Atmosphere pointer field test requires raytrace-cpu");
        return;
    }
    let saved = h.app.session.doc.render.setups["Studio"].clone();
    h.app.cam.yaw += 0.2;
    let camera = h.app.cam;
    setups::refresh_dirty(&mut h.app);
    let doc = h.app.session.doc.clone();
    let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    for _ in 0..3 {
        volume_editor_frame(&mut h, vec![]);
    }
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(history, (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()));
    for row in ["min", "max", "albedo"] {
        for axis in 0..3 {
            let rect = crate::scenario::rect_of(&format!("render:volume:{row}:{axis}")).unwrap();
            assert!(
                rect.is_positive() && rect.left() >= 0.0 && rect.right() <= 308.0,
                "{row}/{axis}: numeric cell must remain in the narrow inspector: {rect:?}"
            );
        }
    }
    volume_editor_click(&mut h, "render:volume-enable");
    assert_eq!(h.app.session.undo.len(), history.1 + 1);
    let with_box = h.app.session.doc.render.setups["Studio"].clone();
    assert_eq!(with_box.camera, saved.camera);
    assert_eq!(h.app.cam, camera);
    assert!(h.app.render_workspace.view.dirty, "real navigation remains a draft");
    volume_editor_number(&mut h, "render:volume:extinction", "0.0025");
    let changed = h.app.session.doc.render.setups["Studio"].clone();
    let mut expected = saved.clone();
    if let Some(RenderJob::CpuRaytrace { options, .. }) = &mut expected.job {
        options.volume = Some(solidcraft_engine::render::job::volume::HomogeneousVolume { extinction_per_mm: 0.0025, ..Default::default() });
    }
    assert_eq!(changed, expected, "the exact box edit changes no lighting, sampling, output or saved camera field");
    assert_eq!(h.app.session.undo.len(), history.1 + 2, "one command for the finished numeric gesture");
    assert_eq!(h.app.cam, camera);
    assert!(h.app.render_workspace.view.dirty);
    let request = image_params(&h.app).unwrap();
    assert_eq!(request["settings"]["job"]["options"]["volume"]["extinction_per_mm"], 0.0025);
    h.app.run("edit.undo", json!({})).unwrap();
    setups::sync_revision(&mut h.app);
    assert_eq!(h.app.session.doc.render.setups["Studio"], with_box);
    assert_eq!(h.app.render_workspace.view.studio.volume.unwrap().extinction_per_mm, 0.001);
    h.app.run("edit.redo", json!({})).unwrap();
    setups::sync_revision(&mut h.app);
    assert_eq!(h.app.session.doc.render.setups["Studio"], changed);
    let reopened = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
    h.app.session = Session::new(reopened);
    reset_document(&mut h.app);
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap().job, changed.job);
    assert_eq!(h.app.render_workspace.view.raytrace.volume, h.app.render_workspace.view.studio.volume);
    let undo = h.app.session.undo.len();
    volume_editor_click(&mut h, "render:volume-enable");
    assert!(h.app.render_workspace.view.studio.volume.is_none());
    assert!(h.app.render_workspace.view.raytrace.volume.is_none());
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(setups::snapshot(&h.app).is_ok(), "removal cannot leave a stale raw-volume marker");
}

#[test]
fn cpu_atmosphere_explicit_indirect_action_preserves_inactive_and_absent_volume_intent() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    for extinction in [None, Some(0.0)] {
        let mut h = cpu_volume_editor_fixture(false, extinction);
        let before = h.app.session.doc.render.setups["Studio"].clone();
        let camera = h.app.cam;
        let undo = h.app.session.undo.len();
        volume_editor_frame(&mut h, vec![]);
        assert_eq!(h.app.session.doc.render.setups["Studio"], before, "paint never adopts sampling or transport");
        volume_editor_click(&mut h, "render:volume-enable-path");
        let mut expected = before.clone();
        if let Some(RenderJob::CpuRaytrace { options, .. }) = &mut expected.job {
            options.path = Some(Default::default());
            options.progressive = Some(Default::default());
            options.primary_samples = 1;
            options.shadows = true;
        }
        assert_eq!(h.app.session.doc.render.setups["Studio"], expected);
        assert_eq!(h.app.cam, camera);
        assert_eq!(h.app.session.undo.len(), undo + 1);
        assert_eq!(h.app.render_workspace.view.studio.volume.is_some(), extinction.is_some());
        h.app.run("edit.undo", json!({})).unwrap();
        setups::sync_revision(&mut h.app);
        assert_eq!(h.app.session.doc.render.setups["Studio"], before);
        assert!(h.app.render_workspace.view.raytrace.path.is_none());
        if extinction.is_some() {
            volume_editor_click(&mut h, "render:volume-enable");
            assert!(h.app.render_workspace.view.studio.volume.is_none(), "inactive authored box can be removed without enabling transport");
            assert!(h.app.render_workspace.view.raytrace.path.is_none());
        }
    }
}

#[test]
fn cpu_atmosphere_invalid_or_incompatible_edits_refuse_atomically_without_erasing_saved_intent() {
    use solidcraft_engine::render::job::volume::HomogeneousVolume;
    for incompatible in ["path", "denoise", "bounds", "nonfinite"] {
        let mut h = cpu_volume_editor_fixture(incompatible != "path", None);
        if incompatible == "denoise" {
            h.app.render_workspace.view.studio.denoise = true;
        }
        let before = setups::draft(&h.app).unwrap();
        let doc = h.app.session.doc.clone();
        let camera = h.app.cam;
        let view = h.app.render_workspace.view.clone();
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        let mut value = HomogeneousVolume::default();
        if incompatible == "bounds" {
            value.min_mm = value.max_mm;
        }
        if incompatible == "nonfinite" {
            value.extinction_per_mm = f64::NAN;
        }
        assert!(environment::commit_atmosphere(&mut h.app, Some(value)).is_err(), "{incompatible}");
        assert_eq!(setups::draft(&h.app).unwrap(), before);
        assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
        assert_eq!(h.app.cam, camera);
        assert_eq!(h.app.render_workspace.view.raytrace, view.raytrace);
        assert_eq!(h.app.render_workspace.view.studio.volume, view.studio.volume);
        assert_eq!(history, (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()));
    }
}

#[test]
fn authored_atmosphere_method_switch_is_explicit_atomic_and_preserves_common_light_addresses() {
    let mut h = environment_editor_fixture();
    h.app
        .run(
            "render.settings.set",
            json!({"name":"Studio","set":{"width":16,"height":12,"job":{"method":"gpu_path","options":{
        "samples":3,"max_depth":4,"seed":81,"environment":null,"lighting":{"environment":[0.1,0.2,0.3],"directional_lights":[
        {"direction":[0,0,1],"color":[1,1,1],"intensity":2}]},"volume":{"extinction_per_mm":0.001}}}}}),
        )
        .unwrap();
    setups::load(&mut h.app);
    let source = h.app.session.doc.render.setups["Studio"].clone();
    let rows = light::rows(&h.app).unwrap();
    assert_eq!(rows.len(), 1);
    light::loaded(&mut h.app);
    h.app.run("render.lights.mute", json!({"id":rows[0].id,"mute":true})).unwrap();
    let visibility = light::preview(&h.app).unwrap();
    let camera = h.app.cam;
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    assert!(switch_volume_method(&mut h.app, ImageMethod::CpuRaytrace, false).is_err());
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.session.undo.len(), undo);
    switch_volume_method(&mut h.app, ImageMethod::CpuRaytrace, true).unwrap();
    assert_eq!(
        h.app.render_workspace.view.studio.volume,
        source.job.as_ref().and_then(|job| match job {
            solidcraft_engine::doc::render_settings::RenderJob::GpuPath { options, .. } => options.volume,
            _ => None,
        })
    );
    assert_eq!(h.app.render_workspace.view.raytrace.path.unwrap().max_depth, 4);
    assert_eq!(light::preview(&h.app).unwrap(), visibility, "mute identity survives only the proven address-preserving method conversion");
    assert!(h.app.render_workspace.view.lights.visibility.muted.contains(&rows[0].id));
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(light::rows(&h.app).unwrap().iter().zip(rows.iter()).all(|(a, b)| a.id == b.id));
    switch_volume_method(&mut h.app, ImageMethod::GpuRaytrace, false).unwrap();
    assert_eq!(h.app.session.doc.render.setups["Studio"], source);
    assert_eq!(light::preview(&h.app).unwrap(), visibility);
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.session.undo.len(), undo + 2);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn clicked_cpu_atmosphere_start_captures_saved_box_and_reopens_identical_png_and_linear_master() {
    use solidcraft_engine::render::job::{CpuHost, Phase};
    let mut h = cpu_volume_editor_fixture(true, None);
    let host = CpuHost::for_platform().unwrap();
    h.app.session.set_render_job_host(Some(host.clone())).unwrap();
    h.app.render_workspace.image_host = Some(host.clone());
    if image_host(&h.app, ImageMethod::CpuRaytrace).is_err() {
        eprintln!("CPU Atmosphere actual Start test requires raytrace-cpu");
        return;
    }
    h.app.render_workspace.view.live_paused = true;
    h.call("ui.resize", json!({"width":1600,"height":2200}));
    enter(&mut h.app, true).unwrap();
    h.frames(3);
    // Author the same physical box through the common visible control before Start.
    volume_editor_click(&mut h, "render:volume-enable");
    let saved = h.app.session.doc.render.setups["Studio"].clone();
    let camera = h.app.cam;
    let doc = h.app.session.doc.clone();
    let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    inspector::select_editor(&mut h.app, inspector::Editor::Output);
    h.frames(3);
    let request = image_params(&h.app).unwrap();
    let checked = h.app.session.execute("render.preflight", &request).unwrap();
    assert_eq!(checked["request"]["raytrace"]["volume"]["extinction_per_mm"], 0.001);
    h.step(&json!({"click":{"handle":"render:stage:render.start"}})).unwrap();
    let handle = h.app.session.render_job_handle(h.app.render_workspace.view.job.unwrap()).unwrap();
    let terminal = handle.wait_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(terminal.progress.phase, Phase::Completed, "{:?}", terminal.error);
    let image = handle.frame().unwrap().unwrap();
    let png = image.png().to_vec();
    let exr = image.linear_exr().unwrap().to_vec();
    assert!(!png.is_empty() && !exr.is_empty());
    assert_eq!(h.app.cam, camera);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(history, (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()));
    let reopened = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
    assert_eq!(reopened.render.setups["Studio"], saved);
    h.app.session = Session::new(reopened);
    h.app.session.set_render_job_host(Some(host)).unwrap();
    reset_document(&mut h.app);
    h.app.render_workspace.view.live_paused = true;
    setups::load(&mut h.app);
    h.app.cam = camera;
    assert_eq!(image_params(&h.app).unwrap(), request);
    let capture = h.app.session.execute("render.start", &request).unwrap();
    let id = serde_json::from_value(capture["job"].clone()).unwrap();
    let again = h.app.session.render_job_handle(id).unwrap();
    assert_eq!(again.wait_timeout(std::time::Duration::from_secs(30)).unwrap().progress.phase, Phase::Completed);
    let image = again.frame().unwrap().unwrap();
    assert_eq!(image.png(), png);
    assert_eq!(image.linear_exr().unwrap(), exr);
    assert_eq!(h.app.session.doc.features, doc.features);
    assert_eq!(h.app.session.doc.materials, doc.materials);
}

#[test]
fn atmosphere_numeric_pointer_edit_keeps_proven_light_mute_solo_selection_and_its_undo_edge() {
    let mut h = cpu_volume_editor_fixture(true, Some(0.001));
    let mut setup = h.app.session.doc.render.setups["Studio"].clone();
    if let Some(solidcraft_engine::doc::render_settings::RenderJob::CpuRaytrace { options, .. }) = &mut setup.job {
        options.lights = Some(vec![solidcraft_engine::render::job::DirectionalLightOptions {
            direction: [0.0, 0.0, 1.0],
            color: [1.0, 1.0, 1.0],
            intensity: 2.0,
        }]);
    }
    h.app.run("render.settings.set", json!({"name":"Studio","set":setup})).unwrap();
    setups::load(&mut h.app);
    assert!(h.app.render_workspace.view.raytrace.path.is_some());
    assert_eq!(h.app.render_workspace.view.studio.volume.unwrap().extinction_per_mm, 0.001);
    let id = light::rows(&h.app).unwrap()[0].id.clone();
    h.app.run("render.lights.mute", json!({"id":id,"mute":true})).unwrap();
    h.app.run("render.lights.solo", json!({"id":id,"solo":true})).unwrap();
    h.app.render_workspace.view.selection = RenderSelection::Light(id.clone());
    let visibility = h.app.render_workspace.view.lights.visibility.clone();
    let rows = light::rows(&h.app).unwrap();
    let before = h.app.session.doc.render.setups["Studio"].clone();
    let undo = h.app.session.undo.len();
    volume_editor_number(&mut h, "render:volume:extinction", "0.003");
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.render_workspace.view.studio.volume.unwrap().extinction_per_mm, 0.003);
    let after = h.app.session.doc.render.setups["Studio"].clone();
    for (command, expected) in [(None, &after), (Some("edit.undo"), &before), (Some("edit.redo"), &after)] {
        if let Some(command) = command {
            h.app.run(command, json!({})).unwrap();
            setups::sync_revision(&mut h.app);
        }
        assert_eq!(&h.app.session.doc.render.setups["Studio"], expected);
        assert_eq!(light::rows(&h.app).unwrap(), rows);
        assert_eq!(h.app.render_workspace.view.lights.visibility, visibility);
        assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Light(id.clone()));
    }
    // An arbitrary same-shape replacement still has no operation proof.
    h.app.run("render.settings.set", json!({"name":"Studio","set":{"width":17}})).unwrap();
    setups::sync_revision(&mut h.app);
    assert_eq!(h.app.render_workspace.view.lights.visibility, Default::default());
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Nothing);
}

#[test]
fn atmosphere_method_adoption_refuses_actual_image_budget_or_depth_without_partial_transfer() {
    use solidcraft_engine::render::job::{CpuBudget, CpuProgressiveOptions};
    for reason in ["depth", "budget"] {
        let mut h = environment_editor_fixture();
        h.app
            .run(
                "render.settings.set",
                json!({"name":"Studio","set":{"width":1024,"height":768,"job":{"method":"gpu_path","options":{
            "environment":null,"lighting":{"environment":[0.1,0.2,0.3]},"volume":{"extinction_per_mm":0.001},"max_depth":4}}}}),
            )
            .unwrap();
        setups::load(&mut h.app);
        if reason == "depth" {
            h.app.render_workspace.view.max_depth = 17;
        } else {
            h.app.render_workspace.view.raytrace.budget = CpuBudget::Compact;
            h.app.render_workspace.view.raytrace.progressive = Some(CpuProgressiveOptions { samples_per_pixel: 64, seed: 81 });
        }
        let before = setups::draft(&h.app).unwrap();
        let source = h.app.session.doc.clone();
        let camera = h.app.cam;
        let raytrace = h.app.render_workspace.view.raytrace.clone();
        let volume = h.app.render_workspace.view.studio.volume;
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        assert!(switch_volume_method(&mut h.app, ImageMethod::CpuRaytrace, true).is_err(), "{reason}");
        assert!(Arc::ptr_eq(&source, &h.app.session.doc));
        assert_eq!(setups::draft(&h.app).unwrap(), before);
        assert_eq!(h.app.cam, camera);
        assert_eq!(h.app.render_workspace.view.raytrace, raytrace);
        assert_eq!(h.app.render_workspace.view.studio.volume, volume);
        assert_eq!(history, (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()));
    }
}

#[test]
fn first_atmosphere_or_indirect_pointer_commit_keeps_unsaved_default_light_identity() {
    for adopt in [false, true] {
        let mut h = Harness::new();
        assert!(h.app.session.doc.render.setups.is_empty());
        h.app.render_workspace.view.method = ImageMethod::CpuRaytrace;
        h.app.render_workspace.view.draft.width = 16;
        h.app.render_workspace.view.draft.height = 12;
        if !adopt {
            h.app.render_workspace.view.raytrace.path = Some(Default::default());
            h.app.render_workspace.view.raytrace.progressive = Some(Default::default());
            h.app.render_workspace.view.raytrace.primary_samples = 1;
        }
        light::loaded(&mut h.app);
        let rows = light::rows(&h.app).unwrap();
        assert!(!rows.is_empty());
        let id = rows[0].id.clone();
        h.app.run("render.lights.mute", json!({"id":id,"mute":true})).unwrap();
        h.app.run("render.lights.solo", json!({"id":id,"solo":true})).unwrap();
        h.app.render_workspace.view.selection = RenderSelection::Light(id.clone());
        let visibility = h.app.render_workspace.view.lights.visibility.clone();
        let undo = h.app.session.undo.len();
        let camera = h.app.cam;
        volume_editor_frame(&mut h, vec![]);
        assert!(h.app.session.doc.render.setups.is_empty(), "paint cannot create the first setup");
        volume_editor_click(&mut h, if adopt { "render:volume-enable-path" } else { "render:volume-enable" });
        assert!(h.app.session.doc.render.setups.contains_key("Studio"));
        assert!(h.app.render_workspace.view.raytrace.path.is_some());
        assert_eq!(h.app.render_workspace.view.studio.volume.is_some(), !adopt);
        assert_eq!(h.app.session.undo.len(), undo + 1);
        assert_eq!(h.app.cam, camera);
        assert_eq!(light::rows(&h.app).unwrap(), rows);
        assert_eq!(h.app.render_workspace.view.lights.visibility, visibility);
        assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Light(id));
    }
}

#[test]
fn atmosphere_and_celestial_pointer_adoption_is_one_transaction_with_complete_cold_saved_intent() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    use solidcraft_engine::render::job::volume::HomogeneousVolume;
    let mut h = celestial_editor_fixture(true);
    let mut source = h.app.session.doc.render.setups["Studio"].clone();
    if let Some(RenderJob::GpuPath { options, .. }) = &mut source.job {
        options.denoise = false;
        options.volume = Some(HomogeneousVolume { extinction_per_mm: 0.003, anisotropy: -0.4, ..Default::default() });
    }
    h.app.run("render.settings.set", json!({"name":"Studio","set":{"job":source.job}})).unwrap();
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), source);
    let rows = light::rows(&h.app).unwrap();
    let id = rows[0].id.clone();
    h.app.run("render.lights.mute", json!({"id":id,"mute":true})).unwrap();
    h.app.run("render.lights.solo", json!({"id":id,"solo":true})).unwrap();
    h.app.render_workspace.view.selection = RenderSelection::Light(id.clone());
    let visibility = h.app.render_workspace.view.lights.visibility.clone();
    let camera = h.app.cam;
    let before = h.app.session.doc.clone();
    let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    for _ in 0..3 {
        celestial_method_frame(&mut h, vec![]);
    }
    assert!(Arc::ptr_eq(&before, &h.app.session.doc), "paint cannot adopt any transport or effect");
    assert_eq!(history, (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()));
    assert!(switch_volume_method(&mut h.app, ImageMethod::CpuRaytrace, false).is_err());
    assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    celestial_method_click(&mut h, "render:method");
    celestial_method_frame(&mut h, vec![]);
    assert_eq!(crate::scenario::handle_at("render:method-cpu-volume"), crate::scenario::handle_at("render:method-cpu-indirect"));
    celestial_method_click(&mut h, "render:method-cpu-volume");
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::CpuRaytrace);
    assert_eq!(h.app.session.undo.len(), history.1 + 1, "fog, Sky, Sun and transport publish in one command");
    assert_eq!(h.app.cam, camera);
    assert_eq!(light::rows(&h.app).unwrap(), rows);
    assert_eq!(h.app.render_workspace.view.lights.visibility, visibility);
    assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Light(id.clone()));
    let cpu = setups::snapshot(&h.app).unwrap();
    let Some(RenderJob::CpuRaytrace { options, .. }) = &cpu.job else { panic!("CPU expected") };
    let Some(RenderJob::GpuPath { options: gpu, .. }) = &source.job else { panic!("GPU expected") };
    assert_eq!(options.volume, gpu.volume);
    assert_eq!(options.sun, gpu.lighting.unwrap().sun);
    assert_eq!(options.sky, gpu.environment);
    assert_eq!(options.lens, gpu.lens);
    assert_eq!(options.path.unwrap().max_depth, 4);
    assert_eq!(options.progressive.unwrap(), Default::default());
    assert!(options.shadows && !options.denoise && options.output.as_ref().unwrap().linear_exr);
    assert_eq!(image_request(&h.app).unwrap()["settings"]["job"]["options"]["sun"]["radiance"], json!([0.0, 0.0, 0.0]));
    let undo = h.app.session.undo.len();
    volume_editor_number(&mut h, "render:volume:extinction", "0.006");
    let changed = setups::snapshot(&h.app).unwrap();
    let mut expected = cpu.clone();
    if let Some(RenderJob::CpuRaytrace { options, .. }) = &mut expected.job {
        options.volume.as_mut().unwrap().extinction_per_mm = 0.006;
    }
    assert_eq!(changed, expected, "a numeric Atmosphere edit leaves finite Sun, full Sky, lens and output intact");
    assert_eq!(h.app.session.undo.len(), undo + 1);
    for (command, expected) in [(None, &changed), (Some("edit.undo"), &cpu), (Some("edit.redo"), &changed)] {
        if let Some(command) = command {
            h.app.run(command, json!({})).unwrap();
            setups::sync_revision(&mut h.app);
        }
        assert_eq!(&setups::snapshot(&h.app).unwrap(), expected);
        assert_eq!(h.app.cam, camera);
        assert_eq!(h.app.render_workspace.view.lights.visibility, visibility);
        assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Light(id.clone()));
    }
    switch_celestial_method(&mut h.app, ImageMethod::GpuRaytrace, false).unwrap();
    let mut expected_gpu = source.clone();
    if let Some(RenderJob::GpuPath { options, .. }) = &mut expected_gpu.job {
        options.volume.as_mut().unwrap().extinction_per_mm = 0.006;
    }
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected_gpu);
    assert_eq!(h.app.render_workspace.view.lights.visibility, visibility);
    switch_celestial_method(&mut h.app, ImageMethod::CpuRaytrace, false).unwrap();
    assert_eq!(setups::snapshot(&h.app).unwrap(), changed);
    let reopened = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
    let mut cold = Harness::new();
    cold.app.session = Session::new(reopened);
    reset_document(&mut cold.app);
    setups::load(&mut cold.app);
    cold.app.cam = camera;
    let before = cold.app.session.doc.clone();
    let history = (cold.app.session.revision, cold.app.session.undo.len(), cold.app.session.redo.len());
    for _ in 0..3 {
        volume_editor_frame(&mut cold, vec![]);
        environment_editor_frame(&mut cold, vec![]);
    }
    assert_eq!(setups::snapshot(&cold.app).unwrap(), changed);
    assert_eq!(image_request(&cold.app).unwrap()["settings"]["job"], json!(changed.job));
    assert!(Arc::ptr_eq(&before, &cold.app.session.doc));
    assert_eq!(history, (cold.app.session.revision, cold.app.session.undo.len(), cold.app.session.redo.len()));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn clicked_cpu_atmosphere_sun_sky_start_and_cold_saved_capture_keep_identical_png_and_raw_master() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    use solidcraft_engine::render::job::{CpuHost, Phase, volume::HomogeneousVolume};
    let mut h = celestial_editor_fixture(false);
    let mut saved = h.app.session.doc.render.setups["Studio"].clone();
    if let Some(RenderJob::CpuRaytrace { options, .. }) = &mut saved.job {
        options.denoise = false;
        options.volume = Some(HomogeneousVolume {
            min_mm: [-10.0; 3],
            max_mm: [10.0; 3],
            extinction_per_mm: 0.03,
            scattering_albedo: [0.25, 0.5, 0.75],
            anisotropy: 0.2,
        });
    }
    h.app.run("render.settings.set", json!({"name":"Studio","set":{"job":saved.job}})).unwrap();
    let host = CpuHost::for_platform().unwrap();
    h.app.session.set_render_job_host(Some(host.clone())).unwrap();
    h.app.render_workspace.image_host = Some(host.clone());
    setups::load(&mut h.app);
    if image_host(&h.app, ImageMethod::CpuRaytrace).is_err() {
        eprintln!("Combined Atmosphere/Sun/Sky Start test requires raytrace-cpu");
        return;
    }
    h.app.render_workspace.view.live_paused = true;
    h.call("ui.resize", json!({"width":1600,"height":2200}));
    enter(&mut h.app, true).unwrap();
    h.frames(3);
    inspector::select_editor(&mut h.app, inspector::Editor::Output);
    h.frames(3);
    let camera = h.app.cam;
    let source = h.app.session.doc.clone();
    let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    let request = image_params(&h.app).unwrap();
    let check = h.app.session.execute("render.preflight", &request).unwrap();
    assert_eq!(check["request"]["raytrace"]["volume"], json!(h.app.render_workspace.view.studio.volume));
    assert_eq!(check["request"]["raytrace"]["sun"], json!(h.app.render_workspace.view.studio.sun));
    assert_eq!(check["request"]["raytrace"]["sky"]["rotation_rad"], 0.731);
    h.step(&json!({"click":{"handle":"render:stage:render.start"}})).unwrap();
    let handle = h.app.session.render_job_handle(h.app.render_workspace.view.job.unwrap()).unwrap();
    let terminal = handle.wait_timeout(std::time::Duration::from_secs(60)).unwrap();
    assert_eq!(terminal.progress.phase, Phase::Completed, "{:?}", terminal.error);
    let frame = handle.frame().unwrap().unwrap();
    let expected = (frame.png().to_vec(), frame.linear_exr().unwrap().to_vec());
    assert!(!expected.0.is_empty() && !expected.1.is_empty());
    assert!(Arc::ptr_eq(&source, &h.app.session.doc));
    assert_eq!(h.app.cam, camera);
    assert_eq!(history, (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()));
    let reopened = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
    h.app.session = Session::new(reopened);
    h.app.session.set_render_job_host(Some(host)).unwrap();
    reset_document(&mut h.app);
    h.app.render_workspace.view.live_paused = true;
    enter(&mut h.app, true).unwrap();
    h.frames(3);
    assert_eq!(h.app.cam, camera, "cold Render entry restores the one saved physical camera without an override");
    assert_eq!(image_params(&h.app).unwrap(), request);
    inspector::select_editor(&mut h.app, inspector::Editor::Output);
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:stage:render.start"}})).unwrap();
    let handle = h.app.session.render_job_handle(h.app.render_workspace.view.job.unwrap()).unwrap();
    let terminal = handle.wait_timeout(std::time::Duration::from_secs(60)).unwrap();
    assert_eq!(terminal.progress.phase, Phase::Completed, "{:?}", terminal.error);
    let frame = handle.frame().unwrap().unwrap();
    assert_eq!((frame.png().to_vec(), frame.linear_exr().unwrap().to_vec()), expected);
    assert_eq!(h.app.session.doc.features, source.features);
    assert_eq!(h.app.session.doc.materials, source.materials);
    assert_eq!(h.app.session.doc.render.setups["Studio"], source.render.setups["Studio"]);
}

#[test]
fn celestial_and_volume_conversion_does_not_reactivate_a_dormant_sun_in_a_disabled_gpu_rig() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    for volume in [false, true] {
        let mut h = celestial_editor_fixture(true);
        h.app.render_workspace.view.studio.denoise = false;
        if volume {
            h.app.render_workspace.view.studio.volume = Some(Default::default());
        }
        assert!(h.app.render_workspace.view.studio.sun_enabled);
        let dormant = h.app.render_workspace.view.studio.sun;
        h.app.render_workspace.view.studio.lighting_enabled = false;
        let gpu = setups::snapshot(&h.app).unwrap();
        let Some(RenderJob::GpuPath { options, .. }) = &gpu.job else { panic!("GPU expected") };
        assert!(options.lighting.is_none(), "the valid GPU draft emits no Sun from a disabled rig");
        assert!(light::rows(&h.app).unwrap().is_empty());
        let document = h.app.session.doc.clone();
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        let camera = h.app.cam;
        let mut prospective = h.app.render_workspace.view.clone();
        prospective.raytrace.path = Some(Default::default());
        prospective.raytrace.progressive = Some(Default::default());
        prospective.raytrace.shadows = true;
        light::celestial_method(&h.app, &mut prospective, ImageMethod::CpuRaytrace).unwrap();
        assert!(!prospective.studio.sun_enabled);
        assert!(prospective.raytrace.sun.is_none());
        assert_eq!(prospective.studio.sun, dormant, "inactive authored numerical controls remain available");
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!(history, (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()));
        switch_celestial_method(&mut h.app, ImageMethod::CpuRaytrace, true).unwrap();
        let cpu = setups::snapshot(&h.app).unwrap();
        let Some(RenderJob::CpuRaytrace { options, .. }) = &cpu.job else { panic!("CPU expected") };
        assert!(options.sun.is_none());
        assert_eq!(options.volume.is_some(), volume);
        assert!(light::rows(&h.app).unwrap().is_empty());
        assert_eq!(h.app.cam, camera);
        assert_eq!(h.app.session.undo.len(), history.1 + 1);
        let mut params = json!({});
        cpu::params(&h.app, &mut params).unwrap();
        assert!(params["raytrace"].get("sun").is_none(), "runtime lowering must not resurrect the disabled source either");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_preparation_fixture(
    disconnect_retirement: bool,
) -> (Harness, solidcraft_engine::render::job::CpuHost, std::sync::mpsc::SyncSender<()>, std::sync::mpsc::Receiver<bool>, std::sync::mpsc::Receiver<()>)
{
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length":10,"width":20,"height":3})).unwrap();
    let host = solidcraft_engine::render::job::CpuHost::cooperative();
    h.app.session.set_render_job_host(Some(host.clone())).unwrap();
    h.app.render_workspace.image_host = Some(host.clone());
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.live_paused = true;
    h.app.render_workspace.view.method = ImageMethod::Raster;
    h.app.render_workspace.view.draft.width = 3;
    h.app.render_workspace.view.draft.height = 2;
    h.frames(3);
    h.app.run("render.scene", json!({})).unwrap();
    h.app.apply_off_thread = Some(std::time::Duration::ZERO);
    h.app.preferences.agent_off_thread = false;
    let (gate, receiver) = std::sync::mpsc::sync_channel(1);
    h.app.apply_gate = Some(Arc::new(std::sync::Mutex::new(receiver)));
    let (ready, ready_receiver) = std::sync::mpsc::sync_channel(1);
    let (retired, retired_receiver) = std::sync::mpsc::sync_channel(1);
    prepare::test_configure(&mut h.app, ready, retired, disconnect_retirement);
    assert!(prepare::enabled(&h.app), "native rendering cannot depend on the agent geometry preference");
    (h, host, gate, ready_receiver, retired_receiver)
}

#[cfg(not(target_arch = "wasm32"))]
fn native_preparation_wait(h: &mut Harness) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while prepare::busy(&h.app) {
        prepare::tick(&mut h.app, &h.ctx);
        assert!(std::time::Instant::now() < deadline, "preparation did not retire: {}", prepare::inspect(&h.app));
        std::thread::yield_now();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_preparation_start_worker(h: &mut Harness, ready: &std::sync::mpsc::Receiver<bool>) {
    prepare::tick(&mut h.app, &h.ctx);
    assert!(ready.recv_timeout(std::time::Duration::from_secs(30)).unwrap(), "the real capsule must be proved before the deterministic delay");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_check_defers_original_control_reply_until_worker_releases_real_capture() {
    use crate::control::ControlRequest;
    use solidcraft_engine::schema::ParamCheck;
    let (mut h, host, gate, ready, retired) = native_preparation_fixture(false);
    let request = image_params(&h.app).unwrap();
    let expected = h.app.run("render.preflight", request.clone()).unwrap();
    assert_eq!(host.admitted_bytes(), 0);
    let document = h.app.session.doc.clone();
    let model = h.app.session.model.state();
    let camera = h.app.cam;
    let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    h.app.session.param_check = ParamCheck::Off;
    let (control, reply) = ControlRequest::new("engine.execute", json!({"command":"render.preflight","params":request,"param_check":"warn"}));
    h.app.run_control(&h.ctx, control);
    native_preparation_start_worker(&mut h, &ready);
    assert!(host.admitted_bytes() > 0, "delayed proof owns actual billed capture resources");
    assert_eq!(reply.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty), "no fake pending success");
    let (inspect_request, inspect_reply) = ControlRequest::new("ui.inspect", json!({}));
    h.app.run_control(&h.ctx, inspect_request);
    assert_eq!(inspect_reply.try_recv().unwrap()["ok"], true, "other native controls remain responsive");
    assert_eq!(prepare::inspect(&h.app)["kind"], "render.preflight");
    assert_eq!(prepare::inspect(&h.app)["preparing"], true);
    assert!(h.app.render_workspace.view.job.is_none());
    gate.send(()).unwrap();
    native_preparation_wait(&mut h);
    retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    let reply = reply.try_recv().unwrap();
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(reply["param_check"], "warn");
    assert_eq!(reply["result"], expected, "same existing strict preflight contract, no new pending shape");
    assert_eq!(h.app.session.param_check, ParamCheck::Off);
    assert!(h.app.render_workspace.view.inspector.checked.is_some());
    assert!(h.app.render_workspace.view.job.is_none());
    assert_eq!(host.admitted_bytes(), 0);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert!(Arc::ptr_eq(&model, &h.app.session.model.state()));
    assert_eq!(h.app.cam, camera);
    assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_render_commits_only_real_registry_identity_and_retirement_does_not_cancel_it() {
    use solidcraft_engine::render::job::Phase;
    let (mut h, host, gate, ready, retired) = native_preparation_fixture(false);
    let document = h.app.session.doc.clone();
    let model = h.app.session.model.state();
    let camera = h.app.cam;
    let history = (h.app.session.undo.len(), h.app.session.redo.len());
    ribbon::perform_job(&mut h.app, "render.start").unwrap();
    native_preparation_start_worker(&mut h, &ready);
    assert!(h.app.render_workspace.view.job.is_none(), "scratch capture cannot mint a job");
    assert!(h.app.session.render_retained_results().unwrap().is_empty());
    assert!(host.admitted_bytes() > 0);
    gate.send(()).unwrap();
    native_preparation_wait(&mut h);
    retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    let job = h.app.render_workspace.view.job.expect("explicit UI Start follows the committed real-session job");
    let handle = h.app.session.render_job_handle(job).unwrap();
    assert_eq!(handle.status().unwrap().job, job);
    assert!(h.app.render_workspace.view.result);
    assert!(h.app.render_workspace.view.follow_job);
    assert_eq!(h.app.render_workspace.view.job_shot.as_deref(), Some(h.app.render_workspace.view.name.as_str()));
    for _ in 0..10_000 {
        if handle.status().unwrap().progress.phase.terminal() {
            break;
        }
        host.pump(4096).unwrap();
    }
    assert_eq!(handle.status().unwrap().progress.phase, Phase::Completed, "retiring Prepared must not cancel the transferred Start token");
    assert!(handle.frame().unwrap().is_some());
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert!(Arc::ptr_eq(&model, &h.app.session.model.state()));
    assert_eq!(h.app.cam, camera);
    assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
    drop(handle);
    drop(h);
    assert_eq!(host.admitted_bytes(), 0, "normal app teardown releases real retained ownership");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_preparation_rejects_changed_sources_and_keeps_capture_until_worker_ack() {
    use crate::control::ControlRequest;
    enum Change {
        Camera,
        Draft,
        Visibility,
        Host,
        Document,
    }
    for (changed, change) in [
        ("camera", Change::Camera),
        ("draft", Change::Draft),
        ("visibility", Change::Visibility),
        ("host", Change::Host),
        ("document", Change::Document),
    ] {
        let (mut h, host, gate, ready, retired) = native_preparation_fixture(false);
        let request = image_params(&h.app).unwrap();
        let (control, reply) = ControlRequest::new("engine.execute", json!({"command":"render.start","params":request}));
        h.app.run_control(&h.ctx, control);
        native_preparation_start_worker(&mut h, &ready);
        let captured = host.admitted_bytes();
        assert!(captured > 0);
        match change {
            Change::Camera => h.app.cam.yaw += 0.3,
            Change::Draft => h.app.render_workspace.view.draft.width = 4,
            Change::Visibility => h.app.session.visibility.hidden_bodies.push("Body1".into()),
            Change::Host => h.app.session.set_render_job_host(Some(solidcraft_engine::render::job::CpuHost::cooperative())).unwrap(),
            Change::Document => {
                h.app.session = Session::default();
                reset_document(&mut h.app);
            }
        }
        let document = h.app.session.doc.clone();
        let camera = h.app.cam;
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        prepare::tick(&mut h.app, &h.ctx);
        assert!(prepare::busy(&h.app), "{changed}: cancellation is not release");
        assert_eq!(host.admitted_bytes(), captured, "{changed}: source changes do not fake freed capture credit");
        assert_eq!(reply.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
        gate.send(()).unwrap();
        native_preparation_wait(&mut h);
        retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
        let reply = reply.try_recv().unwrap();
        assert_eq!(reply["ok"], false, "{changed}: {reply}");
        assert!(h.app.render_workspace.view.job.is_none());
        assert_eq!(host.admitted_bytes(), 0);
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!(h.app.cam, camera);
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_preparation_actual_cancel_keeps_billed_owner_until_worker_cleanup() {
    let (mut h, host, gate, ready, retired) = native_preparation_fixture(false);
    ribbon::perform_job(&mut h.app, "render.preflight").unwrap();
    native_preparation_start_worker(&mut h, &ready);
    let bytes = host.admitted_bytes();
    h.frames(2);
    assert!(crate::scenario::rect_of("render:preparation-cancel").is_some());
    h.step(&json!({"click":{"handle":"render:preparation-cancel"}})).unwrap();
    assert_eq!(prepare::inspect(&h.app)["cancelled"], h.app.tr("render.preparation_cancelled"));
    assert!(prepare::busy(&h.app));
    assert_eq!(host.admitted_bytes(), bytes);
    assert!(ribbon::action_ready(&h.app, "render.start").is_err());
    gate.send(()).unwrap();
    native_preparation_wait(&mut h);
    retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(host.admitted_bytes(), 0);
    assert!(h.app.render_workspace.view.job.is_none());
    assert!(h.app.session.render_retained_results().unwrap().is_empty());
    assert!(ribbon::action_ready(&h.app, "render.start").is_ok());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_preparation_disconnected_retirement_holds_unavailable_owner_across_document_reset() {
    use crate::control::ControlRequest;
    let (mut h, host, gate, ready, _retired) = native_preparation_fixture(true);
    let request = image_params(&h.app).unwrap();
    let (control, reply) = ControlRequest::new("engine.execute", json!({"command":"render.preflight","params":request}));
    h.app.run_control(&h.ctx, control);
    native_preparation_start_worker(&mut h, &ready);
    let bytes = host.admitted_bytes();
    assert!(bytes > 0);
    gate.send(()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        prepare::tick(&mut h.app, &h.ctx);
        if !prepare::inspect(&h.app)["unavailable"].is_null() {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(reply.try_recv().unwrap()["ok"], false);
    assert!(prepare::busy(&h.app));
    assert_eq!(host.admitted_bytes(), bytes, "disconnected cleanup keeps real credit owned");
    assert!(prepare::begin_ui(&mut h.app, solidcraft_engine::render_preparation::Kind::Check, json!({})).is_err());
    h.app.session = Session::default();
    reset_document(&mut h.app);
    prepare::tick(&mut h.app, &h.ctx);
    assert!(prepare::busy(&h.app));
    assert_eq!(host.admitted_bytes(), bytes, "tab/document replacement cannot destroy unavailable ownership on the window");
    drop(h);
    assert_eq!(host.admitted_bytes(), 0, "true app teardown runs ordinary destructors");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_preparation_window_drop_returns_undelivered_capture_to_its_same_worker() {
    let (mut h, host, gate, ready, retired) = native_preparation_fixture(false);
    ribbon::perform_job(&mut h.app, "render.preflight").unwrap();
    native_preparation_start_worker(&mut h, &ready);
    let bytes = host.admitted_bytes();
    assert!(bytes > 0);
    drop(h);
    assert_eq!(host.admitted_bytes(), bytes, "the delayed worker still owns its capture after window teardown");
    gate.send(()).unwrap();
    retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(host.admitted_bytes(), 0, "closed ready receiver destroys the returned capsule on its original worker");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_preparation_keeps_request_vfs_scope_on_proof_and_window_commit() {
    use crate::control::ControlRequest;
    use std::sync::Mutex;
    let (mut h, host, gate, ready, retired) = native_preparation_fixture(false);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = seen.clone();
    let allowed = h.dir.join("allowed-read.hdr").to_string_lossy().into_owned();
    let rejected = h.dir.parent().unwrap().join("outside-preparation-scope.hdr").to_string_lossy().into_owned();
    h.app.session.set_command_check(Some(Arc::new(move |id, _, _| {
        if id == "render.preflight" {
            let scope = solidcraft_engine::io::vfs::current_scope().expect("original native scope must follow both phases");
            assert!(scope.check(&allowed).is_ok());
            assert!(scope.check(&rejected).is_err());
            recorded.lock().unwrap().push(std::thread::current().id());
        }
        Ok(())
    })));
    let request = image_params(&h.app).unwrap();
    let (control, reply) = ControlRequest::new("engine.execute", json!({"command":"render.preflight","params":request,"allow_paths":[h.dir]}));
    let window = std::thread::current().id();
    h.app.run_control(&h.ctx, control);
    native_preparation_start_worker(&mut h, &ready);
    gate.send(()).unwrap();
    native_preparation_wait(&mut h);
    retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(reply.try_recv().unwrap()["ok"], true);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2, "host policy runs once on preparation and once on real-session commit");
    assert_ne!(seen[0], window);
    assert_eq!(seen[1], window);
    assert_eq!(host.admitted_bytes(), 0);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_control_does_not_synchronously_hide_render_preparation_inside_engine_batch() {
    use crate::control::ControlRequest;
    let (mut h, host, _gate, _ready, _retired) = native_preparation_fixture(false);
    let document = h.app.session.doc.clone();
    let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    for method in ["engine.batch", "engine.script"] {
        let (control, reply) = ControlRequest::new(
            method,
            json!({"commands":[{"command":"solid.box","params":{"length":1}},{"command":"render.preflight"}],"param_check":"strict"}),
        );
        h.app.run_control(&h.ctx, control);
        let reply = reply.try_recv().unwrap();
        assert_eq!(reply["ok"], false, "{method}: {reply}");
        assert_eq!(reply["param_check"], "strict");
        assert!(reply["error"].as_str().unwrap().contains("individual engine.execute"));
    }
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
    assert!(!prepare::busy(&h.app));
    assert_eq!(host.admitted_bytes(), 0);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_preparation_failed_input_delivery_retains_source_until_true_teardown() {
    let (mut h, host, _gate, _ready, _retired) = native_preparation_fixture(false);
    let document = Arc::downgrade(&h.app.session.doc);
    let model = Arc::downgrade(&h.app.session.model.state());
    prepare::test_disconnect_input(&mut h.app);
    ribbon::perform_job(&mut h.app, "render.preflight").unwrap();
    prepare::tick(&mut h.app, &h.ctx);
    prepare::tick(&mut h.app, &h.ctx);
    assert!(!prepare::inspect(&h.app)["unavailable"].is_null());
    assert!(prepare::busy(&h.app));
    assert_eq!(host.admitted_bytes(), 0, "proof never ran, so no renderer lease was invented");
    h.app.session = Session::default();
    reset_document(&mut h.app);
    prepare::tick(&mut h.app, &h.ctx);
    assert!(document.upgrade().is_some(), "returned heavy input remains in the bounded unavailable owner");
    assert!(model.upgrade().is_some());
    assert!(prepare::begin_ui(&mut h.app, solidcraft_engine::render_preparation::Kind::Check, json!({})).is_err());
    drop(h);
    assert!(document.upgrade().is_none(), "true app teardown destroys the unavailable input normally");
    assert!(model.upgrade().is_none());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn headless_render_checks_retain_the_existing_strict_synchronous_api() {
    let (mut h, host, _gate, ready, _retired) = native_preparation_fixture(false);
    h.app.apply_off_thread = None;
    assert!(!prepare::enabled(&h.app));
    ribbon::perform_job(&mut h.app, "render.preflight").unwrap();
    assert!(h.app.render_workspace.view.inspector.checked.is_some());
    assert!(!prepare::busy(&h.app));
    assert_eq!(ready.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
    assert_eq!(host.admitted_bytes(), 0);
    assert!(h.app.render_workspace.view.job.is_none());
}

#[cfg(not(target_arch = "wasm32"))]
fn native_batch_fixture(
    disconnect: bool,
) -> (
    Harness,
    solidcraft_engine::render::job::CpuHost,
    std::sync::mpsc::SyncSender<()>,
    std::sync::mpsc::Receiver<bool>,
    std::sync::mpsc::Receiver<()>,
    [Camera; 2],
) {
    let (mut h, host, gate, ready, retired) = native_preparation_fixture(disconnect);
    let first = h.app.cam;
    let second = Camera { yaw: first.yaw + 0.4, ..first };
    for (name, camera, width, height) in [("A worker shot", first, 5, 4), ("B worker shot", second, 9, 8)] {
        h.app
            .run("render.settings.set", json!({"name":name,"set":{"camera":camera,"width":width,"height":height,"job":{"method":"raster"}}}))
            .unwrap();
    }
    h.app.render_workspace.view.draft.width = 3;
    h.app.render_workspace.view.draft.height = 2;
    h.app.render_workspace.view.draft.background = [7, 19, 31];
    (h, host, gate, ready, retired, [first, second])
}

#[cfg(not(target_arch = "wasm32"))]
fn native_batch_wait(h: &mut Harness, predicate: impl Fn(&SolveApp) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        prepare::tick(&mut h.app, &h.ctx);
        if predicate(&h.app) {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "batch did not reach expected state: {}", prepare::inspect_batches(&h.app));
        std::thread::yield_now();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_batch_ready(h: &mut Harness, ready: &std::sync::mpsc::Receiver<bool>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        prepare::tick(&mut h.app, &h.ctx);
        match ready.try_recv() {
            Ok(value) => {
                assert!(value);
                return;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(e) => panic!("proof hook disconnected: {e}"),
        }
        assert!(std::time::Instant::now() < deadline, "batch worker never prepared an owned result: {}", prepare::inspect_batches(&h.app));
        std::thread::yield_now();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_batch_reply(h: &mut Harness, reply: &std::sync::mpsc::Receiver<serde_json::Value>) -> serde_json::Value {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        prepare::tick(&mut h.app, &h.ctx);
        if let Ok(value) = reply.try_recv() {
            return value;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "original batch sender did not receive its actual completion: {}",
            prepare::inspect_batches(&h.app)
        );
        std::thread::yield_now();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_batch_complete(host: &solidcraft_engine::render::job::CpuHost, job: &solidcraft_engine::render::job::JobHandle) {
    for _ in 0..10_000 {
        if job.status().unwrap().progress.phase.terminal() {
            break;
        }
        host.pump(4096).unwrap();
    }
    assert_eq!(job.status().unwrap().progress.phase, solidcraft_engine::render::job::Phase::Completed);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_batch_defers_first_reply_then_uses_one_common_request_and_each_exact_saved_camera() {
    use crate::control::ControlRequest;
    let (mut h, host, gate, ready, retired, cameras) = native_batch_fixture(false);
    let document = h.app.session.doc.clone();
    let model = h.app.session.model.state();
    let camera = h.app.cam;
    let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    let request = image_params(&h.app).unwrap();
    let (control, reply) =
        ControlRequest::new("engine.execute", json!({"command":"render.batch","params":{"request":request},"param_check":"strict"}));
    h.app.run_control(&h.ctx, control);
    native_batch_ready(&mut h, &ready);
    assert!(host.admitted_bytes() > 0);
    assert!(h.app.session.render_queue_state().job.is_none(), "no scratch or serial-zero identity may stand in for a prepared shot");
    assert!(h.app.render_workspace.view.job.is_none());
    assert_eq!(reply.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
    gate.send(()).unwrap();
    let answer = native_batch_reply(&mut h, &reply);
    retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(answer, json!({"ok":true,"result":{"queued":2},"param_check":"strict"}));
    let first = h.app.session.render_queue_state().job.unwrap();
    let first_handle = h.app.session.render_job_handle(first).unwrap();
    let first_status = first_handle.status().unwrap();
    assert_eq!(first_status.request.camera, cameras[0]);
    assert_eq!((first_status.request.width, first_status.request.height, first_status.request.background), (3, 2, [7, 19, 31]));
    assert_eq!(h.app.render_workspace.view.job, Some(first));
    native_batch_complete(&host, &first_handle);
    native_batch_ready(&mut h, &ready);
    assert_eq!(h.app.session.render_queue_state().job, Some(first), "next proof retains the last real identity instead of inventing a preparing job");
    assert!(prepare::inspect_batches(&h.app)["worker"]["preparing"].as_bool().unwrap());
    gate.send(()).unwrap();
    native_batch_wait(&mut h, |app| {
        app.session.render_queue_state().job.is_some_and(|job| job != first) && prepare::inspect_batches(app)["worker"].is_null()
    });
    retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    let second = h.app.session.render_queue_state().job.unwrap();
    let second_handle = h.app.session.render_job_handle(second).unwrap();
    let second_status = second_handle.status().unwrap();
    assert_eq!(second_status.request.camera, cameras[1]);
    assert_eq!(
        (second_status.request.width, second_status.request.height, second_status.request.background),
        (3, 2, [7, 19, 31]),
        "saved per-shot dimensions never replace the captured common request"
    );
    native_batch_complete(&host, &second_handle);
    h.app.session.advance_render_batch().unwrap();
    assert_eq!((h.app.session.render_queue_state().waiting, h.app.session.render_queue_state().running), (0, 0));
    assert!(prepare::inspect_batches(&h.app)["owners"].as_array().unwrap().is_empty());
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert!(Arc::ptr_eq(&model, &h.app.session.model.state()));
    assert_eq!(h.app.cam, camera);
    assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
    assert_eq!(reply.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected), "the original sender is answered exactly once");
    drop(first_handle);
    drop(second_handle);
    drop(h);
    assert_eq!(host.admitted_bytes(), 0);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_batch_scene_all_shots_pointer_prepares_without_submitting_during_paint() {
    let (mut h, host, gate, ready, retired, _) = native_batch_fixture(false);
    h.call("ui.resize", json!({"width":1600,"height":2200}));
    h.app.run("view.render_select", json!({"kind":"nothing"})).unwrap();
    h.frames(3);
    let document = h.app.session.doc.clone();
    let camera = h.app.cam;
    let history = (h.app.session.undo.len(), h.app.session.redo.len());
    assert!(prepare::inspect_batches(&h.app)["owners"].as_array().unwrap().is_empty());
    assert!(h.app.session.render_queue_state().job.is_none());
    h.step(&json!({"click":{"handle":"render:scene:all-shots"}})).unwrap();
    native_batch_ready(&mut h, &ready);
    assert!(h.app.session.render_queue_state().job.is_none());
    assert!(host.admitted_bytes() > 0);
    ribbon::perform_job(&mut h.app, "render.cancel").unwrap();
    gate.send(()).unwrap();
    native_batch_wait(&mut h, |app| prepare::inspect_batches(app)["owners"].as_array().unwrap().is_empty());
    retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(host.admitted_bytes(), 0);
    assert!(h.app.session.render_queue_state().job.is_none());
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(h.app.cam, camera);
    assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_batch_control_cancel_before_first_job_is_truthful_and_waits_for_cleanup() {
    use crate::control::ControlRequest;
    let (mut h, host, gate, ready, retired, _) = native_batch_fixture(false);
    let request = image_params(&h.app).unwrap();
    let document = h.app.session.doc.clone();
    let camera = h.app.cam;
    let (control, reply) = ControlRequest::new("engine.execute", json!({"command":"render.batch","params":{"request":request}}));
    h.app.run_control(&h.ctx, control);
    native_batch_ready(&mut h, &ready);
    let bytes = host.admitted_bytes();
    assert!(bytes > 0);
    let (cancel, cancel_reply) = ControlRequest::new("engine.execute", json!({"command":"render.cancel","params":{},"param_check":"strict"}));
    h.app.run_control(&h.ctx, cancel);
    let cancelled = cancel_reply.try_recv().unwrap();
    assert_eq!(cancelled["ok"], true);
    assert_eq!(cancelled["param_check"], "strict");
    assert_eq!(cancelled["result"]["preparing"], true);
    assert_eq!(cancelled["result"]["cancel_requested"], true);
    assert!(cancelled["result"].get("job").is_none());
    assert!(h.app.session.render_queue_state().job.is_none());
    assert_eq!(host.admitted_bytes(), bytes, "cancel acknowledgement is not resource release");
    assert_eq!(reply.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
    gate.send(()).unwrap();
    let answer = native_batch_reply(&mut h, &reply);
    assert_eq!(answer["ok"], false);
    retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(host.admitted_bytes(), 0);
    assert!(h.app.session.render_retained_results().unwrap().is_empty());
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(h.app.cam, camera);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_batch_finishes_in_exact_inactive_owner_and_close_cancels_its_claim_without_adoption() {
    use crate::control::ControlRequest;
    for close in [false, true] {
        let (mut h, host, gate, ready, retired, _) = native_batch_fixture(false);
        let owner = h.app.session.uid;
        let (control, reply) =
            ControlRequest::new("engine.execute", json!({"command":"render.batch","params":{"request":image_params(&h.app).unwrap()}}));
        h.app.run_control(&h.ctx, control);
        native_batch_ready(&mut h, &ready);
        crate::documents::open_tab(&mut h.app, Session::default()).unwrap();
        let other = h.app.session.doc.clone();
        let camera = h.app.cam;
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        if close {
            crate::documents::close(&mut h.app, 0, true);
        }
        gate.send(()).unwrap();
        let answer = native_batch_reply(&mut h, &reply);
        retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
        assert_eq!(answer["ok"], !close, "{answer}");
        assert!(h.app.render_workspace.view.job.is_none(), "inactive completion cannot adopt its source into the visible document");
        assert!(Arc::ptr_eq(&other, &h.app.session.doc));
        assert_eq!(h.app.cam, camera);
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
        if !close {
            let session = crate::documents::render_owner(&h.app, owner).unwrap();
            let job = session.render_queue_state().job.unwrap();
            let handle = session.render_job_handle(job).unwrap();
            assert!(handle.status().unwrap().job.session != 0);
            native_batch_complete(&host, &handle);
            crate::documents::close(&mut h.app, 0, true);
            native_batch_wait(&mut h, |app| prepare::inspect_batches(app)["owners"].as_array().unwrap().is_empty());
            retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
            drop(handle);
        }
        drop(h);
        for _ in 0..100 {
            if !host.pump(4096).unwrap() {
                break;
            }
        }
        assert_eq!(host.admitted_bytes(), 0);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_batch_plan_credit_releases_only_after_background_plan_retirement() {
    use crate::control::ControlRequest;
    let (mut h, host, gate, ready, retired, _) = native_batch_fixture(false);
    let (control, reply) =
        ControlRequest::new("engine.execute", json!({"command":"render.batch","params":{"request":image_params(&h.app).unwrap()}}));
    h.app.run_control(&h.ctx, control);
    native_batch_ready(&mut h, &ready);
    gate.send(()).unwrap();
    assert_eq!(native_batch_reply(&mut h, &reply)["ok"], true);
    retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    let job = h.app.session.render_queue_state().job.unwrap();
    let handle = h.app.session.render_job_handle(job).unwrap();
    native_batch_complete(&host, &handle);
    let plan = prepare::inspect_batches(&h.app)["owners"][0]["retained_plan_bytes"].as_u64().unwrap();
    let bytes = host.admitted_bytes();
    assert!(plan > 0 && bytes >= plan);
    h.app.run("render.cancel", json!({"job":job})).unwrap();
    assert_eq!(host.admitted_bytes(), bytes, "metadata cancellation cannot drop funded plan backing on the window");
    native_batch_wait(&mut h, |app| prepare::inspect_batches(app)["owners"].as_array().unwrap().is_empty());
    retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(host.admitted_bytes(), bytes - plan, "only the independently funded shared plan retires; real retained final remains owned");
    assert!(handle.frame().unwrap().is_some());
    drop(handle);
    drop(h);
    assert_eq!(host.admitted_bytes(), 0);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_batch_and_individual_check_share_one_preparation_lane_without_deadlock() {
    use crate::control::ControlRequest;
    let (mut h, host, gate, ready, retired, _) = native_batch_fixture(false);
    let request = image_params(&h.app).unwrap();
    let (batch, batch_reply) = ControlRequest::new("engine.execute", json!({"command":"render.batch","params":{"request":request}}));
    h.app.run_control(&h.ctx, batch);
    native_batch_ready(&mut h, &ready);
    let (check, check_reply) = ControlRequest::new("engine.execute", json!({"command":"render.preflight","params":request}));
    h.app.run_control(&h.ctx, check);
    for _ in 0..8 {
        prepare::tick(&mut h.app, &h.ctx);
    }
    assert_eq!(ready.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty), "the second proof cannot start while the batch worker owns the lane");
    assert_eq!(prepare::inspect(&h.app)["waiting_for_worker"], true);
    gate.send(()).unwrap();
    assert_eq!(native_batch_reply(&mut h, &batch_reply)["ok"], true);
    retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    native_batch_ready(&mut h, &ready);
    assert_eq!(check_reply.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
    gate.send(()).unwrap();
    let checked = native_batch_reply(&mut h, &check_reply);
    assert_eq!(checked["ok"], true);
    retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    let job = h.app.session.render_queue_state().job.unwrap();
    let handle = h.app.session.render_job_handle(job).unwrap();
    assert_eq!(
        handle.status().unwrap().progress.phase,
        solidcraft_engine::render::job::Phase::Queued,
        "a Check cannot mint or replace the actual queued batch job"
    );
    h.app.run("render.cancel", json!({"job":job})).unwrap();
    native_batch_wait(&mut h, |app| prepare::inspect_batches(app)["owners"].as_array().unwrap().is_empty());
    retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    drop(handle);
    drop(h);
    for _ in 0..100 {
        if !host.pump(4096).unwrap() {
            break;
        }
    }
    assert_eq!(host.admitted_bytes(), 0);
}

#[cfg(not(target_arch = "wasm32"))]
fn native_batch_probe(h: &mut Harness, ready: &std::sync::mpsc::Receiver<bool>) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        prepare::tick(&mut h.app, &h.ctx);
        if let Ok(proved) = ready.try_recv() {
            return proved;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "preparation probe did not return: {} / {}",
            prepare::inspect(&h.app),
            prepare::inspect_batches(&h.app)
        );
        std::thread::yield_now();
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_batch_replacement_keeps_first_priority_and_cancels_old_epoch_only_after_successful_proof() {
    use crate::control::ControlRequest;
    for valid in [false, true] {
        let (mut h, host, gate, ready, retired, cameras) = native_batch_fixture(false);
        let request = image_params(&h.app).unwrap();
        let document = h.app.session.doc.clone();
        let camera = h.app.cam;
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        let (batch, batch_reply) = ControlRequest::new("engine.execute", json!({"command":"render.batch","params":{"request":request}}));
        h.app.run_control(&h.ctx, batch);
        let mut replacement = request.clone();
        replacement["replace"] = json!(true);
        if !valid {
            replacement["settings"]["width"] = json!(0);
        }
        let (start, start_reply) = ControlRequest::new("engine.execute", json!({"command":"render.start","params":replacement,"param_check":"off"}));
        h.app.run_control(&h.ctx, start);
        native_batch_ready(&mut h, &ready);
        assert_eq!(prepare::inspect_batches(&h.app)["worker"]["preparing"], true, "the original first batch front retains first-action priority");
        assert_eq!(start_reply.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
        gate.send(()).unwrap();
        assert_eq!(native_batch_reply(&mut h, &batch_reply)["result"], json!({"queued":2}));
        retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
        let original = h.app.session.render_queue_state().job.unwrap();
        let original_handle = h.app.session.render_job_handle(original).unwrap();
        assert_eq!(
            native_batch_probe(&mut h, &ready),
            valid,
            "replacement failure must come from actual worker validation, not a hidden synchronous branch"
        );
        assert_eq!(
            original_handle.status().unwrap().progress.phase,
            solidcraft_engine::render::job::Phase::Queued,
            "replacement admission/proof cannot cancel the original job"
        );
        assert_eq!(h.app.session.render_queue_state().waiting, 1);
        assert_eq!(ready.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty), "later batch fronts stay paused during replacement proof");
        gate.send(()).unwrap();
        let result = native_batch_reply(&mut h, &start_reply);
        retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
        assert_eq!(result["ok"], valid, "{result}");
        if valid {
            let job: solidcraft_engine::render::job::JobId = serde_json::from_value(result["result"]["job"].clone()).unwrap();
            assert_ne!(job, original);
            assert!(h.app.session.render_queue_state().job.is_none());
            let handle = h.app.session.render_job_handle(job).unwrap();
            native_batch_complete(&host, &handle);
            assert_eq!(original_handle.status().unwrap().progress.phase, solidcraft_engine::render::job::Phase::Cancelled);
            native_batch_wait(&mut h, |app| prepare::inspect_batches(app)["owners"].as_array().unwrap().is_empty());
            retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
            drop(handle);
        } else {
            assert_eq!(h.app.session.render_queue_state().job, Some(original));
            assert_eq!(h.app.session.render_queue_state().waiting, 1, "failed replacement preserves the original common plan and first actual job");
            assert_eq!(original_handle.status().unwrap().progress.phase, solidcraft_engine::render::job::Phase::Queued);
            native_batch_complete(&host, &original_handle);
            native_batch_ready(&mut h, &ready);
            gate.send(()).unwrap();
            native_batch_wait(&mut h, |app| {
                app.session.render_queue_state().job.is_some_and(|job| job != original) && prepare::inspect_batches(app)["worker"].is_null()
            });
            retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
            let job = h.app.session.render_queue_state().job.unwrap();
            let handle = h.app.session.render_job_handle(job).unwrap();
            assert_eq!(handle.status().unwrap().request.camera, cameras[1], "the original second saved camera resumes unchanged");
            native_batch_complete(&host, &handle);
            h.app.session.advance_render_batch().unwrap();
            drop(handle);
        }
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!(h.app.cam, camera);
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
        drop(original_handle);
        drop(h);
        for _ in 0..100 {
            if !host.pump(4096).unwrap() {
                break;
            }
        }
        assert_eq!(host.admitted_bytes(), 0);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_batch_document_close_drains_first_proof_and_raw_replacement_without_minting_either() {
    use crate::control::ControlRequest;
    let (mut h, host, gate, ready, retired, _) = native_batch_fixture(false);
    let request = image_params(&h.app).unwrap();
    let (batch, batch_reply) = ControlRequest::new("engine.execute", json!({"command":"render.batch","params":{"request":request}}));
    h.app.run_control(&h.ctx, batch);
    native_batch_ready(&mut h, &ready);
    let bytes = host.admitted_bytes();
    assert!(bytes > 0);
    let mut replacement = request;
    replacement["replace"] = json!(true);
    let (start, start_reply) = ControlRequest::new("engine.execute", json!({"command":"render.start","params":replacement}));
    h.app.run_control(&h.ctx, start);
    assert_eq!(ready.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
    crate::documents::close(&mut h.app, 0, true);
    let document = h.app.session.doc.clone();
    let camera = h.app.cam;
    let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    prepare::tick(&mut h.app, &h.ctx);
    assert_eq!(start_reply.try_recv().unwrap()["ok"], false, "the bounded raw replacement has no capture/credit to pretend it submitted");
    assert_eq!(batch_reply.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
    assert_eq!(host.admitted_bytes(), bytes, "document close is cancellation intent, not proof of worker cleanup");
    gate.send(()).unwrap();
    assert_eq!(native_batch_reply(&mut h, &batch_reply)["ok"], false);
    retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(host.admitted_bytes(), 0);
    assert!(prepare::inspect_batches(&h.app)["owners"].as_array().unwrap().is_empty());
    assert!(h.app.render_workspace.view.job.is_none());
    assert!(h.app.session.render_retained_results().unwrap().is_empty());
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(h.app.cam, camera);
    assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_batch_disconnected_retirement_holds_credit_and_never_cleanup_cancels_a_committed_job() {
    use crate::control::ControlRequest;
    let (mut h, host, gate, ready, _retired, _) = native_batch_fixture(true);
    let owner = h.app.session.uid;
    let (batch, reply) = ControlRequest::new("engine.execute", json!({"command":"render.batch","params":{"request":image_params(&h.app).unwrap()}}));
    h.app.run_control(&h.ctx, batch);
    native_batch_ready(&mut h, &ready);
    gate.send(()).unwrap();
    let response = native_batch_reply(&mut h, &reply);
    assert_eq!(response["ok"], true, "a committed real first job retains its truthful identity despite retirement failure");
    assert_eq!(response["result"], json!({"queued":2}));
    assert!(!prepare::inspect_batches(&h.app)["worker"]["unavailable"].is_null());
    let job = h.app.session.render_queue_state().job.unwrap();
    let handle = h.app.session.render_job_handle(job).unwrap();
    native_batch_complete(&host, &handle);
    let held = host.admitted_bytes();
    assert!(held > 0);
    crate::documents::open_tab(&mut h.app, Session::default()).unwrap();
    assert_ne!(h.app.session.uid, owner);
    let (check, checked) = ControlRequest::new("engine.execute", json!({"command":"render.preflight","params":{}}));
    h.app.run_control(&h.ctx, check);
    assert_eq!(checked.try_recv().unwrap()["ok"], false, "unavailable global lane cannot start another proof in a different document");
    assert!(prepare::begin_batch_ui(&mut h.app, json!({})).is_err());
    prepare::tick(&mut h.app, &h.ctx);
    assert_eq!(host.admitted_bytes(), held, "tab replacement cannot fake a released plan or source");
    assert_eq!(handle.status().unwrap().progress.phase, solidcraft_engine::render::job::Phase::Completed);
    drop(handle);
    drop(h);
    assert_eq!(host.admitted_bytes(), 0, "only true app teardown runs the unavailable owners' destructors");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_batch_no_job_cancel_preserves_schema_scope_and_embedding_host_refusals() {
    use crate::control::ControlRequest;
    let (mut h, host, gate, ready, retired, _) = native_batch_fixture(false);
    let (batch, reply) = ControlRequest::new("engine.execute", json!({"command":"render.batch","params":{"request":image_params(&h.app).unwrap()}}));
    h.app.run_control(&h.ctx, batch);
    native_batch_ready(&mut h, &ready);
    let bytes = host.admitted_bytes();
    assert!(bytes > 0);
    for control in [
        json!({"command":"render.cancel","params":{},"param_check":"unknown"}),
        json!({"command":"render.cancel","params":{},"allow_paths":"not-an-array"}),
        json!({"command":"render.cancel","params":{"unknown_cancel_field":true},"param_check":"strict"}),
    ] {
        let (cancel, cancelled) = ControlRequest::new("engine.execute", control);
        h.app.run_control(&h.ctx, cancel);
        assert_eq!(cancelled.try_recv().unwrap()["ok"], false);
        assert_eq!(reply.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
        assert_eq!(host.admitted_bytes(), bytes);
    }
    h.app
        .session
        .set_command_check(Some(Arc::new(|id, _, _| if id == "render.cancel" { Err("embedding host refuses Cancel".into()) } else { Ok(()) })));
    let (cancel, cancelled) = ControlRequest::new("engine.execute", json!({"command":"render.cancel","params":{}}));
    h.app.run_control(&h.ctx, cancel);
    assert!(cancelled.try_recv().unwrap()["error"].as_str().unwrap().contains("embedding host refuses Cancel"));
    assert!(prepare::inspect_batches(&h.app)["owners"][0]["cancelled"].is_null());
    assert_eq!(host.admitted_bytes(), bytes, "refused control cannot touch the live claim or pretend it retired");
    h.app.session.set_command_check(None);
    ribbon::perform_job(&mut h.app, "render.cancel").unwrap();
    gate.send(()).unwrap();
    assert_eq!(native_batch_reply(&mut h, &reply)["ok"], false);
    retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(host.admitted_bytes(), 0);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_batch_fixed_owner_slots_include_closed_documents_until_retirement() {
    use crate::control::ControlRequest;
    let (mut h, host, gate, ready, retired, _) = native_batch_fixture(false);
    let request = json!({"request":{"settings":{"width":3,"height":2,"job":{"method":"raster"}}}});
    let (first, first_reply) = ControlRequest::new("engine.execute", json!({"command":"render.batch","params":request}));
    h.app.run_control(&h.ctx, first);
    native_batch_ready(&mut h, &ready);
    let mut senders = vec![first_reply];
    for _ in 1..crate::documents::MAX_OPEN {
        crate::documents::open_tab(&mut h.app, Session::default()).unwrap();
        h.app.run("render.settings.set", json!({"name":"A","set":{"width":3,"height":2,"job":{"method":"raster"}}})).unwrap();
        let (batch, reply) = ControlRequest::new("engine.execute", json!({"command":"render.batch","params":request}));
        h.app.run_control(&h.ctx, batch);
        assert_eq!(reply.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
        senders.push(reply);
    }
    assert_eq!(prepare::inspect_batches(&h.app)["owners"].as_array().unwrap().len(), 32);
    let bytes = host.admitted_bytes();
    assert!(bytes > 0);
    crate::documents::close(&mut h.app, 1, true);
    crate::documents::open_tab(&mut h.app, Session::default()).unwrap();
    h.app.run("render.settings.set", json!({"name":"A","set":{"width":3,"height":2,"job":{"method":"raster"}}})).unwrap();
    let (extra, extra_reply) = ControlRequest::new("engine.execute", json!({"command":"render.batch","params":request}));
    h.app.run_control(&h.ctx, extra);
    let refused = extra_reply.try_recv().unwrap();
    assert_eq!(refused["ok"], false);
    assert!(refused["error"].as_str().unwrap().contains(h.app.tr("render.batch_owner_limit")));
    assert_eq!(
        prepare::inspect_batches(&h.app)["owners"].as_array().unwrap().len(),
        32,
        "a closed owner is not a freed slot before coordinator cleanup"
    );
    assert_eq!(host.admitted_bytes(), bytes, "raw owner slots cannot manufacture further renderer grants");
    drop(h);
    gate.send(()).unwrap();
    retired.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(host.admitted_bytes(), 0);
    assert!(
        senders.iter().all(|reply| reply.try_recv() == Err(std::sync::mpsc::TryRecvError::Disconnected)),
        "true window teardown disconnects the outstanding original senders without inventing jobs"
    );
}

#[test]
fn light_world_vector_axes_fit_the_real_narrow_inspector_without_clipping_or_paint_edits() {
    for side in ["left", "right"] {
        let mut h = render_lights_fixture();
        h.call("ui.resize", json!({"width":1100,"height":3000}));
        h.app.preferences.render_inspector_side = side.into();
        h.app.preferences.render_browser_side = if side == "left" { "right" } else { "left" }.into();
        h.app.run("render.settings.set", json!({"name":"Studio","set":{"job":{"method":"gpu_path","options":{}}}})).unwrap();
        setups::load(&mut h.app);
        light::edit(&mut h.app, "render.lights.add", None, Some(json!({"kind":"spot"}))).unwrap();
        light::edit(
            &mut h.app,
            "render.lights.set",
            Some("spot:0"),
            Some(json!({
                "position_mm":[-999999999.125,999999999.125,120.125],"emitted_axis":[1.0,2.0,-2.0]
            })),
        )
        .unwrap();
        h.app.run("view.render_select", json!({"kind":"light","id":"spot:0"})).unwrap();
        let document = h.app.session.doc.clone();
        let model = h.app.session.model.state();
        let camera = h.app.cam;
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        let request = image_request(&h.app).unwrap();
        for _ in 0..2 {
            h.frames(4);
            for (key, label) in [("position_mm", "render.lighting_world_position"), ("emitted_axis", "render.spot_axis")] {
                for (index, axis) in ["X", "Y", "Z"].iter().enumerate() {
                    let field = crate::scenario::rect_of(&format!("render:light:{key}:{index}")).unwrap();
                    let row = crate::scenario::rect_of(&format!("dialog:control:{} {axis}", h.app.tr(label))).unwrap();
                    assert!(h.ctx.content_rect().contains_rect(field), "the actual coordinate control fits the window: {side}/{key}/{axis}");
                    assert!(row.contains_rect(field), "the actual response fits its painted control-column clip: {side}/{key}/{axis}");
                }
            }
            assert!(Arc::ptr_eq(&document, &h.app.session.doc));
            assert!(Arc::ptr_eq(&model, &h.app.session.model.state()));
            assert_eq!(h.app.cam, camera);
            assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
            assert_eq!(image_request(&h.app).unwrap(), request);
        }
        assert_eq!(gpu_lighting_request(&request)["punctual_lights"][0]["position_mm"], json!([-999999999.125, 999999999.125, 120.125]));
        assert_eq!(gpu_lighting_request(&request)["punctual_lights"][0]["cone"]["emitted_axis"], json!([1.0, 2.0, -2.0]));
    }
}

fn constant_environment_frame(h: &mut Harness, events: Vec<egui::Event>) -> String {
    crate::scenario::clear_handles();
    let mut output = h.ctx.run_ui(
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(308.0, 3000.0))), events, ..Default::default() },
        |ui| environment::editor(&mut h.app, ui),
    );
    let text = crate::test_support::text_shapes(&output).map(|shape| shape.galley.job.text.clone()).collect::<Vec<_>>().join("\n");
    output.textures_delta.clear();
    text
}

fn constant_environment_click_indirect(h: &mut Harness) {
    constant_environment_frame(h, vec![]);
    let rect = crate::scenario::rect_of("render:environment:enable-indirect").expect("the actual constant-source adoption button");
    assert!(rect.is_positive() && rect.left() >= 0.0 && rect.right() <= 308.0, "the action must fit the narrow inspector: {rect:?}");
    let point = rect.center();
    for pressed in [true, false] {
        constant_environment_frame(
            h,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() },
            ],
        );
    }
}

#[test]
fn cpu_constant_environment_guidance_explains_legacy_reflection_and_transmission_without_paint_writes() {
    use solidcraft_engine::render::job::{ReflectionBody, ReflectionMode, ReflectionOptions};
    for reflection in [false, true] {
        let mut h = cpu_volume_editor_fixture(false, None);
        h.app.session.set_render_job_host(None).unwrap();
        if reflection {
            h.app.render_workspace.view.raytrace.reflections = Some(ReflectionOptions {
                bodies: vec![ReflectionBody { body: "Volume subject".into(), mode: ReflectionMode::Mirror }],
                ..Default::default()
            });
        }
        let document = h.app.session.doc.clone();
        let before = setups::draft(&h.app).unwrap();
        let options = h.app.render_workspace.view.raytrace.clone();
        let camera = h.app.cam;
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        for _ in 0..3 {
            let text = constant_environment_frame(&mut h, vec![]);
            assert!(text.contains(h.app.tr("render.cpu_constant_direct_help")));
            assert!(crate::scenario::rect_of("render:environment:enable-indirect").is_some());
        }
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!(setups::draft(&h.app).unwrap(), before);
        assert_eq!(h.app.render_workspace.view.raytrace, options, "explaining selected reflection sources must never erase them");
        assert_eq!(h.app.cam, camera);
        assert_eq!(history, (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()));
        assert!(h.app.session.render_job_host().is_none(), "painting the action must not require or install a host");
    }
}

#[test]
fn cpu_constant_environment_pointer_indirect_adoption_is_complete_camera_neutral_one_undo_and_cold_portable() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    use solidcraft_engine::render::job::{CpuBudget, CpuProgressiveOptions};
    for progressive in [None, Some(CpuProgressiveOptions { samples_per_pixel: 7, seed: 91 })] {
        let mut h = cpu_volume_editor_fixture(false, Some(0.0));
        h.app.session.set_render_job_host(None).unwrap();
        let mut source = h.app.session.doc.render.setups["Studio"].clone();
        if let Some(RenderJob::CpuRaytrace { options, transparent_background }) = &mut source.job {
            *transparent_background = true;
            options.budget = CpuBudget::Studio;
            options.progressive = progressive;
            if progressive.is_some() {
                options.primary_samples = 1;
            }
            options.lights = None; // Retain the actual implicit three-light owner.
        }
        h.app.run("render.settings.set", json!({"name":"Studio","set":{"job":source.job}})).unwrap();
        setups::load(&mut h.app);
        let before = h.app.session.doc.render.setups["Studio"].clone();
        h.app.cam.yaw += 0.2;
        let camera = h.app.cam;
        setups::refresh_dirty(&mut h.app);
        light::loaded(&mut h.app);
        let rows = light::rows(&h.app).unwrap();
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|row| row.implicit), "the authored None list owns the neutral studio sources");
        assert_eq!(rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(), ["directional:0", "directional:1", "directional:2"]);
        assert_eq!(rows.iter().map(|row| row.intensity).collect::<Vec<_>>(), [3.0, 1.0, 1.5]);
        let Some(RenderJob::CpuRaytrace { options, .. }) = &before.job else { panic!("CPU source lost") };
        assert!(options.lights.is_none() && options.preset.is_none());
        let id = rows[0].id.clone();
        h.app.run("render.lights.mute", json!({"id":id,"mute":true})).unwrap();
        h.app.run("render.lights.solo", json!({"id":id,"solo":true})).unwrap();
        h.app.render_workspace.view.selection = RenderSelection::Light(id.clone());
        let visibility = h.app.render_workspace.view.lights.visibility.clone();
        assert!(rows.iter().all(|row| !visibility.visible(&row.id)), "the solo source is muted; the other sources are not solo");
        // The request copy must materialize all three exact sources at zero.
        // Leaving lights=None would reactivate the implicit studio fallback.
        let expected_lights = json!([
            {"direction":[0.6,0.0,0.8],"color":[1.0,1.0,1.0],"intensity":0.0},
            {"direction":[-0.8,0.0,0.6],"color":[1.0,1.0,1.0],"intensity":0.0},
            {"direction":[0.0,-0.6,0.8],"color":[1.0,1.0,1.0],"intensity":0.0}
        ]);
        let filtered_before = image_request(&h.app).unwrap();
        assert_eq!(filtered_before["settings"]["job"]["options"]["lights"], expected_lights);
        let filtered_setup: solidcraft_engine::doc::render_settings::RenderSetup =
            serde_json::from_value(filtered_before["settings"].clone()).unwrap();
        let filtered_sources = solidcraft_engine::render_lights::inspect(&filtered_setup).unwrap();
        assert_eq!(filtered_sources.len(), 3);
        assert!(filtered_sources.iter().all(|source| !source.implicit && source.intensity == 0.0));
        let irradiance: [f64; 3] =
            std::array::from_fn(|channel| filtered_sources.iter().map(|source| source.colour[channel] * source.intensity).sum());
        assert_eq!(irradiance, [0.0; 3], "every authored directional source contributes exactly zero energy");
        let document = h.app.session.doc.clone();
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        constant_environment_frame(&mut h, vec![]);
        assert!(Arc::ptr_eq(&document, &h.app.session.doc), "opening the source must remain read-only");
        constant_environment_click_indirect(&mut h);
        let mut expected = before.clone();
        if let Some(RenderJob::CpuRaytrace { options, .. }) = &mut expected.job {
            options.path = Some(Default::default());
            options.progressive = Some(progressive.unwrap_or_default());
            options.primary_samples = 1;
            options.shadows = true;
        }
        let after = h.app.session.doc.render.setups["Studio"].clone();
        assert_eq!(after, expected, "the exact saved job must preserve volume, light owner, output, profile, source and all other choices");
        assert_eq!(after.camera, before.camera, "the action cannot silently update the saved shot");
        assert_eq!(h.app.cam, camera);
        assert_eq!(h.app.session.undo.len(), history.1 + 1);
        assert_eq!(h.app.session.redo.len(), 0);
        assert_eq!(light::rows(&h.app).unwrap(), rows);
        assert_eq!(h.app.render_workspace.view.lights.visibility, visibility);
        assert_eq!(h.app.render_workspace.view.selection, RenderSelection::Light(id.clone()));
        assert!(h.app.render_workspace.view.dirty, "unrelated live navigation remains a draft");
        assert_eq!(h.app.render_workspace.view.raytrace.path.unwrap().max_depth, 8);
        if progressive.is_none() {
            assert_eq!(h.app.render_workspace.view.raytrace.progressive.unwrap().samples_per_pixel, 16);
        }
        let request = raw_image_request(&h.app).unwrap();
        assert_eq!(request["settings"]["job"], serde_json::to_value(&after.job).unwrap());
        let filtered = image_request(&h.app).unwrap();
        assert_eq!(filtered["settings"]["job"]["options"]["lights"], expected_lights, "adoption preserves the exact zero-energy captured sources");
        assert_eq!(filtered["settings"]["job"]["options"]["lights"], filtered_before["settings"]["job"]["options"]["lights"]);
        assert!(request["settings"]["job"]["options"]["lights"].is_null(), "raw authored intent retains the implicit owner, not its filtered copy");
        h.app.run("edit.undo", json!({})).unwrap();
        setups::sync_revision(&mut h.app);
        assert_eq!(h.app.session.doc.render.setups["Studio"], before);
        assert!(h.app.render_workspace.view.raytrace.path.is_none());
        assert_eq!(h.app.cam, camera);
        assert_eq!(h.app.render_workspace.view.lights.visibility, visibility);
        assert_eq!(image_request(&h.app).unwrap()["settings"]["job"]["options"]["lights"], expected_lights);
        h.app.run("edit.redo", json!({})).unwrap();
        setups::sync_revision(&mut h.app);
        assert_eq!(h.app.session.doc.render.setups["Studio"], after);
        assert_eq!(h.app.cam, camera);
        assert_eq!(h.app.render_workspace.view.lights.visibility, visibility);
        assert_eq!(image_request(&h.app).unwrap()["settings"]["job"]["options"]["lights"], expected_lights);
        assert!(h.app.session.render_job_host().is_none(), "the offline adoption action cannot mint a host");
        assert!(h.app.render_workspace.view.job.is_none());
        let reopened = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
        // Document reset re-registers the app's existing host, not a new host
        // from saved settings. Pin that owner independently of the fresh Session.
        let cached_host = h.app.render_workspace.image_host.clone();
        let same_cached_host = |actual: Option<&solidcraft_engine::render::job::CpuHost>| match (cached_host.as_ref(), actual) {
            (Some(expected), Some(actual)) => expected.same_host(actual),
            (None, None) => true,
            _ => false,
        };
        h.app.session = Session::new(reopened);
        assert!(h.app.session.render_job_host().is_none(), "the fresh Session starts without an installed host");
        let cold_document = h.app.session.doc.clone();
        let cold_model = h.app.session.model.state();
        let cold_camera = h.app.cam;
        let cold_history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        reset_document(&mut h.app);
        setups::load(&mut h.app);
        assert_eq!(setups::snapshot(&h.app).unwrap().job, after.job);
        assert_eq!(h.app.render_workspace.view.cpu.environment, cpu::Environment::Constant);
        assert!(same_cached_host(h.app.session.render_job_host()), "cold reset/load must reuse only the app's exact existing host");
        assert!(same_cached_host(h.app.render_workspace.image_host.as_ref()));
        assert!(h.app.render_workspace.view.job.is_none());
        let text = constant_environment_frame(&mut h, vec![]);
        assert!(!text.contains(h.app.tr("render.cpu_constant_direct_help")));
        assert!(crate::scenario::rect_of("render:environment:enable-indirect").is_none());
        assert!(same_cached_host(h.app.session.render_job_host()), "painting cannot replace or create a host");
        assert!(h.app.render_workspace.view.job.is_none());
        assert!(Arc::ptr_eq(&h.app.session.doc, &cold_document));
        assert!(Arc::ptr_eq(&h.app.session.model.state(), &cold_model));
        assert_eq!(h.app.cam, cold_camera);
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), cold_history);
    }
}

#[test]
fn cpu_constant_environment_indirect_refusals_preserve_legacy_overrides_full_size_and_both_cameras_atomically() {
    use solidcraft_engine::render::job::{ReflectionBody, ReflectionMode, ReflectionOptions};
    enum Incompatible {
        Reflection,
        Budget,
        LegacyRotation,
        LegacyIntensity,
    }
    for (incompatible, case) in [
        ("reflection", Incompatible::Reflection),
        ("budget", Incompatible::Budget),
        ("legacy_rotation", Incompatible::LegacyRotation),
        ("legacy_intensity", Incompatible::LegacyIntensity),
    ] {
        let mut h = cpu_volume_editor_fixture(false, None);
        h.app.session.set_render_job_host(None).unwrap();
        match case {
            Incompatible::Reflection => {
                h.app.render_workspace.view.raytrace.reflections = Some(ReflectionOptions {
                    bodies: vec![ReflectionBody { body: "Volume subject".into(), mode: ReflectionMode::Mirror }],
                    ..Default::default()
                });
            }
            Incompatible::Budget => {
                h.app.render_workspace.view.draft.width = 2048;
                h.app.render_workspace.view.draft.height = 2048;
            }
            Incompatible::LegacyRotation => h.app.render_workspace.view.draft.environment_rotation = 0.25,
            Incompatible::LegacyIntensity => h.app.render_workspace.view.draft.environment_intensity = 2.0,
        }
        h.app.cam.yaw += 0.3;
        let before = setups::draft(&h.app).unwrap();
        let options = h.app.render_workspace.view.raytrace.clone();
        let document = h.app.session.doc.clone();
        let camera = h.app.cam;
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        constant_environment_click_indirect(&mut h);
        assert!(Arc::ptr_eq(&document, &h.app.session.doc), "{incompatible}: refusal must not create a command");
        assert_eq!(setups::draft(&h.app).unwrap(), before, "{incompatible}: full authored fields stay intact");
        assert_eq!(h.app.render_workspace.view.raytrace, options, "{incompatible}: no selected override or fixed-sampling choice is cleared");
        assert_eq!(h.app.cam, camera);
        assert_eq!(history, (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()));
        assert!(h.app.render_workspace.view.raytrace.path.is_none());
        let error = environment::enable_indirect_lighting(&mut h.app).unwrap_err();
        if incompatible.starts_with("legacy_") {
            assert_eq!(error, h.app.tr("render.setup_legacy_environment"));
        } else if incompatible == "reflection" {
            assert_eq!(error, h.app.tr("render.setup_runtime_unsupported"));
        } else {
            use solidcraft_engine::render::job::{CpuBudget, CpuProgressiveOptions};
            assert!(CpuBudget::Compact.validate_image(2048, 2048, None).is_ok(), "the authored fixed image was valid before adoption");
            let sampling = CpuProgressiveOptions::default();
            assert_eq!(sampling.samples_per_pixel, 16);
            assert!(u64::from(2048_u32).pow(2) * u64::from(sampling.samples_per_pixel) > CpuBudget::Compact.limits().max_camera_samples);
            assert!(CpuBudget::Compact.validate_image(2048, 2048, Some(sampling)).is_err());
            // Compare the prospective translated size reason, never assume it
            // names the current profile rather than the larger offered one.
            let previous = h.app.render_workspace.view.clone();
            h.app.render_workspace.view.raytrace.progressive = Some(sampling);
            let expected = cpu::validate_progressive_image(&h.app).unwrap_err();
            h.app.render_workspace.view = previous;
            assert_eq!(error, expected, "full authored size/profile must be checked before the action can save");
        }
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!(h.app.cam, camera);
    }
}

#[test]
fn cpu_constant_environment_guidance_does_not_change_other_methods_sources_or_existing_indirect_mode() {
    #[derive(Clone, Copy)]
    enum Source {
        None,
        Hdr,
        Sky,
        Path,
        Gpu,
        Raster,
    }
    for (case, source) in
        [("none", Source::None), ("hdr", Source::Hdr), ("sky", Source::Sky), ("path", Source::Path), ("gpu", Source::Gpu), ("raster", Source::Raster)]
    {
        let mut h = cpu_volume_editor_fixture(matches!(source, Source::Path | Source::Sky), None);
        match source {
            Source::None => h.app.render_workspace.view.cpu.environment = cpu::Environment::None,
            Source::Hdr => {
                h.app.render_workspace.view.cpu.environment = cpu::Environment::Hdr;
                h.app.render_workspace.view.cpu.hdr = hdr::Input::FilePending { path: "/must-not-be-read/lighting.hdr".into() };
            }
            Source::Sky => h.app.render_workspace.view.cpu.environment = cpu::Environment::Sky,
            Source::Gpu => h.app.render_workspace.view.method = ImageMethod::GpuRaytrace,
            Source::Raster => h.app.render_workspace.view.method = ImageMethod::Raster,
            Source::Path => {}
        }
        let document = h.app.session.doc.clone();
        let options = h.app.render_workspace.view.raytrace.clone();
        let camera = h.app.cam;
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        let text = constant_environment_frame(&mut h, vec![]);
        assert!(!text.contains(h.app.tr("render.cpu_constant_direct_help")), "{case}");
        assert!(crate::scenario::rect_of("render:environment:enable-indirect").is_none(), "{case}");
        assert!(Arc::ptr_eq(&document, &h.app.session.doc), "{case}: merely opening this source is read-only");
        assert_eq!(h.app.render_workspace.view.raytrace, options);
        assert_eq!(h.app.cam, camera);
        assert_eq!(history, (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()));
    }
}

fn displayed_result_pass(h: &mut Harness, mut configure: impl FnMut(&mut egui::Ui)) -> egui::FullOutput {
    let app = &mut h.app;
    h.ctx.run_ui(
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0))), ..Default::default() },
        |ui| {
            displayed::begin(app);
            configure(ui);
            show_result(app, ui);
            displayed::finish(app, ui.ctx());
        },
    )
}

#[test]
fn render_displayed_receipt_requires_a_visible_final_submission_not_a_cached_image() {
    let mut h = output_viewer_fixture();
    h.app.render_workspace.view.live_paused = true;
    let job = completed_preview(&mut h, [20, 40, 60]);
    h.app.render_workspace.view.job = Some(job);
    h.app.render_workspace.view.result = true;
    cache_render_images(&mut h.app, &h.ctx);
    let frame = h.app.session.render_job_handle(job).unwrap().frame().unwrap().unwrap();
    let cached = h.app.render_workspace.image.as_ref().unwrap().1.as_ref().unwrap().clone();
    assert!(inspect_state(&h.app)["displayed_receipt"].is_null(), "decoding a texture does not submit it");
    let doc = h.app.session.doc.clone();
    let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    let camera = h.app.cam;
    h.frames(3);
    let receipt = inspect_state(&h.app)["displayed_receipt"].clone();
    assert_eq!(receipt["job"], json!(job));
    assert_eq!(receipt["scene_generation"], frame.info.scene_generation);
    assert_eq!((receipt["width"].as_u64(), receipt["height"].as_u64()), (Some(16), Some(16)));
    assert_eq!(receipt["kind"], "final");
    assert!(receipt["frame_sequence"].is_null() && receipt["samples_per_pixel"].is_null() && receipt["adaptive"].is_null());
    assert!(receipt["egui_frame_nr"].is_u64() && receipt["egui_pass_nr"].is_u64());
    assert_eq!(receipt["egui_frame_nr"].as_u64().unwrap() + 1, h.ctx.cumulative_frame_nr());
    assert_eq!(receipt["egui_pass_nr"].as_u64().unwrap() + 1, h.ctx.cumulative_pass_nr());
    assert!(
        h.shapes
            .iter()
            .any(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == cached.id() && shape.clip_rect.is_positive())),
        "the receipt must accompany a real image shape in the completed app UI pass"
    );
    // Automation drains in logic before the next UI pass. Neither inspect nor logic
    // may mint a fresh frame/pass receipt from the still-present texture.
    for _ in 0..3 {
        assert_eq!(inspect_state(&h.app)["displayed_receipt"], receipt);
    }
    h.app.logic(&h.ctx);
    assert_eq!(inspect_state(&h.app)["displayed_receipt"], receipt);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
    assert_eq!(h.app.cam, camera);
    h.app.render_workspace.image.as_mut().unwrap().1 = Err("retained decode/upload error".into());
    displayed_result_pass(&mut h, |_| {}).textures_delta.clear();
    assert!(inspect_state(&h.app)["displayed_receipt"].is_null(), "an error label is not an image submission");
    h.app.render_workspace.image.as_mut().unwrap().1 = Ok(cached);
    displayed_result_pass(&mut h, |_| {}).textures_delta.clear();
    assert!(!inspect_state(&h.app)["displayed_receipt"].is_null());
    h.app.run("render.result.view", json!({"view":"model"})).unwrap();
    assert!(inspect_state(&h.app)["displayed_receipt"].is_null(), "the mode command clears the prior pass before another paint");
    assert!(h.app.render_workspace.image.is_some(), "the diagnostic does not discard or change the retained texture");
}

#[test]
fn render_displayed_receipt_excludes_hidden_clipped_transparent_and_discarded_passes() {
    let mut h = output_viewer_fixture();
    h.app.render_workspace.view.live_paused = true;
    let job = completed_preview(&mut h, [20, 40, 60]);
    h.app.render_workspace.view.job = Some(job);
    h.app.render_workspace.view.result = true;
    displayed_result_pass(&mut h, |_| {}).textures_delta.clear();
    assert!(!inspect_state(&h.app)["displayed_receipt"].is_null());
    displayed_result_pass(&mut h, |ui| ui.set_invisible()).textures_delta.clear();
    assert!(inspect_state(&h.app)["displayed_receipt"].is_null());
    displayed_result_pass(&mut h, |ui| ui.set_opacity(0.0)).textures_delta.clear();
    assert!(inspect_state(&h.app)["displayed_receipt"].is_null());
    displayed_result_pass(&mut h, |ui| ui.set_clip_rect(egui::Rect::NOTHING)).textures_delta.clear();
    assert!(inspect_state(&h.app)["displayed_receipt"].is_null());
    displayed_result_pass(&mut h, |_| {}).textures_delta.clear();
    let previous = inspect_state(&h.app)["displayed_receipt"].clone();
    assert!(!previous.is_null());
    let mut passes = 0;
    let app = &mut h.app;
    let mut output = h.ctx.run_ui(
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0))), ..Default::default() },
        |ui| {
            displayed::begin(app);
            if passes == 0 {
                show_result(app, ui);
                ui.ctx().request_discard("receipt test: layout discards after the actual image call");
                assert!(ui.ctx().will_discard());
                displayed::finish(app, ui.ctx());
                assert_eq!(inspect_state(app)["displayed_receipt"], previous, "a discarded candidate cannot replace the completed pass");
            } else {
                displayed::finish(app, ui.ctx());
                assert!(inspect_state(app)["displayed_receipt"].is_null(), "the final accepted pass submitted no image");
            }
            passes += 1;
        },
    );
    output.textures_delta.clear();
    assert_eq!(passes, 2, "exercise egui's real discard/replay path");
}

#[test]
fn render_displayed_receipt_clears_for_comparison_document_and_workspace_transitions() {
    let mut h = output_viewer_fixture();
    h.app.render_workspace.view.live_paused = true;
    let first = completed_preview(&mut h, [20, 40, 60]);
    let second = completed_preview(&mut h, [80, 100, 120]);
    h.app.render_workspace.view.job = Some(second);
    h.app.run("render.result.select", json!({"index":1})).unwrap();
    h.app.run("render.result.view", json!({"view":"image"})).unwrap();
    h.frames(3);
    assert_eq!(inspect_state(&h.app)["displayed_receipt"]["job"], json!(second));
    h.app.run("render.compare", json!({"left":0,"right":1})).unwrap();
    assert!(inspect_state(&h.app)["displayed_receipt"].is_null());
    h.frames(2);
    assert!(comparison_active(&h.app));
    assert!(inspect_state(&h.app)["displayed_receipt"].is_null(), "two composited sources must not masquerade as one submitted image");
    h.app.run("render.compare", json!({"off":true})).unwrap();
    assert!(inspect_state(&h.app)["displayed_receipt"].is_null(), "ending comparison cannot resurrect an old receipt");
    h.frames(2);
    assert!(!inspect_state(&h.app)["displayed_receipt"].is_null());
    enter(&mut h.app, false).unwrap();
    assert!(inspect_state(&h.app)["displayed_receipt"].is_null());
    enter(&mut h.app, true).unwrap();
    assert!(inspect_state(&h.app)["displayed_receipt"].is_null());
    h.app.render_workspace.view.result = true;
    h.frames(2);
    assert!(!inspect_state(&h.app)["displayed_receipt"].is_null());
    invalidate_images(&mut h.app);
    assert!(inspect_state(&h.app)["displayed_receipt"].is_null());
    h.frames(2);
    assert!(!inspect_state(&h.app)["displayed_receipt"].is_null());
    reset_document(&mut h.app);
    assert!(inspect_state(&h.app)["displayed_receipt"].is_null());
    assert!(h.app.session.render_job_handle(first).unwrap().frame().unwrap().is_some(), "receipt resets do not evict renderer results");
}

#[test]
fn render_displayed_receipt_tracks_actual_cooperative_preview_then_final_without_pumping_it() {
    use solidcraft_engine::render::job::{JobId, Phase};
    for adaptive in [None, Some(json!({"pilot_samples":16,"min_samples":16,"relative_error":0.05,"absolute_error":0.002,"coverage_error":0.01}))] {
        let maximum = if adaptive.is_some() { 16 } else { 3 };
        let mut h = output_viewer_fixture();
        h.app.render_workspace.view.live_paused = true;
        if image_host(&h.app, ImageMethod::CpuRaytrace).is_err() {
            eprintln!("CPU checkpoint receipt test requires raytrace-cpu");
            return;
        }
        let host = h.app.session.render_job_host().unwrap().clone();
        let reply = h.app.session.execute("render.start", &json!({"settings":{"width":2,"height":2,"job":{"method":"cpu_raytrace","options":{"primary_samples":1,"progressive":{"samples_per_pixel":maximum,"seed":81},"adaptive":adaptive}}}})).unwrap();
        let job: JobId = serde_json::from_value(reply["job"].clone()).unwrap();
        let handle = h.app.session.render_job_handle(job).unwrap();
        for _ in 0..100_000 {
            host.pump(1).unwrap();
            if handle.preview().unwrap().is_some() {
                break;
            }
        }
        let checkpoint = handle.preview().unwrap().expect("one real complete-pass CPU checkpoint");
        assert_ne!(handle.status().unwrap().progress.phase, Phase::Completed);
        h.app.render_workspace.view.job = Some(job);
        h.app.render_workspace.view.result = true;
        h.app.render_workspace.view.follow_job = true;
        let before = handle.status().unwrap().progress;
        displayed_result_pass(&mut h, |_| {}).textures_delta.clear();
        let receipt = inspect_state(&h.app)["displayed_receipt"].clone();
        assert_eq!(receipt["kind"], "preview");
        assert_eq!(receipt["job"], json!(checkpoint.info.frame.job));
        assert_eq!(receipt["scene_generation"], checkpoint.info.frame.scene_generation);
        assert_eq!(receipt["frame_sequence"], checkpoint.info.frame_sequence);
        assert_eq!(receipt["samples_per_pixel"], checkpoint.info.samples_per_pixel);
        assert_eq!(receipt["adaptive"], json!(checkpoint.info.adaptive));
        assert_eq!(
            receipt["adaptive"].is_null(),
            adaptive.is_none(),
            "adaptive absolute iterations carry their captured phase instead of claiming uniform SPP"
        );
        let after = handle.status().unwrap().progress;
        assert_eq!(
            (before.done, before.total, before.phase),
            (after.done, after.total, after.phase),
            "painting and inspecting the receipt cannot pump the producer"
        );
        for _ in 0..1000 {
            host.pump(16384).unwrap();
            if handle.status().unwrap().progress.phase == Phase::Completed {
                break;
            }
        }
        assert_eq!(handle.status().unwrap().progress.phase, Phase::Completed);
        displayed_result_pass(&mut h, |_| {}).textures_delta.clear();
        let final_receipt = inspect_state(&h.app)["displayed_receipt"].clone();
        assert_eq!(final_receipt["job"], json!(job));
        assert_eq!(final_receipt["kind"], "final");
        assert!(final_receipt["frame_sequence"].is_null() && final_receipt["samples_per_pixel"].is_null());
        assert!(handle.frame().unwrap().is_some());
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn render_displayed_receipt_uses_the_immutable_live_texture_source_not_the_current_selected_job() {
    let Some(mut h) = live_cpu_fixture() else { return };
    h.app.viewport.rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(8.0, 8.0)));
    live::advance(&mut h.app, &h.ctx, 1.0);
    let captured = wait_live_complete(&mut h, 1.3);
    let selected = h.app.render_workspace.view.job;
    let mut output = h.ctx.run_ui(
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 200.0))), ..Default::default() },
        |ui| {
            displayed::begin(&mut h.app);
            live::paint(&mut h.app, ui, egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0)));
            displayed::finish(&mut h.app, ui.ctx());
        },
    );
    output.textures_delta.clear();
    let receipt = inspect_state(&h.app)["displayed_receipt"].clone();
    assert_eq!(receipt["job"], captured["job"]);
    assert_ne!(receipt["job"], json!(selected));
    assert_eq!(receipt["scene_generation"], h.app.session.revision);
    assert_eq!(receipt["kind"], "live_final");
    assert_eq!(receipt["samples_per_pixel"], captured["samples"]);
    assert_eq!(receipt["frame_sequence"], captured["sequence"]);
    assert_eq!(h.app.render_workspace.view.job, selected, "scratch pixels cannot change final-result ownership");
    let mut output = h.ctx.run_ui(
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 200.0))), ..Default::default() },
        |ui| {
            displayed::begin(&mut h.app);
            let subpixel = egui::Rect::from_min_size(egui::pos2(10.1, 10.1), egui::vec2(0.1, 0.1));
            live::paint(&mut h.app, ui, subpixel);
            displayed::finish(&mut h.app, ui.ctx());
        },
    );
    output.textures_delta.clear();
    assert!(inspect_state(&h.app)["displayed_receipt"].is_null(), "a live image rounded to zero physical area is not a visible submission");
}

fn stable_glass_draft_fixture(method: ImageMethod, with_maps: bool) -> Harness {
    let mut h = Harness::new();
    assert_eq!(h.app.preferences.render_inspector_width, crate::prefs::Prefs::default().render_inspector_width);
    h.call("ui.resize", json!({"width":1440,"height":3000}));
    for (index, name) in ["Glass A", "Glass B", "Map", "Coat"].into_iter().enumerate() {
        h.app.run("solid.box", json!({"body_name":name,"length":5,"width":5,"height":5,"corner":[index * 15,0,0],"operation":"new"})).unwrap();
    }
    let camera = solidcraft_engine::render::Camera { fov: 0.6, distance: 80.0, ..Default::default() };
    let lens = json!({"aperture_radius_mm":0.02,"focus_distance_mm":80.0});
    let job = if method == ImageMethod::CpuRaytrace {
        json!({"method":"cpu_raytrace","options":{"path":{"max_depth":4},
            "progressive":{"samples_per_pixel":4,"seed":17},"lens":lens,"output":{"linear_exr":true}}})
    } else {
        json!({"method":"gpu_path","options":{"samples":4,"max_depth":4,"seed":17,"lens":lens,"linear_exr":true,
            "lighting":{"environment":[0.15,0.2,0.25],"sun":{"direction":[1,2,3],"angular_radius_rad":0.02,"radiance":[2.0,3.0,4.0]}}}})
    };
    h.app.run("render.settings.set", json!({"name":"Studio","set":{"width":8,"height":8,"camera":camera,"job":job}})).unwrap();
    let targets = materials::read_targets(&mut h.app).unwrap();
    let target = |name: &str| targets.iter().find(|target| target.name == name).unwrap();
    let a = target("Glass A");
    h.app
        .run(
            "render.optics.bind",
            json!({"name":"Studio","body_index":a.ordinal,"body_name":a.name,"expected_revision":h.app.session.revision,
                "optics":{"ior":1.43,"boundary":"closed_mesh","absorption_per_mm":[0.01,0.02,0.03]}}),
        )
        .unwrap();
    let coat = target("Coat");
    h.app
        .run(
            "render.coat.bind",
            json!({"name":"Studio","body_index":coat.ordinal,"body_name":coat.name,"expected_revision":h.app.session.revision,
                "coat":{"weight":0.35,"roughness":0.18,"ior":1.4}}),
        )
        .unwrap();
    if with_maps {
        let map = target("Map");
        h.app
            .run(
                "render.library.surface.bind",
                json!({"name":"Studio","surface":{"id":"SURF-001","body_index":map.ordinal,"body_name":map.name,"scale":1.0}}),
            )
            .unwrap();
    }
    h.app.cam = camera;
    enter(&mut h.app, true).unwrap();
    setups::load(&mut h.app);
    h.app.render_workspace.view.live_paused = true;
    h.app.session.set_render_job_host(None).unwrap();
    h
}

fn stable_glass_draft_add(h: &mut Harness, name: &str) {
    let target = materials::read_targets(&mut h.app).unwrap().into_iter().find(|target| target.name == name).unwrap();
    assert!(target.stable_target.is_some());
    let mut binding = materials::Binding::new(target, &h.app);
    binding.glass_enabled = true;
    h.app.render_workspace.view.materials.enabled = true;
    h.app.render_workspace.view.materials.bindings.push(binding);
}

fn stable_glass_draft_viewport(h: &Harness) -> egui::Rect {
    let inspector = crate::scenario::rect_of("render:inspector").unwrap();
    let header = crate::scenario::rect_of("render:inspector:header").unwrap();
    let footer = crate::scenario::rect_of("dialog:editor_footer").unwrap();
    assert!(inspector.contains(header.center()) && inspector.contains(footer.center()));
    let viewport = egui::Rect::from_min_max(
        egui::pos2(inspector.left(), header.bottom() + Tokens::get().space_8),
        egui::pos2(inspector.right(), (inspector.bottom() - Tokens::get().space_8).min(footer.top())),
    )
    .intersect(h.ctx.content_rect());
    assert!(viewport.is_positive());
    viewport
}

fn stable_glass_draft_reveal(h: &mut Harness, handle: &str) -> usize {
    for scrolls in 0..32 {
        let rect = crate::scenario::rect_of(handle).unwrap();
        let viewport = stable_glass_draft_viewport(h);
        assert!(rect.is_positive() && rect.left() >= viewport.left() && rect.right() <= viewport.right(), "{handle}: {rect:?}, {viewport:?}");
        if viewport.contains_rect(rect) {
            return scrolls;
        }
        // Scroll the actual visible inspector body, never an off-clip target.
        let point = egui::pos2(viewport.left() + viewport.width() * 0.25, viewport.center().y);
        h.call("ui.scroll", json!({"x":point.x,"y":point.y,"delta":if rect.top() < viewport.top() {120} else {-120}}));
        for _ in 0..120 {
            if !h.ctx.input(|input| input.is_scrolling()) {
                break;
            }
            h.frame();
        }
        assert!(!h.ctx.input(|input| input.is_scrolling()), "inspector wheel must settle before using a fresh handle");
    }
    panic!("{handle}: could not reveal the complete control in the inspector");
}

fn stable_glass_draft_click(h: &mut Harness, handle: &str) -> usize {
    let scrolls = stable_glass_draft_reveal(h, handle);
    let rect = crate::scenario::rect_of(handle).unwrap();
    let inspector = crate::scenario::rect_of("render:inspector").unwrap();
    assert!(rect.is_positive() && inspector.contains_rect(rect) && stable_glass_draft_viewport(h).contains_rect(rect), "{handle}: {rect:?}");
    h.step(&json!({"click":{"handle":handle}})).unwrap();
    h.frames(2);
    scrolls
}

#[test]
fn stable_glass_pointer_draft_composes_saved_glass_maps_coat_and_lens_until_one_explicit_save() {
    use solidcraft_engine::doc::render_optics::{OpticalMaterial, SavedOptics};
    for method in [ImageMethod::CpuRaytrace, ImageMethod::GpuRaytrace] {
        let factory_width = crate::prefs::Prefs::default().render_inspector_width;
        let mut h = stable_glass_draft_fixture(method, true);
        // Keep the unmodified305pt factory preference and use ordinary wheel navigation.
        h.call("ui.resize", json!({"width":1440,"height":760}));
        assert_eq!(h.ctx.content_rect().size(), egui::vec2(1440.0, 760.0), "the pointer journey must use the requested ordinary viewport");
        h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body{name:"Glass B".into()}]})).unwrap();
        let selected = object_selection(&h.app);
        selection_editor(&mut h.app, &selected);
        h.app.run("view.render_stage", json!({"stage":"look"})).unwrap();
        h.app.cam.yaw += 0.125;
        h.frames(3);
        let document = h.app.session.doc.clone();
        let model = h.app.session.model.state();
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        let camera = h.app.cam;
        let selection = h.app.session.selection.clone();
        let saved = document.render.setups["Studio"].clone();
        assert_ne!(camera, saved.camera, "the optical save must preserve an independently saved camera");
        assert_eq!((saved.optics.len(), saved.surfaces.len(), saved.coats.len()), (1, 1, 1));
        let mut scrolls = stable_glass_draft_click(&mut h, &format!("dialog:section:{}", crate::i18n::tr("render.appearance_editor.glass")));
        scrolls += stable_glass_draft_click(&mut h, "render:glass:enable");
        let absorption = h.app.tr("render.appearance_editor.absorption").to_owned();
        for axis in 0..3 {
            scrolls += stable_glass_draft_reveal(&mut h, &format!("dialog:vector:{absorption}:{axis}"));
            let field = crate::scenario::rect_of(&format!("dialog:vector:{absorption}:{axis}")).unwrap();
            let column = crate::scenario::rect_of(&format!("dialog:control:{absorption} {}", ["R", "G", "B"][axis])).unwrap();
            assert!(column.contains_rect(field), "absorption{axis}: {field:?}, {column:?}");
        }
        let binding = h.app.render_workspace.view.materials.bindings.iter().find(|binding| binding.target.name == "Glass B").unwrap();
        assert!(binding.glass_enabled && !binding.has_runtime_intent());
        let target = binding.target.stable_target.clone().unwrap();
        let mut expected = saved.clone();
        expected.camera = camera;
        expected.optics.set(SavedOptics { target, optics: OpticalMaterial::default() }, false).unwrap();
        let request = image_request(&h.app).unwrap();
        assert_eq!(request, json!({"settings":expected}));
        let mut repeated = request.clone();
        materials::params(&h.app, &mut repeated).unwrap();
        assert_eq!(repeated, request, "already prospective stable rows are idempotent");
        assert!(
            setups::snapshot(&h.app).unwrap_err().contains(h.app.tr("render.setup_runtime_unsupported")),
            "generic Save cannot silently commit Glass drafts"
        );
        h.frames(4);
        assert_eq!(h.app.preferences.render_inspector_width, factory_width, "painting and scrolling cannot widen the inspector");
        assert_eq!(image_request(&h.app).unwrap(), request);
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert!(Arc::ptr_eq(&model, &h.app.session.model.state()));
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
        assert_eq!(h.app.session.selection, selection);
        assert_eq!(h.app.cam, camera);
        assert!(h.app.render_workspace.view.job.is_none());
        scrolls += stable_glass_draft_click(&mut h, &format!("dialog:section:{}", crate::i18n::tr("render.appearance_editor.saved")));
        scrolls += stable_glass_draft_reveal(&mut h, "render:optics-save");
        let save_label = h.app.tr("render.appearance_editor.save").to_owned();
        let save_enabled = {
            let tree = h.output.accesskit_update.as_ref().unwrap();
            let mut buttons =
                tree.nodes.iter().filter(|(_, node)| node.role() == egui::accesskit::Role::Button && node.label() == Some(save_label.as_str()));
            let enabled = !buttons.next().expect("the actual optics Save button must be accessible").1.is_disabled();
            assert!(buttons.next().is_none(), "the reviewed optics Save action must be unambiguous");
            enabled
        };
        assert!(save_enabled, "optics Save is disabled before the real click; status={:?}", h.app.status);
        scrolls += stable_glass_draft_click(&mut h, "render:optics-save");
        assert!(scrolls > 0, "the ordinary-height inspector journey must use actual wheel input");
        expected.camera = saved.camera;
        let committed = h.app.session.doc.clone();
        assert_eq!(
            committed.render.setups["Studio"], expected,
            "actual Save enabled={save_enabled}, status={:?}, last command={:?}",
            h.app.status, h.app.last_command
        );
        assert_eq!(h.app.preferences.render_inspector_width, factory_width, "explicit Save retains the factory inspector width");
        assert_eq!(h.app.session.undo.len(), history.1 + 1);
        assert_eq!(committed.features, document.features);
        assert_eq!(committed.materials, document.materials);
        assert_eq!(committed.appearances, document.appearances);
        assert!(Arc::ptr_eq(&model, &h.app.session.model.state()));
        assert_eq!(h.app.cam, camera);
        assert_eq!(solidcraft_engine::doc::Document::from_json(&committed.to_json()).unwrap(), *committed);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc, document);
        assert_eq!(h.app.cam, camera);
        h.app.run("edit.redo", json!({})).unwrap();
        assert_eq!(h.app.session.doc, committed);
        assert_eq!(h.app.cam, camera);
    }
}

#[test]
fn stable_glass_drafts_survive_checked_file_hdr_and_both_request_builder_orders() {
    for method in [ImageMethod::CpuRaytrace, ImageMethod::GpuRaytrace] {
        let mut h = stable_glass_draft_fixture(method, false);
        stable_glass_draft_add(&mut h, "Glass B");
        let typed = image_request(&h.app).unwrap();
        let target = if method == ImageMethod::CpuRaytrace { hdr::Target::Cpu } else { hdr::Target::Gpu };
        let path = h.dir.join("stable-glass-parity.hdr");
        std::fs::write(&path, original_hdr_bytes(128, true)).unwrap();
        if method == ImageMethod::CpuRaytrace {
            h.app.render_workspace.view.cpu.environment = cpu::Environment::Hdr;
        } else {
            h.app.render_workspace.view.studio.environment = studio::Environment::Hdr;
        }
        hdr::inspect(&mut h.app, target, path.to_string_lossy().into_owned()).unwrap();
        assert!(matches!(hdr::input(&h.app, target), hdr::Input::FileCaptured { .. }));
        let document = h.app.session.doc.clone();
        let model = h.app.session.model.state();
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        let camera = h.app.cam;
        let request = image_request(&h.app).unwrap();
        assert_eq!(request["settings"]["optics"], typed["settings"]["optics"]);
        assert_eq!(request["settings"]["coats"], typed["settings"]["coats"]);
        assert_eq!(request["settings"]["camera"], typed["settings"]["camera"]);
        assert_eq!(request["settings"]["optics"].as_array().unwrap().len(), 2);
        assert!(
            request.pointer("/raytrace/dielectrics").is_none_or(|rows| rows.as_array().is_some_and(Vec::is_empty)),
            "stable Glass never becomes a duplicate ordinal override"
        );
        if method == ImageMethod::CpuRaytrace {
            assert_eq!(request["raytrace"]["lens"], typed["settings"]["job"]["options"]["lens"]);
            assert_eq!(request["raytrace"]["path"], typed["settings"]["job"]["options"]["path"]);
        } else {
            assert_eq!(request["lens"], typed["settings"]["job"]["options"]["lens"]);
            assert_eq!(request["max_depth"], typed["settings"]["job"]["options"]["max_depth"]);
            assert_eq!(request["lighting"], typed["settings"]["job"]["options"]["lighting"]);
            assert_eq!((request["samples"].as_u64(), request["seed"].as_u64()), (Some(4), Some(17)));
        }
        let mut twice = request.clone();
        materials::params(&h.app, &mut twice).unwrap();
        assert_eq!(twice, request);
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert!(Arc::ptr_eq(&model, &h.app.session.model.state()));
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
        assert_eq!(h.app.cam, camera);
        assert!(h.app.render_workspace.view.job.is_none());
        if method == ImageMethod::GpuRaytrace {
            // This executes the real engine capture/Check route against a metadata
            // protocol executor. It never opens a device or qualifies GPU pixels.
            use serde_json::Value;
            use solidcraft_engine::render::{RenderFeature, job};
            struct Capture(Arc<std::sync::Mutex<Option<Value>>>);
            impl job::ImageExecutor for Capture {
                fn supports_linear_exr(&self) -> bool {
                    true
                }
                fn descriptor(&self) -> job::Result<ExecutorDescriptor> {
                    let mut descriptor = job::CpuHost::cooperative().descriptor()?;
                    descriptor.id = ExecutorId::new("gpu-path").unwrap();
                    descriptor.profiles[0].features =
                        vec![RenderFeature::Hdri, RenderFeature::Pbr, RenderFeature::Refraction, RenderFeature::Progressive];
                    Ok(descriptor)
                }
                fn admit(&self, scene: &ProductScene, request: &job::ImageRequest, options: job::JobOptions) -> job::Result<job::Admission> {
                    let admission = job::admit_geometry(scene, request, job::MAX_GPU_PATH_TRIANGLES)?;
                    // Copy bounded scalar metadata, not source meshes/HDR Arcs.
                    let bodies: Vec<_> = scene
                        .meshes
                        .iter()
                        .enumerate()
                        .map(|(index, body)| {
                            json!({
                                "index":index,"name":body.name,"ior":body.surface.map(|surface|surface.dielectric_ior()),
                                "absorption":body.transmission.map(|medium|{let c=medium.extinction();[c.x,c.y,c.z]}),
                                "has_coat":body.surface.is_some_and(|surface|surface.clear_coat().is_some())
                            })
                        })
                        .collect();
                    *self.0.lock().unwrap() = Some(json!({"bodies":bodies,"camera":request.camera,
                        "environment":request.environment.as_ref().map(|source|source.info()),"options":options}));
                    Ok(admission)
                }
                fn begin(
                    &self,
                    _: ProductScene,
                    _: job::ImageRequest,
                    _: job::JobOptions,
                    _: job::CancelToken,
                ) -> job::Result<Box<dyn job::ImageWork>> {
                    Err(job::JobError::new(job::ErrorCode::HostUnavailable, "protocol capture test must not begin image work"))
                }
            }
            let captured = Arc::new(std::sync::Mutex::new(None));
            let host = job::ImageHost::with_executor(Arc::new(Capture(captured.clone()))).unwrap();
            h.app.session.set_render_job_host(Some(host.clone())).unwrap();
            let checked = h.app.run("render.preflight", request.clone()).unwrap();
            assert!(checked.get("job").is_none());
            assert_eq!(checked["resolved"]["executor"], "gpu-path");
            let captured = captured.lock().unwrap().take().unwrap();
            for (name, ior, absorption) in [("Glass A", 1.43, [0.01, 0.02, 0.03]), ("Glass B", 1.5, [0.0; 3])] {
                let target = materials::read_targets(&mut h.app).unwrap().into_iter().find(|target| target.name == name).unwrap();
                let body = captured["bodies"].as_array().unwrap().iter().find(|body| body["name"] == name).unwrap();
                assert_eq!(body["index"], json!(target.ordinal));
                assert_eq!(body["ior"], json!(ior));
                assert_eq!(body["absorption"], json!(absorption));
            }
            assert!(captured["bodies"].as_array().unwrap().iter().any(|body| body["name"] == "Coat" && body["has_coat"] == true));
            assert_eq!(captured["camera"], json!(camera));
            assert_eq!(captured["environment"], checked["environment"]);
            assert_eq!(captured["environment"]["source_sha256"], json!(hdr::input(&h.app, target).resource().unwrap()));
            let expected_options = job::JobOptions {
                samples: h.app.render_workspace.view.samples,
                max_depth: h.app.render_workspace.view.max_depth,
                seed: h.app.render_workspace.view.seed,
                lens: Some(h.app.render_workspace.view.studio.lens),
                lighting: serde_json::from_value(request["lighting"].clone()).unwrap(),
                linear_exr: true,
                ..Default::default()
            };
            assert_eq!(captured["options"], json!(expected_options), "all GPU lighting, lens and numerical options reach capture");
            assert_eq!(host.admitted_bytes(), 0);
            assert!(Arc::ptr_eq(&document, &h.app.session.doc));
            assert!(Arc::ptr_eq(&model, &h.app.session.model.state()));
            assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
            assert_eq!(h.app.cam, camera);
        }
    }
}

#[test]
fn stable_glass_draft_stays_dirty_across_refresh_navigation_and_cancelled_shot_switch() {
    for method in [ImageMethod::CpuRaytrace, ImageMethod::GpuRaytrace] {
        let mut h = stable_glass_draft_fixture(method, false);
        h.app.run("render.settings.set", json!({"name":"Other","set":{"width":8,"height":8,"job":{"method":"raster"}}})).unwrap();
        h.app.render_workspace.view.name = "Studio".into();
        setups::load(&mut h.app);
        stable_glass_draft_add(&mut h, "Glass B");
        assert!(!setups::requires_runtime_request(&h.app));
        assert!(setups::has_runtime_inputs(&h.app), "portable optics are still unsaved local intent");
        h.app.render_workspace.view.dirty = false;
        setups::refresh_dirty(&mut h.app);
        assert!(h.app.render_workspace.view.dirty);
        let request = image_request(&h.app).unwrap();
        let document = h.app.session.doc.clone();
        let camera = h.app.cam;
        enter(&mut h.app, false).unwrap();
        h.app.cam.yaw += 0.3;
        enter(&mut h.app, true).unwrap();
        assert_eq!(h.app.cam, camera, "Render navigation is not replaced by saved-camera hydration");
        assert_eq!(image_request(&h.app).unwrap(), request);
        assert!(h.app.render_workspace.view.dirty);
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        switch_shot(&mut h.app, "Other".into());
        assert!(inspector::confirmation_open(&h.app));
        assert_eq!(h.app.render_workspace.view.name, "Studio");
        assert_eq!(image_request(&h.app).unwrap(), request);
        h.frames(3);
        h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
        setups::refresh_dirty(&mut h.app);
        assert!(h.app.render_workspace.view.dirty);
        assert_eq!(image_request(&h.app).unwrap(), request);
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
        assert_eq!(h.app.cam, camera);
    }
}

#[test]
fn stable_glass_draft_refusals_keep_saved_targets_unknown_identity_and_maps_hdr_intent() {
    for method in [ImageMethod::CpuRaytrace, ImageMethod::GpuRaytrace] {
        // Hostile local state cannot masquerade as an edit of an existing saved row.
        let mut h = stable_glass_draft_fixture(method, false);
        stable_glass_draft_add(&mut h, "Glass A");
        let document = h.app.session.doc.clone();
        let before = (h.app.session.revision, h.app.session.undo.len(), h.app.cam);
        assert!(image_request(&h.app).unwrap_err().contains(h.app.tr("render.material_revalidate_needed")));
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.cam), before);

        let mut h = stable_glass_draft_fixture(method, false);
        stable_glass_draft_add(&mut h, "Glass B");
        h.app.render_workspace.view.materials.bindings[0].target.stable_target = None;
        assert!(h.app.render_workspace.view.materials.bindings[0].has_runtime_intent());
        let document = h.app.session.doc.clone();
        let before = (h.app.session.revision, h.app.session.undo.len(), h.app.cam);
        if method == ImageMethod::CpuRaytrace {
            assert!(image_request(&h.app).unwrap_err().contains(h.app.tr("render.material_cpu_saved_runtime_conflict")));
        } else {
            let request = image_request(&h.app).unwrap();
            assert_eq!(request["settings"]["optics"], json!(document.render.setups["Studio"].optics));
            assert_eq!(request["raytrace"]["dielectrics"][0]["body_name"], "Glass B");
        }
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.cam), before);

        let mut h = stable_glass_draft_fixture(method, true);
        stable_glass_draft_add(&mut h, "Glass B");
        let target = if method == ImageMethod::CpuRaytrace { hdr::Target::Cpu } else { hdr::Target::Gpu };
        let path = h.dir.join("saved-maps-glass.hdr");
        std::fs::write(&path, original_hdr_bytes(128, true)).unwrap();
        if method == ImageMethod::CpuRaytrace {
            h.app.render_workspace.view.cpu.environment = cpu::Environment::Hdr;
        } else {
            h.app.render_workspace.view.studio.environment = studio::Environment::Hdr;
        }
        hdr::inspect(&mut h.app, target, path.to_string_lossy().into_owned()).unwrap();
        let document = h.app.session.doc.clone();
        let before = (h.app.session.revision, h.app.session.undo.len(), h.app.cam);
        assert!(image_request(&h.app).unwrap_err().contains(h.app.tr("render.saved_maps_runtime_conflict")));
        assert_eq!(h.app.render_workspace.view.draft.surfaces, document.render.setups["Studio"].surfaces);
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.cam), before);

        let mut h = stable_glass_draft_fixture(method, false);
        stable_glass_draft_add(&mut h, "Glass B");
        // Bypass UI post-hooks to model an unacknowledged embedding/source replacement.
        h.app.session.execute("render.settings.set", &json!({"name":"Unrelated","set":{"width":13}})).unwrap();
        let document = h.app.session.doc.clone();
        assert!(image_request(&h.app).unwrap_err().contains(h.app.tr("render.material_revalidate_needed")));
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    }
}

#[test]
fn stable_glass_draft_capture_keeps_the_existing_cpu_eight_and_gpu_thirty_two_limits() {
    for (method, limit) in [(ImageMethod::CpuRaytrace, 8), (ImageMethod::GpuRaytrace, 32)] {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"body_name":"Optics","length":1,"width":1,"height":1})).unwrap();
        h.app.run("solid.pattern.rectangular", json!({"bodies":["Optics"],"dir1":[1,0,0],"count1":limit+1,"spacing1":3})).unwrap();
        let targets = materials::read_targets(&mut h.app).unwrap();
        assert_eq!(targets.len(), limit + 1);
        h.app.cam.fov = 0.6;
        let job = if method == ImageMethod::CpuRaytrace {
            json!({"method":"cpu_raytrace","options":{"progressive":{"samples_per_pixel":4,"seed":17},"path":{"max_depth":4}}})
        } else {
            json!({"method":"gpu_path","options":{"samples":4,"max_depth":4,"seed":17}})
        };
        h.app.run("render.settings.set", json!({"name":"Studio","set":{"width":8,"height":8,"camera":h.app.cam,"job":job}})).unwrap();
        setups::load(&mut h.app);
        for target in targets.iter().take(limit) {
            let mut binding = materials::Binding::new(target.clone(), &h.app);
            binding.glass_enabled = true;
            h.app.render_workspace.view.materials.bindings.push(binding);
        }
        h.app.render_workspace.view.materials.enabled = true;
        let document = h.app.session.doc.clone();
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        assert_eq!(image_request(&h.app).unwrap()["settings"]["optics"].as_array().unwrap().len(), limit);
        let mut binding = materials::Binding::new(targets[limit].clone(), &h.app);
        binding.glass_enabled = true;
        h.app.render_workspace.view.materials.bindings.push(binding);
        assert!(image_request(&h.app).is_err(), "{method:?}: over-limit targets must refuse rather than disappear");
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
    }
}

#[test]
fn render_results_batch_continues_in_background_tabs_and_keeps_each_shot_result_with_its_document() {
    let mut h = render_results_batch_fixture();
    let document = h.app.session.doc.clone();
    let before = (h.app.session.undo.len(), h.app.session.redo.len());
    let captured_revision = h.app.session.revision;
    let host = h.app.session.render_job_host().unwrap().clone();
    h.app.run("render.batch", json!({"request":{"settings":{"width":8,"height":8}}})).unwrap();
    crate::documents::open_tab(&mut h.app, Session::default()).unwrap();
    let other = h.app.session.doc.clone();
    for _ in 0..3 {
        for _ in 0..100 {
            if !host.pump(4096).unwrap() {
                break;
            }
        }
        h.frames(1);
    }
    assert!(Arc::ptr_eq(&other, &h.app.session.doc));
    assert!(h.app.session.render_retained_results().unwrap().is_empty());
    crate::documents::switch(&mut h.app, 0);
    h.frames(2);
    let results = h.app.session.render_retained_results().unwrap();
    assert_eq!(results.iter().map(|result| result.shot.as_deref().unwrap()).collect::<Vec<_>>(), ["Front", "Hero", "Top"]);
    assert_eq!((h.app.session.render_queue_state().running, h.app.session.render_queue_state().waiting), (0, 0));
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(before, (h.app.session.undo.len(), h.app.session.redo.len()));
    assert!(results.iter().all(|result| result.frame.scene_generation == captured_revision));
}

#[test]
fn host_product_fit_uses_frame_and_selected_shot_projection_with_its_own_fov() {
    use crate::panel_runtime::{logical_rect, resolve_usable_region};
    use crate::viewport_layout::Layout;
    use egui::{Rect, pos2, vec2};
    use solidcore_record::Vec3;
    use solidcraft_engine::geom::Aabb3;
    let mut h = Harness::new();
    enter(&mut h.app, true).unwrap();
    let name = h.app.render_workspace.view.name.clone();
    h.app.run("render.settings.set", json!({"name":name,"set":{"width":1024,"height":4096}})).unwrap();
    setups::load(&mut h.app);
    let viewport = Rect::from_min_max(pos2(0.0, 34.0), pos2(1400.0, 950.0));
    h.app.shell.content = Some(viewport);
    h.app.shell.workspace_panels.clear();
    h.app.preferences.perspective_fov = 1.2;
    h.app.ui.perspective = true;
    let bounds = Aabb3 { min: Vec3::new(-60.0, -10.0, -5.0), max: Vec3::new(60.0, 10.0, 5.0) };
    for selected_shot in [false, true] {
        h.app.render_workspace.view.stage = if selected_shot { Stage::Look } else { Stage::Frame };
        h.app.render_workspace.view.selection = if selected_shot { RenderSelection::Shot(name.clone()) } else { RenderSelection::Nothing };
        let picture = shot::preview_rect(&h.app, viewport).unwrap();
        assert!((picture.width() / picture.height() - 0.25).abs() < 1e-5);
        let obstacle = Rect::from_min_size(picture.min, vec2(picture.width() * 0.3, 160.0));
        let usable = resolve_usable_region(logical_rect(viewport), &[logical_rect(obstacle)]).unwrap();
        let layout = Layout::host_with_axis(viewport, &usable, crate::prefs::NavigatorStyle::Axes, egui::Vec2::ZERO, Rect::NOTHING, false);
        for fov in [0.0, 0.95] {
            let camera = Camera { fov, ..Default::default() };
            let fitted = fit_host_product_bounds(&h.app, camera, &bounds, &layout, &usable).unwrap();
            assert_eq!(fitted.fov, fov, "the Design FOV preference must never replace the product lens");
            let radius = (bounds.diagonal() + bounds.center().dist(fitted.target)).max(fitted.half_height() * 2.0).max(10.0);
            let mut footprint = Rect::NOTHING;
            for point in corners(&bounds) {
                let (x, y, _) = fitted.to_screen(point, f64::from(picture.width()), f64::from(picture.height()), radius).unwrap();
                footprint.extend_with(pos2(picture.left() + x as f32, picture.top() + y as f32));
            }
            assert!(picture.contains_rect(footprint.expand(Tokens::get().space_4)));
            assert!(!footprint.expand(Tokens::get().space_4).intersect(obstacle).is_positive());
            let centred = centre_host_product_camera(&h.app, camera, Some(&bounds), &layout, &usable).unwrap();
            assert_eq!(centred.distance.to_bits(), camera.distance.to_bits());
            assert_eq!(centred.fov, camera.fov);
        }
    }
}

#[test]
fn host_bounded_results_and_comparison_actions_are_visible_and_clickable() {
    use egui::{Event, Modifiers, PointerButton};
    for comparison in [false, true] {
        let (mut h, _, selected) = render_results_compare_fixture();
        h.app.preferences.language = "en".into();
        h.call("ui.resize", json!({"width":1440,"height":960}));
        h.app.run("view.render_stage", json!({"stage":"frame"})).unwrap();
        crate::panel_runtime::enable_host_specimen(&mut h.app, "tabs").unwrap();
        let select = |h: &mut Harness| {
            if comparison {
                h.app.run("render.compare", json!({"left":0,"right":1})).unwrap();
            } else {
                h.app.run("render.result.select", json!({"index":1})).unwrap();
            }
            h.frames(4);
        };
        select(&mut h);
        h.app.pending_fit = None;
        h.app.cancel_view_animation();
        let canvas = h.app.viewport.rect.unwrap();
        let document = h.app.session.doc.clone();
        let history = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
        let product = h.app.cam;
        let card = h.app.shell.workspace_panels["render:result-controls"].0;
        assert!(card.width() <= 528.0, "exercise the native report's bounded card: {card:?}");
        for target in [
            "render:results",
            "render:result-discard",
            "render:result-model",
            "render:result-fit",
            "render:result-actual",
            "render:result-copy",
            "render:result-export",
            "render:result-close",
        ] {
            let rect = crate::scenario::rect_of(target).unwrap();
            assert!(card.contains_rect(rect), "{target}: {rect:?} outside {card:?}, comparison={comparison}");
            assert!(crate::scenario::rect_of("render:result-actions-clip").unwrap().contains_rect(rect), "{target} is clipped");
        }
        let click = |h: &mut Harness, target: &str| {
            let rect = crate::scenario::rect_of(target).unwrap();
            assert!(h.app.shell.workspace_panels["render:result-controls"].0.contains_rect(rect), "{target}");
            assert!(crate::scenario::rect_of("render:result-actions-clip").unwrap().contains_rect(rect), "{target}");
            let pos = rect.center();
            h.frame_with(vec![Event::PointerMoved(pos)]);
            h.frame_with(vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
            h.frame_with(vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
        };
        click(&mut h, "render:result-actual");
        assert_eq!(h.app.render_workspace.view.result_scale, ResultScale::Actual);
        click(&mut h, "render:result-fit");
        assert_eq!(h.app.render_workspace.view.result_scale, ResultScale::Fit);
        if comparison {
            click(&mut h, "render:compare:swap");
            assert_eq!(h.app.render_workspace.view.compare.as_ref().unwrap().left, Some(selected));
        }
        click(&mut h, "render:result-copy");
        let mut copied = false;
        for _ in 0..4 {
            copied |= h.output.commands.iter().any(|command| matches!(command, egui::OutputCommand::CopyImage(image) if image.size == [16, 16]));
            h.frame();
        }
        assert!(copied, "Copy must receive a real pointer click inside the card");
        click(&mut h, "render:result-export");
        h.settle();
        assert!(h.dir.join("render.png").is_file(), "Export must receive the click and use the harness picker");
        click(&mut h, "render:result-model");
        assert!(!result_active(&h.app));
        assert!(h.app.render_workspace.view.compare.is_none());
        select(&mut h);
        click(&mut h, "render:result-close");
        assert!(!result_active(&h.app));
        assert!(h.app.render_workspace.view.compare.is_none());
        assert_eq!(h.app.render_workspace.view.completed, Some(selected));
        assert_eq!(h.app.viewport.rect, Some(canvas));
        assert_eq!(h.app.cam, product);
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!((h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision), history);
    }
}

/// Render the production legacy card at an assigned bound; no duplicate toolbar
/// or controller logic. Full-app layout/navigation remain covered above.
fn bounded_result_card_frame(h: &mut Harness, bounds: egui::Rect, events: Vec<egui::Event>) -> egui::Rect {
    let canvas = h.app.viewport.rect.unwrap();
    let time = h.ctx.input(|input| input.time) + 1.0 / 60.0;
    crate::scenario::clear_handles();
    let mut painted = egui::Rect::NOTHING;
    let mut output =
        h.ctx.run_ui(egui::RawInput { screen_rect: Some(canvas), time: Some(time), focused: true, events, ..Default::default() }, |ui| {
            painted = host_result_card(&mut h.app, ui, bounds, canvas)
        });
    output.textures_delta.clear();
    h.shapes = output.shapes;
    h.output = output.platform_output;
    painted
}

#[test]
fn host_narrow_result_cards_scroll_real_actions_into_their_clip() {
    use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, TouchPhase};
    for comparison in [false, true] {
        for width in [240.0, 120.0] {
            let (mut h, _, _) = render_results_compare_fixture();
            crate::panel_runtime::enable_host_specimen(&mut h.app, "tabs").unwrap();
            if comparison {
                h.app.run("render.compare", json!({"left":0,"right":1})).unwrap();
            } else {
                h.app.run("render.result.select", json!({"index":1})).unwrap();
            }
            h.frames(4);
            let canvas = h.app.viewport.rect.unwrap();
            let product = h.app.cam;
            let image = h.app.render_workspace.view.result_camera;
            let document = h.app.session.doc.clone();
            let history = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
            let bounds = egui::Rect::from_min_size(canvas.min, egui::vec2(width, 96.0));
            for _ in 0..4 {
                assert!(bounds.contains_rect(bounded_result_card_frame(&mut h, bounds, Vec::new())));
            }
            let initial_close = crate::scenario::rect_of("render:result-close").unwrap();
            let mut moved = false;
            for target in [
                "render:result-copy",
                "render:result-export",
                "render:result-fit",
                "render:result-actual",
                "render:result-close",
                "render:result-model",
            ] {
                let clip = crate::scenario::rect_of("render:result-actions-clip").unwrap();
                assert!(bounds.contains_rect(bounded_result_card_frame(
                    &mut h,
                    bounds,
                    vec![
                        Event::PointerMoved(clip.center()),
                        Event::MouseWheel {
                            source: egui::MouseWheelSource::Unknown,
                            unit: MouseWheelUnit::Point,
                            delta: egui::Vec2::ZERO,
                            modifiers: Modifiers::NONE,
                            phase: TouchPhase::Start
                        },
                    ],
                )));
                let mut interaction = None;
                for _ in 0..60 {
                    let rect = crate::scenario::rect_of(target).unwrap();
                    let clip = crate::scenario::rect_of("render:result-actions-clip").unwrap();
                    let visible = rect.intersect(clip).intersect(bounds);
                    // An intrinsic button can be wider than the card. Its clipped
                    // centre must remain a usable target, even in the 120-point case.
                    if visible.width() >= rect.width().min(24.0) && visible.height() >= rect.height().min(16.0) {
                        assert!(bounds.contains_rect(bounded_result_card_frame(
                            &mut h,
                            bounds,
                            vec![Event::MouseWheel {
                                source: egui::MouseWheelSource::Unknown,
                                unit: MouseWheelUnit::Point,
                                delta: egui::Vec2::ZERO,
                                modifiers: Modifiers::NONE,
                                phase: TouchPhase::End,
                            }],
                        )));
                        assert!(!h.ctx.input(|input| input.is_scrolling()));
                        let rect = crate::scenario::rect_of(target).unwrap();
                        let clip = crate::scenario::rect_of("render:result-actions-clip").unwrap();
                        let visible = rect.intersect(clip).intersect(bounds);
                        assert!(visible.width() >= rect.width().min(24.0) && visible.height() >= rect.height().min(16.0));
                        interaction = Some(visible.center());
                        break;
                    }
                    // A bracketed trackpad gesture follows the measured target, never a
                    // reconstructed toolbar or a private scroll-state offset.
                    let delta = clip.center() - rect.center();
                    let painted = bounded_result_card_frame(
                        &mut h,
                        bounds,
                        vec![
                            Event::PointerMoved(clip.center()),
                            Event::MouseWheel {
                                source: egui::MouseWheelSource::Unknown,
                                unit: MouseWheelUnit::Point,
                                delta,
                                modifiers: Modifiers::NONE,
                                phase: TouchPhase::Move,
                            },
                        ],
                    );
                    assert!(bounds.contains_rect(painted));
                    // ScrollArea stores its new offset after laying out the content.
                    // Publish the resulting geometry before measuring another delta.
                    assert!(bounds.contains_rect(bounded_result_card_frame(&mut h, bounds, Vec::new())));
                    moved |= crate::scenario::rect_of("render:result-close").unwrap() != initial_close;
                }
                let pos = interaction.unwrap_or_else(|| panic!("{target} must be reachable by scrolling: width={width}, comparison={comparison}"));
                assert!(bounds.contains(pos));
                assert!(crate::scenario::rect_of("render:result-actions-clip").unwrap().contains(pos));
                // Exercise the widest action through the real clipped widget;
                // other action outcomes are covered by the full-card fixture.
                if target == "render:result-model" {
                    assert!(result_active(&h.app));
                    for events in [
                        vec![Event::PointerMoved(pos)],
                        vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }],
                        vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }],
                    ] {
                        assert!(bounds.contains_rect(bounded_result_card_frame(&mut h, bounds, events)));
                    }
                    assert!(!result_active(&h.app), "the clipped Model button must receive the pointer click");
                    assert!(h.app.render_workspace.view.compare.is_none());
                }
            }
            assert!(moved, "the narrow fixture must exercise actual scrolling");
            assert_eq!(h.app.viewport.rect, Some(canvas));
            assert_eq!(h.app.cam, product);
            assert_eq!(h.app.render_workspace.view.result_camera, image);
            assert!(Arc::ptr_eq(&document, &h.app.session.doc));
            assert_eq!((h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision), history);
        }
    }
}

#[test]
fn host_tiny_result_cards_clip_paint_and_reject_off_card_action_clicks() {
    use egui::{Event, Modifiers, PointerButton};
    for comparison in [false, true] {
        let (mut h, _, _) = render_results_compare_fixture();
        crate::panel_runtime::enable_host_specimen(&mut h.app, "tabs").unwrap();
        if comparison {
            h.app.run("render.compare", json!({"left":0,"right":1})).unwrap();
        } else {
            h.app.run("render.result.select", json!({"index":1})).unwrap();
        }
        h.frames(4);
        let canvas = h.app.viewport.rect.unwrap();
        let bounds = egui::Rect::from_min_size(canvas.min, egui::vec2(48.0, 44.0));
        for _ in 0..4 {
            assert!(bounds.contains_rect(bounded_result_card_frame(&mut h, bounds, Vec::new())));
        }
        let mut visible_shapes = 0;
        for shape in &h.shapes {
            if shape.clip_rect.is_positive() && shape.shape.visual_bounding_rect().is_positive() {
                assert!(bounds.contains_rect(shape.clip_rect), "legacy card paint escaped: {:?}", shape.clip_rect);
                visible_shapes += 1;
            }
        }
        assert!(visible_shapes > 0, "the tiny card must still exercise visible paint clips");
        let pos = crate::scenario::rect_of("render:result-close").unwrap().center();
        assert!(!bounds.contains(pos), "the tiny fixture must put Close outside its real clip");
        assert!(canvas.contains(pos), "exercise clipping inside the screen, rather than an off-screen pointer");
        for events in [
            vec![Event::PointerMoved(pos)],
            vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }],
            vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }],
        ] {
            assert!(bounds.contains_rect(bounded_result_card_frame(&mut h, bounds, events)));
        }
        assert!(result_active(&h.app), "an off-card target rectangle cannot receive a click");
        assert_eq!(h.app.render_workspace.view.compare.is_some(), comparison);
        assert_eq!(h.app.viewport.rect, Some(canvas));
    }
}

#[test]
fn host_results_and_comparison_keep_full_canvas_real_control_footprints_and_cameras_on_panel_changes() {
    use crate::panel_runtime::{egui_rect, logical_rect};
    for comparison in [false, true] {
        let (mut h, _, _) = render_results_compare_fixture();
        crate::panel_runtime::enable_host_specimen(&mut h.app, "docks").unwrap();
        if comparison {
            h.app.run("render.compare", json!({"left":0,"right":1})).unwrap();
        } else {
            h.app.run("render.result.select", json!({"index":1})).unwrap();
        }
        h.frames(4);
        h.app.pending_fit = None;
        h.app.cancel_view_animation();
        let domain = egui_rect(h.app.universal_panel_frame().unwrap().layout.render_domain);
        assert_eq!(h.app.viewport.rect, Some(domain));
        if comparison {
            assert_eq!(crate::scenario::rect_of("render:compare:canvas"), Some(domain));
        }
        let controls = h.app.shell.workspace_panels["render:result-controls"].0;
        assert!(domain.contains_rect(controls));
        assert!(h.app.shell.cards.iter().all(|card| !controls.intersect(*card).is_positive()), "the later ribbon must never cover actions");
        assert!(h.app.universal_panel_frame().unwrap().layout.usable.contains_rect(logical_rect(controls)));
        assert!(crate::shell::host_usable(&h.app).unwrap().obstacles.contains(&logical_rect(controls)));
        let product = h.app.cam;
        let image = h.app.render_workspace.view.result_camera;
        let draft = h.app.render_workspace.view.draft.clone();
        let dirty = h.app.render_workspace.view.dirty;
        let document = h.app.session.doc.clone();
        let history = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
        h.app.run_panel_command("panel.collapse", &json!({"target":{"type":"instance","id":"specimen-0"},"collapsed":true})).unwrap();
        h.frames(4);
        assert_eq!(h.app.viewport.rect, Some(domain));
        assert_eq!(h.app.cam, product);
        assert_eq!(h.app.render_workspace.view.result_camera, image, "an already fitted image is not reframed by panels");
        assert_eq!(h.app.render_workspace.view.draft, draft);
        assert_eq!(h.app.render_workspace.view.dirty, dirty);
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!((h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision), history);
        h.app.run("render.result.view", json!({"view":"actual"})).unwrap();
        h.frames(3);
        let actual = result_rect(h.app.render_workspace.view.result_camera, [16, 16], domain);
        assert!((actual.width() * h.ctx.pixels_per_point() - 16.0).abs() < 1e-4);
        assert_eq!(h.app.cam, product);
    }
}

#[test]
fn host_result_framing_recovers_after_classic_without_refitting_free_cameras_or_panel_moves() {
    use crate::panel_runtime::logical_rect;
    use solidcraft_engine::panels::ActionSink as _;
    for comparison in [false, true] {
        for scale in [ResultScale::Fit, ResultScale::Actual, ResultScale::Free] {
            let (mut h, _, _) = render_results_compare_fixture();
            crate::panel_runtime::enable_host_specimen(&mut h.app, "docks").unwrap();
            if comparison {
                h.app.run("render.compare", json!({"left":0,"right":1})).unwrap();
            } else {
                h.app.run("render.result.select", json!({"index":1})).unwrap();
            }
            h.frames(4);
            h.app.pending_fit = None;
            h.app.cancel_view_animation();
            match scale {
                ResultScale::Fit => {
                    h.app.run("render.result.view", json!({"view":"fit"})).unwrap();
                }
                ResultScale::Actual => {
                    h.app.run("render.result.view", json!({"view":"actual"})).unwrap();
                }
                ResultScale::Free => {
                    with_result_camera(&mut h.app, |app| {
                        app.cam.distance *= 0.7;
                        app.cam.target.x += 7.0;
                        app.cam.target.y -= 3.0;
                    });
                }
            }
            h.frames(4);
            let canvas = h.app.viewport.rect.unwrap();
            let domain = h.app.render_workspace.view.result_domain;
            let image = h.app.render_workspace.view.result_camera;
            let product = h.app.cam;
            let draft = h.app.render_workspace.view.draft.clone();
            let dirty = h.app.render_workspace.view.dirty;
            let document = h.app.session.doc.clone();
            let history = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
            let layout = h.app.panel_runtime().unwrap().layout().inspect();
            let controllers = h.app.panel_runtime().unwrap().controller_count();
            assert_eq!(h.app.render_workspace.view.result_scale, scale);
            assert_eq!(domain.is_some(), scale != ResultScale::Free);
            match scale {
                ResultScale::Fit => {
                    assert!(crate::shell::host_usable(&h.app).unwrap().contains_rect(logical_rect(result_rect(image, [16, 16], canvas))));
                }
                ResultScale::Actual => {
                    assert!((result_rect(image, [16, 16], canvas).width() * h.ctx.pixels_per_point() - 16.0).abs() < 1e-4);
                }
                ResultScale::Free => {}
            }

            // Disable only the host: the same runtime and retained content survive.
            h.app.disable_universal_panel_host().unwrap();
            h.frames(3);
            assert!(!h.app.universal_panel_host_enabled());
            assert_eq!(h.app.render_workspace.view.result_domain, None);
            let classic = h.app.render_workspace.view.result_camera;
            if scale == ResultScale::Free {
                assert_eq!(classic, image);
            } else {
                let classic_canvas = h.app.viewport.rect.unwrap();
                assert_ne!(classic_canvas, canvas);
                assert_eq!(classic, result_camera([16, 16], classic_canvas, h.ctx.pixels_per_point(), scale));
                assert_ne!(classic, image, "Classic must actually reframe the image: comparison={comparison}, scale={scale:?}");
            }
            assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), layout);
            assert_eq!(h.app.panel_runtime().unwrap().controller_count(), controllers);
            h.app.enable_universal_panel_host(Default::default()).unwrap();
            h.frames(4);
            assert_eq!(h.app.viewport.rect, Some(canvas), "the original host domain must return");
            assert_eq!(h.app.render_workspace.view.result_domain, domain);
            assert_eq!(h.app.render_workspace.view.result_camera, image, "comparison={comparison}, scale={scale:?}");
            assert_eq!(h.app.render_workspace.view.result_scale, scale);
            assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), layout);
            assert_eq!(h.app.panel_runtime().unwrap().controller_count(), controllers);

            h.app
                .run_panel_command(
                    "panel.move",
                    &json!({"target":{"type":"instance","id":"specimen-0"},"destination":{
                        "type":"new_section","section":"camera-moved","index":0,
                        "location":{"type":"dock","zone":"right"},"size":{"width":280,"height":240}
                    }}),
                )
                .unwrap();
            h.frames(4);
            assert_ne!(h.app.panel_runtime().unwrap().layout().inspect(), layout, "the panel move must commit");
            assert_eq!(h.app.viewport.rect, Some(canvas));
            assert_eq!(h.app.render_workspace.view.result_camera, image, "panel changes cannot refit {scale:?}");
            assert_eq!(h.app.render_workspace.view.result_domain, domain);
            assert_eq!(h.app.cam, product);
            assert_eq!(h.app.render_workspace.view.draft, draft);
            assert_eq!(h.app.render_workspace.view.dirty, dirty);
            assert!(Arc::ptr_eq(&document, &h.app.session.doc));
            assert_eq!((h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision), history);
        }
    }
}

#[test]
fn host_result_controls_shield_wheel_and_comparison_divider_keeps_capture() {
    use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, vec2};
    let (mut h, _, _) = render_results_compare_fixture();
    crate::panel_runtime::enable_host_specimen(&mut h.app, "docks").unwrap();
    h.app.run("render.compare", json!({"left":0,"right":1})).unwrap();
    h.frames(4);
    h.app.preferences.nav = "maya".into();
    h.app.render_workspace.view.result_scale = ResultScale::Free;
    let product = h.app.cam;
    let image = h.app.render_workspace.view.result_camera;
    let controls = h.app.shell.workspace_panels["render:result-controls"].0;
    h.frame_with(vec![
        Event::PointerMoved(controls.center()),
        Event::MouseWheel {
            source: egui::MouseWheelSource::Unknown,
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, -300.0),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        },
    ]);
    assert_eq!(h.app.cam, product);
    assert_eq!(h.app.render_workspace.view.result_camera, image);
    let canvas = h.app.viewport.rect.unwrap();
    let start = crate::scenario::rect_of("render:compare:handle").unwrap().center();
    assert!(!controls.contains(start));
    h.frame_with(vec![
        Event::ModifiersChanged(Modifiers::ALT),
        Event::PointerMoved(start),
        Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::ALT },
    ]);
    let end = egui::pos2(canvas.right() + 100.0, start.y);
    h.frame_with(vec![
        Event::PointerMoved(end),
        Event::MouseWheel {
            source: egui::MouseWheelSource::Unknown,
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, -300.0),
            modifiers: Modifiers::ALT,
            phase: egui::TouchPhase::Move,
        },
    ]);
    assert!((h.app.render_workspace.view.compare.as_ref().unwrap().position - 0.95).abs() < 1e-6);
    assert_eq!(h.app.render_workspace.view.result_camera, image);
    assert_eq!(h.app.cam, product);
    h.frame_with(vec![Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::ALT }]);
}

#[test]
fn host_panel_capture_blocks_result_navigation_through_release() {
    use crate::panel_runtime::{HostControlTarget, HostPointerTarget, HostResizeEdge, egui_rect};
    use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, vec2};
    for comparison in [false, true] {
        for resize in [false, true] {
            let (mut h, _, _) = render_results_compare_fixture();
            crate::panel_runtime::enable_host_specimen(&mut h.app, "docks").unwrap();
            if comparison {
                h.app.run("render.compare", json!({"left":0,"right":1})).unwrap();
            } else {
                h.app.run("render.result.select", json!({"index":1})).unwrap();
            }
            h.frames(4);
            h.app.pending_fit = None;
            h.app.cancel_view_animation();
            h.app.preferences.nav = "maya".into();
            h.app.preferences.smooth_wheel_zoom = false;
            h.app.render_workspace.view.result_scale = ResultScale::Free;
            let product = h.app.cam;
            let image = h.app.render_workspace.view.result_camera;
            let frame = h.app.universal_panel_frame().unwrap();
            let section = frame.layout.sections.iter().find(|section| section.instance.0 == "specimen-0").unwrap();
            let target = if resize {
                HostPointerTarget::Resize { section: section.section.clone(), edge: HostResizeEdge::Right }
            } else {
                HostPointerTarget::Header { instance: section.instance.clone() }
            };
            let start = egui_rect(frame.controls.iter().find(|control| control.target == HostControlTarget::Pointer(target.clone())).unwrap().bounds)
                .center();
            let canvas = h.app.viewport.rect.unwrap();
            // Below this content-sized side card and away from the comparison divider.
            let end = egui::pos2(canvas.center().x + 80.0, canvas.center().y + 80.0);
            assert!(!crate::shell::covered(&h.app, end));
            assert!(!h.app.viewport.layout.as_ref().unwrap().navigation.contains(end));
            h.frame_with(vec![Event::PointerMoved(start)]);
            h.frame_with(vec![Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
            assert!(host_navigation_captured(&h.app), "the real host control must capture: comparison={comparison}, resize={resize}");
            h.frame_with(vec![Event::PointerMoved(end)]);
            assert!(host_navigation_captured(&h.app));
            assert!(!crate::shell::covered(&h.app, end), "wheel input must be over exposed canvas during capture");
            let wheel = || Event::MouseWheel {
                source: egui::MouseWheelSource::Unknown,
                unit: MouseWheelUnit::Point,
                delta: vec2(0.0, -300.0),
                modifiers: Modifiers::NONE,
                phase: egui::TouchPhase::Move,
            };
            h.frame_with(vec![wheel()]);
            assert_eq!(h.app.render_workspace.view.result_camera, image, "a stationary host drag still owns wheel input");
            h.frame_with(vec![
                Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
                wheel(),
            ]);
            assert!(!host_navigation_captured(&h.app));
            assert_eq!(h.app.render_workspace.view.result_camera, image, "host release cannot zoom the result");
            assert_eq!(h.app.cam, product);
            assert!(h.app.render_workspace.result_navigation.is_none(), "the deferred response is consumed once");
            // Navigation must recover after release. Find a point outside all current chrome.
            h.frames(2);
            let point = (1..10)
                .flat_map(|y| (1..10).map(move |x| canvas.min + canvas.size() * vec2(x as f32 / 10.0, y as f32 / 10.0)))
                .find(|point| !crate::shell::covered(&h.app, *point) && !h.app.viewport.layout.as_ref().unwrap().navigation.contains(*point))
                .unwrap();
            h.frame_with(vec![Event::PointerMoved(point)]);
            h.frame_with(vec![wheel()]);
            assert_ne!(h.app.render_workspace.view.result_camera, image, "fresh canvas wheel input resumes after host release");
            assert_eq!(h.app.cam, product);
        }
    }
}

#[test]
fn disabling_host_in_render_clears_result_controls_before_classic_layout_and_input() {
    use egui::{Event, Modifiers, MouseWheelUnit, vec2};
    for comparison in [false, true] {
        let (mut h, _, _) = render_results_compare_fixture();
        crate::panel_runtime::enable_host_specimen(&mut h.app, "docks").unwrap();
        if comparison {
            h.app.run("render.compare", json!({"left":0,"right":1})).unwrap();
        } else {
            h.app.run("render.result.select", json!({"index":1})).unwrap();
        }
        h.frames(4);
        h.app.pending_fit = None;
        h.app.cancel_view_animation();
        let controls = h.app.shell.workspace_panels["render:result-controls"].0;
        let product = h.app.cam;
        let document = h.app.session.doc.clone();
        let history = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
        h.app.disable_universal_panel_host().unwrap();
        // The stale entry exists at the transition; the first real Classic frame must remove it.
        assert!(h.app.shell.workspace_panels.contains_key("render:result-controls"));
        h.frame();
        assert!(active(&h.app));
        assert!(!h.app.universal_panel_host_enabled());
        assert_eq!(h.app.render_workspace.view.compare.is_some(), comparison);
        assert!(!h.app.shell.workspace_panels.contains_key("render:result-controls"));
        assert!(!crate::shell::fit_rects(&h.app).contains(&controls));
        assert!(!crate::shell::covered_rects(&h.app).contains(&controls));
        let layout = h.app.viewport.layout.as_ref().unwrap().clone();
        assert!(layout.usable.is_none());
        // Recomputing from the now-clean shell must match what Classic already consumed.
        crate::viewport::refresh_layout(&mut h.app, &h.ctx);
        let refreshed = h.app.viewport.layout.as_ref().unwrap();
        assert_eq!((layout.content, layout.middle, layout.drawable), (refreshed.content, refreshed.middle, refreshed.drawable));
        // Classic paints the image before its legacy panels update shell.content.
        // Keep the first-frame cleanup assertions above, then target the settled
        // Classic image rather than the previous host's full content rectangle.
        h.frames(3);
        let layout = h.app.viewport.layout.as_ref().unwrap().clone();
        let canvas = h.app.viewport.rect.unwrap();
        let responses = h.ctx.viewport(|viewport| {
            viewport
                .prev_pass
                .widgets
                .layers()
                .flat_map(|(_, widgets)| widgets.iter())
                .filter(|widget| widget.enabled && widget.rect == canvas && widget.sense == egui::Sense::click_and_drag())
                .copied()
                .collect::<Vec<_>>()
        });
        assert_eq!(responses.len(), 1, "identify the actual Classic image allocation, comparison={comparison}: {responses:?}");
        let response = responses[0];
        let divider =
            comparison.then(|| crate::scenario::rect_of("render:compare:divider").unwrap().expand(h.ctx.global_style().interaction.interact_radius));
        let point = (1..10)
            .flat_map(|y| (1..10).map(move |x| controls.min + controls.size() * vec2(x as f32 / 10.0, y as f32 / 10.0)))
            .chain((1..10).flat_map(|y| (1..10).map(move |x| canvas.min + canvas.size() * vec2(x as f32 / 10.0, y as f32 / 10.0))))
            .find(|point| {
                response.interact_rect.contains(*point)
                    && !crate::shell::covered(&h.app, *point)
                    && !layout.navigation.contains(*point)
                    && !divider.is_some_and(|divider| divider.contains(*point))
                    && h.ctx.layer_id_at(*point) == Some(response.layer_id)
            })
            .expect("an unoccluded point owned by the actual Classic image response");
        h.app.preferences.nav = "maya".into();
        h.app.preferences.smooth_wheel_zoom = false;
        h.app.render_workspace.view.result_scale = ResultScale::Free;
        assert!(!crate::frame::dock_dragging(&h.ctx), "a dock drag must not own Classic image input, comparison={comparison}");
        h.frame_with(vec![Event::PointerMoved(point)]);
        assert_eq!(h.app.viewport.mouse, Some(point), "shared navigation must accept the Classic image pointer, comparison={comparison}");
        let owned = h.ctx.read_response(response.id).expect("the Classic image response remains mounted");
        assert!(
            owned.enabled() && owned.contains_pointer() && owned.interact_rect.contains(point),
            "wheel input must belong to the image: {owned:?}"
        );
        assert_eq!(h.ctx.layer_id_at(point), Some(owned.layer_id));
        assert!(!comparison_drag_captured(&h.app), "the comparison divider must not own image input");
        let image = h.app.render_workspace.view.result_camera;
        h.frame_with(vec![Event::MouseWheel {
            source: egui::MouseWheelSource::Unknown,
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, -300.0),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        }]);
        assert!(!crate::frame::dock_dragging(&h.ctx), "a dock drag must not own Classic image wheel input, comparison={comparison}");
        assert!(!comparison_drag_captured(&h.app), "the comparison divider must not own image wheel input");
        assert_eq!(h.app.viewport.mouse, Some(point), "shared navigation must accept the Classic image wheel pointer, comparison={comparison}");
        assert_ne!(
            h.app.render_workspace.view.result_camera,
            image,
            "the invisible host card must not block Classic wheel input: comparison={comparison}, mouse={:?}, raw/available events/smooth delta={:?}, wheel speed={}, scale/job={:?}",
            h.app.viewport.mouse,
            h.ctx.input(|input| (input.raw.events.clone(), input.events.clone(), input.smooth_scroll_delta)),
            h.app.preferences.wheel_zoom_speed,
            (h.app.render_workspace.view.result_scale, h.app.render_workspace.view.result_job)
        );
        h.app.run("render.result.view", json!({"view":"fit"})).unwrap();
        h.frame();
        assert_eq!(
            h.app.render_workspace.view.result_camera,
            result_camera([16, 16], h.app.viewport.rect.unwrap(), h.ctx.pixels_per_point(), ResultScale::Fit)
        );
        assert_eq!(h.app.cam, product);
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!((h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision), history);
    }
}

#[test]
fn host_empty_result_keeps_full_domain_and_tiny_blocked_controls_publish_nothing() {
    let mut h = output_viewer_fixture();
    crate::panel_runtime::enable_host_specimen(&mut h.app, "docks").unwrap();
    h.frames(3);
    h.app.render_workspace.view.result = true;
    let domain = h.app.viewport.rect.unwrap();
    let camera = h.app.render_workspace.view.result_camera;
    let ctx = h.ctx.clone();
    for size in [egui::vec2(4.0, 4.0), egui::vec2(40.0, 24.0), domain.size()] {
        let canvas = egui::Rect::from_min_size(domain.min, size);
        h.app.viewport.host_windows = Some(vec![domain]);
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(domain), ..Default::default() }, |ui| {
            let mut scene = ui.new_child(egui::UiBuilder::new().max_rect(canvas));
            show(&mut h.app, &mut scene);
            host_result_overlays(&mut h.app, ui);
        });
        output.textures_delta.clear();
        assert_eq!(h.app.viewport.rect, Some(canvas));
        assert!(!h.app.shell.workspace_panels.contains_key("render:result-controls"));
        assert_eq!(h.app.render_workspace.view.result_camera, camera);
    }
}

#[test]
fn classic_results_and_comparison_keep_their_reserved_chrome_and_scale_policy() {
    for comparison in [false, true] {
        let (mut h, _, _) = render_results_compare_fixture();
        assert!(!h.app.universal_panel_host_enabled(), "the production default remains Classic");
        if comparison {
            h.app.run("render.compare", json!({"left":0,"right":1})).unwrap();
        } else {
            h.app.run("render.result.select", json!({"index":1})).unwrap();
        }
        h.frames(3);
        let canvas = h.app.viewport.rect.unwrap();
        assert!(canvas.top() > crate::toolbar::APP_BAR_HEIGHT);
        assert_eq!(h.app.render_workspace.view.result_camera, result_camera([16, 16], canvas, h.ctx.pixels_per_point(), ResultScale::Fit));
        assert!(!h.app.shell.workspace_panels.contains_key("render:result-controls"));
        if comparison {
            assert_eq!(crate::scenario::rect_of("render:compare:canvas"), Some(canvas));
        }
    }
}

fn advanced_glass_saved_fixture(method: ImageMethod) -> Harness {
    use solidcraft_engine::render::glass_model::{Dispersion, GlassModel, ThinFilm};
    let mut h = stable_glass_draft_fixture(method, true);
    let target = materials::read_targets(&mut h.app).unwrap().into_iter().find(|target| target.name == "Glass A").unwrap();
    let mut optics = h.app.session.doc.render.setups["Studio"].optics.iter().next().unwrap().optics;
    optics.model = Some(GlassModel {
        roughness: 0.21,
        dispersion: Some(Dispersion { abbe_number: 57.0 }),
        thin_film: Some(ThinFilm { ior: 1.37, thickness_nm: 263.0 }),
    });
    // Authored inactive scattering is independent of the interface model and
    // must survive an edit to just one optical field.
    optics.subsurface = Some(solidcraft_engine::render::transmission::subsurface::Subsurface { scattering_per_mm: 0.0, anisotropy: 0.42 });
    h.app
        .run(
            "render.optics.bind",
            json!({"name":"Studio","body_index":target.ordinal,"body_name":target.name,
        "expected_revision":h.app.session.revision,"replace":true,"optics":optics}),
        )
        .unwrap();
    setups::load(&mut h.app);
    h.app.render_workspace.view.live_paused = true;
    h
}

#[test]
fn advanced_glass_lens_save_preserves_caustic_job_drafts_and_refuses_unsaved_optics_atomically() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    use solidcraft_engine::render::job::{CausticOptions, CpuProgressiveOptions, ThinLens};
    for method in [ImageMethod::CpuRaytrace, ImageMethod::GpuRaytrace] {
        let mut h = advanced_glass_saved_fixture(method);
        let saved = h.app.session.doc.render.setups["Studio"].clone();
        let quality = CausticOptions { photons: 8192, radius_mm: 0.125, max_depth: 11, seed: 901 };
        h.app.render_workspace.view.studio.caustics = Some(quality);
        if method == ImageMethod::CpuRaytrace {
            h.app.render_workspace.view.raytrace.progressive = Some(CpuProgressiveOptions { samples_per_pixel: 7, seed: 93 });
        } else {
            h.app.render_workspace.view.samples = 7;
            h.app.render_workspace.view.seed = 93;
        }
        h.app.cam.yaw += 0.125;
        setups::refresh_dirty(&mut h.app);
        let live_camera = h.app.cam;
        let before = h.app.session.doc.clone();
        let model = h.app.session.model.state();
        let undo = h.app.session.undo.len();
        let mut expected_saved = saved.clone();
        let mut expected_draft = setups::draft(&h.app).unwrap();
        assert_ne!(expected_draft.job, saved.job, "the fixture must carry independent caustic quality and sampling drafts");
        let lens = ThinLens { aperture_radius_mm: 0.05, focus_distance_mm: 80.0 };
        for setup in [&mut expected_saved, &mut expected_draft] {
            match setup.job.as_mut().unwrap() {
                RenderJob::CpuRaytrace { options, .. } => options.lens = Some(lens),
                RenderJob::GpuPath { options, .. } => options.lens = Some(lens),
                RenderJob::Raster { .. } => panic!("the fixture requires an image job"),
            }
        }
        shot::set_lens(&mut h.app, "Studio", Some(lens)).unwrap();
        assert_eq!(h.app.session.doc.render.setups["Studio"], expected_saved, "lens Save must preserve every saved advanced optical and job value");
        assert_eq!(setups::draft(&h.app).unwrap(), expected_draft, "independent caustic quality and sampling must remain in the unsaved draft");
        assert_eq!(image_request(&h.app).unwrap()["settings"], json!(expected_draft));
        assert!(h.app.render_workspace.view.dirty);
        assert_eq!(h.app.cam, live_camera);
        assert_eq!(h.app.session.doc.render.setups["Studio"].optics, saved.optics);
        assert!(Arc::ptr_eq(&model, &h.app.session.model.state()));
        assert_eq!(h.app.session.undo.len(), undo + 1);
        assert!(h.app.render_workspace.view.job.is_none());
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc, before);
        assert_eq!(h.app.cam, live_camera);
        h.app.run("edit.redo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.render.setups["Studio"], expected_saved);
        assert_eq!(h.app.cam, live_camera);

        let mut h = advanced_glass_saved_fixture(method);
        let source = h.app.session.doc.render.setups["Studio"].optics.iter().next().unwrap().optics;
        let target = materials::read_targets(&mut h.app).unwrap().into_iter().find(|target| target.name == "Glass A").unwrap();
        let mut binding = materials::Binding::from_saved(target, &h.app, source);
        binding.glass.model.as_mut().unwrap().roughness = 0.43;
        h.app.render_workspace.view.materials.enabled = true;
        h.app.render_workspace.view.materials.bindings.push(binding);
        h.app.render_workspace.view.studio.caustics = Some(quality);
        let before = h.app.session.doc.clone();
        let model = h.app.session.model.state();
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        let camera = h.app.cam;
        let request = image_request(&h.app).unwrap();
        let error = shot::set_lens(&mut h.app, "Studio", Some(lens)).unwrap_err();
        assert!(error.contains(h.app.tr("render.setup_runtime_unsupported")), "an unreviewed optical override must refuse a lens Save");
        assert!(Arc::ptr_eq(&before, &h.app.session.doc));
        assert!(Arc::ptr_eq(&model, &h.app.session.model.state()));
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
        assert_eq!(h.app.cam, camera);
        assert_eq!(image_request(&h.app).unwrap(), request, "a refused lens Save must preserve the complete optical and caustic draft");
        assert!(h.app.render_workspace.view.materials.bindings[0].has_unsaved_optics_intent());
    }
}

#[test]
fn advanced_glass_pointer_edit_keeps_saved_dispersion_film_and_independent_optics_through_native_reopen() {
    use solidcraft_engine::doc::render_optics::SavedOptics;
    for method in [ImageMethod::CpuRaytrace, ImageMethod::GpuRaytrace] {
        let mut h = advanced_glass_saved_fixture(method);
        h.app.run("select.set", json!({"items":[solidcraft_engine::Sel::Body{name:"Glass A".into()}]})).unwrap();
        let selected = object_selection(&h.app);
        selection_editor(&mut h.app, &selected);
        h.app.run("view.render_stage", json!({"stage":"look"})).unwrap();
        h.frames(3);
        let document = h.app.session.doc.clone();
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        let saved = document.render.setups["Studio"].clone();
        let original = saved.optics.iter().next().unwrap().clone();
        assert!(h.app.render_workspace.view.materials.bindings.is_empty(), "opening a saved row must not create an unsaved material override");
        assert_eq!(image_request(&h.app).unwrap()["settings"], json!(saved), "opening the advanced editor must preserve the complete saved setup");
        stable_glass_draft_click(&mut h, &format!("dialog:section:{}", crate::i18n::tr("render.appearance_editor.glass")));
        stable_glass_draft_reveal(&mut h, "render:glass:roughness");
        let field = crate::scenario::rect_of("render:glass:roughness").unwrap();
        assert!(stable_glass_draft_viewport(&h).contains_rect(field));
        h.step(&json!({"click":[field.center().x,field.center().y],"double":true})).unwrap();
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        h.step(&json!({"text":"0.43"})).unwrap();
        h.step(&json!({"key":"Enter"})).unwrap();
        h.frames(2);
        let mut changed = original.optics;
        changed.model.as_mut().unwrap().roughness = 0.43;
        let binding = h.app.render_workspace.view.materials.bindings.iter().find(|binding| binding.target.name == "Glass A").unwrap();
        assert_eq!(binding.glass.model, changed.model, "a roughness edit must retain the hydrated dispersion and film values");
        assert!(binding.has_unsaved_optics_intent());
        let mut expected = saved.clone();
        expected.optics.set(SavedOptics { target: original.target.clone(), optics: changed }, true).unwrap();
        let request = image_request(&h.app).unwrap();
        assert_eq!(request["settings"], json!(expected), "changing roughness must preserve saved camera/lens, maps, coat, absorption and scattering");
        assert!(request.pointer("/raytrace/dielectrics").is_none(), "a stable advanced edit must keep its stable target lane");
        setups::refresh_dirty(&mut h.app);
        assert!(h.app.render_workspace.view.dirty);
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
        stable_glass_draft_click(&mut h, &format!("dialog:section:{}", crate::i18n::tr("render.appearance_editor.saved")));
        h.step(&json!({"widget":h.app.tr("render.appearance_editor.replace")})).unwrap();
        stable_glass_draft_click(&mut h, "render:optics-save");
        assert_eq!(
            h.app.session.doc.render.setups["Studio"], expected,
            "the real saved-optics button must commit the prospective model exactly once"
        );
        assert_eq!(h.app.session.undo.len(), history.1 + 1);
        assert_eq!(h.app.session.doc.features, document.features);
        assert_eq!(h.app.session.doc.materials, document.materials);
        assert_eq!(h.app.session.doc.appearances, document.appearances);
        let path = h.dir.join(format!("advanced-glass-{method:?}.solidcraft"));
        h.app.run("file.save", json!({"path":path})).unwrap();
        h.app.run("doc.open", json!({"path":path})).unwrap();
        assert_eq!(h.app.session.doc.render.setups["Studio"], expected, "actual native save/open retains the advanced stable assignment");
        h.app.render_workspace.view.name = "Studio".into();
        enter(&mut h.app, true).unwrap();
        setups::load(&mut h.app);
        h.app.render_workspace.view.live_paused = true;
        assert_eq!(image_request(&h.app).unwrap()["settings"], json!(expected));
        assert!(h.app.render_workspace.view.materials.bindings.is_empty(), "native hydration must not create an unsaved material override");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn advanced_glass_live_request_host(h: &mut Harness, method: ImageMethod) -> bool {
    use solidcraft_engine::render::{RenderFeature, job};
    if method == ImageMethod::CpuRaytrace {
        let host = job::CpuHost::for_platform().unwrap();
        h.app.session.set_render_job_host(Some(host.clone())).unwrap();
        h.app.render_workspace.image_host = Some(host);
        return image_host(&h.app, method).is_ok();
    }
    struct RequestOnly;
    impl job::ImageExecutor for RequestOnly {
        fn supports_glass_models(&self) -> bool {
            true
        }
        fn supports_caustics(&self) -> bool {
            true
        }
        fn descriptor(&self) -> job::Result<ExecutorDescriptor> {
            let mut descriptor = job::CpuHost::cooperative().descriptor()?;
            descriptor.id = ExecutorId::new("ui-request-only").unwrap();
            descriptor.profiles[0].features.push(RenderFeature::Progressive);
            Ok(descriptor)
        }
        fn admit(&self, _: &ProductScene, _: &job::ImageRequest, _: job::JobOptions) -> job::Result<job::Admission> {
            Err(job::JobError::new(job::ErrorCode::HostUnavailable, "paused UI request test does not render"))
        }
        fn begin(&self, _: ProductScene, _: job::ImageRequest, _: job::JobOptions, _: job::CancelToken) -> job::Result<Box<dyn job::ImageWork>> {
            Err(job::JobError::new(job::ErrorCode::HostUnavailable, "paused UI request test does not render"))
        }
    }
    h.app.render_workspace.path_host = PathHost::Ready(job::ImageHost::with_executor(Arc::new(RequestOnly)).unwrap());
    true
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn caustic_quality_survives_cpu_gpu_ui_save_load_and_changes_the_paused_live_request_identity() {
    use solidcraft_engine::render::job::CausticOptions;
    for method in [ImageMethod::CpuRaytrace, ImageMethod::GpuRaytrace] {
        let mut h = advanced_glass_saved_fixture(method);
        let can_observe_live = advanced_glass_live_request_host(&mut h, method);
        h.app.preferences.render_legacy_image_defaults = false;
        h.app.viewport.rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(256.0, 192.0)));
        h.app.render_workspace.view.live_paused = true;
        let saved = h.app.session.doc.render.setups["Studio"].clone();
        let document = h.app.session.doc.clone();
        let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
        let quality = CausticOptions { photons: 8192, radius_mm: 0.125, max_depth: 11, seed: 901 };
        h.app.render_workspace.view.studio.caustics = Some(quality);
        setups::refresh_dirty(&mut h.app);
        assert!(h.app.render_workspace.view.dirty, "the clean-shot fingerprint includes explicit caustic quality");
        let request = image_request(&h.app).unwrap();
        assert_eq!(request["settings"]["job"]["options"]["caustics"], json!(quality));
        assert_eq!(request["settings"]["optics"], json!(saved.optics));
        let mut runtime = json!({});
        if method == ImageMethod::CpuRaytrace {
            cpu::params(&h.app, &mut runtime).unwrap();
            assert_eq!(runtime["raytrace"]["caustics"], json!(quality), "CPU temporary request capture retains all four quality controls");
        } else {
            studio::params(&h.app, &mut runtime).unwrap();
            assert_eq!(runtime["caustics"], json!(quality), "GPU temporary request capture retains all four quality controls");
        }
        if can_observe_live {
            live::advance(&mut h.app, &h.ctx, 100.0);
            let first = live::inspect(&h.app);
            assert_eq!(first["request"]["settings"]["job"]["options"]["caustics"], json!(quality));
            let mut changed = quality;
            changed.radius_mm = 0.25;
            h.app.render_workspace.view.studio.caustics = Some(changed);
            live::advance(&mut h.app, &h.ctx, 101.0);
            let next = live::inspect(&h.app);
            assert!(
                next["generation"].as_u64().unwrap() > first["generation"].as_u64().unwrap(),
                "a radius edit invalidates the previous live request identity"
            );
            assert_eq!(next["request"]["settings"]["job"]["options"]["caustics"], json!(changed));
            assert_eq!(next["request"]["settings"]["optics"], json!(saved.optics));
            assert_eq!(next["pending"], false);
            assert_eq!(next["running"], false, "inspecting paused live intent must not submit a render");
            h.app.render_workspace.view.studio.caustics = Some(quality);
        }
        assert!(Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), history);
        setups::save(&mut h.app).unwrap();
        let expected = h.app.session.doc.render.setups["Studio"].clone();
        assert_eq!(image_request(&h.app).unwrap()["settings"], json!(expected));
        h.app.render_workspace.view.studio.caustics = None;
        setups::refresh_dirty(&mut h.app);
        assert!(h.app.render_workspace.view.dirty);
        setups::load(&mut h.app);
        assert_eq!(h.app.render_workspace.view.studio.caustics, Some(quality));
        assert!(!h.app.render_workspace.view.dirty);
        if method == ImageMethod::CpuRaytrace {
            assert_eq!(h.app.render_workspace.view.raytrace.caustics, Some(quality));
        }
        let path = h.dir.join(format!("caustic-quality-{method:?}.solidcraft"));
        h.app.run("file.save", json!({"path":path})).unwrap();
        h.app.run("doc.open", json!({"path":path})).unwrap();
        assert_eq!(h.app.session.doc.render.setups["Studio"], expected);
        h.app.render_workspace.view.name = "Studio".into();
        enter(&mut h.app, true).unwrap();
        setups::load(&mut h.app);
        h.app.render_workspace.view.live_paused = true;
        assert_eq!(h.app.render_workspace.view.studio.caustics, Some(quality));
        assert_eq!(image_request(&h.app).unwrap()["settings"], json!(expected));
        assert!(h.app.render_workspace.view.job.is_none());
    }
}
