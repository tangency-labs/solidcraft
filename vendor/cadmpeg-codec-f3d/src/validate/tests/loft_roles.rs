// SPDX-License-Identifier: Apache-2.0

#[test]
fn a_loft_point_group_needs_one_profile_and_one_single_member_point() {
    use crate::records::feature::extrude::DesignExtrudeOperation;
    use crate::records::topology::extrude_selection::DesignOperandRole as Role;
    for (groups, valid) in [
        (vec![(Role::PROFILE, 1), (Role::ROLE_0X21, 1)], true),
        (vec![(Role::ROLE_0X21, 1), (Role::PROFILE, 1)], true),
        (vec![(Role::PROFILE, 1), (Role::ROLE_0X21, 2)], false),
        (vec![(Role::ROLE_0X43, 1), (Role::ROLE_0X21, 1)], false),
        (
            vec![
                (Role::PROFILE, 1),
                (Role::ROLE_0X21, 1),
                (Role::ROLE_0X5, 2),
            ],
            false,
        ),
        (
            vec![(Role::PROFILE, 1), (Role::PROFILE, 1), (Role::ROLE_0X21, 1)],
            false,
        ),
    ] {
        assert_eq!(
            super::super::loft_operand_roles_are_valid(DesignExtrudeOperation::NewBody, &groups),
            valid,
            "{groups:?}"
        );
    }
    assert!(!super::super::loft_operand_roles_are_valid(
        DesignExtrudeOperation::Join,
        &[
            (Role::BODIES_A, 1),
            (Role::PROFILE, 1),
            (Role::ROLE_0X21, 1)
        ]
    ));
}
