use crate::scenario::Harness;
use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::{FeatureKind, PlaneRef};
use solidcraft_engine::geom::prelude::*;

fn tool(h: &Harness) -> Option<&str> {
    h.app.tool.as_ref().map(|t| t.cmd)
}

fn normal(h: &Harness) -> solidcore_record::Vec3 {
    let active = h.app.session.active_sketch;
    h.app.session.world_state().unwrap().sketches.iter().find(|s| Some(s.feature) == active).map(|s| s.plane.normal()).unwrap_or_default()
}

fn press(h: &mut Harness, key: &str) {
    h.step(&json!({"key": key})).unwrap();
    h.settle();
}

/// #12902: Ctrl+Shift+Z redoes, like Ctrl+Y; it used to undo a second time, Shift ignored. With a
/// spline being drawn, Ctrl+Z takes its last point back and Ctrl+Shift+Z still redoes the history.
#[test]
fn ctrl_shift_z_redoes_like_ctrl_y() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[30,20]})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[50,10],"radius":5})).unwrap();
    h.settle();
    let sketch = h.app.session.active_sketch.unwrap();
    let curves = |h: &Harness| h.app.session.model.state().sketch(sketch).unwrap().sketch.curves.len();
    let drawn = curves(&h);
    let undo = h.app.session.undo.len();
    let press = |h: &mut Harness, key: &str, shift: bool| {
        h.step(&json!({"key": key, "cmd": true, "shift": shift})).unwrap();
        h.settle();
        // Well apart, so two presses are never a quick double press.
        h.frames(60);
    };
    press(&mut h, "Z", false);
    assert_eq!((curves(&h), h.app.session.undo.len()), (drawn - 1, undo - 1), "Ctrl+Z undoes the circle");
    press(&mut h, "Z", true);
    assert_eq!((curves(&h), h.app.session.undo.len()), (drawn, undo), "Ctrl+Shift+Z redoes it");
    press(&mut h, "Z", false);
    press(&mut h, "Y", false);
    assert_eq!((curves(&h), h.app.session.undo.len()), (drawn, undo), "Ctrl+Y still redoes");
    // A spline with pending points: Ctrl+Z takes a point back, Ctrl+Shift+Z leaves the points
    // alone and redoes the history.
    press(&mut h, "Z", false);
    h.step(&json!({"start":"sketch.spline.fit_point"})).unwrap();
    for point in [[2, 2, 0], [8, 2, 0]] {
        h.step(&json!({"click":{"world":point}})).unwrap();
    }
    assert_eq!(h.app.tool.as_ref().unwrap().pts.len(), 2);
    press(&mut h, "Z", true);
    assert_eq!(h.app.tool.as_ref().unwrap().pts.len(), 2, "Ctrl+Shift+Z is not a spline-point undo");
    assert_eq!(curves(&h), drawn, "the circle is back");
    press(&mut h, "Z", false);
    assert_eq!((h.app.tool.as_ref().unwrap().pts.len(), curves(&h)), (1, drawn), "Ctrl+Z takes the spline point back first");
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
        assert_eq!(h.app.session.world_state().unwrap().sketches.len(), 1, "{key}");
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
    h.app.ui.pick_mode = solidcraft_engine::PickMode::Faces;
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

#[test]
fn a_sketch_modify_key_outside_a_sketch_opens_no_sketch() {
    for key in ["T", "G"] {
        let mut h = Harness::new();
        let id = crate::keymap::commands_for(&h.app, key).into_iter().find(|id| id.starts_with("sketch.")).unwrap_or_default();
        assert_eq!(solidcraft_engine::find_command(id).map(|c| c.panel), Some("MODIFY"), "{key} is a sketch Modify tool's key");
        press(&mut h, key);
        assert!(h.app.dialog.is_none(), "{key} opened {:?}", h.app.dialog.as_ref().map(|d| &d.kind));
        assert!(h.app.session.active_sketch.is_none(), "{key}");
        assert!(tool(&h).is_none(), "{key}");
    }
    let mut h = Harness::new();
    assert!(crate::keymap::commands_for(&h.app, "O").contains(&"sketch.offset"));
    press(&mut h, "O");
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::Sketch)));
    assert!(h.app.session.active_sketch.is_none());
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
    assert_eq!(h.app.session.world_state().unwrap().sketches.len(), 1);
    assert_eq!(tool(&h), Some("sketch.circle.center"));
}

#[test]
fn move_key_opens_sketch_move_by_default_and_honors_a_cleared_binding() {
    let mut h = Harness::new();
    h.app.run("component.create", json!({"name":"Carriage"})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[10,0]]})).unwrap();
    let sketch = h.app.session.active_sketch;
    let selection = vec![Sel::SketchCurve { id: "l1".into(), sketch: None }];
    h.app.session.selection = selection.clone();

    assert_eq!(crate::keymap::effective(&h.app, "sketch.move").as_deref(), Some("M"));
    press(&mut h, "M");
    let dialog = h.app.dialog.as_ref().expect("M opens Sketch Move in an active sketch by default");
    assert!(matches!(dialog.kind, crate::dialogs::Kind::SketchModify(crate::sketch_modify_ui::Mod::Move { .. })));
    assert_eq!(dialog.inputs[0].items, selection);
    assert_eq!(h.app.session.active_sketch, sketch);
    assert!(crate::dialogs_move::moving(&h.app).is_none(), "M must not open component Move while editing its sketch");

    crate::dialogs::cancel(&mut h.app);
    // Closing the dialog clears its selection. Re-select to verify that an
    // unbound key leaves the current sketch selection alone.
    h.app.session.selection = selection.clone();
    // Without mode isolation, M can still invoke the model's Move command.
    h.app.preferences.sketch_keys_only = true;
    h.app.keymap.custom.insert("sketch.move".into(), Vec::new());
    assert_eq!(crate::keymap::effective(&h.app, "sketch.move"), None, "a cleared user binding overrides the new default");
    assert_eq!(crate::keymap::display_for(&h.app, "sketch.move"), None, "the menu follows the user's cleared key");
    press(&mut h, "M");
    assert!(h.app.dialog.is_none(), "M stays a no-op in a sketch when its sketch binding was cleared");
    assert_eq!(h.app.session.active_sketch, sketch);
    assert_eq!(h.app.session.selection, selection);
}

/// With Keep model keys out of sketches on, M opens Move/Copy Sketch Entities in a sketch
/// and model Move outside. F (Fillet) does nothing in a sketch; Measure works in both.
/// A key only clashes with commands that work in the same place.
#[test]
fn sketch_and_model_keys_are_kept_apart() {
    use crate::keymap::{Scope, bind, command_for, scope};
    let mut h = Harness::new();
    h.app.preferences.sketch_keys_only = true;
    assert_eq!(scope("sketch.move"), Scope::Sketch);
    assert_eq!(scope("solid.fillet"), Scope::Model);
    assert_eq!(crate::keymap::effective(&h.app, "sketch.move").as_deref(), Some("M"));
    assert_eq!(command_for(&h.app, "M"), Some("solid.move"));
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.settle();
    assert_eq!(command_for(&h.app, "M"), Some("sketch.move"));
    assert_eq!(command_for(&h.app, "F"), None, "no model tool from a key pressed in a sketch");
    press(&mut h, "F");
    assert!(h.app.dialog.is_none() && h.app.session.active_sketch.is_some(), "F left the sketch alone");
    assert_eq!(command_for(&h.app, "I"), Some("inspect.measure"), "Measure's key works in a sketch too");
    // A sketch command may take a model command's key, but not another sketch command's.
    bind(&mut h.app, "sketch.fillet", "F", false).unwrap();
    assert_eq!(command_for(&h.app, "F"), Some("sketch.fillet"));
    let e = bind(&mut h.app, "sketch.fillet", "L", false).unwrap_err();
    assert!(e.contains("already used"), "{e}");
}

/// Pressing an active tool or dialog's shortcut again closes it by default; auto-repeat does not,
/// and with `shortcut_again_ends_tool` off the key starts the tool again as before.
#[test]
fn a_tools_shortcut_pressed_again_closes_it_by_default() {
    let mut h = Harness::new();
    assert!(!h.app.preferences.shortcut_cycles_modes, "off by default");
    assert!(h.app.preferences.shortcut_again_ends_tool, "on by default");
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.settle();
    press(&mut h, "L");
    assert_eq!(tool(&h), Some("sketch.line"));
    crate::keymap::press(&mut h.app, "L", true);
    assert_eq!(tool(&h), Some("sketch.line"), "key repeat keeps the tool");
    press(&mut h, "L");
    assert_eq!(tool(&h), None, "pressing L again closes the tool");
    crate::keymap::press(&mut h.app, "L", true);
    assert_eq!(tool(&h), None, "key repeat after closing does not reopen the tool");
    h.app.finish_sketch();
    h.settle();
    press(&mut h, "E");
    assert_eq!(h.app.dialog.as_ref().and_then(|d| d.command()), Some("solid.extrude"));
    crate::keymap::press(&mut h.app, "E", true);
    assert_eq!(h.app.dialog.as_ref().and_then(|d| d.command()), Some("solid.extrude"), "key repeat keeps the dialog");
    press(&mut h, "E");
    assert!(h.app.dialog.is_none(), "pressing E again closes the extrude dialog");
    crate::keymap::press(&mut h.app, "E", true);
    assert!(h.app.dialog.is_none(), "key repeat after closing does not reopen the dialog");
    // The old way, behind the preference: pressing the key again restarts the tool.
    h.app.preferences.shortcut_again_ends_tool = false;
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
    for want in [
        "sketch.rectangle.three_point",
        "sketch.rectangle.center",
        "sketch.rectangle.edge_center",
        "sketch.rectangle.rounded",
        "sketch.rectangle.two_point",
    ] {
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

/// With Keep model keys out of sketches on, E in a sketch does nothing: the sketch stays open and
/// no Extrude opens. Off (the default), the test above holds.
#[test]
fn with_model_keys_kept_out_the_extrude_key_in_a_sketch_does_nothing() {
    let mut h = Harness::new();
    h.app.preferences.sketch_keys_only = true;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    let sketch = h.app.session.active_sketch;
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,10]})).unwrap();
    press(&mut h, "E");
    assert_eq!(h.app.session.active_sketch, sketch, "E left the sketch open");
    assert!(h.app.dialog.is_none(), "no dialog opened: {:?}", h.app.dialog.as_ref().map(|d| &d.kind));
    assert!(h.app.tool.is_none());
}

#[test]
fn a_construct_command_keeps_the_sketch_open() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    let sketch = h.app.session.active_sketch;
    h.app.start("construct.plane.offset");
    h.settle();
    // Starting any other tool, a construction plane included, finishes the open sketch.
    assert!(sketch.is_some());
    assert_eq!(h.app.session.active_sketch, None);
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
    assert_eq!(h.app.session.active_sketch, None, "the sketch finishes right away, and the marker restore follows after evaluation");
    // Extrude waits for the restored model, worked out on the sketch model worker (not the
    // history queue): each frame below is drawn once that worker has answered.
    h.frames_after_sketch_refresh(5);
    assert!(h.app.deferred.is_none() && h.app.deferred_queue.is_empty());
    assert_eq!(h.app.session.doc.marker, None);
    assert_eq!(h.app.session.active_sketch, None);
    assert!(h.app.dialog.is_some(), "Extrude opened once the marker was back");
    let _ = sketch;
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

/// A snap is only materialized when the line is drawn. Hovering between clicks must not
/// mistake it for a point removed by Undo (#11181), including when Undo restores that start.
#[test]
fn a_line_starts_on_a_curve_or_midpoint_and_keeps_that_snap_after_undo() {
    use solidcraft_engine::sketch::ConstraintKind;
    for construction in [false, true] {
        for (start, midpoint) in [([20.0, 10.0], false), ([37.5, 10.0], true)] {
            let mut h = Harness::new();
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            h.app.run("sketch.line", json!({"points":[[0,10],[75,10]],"ids":["guide"],"construction":construction,"infer":false})).unwrap();
            h.call("ui.camera", json!({"target":[37.5,10,0],"distance":200}));
            h.settle();
            h.step(&json!({"start":"sketch.line"})).unwrap();
            let before = h.app.session.doc.to_json();
            h.step(&json!({"click":{"sketch":start}})).unwrap();
            h.settle();
            assert_eq!(h.app.session.doc.to_json(), before, "the first click only remembers its snap");
            for end in [[70.0, 10.0], [60.0, 30.0]] {
                h.step(&json!({"click":{"sketch":end}})).unwrap();
                h.settle();
                let drawn = lines(&h);
                assert_eq!(drawn.len(), 2, "construction={construction}, midpoint={midpoint}: {:?}", h.app.status);
                // An on-curve click is projected from f32 screen coordinates, unlike a grid snap.
                for (actual, expected) in [(drawn[1].0, start), (drawn[1].1, end)] {
                    assert!((actual[0] - expected[0]).abs() < 1e-5 && (actual[1] - expected[1]).abs() < 1e-5, "{actual:?}, expected {expected:?}");
                }
                let sid = h.app.session.active_sketch.unwrap();
                let state = h.app.session.world_state().unwrap();
                let sketch = &state.sketch(sid).unwrap().sketch;
                let guide = sketch.curve_index("guide").unwrap();
                assert!(
                    sketch.constraints.iter().any(|c| match c.kind {
                        ConstraintKind::Midpoint { l, .. } => midpoint && l == guide,
                        ConstraintKind::PointOnCurve { c, .. } => !midpoint && c == guide,
                        _ => false,
                    }),
                    "the line retains its snap constraint"
                );
                h.app.run("edit.undo", json!({})).unwrap();
                h.settle();
                assert_eq!(h.app.session.doc.to_json(), before, "Undo removes the line and its materialized snap together");
            }
        }
    }
}

/// #14637: a line that starts on the middle of another line is drawn, and it stays held there:
/// its start at that middle, its end on the line opposite. The reporter clicked the middle of a
/// line and no line was created at all: the click was read as the midpoint (the guide under the
/// pointer), but the point it left behind was dropped before the second click.
#[test]
fn a_line_started_on_the_middle_of_a_line_is_drawn_and_held_there() {
    use solidcraft_engine::sketch::ConstraintKind;
    for construction in [false, true] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        // The line the reporter clicked on, and the line opposite it.
        h.app.run("sketch.line", json!({"points":[[-40,10],[40,10]],"ids":["across"],"construction":construction,"infer":false})).unwrap();
        h.app.run("sketch.line", json!({"points":[[-40,-20],[40,-20]],"ids":["opposite"],"infer":false})).unwrap();
        h.call("ui.view", json!({"view":"top"}));
        h.call("ui.camera", json!({"target":[0,-5,0],"distance":200}));
        h.settle();
        let before = lines(&h).len();
        h.step(&json!({"start":"sketch.line"})).unwrap();
        // The start on the middle of the first line, the end on the line opposite. The pointer
        // moves between the two clicks, as a hand does.
        for at in [[0.0, 10.0], [20.0, -20.0]] {
            h.step(&json!({"move":{"sketch":at}})).unwrap();
            h.step(&json!({"click":{"sketch":at}})).unwrap();
        }
        h.settle();
        h.step(&json!({"shot":format!("line-from-the-middle-of-a-line-{}", if construction {"construction"} else {"solid"})})).unwrap();
        let drawn = lines(&h);
        assert_eq!(drawn.len(), before + 1, "construction={construction}: no line from the middle; {:?}", h.app.status);
        let sid = h.app.session.active_sketch.unwrap();
        let state = h.app.session.world_state().unwrap();
        let sketch = &state.sketch(sid).unwrap().sketch;
        let Some(solidcraft_engine::sketch::CurveKind::Line { a, b }) = sketch.curves.last().map(|c| c.kind.clone()) else {
            panic!("construction={construction}: no new line");
        };
        let across = sketch.curve_index("across").unwrap();
        let opposite = sketch.curve_index("opposite").unwrap();
        let kinds = || sketch.constraints.iter().map(|k| &k.kind).collect::<Vec<_>>();
        assert!(
            sketch.constraints.iter().any(|k| matches!(k.kind, ConstraintKind::Midpoint { p, l } if p == a && l == across)),
            "construction={construction}: the line's start is not held at the middle: {:?}",
            kinds()
        );
        assert!(
            sketch.constraints.iter().any(|k| matches!(k.kind, ConstraintKind::PointOnCurve { p, c } if p == b && c == opposite)),
            "construction={construction}: the line's end is not held on the line opposite: {:?}",
            kinds()
        );
        // The start is at the middle itself; the end is where the pointer was on the line
        // opposite, a hair off it (a click comes from an f32 screen position).
        let start = sketch.point(a).unwrap();
        let end = sketch.point(b).unwrap();
        assert!((start.x).abs() < 1e-6 && (start.y - 10.0).abs() < 1e-6, "construction={construction}: the line's start is at {start:?}");
        assert!((end.x - 20.0).abs() < 1e-3 && (end.y + 20.0).abs() < 1e-3, "construction={construction}: the line's end is at {end:?}");
    }
}

#[test]
fn a_line_starts_at_a_curve_crossing_and_keeps_it_after_undo() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    for (id, points) in [("across", [[-40, 10], [35, 10]]), ("upright", [[-20, -20], [-20, 40]])] {
        h.app.run("sketch.line", json!({"points":points,"ids":[id],"construction":true,"infer":false})).unwrap();
    }
    h.settle();
    h.step(&json!({"start":"sketch.line"})).unwrap();
    h.step(&json!({"click":{"sketch":[-20,10]}})).unwrap();
    for end in [[20.0, 30.0], [25.0, -10.0]] {
        h.step(&json!({"click":{"sketch":end}})).unwrap();
        h.settle();
        let drawn = lines(&h);
        assert_eq!(drawn.len(), 3, "{:?}", h.app.status);
        assert_line(drawn[2], [-20.0, 10.0], end);
        let state = h.app.session.world_state().unwrap();
        let sketch = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        assert_eq!(
            sketch.constraints.iter().filter(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::PointOnCurve { .. })).count(),
            2,
            "the new line stays on both crossing curves"
        );
        h.app.run("edit.undo", json!({})).unwrap();
        h.settle();
        assert_eq!(lines(&h).len(), 2);
    }
}

/// The Line tool's first click on an existing shape starts the line there and ties it to the shape,
/// as its last click does: a line's midpoint and a circle's centre (#11171, #11186), a circle's
/// edge (#11272), a rectangle's edge (#11291) and a point part-way along a line (#11223).
#[test]
fn a_line_starts_on_a_snap_point_of_an_existing_shape() {
    use solidcraft_engine::sketch::{ConstraintKind, CurveKind};
    let edge = [25.0 + 10.0 * std::f64::consts::FRAC_1_SQRT_2, -15.0 + 10.0 * std::f64::consts::FRAC_1_SQRT_2];
    for (case, shape, start, end) in [
        ("line midpoint", "guide", [-10.0, 10.0], [-5.0, 30.0]),
        ("circle centre", "round", [25.0, -15.0], [45.0, 5.0]),
        ("circle edge", "round", edge, [45.0, 15.0]),
        ("rectangle edge", "box", [-35.0, -28.0], [-50.0, -5.0]),
        ("part-way along a line", "guide", [-22.0, 10.0], [-17.0, 30.0]),
    ] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.line", json!({"points":[[-30,10],[10,10]],"ids":["guide"],"infer":false})).unwrap();
        h.app.run("sketch.circle.center", json!({"center":[25,-15],"radius":10})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[-35,-40],"p1":[-15,-25]})).unwrap();
        h.call("ui.camera", json!({"target":[0,-10,0],"distance":200}));
        h.settle();
        let before = lines(&h).len();
        h.step(&json!({"start":"sketch.line"})).unwrap();
        h.step(&json!({"click":{"sketch":start}})).unwrap();
        h.step(&json!({"click":{"sketch":end}})).unwrap();
        h.settle();
        let drawn = lines(&h);
        assert_eq!(drawn.len(), before + 1, "{case}: no line from the first click; {:?}", h.app.status);
        let near = |a: [f64; 2], b: [f64; 2], tol: f64| (a[0] - b[0]).abs() < tol && (a[1] - b[1]).abs() < tol;
        let line = drawn[before];
        assert!(near(line.0, start, 1e-3) && near(line.1, end, 1.0), "{case}: {line:?}, expected {start:?} to {end:?}");
        let sid = h.app.session.active_sketch.unwrap();
        let state = h.app.session.world_state().unwrap();
        let sketch = &state.sketch(sid).unwrap().sketch;
        let Some(CurveKind::Line { a, .. }) = sketch.curves.last().map(|c| c.kind.clone()) else { panic!("{case}: no new line") };
        // The rectangle's left edge is the one at x = -35.
        let target = match shape {
            "box" => (0..sketch.curves.len())
                .find(|&c| matches!(sketch.shape(c), Some(solidcraft_engine::sketch::Shape::Line { a, b }) if a.x == -35.0 && b.x == -35.0))
                .unwrap(),
            "round" => sketch.curves.iter().position(|c| matches!(c.kind, CurveKind::Circle { .. })).unwrap(),
            id => sketch.curve_index(id).unwrap(),
        };
        let centre = match sketch.curves[target].kind {
            CurveKind::Circle { c, .. } if case == "circle centre" => Some(c),
            _ => None,
        };
        let tied = centre == Some(a)
            || sketch.constraints.iter().any(|c| match c.kind {
                ConstraintKind::Coincident { p, q } => centre.is_some_and(|m| (p, q) == (a, m) || (p, q) == (m, a)),
                ConstraintKind::PointOnCurve { p, c } => p == a && c == target,
                ConstraintKind::Midpoint { p, l } => p == a && l == target,
                _ => false,
            });
        assert!(tied, "{case}: the line's start is not tied to the {shape}");
    }
}

/// #14724: the Line tool's first click on a rectangle's side holds the line's start on that side,
/// as its last click holds its end. The rectangle the reporter drew was a centre rectangle, whose
/// diagonals are construction curves, and the pointer moved between the two clicks.
#[test]
fn a_line_started_on_a_rectangle_side_is_held_on_it() {
    use solidcraft_engine::sketch::{ConstraintKind, CurveKind, Shape};
    for (rectangle, params) in [
        ("sketch.rectangle.two_point", json!({"p0":[-20,-12.5],"p1":[20,12.5]})),
        ("sketch.rectangle.center", json!({"center":[0,0],"corner":[20,12.5]})),
    ] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run(rectangle, params).unwrap();
        h.call("ui.camera", json!({"target":[0,0,0],"distance":200}));
        h.settle();
        let before = lines(&h).len();
        h.step(&json!({"start":"sketch.line"})).unwrap();
        // The start on the left side, the end on the right one. The pointer moves between the
        // two clicks, as a hand does.
        for at in [[-20.0, 6.0], [20.0, 6.0]] {
            h.step(&json!({"move":{"sketch":at}})).unwrap();
            h.step(&json!({"click":{"sketch":at}})).unwrap();
        }
        h.settle();
        let drawn = lines(&h);
        assert_eq!(drawn.len(), before + 1, "{rectangle}: no line from the first click; {:?}", h.app.status);
        let sid = h.app.session.active_sketch.unwrap();
        let state = h.app.session.world_state().unwrap();
        let sketch = &state.sketch(sid).unwrap().sketch;
        let Some(CurveKind::Line { a, b }) = sketch.curves.last().map(|c| c.kind.clone()) else { panic!("{rectangle}: no new line") };
        let side = |x: f64| {
            (0..sketch.curves.len())
                .find(|&c| matches!(sketch.shape(c), Some(Shape::Line { a, b }) if a.x == x && b.x == x))
                .unwrap_or_else(|| panic!("{rectangle}: no side at x {x}"))
        };
        let (left, right) = (side(-20.0), side(20.0));
        let held = |p: usize, c: usize| {
            sketch.constraints.iter().any(|k| matches!(k.kind, ConstraintKind::PointOnCurve { p: q, c: on } if q == p && on == c))
        };
        let kinds = || sketch.constraints.iter().map(|k| &k.kind).collect::<Vec<_>>();
        assert!(held(a, left), "{rectangle}: the line's start is not held on the left side: {:?}", kinds());
        assert!(held(b, right), "{rectangle}: the line's end is not held on the right side: {:?}", kinds());
        // The start is on the side, not merely near it: its x is the side's own. Its height is
        // where the pointer was, and a click comes from an f32 screen position, so it is a hair
        // off it (the same reason the test above measures an on-curve click in 1e-5).
        let start = sketch.point(a).unwrap();
        assert!((start.x + 20.0).abs() < 1e-6 && (start.y - 6.0).abs() < 1e-5, "{rectangle}: the line's start is at {start:?}");
    }
}

/// The Line tool's first click also starts on projected geometry and in construction mode, as its
/// last click does: a projected origin axis or plane (#11598), the projected edges of the face a
/// sketch sits on (#12382), and a construction line on a rectangle's edge (#12228). Reported as one
/// failure with #11369, #11449, #11465, #11736 and #12571.
#[test]
fn a_line_starts_on_projected_geometry_and_in_construction_mode() {
    use solidcraft_engine::sketch::{ConstraintKind, CurveKind};
    for case in ["projected X axis", "projected YZ plane", "face edge", "construction on a rectangle edge"] {
        let mut h = Harness::new();
        // Where the first click goes and where the line ends, in world coordinates.
        let (start, end) = match case {
            "projected X axis" => {
                h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
                h.app.run("sketch.project", json!({"refs":["X"]})).unwrap();
                ([17.0, 0.0, 0.0], [30.0, 20.0, 0.0])
            }
            "projected YZ plane" => {
                h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
                h.app.run("sketch.project", json!({"refs":[{"plane":"YZ"}]})).unwrap();
                ([0.0, 13.0, 0.0], [20.0, 25.0, 0.0])
            }
            "face edge" => {
                h.app.run("solid.box", json!({"length":40,"width":40,"height":10,"body_name":"Block"})).unwrap();
                h.app.run("sketch.create", json!({"plane":{"face":[20,20,10]}})).unwrap();
                ([0.0, 13.0, 10.0], [15.0, 25.0, 10.0])
            }
            _ => {
                h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
                h.app.run("sketch.rectangle.two_point", json!({"p0":[-35,-40],"p1":[-15,-25]})).unwrap();
                ([-35.0, -28.0, 0.0], [-50.0, -5.0, 0.0])
            }
        };
        h.call("ui.view", json!({"view":"top"}));
        h.call("ui.camera", json!({"target":[start[0], start[1], start[2]],"distance":200}));
        h.settle();
        let construction = case.starts_with("construction");
        h.step(&json!({"start":"sketch.line"})).unwrap();
        if construction {
            crate::sketch_palette::set_mode(&mut h.app, 1);
        }
        let curves = |h: &Harness| {
            let state = h.app.session.world_state().unwrap();
            state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch.curves.len()
        };
        let before = curves(&h);
        h.step(&json!({"click":{"world":start}})).unwrap();
        h.step(&json!({"move":{"world":[(start[0] + end[0]) / 2.0, (start[1] + end[1]) / 2.0, start[2]]}})).unwrap();
        h.step(&json!({"click":{"world":end}})).unwrap();
        h.settle();
        assert_eq!(curves(&h), before + 1, "{case}: no line from the first click; {:?}", h.app.status);
        let state = h.app.session.world_state().unwrap();
        let ss = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
        let sketch = &ss.sketch;
        let new = sketch.curves.last().unwrap();
        let CurveKind::Line { a, .. } = new.kind else { panic!("{case}: the new curve is not a line") };
        assert_eq!(new.construction, construction, "{case}: line type");
        let at = ss.plane.to_world(sketch.point(a).unwrap());
        assert!((at.x - start[0]).abs() < 1e-3 && (at.y - start[1]).abs() < 1e-3, "{case}: the line starts at {at:?}, expected {start:?}");
        let tied = sketch.constraints.iter().any(|c| match c.kind {
            ConstraintKind::PointOnCurve { p, c } | ConstraintKind::Midpoint { p, l: c } => p == a && Some(c) != sketch.curves.len().checked_sub(1),
            _ => false,
        });
        assert!(tied, "{case}: the line's start is not tied to the edge it was started on: {:?}", sketch.constraints);
    }
}

/// The Line tool's first click starts a line on the edges a sketch projects from the face it
/// sits on: a rectangle drawn in a sketch and extruded, a new sketch picked on the extruded face,
/// and a line started on each of the four edges that face projects. Reported as #14796.
#[test]
fn a_line_starts_on_the_projected_edges_of_a_picked_extruded_face() {
    use solidcraft_engine::sketch::{ConstraintKind, CurveKind, Shape};
    for edge in 0..4 {
        let mut h = Harness::new();
        h.app.set_origin_shown(false);
        // A rectangle drawn in a sketch and extruded: the extruded body has a cap face on top.
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,40]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("solid.extrude", json!({"distance":10})).unwrap();
        h.call("ui.view", json!({"view":"top"}));
        h.call("ui.view", json!({"view":"fit"}));
        h.settle();
        // A new sketch picked on that face: the face's edges are projected into it.
        h.step(&json!({"start":"sketch.create"})).unwrap();
        h.step(&json!({"click":{"world":[20,20,10]}})).unwrap();
        h.settle();
        let state = h.app.session.world_state().unwrap();
        let ss = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
        let sketch = &ss.sketch;
        let projected: Vec<usize> = (0..sketch.curves.len()).filter(|&c| sketch.curves[c].link.is_some()).collect();
        assert_eq!(projected.len(), 4, "the picked face projects its four edges: {:?}", sketch.curves);
        // The middle of every projected edge, in world coordinates, and their middle: the middle
        // of the projected rectangle lies on no edge.
        let middles: Vec<_> = projected
            .iter()
            .map(|&c| {
                let (a, b) = match sketch.shape(c).unwrap() {
                    Shape::Line { a, b } => (a, b),
                    other => panic!("a projected edge is not a line: {other:?}"),
                };
                ss.plane.to_world((a + b) / 2.0)
            })
            .collect();
        let start = middles[edge];
        let n = middles.len() as f64;
        let end = [
            middles.iter().map(|v| v.x).sum::<f64>() / n,
            middles.iter().map(|v| v.y).sum::<f64>() / n,
            middles.iter().map(|v| v.z).sum::<f64>() / n,
        ];
        let before = sketch.curves.len();
        h.step(&json!({"start":"sketch.line"})).unwrap();
        h.step(&json!({"click":{"world":[start.x, start.y, start.z]}})).unwrap();
        h.step(&json!({"move":{"world":end}})).unwrap();
        h.step(&json!({"click":{"world":end}})).unwrap();
        h.settle();
        let state = h.app.session.world_state().unwrap();
        let ss = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
        let sketch = &ss.sketch;
        assert_eq!(sketch.curves.len(), before + 1, "edge {edge}: no line from the first click; {:?}", h.app.status);
        let new = sketch.curves.last().unwrap();
        let CurveKind::Line { a, .. } = new.kind else { panic!("edge {edge}: the new curve is not a line") };
        let at = ss.plane.to_world(sketch.point(a).unwrap());
        assert!(
            (at.x - start.x).abs() < 1e-3 && (at.y - start.y).abs() < 1e-3 && (at.z - start.z).abs() < 1e-3,
            "edge {edge}: the line starts at {at:?}, expected {start:?}"
        );
        let tied = sketch.constraints.iter().any(|c| match c.kind {
            ConstraintKind::PointOnCurve { p, c } | ConstraintKind::Midpoint { p, l: c } => p == a && projected.contains(&c),
            _ => false,
        });
        assert!(tied, "edge {edge}: the line's start is not tied to the projected edge it was started on: {:?}", sketch.constraints);
    }
}

#[test]
fn a_line_snap_whose_source_curve_is_undone_ends_without_an_error() {
    for start in [[-20.0, 10.0], [-2.5, 10.0]] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.line", json!({"points":[[-40,10],[35,10]],"construction":true,"infer":false})).unwrap();
        h.settle();
        h.step(&json!({"start":"sketch.line"})).unwrap();
        h.step(&json!({"click":{"sketch":start}})).unwrap();
        h.app.run("edit.undo", json!({})).unwrap();
        h.settle();
        h.step(&json!({"click":{"sketch":[-20,-20]}})).unwrap();
        assert!(lines(&h).is_empty(), "the missing snap must not create a line");
        h.step(&json!({"click":{"sketch":[-10,-20]}})).unwrap();
        h.settle();
        let drawn = lines(&h);
        assert_eq!(drawn.len(), 1, "{:?}", h.app.status);
        assert_line(drawn[0], [-20.0, -20.0], [-10.0, -20.0]);
        h.step(&json!({"expect":{"errors":0}})).unwrap();
    }
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

/// #22247: a shape's key starts the variant used last (#4308), and its menu says so. After a
/// 2-point circle, C starts 2-Point Circle, and the Circle menu shows C beside that row, not
/// beside Center Diameter Circle. A variant with a key of its own keeps it, and then the family's
/// key starts its first command again, shown there.
#[test]
fn a_shape_menu_shows_the_key_beside_the_variant_it_starts() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.settle();
    let key = |h: &Harness, id: &'static str| crate::keymap::display_starting(&h.app, id);
    let c = crate::keymap::display_for(&h.app, "sketch.circle.center");
    assert!(c.is_some(), "Center Diameter Circle has the family's key");
    assert_eq!(key(&h, "sketch.circle.center"), c, "before any other circle, the key is beside the first");
    assert_eq!(key(&h, "sketch.circle.two_point"), None);
    h.app.preferences.sketch_variants.remember("sketch.circle.two_point");
    assert_eq!(key(&h, "sketch.circle.center"), None, "C no longer starts Center Diameter Circle");
    assert_eq!(key(&h, "sketch.circle.two_point"), c, "C starts 2-Point Circle, and its row says so");
    assert_eq!(key(&h, "sketch.circle.three_point"), None);
    press(&mut h, c.as_deref().unwrap_or("C"));
    assert_eq!(tool(&h), Some("sketch.circle.two_point"), "the key starts the row it is shown beside");
    // Given a key of its own, 2-Point Circle shows that one, and C is Center Diameter's again.
    h.app.keymap.custom.insert("sketch.circle.two_point".into(), vec!["Shift+C".into()]);
    assert_eq!(key(&h, "sketch.circle.center"), c);
    assert_eq!(key(&h, "sketch.circle.two_point"), crate::keymap::display_for(&h.app, "sketch.circle.two_point"));
    // Commands outside the shape families show their own key, as before.
    assert_eq!(key(&h, "sketch.line"), crate::keymap::display_for(&h.app, "sketch.line"));
}
