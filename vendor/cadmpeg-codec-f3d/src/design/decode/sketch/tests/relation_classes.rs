// SPDX-License-Identifier: Apache-2.0

mod cast_limits;

use crate::design::decode::sketch::{
    admit_sketch_relation, decode_pattern_definition, parse_classed_sketch_relation,
    rectangular_pattern_flags, relation_mask_width, SketchRelationClass, SketchRelationMaskWidth,
};
use crate::records::sketch_relations::{SketchPatternDefinition, SketchPatternDirection};
use crate::test_support::push_reference_u64;

#[test]
fn sketch_relation_assembly_admits_a_plain_relation() {
    use crate::records::decal::DesignRecordHeader;
    use crate::records::sketch_relations::SketchRelationDefinition;

    let record = relation_record(&[(300, 0)], &[], 201, 1, &[300]);
    let header = DesignRecordHeader {
        id: "f3d:BulkStream.dat:design-record-header#0".to_owned(),
        record_index: 7,
        class_tag: crate::records::references::DesignClassTag::try_from("298".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let parsed = tested_parse_classed_sketch_relation(&record, SketchRelationClass::Plain)
        .expect("plain relation parse");
    let definition =
        SketchRelationDefinition::new(parsed.state, None).expect("plain relation definition");
    let mut admitted = Vec::new();
    admit_sketch_relation(
        &mut admitted,
        "BulkStream.dat",
        &header,
        &record,
        parsed,
        definition,
    )
    .expect("relation admission");
    assert_eq!(admitted.len(), 1);
    assert_eq!(
        admitted[0].id,
        format!(
            "{}:sketch-relation#7",
            crate::ids::native_scope("BulkStream.dat")
        )
    );
    assert_eq!(admitted[0].raw_bytes(), record);
}

fn tested_parse_classed_sketch_relation(
    payload: &[u8],
    class: SketchRelationClass,
) -> Option<crate::design::decode::sketch::ParsedSketchRelation> {
    parse_classed_sketch_relation(payload, class).unwrap()
}

/// One absent reference.
fn push_absent_reference(out: &mut Vec<u8>) {
    out.push(0);
}

/// A relation record: the header, the paired member run, an empty property
/// block, `class_members`, `ParentNode`, the u64 mask, the return run, and
/// the trailing zero byte. The record ends where the parse must end.
fn relation_record(
    members: &[(u32, u32)],
    class_members: &[u8],
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
    out.extend_from_slice(
        &u32::try_from(members.len())
            .expect("member count fits a u32")
            .to_le_bytes(),
    );
    for (reference, ordinal) in members {
        push_reference_u64(&mut out, u64::from(*reference));
        out.extend_from_slice(&ordinal.to_le_bytes());
    }
    out.push(0);
    out.extend_from_slice(class_members);
    push_reference_u64(&mut out, u64::from(owner));
    out.extend_from_slice(&mask.to_le_bytes());
    out.extend_from_slice(
        &u32::try_from(returns.len())
            .expect("return count fits a u32")
            .to_le_bytes(),
    );
    for reference in returns {
        push_reference_u64(&mut out, u64::from(*reference));
    }
    out.push(0);
    out
}

/// A relation-base-class version-0 record: no paired run and a u32 mask.
fn legacy_relation_record(owner: u32, mask: u32, returns: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"298");
    out.extend_from_slice(&7u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 8]);
    out.push(0);
    out.push(0);
    push_reference_u64(&mut out, u64::from(owner));
    out.extend_from_slice(&mask.to_le_bytes());
    out.extend_from_slice(
        &u32::try_from(returns.len())
            .expect("return count fits a u32")
            .to_le_bytes(),
    );
    for reference in returns {
        push_reference_u64(&mut out, u64::from(*reference));
    }
    out.push(0);
    out
}

/// The two pattern tables, both empty.
fn empty_pattern_tables() -> [u8; 8] {
    [0u8; 8]
}

/// One rectangular direction clause.
fn push_direction_clause(
    out: &mut Vec<u8>,
    count: u32,
    count_parameter: u32,
    direction: [f64; 3],
    distance: f64,
    distance_parameter: u32,
) {
    out.extend_from_slice(&count.to_le_bytes());
    push_reference_u64(out, u64::from(count_parameter));
    for axis in direction {
        out.extend_from_slice(&axis.to_le_bytes());
    }
    out.extend_from_slice(&distance.to_le_bytes());
    push_reference_u64(out, u64::from(distance_parameter));
}

/// A glyph run: the text reference, the character count, and one block of
/// `u32 16` and a row-major 4x4 transform.
fn push_glyph_run(out: &mut Vec<u8>, text: u32, translation: f64) {
    push_reference_u64(out, u64::from(text));
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&16u32.to_le_bytes());
    for row in 0..4 {
        for column in 0..4 {
            let value = if row == 0 && column == 3 {
                translation
            } else {
                f64::from(u8::from(row == column))
            };
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
}

#[test]
fn relation_classes_are_named_by_type_guid() {
    assert_eq!(
        SketchRelationClass::of("60403D47-0C49-49B0-BDE8-1679608164A2", 3),
        Some(SketchRelationClass::Plain)
    );
    assert_eq!(
        SketchRelationClass::of("d3bd153b-eb8a-405e-9d29-69ee0c3d227c", 0),
        Some(SketchRelationClass::Plain)
    );
    assert_eq!(
        SketchRelationClass::of("73762C3B-82DC-4632-93B0-B8FE1CC5282F", 0),
        Some(SketchRelationClass::Plain)
    );
    assert_eq!(
        SketchRelationClass::of("24DB790E-3DCD-4336-AFA3-6F119EF2239B", 0),
        Some(SketchRelationClass::Tangent)
    );
    assert_eq!(
        SketchRelationClass::of("8269E861-0BB7-47E0-9911-5AE3EC475058", 3),
        Some(SketchRelationClass::CircularPattern)
    );
    assert_eq!(
        SketchRelationClass::of("40800FB9-C2BE-494E-A047-7D76E82B9F6C", 5),
        Some(SketchRelationClass::RectangularPattern)
    );
    assert_eq!(
        SketchRelationClass::of("8B369926-123F-4F9D-878E-6D4C076128D3", 0),
        Some(SketchRelationClass::TextFrame)
    );
    assert_eq!(
        SketchRelationClass::of("9D30FCDC-EA07-4141-93E2-918B1A59E962", 0),
        Some(SketchRelationClass::TextPath {
            leading_flag: false
        })
    );
    assert_eq!(
        SketchRelationClass::of("9D30FCDC-EA07-4141-93E2-918B1A59E962", 1),
        Some(SketchRelationClass::TextPath { leading_flag: true })
    );
    assert_eq!(
        SketchRelationClass::of("69EE2FA7-BCC7-449E-9CA9-976CEFDFED44", 0),
        None
    );
}

#[test]
fn relation_leading_block_selects_member_run_and_mask_width() {
    let modern = relation_record(&[(300, 0)], &[], 201, 0x0020_0000_0000, &[300]);
    assert_eq!(
        relation_mask_width(&modern),
        Some(SketchRelationMaskWidth::U64)
    );
    let modern_parsed =
        tested_parse_classed_sketch_relation(&modern, SketchRelationClass::Plain).unwrap();
    assert_eq!(modern_parsed.state, 0x0020_0000_0000);
    assert_eq!(
        modern_parsed
            .members
            .iter()
            .map(|row| row.reference.value)
            .collect::<Vec<_>>(),
        [300]
    );

    let legacy = legacy_relation_record(201, 0x8000_0000, &[300]);
    assert_eq!(
        relation_mask_width(&legacy),
        Some(SketchRelationMaskWidth::U32)
    );
    let legacy_parsed =
        tested_parse_classed_sketch_relation(&legacy, SketchRelationClass::Plain).unwrap();
    assert_eq!(legacy_parsed.state, 0x8000_0000);
    assert!(legacy_parsed.members.is_empty());
    assert_eq!(
        legacy_parsed
            .return_members
            .iter()
            .map(|row| row.value)
            .collect::<Vec<_>>(),
        [300]
    );

    let mut invalid = legacy;
    invalid[19] = 2;
    assert_eq!(relation_mask_width(&invalid), None);
    assert!(tested_parse_classed_sketch_relation(&invalid, SketchRelationClass::Plain).is_none());
}

#[test]
fn plain_relation_reads_parent_node_without_class_members() {
    let record = relation_record(&[(300, 1), (301, 0)], &[], 201, 0x1, &[300, 301]);
    let parsed = tested_parse_classed_sketch_relation(&record, SketchRelationClass::Plain)
        .expect("the classed parse reads the record");
    assert_eq!(parsed.owner_reference, 201);
    assert_eq!(parsed.state, 0x1);
    assert_eq!(
        parsed
            .members
            .iter()
            .map(|row| row.reference.value)
            .collect::<Vec<_>>(),
        [300, 301]
    );
    assert_eq!(
        parsed
            .return_members
            .iter()
            .map(|row| row.value)
            .collect::<Vec<_>>(),
        [300, 301]
    );
    assert!(parsed.auxiliary_references.is_empty());
    assert_eq!(parsed.parsed_end, record.len());
}

#[test]
fn tangent_relation_reads_its_three_flags() {
    // The middle flag is `1`, which the reference-marker walk reads as the
    // presence byte of a reference and steps into the flags.
    let record = relation_record(&[(300, 1), (301, 0)], &[0, 1, 0], 201, 0x100, &[300, 301]);
    let parsed = tested_parse_classed_sketch_relation(&record, SketchRelationClass::Tangent)
        .expect("the classed parse reads the record");
    assert_eq!(parsed.owner_reference, 201);
    assert_eq!(parsed.state, 0x100);
    assert!(parsed.auxiliary_references.is_empty());
    assert_eq!(parsed.parsed_end, record.len());
    assert!(tested_parse_classed_sketch_relation(
        &relation_record(&[(300, 1)], &[0, 2, 0], 201, 0x100, &[300]),
        SketchRelationClass::Tangent
    )
    .is_none());
}

#[test]
fn circular_pattern_relation_reads_its_parameters_and_tables() {
    let mut class_members = Vec::new();
    push_reference_u64(&mut class_members, 336);
    push_reference_u64(&mut class_members, 333);
    class_members.extend_from_slice(&std::f64::consts::TAU.to_le_bytes());
    class_members.extend_from_slice(&3u32.to_le_bytes());
    class_members.extend_from_slice(&empty_pattern_tables());
    class_members.push(0);
    let record = relation_record(
        &[(300, 1), (301, 0)],
        &class_members,
        201,
        0x1000_0000,
        &[300, 301],
    );
    let mut parsed =
        tested_parse_classed_sketch_relation(&record, SketchRelationClass::CircularPattern)
            .expect("the classed parse reads the record");
    assert_eq!(parsed.owner_reference, 201);
    assert_eq!(
        parsed
            .auxiliary_references
            .iter()
            .map(|row| row.value)
            .collect::<Vec<_>>(),
        [336, 333]
    );
    assert_eq!(parsed.parsed_end, record.len());
    assert_eq!(
        decode_pattern_definition(&record, &mut parsed),
        Some(SketchPatternDefinition::Circular {
            angle_parameter: Some(336),
            count_parameter: Some(333),
            evaluated_angle: cadmpeg_ir::scalar::FiniteReal::new(std::f64::consts::TAU).unwrap(),
            evaluated_count: crate::records::sketch_relations::SketchPatternCount::try_from(3)
                .unwrap(),
        })
    );
}

#[test]
fn circular_pattern_relation_reads_populated_tables_and_absent_parameters() {
    let mut class_members = Vec::new();
    push_absent_reference(&mut class_members);
    push_absent_reference(&mut class_members);
    class_members.extend_from_slice(&std::f64::consts::TAU.to_le_bytes());
    class_members.extend_from_slice(&6u32.to_le_bytes());
    // One map entry keyed `1` holding two values, then a two-entry u32 run.
    class_members.extend_from_slice(&1u32.to_le_bytes());
    class_members.extend_from_slice(&1u64.to_le_bytes());
    class_members.extend_from_slice(&2u32.to_le_bytes());
    class_members.extend_from_slice(&122u64.to_le_bytes());
    class_members.extend_from_slice(&118u64.to_le_bytes());
    class_members.extend_from_slice(&2u32.to_le_bytes());
    class_members.extend_from_slice(&1u32.to_le_bytes());
    class_members.extend_from_slice(&2u32.to_le_bytes());
    class_members.push(0);
    let record = relation_record(&[(300, 1)], &class_members, 201, 0x1000_0000, &[300]);
    let mut parsed =
        tested_parse_classed_sketch_relation(&record, SketchRelationClass::CircularPattern)
            .expect("the classed parse reads the record");
    assert_eq!(parsed.owner_reference, 201);
    assert!(parsed.auxiliary_references.is_empty());
    assert_eq!(parsed.parsed_end, record.len());
    assert_eq!(
        decode_pattern_definition(&record, &mut parsed),
        Some(SketchPatternDefinition::Circular {
            angle_parameter: None,
            count_parameter: None,
            evaluated_angle: cadmpeg_ir::scalar::FiniteReal::new(std::f64::consts::TAU).unwrap(),
            evaluated_count: crate::records::sketch_relations::SketchPatternCount::try_from(6)
                .unwrap(),
        })
    );
}

#[test]
fn rectangular_pattern_relation_reads_a_nonempty_reference_run_before_its_clauses() {
    let mut class_members = vec![1, 0, 0];
    class_members.extend_from_slice(&1u32.to_le_bytes());
    push_reference_u64(&mut class_members, 900);
    class_members.extend_from_slice(&empty_pattern_tables());
    push_direction_clause(&mut class_members, 3, 464, [1.0, 0.0, 0.0], 3.0, 470);
    push_direction_clause(&mut class_members, 1, 467, [0.0, 1.0, 0.0], 0.5, 473);
    let record = relation_record(
        &[(300, 1), (301, 0)],
        &class_members,
        201,
        0x2000_0000,
        &[300, 301],
    );
    let mut parsed =
        tested_parse_classed_sketch_relation(&record, SketchRelationClass::RectangularPattern)
            .expect("the classed parse reads the record");
    assert_eq!(parsed.owner_reference, 201);
    assert_eq!(
        parsed
            .auxiliary_references
            .iter()
            .map(|row| row.value)
            .collect::<Vec<_>>(),
        [900, 464, 470, 467, 473]
    );
    assert!(matches!(
        parsed.class_members,
        super::super::RelationClassMembers::Rectangular {
            reference_count: 1,
            ..
        }
    ));
    assert!(matches!(
        parsed.class_members,
        super::super::RelationClassMembers::Rectangular {
            clauses,
            ..
        } if clauses.map(|clause| [(clause.count_parameter, clause.count_offset + 5), (clause.distance_parameter, clause.direction_offset + 33)]).into_iter().flatten().eq(parsed.auxiliary_references[1..5].iter().map(|row| (Some(row.value), row.offset)))
    ));
    assert_eq!(parsed.parsed_end, record.len());
    assert_eq!(
        decode_pattern_definition(&record, &mut parsed),
        Some(SketchPatternDefinition::Rectangular {
            directions: [
                SketchPatternDirection {
                    evaluated_count:
                        crate::records::sketch_relations::SketchPatternCount::try_from(3).unwrap(),
                    count_parameter: Some(464),
                    direction: [1.0, 0.0, 0.0].try_into().unwrap(),
                    evaluated_distance: cadmpeg_ir::scalar::FiniteReal::new(3.0).unwrap(),
                    distance_parameter: Some(470),
                },
                SketchPatternDirection {
                    evaluated_count:
                        crate::records::sketch_relations::SketchPatternCount::try_from(1).unwrap(),
                    count_parameter: Some(467),
                    direction: [0.0, 1.0, 0.0].try_into().unwrap(),
                    evaluated_distance: cadmpeg_ir::scalar::FiniteReal::new(0.5).unwrap(),
                    distance_parameter: Some(473),
                },
            ],
        })
    );
}

#[test]
fn rectangular_pattern_relation_reads_clauses_after_an_empty_reference_run() {
    let mut class_members = vec![0, 0, 0];
    class_members.extend_from_slice(&0u32.to_le_bytes());
    class_members.extend_from_slice(&empty_pattern_tables());
    push_direction_clause(&mut class_members, 4, 464, [1.0, 0.0, 0.0], 2.0, 470);
    push_direction_clause(&mut class_members, 2, 467, [0.0, 1.0, 0.0], 1.5, 473);
    let record = relation_record(&[(300, 1)], &class_members, 201, 0x2000_0000, &[300]);
    let mut parsed =
        tested_parse_classed_sketch_relation(&record, SketchRelationClass::RectangularPattern)
            .expect("the classed parse reads the record");
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
    assert!(matches!(
        parsed.class_members,
        super::super::RelationClassMembers::Rectangular {
            clauses,
            ..
        } if clauses.map(|clause| [(clause.count_parameter, clause.count_offset + 5), (clause.distance_parameter, clause.direction_offset + 33)]).into_iter().flatten().eq(parsed.auxiliary_references[0..4].iter().map(|row| (Some(row.value), row.offset)))
    ));
    assert_eq!(parsed.parsed_end, record.len());
    let Some(SketchPatternDefinition::Rectangular { directions }) =
        decode_pattern_definition(&record, &mut parsed)
    else {
        panic!("expected a rectangular pattern definition");
    };
    assert_eq!(directions[0].evaluated_count.get(), 4);
    assert_eq!(directions[1].evaluated_count.get(), 2);
}

#[test]
fn rectangular_pattern_retains_nonempty_count_with_an_absent_reference() {
    let mut class_members = vec![0, 0, 0];
    class_members.extend_from_slice(&1u32.to_le_bytes());
    push_absent_reference(&mut class_members);
    class_members.extend_from_slice(&empty_pattern_tables());
    push_direction_clause(&mut class_members, 2, 464, [1.0, 0.0, 0.0], 1.5, 470);
    push_direction_clause(&mut class_members, 1, 467, [0.0, 1.0, 0.0], 0.0, 473);
    let record = relation_record(&[(300, 1)], &class_members, 201, 0x2000_0000, &[300]);
    let mut parsed =
        tested_parse_classed_sketch_relation(&record, SketchRelationClass::RectangularPattern)
            .expect("the classed parse reads the absent run member");

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
            reference_count: 1,
            ..
        }
    ));
    assert!(matches!(
        parsed.class_members,
        super::super::RelationClassMembers::Rectangular {
            clauses,
            ..
        } if clauses.map(|clause| [(clause.count_parameter, clause.count_offset + 5), (clause.distance_parameter, clause.direction_offset + 33)]).into_iter().flatten().eq(parsed.auxiliary_references[0..4].iter().map(|row| (Some(row.value), row.offset)))
    ));
    assert!(matches!(
        decode_pattern_definition(&record, &mut parsed),
        Some(SketchPatternDefinition::Rectangular { .. })
    ));
}

#[test]
fn rectangular_pattern_withholds_an_invalid_count_beside_a_null_parameter() {
    let mut class_members = vec![0, 0, 0];
    class_members.extend_from_slice(&2u32.to_le_bytes());
    // The first counted-run reference uses the segment form. Its trailing
    // segment value is a plausible count if a reader starts one reference
    // early.
    class_members.push(1);
    class_members.extend_from_slice(&900u64.to_le_bytes());
    class_members.extend_from_slice(&[0, 1]);
    class_members.extend_from_slice(&2u32.to_le_bytes());
    push_reference_u64(&mut class_members, 901);
    class_members.extend_from_slice(&empty_pattern_tables());

    class_members.extend_from_slice(&0u32.to_le_bytes());
    push_absent_reference(&mut class_members);
    // Together with the empty table counts and absent-reference marker,
    // these bytes form a shifted unit vector `[0, 1, 0]`.
    class_members.extend_from_slice(&[0x00, 0xf0, 0x3f, 0, 0, 0, 0, 0]);
    class_members.extend_from_slice(&0.0f64.to_le_bytes());
    class_members.extend_from_slice(&0.0f64.to_le_bytes());
    class_members.extend_from_slice(&0.0f64.to_le_bytes());
    push_reference_u64(&mut class_members, 470);

    push_direction_clause(&mut class_members, 1, 467, [1.0, 0.0, 0.0], 0.5, 473);
    let record = relation_record(&[(300, 1)], &class_members, 201, 0x2000_0000, &[300]);
    let mut parsed =
        tested_parse_classed_sketch_relation(&record, SketchRelationClass::RectangularPattern)
            .expect("the classed parse retains the incomplete relation");

    assert_eq!(
        parsed
            .auxiliary_references
            .iter()
            .map(|row| row.value)
            .collect::<Vec<_>>(),
        [900, 901, 470, 467, 473]
    );
    assert!(matches!(
        parsed.class_members,
        super::super::RelationClassMembers::Rectangular {
            reference_count: 2,
            ..
        }
    ));
    assert!(matches!(
        parsed.class_members,
        super::super::RelationClassMembers::Rectangular { clauses, .. } if clauses[0].count_parameter.is_none()
    ));
    assert_eq!(decode_pattern_definition(&record, &mut parsed), None);
}

#[test]
fn text_frame_relation_reads_its_two_references() {
    let mut class_members = Vec::new();
    push_absent_reference(&mut class_members);
    push_reference_u64(&mut class_members, 2394);
    let record = relation_record(
        &[(2394, 0), (2403, 0)],
        &class_members,
        201,
        0x100_0000_0000,
        &[2403],
    );
    let mut parsed = tested_parse_classed_sketch_relation(&record, SketchRelationClass::TextFrame)
        .expect("the classed parse reads the record");
    assert_eq!(
        parsed
            .auxiliary_references
            .iter()
            .map(|row| row.value)
            .collect::<Vec<_>>(),
        [2394]
    );
    assert_eq!(parsed.parsed_end, record.len());
    assert_eq!(
        decode_pattern_definition(&record, &mut parsed),
        Some(SketchPatternDefinition::TextFrame {
            text_reference: 2394
        })
    );

    let mut both = Vec::new();
    push_reference_u64(&mut both, 2404);
    push_reference_u64(&mut both, 2394);
    let record = relation_record(
        &[(2394, 0), (2403, 0)],
        &both,
        201,
        0x100_0000_0000,
        &[2403],
    );
    let parsed = tested_parse_classed_sketch_relation(&record, SketchRelationClass::TextFrame)
        .expect("the classed parse reads the record");
    assert_eq!(
        parsed
            .auxiliary_references
            .iter()
            .map(|row| row.value)
            .collect::<Vec<_>>(),
        [2404, 2394]
    );
    assert_eq!(parsed.parsed_end, record.len());
}

#[test]
fn text_path_relation_reads_its_glyph_run_at_both_versions() {
    for leading_flag in [false, true] {
        let mut class_members = Vec::new();
        if leading_flag {
            class_members.push(1);
        }
        push_glyph_run(&mut class_members, 2, 5.0);
        let record = relation_record(
            &[(1, 1), (2, 0)],
            &class_members,
            201,
            0x200_0000_0000,
            &[1],
        );
        let mut parsed = tested_parse_classed_sketch_relation(
            &record,
            SketchRelationClass::TextPath { leading_flag },
        )
        .expect("the classed parse reads the record");
        assert_eq!(
            parsed
                .auxiliary_references
                .iter()
                .map(|row| row.value)
                .collect::<Vec<_>>(),
            [2]
        );
        assert_eq!(parsed.parsed_end, record.len());
        let Some(SketchPatternDefinition::TextPath {
            text_reference,
            glyph_transforms,
        }) = decode_pattern_definition(&record, &mut parsed)
        else {
            panic!("expected a text-path pattern definition");
        };
        assert_eq!(text_reference, 2);
        assert_eq!(glyph_transforms[0].rows()[0][3], 5.0);
        // The version-0 layout has no leading byte, so reading one steps
        // into the text reference and the run no longer closes.
        assert!(tested_parse_classed_sketch_relation(
            &record,
            SketchRelationClass::TextPath {
                leading_flag: !leading_flag
            }
        )
        .is_none_or(|other| other.parsed_end != record.len()));
    }
}

#[test]
fn text_path_glyph_constructor_rejects_non_finite_source_coefficients() {
    for translation in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut class_members = Vec::new();
        push_glyph_run(&mut class_members, 2, translation);
        let record = relation_record(
            &[(1, 1), (2, 0)],
            &class_members,
            201,
            0x200_0000_0000,
            &[1],
        );
        assert!(tested_parse_classed_sketch_relation(
            &record,
            SketchRelationClass::TextPath {
                leading_flag: false
            }
        )
        .is_none());
    }
}

#[test]
fn fixture_relation_frames_retain_source_bytes_and_rectangular_flags() {
    let cases = crate::test_support::sketch_relations::relation_fixture_records();
    assert_eq!(
        cases.len(),
        crate::test_support::verification::case_count(8, 300)
    );
    for case in cases {
        let relation = case.decode();
        assert_eq!(
            relation.raw_bytes(),
            case.source_hex.as_ref(),
            "{}",
            case.label
        );
        assert_eq!(
            rectangular_pattern_flags(relation.raw_bytes()),
            case.rectangular_flags,
            "{}",
            case.label
        );
        let json = serde_json::to_string(&relation).unwrap();
        let restored: crate::records::sketch_relations::SketchRelation =
            serde_json::from_str(&json).unwrap();
        assert_eq!(restored, relation, "{}", case.label);
    }
}

#[test]
fn text_frame_center_does_not_substitute_for_an_absent_text_reference() {
    let mut class_members = Vec::new();
    push_reference_u64(&mut class_members, 2394);
    push_absent_reference(&mut class_members);
    let record = relation_record(
        &[(2394, 0), (2403, 0)],
        &class_members,
        201,
        0x100_0000_0000,
        &[2403],
    );
    let mut parsed =
        tested_parse_classed_sketch_relation(&record, SketchRelationClass::TextFrame).unwrap();
    assert_eq!(decode_pattern_definition(&record, &mut parsed), None);
}
