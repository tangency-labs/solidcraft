use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_doc::{Document, FeatureKind, MeshData};

use super::{
    MeshLoftParams, MeshLoftResult, MeshRebuildParams, MeshRebuildResult, MeshRecognizeParams, MeshRecognizeResult, MeshReconstructParams,
    MeshReconstructResult,
};
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
    assert_keys(input_schema_for::<MeshLoftParams>(), &["axes", "body", "hull", "margin", "name", "plan_only", "points", "sections", "spacing"]);
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
        ("mesh.loft", json!({})),
        ("mesh.loft", json!({"body": "Mesh"})),
        ("mesh.loft", json!({"body": "Mesh", "axes": ["sideways"]})),
        ("mesh.loft", json!({"body": "Mesh", "sections": 500})),
        ("mesh.loft", json!({"body": "Mesh", "points": 3})),
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

#[test]
fn a_loft_reply_matches_its_output_schema() {
    let mut s = imported_box();
    let reply = s.execute("mesh.loft", &json!({"body": "Mesh", "sections": 6})).unwrap();
    assert!(reply.get("param_warnings").is_none(), "{reply}");
    let schema = output_schema_for::<MeshLoftResult>();
    let schema = schema.as_value();
    assert!(conforms(schema, schema, &reply), "{reply} doesn't match {schema}");
}

#[test]
fn a_loft_builds_a_solid_through_the_silhouettes() {
    let mut s = imported_box();
    let reply = s.execute("mesh.loft", &json!({"body": "Mesh", "sections": 6, "points": 24})).unwrap();
    let along = reply.get("along").and_then(|v| v.as_array()).expect("along");
    assert_eq!(along.len(), 1, "one direction asked for, one read");
    let read = along.first().expect("the reading");
    assert_eq!(read.get("hulled").and_then(serde_json::Value::as_u64), Some(0), "a clean box closes at every height");
    assert_eq!(read.get("heights").and_then(|v| v.as_array()).map(Vec::len), Some(6));
    // The box is 10 by 20 by 30, read along Z, so the profiles are its 10 by 20 face.
    let across: Vec<f64> = read.get("across").and_then(|v| v.as_array()).expect("across").iter().filter_map(serde_json::Value::as_f64).collect();
    assert!((across[0] - 10.).abs() < 0.5 && (across[1] - 20.).abs() < 0.5, "{across:?}");
    assert!(reply.get("features").and_then(|v| v.as_array()).is_some_and(|f| !f.is_empty()), "{reply}");
    assert!(s.model.state().body("Mesh lofted").is_some(), "the lofted solid should be there");
    assert!(s.model.state().body("Mesh").is_some(), "the mesh body should be left alone");
}

#[test]
fn several_axes_are_lofted_apart_and_then_intersected() {
    let mut s = imported_box();
    let reply = s.execute("mesh.loft", &json!({"body": "Mesh", "axes": ["x", "z"], "sections": 4, "points": 16})).unwrap();
    assert_eq!(reply.get("along").and_then(|v| v.as_array()).map(Vec::len), Some(2));
    let bodies: Vec<&str> = reply.get("bodies").and_then(|v| v.as_array()).expect("bodies").iter().filter_map(serde_json::Value::as_str).collect();
    assert_eq!(bodies, vec!["Mesh lofted", "Mesh along 2"]);
    let steps = reply.get("steps").and_then(|v| v.as_array()).expect("steps");
    let combines = steps.iter().filter(|step| step.get("command").and_then(serde_json::Value::as_str) == Some("solid.combine")).count();
    assert_eq!(combines, 1, "the two lofts should be intersected once");
    // Only the intersection is left; the second loft was consumed by it.
    let state = s.model.state();
    assert!(state.body("Mesh lofted").is_some());
    assert!(state.body("Mesh along 2").is_none(), "the tool body should have been consumed");
}

#[test]
fn a_loft_plan_only_changes_nothing() {
    let mut s = imported_box();
    let reply = s.execute("mesh.loft", &json!({"body": "Mesh", "sections": 4, "plan_only": true})).unwrap();
    assert!(reply.get("features").and_then(|v| v.as_array()).is_some_and(|f| f.is_empty()), "{reply}");
    assert!(reply.get("steps").and_then(|v| v.as_array()).is_some_and(|s| !s.is_empty()), "{reply}");
    assert!(s.model.state().body("Mesh lofted").is_none(), "plan_only should build nothing");
}
