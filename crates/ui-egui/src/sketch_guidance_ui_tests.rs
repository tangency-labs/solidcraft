#![allow(clippy::unwrap_used, clippy::panic)]
use crate::scenario::Harness;
use crate::sketch_modify_ui::{Mod, candidate};
use crate::viewport::Hit;
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::geom::Vec3;

fn step(h: &mut Harness, value: Value) {
    h.step(&value).unwrap();
}
fn fixture() -> Harness {
    let mut h = Harness::new();
    h.app.preferences.look_at_anim = false;
    for (command, params) in [
        ("sketch.create", json!({"plane":"XY"})),
        ("sketch.line", json!({"points":[[-30,10],[-15,10]]})),
        ("sketch.line", json!({"points":[[-15,10],[-15,25]]})),
        ("sketch.line", json!({"points":[[0,-10],[0,40]],"construction":true})),
    ] {
        step(&mut h, json!({"call":"engine.execute","params":{"command":command,"params":params}}));
    }
    step(&mut h, json!({"call":"ui.view","params":{"view":"top"}}));
    h
}
fn pick(h: &mut Harness, xy: [f64; 2], ctrl: bool) {
    step(h, json!({"click":{"sketch":xy},"ctrl":ctrl}));
}
fn preview(h: &mut Harness, curves: usize) {
    step(h, json!({"until":{"count":{"name":"sketch_preview_curves","value":curves}}}));
}

#[test]
fn sketch_dialog_mirror_toggles_clears_references_previews_and_cancels() {
    let mut h = fixture();
    // Keep the source curves and reference between the floating dialog and Sketch Palette.
    step(&mut h, json!({"call":"ui.camera","params":{"target":[-40,0,0],"distance":150}}));
    let original = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    let revision = h.app.session.revision;
    step(&mut h, json!({"start":"sketch.mirror"}));
    assert!(h.app.tool.is_none());
    pick(&mut h, [-22.5, 10.0], false);
    pick(&mut h, [-15.0, 17.5], false);
    pick(&mut h, [-22.5, 10.0], true);
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items.len(), 1);
    pick(&mut h, [-22.5, 10.0], true);
    step(&mut h, json!({"call":"ui.dialogInput","params":{"index":1}}));
    pick(&mut h, [0.0, 20.0], false);
    preview(&mut h, 2);
    assert_eq!(h.app.session.doc.to_json(), original);
    assert_eq!(h.app.session.undo.len(), undo);
    assert_eq!(h.app.session.revision, revision);
    step(&mut h, json!({"click":{"handle":"dialog:clear:Mirror line"}}));
    assert!(h.app.dialog.as_ref().unwrap().inputs[1].items.is_empty());
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items.len(), 2);
    pick(&mut h, [0.0, 20.0], false);
    preview(&mut h, 2);
    step(&mut h, json!({"key":"Escape"}));
    h.frames(3);
    assert!(h.app.dialog.is_none());
    assert!(!h.app.preview.active);
    assert_eq!(h.app.session.doc.to_json(), original);
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
fn sketch_dialog_patterns_update_counts_and_place_an_unsnapped_centre() {
    let mut h = fixture();
    // Keep the picked geometry clear of the two-column rectangular-pattern form.
    step(&mut h, json!({"call":"ui.camera","params":{"target":[-60,0,0],"distance":120}}));
    step(&mut h, json!({"start":"sketch.pattern.rectangular"}));
    pick(&mut h, [-22.5, 10.0], false);
    if let crate::dialogs::Kind::SketchModify(Mod::PatternRect { count, count2, .. }) = &mut h.app.dialog.as_mut().unwrap().kind {
        *count = "4".into();
        *count2 = "2".into();
    }
    h.frames(3);
    preview(&mut h, 7);
    step(&mut h, json!({"click":{"handle":"dialog:OK"}}));
    assert_eq!(h.app.session.world_state().sketch(1).unwrap().sketch.curves.len(), 10);
    step(&mut h, json!({"start":"sketch.pattern.circular"}));
    pick(&mut h, [-22.5, 10.0], false);
    step(&mut h, json!({"call":"ui.dialogInput","params":{"index":1}}));
    pick(&mut h, [-40.0, -20.0], false);
    if let crate::dialogs::Kind::SketchModify(Mod::PatternCirc { count, angle, .. }) = &mut h.app.dialog.as_mut().unwrap().kind {
        *count = "4".into();
        *angle = "180 deg".into();
    }
    h.frames(3);
    preview(&mut h, 3);
    step(&mut h, json!({"click":{"handle":"dialog:OK"}}));
    assert_eq!(h.app.session.world_state().sketch(1).unwrap().sketch.curves.len(), 13);
}

#[test]
fn sketch_dialog_offset_precise_distance_flip_and_editable_dimension() {
    let mut h = fixture();
    // The floating browser shifts the dialog over the source curve at the default camera.
    step(&mut h, json!({"call":"ui.camera","params":{"target":[-40,0,0],"distance":150}}));
    step(&mut h, json!({"start":"sketch.offset"}));
    pick(&mut h, [-22.5, 10.0], false);
    if let crate::dialogs::Kind::SketchModify(Mod::Offset { distance, flip, chain, .. }) = &mut h.app.dialog.as_mut().unwrap().kind {
        *distance = "2 mm".into();
        *flip = true;
        *chain = false;
    }
    h.frames(3);
    preview(&mut h, 1);
    step(&mut h, json!({"click":{"handle":"dialog:OK"}}));
    let param = h.app.session.doc.params.iter().find(|p| p.name.starts_with('d')).unwrap().name.clone();
    step(&mut h, json!({"call":"engine.execute","params":{"command":"parameters.change","params":{"name":param,"expression":"3 mm"}}}));
    let state = h.app.session.world_state();
    let sk = &state.sketch(1).unwrap().sketch;
    assert_eq!(sk.offsets[0].distance, -3.0);
    assert!(sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::OffsetDistance { .. })));
}

#[test]
fn sketch_dialog_mirror_refuses_external_and_curved_references() {
    let mut h = fixture();
    h.app.start("sketch.mirror");
    let d = h.app.dialog.as_mut().unwrap();
    d.active = 1;
    assert!(candidate(d, &h.app.session, &Hit::SketchCurve { sketch: 2, id: "l1".into(), straight: true, point: Vec3::ZERO }).is_none());
    assert!(candidate(d, &h.app.session, &Hit::SketchCurve { sketch: 1, id: "c1".into(), straight: false, point: Vec3::ZERO }).is_none());
}

#[test]
fn sketch_dialog_box_selection_adds_replaces_and_preserves_reference() {
    let mut h = fixture();
    // Put every selection box between the left dialog and the right Sketch Palette.
    step(&mut h, json!({"call":"ui.camera","params":{"target":[-40,0,0],"distance":150}}));
    step(&mut h, json!({"call":"engine.execute","params":{"command":"sketch.line","params":{"points":[[-50,-20],[-40,-20]]}}}));
    step(&mut h, json!({"start":"sketch.mirror"}));
    step(&mut h, json!({"call":"ui.dialogInput","params":{"index":1}}));
    pick(&mut h, [0.0, 20.0], false);
    step(&mut h, json!({"call":"ui.dialogInput","params":{"index":0}}));
    step(&mut h, json!({"drag":[{"sketch":[-32,8]},{"sketch":[-13,27]}]}));
    let sources = |h: &Harness| {
        h.app.dialog.as_ref().unwrap().inputs[0]
            .items
            .iter()
            .filter_map(|s| if let Sel::SketchCurve { id, .. } = s { Some(id.clone()) } else { None })
            .collect::<Vec<_>>()
    };
    assert_eq!(sources(&h), vec!["l1", "l2"]);
    step(&mut h, json!({"drag":[{"sketch":[-52,-22]},{"sketch":[-38,-18]}],"shift":true}));
    assert_eq!(sources(&h), vec!["l1", "l2", "l4"]);
    step(&mut h, json!({"drag":[{"sketch":[-32,8]},{"sketch":[-13,12]}]}));
    assert_eq!(sources(&h), vec!["l1"]);
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[1].items, vec![Sel::SketchCurve { id: "l3".into(), sketch: None }]);
    step(&mut h, json!({"key":"Escape"}));
    step(&mut h, json!({"start":"sketch.offset"}));
    step(&mut h, json!({"drag":[{"sketch":[-32,8]},{"sketch":[-13,27]}]}));
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items.len(), 2, "Offset accepts curves, not their endpoints or dimensions");
}

#[test]
fn sketch_modify_inputs_refuse_other_sketches_without_changing_hole_points() {
    let mut h = fixture();
    for command in ["sketch.mirror", "sketch.pattern.rectangular", "sketch.pattern.circular", "sketch.offset", "sketch.move", "sketch.scale"] {
        h.app.start(command);
        let d = h.app.dialog.as_ref().unwrap();
        assert!(candidate(d, &h.app.session, &Hit::SketchCurve { sketch: 2, id: "l1".into(), straight: true, point: Vec3::ZERO }).is_none());
        assert!(candidate(d, &h.app.session, &Hit::SketchPoint { sketch: 2, id: "p1".into(), at: solidcraft_engine::geom::Vec2::ZERO }).is_none());
        if command != "sketch.offset" {
            assert!(d.inputs[0].accept & crate::selection::SKETCH_POINTS != 0);
            assert_eq!(d.inputs[0].accept & crate::selection::POINTS, 0);
        }
    }
    let input = crate::selection::SelInput::new("Hole points", crate::selection::POINTS, true);
    assert_eq!(
        input.accepts(&Hit::SketchPoint { sketch: 2, id: "p1".into(), at: solidcraft_engine::geom::Vec2::ZERO }, None, |_, _| false),
        Some(Sel::SketchPoint { id: "2:p1".into() })
    );
    h.app.start("sketch.pattern.circular");
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[0].items = vec![Sel::SketchCurve { id: "l1".into(), sketch: None }];
    d.inputs[1].items = vec![Sel::SketchPoint { id: "2:p1".into() }];
    assert!(crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).is_err());
}

#[test]
fn sketch_preselected_points_toggle_once_and_empty_shift_box_keeps_reference() {
    let mut h = fixture();
    let state = h.app.session.world_state();
    let point = state.sketch(1).unwrap().sketch.points[1].id.clone();
    h.app.session.selection = vec![Sel::SketchPoint { id: point.clone() }];
    h.app.start("sketch.mirror");
    let d = h.app.dialog.as_mut().unwrap();
    assert_eq!(d.inputs[0].items, vec![Sel::SketchPoint { id: point.clone() }]);
    d.active = 0;
    let selected = candidate(d, &h.app.session, &Hit::SketchPoint { sketch: 1, id: point, at: solidcraft_engine::geom::Vec2::ZERO }).unwrap();
    d.pick(&h.app.session, selected);
    assert!(d.inputs[0].items.is_empty(), "clicking a preselection removes it instead of duplicating it");
    d.active = 1;
    let reference = Sel::SketchCurve { id: "l3".into(), sketch: None };
    d.pick(&h.app.session, reference.clone());
    d.take_box(&h.app.session, Vec::new(), true);
    assert_eq!(d.inputs[1].items, vec![reference]);
    d.take_box(&h.app.session, vec![Sel::SketchCurve { id: "l1".into(), sketch: None }, Sel::SketchCurve { id: "l2".into(), sketch: None }], true);
    assert_eq!(d.inputs[1].items, vec![Sel::SketchCurve { id: "l1".into(), sketch: None }], "a box replaces a single reference with one item");
}

#[test]
fn sketch_modify_previews_use_the_active_occurrences_world_placement() {
    let mut h = Harness::new();
    let part = h.app.run("component.create", json!({"name":"Part","grounded":false})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    let sid = h.app.session.active_sketch.unwrap();
    h.app.run("sketch.line", json!({"points":[[-30,0],[-20,0]]})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,-10],[0,10]],"construction":true})).unwrap();
    h.app
        .run("occurrence.move", json!({"occurrence":part["occurrence"],"translate":[100,30,20],"axis":[0,0,1],"angle":"35 deg","capture":false}))
        .unwrap();
    let original = h.app.session.doc.to_json();
    let colors = crate::preview::Colors::from_tokens(&crate::theme::Tokens::get());
    for (command, params) in [
        ("sketch.mirror", json!({"entities":["l1"],"line":"l2"})),
        ("sketch.pattern.rectangular", json!({"entities":["l1"],"count":3,"spacing":10})),
        ("sketch.pattern.circular", json!({"entities":["l1"],"center":[0,0],"count":4})),
        ("sketch.offset", json!({"curves":["l1"],"distance":2,"chain":false})),
    ] {
        let built = crate::preview::compute(&h.app.session, &[(command.into(), params.clone())], colors).unwrap();
        let mut scratch = h.app.session.scratch();
        scratch.execute(command, &params).unwrap();
        let state = scratch.world_state();
        let expected = state.sketch(sid).unwrap().plane;
        assert!(expected.origin.len() > 50.0, "the fixture must exercise a moved occurrence");
        assert_eq!(built.sketches.len(), 1);
        assert!(!built.sketches[0].curves.is_empty());
        assert_eq!(built.sketches[0].sketch.plane, expected, "{command}");
        assert_eq!(h.app.session.doc.to_json(), original);
    }
}

#[test]
fn sketch_corner_preselected_point_uses_the_same_identity_as_corner_picking() {
    let mut h = fixture();
    let state = h.app.session.world_state();
    let sk = &state.sketch(1).unwrap().sketch;
    let index = sk.resolve_point("l1.end").unwrap();
    let point = sk.points[index].id.clone();
    h.app.session.selection = vec![Sel::SketchPoint { id: point }];
    h.app.start("sketch.fillet");
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items.len(), 1);
    pick(&mut h, [-15.0, 10.0], true);
    assert!(h.app.dialog.as_ref().unwrap().inputs[0].items.is_empty());
}

#[test]
fn sketch_modify_keeps_custom_point_ids_that_contain_colons() {
    let mut h = fixture();
    h.app.run("sketch.point", json!({"point":[-40,0],"id":"2:centre"})).unwrap();
    h.app.start("sketch.pattern.circular");
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[0].items = vec![Sel::SketchCurve { id: "l1".into(), sketch: None }];
    d.inputs[1].items = vec![Sel::SketchPoint { id: "2:centre".into() }];
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].1["center"], "2:centre");
    for (command, params) in commands {
        h.app.run(&command, params).unwrap();
    }
    assert_eq!(h.app.session.world_state().sketch(1).unwrap().sketch.curves.len(), 8);
}

fn tracking_fixture() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.preferences.look_at_anim = false;
    for (command, params) in [
        ("sketch.create", json!({"plane":"XY"})),
        ("sketch.rectangle.two_point", json!({"p0":[-30,-20],"p1":[30,20]})),
        ("sketch.circle.center", json!({"center":[-10,3],"radius":7})),
    ] {
        h.app.run(command, params).unwrap();
    }
    h.call("ui.view", json!({"view":"top"}));
    h.call("ui.camera", json!({"target":[-30,0,0],"distance":180}));
    h.frames(3);
    h
}

#[test]
fn centre_and_midpoint_tracking_is_visual_in_idle_and_drawing_modes() {
    use solidcraft_engine::geom::Vec2;
    let mut h = tracking_fixture();
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    for tool in [None, Some("sketch.line"), Some("sketch.circle.center")] {
        if let Some(tool) = tool {
            h.app.start(tool);
        }
        for (anchor, end) in [([-10.0, 3.0], [20.0, 3.0]), ([0.0, 20.0], [0.0, -10.0]), ([30.0, 0.0], [-10.0, 0.0])] {
            step(&mut h, json!({"move":{"sketch":anchor}}));
            step(&mut h, json!({"move":{"sketch":end}}));
            let (a, b) = h.app.viewport.tracking.guide.expect("centre/MID guide");
            assert!(a.dist(Vec2::new(anchor[0], anchor[1])) < 1e-4);
            assert!(b.dist(Vec2::new(end[0], end[1])) < 1e-4);
            step(&mut h, json!({"move":{"sketch":[15,12]}}));
            assert!(h.app.viewport.tracking.guide.is_none(), "off-axis hides the guide");
        }
        step(&mut h, json!({"key":"Escape"}));
    }
    assert_eq!(h.app.session.doc.to_json(), before, "guides add no geometry or constraints");
    assert_eq!(h.app.session.undo.len(), undo);
}

/// #9105: while a drawing tool waits for a point, a rectangle corner starts a guide as a
/// midpoint does; idle hovering and the preference turned off keep the old centre/MID-only guides.
#[test]
fn a_corner_starts_a_guide_while_drawing_unless_turned_off() {
    use solidcraft_engine::geom::Vec2;
    let mut h = tracking_fixture();
    assert!(h.app.preferences.sketch_tracking_points, "on by default");
    let before = h.app.session.doc.to_json();
    let corner = Vec2::new(30.0, 20.0);
    let hover_corner_then_level = |h: &mut Harness| {
        step(h, json!({"move":{"sketch":[30,20]}}));
        step(h, json!({"move":{"sketch":[45,20]}}));
        h.app.viewport.tracking.guide
    };
    assert!(hover_corner_then_level(&mut h).is_none(), "idle: a corner is no reference");
    for tool in ["sketch.line", "sketch.rectangle.two_point", "sketch.circle.center"] {
        h.app.start(tool);
        let (a, b) = hover_corner_then_level(&mut h).unwrap_or_else(|| panic!("{tool}: a guide from the corner"));
        assert!(a.dist(corner) < 1e-9 && b.dist(Vec2::new(45.0, 20.0)) < 1e-4, "{tool}: {a:?} {b:?}");
        step(&mut h, json!({"key":"Escape"}));
    }
    h.call("ui.prefs", json!({"sketch_tracking_points": false}));
    h.app.start("sketch.line");
    assert!(hover_corner_then_level(&mut h).is_none(), "the preference keeps the old guides");
    step(&mut h, json!({"move":{"sketch":[0,20]}}));
    step(&mut h, json!({"move":{"sketch":[0,-10]}}));
    assert!(h.app.viewport.tracking.guide.is_some(), "midpoints still guide");
    step(&mut h, json!({"key":"Escape"}));
    assert_eq!(h.app.session.doc.to_json(), before, "guides add no geometry or constraints");
}

#[test]
fn two_recent_midpoints_snap_where_their_guides_meet() {
    use solidcraft_engine::geom::Vec2;
    let mut h = tracking_fixture();
    // Grid snap at this zoom already rounds a nearby cursor onto the origin. Snap off leaves
    // the cursor where it is, so only the remembered crossing can pull it to (0, 0).
    h.app.run("sketch.options", json!({"snap": false})).unwrap();
    h.app.start("sketch.line");
    // Top midpoint (0, 20) and the circle centre (-10, 3) meet at (0, 3), which is not the
    // model origin and not a sketch point. A cursor a fraction of a millimetre away stays put
    // until both references have been hovered.
    let cross = Vec2::new(0.0, 3.0);
    let near = json!({"sketch":[0.4,3.3]});
    step(&mut h, json!({"move":{"sketch":[0,20]}}));
    let [x, y] = h.at(&near).unwrap();
    let pos = egui::pos2(x as f32, y as f32);
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.expect("viewport"));
    let (at, _) = crate::viewport::sketch_point_at(&h.app, &proj, pos).unwrap();
    assert!(at.dist(cross) > 0.2, "one reference does not pin both coordinates: {at:?}");
    step(&mut h, json!({"move":{"sketch":[-10,3]}}));
    step(&mut h, json!({"move":{"sketch":[0.4,3.3]}}));
    let (a, b) = h.app.viewport.tracking.guide.expect("current guide");
    let (c, d) = h.app.viewport.tracking.remembered_guide.expect("remembered guide");
    assert!(b.dist(cross) < 1e-6 && d.dist(cross) < 1e-6, "{a:?} {b:?} {c:?} {d:?}");
    let [x, y] = h.at(&near).unwrap();
    let pos = egui::pos2(x as f32, y as f32);
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.expect("viewport"));
    let (at, _) = crate::viewport::sketch_point_at(&h.app, &proj, pos).unwrap();
    assert!(at.dist(cross) > 0.2, "the crossing snap is off until asked for: {at:?}");
    h.call("ui.prefs", json!({"sketch_recent_snap": true}));
    let (at, id) = crate::viewport::sketch_point_at(&h.app, &proj, pos).unwrap();
    assert!(id.is_none(), "{id:?}");
    assert!(at.dist(cross) < 1e-6, "{at:?}");
    crate::tool_modifiers::set_target_midpoint(true);
    let shifted = crate::viewport::sketch_point_at(&h.app, &proj, pos);
    crate::tool_modifiers::set_target_midpoint(false);
    let (at, _) = shifted.unwrap();
    assert!(at.dist(cross) > 0.2, "shift midpoint mode skips the crossing: {at:?}");
    step(&mut h, json!({"click":{"sketch":[0.4,3.3]}}));
    let placed = h.app.tool.as_ref().unwrap().pts[0].0;
    assert!(placed.dist(cross) < 1e-6, "{placed:?}");
    let curves = h.app.session.world_state().sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch.curves.len();
    assert_eq!(curves, 5, "the snap adds no construction geometry");
    h.call("ui.prefs", json!({"sketch_tracking_guides":false}));
    h.frames(2);
    let (at, _) = crate::viewport::sketch_point_at(&h.app, &proj, pos).unwrap();
    assert!(at.dist(cross) > 0.2, "the preference keeps the old cursor: {at:?}");
}

#[test]
fn tracking_clears_on_escape_sketch_exit_and_preference_and_respects_suppression() {
    let mut h = tracking_fixture();
    for action in ["suppression", "preference", "escape", "exit"] {
        step(&mut h, json!({"move":{"sketch":[-10,3]}}));
        step(&mut h, json!({"move":{"sketch":[20,3]}}));
        assert!(h.app.viewport.tracking.guide.is_some());
        match action {
            "suppression" => step(&mut h, json!({"move":{"sketch":[20,3]},"alt":true})),
            "preference" => {
                h.call("ui.prefs", json!({"sketch_tracking_guides":false}));
                h.frames(2);
            }
            "escape" => step(&mut h, json!({"key":"Escape"})),
            _ => {
                h.app.run("sketch.finish", json!({})).unwrap();
                h.frames(2);
            }
        }
        assert!(h.app.viewport.tracking.guide.is_none(), "{action}");
        if action == "exit" {
            break;
        }
        if action == "preference" {
            h.call("ui.prefs", json!({"sketch_tracking_guides":true}));
        }
        step(&mut h, json!({"move":{"sketch":[15,12]}}));
    }
}

#[test]
fn tracking_does_not_leak_outside_the_viewport_or_into_another_sketch() {
    let mut h = tracking_fixture();
    step(&mut h, json!({"move":{"sketch":[-10,3]}}));
    step(&mut h, json!({"move":{"sketch":[20,3]}}));
    assert!(h.app.viewport.tracking.guide.is_some());
    h.call("ui.move", json!({"x":0,"y":0}));
    assert!(h.app.viewport.tracking.guide.is_none());
    step(&mut h, json!({"move":{"sketch":[20,3]}}));
    assert!(h.app.viewport.tracking.guide.is_none(), "leaving requires acquiring a reference again");
    step(&mut h, json!({"move":{"sketch":[-10,3]}}));
    step(&mut h, json!({"move":{"sketch":[20,3]}}));
    assert!(h.app.viewport.tracking.guide.is_some());
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.frames(3);
    assert!(h.app.viewport.tracking.guide.is_none());
}

}

#[test]
fn dialog_tab_mirror_advances_from_canvas_and_reverses_without_changing_picks() {
    let mut h = fixture();
    step(&mut h, json!({"start":"sketch.mirror"}));
    pick(&mut h, [-22.5, 10.0], false);
    pick(&mut h, [-15.0, 17.5], false);
    step(&mut h, json!({"key":"Tab"}));
    assert_eq!(h.app.dialog.as_ref().unwrap().active, 1);
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items.len(), 2);
    pick(&mut h, [0.0, 20.0], false);
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[1].items.len(), 1);
    step(&mut h, json!({"key":"Tab", "shift":true}));
    assert_eq!(h.app.dialog.as_ref().unwrap().active, 0);
    pick(&mut h, [-22.5, 10.0], true);
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items.len(), 1);
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[1].items.len(), 1);
}

#[test]
fn dialog_tab_patterns_follow_display_order_in_both_directions() {
    for command in ["sketch.pattern.rectangular", "sketch.pattern.circular"] {
        let mut h = fixture();
        step(&mut h, json!({"call":"ui.camera","params":{"target":[-60,0,0],"distance":120}}));
        step(&mut h, json!({"start":command}));
        pick(&mut h, [-22.5, 10.0], false);
        step(&mut h, json!({"key":"Tab"}));
        assert_eq!(h.app.dialog.as_ref().unwrap().active, 1, "{command}");
        if command == "sketch.pattern.rectangular" {
            step(&mut h, json!({"key":"Tab"}));
            assert_eq!(h.app.dialog.as_ref().unwrap().active, 2);
            step(&mut h, json!({"key":"Tab"})); // First direction section header.
        }
        step(&mut h, json!({"key":"Tab"}));
        step(&mut h, json!({"key":"A", "cmd":true}));
        step(&mut h, json!({"text":"4"}));
        let d = h.app.dialog.as_ref().unwrap();
        match &d.kind {
            crate::dialogs::Kind::SketchModify(Mod::PatternRect { count, .. } | Mod::PatternCirc { count, .. }) => assert_eq!(count, "4"),
            _ => panic!("pattern dialog"),
        }
        if command == "sketch.pattern.rectangular" {
            step(&mut h, json!({"key":"Tab", "shift":true})); // First direction section header.
            step(&mut h, json!({"key":"Tab", "shift":true}));
            assert_eq!(h.app.dialog.as_ref().unwrap().active, 2);
        }
        step(&mut h, json!({"key":"Tab", "shift":true}));
        assert_eq!(h.app.dialog.as_ref().unwrap().active, 1);
        step(&mut h, json!({"key":"Tab", "shift":true}));
        assert_eq!(h.app.dialog.as_ref().unwrap().active, 0);
        if command == "sketch.pattern.rectangular" {
            // Entities, both directions, then each section header and its values.
            for _ in 0..6 {
                step(&mut h, json!({"key":"Tab"}));
            }
            assert_eq!(h.app.dialog.as_ref().unwrap().active, 2);
            step(&mut h, json!({"key":"Tab"}));
            step(&mut h, json!({"key":"A", "cmd":true}));
            step(&mut h, json!({"text":"2"}));
            let crate::dialogs::Kind::SketchModify(Mod::PatternRect { count2, .. }) = &h.app.dialog.as_ref().unwrap().kind else {
                panic!("rectangular pattern dialog");
            };
            assert_eq!(count2, "2");
            step(&mut h, json!({"key":"Tab", "shift":true}));
            assert_eq!(h.app.dialog.as_ref().unwrap().active, 2);
            step(&mut h, json!({"key":"Tab", "shift":true}));
            step(&mut h, json!({"key":"A", "cmd":true}));
            step(&mut h, json!({"text":"25 mm"}));
            let crate::dialogs::Kind::SketchModify(Mod::PatternRect { spacing, .. }) = &h.app.dialog.as_ref().unwrap().kind else {
                panic!("rectangular pattern dialog");
            };
            assert_eq!(spacing, "25 mm");
        }
    }
}

#[test]
fn dialog_tab_does_not_intercept_modified_tabs_or_collapsed_dialogs() {
    let mut h = fixture();
    step(&mut h, json!({"start":"sketch.mirror"}));
    pick(&mut h, [-22.5, 10.0], false);
    for modifiers in [egui::Modifiers::CTRL, egui::Modifiers::ALT, egui::Modifiers::COMMAND] {
        for pressed in [true, false] {
            h.app.synthetic.push(egui::Event::Key { key: egui::Key::Tab, physical_key: None, pressed, repeat: false, modifiers });
        }
        h.frames(3);
        assert_eq!(h.app.dialog.as_ref().unwrap().active, 0);
    }
    h.app.dialog.as_mut().unwrap().collapsed = true;
    h.frames(3);
    step(&mut h, json!({"key":"Tab"}));
    assert_eq!(h.app.dialog.as_ref().unwrap().active, 0);
    h.app.dialog.as_mut().unwrap().collapsed = false;
    step(&mut h, json!({"key":"Escape"}));
    assert!(h.app.dialog.is_none());
}

#[test]
fn dialog_tab_enter_on_mirror_selection_applies_the_dialog() {
    let mut h = fixture();
    step(&mut h, json!({"start":"sketch.mirror"}));
    pick(&mut h, [-22.5, 10.0], false);
    pick(&mut h, [-15.0, 17.5], false);
    step(&mut h, json!({"key":"Tab"}));
    pick(&mut h, [0.0, 20.0], false);
    step(&mut h, json!({"key":"Tab", "shift":true}));
    step(&mut h, json!({"key":"Tab"}));
    assert_eq!(h.app.dialog.as_ref().unwrap().active, 1);
    step(&mut h, json!({"key":"Space"}));
    assert!(h.app.dialog.is_some(), "Space activates the selection without applying the dialog");
    step(&mut h, json!({"key":"Enter"}));
    assert!(h.app.dialog.is_none(), "Enter on the Mirror line selection must apply the dialog");
    assert_eq!(h.app.session.world_state().sketch(1).unwrap().sketch.curves.len(), 5);
}

/// Lining up while drawing (#7828), turned on (it is off by default): a fixture whose points sit
/// off the grid, so that only lining up can place a point exactly on them, seen from the top at
/// about 1 mm of snap tolerance.
fn lining_up_fixture(points: &[(&str, Value)]) -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.preferences.look_at_anim = false;
    assert!(!h.app.preferences.sketch_align_points, "off unless turned on");
    h.app.preferences.sketch_align_points = true;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    for (command, params) in points {
        h.app.run(command, params.clone()).unwrap();
    }
    h.call("ui.view", json!({"view":"top"}));
    h.call("ui.camera", json!({"target":[25,0,0],"distance":180}));
    h.frames(3);
    h
}

/// The active sketch's point at `at`, and the points level (`Horizontal`) or plumb (`Vertical`)
/// constraints tie it to.
fn point_and_alignments(h: &Harness, at: [f64; 2]) -> Option<(String, Vec<(&'static str, String)>)> {
    use solidcraft_engine::sketch::ConstraintKind::{HorizontalPoints, VerticalPoints};
    let st = h.app.session.world_state();
    let sk = &st.sketch(h.app.session.active_sketch?)?.sketch;
    let at = solidcraft_engine::geom::Vec2::new(at[0], at[1]);
    let i = sk.points.iter().position(|p| p.pos.dist(at) < 1e-9)?;
    let id = |k: usize| sk.points.get(k).map(|p| p.id.clone()).unwrap_or_default();
    let tied = sk
        .constraints
        .iter()
        .filter_map(|c| match c.kind {
            HorizontalPoints { p, q } if p == i || q == i => Some(("Horizontal", id(if p == i { q } else { p }))),
            VerticalPoints { p, q } if p == i || q == i => Some(("Vertical", id(if p == i { q } else { p }))),
            _ => None,
        })
        .collect();
    Some((id(i), tied))
}

#[test]
fn line_up_takes_the_nearest_point_on_each_axis_on_its_own() {
    use crate::sketch_tools::{Lined, line_up};
    use solidcraft_engine::geom::Vec2;
    let points = [(Some("a".to_string()), Vec2::new(0.0, 10.4)), (Some("b".to_string()), Vec2::new(50.3, -30.0)), (None, Vec2::new(5.0, 10.2))];
    let (level, plumb) = line_up(&points, Vec2::new(50.0, 10.0), 0.5);
    assert_eq!(level, Some(Lined { id: None, at: Vec2::new(5.0, 10.2), horizontal: true }), "the nearer of the two level points");
    assert_eq!(plumb, Some(Lined { id: Some("b".into()), at: Vec2::new(50.3, -30.0), horizontal: false }));
    // Right on a point is a point snap's business, not lining up; far from all, nothing.
    assert_eq!(line_up(&points, Vec2::new(50.3, -30.1), 0.5), (None, None));
    assert_eq!(line_up(&points, Vec2::new(25.0, -10.0), 0.5), (None, None));
}

/// #8089, a duplicate of #7828: a rectangle drawn beside another takes its height from one
/// rectangle and its width from a point below, both at once, and keeps both alignments, in the
/// rectangle's one undo step. With the preference off, only one axis lines up, as before.
#[test]
fn a_rectangle_corner_lines_up_with_two_points_at_once_and_keeps_both() {
    for aligning in [true, false] {
        let mut h = lining_up_fixture(&[
            ("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20.37,10.43]})),
            ("sketch.point", json!({"point":[50.37,-30.21]})),
        ]);
        h.app.preferences.sketch_align_points = aligning;
        let undo = h.app.session.undo.len();
        let below = point_and_alignments(&h, [50.37, -30.21]).unwrap().0;
        step(&mut h, json!({"start":"sketch.rectangle.two_point"}));
        step(&mut h, json!({"click":{"sketch":[30,0.3]}}));
        step(&mut h, json!({"move":{"sketch":[50.2,10.6]}}));
        if aligning {
            step(&mut h, json!({"expect":{"count":{"name":"sketch_align_guides","value":2}}}));
        }
        step(&mut h, json!({"click":{"sketch":[50.2,10.6]}}));
        let corner = point_and_alignments(&h, [50.37, 10.43]);
        if !aligning {
            assert!(corner.is_none(), "off: the corner follows the grid, not the other points");
            continue;
        }
        let (_, tied) = corner.expect("the corner lands level with the rectangle's top and plumb with the point");
        assert!(tied.iter().any(|(kind, to)| *kind == "Vertical" && *to == below), "{tied:?}");
        assert!(tied.iter().any(|(kind, _)| *kind == "Horizontal"), "{tied:?}");
        let (_, start) = point_and_alignments(&h, [30.0, 0.0]).expect("the first corner lines up with the bottom");
        assert!(start.iter().any(|(kind, _)| *kind == "Horizontal"), "{start:?}");
        assert_eq!(h.app.session.undo.len(), undo + 1, "the alignments are in the rectangle's undo step");
    }
}

/// #7828: a line kept horizontal from its start also lines up under a point above, and the
/// click keeps both: the line's Horizontal and the end's alignment with that point.
#[test]
fn a_horizontal_line_ends_plumb_with_a_point_above_and_keeps_both() {
    let mut h = lining_up_fixture(&[("sketch.point", json!({"point":[20.37,30.13]}))]);
    let above = point_and_alignments(&h, [20.37, 30.13]).unwrap().0;
    step(&mut h, json!({"start":"sketch.line"}));
    step(&mut h, json!({"click":{"sketch":[0,0]}}));
    step(&mut h, json!({"move":{"sketch":[20.2,0.15]}}));
    step(&mut h, json!({"expect":{"count":{"name":"sketch_align_guides","value":1}}}));
    step(&mut h, json!({"click":{"sketch":[20.2,0.15]}}));
    step(&mut h, json!({"key":"Escape"}));
    let (_, tied) = point_and_alignments(&h, [20.37, 0.0]).expect("the end is level with the start and plumb with the point");
    assert_eq!(tied, [("Vertical", above)]);
    let st = h.app.session.world_state();
    let sk = &st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    assert!(
        sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::Horizontal { .. })),
        "the line stays horizontal"
    );
}
