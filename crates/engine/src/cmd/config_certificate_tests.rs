use crate::Session;
use serde_json::{Value, json};
use solidcraft_doc::{
    FeatureKind,
    config::{Column, FaceCertificate},
};
use solidcraft_geom::Vec3;

fn run(session: &mut Session, command: &str, params: Value) -> Value {
    session.execute(command, &params).unwrap_or_else(|error| panic!("{command} {params}: {error}"))
}
fn paint(session: &mut Session, body: &str, point: Value) {
    let table = run(session, "config.configure", json!({"face_appearances":[{"body":body,"point":point}]}));
    let key = table["columns"][0].as_str().unwrap();
    run(session, "config.row", json!({"name":"Painted","values":{key:"ABS - Red"}}));
    run(session, "config.activate", json!({"row":"Painted"}));
}
fn painted_radius(session: &Session, name: &str, radius: f64) -> usize {
    let state = session.model.state();
    let body =
        state.body(name).unwrap_or_else(|| panic!("missing {name}; bodies {:?}", state.bodies.iter().map(|body| &body.name).collect::<Vec<_>>()));
    let painted = session.doc.face_colors(body);
    let mesh = body.body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.001)).unwrap();
    let inverse = solidcraft_doc::mat_inverse(&body.source_transform).unwrap();
    for (triangle, face) in mesh.triangles.iter().zip(&mesh.tri_face) {
        if painted.iter().any(|(index, _)| *index == *face as usize) {
            for vertex in mesh.tri(triangle).unwrap() {
                let vertex = solidcraft_doc::apply_point(&inverse, vertex);
                assert!((vertex.x.hypot(vertex.y) - radius).abs() < 1e-7, "{name}: {vertex:?}");
            }
        }
    }
    assert!(!painted.is_empty());
    painted.len()
}

#[test]
fn certified_annulus_side_rows_follow_radius_edits_and_clone() {
    let mut s = Session::default();
    let part = run(&mut s, "component.create", json!({"name":"RingPart"}))["component"].as_u64().unwrap();
    let sketch = run(&mut s, "sketch.create", json!({"plane":"XY"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":10,"id":"outer"}));
    run(&mut s, "sketch.dimension", json!({"entities":["outer"],"type":"radius","value":10}));
    run(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":9,"id":"inner"}));
    run(&mut s, "sketch.dimension", json!({"entities":["inner"],"type":"radius","value":9}));
    let FeatureKind::Sketch { sketch: source_sketch, .. } = &s.doc.feature(sketch).unwrap().kind else { panic!() };
    let dimensions: Vec<_> = source_sketch.constraints.iter().filter_map(|dimension| dimension.param.clone()).collect();
    assert_eq!(dimensions.len(), 2);
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance":10,"profiles":[{"point":[9.5,0]}],"body_name":"Ring"}));
    paint(&mut s, "Ring", json!([10, 0, 5]));
    assert_eq!(painted_radius(&s, "Ring", 10.), 2, "both constructed arc pieces share outer curve");
    let Column::FaceAppearance(target) = &s.doc.configs.columns[0] else { panic!() };
    assert_eq!(target.certificate, Some(FaceCertificate::ExtrudeSide { sketch, curve: "outer".into() }));
    run(&mut s, "parameters.change", json!({"name":dimensions[0],"expression":"12 mm"}));
    run(&mut s, "parameters.change", json!({"name":dimensions[1],"expression":"9.25 mm"}));
    run(&mut s, "config.activate", json!({"row":"Painted"}));
    assert_eq!(painted_radius(&s, "Ring", 12.), 2);
    run(&mut s, "component.paste_new", json!({"component":part,"translate":[30,0,0]}));
    assert_eq!(s.doc.configs.columns.len(), 2);
    run(&mut s, "config.activate", json!({"row":"Painted"}));
    assert_eq!(painted_radius(&s, "Ring (2)", 12.), 2);
    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    let mut reopened = Session::new(reopened);
    run(&mut reopened, "config.activate", json!({"row":"Painted"}));
    assert_eq!(painted_radius(&reopened, "Ring", 12.), 2);
    assert_eq!(painted_radius(&reopened, "Ring (2)", 12.), 2);
    run(&mut reopened, "config.activate", json!({"row":"Default"}));
    assert!(reopened.doc.appearances.faces.is_empty());
}

fn imported(session: &mut Session, step: String, name: &str) -> u64 {
    let owner = session.active_component;
    let id = session
        .doc_mut()
        .add_feature(FeatureKind::Import { file: "fixture.step".into(), step: step.into(), components: vec![], skip: Vec::new() }, Some(name))
        .unwrap();
    let feature = session.doc_mut().feature_mut(id).unwrap();
    feature.body_names = vec![name.into()];
    feature.component = owner;
    session.refresh();
    assert!(session.model.result(id).unwrap().error.is_none());
    id
}

#[test]
fn legacy_v9_imported_emboss_anchor_and_ordinary_face_paint_replay_without_renaming() {
    let mut s = Session::default();
    let cylinder = solidcraft_kernel::cylinder(Vec3::ZERO, Vec3::Z, 10., 20.).unwrap();
    let step = solidcraft_kernel::step_export(&[&cylinder], "legacy anchored cylinder").unwrap();
    let source = imported(&mut s, step, "Sleeve");
    run(&mut s, "material.assign", json!({"bodies":["Sleeve"],"material":"Steel"}));
    let before_volume = s.model.state().body("Sleeve").unwrap().body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-4)).unwrap().measure().volume;
    let legacy_names = solidcraft_doc::naming::face_names(s.model.state().body("Sleeve").unwrap());
    assert!(legacy_names.iter().all(|name| name.starts_with(&format!("F{source}:face:"))));
    run(&mut s, "appearance.assign", json!({"faces":[{"body":"Sleeve","point":[6,0,20]}],"appearance":"ABS - Blue"}));
    assert!(s.doc.appearances.faces[0].certificate.is_none());
    let badge = run(&mut s, "sketch.create", json!({"plane":{"face":[0,0,20]},"name":"Badge","project_edges":false}))["sketch"].as_u64().unwrap();
    let FeatureKind::Sketch { plane: saved_plane, .. } = &s.doc.feature(badge).unwrap().kind else { panic!() };
    let saved_plane = saved_plane.clone();
    assert!(matches!(&saved_plane, solidcraft_doc::PlaneRef::Face { name: Some(name), .. } if legacy_names.contains(name)));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-2,-2],"p1":[2,2]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.emboss", json!({"sketch":"Badge","profiles":"all","depth":1,"targets":["Sleeve"],"name":"Raised Badge"}));
    let anchored = s.doc.find_feature("Raised Badge").unwrap().clone();
    assert!(matches!(anchored.kind, FeatureKind::Emboss { face: None, .. }));
    let volume = s.model.state().body("Sleeve").unwrap().body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-4)).unwrap().measure().volume;
    assert!((volume - std::f64::consts::PI * 2000. - 16.).abs() < 0.1, "volume {volume}");
    assert!((volume - before_volume - 16.).abs() < 0.01, "addedvolume {} baseline {before_volume}", volume - before_volume);
    let paint = s.doc.appearances.faces.clone();
    let mut legacy: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    legacy["format"] = json!("solidcraft/9");
    let reopened = Session::new(solidcraft_doc::Document::from_json(&legacy.to_string()).unwrap());
    // Saved before format 14, it keeps naming the pieces it makes in the kernel's shell order,
    // and numbering the pieces of its faces as that build did (#3902).
    let anchored = solidcraft_doc::Feature { shell_piece_order: true, kernel_piece_order: true, ..anchored };
    assert_eq!(reopened.doc.feature(anchored.id).unwrap(), &anchored);
    // Names saved before format 14 keep naming the face they named, marked so they are found as
    // the build that saved them found them.
    let mark = |name: &Option<String>| name.as_ref().map(|n| format!("{}{n}", solidcraft_doc::naming::LEGACY_MARK));
    let mut saved_plane = saved_plane;
    if let solidcraft_doc::PlaneRef::Face { name, .. } = &mut saved_plane {
        *name = mark(name);
    }
    let paint: Vec<_> = paint
        .into_iter()
        .map(|mut face| {
            if !face.strict_name {
                face.name = mark(&face.name);
            }
            face
        })
        .collect();
    let FeatureKind::Sketch { plane, .. } = &reopened.doc.feature(badge).unwrap().kind else { panic!() };
    assert_eq!(plane, &saved_plane);
    assert_eq!(reopened.doc.appearances.faces, paint);
    assert_eq!(reopened.doc.materials.get("Sleeve"), Some(&"Steel".to_string()));
    assert!(reopened.model.result(anchored.id).unwrap().error.is_none());
    let state = reopened.model.state();
    let body = state.body("Sleeve").unwrap();
    assert!((body.body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-4)).unwrap().measure().volume - volume).abs() < 1e-8);
    assert!(!reopened.doc.face_colors(body).is_empty());
}

#[test]
fn certified_side_refuses_reused_body_name_from_a_new_producer() {
    let mut s = Session::default();
    let sketch = run(&mut s, "sketch.create", json!({"plane":"XY"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":10,"id":"circle"}));
    run(&mut s, "sketch.finish", json!({}));
    let source = run(&mut s, "solid.extrude", json!({"distance":10,"body_name":"Round"}))["feature"].as_u64().unwrap();
    paint(&mut s, "Round", json!([10, 0, 5]));
    let Column::FaceAppearance(target) = s.doc.configs.columns[0].clone() else { panic!() };
    let mut replacement = s.doc.feature(source).unwrap().clone();
    replacement.id = s.doc.next_id;
    assert!(matches!(replacement.kind, FeatureKind::Extrude { sketch: original, .. } if original == sketch));
    s.doc_mut().feature_mut(source).unwrap().suppressed = true;
    s.doc_mut().next_id += 1;
    s.doc_mut().features.push(replacement);
    s.refresh();
    let state = s.model.state();
    let body = state.body("Round").unwrap();
    assert!(s.doc.configuration_face_indices(body, &target).is_err());
    assert!(s.doc.face_colors(body).is_empty());
    let before = s.doc.clone();
    let history = s.undo.len();
    assert!(s.execute("config.activate", &json!({"row":"Painted"})).is_err());
    assert_eq!(s.doc, before);
    assert_eq!(s.undo.len(), history);
}

#[test]
fn legacy_v9_annulus_outer_side_emboss_keeps_its_original_heuristic_anchor() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":10,"id":"outer"}));
    run(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":9,"id":"inner"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance":20,"profiles":[{"point":[9.5,0]}],"body_name":"Sleeve"}));
    let names = solidcraft_doc::naming::face_names(s.model.state().body("Sleeve").unwrap());
    // The outer wall is named after its own sketch curve: its history says so. (Builds before
    // format 14 named it after the curve nearest its centre, the inner circle; see below.)
    let name = solidcraft_doc::naming::face_names_at(&s.model.state(), &[Vec3::new(0., 10., 7.5)]).remove(0);
    assert!(name.contains("side:outer"));
    run(&mut s, "sketch.create", json!({"plane":{"origin":[0,10,0],"x_dir":[-1,0,0],"y_dir":[0,0,1]},"name":"Badge"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-2,5],"p1":[2,10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.emboss", json!({"sketch":"Badge","profiles":"all","face":[0,10,7.5],"depth":1,"targets":["Sleeve"],"name":"Raised Badge"}));
    let feature = s.doc.find_feature("Raised Badge").unwrap();
    assert_eq!(feature.face_names, vec![name]);
    assert!(feature.face_names.iter().all(|name| names.contains(name)));
    let feature_id = feature.id;
    let volume = s.model.state().body("Sleeve").unwrap().body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-4)).unwrap().measure().volume;
    assert!((volume - std::f64::consts::PI * 380. - 21.).abs() < 0.2, "volume {volume}");
    let mut legacy: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    legacy["format"] = json!("solidcraft/9");
    // What a version 9 design stored for that pick: the centroid-family name of the outer wall
    // piece holding the point. It keeps finding that piece, as it did when saved.
    for feature in legacy["features"].as_array_mut().unwrap() {
        if feature["id"] == json!(feature_id) {
            feature["face_names"] = json!(["F2:side:inner#4"]);
        }
    }
    let reopened = Session::new(solidcraft_doc::Document::from_json(&legacy.to_string()).unwrap());
    assert!(reopened.model.result(feature_id).unwrap().error.is_none());
    assert!((reopened.model.state().body("Sleeve").unwrap().body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-4)).unwrap().measure().volume - volume).abs() < 1e-8);
}

#[test]
fn certified_side_transfer_preserves_geometry_and_unsupported_taper_refuses_atomically() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":10}));
    run(&mut s, "sketch.finish", json!({}));
    let source = run(&mut s, "solid.extrude", json!({"distance":10,"body_name":"Round"}))["feature"].as_u64().unwrap();
    paint(&mut s, "Round", json!([10, 0, 5]));
    let created = run(&mut s, "component.create", json!({"name":"Placed","grounded":false}));
    let destination = created["component"].as_u64().unwrap();
    let occurrence = created["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":occurrence,"translate":[30,20,0],"angle":90,"axis":[0,0,1],"capture":false}));
    run(&mut s, "component.move_bodies", json!({"component":destination,"bodies":["Round"]}));
    run(&mut s, "config.activate", json!({"row":"Painted"}));
    assert_eq!(painted_radius(&s, "Round", 10.), 2);
    let before = s.world_state().body("Round").unwrap().mesh().bounds();
    let mut s = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    run(&mut s, "config.activate", json!({"row":"Painted"}));
    assert_eq!(painted_radius(&s, "Round", 10.), 2);
    assert!(
        s.world_state().body("Round").unwrap().mesh().bounds().min.dist(before.min) < 1e-8,
        "before {before:?}, reopened {:?}",
        s.world_state().body("Round").unwrap().mesh().bounds()
    );
    // A successful ordinary modifier has no exact untapered source certificate.
    let FeatureKind::Extrude { extent, .. } = &mut s.doc_mut().feature_mut(source).unwrap().kind else { panic!() };
    extent.taper = Some("5 deg".into());
    s.refresh();
    assert!(s.model.result(source).unwrap().error.is_none());
    assert!(s.doc.face_colors(s.model.state().body("Round").unwrap()).is_empty());
    let document = s.doc.clone();
    let history = s.undo.len();
    assert!(s.execute("config.preview", &json!({"row":"Painted"})).is_err());
    assert!(s.execute("config.activate", &json!({"row":"Painted"})).is_err());
    assert_eq!(s.doc, document);
    assert_eq!(s.undo.len(), history);
}

#[test]
fn certified_import_payload_binding_roundtrip_clone_and_replacement_refusal() {
    let geometry = solidcraft_kernel::cylinder(Vec3::ZERO, Vec3::Z, 10., 10.).unwrap();
    let step = solidcraft_kernel::step_export(&[&geometry], "certificate fixture").unwrap();
    let mut s = Session::default();
    let part = run(&mut s, "component.create", json!({"name":"ImportedPart"}))["component"].as_u64().unwrap();
    let source = imported(&mut s, step.clone(), "Imported");
    paint(&mut s, "Imported", json!([10, 0, 5]));
    let pieces = painted_radius(&s, "Imported", 10.);
    let Column::FaceAppearance(target) = &s.doc.configs.columns[0] else { panic!() };
    assert!(
        matches!(&target.certificate, Some(FaceCertificate::Imported { item, face, payload_sha256 }) if *item != 0 && *face != 0 && payload_sha256.len() == 64)
    );
    run(&mut s, "component.paste_new", json!({"component":part,"translate":[30,0,0]}));
    run(&mut s, "config.activate", json!({"row":"Painted"}));
    assert_eq!(painted_radius(&s, "Imported (2)", 10.), pieces);
    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    let mut s = Session::new(reopened);
    run(&mut s, "config.activate", json!({"row":"Painted"}));
    assert_eq!(painted_radius(&s, "Imported", 10.), pieces);
    // Identical geometry, entity IDs and item IDs, but a replaced embedded payload.
    let FeatureKind::Import { step: payload, .. } = &mut s.doc_mut().feature_mut(source).unwrap().kind else { panic!() };
    *payload = step.replace("certificate fixture", "replacement fixture").into();
    assert_ne!(&**payload, step.as_str());
    s.refresh();
    assert!(s.doc.face_colors(s.model.state().body("Imported").unwrap()).is_empty());
    assert_eq!(painted_radius(&s, "Imported (2)", 10.), pieces);
    let before = s.doc.clone();
    let history = s.undo.len();
    assert!(s.execute("config.preview", &json!({"row":"Painted"})).is_err());
    assert!(s.execute("config.activate", &json!({"row":"Painted"})).is_err());
    assert_eq!(s.doc, before);
    assert_eq!(s.undo.len(), history);
}
