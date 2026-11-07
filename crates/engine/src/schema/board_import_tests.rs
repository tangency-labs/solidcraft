use serde_json::{Value, json};

use super::{BoardImportResult, InsertGerberParams};
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<InsertGerberParams>(), &["drills", "drills_text", "name", "origin", "outline", "outline_text", "thickness"]);
}

#[test]
fn strict_keeps_todays_errors() {
    let text = "%MOMM*%\nM02*\n";
    assert_strict_keeps_todays_errors(&[
        ("file.insert_gerber", json!({"outline_text": text, "thickness": 0})),
        ("file.insert_gerber", json!({"outline_text": text, "thickness": 21})),
        ("file.insert_gerber", json!({"outline_text": text, "thickness": "1.6"})),
        ("file.insert_gerber", json!({"outline_text": text, "origin": "centre"})),
        ("file.insert_gerber", json!({"outline_text": text, "origin": 1})),
        ("file.insert_gerber", json!({"outline_text": text, "drills_text": vec![text; 17]})),
        ("file.insert_gerber", json!({"outline": "a.gbr", "outline_text": text})),
    ]);
}

/// The reply type's schema takes what the command returns.
#[test]
fn a_board_reply_matches_its_schema() {
    let schema = output_schema_for::<BoardImportResult>();
    let schema = schema.as_value();
    let reply = BoardImportResult {
        component: 1,
        component_name: "board-Edge_Cuts".into(),
        sketch: 1,
        feature: 2,
        body: "Board".into(),
        bodies: vec!["Board".into()],
        holes: 4,
        slots: 1,
        skipped: 1,
        thickness: 1.6,
        offset: [-10.0, 0.0],
        warnings: vec!["skipped the 3.2 mm hole at (40, 10): it is not wholly inside the board".into()],
        param_warnings: Vec::new(),
    };
    let r = serde_json::to_value(reply).unwrap_or(Value::Null);
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert!(r.get("param_warnings").is_none(), "{r}");
}
