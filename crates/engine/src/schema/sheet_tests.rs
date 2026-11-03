use serde_json::json;

use super::BeadParams;
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<BeadParams>(),
        &[
            "angle",
            "base_radius",
            "body",
            "body_name",
            "body_names",
            "crown_radius",
            "crown_width",
            "curves",
            "depth",
            "end_length",
            "flip",
            "margin",
            "name",
            "participants",
            "shape",
            "sketch",
            "width",
        ],
    );
}

#[test]
fn a_bead_reply_matches_the_output_schema() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    let mut s = Session::default();
    // A plate 80 x 50 x 1 thickened from its top face (z = 1), and a line on that face.
    s.execute("solid.box", &json!({"length": 80, "width": 50, "height": 1, "body_name": "Plate"})).unwrap();
    s.execute("sheet.convert", &json!({"mode": "thicken", "body": "Plate", "face": [40, 25, 1]})).unwrap();
    s.execute("sketch.create", &json!({"plane": "XY", "offset": 1, "name": "Beads"})).unwrap();
    s.execute("sketch.line", &json!({"points": [[20, 25], [60, 25]], "ids": ["c1"]})).unwrap();
    s.execute("sketch.finish", &json!({})).unwrap();
    let r = s
        .execute(
            "sheet.bead",
            &json!({"sketch": "Beads", "curves": ["c1"], "depth": 2, "width": 10, "base_radius": 0.5, "crown_radius": 0.5, "name": "Rib"}),
        )
        .unwrap();
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert!(s.execute("sheet.bead", &json!({"sketch": "Beads", "curves": ["c1"], "depth": 1.5, "width": 8, "colour": "red"})).is_err());
}
