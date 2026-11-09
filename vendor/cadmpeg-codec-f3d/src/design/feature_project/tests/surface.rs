// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::surface_ops::DesignSurfaceExtendMethod;
use crate::records::feature::surface_ops::DesignSurfaceExtendOperation;
use crate::records::feature::surface_ops::DesignSurfaceOffsetOperation;
use crate::records::feature::surface_ops::DesignSurfaceOffsetSupport;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::features::FaceSelection;
use cadmpeg_ir::features::Feature;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureOperation;

const EPS_SURFACE_DISTANCE_MM: f64 = 1.0e-12;

#[test]
fn dispatcher_projects_perpendicular_surface_extend() {
    let mut scope = DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:parameter-scope#surface-extend",
        crate::records::feature::scope::DesignFeatureKind::SurfaceExtend,
        12,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::SurfaceExtend(slot) =
        scope.payload_mut()
    {
        *slot = Some(DesignSurfaceExtendOperation {
            distance: crate::test_support::real(0.04),
            distance_offset: 40,
            distance_record_index: 400,
            method: DesignSurfaceExtendMethod::Perpendicular,
            method_offset: 102,
            boundary_record_index: 500,
            boundary_reference_record_index: 900,
            boundary_reference_offset: 106,
            edge_record_indices: vec![503, 507],
            tolerance: cadmpeg_ir::scalar::PositiveReal::new(f64::EPSILON)
                .expect("checked fixture value"),
            tolerance_offset: 139,
        });
    }
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = &[scope];
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    scopes,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });

    let [Feature { evaluation, .. }] = features.as_slice() else {
        panic!("perpendicular SurfaceExtend did not project as a typed feature");
    };
    let FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
        faces: FaceSelection::Native(native),
        distance: Some(distance),
        method: cadmpeg_ir::features::SurfaceExtension::Perpendicular,
    }) = evaluation.definition()
    else {
        panic!("perpendicular SurfaceExtend did not project as a typed feature");
    };
    assert!(native.ends_with(":design-record#500"));
    assert!((distance.get() - 0.4).abs() < EPS_SURFACE_DISTANCE_MM);
}

fn grouped_surface_offset_scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:test:scope#12",
        crate::records::feature::scope::DesignFeatureKind::SurfaceOffset,
        12,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::SurfaceOffset(slot) =
        scope.payload_mut()
    {
        *slot = Some(DesignSurfaceOffsetOperation {
            distance: crate::test_support::real(-0.25),
            distance_offset: 40,
            distance_record_index: 300,
            support: DesignSurfaceOffsetSupport::FaceGroups {
                group_record_indices: vec![100],
            },
        });
    }
    scope
}

fn projected_surface_offset(
    operands: &[crate::records::topology::face::DesignFaceOperand],
) -> Option<(FaceSelection, f64)> {
    let scope = grouped_surface_offset_scope();
    let group = super::direct_face::group(DesignOperandRole::PROFILE);
    let operation = scope.surface_offset_operation()?.clone();
    let Some(FeatureDefinition::Operation(FeatureOperation::OffsetSurface {
        faces,
        distance: Some(distance),
    })) = super::super::surface_offset::project_surface_offset(
        &scope,
        &operation,
        std::slice::from_ref(&group),
        operands,
        &[],
    )
    .unwrap()
    else {
        return None;
    };
    Some((faces, distance.get()))
}

#[test]
fn grouped_surface_offset_projects_resolved_member_faces() {
    let (faces, distance) =
        projected_surface_offset(&[super::direct_face::resolved_member_operand()])
            .expect("grouped SurfaceOffset projects");
    assert_eq!(
        faces,
        FaceSelection::Resolved {
            faces: vec![
                cadmpeg_ir::ids::FaceId::try_from("f3d:brep:entity#27".to_owned()).unwrap()
            ],
            native: "f3d:test:scope#12".into(),
        }
    );
    assert!((distance + 2.5).abs() < EPS_SURFACE_DISTANCE_MM);
}

#[test]
fn grouped_surface_offset_keeps_its_native_selection_without_member_faces() {
    let (faces, distance) = projected_surface_offset(&[]).expect("grouped SurfaceOffset projects");
    assert_eq!(faces, FaceSelection::Native("f3d:test:scope#12".into()));
    assert!((distance + 2.5).abs() < EPS_SURFACE_DISTANCE_MM);
}
