use serde_json::json;

use super::{SectionDeleteParams, SectionEditParams, SectionListParams, SectionParams, SectionRenameParams, SectionVisibilityParams};
use crate::schema::input_schema_for;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<SectionParams>(), &["at", "clear", "flip", "hatch", "name", "offset", "plane"]);
    assert_keys(input_schema_for::<SectionEditParams>(), &["at", "flip", "hatch", "name", "offset", "plane", "section"]);
    assert_keys(input_schema_for::<SectionVisibilityParams>(), &["section", "visible"]);
    assert_keys(input_schema_for::<SectionRenameParams>(), &["name", "section"]);
    assert_keys(input_schema_for::<SectionDeleteParams>(), &["section"]);
    assert_keys(input_schema_for::<SectionListParams>(), &[]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("inspect.section", json!({"plane": "Nope"})),
        ("inspect.section", json!({"plane": {"normal": [0, 0, 0]}})),
        ("inspect.section", json!({"plane": "XZ", "at": 1, "offset": 1})),
        ("inspect.section.delete", json!({"section": 7})),
        ("inspect.section.rename", json!({"section": "Nope", "name": "A"})),
    ]);
}
