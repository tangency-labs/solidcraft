use super::*;
use crate::scenario::Harness;
use egui::Color32;
use solidcraft_engine::Session;

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
            "render.environment.needs_gpu",
            "render.environment.gradient_gpu",
            "render.environment.blur_pending",
            "render.environment.world_pending",
            "render.environment.medium",
            "render.environment.min",
            "render.environment.max",
        ] {
            assert!(text.contains(h.app.tr(key)), "missing {key}: {text}");
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
    let expected = setups::snapshot(&h.app).unwrap();
    assert_eq!(h.app.session.doc.render.setups[&h.app.render_workspace.view.name], expected);
    h.app.run("edit.undo", json!({})).unwrap();
    setups::load(&mut h.app);
    assert!(h.app.render_workspace.view.studio.volume.is_none());
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
    h.app.render_workspace.view.inspector.page = inspector::Page::Environment;
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
    h.app.render_workspace.view.inspector.page = inspector::Page::Lighting;
    h.app.render_workspace.view.selection = RenderSelection::Environment;
    environment_ring_frame(&mut h, egui::Modifiers::NONE, vec![]);
    assert!(h.app.render_workspace.view.environment_ring.geometry.is_some());
    h.app.render_workspace.view.selection = RenderSelection::Nothing;
    environment_ring_frame(&mut h, egui::Modifiers::NONE, vec![]);
    assert!(h.app.render_workspace.view.environment_ring.geometry.is_none());
    h.app.render_workspace.view.inspector.page = inspector::Page::Environment;
    h.app.render_workspace.view.result = true;
    environment_ring_frame(&mut h, egui::Modifiers::NONE, vec![]);
    assert!(h.app.render_workspace.view.environment_ring.geometry.is_none());
}

#[test]
fn environment_editor_opening_pages_and_sources_preserves_design_and_history() {
    let mut h = environment_editor_fixture();
    let before = h.app.session.doc.clone();
    let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    for page in [inspector::Page::Environment, inspector::Page::Background] {
        h.app.render_workspace.view.inspector.page = page;
        environment_editor_frame(&mut h, vec![]);
        assert!(crate::scenario::rect_of("render:environment:rotation-slider").is_some());
        h.app.ui.show_browser = false;
        environment::open_sources(&mut h.app);
        assert!(h.app.ui.show_browser);
        assert_eq!(h.app.render_workspace.view.stage, Stage::Light);
        assert_eq!(h.app.render_workspace.view.inspector.page, page);
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
    assert!(!shot::overlay_visible(&h.app));
    h.app.render_workspace.view.inspector.page = inspector::Page::Camera;
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
fn render_lights_editor_opening_and_disabled_gpu_types_never_edit_or_start_a_job() {
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
    h.app.render_workspace.view.inspector.page = inspector::Page::Lighting;
    let document = h.app.session.doc.clone();
    let before = (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision, h.app.session.is_dirty());
    h.frames(3);
    assert!(crate::scenario::rect_of("render:light:editor").is_some());
    h.step(&json!({"click":{"handle":"render:light:type:point"}})).unwrap();
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(before, (h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision, h.app.session.is_dirty()));
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
    let page = h.app.render_workspace.view.inspector.page;
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
    assert_eq!(h.app.render_workspace.view.inspector.page, page);
    assert_eq!(setups::snapshot(&h.app).unwrap(), snapshot);
    assert!(h.app.render_workspace.view.job.is_none());
    assert!(h.app.dialog.is_none() && h.app.tool.is_none());
}

/// The vertices of a product scene's `class` triangles: their runs in its blocks.
fn class_vertices(gpu: &GpuScene, class: crate::gpu::TriClass) -> Vec<u8> {
    let mut out = Vec::new();
    for b in &gpu.blocks {
        for r in b.bytes.runs(class).iter() {
            out.extend_from_slice(
                b.bytes.vertices.get(r.start as usize * crate::gpu::TRI_SIZE..r.end as usize * crate::gpu::TRI_SIZE).unwrap_or_default(),
            );
        }
    }
    out
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
        h.app.render_workspace.view.inspector.page = inspector::Page::All;
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
        h.app.render_workspace.view.inspector.page = inspector::Page::All;
        h.frames(3);
        assert!(h.widget("GPU path tracer: compiling shaders…").is_some());
        enter(&mut h.app, false).unwrap();
        release.send(()).unwrap();
        finish_setup(&mut h);
        assert!(matches!(h.app.render_workspace.path_host, PathHost::Ready(_)));
        enter(&mut h.app, true).unwrap();
        h.app.render_workspace.view.inspector.page = inspector::Page::All;
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
        h.app.render_workspace.view.inspector.page = inspector::Page::All;
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
        assert!(!class_vertices(&gpu, crate::gpu::TriClass::Pbr).is_empty() && class_vertices(&gpu, crate::gpu::TriClass::Opaque).is_empty());
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
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
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
    for language in ["en", "de", "ko", "ru"] {
        let mut h = Harness::new();
        h.app.preferences.language = language.into();
        h.frames(1);
        let check = h.app.tr("cmd.render_preflight").to_string();
        let start = h.app.tr("render.image_start").to_string();
        let export = h.app.tr("render.image_export").to_string();
        h.call("ui.resize", json!({"width":1600,"height":1400}));
        enter(&mut h.app, true).unwrap();
        h.app.render_workspace.view.inspector.page = inspector::Page::All;
        h.app.render_workspace.view.draft.width = 16;
        h.app.render_workspace.view.draft.height = 16;
        h.app.render_workspace.view.dirty = true;
        h.frames(3);
        let doc = h.app.session.doc.clone();
        let undo = h.app.session.undo.len();
        h.step(&json!({"widget":check})).unwrap();
        h.step(&json!({"widget":start})).unwrap();
        for _ in 0..50 {
            h.frames(1);
            if h.app.render_workspace.view.completed.is_some() {
                break;
            }
        }
        let good = h.app.render_workspace.view.completed.unwrap();
        h.step(&json!({"widget":export})).unwrap();
        assert!(h.dir.join("render.png").is_file());
        h.app.render_workspace.view.draft.width = 1024;
        h.app.render_workspace.view.draft.height = 768;
        h.step(&json!({"widget":start})).unwrap();
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
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    h.frames(3);
    h.step(&json!({"widget":"Restore Saved Camera"})).unwrap();
    h.frames(3);
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.render_workspace.view.draft.background, [21, 43, 65]);
    h.app.session.selection = vec![solidcraft_engine::Sel::Body { name: "Product".into() }];
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
fn progressive_image_controls_route_to_the_registered_host() {
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
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
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
    h.frames(3);
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":"render:method"}})).unwrap();
    h.step(&json!({"click":{"handle":"render:method-gpu"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::GpuRaytrace);
    let checked = image_params(&h.app).unwrap();
    assert_eq!(checked["raytrace"]["clear_coats"][0]["weight"], 1.0);
    assert_eq!(checked["raytrace"]["textures"][0]["base_color_srgb"], map_path.to_string_lossy().as_ref());
    h.app.run("render.preflight", checked.clone()).unwrap();
    assert_eq!(image_params(&h.app).unwrap(), checked);
    let start = h.app.tr("render.image_start").to_string();
    h.step(&json!({"widget":start})).unwrap();
    let job = h.app.render_workspace.view.job.unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while h.app.render_workspace.view.completed.is_none() {
        assert!(std::time::Instant::now() < deadline, "progressive UI job did not complete");
        std::thread::sleep(std::time::Duration::from_millis(5));
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
    let export = h.app.tr("render.image_export").to_string();
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
    let status = h.app.session.render_job_handle(job).unwrap().wait_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(status.progress.phase, solidcraft_engine::render::job::Phase::Completed, "{:?}", status.error);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(!h.app.session.is_dirty());
}

#[test]
fn clicked_render_workspace_saves_one_setup_and_restores_design_view() {
    for language in ["en", "de", "ko", "ru"] {
        let mut h = Harness::new();
        h.app.preferences.language = language.into();
        h.app.preferences.perspective_fov = 50f64.to_radians();
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
        let heading = h.app.tr("render.page_scene").to_string();
        let perspective = h.app.tr("render.perspective").to_string();
        let save = h.app.tr("render.save_setup").to_string();
        let back = h.app.tr("render.return_design").to_string();
        assert!(h.widget(&heading).is_some(), "{language}");
        h.step(&json!({"click":{"handle":"render:scene:frame"}})).unwrap();
        assert_eq!(h.app.render_workspace.view.inspector.page, inspector::Page::Camera);
        h.step(&json!({"widget":perspective})).unwrap();
        assert_eq!(h.app.cam.fov, 50f64.to_radians(), "{language}: Perspective takes the Field of view preference's angle (#5364)");
        assert!(!h.app.session.is_dirty(), "preview edits are staged");
        h.step(&json!({"widget":save})).unwrap();
        assert_eq!(h.app.session.undo.len(), undo + 1);
        assert_eq!(h.app.session.doc.render.setups["Studio"].camera, h.app.cam);
        h.step(&json!({"widget":back})).unwrap();
        assert!(!active(&h.app));
        assert_eq!(h.app.cam, camera);
        assert_eq!(flags, (h.app.ui.show_grid, h.app.origin_shown(), h.app.ui.visual_style, h.app.ui.perspective));
        h.app.run("edit.undo", json!({})).unwrap();
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
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
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
    assert!(class_vertices(&gpu, crate::gpu::TriClass::Opaque).is_empty());
    assert!(!class_vertices(&gpu, crate::gpu::TriClass::Glass).is_empty());
    let color = |v: &[u8]| {
        let i = v.get(crate::gpu::TRI_SIZE - 4..).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes)?;
        gpu.palette.get(crate::gpu::PaletteIndex(i))
    };
    assert!(
        class_vertices(&gpu, crate::gpu::TriClass::Glass)
            .as_chunks::<{ crate::gpu::TRI_SIZE }>()
            .0
            .iter()
            .all(|v| color(v) == Some([51, 102, 153, 102]))
    );
    assert!(gpu.lines.is_empty() && gpu.overlay.is_empty() && gpu.under.is_empty() && gpu.hidden.is_empty() && gpu.ghost.is_empty());
    s.selection = vec![solidcraft_engine::Sel::Body { name: "Glass".into() }];
    s.section = Some((solidcraft_engine::geom::Vec3::ZERO, solidcraft_engine::geom::Vec3::X));
    let after = solidcraft_engine::product_scene::extract(&s).unwrap();
    assert_eq!(gpu_scene(&after).unwrap().0, key);
    s.execute("appearance.assign", &json!({"bodies":["Glass"],"color":"#336699","opacity":0.0})).unwrap();
    let invisible = solidcraft_engine::product_scene::extract(&s).unwrap();
    assert_eq!(invisible.meshes[0].opacity, 0.0);
    let (_, empty) = gpu_scene(&invisible).unwrap();
    assert!(class_vertices(&empty, crate::gpu::TriClass::Opaque).is_empty() && class_vertices(&empty, crate::gpu::TriClass::Glass).is_empty());
}

/// The live preview keeps its blocks across scenes: a colour edit rewrites palette entries and
/// keeps every vertex, an opacity class change keeps the vertices and moves the triangles to the
/// see-through runs, and a geometry edit builds the block again.
#[test]
fn product_appearance_edits_keep_the_blocks_vertices() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10,"body_name":"Part"})).unwrap();
    let mut cache = ProductCache::default();
    let mut build =
        |s: &Session| gpu_scene_cached(&solidcraft_engine::product_scene::extract(s).unwrap(), &[], MAX_GEOMETRY_BYTES as u64, &mut cache).unwrap();
    let (plain_key, plain) = build(&s);
    let vertices = plain.blocks[0].bytes.vertices.clone();
    let first_colour = |gpu: &GpuScene, class| {
        let v = class_vertices(gpu, class);
        let i = v.get(crate::gpu::TRI_SIZE - 4..crate::gpu::TRI_SIZE).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes)?;
        gpu.palette.get(crate::gpu::PaletteIndex(i))
    };
    s.execute("appearance.assign", &json!({"bodies":["Part"],"color":"#336699"})).unwrap();
    let (colour_key, colour) = build(&s);
    assert_ne!(colour_key, plain_key, "the scene is taken again");
    assert!(Arc::ptr_eq(&vertices, &colour.blocks[0].bytes.vertices), "a colour edit keeps every vertex");
    assert_eq!(first_colour(&colour, crate::gpu::TriClass::Opaque), Some([51, 102, 153, 255]), "the palette carries the colour");
    s.execute("appearance.assign", &json!({"bodies":["Part"],"color":"#336699","opacity":0.5})).unwrap();
    let (_, glass) = build(&s);
    assert!(Arc::ptr_eq(&vertices, &glass.blocks[0].bytes.vertices), "an opacity class change keeps every vertex");
    assert!(class_vertices(&glass, crate::gpu::TriClass::Opaque).is_empty(), "no triangle is left opaque");
    assert_eq!(first_colour(&glass, crate::gpu::TriClass::Glass), Some([51, 102, 153, 128]));
    s.execute("solid.box", &json!({"length":5,"width":5,"height":5,"corner":[3,3,10],"operation":"cut"})).unwrap();
    let (_, cut) = build(&s);
    assert!(!Arc::ptr_eq(&vertices, &cut.blocks[0].bytes.vertices), "a geometry edit builds the block again");
}

/// A PBR material edit (metallic, roughness, IOR) rewrites the palette's material table and
/// keeps every vertex; the PBR triangles are the block's PBR runs, 28 bytes a vertex.
#[test]
fn product_material_edits_keep_the_blocks_vertices() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10,"body_name":"Part"})).unwrap();
    let look = |roughness: f64| json!({"bodies":["Part"],"color":"#b47846","pbr":{"metallic":1.0,"roughness":roughness,"dielectric_ior":1.5}});
    s.execute("appearance.assign", &look(0.2)).unwrap();
    let mut cache = ProductCache::default();
    let mut build =
        |s: &Session| gpu_scene_cached(&solidcraft_engine::product_scene::extract(s).unwrap(), &[], MAX_GEOMETRY_BYTES as u64, &mut cache).unwrap();
    let (rough_key, rough) = build(&s);
    let vertices = rough.blocks[0].bytes.vertices.clone();
    assert_eq!(class_vertices(&rough, crate::gpu::TriClass::Pbr).len(), 12 * 3 * crate::gpu::TRI_SIZE, "every triangle is PBR");
    assert!(rough.pbr.is_empty(), "no separate PBR stream");
    let material = |gpu: &GpuScene| gpu.palette.materials().iter().copied().find(|m| m.pbr);
    assert_eq!(material(&rough).map(|m| (m.metallic, m.roughness)), Some((1.0, 0.2)));
    s.execute("appearance.assign", &look(0.6)).unwrap();
    let (smooth_key, smooth) = build(&s);
    assert_ne!(smooth_key, rough_key, "the scene is taken again");
    assert!(Arc::ptr_eq(&vertices, &smooth.blocks[0].bytes.vertices), "a material edit keeps every vertex");
    assert_eq!(material(&smooth).map(|m| m.roughness), Some(0.6));
}

/// A part placed several times is drawn as instances of its one shared mesh: its triangles are
/// in no block, every placement has its own palette slot, a colour edit keeps the shared
/// triangles, and the picture is the one drawn with every copy built (pixel for pixel, but for
/// a few on edges). Skipped without a GPU adapter for the picture.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn placed_copies_are_drawn_as_instances_of_their_shared_mesh() {
    use solidcraft_engine::render::StandardView;
    let mut s = Session::default();
    let part = s.execute("component.create", &json!({"name":"Part","grounded":false})).unwrap();
    s.execute("solid.cylinder", &json!({"base":[0,0,0],"diameter":10,"height":12,"body_name":"Pin"})).unwrap();
    s.execute("component.activate", &json!({"component":0})).unwrap();
    for x in [20, 40] {
        s.execute("occurrence.copy", &json!({"component":part["component"],"translate":[x,0,0]})).unwrap();
    }
    s.execute("solid.box", &json!({"length":60,"width":20,"height":4,"corner":[-10,-10,-4],"body_name":"Base"})).unwrap();
    let scene = solidcraft_engine::product_scene::extract(&s).unwrap();
    let placements = solidcraft_engine::product_scene::placements(&s, &scene);
    let mut cache = ProductCache::default();
    let (_, instanced) = gpu_scene_cached(&scene, &placements, MAX_GEOMETRY_BYTES as u64, &mut cache).unwrap();
    assert_eq!(instanced.instanced.len(), 1, "one shared mesh");
    assert_eq!(instanced.instanced[0].count(), 3, "placed three times");
    let base_triangles = scene.meshes.iter().find(|m| m.name.starts_with("Base")).unwrap().mesh.triangles.len();
    assert_eq!(
        class_vertices(&instanced, crate::gpu::TriClass::Opaque).len(),
        base_triangles * 3 * crate::gpu::TRI_SIZE,
        "only the base is in a block"
    );
    let shared = instanced.instanced[0].tris.clone();
    s.execute("appearance.assign", &json!({"bodies":[scene.meshes.iter().find(|m| m.name.starts_with("Pin")).unwrap().name], "color":"#336699"}))
        .unwrap();
    let painted = solidcraft_engine::product_scene::extract(&s).unwrap();
    let (_, recoloured) =
        gpu_scene_cached(&painted, &solidcraft_engine::product_scene::placements(&s, &painted), MAX_GEOMETRY_BYTES as u64, &mut cache).unwrap();
    assert!(Arc::ptr_eq(&shared, &recoloured.instanced[0].tris), "a colour edit keeps the shared triangles");
    let (_, built) = gpu_scene(&scene).unwrap();
    assert!(built.instanced.is_empty(), "without placements every copy is built");
    const SIZE: u32 = 192;
    let mut camera = Camera { fov: 0.0, ..Default::default() };
    camera.set_view(StandardView::Iso);
    camera.fit(&scene.bounds);
    let radius = scene.bounds.diagonal().max(10.0) + scene.bounds.center().dist(camera.target);
    let eye = camera.eye().to_f32();
    let render = |gpu: GpuScene| {
        crate::gpu::render_product_offscreen(
            gpu,
            None,
            camera.view_proj(1.0, radius).to_f32(),
            camera.back().to_f32(),
            [eye[0], eye[1], eye[2], 0.0],
            SIZE,
            false,
        )
    };
    let (Some(a), Some(b)) = (render(instanced), render(built)) else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let differ = a.chunks(4).zip(b.chunks(4)).filter(|(x, y)| x.iter().zip(*y).any(|(p, q)| p.abs_diff(*q) > 6)).count();
    let shown = b.chunks(4).filter(|p| p[..3] != b[..3]).count();
    assert!(shown > 1000, "the parts are in view ({shown} px)");
    assert!(differ * 200 < shown, "{differ} of {shown} px differ between the instanced and built pictures");
}

/// A part with a PBR material placed several times is drawn as PBR instances of its shared
/// mesh, each placement's material read through its palette slot, and the picture is the one
/// drawn with every copy built. Skipped without a GPU adapter for the picture.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pbr_placements_are_drawn_as_instances_with_their_material() {
    use solidcraft_engine::render::StandardView;
    let mut s = Session::default();
    let part = s.execute("component.create", &json!({"name":"Part","grounded":false})).unwrap();
    s.execute("solid.sphere", &json!({"radius":6,"body_name":"Ball"})).unwrap();
    s.execute("component.activate", &json!({"component":0})).unwrap();
    for x in [16, 32] {
        s.execute("occurrence.copy", &json!({"component":part["component"],"translate":[x,0,0]})).unwrap();
    }
    let balls: Vec<String> = s.world_state().bodies.iter().map(|b| b.name.clone()).collect();
    s.execute("appearance.assign", &json!({"bodies":balls,"color":"#b47846","pbr":{"metallic":1.0,"roughness":0.3,"dielectric_ior":1.5}})).unwrap();
    let scene = solidcraft_engine::product_scene::extract(&s).unwrap();
    let placements = solidcraft_engine::product_scene::placements(&s, &scene);
    let (_, instanced) = gpu_scene_cached(&scene, &placements, MAX_GEOMETRY_BYTES as u64, &mut ProductCache::default()).unwrap();
    assert_eq!(instanced.instanced.len(), 1);
    assert_eq!((instanced.instanced[0].look, instanced.instanced[0].count()), (crate::gpu::InstanceLook::Pbr, 3));
    assert!(instanced.blocks.iter().all(|b| b.bytes.vertices.is_empty()), "no ball is built");
    let (_, built) = gpu_scene(&scene).unwrap();
    const SIZE: u32 = 192;
    let mut camera = Camera { fov: 0.0, ..Default::default() };
    camera.set_view(StandardView::Iso);
    camera.fit(&scene.bounds);
    let radius = scene.bounds.diagonal().max(10.0) + scene.bounds.center().dist(camera.target);
    let eye = camera.eye().to_f32();
    let render = |gpu: GpuScene| {
        crate::gpu::render_product_offscreen(
            gpu,
            None,
            camera.view_proj(1.0, radius).to_f32(),
            camera.back().to_f32(),
            [eye[0], eye[1], eye[2], 0.0],
            SIZE,
            false,
        )
    };
    let (Some(a), Some(b)) = (render(instanced), render(built)) else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let differ = a.chunks(4).zip(b.chunks(4)).filter(|(x, y)| x.iter().zip(*y).any(|(p, q)| p.abs_diff(*q) > 6)).count();
    let shown = b.chunks(4).filter(|p| p[..3] != b[..3]).count();
    assert!(shown > 1000, "the balls are in view ({shown} px)");
    assert!(differ * 100 < shown, "{differ} of {shown} px differ between the instanced and built PBR pictures");
}

#[test]
fn transmission_uses_legacy_preview_buffers_and_invalidates_the_opaque_cache() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10,"body_name":"Glass"})).unwrap();
    let appearance = json!({"bodies":["Glass"],"color":"#336699",
        "pbr":{"metallic":0,"roughness":0.1,"dielectric_ior":1.5}});
    s.execute("appearance.assign", &appearance).unwrap();
    let (opaque_key, opaque) = gpu_scene(&solidcraft_engine::product_scene::extract(&s).unwrap()).unwrap();
    assert!(!class_vertices(&opaque, crate::gpu::TriClass::Pbr).is_empty());
    let mut scene = solidcraft_engine::product_scene::extract(&s).unwrap();
    scene.meshes[0].transmission =
        Some(solidcraft_engine::render::transmission::Transmission::new(solidcraft_engine::geom::Vec3::ZERO, None).unwrap());
    let (glass_key, glass) = gpu_scene(&scene).unwrap();
    assert_ne!(opaque_key, glass_key);
    assert!(class_vertices(&glass, crate::gpu::TriClass::Pbr).is_empty());
    assert!(!class_vertices(&glass, crate::gpu::TriClass::Opaque).is_empty());
    scene.meshes[0].opacity = 0.4;
    let (_, transparent) = gpu_scene(&scene).unwrap();
    assert!(class_vertices(&transparent, crate::gpu::TriClass::Pbr).is_empty());
    assert!(!class_vertices(&transparent, crate::gpu::TriClass::Glass).is_empty());
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
    let mut info = PreviewInfo { frame_sequence: 1, samples_per_pixel: 1, frame: frame.info.clone() };
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
    let mut info = PreviewInfo { frame_sequence: 1, samples_per_pixel: 1, frame: frame.info.clone() };
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
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
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
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
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
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    h.frames(3);
    let camera = h.app.cam;
    let doc = h.app.session.doc.clone();
    let result = h.app.tr("render.result_title").to_string();
    h.step(&json!({"widget":result})).unwrap();
    h.frames(2);
    assert!(h.app.render_workspace.view.result);
    h.step(&json!({"widget":"100%"})).unwrap();
    h.frames(2);
    assert_eq!(h.app.render_workspace.view.result_scale, ResultScale::Actual);
    let fit = h.app.tr("render.result_fit").to_string();
    h.step(&json!({"widget":fit})).unwrap();
    h.frames(2);
    assert_eq!(h.app.render_workspace.view.result_scale, ResultScale::Fit);
    let preview = h.app.tr("render.result_preview").to_string();
    h.step(&json!({"widget":preview})).unwrap();
    assert!(!h.app.render_workspace.view.result);
    assert_eq!(h.app.cam, camera);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
}

#[test]
fn image_method_requests_never_fall_back_or_mix_cpu_and_gpu_options() {
    let mut h = Harness::new();
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
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
fn native_cpu_controls_check_and_start_the_same_capture_and_keep_it_exportable() {
    use solidcraft_engine::render::job::{CpuHost, Phase};
    let mut h = Harness::new();
    let host = CpuHost::for_platform().unwrap();
    h.app.session.set_render_job_host(Some(host.clone())).unwrap();
    h.app.render_workspace.image_host = Some(host);
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5})).unwrap();
    h.call("ui.resize", json!({"width":1600,"height":1800}));
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    h.app.render_workspace.view.draft.width = 16;
    h.app.render_workspace.view.draft.height = 16;
    h.app.render_workspace.view.dirty = true;
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
    h.step(&json!({"click":{"handle":"render:aa"}})).unwrap();
    h.step(&json!({"click":{"handle":"render:aa-4"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.raytrace.primary_samples, 4);
    h.app.render_workspace.view.raytrace.preset = Some(LightingPreset::WarmCool);
    h.app.render_workspace.view.draft.exposure_ev = 1.0;
    let numeric = image_params(&h.app).unwrap();
    assert_eq!(numeric["settings"]["job"]["method"], "cpu_raytrace");
    assert!(numeric.get("raytrace").is_none());
    let numeric_check = h.app.tr("cmd.render_preflight").to_string();
    h.step(&json!({"widget":numeric_check})).unwrap();
    assert_eq!(h.app.render_workspace.view.cpu.checked.as_ref().unwrap().request, numeric);
    let reflected = h.app.session.world_state().bodies.first().unwrap().name.clone();
    let reflection_section = format!("dialog:section:{}", h.app.tr("render.cpu_reflections"));
    h.step(&json!({"click":{"handle":reflection_section}})).unwrap();
    h.step(&json!({"click":{"handle":"render:cpu-reflection-body"}})).unwrap();
    h.step(&json!({"click":{"handle":format!("dialog:list:{reflected}")}})).unwrap();
    h.step(&json!({"click":{"handle":"render:cpu-reflection-add"}})).unwrap();
    h.step(&json!({"click":{"handle":format!("render:cpu-reflection-mode:{reflected}")}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:list:render.cpu_reflection_mirror"}})).unwrap();
    assert_eq!(
        h.app.render_workspace.view.raytrace.reflections.as_ref().unwrap().bodies[0].mode,
        solidcraft_engine::render::job::ReflectionMode::Mirror
    );
    h.step(&json!({"click":{"handle":reflection_section}})).unwrap();
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
    let check = h.app.tr("cmd.render_preflight").to_string();
    h.step(&json!({"widget":check})).unwrap();
    assert_eq!(image_params(&h.app).unwrap(), params);
    let checked = h.app.render_workspace.view.cpu.checked.as_ref().unwrap();
    assert_eq!(checked.request, params);
    assert_eq!(checked.environment, preflight["environment"]);
    let start = h.app.tr("render.image_start").to_string();
    h.step(&json!({"widget":start})).unwrap();
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
    assert!(image_params(&h.app).unwrap_err().contains(h.app.tr("render.cpu_opaque_required")));
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
    let ctx = crate::fonts::test_context();
    let slot: Slot = Default::default();
    save(&mut h.app, &slot);
    export_image(&mut h.app, job, false);
    crate::file_picker::tick(&mut h.app, &ctx);
    let newer = completed_preview(&mut h, [20, 30, 40]);
    h.app.render_workspace.view.completed = Some(newer);
    *slot.borrow_mut() = Some(Some(path.to_string_lossy().into_owned()));
    crate::file_picker::tick(&mut h.app, &ctx);
    assert!(!path.exists(), "the evicted requested image must fail instead of exporting the newer image");
    assert!(matches!(&h.app.status, Some((_, _, true))));
    assert_eq!(h.app.render_workspace.view.completed, Some(newer));
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
            let wheel = Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: vec2(0.0, 3.0), modifiers: none, phase: egui::TouchPhase::Move };
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
        h.app.render_workspace.view.inspector.page = inspector::Page::All;
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
            crate::prefs::NavigatorStyle::Cube => "navigation_cube:control:home",
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
        assert!(h.app.cam.back().dot(solidcraft_engine::geom::Vec3::Z) > 0.999, "{style:?}: view from +Z, got {:?}", h.app.cam.back());
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
    let target: materials::Target = serde_json::from_value(h.app.run("render.scene", json!({})).unwrap()["bodies"][0].clone()).unwrap();
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
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
        let medium = solidcraft_engine::render::transmission::Transmission::new(solidcraft_engine::geom::Vec3::new(r, g, b), None).unwrap();
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
fn gpu_material_identity_requires_explicit_revalidation_and_never_retargets() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":3,"body_name":"Optics"})).unwrap();
    let target: materials::Target = serde_json::from_value(h.app.run("render.scene", json!({})).unwrap()["bodies"][0].clone()).unwrap();
    let mut binding = materials::Binding::new(target, &h.app);
    binding.coat_enabled = true;
    h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
    h.app.render_workspace.view.materials.enabled = true;
    h.app.render_workspace.view.materials.bindings = vec![binding.clone()];
    materials::params(&h.app, &mut json!({})).unwrap();
    h.app.run("appearance.assign", json!({"bodies":["Optics"],"color":"#123456"})).unwrap();
    assert!(materials::params(&h.app, &mut json!({})).is_err());
    materials::revalidate(&mut h.app, &mut binding).unwrap();
    h.app.render_workspace.view.materials.bindings = vec![binding.clone()];
    materials::params(&h.app, &mut json!({})).unwrap();
    h.app.run("solid.box", json!({"length":3,"width":3,"height":3,"corner":[20,0,0],"body_name":"Other","operation":"new"})).unwrap();
    let scene = h.app.run("render.scene", json!({})).unwrap();
    let target: materials::Target =
        serde_json::from_value(scene["bodies"].as_array().unwrap().iter().find(|body| body["name"] == "Other").unwrap().clone()).unwrap();
    let mut moved = materials::Binding::new(target.clone(), &h.app);
    moved.coat_enabled = true;
    h.app.render_workspace.view.materials.bindings = vec![moved.clone()];
    h.app.run("browser.visibility", json!({"items":["Optics"],"visible":false})).unwrap();
    assert!(materials::params(&h.app, &mut json!({})).is_err());
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
fn clicked_gpu_glass_recipe_stays_temporary_and_blocks_cpu_rendering() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2200}));
    h.app.run("solid.box", json!({"length":10,"width":10,"height":3,"body_name":"Optics"})).unwrap();
    h.app.session.mark_saved();
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
    h.frames(3);
    let doc = h.app.session.doc.clone();
    let title = h.app.tr("render.material_title").to_string();
    h.step(&json!({"click":{"handle":format!("dialog:section:{title}")}})).unwrap();
    let refresh = h.app.tr("render.material_refresh").to_string();
    h.step(&json!({"widget":refresh})).unwrap();
    let choose = h.app.tr("render.material_choose_body").to_string();
    h.step(&json!({"widget":choose})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:list:render:material-body-0"}})).unwrap();
    let add = h.app.tr("render.material_add").to_string();
    h.step(&json!({"widget":add})).unwrap();
    let glass = h.app.tr("render.appearance_editor.glass").to_string();
    h.step(&json!({"click":{"handle":format!("dialog:section:{glass}")}})).unwrap();
    h.step(&json!({"widget":h.app.tr("render.material_custom")})).unwrap();
    h.step(&json!({"click":{"handle":"render:glass:starter.glass.amber-tint"}})).unwrap();
    let binding = &h.app.render_workspace.view.materials.bindings[0];
    assert!(binding.glass_enabled && !binding.coat_enabled && !binding.maps_enabled);
    assert_eq!(binding.glass.recipe, Some("starter.glass.amber-tint"));
    assert_eq!(binding.target.name, "Optics");
    h.app.render_workspace.view.method = ImageMethod::CpuRaytrace;
    assert!(image_params(&h.app).is_err(), "CPU must not silently ignore active GPU material assignments");
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
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    h.app.render_workspace.view.materials.bindings = targets.iter().skip(1).cloned().map(|target| materials::Binding::new(target, &h.app)).collect();
    h.frames(3);
    let title = h.app.tr("render.material_title").to_string();
    h.step(&json!({"click":{"handle":format!("dialog:section:{title}")}})).unwrap();
    let refresh = h.app.tr("render.material_refresh").to_string();
    h.step(&json!({"widget":refresh})).unwrap();
    let choose = h.app.tr("render.material_choose_body").to_string();
    h.step(&json!({"widget":choose})).unwrap();
    h.step(&json!({"click":{"handle":format!("dialog:list:render:material-body-{}", targets[1].ordinal)}})).unwrap();
    let add = h.app.tr("render.material_add").to_string();
    h.step(&json!({"widget":add})).unwrap();
    let include = h.app.tr("render.material_binding_enable").to_string();
    h.step(&json!({"widget":include})).unwrap();
    assert!(!h.app.render_workspace.view.materials.bindings[0].enabled, "the existing assignment remains editable at the limit");
    assert!(h.app.render_workspace.view.materials.bindings.iter().skip(1).all(|binding| binding.enabled));
    assert_eq!(h.app.render_workspace.view.materials.bindings.len(), 128);

    // The first scene body has no assignment: selecting it cannot create a 129th entry.
    h.step(&json!({"widget":refresh})).unwrap();
    h.step(&json!({"widget":choose})).unwrap();
    h.step(&json!({"click":{"handle":format!("dialog:list:render:material-body-{}", targets[0].ordinal)}})).unwrap();
    h.step(&json!({"widget":add})).unwrap();
    assert_eq!(h.app.render_workspace.view.materials.bindings.len(), 128);
    assert!(h.app.render_workspace.view.materials.bindings.iter().all(|binding| binding.target != targets[0]));
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(!h.app.session.is_dirty());
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
    h.step(&json!({"click":{"handle":"render:setup-name"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"GPU copy"})).unwrap();
    assert_eq!(h.app.render_workspace.view.name, "GPU copy");
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected, "typing Save As must not reset the settings");
    assert!(h.app.render_workspace.view.dirty);
    let undo = h.app.session.undo.len();
    h.step(&json!({"widget":h.app.tr("render.save_setup")})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.doc.render.setups["GPU copy"], expected);
    h.app.render_workspace.view.seed = 8;
    h.app.render_workspace.view.studio.ambient[1] = 0.125;
    setups::refresh_dirty(&mut h.app);
    assert!(h.app.render_workspace.view.dirty);
    h.step(&json!({"widget":h.app.tr("render.reload_settings")})).unwrap();
    if h.app.render_workspace.view.dirty {
        h.step(&json!({"widget":h.app.tr("render.discard_continue")})).unwrap();
    }
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    assert!(!h.app.render_workspace.view.dirty);
    select_image_method(&mut h.app, ImageMethod::Raster);
    h.app.render_workspace.view.name = "Raster saved".into();
    setups::save(&mut h.app).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:setups"}})).unwrap();
    h.step(&json!({"click":{"handle":"render:setup:GPU copy"}})).unwrap();
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
fn cpu_setup_reload_preserves_unedited_numeric_options_and_explicit_empty_values() {
    use solidcraft_engine::doc::render_settings::{CpuJob, RenderJob};
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2400}));
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
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
    let mut expected = RenderSetup { camera: h.app.cam, exposure_ev: 1.5, job: Some(RenderJob::CpuRaytrace { options }), ..Default::default() };
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
        expected.job = Some(RenderJob::CpuRaytrace { options });
        h.app.run("render.settings.set", json!({"name":"CPU numeric","set":expected})).unwrap();
        setups::load(&mut h.app);
        assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    }
}

#[test]
fn cpu_progressive_controls_resize_only_on_request_and_save_reload_without_a_host() {
    use solidcraft_engine::render::job::CpuProgressiveOptions;
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2600}));
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    h.app.session.set_render_job_host(None).unwrap();
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    h.app.render_workspace.view.raytrace.primary_samples = 16;
    h.app.render_workspace.view.draft.width = 1024;
    h.app.render_workspace.view.draft.height = 1024;
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
    let resize = format!("{} 512 × 512", h.app.tr("render.image_use_largest"));
    assert!(h.widget(&resize).is_some(), "the proposed dimensions must be visible before accepting them");
    h.step(&json!({"widget":resize})).unwrap();
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (512, 512));
    let expected = setups::snapshot(&h.app).unwrap();
    assert_eq!(image_request(&h.app).unwrap(), json!({"settings":expected}));
    assert!(image_request(&h.app).unwrap().get("progressive").is_none(), "CPU sampling must not select the GPU executor");
    h.step(&json!({"widget":h.app.tr("render.save_setup")})).unwrap();
    assert_eq!(h.app.session.doc.render.setups["Progressive CPU"], expected);
    h.step(&json!({"click":{"handle":"render:cpu-sampling"}})).unwrap();
    h.step(&json!({"click":{"handle":"render:cpu-fixed"}})).unwrap();
    assert!(h.app.render_workspace.view.raytrace.progressive.is_none());
    assert!(h.app.render_workspace.view.dirty);
    h.step(&json!({"widget":h.app.tr("render.reload_settings")})).unwrap();
    if h.app.render_workspace.view.dirty {
        h.step(&json!({"widget":h.app.tr("render.discard_continue")})).unwrap();
    }
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
        job: Some(RenderJob::CpuRaytrace { options }),
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
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
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
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    h.app.session.set_render_job_host(None).unwrap();
    let path = h.dir.join("original-embedded.hdr");
    let bytes = original_hdr_bytes(128, true);
    std::fs::write(&path, &bytes).unwrap();
    let resource = solidcraft_engine::doc::render_resources::HdrId::of(&bytes);
    let save = h.app.tr("render.save_setup").to_string();
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
    h.step(&json!({"widget":save})).unwrap();
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
    h.step(&json!({"widget":save})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.doc.render.resources.len(), 1, "both setups share one source payload");
    assert_eq!(h.app.session.doc.render.resources.total_bytes(), bytes.len());
    let cpu = h.app.session.doc.render.setups.get("HDR CPU").unwrap().clone();
    assert!(matches!(&cpu.job, Some(RenderJob::CpuRaytrace { options })
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
        let section = if target == hdr::Target::Cpu { "render.cpu_environment" } else { "render.studio_environment" };
        h.frames(2);
        if target == hdr::Target::Gpu {
            h.step(&json!({"click":{"handle":format!("dialog:section:{}", h.app.tr("render.image_advanced"))}})).unwrap();
        }
        h.step(&json!({"click":{"handle":format!("dialog:section:{}", h.app.tr(section))}})).unwrap();
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
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
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
        h.app.run("solid.box", json!({"body_name":"Target","length":10,"width":20,"height":5})).unwrap();
        enter(&mut h.app, true).unwrap();
        h.app.render_workspace.view.inspector.page = inspector::Page::All;
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
fn cpu_resource_profile_edits_preserve_resolution_samples_and_saved_intent() {
    use solidcraft_engine::render::job::{CpuBudget, CpuProgressiveOptions};
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2600}));
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    h.app.session.set_render_job_host(None).unwrap();
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    h.app.render_workspace.view.name = "Studio output".into();
    h.app.render_workspace.view.draft.width = 3840;
    h.app.render_workspace.view.draft.height = 2160;
    h.app.render_workspace.view.raytrace.budget = CpuBudget::Studio;
    h.app.render_workspace.view.raytrace.progressive = Some(CpuProgressiveOptions { samples_per_pixel: 128, seed: 82 });
    h.frames(3);
    let expected = setups::snapshot(&h.app).unwrap();
    setups::save(&mut h.app).unwrap();
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);

    h.step(&json!({"click":{"handle":"render.cpu_budget_compact"}})).unwrap();
    h.frames(3);
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (3840, 2160));
    assert_eq!(h.app.render_workspace.view.raytrace.progressive.unwrap().samples_per_pixel, 128);
    assert!(setups::snapshot(&h.app).is_err());
    assert!(h.app.render_workspace.view.dirty);
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
fn setup_actions_and_return_design_stay_visible_with_long_render_controls() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1280,"height":900}));
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    h.frames(3);
    for key in ["render.save_setup", "render.reload_settings", "render.return_design"] {
        let label = h.app.tr(key).to_string();
        assert!(h.widget(&label).is_some(), "sticky action {key}");
    }
    h.step(&json!({"widget":h.app.tr("render.return_design")})).unwrap();
    assert!(!active(&h.app));
}

#[test]
fn dirty_indication_preserves_reload_and_resource_picker_hit_targets() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2600}));
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    setups::save(&mut h.app).unwrap();
    setups::load(&mut h.app);
    h.frames(3);
    let reload = h.app.tr("render.reload_settings").to_string();
    let clean_reload = h.widget(&reload).unwrap();
    let clean_picker = crate::scenario::rect_of("render:cpu-budget").unwrap();
    h.app.render_workspace.view.draft.exposure_ev = 1.0;
    setups::refresh_dirty(&mut h.app);
    assert!(h.app.render_workspace.view.dirty);
    h.frames(3);
    assert_eq!(h.widget(&reload).unwrap(), clean_reload, "dirty text must not move Reload");
    assert_eq!(crate::scenario::rect_of("render:cpu-budget").unwrap(), clean_picker, "dirty text must not move the CPU resource picker");
    h.step(&json!({"widget":reload})).unwrap();
    if h.app.render_workspace.view.dirty {
        h.step(&json!({"widget":h.app.tr("render.discard_continue")})).unwrap();
    }
    assert!(!h.app.render_workspace.view.dirty);
    assert_eq!(h.app.render_workspace.view.draft.exposure_ev, 0.0);
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
    assert_eq!(
        (plan.pbr, plan.tris, plan.glass),
        (
            class_vertices(&gpu, crate::gpu::TriClass::Pbr).len(),
            class_vertices(&gpu, crate::gpu::TriClass::Opaque).len(),
            class_vertices(&gpu, crate::gpu::TriClass::Glass).len()
        )
    );
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
    use solidcraft_engine::geom::{Mesh, Vec3};
    use solidcraft_engine::render::{ProductMesh, surface::OpaqueSurface};
    // One immutable original triangle repeated in the index list; this tests
    // payload admission only. Actual SELENE upload is separate native evidence.
    let count = (128 * 1024 * 1024) / (3 * crate::gpu::TRI_SIZE) + 1;
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
    assert_eq!(plan.bytes().unwrap(), count * 3 * crate::gpu::TRI_SIZE);
    assert!(Arc::ptr_eq(&scene.meshes[0].mesh, &mesh));
    let mut maximum = PreviewUploadPlan::default();
    assert!(maximum.add(true, false, MAX_GEOMETRY_BYTES / (3 * crate::gpu::TRI_SIZE) + 1, MAX_GEOMETRY_BYTES).is_err());
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
    h.app
        .render_workspace
        .prepare_preview(signature, || solidcraft_engine::product_scene::extract(&session).map(|s| (s, Vec::new())).map_err(|e| e.to_string()))
        .unwrap();
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
    assert!(h.app.render_workspace.view.dirty);
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
    assert_eq!(h.app.render_workspace.view.inspector.page, inspector::Page::Scene);
}

#[test]
fn render_workspace_stage_chips_dispatch_without_mutations_and_remember_each_document() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1440,"height":1000}));
    enter(&mut h.app, true).unwrap();
    let document = h.app.session.doc.clone();
    let history = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    for stage in Stage::ALL {
        h.frames(3);
        h.step(&json!({"click":{"handle":format!("render:stage:{}",stage.key())}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.render_workspace.view.stage, stage);
        assert_eq!(h.app.render_workspace.view.inspector.page, stage.page());
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
            assert_eq!(h.app.render_workspace.view.inspector.page, inspector::Page::Diagnostics);
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
                        solidcraft_engine::geom::Vec3::new(x, y, z),
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
        .flat_map(|layout| [inspector::Page::Scene, inspector::Page::Appearance].map(|page| (layout, page)))
    {
        let mut h = Harness::new();
        h.app.ui.layout = layout;
        h.call("ui.resize", json!({"width":1600,"height":1000}));
        h.app.run("solid.box", json!({"length":30,"width":20,"height":10})).unwrap();
        h.frames(4);

        enter(&mut h.app, true).unwrap();
        h.app.render_workspace.view.inspector.page = page;
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
            } else if page == inspector::Page::Appearance {
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
        assert_eq!((h.app.ui.pick_bodies, h.app.ui.pick_components), (body, component));
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
    let left = proj.to_screen(solidcraft_engine::geom::Vec3::new(-20.0, 10.0, 20.0)).unwrap();
    let right = proj.to_screen(solidcraft_engine::geom::Vec3::new(40.0, 10.0, 20.0)).unwrap();
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
    assert!(!h.app.ui.pick_bodies && !h.app.ui.pick_components, "result canvas must disable the stage's Pick control");
    let path = h.dir.join("overlay-free.png");
    h.app.run("render.export", json!({"job":second,"path":path})).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), clean.png());
    assert_eq!(h.app.session.render_job_handle(second).unwrap().frame().unwrap().unwrap().png(), clean.png());
}

#[test]
fn render_workspace_pick_filter_rejects_invalid_requests_atomically() {
    let mut h = Harness::new();
    let doc = h.app.session.doc.clone();
    for mode in ["body", "face", "component"] {
        assert_eq!(h.app.run("select.filter", json!({"mode":mode})).unwrap()["mode"], mode);
        assert_eq!(h.app.ui.pick_bodies, mode == "body");
        assert_eq!(h.app.ui.pick_components, mode == "component");
    }
    for request in [json!({}), json!({"mode":"edge"}), json!({"mode":"body","extra":true})] {
        assert!(h.app.run("select.filter", request).is_err());
        assert!(!h.app.ui.pick_bodies && h.app.ui.pick_components);
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
    use solidcraft_engine::geom::Vec3;
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
    use solidcraft_engine::{Sel, geom::Vec3};
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
    assert_eq!(h.app.render_workspace.view.inspector.page, inspector::Page::Environment);
    h.step(&json!({"click":{"browser":"Key"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.inspector.page, inspector::Page::Lighting);
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
fn render_workspace_browser_shot_switch_keeps_the_existing_dirty_confirmation() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1440,"height":1100}));
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.name = "A".into();
    setups::save(&mut h.app).unwrap();
    h.app.run("render.settings.set", json!({"name":"B","set":{"width":321,"job":{"method":"raster"}}})).unwrap();
    h.app.render_workspace.view.draft.width = 123;
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:tree:shot:B"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.name, "A");
    assert!(inspector::confirmation_open(&h.app));
    h.step(&json!({"widget":h.app.tr("render.keep_editing")})).unwrap();
    assert_eq!(h.app.render_workspace.view.draft.width, 123);
    h.step(&json!({"click":{"handle":"render:tree:shot:B"}})).unwrap();
    h.step(&json!({"widget":h.app.tr("render.discard_continue")})).unwrap();
    assert_eq!(h.app.render_workspace.view.name, "B");
    assert_eq!(h.app.render_workspace.view.draft.width, 321);
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
    assert_eq!(h.app.render_workspace.view.inspector.page, inspector::Page::Environment);
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
    assert_eq!(h.app.render_workspace.view.inspector.page, inspector::Page::Scene);
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
    assert_eq!(h.app.render_workspace.view.inspector.page, inspector::Page::Lighting);
    assert!(crate::scenario::rect_of("render:light:editor").is_some());
    assert!(crate::scenario::handle_at("render:tree:shots:add").is_some());
    h.step(&json!({"click":{"handle":"render:tree:shots:add"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.inspector.page, inspector::Page::Scene);
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
    let evaluated = h.app.session.world_state();
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
    assert!(Arc::ptr_eq(&evaluated, &h.app.session.world_state()));
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
    h.app.render_workspace.view.inspector.page = inspector::Page::Camera;
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
    assert!(crate::scenario::handle_at("render:shots:update").is_some());
    h.step(&json!({"click":{"handle":"render:shots:update"}})).unwrap();
    h.frames(2);
    let mut expected = saved.clone();
    expected.camera = h.app.cam;
    assert_eq!(h.app.session.doc.render.setups["Studio"], expected, "Update must preserve saved output and lighting");
    assert_eq!(h.app.render_workspace.view.draft.width, 317, "Update must preserve the editor draft");
    assert!(!shots::camera_modified(&h.app, "Studio"));
    assert!(crate::scenario::rect_of("dialog:list:shot:Studio:modified").is_none());
    assert!(crate::scenario::handle_at("render:shots:update").is_none());
    h.app.cam.roll += 0.2;
    h.frames(3);
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":"render:shots:revert"}})).unwrap();
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
    let target = materials::read_targets(&mut h.app).unwrap().remove(0);
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
fn output_viewer_radial_defaults_enable_lights_and_keep_planned_compare_disabled() {
    let h = output_viewer_fixture();
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
    assert!(crate::mouse_gesture::item(&h.app, "render.lights.add").unwrap().enabled);
    assert!(!crate::mouse_gesture::item(&h.app, "render.compare").unwrap().enabled);
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
fn output_viewer_has_sections_without_pages_or_footer_and_selected_row_routes_to_output() {
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
    assert_eq!(h.app.render_workspace.view.inspector.page, inspector::Page::Lighting);
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
            ImageMethod::GpuRaytrace => "render.light_gpu_punctual",
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
    assert!(crate::scenario::rect_of("render:look_chain").is_some());
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
                .and_then(|rect| crate::viewport::projection(&h.app, rect).to_screen(solidcraft_engine::geom::Vec3::new(0.0, 0.0, 28.0)))
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
        h.app.render_workspace.view.inspector.page = inspector::Page::Lighting;
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
    h.app.render_workspace.view.inspector.page = inspector::Page::Lighting;
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
        Some(solidcraft_engine::doc::render_settings::RenderJob::CpuRaytrace { options }) if !options.shadows));
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
            // 2^21 PBR triangles in the blocks, 28 bytes a vertex: 168 MiB.
            mesh: Arc::new(Mesh { triangles: vec![[0; 3]; 1 << 21], ..Default::default() }),
            color: [128; 3],
            opacity: 1.0,
            surface: Some(
                solidcraft_engine::render::surface::OpaqueSurface::new(solidcraft_engine::geom::Vec3::new(0.5, 0.5, 0.5), 0.0, 0.5, 1.5).unwrap(),
            ),
            transmission: None,
            faces: Vec::new(),
        }],
        bounds: Aabb3::EMPTY,
    };
    let error = gpu_scene_cached(&scene, &[], WEB_GEOMETRY_BYTES, &mut ProductCache::default()).err().unwrap();
    assert!(error.contains("168.0 MiB"), "{error}");
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
    assert!(state.prepare_preview((7, 123, 99, bytes - 1), || Ok((scene.clone(), Vec::new()))).is_err());
    state.prepare_preview((7, 123, 99, bytes), || Ok((scene, Vec::new()))).unwrap();
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
    use solidcraft_engine::geom::{Aabb3, Vec3};
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

fn corners(b: &solidcraft_engine::geom::Aabb3) -> Vec<solidcraft_engine::geom::Vec3> {
    let mut out = Vec::new();
    for x in [b.min.x, b.max.x] {
        for y in [b.min.y, b.max.y] {
            for z in [b.min.z, b.max.z] {
                out.push(solidcraft_engine::geom::Vec3::new(x, y, z));
            }
        }
    }
    out
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
