use crate::Session;
use serde_json::{Value, json};
use solidcraft_geom::Vec3;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|error| panic!("{command} {params}: {error}"))
}
fn top_pick(s: &Session, body: &str) -> Value {
    let world = s.world_state();
    let body = world.body(body).unwrap();
    let face = body
        .body
        .faces(0.01)
        .unwrap()
        .into_iter()
        .filter(|face| face.plane_normal.is_some_and(|normal| normal.dot(Vec3::Z) > 0.999))
        .max_by(|a, b| a.centroid.z.total_cmp(&b.centroid.z))
        .unwrap();
    json!({"body":body.name,"index":face.index,"face":[face.centroid.x,face.centroid.y,face.centroid.z]})
}
fn draw(s: &mut Session) {
    run(s, "sketch.rectangle.two_point", json!({"p0":[1,1],"p1":[3,3]}));
    run(s, "sketch.dimension", json!({"entities":["l1"],"value":2}));
    run(s, "sketch.finish", json!({}));
}
#[test]
fn exact_selected_base_follows_height_and_never_retargets_control() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Base","name":"BaseFeature"}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":30,"body_name":"Control","name":"ControlFeature"}));
    let pick = top_pick(&s, "Base");
    let sketch = run(&mut s, "sketch.create", json!({"name":"Attached","plane":pick,"project_edges":false}))["sketch"].as_u64().unwrap();
    draw(&mut s);
    run(&mut s, "solid.extrude", json!({"sketch":sketch,"distance":2,"operation":"new","body_name":"Badge"}));
    let height = s.doc.find_feature("BaseFeature").unwrap().param_names[2].clone();
    run(&mut s, "parameters.change", json!({"name":height,"expression":"20 mm"}));
    assert!((s.model.state().sketch(sketch).unwrap().plane.origin.z - 20.).abs() < 1e-6);
    assert!((s.world_state().body("Badge").unwrap().mesh().bounds().max.z - 22.).abs() < 1e-6);
    run(&mut s, "timeline.delete", json!({"features":["BaseFeature"]}));
    assert!(s.model.result(sketch).unwrap().error.is_some());
    assert!(s.model.state().body("Badge").is_none());
    assert!((s.world_state().body("Control").unwrap().mesh().bounds().max.z - 30.).abs() < 1e-6);
    let before = s.doc.to_json();
    let reopened = Session::new(solidcraft_doc::Document::from_json(&before).unwrap());
    assert!(reopened.model.result(sketch).unwrap().error.is_some());
}
#[test]
fn explicit_rebind_preserves_geometry_dimensions_context_and_one_undo() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Base","name":"BaseFeature"}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":30,"body_name":"Control"}));
    let pick = top_pick(&s, "Base");
    let id = run(&mut s, "sketch.create", json!({"plane":pick,"project_edges":false}))["sketch"].as_u64().unwrap();
    draw(&mut s);
    run(&mut s, "timeline.delete", json!({"features":["BaseFeature"]}));
    let geometry = s.doc.sketch(id).unwrap().clone();
    let before = s.doc.to_json();
    let context = (s.active_component, s.active_occurrence_path.clone(), s.active_sketch);
    let pick = top_pick(&s, "Control");
    let commands = vec![("sketch.redefine".into(), json!({"sketch":id,"plane":pick}))];
    let preview = s.preview_visible_world(&commands).unwrap();
    assert_eq!(s.doc.to_json(), before);
    assert!(preview.after.sketch(id).is_some());
    run(&mut s, "sketch.redefine", commands[0].1.clone());
    assert_eq!(s.doc.sketch(id).unwrap(), &geometry);
    assert_eq!((s.active_component, s.active_occurrence_path.clone(), s.active_sketch), context);
    assert!((s.model.state().sketch(id).unwrap().plane.origin.z - 30.).abs() < 1e-6);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
    assert!(s.model.result(id).unwrap().error.is_some());
    run(&mut s, "edit.redo", json!({}));
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!((reopened.model.state().sketch(id).unwrap().plane.origin.z - 30.).abs() < 1e-6);
}
#[test]
fn unsupported_face_has_explicit_fixed_alternative_and_no_live_projection() {
    let mut s = Session::default();
    // A top half Offset Face raised: no construction certificate and no exact history name yet.
    run(&mut s, "solid.box", json!({"length":40,"width":30,"height":20,"body_name":"Base"}));
    run(&mut s, "solid.split_face", json!({"faces":[[10,15,20]],"plane":{"origin":[20,0,0],"normal":[1,0,0]}}));
    run(&mut s, "solid.offset_face", json!({"faces":[[10,15,20]],"distance":5}));
    let mut pick = top_pick(&s, "Base");
    let before = s.doc.to_json();
    let error = s.execute("sketch.create", &json!({"plane":pick})).unwrap_err();
    assert!(error.to_string().contains("fixed"), "{error}");
    assert_eq!(s.doc.to_json(), before);
    pick["fixed"] = json!(true);
    let id = run(&mut s, "sketch.create", json!({"plane":pick}))["sketch"].as_u64().unwrap();
    assert!(matches!(s.doc.feature(id).unwrap().kind, solidcraft_doc::FeatureKind::Sketch { plane: solidcraft_doc::PlaneRef::Custom { .. }, .. }));
    assert!(s.doc.sketch(id).unwrap().links.is_empty());
}
#[test]
fn malformed_exact_face_and_path_leave_every_session_field_unchanged() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Base"}));
    let pick = top_pick(&s, "Base");
    for invalid in [
        json!({"body":"Missing","index":pick["index"],"face":pick["face"]}),
        json!({"body":"Base","index":-1,"face":pick["face"]}),
        json!({"body":"Base","index":pick["index"],"face":pick["face"],"path":[999]}),
        json!({"body":"Base","face":pick["face"]}),
    ] {
        let before = s.doc.to_json();
        let pending = s.pending_moves.clone();
        let context = (s.active_component, s.active_occurrence_path.clone(), s.active_sketch);
        assert!(s.execute("sketch.create", &json!({"plane":invalid})).is_err());
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.pending_moves, pending);
        assert_eq!((s.active_component, s.active_occurrence_path.clone(), s.active_sketch), context);
    }
}

#[test]
fn default_certified_borders_follow_width_and_height_with_stable_entities() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":8,"height":10,"body_name":"Base","name":"BaseFeature"}));
    let pick = top_pick(&s, "Base");
    let id = run(&mut s, "sketch.create", json!({"plane":pick}))["sketch"].as_u64().unwrap();
    assert!(s.doc.sketch(id).unwrap().links.iter().any(|link| link.auto_face));
    let old_ids: Vec<_> = s.doc.sketch(id).unwrap().curves.iter().map(|curve| curve.id.clone()).collect();
    run(&mut s, "sketch.finish", json!({}));
    let dimensions = s.doc.find_feature("BaseFeature").unwrap().param_names.clone();
    run(&mut s, "parameters.change", json!({"name":dimensions[0],"expression":"20 mm"}));
    run(&mut s, "parameters.change", json!({"name":dimensions[2],"expression":"15 mm"}));
    let resolved = s.model.state();
    let sketch = resolved.sketch(id).unwrap();
    assert!((sketch.plane.origin.z - 15.).abs() < 1e-6);
    assert!(sketch.sketch.links.iter().all(|link| !link.lost));
    assert_eq!(sketch.sketch.curves.iter().map(|curve| curve.id.clone()).collect::<Vec<_>>(), old_ids);
    let max_x = sketch.sketch.points.iter().map(|point| point.pos.x).fold(f64::NEG_INFINITY, f64::max);
    let min_x = sketch.sketch.points.iter().map(|point| point.pos.x).fold(f64::INFINITY, f64::min);
    let max_y = sketch.sketch.points.iter().map(|point| point.pos.y).fold(f64::NEG_INFINITY, f64::max);
    let min_y = sketch.sketch.points.iter().map(|point| point.pos.y).fold(f64::INFINITY, f64::min);
    assert!(((max_x - min_x) * (max_y - min_y) - 160.).abs() < 1e-5);
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!(reopened.model.state().sketch(id).unwrap().sketch.links.iter().all(|link| !link.lost));
}

#[test]
fn nested_offset_retains_exact_attachment_during_height_change() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Base","name":"BaseFeature"}));
    let pick = top_pick(&s, "Base");
    let id = run(&mut s, "sketch.create", json!({"plane":pick,"offset":"3 mm","project_edges":false}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.finish", json!({}));
    let height = s.doc.find_feature("BaseFeature").unwrap().param_names[2].clone();
    run(&mut s, "parameters.change", json!({"name":height,"expression":"20 mm"}));
    assert!((s.model.state().sketch(id).unwrap().plane.origin.z - 23.).abs() < 1e-6);
}

#[test]
fn exact_nested_source_and_cross_owner_pending_paths_capture_only_involved_positions() {
    let mut s = Session::default();
    let parent = run(&mut s, "component.create", json!({"name":"Parent","parent":0}));
    let second = run(&mut s, "occurrence.copy", json!({"occurrence":parent["occurrence"],"translate":[40,0,0]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "component.activate", json!({"component":parent["component"],"occurrence":second,"path":[second]}));
    let child = run(&mut s, "component.create", json!({"name":"Child"}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Base"}));
    let child_occ = child["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":second,"translate":[10,0,0],"capture":false}));
    run(&mut s, "occurrence.move", json!({"occurrence":child_occ,"translate":[0,20,0],"capture":false}));
    let unrelated = run(&mut s, "component.create", json!({"name":"Unrelated","parent":0}));
    run(&mut s, "occurrence.move", json!({"occurrence":unrelated["occurrence"],"translate":[300,0,0],"capture":false}));
    let author = run(&mut s, "component.create", json!({"name":"Author","parent":0}));
    run(&mut s, "occurrence.move", json!({"occurrence":author["occurrence"],"translate":[100,0,0],"angle":"90 deg","capture":false}));
    let world = s.world_state();
    let key = s
        .placed_document()
        .body_frames_with_source(&s.model.state())
        .into_iter()
        .find(|(_, source, _, path, _)| source == "Base" && *path == vec![second, child_occ])
        .unwrap()
        .0;
    assert!(world.body(&key).is_some());
    let mut pick = top_pick(&s, &key);
    pick["path"] = json!([second, child_occ]);
    let before = s.doc.to_json();
    let pending = s.pending_moves.clone();
    let id = run(&mut s, "sketch.create", json!({"plane":pick,"project_edges":false}))["sketch"].as_u64().unwrap();
    let solidcraft_doc::FeatureKind::Sketch { plane, .. } = &s.doc.feature(id).unwrap().kind else { panic!("not sketch") };
    let anchor = super::anchor(plane).unwrap();
    assert_eq!(anchor.path, vec![second, child_occ]);
    assert_eq!(anchor.author_path, vec![author["occurrence"].as_u64().unwrap()]);
    assert_eq!(s.pending_moves.len(), 1);
    assert!(s.pending_moves.contains_key(&unrelated["occurrence"].as_u64().unwrap()));
    let saved = s.doc.to_json();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
    assert_eq!(s.pending_moves, pending);
    run(&mut s, "edit.redo", json!({}));
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    assert!(reopened.model.result(id).unwrap().error.is_none());
    run(&mut reopened, "component.activate", json!({"component":0}));
    let original = reopened.doc.to_json();
    let undo = reopened.undo.len();
    let dirty = reopened.is_dirty();
    run(&mut reopened, "sketch.edit", json!({"sketch":id}));
    assert_eq!(reopened.doc.to_json(), original);
    assert_eq!(reopened.undo.len(), undo);
    assert_eq!(reopened.is_dirty(), dirty);
    assert_eq!(reopened.active_component, author["component"].as_u64().unwrap());
    assert_eq!(reopened.active_occurrence_path, Some(vec![author["occurrence"].as_u64().unwrap()]));
}

#[test]
fn fixed_rebind_freezes_current_resized_borders_and_only_detaches_auto_links() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":8,"height":10,"body_name":"Base","name":"BaseFeature"}));
    let pick = top_pick(&s, "Base");
    let id = run(&mut s, "sketch.create", json!({"plane":pick}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.project", json!({"refs":[{"origin":true}]}));
    run(&mut s, "sketch.finish", json!({}));
    let length = s.doc.find_feature("BaseFeature").unwrap().param_names[0].clone();
    run(&mut s, "parameters.change", json!({"name":length,"expression":"20 mm"}));
    let visible = s.model.state().sketch(id).unwrap().sketch.clone();
    let auto: Vec<_> = visible.links.iter().filter(|link| link.auto_face).map(|link| link.id.clone()).collect();
    let manual: Vec<_> = visible.links.iter().filter(|link| !link.auto_face).map(|link| link.id.clone()).collect();
    let before = s.doc.to_json();
    let mut pick = top_pick(&s, "Base");
    pick["fixed"] = json!(true);
    run(&mut s, "sketch.redefine", json!({"sketch":id,"plane":pick}));
    let fixed = s.doc.sketch(id).unwrap();
    assert!(auto.iter().all(|id| fixed.link(id).is_none()));
    assert!(manual.iter().all(|id| fixed.link(id).is_some()));
    assert_eq!(
        fixed.curves.iter().map(|curve| curve.id.clone()).collect::<Vec<_>>(),
        visible.curves.iter().map(|curve| curve.id.clone()).collect::<Vec<_>>()
    );
    assert_eq!(fixed.points.iter().map(|point| point.pos).collect::<Vec<_>>(), visible.points.iter().map(|point| point.pos).collect::<Vec<_>>());
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!(reopened.model.result(id).unwrap().error.is_none());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn strict_anchor_timeline_copy_remaps_producer_and_refuses_external_translation_atomically() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Base","name":"BaseFeature"}));
    let pick = top_pick(&s, "Base");
    let id = run(&mut s, "sketch.create", json!({"name":"Attached","plane":pick}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "timeline.copy", json!({"features":["Attached"]}));
    let before = s.doc.to_json();
    let pending = s.pending_moves.clone();
    let error = s.execute("timeline.paste", &json!({"translate":[30,0,0]})).unwrap_err();
    assert!(error.to_string().contains("external exact face"), "{error}");
    assert_eq!(s.doc.to_json(), before);
    assert_eq!(s.pending_moves, pending);
    run(&mut s, "timeline.copy", json!({"features":["BaseFeature","Attached"]}));
    let pasted = run(&mut s, "timeline.paste", json!({"translate":[30,0,0]}));
    assert!(pasted["errors"].as_array().unwrap().is_empty(), "{pasted}");
    let new_id = pasted["pasted"][1]["id"].as_u64().unwrap();
    let solidcraft_doc::FeatureKind::Sketch { plane, .. } = &s.doc.feature(new_id).unwrap().kind else { panic!("not sketch") };
    let anchor = super::anchor(plane).unwrap();
    assert_ne!(anchor.target.origin_feature, s.doc.find_feature("BaseFeature").unwrap().id);
    assert_ne!(anchor.target.body, "Base");
    assert!(s.model.state().sketch(new_id).unwrap().sketch.links.iter().all(|link| !link.lost));
    assert!(s.model.state().sketch(id).unwrap().sketch.links.iter().all(|link| !link.lost));
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!(reopened.model.result(new_id).unwrap().error.is_none());
}

#[test]
fn strict_anchor_deep_clone_is_independent_and_rename_preserves_certified_projection() {
    let mut s = Session::default();
    let part = run(&mut s, "component.create", json!({"name":"Part","parent":0}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Base","name":"BaseFeature"}));
    let pick = top_pick(&s, "Base");
    let id = run(&mut s, "sketch.create", json!({"name":"Attached","plane":pick}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.finish", json!({}));
    let height = s.doc.find_feature("BaseFeature").unwrap().param_names[2].clone();
    let pasted = run(&mut s, "component.paste_new", json!({"component":part["component"],"translate":[30,0,0]}));
    let owner = pasted["component"].as_u64().unwrap();
    let new_feature = s
        .doc
        .features
        .iter()
        .find(|feature| feature.component == owner && matches!(feature.kind, solidcraft_doc::FeatureKind::Sketch { .. }))
        .unwrap();
    let new_id = new_feature.id;
    let solidcraft_doc::FeatureKind::Sketch { plane, .. } = &new_feature.kind else { panic!("not sketch") };
    let anchor = super::anchor(plane).unwrap();
    let body = anchor.target.body.clone();
    assert_eq!(anchor.component, owner);
    run(&mut s, "parameters.change", json!({"name":height,"expression":"20 mm"}));
    assert!((s.model.state().sketch(id).unwrap().plane.origin.z - 20.).abs() < 1e-6);
    assert!((s.model.state().sketch(new_id).unwrap().plane.origin.z - 10.).abs() < 1e-6);
    run(&mut s, "body.rename", json!({"body":body,"name":"Independent"}));
    assert!(s.model.result(new_id).unwrap().error.is_none());
    assert!(s.model.state().sketch(new_id).unwrap().sketch.links.iter().all(|link| !link.lost));
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!(reopened.model.result(new_id).unwrap().error.is_none());
}

#[test]
fn legacy_or_fixed_rebind_needs_explicit_author_path_for_repeated_owner() {
    let mut s = Session::default();
    let part = run(&mut s, "component.create", json!({"name":"Part","parent":0}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Base"}));
    let mut pick = top_pick(&s, "Base");
    pick["fixed"] = json!(true);
    let id = run(&mut s, "sketch.create", json!({"plane":pick}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.finish", json!({}));
    let second = run(&mut s, "occurrence.copy", json!({"occurrence":part["occurrence"],"translate":[20,0,0]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "component.activate", json!({"component":0}));
    let pick = top_pick(&s, "Base");
    let before = s.doc.to_json();
    let error = s.execute("sketch.redefine", &json!({"sketch":id,"plane":pick})).unwrap_err();
    assert!(error.to_string().contains("ambiguous"), "{error}");
    assert_eq!(s.doc.to_json(), before);
    run(&mut s, "sketch.redefine", json!({"sketch":id,"plane":pick,"author_path":[second]}));
    let solidcraft_doc::FeatureKind::Sketch { plane, .. } = &s.doc.feature(id).unwrap().kind else { panic!("not sketch") };
    assert_eq!(super::anchor(plane).unwrap().author_path, vec![second]);
    assert_eq!(s.active_component, 0);
}

#[test]
fn replacement_producer_with_same_body_name_cannot_heal_lost_anchor() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Base","name":"Original"}));
    let pick = top_pick(&s, "Base");
    let id = run(&mut s, "sketch.create", json!({"plane":pick,"project_edges":false}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "timeline.suppress", json!({"feature":"Original","suppressed":true}));
    run(&mut s, "timeline.rollback", json!({"position":1}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":30,"body_name":"Base","name":"Replacement"}));
    run(&mut s, "timeline.rollback", json!({}));
    assert!(s.model.state().body("Base").is_some());
    assert!(s.model.result(id).unwrap().error.is_some());
    let before = s.doc.to_json();
    let pick = top_pick(&s, "Base");
    run(&mut s, "sketch.redefine", json!({"sketch":id,"plane":pick}));
    assert!((s.model.state().sketch(id).unwrap().plane.origin.z - 30.).abs() < 1e-6);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
    assert!(s.model.result(id).unwrap().error.is_some());
}

#[test]
fn freezing_rebuilt_arc_borders_keeps_authored_dimension_entity_indices() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name":"turn","expression":"270 deg","unit":"deg"}));
    let profile = run(&mut s, "sketch.create", json!({"plane":"XY"}))["sketch"].as_u64().unwrap();
    let arc = run(
        &mut s,
        "sketch.draw.dimensioned",
        json!({"command":"sketch.arc.center_point","params":{"center":[0,0],"start":[10,0],"sweep":270},"dimensions":{"radius":10,"sweep":"turn"}}),
    )["curves"][0]
        .as_str()
        .unwrap()
        .to_owned();
    run(&mut s, "sketch.line", json!({"points":[format!("{arc}.end"),format!("{arc}.start")],"infer":false}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":profile,"distance":10,"operation":"new","body_name":"Sector"}));
    let pick = top_pick(&s, "Sector");
    let id = run(&mut s, "sketch.create", json!({"plane":pick}))["sketch"].as_u64().unwrap();
    let circle = run(&mut s, "sketch.circle.center", json!({"center":[20,20],"radius":2}))["curves"][0].as_str().unwrap().to_owned();
    run(&mut s, "sketch.dimension", json!({"entities":[circle],"type":"radius","value":"2 mm"}));
    run(&mut s, "sketch.finish", json!({}));
    let before_count = s.model.state().sketch(id).unwrap().sketch.curves.len();
    run(&mut s, "parameters.change", json!({"name":"turn","expression":"300 deg"}));
    assert!(s.model.result(id).unwrap().error.is_none());
    let visible = s.model.state().sketch(id).unwrap().sketch.clone();
    assert_ne!(visible.curves.len(), before_count, "fixture must rebuild projected topology");
    let mut pick = top_pick(&s, "Sector");
    pick["fixed"] = json!(true);
    let before = s.doc.to_json();
    run(&mut s, "sketch.redefine", json!({"sketch":id,"plane":pick}));
    let frozen = s.doc.sketch(id).unwrap();
    let circle_index = frozen.curve_index(&circle).unwrap();
    let dimension = frozen
        .constraints
        .iter()
        .find(|constraint| matches!(constraint.kind, solidcraft_sketch::ConstraintKind::Radius { c, .. } if c == circle_index))
        .unwrap();
    assert!((solidcraft_sketch::measure_dimension(frozen, &dimension.kind).unwrap() - 2.).abs() < 1e-6);
    assert_eq!(frozen.curves.iter().map(|curve| &curve.id).collect::<Vec<_>>(), visible.curves.iter().map(|curve| &curve.id).collect::<Vec<_>>());
    assert!(s.model.result(id).unwrap().error.is_none());
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!(reopened.model.result(id).unwrap().error.is_none());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn angled_attachment_tracks_source_plane_and_reopens() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Base","name":"BaseFeature"}));
    let pick = top_pick(&s, "Base");
    let id = run(&mut s, "sketch.create", json!({"plane":pick,"project_edges":false}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.finish", json!({}));
    let solidcraft_doc::FeatureKind::Sketch { plane, .. } = &mut s.doc_mut().feature_mut(id).unwrap().kind else { panic!("not sketch") };
    *plane = solidcraft_doc::PlaneRef::AtAngle {
        base: Box::new(plane.clone()),
        axis_origin: Vec3::ZERO,
        axis_dir: Vec3::X,
        angle: "90 deg".into(),
        line: None,
    };
    s.refresh();
    let height = s.doc.find_feature("BaseFeature").unwrap().param_names[2].clone();
    run(&mut s, "parameters.change", json!({"name":height,"expression":"20 mm"}));
    let placed = s.model.state().sketch(id).unwrap().plane;
    assert!((placed.origin.y + 20.).abs() < 1e-6);
    assert!(placed.origin.z.abs() < 1e-6);
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_eq!(reopened.model.state().sketch(id).unwrap().plane, placed);
}

#[test]
fn rounded_enclosure_shell_side_and_floor_attach_through_width_wall_and_reopen() {
    use solidcraft_doc::{FeatureKind, config::FaceCertificate, face_anchor::plane_face_anchor};
    let mut s = Session::default();
    for (name, expression) in [("width", "80 mm"), ("depth", "60 mm"), ("height", "30 mm"), ("wall", "2 mm")] {
        run(&mut s, "parameters.add", json!({"name":name,"expression":expression}));
    }
    let base = run(&mut s, "sketch.create", json!({"plane":"XY","name":"Outline"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.rectangle.center", json!({"center":[0,0],"corner":[40,30]}));
    run(&mut s, "sketch.dimension", json!({"entities":["l1"],"value":"width"}));
    run(&mut s, "sketch.dimension", json!({"entities":["l2"],"value":"depth"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":base,"distance":"height","operation":"new","body_name":"Case"}));
    run(&mut s, "solid.fillet", json!({"body":"Case","edges":[[40,30,15],[-40,30,15],[-40,-30,15],[40,-30,15]],"radius":6}));
    run(&mut s, "solid.shell", json!({"body":"Case","faces":[[0,0,30]],"thickness":"wall","name":"Hollow"}));
    assert_eq!(s.model.state().body("Case").unwrap().body.face_count(), 19, "rounded Shell has no separate base strips");
    let pick = |s: &Session, normal: Vec3, point: Vec3| {
        let state = s.model.state();
        let body = state.body("Case").unwrap();
        let face = body
            .body
            .faces(0.01)
            .unwrap()
            .into_iter()
            .find(|f| f.plane_normal.is_some_and(|n| n.dot(normal) > 0.999) && (f.centroid - point).dot(normal).abs() < 1e-6)
            .unwrap();
        json!({"body":"Case","index":face.index,"face":[point.x,point.y,point.z]})
    };
    let side_pick = pick(&s, Vec3::X, Vec3::new(40., 10., 20.));
    let side = run(&mut s, "sketch.create", json!({"name":"USB","plane":side_pick,"project_edges":true}))["sketch"].as_u64().unwrap();
    let port_plane = s.model.state().sketch(side).unwrap().plane;
    let p0 = port_plane.to_local(Vec3::new(40., -6., 8.5));
    let p1 = port_plane.to_local(Vec3::new(40., 6., 15.5));
    let port_curves = run(&mut s, "sketch.rectangle.two_point", json!({"p0":[p0.x,p0.y],"p1":[p1.x,p1.y]}))["curves"].clone();
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch":side,"profiles":[{"curves":port_curves}],"distance":"wall","direction":"negative","operation":"cut","targets":["Case"],"name":"USBcut"}),
    );
    let floor_pick = pick(&s, Vec3::Z, Vec3::new(6., -9., 2.));
    let floor = run(&mut s, "sketch.create", json!({"name":"Bosses","plane":floor_pick,"project_edges":true}))["sketch"].as_u64().unwrap();
    let boss_curves = run(&mut s, "sketch.circle.center", json!({"center":[30,20],"radius":3.5}))["curves"].clone();
    let centre = format!("{}.center", boss_curves[0].as_str().unwrap());
    run(&mut s, "sketch.dimension", json!({"entities":["origin",centre],"type":"horizontal","value":"width / 2 - 10 mm"}));
    run(&mut s, "sketch.dimension", json!({"entities":["origin",centre],"type":"vertical","value":"depth / 2 - 10 mm"}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch":floor,"profiles":[{"curves":boss_curves}],"distance":12,"operation":"join","targets":["Case"],"name":"Boss"}),
    );
    run(
        &mut s,
        "solid.pattern.rectangular",
        json!({"features":["Boss"],"dir1":[-1,0,0],"count1":2,"spacing1":"width - 20 mm","dir2":[0,-1,0],"count2":2,"spacing2":"depth - 20 mm","name":"BossPattern"}),
    );
    for id in [side, floor] {
        let FeatureKind::Sketch { plane, .. } = &s.doc.feature(id).unwrap().kind else { panic!() };
        assert!(matches!(plane_face_anchor(plane).unwrap().target.certificate, Some(FaceCertificate::ShellPlanar { .. })));
    }
    run(&mut s, "parameters.change", json!({"name":"width","expression":"100 mm"}));
    run(&mut s, "parameters.change", json!({"name":"wall","expression":"3 mm"}));
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    for session in [&s, &reopened] {
        for id in [side, floor] {
            let result = session.model.result(id).unwrap();
            assert!(result.error.is_none(), "{}: {:?}", result.name, result.error);
            assert!(result.warning.is_none(), "{}: {:?}", result.name, result.warning);
            assert!(
                session.model.state().sketch(id).unwrap().sketch.links.iter().all(|link| !link.lost),
                "projected boundary must resolve current geometry"
            );
        }
        let state = session.model.state();
        assert!(state.body("Case").is_some());
        assert!((state.sketch(side).unwrap().plane.origin.x - 50.).abs() < 1e-6);
        assert!((state.sketch(floor).unwrap().plane.origin.z - 3.).abs() < 1e-6);
        assert!(state.sketch(floor).unwrap().plane.normal().dot(Vec3::Z) > 0.999);
        let mesh = state.body("Case").unwrap().body.tessellate_with(1e-4, true).unwrap();
        let case = &state.body("Case").unwrap().body;
        let corners: Vec<_> = (0..case.face_count()).filter(|i| matches!(solidcraft_kernel::face_surface(case, *i), Some(solidcraft_kernel::FaceSurface::Cylinder { radius, .. }) if (radius - 6.).abs() < 1e-6)).collect();
        assert_eq!(corners.len(), 4, "later port/boss features must not subdivide untouched outer corners");
        for index in corners {
            let points: Vec<_> = mesh
                .edge_faces
                .iter()
                .enumerate()
                .filter(|(_, faces)| faces.contains(&(index as u32)))
                .flat_map(|(edge, _)| &mesh.edges[edge])
                .collect();
            assert!(points.iter().any(|p| p.z.abs() < 1e-8));
            assert!(points.iter().any(|p| (p.z - 30.).abs() < 1e-8));
        }

        assert!(!mesh.contains(Vec3::new(49., 0., 12.)), "USB must remain open after resize/reopen");
        assert!(mesh.contains(Vec3::new(49., 15., 12.)), "adjacent right wall remains solid");
        assert!(mesh.contains(Vec3::new(-49., 0., 12.)), "opposite wall must stay solid");
        let seed = state.sketch(floor).unwrap().plane.to_world(solidcraft_geom::Vec2::new(40., 20.)) + Vec3::Z * 6.;
        assert!(mesh.contains(seed), "seed boss starts on current floor");
        for shift in [Vec3::ZERO, Vec3::new(-80., 0., 0.), Vec3::new(0., -40., 0.), Vec3::new(-80., -40., 0.)] {
            assert!(mesh.contains(seed + shift), "each patterned boss must survive resize/reopen at {:?}", seed + shift);
        }
        assert!(!mesh.contains(Vec3::new(0., 0., 6.)), "the cavity remains hollow");
        let area = |w: f64, d: f64, r: f64| w * d - (4. - std::f64::consts::PI) * r * r;
        let expected = area(100., 60., 6.) * 30. - area(94., 54., 3.) * 27. - 12. * 7. * 3. + 4. * std::f64::consts::PI * 3.5 * 3.5 * 12.;
        assert!((mesh.measure().volume - expected).abs() < 0.1, "actual {}, analytic {expected}", mesh.measure().volume);
    }
    run(&mut s, "timeline.suppress", json!({"feature":"Hollow","suppressed":true}));
    assert!(s.model.result(side).unwrap().error.is_some());
    assert!(s.model.result(floor).unwrap().error.is_some());
}

#[test]
fn offset_rectangle_pocket_has_one_logical_outer_wall_and_closed_face_projection() {
    let mut s = Session::default();
    let base = run(&mut s, "sketch.create", json!({"plane":"XY"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.rectangle.center", json!({"center":[0,0],"corner":[30,27]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":base,"distance":10,"operation":"new","body_name":"Stock"}));
    let control_sketch = run(&mut s, "sketch.create", json!({"plane":"XY"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[100,100],"p1":[104,104]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":control_sketch,"distance":10,"operation":"new","body_name":"Control"}));
    let untouched = s.model.state().body("Control").unwrap().body.clone();
    let before_control = solidcraft_kernel::step_export(&[&untouched], "control snapshot").unwrap();
    let pick = top_pick(&s, "Stock");
    let pocket = run(&mut s, "sketch.create", json!({"plane":pick}))["sketch"].as_u64().unwrap();
    let side = {
        let state = s.model.state();
        let sk = &state.sketch(pocket).unwrap().sketch;
        let curve = sk.curves.iter().find(|c| c.id == "l1").unwrap();
        let solidcraft_sketch::CurveKind::Line { a, b } = curve.kind else { panic!() };
        sk.point(a).unwrap() + sk.point(b).unwrap()
    };
    let outer = run(&mut s, "sketch.offset", json!({"curves":["l1"],"distance":0.3425231845240546,"side":[side.x,side.y]}))["curves"].clone();
    let inner = run(&mut s, "sketch.offset", json!({"curves":[outer[0]],"distance":5.609348914858092,"side":[0,0]}))["curves"].clone();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":pocket,"profiles":[{"curves":inner}],"distance":-9,"operation":"cut"}));
    let control = s.model.state().body("Control").unwrap().body.clone();
    assert!(control.same(&untouched), "a separate body crossing the cutter end plane remains the exact same solid");
    assert_eq!(solidcraft_kernel::step_export(&[&control], "control snapshot").unwrap(), before_control);
    let state = s.model.state();
    let body = state.body("Stock").unwrap();
    let display = body.body.display_mesh(0.01).unwrap();
    let face = body
        .body
        .faces(0.01)
        .unwrap()
        .into_iter()
        .find(|face| face.plane_normal.is_some_and(|n| n.dot(Vec3::X) > 0.999) && (face.centroid.x - 30.).abs() < 1e-6)
        .unwrap();
    assert_eq!(body.body.faces(0.01).unwrap().len(), 11, "actual pocket topology has no outer floor-height strips");
    assert_eq!(display.surface_patch(face.index).len(), 1);
    let attached = run(&mut s, "sketch.create", json!({"plane":{"body":"Stock","index":face.index,"face":[30,0,0.5]}}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.finish", json!({}));
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    for session in [&s, &reopened] {
        let result = session.model.result(attached).unwrap();
        assert!(result.error.is_none(), "{:?}", result.error);
        assert!(result.warning.is_none(), "{:?}", result.warning);
        let state = session.model.state();
        let sketch = &state.sketch(attached).unwrap().sketch;
        assert!(sketch.links.iter().all(|link| !link.lost));
        assert_eq!(sketch.curves.len(), 4, "whole outer-wall rectangle has no floor-height stations");
        let mesh = state.body("Stock").unwrap().body.display_mesh(0.01).unwrap();
        assert!(mesh.contains(Vec3::new(0., 0., 0.5)));
        assert!(!mesh.contains(Vec3::new(0., 0., 5.)));
        assert!(mesh.contains(Vec3::new(28., 0., 5.)));
        let control = &state.body("Control").unwrap().body;
        assert_eq!(control.faces(0.01).unwrap().len(), 6);
        assert_eq!(control.edges(0.01).unwrap().len(), 12);
        let control_mesh = control.tessellate(0.01).unwrap();
        assert!((control_mesh.measure().volume - 160.).abs() < 1e-6);
        assert!(control_mesh.contains(Vec3::new(102., 102., 0.5)));
        assert!(control_mesh.contains(Vec3::new(102., 102., 5.)));
        let expected = 60. * 54. * 10. - 4. * 24.73317426966596 * 21.733174269665962 * 9.;
        assert!((mesh.measure().volume - expected).abs() < 1e-6, "actual {}, expected {expected}", mesh.measure().volume);
    }
}

/// Every flat face of every body: (body, face index, a point well inside it, its normal). The
/// middle of the face's largest triangle, so an L-shaped face is picked on itself.
fn flat_faces(s: &Session) -> Vec<(String, usize, Vec3, Vec3)> {
    let world = s.world_state();
    let mut out = Vec::new();
    for body in &world.bodies {
        let mesh = body.mesh();
        let mut inside: std::collections::BTreeMap<usize, (Vec3, f64)> = std::collections::BTreeMap::new();
        for (triangle, face) in mesh.triangles.iter().zip(&mesh.tri_face) {
            if let Some([a, b, c]) = mesh.tri(triangle) {
                let area = (b - a).cross(c - a).len();
                let entry = inside.entry(*face as usize).or_insert(((a + b + c) * (1. / 3.), -1.));
                if area > entry.1 {
                    *entry = ((a + b + c) * (1. / 3.), area);
                }
            }
        }
        for (index, (point, _)) in inside {
            if let Some(plane) = body.body.planar_face(index) {
                out.push((body.name.clone(), index, point, plane.normal()));
            }
        }
    }
    out
}

fn timeline_errors(s: &Session) -> Vec<String> {
    s.model.results.iter().filter_map(|r| r.error.as_ref().map(|e| format!("{}: {e}", r.name))).collect()
}

fn save_and_reopen(s: &mut Session, dir: &std::path::Path, name: &str) -> Session {
    let path = dir.join(name);
    run(s, "file.save_as", json!({"path": path.to_string_lossy()}));
    let mut reopened = Session::default();
    run(&mut reopened, "doc.open", json!({"path": path.to_string_lossy()}));
    reopened
}

/// A sketch on every flat face of `s`'s bodies (as the desktop picks it: body, index, a point on
/// it) stays attached to that face, and a square drawn on it extrudes out of the face, fresh and
/// after the result is saved and opened again.
fn sketch_and_extrude_on_every_flat_face(s: &Session, dir: &std::path::Path, label: &str) -> usize {
    use solidcraft_doc::FaceAttachmentStatus;
    let faces = flat_faces(s);
    let before = timeline_errors(s);
    for (body, index, point, normal) in &faces {
        let what = format!("{label}: {body} face {index} at {point:?}");
        let mut t = s.scratch();
        let pick = json!({"body": body, "index": index, "face": [point.x, point.y, point.z]});
        let sketch =
            t.execute("sketch.create", &json!({"plane": pick, "project_edges": false})).unwrap_or_else(|error| panic!("{what}: {error}"))["sketch"]
                .as_u64()
                .unwrap();
        let plane = t.model.state().sketch(sketch).unwrap().plane;
        assert!(plane.normal().dot(*normal) > 1. - 1e-9, "{what}: the sketch faces out of the face");
        let local = [(*point - plane.origin).dot(plane.x), (*point - plane.origin).dot(plane.y)];
        run(&mut t, "sketch.rectangle.two_point", json!({"p0": [local[0] - 0.5, local[1] - 0.5], "p1": [local[0] + 0.5, local[1] + 0.5]}));
        run(&mut t, "sketch.finish", json!({}));
        t.execute("solid.extrude", &json!({"sketch": sketch, "distance": 1, "operation": "join"})).unwrap_or_else(|error| panic!("{what}: {error}"));
        let check = |t: &Session, when: &str| {
            assert!(
                matches!(t.placed_document().sketch_face_attachment(sketch, &t.model), FaceAttachmentStatus::Healthy { .. }),
                "{what} ({when}): the sketch must stay attached to its face"
            );
            assert_eq!(timeline_errors(t), before, "{what} ({when}): no new timeline errors");
        };
        check(&t, "fresh");
        let reopened = save_and_reopen(&mut t, dir, &format!("{label}-{index}.solidcraft"));
        check(&reopened, "after save and reopen");
    }
    faces.len()
}

/// The logo: one sketch extruded up and the same sketch extruded down, joined. Its side walls
/// are merged from both extrudes' sides, or run along two coincident sketch lines (squares drawn
/// edge to edge); each still takes a sketch that follows it, as the tops and bottoms do.
#[test]
fn every_flat_face_of_one_sketch_extruded_up_and_down_takes_an_attached_sketch() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/action_matrix/designs");
    let dir = std::env::temp_dir().join(format!("solidcraft-up-down-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script: Value = serde_json::from_str(&std::fs::read_to_string(root.join("up-down-join.json")).unwrap()).unwrap();
    let mut s = Session::default();
    s.run_script(&script).unwrap();
    assert_eq!(s.world_state().bodies.len(), 1, "the two extrudes join into one body");
    assert_eq!(sketch_and_extrude_on_every_flat_face(&s, &dir, "synthetic"), 12, "8 side walls, top, bottom, 2 at the sketch plane");
    let reopened = save_and_reopen(&mut s, &dir, "synthetic.solidcraft");
    assert_eq!(sketch_and_extrude_on_every_flat_face(&reopened, &dir, "synthetic-reopened"), 12);
    // The logo design as it was reported (format 17, with a removed shelled body before it).
    let mut logo = Session::default();
    run(&mut logo, "doc.open", json!({"path": root.join("logo-up-down-join.sc").to_string_lossy()}));
    assert_eq!(sketch_and_extrude_on_every_flat_face(&logo, &dir, "logo"), 12);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A face given next to `plane` instead of inside it was ignored and the sketch went to XY:
/// it is refused, with where it belongs, and nothing is added. Inside `plane` it attaches.
#[test]
fn a_face_next_to_plane_is_refused_not_ignored() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10}));
    let features = s.doc.features.len();
    for p in [
        json!({"face": [10, 10, 10]}),
        json!({"plane": "XY", "face": [10, 10, 10]}),
        json!({"origin": [0, 0, 5], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}),
    ] {
        let e = s.execute("sketch.create", &p).unwrap_err().to_string();
        assert!(e.contains("belongs inside `plane`"), "{p}: {e}");
    }
    assert_eq!(s.doc.features.len(), features);
    assert!(s.active_sketch.is_none());
    let r = run(&mut s, "sketch.create", json!({"plane": {"face": [10, 10, 10]}}));
    assert_eq!(r["attachment"], "exact_face", "{r}");
}

/// #3484: a rectangle swept along a path with a right-angle corner has flat side walls; a sketch
/// can be created on them (they are exact planes, like those of a straight sweep).
#[test]
fn sketch_on_a_flat_wall_of_a_cornered_sweep() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Q"}));
    run(&mut s, "sketch.rectangle.center", json!({"center": [0, 0], "corner": [30, 15]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "P"}));
    // The XZ sketch's y runs down world Z: 100 up, then 100 along +X.
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, -100], [100, -100]], "ids": ["up", "across"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.sweep", json!({"sketch": "Q", "path_sketch": "P", "path": ["up", "across"], "operation": "new", "body_name": "Tube"}));
    // Walls of the first leg (normal -Y, +X) and of the second (underside, -Y), one point each.
    for point in [[0.0, -15.0, 50.0], [30.0, 0.0, 50.0], [60.0, -15.0, 85.0]] {
        let r = s.execute("sketch.create", &json!({"plane": {"face": point}})).unwrap_or_else(|e| panic!("{point:?}: {e}"));
        assert_eq!(r["attachment"], "exact_face", "{point:?}: {r}");
        run(&mut s, "sketch.finish", json!({}));
    }
    // A fixed plane on the same wall is accepted by index as well.
    let world = s.world_state();
    let body = world.body("Tube").unwrap();
    let wall = body.body.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.dot(-Vec3::Y) > 0.999)).unwrap();
    let pick = json!({"body": "Tube", "index": wall.index, "face": [wall.centroid.x, wall.centroid.y, wall.centroid.z]});
    drop(world);
    run(&mut s, "sketch.create", json!({"plane": pick.clone(), "fixed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": pick}));
}

/// #3248: a custom plane's `origin` that is present but not three numbers (an expression such as
/// "w/2", a number with a unit, too few values) became [0, 0, 0] without a word. It is refused,
/// like a bad `x_dir`, and nothing is added. A missing origin is still the world origin.
#[test]
fn a_custom_plane_origin_that_is_not_numbers_is_refused_not_zeroed() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "w", "expression": "40 mm"}));
    let features = s.doc.features.len();
    for origin in [json!(["w/2", 0, 0]), json!(["20 mm", 0, 0]), json!([20, 0]), json!("w"), json!([20, 0, null])] {
        let p = json!({"plane": {"origin": origin, "x_dir": [0, 1, 0], "y_dir": [0, 0, 1]}});
        let e = s.execute("sketch.create", &p).unwrap_err().to_string();
        assert!(e.contains("origin"), "{p}: {e}");
    }
    assert_eq!(s.doc.features.len(), features);
    assert!(s.active_sketch.is_none());
    let origin_of = |s: &mut Session, plane: Value| {
        run(s, "sketch.create", json!({"plane": plane}));
        let r = run(s, "sketch.inspect", json!({}));
        run(s, "sketch.finish", json!({}));
        r["plane"]["origin"].clone()
    };
    assert_eq!(origin_of(&mut s, json!({"origin": [20, 0, 0], "x_dir": [0, 1, 0], "y_dir": [0, 0, 1]})), json!([20.0, 0.0, 0.0]));
    assert_eq!(origin_of(&mut s, json!({"x_dir": [0, 1, 0], "y_dir": [0, 0, 1]})), json!([0.0, 0.0, 0.0]));
}
/// A sketch on a body's face projects that face's edges from the body's display mesh. The body
/// it projects from is the model's own, not a copy that meshes it again: on a body whose curved
/// edges are slow to mesh, the copy's mesh was thrown away and the next feature meshed the body
/// a second time, which made a sketch on a face as slow as a cut.
#[test]
fn a_sketch_on_a_face_meshes_its_body_once() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Base","name":"BaseFeature"}));
    assert!(s.model.state().body("Base").unwrap().cached_mesh().is_none());
    let pick = top_pick(&s, "Base");
    run(&mut s, "sketch.create", json!({"name":"Attached","plane":pick}));
    run(&mut s, "sketch.finish", json!({}));
    assert!(s.model.state().body("Base").unwrap().cached_mesh().is_some(), "the sketch's projection meshed a copy of the body");
}
/// Issue 6720: a face picked by its point alone was refused on a body with many faces. The
/// search charged the whole mesh once per face, and the 5 000 facets a modelled thread leaves
/// put a plain block past the limit. The search reads each face's own triangles now.
#[test]
fn a_face_picked_by_point_on_a_block_with_a_modelled_thread_is_found() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner":[-20,-20,0],"length":40,"width":40,"height":20,"body_name":"Block"}));
    run(&mut s, "solid.cylinder", json!({"base":[0,0,-1],"radius":3.4,"height":22,"body_name":"Bore"}));
    run(&mut s, "solid.combine", json!({"target":"Block","tools":["Bore"],"operation":"cut"}));
    run(&mut s, "solid.thread", json!({"face":[3.4,0,10],"designation":"M8","modeled":true}));
    assert!(s.model.state().body("Block").unwrap().body.face_count() > 1000);
    run(&mut s, "sketch.create", json!({"plane":{"face":[15,15,20],"fixed":true}}));
    let plane = run(&mut s, "sketch.inspect", json!({}))["plane"].clone();
    assert!((plane["origin"][2].as_f64().unwrap() - 20.0).abs() < 1e-6, "{plane}");
}
/// A Mirror that combines with its source makes each face's twin from the same source face: a
/// sketch on the mirrored half sits on that half, not on its original (#5779).
#[test]
fn a_sketch_on_the_mirrored_half_of_a_combined_body_sits_on_that_half() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":30,"width":20,"height":10,"body_name":"Part","name":"Base"}));
    run(&mut s, "solid.mirror", json!({"bodies":["Part"],"plane":"XY","combine":true,"name":"Mirror1"}));
    assert_eq!(s.model.state().bodies.len(), 1, "the mirror combined");
    let pick = |s: &Session, normal: Vec3, point: Vec3| {
        let world = s.world_state();
        let body = world.body("Part").unwrap();
        let face = body
            .body
            .faces(0.01)
            .unwrap()
            .into_iter()
            .find(|f| f.plane_normal.is_some_and(|n| n.dot(normal) > 0.999) && (f.centroid - point).dot(normal).abs() < 1e-6)
            .unwrap_or_else(|| panic!("a face at {point:?}"));
        json!({"body":"Part","index":face.index,"face":[point.x,point.y,point.z]})
    };
    for (name, normal, point) in [
        ("Bottom", -Vec3::Z, Vec3::new(15., 10., -10.)),
        ("Top", Vec3::Z, Vec3::new(15., 10., 10.)),
        ("Front low", -Vec3::Y, Vec3::new(15., 0., -5.)),
        ("Front high", -Vec3::Y, Vec3::new(15., 0., 5.)),
    ] {
        let p = pick(&s, normal, point);
        let id = run(&mut s, "sketch.create", json!({"name":name,"plane":p,"project_edges":false}))["sketch"].as_u64().unwrap();
        run(&mut s, "sketch.finish", json!({}));
        let plane = s.model.state().sketch(id).unwrap().plane;
        assert!(plane.normal().dot(normal) > 0.999, "{name}: normal {:?}", plane.normal());
        assert!((plane.origin - point).dot(normal).abs() < 1e-6, "{name}: origin {:?}", plane.origin);
    }
}
