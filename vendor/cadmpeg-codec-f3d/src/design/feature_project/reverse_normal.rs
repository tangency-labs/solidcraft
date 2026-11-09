// SPDX-License-Identifier: Apache-2.0

use super::{native_stream, single_operand_group};
use crate::loss::F3dLossCode;
use crate::records::entity_header::SegmentType;
use crate::records::feature::path_features::pipe::BODY_GUID;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::report::loss::LossNote;

const WHOLE_BODY_VERSION: u32 = 1;

pub(super) fn project_reverse_normal(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    design_types: &[SegmentType],
    losses: &mut Vec<LossNote>,
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};

    let operation = scope.reverse_normal_operation()?;
    let group = single_operand_group(groups, scope, DesignOperandRole::BODIES_A)?;
    let mut references = scope.reference_members().values().copied();
    if group.scope_reference_ordinal != 0
        || references.next() != Some(group.record_index)
        || group.record_index != operation.body_group.value
        || !group
            .members()
            .iter()
            .map(|member| member.value)
            .eq(references)
    {
        return None;
    }
    let types = native_stream(&scope.id)
        .and_then(|stream| stream.strip_prefix("f3d:"))
        .and_then(crate::ids::decode_identity_key_component)
        .map(|entry| crate::design::decode::meta::stream_types_by_entity(design_types, &entry))
        .unwrap_or_default();
    let unverified = group
        .members()
        .iter()
        .map(|member| member.value)
        .filter(|member| {
            types
                .get(&u64::from(*member))
                .is_none_or(|(guid, version)| {
                    !guid.eq_ignore_ascii_case(BODY_GUID) || *version != WHOLE_BODY_VERSION
                })
        })
        .collect::<Vec<_>>();
    if !unverified.is_empty() {
        losses.push(F3dLossCode::FeatureReverseNormalBodyUnverified.note(format!(
            "Reverse Normal {} reverses body-group members {unverified:?} as whole bodies; the type table registers no whole-body record type {BODY_GUID} version {WHOLE_BODY_VERSION} for them.",
            scope.id
        )));
    }
    Some(FeatureDefinition::Operation(
        FeatureOperation::ReverseShape {
            source: BodySelection::Native(group.id.clone()),
        },
    ))
}
