// SPDX-License-Identifier: Apache-2.0

use crate::design::feature_project::reverse_normal::project_reverse_normal;
use crate::loss::F3dLossCode;
use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};
use crate::records::feature::body_ops::DesignReverseNormalOperation;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::identity::{Located, ReferenceRun};
use crate::records::references::DesignClassTag;
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
    DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
    DesignConstructionOperandRole,
};
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;

const STREAM: &str = "f3d:native/BulkStream.dat";
const SCOPE: u32 = 700;
const GROUP: u32 = 704;
const BODIES: [u32; 2] = [707, 711];
const WHOLE_BODY_GUID: &str = "9716F783-676D-42E0-93F9-EBF273E7C035";
const OTHER_GUID: &str = "5A1BF548-241F-46FD-9FB5-E4B05126EB9D";

fn scope(body_group: Option<u32>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#{SCOPE}"),
        DesignFeatureKind::ReverseNormal,
        SCOPE,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(vec![GROUP, BODIES[0], BODIES[1]]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let DesignScopePayloadMut::ReverseNormal(slot) = scope.payload_mut() {
        *slot = body_group.map(|value| DesignReverseNormalOperation {
            body_group: Located { value, offset: 26 },
        });
    }
    scope
}

fn group(scope_reference_ordinal: u32, members: &[u32]) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("{STREAM}:design-construction-operand-group#{GROUP}"),
        scope_record_index: SCOPE,
        scope_reference_ordinal,
        record_index: GROUP,
        byte_offset: 0,
        class_tag: DesignClassTag::try_from("329".to_owned()).unwrap(),
        members: members
            .iter()
            .copied()
            .enumerate()
            .map(|(index, value)| Located {
                value,
                offset: u64_from_index(index) * 11,
            })
            .collect(),
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 0,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 1,
                opaque_index_offset: 18,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 22,
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_A),
        role_offset: 0,
        paired_class_tag: DesignClassTag::try_from("268".to_owned()).unwrap(),
        paired_byte_offset: 0,
    })
    .unwrap()
}

fn type_row(guid: &str, version: u32, byte_offset: u64, members: &[u32]) -> SegmentType {
    SegmentType::try_new(
        format!("f3d:native/MetaStream.dat:design-type#{byte_offset}"),
        SegmentTypeData {
            byte_offset,
            type_guid: guid.to_owned().try_into().unwrap(),
            type_guid_offset: 0,
            base_type_guid: BaseTypeGuid::Absent,
            version,
            version_offset: 0,
            module: "Fusion".to_owned(),
            entities: ReferenceRun::unlocated(members.iter().copied().map(u64::from).collect()),
        },
    )
    .unwrap()
}

fn project(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    types: &[SegmentType],
) -> (Option<FeatureDefinition>, Vec<LossNote>) {
    let mut losses = Vec::new();
    let definition = project_reverse_normal(scope, groups, types, &mut losses);
    (definition, losses)
}

fn reversal(group: &DesignConstructionOperandGroup) -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::ReverseShape {
        source: BodySelection::Native(group.id.clone()),
    })
}

#[test]
fn whole_body_members_project_a_shape_reversal() {
    let group = group(0, &BODIES);
    let (definition, losses) = project(
        &scope(Some(GROUP)),
        std::slice::from_ref(&group),
        &[type_row(WHOLE_BODY_GUID, 1, 10, &BODIES)],
    );
    assert_eq!(definition, Some(reversal(&group)));
    assert!(losses.is_empty());
}

#[test]
fn members_without_the_whole_body_type_keep_the_reversal_with_a_loss() {
    let group = group(0, &BODIES);
    for types in [
        vec![
            type_row(WHOLE_BODY_GUID, 1, 10, &BODIES[..1]),
            type_row(OTHER_GUID, 1, 20, &BODIES[1..]),
        ],
        vec![
            type_row(WHOLE_BODY_GUID, 1, 10, &BODIES[..1]),
            type_row(WHOLE_BODY_GUID, 2, 20, &BODIES[1..]),
        ],
        vec![type_row(WHOLE_BODY_GUID, 1, 10, &BODIES[..1])],
    ] {
        let (definition, losses) =
            project(&scope(Some(GROUP)), std::slice::from_ref(&group), &types);
        assert_eq!(definition, Some(reversal(&group)));
        let [loss] = losses.as_slice() else {
            panic!("expected one loss, got {losses:?}");
        };
        assert_eq!(
            loss.code,
            F3dLossCode::FeatureReverseNormalBodyUnverified.kind()
        );
        assert_eq!(
            loss.message,
            "Reverse Normal f3d:native/BulkStream.dat:design-parameter-scope#700 reverses body-group members [711] as whole bodies; the type table registers no whole-body record type 9716F783-676D-42E0-93F9-EBF273E7C035 version 1 for them."
        );
    }
}

#[test]
fn body_group_outside_the_prefix_and_reference_table_stays_native() {
    let types = [type_row(WHOLE_BODY_GUID, 1, 10, &BODIES)];
    for (scope, group) in [
        (scope(None), group(0, &BODIES)),
        (scope(Some(BODIES[0])), group(0, &BODIES)),
        (scope(Some(GROUP)), group(1, &BODIES)),
        (scope(Some(GROUP)), group(0, &[BODIES[1], BODIES[0]])),
        (scope(Some(GROUP)), group(0, &BODIES[..1])),
    ] {
        let (definition, losses) = project(&scope, &[group], &types);
        assert_eq!(definition, None);
        assert!(losses.is_empty());
    }
}
