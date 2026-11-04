// SPDX-License-Identifier: Apache-2.0

use super::extrude_fallbacks::{profile_group, scope};
use crate::design::feature_project::project_fixed_loft;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features::DesignLoftConstruction;
use crate::records::feature::scope::DesignScopePayload;
use crate::records::topology::construction::DesignConstructionOperandRole;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, LoftSection};

fn loft_input(
    roles: [DesignOperandRole; 2],
) -> (
    crate::records::feature::scope::DesignParameterScope,
    [crate::records::topology::construction::DesignConstructionOperandGroup; 2],
) {
    let mut scope = scope();
    scope
        .try_edit(|draft| {
            draft.payload = DesignScopePayload::Loft(Some(DesignLoftConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: 128,
            }));
        })
        .unwrap();
    let mut first = profile_group(100, 0);
    first.operand_role = DesignConstructionOperandRole::Other(roles[0]);
    let mut second = profile_group(101, 1);
    second.operand_role = DesignConstructionOperandRole::Other(roles[1]);
    (scope, [first, second])
}

fn project(
    ctx: &DecodeContext<'_>,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
) -> Result<Option<FeatureDefinition>, CodecError> {
    project_fixed_loft(
        scope,
        &crate::design::feature_project::ProjectInputs {
            construction_groups: groups,
            ..Default::default()
        },
        ctx,
    )
}

fn assert_loft_sections(roles: [DesignOperandRole; 2], has_point: bool) {
    let (scope, groups) = loft_input(roles);
    let definition =
        crate::test_support::with_decode_context(|decode_ctx| project(decode_ctx, &scope, &groups))
            .unwrap()
            .unwrap();
    assert!(matches!(definition,
        FeatureDefinition::Operation(FeatureOperation::Loft { sections, .. })
            if sections.len() == 2
                && sections.iter().any(|section| matches!(section, LoftSection::Point(_))) == has_point
    ));
}

#[test]
fn loft_projects_point_section() {
    assert_loft_sections(
        [DesignOperandRole::ROLE_0X43, DesignOperandRole::ROLE_0X5],
        true,
    );
}

#[test]
fn loft_projects_native_sections() {
    assert_loft_sections(
        [DesignOperandRole::ROLE_0X5, DesignOperandRole::ROLE_0X5],
        false,
    );
}

#[test]
fn solevacion_projects_like_loft() {
    let (mut scope, groups) =
        loft_input([DesignOperandRole::ROLE_0X5, DesignOperandRole::ROLE_0X5]);
    scope
        .try_edit(|draft| {
            draft.payload = DesignScopePayload::Solevacion(Some(DesignLoftConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: 128,
            }));
        })
        .unwrap();
    let definition =
        crate::test_support::with_decode_context(|decode_ctx| project(decode_ctx, &scope, &groups))
            .unwrap()
            .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Loft { sections, .. }) if sections.len() == 2
    ));
}
