//! Real keyboard input must stay inside the dimensions of the shape being drawn.

use serde_json::json;

use super::{BOX, in_view};
use crate::scenario::Harness;

fn drawing(command: &str) -> Harness {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.step(&json!({"start": command})).unwrap();
    h.step(&json!({"click": {"world": [0, 0, 0]}})).unwrap();
    h.step(&json!({"move": {"world": [20, 10, 0]}})).unwrap();
    assert_focus(&h, 0);
    h
}

fn assert_focus(h: &Harness, index: usize) {
    let expected = egui::Id::new(("sc_dim", index));
    assert_eq!(h.ctx.memory(|memory| memory.focused()), Some(expected), "focus must stay in inline dimension {index}");
}

fn tab(h: &mut Harness, backwards: bool) {
    h.step(&json!({"key": "Tab", "shift": backwards})).unwrap();
}

fn type_value(h: &mut Harness, text: &str) {
    h.step(&json!({"key": "A", "cmd": true})).unwrap();
    h.step(&json!({"text": text})).unwrap();
}

#[test]
fn tabs_cycle_rectangle_dimensions_indefinitely_and_enter_commits_values() {
    for command in ["sketch.rectangle.two_point", "sketch.rectangle.center"] {
        let mut h = drawing(command);
        type_value(&mut h, "30");
        tab(&mut h, false);
        assert_focus(&h, 1);
        type_value(&mut h, "12");
        for i in 0..8 {
            tab(&mut h, false);
            assert_focus(&h, i % 2);
        }
        for i in 0..8 {
            tab(&mut h, true);
            assert_focus(&h, i % 2);
        }
        // Revisited locked dimensions are selected, so another value replaces them directly.
        h.step(&json!({"text": "14"})).unwrap();
        tab(&mut h, true);
        assert_focus(&h, 0);
        h.step(&json!({"text": "32"})).unwrap();
        let dims = &h.app.tool.as_ref().unwrap().dims;
        assert_eq!(dims.iter().map(|d| d.value.as_str()).collect::<Vec<_>>(), ["32", "14"]);
        assert!(dims.iter().all(|d| d.locked));
        h.step(&json!({"key": "Enter"})).unwrap();
        let state = h.app.session.world_state();
        let sketch = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
        assert_eq!(sketch.sketch.curves.iter().filter(|curve| !curve.construction).count(), 4);
        assert_eq!(sketch.profiles.len(), 1);
        assert!((sketch.profiles[0].area - 448.0).abs() < 1e-6, "{command}: {}", sketch.profiles[0].area);
    }
}

#[test]
fn line_dimensions_wrap_both_directions_and_enter_uses_length_and_angle() {
    let mut h = drawing("sketch.line");
    type_value(&mut h, "10");
    tab(&mut h, false);
    type_value(&mut h, "45 deg");
    for i in 0..6 {
        tab(&mut h, false);
        assert_focus(&h, i % 2);
    }
    tab(&mut h, true);
    assert_focus(&h, 0);
    h.step(&json!({"key": "Enter"})).unwrap();
    let state = h.app.session.world_state();
    let sketch = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    assert_eq!(sketch.sketch.curves.len(), 1);
    let points: Vec<_> = sketch.sketch.points.iter().map(|point| point.pos).collect();
    assert!(points.iter().any(|point| (point.x - 10.0 / 2f64.sqrt()).abs() < 1e-6 && (point.y - 10.0 / 2f64.sqrt()).abs() < 1e-6));
}

#[test]
fn single_dimension_keeps_focus_for_tab_and_shift_tab() {
    let mut h = drawing("sketch.circle.center");
    type_value(&mut h, "8");
    for backwards in [false, false, true, true, false] {
        tab(&mut h, backwards);
        assert_focus(&h, 0);
    }
    assert_eq!(h.app.tool.as_ref().unwrap().dims[0].value, "8");
    h.step(&json!({"key": "Enter"})).unwrap();
    let state = h.app.session.world_state();
    let sketch = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    assert_eq!(sketch.profiles.len(), 1);
    assert!((sketch.profiles[0].area - std::f64::consts::PI * 16.0).abs() < 1e-6);
}

#[test]
fn forward_tab_completes_expression_then_cycles_and_shift_tab_moves_back() {
    let mut h = drawing("sketch.rectangle.two_point");
    h.app.run("parameters.add", json!({"name": "width", "expression": "24 mm"})).unwrap();
    h.frames(2);
    type_value(&mut h, "wid");
    tab(&mut h, false);
    assert_focus(&h, 0);
    assert_eq!(h.app.tool.as_ref().unwrap().dims[0].value, "width");
    tab(&mut h, false);
    assert_focus(&h, 1);
    type_value(&mut h, "5");
    tab(&mut h, true);
    assert_focus(&h, 0);
    type_value(&mut h, "wid");
    tab(&mut h, true);
    assert_focus(&h, 1);
    assert_eq!(h.app.tool.as_ref().unwrap().dims[0].value, "wid", "Shift+Tab must navigate rather than accepting a completion");
    tab(&mut h, true);
    assert_focus(&h, 0);
    tab(&mut h, false);
    assert_focus(&h, 0);
    assert_eq!(h.app.tool.as_ref().unwrap().dims[0].value, "width");
    h.step(&json!({"key": "Enter"})).unwrap();
    let state = h.app.session.world_state();
    let sketch = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    assert_eq!(sketch.profiles.len(), 1);
    assert!((sketch.profiles[0].area - 120.0).abs() < 1e-6);
}

/// #3257: clicking a parameter suggestion under a line's length box puts the name in the box,
/// which keeps the focus; the click places nothing, and Enter draws the line that long.
#[test]
fn clicking_a_parameter_suggestion_fills_the_length_box() {
    let mut h = drawing("sketch.line");
    h.app.run("parameters.add", json!({"name": "width", "expression": "24 mm"})).unwrap();
    h.frames(2);
    type_value(&mut h, "wid");
    h.frames(1);
    h.step(&json!({"click": {"handle": "dialog:suggestion:0"}})).unwrap();
    h.frames(1);
    assert_focus(&h, 0);
    assert_eq!(h.app.tool.as_ref().unwrap().dims[0].value, "width");
    let sketch = |h: &Harness| h.app.session.world_state().sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch.curves.len();
    assert_eq!(sketch(&h), 0, "the click on the list placed no point");
    h.step(&json!({"key": "Enter"})).unwrap();
    let state = h.app.session.world_state();
    let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    assert_eq!(sk.curves.len(), 1);
    let length = sk.shape(0).map(|s| match s {
        solidcraft_engine::sketch::Shape::Line { a, b } => a.dist(b),
        _ => 0.0,
    });
    assert!(length.is_some_and(|l| (l - 24.0).abs() < 1e-6), "{length:?}");
}

/// #3278: after the 2-point circle's first point, a Diameter box takes the keyboard; the typed
/// diameter sets the second point along the cursor's direction, and Enter draws that circle.
#[test]
fn two_point_circle_takes_a_typed_diameter() {
    let mut h = drawing("sketch.circle.two_point");
    assert_eq!(h.app.tool.as_ref().unwrap().dims.iter().map(|b| b.label).collect::<Vec<_>>(), ["Diameter"]);
    type_value(&mut h, "30");
    h.step(&json!({"key": "Enter"})).unwrap();
    let state = h.app.session.world_state();
    let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    assert_eq!(sk.curves.len(), 1, "{:?}", h.app.status);
    assert!(sk.radius(0).is_some_and(|r| (r - 15.0).abs() < 1e-6), "{:?}", sk.radius(0));
    let c = sk.center(0).unwrap();
    // From the first point (the origin) toward the cursor at (20, 10).
    let dir = solidcraft_engine::geom::Vec2::new(20.0, 10.0).normalized().unwrap();
    assert!((c - dir * 15.0).len() < 1e-6, "{c:?}");
}

#[test]
fn inch_live_lengths_use_design_units_and_typed_four_digits_commit_in_inches() {
    let mut h = Harness::new();
    h.app.run("document.units", json!({"units":"in"})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.step(&json!({"start":"sketch.rectangle.two_point"})).unwrap();
    h.step(&json!({"click":{"world":[0,0,0]}})).unwrap();
    h.step(&json!({"move":{"world":[25.4,12.7,0]}})).unwrap();
    let dims = &h.app.tool.as_ref().unwrap().dims;
    // Pointer snapping rounds in model space; displayed values must still be inches.
    assert!((dims[0].value.parse::<f64>().unwrap() - 1.0).abs() < 0.01);
    assert!((dims[1].value.parse::<f64>().unwrap() - 0.5).abs() < 0.01);
    type_value(&mut h, "1.2345");
    tab(&mut h, false);
    type_value(&mut h, "0.500");
    let tool = h.app.tool.as_ref().unwrap();
    assert!((crate::sketch_dims::locked_value(&h.app, tool, "Width").unwrap() - 31.3563).abs() < 1e-6);
    assert!((crate::sketch_dims::locked_value(&h.app, tool, "Height").unwrap() - 12.7).abs() < 1e-6);
    h.step(&json!({"key":"Enter"})).unwrap();
    let st = h.app.session.world_state();
    let sk = &st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    assert!(sk.constraints.iter().any(|c| c.kind.value().is_some_and(|v| (v - 31.3563).abs() < 1e-6)));
}

/// A conic's fullness is a typed value as well as a drag. The Rho box takes the keyboard once
/// the chord is down, shows the parabola the command would otherwise default to, and the value
/// typed into it reaches the command when the apex is placed.
#[test]
fn a_typed_rho_shapes_the_conic_whose_apex_is_placed_next() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.step(&json!({"start": "sketch.conic"})).unwrap();
    h.step(&json!({"click": {"world": [0, 0, 0]}})).unwrap();
    h.step(&json!({"move": {"world": [40, 0, 0]}})).unwrap();
    // One point down is a chord with one end: there is no shape to put a number on yet.
    assert!(h.app.tool.as_ref().unwrap().dims.is_empty());
    h.step(&json!({"click": {"world": [40, 0, 0]}})).unwrap();
    h.step(&json!({"move": {"world": [20, 20, 0]}})).unwrap();
    assert_focus(&h, 0);
    let dims = &h.app.tool.as_ref().unwrap().dims;
    assert_eq!(dims.iter().map(|d| d.label).collect::<Vec<_>>(), ["Rho"]);
    assert_eq!(dims[0].value, "0.5", "the live box reads as a ratio, not as a rounded count");
    assert!(!dims[0].locked);

    type_value(&mut h, "0.8");
    assert!(h.app.tool.as_ref().unwrap().dims[0].locked);
    h.step(&json!({"key": "Enter"})).unwrap();

    let state = h.app.session.world_state();
    let sketch = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    let (apex, rho) = sketch
        .curves
        .iter()
        .find_map(|curve| match curve.kind {
            solidcraft_engine::sketch::CurveKind::Conic { apex, rho, .. } => Some((apex, rho)),
            _ => None,
        })
        .expect("the typed value placed a conic");
    assert!((rho - 0.8).abs() < 1e-9, "typed rho reached the command: {rho}");
    // The apex lands where the cursor was, within the pixel the cursor was converted through.
    assert!(sketch.point(apex).unwrap().dist(solidcraft_engine::geom::Vec2::new(20.0, 20.0)) < 0.5, "{:?}", sketch.point(apex));
}

/// A rho the command would refuse stays in its box to be corrected, and places nothing.
#[test]
fn a_rho_outside_zero_to_one_is_refused_before_it_reaches_the_command() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.step(&json!({"start": "sketch.conic"})).unwrap();
    h.step(&json!({"click": {"world": [0, 0, 0]}})).unwrap();
    h.step(&json!({"click": {"world": [40, 0, 0]}})).unwrap();
    h.step(&json!({"move": {"world": [20, 20, 0]}})).unwrap();
    type_value(&mut h, "5");
    h.step(&json!({"key": "Enter"})).unwrap();

    let state = h.app.session.world_state();
    let sketch = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    assert!(sketch.curves.is_empty(), "a refused rho must place nothing");
    let tool = h.app.tool.as_ref().expect("the tool stays active for a correction");
    assert_eq!(tool.pts.len(), 2, "the chord stays down");
    assert_eq!(tool.dims.iter().map(|d| d.value.as_str()).collect::<Vec<_>>(), ["5"], "the refused value stays in its box");
    assert!(h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "the refusal is reported");
}

/// Rounded Rectangle (#891): typed width, height and corner radius make one profile whose
/// size is the typed overall size; the radius stays its own parameter.
#[test]
fn rounded_rectangle_takes_typed_width_height_and_radius() {
    let mut h = drawing("sketch.rectangle.rounded");
    assert_eq!(h.app.tool.as_ref().unwrap().dims.iter().map(|d| d.label).collect::<Vec<_>>(), ["Width", "Height", "Radius"]);
    type_value(&mut h, "30");
    tab(&mut h, false);
    type_value(&mut h, "16");
    tab(&mut h, false);
    assert_focus(&h, 2);
    type_value(&mut h, "4");
    let undo = h.app.session.undo.len();
    h.step(&json!({"key": "Enter"})).unwrap();
    let state = h.app.session.world_state();
    let sketch = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    let kinds: Vec<_> = sketch.sketch.curves.iter().map(|c| matches!(c.kind, solidcraft_engine::sketch::CurveKind::Arc { .. })).collect();
    assert_eq!(kinds.iter().filter(|arc| **arc).count(), 4, "four corner arcs");
    assert_eq!(kinds.len(), 8);
    assert_eq!(sketch.profiles.len(), 1);
    let want = 30.0 * 16.0 - (4.0 - std::f64::consts::PI) * 16.0;
    assert!((sketch.profiles[0].area - want).abs() < 1e-6, "area {} want {want}", sketch.profiles[0].area);
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
}

/// Without a typed radius the corners get a fifth of the shorter side.
#[test]
fn rounded_rectangle_clicks_use_a_default_radius() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.step(&json!({"start": "sketch.rectangle.rounded"})).unwrap();
    h.step(&json!({"click": {"sketch": [0, 0]}})).unwrap();
    h.step(&json!({"click": {"sketch": [40, 20]}})).unwrap();
    let state = h.app.session.world_state();
    let sketch = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    assert_eq!(sketch.profiles.len(), 1, "{:?}", h.app.status);
    let (w, hh) = (40.0, 20.0);
    let r: f64 = 0.2 * 20.0;
    let want = w * hh - (4.0 - std::f64::consts::PI) * r * r;
    assert!((sketch.profiles[0].area - want).abs() < 0.05 * want, "area {} want about {want}", sketch.profiles[0].area);
}

/// "Dimension new shapes" (Preferences › Sketch) is off by default: shapes drawn by clicks get no
/// dimensions. On, every size of the shape is dimensioned at its size as drawn, in the same undo
/// step as the shape.
#[test]
fn dimension_new_shapes_dimensions_every_size_only_when_on() {
    use solidcraft_engine::doc::expr::Kind;
    let dimensions = |h: &Harness| -> Vec<f64> {
        let doc = &h.app.session.doc;
        let mut values: Vec<f64> = doc.params.iter().filter(|p| p.model).filter_map(|p| doc.eval(&p.name, Kind::Length).ok()).collect();
        values.sort_by(f64::total_cmp);
        values
    };
    let rectangle = |h: &mut Harness| {
        h.step(&json!({"start": "sketch.rectangle.two_point"})).unwrap();
        h.step(&json!({"click": {"sketch": [0, 0]}})).unwrap();
        h.step(&json!({"click": {"sketch": [40, 20]}})).unwrap();
    };

    let mut h = Harness::new();
    assert!(!h.app.preferences.sketch_auto_dimensions, "Dimension new shapes is on by default");
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    rectangle(&mut h);
    assert!(dimensions(&h).is_empty(), "off, yet the rectangle has dimensions {:?}", dimensions(&h));

    let mut h = Harness::new();
    h.app.preferences.sketch_auto_dimensions = true;
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    rectangle(&mut h);
    let sizes = dimensions(&h);
    assert_eq!(sizes.len(), 2, "width and height: {sizes:?} {:?}", h.app.status);
    assert!((sizes[0] - 20.0).abs() < 0.5 && (sizes[1] - 40.0).abs() < 0.5, "{sizes:?}");
    h.step(&json!({"key": "Escape"})).unwrap();
    h.step(&json!({"start": "sketch.circle.center"})).unwrap();
    h.step(&json!({"click": {"sketch": [70, 10]}})).unwrap();
    h.step(&json!({"click": {"sketch": [80, 10]}})).unwrap();
    assert_eq!(dimensions(&h).len(), 3, "the circle's diameter joins: {:?} {:?}", dimensions(&h), h.app.status);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(dimensions(&h).len(), 2, "one undo removes the circle with its dimension");
}

/// "Also place them from the origin" finishes the job: a rectangle drawn away from the origin
/// gets its width, height and place, and the sketch is fully constrained. On its own (without
/// "Dimension new shapes") it does nothing.
#[test]
fn place_from_the_origin_fully_constrains_new_shapes() {
    let draw = |dimension: bool, locate: bool| -> (usize, u64) {
        let mut h = Harness::new();
        assert!(!h.app.preferences.sketch_auto_locate, "Also place them from the origin is on by default");
        h.app.preferences.sketch_auto_dimensions = dimension;
        h.app.preferences.sketch_auto_locate = locate;
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.step(&json!({"start": "sketch.rectangle.two_point"})).unwrap();
        h.step(&json!({"click": {"sketch": [10, 10]}})).unwrap();
        h.step(&json!({"click": {"sketch": [50, 30]}})).unwrap();
        let dimensions = h.app.session.doc.params.iter().filter(|p| p.model).count();
        let dof = h.app.run("sketch.solve", json!({})).unwrap()["dof"].as_u64().unwrap();
        (dimensions, dof)
    };
    assert_eq!(draw(true, true), (4, 0), "width, height and two distances from the origin");
    assert_eq!(draw(true, false), (2, 2), "sizes only: the rectangle can still move");
    assert_eq!(draw(false, true), (0, 4), "placing needs Dimension new shapes");
}

/// A line from a circle's centre to the circle, ending a little off the quadrant: the preview
/// snaps to horizontal or vertical, so the click must place exactly that line, on the circle.
#[test]
fn a_line_end_on_a_circle_within_the_axis_snap_lands_on_the_quadrant_the_preview_shows() {
    use solidcraft_engine::geom::Vec2;
    use solidcraft_engine::sketch::{ConstraintKind, CurveKind};
    let r = 30.0_f64;
    for (deg, end, horizontal) in [(-1.5_f64, Vec2::new(r, 0.0), true), (91.8, Vec2::new(0.0, r), false), (178.0, Vec2::new(-r, 0.0), true)] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.circle.center", json!({"center": [0, 0], "radius": r})).unwrap();
        h.settle();
        let sid = h.app.session.active_sketch.unwrap();
        h.step(&json!({"start": "sketch.line"})).unwrap();
        h.step(&json!({"click": {"world": [0, 0, 0]}})).unwrap();
        let at = Vec2::from_angle(deg.to_radians()) * r;
        h.step(&json!({"move": {"world": [at.x, at.y, 0]}})).unwrap();
        let tool = h.app.tool.as_ref().unwrap();
        let hover = tool.hover.clone().unwrap();
        assert!(hover.1.as_deref().is_some_and(|s| s.starts_with("on:")), "{deg}: {hover:?}");
        let shown = crate::sketch_dims::shown(&h.app, tool, &hover);
        assert!(shown.dist(end) < 1e-9, "{deg}: preview at {shown:?}");
        h.step(&json!({"click": {"world": [at.x, at.y, 0]}})).unwrap();
        let st = h.app.session.world_state();
        let sk = &st.sketch(sid).unwrap().sketch;
        let (line, a, b) = sk
            .curves
            .iter()
            .enumerate()
            .find_map(|(i, c)| if let CurveKind::Line { a, b } = c.kind { Some((i, a, b)) } else { None })
            .unwrap_or_else(|| panic!("{deg}: no line, {:?}", h.app.status));
        assert!(sk.point(a).unwrap().dist(Vec2::ZERO) < 1e-9, "{deg}");
        assert!(sk.point(b).unwrap().dist(end) < 1e-9, "{deg}: placed at {:?}", sk.point(b));
        assert!(sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::PointOnCurve { p, c: 0 } if p == b)), "{deg}: end not on the circle");
        let axis = |k: &ConstraintKind| {
            if horizontal {
                matches!(k, ConstraintKind::Horizontal { l } if *l == line)
            } else {
                matches!(k, ConstraintKind::Vertical { l } if *l == line)
            }
        };
        assert!(sk.constraints.iter().any(|c| axis(&c.kind)), "{deg}: axis constraint missing");
    }
}

/// Outside the axis snap a line end on a circle stays where the cursor meets the circle.
#[test]
fn a_line_end_on_a_circle_outside_the_axis_snap_stays_at_the_cursor() {
    use solidcraft_engine::geom::Vec2;
    use solidcraft_engine::sketch::CurveKind;
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.circle.center", json!({"center": [0, 0], "radius": 30})).unwrap();
    h.settle();
    let sid = h.app.session.active_sketch.unwrap();
    h.step(&json!({"start": "sketch.line"})).unwrap();
    h.step(&json!({"click": {"world": [0, 0, 0]}})).unwrap();
    let at = Vec2::from_angle(20f64.to_radians()) * 30.0;
    h.step(&json!({"move": {"world": [at.x, at.y, 0]}})).unwrap();
    h.step(&json!({"click": {"world": [at.x, at.y, 0]}})).unwrap();
    let st = h.app.session.world_state();
    let sk = &st.sketch(sid).unwrap().sketch;
    let b = sk.curves.iter().find_map(|c| if let CurveKind::Line { b, .. } = c.kind { Some(b) } else { None }).unwrap();
    let end = sk.point(b).unwrap();
    assert!((end.len() - 30.0).abs() < 1e-9 && end.dist(at) < 0.5, "{end:?}");
}

/// #6198: the Midpoint Line tool shows the same Length/Angle readout as the Line tool while
/// the line is being drawn.
#[test]
fn a_midpoint_line_shows_its_length_and_angle_while_it_is_drawn() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.step(&json!({"start": "sketch.line.midpoint"})).unwrap();
    h.step(&json!({"click": {"world": [0, 0, 0]}})).unwrap();
    h.step(&json!({"move": {"world": [10, 0, 0]}})).unwrap();
    let tool = h.app.tool.as_ref().unwrap();
    assert_eq!(tool.dims.iter().map(|d| d.label).collect::<Vec<_>>(), ["Length", "Angle"], "the midpoint line shows the line tool's boxes");
    // The line runs from the cursor's mirror image through the clicked middle to the
    // cursor: its length is twice the middle-to-cursor distance.
    let length: f64 = tool.dims[0].value.parse().unwrap_or_default();
    assert!((length - 20.0).abs() < 0.01, "the whole line's length: {}", tool.dims[0].value);
    let angle: f64 = tool.dims[1].value.trim_end_matches(" deg").parse().unwrap_or_default();
    assert!(angle.abs() < 0.1, "the line's angle: {}", tool.dims[1].value);
}

/// #6198: a typed length and angle drive the midpoint line that is drawn.
#[test]
fn a_midpoint_line_takes_a_typed_length_and_angle() {
    use solidcraft_engine::sketch::ConstraintKind;
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.step(&json!({"start": "sketch.line.midpoint"})).unwrap();
    h.step(&json!({"click": {"world": [0, 0, 0]}})).unwrap();
    h.step(&json!({"move": {"world": [10, 0, 0]}})).unwrap();
    type_value(&mut h, "20");
    tab(&mut h, false);
    type_value(&mut h, "45 deg");
    h.step(&json!({"key": "Enter"})).unwrap();
    let state = h.app.session.world_state();
    let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    assert_eq!(sk.curves.len(), 1, "{:?}", h.app.status);
    assert!(
        sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Length { value, .. } if (value - 20.0).abs() < 1e-9)),
        "the typed length is a driving dimension: {:?}",
        sk.constraints
    );
    assert!(
        sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::LineAngle { value, .. } if (value - 45f64.to_radians()).abs() < 1e-9)),
        "the typed angle is a driving dimension: {:?}",
        sk.constraints
    );
}

/// A click on a line type in the palette switches the drawing mode; it must not take the keyboard
/// away from the dimension box of the shape being drawn (#2927).
#[test]
fn a_palette_linetype_click_leaves_the_dimension_box_focused() {
    let mut h = drawing("sketch.rectangle.two_point");
    type_value(&mut h, "30");
    assert_focus(&h, 0);
    let at = h.widget("construction").expect("the palette's construction button");
    h.call("ui.click", json!({"x": at[0], "y": at[1]}));
    h.settle();
    assert_focus(&h, 0);
    // The keyboard still reaches the box: typing keeps editing the dimension (a locked box keeps
    // its caret, so the text is appended), and Tab still moves to the next box.
    h.step(&json!({"text": "4"})).unwrap();
    assert_focus(&h, 0);
    assert_eq!(h.app.tool.as_ref().unwrap().dims[0].value, "304");
    tab(&mut h, false);
    assert_focus(&h, 1);
    assert_eq!(crate::sketch_palette::mode(&h.app), 1, "the click still switched the drawing line type");
}

/// A value box set off the cursor stays in the drawing area (#2201): past an edge it goes to the
/// cursor's other side, still clear of it; with no room either side it is pushed inside.
#[test]
fn value_boxes_stay_in_view_and_off_the_cursor() {
    let view = egui::Rect::from_min_max(egui::pos2(250.0, 130.0), egui::pos2(1650.0, 960.0));
    let cursor = egui::pos2(565.0, 142.0);
    // 50 px above the cursor: over the ribbon.
    let above = cursor + egui::vec2(-34.0, -11.0 - 50.0);
    let p = in_view(above, Some(view), Some(cursor));
    let rect = egui::Rect::from_min_size(p, BOX);
    assert!(view.contains_rect(rect), "{rect:?} in {view:?}");
    assert!(!rect.expand(2.0).contains(cursor), "{rect:?} clear of {cursor:?}");
    assert!(rect.min.y > cursor.y, "below the cursor now");
    // A box that fits stays where it is; without a view nothing moves.
    let inside = egui::pos2(700.0, 500.0);
    assert_eq!(in_view(inside, Some(view), Some(cursor)), inside);
    assert_eq!(in_view(above, None, Some(cursor)), above);
    // No room on either side: inside the view all the same.
    let tiny = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(80.0, 30.0));
    assert!(tiny.contains_rect(egui::Rect::from_min_size(in_view(egui::pos2(-200.0, -200.0), Some(tiny), Some(egui::pos2(40.0, 15.0))), BOX)));
}

/// `name = expression` in a value box, as in the Dimension tool's: the shape takes the
/// expression's size while it is typed and when placed, and the new parameter `name` drives its
/// dimension (#7567).
#[test]
fn a_box_typed_name_equals_value_defines_the_parameter_driving_the_dimension() {
    let mut h = drawing("sketch.circle.center");
    type_value(&mut h, "width=20");
    let tool = h.app.tool.as_ref().unwrap();
    assert_eq!(crate::sketch_dims::locked_value(&h.app, tool, "Diameter"), Some(20.0), "the preview follows the typed size");
    h.step(&json!({"key": "Enter"})).unwrap();
    let state = h.app.session.world_state();
    let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    assert_eq!(sk.curves.len(), 1, "{:?}", h.app.status);
    assert!(sk.radius(0).is_some_and(|r| (r - 10.0).abs() < 1e-6), "{:?}", sk.radius(0));
    let width = h.app.session.doc.param("width").expect("the parameter is defined");
    assert_eq!((width.expr.as_str(), width.unit.as_str(), width.model), ("20", "mm", false));
    let dimension = sk.constraints.iter().find_map(|c| c.param.as_deref()).expect("a driving dimension");
    assert_eq!(h.app.session.doc.param(dimension).unwrap().expr, "width");
    assert_eq!(h.app.status.as_ref().map(|(text, _, error)| (text.as_str(), *error)), Some(("Parameter `width` defined", false)));

    // In an inch design the right-hand side is in inches, while typing too.
    let mut h = Harness::new();
    h.app.run("document.units", json!({"units":"in"})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.step(&json!({"start":"sketch.rectangle.two_point"})).unwrap();
    h.step(&json!({"click":{"world":[0,0,0]}})).unwrap();
    h.step(&json!({"move":{"world":[25.4,12.7,0]}})).unwrap();
    type_value(&mut h, "#w = 2");
    tab(&mut h, false);
    type_value(&mut h, "h = 1");
    let tool = h.app.tool.as_ref().unwrap();
    assert!((crate::sketch_dims::locked_value(&h.app, tool, "Width").unwrap() - 50.8).abs() < 1e-6);
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(h.app.session.doc.param("w").map(|p| p.unit.as_str()), Some("in"), "{:?}", h.app.status);
    assert!(h.app.session.doc.param("h").is_some());
    assert_eq!(h.app.status.as_ref().map(|(text, _, _)| text.as_str()), Some("Parameters `h`, `w` defined"));
}

/// A box that is not a dimension (a polygon's sides, a rounded rectangle's corner radius) has
/// nothing for a parameter to drive: `name = ...` there is refused, not quietly dropped.
#[test]
fn a_definition_in_a_box_that_is_not_a_dimension_is_refused() {
    for (command, tabs, text, says) in [
        ("sketch.polygon.inscribed", 1, "n = 6", "Sides: only a box that becomes a dimension can define `n`"),
        ("sketch.rectangle.rounded", 2, "r = 2", "Radius: only a box that becomes a dimension can define `r`"),
        ("sketch.circle.center", 0, "w =", "Diameter: `w =` needs a value after the `=`"),
    ] {
        let mut h = drawing(command);
        for _ in 0..tabs {
            tab(&mut h, false);
        }
        type_value(&mut h, text);
        h.step(&json!({"key": "Enter"})).unwrap();
        let state = h.app.session.world_state();
        let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        assert!(sk.curves.is_empty(), "{command} {text}: placed {:?}", h.app.status);
        assert!(h.app.session.doc.params.iter().all(|p| p.model), "{command} {text}: no user parameter");
        let status = h.app.status.as_ref().expect("the refusal is reported");
        assert!(status.2 && status.0.starts_with(says), "{command} {text}: {status:?}");
        assert_eq!(h.app.tool.as_ref().unwrap().pts.len(), 1, "{command} {text}: the first point stays down");
    }
}

/// #7704: holding Alt while drawing a rectangle makes all sides equal length (a square) and
/// applies an Equal constraint so only one dimension is needed.
#[test]
fn holding_alt_squares_rectangles_and_needs_only_one_typed_size() {
    use solidcraft_engine::sketch::ConstraintKind;
    for command in ["sketch.rectangle.two_point", "sketch.rectangle.center"] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.step(&json!({"start": command})).unwrap();
        h.step(&json!({"click": {"world": [0, 0, 0]}})).unwrap();
        h.step(&json!({"move": {"world": [24, 10, 0]}, "alt": true})).unwrap();
        let dims = &h.app.tool.as_ref().unwrap().dims;
        let w: f64 = dims[0].value.parse().unwrap();
        let hh: f64 = dims[1].value.parse().unwrap();
        assert!((w - hh).abs() < 1e-6, "{command}: live boxes must match ({w} vs {hh})");
        h.step(&json!({"text": "18"})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        let state = h.app.session.world_state();
        let sketch = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
        assert!(sketch.sketch.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Equal { .. })), "{command}");
        assert_eq!(sketch.profiles.len(), 1, "{command}");
        assert!((sketch.profiles[0].area - 324.0).abs() < 1e-6, "{command}: {}", sketch.profiles[0].area);
    }

    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.step(&json!({"start": "sketch.rectangle.three_point"})).unwrap();
    h.step(&json!({"click": {"sketch": [0, 0]}})).unwrap();
    h.step(&json!({"click": {"sketch": [20, 0]}})).unwrap();
    h.step(&json!({"move": {"sketch": [20, 7]}, "alt": true})).unwrap();
    h.step(&json!({"click": {"sketch": [20, 7]}, "alt": true})).unwrap();
    let state = h.app.session.world_state();
    let sketch = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    assert!(sketch.sketch.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Equal { .. })));
    assert_eq!(sketch.profiles.len(), 1);
    assert!((sketch.profiles[0].area - 400.0).abs() < 1e-6, "{}", sketch.profiles[0].area);
}

#[test]
fn releasing_alt_before_placement_restores_a_rectangle() {
    let mut h = drawing("sketch.rectangle.two_point");
    h.step(&json!({"move": {"sketch": [24, 10]}, "alt": true})).unwrap();
    h.step(&json!({"move": {"sketch": [24, 10]}, "alt": false})).unwrap();
    h.step(&json!({"click": {"sketch": [24, 10]}})).unwrap();
    let state = h.app.session.world_state();
    let sketch = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    assert_eq!(sketch.profiles.len(), 1, "{:?}", h.app.status);
    assert!((sketch.profiles[0].area - 240.0).abs() < 1e-6);
    assert!(!sketch.sketch.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::Equal { .. })));
}

#[test]
fn alt_square_works_with_rounded_corners_and_automatic_dimensions() {
    for command in ["sketch.rectangle.two_point", "sketch.rectangle.center", "sketch.rectangle.rounded"] {
        for auto in [false, true] {
            let mut h = drawing(command);
            h.app.preferences.sketch_auto_dimensions = auto;
            h.step(&json!({"move": {"sketch": [24, 10]}, "alt": true})).unwrap();
            h.step(&json!({"text": "30", "alt": true})).unwrap();
            assert!(h.app.tool.as_ref().unwrap().square, "{command}: typing latches Alt");
            h.step(&json!({"key": "Enter", "alt": false})).unwrap();
            let state = h.app.session.world_state();
            let sketch = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
            assert_eq!(sketch.profiles.len(), 1, "{command}, auto={auto}: {:?}", h.app.status);
            let area = if command.ends_with("rounded") { 900.0 - (4.0 - std::f64::consts::PI) * 36.0 } else { 900.0 };
            assert!((sketch.profiles[0].area - area).abs() < 1e-5, "{command}, auto={auto}: {}", sketch.profiles[0].area);
            assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{command}, auto={auto}: {:?}", h.app.status);
        }
    }
}

#[test]
fn explicitly_typed_unequal_dimensions_override_alt_square() {
    let mut h = drawing("sketch.rectangle.two_point");
    h.step(&json!({"move": {"sketch": [24, 10]}, "alt": true})).unwrap();
    h.step(&json!({"text": "30", "alt": true})).unwrap();
    tab(&mut h, false);
    h.step(&json!({"text": "12"})).unwrap();
    h.step(&json!({"key": "Enter"})).unwrap();
    let state = h.app.session.world_state();
    let sketch = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    assert_eq!(sketch.profiles.len(), 1, "{:?}", h.app.status);
    assert!((sketch.profiles[0].area - 360.0).abs() < 1e-6);
    assert!(!sketch.sketch.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::Equal { .. })));
}
