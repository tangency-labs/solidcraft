// SPDX-License-Identifier: Apache-2.0
//! `Mirror` scope projection.

use super::pattern_seeds::pattern_group_seeds;
use super::unique_feature_match;
use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::{
    feature::{mirror::DesignMirrorAssumedPlaneScope, scope::DesignParameterScope},
    topology::{
        construction::DesignConstructionOperandGroup,
        entity_selection::DesignEntitySelectionOperand, extrude_selection::DesignOperandRole,
        face::DesignFaceOperand,
    },
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;

pub(super) fn project_mirror(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    entity_selection_operands: &[DesignEntitySelectionOperand],
    scopes: &[DesignParameterScope],
    losses: &mut Vec<LossNote>,
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        patterns::{PatternKind, PatternSeed, PatternTransform},
        FeatureDefinition, FeatureOperation,
    };

    let construction = scope.mirror_construction()?;
    let stream = native_stream(&scope.id)?;
    let matching_groups = || {
        groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        })
    };
    let seed_group = unique_feature_match(matching_groups().filter(|group| {
        group.record_index == construction.seed_group_record_index
            && (matches!(
                group.role(),
                DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B
            ) || crate::design::is_pattern_face_seed_group(&scope.kind(), group))
            && !group.members().is_empty()
    }));
    let plane_group = unique_feature_match(matching_groups().filter(|group| {
        group.record_index == construction.plane_group_record_index
            && group.role() == DesignOperandRole::ROLE_0X5
            && group.members().len() == 1
    }));
    let (Some(seed_group), Some(_plane_group)) = (seed_group, plane_group) else {
        return None;
    };
    let seeds = if let Some(record_index) = construction
        .seed_feature_scope_record_index
        .map(|reference| reference.value)
    {
        let seed_scope = unique_feature_match(scopes.iter().filter(|candidate| {
            native_stream(&candidate.id) == Some(stream) && candidate.record_index == record_index
        }));
        let seed_scope = seed_scope?;
        vec![PatternSeed::Feature(crate::ids::neutral_feature_id(
            seed_scope,
        ))]
    } else {
        pattern_group_seeds(
            scope,
            seed_group,
            face_operands,
            entity_selection_operands,
            scopes,
        )
    };
    let assumed_plane_scope = match (&construction.plane, &construction.plane_scope_record_index) {
        (None, None) => construction.assumed_plane_scope.as_ref(),
        _ => None,
    };
    let (plane_origin, plane_normal, scale_origin) = match construction.plane {
        Some(plane) => (plane.origin().get(), *plane.normal().as_raw(), false),
        None => {
            let plane_scope_record_index = construction
                .plane_scope_record_index
                .map(|value| value.value)
                .or(assumed_plane_scope.map(|assumed| assumed.scope_record_index))?;
            let plane = unique_feature_match(scopes.iter().filter(|candidate| {
                native_stream(&candidate.id) == Some(stream)
                    && candidate.record_index == plane_scope_record_index
                    && candidate.kind()
                        == crate::records::feature::scope::DesignFeatureKind::WorkPlane
                    && candidate.work_plane_transform().is_some()
            }));
            let plane = plane?;
            let transform = plane.work_plane_transform()?;
            (
                Point3::new(transform[0][3], transform[1][3], transform[2][3]),
                Vector3::new(transform[0][2], transform[1][2], transform[2][2]),
                true,
            )
        }
    };
    let origin_scale = if scale_origin { 10.0 } else { 1.0 };
    let plane_origin = cadmpeg_ir::features::FinitePoint3::new(Point3::new(
        plane_origin.x * origin_scale,
        plane_origin.y * origin_scale,
        plane_origin.z * origin_scale,
    ))?;
    let plane_normal = cadmpeg_ir::features::FeatureDirection3::new(plane_normal)?;
    let pattern = PatternKind::new(PatternTransform::Mirror {
        plane_origin,
        plane_normal,
    })
    .ok()?;
    if let Some(assumed) = assumed_plane_scope {
        losses.push(mirror_plane_work_plane_assumed_loss(scope, assumed));
    }
    Some(FeatureDefinition::Operation(FeatureOperation::Pattern {
        seeds,
        pattern,
    }))
}

fn mirror_plane_work_plane_assumed_loss(
    scope: &DesignParameterScope,
    assumed: &DesignMirrorAssumedPlaneScope,
) -> LossNote {
    let faces = if assumed.unusable_faces.is_empty() {
        "no face candidate".to_owned()
    } else {
        assumed
            .unusable_faces
            .iter()
            .map(|face| {
                format!(
                    "face {} of history {} ({})",
                    face.face_slot,
                    face.history_id,
                    face.defect.describe()
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    F3dLossCode::FeatureMirrorPlaneWorkPlaneAssumed.note(format!(
        "Mirror scope {} reads its plane from WorkPlane scope {} after its plane selection identity at byte {}; passed-over face candidates: {faces}.",
        scope.id, assumed.scope_record_index, assumed.identity_offset,
    ))
}
