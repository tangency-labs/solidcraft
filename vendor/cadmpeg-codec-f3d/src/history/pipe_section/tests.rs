// SPDX-License-Identifier: Apache-2.0

use super::{bind_circular_pipe_sections, PipeSectionInputs};
use crate::loss::F3dLossCode;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features::DesignPipeConstruction;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayload};
use crate::records::parameters::{DesignParameterOwner, DesignParameterOwnerWire};
use crate::records::references::DesignClassTag;
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
    DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
    DesignConstructionOperandRole,
};
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    GeneratedSweepSection, PathRef, SweepSection, SweepShape,
};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossKind;

const STREAM: &str = "f3d:Design/BulkStream.dat";
const LINEAR_TOLERANCE: f64 = 1.0e-6;

fn pipe_scope(operation: DesignExtrudeOperation, is_hollow: bool) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#20"),
        DesignFeatureKind::Pipe,
        20,
    );
    scope
        .try_edit(|draft| {
            draft.payload = DesignScopePayload::Pipe(Some(DesignPipeConstruction {
                operation,
                operation_offset: 22,
                is_hollow,
                is_hollow_offset: 21,
                opaque_post_operation: cadmpeg_ir::native::bytes::NativeBytes::new([0, 0]),
                opaque_post_operation_offset: 26,
                record_indexes: [21, 22, 23, 24],
                point_selection: None,
            }));
            draft.reference_count_offset = 40;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.kind_offset + 96;
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

/// The forward and reverse fractions 1, `size` and `thickness`, in
/// centimetres.
fn owners(size: f64, thickness: f64) -> Vec<DesignParameterOwner> {
    owners_with_fractions([1.0, 1.0], size, thickness)
}

fn owners_with_fractions(
    [forward, reverse]: [f64; 2],
    size: f64,
    thickness: f64,
) -> Vec<DesignParameterOwner> {
    [forward, reverse, size, thickness]
        .into_iter()
        .zip(21u32..)
        .map(|(value, record_index)| {
            DesignParameterOwner::try_from(DesignParameterOwnerWire {
                id: format!("{STREAM}:design-parameter-owner#{record_index}"),
                byte_offset: 120,
                frame_length: 104,
                class_tag: DesignClassTag::try_from("292".to_owned()).unwrap(),
                record_index,
                scope_record_index: 20,
                local_ordinal: record_index - 21,
                evaluated_value: value,
                evaluated_value_offset: 160,
                parameter_record_index: record_index - 1,
                owned_ordinal: 0,
                variant: Some(0),
                companion_record_index: record_index + 1,
            })
            .unwrap()
        })
        .collect()
}

fn path_group(record_index: u32, role: DesignOperandRole) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("{STREAM}:design-construction-operand-group#{record_index}"),
        scope_record_index: 20,
        scope_reference_ordinal: 0,
        record_index,
        byte_offset: 900,
        class_tag: DesignClassTag::try_from("277".to_owned()).unwrap(),
        members: vec![crate::records::identity::Located {
            value: record_index + 1,
            offset: 926,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 921,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 1,
                opaque_index_offset: 971,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 975,
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(role),
        role_offset: 953,
        paired_class_tag: DesignClassTag::try_from("265".to_owned()).unwrap(),
        paired_byte_offset: 1024,
    })
    .unwrap()
}

fn cylinder(radius: f64) -> SurfaceGeometry {
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            radius,
        )
        .unwrap(),
    ))
}

fn torus(minor_radius: f64) -> SurfaceGeometry {
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
        cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            20.0,
            minor_radius,
        )
        .unwrap(),
    ))
}

fn cone(radius: f64) -> SurfaceGeometry {
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            radius,
            1.0,
            0.3,
        )
        .unwrap(),
    ))
}

/// The unit cube with its first face carriers replaced by `carriers`.
fn result_model(carriers: &[SurfaceGeometry]) -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    for (surface, geometry) in ir.model.surfaces.iter_mut().zip(carriers) {
        surface.geometry = geometry.clone();
    }
    ir
}

fn pipe_feature(scope: &DesignParameterScope, ir: &cadmpeg_ir::CadIr) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint("test:model:feature#pipe").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Pipe".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "Pipe".into(),
                parameters: Default::default(),
            }),
            cadmpeg_ir::features::DistinctMembers::try_from(vec![ir.model.bodies[0].id.clone()])
                .unwrap(),
        ),
        native_ref: Some(scope.id.clone()),
    }
}

/// The definition the binder gives a Pipe over `scope`, `owners`, `groups`
/// and the cube result whose first carriers are `carriers`.
fn bound(
    scope: &DesignParameterScope,
    owners: &[DesignParameterOwner],
    groups: &[DesignConstructionOperandGroup],
    carriers: &[SurfaceGeometry],
) -> FeatureDefinition {
    bound_with(scope, owners, groups, carriers, |_, _| {})
}

/// `bound`, after `edit` changes the result model and the feature list.
fn bound_with(
    scope: &DesignParameterScope,
    owners: &[DesignParameterOwner],
    groups: &[DesignConstructionOperandGroup],
    carriers: &[SurfaceGeometry],
    edit: impl FnOnce(&mut cadmpeg_ir::CadIr, &mut Vec<Feature>),
) -> FeatureDefinition {
    bound_with_losses(scope, owners, groups, carriers, edit).0
}

/// `bound_with`, and the loss kinds the binder reports.
fn bound_with_losses(
    scope: &DesignParameterScope,
    owners: &[DesignParameterOwner],
    groups: &[DesignConstructionOperandGroup],
    carriers: &[SurfaceGeometry],
    edit: impl FnOnce(&mut cadmpeg_ir::CadIr, &mut Vec<Feature>),
) -> (FeatureDefinition, Vec<LossKind>) {
    let mut ir = result_model(carriers);
    let mut features = vec![pipe_feature(scope, &ir)];
    edit(&mut ir, &mut features);
    let losses = bind_circular_pipe_sections(
        &mut features,
        &PipeSectionInputs {
            scopes: std::slice::from_ref(scope),
            groups,
            owners,
            bodies: &ir.model.bodies,
            regions: &ir.model.regions,
            faces: &ir.model.faces,
            surfaces: &ir.model.surfaces,
            linear_tolerance: LINEAR_TOLERANCE,
        },
    );
    let definition = features[0].evaluation.definition().clone();
    let losses = losses.into_iter().map(|loss| loss.code).collect::<Vec<_>>();
    let projected = matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Sweep { .. })
    );
    let assumptions = [
        F3dLossCode::FeaturePipeCircularSectionInferred.kind(),
        F3dLossCode::FeaturePipeWholePathExtentAssumed.kind(),
    ];
    assert_eq!(
        losses.as_slice(),
        if projected { &assumptions[..] } else { &[] }
    );
    (definition, losses)
}

/// The outer radius and wall thickness of a bound circular section.
fn circular_section(definition: &FeatureDefinition) -> Option<(f64, Option<f64>)> {
    let FeatureDefinition::Operation(FeatureOperation::Sweep {
        shape:
            SweepShape::Solid {
                section: SweepSection::Generated(GeneratedSweepSection::CircularRegion { region }),
                ..
            },
        ..
    }) = definition
    else {
        return None;
    };
    Some((
        region.outer_radius().get(),
        region
            .wall_thickness()
            .map(cadmpeg_ir::scalar::PositiveLength::get),
    ))
}

#[test]
fn a_new_body_pipe_with_circular_result_faces_projects_as_a_circular_sweep() {
    let scope = pipe_scope(DesignExtrudeOperation::NewBody, false);
    let groups = [path_group(30, DesignOperandRole::ROLE_0X5)];
    let (definition, losses) = bound_with_losses(
        &scope,
        &owners(0.5, 0.1),
        &groups,
        &[cylinder(2.5)],
        |_, _| {},
    );
    assert_eq!(circular_section(&definition), Some((2.5, None)));
    assert_eq!(
        losses,
        [
            F3dLossCode::FeaturePipeCircularSectionInferred.kind(),
            F3dLossCode::FeaturePipeWholePathExtentAssumed.kind()
        ]
    );
    let FeatureDefinition::Operation(FeatureOperation::Sweep {
        path: Some(PathRef::Native(path)),
        path_extent: Some(extent),
        ..
    }) = &definition
    else {
        panic!("Sweep along the path group: {definition:?}");
    };
    assert_eq!(path, &groups[0].id);
    assert_eq!(extent.along_fraction.get(), 1.0);
    assert_eq!(extent.against_fraction.get(), 1.0);
    for carrier in [torus(2.5), torus(-2.5)] {
        assert_eq!(
            circular_section(&bound(&scope, &owners(0.5, 0.1), &groups, &[carrier])),
            Some((2.5, None))
        );
    }

    let hollow = pipe_scope(DesignExtrudeOperation::NewBody, true);
    let definition = bound(
        &hollow,
        &owners(0.5, 0.1),
        &groups,
        &[cylinder(2.5), cylinder(1.5)],
    );
    assert_eq!(circular_section(&definition), Some((2.5, Some(1.0))));
}

#[test]
fn a_pipe_needs_curved_result_faces_of_its_section_radii() {
    let scope = pipe_scope(DesignExtrudeOperation::NewBody, false);
    let hollow = pipe_scope(DesignExtrudeOperation::NewBody, true);
    let groups = [path_group(30, DesignOperandRole::ROLE_0X5)];
    for (scope, carriers) in [
        (&scope, Vec::new()),
        (&scope, vec![cylinder(3.0)]),
        (&scope, vec![cylinder(2.5), cylinder(1.5)]),
        (&scope, vec![cylinder(2.5), cone(2.5)]),
        (&hollow, vec![cylinder(2.5)]),
    ] {
        assert!(
            matches!(
                bound(scope, &owners(0.5, 0.1), &groups, &carriers),
                FeatureDefinition::Operation(FeatureOperation::Native { .. })
            ),
            "{carriers:?}"
        );
    }
}

#[test]
fn a_pipe_needs_a_new_body_operation_one_path_group_and_its_scalar_owners() {
    let groups = [path_group(30, DesignOperandRole::ROLE_0X5)];
    let mut missing_owner = owners(0.5, 0.1);
    missing_owner.pop();
    let mut duplicate_owner = owners(0.5, 0.1);
    duplicate_owner.extend(owners(0.5, 0.1).pop());
    let two_paths = [
        path_group(30, DesignOperandRole::ROLE_0X5),
        path_group(40, DesignOperandRole::ROLE_0X5),
    ];
    let no_path = [path_group(30, DesignOperandRole::BODIES_A)];
    let new_body = pipe_scope(DesignExtrudeOperation::NewBody, false);
    let join = pipe_scope(DesignExtrudeOperation::Join, false);
    for (scope, owners, groups) in [
        (&join, owners(0.5, 0.1), &groups[..]),
        (&new_body, missing_owner, &groups[..]),
        (&new_body, duplicate_owner, &groups[..]),
        (&new_body, owners(0.5, 0.1), &two_paths[..]),
        (&new_body, owners(0.5, 0.1), &no_path[..]),
    ] {
        assert!(matches!(
            bound(scope, &owners, groups, &[cylinder(2.5)]),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ));
    }
}

#[test]
fn a_pipe_needs_whole_path_fractions() {
    let scope = pipe_scope(DesignExtrudeOperation::NewBody, false);
    let groups = [path_group(30, DesignOperandRole::ROLE_0X5)];
    for fractions in [[1.0, 0.0], [0.5, 1.0]] {
        let (definition, losses) = bound_with_losses(
            &scope,
            &owners_with_fractions(fractions, 0.5, 0.1),
            &groups,
            &[cylinder(2.5)],
            |_, _| {},
        );
        assert!(matches!(
            definition,
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ));
        assert!(losses.is_empty());
    }
}

#[test]
fn a_pipe_needs_solid_result_bodies_that_no_later_feature_changes() {
    let scope = pipe_scope(DesignExtrudeOperation::NewBody, false);
    let groups = [path_group(30, DesignOperandRole::ROLE_0X5)];
    let later = |features: &mut Vec<Feature>, definition: FeatureDefinition, shares: bool| {
        let mut feature = features[0].clone();
        feature.id = FeatureId::mint("test:model:feature#later").unwrap();
        feature.ordinal = 1;
        feature.native_ref = None;
        let outputs = if shares {
            features[0].evaluation.outputs().clone()
        } else {
            Vec::new()
        };
        let outputs = cadmpeg_ir::features::DistinctMembers::try_from(outputs).unwrap();
        feature.evaluation = FeatureEvaluation::new(definition, outputs);
        features.push(feature);
    };
    let native = || {
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: "Hole".into(),
            parameters: Default::default(),
        })
    };
    for shares in [true, false] {
        assert!(matches!(
            bound_with(
                &scope,
                &owners(0.5, 0.1),
                &groups,
                &[cylinder(2.5)],
                |_, features| later(features, native(), shares)
            ),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ));
    }
    assert!(matches!(
        bound_with(
            &scope,
            &owners(0.5, 0.1),
            &groups,
            &[cylinder(2.5)],
            |ir, _| ir.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Sheet
        ),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    ));
    assert!(matches!(
        bound_with(
            &scope,
            &owners(0.5, 0.1),
            &groups,
            &[cylinder(2.5)],
            |ir, _| ir.model.regions.clear()
        ),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    ));
    assert!(matches!(
        bound_with(
            &scope,
            &owners(0.5, 0.1),
            &groups,
            &[cylinder(2.5)],
            |ir, _| {
                let mut twin = ir.model.surfaces[0].clone();
                twin.geometry = cylinder(4.0);
                ir.model.surfaces.push(twin);
            }
        ),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    ));
}
