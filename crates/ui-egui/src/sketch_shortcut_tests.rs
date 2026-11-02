use crate::scenario::Harness;
use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::{FeatureKind, PlaneRef};

fn tool(h: &Harness) -> Option<&str> {
    h.app.tool.as_ref().map(|t| t.cmd)
}

fn normal(h: &Harness) -> solidcraft_engine::geom::Vec3 {
    let active = h.app.session.active_sketch;
    h.app.session.world_state().sketches.iter().find(|s| Some(s.feature) == active).map(|s| s.plane.normal()).unwrap_or_default()
}

fn press(h: &mut Harness, key: &str) {
    h.step(&json!({"key": key})).unwrap();
    h.settle();
}

#[test]
fn spline_undo_removes_pending_points_before_sketch_history() {
    for cmd in ["sketch.spline.fit_point", "sketch.spline.control_point", "sketch.spline.control_point_5"] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[-20,-20],"p1":[-10,-10]})).unwrap();
        h.settle();
        h.step(&json!({"start":cmd})).unwrap();
        for point in [[2, 2, 0], [8, 2, 0], [9, 7, 0]] {
            h.step(&json!({"click":{"world":point}})).unwrap();
        }
        let points = h.app.tool.as_ref().unwrap().pts.clone();
        assert_eq!(points.len(), 3);
        let before = (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.revision);
        for remaining in (0..3).rev() {
            h.step(&json!({"key":"Z","cmd":true})).unwrap();
            h.settle();
            assert_eq!(h.app.tool.as_ref().unwrap().pts, points[..remaining], "{cmd}");
            assert_eq!((h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.revision), before, "{cmd}");
        }
        assert_eq!(tool(&h), Some(cmd));
        h.step(&json!({"key":"Z","cmd":true})).unwrap();
        h.settle();
        assert_eq!(h.app.session.undo.len(), before.1 - 1, "history resumes once the spline is empty");
        let sketch = h.app.session.active_sketch.unwrap();
        assert!(h.app.session.model.state().sketch(sketch).unwrap().sketch.curves.is_empty());
    }
}

#[test]
fn spline_backspace_removes_pending_points_and_keeps_the_tool_ready() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[-20,-20],"p1":[-10,-10]})).unwrap();
    h.settle();
    h.step(&json!({"start":"sketch.spline.fit_point"})).unwrap();
    for point in [[2, 2, 0], [8, 2, 0], [9, 7, 0]] {
        h.step(&json!({"click":{"world":point}})).unwrap();
    }
    let points = h.app.tool.as_ref().unwrap().pts.clone();
    assert_eq!(points.len(), 3);
    let before = (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.revision);
    for remaining in (0..3).rev() {
        press(&mut h, "Backspace");
        assert_eq!(h.app.tool.as_ref().unwrap().pts, points[..remaining]);
        assert_eq!((h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.revision), before);
    }
    assert_eq!(tool(&h), Some("sketch.spline.fit_point"));
    press(&mut h, "Escape");
    assert!(tool(&h).is_none());
    assert_eq!((h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.revision), before);
}

#[test]
fn sketch_keys_outside_a_sketch_sketch_on_the_selected_plane_and_start_their_tool() {
    for (key, cmd) in
        [("C", "sketch.circle.center"), ("L", "sketch.line"), ("R", "sketch.rectangle.two_point"), ("D", "sketch.dimension"), ("P", "sketch.project")]
    {
        let mut h = Harness::new();
        h.app.session.selection = vec![Sel::Plane { name: "XZ".into() }];
        press(&mut h, key);
        assert!(h.app.session.active_sketch.is_some(), "{key}: no sketch opened");
        assert_eq!(h.app.session.world_state().sketches.len(), 1, "{key}");
        assert!(normal(&h).y.abs() > 0.999, "{key}: not on XZ: {:?}", normal(&h));
        if cmd == "sketch.project" {
            assert!(h.app.preferences.project_options_dialog, "{key}: options are enabled by default");
            assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::SketchProject { .. })), "{key}");
            assert!(tool(&h).is_none(), "{key}: Project should open its options dialog");
        } else {
            assert_eq!(tool(&h), Some(cmd), "{key}");
            assert!(h.app.dialog.is_none(), "{key}: dialog left open");
        }
    }
}

#[test]
fn project_can_keep_its_previous_direct_pick_workflow() {
    let mut h = Harness::new();
    h.app.preferences.project_options_dialog = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.start("sketch.project");
    h.settle();
    assert!(h.app.dialog.is_none());
    assert_eq!(tool(&h), Some("sketch.project"));
}

#[test]
fn a_sketch_key_sketches_on_the_selected_planar_face() {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.ui.pick_bodies = false;
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"BaseBox"})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.settle();
    h.step(&json!({"click":{"world":[3,4,10]}})).unwrap();
    assert!(matches!(h.app.session.selection.as_slice(), [Sel::Face { body, .. }] if body == "BaseBox"), "{:?}", h.app.session.selection);
    press(&mut h, "C");
    let sketch = h.app.session.active_sketch.expect("sketch opened on the face");
    let FeatureKind::Sketch { plane: PlaneRef::Face { anchor: Some(anchor), .. }, .. } = &h.app.session.doc.feature(sketch).unwrap().kind else {
        panic!("the sketch must sit on the picked face");
    };
    assert_eq!(anchor.target.body, "BaseBox");
    assert_eq!(tool(&h), Some("sketch.circle.center"));
}

#[test]
fn a_sketch_key_with_nothing_selected_asks_for_the_plane_then_starts_the_tool() {
    let mut h = Harness::new();
    press(&mut h, "C");
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::Sketch)));
    assert!(h.app.session.active_sketch.is_none());
    assert!(tool(&h).is_none());
    h.step(&json!({"click":{"plane":"XY"}})).unwrap();
    h.settle();
    assert!(normal(&h).z.abs() > 0.999, "{:?}", normal(&h));
    assert_eq!(tool(&h), Some("sketch.circle.center"));
    assert!(h.app.dialog.is_none());
}

/// #1804: Escape puts down the sketch tool. Another Escape, on its own, does not leave the
/// sketch; only a quick second press does (and finishes it, keeping it).
#[test]
fn a_second_escape_stays_in_the_sketch() {
    // Presses further apart than the double-press window.
    let pause = |h: &mut Harness| h.frames((crate::toolbar::ESCAPE_TWICE_S * 60.0) as usize + 6);
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    let sketch = h.app.session.active_sketch;
    h.app.start("sketch.line");
    h.settle();
    assert_eq!(tool(&h), Some("sketch.line"));
    press(&mut h, "Escape");
    assert!(tool(&h).is_none(), "the first Escape puts the tool down");
    assert_eq!(h.app.session.active_sketch, sketch);
    pause(&mut h);
    press(&mut h, "Escape");
    assert_eq!(h.app.session.active_sketch, sketch, "a second Escape stays in the sketch");
    h.app.run("sketch.line", json!({"points":[[0,0],[10,0]]})).unwrap();
    h.app.session.selection = vec![Sel::SketchCurve { id: "l1".into(), sketch: None }];
    pause(&mut h);
    press(&mut h, "Escape");
    assert!(h.app.session.selection.is_empty(), "Escape still clears a selection");
    assert_eq!(h.app.session.active_sketch, sketch);
    pause(&mut h);
    press(&mut h, "Escape");
    assert_eq!(h.app.session.active_sketch, sketch, "Escape after clearing the selection stays in the sketch");
    press(&mut h, "Escape");
    assert!(h.app.session.active_sketch.is_none(), "a quick second Escape finishes the sketch");
    assert_eq!(h.app.session.doc.features.len(), 1, "and keeps it");
}

#[test]
fn escape_drops_the_tool_waiting_for_a_sketch() {
    let mut h = Harness::new();
    press(&mut h, "C");
    press(&mut h, "Escape");
    assert!(h.app.dialog.is_none());
    h.step(&json!({"start":"sketch.create"})).unwrap();
    h.step(&json!({"click":{"plane":"XY"}})).unwrap();
    h.settle();
    assert!(h.app.session.active_sketch.is_some());
    assert!(tool(&h).is_none(), "a cancelled tool came back: {:?}", tool(&h));
}

#[test]
fn construction_key_outside_a_sketch_opens_nothing_and_keys_in_a_sketch_keep_it() {
    let mut h = Harness::new();
    press(&mut h, "X");
    assert!(h.app.dialog.is_none());
    assert!(h.app.session.active_sketch.is_none());
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    press(&mut h, "C");
    assert_eq!(h.app.session.world_state().sketches.len(), 1);
    assert_eq!(tool(&h), Some("sketch.circle.center"));
}

#[test]
fn move_key_keeps_sketch_mode_behavior_and_honors_a_sketch_move_binding() {
    let mut h = Harness::new();
    h.app.run("component.create", json!({"name":"Carriage"})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[10,0]]})).unwrap();
    let sketch = h.app.session.active_sketch;
    let selection = vec![Sel::SketchCurve { id: "l1".into(), sketch: None }];
    h.app.session.selection = selection.clone();

    press(&mut h, "M");
    assert!(h.app.dialog.is_none(), "the default body Move shortcut is disabled in a sketch");
    assert!(h.app.tree.occurrence_move.is_none());
    assert_eq!(h.app.session.active_sketch, sketch);
    assert_eq!(h.app.session.selection, selection);

    crate::keymap::bind(&mut h.app, "sketch.move", "M", true).unwrap();
    press(&mut h, "M");
    let dialog = h.app.dialog.as_ref().expect("a rebound M opens Sketch Move in an active sketch");
    assert!(matches!(dialog.kind, crate::dialogs::Kind::SketchModify(crate::sketch_modify_ui::Mod::Move { .. })));
    assert_eq!(dialog.inputs[0].items, selection);
    assert_eq!(h.app.session.active_sketch, sketch);
    assert!(h.app.tree.occurrence_move.is_none(), "M must not open component Move while editing its sketch");
}

/// #2459: by default a tool's shortcut pressed again starts the tool again, in the mode the key
/// is bound to; the preference is off.
#[test]
fn a_tools_shortcut_pressed_again_restarts_it_by_default() {
    let mut h = Harness::new();
    assert!(!h.app.preferences.shortcut_cycles_modes, "off by default");
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.settle();
    press(&mut h, "R");
    assert_eq!(tool(&h), Some("sketch.rectangle.two_point"));
    press(&mut h, "R");
    assert_eq!(tool(&h), Some("sketch.rectangle.two_point"), "the same mode again");
}

/// A variant with a key of its own doesn't take over its family's key: with G bound to the
/// centre rectangle, G then Escape then R starts the corner rectangle R is bound to. A variant
/// without a key still comes back on R (#4308).
#[test]
fn a_variant_with_its_own_key_leaves_the_family_key_alone() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.settle();
    crate::keymap::bind(&mut h.app, "sketch.rectangle.center", "G", true).unwrap();
    press(&mut h, "G");
    assert_eq!(tool(&h), Some("sketch.rectangle.center"));
    press(&mut h, "Escape");
    press(&mut h, "R");
    assert_eq!(tool(&h), Some("sketch.rectangle.two_point"), "R starts what it is bound to");
    press(&mut h, "Escape");
    h.app.start("sketch.rectangle.three_point");
    h.settle();
    press(&mut h, "Escape");
    press(&mut h, "R");
    assert_eq!(tool(&h), Some("sketch.rectangle.three_point"), "a variant without a key comes back on R");
}

/// #2459: with Preferences › Sketch asking for it, a tool's shortcut pressed again before the
/// first point steps through its family's modes and wraps round; a held key's repeats and a shape
/// already begun keep the tool.
#[test]
fn a_tools_shortcut_pressed_again_cycles_its_modes() {
    let mut h = Harness::new();
    h.app.preferences.shortcut_cycles_modes = true;
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.settle();
    press(&mut h, "R");
    assert_eq!(tool(&h), Some("sketch.rectangle.two_point"), "the first press starts the bound mode");
    for want in ["sketch.rectangle.three_point", "sketch.rectangle.center", "sketch.rectangle.rounded", "sketch.rectangle.two_point"] {
        press(&mut h, "R");
        assert_eq!(tool(&h), Some(want));
    }
    // Auto-repeat of a held R doesn't skip modes.
    crate::keymap::press(&mut h.app, "R", true);
    assert_eq!(tool(&h), Some("sketch.rectangle.two_point"), "a repeat changes nothing");
    // Another tool's key starts that tool, at its bound mode.
    press(&mut h, "C");
    assert_eq!(tool(&h), Some("sketch.circle.center"));
    press(&mut h, "C");
    assert_eq!(tool(&h), Some("sketch.circle.two_point"));
    // Once the shape is begun, its key keeps the tool and the point.
    press(&mut h, "R");
    h.step(&json!({"click": {"world": [0, 0, 0]}})).unwrap();
    assert_eq!(h.app.tool.as_ref().map(|t| t.pts.len()), Some(1));
    press(&mut h, "R");
    assert_eq!(tool(&h), Some("sketch.rectangle.two_point"));
    assert_eq!(h.app.tool.as_ref().map(|t| t.pts.len()), Some(1), "the placed corner is kept");
}

#[test]
fn a_feature_started_in_a_sketch_finishes_the_sketch_and_takes_its_profile() {
    for cmd in ["solid.extrude", "solid.revolve"] {
        let mut h = Harness::new();
        h.app.start("sketch.create");
        h.step(&json!({"click":{"plane":"XY"}})).unwrap();
        h.settle();
        let sketch = h.app.session.active_sketch.expect("sketch opened");
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,10]})).unwrap();
        h.app.start(cmd);
        h.settle();
        assert!(h.app.session.active_sketch.is_none(), "{cmd}: sketch still open");
        assert_eq!(h.app.ui.tab, "MODEL", "{cmd}");
        let items = h.app.dialog.as_ref().and_then(|d| d.inputs.first()).map(|i| i.items.clone()).unwrap_or_default();
        assert_eq!(items, vec![Sel::Profile { sketch, index: 0 }], "{cmd}");
    }
}

#[test]
fn the_extrude_key_in_a_sketch_finishes_the_sketch() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,10]})).unwrap();
    press(&mut h, "E");
    assert!(h.app.session.active_sketch.is_none());
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::Extrude { .. })));
}

#[test]
fn a_construct_command_keeps_the_sketch_open() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    let sketch = h.app.session.active_sketch;
    h.app.start("construct.plane.offset");
    h.settle();
    assert_eq!(h.app.session.active_sketch, sketch);
}

#[test]
fn a_feature_started_while_the_marker_rolls_back_waits_for_it() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    let sketch = h.app.session.active_sketch.expect("sketch opened");
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,10]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("solid.extrude", json!({"distance": 5})).unwrap();
    h.app.edit_feature(sketch);
    h.app.apply_off_thread = Some(std::time::Duration::ZERO);
    h.app.start("solid.extrude");
    assert_eq!(h.app.session.active_sketch, Some(sketch), "Finish and the marker restore commit together after evaluation");
    assert!(h.app.deferred.is_some(), "the marker is not rolling back off the window thread");
    assert!(h.app.dialog.is_none(), "Extrude opened before the marker was back");
    for _ in 0..500 {
        if h.app.deferred.is_none() && h.app.deferred_queue.is_empty() {
            break;
        }
        h.frames(1);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(h.app.session.doc.marker, None);
    assert_eq!(h.app.session.active_sketch, None);
    let items = h.app.dialog.as_ref().and_then(|d| d.inputs.first()).map(|i| i.items.clone()).unwrap_or_default();
    assert_eq!(items, vec![Sel::Profile { sketch, index: 0 }], "Extrude did not open once the marker was back");
}

/// The SKETCH tab kept shown (#7904): right after MODEL with no sketch open, its drawing and
/// reference tools look enabled and start a sketch first (on the selected plane, else on the plane
/// picked next) with the tool ready; tools that need a sketch's geometry stay disabled. Off, as by
/// default, the tab waits for a sketch.
#[test]
fn the_kept_sketch_tab_sits_after_model_and_its_tools_start_a_sketch() {
    let mut h = Harness::new();
    assert!(!crate::workspace::visible_tabs(&h.app).contains(&"SKETCH"), "off by default");
    h.call("ui.prefs", json!({"sketch_tab_always": true}));
    assert_eq!(crate::workspace::visible_tabs(&h.app)[..2], ["MODEL", "SKETCH"]);
    let info = |h: &Harness, id: &str| crate::render_workspace::interactive_command_info(&h.app, solidcraft_engine::find_command(id).unwrap());
    assert!(info(&h, "sketch.line").enabled && info(&h, "sketch.project").enabled);
    assert!(!info(&h, "sketch.dimension").enabled && !info(&h, "sketch.trim").enabled && !info(&h, "sketch.finish").enabled);

    // A plane selected first: the sketch opens on it and the tool is out.
    h.app.session.selection = vec![Sel::Plane { name: "XZ".into() }];
    h.settle();
    h.step(&json!({"click":{"handle":"tab:SKETCH"}})).unwrap();
    h.step(&json!({"click":{"handle":"button:sketch.line"}})).unwrap();
    h.settle();
    assert!(normal(&h).y.abs() > 0.999, "on XZ: {:?}", normal(&h));
    assert_eq!(tool(&h), Some("sketch.line"));
    assert!(h.app.dialog.is_none());
    assert_eq!(crate::workspace::visible_tabs(&h.app)[..2], ["MODEL", "SKETCH"], "the same place while sketching");
    h.app.start("sketch.finish");
    h.settle();
    assert!(h.app.session.active_sketch.is_none());

    // Nothing selected, in a fresh window (the view still looks at XZ): Create Sketch asks for
    // the plane, then the tool starts.
    let mut h = Harness::new();
    h.call("ui.prefs", json!({"sketch_tab_always": true}));
    h.settle();
    h.step(&json!({"click":{"handle":"tab:SKETCH"}})).unwrap();
    h.step(&json!({"click":{"handle":"button:sketch.circle.center"}})).unwrap();
    h.settle();
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::Sketch)));
    assert_eq!(h.app.ui.tab, "SKETCH", "the ribbon stays where the click was");
    h.step(&json!({"click":{"plane":"XY"}})).unwrap();
    h.settle();
    assert!(normal(&h).z.abs() > 0.999, "{:?}", normal(&h));
    assert_eq!(tool(&h), Some("sketch.circle.center"));
    h.app.start("sketch.finish");
    h.settle();

    // Off again: the tab goes, and a drawing tool outside a sketch is refused as before.
    h.call("ui.prefs", json!({"sketch_tab_always": false}));
    h.settle();
    assert!(!crate::workspace::visible_tabs(&h.app).contains(&"SKETCH"));
    assert!(!info(&h, "sketch.line").enabled);
    h.app.start("sketch.line");
    assert!(h.app.dialog.is_none() && tool(&h).is_none());
}

fn line_chain(h: &mut Harness, points: &[[i32; 3]]) {
    h.step(&json!({"start":"sketch.line"})).unwrap();
    for point in points {
        h.step(&json!({"click":{"world":point}})).unwrap();
    }
}

/// Each line of the active sketch, as (start, end).
fn lines(h: &Harness) -> Vec<([f64; 2], [f64; 2])> {
    use solidcraft_engine::sketch::Shape;
    let sid = h.app.session.active_sketch.unwrap();
    let state = h.app.session.model.state();
    let sketch = &state.sketch(sid).unwrap().sketch;
    (0..sketch.curves.len())
        .filter_map(|c| match sketch.shape(c)? {
            Shape::Line { a, b } => Some(([a.x, a.y], [b.x, b.y])),
            _ => None,
        })
        .collect()
}

fn assert_line(line: ([f64; 2], [f64; 2]), start: [f64; 2], end: [f64; 2]) {
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6;
    assert!(near(line.0, start) && near(line.1, end), "{line:?}, expected {start:?} to {end:?}");
}

/// The Undo button runs `edit.undo` and leaves the tool on: the chain goes back to the end of the
/// last segment still there, and the next click draws from it (#10016).
#[test]
fn undo_in_a_line_chain_goes_back_to_the_last_segment_still_there() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.settle();
    line_chain(&mut h, &[[-20, 0, 0], [0, 0, 0], [0, 15, 0]]);
    assert_eq!(lines(&h).len(), 2);

    h.app.run("edit.undo", json!({})).unwrap();
    h.settle();
    assert_eq!(lines(&h).len(), 1);
    h.step(&json!({"click":{"world":[20, 15, 0]}})).unwrap();
    h.settle();
    let drawn = lines(&h);
    assert_eq!(drawn.len(), 2, "the click drew a line: {:?}", h.app.session.log.last());
    assert_line(drawn[0], [-20.0, 0.0], [0.0, 0.0]);
    assert_line(drawn[1], [0.0, 0.0], [20.0, 15.0]);
    let chain = h.app.tool.as_ref().unwrap();
    assert_eq!(chain.cmd, "sketch.line", "the tool stays on");
    assert_eq!(chain.pts.len(), 1, "and the chain carries on from the new end");

    // Undo twice: back past the first segment, to where the chain began.
    h.app.run("edit.undo", json!({})).unwrap();
    h.app.run("edit.undo", json!({})).unwrap();
    h.settle();
    assert!(lines(&h).is_empty());
    h.step(&json!({"click":{"world":[5, 5, 0]}})).unwrap();
    h.settle();
    let drawn = lines(&h);
    assert_eq!(drawn.len(), 1, "{:?}", h.app.session.log.last());
    assert_line(drawn[0], [-20.0, 0.0], [5.0, 5.0]);
}

/// A chain begun on a point Undo then takes away has nowhere to go back to: it ends, the tool
/// stays on, and the next click begins a new chain with no error.
#[test]
fn a_chain_whose_first_point_is_undone_ends_and_the_tool_stays_on() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[-20, -20], [-10, -20]]})).unwrap();
    h.settle();
    h.step(&json!({"start":"sketch.line"})).unwrap();
    h.step(&json!({"click":{"world":[-10, -20, 0]}})).unwrap();
    assert!(h.app.tool.as_ref().unwrap().pts[0].1.is_some(), "the click snapped to the line's end");
    h.app.run("edit.undo", json!({})).unwrap();
    h.settle();
    assert!(lines(&h).is_empty());
    h.step(&json!({"click":{"world":[5, 5, 0]}})).unwrap();
    h.settle();
    let chain = h.app.tool.as_ref().unwrap();
    assert_eq!(chain.cmd, "sketch.line");
    assert_eq!(chain.pts.len(), 1, "the click began a new chain: {:?}", h.app.session.log.last());
    assert!(chain.pts[0].1.is_none() && (chain.pts[0].0.x - 5.0).abs() < 1e-6, "{:?}", chain.pts);
    assert!(lines(&h).is_empty());
    h.step(&json!({"click":{"world":[15, 5, 0]}})).unwrap();
    h.settle();
    assert_eq!(lines(&h).len(), 1, "{:?}", h.app.session.log.last());
}

/// Esc ends a chain for good: a new chain begun on its last point doesn't fall back into the old
/// one when Undo takes that point away.
#[test]
fn undo_does_not_bring_back_a_chain_ended_with_esc() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.settle();
    line_chain(&mut h, &[[-20, 0, 0], [0, 0, 0], [0, 15, 0]]);
    press(&mut h, "Escape");
    assert_eq!(tool(&h), Some("sketch.line"));
    assert!(h.app.tool.as_ref().unwrap().pts.is_empty());
    h.step(&json!({"click":{"world":[0, 15, 0]}})).unwrap();
    assert!(h.app.tool.as_ref().unwrap().pts[0].1.is_some(), "the new chain begins on the old one's end");
    h.app.run("edit.undo", json!({})).unwrap();
    h.settle();
    h.step(&json!({"click":{"world":[5, 5, 0]}})).unwrap();
    h.settle();
    let chain = h.app.tool.as_ref().unwrap();
    assert_eq!(chain.pts.len(), 1);
    assert!(chain.pts[0].1.is_none(), "a new chain, from the click, not the old one's point: {:?}", chain.pts);
    assert_eq!(lines(&h).len(), 1, "only the first segment is left");
}
