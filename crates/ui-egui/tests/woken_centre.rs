use serde_json::json;
use solidcraft_ui_egui::scenario::Harness;

fn sketch() -> Harness {
    round_sketch(false)
}

fn round_sketch(arc: bool) -> Harness {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    if arc {
        h.app.run("sketch.arc.center_point", json!({"center":[20,20],"start":[30,20],"end":[20,30]})).unwrap();
    } else {
        h.app.run("sketch.circle.center", json!({"center":[20,20],"radius":10})).unwrap();
    }
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

/// The pointer leaves the circle: past its radius from the centre, the centre sleeps again
/// (#16346). Just outside the circle it stays awake, so the pointer can still cross it.
#[test]
fn the_centre_sleeps_when_the_pointer_leaves_the_circle() {
    let mut h = sketch();
    h.step(&json!({"move":{"world":[30,20,0]}})).unwrap();
    h.frames(2);
    assert!(h.app.viewport.woken_centre.is_some(), "the centre wakes");
    // Just outside the circle, still within its radius from the centre: it stays awake.
    h.step(&json!({"move":{"world":[31,20,0]}})).unwrap();
    h.frames(2);
    assert!(h.app.viewport.woken_centre.is_some(), "the centre stays awake near the circle");
    // Past the radius from the centre: it sleeps.
    h.step(&json!({"move":{"world":[33,20,0]}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.viewport.woken_centre, None, "the centre sleeps");
}

/// Clicking empty space deselects, and the centre goes back to sleep with the selection
/// (#16346): the click is inside the circle, away from the centre.
#[test]
fn the_centre_sleeps_when_the_user_clicks_empty_space() {
    let mut h = sketch();
    h.step(&json!({"move":{"world":[30,20,0]}})).unwrap();
    h.frames(2);
    assert!(h.app.viewport.woken_centre.is_some(), "the centre wakes");
    h.step(&json!({"click":{"world":[26,20,0]}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.viewport.woken_centre, None, "the centre sleeps");
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

/// Check the painted selection ring, rather than the hover state that can disappear separately.
fn centre_is_highlighted(h: &mut Harness, centre: [f64; 2]) -> bool {
    let [x, y] = h.at(&json!({"sketch":centre})).unwrap();
    fn ring(shape: &egui::Shape, at: egui::Pos2) -> bool {
        match shape {
            egui::Shape::Circle(c) => c.center.distance(at) < 0.5 && (5.0..=7.0).contains(&c.radius) && c.stroke.width > 0.0,
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| ring(shape, at)),
            _ => false,
        }
    }
    h.shapes.iter().any(|shape| ring(&shape.shape, egui::pos2(x as f32, y as f32)))
}

#[test]
fn selected_arcs_and_circles_keep_their_centres_highlighted_until_deselected() {
    for arc in [false, true] {
        let mut h = round_sketch(arc);
        h.step(&json!({"click":{"sketch":[27.0710678,27.0710678]}})).unwrap();
        let selection = h.app.session.selection.clone();
        assert!(matches!(selection.as_slice(), [solidcraft_engine::Sel::SketchCurve { .. }]));
        // Outside the curve's radius, then over an unrelated line: only the hover sleeps.
        for point in [[33, 20], [38, 20]] {
            h.step(&json!({"move":{"sketch":point}})).unwrap();
            h.frames(2);
            assert_eq!(h.app.session.selection, selection);
            assert_eq!(h.app.viewport.woken_centre, None);
            assert!(centre_is_highlighted(&mut h, [20.0, 20.0]), "arc={arc}, pointer={point:?}: selected centre stays highlighted");
        }
        h.step(&json!({"click":{"sketch":[33,25]}})).unwrap();
        h.frames(2);
        assert!(h.app.session.selection.is_empty());
        assert!(!centre_is_highlighted(&mut h, [20.0, 20.0]), "deselecting removes the centre's ring");
    }
}

#[test]
fn selecting_two_round_curves_keeps_both_centres_highlighted() {
    let mut h = round_sketch(true);
    h.app.run("sketch.circle.center", json!({"center":[60,20],"radius":10})).unwrap();
    h.call("ui.view", json!({"view":"fit"}));
    h.frames(3);
    h.step(&json!({"click":{"sketch":[27.0710678,27.0710678]}})).unwrap();
    h.step(&json!({"click":{"sketch":[67.0710678,27.0710678]},"shift":true})).unwrap();
    h.step(&json!({"move":{"sketch":[38,20]}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.session.selection.len(), 2);
    for centre in [[20.0, 20.0], [60.0, 20.0]] {
        assert!(centre_is_highlighted(&mut h, centre), "selected centre {centre:?}");
    }
    h.step(&json!({"click":{"sketch":[27.0710678,27.0710678]},"shift":true})).unwrap();
    h.step(&json!({"move":{"sketch":[38,20]}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.session.selection.len(), 1);
    assert!(!centre_is_highlighted(&mut h, [20.0, 20.0]));
    assert!(centre_is_highlighted(&mut h, [60.0, 20.0]));
}

#[test]
fn a_selected_curve_from_another_sketch_does_not_highlight_the_local_centre() {
    use solidcraft_engine::Sel;

    let mut h = sketch();
    let first = h.app.session.active_sketch.unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[20,20],"radius":10})).unwrap();
    let active = h.app.session.active_sketch.unwrap();
    h.step(&json!({"move":{"sketch":[33,20]}})).unwrap();
    for item in [Sel::SketchCurve { id: "c1".into(), sketch: Some(first) }, Sel::ExternalSketchCurve { sketch: first, id: "c1".into() }] {
        h.app.session.selection = vec![item];
        h.frames(2);
        assert!(!centre_is_highlighted(&mut h, [20.0, 20.0]));
    }
    h.app.session.selection = vec![Sel::SketchCurve { id: "c1".into(), sketch: Some(active) }];
    h.frames(2);
    assert!(centre_is_highlighted(&mut h, [20.0, 20.0]));
}
