use serde_json::json;

use super::{PointCloudReloadParams, PointCloudReloadResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<PointCloudReloadParams>(), &["cloud"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[("pointcloud.reload", json!({"cloud": [1]}))]);
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
