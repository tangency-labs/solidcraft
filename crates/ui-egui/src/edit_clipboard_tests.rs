//! Copy and Paste from the keyboard and the context menu (#1480).

use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::geom::Vec2;

use crate::scenario::Harness;

fn strs(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// A sketch on XY with a 20 × 10 rectangle (dimensioned, its corner on the origin), looked at
/// from the top, and the rectangle selected.
fn rectangle() -> (Harness, Vec<String>) {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    let rect = strs(&h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]})).unwrap()["curves"]);
    h.app.run("sketch.constraint.coincident", json!({"a": format!("{}.start", rect[0]), "b": "origin"})).unwrap();
    h.app.run("sketch.dimension", json!({"entities": [rect[0]], "value": 20})).unwrap();
    h.app.run("sketch.dimension", json!({"entities": [rect[1]], "value": 10})).unwrap();
    h.call("ui.view", json!({"view": "top"}));
    h.call("ui.camera", json!({"target": [20, 10, 0], "distance": 150}));
    let items: Vec<Value> = rect.iter().map(|id| json!({"type": "sketch_curve", "id": id})).collect();
    h.app.run("select.set", json!({ "items": items })).unwrap();
    h.frames(4);
    (h, rect)
}

/// Send the window system's clipboard event (what Ctrl+C / Ctrl+V become), with the pointer at
/// `at` (sketch coordinates) or off the viewport.
fn clipboard_event(h: &mut Harness, event: egui::Event, at: Option<[f64; 2]>) {
    match at {
        Some(p) => h.step(&json!({"move": {"sketch": p}})).unwrap(),
        None => drop(h.call("ui.move", json!({"x": 100, "y": 990}))),
    }
    h.app.synthetic.push(event);
    h.settle();
}

fn curves(h: &Harness) -> Vec<String> {
    h.app.session.doc.sketch(h.app.session.active_sketch.unwrap()).unwrap().curves.iter().map(|c| c.id.clone()).collect()
}

fn start(h: &Harness, id: &str) -> Vec2 {
    let sk = h.app.session.doc.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    let c = &sk.curves[sk.curve_index(id).unwrap()];
    sk.point(c.kind.point_ids()[0]).unwrap()
}

fn clipboard_frame(h: &mut Harness, ctx: &egui::Context, events: Vec<egui::Event>) {
    let _ = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| super::handle(&mut h.app, ui.ctx()));
}

fn middle_button(pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos: egui::pos2(500.0, 500.0), button: egui::PointerButton::Middle, pressed, modifiers: egui::Modifiers::NONE }
}

#[test]
fn middle_button_press_and_hold_do_not_paste() {
    let (mut h, _) = rectangle();
    assert!(super::copy(&mut h.app));
    let before = curves(&h);
    let undo = h.app.session.undo.len();
    let ctx = egui::Context::default();
    clipboard_frame(&mut h, &ctx, vec![middle_button(true), egui::Event::Paste("primary selection".into())]);
    assert_eq!(curves(&h), before, "middle-button press must only navigate");
    clipboard_frame(&mut h, &ctx, vec![egui::Event::Paste("primary selection".into())]);
    assert_eq!(curves(&h), before, "holding the middle button must only navigate");
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
fn middle_button_release_and_single_frame_click_do_not_paste() {
    for single_frame in [false, true] {
        let (mut h, _) = rectangle();
        assert!(super::copy(&mut h.app));
        let before = curves(&h);
        let undo = h.app.session.undo.len();
        let ctx = egui::Context::default();
        let mut events = vec![middle_button(true)];
        if !single_frame {
            clipboard_frame(&mut h, &ctx, std::mem::take(&mut events));
        }
        events.extend([middle_button(false), egui::Event::Paste("primary selection".into())]);
        clipboard_frame(&mut h, &ctx, events);
        assert_eq!(curves(&h), before, "middle-button release must not paste (single frame: {single_frame})");
        assert_eq!(h.app.session.undo.len(), undo);
    }
}

#[test]
fn keyboard_paste_still_works_with_and_without_middle_button() {
    for modifiers in [egui::Modifiers::CTRL | egui::Modifiers::COMMAND, egui::Modifiers::MAC_CMD | egui::Modifiers::COMMAND] {
        let (mut h, _) = rectangle();
        assert!(super::copy(&mut h.app));
        let before = curves(&h).len();
        let ctx = egui::Context::default();
        clipboard_frame(
            &mut h,
            &ctx,
            vec![middle_button(true), egui::Event::Key { key: egui::Key::V, physical_key: None, pressed: true, repeat: false, modifiers }],
        );
        assert_eq!(curves(&h).len(), before + 4, "explicit keyboard paste works while navigating");
        clipboard_frame(&mut h, &ctx, vec![middle_button(false)]);
        clipboard_frame(&mut h, &ctx, vec![egui::Event::Paste("keyboard clipboard".into())]);
        assert_eq!(curves(&h).len(), before + 8, "keyboard paste events work after navigation ends");
    }
}

#[test]
fn ctrl_c_ctrl_v_pastes_the_sketch_selection_at_the_cursor_selected_in_one_undo_step() {
    let (mut h, rect) = rectangle();
    let before = curves(&h);
    let undo = h.app.session.undo.len();
    clipboard_event(&mut h, egui::Event::Copy, Some([10.0, 5.0]));
    assert!(h.app.session.sketch_clipboard.is_some(), "{:?}", h.app.status);
    // The window system only sends Paste while its clipboard holds text: a copy leaves some.
    h.call("ui.inspect", json!({}));
    clipboard_event(&mut h, egui::Event::Paste("x".into()), Some([60.0, 40.0]));
    let after = curves(&h);
    assert_eq!(after.len(), before.len() + 4, "{:?}", h.app.status);
    let pasted: Vec<String> = after.iter().filter(|c| !before.contains(c)).cloned().collect();
    // Centred at the cursor: the rectangle (centre 10, 5) goes to (60, 40), give or take the snap.
    let corner = start(&h, &pasted[0]);
    assert!(corner.dist(Vec2::new(50.0, 35.0)) < 1.0, "{corner:?}");
    // The originals stay put, with their constraints.
    assert!(start(&h, &rect[0]).dist(Vec2::ZERO) < 1e-9);
    // Selected, ready to move.
    let selected: Vec<String> =
        h.app.session.selection.iter().filter_map(|s| if let Sel::SketchCurve { id, .. } = s { Some(id.clone()) } else { None }).collect();
    assert_eq!(selected, pasted);
    // One undo step.
    assert_eq!(h.app.session.undo.len(), undo + 1);
    h.step(&json!({"key": "Z", "cmd": true})).unwrap();
    assert_eq!(curves(&h), before);
}

#[test]
fn copy_leaves_text_on_the_system_clipboard_so_paste_can_follow() {
    let (mut h, _) = rectangle();
    h.step(&json!({"move": {"sketch": [10, 5]}})).unwrap();
    h.app.synthetic.push(egui::Event::Copy);
    let mut copied = Vec::new();
    for _ in 0..6 {
        h.frame();
        copied.extend(h.output.commands.iter().filter_map(|c| match c {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        }));
    }
    assert!(copied.iter().any(|t| t.starts_with("SolidCraft sketch geometry")), "{copied:?}");
}

#[test]
fn paste_away_from_the_viewport_offsets_from_the_original_and_cmd_keys_work_too() {
    let (mut h, rect) = rectangle();
    let before = curves(&h);
    // Ctrl+C / Ctrl+V as key presses (as the control channel and some platforms send them).
    h.step(&json!({"move": {"sketch": [10, 5]}})).unwrap();
    h.step(&json!({"key": "C", "cmd": true})).unwrap();
    h.call("ui.move", json!({"x": 100, "y": 990}));
    h.step(&json!({"key": "V", "cmd": true})).unwrap();
    let pasted: Vec<String> = curves(&h).into_iter().filter(|c| !before.contains(c)).collect();
    assert_eq!(pasted.len(), 4, "{:?}", h.app.status);
    // 20 mm wide: a 2 mm step down and to the right.
    let d = start(&h, &pasted[0]) - start(&h, &rect[0]);
    assert!(d.dist(Vec2::new(2.0, -2.0)) < 1e-6, "{d:?}");
}

#[test]
fn the_context_menu_copies_and_pastes_sketch_geometry_at_the_click() {
    let (mut h, rect) = rectangle();
    let before = curves(&h);
    h.step(&json!({"click": {"sketch": [10, 0]}, "button": "right"})).unwrap();
    let menu = h.call("ui.menu", json!({}));
    let labels: Vec<&str> = menu["result"]["items"].as_array().unwrap().iter().filter_map(|i| i["label"].as_str()).collect();
    assert!(labels.contains(&"Copy") && labels.contains(&"Paste"), "{labels:?}");
    h.call("ui.menuPick", json!({"item": "Copy"}));
    assert!(h.app.session.sketch_clipboard.is_some());
    h.step(&json!({"click": {"sketch": [70, 30]}, "button": "right"})).unwrap();
    h.call("ui.menuPick", json!({"item": "Paste"}));
    h.frames(3);
    let pasted: Vec<String> = curves(&h).into_iter().filter(|c| !before.contains(c)).collect();
    assert!(!pasted.is_empty(), "{:?}", h.app.status);
    // Whatever the right-click selected was copied, and its copy is centred on the click.
    let sk = h.app.session.doc.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    let pts: Vec<Vec2> = pasted.iter().flat_map(|c| sk.curves[sk.curve_index(c).unwrap()].kind.point_ids()).map(|i| sk.point(i).unwrap()).collect();
    let (lo, hi) = pts.iter().fold((Vec2::new(1e9, 1e9), Vec2::new(-1e9, -1e9)), |(lo, hi), p| {
        (Vec2::new(lo.x.min(p.x), lo.y.min(p.y)), Vec2::new(hi.x.max(p.x), hi.y.max(p.y)))
    });
    assert!(((lo + hi) * 0.5).dist(Vec2::new(70.0, 30.0)) < 1.0, "{lo:?} {hi:?}");
    assert!(start(&h, &rect[0]).dist(Vec2::ZERO) < 1e-9);
}

#[test]
fn ctrl_c_ctrl_v_outside_a_sketch_copies_and_pastes_timeline_features() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    h.frames(3);
    let id = h.app.session.doc.features[0].id;
    h.app.run("select.set", json!({"items": [{"type": "feature", "id": id}]})).unwrap();
    h.frames(2);
    clipboard_event(&mut h, egui::Event::Copy, None);
    clipboard_event(&mut h, egui::Event::Paste("x".into()), None);
    assert_eq!(h.app.session.doc.features.len(), 2, "{:?}", h.app.status);
}

#[test]
fn ctrl_x_cuts_the_sketch_selection_and_ctrl_v_brings_it_back() {
    let (mut h, rect) = rectangle();
    let before = curves(&h);
    clipboard_event(&mut h, egui::Event::Cut, Some([10.0, 5.0]));
    let left = curves(&h);
    assert!(rect.iter().all(|c| !left.contains(c)), "{:?}", h.app.status);
    assert!(h.app.session.sketch_clipboard.is_some());
    clipboard_event(&mut h, egui::Event::Paste("x".into()), Some([10.0, 5.0]));
    assert_eq!(curves(&h).len(), before.len(), "{:?}", h.app.status);
    // The context menu offers Cut too.
    h.step(&json!({"click": {"sketch": [10, 0]}, "button": "right"})).unwrap();
    let menu = h.call("ui.menu", json!({}));
    let labels: Vec<&str> = menu["result"]["items"].as_array().unwrap().iter().filter_map(|i| i["label"].as_str()).collect();
    assert!(labels.contains(&"Cut"), "{labels:?}");
}

struct ContributionPart(std::path::PathBuf);
impl ContributionPart {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        Self(std::env::temp_dir().join(format!("solidcraft-linked-clipboard-{}-{stamp}.solidcraft", std::process::id())))
    }
}
impl Drop for ContributionPart {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn contribution_saved_part(file: &ContributionPart) -> crate::SolveApp {
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    app.run("solid.box", json!({"length":20,"width":10,"height":5,"body_name":"Plate"})).unwrap();
    app.run("file.save", json!({"path":file.0.to_string_lossy()})).unwrap();
    app.tree.picked_components = vec![0];
    app
}

#[test]
fn contribution_linked_design_clipboard_crosses_tabs_reuses_links_and_undoes_once() {
    let file = ContributionPart::new();
    let mut app = contribution_saved_part(&file);
    assert!(super::copy(&mut app));
    crate::documents::open_tab(&mut app, solidcraft_engine::Session::default()).unwrap();
    assert!(super::can_paste(&app));
    for n in 1..=2 {
        assert!(super::paste(&mut app, None), "{:?}", app.status);
        assert_eq!(app.session.doc.components.len(), 1);
        assert_eq!(app.session.doc.links.len(), 1);
        assert_eq!(app.session.doc.features.len(), 1);
        assert_eq!(app.session.doc.occurrences.len(), n);
        assert_eq!(app.session.undo.len(), n);
        assert!(app.session.doc.occurrences.iter().all(|o| !o.grounded));
        assert!(app.tree.occurrence_move.is_some());
    }
    app.tree.occurrence_move = None;
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(app.session.doc.occurrences.len(), 1);
    app.run("edit.undo", json!({})).unwrap();
    assert!(app.session.doc.links.is_empty());
    assert!(app.session.doc.components.is_empty());
    app.run("edit.redo", json!({})).unwrap();
    assert_eq!(app.session.doc.links.len(), 1);
    crate::documents::switch(&mut app, 0);
    assert_eq!(app.session.doc.features.len(), 1, "the source is untouched");
}

#[test]
fn contribution_occurrence_move_panel_belongs_to_its_document() {
    let file = ContributionPart::new();
    let mut app = contribution_saved_part(&file);
    assert!(super::copy(&mut app));
    crate::documents::open_tab(&mut app, solidcraft_engine::Session::default()).unwrap();
    assert!(super::paste(&mut app, None));
    let id = app.tree.occurrence_move.as_ref().unwrap().occurrence;
    app.run("occurrence.move", json!({"occurrence":id,"translate":[10,0,0],"capture":false})).unwrap();
    let pending = app.session.pending_moves.clone();
    crate::documents::switch(&mut app, 0);
    assert!(app.tree.occurrence_move.is_none());
    assert!(app.session.doc.occurrences.is_empty());
    crate::documents::switch(&mut app, 1);
    assert_eq!(app.tree.occurrence_move.as_ref().unwrap().occurrence, id);
    assert_eq!(app.session.pending_moves, pending);
}

#[test]
fn contribution_linked_paste_uses_the_pointer_and_pending_numeric_move() {
    use solidcraft_engine::geom::Vec3;
    let file = ContributionPart::new();
    let mut h = crate::scenario::Harness::new();
    h.app = contribution_saved_part(&file);
    assert!(super::copy(&mut h.app));
    crate::documents::open_tab(&mut h.app, solidcraft_engine::Session::default()).unwrap();
    h.app.run("solid.box", json!({"length":80,"width":50,"height":8,"body_name":"Destination"})).unwrap();
    h.app.fit_view();
    h.frames(3);
    let projection = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let pointed = Vec3::new(40., 25., 8.);
    let at = projection.to_screen(pointed).unwrap();
    let undo = h.app.session.undo.len();
    assert!(super::paste(&mut h.app, Some(at)));
    h.frames(3);
    let path = h.app.tree.occurrence_move.as_ref().unwrap().path.clone();
    let initial = solidcraft_engine::doc::apply_point(&h.app.session.occurrence_placement(&path).unwrap().world, Vec3::ZERO);
    assert!((initial - pointed).len() < 1e-5, "{initial:?}");
    assert_eq!(h.app.session.undo.len(), undo + 1);
    let camera = h.app.cam;
    let field = h.call("ui.at", json!({"handle":"occurrence-move:distance:0"}));
    h.call("ui.click", json!({"x":field["result"][0],"y":field["result"][1],"double":true}));
    h.frames(2);
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"12"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    h.frames(3);
    let moved = solidcraft_engine::doc::apply_point(&h.app.session.occurrence_placement(&path).unwrap().world, Vec3::ZERO);
    assert!(moved.x > initial.x + 10., "numeric move must retain the pasted occurrence: {initial:?} -> {moved:?}");
    assert_eq!(h.app.cam, camera, "numeric editing does not move the camera");
    let movement = h.app.tree.occurrence_move.as_ref().unwrap();
    assert!((movement.translate[0] - (moved.x - initial.x)).abs() < 1e-5);
    assert_eq!(h.app.session.undo.len(), undo + 1, "pending movement does not split the initial paste undo");
    h.app.run("edit.undo", json!({})).unwrap();
    h.frames(3);
    assert!(h.app.session.doc.links.is_empty());
    assert!(h.app.tree.occurrence_move.is_none());
    crate::documents::switch(&mut h.app, 0);
    assert_eq!(h.app.session.doc.features.len(), 1, "source geometry stays untouched");
}

#[test]
fn contribution_paste_pointer_uses_parent_coordinates_and_safe_fallbacks() {
    use solidcraft_engine::{doc, geom::Vec3};
    let mut h = crate::scenario::Harness::new();
    let parent = h.app.run("component.create", json!({"name":"Parent","grounded":false})).unwrap();
    h.app
        .run("occurrence.move", json!({"occurrence":parent["occurrence"],"translate":[20,30,10],"axis":[1,0,0],"angle":"45 deg","capture":true}))
        .unwrap();
    h.frames(3);
    let path = vec![parent["occurrence"].as_u64().unwrap()];
    h.app.run("component.activate", json!({"component":parent["component"],"occurrence":parent["occurrence"],"path":path})).unwrap();
    let (actual, zero) = super::paste_placement(&h.app, None);
    assert_eq!(actual, path);
    assert_eq!(zero, [0.; 3]);
    assert_eq!(super::paste_placement(&h.app, Some(egui::pos2(-100., -100.))).1, [0.; 3]);
    let frame = h.app.session.occurrence_placement(&path).unwrap().world;
    let target = doc::apply_point(&frame, Vec3::new(5., 7., 0.));
    h.app.cam.target = target;
    h.app.cam.distance = 100.;
    h.frames(2);
    let projection = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let at = projection.to_screen(target).unwrap();
    let (_, local) = super::paste_placement(&h.app, Some(at));
    assert!((Vec3::new(local[0], local[1], local[2]) - Vec3::new(5., 7., 0.)).len() < 1e-5, "{local:?}");
}

#[test]
fn contribution_linked_clipboard_refuses_unsaved_dirty_self_and_missing_sources() {
    let file = ContributionPart::new();
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    assert!(!super::copy_design(&mut app));
    app = contribution_saved_part(&file);
    assert!(super::copy_design(&mut app));
    assert!(!super::can_paste(&app), "Paste is off in the design copied: it can't go into itself");
    let doc = app.session.doc.clone();
    assert!(!super::paste(&mut app, None));
    assert_eq!(app.session.doc, doc);
    app.run("solid.box", json!({"length":2,"width":2,"height":2,"corner":[30,0,0]})).unwrap();
    assert!(!super::copy_design(&mut app));
    crate::documents::open_tab(&mut app, solidcraft_engine::Session::default()).unwrap();
    std::fs::remove_file(&file.0).unwrap();
    assert!(!super::paste(&mut app, None));
    assert!(app.session.doc.features.is_empty());
    assert!(app.session.undo.is_empty());
}

#[test]
fn contribution_feature_copy_replaces_the_linked_design_clipboard() {
    let file = ContributionPart::new();
    let mut app = contribution_saved_part(&file);
    assert!(super::copy(&mut app));
    app.tree.picked_components.clear();
    app.session.selection = vec![Sel::Feature { id: app.session.doc.features[0].id }];
    assert!(super::copy(&mut app));
    assert!(app.linked_design_clipboard.is_none());
    assert!(!app.session.clipboard.is_empty());
}

#[test]
fn contribution_native_copy_paste_events_insert_the_saved_design_in_another_tab() {
    let file = ContributionPart::new();
    let mut h = Harness::new();
    h.app = contribution_saved_part(&file);
    h.frames(3);
    clipboard_event(&mut h, egui::Event::Copy, None);
    assert!(h.app.linked_design_clipboard.is_some());
    crate::documents::open_tab(&mut h.app, solidcraft_engine::Session::default()).unwrap();
    clipboard_event(&mut h, egui::Event::Paste("SolidCraft linked design".into()), None);
    assert_eq!(h.app.session.doc.links.len(), 1, "{:?}", h.app.status);
    assert_eq!(h.app.session.undo.len(), 1);
}

/// A copy that fails (here, with nothing selected) leaves the clipboard empty: Paste is off, and
/// what was copied before is not pasted.
#[test]
fn a_failed_copy_empties_the_clipboard() {
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    app.run("solid.box", json!({"length":2,"width":2,"height":2,"body_name":"Cube"})).unwrap();
    app.run("select.set", json!({"items": [{"type": "body", "name": "Cube"}]})).unwrap();
    assert!(super::copy(&mut app));
    assert!(super::can_paste(&app));
    app.run("select.clear", json!({})).unwrap();
    assert!(!super::copy(&mut app));
    assert!(app.copied_bodies.is_empty());
    assert!(!super::can_paste(&app), "nothing is left to paste");
    assert!(!super::paste(&mut app, None));
    assert_eq!(app.session.model.state().bodies.len(), 1);
}

/// Cut on a body only marks it; Paste moves it into the active component (where it is), and the
/// clipboard empties. Esc before that leaves it where it was; a locked body is not cut.
#[test]
fn a_cut_body_moves_into_the_component_it_is_pasted_in() {
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    app.run("solid.box", json!({"length":2,"width":2,"height":2,"body_name":"Cube"})).unwrap();
    let part = app.run("component.create", json!({"name":"Part","activate":false})).unwrap()["component"].as_u64().unwrap();
    let owner = |app: &crate::SolveApp| {
        let state = app.session.model.state();
        app.session.doc.model_body_component(state.body("Cube").unwrap())
    };
    app.run("select.set", json!({"items": [{"type": "body", "name": "Cube"}]})).unwrap();
    assert!(super::can_cut(&app));
    assert!(super::cut(&mut app));
    assert_eq!(owner(&app), 0, "a cut body stays until it is pasted");
    super::drop_cut(&mut app);
    assert!(!super::can_paste(&app), "Esc left it where it was");
    app.run("select.set", json!({"items": [{"type": "body", "name": "Cube"}]})).unwrap();
    assert!(super::cut(&mut app));
    app.run("component.activate", json!({"component": part})).unwrap();
    assert!(super::paste(&mut app, None));
    assert_eq!(owner(&app), part);
    assert_eq!(app.session.model.state().bodies.len(), 1, "moved, not copied");
    assert!(app.cut_bodies.is_empty() && !super::can_paste(&app));
    app.session.visibility.locked_bodies.push("Cube".into());
    app.run("select.set", json!({"items": [{"type": "body", "name": "Cube"}]})).unwrap();
    assert!(!super::can_cut(&app) && !super::cut(&mut app), "a locked body stays");
}

/// A component placed once cuts and pastes into another component, where it is; it never pastes
/// into itself, and an instance (a component placed more than once) isn't cut.
#[test]
fn a_cut_component_moves_into_the_component_it_is_pasted_in() {
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    let made = |app: &mut crate::SolveApp, name: &str| {
        let out = app.run("component.create", json!({"name": name, "activate": false})).unwrap();
        (out["component"].as_u64().unwrap(), out["occurrence"].as_u64().unwrap())
    };
    let (part, part_at) = made(&mut app, "Part");
    let (holder, holder_at) = made(&mut app, "Holder");
    app.run("select.set", json!({"items": [{"type": "occurrence", "path": [part_at]}]})).unwrap();
    assert!(super::can_cut(&app));
    assert!(super::cut(&mut app));
    assert!(!super::can_paste_into(&app, part), "not into itself");
    assert!(super::can_paste_into(&app, holder));
    assert!(super::paste_into(&mut app, None, false, Some((holder, vec![holder_at]))));
    assert_eq!(app.session.doc.occurrences.iter().find(|o| o.id == part_at).unwrap().parent, holder);
    assert_eq!(app.session.selection, vec![solidcraft_engine::Sel::Occurrence { path: vec![holder_at, part_at] }]);
    assert!(!crate::browser::is_open(&app, &format!("o:{holder_at}/{part_at}"), true), "it arrives folded");
    assert!(app.cut_placements.is_empty() && !super::can_paste(&app));
    app.run("occurrence.copy", json!({"component": holder})).unwrap();
    app.run("select.set", json!({"items": [{"type": "occurrence", "path": [holder_at]}]})).unwrap();
    assert!(!super::can_cut(&app) && !super::cut(&mut app), "an instance isn't cut");
}

/// A copied body pastes into a component's Bodies folder from the folder's menu: the copy goes
/// into that component. With nothing copied, the folder's Paste is off.
#[test]
fn a_copied_body_pastes_into_a_bodies_folder() {
    use crate::context_menu::{Target, items, run_item};
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    app.run("solid.box", json!({"length":2,"width":2,"height":2,"body_name":"Cube"})).unwrap();
    let out = app.run("component.create", json!({"name":"Part","activate":false})).unwrap();
    let (part, at) = (out["component"].as_u64().unwrap(), out["occurrence"].as_u64().unwrap());
    let folder = Target::Folder { component: part, folder: "bodies".into(), scope: format!("o:{at}") };
    let paste = |app: &crate::SolveApp| items(app, &folder).into_iter().find(|i| i.id == "ui.paste").unwrap();
    assert!(!paste(&app).enabled, "nothing copied yet");
    app.run("select.set", json!({"items": [{"type": "body", "name": "Cube"}]})).unwrap();
    assert!(super::copy(&mut app));
    let item = paste(&app);
    assert!(item.enabled);
    run_item(&mut app, &item, egui::Pos2::ZERO);
    let state = app.session.model.state();
    let copy = state.body("Cube (2)").expect("the copy");
    assert_eq!(app.session.doc.model_body_component(copy), part);
}

/// A copied sketch pastes into a component's Sketches folder from the folder's menu: the copy,
/// named after it, goes into that component.
#[test]
fn a_copied_sketch_pastes_into_a_sketches_folder() {
    use crate::context_menu::{Target, items, run_item};
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    app.run("sketch.create", json!({"plane": "XY", "name": "S"})).unwrap();
    app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]})).unwrap();
    app.run("sketch.finish", json!({})).unwrap();
    let out = app.run("component.create", json!({"name":"Part","activate":false})).unwrap();
    let (part, at) = (out["component"].as_u64().unwrap(), out["occurrence"].as_u64().unwrap());
    let folder = Target::Folder { component: part, folder: "sketches".into(), scope: format!("o:{at}") };
    let paste = |app: &crate::SolveApp| items(app, &folder).into_iter().find(|i| i.id == "ui.paste").unwrap();
    let id = app.session.doc.find_feature("S").unwrap().id;
    app.run("select.set", json!({"items": [{"type": "feature", "id": id}]})).unwrap();
    assert!(super::copy(&mut app));
    let item = paste(&app);
    assert!(item.enabled);
    run_item(&mut app, &item, egui::Pos2::ZERO);
    let copy = app.session.doc.find_feature("S (2)").expect("the copy");
    assert_eq!(copy.component, part);
}

fn plain_app() -> crate::SolveApp {
    crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default())
}

fn select(app: &mut crate::SolveApp, item: Value) {
    app.run("select.set", json!({"items": [item]})).unwrap();
}

/// Copied bodies belong to the design they were copied in: another tab has nothing of them to
/// paste, and their own design still pastes them when it is back.
#[test]
fn copied_bodies_stay_with_their_design() {
    let mut app = plain_app();
    app.run("solid.box", json!({"length":2,"width":2,"height":2,"body_name":"Cube"})).unwrap();
    select(&mut app, json!({"type": "body", "name": "Cube"}));
    assert!(super::copy(&mut app));
    crate::documents::open_tab(&mut app, solidcraft_engine::Session::default()).unwrap();
    assert!(!super::can_paste(&app), "nothing of the other design pastes here");
    assert!(!super::paste(&mut app, None));
    assert!(app.session.model.state().bodies.is_empty());
    crate::documents::switch(&mut app, 0);
    assert!(super::can_paste(&app));
    assert!(super::paste(&mut app, None));
    assert!(app.session.model.state().body("Cube (2)").is_some());
}

/// Whatever was copied last is what pastes: a component copied after a body replaces it, and a
/// body copied after that replaces the component.
#[test]
fn the_last_copy_is_what_pastes() {
    let mut app = plain_app();
    app.run("solid.box", json!({"length":2,"width":2,"height":2,"body_name":"Cube"})).unwrap();
    let at = app.run("component.create", json!({"name":"Part","activate":false})).unwrap()["occurrence"].as_u64().unwrap();
    select(&mut app, json!({"type": "body", "name": "Cube"}));
    assert!(super::copy(&mut app));
    select(&mut app, json!({"type": "occurrence", "path": [at]}));
    assert!(super::copy(&mut app));
    assert!(app.copied_bodies.is_empty() && app.copied_placements == vec![vec![at]]);
    let placements = app.session.doc.occurrences.len();
    assert!(super::paste(&mut app, None));
    assert_eq!(app.session.doc.occurrences.len(), placements + 1, "the component pasted");
    assert!(app.session.model.state().body("Cube (2)").is_none(), "not the body");
    select(&mut app, json!({"type": "body", "name": "Cube"}));
    assert!(super::copy(&mut app));
    assert!(app.copied_placements.is_empty());
    assert!(super::paste(&mut app, None));
    assert!(app.session.model.state().body("Cube (2)").is_some(), "now the body");
    assert_eq!(app.session.doc.occurrences.len(), placements + 1);
}

/// Paste on a group puts what was pasted in that group: a body in a Bodies group, a sketch in a
/// Sketches group.
#[test]
fn pasting_on_a_group_puts_the_copy_in_it() {
    use crate::context_menu::{Target, items, run_item};
    let mut app = plain_app();
    app.run("solid.box", json!({"length":2,"width":2,"height":2,"body_name":"Cube"})).unwrap();
    app.run("sketch.create", json!({"plane": "XY", "name": "S"})).unwrap();
    app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]})).unwrap();
    app.run("sketch.finish", json!({})).unwrap();
    let bodies = app.run("browser.group", json!({"folder": "bodies", "name": "Kept"})).unwrap()["group"].as_u64().unwrap();
    let sketches = app.run("browser.group", json!({"folder": "sketches", "name": "Drawn"})).unwrap()["group"].as_u64().unwrap();
    let paste_on = |app: &mut crate::SolveApp, group: u64| {
        let item = items(app, &Target::Group { id: group, scope: "c0".into() }).into_iter().find(|i| i.id == "ui.paste").unwrap();
        assert!(item.enabled);
        run_item(app, &item, egui::Pos2::ZERO);
    };
    let holds = |app: &crate::SolveApp, group: u64, key: &str| {
        app.session.doc.browser_groups.iter().any(|g| g.id == group && g.items.iter().any(|k| k == key))
    };
    select(&mut app, json!({"type": "body", "name": "Cube"}));
    assert!(super::copy(&mut app));
    paste_on(&mut app, bodies);
    assert!(holds(&app, bodies, "Cube (2)"));
    let id = app.session.doc.find_feature("S").unwrap().id;
    select(&mut app, json!({"type": "feature", "id": id}));
    assert!(super::copy(&mut app));
    paste_on(&mut app, sketches);
    let copy = app.session.doc.find_feature("S (2)").unwrap().id;
    assert!(holds(&app, sketches, &copy.to_string()));
}

/// While a component is cut, Paste moves it and Paste New (an independent copy) is off.
#[test]
fn paste_new_is_off_while_a_component_is_cut() {
    use crate::context_menu::{Target, items};
    let mut app = plain_app();
    let at = app.run("component.create", json!({"name":"Part","activate":false})).unwrap()["occurrence"].as_u64().unwrap();
    let holder = app.run("component.create", json!({"name":"Holder","activate":false})).unwrap()["component"].as_u64().unwrap();
    select(&mut app, json!({"type": "occurrence", "path": [at]}));
    assert!(super::cut(&mut app));
    let menu = items(&app, &Target::Component { id: holder });
    let find = |id: &str| menu.iter().find(|i| i.id == id).unwrap();
    assert!(find("ui.paste").enabled);
    assert!(!find("ui.pasteNew").enabled);
}

/// A body of a component's second placement is that component's body seen there: it isn't cut on
/// its own (that would take it out of every placement), but it copies, and pastes where that
/// placement shows it.
#[test]
fn a_second_placements_body_copies_but_is_not_cut() {
    let mut app = plain_app();
    let made = app.run("component.create", json!({"name":"B"})).unwrap();
    let b = made["component"].as_u64().unwrap();
    app.run("solid.box", json!({"length":2,"width":2,"height":2,"body_name":"Cube"})).unwrap();
    app.run("component.activate", json!({"component": 0})).unwrap();
    let second = app.run("occurrence.copy", json!({"component": b, "translate": [30, 0, 0]})).unwrap()["occurrence"].as_u64().unwrap();
    let placed = app.session.occurrence_placement(&[second]).unwrap();
    let seen = placed.bodies.first().unwrap().world.clone();
    assert_ne!(seen, "Cube", "the second placement shows it under a name of its own");
    select(&mut app, json!({"type": "body", "name": seen}));
    assert!(!super::can_cut(&app));
    assert!(!super::cut(&mut app));
    assert!(app.cut_bodies.is_empty() && !super::can_paste(&app));
    assert!(app.status.as_ref().is_some_and(|(text, _, error)| *error && text.contains("placed copy")), "{:?}", app.status);
    select(&mut app, json!({"type": "body", "name": seen}));
    assert!(super::copy(&mut app));
    assert!(super::paste(&mut app, None), "{:?}", app.status);
    let world = app.session.world_state();
    let (there, copy) = (world.body(&seen).unwrap().mesh().bounds(), world.body("Cube (2)").expect("the copy").mesh().bounds());
    assert!(there.min.dist(copy.min) < 1e-6 && there.max.dist(copy.max) < 1e-6, "{there:?} vs {copy:?}");
}
