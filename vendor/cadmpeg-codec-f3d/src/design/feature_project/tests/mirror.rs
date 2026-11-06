// SPDX-License-Identifier: Apache-2.0
use crate::records::topology::extrude_selection::DesignOperandRole;

use crate::design::feature_project::project_mirror;
use crate::records::{
    feature::{mirror::DesignMirrorConstruction, scope::DesignParameterScope},
    topology::{
        construction::DesignConstructionOperandGroup,
        construction::DesignConstructionOperandGroupFrame,
    },
};
use cadmpeg_ir::features::{
    patterns::{PatternSeed, PatternTransform},
    BodySelection, FaceSelection, FeatureDefinition, FeatureOperation,
};
use cadmpeg_ir::math::{Point3, Vector3};

fn group(
    scope_record_index: u32,
    record_index: u32,
    role: DesignOperandRole,
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: format!("f3d:Design/BulkStream.dat:group#{record_index}"),
            scope_record_index,
            scope_reference_ordinal: 0,
            record_index,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("282".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: record_index + 1,
                offset: 0,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(role),
            role_offset: 0,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap()
}

fn mirror_scope(seed_group_record_index: u32) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#10",
        crate::records::feature::scope::DesignFeatureKind::Mirror,
        10,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::Mirror(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::SymetrieMiroir(slot) =
        scope.payload_mut()
    {
        *slot = Some(DesignMirrorConstruction {
            count_record_index: 11,
            count_offset: 0,
            stitch_tolerance: cadmpeg_ir::scalar::PositiveReal::new(0.001)
                .expect("checked fixture value"),
            stitch_tolerance_offset: 0,
            tolerance_source: crate::records::feature::mirror::DesignMirrorToleranceSource::Owner {
                record_index: 12,
            },
            seed_group_record_index,
            plane_group_record_index: 30,
            seed_feature_scope_record_index: None,
            plane_scope_record_index: None,
            plane_selection_record_index: None,
            plane: crate::records::feature::patterns::DesignPlane::from_parts(
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                    .expect("finite plane origin"),
                cadmpeg_ir::features::FiniteVector3::new(Vector3::new(0.0, 0.0, 1.0))
                    .expect("finite plane normal"),
            ),
        });
    }
    scope
}

#[test]
fn mirror_seed_role_selects_body_or_face_semantics() {
    let body_scope = mirror_scope(20);
    let body_groups = [
        group(10, 20, DesignOperandRole::BODIES_B),
        group(10, 30, DesignOperandRole::ROLE_0X5),
    ];
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) =
        project_mirror(&body_scope, &body_groups, &[], &[]).expect("body mirror")
    else {
        panic!("mirror projects a pattern");
    };
    assert!(matches!(
        (pattern).definition(),
        PatternTransform::Mirror { .. }
    ));
    assert!(matches!(
        seeds.as_slice(),
        [PatternSeed::Bodies(BodySelection::Native(native))]
            if native == "f3d:Design/BulkStream.dat:group#20"
    ));

    let face_scope = mirror_scope(40);
    let face_groups = [
        group(10, 40, DesignOperandRole::BODIES_A),
        group(10, 30, DesignOperandRole::ROLE_0X5),
    ];
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) =
        project_mirror(&face_scope, &face_groups, &[], &[]).expect("face mirror")
    else {
        panic!("mirror projects a pattern");
    };
    assert!(matches!(
        seeds.as_slice(),
        [PatternSeed::Faces(FaceSelection::Native(native))]
            if native == "f3d:Design/BulkStream.dat:group#40"
    ));
}
