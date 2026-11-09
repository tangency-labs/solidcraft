use serde_json::json;
use solidcraft_ui_egui::scenario::Harness;

fn sketch() -> Harness {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[20,20],"radius":10})).unwrap();
    h.app.run("sketch.line", json!({"points":[[38,10],[38,30]],"infer":false,"connect":false})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.frames(3);
    h
}

/// A line's first point: the circle's centre, or a point of its own.
fn starts_at_centre(h: &Harness) -> bool {
    let st = h.app.session.world_state().unwrap();
    let sk = &st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    let centre = sk.resolve_point("c1.center").unwrap();
    sk.curves.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::CurveKind::Line { a, b } if a == centre || b == centre))
}

/// Hovering a circle's edge wakes its centre: it is drawn, and a click 12 points from it (beyond
/// a point's usual reach) takes it, here as the start of a line.
#[test]
fn hovering_a_circles_edge_makes_its_centre_easy_to_take() {
    for wake in [false, true] {
        let mut h = sketch();
        h.step(&json!({"start":"sketch.line"})).unwrap();
        if wake {
            h.step(&json!({"move":{"world":[30,20,0]}})).unwrap();
            h.frames(2);
            assert!(h.at(&json!({"handle":"woken_centre"})).is_ok(), "the centre shows");
        }
        let [cx, cy] = h.at(&json!({"world":[20,20,0]})).unwrap();
        h.step(&json!({"move":[cx + 12.0, cy]})).unwrap();
        h.step(&json!({"click":[cx + 12.0, cy]})).unwrap();
        h.step(&json!({"click":{"world":[20,27,0]}})).unwrap();
        h.step(&json!({"key":"Escape"})).unwrap();
        assert_eq!(starts_at_centre(&h), wake, "wake={wake}");
    }
}

/// Another curve hovered puts the centre back to sleep.
#[test]
fn another_curve_puts_the_centre_back_to_sleep() {
    let mut h = sketch();
    h.step(&json!({"move":{"world":[30,20,0]}})).unwrap();
    h.frames(2);
    assert!(h.app.viewport.woken_centre.is_some());
    // Across the circle's inside toward the centre it stays awake.
    h.step(&json!({"move":{"world":[25,20,0]}})).unwrap();
    h.frames(2);
    assert!(h.app.viewport.woken_centre.is_some());
    h.step(&json!({"move":{"world":[38,20,0]}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.viewport.woken_centre, None);
}
