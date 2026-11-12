//! Real 3D drawing and triad input in a rotated second component occurrence.

use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::geom::prelude::*;

#[test]
fn bridge_line_snaps_to_points_on_two_visible_sketch_planes() {
    let mut h = crate::scenario::Harness::new();
    for (name, offset, point) in [("Lower", 10, [10, 20]), ("Upper", 40, [30, 35])] {
        h.app.run("sketch.create", json!({"plane":{"origin":[0,0,offset],"x_dir":[1,0,0],"y_dir":[0,1,0]},"name":name})).unwrap();
        h.app.run("sketch.point", json!({"point":point})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
    }
    let bridge = h.app.run("sketch.create", json!({"plane":"XY","name":"Bridge"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.options", json!({"sketch_3d":true})).unwrap();
    h.app.set_origin_shown(false);
    h.call("ui.view", json!({"view":"home"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.frames(3);
    h.app.start("sketch.line");
    h.frames(3);
    // Idle selection offers other sketches' points for Project preselection; a sketch tool's
    // ordinary pick stays local, and only the 3D bridge snap (`pick_3d`) reaches across.
    let [x, y] = h.at(&json!({"world":[10,20,10]})).unwrap();
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    assert!(
        crate::viewport::pick(&h.app, &proj, egui::pos2(x as f32, y as f32))
            .iter()
            .all(|hit| { !matches!(hit, crate::viewport::Hit::SketchPoint { sketch, .. } if *sketch != bridge) }),
        "a sketch tool's ordinary pick must keep its local sketch-point scope"
    );
    for p in [[10, 20, 10], [30, 35, 40]] {
        let [x, y] = h.at(&json!({"world":p})).unwrap();
        let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        let picked = crate::sketch3d::point_at(&h.app, &proj, egui::pos2(x as f32, y as f32)).unwrap().0;
        assert!(picked.dist(Vec3::new(p[0] as f64, p[1] as f64, p[2] as f64)) < 1e-6, "{picked:?}");
        h.step(&json!({"click":{"world":p}})).unwrap();
    }
    h.step(&json!({"key":"Escape"})).unwrap();
    h.step(&json!({"key":"Escape"})).unwrap();
    let state = h.app.session.world_state().unwrap();
    let wire = state.sketch(bridge).unwrap().sketch.wires.first().unwrap();
    assert!(wire.fit[0].dist(Vec3::new(10.0, 20.0, 10.0)) < 1e-6);
    assert!(wire.fit[1].dist(Vec3::new(30.0, 35.0, 40.0)) < 1e-6);
    h.app.run("edit.undo", json!({})).unwrap();
    assert!(h.app.session.world_state().unwrap().sketch(bridge).unwrap().sketch.wires.is_empty());
    h.app.run("edit.redo", json!({})).unwrap();
    assert_eq!(h.app.session.world_state().unwrap().sketch(bridge).unwrap().sketch.wires[0].fit, wire.fit);
}

/// Another sketch's point and an active sketch on the same XY plane, so both lie under one cursor.
fn coplanar_foreign_point() -> (crate::scenario::Harness, u64, u64) {
    let mut h = crate::scenario::Harness::new();
    let other = h.app.run("sketch.create", json!({"plane":"XY","name":"Other"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.point", json!({"point":[10,20]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let active = h.app.run("sketch.create", json!({"plane":"XY","name":"Active"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.set_origin_shown(false);
    h.call("ui.view", json!({"view":"home"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.frames(3);
    (h, other, active)
}

#[test]
fn sketch_tools_never_pick_another_sketchs_point() {
    // Both in plain 2D sketching and while 3D drawing is on (the bridge snap's mode), the
    // sketch-tool pick path must not return a foreign point as a local point plus id.
    for sketch_3d in [false, true] {
        let (mut h, other, active) = coplanar_foreign_point();
        h.app.run("sketch.options", json!({"sketch_3d":sketch_3d})).unwrap();
        h.app.start("sketch.line");
        h.frames(3);
        assert_eq!(crate::sketch3d::drawing(&h.app), sketch_3d);
        let [x, y] = h.at(&json!({"world":[10,20,0]})).unwrap();
        let pos = egui::pos2(x as f32, y as f32);
        let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        let hits = crate::viewport::pick(&h.app, &proj, pos);
        assert!(
            !hits.iter().any(|hit| matches!(hit, crate::viewport::Hit::SketchPoint { sketch, .. } if *sketch == other)),
            "sketch_3d={sketch_3d}: pick leaked a point of sketch {other}: {hits:?}"
        );
        let (at, id) = crate::viewport::sketch_point_at(&h.app, &proj, pos).unwrap();
        assert!(id.is_none(), "sketch_3d={sketch_3d}: snapped to foreign point id {id:?}");
        assert!(at.dist(solidcore_record::Vec2::new(10.0, 20.0)) < 0.5, "{at:?}");
        if !sketch_3d {
            let before = h.app.session.world_state().unwrap().sketch(other).unwrap().sketch.clone();
            // A real 2D line click there creates nothing tied to the other sketch's point.
            h.step(&json!({"click":{"world":[10,20,0]}})).unwrap();
            h.step(&json!({"click":{"world":[40,5,0]}})).unwrap();
            h.step(&json!({"key":"Escape"})).unwrap();
            let state = h.app.session.world_state().unwrap();
            let own = &state.sketch(active).unwrap().sketch;
            assert!(!own.curves.is_empty(), "line was drawn");
            assert_eq!(state.sketch(other).unwrap().sketch, before, "other sketch untouched");
        }
    }
}

#[test]
fn bridge_snap_still_sees_other_sketch_points() {
    let (mut h, other, _) = coplanar_foreign_point();
    h.app.run("sketch.options", json!({"sketch_3d":true})).unwrap();
    h.app.start("sketch.line");
    h.frames(3);
    let [x, y] = h.at(&json!({"world":[10,20,0]})).unwrap();
    let pos = egui::pos2(x as f32, y as f32);
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    assert!(
        crate::viewport::pick_3d(&h.app, &proj, pos)
            .iter()
            .any(|hit| matches!(hit, crate::viewport::Hit::SketchPoint { sketch, .. } if *sketch == other)),
        "the 3D bridge pick offers the other sketch's point"
    );
    let (snapped, kind) = crate::sketch3d::point_at(&h.app, &proj, pos).unwrap();
    assert_eq!(kind, "Pt");
    assert!(snapped.dist(Vec3::new(10.0, 20.0, 0.0)) < 1e-6, "{snapped:?}");
    // Hover highlights the snap target the click will use.
    h.step(&json!({"move":{"world":[10,20,0]}})).unwrap();
    h.frames(2);
    assert!(
        matches!(&h.app.viewport.hover, Some(crate::viewport::Hit::SketchPoint { sketch, .. }) if *sketch == other),
        "{:?}",
        h.app.viewport.hover
    );
}

#[test]
fn second_occurrence_drawn_wire_fit_picks_and_world_drag_stay_aligned() {
    let mut h = crate::scenario::Harness::new();
    let component = h.app.run("component.create", json!({"name":"Part"})).unwrap()["component"].as_u64().unwrap();
    let first = h.app.session.doc.occurrence_of(component).unwrap().id;
    let second = h.app.run("occurrence.copy", json!({"component":component})).unwrap()["occurrence"].as_u64().unwrap();
    h.app.run("occurrence.move", json!({"occurrence":second,"translate":[20,30,40],"axis":[0,0,1],"angle":"90 deg","capture":true})).unwrap();
    h.app.run("component.activate", json!({"component":component,"occurrence":second})).unwrap();
    h.app.run("solid.box", json!({"corner":[17,32,45],"length":2,"width":2,"height":10,"body_name":"SnapVertices"})).unwrap();
    h.app.run("browser.visibility", json!({"paths":[[first]],"visible":false})).unwrap();
    let sketch = h.app.run("sketch.create", json!({"plane":"XY","name":"Wire"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.set_origin_shown(false);
    h.settle();
    h.step(&json!({"click":{"handle":"sketch_palette:options_fold"}})).unwrap();
    h.app.run("sketch.options", json!({"sketch_3d":true})).unwrap();
    assert!(
        crate::sketch3d::active(&h.app),
        "palette click must enable 3D: raw={:?}, world={:?}, status={:?}, active={:?}",
        h.app.session.model.state().sketch(sketch).unwrap().sketch.view,
        h.app.session.world_state().unwrap().sketch(sketch).unwrap().sketch.view,
        h.app.status,
        h.app.session.active_sketch
    );
    h.call("ui.view", json!({"view":"home"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.app.start("sketch.line");
    h.frames(2);
    // The clickable Line instruction toast covers the fitted lower endpoint. This scenario
    // tests model picks, so dismiss the transient instruction before clicking through that area.
    h.app.status = None;
    h.frames(2);
    for world in [Vec3::new(17.0, 32.0, 45.0), Vec3::new(17.0, 32.0, 55.0)] {
        let [x, y] = h.at(&json!({"world":[world.x,world.y,world.z]})).unwrap();
        let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        let screen = egui::pos2(x as f32, y as f32);
        assert!(!crate::scenario::rect_of("toast").is_some_and(|r| r.contains(screen)), "the drawing click must clear the message action");
        let picked = crate::sketch3d::point_at(&h.app, &proj, screen);
        assert!(picked.is_some_and(|(p, _)| p.dist(world) < 1e-5), "world={world:?}, picked={picked:?}, status={:?}", h.app.status);
    }
    h.step(&json!({"click":{"world":[17,32,45]}})).unwrap();
    h.step(&json!({"click":{"world":[17,32,55]}})).unwrap();
    h.step(&json!({"key":"Escape"})).unwrap();
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.tool.is_none());
    let raw = h.app.session.model.state();
    let wire = raw.sketch(sketch).unwrap().sketch.wires.first().expect("actual off-plane clicks create a wire");
    assert!(wire.fit[0].dist(Vec3::new(2.0, 3.0, 5.0)) < 1e-6, "{:?}", wire.fit);
    assert!(wire.fit[1].dist(Vec3::new(2.0, 3.0, 15.0)) < 1e-6, "{:?}", wire.fit);
    let world = h.app.session.world_state().unwrap();
    let shown = &world.sketch(sketch).unwrap().sketch.wires[0];
    assert!(shown.fit[0].dist(Vec3::new(17.0, 32.0, 45.0)) < 1e-6, "{:?}", shown.fit);
    assert!(shown.fit[1].dist(Vec3::new(17.0, 32.0, 55.0)) < 1e-6, "{:?}", shown.fit);
    // A drawn point drags freely, parallel to the sketch plane (the placed sketch's plane).
    let before = h.at(&json!({"world":[17,32,45]})).unwrap();
    let after = h.at(&json!({"world":[21,32,45]})).unwrap();
    let history = h.app.session.undo.len();
    h.call("ui.drag", json!({"x0":before[0],"y0":before[1],"x1":after[0],"y1":after[1],"steps":8}));
    let moved = h.app.session.model.state();
    let fit = &moved.sketch(sketch).unwrap().sketch.wires[0].fit;
    assert!(fit[0].dist(Vec3::new(2.0, -1.0, 5.0)) < 1e-5, "{fit:?}");
    assert!(fit[1].dist(Vec3::new(2.0, 3.0, 15.0)) < 1e-6);
    let placed = h.app.session.world_state().unwrap();
    assert!(placed.sketch(sketch).unwrap().sketch.wires[0].fit[0].dist(Vec3::new(21.0, 32.0, 45.0)) < 1e-5);
    assert_eq!(h.app.session.undo.len(), history + 1);
    h.app.run("edit.undo", json!({})).unwrap();
    let restored = h.app.session.model.state();
    assert!(restored.sketch(sketch).unwrap().sketch.wires[0].fit[0].dist(Vec3::new(2.0, 3.0, 5.0)) < 1e-6);
}

#[test]
fn arc_tool_draws_an_exact_3d_arc_off_the_plane() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"corner":[10,0,5],"length":10,"width":10,"height":5,"body_name":"ArcPicks"})).unwrap();
    let sketch = h.app.run("sketch.create", json!({"plane":"XY","name":"Arcs"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.options", json!({"sketch_3d":true})).unwrap();
    h.app.session.visibility.origin_hidden = Some(true);
    h.call("ui.view", json!({"view":"home"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.frames(3);
    h.app.start("sketch.arc.three_point");
    h.frames(2);
    h.app.status = None;
    // Three top-face vertices: off the XY sketch plane, so the arc tool draws a 3D arc.
    for p in [[10.0, 0.0, 10.0], [20.0, 10.0, 10.0], [10.0, 10.0, 10.0]] {
        let [x, y] = h.at(&json!({"world": p})).unwrap();
        let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        let picked = crate::sketch3d::point_at(&h.app, &proj, egui::pos2(x as f32, y as f32));
        assert!(picked.is_some_and(|(q, _)| (q - Vec3::new(p[0], p[1], p[2])).dist(Vec3::ZERO) < 1e-5), "p={p:?}, picked={picked:?}");
        h.step(&json!({"click": {"world": p}})).unwrap();
    }
    h.step(&json!({"key": "Escape"})).unwrap();
    let state = h.app.session.world_state().unwrap();
    let wires = &state.sketch(sketch).unwrap().sketch.wires;
    assert_eq!(wires.len(), 1, "three off-plane clicks draw one 3D arc");
    let w = &wires[0];
    let arc = w.arc.expect("the drawn arc is stored exactly");
    assert!(arc.center.dist(Vec3::new(15.0, 5.0, 10.0)) < 1e-6, "{:?}", arc.center);
    assert!((arc.radius - 50.0_f64.sqrt()).abs() < 1e-6, "{}", arc.radius);
    assert_eq!(w.fit.len(), 3);
}

#[test]
fn arc3d_button_starts_its_own_tool() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"corner":[10,0,5],"length":10,"width":10,"height":5,"body_name":"Arc3dBtn"})).unwrap();
    let sketch = h.app.run("sketch.create", json!({"plane":"XY","name":"Arcs"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.options", json!({"sketch_3d":true})).unwrap();
    h.app.session.visibility.origin_hidden = Some(true);
    h.call("ui.view", json!({"view":"home"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.frames(3);
    h.app.start("sketch.arc3d");
    assert!(h.app.tool.as_ref().is_some_and(|t| t.cmd == "sketch.arc3d"), "the 3D Arc button starts a tool: {:?}", h.app.status);
    h.frames(2);
    h.app.status = None;
    // Same click order as the 2D arc tool: start, end, then a point on the arc.
    for p in [[10.0, 0.0, 10.0], [10.0, 10.0, 10.0], [20.0, 10.0, 10.0]] {
        h.step(&json!({"click": {"world": p}})).unwrap();
    }
    let state = h.app.session.world_state().unwrap();
    let wires = &state.sketch(sketch).unwrap().sketch.wires;
    assert_eq!(wires.len(), 1, "three clicks draw one exact 3D arc");
    let arc = wires[0].arc.expect("the drawn arc is stored exactly");
    assert!(arc.center.dist(Vec3::new(15.0, 5.0, 10.0)) < 1e-6, "{:?}", arc.center);
    assert!((arc.radius - 50.0_f64.sqrt()).abs() < 1e-6, "{}", arc.radius);
    assert!(arc.start.dist(Vec3::new(10.0, 0.0, 10.0)) < 1e-6, "{:?}", arc.start);
    assert!(arc.end.dist(Vec3::new(10.0, 10.0, 10.0)) < 1e-6, "{:?}", arc.end);
    // With 3D Sketch off the button is not offered: its tool could only fail.
    h.step(&json!({"key": "Escape"})).unwrap();
    assert!(h.app.tool.is_none(), "Escape ends the tool");
    h.app.run("sketch.options", json!({"sketch_3d": false})).unwrap();
    h.app.start("sketch.arc3d");
    assert!(h.app.tool.is_none(), "3D Arc is disabled while 3D Sketch is off");
}

fn move_kind(h: &crate::scenario::Harness) -> (String, String, String) {
    match h.app.dialog.as_ref().map(|d| d.kind.clone()) {
        Some(crate::dialogs::Kind::SketchModify(crate::sketch_modify_ui::Mod::Move { x, y, z, .. })) => (x, y, z),
        _ => panic!("Move/Copy stays open"),
    }
}

fn mm(h: &crate::scenario::Harness, v: &str) -> f64 {
    h.app.session.doc.eval(v, solidcraft_engine::doc::expr::Kind::Length).unwrap()
}

fn has(h: &mut crate::scenario::Harness, handle: &str) -> bool {
    h.at(&json!({"handle":format!("move_{handle}")})).is_ok()
}

/// The preference brings back the X Distance arrow alone, and a clicked point's own arrows.
#[test]
fn the_older_handles_preference_keeps_the_arrows_it_had() {
    use solidcraft_engine::Sel;
    let mut h = crate::scenario::Harness::new();
    let sketch = h.app.run("sketch.create", json!({"plane":"XY","name":"Top"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.session.visibility.origin_hidden = Some(true);
    h.app.run("sketch.options", json!({"sketch_3d":true})).unwrap();
    h.app.run("sketch.point3d", json!({"point":[10,5,20]})).unwrap();
    let point = h.app.run("sketch.point", json!({"point":[-30,-20]})).unwrap()["point"].as_str().unwrap().to_string();
    h.call("ui.view", json!({"view":"home"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.settle();
    let [x, y] = h.at(&json!({"world":[10,5,20]})).unwrap();
    let at = egui::pos2(x as f32, y as f32);
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    // By default a click selects the point and it has no arrows.
    assert!(crate::sketch3d::click(&mut h.app, &proj, at));
    assert!(crate::sketch3d::handle_points(&h.app, &proj).is_empty());

    h.app.preferences.sketch_move_classic = true;
    let tips = crate::sketch3d::handle_points(&h.app, &proj);
    assert_eq!(tips.iter().map(|(n, _)| *n).collect::<Vec<_>>(), ["x", "y", "z", "plane"]);
    // The point itself no longer drags; the X arrow moves it along X only.
    assert!(!crate::sketch3d::drag_start(&h.app, &proj, at));
    let tip = tips[0].1;
    assert!(crate::sketch3d::drag_start(&h.app, &proj, tip));
    crate::sketch3d::drag_to(&mut h.app, &proj, tip + (tip - at));
    crate::sketch3d::drag_end();
    let (_, _, moved) = crate::sketch3d::selected(&h.app).unwrap();
    assert!(moved.x > 10.01, "{moved:?}");
    assert!((moved.y - 5.0).abs() < 1e-6 && (moved.z - 20.0).abs() < 1e-6, "{moved:?}");

    h.step(&json!({"start":"sketch.move"})).unwrap();
    h.app.dialog.as_mut().unwrap().inputs[0].items = vec![Sel::SketchPoint { id: format!("{sketch}:{point}") }];
    h.frames(3);
    assert!(!has(&mut h, "x") && !has(&mut h, "ball"), "no triad");
    assert!(h.at(&json!({"handle":"arrow"})).is_ok(), "the X Distance arrow");
}

/// Move/Copy in a 3D sketch has the triad in the sketch's frame: X, Y and Z arrows, the three
/// plane squares, the ring for Angle and the ball. Looking straight down on the sketch the Z
/// arrow points at the viewer; it is drawn up and to the left, and dragging it lifts the point
/// by the drag's length at the view's scale. The lifted point is picked again by a click, and
/// moves on.
#[test]
fn move_copy_triad_in_a_3d_sketch_lifts_and_moves_drawn_points() {
    use solidcraft_engine::Sel;
    let mut h = crate::scenario::Harness::new();
    let sketch = h.app.run("sketch.create", json!({"plane":"XY","name":"Top"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.session.visibility.origin_hidden = Some(true);
    h.app.run("sketch.options", json!({"sketch_3d":true})).unwrap();
    let point = h.app.run("sketch.point", json!({"point":[10,5]})).unwrap()["point"].as_str().unwrap().to_string();
    h.app.run("sketch.point", json!({"point":[-30,-20]})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.settle();
    h.step(&json!({"start":"sketch.move"})).unwrap();
    h.app.dialog.as_mut().unwrap().inputs[0].items = vec![Sel::SketchPoint { id: format!("{sketch}:{point}") }];
    h.frames(3);
    for handle in ["x", "y", "z", "xy", "yz", "xz", "rz", "ball"] {
        assert!(has(&mut h, handle), "the 3D sketch triad shows {handle}");
    }
    assert!(!has(&mut h, "rx") && !has(&mut h, "ry"), "Angle turns about the normal only");
    let [x0, y0] = h.at(&json!({"handle":"move_z"})).unwrap();
    let base = h.at(&json!({"world":[10,5,0]})).unwrap();
    assert!(x0 < base[0] - 20.0 && y0 < base[1] - 20.0, "the head-on Z arrow is drawn up and to the left");
    let along = 40.0 / std::f64::consts::SQRT_2;
    h.call("ui.drag", json!({"x0":x0,"y0":y0,"x1":x0-along,"y1":y0-along,"steps":8}));
    let lift = mm(&h, &move_kind(&h).2);
    assert!(lift > 0.5, "dragging the Z arrow up sets Z Distance: {lift}");
    let history = h.app.session.undo.len();
    h.frames(2);
    h.step(&json!({"key":"Enter"})).unwrap();
    h.settle();
    let sk = h.app.session.model.state().sketch(sketch).unwrap().sketch.clone();
    let wire =
        sk.wires.iter().find(|w| w.fit.iter().any(|p| (p.x - 10.0).abs() < 1e-6 && (p.y - 5.0).abs() < 1e-6)).expect("the point left the plane");
    assert!((wire.fit[0].z - lift).abs() < 1e-6, "lifted {} mm, Z Distance {lift}", wire.fit[0].z);
    assert_eq!(h.app.session.undo.len(), history + 1, "one undo step");
    // The lifted point is picked by a click in Move/Copy, and moves 5 mm along X.
    h.step(&json!({"start":"sketch.move"})).unwrap();
    h.step(&json!({"click":{"world":[10,5,lift]}})).unwrap();
    let picked = h.app.dialog.as_ref().unwrap().inputs[0].items.clone();
    assert_eq!(picked, vec![Sel::SketchPoint { id: format!("{}@0", wire.id) }]);
    if let Some(crate::dialogs::Kind::SketchModify(crate::sketch_modify_ui::Mod::Move { x, .. })) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *x = "5 mm".into();
    }
    h.frames(2);
    h.step(&json!({"key":"Enter"})).unwrap();
    h.settle();
    let moved = h.app.session.model.state().sketch(sketch).unwrap().sketch.wires.iter().find(|w| w.id == wire.id).unwrap().fit[0];
    assert!(moved.dist(solidcore_record::Vec3::new(15.0, 5.0, lift)) < 1e-6, "{moved:?}");
}

/// In a flat sketch the triad moves things in the sketch plane: X and Y arrows, the XY square,
/// the ring and the ball, and no Z. It travels with the move (it stays its own size): dragging
/// the square sets X and Y Distance, and the triad's centre follows.
#[test]
fn move_copy_triad_in_a_flat_sketch_moves_in_the_plane_and_follows() {
    use solidcraft_engine::Sel;
    let mut h = crate::scenario::Harness::new();
    let sketch = h.app.run("sketch.create", json!({"plane":"XY","name":"Flat"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.session.visibility.origin_hidden = Some(true);
    let point = h.app.run("sketch.point", json!({"point":[10,5]})).unwrap()["point"].as_str().unwrap().to_string();
    h.app.run("sketch.point", json!({"point":[-30,-20]})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.settle();
    h.step(&json!({"start":"sketch.move"})).unwrap();
    h.app.dialog.as_mut().unwrap().inputs[0].items = vec![Sel::SketchPoint { id: format!("{sketch}:{point}") }];
    h.frames(3);
    for handle in ["x", "y", "xy", "rz", "ball"] {
        assert!(has(&mut h, handle), "the flat sketch triad shows {handle}");
    }
    for handle in ["z", "yz", "xz", "rx", "ry"] {
        assert!(!has(&mut h, handle), "no {handle} in a flat sketch");
    }
    let [bx, by] = h.at(&json!({"handle":"move_ball"})).unwrap();
    let [sx, sy] = h.at(&json!({"handle":"move_xy"})).unwrap();
    h.call("ui.drag", json!({"x0":sx,"y0":sy,"x1":sx+60.0,"y1":sy-30.0,"steps":8}));
    let (x, y, z) = move_kind(&h);
    let (dx, dy) = (mm(&h, &x), mm(&h, &y));
    assert!(dx > 0.1 && dy > 0.1 && mm(&h, &z).abs() < 1e-9, "the square moves along X and Y only: {x}, {y}, {z}");
    h.frames(2);
    let [ax, ay] = h.at(&json!({"handle":"move_ball"})).unwrap();
    assert!(ax > bx + 30.0 && ay < by - 10.0, "the triad follows the move: from {bx},{by} to {ax},{ay}");
    h.step(&json!({"key":"Enter"})).unwrap();
    h.settle();
    let st = h.app.session.model.state();
    let p = st.sketch(sketch).unwrap().sketch.points.iter().find(|q| q.id == point).unwrap().pos;
    assert!((p.x - (10.0 + dx)).abs() < 1e-6 && (p.y - (5.0 + dy)).abs() < 1e-6, "{p:?}");
}

/// Shared setup: a box whose top-face vertices sit off the active 3D sketch's XY plane.
fn box_and_3d_sketch(name: &str) -> (crate::scenario::Harness, u64) {
    let mut h = crate::scenario::Harness::new();
    h.app.run("solid.box", json!({"corner":[10,0,5],"length":10,"width":10,"height":5,"body_name":name})).unwrap();
    let sketch = h.app.run("sketch.create", json!({"plane":"XY","name":name})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.options", json!({"sketch_3d":true})).unwrap();
    h.app.session.visibility.origin_hidden = Some(true);
    h.call("ui.view", json!({"view":"home"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.frames(3);
    (h, sketch)
}

/// Three top-face box vertices: off the XY sketch plane, so the 3D tools keep them as world points.
const OFF_PLANE: [[f64; 3]; 3] = [[10.0, 0.0, 10.0], [20.0, 10.0, 10.0], [10.0, 10.0, 10.0]];

#[test]
fn line3d_tool_draws_polyline_off_plane() {
    let (mut h, sketch) = box_and_3d_sketch("Line3dPicks");
    h.app.start("sketch.line3d");
    assert!(h.app.tool.as_ref().is_some_and(|t| t.cmd == "sketch.line3d"), "the 3D Line button starts a tool: {:?}", h.app.status);
    h.frames(2);
    h.app.status = None;
    for p in OFF_PLANE {
        let [x, y] = h.at(&json!({"world": p})).unwrap();
        let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        let picked = crate::sketch3d::point_at(&h.app, &proj, egui::pos2(x as f32, y as f32));
        assert!(picked.is_some_and(|(q, _)| (q - Vec3::new(p[0], p[1], p[2])).dist(Vec3::ZERO) < 1e-5), "p={p:?}, picked={picked:?}");
        h.step(&json!({"click": {"world": p}})).unwrap();
    }
    h.step(&json!({"key": "Escape"})).unwrap();
    let state = crate::open_meshing::session_world(&h.app.session);
    let wires = &state.sketch(sketch).unwrap().sketch.wires;
    assert_eq!(wires.len(), 2, "three off-plane clicks finish as two joined 3D segments");
    let expected = [[Vec3::new(10.0, 0.0, 10.0), Vec3::new(20.0, 10.0, 10.0)], [Vec3::new(20.0, 10.0, 10.0), Vec3::new(10.0, 10.0, 10.0)]];
    for (w, pair) in wires.iter().zip(expected) {
        assert!(w.fit[0].dist(pair[0]) < 1e-6 && w.fit[1].dist(pair[1]) < 1e-6, "{:?}", w.fit);
    }
}

#[test]
fn spline3d_tool_fit_points_off_plane() {
    let (mut h, sketch) = box_and_3d_sketch("Spline3dPicks");
    h.app.start("sketch.spline3d");
    assert!(h.app.tool.as_ref().is_some_and(|t| t.cmd == "sketch.spline3d"), "the 3D Spline button starts a tool: {:?}", h.app.status);
    h.frames(2);
    h.app.status = None;
    for p in OFF_PLANE {
        h.step(&json!({"click": {"world": p}})).unwrap();
    }
    h.step(&json!({"key": "Escape"})).unwrap();
    let state = crate::open_meshing::session_world(&h.app.session);
    let wires = &state.sketch(sketch).unwrap().sketch.wires;
    assert_eq!(wires.len(), 1, "three off-plane clicks finish as one 3D spline");
    let w = &wires[0];
    assert_eq!(w.fit.len(), 3, "{:?}", w.fit);
    for (fit, p) in w.fit.iter().zip(OFF_PLANE) {
        assert!(fit.dist(Vec3::new(p[0], p[1], p[2])) < 1e-6, "{:?}", w.fit);
    }
}

#[test]
fn point3d_tool_places_points_and_stays() {
    let (mut h, sketch) = box_and_3d_sketch("Point3dPicks");
    h.app.start("sketch.point3d");
    assert!(h.app.tool.as_ref().is_some_and(|t| t.cmd == "sketch.point3d"), "the 3D Point button starts a tool: {:?}", h.app.status);
    h.frames(2);
    h.app.status = None;
    for p in [OFF_PLANE[0], OFF_PLANE[1]] {
        h.step(&json!({"click": {"world": p}})).unwrap();
    }
    let state = crate::open_meshing::session_world(&h.app.session);
    let wires = &state.sketch(sketch).unwrap().sketch.wires;
    assert_eq!(wires.len(), 2, "two clicks place two drawn 3D points");
    assert!(wires.iter().all(|w| w.fit.len() == 1), "{:?}", wires.iter().map(|w| &w.fit).collect::<Vec<_>>());
    assert!(wires[0].fit[0].dist(Vec3::new(10.0, 0.0, 10.0)) < 1e-6, "{:?}", wires[0].fit);
    assert!(wires[1].fit[0].dist(Vec3::new(20.0, 10.0, 10.0)) < 1e-6, "{:?}", wires[1].fit);
    assert!(h.app.tool.as_ref().is_some_and(|t| t.cmd == "sketch.point3d"), "the tool stays for the next point");
}
