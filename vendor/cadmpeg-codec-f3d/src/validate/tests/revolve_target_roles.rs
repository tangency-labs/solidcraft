// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features::{
    DesignPathFeatureConstruction, DesignRevolveConstruction, DesignRevolveTargets,
    DesignRevolveTravel,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::{Located, ReferenceRun};
use crate::records::topology::construction::DesignConstructionOperandRole;
use crate::records::topology::extrude_selection::DesignOperandRole;

const STREAM: &str = "f3d:Design/BulkStream.dat";
const OWNER: u32 = 108;

fn to_targets(side_one: u32, side_two: Option<u32>) -> DesignRevolveTravel {
    DesignRevolveTravel::ToTargets(DesignRevolveTargets {
        side_one: Located {
            value: side_one,
            offset: 74,
        },
        side_two: side_two.map(|value| Located { value, offset: 62 }),
    })
}

fn angle() -> DesignRevolveTravel {
    DesignRevolveTravel::Angle(cadmpeg_ir::scalar::PositiveAngle::new(1.0).unwrap())
}

fn scope(travel: DesignRevolveTravel, references: Vec<u32>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#10"),
        DesignFeatureKind::Revolve,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.payload = DesignPathFeatureConstruction::Revolve(DesignRevolveConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: 0,
                travel,
                angle_record_index: OWNER,
                angle_offset: 0,
                second_side: None,
                opposite_angle: None,
                solid: Some(true),
                reversed: false,
            })
            .into();
            draft.reference_members = ReferenceRun::unlocated(references);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

/// A Revolve scope with one group of each given record and role.
fn native(
    travel: DesignRevolveTravel,
    groups: &[(u32, DesignOperandRole)],
) -> crate::native::F3dNative {
    let mut native = super::construction_group::native(true, false);
    native.design_parameter_scopes = vec![scope(
        travel,
        vec![100, 101, 102, 103, 104, 105, 106, 107, OWNER, OWNER + 2],
    )];
    let template = native.design_construction_operand_groups.remove(0);
    for (record_index, role) in groups {
        let mut group = template.clone();
        group.id = format!("{STREAM}:design-construction-operand-group#{record_index}");
        group.record_index = *record_index;
        group.operand_role = DesignConstructionOperandRole::Other(*role);
        native.design_construction_operand_groups.push(group);
    }
    native
}

fn path_findings(native: &crate::native::F3dNative) -> usize {
    crate::test_support::with_decode_context(|decode| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let ctx = super::super::Ctx::new(&ir, native, decode);
        let mut findings = Vec::new();
        super::super::validate_path_feature_operand_roles(&ctx, &mut findings);
        findings.len()
    })
}

#[test]
fn a_to_object_revolve_admits_its_target_group_role() {
    crate::test_support::with_decode_context(|decode| {
        let ir = cadmpeg_ir::CadIr::empty();
        for (travel, finding_count) in [(to_targets(100, None), 0), (angle(), 1)] {
            let mut native = super::construction_group::native(true, false);
            native.design_parameter_scopes = vec![scope(travel, vec![100, 101])];
            native.design_construction_operand_groups[0].operand_role =
                DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X5);
            let ctx = super::super::Ctx::new(&ir, &native, decode);
            let mut findings = Vec::new();
            super::super::validate_construction_operand_groups(&ctx, &mut findings);
            assert_eq!(findings.len(), finding_count, "{findings:?}");
        }
    });
}

#[test]
fn a_to_object_revolve_has_one_target_group_for_each_target() {
    let base = [
        (100, DesignOperandRole::PROFILE),
        (102, DesignOperandRole::ROLE_0X21),
    ];
    let with =
        |extra: &[(u32, DesignOperandRole)]| base.iter().chain(extra).copied().collect::<Vec<_>>();
    let target = |record| (record, DesignOperandRole::ROLE_0X5);
    for (name, travel, groups, finding_count) in [
        ("one-sided", to_targets(104, None), with(&[target(104)]), 0),
        (
            "two-sided",
            to_targets(106, Some(104)),
            with(&[target(104), target(106)]),
            0,
        ),
        ("missing target group", to_targets(104, None), with(&[]), 1),
        (
            "other target group",
            to_targets(104, None),
            with(&[target(106)]),
            1,
        ),
        (
            "extra target group",
            to_targets(104, None),
            with(&[target(104), target(106)]),
            1,
        ),
        (
            "missing side-two group",
            to_targets(106, Some(104)),
            with(&[target(106)]),
            1,
        ),
        ("angle with target group", angle(), with(&[target(104)]), 1),
        ("angle", angle(), with(&[]), 0),
    ] {
        assert_eq!(
            path_findings(&native(travel, &groups)),
            finding_count,
            "{name}"
        );
    }
    let mut two_members = native(to_targets(104, None), &with(&[target(104)]));
    let group = two_members
        .design_construction_operand_groups
        .last_mut()
        .unwrap();
    let member = group.members()[0];
    group
        .try_set_members(vec![
            member,
            Located {
                value: member.value + 1,
                offset: member.offset + 11,
            },
        ])
        .unwrap();
    assert_eq!(path_findings(&two_members), 1, "two-member target group");
}
