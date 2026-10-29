use serde_json::json;

use super::*;
use crate::schema::tests::{assert_keys, drawing_session};
use crate::schema::{ParamCheck, input_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<RevisionAddParams>(), &["set"]);
    assert_keys(input_schema_for::<RevisionEditParams>(), &["row", "set"]);
    assert_keys(input_schema_for::<RevisionDeleteParams>(), &["row"]);
    assert_keys(input_schema_for::<RowSet>(), &["checker", "date", "description", "designer", "hidden", "revision"]);
}

/// A call that already fails gets the same error in strict as with the check off.
#[test]
fn strict_keeps_todays_errors() {
    let cases = [
        ("drawing.revision.add", json!({"set": {"date": 5}})),
        ("drawing.revision.add", json!({"set": {"hidden": "no"}})),
        ("drawing.revision.add", json!({"set": {"revision": "1\n2"}})),
        ("drawing.revision.edit", json!({"row": 5, "set": {}})),
        ("drawing.revision.edit", json!({"row": "0000000000001", "set": {}})),
        ("drawing.revision.delete", json!({"row": null})),
    ];
    for (command, p) in cases {
        let error = |level| drawing_session(level).0.execute(command, &p).err().map(|e| e.to_string());
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{command} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{command} {p}");
    }
}
