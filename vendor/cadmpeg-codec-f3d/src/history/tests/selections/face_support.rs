// SPDX-License-Identifier: Apache-2.0

use crate::history::{
    face_operand_support_candidates, historical_face_support_contexts, TopologyQueryCache,
};
use crate::history_records::{AsmHistoricalCarrierBinding, AsmHistoricalTopology};
use crate::records::dimensions::DesignRecipeReference;
use cadmpeg_ir::ids::FaceId;
use std::collections::HashSet;

fn face(slot: i64) -> FaceId {
    FaceId::mint(format!("f3d:brep:entity#{slot}")).unwrap()
}

fn reference() -> DesignRecipeReference {
    DesignRecipeReference {
        selector: 2,
        selector_offset: 0,
        token: "37".into(),
        token_offset: 4,
        design_reference: -73,
        design_reference_offset: 8,
        candidate_faces: vec![face(10)],
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    }
}

#[test]
fn extrude_support_keeps_the_first_reference_after_context_subtraction() {
    let topology = AsmHistoricalTopology {
        faces: vec![10, 11, 12, 13],
        face_surfaces: (10..14)
            .map(|entity| AsmHistoricalCarrierBinding {
                entity,
                carrier: entity + 100,
            })
            .collect(),
        ..Default::default()
    };
    let history =
        crate::test_support::history::history("f3d:history".into(), 0, None, false, Vec::new());
    let mut reference = reference();
    let preceding_candidates = vec![face(11), face(12)];
    for (keys, expected) in [
        (vec![face(10)], vec![10]),
        (vec![face(10), face(13)], vec![10, 13]),
        (Vec::new(), Vec::new()),
    ] {
        reference.candidate_faces = keys;
        let candidates =
            face_operand_support_candidates(&preceding_candidates, Some(&reference), Some(-73));
        let contexts = historical_face_support_contexts(
            &candidates,
            &history,
            &topology,
            &HashSet::new(),
            &mut TopologyQueryCache::default(),
        );
        let target_contexts = contexts
            .iter()
            .filter(|context| matches!(context.active_face_slot, 10 | 13))
            .collect::<Vec<_>>();
        assert_eq!(
            target_contexts
                .iter()
                .map(|context| context.active_face_slot)
                .collect::<Vec<_>>(),
            expected
        );
        for context in target_contexts {
            assert_eq!(context.preceding_face_slots, [context.active_face_slot]);
            assert_eq!(context.surface_slot, context.active_face_slot + 100);
        }
    }
}

#[test]
fn extrude_support_deduplicates_the_first_reference_and_uses_alternate_selectors() {
    let mut reference = reference();
    reference.alternate_selector_faces = std::mem::take(&mut reference.candidate_faces);
    let candidates = vec![face(10), face(11)];
    assert_eq!(
        face_operand_support_candidates(&candidates, Some(&reference), Some(-73)).as_ref(),
        candidates
    );
}

#[test]
fn extrude_support_requires_the_recipe_design_reference() {
    let reference = reference();
    let candidates = vec![face(11)];
    for recipe_reference in [None, Some(-74)] {
        assert_eq!(
            face_operand_support_candidates(&candidates, Some(&reference), recipe_reference)
                .as_ref(),
            candidates
        );
    }
}
