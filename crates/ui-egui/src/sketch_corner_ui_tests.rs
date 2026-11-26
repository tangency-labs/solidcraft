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
    // The ellipse's construction axes come first, so the line's id is read from the reply.
    let line =
        h.app.run("sketch.line", json!({"points":[[0,3],[5,3]],"connect":false,"infer":false})).unwrap()["curves"][0].as_str().unwrap().to_string();
    h.frames(2);
    h.step(&json!({"start":"sketch.extend"})).unwrap();
    h.step(&json!({"click":{"world":[5,3,0]}})).unwrap();
    let st = h.app.session.world_state().unwrap();
    let ss = st.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    let end = ss.sketch.point(ss.sketch.resolve_point(&format!("{line}.end")).unwrap()).unwrap();
    assert!(end.dist(Vec2::new(12.0, 3.0)) < 1e-7, "clicked endpoint must extend: {end:?}, {:?}", h.app.status);
    assert_eq!(ss.sketch.point(ss.sketch.resolve_point(&format!("{line}.start")).unwrap()), Some(Vec2::new(0.0, 3.0)));
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

    let st = crate::open_meshing::session_world(&h.app.session);
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

/// Run frames until the preview of the typed size is in. It computes on its own thread, and
/// its result (a failure shows its reason in the window, which moves the buttons) arrives on
/// whichever frame the thread finishes. A test that reads the window or clicks a handle
/// before then races that frame, and on a loaded machine loses.
fn preview_in(h: &mut crate::scenario::Harness) {
    let began = std::time::Instant::now();
    while h.app.preview.busy && began.elapsed() < std::time::Duration::from_secs(60) {
        h.frame();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    h.frames(2);
}

fn apply(h: &mut crate::scenario::Harness) {
    // Preview refusals can reflow the window during hover. The widget path resolves the
    // button again after hovering, so the click follows its current position.
    h.frames(2);
    preview_in(h);
    h.step(&json!({"widget":"OK"})).unwrap();
    h.frames(2);
}

/// Is there a point of the active sketch at `at`?
fn has_point(h: &crate::scenario::Harness, at: Vec2) -> bool {
    let st = h.app.session.world_state().unwrap();
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
        let st = h.app.session.world_state().unwrap();
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
        let st = h.app.session.world_state().unwrap();
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
                let state = session.world_state().unwrap();
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
    let st = h.app.session.world_state().unwrap();
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
            h.step(&json!({"click":{"handle":"dialog:Flip"}})).unwrap();
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

/// #9316: the refusal of a size being typed (`4 m` on the way to `4 mm`) goes once the size fits.
#[test]
fn a_refused_size_gives_way_to_one_that_fits() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,30]})).unwrap();
    h.frames(2);
    h.step(&json!({"start":"sketch.fillet"})).unwrap();
    h.frames(2);
    h.step(&json!({"click":{"world":[0,0,0]}})).unwrap();
    h.frames(2);
    let shown = |h: &mut crate::scenario::Harness, size: &str| {
        set_size(&mut h.app, size);
        h.settle();
        preview_in(h);
        h.app.dialog.as_ref().and_then(|d| d.error.clone()).or_else(|| h.app.preview.error.clone())
    };
    assert!(shown(&mut h, "4 m").is_some_and(|e| e.contains("too large")));
    assert_eq!(shown(&mut h, "4 mm"), None);
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
    let st = h.app.session.world_state().unwrap();
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
    let st = h.app.session.world_state().unwrap();
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
    let st = h.app.session.world_state().unwrap();
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
    let sk = h.app.session.world_state().unwrap().sketch(sid).unwrap().sketch.clone();
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

    let sk = h.app.session.world_state().unwrap().sketch(sid).unwrap().sketch.clone();
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

/// #14227: a spline crossed by two lines shows the piece between them too, not the whole curve.
#[test]
fn trim_hover_shows_the_piece_a_click_takes_away_on_a_spline() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    let spline = h.app.run("sketch.spline.fit_point", json!({"points": [[0,0],[5,0],[30,0]]})).unwrap()["curves"][0].as_str().unwrap().to_string();
    h.app.run("sketch.line", json!({"points": [[10,-5],[10,20]], "connect": false})).unwrap();
    h.app.run("sketch.line", json!({"points": [[20,-5],[20,20]], "connect": false})).unwrap();
    h.settle();
    let sid = h.app.session.active_sketch.unwrap();
    h.step(&json!({"start":"sketch.trim"})).unwrap();
    h.step(&json!({"move":{"world":[15,0,0]}})).unwrap();
    assert!(matches!(&h.app.viewport.hover, Some(crate::viewport::Hit::SketchCurve { id, .. }) if *id == spline), "{:?}", h.app.viewport.hover);
    let piece = trim_hover_piece(&h.app, sid, &spline).unwrap();
    let ends = [piece[0], piece[piece.len() - 1]];
    assert!(ends.iter().any(|p| (p.x - 10.0).abs() < 1e-6) && ends.iter().any(|p| (p.x - 20.0).abs() < 1e-6), "{ends:?}");
    assert!(piece.iter().all(|p| p.x > 10.0 - 1e-6 && p.x < 20.0 + 1e-6), "{piece:?}");
}

/// Trim's hover keeps the cuts of the curve under the cursor while the sketch is unchanged and
/// Extend's preview while the cursor stays nearer one end: the piece follows the cursor along the
/// line, and a curve added in the way moves the piece's end and the extension's stop with it.
#[test]
fn trim_and_extend_hovers_follow_the_cursor_and_a_changed_sketch() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    let line = h.app.run("sketch.line", json!({"points": [[0,0],[30,0]], "connect": false})).unwrap()["curves"][0].as_str().unwrap().to_string();
    h.app.run("sketch.line", json!({"points": [[10,-5],[10,5]], "connect": false})).unwrap();
    h.app.run("sketch.line", json!({"points": [[20,-5],[20,5]], "connect": false})).unwrap();
    h.settle();
    let sid = h.app.session.active_sketch.unwrap();
    let ends = |p: &[Vec2]| {
        let mut x = [p[0].x, p[p.len() - 1].x];
        x.sort_by(f64::total_cmp);
        x
    };
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6;
    h.step(&json!({"start":"sketch.trim"})).unwrap();
    for (at, want) in [(15.0, [10.0, 20.0]), (16.0, [10.0, 20.0]), (25.0, [20.0, 30.0]), (4.0, [0.0, 10.0])] {
        h.step(&json!({"move":{"world":[at,0,0]}})).unwrap();
        let piece = trim_hover_piece(&h.app, sid, &line).unwrap();
        assert!(near(ends(&piece), want), "at {at}: {piece:?}");
    }
    h.app.run("sketch.line", json!({"points": [[24,-5],[24,5]], "connect": false})).unwrap();
    h.settle();
    h.step(&json!({"move":{"world":[27,0,0]}})).unwrap();
    let piece = trim_hover_piece(&h.app, sid, &line).unwrap();
    assert!(near(ends(&piece), [24.0, 30.0]), "the new line cuts: {piece:?}");
    let at = h.app.tool.as_ref().and_then(|t| t.hover.as_ref()).map(|h| h.0).unwrap();
    let fresh = solidcraft_engine::cmd::trim_piece(&h.app.session.world_state().unwrap().sketch(sid).unwrap().sketch, &line, at);
    assert_eq!(Some(piece), fresh, "the kept cuts are those of the sketch as it is");
    h.step(&json!({"key":"Escape"})).unwrap();

    let short = h.app.run("sketch.line", json!({"points": [[0,20],[8,20]], "connect": false})).unwrap()["curves"][0].as_str().unwrap().to_string();
    h.app.run("sketch.line", json!({"points": [[18,15],[18,25]], "connect": false})).unwrap();
    h.settle();
    h.step(&json!({"start":"sketch.extend"})).unwrap();
    for at in [7.0, 6.0, 5.0] {
        h.step(&json!({"move":{"world":[at,20,0]}})).unwrap();
        let piece = extend_preview_piece(&h.app, sid, &short).unwrap();
        assert!(near([piece[0].x, piece[1].x], [8.0, 18.0]), "at {at}: {piece:?}");
    }
    h.step(&json!({"move":{"world":[2,20,0]}})).unwrap();
    let piece = extend_preview_piece(&h.app, sid, &short).unwrap();
    assert!((piece[0].x).abs() < 1e-6 && piece[1].x < 0.0, "the other end grows: {piece:?}");
    h.app.run("sketch.line", json!({"points": [[13,15],[13,25]], "connect": false})).unwrap();
    h.settle();
    h.step(&json!({"move":{"world":[7,20,0]}})).unwrap();
    let piece = extend_preview_piece(&h.app, sid, &short).unwrap();
    assert!(near([piece[0].x, piece[1].x], [8.0, 13.0]), "the new line stops it: {piece:?}");
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

/// #15387: the Point tool clicked on the crossing of two lines makes a point held on both lines
/// (`PointOnCurve` on each), not a free point that happens to sit at their intersection.
#[test]
fn a_point_clicked_on_a_line_crossing_is_held_on_both() {
    use solidcraft_engine::sketch::ConstraintKind;
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[20,20]],"ids":["a"],"infer":false})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,20],[20,0]],"ids":["b"],"infer":false})).unwrap();
    h.settle();
    h.step(&json!({"start":"sketch.point"})).unwrap();
    h.step(&json!({"click":{"world":[10, 10, 0]}})).unwrap();
    let sid = h.app.session.active_sketch.unwrap();
    let state = h.app.session.model.state();
    let sk = &state.sketch(sid).unwrap().sketch;
    let p = sk.points.len() - 1;
    for line in ["a", "b"] {
        let c = sk.curve_index(line).unwrap();
        let held = sk.constraints.iter().any(|k| k.kind == ConstraintKind::PointOnCurve { p, c });
        assert!(held, "not held on {line}: {:?}", sk.constraints.iter().map(|k| &k.kind).collect::<Vec<_>>());
    }
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
            let state = crate::open_meshing::session_world(&h.app.session);
            let sk = &state.sketch(sketch).unwrap().sketch;
            let hit = Hit::SketchCurve { sketch, id: id.clone(), straight: true, point: solidcore_record::Vec3::ZERO };
            let offered = candidate(h.app.dialog.as_ref().unwrap(), &h.app.session, &hit).is_some();
            // Fillet takes a drawn 3D line, to round the corner it makes with another (#21319).
            let fillet_line = cmd == "sketch.fillet" && *id == line;
            assert_eq!(offered, !sk.is_spatial_corner_curve(id) || fillet_line, "{cmd}: {id}");
            assert_eq!(offered, id == "side" || fillet_line, "{cmd}: {id}");
        }
        h.app.dialog = None;
    }
}

#[test]
fn line_arc_fillet_preview_and_apply_work_in_both_pick_orders() {
    for reverse in [false, true] {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.line", json!({"points":[[0,0],[40,0]],"ids":["l1"]})).unwrap();
        h.app.run("sketch.arc.center_point", json!({"center":[50,0],"start":[60,0],"end":[40,0],"id":"a1"})).unwrap();
        h.app.run("sketch.constraint.fix", json!({"entity":"l1.end"})).unwrap();
        h.app.run("view.fit", json!({})).unwrap();
        h.settle();
        let undo = h.app.session.undo.len();
        h.step(&json!({"start":"sketch.fillet"})).unwrap();
        h.frames(2);
        let mut picks = [[32, 0, 0], [50, 10, 0]];
        if reverse {
            picks.reverse();
        }
        for point in picks {
            h.step(&json!({"click":{"world":point}})).unwrap();
            h.frames(2);
        }
        assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items.len(), 2);
        set_size(&mut h.app, "3 mm");
        h.step(&json!({"until":{"count":{"name":"sketch_preview_curves","value":3}}})).unwrap();
        apply(&mut h);
        assert!(h.app.dialog.is_none(), "reverse {reverse}: {:?}", h.app.status);
        assert_eq!(h.app.session.undo.len(), undo + 1);
        let st = h.app.session.world_state().unwrap();
        let ss = st.sketch(h.app.session.active_sketch.unwrap()).unwrap();
        assert!(ss.report.ok());
        assert_eq!(ss.sketch.curves.len(), 3);
        let want = Vec2::new(50.0 - 160f64.sqrt(), 3.0);
        assert!(ss.sketch.curves.iter().enumerate().any(|(i, _)| {
            ss.sketch.radius(i).is_some_and(|r| (r - 3.0).abs() < 1e-7) && ss.sketch.center(i).is_some_and(|c| c.dist(want) < 1e-6)
        }));
    }
}

/// #21319: the Fillet tool takes two drawn 3D lines, previews the round, and rounds the corner
/// they share with a 3D arc tangent to both, in one undo step.
#[test]
fn fillet_tool_rounds_the_corner_of_two_picked_3d_lines() {
    use solidcraft_engine::sketch::WireConstraintKind;
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.options", json!({"sketch_3d":true})).unwrap();
    h.app.run("sketch.line3d", json!({"points":[[0,0,0],[100,0,0],[100,60,40]]})).unwrap();
    h.app.run("view.fit", json!({})).unwrap();
    h.settle();
    let undo = h.app.session.undo.len();
    h.step(&json!({"start":"sketch.fillet"})).unwrap();
    h.frames(2);
    for point in [[40, 0, 0], [100, 30, 20]] {
        h.step(&json!({"click":{"world":point}})).unwrap();
        h.frames(2);
    }
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items.len(), 2, "{:?}", h.app.status);
    set_size(&mut h.app, "20 mm");
    // The preview draws the arc and the two shortened lines.
    h.step(&json!({"until":{"count":{"name":"sketch_preview_curves","value":3}}})).unwrap();
    apply(&mut h);
    assert!(h.app.dialog.is_none(), "{:?}", h.app.status);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    let st = h.app.session.world_state().unwrap();
    let ss = st.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    assert!(ss.report.wires_ok());
    let arc = ss.sketch.wires.iter().find(|w| w.arc.is_some()).expect("a 3D arc rounds the corner");
    assert!(arc.arc.is_some_and(|a| (a.radius - 20.0).abs() < 1e-6), "{arc:?}");
    let tangents =
        ss.sketch.wire_constraints.iter().filter(|c| matches!(&c.kind, WireConstraintKind::Tangent { arc: on, .. } if *on == arc.id)).count();
    assert_eq!(tangents, 2, "{:?}", ss.sketch.wire_constraints);
}

/// An ellipse's axis is never a side of a Fillet or Chamfer corner (the engine refuses it), so a
/// line drawn from the major end is no corner the picker offers.
#[test]
fn an_ellipse_axis_end_is_no_corner_to_pick() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    let e = h.app.run("sketch.ellipse", json!({"center":[0,0],"major":[20,0],"minor_radius":8})).unwrap()["curves"][0].as_str().unwrap().to_string();
    let major_end = {
        let st = h.app.session.model.state();
        let sk = &st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        let CurveKind::Ellipse { m, .. } = sk.curves[sk.curve_index(&e).unwrap()].kind else { panic!("an ellipse") };
        sk.points[m].id.clone()
    };
    h.app.run("sketch.line", json!({"points":[major_end, [40,10]],"connect":false,"infer":false})).unwrap();
    let st = h.app.session.model.state();
    let sk = &st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    let pi = sk.resolve_point(&major_end).unwrap();
    for arcs_ok in [true, false] {
        assert_eq!(crate::sketch_tools::corner_curves(sk, pi, arcs_ok), None, "the axis and the line are no corner (arcs_ok {arcs_ok})");
    }
}

/// A corner where a conic or a control point spline meets a line is one the Fillet tool offers
/// (#9178), and one the Chamfer tool does not: chamfers stay on lines. A fit point spline's
/// corner is offered to neither (fillets do not take it yet).
#[test]
fn a_conic_or_spline_corner_is_offered_to_fillet_only() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[10,0]],"ids":["base"],"connect":false})).unwrap();
    h.app.run("sketch.conic", json!({"start":"base.end","apex":[20,5],"end":[20,10],"rho":0.5})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,20],[10,20]],"ids":["top"],"connect":false})).unwrap();
    h.app.run("sketch.spline.control_point", json!({"points":["top.end",[15,25],[20,24],[25,30]]})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,40],[10,40]],"ids":["fit"],"connect":false})).unwrap();
    h.app.run("sketch.spline.fit_point", json!({"points":["fit.end",[15,45],[20,44],[25,50]]})).unwrap();
    let st = h.app.session.model.state();
    let sk = &st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    for corner in ["base.end", "top.end"] {
        let pi = sk.resolve_point(corner).unwrap();
        assert!(crate::sketch_tools::corner_curves(sk, pi, true).is_some(), "{corner} is a corner a fillet rounds");
        assert_eq!(crate::sketch_tools::corner_curves(sk, pi, false), None, "{corner} is no corner a chamfer cuts");
    }
    let pi = sk.resolve_point("fit.end").unwrap();
    assert_eq!(crate::sketch_tools::corner_curves(sk, pi, true), None, "a fit point spline corner is not offered");
}

/// A sketch on XY with a horizontal line `base` (y = 0, x 0 to 60) and a vertical line `side`
/// (x = 50, y -10 to 40), both fixed, and the drawing tool `tool` clicked at each of `clicks`.
/// Returns the sketch as the clicks left it.
fn rectangle_clicked_on_lines(tool: &str, clicks: &[[i32; 3]]) -> solidcraft_engine::sketch::Sketch {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[60,0]],"ids":["base"]})).unwrap();
    h.app.run("sketch.line", json!({"points":[[50,-10],[50,40]],"ids":["side"]})).unwrap();
    for line in ["base", "side"] {
        h.app.run("sketch.constraint.fix", json!({"entity":line})).unwrap();
    }
    h.step(&json!({"call":"ui.view","params":{"view":"fit"}})).unwrap();
    h.settle();
    h.step(&json!({"start":tool})).unwrap();
    for at in clicks {
        h.step(&json!({"move":{"world":at}})).unwrap();
        h.step(&json!({"click":{"world":at}})).unwrap();
    }
    let sid = h.app.session.active_sketch.unwrap();
    h.app.session.model.state().sketch(sid).unwrap().sketch.clone()
}

/// The points of `sk` held on curve `id` by a Coincident (point on curve), where they are (as
/// clicked: within what a pixel of the view is in the sketch).
fn held_on(sk: &solidcraft_engine::sketch::Sketch, id: &str) -> Vec<Vec2> {
    let curve = sk.curve_index(id).unwrap();
    sk.constraints
        .iter()
        .filter_map(|k| match k.kind {
            solidcraft_engine::sketch::ConstraintKind::PointOnCurve { p, c } if c == curve => sk.point(p),
            _ => None,
        })
        .collect()
}

/// A 2-Point Rectangle corner clicked on a line shows the "On" snap and is kept on that line, as
/// a Line's point clicked there is: the first corner, and the second one too.
#[test]
fn a_two_point_rectangle_corner_clicked_on_a_line_stays_on_it() {
    let line = rectangle_clicked_on_lines("sketch.line", &[[20, 0, 0], [30, 25, 0]]);
    assert_eq!(held_on(&line, "base").len(), 1, "the Line's point on the line: {:?}", line.constraints);

    let first = rectangle_clicked_on_lines("sketch.rectangle.two_point", &[[20, 0, 0], [40, 25, 0]]);
    let on_base = held_on(&first, "base");
    assert_eq!(on_base.len(), 1, "the first corner on the line: {:?}", first.constraints);
    assert!(on_base[0].dist(Vec2::new(20.0, 0.0)) < 1e-3, "{on_base:?}");
    assert!(held_on(&first, "side").is_empty(), "the second corner was clicked off the lines: {:?}", first.constraints);

    let both = rectangle_clicked_on_lines("sketch.rectangle.two_point", &[[20, 0, 0], [50, 25, 0]]);
    assert_eq!(held_on(&both, "base").len(), 1, "the first corner on the line: {:?}", both.constraints);
    let on_side = held_on(&both, "side");
    assert_eq!(on_side.len(), 1, "the second corner on the other line: {:?}", both.constraints);
    assert!(on_side[0].dist(Vec2::new(50.0, 25.0)) < 1e-3, "{on_side:?}");
}

/// A Center Rectangle's centre and its corner, each clicked on a line, are kept on those lines.
#[test]
fn a_center_rectangle_clicked_on_lines_stays_on_them() {
    let sk = rectangle_clicked_on_lines("sketch.rectangle.center", &[[20, 0, 0], [50, 12, 0]]);
    let on_base = held_on(&sk, "base");
    assert_eq!(on_base.len(), 1, "the centre on the line: {:?}", sk.constraints);
    assert!(on_base[0].dist(Vec2::new(20.0, 0.0)) < 1e-3, "{on_base:?}");
    let on_side = held_on(&sk, "side");
    assert_eq!(on_side.len(), 1, "the corner on the other line: {:?}", sk.constraints);
    assert!(on_side[0].dist(Vec2::new(50.0, 12.0)) < 1e-3, "{on_side:?}");
}

/// A drawing left open while a model sketch is drawn in Design: the rectangle's corner clicked on
/// a line is kept on it, as with no drawing open.
#[test]
fn a_rectangle_corner_clicked_on_a_line_stays_on_it_with_a_drawing_open() {
    let mut h = crate::scenario::Harness::new();
    // A body away from the sketch, for the drawing to show.
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 5, "corner": [200, 200, -50]})).unwrap();
    h.app.run("drawing.create", json!({})).unwrap();
    h.app.ui.tab = "MODEL".into();
    // Setup only: drawing.create focuses the new Sheet view, and a sketch drawn in the Model view
    // takes input only while that view is focused; the drawing stays open in its Sheet view.
    let model = h.app.docs.views.views().iter().find(|view| view.kind == solidcraft_views::ViewKind::Model).unwrap().id;
    assert!(crate::documents::view_activate(&mut h.app, model));
    h.settle();
    assert!(h.app.session.open_drawing.is_some());
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[60,0]],"ids":["base"]})).unwrap();
    h.app.run("sketch.constraint.fix", json!({"entity":"base"})).unwrap();
    h.step(&json!({"call":"ui.view","params":{"view":"fit"}})).unwrap();
    h.settle();
    assert!(!h.app.session.sheet_sketching());
    h.step(&json!({"start":"sketch.rectangle.two_point"})).unwrap();
    for at in [[20, 0, 0], [40, 25, 0]] {
        h.step(&json!({"move":{"world":at}})).unwrap();
        h.step(&json!({"click":{"world":at}})).unwrap();
    }
    let sid = h.app.session.active_sketch.unwrap();
    let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
    let on_base = held_on(&sk, "base");
    assert_eq!(on_base.len(), 1, "the first corner on the line: {:?}", sk.constraints);
    assert!(on_base[0].dist(Vec2::new(20.0, 0.0)) < 1e-3, "{on_base:?}");
}

/// A 2-Point Rectangle corner clicked on a line's midpoint is held at that midpoint.
#[test]
fn a_two_point_rectangle_corner_clicked_on_a_midpoint_is_held_there() {
    let sk = rectangle_clicked_on_lines("sketch.rectangle.two_point", &[[30, 0, 0], [40, 25, 0]]);
    let base = sk.curve_index("base").unwrap();
    let held: Vec<usize> = sk
        .constraints
        .iter()
        .filter_map(|k| match k.kind {
            solidcraft_engine::sketch::ConstraintKind::Midpoint { p, l } if l == base => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(held.len(), 1, "one MidPoint constraint on the line: {:?}", sk.constraints);
    // The point held there is the rectangle's corner: an end of two of its sides.
    let sides = sk.curves.iter().enumerate().filter(|(i, c)| *i != base && c.kind.point_ids().contains(&held[0])).count();
    assert_eq!(sides, 2, "the held point is a corner of the rectangle: {:?}", sk.curves);
    assert!(sk.point(held[0]).is_some_and(|at| at.dist(Vec2::new(30.0, 0.0)) < 1e-3));
}

/// #18756 as reported: a sketch on a box's top face with the face's edges projected, and a 2-Point
/// Rectangle whose first click is on a projected edge. The corner is kept on that edge.
#[test]
fn a_two_point_rectangle_corner_clicked_on_a_projected_edge_stays_on_it() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length":60,"width":40,"height":10})).unwrap();
    h.app.run("sketch.create", json!({"plane":{"face":[30,20,10]},"project_edges":true})).unwrap();
    h.step(&json!({"call":"ui.view","params":{"view":"fit"}})).unwrap();
    h.settle();
    h.step(&json!({"start":"sketch.rectangle.two_point"})).unwrap();
    for at in [[20, 0, 10], [45, 25, 10]] {
        h.step(&json!({"move":{"world":at}})).unwrap();
        h.step(&json!({"click":{"world":at}})).unwrap();
    }
    let sid = h.app.session.active_sketch.unwrap();
    let state = h.app.session.model.state();
    let ss = state.sketch(sid).unwrap();
    let sk = &ss.sketch;
    let held: Vec<Vec2> = sk
        .constraints
        .iter()
        .filter_map(|k| match k.kind {
            solidcraft_engine::sketch::ConstraintKind::PointOnCurve { p, c } if sk.is_linked_curve(c) => sk.point(p),
            _ => None,
        })
        .collect();
    assert_eq!(held.len(), 1, "the corner on the projected edge: {:?}", sk.constraints);
    assert!(held[0].dist(Vec2::new(20.0, 0.0)) < 1e-3, "{held:?}");
    assert!(ss.report.ok());
    assert_eq!(ss.report.dof, 3, "along the edge, width and height");
}

/// A 2-Point Rectangle corner clicked on a body's vertex is joined to that vertex, projected.
#[test]
fn a_two_point_rectangle_corner_clicked_on_a_body_vertex_is_joined_to_it() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"length":60,"width":40,"height":10})).unwrap();
    h.app.run("sketch.create", json!({"plane":{"face":[30,20,10]},"project_edges":false})).unwrap();
    // A point level with the vertex. A click snapped to a body vertex takes no level or plumb
    // alignment (the vertex snap comes first), so nothing is aligned to the projected vertex in
    // place of the corner.
    h.app.run("sketch.point", json!({"point":[20,0]})).unwrap();
    h.step(&json!({"call":"ui.view","params":{"view":"fit"}})).unwrap();
    h.settle();
    h.step(&json!({"start":"sketch.rectangle.two_point"})).unwrap();
    for at in [[60, 0, 10], [40, 20, 10]] {
        h.step(&json!({"move":{"world":at}})).unwrap();
        h.step(&json!({"click":{"world":at}})).unwrap();
    }
    let sid = h.app.session.active_sketch.unwrap();
    let state = h.app.session.model.state();
    let ss = state.sketch(sid).unwrap();
    let sk = &ss.sketch;
    let joined = sk.constraints.iter().filter(|k| matches!(k.kind, solidcraft_engine::sketch::ConstraintKind::Coincident { .. })).count();
    assert_eq!(joined, 1, "the corner joined to the projected vertex: {:?}", sk.constraints);
    let aligned = sk.constraints.iter().any(|k| {
        matches!(
            k.kind,
            solidcraft_engine::sketch::ConstraintKind::HorizontalPoints { .. } | solidcraft_engine::sketch::ConstraintKind::VerticalPoints { .. }
        )
    });
    assert!(!aligned, "no alignment was added for the vertex click: {:?}", sk.constraints);
    assert!(ss.report.ok());
    assert_eq!(ss.report.dof, 4, "width and height, and the free point");
}

/// With the snap suppress modifier held (Alt by default), a rectangle corner clicked over a line
/// takes no snap and is not tied to the line.
#[test]
fn a_rectangle_corner_clicked_with_snap_suppressed_is_not_tied() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[60,0]],"ids":["base"]})).unwrap();
    h.step(&json!({"call":"ui.view","params":{"view":"fit"}})).unwrap();
    h.settle();
    h.step(&json!({"start":"sketch.rectangle.two_point"})).unwrap();
    for at in [[20, 0, 0], [40, 25, 0]] {
        h.step(&json!({"move":{"world":at},"alt":true})).unwrap();
        h.step(&json!({"click":{"world":at},"alt":true})).unwrap();
    }
    let sid = h.app.session.active_sketch.unwrap();
    let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
    assert_eq!(sk.curves.len(), 5, "the rectangle was drawn: {:?}", sk.curves);
    assert!(held_on(&sk, "base").is_empty(), "{:?}", sk.constraints);
}

/// A 2-Point Rectangle corner clicked on the crossing of two lines is held on both.
#[test]
fn a_two_point_rectangle_corner_clicked_on_a_crossing_is_held_on_both() {
    let sk = rectangle_clicked_on_lines("sketch.rectangle.two_point", &[[50, 0, 0], [30, 20, 0]]);
    let (on_base, on_side) = (held_on(&sk, "base"), held_on(&sk, "side"));
    assert_eq!((on_base.len(), on_side.len()), (1, 1), "{:?}", sk.constraints);
    assert!(on_base[0].dist(Vec2::new(50.0, 0.0)) < 1e-3 && on_side[0].dist(on_base[0]) < 1e-9, "{on_base:?} {on_side:?}");
}

/// Typed sizes with a corner held on a line. The size boxes appear after the first click, so the
/// first corner keeps its snap. The second corner then comes from the typed values, not from the
/// pointer: with one size typed it takes no snap either, where it is clicked on a line.
#[test]
fn typed_sizes_keep_the_first_corner_on_its_line_and_place_the_second() {
    let drawn = |sizes: &[&str], second: [i32; 3], enter: bool| {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.line", json!({"points":[[0,0],[60,0]],"ids":["base"]})).unwrap();
        h.app.run("sketch.line", json!({"points":[[50,-10],[50,40]],"ids":["side"]})).unwrap();
        for line in ["base", "side"] {
            h.app.run("sketch.constraint.fix", json!({"entity":line})).unwrap();
        }
        h.step(&json!({"call":"ui.view","params":{"view":"fit"}})).unwrap();
        h.settle();
        h.step(&json!({"start":"sketch.rectangle.two_point"})).unwrap();
        h.step(&json!({"move":{"world":[20, 0, 0]}})).unwrap();
        h.step(&json!({"click":{"world":[20, 0, 0]}})).unwrap();
        h.step(&json!({"move":{"world":second}})).unwrap();
        for (i, size) in sizes.iter().enumerate() {
            if i > 0 {
                h.step(&json!({"key":"Tab"})).unwrap();
            }
            h.step(&json!({"key":"A","cmd":true})).unwrap();
            h.step(&json!({"text":size})).unwrap();
        }
        if enter {
            h.step(&json!({"key":"Enter"})).unwrap();
        } else {
            h.step(&json!({"click":{"world":second}})).unwrap();
        }
        let sid = h.app.session.active_sketch.unwrap();
        let state = h.app.session.model.state();
        let ss = state.sketch(sid).unwrap();
        assert!(ss.report.ok(), "{sizes:?}: {:?}", ss.report);
        (ss.sketch.clone(), ss.report.dof)
    };
    // Both sizes typed: the first corner on the line, both sizes driving, one freedom left.
    let (sk, dof) = drawn(&["30", "12"], [40, 25, 0], true);
    assert_eq!(held_on(&sk, "base").len(), 1, "{:?}", sk.constraints);
    assert_eq!(sk.constraints.iter().filter(|k| k.param.is_some() && !k.driven).count(), 2, "{:?}", sk.constraints);
    assert_eq!(dof, 1, "along the line");
    // Only the width typed, the second click on the other line: the corner goes where the typed
    // width puts it (x = 45, off the line at x = 50), so the click's snap is not used. Without a
    // typed size the same click is held on that line
    // (`a_two_point_rectangle_corner_clicked_on_a_line_stays_on_it`).
    let (sk, _) = drawn(&["25"], [50, 25, 0], false);
    assert_eq!(held_on(&sk, "base").len(), 1, "{:?}", sk.constraints);
    assert!(held_on(&sk, "side").is_empty(), "{:?}", sk.constraints);
    let corner = sk.points.iter().map(|p| p.pos).find(|p| p.dist(Vec2::new(45.0, 25.0)) < 1e-3);
    assert!(corner.is_some(), "the second corner is 25 mm from the first: {:?}", sk.points.iter().map(|p| p.pos).collect::<Vec<_>>());
}

/// The Undo button between a rectangle's two clicks can take away the curve the first corner snapped to. The
/// second click then still draws the rectangle, through the places clicked, and the tool goes on.
#[test]
fn a_rectangle_whose_first_corner_lost_its_curve_to_undo_is_still_drawn() {
    for (tool, lines) in [("sketch.rectangle.two_point", 4), ("sketch.rectangle.center", 6)] {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.line", json!({"points":[[0,30],[60,30]],"ids":["keep"]})).unwrap();
        h.app.run("sketch.line", json!({"points":[[0,0],[60,0]],"ids":["base"]})).unwrap();
        h.step(&json!({"call":"ui.view","params":{"view":"fit"}})).unwrap();
        h.settle();
        h.step(&json!({"start":tool})).unwrap();
        h.step(&json!({"move":{"world":[20, 0, 0]}})).unwrap();
        h.step(&json!({"click":{"world":[20, 0, 0]}})).unwrap();
        let first = h.app.tool.as_ref().unwrap().pts.clone();
        assert!(first.len() == 1 && first[0].1.as_deref().is_some_and(|snap| snap.starts_with("on:base:")), "{tool}: {first:?}");
        // The Undo button: it runs `edit.undo` and leaves the tool on with its first point (the
        // Undo key steps the tool itself back first).
        h.app.run("edit.undo", json!({})).unwrap();
        h.settle();
        let sid = h.app.session.active_sketch.unwrap();
        let gone = h.app.session.model.state().sketch(sid).unwrap().sketch.curve_index("base").is_none();
        assert!(gone, "{tool}: Undo took the line the corner is on");
        assert_eq!(h.app.tool.as_ref().map(|t| t.pts.len()), Some(1), "{tool}: the first corner is still placed");
        h.step(&json!({"move":{"world":[40, 20, 0]}})).unwrap();
        h.step(&json!({"click":{"world":[40, 20, 0]}})).unwrap();
        let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
        assert!(!h.app.status.as_ref().is_some_and(|s| s.2), "{tool}: {:?}", h.app.status);
        assert_eq!(sk.curves.len(), 1 + lines, "{tool}: the rectangle beside the line that stayed: {:?}", sk.curves);
        assert!(h.app.tool.as_ref().is_some_and(|t| t.pts.is_empty()), "{tool}: ready for the next rectangle");
    }
}

/// A sketch on XY with the lines `keep` (y = 30) and `base` (y = 0), the tool `tool` clicked at
/// each of `before`, then the Undo button (it runs `edit.undo` and leaves the tool on), which
/// takes `base` away.
fn clicked_then_undone(tool: &str, before: &[[i32; 3]]) -> crate::scenario::Harness {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,30],[60,30]],"ids":["keep"]})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[60,0]],"ids":["base"]})).unwrap();
    h.step(&json!({"call":"ui.view","params":{"view":"fit"}})).unwrap();
    h.settle();
    h.step(&json!({"start":tool})).unwrap();
    for at in before {
        h.step(&json!({"move":{"world":at}})).unwrap();
        h.step(&json!({"click":{"world":at}})).unwrap();
    }
    let pts = h.app.tool.as_ref().unwrap().pts.clone();
    assert!(pts.last().and_then(|p| p.1.as_deref()).is_some_and(|snap| snap.starts_with("on:base:")), "{tool}: {pts:?}");
    h.app.run("edit.undo", json!({})).unwrap();
    h.settle();
    let sid = h.app.session.active_sketch.unwrap();
    assert!(h.app.session.model.state().sketch(sid).unwrap().sketch.curve_index("base").is_none(), "{tool}: Undo took the line");
    assert_eq!(h.app.tool.as_ref().map(|t| t.pts.len()), Some(before.len()), "{tool}: the clicks are still placed");
    h
}

/// The Undo button between a shape's clicks can take away the curve a stored click snapped to
/// (#21835). The next clicks then still draw the shape, through the places clicked, and the tool
/// goes on: a circle or an arc from its centre, the centre or start on the removed line, and the
/// other shapes that send a stored snap again.
#[test]
fn a_shape_whose_stored_click_lost_its_curve_to_undo_is_still_drawn() {
    let cases: [(&str, &[[i32; 3]], &[[i32; 3]], usize); 6] = [
        ("sketch.circle.center", &[[20, 0, 0]], &[[40, 20, 0]], 1),
        ("sketch.arc.center_point", &[[20, 0, 0]], &[[40, 15, 0], [10, 20, 0]], 1),
        ("sketch.arc.center_point", &[[20, 15, 0], [20, 0, 0]], &[[35, 15, 0]], 1),
        ("sketch.circle.three_point", &[[20, 0, 0]], &[[40, 20, 0], [5, 20, 0]], 1),
        ("sketch.arc.three_point", &[[20, 0, 0]], &[[45, 20, 0], [30, 15, 0]], 1),
        ("sketch.rectangle.edge_center", &[[20, 0, 0]], &[[40, 8, 0], [40, 20, 0]], 4),
    ];
    let mut failed = Vec::new();
    for (tool, before, after, added) in cases {
        let mut h = clicked_then_undone(tool, before);
        for at in after {
            h.step(&json!({"move":{"world":at}})).unwrap();
            h.step(&json!({"click":{"world":at}})).unwrap();
        }
        let sid = h.app.session.active_sketch.unwrap();
        let curves = h.app.session.model.state().sketch(sid).unwrap().sketch.curves.len();
        let error = h.app.status.as_ref().filter(|s| s.2).map(|s| s.0.clone());
        let ready = h.app.tool.as_ref().is_some_and(|t| t.pts.is_empty());
        if error.is_some() || curves != 1 + added || !ready {
            failed.push(format!("{tool} after {} click(s): {curves} curves, ready {ready}, {error:?}", before.len()));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// Redo after that Undo brings the line back, and the circle's centre clicked on it is held on it
/// again: the stored snap is only set aside while its curve is gone.
#[test]
fn a_circle_centre_snapped_to_a_line_undone_and_redone_stays_on_it() {
    let mut h = clicked_then_undone("sketch.circle.center", &[[20, 0, 0]]);
    h.app.run("edit.redo", json!({})).unwrap();
    h.settle();
    h.step(&json!({"move":{"world":[40, 20, 0]}})).unwrap();
    h.step(&json!({"click":{"world":[40, 20, 0]}})).unwrap();
    let sid = h.app.session.active_sketch.unwrap();
    let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
    assert!(!h.app.status.as_ref().is_some_and(|s| s.2), "{:?}", h.app.status);
    assert_eq!(sk.curves.len(), 3, "{:?}", sk.curves);
    let held = held_on(&sk, "base");
    assert!(held.iter().any(|p| p.dist(Vec2::new(20.0, 0.0)) < 0.5), "the centre is held on the line: {held:?}");
}

/// The Edge-Center Rectangle's second click snapped onto a line, a little off a sketch axis, and
/// the Undo button then took that line away: the rectangle is drawn with its edge the way it was
/// clicked and previewed, not nudged onto the axis.
#[test]
fn an_edge_center_rectangle_whose_edge_end_lost_its_curve_to_undo_keeps_its_clicked_direction() {
    let mut h = clicked_then_undone("sketch.rectangle.edge_center", &[[-15, 2, 0], [55, 0, 0]]);
    let pts = h.app.tool.as_ref().unwrap().pts.clone();
    assert!(pts[0].1.is_none(), "the middle is a free click: {pts:?}");
    let clicked = (pts[1].0 - pts[0].0).normalized().unwrap();
    let off_axis = clicked.y.atan2(clicked.x).to_degrees().abs();
    assert!((1.0..2.0).contains(&off_axis), "the edge is clicked {off_axis} degrees off the X axis");
    h.step(&json!({"move":{"world":[40, 15, 0]}})).unwrap();
    h.step(&json!({"click":{"world":[40, 15, 0]}})).unwrap();
    assert!(!h.app.status.as_ref().is_some_and(|s| s.2), "{:?}", h.app.status);
    let sid = h.app.session.active_sketch.unwrap();
    let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
    let keep = sk.curve_index("keep").unwrap();
    let sides: Vec<Vec2> = sk
        .curves
        .iter()
        .enumerate()
        .filter_map(|(i, c)| match c.kind {
            CurveKind::Line { a, b } if i != keep => (sk.point(b)? - sk.point(a)?).normalized(),
            _ => None,
        })
        .collect();
    assert_eq!(sides.len(), 4, "{:?}", sk.curves);
    for side in &sides {
        let (along, across) = (side.dot(clicked).abs(), side.dot(clicked.perp()).abs());
        assert!(along.min(across) < 1e-6, "a side runs along {side:?}, the edge was clicked along {clicked:?}");
    }
}
