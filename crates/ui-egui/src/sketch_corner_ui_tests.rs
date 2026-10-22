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

/// Corner previews draw new curves when a corner is picked, just like Mirror and Offset do.
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

    // The preview should have drawn new curves (fillet arcs at picked corners).
    h.step(&json!({"until":{"count":{"name":"sketch_preview_curves","value":1}}})).unwrap();

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

    // Chamfer preview should also produce curves.
    h.step(&json!({"until":{"count":{"name":"sketch_preview_curves","value":1}}})).unwrap();

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
/// origin, puts its circle in the corner between the clicks without moving the rectangle, and
/// fixed sides no longer make it fail.
#[test]
fn two_tangent_circle_clicked_on_two_sides_fits_the_corner_and_moves_nothing() {
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
        let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
        let circle = sk.curves.iter().position(|c| matches!(c.kind, CurveKind::Circle { .. }));
        let circle = circle.unwrap_or_else(|| panic!("fixed {fixed}: no circle: {:?}", h.app.status));
        assert!(sk.center(circle).unwrap().dist(Vec2::new(-750.0, 50.0)) < 1e-6, "fixed {fixed}: {:?}", sk.center(circle));
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
