use serde_json::json;

use super::{CopeDxfParams, CopeDxfResult, CopeParams, MemberParams};
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

#[test]
fn cope_contracts_and_physical_dxf_reply_conform() {
    assert_keys(input_schema_for::<CopeParams>(), &["body_name", "body_names", "end", "member", "name", "participants", "tools"]);
    assert_keys(input_schema_for::<CopeDxfParams>(), &["end", "member", "path", "seam", "tolerance"]);
    let p = json!({"member":"Rail", "end":"end", "seam":"45 deg", "tolerance":0.01});
    let schema = input_schema_for::<CopeDxfParams>();
    assert!(conforms(schema.as_value(), schema.as_value(), &p));
    assert!(serde_json::from_value::<CopeDxfParams>(p).is_ok());
    let p = json!({"member":"Rail", "tools":["Post"], "end":"start"});
    let schema = input_schema_for::<CopeParams>();
    assert!(conforms(schema.as_value(), schema.as_value(), &p));
    let out = serde_json::json!({
        "member":"Rail", "end":"start", "units":"mm", "scale":1.0, "outer_diameter":40.0,
        "circumference":125.66370614359172, "seam":0.0, "origin":[0.0,0.0,0.0], "component":0,
        "axis":[1.0,0.0,0.0], "seam_direction":[0.0,1.0,0.0], "wrap_direction":"right_hand_about_inward_axis",
        "tolerance":0.01, "segments":1, "max_segments":65536, "profile":[[0.0,20.0],[125.66370614359172,20.0]], "dxf":"drawing"
    });
    let schema = output_schema_for::<CopeDxfResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &out));
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    let err = s.execute("frame.export_cope_dxf", &json!({"member":"Rail", "end":"start", "typo":true})).unwrap_err();
    assert!(err.to_string().contains("typo"));
}
