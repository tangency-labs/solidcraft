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
        assert_eq!(tool(&h), Some(cmd), "{key}");
        assert!(h.app.dialog.is_none(), "{key}: dialog left open");
    }
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
    h.app.session.selection = vec![Sel::SketchCurve { id: "l1".into() }];
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
    let selection = vec![Sel::SketchCurve { id: "l1".into() }];
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
