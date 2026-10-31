//! A sketch tool's click that moves on the way (#2448): egui takes a press that moves past its
//! click distance as a drag, which the tools dropped, so a hand that moved while clicking placed
//! nothing.

use crate::scenario::Harness;
use serde_json::json;
use solidcraft_engine::sketch::CurveKind;

fn harness(command: &str) -> Harness {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.cam.distance = 120.0;
    h.settle();
    h.step(&json!({"start":command})).unwrap();
    h
}

/// Press `dx` screen points left of sketch point `at`, move there and let go.
fn moving_click(h: &mut Harness, at: [f64; 2], dx: f64) {
    let p = h.call("ui.at", json!({"sketch":at}))["result"].clone();
    let (x, y) = (p[0].as_f64().unwrap(), p[1].as_f64().unwrap());
    h.call("ui.drag", json!({"x0":x - dx,"y0":y,"x1":x,"y1":y,"steps":4}));
}

fn curves(h: &Harness) -> usize {
    let sid = h.app.session.active_sketch.unwrap();
    h.app.session.model.state().sketch(sid).unwrap().sketch.curves.len()
}

#[test]
fn clicks_that_move_place_their_points() {
    for (command, points, finish) in [
        ("sketch.slot.center_to_center", vec![[-20.0, 0.0], [20.0, 0.0], [0.0, 8.0]], None),
        ("sketch.line", vec![[-20.0, 0.0], [20.0, 5.0]], Some("Escape")),
        ("sketch.arc.three_point", vec![[-20.0, 0.0], [20.0, 0.0], [0.0, 10.0]], None),
        ("sketch.rectangle.three_point", vec![[-20.0, 0.0], [20.0, 0.0], [20.0, 10.0]], None),
        ("sketch.circle.three_point", vec![[-20.0, 0.0], [20.0, 0.0], [0.0, 20.0]], None),
        ("sketch.ellipse", vec![[0.0, 0.0], [25.0, 0.0], [0.0, 10.0]], None),
        ("sketch.spline.fit_point", vec![[-20.0, 0.0], [0.0, 10.0], [20.0, 0.0]], Some("right")),
    ] {
        let mut h = harness(command);
        for at in &points {
            moving_click(&mut h, *at, 12.0);
        }
        match finish {
            Some("right") => {
                let p = h.call("ui.at", json!({"sketch":points[points.len() - 1]}))["result"].clone();
                h.call("ui.click", json!({"x":p[0],"y":p[1],"button":"right"}));
            }
            Some(key) => h.step(&json!({"key":key})).unwrap(),
            None => {}
        }
        h.settle();
        assert!(curves(&h) > 0, "{command}: nothing drawn; {:?} {:?}", h.app.tool, h.app.status);
        // The points went where the button was let go: the first one is at its spot.
        let sid = h.app.session.active_sketch.unwrap();
        let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
        let first = solidcraft_engine::geom::Vec2::new(points[0][0], points[0][1]);
        // (A three-point circle keeps only its centre: through all three, its radius is 20.)
        let through = sk.curves.iter().any(|c| matches!(c.kind, CurveKind::Circle { r, .. } if (r - 20.0).abs() < 0.5));
        assert!(sk.points.iter().any(|p| (p.pos - first).len() < 0.5) || through, "{command}: no point at {first:?}");
    }
}

#[test]
fn a_short_drag_starts_a_rectangle_circle_or_polygon_by_clicks() {
    for (command, second, count) in [
        ("sketch.rectangle.two_point", [20.0, 10.0], 4),
        ("sketch.rectangle.center", [20.0, 10.0], 6),
        ("sketch.circle.center", [15.0, 0.0], 1),
        ("sketch.polygon.inscribed", [15.0, 0.0], 7),
    ] {
        let mut h = harness(command);
        // A press that moves 12 points (past egui's click distance, short of a drawn shape) is
        // the first point, not a sliver of a shape.
        moving_click(&mut h, [0.0, 0.0], 12.0);
        h.settle();
        assert_eq!(curves(&h), 0, "{command}: a short drag drew a shape");
        assert_eq!(h.app.tool.as_ref().unwrap().pts.len(), 1, "{command}");
        moving_click(&mut h, second, 12.0);
        h.settle();
        assert_eq!(curves(&h), count, "{command}: {:?}", h.app.status);
        let sid = h.app.session.active_sketch.unwrap();
        let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
        let far = solidcraft_engine::geom::Vec2::new(second[0], second[1]);
        let circle = |c: &solidcraft_engine::sketch::Curve| matches!(c.kind, CurveKind::Circle { r, .. } if (r - far.len()).abs() < 0.5);
        let on_shape = sk.points.iter().any(|p| (p.pos - far).len() < 0.5) || sk.curves.iter().any(circle);
        assert!(on_shape, "{command}: the second click wasn't where it was let go");
    }
}

/// Press at sketch point `from`, drag to `to` and let go.
fn drag(h: &mut Harness, from: [f64; 2], to: [f64; 2]) {
    let a = h.call("ui.at", json!({"sketch":from}))["result"].clone();
    let b = h.call("ui.at", json!({"sketch":to}))["result"].clone();
    h.call("ui.drag", json!({"x0":a[0],"y0":a[1],"x1":b[0],"y1":b[1],"steps":8}));
    h.settle();
}

/// The lines of the sketch being edited, end to end.
fn lines(h: &Harness) -> Vec<[solidcraft_engine::geom::Vec2; 2]> {
    let sid = h.app.session.active_sketch.unwrap();
    let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
    (0..sk.curves.len())
        .filter_map(|i| match sk.shape(i) {
            Some(solidcraft_engine::sketch::Shape::Line { a, b }) => Some([a, b]),
            _ => None,
        })
        .collect()
}

/// #7247: with Preferences › Sketch › Press and drag draws one line, a press, a drag and the
/// release draw one line from the press to the release and end there; the next drag starts a
/// new line, snapping to the first one's end as a click does, and a drag that ends close to
/// horizontal lines up as a click does. Clicks still chain, and a short drag is still a click.
#[test]
fn a_line_drag_draws_one_line_with_the_preference() {
    let mut h = harness("sketch.line");
    h.call("ui.prefs", json!({"line_drag_draws_one": true}));
    drag(&mut h, [-20.0, 0.0], [20.0, 10.0]);
    let drawn = lines(&h);
    assert_eq!(drawn.len(), 1, "one line: {:?}", h.app.status);
    let near = |p: solidcraft_engine::geom::Vec2, q: [f64; 2]| (p - solidcraft_engine::geom::Vec2::new(q[0], q[1])).len() < 0.5;
    assert!(near(drawn[0][0], [-20.0, 0.0]) && near(drawn[0][1], [20.0, 10.0]), "from the press to the release: {drawn:?}");
    let tool = h.app.tool.as_ref().expect("the Line tool stays on");
    assert_eq!((tool.cmd, tool.pts.len()), ("sketch.line", 0), "no chain goes on from the line's end");
    // The next drag starts on the first line's end (a little off it) and snaps there.
    drag(&mut h, [20.3, 10.2], [20.0, 30.0]);
    let drawn = lines(&h);
    assert_eq!(drawn.len(), 2);
    assert!((drawn[1][0] - drawn[0][1]).len() < 1e-9, "the press snapped to the end: {drawn:?}");
    let sid = h.app.session.active_sketch.unwrap();
    let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
    assert!(
        sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::Vertical { .. })),
        "the nearly vertical drag lined up as a click does: {:?}",
        sk.constraints
    );
    assert_eq!(h.app.tool.as_ref().map(|t| t.pts.len()), Some(0));
    // A short drag is still a click: the chain starts where it is let go, and clicks go on.
    moving_click(&mut h, [-20.0, -20.0], 12.0);
    assert_eq!(lines(&h).len(), 2, "a short drag drew nothing");
    assert_eq!(h.app.tool.as_ref().map(|t| t.pts.len()), Some(1));
    for at in [[0.0, -25.0], [20.0, -20.0]] {
        let p = h.call("ui.at", json!({"sketch":at}))["result"].clone();
        h.call("ui.click", json!({"x":p[0],"y":p[1]}));
        h.settle();
    }
    assert_eq!(lines(&h).len(), 4, "clicks chain as before");
    assert_eq!(h.app.tool.as_ref().map(|t| t.pts.len()), Some(1), "the chain is still open");
}

/// Off, as by default, a drag in the Line tool is a click that moved on the way (#2448): the
/// chain's first point goes where it is let go, and nothing is drawn yet.
#[test]
fn without_the_preference_a_line_drag_is_a_click_where_it_is_let_go() {
    let mut h = harness("sketch.line");
    assert!(!h.app.preferences.line_drag_draws_one, "off by default");
    drag(&mut h, [-20.0, 0.0], [20.0, 10.0]);
    assert!(lines(&h).is_empty());
    let tool = h.app.tool.as_ref().unwrap();
    assert_eq!(tool.pts.len(), 1);
    assert!((tool.pts[0].0 - solidcraft_engine::geom::Vec2::new(20.0, 10.0)).len() < 0.5, "{:?}", tool.pts);
}

/// The preference saves, and the Sketch page's checkbox, Restore Defaults and Cancel treat it as
/// the page's own.
#[test]
fn the_line_drag_preference_saves_and_follows_the_sketch_page() {
    use crate::prefs::control;
    let mut h = Harness::new();
    h.call("ui.prefs", json!({"open": "Sketch"}));
    h.frames(4);
    crate::prefs::reveal_preference(&mut h, "prefs:line_drag_draws_one");
    h.step(&json!({"click":{"handle":"prefs:line_drag_draws_one"}})).unwrap();
    h.frames(2);
    assert!(h.app.preferences.line_drag_draws_one, "the checkbox turns it on");
    control(&mut h.app, &json!({"button": "apply"})).unwrap();
    let mut other = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    other.load_prefs(&h.app.prefs());
    assert!(other.preferences.line_drag_draws_one, "saved");
    control(&mut h.app, &json!({"button": "defaults"})).unwrap();
    assert!(!h.app.preferences.line_drag_draws_one);
    control(&mut h.app, &json!({"button": "cancel"})).unwrap();
    assert!(h.app.preferences.line_drag_draws_one);
}

// ---- The Line tool's switch to tangent arcs and back (#9489) ----

/// The Line tool in a sketch with one segment drawn, (-20, 0) to (0, 0), the pointer on its end.
fn chain_with_a_line(on: bool) -> Harness {
    let mut h = harness("sketch.line");
    h.app.preferences.line_hover_tangent_arc = on;
    click_at(&mut h, [-20.0, 0.0]);
    click_at(&mut h, [0.0, 0.0]);
    assert_eq!(curves(&h), 1, "{:?}", h.app.status);
    h
}

fn click_at(h: &mut Harness, at: [f64; 2]) {
    h.step(&json!({"move":{"sketch":at}})).unwrap();
    h.frames(2);
    h.step(&json!({"click":{"sketch":at}})).unwrap();
    h.frames(2);
}

fn tool(h: &Harness) -> Option<&'static str> {
    h.app.tool.as_ref().map(|t| t.cmd)
}

/// Move off the point, back onto it, and rest there for `frames` frames at 60 Hz.
fn leave_and_rest(h: &mut Harness, on: [f64; 2], frames: usize) {
    h.step(&json!({"move":{"sketch":[on[0] + 15.0, on[1] + 15.0]}})).unwrap();
    h.frames(2);
    h.step(&json!({"move":{"sketch":on}})).unwrap();
    h.frames(frames);
}

/// A rest long enough for the switch: about half a second (it takes 0.35 s).
const REST: usize = 30;

fn sketch(h: &Harness) -> solidcraft_engine::sketch::Sketch {
    let sid = h.app.session.active_sketch.unwrap();
    h.app.session.model.state().sketch(sid).unwrap().sketch.clone()
}

/// The arc last drawn: centre, radius and where it starts and ends as drawn.
fn last_arc(
    sk: &solidcraft_engine::sketch::Sketch,
) -> (solidcraft_engine::geom::Vec2, f64, solidcraft_engine::geom::Vec2, solidcraft_engine::geom::Vec2) {
    let (index, curve) = sk.curves.iter().enumerate().rfind(|(_, c)| matches!(c.kind, CurveKind::Arc { .. })).expect("an arc");
    let CurveKind::Arc { c, a, b } = curve.kind else { panic!("not an arc: {curve:?}") };
    let centre = sk.point(c).unwrap();
    // As drawn: a clockwise arc is stored the other way round.
    let (from, to) = if curve.reversed { (b, a) } else { (a, b) };
    let (start, end) = (sk.point(from).unwrap(), sk.point(to).unwrap());
    assert!(index > 0, "the arc follows a line");
    (centre, centre.dist(start), start, end)
}

#[test]
fn off_by_default_resting_on_the_last_point_does_nothing() {
    assert!(!crate::prefs::Prefs::default().line_hover_tangent_arc, "off by default");
    let mut h = chain_with_a_line(false);
    leave_and_rest(&mut h, [0.0, 0.0], REST * 2);
    assert_eq!(tool(&h), Some("sketch.line"), "the tool stays the Line tool");
    assert_eq!(crate::scenario::count_of("line_arc_mode_chip"), None, "and nothing shows at the cursor");
    // The next click is a line, as ever.
    click_at(&mut h, [0.0, 20.0]);
    assert_eq!(curves(&h), 2);
    assert!(sketch(&h).curves.iter().all(|c| matches!(c.kind, CurveKind::Line { .. })));
}

#[test]
fn a_rest_on_the_last_point_makes_the_next_click_a_tangent_arc_and_back_to_a_line() {
    let mut h = chain_with_a_line(true);
    leave_and_rest(&mut h, [0.0, 0.0], REST);
    assert_eq!(tool(&h), Some("sketch.arc.tangent"), "{:?}", h.app.status);
    assert_eq!(crate::scenario::count_of("line_arc_mode_chip"), Some(1.0), "the cursor says Tangent Arc");
    assert_eq!(h.app.tool.as_ref().and_then(crate::sketch_line_arc::shown_label), Some("Tangent Arc"), "in words, not a message id");

    // The line runs along +X into (0, 0): the tangent arc to (10, 10) has its centre on the
    // normal there, (0, 10), at radius 10, and leaves running along +X.
    click_at(&mut h, [10.0, 10.0]);
    assert_eq!(curves(&h), 2, "{:?}", h.app.status);
    let sk = sketch(&h);
    let (centre, radius, start, end) = last_arc(&sk);
    assert!(centre.dist(solidcraft_engine::geom::Vec2::new(0.0, 10.0)) < 1e-6, "centre {centre:?}");
    assert!((radius - 10.0).abs() < 1e-6, "radius {radius}");
    assert!(start.dist(solidcraft_engine::geom::Vec2::ZERO) < 1e-6 && end.dist(solidcraft_engine::geom::Vec2::new(10.0, 10.0)) < 1e-6);
    let (line, arc) = (sk.curves.first().unwrap(), sk.curves.get(1).unwrap());
    let shared = sk.resolve_point(&format!("{}.end", line.id)) == sk.resolve_point(&format!("{}.start", arc.id));
    assert!(shared, "the arc starts on the line's own end");
    assert!(
        sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::Tangent { a: 0, b: 1 })),
        "tangent to the line: {:?}",
        sk.constraints
    );
    assert_eq!(tool(&h), Some("sketch.arc.tangent"), "it keeps drawing arcs until switched back");

    // The arc's end is the chain's point; the same gesture goes back to lines.
    leave_and_rest(&mut h, [10.0, 10.0], REST);
    assert_eq!(tool(&h), Some("sketch.line"), "{:?}", h.app.status);
    assert_eq!(crate::scenario::count_of("line_arc_mode_chip"), Some(2.0), "the cursor says Line");
    assert_eq!(h.app.tool.as_ref().and_then(crate::sketch_line_arc::shown_label), Some("Line"));
    click_at(&mut h, [10.0, 30.0]);
    assert_eq!(curves(&h), 3, "{:?}", h.app.status);
    let sk = sketch(&h);
    let (arc, line) = (sk.curves.get(1).unwrap(), sk.curves.get(2).unwrap());
    assert!(matches!(line.kind, CurveKind::Line { .. }));
    let shared = sk.resolve_point(&format!("{}.end", arc.id)) == sk.resolve_point(&format!("{}.start", line.id));
    assert!(shared, "the line starts on the arc's end");
    // And the chain goes on from there as lines.
    click_at(&mut h, [30.0, 30.0]);
    assert_eq!(curves(&h), 4);
    assert_eq!(tool(&h), Some("sketch.line"));
}

#[test]
fn a_rounded_corner_outline_closes_in_one_line_session() {
    let mut h = harness("sketch.line");
    h.app.preferences.line_hover_tangent_arc = true;
    click_at(&mut h, [-20.0, -20.0]);
    click_at(&mut h, [10.0, -20.0]);
    leave_and_rest(&mut h, [10.0, -20.0], REST);
    click_at(&mut h, [20.0, -10.0]);
    leave_and_rest(&mut h, [20.0, -10.0], REST);
    click_at(&mut h, [20.0, 20.0]);
    leave_and_rest(&mut h, [20.0, 20.0], REST);
    click_at(&mut h, [10.0, 30.0]);
    leave_and_rest(&mut h, [10.0, 30.0], REST);
    click_at(&mut h, [-20.0, 30.0]);
    leave_and_rest(&mut h, [-20.0, 30.0], REST);
    click_at(&mut h, [-30.0, 20.0]);
    leave_and_rest(&mut h, [-30.0, 20.0], REST);
    click_at(&mut h, [-30.0, -10.0]);
    leave_and_rest(&mut h, [-30.0, -10.0], REST);
    // The last arc ends on the first line's start: the outline is closed.
    click_at(&mut h, [-20.0, -20.0]);
    let sk = sketch(&h);
    assert_eq!(sk.curves.len(), 8, "{:?}", h.app.status);
    let tangents = sk.constraints.iter().filter(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::Tangent { .. })).count();
    assert!(tangents >= 4, "every arc is tangent to the line before it: {tangents}");
    let first = sk.resolve_point(&format!("{}.start", sk.curves[0].id));
    let last = sk.resolve_point(&format!("{}.end", sk.curves[7].id));
    assert_eq!(first, last, "the last arc ends on the first point");
}

#[test]
fn a_pointer_that_never_left_or_keeps_moving_or_rests_elsewhere_does_not_switch() {
    // A pause straight after the click, on the point it placed.
    let mut h = chain_with_a_line(true);
    h.frames(REST * 3);
    assert_eq!(tool(&h), Some("sketch.line"), "a pause on the placed point switches nothing");

    // A little jitter there isn't leaving it either.
    h.step(&json!({"move":{"sketch":[0.5, 0.5]}})).unwrap();
    h.frames(REST * 2);
    assert_eq!(tool(&h), Some("sketch.line"), "a small wobble is not leaving");

    // Resting somewhere else.
    h.step(&json!({"move":{"sketch":[20.0, 20.0]}})).unwrap();
    h.frames(REST * 2);
    assert_eq!(tool(&h), Some("sketch.line"), "a rest elsewhere");
    h.step(&json!({"move":{"sketch":[-20.0, 0.0]}})).unwrap();
    h.frames(REST * 2);
    assert_eq!(tool(&h), Some("sketch.line"), "a rest on the line's other end");

    // Over the point, never still: each move restarts the wait.
    let p = h.call("ui.at", json!({"sketch":[0.0, 0.0]}))["result"].clone();
    let (x, y) = (p[0].as_f64().unwrap(), p[1].as_f64().unwrap());
    for i in 0..80 {
        let d = if i % 2 == 0 { 3.0 } else { -3.0 };
        h.call("ui.move", json!({"x": x + d, "y": y}));
        h.frames(1);
    }
    assert_eq!(tool(&h), Some("sketch.line"), "a moving pointer never switches");

    // Too short a rest.
    leave_and_rest(&mut h, [0.0, 0.0], 5);
    assert_eq!(tool(&h), Some("sketch.line"), "a tenth of a second is not a rest");

    // Before any segment there is no chain to switch.
    let mut h = harness("sketch.line");
    h.app.preferences.line_hover_tangent_arc = true;
    click_at(&mut h, [-20.0, 0.0]);
    leave_and_rest(&mut h, [-20.0, 0.0], REST * 2);
    assert_eq!(tool(&h), Some("sketch.line"), "the first point alone switches nothing");

    // Nor is the Tangent Arc tool itself switched by resting on its start.
    let mut h = harness("sketch.line");
    h.app.preferences.line_hover_tangent_arc = true;
    click_at(&mut h, [-20.0, 0.0]);
    click_at(&mut h, [0.0, 0.0]);
    h.step(&json!({"key":"Escape"})).unwrap();
    h.step(&json!({"start":"sketch.arc.tangent"})).unwrap();
    click_at(&mut h, [0.0, 0.0]);
    leave_and_rest(&mut h, [0.0, 0.0], REST * 2);
    assert_eq!(tool(&h), Some("sketch.arc.tangent"));
    assert!(h.app.tool.as_ref().is_some_and(|t| !crate::sketch_line_arc::in_arc_mode(t)), "that tool is no chain");
}

#[test]
fn staying_on_the_point_after_a_switch_does_not_switch_back() {
    let mut h = chain_with_a_line(true);
    leave_and_rest(&mut h, [0.0, 0.0], REST);
    assert_eq!(tool(&h), Some("sketch.arc.tangent"));
    h.frames(REST * 4);
    assert_eq!(tool(&h), Some("sketch.arc.tangent"), "the pointer has to leave first");
}

#[test]
fn esc_and_right_click_end_a_chain_drawing_arcs_as_they_end_a_line_chain() {
    for right_click in [false, true] {
        let mut h = chain_with_a_line(true);
        leave_and_rest(&mut h, [0.0, 0.0], REST);
        click_at(&mut h, [10.0, 10.0]);
        assert_eq!(tool(&h), Some("sketch.arc.tangent"));
        if right_click {
            let p = h.call("ui.at", json!({"sketch":[20.0, 20.0]}))["result"].clone();
            h.call("ui.click", json!({"x":p[0],"y":p[1],"button":"right"}));
        } else {
            h.step(&json!({"key":"Escape"})).unwrap();
        }
        h.frames(2);
        // The chain has ended and the Line tool is out, with no point placed.
        let t = h.app.tool.as_ref().expect("the Line tool stays out");
        assert_eq!((t.cmd, t.pts.len()), ("sketch.line", 0), "right click: {right_click}");
        // A click starts a new chain: a separate line, and a rest needs a segment again.
        click_at(&mut h, [30.0, 0.0]);
        click_at(&mut h, [40.0, 0.0]);
        assert_eq!(curves(&h), 3);
        // Esc twice leaves the tool, as ever.
        h.step(&json!({"key":"Escape"})).unwrap();
        h.step(&json!({"key":"Escape"})).unwrap();
        h.frames(2);
        assert_eq!(tool(&h), None);
    }
}

#[test]
fn undo_takes_back_one_arc_as_it_takes_back_one_segment() {
    let mut h = chain_with_a_line(true);
    let undo_before = h.app.session.undo.len();
    leave_and_rest(&mut h, [0.0, 0.0], REST);
    assert_eq!(h.app.session.undo.len(), undo_before, "switching changes nothing in the design");
    click_at(&mut h, [10.0, 10.0]);
    assert_eq!(curves(&h), 2);
    assert_eq!(h.app.session.undo.len(), undo_before + 1, "the arc is one undo step");
    h.app.run("edit.undo", json!({})).unwrap();
    h.frames(2);
    assert_eq!(curves(&h), 1, "only the arc goes");
    assert_eq!(sketch(&h).curves.first().map(|c| matches!(c.kind, CurveKind::Line { .. })), Some(true));
    // As with a line undone mid-chain, the chain's point went with it: the next click is refused
    // and changes nothing, and Esc ends the chain, leaving the Line tool ready for a new one.
    click_at(&mut h, [20.0, 20.0]);
    assert_eq!(curves(&h), 1, "{:?}", h.app.status);
    h.step(&json!({"key":"Escape"})).unwrap();
    h.frames(2);
    assert_eq!(h.app.tool.as_ref().map(|t| (t.cmd, t.pts.len())), Some(("sketch.line", 0)));
    click_at(&mut h, [30.0, 0.0]);
    click_at(&mut h, [40.0, 0.0]);
    assert_eq!(curves(&h), 2, "{:?}", h.app.status);
}

/// The preference saves, and the Sketch page's checkbox, Restore Defaults and Cancel treat it as
/// the page's own.
#[test]
fn the_tangent_arc_preference_saves_and_follows_the_sketch_page() {
    use crate::prefs::control;
    let mut h = Harness::new();
    h.call("ui.prefs", json!({"open": "Sketch"}));
    h.frames(4);
    crate::prefs::reveal_preference(&mut h, "prefs:line_hover_tangent_arc");
    h.step(&json!({"click":{"handle":"prefs:line_hover_tangent_arc"}})).unwrap();
    h.frames(2);
    assert!(h.app.preferences.line_hover_tangent_arc, "the checkbox turns it on");
    control(&mut h.app, &json!({"button": "apply"})).unwrap();
    let mut other = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    other.load_prefs(&h.app.prefs());
    assert!(other.preferences.line_hover_tangent_arc, "saved");
    control(&mut h.app, &json!({"button": "defaults"})).unwrap();
    assert!(!h.app.preferences.line_hover_tangent_arc);
    control(&mut h.app, &json!({"button": "cancel"})).unwrap();
    assert!(h.app.preferences.line_hover_tangent_arc);
}

#[test]
fn arcs_follow_one_another_each_tangent_to_the_last_and_a_pause_after_one_switches_nothing() {
    let mut h = chain_with_a_line(true);
    leave_and_rest(&mut h, [0.0, 0.0], REST);
    click_at(&mut h, [10.0, 10.0]);
    // The pointer is on the arc's end, where it was clicked: thinking there switches nothing.
    h.frames(REST * 3);
    assert_eq!(tool(&h), Some("sketch.arc.tangent"), "a pause on the placed point switches nothing");
    // The first arc arrives at (10, 10) running along +Y, so the next one, to (20, 20), turns the
    // other way about (20, 10) at radius 10: an S.
    click_at(&mut h, [20.0, 20.0]);
    assert_eq!(curves(&h), 3, "{:?}", h.app.status);
    let sk = sketch(&h);
    let (centre, radius, start, end) = last_arc(&sk);
    assert!(centre.dist(solidcraft_engine::geom::Vec2::new(20.0, 10.0)) < 1e-6, "centre {centre:?}");
    assert!((radius - 10.0).abs() < 1e-6, "radius {radius}");
    assert!(
        start.dist(solidcraft_engine::geom::Vec2::new(10.0, 10.0)) < 1e-6 && end.dist(solidcraft_engine::geom::Vec2::new(20.0, 20.0)) < 1e-6,
        "from {start:?} to {end:?}"
    );
    assert!(
        sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::Tangent { a: 1, b: 2 })),
        "tangent to the arc before it: {:?}",
        sk.constraints
    );
    // After a clockwise arc the chain still goes on from its drawn end, as a line.
    leave_and_rest(&mut h, [20.0, 20.0], REST);
    assert_eq!(tool(&h), Some("sketch.line"), "{:?}", h.app.status);
    click_at(&mut h, [20.0, 40.0]);
    assert_eq!(curves(&h), 4, "{:?}", h.app.status);
    let sk = sketch(&h);
    let CurveKind::Line { a: line_start, .. } = sk.curves[3].kind else { panic!("a line: {:?}", sk.curves[3]) };
    let CurveKind::Arc { a: arc_end, .. } = sk.curves[2].kind else { panic!("an arc: {:?}", sk.curves[2]) };
    let joined = line_start == arc_end || sk.point(line_start).unwrap().dist(sk.point(arc_end).unwrap()) < 1e-6;
    assert!(joined, "the line starts where the arc ends");
}
