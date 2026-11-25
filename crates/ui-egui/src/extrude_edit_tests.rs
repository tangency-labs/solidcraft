//! Exercise the actual feature editor, including frames rendered before OK.
use crate::scenario::Harness;
use serde_json::{Value, json};
use solidcraft_engine::doc::{Extent, FeatureKind};
use solidcraft_engine::geom::MeshGeom;

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
fn clearing_edited_extrude_offset_keeps_the_field_available_for_replacement() {
    for params in [json!({"start_offset":"5 mm"}), json!({"start_offset":"0 mm"}), json!({"from":[0,0,5],"start_offset":"5 mm"})] {
        let mut h = make(params);
        let original = extent(&h);
        let undo = h.app.session.undo.len();
        open(&mut h);
        h.step(&json!({"click":{"handle":"dialog:Offset"}})).unwrap();
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        h.step(&json!({"key":"Backspace"})).unwrap();
        h.frames(2);
        assert!(crate::scenario::handle_at("dialog:Offset").is_some(), "clearing a value must keep its field visible");
        h.step(&json!({"text":"7 mm"})).unwrap();
        h.step(&json!({"widget":"OK"})).unwrap();
        assert!(h.app.dialog.is_none());
        assert_eq!(extent(&h).start_offset.as_deref(), Some("7 mm"));
        assert_eq!(extent(&h).from, original.from);
        assert_eq!(h.app.session.undo.len(), undo + 1);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(extent(&h), original);
    }
}

#[test]
fn new_extrude_keeps_empty_offset_until_profile_plane_is_selected() {
    let mut h = make(json!({}));
    h.app.run("timeline.delete", json!({"feature":"Depth"})).unwrap();
    let sketch = h.app.session.doc.find_feature("Base").unwrap().id;
    h.app.session.selection = vec![solidcraft_engine::Sel::Profile { sketch, index: 0 }];
    h.app.start("solid.extrude");
    h.frames(8);
    h.step(&json!({"click":{"handle":"dialog:section:Advanced"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Start"}})).unwrap();
    h.step(&json!({"widget":"Offset"})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Offset"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"key":"Backspace"})).unwrap();
    h.frames(2);
    assert!(crate::scenario::handle_at("dialog:Offset").is_some());
    h.step(&json!({"text":"3 mm"})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Start"}})).unwrap();
    h.step(&json!({"widget":"Profile Plane"})).unwrap();
    assert!(crate::scenario::handle_at("dialog:Offset").is_none());
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none());
    let FeatureKind::Extrude { extent, .. } = &h.app.session.doc.features.last().unwrap().kind else { panic!("extrude") };
    assert!(extent.start_offset.is_none());
    assert!(extent.from.is_none());
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
    assert_eq!(crate::open_meshing::session_world(&h.app.session).body("Main").unwrap().mesh().bounds().max.z, 10.0);
    open(&mut h);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "the restored feature can be edited normally");
}

// An editor kept open over an Undo refused its OK and Cancel afterwards, and Redo then lost the
// edited feature's place (#10688): the Undo cancels it first, as starting another tool does.
#[test]
fn undo_that_keeps_the_edited_feature_cancels_its_editor_first() {
    let mut h = make(json!({}));
    let feature = h.app.session.doc.find_feature("Depth").unwrap().id;
    h.app.run("solid.box", json!({"corner":[30,0,0],"length":2,"width":2,"height":2})).unwrap();
    let full = h.app.session.doc.clone();
    open(&mut h);
    h.app.start("edit.undo");
    h.frames(8);
    assert!(h.app.session.doc.feature(feature).is_some());
    assert!(h.app.dialog.is_none() && h.app.editing_state.is_none());
    assert!(h.app.session.feature_edit_sketch.is_none());
    assert_eq!(h.app.session.doc.features.len(), 2);
    assert_eq!(h.app.session.doc.marker, None, "the editor's rollback is put back before the Undo");
    assert_eq!(h.app.session.redo.len(), 1);

    h.app.start("edit.redo");
    h.frames(8);
    assert_eq!(*h.app.session.doc, *full, "Undo then Redo leaves the design as it was");
    open(&mut h);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "the feature can be edited again");
    assert!(h.app.status.as_ref().is_none_or(|s| !s.0.contains("changed while the feature editor was open")), "{:?}", h.app.status);
}

// A History step while an editor is open moved from the editor's rollback and left OK and
// Cancel refusing the design (#10688): the step cancels the editor, then moves from the marker.
#[test]
fn a_history_step_cancels_the_editor_and_steps_from_the_design_marker() {
    let mut h = make(json!({}));
    h.app.run("solid.box", json!({"corner":[30,0,0],"length":2,"width":2,"height":2})).unwrap();
    open(&mut h);
    assert_eq!(h.app.session.doc.marker, Some(1), "Edit Feature rolls back to the extrude");
    crate::timeline::run_transport(&mut h.app, crate::ui_kit::HistoryTransport::Back);
    h.frames(8);
    assert!(h.app.dialog.is_none() && h.app.editing_state.is_none());
    assert_eq!(h.app.session.doc.marker, Some(2), "one step back from the end, not from the editor's rollback");
    crate::timeline::run_transport(&mut h.app, crate::ui_kit::HistoryTransport::Last);
    h.frames(8);
    assert_eq!(h.app.session.doc.marker, None);
    assert_eq!(h.app.session.world_state().unwrap().body("Main").unwrap().mesh().bounds().max.z, 10.0);
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
    assert_eq!(crate::open_meshing::session_world(&h.app.session).body("Main").unwrap().mesh().bounds().max.z, 10.0);
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
    let state = h.app.session.world_state().unwrap();
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
        let before = h.app.session.world_state().unwrap().body("Main").unwrap().mesh().bounds();
        open(&mut h);
        h.step(&json!({"widget":"OK"})).unwrap();
        assert!(h.app.dialog.is_none(), "{p}: {:?}", h.app.session.log);
        assert_eq!(extent(&h), original, "{p}");
        assert_eq!(h.app.session.doc.find_feature("Depth").unwrap().param_names, names, "{p}");
        assert_eq!(h.app.session.world_state().unwrap().body("Main").unwrap().mesh().bounds(), before, "{p}");
    }
}

#[test]
fn negative_edit_keeps_downstream_distance_reference_positive() {
    let mut h = make(json!({"direction":"negative","distance":"10"}));
    h.app.run("solid.box", json!({"length":10,"width":10,"height":"d1","corner":[30,0,0],"body_name":"Dependent"})).unwrap();
    open(&mut h);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert_eq!(h.app.session.doc.eval("d1", solidcraft_engine::doc::expr::Kind::Length).unwrap(), 10.0);
    assert_eq!(h.app.session.world_state().unwrap().body("Dependent").unwrap().mesh().bounds().max.z, 10.0);
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
    assert_eq!(h.app.session.world_state().unwrap().body("Main").unwrap().mesh().bounds().max.z, 19.0);
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
    assert_eq!(h.app.session.world_state().unwrap().body("Main").unwrap().mesh().bounds().max.z, 37.0);
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
    let state = h.app.session.world_state().unwrap();
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
    assert_eq!(h.app.session.world_state().unwrap().body("Main").unwrap().mesh().bounds().max.z, 21.0);
    let path = h.dir.join("extent.solidcraft");
    h.app.run("file.save", json!({"path":path})).unwrap();
    h.app.run("doc.open", json!({"path":path})).unwrap();
    assert_eq!(extent(&h).to_offset.as_deref(), Some("clearance"));
    assert_eq!(h.app.session.world_state().unwrap().body("Main").unwrap().mesh().bounds().max.z, 21.0);
}

#[test]
fn from_object_start_can_be_explicitly_changed_to_profile_plane() {
    let mut h = make(json!({"from":[0,0,5]}));
    open(&mut h);
    h.step(&json!({"click":{"handle":"dialog:Start"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Start:Profile Plane"}})).unwrap();
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(extent(&h).from.is_none());
    assert_eq!(h.app.session.world_state().unwrap().body("Main").unwrap().mesh().bounds().min.z, 0.0);
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
    assert_eq!(h.app.session.world_state().unwrap().body("Main").unwrap().mesh().bounds().min.z, -10.0);
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
    assert!(h.app.session.world_state().unwrap().body("RedoBody").is_some());
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
        let state = h.app.session.world_state().unwrap();
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
        assert_eq!(h.app.session.world_state().unwrap().body("Main").unwrap().mesh().bounds(), bounds);
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
    let mesh = h.app.session.world_state().unwrap().body("Main").unwrap().mesh();
    let min_z = mesh.bounds().min.z;
    // The extrude starts on the slanted face, so where it starts under the merged profiles'
    // middle (x = 5, y = 5) is where a ray up through the solid first meets it.
    let (t, _) = mesh.raycast(solidcore_record::Vec3::new(5.0, 5.0, 0.0), solidcore_record::Vec3::new(0.0, 0.0, 1.0)).unwrap();
    let start_z = t;
    assert!((start_z - (10.0 + 2.0 / angle.cos() - 5.0 * angle.tan())).abs() < 1e-6, "the start follows the face: {start_z}");
    open(&mut h);
    let base = crate::canvas::anchor(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().0;
    assert!((base.z - start_z).abs() < 1e-6, "displayed start {} geometry start {start_z}", base.z);
    // Averaging the two unmerged rectangle loops gives x=3.5; their internal
    // seam must not bias the reference ray used by the extent handle.
    let unmerged_height = 10.0 + 2.0 / angle.cos() - 3.5 * angle.tan();
    assert!((base.z - unmerged_height).abs() > 0.01, "fixture must distinguish the raw-profile centre");
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!((h.app.session.world_state().unwrap().body("Main").unwrap().mesh().bounds().min.z - min_z).abs() < 1e-7);
}

/// An extrude from a design saved before From followed a slanted face (`from_level`) still
/// starts at the face's level, and the window's arrow starts there too, not on the slope.
#[test]
fn an_old_extrude_kept_on_its_flat_start_shows_the_arrow_at_that_level() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":30,"width":30,"height":2,"corner":[-10,-10,10],"body_name":"Reference"})).unwrap();
    h.app.run("solid.move", json!({"bodies":["Reference"],"axis":[0,1,0],"angle":"atan(0.1)","origin":[0,0,10]})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XY","name":"Square"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let angle = 0.1f64.atan();
    let point = [2.0 * angle.sin(), 0.0, 10.0 + 2.0 * angle.cos()];
    h.app.run("solid.extrude", json!({"sketch":"Square","distance":10,"from":point,"name":"Depth","body_name":"Main","operation":"new"})).unwrap();
    h.app.run("timeline.edit", json!({"feature":"Depth","set":{"extent":{"from_level":true}}})).unwrap();
    let min_z = h.app.session.world_state().unwrap().body("Main").unwrap().mesh().bounds().min.z;
    let max_z = h.app.session.world_state().unwrap().body("Main").unwrap().mesh().bounds().max.z;
    assert!((max_z - min_z - 10.0).abs() < 1e-6, "the old start is flat: z {min_z} to {max_z}");
    open(&mut h);
    let base = crate::canvas::anchor(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().0;
    assert!((base.z - min_z).abs() < 1e-6, "displayed start {} geometry start {min_z}", base.z);
}

/// With To Object, the arrow starts where the build does (#18825). A square on the plane x 3,
/// past the end of a slot whose wall slants across X (x = 4 (y + 11) / 7), extruded From that
/// wall To the block's side: alone, the extrude starts on the wall's plane under the square's
/// middle, x 4 (9 / 7); with Two Sides it starts at the From pick's level, x 20 / 7, as the build
/// does, and the arrow follows it there.
#[test]
fn the_arrow_from_a_slanted_face_with_to_starts_where_the_build_does() {
    for (two_sides, start_x) in [(false, 36.0 / 7.0), (true, 20.0 / 7.0)] {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"corner":[-10,-10,0],"length":20,"width":20,"height":10,"body_name":"Block"})).unwrap();
        h.app.run("sketch.create", json!({"plane":"XY","name":"SlotSketch"})).unwrap();
        h.app.run("sketch.line", json!({"closed":true,"points":[[0,-11],[2,-11],[6,-4],[4,-4]]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("solid.extrude", json!({"sketch":"SlotSketch","distance":20,"operation":"cut","name":"Slot"})).unwrap();
        h.app.run("sketch.create", json!({"plane":"YZ","offset":3,"name":"Across"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[-3,4],"p1":[-1,6]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        let mut p = json!({"sketch":"Across","from":[20.0 / 7.0, -6, 5],"to":[10, 5, 5],"operation":"new","name":"Depth","body_name":"Fill"});
        if two_sides {
            p["distance2"] = json!(3);
        }
        h.app.run("solid.extrude", p).unwrap();
        h.frames(4);
        // Where the body starts under the square's middle (y -2, z 5): a ray along +X first
        // meets it there, 3 mm before the start with Two Sides.
        let mesh = h.app.session.world_state().unwrap().body("Fill").unwrap().mesh();
        let (t, _) = mesh.raycast(solidcore_record::Vec3::new(-20.0, -2.0, 5.0), solidcore_record::Vec3::new(1.0, 0.0, 0.0)).unwrap();
        let built = -20.0 + t + if two_sides { 3.0 } else { 0.0 };
        assert!((built - start_x).abs() < 1e-6, "two sides {two_sides}: the build starts at x {built}");
        open(&mut h);
        let base = crate::canvas::anchor(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().0;
        assert!((base.x - start_x).abs() < 1e-6, "two sides {two_sides}: the arrow starts at x {} where the build starts at {start_x}", base.x);
    }
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
    let before: Vec<_> = h.app.session.world_state().unwrap().bodies.iter().map(|b| (b.name.clone(), b.mesh().bounds())).collect();
    open(&mut h);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert_eq!(extent(&h), original);
    let after: Vec<_> = h.app.session.world_state().unwrap().bodies.iter().map(|b| (b.name.clone(), b.mesh().bounds())).collect();
    assert_eq!(after, before);
}

#[test]
fn dragging_to_update_value_reselects_whole_value_box_text() {
    let mut h = make(json!({"distance":"4 mm"}));
    open(&mut h);
    let id = egui::Id::unique("sc_canvas_value");
    let selection = |h: &Harness| {
        let range = egui::text_edit::TextEditState::load(&h.ctx, id).unwrap().cursor.char_range().unwrap();
        let (a, b) = (range.primary.index, range.secondary.index);
        (a.min(b).0, a.max(b).0)
    };
    assert_eq!(selection(&h), (0, 4), "the pick selects all of \"4 mm\"");
    h.step(&json!({"drag":[{"handle":"arrow"},{"world":[5,5,15]}]})).unwrap();
    let Some(crate::dialogs::Dialog { kind: crate::dialogs::Kind::Extrude { distance, .. }, .. }) = &h.app.dialog else { panic!("extrude dialog") };
    // Snapped drag writes Length decimals (and enough places for the snap step) (#9035).
    assert!(distance.starts_with("15"), "{distance}");
    assert!(distance.ends_with(" mm"), "{distance}");
    assert_ne!(distance, "15 mm", "snapped drag pads to Length decimals: {distance}");
    assert_eq!(selection(&h), (0, distance.len()), "the drag grew the text length; the selection must cover all of it ({distance})");
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
        assert_eq!(h.app.session.world_state().unwrap().body("Main").unwrap().mesh().bounds(), preview_bounds);
        let path = h.dir.join("symmetric.solidcraft");
        h.app.run("file.save_as", json!({"path":path})).unwrap();
        h.app.run("doc.open", json!({"path":path})).unwrap();
        assert_eq!(h.app.session.world_state().unwrap().body("Main").unwrap().mesh().bounds(), preview_bounds);
        open(&mut h);
        h.step(&json!({"widget":"OK"})).unwrap();
        assert_eq!(h.app.session.world_state().unwrap().body("Main").unwrap().mesh().bounds(), preview_bounds);
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
        assert_eq!(h.app.session.world_state().unwrap().body("Main").unwrap().mesh().bounds(), bounds);
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
    assert_eq!(value_box_texts(&h), ["Distance", "All"], "the box names the extent until the drag");
    h.step(&json!({"drag":[{"handle":"arrow"},{"world":[5,5,60]}]})).unwrap();
    let d = h.app.dialog.as_ref().unwrap();
    let crate::dialogs::Kind::Extrude { distance, all, .. } = &d.kind else { panic!("extrude") };
    assert!(!all);
    assert_eq!(h.app.session.doc.eval(distance, solidcraft_engine::doc::expr::Kind::Length).unwrap(), 120.0);
    h.step(&json!({"widget":"OK"})).unwrap();
    let bounds = h.app.session.world_state().unwrap().body("Main").unwrap().mesh().bounds();
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
    hold_free_drag_key(&mut h);
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
    let bounds = h.app.session.world_state().unwrap().body("Main").unwrap().mesh().bounds();
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
    let state = h.app.session.world_state().unwrap();
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

/// Two Sides with a negative Distance 1: its side turns over, but Distance 2 stays where Direction
/// puts it, and has its own value box like Distance 1.
#[test]
fn two_sides_second_arrow_stays_put_when_the_first_distance_turns_negative() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY","name":"Disc"})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[10,10],"radius":2})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("solid.extrude", json!({"sketch":"Disc","distance":"-5 mm","distance2":"3 mm","name":"Depth"})).unwrap();
    h.frames(4);
    open(&mut h);
    assert!(h.widget("Distance 2").is_some(), "Two Sides shows the second side's distance");
    let centre = h.at(&json!({"world":[10,10,0]})).unwrap()[1] as f32;
    let tips = || (crate::scenario::handle_at("arrow").unwrap().y - centre, crate::scenario::handle_at("arrow2").unwrap().y - centre);
    let (first, second) = tips();
    assert!(first > 0.0 && second > 0.0, "the first arrow turned below the sketch, the second stayed below it: {first} {second}");
}

/// The texts painted in the main value box in the view, left to right.
fn value_box_texts(h: &Harness) -> Vec<String> {
    fn walk(shape: &egui::Shape, area: egui::Rect, out: &mut Vec<(f32, String)>) {
        match shape {
            egui::Shape::Text(t) if area.contains(t.pos) && !t.galley.job.text.trim().is_empty() => {
                out.push((t.pos.x, t.galley.job.text.trim().to_string()));
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, area, out)),
            _ => {}
        }
    }
    let area = h.ctx.memory(|m| m.area_rect(egui::Id::unique("sc_canvas_area"))).expect("the value box in the view");
    let mut texts = Vec::new();
    h.shapes.iter().for_each(|s| walk(&s.shape, area, &mut texts));
    texts.sort_by(|a, b| a.0.total_cmp(&b.0));
    texts.into_iter().map(|(_, text)| text).collect()
}

/// The hint the main value box's field gives a screen reader while it is empty.
fn value_box_hint(h: &Harness) -> String {
    let id = egui::Id::unique("sc_canvas_value").accesskit_id();
    let nodes = &h.output.accesskit_update.as_ref().expect("the accessibility tree").nodes;
    nodes.iter().find(|(node, _)| *node == id).and_then(|(_, node)| node.placeholder()).unwrap_or_default().to_owned()
}

/// A 20 x 20 x 40 box that runs 10 mm above the sketch plane and 30 mm below it, cut by a
/// circle with `params`, its dialog open on the cut.
fn open_cut(params: Value) -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length":20,"width":20,"height":40,"corner":[0,0,-30],"body_name":"Main"})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XY","name":"Hole"})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[10,10],"radius":2})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let mut cut = json!({"sketch":"Hole","operation":"cut","targets":["Main"],"name":"Depth"});
    cut.as_object_mut().unwrap().extend(params.as_object().unwrap().clone());
    h.app.run("solid.extrude", cut).unwrap();
    h.frames(4);
    open(&mut h);
    h
}

fn set_all(h: &mut Harness, on: bool) {
    let Some(crate::dialogs::Dialog { kind: crate::dialogs::Kind::Extrude { all, .. }, .. }) = &mut h.app.dialog else { panic!("extrude dialog") };
    *all = on;
    h.frames(2);
}

fn distance_and_all(h: &Harness) -> (String, bool) {
    let Some(crate::dialogs::Dialog { kind: crate::dialogs::Kind::Extrude { distance, all, .. }, .. }) = &h.app.dialog else {
        panic!("extrude dialog")
    };
    (distance.clone(), *all)
}

/// Extent Type All (#18765): the depth of the Distance kept from before is not what the cut
/// reaches, so the box in the view names the extent in place of it. A number typed there is the
/// new Distance and ends All.
#[test]
fn with_extent_all_the_value_box_names_the_extent_until_a_distance_is_typed() {
    let mut h = open_cut(json!({"distance":"10 mm","direction":"negative"}));
    assert_eq!(value_box_texts(&h), ["Distance", "10 mm"]);
    assert_eq!(value_box_hint(&h), "");
    set_all(&mut h, true);
    assert_eq!(value_box_texts(&h), ["Distance", "All"], "the arrow is at the through-all reach, not at 10 mm");
    assert_eq!(value_box_hint(&h), "All", "the field says so to a screen reader too");
    // A space typed there is not a Distance.
    h.step(&json!({"text":" "})).unwrap();
    h.frames(2);
    assert_eq!(distance_and_all(&h), ("10 mm".into(), true), "a space alone keeps All");
    assert_eq!(value_box_texts(&h), ["Distance", "All"]);
    // Back to Distance, the kept value is shown again.
    set_all(&mut h, false);
    assert_eq!(value_box_texts(&h), ["Distance", "10 mm"]);
    set_all(&mut h, true);
    h.step(&json!({"text":"5"})).unwrap();
    h.frames(2);
    assert_eq!(distance_and_all(&h), ("5".into(), false), "typing in the box ends All");
    assert_eq!(value_box_texts(&h), ["Distance", "5"]);
    // The same text as the kept Distance is an entry too.
    set_all(&mut h, true);
    h.step(&json!({"text":"5"})).unwrap();
    h.frames(2);
    assert_eq!(distance_and_all(&h), ("5".into(), false), "typing the kept Distance again ends All");
}

/// With All the kept Distance still picks the side by its sign (#10415). The box names the
/// extent for a negative one as well, and the cut stays on its side.
#[test]
fn with_extent_all_a_negative_kept_distance_keeps_its_side() {
    let mut h = open_cut(json!({"distance":"-10 mm"}));
    assert_eq!(value_box_texts(&h), ["Distance", "-10 mm"]);
    set_all(&mut h, true);
    assert_eq!(value_box_texts(&h), ["Distance", "All"]);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.session.log);
    let e = extent(&h);
    assert_eq!((e.through_all, e.distance.as_str()), (true, "-10 mm"));
    // Below the sketch the box is 30 mm deep, above it 10 mm.
    let depth = (16000.0 - volume(&h, "Main")) / (std::f64::consts::PI * 4.0);
    assert!((depth - 30.0).abs() < 0.05, "{depth}");
}

/// With value stepping on, the box that names the extent is an empty box: Up and Down have no
/// number to step, so the kept Distance and All stay as they are.
#[test]
fn with_extent_all_the_value_box_has_no_number_to_step() {
    let mut h = open_cut(json!({"distance":"10 mm","direction":"negative"}));
    h.app.preferences.step_values = true;
    h.frames(2);
    h.step(&json!({"key":"ArrowUp"})).unwrap();
    h.frames(2);
    assert_eq!(distance_and_all(&h), ("11 mm".into(), false), "a Distance steps");
    set_all(&mut h, true);
    for key in ["ArrowUp", "ArrowDown"] {
        h.step(&json!({"key":key})).unwrap();
        h.frames(2);
        assert_eq!(distance_and_all(&h), ("11 mm".into(), true), "{key}");
        assert_eq!(value_box_texts(&h), ["Distance", "All"], "{key}");
    }
}

/// Distance 2's box in the view stays while its text does not evaluate, so a cleared box or a
/// `-` typed first can be finished there.
#[test]
fn two_sides_second_value_box_stays_while_its_text_is_incomplete() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY","name":"Disc"})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[10,10],"radius":2})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("solid.extrude", json!({"sketch":"Disc","distance":"5 mm","distance2":"3 mm","name":"Depth"})).unwrap();
    h.frames(4);
    open(&mut h);
    let area = || egui::Id::unique("sc_canvas_area2");
    assert!(h.ctx.memory(|m| m.area_rect(area())).is_some(), "Distance 2's box in the view");
    for text in ["-", ""] {
        let Some(crate::dialogs::Dialog { kind: crate::dialogs::Kind::Extrude { distance2, .. }, .. }) = &mut h.app.dialog else {
            panic!("extrude dialog")
        };
        *distance2 = text.into();
        h.frames(3);
        assert!(h.ctx.read_response(egui::Id::unique("sc_canvas_value2")).is_some(), "Distance 2's box went away at {text:?}");
    }
}

/// Without value boxes in the view (Preferences, #9479), nothing covers the model: the command
/// window's Distance takes the keyboard instead, its text selected, and the arrow still drags it.
#[test]
fn without_value_boxes_in_the_view_the_distance_is_typed_in_the_window() {
    let mut h = make(json!({"distance":"4 mm"}));
    h.app.preferences.value_boxes_in_view = false;
    open(&mut h);
    assert!(h.ctx.memory(|m| m.area_rect(egui::Id::unique("sc_canvas_area"))).is_none(), "a value box in the view");
    let focused = h.ctx.memory(|m| m.focused());
    assert!(focused.is_some() && focused != Some(egui::Id::unique("sc_canvas_value")), "{focused:?}");
    h.step(&json!({"text":"25 mm"})).unwrap();
    h.frames(2);
    assert!((dialog_distance(&h) - 25.0).abs() < 1e-9, "{}", dialog_distance(&h));
    h.step(&json!({"drag":[{"handle":"arrow"},on_arrow(&h, 30.0)]})).unwrap();
    assert!((dialog_distance(&h) - 30.0).abs() < 0.5, "{}", dialog_distance(&h));
    assert!(h.ctx.memory(|m| m.area_rect(egui::Id::unique("sc_canvas_area"))).is_none(), "a value box in the view");
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
        assert!(crate::open_meshing::session_world(&h.app.session).body("Extension").is_none(), "the feature's result is rolled off");
        assert_eq!(h.app.session.undo.len(), undo);
        h.app.session.visibility.hidden_bodies.push("Other".into());
        h.step(&json!({"widget":finish})).unwrap();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.session.log);
        assert!(!h.app.body_visible("Source"));
        assert!(!h.app.body_visible("Other"), "unrelated visibility edits are preserved");
        assert_eq!(h.app.session.visibility.hidden_sketches, visibility.hidden_sketches);
        assert_eq!(h.app.session.visibility.shown_sketches, visibility.shown_sketches);
        assert!(crate::open_meshing::session_world(&h.app.session).body("Extension").is_some());
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

/// Hold the free-drag key, the platform's primary modifier: Ctrl on Windows and Linux, Cmd on
/// macOS (#13274). Physical Ctrl alone is not the primary key on macOS.
fn hold_free_drag_key(h: &mut Harness) {
    use crate::tool_modifiers::{Held, Platform};
    h.app.synthetic_mods.command = true;
    for platform in [Platform::Mac, Platform::Other] {
        assert!(Held::new(h.app.synthetic_mods, platform).primary, "{platform:?} sees no primary key in {:?}", h.app.synthetic_mods);
    }
}

/// A block 12.345678 mm thick under XY (so no round step lands on its underside), a 10 mm
/// square sketched on its top and cut 5 mm into it, a second block beside it whose underside is
/// at -7.777 mm, and the cut open in its editor.
fn cut_into_block() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length":30,"width":30,"height":12.345678,"corner":[-10,-10,-12.345678],"body_name":"Block"})).unwrap();
    h.app
        .run("solid.box", json!({"length":10,"width":10,"height":7.777,"corner":[40,0,-7.777],"body_name":"Beside","operation":"new_body"}))
        .unwrap();
    h.app.run("sketch.create", json!({"plane":"XY","name":"Pocket"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let cut = json!({"sketch":"Pocket","direction":"negative","distance":"5 mm","operation":"cut","targets":["Block"],"name":"Depth"});
    h.app.run("solid.extrude", cut).unwrap();
    h.call("ui.view", json!({"view":"fit"}));
    h.frames(4);
    open(&mut h);
    h
}

/// Presses the arrow's tip and holds it at the world point `to`, frame by frame, without letting
/// go; returns the face the arrow stopped at then.
fn hold_arrow_at(h: &mut Harness, to: Value) -> Option<crate::extrude_face_snap::Stop> {
    let from = h.at(&json!({"handle":"arrow"})).unwrap();
    let from = egui::pos2(from[0] as f32, from[1] as f32);
    let to = h.at(&to).unwrap();
    let to = egui::pos2(to[0] as f32, to[1] as f32);
    h.app.synthetic.push(egui::Event::PointerMoved(from));
    h.frame();
    h.app.synthetic.push(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: h.app.synthetic_mods,
    });
    h.frame();
    for k in 1..=4 {
        h.app.synthetic.push(egui::Event::PointerMoved(from + (to - from) * (k as f32 / 4.0)));
        h.frame();
    }
    crate::extrude_face_snap::current()
}

fn let_go(h: &mut Harness) {
    let at = h.ctx.input(|i| i.pointer.hover_pos()).unwrap_or_default();
    h.app.synthetic.push(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: h.app.synthetic_mods,
    });
    h.frames(2);
}

/// Extrude Cut depth stops at the far face (#2398): dragged near the block's underside, the
/// cut's arrow takes its exact depth (not a round step) and outlines that face; OK cuts through.
#[test]
fn the_extrude_arrow_stops_at_a_parallel_face_and_outlines_it() {
    let mut h = cut_into_block();
    let stop = hold_arrow_at(&mut h, json!({"world":[5,5,-12.2]}));
    let stop = stop.expect("the arrow stops at the block's underside");
    assert_eq!((stop.body.as_str(), (stop.distance - 12.345678).abs() < 1e-9), ("Block", true), "{stop:?}");
    assert!((dialog_distance(&h) - 12.345678).abs() < 1e-9, "{}", dialog_distance(&h));
    let_go(&mut h);
    assert!(crate::extrude_face_snap::current().is_none(), "no outline once the arrow is let go");
    assert!((dialog_distance(&h) - 12.345678).abs() < 1e-9, "{}", dialog_distance(&h));
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.session.log);
    let expected = 30.0 * 30.0 * 12.345678 - 10.0 * 10.0 * 12.345678;
    assert!((volume(&h, "Block") - expected).abs() < 1e-6, "{} for {expected}", volume(&h, "Block"));
}

/// The free drag (Ctrl held) passes the face by, and a face beside the profile never catches the
/// arrow: the distance stays the pointer's or a round step.
#[test]
fn the_extrude_arrow_passes_faces_beside_the_profile_and_in_a_free_drag() {
    let mut h = cut_into_block();
    // The other block's underside at -7.777 is not under the square.
    assert_eq!(hold_arrow_at(&mut h, json!({"world":[5,5,-7.75]})), None);
    assert!((dialog_distance(&h) - 7.777).abs() > 1e-6, "{}", dialog_distance(&h));
    let_go(&mut h);
    hold_free_drag_key(&mut h);
    assert_eq!(hold_arrow_at(&mut h, json!({"world":[5,5,-12.2]})), None);
    assert!((dialog_distance(&h) - 12.345678).abs() > 1e-3, "{}", dialog_distance(&h));
    let_go(&mut h);
}

/// With Preferences › Design › "Extrude arrow stops at faces" off, the arrow keeps the old
/// behaviour (#2398): dragged near the block's underside it takes a round step, not the face's
/// distance, and nothing is outlined.
#[test]
fn without_face_stops_the_extrude_arrow_moves_in_round_steps() {
    let mut h = cut_into_block();
    h.app.preferences.extrude_arrow_face_stops = false;
    assert_eq!(hold_arrow_at(&mut h, json!({"world":[5,5,-12.2]})), None);
    let d = dialog_distance(&h);
    assert!((d - 12.345678).abs() > 1e-3, "{d}");
    let step = crate::canvas::snap_step(h.app.cam.half_height());
    assert!(((d / step).round() * step - d).abs() < 1e-9, "{d} is not a multiple of the {step} mm step");
    let_go(&mut h);
}

/// The extrude "Depth" (a 10 x 10 x 10 block) and a later box "Pad" beside it, both in view.
fn two_features() -> Harness {
    let mut h = make(json!({}));
    h.app.run("solid.box", json!({"corner":[30,0,0],"length":8,"width":8,"height":4,"name":"Pad"})).unwrap();
    h.app.animate_fit_all();
    h.frames(30);
    h
}

fn editing_feature(h: &Harness) -> Option<u64> {
    h.app.dialog.as_ref().and_then(|d| d.editing).map(|(id, _)| id)
}

fn feature_id(h: &Harness, name: &str) -> u64 {
    h.app.session.doc.find_feature(name).unwrap().id
}

// Double-clicking a body's face in the view opens the editor of the feature that made the face,
// as a double-click on that feature's row does (#10989).
#[test]
fn double_clicking_a_face_edits_the_feature_that_made_it() {
    let mut h = two_features();
    h.step(&json!({"click":{"world":[5,5,10]},"double":true})).unwrap();
    h.frames(8);
    assert!(h.app.dialog.is_some(), "the extrude's editor opens");
    assert_eq!(editing_feature(&h), Some(feature_id(&h, "Depth")));
    h.step(&json!({"widget":"Cancel"})).unwrap();
    h.settle();
    h.frames(30);
    assert!(h.app.dialog.is_none());
    // A face of the later feature opens that feature, not the extrude.
    h.step(&json!({"click":{"world":[34,4,4]},"double":true})).unwrap();
    h.frames(8);
    assert_eq!(editing_feature(&h), Some(feature_id(&h, "Pad")));
}

// It is the same editor the timeline row opens.
#[test]
fn double_clicking_a_face_opens_what_the_timeline_row_opens() {
    let mut h = two_features();
    open(&mut h);
    let from_row = h.app.dialog.as_ref().map(|d| (format!("{:?}", d.kind), d.editing.map(|e| e.0)));
    h.step(&json!({"widget":"Cancel"})).unwrap();
    h.settle();
    h.frames(30);
    h.step(&json!({"click":{"world":[5,5,10]},"double":true})).unwrap();
    h.frames(8);
    assert_eq!(h.app.dialog.as_ref().map(|d| (format!("{:?}", d.kind), d.editing.map(|e| e.0))), from_row);
}

// A single click only selects the face, and a double-click with Shift held is left to the selection.
#[test]
fn a_single_click_on_a_face_only_selects_it() {
    let mut h = two_features();
    h.step(&json!({"click":{"world":[5,5,10]}})).unwrap();
    h.frames(30);
    assert!(h.app.dialog.is_none() && h.app.pending_feature_edit.is_none());
    assert!(h.app.session.selection.iter().any(|s| matches!(s, solidcraft_engine::Sel::Face { .. })), "{:?}", h.app.session.selection);
    h.step(&json!({"click":{"world":[5,5,10]},"double":true,"shift":true})).unwrap();
    h.frames(8);
    assert!(h.app.dialog.is_none(), "a double-click with a modifier is left to the selection");
}

// Inside a dialog or tool the double-click does what it did: it does not swap the dialog.
#[test]
fn double_clicking_a_face_inside_an_open_dialog_leaves_it() {
    let mut h = two_features();
    h.app.start("solid.box");
    h.frames(4);
    let primitive = |h: &Harness| h.app.dialog.as_ref().is_some_and(|d| format!("{:?}", d.kind).starts_with("Primitive { cmd: \"solid.box\""));
    assert!(primitive(&h));
    h.step(&json!({"click":{"world":[5,5,10]},"double":true})).unwrap();
    h.frames(8);
    assert!(primitive(&h), "the Box dialog stays open");
    assert_eq!(editing_feature(&h), None);
    assert!(h.app.pending_feature_edit.is_none());
}

// In a component placed away from the origin the face is found in the component's own frame,
// so a later feature beside it is not opened instead: placed for good, or moved and not captured.
#[test]
fn double_clicking_a_face_of_a_moved_component_edits_its_feature() {
    for capture in [true, false] {
        let mut h = make(json!({}));
        let part = h.app.run("component.create", json!({"name":"Part","grounded":false})).unwrap();
        let occurrence = part["occurrence"].as_u64().unwrap();
        h.app.run("solid.box", json!({"corner":[0,0,0],"length":4,"width":4,"height":4,"name":"Inner"})).unwrap();
        h.app.run("solid.box", json!({"corner":[20,0,0],"length":4,"width":4,"height":4,"name":"Later"})).unwrap();
        h.app.run("occurrence.move", json!({"occurrence":occurrence,"translate":[40,0,0],"capture":capture})).unwrap();
        h.app.animate_fit_all();
        h.frames(30);
        h.step(&json!({"click":{"world":[42,2,4]},"double":true})).unwrap();
        h.frames(8);
        assert_eq!(editing_feature(&h), Some(feature_id(&h, "Inner")), "capture: {capture}");
    }
}

#[test]
fn surface_extrude_dialog_has_the_drag_arrow_on_every_side() {
    for (direction, tip_z) in [("positive", 10.0), ("negative", -10.0), ("symmetric", 5.0)] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY","name":"Base"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("surface.extrude", json!({"sketch":"Base","distance":"10 mm","direction":direction,"name":"Wall","body_name":"Sheet"})).unwrap();
        h.frames(4);
        h.step(&json!({"call":"ui.editFeature","params":{"feature":"Wall"}})).unwrap();
        h.frames(8);
        let tip = h.at(&json!({"world":[5,5,tip_z]})).unwrap();
        let handle = h.at(&json!({"handle":"arrow"})).unwrap_or_else(|_| panic!("{direction}: a surface extrude shows the drag arrow"));
        assert!((handle[0] - tip[0]).hypot(handle[1] - tip[1]) < 1.0, "{direction}: {handle:?} expected {tip:?}");
        h.step(&json!({"drag":[{"handle":"arrow"},{"world":[5,5,if direction == "negative" { -30.0 } else { 30.0 }]}]})).unwrap();
        let Some(crate::dialogs::Dialog { kind: crate::dialogs::Kind::Surface(crate::dialogs_surface::Surf::Extrude { distance, .. }), .. }) =
            &h.app.dialog
        else {
            panic!("surface extrude dialog")
        };
        let total = h.app.session.doc.eval(distance, solidcraft_engine::doc::expr::Kind::Length).unwrap();
        let expected = if direction == "symmetric" { 60.0 } else { 30.0 };
        assert!((total - expected).abs() < 1.0, "{direction}: dragging the arrow sets the distance, got {distance}");
    }
}

/// Double-clicking the distance arrow in the view reverses the direction, as Reverse Direction
/// does, and keeps the distance (#17402); Symmetric has no direction to reverse.
#[test]
fn double_click_on_the_arrow_reverses_the_direction() {
    use solidcraft_engine::doc::Direction;
    let mut h = make(json!({}));
    open(&mut h);
    h.step(&json!({"click":{"handle":"arrow"},"double":true})).unwrap();
    h.frames(2);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.session.log);
    let e = extent(&h);
    assert_eq!(e.direction, Direction::Negative);
    let distance = h.app.session.doc.eval(e.distance.as_str(), solidcraft_engine::doc::expr::Kind::Length).unwrap();
    assert!((distance - 10.0).abs() < 1e-9, "the distance stays: {}", e.distance);
    open(&mut h);
    h.step(&json!({"click":{"handle":"arrow"},"double":true})).unwrap();
    h.frames(2);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert_eq!(extent(&h).direction, Direction::Positive, "a second double-click turns it back");

    let mut h = make(json!({"direction":"symmetric"}));
    open(&mut h);
    h.step(&json!({"click":{"handle":"arrow"},"double":true})).unwrap();
    h.frames(2);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert_eq!(extent(&h).direction, Direction::Symmetric, "Symmetric stays symmetric");
}

/// Two boxes and a sketch on the first one's top face whose rectangle reaches over the second
/// one's top too, as when one extrusion should bridge parts (#17657).
fn two_boxes_and_a_bridge() -> (Harness, u64) {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Left"})).unwrap();
    h.app.run("solid.box", json!({"corner":[20,0,0],"length":10,"width":10,"height":10,"body_name":"Right"})).unwrap();
    let sketch =
        h.app.run("sketch.create", json!({"plane":{"face":[5,5,10]},"name":"Bridge","project_edges":false})).unwrap()["sketch"].as_u64().unwrap();
    let plane = crate::open_meshing::session_world(&h.app.session).sketch(sketch).unwrap().plane;
    let at = |x: f64, y: f64| {
        use solidcraft_engine::geom::prelude::*;
        let q = plane.to_local(solidcore_record::Vec3::new(x, y, 10.0));
        json!([q.x, q.y])
    };
    h.app.run("sketch.rectangle.two_point", json!({"p0": at(2.0, 2.0), "p1": at(28.0, 8.0)})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.frames(4);
    (h, sketch)
}

fn target_bodies(h: &Harness) -> Option<Vec<String>> {
    use crate::selection::SelectionExt;
    let d = h.app.dialog.as_ref()?;
    d.inputs.iter().find(|i| i.label == "Target Bodies").map(|i| i.items.body_names())
}

fn overlapping_boxes_and_a_cut_profile() -> (Harness, u64) {
    let mut h = Harness::new();
    h.app.home.open = false;
    for (name, x) in [("Left", 0), ("Right", 30)] {
        h.app.run("solid.box", json!({"length":40,"width":20,"height":10,"corner":[x,0,0],"operation":"new","body_name":name})).unwrap();
    }
    let sketch = h.app.run("sketch.create", json!({"plane":"XY","offset":"10 mm","name":"CutProfile"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[32,5],"p1":[38,15]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.frames(4);
    (h, sketch)
}

/// A To Object cut through overlapping bodies can leave the unpicked body intact (#17323).
#[test]
fn extrude_cut_only_cuts_the_target_bodies_picked_in_the_dialog() {
    for (through_all, picked, hidden_left) in [(false, false, false), (false, true, false), (true, true, false), (false, false, true)] {
        let (mut h, sketch) = overlapping_boxes_and_a_cut_profile();
        if hidden_left {
            h.app.session.visibility.hidden_bodies.push("Left".into());
        }
        h.app.session.selection = vec![solidcraft_engine::Sel::Profile { sketch, index: 0 }];
        h.app.start("solid.extrude");
        h.frames(4);
        {
            let d = h.app.dialog.as_mut().unwrap();
            let crate::dialogs::Kind::Extrude { operation, auto_op, direction, all, .. } = &mut d.kind else { panic!("extrude") };
            *operation = 2;
            *auto_op = false;
            *direction = 1;
            *all = through_all;
            if !through_all {
                d.pick(&h.app.session, solidcraft_engine::Sel::Vertex { body: "Right".into(), point: solidcore_record::Vec3::new(35.0, 10.0, 0.0) });
            }
        }
        h.frames(2);
        assert_eq!(target_bodies(&h), Some(vec![]), "Cut shows Target Bodies");
        if picked {
            let d = h.app.dialog.as_mut().unwrap();
            d.active = d.inputs.iter().position(|i| i.label == "Target Bodies").unwrap();
            d.pick(&h.app.session, solidcraft_engine::Sel::Body { name: "Right".into() });
        }
        h.frames(2);
        let undo = h.app.session.undo.len();
        h.step(&json!({"widget":"OK"})).unwrap();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.session.log);
        let left_volume = if picked || hidden_left { 8000.0 } else { 7400.0 };
        assert!((volume(&h, "Left") - left_volume).abs() < 1e-6);
        assert!((volume(&h, "Right") - 7400.0).abs() < 1e-6);
        let FeatureKind::Extrude { targets, extent, .. } = &h.app.session.doc.features.last().unwrap().kind else { panic!("extrude") };
        assert_eq!(targets.iter().map(|n| n.0.as_str()).collect::<Vec<_>>(), if picked || hidden_left { vec!["Right"] } else { vec![] });
        assert_eq!(extent.to.as_ref().map(|p| p.0), (!through_all).then_some(solidcore_record::Vec3::new(35.0, 10.0, 0.0)));
        assert_eq!(extent.through_all, through_all);
        assert_eq!(h.app.session.undo.len(), undo + 1);
        h.app.run("edit.undo", json!({})).unwrap();
        assert!((volume(&h, "Left") - 8000.0).abs() < 1e-6);
        assert!((volume(&h, "Right") - 8000.0).abs() < 1e-6);
        h.app.run("edit.redo", json!({})).unwrap();
        assert!((volume(&h, "Left") - left_volume).abs() < 1e-6);
        assert!((volume(&h, "Right") - 7400.0).abs() < 1e-6);
    }
}

#[test]
fn editing_extrude_cut_restores_targets_and_clearing_them_keeps_the_original_scope() {
    for participants in [vec![], vec!["Left"]] {
        let (mut h, _) = overlapping_boxes_and_a_cut_profile();
        h.app
            .run(
                "solid.extrude",
                json!({"sketch":"CutProfile","operation":"cut","direction":"negative","to":[35,10,0],
            "targets":["Right"],"participants":participants,"name":"Depth"}),
            )
            .unwrap();
        let undo = h.app.session.undo.len();
        open(&mut h);
        assert_eq!(target_bodies(&h), Some(vec!["Right".into()]));
        h.step(&json!({"widget":"OK"})).unwrap();
        assert!(h.app.dialog.is_none());
        assert_eq!(h.app.session.undo.len(), undo + 1);
        assert!((volume(&h, "Left") - 8000.0).abs() < 1e-6);
        assert!((volume(&h, "Right") - 7400.0).abs() < 1e-6);
        open(&mut h);
        h.app.dialog.as_mut().unwrap().inputs.iter_mut().find(|i| i.label == "Target Bodies").unwrap().items.clear();
        h.frames(2);
        h.step(&json!({"widget":"OK"})).unwrap();
        assert!(h.app.dialog.is_none());
        let feature = h.app.session.doc.find_feature("Depth").unwrap();
        let FeatureKind::Extrude { targets, .. } = &feature.kind else { panic!("extrude") };
        assert!(targets.is_empty(), "clearing the input must remove the stored targets");
        assert_eq!(feature.participants, participants);
        assert!((volume(&h, "Left") - 7400.0).abs() < 1e-6);
        assert!((volume(&h, "Right") - if participants.is_empty() { 7400.0 } else { 8000.0 }).abs() < 1e-6);
        h.app.run("edit.undo", json!({})).unwrap();
        assert!((volume(&h, "Left") - 8000.0).abs() < 1e-6);
        assert!((volume(&h, "Right") - 7400.0).abs() < 1e-6);
    }
}

#[test]
fn extrude_target_picks_survive_join_cut_switches_but_not_new_body() {
    for initial_operation in ["cut", "intersect"] {
        let (mut h, _) = overlapping_boxes_and_a_cut_profile();
        h.app
            .run(
                "solid.extrude",
                json!({"sketch":"CutProfile","operation":initial_operation,"direction":"negative","distance":"10 mm",
        "targets":["Right"],"name":"Depth"}),
            )
            .unwrap();
        open(&mut h);
        for (op, tip) in [(2, "ui.extrude_cut_targets_tip"), (1, "ui.extrude_join_targets_tip"), (2, "ui.extrude_cut_targets_tip")] {
            let d = h.app.dialog.as_mut().unwrap();
            let crate::dialogs::Kind::Extrude { operation, .. } = &mut d.kind else { panic!("extrude") };
            *operation = op;
            h.frames(2);
            assert_eq!(target_bodies(&h), Some(vec!["Right".into()]));
            let input = h.app.dialog.as_ref().unwrap().inputs.iter().find(|i| i.label == "Target Bodies").unwrap();
            assert_eq!(input.tip, Some(tip));
        }
        let d = h.app.dialog.as_mut().unwrap();
        d.active = d.inputs.iter().position(|i| i.label == "Target Bodies").unwrap();
        let crate::dialogs::Kind::Extrude { operation, .. } = &mut d.kind else { panic!("extrude") };
        *operation = 0;
        h.frames(2);
        assert_eq!(target_bodies(&h), None);
        assert_eq!(h.app.dialog.as_ref().unwrap().active, 0);
        let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        assert!(commands[0].1["params"].get("targets").is_none(), "the discarded targets must not be applied while their input is hidden");
        let crate::dialogs::Kind::Extrude { operation, .. } = &mut h.app.dialog.as_mut().unwrap().kind else { panic!("extrude") };
        *operation = 2;
        h.frames(2);
        assert_eq!(target_bodies(&h), Some(vec![]));
        h.step(&json!({"widget":"OK"})).unwrap();
        assert!(h.app.dialog.is_none());
        assert!((volume(&h, "Left") - 7400.0).abs() < 1e-6, "the discarded target must not return from the stored feature");
        assert!((volume(&h, "Right") - 7400.0).abs() < 1e-6);
    }
}

/// Extrude's Join shows Target Bodies, and joins the bodies picked there; with none picked it
/// joins the body its sketch is on, as before. Editing the extrude shows the stored bodies, and
/// another operation hides the input (#17657).
#[test]
fn extrude_join_joins_the_target_bodies_picked_in_the_dialog() {
    for picked in [vec![], vec!["Left", "Right"]] {
        let (mut h, sketch) = two_boxes_and_a_bridge();
        let state = crate::open_meshing::session_world(&h.app.session);
        assert_eq!(state.sketch(sketch).unwrap().profiles.len(), 1, "one rectangle, no projected outline");
        let index = 0;
        h.app.session.selection = vec![solidcraft_engine::Sel::Profile { sketch, index }];
        h.app.start("solid.extrude");
        h.frames(4);
        {
            let d = h.app.dialog.as_mut().unwrap();
            let crate::dialogs::Kind::Extrude { operation, auto_op, distance, .. } = &mut d.kind else { panic!("extrude") };
            *operation = 1;
            *auto_op = false;
            *distance = "3 mm".into();
        }
        h.frames(2);
        assert_eq!(target_bodies(&h), Some(vec![]), "Join shows an empty Target Bodies");
        assert_eq!(h.app.dialog.as_ref().unwrap().active, 0, "the profiles stay the active input");
        if !picked.is_empty() {
            let d = h.app.dialog.as_mut().unwrap();
            d.active = d.inputs.iter().position(|i| i.label == "Target Bodies").unwrap();
            for name in &picked {
                d.pick(&h.app.session, solidcraft_engine::Sel::Body { name: (*name).into() });
            }
            h.frames(2);
            assert_eq!(target_bodies(&h), Some(picked.iter().map(|n| n.to_string()).collect()));
        }
        h.step(&json!({"widget":"OK"})).unwrap();
        assert!(h.app.dialog.is_none());
        let FeatureKind::Extrude { targets, .. } = &h.app.session.doc.features.last().unwrap().kind else { panic!("extrude") };
        let targets: Vec<String> = targets.iter().map(|t| t.0.clone()).collect();
        let bodies = crate::open_meshing::session_world(&h.app.session).bodies.len();
        if picked.is_empty() {
            assert_eq!(targets, ["Left"], "none picked: the body the sketch is on");
            assert_eq!(bodies, 2, "Right stays apart");
        } else {
            assert_eq!(targets, picked, "the picked bodies");
            assert_eq!(bodies, 1, "the bridge joins both boxes");
        }
        // The editor shows the stored bodies; New Body hides them.
        let name = h.app.session.doc.features.last().unwrap().name.clone();
        h.step(&json!({"call":"ui.editFeature","params":{"feature":name}})).unwrap();
        h.frames(4);
        assert_eq!(target_bodies(&h), Some(targets.clone()));
        if let Some(crate::dialogs::Kind::Extrude { operation, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            *operation = 0;
        }
        h.frames(2);
        assert_eq!(target_bodies(&h), None, "New Body has no Target Bodies");
    }
}

/// A body picked into Target Bodies stops Extrude choosing its operation at once, in the pick:
/// on the web the check runs before the form is drawn again, and a New Body it chose would take
/// the row and the pick away (#17657).
#[test]
fn a_target_body_pick_stops_the_automatic_operation_at_once() {
    let (mut h, sketch) = two_boxes_and_a_bridge();
    h.app.session.selection = vec![solidcraft_engine::Sel::Profile { sketch, index: 0 }];
    h.app.start("solid.extrude");
    h.frames(4);
    assert!(
        matches!(h.app.dialog.as_ref().unwrap().kind, crate::dialogs::Kind::Extrude { operation: 1, auto_op: true, .. }),
        "a profile on a body's face: Extrude chooses Join"
    );
    let d = h.app.dialog.as_mut().unwrap();
    d.active = d.inputs.iter().position(|i| i.label == "Target Bodies").expect("Join shows Target Bodies");
    d.pick(&h.app.session, solidcraft_engine::Sel::Body { name: "Right".into() });
    assert!(matches!(d.kind, crate::dialogs::Kind::Extrude { auto_op: false, operation: 1, .. }), "{:?}", d.kind);
    h.frames(4);
    assert_eq!(target_bodies(&h), Some(vec!["Right".to_string()]), "the pick stays");
}

/// Switching To Object off keeps From Object the active input when Target Bodies follows it
/// (#17657): the active input is the same one, not the input that moved into its place.
#[test]
fn removing_an_earlier_input_keeps_the_active_one() {
    use crate::selection::{BODIES, FACES, SelInput, VERTICES};
    let mut inputs = vec![
        SelInput::new("Profiles", FACES, true),
        SelInput::new("To Object", FACES | VERTICES, false),
        SelInput::new("From Object", FACES | VERTICES, false),
        SelInput::new("Target Bodies", BODIES, true),
    ];
    let mut active = 2;
    crate::dialogs::set_object_accepting(&mut inputs, &mut active, "To Object", FACES | VERTICES, false);
    assert_eq!(inputs[active].label, "From Object");
    // The active input itself going leaves the input after it active, as before.
    crate::dialogs::set_object_accepting(&mut inputs, &mut active, "From Object", FACES | VERTICES, false);
    assert_eq!(inputs[active].label, "Target Bodies");
}

#[test]
fn editing_symmetric_extrude_toggles_symmetric_offset() {
    let mut h = make(json!({"direction": "symmetric", "start_offset": "4 mm"}));
    assert!(!extent(&h).symmetric_offset);
    open(&mut h);
    h.frames(2);
    assert!(crate::scenario::handle_at("dialog:Symmetric Offset").is_some());
    h.step(&json!({"click":{"handle":"dialog:Symmetric Offset"}})).unwrap();
    h.frames(2);
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none());
    assert!(extent(&h).symmetric_offset);
}

/// Clearing a moved command window must leave both real Extrude arrow grips accessible,
/// including the primary arrow that is drawn after Distance 2's floating box.
#[test]
fn floating_extrude_fields_clear_both_arrow_grips_beside_the_window() {
    for secondary in [false, true] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1600,"height":1200}));
        h.call("ui.prefs", json!({"language":"en","look_at_anim":false}));
        let mut h = make_in(h, json!({"distance":"2 mm","distance2":"1 mm"}));
        h.call("ui.view", json!({"view":"iso"}));
        h.call("ui.view", json!({"view":"fit"}));
        open(&mut h);
        h.app.cam = h.app.cam.looking_from(solidcore_record::Vec3::new(1.0, -1.0, 0.8));
        h.app.cam.target += solidcore_record::Vec3::new(3.0, -2.0, 0.0);
        h.frames(4);
        let name = if secondary { "arrow2" } else { "arrow" };
        let tip = crate::scenario::handle_at(name).expect("the drawn Extrude arrow");
        let window = h.app.viewport.dialog_rect.unwrap();
        let title = crate::scenario::handle_at("dialog:title").unwrap();
        let delta = egui::vec2(tip.x + 30.0 - window.left(), tip.y - 80.0 - window.top());
        h.call("ui.drag", json!({"x0":title.x,"y0":title.y,"x1":title.x+delta.x,"y1":title.y+delta.y,"steps":8}));
        h.frames(24);
        let tip = crate::scenario::handle_at(name).unwrap();
        let window = h.app.viewport.dialog_rect.unwrap();
        assert!((window.left() - tip.x - 30.0).abs() < 3.0 && window.y_range().contains(tip.y), "window beside {name}: {window:?}, {tip:?}");
        let fields: Vec<_> = ["sc_canvas_area", "sc_canvas_area2"]
            .into_iter()
            .map(|id| h.ctx.memory(|m| m.area_rect(egui::Id::unique(id))).expect("each actual floating box"))
            .collect();
        let grabs: Vec<_> = ["arrow", "arrow2"]
            .into_iter()
            .map(|id| egui::Rect::from_center_size(crate::scenario::handle_at(id).unwrap(), egui::vec2(20.0, 20.0)))
            .collect();
        let design = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        let (base, dir) = crate::canvas::anchor(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        let to = base + dir.unwrap() * if secondary { -6.0 } else { 6.0 };
        h.step(&json!({"drag":[{"handle":name},{"world":[to.x,to.y,to.z]}],"ctrl":true})).unwrap();
        h.frames(4);
        for field in fields {
            for grab in &grabs {
                assert!(!field.intersects(*grab), "{name}: floating field {field:?} covers real arrow grip {grab:?}");
            }
        }
        let Some(crate::dialogs::Kind::Extrude { distance, distance2, .. }) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("Extrude") };
        let eval = |text: &str| h.app.session.doc.eval(text, solidcraft_engine::doc::expr::Kind::Length).unwrap();
        let (first, second) = (eval(distance), eval(distance2));
        let (changed, kept, original) = if secondary { (second, first, 2.0) } else { (first, second, 1.0) };
        assert!((changed - 6.0).abs() < 0.3, "{name}: Distance={first}, Distance2={second}");
        assert!((kept - original).abs() < 1e-9, "the other distance stays unchanged");
        assert_eq!(h.app.session.doc.to_json(), design, "a preview drag does not commit the feature");
        assert_eq!(h.app.session.undo.len(), undo);
    }
}

/// Deferring Distance 2's box until the arrows are known keeps its own text and keyboard target.
#[test]
fn floating_extrude_second_field_types_into_its_own_distance() {
    let mut h = make(json!({"distance":"5 mm","distance2":"3 mm"}));
    open(&mut h);
    let field = h.ctx.read_response(egui::Id::unique("sc_canvas_value2")).expect("Distance 2's actual input").rect;
    h.step(&json!({"click":[field.center().x,field.center().y]})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"7 mm"})).unwrap();
    h.frames(3);
    let Some(crate::dialogs::Kind::Extrude { distance, distance2, .. }) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("Extrude") };
    assert_eq!((distance.as_str(), distance2.as_str()), ("5 mm", "7 mm"));
}
