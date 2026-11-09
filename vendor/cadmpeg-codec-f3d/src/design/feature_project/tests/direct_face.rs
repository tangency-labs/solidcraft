// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::direct_face::{
    DesignOffsetFacesOperation, DesignShellOperation, DesignThickenOperation,
};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{BodySelection, FaceSelection, FeatureDefinition, FeatureOperation};

fn scope(kind: DesignFeatureKind) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty("f3d:test:scope#12", kind, 12);
    match scope.payload_mut() {
        DesignScopePayloadMut::OffsetFaces(slot) => {
            *slot = Some(DesignOffsetFacesOperation {
                distance: cadmpeg_ir::scalar::FiniteReal::new(0.25).unwrap(),
                distance_record_index: 300,
                distance_offset: 0,
            });
        }
        DesignScopePayloadMut::Thicken(slot) => {
            *slot = Some(DesignThickenOperation {
                signed_thickness: cadmpeg_ir::scalar::FiniteReal::new(0.5).unwrap(),
                thickness_record_index: 300,
                thickness_offset: 0,
            });
        }
        DesignScopePayloadMut::Shell(slot) => {
            *slot = Some(DesignShellOperation {
                thickness: cadmpeg_ir::scalar::PositiveReal::new(0.5).unwrap(),
                thickness_record_index: 300,
                thickness_offset: 0,
                outward: true,
                outward_offset: 0,
                wall_slots: None,
            });
        }
        _ => panic!("unsupported direct-face scope fixture"),
    }
    scope
}

pub(super) fn group(role: DesignOperandRole) -> DesignConstructionOperandGroup {
    serde_json::from_value(serde_json::json!({
        "id": "f3d:test:group#100",
        "scope_record_index": 12,
        "scope_reference_ordinal": 0,
        "record_index": 100,
        "byte_offset": 1000,
        "class_tag": "332",
        "members": [200],
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

fn projected_operation(
    projected: Result<Option<FeatureDefinition>, CodecError>,
) -> FeatureOperation {
    let Some(FeatureDefinition::Operation(operation)) = projected.unwrap() else {
        panic!("expected a projected operation");
    };
    operation
}

#[test]
fn offset_faces_projects_native_group() {
    let scope = scope(DesignFeatureKind::OffsetFaces);
    let group = group(DesignOperandRole::ROLE_0X10);
    let FeatureOperation::MoveFace { faces, .. } = projected_operation(
        super::super::project_offset_faces(&scope, &[], &[], std::slice::from_ref(&group)),
    ) else {
        panic!("expected a face move");
    };
    assert_eq!(faces, FaceSelection::Native(group.id.clone()));
}

#[test]
fn thicken_projects_native_group() {
    let scope = scope(DesignFeatureKind::Thicken);
    let group = group(DesignOperandRole::ROLE_0X5);
    let FeatureOperation::Thicken { faces, .. } = projected_operation(
        super::super::project_thicken(&scope, &[], std::slice::from_ref(&group)),
    ) else {
        panic!("expected a thicken");
    };
    assert_eq!(faces, FaceSelection::Native(group.id.clone()));
}

fn thicken_after_state(previous_state: i64) -> DesignParameterScope {
    let mut scope = scope(DesignFeatureKind::Thicken);
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(previous_state);
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

#[test]
fn thicken_lists_a_whole_body_group_beside_resolved_faces_as_unresolved() {
    let scope = thicken_after_state(5);
    let faces_group = group(DesignOperandRole::ROLE_0X12);
    let mut whole_body = group(DesignOperandRole::ROLE_0X5);
    whole_body.id = "f3d:test:group#110".into();
    whole_body.record_index = 110;
    let groups = [faces_group, whole_body.clone()];
    let FeatureOperation::Thicken { faces, .. } = projected_operation(
        super::super::project_thicken(&scope, &[resolved_member_operand()], &groups),
    ) else {
        panic!("expected a thicken");
    };
    let mut rebound = faces.clone();
    super::super::list_thicken_whole_bodies(&mut rebound, &scope, &groups);
    assert_eq!(rebound, faces);
    let FaceSelection::HistoricalPartial {
        faces,
        unresolved,
        native,
        ..
    } = faces
    else {
        panic!("expected a partial historical selection, got {faces:?}");
    };
    assert_eq!(faces.as_slice().len(), 1);
    assert_eq!(unresolved.as_slice(), [whole_body.id]);
    assert_eq!(native, scope.id.as_str());
}

#[test]
fn thicken_keeps_resolved_faces_without_a_whole_body_group() {
    let scope = thicken_after_state(5);
    let FeatureOperation::Thicken { faces, .. } =
        projected_operation(super::super::project_thicken(
            &scope,
            &[resolved_member_operand()],
            &[group(DesignOperandRole::ROLE_0X12)],
        ))
    else {
        panic!("expected a thicken");
    };
    assert!(
        matches!(faces, FaceSelection::Historical { .. }),
        "{faces:?}"
    );
}

#[test]
fn shell_projects_native_face_group() {
    let scope = scope(DesignFeatureKind::Shell);
    let group = group(DesignOperandRole::ROLE_0X10);
    let FeatureOperation::Shell {
        bodies,
        removed_faces,
        ..
    } = projected_operation(super::super::project_shell(
        &scope,
        &[],
        std::slice::from_ref(&group),
    ))
    else {
        panic!("expected a shell");
    };
    assert_eq!(removed_faces, FaceSelection::Native(group.id.clone()));
    assert_eq!(bodies, None);
}

#[test]
fn shell_projects_native_body_group() {
    let scope = scope(DesignFeatureKind::Shell);
    let group = group(DesignOperandRole::BODIES_A);
    let FeatureOperation::Shell {
        bodies,
        removed_faces,
        ..
    } = projected_operation(super::super::project_shell(
        &scope,
        &[],
        std::slice::from_ref(&group),
    ))
    else {
        panic!("expected a shell");
    };
    assert_eq!(bodies, Some(BodySelection::Native(group.id.clone())));
    assert_eq!(removed_faces, FaceSelection::Faces(Vec::new()));
}

fn wall_slot_scope(
    inside: f64,
    outside: Option<f64>,
    rounded_corners: bool,
) -> DesignParameterScope {
    use crate::records::feature::direct_face::{DesignShellOutsideWall, DesignShellWallSlots};
    let mut scope = scope(DesignFeatureKind::Shell);
    if let DesignScopePayloadMut::Shell(Some(operation)) = scope.payload_mut() {
        operation.thickness = cadmpeg_ir::scalar::PositiveReal::new(inside).unwrap();
        operation.outward = false;
        operation.wall_slots = Some(DesignShellWallSlots {
            tangent_chain: false,
            rounded_corners,
            outside: outside.map(|thickness| DesignShellOutsideWall {
                thickness: cadmpeg_ir::scalar::PositiveReal::new(thickness).unwrap(),
                thickness_record_index: 301,
                thickness_offset: 0,
            }),
        });
    }
    scope
}

#[test]
fn wall_slots_project_thickness_direction_mode_and_corners() {
    use cadmpeg_ir::features::{ShellJoin, ShellMode};
    let group = group(DesignOperandRole::ROLE_0X10);
    let project = |scope: &DesignParameterScope| {
        let FeatureOperation::Shell {
            thickness,
            outward,
            mode,
            join,
            ..
        } = projected_operation(super::super::project_shell(
            scope,
            &[],
            std::slice::from_ref(&group),
        ))
        else {
            panic!("expected a shell");
        };
        (thickness.unwrap().get(), outward, mode, join)
    };
    assert_eq!(
        project(&wall_slot_scope(0.2, None, false)),
        (2.0, Some(false), None, Some(ShellJoin::Intersection))
    );
    assert_eq!(
        project(&wall_slot_scope(0.2, None, true)),
        (2.0, Some(false), None, Some(ShellJoin::Arc))
    );
    assert_eq!(
        project(&wall_slot_scope(0.1, Some(0.1), false)),
        (
            2.0,
            None,
            Some(ShellMode::BothSides),
            Some(ShellJoin::Intersection)
        )
    );
}

#[test]
fn walls_of_different_thickness_have_no_neutral_shell() {
    let group = group(DesignOperandRole::ROLE_0X10);
    let scope = wall_slot_scope(0.3, Some(0.1), false);
    assert_eq!(
        super::super::project_shell(&scope, &[], std::slice::from_ref(&group)).unwrap(),
        None
    );
}

#[test]
fn a_shell_without_wall_slots_states_no_corners() {
    let group = group(DesignOperandRole::ROLE_0X10);
    let scope = scope(DesignFeatureKind::Shell);
    let FeatureOperation::Shell { outward, join, .. } = projected_operation(
        super::super::project_shell(&scope, &[], std::slice::from_ref(&group)),
    ) else {
        panic!("expected a shell");
    };
    assert_eq!(outward, Some(true));
    assert_eq!(join, None);
}

fn move_face_scope(transform: [[f64; 4]; 4]) -> DesignParameterScope {
    use crate::records::feature::direct_face::{DesignMoveForm, DesignMoveOperation};
    let mut scope =
        DesignParameterScope::empty("f3d:test:scope#12", DesignFeatureKind::MoveFace, 12);
    let DesignScopePayloadMut::MoveFace(slot) = scope.payload_mut() else {
        panic!("MoveFace payload");
    };
    *slot = Some(DesignMoveOperation {
        transform: transform.try_into().unwrap(),
        transform_offset: 448,
        transform_record_index: 300,
        form: DesignMoveForm::Form5,
        form_offset: 443,
        transform_reference_offset: None,
    });
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![100, 200, 300]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

#[test]
fn move_face_projects_a_translation_of_its_group() {
    use cadmpeg_ir::features::FaceMotion;
    let scope = move_face_scope([
        [1., 0., 0., 0.],
        [0., 1., 0., -0.25],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ]);
    let group = group(DesignOperandRole::ROLE_0X10);
    let FeatureOperation::MoveFace {
        faces,
        motion: FaceMotion::Translate {
            direction,
            distance,
        },
    } = projected_operation(super::super::project_move_face(
        &scope,
        std::slice::from_ref(&group),
    ))
    else {
        panic!("expected a face translation");
    };
    assert_eq!(faces, FaceSelection::Native(group.id.clone()));
    assert_eq!(direction.y, -1.);
    assert_eq!(distance.get(), 2.5);
}

#[test]
fn move_face_projects_a_transform_stored_as_the_first_reference() {
    let mut scope = move_face_scope([
        [1., 0., 0., 0.],
        [0., 1., 0., -0.25],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ]);
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![300, 100, 200]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut faces = group(DesignOperandRole::ROLE_0X10);
    assert!(
        super::super::project_move_face(&scope, std::slice::from_ref(&faces))
            .unwrap()
            .is_none(),
        "the group is reference one, so ordinal zero is not its place"
    );
    faces.scope_reference_ordinal = 1;
    assert!(matches!(
        projected_operation(super::super::project_move_face(
            &scope,
            std::slice::from_ref(&faces),
        )),
        FeatureOperation::MoveFace { .. }
    ));
}

#[test]
fn move_face_stays_native_without_a_translation_or_its_exact_group() {
    let translation = [
        [1., 0., 0., 0.25],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ];
    let faces = group(DesignOperandRole::ROLE_0X10);
    let refused = |transform, groups: &[DesignConstructionOperandGroup]| {
        super::super::project_move_face(&move_face_scope(transform), groups)
            .unwrap()
            .is_none()
    };
    assert!(!refused(translation, std::slice::from_ref(&faces)));
    assert!(refused(translation, &[]));
    assert!(refused(translation, &[group(DesignOperandRole::BODIES_A)]));
    assert!(refused(translation, &[faces.clone(), faces.clone()]));
    let mut other_member = faces.clone();
    other_member.record_index = 101;
    assert!(refused(translation, &[other_member]));
    let identity = [
        [1., 0., 0., 0.],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ];
    assert!(refused(identity, std::slice::from_ref(&faces)));
}

#[test]
fn move_face_projects_a_rotation_about_the_axis_point_nearest_the_origin() {
    use cadmpeg_ir::features::FaceMotion;
    const EPS_ROTATION_FIELD: f64 = 1.0e-9;
    let (sine, cosine) = 5f64.to_radians().sin_cos();
    // A rotation about +X through (0, 1.5, 2) cm stores t = p - R p.
    let (y, z) = (1.5, 2.0);
    let five_degrees = [
        [1., 0., 0., 0.],
        [0., cosine, -sine, y - (cosine * y - sine * z)],
        [0., sine, cosine, z - (sine * y + cosine * z)],
        [0., 0., 0., 1.],
    ];
    let quarter_turn = [
        [0., 1., 0., 0.25],
        [-1., 0., 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ];
    let faces = group(DesignOperandRole::ROLE_0X10);
    for (transform, origin, axis, degrees) in [
        (five_degrees, [0., 15., 20.], [1., 0., 0.], 5.),
        (quarter_turn, [1.25, -1.25, 0.], [0., 0., -1.], 90.),
    ] {
        let FeatureOperation::MoveFace {
            faces: selected,
            motion:
                FaceMotion::Rotate {
                    axis_origin,
                    axis_dir,
                    angle,
                },
        } = projected_operation(super::super::project_move_face(
            &move_face_scope(transform),
            std::slice::from_ref(&faces),
        ))
        else {
            panic!("expected a face rotation");
        };
        assert_eq!(selected, FaceSelection::Native(faces.id.clone()));
        let point = axis_origin.get();
        let direction = axis_dir.get();
        for (actual, expected) in [point.x, point.y, point.z]
            .into_iter()
            .zip(origin)
            .chain(
                [direction.x, direction.y, direction.z]
                    .into_iter()
                    .zip(axis),
            )
            .chain([(angle.get(), f64::to_radians(degrees))])
        {
            assert!(
                (actual - expected).abs() <= EPS_ROTATION_FIELD,
                "{actual} {expected}"
            );
        }
    }
}

#[test]
fn move_face_stays_native_for_a_screw_motion() {
    let screw = [
        [0., 1., 0., 0.25],
        [-1., 0., 0., 0.],
        [0., 0., 1., 0.25],
        [0., 0., 0., 1.],
    ];
    assert!(super::super::project_move_face(
        &move_face_scope(screw),
        std::slice::from_ref(&group(DesignOperandRole::ROLE_0X10)),
    )
    .unwrap()
    .is_none());
}

#[test]
fn move_face_stays_native_for_a_reflection() {
    let faces = group(DesignOperandRole::ROLE_0X10);
    for transform in [
        [
            [-1., 0., 0., 0.25],
            [0., 1., 0., 0.],
            [0., 0., 1., 0.],
            [0., 0., 0., 1.],
        ],
        [
            [1., 0., 0., 0.],
            [0., 1., 0., 0.25],
            [0., 0., -1., 0.],
            [0., 0., 0., 1.],
        ],
    ] {
        assert!(super::super::project_move_face(
            &move_face_scope(transform),
            std::slice::from_ref(&faces),
        )
        .unwrap()
        .is_none());
    }
}

#[test]
fn move_transform_projects_signed_rotation_translation_and_unique_body_group() {
    use crate::records::feature::direct_face::{DesignMoveForm, DesignMoveOperation};
    let mut scope = DesignParameterScope::empty("f3d:test:scope#12", DesignFeatureKind::Move, 12);
    let DesignScopePayloadMut::Move(slot) = scope.payload_mut() else {
        panic!("Move payload");
    };
    *slot = Some(DesignMoveOperation {
        transform: [
            [0., 1., 0., 7.],
            [-1., 0., 0., 11.],
            [0., 0., 1., 13.],
            [0., 0., 0., 1.],
        ]
        .try_into()
        .unwrap(),
        transform_offset: 448,
        transform_record_index: 71,
        form: DesignMoveForm::Form1,
        form_offset: 443,
        transform_reference_offset: None,
    });
    let group = group(DesignOperandRole::BODIES_A);
    let Some(FeatureDefinition::Operation(FeatureOperation::MoveBody {
        bodies,
        translation,
        rotation: Some(rotation),
        copies,
    })) = super::super::project_move(&scope, std::slice::from_ref(&group))
    else {
        panic!("MoveBody");
    };
    assert_eq!(bodies, BodySelection::Native(group.id.clone()));
    assert_eq!(
        translation.get(),
        cadmpeg_ir::math::Vector3::new(70., 110., 130.)
    );
    assert_eq!(
        rotation.origin.get(),
        cadmpeg_ir::math::Point3::new(0., 0., 0.)
    );
    assert_eq!(rotation.direction.z, -1.);
    assert!((rotation.angle.get() - std::f64::consts::FRAC_PI_2).abs() <= f64::EPSILON);
    assert_eq!(copies, 0);
    assert!(super::super::project_move(&scope, &[]).is_none());
    assert!(super::super::project_move(&scope, &[group.clone(), group.clone()]).is_none());
    let DesignScopePayloadMut::Move(Some(operation)) = scope.payload_mut() else {
        panic!("Move payload");
    };
    operation.transform = [
        [-1., 0., 0., 0.],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ]
    .try_into()
    .unwrap();
    assert!(super::super::project_move(&scope, &[group]).is_none());
}

/// A bounded-face member of group 100 whose candidate lanes resolve face 27.
pub(super) fn resolved_member_operand() -> crate::records::topology::face::DesignFaceOperand {
    crate::records::topology::face::DesignFaceOperand::try_new(
        crate::records::topology::face::DesignFaceOperandDraft {
            id: "f3d:test:face-operand#200".into(),
            scope_record_index: 12,
            scope_reference_ordinal: 0,
            group: Some(crate::records::topology::body_recipe::DesignOperandGroup {
                group_record_index: 100,
                group_member_ordinal: 0,
            }),
            record_index: 200,
            byte_offset: 1200,
            class_tag: "297".to_owned().try_into().unwrap(),
            paired_byte_offset: 1250,
            paired_class_tag: "259".to_owned().try_into().unwrap(),
            recipe_record_index: 203,
            recipe_record_byte_offset: 1300,
            recipe_id: "f3d:test:recipe#203".into(),
            recipe_prefix_offset: 1311,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: Vec::new(),
            recipe_kind: crate::records::recipes::ConstructionRecipeKind::BoundedFace,
            recipe_program_offset: 1350,
            recipe_program: vec![0, -1],
            recipe_nodes: Vec::new(),
            candidate_faces: Vec::new(),
            unreferenced_candidate_faces: Vec::new(),
            alternate_selector_candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            changed_candidate_faces: Vec::new(),
            historical_support_contexts: Vec::new(),
            resolved_face_slots: vec![27],
            resolved_active_face: None,
            next_record_index: 220,
            next_byte_offset: 1411,
        },
    )
    .unwrap()
}

#[test]
fn offset_faces_keeps_its_group_when_a_member_resolves_a_face() {
    let scope = scope(DesignFeatureKind::OffsetFaces);
    let group = group(DesignOperandRole::ROLE_0X10);
    let operand = resolved_member_operand();
    let FeatureOperation::MoveFace { faces, .. } =
        projected_operation(super::super::project_offset_faces(
            &scope,
            &[],
            std::slice::from_ref(&operand),
            std::slice::from_ref(&group),
        ))
    else {
        panic!("expected a face move");
    };
    assert_eq!(faces, FaceSelection::Native(group.id.clone()));
}

#[test]
fn shell_keeps_its_group_when_a_member_resolves_a_face() {
    let scope = scope(DesignFeatureKind::Shell);
    let group = group(DesignOperandRole::ROLE_0X10);
    let FeatureOperation::Shell { removed_faces, .. } =
        projected_operation(super::super::project_shell(
            &scope,
            &[resolved_member_operand()],
            std::slice::from_ref(&group),
        ))
    else {
        panic!("expected a shell");
    };
    assert_eq!(removed_faces, FaceSelection::Native(group.id.clone()));
}
