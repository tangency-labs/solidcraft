use super::{ReplaceParams, ToParam};
use crate::schema::input_schema_for;
use crate::schema::tests::assert_keys;

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<ReplaceParams>(), &["drawing", "dry_run", "sheet", "to", "view"]);
    assert_keys(input_schema_for::<ToParam>(), &["component", "path"]);
}
