//! The headset's Browser (`vr_browser`): drawn into a context of its own, its eyes and rows
//! answer a pointer as the window's do.

use crate::scenario::Harness;
use serde_json::json;

/// One pass of the headset's Browser in `ctx`, 300 × 480 points, given `events`.
fn pass(h: &mut Harness, ctx: &egui::Context, lit: &mut Vec<String>, events: Vec<egui::Event>) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 480.0))),
        events,
        focused: true,
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |ui| crate::vr_browser::show(&mut h.app, ui, lit));
    // No renderer here: its pictures are let go unapplied.
    out.textures_delta.clear();
}

/// Where the headset's Browser drew `name`'s eye (the latest published wins: its own pass).
fn eye(name: &str) -> egui::Pos2 {
    solidcraft_ui_registry::handle_at(&format!("eye:{name}")).unwrap_or_else(|| panic!("no eye for {name}"))
}

/// A press and release on the eye of `name`, the release `jitter` points off the press.
fn click_eye(h: &mut Harness, ctx: &egui::Context, lit: &mut Vec<String>, name: &str, jitter: f32) {
    pass(h, ctx, lit, Vec::new());
    let at = eye(name);
    click_at(h, ctx, lit, at, jitter);
}

/// A press at `at` and a release `jitter` points off it, the pointer moved there first.
fn click_at(h: &mut Harness, ctx: &egui::Context, lit: &mut Vec<String>, at: egui::Pos2, jitter: f32) {
    let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    pass(h, ctx, lit, vec![egui::Event::PointerMoved(at)]);
    pass(h, ctx, lit, vec![button(at, true)]);
    let off = at + egui::vec2(jitter, jitter * 0.5);
    pass(h, ctx, lit, vec![egui::Event::PointerMoved(off)]);
    pass(h, ctx, lit, vec![button(off, false)]);
    pass(h, ctx, lit, Vec::new());
}

/// Two bodies, the window drawn once.
fn two_bodies() -> Harness {
    let mut h = Harness::new();
    for (name, x) in [("One", 0), ("Two", 20)] {
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[x,0,0],"operation":"new_body","body_name":name})).unwrap();
    }
    h.settle();
    h
}

/// A click on a body's eye in the headset's Browser hides it, and another shows it again.
#[test]
fn an_eye_in_the_headsets_browser_hides_and_shows() {
    let mut h = two_bodies();
    let ctx = egui::Context::default();
    let mut lit = Vec::new();
    click_eye(&mut h, &ctx, &mut lit, "Two", 0.0);
    assert!(h.app.session.visibility.hidden_bodies.iter().any(|b| b == "Two"), "hidden: {:?}", h.app.session.visibility.hidden_bodies);
    click_eye(&mut h, &ctx, &mut lit, "Two", 0.0);
    assert!(!h.app.session.visibility.hidden_bodies.iter().any(|b| b == "Two"));
}

/// A ray shakes as the trigger is pulled: an eye pressed and let go a few points off still
/// toggles once, and only its own row.
#[test]
fn an_eye_pressed_with_a_shaking_ray_still_toggles() {
    let mut h = two_bodies();
    let ctx = egui::Context::default();
    let mut lit = Vec::new();
    click_eye(&mut h, &ctx, &mut lit, "Two", 4.0);
    assert_eq!(h.app.session.visibility.hidden_bodies, vec!["Two".to_string()]);
}

/// A placed part's eye in the headset's Browser hides the placement, as the window's does.
#[test]
fn a_placements_eye_in_the_headsets_browser_hides_it() {
    let mut h = Harness::new();
    h.app.preferences.browser_components_open = true;
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Top"})).unwrap();
    let part = h.app.run("component.create", json!({"name":"Part"})).unwrap()["occurrence"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[20,0,0],"body_name":"Inner"})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    h.settle();
    let ctx = egui::Context::default();
    let mut lit = Vec::new();
    click_eye(&mut h, &ctx, &mut lit, &format!("occurrence:{part}"), 3.0);
    assert!(h.app.session.occurrence_hidden(&[part]), "hidden placements: {:?}", h.app.session.visibility.hidden_occurrences);
}

/// A press and release where the headset's Browser has no row (below its last one) hides
/// nothing, selects nothing and lights nothing.
#[test]
fn a_click_on_no_row_in_the_headsets_browser_changes_nothing() {
    let mut h = two_bodies();
    let ctx = egui::Context::default();
    let mut lit = Vec::new();
    pass(&mut h, &ctx, &mut lit, Vec::new());
    let below = solidcraft_ui_registry::handle_at("row:Two").expect("a row for Two").y + 150.0;
    assert!(below < 470.0, "the window has room below its rows: {below}");
    let (doc, visibility) = (h.app.session.doc.clone(), h.app.session.visibility.clone());
    click_at(&mut h, &ctx, &mut lit, egui::pos2(150.0, below), 0.0);
    assert_eq!(h.app.session.visibility, visibility);
    assert_eq!(h.app.session.doc, doc);
    assert!(h.app.session.selection.is_empty(), "{:?}", h.app.session.selection);
    assert!(lit.is_empty() && h.app.viewport.hover_bodies.is_empty(), "lit: {lit:?}");
}

/// The ray on a body's row in the headset's Browser lights that body in the design, and the
/// window closing (lighting nothing) puts it out again.
#[test]
fn a_row_pointed_at_in_the_headsets_browser_lights_its_body() {
    let mut h = two_bodies();
    let ctx = egui::Context::default();
    let mut lit = Vec::new();
    pass(&mut h, &ctx, &mut lit, Vec::new());
    let row = solidcraft_ui_registry::handle_at("row:One").expect("a row for One");
    pass(&mut h, &ctx, &mut lit, vec![egui::Event::PointerMoved(row)]);
    pass(&mut h, &ctx, &mut lit, Vec::new());
    assert_eq!(lit, vec!["One".to_string()]);
    assert!(h.app.viewport.hover_bodies.iter().any(|b| b == "One"), "{:?}", h.app.viewport.hover_bodies);
    crate::vr_pick::light(&mut h.app, &mut lit, Vec::new());
    assert!(!h.app.viewport.hover_bodies.iter().any(|b| b == "One"), "{:?}", h.app.viewport.hover_bodies);
}

/// The headset's Browser is drawn by a context of its own, but the window's stays the one the
/// Browser's drags and captures are ended through (a document switch ends them there).
#[test]
fn the_headsets_browser_leaves_the_windows_context_in_charge() {
    let mut h = two_bodies();
    let window = egui::Context::default();
    crate::semantic_content::ensure_session(&mut h.app, &window);
    let headset = egui::Context::default();
    let mut lit = Vec::new();
    pass(&mut h, &headset, &mut lit, Vec::new());
    assert!(h.app.tree.content.context() == Some(&window), "the window's context is kept");
}
