//! The strip says what the thing in hand answers to, and only what is in hand answers.

use serde_json::json;

use crate::scenario::Harness;

fn app() -> Harness {
    let mut h = Harness::new();
    // Drawing a primitive belongs to Plasticity's controls (`solid_draw::start`).
    crate::plasticity::apply(&mut h.app, crate::plasticity::SCHEME_PLASTICITY);
    h.app.set_origin_shown(false);
    h.settle();
    h
}

fn keys(h: &Harness) -> Vec<&'static str> {
    crate::key_hints::current(&h.app).map(|(_, hints)| hints.iter().map(|x| x.key).collect()).unwrap_or_default()
}

fn line(h: &Harness) -> String {
    crate::key_hints::current(&h.app).map(|(line, _)| line).unwrap_or_default()
}

#[test]
fn nothing_in_hand_says_nothing() {
    let h = app();
    assert_eq!(crate::key_hints::current(&h.app), None);
    // And no key is taken, so the commands that carry those letters still work.
    let mut h = h;
    for key in ["Alt+S", "Alt+C", "Q", "W", "Backspace"] {
        assert!(!crate::key_hints::handle_key(&mut h.app, key), "{key}");
    }
}

#[test]
fn a_sketch_tool_names_its_own_keys() {
    let mut h = app();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.start("sketch.rectangle.two_point");
    h.settle();
    assert!(line(&h).contains("Rectangle"), "{}", line(&h));
    assert_eq!(keys(&h), ["Alt+S", "Alt+C", "Esc"], "a rectangle offers square and centre");
    // Backspace is offered once there is a point to take back.
    crate::tools::place(&mut h.app, (solidcore_record::Vec2::new(0.0, 0.0), None));
    assert!(keys(&h).contains(&"Backspace"), "{:?}", keys(&h));
}

#[test]
fn a_tool_without_those_keys_offers_only_what_it_has() {
    let mut h = app();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.start("sketch.circle.center");
    h.settle();
    let keys = keys(&h);
    assert!(!keys.contains(&"Alt+S") && !keys.contains(&"Alt+C"), "a circle is not a rectangle: {keys:?}");
    assert!(keys.contains(&"Esc"), "{keys:?}");
}

#[test]
fn a_shape_being_drawn_is_asked_before_a_sketch_tool() {
    let mut h = app();
    h.app.start("solid.box");
    h.settle();
    assert!(h.app.solid_draw.is_some() && h.app.tool.is_none());
    // The drawing's own keys, not a sketch tool's.
    assert_eq!(keys(&h), ["B", "Q", "W", "Shift+E", "Shift+Q", "T", "Backspace", "Esc"]);
    assert!(crate::key_hints::handle_key(&mut h.app, "W"), "the drawing takes it");
}
