// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::operands::{
    face_source_carrier_spec, face_source_record_is_registered, parse_face_source_carrier_prefix,
    FaceSourceType,
};
use crate::test_support::write_marked_reference;

fn indexed_header(bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32) {
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(class_tag);
    bytes.extend_from_slice(&record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
}

fn source_carrier(
    class_tag: &[u8; 3],
    record_index: u32,
    scope_record_index: u32,
    source_count: usize,
    scalar_offset: usize,
    discriminator: u32,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, class_tag, record_index);
    bytes.resize(scalar_offset + 16, 0);
    write_marked_reference(&mut bytes, 21, scope_record_index);
    bytes[32..36].copy_from_slice(
        &(u32::try_from(source_count).expect("fixture value fits u32")).to_le_bytes(),
    );
    for ordinal in 0..source_count {
        let offset = 36 + ordinal * 11;
        write_marked_reference(
            &mut bytes,
            offset,
            200 + u32::try_from(ordinal).expect("fixture value fits u32"),
        );
    }
    bytes[scalar_offset..scalar_offset + 4].copy_from_slice(&discriminator.to_le_bytes());
    bytes[scalar_offset + 4..scalar_offset + 12].copy_from_slice(&0.125f64.to_le_bytes());
    bytes[scalar_offset + 12..scalar_offset + 16].copy_from_slice(&discriminator.to_le_bytes());
    bytes
}

fn parse(bytes: &[u8]) -> Option<Vec<(usize, u32)>> {
    crate::test_support::with_decode_context(|ctx| {
        parse_face_source_carrier_prefix(ctx, bytes, 0, bytes.len(), 12).unwrap()
    })
}

#[test]
fn face_source_carriers_follow_counts_and_repeat_the_opaque_discriminator() {
    for (class_tag, source_count, scalar_offset, discriminator) in [
        (*b"398", 4, 80, 100),
        (*b"394", 2, 58, 109),
        (*b"356", 2, 58, 109),
        (*b"414", 1, 47, 110),
        (*b"777", 3, 69, 252),
    ] {
        let layout = face_source_carrier_spec(source_count).unwrap();
        assert_eq!(layout.source_count, source_count);
        assert_eq!(layout.scalar_offset, scalar_offset);
        let bytes = source_carrier(
            &class_tag,
            100,
            12,
            source_count,
            scalar_offset,
            discriminator,
        );
        let references = parse(&bytes).unwrap();
        assert_eq!(
            references,
            (0..source_count)
                .map(|ordinal| (
                    36 + ordinal * 11,
                    200 + u32::try_from(ordinal).expect("fixture value fits u32"),
                ))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn face_source_carrier_prefix_rejects_unbounded_counts_and_inconsistent_lanes() {
    let bytes = source_carrier(b"398", 100, 12, 4, 80, 100);
    for count in [0u32, 3, u32::MAX] {
        let mut invalid = bytes.clone();
        invalid[32..36].copy_from_slice(&count.to_le_bytes());
        assert!(parse(&invalid).is_none());
    }
    let mut invalid = bytes.clone();
    invalid[80..84].copy_from_slice(&101u32.to_le_bytes());
    assert!(parse(&invalid).is_none());
    let mut invalid = bytes.clone();
    invalid[84..92].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(parse(&invalid).is_none());
    for offset in [11, 26, 41] {
        let mut invalid = bytes.clone();
        invalid[offset] = 1;
        assert!(parse(&invalid).is_none());
    }
    let mut invalid = bytes;
    invalid[37..41].fill(0);
    assert!(parse(&invalid).is_none());
}

#[test]
fn face_source_identity_uses_registration_and_segment_instead_of_class_literals() {
    use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};
    use crate::records::identity::ReferenceRun;
    let mut types = vec![SegmentType::try_new(
        "f3d:test/Design1/MetaStream.dat:design-type#10".into(),
        SegmentTypeData {
            byte_offset: 10,
            type_guid: "CAA55628-037B-4D01-9C74-26118F70B63E"
                .to_owned()
                .try_into()
                .unwrap(),
            type_guid_offset: 14,
            base_type_guid: BaseTypeGuid::Absent,
            version: 0,
            version_offset: 50,
            module: "Fusion".into(),
            entities: ReferenceRun::unlocated(vec![100]),
        },
    )
    .unwrap()];
    let source = "f3d:test/Design1/BulkStream.dat:design-parameter-scope#0";
    let matches = |types: &[SegmentType], source, class| {
        face_source_record_is_registered(types, source, 100, class, FaceSourceType::Carrier)
    };
    assert!(matches(&types, source, "256"));
    assert!(!matches(&types, source, "398"));
    assert!(!matches(
        &types,
        "f3d:other/Design1/BulkStream.dat:scope#0",
        "256"
    ));
    types[0].set_type_guid(
        "F0130424-8B7E-4092-93C9-1CA807482534"
            .to_owned()
            .try_into()
            .unwrap(),
    );
    assert!(!matches(&types, source, "256"));
}
