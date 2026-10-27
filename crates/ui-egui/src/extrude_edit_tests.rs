//! Exercise the actual feature editor, including frames rendered before OK.
use crate::scenario::Harness;
use serde_json::{Value, json};
use solidcraft_engine::doc::{Extent, FeatureKind};

fn make(params: Value) -> Harness {
    make_in(Harness::new(), params)
}
fn make_in(mut h: Harness, params: Value) -> Harness {
    h.app.run("sketch.create", json!({"plane":"XY","name":"Base"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let mut p = json!({"sketch":"Base","distance":"10 mm","operation":"new","name":"Depth","body_name":"Main"});
    p.as_object_mut().unwrap().extend(params.as_object().unwrap().clone());
    h.app.run("solid.extrude", p).unwrap();
    h.frames(4);
    h
}
fn extent(h: &Harness) -> Extent {
    let FeatureKind::Extrude { extent, .. } = &h.app.session.doc.find_feature("Depth").unwrap().kind else { panic!("extrude") };
    extent.clone()
}
fn open(h: &mut Harness) {
    h.step(&json!({"call":"ui.editFeature","params":{"feature":"Depth"}})).unwrap();
    h.frames(8);
}

#[test]
fn undoing_the_edited_feature_closes_its_editor_and_preserves_redo() {
    let mut h = make(json!({}));
    let feature = h.app.session.doc.find_feature("Depth").unwrap().id;
    open(&mut h);
    assert!(h.app.dialog.is_some());

    h.app.start("edit.undo");
    h.frames(8);
    assert!(h.app.session.doc.feature(feature).is_none());
    assert!(h.app.dialog.is_none(), "a removed feature must not keep an Edit Extrude dialog");
    assert!(h.app.editing_state.is_none());
    assert!(h.app.applying.is_none());
    assert!(!h.app.preview.active);
    assert!(h.app.session.feature_edit_sketch.is_none());
    assert_eq!(h.app.session.redo.len(), 1);
    assert_eq!(h.app.session.doc.features.len(), 1);
    let revision = h.app.session.revision;
    h.frames(8);
    assert_eq!(h.app.session.revision, revision, "closing the stale editor does not change the design");

    h.app.run("edit.redo", json!({})).unwrap();
    assert!(h.app.session.doc.feature(feature).is_some());
    assert!(h.app.dialog.is_none(), "redo does not resurrect the old editor");
    assert_eq!(h.app.session.world_state().body("Main").unwrap().mesh().bounds().max.z, 10.0);
    open(&mut h);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "the restored feature can be edited normally");
}

#[test]
fn undo_that_keeps_the_edited_feature_keeps_its_editor() {
    let mut h = make(json!({}));
    let feature = h.app.session.doc.find_feature("Depth").unwrap().id;
    h.app.run("solid.box", json!({"corner":[30,0,0],"length":2,"width":2,"height":2})).unwrap();
    open(&mut h);
    h.app.start("edit.undo");
    h.frames(8);
    assert!(h.app.session.doc.feature(feature).is_some());
    assert_eq!(h.app.dialog.as_ref().unwrap().editing.map(|(id, _)| id), Some(feature));
    assert!(h.app.editing_state.is_some());
    assert!(h.app.session.feature_edit_sketch.is_some());
    assert_eq!(h.app.session.redo.len(), 1);
}

#[test]
fn deleting_the_edited_feature_closes_its_editor_and_undo_restores_the_design() {
    let mut h = make(json!({}));
    let original = h.app.session.doc.clone();
    open(&mut h);
    h.app.run("timeline.delete", json!({"feature":"Depth"})).unwrap();
    let deleted = h.app.session.doc.clone();
    let history = (h.app.session.undo.len(), h.app.session.redo.len());
    h.frames(8);
    assert!(h.app.dialog.is_none() && h.app.editing_state.is_none());
    assert!(!h.app.preview.active);
    assert!(std::sync::Arc::ptr_eq(&deleted, &h.app.session.doc), "closing preserves the current document and marker");
    assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
    h.app.run("edit.undo", json!({})).unwrap();
    assert!(std::sync::Arc::ptr_eq(&original, &h.app.session.doc), "Undo restores the document before the temporary editor rollback");
    assert_eq!(h.app.session.world_state().body("Main").unwrap().mesh().bounds().max.z, 10.0);
}

#[test]
fn unchanged_all_profiles_includes_profiles_added_after_edit() {
    let mut h = make(json!({}));
    open(&mut h);
    h.step(&json!({"widget":"OK"})).unwrap();
    let FeatureKind::Extrude { profiles, .. } = &h.app.session.doc.find_feature("Depth").unwrap().kind else { panic!("extrude") };
    assert!(matches!(profiles, solidcraft_engine::doc::ProfileSel::All));
    h.app.run("sketch.edit", json!({"sketch":"Base"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[20,0],"p1":[25,5]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let state = h.app.session.world_state();
    let volume: f64 = state.bodies.iter().map(|b| solidcraft_engine::doc::kernel::measure(&b.body).unwrap().volume).sum();
    assert!((volume - 1250.0).abs() < 1e-7, "{volume}");
}

#[test]
fn unchanged_actual_editor_preserves_all_extents_and_parameter_identities() {
    for p in [
        json!({"direction":"negative","distance":"10"}),
        json!({"direction":"negative","distance":"-10 mm"}),
        json!({"direction":"symmetric","distance":"5 mm"}),
        json!({"direction":"symmetric","distance":"5 mm","convention":"legacy_per_side"}),
        json!({"distance":"10 mm","distance2":"3 mm"}),
        json!({"direction":"negative","distance":"10 mm","distance2":"3 mm"}),
        json!({"from":[0,0,5],"start_offset":"0 mm"}),
        json!({"to":[0,0,10],"to_offset":"3 mm"}),
        json!({"to":[0,0,-10],"to_offset":"-2 mm"}),
        json!({"taper":"0 deg"}),
    ] {
        let mut h = make(p.clone());
        let original = extent(&h);
        let names = h.app.session.doc.find_feature("Depth").unwrap().param_names.clone();
        let before = h.app.session.world_state().body("Main").unwrap().mesh().bounds();
        open(&mut h);
        h.step(&json!({"widget":"OK"})).unwrap();
        assert!(h.app.dialog.is_none(), "{p}: {:?}", h.app.session.log);
        assert_eq!(extent(&h), original, "{p}");
        assert_eq!(h.app.session.doc.find_feature("Depth").unwrap().param_names, names, "{p}");
        assert_eq!(h.app.session.world_state().body("Main").unwrap().mesh().bounds(), before, "{p}");
    }
}

#[test]
fn negative_edit_keeps_downstream_distance_reference_positive() {
    let mut h = make(json!({"direction":"negative","distance":"10"}));
    h.app.run("solid.box", json!({"length":10,"width":10,"height":"d1","corner":[30,0,0],"body_name":"Dependent"})).unwrap();
    open(&mut h);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert_eq!(h.app.session.doc.eval("d1", solidcraft_engine::doc::expr::Kind::Length).unwrap(), 10.0);
    assert_eq!(h.app.session.world_state().body("Dependent").unwrap().mesh().bounds().max.z, 10.0);
}

#[test]
fn cancel_after_canvas_edit_preserves_document_and_undo() {
    let mut h = make(json!({"from":[0,0,5],"to":[0,0,15],"to_offset":"2 mm"}));
    let original = extent(&h);
    let undo = h.app.session.undo.len();
    open(&mut h);
    assert_eq!(crate::canvas::anchor(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().0.z, 5.0);
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"4 mm"})).unwrap();
    h.frames(8);
    h.step(&json!({"widget":"Cancel"})).unwrap();
    assert_eq!(extent(&h), original);
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
fn to_offset_canvas_edit_changes_offset_and_one_undo_restores_extent() {
    let mut h = make(json!({"from":[0,0,5],"to":[0,0,15],"to_offset":"2 mm"}));
    let original = extent(&h);
    open(&mut h);
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"4 mm"})).unwrap();
    h.step(&json!({"widget":"OK"})).unwrap();
    assert_eq!(extent(&h).to_offset.as_deref(), Some("4 mm"));
    assert_eq!(extent(&h).distance, original.distance);
    assert_eq!(h.app.session.world_state().body("Main").unwrap().mesh().bounds().max.z, 19.0);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(extent(&h), original);
}

#[test]
fn moved_owner_references_survive_actual_edit_with_another_component_active() {
    let mut h = Harness::new();
    let owner = h.app.run("component.create", json!({"name":"Owner","grounded":false})).unwrap();
    let mut h = make_in(h, json!({"from":[0,0,5],"to":[0,0,15],"to_offset":"2 mm"}));
    h.app.run("occurrence.move", json!({"occurrence":owner["occurrence"],"translate":[100,0,20]})).unwrap();
    let other = h.app.run("component.create", json!({"name":"Other","parent":0})).unwrap();
    h.app.run("occurrence.move", json!({"occurrence":other["occurrence"],"translate":[200,0,40]})).unwrap();
    let original = extent(&h);
    let active = h.app.session.active_component;
    open(&mut h);
    let base = crate::canvas::anchor(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().0;
    assert_eq!(base.z, 25.0);
    assert!(base.x >= 100.0 && base.x <= 110.0);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert_eq!(extent(&h), original);
    assert_eq!(h.app.session.active_component, active);
    assert_eq!(h.app.session.world_state().body("Main").unwrap().mesh().bounds().max.z, 37.0);
}

#[test]
fn dragged_to_object_handle_edits_offset_not_dormant_distance() {
    let mut h = make(json!({"from":[0,0,5],"to":[0,0,15],"to_offset":"2 mm"}));
    open(&mut h);
    let original = extent(&h);
    h.step(&json!({"drag":[{"handle":"arrow"},{"world":[5,5,21]}],"ctrl":true})).unwrap();
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.session.log);
    assert_eq!(extent(&h).distance, original.distance);
    let state = h.app.session.world_state();
    let z = state.body("Main").unwrap().mesh().bounds().max.z;
    assert!((z - 21.0).abs() < 0.1, "dragged end {z}");
}

#[test]
fn referenced_offset_recomputes_after_edit_and_save_reopen() {
    let mut h = Harness::new();
    h.app.run("parameters.add", json!({"name":"clearance","expression":"2 mm"})).unwrap();
    let mut h = make_in(h, json!({"to":[0,0,15],"to_offset":"clearance"}));
    open(&mut h);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert_eq!(extent(&h).to_offset.as_deref(), Some("clearance"));
    h.app.run("parameters.change", json!({"name":"clearance","expression":"6 mm"})).unwrap();
    assert_eq!(h.app.session.world_state().body("Main").unwrap().mesh().bounds().max.z, 21.0);
    let path = h.dir.join("extent.solidcraft");
    h.app.run("file.save", json!({"path":path})).unwrap();
    h.app.run("doc.open", json!({"path":path})).unwrap();
    assert_eq!(extent(&h).to_offset.as_deref(), Some("clearance"));
    assert_eq!(h.app.session.world_state().body("Main").unwrap().mesh().bounds().max.z, 21.0);
}

#[test]
fn from_object_start_can_be_explicitly_changed_to_profile_plane() {
    let mut h = make(json!({"from":[0,0,5]}));
    open(&mut h);
    h.step(&json!({"click":{"handle":"dialog:Start"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Start:Profile Plane"}})).unwrap();
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(extent(&h).from.is_none());
    assert_eq!(h.app.session.world_state().body("Main").unwrap().mesh().bounds().min.z, 0.0);
}

#[test]
fn explicit_reverse_keeps_original_expression_and_parameter_sign() {
    let mut h = make(json!({"distance":"10"}));
    open(&mut h);
    h.step(&json!({"widget":"Reverse Direction"})).unwrap();
    h.step(&json!({"widget":"OK"})).unwrap();
    assert_eq!(extent(&h).distance, "10");
    assert_eq!(extent(&h).direction, solidcraft_engine::doc::Direction::Negative);
    assert_eq!(h.app.session.doc.eval("d1", solidcraft_engine::doc::expr::Kind::Length).unwrap(), 10.0);
    assert_eq!(h.app.session.world_state().body("Main").unwrap().mesh().bounds().min.z, -10.0);
}

#[test]
fn failed_edit_and_cancel_keep_full_history_and_existing_redo() {
    let mut h = make(json!({}));
    for i in 0..205 {
        h.app.run("timeline.rename", json!({"feature":1,"name":format!("Base{i}")})).unwrap();
    }
    h.app.run("solid.box", json!({"length":2,"width":2,"height":2,"corner":[30,0,0],"body_name":"RedoBody"})).unwrap();
    h.app.run("edit.undo", json!({})).unwrap();
    let undo = h.app.session.undo.len();
    let redo = h.app.session.redo.len();
    assert_eq!(undo, 199);
    assert_eq!(redo, 1);
    h.app.session.selection = vec![solidcraft_engine::Sel::Body { name: "Main".into() }];
    let selection = h.app.session.selection.clone();
    let original = serde_json::to_value(&*h.app.session.doc).unwrap();
    open(&mut h);
    assert_eq!(h.app.session.undo.len(), undo);
    assert_eq!(h.app.session.redo.len(), redo);
    assert_eq!(h.app.session.selection, selection);
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"missing_extent"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(h.app.dialog.is_some(), "failed edit must remain editable");
    h.step(&json!({"widget":"Cancel"})).unwrap();
    assert_eq!(serde_json::to_value(&*h.app.session.doc).unwrap(), original);
    assert_eq!(h.app.session.undo.len(), undo);
    assert_eq!(h.app.session.redo.len(), redo);
    assert_eq!(h.app.session.selection, selection);
    h.app.run("edit.redo", json!({})).unwrap();
    assert!(h.app.session.world_state().body("RedoBody").is_some());
}

#[test]
fn tapered_from_and_signed_to_have_correct_start_volume_and_preview_parity() {
    for (params, lo, hi) in [
        (json!({"from":[0,0,5],"distance":"10 mm","taper":"atan(0.1)"}), 5.0, 15.0),
        (json!({"from":[0,0,5],"to":[0,0,15],"direction":"negative","taper":"atan(0.1)"}), 5.0, 15.0),
        (json!({"from":[0,0,5],"to":[0,0,-5],"direction":"negative","taper":"atan(0.1)"}), -5.0, 5.0),
        (json!({"from":[0,0,5],"to":[0,0,-5],"direction":"positive","taper":"atan(0.1)"}), -5.0, 5.0),
    ] {
        let mut h = make(params.clone());
        let state = h.app.session.world_state();
        let body = state.body("Main").unwrap();
        let bounds = body.mesh().bounds();
        assert!((bounds.min.z - lo).abs() < 1e-8 && (bounds.max.z - hi).abs() < 1e-8, "{params}: {bounds:?}");
        let expected = 10.0 * (100.0 + 120.0 + 144.0) / 3.0;
        let volume = solidcraft_engine::doc::kernel::measure(&body.body).unwrap().volume;
        assert!((volume - expected).abs() < 1e-5, "{params}: volume{volume} expected{expected}");
        let original = extent(&h);
        open(&mut h);
        let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        let preview = h.app.session.preview(&commands).unwrap();
        assert_eq!(preview.after.body("Main").unwrap().mesh().bounds(), bounds);
        h.step(&json!({"widget":"OK"})).unwrap();
        assert_eq!(extent(&h), original);
        assert_eq!(h.app.session.world_state().body("Main").unwrap().mesh().bounds(), bounds);
    }
}

#[test]
fn adjacent_profiles_from_slanted_face_use_same_merged_centre_as_geometry() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":30,"width":30,"height":2,"corner":[-10,-10,10],"body_name":"Reference"})).unwrap();
    h.app.run("solid.move", json!({"bodies":["Reference"],"axis":[0,1,0],"angle":"atan(0.1)","origin":[0,0,10]})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XY","name":"Adjacent"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[2,10]})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[2,0],"p1":[10,10]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let angle = 0.1f64.atan();
    let point = [2.0 * angle.sin(), 0.0, 10.0 + 2.0 * angle.cos()];
    h.app.run("solid.extrude", json!({"sketch":"Adjacent","distance":10,"from":point,"name":"Depth","body_name":"Main","operation":"new"})).unwrap();
    let min_z = h.app.session.world_state().body("Main").unwrap().mesh().bounds().min.z;
    open(&mut h);
    let base = crate::canvas::anchor(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().0;
    assert!((base.z - min_z).abs() < 1e-7, "displayed start{} geometry start{min_z}", base.z);
    // Averaging the two unmerged rectangle loops gives x=3.5; their internal
    // seam must not bias the reference ray used by the extent handle.
    let unmerged_height = 10.0 + 2.0 / angle.cos() - 3.5 * angle.tan();
    assert!((base.z - unmerged_height).abs() > 0.01, "fixture must distinguish the raw-profile centre");
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!((h.app.session.world_state().body("Main").unwrap().mesh().bounds().min.z - min_z).abs() < 1e-7);
}

#[test]
fn rotated_second_occurrence_keeps_saved_from_and_to_points_exact() {
    let mut h = Harness::new();
    let owner = h.app.run("component.create", json!({"name":"Owner"})).unwrap();
    let mut h = make_in(h, json!({"from":[0,0,5],"to":[0,0,15],"to_offset":"2 mm"}));
    let second = h.app.run("occurrence.copy", json!({"component":owner["component"],"translate":[100,0,20]})).unwrap()["occurrence"].clone();
    h.app.run("occurrence.move", json!({"occurrence":second,"axis":[1,0,0],"angle":"17 deg"})).unwrap();
    h.app.run("component.activate", json!({"component":owner["component"],"occurrence":second,"path":[second]})).unwrap();
    let original = extent(&h);
    let before: Vec<_> = h.app.session.world_state().bodies.iter().map(|b| (b.name.clone(), b.mesh().bounds())).collect();
    open(&mut h);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert_eq!(extent(&h), original);
    let after: Vec<_> = h.app.session.world_state().bodies.iter().map(|b| (b.name.clone(), b.mesh().bounds())).collect();
    assert_eq!(after, before);
}

#[test]
fn dragging_to_update_value_reselects_whole_value_box_text() {
    let mut h = make(json!({"distance":"4 mm"}));
    open(&mut h);
    let id = egui::Id::new("sc_canvas_value");
    let selection = |h: &Harness| {
        let range = egui::text_edit::TextEditState::load(&h.ctx, id).unwrap().cursor.char_range().unwrap();
        let (a, b) = (range.primary.index, range.secondary.index);
        (a.min(b).0, a.max(b).0)
    };
    assert_eq!(selection(&h), (0, 4), "the pick selects all of \"4 mm\"");
    h.step(&json!({"drag":[{"handle":"arrow"},{"world":[5,5,15]}]})).unwrap();
    let Some(crate::dialogs::Dialog { kind: crate::dialogs::Kind::Extrude { distance, .. }, .. }) = &h.app.dialog else { panic!("extrude dialog") };
    assert_eq!(distance, "15 mm");
    assert_eq!(selection(&h), (0, 5), "the drag grew the text length; the selection must cover all of it");
}

#[test]
fn symmetric_total_distance_preview_handle_drag_apply_and_reopen_agree() {
    for (convention, cap, total_after_drag) in [("total_distance", 50.0, 120.0), ("legacy_per_side", 100.0, 60.0)] {
        let mut h = make(json!({"direction":"symmetric","distance":"100 mm","convention":convention}));
        h.call("ui.view", json!({"view":"fit"}));
        h.frames(4);
        // Leave room above the fitted solid to grow the arrow without entering the ribbon.
        h.app.cam.distance *= 1.5;
        open(&mut h);
        let expected_tip = h.at(&json!({"world":[5,5,cap]})).unwrap();
        let handle = h.at(&json!({"handle":"arrow"})).unwrap();
        assert!((handle[0] - expected_tip[0]).hypot(handle[1] - expected_tip[1]) < 1.0, "{convention}: {handle:?} expected {expected_tip:?}");
        h.step(&json!({"drag":[{"handle":"arrow"},{"world":[5,5,60]}]})).unwrap();
        let d = h.app.dialog.as_ref().unwrap();
        let crate::dialogs::Kind::Extrude { distance, .. } = &d.kind else { panic!("extrude") };
        assert_eq!(h.app.session.doc.eval(distance, solidcraft_engine::doc::expr::Kind::Length).unwrap(), total_after_drag);
        let commands = crate::dialogs::apply_commands(&h.app, d).unwrap();
        let preview = h.app.session.preview(&commands).unwrap();
        let preview_bounds = preview.after.body("Main").unwrap().mesh().bounds();
        assert!((preview_bounds.min.z + 60.0).abs() < 1e-7 && (preview_bounds.max.z - 60.0).abs() < 1e-7);
        h.step(&json!({"widget":"OK"})).unwrap();
        assert_eq!(h.app.session.world_state().body("Main").unwrap().mesh().bounds(), preview_bounds);
        let path = h.dir.join("symmetric.solidcraft");
        h.app.run("file.save_as", json!({"path":path})).unwrap();
        h.app.run("doc.open", json!({"path":path})).unwrap();
        assert_eq!(h.app.session.world_state().body("Main").unwrap().mesh().bounds(), preview_bounds);
        open(&mut h);
        h.step(&json!({"widget":"OK"})).unwrap();
        assert_eq!(h.app.session.world_state().body("Main").unwrap().mesh().bounds(), preview_bounds);
    }
}

#[test]
fn switching_one_side_to_symmetric_in_actual_editor_keeps_total_thickness() {
    for convention in ["total_distance", "legacy_per_side"] {
        let mut h = make(json!({"convention":convention}));
        open(&mut h);
        h.step(&json!({"widget":"One Side"})).unwrap();
        h.step(&json!({"widget":"Symmetric"})).unwrap();
        let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        let preview = h.app.session.preview(&commands).unwrap();
        let bounds = preview.after.body("Main").unwrap().mesh().bounds();
        assert!((bounds.min.z + 5.0).abs() < 1e-7 && (bounds.max.z - 5.0).abs() < 1e-7);
        h.step(&json!({"widget":"OK"})).unwrap();
        assert_eq!(h.app.session.world_state().body("Main").unwrap().mesh().bounds(), bounds);
        assert_eq!(extent(&h).convention, solidcraft_engine::doc::ExtrudeConvention::TotalDistance);
    }
}

#[test]
fn symmetric_through_all_handle_drag_switches_to_total_distance() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":100,"corner":[0,0,-50],"body_name":"Reference"})).unwrap();
    let mut h = make_in(h, json!({"direction":"symmetric","through_all":true}));
    h.call("ui.view", json!({"view":"fit"}));
    h.frames(4);
    open(&mut h);
    h.step(&json!({"drag":[{"handle":"arrow"},{"world":[5,5,60]}]})).unwrap();
    let d = h.app.dialog.as_ref().unwrap();
    let crate::dialogs::Kind::Extrude { distance, all, .. } = &d.kind else { panic!("extrude") };
    assert!(!all);
    assert_eq!(h.app.session.doc.eval(distance, solidcraft_engine::doc::expr::Kind::Length).unwrap(), 120.0);
    h.step(&json!({"widget":"OK"})).unwrap();
    let bounds = h.app.session.world_state().body("Main").unwrap().mesh().bounds();
    assert!((bounds.min.z + 60.0).abs() < 1e-7 && (bounds.max.z - 60.0).abs() < 1e-7);
}

/// Inspect each held frame, not just the settled end of the drag: the displayed arrow must
/// be at the distance the pointer just wrote, rather than the previous frame's distance.
#[test]
fn extrusion_arrow_tracks_the_current_drag_frame() {
    let mut h = make(json!({"distance":10}));
    open(&mut h);
    let at = h.at(&json!({"handle":"arrow"})).unwrap();
    let at = egui::pos2(at[0] as f32, at[1] as f32);
    h.app.synthetic_mods.ctrl = true;
    h.app.synthetic.push(egui::Event::PointerMoved(at));
    h.frame();
    h.app.synthetic.push(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: h.app.synthetic_mods,
    });
    h.frame();
    for dy in [-20.0, -40.0, -60.0] {
        h.app.synthetic.push(egui::Event::PointerMoved(at + egui::vec2(0.0, dy)));
        h.frame();
        let d = h.app.dialog.as_ref().unwrap();
        let crate::dialogs::Kind::Extrude { distance, .. } = &d.kind else { panic!("extrude") };
        let distance = h.app.session.doc.eval(distance, solidcraft_engine::doc::expr::Kind::Length).unwrap();
        assert!(distance > 10.0);
        let (base, normal) = crate::canvas::anchor(&h.app, d).unwrap();
        let world = base + normal.unwrap() * distance;
        let expected = h.at(&json!({"world":[world.x, world.y, world.z]})).unwrap();
        let drawn = h.at(&json!({"handle":"arrow"})).unwrap();
        assert!((drawn[0] - expected[0]).hypot(drawn[1] - expected[1]) < 0.1, "arrow {drawn:?} trails current value {expected:?}");
    }
}

/// The world point `at` mm along the open extrude's arrow.
fn on_arrow(h: &Harness, at: f64) -> Value {
    let d = h.app.dialog.as_ref().unwrap();
    let (base, normal) = crate::canvas::anchor(&h.app, d).unwrap();
    let p = base + normal.unwrap() * at;
    json!({"world":[p.x, p.y, p.z]})
}

fn dialog_distance(h: &Harness) -> f64 {
    let d = h.app.dialog.as_ref().unwrap();
    let crate::dialogs::Kind::Extrude { distance, .. } = &d.kind else { panic!("extrude") };
    h.app.session.doc.eval(distance, solidcraft_engine::doc::expr::Kind::Length).unwrap()
}

/// The arrow's stem takes the drag as its tip does (#1740): pressed 4 mm along a 10 mm arrow and
/// moved to 9 mm, it grows by the 5 mm the pointer moved, not to where the pointer is.
#[test]
fn the_extrude_arrow_drags_from_its_stem() {
    let mut h = make(json!({"distance":10}));
    open(&mut h);
    let (from, to) = (on_arrow(&h, 4.0), on_arrow(&h, 9.0));
    h.step(&json!({"drag":[from, to]})).unwrap();
    assert!((dialog_distance(&h) - 15.0).abs() < 1e-9, "{}", dialog_distance(&h));
    h.step(&json!({"widget":"OK"})).unwrap();
    let bounds = h.app.session.world_state().body("Main").unwrap().mesh().bounds();
    assert!((bounds.max.z - 15.0).abs() < 1e-7, "{bounds:?}");
}

/// A click on the arrow's stem is the arrow's (#1740): a new extrude of a square sketched on a
/// block keeps its profile picked and its distance. Before, the click went through to the square
/// under the stem and unpicked it.
#[test]
fn a_click_on_the_extrude_arrow_stem_keeps_the_profile() {
    let mut h = Harness::new();
    h.app.home.open = false;
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [60, 20]})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance": 10})),
        ("sketch.create", json!({"plane": {"origin": [0, 0, 10], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}})),
        ("sketch.rectangle.two_point", json!({"p0": [5, 5], "p1": [15, 15]})),
    ] {
        h.app.run(id, p).unwrap();
    }
    h.app.finish_sketch();
    h.app.start("solid.extrude");
    h.frames(4);
    let picked = |h: &Harness| h.app.dialog.as_ref().unwrap().inputs.iter().map(|i| i.items.len()).collect::<Vec<_>>();
    let before = picked(&h);
    assert!(before.iter().any(|n| *n > 0), "{before:?}");
    let distance = dialog_distance(&h);
    let at = on_arrow(&h, distance * 0.3);
    h.step(&json!({"click": at})).unwrap();
    h.frames(2);
    assert_eq!(picked(&h), before);
    assert!((dialog_distance(&h) - distance).abs() < 1e-9, "{} for {distance}", dialog_distance(&h));
}

/// The volume of the body `name`.
fn volume(h: &Harness, name: &str) -> f64 {
    let state = h.app.session.world_state();
    solidcraft_engine::doc::kernel::measure(&state.body(name).unwrap().body).unwrap().volume
}

/// Two Sides with All (#2700): the editor shows Distance 2 and Reverse for a through-all cut, and
/// Reverse swaps the sides: through all down and 3 mm up instead of through all up and 3 mm down.
#[test]
fn two_sides_with_all_shows_distance_2_and_reverse_swaps_the_sides() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length":20,"width":20,"height":40,"corner":[0,0,-30],"body_name":"Main"})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XY","name":"Hole"})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[10,10],"radius":2})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let cut = json!({"sketch":"Hole","through_all":true,"distance2":"3 mm","operation":"cut","targets":["Main"],"name":"Depth"});
    h.app.run("solid.extrude", cut).unwrap();
    h.frames(4);
    // The box runs 10 mm above the sketch and 30 mm below it.
    let area = std::f64::consts::PI * 4.0;
    let depth = |h: &Harness| (16000.0 - volume(h, "Main")) / area;
    assert!((depth(&h) - 13.0).abs() < 0.05, "{}", depth(&h));
    open(&mut h);
    assert!(h.widget("Distance 2").is_some(), "Two Sides with All shows the second side's distance");
    assert!(h.widget("Reverse Direction").is_some(), "Two Sides offers Reverse");
    // The first side's arrow goes up (through all) and the second's down, either side of the
    // hole's centre on screen; Reverse swaps them.
    let centre = h.at(&json!({"world":[10,10,0]})).unwrap()[1] as f32;
    let tips = || (crate::scenario::handle_at("arrow").unwrap().y - centre, crate::scenario::handle_at("arrow2").unwrap().y - centre);
    let (first, second) = tips();
    assert!(first < 0.0 && second > 0.0, "the first side's arrow is above the sketch, the second's below: {first} {second}");
    h.step(&json!({"widget":"Reverse Direction"})).unwrap();
    h.frames(2);
    let (first, second) = tips();
    assert!(first > 0.0 && second < 0.0, "reversed, the first side's arrow is below, the second's above: {first} {second}");
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.session.log);
    let e = extent(&h);
    assert_eq!((e.direction, e.through_all, e.distance2.as_deref()), (solidcraft_engine::doc::Direction::Negative, true, Some("3 mm")));
    assert!((depth(&h) - 33.0).abs() < 0.05, "{}", depth(&h));
    // And it opens reversed again: OK without changes keeps it.
    open(&mut h);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert_eq!(extent(&h), e);
}

/// Without value boxes in the view (Preferences, #9479), nothing covers the model: the command
/// window's Distance takes the keyboard instead, its text selected, and the arrow still drags it.
#[test]
fn without_value_boxes_in_the_view_the_distance_is_typed_in_the_window() {
    let mut h = make(json!({"distance":"4 mm"}));
    h.app.preferences.value_boxes_in_view = false;
    open(&mut h);
    assert!(h.ctx.memory(|m| m.area_rect(egui::Id::new("sc_canvas_area"))).is_none(), "a value box in the view");
    let focused = h.ctx.memory(|m| m.focused());
    assert!(focused.is_some() && focused != Some(egui::Id::new("sc_canvas_value")), "{focused:?}");
    h.step(&json!({"text":"25 mm"})).unwrap();
    h.frames(2);
    assert!((dialog_distance(&h) - 25.0).abs() < 1e-9, "{}", dialog_distance(&h));
    h.step(&json!({"drag":[{"handle":"arrow"},on_arrow(&h, 30.0)]})).unwrap();
    assert!((dialog_distance(&h) - 30.0).abs() < 0.5, "{}", dialog_distance(&h));
    assert!(h.ctx.memory(|m| m.area_rect(egui::Id::new("sc_canvas_area"))).is_none(), "a value box in the view");
}

#[test]
fn editing_face_extrusion_temporarily_shows_its_hidden_source_body() {
    for finish in ["Cancel", "OK"] {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Source"})).unwrap();
        h.app.run("solid.box", json!({"length":2,"width":2,"height":2,"corner":[30,0,0],"body_name":"Other"})).unwrap();
        h.app
            .run("solid.extrude", json!({"face":[5,5,10],"face_body":"Source","distance":5,"operation":"new","body_name":"Extension","name":"Depth"}))
            .unwrap();
        h.app.session.visibility.hidden_bodies.push("Source".into());
        let visibility = h.app.session.visibility.clone();
        let undo = h.app.session.undo.len();
        open(&mut h);
        assert!(h.app.body_visible("Source"));
        assert!(!h.app.session.body_visible("Source"), "the editor must not change persistent visibility");
        assert!(h.app.body_visible("Other"));
        assert!(h.app.session.world_state().body("Extension").is_none(), "the feature's result is rolled off");
        assert_eq!(h.app.session.undo.len(), undo);
        h.app.session.visibility.hidden_bodies.push("Other".into());
        h.step(&json!({"widget":finish})).unwrap();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.session.log);
        assert!(!h.app.body_visible("Source"));
        assert!(!h.app.body_visible("Other"), "unrelated visibility edits are preserved");
        assert_eq!(h.app.session.visibility.hidden_sketches, visibility.hidden_sketches);
        assert_eq!(h.app.session.visibility.shown_sketches, visibility.shown_sketches);
        assert!(h.app.session.world_state().body("Extension").is_some());
    }
}

#[test]
fn failed_face_extrusion_edit_keeps_context_until_escape_and_preserves_visibility() {
    let mut h = Harness::new();
    let owner = h.app.run("component.create", json!({"name":"Part"})).unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Source"})).unwrap();
    h.app.run("solid.extrude", json!({"face":[5,5,10],"face_body":"Source","distance":5,"operation":"new","name":"Depth"})).unwrap();
    let path = vec![owner["occurrence"].as_u64().unwrap()];
    h.app.session.visibility.hidden_occurrences.push(path);
    let visibility = h.app.session.visibility.clone();
    assert!(!h.app.body_visible("Source"));
    open(&mut h);
    assert!(h.app.body_visible("Source"), "a hidden parent must not hide the source during editing");
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"missing_length"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(h.app.dialog.is_some());
    assert!(h.app.body_visible("Source"));
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.dialog.is_none());
    assert!(!h.app.body_visible("Source"));
    assert_eq!(h.app.session.visibility, visibility);
}

/// The Extrude window's Targets: under Operation (none for a New Body), a row per body. A new
/// Join, Cut or Intersect lists the bodies it changes, the visible ones, following what is
/// shown until the list is edited by hand; what is listed is what the feature stores.
mod targets {
    use crate::dialogs::{Kind, TARGETS, apply_commands};
    use crate::scenario::Harness;
    use serde_json::{Value, json};
    use solidcraft_engine::Sel;
    use solidcraft_engine::doc::FeatureKind;

    /// A 40 x 40 x 5 plate, a copy of it where it is, and a rim sketched over the plate's top: on
    /// the plate's face, or on a plane at its height.
    fn plate_and_copy(on_face: bool) -> Harness {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":40,"width":40,"height":5,"body_name":"Plate"})).unwrap();
        h.app.run("solid.move", json!({"bodies":["Plate"],"copy":true})).unwrap();
        let plane = if on_face {
            let st = h.app.session.world_state();
            let faces = st.body("Plate").unwrap().body.faces(0.01).unwrap();
            let top = faces.into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99)).unwrap();
            json!({"face":[20,20,5],"body":"Plate","index":top.index})
        } else {
            json!({"origin":[0,0,5],"x_dir":[1,0,0],"y_dir":[0,1,0]})
        };
        h.app.run("sketch.create", json!({"plane":plane,"name":"Rim"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[2,2],"p1":[38,38]})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[4,4],"p1":[36,36]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h
    }

    fn hide(h: &mut Harness, body: &str) {
        h.app.run("browser.visibility", json!({"items":[body],"visible":false})).unwrap();
    }

    /// The extrude window on the rim's ring, joining.
    fn start_join(h: &mut Harness) {
        let ring = rim_ring(h);
        h.app.session.selection = vec![ring];
        h.app.start("solid.extrude");
        let d = h.app.dialog.as_mut().unwrap();
        if let Kind::Extrude { distance, operation, auto_op, .. } = &mut d.kind {
            *distance = "3 mm".into();
            *operation = 1;
            *auto_op = false;
        }
        h.frames(2);
    }

    fn rim_ring(h: &Harness) -> Sel {
        let st = h.app.session.world_state();
        let ss = st.sketches.iter().find(|ss| ss.name == "Rim").unwrap();
        let index = ss.profiles.iter().position(|p| !p.region.holes.is_empty()).unwrap();
        Sel::Profile { sketch: ss.feature, index }
    }

    fn params(h: &Harness) -> Value {
        apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0).1
    }

    fn volume(h: &Harness, body: &str) -> f64 {
        solidcraft_engine::doc::kernel::measure(&h.app.session.model.state().body(body).unwrap().body).unwrap().volume
    }

    const RIM: f64 = (36.0 * 36.0 - 32.0 * 32.0) * 3.0;

    /// The reported case: a copy hidden, the join goes into the plate alone and the copy stays.
    #[test]
    fn a_join_with_a_body_hidden_targets_the_visible_one() {
        let mut h = plate_and_copy(false);
        hide(&mut h, "Plate (2)");
        start_join(&mut h);
        assert_eq!(params(&h)["targets"], json!(["Plate"]));
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog);
        assert!((volume(&h, "Plate") - (8000.0 + RIM)).abs() < 1e-6);
        assert!((volume(&h, "Plate (2)") - 8000.0).abs() < 1e-6);
        // Showing the copy again changes nothing: the targets are the feature's.
        h.app.run("browser.visibility", json!({"items":["Plate (2)"],"visible":true})).unwrap();
        assert_eq!(h.app.session.model.state().bodies.len(), 2);
        let FeatureKind::Extrude { targets, .. } = &h.app.session.doc.features.last().unwrap().kind else { panic!("an extrude") };
        assert_eq!(targets.iter().map(|t| t.to_string()).collect::<Vec<_>>(), ["Plate"]);
    }

    /// Hiding the other one (the copy shown) targets the copy, even from a sketch on the plate's face.
    #[test]
    fn a_join_from_a_hidden_bodys_face_targets_the_visible_one() {
        let mut h = plate_and_copy(true);
        hide(&mut h, "Plate");
        start_join(&mut h);
        assert_eq!(params(&h)["targets"], json!(["Plate (2)"]));
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(volume(&h, "Plate (2)") > 8000.0 + 100.0, "the copy is joined");
        assert!((volume(&h, "Plate") - 8000.0).abs() < 1e-6);
    }

    /// Every body shown: all of them are listed and stored. A sketch on a shown body's face joins
    /// that body alone, as the command does.
    #[test]
    fn the_list_holds_the_visible_bodies_or_the_face_body() {
        let mut h = plate_and_copy(false);
        start_join(&mut h);
        assert_eq!(params(&h)["targets"], json!(["Plate", "Plate (2)"]));
        assert!(h.widget("2 selected").is_some(), "the chip reads like the other selection fields");
        assert!(h.widget("↖ 2 selected").is_none());
        let mut h = plate_and_copy(true);
        start_join(&mut h);
        assert_eq!(params(&h)["targets"], json!(["Plate"]));
        assert!(h.widget("1 selected").is_some());
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog);
        assert_eq!(h.app.session.model.state().bodies.len(), 2, "the copy stays a body of its own");
        assert!(volume(&h, "Plate") > 8000.0 + 100.0, "the plate is joined");
        assert!((volume(&h, "Plate (2)") - 8000.0).abs() < 1e-6, "the sketch's face body is joined alone");
    }

    /// A pick into To Object moves on to From Object, not to the Targets of a New Body.
    #[test]
    fn a_to_object_pick_moves_on_to_from_object() {
        let mut h = Harness::new();
        h.app.run("component.create", json!({"name":"Base"})).unwrap();
        h.app.run("solid.box", json!({"length":40,"width":40,"height":5,"body_name":"Plate"})).unwrap();
        h.app.run("component.activate", json!({"component":0})).unwrap();
        h.app.run("component.create", json!({"name":"Lid"})).unwrap();
        sketch_profile(&mut h, 30.0, [0.0, 0.0], [40.0, 40.0]);
        h.app.start("solid.extrude");
        if let Kind::Extrude { operation, auto_op, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
            (*operation, *auto_op) = (0, false);
        }
        h.frames(2);
        h.step(&json!({"click":{"handle":"dialog:Extent Type"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:Extent Type:To Object"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:section:Advanced"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:Start"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:Start:From Object"}})).unwrap();
        let labels = |h: &Harness| h.app.dialog.as_ref().unwrap().inputs.iter().map(|i| i.label).collect::<Vec<_>>();
        let to = labels(&h).iter().position(|l| *l == "To Object").unwrap();
        assert!(labels(&h).contains(&"From Object"), "{:?}", labels(&h));
        h.call("ui.dialogInput", json!({"index":to}));
        let st = h.app.session.world_state();
        let top = st.body("Plate").unwrap().body.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99)).unwrap();
        let face = Sel::Face { body: "Plate".into(), index: top.index, point: top.centroid };
        let d = h.app.dialog.as_mut().unwrap();
        d.pick(&h.app.session, face);
        h.frames(2);
        let d = h.app.dialog.as_ref().unwrap();
        assert_eq!(d.inputs.get(d.active).map(|i| i.label), Some("From Object"), "{:?}", labels(&h));
    }

    /// The design active, its plate kept in a component: the plate is listed. A component of its
    /// own, empty: nothing is, and Auto makes a new body there rather than joining the other
    /// component's plate.
    #[test]
    fn bodies_of_other_components_are_listed_only_from_the_design() {
        let mut h = Harness::new();
        h.app.run("component.create", json!({"name":"Base"})).unwrap();
        h.app.run("solid.box", json!({"length":40,"width":40,"height":5,"body_name":"Plate"})).unwrap();
        h.app.run("component.activate", json!({"component":0})).unwrap();
        h.app.run("sketch.create", json!({"plane":{"origin":[0,0,5],"x_dir":[1,0,0],"y_dir":[0,1,0]},"name":"Rim"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[2,2],"p1":[38,38]})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[4,4],"p1":[36,36]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        start_join(&mut h);
        assert_eq!(params(&h)["targets"], json!(["Plate"]));
        h.app.dialog = None;
        h.app.run("component.create", json!({"name":"Lid"})).unwrap();
        h.app.run("sketch.create", json!({"plane":{"origin":[0,0,30],"x_dir":[1,0,0],"y_dir":[0,1,0]},"name":"Top"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,40]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        let st = h.app.session.world_state();
        let top = st.sketches.iter().find(|ss| ss.name == "Top").unwrap().feature;
        h.app.session.selection = vec![Sel::Profile { sketch: top, index: 0 }];
        h.app.start("solid.extrude");
        h.frames(2);
        assert!(h.app.dialog.as_ref().unwrap().inputs.iter().find(|i| i.label == TARGETS).is_some_and(|i| i.items.is_empty()));
        let p = params(&h);
        assert_eq!((p["operation"].as_str(), p.get("targets")), (Some("new"), None), "{p}");
        h.step(&json!({"key":"Enter"})).unwrap();
        assert_eq!(h.app.session.model.state().bodies.len(), 2);
    }

    /// A box named `body` in a new component `name` (made under the active one, and left active).
    fn part(h: &mut Harness, name: &str, body: &str, x: f64) {
        h.app.run("component.create", json!({"name":name})).unwrap();
        h.app.run("solid.box", json!({"corner":[x,0,0],"length":20,"width":20,"height":5,"body_name":body})).unwrap();
    }

    /// A rectangle sketched on a plane at height `z`, from `p0` to `p1`, picked as the profile.
    fn sketch_profile(h: &mut Harness, z: f64, p0: [f64; 2], p1: [f64; 2]) {
        h.app.run("sketch.create", json!({"plane":{"origin":[0,0,z],"x_dir":[1,0,0],"y_dir":[0,1,0]},"name":"Pick"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":p0,"p1":p1})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        let st = h.app.session.world_state();
        let sketch = st.sketches.iter().find(|ss| ss.name == "Pick").unwrap().feature;
        h.app.session.selection = vec![Sel::Profile { sketch, index: 0 }];
    }

    /// The window's Auto on what is selected: the operation it lands on and the targets it sends.
    fn auto(h: &mut Harness) -> (String, Value) {
        h.app.start("solid.extrude");
        h.frames(3);
        let p = params(h);
        (p["operation"].as_str().unwrap_or_default().to_string(), p.get("targets").cloned().unwrap_or(Value::Null))
    }

    /// What the window starts with is what the command takes on its own, less hidden bodies: from
    /// the design with parts in two components a new body, not a lump in one of them; a face of
    /// another component's body picked in an empty component a new body; with hidden bodies only a
    /// new body, not a refusal; from a component holding one part, that part; a Cut from an empty
    /// component every shown body.
    #[test]
    fn the_default_is_what_the_command_takes() {
        let mut h = Harness::new();
        part(&mut h, "A", "ABox", 0.0);
        h.app.run("component.activate", json!({"component":0})).unwrap();
        part(&mut h, "B", "BBox", 40.0);
        h.app.run("component.activate", json!({"component":0})).unwrap();
        sketch_profile(&mut h, 0.0, [100.0, 100.0], [110.0, 110.0]);
        assert_eq!(auto(&mut h), ("new".to_string(), Value::Null), "two components: a new body at the design");
        h.app.dialog = None;

        let mut h = Harness::new();
        part(&mut h, "Base", "Plate", 0.0);
        h.app.run("component.activate", json!({"component":0})).unwrap();
        h.app.run("component.create", json!({"name":"Lid"})).unwrap();
        let st = h.app.session.world_state();
        let top = st.body("Plate").unwrap().body.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99)).unwrap();
        h.app.session.selection = vec![Sel::Face { body: "Plate".into(), index: top.index, point: top.centroid }];
        assert_eq!(auto(&mut h).0, "new", "a face of another component's body, from an empty component");
        h.app.dialog = None;

        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":20,"width":20,"height":5,"body_name":"Hidden"})).unwrap();
        hide(&mut h, "Hidden");
        sketch_profile(&mut h, 5.0, [2.0, 2.0], [8.0, 8.0]);
        assert_eq!(auto(&mut h), ("new".to_string(), Value::Null), "hidden bodies only");
        h.step(&json!({"key":"Enter"})).unwrap();
        assert_eq!(h.app.session.model.state().bodies.len(), 2, "OK makes the new body");

        let mut h = Harness::new();
        h.app.run("component.create", json!({"name":"Assembly"})).unwrap();
        part(&mut h, "Part", "PBox", 0.0);
        h.app.run("component.activate", json!({"component":"Assembly"})).unwrap();
        sketch_profile(&mut h, 5.0, [2.0, 2.0], [8.0, 8.0]);
        let (op, targets) = auto(&mut h);
        assert_ne!(op, "new");
        assert_eq!(targets, json!(["PBox"]), "a component with no body of its own works on its part's (an opened STEP assembly)");
        h.app.dialog = None;

        let mut h = Harness::new();
        h.app.run("component.create", json!({"name":"Sub"})).unwrap();
        part(&mut h, "A", "ABox", 0.0);
        h.app.run("component.activate", json!({"component":"Sub"})).unwrap();
        part(&mut h, "B", "BBox", 30.0);
        h.app.run("component.activate", json!({"component":"Sub"})).unwrap();
        sketch_profile(&mut h, 5.0, [10.0, 5.0], [40.0, 15.0]);
        h.app.start("solid.extrude");
        if let Kind::Extrude { operation, auto_op, distance, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
            (*operation, *auto_op, *distance) = (2, false, "-2 mm".into());
        }
        h.frames(3);
        assert_eq!(params(&h)["targets"], json!(["ABox", "BBox"]), "a Cut from an empty component takes every body");
    }

    /// Showing or hiding a body while the window is open changes the list, until a body is taken
    /// out by hand: then it stays as edited.
    #[test]
    fn the_list_follows_visibility_until_edited() {
        let mut h = plate_and_copy(false);
        start_join(&mut h);
        hide(&mut h, "Plate (2)");
        h.frames(2);
        assert_eq!(params(&h)["targets"], json!(["Plate"]));
        h.app.run("browser.visibility", json!({"items":["Plate (2)"],"visible":true})).unwrap();
        h.frames(2);
        assert_eq!(params(&h)["targets"], json!(["Plate", "Plate (2)"]));
        h.step(&json!({"click":{"handle":"dialog:list:Targets:Plate (2):remove"}})).unwrap();
        h.frames(2);
        assert_eq!(params(&h)["targets"], json!(["Plate"]), "the row's × takes it out");
        hide(&mut h, "Plate");
        h.frames(2);
        assert_eq!(params(&h)["targets"], json!(["Plate"]), "an edited list stays");
    }

    /// The row sits under Operation and is not there for a New Body; an empty list is refused.
    #[test]
    fn the_row_is_under_operation_and_not_for_a_new_body() {
        let mut h = plate_and_copy(false);
        start_join(&mut h);
        let (op, row) = (crate::scenario::handle_at("dialog:row:Operation").unwrap(), crate::scenario::handle_at("dialog:row:Targets").unwrap());
        assert!(row.y > op.y, "Targets {row:?} under Operation {op:?}");
        let options = crate::scenario::handle_at("dialog:section:Options").unwrap();
        for name in ["Plate", "Plate (2)"] {
            let body = crate::scenario::handle_at(&format!("dialog:list:Targets:{name}:remove")).unwrap();
            assert!(body.y > row.y && body.y < options.y, "a row per body under Targets: {name} {body:?}");
        }
        if let Kind::Extrude { operation, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
            *operation = 0;
        }
        crate::scenario::clear_handles();
        h.frames(2);
        assert!(crate::scenario::handle_at("dialog:row:Targets").is_none(), "no Targets for a new body");
        assert!(params(&h).get("targets").is_none());
        if let Kind::Extrude { operation, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
            *operation = 1;
        }
        h.frames(2);
        for name in ["Plate", "Plate (2)"] {
            h.step(&json!({"click":{"handle":format!("dialog:list:Targets:{name}:remove")}})).unwrap();
            h.frames(2);
        }
        let refused = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap_err();
        assert!(refused.contains("New Body"), "{refused}");
    }

    /// Clicking a listed body in the viewport takes it out; what is left is what the join changes,
    /// and Edit shows and keeps it.
    #[test]
    fn picked_targets_are_sent_and_kept_when_editing() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":40,"width":40,"height":5,"body_name":"Plate"})).unwrap();
        h.app.run("solid.box", json!({"length":40,"width":40,"height":5,"corner":[0,0,10],"operation":"new","body_name":"Ref"})).unwrap();
        h.app.run("sketch.create", json!({"plane":{"origin":[0,0,5],"x_dir":[1,0,0],"y_dir":[0,1,0]},"name":"Rim"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[2,2],"p1":[38,38]})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[4,4],"p1":[36,36]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        start_join(&mut h);
        let d = h.app.dialog.as_ref().unwrap();
        assert_eq!(d.active, 0, "the profiles stay the active input");
        let index = d.inputs.iter().position(|i| i.label == TARGETS).unwrap();
        assert_eq!(d.inputs[index].items, vec![Sel::Body { name: "Plate".into() }, Sel::Body { name: "Ref".into() }]);
        assert!(!h.app.highlighted().iter().any(|x| matches!(x, Sel::Body { .. })), "targets light up only while their row is active");
        h.call("ui.dialogInput", json!({"index":index}));
        assert!(h.app.highlighted().contains(&Sel::Body { name: "Ref".into() }));
        h.call("ui.view", json!({"view":"front"}));
        h.frames(2);
        h.step(&json!({"click":{"world":[20,0,12.5]}})).unwrap();
        assert_eq!(h.app.dialog.as_ref().unwrap().inputs[index].items, vec![Sel::Body { name: "Plate".into() }]);
        assert_eq!(params(&h)["targets"], json!(["Plate"]));
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog);
        let feature = h.app.session.doc.features.last().unwrap().id;
        let before = serde_json::to_value(&h.app.session.doc.feature(feature).unwrap().kind).unwrap();
        h.app.edit_feature(feature);
        h.frames(2);
        let d = h.app.dialog.as_ref().unwrap();
        let index = d.inputs.iter().position(|i| i.label == TARGETS).unwrap();
        assert_eq!(d.inputs[index].items, vec![Sel::Body { name: "Plate".into() }]);
        let edited = apply_commands(&h.app, d).unwrap();
        assert_eq!(edited[0].1["params"]["targets"], json!(["Plate"]));
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog);
        assert_eq!(serde_json::to_value(&h.app.session.doc.feature(feature).unwrap().kind).unwrap(), before);
    }

    /// Editing a feature made with no targets keeps it that way, whatever is hidden now; one
    /// stored with `*` keeps it, and the field names each.
    #[test]
    fn editing_keeps_a_feature_without_targets_as_it_is() {
        let mut h = plate_and_copy(false);
        h.app.run("solid.extrude", json!({"sketch":"Rim","profiles":[0],"distance":3,"operation":"join","name":"Lip"})).unwrap();
        let feature = h.app.session.doc.find_feature("Lip").unwrap().id;
        hide(&mut h, "Plate");
        h.app.edit_feature(feature);
        h.frames(2);
        let edited = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        assert!(edited[0].1["params"].get("targets").is_none(), "{}", edited[0].1);
        assert!(h.widget("All bodies of the component").is_some());
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(h.app.dialog.is_none());
        h.app.run("solid.extrude", json!({"sketch":"Rim","profiles":[0],"distance":3,"operation":"join","targets":["*"],"name":"Every"})).unwrap();
        let feature = h.app.session.doc.find_feature("Every").unwrap().id;
        h.app.edit_feature(feature);
        h.frames(2);
        let edited = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        assert_eq!(edited[0].1["params"]["targets"], json!(["*"]), "{}", edited[0].1);
        assert!(h.widget("All bodies").is_some(), "a stored `*` is every body of the design");
        assert!(h.widget("All bodies of the component").is_none());
    }

    /// An edit that empties a stored target list is refused, as a new Join is.
    #[test]
    fn an_edit_that_empties_its_targets_is_refused() {
        let mut h = plate_and_copy(false);
        h.app.run("solid.extrude", json!({"sketch":"Rim","profiles":[0],"distance":3,"operation":"join","targets":["Plate"],"name":"Lip"})).unwrap();
        let feature = h.app.session.doc.find_feature("Lip").unwrap().id;
        h.app.edit_feature(feature);
        h.frames(2);
        let d = h.app.dialog.as_mut().unwrap();
        d.inputs.iter_mut().find(|i| i.label == TARGETS).unwrap().items.clear();
        let refused = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap_err();
        assert!(refused.contains("New Body"), "{refused}");
    }

    /// A face picked before Extrude is still extruded (Press Pull style), not taken as a target.
    #[test]
    fn a_preselected_face_stays_the_profile_and_a_preselected_body_is_the_target() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":20,"width":20,"height":10,"body_name":"Base"})).unwrap();
        let st = h.app.session.world_state();
        let base = st.body("Base").unwrap();
        let top = base.body.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99)).unwrap();
        h.app.session.selection = vec![Sel::Face { body: "Base".into(), index: top.index, point: top.centroid }];
        h.app.start("solid.extrude");
        let d = h.app.dialog.as_ref().unwrap();
        assert!(matches!(d.inputs[0].items.first(), Some(Sel::Face { .. })), "{:?}", d.inputs);
        assert_eq!(d.inputs.iter().find(|i| i.label == TARGETS).unwrap().items, vec![Sel::Body { name: "Base".into() }], "the face's body");
        h.app.dialog = None;
        h.app.session.selection = vec![Sel::Body { name: "Base".into() }];
        h.app.start("solid.extrude");
        let d = h.app.dialog.as_ref().unwrap();
        assert_eq!(d.inputs.iter().find(|i| i.label == TARGETS).unwrap().items, vec![Sel::Body { name: "Base".into() }]);
    }
}
