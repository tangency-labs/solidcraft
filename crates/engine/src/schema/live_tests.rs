use serde_json::json;

use super::{LiveMeshesParams, LiveMeshesResult};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<LiveMeshesParams>(),
        &["angle_deg", "bodies", "chord_mm", "chord_relative", "have", "include_hidden", "max_mb", "normals", "overrides", "quality", "topology"],
    );
}

#[test]
fn replies_match_the_output_schema() {
    let schema = output_schema_for::<LiveMeshesResult>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let empty = s.execute("live.meshes", &json!({})).unwrap_or_else(|e| panic!("{e}"));
    assert!(conforms(schema, schema, &empty), "{empty} doesn't match {schema}");
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "body_name": "Block"})).unwrap_or_else(|e| panic!("{e}"));
    s.execute("appearance.assign", &json!({"faces": [[5, 5, 10]], "color": "#ff0000"})).unwrap_or_else(|e| panic!("{e}"));
    for p in [
        json!({}),
        json!({"quality": "fine", "topology": "quads", "normals": false}),
        json!({"chord_mm": 0.1, "angle_deg": 10}),
        json!({"chord_relative": 0.01, "bodies": ["Block"], "include_hidden": true, "max_mb": 2}),
        json!({"overrides": [{"body": "Block", "topology": "triangles", "quality": "coarse"}], "have": ["0123"]}),
    ] {
        let r = s.execute("live.meshes", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
    }
    let r = s.execute("live.meshes", &json!({"unknown_extra": 1})).unwrap_or_else(|e| panic!("{e}"));
    assert!(r.get("param_warnings").is_some(), "{r}");
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    s.param_check = ParamCheck::Strict;
    assert!(s.execute("live.meshes", &json!({"unknown_extra": 1})).is_err());
}
