// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::report::loss::LossKind;

use super::{check_fallback_faces, LaneFit, OperandLanes, Withdrawals};
use crate::history::topology_cache::TopologyQueryCache;
use crate::loss::F3dLossCode;
use crate::records::topology::face::DesignFaceOperand;
use crate::test_support::history::{bounded_face_operand, box_topology, recipe_operand_reference};

/// One recipe operand per lane, each naming its faces.
fn operand(lanes: &[&[i64]]) -> DesignFaceOperand {
    bounded_face_operand(
        lanes
            .iter()
            .zip(0..)
            .map(|(faces, ordinal)| recipe_operand_reference(ordinal, "1", 300, faces))
            .collect(),
    )
}

fn fit_of(operand: &DesignFaceOperand, faces: &[i64]) -> LaneFit {
    let topology = box_topology();
    let mut queries = TopologyQueryCache::default();
    OperandLanes::of(operand, &topology, &mut queries)
        .unwrap()
        .fit(faces, &topology, &mut queries)
}

fn fit(faces: &[i64], lanes: &[&[i64]]) -> LaneFit {
    fit_of(&operand(lanes), faces)
}

/// The slots and loss code that the check leaves for `slots`.
fn check(slots: &[i64], lanes: &[&[i64]]) -> (Vec<i64>, Option<LossKind>) {
    let topology = box_topology();
    let mut queries = TopologyQueryCache::default();
    let mut operand = operand(lanes);
    operand.resolved_face_slots = slots.to_vec();
    let mut withdrawals = Withdrawals::default();
    check_fallback_faces(&mut operand, &topology, &mut queries, &mut withdrawals);
    (
        operand.resolved_face_slots,
        withdrawals.losses.first().map(|loss| loss.code.clone()),
    )
}

#[test]
fn a_face_that_touches_every_operand_lane_fits() {
    assert_eq!(fit(&[10], &[&[20], &[30]]), LaneFit::Touches);
}

#[test]
fn the_first_lane_apart_from_the_faces_is_reported() {
    assert_eq!(fit(&[40], &[&[20], &[10], &[30]]), LaneFit::Apart(1));
    assert_eq!(fit(&[20], &[&[10], &[30, 40], &[30]]), LaneFit::Apart(2));
}

#[test]
fn a_lane_that_holds_the_face_or_one_neighbour_fits() {
    assert_eq!(fit(&[40], &[&[10, 40]]), LaneFit::Touches);
    assert_eq!(fit(&[10], &[&[40, 20]]), LaneFit::Touches);
}

#[test]
fn a_face_set_fits_as_one_region() {
    // The back 30 touches the bottom 40 but not the front 20.
    assert_eq!(fit(&[20], &[&[30]]), LaneFit::Apart(0));
    assert_eq!(fit(&[20, 40], &[&[30]]), LaneFit::Touches);
}

#[test]
fn the_names_of_one_recipe_operand_form_one_lane() {
    let separate = operand(&[&[10], &[20]]);
    assert_eq!(fit_of(&separate, &[40]), LaneFit::Apart(0));
    let mut joined = separate;
    joined.recipe_references[1].operand_offset = joined.recipe_references[0].operand_offset;
    assert_eq!(fit_of(&joined, &[40]), LaneFit::Touches);
}

#[test]
fn negative_design_references_take_no_part() {
    let mut operand = operand(&[&[10], &[20]]);
    operand.recipe_references[0].design_reference = -1;
    assert_eq!(fit_of(&operand, &[40]), LaneFit::Touches);
    // A negative reference that names a face leaves its lane unread.
    operand.recipe_references[0].design_reference = 0;
    assert_eq!(fit_of(&operand, &[40]), LaneFit::Apart(0));
    let mut named = operand.clone();
    named
        .recipe_references
        .push(recipe_operand_reference(0, "1", -1, &[20]));
    assert_eq!(fit_of(&named, &[40]), LaneFit::Touches);
}

#[test]
fn lanes_outside_the_topology_and_unreadable_lane_faces_contradict_nothing() {
    assert_eq!(fit(&[10], &[&[99]]), LaneFit::Touches);
    assert_eq!(fit(&[10], &[&[50]]), LaneFit::Touches);
    assert_eq!(fit(&[10], &[&[40, 50]]), LaneFit::Touches);
    assert_eq!(fit(&[10], &[&[]]), LaneFit::Touches);
}

#[test]
fn a_lane_with_a_name_outside_the_topology_contradicts_nothing() {
    assert_eq!(fit(&[10], &[&[40]]), LaneFit::Apart(0));
    assert_eq!(fit(&[10], &[&[40, 99]]), LaneFit::Touches);
}

#[test]
fn a_reference_without_faces_voids_its_lane() {
    let mut operand = operand(&[&[10], &[20]]);
    assert_eq!(fit_of(&operand, &[40]), LaneFit::Apart(0));
    operand
        .recipe_references
        .push(recipe_operand_reference(0, "1", 300, &[]));
    assert_eq!(fit_of(&operand, &[40]), LaneFit::Touches);
}

#[test]
fn faces_whose_boundary_cannot_be_read_are_unreadable() {
    assert_eq!(fit(&[50], &[&[40]]), LaneFit::Unreadable);
    assert_eq!(fit(&[99], &[&[40]]), LaneFit::Unreadable);
    assert_eq!(fit(&[10, 50], &[&[40]]), LaneFit::Unreadable);
}

#[test]
fn unnumbered_recipe_operands_give_no_lanes() {
    let mut operand = operand(&[&[10]]);
    operand.recipe_references[0].operand_offset = None;
    operand.recipe_references[0].token_offset = operand.recipe_references[0].selector_offset + 4;
    let topology = box_topology();
    let mut queries = TopologyQueryCache::default();
    assert!(OperandLanes::of(&operand, &topology, &mut queries).is_none());
}

#[test]
fn a_fitting_or_unreadable_fallback_stays() {
    assert_eq!(check(&[10], &[&[20], &[30]]), (vec![10], None));
    assert_eq!(check(&[50], &[&[40]]), (vec![50], None));
}

#[test]
fn an_empty_fallback_stays_empty() {
    assert_eq!(check(&[], &[&[10]]), (vec![], None));
}

#[test]
fn a_failing_face_or_face_set_is_withdrawn() {
    let contradicted = Some(F3dLossCode::FeatureFaceOperandFallbackContradicted.kind());
    assert_eq!(check(&[60], &[&[40]]), (vec![], contradicted.clone()));
    assert_eq!(check(&[10, 60], &[&[40]]), (vec![], contradicted));
}

#[test]
fn a_withdrawal_keeps_its_faces_by_operand() {
    let topology = box_topology();
    let mut queries = TopologyQueryCache::default();
    let mut operand = operand(&[&[40]]);
    operand.resolved_face_slots = vec![60];
    let mut withdrawals = Withdrawals::default();
    check_fallback_faces(&mut operand, &topology, &mut queries, &mut withdrawals);
    assert!(withdrawals.withdrew(&operand));
    // A later face must hold no withdrawn face and touch the lane of 40.
    assert!(!withdrawals.admits(&operand, &[60, 20], &topology, &mut queries));
    assert!(!withdrawals.admits(&operand, &[60], &topology, &mut queries));
    assert!(withdrawals.admits(&operand, &[20], &topology, &mut queries));
    assert!(!withdrawals.admits(&operand, &[10], &topology, &mut queries));
    let other = self::operand(&[&[40]]);
    let mut other = other;
    other.id = "another operand".into();
    assert!(withdrawals.admits(&other, &[60], &topology, &mut queries));
}

#[test]
fn a_group_member_needs_its_own_face_to_touch_its_lanes() {
    let topology = box_topology();
    let mut queries = TopologyQueryCache::default();
    // The back 30 touches the top 10 but not the front 20.
    let mut withdrawn = operand(&[&[30]]);
    withdrawn.resolved_face_slots = vec![60];
    let mut withdrawals = Withdrawals::default();
    check_fallback_faces(&mut withdrawn, &topology, &mut queries, &mut withdrawals);
    let mut other = operand(&[&[30]]);
    other.id = "another operand".into();
    // Only the other member's face touches the withdrawn member's lane.
    assert!(!withdrawals.admits_pairs(&[(&withdrawn, 20), (&other, 10)], &topology, &mut queries));
    assert!(withdrawals.admits_pairs(&[(&withdrawn, 10), (&other, 20)], &topology, &mut queries));
    // The group may not hand the withdrawn face to another member.
    assert!(!withdrawals.admits_pairs(&[(&withdrawn, 10), (&other, 60)], &topology, &mut queries));
}

#[test]
fn a_node_without_a_parsed_structure_names_no_lane() {
    use crate::records::topology::face::DesignFaceRecipeNode;
    // Lanes 0 and 1 touch the top 10; lane 2 names the bottom 40.
    let mut operand = operand(&[&[20], &[30], &[40]]);
    assert_eq!(fit_of(&operand, &[10]), LaneFit::Apart(2));
    let structure = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::operands::face_recipe_structure(
            ctx,
            &[
                0, -1, 1, -1, 2, -1, 3, 0, -1, 1, -1, 1, -1, 0, 0, -1, 3, 0, -1, 1, -1, 1, -1, 0,
                0, -1,
            ],
        )
        .unwrap()
        .unwrap()
    });
    operand.recipe_nodes = vec![
        DesignFaceRecipeNode {
            byte_offset: 0,
            end_byte_offset: 12,
            program: Vec::new(),
            recipe_structure: Some(structure),
        },
        DesignFaceRecipeNode {
            byte_offset: 12,
            end_byte_offset: 24,
            program: Vec::new(),
            recipe_structure: None,
        },
    ];
    assert_eq!(fit_of(&operand, &[10]), LaneFit::Touches);
}
