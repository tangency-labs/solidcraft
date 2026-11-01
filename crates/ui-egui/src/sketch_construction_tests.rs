use crate::scenario::Harness;
use serde_json::json;
use solidcraft_engine::sketch::CurveKind;

fn sketch() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[20,0]],"ids":["a"],"infer":false})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[0,20]],"ids":["b"],"infer":false})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[40,0],"radius":5,"id":"c"})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[60,0],"radius":5,"id":"d"})).unwrap();
    h.settle();
    h
}

fn steps(h: &mut Harness, id: &str, clicks: &[[f64; 2]]) {
    h.step(&json!({"start":id})).unwrap();
    for q in clicks {
        h.step(&json!({"click":{"sketch":q}})).unwrap();
    }
}

#[test]
fn every_construction_variant_is_interactive_and_stays_infinite() {
    let cases: &[(&str, &[[f64; 2]])] = &[
        ("sketch.construction_line", &[[5., 10.], [15., 15.]]),
        ("sketch.construction_line.horizontal", &[[5., 10.]]),
        ("sketch.construction_line.vertical", &[[5., 10.]]),
        ("sketch.construction_line.angle", &[[5., 10.], [15., 20.]]),
        ("sketch.construction_line.parallel", &[[10., 0.], [5., 10.]]),
        ("sketch.construction_line.perpendicular", &[[10., 0.], [5., 10.]]),
        ("sketch.construction_line.tangent_external", &[[40., 5.], [50., 10.], [50., 15.]]),
        ("sketch.construction_line.tangent_at", &[[40., 5.], [45., 0.]]),
        ("sketch.construction_line.tangent_two", &[[40., 5.], [60., 5.], [50., 8.]]),
        ("sketch.construction_line.bisector", &[[10., 0.], [0., 10.], [10., 10.]]),
    ];
    for (id, clicks) in cases {
        let mut h = sketch();
        crate::sketch_palette::set_mode(&mut h.app, 2);
        let sid = h.app.session.active_sketch.unwrap();
        let before = h.app.session.doc.sketch(sid).unwrap().curves.len();
        steps(&mut h, id, clicks);
        let sk = h.app.session.doc.sketch(sid).unwrap();
        assert_eq!(sk.curves.len(), before + 1, "{id}: {:?}", h.app.tool);
        let curve = sk.curves.last().unwrap();
        assert!(curve.infinite && curve.construction && !curve.centerline, "{id}: {curve:?}");
        assert!(matches!(curve.kind, CurveKind::Line { .. }));
    }
}

#[test]
fn world_axes_are_not_mistaken_for_sketch_axes() {
    // The world Y axis is sketch X on YZ. Sending its name as local "y" would
    // silently create a perpendicular reference, so only sketch curves are picked.
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"YZ"})).unwrap();
    for id in ["sketch.construction_line.parallel", "sketch.construction_line.perpendicular", "sketch.construction_line.bisector"] {
        let mut tool = crate::sketch_tools::tool_for(id).unwrap();
        super::on_click(&mut h.app, &mut tool, &[crate::viewport::Hit::Axis { name: "Y".into() }], None);
        assert!(tool.picks.is_empty(), "{id} must not reinterpret a world axis as a local axis");
    }
}

#[test]
fn construction_angle_accepts_inline_value_and_undo_is_one_step() {
    let mut h = sketch();
    steps(&mut h, "sketch.construction_line.angle", &[[5., 10.]]);
    h.step(&json!({"move":{"sketch":[15,15]}})).unwrap();
    h.settle();
    let tool = h.app.tool.as_mut().unwrap();
    tool.dims = crate::sketch_dims::boxes_for(tool.cmd, 1);
    let dim = tool.dims.first_mut().unwrap();
    dim.locked = true;
    dim.value = "30 deg".into();
    let mut tool = h.app.tool.take().unwrap();
    super::place_point(&mut h.app, &mut tool, (solidcraft_engine::geom::Vec2::new(15., 15.), None));
    let sid = h.app.session.active_sketch.unwrap();
    let sk = h.app.session.doc.sketch(sid).unwrap();
    let curve = sk.curves.last().unwrap();
    let CurveKind::Line { a, b } = curve.kind else { panic!("line") };
    assert!(((sk.point(b).unwrap() - sk.point(a).unwrap()).angle().to_degrees() - 30.).abs() < 1e-6);
    let count = sk.curves.len();
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.sketch(sid).unwrap().curves.len(), count - 1);
}

#[test]
fn construction_family_menu_and_preference() {
    for enabled in [true, false] {
        let mut h = sketch();
        h.app.preferences.sketch_construction_lines = enabled;
        h.settle();
        h.step(&json!({"click":{"handle":"panel:SKETCH:CREATE"}})).unwrap();
        h.frames(4);
        assert_eq!(crate::scenario::handle_at("create_family:Construction Line").is_some(), enabled);
        if enabled {
            h.step(&json!({"click":{"handle":"create_family:Construction Line"}})).unwrap();
            h.frames(4);
            let family = crate::sketch_families::family("sketch.construction_line").unwrap();
            for id in family.commands {
                assert!(crate::scenario::handle_at(&format!("dropdown:{id}")).is_some(), "{id}");
            }
        }
    }
}

fn digit(h: &mut Harness, d: char) {
    let key = egui::Key::from_name(&d.to_string()).unwrap();
    h.frame_with(vec![
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE },
        egui::Event::Text(d.to_string()),
    ]);
}

fn length_box_focused(h: &Harness) -> bool {
    h.ctx.memory(|m| m.focused()) == Some(crate::sketch_dims::box_id(0))
}

/// The one-click tools show their Length box at once, but it takes the keyboard only for a digit:
/// a letter still reaches the shortcuts (#10258).
#[test]
fn a_waiting_length_box_leaves_letters_to_the_shortcuts_and_takes_a_digit() {
    for id in ["sketch.construction_line.horizontal", "sketch.construction_line.parallel"] {
        let mut h = sketch();
        steps(&mut h, id, &[]);
        h.step(&json!({"move":{"sketch":[5,10]}})).unwrap();
        h.settle();
        let tool = h.app.tool.as_ref().unwrap();
        assert_eq!(tool.dims.iter().map(|b| (b.label, b.value.as_str(), b.locked)).collect::<Vec<_>>(), [("Length", "\u{221e}", false)], "{id}");
        assert!(!length_box_focused(&h), "{id}: no keyboard before a digit");
        h.step(&json!({"key":"L"})).unwrap();
        h.settle();
        assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some("sketch.line"), "{id}: L still starts a line");

        let mut h = sketch();
        steps(&mut h, id, &[]);
        h.step(&json!({"move":{"sketch":[5,10]}})).unwrap();
        h.settle();
        digit(&mut h, '2');
        digit(&mut h, '5');
        h.frames(2);
        assert!(length_box_focused(&h), "{id}: a digit gives the box the keyboard");
        let b = &h.app.tool.as_ref().unwrap().dims[0];
        assert_eq!((b.value.as_str(), b.locked), ("25", true), "{id}");
        assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some(id), "{id}: digits are not shortcuts");
    }
}

/// With a length the tool takes two clicks: the start, then the side the segment runs to. The
/// segment keeps its start's snap and gets the length as a dimension.
#[test]
fn a_typed_length_makes_two_clicks_draw_a_dimensioned_segment() {
    let cases: &[(&str, &[[f64; 2]], [f64; 2], [f64; 2])] = &[
        ("sketch.construction_line.horizontal", &[], [20., 0.], [6., 5.]),
        ("sketch.construction_line.vertical", &[], [20., 0.], [23., -9.]),
        ("sketch.construction_line.parallel", &[[10., 0.]], [0., 20.], [-10., 24.]),
        ("sketch.construction_line.perpendicular", &[[10., 0.]], [0., 20.], [-4., 30.]),
    ];
    for (id, picks, start, side) in cases {
        let mut h = sketch();
        let sid = h.app.session.active_sketch.unwrap();
        let before = h.app.session.doc.sketch(sid).unwrap().curves.len();
        steps(&mut h, id, picks);
        h.step(&json!({"move":{"sketch":start}})).unwrap();
        h.settle();
        digit(&mut h, '8');
        // Apart from the reference pick, so the two clicks are not one double click.
        h.frames(30);
        h.step(&json!({"click":{"sketch":start}})).unwrap();
        h.settle();
        assert_eq!(h.app.session.doc.sketch(sid).unwrap().curves.len(), before, "{id}: the first click only sets the start");
        assert_eq!(h.app.tool.as_ref().unwrap().dims[0].value, "8", "{id}: the length stays typed");
        // A hand moves there first: the box steps out of the way, behind the start.
        h.step(&json!({"move":{"sketch":side}})).unwrap();
        h.settle();
        h.step(&json!({"click":{"sketch":side}})).unwrap();
        h.settle();
        let sk = h.app.session.doc.sketch(sid).unwrap();
        assert_eq!(sk.curves.len(), before + 1, "{id}: {:?}", h.app.status);
        let curve = sk.curves.last().unwrap();
        assert!(!curve.infinite && curve.construction, "{id}: {curve:?}");
        let CurveKind::Line { a, b } = curve.kind else { panic!("{id}: a line") };
        let (a, b) = (sk.point(a).unwrap(), sk.point(b).unwrap());
        assert!(a.dist(solidcraft_engine::geom::Vec2::new(start[0], start[1])) < 1e-6, "{id}: starts at the snapped point {a:?}");
        assert!((a.dist(b) - 8.0).abs() < 1e-6, "{id}: {a:?} {b:?}");
        let s = solidcraft_engine::geom::Vec2::new(side[0], side[1]);
        assert!((b - a).dot(s - a) > 0.0, "{id}: runs towards the side clicked");
        assert!(sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::Length { .. })), "{id}");
        let tool = h.app.tool.as_ref().unwrap();
        assert!(tool.pts.is_empty() && tool.dims.iter().all(|b| !b.locked), "{id}: ready for the next line");
    }
}

/// The angle tool has a Length box beside Angle; Tab reaches it and Enter places the segment.
#[test]
fn the_angle_tool_takes_a_length_beside_its_angle() {
    let mut h = sketch();
    steps(&mut h, "sketch.construction_line.angle", &[[5., 10.]]);
    h.step(&json!({"move":{"sketch":[15,20]}})).unwrap();
    h.settle();
    assert_eq!(h.app.tool.as_ref().unwrap().dims.iter().map(|b| b.label).collect::<Vec<_>>(), ["Angle", "Length"]);
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"30 deg"})).unwrap();
    h.step(&json!({"key":"Tab"})).unwrap();
    h.step(&json!({"text":"12"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    h.settle();
    let sid = h.app.session.active_sketch.unwrap();
    let sk = h.app.session.doc.sketch(sid).unwrap();
    let curve = sk.curves.last().unwrap();
    assert!(!curve.infinite && curve.construction, "{curve:?}");
    let CurveKind::Line { a, b } = curve.kind else { panic!("line") };
    let d = sk.point(b).unwrap() - sk.point(a).unwrap();
    assert!((d.len() - 12.0).abs() < 1e-6 && (d.angle().to_degrees() - 30.0).abs() < 1e-6, "{d:?}");
}

fn click_switch(h: &mut Harness, which: &str) {
    let at = crate::scenario::handle_at(&format!("construction_switch:{which}")).expect("the Construction/Normal switch");
    h.call("ui.click", json!({"x": at.x, "y": at.y}));
    h.settle();
}

/// The Construction/Normal switch is greyed out while the line is infinite; with a length, Normal
/// makes the segment a normal sketch line. The Sketch Palette's line type is left alone (#10258).
#[test]
fn the_normal_switch_needs_a_length_and_draws_a_normal_line() {
    let mut h = sketch();
    crate::sketch_palette::set_mode(&mut h.app, 1);
    let sid = h.app.session.active_sketch.unwrap();
    let before = h.app.session.doc.sketch(sid).unwrap().curves.len();
    steps(&mut h, "sketch.construction_line.horizontal", &[]);
    h.step(&json!({"move":{"sketch":[20,0]}})).unwrap();
    h.settle();
    click_switch(&mut h, "normal");
    assert!(!h.app.tool.as_ref().unwrap().normal, "an infinite line stays construction");

    h.step(&json!({"move":{"sketch":[20,0]}})).unwrap();
    h.settle();
    digit(&mut h, '8');
    h.frames(2);
    click_switch(&mut h, "normal");
    assert!(h.app.tool.as_ref().unwrap().normal, "with a length the switch works");
    assert!(h.app.tool.as_ref().unwrap().pts.is_empty(), "a click on the switch places no point");
    assert_eq!(h.app.tool.as_ref().unwrap().dims[0].value, "8");

    h.step(&json!({"move":{"sketch":[20,0]}})).unwrap();
    h.settle();
    h.frames(30);
    h.step(&json!({"click":{"sketch":[20,0]}})).unwrap();
    h.settle();
    h.step(&json!({"move":{"sketch":[6,5]}})).unwrap();
    h.settle();
    h.step(&json!({"click":{"sketch":[6,5]}})).unwrap();
    h.settle();
    let sk = h.app.session.doc.sketch(sid).unwrap();
    assert_eq!(sk.curves.len(), before + 1, "{:?}", h.app.status);
    let curve = sk.curves.last().unwrap();
    assert!(!curve.construction && !curve.infinite && !curve.centerline, "{curve:?}");
    let CurveKind::Line { a, b } = curve.kind else { panic!("a line") };
    assert!((sk.point(a).unwrap().dist(sk.point(b).unwrap()) - 8.0).abs() < 1e-6);
    assert_eq!(crate::sketch_palette::mode(&h.app), 1, "the palette's line type is untouched");

    // Without a length the next line is infinite construction again, whatever the switch says.
    h.frames(30);
    h.step(&json!({"click":{"sketch":[5,10]}})).unwrap();
    h.settle();
    let sk = h.app.session.doc.sketch(sid).unwrap();
    let curve = sk.curves.last().unwrap();
    assert!(curve.construction && curve.infinite, "{curve:?}");
}

#[test]
fn perpendicular_click_on_reference_endpoint_places_line() {
    // #10600: after picking the reference line, a click exactly on that line's own endpoint
    // must reach the tool and place the perpendicular through the endpoint. The Length box
    // waits beside the pointer there, and its click must not swallow the endpoint's.
    let mut h = sketch();
    let sid = h.app.session.active_sketch.unwrap();
    let before = h.app.session.doc.sketch(sid).unwrap().curves.len();
    steps(&mut h, "sketch.construction_line.perpendicular", &[[10., 0.], [20., 0.]]);
    let sk = h.app.session.doc.sketch(sid).unwrap();
    assert_eq!(sk.curves.len(), before + 1, "endpoint click swallowed: tool={:?} status={:?}", h.app.tool, h.app.status);
    let curve = sk.curves.last().unwrap();
    assert!(curve.infinite && curve.construction, "{curve:?}");
    let CurveKind::Line { a, b } = curve.kind else { panic!("a line") };
    let (p, q) = (sk.point(a).unwrap(), sk.point(b).unwrap());
    let d = q - p;
    let end = sk.resolve_point("a.end").and_then(|i| sk.point(i)).unwrap();
    // Through a.end (20, 0) and perpendicular to a, which runs along the sketch x axis.
    assert!((end - p).x.abs() < 1e-6, "not through a.end: {p:?} {q:?}");
    assert!(d.x.abs() < 1e-6, "not perpendicular to a: {d:?}");
}
