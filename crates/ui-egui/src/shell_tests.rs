use super::*;
use crate::scenario::{Harness, rect_of};
use crate::toolbar;
use serde_json::json;
use solidcraft_engine::panels::ActionSink as _;

/// Inspect existing caches; never warm/evaluate geometry as part of an assertion.
fn assert_panel_host_preserves_design(h: &mut Harness, exercise: impl FnOnce(&mut Harness)) {
    let document = h.app.session.doc.clone();
    let model = h.app.session.model.state();
    let cache = h.app.session.model.cache.clone();
    let meshes = model.bodies.iter().map(|body| body.cached_mesh()).collect::<Vec<_>>();
    let revision = h.app.session.revision;
    let undo = h.app.session.undo.len();
    let dirty = h.app.session.is_dirty();
    let recomputed = h.app.session.model.recomputed;
    let camera = h.app.cam;
    let render_domain = h.app.viewport.rect;
    let sample_points = [solidcore_record::Vec3::ZERO, solidcore_record::Vec3::new(1.0, 2.0, 3.0)];
    let screen_points = render_domain.map(|domain| sample_points.map(|point| crate::viewport::projection(&h.app, domain).to_screen(point)));
    let preferences = serde_json::to_value(&h.app.preferences).unwrap();
    exercise(h);
    assert!(std::sync::Arc::ptr_eq(&document, &h.app.session.doc), "panel chrome must retain document identity");
    let after = h.app.session.model.state();
    assert!(std::sync::Arc::ptr_eq(&model, &after), "panel chrome must retain model identity");
    assert!(std::sync::Arc::ptr_eq(&cache, &h.app.session.model.cache), "panel chrome must retain evaluation cache identity");
    assert_eq!(h.app.session.revision, revision);
    assert_eq!(h.app.session.undo.len(), undo);
    assert_eq!(h.app.session.is_dirty(), dirty);
    assert_eq!(h.app.session.model.recomputed, recomputed);
    assert_eq!(h.app.cam, camera);
    if render_domain.is_some() {
        assert_eq!(h.app.viewport.rect, render_domain, "ordinary panel work preserves the full render projection");
        assert_eq!(
            h.app.viewport.rect.map(|domain| sample_points.map(|point| crate::viewport::projection(&h.app, domain).to_screen(point))),
            screen_points,
            "panel layout never changes the model's screen position or framing offset"
        );
    }
    assert_eq!(serde_json::to_value(&h.app.preferences).unwrap(), preferences);
    for (body, mesh) in after.bodies.iter().zip(meshes) {
        match (mesh, body.cached_mesh()) {
            (Some(before), Some(after)) => assert!(std::sync::Arc::ptr_eq(&before, &after), "panel chrome must retain cached mesh identity"),
            (None, None) => {}
            _ => panic!("panel chrome changed mesh cache population"),
        }
    }
}

#[test]
fn universal_panel_host_is_opt_in_and_each_specimen_preserves_design_and_preferences() {
    for preset in ["docks", "tabs", "collapsed", "floating", "overflow", "pinned", "surfaces"] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
        h.settle();
        assert!(h.app.panel_runtime().is_none());
        assert!(h.app.universal_panel_frame().is_none());
        let legacy_browser = rect_of("browser:card");
        assert!(legacy_browser.is_some(), "default still mounts legacy Browser");
        assert_panel_host_preserves_design(&mut h, |h| {
            crate::panel_runtime::enable_host_specimen(&mut h.app, preset).unwrap();
            h.frames(4);
            assert!(h.app.universal_panel_frame().is_some(), "actual workspace host: {preset}");
            let runtime = h.app.panel_runtime().unwrap();
            for spec in runtime.layout().registry().specs() {
                assert!(!spec.header_actions.iter().any(|action| action.id.0 == "pulse"), "native specimens have no debug action");
            }
            assert!(!h.app.universal_panel_frame().unwrap().controls.iter().any(|control| matches!(&control.target,
                crate::panel_runtime::HostControlTarget::Pointer(crate::panel_runtime::HostPointerTarget::HeaderAction { action, .. }) if action.0 == "pulse")));
            assert!(rect_of("browser:card").is_none(), "legacy placement must be suppressed: {preset}");
            h.app.disable_universal_panel_host().unwrap();
            h.frames(3);
            assert!(h.app.universal_panel_frame().is_none());
            assert!(rect_of("browser:card").is_some(), "legacy placement returns when opt-in ends");
        });
    }
}

#[test]
fn surface_fixture_routes_real_input_through_short_panel_gaps_at_both_scales_and_directions() {
    use crate::panel_runtime::egui_rect;
    use solidcraft_engine::panels::{PanelSurface, Point, Zone};
    for scale in [1.0, 2.0] {
        for language in ["en", "ar"] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.preferences.language = language.into();
            h.call("ui.resize", json!({"width":1600,"height":1100}));
            h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
            h.settle();
            h.ctx.set_pixels_per_point(scale);
            crate::panel_runtime::control_specimen(&mut h.app, &json!({"preset":"surfaces","translucency_percent":100})).unwrap();
            h.frames(5);
            let frame = h.app.universal_panel_frame().unwrap();
            assert_eq!(frame.layout.render_domain, crate::panel_runtime::logical_rect(h.app.viewport.rect.unwrap()));
            let left = frame.layout.sections.iter().find(|section| section.zone == Zone::Left).unwrap();
            assert_eq!(left.surface, PanelSurface::Opaque, "the inactive text-heavy editor constrains its tab group");
            let clear = frame.layout.sections.iter().find(|section| section.instance.0 == "specimen-2").unwrap();
            assert_eq!(clear.surface, PanelSurface::Translucent);
            assert!(clear.contrast.is_some(), "effective alpha must have a truthful diagnostic");
            let body = egui_rect(clear.body.unwrap());
            let blocked = body.center();
            let gap = egui::pos2(left.outer.origin.x + left.outer.size.width * 0.5, left.outer.origin.y + left.outer.size.height + 24.0);
            assert!(!frame.layout.consumes_pointer(Point { x: gap.x, y: gap.y }), "short side content leaves a real gap");
            let camera = h.app.cam;
            h.call("ui.move", json!({"x":blocked.x,"y":blocked.y}));
            h.call("ui.scroll", json!({"x":blocked.x,"y":blocked.y,"delta":120}));
            assert!(h.app.viewport.mouse.is_none(), "translucent content shields model picking");
            assert_eq!(h.app.cam, camera, "content wheel cannot reach model navigation");
            h.call("ui.move", json!({"x":gap.x,"y":gap.y}));
            assert_eq!(h.app.viewport.mouse, Some(gap), "uncovered points reach the existing picking path");
            h.call("ui.scroll", json!({"x":gap.x,"y":gap.y,"delta":120}));
            assert_ne!(h.app.cam.distance.to_bits(), camera.distance.to_bits(), "gap wheel reaches the existing navigation path");
        }
    }
}

#[test]
fn specimen_rejects_unknown_presets_and_cannot_take_over_an_existing_runtime() {
    let mut h = Harness::new();
    h.app.home.open = false;
    assert_panel_host_preserves_design(&mut h, |h| {
        assert!(crate::panel_runtime::enable_host_specimen(&mut h.app, "unbounded-user-input").is_err());
        assert!(h.app.panel_runtime().is_none());
        assert!(h.app.universal_panel_frame().is_none());
        crate::panel_runtime::enable_host_specimen(&mut h.app, "tabs").unwrap();
        h.frames(3);
        let before = h.app.panel_runtime().unwrap().layout().inspect();
        let controllers = h.app.panel_runtime().unwrap().controller_count();
        assert!(crate::panel_runtime::enable_host_specimen(&mut h.app, "docks").is_err());
        assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), before);
        assert_eq!(h.app.panel_runtime().unwrap().controller_count(), controllers);
        h.frames(2);
        assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), before);
    });
}

fn assert_host_legacy_window(h: &Harness, bounds: egui::Rect) {
    use crate::panel_runtime::logical_rect;
    assert_eq!(h.app.viewport.host_windows.as_deref(), Some([bounds].as_slice()), "only the actual legacy window reserves space");
    let usable = crate::shell::host_usable(&h.app).unwrap();
    assert!(usable.obstacles.contains(&logical_rect(bounds)));
    let controls = h.app.viewport.layout.as_ref().unwrap();
    assert!(controls.usable.as_ref().unwrap().obstacles.contains(&logical_rect(bounds)));
    for control in [controls.navigation, controls.axis_view] {
        if control.is_positive() {
            assert!(!control.intersect(bounds).is_positive(), "same-pass controls avoid the real editor");
        }
    }
}

fn assert_host_legacy_windows_closed(h: &Harness) {
    assert_eq!(h.app.viewport.host_windows.as_deref(), Some([].as_slice()));
    assert!(rect_of("dialog:window").is_none() && rect_of("dialog:outer").is_none());
    assert_eq!(crate::shell::host_usable(&h.app).unwrap(), h.app.universal_panel_frame().unwrap().layout.usable);
}

#[test]
fn host_retains_one_legacy_move_editor_and_the_pending_capture_question() {
    let (mut h, _, occurrence) = crate::test_support::component_box();
    h.app.home.open = false;
    h.app.preferences.language = "en".into();
    h.app.run("occurrence.move", json!({"occurrence":occurrence,"translate":[10,0,0],"capture":false})).unwrap();
    h.settle();
    let pending = h.app.session.pending_moves.clone();
    h.app.tree.occurrence_move = Some(crate::browser::OccMove::new(occurrence));
    h.frames(3);
    let baseline = h.app.tree.occurrence_move.as_ref().unwrap().baseline;
    assert!(baseline.is_some());
    assert_panel_host_preserves_design(&mut h, |h| {
        crate::panel_runtime::enable_host_specimen(&mut h.app, "tabs").unwrap();
        h.frames(4);
        let draft = h.app.tree.occurrence_move.as_ref().unwrap();
        assert_eq!(draft.baseline, baseline);
        assert!(draft.had_pending);
        assert_eq!(draft.translate, [0.0; 3]);
        assert_eq!(crate::scenario::handles("occurrence-move:distance:0").len(), 1);
        assert_host_legacy_window(h, rect_of("dialog:window").unwrap());
        assert_eq!(h.app.session.pending_moves, pending);
    });
    // Cancel must restore the opening baseline, including the earlier pending move.
    h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
    h.frames(2);
    assert!(h.app.tree.occurrence_move.is_none());
    assert_eq!(h.app.session.pending_moves, pending);
    assert_host_legacy_windows_closed(&h);
    assert_panel_host_preserves_design(&mut h, |h| {
        h.app.start("sketch.create");
        h.frames(3);
        assert_eq!(h.app.tree.capture_prompt.as_deref(), Some("sketch.create"));
        assert!(h.app.dialog.is_none(), "the guarded command still waits for its existing question");
        assert!(h.widget("Capture Position").is_some() && h.widget("Revert").is_some());
        assert_host_legacy_window(h, rect_of("dialog:window").unwrap());
        h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
        h.frames(2);
        assert!(h.app.tree.capture_prompt.is_none() && h.app.dialog.is_none());
        assert_eq!(h.app.session.pending_moves, pending);
        assert_host_legacy_windows_closed(h);
    });
}

#[test]
fn host_keeps_legacy_canvas_cloud_and_attachment_drafts_with_only_visible_footprints() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.preferences.language = "en".into();
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.extend(400u32.to_be_bytes());
    png.extend(200u32.to_be_bytes());
    png.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    let data = solidcraft_engine::doc::canvas::base64_encode(&png);
    let canvas = h.app.run("canvas.insert", json!({"data":data,"plane":"XY","width":80})).unwrap()["canvas"].as_u64().unwrap();
    let path = h.dir.join("legacy-editor.xyz");
    std::fs::write(&path, "0 0 0 255 0 0\n1 0 0 0 255 0\n").unwrap();
    let cloud = h.app.run("pointcloud.insert", json!({"path":path.to_string_lossy()})).unwrap()["cloud"].as_u64().unwrap();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    let sketch = h.app.session.active_sketch.unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.settle();
    crate::panel_runtime::enable_host_specimen(&mut h.app, "tabs").unwrap();
    h.frames(4);
    h.ctx.options_mut(|options| options.max_passes = 1.try_into().unwrap());
    for editor in ["canvas", "cloud", "attachment"] {
        assert_panel_host_preserves_design(&mut h, |h| {
            match editor {
                "canvas" => {
                    let mut draft = crate::browser::CanvasPanel::new(&h.app, canvas, true);
                    draft.a = [3.0, 4.0];
                    draft.distance = 73.0;
                    h.app.tree.canvas_panel = Some(draft);
                }
                "cloud" => h.app.tree.cloud_panel = Some(crate::browser::CloudPanel::new(&h.app, cloud)),
                _ => {
                    h.app.tree.redefine = Some(sketch);
                    let mut draft = crate::sketch_attachment_ui::Draft::new(&h.app, sketch);
                    draft.label = "retained plane draft".into();
                    draft.error = Some("retained draft error".into());
                    h.app.tree.redefine_draft = Some(draft);
                }
            }
            h.frame();
            assert_eq!(h.app.viewport.host_windows.as_deref(), Some([].as_slice()), "{editor}: invisible sizing cannot reserve a window");
            h.frames(3);
            let name = if editor == "attachment" { "dialog:outer" } else { "dialog:window" };
            assert_eq!(crate::scenario::rects(name).iter().filter(|(key, _)| key == name).count(), 1, "{editor}: one legacy producer");
            assert_host_legacy_window(h, rect_of(name).unwrap());
            match editor {
                "canvas" => {
                    let draft = h.app.tree.canvas_panel.as_ref().unwrap();
                    assert_eq!((draft.a, draft.distance), ([3.0, 4.0], 73.0));
                }
                "cloud" => assert_eq!(h.app.tree.cloud_panel.as_ref().unwrap().cloud, cloud),
                _ => {
                    let draft = h.app.tree.redefine_draft.as_ref().unwrap();
                    assert_eq!(draft.label, "retained plane draft");
                    assert_eq!(draft.error.as_deref(), Some("retained draft error"));
                }
            }
            h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
            h.frames(2);
            assert!(h.app.tree.canvas_panel.is_none() && h.app.tree.cloud_panel.is_none() && h.app.tree.redefine_draft.is_none());
            assert_host_legacy_windows_closed(h);
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn host_clears_suppressed_assistant_geometry_without_losing_chat_or_tool_width() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.ui.layout = LayoutMode::Floating;
    h.call("ui.agent_chat", json!({"open":true,"side":"right","width":440,"picker":false}));
    h.frames(3);
    h.app.agent_chat.input = "retained unsent draft".into();
    h.app.agent_chat.items.push(crate::agent_chat::Item::Agent { thought: false, text: "retained conversation".into() });
    let items = crate::agent_chat::status(&h.app)["items"].clone();
    let stale = h.app.agent_chat.panel_rect.unwrap();
    let viewport = h.app.viewport.rect.unwrap();
    assert!(crate::agent_chat::tool_space(&h.app, viewport).width() < viewport.width());
    assert_panel_host_preserves_design(&mut h, |h| {
        crate::panel_runtime::enable_host_specimen(&mut h.app, "tabs").unwrap();
        h.frame();
        assert!(h.app.agent_chat.panel_rect.is_none() && h.app.agent_chat.drop_rect.is_none() && h.app.agent_chat.dock_space.is_none());
        assert!(rect_of("agent_chat:panel").is_none());
        let domain = h.app.viewport.rect.unwrap();
        assert_eq!(crate::agent_chat::tool_space(&h.app, domain), domain);
        assert_eq!(crate::shell::tool_drag_rect(&h.app, domain).x_range(), domain.x_range());
        assert!(!crate::shell::host_usable(&h.app).unwrap().obstacles.contains(&crate::panel_runtime::logical_rect(stale)));
        assert!(h.app.agent_chat.open);
        assert_eq!(h.app.agent_chat.input, "retained unsent draft");
        assert_eq!(crate::agent_chat::status(&h.app)["items"], items);
        h.app.disable_universal_panel_host().unwrap();
        h.frames(3);
        assert!(h.app.agent_chat.panel_rect.is_some() && h.app.agent_chat.drop_rect.is_some());
        assert_eq!(h.app.agent_chat.input, "retained unsent draft");
        assert_eq!(crate::agent_chat::status(&h.app)["items"], items);
    });
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn host_retains_the_existing_popped_assistant_window_path() {
    // Harness uses egui's embedded native-window fallback; this is not OS-window evidence.
    let mut h = Harness::new();
    h.app.home.open = false;
    let saved_place = Some([120.0, 80.0, 520.0, 700.0]);
    h.app.preferences.agent_chat_window = saved_place;
    h.call("ui.agent_chat", json!({"open":true,"popped":true,"picker":false}));
    h.frames(3);
    assert!(rect_of("agent_chat:window").is_some() && h.app.agent_chat.window_placed);
    h.app.agent_chat.input = "native window draft".into();
    assert_panel_host_preserves_design(&mut h, |h| {
        crate::panel_runtime::enable_host_specimen(&mut h.app, "tabs").unwrap();
        h.frames(4);
        assert!(rect_of("agent_chat:window").is_some(), "host enablement must still invoke the existing native-window producer");
        assert!(rect_of("agent_chat:panel").is_none());
        assert!(h.app.agent_chat.window_placed && h.app.agent_chat.open && h.app.preferences.agent_chat_popped);
        assert_eq!(h.app.agent_chat.input, "native window draft");
        let domain = h.app.viewport.rect.unwrap();
        assert_eq!(crate::agent_chat::tool_space(&h.app, domain), domain);
        h.step(&json!({"click":{"handle":"agent_chat:dock"}})).unwrap();
        h.frames(2);
        assert!(!h.app.preferences.agent_chat_popped && !h.app.agent_chat.window_placed);
        assert!(rect_of("agent_chat:window").is_none());
        assert!(h.app.agent_chat.panel_rect.is_none() && h.app.agent_chat.drop_rect.is_none() && h.app.agent_chat.dock_space.is_none());
        assert_eq!(h.app.preferences.agent_chat_window, saved_place, "Dock retains the saved opening position and size");
        assert!(h.app.agent_chat.open);
        assert_eq!(h.app.agent_chat.input, "native window draft");
        h.call("ui.agent_chat", json!({"popped":true}));
        h.frames(3);
        assert!(rect_of("agent_chat:window").is_some() && h.app.agent_chat.window_placed);
        assert_eq!(h.app.preferences.agent_chat_window, saved_place);

        h.app.agent_chat.items.push(crate::agent_chat::Item::Agent { thought: false, text: "Retained transcript before New Chat".into() });
        h.app.agent_chat.pending.push(crate::agent_chat::Pending {
            text: "Retained queued message".into(),
            image: false,
            context: Default::default(),
            files: Vec::new(),
        });
        h.frames(2);
        let pending = h.app.agent_chat.pending.clone();
        let transcript = rect_of("agent_chat:msg:0").expect("the real transcript publishes its message footprint").center();
        assert!(h.app.agent_chat.panel_rect.unwrap().contains(transcript));
        let composer = egui::Id::unique("sc_agent_chat_input");
        h.ctx.memory_mut(|memory| memory.surrender_focus(composer));
        assert!(!h.ctx.memory(|memory| memory.has_focus(composer)));
        assert!(crate::agent_chat::new_chat_needs_confirm(&h.app));
        assert!(!h.app.agent_chat.confirm_new);
        // Existing New Chat is Command/Ctrl+Shift+L, including transcript hover.
        h.frame_with(vec![
            egui::Event::PointerMoved(transcript),
            egui::Event::Key {
                key: egui::Key::L,
                physical_key: Some(egui::Key::L),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT),
            },
        ]);
        assert!(h.app.agent_chat.confirm_new, "hover routes New Chat without composer focus");
        assert_eq!(h.app.agent_chat.pending, pending, "the existing guard keeps queued work until confirmation");
        assert!(
            matches!(h.app.agent_chat.items.as_slice(), [crate::agent_chat::Item::Agent { text, .. }] if text == "Retained transcript before New Chat")
        );
        assert_eq!(h.app.agent_chat.input, "native window draft");
        h.frame_with(vec![egui::Event::Key {
            key: egui::Key::L,
            physical_key: Some(egui::Key::L),
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT),
        }]);
        h.step(&json!({"click":{"handle":"agent_chat:new_confirm"}})).unwrap();
        h.frames(2);
        assert!(!h.app.agent_chat.confirm_new && !crate::agent_chat::new_chat_needs_confirm(&h.app));
        assert!(h.app.agent_chat.items.is_empty() && h.app.agent_chat.pending.is_empty());
        assert_eq!(h.app.agent_chat.input, "native window draft", "existing New Chat leaves the unsent composer draft intact");
        h.app.disable_universal_panel_host().unwrap();
        h.frames(3);
        assert!(rect_of("agent_chat:window").is_some() && h.app.agent_chat.window_placed);
        assert_eq!(h.app.agent_chat.input, "native window draft");
    });
}

fn panel_host_point(h: &Harness, target: crate::panel_runtime::HostPointerTarget) -> egui::Pos2 {
    let bounds =
        h.app.universal_panel_frame().unwrap().layout.control(&target).unwrap_or_else(|| panic!("missing actual host control {target:?}")).bounds;
    egui::pos2(bounds.origin.x + bounds.size.width * 0.5, bounds.origin.y + bounds.size.height * 0.5)
}

fn panel_host_drop_point(
    h: &Harness,
    instance: &solidcraft_engine::panels::InstanceId,
    target: &crate::panel_runtime::HostDropTargetId,
) -> egui::Pos2 {
    let frame = h.app.universal_panel_frame().unwrap();
    let targets = crate::panel_runtime::host_drop_targets(h.app.panel_runtime().unwrap().layout(), &frame.layout, instance).unwrap();
    let bounds =
        targets.iter().rev().find(|candidate| candidate.id == *target).unwrap_or_else(|| panic!("missing explicit landing {target:?}")).bounds;
    egui::pos2(bounds.origin.x + bounds.size.width / 2.0, bounds.origin.y + bounds.size.height / 2.0)
}

fn panel_host_pointer(h: &mut Harness, at: egui::Pos2, pressed: bool) {
    h.frame_with(vec![
        egui::Event::PointerMoved(at),
        egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
    ]);
}

fn panel_host_click(h: &mut Harness, at: egui::Pos2) {
    h.frame_with(vec![egui::Event::PointerMoved(at)]);
    panel_host_pointer(h, at, true);
    panel_host_pointer(h, at, false);
}

fn panel_host_key(h: &mut Harness, key: egui::Key) {
    h.frame_with(vec![egui::Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]);
    h.frame_with(vec![egui::Event::Key { key, physical_key: Some(key), pressed: false, repeat: false, modifiers: egui::Modifiers::NONE }]);
}

fn panel_host_semantic_click(h: &mut Harness, target: crate::panel_runtime::HostControlTarget) {
    let bounds = h
        .app
        .universal_panel_frame()
        .unwrap()
        .controls
        .iter()
        .find(|control| control.target == target)
        .unwrap_or_else(|| panic!("missing actual action {target:?}"))
        .bounds;
    panel_host_click(h, egui::pos2(bounds.origin.x + bounds.size.width / 2.0, bounds.origin.y + bounds.size.height / 2.0));
    h.frames(2);
}

fn assert_panel_host_landing(h: &Harness, shown: &crate::panel_runtime::HostLayout) {
    let committed = &h.app.universal_panel_frame().unwrap().layout;
    assert_eq!(committed.sections, shown.sections, "paint the exact admitted slots and neighbours");
    assert_eq!(committed.resolved.panels, shown.resolved.panels);
    assert_eq!(committed.extent, shown.extent);
    assert_eq!(committed.allocation_environment.scroll, shown.allocation_environment.scroll);
    assert_eq!(committed.render_domain, shown.render_domain);
}

fn warm_empty_universal_canvas_before_cold_cards(h: &mut Harness) {
    use solidcraft_engine::panels::{Layout, LayoutAction};
    if h.app.universal_panel_frame().is_none() {
        let capacity = crate::panel_runtime::logical_rect(h.ctx.content_rect());
        let app = &mut h.app;
        app.panel_runtime.as_mut().unwrap().prepare_host(&app.session, capacity).unwrap();
    }
    let saved = h.app.panel_runtime().unwrap().layout().layout().clone();
    h.app.dispatch_panel_layout(LayoutAction::Replace { expected_revision: None, layout: Layout::empty(saved.workspace.clone()) }).unwrap();
    // The universal canvas has a distinct egui identity. Establish its actual
    // hit rectangle without creating floating Area sizing state.
    h.frames(2);
    let empty = h.app.universal_panel_frame().unwrap();
    assert!(empty.layout.sections.is_empty());
    assert!(empty.layout.painted.is_empty() && empty.layout.input.is_empty());
    assert!(h.app.viewport.rect.is_some());
    h.app.dispatch_panel_layout(LayoutAction::Replace { expected_revision: None, layout: saved }).unwrap();
}

#[test]
fn actual_tabs_preset_raw_widget_drag_retains_the_inactive_editor_identity_and_draft() {
    use crate::panel_runtime::{HostControlTarget, HostEvent, HostPointerTarget, enable_host_specimen, inspect_host_specimen};
    use solidcraft_engine::panels::{Destination, InstanceId, Point, SectionId};
    fn editor_probe(observation: &serde_json::Value) -> &serde_json::Value {
        observation["runtime"]["probe"]["controllers"].as_array().unwrap().iter().find(|controller| controller["instance"] == "specimen-0").unwrap()
    }
    fn assert_inactive_editor_has_not_rendered(h: &mut Harness, before: &serde_json::Value) {
        let observation = inspect_host_specimen(&mut h.app).unwrap();
        let editor = editor_probe(&observation);
        assert_eq!(editor["state"]["presented"], false);
        assert_eq!(editor["renders"], before["renders"], "press, preview and cancellation cannot render the inactive editor");
    }
    for scale in [1.0, 2.0] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.ctx.set_pixels_per_point(scale);
        enable_host_specimen(&mut h.app, "tabs").unwrap();
        h.frames(3);
        let editor = InstanceId::new("specimen-0").unwrap();
        let source_id = SectionId::new("specimen-section-0").unwrap();
        let destination_id = SectionId::new("specimen-section-1").unwrap();
        let body = h.app.universal_panel_frame().unwrap().layout.sections.iter().find(|section| section.instance == editor).unwrap().body.unwrap();
        let draft_label = h.app.tr("ui.panel_fixture.draft").to_string();
        let mut edit_at = None;
        for _ in 0..8 {
            let nodes = &h.output.accesskit_update.as_ref().unwrap().nodes;
            edit_at = nodes
                .iter()
                .filter(|(_, node)| {
                    node.role() == egui::accesskit::Role::MultilineTextInput
                        && nodes.iter().any(|(id, label)| {
                            node.labelled_by().contains(id)
                                && label.role() == egui::accesskit::Role::Label
                                && label.value() == Some(draft_label.as_str())
                        })
                })
                .find_map(|(_, node)| {
                    let bounds = node.bounds()?;
                    let at = Point { x: ((bounds.x0 + bounds.x1) / 2.0) as f32, y: ((bounds.y0 + bounds.y1) / 2.0) as f32 };
                    body.contains(at).then_some(egui::pos2(at.x, at.y))
                });
            if edit_at.is_some() {
                break;
            }
            let at = egui::pos2(body.origin.x + body.size.width / 2.0, body.origin.y + body.size.height / 2.0);
            h.frame_with(vec![
                egui::Event::PointerMoved(at),
                egui::Event::MouseWheel {
                    source: egui::MouseWheelSource::Unknown,
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -64.0),
                    modifiers: egui::Modifiers::NONE,
                    phase: egui::TouchPhase::Move,
                },
            ]);
            h.frames(8);
        }
        let edit_at = edit_at.expect("the actual specimen editor has a reachable accessible text input inside its body");
        panel_host_click(&mut h, edit_at);
        h.frame_with(vec![egui::Event::Text("Draft entered through the specimen text field".into())]);
        panel_host_semantic_click(&mut h, HostControlTarget::Pointer(HostPointerTarget::Tab { instance: InstanceId::new("specimen-2").unwrap() }));
        let observation = inspect_host_specimen(&mut h.app).unwrap();
        let before_editor = editor_probe(&observation).clone();
        assert_eq!(before_editor["draft"], "Draft entered through the specimen text field");
        assert_eq!(before_editor["state"]["presented"], false);

        assert_panel_host_preserves_design(&mut h, |h| {
            let before = h.app.panel_runtime().unwrap().layout().inspect();
            let target = HostControlTarget::Pointer(HostPointerTarget::Tab { instance: editor.clone() });
            let control = h.app.universal_panel_frame().unwrap().controls.iter().find(|control| control.target == target).unwrap();
            assert!(
                h.output.accesskit_update.as_ref().unwrap().nodes.iter().any(|(id, node)| *id == control.id.accesskit_id()
                    && node.role() == egui::accesskit::Role::Tab
                    && node.label() == Some(control.accessible_name.as_str())
                    && node.is_selected() == Some(false)),
                "use the actual inactive accessible Tab widget, not a reducer-only target"
            );
            let bounds = control.bounds;
            let card = h.app.universal_panel_frame().unwrap().layout.sections.iter().find(|card| card.section == source_id).unwrap();
            assert_eq!(bounds.origin.y, card.header.origin.y);
            assert_eq!(bounds.size.height, card.header.size.height);
            assert!(!crate::panel_runtime::egui_rect(card.title.unwrap()).intersect(crate::panel_runtime::egui_rect(bounds)).is_positive());
            let start = egui::pos2(bounds.origin.x + bounds.size.width / 2.0, bounds.origin.y + bounds.size.height / 2.0);
            h.frame_with(vec![egui::Event::PointerMoved(start)]);
            panel_host_pointer(h, start, true);
            assert_inactive_editor_has_not_rendered(h, &before_editor);
            h.frame_with(vec![egui::Event::PointerMoved(start + egui::vec2(5.99, 0.0))]);
            assert!(h.app.universal_panel_frame().unwrap().preview.is_none());
            assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), before);
            assert_inactive_editor_has_not_rendered(h, &before_editor);
            h.frame_with(vec![egui::Event::PointerMoved(start + egui::vec2(6.0, 0.0))]);
            assert!(h.app.universal_panel_frame().unwrap().preview.is_some());
            assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), before);
            assert_inactive_editor_has_not_rendered(h, &before_editor);
            let end =
                panel_host_drop_point(h, &editor, &crate::panel_runtime::HostDropTargetId::Tab { section: destination_id.clone(), before: None });
            h.frame_with(vec![egui::Event::PointerMoved(end)]);
            let preview = h.app.universal_panel_frame().unwrap().preview.as_ref().unwrap();
            assert_eq!(preview.destination, Some(Destination::JoinTab { section: destination_id.clone(), index: 2 }));
            assert!(preview.rejection.is_none());
            let candidate = preview.projected.clone().expect("the preview includes the exact landing and affected neighbours");
            assert!(preview.tab_marker.is_some());
            assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), before);
            assert_inactive_editor_has_not_rendered(h, &before_editor);
            panel_host_key(h, egui::Key::Escape);
            panel_host_pointer(h, start + egui::vec2(6.0, 0.0), false);
            h.frames(2);
            assert_eq!(
                h.app.panel_runtime().unwrap().layout().inspect(),
                before,
                "Escape and the eventual real Tab release cannot activate the inactive editor"
            );
            assert!(h.app.universal_panel_frame().unwrap().preview.is_none());
            assert!(!h.app.panel_runtime_mut().unwrap().host().unwrap().interaction().is_captured());
            assert_inactive_editor_has_not_rendered(h, &before_editor);

            // A fresh accepted drag activates the retained editor only as part
            // of the same Move+Activate transaction that makes its landing visible.
            let start = panel_host_point(h, HostPointerTarget::Tab { instance: editor.clone() });
            panel_host_pointer(h, start, true);
            h.frame_with(vec![egui::Event::PointerMoved(end)]);
            let preview = h.app.universal_panel_frame().unwrap().preview.as_ref().unwrap();
            assert_eq!(preview.destination, Some(Destination::JoinTab { section: destination_id.clone(), index: 2 }));
            assert!(preview.rejection.is_none() && preview.tab_marker.is_some());
            assert_eq!(preview.projected.as_ref(), Some(&candidate));
            let projected = preview.projected.clone().unwrap();
            assert_inactive_editor_has_not_rendered(h, &before_editor);
            panel_host_pointer(h, end, false);
            let release = h.app.universal_panel_frame().unwrap();
            let commits = release
                .events
                .iter()
                .filter_map(|event| match event {
                    HostEvent::PreparedCommitted { reply } => Some(reply),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(commits.len(), 1, "consume the shown candidate exactly once");
            assert_eq!(commits[0].revision, before.revision + 1);
            h.frames(2);
            let layout = h.app.panel_runtime().unwrap().layout().layout();
            assert_eq!(layout.section(&source_id).unwrap().tabs, vec![InstanceId::new("specimen-2").unwrap()]);
            assert_eq!(layout.section(&source_id).unwrap().active, InstanceId::new("specimen-2").unwrap());
            let destination = layout.section(&destination_id).unwrap();
            assert_eq!(destination.tabs, vec![InstanceId::new("specimen-1").unwrap(), InstanceId::new("specimen-3").unwrap(), editor.clone()]);
            assert_eq!(destination.active, editor.clone(), "the dragged retained editor is the visible landing tab");
            assert_eq!(layout.sections.len(), 2);
            assert_eq!(h.app.panel_runtime().unwrap().layout().inspect().revision, before.revision + 1);
            assert_panel_host_landing(h, &projected);
        });
        let after = inspect_host_specimen(&mut h.app).unwrap();
        let retained = editor_probe(&after);
        for field in ["controller_identity", "draft", "transaction_live", "applies", "cancels", "finishes", "reopens", "destroys"] {
            assert_eq!(retained[field], before_editor[field], "retained editor state survives its actual Tab drag: {field}");
        }
        assert_eq!(retained["state"]["presented"], true);
        assert!(
            retained["renders"].as_u64().unwrap() > before_editor["renders"].as_u64().unwrap(),
            "the accepted active landing must actually render the editor"
        );
        assert_eq!(retained["reparents"].as_u64().unwrap(), before_editor["reparents"].as_u64().unwrap() + 1);
        assert!(retained["updates"].as_u64().unwrap() > before_editor["updates"].as_u64().unwrap());
        assert_eq!(after["runtime"]["controller_count"], observation["runtime"]["controller_count"]);
        assert_eq!(after["runtime"]["probe"]["content_command_calls"], 0);
    }
}

#[test]
fn actual_clamped_floating_resize_preserves_session_provider_controller_and_camera_invariants() {
    use crate::panel_runtime::{HostEvent, HostPointerTarget, HostResizeEdge, enable_host_specimen, inspect_host_specimen};
    use solidcraft_engine::panels::{LayoutAction, Mutation, Point, SectionId, Size};
    for scale in [1.0, 2.0] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
        h.settle();
        h.call("ui.resize", json!({"width":800,"height":800}));
        h.ctx.set_pixels_per_point(scale);
        enable_host_specimen(&mut h.app, "floating").unwrap();
        h.frames(3);
        let section_id = SectionId::new("specimen-section-0").unwrap();
        h.app
            .dispatch_panel_layout(LayoutAction::Apply {
                expected_revision: None,
                mutations: vec![Mutation::SetFloatingRect {
                    section: section_id.clone(),
                    origin: Point { x: 1000.0, y: 0.0 },
                    size: Size { width: 280.0, height: 240.0 },
                }],
            })
            .unwrap();
        h.frames(3);
        let observations = inspect_host_specimen(&mut h.app).unwrap();
        assert_panel_host_preserves_design(&mut h, |h| {
            let before = h.app.panel_runtime().unwrap().layout().inspect();
            let layout = &h.app.universal_panel_frame().unwrap().layout;
            let gap = crate::ui_kit::Container::new(&h.ctx, crate::ui_kit::Surface::Panel).gap();
            assert_eq!(layout.capacity.size.width, layout.render_domain.size.width - 2.0 * gap);
            let outer = layout.sections.iter().find(|section| section.section == section_id).unwrap().outer;
            assert_eq!(outer.origin.x, layout.capacity.origin.x + layout.capacity.size.width - 280.0 - layout.allocation_environment.scroll.x);
            assert_eq!(outer.size, Size { width: 280.0, height: 240.0 });
            let expected = solidcraft_engine::panels::LogicalRect { origin: outer.origin, size: Size { width: 250.0, height: 240.0 } };
            let start = panel_host_point(h, HostPointerTarget::Resize { section: section_id.clone(), edge: HostResizeEdge::BottomRight });
            h.frame_with(vec![egui::Event::PointerMoved(start)]);
            panel_host_pointer(h, start, true);
            let end = start - egui::vec2(30.0, 0.0);
            h.frame_with(vec![egui::Event::PointerMoved(end)]);
            let preview = h.app.universal_panel_frame().unwrap().preview.as_ref().unwrap().clone();
            assert_eq!(preview.bounds, expected);
            assert!(preview.rejection.is_none());
            assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), before);
            panel_host_pointer(h, end, false);
            let actions = h
                .app
                .universal_panel_frame()
                .unwrap()
                .events
                .iter()
                .filter_map(|event| if let HostEvent::PreparedCommitted { reply } = event { Some(reply) } else { None })
                .collect::<Vec<_>>();
            assert_eq!(actions.len(), 1);
            assert_eq!(actions[0].revision, before.revision + 1);
            h.frames(2);
            assert_eq!(
                h.app.universal_panel_frame().unwrap().layout.sections.iter().find(|section| section.section == section_id).unwrap().outer,
                expected
            );
            assert_eq!(h.app.panel_runtime().unwrap().layout().inspect().revision, before.revision + 1);
        });
        let after = inspect_host_specimen(&mut h.app).unwrap();
        assert_eq!(after["runtime"]["controller_count"], observations["runtime"]["controller_count"]);
        assert_eq!(after["runtime"]["probe"]["content_command_calls"], 0);
        for before in observations["runtime"]["probe"]["controllers"].as_array().unwrap() {
            let retained = after["runtime"]["probe"]["controllers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|controller| controller["instance"] == before["instance"] && controller["owner"] == before["owner"])
                .unwrap();
            for field in ["controller_identity", "draft", "transaction_live", "reparents", "applies", "cancels", "finishes", "destroys"] {
                assert_eq!(retained[field], before[field], "rect-only application resize must retain its controller/provider state: {field}");
            }
        }
    }
}

#[test]
fn actual_host_raw_pointer_threshold_preview_escape_and_drop_preserve_the_design_at_both_scales() {
    use crate::panel_runtime::HostPointerTarget;
    use solidcraft_engine::panels::{InstanceId, Zone};
    for scale in [1.0, 2.0] {
        for zone in [Zone::Left, Zone::Right, Zone::Bottom] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
            h.call("ui.resize", json!({"width":1280,"height":800}));
            h.ctx.set_pixels_per_point(scale);
            crate::panel_runtime::enable_host_specimen(&mut h.app, "docks").unwrap();
            h.frames(4);
            assert_panel_host_preserves_design(&mut h, |h| {
                let before = h.app.panel_runtime().unwrap().layout().inspect();
                let projection = h.app.viewport.rect;
                let target = HostPointerTarget::Header { instance: InstanceId::new("specimen-0").unwrap() };
                let start = panel_host_point(h, target.clone());
                panel_host_pointer(h, start, true);
                h.frame_with(vec![egui::Event::PointerMoved(start + egui::vec2(5.99, 0.0))]);
                assert!(h.app.universal_panel_frame().unwrap().preview.is_none());
                h.frame_with(vec![egui::Event::PointerMoved(start + egui::vec2(6.0, 0.0))]);
                assert!(h.app.universal_panel_frame().unwrap().preview.is_some(), "six logical points, including 12 physical pixels at 200%");
                assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), before);
                assert_eq!(h.app.viewport.rect, projection, "preview must not move camera projection");
                panel_host_key(h, egui::Key::Escape);
                panel_host_pointer(h, start, false);
                assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), before);
                assert!(h.app.universal_panel_frame().unwrap().preview.is_none());
                let capacity = h.app.universal_panel_frame().unwrap().layout.capacity;
                let x0 = capacity.origin.x;
                let y0 = capacity.origin.y;
                let w = capacity.size.width;
                let height = capacity.size.height;
                let drop = match zone {
                    Zone::Left => egui::pos2(x0 + 1.0, y0 + height * 0.5),
                    Zone::Right => egui::pos2(x0 + w - 1.0, y0 + height * 0.5),
                    Zone::Bottom => egui::pos2(x0 + w * 0.5, y0 + height - 1.0),
                    _ => egui::pos2(x0 + w * 0.5, y0 + height * 0.5),
                };
                let start = panel_host_point(h, target);
                panel_host_pointer(h, start, true);
                h.frame_with(vec![egui::Event::PointerMoved(start + egui::vec2(8.0, 0.0))]);
                h.frame_with(vec![egui::Event::PointerMoved(drop)]);
                let landing = h.app.universal_panel_frame().unwrap().preview.as_ref().unwrap();
                assert!(landing.rejection.is_none());
                let projected = landing.projected.clone().expect("explicit target resolves final affected geometry");
                assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), before);
                panel_host_pointer(h, drop, false);
                h.frames(3);
                let reply = h.app.panel_runtime().unwrap().layout().inspect();
                assert_eq!(reply.revision, before.revision + 1, "one release commits once at scale {scale}: {zone:?}");
                assert_eq!(reply.layout.containing(&InstanceId::new("specimen-0").unwrap()).unwrap().location.zone(), zone);
                assert_eq!(h.app.universal_panel_frame().unwrap().reply, reply, "published frame must describe committed state");
                assert_panel_host_landing(h, &projected);
            });
        }
    }
}

#[test]
fn host_full_canvas_and_same_pass_controls_follow_actual_sizing_footprints_without_moving_the_model() {
    use crate::panel_runtime::{enable_host_specimen, logical_rect};
    use solidcraft_engine::panels::{LayoutAction, Mutation, Point, Size};
    for scale in [1.0, 2.0] {
        for mode in [LayoutMode::Docked, LayoutMode::Floating] {
            for preset in ["docks", "floating", "collapsed"] {
                let mut h = Harness::new();
                h.app.home.open = false;
                h.app.ui.layout = mode;
                h.call("ui.resize", json!({"width":1400,"height":1000}));
                h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
                h.settle();
                h.ctx.set_pixels_per_point(scale);
                h.ctx.options_mut(|options| options.max_passes = 1.try_into().unwrap());
                h.app.preferences.navigation_bar.position = match preset {
                    "docks" => crate::navigation_bar::Position::Centre,
                    "floating" => crate::navigation_bar::Position::Left,
                    _ => crate::navigation_bar::Position::Right,
                };
                let camera = h.app.cam;
                enable_host_specimen(&mut h.app, preset).unwrap();
                warm_empty_universal_canvas_before_cold_cards(&mut h);
                if preset == "floating" {
                    let sections =
                        h.app.panel_runtime().unwrap().layout().layout().sections.iter().map(|section| section.id.clone()).collect::<Vec<_>>();
                    let origins =
                        [Point { x: 500.0, y: 300.0 }, Point { x: 40.0, y: 220.0 }, Point { x: 40.0, y: 560.0 }, Point { x: 400.0, y: 650.0 }];
                    h.app
                        .dispatch_panel_layout(LayoutAction::Apply {
                            expected_revision: None,
                            mutations: sections
                                .into_iter()
                                .zip(origins)
                                .map(|(section, origin)| Mutation::SetFloatingRect { section, origin, size: Size { width: 280.0, height: 240.0 } })
                                .collect(),
                        })
                        .unwrap();
                }
                let pointer = egui::pos2(640.0, 420.0);
                let mut first_projection = None;
                for visible_pass in [false, true] {
                    h.frame_with(vec![egui::Event::PointerMoved(pointer)]);
                    let frame = h.app.universal_panel_frame().unwrap();
                    let domain = h.app.viewport.rect.unwrap();
                    assert_eq!(domain, egui::Rect::from_min_max(egui::pos2(0.0, toolbar::APP_BAR_HEIGHT), h.ctx.content_rect().max));
                    assert_eq!(frame.layout.render_domain, logical_rect(domain));
                    assert_eq!(frame.layout.viewport, Some(logical_rect(domain)));
                    assert_eq!(h.app.cam, camera);
                    let model_points = [solidcore_record::Vec3::ZERO, solidcore_record::Vec3::new(40.0, 30.0, 20.0)]
                        .map(|point| crate::viewport::projection(&h.app, domain).to_screen(point));
                    if let Some(first) = first_projection {
                        assert_eq!(model_points, first, "the newly visible panel cannot shift model pixels or framing");
                    } else {
                        first_projection = Some(model_points);
                    }
                    let controls = h.app.viewport.layout.as_ref().unwrap();
                    assert_eq!(controls.viewport, domain);
                    for (name, bounds) in [("nav:outer", controls.navigation), ("axis_view:outer", controls.axis_view)] {
                        if bounds.is_positive() {
                            assert_eq!(rect_of(name), Some(bounds), "the published control and final same-pass metadata agree");
                            assert!(frame.layout.usable.contains_rect(logical_rect(bounds)));
                            assert!(frame.layout.usable.obstacles.iter().all(|obstacle| {
                                !bounds
                                    .expand(crate::theme::Tokens::get().space_4)
                                    .intersect(crate::panel_runtime::egui_rect(*obstacle))
                                    .is_positive()
                            }));
                        } else {
                            assert!(rect_of(name).is_none());
                        }
                    }
                    assert!(controls.navigation.is_positive(), "the measured navbar remains accessible in ample usable space");
                    assert!(!controls.navigation.intersect(controls.axis_view).is_positive());
                    if preset == "floating" {
                        let inside = Point { x: pointer.x, y: pointer.y };
                        assert_eq!(frame.layout.consumes_pointer(inside), visible_pass);
                        assert_eq!(
                            h.app.viewport.mouse,
                            if visible_pass { None } else { Some(pointer) },
                            "first invisible sizing reaches navigation; the next actual card shields it"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn first_invisible_host_sizing_pass_allows_real_wheel_navigation_and_the_visible_card_blocks_it() {
    use solidcraft_engine::panels::{LayoutAction, Mutation, Point, Size};
    for scale in [1.0, 2.0] {
        for mode in [LayoutMode::Docked, LayoutMode::Floating] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.ui.layout = mode;
            h.call("ui.resize", json!({"width":1400,"height":1000}));
            h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
            h.settle();
            h.ctx.set_pixels_per_point(scale);
            h.ctx.options_mut(|options| options.max_passes = 1.try_into().unwrap());
            crate::panel_runtime::enable_host_specimen(&mut h.app, "floating").unwrap();
            warm_empty_universal_canvas_before_cold_cards(&mut h);
            let sections = h.app.panel_runtime().unwrap().layout().layout().sections.iter().map(|section| section.id.clone()).collect::<Vec<_>>();
            let origins = [Point { x: 500.0, y: 300.0 }, Point { x: 40.0, y: 220.0 }, Point { x: 40.0, y: 560.0 }, Point { x: 400.0, y: 650.0 }];
            h.app
                .dispatch_panel_layout(LayoutAction::Apply {
                    expected_revision: None,
                    mutations: sections
                        .into_iter()
                        .zip(origins)
                        .map(|(section, origin)| Mutation::SetFloatingRect { section, origin, size: Size { width: 280.0, height: 240.0 } })
                        .collect(),
                })
                .unwrap();
            let at = egui::pos2(640.0, 420.0);
            let wheel = || {
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::MouseWheel {
                        source: egui::MouseWheelSource::Unknown,
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, 120.0),
                        modifiers: egui::Modifiers::NONE,
                        phase: egui::TouchPhase::Move,
                    },
                ]
            };
            let before = h.app.cam;
            h.frame_with(wheel());
            assert!(!h.app.universal_panel_frame().unwrap().layout.consumes_pointer(Point { x: at.x, y: at.y }));
            assert_eq!(h.app.viewport.mouse, Some(at));
            assert_ne!(h.app.cam.distance.to_bits(), before.distance.to_bits(), "the real first-pass wheel reaches the existing navigation path");
            let navigated = h.app.cam;
            h.frame_with(wheel());
            assert!(h.app.universal_panel_frame().unwrap().layout.consumes_pointer(Point { x: at.x, y: at.y }));
            assert!(h.app.viewport.mouse.is_none());
            assert_eq!(h.app.cam, navigated, "an actual visible card shields the next wheel without moving projection");
            h.frames(3);
            assert_eq!(h.app.cam, navigated, "blocked input cannot leave delayed navigation behind");
        }
    }
}

#[test]
fn actual_host_fit_and_centring_respect_the_measured_sketch_palette_at_both_scales() {
    use crate::panel_runtime::{HostControlTarget, HostPointerTarget, egui_rect, enable_host_specimen, logical_rect};
    use solidcore_record::Vec3;
    use solidcraft_engine::geom::{Aabb3, Vec3Geom as _};
    use solidcraft_engine::panels::{InstanceId, LayoutAction, Mutation};
    fn projected_extent(h: &Harness, bounds: Aabb3) -> egui::Rect {
        let domain = h.app.viewport.rect.unwrap();
        let projection = crate::viewport::projection(&h.app, domain);
        let mut extent = egui::Rect::NOTHING;
        for x in [bounds.min.x, bounds.max.x] {
            for y in [bounds.min.y, bounds.max.y] {
                for z in [bounds.min.z, bounds.max.z] {
                    extent.extend_with(projection.to_screen(Vec3::new(x, y, z)).expect("every actual sketch-bound corner must project"));
                }
            }
        }
        extent
    }
    fn assert_palette_and_controls(h: &Harness, palette: egui::Rect) {
        let frame = h.app.universal_panel_frame().unwrap();
        let layout = h.app.viewport.layout.as_ref().unwrap();
        let usable = layout.usable.as_ref().unwrap();
        let effective = crate::shell::host_usable(&h.app).unwrap();
        assert_eq!(rect_of("sketch_palette:outer"), Some(palette));
        assert!(crate::shell::fit_rects(&h.app).contains(&palette), "the palette must be an actual persistent footprint");
        assert!(usable.obstacles.contains(&logical_rect(palette)));
        assert!(usable.free.iter().all(|piece| !egui_rect(*piece).intersect(palette).is_positive()));
        for (name, control) in [("nav:outer", layout.navigation), ("axis_view:outer", layout.axis_view)] {
            if control.is_positive() {
                assert_eq!(rect_of(name), Some(control), "published controls must match the actual same-pass footprint");
                assert!(!control.expand(crate::theme::Tokens::get().space_4).intersect(palette).is_positive());
                assert!(effective.contains_rect(logical_rect(control)));
                assert!(frame.layout.usable.obstacles.iter().all(|obstacle| !control.intersect(egui_rect(*obstacle)).is_positive()));
            } else {
                assert!(rect_of(name).is_none());
            }
        }
    }
    for scale in [1.0, 2.0] {
        for mode in [LayoutMode::Docked, LayoutMode::Floating] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.ui.layout = mode;
            h.app.preferences.look_at_anim = false;
            h.app.preferences.navigator_fit = false;
            h.app.preferences.home_fit_size = 1.0;
            h.call("ui.resize", json!({"width":1400,"height":1000}));
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            h.app.run("sketch.line", json!({"points":[[0,0],[100,0]],"infer":false,"connect":false})).unwrap();
            h.settle();
            h.ctx.set_pixels_per_point(scale);
            enable_host_specimen(&mut h.app, "docks").unwrap();
            h.frames(4);
            let sketch = h.app.session.active_sketch.expect("the real sketch palette requires an active sketch");
            let palette = h.app.viewport.palette_rect.expect("the actual sketch palette must paint beside the host");
            let domain = h.app.viewport.rect.unwrap();
            assert_eq!(domain, egui::Rect::from_min_max(egui::pos2(0.0, toolbar::APP_BAR_HEIGHT), h.ctx.content_rect().max));
            assert!(domain.contains_rect(palette));
            assert!(
                h.app.universal_panel_frame().unwrap().layout.usable.free.iter().any(|piece| egui_rect(*piece).intersect(palette).is_positive()),
                "the palette must cover space beyond host-only footprints, so this exercises the missing union"
            );
            assert_palette_and_controls(&h, palette);
            let bounds = crate::viewport::view_bounds(&h.app);
            assert!((bounds.max.x - bounds.min.x - 100.0).abs() < 1e-8);
            h.app.fit_view();
            h.settle();
            assert_eq!(h.app.viewport.rect, Some(domain), "explicit fitting preserves the full render domain");
            let footprint = projected_extent(&h, bounds);
            let padded = footprint.expand(crate::theme::Tokens::get().space_8 - 0.01);
            assert!(
                h.app.viewport.layout.as_ref().unwrap().usable.as_ref().unwrap().contains_rect(logical_rect(padded)),
                "{scale}x {mode:?}: the full 100 mm line and its clearance must avoid the palette, host and controls: {padded:?}"
            );
            for endpoint in [Vec3::ZERO, Vec3::new(100.0, 0.0, 0.0)] {
                let point = crate::viewport::projection(&h.app, domain).to_screen(endpoint).unwrap();
                assert!(domain.contains(point) && !palette.contains(point));
            }
            let fitted = h.app.cam;
            let contour = h.app.viewport.layout.as_ref().unwrap().usable.clone();
            let editor = InstanceId::new("specimen-0").unwrap();
            panel_host_semantic_click(&mut h, HostControlTarget::Pointer(HostPointerTarget::Menu { instance: editor.clone() }));
            assert!(h.app.universal_panel_frame().unwrap().menu_bounds.is_some());
            assert_eq!(h.app.viewport.layout.as_ref().unwrap().usable, contour, "transient host menus cannot change the fitting/control contour");
            h.app.fit_view();
            h.settle();
            assert_eq!(h.app.cam, fitted);
            panel_host_key(&mut h, egui::Key::Escape);

            h.app.cam.distance = 2.0;
            h.app.cam.yaw = 1.0;
            h.app.cam.pitch = 0.2;
            let distance = h.app.cam.distance.to_bits();
            let lens = h.app.cam.fov.to_bits();
            let look_at = crate::scenario::handle_at("sketch_palette:look_at").unwrap();
            assert!(palette.contains(look_at));
            panel_host_click(&mut h, look_at);
            h.settle();
            assert_eq!(h.app.cam.distance.to_bits(), distance, "explicit palette Look At must not zoom out to fit an oversized line");
            assert_eq!(h.app.cam.fov.to_bits(), lens);
            assert!(h.app.cam.back().dist(Vec3::Z) < 1e-6);
            let focus = crate::viewport::projection(&h.app, domain).to_screen(Vec3::new(50.0, 0.0, 0.0)).unwrap();
            assert!(h.app.viewport.layout.as_ref().unwrap().usable.as_ref().unwrap().free.iter().any(|piece| egui_rect(*piece).contains(focus)));
            h.app.run("select.set", json!({"items":[{"type":"plane","name":"XY"}]})).unwrap();
            let selection = h.app.session.selection.clone();
            h.app.start("view.normal_to");
            h.settle();
            assert_eq!(h.app.cam.distance.to_bits(), distance);
            assert_eq!(h.app.cam.fov.to_bits(), lens);
            assert!(h.app.cam.back().dist(-Vec3::Z) < 1e-6, "normal-to retains its existing second-invocation flip semantics");
            assert_eq!(h.app.session.selection, selection);
            assert_eq!(h.app.viewport.rect, Some(domain));
            let focus = crate::viewport::projection(&h.app, domain).to_screen(Vec3::ZERO).unwrap();
            assert!(h.app.viewport.layout.as_ref().unwrap().usable.as_ref().unwrap().free.iter().any(|piece| egui_rect(*piece).contains(focus)));
            assert_panel_host_preserves_design(&mut h, |h| {
                let section = h.app.panel_runtime().unwrap().layout().layout().containing(&editor).unwrap().id.clone();
                h.app
                    .dispatch_panel_layout(LayoutAction::Apply {
                        expected_revision: None,
                        mutations: vec![Mutation::Collapse { section, collapsed: true }],
                    })
                    .unwrap();
                h.frames(3);
                assert_eq!(h.app.session.active_sketch, Some(sketch));
            });
            assert_palette_and_controls(&h, h.app.viewport.palette_rect.unwrap());
            h.app.run("sketch.finish", json!({})).unwrap();
            h.settle();
            assert!(h.app.viewport.palette_rect.is_none());
            assert!(rect_of("sketch_palette:outer").is_none());
            assert!(
                !h.app.viewport.layout.as_ref().unwrap().usable.as_ref().unwrap().obstacles.contains(&logical_rect(palette)),
                "a closed palette cannot leave a phantom reservation"
            );
        }
    }
}

#[test]
fn actual_host_full_header_restore_preserves_weights_and_leaves_gaps_for_navigation() {
    use crate::panel_runtime::HostPointerTarget;
    use solidcraft_engine::panels::{InstanceId, Point};
    for scale in [1.0, 2.0] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.ctx.set_pixels_per_point(scale);
        crate::panel_runtime::enable_host_specimen(&mut h.app, "collapsed").unwrap();
        h.frames(4);
        let desired = h.app.panel_runtime().unwrap().layout().layout().clone();
        let camera = serde_json::to_value(&h.app.cam).unwrap();
        for index in [0, 1] {
            let instance = InstanceId::new(format!("specimen-{index}")).unwrap();
            let at = panel_host_point(&h, HostPointerTarget::Collapse { instance: instance.clone() });
            panel_host_click(&mut h, at);
            h.frames(2);
            assert!(!h.app.panel_runtime().unwrap().layout().layout().containing(&instance).unwrap().collapsed);
            assert!(
                h.app.universal_panel_frame().unwrap().layout.sections.iter().find(|section| section.instance == instance).unwrap().body.is_some()
            );
            assert!(h.app.universal_panel_frame().unwrap().layout.resolved.rails.is_empty());
            let at = panel_host_point(&h, HostPointerTarget::Collapse { instance });
            panel_host_click(&mut h, at);
            h.frames(2);
        }
        assert_eq!(h.app.panel_runtime().unwrap().layout().layout(), &desired);
        assert_eq!(serde_json::to_value(&h.app.cam).unwrap(), camera);
        let frame = h.app.universal_panel_frame().unwrap();
        let left = frame.layout.sections.iter().find(|section| section.instance.0 == "specimen-0").unwrap();
        let outside = egui::pos2(left.outer.origin.x + left.outer.size.width * 0.5, frame.layout.capacity.origin.y + 160.0);
        assert!(!frame.layout.consumes_pointer(Point { x: outside.x, y: outside.y }));
        h.frame_with(vec![egui::Event::PointerMoved(outside)]);
        assert_eq!(h.app.viewport.mouse, Some(outside), "uncovered gaps reach the existing navigation path");
        assert_eq!(serde_json::to_value(&h.app.cam).unwrap(), camera);
    }
}
#[test]
fn actual_host_document_switch_and_lost_pointer_cancel_pending_drag() {
    use crate::panel_runtime::HostPointerTarget;
    use solidcraft_engine::panels::InstanceId;
    for switch_document in [false, true] {
        let mut h = Harness::new();
        h.app.home.open = false;
        crate::panel_runtime::enable_host_specimen(&mut h.app, "docks").unwrap();
        h.frames(3);
        let before = h.app.panel_runtime().unwrap().layout().layout().clone();
        let start = panel_host_point(&h, HostPointerTarget::Header { instance: InstanceId::new("specimen-0").unwrap() });
        panel_host_pointer(&mut h, start, true);
        let drop = start + egui::vec2(500.0, 200.0);
        h.frame_with(vec![egui::Event::PointerMoved(drop)]);
        assert!(h.app.universal_panel_frame().unwrap().preview.is_some());
        if switch_document {
            crate::documents::open_tab(&mut h.app, solidcraft_engine::Session::default()).unwrap();
            h.app.home.open = false;
            h.frames(2);
        } else {
            h.frame_with(vec![egui::Event::PointerGone]);
        }
        panel_host_pointer(&mut h, drop, false);
        h.frames(2);
        assert!(h.app.universal_panel_frame().unwrap().preview.is_none());
        assert_eq!(h.app.panel_runtime().unwrap().layout().layout(), &before);
    }
}

#[derive(Default)]
struct ShellPanelServices {
    commands: crate::panel_runtime::FakeContentCommands,
    local: crate::panel_runtime::FakeLocalServices,
    models: crate::panel_runtime::FakeModels,
}
impl crate::panel_runtime::AppPanelServices for ShellPanelServices {
    fn refresh_read_only(&mut self, _: &solidcraft_engine::Session) -> solidcraft_engine::panels::PanelResult<()> {
        Ok(())
    }
    fn services(&mut self) -> crate::panel_runtime::ContentServices<'_> {
        crate::panel_runtime::ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local }
    }
}

#[test]
fn enabling_generic_runtime_alone_keeps_legacy_workspace_placement_until_host_enablement() {
    use crate::panel_runtime::*;
    use solidcraft_engine::panels::*;
    let mut h = Harness::new();
    h.app.home.open = false;
    h.frames(3);
    let legacy_browser = rect_of("browser:card").unwrap();
    assert_panel_host_preserves_design(&mut h, |h| {
        let mut fixture = PanelMountHarness::new(false).unwrap();
        fixture.dispatch(vec![fixture_open("fixture.log", "log", "log", Zone::Left, 0).unwrap()]).unwrap();
        h.app.enable_panel_runtime(fixture.runtime, Box::<ShellPanelServices>::default()).unwrap();
        h.frames(3);
        assert!(!h.app.universal_panel_host_enabled());
        assert!(h.app.universal_panel_frame().is_none());
        assert_eq!(rect_of("browser:card"), Some(legacy_browser));
        let before = h.app.panel_runtime().unwrap().layout().inspect();
        let count = h.app.panel_runtime().unwrap().controller_count();
        let mut invalid = HostOptions::default();
        invalid.metrics.header_height = f32::NAN;
        assert!(h.app.enable_universal_panel_host(invalid).is_err());
        assert_eq!(h.app.panel_runtime().unwrap().layout().inspect(), before);
        assert_eq!(h.app.panel_runtime().unwrap().controller_count(), count);
        assert!(!h.app.universal_panel_host_enabled());
        h.app.enable_universal_panel_host(HostOptions::default()).unwrap();
        h.frames(3);
        assert!(rect_of("browser:card").is_none());
        h.app.disable_universal_panel_host().unwrap();
        h.frames(3);
        assert_eq!(rect_of("browser:card"), Some(legacy_browser));
        assert_eq!(h.app.panel_runtime().unwrap().controller_count(), count, "host lifetime does not recreate controllers");
    });
}

#[test]
fn universal_host_scenarios_drive_real_shared_actions_and_leave_the_fixture_disabled() {
    for (name, source) in [
        ("shared actions", include_str!("../tests/scenarios/universal_panel_host_shared_actions.json")),
        ("collapsed", include_str!("../tests/scenarios/universal_panel_host_collapsed.json")),
        ("columns and inline", include_str!("../tests/scenarios/universal_panel_host_columns_inline.json")),
        ("overflow and scale", include_str!("../tests/scenarios/universal_panel_host_overflow_scale.json")),
    ] {
        let mut h = Harness::new();
        h.app.home.open = false;
        let before = h.app.session.doc.clone();
        let steps: serde_json::Value = serde_json::from_str(source).unwrap();
        for (index, step) in steps.as_array().unwrap().iter().enumerate() {
            let action_before =
                (step["params"]["command"] == "panel.action").then(|| crate::panel_runtime::inspect_host_specimen(&mut h.app).unwrap());
            h.step(step).unwrap_or_else(|reason| panic!("{name}: step {index}: {reason}"));
            if let Some(action_before) = action_before {
                let after = crate::panel_runtime::inspect_host_specimen(&mut h.app).unwrap();
                let applies = |value: &serde_json::Value| {
                    value["runtime"]["probe"]["controllers"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|controller| controller["instance"] == "specimen-0")
                        .unwrap()["applies"]
                        .as_u64()
                        .unwrap()
                };
                assert_eq!(applies(&after), applies(&action_before) + 1, "the scenario must invoke the editor's Apply binding");
                assert_eq!(after["session"], action_before["session"], "synthetic Apply leaves the Session unchanged");
            }
        }
        assert!(h.app.panel_runtime().is_none());
        assert!(!h.app.universal_panel_host_enabled());
        assert!(std::sync::Arc::ptr_eq(&before, &h.app.session.doc));
        assert_eq!(h.app.session.undo.len(), 0);
        assert!(rect_of("browser:card").is_some());
    }
}

#[test]
fn actual_host_resize_reset_header_collapse_restore_and_tab_join_preserve_document_cache_and_camera() {
    use crate::panel_runtime::{HostControlTarget, HostPointerTarget, HostSharedAction};
    use solidcraft_engine::panels::InstanceId;
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    h.settle();
    crate::panel_runtime::enable_host_specimen(&mut h.app, "docks").unwrap();
    h.frames(3);
    assert_panel_host_preserves_design(&mut h, |h| {
        let sash = h
            .app
            .universal_panel_frame()
            .unwrap()
            .layout
            .controls
            .iter()
            .find(|control| matches!(control.target, HostPointerTarget::Sash { .. }))
            .unwrap()
            .target
            .clone();
        let start = panel_host_point(h, sash.clone());
        panel_host_pointer(h, start, true);
        h.frame_with(vec![egui::Event::PointerMoved(start + egui::vec2(0.0, 30.0))]);
        panel_host_pointer(h, start + egui::vec2(0.0, 30.0), false);
        h.frames(2);
        let at = panel_host_point(h, sash);
        panel_host_click(h, at);
        panel_host_click(h, at);
        h.frames(2);
        let editor = InstanceId::new("specimen-0").unwrap();
        panel_host_semantic_click(h, HostControlTarget::Pointer(HostPointerTarget::Menu { instance: editor.clone() }));
        panel_host_semantic_click(h, HostControlTarget::Shared { instance: editor.clone(), action: HostSharedAction::Collapse });
        panel_host_semantic_click(h, HostControlTarget::Pointer(HostPointerTarget::Collapse { instance: editor.clone() }));
        assert!(h.app.panel_runtime().unwrap().layout().interaction().peek.is_none());
        assert!(!h.app.panel_runtime().unwrap().layout().layout().containing(&editor).unwrap().collapsed);
        let start = panel_host_point(h, HostPointerTarget::Header { instance: editor.clone() });
        let destination = h.app.panel_runtime().unwrap().layout().layout().containing(&InstanceId::new("specimen-1").unwrap()).unwrap();
        let destination_id = destination.id.clone();
        let insertion = destination.tabs.len();
        let end = panel_host_drop_point(h, &editor, &crate::panel_runtime::HostDropTargetId::Tab { section: destination_id.clone(), before: None });
        panel_host_pointer(h, start, true);
        h.frame_with(vec![egui::Event::PointerMoved(end)]);
        let shown = h.app.universal_panel_frame().unwrap().preview.as_ref().unwrap();
        assert!(shown.rejection.is_none());
        let Some(solidcraft_engine::panels::LayoutAction::Apply { mutations, .. }) = shown.action.as_ref() else {
            panic!("the actual tab append must show a prepared atomic landing");
        };
        assert_eq!(
            mutations,
            &vec![
                solidcraft_engine::panels::Mutation::Move {
                    instance: editor.clone(),
                    destination: solidcraft_engine::panels::Destination::JoinTab { section: destination_id.clone(), index: insertion },
                },
                solidcraft_engine::panels::Mutation::Activate { instance: editor.clone() },
            ]
        );
        let projected = shown.projected.clone().unwrap();
        panel_host_pointer(h, end, false);
        h.frames(2);
        let joined = h.app.panel_runtime().unwrap().layout().layout().containing(&editor).unwrap();
        assert_eq!(joined.tabs.len(), 2);
        assert_eq!(joined.id, destination_id);
        assert_eq!(joined.tabs[insertion], editor);
        assert_eq!(joined.active, editor);
        assert_panel_host_landing(h, &projected);
    });
}

#[test]
fn modes_are_transient_and_available_to_control_and_scenarios() {
    let mut h = Harness::new();
    assert_eq!(h.app.ui.layout, LayoutMode::Floating);
    let prefs = serde_json::to_value(&h.app.preferences).unwrap();
    let doc = h.app.session.doc.to_json();
    h.step(&json!({"call":"ui.set", "params":{"layout":"docked"}})).unwrap();
    assert_eq!(h.app.ui.layout, LayoutMode::Docked);
    h.step(&json!({"call":"ui.set", "params":{"layout":"invalid"},"fail":true})).unwrap();
    assert_eq!(h.app.ui.layout, LayoutMode::Docked);
    h.call("ui.set", json!({"layout":"floating"}));
    assert_eq!(h.app.ui.layout, LayoutMode::Floating);
    assert_eq!(h.app.session.doc.to_json(), doc);
    assert_eq!(serde_json::to_value(&h.app.preferences).unwrap(), prefs);
}

#[test]
fn both_modes_keep_their_geometry_and_fit_clear_of_covered_rects() {
    for mode in [LayoutMode::Docked, LayoutMode::Floating] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":900}));
        h.call("ui.set", json!({"layout":mode}));
        h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
        h.settle();
        let viewport = h.app.viewport.rect.unwrap();
        let workspace = rect_of("ribbon:workspace").unwrap();
        let toolbar = rect_of("toolbar").unwrap();
        if mode == LayoutMode::Floating {
            let ribbon = rect_of("ribbon:card").unwrap();
            assert_eq!(viewport.left(), 0.0);
            assert_eq!(viewport.top(), toolbar::APP_BAR_HEIGHT);
            assert_eq!(workspace.left(), SHELL_GAP);
            assert_eq!(workspace.top(), viewport.top() + SHELL_GAP);
            assert_eq!(ribbon.left() - workspace.right(), SHELL_GAP);
            assert_eq!(ribbon.right(), viewport.right() - SHELL_GAP);
            assert!(!covered(&h.app, pos2(4.0, workspace.center().y)));
            assert!(!covered(&h.app, pos2(workspace.right() + 2.0, workspace.center().y)));
            assert!(covered(&h.app, workspace.center()));
            assert!(covered(&h.app, ribbon.center()));
        } else {
            assert_eq!(toolbar.top(), toolbar::APP_BAR_HEIGHT);
            assert_eq!(viewport.top(), toolbar.bottom());
            assert_eq!(toolbar.height(), toolbar::RIBBON_HEIGHT);
            assert_eq!(workspace.left(), 0.0);
            assert_eq!(covered_rects(&h.app), vec![h.app.shell.history.unwrap(), rect_of("browser:card").unwrap()]);
        }
        h.app.fit_view();
        h.settle();
        let layout = h.app.viewport.layout.as_ref().unwrap();
        for covered in covered_rects(&h.app) {
            assert!(!layout.drawable.intersect(covered).is_positive(), "{layout:?} {covered:?}");
        }
        let projection = crate::viewport::projection(&h.app, viewport);
        for point in [solidcore_record::Vec3::ZERO, solidcore_record::Vec3::new(40.0, 30.0, 20.0)] {
            assert!(layout.drawable.contains(projection.to_screen(point).unwrap()));
        }
        h.app.start("solid.extrude");
        h.settle();
        if mode == LayoutMode::Floating {
            assert!(h.app.viewport.dialog_rect.unwrap().top() >= h.app.viewport.layout.as_ref().unwrap().axis_view.bottom());
        }
    }
}

#[test]
fn cards_block_model_hover_wheel_and_drag_but_the_gap_remains_viewport() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    h.settle();
    let ribbon = rect_of("ribbon:card").unwrap();
    // Blank space at the right, away from the search and command buttons.
    let at = pos2(ribbon.right() - 12.0, ribbon.bottom() - 28.0);
    let camera = h.app.cam;
    let doc = h.app.session.doc.to_json();
    h.call("ui.move", json!({"x":at.x,"y":at.y}));
    assert!(h.app.viewport.hover.is_none());
    assert!(h.app.viewport.mouse.is_none());
    h.call("ui.scroll", json!({"x":at.x,"y":at.y,"delta":120}));
    h.call("ui.drag", json!({"x0":at.x,"y0":at.y,"x1":at.x-30.0,"y1":at.y+15.0,"button":"middle","steps":6}));
    assert_eq!(h.app.cam, camera);
    assert!(h.app.viewport.boxsel.is_none());
    assert_eq!(h.app.session.doc.to_json(), doc);
    let gap = pos2(4.0, ribbon.center().y);
    h.call("ui.move", json!({"x":gap.x,"y":gap.y}));
    assert_eq!(h.app.viewport.mouse, Some(gap));
}

#[test]
fn floating_sketch_palette_and_compass_share_measured_clearance() {
    for style in crate::prefs::NavigatorStyle::ALL {
        for (width, height, scale) in [(1400.0, 900.0, 1.0), (700.0, 720.0, 2.0)] {
            let mut h = Harness::new();
            h.app.preferences.navigator_style = style;
            h.call("ui.resize", json!({"width":width,"height":height}));
            h.call("ui.prefs", json!({"ui_scale":scale}));
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            h.app.run("sketch.line", json!({"points":[[0,0],[20,0]]})).unwrap();
            h.settle();
            h.app.fit_view();
            h.settle();
            let layout = h.app.viewport.layout.as_ref().unwrap();
            let palette = h.app.viewport.palette_rect.unwrap();
            assert!(palette.top() >= layout.axis_view.bottom(), "{style:?} {palette:?} {layout:?}");
            assert!(layout.viewport.expand(1.0).contains_rect(palette), "{palette:?} {layout:?}");
            assert!(palette.contains(crate::scenario::handle_at("sketch_palette:look_at").unwrap()));
            let proj = crate::viewport::projection(&h.app, layout.viewport);
            for point in [solidcore_record::Vec3::ZERO, solidcore_record::Vec3::new(20.0, 0.0, 0.0)] {
                assert!(layout.drawable.contains(proj.to_screen(point).unwrap()), "{layout:?}");
            }
            if width > 1000.0 {
                let content = layout.content;
                h.app.start("construct.plane.offset");
                h.settle();
                let dialog = h.app.viewport.dialog_rect.unwrap();
                let expected_top = (content.top() + crate::viewport::AXIS_VIEW_CLEARANCE).min((content.bottom() - 360.0).max(content.top()));
                assert!((dialog.left() - content.left()).abs() < 1.0);
                assert!((dialog.top() - expected_top).abs() < 1.0, "left dialogs retain their existing top: {dialog:?}");
            }
        }
    }
}

#[test]
fn short_floating_palette_stays_in_viewport() {
    for style in crate::prefs::NavigatorStyle::ALL {
        for height in [475, 385, 300] {
            let mut h = Harness::new();
            h.app.preferences.navigator_style = style;
            h.call("ui.resize", json!({"width":700,"height":height}));
            h.call("ui.prefs", json!({"ui_scale":2.0}));
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            // Main's open/untied-end feedback also consumes footer space.
            for points in [[[0, 0], [10, 0]], [[10, 0], [10, 10]], [[10, 10], [0, 10]], [[0, 10], [0, 0]], [[20, 0], [30, 5]]] {
                h.app.run("sketch.line", json!({"points":points,"infer":false,"connect":false})).unwrap();
            }
            // Include the extra Feature Options section in the tiny-height case.
            if height == 300 {
                h.app.start("sketch.rectangle.two_point");
            }
            h.settle();
            let viewport = h.app.viewport.rect.unwrap();
            let palette = h.app.viewport.palette_rect.unwrap();
            assert!(
                viewport.contains_rect(palette),
                "{style:?}, height={height}: {palette:?} extends beyond {viewport:?}, right={:.9}, bottom={:.9}",
                palette.right(),
                palette.bottom()
            );
            let compass = h.app.viewport.layout.as_ref().unwrap().axis_view;
            if compass.is_positive() {
                assert!(palette.top() < compass.bottom(), "short panels reclaim compass space");
            }
            let action = crate::scenario::handle_at("sketch_palette:look_at").unwrap();
            if !palette.contains(action) {
                // Once reclaiming is insufficient, the whole palette scrolls to its footer.
                h.call("ui.scroll", json!({"x":palette.center().x,"y":palette.center().y,"delta":-1200}));
                h.frames(20);
            }
            let action = crate::scenario::handle_at("sketch_palette:look_at").unwrap();
            assert!(palette.contains(action), "footer must be reachable: {style:?}, height={height}, {action:?} in {palette:?}");
            let finish = crate::scenario::handle_at("sketch_palette:finish").unwrap();
            h.call("ui.click", json!({"x":finish.x,"y":finish.y}));
            assert!(h.app.session.active_sketch.is_none(), "Finish Sketch must work after scrolling");
        }
    }
}

#[test]
fn selection_drag_released_over_a_card_leaves_no_rubber_band() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1400,"height":900}));
    h.settle();
    let ribbon = rect_of("ribbon:card").unwrap();
    let viewport = h.app.viewport.rect.unwrap();
    h.call(
        "ui.drag",
        json!({"x0":viewport.right()-20.0,"y0":viewport.bottom()-100.0,
        "x1":ribbon.right()-12.0,"y1":ribbon.bottom()-28.0,"steps":12}),
    );
    assert!(h.app.viewport.boxsel.is_none());
    assert!(h.app.session.selection.is_empty());
}

#[test]
fn floating_browser_clears_cards_shields_input_and_can_dock_again() {
    for mode in [LayoutMode::Floating, LayoutMode::Docked] {
        for timeline_left in [false, true] {
            let mut h = Harness::new();
            h.call("ui.set", json!({"layout":mode}));
            h.app.preferences.timeline_left = timeline_left;
            h.app.preferences.browser_float = true;
            h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
            h.settle();
            let browser = rect_of("browser:float").unwrap();
            let toolbar = rect_of("toolbar").unwrap();
            assert!(browser.top() >= toolbar.bottom(), "{mode:?}: {browser:?} must clear {toolbar:?}");
            assert!(crate::scenario::handle_at("row:Untitled").is_some());
            assert!(crate::scenario::handle_at("left-browser-rail").is_some());
            assert!(covered(&h.app, browser.center()));
            let viewport = h.app.viewport.rect.unwrap();
            // A left-side history still takes a column in Docked mode.
            if mode == LayoutMode::Floating || !timeline_left {
                assert_eq!(viewport.left(), 0.0);
            } else {
                assert!(viewport.left() > 0.0);
            }
            if mode == LayoutMode::Floating {
                assert_eq!(viewport.top(), toolbar::APP_BAR_HEIGHT);
                assert!(!browser.intersect(rect_of("ribbon:card").unwrap()).is_positive());
            }
            h.app.fit_view();
            h.settle();
            assert!(!h.app.viewport.layout.as_ref().unwrap().drawable.intersect(browser).is_positive());
            let camera = h.app.cam;
            let doc = h.app.session.doc.to_json();
            let at = browser.center();
            h.call("ui.move", json!({"x":at.x,"y":at.y}));
            assert!(h.app.viewport.hover.is_none());
            assert!(h.app.viewport.mouse.is_none());
            h.call("ui.scroll", json!({"x":at.x,"y":at.y,"delta":120}));
            h.call("ui.drag", json!({"x0":at.x,"y0":at.y,"x1":at.x+20.0,"y1":at.y+10.0,"button":"middle","steps":6}));
            assert_eq!(h.app.cam, camera);
            assert_eq!(h.app.session.doc.to_json(), doc);
            // The browser remains interactive above the shielded viewport.
            h.step(&json!({"click":{"chevron":"Origin"}})).unwrap();
            assert!(crate::browser::drawn_rows().iter().any(|(name, _, _, open)| name == "Origin" && *open == Some(true)));
            h.app.preferences.browser_float = false;
            h.settle();
            assert!(h.app.shell.browser.is_some());
            assert!(rect_of("browser:float").is_none());
            assert!(rect_of("browser:card").is_some());
            // The saved width follows both the floating card and the docked column.
            let rail = crate::scenario::handle_at("left-browser-rail").unwrap();
            let width = h.app.preferences.browser_width;
            h.call("ui.drag", json!({"x0":rail.x,"y0":rail.y,"x1":rail.x+60.0,"y1":rail.y,"steps":6}));
            assert!(h.app.preferences.browser_width > width + 30.0);
        }
    }
}

#[test]
fn browser_and_history_cards_block_input_while_their_gaps_and_space_below_browser_do_not() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1400,"height":900}));
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    h.settle();
    let browser = rect_of("browser:card").unwrap();
    let history = rect_of("history:card").unwrap();
    let viewport = h.app.viewport.rect.unwrap();
    assert_eq!(history.left(), viewport.left() + SHELL_GAP);
    assert_eq!(history.right(), viewport.right() - SHELL_GAP);
    assert_eq!(history.bottom(), viewport.bottom() - SHELL_GAP);
    let layout = h.app.viewport.layout.as_ref().unwrap();
    assert!(layout.navigation.bottom() < history.top());
    assert!(layout.axis_view.top() >= rect_of("ribbon:card").unwrap().bottom());
    for r in [browser, history] {
        assert!(covered(&h.app, r.center()));
        assert!(!layout.drawable.intersect(r).is_positive());
    }
    for at in [pos2(browser.left() + 2.0, browser.top() + 2.0), pos2(history.right() - 30.0, history.center().y)] {
        let camera = h.app.cam;
        h.call("ui.move", json!({"x":at.x,"y":at.y}));
        assert!(h.app.viewport.mouse.is_none());
        assert!(h.app.viewport.hover.is_none());
        h.call("ui.scroll", json!({"x":at.x,"y":at.y,"delta":120}));
        h.call("ui.drag", json!({"x0":at.x,"y0":at.y,"x1":at.x+10.0,"y1":at.y+2.0,"button":"middle","steps":6}));
        assert_eq!(h.app.cam, camera);
    }
    for at in [pos2(3.0, browser.center().y), pos2(browser.center().x, browser.bottom() + SHELL_GAP), pos2(3.0, history.center().y)] {
        assert!(!covered(&h.app, at));
        h.call("ui.move", json!({"x":at.x,"y":at.y}));
        assert_eq!(h.app.viewport.mouse, Some(at));
    }
    h.app.ui.show_browser = false;
    h.app.ui.show_timeline = false;
    h.settle();
    assert!(h.app.shell.browser.is_none() && h.app.shell.history.is_none());
    assert!(!covered(&h.app, browser.center()));
    assert!(!covered(&h.app, history.center()));
}

#[test]
fn left_history_and_browser_share_the_left_edge_without_moving_right_controls() {
    for mode in [LayoutMode::Floating, LayoutMode::Docked] {
        let mut h = Harness::new();
        h.app.ui.layout = mode;
        h.app.preferences.timeline_left = true;
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"name":"First"})).unwrap();
        h.settle();
        assert!(rect_of("history:card").is_none());
        let history = rect_of("history:vertical").unwrap();
        let browser = rect_of("browser:card").unwrap();
        assert!((browser.left() - history.right() - Container::for_layout(mode, Surface::Panel).gap()).abs() < 1.0);
        assert!(crate::scenario::handle_at("timeline:First").is_some());
        let viewport = h.app.viewport.rect.unwrap();
        let content = content_rect(&h.app, viewport);
        assert_eq!(viewport.left(), if mode == LayoutMode::Floating { 0.0 } else { crate::ui_kit::history_width(true) });
        assert_eq!(content.left(), browser.right() + Container::for_layout(mode, Surface::Panel).gap());
        assert_eq!(content.bottom(), viewport.bottom());
        if mode == LayoutMode::Floating {
            assert!(covered(&h.app, history.center()));
            assert!(covered(&h.app, browser.center()));
            let gap_x = (history.right() + browser.left()) / 2.0;
            assert!(covered(&h.app, pos2(gap_x, history.center().y)), "the seam pill shields viewport input");
            assert!(!covered(&h.app, pos2(gap_x, history.top() + 4.0)), "the rest of the shell gap remains available");
        }
    }
}

#[test]
fn assistant_floats_without_changing_projection_and_shields_the_model() {
    for dark in [false, true] {
        for side in ["left", "right"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", serde_json::json!({"width":1400,"height":1000}));
            h.call("ui.set", serde_json::json!({"dark":dark,"layout":"floating"}));
            h.frames(4);
            let projection = h.app.viewport.rect.expect("viewport");
            let original_controls = h.app.viewport.layout.clone().expect("layout");
            h.call("ui.agent_chat", serde_json::json!({"open":true,"picker":false,"side":side,"width":440}));
            h.frames(6);
            let card = crate::scenario::rect_of("agent_chat:panel").expect("floating Assistant");
            assert_eq!(h.app.viewport.rect, Some(projection), "opening the card preserves projection");
            assert!(card.left() > projection.left() && card.right() < projection.right());
            assert!(card.bottom() < projection.bottom());
            let content = h.app.shell.content.expect("content");
            assert!((card.top() - content.top()).abs() < 1.0, "full-height top: {card:?} {content:?}");
            assert!((card.bottom() - content.bottom()).abs() < 1.0, "full-height bottom: {card:?} {content:?}");
            assert!(covered(&h.app, card.center()), "chat shields viewport input");
            if side == "right" {
                let layout = h.app.viewport.layout.as_ref().expect("layout");
                assert!(!card.intersect(layout.axis_view).is_positive(), "card clears compass");
                assert!(!card.intersect(layout.navigation).is_positive(), "card clears navigation");
                assert!(layout.axis_view.right() < original_controls.axis_view.right(), "compass moves left");
                assert!(layout.navigation.center().x < original_controls.navigation.center().x, "navigation moves left");
            } else {
                let browser = h.app.shell.browser.expect("browser card");
                assert!(card.right() < browser.left(), "left Assistant precedes browser");
                assert!((card.top() - browser.top()).abs() < 1.0, "Assistant and browser share the top margin");
            }
            let camera = h.app.cam;
            let at = card.center();
            h.call("ui.move", json!({"x":at.x,"y":at.y}));
            h.call("ui.scroll", json!({"x":at.x,"y":at.y,"delta":120}));
            h.call("ui.drag", json!({"x0":at.x,"y0":at.y,"x1":at.x-30.0,"y1":at.y+15.0,"button":"middle","steps":6}));
            assert_eq!(h.app.cam, camera, "chat consumes wheel and orbit input");
            assert!(h.app.viewport.boxsel.is_none());
            h.app.start("solid.extrude");
            h.frames(5);
            let feature = h.app.viewport.dialog_rect.expect("feature panel");
            let beside = crate::scenario::rect_of("agent_chat:panel").expect("Assistant beside feature");
            assert!(!beside.intersect(feature).is_positive(), "feature and Assistant coexist: {beside:?} {feature:?}");
            assert!(
                (beside.min - card.min).length() < 1.0 && (beside.max - card.max).length() < 1.0,
                "opening a tool must not move Assistant: {card:?} -> {beside:?}"
            );
            if side == "right" {
                assert!(feature.right() + SHELL_GAP <= beside.left() + 1.0, "tool stays inside Assistant's rail");
            }
            h.app.dialog = None;
            h.app.tool = None;
            h.frames(3);
            h.call("ui.agent_chat", serde_json::json!({"minimized":true}));
            h.frames(4);
            let folded = crate::scenario::rect_of("agent_chat:panel").expect("floating folded card");
            assert!(folded.width() < 60.0 && folded.height() < 120.0);
            assert!(covered(&h.app, folded.center()));
            assert_eq!(h.app.viewport.rect, Some(projection));
            let folded_controls = h.app.viewport.layout.as_ref().expect("layout");
            assert_eq!(folded_controls.axis_view, original_controls.axis_view);
            if side == "right" {
                assert_eq!(folded_controls.navigation, original_controls.navigation);
            } else {
                let shift = (folded.width() + SHELL_GAP) / 2.0;
                assert!((folded_controls.navigation.center().x - original_controls.navigation.center().x - shift).abs() < 1.0);
            }
            h.call("ui.agent_chat", serde_json::json!({"open":false}));
            h.frames(3);
            assert!(!covered(&h.app, card.center()), "closed chat leaves no stale obstruction");
        }
    }
}

#[test]
fn assistant_left_dock_precedes_history_and_drag_hints_match_the_result() {
    for layout in ["floating", "docked"] {
        for (names, stacked) in [(false, false), (true, false), (true, true)] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.preferences.timeline_left = true;
            h.app.preferences.timeline_under_browser = stacked;
            h.app.preferences.timeline_left_width = crate::ui_kit::history_width(names);
            h.call("ui.resize", json!({"width":1600,"height":1000}));
            h.call("ui.set", json!({"layout":layout}));
            h.call("ui.agent_chat", json!({"open":true,"side":"right","width":440,"picker":false}));
            h.frames(6);
            let projection = h.app.viewport.rect;
            for left in [true, false] {
                let grip = crate::scenario::handle_at("agent_chat:grip").expect("drag grip");
                let target = pos2(if left { 100.0 } else { 1500.0 }, grip.y);
                h.app.synthetic.push(egui::Event::PointerMoved(grip));
                h.frame();
                h.app.synthetic.push(egui::Event::PointerButton {
                    pos: grip,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                });
                h.frame();
                h.app.synthetic.push(egui::Event::PointerMoved(target));
                h.frames(2);
                let hint = rect_of("agent_chat:dock_hint").expect("visible dock target");
                h.app.synthetic.push(egui::Event::PointerButton {
                    pos: target,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                });
                h.frames(6);
                let card = h.app.agent_chat.panel_rect.expect("docked Assistant");
                assert_eq!(h.app.preferences.agent_chat_side, if left { "left" } else { "right" });
                assert!((hint.min - card.min).length() < 1.0 && (hint.max - card.max).length() < 1.0, "{layout} preview {hint:?} matches {card:?}");
                if layout == "floating" {
                    assert_eq!(h.app.viewport.rect, projection, "docking keeps model projection");
                }
                if left {
                    let history = h.app.shell.history.expect("vertical history");
                    let browser = h.app.shell.browser.expect("browser");
                    assert!(card.right() <= history.left(), "Assistant precedes history: {card:?} {history:?}");
                    if stacked {
                        assert!((history.left() - browser.left()).abs() < 1.0);
                        assert!((history.width() - browser.width()).abs() < 1.0);
                        assert!(browser.bottom() <= history.top(), "History stays below Browser");
                    } else {
                        assert!(history.right() <= browser.left(), "history precedes browser");
                    }
                }
            }
            h.call("ui.agent_chat", json!({"side":"left","minimized":true}));
            h.frames(6);
            let strip = h.app.agent_chat.drop_rect.expect("minimized Assistant");
            assert!(strip.right() <= h.app.shell.history.expect("history").left());
            h.call("ui.agent_chat", json!({"open":false}));
            h.frames(6);
            assert!(h.app.shell.history.expect("history returns to outer edge").left() < strip.right());
        }
    }
}

#[test]
fn assistant_right_edge_survives_palette_tool_resize_and_minimize() {
    for layout in ["floating", "docked"] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1800,"height":1000}));
        h.call("ui.set", json!({"layout":layout}));
        h.call("ui.agent_chat", json!({"open":true,"side":"right","width":380,"picker":false}));
        h.frames(5);
        let right = h.app.agent_chat.panel_rect.unwrap().right();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.frames(6);
        for width in [380.0, 480.0] {
            h.call("ui.agent_chat", json!({"width":width}));
            h.frames(6);
            let chat = h.app.agent_chat.panel_rect.unwrap();
            let palette = h.app.viewport.palette_rect.unwrap();
            assert!((chat.right() - right).abs() < 1.0, "{layout}: Assistant stays outside the palette");
            assert!(
                palette.right() + Container::for_layout(h.app.ui.layout, Surface::Panel).gap() <= chat.left() + 1.0,
                "{layout}: {palette:?}, {chat:?}"
            );
        }
        h.call("ui.agent_chat", json!({"minimized":true}));
        h.frames(5);
        let strip = h.app.agent_chat.drop_rect.unwrap();
        let palette = h.app.viewport.palette_rect.unwrap();
        assert!((strip.right() - right).abs() < 1.0);
        assert!(palette.right() <= strip.left(), "palette clears the minimized Assistant too");
        h.call("ui.agent_chat", json!({"open":false}));
        h.frames(5);
        assert!(h.app.viewport.palette_rect.unwrap().right() > palette.right(), "closing Assistant reclaims its rail");
    }
}

#[test]
fn tool_sheets_drag_over_browser_and_history_but_clear_protected_panels() {
    for side in ["left", "right"] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1800,"height":1000}));
        h.call("ui.set", json!({"layout":"floating"}));
        h.call("ui.agent_chat", json!({"open":true,"side":side,"width":380,"picker":false}));
        h.frames(5);
        h.app.start("sketch.create");
        h.frames(5);
        let chat = h.app.agent_chat.panel_rect.unwrap();
        let browser = h.app.shell.browser.unwrap();
        let history = h.app.shell.history.unwrap();
        let ribbon = h.app.shell.cards[1];
        let workspace = h.app.shell.cards[0];
        for target in [browser.center(), history.center(), ribbon.center(), workspace.center(), egui::pos2(-1000.0, 400.0), egui::pos2(3000.0, 400.0)]
        {
            let title = crate::scenario::handle_at("dialog:title").unwrap();
            h.call("ui.drag", json!({"x0":title.x,"y0":title.y,"x1":target.x,"y1":target.y,"steps":10}));
            h.frames(4);
            let sheet = h.app.viewport.dialog_rect.unwrap();
            assert!(!sheet.intersect(chat).is_positive(), "{side}: {sheet:?} overlaps Assistant {chat:?}");
            assert!(!sheet.intersect(ribbon).is_positive(), "sheet clears ribbon");
            assert!(!sheet.intersect(workspace).is_positive(), "sheet clears workspace selector");
            if target == browser.center() {
                assert!(sheet.intersect(browser).is_positive(), "sheet can cover Browser");
                let layer = h.ctx.layer_id_at(sheet.intersect(browser).center()).unwrap();
                assert_eq!(layer.id, egui::Id::unique(("sc_dialog", "CREATE SKETCH")), "sheet receives input above Browser");
            } else if target == history.center() {
                assert!(sheet.intersect(history).is_positive(), "sheet can cover History");
            }
        }
        h.step(&json!({"widget":"Cancel"})).unwrap();
        assert!(h.app.dialog.is_none(), "dragged sheet remains interactive");
    }
}

#[test]
fn tool_sheets_stay_above_browser_and_history_after_panel_clicks() {
    for dock in ["bottom", "left", "under_browser"] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1800,"height":1000}));
        h.call("ui.set", json!({"layout":"floating"}));
        h.app.run("view.history_layout", json!({"dock":dock})).unwrap();
        h.app.start("solid.extrude");
        h.settle();
        for panel in [h.app.shell.browser.unwrap(), h.app.shell.history.unwrap()] {
            let title = crate::scenario::handle_at("dialog:title").unwrap();
            let target = panel.center();
            h.call("ui.drag", json!({"x0":title.x,"y0":title.y,"x1":target.x,"y1":target.y,"steps":10}));
            let sheet = h.app.viewport.dialog_rect.unwrap();
            let overlap = sheet.intersect(panel);
            assert!(overlap.is_positive(), "{dock}: the dialog overlaps the panel");
            let exposed = panel.left_top() + egui::vec2(10.0, 10.0);
            assert!(!sheet.contains(exposed), "the panel click is outside the dialog");
            h.call("ui.click", json!({"x":exposed.x,"y":exposed.y}));
            let layer = h.ctx.layer_id_at(overlap.center()).unwrap();
            assert_eq!(layer.id, egui::Id::unique(("sc_dialog", "EXTRUDE")), "{dock}: clicking a panel must not cover the dialog");
            let panel_layer = h.ctx.layer_id_at(exposed).unwrap();
            h.call("ui.click", json!({"x":1700,"y":500}));
            assert_eq!(h.ctx.layer_id_at(exposed), Some(panel_layer), "clicking the model view must not cover the panel");
        }
        h.step(&json!({"widget":"Cancel"})).unwrap();
        assert!(h.app.dialog.is_none(), "{dock}: the overlapping dialog remains clickable");
    }
}

#[test]
fn tool_sheet_dropdowns_and_preferences_still_open_above_the_sheet() {
    for layout in ["floating", "compact"] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1400,"height":900}));
        h.call("ui.set", json!({"layout":layout}));
        h.app.start("solid.extrude");
        h.settle();
        let title = crate::scenario::handle_at("dialog:title").unwrap();
        h.call("ui.drag", json!({"x0":title.x,"y0":title.y,"x1":700,"y1":350,"steps":10}));
        h.step(&json!({"widget":"One Side"})).unwrap();
        h.step(&json!({"widget":"Symmetric"})).unwrap();
        assert!(matches!(h.app.dialog.as_ref().unwrap().kind, crate::dialogs::Kind::Extrude { direction: 2, .. }));
        let sheet = h.app.viewport.dialog_rect.unwrap();
        h.call("ui.prefs", json!({"open":"General"}));
        let preferences = rect_of("dialog:window").unwrap();
        let overlap = sheet.intersect(preferences);
        assert!(overlap.is_positive(), "Preferences overlaps the feature dialog");
        assert_eq!(h.ctx.layer_id_at(overlap.center()).unwrap().id, egui::Id::unique("sc_prefs"), "Preferences receives input above the sheet");
        let cancel = rect_of("prefs:cancel").expect("Preferences Cancel");
        assert_eq!(h.ctx.layer_id_at(cancel.center()).unwrap().id, egui::Id::unique("sc_prefs"), "Preferences owns its Cancel target");
        h.step(&json!({"click":{"handle":"prefs:cancel"}})).unwrap();
        assert!(!h.app.prefs_window.open && h.app.dialog.is_some(), "Cancel closes Preferences, leaving the feature dialog open");
        h.step(&json!({"widget":"Cancel"})).unwrap();
        assert!(h.app.dialog.is_none());
    }
}

#[test]
fn compact_layout_is_saved_and_survives_cancel_apply_defaults_and_bad_input() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.start("solid.extrude");
    h.settle();
    let design = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    let camera = h.app.cam;
    h.call("ui.prefs", json!({"open":"Layout"}));
    crate::prefs::reveal_preference(&mut h, "dialog:Panel style:Compact");
    h.step(&json!({"click":{"handle":"dialog:Panel style:Compact"}})).unwrap();
    h.call("ui.prefs", json!({"button":"ok"}));
    h.settle();
    assert!(h.app.dialog.is_some(), "changing layout keeps the active tool");
    assert_eq!(h.app.preferences.layout, LayoutMode::Docked);
    assert_eq!(h.app.ui.layout, LayoutMode::Docked);
    assert_eq!(h.app.cam, camera);
    h.call("engine.execute", json!({"command":"view.layout", "params":{"mode":"compact"}}));
    assert_eq!(h.app.cam, camera, "programmatic layout changes never auto-fit");
    assert_eq!(h.app.session.doc.to_json(), design);
    assert_eq!(h.app.session.undo.len(), undo);
    let mut restored = crate::SolveApp::new(Default::default(), Default::default());
    restored.load_prefs(&h.app.prefs());
    assert_eq!(restored.ui.layout, LayoutMode::Docked);
    assert_eq!(restored.preferences.layout, LayoutMode::Docked);
    h.call("ui.prefs", json!({"open":"Layout","layout":"floating"}));
    h.call("ui.prefs", json!({"button":"cancel"}));
    assert_eq!(h.app.ui.layout, LayoutMode::Docked);
    h.call("ui.prefs", json!({"open":"Layout","layout":"floating"}));
    h.call("ui.prefs", json!({"button":"apply"}));
    h.call("ui.prefs", json!({"layout":"compact"}));
    h.call("ui.prefs", json!({"button":"cancel"}));
    assert_eq!(h.app.ui.layout, LayoutMode::Floating);
    h.call("ui.prefs", json!({"open":"Layout","layout":"compact"}));
    h.call("ui.prefs", json!({"button":"defaults"}));
    assert_eq!(h.app.ui.layout, LayoutMode::Floating);
    assert_eq!(h.app.preferences.layout, LayoutMode::Floating);
    h.call("ui.prefs", json!({"button":"ok"}));
    assert!(crate::prefs::set(&mut h.app, &json!({"layout":"invalid"})).is_err());
    assert_eq!(h.app.ui.layout, LayoutMode::Floating);
    restored.load_prefs(r#"{"preferences":{"layout":"invalid","default_units":"in"}}"#);
    assert_eq!(restored.ui.layout, LayoutMode::Floating);
    assert_eq!(restored.preferences.default_units, "in");
    assert_eq!(crate::prefs::lenient(&json!({})).layout, LayoutMode::Floating);
}

#[test]
fn compact_chrome_is_flush_and_browser_hugs_content_in_both_themes() {
    for dark in [false, true] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":900}));
        h.call("ui.prefs", json!({"layout":"compact"}));
        h.call("ui.set", json!({"dark":dark}));
        h.settle();
        let bar = rect_of("toolbar").unwrap();
        let browser = rect_of("browser:card").unwrap();
        let workspace = rect_of("ribbon:workspace").unwrap();
        assert_eq!(workspace.left(), bar.left());
        assert_eq!(workspace.top(), bar.top());
        assert_eq!(h.app.shell.cards.get(1).unwrap().left(), workspace.right(), "one joined top bar");
        assert_eq!(browser.left(), 0.0);
        assert_eq!(browser.top(), bar.bottom(), "square, flush Browser/ribbon join");
        assert!(browser.height() < h.app.viewport.rect.unwrap().height() / 2.0, "Browser hugs its contents");
        let compact = crate::ui_kit::Container::new(&h.ctx, crate::ui_kit::Surface::Panel);
        assert_eq!(compact.gap(), 0.0);
        let frame = compact.frame(&h.ctx, crate::theme::Tokens::get().panel, crate::frame::Side::Left);
        assert_eq!(frame.shadow, egui::Shadow::NONE);
        assert_eq!(frame.corner_radius, egui::CornerRadius::ZERO);
        h.app.start("solid.extrude");
        h.settle();
        let dialog = h.app.viewport.dialog_rect.unwrap();
        assert!((dialog.right() - h.app.viewport.rect.unwrap().right()).abs() < 1.0, "{dialog:?}");
        let before = dialog;
        h.step(&json!({"drag":[{"handle":"dialog:title"},[700,300]],"steps":6})).unwrap();
        h.settle();
        let moved = h.app.viewport.dialog_rect.unwrap();
        assert!(moved.right() < before.right() - 100.0, "compact tool sheets detach from the right edge");
        let feature = crate::ui_kit::Container::new(&h.ctx, crate::ui_kit::Surface::Feature);
        assert_eq!(feature.frame(&h.ctx, crate::theme::Tokens::get().dialog_body, crate::frame::Side::Right).shadow, egui::Shadow::NONE);
        h.app.run("view.layout", json!({"mode":"floating"})).unwrap();
        h.settle();
        assert_eq!(rect_of("browser:card").unwrap().left(), SHELL_GAP);
        assert!(h.app.dialog.is_some());
    }
}

#[test]
fn two_collapsed_compact_headers_leave_live_viewport_below_their_column() {
    for dark in [false, true] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":900}));
        h.call("ui.prefs", json!({"layout":"compact"}));
        h.call("ui.set", json!({"dark":dark}));
        h.app.run("view.history_layout", json!({"dock":"under_browser"})).unwrap();
        h.app.preferences.timeline_left_browser_collapsed = true;
        h.app.preferences.timeline_collapsed = true;
        h.frames(12);
        let browser = h.app.shell.browser.unwrap();
        let history = h.app.shell.history.unwrap();
        let below = egui::pos2(browser.center().x, history.bottom() + 40.0);
        assert!(h.app.viewport.rect.unwrap().contains(below));
        assert!(!covered(&h.app, below));
        assert!(covered(&h.app, browser.center()));
        assert!(covered(&h.app, history.center()));
        h.call("ui.move", json!({"x":below.x,"y":below.y}));
        assert_eq!(h.app.viewport.mouse, Some(below), "the vacated column remains live viewport");
    }
}

#[test]
fn ribbon_labels_are_saved_independently_and_icon_only_commands_remain_reachable() {
    for mode in [LayoutMode::Floating, LayoutMode::Docked] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":900}));
        h.call("ui.prefs", json!({"layout":mode}));
        h.settle();
        let labelled = rect_of("toolbar").unwrap();
        h.call("ui.prefs", json!({"ribbon_labels":false}));
        h.settle();
        let icons = rect_of("toolbar").unwrap();
        assert!(icons.height() < labelled.height());
        assert_eq!(h.app.ui.layout, mode);
        let command = rect_of("button:sketch.create").unwrap();
        assert!(icons.contains_rect(command));
        h.step(&json!({"click":{"handle":"button:sketch.create"}})).unwrap();
        assert!(h.app.dialog.is_some(), "the icon still starts Create Sketch");
        let mut restored = crate::SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(!restored.preferences.ribbon_labels);
        assert_eq!(restored.ui.layout, mode);
        h.call("ui.prefs", json!({"open":"Interface","ribbon_labels":true}));
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(!h.app.preferences.ribbon_labels);
    }
    assert!(crate::prefs::lenient(&json!({})).ribbon_labels);
}

#[test]
fn embed_keeps_model_panels_without_application_chrome() {
    let mut h = Harness::new();
    h.app.embedded = true;
    h.app.home.show(h.app.session.revision);
    h.app.ui.show_browser = false;
    h.app.ui.show_timeline = false;
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    h.settle();
    assert!(!h.app.home.open);
    assert_eq!(h.app.ui.layout, LayoutMode::Docked);
    assert!(h.app.ui.show_browser && h.app.ui.show_timeline);
    assert!(rect_of("toolbar").is_none());
    assert!(rect_of("ribbon:workspace").is_none());
    assert!(rect_of("browser:card").is_some());
    assert!(h.app.viewport.rect.is_some());
}

#[test]
fn embed_editor_keeps_the_ribbon_and_leaves_files_to_the_host() {
    let mut h = Harness::new();
    h.settle();
    assert!(rect_of("appbar:home").is_some(), "the full app has its application bar");
    h.app.embed_editor = true;
    h.app.home.show(h.app.session.revision);
    h.settle();
    assert!(!h.app.home.open, "the host shows the design, never the start page");
    assert_eq!(h.app.ui.layout, LayoutMode::Docked);
    assert!(rect_of("appbar:home").is_none() && rect_of("titlebar:report").is_none(), "no application bar");
    assert!(rect_of("button:sketch.create").is_some() && rect_of("browser:card").is_some(), "the modelling surface stays");
    // With no bar to hold it, command search stays in the ribbon whatever the host's header layout.
    h.app.header = crate::toolbar::HeaderLayout::COMPACT;
    h.settle();
    assert!(rect_of("ribbon:search").is_some() && rect_of("titlebar:search").is_none());
    h.step(&json!({"click":{"handle":"button:sketch.create"}})).unwrap();
    assert!(h.app.dialog.is_some(), "and its commands work");
    h.step(&json!({"key":"Escape"})).unwrap();
    // New, Open, Save, Close and Preferences belong to the host page.
    let (docs, revision) = (h.app.docs.count(), h.app.session.revision);
    for key in ["N", "O", "S", "W", "Comma"] {
        h.step(&json!({"key":key,"cmd":true})).unwrap();
    }
    h.settle();
    assert_eq!((h.app.docs.count(), h.app.session.revision), (docs, revision));
    assert!(!h.app.file_picker_busy() && !h.app.prefs_window.open, "{}", h.call("ui.inspect", json!({})));
    // Undo stays the design's own.
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    h.settle();
    h.step(&json!({"key":"Z","cmd":true})).unwrap();
    assert!(h.app.session.doc.features.is_empty(), "Ctrl+Z undoes in the embed too");
}

/// A design with a finished sketch nothing uses yet, so Extrude opens ready for OK.
fn sketched(h: &mut Harness) {
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
}

#[test]
fn a_host_keeps_the_design_and_gives_it_back_with_its_timeline() {
    let mut h = Harness::new();
    h.app.embed_editor = true;
    sketched(&mut h);
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    h.settle();
    let kept = h.call("document.export", json!({}));
    assert_eq!(kept["ok"], true, "{kept}");
    let kept = &kept["result"];
    assert_eq!((kept["format"].as_str(), kept["dirty"].as_bool()), (Some("json"), Some(true)));
    assert_eq!(kept["revision"], h.call("document.revision", json!({}))["result"]["revision"], "the revision that follows the design");
    let text = kept["text"].as_str().unwrap().to_string();
    let timeline = |h: &Harness| h.app.session.doc.features.iter().map(|f| f.name.clone()).collect::<Vec<_>>();
    let before = timeline(&h);
    assert!(before.len() >= 2, "{before:?}");
    // The host says it saved: the design is saved from there, until the next change.
    let at = kept["revision"].as_u64().unwrap();
    assert_eq!(h.call("document.saved", json!({"revision": at}))["result"]["dirty"], false);
    assert!(!h.app.session.is_dirty());
    assert_eq!(h.call("document.saved", json!({"revision": at + 7}))["ok"], false, "another revision is refused");
    // Something else is drawn and left open; the kept design comes back in its place, saved,
    // timeline and all, and what was going on in the other leaves with it.
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    assert_eq!(h.call("document.saved", json!({"revision": at}))["ok"], false, "the design changed since");
    h.call("ui.start", json!({"command":"sketch.create"}));
    h.settle();
    assert!(h.app.dialog.is_some(), "{}", h.call("ui.inspect", json!({})));
    let (docs, id) = (h.app.docs.count(), h.app.session.document_id.clone());
    let back = h.call("document.import", json!({"text": text, "name": "Bracket"}));
    assert_eq!(back["ok"], true, "{back}");
    assert_eq!(back["result"]["errors"], 0);
    assert_eq!(back["result"]["revision"], h.call("document.revision", json!({}))["result"]["revision"]);
    assert_eq!(timeline(&h), before);
    assert_eq!((h.app.session.doc.name.as_str(), h.app.session.is_dirty(), h.app.docs.count()), ("Bracket", false, docs));
    assert_ne!(h.app.session.document_id, id, "another design, as one opened is");
    assert!(h.app.session.undo.is_empty(), "the undo history is the opened design's own");
    assert!(h.app.dialog.is_none() && h.app.tool.is_none());
    h.settle();
    for bad in [json!({}), json!({"text": "{"}), json!({"text": "not a design"})] {
        assert_eq!(h.call("document.import", bad.clone())["ok"], false, "{bad}");
        assert_eq!(timeline(&h), before, "a refused import leaves the design as it was");
    }
}

/// Dialogs › In the left column (#6707): Extrude takes the place of the browser and the History
/// at the left, at the column's width and never narrower than its card, in both layouts and
/// every History dock. The view keeps its size, the dialog is covered, the content area starts
/// beside it, and OK, Cancel or Esc bring the column back exactly as it was.
#[test]
fn a_dialog_in_the_left_column_takes_the_place_of_browser_and_history() {
    let card = crate::theme::Tokens::get().form_card_width;
    for mode in [LayoutMode::Floating, LayoutMode::Docked] {
        for (dock, close) in [("bottom", "Cancel"), ("left", "Escape"), ("under_browser", "OK")] {
            let what = format!("{mode:?}, History {dock}, {close}");
            let mut h = Harness::new();
            h.call("ui.resize", json!({"width":1400,"height":900}));
            h.call("ui.set", json!({"layout":mode}));
            h.call("ui.prefs", json!({"dialog_place":"left_column"}));
            h.app.run("view.history_layout", json!({"dock":dock})).unwrap();
            sketched(&mut h);
            h.settle();
            let browser = h.app.shell.browser.expect("browser");
            let history = h.app.shell.history.expect("history");
            let viewport = h.app.viewport.rect.unwrap();
            let prefs = serde_json::to_value(&h.app.preferences).unwrap();
            let column = if dock == "bottom" { browser } else { browser.union(history) };
            h.step(&json!({"start":"solid.extrude"})).unwrap();
            h.settle();
            let dialog = h.app.viewport.dialog_rect.expect("dialog");
            assert_eq!(rect_of("dialog:outer"), Some(dialog), "{what}");
            let place = rect_of("dialog:column").expect("the column is published");
            assert!((dialog.left() - column.left()).abs() <= 1.0 && (place.left() - column.left()).abs() <= 1.0, "{what}: {dialog:?} {column:?}");
            assert!((dialog.top() - column.top()).abs() <= 1.0, "{what}: {dialog:?} {column:?}");
            // The card's outline lies outside its response rect, one stroke at each side.
            assert!((dialog.width() - column.width().max(card)).abs() <= 2.5, "{what}: {dialog:?} {column:?}");
            assert!(dialog.bottom() <= place.bottom() + 1.0, "{what}: {dialog:?} {place:?}");
            assert!(rect_of("browser:card").is_none() && h.app.shell.browser.is_none(), "{what}: the browser is not drawn");
            if dock == "bottom" {
                assert_eq!(h.app.shell.history, Some(history), "{what}: the bottom strip stays");
                assert!(dialog.bottom() < history.top(), "{what}");
            } else {
                assert!(rect_of("history:vertical").is_none(), "{what}: the History is not drawn");
            }
            assert_eq!(h.app.viewport.rect, Some(viewport), "{what}: the view keeps its size");
            assert!(covered(&h.app, dialog.center()), "{what}");
            assert!(content_rect(&h.app, viewport).left() >= dialog.right(), "{what}");
            assert!(h.widget("Distance").is_some(), "{what}: the inputs are there");
            match close {
                "Escape" => h.step(&json!({"key":"Escape"})).unwrap(),
                other => h.step(&json!({"widget":other})).unwrap(),
            }
            h.settle();
            assert!(h.app.dialog.is_none(), "{what}");
            assert_eq!(h.app.session.doc.features.len(), if close == "OK" { 2 } else { 1 }, "{what}: OK adds the extrude");
            let back = h.app.shell.browser.expect("the browser is back");
            assert_eq!((back.x_range(), back.top()), (browser.x_range(), browser.top()), "{what}");
            assert_eq!(h.app.shell.history, Some(history), "{what}: the History is back as it was");
            assert_eq!(serde_json::to_value(&h.app.preferences).unwrap(), prefs, "{what}: no width or fold changed");
            assert!(rect_of("dialog:column").is_none(), "{what}");
        }
    }
}

/// With no column at the left (the browser hidden, or translucent over the view) a dialog in
/// the left column docks at the left edge at its own width, above the bottom History strip.
#[test]
fn without_a_left_column_a_dialog_docks_at_the_left_edge_at_its_own_width() {
    let card = crate::theme::Tokens::get().form_card_width;
    for (mode, browser) in [(LayoutMode::Floating, "hidden"), (LayoutMode::Docked, "hidden"), (LayoutMode::Floating, "translucent")] {
        let what = format!("{mode:?}, browser {browser}");
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":900}));
        h.call("ui.set", json!({"layout":mode}));
        h.call("ui.prefs", json!({"dialog_place":"left_column","browser_float":browser == "translucent"}));
        h.app.ui.show_browser = browser != "hidden";
        sketched(&mut h);
        h.settle();
        let history = h.app.shell.history.expect("history");
        let viewport = h.app.viewport.rect.unwrap();
        h.step(&json!({"start":"solid.extrude"})).unwrap();
        h.settle();
        let dialog = h.app.viewport.dialog_rect.expect("dialog");
        let gap = Container::for_layout(mode, Surface::Panel).gap();
        assert!((dialog.left() - (viewport.left() + gap)).abs() <= 1.0, "{what}: {dialog:?} {viewport:?}");
        assert!((dialog.width() - card).abs() <= 2.5, "{what}: {dialog:?}");
        assert!(dialog.bottom() < history.top(), "{what}");
        assert!(rect_of("browser:card").is_none(), "{what}");
        h.step(&json!({"widget":"Cancel"})).unwrap();
        h.settle();
        assert_eq!(rect_of("browser:card").is_some(), browser == "translucent", "{what}");
    }
}

/// The Assistant docked on the left keeps the outside edge: the column starts beside it, where
/// the browser was.
#[test]
fn a_dialog_in_the_left_column_starts_beside_the_assistant() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.call("ui.resize", json!({"width":1800,"height":1000}));
    h.call("ui.set", json!({"layout":"floating"}));
    h.call("ui.prefs", json!({"dialog_place":"left_column"}));
    h.call("ui.agent_chat", json!({"open":true,"side":"left","width":380,"picker":false}));
    h.frames(5);
    let browser = h.app.shell.browser.unwrap();
    h.app.start("sketch.create");
    h.frames(5);
    let chat = h.app.agent_chat.panel_rect.unwrap();
    let dialog = h.app.viewport.dialog_rect.unwrap();
    assert!((dialog.left() - browser.left()).abs() <= 1.0, "{dialog:?} {browser:?}");
    assert!(!dialog.intersect(chat).is_positive(), "{dialog:?} {chat:?}");
    let title = crate::scenario::handle_at("dialog:title").unwrap();
    h.call("ui.drag", json!({"x0":title.x,"y0":title.y,"x1":title.x + 300.0,"y1":title.y + 200.0,"steps":10}));
    h.frames(4);
    assert_eq!(h.app.viewport.dialog_rect, Some(dialog), "a dialog in the column stays in its place");
}

/// The Sketch Palette keeps the right edge while a sketch dialog holds the left column, also on
/// a view too narrow for both side by side; editing a feature reopens its dialog in the column
/// and Cancel rolls the marker back.
#[test]
fn the_sketch_palette_stays_and_editing_a_feature_opens_in_the_left_column() {
    for width in [1400.0, 900.0] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":width,"height":900}));
        h.call("ui.prefs", json!({"dialog_place":"left_column"}));
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
        h.settle();
        let palette = rect_of("sketch_palette:outer").expect("palette");
        let browser = h.app.shell.browser.unwrap();
        h.step(&json!({"start":"inspect.measure"})).unwrap();
        h.settle();
        let dialog = h.app.viewport.dialog_rect.expect("measure");
        let kept = rect_of("sketch_palette:outer").expect("the palette stays on screen");
        assert!((kept.right() - palette.right()).abs() <= 1.0 && (kept.top() - palette.top()).abs() <= 1.0, "width {width}: {kept:?} {palette:?}");
        assert!((dialog.left() - browser.left()).abs() <= 1.0 && !dialog.intersect(palette).is_positive(), "width {width}: {dialog:?}");
        h.step(&json!({"widget":"Close"})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("solid.extrude", json!({"distance":10})).unwrap();
        h.settle();
        let extrude = h.app.session.doc.features.last().unwrap().id;
        let marker = h.app.session.doc.marker;
        h.app.edit_feature(extrude);
        h.settle();
        assert!(h.app.dialog.as_ref().is_some_and(|d| d.editing.is_some()));
        assert_ne!(h.app.session.doc.marker, marker, "editing rolls the marker to the feature");
        let edit = h.app.viewport.dialog_rect.expect("edit dialog");
        assert!((edit.left() - browser.left()).abs() <= 1.0, "width {width}: {edit:?}");
        assert!(h.app.shell.browser.is_none());
        h.step(&json!({"widget":"Cancel"})).unwrap();
        h.settle();
        assert_eq!(h.app.session.doc.marker, marker);
        assert!(h.app.shell.browser.is_some());
    }
}

/// OK and Cancel in the view's corner (#6709) with a dialog in the left column: the pair keeps
/// the view's top right, beside the column's content area and clear of the dialog; its check
/// applies the dialog and its cross cancels, and either way the column comes back.
#[test]
fn the_view_corner_answers_a_dialog_in_the_left_column() {
    for how in ["ok", "cancel"] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":900}));
        h.call("ui.prefs", json!({"dialog_place":"left_column","view_confirm_corner":true}));
        sketched(&mut h);
        h.settle();
        let browser = h.app.shell.browser.expect("browser");
        h.step(&json!({"start":"solid.extrude"})).unwrap();
        h.settle();
        let dialog = h.app.viewport.dialog_rect.expect("dialog");
        let pair = rect_of("view:confirm").expect("the pair in the view's corner");
        let content = content_rect(&h.app, h.app.viewport.rect.unwrap());
        assert!((dialog.left() - browser.left()).abs() <= 1.0, "{how}: the dialog holds the column: {dialog:?}");
        assert!(content.contains_rect(pair) && pair.left() > content.center().x, "{how}: {pair:?} in the top right of {content:?}");
        assert!(!pair.intersects(dialog), "{how}: {pair:?} {dialog:?}");
        assert!(covered(&h.app, pair.center()), "{how}");
        assert_eq!(crate::scenario::count_of("view:confirm:ok:enabled"), Some(1.0), "{how}");
        h.step(&json!({"click":{"handle":format!("view:confirm:{how}")}})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{how}: the dialog closes");
        assert_eq!(h.app.session.doc.features.len(), if how == "ok" { 2 } else { 1 }, "{how}: the check makes the extrude");
        assert!(h.app.shell.browser.is_some_and(|b| b.x_range() == browser.x_range()), "{how}: the column comes back");
    }
}

/// Dialogs is a Layout preference: set from the page or the control channel, saved,
/// refused when unknown, and Cancel and Restore Defaults put it back. A dialog already open
/// moves with it.
#[test]
fn dialog_place_is_saved_and_survives_cancel_defaults_and_bad_input() {
    use crate::dialog_column::DialogPlace;
    let mut h = Harness::new();
    sketched(&mut h);
    h.step(&json!({"start":"solid.extrude"})).unwrap();
    h.settle();
    let right = h.app.viewport.dialog_rect.unwrap();
    assert_eq!(h.app.preferences.dialog_place, DialogPlace::Edge);
    h.call("ui.prefs", json!({"open":"Layout"}));
    crate::prefs::reveal_preference(&mut h, "dialog:Dialogs:In the left column");
    h.step(&json!({"click":{"handle":"dialog:Dialogs:In the left column"}})).unwrap();
    assert_eq!(h.app.preferences.dialog_place, DialogPlace::LeftColumn);
    h.call("ui.prefs", json!({"button":"ok"}));
    h.settle();
    let left = h.app.viewport.dialog_rect.unwrap();
    assert!(left.left() < right.left() && h.app.shell.browser.is_none(), "the open dialog moves to the column: {left:?}");
    let mut restored = crate::SolveApp::new(Default::default(), Default::default());
    restored.load_prefs(&h.app.prefs());
    assert_eq!(restored.preferences.dialog_place, DialogPlace::LeftColumn);
    h.call("ui.prefs", json!({"open":"Layout","dialog_place":"edge"}));
    h.call("ui.prefs", json!({"button":"cancel"}));
    assert_eq!(h.app.preferences.dialog_place, DialogPlace::LeftColumn);
    h.call("ui.prefs", json!({"open":"Layout"}));
    h.call("ui.prefs", json!({"button":"defaults"}));
    assert_eq!(h.app.preferences.dialog_place, DialogPlace::Edge);
    h.call("ui.prefs", json!({"button":"ok"}));
    assert!(crate::prefs::set(&mut h.app, &json!({"dialog_place":"top"})).is_err());
    assert_eq!(h.app.preferences.dialog_place, DialogPlace::Edge);
    restored.load_prefs(r#"{"preferences":{"dialog_place":"top","default_units":"in"}}"#);
    assert_eq!(restored.preferences.dialog_place, DialogPlace::Edge);
    assert_eq!(restored.preferences.default_units, "in");
}

/// Every text the last frame painted.
fn painted_texts(h: &Harness) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.shapes.iter().for_each(|s| walk(&s.shape, &mut out));
    out
}

#[test]
fn status_bar_spans_the_window_below_history_in_both_layouts_and_is_covered() {
    for mode in [LayoutMode::Docked, LayoutMode::Floating] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":900}));
        h.call("ui.set", json!({"layout":mode}));
        h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
        h.settle();
        assert!(rect_of("status:bar").is_none() && h.app.shell.status_bar.is_none(), "off by default");
        let viewport_without = h.app.viewport.rect.unwrap();
        let covered_without = covered_rects(&h.app);
        h.call("ui.prefs", json!({"status_bar": true}));
        h.settle();
        let bar = rect_of("status:bar").unwrap();
        assert_eq!(h.app.shell.status_bar, Some(bar));
        assert_eq!((bar.left(), bar.right(), bar.bottom()), (0.0, 1400.0, 900.0), "{mode:?}: full width at the window's bottom: {bar:?}");
        assert_eq!(bar.height(), crate::status_bar::HEIGHT);
        let viewport = h.app.viewport.rect.unwrap();
        assert!(viewport.bottom() <= bar.top() + 0.5, "{mode:?}: the view ends above the bar: {viewport:?} {bar:?}");
        assert_eq!(viewport.bottom(), viewport_without.bottom() - bar.height(), "{mode:?}: the view gives up only the bar's height");
        let history = h.app.shell.history.unwrap();
        assert!(history.bottom() <= bar.top() + 0.5, "{mode:?}: History sits above the bar: {history:?} {bar:?}");
        let rects = covered_rects(&h.app);
        assert!(rects.contains(&bar), "{mode:?}: the bar is covered: {rects:?}");
        assert_eq!(rects.len(), covered_without.len() + 1, "{mode:?}: {rects:?} {covered_without:?}");
        assert!(covered(&h.app, bar.center()));
        h.call("ui.prefs", json!({"status_bar": false}));
        h.settle();
        assert!(rect_of("status:bar").is_none() && h.app.shell.status_bar.is_none());
        assert_eq!(h.app.viewport.rect.unwrap(), viewport_without);
    }
}

#[test]
fn viewport_sketch_status_distinguishes_empty_and_constrained_geometry() {
    for mode in [LayoutMode::Docked, LayoutMode::Floating] {
        let mut h = Harness::new();
        h.call("ui.set", json!({"layout":mode}));
        let corner = |h: &Harness| painted_texts(h).into_iter().filter(|t| t.starts_with("Sketch1 — ")).collect::<Vec<_>>();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.settle();
        assert_eq!(corner(&h), ["Sketch1 — No geometry yet"], "{mode:?}: an empty sketch is not fully constrained");
        assert!(h.widget("No geometry yet").is_some(), "the palette agrees with the viewport");

        let rectangle = h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
        h.settle();
        assert_eq!(corner(&h), ["Sketch1 — 2 degrees of freedom"], "the rectangle's width and height are free");
        h.app.run("sketch.constraint.fix", json!({"entities":rectangle["curves"]})).unwrap();
        h.settle();
        assert_eq!(corner(&h), ["Sketch1 — fully constrained"], "drawn geometry can be fully constrained");

        h.app.run("edit.undo", json!({})).unwrap();
        h.settle();
        assert_eq!(corner(&h), ["Sketch1 — 2 degrees of freedom"]);
        h.app.run("edit.undo", json!({})).unwrap();
        h.settle();
        assert_eq!(corner(&h), ["Sketch1 — No geometry yet"], "undoing the geometry restores the empty status");
    }
}

#[test]
fn status_bar_names_what_is_edited_and_the_sketch_state_and_replaces_the_views_corner_text() {
    use crate::status_bar::SketchState;
    let mut h = Harness::new();
    let corner = |h: &Harness| painted_texts(h).into_iter().filter(|t| t.starts_with("Sketch1 — ")).collect::<Vec<_>>();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.settle();
    assert_eq!(corner(&h), ["Sketch1 — No geometry yet"], "without the bar the state stays in the view's corner");
    h.call("ui.prefs", json!({"status_bar": true}));
    h.settle();
    let contents = crate::status_bar::contents(&h.app);
    assert_eq!(contents.editing.as_deref(), Some("Editing Sketch1"));
    assert_eq!(contents.sketch, Some(SketchState::Empty), "a blank sketch is not called fully constrained");
    let rectangle = h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
    h.settle();
    let contents = crate::status_bar::contents(&h.app);
    let Some(SketchState::Under(dof)) = contents.sketch else { panic!("{contents:?}") };
    assert!(dof > 0);
    let texts = painted_texts(&h);
    assert!(corner(&h).is_empty(), "with the bar on, the view's corner shows no state: {texts:?}");
    assert!(texts.contains(&format!("Under constrained ({dof} degrees of freedom)")), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Editing Sketch1") && texts.iter().any(|t| t == "mm"), "{texts:?}");
    assert!(crate::scenario::handle_at("status:editing").unwrap().x < crate::scenario::handle_at("status:sketch").unwrap().x);
    assert!(crate::scenario::handle_at("status:sketch").unwrap().x < crate::scenario::handle_at("status:units").unwrap().x);
    // Fully constrained: the rectangle fixed in place.
    h.app.run("sketch.constraint.fix", json!({"entities": rectangle["curves"]})).unwrap();
    h.settle();
    assert_eq!(crate::status_bar::contents(&h.app).sketch, Some(SketchState::Full));
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("solid.extrude", json!({"distance":10})).unwrap();
    h.settle();
    let idle = crate::status_bar::contents(&h.app);
    assert_eq!((idle.editing, idle.sketch, idle.units.as_str()), (None, None, "mm"), "idle: only the units");
    assert!(crate::scenario::handle_at("status:editing").is_none() && crate::scenario::handle_at("status:sketch").is_none());
    // A feature's dialog, editing it.
    let extrude = h.app.session.doc.features.iter().find(|f| f.name == "Extrude1").map(|f| f.id).unwrap();
    h.app.edit_feature(extrude);
    h.settle();
    assert!(h.app.dialog.as_ref().is_some_and(|d| d.editing.is_some()));
    assert_eq!(crate::status_bar::contents(&h.app).editing.as_deref(), Some("Editing Extrude1"));
    // In another language the text is translated, and so is a default feature name.
    let _ja = crate::i18n::scope("ja");
    assert_eq!(crate::status_bar::contents(&h.app).editing, Some(format!("編集中: {}", crate::i18n::tr_name("Extrude1"))));
    assert_eq!(crate::i18n::tr_name("Sketch1"), "スケッチ1");
}

#[test]
fn status_bar_units_open_the_units_menu_above_the_bar_and_change_units_in_one_undo_step() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1400,"height":900}));
    h.call("ui.prefs", json!({"status_bar": true}));
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    h.settle();
    let undo = h.app.session.undo.len();
    let button = rect_of("status:units").unwrap();
    assert!(rect_of("status:bar").unwrap().contains_rect(button));
    h.step(&json!({"click":{"handle":"status:units"}})).unwrap();
    h.settle();
    assert_eq!(h.app.menu.target, crate::context_menu::Target::Units);
    let at = h.app.viewport.context_menu.unwrap();
    assert_eq!(at, button.right_top());
    let menu = crate::context_menu::describe(&h.app);
    let labels: Vec<_> = menu["items"].as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap().to_string()).collect();
    assert_eq!(labels, crate::prefs::UNITS);
    // The list opens above the bar, inside the window.
    let list = h.ctx.memory(|m| m.area_rect(egui::Id::unique("sc_context_menu"))).unwrap();
    assert!(list.bottom() <= button.top() + 0.5 && list.top() >= 0.0, "{list:?} {button:?}");
    assert_eq!(h.app.session.undo.len(), undo, "opening the menu changes nothing");
    h.call("ui.menuPick", json!({"item":"in"}));
    h.settle();
    assert_eq!(h.app.session.doc.units, "in");
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(h.app.viewport.context_menu.is_none());
    assert_eq!(crate::status_bar::contents(&h.app).units, "in");
    h.app.run("edit.undo", json!({})).unwrap();
    h.settle();
    assert_eq!(h.app.session.doc.units, "mm");
    // A second click on the button closes the menu it opened.
    h.step(&json!({"click":{"handle":"status:units"}})).unwrap();
    h.settle();
    assert!(h.app.viewport.context_menu.is_some());
    h.step(&json!({"click":{"handle":"status:units"}})).unwrap();
    h.settle();
    assert!(h.app.viewport.context_menu.is_none());
}

#[test]
fn status_bar_mouse_mapping_opens_the_mappings_and_picks_one_as_the_preferences_page_does() {
    use crate::context_menu::Target;
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1400,"height":900}));
    h.call("ui.prefs", json!({"status_bar": true}));
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    h.settle();
    let doc = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    let button = rect_of("status:navigation").unwrap();
    assert!(rect_of("status:bar").unwrap().contains_rect(button));
    assert!(button.right() < rect_of("status:units").unwrap().left(), "the mapping sits left of the units");
    assert_eq!(crate::status_bar::contents(&h.app).navigation, "Fusion");
    assert!(painted_texts(&h).iter().any(|t| t == "Fusion"));
    h.step(&json!({"click":{"handle":"status:navigation"}})).unwrap();
    h.settle();
    assert_eq!(h.app.menu.target, Target::Navigation);
    assert_eq!(h.app.viewport.context_menu, Some(button.right_top()));
    let menu = crate::context_menu::items(&h.app, &Target::Navigation);
    let labels: Vec<_> = menu.iter().map(|item| item.label.as_str()).collect();
    assert_eq!(labels, crate::prefs::NAV_PRESETS.iter().map(|preset| preset.translated_label()).collect::<Vec<_>>());
    assert_eq!(menu.iter().filter(|item| item.shortcut == "current").map(|item| item.label.as_str()).collect::<Vec<_>>(), ["Fusion"]);
    let list = h.ctx.memory(|m| m.area_rect(egui::Id::unique("sc_context_menu"))).unwrap();
    assert!(list.bottom() <= button.top() + 0.5 && list.top() >= 0.0, "{list:?} {button:?}");
    // Each mapping, picked from the menu, sets what Preferences › Navigation sets for it.
    for preset in crate::prefs::NAV_PRESETS {
        let mut expected = h.app.preferences.clone();
        crate::prefs::choose_nav_preset(&mut expected, preset);
        if h.app.viewport.context_menu.is_none() {
            h.step(&json!({"click":{"handle":"status:navigation"}})).unwrap();
            h.settle();
        }
        h.call("ui.menuPick", json!({"item": preset.translated_label()}));
        h.settle();
        assert_eq!(serde_json::to_value(&h.app.preferences).unwrap(), serde_json::to_value(&expected).unwrap(), "{}", preset.id);
        assert!(h.app.viewport.context_menu.is_none());
        assert_eq!(crate::status_bar::contents(&h.app).navigation, preset.translated_label());
    }
    assert_eq!(h.app.session.doc.to_json(), doc, "a preference, not a design edit");
    assert_eq!(h.app.session.undo.len(), undo);
    // An unknown mapping changes nothing.
    let before = serde_json::to_value(&h.app.preferences).unwrap();
    let hostile = crate::ui_kit::Item::action("ui.navPreset", "Nope", "").with(json!({"preset": "nope"}));
    crate::context_menu::run_item(&mut h.app, &hostile, egui::pos2(0.0, 0.0));
    assert_eq!(serde_json::to_value(&h.app.preferences).unwrap(), before);
    // A second click on the button closes the menu it opened.
    h.step(&json!({"click":{"handle":"status:navigation"}})).unwrap();
    h.settle();
    assert!(h.app.viewport.context_menu.is_some());
    h.step(&json!({"click":{"handle":"status:navigation"}})).unwrap();
    h.settle();
    assert!(h.app.viewport.context_menu.is_none());
}

#[test]
fn status_bar_stays_out_of_the_embed() {
    let mut h = Harness::new();
    h.app.embedded = true;
    h.app.preferences.status_bar = true;
    h.settle();
    assert!(!crate::status_bar::shown(&h.app));
    assert!(rect_of("status:bar").is_none() && h.app.shell.status_bar.is_none());
}

/// Content kept from an earlier frame that lies outside the viewport now (the window shrank, the
/// scale grew) gave an inside-out content rect, min past max, and the Render workspace's frame
/// overlay panicked clamping into it: `min > max, or either was NaN. min = 1004.0, max = 937.8125`.
/// The viewport stands in for such content, and the space left beside cards never panics.
#[test]
fn content_outside_a_shrunk_viewport_gives_way_to_the_viewport() {
    let mut h = Harness::new();
    let viewport = Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 600.0));
    h.app.shell.content = Some(Rect::from_min_max(pos2(900.0, 650.0), pos2(1600.0, 1000.0)));
    assert_eq!(content_rect(&h.app, viewport), viewport);
    // A rect already turned inside out, with History and Browser cards at its edges.
    let inside_out = Rect::from_min_max(pos2(500.0, 1004.0), pos2(300.0, 938.0));
    h.app.shell.history = Some(Rect::from_min_max(pos2(0.0, 900.0), pos2(800.0, 1000.0)));
    h.app.shell.browser = Some(Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 800.0)));
    for left in [false, true] {
        h.app.preferences.timeline_left = left;
        let free = workspace_content(&h.app, inside_out);
        assert!(free.min.x.is_finite() && free.min.y.is_finite(), "{free:?}");
    }
}

/// The interface scale raised after a click at the top of a narrow window (found by a UI fuzzer
/// and shrunk to these three steps): the content kept from the frame before lay outside the new
/// viewport, and the next frame's layout panicked clamping into it (`min = 264.0, max =
/// 165.28125`).
#[test]
fn a_larger_interface_scale_in_a_narrow_window_keeps_drawing() {
    let mut h = Harness::new();
    for step in [
        json!({"call": "ui.resize", "params": {"width": 330.5693343218654, "height": 1445.8773151935186}}),
        json!({"click": [206.1875, 17.0]}),
        json!({"call": "ui.prefs", "params": {"ui_scale": 2.0}}),
    ] {
        h.step(&step).unwrap();
    }
    h.frames(3);
    assert!(h.app.viewport.rect.unwrap().is_positive());
}

#[test]
fn docked_assistant_beside_an_inverted_view_area_does_not_panic() {
    // A surface reconfigure can leave the view area inside out, its left past its right (#9492).
    for side in ["right", "left"] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.set", json!({"layout":"floating"}));
        h.call("ui.agent_chat", json!({"open":true,"side":side,"width":380,"picker":false}));
        h.frames(5);
        assert!(h.app.agent_chat.panel_rect.is_some(), "{side}: docked Assistant");
        h.app.viewport.rect = Some(Rect::from_min_max(pos2(264.1875, 30.0), pos2(159.0, 20.0)));
        let ctx = h.ctx.clone();
        crate::viewport::refresh_layout(&mut h.app, &ctx);
        assert!(h.app.viewport.layout.is_some(), "{side}: a layout for the next frame");
    }
}
