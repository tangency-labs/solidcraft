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
