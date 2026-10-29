use super::{AddParams, BalloonSet, EditParams, InspectParams};
use crate::schema::input_schema_for;
use crate::schema::tests::assert_keys;

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<AddParams>(), &["at", "edge", "view"]);
    assert_keys(input_schema_for::<InspectParams>(), &["balloon"]);
    assert_keys(input_schema_for::<EditParams>(), &["balloon", "set"]);
    assert_keys(input_schema_for::<BalloonSet>(), &["at"]);
}
