// Home and Fit frame what is shown now, and a middle-button double-click goes Home.
use crate::scenario::Harness;
use serde_json::json;
use solidcraft_engine::geom::{Aabb3, Vec3};
use solidcraft_engine::render::StandardView;

/// A block drawn at the origin and extruded, then moved 500 mm along X: its sketch stays at the
/// origin, used by the extrude (so not drawn).
fn moved_block(h: &mut Harness) {
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("solid.extrude", json!({"distance": 5})).unwrap();
    let body = h.app.session.world_state().bodies[0].name.clone();
    h.app.run("solid.move", json!({"bodies": [body], "translate": [500, 0, 0]})).unwrap();
    h.app.preferences.look_at_anim = false;
    h.settle();
}

fn near_box(a: Aabb3, lo: [f64; 3], hi: [f64; 3]) {
    let (lo, hi) = (Vec3::new(lo[0], lo[1], lo[2]), Vec3::new(hi[0], hi[1], hi[2]));
    assert!(a.min.dist(lo) < 1e-6 && a.max.dist(hi) < 1e-6, "{a:?} != {lo:?}..{hi:?}");
}

/// The camera Home gives: the iso view framing what is shown.
fn home(h: &Harness) -> solidcraft_engine::render::Camera {
    let mut cam = h.app.cam;
    cam.set_view(StandardView::Iso);
    h.app.fitted(cam)
}

/// Somewhere in the view away from the model and the overlays.
fn spot(h: &Harness) -> egui::Pos2 {
    let d = h.app.viewport.layout.as_ref().unwrap().drawable;
    d.left_top() + egui::vec2(d.width() * 0.3, d.height() * 0.7)
}

fn same_cam(a: &solidcraft_engine::render::Camera, b: &solidcraft_engine::render::Camera) -> bool {
    a.target.dist(b.target) < 1e-6 && (a.distance - b.distance).abs() < 1e-6 && (a.yaw - b.yaw).abs() < 1e-9 && (a.pitch - b.pitch).abs() < 1e-9
}

#[test]
fn home_and_fit_frame_the_moved_body_not_its_old_place() {
    let mut h = Harness::new();
    moved_block(&mut h);
    near_box(crate::viewport::view_bounds(&h.app), [500., 0., 0.], [510., 10., 5.]);
    for view in ["home", "fit"] {
        h.app.cam.pan(300.0, 200.0, 500.0);
        h.app.animate_view(view);
        h.settle();
        // Near the body's centre (the fit leaves room for the palettes), not 250 mm away.
        assert!(h.app.cam.target.dist(Vec3::new(505.0, 5.0, 2.5)) < 5.0, "{view} centres the body: {:?}", h.app.cam.target);
        assert!(h.app.cam.distance < 200.0, "{view} frames 10 mm, not 510: {}", h.app.cam.distance);
    }
    // The orbit pivot over empty space (#464) is the centre of the same box.
    let pivot = crate::viewport::view_bounds(&h.app).center();
    assert!(pivot.dist(Vec3::new(505.0, 5.0, 2.5)) < 1e-6);
    // The Sketches folder hidden: an unused sketch no longer counts either.
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.circle.center", json!({"center": [-300, 0], "radius": 5})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    near_box(crate::viewport::view_bounds(&h.app), [-305., -5., 0.], [510., 10., 5.]);
    h.app.session.visibility.hidden_folders.push("c0/sketches".into());
    near_box(crate::viewport::view_bounds(&h.app), [500., 0., 0.], [510., 10., 5.]);
}

#[test]
fn middle_double_click_goes_home_in_every_preset() {
    for preset in crate::prefs::NAV_PRESETS.iter().map(|p| p.id) {
        let mut h = Harness::new();
        h.call("ui.prefs", json!({"nav": preset}));
        moved_block(&mut h);
        h.app.cam.set_view(StandardView::Front);
        h.app.cam.pan(120.0, -80.0, 500.0);
        let p = spot(&h);
        h.call("ui.click", json!({"x": p.x, "y": p.y, "button": "middle", "double": true}));
        h.settle();
        let want = home(&h);
        assert!(same_cam(&h.app.cam, &want), "{preset}: {:?} != {:?}", h.app.cam, want);
        assert!(h.app.cam.target.dist(Vec3::new(505.0, 5.0, 2.5)) < 5.0, "{preset}: {:?}", h.app.cam.target);
    }
}

#[test]
fn middle_drags_and_single_clicks_do_not_go_home() {
    for preset in crate::prefs::NAV_PRESETS.iter().map(|p| p.id) {
        let mut h = Harness::new();
        h.call("ui.prefs", json!({"nav": preset}));
        moved_block(&mut h);
        h.app.cam.set_view(StandardView::Front);
        h.app.cam.pan(120.0, -80.0, 500.0);
        let p = spot(&h);
        // A middle drag (pan or orbit), then a click where it ended: a drag and one click.
        h.call("ui.drag", json!({"x0": p.x, "y0": p.y, "x1": p.x + 40.0, "y1": p.y + 25.0, "button": "middle", "steps": 4}));
        h.settle();
        let dragged = h.app.cam;
        let q = p + egui::vec2(40.0, 25.0);
        h.call("ui.click", json!({"x": q.x, "y": q.y, "button": "middle"}));
        h.settle();
        assert!(same_cam(&h.app.cam, &dragged), "{preset}: a drag and a click went home");
        // A left click and then a middle click are not a middle double-click.
        h.call("ui.click", json!({"x": q.x, "y": q.y}));
        h.call("ui.click", json!({"x": q.x, "y": q.y, "button": "middle"}));
        h.settle();
        assert!(same_cam(&h.app.cam, &dragged), "{preset}: left + middle went home");
    }
}

#[test]
fn middle_double_click_preference_fit_or_off() {
    let mut h = Harness::new();
    moved_block(&mut h);
    let bad = h.call("ui.prefs", json!({"middle_double_click": "zoom"}));
    assert!(bad.to_string().contains("middle_double_click"), "{bad}");
    assert_eq!(h.app.preferences.middle_double_click, "home");

    h.call("ui.prefs", json!({"middle_double_click": "fit"}));
    h.app.cam.set_view(StandardView::Front);
    h.app.cam.pan(120.0, -80.0, 500.0);
    let (yaw, pitch) = (h.app.cam.yaw, h.app.cam.pitch);
    let p = spot(&h);
    h.call("ui.click", json!({"x": p.x, "y": p.y, "button": "middle", "double": true}));
    h.settle();
    assert_eq!((h.app.cam.yaw, h.app.cam.pitch), (yaw, pitch), "Fit keeps the orientation");
    assert!(h.app.cam.target.dist(Vec3::new(505.0, 5.0, 2.5)) < 5.0, "{:?}", h.app.cam.target);
    // Fit frames everything shown, not the selection.
    let body = h.app.session.world_state().bodies[0].name.clone();
    h.app.run("select.set", json!({"items": [{"type": "body", "name": body}]})).unwrap();
    let fitted = h.app.cam;
    h.app.cam.pan(60.0, 0.0, 500.0);
    // Well after the last pair (a third click soon after a double-click is a triple-click).
    h.frames(60);
    h.call("ui.click", json!({"x": p.x, "y": p.y, "button": "middle", "double": true}));
    h.settle();
    assert!(same_cam(&h.app.cam, &fitted));

    h.call("ui.prefs", json!({"middle_double_click": "off"}));
    h.app.cam.pan(60.0, 0.0, 500.0);
    let moved = h.app.cam;
    h.frames(60);
    h.call("ui.click", json!({"x": p.x, "y": p.y, "button": "middle", "double": true}));
    h.settle();
    assert!(same_cam(&h.app.cam, &moved), "Off does nothing");
}

#[test]
fn view_bar_json_commands_apply_ui_state_without_dialogs_or_undo() {
    use crate::viewport::NavMode;
    let mut h = Harness::new();
    moved_block(&mut h);
    let design = h.app.session.doc.to_json();
    let undo = h.app.session.history.len();
    let camera = h.app.cam;
    for (tool, mode) in [("orbit", Some(NavMode::Orbit)), ("pan", Some(NavMode::Pan)), ("zoom", Some(NavMode::Zoom)), ("none", None)] {
        let r = h.call("engine.execute", json!({"command":"view.tool","params":{"tool":tool}}));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(h.app.viewport.nav, mode);
    }
    for (index, style) in ["shaded_with_edges", "shaded", "wireframe", "shaded_with_hidden_edges"].into_iter().enumerate() {
        let r = h.call("engine.execute", json!({"command":"view.visual_style","params":{"style":style}}));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(usize::from(h.app.ui.visual_style), index);
    }
    for id in ["view.spin", "view.grid", "view.ground_shadow", "view.navigation_bar"] {
        let key = if id == "view.navigation_bar" { "visible" } else { "on" };
        let state = |app: &crate::SolveApp| match id {
            "view.spin" => app.viewport.spin,
            "view.grid" => app.ui.show_grid,
            "view.ground_shadow" => app.ui.ground_shadow,
            _ => app.ui.show_navigation_bar,
        };
        for on in [false, true] {
            h.app.run(id, json!({key:on})).unwrap();
            assert_eq!(state(&h.app), on);
            h.app.run(id, json!({key:on})).unwrap();
            assert_eq!(state(&h.app), on, "explicit is idempotent");
            h.app.run(id, json!({})).unwrap();
            assert_eq!(state(&h.app), !on);
        }
        let before = state(&h.app);
        for value in [json!(null), json!("false"), json!(0)] {
            assert!(h.app.run(id, json!({key:value})).is_err());
            assert_eq!(state(&h.app), before);
        }
    }
    h.app.run("view.spin", json!({"on":false})).unwrap();
    assert_eq!(h.app.cam, camera, "display commands must not trigger the control adapter's first-body fit");
    for (id, params) in [("view.tool", json!({"tool":"bad"})), ("view.visual_style", json!({"style":2}))] {
        let state = (h.app.viewport.nav, h.app.ui.visual_style);
        assert!(h.app.run(id, params).is_err());
        assert_eq!((h.app.viewport.nav, h.app.ui.visual_style), state);
    }
    assert!(h.app.dialog.is_none() && h.app.tool.is_none());
    assert_eq!(h.app.session.doc.to_json(), design);
    assert_eq!(h.app.session.history.len(), undo);
}

#[test]
fn navigation_bar_visibility_defaults_and_round_trips() {
    let mut h = Harness::new();
    moved_block(&mut h);
    assert!(h.app.ui.show_navigation_bar);
    assert!(crate::scenario::rect_of("nav:outer").is_some());
    h.call("engine.execute", json!({"command":"view.navigation_bar","params":{"visible":false}}));
    assert!(!h.app.ui.show_navigation_bar);
    assert!(!h.app.viewport.layout.as_ref().unwrap().navigation.is_positive());
    assert!(crate::scenario::rect_of("nav:outer").is_none());
    let mut restored = crate::SolveApp::new(Default::default(), Default::default());
    restored.load_prefs(&h.app.prefs());
    assert!(!restored.ui.show_navigation_bar);
    let mut old = crate::SolveApp::new(Default::default(), Default::default());
    old.load_prefs("{\"grid\":false}");
    assert!(old.ui.show_navigation_bar);
    h.app.start("view.navigation_bar");
    h.settle();
    assert!(h.app.ui.show_navigation_bar);
    assert!(crate::scenario::rect_of("nav:outer").is_some());
}

#[test]
fn navigation_bar_buttons_run_registered_commands() {
    let mut h = Harness::new();
    moved_block(&mut h);
    let undo = h.app.session.history.len();
    let mut orbit_on = false;
    h.call("ui.prefs", json!({"navigation_bar":{"slots":["orbit","pan","zoom","fit","home","visual_style","spin"]}}));
    for (slot, id) in [
        ("orbit", "view.tool"),
        ("orbit", "view.tool"),
        ("pan", "view.tool"),
        ("zoom", "view.tool"),
        ("fit", "view.fit"),
        ("home", "view.home"),
        ("spin", "view.spin"),
    ] {
        let at = crate::scenario::handle_at(&format!("nav:slot:{slot}")).unwrap();
        let log = h.app.session.log.len();
        h.call("ui.click", json!({"x":at.x,"y":at.y}));
        let label = solidcraft_engine::find_command(id).unwrap().label;
        let echo = crate::i18n::Locale::for_language(&h.app.preferences.language).format("ui.command_done", &[("command", label)]);
        assert!(h.app.session.log.iter().skip(log).any(|line| line == &echo), "{id} was not run");
        if slot == "orbit" {
            orbit_on = !orbit_on;
            assert_eq!(h.app.viewport.nav, if orbit_on { Some(crate::viewport::NavMode::Orbit) } else { None });
        }
    }
    let log = h.app.session.log.len();
    h.step(&json!({"click":{"handle":"nav:slot:visual_style"}})).unwrap();
    h.step(&json!({"click":{"handle":"nav:style:shaded"}})).unwrap();
    let label = solidcraft_engine::find_command("view.visual_style").unwrap().label;
    let echo = crate::i18n::Locale::for_language(&h.app.preferences.language).format("ui.command_done", &[("command", label)]);
    assert!(h.app.session.log.iter().skip(log).any(|line| line == &echo));
    assert_eq!(h.app.ui.visual_style, 1);
    assert_eq!(h.app.session.history.len(), undo);
}

#[test]
fn fit_and_home_json_commands_preserve_design_framing() {
    let mut h = Harness::new();
    moved_block(&mut h);
    let design = h.app.session.doc.to_json();
    let undo = h.app.session.history.len();
    for command in ["view.fit", "view.home"] {
        h.app.cam.set_view(StandardView::Front);
        h.app.cam.pan(300.0, 200.0, 500.0);
        let direction = (h.app.cam.yaw, h.app.cam.pitch);
        let r = h.call("engine.execute", json!({"command":command,"params":{}}));
        assert_eq!(r["ok"], true, "{r}");
        assert!(h.app.cam.target.dist(Vec3::new(505.0, 5.0, 2.5)) < 5.0);
        assert!(h.app.cam.distance < 200.0);
        if command == "view.fit" {
            assert_eq!((h.app.cam.yaw, h.app.cam.pitch), direction);
        } else {
            assert!(same_cam(&h.app.cam, &home(&h)));
        }
    }
    assert_eq!(h.app.session.doc.to_json(), design);
    assert_eq!(h.app.session.history.len(), undo);
}

/// #10091: Preferences › Navigation "Home and Fit size" is how much of the view the model fills
/// when Home and Fit (and an opened design or a navigator view change) frame it; 100 %, the
/// default, fills the room between the panels and the controls as before.
#[test]
fn home_fit_size_scales_the_framed_model() {
    let mut h = Harness::new();
    h.app.preferences.look_at_anim = false;
    moved_block(&mut h);
    assert_eq!(h.app.preferences.home_fit_size, 1.0, "fills the room by default");
    h.call("engine.execute", json!({"command":"view.home","params":{}}));
    h.frames(2);
    let tight = h.app.cam;
    h.call("ui.prefs", json!({"home_fit_size":0.8}));
    for command in ["view.home", "view.fit"] {
        h.call("engine.execute", json!({"command":command,"params":{}}));
        h.frames(2);
        assert!((h.app.cam.distance / tight.distance - 1.25).abs() < 0.02, "{command}: {} vs {}", h.app.cam.distance, tight.distance);
        assert_eq!((h.app.cam.yaw, h.app.cam.pitch), (tight.yaw, tight.pitch), "{command}: the size only zooms");
    }
    for bad in [1.5, 0.05, 0.0, -1.0] {
        let r = h.call("ui.prefs", json!({"home_fit_size":bad}));
        assert_eq!(r["ok"], false, "{bad} is outside 10 %…100 %: {r}");
    }
    assert!((h.app.preferences.home_fit_size - 0.8).abs() < 1e-6);
    h.call("ui.prefs", json!({"open":"Navigation"}));
    h.call("ui.prefs", json!({"button":"defaults"}));
    assert_eq!(h.app.preferences.home_fit_size, 1.0, "Navigation defaults restore 100 %");
    h.call("ui.prefs", json!({"button":"cancel"}));
}

#[test]
fn fit_json_frames_selection_while_home_frames_everything() {
    let mut h = Harness::new();
    h.app.preferences.look_at_anim = false;
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Near"})).unwrap();
    h.app.run("solid.box", json!({"corner":[500,0,0],"length":10,"width":10,"height":10,"body_name":"Far"})).unwrap();
    h.settle();
    h.app.run("select.set", json!({"items":[{"type":"body","name":"Near"}]})).unwrap();
    let design = h.app.session.doc.to_json();
    let undo = h.app.session.history.len();
    let r = h.call("engine.execute", json!({"command":"view.fit","params":{}}));
    assert_eq!(r["ok"], true, "{r}");
    let selected = h.app.cam;
    assert!(selected.target.dist(Vec3::new(5.0, 5.0, 5.0)) < 5.0);
    let r = h.call("engine.execute", json!({"command":"view.home","params":{}}));
    assert_eq!(r["ok"], true, "{r}");
    assert!(h.app.cam.distance > selected.distance * 10.0);
    assert!(same_cam(&h.app.cam, &home(&h)));
    assert_eq!(h.app.session.doc.to_json(), design);
    assert_eq!(h.app.session.history.len(), undo);
}

#[test]
fn navigation_bar_display_menu_runs_grid_and_shadow_commands() {
    let mut h = Harness::new();
    moved_block(&mut h);
    h.step(&json!({"click":{"handle":"nav:view"}})).unwrap();
    for (label, id) in [("Grid", "view.grid"), ("Ground shadow", "view.ground_shadow")] {
        let before = if id == "view.grid" { h.app.ui.show_grid } else { h.app.ui.ground_shadow };
        let log = h.app.session.log.len();
        h.step(&json!({"widget":label})).unwrap();
        let after = if id == "view.grid" { h.app.ui.show_grid } else { h.app.ui.ground_shadow };
        assert_eq!(after, !before);
        let echo = crate::i18n::Locale::for_language(&h.app.preferences.language).format("ui.command_done", &[("command", label)]);
        assert!(h.app.session.log.iter().skip(log).any(|line| line == &echo), "{id} was not run");
    }
}

#[test]
fn navigation_bar_toggle_slots_apply_commands_and_projection_compatibility() {
    let mut h = Harness::new();
    moved_block(&mut h);
    h.call("ui.prefs", json!({"navigation_bar":{"slots":["grid","ground_shadow","ambient_occlusion","projection"]}}));
    let design = h.app.session.doc.to_json();
    let undo = h.app.session.history.len();
    for slot in ["grid", "ground_shadow", "ambient_occlusion", "projection"] {
        let value = |app: &crate::SolveApp| match slot {
            "grid" => app.ui.show_grid,
            "ground_shadow" => app.ui.ground_shadow,
            "ambient_occlusion" => app.preferences.ambient_occlusion,
            _ => app.ui.perspective,
        };
        let before = value(&h.app);
        let log = h.app.session.log.len();
        h.step(&json!({"click":{"handle":format!("nav:slot:{slot}")}})).unwrap();
        assert_eq!(value(&h.app), !before, "{slot}");
        if slot != "projection" {
            let command = solidcraft_engine::find_command(&format!("view.{slot}")).unwrap();
            let echo = crate::i18n::Locale::for_language(&h.app.preferences.language).format("ui.command_done", &[("command", command.label)]);
            assert!(h.app.session.log.iter().skip(log).any(|line| line == &echo), "{slot}");
        }
    }
    assert!(h.app.dialog.is_none() && h.app.tool.is_none());
    assert_eq!(h.app.session.doc.to_json(), design);
    assert_eq!(h.app.session.history.len(), undo);
}

#[test]
fn navigation_bar_tool_radio_group_and_overflow_execute_in_list_order() {
    use crate::viewport::NavMode;
    let mut h = Harness::new();
    moved_block(&mut h);
    h.call("ui.prefs", json!({"navigation_bar":{"slots":["zoom","fit","pan","separator","orbit"]}}));
    for (slot, mode) in [("pan", Some(NavMode::Pan)), ("zoom", Some(NavMode::Zoom)), ("orbit", Some(NavMode::Orbit)), ("orbit", None)] {
        h.step(&json!({"click":{"handle":format!("nav:slot:{slot}")}})).unwrap();
        assert_eq!(h.app.viewport.nav, mode);
    }
    h.call("ui.prefs", json!({"navigation_bar":{"slots":["pan","fit","home","visual_style","display","grid","ground_shadow","ambient_occlusion","projection","spin","orbit","zoom"]}}));
    h.app.ui.show_browser = false;
    h.app.ui.show_timeline = false;
    h.call("ui.resize", json!({"width":300,"height":600}));
    h.settle();
    let bar = crate::scenario::rect_of("nav:outer").unwrap();
    assert_eq!(bar, h.app.viewport.layout.as_ref().unwrap().navigation);
    let metrics = crate::ui_kit::NavigationMetrics::for_size(crate::navigation_bar::Size::Regular);
    assert_eq!(h.app.preferences.navigation_bar.size, crate::navigation_bar::Size::Regular);
    assert_eq!(bar.height(), metrics.height());
    let more = crate::scenario::rect_of("nav:more").unwrap();
    assert_eq!(more.size(), egui::vec2(metrics.button, metrics.button));
    let visible: Vec<_> = crate::scenario::handles("nav:slot:").into_iter().map(|(id, _)| id).collect();
    assert!(!visible.is_empty() && visible.len() < h.app.preferences.navigation_bar.slots.len());
    assert_eq!(visible, h.app.preferences.navigation_bar.slots.iter().take(visible.len()).map(|id| format!("nav:slot:{id}")).collect::<Vec<_>>());
    for handle in &visible {
        let slot = crate::scenario::rect_of(handle).unwrap();
        assert_eq!(slot.center().y, more.center().y, "one row: {handle}");
        assert_eq!(slot.height(), metrics.button);
    }
    assert!(!visible.iter().any(|handle| handle == "nav:slot:zoom"), "the final slot folds into More");
    h.step(&json!({"click":{"handle":"nav:more"}})).unwrap();
    let all: Vec<_> = crate::scenario::handles("nav:slot:").into_iter().map(|(id, _)| id).collect();
    assert_eq!(all, h.app.preferences.navigation_bar.slots.iter().map(|id| format!("nav:slot:{id}")).collect::<Vec<_>>());
    assert!(all.len() > visible.len());
    h.step(&json!({"click":{"handle":"nav:slot:zoom"}})).unwrap();
    assert_eq!(h.app.viewport.nav, Some(NavMode::Zoom));
    assert!(crate::scenario::rect_of("nav:slot:zoom").is_none(), "choosing an action closes More");
}

#[test]
fn navigation_bar_look_at_keeps_the_active_sketch_fallback() {
    let mut h = Harness::new();
    h.app.preferences.look_at_anim = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.cam.set_view(StandardView::Iso);
    h.settle();
    let sketch = h.app.session.active_sketch;
    let design = h.app.session.doc.to_json();
    let undo = h.app.session.history.len();
    h.step(&json!({"click":{"handle":"nav:slot:look_at"}})).unwrap();
    assert!(h.app.cam.back().dist(Vec3::Z) < 1e-6);
    assert_eq!(h.app.session.active_sketch, sketch);
    assert_eq!(h.app.session.doc.to_json(), design);
    assert_eq!(h.app.session.history.len(), undo);
}
