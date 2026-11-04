// SPDX-License-Identifier: Apache-2.0
use crate::records::bodies::{DesignBodyBinding, DesignBodyBindingWire};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

fn fixture() -> (DesignParameterScope, DesignBodyBinding) {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::BaseFeature,
        10,
    );
    let binding = DesignBodyBinding::try_from(DesignBodyBindingWire {
        component_owner: None,
        component_owner_unresolved: false,
        history_only: false,
        id: "f3d:Design/BulkStream.dat:design-body-binding#10".to_owned(),
        stream: "Design/BulkStream.dat".to_owned(),
        pair_count: 1,
        pair_ordinal: 0,
        asm_body_key: 7,
        asm_body_key_offset: 10,
        entity_suffix: 20,
        entity_suffix_offset: 18,
        blob_name: "BREP.body".to_owned(),
        blob_name_offset: 30,
        body: Some(cadmpeg_ir::ids::BodyId::try_from("f3d:model:body#1").unwrap()),
    })
    .unwrap();
    (scope, binding)
}

fn project_copied_body(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    binding: &DesignBodyBinding,
) -> Result<Vec<cadmpeg_ir::features::Feature>, CodecError> {
    let timeline = crate::records::entity_header::DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream("f3d:Design/BulkStream.dat", 0),
        crate::records::entity_header::DesignTimelineFrame::test_items(
            0,
            vec![crate::records::identity::Located {
                value: 10,
                offset: 0,
            }],
        ),
        crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        std::num::NonZeroU64::new(1).unwrap(),
        0,
        std::num::NonZeroU64::new(1).unwrap(),
    )
    .unwrap();
    crate::design::feature_project::project_parameter_design_with_edge_identities(
        ctx,
        &crate::design::feature_project::ProjectInputs {
            scopes: std::slice::from_ref(scope),
            timelines: std::slice::from_ref(&timeline),
            body_bindings: std::slice::from_ref(binding),
            ..Default::default()
        },
    )
    .map(|projected| projected.features)
}

fn copied_body_fixture() -> (DesignParameterScope, DesignBodyBinding) {
    use crate::records::feature::body_ops::{DesignCopiedBody, DesignCopyPasteBodiesOperation};
    let (_, binding) = fixture();
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::CopyPasteBodies,
        10,
    );
    scope.feature_ordinal = std::num::NonZeroU32::new(1).unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::CopyPasteBodies(slot) =
        scope.payload_mut()
    {
        *slot = Some(
            DesignCopyPasteBodiesOperation::try_new(
                vec![DesignCopiedBody {
                    operand: crate::records::identity::Located {
                        value: 502,
                        offset: 26,
                    },
                    source: crate::records::identity::Located {
                        value: 10,
                        offset: 25,
                    },
                    copied: crate::records::identity::Located {
                        value: 20,
                        offset: 40,
                    },
                }],
                crate::records::feature::body_ops::CopyPasteRecordLocation {
                    record_index: 501,
                    class_tag: crate::records::references::DesignClassTag::try_from(
                        "264".to_owned(),
                    )
                    .unwrap(),
                    byte_offset: 0,
                },
                crate::records::feature::body_ops::CopyPasteRecordLocation {
                    record_index: 503,
                    class_tag: crate::records::references::DesignClassTag::try_from(
                        "264".to_owned(),
                    )
                    .unwrap(),
                    byte_offset: 0,
                },
            )
            .unwrap(),
        );
    }
    (scope, binding)
}

#[test]
fn copied_body_output_preserves_resolved_body() {
    let (scope, binding) = copied_body_fixture();
    let features = crate::test_support::with_decode_context(|decode_ctx| {
        project_copied_body(decode_ctx, &scope, &binding)
    })
    .unwrap();
    assert_eq!(features.len(), 1);
    assert_eq!(
        features[0].evaluation.outputs().as_slice(),
        [binding.body.as_ref().unwrap().clone()]
    );
}

#[test]
fn stored_body_map_requires_complete_unique_membership() {
    use cadmpeg_ir::features::BodySelection;
    let (scope, binding) = fixture();
    let selected = super::super::design_body_selection(
        &scope,
        [20].into_iter(),
        std::slice::from_ref(&binding),
    );
    assert!(matches!(selected, BodySelection::Resolved { bodies, .. }
        if bodies.as_slice() == [binding.body.clone().unwrap()]));
    for body in [
        None,
        Some(cadmpeg_ir::ids::BodyId::try_from("f3d:model:body#2").unwrap()),
    ] {
        let mut other = binding.clone();
        other.body = body;
        let selected = super::super::design_body_selection(
            &scope,
            [20].into_iter(),
            &[binding.clone(), other],
        );
        assert!(matches!(selected, BodySelection::Native(_)));
    }
    let selected = super::super::design_body_selection(
        &scope,
        [21].into_iter(),
        std::slice::from_ref(&binding),
    );
    assert!(matches!(selected, BodySelection::Native(_)));
    let selected = super::super::design_body_selection(&scope, [].into_iter(), &[]);
    assert!(matches!(selected, BodySelection::Resolved { bodies, .. } if bodies.is_empty()));
}
