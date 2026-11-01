use serde_json::json;

use super::PlateParams;
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn plate_contract_and_reply_agree() {
    let schema = input_schema_for::<PlateParams>();
    assert_keys(
        schema.clone(),
        &[
            "angle",
            "body",
            "body_name",
            "body_names",
            "diameter",
            "face",
            "height",
            "holes",
            "index",
            "name",
            "offset",
            "participants",
            "path",
            "shape",
            "thickness",
            "width",
            "x_offset",
            "y_offset",
        ],
    );
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":40,"width":40,"height":100})).unwrap();
    s.param_check = ParamCheck::Strict;
    for mut params in [
        json!({"face":[20,20,100],"width":100,"height":80,"thickness":8}),
        json!({"face":[20,20,100],"shape":"round","diameter":120,"thickness":8,"holes":{"pattern":"circle","diameter":10,"pitch_diameter":90,"count":6,"angle":"15 deg"}}),
        json!({"face":[20,20,0],"width":100,"height":80,"thickness":8,"holes":{"pattern":"rectangle","diameter":10,"pitch_x":70,"pitch_y":50}}),
    ] {
        let state = s.world_state();
        let body = state.bodies.first().unwrap();
        let high = params["face"][2].as_f64().unwrap() > 0.;
        let face =
            body.body.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z * if high { 1. } else { -1. } > 0.99)).unwrap();
        params["body"] = json!(body.name);
        params["index"] = json!(face.index);
        let reply = s.execute("frame.plate", &params).unwrap();
        let out = output_schema_for::<FeatureResult>();
        assert!(conforms(out.as_value(), out.as_value(), &reply));
        assert!(conforms(schema.as_value(), schema.as_value(), &params));
    }
    assert!(s.execute("frame.plate", &json!({"face":[20,20,100],"width":100,"height":80,"thickness":8,"typo":1})).is_err());
}
