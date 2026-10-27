use serde_json::json;

use super::TrimParams;
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_cover_trim_parameters_and_results() {
    let schema = input_schema_for::<TrimParams>();
    assert_keys(schema.clone(), &["body_name", "body_names", "end", "gap", "members", "name", "participants", "to"]);
    for to in [
        json!("Post"),
        json!({"member": {"feature": 3, "line": "l1"}, "side": "left"}),
        json!({"feature": "Members", "line": "l1"}),
        json!({"plane": "YZ"}),
        json!({"plane": {"face": [150, 0, 0], "body": "Block", "index": 2, "path": []}}),
        json!({"plane_ref": {"type": "origin", "name": "YZ"}}),
    ] {
        let p = json!({"members": ["Rail", {"feature": 3, "line": "l2"}], "to": to, "gap": "2 mm", "end": "start"});
        assert!(serde_json::from_value::<TrimParams>(p.clone()).is_ok(), "{p}");
        assert!(conforms(schema.as_value(), schema.as_value(), &p), "{p}");
    }
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    for (command, params) in [
        ("sketch.create", json!({"plane": "XY", "name": "Line"})),
        ("sketch.line", json!({"points": [[0, 0], [100, 0]]})),
        ("sketch.finish", json!({})),
        ("frame.member", json!({"sketch": "Line", "profile": {"kind": "bar", "width": 10, "height": 10}, "body_name": "Bar"})),
    ] {
        s.execute(command, &params).unwrap();
    }
    let mut p = json!({"members": "Bar", "to": {"plane": {"origin": [150, 0, 0], "normal": [1, 0, 0]}}});
    assert!(conforms(schema.as_value(), schema.as_value(), &p));
    let before = s.doc.features.len();
    p["gaap"] = json!(2);
    assert!(s.execute("frame.trim", &p).unwrap_err().to_string().contains("gaap"));
    assert_eq!(s.doc.features.len(), before);
    p.as_object_mut().unwrap().remove("gaap");
    let result = s.execute("frame.trim", &p).unwrap();
    let output = output_schema_for::<FeatureResult>();
    assert!(conforms(output.as_value(), output.as_value(), &result), "{result}");
    assert_eq!(result["changed_bodies"], json!(["Bar"]));
}
