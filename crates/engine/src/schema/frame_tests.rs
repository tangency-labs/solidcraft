use serde_json::json;

use super::MemberParams;
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<MemberParams>(),
        &[
            "anchor",
            "angle",
            "body_name",
            "body_names",
            "continuous",
            "lines",
            "mirror",
            "name",
            "names",
            "participants",
            "profile",
            "profile_sketch",
            "profiles",
            "sketch",
        ],
    );
    let schema = input_schema_for::<MemberParams>();
    let p = json!({"sketch": "Chassis", "lines": ["l1", "a1", "l2"], "continuous": true,
        "profile": {"kind": "round_tube", "diameter": "20 mm", "wall": 2}, "names": {"$path": "Hoop"}});
    assert!(conforms(schema.as_value(), schema.as_value(), &p));
    assert!(serde_json::from_value::<MemberParams>(p).is_ok());
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    s.execute("sketch.create", &json!({"plane": "XY", "name": "Line"})).unwrap();
    s.execute("sketch.line", &json!({"points": [[0, 0], [100, 0]]})).unwrap();
    s.execute("sketch.finish", &json!({})).unwrap();
    let p = json!({"sketch": "Line", "lines": ["l1"], "continuous": true, "profile": {"kind": "bar", "width": 10, "height": 5}});
    let result = s.execute("frame.member", &p).unwrap();
    let schema = output_schema_for::<FeatureResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &result), "{result}");
    let mut unknown = p;
    unknown["typo"] = json!(true);
    assert!(s.execute("frame.member", &unknown).unwrap_err().to_string().contains("typo"));
}
