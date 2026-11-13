use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_doc::{Document, FeatureKind, MeshData};

use super::{MeshRebuildParams, MeshRebuildResult, MeshRecognizeParams, MeshRecognizeResult};
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
