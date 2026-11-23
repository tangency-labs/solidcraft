//! The Display States folder in the Browser: the circle shows the active state and applies one,
//! a state's menu updates, renames and deletes it.

use egui::pos2;
use serde_json::json;

use crate::context_menu::{self, Target};
use crate::scenario::Harness;
use crate::test_support::click;

fn harness() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.ui.show_browser = true;
    for (i, n) in ["A", "B"].iter().enumerate() {
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [30 * i, 0, 0], "body_name": n})).unwrap();
    }
    h.app.tree.expanded.insert("displaystates".into());
    h.frames(3);
    h
}

fn menu(h: &mut Harness, target: &Target, label: &str) {
    let items = context_menu::items(&h.app, target);
    let item = items.iter().find(|i| i.label == label).unwrap_or_else(|| panic!("no {label} in {items:?}")).clone();
    context_menu::run_item(&mut h.app, &item, pos2(0.0, 0.0));
    h.frames(3);
}

#[test]
fn the_circle_applies_a_state_and_marks_the_active_one() {
    let mut h = harness();
    h.app.run("display.save", json!({"name": "Both"})).unwrap();
    h.app.run("browser.visibility", json!({"items": ["B"], "visible": false})).unwrap();
    h.app.run("display.save", json!({"name": "Only A"})).unwrap();
    h.frames(3);
    assert_eq!(h.app.session.active_display_state(), Some("Only A"));
    assert!(crate::scenario::handle_at("row:Both").is_some() && crate::scenario::handle_at("row:Only A").is_some());

    click(&mut h, "activate:Both");
    assert!(h.app.session.visibility.hidden_bodies.is_empty(), "the circle applies the state");
    assert_eq!(h.app.session.active_display_state(), Some("Both"));

    // A change by hand leaves no state active.
    h.app.run("browser.visibility", json!({"items": ["A"], "visible": false})).unwrap();
    assert_eq!(h.app.session.active_display_state(), None);
}

#[test]
fn a_state_menu_updates_in_place_renames_and_deletes() {
    let mut h = harness();
    h.app.run("display.save", json!({"name": "First"})).unwrap();
    h.app.run("display.save", json!({"name": "Second"})).unwrap();
    h.app.run("browser.visibility", json!({"items": ["B"], "visible": false})).unwrap();
    let state = Target::DisplayState { name: "First".into() };
    menu(&mut h, &state, "Update to What Is Shown Now");
    let names: Vec<&str> = h.app.session.doc.display_states.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["First", "Second"], "an update keeps the order");
    assert_eq!(h.app.session.doc.display_states[0].snapshot.visibility.hidden_bodies, vec!["B".to_string()]);

    menu(&mut h, &state, "Delete");
    assert!(crate::scenario::handle_at("row:First").is_none());
    assert_eq!(h.app.session.doc.display_states.len(), 1);
}

#[test]
fn a_state_keeps_the_looks_of_the_bodies() {
    let mut h = harness();
    h.app.run("appearance.assign", json!({"bodies": ["A"], "color": "#ff0000"})).unwrap();
    h.app.run("display.save", json!({"name": "Red"})).unwrap();
    h.app.run("appearance.assign", json!({"bodies": ["A"], "color": "#0000ff"})).unwrap();
    h.frames(3);
    click(&mut h, "activate:Red");
    assert_eq!(h.app.session.doc.appearances.bodies["A"].color, [255, 0, 0]);
}
