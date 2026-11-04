// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features::DesignSweepConstruction;
use crate::records::feature::scope::{DesignScopePayload, DesignSweepScope};
use crate::records::topology::construction::DesignConstructionOperandRole;
use crate::records::topology::extrude_selection::DesignOperandRole;

#[test]
fn sweep_body_roles_preserve_group_frames_and_operation_contracts() {
    crate::test_support::with_decode_context(|decode| {
        for role in [DesignOperandRole::BODIES_A, DesignOperandRole::BODIES_B] {
            for operation in [
                DesignExtrudeOperation::Join,
                DesignExtrudeOperation::Cut,
                DesignExtrudeOperation::Intersect,
                DesignExtrudeOperation::NewBody,
            ] {
                let mut native = super::construction_group::native(true, false);
                let scope = &mut native.design_parameter_scopes[0];
                scope
                    .try_edit(|draft| {
                        draft.payload = DesignScopePayload::Sweep(Some(DesignSweepScope {
                            construction: Some(DesignSweepConstruction {
                                operation,
                                operation_offset: 31,
                                values: crate::test_support::reals([1.0, 1.0, 1.0, 1.0, 0.0, 0.0])
                                    .to_vec(),
                                record_indexes: vec![211, 223, 227, 229, 233, 239],
                                value_offsets: vec![311, 323, 327, 329, 333, 339],
                            }),
                            sweep_profile: None,
                        }));
                    })
                    .unwrap();
                let scope = scope.clone();
                native.design_construction_operand_groups[0].operand_role =
                    DesignConstructionOperandRole::Other(role);
                let ir = cadmpeg_ir::examples::unit_cube().unwrap();
                let ctx = super::super::Ctx::new(&ir, &native, decode);
                let mut findings = Vec::new();
                super::super::validate_construction_operand_groups(&ctx, &mut findings);
                assert!(findings.is_empty(), "{findings:?}");
                let template = native.design_construction_operand_groups[0].clone();
                for (index, operand_role) in
                    [DesignOperandRole::PROFILE, DesignOperandRole::ROLE_0X5]
                        .into_iter()
                        .enumerate()
                {
                    let mut group = template.clone();
                    group.id = format!(
                        "f3d:Design/BulkStream.dat:design-construction-operand-group#{}",
                        317 + index
                    );
                    group.record_index = 317 + u32::try_from(index).unwrap();
                    group.operand_role = DesignConstructionOperandRole::Other(operand_role);
                    native.design_construction_operand_groups.push(group);
                }
                let ctx = super::super::Ctx::new(&ir, &native, decode);
                super::super::validate_path_feature_operand_roles(&ctx, &mut findings);
                assert_eq!(
                    findings.is_empty(),
                    operation != DesignExtrudeOperation::NewBody
                );
                assert_eq!(
                    scope.record_index,
                    native.design_construction_operand_groups[0].scope_record_index
                );
                if operation == DesignExtrudeOperation::NewBody {
                    native.design_construction_operand_groups.remove(0);
                    let ctx = super::super::Ctx::new(&ir, &native, decode);
                    let mut findings = Vec::new();
                    super::super::validate_path_feature_operand_roles(&ctx, &mut findings);
                    assert!(findings.is_empty());
                }
                native.design_construction_operand_groups.push(template);
                let ctx = super::super::Ctx::new(&ir, &native, decode);
                let mut findings = Vec::new();
                super::super::validate_path_feature_operand_roles(&ctx, &mut findings);
                assert_eq!(findings.len(), 1);
            }
        }
    });
}
