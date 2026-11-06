// SPDX-License-Identifier: Apache-2.0

use super::{fixed_scope, recipe_edge_operand};
use crate::design::edge_resolve::edge_treatment_recipe_state_id;

#[test]
fn edge_treatment_recipe_state_is_the_state_all_scope_operands_share() {
    let scope = fixed_scope();
    let mut first = recipe_edge_operand(10, &[], &[]);
    let mut second = recipe_edge_operand(20, &[], &[]);
    first.recipe_state_id = Some(663);
    second.recipe_state_id = Some(663);
    assert_eq!(
        edge_treatment_recipe_state_id(&scope, &[first.clone(), second.clone()]),
        Some(663)
    );

    second.recipe_state_id = Some(664);
    assert_eq!(
        edge_treatment_recipe_state_id(&scope, &[first.clone(), second.clone()]),
        None
    );

    second.recipe_state_id = None;
    assert_eq!(
        edge_treatment_recipe_state_id(&scope, &[first, second]),
        None
    );
    assert_eq!(edge_treatment_recipe_state_id(&scope, &[]), None);
}
