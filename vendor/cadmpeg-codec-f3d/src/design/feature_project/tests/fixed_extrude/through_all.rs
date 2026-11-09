// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::test_support::parameter_record;
use crate::layout::class_349_two_sided_through_all_extrude_frame as layout;
use crate::records::identity::ReferenceRun;

#[test]
fn two_sided_through_all_projects_both_terminations_and_their_owned_tapers() {
    let mut scope = fixed_extrude(
        DesignExtrudeExtent::TwoSidedThroughAll,
        DesignFixedExtrudeParameters::default(),
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = cadmpeg_core::decode::u64_from_index(layout::LEN);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset =
                draft.byte_offset + cadmpeg_core::decode::u64_from_index(layout::REFERENCE_COUNT);
            draft.reference_members =
                ReferenceRun::unlocated(vec![100, 101, 102, 103, 104, 105, 44, 107, 108, 45, 110]);
            draft.layout_fixture_references();
            draft.feature_ordinal_offset =
                draft.byte_offset + cadmpeg_core::decode::u64_from_index(layout::FEATURE_ORDINAL);
            if let DesignScopePayload::Extrude(Some(extrude)) = &mut draft.payload {
                extrude.fixed_extrude_parameters = None;
                if let Some(DesignExtrudePrologue::ReferenceAware {
                    direction_face_extend_values,
                    side_extent_discriminators,
                    ..
                }) = &mut extrude.extrude_prologue
                {
                    *direction_face_extend_values = [2, 0];
                    *side_extent_discriminators = [4, 4];
                }
            }
        })
        .unwrap();
    let parameter = |owner, kind, value| {
        parse_design_parameter_record(&parameter_record(
            Some(owner),
            "value",
            kind,
            Some("rad"),
            "d1",
            value,
        ))
        .unwrap()
    };
    let first = parameter(44, "TaperAngle", 0.2);
    let second = parameter(45, "Side2TaperAngle", -0.1);
    let placement = placement();
    let project = |parameters: &[(u32, &crate::records::parameters::DesignParameter)]| {
        crate::test_support::with_decode_context(|ctx| {
            project_extrude(
                ctx,
                &scope,
                parameters,
                &[],
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .unwrap()
    };
    let result = project(&[(0, &first), (1, &second)]).unwrap();
    assert!(matches!(
        result,
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            extent: ExtrudeExtent::TwoSided {
                first: ExtrudeSide { termination: LinearTermination::ThroughAll {}, draft: Some(a) },
                second: ExtrudeSide { termination: LinearTermination::ThroughAll {}, draft: Some(b) },
            }, ..
        }) if a.get() == 0.2 && b.get() == -0.1
    ));
    assert!(project(&[(0, &first)]).is_none());
    let wrong_owner = parameter(44, "Side2TaperAngle", -0.1);
    assert!(project(&[(0, &first), (1, &wrong_owner)]).is_none());
    let distance = parameter(46, "AlongDistance", 1.0);
    assert!(project(&[(0, &first), (1, &second), (2, &distance)]).is_none());
}
