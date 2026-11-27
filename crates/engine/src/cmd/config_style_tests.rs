use crate::Session;
use serde_json::{Value, json};
use solidcraft_doc::config::{Column, ConfigFaceTarget};
use solidcraft_geom::prelude::*;
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
    let world = s.world_state().unwrap();
    assert_eq!(world.bodies.len(), 2);
    assert!(world.bodies.iter().all(|body| s.doc.model_body_look(body).unwrap().name == "ABS - Red"));
    run(&mut s, "appearance.assign", json!({"bodies":[body],"appearance":"ABS - Yellow"}));
    assert!(s.world_state().unwrap().bodies.iter().all(|body| s.doc.model_body_look(body).unwrap().name == "ABS - Yellow"));
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
    let history = s.undo.len();
    let preview = run(&mut s, "config.preview", json!({"row":"Tall"}));
    assert_eq!(s.doc, before);
    assert_eq!(s.revision, revision);
    assert_eq!(s.undo.len(), history);
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
    let history = s.undo.len();
    assert!(s.execute("config.activate", &json!({"row":"Paint"})).is_err());
    assert!(s.execute("config.preview", &json!({"row":"Paint"})).is_err());
    assert_eq!(s.doc, before);
    assert_eq!(s.revision, revision);
    assert_eq!(s.undo.len(), history);
    let look = solidcraft_doc::appearance::library("ABS - Red").unwrap();
    s.doc_mut().appearances.faces.push(solidcraft_doc::appearance::FaceLook {
        body: body.clone(),
        point: solidcore_record::Vec3::new(5., 5., 10.),
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
    let world = s.world_state().unwrap();
    let placed = world.bodies.iter().find(|body| body.name != body.definition_name()).unwrap();
    let placed_name = placed.name.clone();
    let point = solidcraft_doc::apply_point(&placed.source_transform, solidcore_record::Vec3::new(5., 5., 10.));
    let key = face_column(&mut s, &placed_name, json!(point));
    let Column::FaceAppearance(target) = &s.doc.configs.columns[0] else { panic!() };
    assert_eq!(target.body, body);
    assert!(target.point.dist(solidcore_record::Vec3::new(5., 5., 10.)) < 1e-8);
    run(&mut s, "config.row", json!({"name":"Paint","values":{key:"ABS - Blue"}}));
    run(&mut s, "config.activate", json!({"row":"Paint"}));
    assert!(s.world_state().unwrap().bodies.iter().all(|body| s.doc.face_colors(body).len() == 1));
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
        point: solidcore_record::Vec3::ZERO,
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

#[test]
fn configuration_face_indices_verifies_exact_planar_faces() {
    let mut s = Session::default();
    let body_name = block(&mut s);
    let state = s.model.state();
    let mb = state.body(&body_name).unwrap();
    let id = s.doc.features[0].id;
    let target = ConfigFaceTarget {
        body: body_name.clone(),
        origin_feature: id,
        name: format!("F{id}:+z"),
        point: solidcore_record::Vec3::new(5., 5., 10.),
        certificate: None,
    };
    let indices = s.doc.configuration_face_indices(mb, &target).unwrap();
    assert!(!indices.is_empty());
}

/// A catalogue row can choose between alternative features (a cube or a rod, as a generator's
/// part does for two profiles): the one it suppresses makes no body, the one it keeps takes the
/// body's name, and a later feature (a pattern) finds that body by the name in every row.
#[test]
fn a_row_can_pick_between_alternative_features_and_a_later_feature_still_finds_the_body_by_name() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Base"}));
    let (cube, cube_name) = (s.doc.features[0].id, s.doc.features[0].name.clone());
    run(&mut s, "timeline.suppress", json!({"feature": cube_name, "suppressed": true}));
    run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 30, "body_name": "Base"}));
    let (rod, rod_name) = (s.doc.features[1].id, s.doc.features[1].name.clone());
    run(&mut s, "solid.pattern.rectangular", json!({"bodies": ["Base"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 30}));
    run(&mut s, "config.configure", json!({"features": [cube_name, rod_name]}));
    let (cube_key, rod_key) = (format!("suppress:{cube}"), format!("suppress:{rod}"));
    run(&mut s, "config.row", json!({"name": "Cube", "values": {cube_key.clone(): false, rod_key.clone(): true}}));
    run(&mut s, "config.row", json!({"name": "Rod", "values": {cube_key: true, rod_key: false}}));
    let volume = |s: &mut Session| run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    for _ in 0..2 {
        run(&mut s, "config.activate", json!({"row": "Cube"}));
        assert!((volume(&mut s) - 2000.0).abs() < 1e-6, "{}", volume(&mut s));
        run(&mut s, "config.activate", json!({"row": "Rod"}));
        assert!((volume(&mut s) - 960.0).abs() < 1e-6, "{}", volume(&mut s));
    }
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "no feature fails in either row");
}

/// Tables of a few hundred rows (a linear-bushing catalogue has 480) work through save, open and
/// insertion; the table holds at most `MAX_ROWS` (2000) rows and says so when a row would pass it.
#[test]
fn a_catalogue_of_hundreds_of_rows_inserts_any_row_and_the_limit_is_stated() {
    let dir = std::env::temp_dir().join(format!("solidcraft-config-rows-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("bar.sc");
    let mut part = Session::default();
    run(&mut part, "parameters.add", json!({"name": "w", "expression": "10 mm"}));
    run(&mut part, "solid.box", json!({"length": 10, "width": "w", "height": 5, "body_name": "Bar"}));
    run(&mut part, "config.configure", json!({"params": ["w"]}));
    for i in 0..480 {
        part.doc_mut().add_configuration_row(format!("R{i}"), vec![format!("{} mm", 10 + i)]).unwrap();
    }
    run(&mut part, "file.save", json!({"path": file}));
    let mut s = Session::default();
    run(&mut s, "component.insert_design", json!({"path": file, "configuration": "R479"}));
    let volume = run(&mut s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    assert!((volume - 10.0 * 489.0 * 5.0).abs() < 1e-6, "{volume}");
    let mut full = Session::default();
    for i in 0..solidcraft_doc::config::MAX_ROWS {
        full.doc_mut().add_configuration_row(format!("R{i}"), Vec::new()).unwrap();
    }
    assert!(full.doc_mut().add_configuration_row("one too many".into(), Vec::new()).is_err());
}
