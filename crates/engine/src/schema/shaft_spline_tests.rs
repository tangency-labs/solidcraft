//! Tests of the contract and reply of `shaft_spline.insert`.

use serde_json::{Value, json};

use super::{ShaftSplineAdded, ShaftSplineParams};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<ShaftSplineParams>(),
        &[
            "addendum",
            "body_name",
            "body_names",
            "bore_face",
            "bore_start",
            "cutter_diameter",
            "dedendum",
            "end_chamfer",
            "end_chamfer_angle",
            "end_chamfer_distance2",
            "entry_chamfer",
            "entry_chamfer_angle",
            "entry_chamfer_distance2",
            "flank_angle",
            "form_clearance",
            "hub_addendum",
            "hub_diameter",
            "hub_root_round",
            "hub_tip_round",
            "length",
            "major_clearance",
            "major_diameter",
            "make",
            "master_angle",
            "master_spline",
            "minor_clearance",
            "minor_diameter",
            "module",
            "name",
            "participants",
            "pressure_angle",
            "profile_shift",
            "root_fillet",
            "root_round",
            "shaft_face",
            "shaft_start",
            "side_clearance",
            "teeth",
            "thinning",
            "tip_clearance",
            "tip_corner",
            "tip_size",
            "tooth_width",
            "type",
        ],
    );
}

fn straight() -> Value {
    json!({
        "type": "straight", "teeth": 6, "length": 20, "major_diameter": 28, "minor_diameter": 23, "tooth_width": 6,
        "side_clearance": 0.05, "major_clearance": 0.3, "minor_clearance": 0.2
    })
}

fn with(mut p: Value, changes: Value) -> Value {
    for (k, v) in changes.as_object().cloned().unwrap_or_default() {
        p[k] = v;
    }
    p
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("shaft_spline.insert", with(straight(), json!({"length": 0}))),
        ("shaft_spline.insert", with(straight(), json!({"master_angle": 400}))),
        ("shaft_spline.insert", with(straight(), json!({"make": "both"}))),
        ("shaft_spline.insert", with(straight(), json!({"module": 2}))),
        ("shaft_spline.insert", with(straight(), json!({"end_chamfer": -1}))),
        ("shaft_spline.insert", with(straight(), json!({"end_chamfer_angle": 30}))),
        ("shaft_spline.insert", with(straight(), json!({"end_chamfer": 1, "end_chamfer_distance2": 0.5, "end_chamfer_angle": 30}))),
        ("shaft_spline.insert", with(straight(), json!({"end_chamfer": 1, "end_chamfer_angle": 90}))),
        ("shaft_spline.insert", with(straight(), json!({"entry_chamfer_distance2": 0.5}))),
        ("shaft_spline.insert", with(straight(), json!({"bore_face": [11, 0, 25], "hub_diameter": 40}))),
    ]);
}

#[test]
fn the_contract_refuses_what_the_handler_refuses() {
    let schema = input_schema_for::<ShaftSplineParams>();
    let root = schema.as_value();
    assert!(conforms(root, root, &straight()));
    let picked = with(
        straight(),
        json!({"shaft_face": [14, 0, 55], "bore_face": [11, 0, 25], "cutter_diameter": 40, "end_chamfer": 1, "entry_chamfer": "0.5 mm"}),
    );
    assert!(conforms(root, root, &picked), "{picked}");
    // A chamfer's second distance and its angle are a length and an angle, numbers or expressions.
    let chamfers = json!({"end_chamfer": 1, "end_chamfer_distance2": "0.5 mm", "entry_chamfer": 1, "entry_chamfer_angle": 30});
    let set = with(with(straight(), json!({"make": "both", "hub_diameter": 40})), chamfers);
    assert!(conforms(root, root, &set), "{set}");
    assert!(serde_json::from_value::<ShaftSplineParams>(set).is_ok());
    for bad in [json!({"teeth": 6, "length": 20, "side_clearance": 0.05}), with(straight(), json!({"extra": 1}))] {
        assert!(!conforms(root, root, &bad), "{bad}");
    }
    // The keywords and the picked points are checked by the contract's own types. The count of teeth is a
    // number or an expression there; the handler says what a count must be.
    for bad in [
        with(straight(), json!({"type": "helical"})),
        with(straight(), json!({"make": "pulley"})),
        with(straight(), json!({"tip_corner": "bevelled"})),
        with(straight(), json!({"master_spline": "keyed"})),
        with(straight(), json!({"shaft_face": [14, 0]})),
        with(straight(), json!({"bore_face": "the bore"})),
    ] {
        assert!(serde_json::from_value::<ShaftSplineParams>(bad.clone()).is_err(), "{bad}");
    }
    assert!(serde_json::from_value::<ShaftSplineParams>(straight()).is_ok());
    assert!(serde_json::from_value::<ShaftSplineParams>(picked).is_ok());
}

/// The reply is what its schema says, for new parts and for a picked shaft: in Warn without warnings for a good
/// call and with them for a misspelt key.
#[test]
fn the_reply_matches_its_schema() {
    let schema = output_schema_for::<ShaftSplineAdded>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let both = with(straight(), json!({"make": "both", "hub_diameter": 40, "master_spline": "wide_tooth", "master_angle": 90}));
    let r = s.execute("shaft_spline.insert", &both).unwrap();
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert!(r.get("param_warnings").is_none(), "{r}");
    assert_eq!(r["master_spline"]["form"], "wide_tooth", "{r}");
    assert_eq!((r["positions"].as_u64(), r["created_bodies"].as_array().map(Vec::len)), (Some(1), Some(2)), "{r}");
    let involute = json!({"type": "involute", "teeth": 12, "length": 5, "module": 1, "side_clearance": 0.05, "tip_clearance": 0.2});
    let r = s.execute("shaft_spline.insert", &involute).unwrap();
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert!(r["pitch_diameter"].as_f64().is_some() && r.get("master_spline").is_none(), "{r}");
    let r = s.execute("shaft_spline.insert", &with(straight(), json!({"lenght": 3}))).unwrap();
    assert!(conforms(schema, schema, &r) && r.get("param_warnings").is_some(), "{r}");
    // A spline cut into a picked shaft: the shaft changes, nothing is made.
    let mut s = Session::default();
    s.execute("solid.cylinder", &json!({"radius": 14, "height": 60, "body_name": "Shaft"})).unwrap();
    let r = s.execute("shaft_spline.insert", &with(straight(), json!({"shaft_face": [14, 0, 55]}))).unwrap();
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert_eq!(r["bodies"], json!(["Shaft"]), "{r}");
}
