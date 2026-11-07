#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use solidcraft_doc::{Document, FeatureKind, MeshData};
use solidcraft_engine::Session;
use solidcraft_kernel::{Body, BoolOp, boolean, box_solid, extrude_planar_regions};

struct EngineHost {
    session: Session,
    calls: Vec<String>,
    writes: Vec<Value>,
}
impl Host for EngineHost {
    fn run(&mut self, command: &str, params: Value) -> Result<Value, String> {
        self.calls.push(command.into());
        match command {
            "model.mesh" => {
                let state = self.session.model.state();
                let body = state.body(params["body"].as_str().unwrap()).ok_or("unknown body")?;
                let mesh = body.body.tessellate(params["tolerance"].as_f64().unwrap()).map_err(|e| e.to_string())?;
                Ok(json!({"body":params["body"],"component":body.component,"frame":"component-local-mm",
                    "positions":mesh.positions.iter().map(|p| [p.x,p.y,p.z]).collect::<Vec<_>>(),"triangles":mesh.triangles}))
            }
            "timeline.add" => {
                self.writes.push(params.clone());
                self.session.execute(command, &params).map_err(|e| e.to_string())
            }
            _ => Err("ungranted command".into()),
        }
    }
}
fn host(source: &Body, component: bool) -> EngineHost {
    let mesh = source.tessellate(0.005).unwrap();
    let mut doc = Document::default();
    let owner = if component { doc.add_component(Some("Part"), 0).unwrap() } else { 0 };
    doc.add_feature(
        FeatureKind::MeshImport {
            file: "synthetic.stl".into(),
            meshes: vec![MeshData {
                name: "Source".into(),
                positions: mesh.positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect(),
                triangles: mesh.triangles.iter().flatten().copied().collect(),
                color: None,
            }],
        },
        Some("Synthetic source"),
    )
    .unwrap();
    let f = doc.features.last_mut().unwrap();
    f.body_names = vec!["Source".into()];
    f.component = owner;
    EngineHost { session: Session::new(doc), calls: vec![], writes: vec![] }
}
fn box_host() -> EngineHost {
    host(&box_solid(Vec3::ZERO, Vec3::new(10., 20., 30.)).unwrap(), false)
}
fn volume(host: &EngineHost, name: &str) -> f64 {
    solidcraft_kernel::measure(&host.session.model.state().body(name).unwrap().body).unwrap().volume
}

#[test]
fn reconstruction_is_embedded_as_native_step_and_needs_no_plugin_to_reopen() {
    let mut h = box_host();
    execute(&mut h, "convert", json!({"body":"Source"})).unwrap();
    assert_eq!(h.calls, vec!["model.mesh", "timeline.add", "timeline.add"]);
    assert!(h.session.model.results.iter().all(|r| r.error.is_none()));
    assert!((volume(&h, "Source Solid") - 6000.).abs() < 1e-6);
    assert!(h.session.model.state().body("Source").is_none());
    assert!(matches!(h.session.doc.features[0].kind, FeatureKind::MeshImport { .. }));
    assert!(!h.session.model.state().body("Source Solid").unwrap().body.is_mesh());
    let saved = h.session.doc.to_json();
    assert!(!saved.contains("plugin.wasm"));
    let cold = EngineHost { session: Session::new(Document::from_json(&saved).unwrap()), calls: vec![], writes: vec![] };
    assert!((volume(&cold, "Source Solid") - 6000.).abs() < 1e-6);
    assert!(cold.session.model.results.iter().all(|r| r.error.is_none()));
}

#[test]
fn native_replacement_sequence_preserves_input_history_across_undo_and_redo() {
    let mut h = box_host();
    let before = h.session.doc.to_json();
    execute(&mut h, "convert", json!({"body":"Source"})).unwrap();
    let after = h.session.doc.to_json();
    h.session.execute("edit.undo", &json!({})).unwrap();
    h.session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(h.session.doc.to_json(), before);
    h.session.execute("edit.redo", &json!({})).unwrap();
    h.session.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(h.session.doc.to_json(), after);
}

#[test]
fn reconstruction_stays_in_the_source_component() {
    let mut h = host(&box_solid(Vec3::ZERO, Vec3::new(10., 20., 30.)).unwrap(), true);
    let owner = h.session.model.state().body("Source").unwrap().component;
    assert_ne!(owner, 0);
    execute(&mut h, "convert", json!({"body":"Source"})).unwrap();
    assert_eq!(h.session.model.state().body("Source Solid").unwrap().component, owner);
    assert!(h.session.model.results.iter().all(|r| r.error.is_none()));
}

#[test]
fn a_reconstructed_planar_face_accepts_a_durable_sketch_after_reopen() {
    let mut h = box_host();
    execute(&mut h, "convert", json!({"body":"Source"})).unwrap();
    let (index, point) = {
        let state = h.session.model.state();
        let body = state.body("Source Solid").unwrap();
        let face = body.body.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.x < -0.99)).unwrap();
        (face.index, solidcraft_doc::naming::face_spot(body, face.index).unwrap().point)
    };
    h.session.execute("sketch.create", &json!({"plane":{"body":"Source Solid","face":point,"index":index},"name":"Attached"})).unwrap();
    h.session.execute("sketch.finish", &json!({})).unwrap();
    let f = h.session.doc.find_feature("Attached").unwrap();
    assert!(matches!(f.kind, FeatureKind::Sketch { plane: solidcraft_doc::PlaneRef::Face { anchor: Some(_), .. }, .. }));
    let cold = Session::new(Document::from_json(&h.session.doc.to_json()).unwrap());
    assert!(cold.model.results.iter().all(|r| r.error.is_none()));
}

#[test]
fn changing_recorded_reconstruction_identity_cannot_reuse_an_old_face_ordinal() {
    let mut h = box_host();
    execute(&mut h, "convert", json!({"body":"Source","tolerance":0.01})).unwrap();
    let (index, point) = {
        let state = h.session.model.state();
        let body = state.body("Source Solid").unwrap();
        let face = body.body.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.x < -0.99)).unwrap();
        (face.index, solidcraft_doc::naming::face_spot(body, face.index).unwrap().point)
    };
    h.session.execute("sketch.create", &json!({"plane":{"body":"Source Solid","face":point,"index":index},"name":"Attached"})).unwrap();
    h.session.execute("sketch.finish", &json!({})).unwrap();
    let attached = h.session.doc.features.iter().position(|f| f.name == "Attached").unwrap();
    let mut other = box_host();
    execute(&mut other, "convert", json!({"body":"Source","tolerance":0.02})).unwrap();
    let replacement = other.writes[0]["feature"]["step"].as_str().unwrap();
    let mut doc = h.session.doc.as_ref().clone();
    let import = doc.features.iter_mut().find(|f| matches!(f.kind, FeatureKind::Import { .. })).unwrap();
    if let FeatureKind::Import { step, .. } = &mut import.kind {
        *step = replacement.into();
    }
    // Same feature and face ordinal, but a different tolerance certificate: reject
    // the old attachment after a cache-free read instead of silently retargeting it.
    let cold = Session::new(Document::from_json(&doc.to_json()).unwrap());
    assert!(cold.model.results[attached].error.is_some());
}

fn bracket() -> Body {
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2};
    let base = box_solid(Vec3::ZERO, Vec3::new(20., 16., 2.)).unwrap();
    let wall = box_solid(Vec3::new(4., 0., 2.), Vec3::new(8., 16., 12.)).unwrap();
    let mut body = boolean(&base, &wall, BoolOp::Union).unwrap().unwrap();
    for (plane, lo, hi, center) in [(Plane::XY, 0., 2., Vec2::new(14., 8.)), (Plane::YZ, 4., 8., Vec2::new(8., 8.))] {
        let tool = extrude_planar_regions(&plane, &[Region2 { outer: Loop2::circle(center, 2.), holes: vec![] }], lo, hi).unwrap();
        body = boolean(&body, &tool, BoolOp::Cut).unwrap().unwrap();
    }
    body
}
#[test]
fn the_pr8061_bracket_with_perpendicular_bores_stays_analytic() {
    let mut h = host(&bracket(), false);
    execute(&mut h, "convert", json!({"body":"Source"})).unwrap();
    assert!((volume(&h, "Source Solid") - (1280. - 24. * std::f64::consts::PI)).abs() < 0.03);
    let state = h.session.model.state();
    let b = &state.body("Source Solid").unwrap().body;
    assert!(b.validity().is_empty());
    assert_eq!(solidcraft_kernel::measure(b).unwrap().merged.face_types.get("cylinder"), Some(&2));
}

#[test]
fn fingerprints_change_with_reconstruction_inputs() {
    let mut a = box_host();
    let a = execute(&mut a, "convert", json!({"body":"Source","tolerance":0.01})).unwrap();
    let mut b = box_host();
    let b = execute(&mut b, "convert", json!({"body":"Source","tolerance":0.02})).unwrap();
    let mut c = host(&box_solid(Vec3::ZERO, Vec3::new(11., 20., 30.)).unwrap(), false);
    let c = execute(&mut c, "convert", json!({"body":"Source","tolerance":0.01})).unwrap();
    assert_ne!(a["source_fingerprint"], b["source_fingerprint"]);
    assert_ne!(a["source_fingerprint"], c["source_fingerprint"]);
    assert_eq!(a["algorithm"], ALGORITHM);
    assert_eq!(a["sdk_revision"], SDK_REV);
    assert_eq!(a["face_order_version"], FACE_ORDER_VERSION);
}

#[test]
fn hostile_parameters_are_refused_before_any_host_call() {
    for p in [
        json!({}),
        json!({"body":""}),
        json!({"body":"Source","tolerance":0}),
        json!({"body":"Source","tolerance":2}),
        json!({"body":"Source","picks":[]}),
        json!({"body":"Source","all_surfaces":true}),
        json!({"body":"Source","name":"bad\nname"}),
        json!({"body":"Source","name":"Source"}),
        json!({"body":12}),
    ] {
        let mut h = box_host();
        let before = h.session.doc.to_json();
        assert!(execute(&mut h, "convert", p).is_err());
        assert!(h.calls.is_empty());
        assert_eq!(h.session.doc.to_json(), before);
    }
    let mut h = box_host();
    assert!(execute(&mut h, "other", json!({"body":"Source"})).is_err());
    assert!(h.calls.is_empty());
}

#[test]
fn unsupported_closed_mesh_does_not_issue_a_write() {
    let mesh = solidcraft_kernel::mesh_body(
        &[Vec3::ZERO, Vec3::new(8., 0., 0.), Vec3::new(0., 8., 0.), Vec3::new(0., 0., 8.)],
        &[[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
    )
    .unwrap();
    let mut h = host(&mesh, false);
    let before = h.session.doc.to_json();
    assert!(execute(&mut h, "convert", json!({"body":"Source"})).is_err());
    assert!(h.writes.is_empty());
    assert_eq!(h.session.doc.to_json(), before);
}

struct ReplyHost {
    reply: Value,
    writes: usize,
}
impl Host for ReplyHost {
    fn run(&mut self, command: &str, _: Value) -> Result<Value, String> {
        if command == "model.mesh" {
            Ok(self.reply.clone())
        } else {
            self.writes += 1;
            Err("write should never occur".into())
        }
    }
}
#[test]
fn malformed_meshes_and_wrong_frames_are_refused_without_writing() {
    let base = json!({"body":"Source","component":0,"frame":"component-local-mm","positions":[[0,0,0],[1,0,0],[0,1,0]],"triangles":[[0,1,2]]});
    for (key, value) in [
        ("frame", json!("world-mm")),
        ("body", json!("Other")),
        ("positions", json!([[null, 0, 0]])),
        ("triangles", json!([[0, 1, 9999]])),
        ("positions", json!([[100001, 0, 0], [0, 1, 0], [0, 0, 1]])),
        ("triangles", json!([])),
    ] {
        let mut reply = base.clone();
        reply[key] = value;
        let mut h = ReplyHost { reply, writes: 0 };
        assert!(execute(&mut h, "convert", json!({"body":"Source"})).is_err());
        assert_eq!(h.writes, 0);
    }
}

#[test]
fn selecting_an_identical_body_in_the_same_component_changes_recorded_identity() {
    let mut h = box_host();
    let mut doc = h.session.doc.as_ref().clone();
    let FeatureKind::MeshImport { mut meshes, .. } = doc.features[0].kind.clone() else {
        panic!("synthetic source must be a mesh import");
    };
    assert_eq!(meshes.len(), 1);
    meshes[0].name = "Second".into();
    doc.add_feature(FeatureKind::MeshImport { file: "synthetic-duplicate.stl".into(), meshes }, Some("Second synthetic source")).unwrap();
    let f = doc.features.last_mut().unwrap();
    f.body_names = vec!["Second".into()];
    f.component = 0;
    h.session = Session::new(doc);
    let before_a = h.run("model.mesh", json!({"body":"Source","tolerance":0.05})).unwrap();
    let before_b = h.run("model.mesh", json!({"body":"Second","tolerance":0.05})).unwrap();
    assert_eq!(before_a["component"], before_b["component"]);
    assert_eq!(before_a["positions"], before_b["positions"]);
    assert_eq!(before_a["triangles"], before_b["triangles"]);
    let a = execute(&mut h, "convert", json!({"body":"Source","name":"First solid"})).unwrap();
    let b = execute(&mut h, "convert", json!({"body":"Second","name":"Second solid"})).unwrap();
    assert_ne!(a["source_fingerprint"], b["source_fingerprint"]);
    assert_ne!(h.writes[0]["feature"]["file"], h.writes[2]["feature"]["file"]);
    assert!((volume(&h, "First solid") - 6000.).abs() < 1e-6);
    assert!((volume(&h, "Second solid") - 6000.).abs() < 1e-6);
}
