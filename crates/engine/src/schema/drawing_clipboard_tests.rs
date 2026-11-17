use super::{CopyParams, PasteParams};
use crate::schema::input_schema_for;
use crate::schema::tests::assert_keys;

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<CopyParams>(), &["sheets", "tables", "views"]);
    assert_keys(input_schema_for::<PasteParams>(), &["from_design", "position_mm", "sheet", "to"]);
}
