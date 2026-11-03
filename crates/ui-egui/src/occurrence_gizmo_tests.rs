//! Regression for the existing occurrence Move/Copy arrows, independent of clipboard paste.
use crate::{
    context_menu::{self, Target},
    scenario::Harness,
};
use serde_json::json;
use solidcraft_engine::{Sel, doc, geom::Vec3};

#[test]
fn occurrence_gizmo_drag_matches_numeric_input_and_preserves_joints_capture_and_undo() {
    let mut h = Harness::new();
    let part = h.app.run("component.create", json!({"name":"Block","grounded":false})).unwrap();
    let first = part["occurrence"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    let second = h.app.run("occurrence.copy", json!({"occurrence":first,"translate":[50,0,0]})).unwrap()["occurrence"].as_u64().unwrap();
    h.app
        .run(
            "joint.create",
            json!({"name":"Fixed peer","type":"rigid","flip":true,"a":{"occurrence":0,"point":[50,0,0]},"b":{"occurrence":second,"point":[50,0,0]}}),
        )
        .unwrap();
    let joints = serde_json::to_value(&h.app.session.doc.assembly.joints).unwrap();
    let original = h.app.session.doc.to_json();
    let initial = doc::apply_point(&h.app.session.occurrence_placement(&[first]).unwrap().world, Vec3::ZERO);
    let peer = doc::apply_point(&h.app.session.occurrence_placement(&[second]).unwrap().world, Vec3::ZERO);
    h.app.cam.target = initial;
    h.app.cam.distance = 100.;
    h.frames(3);
    let target = Target::Occurrence { path: vec![first] };
    let item = context_menu::items(&h.app, &target).into_iter().find(|item| item.id == "ui.moveOccurrence").unwrap();
    context_menu::run_item(&mut h.app, &item, egui::Pos2::ZERO);
    h.frames(3);
    let camera = h.app.cam;
    let start = crate::gizmo::handle_point("x", Vec3::ZERO, 60.).unwrap();
    let end = start + Vec3::X * 12.;
    h.step(&json!({"drag":[{"world":[start.x,start.y,start.z]},{"world":[end.x,end.y,end.z]}],"steps":12})).unwrap();
    h.frames(3);
    let arrow = doc::apply_point(&h.app.session.occurrence_placement(&[first]).unwrap().world, Vec3::ZERO);
    assert!(arrow.x > initial.x + 1., "visible arrow must actually move this occurrence: {initial:?}->{arrow:?}");
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.session.doc.to_json(), original, "drag is a pending position");
    assert_eq!(doc::apply_point(&h.app.session.occurrence_placement(&[second]).unwrap().world, Vec3::ZERO), peer);
    assert_eq!(serde_json::to_value(&h.app.session.doc.assembly.joints).unwrap(), joints);
    h.app.run("component.revert_position", json!({"occurrences":[first]})).unwrap();
    h.app.tree.occurrence_move.as_mut().unwrap().translate = [0.; 3];
    h.frames(2);
    let at = h.call("ui.at", json!({"handle":"occurrence-move:distance:0"}));
    h.call("ui.click", json!({"x":at["result"][0],"y":at["result"][1],"double":true}));
    h.frames(2);
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":format!("{}",arrow.x-initial.x)})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    h.frames(3);
    let numeric = doc::apply_point(&h.app.session.occurrence_placement(&[first]).unwrap().world, Vec3::ZERO);
    assert!((numeric - arrow).len() < 1e-6, "numeric and arrow routes must agree: {numeric:?}, {arrow:?}");
    let undo = h.app.session.undo.len();
    h.app.run("component.capture_position", json!({})).unwrap();
    h.app.tree.occurrence_move = None;
    assert_eq!(h.app.session.undo.len(), undo + 1);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), original);
    h.app.run("edit.redo", json!({})).unwrap();
    assert!((doc::apply_point(&h.app.session.occurrence_placement(&[first]).unwrap().world, Vec3::ZERO) - numeric).len() < 1e-6);
    assert_eq!(serde_json::to_value(&h.app.session.doc.assembly.joints).unwrap(), joints);
    assert!(!h.app.session.selection.contains(&Sel::Occurrence { path: vec![second] }));
}

/// The triad sits on the part, wherever its component's origin is: a part modelled away from
/// that origin must not be turned and moved from over there. A second ring reads what was
/// dragged and turns about the axis it shows: one the first turn leaves it on (Local), or the
/// world's (Fixed).
#[test]
fn occurrence_triad_sits_on_the_part_and_its_rings_turn_about_their_axes() {
    for fixed in [false, true] {
        let mut h = Harness::new();
        let part = h.app.run("component.create", json!({"name":"Block","grounded":false})).unwrap();
        let first = part["occurrence"].as_u64().unwrap();
        h.app.run("solid.box", json!({"length":10,"width":20,"height":30,"corner":[100,40,0]})).unwrap();
        h.app.run("component.activate", json!({"component":0})).unwrap();
        let centre = Vec3::new(105., 50., 15.);
        h.app.cam = h.app.cam.looking_from(Vec3::new(1.0, 0.8, 0.6));
        h.app.cam.target = centre;
        h.app.cam.distance = 200.;
        h.frames(3);
        let target = Target::Occurrence { path: vec![first] };
        let item = context_menu::items(&h.app, &target).into_iter().find(|item| item.id == "ui.moveOccurrence").unwrap();
        context_menu::run_item(&mut h.app, &item, egui::Pos2::ZERO);
        h.frames(3);
        let at = crate::browser::occurrence_triad_model(&h.app).unwrap().center;
        assert!((at - centre).len() < 1e-6, "triad at {at:?}, part at {centre:?}");

        let mut m = h.app.tree.occurrence_move.clone().unwrap();
        m.fixed = fixed;
        m.angles = [0., 40., 0.];
        crate::browser::apply_occurrence_move(&mut h.app, &m);
        h.app.tree.occurrence_move = Some(m);
        h.frames(3);
        let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        let t = crate::browser::occurrence_triad_model(&h.app).unwrap();
        let grip = crate::gizmo::ring_grip(&proj, &t, 0).unwrap();
        let [start, end] = [grip, grip + 35.].map(|a| crate::gizmo::handle_point("rx", Vec3::ZERO, a).unwrap());
        h.step(&json!({"drag":[{"world":[start.x,start.y,start.z]},{"world":[end.x,end.y,end.z]}],"steps":12})).unwrap();
        h.frames(3);
        let m = h.app.tree.occurrence_move.as_ref().unwrap();
        assert_eq!(m.angles.map(|a| a.round()), [35., 40., 0.], "fixed {fixed}");
        let rotation = crate::gizmo::rotation;
        let (x, y) = (rotation(Vec3::X, 35_f64.to_radians()), rotation(Vec3::Y, 40_f64.to_radians()));
        let want = if fixed { crate::gizmo::mul(&x, &y) } else { crate::gizmo::mul(&y, &x) };
        let world = h.app.session.occurrence_placement(&[first]).unwrap().world;
        for (got, want) in crate::dialogs_move::rows_of(&world).iter().flatten().zip(want.iter().flatten()) {
            assert!((got - want).abs() < 1e-6, "fixed {fixed}: {world:?}");
        }
        let bounds = h.app.session.world_state().bodies.first().unwrap().mesh().bounds();
        assert!((bounds.center() - centre).len() < 1.0, "the part stays where it is: {bounds:?}");
    }
}
