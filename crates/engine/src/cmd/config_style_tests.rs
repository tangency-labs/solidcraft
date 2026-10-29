use crate::Session;
use serde_json::{Value, json};
use solidcraft_doc::config::{Column, ConfigFaceTarget};
fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|error| panic!("{command} {params}: {error}"))
}
fn block(s: &mut Session) -> String {
    run(s, "solid.box", json!({"length":10,"width":10,"height":10}));
    s.model.state().bodies[0].name.clone()
}
fn face_column(s: &mut Session, body: &str, point: Value) -> String {
    run(s, "config.configure", json!({"face_appearances":[{"body":body,"point":point}]}))["columns"][0].as_str().unwrap().into()
}

#[test]
fn component_appearance_shared_definition_inheritance_and_delete() {
    let mut s = Session::default();
    let component = run(&mut s, "component.create", json!({"name":"Part"}))["component"].as_u64().unwrap();
    let body = block(&mut s);
    run(&mut s, "material.assign", json!({"bodies":[body],"material":"Steel"}));
    run(&mut s, "occurrence.copy", json!({"component":component,"translate":[30,0,0]}));
    let table = run(&mut s, "config.configure", json!({"component_appearances":[component]}));
    let key = table["columns"][0].as_str().unwrap().to_string();
    run(&mut s, "config.row", json!({"name":"Red","values":{key.clone():"ABS - Red"}}));
    run(&mut s, "config.activate", json!({"row":"Red"}));
    let world = s.world_state();
    assert_eq!(world.bodies.len(), 2);
    assert!(world.bodies.iter().all(|body| s.doc.model_body_look(body).unwrap().name == "ABS - Red"));
    run(&mut s, "appearance.assign", json!({"bodies":[body],"appearance":"ABS - Yellow"}));
    assert!(s.world_state().bodies.iter().all(|body| s.doc.model_body_look(body).unwrap().name == "ABS - Yellow"));
    run(&mut s, "config.activate", json!({"row":"Default"}));
    assert!(!s.doc.appearances.components.contains_key(&component));
    assert_eq!(s.doc.appearances.bodies[&body].name, "ABS - Yellow");
    run(&mut s, "component.rename", json!({"component":component,"name":"Renamed"}));
    assert_eq!(run(&mut s, "config.table", json!({}))["columns"][0], key);
    run(&mut s, "appearance.assign", json!({"components":[component.to_string()],"appearance":"ABS - Blue"}));
    run(&mut s, "component.delete", json!({"component":component}));
    assert!(s.doc.configs.columns.is_empty());
    assert!(s.doc.configs.rows.iter().all(|row| row.cells.is_empty()));
    assert!(!s.doc.appearances.components.contains_key(&component));
}

#[test]
fn strict_face_rows_resize_rename_preview_suppress_and_roundtrip() {
    let mut s = Session::default();
    let body = block(&mut s);
    let key = face_column(&mut s, &body, json!([5, 5, 10]));
    let feature = s.doc.features[0].name.clone();
    let height = s.doc.features[0].param_names[2].clone();
    run(&mut s, "config.column", json!({"param":height}));
    run(&mut s, "config.column", json!({"feature":feature}));
    run(&mut s, "config.row", json!({"name":"Tall","values":{key:"ABS - Red",height:"20 mm"}}));
    let before = s.doc.clone();
    let revision = s.revision;
    let history = s.history.len();
    let preview = run(&mut s, "config.preview", json!({"row":"Tall"}));
    assert_eq!(s.doc, before);
    assert_eq!(s.revision, revision);
    assert_eq!(s.history.len(), history);
    assert_eq!(preview["styles"][0]["dormant"], false);
    run(&mut s, "config.activate", json!({"row":"Tall"}));
    let state = s.model.state();
    let mb = state.body(&body).unwrap();
    let colors = s.doc.face_colors(mb);
    assert_eq!(colors.len(), 1);
    let names = solidcraft_doc::naming::face_names(mb);
    assert!(names[colors[0].0].ends_with(":+z"));
    assert!(s.doc.appearances.faces[0].strict_name);
    run(&mut s, "appearance.assign", json!({"faces":[{"body":body,"point":[5,5,20]}]}));
    assert!(s.doc.appearances.faces.is_empty(), "clear resolves semantic name after configured resize");
    run(&mut s, "config.activate", json!({"row":"Tall"}));
    run(&mut s, "body.rename", json!({"body":body,"name":"Housing"}));
    let key = s.doc.config_key(&s.doc.configs.columns[0]);
    let suppress = format!("suppress:{feature}");
    run(&mut s, "config.row", json!({"name":"Hidden","values":{key:"ABS - Blue",suppress:true}}));
    let hidden = run(&mut s, "config.activate", json!({"row":"Hidden"}));
    assert_eq!(hidden["styles"][0]["dormant"], true);
    assert!(s.model.state().bodies.is_empty());
    let reopened = solidcraft_doc::format::read(&s.doc.to_json()).unwrap();
    assert_eq!(reopened, *s.doc);
    run(&mut s, "config.activate", json!({"row":"Tall"}));
    assert_eq!(s.doc.face_colors(s.model.state().body("Housing").unwrap())[0].1.name, "ABS - Red");
    run(&mut s, "config.activate", json!({"row":"Default"}));
    assert!(s.doc.appearances.faces.is_empty());
    let before_delete = s.doc.clone();
    run(&mut s, "timeline.delete", json!({"feature":feature}));
    assert!(s.doc.configs.columns.is_empty());
    assert!(s.doc.configs.rows.iter().all(|row| row.cells.is_empty()));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc, before_delete);
    run(&mut s, "config.activate", json!({"row":"Tall"}));
    assert!(s.doc.appearances.faces[0].strict_name);
}

#[test]
fn strict_face_split_pieces_share_lineage_without_index_targets() {
    let mut s = Session::default();
    let body = block(&mut s);
    let key = face_column(&mut s, &body, json!([5, 5, 10]));
    run(&mut s, "config.row", json!({"name":"Paint","values":{key:"ABS - Red"}}));
    run(&mut s, "solid.split_face", json!({"body":body,"faces":[[5,5,10]],"plane":{"origin":[5,0,0],"normal":[1,0,0]}}));
    run(&mut s, "config.activate", json!({"row":"Paint"}));
    let state = s.model.state();
    let body = state.body(&body).unwrap();
    let colors = s.doc.face_colors(body);
    let names = solidcraft_doc::naming::face_names(body);
    assert!(colors.len() >= 2, "split should create multiple painted pieces: {names:?}");
    assert!(colors.iter().all(|(index, look)| solidcraft_doc::naming::strip_piece(&names[*index]).ends_with(":+z") && look.name == "ABS - Red"));
}

#[test]
fn strict_face_missing_lineage_never_retargets_and_rolls_back() {
    let mut s = Session::default();
    let body = block(&mut s);
    let key = face_column(&mut s, &body, json!([5, 5, 10]));
    run(&mut s, "config.row", json!({"name":"Paint","values":{key:"ABS - Red"}}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"corner":[30,0,0],"operation":"new"}));
    let other = s.doc.features.last().unwrap().id;
    let Column::FaceAppearance(target) = &mut s.doc_mut().configs.columns[0] else { panic!() };
    target.origin_feature = other;
    target.name = format!("F{other}:+z");
    let before = s.doc.clone();
    let revision = s.revision;
    let history = s.history.len();
    assert!(s.execute("config.activate", &json!({"row":"Paint"})).is_err());
    assert!(s.execute("config.preview", &json!({"row":"Paint"})).is_err());
    assert_eq!(s.doc, before);
    assert_eq!(s.revision, revision);
    assert_eq!(s.history.len(), history);
    let look = solidcraft_doc::appearance::library("ABS - Red").unwrap();
    s.doc_mut().appearances.faces.push(solidcraft_doc::appearance::FaceLook {
        body: body.clone(),
        point: solidcraft_geom::Vec3::new(5., 5., 10.),
        name: Some(format!("F{other}:+z")),
        strict_name: true,
        certificate: None,
        look,
    });
    assert!(s.doc.face_colors(s.model.state().body(&body).unwrap()).is_empty());
    s.doc_mut().appearances.faces[0].strict_name = false;
    assert_eq!(s.doc.face_colors(s.model.state().body(&body).unwrap()).len(), 1);
}

#[test]
fn face_occurrence_pick_uses_definition_identity_and_coordinates() {
    let mut s = Session::default();
    let component = run(&mut s, "component.create", json!({"name":"Part"}))["component"].as_u64().unwrap();
    let body = block(&mut s);
    run(&mut s, "occurrence.copy", json!({"component":component,"translate":[30,0,0]}));
    let world = s.world_state();
    let placed = world.bodies.iter().find(|body| body.name != body.definition_name()).unwrap();
    let placed_name = placed.name.clone();
    let point = solidcraft_doc::apply_point(&placed.source_transform, solidcraft_geom::Vec3::new(5., 5., 10.));
    let key = face_column(&mut s, &placed_name, json!(point));
    let Column::FaceAppearance(target) = &s.doc.configs.columns[0] else { panic!() };
    assert_eq!(target.body, body);
    assert!(target.point.dist(solidcraft_geom::Vec3::new(5., 5., 10.)) < 1e-8);
    run(&mut s, "config.row", json!({"name":"Paint","values":{key:"ABS - Blue"}}));
    run(&mut s, "config.activate", json!({"row":"Paint"}));
    assert!(s.world_state().bodies.iter().all(|body| s.doc.face_colors(body).len() == 1));
}

#[test]
fn extrude_cap_certification_follows_actual_positive_negative_and_symmetric_names() {
    for direction in ["positive", "negative", "symmetric"] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane":"xy"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance":10,"direction":direction,"body_name":"Pad"}));
        let state = s.model.state();
        let body = state.body("Pad").unwrap();
        let faces = body.body.faces(0.01).unwrap();
        let names = solidcraft_doc::naming::face_names(body);
        let mut caps = 0;
        for face in faces {
            let name = solidcraft_doc::naming::strip_piece(&names[face.index]);
            let origin = name.strip_prefix('F').unwrap().split_once(':').unwrap().0.parse().unwrap();
            let target = ConfigFaceTarget { body: "Pad".into(), origin_feature: origin, name: name.into(), point: face.centroid, certificate: None };
            if name.ends_with(":start") || name.ends_with(":top") || name.ends_with(":bottom") {
                s.doc.certify_config_face(&target).unwrap();
                s.doc.configuration_face_indices(body, &target).unwrap();
                caps += 1;
            } else {
                assert!(s.doc.certify_config_face(&target).is_err());
            }
        }
        assert_eq!(caps, 2, "{direction}");
    }
}

#[test]
fn ordinary_extent_edits_never_transfer_strict_cap_paint_to_ambiguous_caps() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"xy"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance":10,"body_name":"Pad","name":"PadFeature","start_offset":"0 mm"}));
    let key = face_column(&mut s, "Pad", json!([5, 5, 10]));
    run(&mut s, "config.row", json!({"name":"Paint","values":{key:"ABS - Red"}}));
    run(&mut s, "config.activate", json!({"row":"Paint"}));
    assert_eq!(s.doc.face_colors(s.model.state().body("Pad").unwrap()).len(), 1);
    run(&mut s, "timeline.redefine", json!({"feature":"PadFeature","command":"solid.extrude","params":{"distance":10,"start_offset":5}}));
    assert!(s.doc.face_colors(s.model.state().body("Pad").unwrap()).is_empty(), "ordinary offset edit must not paint both new top-named caps");
    let before = s.doc.clone();
    assert!(s.execute("config.activate", &json!({"row":"Paint"})).is_err());
    assert_eq!(s.doc, before);
}

#[test]
fn hostile_and_uncertified_face_targets_are_rejected() {
    let mut s = Session::default();
    let body = block(&mut s);
    let id = s.doc.features[0].id;
    let target = ConfigFaceTarget {
        body: body.clone(),
        origin_feature: id,
        name: format!("F{id}:face:0"),
        point: solidcraft_geom::Vec3::ZERO,
        certificate: None,
    };
    assert!(s.doc.certify_config_face(&target).is_err());
    for value in [
        json!({"body":body,"origin_feature":id,"name":"x".repeat(1_000_000),"point":[0,0,0]}),
        json!({"body":body,"origin_feature":id,"name":format!("F{id}:+z"),"point":[0,0,1e20]}),
        json!({"body":body,"origin_feature":id,"name":format!("F{id}:+z"),"point":[0,0,0],"extra":[[]]}),
    ] {
        assert!(solidcraft_doc::config::validate_styles_json(&json!({"configs":{"columns":[{"face_appearance":value}]}})).is_err());
    }
    assert!(s.execute("config.column", &json!({"component_appearance":0})).is_err());
    assert!(s.execute("config.column", &json!({"face_appearance":{"body":body,"point":[0,0,1e20]}})).is_err());
}
