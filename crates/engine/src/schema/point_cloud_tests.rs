use serde_json::json;

use super::{
    PointCloudDeleteParams, PointCloudDeleteResult, PointCloudListParams, PointCloudListResult, PointCloudReloadParams, PointCloudReloadResult,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<PointCloudReloadParams>(), &["cloud"]);
    assert_keys(input_schema_for::<PointCloudDeleteParams>(), &["cloud"]);
    assert_keys(input_schema_for::<PointCloudListParams>(), &[]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("pointcloud.reload", json!({"cloud": [1]})),
        ("pointcloud.delete", json!({"cloud": [1]})),
        ("pointcloud.delete", json!({"cloud": {"name": "scan"}})),
    ]);
}

#[test]
fn the_reply_matches_the_output_schema() {
    let dir = std::env::temp_dir().join(format!("solidcraft-pointcloud-schema-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("two.xyz");
    std::fs::write(&path, "0 0 0\n1 0 0\n").unwrap();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let id = s.execute("pointcloud.insert", &json!({"path": path})).unwrap()["cloud"].clone();
    let reply = s.execute("pointcloud.reload", &json!({"cloud": id})).unwrap();
    assert!(reply.get("param_warnings").is_none(), "{reply}");
    let schema = output_schema_for::<PointCloudReloadResult>();
    let schema = schema.as_value();
    assert!(conforms(schema, schema, &reply), "{reply} doesn't match {schema}");
    let _ = std::fs::remove_dir_all(dir);
}

/// A scan with a colour, edges and a face, a whole-numbered property whose values the file names,
/// and a plain one: everything `pointcloud.list` has to describe.
const SCAN: &str = "ply\nformat ascii 1.0\ncomment role: 7 wall, -1 none\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\n\
    property uchar red\nproperty uchar green\nproperty uchar blue\nproperty double t\n\
    element edge 2\nproperty int vertex1\nproperty int vertex2\nproperty int role\n\
    element face 1\nproperty list uchar int vertex_indices\nend_header\n\
    0 0 0 200 60 60 0.5\n1 0 0 60 200 60 1.5\n1 1 0 60 60 200 2.5\n0 1 0 200 200 60 3.5\n0 1 7\n1 2 -1\n4 0 1 2 3\n";

#[test]
fn a_listed_cloud_and_a_deleted_one_match_their_schemas() {
    let dir = std::env::temp_dir().join(format!("solidcraft-pointcloud-list-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("scan.ply");
    std::fs::write(&path, SCAN).unwrap();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let inserted =
        s.execute("pointcloud.insert", &json!({"path": path, "color_by": "role", "filters": [{"property": "role", "values": [7]}]})).unwrap();
    let id = inserted["cloud"].clone();
    let reply = s.execute("pointcloud.list", &json!({})).unwrap();
    assert!(reply.get("param_warnings").is_none(), "{reply}");
    let schema = output_schema_for::<PointCloudListResult>();
    let schema = schema.as_value();
    assert!(conforms(schema, schema, &reply), "{reply} doesn't match {schema}");
    // The interesting parts of the reply are really there, so the schema above describes them.
    let cloud = &reply["clouds"][0];
    assert_eq!(cloud["cloud"], id, "{reply}");
    assert_eq!((cloud["segments"].as_u64(), cloud["triangles"].as_u64()), (Some(2), Some(2)), "{reply}");
    assert_eq!(cloud["color_by"], "role", "coloured by a property names it");
    assert_eq!(cloud["filters"], json!([{"property": "role", "values": [7]}]), "a filter reads as it was given");
    assert!(cloud["color_range"].is_array(), "the ramp's range now: {reply}");
    assert!(cloud["height_auto"].as_bool().unwrap_or(false), "no range of its own: {reply}");
    let role = cloud["properties"].as_array().unwrap().iter().find(|p| p["name"] == json!("role")).expect("the file's property");
    assert_eq!(role["integer"], json!(true), "{role}");
    assert_eq!(role["names"], json!([[7, "wall"], [-1, "none"]]), "{role}");
    assert!(role["span"].is_array(), "{role}");
    assert!(cloud["properties"].as_array().unwrap().iter().any(|p| p["name"] == json!("z") && p["span"].is_array()), "{reply}");
    assert!(cloud["bounds"].is_array(), "{reply}");
    assert_eq!(cloud["matrix"].as_array().unwrap().len(), 12, "{reply}");
    assert_eq!(cloud["file"], "scan.ply", "the file's name: {reply}");
    assert!(cloud["source"].as_str().is_some_and(|s| s.ends_with("scan.ply")), "the path a reload reads: {reply}");

    let deleted = s.execute("pointcloud.delete", &json!({"cloud": id})).unwrap();
    assert_eq!(deleted, json!({"deleted": id}));
    let schema = output_schema_for::<PointCloudDeleteResult>();
    let schema = schema.as_value();
    assert!(conforms(schema, schema, &deleted), "{deleted} doesn't match {schema}");
    assert_eq!(s.execute("pointcloud.list", &json!({})).unwrap()["clouds"], json!([]), "it is gone");
    let _ = std::fs::remove_dir_all(dir);
}
