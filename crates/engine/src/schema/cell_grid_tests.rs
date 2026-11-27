use serde_json::json;

use super::CellGridParams;
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<CellGridParams>(),
        &[
            "anchor",
            "angle",
            "body_name",
            "body_names",
            "cell_angle",
            "cell_origin",
            "cell_sketch",
            "clip",
            "depth",
            "direction",
            "dot_grid",
            "flip",
            "keepout_clip",
            "margin",
            "mode",
            "name",
            "participants",
            "pitch_x",
            "pitch_y",
            "profiles",
            "shape",
            "sides",
            "size",
            "sketch",
            "snap_inset",
            "stagger",
            "targets",
            "through_all",
            "web",
        ],
    );
}

#[test]
fn the_contract_refuses_what_the_handler_refuses() {
    let schema = input_schema_for::<CellGridParams>();
    let root = schema.as_value();
    let ok = json!({"size": 5, "web": 1, "depth": 3, "shape": "circle", "dot_grid": "square", "clip": "free", "anchor": [0, 0, 0]});
    assert!(conforms(root, root, &ok));
    // `size` is required by the handler except for a sketched cell, which has no size.
    let sketched = json!({"shape": "sketch", "cell_sketch": "Cell", "pitch_x": 20, "pitch_y": "2 * 10", "stagger": 0.5, "cell_angle": "30 deg", "flip": true, "cell_origin": [1, "2 mm"], "web": 1, "depth": 3});
    assert!(conforms(root, root, &sketched));
    assert!(conforms(root, root, &json!({"shape": "sketch", "cell_sketch": 3, "cell_origin": {"x": 1, "y": 2}, "web": 1})));
    for bad in [json!({"size": 5}), json!({"size": 5, "web": 1, "extra": 1})] {
        assert!(!conforms(root, root, &bad), "{bad}");
    }
    // The keywords are checked by the contract's own types.
    for bad in [
        json!({"size": 5, "web": 1, "shape": "blob"}),
        json!({"size": 5, "web": 1, "mode": "flat"}),
        json!({"size": 5, "web": 1, "dot_grid": "triangle"}),
        json!({"size": 5, "web": 1, "clip": "x"}),
        json!({"web": 1, "shape": "sketch", "cell_origin": [1]}),
        json!({"web": 1, "shape": "sketch", "cell_origin": [1, 2, 3]}),
        json!({"web": 1, "shape": "sketch", "cell_origin": ["a", true]}),
        json!({"web": 1, "shape": "sketch", "flip": "yes"}),
        json!({"web": 1, "shape": "sketch", "cell_sketch": true}),
        json!({"web": 1, "shape": "sketch", "pitch_x": [1]}),
        json!({"web": 1, "shape": "sketch", "stagger": {}}),
    ] {
        assert!(serde_json::from_value::<CellGridParams>(bad.clone()).is_err(), "{bad}");
    }
}

#[test]
fn the_reply_is_a_feature_result() {
    let mut s = Session::default();
    for (id, p) in [
        ("solid.box", json!({"length": 30, "width": 30, "height": 3, "body_name": "Plate"})),
        ("sketch.create", json!({"plane": "XY", "name": "Field"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [30, 30]})),
        ("sketch.finish", json!({})),
    ] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    let reply = s.execute("solid.cell_grid", &json!({"sketch": "Field", "size": 5, "web": 1, "through_all": true})).unwrap();
    let schema = output_schema_for::<FeatureResult>();
    let root = schema.as_value();
    assert!(conforms(root, root, &reply), "{reply}");
}
