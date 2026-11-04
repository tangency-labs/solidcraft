// SPDX-License-Identifier: Apache-2.0
use super::{group, recipe_edge_operand, resolved_edge_treatment_group};

#[test]
fn standard_recipe_group_keeps_proven_members_and_unresolved_recipe_identity() {
    let mut selection_group = group(2, 10);
    selection_group
        .try_set_members(vec![
            crate::records::identity::Located {
                value: 10,
                offset: 0,
            },
            crate::records::identity::Located {
                value: 11,
                offset: 11,
            },
        ])
        .unwrap();
    let mut first = recipe_edge_operand(10, &[], &[]);
    first.recipe_state_id = Some(7);
    first.recipe_structure = Some(
        crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
            root: 1,
            sides: vec![
                crate::records::topology::edge_recipe::DesignTopologyRecipeSide {
                    header_value: 1,
                    scalars: Vec::new(),
                    payload_prefix: vec![0],
                    entries: Vec::new(),
                },
            ],
        },
    );
    first.resolved_edge_slot = Some(17);
    let mut second = recipe_edge_operand(11, &[], &[]);
    second.recipe_state_id = Some(7);
    second.recipe_structure = first.recipe_structure.clone();
    second.terminal_reference_edge_slots = vec![vec![18], vec![19]];
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#fillet").unwrap();
    let selected = resolved_edge_treatment_group(
        &selection_group,
        std::slice::from_ref(&selection_group),
        &[first, second.clone()],
        &[],
        Some(7),
        &feature_id,
        None,
    )
    .unwrap();
    let cadmpeg_ir::features::EdgeSelection::HistoricalPartial {
        edges, unresolved, ..
    } = selected
    else {
        panic!("partial historical selection");
    };
    assert_eq!(
        edges.as_slice(),
        &[crate::ids::history_input_edge_id(
            &crate::ids::feature_input_prefix(&feature_id, 7),
            17
        )]
    );
    assert_eq!(unresolved.as_slice(), &[second.id]);
}
