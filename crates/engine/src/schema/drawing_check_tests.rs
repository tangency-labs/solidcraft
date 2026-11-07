use super::{CheckParams, CheckResult, CoverageState, FindingKind};
use crate::schema::tests::{assert_keys, closed_list, names_of};
use crate::schema::{input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<CheckParams>(), &["sheet"]);
}

/// The reply's kinds and coverage states are closed lists: the schema names every value of each,
/// and nothing else.
#[test]
fn the_kinds_are_a_closed_list() {
    let schema = output_schema_for::<CheckResult>();
    assert_eq!(closed_list(&schema, "FindingKind"), names_of(FindingKind::ALL));
    assert_eq!(closed_list(&schema, "CoverageState"), names_of(&CoverageState::ALL));
}
