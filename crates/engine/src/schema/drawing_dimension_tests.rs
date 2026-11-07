use serde_json::{Value, json};
use solidcraft_doc::drawing::dimension::{MAX_CHAIN_POINTS, MAX_SET_TARGETS};

use super::{AddParams, DimensionSet, EditParams, InspectParams, NUMBERED_SLOTS, RepairParams, SlotParam, TargetsSet};
use crate::schema::input_schema_for;
use crate::schema::tests::assert_keys;

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<AddParams>(), &["decimals", "kind", "offset_mm", "override_text", "text", "view"]);
    assert_keys(input_schema_for::<InspectParams>(), &["dimension"]);
    assert_keys(input_schema_for::<EditParams>(), &["dimension", "set"]);
    assert_keys(input_schema_for::<RepairParams>(), &["dimension", "pick", "slot"]);
    assert_keys(
        input_schema_for::<DimensionSet>(),
        &["decimals", "diameter", "direction", "offset_mm", "other_side", "override_text", "reading", "targets", "text"],
    );
    assert_keys(input_schema_for::<TargetsSet>(), &["add", "remove"]);
}

/// A repair's slot is one of five words, or a chain's point or a set's target by its number: the
/// last point a chain or the last target a set may have is taken, one past both, a negative number
/// and another word are refused.
#[test]
fn a_slot_is_a_word_or_a_chain_points_number() {
    let read = |v: Value| serde_json::from_value::<SlotParam>(v).map_err(|e| e.to_string());
    assert_eq!(NUMBERED_SLOTS, MAX_CHAIN_POINTS.max(MAX_SET_TARGETS));
    for last in [MAX_CHAIN_POINTS - 1, MAX_SET_TARGETS - 1] {
        assert_eq!(read(json!(last)), Ok(SlotParam::Point(last)));
    }
    assert_eq!(read(json!("b")), Ok(SlotParam::B));
    assert_eq!(read(json!("origin")), Ok(SlotParam::Origin));
    for v in [json!(NUMBERED_SLOTS), json!(-1), json!("c"), json!("Origin"), json!(1.5)] {
        assert!(read(v.clone()).is_err_and(|e| e.contains("a slot is")), "{v}: {:?}", read(v.clone()));
    }
}
