// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::sketch::decode_pattern_definition;
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design::decode::sketch::next_indexed_record_offset_with_index;
use crate::design::decode::sketch::parse_classed_sketch_relation;
use crate::design::decode::sketch::parse_genesis_entity_header;
use crate::design::decode::sketch::parse_settled_entity_header;
use crate::design::decode::sketch::SketchRelationClass;
use crate::design::test_support::push_genesis_block;
use crate::design::test_support::push_reference;
use crate::records::sketch_relations::SketchConstraintKind;

fn tested_parse_classed_sketch_relation(
    payload: &[u8],
    class: SketchRelationClass,
) -> Option<crate::design::decode::sketch::ParsedSketchRelation> {
    parse_classed_sketch_relation(payload, class).unwrap()
}

#[test]
fn variable_width_relation_uses_counted_runs_and_next_record_boundary() {
    // The eleven-byte reference form puts each pair at fifteen bytes: the
    // reference at `marker`, its relation ordinal four bytes later.
    let mut record = vec![0u8; 127];
    record[0..4].copy_from_slice(&3u32.to_le_bytes());
    record[4..7].copy_from_slice(b"286");
    record[7..11].copy_from_slice(&1239u32.to_le_bytes());
    record[19] = 1;
    record[20..24].copy_from_slice(&3u32.to_le_bytes());
    for (marker, reference) in [(24, 1224u32), (39, 1228), (54, 1236)] {
        record[marker] = 1;
        record[marker + 1..marker + 9].copy_from_slice(&u64::from(reference).to_le_bytes());
    }
    record[35..39].copy_from_slice(&3u32.to_le_bytes());
    record[50..54].copy_from_slice(&1u32.to_le_bytes());
    // Offset 69 is the base level's property-block presence byte; the
    // `ParentNode` reference follows it.
    record[70] = 1;
    record[71..79].copy_from_slice(&1041u64.to_le_bytes());
    record[81..89].copy_from_slice(&4u64.to_le_bytes());
    record[89..93].copy_from_slice(&3u32.to_le_bytes());
    for (marker, reference) in [(93, 1224u32), (104, 1228), (115, 1236)] {
        record[marker] = 1;
        record[marker + 1..marker + 9].copy_from_slice(&u64::from(reference).to_le_bytes());
    }
    let mut bytes = record.clone();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"277");
    bytes.extend_from_slice(&1240u32.to_le_bytes());

    assert_eq!(next_indexed_record_offset(&bytes, 11), Some(127));
    let parsed = tested_parse_classed_sketch_relation(&record, SketchRelationClass::Plain).unwrap();
    assert_eq!(
        parsed
            .members
            .iter()
            .map(|row| row.reference.value)
            .collect::<Vec<_>>(),
        [1224, 1228, 1236]
    );
    assert_eq!(
        parsed
            .members
            .iter()
            .map(|row| row.relation_ordinal)
            .collect::<Vec<_>>(),
        [3, 1, 0]
    );
    let empty_members: [u32; 0] = [];
    assert_eq!(
        parsed
            .auxiliary_references
            .iter()
            .map(|row| row.value)
            .collect::<Vec<_>>(),
        empty_members
    );
    assert_eq!(parsed.owner_reference, 1041);
    assert_eq!(parsed.state, 4);
    assert_eq!(parsed.state_offset, 81);
    assert_eq!(parsed.entity_genesis, None);
    assert_eq!(
        parsed
            .return_members
            .iter()
            .map(|row| row.value)
            .collect::<Vec<_>>(),
        [1224, 1228, 1236]
    );
    assert_eq!(parsed.parsed_end, 127);
}

#[test]
fn indexed_record_search_requires_the_expected_identity() {
    let mut bytes = vec![0xaa; 9];
    let decoy = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"278");
    bytes.extend_from_slice(&41u32.to_le_bytes());
    bytes.extend_from_slice(&[0xbb; 7]);
    let expected = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"306");
    bytes.extend_from_slice(&42u32.to_le_bytes());

    assert_eq!(next_indexed_record_offset(&bytes, 0), Some(decoy));
    assert_eq!(
        next_indexed_record_offset_with_index(&bytes, 0, 42),
        Some(expected)
    );
}

#[test]
fn genesis_relation_parses_u64_text_frame_mask_and_relation_ordinals() {
    let mut auxiliary = vec![0];
    push_reference(&mut auxiliary, 2394);
    auxiliary.extend_from_slice(&[0u8; 6]);
    let record = genesis_relation_record(
        &[(2394, 0), (2403, 0), (2404, 0)],
        2,
        &auxiliary,
        1425,
        0x100_0000_0000,
        &[2403, 2404],
    );
    let mut parsed =
        tested_parse_classed_sketch_relation(&record, SketchRelationClass::TextFrame).unwrap();
    assert_eq!(
        parsed
            .members
            .iter()
            .map(|row| row.reference.value)
            .collect::<Vec<_>>(),
        [2394, 2403, 2404]
    );
    assert_eq!(
        parsed
            .members
            .iter()
            .map(|row| row.relation_ordinal)
            .collect::<Vec<_>>(),
        [0, 0, 0]
    );
    assert_eq!(parsed.entity_genesis, Some(2));
    assert_eq!(
        parsed
            .auxiliary_references
            .iter()
            .map(|row| row.value)
            .collect::<Vec<_>>(),
        [2394]
    );
    assert_eq!(parsed.owner_reference, 1425);
    assert_eq!(parsed.state, 0x100_0000_0000);
    assert_eq!(
        parsed
            .return_members
            .iter()
            .map(|row| row.value)
            .collect::<Vec<_>>(),
        [2403, 2404]
    );
    assert_eq!(
        crate::records::sketch_relations::constraint_kinds_from_state(parsed.state),
        (vec![SketchConstraintKind::TextFrame], 0)
    );
    assert_eq!(
        decode_pattern_definition(&record, &mut parsed),
        Some(
            crate::records::sketch_relations::SketchPatternDefinition::TextFrame {
                text_reference: 2394
            }
        )
    );
}

#[test]
fn genesis_relation_parses_text_path_glyph_run() {
    let glyphs: [[[f64; 4]; 4]; 2] = [
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, -5.0627],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        [
            [1.0, 0.0, 0.0, 0.6216],
            [0.0, 1.0, 0.0, -5.0627],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    ];
    let mut auxiliary = vec![1u8];
    push_reference(&mut auxiliary, 304);
    auxiliary.extend_from_slice(&[0u8; 6]);
    auxiliary.extend_from_slice(&2u32.to_le_bytes());
    for transform in &glyphs {
        auxiliary.extend_from_slice(&16u32.to_le_bytes());
        for value in transform.iter().flatten() {
            auxiliary.extend_from_slice(&value.to_le_bytes());
        }
    }
    let record = genesis_relation_record(
        &[(237, 1), (304, 0)],
        2,
        &auxiliary,
        201,
        0x200_0000_0000,
        &[237],
    );
    let mut parsed = tested_parse_classed_sketch_relation(
        &record,
        SketchRelationClass::TextPath { leading_flag: true },
    )
    .unwrap();
    assert_eq!(
        parsed
            .members
            .iter()
            .map(|row| row.reference.value)
            .collect::<Vec<_>>(),
        [237, 304]
    );
    assert_eq!(
        parsed
            .members
            .iter()
            .map(|row| row.relation_ordinal)
            .collect::<Vec<_>>(),
        [1, 0]
    );
    assert_eq!(parsed.entity_genesis, Some(2));
    assert_eq!(
        parsed
            .auxiliary_references
            .iter()
            .map(|row| row.value)
            .collect::<Vec<_>>(),
        [304]
    );
    assert_eq!(parsed.owner_reference, 201);
    assert_eq!(parsed.state, 0x200_0000_0000);
    assert_eq!(
        parsed
            .return_members
            .iter()
            .map(|row| row.value)
            .collect::<Vec<_>>(),
        [237]
    );
    assert_eq!(
        match &parsed.class_members {
            super::super::RelationClassMembers::TextPath { glyph_transforms } =>
                Some(glyph_transforms),
            _ => None,
        }
        .map(|transforms| transforms
            .iter()
            .map(|transform| transform.rows())
            .collect::<Vec<_>>())
        .as_deref(),
        Some(&glyphs[..])
    );
    assert_eq!(
        crate::records::sketch_relations::constraint_kinds_from_state(parsed.state),
        (vec![SketchConstraintKind::TextPath], 0)
    );
    assert_eq!(
        decode_pattern_definition(&record, &mut parsed),
        Some(
            crate::records::sketch_relations::SketchPatternDefinition::TextPath {
                text_reference: 304,
                glyph_transforms: glyphs
                    .into_iter()
                    .map(
                        |rows| crate::records::sketch_relations::SketchGlyphTransform::try_from(
                            rows
                        )
                        .expect("finite native glyph")
                    )
                    .collect(),
            }
        )
    );
}

#[test]
fn genesis_relation_parses_circular_pattern_auxiliary_run() {
    let mut auxiliary = Vec::new();
    push_reference(&mut auxiliary, 336);
    auxiliary.extend_from_slice(&[0u8; 6]);
    push_reference(&mut auxiliary, 333);
    auxiliary.extend_from_slice(&[0u8; 6]);
    auxiliary.extend_from_slice(&std::f64::consts::TAU.to_le_bytes());
    auxiliary.extend_from_slice(&3u32.to_le_bytes());
    auxiliary.extend_from_slice(&[0u8; 9]);
    let record = genesis_relation_record(
        &[(280, 1), (291, 1), (327, 0), (330, 0)],
        2,
        &auxiliary,
        201,
        0x1000_0000,
        &[291, 327, 330, 280],
    );
    let mut parsed =
        tested_parse_classed_sketch_relation(&record, SketchRelationClass::CircularPattern)
            .unwrap();
    assert_eq!(
        parsed
            .members
            .iter()
            .map(|row| row.relation_ordinal)
            .collect::<Vec<_>>(),
        [1, 1, 0, 0]
    );
    assert_eq!(
        parsed
            .auxiliary_references
            .iter()
            .map(|row| row.value)
            .collect::<Vec<_>>(),
        [336, 333]
    );
    assert_eq!(parsed.state, 0x1000_0000);
    assert_eq!(
        decode_pattern_definition(&record, &mut parsed),
        Some(
            crate::records::sketch_relations::SketchPatternDefinition::Circular {
                angle_parameter: Some(336),
                count_parameter: Some(333),
                evaluated_angle: cadmpeg_ir::scalar::FiniteReal::new(std::f64::consts::TAU)
                    .unwrap(),
                evaluated_count: crate::records::sketch_relations::SketchPatternCount::try_from(3)
                    .unwrap(),
            }
        )
    );
}

#[test]
fn genesis_relation_parses_rectangular_pattern_auxiliary_run() {
    let mut auxiliary = Vec::new();
    push_reference(&mut auxiliary, 0);
    auxiliary.extend_from_slice(&[0u8; 10]);
    auxiliary.extend_from_slice(&3u32.to_le_bytes());
    push_reference(&mut auxiliary, 464);
    auxiliary.extend_from_slice(&[0u8; 6]);
    for value in [1.0f64, 0.0, 0.0, 3.0] {
        auxiliary.extend_from_slice(&value.to_le_bytes());
    }
    push_reference(&mut auxiliary, 470);
    auxiliary.extend_from_slice(&[0u8; 6]);
    auxiliary.extend_from_slice(&1u32.to_le_bytes());
    push_reference(&mut auxiliary, 467);
    auxiliary.extend_from_slice(&[0u8; 6]);
    for value in [0.0f64, 1.0, 0.0, 0.5] {
        auxiliary.extend_from_slice(&value.to_le_bytes());
    }
    push_reference(&mut auxiliary, 473);
    auxiliary.extend_from_slice(&[0u8; 6]);
    let record = genesis_relation_record(
        &[(352, 3), (353, 1), (442, 0), (445, 0)],
        2,
        &auxiliary,
        201,
        0x2000_0000,
        &[353, 352, 442, 445],
    );
    let mut parsed =
        tested_parse_classed_sketch_relation(&record, SketchRelationClass::RectangularPattern)
            .unwrap();
    assert_eq!(
        parsed
            .members
            .iter()
            .map(|row| row.relation_ordinal)
            .collect::<Vec<_>>(),
        [3, 1, 0, 0]
    );
    assert_eq!(
        parsed
            .auxiliary_references
            .iter()
            .map(|row| row.value)
            .collect::<Vec<_>>(),
        [464, 470, 467, 473]
    );
    assert!(matches!(
        parsed.class_members,
        super::super::RelationClassMembers::Rectangular {
            reference_count: 0,
            ..
        }
    ));
    assert_eq!(parsed.state, 0x2000_0000);
    let Some(crate::records::sketch_relations::SketchPatternDefinition::Rectangular { directions }) =
        decode_pattern_definition(&record, &mut parsed)
    else {
        panic!("expected rectangular pattern definition");
    };
    assert_eq!(directions[0].evaluated_count.get(), 3);
    assert_eq!(directions[0].count_parameter, Some(464));
    assert_eq!(directions[0].direction.get(), [1.0, 0.0, 0.0]);
    assert_eq!(directions[0].evaluated_distance.get(), 3.0);
    assert_eq!(directions[0].distance_parameter, Some(470));
    assert_eq!(directions[1].evaluated_count.get(), 1);
    assert_eq!(directions[1].count_parameter, Some(467));
    assert_eq!(directions[1].direction.get(), [0.0, 1.0, 0.0]);
    assert_eq!(directions[1].evaluated_distance.get(), 0.5);
    assert_eq!(directions[1].distance_parameter, Some(473));
}

#[test]
fn genesis_entity_header_variant_resolves_suffix_and_id() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"281");
    bytes.extend_from_slice(&201u32.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 10]);
    push_genesis_block(&mut bytes, 4);
    bytes.extend_from_slice(&5u32.to_le_bytes());
    let payload_start = bytes.len();
    for unit in "0_201".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let crate::design::decode::sketch::NamedEntityHeader {
        entity_id,
        entity_id_offset,
        optional_slot_present,
        end,
    } = parse_genesis_entity_header(&bytes, 0).unwrap();
    assert_eq!(entity_id_offset, payload_start);
    assert_eq!(entity_id.suffix(), 201);
    assert_eq!(entity_id.as_str(), "0_201");
    assert!(!optional_slot_present);
    assert_eq!(end, bytes.len());
    assert!(parse_settled_entity_header(&bytes, 0).is_none());
}

fn genesis_relation_record(
    members: &[(u32, u32)],
    genesis: u64,
    auxiliary: &[u8],
    owner: u32,
    mask: u64,
    returns: &[u32],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"298");
    out.extend_from_slice(&7u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 8]);
    out.push(1);
    out.extend_from_slice(&u32::try_from(members.len()).unwrap().to_le_bytes());
    for (reference, relation_ordinal) in members {
        push_reference(&mut out, *reference);
        out.extend_from_slice(&[0u8; 6]);
        out.extend_from_slice(&relation_ordinal.to_le_bytes());
    }
    push_genesis_block(&mut out, genesis);
    out.extend_from_slice(auxiliary);
    push_reference(&mut out, owner);
    out.extend_from_slice(&[0u8; 6]);
    out.extend_from_slice(&mask.to_le_bytes());
    out.extend_from_slice(&u32::try_from(returns.len()).unwrap().to_le_bytes());
    for reference in returns {
        push_reference(&mut out, *reference);
        out.extend_from_slice(&[0u8; 6]);
    }
    out.extend_from_slice(&[0u8; 4]);
    out
}

#[test]
fn indexed_record_header_requires_a_complete_class_header() {
    use crate::design::decode::sketch::indexed_record_header_at;

    let index_at = |bytes: &[u8], at: usize| {
        indexed_record_header_at(bytes, at).map(|header| header.record_index)
    };
    let header = [3, 0, 0, 0, b'2', b'5', b'7', 42, 0, 0, 0];
    assert_eq!(index_at(&header, 0), Some(42));
    for (offset, byte) in [(0, 2), (4, b'x'), (5, 0), (6, b' ')] {
        let mut invalid = header;
        invalid[offset] = byte;
        assert_eq!(index_at(&invalid, 0), None);
    }
    for length in 0..header.len() {
        assert_eq!(index_at(&header[..length], 0), None);
    }
    assert_eq!(index_at(&header, usize::MAX), None);
}

fn registered_type(
    type_guid: &str,
    entities: &[u64],
    byte_offset: u64,
) -> crate::records::entity_header::SegmentType {
    crate::records::entity_header::SegmentType::try_new(
        format!("f3d:Design/MetaStream.dat:design-type#{byte_offset}"),
        crate::records::entity_header::SegmentTypeData {
            byte_offset,
            type_guid: type_guid.to_owned().try_into().unwrap(),
            type_guid_offset: 4,
            base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
            version: 1,
            version_offset: 44,
            module: "MSketch".into(),
            entities: crate::records::identity::ReferenceRun::located(
                entities
                    .iter()
                    .map(|value| crate::records::identity::Located {
                        value: *value,
                        offset: 100,
                    })
                    .collect(),
            ),
        },
    )
    .unwrap()
}

/// Relation records come from the relation-class registrations of the type
/// table, not from a sketch member run; other registrations do not count.
#[test]
fn relation_records_are_the_relation_class_registrations() {
    let relation = registered_type("60403D47-0C49-49B0-BDE8-1679608164A2", &[547, 548], 10);
    let container = registered_type("44A64366-4BD3-4B24-881A-F94C206E8F2D", &[222], 20);
    let indices =
        crate::design::decode::sketch::registered_relation_record_indices([&relation, &container]);
    assert_eq!(indices, [547, 548]);
}
