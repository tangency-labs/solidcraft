// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::path_features::{
    DesignPathFeatureConstruction, DesignRevolveConstruction, DesignRevolveTargets,
    DesignRevolveTravel,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::Located;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::edge_identity::DesignEdgeOperand;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::features::{
    AngularTermination, FaceSelection, FeatureDefinition, FeatureOperation, RevolveExtent,
};

const SIDE_ONE: u32 = 102;
const SIDE_TWO: u32 = 103;

fn scope(travel: DesignRevolveTravel) -> DesignParameterScope {
    let mut scope =
        DesignParameterScope::empty("f3d:test:scope#12", DesignFeatureKind::Revolve, 12);
    scope
        .try_edit(|draft| {
            draft.payload = DesignPathFeatureConstruction::Revolve(DesignRevolveConstruction {
                operation: crate::records::feature::extrude::DesignExtrudeOperation::NewBody,
                operation_offset: 0,
                travel,
                angle_record_index: 300,
                angle_offset: 0,
                second_side: None,
                opposite_angle: None,
                solid: Some(true),
                reversed: false,
            })
            .into();
        })
        .unwrap();
    scope
}

fn targets(side_two: Option<u32>) -> DesignRevolveTravel {
    DesignRevolveTravel::ToTargets(DesignRevolveTargets {
        side_one: Located {
            value: SIDE_ONE,
            offset: 74,
        },
        side_two: side_two.map(|value| Located { value, offset: 62 }),
    })
}

fn group(
    record_index: u32,
    member: u32,
    role: DesignOperandRole,
) -> DesignConstructionOperandGroup {
    serde_json::from_value(serde_json::json!({
        "id": format!("f3d:test:group#{record_index}"),
        "scope_record_index": 12,
        "scope_reference_ordinal": record_index - 100,
        "record_index": record_index,
        "byte_offset": 1000,
        "class_tag": "332",
        "members": [member],
        "member_offsets": [1026],
        "frame": {
            "member_count_offset": 1021,
            "opaque_index": 1,
            "opaque_index_offset": 1072,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 1076,
            "variant": false
        },
        "role": role.raw(),
        "role_offset": 1054,
        "paired_class_tag": "259",
        "paired_byte_offset": 1125
    }))
    .unwrap()
}

fn axis() -> DesignEdgeOperand {
    serde_json::from_value(serde_json::json!({
        "id": "f3d:test:edge-operand#201",
        "scope_record_index": 12,
        "scope_reference_ordinal": 1,
        "record_index": 201,
        "byte_offset": 0,
        "class_tag": "376",
        "paired_byte_offset": 16,
        "paired_class_tag": "260",
        "recipe_record_index": 204,
        "recipe_record_byte_offset": 32,
        "recipe_id": "f3d:test:recipe#204",
        "recipe_prefix_offset": 43,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_program_offset": 0,
        "recipe_program": [-1, -1, 2],
        "resolved_axis_origin": [0.0, 0.0, 0.0],
        "resolved_axis_direction": [0.0, 0.0, 1.0],
        "next_record_index": 205,
        "next_byte_offset": 160
    }))
    .unwrap()
}

fn groups(target_records: &[u32]) -> Vec<DesignConstructionOperandGroup> {
    let mut groups = vec![
        group(100, 200, DesignOperandRole::PROFILE),
        group(101, 201, DesignOperandRole::ROLE_0X21),
    ];
    groups.extend(
        target_records
            .iter()
            .map(|record| group(*record, record + 100, DesignOperandRole::ROLE_0X5)),
    );
    groups
}

fn extent(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) -> Option<RevolveExtent> {
    let axis = axis();
    let FeatureDefinition::Operation(FeatureOperation::Revolve { construction, .. }) =
        super::super::project_fixed_revolve_with_entities(
            scope,
            groups,
            std::slice::from_ref(&axis),
            &[],
            &[],
            (&[], &[]),
        )?
    else {
        panic!("expected a revolve");
    };
    construction.extent().cloned()
}

#[test]
fn a_one_sided_target_projects_a_native_target_shape() {
    let groups = groups(&[SIDE_ONE]);
    assert_eq!(
        extent(&scope(targets(None)), &groups),
        Some(RevolveExtent::OneSided {
            termination: AngularTermination::ToShape {
                target: FaceSelection::Native(groups[2].id.clone()),
            },
        })
    );
}

#[test]
fn a_two_sided_scope_projects_both_sides_unresolved() {
    assert_eq!(
        extent(
            &scope(targets(Some(SIDE_TWO))),
            &groups(&[SIDE_TWO, SIDE_ONE])
        ),
        Some(RevolveExtent::TwoSided {
            first: AngularTermination::Unresolved {},
            second: AngularTermination::Unresolved {},
        })
    );
}

#[test]
fn every_target_needs_its_own_target_group() {
    let one_sided = scope(targets(None));
    assert_eq!(extent(&one_sided, &groups(&[])), None);
    assert_eq!(extent(&one_sided, &groups(&[SIDE_TWO])), None);
    assert_eq!(extent(&one_sided, &groups(&[SIDE_ONE, SIDE_TWO])), None);
    assert_eq!(
        extent(&scope(targets(Some(SIDE_TWO))), &groups(&[SIDE_ONE])),
        None
    );
    let angle = scope(DesignRevolveTravel::Angle(
        cadmpeg_ir::scalar::PositiveAngle::new(1.0).unwrap(),
    ));
    assert!(extent(&angle, &groups(&[])).is_some());
    assert_eq!(extent(&angle, &groups(&[SIDE_ONE])), None);
}

#[test]
fn a_target_group_is_the_named_record_with_one_member() {
    let mut two_members = groups(&[SIDE_ONE]);
    let member = two_members[2].members()[0];
    two_members[2]
        .try_set_members(vec![
            member,
            Located {
                value: member.value + 1,
                offset: member.offset + 11,
            },
        ])
        .unwrap();
    assert_eq!(extent(&scope(targets(None)), &two_members), None);
    assert_eq!(
        extent(&scope(targets(Some(SIDE_TWO))), &groups(&[SIDE_ONE, 104])),
        None
    );
}
