//! The primitives' handle: the marker places a new primitive on a face while its dialog is open,
//! moves one from Edit Feature, and an edit opened again finds it where it was moved.

use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::FeatureKind;
use solidcraft_engine::geom::Vec3;

use super::{footprint, frame, move_to};
use crate::dialogs::Kind;
use crate::scenario::Harness;
use crate::viewport::Hit;

/// An 80 × 50 × 10 plate seen from the top, with the handles on.
fn plate() -> Harness {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.preferences.placement_handles = true;
    h.app.run("solid.box", json!({"length": 80, "width": 50, "height": 10})).unwrap();
    h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
    h.settle();
    h
}

fn start(h: &mut Harness, cmd: &str) {
    h.step(&json!({"start": cmd})).unwrap();
    h.settle();
}

fn drag(h: &mut Harness, to: [f64; 3]) {
    assert!(h.at(&json!({"handle": "primitive:position"})).is_ok(), "the marker is published");
    assert!(h.at(&json!({"handle": "primitive:rotate"})).is_err(), "a primitive has no ring");
    h.step(&json!({"drag": [{"handle": "primitive:position"}, {"world": to}], "steps": 12})).unwrap();
    h.settle();
}

fn ok(h: &mut Harness) {
    h.step(&json!({"click": {"handle": "dialog:OK"}})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "OK closed the dialog: {:?}", h.app.dialog.as_ref().map(|d| &d.error));
}

fn drag_ok(h: &mut Harness, to: [f64; 3]) {
    drag(h, to);
    ok(h);
}

fn value(h: &Harness, e: &str) -> f64 {
    h.app.session.doc.eval(e, solidcraft_engine::doc::expr::Kind::Length).unwrap()
}

fn point(h: &Harness, p: &solidcraft_engine::doc::PointExpr) -> Vec3 {
    Vec3::new(value(h, &p[0]), value(h, &p[1]), value(h, &p[2]))
}

/// The open dialog's typed placement under `key` (the corner or centre fields), in mm.
fn typed(h: &Harness, key: &str) -> Vec3 {
    let v = h.app.dialog.as_ref().and_then(|d| d.extra.get(key)).and_then(|v| v.as_array()).expect("typed fields");
    let c = |i: usize| value(h, v[i].as_str().expect("a typed field"));
    Vec3::new(c(0), c(1), c(2))
}

/// The last feature: the primitive just made.
fn last(h: &Harness) -> (String, FeatureKind) {
    let f = h.app.session.doc.features.last().expect("a feature");
    (f.name.clone(), f.kind.clone())
}

/// Edit Feature on `name` again: the marker stands on screen where `world` is.
fn reopened_at(h: &mut Harness, name: &str, world: [f64; 3]) {
    h.step(&json!({"call": "ui.editFeature", "params": {"feature": name}})).unwrap();
    h.settle();
    let at = h.at(&json!({"handle": "primitive:position"})).expect("the marker is published again");
    let expect = h.at(&json!({"world": world})).unwrap();
    assert!((at[0] - expect[0]).hypot(at[1] - expect[1]) < 2.0, "the edit opens with the marker where it was moved: {at:?} {expect:?}");
}

#[test]
fn marker_stands_a_new_box_on_the_face_through_its_corner_fields() {
    let mut h = plate();
    start(&mut h, "solid.box");
    drag(&mut h, [40.0, 25.0, 10.0]);
    let d = h.app.dialog.as_ref().expect("the dialog is open");
    assert!(matches!(d.kind, Kind::Primitive { at: None, .. }), "the typed placement stays: the fields show it");
    assert!(typed(&h, "corner").dist(Vec3::new(30.0, 15.0, 10.0)) < 1e-3, "the corner fields took the drop: {:?}", d.extra.get("corner"));
    ok(&mut h);
    let (name, kind) = last(&h);
    let FeatureKind::Box { corner, .. } = &kind else { panic!("a Box: {kind:?}") };
    // 20 × 20 × 20 by default, centred over the drop and standing on the top.
    assert!(point(&h, corner).dist(Vec3::new(30.0, 15.0, 10.0)) < 1e-3, "the corner: {:?}", point(&h, corner));
    reopened_at(&mut h, &name, [40.0, 25.0, 10.0]);
}

#[test]
fn marker_moves_a_box_placed_by_its_centre_through_its_centre_fields() {
    let mut h = plate();
    let made = h.app.run("solid.box", json!({"length": 10, "width": 6, "height": 8, "center": [20, 15, 14], "operation": "join"})).unwrap();
    let name = made["name"].as_str().unwrap().to_string();
    h.settle();
    h.step(&json!({"call": "ui.editFeature", "params": {"feature": name}})).unwrap();
    h.settle();
    // Edit Feature reads the stored corner back; choosing Center keeps the box where it is.
    let at = h.at(&json!({"handle": "primitive:position"})).unwrap();
    let expect = h.at(&json!({"world": [20, 15, 10]})).unwrap();
    assert!((at[0] - expect[0]).hypot(at[1] - expect[1]) < 2.0, "the marker stands under the centre, on the bottom: {at:?} {expect:?}");
    {
        let d = h.app.dialog.as_mut().expect("a dialog");
        d.extra.remove("corner");
        d.extra.insert("center".into(), json!(["20 mm", "15 mm", "14 mm"]));
    }
    drag(&mut h, [60.0, 35.0, 10.0]);
    let d = h.app.dialog.as_ref().expect("the dialog is open");
    assert!(typed(&h, "center").dist(Vec3::new(60.0, 35.0, 14.0)) < 1e-3, "the centre fields took the drop: {:?}", d.extra.get("center"));
    ok(&mut h);
    let f = h.app.session.doc.find_feature(&name).expect("the Box is still there");
    let FeatureKind::Box { corner, .. } = &f.kind else { panic!("a Box: {:?}", f.kind) };
    assert!(point(&h, corner).dist(Vec3::new(55.0, 32.0, 10.0)) < 1e-3, "moved: {:?}", point(&h, corner));
    reopened_at(&mut h, &name, [60.0, 35.0, 10.0]);
}

#[test]
fn a_dropped_box_stands_out_from_a_side_face_and_under_the_underside() {
    let mut extra = serde_json::Map::new();
    let size = Vec3::new(20.0, 10.0, 6.0);
    let written = crate::primitive_placement::dropped(&mut extra, Vec3::new(80.0, 25.0, 5.0), Vec3::X, size);
    assert_eq!(written, Some(json!(["80 mm", "20 mm", "2 mm"])), "out from the +X side, centred across it");
    assert_eq!(extra.get("corner"), written.as_ref());
    extra.remove("corner");
    extra.insert("center".into(), json!(["0", "0", "0"]));
    let written = crate::primitive_placement::dropped(&mut extra, Vec3::new(40.0, 25.0, 0.0), -Vec3::Z, size);
    assert_eq!(written, Some(json!(["40 mm", "25 mm", "-3 mm"])), "below the underside: the centre half the height down");
    assert!(!extra.contains_key("corner"), "the choice of Center stays");
}

#[test]
fn marker_stands_a_new_cylinder_on_the_face_through_its_face_input() {
    let mut h = plate();
    start(&mut h, "solid.cylinder");
    // Nothing was pre-selected: the marker starts where the command would put it, the origin.
    drag(&mut h, [40.0, 25.0, 10.0]);
    let d = h.app.dialog.as_ref().expect("the dialog is still open");
    match d.inputs[0].items.as_slice() {
        [Sel::Face { point, .. }] => assert!(point.dist(Vec3::new(40.0, 25.0, 10.0)) < 0.5, "the Face input: {point:?}"),
        other => panic!("one face place in the Face input, not {other:?}"),
    }
    let Kind::Primitive { at, .. } = &d.kind else { panic!("a primitive") };
    assert!(at.is_some_and(|(p, n)| p.dist(Vec3::new(40.0, 25.0, 10.0)) < 0.5 && n == Some(Vec3::Z)), "it stands where the input is: {at:?}");
    ok(&mut h);
    let (name, kind) = last(&h);
    let FeatureKind::Cylinder { base, axis, .. } = &kind else { panic!("a Cylinder: {kind:?}") };
    assert!(point(&h, base).dist(Vec3::new(40.0, 25.0, 10.0)) < 0.5, "stands where it was dropped: {:?}", point(&h, base));
    assert!(axis.0.dist(Vec3::Z) < 1e-9, "along the face normal: {:?}", axis.0);
    reopened_at(&mut h, &name, [40.0, 25.0, 10.0]);
}

#[test]
fn dragging_a_cylinder_with_a_reference_edge_measures_its_distance_again() {
    let mut h = plate();
    start(&mut h, "solid.cylinder");
    h.step(&json!({"click": {"world": [30, 25, 10]}})).unwrap();
    h.settle();
    h.step(&json!({"click": {"world": [0, 20, 10]}})).unwrap();
    h.settle();
    let distance = |h: &Harness| match &h.app.dialog.as_ref().expect("a dialog").kind {
        Kind::Primitive { refs, .. } => value(h, &refs.distance[0]),
        other => panic!("a primitive: {other:?}"),
    };
    assert!((distance(&h) - 30.0).abs() < 0.5, "measured from the left edge: {}", distance(&h));
    drag(&mut h, [50.0, 30.0, 10.0]);
    assert!((distance(&h) - 50.0).abs() < 0.5, "measured again from the drop: {}", distance(&h));
    ok(&mut h);
    let (_, kind) = last(&h);
    let FeatureKind::Cylinder { base, references, .. } = &kind else { panic!("a Cylinder: {kind:?}") };
    assert_eq!(references.len(), 1);
    assert!(point(&h, base).dist(Vec3::new(50.0, 30.0, 10.0)) < 0.5, "{:?}", point(&h, base));
}

#[test]
fn marker_stands_a_new_cone_on_the_face() {
    let mut h = plate();
    start(&mut h, "solid.cone");
    drag_ok(&mut h, [40.0, 25.0, 10.0]);
    let (name, kind) = last(&h);
    let FeatureKind::Cone { base, axis, .. } = &kind else { panic!("a Cone: {kind:?}") };
    assert!(point(&h, base).dist(Vec3::new(40.0, 25.0, 10.0)) < 0.5, "{:?}", point(&h, base));
    assert!(axis.0.dist(Vec3::Z) < 1e-9, "{:?}", axis.0);
    reopened_at(&mut h, &name, [40.0, 25.0, 10.0]);
}

#[test]
fn marker_moves_a_sphere_from_edit_feature() {
    let mut h = plate();
    let made = h.app.run("solid.sphere", json!({"radius": 4, "center": [15, 15, 10], "operation": "join"})).unwrap();
    let name = made["name"].as_str().unwrap().to_string();
    h.settle();
    reopened_at(&mut h, &name, [15.0, 15.0, 10.0]);
    drag_ok(&mut h, [60.0, 35.0, 10.0]);
    let f = h.app.session.doc.find_feature(&name).expect("the Sphere is still there");
    let FeatureKind::Sphere { center, .. } = &f.kind else { panic!("a Sphere: {:?}", f.kind) };
    assert!(point(&h, center).dist(Vec3::new(60.0, 35.0, 10.0)) < 0.5, "moved: {:?}", point(&h, center));
    h.step(&json!({"expect": {"errors": 0, "bodies": 1}})).unwrap();
    reopened_at(&mut h, &name, [60.0, 35.0, 10.0]);
}

#[test]
fn marker_stands_a_new_torus_on_the_face() {
    let mut h = plate();
    start(&mut h, "solid.torus");
    drag_ok(&mut h, [40.0, 25.0, 10.0]);
    let (name, kind) = last(&h);
    let FeatureKind::Torus { center, .. } = &kind else { panic!("a Torus: {kind:?}") };
    assert!(point(&h, center).dist(Vec3::new(40.0, 25.0, 10.0)) < 0.5, "{:?}", point(&h, center));
    reopened_at(&mut h, &name, [40.0, 25.0, 10.0]);
}

#[test]
fn marker_stands_a_new_coil_on_the_face() {
    let mut h = plate();
    start(&mut h, "solid.coil");
    drag_ok(&mut h, [40.0, 25.0, 10.0]);
    let (name, kind) = last(&h);
    let FeatureKind::Coil { base, axis, .. } = &kind else { panic!("a Coil: {kind:?}") };
    assert!(point(&h, base).dist(Vec3::new(40.0, 25.0, 10.0)) < 0.5, "{:?}", point(&h, base));
    assert!(axis.0.dist(Vec3::Z) < 1e-9, "{:?}", axis.0);
    reopened_at(&mut h, &name, [40.0, 25.0, 10.0]);
}

#[test]
fn a_coil_with_a_sketch_profile_has_no_marker() {
    let mut h = plate();
    // A circle on the plate's top: a profile the Coil can sweep.
    let sketch = h.app.run("sketch.create", json!({"plane": {"face": [40, 25, 10]}})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.circle.center", json!({"center": [60, 25], "radius": 3})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.settle();
    start(&mut h, "solid.coil");
    assert!(h.at(&json!({"handle": "primitive:position"})).is_ok(), "a coil of its own section has the marker");
    h.app.dialog.as_mut().expect("a dialog").inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    h.frames(2);
    assert!(h.at(&json!({"handle": "primitive:position"})).is_err(), "with a profile picked, no marker");
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].1["sketch"], json!(sketch), "the coil is made from the profile: {:?}", commands[0].1);
}

/// With the preference off (the default) no primitive's dialog shows the marker.
#[test]
fn with_the_preference_off_no_primitive_has_a_marker() {
    let mut h = plate();
    h.app.preferences.placement_handles = false;
    for cmd in ["solid.box", "solid.cone", "solid.coil", "solid.cylinder", "solid.sphere", "solid.torus"] {
        start(&mut h, cmd);
        assert!(h.app.dialog.is_some(), "{cmd}: a dialog");
        assert!(h.at(&json!({"handle": "primitive:position"})).is_err(), "{cmd}: no marker is published");
        h.app.dialog = None;
    }
}

/// The open dialog's marker place and the direction the solid grows from it.
fn place(h: &Harness) -> (Vec3, Vec3) {
    frame(&h.app, h.app.dialog.as_ref().expect("a dialog")).expect("a place")
}

/// The open dialog's footprint at its marker.
fn outline(h: &Harness) -> Vec<Vec3> {
    let (base, n) = place(h);
    footprint(&h.app, h.app.dialog.as_ref().expect("a dialog"), base, n)
}

/// The marker dropped on the face at `p`, as one frame of a drag does; whether it took it. The
/// frame after is not run, so the place read next is the one the drop itself left.
fn drop_on(h: &mut Harness, p: [f64; 3]) -> bool {
    let Some(Sel::Face { body, index, point }) = crate::dialogs::face_sel(&h.app.session, Vec3::new(p[0], p[1], p[2])) else {
        panic!("a face at {p:?}")
    };
    let mut d = h.app.dialog.take().expect("a dialog");
    let took = move_to(&h.app, &mut d, &[Hit::Face { body, index, point }], 0);
    h.app.dialog = Some(d);
    took
}

#[test]
fn a_box_dropped_on_a_side_face_stands_on_it_and_reopens_there() {
    let mut h = plate();
    start(&mut h, "solid.box");
    assert!(drop_on(&mut h, [80.0, 25.0, 5.0]), "the side face takes the box");
    let (base, n) = place(&h);
    assert!(base.dist(Vec3::new(80.0, 25.0, 5.0)) < 1e-3 && n.dist(Vec3::X) < 1e-9, "on the side face: {base:?} {n:?}");
    assert!(outline(&h).iter().all(|p| (p.x - 80.0).abs() < 1e-6), "the outline lies on the side face: {:?}", outline(&h));
    ok(&mut h);
    let (name, _) = last(&h);
    h.step(&json!({"call": "ui.editFeature", "params": {"feature": name}})).unwrap();
    h.settle();
    let (base, n) = place(&h);
    assert!(base.dist(Vec3::new(80.0, 25.0, 5.0)) < 1e-3 && n.dist(Vec3::X) < 1e-9, "reopened on the side face: {base:?} {n:?}");
}

#[test]
fn a_side_drop_does_not_follow_another_box_with_the_same_fields() {
    let mut h = plate();
    start(&mut h, "solid.box");
    assert!(drop_on(&mut h, [80.0, 25.0, 5.0]));
    let typed = h.app.dialog.as_ref().unwrap().extra.get("corner").cloned().expect("the corner fields");
    // Another design with no plate: the same fields stand the box on nothing, so its marker is
    // under it.
    let mut other = Harness::new();
    other.app.preferences.placement_handles = true;
    start(&mut other, "solid.box");
    other.app.dialog.as_mut().unwrap().extra.insert("corner".into(), typed);
    let (base, n) = place(&other);
    assert!(n.dist(Vec3::Z) < 1e-9 && base.dist(Vec3::new(90.0, 25.0, -5.0)) < 1e-3, "the middle of its bottom: {base:?} {n:?}");
}

#[test]
fn a_box_on_a_vertex_has_its_marker_and_outline_at_its_bottom() {
    let mut h = plate();
    h.app.session.selection = vec![Sel::Vertex { body: "Body1".into(), point: Vec3::new(80.0, 50.0, 10.0) }];
    start(&mut h, "solid.box");
    // The box is centred on the vertex (20 high), so its bottom is 10 below it.
    let (base, n) = place(&h);
    assert!(base.dist(Vec3::new(80.0, 50.0, 0.0)) < 1e-6 && n.dist(Vec3::Z) < 1e-9, "{base:?} {n:?}");
    assert!(outline(&h).iter().all(|p| p.z.abs() < 1e-6), "the outline is the box's bottom: {:?}", outline(&h));
}

#[test]
fn a_box_moved_by_its_marker_drops_its_point_pick() {
    let mut h = plate();
    start(&mut h, "solid.box");
    {
        let s = &h.app.session;
        let d = h.app.dialog.as_mut().unwrap();
        d.pick(s, Sel::Vertex { body: "Body1".into(), point: Vec3::new(0.0, 0.0, 10.0) });
    }
    assert!(!h.app.dialog.as_ref().unwrap().inputs[0].items.is_empty(), "the point is picked");
    assert!(drop_on(&mut h, [40.0, 25.0, 10.0]));
    let d = h.app.dialog.as_ref().unwrap();
    assert!(d.inputs[0].items.is_empty(), "the pick no longer says where the box is: {:?}", d.inputs[0].items);
    assert!(crate::canvas::anchor(&h.app, d).is_none(), "nothing on the canvas stays at the old point");
}

#[test]
fn a_torus_dropped_on_a_side_face_is_outlined_flat_as_it_is_made() {
    let mut h = plate();
    start(&mut h, "solid.torus");
    assert!(drop_on(&mut h, [80.0, 25.0, 5.0]));
    let o = outline(&h);
    assert!(!o.is_empty() && o.iter().all(|p| (p.z - 5.0).abs() < 1e-6), "flat about its centre: {o:?}");
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert!(commands[0].1.get("axis").is_none(), "the torus has no axis to tilt: {:?}", commands[0].1);
}

#[test]
fn a_cylinder_follows_its_marker_in_the_same_frame() {
    let mut h = plate();
    start(&mut h, "solid.cylinder");
    assert!(drop_on(&mut h, [40.0, 25.0, 10.0]));
    let (base, n) = place(&h);
    assert!(base.dist(Vec3::new(40.0, 25.0, 10.0)) < 0.5 && n.dist(Vec3::Z) < 1e-9, "no frame behind the pointer: {base:?}");
}

#[test]
fn a_cone_coil_or_cylinder_is_not_dropped_on_a_curved_face() {
    let mut h = plate();
    h.app.run("solid.cylinder", json!({"radius": 10, "height": 20, "base": [40, 25, 10], "operation": "new"})).unwrap();
    h.settle();
    for cmd in ["solid.cone", "solid.coil", "solid.cylinder"] {
        start(&mut h, cmd);
        let before = place(&h);
        assert!(!drop_on(&mut h, [50.0, 25.0, 20.0]), "{cmd}: a curved face has no one axis to stand on");
        assert_eq!(place(&h), before, "{cmd}: it stays where it was");
        h.app.dialog = None;
    }
}

#[test]
fn a_tilted_cone_placed_at_a_point_keeps_its_axis_for_the_marker() {
    let mut h = plate();
    let made = h
        .app
        .run("solid.cone", json!({"radius": 5, "top_radius": 0, "height": 10, "base": [20, 20, 30], "axis": [1, 0, 0], "operation": "new"}))
        .unwrap();
    let name = made["name"].as_str().unwrap().to_string();
    h.settle();
    h.step(&json!({"call": "ui.editFeature", "params": {"feature": name}})).unwrap();
    h.settle();
    if let Some(Kind::Primitive { at, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *at = Some((Vec3::new(20.0, 20.0, 30.0), None));
    }
    let (_, n) = place(&h);
    assert!(n.dist(Vec3::X) < 1e-9, "the axis the cone is made along: {n:?}");
    assert!(outline(&h).iter().all(|p| (p.x - 20.0).abs() < 1e-6), "the base circle across that axis");
}

#[test]
fn a_cylinder_with_a_reference_edge_reopens_with_its_marker_and_outline_on_its_face() {
    let mut h = plate();
    let made = h
        .app
        .run("solid.cylinder", json!({"radius": 4, "height": 8, "base": [30, 25, 10], "axis": [0, 0, 1], "references": [{"edge": [0, 20, 10], "distance": "30 mm"}], "operation": "join"}))
        .unwrap();
    let name = made["name"].as_str().unwrap().to_string();
    h.settle();
    reopened_at(&mut h, &name, [30.0, 25.0, 10.0]);
    let o = outline(&h);
    assert!(!o.is_empty() && o.iter().all(|p| (p.z - 10.0).abs() < 1e-6 && (p.dist(Vec3::new(30.0, 25.0, 10.0)) - 4.0).abs() < 1e-6), "{o:?}");
}
