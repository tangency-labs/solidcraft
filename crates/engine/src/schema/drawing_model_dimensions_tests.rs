use super::{FromModelParams, FromModelResult, SkipReason, SourceKind};
use crate::schema::tests::{assert_keys, closed_list, names_of};
use crate::schema::{input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<FromModelParams>(), &["body", "feature", "sketch", "view"]);
}

/// What a model dimension is and why one is skipped are closed lists: the schema names every value
/// of each, and nothing else.
#[test]
fn the_kinds_and_reasons_are_closed_lists() {
    let schema = output_schema_for::<FromModelResult>();
    assert_eq!(closed_list(&schema, "SourceKind"), names_of(&SourceKind::ALL));
    assert_eq!(closed_list(&schema, "SkipReason"), names_of(&SkipReason::ALL));
}
