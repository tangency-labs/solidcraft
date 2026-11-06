// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::scopes::component_constructions::exact_derived_instance_construction;
use crate::design::test_support::identity_matrix;
use crate::layout::{
    derived_instance_relation_310_57 as relation_310, derived_instance_scope_279_261 as scope_279,
};
use crate::records::feature::{
    assembly_features::DesignComponentOccurrence, scope::DesignParameterScope,
};

const COMPONENT: &str = "3ad5b67c-2bc5-4ccd-bac9-26ac75616116";
const OCCURRENCE: &str = "f867facf-edec-4109-9553-b3703c4e0caf";

#[test]
fn derived_instance_requires_exact_relation_carrier_and_transform_join() {
    let (mut bytes, mut scope, occurrence) = fixture();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);

    let construction = exact_derived_instance_construction(
        &bytes,
        &records,
        &scope,
        std::slice::from_ref(&occurrence),
    )
    .unwrap()
    .expect("exact DerivedInstance construction");
    assert_eq!(construction.reference_record_index, 305);
    assert_eq!(construction.relation_record_index, 383);
    assert_eq!(construction.carrier_record_index, 382);
    assert_eq!(construction.component_guid.as_str(), COMPONENT);
    assert_eq!(construction.occurrence_guid.as_str(), OCCURRENCE);
    assert_eq!(
        construction.transform,
        occurrence.transform().as_ref().copied().unwrap().value
    );
    assert_eq!(
        construction.transform_offset,
        425 + u64_from_index(scope_279::TRANSFORM)
    );

    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("262".to_owned()).unwrap();
    assert!(exact_derived_instance_construction(
        &bytes,
        &records,
        &scope,
        std::slice::from_ref(&occurrence)
    )
    .unwrap()
    .is_none());

    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("261".to_owned()).unwrap();
    bytes[425 + scope_279::TRANSFORM + 6] = 0;
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    assert!(exact_derived_instance_construction(
        &bytes,
        &records,
        &scope,
        std::slice::from_ref(&occurrence)
    )
    .unwrap()
    .is_none());
}

fn fixture() -> (Vec<u8>, DesignParameterScope, DesignComponentOccurrence) {
    const RELATION_AT: usize = 357;
    const RELATION_PAIRED_AT: usize = RELATION_AT + relation_310::LEN;
    const SCOPE_AT: usize = RELATION_PAIRED_AT + 11;
    const SCOPE_PAIRED_AT: usize = SCOPE_AT + scope_279::LEN;

    let mut bytes = vec![0; SCOPE_PAIRED_AT + 11];
    header(&mut bytes, 0, b"380", 382);
    header(&mut bytes, RELATION_AT, b"310", 383);
    header(&mut bytes, RELATION_PAIRED_AT, b"261", 384);
    header(&mut bytes, SCOPE_AT, b"279", 385);
    header(&mut bytes, SCOPE_PAIRED_AT, b"261", 385);

    bytes[RELATION_AT + relation_310::CARRIER_MARKER] = 1;
    bytes[RELATION_AT + relation_310::CARRIER_RECORD_INDEX
        ..RELATION_AT + relation_310::CARRIER_RECORD_INDEX + 4]
        .copy_from_slice(&382u32.to_le_bytes());
    bytes[RELATION_AT + relation_310::MIDDLE_MARKER] = 1;
    bytes[RELATION_AT + relation_310::MIDDLE_RECORD_INDEX
        ..RELATION_AT + relation_310::MIDDLE_RECORD_INDEX + 4]
        .copy_from_slice(&384u32.to_le_bytes());
    bytes[RELATION_AT + relation_310::SCOPE_MARKER] = 1;
    bytes[RELATION_AT + relation_310::SCOPE_RECORD_INDEX
        ..RELATION_AT + relation_310::SCOPE_RECORD_INDEX + 4]
        .copy_from_slice(&385u32.to_le_bytes());

    bytes[SCOPE_AT + scope_279::REFERENCE_MARKER] = 1;
    bytes[SCOPE_AT + scope_279::REFERENCE_RECORD_INDEX
        ..SCOPE_AT + scope_279::REFERENCE_RECORD_INDEX + 4]
        .copy_from_slice(&305u32.to_le_bytes());
    bytes[SCOPE_AT + scope_279::REFERENCE_COUNT..SCOPE_AT + scope_279::REFERENCE_COUNT + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    bytes[SCOPE_AT + scope_279::RELATION_REFERENCE] = 1;
    bytes[SCOPE_AT + scope_279::RELATION_REFERENCE + 1
        ..SCOPE_AT + scope_279::RELATION_REFERENCE + 5]
        .copy_from_slice(&383u32.to_le_bytes());
    let transform = identity_matrix();
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = SCOPE_AT + scope_279::TRANSFORM + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#425",
        crate::records::feature::scope::DesignFeatureKind::DerivedInstance,
        385,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(SCOPE_AT);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("279".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = u64_from_index(scope_279::LEN);
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![383]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("261".to_owned()).unwrap();

    let occurrence = DesignComponentOccurrence::try_new(
        crate::records::feature::assembly_features::DesignComponentOccurrenceDraft {
            id: "f3d:Design/BulkStream.dat:design-component-occurrence#0".into(),
            class_tag: crate::records::references::DesignClassTag::try_from("380".to_owned())
                .unwrap(),
            record_index: 382,
            byte_offset: 0,
            component_record_index: 305,
            component_guid: COMPONENT.to_owned().try_into().expect("GUID"),
            occurrence_guid: OCCURRENCE.to_owned().try_into().expect("GUID"),
            placement: crate::records::feature::assembly_features::DesignComponentOccurrencePlacement::Explicit {
                ordinal: std::num::NonZeroU32::MIN,
                transform: transform.try_into().unwrap(),
            },
            owner_entity: 0,
        },
    )
    .unwrap();
    (bytes, scope, occurrence)
}

fn header(bytes: &mut [u8], at: usize, class_tag: &[u8; 3], record_index: u32) {
    bytes[at..at + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[at + 4..at + 7].copy_from_slice(class_tag);
    bytes[at + 7..at + 11].copy_from_slice(&record_index.to_le_bytes());
}
