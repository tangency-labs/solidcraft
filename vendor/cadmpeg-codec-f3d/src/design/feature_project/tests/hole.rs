// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::feature_project::project_hole;
use crate::design::test_support::parameter_record;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

const EPS_HOLE_EXTENT_TEST: f64 = 1.0e-12;

fn hole_parameters() -> [crate::records::parameters::DesignParameter; 3] {
    let mut tip = parse_design_parameter_record(&parameter_record(
        Some(114),
        "180 deg",
        "TipAngle",
        Some("mm"),
        "d6",
        std::f64::consts::PI,
    ))
    .unwrap();
    tip.try_set_unit_value("deg".to_owned()).unwrap();
    [
        parse_design_parameter_record(&parameter_record(
            Some(94),
            "10 mm",
            "HoleDepth",
            Some("mm"),
            "d4",
            1.0,
        ))
        .unwrap(),
        parse_design_parameter_record(&parameter_record(
            Some(104),
            "4 mm",
            "HoleDiameter",
            Some("mm"),
            "d5",
            0.4,
        ))
        .unwrap(),
        tip,
    ]
}

#[test]
fn hole_without_face_operand_falls_back_to_scope_face() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#32",
        DesignFeatureKind::Hole,
        32,
    );
    set_extent(&mut scope, 0, false);
    let parameters = hole_parameters();
    let indexed = [
        (0, &parameters[0]),
        (1, &parameters[1]),
        (2, &parameters[2]),
    ];
    let definition = project_hole(&scope, &indexed, &[], &[], &mut Vec::new())
        .unwrap()
        .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Hole {
            face: Some(FaceSelection::Native(id)), ..
        }) if id == scope.id
    ));
}

fn set_extent(scope: &mut DesignParameterScope, control: u8, saved: bool) {
    if let crate::records::feature::scope::DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
        slot.extent = Some(crate::records::feature::hole::DesignHoleExtent {
            leading_word: None,
            extent_control: Some(control),
            saved_non_distance_depth: saved.then(|| crate::records::identity::Located {
                value: cadmpeg_ir::scalar::FiniteReal::try_from(1.0).unwrap(),
                offset: 5,
            }),
            direction_control: Some(crate::records::identity::Located {
                value: 0,
                offset: 6,
            }),
        });
    }
}

fn set_direction_control(scope: &mut DesignParameterScope, value: Option<u8>) {
    if let crate::records::feature::scope::DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
        if let Some(extent) = &mut slot.extent {
            extent.direction_control =
                value.map(|value| crate::records::identity::Located { value, offset: 6 });
        }
    }
}

fn set_carrier(
    scope: &mut DesignParameterScope,
    reference_type: u32,
    position: [f64; 3],
    locations: &[(u32, [f64; 3])],
    face_selection: bool,
) {
    let locations: Vec<_> = locations
        .iter()
        .enumerate()
        .map(|(index, (reference_type, position))| {
            serde_json::json!({
                "record_index": index, "position": position,
                "position_offset": index, "reference_type": reference_type
            })
        })
        .collect();
    let mut construction = serde_json::json!({
        "point_record_index": 1, "point_record_byte_offset": 2,
        "position": position, "position_offset": 3,
        "direction": [0.0, 0.0, 1.0], "direction_offset": 4,
        "point_parameters": [0.0, 0.0], "point_parameter_offsets": [5, 6],
        "reference_type": reference_type, "reference_type_offset": 7,
        "input_record_indices": [8], "input_record_offsets": [9],
        "locations": locations
    });
    if face_selection {
        construction["face_selection"] = serde_json::json!({
            "record_index": 1, "byte_offset": 2, "class_tag": "451",
            "asset_id": "11111111-1111-1111-1111-111111111111",
            "asset_id_offset": 3,
            "context_id": "22222222-2222-2222-2222-222222222222",
            "context_id_offset": 4,
            "identity_record_index": 5, "identity_record_offset": 6,
            "primary_identity": 7, "primary_identity_offset": 8,
            "next_record_index": 9, "next_byte_offset": 10
        });
    }
    if let crate::records::feature::scope::DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
        slot.point = Some(serde_json::from_value(construction).unwrap());
    }
}

fn projected_placements(
    scope: &DesignParameterScope,
) -> (
    Option<Vec<cadmpeg_ir::features::holes::HolePlacement>>,
    Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    let parameters = hole_parameters();
    let indexed = [
        (0, &parameters[0]),
        (1, &parameters[1]),
        (2, &parameters[2]),
    ];
    let mut losses = Vec::new();
    let definition = project_hole(scope, &indexed, &[], &[], &mut losses)
        .unwrap()
        .unwrap();
    let FeatureDefinition::Operation(FeatureOperation::Hole { placements, .. }) = definition else {
        panic!("Hole definition");
    };
    (placements, losses)
}

fn directed(position: [f64; 3]) -> cadmpeg_ir::features::holes::HolePlacement {
    directed_along(position, -1.0)
}

fn directed_along(position: [f64; 3], z: f64) -> cadmpeg_ir::features::holes::HolePlacement {
    cadmpeg_ir::features::holes::HolePlacement::Directed {
        position: cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            position[0],
            position[1],
            position[2],
        ))
        .unwrap(),
        direction: cadmpeg_ir::features::FeatureDirection3::new(cadmpeg_ir::math::Vector3::new(
            0.0, 0.0, z,
        ))
        .unwrap(),
    }
}

#[test]
fn hole_placements_come_from_the_carrier_form_in_millimetres_against_the_normal() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#32",
        DesignFeatureKind::Hole,
        32,
    );
    set_extent(&mut scope, 0, false);

    set_carrier(
        &mut scope,
        13,
        [0.0; 3],
        &[(10, [1.0, 1.5, 2.0]), (10, [3.0, 1.5, 2.0])],
        true,
    );
    let (placements, losses) = projected_placements(&scope);
    assert_eq!(
        placements,
        Some(vec![
            directed([10.0, 15.0, 20.0]),
            directed([30.0, 15.0, 20.0])
        ])
    );
    assert!(losses.is_empty());

    set_carrier(
        &mut scope,
        19,
        [1.0, 1.5, 2.0],
        &[(17, [1.0, 1.5, 2.0])],
        false,
    );
    let (placements, losses) = projected_placements(&scope);
    assert_eq!(placements, Some(vec![directed([10.0, 15.0, 20.0])]));
    assert!(losses.is_empty());
}

#[test]
fn hole_direction_control_selects_the_travel_of_every_placement() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#32",
        DesignFeatureKind::Hole,
        32,
    );
    set_extent(&mut scope, 0, false);
    set_carrier(
        &mut scope,
        13,
        [0.0; 3],
        &[(10, [1.0, 1.5, 2.0]), (10, [3.0, 1.5, 2.0])],
        true,
    );
    set_direction_control(&mut scope, Some(1));
    let (placements, losses) = projected_placements(&scope);
    assert_eq!(
        placements,
        Some(vec![
            directed_along([10.0, 15.0, 20.0], 1.0),
            directed_along([30.0, 15.0, 20.0], 1.0)
        ])
    );
    assert!(losses.is_empty());

    set_direction_control(&mut scope, None);
    let (placements, losses) = projected_placements(&scope);
    assert_eq!(placements, None);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::HolePlacementUnproven.kind()
    );
}

/// Carrier `refType`, carrier position, and point records.
type CarrierCase = (u32, [f64; 3], &'static [(u32, [f64; 3])]);

#[test]
fn hole_placement_without_an_admitted_form_stays_native_with_its_loss() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#32",
        DesignFeatureKind::Hole,
        32,
    );
    set_extent(&mut scope, 0, false);
    let cases: [CarrierCase; 4] = [
        (13, [0.0; 3], &[]),
        (
            13,
            [0.0; 3],
            &[(10, [1.0, 1.5, 2.0]), (17, [3.0, 1.5, 2.0])],
        ),
        (19, [1.0, 1.5, 2.0], &[(17, [3.0, 1.5, 2.0])]),
        (18, [1.0, 1.5, 2.0], &[(17, [1.0, 1.5, 2.0])]),
    ];
    for (reference_type, position, locations) in cases {
        set_carrier(&mut scope, reference_type, position, locations, false);
        let (placements, losses) = projected_placements(&scope);
        assert_eq!(placements, None);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::HolePlacementUnproven.kind()
        );
    }
}

#[test]
fn hole_without_a_carrier_reports_its_unproved_placement() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#32",
        DesignFeatureKind::Hole,
        32,
    );
    set_extent(&mut scope, 0, false);
    let (placements, losses) = projected_placements(&scope);
    assert_eq!(placements, None);
    assert_eq!(losses.len(), 1);
}

#[test]
fn through_all_admits_a_carrier_selection_only_on_a_sketch_point_carrier() {
    let parameters = hole_parameters();
    let indexed = [(1, &parameters[1]), (2, &parameters[2])];
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#32",
        DesignFeatureKind::Hole,
        32,
    );
    set_extent(&mut scope, 1, true);

    set_carrier(&mut scope, 13, [0.0; 3], &[(10, [1.0, 1.5, 2.0])], true);
    let definition = project_hole(&scope, &indexed, &[], &[], &mut Vec::new())
        .unwrap()
        .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Hole {
            extent: Some(cadmpeg_ir::features::LinearTermination::ThroughAll {}),
            ..
        })
    ));

    set_carrier(
        &mut scope,
        19,
        [1.0, 1.5, 2.0],
        &[(17, [1.0, 1.5, 2.0])],
        true,
    );
    let mut losses = Vec::new();
    assert!(project_hole(&scope, &indexed, &[], &[], &mut losses)
        .unwrap()
        .is_none());
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::HoleExtentUnproven.kind()
    );
}

fn face_operand(
    record_index: u32,
    ordinal: u32,
) -> crate::records::topology::face::DesignFaceOperand {
    use crate::records::topology::face::{DesignFaceOperand, DesignFaceOperandDraft};
    DesignFaceOperand::try_new(DesignFaceOperandDraft {
        id: format!("f3d:Design/BulkStream.dat:face-operand#{record_index}"),
        scope_record_index: 1,
        scope_reference_ordinal: ordinal,
        group: None,
        record_index,
        byte_offset: 10,
        class_tag: "256".to_owned().try_into().unwrap(),
        paired_byte_offset: 30,
        paired_class_tag: "257".to_owned().try_into().unwrap(),
        recipe_record_index: record_index + 3,
        recipe_record_byte_offset: 40,
        recipe_id: format!(
            "f3d:Design/BulkStream.dat:construction-recipe#{}",
            record_index + 3
        ),
        recipe_prefix_offset: 51,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_kind: crate::records::recipes::ConstructionRecipeKind::BoundedFace,
        recipe_program_offset: 60,
        recipe_program: vec![0, -1],
        recipe_nodes: Vec::new(),
        candidate_faces: Vec::new(),
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: Vec::new(),
        resolved_active_face: None,
        next_record_index: record_index + 20,
        next_byte_offset: 70,
    })
    .unwrap()
}

#[test]
fn hole_extent_projection_checks_distance_shoulder_and_through_all() {
    use cadmpeg_ir::features::{
        holes::{HoleBottom, HoleConstruction, HoleKind},
        LinearTermination,
    };
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#1",
        DesignFeatureKind::Hole,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::located(vec![
                crate::records::identity::Located {
                    value: 2,
                    offset: 1,
                },
                crate::records::identity::Located {
                    value: 3,
                    offset: 2,
                },
            ]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let faces = vec![face_operand(2, 0), face_operand(3, 1)];
    let mut parameters = hole_parameters().to_vec();
    for drilled in [false, true] {
        if drilled {
            parameters[2].try_set_evaluated_value(2.0).unwrap();
        }
        set_extent(&mut scope, 0, false);
        let indexed: Vec<_> = parameters
            .iter()
            .enumerate()
            .map(|(i, p)| (u32::try_from(i).unwrap(), p))
            .collect();
        let definition = project_hole(&scope, &indexed, &[], &faces, &mut Vec::new())
            .unwrap()
            .unwrap();
        let FeatureDefinition::Operation(FeatureOperation::Hole { extent, bottom, .. }) =
            definition
        else {
            panic!()
        };
        assert!(
            matches!(extent, Some(LinearTermination::Blind {length}) if (length.get()-10.0).abs() < EPS_HOLE_EXTENT_TEST)
        );
        if drilled {
            assert!(matches!(
                bottom,
                Some(HoleBottom::Angled {
                    depth_to_tip: false,
                    ..
                })
            ));
        } else {
            assert_eq!(bottom, Some(HoleBottom::Flat));
        }
    }
    parameters.remove(0);
    for counterbore in [false, true] {
        if counterbore {
            for (name, value) in [("CBDepth", 0.3), ("CBDiameter", 1.0)] {
                parameters.push(
                    parse_design_parameter_record(&parameter_record(
                        Some(5),
                        "1 mm",
                        name,
                        Some("mm"),
                        "d1",
                        value,
                    ))
                    .unwrap(),
                );
            }
        }
        set_extent(&mut scope, 1, true);
        let indexed: Vec<_> = parameters
            .iter()
            .enumerate()
            .map(|(i, p)| (u32::try_from(i).unwrap(), p))
            .collect();
        let definition = project_hole(&scope, &indexed, &[], &faces, &mut Vec::new())
            .unwrap()
            .unwrap();
        let FeatureDefinition::Operation(FeatureOperation::Hole { extent, shape, .. }) = definition
        else {
            panic!()
        };
        assert_eq!(extent, Some(LinearTermination::ThroughAll {}));
        assert_eq!(
            super::super::hole_termination(&scope, &indexed, &[], &[]),
            Some(LinearTermination::ThroughAll {})
        );
        assert_eq!(
            matches!(
                shape.construction(),
                HoleConstruction::Form {
                    kind: HoleKind::CounterboreDrilled { .. },
                    ..
                }
            ),
            counterbore
        );
    }
}

#[test]
fn hole_unproved_extent_has_loss_for_depth_extra_face_and_missing_saved_depth() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#1",
        DesignFeatureKind::Hole,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::located(vec![
                crate::records::identity::Located {
                    value: 2,
                    offset: 1,
                },
                crate::records::identity::Located {
                    value: 3,
                    offset: 2,
                },
            ]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    set_extent(&mut scope, 1, true);
    let parameters = hole_parameters();
    let indexed: Vec<_> = parameters
        .iter()
        .enumerate()
        .map(|(i, p)| (u32::try_from(i).unwrap(), p))
        .collect();
    let faces = [face_operand(2, 0), face_operand(3, 1), face_operand(5, 2)];
    for (parameters, faces) in [(&indexed[..], &faces[..2]), (&indexed[1..], &faces[..])] {
        assert!(super::super::hole_termination(&scope, parameters, &[], faces).is_none());
        let mut losses = Vec::new();
        assert!(project_hole(&scope, parameters, &[], faces, &mut losses)
            .unwrap()
            .is_none());
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::HoleExtentUnproven.kind()
        );
    }
    set_extent(&mut scope, 1, false);
    assert!(
        project_hole(&scope, &indexed[1..], &[], &faces[..2], &mut Vec::new())
            .unwrap()
            .is_none()
    );
}

#[test]
fn hole_without_an_extent_control_is_distance_only_with_its_depth() {
    use cadmpeg_ir::features::LinearTermination;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#1",
        DesignFeatureKind::Hole,
        1,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
        slot.extent = Some(crate::records::feature::hole::DesignHoleExtent {
            leading_word: None,
            extent_control: None,
            saved_non_distance_depth: None,
            direction_control: None,
        });
    }
    let parameters = hole_parameters();
    let indexed: Vec<_> = parameters
        .iter()
        .enumerate()
        .map(|(i, p)| (u32::try_from(i).unwrap(), p))
        .collect();
    assert!(matches!(
        super::super::hole_termination(&scope, &indexed, &[], &[]),
        Some(LinearTermination::Blind { .. })
    ));
    let without_depth: Vec<_> = indexed
        .iter()
        .copied()
        .filter(|(_, parameter)| parameter.source_kind() != "HoleDepth")
        .collect();
    assert_eq!(without_depth.len(), indexed.len() - 1);
    assert_eq!(
        super::super::hole_termination(&scope, &without_depth, &[], &[]),
        None
    );
    let mut losses = Vec::new();
    assert!(project_hole(&scope, &without_depth, &[], &[], &mut losses)
        .unwrap()
        .is_none());
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::HoleExtentUnproven.kind()
    );
}

fn hole_scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#32",
        DesignFeatureKind::Hole,
        32,
    );
    set_extent(&mut scope, 0, false);
    scope
}

/// A length parameter in centimetres or an angle parameter in radians.
fn named_parameter(
    name: &str,
    value: f64,
    angle: bool,
) -> crate::records::parameters::DesignParameter {
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(5),
        "1",
        name,
        Some("mm"),
        "d1",
        value,
    ))
    .unwrap();
    if angle {
        parameter.try_set_unit_value("deg".to_owned()).unwrap();
    }
    parameter
}

fn projected_hole(
    scope: &DesignParameterScope,
    parameters: &[crate::records::parameters::DesignParameter],
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
) -> Option<FeatureOperation> {
    let indexed: Vec<_> = parameters
        .iter()
        .enumerate()
        .map(|(i, p)| (u32::try_from(i).unwrap(), p))
        .collect();
    match project_hole(scope, &indexed, groups, &[], &mut Vec::new()).unwrap()? {
        FeatureDefinition::Operation(operation) => Some(operation),
        FeatureDefinition::PostProcess { .. } => panic!("expected an operation"),
    }
}

fn countersink(diameter: f64, angle: f64) -> Vec<crate::records::parameters::DesignParameter> {
    let mut parameters = hole_parameters().to_vec();
    parameters.push(named_parameter("CSDiameter", diameter, false));
    parameters.push(named_parameter("CSAngle", angle, true));
    parameters
}

#[test]
fn countersink_parameters_state_a_countersink_entry() {
    use cadmpeg_ir::features::holes::{HoleBottom, HoleConstruction, HoleKind};
    for (tip, bottom_is_flat) in [(std::f64::consts::PI, true), (118_f64.to_radians(), false)] {
        let mut parameters = countersink(1.0, std::f64::consts::FRAC_PI_2);
        parameters[2].try_set_evaluated_value(tip).unwrap();
        let Some(FeatureOperation::Hole { shape, bottom, .. }) =
            projected_hole(&hole_scope(), &parameters, &[])
        else {
            panic!("expected a hole");
        };
        let HoleConstruction::Form {
            kind: HoleKind::Countersink { diameter, angle },
            ..
        } = shape.construction()
        else {
            panic!("expected a countersink, got {:?}", shape.construction());
        };
        assert!((diameter.get() - 10.0).abs() < EPS_HOLE_EXTENT_TEST);
        assert!((angle.get() - std::f64::consts::FRAC_PI_2).abs() < EPS_HOLE_EXTENT_TEST);
        assert_eq!(matches!(bottom, Some(HoleBottom::Flat)), bottom_is_flat);
        assert_eq!(
            matches!(
                bottom,
                Some(HoleBottom::Angled {
                    depth_to_tip: false,
                    ..
                })
            ),
            !bottom_is_flat
        );
    }
}

#[test]
fn a_countersink_no_wider_than_the_hole_or_with_a_straight_angle_stays_native() {
    for parameters in [
        countersink(0.4, std::f64::consts::FRAC_PI_2),
        countersink(0.3, std::f64::consts::FRAC_PI_2),
        countersink(1.0, std::f64::consts::PI),
    ] {
        assert_eq!(projected_hole(&hole_scope(), &parameters, &[]), None);
    }
}

#[test]
fn mixed_counterbore_and_countersink_parameters_stay_native() {
    let mut parameters = hole_parameters().to_vec();
    parameters.push(named_parameter("CBDepth", 0.3, false));
    parameters.push(named_parameter(
        "CSAngle",
        std::f64::consts::FRAC_PI_2,
        true,
    ));
    assert_eq!(projected_hole(&hole_scope(), &parameters, &[]), None);
    let mut parameters = hole_parameters().to_vec();
    parameters[2]
        .try_set_evaluated_value(118_f64.to_radians())
        .unwrap();
    parameters.push(named_parameter("CSDiameter", 1.0, false));
    parameters.push(named_parameter("CBDiameter", 1.0, false));
    assert_eq!(
        projected_hole(&hole_scope(), &parameters, &[]),
        None,
        "a countersink needs its own angle"
    );
}

fn extent_group(
    role: crate::records::topology::extrude_selection::DesignOperandRole,
    members: &[u32],
) -> crate::records::topology::construction::DesignConstructionOperandGroup {
    let member_offsets: Vec<u64> = (0..u64::try_from(members.len()).unwrap())
        .map(|ordinal| 1026 + ordinal * 11)
        .collect();
    serde_json::from_value(serde_json::json!({
        "id": "f3d:Design/BulkStream.dat:group#40",
        "scope_record_index": 32,
        "scope_reference_ordinal": 8,
        "record_index": 40,
        "byte_offset": 1000,
        "class_tag": "332",
        "members": members,
        "member_offsets": member_offsets,
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

#[test]
fn an_extents_to_hole_ends_on_the_face_of_its_extent_group() {
    use crate::records::topology::extrude_selection::DesignOperandRole;
    use cadmpeg_ir::features::LinearTermination;
    let mut scope = hole_scope();
    set_extent(&mut scope, 0, true);
    let without_depth = hole_parameters()[1..].to_vec();
    let group = extent_group(DesignOperandRole::ROLE_0X5, &[7]);
    let Some(FeatureOperation::Hole { extent, .. }) =
        projected_hole(&scope, &without_depth, std::slice::from_ref(&group))
    else {
        panic!("expected a hole");
    };
    assert_eq!(
        extent,
        Some(LinearTermination::ToFace {
            face: FaceSelection::Native(group.id.clone()),
            offset: None,
        })
    );
}

#[test]
fn an_extents_to_hole_needs_one_extent_group_with_one_member() {
    use crate::records::topology::extrude_selection::DesignOperandRole;
    let mut scope = hole_scope();
    set_extent(&mut scope, 0, true);
    let without_depth = hole_parameters()[1..].to_vec();
    for groups in [
        Vec::new(),
        vec![extent_group(DesignOperandRole::ROLE_0X5, &[7, 8])],
        vec![extent_group(DesignOperandRole::BODIES_A, &[7])],
        vec![
            extent_group(DesignOperandRole::ROLE_0X5, &[7]),
            extent_group(DesignOperandRole::ROLE_0X5, &[8]),
        ],
    ] {
        assert_eq!(projected_hole(&scope, &without_depth, &groups), None);
    }
}

#[test]
fn a_hole_depth_or_an_unsaved_depth_keeps_the_distance_extent() {
    use crate::records::topology::extrude_selection::DesignOperandRole;
    use cadmpeg_ir::features::LinearTermination;
    let group = extent_group(DesignOperandRole::ROLE_0X5, &[7]);
    let mut saved = hole_scope();
    set_extent(&mut saved, 0, true);
    let Some(FeatureOperation::Hole { extent, .. }) =
        projected_hole(&saved, &hole_parameters(), std::slice::from_ref(&group))
    else {
        panic!("expected a hole");
    };
    assert!(matches!(extent, Some(LinearTermination::Blind { .. })));
    assert_eq!(
        projected_hole(
            &hole_scope(),
            &hole_parameters()[1..],
            std::slice::from_ref(&group)
        ),
        None
    );
}
