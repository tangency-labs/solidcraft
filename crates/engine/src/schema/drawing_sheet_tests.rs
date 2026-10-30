use serde_json::{Value, json};

use super::*;
use crate::schema::tests::{assert_keys, conforms, drawing_session};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    let settings = ["documents", "dpi", "excluded", "name", "orientation", "size", "title_block"];
    let with = |key: &'static str| -> Vec<&'static str> {
        let mut keys: Vec<&str> = settings.iter().copied().chain([key]).collect();
        keys.sort_unstable();
        keys
    };
    assert_keys(input_schema_for::<SheetAddParams>(), &with("after"));
    assert_keys(input_schema_for::<SheetSetParams>(), &with("sheet"));
    assert_keys(input_schema_for::<SheetReorderParams>(), &["sheets"]);
    assert_keys(input_schema_for::<SheetDeleteParams>(), &["contents", "sheet"]);
}

/// A call that already fails gets the same error in strict as with the check off.
#[test]
fn strict_keeps_todays_errors() {
    let cases = [
        ("drawing.sheet.add", json!({"after": 5})),
        ("drawing.sheet.add", json!({"name": null})),
        ("drawing.sheet.add", json!({"size": {"standard": "b7"}})),
        ("drawing.sheet.add", json!({"orientation": "upright"})),
        ("drawing.sheet.add", json!({"dpi": "high"})),
        ("drawing.sheet.set", json!({"sheet": 5})),
        ("drawing.sheet.set", json!({"sheet": "Sheet 1", "documents": {"source": 5}})),
        ("drawing.sheet.set", json!({"sheet": "Sheet 1", "documents": {"source": "0000000000001", "component": -1}})),
        ("drawing.sheet.set", json!({"sheet": "Sheet 1", "excluded": {"count": "yes"}})),
        ("drawing.sheet.set", json!({"sheet": "Sheet 1", "excluded": {"cuont": true}})),
        ("drawing.sheet.set", json!({"sheet": "Sheet 1", "excluded": []})),
        ("drawing.sheet.set", json!({"sheet": "Sheet 1", "title_block": 5})),
        ("drawing.sheet.set", json!({"sheet": "Sheet 1", "title_block": {"size_mm": [180, 54], "colour": "red"}})),
        ("drawing.sheet.reorder", json!({"sheets": "all"})),
        ("drawing.sheet.reorder", json!({"sheets": [5]})),
        ("drawing.sheet.delete", json!({"sheet": null})),
        ("drawing.sheet.delete", json!({"sheet": "Sheet 1", "contents": "keep"})),
    ];
    for (command, p) in cases {
        let error = |level| drawing_session(level).0.execute(command, &p).err().map(|e| e.to_string());
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{command} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{command} {p}");
    }
}

/// Each command's reply, in each form, matches its output schema, with no warnings in warn mode.
#[test]
fn replies_match_the_output_schema() {
    let (mut s, source) = drawing_session(ParamCheck::Warn);
    let schema = output_schema_for::<SheetsResult>();
    let schema = schema.as_value();
    let mut check = |command: &str, p: Value| {
        let r = s.execute(command, &p).unwrap_or_else(|e| panic!("{command}: {e}"));
        assert!(conforms(schema, schema, &r) && r.get("param_warnings").is_none(), "{command}: {r} doesn't match {schema}");
        r
    };
    let full = json!({
        "name": "Details", "size": {"custom": {"sides_mm": [300, 200]}}, "orientation": "portrait", "dpi": 300,
        "documents": {"source": source, "component": 0}, "excluded": {"count": true, "print": false},
    });
    let added = check("drawing.sheet.add", full);
    let id = added["sheet"].clone();
    let other = check("drawing.sheet.add", json!({"after": "Sheet 1"}))["sheet"].clone();
    check("drawing.sheet.set", json!({"sheet": id, "size": {"standard": "a2"}, "documents": null}));
    let order: Vec<Value> =
        check("drawing.sheet.set", json!({"sheet": "Sheet 1"}))["sheets"].as_array().unwrap().iter().rev().map(|sheet| sheet["id"].clone()).collect();
    check("drawing.sheet.reorder", json!({"sheets": order}));
    check("drawing.sheet.delete", json!({"sheet": id}));
    check("drawing.sheet.delete", json!({"sheet": other, "contents": "delete"}));
}
