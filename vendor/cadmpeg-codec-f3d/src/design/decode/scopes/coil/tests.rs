// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use cadmpeg_test_support::bytes::{put_u32, put_u64};

use super::exact_coil_placement;
use crate::design::decode::scopes::parameter_scope::parse_parameter_scope;
use crate::layout::coil_compact_persistent_selection_prefix as coil_persist_selection;
use crate::layout::coil_legacy_placement_identity_frame as coil_legacy_identity;
use crate::layout::coil_modern_placement_matrix_frame as coil_modern_matrix;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::coil::DesignCoilExtent;
use crate::records::feature::coil::DesignCoilSection;
use crate::records::feature::coil::DesignCoilSectionPlacement;
use crate::records::feature::coil::DesignCoilSelection;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::recipes::ConstructionRecipe;
use crate::records::recipes::ConstructionRecipeKind;
use crate::test_support::indexed_header;
use crate::test_support::lp_utf16;

fn marked(bytes: &mut [u8], offset: usize, record_index: u32) {
    bytes[offset] = 1;
    bytes[offset + 1..offset + 5].copy_from_slice(&record_index.to_le_bytes());
    bytes[offset + 5..offset + 11].fill(0);
}

fn compact_coil_placement_fixture(
    matrix: Option<[[f64; 4]; 4]>,
) -> (Vec<u8>, DesignParameterScope, usize) {
    let mut bytes = Vec::new();
    let selection_record_index = 100;
    let transform_record_index = 200;
    indexed_header(&mut bytes, *b"333", selection_record_index);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&(selection_record_index + 3).to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "11111111-1111-4111-8111-111111111111");
    lp_utf16(&mut bytes, "22222222-2222-4222-8222-222222222222");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    indexed_header(&mut bytes, *b"265", selection_record_index);
    indexed_header(&mut bytes, *b"301", selection_record_index + 1);
    indexed_header(&mut bytes, *b"446", selection_record_index + 2);
    indexed_header(&mut bytes, *b"429", selection_record_index + 3);
    bytes.extend_from_slice(&[0; 18]);
    bytes.extend_from_slice(&1331u64.to_le_bytes());
    bytes.extend_from_slice(&183u64.to_le_bytes());
    indexed_header(&mut bytes, *b"311", selection_record_index + 4);

    let transform_start = bytes.len();
    indexed_header(&mut bytes, *b"270", transform_record_index);
    let frame_length = if matrix.is_some() { 341 } else { 213 };
    bytes.resize(transform_start + frame_length, 0);
    bytes[transform_start + 55] = 1;
    if let Some(matrix) = matrix {
        bytes[transform_start + 65] = 0;
        for (ordinal, value) in matrix.into_iter().flatten().enumerate() {
            let offset = transform_start + 66 + ordinal * 8;
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
    } else {
        bytes[transform_start + 65] = 1;
    }
    indexed_header(&mut bytes, *b"259", transform_record_index);

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#42",
        crate::records::feature::scope::DesignFeatureKind::CoilPrimitive,
        42,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 442;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                selection_record_index,
                transform_record_index,
                300,
                301,
                302,
                303,
                304,
                305,
            ]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope, transform_start)
}

fn modern_coil_matrix_placement_fixture() -> (Vec<u8>, DesignParameterScope, usize) {
    let (mut bytes, mut scope, transform_start) = compact_coil_placement_fixture(None);
    bytes.insert(coil_persist_selection::NESTED_SELECTION_MARKER, 0);
    let transform_start = transform_start + 1;
    bytes[4..7].copy_from_slice(b"286");
    bytes.truncate(transform_start);
    indexed_header(&mut bytes, *b"450", 200);
    bytes.resize(transform_start + coil_modern_matrix::LEN, 0);
    let transform: [[f64; 4]; 4] = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, -1.0, 0.0, 0.0],
        [0.0, 0.0, -1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let offset = transform_start + coil_modern_matrix::MATRIX + ordinal * 8;
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes[transform_start + coil_modern_matrix::CONSTANT_512
        ..transform_start + coil_modern_matrix::CONSTANT_512 + 4]
        .copy_from_slice(&512u32.to_le_bytes());
    bytes[transform_start + coil_modern_matrix::CONSTANT_256
        ..transform_start + coil_modern_matrix::CONSTANT_256 + 4]
        .copy_from_slice(&256u32.to_le_bytes());
    marked(
        &mut bytes,
        transform_start + coil_modern_matrix::SELECTION_REFERENCE,
        100,
    );
    bytes[transform_start + coil_modern_matrix::SELECTION_FLAG
        ..transform_start + coil_modern_matrix::SELECTION_FLAG + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    marked(
        &mut bytes,
        transform_start + coil_modern_matrix::AUXILIARY_REFERENCE,
        225,
    );
    put_u64(
        &mut bytes,
        transform_start + coil_modern_matrix::CONSTANT_1024,
        1024,
    );
    put_u64(
        &mut bytes,
        transform_start + coil_modern_matrix::IDENTITY_LANE_PREFIX,
        0x7000_0000_0000_0000,
    );
    put_u64(
        &mut bytes,
        transform_start + coil_modern_matrix::IDENTITY_LANE,
        0x703e_0000_0000_0001,
    );
    marked(
        &mut bytes,
        transform_start + coil_modern_matrix::SUCCESSOR_REFERENCE,
        202,
    );
    marked(
        &mut bytes,
        transform_start + coil_modern_matrix::PREDECESSOR_REFERENCE,
        201,
    );
    marked(
        &mut bytes,
        transform_start + coil_modern_matrix::OWNER_REFERENCE,
        scope.record_index,
    );
    indexed_header(&mut bytes, *b"259", 200);
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("353".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("259".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 427;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope, transform_start)
}

fn compact_coil_owner_identity_fixture() -> (Vec<u8>, DesignParameterScope, usize) {
    let (mut bytes, scope, transform_start) = compact_coil_placement_fixture(None);
    let paired_start = transform_start + 213;
    let paired = bytes.split_off(paired_start);
    bytes.extend_from_slice(&[0; 9]);
    bytes.push(1);
    bytes.extend_from_slice(&scope.record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&paired);
    (bytes, scope, transform_start)
}

fn legacy_coil_placement_identity_fixture() -> (Vec<u8>, DesignParameterScope, usize) {
    let (mut bytes, mut scope, transform_start) = compact_coil_placement_fixture(None);
    bytes.truncate(transform_start);
    indexed_header(&mut bytes, *b"395", 200);
    bytes.resize(transform_start + coil_legacy_identity::LEN, 0);
    marked(
        &mut bytes,
        transform_start + coil_legacy_identity::LEADING_REFERENCE_MARKER,
        0,
    );
    put_u32(
        &mut bytes,
        transform_start + coil_legacy_identity::PROLOGUE_VALUE,
        2,
    );
    put_u32(
        &mut bytes,
        transform_start + coil_legacy_identity::PROLOGUE_FLAG,
        1,
    );
    marked(
        &mut bytes,
        transform_start + coil_legacy_identity::SELECTION_REFERENCE_MARKER,
        100,
    );
    put_u32(
        &mut bytes,
        transform_start + coil_legacy_identity::SELECTION_FLAG,
        1,
    );
    marked(
        &mut bytes,
        transform_start + coil_legacy_identity::AUXILIARY_REFERENCE_MARKER,
        350,
    );
    put_u32(
        &mut bytes,
        transform_start + coil_legacy_identity::TAIL_VALUE,
        4,
    );
    put_u32(
        &mut bytes,
        transform_start + coil_legacy_identity::INTERMEDIATE_SELECTOR,
        109,
    );
    bytes[transform_start + coil_legacy_identity::CARRIER_SCALAR
        ..transform_start + coil_legacy_identity::CARRIER_SCALAR + 8]
        .copy_from_slice(&6.64e-5f64.to_le_bytes());
    put_u32(
        &mut bytes,
        transform_start + coil_legacy_identity::TAIL_SELECTOR,
        109,
    );
    marked(
        &mut bytes,
        transform_start + coil_legacy_identity::SUCCESSOR_REFERENCE_MARKER,
        202,
    );
    marked(
        &mut bytes,
        transform_start + coil_legacy_identity::PREDECESSOR_REFERENCE_MARKER,
        201,
    );
    marked(
        &mut bytes,
        transform_start + coil_legacy_identity::OWNER_REFERENCE_MARKER,
        scope.record_index,
    );
    indexed_header(&mut bytes, *b"258", 200);
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("393".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 427;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope, transform_start)
}

fn compact_coil_spiral_placement_fixture() -> (Vec<u8>, DesignParameterScope, usize) {
    let (bytes, mut scope, transform_start) = compact_coil_placement_fixture(None);
    scope
        .try_edit(|draft| {
            draft.frame_length = 411;
            draft.reference_members = {
                let mut values: Vec<u32> = draft.reference_members.values().copied().collect();
                values.pop();
                crate::records::identity::ReferenceRun::unlocated(values)
            };
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::SpirePrimitive(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::CoilPrimitive(slot) =
        scope.payload_mut()
    {
        slot.get_or_insert_with(Default::default).extent = Some(
            crate::records::identity::MaybeRecordedValue::Unlocated(DesignCoilExtent::Spiral),
        );
    }
    (bytes, scope, transform_start)
}

fn compact_coil_face_selection_fixture() -> (Vec<u8>, DesignParameterScope, Vec<ConstructionRecipe>)
{
    let mut bytes = Vec::new();
    let selection_record_index = 100;
    let transform_record_index = 200;
    indexed_header(&mut bytes, *b"333", selection_record_index);
    bytes.extend_from_slice(&[0; 12]);
    bytes.push(1);
    bytes.extend_from_slice(&(selection_record_index + 3).to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.push(1);
    bytes.extend_from_slice(&[0; 3]);
    lp_utf16(&mut bytes, "11111111-1111-4111-8111-111111111111");
    lp_utf16(&mut bytes, "22222222-2222-4222-8222-222222222222");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    indexed_header(&mut bytes, *b"265", selection_record_index);
    indexed_header(&mut bytes, *b"301", selection_record_index + 1);
    indexed_header(&mut bytes, *b"446", selection_record_index + 2);
    indexed_header(&mut bytes, *b"429", selection_record_index + 3);
    bytes.extend_from_slice(&16u32.to_le_bytes());
    let recipe_byte_offset = bytes.len();
    bytes.extend_from_slice(b"face_recipe_data");
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&(-1i32).to_le_bytes());
    let transform_start = bytes.len();

    indexed_header(&mut bytes, *b"270", transform_record_index);
    bytes.resize(transform_start + 341, 0);
    bytes[transform_start + 55] = 1;
    bytes[transform_start + 65] = 0;
    let transform: [[f64; 4]; 4] = [
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, -1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.7],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let offset = transform_start + 66 + ordinal * 8;
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    indexed_header(&mut bytes, *b"259", transform_record_index);

    let stream = "f3d:Design/BulkStream.dat";
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#42",
        crate::records::feature::scope::DesignFeatureKind::CoilPrimitive,
        42,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 432;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                selection_record_index,
                transform_record_index,
                300,
                301,
                302,
                303,
                304,
                305,
            ]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let recipes = vec![ConstructionRecipe {
        id: format!("{stream}:construction-recipe#{recipe_byte_offset}"),
        byte_offset: u64_from_index(recipe_byte_offset),
        kind: ConstructionRecipeKind::Face,
        design: Some(crate::records::recipes::ConstructionRecipeDesign {
            id: crate::records::identity::RecordedValue {
                value: "body".into(),
                offset: 0,
            },
            selector: None,
        }),
        recipe_index: 0,
        record_index: Some(crate::records::identity::RecordedValue {
            value: 103,
            offset: 0,
        }),
    }];
    (bytes, scope, recipes)
}

#[test]
fn compact_coil_placement_accepts_identity_and_matrix_frames() {
    let explicit = [
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, -1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.7],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for (ordinal, (matrix, expected_offset)) in [(None, None), (Some(explicit), Some(66))]
        .into_iter()
        .enumerate()
    {
        let (bytes, mut scope, transform_start) = compact_coil_placement_fixture(matrix);
        scope
            .try_edit(|draft| {
                draft.frame_length = if ordinal == 0 { 432 } else { 442 };
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_tail();
            })
            .unwrap();
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let placement = exact_coil_placement(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &records,
            &scope,
            &[],
        )
        .unwrap()
        .expect("compact Coil placement");
        assert_eq!(placement.selection_record_index, 100);
        assert_eq!(placement.transform_record_index, 200);
        assert_eq!(
            placement.selection,
            DesignCoilSelection::Persistent {
                asset_id: "11111111-1111-4111-8111-111111111111"
                    .to_owned()
                    .try_into()
                    .expect("GUID"),
                context_id: "22222222-2222-4222-8222-222222222222"
                    .to_owned()
                    .try_into()
                    .expect("GUID"),
                identity_record_index: 103,
                primary_identity: 1331,
                secondary: Some(crate::records::identity::DesignSecondaryIdentity {
                    identity: 183,
                    curve_identity: None
                }),
            }
        );
        assert_eq!(
            placement.explicit_transform.map(|matrix| matrix.offset),
            expected_offset.map(|offset| u64_from_index(transform_start + offset))
        );
        assert_eq!(
            *placement.transform(),
            matrix
                .unwrap_or([
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ])
                .try_into()
                .unwrap()
        );
    }
}

#[test]
fn modern_coil_placement_accepts_class_450_matrix_frame() {
    let (bytes, scope, transform_start) = modern_coil_matrix_placement_fixture();
    let placement = exact_coil_placement(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &[],
    )
    .unwrap()
    .expect("modern Coil matrix placement");
    assert_eq!(placement.selection_record_index, 100);
    assert_eq!(placement.selection_class_tag.as_str(), "286");
    assert_eq!(placement.transform_record_index, 200);
    assert_eq!(placement.transform_class_tag.as_str(), "450");
    assert_eq!(
        *placement.transform(),
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, -1.0, 0.0, 0.0],
            [0.0, 0.0, -1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
        .try_into()
        .unwrap()
    );
    assert_eq!(
        placement.explicit_transform.map(|matrix| matrix.offset),
        Some(u64_from_index(transform_start + coil_modern_matrix::MATRIX))
    );
}

#[test]
fn modern_coil_placement_requires_exact_class_450_matrix_carrier() {
    let (mut bytes, scope, transform_start) = modern_coil_matrix_placement_fixture();
    bytes[transform_start + coil_modern_matrix::CONSTANT_512 + 1] = 0;
    assert_eq!(
        exact_coil_placement(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &[]
        )
        .unwrap(),
        None
    );

    let (mut bytes, scope, transform_start) = modern_coil_matrix_placement_fixture();
    bytes[transform_start + coil_modern_matrix::IDENTITY_LANE + 7] = 0;
    assert_eq!(
        exact_coil_placement(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &[]
        )
        .unwrap(),
        None
    );

    let (mut bytes, scope, transform_start) = modern_coil_matrix_placement_fixture();
    bytes[transform_start + coil_modern_matrix::OWNER_REFERENCE + 1] = 0;
    assert_eq!(
        exact_coil_placement(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &[]
        )
        .unwrap(),
        None
    );
}

#[test]
fn compact_coil_placement_accepts_owner_referenced_identity_frame() {
    let (bytes, scope, transform_start) = compact_coil_owner_identity_fixture();
    let placement = exact_coil_placement(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &[],
    )
    .unwrap()
    .expect("owner-referenced compact Coil placement");
    assert_eq!(
        *placement.transform(),
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
        .try_into()
        .unwrap()
    );
    assert_eq!(
        placement.explicit_transform.map(|matrix| matrix.offset),
        None
    );
    assert_eq!(
        placement.transform_record_byte_offset,
        u64_from_index(transform_start)
    );
}

#[test]
fn legacy_coil_placement_accepts_identity_frame() {
    let (bytes, scope, transform_start) = legacy_coil_placement_identity_fixture();
    let placement = exact_coil_placement(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &[],
    )
    .unwrap()
    .expect("legacy Coil placement");
    assert_eq!(placement.selection_record_index, 100);
    assert_eq!(placement.transform_record_index, 200);
    assert_eq!(
        placement.explicit_transform.map(|matrix| matrix.offset),
        None
    );
    assert_eq!(
        placement.transform_record_byte_offset,
        u64_from_index(transform_start)
    );
    assert_eq!(
        *placement.transform(),
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
        .try_into()
        .unwrap()
    );
}

#[test]
fn legacy_coil_placement_requires_exact_identity_carrier() {
    let (mut bytes, scope, transform_start) = legacy_coil_placement_identity_fixture();
    bytes[transform_start + coil_legacy_identity::TAIL_VALUE] = 5;
    assert_eq!(
        exact_coil_placement(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &[]
        )
        .unwrap(),
        None
    );

    let (bytes, mut scope, _) = legacy_coil_placement_identity_fixture();
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("432".to_owned()).unwrap();
    assert_eq!(
        exact_coil_placement(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &[]
        )
        .unwrap(),
        None
    );

    let (mut bytes, scope, transform_start) = legacy_coil_placement_identity_fixture();
    bytes[transform_start + coil_legacy_identity::SUCCESSOR_RECORD_INDEX] = 203;
    assert_eq!(
        exact_coil_placement(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &[]
        )
        .unwrap(),
        None
    );
}

#[test]
fn compact_coil_spiral_placement_accepts_seven_reference_form() {
    let (bytes, scope, transform_start) = compact_coil_spiral_placement_fixture();
    let placement = exact_coil_placement(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &[],
    )
    .unwrap()
    .expect("seven-reference compact Coil spiral placement");
    assert_eq!(placement.selection_record_index, 100);
    assert_eq!(placement.transform_record_index, 200);
    assert_eq!(
        placement.transform_record_byte_offset,
        u64_from_index(transform_start)
    );
}

#[test]
fn compact_coil_seven_reference_form_requires_spiral_extent() {
    let (bytes, mut scope, _) = compact_coil_spiral_placement_fixture();
    if let crate::records::feature::scope::DesignScopePayloadMut::SpirePrimitive(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::CoilPrimitive(slot) =
        scope.payload_mut()
    {
        slot.get_or_insert_with(Default::default).extent =
            Some(crate::records::identity::MaybeRecordedValue::Unlocated(
                DesignCoilExtent::RevolutionsHeight,
            ));
    }
    assert_eq!(
        exact_coil_placement(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &[]
        )
        .unwrap(),
        None
    );
}

#[test]
fn compact_coil_placement_rejects_ambiguous_or_reflected_frames() {
    let (mut bytes, scope, transform_start) = compact_coil_placement_fixture(Some([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.7],
        [0.0, 0.0, 0.0, 1.0],
    ]));
    let matrix_value_offset = transform_start + 66 + 10 * 8;
    bytes[matrix_value_offset..matrix_value_offset + 8].copy_from_slice(&(-1.0f64).to_le_bytes());
    assert_eq!(
        exact_coil_placement(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &[]
        )
        .unwrap(),
        None
    );

    let (mut bytes, scope, transform_start) = compact_coil_owner_identity_fixture();
    bytes[transform_start + 65] = 0;
    assert_eq!(
        exact_coil_placement(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &[]
        )
        .unwrap(),
        None
    );

    let (mut bytes, scope, transform_start) = compact_coil_owner_identity_fixture();
    bytes[transform_start + 223] ^= 1;
    assert_eq!(
        exact_coil_placement(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &[]
        )
        .unwrap(),
        None
    );
}

#[test]
fn compact_coil_placement_accepts_face_recipe_selection() {
    let (bytes, scope, recipes) = compact_coil_face_selection_fixture();
    let placement = exact_coil_placement(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &recipes,
    )
    .unwrap()
    .expect("compact Coil face placement");
    assert_eq!(
        placement.selection,
        DesignCoilSelection::FaceRecipe {
            asset_id: "11111111-1111-4111-8111-111111111111"
                .to_owned()
                .try_into()
                .expect("GUID"),
            context_id: "22222222-2222-4222-8222-222222222222"
                .to_owned()
                .try_into()
                .expect("GUID"),
            recipe_record_index: 103,
            recipe_record_byte_offset: recipes[0].byte_offset - 15,
            recipe_id: recipes[0].id.clone(),
            recipe_kind: crate::records::feature::scope::DesignFaceRecipeKind::Face,
            design: Some(crate::records::recipes::ConstructionRecipeDesign {
                id: "body".into(),
                selector: None
            }),
        }
    );
}

#[test]
fn coil_scope_discriminators_use_the_fixed_scope_prologue() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"301");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.resize(120, 0);
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    bytes[24] = 1;
    bytes[26..30].copy_from_slice(&2u32.to_le_bytes());
    bytes[30..34].copy_from_slice(&3u32.to_le_bytes());
    bytes[92..96].copy_from_slice(&2u32.to_le_bytes());
    bytes[107..111].copy_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "SpirePrimitive");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned()).unwrap(),
        byte_offset: 0,
    };

    let scope = parse_parameter_scope(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("Coil scope");
    assert_eq!(scope.coil_operation(), Some(DesignExtrudeOperation::Cut));
    assert_eq!(scope.coil_operation_offset(), Some(20));
    assert_eq!(scope.coil_extent(), Some(DesignCoilExtent::HeightPitch));
    assert_eq!(scope.coil_extent_offset(), Some(30));
    assert_eq!(
        scope.coil_section(),
        Some(DesignCoilSection::ExternalTriangle)
    );
    assert_eq!(scope.coil_section_offset(), Some(92));
    assert_eq!(
        scope.coil_section_placement(),
        Some(DesignCoilSectionPlacement::Inside)
    );
    assert_eq!(scope.coil_section_placement_offset(), Some(107));
    assert_eq!(scope.coil_clockwise(), Some(true));
    assert_eq!(scope.coil_clockwise_offset(), Some(24));

    bytes[24] = 0;
    let scope = parse_parameter_scope(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("counterclockwise Spire scope");
    assert_eq!(scope.coil_clockwise(), Some(false));
    assert_eq!(scope.coil_clockwise_offset(), Some(24));
}

#[test]
fn compact_coil_scope_uses_its_own_closed_discriminators() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"353");
    bytes.extend_from_slice(&6644u32.to_le_bytes());
    bytes.resize(120, 0);
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[25] = 1;
    bytes[26..30].copy_from_slice(&4u32.to_le_bytes());
    bytes[30..34].copy_from_slice(&1u32.to_le_bytes());
    bytes[92..96].copy_from_slice(&1u32.to_le_bytes());
    bytes[107..111].copy_from_slice(&1u32.to_le_bytes());
    let references: [u32; 8] = [6645, 6650, 6653, 6656, 6659, 6662, 6665, 6668];
    bytes.extend_from_slice(
        &(u32::try_from(references.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for reference in references {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&310u32.to_le_bytes());
    lp_utf16(&mut bytes, "CoilPrimitive");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&309u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&6644u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 6644,
        class_tag: crate::records::references::DesignClassTag::try_from("353".to_owned()).unwrap(),
        byte_offset: 0,
    };

    let scope = parse_parameter_scope(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("compact Coil scope");
    assert_eq!(
        scope.coil_operation(),
        Some(DesignExtrudeOperation::NewBody)
    );
    assert_eq!(
        scope.coil_extent(),
        Some(DesignCoilExtent::RevolutionsHeight)
    );
    assert_eq!(scope.coil_section(), Some(DesignCoilSection::Circular));
    assert_eq!(
        scope.coil_section_placement(),
        Some(DesignCoilSectionPlacement::Inside)
    );
    assert_eq!(scope.coil_clockwise(), Some(false));
    assert_eq!(scope.coil_clockwise_offset(), Some(25));

    // A right-hand coil travels clockwise viewed along its positive axis.
    let mut right_hand = bytes.clone();
    right_hand[25] = 0;
    let scope = parse_parameter_scope(
        &right_hand,
        &crate::design::test_support::indexed_record_offsets_for_test(&right_hand),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("right-hand compact Coil scope");
    assert_eq!(scope.coil_clockwise(), Some(true));
    assert_eq!(scope.coil_clockwise_offset(), Some(25));

    let mut invalid_hand = bytes.clone();
    invalid_hand[25] = 2;
    let scope = parse_parameter_scope(
        &invalid_hand,
        &crate::design::test_support::indexed_record_offsets_for_test(&invalid_hand),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("invalid handedness retains the native Coil scope");
    assert_eq!(scope.coil_clockwise(), None);
    assert_eq!(scope.coil_clockwise_offset(), None);

    for (placement_code, placement) in [
        (1u32, DesignCoilSectionPlacement::Inside),
        (2u32, DesignCoilSectionPlacement::Center),
        (3u32, DesignCoilSectionPlacement::Outside),
    ] {
        for (section_code, section) in [
            (1u32, DesignCoilSection::Circular),
            (2u32, DesignCoilSection::Square),
            (3u32, DesignCoilSection::ExternalTriangle),
            (4u32, DesignCoilSection::InternalTriangle),
        ] {
            bytes[92..96].copy_from_slice(&placement_code.to_le_bytes());
            bytes[107..111].copy_from_slice(&section_code.to_le_bytes());
            let parsed = parse_parameter_scope(
                &bytes,
                &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
                header.record_index,
                &header.class_tag,
                header.byte_offset,
            )
            .unwrap()
            .expect("compact Coil scope");
            assert_eq!(parsed.coil_section(), Some(section));
            assert_eq!(parsed.coil_section_placement(), Some(placement));
        }
    }

    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    let unsupported = parse_parameter_scope(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("unsupported Coil operation remains a native scope");
    assert!(unsupported.coil_operation().is_none());
}

#[test]
fn compact_coil_new_body_scope_accepts_unlinked_state_trailer() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"338");
    bytes.extend_from_slice(&6644u32.to_le_bytes());
    bytes.resize(228, 0);
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[24] = 0;
    bytes[26..30].copy_from_slice(&4u32.to_le_bytes());
    bytes[30..34].copy_from_slice(&1u32.to_le_bytes());
    bytes[92..96].copy_from_slice(&1u32.to_le_bytes());
    bytes[107..111].copy_from_slice(&1u32.to_le_bytes());
    let references: [u32; 8] = [6645, 6650, 6653, 6656, 6659, 6662, 6665, 6668];
    bytes.extend_from_slice(
        &(u32::try_from(references.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for reference in references {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    lp_utf16(&mut bytes, "CoilPrimitive");
    let mut tail = [0; 88];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&6644u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 6644,
        class_tag: crate::records::references::DesignClassTag::try_from("338".to_owned()).unwrap(),
        byte_offset: 0,
    };

    let scope = parse_parameter_scope(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("compact Coil new-body scope");
    assert_eq!(scope.frame_length(), 442);
    assert_eq!(
        scope.kind(),
        crate::records::feature::scope::DesignFeatureKind::CoilPrimitive
    );
    assert_eq!(
        scope.coil_operation(),
        Some(DesignExtrudeOperation::NewBody)
    );
    assert_eq!(scope.history_state_id(), Some(3));
    assert_eq!(scope.previous_history_state_id(), None);
    assert_eq!(scope.previous_history_state_id_offset(), None);
}

#[test]
fn long_coil_scope_discriminators_use_the_ten_reference_envelope() {
    let scope = |frame_length: usize, operation: u32| {
        let reference_members: [u32; 10] =
            [1001, 1002, 1003, 1004, 1005, 1006, 1007, 1008, 1009, 1010];
        let kind = "CoilPrimitive";
        let kind_length = 4 + kind.encode_utf16().count() * 2;
        let tail_length = if frame_length == 572 { 76 } else { 78 };
        let kind_at = frame_length - tail_length - kind_length;
        let reference_count_at = kind_at - 4 - 4 - reference_members.len() * 11;
        let mut bytes = vec![0; reference_count_at];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"345");
        bytes[7..11].copy_from_slice(&331u32.to_le_bytes());
        bytes[22..26].copy_from_slice(&operation.to_le_bytes());
        bytes[26..30].copy_from_slice(&1u32.to_le_bytes());
        for (offset, target) in [(30usize, 1005u32), (41, 1009)] {
            bytes[offset] = 1;
            bytes[offset + 1..offset + 5].copy_from_slice(&target.to_le_bytes());
        }
        if matches!(frame_length, 572 | 578) {
            let matrix: [f64; 16] = [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ];
            for (ordinal, value) in matrix.into_iter().enumerate() {
                bytes[77 + ordinal * 8..85 + ordinal * 8].copy_from_slice(&value.to_le_bytes());
            }
        }
        bytes.extend_from_slice(
            &(u32::try_from(reference_members.len()).expect("fixture value fits u32"))
                .to_le_bytes(),
        );
        for reference in reference_members {
            bytes.push(1);
            bytes.extend_from_slice(&reference.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }
        bytes.extend_from_slice(&310u32.to_le_bytes());
        lp_utf16(&mut bytes, kind);
        let mut tail = vec![0; tail_length];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[31..35].copy_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"259");
        bytes.extend_from_slice(&331u32.to_le_bytes());
        assert_eq!(bytes.len(), frame_length + 11);
        let header = DesignRecordHeader {
            id: "generated:scope-header#0".into(),
            record_index: 331,
            class_tag: crate::records::references::DesignClassTag::try_from("345".to_owned())
                .unwrap(),
            byte_offset: 0,
        };
        parse_parameter_scope(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            header.record_index,
            &header.class_tag,
            header.byte_offset,
        )
        .unwrap()
        .expect("long Coil scope")
    };

    let boolean = scope(450, 1);
    assert_eq!(boolean.coil_operation(), Some(DesignExtrudeOperation::Join));
    assert_eq!(boolean.coil_operation_offset(), Some(22));
    assert_eq!(boolean.coil_extent(), None);
    assert_eq!(boolean.coil_section(), Some(DesignCoilSection::Circular));
    assert_eq!(boolean.coil_section_offset(), None);
    assert_eq!(
        boolean.coil_section_placement(),
        Some(DesignCoilSectionPlacement::Inside)
    );
    assert_eq!(boolean.coil_section_placement_offset(), None);
    assert_eq!(boolean.coil_clockwise(), Some(false));
    assert_eq!(boolean.coil_clockwise_offset(), None);

    let new_body = scope(578, 2);
    assert_eq!(
        new_body.coil_operation(),
        Some(DesignExtrudeOperation::NewBody)
    );
    assert_eq!(new_body.coil_operation_offset(), Some(22));
    let transform = new_body.coil_transform().expect("long Coil placement");
    assert_eq!(transform.transform_offset, 77);
    assert_eq!(
        transform.transform,
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
        .try_into()
        .unwrap()
    );

    for (operation, expected) in [
        (1, DesignExtrudeOperation::Join),
        (2, DesignExtrudeOperation::Cut),
        (3, DesignExtrudeOperation::Intersect),
    ] {
        let boolean = scope(572, operation);
        assert_eq!(boolean.coil_operation(), Some(expected));
        let transform = boolean.coil_transform().expect("572-byte Coil placement");
        assert_eq!(transform.transform_offset, 77);
        assert_eq!(
            transform.transform,
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ]
            .try_into()
            .unwrap()
        );
    }
}
