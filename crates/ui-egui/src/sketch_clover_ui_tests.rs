//! Rectangles of one size drawn by clicking around a shared corner (a "clover"): each one's
//! corner snaps onto the corners already there and its typed width and height go in. None is
//! refused as over-constraining the sketch (#687).

use serde_json::json;

use crate::scenario::Harness;

fn type_value(h: &mut Harness, text: &str) {
    h.step(&json!({"key": "A", "cmd": true})).unwrap();
    h.step(&json!({"text": text})).unwrap();
}

/// Draw a rectangle from `a` (clicked, snapping onto what is there) towards `b`, typing its
/// width and height when given, else clicking `b`.
fn rectangle(h: &mut Harness, a: [f64; 2], b: [f64; 2], typed: bool) {
    h.step(&json!({"start": "sketch.rectangle.two_point"})).unwrap();
    h.step(&json!({"click": {"world": [a[0], a[1], 0]}})).unwrap();
    h.step(&json!({"move": {"world": [b[0], b[1], 0]}})).unwrap();
    if typed {
        type_value(h, "10");
        h.step(&json!({"key": "Tab"})).unwrap();
        type_value(h, "10");
        h.step(&json!({"key": "Enter"})).unwrap();
    } else {
        h.step(&json!({"click": {"world": [b[0], b[1], 0]}})).unwrap();
    }
    h.step(&json!({"key": "Escape"})).unwrap();
    h.settle();
}

const CLOVER: [([f64; 2], [f64; 2]); 4] = [([0., 0.], [-10., -10.]), ([0., 0.], [10., -10.]), ([0., 0.], [10., 10.]), ([0., 0.], [-10., 10.])];

#[test]
fn a_clover_of_typed_10_by_10_rectangles_is_drawn_and_fully_constrained() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    for (i, (a, b)) in CLOVER.iter().enumerate() {
        rectangle(&mut h, *a, *b, true);
        if let Some((message, _, true)) = &h.app.status {
            panic!("rectangle {i} was refused: {message}");
        }
        let state = h.app.session.world_state().unwrap();
        let solved = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
        assert_eq!(solved.sketch.curves.iter().filter(|c| !c.construction).count(), 4 * (i + 1), "rectangle {i} was not added");
        assert!(solved.report.ok());
    }
    let state = h.app.session.world_state().unwrap();
    let solved = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    assert_eq!(solved.profiles.len(), 4);
    assert!(solved.profiles.iter().all(|p| (p.area - 100.0).abs() < 1e-6), "{:?}", solved.profiles.iter().map(|p| p.area).collect::<Vec<_>>());
    assert!(solved.report.fully_constrained(), "the clover on the origin, every size typed: {:?}", solved.report);
}

#[test]
fn a_clover_drawn_by_clicks_snaps_every_corner_without_redundant_constraints() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    // Clicked, the last one goes from corner to corner of the ones already there, so both its
    // clicks snap (a free click lands only as near as the pointer allows).
    let mut clover = CLOVER;
    clover[3] = ([0., 10.], [-10., 0.]);
    for (a, b) in clover {
        rectangle(&mut h, a, b, false);
    }
    assert!(h.app.status.as_ref().is_none_or(|(_, _, error)| !error), "{:?}", h.app.status);
    let state = h.app.session.world_state().unwrap();
    let solved = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    assert_eq!(solved.profiles.len(), 4);
    assert!(solved.report.ok() && solved.report.redundant.is_empty(), "{:?}", solved.report);
    // On the origin: the two column widths and the two row heights are left.
    assert_eq!(solved.report.dof, 4);
}
