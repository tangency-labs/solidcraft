// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::scopes::direct_face::{
    exact_direct_face_operation, exact_scale_operation,
};
use crate::design::decode::scopes::fixed_parameters::exact_fixed_extrude_parameters;
use crate::design::decode::scopes::solid_primitive::exact_solid_primitive;
use crate::design::decode::scopes::surfaces::{
    exact_surface_extend_operation, exact_surface_offset_operation,
};
use crate::records::feature::body_ops::DesignScaleOperation;
use crate::records::feature::direct_face::DesignDirectFaceOperation;
use crate::records::feature::extrude::{
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
};
use crate::records::feature::fixed_parameters::{
    DesignFixedExtrudeDistance, DesignFixedExtrudeParameters, DesignFixedExtrudeScalar,
};
use crate::records::feature::primitives::DesignSolidPrimitive;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::surface_ops::{
    DesignSurfaceExtendMethod, DesignSurfaceExtendOperation, DesignSurfaceOffsetOperation,
    DesignSurfaceOffsetSupport,
};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::{
    construction::DesignConstructionOperandGroupFrame, extrude_selection::DesignOperandRole,
};
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::features::{FaceSelection, Feature, FeatureDefinition};
use std::collections::HashMap;

fn tested_surface_extend_operation(
    bytes: &[u8],
    records: &crate::design::decode::sketch::IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignSurfaceExtendOperation> {
    exact_surface_extend_operation(bytes, records, scope)
}

fn tested_surface_offset_operation(
    bytes: &[u8],
    records: &crate::design::decode::sketch::IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignSurfaceOffsetOperation> {
    exact_surface_offset_operation(bytes, records, scope).unwrap()
}

pub(super) fn fixed_kind_tail_operations(
    mut bytes: Vec<u8>,
    scope: DesignParameterScope,
    transform: [[f64; 4]; 4],
) {
    let move_at = bytes.len();
    let mut move_frame = vec![0; 254];
    move_frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    move_frame[4..7].copy_from_slice(b"368");
    move_frame[7..11].copy_from_slice(&90u32.to_le_bytes());
    move_frame[43..47].copy_from_slice(&5u32.to_le_bytes());
    let mut move_transform =
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows();
    move_transform[1][3] = 15.0;
    for (ordinal, value) in move_transform.into_iter().flatten().enumerate() {
        let at = 48 + ordinal * 8;
        move_frame[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    move_frame.extend_from_slice(&3u32.to_le_bytes());
    move_frame.extend_from_slice(b"265");
    move_frame.extend_from_slice(&90u32.to_le_bytes());
    bytes.extend_from_slice(&move_frame);
    let mut move_scope = scope.clone();
    move_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Move
                .try_into()
                .unwrap();
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![90]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = crate::design::decode::scopes::direct_face::exact_move_operation(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &move_scope,
    )
    .expect("class-368 Move frame");
    assert_eq!(decoded.transform, move_transform.try_into().unwrap());
    assert_eq!(decoded.transform_offset, u64_from_index(move_at + 48));
    assert_eq!(u32::from(decoded.form), 5);

    let compact_move_at = bytes.len();
    let mut compact_move = vec![0; 253];
    compact_move[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_move[4..7].copy_from_slice(b"296");
    compact_move[7..11].copy_from_slice(&91u32.to_le_bytes());
    compact_move[43..47].copy_from_slice(&1u32.to_le_bytes());
    for (ordinal, value) in move_transform.into_iter().flatten().enumerate() {
        let at = 48 + ordinal * 8;
        compact_move[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_move.extend_from_slice(&3u32.to_le_bytes());
    compact_move.extend_from_slice(b"265");
    compact_move.extend_from_slice(&91u32.to_le_bytes());
    bytes.extend_from_slice(&compact_move);
    let mut compact_move_scope = scope.clone();
    compact_move_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Move
                .try_into()
                .unwrap();
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![91]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = crate::design::decode::scopes::direct_face::exact_move_operation(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &compact_move_scope,
    )
    .expect("class-296 Move frame");
    assert_eq!(decoded.transform, move_transform.try_into().unwrap());
    assert_eq!(
        decoded.transform_offset,
        u64_from_index(compact_move_at + 48)
    );
    assert_eq!(decoded.transform_record_index, 91);
    assert_eq!(u32::from(decoded.form), 1);
    assert_eq!(decoded.form_offset, u64_from_index(compact_move_at + 43));
    bytes[compact_move_at + 4..compact_move_at + 7].copy_from_slice(b"362");
    bytes[compact_move_at + 43..compact_move_at + 47].copy_from_slice(&5u32.to_le_bytes());
    let decoded = crate::design::decode::scopes::direct_face::exact_move_operation(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &compact_move_scope,
    )
    .expect("class-362 Move frame");
    assert_eq!(decoded.transform, move_transform.try_into().unwrap());
    assert_eq!(u32::from(decoded.form), 5);

    let class_433_move_at = bytes.len();
    let mut class_433_move = vec![0; 253];
    class_433_move[0..4].copy_from_slice(&3u32.to_le_bytes());
    class_433_move[4..7].copy_from_slice(b"433");
    class_433_move[7..11].copy_from_slice(&92u32.to_le_bytes());
    class_433_move[43..47].copy_from_slice(&5u32.to_le_bytes());
    for (ordinal, value) in move_transform.into_iter().flatten().enumerate() {
        let at = 48 + ordinal * 8;
        class_433_move[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    class_433_move.extend_from_slice(&3u32.to_le_bytes());
    class_433_move.extend_from_slice(b"265");
    class_433_move.extend_from_slice(&92u32.to_le_bytes());
    bytes.extend_from_slice(&class_433_move);
    let mut class_433_move_scope = scope.clone();
    class_433_move_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Move
                .try_into()
                .unwrap();
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![92]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = crate::design::decode::scopes::direct_face::exact_move_operation(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &class_433_move_scope,
    )
    .expect("class-433 Move frame");
    assert_eq!(decoded.transform, move_transform.try_into().unwrap());
    assert_eq!(
        decoded.transform_offset,
        u64_from_index(class_433_move_at + 48)
    );
    assert_eq!(decoded.transform_record_index, 92);
    assert_eq!(u32::from(decoded.form), 5);

    let scale_at = bytes.len();
    let mut scale = vec![0; 317];
    scale[20..24].copy_from_slice(&1u32.to_le_bytes());
    scale[25..33].copy_from_slice(&1.5f64.to_le_bytes());
    for (offset, record_index) in [(33, 105u32), (44, 101), (68, 102)] {
        scale[offset] = 1;
        scale[offset + 1..offset + 5].copy_from_slice(&record_index.to_le_bytes());
    }
    scale[55..59].copy_from_slice(&1u32.to_le_bytes());
    scale[60..64].copy_from_slice(&1u32.to_le_bytes());
    scale[64..68].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&scale);
    let mut scale_scope = scope.clone();
    scale_scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(scale_at);
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Massstab
                .try_into()
                .unwrap();
            draft.frame_length = 317;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![101, 102, 103, 104, 105]);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let scale_records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    assert_eq!(
        exact_scale_operation(&bytes, &scale_records, &scale_scope, &HashMap::new()).unwrap(),
        Some(DesignScaleOperation {
            body_group_record_index: 102,
            center_record_index: 105,
            center_position: None,
            uniform_factor: cadmpeg_ir::scalar::PositiveReal::new(1.5)
                .expect("checked fixture value"),
            uniform_factor_offset: u64_from_index(scale_at + 25),
        })
    );

    let sphere_at = bytes.len();
    let mut sphere = vec![0; 462];
    sphere[0..4].copy_from_slice(&3u32.to_le_bytes());
    sphere[4..7].copy_from_slice(b"302");
    sphere[7..11].copy_from_slice(&80u32.to_le_bytes());
    sphere[25..29].copy_from_slice(&4u32.to_le_bytes());
    sphere[29] = 1;
    sphere[30] = 1;
    sphere[41] = 1;
    sphere[42..46].copy_from_slice(&70u32.to_le_bytes());
    sphere[52] = 1;
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 64 + ordinal * 8;
        sphere[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&sphere);
    let mut diameter = vec![0; 104];
    diameter[0..4].copy_from_slice(&3u32.to_le_bytes());
    diameter[4..7].copy_from_slice(b"277");
    diameter[7..11].copy_from_slice(&70u32.to_le_bytes());
    diameter[40..48].copy_from_slice(&8.0f64.to_le_bytes());
    diameter.extend_from_slice(&3u32.to_le_bytes());
    diameter.extend_from_slice(b"261");
    diameter.extend_from_slice(&70u32.to_le_bytes());
    bytes.extend_from_slice(&diameter);
    let mut sphere_scope = scope.clone();
    sphere_scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(sphere_at);
            draft.payload = crate::records::feature::scope::DesignFeatureKind::SpherePrimitive
                .try_into()
                .unwrap();
            draft.frame_length = 462;
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(matches!(
        exact_solid_primitive(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &sphere_scope,
            &[],
        ),
        Some(DesignSolidPrimitive::Sphere(
            crate::records::feature::primitives::DesignSpherePrimitive {
                diameter,
                diameter_record_index: 70,
                operation: DesignExtrudeOperation::NewBody,
                ..
            }
        )) if diameter.get() == 8.0
    ));

    let torus_at = bytes.len();
    let mut torus = vec![0; 486];
    torus[0..4].copy_from_slice(&3u32.to_le_bytes());
    torus[4..7].copy_from_slice(b"305");
    torus[7..11].copy_from_slice(&81u32.to_le_bytes());
    torus[25..29].copy_from_slice(&4u32.to_le_bytes());
    torus[29] = 1;
    torus[30] = 1;
    torus[31..35].copy_from_slice(&71u32.to_le_bytes());
    torus[41] = 1;
    torus[52] = 1;
    torus[53..57].copy_from_slice(&72u32.to_le_bytes());
    torus[63] = 1;
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 75 + ordinal * 8;
        torus[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&torus);
    for (record_index, value) in [(71u32, 15.0f64), (72, 4.0)] {
        let mut diameter = vec![0; 104];
        diameter[0..4].copy_from_slice(&3u32.to_le_bytes());
        diameter[4..7].copy_from_slice(b"277");
        diameter[7..11].copy_from_slice(&record_index.to_le_bytes());
        diameter[40..48].copy_from_slice(&value.to_le_bytes());
        diameter.extend_from_slice(&3u32.to_le_bytes());
        diameter.extend_from_slice(b"261");
        diameter.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&diameter);
    }
    let mut torus_scope = scope.clone();
    torus_scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(torus_at);
            draft.payload = crate::records::feature::scope::DesignFeatureKind::TorusPrimitive
                .try_into()
                .unwrap();
            draft.frame_length = 486;
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(matches!(
        exact_solid_primitive(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &torus_scope,
            &[],
        ),
        Some(DesignSolidPrimitive::Torus(
            crate::records::feature::primitives::DesignTorusPrimitive {
                major_diameter,
                minor_diameter,
                operation: DesignExtrudeOperation::NewBody,
                ..
            }
        )) if major_diameter.get() == 15.0 && minor_diameter.get() == 4.0
    ));

    let offset_at = bytes.len();
    let mut offset = vec![0; 286];
    offset[25] = 1;
    offset[26..30].copy_from_slice(&73u32.to_le_bytes());
    bytes.extend_from_slice(&offset);
    let mut distance = vec![0; 104];
    distance[0..4].copy_from_slice(&3u32.to_le_bytes());
    distance[4..7].copy_from_slice(b"277");
    distance[7..11].copy_from_slice(&73u32.to_le_bytes());
    distance[40..48].copy_from_slice(&(-0.5f64).to_le_bytes());
    distance.extend_from_slice(&3u32.to_le_bytes());
    distance.extend_from_slice(b"261");
    distance.extend_from_slice(&73u32.to_le_bytes());
    bytes.extend_from_slice(&distance);
    let mut offset_scope = scope.clone();
    offset_scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(offset_at);
            draft.payload = crate::records::feature::scope::DesignFeatureKind::OffsetFaces
                .try_into()
                .unwrap();
            draft.frame_length = 286;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![1, 2, 3, 73]);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(matches!(
        exact_direct_face_operation(&bytes, &crate::design::test_support::indexed_record_offsets_for_test(&bytes), &offset_scope),
        Some(DesignDirectFaceOperation::OffsetFaces(
            crate::records::feature::direct_face::DesignOffsetFacesOperation {
                distance,
                distance_record_index: 73,
                ..
            }
        )) if distance.get() == -0.5
    ));

    let compact_offset_at = bytes.len();
    let mut compact_offset = vec![0; 275];
    compact_offset[25] = 1;
    compact_offset[26..30].copy_from_slice(&1_777u32.to_le_bytes());
    bytes.extend_from_slice(&compact_offset);
    let mut compact_distance = vec![0; 105];
    compact_distance[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_distance[4..7].copy_from_slice(b"312");
    compact_distance[7..11].copy_from_slice(&1_777u32.to_le_bytes());
    compact_distance[24] = 1;
    compact_distance[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
    compact_distance[40..48].copy_from_slice(&0.254f64.to_le_bytes());
    compact_distance.extend_from_slice(&3u32.to_le_bytes());
    compact_distance.extend_from_slice(b"259");
    compact_distance.extend_from_slice(&1_777u32.to_le_bytes());
    bytes.extend_from_slice(&compact_distance);
    offset_scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(compact_offset_at);
            draft.frame_length = 275;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![1, 2, 1_777]);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(matches!(
        exact_direct_face_operation(&bytes, &crate::design::test_support::indexed_record_offsets_for_test(&bytes), &offset_scope),
        Some(DesignDirectFaceOperation::OffsetFaces(
            crate::records::feature::direct_face::DesignOffsetFacesOperation {
                distance,
                distance_record_index: 1_777,
                ..
            }
        )) if distance.get() == 0.254
    ));

    // Prologue forms whose scope tail does not give the 253- or 264-byte
    // length: marker zero with the reference count at 126, and marker one
    // with it at 130. Both name scalar 73 and the face group, reference 1.
    for (marker_one, count_offset) in [(false, 126), (true, 130)] {
        let distance_marker = if marker_one { 25 } else { 21 };
        let prologue_at = bytes.len();
        let mut prologue = vec![0; 300];
        prologue[20] = u8::from(marker_one);
        prologue[distance_marker] = 1;
        prologue[distance_marker + 1..distance_marker + 5].copy_from_slice(&73u32.to_le_bytes());
        prologue[distance_marker + 11..distance_marker + 15].copy_from_slice(&1u32.to_le_bytes());
        prologue[distance_marker + 15] = 1;
        prologue[distance_marker + 16..distance_marker + 20].copy_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&prologue);
        let mut prologue_scope = offset_scope.clone();
        prologue_scope
            .try_edit(|draft| {
                draft.byte_offset = u64_from_index(prologue_at);
                draft.reference_members =
                    crate::records::identity::ReferenceRun::unlocated(vec![1, 2, 73]);
                draft.reference_count_offset = draft.byte_offset + count_offset;
                draft.layout_fixture_references();
                // The kind follows the reference table; the fixture tail follows the kind.
                draft.paired_byte_offset = draft.kind_offset + 22 + 72;
                draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                draft.layout_fixture_tail();
            })
            .unwrap();
        let operation =
            |bytes: &[u8], scope: &crate::records::feature::scope::DesignParameterScope| {
                exact_direct_face_operation(
                    bytes,
                    &crate::design::test_support::indexed_record_offsets_for_test(bytes),
                    scope,
                )
            };
        assert!(matches!(
            operation(&bytes, &prologue_scope),
            Some(DesignDirectFaceOperation::OffsetFaces(
                crate::records::feature::direct_face::DesignOffsetFacesOperation {
                    distance,
                    distance_record_index: 73,
                    ..
                }
            )) if distance.get() == -0.5
        ));
        // The u32 one and the face-group reference are part of the form.
        bytes[prologue_at + distance_marker + 11] = 2;
        assert!(operation(&bytes, &prologue_scope).is_none());
        bytes[prologue_at + distance_marker + 11] = 1;
        bytes[prologue_at + distance_marker + 16] = 2;
        assert!(operation(&bytes, &prologue_scope).is_none());
        bytes[prologue_at + distance_marker + 16] = 1;
        // The marker byte must agree with the reference-count offset.
        bytes[prologue_at + 20] = u8::from(!marker_one);
        assert!(operation(&bytes, &prologue_scope).is_none());
        bytes[prologue_at + 20] = u8::from(marker_one);
    }

    let thicken_at = bytes.len();
    let mut thicken = vec![0; 301];
    thicken[47] = 1;
    thicken[48..52].copy_from_slice(&74u32.to_le_bytes());
    bytes.extend_from_slice(&thicken);
    let mut thickness = vec![0; 104];
    thickness[0..4].copy_from_slice(&3u32.to_le_bytes());
    thickness[4..7].copy_from_slice(b"277");
    thickness[7..11].copy_from_slice(&74u32.to_le_bytes());
    thickness[40..48].copy_from_slice(&(-1.0f64).to_le_bytes());
    thickness.extend_from_slice(&3u32.to_le_bytes());
    thickness.extend_from_slice(b"261");
    thickness.extend_from_slice(&74u32.to_le_bytes());
    bytes.extend_from_slice(&thickness);
    let mut thicken_scope = scope.clone();
    thicken_scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(thicken_at);
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Thicken
                .try_into()
                .unwrap();
            draft.frame_length = 301;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![1, 2, 74]);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(matches!(
        exact_direct_face_operation(&bytes, &crate::design::test_support::indexed_record_offsets_for_test(&bytes), &thicken_scope),
        Some(DesignDirectFaceOperation::Thicken(
            crate::records::feature::direct_face::DesignThickenOperation {
                signed_thickness,
                thickness_record_index: 74,
                ..
            }
        )) if signed_thickness.get() == -1.0
    ));
    thicken_scope
        .try_edit(|draft| {
            draft.frame_length = 295;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_direct_face_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &thicken_scope
        ),
        None
    );
    let compact_thicken_at = bytes.len();
    let mut compact_thicken = vec![0; 295];
    compact_thicken[45] = 1;
    compact_thicken[46] = 1;
    compact_thicken[47..51].copy_from_slice(&74u32.to_le_bytes());
    bytes.extend_from_slice(&compact_thicken);
    thicken_scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(compact_thicken_at);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(matches!(
        exact_direct_face_operation(&bytes, &crate::design::test_support::indexed_record_offsets_for_test(&bytes), &thicken_scope),
        Some(DesignDirectFaceOperation::Thicken(
            crate::records::feature::direct_face::DesignThickenOperation {
                signed_thickness,
                thickness_record_index: 74,
                ..
            }
        )) if signed_thickness.get() == -1.0
    ));
    let shifted_thicken_at = bytes.len();
    let mut shifted_thicken = vec![0; 312];
    shifted_thicken[34] = 1;
    shifted_thicken[35..39].copy_from_slice(&200u32.to_le_bytes());
    shifted_thicken[46..48].copy_from_slice(&[1, 1]);
    shifted_thicken[48..52].copy_from_slice(&74u32.to_le_bytes());
    bytes.extend_from_slice(&shifted_thicken);
    let shifted_thicken_scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            byte_offset: u64_from_index(shifted_thicken_at),
            reference_count_offset: u64_from_index(shifted_thicken_at + 9),
            frame_length: 312,
            reference_members: crate::records::identity::ReferenceRun::unlocated(vec![
                74, 200, 201, 202,
            ]),
            ..thicken_scope.clone().into_draft()
        }
        .with_fixture_layout(),
    )
    .unwrap();
    assert!(matches!(
        exact_direct_face_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &shifted_thicken_scope,
        ),
        Some(DesignDirectFaceOperation::Thicken(
            crate::records::feature::direct_face::DesignThickenOperation {
                signed_thickness,
                thickness_record_index: 74,
                ..
            }
        )) if signed_thickness.get() == -1.0
    ));
    {
        let construction = exact_direct_face_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &thicken_scope,
        );
        match (thicken_scope.payload_mut(), construction) {
            (
                crate::records::feature::scope::DesignScopePayloadMut::OffsetFaces(slot)
                | crate::records::feature::scope::DesignScopePayloadMut::DecalerLesFaces(slot),
                Some(crate::records::feature::direct_face::DesignDirectFaceOperation::OffsetFaces(
                    value,
                )),
            ) => *slot = Some(value),
            (
                crate::records::feature::scope::DesignScopePayloadMut::Shell(slot)
                | crate::records::feature::scope::DesignScopePayloadMut::Schale(slot),
                Some(crate::records::feature::direct_face::DesignDirectFaceOperation::Shell(value)),
            ) => *slot = Some(value),
            (
                crate::records::feature::scope::DesignScopePayloadMut::Thicken(slot),
                Some(crate::records::feature::direct_face::DesignDirectFaceOperation::Thicken(
                    value,
                )),
            ) => *slot = Some(value),
            _ => {}
        }
    }
    let thicken_group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "thicken-group".into(),
            scope_record_index: thicken_scope.record_index,
            scope_reference_ordinal: 0,
            record_index: 200,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("264".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 201,
                offset: 0,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: vec![crate::records::identity::Located {
                        value: 202,
                        offset: 0,
                    }],
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
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::ROLE_0X5,
                ),
            role_offset: 0,

            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "264".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap();
    assert!(matches!(
        crate::design::feature_project::project_thicken(&thicken_scope, &[], std::slice::from_ref(&thicken_group)).unwrap(),
        Some(cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Thicken {
            faces: cadmpeg_ir::features::FaceSelection::Native(native),
            thickness: Some(actual_thickness),
            side: Some(cadmpeg_ir::features::ThickenSide::Reverse),
        })) if (native == "thicken-group") && actual_thickness.get() == 10.0
    ));
    let mut bounded_face_thicken_group = thicken_group.clone();
    bounded_face_thicken_group.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::ROLE_0X12,
        );
    assert!(matches!(
        crate::design::feature_project::project_thicken(&thicken_scope, &[], std::slice::from_ref(&bounded_face_thicken_group)).unwrap(),
        Some(cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Thicken {
            faces: cadmpeg_ir::features::FaceSelection::Native(native),
            ..
        })) if native == "thicken-group"
    ));
    let shell_at = bytes.len();
    let mut shell = vec![0; 278];
    shell[25] = 1;
    shell[27] = 1;
    shell[28..32].copy_from_slice(&1_778u32.to_le_bytes());
    shell[51..55].copy_from_slice(&1u32.to_le_bytes());
    shell[55] = 1;
    shell[56..60].copy_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&shell);
    let mut shell_thickness = vec![0; 105];
    shell_thickness[0..4].copy_from_slice(&3u32.to_le_bytes());
    shell_thickness[4..7].copy_from_slice(b"321");
    shell_thickness[7..11].copy_from_slice(&1_778u32.to_le_bytes());
    shell_thickness[24] = 1;
    shell_thickness[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
    shell_thickness[40..48].copy_from_slice(&0.5f64.to_le_bytes());
    shell_thickness.extend_from_slice(&3u32.to_le_bytes());
    shell_thickness.extend_from_slice(b"265");
    shell_thickness.extend_from_slice(&1_778u32.to_le_bytes());
    bytes.extend_from_slice(&shell_thickness);
    let mut shell_scope = scope.clone();
    shell_scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(shell_at);
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Shell
                .try_into()
                .unwrap();
            draft.frame_length = 278;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![200, 201, 1_778]);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    {
        let construction = exact_direct_face_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &shell_scope,
        );
        match (shell_scope.payload_mut(), construction) {
            (
                crate::records::feature::scope::DesignScopePayloadMut::OffsetFaces(slot)
                | crate::records::feature::scope::DesignScopePayloadMut::DecalerLesFaces(slot),
                Some(crate::records::feature::direct_face::DesignDirectFaceOperation::OffsetFaces(
                    value,
                )),
            ) => *slot = Some(value),
            (
                crate::records::feature::scope::DesignScopePayloadMut::Shell(slot)
                | crate::records::feature::scope::DesignScopePayloadMut::Schale(slot),
                Some(crate::records::feature::direct_face::DesignDirectFaceOperation::Shell(value)),
            ) => *slot = Some(value),
            (
                crate::records::feature::scope::DesignScopePayloadMut::Thicken(slot),
                Some(crate::records::feature::direct_face::DesignDirectFaceOperation::Thicken(
                    value,
                )),
            ) => *slot = Some(value),
            _ => {}
        }
    }
    assert!(matches!(
        &shell_scope.payload(),
        crate::records::feature::scope::DesignScopePayload::Shell(Some(
            crate::records::feature::direct_face::DesignShellOperation {
                thickness,
                thickness_record_index: 1_778,
                outward: true,
                ..
            }
        )) if thickness.get() == 0.5
    ));
    let mut shell_group = thicken_group.clone();
    shell_group.id = "shell-group".into();
    shell_group.scope_record_index = shell_scope.record_index;
    shell_group.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::ROLE_0X10,
        );
    assert!(matches!(
        crate::design::feature_project::project_shell(&shell_scope, &[], std::slice::from_ref(&shell_group)).unwrap(),
        Some(cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Shell {
            removed_faces: cadmpeg_ir::features::FaceSelection::Native(native),
            thickness: Some(actual_thickness),
            outward: Some(true),
            ..
        })) if (native == "shell-group") && actual_thickness.get() == 5.0
    ));
    let compact_shell_at = bytes.len();
    let mut compact_shell = vec![0; 268];
    compact_shell[21] = 1;
    compact_shell[22] = 1;
    compact_shell[23..27].copy_from_slice(&9_000u32.to_le_bytes());
    compact_shell[42..46].copy_from_slice(&1u32.to_le_bytes());
    compact_shell[46] = 1;
    compact_shell[47..51].copy_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&compact_shell);
    let mut compact_shell_thickness = vec![0; 103];
    compact_shell_thickness[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_shell_thickness[4..7].copy_from_slice(b"354");
    compact_shell_thickness[7..11].copy_from_slice(&9_000u32.to_le_bytes());
    compact_shell_thickness[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
    compact_shell_thickness[24] = 1;
    compact_shell_thickness[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
    compact_shell_thickness[40..48].copy_from_slice(&0.25f64.to_le_bytes());
    compact_shell_thickness[48] = 1;
    compact_shell_thickness[49..53].copy_from_slice(&9_001u32.to_le_bytes());
    compact_shell_thickness[59..63].copy_from_slice(&10u32.to_le_bytes());
    compact_shell_thickness[67] = 1;
    compact_shell_thickness[68..72].copy_from_slice(&scope.record_index.to_le_bytes());
    compact_shell_thickness[80] = 1;
    compact_shell_thickness[81..85].copy_from_slice(&9_002u32.to_le_bytes());
    compact_shell_thickness[92] = 1;
    compact_shell_thickness[93..97].copy_from_slice(&scope.record_index.to_le_bytes());
    compact_shell_thickness.extend_from_slice(&3u32.to_le_bytes());
    compact_shell_thickness.extend_from_slice(b"258");
    compact_shell_thickness.extend_from_slice(&9_000u32.to_le_bytes());
    bytes.extend_from_slice(&compact_shell_thickness);
    let mut compact_shell_scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            byte_offset: u64_from_index(compact_shell_at),
            reference_count_offset: u64_from_index(compact_shell_at + 9),
            frame_length: 268,
            reference_members: crate::records::identity::ReferenceRun::unlocated(vec![
                200, 201, 9_000,
            ]),
            ..shell_scope.clone().into_draft()
        }
        .with_fixture_layout(),
    )
    .unwrap();
    assert!(matches!(
        exact_direct_face_operation(&bytes, &crate::design::test_support::indexed_record_offsets_for_test(&bytes), &compact_shell_scope),
        Some(DesignDirectFaceOperation::Shell(crate::records::feature::direct_face::DesignShellOperation {
            thickness,
            thickness_record_index: 9_000,
            outward: true,
            outward_offset,
            ..
        })) if thickness.get() == 0.25 && outward_offset == u64_from_index(compact_shell_at + 21)
    ));
    let shifted_shell_at = bytes.len();
    let mut shifted_shell = vec![0; 278];
    shifted_shell[20] = 1;
    shifted_shell[25] = 1;
    shifted_shell[27] = 1;
    shifted_shell[28..32].copy_from_slice(&9_000u32.to_le_bytes());
    shifted_shell[51..55].copy_from_slice(&1u32.to_le_bytes());
    shifted_shell[55] = 1;
    shifted_shell[56..60].copy_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&shifted_shell);
    let shifted_shell_scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            byte_offset: u64_from_index(shifted_shell_at),
            reference_count_offset: u64_from_index(shifted_shell_at + 9),
            frame_length: 278,
            reference_members: crate::records::identity::ReferenceRun::unlocated(vec![
                9_000, 200, 201,
            ]),
            ..shell_scope.clone().into_draft()
        }
        .with_fixture_layout(),
    )
    .unwrap();
    assert!(matches!(
        exact_direct_face_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &shifted_shell_scope,
        ),
        Some(DesignDirectFaceOperation::Shell(crate::records::feature::direct_face::DesignShellOperation {
            thickness,
            thickness_record_index: 9_000,
            outward: false,
            outward_offset,
            ..
        })) if thickness.get() == 0.25 && outward_offset == u64_from_index(shifted_shell_at + 21)
    ));
    let thickness_first_shell_at = bytes.len();
    let mut thickness_first_shell = vec![0; 278];
    thickness_first_shell[20] = 1;
    thickness_first_shell[21] = 1;
    thickness_first_shell[25..29].copy_from_slice(&[1, 0, 0, 1]);
    thickness_first_shell[29..33].copy_from_slice(&9_000u32.to_le_bytes());
    thickness_first_shell[51..55].copy_from_slice(&1u32.to_le_bytes());
    thickness_first_shell[55] = 1;
    thickness_first_shell[56..60].copy_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&thickness_first_shell);
    let thickness_first_shell_scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            byte_offset: u64_from_index(thickness_first_shell_at),
            reference_count_offset: u64_from_index(thickness_first_shell_at + 9),
            frame_length: 278,
            reference_members: crate::records::identity::ReferenceRun::unlocated(vec![
                9_000, 200, 201,
            ]),
            ..shell_scope.clone().into_draft()
        }
        .with_fixture_layout(),
    )
    .unwrap();
    assert!(matches!(
        exact_direct_face_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &thickness_first_shell_scope,
        ),
        Some(DesignDirectFaceOperation::Shell(crate::records::feature::direct_face::DesignShellOperation {
            thickness,
            thickness_record_index: 9_000,
            outward: true,
            outward_offset,
            ..
        })) if thickness.get() == 0.25 && outward_offset == u64_from_index(thickness_first_shell_at + 21)
    ));

    // Negative rejection tests for the 268-byte thickness-first Shell scope:
    // 1. byte 27 = 1 rejects because the [1, 0, 0, 1] marker at 25..29 requires byte 27 == 0.
    let neg_byte_27_at = bytes.len();
    let mut neg_byte_27 = thickness_first_shell.clone();
    neg_byte_27[27] = 1;
    bytes.extend_from_slice(&neg_byte_27);
    let neg_byte_27_scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            byte_offset: u64_from_index(neg_byte_27_at),
            reference_count_offset: u64_from_index(neg_byte_27_at + 9),
            frame_length: 278,
            reference_members: crate::records::identity::ReferenceRun::unlocated(vec![
                9_000, 200, 201,
            ]),
            ..shell_scope.clone().into_draft()
        }
        .with_fixture_layout(),
    )
    .unwrap();
    assert_eq!(
        exact_direct_face_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &neg_byte_27_scope,
        ),
        None
    );

    // 2. outward = 2 rejects because outward admits only 0 or 1.
    let neg_outward_at = bytes.len();
    let mut neg_outward = thickness_first_shell.clone();
    neg_outward[21] = 2;
    bytes.extend_from_slice(&neg_outward);
    let neg_outward_scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            byte_offset: u64_from_index(neg_outward_at),
            reference_count_offset: u64_from_index(neg_outward_at + 9),
            frame_length: 278,
            reference_members: crate::records::identity::ReferenceRun::unlocated(vec![
                9_000, 200, 201,
            ]),
            ..shell_scope.clone().into_draft()
        }
        .with_fixture_layout(),
    )
    .unwrap();
    assert_eq!(
        exact_direct_face_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &neg_outward_scope,
        ),
        None
    );

    // 3. thickness/first-reference mismatch rejects when the u32 at offset 29 disagrees with the first member.
    let neg_mismatch_at = bytes.len();
    bytes.extend_from_slice(&thickness_first_shell);
    let neg_mismatch_scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            byte_offset: u64_from_index(neg_mismatch_at),
            reference_count_offset: u64_from_index(neg_mismatch_at + 9),
            frame_length: 278,
            reference_members: crate::records::identity::ReferenceRun::unlocated(vec![
                9_001, 200, 201,
            ]),
            ..shell_scope.clone().into_draft()
        }
        .with_fixture_layout(),
    )
    .unwrap();
    assert_eq!(
        exact_direct_face_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &neg_mismatch_scope,
        ),
        None
    );
    {
        let construction = exact_direct_face_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &compact_shell_scope,
        );
        match (compact_shell_scope.payload_mut(), construction) {
            (
                crate::records::feature::scope::DesignScopePayloadMut::OffsetFaces(slot)
                | crate::records::feature::scope::DesignScopePayloadMut::DecalerLesFaces(slot),
                Some(crate::records::feature::direct_face::DesignDirectFaceOperation::OffsetFaces(
                    value,
                )),
            ) => *slot = Some(value),
            (
                crate::records::feature::scope::DesignScopePayloadMut::Shell(slot)
                | crate::records::feature::scope::DesignScopePayloadMut::Schale(slot),
                Some(crate::records::feature::direct_face::DesignDirectFaceOperation::Shell(value)),
            ) => *slot = Some(value),
            (
                crate::records::feature::scope::DesignScopePayloadMut::Thicken(slot),
                Some(crate::records::feature::direct_face::DesignDirectFaceOperation::Thicken(
                    value,
                )),
            ) => *slot = Some(value),
            _ => {}
        }
    }
    shell_group.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::BODIES_A,
        );
    assert!(matches!(
        crate::design::feature_project::project_shell(&compact_shell_scope, &[], std::slice::from_ref(&shell_group)).unwrap(),
        Some(cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Shell {
            bodies: Some(cadmpeg_ir::features::BodySelection::Native(body)),
            removed_faces: cadmpeg_ir::features::FaceSelection::Faces(removed),
            thickness: Some(actual_thickness),
            outward: Some(true),
            ..
        })) if (body == "shell-group" && removed.is_empty()) && actual_thickness.get() == 2.5
    ));
    {
        let construction = exact_direct_face_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &offset_scope,
        );
        match (offset_scope.payload_mut(), construction) {
            (
                crate::records::feature::scope::DesignScopePayloadMut::OffsetFaces(slot)
                | crate::records::feature::scope::DesignScopePayloadMut::DecalerLesFaces(slot),
                Some(crate::records::feature::direct_face::DesignDirectFaceOperation::OffsetFaces(
                    value,
                )),
            ) => *slot = Some(value),
            (
                crate::records::feature::scope::DesignScopePayloadMut::Shell(slot)
                | crate::records::feature::scope::DesignScopePayloadMut::Schale(slot),
                Some(crate::records::feature::direct_face::DesignDirectFaceOperation::Shell(value)),
            ) => *slot = Some(value),
            (
                crate::records::feature::scope::DesignScopePayloadMut::Thicken(slot),
                Some(crate::records::feature::direct_face::DesignDirectFaceOperation::Thicken(
                    value,
                )),
            ) => *slot = Some(value),
            _ => {}
        }
    }
    let mut offset_group = thicken_group.clone();
    offset_group.id = "offset-group".into();
    offset_group.scope_record_index = offset_scope.record_index;
    offset_group.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::ROLE_0X10,
        );
    assert!(matches!(
        crate::design::feature_project::project_offset_faces(&offset_scope, &[], &[], std::slice::from_ref(&offset_group)).unwrap(),
        Some(cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::MoveFace {
            faces: cadmpeg_ir::features::FaceSelection::Native(native),
            motion: cadmpeg_ir::features::FaceMotion::Offset {
                distance: actual_distance
            },
        })) if (native == "offset-group") && actual_distance.get() == 2.54
    ));
    bytes[compact_thicken_at + 46] = 0;
    assert_eq!(
        exact_direct_face_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &thicken_scope
        ),
        None
    );

    for (record_index, ordinal, value) in [(75u32, 0u8, -2.0f64), (76, 1, 0.0)] {
        let mut scalar = vec![0; 104];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"277");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"261");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    let mut extrude_scope = scope.clone();
    extrude_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Extrude
                .try_into()
                .unwrap();
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::Extrusion(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::Extrusao(slot) =
        extrude_scope.payload_mut()
    {
        slot.get_or_insert_with(Default::default).extrude_prologue =
            Some(DesignExtrudePrologue::ReferenceAware {
                reference: None,
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: 28,
                direction_face_extend_values: [1, 2],
                side_extent_discriminators: [1, 0],
                side_extent_discriminator_offsets: [77, 90],
                first_side_target_ordinal: None,
                extent: DesignExtrudeExtent::OneSidedDistance,
                direction_face_extend_offsets: [32, 36],
                direction_reversed: false,
                direction_reversed_offset: 40,
                solid_operation: true,
                solid_operation_offset: 41,
                start: DesignExtrudeStart::ProfilePlane,
                start_offset: 42,
            });
    }
    extrude_scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![50, 75, 76, 51]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_fixed_extrude_parameters(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &extrude_scope,
            &[],
            &[],
        ),
        Some(DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(
                DesignFixedExtrudeScalar {
                    value: cadmpeg_ir::scalar::NonZeroReal::new(-2.0).unwrap(),
                    record_index: 75,
                    value_offset: u64_from_index(bytes.len() - 2 * 115 + 40),
                },
            )),
            taper_angle: Some(DesignFixedExtrudeScalar {
                value: crate::test_support::real(0.0),
                record_index: 76,
                value_offset: u64_from_index(bytes.len() - 115 + 40),
            }),
            against_distance: None,
            side_two_taper_angle: None,
            profile_offset: None,
            side_one_offset: None,
            ..Default::default()
        })
    );
    extrude_scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![50, 75, 51]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_fixed_extrude_parameters(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &extrude_scope,
            &[],
            &[],
        ),
        Some(DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(
                DesignFixedExtrudeScalar {
                    value: cadmpeg_ir::scalar::NonZeroReal::new(-2.0).unwrap(),
                    record_index: 75,
                    value_offset: u64_from_index(bytes.len() - 2 * 115 + 40),
                },
            )),
            taper_angle: None,
            against_distance: None,
            side_two_taper_angle: None,
            profile_offset: None,
            side_one_offset: None,
            ..Default::default()
        })
    );
    extrude_scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![50, 75, 76, 51]);
            draft.reference_members = {
                let mut values: Vec<u32> = draft.reference_members.values().copied().collect();
                values.push(75);
                crate::records::identity::ReferenceRun::unlocated(values)
            };
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_fixed_extrude_parameters(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &extrude_scope,
            &[],
            &[],
        ),
        None
    );

    let extend_distance_at = bytes.len();
    let extend_distance_record_index = 400u32;
    let extend_boundary_record_index = 500u32;
    let extend_edge_record_indices = [503u32, 507u32];
    let mut extend_distance = vec![0; 104];
    extend_distance[0..4].copy_from_slice(&3u32.to_le_bytes());
    extend_distance[4..7].copy_from_slice(b"299");
    extend_distance[7..11].copy_from_slice(&extend_distance_record_index.to_le_bytes());
    extend_distance[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
    extend_distance[24] = 1;
    extend_distance[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
    extend_distance[35] = 0;
    extend_distance[40..48].copy_from_slice(&0.04f64.to_le_bytes());
    extend_distance[48] = 1;
    extend_distance[49..53].copy_from_slice(&(extend_distance_record_index - 1).to_le_bytes());
    extend_distance[59..63].copy_from_slice(&1016u32.to_le_bytes());
    extend_distance[67] = 1;
    extend_distance[68..72].copy_from_slice(&scope.record_index.to_le_bytes());
    extend_distance[78..81].copy_from_slice(&[1, 0, 0]);
    extend_distance[81] = 1;
    extend_distance[82..86].copy_from_slice(&(extend_distance_record_index + 1).to_le_bytes());
    extend_distance[93] = 1;
    extend_distance[94..98].copy_from_slice(&scope.record_index.to_le_bytes());
    extend_distance.extend_from_slice(&3u32.to_le_bytes());
    extend_distance.extend_from_slice(b"258");
    extend_distance.extend_from_slice(&extend_distance_record_index.to_le_bytes());
    bytes.extend_from_slice(&extend_distance);

    let extend_boundary_at = bytes.len();
    let extend_boundary_tail = 25 + extend_edge_record_indices.len() * 11;
    let mut extend_boundary = vec![0; 113 + extend_edge_record_indices.len() * 11];
    extend_boundary[0..4].copy_from_slice(&3u32.to_le_bytes());
    extend_boundary[4..7].copy_from_slice(b"290");
    extend_boundary[7..11].copy_from_slice(&extend_boundary_record_index.to_le_bytes());
    extend_boundary[21..25].copy_from_slice(
        &(u32::try_from(extend_edge_record_indices.len()).expect("fixture value fits u32"))
            .to_le_bytes(),
    );
    for (ordinal, record_index) in extend_edge_record_indices.iter().enumerate() {
        let at = 25 + ordinal * 11;
        extend_boundary[at] = 1;
        extend_boundary[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    }
    extend_boundary[extend_boundary_tail + 2..extend_boundary_tail + 6]
        .copy_from_slice(&1u32.to_le_bytes());
    extend_boundary[extend_boundary_tail + 6] = 1;
    extend_boundary[extend_boundary_tail + 7..extend_boundary_tail + 11]
        .copy_from_slice(&900u32.to_le_bytes());
    extend_boundary[extend_boundary_tail + 21..extend_boundary_tail + 25]
        .copy_from_slice(&8u32.to_le_bytes());
    extend_boundary[extend_boundary_tail + 35..extend_boundary_tail + 39]
        .copy_from_slice(&210u32.to_le_bytes());
    extend_boundary[extend_boundary_tail + 39..extend_boundary_tail + 47]
        .copy_from_slice(&1.0e-6f64.to_le_bytes());
    extend_boundary[extend_boundary_tail + 47..extend_boundary_tail + 51]
        .copy_from_slice(&210u32.to_le_bytes());
    extend_boundary[extend_boundary_tail + 51] = 1;
    extend_boundary[extend_boundary_tail + 52..extend_boundary_tail + 56]
        .copy_from_slice(&(extend_boundary_record_index + 2).to_le_bytes());
    extend_boundary[extend_boundary_tail + 62..extend_boundary_tail + 65]
        .copy_from_slice(&[1, 0, 0]);
    extend_boundary[extend_boundary_tail + 65] = 1;
    extend_boundary[extend_boundary_tail + 66..extend_boundary_tail + 70]
        .copy_from_slice(&(extend_boundary_record_index + 1).to_le_bytes());
    extend_boundary[extend_boundary_tail + 77] = 1;
    extend_boundary[extend_boundary_tail + 78..extend_boundary_tail + 82]
        .copy_from_slice(&scope.record_index.to_le_bytes());
    extend_boundary.extend_from_slice(&3u32.to_le_bytes());
    extend_boundary.extend_from_slice(b"258");
    extend_boundary.extend_from_slice(&extend_boundary_record_index.to_le_bytes());
    bytes.extend_from_slice(&extend_boundary);

    let mut extend_scope = scope.clone();
    extend_scope.id = "f3d:native/BulkStream.dat:parameter-scope#12".into();
    extend_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::SurfaceExtend
                .try_into()
                .unwrap();
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                extend_distance_record_index,
                extend_boundary_record_index,
                extend_edge_record_indices[0],
                extend_edge_record_indices[1],
            ]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let operation = tested_surface_extend_operation(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &extend_scope,
    )
    .expect("exact SurfaceExtend construction");
    assert_eq!(
        operation,
        DesignSurfaceExtendOperation {
            distance: crate::test_support::real(0.04),
            distance_offset: u64_from_index(extend_distance_at + 40),
            distance_record_index: extend_distance_record_index,
            method: DesignSurfaceExtendMethod::Tangent,
            method_offset: u64_from_index(extend_boundary_at + extend_boundary_tail + 2),
            boundary_record_index: extend_boundary_record_index,
            boundary_reference_record_index: 900,
            boundary_reference_offset: u64_from_index(
                extend_boundary_at + extend_boundary_tail + 6
            ),
            edge_record_indices: extend_edge_record_indices.to_vec(),
            tolerance: cadmpeg_ir::scalar::PositiveReal::new(1.0e-6)
                .expect("checked fixture value"),
            tolerance_offset: u64_from_index(extend_boundary_at + extend_boundary_tail + 39),
        }
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::SurfaceExtend(slot) =
        extend_scope.payload_mut()
    {
        *slot = Some(operation);
    }
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&extend_scope);
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
    assert!(matches!(
        features.as_slice(), [Feature {
            evaluation,
            ..
        }] if matches!((evaluation.definition(),), (FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
                faces: FaceSelection::Native(native),
                distance: Some(distance),
                method: cadmpeg_ir::features::SurfaceExtension::Linear,
            }),) if native.ends_with(":design-record#500") && distance.get() == 0.4)));

    bytes[extend_distance_at + 40..extend_distance_at + 48]
        .copy_from_slice(&(-0.4f64).to_le_bytes());
    bytes[extend_boundary_at + extend_boundary_tail + 21
        ..extend_boundary_at + extend_boundary_tail + 25]
        .copy_from_slice(&65u32.to_le_bytes());
    extend_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::SurfaceOffset
                .try_into()
                .unwrap();
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::SurfaceExtend(slot) =
        extend_scope.payload_mut()
    {
        *slot = None;
    }
    let operation = tested_surface_offset_operation(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &extend_scope,
    )
    .expect("exact SurfaceOffset construction");
    assert_eq!(
        operation,
        DesignSurfaceOffsetOperation {
            distance: crate::test_support::real(-0.4),
            distance_offset: u64_from_index(extend_distance_at + 40),
            distance_record_index: extend_distance_record_index,
            support: DesignSurfaceOffsetSupport::BoundaryCarrier {
                boundary_record_index: extend_boundary_record_index,
                boundary_reference_record_index: 900,
                boundary_reference_offset: u64_from_index(
                    extend_boundary_at + extend_boundary_tail + 6
                ),
                edge_record_indices: extend_edge_record_indices.to_vec(),
                tolerance: cadmpeg_ir::scalar::PositiveReal::new(1.0e-6)
                    .expect("checked fixture value"),
                tolerance_offset: u64_from_index(extend_boundary_at + extend_boundary_tail + 39),
            },
        }
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::SurfaceOffset(slot) =
        extend_scope.payload_mut()
    {
        *slot = Some(operation);
    }
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&extend_scope);
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
    assert!(matches!(
        features.as_slice(), [Feature {
            evaluation,
            ..
        }] if matches!((evaluation.definition(),), (FeatureDefinition::Operation(FeatureOperation::OffsetSurface {
                faces: FaceSelection::Native(native),
                distance: Some(distance),
            }),) if native.ends_with(":design-record#500") && distance.get() == -4.0)));

    let grouped_record_index = 600u32;
    let grouped_member_record_index = 601u32;
    let mut grouped = Vec::new();
    grouped.extend_from_slice(&3u32.to_le_bytes());
    grouped.extend_from_slice(b"282");
    grouped.extend_from_slice(&grouped_record_index.to_le_bytes());
    grouped.extend_from_slice(&[0; 10]);
    grouped.extend_from_slice(&1u32.to_le_bytes());
    grouped.push(1);
    grouped.extend_from_slice(&grouped_member_record_index.to_le_bytes());
    grouped.extend_from_slice(&[0; 6]);
    grouped.extend_from_slice(&[0; 2]);
    grouped.extend_from_slice(&1u32.to_le_bytes());
    grouped.push(1);
    grouped.extend_from_slice(&(grouped_record_index + 2).to_le_bytes());
    grouped.extend_from_slice(&[0; 6]);
    grouped.extend_from_slice(&0x0000_0041_0000_0000u64.to_le_bytes());
    grouped.extend_from_slice(&[0; 10]);
    grouped.extend_from_slice(&252u32.to_le_bytes());
    grouped.extend_from_slice(&0.0001f64.to_le_bytes());
    grouped.extend_from_slice(&252u32.to_le_bytes());
    grouped.push(1);
    grouped.extend_from_slice(&(grouped_record_index + 2).to_le_bytes());
    grouped.extend_from_slice(&[0; 6]);
    grouped.extend_from_slice(&[1, 1, 0, 1]);
    grouped.extend_from_slice(&(grouped_record_index + 1).to_le_bytes());
    grouped.extend_from_slice(&[0; 6]);
    grouped.push(0);
    grouped.push(1);
    grouped.extend_from_slice(&extend_scope.record_index.to_le_bytes());
    grouped.extend_from_slice(&[0; 6]);
    grouped.extend_from_slice(&3u32.to_le_bytes());
    grouped.extend_from_slice(b"260");
    grouped.extend_from_slice(&grouped_record_index.to_le_bytes());
    bytes.extend_from_slice(&grouped);
    let mut grouped_scope = extend_scope.clone();
    grouped_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                extend_distance_record_index,
                grouped_record_index,
                grouped_member_record_index,
            ]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let grouped_operation = tested_surface_offset_operation(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &grouped_scope,
    )
    .expect("exact grouped SurfaceOffset construction");
    assert_eq!(
        grouped_operation,
        DesignSurfaceOffsetOperation {
            distance: crate::test_support::real(-0.4),
            distance_offset: u64_from_index(extend_distance_at + 40),
            distance_record_index: extend_distance_record_index,
            support: DesignSurfaceOffsetSupport::FaceGroups {
                group_record_indices: vec![grouped_record_index],
            },
        }
    );

    bytes[extend_boundary_at + 21..extend_boundary_at + 25]
        .copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        tested_surface_offset_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &extend_scope,
        ),
        None
    );

    let embedded_default_at = bytes.len();
    for (record_index, ordinal) in [(273u32, 0u8), (274, 1)] {
        let mut scalar = vec![0; 104];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"277");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal;
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"261");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    let embedded_distance_at = bytes.len();
    let embedded_distance_record_index = 275u32;
    let mut embedded_distance = vec![0; 100];
    embedded_distance[0..4].copy_from_slice(&3u32.to_le_bytes());
    embedded_distance[4..7].copy_from_slice(b"314");
    embedded_distance[7..11].copy_from_slice(&embedded_distance_record_index.to_le_bytes());
    embedded_distance[21] = 1;
    embedded_distance[22..26].copy_from_slice(&scope.record_index.to_le_bytes());
    embedded_distance[32..36].copy_from_slice(&1u32.to_le_bytes());
    embedded_distance[36] = 1;
    embedded_distance[37..41].copy_from_slice(&999u32.to_le_bytes());
    embedded_distance[47..51].copy_from_slice(&210u32.to_le_bytes());
    embedded_distance[51..59].copy_from_slice(&0.25f64.to_le_bytes());
    embedded_distance[59..63].copy_from_slice(&210u32.to_le_bytes());
    embedded_distance[63] = 1;
    embedded_distance[64..68].copy_from_slice(&(embedded_distance_record_index + 2).to_le_bytes());
    embedded_distance[74] = 1;
    embedded_distance[77] = 1;
    embedded_distance[78..82].copy_from_slice(&(embedded_distance_record_index + 1).to_le_bytes());
    embedded_distance[89] = 1;
    embedded_distance[90..94].copy_from_slice(&scope.record_index.to_le_bytes());
    embedded_distance.extend_from_slice(&3u32.to_le_bytes());
    embedded_distance.extend_from_slice(b"258");
    embedded_distance.extend_from_slice(&embedded_distance_record_index.to_le_bytes());
    bytes.extend_from_slice(&embedded_distance);
    extrude_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                50,
                273,
                274,
                embedded_distance_record_index,
                51,
            ]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_fixed_extrude_parameters(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &extrude_scope,
            &[],
            &[],
        ),
        Some(DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::DistanceConstruction(
                DesignFixedExtrudeScalar {
                    value: cadmpeg_ir::scalar::PositiveReal::new(0.25).unwrap(),
                    record_index: embedded_distance_record_index,
                    value_offset: u64_from_index(embedded_distance_at + 51),
                },
            )),
            taper_angle: Some(DesignFixedExtrudeScalar {
                value: crate::test_support::real(0.0),
                record_index: 274,
                value_offset: u64_from_index(embedded_default_at + 115 + 40),
            }),
            against_distance: None,
            side_two_taper_angle: None,
            profile_offset: None,
            side_one_offset: None,
            ..Default::default()
        })
    );
    bytes[embedded_default_at + 115 + 40..embedded_default_at + 115 + 48]
        .copy_from_slice(&0.1f64.to_le_bytes());
    assert_eq!(
        exact_fixed_extrude_parameters(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &extrude_scope,
            &[],
            &[],
        ),
        Some(DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::DistanceConstruction(
                DesignFixedExtrudeScalar {
                    value: cadmpeg_ir::scalar::PositiveReal::new(0.25).unwrap(),
                    record_index: embedded_distance_record_index,
                    value_offset: u64_from_index(embedded_distance_at + 51),
                },
            )),
            taper_angle: Some(DesignFixedExtrudeScalar {
                value: crate::test_support::real(0.1),
                record_index: 274,
                value_offset: u64_from_index(embedded_default_at + 115 + 40),
            }),
            against_distance: None,
            side_two_taper_angle: None,
            profile_offset: None,
            side_one_offset: None,
            ..Default::default()
        })
    );
    bytes[embedded_default_at + 115 + 40..embedded_default_at + 115 + 48]
        .copy_from_slice(&0.0f64.to_le_bytes());
    extrude_scope
        .try_edit(|draft| {
            draft.reference_members = {
                let mut values: Vec<u32> = draft.reference_members.values().copied().collect();
                values.insert(2, 273);
                crate::records::identity::ReferenceRun::unlocated(values)
            };
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_fixed_extrude_parameters(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &extrude_scope,
            &[],
            &[],
        ),
        None
    );

    bytes[embedded_default_at + 115 + 40..embedded_default_at + 115 + 48]
        .copy_from_slice(&1.5f64.to_le_bytes());
    extrude_scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![50, 273, 274, 51]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_fixed_extrude_parameters(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &extrude_scope,
            &[],
            &[],
        ),
        Some(DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(
                DesignFixedExtrudeScalar {
                    value: cadmpeg_ir::scalar::NonZeroReal::new(1.5).unwrap(),
                    record_index: 274,
                    value_offset: u64_from_index(embedded_default_at + 115 + 40),
                },
            )),
            taper_angle: Some(DesignFixedExtrudeScalar {
                value: crate::test_support::real(0.0),
                record_index: 273,
                value_offset: u64_from_index(embedded_default_at + 40),
            }),
            against_distance: None,
            side_two_taper_angle: None,
            profile_offset: None,
            side_one_offset: None,
            ..Default::default()
        })
    );
    extrude_scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![50, 274, 273, 51]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_fixed_extrude_parameters(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &extrude_scope,
            &[],
            &[],
        ),
        Some(DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(
                DesignFixedExtrudeScalar {
                    value: cadmpeg_ir::scalar::NonZeroReal::new(1.5).unwrap(),
                    record_index: 274,
                    value_offset: u64_from_index(embedded_default_at + 115 + 40),
                },
            )),
            taper_angle: Some(DesignFixedExtrudeScalar {
                value: crate::test_support::real(0.0),
                record_index: 273,
                value_offset: u64_from_index(embedded_default_at + 40),
            }),
            against_distance: None,
            side_two_taper_angle: None,
            profile_offset: None,
            side_one_offset: None,
            ..Default::default()
        })
    );
    bytes[embedded_default_at + 115 + 40..embedded_default_at + 115 + 48]
        .copy_from_slice(&0.0f64.to_le_bytes());

    let mut offset_24_bytes = vec![0; 120];
    offset_24_bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    offset_24_bytes[4..7].copy_from_slice(b"375");
    offset_24_bytes[7..11].copy_from_slice(&1u32.to_le_bytes());
    offset_24_bytes[24..28].copy_from_slice(&4u32.to_le_bytes());
    offset_24_bytes[28..32].copy_from_slice(&1u32.to_le_bytes());
    offset_24_bytes[32..36].copy_from_slice(&2u32.to_le_bytes());
    offset_24_bytes[36] = 0;
    offset_24_bytes[37] = 1;
    offset_24_bytes[38] = 0;
    offset_24_bytes[42..50].copy_from_slice(&1.0f64.to_le_bytes());
    offset_24_bytes[73..77].copy_from_slice(&1u32.to_le_bytes());
    let decoded_offset_24 = crate::design::decode::scopes::extrude::exact_extrude_prologue(
        &offset_24_bytes,
        0,
        120,
        "375",
        "260",
        100,
        &[],
    );
    assert!(matches!(
        decoded_offset_24,
        Some(DesignExtrudePrologue::ReferenceAware {
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 24,
            extent: DesignExtrudeExtent::OneSidedDistance,
            ..
        })
    ));

    let mut scope_375 = extrude_scope.clone();
    scope_375.class_tag =
        crate::records::references::DesignClassTag::try_from("375".to_owned()).unwrap();
    scope_375.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("260".to_owned()).unwrap();
    let to_face_prologue = DesignExtrudePrologue::ReferenceAware {
        reference: None,
        operation: DesignExtrudeOperation::Join,
        operation_offset: 28,
        direction_face_extend_values: [1, 2],
        side_extent_discriminators: [2, 0],
        side_extent_discriminator_offsets: [77, 90],
        first_side_target_ordinal: None,
        extent: DesignExtrudeExtent::OneSidedToFace,
        direction_face_extend_offsets: [32, 36],
        direction_reversed: false,
        direction_reversed_offset: 40,
        solid_operation: true,
        solid_operation_offset: 41,
        start: DesignExtrudeStart::ProfilePlane,
        start_offset: 42,
    };
    for length in [477, 481] {
        scope_375
            .try_edit(|draft| {
                draft.frame_length = length;
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_tail();
            })
            .unwrap();
        assert!(
            crate::design::face_resolve::extrude_omits_zero_side_one_offset(
                &scope_375,
                &to_face_prologue,
                0
            )
        );
        assert!(
            !crate::design::face_resolve::extrude_omits_zero_side_one_offset(
                &scope_375,
                &to_face_prologue,
                1
            )
        );
    }
    scope_375
        .try_edit(|draft| {
            draft.frame_length = 478;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(
        !crate::design::face_resolve::extrude_omits_zero_side_one_offset(
            &scope_375,
            &to_face_prologue,
            0
        )
    );

    super::fixed_kind_operations::continue_fixed_kind_operations(bytes, scope, &thicken_group);
}
