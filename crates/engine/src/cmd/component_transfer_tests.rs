use super::*;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap()
}

#[test]
fn recorded_assignment_preserves_sample_joined_material_and_timeline() {
    let mut s = Session::default();
    s.run_script(&crate::sample::script()).unwrap();
    let before = s.model.state();
    assert_eq!(before.bodies.len(), 1);
    let volume = solidcraft_kernel::measure(&before.body("Plate").unwrap().body).unwrap().volume;
    let created = run(&mut s, "component.from_bodies", json!({"bodies":["Plate"]}));
    let to = created["components"][0]["component"].as_u64().unwrap();
    let feature = created["components"][0]["feature"].as_u64().unwrap();
    let after = s.model.state();
    assert_eq!(after.bodies.len(), 1);
    let body = after.body("Plate").unwrap();
    assert_eq!(body.component, to);
    assert!(body.transferred);
    assert!((solidcraft_kernel::measure(&body.body).unwrap().volume - volume).abs() < 1e-6);
    assert!(body.mesh().contains(Vec3::new(40.0, 35.0, 12.0)));
    let index = s.doc.feature_index(feature).unwrap();
    s.doc_mut().marker = Some(index);
    s.refresh();
    assert_eq!(s.world_state().body("Plate").unwrap().component, 0);
    s.doc_mut().marker = None;
    s.refresh();
    assert_eq!(s.world_state().body("Plate").unwrap().component, to);
}

#[test]
fn transformed_pending_destination_repeated_transfer_preserves_world_pose() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Block"}));
    let created = run(&mut s, "component.create", json!({"name":"Destination","grounded":false}));
    let to = created["component"].as_u64().unwrap();
    let occurrence = created["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":occurrence,"translate":[30,20,0],"angle":90,"axis":[0,0,1],"capture":false}));
    run(&mut s, "component.move_bodies", json!({"component":to,"bodies":["Block"]}));
    let state = s.world_state();
    let bounds = state.body("Block").unwrap().mesh().bounds();
    assert!(bounds.min.dist(Vec3::ZERO) < 1e-6);
    assert!(bounds.max.dist(Vec3::new(10.0, 10.0, 10.0)) < 1e-6);
    assert!(!s.pending_moves.contains_key(&occurrence));
    let mut destination_reopened = Session { doc: solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().into(), ..Session::default() };
    destination_reopened.refresh();
    assert!(destination_reopened.world_state().body("Block").unwrap().mesh().bounds().min.dist(Vec3::ZERO) < 1e-6);
    run(&mut s, "component.move_bodies", json!({"component":0,"bodies":["Block"]}));
    assert!(s.world_state().body("Block").unwrap().mesh().bounds().min.dist(Vec3::ZERO) < 1e-6);
    let mut reopened = Session { doc: solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().into(), ..Session::default() };
    reopened.refresh();
    assert!(reopened.world_state().body("Block").unwrap().mesh().bounds().min.dist(Vec3::ZERO) < 1e-6);
}

#[test]
fn shared_destination_refuses_without_mutating_document_or_session() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Block"}));
    let c = run(&mut s, "component.create", json!({"name":"Shared"}))["component"].as_u64().unwrap();
    run(&mut s, "occurrence.copy", json!({"component":c,"translate":[20,0,0]}));
    let document = s.doc.to_json();
    let active = s.active_component;
    assert!(s.execute("component.move_bodies", &json!({"component":c,"bodies":["Block"]})).is_err());
    assert_eq!(s.doc.to_json(), document);
    assert_eq!(s.active_component, active);
}

#[test]
fn historical_join_cut_and_new_destination_modeling_keep_material() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Block"}));
    run(&mut s, "solid.box", json!({"corner":[8,0,0],"length":10,"width":10,"height":10,"operation":"join"}));
    run(&mut s, "solid.box", json!({"corner":[2,2,0],"length":2,"width":2,"height":10,"operation":"cut"}));
    let original = s.model.state();
    assert_eq!(original.bodies.len(), 1);
    let v = solidcraft_kernel::measure(&original.body("Block").unwrap().body).unwrap().volume;
    assert!((v - 1760.0).abs() < 1e-6);
    run(&mut s, "material.assign", json!({"bodies":["Block"],"material":"Brass"}));
    run(&mut s, "appearance.assign", json!({"bodies":["Block"],"color":"#bc8537"}));
    let created = run(&mut s, "component.from_bodies", json!({"bodies":["Block"]}));
    let c = created["components"][0]["component"].as_u64().unwrap();
    assert_eq!(s.doc.materials["Block"], "Brass");
    assert_eq!(s.doc.appearances.bodies["Block"].color, [188, 133, 55]);
    run(&mut s, "component.activate", json!({"component":c}));
    run(&mut s, "solid.box", json!({"corner":[18,0,0],"length":2,"width":10,"height":10,"operation":"join"}));
    assert_eq!(s.model.state().bodies.len(), 1);
    assert_eq!(s.model.state().body("Block").unwrap().component, c);
    let final_volume = solidcraft_kernel::measure(&s.model.state().body("Block").unwrap().body).unwrap().volume;
    assert!((final_volume - 1960.0).abs() < 1e-6);
    run(&mut s, "component.move_bodies", json!({"component":0,"bodies":["Block"]}));
    assert_eq!(s.model.state().body("Block").unwrap().component, 0);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.model.state().body("Block").unwrap().component, c);
    let mut reopened = Session { doc: solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().into(), ..Session::default() };
    reopened.refresh();
    assert!((solidcraft_kernel::measure(&reopened.model.state().body("Block").unwrap().body).unwrap().volume - final_volume).abs() < 1e-6);
}

#[test]
fn ownership_only_boundary_invalidates_old_boolean_cache() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Block"}));
    run(&mut s, "solid.box", json!({"corner":[8,0,0],"length":10,"width":10,"height":10,"operation":"join"}));
    assert_eq!(s.model.state().bodies.len(), 1);
    let c = s.doc_mut().add_component(Some("Destination"), 0).unwrap();
    let id = s
        .doc_mut()
        .add_feature(FeatureKind::BodyTransfer { bodies: vec!["Block".into()], from: 0, to: c, transform: solidcraft_doc::IDENTITY }, None)
        .unwrap();
    let index = s.doc.feature_index(id).unwrap();
    let transfer = s.doc_mut().features.remove(index);
    s.doc_mut().features.insert(1, transfer);
    s.refresh();
    assert_eq!(s.model.state().bodies.len(), 2);
    assert_eq!(s.model.state().body("Block").unwrap().component, c);
    let mut reopened = Session { doc: solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().into(), ..Session::default() };
    reopened.refresh();
    assert_eq!(reopened.model.state().bodies.len(), 2);
    assert_eq!(reopened.model.state().body("Block").unwrap().component, c);
}

#[test]
fn failed_transfer_cannot_change_evaluated_world_owner() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Block"}));
    let c = s.doc_mut().add_component(Some("Destination"), 0).unwrap();
    let id = s
        .doc_mut()
        .add_feature(FeatureKind::BodyTransfer { bodies: vec!["Block".into()], from: c, to: c, transform: solidcraft_doc::IDENTITY }, None)
        .unwrap();
    s.refresh();
    assert!(s.model.result(id).unwrap().error.is_some());
    assert_eq!(s.world_state().body("Block").unwrap().component, 0);
}

#[test]
fn transferred_thread_follows_body_frame_without_changing_world_axis() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":4,"height":20,"body_name":"Shaft"}));
    run(&mut s, "solid.thread", json!({"face":[4,0,10],"designation":"M8"}));
    let before = s.world_state().threads[0].clone();
    let c = run(&mut s, "component.create", json!({"name":"Threaded","grounded":false}));
    let to = c["component"].as_u64().unwrap();
    let occurrence = c["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":occurrence,"translate":[30,20,10],"axis":[1,0,0],"angle":90,"capture":true}));
    run(&mut s, "component.move_bodies", json!({"component":to,"bodies":["Shaft"]}));
    let after = s.world_state().threads[0].clone();
    assert!(after.axis_point.dist(before.axis_point) < 1e-6);
    assert!(after.axis.dist(*before.axis) < 1e-6);
    let mut reopened = Session { doc: solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().into(), ..Session::default() };
    reopened.refresh();
    assert!(reopened.world_state().threads[0].axis.dist(*before.axis) < 1e-6);
}

#[test]
fn inherited_component_appearance_and_legacy_assignment_survive_transfer() {
    let mut s = Session::default();
    let source = run(&mut s, "component.create", json!({"name":"Source"}))["component"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Block"}));
    s.doc_mut()
        .appearances
        .components
        .insert(source, solidcraft_doc::appearance::Look { name: "Green".into(), color: [0, 150, 0], opacity: 0.7, pbr: None });
    s.refresh();
    let look = s.doc.model_body_look(s.model.state().body("Block").unwrap()).unwrap();
    run(&mut s, "component.move_bodies", json!({"component":0,"bodies":["Block"]}));
    assert_eq!(s.doc.model_body_look(s.model.state().body("Block").unwrap()), Some(look));
    // An unrecorded legacy assignment retains its established replay policy.
    let mut legacy = Session::default();
    run(&mut legacy, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Old"}));
    let c = legacy.doc_mut().add_component(Some("Legacy"), 0).unwrap();
    legacy.doc_mut().body_components.insert("Old".into(), c);
    legacy.doc_mut().body_offsets.insert("Old".into(), solidcraft_doc::rigid(Vec3::new(7.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0));
    legacy.refresh();
    assert!(!legacy.model.state().body("Old").unwrap().transferred);
    run(&mut legacy, "component.move_bodies", json!({"component":0,"bodies":["Old"]}));
    assert!((legacy.world_state().body("Old").unwrap().mesh().bounds().min.x - 7.0).abs() < 1e-6);
    let mut reopened = Session { doc: solidcraft_doc::Document::from_json(&legacy.doc.to_json()).unwrap().into(), ..Session::default() };
    reopened.refresh();
    assert!((reopened.world_state().body("Old").unwrap().mesh().bounds().min.x - 7.0).abs() < 1e-6);
}

#[test]
fn deleting_external_transfer_source_refuses_without_dangling_archive() {
    let mut s = Session::default();
    let a = run(&mut s, "component.create", json!({"name":"Source"}))["component"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length":2,"width":2,"height":2,"body_name":"Block"}));
    run(&mut s, "component.activate", json!({"component":0}));
    let b = run(&mut s, "component.create", json!({"name":"Destination"}))["component"].as_u64().unwrap();
    run(&mut s, "component.move_bodies", json!({"component":b,"bodies":["Block"]}));
    let document = s.doc.to_json();
    assert!(s.execute("component.delete", &json!({"component":a})).is_err());
    assert_eq!(s.doc.to_json(), document);
    assert!(solidcraft_doc::Document::from_json(&document).is_ok());
}

#[test]
fn earlier_selected_cylinder_emboss_keeps_source_frame_after_transfer_and_edit() {
    let mut s = Session::default();
    let source = run(&mut s, "component.create", json!({"name":"Source","grounded":false}));
    let occurrence = source["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.cylinder", json!({"radius":10,"height":20,"body_name":"Sleeve"}));
    run(&mut s, "occurrence.move", json!({"occurrence":occurrence,"translate":[30,0,0],"capture":true}));
    run(&mut s, "sketch.create", json!({"name":"Badge","plane":{"origin":[30,10,0],"x_dir":[-1,0,0],"y_dir":[0,0,1]}}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-2,5],"p1":[2,10]}));
    run(&mut s, "sketch.finish", json!({}));
    let emboss = run(&mut s, "solid.emboss", json!({"sketch":"Badge","face":[30,10,7.5],"depth":1}))["feature"].as_u64().unwrap();
    let before = solidcraft_kernel::measure(&s.world_state().body("Sleeve").unwrap().body).unwrap().volume;
    run(&mut s, "component.activate", json!({"component":0}));
    let destination = run(&mut s, "component.create", json!({"name":"Destination"}));
    let to = destination["component"].as_u64().unwrap();
    let destination_occurrence = destination["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":destination_occurrence,"translate":[0,40,0],"axis":[1,0,0],"angle":90,"capture":true}));
    run(&mut s, "component.move_bodies", json!({"component":to,"bodies":["Sleeve"]}));
    assert!(s.doc.features.iter().all(|feature| s.model.result(feature.id).is_some_and(|result| result.error.is_none())));
    let mut reopened = Session { doc: solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().into(), ..Session::default() };
    reopened.refresh();
    assert!(reopened.doc.features.iter().all(|feature| reopened.model.result(feature.id).is_some_and(|result| result.error.is_none())));
    assert!((solidcraft_kernel::measure(&reopened.world_state().body("Sleeve").unwrap().body).unwrap().volume - before).abs() < 1e-5);
    let depth = reopened.doc.feature(emboss).unwrap().param_names[0].clone();
    run(&mut reopened, "parameters.change", json!({"name":depth,"expression":"2 mm"}));
    assert!(solidcraft_kernel::measure(&reopened.world_state().body("Sleeve").unwrap().body).unwrap().volume > before);
}

#[test]
fn transfer_undo_redo_restores_original_pending_source_world_pose() {
    let mut s = Session::default();
    let created = run(&mut s, "component.create", json!({"name":"Source","grounded":false}));
    let source = created["component"].as_u64().unwrap();
    let occurrence = created["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length":2,"width":2,"height":2,"body_name":"Block"}));
    run(&mut s, "occurrence.move", json!({"occurrence":occurrence,"translate":[10,0,0],"capture":false}));
    let pending = s.pending_moves.clone();
    let before = s.world_state().body("Block").unwrap().mesh().bounds();
    run(&mut s, "component.move_bodies", json!({"component":0,"bodies":["Block"]}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.pending_moves, pending);
    assert_eq!(s.world_state().body("Block").unwrap().component, source);
    assert!(s.world_state().body("Block").unwrap().mesh().bounds().min.dist(before.min) < 1e-6);
    run(&mut s, "edit.redo", json!({}));
    assert!(s.pending_moves.is_empty());
    assert_eq!(s.world_state().body("Block").unwrap().component, 0);
    assert!(s.world_state().body("Block").unwrap().mesh().bounds().min.dist(before.min) < 1e-6);
}

#[test]
fn transfer_checkpoint_restores_pending_world_pose_and_history() {
    let mut s = Session::default();
    let created = run(&mut s, "component.create", json!({"name":"Source","grounded":false}));
    let source = created["component"].as_u64().unwrap();
    let occurrence = created["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length":2,"width":2,"height":2,"body_name":"Block"}));
    run(&mut s, "occurrence.move", json!({"occurrence":occurrence,"translate":[10,0,0],"capture":false}));
    let pending = s.pending_moves.clone();
    let before = s.world_state().body("Block").unwrap().mesh().bounds();
    let undo_count = s.undo.len();
    let checkpoint = s.checkpoint();
    run(&mut s, "component.move_bodies", json!({"component":0,"bodies":["Block"]}));
    s.restore_checkpoint(checkpoint).unwrap();
    assert_eq!(s.pending_moves, pending);
    assert_eq!(s.undo.len(), undo_count);
    assert_eq!(s.world_state().body("Block").unwrap().component, source);
    assert!(s.world_state().body("Block").unwrap().mesh().bounds().min.dist(before.min) < 1e-6);
}

#[test]
fn edited_transfer_destination_updates_history_owner_copy_and_reopen() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":2,"width":2,"height":2,"body_name":"Block"}));
    let a = run(&mut s, "component.create", json!({"name":"A","parent":0}))["component"].as_u64().unwrap();
    let b = run(&mut s, "component.create", json!({"name":"B","parent":0}))["component"].as_u64().unwrap();
    let transferred = run(&mut s, "component.move_bodies", json!({"component":a,"bodies":["Block"]}));
    let feature = transferred["features"][0].as_u64().unwrap();
    run(&mut s, "timeline.edit", json!({"feature":feature,"set":{"to":b,"component":a}}));
    assert_eq!(s.doc.feature(feature).unwrap().component, b);
    assert_eq!(s.model.state().body("Block").unwrap().component, b);
    let saved = s.doc.to_json();
    let pending = s.pending_moves.clone();
    assert!(s.execute("timeline.edit", &json!({"feature":feature,"set":{"to":u64::MAX}})).is_err());
    assert_eq!(s.doc.to_json(), saved);
    assert_eq!(s.pending_moves, pending);
    let copied = run(&mut s, "component.paste_new", json!({"component":b}))["component"].as_u64().unwrap();
    assert_eq!(s.model.state().body("Block (copy)").unwrap().component, copied);
    let mut reopened = Session { doc: solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap().into(), ..Session::default() };
    reopened.refresh();
    assert_eq!(reopened.model.state().body("Block").unwrap().component, b);
    assert_eq!(reopened.model.state().body("Block (copy)").unwrap().component, copied);
}
