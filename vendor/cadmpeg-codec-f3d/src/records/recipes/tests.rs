// SPDX-License-Identifier: Apache-2.0

use super::{ConstructionRecipe, ConstructionRecipeWire, CONSTRUCTION_RECIPE_CLONE_COUNT};

fn recipe(with_design: bool) -> ConstructionRecipe {
    let suffix = if with_design {
        r#","design_id":"301","design_id_offset":12,"design_selector":{"value":2,"byte_offset":15}"#
    } else {
        ""
    };
    serde_json::from_str(&format!(
        r#"{{"id":"f3d:native:recipe#0","byte_offset":80,"record_index_offset":64,"kind":"body"{suffix},"recipe_index":0,"record_index":7}}"#
    ))
    .unwrap()
}

#[test]
fn construction_recipe_borrowed_wire_matches_owned_wire_bytes() {
    for record in [recipe(false), recipe(true)] {
        let owned = ConstructionRecipeWire::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn construction_recipe_native_retained_limit_refuses_before_record_clone() {
    let record = recipe(true);
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "construction_recipes",
        || CONSTRUCTION_RECIPE_CLONE_COUNT.with(|count| count.set(0)),
        || CONSTRUCTION_RECIPE_CLONE_COUNT.with(std::cell::Cell::get),
    );
}
