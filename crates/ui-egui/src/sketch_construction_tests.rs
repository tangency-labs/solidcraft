use crate::scenario::Harness;
use serde_json::json;
use solidcraft_engine::geom::prelude::*;
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
    super::place_point(&mut h.app, &mut tool, (solidcore_record::Vec2::new(15., 15.), None));
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
fn construction_offset_box_follows_the_modes_that_have_a_distance() {
    for id in ["sketch.construction_line.horizontal", "sketch.construction_line.vertical"] {
        let mut h = sketch();
        h.step(&json!({"start": id})).unwrap();
        let mut tool = h.app.tool.take().unwrap();
        crate::sketch_dims::sync(&mut tool);
        assert_eq!(tool.dims.iter().map(|d| d.label).collect::<Vec<_>>(), vec!["Length", "Offset"], "{id}");
    }
    let mut h = sketch();
    h.step(&json!({"start":"sketch.construction_line.parallel"})).unwrap();
    let mut tool = h.app.tool.take().unwrap();
    crate::sketch_dims::sync(&mut tool);
    assert_eq!(tool.dims.iter().map(|d| d.label).collect::<Vec<_>>(), vec!["Length"], "the offset waits until the reference line is picked");
    tool.picks.push("a".into());
    crate::sketch_dims::sync(&mut tool);
    assert_eq!(tool.dims.iter().map(|d| d.label).collect::<Vec<_>>(), vec!["Length", "Offset"]);

    let mut h = sketch();
    h.step(&json!({"start":"sketch.construction_line.perpendicular"})).unwrap();
    let mut tool = h.app.tool.take().unwrap();
    tool.picks.push("a".into());
    crate::sketch_dims::sync(&mut tool);
    assert_eq!(tool.dims.iter().map(|d| d.label).collect::<Vec<_>>(), vec!["Length"], "a perpendicular line has no offset");
}

#[test]
fn typed_offset_places_the_line_and_its_dimension() {
    let mut h = sketch();
    steps(&mut h, "sketch.construction_line.parallel", &[[10., 0.]]);
    let tool = h.app.tool.as_mut().unwrap();
    tool.dims = crate::sketch_dims::boxes_for(tool.cmd, 1);
    let dim = tool.dims.iter_mut().find(|d| d.label == "Offset").unwrap();
    dim.locked = true;
    dim.value = "8".into();
    let mut tool = h.app.tool.take().unwrap();
    super::place_point(&mut h.app, &mut tool, (solidcore_record::Vec2::new(5., 10.), None));
    let sid = h.app.session.active_sketch.unwrap();
    let sk = h.app.session.doc.sketch(sid).unwrap();
    let curve = sk.curves.last().unwrap();
    assert!(curve.infinite && curve.construction, "an offset stays an infinite construction line: {curve:?}");
    let CurveKind::Line { a, b } = curve.kind else { panic!("line") };
    let (pa, pb) = (sk.point(a).unwrap(), sk.point(b).unwrap());
    assert!((pa.y - 8.).abs() < 1e-6 && (pb.y - 8.).abs() < 1e-6, "{pa:?} {pb:?}");
    assert!(pa.dist(solidcore_record::Vec2::new(5., 10.)) > 1.0, "the click is not the segment start: {pa:?}");
    assert!(!sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::Length { .. })));
    let distance = sk.constraints.iter().find(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::PointLineDistance { .. })).unwrap();
    assert!((distance.kind.value().unwrap() - 8.).abs() < 1e-6);
    assert_eq!(h.app.session.doc.param(distance.param.as_deref().unwrap()).unwrap().expr, "8");
    let count = sk.curves.len();
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.sketch(sid).unwrap().curves.len(), count - 1);

    let mut h = sketch();
    h.step(&json!({"start":"sketch.construction_line.horizontal"})).unwrap();
    let tool = h.app.tool.as_mut().unwrap();
    tool.dims = crate::sketch_dims::boxes_for(tool.cmd, 1);
    let dim = tool.dims.iter_mut().find(|d| d.label == "Offset").unwrap();
    dim.locked = true;
    dim.value = "6".into();
    let mut tool = h.app.tool.take().unwrap();
    super::place_point(&mut h.app, &mut tool, (solidcore_record::Vec2::new(2., -4.), None));
    let sid = h.app.session.active_sketch.unwrap();
    let sk = h.app.session.doc.sketch(sid).unwrap();
    let curve = sk.curves.last().unwrap();
    assert!(curve.infinite && curve.construction, "{curve:?}");
    let CurveKind::Line { a, .. } = curve.kind else { panic!("line") };
    let start = sk.point(a).unwrap();
    assert!((start.y + 6.).abs() < 1e-6);
    assert!(start.dist(solidcore_record::Vec2::new(2., -4.)) > 1.0, "the click is not pinned as the line start: {start:?}");
    assert!(sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::DistanceY { .. })));
    assert!(!sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::Length { .. })));
}

#[test]
fn an_empty_locked_offset_box_places_the_line_through_the_click() {
    let mut h = sketch();
    h.step(&json!({"start":"sketch.construction_line.horizontal"})).unwrap();
    let tool = h.app.tool.as_mut().unwrap();
    tool.dims = crate::sketch_dims::boxes_for(tool.cmd, 1);
    let dim = tool.dims.iter_mut().find(|d| d.label == "Offset").unwrap();
    dim.locked = true;
    dim.value.clear();
    let mut tool = h.app.tool.take().unwrap();
    super::place_point(&mut h.app, &mut tool, (solidcore_record::Vec2::new(2., -4.), None));
    let sid = h.app.session.active_sketch.unwrap();
    let sk = h.app.session.doc.sketch(sid).unwrap();
    let curve = sk.curves.last().unwrap();
    assert!(curve.infinite && curve.construction, "{curve:?}");
    let CurveKind::Line { a, .. } = curve.kind else { panic!("line") };
    let start = sk.point(a).unwrap();
    assert!((start.y + 4.).abs() < 1e-6, "an empty offset is through the click: {start:?}");
    assert!(!sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::DistanceY { .. })));
}

/// The Offset box is opt-in. Once it has the keyboard, a digit sets the distance and one click
/// places an infinite construction line. It does not start a finite segment.
#[test]
fn a_focused_offset_box_places_an_infinite_line() {
    let cases: &[(&str, &[[f64; 2]], [f64; 2])] = &[
        ("sketch.construction_line.horizontal", &[], [2., -4.]),
        ("sketch.construction_line.vertical", &[], [-3., 4.]),
        ("sketch.construction_line.parallel", &[[10., 0.]], [5., 10.]),
    ];
    for (id, picks, at) in cases {
        let mut h = sketch();
        let sid = h.app.session.active_sketch.unwrap();
        let before = h.app.session.doc.sketch(sid).unwrap().curves.len();
        steps(&mut h, id, picks);
        h.step(&json!({"move":{"sketch":at}})).unwrap();
        h.settle();
        focus_box(&mut h, "Offset");
        digit(&mut h, '8');
        h.frames(30);
        h.step(&json!({"click":{"sketch":at}})).unwrap();
        h.settle();
        let sk = h.app.session.doc.sketch(sid).unwrap();
        assert_eq!(sk.curves.len(), before + 1, "{id}: one click places the line, {:?}", h.app.status);
        let curve = sk.curves.last().unwrap();
        assert!(curve.infinite && curve.construction, "{id}: {curve:?}");
        let CurveKind::Line { a, b } = curve.kind else { panic!("{id}: a line") };
        let (pa, pb) = (sk.point(a).unwrap(), sk.point(b).unwrap());
        let click = solidcore_record::Vec2::new(at[0], at[1]);
        assert!(pa.dist(click) > 1.0 && pb.dist(click) > 1.0, "{id}: the click is not a segment end {pa:?} {pb:?}");
        match *id {
            "sketch.construction_line.horizontal" => assert!((pa.y + 8.0).abs() < 1e-6 && (pb.y + 8.0).abs() < 1e-6, "{pa:?} {pb:?}"),
            "sketch.construction_line.vertical" => assert!((pa.x + 8.0).abs() < 1e-6 && (pb.x + 8.0).abs() < 1e-6, "{pa:?} {pb:?}"),
            _ => assert!((pa.y - 8.0).abs() < 1e-6 && (pb.y - 8.0).abs() < 1e-6, "{pa:?} {pb:?}"),
        }
        assert!(
            sk.constraints.iter().any(|c| {
                matches!(
                    c.kind,
                    solidcraft_engine::sketch::ConstraintKind::PointLineDistance { .. }
                        | solidcraft_engine::sketch::ConstraintKind::DistanceX { .. }
                        | solidcraft_engine::sketch::ConstraintKind::DistanceY { .. }
                )
            }),
            "{id}"
        );
        assert!(!sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::Length { .. })), "{id}");
        let tool = h.app.tool.as_ref().unwrap();
        assert!(tool.pts.is_empty(), "{id}: the click was not kept as a segment start");
        let count = sk.curves.len();
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.sketch(sid).unwrap().curves.len(), count - 1, "{id}: one undo step");
    }
}

/// Length still limits the line. With an offset as well, the segment is shifted and the click is not its start.
#[test]
fn a_typed_length_and_offset_shift_a_segment() {
    let mut h = sketch();
    let sid = h.app.session.active_sketch.unwrap();
    let before = h.app.session.doc.sketch(sid).unwrap().curves.len();
    h.step(&json!({"start":"sketch.construction_line.horizontal"})).unwrap();
    let tool = h.app.tool.as_mut().unwrap();
    tool.dims = crate::sketch_dims::boxes_for(tool.cmd, 0);
    for (label, value) in [("Length", "10"), ("Offset", "6")] {
        let dim = tool.dims.iter_mut().find(|d| d.label == label).unwrap();
        dim.locked = true;
        dim.value = value.into();
    }
    let mut tool = h.app.tool.take().unwrap();
    let click = solidcore_record::Vec2::new(2., -4.);
    super::place_point(&mut h.app, &mut tool, (click, None));
    assert_eq!(tool.pts.len(), 1, "a length still waits for the side");
    assert_eq!(h.app.session.doc.sketch(sid).unwrap().curves.len(), before);
    super::place_point(&mut h.app, &mut tool, (solidcore_record::Vec2::new(12., -4.), None));
    let sk = h.app.session.doc.sketch(sid).unwrap();
    assert_eq!(sk.curves.len(), before + 1, "{:?}", h.app.status);
    let curve = sk.curves.last().unwrap();
    assert!(!curve.infinite && curve.construction, "{curve:?}");
    let CurveKind::Line { a, b } = curve.kind else { panic!("a line") };
    let (pa, pb) = (sk.point(a).unwrap(), sk.point(b).unwrap());
    assert!((pa.y + 6.0).abs() < 1e-6 && (pb.y + 6.0).abs() < 1e-6, "{pa:?} {pb:?}");
    assert!((pa.dist(pb) - 10.0).abs() < 1e-6, "{pa:?} {pb:?}");
    assert!(pa.dist(click) > 1.0, "the click is not the segment start: {pa:?}");
    assert!(sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::DistanceY { .. })));
    assert!(sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::Length { .. })));
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

/// Give one value box the keyboard. A bare digit still goes to Length until a box is focused.
fn focus_box(h: &mut Harness, label: &str) {
    let i = h.app.tool.as_ref().unwrap().dims.iter().position(|b| b.label == label).unwrap_or_else(|| panic!("{label}"));
    h.ctx.memory_mut(|m| m.request_focus(crate::sketch_dims::box_id(i)));
    h.frames(2);
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
        let labels: Vec<_> = tool.dims.iter().map(|b| b.label).collect();
        assert_eq!(labels[0], "Length", "{id}");
        assert_eq!((tool.dims[0].value.as_str(), tool.dims[0].locked), ("\u{221e}", false), "{id}");
        if id.ends_with("horizontal") {
            assert_eq!(labels, vec!["Length", "Offset"], "{id}");
        } else {
            assert_eq!(labels, vec!["Length"], "{id}");
        }
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
        assert!(length_box_focused(&h), "{id}: a digit gives the length box the keyboard");
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
        assert!(a.dist(solidcore_record::Vec2::new(start[0], start[1])) < 1e-6, "{id}: starts at the snapped point {a:?}");
        assert!((a.dist(b) - 8.0).abs() < 1e-6, "{id}: {a:?} {b:?}");
        let s = solidcore_record::Vec2::new(side[0], side[1]);
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

/// Click one of the Sketch Palette's Line Type icons: normal, construction, centerline, infinite.
fn click_line_type(h: &mut Harness, which: &str) {
    let at = crate::scenario::handle_at(which).expect("the palette's Line Type icon");
    h.call("ui.click", json!({"x": at.x, "y": at.y}));
    h.settle();
}

fn shown_line_type(h: &Harness) -> u8 {
    crate::sketch_construction::shown_line_type(h.app.tool.as_ref().unwrap())
}

/// Bring the pointer back over the sketch and type one digit, which goes to the Length box.
fn type_length(h: &mut Harness, d: char) {
    h.step(&json!({"move":{"sketch":[20,0]}})).unwrap();
    h.settle();
    digit(h, d);
    h.frames(2);
}

/// Place a horizontal construction line with the typed length: one click for where it is, one
/// for the side it runs to.
fn place_with_length(h: &mut Harness) {
    h.step(&json!({"move":{"sketch":[20,0]}})).unwrap();
    h.settle();
    h.frames(30);
    h.step(&json!({"click":{"sketch":[20,0]}})).unwrap();
    h.settle();
    h.step(&json!({"move":{"sketch":[6,5]}})).unwrap();
    h.settle();
    h.step(&json!({"click":{"sketch":[6,5]}})).unwrap();
    h.settle();
}

/// While a construction line tool runs, the Sketch Palette's Line Type icons are its line type
/// (#15066): they sit in the palette, so they stay put while the pointer moves, which the
/// Construction/Normal switch beside the Length box did not. Infinite is lit at the start. Normal
/// and Centerline draw a segment of that type at the typed length. Infinite clears the length.
/// The palette's drawing line type (X) is left alone (#10258).
#[test]
fn the_palette_line_type_icons_set_the_construction_line_type() {
    let mut h = sketch();
    crate::sketch_palette::set_mode(&mut h.app, 1);
    let sid = h.app.session.active_sketch.unwrap();
    let before = h.app.session.doc.sketch(sid).unwrap().curves.len();
    steps(&mut h, "sketch.construction_line.horizontal", &[]);
    h.step(&json!({"move":{"sketch":[20,0]}})).unwrap();
    h.settle();
    assert_eq!(shown_line_type(&h), 3, "without a length the line is infinite");
    let icon = || crate::scenario::handle_at("normal").expect("the Normal icon");
    let palette =
        crate::scenario::rect_of("sketch_palette:outer").or_else(|| crate::scenario::rect_of("sketch_palette:options")).expect("the Sketch Palette");
    let at = icon();
    assert!(palette.contains(at), "the icon is in the palette: {at:?} {palette:?}");
    h.step(&json!({"move":{"sketch":[-30,25]}})).unwrap();
    h.settle();
    assert_eq!(icon(), at, "the icon stays put while the pointer moves");

    click_line_type(&mut h, "normal");
    assert_eq!(shown_line_type(&h), 0);
    assert!(h.app.tool.as_ref().unwrap().pts.is_empty(), "a click on the icon places no point");
    type_length(&mut h, '8');
    assert_eq!(h.app.tool.as_ref().unwrap().dims[0].value, "8");
    assert_eq!(shown_line_type(&h), 0, "with a length Normal stays lit");
    place_with_length(&mut h);
    let sk = h.app.session.doc.sketch(sid).unwrap();
    assert_eq!(sk.curves.len(), before + 1, "{:?}", h.app.status);
    let curve = sk.curves.last().unwrap();
    assert!(!curve.construction && !curve.infinite && !curve.centerline, "{curve:?}");
    let CurveKind::Line { a, b } = curve.kind else { panic!("a line") };
    assert!((sk.point(a).unwrap().dist(sk.point(b).unwrap()) - 8.0).abs() < 1e-6);
    assert_eq!(crate::sketch_palette::mode(&h.app), 1, "the palette's line type is untouched");

    // Centerline, the same way.
    click_line_type(&mut h, "centerline");
    type_length(&mut h, '5');
    place_with_length(&mut h);
    let sk = h.app.session.doc.sketch(sid).unwrap();
    assert_eq!(sk.curves.len(), before + 2, "{:?}", h.app.status);
    let curve = sk.curves.last().unwrap();
    assert!(curve.centerline && !curve.construction && !curve.infinite, "{curve:?}");

    // Infinite clears a typed length: one click draws an infinite construction line.
    type_length(&mut h, '7');
    click_line_type(&mut h, "infinite");
    assert!(crate::sketch_construction::typed_length(h.app.tool.as_ref().unwrap()).is_none(), "Infinite clears the length");
    assert_eq!(shown_line_type(&h), 3);
    h.step(&json!({"move":{"sketch":[5,10]}})).unwrap();
    h.settle();
    h.frames(30);
    h.step(&json!({"click":{"sketch":[5,10]}})).unwrap();
    h.settle();
    let sk = h.app.session.doc.sketch(sid).unwrap();
    assert_eq!(sk.curves.len(), before + 3, "{:?}", h.app.status);
    let curve = sk.curves.last().unwrap();
    assert!(curve.construction && curve.infinite, "{curve:?}");
}

/// A line type other than Infinite always draws a segment, as the Line tool does: with no length
/// typed, the first click starts it and the second ends it where the pointer is (#15066).
#[test]
fn a_chosen_line_type_without_a_length_draws_a_segment_to_the_pointer() {
    let mut h = sketch();
    let sid = h.app.session.active_sketch.unwrap();
    let before = h.app.session.doc.sketch(sid).unwrap().curves.len();
    steps(&mut h, "sketch.construction_line.horizontal", &[]);
    h.step(&json!({"move":{"sketch":[20,0]}})).unwrap();
    h.settle();
    click_line_type(&mut h, "normal");
    h.step(&json!({"move":{"sketch":[5,10]}})).unwrap();
    h.settle();
    h.frames(30);
    h.step(&json!({"click":{"sketch":[5,10]}})).unwrap();
    h.settle();
    assert_eq!(h.app.session.doc.sketch(sid).unwrap().curves.len(), before, "the first click only starts the segment");
    h.step(&json!({"move":{"sketch":[17,13]}})).unwrap();
    h.settle();
    h.step(&json!({"click":{"sketch":[17,13]}})).unwrap();
    h.settle();
    let sk = h.app.session.doc.sketch(sid).unwrap();
    assert_eq!(sk.curves.len(), before + 1, "{:?}", h.app.status);
    let curve = sk.curves.last().unwrap();
    assert!(!curve.construction && !curve.infinite && !curve.centerline, "{curve:?}");
    let CurveKind::Line { a, b } = curve.kind else { panic!("a line") };
    let (a, b) = (sk.point(a).unwrap(), sk.point(b).unwrap());
    assert!((a.dist(b) - 12.0).abs() < 1e-6 && (a.y - b.y).abs() < 1e-9, "a horizontal segment to the pointer: {a:?} {b:?}");
}

/// Infinite chosen after a segment's first click drops that start: the next click places the
/// infinite line, rather than one through the abandoned start.
#[test]
fn infinite_chosen_after_a_segment_start_places_the_line_at_the_next_click() {
    let mut h = sketch();
    let sid = h.app.session.active_sketch.unwrap();
    let before = h.app.session.doc.sketch(sid).unwrap().curves.len();
    steps(&mut h, "sketch.construction_line.horizontal", &[]);
    h.step(&json!({"move":{"sketch":[20,0]}})).unwrap();
    h.settle();
    click_line_type(&mut h, "normal");
    h.step(&json!({"move":{"sketch":[5,10]}})).unwrap();
    h.settle();
    h.frames(30);
    h.step(&json!({"click":{"sketch":[5,10]}})).unwrap();
    h.settle();
    assert_eq!(h.app.tool.as_ref().unwrap().pts.len(), 1, "the segment started");
    click_line_type(&mut h, "infinite");
    assert!(h.app.tool.as_ref().unwrap().pts.is_empty(), "Infinite drops the start");
    h.step(&json!({"move":{"sketch":[17,13]}})).unwrap();
    h.settle();
    h.step(&json!({"click":{"sketch":[17,13]}})).unwrap();
    h.settle();
    let sk = h.app.session.doc.sketch(sid).unwrap();
    assert_eq!(sk.curves.len(), before + 1, "{:?}", h.app.status);
    let curve = sk.curves.last().unwrap();
    assert!(curve.infinite && curve.construction, "{curve:?}");
    let CurveKind::Line { a, .. } = curve.kind else { panic!("a line") };
    assert!((sk.point(a).unwrap().y - 13.0).abs() < 1e-6, "through the click, not the old start");
}

/// An angle segment with its Angle typed and no length: the Length box shows the length the click
/// draws, and the click draws it. The click puts the end on the typed angle at the pointer's
/// distance, so the box measures that end rather than the raw pointer.
#[test]
fn a_typed_angle_without_a_length_draws_the_segment_its_length_box_shows() {
    let mut h = sketch();
    let sid = h.app.session.active_sketch.unwrap();
    let before = h.app.session.doc.sketch(sid).unwrap().curves.len();
    steps(&mut h, "sketch.construction_line.angle", &[[5., 10.]]);
    let mut tool = h.app.tool.take().unwrap();
    tool.line_type = 0;
    tool.dims = crate::sketch_dims::boxes_for(tool.cmd, 1);
    let angle = tool.dims.iter_mut().find(|b| b.label == "Angle").unwrap();
    angle.locked = true;
    angle.value = "30 deg".into();
    // 5 from the start, off the typed angle.
    let pointer = (solidcore_record::Vec2::new(8., 14.), None);
    let shown = super::pointer_segment_length(&h.app, &tool, &pointer).expect("a length to show");
    assert!((shown - 5.0).abs() < 1e-6, "the Length box shows the length drawn: {shown}");
    super::place_point(&mut h.app, &mut tool, pointer);
    let sk = h.app.session.doc.sketch(sid).unwrap();
    assert_eq!(sk.curves.len(), before + 1, "the click draws the segment: {:?}", h.app.status);
    let curve = sk.curves.last().unwrap();
    assert!(!curve.construction && !curve.infinite, "{curve:?}");
    let CurveKind::Line { a, b } = curve.kind else { panic!("a line") };
    let d = sk.point(b).unwrap() - sk.point(a).unwrap();
    assert!((d.len() - 5.0).abs() < 1e-6 && (d.angle().to_degrees() - 30.0).abs() < 1e-6, "{d:?}");
}

fn click_switch(h: &mut Harness, which: &str) {
    let at = crate::scenario::handle_at(&format!("construction_switch:{which}")).expect("the Construction/Normal switch");
    h.call("ui.click", json!({"x": at.x, "y": at.y}));
    h.settle();
}

/// With the `construction_line_classic` preference the tools work as before #15066: the
/// Construction/Normal switch under the Length box is greyed out while the line is infinite, and
/// with a length Normal makes the segment a normal sketch line. The Sketch Palette's Line Type
/// icons leave the tool alone and its line type is untouched (#10258).
#[test]
fn the_classic_preference_keeps_the_construction_normal_switch() {
    let mut h = sketch();
    h.app.preferences.construction_line_classic = true;
    crate::sketch_palette::set_mode(&mut h.app, 1);
    let sid = h.app.session.active_sketch.unwrap();
    let before = h.app.session.doc.sketch(sid).unwrap().curves.len();
    steps(&mut h, "sketch.construction_line.horizontal", &[]);
    h.step(&json!({"move":{"sketch":[20,0]}})).unwrap();
    h.settle();
    click_switch(&mut h, "normal");
    assert_eq!(h.app.tool.as_ref().unwrap().line_type, 3, "an infinite line stays construction");

    // The palette's icons pick the drawing line type, not the tool's.
    click_line_type(&mut h, "centerline");
    assert_eq!(h.app.tool.as_ref().unwrap().line_type, 3, "the palette leaves the tool alone");
    assert_eq!(crate::sketch_palette::mode(&h.app), 2);
    crate::sketch_palette::set_mode(&mut h.app, 1);

    type_length(&mut h, '8');
    click_switch(&mut h, "normal");
    assert_eq!(h.app.tool.as_ref().unwrap().line_type, 0, "with a length the switch works");
    assert!(h.app.tool.as_ref().unwrap().pts.is_empty(), "a click on the switch places no point");
    assert_eq!(h.app.tool.as_ref().unwrap().dims[0].value, "8");

    place_with_length(&mut h);
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
