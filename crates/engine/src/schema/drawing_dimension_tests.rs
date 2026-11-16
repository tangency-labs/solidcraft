use serde_json::{Value, json};
use solidcraft_doc::drawing::dimension::MAX_CHAIN_POINTS;

use super::{AddParams, DimensionSet, EditParams, InspectParams, RepairParams, SlotParam};
use crate::schema::input_schema_for;
use crate::schema::tests::assert_keys;

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<AddParams>(), &["decimals", "dry_run", "kind", "offset_mm", "override_text", "text", "view"]);
    assert_keys(input_schema_for::<InspectParams>(), &["dimension"]);
    assert_keys(input_schema_for::<EditParams>(), &["dimension", "set"]);
    assert_keys(input_schema_for::<RepairParams>(), &["dimension", "pick", "slot"]);
    assert_keys(
        input_schema_for::<DimensionSet>(),
        &["decimals", "diameter", "direction", "offset_mm", "other_side", "override_text", "reading", "text"],
    );
}

/// A repair's slot is one of four words, or a chain's point by its number: the last point a chain
/// may have is taken, one past it, a negative number and another word are refused.
#[test]
fn a_slot_is_a_word_or_a_chain_points_number() {
    let read = |v: Value| serde_json::from_value::<SlotParam>(v).map_err(|e| e.to_string());
    let last = MAX_CHAIN_POINTS - 1;
    assert_eq!(read(json!(last)), Ok(SlotParam::Point(last)));
    assert_eq!(read(json!("b")), Ok(SlotParam::B));
    for v in [json!(MAX_CHAIN_POINTS), json!(-1), json!("c"), json!(1.5)] {
        assert!(read(v.clone()).is_err_and(|e| e.contains("a slot is")), "{v}: {:?}", read(v.clone()));
    }
}
