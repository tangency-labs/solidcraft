use super::*;
use solidcraft_engine::sketch::CurveKind;

#[test]
fn three_point_arc_follows_its_snapped_through_point_after_dimension_change() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[-20,0],[20,0]],"ids":["base"]})).unwrap();
    h.app.run("sketch.constraint.fix", json!({"entity":"base"})).unwrap();
    h.app.run("sketch.line", json!({"points":["origin",[0,10]],"ids":["height"],"infer":true})).unwrap();
    let dim = h.app.run("sketch.dimension", json!({"entities":["height"],"value":10})).unwrap();
    h.settle();
    h.step(&json!({"start":"sketch.arc.three_point"})).unwrap();
    for point in [[-20, 0, 0], [20, 0, 0], [0, 10, 0]] {
        h.step(&json!({"click":{"world":point}})).unwrap();
    }
    h.app.run("parameters.change", json!({"name":dim["param"],"expression":"15 mm"})).unwrap();
    let sid = h.app.session.active_sketch.unwrap();
    let state = h.app.session.model.state();
    let ss = state.sketch(sid).unwrap();
    let sk = &ss.sketch;
    let arc = sk.curves.iter().position(|c| matches!(c.kind, CurveKind::Arc { .. })).unwrap();
    let through = sk.point(sk.resolve_point("height.end").unwrap()).unwrap();
    assert!(ss.report.ok());
    assert!(through.dist(Vec2::new(0.0, 15.0)) < 1e-6, "{through:?}");
    let distance = sk.shape(arc).unwrap().dist(through);
    assert!(distance < 1e-6, "the arc misses its snapped through point by {distance} mm");
}

/// #6645: the centre-point arc runs the way the cursor went round the centre from the start.
/// Round clockwise to the top it is three quarters through the bottom; counter-clockwise, as
/// before, the quarter through the upper right. The start keeps its place and its name. With
/// the preference off it is counter-clockwise whichever way the cursor went, as it always was.
#[test]
fn center_point_arc_runs_the_way_the_cursor_went_round() {
    let clockwise = &[[7, -7, 0], [0, -10, 0], [-10, 0, 0], [-7, 7, 0]][..];
    let quarter = Vec2::new(7.071_067_811_865_476, 7.071_067_811_865_476);
    for (follow, path, reversed, through) in
        [(true, clockwise, true, Vec2::new(0.0, -10.0)), (true, &[[7, 7, 0]][..], false, quarter), (false, clockwise, false, quarter)]
    {
        let mut h = crate::scenario::Harness::new();
        assert!(h.app.preferences.center_arc_follows_cursor, "on by default");
        h.app.preferences.center_arc_follows_cursor = follow;
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.settle();
        h.step(&json!({"start":"sketch.arc.center_point"})).unwrap();
        h.step(&json!({"click":{"world":[0, 0, 0]}})).unwrap();
        h.step(&json!({"click":{"world":[10, 0, 0]}})).unwrap();
        for point in path {
            h.step(&json!({"move":{"world":point}})).unwrap();
        }
        h.step(&json!({"move":{"world":[0, 10, 0]}})).unwrap();
        let sweep = h.app.tool.as_ref().and_then(|t| t.hover.as_ref().map(|p| crate::tools::center_arc_sweep(t, p.0))).unwrap();
        let expected = if reversed { -270.0 } else { 90.0 };
        assert!((sweep.to_degrees() - expected).abs() < 1e-6, "previewed sweep {}°", sweep.to_degrees());
        h.step(&json!({"click":{"world":[0, 10, 0]}})).unwrap();
        let sid = h.app.session.active_sketch.unwrap();
        let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
        let arc = sk.curves.iter().position(|c| matches!(c.kind, CurveKind::Arc { .. })).unwrap();
        assert_eq!(sk.curves[arc].reversed, reversed, "{:?}", h.app.status);
        let id = sk.curves[arc].id.clone();
        let start = sk.point(sk.resolve_point(&format!("{id}.start")).unwrap()).unwrap();
        assert!(start.dist(Vec2::new(10.0, 0.0)) < 1e-6, "{start:?}");
        let off = sk.shape(arc).unwrap().dist(through);
        assert!(off < 1e-6, "the arc misses {through:?} by {off} mm");
    }
}

#[test]
fn clicked_periodic_fit_closes_at_start_and_right_click_keeps_natural() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.settle();
    h.step(&json!({"start":"sketch.spline.fit_point"})).unwrap();
    for point in [[2, 2, 0], [8, 2, 0], [9, 7, 0], [1, 6, 0]] {
        h.step(&json!({"click":{"world":point}})).unwrap();
    }
    let sid = h.app.session.active_sketch.unwrap();
    assert!(h.app.session.model.state().sketch(sid).unwrap().sketch.curves.is_empty());
    h.step(&json!({"move":{"world":[2,2,0]}})).unwrap();
    assert_eq!(h.app.tool.as_ref().unwrap().hover.as_ref().unwrap().1.as_deref(), Some("Close smooth loop"));
    h.step(&json!({"click":{"world":[2,2,0]}})).unwrap();
    let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
    assert_eq!(sk.curves.len(), 1, "{:?}", h.app.status);
    assert!(matches!(sk.curves[0].kind, CurveKind::Spline { periodic: true, .. }));
    sk.validate_periodic_splines().unwrap();
    assert_eq!(solidcraft_engine::sketch::find_profiles(&sk).len(), 1);
    assert!(h.app.tool.as_ref().unwrap().pts.is_empty());
    for point in [[15, 2, 0], [20, 2, 0], [20, 6, 0]] {
        h.step(&json!({"click":{"world":point}})).unwrap();
    }
    h.step(&json!({"click":{"world":[20,6,0]},"button":"right"})).unwrap();
    let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
    assert_eq!(sk.curves.len(), 2);
    assert!(matches!(sk.curves[1].kind, CurveKind::Spline { periodic: false, .. }));
}

/// Curve ids in the open dialog's first selection input.
fn dialog_curves(h: &crate::scenario::Harness) -> Vec<String> {
    h.app
        .dialog
        .as_ref()
        .and_then(|d| d.inputs.first())
        .map(|i| {
            i.items.iter().filter_map(|x| if let solidcraft_engine::Sel::SketchCurve { id, .. } = x { Some(id.clone()) } else { None }).collect()
        })
        .unwrap_or_default()
}

/// Type `value` into the open Offset window's Distance row, then apply with OK.
fn offset_distance(h: &mut crate::scenario::Harness, value: &str) {
    h.step(&json!({"click":{"handle":"dialog:Distance"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":value})).unwrap();
    h.step(&json!({"widget":"OK"})).unwrap();
}

#[test]
fn offset_window_distance_and_flip_offset_an_ellipse_exactly_and_undo_restores() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.ellipse", json!({"center":[0,0],"major":[10,0],"minor_radius":5})).unwrap();
    h.settle();
    let sid = h.app.session.active_sketch.unwrap();
    h.step(&json!({"start":"sketch.offset"})).unwrap();
    // Major tip is also a source handle: Offset must choose the actual curve.
    h.step(&json!({"click":{"world":[10,0,0]}})).unwrap();
    assert_eq!(dialog_curves(&h), vec!["e1".to_string()], "{:?}", h.app.status);
    // Nothing is committed until the window is applied.
    assert!(h.app.session.model.state().sketch(sid).unwrap().sketch.freeform_offsets.is_empty());
    h.step(&json!({"click":{"handle":"dialog:Flip"}})).unwrap();
    offset_distance(&mut h, "1 mm");
    assert!(h.app.dialog.is_none(), "{:?}", h.app.status);
    let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
    assert_eq!(sk.freeform_offsets.len(), 1, "{:?}", h.app.status);
    assert!((sk.freeform_offsets[0].distance + 1.0).abs() < 1e-4, "{:?}", sk.freeform_offsets[0].distance);
    let first = sk.freeform_offsets[0].output.clone();
    let saved = h.app.session.doc.to_json();
    h.step(&json!({"start":"sketch.offset"})).unwrap();
    h.step(&json!({"click":{"world":[11,0,0]}})).unwrap();
    assert_eq!(dialog_curves(&h), vec![first], "{:?}", h.app.status);
    h.step(&json!({"click":{"handle":"dialog:Flip"}})).unwrap();
    offset_distance(&mut h, "0.5 mm");
    let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
    assert_eq!(sk.freeform_offsets.len(), 2, "{:?}", h.app.status);
    assert!((sk.freeform_offsets[1].distance + 0.5).abs() < 1e-4, "{:?}", sk.freeform_offsets[1].distance);
    assert!(sk.freeform_offsets.iter().all(|r| r.certificate == 2));
    assert_eq!(solidcraft_engine::sketch::find_profiles(&sk).len(), 3);
    let last = sk.curve_index(&sk.freeform_offsets[1].output).unwrap();
    assert!(sk.exact_segs(last).unwrap()[0].point_at(0.0).dist(Vec2::new(11.5, 0.0)) < 1e-4);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), saved);
}

#[test]
fn offset_window_without_a_curve_reports_it_and_keeps_the_window_open() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.ellipse", json!({"center":[0,0],"major":[10,0],"minor_radius":5})).unwrap();
    h.settle();
    let sid = h.app.session.active_sketch.unwrap();
    h.step(&json!({"start":"sketch.offset"})).unwrap();
    h.step(&json!({"widget":"OK"})).unwrap();
    let error = h.app.dialog.as_ref().and_then(|d| d.error.clone());
    assert_eq!(error.as_deref(), Some("Select the curves to offset first"));
    assert!(h.app.session.model.state().sketch(sid).unwrap().sketch.freeform_offsets.is_empty());
}

#[test]
fn clicked_extend_endpoint_reaches_exact_ellipse_and_preserves_support() {
    let mut h = crate::scenario::Harness::new();
    h.app.set_origin_shown(true);
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.ellipse", json!({"center":[20,0],"major":[30,0],"minor_radius":5})).unwrap();
    h.app.run("sketch.constraint.fix", json!({"entity":"e1"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,3],[5,3]],"connect":false,"infer":false})).unwrap();
    h.frames(2);
    h.step(&json!({"start":"sketch.extend"})).unwrap();
    h.step(&json!({"click":{"world":[5,3,0]}})).unwrap();
    let st = h.app.session.world_state();
    let ss = st.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    let end = ss.sketch.point(ss.sketch.resolve_point("l1.end").unwrap()).unwrap();
    assert!(end.dist(Vec2::new(12.0, 3.0)) < 1e-7, "clicked endpoint must extend: {end:?}, {:?}", h.app.status);
    assert_eq!(ss.sketch.point(ss.sketch.resolve_point("l1.start").unwrap()), Some(Vec2::new(0.0, 3.0)));
}

#[test]
fn derived_offset_context_action_releases_geometry_and_undo_restores() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.ellipse", json!({"center":[0,0],"major":[10,0],"minor_radius":5})).unwrap();
    let out = h.app.run("sketch.offset", json!({"curves":["e1"],"distance":-1,"chain":false})).unwrap();
    let id = out["curves"][0].as_str().unwrap().to_string();
    h.app.session.selection = vec![solidcraft_engine::Sel::SketchCurve { id: id.clone(), sketch: None }];
    let items = crate::context_menu::items(&h.app, &crate::context_menu::Target::Viewport);
    let item = items.iter().find(|item| item.id == "sketch.offset.break").unwrap();
    assert_eq!(item.params, Some(json!({"curve":id})));
    crate::context_menu::run_item(&mut h.app, item, egui::Pos2::ZERO);
    let sid = h.app.session.active_sketch.unwrap();
    assert!(h.app.session.model.state().sketch(sid).unwrap().sketch.freeform_offsets.is_empty());
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.model.state().sketch(sid).unwrap().sketch.freeform_offsets.len(), 1);
}

#[test]
fn shift_clicks_on_two_curves_extend_both_to_where_they_meet() {
    // A runs right along y=0 to (8,0); B comes down x=12 from (12,6) to (12,2). With Shift, the
    // first click remembers A and the second (on B) extends both to (12,0). Without Shift a click
    // is still the single-curve extend.
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[8,0]],"connect":false,"infer":false})).unwrap();
    h.app.run("sketch.line", json!({"points":[[12,6],[12,2]],"connect":false,"infer":false})).unwrap();
    h.call("ui.view", json!({"view": "top"}));
    h.app.cam.distance = 60.0;
    h.frames(3);
    h.call("ui.start", json!({"command": "sketch.extend"}));
    h.frames(2);
    let undo = h.app.session.undo.len();

    shift_click(&mut h, &json!({"sketch": [6, 0]}));
    assert_eq!(h.app.session.undo.len(), undo, "the first Shift click only remembers its curve");
    shift_click(&mut h, &json!({"sketch": [12, 3]}));
    assert_eq!(h.app.session.undo.len(), undo + 1, "the second runs the two-curve extend, in one step");

    let st = h.app.session.world_state();
    let ss = st.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    for end in ["l1.end", "l2.end"] {
        let p = ss.sketch.point(ss.sketch.resolve_point(end).unwrap()).unwrap();
        assert!(p.dist(Vec2::new(12.0, 0.0)) < 1e-6, "{end} at the meeting point: {p:?}, {:?}", h.app.status);
    }
}

#[test]
fn escape_drops_a_pending_pair_pick() {
    // The first Shift click waits for a second curve. Escape cancels the tool, so a later Shift
    // click on the other curve, with the tool started again, starts afresh and runs nothing.
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[8,0]],"connect":false,"infer":false})).unwrap();
    h.app.run("sketch.line", json!({"points":[[12,6],[12,2]],"connect":false,"infer":false})).unwrap();
    h.call("ui.view", json!({"view": "top"}));
    h.app.cam.distance = 60.0;
    h.frames(3);
    h.call("ui.start", json!({"command": "sketch.extend"}));
    h.frames(2);
    shift_click(&mut h, &json!({"sketch": [6, 0]}));
    h.step(&json!({"key": "Escape"})).unwrap();
    h.call("ui.start", json!({"command": "sketch.extend"}));
    h.frames(2);
    let undo = h.app.session.undo.len();

    shift_click(&mut h, &json!({"sketch": [12, 3]}));
    assert_eq!(h.app.session.undo.len(), undo, "the pending first pick went with Escape");
}

/// A click with Shift held at `at`: hover first so the pointer is over the curve, then press and
/// release with the modifier.
fn shift_click(h: &mut crate::scenario::Harness, at: &Value) {
    let [sx, sy] = h.at(at).expect("on screen");
    let p = egui::pos2(sx as f32, sy as f32);
    let shift = egui::Modifiers { shift: true, ..Default::default() };
    h.app.synthetic.push(egui::Event::PointerMoved(p));
    h.app.synthetic.push(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: shift });
    h.app.synthetic.push(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: shift });
    while !h.app.synthetic.is_empty() {
        h.frame();
    }
    h.frames(2);
}

/// The size fields of the open Fillet or Chamfer window, as if they had been typed.
fn set_size(app: &mut SolveApp, text: &str) {
    match app.dialog.as_mut().map(|d| &mut d.kind) {
        Some(crate::dialogs::Kind::SketchModify(crate::sketch_modify_ui::Mod::Corner { size, distance2, .. })) => {
            *size = text.to_string();
            *distance2 = text.to_string();
        }
        other => panic!("the corner window is not open: {other:?}"),
    }
}

/// Replace a dialog field's contents by typing, the way a person does.
fn retype(h: &mut crate::scenario::Harness, handle: &str, text: &str) {
    h.step(&json!({"click":{"handle":handle}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":text})).unwrap();
    h.frames(2);
}

fn apply(h: &mut crate::scenario::Harness) {
    // Lay the window out once before reading its OK handle. An error line that wraps onto a
    // second row moves the button, and a click on the handle from the frame before then
    // lands above it and commits nothing.
    h.frames(2);
    h.step(&json!({"click":{"handle":"dialog:OK"}})).unwrap();
    h.frames(2);
}

/// Is there a point of the active sketch at `at`?
fn has_point(h: &crate::scenario::Harness, at: Vec2) -> bool {
    let st = h.app.session.world_state();
    let Some(ss) = h.app.session.active_sketch.and_then(|sid| st.sketch(sid)) else { return false };
    ss.sketch.points.iter().any(|p| p.pos.dist(at) < 1e-6)
}

#[test]
fn corner_window_takes_the_corner_then_applies_the_typed_size() {
    for command in ["sketch.fillet", "sketch.chamfer.equal_distance", "sketch.chamfer.distance_angle", "sketch.chamfer.two_distance"] {
        let mut h = crate::scenario::Harness::new();
        h.app.set_origin_shown(true);
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,30]})).unwrap();
        h.frames(2);
        let sid = h.app.session.active_sketch.unwrap();
        let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        let cursor = proj.to_screen(Vec3::ZERO).unwrap();
        // The ordinary snap still finds the fixed origin; the window must instead reach the
        // rectangle's overlapping endpoint.
        assert_eq!(sketch_point_at(&h.app, &proj, cursor).unwrap().1.as_deref(), Some("origin"));
        let before = (h.app.session.doc.to_json(), h.app.session.undo.len());
        h.step(&json!({"start":command})).unwrap();
        h.frames(2);
        assert!(h.app.tool.is_none(), "{command} must open a window, not a click tool");
        assert!(h.app.dialog.is_some(), "{command} must open a window");
        h.step(&json!({"move":{"world":[0,0,0]}})).unwrap();
        let hover = h.app.viewport.hover.clone();
        let Some(crate::viewport::Hit::SketchPoint { id, .. }) = hover else {
            panic!("{command}: no corner under the cursor: {hover:?}");
        };
        assert_ne!(id, "origin", "the actual shared endpoint must be highlighted");
        h.step(&json!({"click":{"world":[0,0,0]}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.dialog.as_ref().map(|d| d.inputs[0].items.len()), Some(1), "{command}: the corner belongs in the input");
        assert_eq!((h.app.session.doc.to_json(), h.app.session.undo.len()), before, "{command} must change nothing before OK");
        // 5 mm: the size the click flow used to guess and commit before asking.
        set_size(&mut h.app, "5");
        apply(&mut h);
        assert!(h.app.dialog.is_none(), "{command}: {:?}", h.app.status);
        assert_eq!(h.app.session.undo.len(), before.1 + 1, "{command} must be one undo entry");
        let st = h.app.session.world_state();
        let ss = st.sketch(sid).unwrap();
        assert_eq!(ss.sketch.point(0), Some(Vec2::ZERO));
        assert!(ss.sketch.points[0].fixed);
        assert_eq!(ss.sketch.curves.len(), 5, "{command} must actually modify the geometry; status: {:?}", h.app.status);
        if command == "sketch.fillet" {
            assert!(ss.sketch.curves.iter().any(|c| matches!(c.kind, CurveKind::Arc { .. })));
        } else {
            assert!(ss.sketch.curves.iter().all(|c| matches!(c.kind, CurveKind::Line { .. })));
        }
        let expected = if command == "sketch.fillet" { 600.0 - 25.0 * (1.0 - std::f64::consts::PI / 4.0) } else { 587.5 };
        assert_eq!(ss.profiles.len(), 1);
        assert!((ss.profiles[0].area - expected).abs() < 1e-6, "{command}: {}", ss.profiles[0].area);
        drop(st);
        h.app.run("edit.undo", json!({})).unwrap();
        let st = h.app.session.world_state();
        let ss = st.sketch(sid).unwrap();
        assert_eq!(ss.sketch.curves.len(), 4);
        assert!((ss.profiles[0].area - 600.0).abs() < 1e-6);
    }
}

/// #2420: the shared corner window shows the design unit, including both chamfer legs.
#[test]
fn corner_defaults_show_design_units() {
    for units in ["mm", "cm", "m", "in", "ft"] {
        for command in ["sketch.fillet", "sketch.chamfer.equal_distance", "sketch.chamfer.two_distance", "sketch.chamfer.distance_angle"] {
            let mut h = crate::scenario::Harness::new();
            h.app.run("document.units", json!({"units": units})).unwrap();
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            h.step(&json!({"start":command})).unwrap();
            let Some(crate::dialogs::Kind::SketchModify(crate::sketch_modify_ui::Mod::Corner { size, distance2, angle, .. })) =
                h.app.dialog.as_ref().map(|d| &d.kind)
            else {
                panic!("{command}: no corner window");
            };
            assert_eq!(size, &format!("2 {units}"), "{command}");
            assert_eq!(distance2, &format!("2 {units}"), "{command}");
            assert_eq!(angle, "45 deg", "length units must not change angles");
        }
    }
}

/// A typed inch radius must reach both corner selection routes as a persistent dimension.
#[test]
fn fillet_window_reads_design_units_and_preserves_explicit_units_and_parameters() {
    for (units, text, radius) in
        [("in", "0.25", 6.35), ("cm", "0.25", 2.5), ("mm", "0.25", 0.25), ("in", "2 mm", 2.0), ("mm", "0.25 in", 6.35), ("in", "rounding", 3.0)]
    {
        for lines in [false, true] {
            let mut h = crate::scenario::Harness::new();
            h.app.preferences.length_decimals = 4;
            h.app.run("document.units", json!({"units":units})).unwrap();
            h.app.run("parameters.add", json!({"name":"rounding", "expression":"3 mm"})).unwrap();
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
            // Keep the outer size fixed when the driving radius changes.
            h.app.run("sketch.dimension", json!({"entities":["l1"],"value":"40 mm"})).unwrap();
            h.app.run("sketch.dimension", json!({"entities":["l2"],"value":"30 mm"})).unwrap();
            h.frames(2);
            h.step(&json!({"start":"sketch.fillet"})).unwrap();
            if lines {
                for point in [[25, 0, 0], [40, 15, 0]] {
                    h.step(&json!({"click":{"world":point}})).unwrap();
                }
            } else {
                h.step(&json!({"click":{"world":[40,0,0]}})).unwrap();
            }
            retype(&mut h, "dialog:Radius", text);
            apply(&mut h);
            assert!(h.app.dialog.is_none(), "{units}, {text}, lines {lines}: {:?}", h.app.status);
            let check = |session: &solidcraft_engine::Session, expected: f64| {
                let state = session.world_state();
                let ss = state.sketch(1).unwrap();
                let arc = ss
                    .sketch
                    .curves
                    .iter()
                    .find_map(|c| match c.kind {
                        CurveKind::Arc { c: center, a: start, .. } => Some(ss.sketch.point(center).unwrap().dist(ss.sketch.point(start).unwrap())),
                        _ => None,
                    })
                    .unwrap();
                assert!((arc - expected).abs() < 1e-6, "{units}, {text}, lines {lines}: radius {arc}, expected {expected}");
                let area = 1200.0 - expected * expected * (1.0 - std::f64::consts::PI / 4.0);
                assert!(
                    (ss.profiles[0].area - area).abs() < 1e-6,
                    "{units}, {text}, lines {lines}: area {}, expected {area}, radius {expected}",
                    ss.profiles[0].area
                );
            };
            check(&h.app.session, radius);
            if text == "rounding" {
                h.app.run("parameters.change", json!({"name":"rounding", "expression":"4 mm"})).unwrap();
                check(&h.app.session, 4.0);
            }
            let reopened = solidcraft_engine::Session::new(solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap());
            check(&reopened, if text == "rounding" { 4.0 } else { radius });
        }
    }
}

/// The window's reason to exist: a chamfer angle other than the 45 degrees the command used to
/// receive hard-coded. 10 mm along the first line at 30 degrees puts the second cut at
/// 10 * sin 30 / sin 60 mm, where 45 degrees would put it at 10 mm.
#[test]
fn typed_chamfer_angle_other_than_45_degrees_reaches_the_command() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
    h.frames(2);
    h.step(&json!({"start":"sketch.chamfer.distance_angle"})).unwrap();
    h.frames(2);
    h.step(&json!({"click":{"world":[40,0,0]}})).unwrap();
    h.frames(2);
    retype(&mut h, "dialog:Distance 1", "10");
    retype(&mut h, "dialog:Angle", "30");
    apply(&mut h);
    assert!(h.app.dialog.is_none(), "{:?}", h.app.status);
    let second = 10.0 / 3f64.sqrt();
    assert!(has_point(&h, Vec2::new(30.0, 0.0)), "10 mm along the first line");
    assert!(has_point(&h, Vec2::new(40.0, second)), "{second} mm along the second line, not 10 mm");
    assert!(!has_point(&h, Vec2::new(40.0, 10.0)), "45 degrees must no longer be forced");
    let st = h.app.session.world_state();
    let ss = st.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    assert!((ss.profiles[0].area - (1200.0 - 0.5 * 10.0 * second)).abs() < 1e-6, "{}", ss.profiles[0].area);
}

/// One window serves all four commands: the Chamfer Type row of icon buttons switches which
/// chamfer command OK runs, with no drop-down to open.
#[test]
fn the_chamfer_type_icons_switch_which_command_the_window_runs() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
    h.frames(2);
    h.step(&json!({"start":"sketch.chamfer.equal_distance"})).unwrap();
    h.frames(2);
    h.step(&json!({"click":{"world":[40,0,0]}})).unwrap();
    h.frames(2);
    // Every mode is a button on screen, not an item behind a drop-down.
    for label in ["Equal Distance", "Two Distances", "Distance and Angle"] {
        assert!(
            crate::scenario::handle_at(&format!("dialog:Chamfer Type:{label}")).is_some(),
            "{label} must be its own button on the Chamfer Type row"
        );
    }
    h.step(&json!({"click":{"handle":"dialog:Chamfer Type:Distance and Angle"}})).unwrap();
    h.frames(2);
    retype(&mut h, "dialog:Distance 1", "10");
    retype(&mut h, "dialog:Angle", "30");
    apply(&mut h);
    assert!(h.app.dialog.is_none(), "{:?}", h.app.status);
    // The Distance and Angle command ran, not the Equal Distance one the window opened on.
    let second = 10.0 / 3f64.sqrt();
    assert!(has_point(&h, Vec2::new(30.0, 0.0)));
    assert!(has_point(&h, Vec2::new(40.0, second)));
    assert!(!has_point(&h, Vec2::new(40.0, 10.0)), "Equal Distance would have cut 10 mm off both legs");
}

/// Two Distances forced its second distance equal to the first. Both are typed now, and Flip
/// names the two lines in the other order so the first distance lands on the other leg.
#[test]
fn two_distance_chamfer_takes_both_distances_and_flip_swaps_the_legs() {
    for (flip, first, second) in [(false, Vec2::new(28.0, 0.0), Vec2::new(40.0, 4.0)), (true, Vec2::new(40.0, 12.0), Vec2::new(36.0, 0.0))] {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
        h.frames(2);
        h.step(&json!({"start":"sketch.chamfer.two_distance"})).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"world":[40,0,0]}})).unwrap();
        h.frames(2);
        retype(&mut h, "dialog:Distance 1", "12");
        retype(&mut h, "dialog:Distance 2", "4");
        if flip {
            // Each corner row has its own Flip, so turning one leg round leaves the others as they are.
            h.step(&json!({"click":{"handle":"dialog:Corner 0:Flip"}})).unwrap();
            h.frames(2);
        }
        apply(&mut h);
        assert!(h.app.dialog.is_none(), "flip {flip}: {:?}", h.app.status);
        assert!(has_point(&h, first), "flip {flip}: the first distance belongs at {first:?}");
        assert!(has_point(&h, second), "flip {flip}: the second distance belongs at {second:?}");
    }
}

#[test]
fn escape_leaves_the_sketch_and_the_history_untouched() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,30]})).unwrap();
    h.frames(2);
    let before = (h.app.session.doc.to_json(), h.app.session.undo.len());
    h.step(&json!({"start":"sketch.fillet"})).unwrap();
    h.frames(2);
    h.step(&json!({"click":{"world":[0,0,0]}})).unwrap();
    h.frames(2);
    set_size(&mut h.app, "5");
    h.frames(2);
    h.step(&json!({"key":"Escape"})).unwrap();
    h.frames(2);
    assert!(h.app.dialog.is_none());
    assert_eq!((h.app.session.doc.to_json(), h.app.session.undo.len()), before);
}

/// A size the corner cannot take is refused with a reason, and the window stays open on it.
#[test]
fn an_impossible_size_keeps_the_window_open_with_its_reason() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,30]})).unwrap();
    h.frames(2);
    let before = (h.app.session.doc.to_json(), h.app.session.undo.len());
    h.step(&json!({"start":"sketch.chamfer.equal_distance"})).unwrap();
    h.frames(2);
    h.step(&json!({"click":{"world":[0,0,0]}})).unwrap();
    h.frames(2);
    for size in ["500", "0", "-3", "not a number"] {
        set_size(&mut h.app, size);
        apply(&mut h);
        assert!(h.app.dialog.as_ref().is_some_and(|d| d.error.is_some()), "{size} must be refused with a reason");
        assert_eq!((h.app.session.doc.to_json(), h.app.session.undo.len()), before, "{size} must change nothing");
    }
    set_size(&mut h.app, "4");
    apply(&mut h);
    assert!(h.app.dialog.is_none(), "{:?}", h.app.status);
    assert_eq!(h.app.session.undo.len(), before.1 + 1);
}

/// A fillet rounds every picked corner to one radius, as one undo entry.
#[test]
fn a_fillet_rounds_every_picked_corner_to_one_radius() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
    h.frames(2);
    let undo = h.app.session.undo.len();
    h.step(&json!({"start":"sketch.fillet"})).unwrap();
    h.frames(2);
    for corner in [[40, 0, 0], [40, 30, 0], [0, 30, 0]] {
        h.step(&json!({"click":{"world":corner}})).unwrap();
        h.frames(2);
    }
    assert_eq!(h.app.dialog.as_ref().map(|d| d.inputs[0].items.len()), Some(3));
    set_size(&mut h.app, "5");
    apply(&mut h);
    assert!(h.app.dialog.is_none(), "{:?}", h.app.status);
    assert_eq!(h.app.session.undo.len(), undo + 1, "one radius, one undo entry");
    let st = h.app.session.world_state();
    let ss = st.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    assert_eq!(ss.sketch.curves.iter().filter(|c| matches!(c.kind, CurveKind::Arc { .. })).count(), 3);
    assert!((ss.profiles[0].area - (1200.0 - 3.0 * 25.0 * (1.0 - std::f64::consts::PI / 4.0))).abs() < 1e-6, "{}", ss.profiles[0].area);
}

#[test]
fn fully_constrained_center_rectangle_corners_can_be_picked_and_filleted() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.center", json!({"center":[0,0],"corner":[20,15]})).unwrap();
    h.app.run("sketch.constraint.coincident", json!({"a":"p5","b":"origin"})).unwrap();
    h.app.run("sketch.dimension", json!({"entities":["l1"],"value":40})).unwrap();
    h.app.run("sketch.dimension", json!({"entities":["l2"],"value":30})).unwrap();
    h.frames(2);
    h.step(&json!({"start":"sketch.fillet"})).unwrap();
    h.frames(2);
    // Fit after opening the dialog so all four corners clear the shell and both panels.
    h.app.fit_view();
    h.settle();
    for corner in [[-20, -15, 0], [20, -15, 0], [20, 15, 0], [-20, 15, 0]] {
        h.step(&json!({"click":{"world":corner}})).unwrap();
        h.frames(2);
    }
    assert_eq!(h.app.dialog.as_ref().map(|d| d.inputs[0].items.len()), Some(4));
    set_size(&mut h.app, "3");
    apply(&mut h);
    assert!(h.app.dialog.is_none(), "{:?}", h.app.status);
    let info = h.app.run("sketch.inspect", json!({})).unwrap();
    assert_eq!(info["dof"], 0, "{info}");
    assert_eq!(info["status"], "solved", "{info}");
    let st = h.app.session.world_state();
    let ss = st.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    assert_eq!(ss.sketch.curves.iter().filter(|c| matches!(c.kind, CurveKind::Arc { .. })).count(), 4);
    assert!((ss.profiles[0].area - (1200.0 - 36.0 * (1.0 - std::f64::consts::PI / 4.0))).abs() < 1e-6);
}

#[test]
fn ordinary_drawing_keeps_origin_snap_and_real_origin_corners_work() {
    let mut h = crate::scenario::Harness::new();
    h.app.set_origin_shown(true);
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.step(&json!({"start":"sketch.line"})).unwrap();
    h.step(&json!({"move":{"world":[0,0,0]}})).unwrap();
    assert_eq!(h.app.tool.as_ref().unwrap().hover.as_ref().unwrap().1.as_deref(), Some("origin"));
    h.step(&json!({"click":{"world":[0,0,0]}})).unwrap();
    assert_eq!(h.app.tool.as_ref().unwrap().pts[0].1.as_deref(), Some("origin"));
    h.app.tool = None;
    h.app.run("sketch.line", json!({"points":["origin",[20,0],[20,20],[0,20],"origin"]})).unwrap();
    h.step(&json!({"start":"sketch.fillet"})).unwrap();
    h.frames(2);
    h.step(&json!({"click":{"world":[0,0,0]}})).unwrap();
    h.frames(2);
    set_size(&mut h.app, "5");
    apply(&mut h);
    assert!(h.app.dialog.is_none(), "{:?}", h.app.status);
    let st = h.app.session.world_state();
    let ss = st.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    assert_eq!(ss.sketch.point(0), Some(Vec2::ZERO));
    assert!(ss.sketch.curves.iter().any(|c| matches!(c.kind, CurveKind::Arc { .. })));
    assert_eq!(ss.profiles.len(), 1);
}

/// #1824: where a third line ends at a corner's point, the point names no single corner; the
/// Fillet window takes the corner's two lines instead and rounds the corner between them.
#[test]
fn fillet_window_rounds_the_corner_between_two_picked_lines() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[10,0]],"ids":["left"]})).unwrap();
    h.app.run("sketch.line", json!({"points":["left.end",[20,0]],"ids":["right"]})).unwrap();
    h.app.run("sketch.line", json!({"points":["left.end",[18,8]],"ids":["diag"]})).unwrap();
    h.settle();
    let sid = h.app.session.active_sketch.unwrap();
    h.step(&json!({"start":"sketch.fillet"})).unwrap();
    // The shared point ends three lines, so it names no single corner: pick the two lines.
    h.step(&json!({"click":{"world":[16,0,0]}})).unwrap();
    h.step(&json!({"click":{"world":[13,3,0]}})).unwrap();
    assert_eq!(dialog_curves(&h), vec!["right".to_string(), "diag".to_string()], "{:?}", h.app.status);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.status);
    let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
    assert_eq!(sk.curves.iter().filter(|c| matches!(c.kind, CurveKind::Arc { .. })).count(), 1, "{:?}", h.app.status);
}

/// #1824 (review): the Chamfer window takes a corner's two lines the same way.
#[test]
fn chamfer_window_cuts_the_corner_between_two_picked_lines() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[10,0]],"ids":["left"]})).unwrap();
    h.app.run("sketch.line", json!({"points":["left.end",[20,0]],"ids":["right"]})).unwrap();
    h.app.run("sketch.line", json!({"points":["left.end",[18,8]],"ids":["diag"]})).unwrap();
    h.settle();
    let sid = h.app.session.active_sketch.unwrap();
    let before = h.app.session.model.state().sketch(sid).unwrap().sketch.curves.len();
    h.step(&json!({"start":"sketch.chamfer.equal_distance"})).unwrap();
    h.step(&json!({"click":{"world":[16,0,0]}})).unwrap();
    h.step(&json!({"click":{"world":[13,3,0]}})).unwrap();
    assert_eq!(dialog_curves(&h), vec!["right".to_string(), "diag".to_string()], "{:?}", h.app.status);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.status);
    let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
    assert_eq!(sk.curves.len(), before + 1, "a chamfer line: {:?}", h.app.status);
    assert!(sk.curve_index("left").is_some(), "the third line stays");
}

/// Corner previews draw the new corner curve and the two shortened sides when a corner is picked.
/// This tests that `previews()` for Corner produces sketch preview curves visible in the viewport.
#[test]
fn fillet_chamfer_corner_preview_draws_curves() {
    let mut h = crate::scenario::Harness::new();
    // Create a simple L-shape with two lines meeting at (0, 10).
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points": [[-30,10],[0,10]], "ids": ["top"]})).unwrap();
    h.app.run("sketch.line", json!({"points": ["top.end",[0,-20]], "ids": ["right"]})).unwrap();
    h.frames(2);

    // Open the fillet tool and pick the corner at (0, 10).
    h.step(&json!({"start":"sketch.fillet"})).unwrap();
    h.frames(2);
    h.step(&json!({"click":{"world":[0.0,10.0,0.0]}})).unwrap();
    h.frames(3);

    // Show the fillet arc and both shortened sides, whose ids stay the same.
    h.step(&json!({"until":{"count":{"name":"sketch_preview_curves","value":3}}})).unwrap();

    // Verify the sketch itself hasn't been modified yet.
    let sid = h.app.session.active_sketch.unwrap();
    let sk = h.app.session.world_state().sketch(sid).unwrap().sketch.clone();
    assert_eq!(sk.curves.len(), 2, "fillet preview must not modify the sketch");

    // Now test chamfer too. Close the fillet dialog first.
    h.step(&json!({"key":"Escape"})).unwrap();
    h.frames(3);
    h.step(&json!({"start":"sketch.chamfer.equal_distance"})).unwrap();
    h.frames(2);
    h.step(&json!({"click":{"world":[0.0,10.0,0.0]}})).unwrap();
    h.frames(3);

    // Show the chamfer line and both shortened sides too.
    h.step(&json!({"until":{"count":{"name":"sketch_preview_curves","value":3}}})).unwrap();

    let sk = h.app.session.world_state().sketch(sid).unwrap().sketch.clone();
    assert_eq!(sk.curves.len(), 2, "chamfer preview must not modify the sketch");
}

/// #3052: with Trim, the hover over a line crossed by two others is the piece between them, the
/// one a click takes away; the whole line is no longer shown hovered. With another tool, nothing.
#[test]
fn trim_hover_shows_the_piece_a_click_takes_away() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    let line = h.app.run("sketch.line", json!({"points": [[0,0],[30,0]], "connect": false})).unwrap()["curves"][0].as_str().unwrap().to_string();
    h.app.run("sketch.line", json!({"points": [[10,-5],[10,5]], "connect": false})).unwrap();
    h.app.run("sketch.line", json!({"points": [[20,-5],[20,5]], "connect": false})).unwrap();
    h.settle();
    let sid = h.app.session.active_sketch.unwrap();
    h.step(&json!({"start":"sketch.trim"})).unwrap();
    h.step(&json!({"move":{"world":[15,0,0]}})).unwrap();
    assert!(matches!(&h.app.viewport.hover, Some(crate::viewport::Hit::SketchCurve { id, .. }) if *id == line), "{:?}", h.app.viewport.hover);
    let piece = trim_hover_piece(&h.app, sid, &line).unwrap();
    let ends = [piece[0], piece[piece.len() - 1]];
    assert!(
        ends.iter().all(|p| p.y.abs() < 1e-6) && ends.iter().any(|p| (p.x - 10.0).abs() < 1e-6) && ends.iter().any(|p| (p.x - 20.0).abs() < 1e-6),
        "{ends:?}"
    );
    h.step(&json!({"key":"Escape"})).unwrap();
    h.step(&json!({"move":{"world":[15,0,0]}})).unwrap();
    assert!(trim_hover_piece(&h.app, sid, &line).is_none(), "only Trim shows a piece");
}

/// #3141: the 2-tangent circle tool, clicked on two sides of a rectangle drawn away from the
/// origin, offers its default radius before it puts the circle in the corner between the clicks.
/// A typed radius reaches the command, neither path moves the rectangle, and fixed sides work.
#[test]
fn two_tangent_circle_takes_its_default_or_a_typed_radius_and_moves_nothing() {
    for fixed in [false, true] {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        let r = h.app.run("sketch.rectangle.two_point", json!({"p0":[-800,0],"p1":[-300,500]})).unwrap();
        if fixed {
            h.app.run("sketch.constraint.fix", json!({"entities": r["curves"]})).unwrap();
        }
        h.step(&json!({"call":"ui.view","params":{"view":"fit"}})).unwrap();
        h.settle();
        let sid = h.app.session.active_sketch.unwrap();
        let points =
            |h: &crate::scenario::Harness| h.app.session.model.state().sketch(sid).unwrap().sketch.points.iter().map(|p| p.pos).collect::<Vec<_>>();
        let before = points(&h);
        h.step(&json!({"start":"sketch.circle.two_tangent"})).unwrap();
        h.step(&json!({"click":{"world":[-550,0,0]}})).unwrap();
        h.step(&json!({"click":{"world":[-800,250,0]}})).unwrap();
        let before_circle = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
        assert!(!before_circle.curves.iter().any(|c| matches!(c.kind, CurveKind::Circle { .. })), "the second pick waits for Radius");
        let tool = h.app.tool.as_ref().unwrap();
        assert_eq!(tool.dims.iter().map(|dimension| dimension.label).collect::<Vec<_>>(), ["Radius"]);
        let offered = tool.dims[0].value.parse::<f64>().unwrap_or_default();
        assert!((offered - 50.0).abs() < 1e-6, "the shorter 500 mm side offers 50 mm: {offered}");
        let radius = if fixed {
            h.step(&json!({"key":"A","cmd":true})).unwrap();
            h.step(&json!({"text":"30"})).unwrap();
            30.0
        } else {
            50.0
        };
        h.step(&json!({"key":"Enter"})).unwrap();
        let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
        let circle = sk.curves.iter().position(|c| matches!(c.kind, CurveKind::Circle { .. }));
        let circle = circle.unwrap_or_else(|| panic!("fixed {fixed}: no circle: {:?}", h.app.status));
        assert!(sk.center(circle).unwrap().dist(Vec2::new(-800.0 + radius, radius)) < 1e-6, "fixed {fixed}: {:?}", sk.center(circle));
        assert!((sk.radius(circle).unwrap() - radius).abs() < 1e-6, "fixed {fixed}: {:?}", sk.radius(circle));
        let after = points(&h);
        assert!(before.iter().zip(&after).all(|(a, b)| a.dist(*b) < 1e-9), "fixed {fixed}: the rectangle moved: {after:?}");
    }
}

/// #3099: the centre circle tool, its centre snapped to a line's midpoint and its second click
/// to the line's end, keeps the circle through that end: nothing is left free.
#[test]
fn centre_circle_clicked_through_a_line_end_stays_on_it() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,30],[40,30]]})).unwrap();
    h.step(&json!({"call":"ui.view","params":{"view":"fit"}})).unwrap();
    h.settle();
    let sid = h.app.session.active_sketch.unwrap();
    h.step(&json!({"start":"sketch.circle.center"})).unwrap();
    for at in [[20, 30, 0], [40, 30, 0]] {
        // The line's end lands in the view's bottom-right corner, where notices stack and take clicks
        // (#7893): this test is about the circle, so the transient status notice goes before each click.
        h.app.status = None;
        h.frame();
        h.step(&json!({"move":{"world":at}})).unwrap();
        h.step(&json!({"click":{"world":at}})).unwrap();
    }
    let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
    let circle = sk.curves.iter().position(|c| matches!(c.kind, CurveKind::Circle { .. })).unwrap();
    let end = sk.resolve_point("l1.end").unwrap();
    assert!(
        sk.constraints.iter().any(|k| matches!(k.kind, solidcraft_engine::sketch::ConstraintKind::Midpoint { l: 0, .. })),
        "{:?}",
        sk.constraints
    );
    assert!(
        sk.constraints.iter().any(|k| k.kind == solidcraft_engine::sketch::ConstraintKind::PointOnCurve { p: end, c: circle }),
        "the circle stays on the line's end: {:?}",
        sk.constraints
    );
}

/// #7299: the centre arc tool, its centre snapped to a rectangle's corner, ties the arc to the
/// corner with a Coincident (which the sketch shows as a glyph), not by quietly sharing the corner
/// as its centre.
#[test]
fn centre_arc_on_an_existing_point_is_tied_by_a_coincident() {
    use solidcraft_engine::sketch::ConstraintKind;
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[10,10],"p1":[40,30]})).unwrap();
    h.step(&json!({"call":"ui.view","params":{"view":"fit"}})).unwrap();
    h.settle();
    let sid = h.app.session.active_sketch.unwrap();
    let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
    let at_corner = |p: Option<_>| p.is_some_and(|p: Vec2| (p.x - 40.0).abs() < 1e-9 && (p.y - 30.0).abs() < 1e-9);
    let corner = (0..sk.points.len()).find(|&i| at_corner(sk.point(i))).unwrap();
    h.step(&json!({"start":"sketch.arc.center_point"})).unwrap();
    for at in [[40, 30, 0], [34, 30, 0], [40, 24, 0]] {
        h.step(&json!({"move":{"world":at}})).unwrap();
        h.step(&json!({"click":{"world":at}})).unwrap();
    }
    let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
    let centre = sk.curves.iter().find_map(|c| match c.kind {
        CurveKind::Arc { c, .. } => Some(c),
        _ => None,
    });
    let centre = centre.unwrap_or_else(|| panic!("an arc is drawn: {:?}", h.app.status));
    assert_ne!(centre, corner, "the arc has a centre of its own");
    assert!(at_corner(sk.point(centre)), "the centre is on the corner");
    let tied = sk
        .constraints
        .iter()
        .any(|k| matches!(k.kind, ConstraintKind::Coincident { p, q } if (p, q) == (corner, centre) || (p, q) == (centre, corner)));
    assert!(tied, "a Coincident ties the centre to the corner: {:?}", sk.constraints);
}

/// #4691: the Point tool clicked on a line's midpoint makes a point held there (a MidPoint
/// constraint), not a free point that happens to sit on the middle.
#[test]
fn a_point_clicked_on_a_line_midpoint_is_held_there() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[20,0]],"ids":["base"]})).unwrap();
    h.settle();
    h.step(&json!({"start":"sketch.point"})).unwrap();
    h.step(&json!({"click":{"world":[10, 0, 0]}})).unwrap();
    let sid = h.app.session.active_sketch.unwrap();
    let state = h.app.session.model.state();
    let sk = &state.sketch(sid).unwrap().sketch;
    let held = sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::Midpoint { .. }));
    assert!(held, "no MidPoint constraint: {:?}", sk.constraints.iter().map(|c| &c.kind).collect::<Vec<_>>());
}

/// #7299: the centre circle tool, its centre snapped to a rectangle's corner, ties the circle
/// to the corner with a Coincident (which the sketch shows as a glyph that can be picked and
/// deleted), not by quietly sharing the corner as its centre.
#[test]
fn centre_circle_on_an_existing_point_is_tied_by_a_coincident() {
    use solidcraft_engine::sketch::ConstraintKind;
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[10,10],"p1":[40,30]})).unwrap();
    h.step(&json!({"call":"ui.view","params":{"view":"fit"}})).unwrap();
    h.settle();
    let sid = h.app.session.active_sketch.unwrap();
    let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
    let at_corner = |p: Option<_>| p.is_some_and(|p: Vec2| (p.x - 40.0).abs() < 1e-9 && (p.y - 30.0).abs() < 1e-9);
    let corner = (0..sk.points.len()).find(|&i| at_corner(sk.point(i))).unwrap();
    h.step(&json!({"start":"sketch.circle.center"})).unwrap();
    for at in [[40, 30, 0], [34, 24, 0]] {
        h.step(&json!({"move":{"world":at}})).unwrap();
        h.step(&json!({"click":{"world":at}})).unwrap();
    }
    let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
    let centre = sk.curves.iter().find_map(|c| match c.kind {
        CurveKind::Circle { c, .. } => Some(c),
        _ => None,
    });
    let centre = centre.unwrap_or_else(|| panic!("a circle is drawn: {:?}", h.app.status));
    assert_ne!(centre, corner, "the circle has a centre of its own");
    assert!(at_corner(sk.point(centre)), "the centre is on the corner");
    let tied = sk
        .constraints
        .iter()
        .any(|k| matches!(k.kind, ConstraintKind::Coincident { p, q } if (p, q) == (corner, centre) || (p, q) == (centre, corner)));
    assert!(tied, "a Coincident ties the centre to the corner: {:?}", sk.constraints);
}

#[test]
fn corner_candidates_agree_with_engine_spatial_curve_filter() {
    use crate::sketch_modify_ui::candidate;
    use crate::viewport::Hit;

    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.sphere", json!({"radius":20})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XY", "offset":50})).unwrap();
    h.app.run("sketch.line", json!({"points":[[-10,0],[10,0]],"ids":["source"]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let src = h.app.session.doc.features.last().unwrap().id;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    let projected = h.app.run("sketch.project_to_surface", json!({"curves":[{"sketch":src,"curve":"source"}],"face":[0,0,20]})).unwrap();
    let linked = projected["links"][0]["wires"][0].as_str().unwrap().to_owned();
    h.app.run("sketch.options", json!({"sketch_3d":true})).unwrap();
    let line = h.app.run("sketch.line3d", json!({"points":[[0,0,0],[20,0,20]]})).unwrap()["curves"][0].as_str().unwrap().to_owned();
    let spline = h.app.run("sketch.spline3d", json!({"points":[[0,0,0],[10,5,10],[20,0,20]]})).unwrap()["curves"][0].as_str().unwrap().to_owned();
    h.app.run("sketch.line", json!({"points":[[0,0],[0,20]],"ids":["side"]})).unwrap();
    let sketch = h.app.session.active_sketch.unwrap();
    for cmd in ["sketch.fillet", "sketch.chamfer.equal_distance", "sketch.chamfer.two_distance", "sketch.chamfer.distance_angle"] {
        h.app.start(cmd);
        for id in [&line, &spline, &linked, &"side".to_owned()] {
            let state = h.app.session.world_state();
            let sk = &state.sketch(sketch).unwrap().sketch;
            let hit = Hit::SketchCurve { sketch, id: id.clone(), straight: true, point: solidcraft_engine::geom::Vec3::ZERO };
            let offered = candidate(h.app.dialog.as_ref().unwrap(), &h.app.session, &hit).is_some();
            assert_eq!(offered, !sk.is_spatial_corner_curve(id), "{cmd}: {id}");
            assert_eq!(offered, id == "side", "{cmd}: {id}");
        }
        h.app.dialog = None;
    }
}

/// The picks the corner window's input holds for the ids given.
fn line_picks(h: &crate::scenario::Harness, ids: &[&str]) -> Vec<solidcraft_engine::Sel> {
    let sid = h.app.session.active_sketch.unwrap();
    let state = h.app.session.model.state();
    let sk = &state.sketch(sid).unwrap().sketch;
    ids.iter()
        .map(|id| {
            if sk.curve_index(id).is_some() {
                solidcraft_engine::Sel::SketchCurve { id: (*id).to_string(), sketch: None }
            } else {
                solidcraft_engine::Sel::SketchPoint { id: (*id).to_string() }
            }
        })
        .collect()
}

/// The corner rows a set of picks makes, as their labels and corner points.
fn corner_rows(h: &crate::scenario::Harness, ids: &[&str]) -> Vec<(String, Option<String>)> {
    let sid = h.app.session.active_sketch.unwrap();
    let state = h.app.session.model.state();
    let sk = &state.sketch(sid).unwrap().sketch;
    crate::sketch_corner_rows::rows(sk, &line_picks(h, ids), &serde_json::Map::new()).into_iter().map(|row| (row.label(), row.point)).collect()
}

/// A square of named sides a, b, c, d, each meeting its neighbour at a corner.
fn square() -> crate::scenario::Harness {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({ "plane": "XY" })).unwrap();
    h.app.run("sketch.line", json!({ "points": [[0, 0], [10, 0]], "ids": ["a"] })).unwrap();
    h.app.run("sketch.line", json!({ "points": ["a.end", [10, 10]], "ids": ["b"] })).unwrap();
    h.app.run("sketch.line", json!({ "points": ["b.end", [0, 10]], "ids": ["c"] })).unwrap();
    h.app.run("sketch.line", json!({ "points": ["c.end", "a.start"], "ids": ["d"] })).unwrap();
    h.settle();
    h
}

fn labels(h: &crate::scenario::Harness, ids: &[&str]) -> Vec<String> {
    corner_rows(h, ids).into_iter().map(|(label, _)| label).collect()
}

/// A picked line looks for an earlier line with a free end at one of its own. The shared end
/// becomes that corner, the row stays if its line is unpicked, and a line at 180 degrees to
/// another keeps a row of its own.
#[test]
fn picked_lines_pair_into_corners_and_unpicking_one_leaves_its_neighbours() {
    let h = square();
    assert_eq!(labels(&h, &["a"]), ["a"]);
    assert_eq!(labels(&h, &["a", "b"]), ["a-b"]);
    assert_eq!(labels(&h, &["a", "b", "c"]), ["a-b", "b-c"]);
    // b is gone from both rows; a and c are each left on their own.
    assert_eq!(labels(&h, &["a", "c"]), ["a", "c"]);
    // d then makes a corner with each of them, at its two ends.
    let rows = labels(&h, &["a", "c", "d"]);
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(rows.contains(&"a-d".to_string()) && rows.contains(&"c-d".to_string()), "{rows:?}");
    // All four sides make four corners.
    assert_eq!(labels(&h, &["a", "b", "c", "d"]).len(), 4);
}

/// Two lines in line with each other make no corner: each keeps a row, so a 180 degree pair
/// shows two rows rather than one.
#[test]
fn lines_in_line_with_each_other_keep_rows_of_their_own() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({ "plane": "XY" })).unwrap();
    h.app.run("sketch.line", json!({ "points": [[0, 0], [10, 0]], "ids": ["left"] })).unwrap();
    h.app.run("sketch.line", json!({ "points": ["left.end", [20, 0]], "ids": ["right"] })).unwrap();
    h.settle();
    assert_eq!(labels(&h, &["left", "right"]), ["left", "right"]);
}

/// Four lines meeting at one point pair in the order they were picked, each end used up once:
/// A-B then C, C takes B's free end when A goes, and B joins the first line with a free corner.
#[test]
fn lines_meeting_at_one_point_pair_in_pick_order() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({ "plane": "XY" })).unwrap();
    h.app.run("sketch.line", json!({ "points": [[-10, 0], "origin"], "ids": ["a"] })).unwrap();
    h.app.run("sketch.line", json!({ "points": ["origin", [0, 10]], "ids": ["b"] })).unwrap();
    h.app.run("sketch.line", json!({ "points": ["origin", [10, 0]], "ids": ["c"] })).unwrap();
    h.app.run("sketch.line", json!({ "points": [[0, -10], "origin"], "ids": ["d"] })).unwrap();
    h.settle();
    assert_eq!(labels(&h, &["a", "b", "c"]), ["a-b", "c"]);
    assert_eq!(labels(&h, &["b", "c"]), ["b-c"]);
    // a and c are 180 degrees apart, so neither takes the other.
    assert_eq!(labels(&h, &["a", "c"]), ["a", "c"]);
    // b takes the first line with a free corner, a, and uses that end up.
    assert_eq!(labels(&h, &["a", "c", "b"]), ["a-b", "c"]);
}

/// A picked point joins the row of the line it ends: the corner is that line's row, and the
/// command gets both of its sides rather than the bare point.
#[test]
fn a_picked_point_joins_the_row_of_the_line_that_ends_there() {
    let h = square();
    let rows = corner_rows(&h, &["a", "a.start"]);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].0, "a");
    assert!(rows[0].1.is_some(), "the row now names the corner point");
    let sid = h.app.session.active_sketch.unwrap();
    let state = h.app.session.model.state();
    let sk = &state.sketch(sid).unwrap().sketch;
    let picks = crate::sketch_corner_rows::rows(sk, &line_picks(&h, &["a", "a.start"]), &serde_json::Map::new());
    let params = crate::sketch_corner_rows::params(sk, &picks[0]);
    assert_eq!(params["a"], json!("a"));
    assert_eq!(params["b"], json!("d"), "the other line ending at a's start: {:?}", picks);
}

/// Two distances with two corners: each row's Flip swaps only its own legs, and a row turned
/// off is not cut while it stays in the list.
#[test]
fn each_corner_row_flips_and_turns_off_on_its_own() {
    let h = square();
    let picks = line_picks(&h, &["a", "b", "c", "d"]);
    let sid = h.app.session.active_sketch.unwrap();
    let inputs = {
        let mut input = crate::selection::SelInput::new("Corner", crate::selection::CURVES, true);
        input.items = picks.clone();
        vec![input]
    };
    let mut extra = serde_json::Map::new();
    {
        let state = h.app.session.model.state();
        let sk = &state.sketch(sid).unwrap().sketch;
        let rows = crate::sketch_corner_rows::rows(sk, &picks, &extra);
        assert_eq!(rows.len(), 4, "{:?}", rows.iter().map(|r| r.label()).collect::<Vec<_>>());
        crate::sketch_corner_rows::set_flip(&mut extra, &rows[1].key, true);
        crate::sketch_corner_rows::set_off(&mut extra, &rows[2].key, true);
        let kept = crate::sketch_corner_rows::rows(sk, &picks, &extra);
        assert_eq!(kept.len(), 4, "a row turned off stays in the list");
        assert!(!kept[2].enabled);
        let k = crate::sketch_modify_ui::Mod::Corner { chamfer: true, ctype: 1, size: "2".into(), distance2: "1".into(), angle: "45 deg".into() };
        // One command cuts them all, so OK is one undo step: each corner keeps its own legs.
        let cmds = crate::sketch_modify_ui::commands(&h.app, &k, &inputs, &extra).unwrap();
        assert_eq!(cmds.len(), 1, "{cmds:?}");
        assert_eq!(cmds[0].0, "sketch.chamfer.two_distance");
        let points = cmds[0].1["points"].as_array().expect("the corners are a list").clone();
        assert_eq!(points.len(), 3, "the corner turned off is not cut: {points:?}");
        assert_eq!(points[0]["a"], json!(rows[0].lines[0]));
        assert_eq!(points[0]["b"], json!(rows[0].lines[1]));
        assert_eq!(points[1]["a"], json!(rows[1].lines[1]), "only the second row is flipped");
        assert_eq!(points[1]["b"], json!(rows[1].lines[0]));
        assert_eq!(points[2]["a"], json!(rows[3].lines[0]), "the fourth row follows the third");
    }
}

/// Two profiles drawn apart have a corner point each where they meet (both held on the origin),
/// as `sketch.rectangle.two_point` draws them. Picking the two lines that meet there makes one
/// row, and the command receives those two lines.
#[test]
fn lines_of_two_profiles_meeting_at_one_place_make_one_corner() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[-26.2,-18.8],"p1":[0,0]})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[24.8,27.8]})).unwrap();
    h.settle();
    assert_eq!(labels(&h, &["l2"]), ["l2"]);
    assert_eq!(labels(&h, &["l2", "l5"]), ["l2-l5"], "the two lines meet at one place");
    let sid = h.app.session.active_sketch.unwrap();
    let state = h.app.session.model.state();
    let sk = &state.sketch(sid).unwrap().sketch;
    let rows = crate::sketch_corner_rows::rows(sk, &line_picks(&h, &["l2", "l5"]), &serde_json::Map::new());
    assert_eq!(crate::sketch_corner_rows::params(sk, &rows[0]), json!({"a": "l2", "b": "l5"}));
}

/// A corner cut between two profiles drawn apart repaints its own two lines and the arc it makes,
/// not the rest of the sketch (the whole sketch used to show in the preview colour).
#[test]
fn a_corner_cut_between_two_profiles_repaints_only_its_own_lines() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[-26.2,-18.8],"p1":[0,0]})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[24.8,27.8]})).unwrap();
    h.settle();
    h.step(&json!({"start":"sketch.fillet"})).unwrap();
    h.frames(2);
    // The two lines that meet at the shared corner, picked away from the corner itself.
    for at in [[0.0, -9.4, 0.0], [12.4, 0.0, 0.0]] {
        h.step(&json!({"click":{"world":at}})).unwrap();
        h.frames(2);
    }
    let rows = crate::sketch_modify_ui::dialog_rows(&h.app.session, h.app.dialog.as_ref().unwrap());
    assert_eq!(rows.len(), 1, "one corner: {:?}", rows.iter().map(|r| r.label()).collect::<Vec<_>>());
    assert_eq!(rows[0].label(), "l2-l5");
    // The two trimmed lines and the arc, not the six lines the cut leaves alone.
    assert_eq!(crate::scenario::count_of("sketch_preview_curves"), Some(3.0), "only the cut curves are drawn as changed");
    // The row's X takes the arc away and leaves the two lines picked: the selection stays.
    h.step(&json!({"click":{"handle":"dialog:list:corner:0:remove"}})).unwrap();
    h.step(&json!({"until":{"count":{"name":"sketch_preview_curves","value":2}}})).unwrap();
    let rows = crate::sketch_modify_ui::dialog_rows(&h.app.session, h.app.dialog.as_ref().unwrap());
    assert_eq!(rows.len(), 1, "the row stays in the list, with the V that brings it back");
    assert!(!rows[0].enabled, "its corner is no longer cut");
    assert_eq!(crate::scenario::count_of("sketch_preview_curves"), Some(2.0), "the arc is gone, its two lines stay picked");
    // The same button shows the cut again.
    h.step(&json!({"click":{"handle":"dialog:list:corner:0:remove"}})).unwrap();
    h.step(&json!({"until":{"count":{"name":"sketch_preview_curves","value":3}}})).unwrap();
}

/// Each row's X sits in one column whatever the row is called and however many rows there are,
/// and clicking it turns that row's cut off (the row stays, with the V that brings it back)
/// without touching the others.
#[test]
fn a_corner_rows_x_turns_its_own_cut_off_and_the_buttons_line_up() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
    h.settle();
    h.step(&json!({"start":"sketch.chamfer.two_distance"})).unwrap();
    h.frames(2);
    // The four corners picked straight into the window: a click on the canvas would land under
    // the window, which sits over the middle of the view.
    let picks = line_picks(&h, &["l1", "l2", "l3", "l4"]);
    if let Some(input) = h.app.dialog.as_mut().and_then(|d| d.inputs.first_mut()) {
        input.items = picks;
    }
    h.frames(2);
    let rows = crate::sketch_modify_ui::dialog_rows(&h.app.session, h.app.dialog.as_ref().unwrap());
    assert_eq!(rows.len(), 4, "{:?}", rows.iter().map(|r| r.label()).collect::<Vec<_>>());
    let at = |name: &str| crate::scenario::handle_at(name).unwrap_or_else(|| panic!("missing {name}"));
    let first = at("dialog:list:corner:0:remove");
    let flip = at("dialog:Corner 0:Flip");
    let card = crate::scenario::rect_of("dialog:card").unwrap();
    assert!(card.expand(1.0).contains(first), "the X at {first:?} must be inside the card {card:?}");
    for index in 1..rows.len() {
        let x = at(&format!("dialog:list:corner:{index}:remove"));
        assert!((x.x - first.x).abs() < 0.5, "row {index}'s X moved to {} from {}", x.x, first.x);
        let f = at(&format!("dialog:Corner {index}:Flip"));
        assert!((f.x - flip.x).abs() < 0.5, "row {index}'s Flip moved to {} from {}", f.x, flip.x);
        assert!(f.x < x.x, "the Flip sits before the X");
    }
    let enabled = |h: &crate::scenario::Harness| {
        crate::sketch_modify_ui::dialog_rows(&h.app.session, h.app.dialog.as_ref().unwrap()).iter().filter(|r| r.enabled).count()
    };
    assert_eq!(enabled(&h), 4, "four corners are cut before any click");
    // The X takes the cut away and leaves the row; the same button brings it back.
    h.step(&json!({"click":{"handle":"dialog:list:corner:0:remove"}})).unwrap();
    h.frames(2);
    let now = crate::sketch_modify_ui::dialog_rows(&h.app.session, h.app.dialog.as_ref().unwrap());
    assert_eq!(now.len(), 4, "a row turned off stays in the list");
    assert!(!now[0].enabled, "the clicked row is off");
    assert!(now[1..].iter().all(|row| row.enabled), "the other rows keep their cuts");
    let d = h.app.dialog.as_ref().unwrap().clone();
    let cmds = crate::dialogs::preview_commands(&h.app, &d).unwrap();
    assert_eq!(cmds.len(), 1, "one command cuts every corner: {cmds:?}");
    assert_eq!(cmds[0].1["points"].as_array().map(Vec::len), Some(3), "the corner turned off is not cut: {cmds:?}");
    assert!(crate::scenario::handle_at("dialog:list:corner:0:remove").is_some(), "the row keeps its Show button");
    h.step(&json!({"click":{"handle":"dialog:list:corner:0:remove"}})).unwrap();
    h.frames(2);
    assert_eq!(enabled(&h), 4, "the same button shows the cut again");
}

/// Two profiles meeting at one point have a corner each: the lines of one pair give their ends
/// at the place, and the lines of the other pair still have theirs, so both corners are cut. Four
/// lines at one point make two rows of two, not one row and two lines waiting.
#[test]
fn two_profiles_meeting_at_one_place_make_a_corner_for_each_pair() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    // Two profiles' corners on the origin, the way two rectangles drawn from it meet there.
    for (id, points) in [
        ("left", [[-10.0, -10.0], [0.0, 0.0]]),
        ("up", [[0.0, 0.0], [0.0, 10.0]]),
        ("down", [[0.0, 0.0], [0.0, -10.0]]),
        ("right", [[10.0, 0.0], [0.0, 0.0]]),
    ] {
        h.app.run("sketch.line", json!({"points": points, "ids": [id]})).unwrap();
    }
    h.settle();
    assert_eq!(labels(&h, &["left", "up"]), ["left-up"]);
    assert_eq!(labels(&h, &["left", "up", "down", "right"]), ["left-up", "down-right"]);
    // Each row is a corner of its own two lines, in the order they were picked.
    let sid = h.app.session.active_sketch.unwrap();
    let state = h.app.session.model.state();
    let sk = &state.sketch(sid).unwrap().sketch;
    let rows = crate::sketch_corner_rows::rows(sk, &line_picks(&h, &["left", "up", "down", "right"]), &serde_json::Map::new());
    assert_eq!(crate::sketch_corner_rows::params(sk, &rows[0]), json!({"a": "left", "b": "up"}));
    assert_eq!(crate::sketch_corner_rows::params(sk, &rows[1]), json!({"a": "down", "b": "right"}));
}

/// A corner row's X hides its cut, and that hiding outlives the row: when the lines leave the
/// list and are picked again the row comes back as the user left it, rather than cutting again.
#[test]
fn a_hidden_corner_stays_hidden_when_its_lines_leave_the_list_and_come_back() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
    h.settle();
    h.step(&json!({"start":"sketch.fillet"})).unwrap();
    h.frames(2);
    let rows = |h: &crate::scenario::Harness| crate::sketch_modify_ui::dialog_rows(&h.app.session, h.app.dialog.as_ref().unwrap());
    let set = |h: &mut crate::scenario::Harness, picks: Vec<solidcraft_engine::Sel>| {
        if let Some(input) = h.app.dialog.as_mut().and_then(|d| d.inputs.first_mut()) {
            input.items = picks;
        }
        h.frames(2);
    };
    let picks = line_picks(&h, &["l1", "l2"]);
    set(&mut h, picks.clone());
    assert_eq!(rows(&h).len(), 1);
    h.step(&json!({"click":{"handle":"dialog:list:corner:0:remove"}})).unwrap();
    h.frames(2);
    assert!(!rows(&h)[0].enabled, "the X takes the cut away");
    // The lines leave the list, so no corner is shown...
    set(&mut h, Vec::new());
    assert!(rows(&h).is_empty());
    // ...and come back: the corner is still the one the user left.
    set(&mut h, picks);
    let back = rows(&h);
    assert_eq!(back.len(), 1);
    assert!(!back[0].enabled, "a corner the user hid stays hidden");
}
