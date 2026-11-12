use super::{PublicationsParams, PublishParams, UnpublishParams};
use crate::schema::input_schema_for;
use crate::schema::tests::assert_keys;

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<PublishParams>(), &["items", "name"]);
    assert_keys(input_schema_for::<UnpublishParams>(), &["name"]);
    assert_keys(input_schema_for::<PublicationsParams>(), &[]);
}
