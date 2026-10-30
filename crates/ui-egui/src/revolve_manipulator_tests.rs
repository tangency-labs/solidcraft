use crate::scenario::Harness;
use serde_json::{Value, json};
use solidcraft_engine::doc::{FeatureKind, kernel};

fn fixture(mut h: Harness, extra: Value) -> Harness {
    h.app.run("sketch.create", json!({"plane":"XY","name":"Profile"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[10,0],"p1":[15,5]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let mut params = json!({"sketch":"Profile","axis":"Y","angle":"90 deg","operation":"new","name":"Turn","body_name":"Main"});
    params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    h.app.run("solid.revolve", params).unwrap();
    h.frames(4);
    h
}
fn open(h: &mut Harness) {
    h.step(&json!({"call":"ui.view","params":{"view":"front"}})).unwrap();
    h.step(&json!({"call":"ui.view","params":{"view":"fit"}})).unwrap();
    h.step(&json!({"call":"ui.scroll","params":{"x":700,"y":450,"delta":-400}})).unwrap();
    h.step(&json!({"call":"ui.editFeature","params":{"feature":"Turn"}})).unwrap();
    h.frames(8);
}
fn at_angle(degrees: f64) -> Value {
    let a = degrees.to_radians();
    json!([12.5 * a.cos(), 2.5, 12.5 * a.sin()])
}
fn handle_at(h: &mut Harness, name: &str, world: Value) {
    let actual = h.at(&json!({"handle":name})).unwrap();
    let expected = h.at(&json!({"world":world})).unwrap();
    assert!((actual[0] - expected[0]).hypot(actual[1] - expected[1]) < 1.0, "{actual:?} != {expected:?}");
}
fn volume(h: &Harness) -> f64 {
    kernel::measure(&h.app.session.world_state().body("Main").unwrap().body).unwrap().volume
}
fn angle(h: &Harness) -> String {
    let FeatureKind::Revolve { angle, .. } = &h.app.session.doc.find_feature("Turn").unwrap().kind else { panic!("revolve") };
    angle.to_string()
}
fn expected_volume(degrees: f64) -> f64 {
    0.5 * (225.0 - 100.0) * 5.0 * degrees.to_radians()
}

#[test]
fn actual_positive_negative_axis_and_negative_sweep_drags_follow_geometry() {
    for (params, sign) in [(json!({}), 1.0), (json!({"axis":{"origin":[0,0,0],"dir":[0,-2,0]}}), 1.0), (json!({"angle":"-90 deg"}), -1.0)] {
        let mut h = fixture(Harness::new(), params.clone());
        let original = serde_json::to_value(&h.app.session.doc.find_feature("Turn").unwrap().kind).unwrap();
        let names = h.app.session.doc.find_feature("Turn").unwrap().param_names.clone();
        let undo = h.app.session.undo.len();
        open(&mut h);
        handle_at(&mut h, "revolve", at_angle(sign * 90.0));
        h.step(&json!({"drag":[{"handle":"revolve"},{"world":at_angle(sign * 60.0)}]})).unwrap();
        let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        let preview = h.app.session.preview_world(&commands).unwrap();
        assert!((kernel::measure(&preview.after.body("Main").unwrap().body).unwrap().volume - expected_volume(60.0)).abs() < 0.03);
        let bounds = preview.after.body("Main").unwrap().mesh().bounds();
        if sign > 0.0 {
            assert!(bounds.min.z > -1e-8 && bounds.max.z > 12.9);
        } else {
            assert!(bounds.max.z < 1e-6 && bounds.min.z < -12.9, "{params}: {bounds:?}");
        }
        h.step(&json!({"widget":"OK"})).unwrap();
        assert_eq!(angle(&h), if params.get("angle").is_some() { "-60 deg" } else { "60 deg" });
        assert_eq!(h.app.session.doc.find_feature("Turn").unwrap().param_names, names);
        assert!((volume(&h) - expected_volume(60.0)).abs() < 0.03);
        assert_eq!(h.app.session.undo.len(), undo + 1);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(serde_json::to_value(&h.app.session.doc.find_feature("Turn").unwrap().kind).unwrap(), original);
    }
}

#[test]
fn symmetric_boundary_drags_edit_included_angle_and_cancel_is_lossless() {
    for (handle, sign) in [("revolve", 1.0), ("revolve2", -1.0)] {
        let mut h = fixture(Harness::new(), json!({"angle":"120 deg","direction":"symmetric"}));
        let original = serde_json::to_value(&*h.app.session.doc).unwrap();
        let undo = h.app.session.undo.len();
        open(&mut h);
        handle_at(&mut h, "revolve", at_angle(60.0));
        handle_at(&mut h, "revolve2", at_angle(-60.0));
        h.step(&json!({"drag":[{"handle":handle},{"world":at_angle(sign * 45.0)}]})).unwrap();
        let crate::dialogs::Kind::Revolve { angle: entered, .. } = &h.app.dialog.as_ref().unwrap().kind else { panic!("revolve") };
        assert_eq!(entered, "90 deg");
        h.step(&json!({"widget":"Cancel"})).unwrap();
        assert_eq!(serde_json::to_value(&*h.app.session.doc).unwrap(), original);
        assert_eq!(h.app.session.undo.len(), undo);
        open(&mut h);
        h.step(&json!({"drag":[{"handle":handle},{"world":at_angle(sign * 45.0)}]})).unwrap();
        let crate::dialogs::Kind::Revolve { angle: entered, .. } = &h.app.dialog.as_ref().unwrap().kind else { panic!("revolve") };
        assert_eq!(entered, "90 deg", "second {handle} drag");
        h.step(&json!({"widget":"OK"})).unwrap();
        assert!((volume(&h) - expected_volume(90.0)).abs() < 0.1, "{handle}: {} angle {}", volume(&h), angle(&h));
        let bounds = h.app.session.world_state().body("Main").unwrap().mesh().bounds();
        assert!((bounds.max.z - 15.0 / 2.0f64.sqrt()).abs() < 0.03 && (bounds.min.z + 15.0 / 2.0f64.sqrt()).abs() < 0.03);
        let path = h.dir.join("symmetric.solidcraft");
        h.app.run("file.save", json!({"path":path})).unwrap();
        h.app.run("doc.open", json!({"path":path})).unwrap();
        assert_eq!(angle(&h), "90 deg");
        assert!((volume(&h) - expected_volume(90.0)).abs() < 0.1);
    }
}

#[test]
fn to_object_displays_derived_sweep_without_dormant_edit_until_angle_selected() {
    let mut h = fixture(Harness::new(), json!({"angle":"45 deg","angle2":"30 deg","to":[0,2,12]}));
    let original = serde_json::to_value(&h.app.session.doc.find_feature("Turn").unwrap().kind).unwrap();
    open(&mut h);
    assert!(h.widget("Sweep 90.0°").is_some());
    assert!(crate::scenario::handle_at("revolve").is_none());
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"180 deg"})).unwrap();
    let crate::dialogs::Kind::Revolve { angle, .. } = &h.app.dialog.as_ref().unwrap().kind else { panic!("revolve") };
    assert_eq!(angle, "45 deg");
    h.step(&json!({"widget":"OK"})).unwrap();
    assert_eq!(serde_json::to_value(&h.app.session.doc.find_feature("Turn").unwrap().kind).unwrap(), original);
    open(&mut h);
    h.step(&json!({"click":{"handle":"dialog:Type"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Type:Angle"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Angle"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"90 deg"})).unwrap();
    h.step(&json!({"widget":"OK"})).unwrap();
    let FeatureKind::Revolve { to, angle2, .. } = &h.app.session.doc.find_feature("Turn").unwrap().kind else { panic!("revolve") };
    assert!(to.is_none());
    assert_eq!(angle2.as_deref(), Some("30 deg"));
    assert!((volume(&h) - expected_volume(120.0)).abs() < 0.03);
}

#[test]
fn two_side_boundary_preserves_other_angle_and_parameter_binding() {
    let mut h = fixture(Harness::new(), json!({"angle":"60 deg","angle2":"30 deg"}));
    h.app.run("solid.box", json!({"length":1,"width":1,"height":"(d2 / (1 rad)) * 1 mm","corner":[30,0,0],"body_name":"Dependent"})).unwrap();
    let names = h.app.session.doc.find_feature("Turn").unwrap().param_names.clone();
    open(&mut h);
    handle_at(&mut h, "revolve", at_angle(60.0));
    handle_at(&mut h, "revolve2", at_angle(-30.0));
    h.step(&json!({"drag":[{"handle":"revolve2"},{"world":at_angle(-45.0)}]})).unwrap();
    h.step(&json!({"widget":"OK"})).unwrap();
    let FeatureKind::Revolve { angle, angle2, .. } = &h.app.session.doc.find_feature("Turn").unwrap().kind else { panic!("revolve") };
    assert_eq!(angle, "60 deg");
    assert_eq!(angle2.as_deref(), Some("45 deg"));
    assert_eq!(h.app.session.doc.find_feature("Turn").unwrap().param_names, names);
    assert!((h.app.session.world_state().body("Dependent").unwrap().mesh().bounds().max.z - std::f64::consts::FRAC_PI_4).abs() < 1e-8);
    assert!((volume(&h) - expected_volume(105.0)).abs() < 0.03);
}

#[test]
fn selected_second_placement_custom_axis_handle_and_preview_stay_in_its_frame() {
    let mut h = Harness::new();
    let owner = h.app.run("component.create", json!({"name":"Owner"})).unwrap();
    let mut h = fixture(h, json!({"axis":{"origin":[0,0,0],"dir":[0,2,0]}}));
    let second = h.app.run("occurrence.copy", json!({"component":owner["component"],"translate":[100,0,20]})).unwrap()["occurrence"].clone();
    h.app.run("occurrence.move", json!({"occurrence":second,"axis":[0,0,1],"angle":"17 deg"})).unwrap();
    h.app.run("component.activate", json!({"component":owner["component"],"occurrence":second,"path":[second]})).unwrap();
    let frame = h.app.session.active_component_frame();
    let world = |degrees: f64| {
        let a = degrees.to_radians();
        let p = solidcraft_engine::doc::apply_point(&frame, solidcraft_engine::geom::Vec3::new(12.5 * a.cos(), 2.5, 12.5 * a.sin()));
        json!([p.x, p.y, p.z])
    };
    let axis = match &h.app.session.doc.find_feature("Turn").unwrap().kind {
        FeatureKind::Revolve { axis, .. } => axis.clone(),
        _ => panic!("revolve"),
    };
    open(&mut h);
    assert_eq!(crate::scenario::count_of("dialog:selection:Axis:filled"), Some(1.0), "the stored custom axis stays selected");
    handle_at(&mut h, "revolve", world(90.0));
    h.step(&json!({"drag":[{"handle":"revolve"},{"world":world(60.0)}]})).unwrap();
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let preview = h.app.session.preview_world(&commands).unwrap();
    let placed = preview.after.bodies.iter().find(|body| body.mesh().bounds().center().x > 50.0).unwrap();
    assert!((kernel::measure(&placed.body).unwrap().volume - expected_volume(60.0)).abs() < 0.03);
    h.step(&json!({"widget":"OK"})).unwrap();
    let FeatureKind::Revolve { axis: after, angle, .. } = &h.app.session.doc.find_feature("Turn").unwrap().kind else { panic!("revolve") };
    assert_eq!(after, &axis);
    assert_eq!(angle, "60 deg");
    assert_eq!(h.app.session.active_occurrence_path.as_deref(), Some(&[second.as_u64().unwrap()][..]));
}

#[test]
fn curved_to_object_has_surface_feedback_without_an_angular_handle_or_input() {
    let mut h = Harness::new();
    h.app.run("solid.cylinder", json!({"radius":3,"height":5,"base":[0,0,12],"axis":"Y","operation":"new","body_name":"Target"})).unwrap();
    let mut h = fixture(h, json!({}));
    open(&mut h);
    h.step(&json!({"click":{"handle":"dialog:Type"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Type:To Object"}})).unwrap();
    let mut dialog = h.app.dialog.take().unwrap();
    dialog.active = dialog.inputs.iter().position(|input| input.label == "To Object").unwrap();
    dialog.pick(&h.app.session, solidcraft_engine::Sel::Vertex { body: "Target".into(), point: solidcraft_engine::geom::Vec3::new(0.0, 2.0, 15.0) });
    h.app.dialog = Some(dialog);
    h.frames(8);
    assert!(h.widget("To surface").is_some());
    assert!(crate::scenario::handle_at("revolve").is_none());
    assert!(crate::scenario::handle_at("revolve2").is_none());
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"180 deg"})).unwrap();
    let crate::dialogs::Kind::Revolve { angle: entered, .. } = &h.app.dialog.as_ref().unwrap().kind else { panic!("revolve") };
    assert_eq!(entered, "90 deg");
    h.step(&json!({"widget":"Cancel"})).unwrap();
    assert_eq!(angle(&h), "90 deg");
}

#[test]
fn free_drag_and_invalid_expression_stay_editable_before_commit() {
    let mut h = fixture(Harness::new(), json!({}));
    open(&mut h);
    h.step(&json!({"drag":[{"handle":"revolve"},{"world":at_angle(37.3)}],"ctrl":true})).unwrap();
    let crate::dialogs::Kind::Revolve { angle: entered, .. } = &h.app.dialog.as_ref().unwrap().kind else { panic!("revolve") };
    assert!((h.app.session.doc.eval(entered, solidcraft_engine::doc::expr::Kind::Angle).unwrap().to_degrees() - 37.3).abs() < 0.2);
    h.step(&json!({"click":{"handle":"dialog:Angle"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"unknown_angle"})).unwrap();
    h.frames(4);
    assert!(h.widget("unknown_angle").is_some());
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_some());
    h.step(&json!({"click":{"handle":"dialog:Angle"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"60 deg"})).unwrap();
    let crate::dialogs::Kind::Revolve { angle: entered, .. } = &h.app.dialog.as_ref().unwrap().kind else { panic!("revolve") };
    assert_eq!(entered, "60 deg");
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().map(|dialog| &dialog.error));
    assert_eq!(angle(&h), "60 deg");
    assert!((volume(&h) - expected_volume(60.0)).abs() < 0.03);
}

/// Typing 0° builds no solid, and the dialog keeps the range error. The grab point stays on
/// the profile, and dragging it brings the angle back.
#[test]
fn zero_angle_keeps_the_grab_point_and_a_drag_brings_the_angle_back() {
    for (params, handle, drag_to, entered_angle, committed) in
        [(json!({}), "revolve", 45.0, "45 deg", 45.0), (json!({"direction": "symmetric"}), "revolve2", -45.0, "90 deg", 90.0)]
    {
        let mut h = fixture(Harness::new(), params.clone());
        open(&mut h);
        h.step(&json!({"click":{"handle":"dialog:Angle"}})).unwrap();
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        h.step(&json!({"text":"0 deg"})).unwrap();
        h.frames(4);
        handle_at(&mut h, "revolve", at_angle(0.0));
        // A preview refusal stays on the preview while the angle field is still being typed in.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while h.app.preview.busy {
            assert!(std::time::Instant::now() < deadline, "the preview did not finish");
            h.frames(1);
        }
        let error = h.app.dialog.as_ref().and_then(|dialog| dialog.error.clone()).or_else(|| h.app.preview.error.clone()).unwrap_or_default();
        assert!(error.contains("more than 0"), "{error}");
        h.step(&json!({"widget":"OK"})).unwrap();
        assert!(h.app.dialog.is_some(), "a zero angle still builds no solid");
        h.step(&json!({"drag":[{"handle":handle},{"world":at_angle(drag_to)}]})).unwrap();
        let crate::dialogs::Kind::Revolve { angle: entered, .. } = &h.app.dialog.as_ref().unwrap().kind else { panic!("revolve") };
        assert_eq!(entered, entered_angle);
        h.step(&json!({"widget":"OK"})).unwrap();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().map(|dialog| &dialog.error));
        assert_eq!(angle(&h), entered_angle);
        let got = volume(&h);
        let want = expected_volume(committed);
        // A symmetric drag that starts with both handles stacked at 0° lands within a tenth of a
        // cubic millimetre of the analytic quarter turn.
        assert!((got - want).abs() < 0.1, "params={params} got={got} want={want}");
    }
}

#[test]
fn second_side_can_be_dragged_to_zero_without_becoming_full_turn() {
    let mut h = fixture(Harness::new(), json!({"angle":"60 deg","angle2":"30 deg"}));
    open(&mut h);
    h.step(&json!({"drag":[{"handle":"revolve2"},{"world":at_angle(0.0)}]})).unwrap();
    assert_eq!(h.app.dialog.as_ref().unwrap().extra.get("angle2"), Some(&json!("0 deg")));
    h.step(&json!({"widget":"OK"})).unwrap();
    let FeatureKind::Revolve { angle2, .. } = &h.app.session.doc.find_feature("Turn").unwrap().kind else { panic!("revolve") };
    assert_eq!(angle2.as_deref(), Some("0 deg"));
    assert!((volume(&h) - expected_volume(60.0)).abs() < 0.03);
}

#[test]
fn edge_on_ring_drag_uses_nearest_projected_branch() {
    let mut h = fixture(Harness::new(), json!({}));
    open(&mut h);
    h.step(&json!({"call":"ui.view","params":{"view":"top"}})).unwrap();
    handle_at(&mut h, "revolve", at_angle(90.0));
    h.step(&json!({"drag":[{"handle":"revolve"},{"world":at_angle(60.0)}]})).unwrap();
    h.step(&json!({"widget":"OK"})).unwrap();
    assert_eq!(angle(&h), "60 deg");
    assert!((volume(&h) - expected_volume(60.0)).abs() < 0.03);
}

#[test]
fn legacy_reversed_axis_symmetric_interval_is_shown_and_preserved_exactly() {
    let mut h = fixture(
        Harness::new(),
        json!({"axis":{"origin":[0,0,0],"dir":[0,-2,0]},"direction":"symmetric","angle":"120 deg","convention":"legacy_axis_start"}),
    );
    // Exercise an actual older design: no convention marker was stored by version 10.
    let mut saved: Value = serde_json::from_str(&h.app.session.doc.to_json()).unwrap();
    saved["format"] = json!("solidcraft/10");
    for feature in saved["features"].as_array_mut().unwrap() {
        feature.as_object_mut().unwrap().remove("convention");
    }
    let path = h.dir.join("legacy-revolve.solidcraft");
    std::fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
    h.app.run("doc.open", json!({"path":path})).unwrap();
    let FeatureKind::Revolve { convention, .. } = &h.app.session.doc.find_feature("Turn").unwrap().kind else { panic!("revolve") };
    assert_eq!(*convention, solidcraft_engine::doc::RevolveConvention::LegacyAxisStart);
    let original = serde_json::to_value(&h.app.session.doc.find_feature("Turn").unwrap().kind).unwrap();
    let before = h.app.session.world_state().body("Main").unwrap().mesh().bounds();
    assert!(before.min.x < -14.9 && before.max.x < 7.6 && before.min.z > -1e-6 && before.max.z > 14.9);
    open(&mut h);
    handle_at(&mut h, "revolve", at_angle(180.0));
    handle_at(&mut h, "revolve2", at_angle(60.0));
    h.step(&json!({"widget":"OK"})).unwrap();
    assert_eq!(serde_json::to_value(&h.app.session.doc.find_feature("Turn").unwrap().kind).unwrap(), original);
    assert_eq!(h.app.session.world_state().body("Main").unwrap().mesh().bounds(), before);
    open(&mut h);
    h.step(&json!({"drag":[{"handle":"revolve"},{"world":at_angle(135.0)}]})).unwrap();
    h.step(&json!({"widget":"OK"})).unwrap();
    assert_eq!(angle(&h), "90 deg");
    let after = h.app.session.world_state().body("Main").unwrap().mesh().bounds();
    assert!((after.min.x + 15.0 / 2.0f64.sqrt()).abs() < 0.03 && (after.max.x - 15.0 / 2.0f64.sqrt()).abs() < 0.03);
    assert!(after.min.z > 7.0 && after.max.z > 14.9);
    assert!((volume(&h) - expected_volume(90.0)).abs() < 0.1);
}

#[test]
fn canonical_symmetric_ring_preview_drag_and_reopen_match_material() {
    for axis in [json!("Y"), json!({"origin":[0,0,0],"dir":[0,-2,0]})] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY","name":"Profile"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[2,0],"p1":[4,10]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app
            .run("solid.revolve", json!({"sketch":"Profile","axis":axis,"angle":"90 deg","direction":"symmetric","name":"Turn","body_name":"Main"}))
            .unwrap();
        let before = serde_json::to_value(&h.app.session.doc.find_feature("Turn").unwrap().kind).unwrap();
        let names = h.app.session.doc.find_feature("Turn").unwrap().param_names.clone();
        open(&mut h);
        let at = |degrees: f64| {
            let a = degrees.to_radians();
            json!([3.0 * a.cos(), 5.0, 3.0 * a.sin()])
        };
        handle_at(&mut h, "revolve", at(45.0));
        handle_at(&mut h, "revolve2", at(-45.0));
        let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        let preview = h.app.session.preview_world(&commands).unwrap();
        let body = &preview.after.body("Main").unwrap().body;
        assert!(body.contains(solidcraft_engine::geom::Vec3::new(3.0, 5.0, 0.0)));
        assert!(!body.contains(solidcraft_engine::geom::Vec3::new(0.0, 5.0, 3.0)));
        let bounds = preview.after.body("Main").unwrap().mesh().bounds();
        assert!((bounds.min.x - 2.0f64.sqrt()).abs() < 0.02 && (bounds.max.x - 4.0).abs() < 0.02, "{bounds:?}");
        assert!((bounds.min.z + 2.0 * 2.0f64.sqrt()).abs() < 0.02 && (bounds.max.z - 2.0 * 2.0f64.sqrt()).abs() < 0.02);
        h.step(&json!({"widget":"OK"})).unwrap();
        assert_eq!(serde_json::to_value(&h.app.session.doc.find_feature("Turn").unwrap().kind).unwrap(), before);
        open(&mut h);
        h.step(&json!({"drag":[{"handle":"revolve2"},{"world":at(-30.0)}]})).unwrap();
        h.step(&json!({"widget":"OK"})).unwrap();
        assert_eq!(angle(&h), "60 deg");
        assert_eq!(h.app.session.doc.find_feature("Turn").unwrap().param_names, names);
        assert!((volume(&h) - 60.0 * std::f64::consts::FRAC_PI_3).abs() < 0.03);
        let path = h.dir.join("canonical-revolve.solidcraft");
        h.app.run("file.save", json!({"path":path})).unwrap();
        h.app.run("doc.open", json!({"path":path})).unwrap();
        open(&mut h);
        handle_at(&mut h, "revolve", at(30.0));
        handle_at(&mut h, "revolve2", at(-30.0));
        let FeatureKind::Revolve { convention, .. } = &h.app.session.doc.find_feature("Turn").unwrap().kind else { panic!("revolve") };
        assert_eq!(*convention, solidcraft_engine::doc::RevolveConvention::SketchNormal);
        h.step(&json!({"widget":"Cancel"})).unwrap();
    }
}

#[test]
fn reversed_axis_to_object_canvas_sweep_and_preview_follow_the_quarter_target() {
    let mut h = fixture(Harness::new(), json!({"axis":{"origin":[0,0,0],"dir":[0,-2,0]},"to":[0,2,12],"angle":"45 deg"}));
    let original = serde_json::to_value(&h.app.session.doc.find_feature("Turn").unwrap().kind).unwrap();
    let names = h.app.session.doc.find_feature("Turn").unwrap().param_names.clone();
    let before = h.app.session.world_state().body("Main").unwrap().mesh().bounds();
    let p = 12.5 / 2.0f64.sqrt();
    open(&mut h);
    assert!(h.widget("Sweep 90.0°").is_some());
    assert!(crate::scenario::handle_at("revolve").is_none());
    assert!(crate::scenario::handle_at("revolve2").is_none());
    let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let preview = h.app.session.preview_world(&commands).unwrap();
    let result = preview.after.body("Main").unwrap();
    assert_eq!(result.mesh().bounds(), before);
    assert!(result.body.contains(solidcraft_engine::geom::Vec3::new(p, 2.5, p)));
    for (x, z) in [(-p, p), (-p, -p), (p, -p)] {
        assert!(!result.body.contains(solidcraft_engine::geom::Vec3::new(x, 2.5, z)));
    }
    assert!((kernel::measure(&result.body).unwrap().volume - expected_volume(90.0)).abs() < 0.03);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert_eq!(serde_json::to_value(&h.app.session.doc.find_feature("Turn").unwrap().kind).unwrap(), original);
    assert_eq!(h.app.session.doc.find_feature("Turn").unwrap().param_names, names);
    let path = h.dir.join("reversed-to-object.solidcraft");
    h.app.run("file.save", json!({"path":path})).unwrap();
    h.app.run("doc.open", json!({"path":path})).unwrap();
    open(&mut h);
    assert!(h.widget("Sweep 90.0°").is_some());
    h.step(&json!({"widget":"OK"})).unwrap();
    assert_eq!(serde_json::to_value(&h.app.session.doc.find_feature("Turn").unwrap().kind).unwrap(), original);
    assert_eq!(h.app.session.world_state().body("Main").unwrap().mesh().bounds(), before);
}
