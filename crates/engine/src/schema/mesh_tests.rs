use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_doc::{Document, FeatureKind, MeshData};

use super::{MeshRebuildParams, MeshRebuildResult, MeshRecognizeParams, MeshRecognizeResult, MeshReconstructParams, MeshReconstructResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<MeshRecognizeParams>(), &["body", "groups", "max_patches", "tolerance"]);
    assert_keys(
        input_schema_for::<MeshRebuildParams>(),
        &["align", "axis", "body", "groups", "name", "parameters", "plan_only", "prefix", "tolerance"],
    );
    assert_keys(
        input_schema_for::<MeshReconstructParams>(),
        &["axis", "body", "grid", "min_thickness", "name", "parameters", "plan_only", "prefix", "snap_angle", "symmetry", "tolerance"],
    );
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("mesh.recognize", json!({})),
        ("mesh.recognize", json!({"body": "Mesh"})),
        ("mesh.recognize", json!({"max_patches": 0})),
        ("mesh.recognize", json!({"max_patches": 5000})),
        ("mesh.rebuild", json!({})),
        ("mesh.rebuild", json!({"body": "Mesh"})),
        ("mesh.rebuild", json!({"body": "Mesh", "align": 90})),
        ("mesh.rebuild", json!({"body": "Mesh", "axis": [0, 0, 0]})),
        ("mesh.reconstruct", json!({})),
        ("mesh.reconstruct", json!({"body": "Mesh"})),
        ("mesh.reconstruct", json!({"body": "Mesh", "symmetry": "sideways"})),
        ("mesh.reconstruct", json!({"body": "Mesh", "axis": [0, 0, 0]})),
        ("mesh.reconstruct", json!({"body": "Mesh", "grid": "-1 mm"})),
    ]);
}

/// A session holding a box imported as a mesh.
fn imported_box() -> Session {
    let body = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10., 20., 30.)).unwrap();
    let mesh = body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.01)).unwrap();
    let mut doc = Document::default();
    doc.add_feature(
        FeatureKind::MeshImport {
            file: "box.stl".into(),
            meshes: vec![MeshData {
                name: "Mesh".into(),
                positions: mesh.positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect(),
                triangles: mesh.triangles.iter().flatten().copied().collect(),
                color: None,
            }],
        },
        Some("Imported mesh"),
    )
    .unwrap();
    Session { param_check: ParamCheck::Warn, ..Session::new(doc) }
}

#[test]
fn the_reply_matches_the_output_schema() {
    let mut s = imported_box();
    let reply = s.execute("mesh.recognize", &json!({"body": "Mesh"})).unwrap();
    assert!(reply.get("param_warnings").is_none(), "{reply}");
    let schema = output_schema_for::<MeshRecognizeResult>();
    let schema = schema.as_value();
    assert!(conforms(schema, schema, &reply), "{reply} doesn't match {schema}");
}

#[test]
fn a_rebuild_reply_matches_its_output_schema() {
    let mut s = imported_box();
    let reply = s.execute("mesh.rebuild", &json!({"body": "Mesh"})).unwrap();
    assert!(reply.get("param_warnings").is_none(), "{reply}");
    let schema = output_schema_for::<MeshRebuildResult>();
    let schema = schema.as_value();
    assert!(conforms(schema, schema, &reply), "{reply} doesn't match {schema}");
}

#[test]
fn a_reconstruct_reply_matches_its_output_schema() {
    let mut s = imported_box();
    let reply = s.execute("mesh.reconstruct", &json!({"body": "Mesh"})).unwrap();
    assert!(reply.get("param_warnings").is_none(), "{reply}");
    let schema = output_schema_for::<MeshReconstructResult>();
    let schema = schema.as_value();
    assert!(conforms(schema, schema, &reply), "{reply} doesn't match {schema}");
}

#[test]
fn a_reconstruct_builds_the_part_again_as_ordinary_features() {
    let mut s = imported_box();
    let reply = s.execute("mesh.reconstruct", &json!({"body": "Mesh", "grid": "0.5 mm"})).unwrap();
    // One layer, built as a sketch and an extrusion, with the mesh body left where it was.
    let slabs = reply.get("slabs").and_then(|v| v.as_array()).expect("slabs");
    assert_eq!(slabs.len(), 1, "{reply}");
    assert!(reply.get("features").and_then(|v| v.as_array()).is_some_and(|f| !f.is_empty()), "{reply}");
    let size = reply.get("size").and_then(|v| v.as_array()).expect("size");
    let mut read: Vec<f64> = size.iter().filter_map(serde_json::Value::as_f64).collect();
    read.sort_by(f64::total_cmp);
    // A 10 by 20 by 30 box, whichever way round the frame picked it up.
    assert!(read.iter().zip([10., 20., 30.]).all(|(got, want)| (got - want).abs() < 0.2), "{read:?}");
    let state = s.model.state();
    assert!(state.body("Mesh rebuilt").is_some(), "the rebuilt solid should be there");
    assert!(state.body("Mesh").is_some(), "the mesh body should be left alone");
}

#[test]
fn a_reconstruct_plan_only_changes_nothing() {
    let mut s = imported_box();
    let reply = s.execute("mesh.reconstruct", &json!({"body": "Mesh", "plan_only": true})).unwrap();
    assert!(reply.get("features").and_then(|v| v.as_array()).is_some_and(|f| f.is_empty()), "{reply}");
    assert!(reply.get("steps").and_then(|v| v.as_array()).is_some_and(|s| !s.is_empty()), "{reply}");
    assert!(s.model.state().body("Mesh rebuilt").is_none(), "plan_only should build nothing");
}
