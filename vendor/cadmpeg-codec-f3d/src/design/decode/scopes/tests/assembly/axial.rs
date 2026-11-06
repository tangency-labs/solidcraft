// SPDX-License-Identifier: Apache-2.0

use super::{
    append_axial_test_component_operand, axial_test_alignment, axial_test_component_scope,
};
use crate::design::decode::scopes::axial_assembly::bind_axial_assembly_operand_targets;
use crate::records::feature::assembly::DesignAssemblyAxialOperandTarget;
use crate::records::feature::scope::DesignParameterScope;

#[test]
fn axial_assembly_selectors_bind_component_insert_occurrences_exactly() {
    let first_transform = crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows();
    let mut second_transform =
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows();
    second_transform[2][3] = 4.25;
    let first_role = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    let second_role = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
    let mut bytes = vec![0; 772];
    let first_members = append_axial_test_component_operand(
        &mut bytes,
        70,
        [10, 30],
        first_transform,
        7_001,
        first_role,
        false,
    );
    let second_members = append_axial_test_component_operand(
        &mut bytes,
        80,
        [100, 120],
        second_transform,
        8_001,
        second_role,
        true,
    );
    let mut assembly = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:assembly#500",
        crate::records::feature::scope::DesignFeatureKind::Assemble,
        500,
    );
    assembly
        .try_edit(|draft| {
            draft.frame_length = 772;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(
                first_members
                    .into_iter()
                    .chain(second_members)
                    .chain([90, 91])
                    .collect(),
            );
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::Assemble(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::AsBuilt(slot) =
        assembly.payload_mut()
    {
        *slot = Some(axial_test_alignment([first_transform, second_transform]));
    }
    let mut scopes = vec![
        assembly,
        axial_test_component_scope(200, first_role),
        axial_test_component_scope(300, second_role),
    ];
    let unresolved_scopes = scopes.clone();

    bind_axial_assembly_operand_targets(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &mut scopes,
    )
    .unwrap();
    let targets = scopes[0]
        .assembly_alignment()
        .and_then(|alignment| {
            let crate::records::feature::assembly::DesignAssemblyAlignmentForm::Qualified(operands) =
                alignment.form.as_ref()?
            else {
                return None;
            };
            let [first, second] = operands.each_ref().map(|operand| match &operand.qualifier {
                crate::records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { target } => {
                    Some(target.clone())
                }
                _ => None,
            });
            Some([first?, second?])
        })
        .expect("two exact pathless assembly targets");
    let DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
        component_insert_scope_record_index,
        construction_byte_offset,
        construction_transform_offset,
        axis_record_index_offsets,
        construction_paired_byte_offset,
        selectors,
        ..
    } = targets[0].clone()
    else {
        panic!("first operand must select a component insertion");
    };
    assert_eq!(component_insert_scope_record_index, 200);
    assert_eq!(construction_transform_offset, construction_byte_offset + 48);
    assert_eq!(axis_record_index_offsets[0], construction_byte_offset + 193);
    assert_eq!(axis_record_index_offsets[1], construction_byte_offset + 209);
    assert_eq!(
        construction_paired_byte_offset,
        construction_byte_offset + 380
    );
    assert_eq!(selectors[0].axis_paired_class_tag.as_str(), "261");
    assert_eq!(selectors[0].selector_paired_class_tag.as_str(), "261");
    assert_eq!(selectors[0].occurrence_reference, 10_001);
    assert_eq!(selectors[1].occurrence_reference, 10_002);
    assert_eq!(selectors[0].external_object_reference, 7_001);
    assert!(selectors[0].external_version.is_none());
    let DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
        component_insert_scope_record_index,
        selectors: versioned_selectors,
        ..
    } = targets[1].clone()
    else {
        panic!("second operand must select a component insertion");
    };
    assert_eq!(component_insert_scope_record_index, 300);
    assert!(versioned_selectors[0].external_version.is_some());
    assert_eq!(
        versioned_selectors[0]
            .external_version
            .as_ref()
            .map(|version| version.version_urn.value.as_str()),
        Some("urn:test:version:2")
    );

    let mut mismatched = bytes.clone();
    let mismatch_at =
        usize::try_from(selectors[1].external_object_reference_offset).expect("test offset");
    mismatched[mismatch_at..mismatch_at + 8].copy_from_slice(&7_002_u64.to_le_bytes());
    let mut mismatched_scopes = unresolved_scopes;
    bind_axial_assembly_operand_targets(
        &mismatched,
        &crate::design::test_support::indexed_record_offsets_for_test(&mismatched),
        &mut mismatched_scopes,
    )
    .unwrap();
    assert!(mismatched_scopes[0]
        .assembly_alignment()
        .is_some_and(|alignment| !matches!(
            alignment.form.as_ref(),
            Some(crate::records::feature::assembly::DesignAssemblyAlignmentForm::Qualified([
                crate::records::feature::assembly::DesignQualifiedAssemblyOperand {
                    qualifier: crate::records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { .. },
                    ..
                },
                crate::records::feature::assembly::DesignQualifiedAssemblyOperand {
                    qualifier: crate::records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { .. },
                    ..
                },
            ]))
        )));
}

#[test]
fn axial_assembly_selector_binds_a_document_root_joint_origin() {
    let first_transform = crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows();
    let mut second_transform =
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows();
    second_transform[1][3] = 2.5;
    let role = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    let mut bytes = vec![0; 705];
    let members = append_axial_test_component_operand(
        &mut bytes,
        70,
        [10, 30],
        first_transform,
        7_001,
        role,
        false,
    );
    let mut assembly = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:assembly#500",
        crate::records::feature::scope::DesignFeatureKind::Assemble,
        500,
    );
    assembly
        .try_edit(|draft| {
            draft.frame_length = 705;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(
                members.into_iter().chain([90, 91]).collect(),
            );
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::Assemble(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::AsBuilt(slot) =
        assembly.payload_mut()
    {
        *slot = Some(axial_test_alignment([first_transform, second_transform]));
    }
    let mut origin = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:joint-origin#80",
        crate::records::feature::scope::DesignFeatureKind::JointOrigin,
        80,
    );
    origin.with_joint_origin_transform(second_transform.try_into().unwrap());
    let mut scopes = vec![assembly, axial_test_component_scope(200, role), origin];

    bind_axial_assembly_operand_targets(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &mut scopes,
    )
    .unwrap();
    let targets = scopes[0]
        .assembly_alignment()
        .and_then(|alignment| {
            let crate::records::feature::assembly::DesignAssemblyAlignmentForm::Qualified(operands) =
                alignment.form.as_ref()?
            else {
                return None;
            };
            let [first, second] = operands.each_ref().map(|operand| match &operand.qualifier {
                crate::records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { target } => {
                    Some(target.clone())
                }
                _ => None,
            });
            Some([first?, second?])
        })
        .expect("component and root assembly targets");
    assert!(matches!(
        &targets[0],
        DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
            component_insert_scope_record_index: 200,
            ..
        }
    ));
    assert_eq!(
        targets[1],
        DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin {
            scope_record_index: 80
        }
    );
}
