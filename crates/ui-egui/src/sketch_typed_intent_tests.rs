//! Keyboard-authored intent must survive stage transitions and later model edits.
use crate::scenario::Harness;
use serde_json::json;
use solidcore_record::Vec2;
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::sketch::{ConstraintKind, CurveKind};

fn start(command: &str) -> Harness {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.step(&json!({"start":command})).unwrap();
    h.step(&json!({"click":{"world":[0,0,0]}})).unwrap();
    h.step(&json!({"move":{"world":[20,0,0]}})).unwrap();
    h
}
fn type_value(h: &mut Harness, expression: &str) {
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":expression})).unwrap();
}
fn enter(h: &mut Harness) {
    h.step(&json!({"key":"Enter"})).unwrap();
}
fn constraints(h: &Harness) -> Vec<ConstraintKind> {
    h.app
        .session
        .world_state()
        .unwrap()
        .sketch(h.app.session.active_sketch.unwrap())
        .unwrap()
        .sketch
        .constraints
        .iter()
        .map(|c| c.kind.clone())
        .collect()
}

#[test]
fn typed_arc_radius_and_signed_sweep_survive_stages_parameter_edit_and_reopen() {
    let mut h = start("sketch.arc.center_point");
    h.app.run("parameters.add", json!({"name":"arc_radius","expression":"5 mm"})).unwrap();
    type_value(&mut h, "arc_radius");
    enter(&mut h);
    let tool = h.app.tool.as_ref().unwrap();
    assert_eq!(tool.pts.len(), 2);
    assert_eq!(tool.dim_history[0].value, "arc_radius");
    assert_eq!(tool.dims[0].label, "Sweep");
    type_value(&mut h, "-90 deg");
    enter(&mut h);
    assert!(h.app.tool.as_ref().unwrap().dim_history.is_empty());
    let kinds = constraints(&h);
    assert!(kinds.iter().any(|k| matches!(k, ConstraintKind::Radius{value,..} if (*value-5.0).abs()<1e-7)));
    assert!(kinds.iter().any(|k| matches!(k, ConstraintKind::ArcSweep{value,..} if (*value+std::f64::consts::FRAC_PI_2).abs()<1e-7)));
    h.app.run("parameters.change", json!({"name":"arc_radius","expression":"8 mm"})).unwrap();
    let path = h.dir.join("typed-arc.solidcraft");
    h.app.run("file.save", json!({"path":path})).unwrap();
    h.app.run("doc.open", json!({"path":path})).unwrap();
    let state = h.app.session.world_state().unwrap();
    let sketch = &state.sketches[0].sketch;
    let curve = &sketch.curves[0];
    assert!(curve.reversed, "negative typed sweep must remain clockwise");
    let CurveKind::Arc { c, a, .. } = curve.kind else { panic!("expected arc") };
    assert!((sketch.points[c].pos.dist(sketch.points[a].pos) - 8.0).abs() < 1e-6);
    assert!(sketch.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::ArcSweep{value,..} if value<0.0)));
}

#[test]
fn slots_keep_typed_length_and_width_as_dimensions_and_start_fresh() {
    for command in ["sketch.slot.center_to_center", "sketch.slot.overall", "sketch.slot.center_point"] {
        let mut h = start(command);
        type_value(&mut h, "30");
        enter(&mut h);
        assert_eq!(h.app.tool.as_ref().unwrap().dim_history[0].value, "30");
        h.step(&json!({"move":{"world":[15,5,0]}})).unwrap();
        type_value(&mut h, "6");
        enter(&mut h);
        let kinds = constraints(&h);
        assert!(kinds.iter().any(|k| matches!(k,ConstraintKind::Diameter{value,..} if (*value-6.0).abs()<1e-7)), "{command}");
        if command.ends_with("overall") {
            assert!(kinds.iter().any(|k| matches!(k,ConstraintKind::SlotOverallLength{value,..} if (*value-30.0).abs()<1e-7)));
        } else {
            assert!(kinds.iter().any(|k| matches!(k,ConstraintKind::Length{value,..} if (*value-30.0).abs()<1e-7)));
        }
        h.step(&json!({"click":{"world":[50,20,0]}})).unwrap();
        h.step(&json!({"move":{"world":[70,20,0]}})).unwrap();
        let tool = h.app.tool.as_ref().unwrap();
        assert!(tool.dim_history.is_empty());
        assert!(tool.dims.iter().all(|d| !d.locked));
        type_value(&mut h, "12");
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(h.app.tool.as_ref().unwrap().dim_history.is_empty());
        assert!(h.app.tool.as_ref().unwrap().pts.is_empty());
    }
}

#[test]
fn arc_slots_offer_typed_dimensions_and_keep_them_after_creation() {
    for command in ["sketch.slot.arc_center", "sketch.slot.arc_three_point"] {
        let mut h = start(command);
        if command == "sketch.slot.arc_center" {
            assert_eq!(h.app.tool.as_ref().unwrap().dims[0].label, "Radius");
            type_value(&mut h, "20 mm");
            enter(&mut h);
            assert_eq!(h.app.tool.as_ref().unwrap().dims[0].label, "Sweep");
            type_value(&mut h, "90 deg");
            enter(&mut h);
        } else {
            h.step(&json!({"click":{"world":[20,20,0]}})).unwrap();
            h.step(&json!({"click":{"world":[40,0,0]}})).unwrap();
        }
        h.step(&json!({"move":{"world":[40,5,0]}})).unwrap();
        assert_eq!(h.app.tool.as_ref().unwrap().dims[0].label, "Width");
        type_value(&mut h, "6 mm");
        let undo = h.app.session.undo.len();
        enter(&mut h);
        assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step for {command}");
        let kinds = constraints(&h);
        assert!(kinds.iter().any(|k| matches!(k, ConstraintKind::Diameter { value, .. } if (*value - 6.0).abs() < 1e-7)));
        if command == "sketch.slot.arc_center" {
            assert!(kinds.iter().any(|k| matches!(k, ConstraintKind::Radius { value, .. } if (*value - 20.0).abs() < 1e-7)));
            assert!(kinds.iter().any(|k| matches!(k, ConstraintKind::ArcSweep { value, .. } if (*value - std::f64::consts::FRAC_PI_2).abs() < 1e-7)));
        }
        assert!(h.app.tool.as_ref().unwrap().pts.is_empty());
        assert!(h.app.tool.as_ref().unwrap().dim_history.is_empty());
        h.app.run("edit.undo", json!({})).unwrap();
        assert!(constraints(&h).is_empty());
    }
}

#[test]
fn arc_slot_invalid_width_keeps_inputs_and_click_uses_corrected_value() {
    let mut h = start("sketch.slot.arc_center");
    type_value(&mut h, "10 mm");
    enter(&mut h);
    type_value(&mut h, "90 deg");
    enter(&mut h);
    h.step(&json!({"move":{"world":[0,15,0]}})).unwrap();
    type_value(&mut h, "25 mm");
    let undo = h.app.session.undo.len();
    enter(&mut h);
    assert_eq!(h.app.session.undo.len(), undo);
    assert_eq!(h.app.tool.as_ref().unwrap().pts.len(), 3);
    assert_eq!(h.app.tool.as_ref().unwrap().dims[0].value, "25 mm");
    assert!(constraints(&h).is_empty());
    type_value(&mut h, "4 mm");
    h.step(&json!({"click":{"world":[30,10,0]}})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(h.app.tool.as_ref().unwrap().pts.is_empty());
    assert!(constraints(&h).iter().any(|k| matches!(k, ConstraintKind::Diameter { value, .. } if (*value - 4.0).abs() < 1e-7)));
}

#[test]
fn invalid_stage_values_remain_editable_without_partial_geometry() {
    let mut h = start("sketch.arc.center_point");
    type_value(&mut h, "-3");
    enter(&mut h);
    assert_eq!(h.app.tool.as_ref().unwrap().pts.len(), 1);
    assert_eq!(h.app.tool.as_ref().unwrap().dims[0].value, "-3");
    assert!(constraints(&h).is_empty());
    type_value(&mut h, "5");
    enter(&mut h);
    type_value(&mut h, "360 deg");
    enter(&mut h);
    assert_eq!(h.app.tool.as_ref().unwrap().pts.len(), 2);
    assert_eq!(h.app.tool.as_ref().unwrap().dims[0].value, "360 deg");
    assert!(constraints(&h).is_empty());
    type_value(&mut h, "90 deg");
    enter(&mut h);
    assert_eq!(constraints(&h).iter().filter(|c| c.is_dimension()).count(), 2);

    let mut slot = start("sketch.slot.overall");
    type_value(&mut slot, "4");
    enter(&mut slot);
    type_value(&mut slot, "6");
    enter(&mut slot);
    assert_eq!(slot.app.tool.as_ref().unwrap().pts.len(), 2);
    assert_eq!(slot.app.tool.as_ref().unwrap().dim_history[0].value, "4");
    assert_eq!(slot.app.tool.as_ref().unwrap().dims[0].value, "6");
    assert!(constraints(&slot).is_empty());
    type_value(&mut slot, "2");
    enter(&mut slot);
    assert_eq!(constraints(&slot).iter().filter(|c| c.is_dimension()).count(), 2);
}

#[test]
fn polygon_radius_is_persistent_and_fractional_sides_are_rejected() {
    let mut h = start("sketch.polygon.inscribed");
    type_value(&mut h, "10");
    h.step(&json!({"key":"Tab"})).unwrap();
    type_value(&mut h, "5.5");
    enter(&mut h);
    assert_eq!(h.app.tool.as_ref().unwrap().pts.len(), 1);
    assert!(constraints(&h).is_empty());
    type_value(&mut h, "5");
    enter(&mut h);
    assert!(constraints(&h).iter().any(|c| matches!(c,ConstraintKind::Radius{value,..} if (*value-10.0).abs()<1e-7)));
    let state = h.app.session.world_state().unwrap();
    let sketch = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    assert_eq!(sketch.curves.iter().filter(|c| !c.construction).count(), 5);
}

/// "By side" offers the side length and the number of sides, as the other polygon modes do
/// (#11271): the typed length becomes a dimension of the first side.
#[test]
fn edge_polygon_takes_a_typed_side_length_and_side_count() {
    let mut h = start("sketch.polygon.edge");
    let labels: Vec<_> = h.app.tool.as_ref().unwrap().dims.iter().map(|d| d.label).collect();
    assert_eq!(labels, ["Length", "Sides"]);
    type_value(&mut h, "12");
    h.step(&json!({"key":"Tab"})).unwrap();
    type_value(&mut h, "5");
    let undo = h.app.session.undo.len();
    enter(&mut h);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(constraints(&h).iter().any(|c| matches!(c, ConstraintKind::Length { value, .. } if (*value - 12.0).abs() < 1e-7)));
    // The design's own state: this sketch is in no component, so it matches the world view.
    let state = h.app.session.model.state();
    let sketch = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    let sides: Vec<f64> = sketch
        .curves
        .iter()
        .filter(|c| !c.construction)
        .filter_map(|c| match c.kind {
            CurveKind::Line { a, b } => Some(sketch.points[a].pos.dist(sketch.points[b].pos)),
            _ => None,
        })
        .collect();
    assert_eq!(sides.len(), 5);
    assert!(sides.iter().all(|l| (l - 12.0).abs() < 1e-6), "{sides:?}");
    // The first side runs from the first click towards the cursor.
    let CurveKind::Line { a, b } = sketch.curves[0].kind else { panic!("expected the first side") };
    assert!(sketch.points[a].pos.dist(solidcore_record::Vec2::new(0.0, 0.0)) < 1e-6);
    assert!(sketch.points[b].pos.dist(solidcore_record::Vec2::new(12.0, 0.0)) < 1e-6);
}

#[test]
fn line_angle_survives_drag_and_one_undo_removes_geometry_and_dimensions() {
    let mut h = start("sketch.line");
    let undo = h.app.session.undo.len();
    type_value(&mut h, "10");
    h.step(&json!({"key":"Tab"})).unwrap();
    type_value(&mut h, "30 deg");
    enter(&mut h);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(h.app.tool.as_ref().unwrap().dims.iter().all(|d| !d.locked), "next chained segment starts without stale locks");
    h.step(&json!({"key":"Escape"})).unwrap();
    h.step(&json!({"key":"Escape"})).unwrap();
    h.step(&json!({"drag":[{"world":[8.660254037844386,5,0]},{"world":[16,14,0]}]})).unwrap();
    let state = h.app.session.world_state().unwrap();
    let sketch = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    let CurveKind::Line { a, b } = sketch.curves[0].kind else { panic!("expected line") };
    let vector = sketch.points[b].pos - sketch.points[a].pos;
    assert!((vector.len() - 10.0).abs() < 1e-6);
    assert!((vector.angle() - 30f64.to_radians()).abs() < 1e-6);
    // A drag may itself add an undo entry. Undo that before the original atomic draw.
    while h.app.session.undo.len() > undo + 1 {
        h.app.run("edit.undo", json!({})).unwrap();
    }
    h.app.run("edit.undo", json!({})).unwrap();
    assert!(constraints(&h).is_empty());
    assert!(h.app.session.world_state().unwrap().sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch.curves.is_empty());
}

#[test]
fn typed_drawing_preserves_construction_and_centerline_modes_at_undo_limit() {
    for mode in [1, 2] {
        let mut h = start("sketch.line");
        crate::sketch_palette::set_mode(&mut h.app, mode);
        let snapshot = h.app.session.undo.last().unwrap().clone();
        h.app.session.undo.resize(200, snapshot);
        type_value(&mut h, "10");
        h.step(&json!({"key":"Tab"})).unwrap();
        type_value(&mut h, "30 deg");
        enter(&mut h);
        assert_eq!(h.app.session.undo.len(), 200);
        let state = h.app.session.world_state().unwrap();
        let sketch = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        assert_eq!(sketch.curves[0].construction, mode == 1);
        assert_eq!(sketch.curves[0].centerline, mode == 2);
        h.app.run("edit.undo", json!({})).unwrap();
        let state = h.app.session.world_state().unwrap();
        let sketch = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        assert!(sketch.curves.is_empty());
        assert!(sketch.constraints.is_empty());
        crate::sketch_palette::set_mode(&mut h.app, 0);
    }
}

/// The third pick of a Center Point Arc reaches existing geometry under the pointer: the value
/// box (the arc's sweep, a slot's width) stays clear of the cursor instead of sitting on it
/// (#2201).
#[test]
fn the_arc_end_pick_snaps_to_a_point_under_the_value_box() {
    for (command, picks, end) in [
        ("sketch.arc.center_point", [[0.0, 0.0], [10.0, 0.0]], [0.0, 10.0]),
        ("sketch.slot.center_to_center", [[-10.0, 0.0], [10.0, 0.0]], [0.0, 10.0]),
    ] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.line", json!({"points":[[0, 0], end], "infer": false})).unwrap();
        h.app.run("sketch.construction", json!({"curves":["l1"]})).unwrap();
        h.call("ui.view", json!({"view":"top"}));
        h.app.cam.distance = 60.0;
        h.app.cam.target = solidcore_record::Vec3::new(0.0, 5.0, 0.0);
        h.settle();
        h.step(&json!({"start":command})).unwrap();
        for p in picks {
            h.step(&json!({"click":{"world":[p[0], p[1], 0]}})).unwrap();
        }
        h.step(&json!({"move":{"world":[end[0], end[1], 0]}})).unwrap();
        let cursor = h.at(&json!({"world":[end[0], end[1], 0]})).unwrap();
        let cursor = egui::pos2(cursor[0] as f32, cursor[1] as f32);
        let blocking = h.ctx.memory(|m| m.area_rect(egui::Id::unique(("sc_dim_area", 0usize)))).filter(|r| r.expand(2.0).contains(cursor));
        assert!(blocking.is_none(), "{command}: the value box {blocking:?} sits under the cursor {cursor:?}");
        h.step(&json!({"click":{"world":[end[0], end[1], 0]}})).unwrap();
        // A slot's third pick sets its width only; the arc's end takes the point it lands on.
        if command != "sketch.arc.center_point" {
            continue;
        }
        let state = h.app.session.world_state().unwrap();
        let sketch = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        let line_end = sketch.curves.iter().find(|c| c.id == "l1").and_then(|c| match c.kind {
            CurveKind::Line { b, .. } => Some(b),
            _ => None,
        });
        let snapped =
            sketch.constraints.iter().any(|c| matches!(&c.kind, ConstraintKind::Coincident { p, q } if Some(*p) == line_end || Some(*q) == line_end))
                || sketch.curves.iter().filter(|c| c.id != "l1").any(|c| match c.kind {
                    CurveKind::Arc { a, b, .. } => Some(a) == line_end || Some(b) == line_end,
                    _ => false,
                });
        assert!(snapped, "{command}: the third pick snapped to the line's end: {:?}", sketch.constraints.iter().map(|c| &c.kind).collect::<Vec<_>>());
    }
}

/// A Center Point Arc is drawn with three clicks, its value boxes optional: the Radius box never
/// sits under the cursor, so the second click places the start point instead of landing in the
/// box and waiting for Enter (#12208, #12205). Small radii in every direction, where a box at the
/// radius's middle covered the cursor.
#[test]
fn a_center_point_arc_takes_three_clicks_without_enter() {
    for (radius, angle) in [(2.0f64, 0.0f64), (4.0, 0.0), (4.0, 90.0), (4.0, 180.0), (4.0, 270.0), (6.0, 45.0), (15.0, 0.0)] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.call("ui.view", json!({"view":"top"}));
        h.app.cam.distance = 60.0;
        h.app.cam.target = solidcore_record::Vec3::new(0.0, 0.0, 0.0);
        h.settle();
        h.step(&json!({"start":"sketch.arc.center_point"})).unwrap();
        h.step(&json!({"click":{"world":[0, 0, 0]}})).unwrap();
        let (s, c) = angle.to_radians().sin_cos();
        let start = [radius * c, radius * s];
        h.step(&json!({"move":{"world":[start[0], start[1], 0]}})).unwrap();
        let cursor = h.at(&json!({"world":[start[0], start[1], 0]})).unwrap();
        let cursor = egui::pos2(cursor[0] as f32, cursor[1] as f32);
        let blocking = h.ctx.memory(|m| m.area_rect(egui::Id::unique(("sc_dim_area", 0usize)))).filter(|r| r.expand(2.0).contains(cursor));
        assert!(blocking.is_none(), "radius {radius} at {angle} deg: the Radius box {blocking:?} sits under the cursor {cursor:?}");
        h.step(&json!({"click":{"world":[start[0], start[1], 0]}})).unwrap();
        assert_eq!(h.app.tool.as_ref().unwrap().pts.len(), 2, "radius {radius} at {angle} deg: the click placed the start point");
        let (s, c) = (angle + 90.0).to_radians().sin_cos();
        h.step(&json!({"move":{"world":[radius * c, radius * s, 0]}})).unwrap();
        h.step(&json!({"click":{"world":[radius * c, radius * s, 0]}})).unwrap();
        let state = h.app.session.world_state().unwrap();
        let sketch = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        let arc = sketch.curves.iter().find_map(|curve| match curve.kind {
            CurveKind::Arc { c, a, .. } => Some(sketch.points[a].pos.dist(sketch.points[c].pos)),
            _ => None,
        });
        assert!(arc.is_some_and(|r| (r - radius).abs() < 0.5), "radius {radius} at {angle} deg: three clicks made the arc, got {arc:?}");
        assert!(!sketch.constraints.iter().any(|c| c.kind.is_dimension()), "radius {radius} at {angle} deg: clicks alone add no typed dimension");
    }
}

/// A value box that follows the cursor has its whole text selected, so typing replaces it.
/// Right Arrow (or Left, Home, End) keeps the shown value and moves the caret into it, so
/// typing adds to it: `-5` after the length makes the expression `<length>-5` (#20607).
#[test]
fn a_caret_key_in_a_following_value_box_keeps_its_value_to_add_to() {
    let mut h = start("sketch.line");
    let live = h.app.tool.as_ref().unwrap().dims[0].value.clone();
    assert!(!live.is_empty() && !h.app.tool.as_ref().unwrap().dims[0].locked, "the first box shows the live value: {live:?}");
    h.step(&json!({"key":"ArrowRight"})).unwrap();
    h.step(&json!({"text":"-5"})).unwrap();
    let b = &h.app.tool.as_ref().unwrap().dims[0];
    assert!(b.locked, "{b:?}");
    assert_eq!(b.value, format!("{live}-5"));
}

/// An L of two 50 mm lines meeting at the origin, with the 3-Point Arc started and its ends
/// clicked on the lines' free ends (#20719).
fn l_with_arc_ends(tangent_snap: bool) -> Harness {
    let mut h = Harness::new();
    h.app.preferences.sketch_arc_tangent_snap = tangent_snap;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[0,50]]})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[50,0]]})).unwrap();
    h.app.cam.distance = 150.0;
    h.settle();
    h.step(&json!({"start":"sketch.arc.three_point"})).unwrap();
    for at in [[0.0, 50.0], [50.0, 0.0]] {
        h.step(&json!({"move":{"sketch":at}})).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"sketch":at}})).unwrap();
        h.frames(2);
    }
    h
}

/// Whether a Coincident constraint holds the (first) arc's centre point on another point. Its
/// ends are held on the L's line ends by the clicks that placed them, so only the centre tells.
fn centre_held(h: &Harness) -> bool {
    let st = h.app.session.world_state().unwrap();
    let sk = &st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    let Some(centre) = sk.curves.iter().find_map(|c| match c.kind {
        CurveKind::Arc { c, .. } => Some(c),
        _ => None,
    }) else {
        return false;
    };
    sk.constraints.iter().any(|k| matches!(k.kind, ConstraintKind::Coincident { p, q } if p == centre || q == centre))
}

fn arc_numbers(h: &Harness) -> (f64, f64, Vec2) {
    let st = h.app.session.world_state().unwrap();
    let sk = &st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    let arc = sk.curves.iter().position(|c| matches!(c.kind, CurveKind::Arc { .. })).expect("the arc");
    let poly = sk.polyline(arc);
    let (first, last) = (poly[0], poly[poly.len() - 1]);
    (sk.radius(arc).unwrap(), first.dist(last), poly[poly.len() / 2])
}

#[test]
fn three_point_arc_third_point_offers_radius_and_sweep_boxes() {
    let mut h = l_with_arc_ends(true);
    h.step(&json!({"move":{"sketch":[10.0, 10.0]}})).unwrap();
    h.frames(2);
    let labels: Vec<_> = h.app.tool.as_ref().unwrap().dims.iter().map(|d| d.label).collect();
    assert_eq!(labels, ["Radius", "Sweep"]);
    type_value(&mut h, "50");
    enter(&mut h);
    assert!(constraints(&h).iter().any(|k| matches!(k, ConstraintKind::Radius { value, .. } if (*value - 50.0).abs() < 1e-7)));
    let (radius, chord, middle) = arc_numbers(&h);
    assert!((radius - 50.0).abs() < 1e-6, "{radius}");
    assert!((chord - 50.0 * 2f64.sqrt()).abs() < 1e-6, "{chord}");
    // The cursor was inside the corner: the short arc, not the long one round the outside.
    assert!(middle.x + middle.y < 50.0, "{middle:?}");
}

#[test]
fn three_point_arc_typed_sweep_gives_that_sweep_and_too_small_a_radius_is_refused() {
    let mut h = l_with_arc_ends(true);
    h.step(&json!({"move":{"sketch":[10.0, 10.0]}})).unwrap();
    h.frames(2);
    h.step(&json!({"key":"Tab"})).unwrap();
    type_value(&mut h, "90 deg");
    enter(&mut h);
    let kinds = constraints(&h);
    assert!(kinds.iter().any(|k| matches!(k, ConstraintKind::ArcSweep { value, .. } if (*value - std::f64::consts::FRAC_PI_2).abs() < 1e-7)));
    assert!(!kinds.iter().any(|k| matches!(k, ConstraintKind::Radius { .. })));
    let (radius, ..) = arc_numbers(&h);
    assert!((radius - 50.0).abs() < 1e-6, "{radius}");

    let mut small = l_with_arc_ends(true);
    small.step(&json!({"move":{"sketch":[10.0, 10.0]}})).unwrap();
    small.frames(2);
    type_value(&mut small, "20");
    enter(&mut small);
    assert_eq!(small.app.tool.as_ref().unwrap().pts.len(), 2, "the arc waits for a radius that fits");
    assert!(
        !small
            .app
            .session
            .world_state()
            .unwrap()
            .sketch(small.app.session.active_sketch.unwrap())
            .unwrap()
            .sketch
            .curves
            .iter()
            .any(|c| matches!(c.kind, CurveKind::Arc { .. }))
    );
}

#[test]
fn three_point_arc_snaps_tangent_to_both_lines_of_the_l() {
    let mut h = l_with_arc_ends(true);
    // A little off the tangent arc's middle, (14.64, 14.64).
    let off = [14.9, 14.4];
    h.step(&json!({"move":{"sketch":off}})).unwrap();
    h.frames(2);
    let hover = h.app.tool.as_ref().unwrap().hover.clone().unwrap();
    let centre = Vec2::new(50.0, 50.0);
    assert!((hover.0.dist(centre) - 50.0).abs() < 1e-9, "snapped onto the R50 circle: {:?}", hover.0);
    h.step(&json!({"click":{"sketch":off}})).unwrap();
    h.frames(2);
    let (radius, chord, middle) = arc_numbers(&h);
    assert!((radius - 50.0).abs() < 1e-3, "{radius}");
    assert!((chord - 50.0 * 2f64.sqrt()).abs() < 1e-3, "{chord}");
    assert!(middle.x + middle.y < 50.0, "{middle:?}");
    let tangents = constraints(&h).iter().filter(|k| matches!(k, ConstraintKind::Tangent { .. })).count();
    assert!(tangents >= 1, "a Tangent constraint with the lines it starts and ends on");
}

#[test]
fn three_point_arc_centre_is_the_circle_through_its_points_and_centre_point_arc_is_unchanged() {
    let k = 50.0 * 0.5f64.sqrt();
    let (centre, radius, sweep) =
        crate::sketch_dims::three_point_circle(Vec2::new(0.0, 50.0), Vec2::new(50.0, 0.0), Vec2::new(50.0 - k, 50.0 - k)).unwrap();
    assert!(centre.dist(Vec2::new(50.0, 50.0)) < 1e-9 && (radius - 50.0).abs() < 1e-9 && (sweep - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    // The long way round, through the far side of the circle.
    let (_, _, long) = crate::sketch_dims::three_point_circle(Vec2::new(0.0, 50.0), Vec2::new(50.0, 0.0), Vec2::new(50.0 + k, 50.0 + k)).unwrap();
    assert!((long - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    let labels = |cmd, n| crate::sketch_dims::boxes_for(cmd, n).into_iter().map(|b| b.label).collect::<Vec<_>>();
    assert_eq!(labels("sketch.arc.center_point", 1), ["Radius"]);
    assert_eq!(labels("sketch.arc.center_point", 2), ["Sweep"]);
    assert_eq!(labels("sketch.arc.three_point", 1), Vec::<&str>::new());
    assert_eq!(labels("sketch.arc.three_point", 2), ["Radius", "Sweep"]);
}

/// The tangent snap is opt-in: with the tool option off the same hover and click take the snap the
/// arc had before, on no tangent circle, and add no Tangent constraint (#20719).
#[test]
fn three_point_arc_tangent_snap_is_off_unless_turned_on() {
    assert!(!crate::prefs::Prefs::default().sketch_arc_tangent_snap);
    let mut h = l_with_arc_ends(false);
    let off = [14.9, 14.4];
    h.step(&json!({"move":{"sketch":off}})).unwrap();
    h.frames(2);
    let hover = h.app.tool.as_ref().unwrap().hover.clone().unwrap();
    assert!((hover.0.dist(Vec2::new(50.0, 50.0)) - 50.0).abs() > 1e-6, "not snapped tangent: {:?}", hover.0);
    h.step(&json!({"click":{"sketch":off}})).unwrap();
    h.frames(2);
    assert!(!constraints(&h).iter().any(|k| matches!(k, ConstraintKind::Tangent { .. })));
}

/// #20719: the Radius and Sweep boxes and the centre cross are part of the tangent mode. With it
/// off the 3-Point Arc offers no box, so a typed value locks nothing and the click draws the arc
/// through the pointer as it always did; with it on both are there.
#[test]
fn three_point_arc_boxes_and_centre_cross_are_off_unless_the_mode_is_on() {
    let mut h = l_with_arc_ends(false);
    h.step(&json!({"move":{"sketch":[10.0, 10.0]}})).unwrap();
    h.frames(2);
    assert!(h.app.tool.as_ref().unwrap().dims.is_empty(), "no Radius or Sweep box");
    assert_eq!(crate::scenario::count_of("arc_centre_marker"), None, "no centre cross");
    let t = h.app.tool.as_ref().unwrap();
    assert!(!crate::sketch_dims::has_locked(t, "Radius") && !crate::sketch_dims::has_locked(t, "Sweep"), "no value is locked");
    assert!(crate::sketch_dims::dimensions(t).as_object().is_some_and(|d| d.is_empty()), "nothing becomes a dimension");
    assert!(!constraints(&h).iter().any(|k| matches!(k, ConstraintKind::Radius { .. } | ConstraintKind::ArcSweep { .. })));

    let mut on = l_with_arc_ends(true);
    on.step(&json!({"move":{"sketch":[10.0, 10.0]}})).unwrap();
    on.frames(2);
    assert_eq!(on.app.tool.as_ref().unwrap().dims.len(), 2);
    assert_eq!(crate::scenario::count_of("arc_centre_marker"), Some(1.0));
}

/// #20719: a click commits the point the hover showed. An arc ends at (10, 0) with its centre
/// below; the new arc runs from (0, 1) to (10, 0). The Line tool's tangent snap would put the click
/// on the horizontal line (5, 0) instead of the tangent circle's point near (5, 0.25).
#[test]
fn three_point_arc_click_commits_the_tangent_point_the_hover_showed() {
    let mut h = Harness::new();
    h.app.preferences.sketch_arc_tangent_snap = true;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    let k = 100.0 * 0.5f64.sqrt();
    h.app.run("sketch.arc.three_point", json!({"start":[10,0],"end":[110,-100],"through":[10.0 + k, -100.0 + k]})).unwrap();
    // Close enough that the click at (0, 1) is not taken by the origin's snap a millimetre away.
    h.app.cam.distance = 50.0;
    h.settle();
    h.step(&json!({"start":"sketch.arc.three_point"})).unwrap();
    for at in [[0.0, 1.0], [10.0, 0.0]] {
        h.step(&json!({"move":{"sketch":at}})).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"sketch":at}})).unwrap();
        h.frames(2);
    }
    let cursor = [5.0, 0.25];
    h.step(&json!({"move":{"sketch":cursor}})).unwrap();
    h.frames(2);
    let hover = h.app.tool.as_ref().unwrap().hover.clone().unwrap();
    // The circle through (0, 1) and (10, 0) tangent to the arc at (10, 0): centre (10, 50.5).
    assert!((hover.0.dist(Vec2::new(10.0, 50.5)) - 50.5).abs() < 1e-6, "{:?}", hover.0);
    h.step(&json!({"click":{"sketch":cursor}})).unwrap();
    h.frames(2);
    let st = h.app.session.world_state().unwrap();
    let sk = &st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    let new_arc = sk.curves.iter().rposition(|c| matches!(c.kind, CurveKind::Arc { .. })).expect("the new arc");
    let (centre, radius) = (sk.center(new_arc).unwrap(), sk.radius(new_arc).unwrap());
    assert!(centre.dist(Vec2::new(10.0, 50.5)) < 1e-3 && (radius - 50.5).abs() < 1e-3, "{centre:?} {radius}");
    assert!(constraints(&h).iter().any(|k| matches!(k, ConstraintKind::Tangent { .. })));
}

/// #20719: the arc from one free end of the L to the other, drawn on the outside of the corner. The
/// pointer brings the arc's centre to the corner, so the arc is the exact quarter circle (R 50,
/// 90 deg) and the click keeps its centre on the corner. A pointer with no point near its centre
/// gets the plain arc, and with the tool's snaps off nothing is snapped or added.
#[test]
fn three_point_arc_centre_snaps_to_the_corner_of_the_l() {
    let near_corner = [35.4, 35.3];
    let mut h = l_with_arc_ends(true);
    h.step(&json!({"move":{"sketch":near_corner}})).unwrap();
    h.frames(2);
    let hover = h.app.tool.as_ref().unwrap().hover.clone().unwrap();
    assert!((hover.0.len() - 50.0).abs() < 1e-9, "on the R50 circle round the corner: {:?}", hover.0);
    h.step(&json!({"click":{"sketch":near_corner}})).unwrap();
    h.frames(2);
    let (radius, chord, middle) = arc_numbers(&h);
    assert!((radius - 50.0).abs() < 1e-6, "{radius}");
    assert!((chord - 50.0 * 2f64.sqrt()).abs() < 1e-6, "a quarter circle: {chord}");
    assert!(middle.x + middle.y > 50.0, "the outside of the corner: {middle:?}");
    let st = h.app.session.world_state().unwrap();
    let sk = &st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    let arc = sk.curves.iter().position(|c| matches!(c.kind, CurveKind::Arc { .. })).expect("the arc");
    assert!(sk.center(arc).unwrap().len() < 1e-6, "{:?}", sk.center(arc));
    assert!(centre_held(&h), "the centre is held on the corner");

    // No point near the arc's centre: the plain arc through the pointer, and no constraint.
    let mut plain = l_with_arc_ends(true);
    // (40, 40): on screen, and the arc through it has its centre at (11.7, 11.7), far from any point.
    plain.step(&json!({"move":{"sketch":[40.0, 40.0]}})).unwrap();
    plain.frames(2);
    let hover = plain.app.tool.as_ref().unwrap().hover.clone().unwrap();
    assert!(hover.0.dist(Vec2::new(40.0, 40.0)) < 0.5, "{:?}", hover.0);
    plain.step(&json!({"click":{"sketch":[40.0, 40.0]}})).unwrap();
    plain.frames(2);
    assert!(!centre_held(&plain), "a plain arc's centre is held on nothing");

    // The tool's snaps off: the corner pulls nothing.
    let mut off = l_with_arc_ends(false);
    off.step(&json!({"move":{"sketch":near_corner}})).unwrap();
    off.frames(2);
    let hover = off.app.tool.as_ref().unwrap().hover.clone().unwrap();
    assert!((hover.0.len() - 50.0).abs() > 1e-6 || hover.0.dist(Vec2::new(near_corner[0], near_corner[1])) < 0.5, "{:?}", hover.0);
    off.step(&json!({"click":{"sketch":near_corner}})).unwrap();
    off.frames(2);
    assert!(!centre_held(&off), "with the snaps off the centre is held on nothing");
}
