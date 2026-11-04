// SPDX-License-Identifier: Apache-2.0

use super::extrude_fallbacks::{profile_group, scope_with_extent};
use crate::design::feature_project::{bind_extrude_work_plane_targets, project_extrude};
use crate::records::feature::extrude::DesignExtrudeExtent;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandRole,
};
use crate::records::topology::entity_selection::DesignEntitySelectionOperand;
use crate::records::topology::extrude_selection::{
    DesignExtrudeFaceEncoding, DesignExtrudeFaceRole,
};
use cadmpeg_ir::features::{
    ExtrudeExtent, ExtrudeStart, FaceSelection, FeatureDefinition, FeatureOperation,
    FeatureSupportPlaneFrame, LinearTermination, PlaneTargetSource,
};
use cadmpeg_ir::math::{Point3, Vector3};

/// The class-330/258 to-face frame, which omits its zero termination offset.
fn zero_offset_to_face_scope() -> DesignParameterScope {
    let mut scope = scope_with_extent(DesignExtrudeExtent::OneSidedToFace);
    scope.class_tag = "330".to_owned().try_into().unwrap();
    scope.paired_class_tag = "258".to_owned().try_into().unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 476;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn face_group(record_index: u32, ordinal: u32) -> DesignConstructionOperandGroup {
    let mut group = profile_group(record_index, ordinal);
    group.operand_role = DesignConstructionOperandRole::ExtrudeFaces {
        encoding: DesignExtrudeFaceEncoding::Faces,
        usage: DesignExtrudeFaceRole::Termination,
    };
    group
}

/// A `WorkPlane` with a +z normal through `(1, 2, z_cm)`.
fn work_plane(record_index: u32, z_cm: f64) -> DesignParameterScope {
    let mut plane = DesignParameterScope::empty(
        &format!("f3d:Design/BulkStream.dat:scope#{record_index}"),
        crate::records::feature::scope::DesignFeatureKind::WorkPlane,
        record_index,
    );
    plane.with_work_plane_transform(
        crate::records::sketch_placement::SketchPlacementMatrix::try_from([
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 2.0],
            [0.0, 0.0, 1.0, z_cm],
            [0.0, 0.0, 0.0, 1.0],
        ])
        .unwrap(),
    );
    plane
}

/// A compact entity selection: member `ordinal` of `group`, identity `primary`.
fn selection(
    group: &DesignConstructionOperandGroup,
    ordinal: usize,
    primary_identity: u64,
) -> DesignEntitySelectionOperand {
    let record_index = group.members()[ordinal].value;
    crate::records::topology::entity_selection::DesignEntitySelectionOperand::try_new(
        crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
            id: format!("f3d:Design/BulkStream.dat:entity-selection#{record_index}"),
            scope_record_index: group.scope_record_index,
            group_record_index: group.record_index,
            group_member_ordinal: u32::try_from(ordinal).unwrap(),
            record_index,
            byte_offset: 0,
            class_tag: "372".to_owned().try_into().unwrap(),
            asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
                .to_owned()
                .try_into()
                .unwrap(),
            asset_id_offset: 0,
            context_id: "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e"
                .to_owned()
                .try_into()
                .unwrap(),
            context_id_offset: 0,
            identity_record_index: record_index + 3,
            identity_record_offset: 0,
            primary_identity,
            primary_identity_offset: 21,
            secondary: None,
            historical_edge_candidates: Vec::new(),
            historical_face_candidates: Vec::new(),
            resolved_edge_slot: None,
            next_record_index: record_index + 4,
            next_byte_offset: 29,
        },
    )
    .unwrap()
}

fn plane_frame(z_mm: f64) -> FeatureSupportPlaneFrame {
    FeatureSupportPlaneFrame::new(
        Point3::new(10.0, 20.0, z_mm),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .unwrap()
}

fn native(group: &DesignConstructionOperandGroup) -> cadmpeg_core::text::NonBlankString {
    cadmpeg_core::text::NonBlankString::new(group.id.clone()).unwrap()
}

fn to_plane(group: &DesignConstructionOperandGroup, z_mm: f64) -> LinearTermination {
    LinearTermination::ToPlane {
        plane: plane_frame(z_mm),
        native: native(group),
        source: Some(PlaneTargetSource::WorkPlane),
    }
}

/// The projected Extrude of the zero-offset to-face fixture, ending at `target`.
fn projected_to_face(target: &DesignConstructionOperandGroup) -> FeatureDefinition {
    let scope = zero_offset_to_face_scope();
    let definition = crate::test_support::with_decode_context(|decode_ctx| {
        project_extrude(
            decode_ctx,
            &scope,
            &[],
            &[profile_group(100, 0), target.clone()],
            &[],
            &[],
            &[],
        )
    })
    .expect("project the extrude feature")
    .expect("zero-offset to-face Extrude");
    assert_eq!(
        termination(&definition, 0),
        LinearTermination::ToFace {
            face: FaceSelection::Native(target.id.clone()),
            offset: None,
        }
    );
    definition
}

fn termination(definition: &FeatureDefinition, side: usize) -> LinearTermination {
    let FeatureDefinition::Operation(FeatureOperation::Extrude { extent, .. }) = definition else {
        panic!("expected an Extrude");
    };
    match (extent, side) {
        (ExtrudeExtent::OneSided { side } | ExtrudeExtent::TwoSided { first: side, .. }, 0)
        | (ExtrudeExtent::TwoSided { second: side, .. }, 1) => side.termination.clone(),
        _ => panic!("expected an Extrude side"),
    }
}

fn bound(
    definition: &FeatureDefinition,
    planes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    selections: &[DesignEntitySelectionOperand],
) -> FeatureDefinition {
    let mut bound = definition.clone();
    bind_extrude_work_plane_targets(
        &mut bound,
        &zero_offset_to_face_scope(),
        planes,
        groups,
        selections,
    );
    bound
}

#[test]
fn a_zero_offset_target_naming_a_work_plane_ends_at_the_plane() {
    let target = face_group(101, 1);
    let definition = projected_to_face(&target);
    let bound = bound(
        &definition,
        &[work_plane(601, 3.0)],
        std::slice::from_ref(&target),
        &[selection(&target, 0, 600)],
    );
    assert_eq!(termination(&bound, 0), to_plane(&target, 30.0));
}

#[test]
fn a_target_without_exactly_one_work_plane_of_its_scope_keeps_the_face_group() {
    let target = face_group(101, 1);
    let definition = projected_to_face(&target);
    let planes = [work_plane(601, 3.0), work_plane(701, 5.0)];

    // The identity names the record before a scope that is not a WorkPlane.
    let not_a_plane = [selection(&target, 0, 601)];
    assert_eq!(
        bound(
            &definition,
            &planes,
            std::slice::from_ref(&target),
            &not_a_plane
        ),
        definition
    );

    // The group belongs to another scope.
    let mut foreign = target.clone();
    foreign.scope_record_index = 13;
    assert_eq!(
        bound(
            &definition,
            &planes,
            std::slice::from_ref(&foreign),
            &[selection(&target, 0, 600)],
        ),
        definition
    );

    // Two WorkPlane members have no single target plane.
    let mut two_members = target.clone();
    two_members
        .try_set_members(vec![
            crate::records::identity::Located {
                value: 201,
                offset: 1026,
            },
            crate::records::identity::Located {
                value: 202,
                offset: 1037,
            },
        ])
        .unwrap();
    assert_eq!(
        bound(
            &definition,
            &planes,
            std::slice::from_ref(&two_members),
            &[
                selection(&two_members, 0, 600),
                selection(&two_members, 1, 700)
            ],
        ),
        definition
    );
}

#[test]
fn a_target_with_an_offset_keeps_the_face_group() {
    let target = face_group(101, 1);
    let mut definition = projected_to_face(&target);
    if let FeatureDefinition::Operation(FeatureOperation::Extrude {
        extent: ExtrudeExtent::OneSided { side },
        ..
    }) = &mut definition
    {
        side.termination = LinearTermination::ToFace {
            face: FaceSelection::Native(target.id.clone()),
            offset: Some(cadmpeg_ir::scalar::Length::new(2.5).unwrap()),
        };
    }
    assert_eq!(
        bound(
            &definition,
            &[work_plane(601, 3.0)],
            std::slice::from_ref(&target),
            &[selection(&target, 0, 600)],
        ),
        definition
    );
}

#[test]
fn each_side_of_a_two_sided_extent_ends_at_its_own_plane() {
    let first = face_group(101, 1);
    let second = face_group(103, 3);
    let mut definition = projected_to_face(&first);
    if let FeatureDefinition::Operation(FeatureOperation::Extrude { extent, .. }) = &mut definition
    {
        let ExtrudeExtent::OneSided { side } = extent.clone() else {
            panic!("expected a one-sided Extrude");
        };
        let mut second_side = side.clone();
        second_side.termination = LinearTermination::ToFace {
            face: FaceSelection::Native(second.id.clone()),
            offset: None,
        };
        *extent = ExtrudeExtent::TwoSided {
            first: side,
            second: second_side,
        };
    }
    let bound = bound(
        &definition,
        &[work_plane(601, 3.0), work_plane(701, -2.0)],
        &[first.clone(), second.clone()],
        &[selection(&first, 0, 600), selection(&second, 0, 700)],
    );
    assert_eq!(termination(&bound, 0), to_plane(&first, 30.0));
    assert_eq!(termination(&bound, 1), to_plane(&second, -20.0));
}

#[test]
fn a_zero_offset_start_naming_a_work_plane_starts_on_the_plane() {
    let target = face_group(101, 1);
    let start = face_group(103, 3);
    let with_start = |offset| {
        let mut definition = projected_to_face(&target);
        if let FeatureDefinition::Operation(FeatureOperation::Extrude { start: slot, .. }) =
            &mut definition
        {
            *slot = ExtrudeStart::FromFace {
                face: FaceSelection::Native(start.id.clone()),
                offset,
            };
        }
        let bound = bound(
            &definition,
            &[work_plane(701, 1.0)],
            std::slice::from_ref(&start),
            &[selection(&start, 0, 700)],
        );
        let FeatureDefinition::Operation(FeatureOperation::Extrude { start, .. }) = bound else {
            panic!("expected an Extrude");
        };
        start
    };
    assert_eq!(
        with_start(None),
        ExtrudeStart::FromPlane {
            plane: plane_frame(10.0),
            native: native(&start),
            source: Some(PlaneTargetSource::WorkPlane),
        }
    );
    let offset = Some(cadmpeg_ir::scalar::Length::new(0.5).unwrap());
    assert_eq!(
        with_start(offset),
        ExtrudeStart::FromFace {
            face: FaceSelection::Native(start.id.clone()),
            offset,
        }
    );
}

#[test]
fn the_dispatcher_binds_the_plane_of_a_projected_extrude() {
    let target = face_group(101, 1);
    let scopes = [zero_offset_to_face_scope(), work_plane(601, 3.0)];
    let groups = [profile_group(100, 0), target.clone()];
    let selections = [selection(&target, 0, 600)];
    let timelines = crate::design::test_support::synthetic_feature_timelines(&scopes);
    let projected = crate::test_support::with_decode_context(|ctx| {
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                scopes: &scopes,
                timelines: &timelines,
                construction_groups: &groups,
                entity_selection_operands: &selections,
                ..Default::default()
            },
        )
    })
    .expect("test projection has a synthetic exact timeline");
    let extrude = projected
        .features
        .iter()
        .find(|feature| feature.id == crate::ids::neutral_feature_id(&scopes[0]))
        .expect("projected Extrude");
    assert_eq!(
        termination(extrude.evaluation.definition(), 0),
        to_plane(&target, 30.0)
    );
}
