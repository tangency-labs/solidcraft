// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::operands::body_recipe_operand_end;
use crate::design::decode::operands::body_recipe_prologue_end;
use crate::design::decode::operands::parse_body_recipe_operand;
use crate::design::decode::operands::unique_body_recipe;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::recipes::ConstructionRecipe;
use crate::records::recipes::ConstructionRecipeKind;
use crate::records::sketch_links::PersistentSubentityTag;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::construction::DesignConstructionOperandGroupFrame;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::test_support::indexed_header;
use crate::test_support::lp_utf16;
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::ids::FaceId;

#[test]
fn body_recipe_operand_decodes_counted_and_empty_reference_tables() {
    let mut group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
            scope_record_index: 80,
            scope_reference_ordinal: 0,
            record_index: 90,
            byte_offset: 900,
            class_tag: crate::records::references::DesignClassTag::try_from("269".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 100,
                offset: 926,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 921,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: vec![crate::records::identity::Located {
                        value: 200,
                        offset: 943,
                    }],
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
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::ROLE_0X5,
                ),
            role_offset: 953,

            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "265".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 1024,
        },
    )
    .unwrap();
    let mut record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("365".to_owned()).unwrap(),
        record_index: 100,
    };
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"365", 100);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&2265u64.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&2266u64.to_le_bytes());
    bytes.extend_from_slice(&32u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&103u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "53aa8ab4-194a-434b-bd52-8c6d761dc147");
    lp_utf16(&mut bytes, "8e685642-4d68-4909-96d0-0dd4437491b6");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[7, 0, 0, 0]);
    indexed_header(&mut bytes, *b"259", 100);
    indexed_header(&mut bytes, *b"283", 101);
    indexed_header(&mut bytes, *b"463", 102);
    indexed_header(&mut bytes, *b"452", 103);
    let recipe_at = bytes.len();
    bytes.extend_from_slice(b"body_recipe_data");
    let next_at = bytes.len();
    indexed_header(&mut bytes, *b"311", 104);
    let mut recipe = ConstructionRecipe {
        id: format!("f3d:Design/BulkStream.dat:construction-recipe#{recipe_at}"),
        byte_offset: u64_from_index(recipe_at),
        kind: ConstructionRecipeKind::Body,
        design: Some(crate::records::recipes::ConstructionRecipeDesign {
            id: crate::records::identity::RecordedValue {
                value: "2265".into(),
                offset: 0,
            },
            selector: Some(crate::records::recipes::ConstructionRecipeSelector {
                value: 9,
                byte_offset: 0,
            }),
        }),
        recipe_index: 0,
        record_index: Some(crate::records::identity::RecordedValue {
            value: 0,
            offset: 0,
        }),
    };
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#80",
        crate::records::feature::scope::DesignFeatureKind::BoundaryFill,
        80,
    );

    let mut operand = parse_body_recipe_operand(&bytes, &group, 0, &record, &recipe)
        .expect("body recipe operand");
    assert_eq!(operand.references().len(), 2);
    assert_eq!(operand.references()[0].design_reference, 2265);
    assert_eq!(operand.references()[0].form, 3);
    assert_eq!(operand.references()[1].design_reference, 2266);
    assert_eq!(operand.references()[1].form, 32);
    assert_eq!(
        operand
            .clone()
            .into_draft()
            .selector_tail
            .map(|tail| tail.value),
        Some([7, 0, 0, 0])
    );
    assert_eq!(
        operand
            .clone()
            .into_draft()
            .selector_tail
            .map(|tail| tail.offset),
        Some(220)
    );
    assert_eq!(
        operand.owner,
        crate::records::topology::body_recipe::DesignOperandOwner::Group {
            group_record_index: 90,
            group_member_ordinal: 0,
        }
    );
    assert_eq!(operand.nested_record_index(), 103);
    assert_eq!(operand.recipe_id, recipe.id);
    assert_eq!(operand.next_byte_offset(), u64_from_index(next_at));
    operand.id = "f3d:Design/BulkStream.dat:body-recipe-operand#0".into();
    let candidate_tags = [
        PersistentSubentityTag {
            id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#1".into(),
            target: AttributeTarget::Face(
                FaceId::mint("test:model:face#same-stream").expect("identity grammar"),
            ),
            selector: 1,
            token: cadmpeg_core::text::NonBlankString::new("0").unwrap(),
            design_references: vec![2265],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#2".into(),
            target: AttributeTarget::Face(
                FaceId::mint("test:model:face#other-selector").expect("identity grammar"),
            ),
            selector: 2,
            token: cadmpeg_core::text::NonBlankString::new("0").unwrap(),
            design_references: vec![2265, 2266],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "f3d:xref/Other/occurrence-0/design:persistent-subentity-tag#1".into(),
            target: AttributeTarget::Face(
                FaceId::mint("test:model:face#other-stream").expect("identity grammar"),
            ),
            selector: 0,
            token: cadmpeg_core::text::NonBlankString::new("0").unwrap(),
            design_references: vec![2265],
            ordinal: 0,
        },
    ];
    crate::design::decode::operands::bind_body_recipe_operand_candidates(
        std::slice::from_mut(&mut operand),
        std::slice::from_ref(&recipe),
        &candidate_tags,
        std::slice::from_ref(&scope),
    );
    assert_eq!(
        operand.references()[0].candidate_faces,
        [
            FaceId::mint("test:model:face#other-selector").expect("identity grammar"),
            FaceId::mint("test:model:face#same-stream").expect("identity grammar")
        ]
    );

    // A legacy Combine tool keeps the same identity envelope with no
    // persistent Design-reference clauses. The marker therefore follows the
    // zero count at the ordinary reference-table cursor.
    let mut empty_bytes = bytes[..25].to_vec();
    empty_bytes[21..25].fill(0);
    empty_bytes.extend_from_slice(&bytes[49..]);
    let empty_recipe_at = recipe_at - 24;
    let empty_next_at = next_at - 24;
    let empty_recipe = ConstructionRecipe {
        id: format!("f3d:Design/BulkStream.dat:construction-recipe#{empty_recipe_at}"),
        byte_offset: u64_from_index(empty_recipe_at),
        ..recipe.clone()
    };
    let empty = parse_body_recipe_operand(&empty_bytes, &group, 0, &record, &empty_recipe)
        .expect("empty body recipe operand");
    assert!(empty.references().is_empty());
    assert_eq!(empty.nested_record_index(), 103);
    assert_eq!(empty.next_byte_offset(), u64_from_index(empty_next_at));

    let mut combine_scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#80",
        crate::records::feature::scope::DesignFeatureKind::Combine,
        80,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::Combine(slot) =
        combine_scope.payload_mut()
    {
        *slot = Some(crate::records::feature::combine::DesignCombineOperation {
            form: crate::records::feature::combine::DesignCombineForm::Standard,
            operation: cadmpeg_ir::features::BooleanKind::Join,
            operation_offset: 0,
            keep_tools: false,
            keep_tools_offset: 0,
            target_record_index: 0,
            target_body_node: None,
            tools: crate::records::feature::combine::DesignCombineTools {
                first: crate::records::feature::combine::DesignCombineBodySelection {
                    record_index: record.record_index,
                    external_identity: None,
                    body_node: None,
                },
                additional: Vec::new(),
            },
        });
    }
    let mut combine_recipe = recipe.clone();
    combine_recipe.design.as_mut().unwrap().selector =
        Some(crate::records::recipes::ConstructionRecipeSelector {
            value: 1,
            byte_offset: 0,
        });
    let mut combine_operand = operand.clone();
    crate::design::decode::operands::bind_body_recipe_operand_candidates(
        std::slice::from_mut(&mut combine_operand),
        std::slice::from_ref(&combine_recipe),
        &[
            PersistentSubentityTag {
                id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("test:model:face#same-stream").expect("identity grammar"),
                ),
                selector: 1,
                token: cadmpeg_core::text::NonBlankString::new("0").unwrap(),
                design_references: vec![2265],
                ordinal: 0,
            },
            PersistentSubentityTag {
                id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#2".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("test:model:face#other-selector").expect("identity grammar"),
                ),
                selector: 2,
                token: cadmpeg_core::text::NonBlankString::new("0").unwrap(),
                design_references: vec![2265, 2266],
                ordinal: 0,
            },
        ],
        std::slice::from_ref(&combine_scope),
    );
    assert_eq!(
        combine_operand.references()[0].candidate_faces,
        [FaceId::mint("test:model:face#same-stream").expect("identity grammar")]
    );

    let mut nested = Vec::new();
    indexed_header(&mut nested, *b"302", 1);
    indexed_header(&mut nested, *b"305", 11);
    bytes.splice(next_at..next_at, nested.iter().copied());
    let operand = parse_body_recipe_operand(&bytes, &group, 0, &record, &recipe)
        .expect("body recipe operand with nested recipe records");
    assert_eq!(
        operand.next_byte_offset(),
        u64_from_index(next_at + nested.len())
    );

    let stream_name = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let archive = crate::test_support::zip_test::f3d_with_configuration(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
        stream_name,
        &bytes,
    );
    group.id = crate::ids::native_scoped_id(stream_name, "operand-group", 90);
    record.id = crate::ids::native_scoped_id(stream_name, "record", 100);
    recipe.id = crate::ids::native_scoped_id(stream_name, "construction-recipe", recipe_at);
    let output_id = crate::ids::native_scoped_id(stream_name, "design-body-recipe-operand", 0);
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let decoded = crate::design::decode::operands::decode_body_recipe_operands(
            scan,
            &[],
            std::slice::from_ref(&group),
            std::slice::from_ref(&record),
            std::slice::from_ref(&recipe),
        )
        .unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].id, output_id);
    });
}

#[test]
fn class_367_body_recipe_operand_decodes_scale_member_frame() {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"367", 100);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&301u64.to_le_bytes());
    bytes.extend_from_slice(&33u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&103u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "53aa8ab4-194a-434b-bd52-8c6d761dc147");
    lp_utf16(&mut bytes, "8e685642-4d68-4909-96d0-0dd4437491b6");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    indexed_header(&mut bytes, *b"264", 100);
    indexed_header(&mut bytes, *b"404", 101);
    indexed_header(&mut bytes, *b"416", 102);
    indexed_header(&mut bytes, *b"424", 103);
    let recipe_at = bytes.len();
    bytes.extend_from_slice(b"body_recipe_data");
    let next_at = bytes.len();
    indexed_header(&mut bytes, *b"280", 104);

    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
            scope_record_index: 80,
            scope_reference_ordinal: 1,
            record_index: 90,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("287".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 100,
                offset: 21,
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
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::BODIES_A,
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
    let record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("367".to_owned()).unwrap(),
        record_index: 100,
    };
    let recipe = ConstructionRecipe {
        id: format!("f3d:Design/BulkStream.dat:construction-recipe#{recipe_at}"),
        byte_offset: u64_from_index(recipe_at),
        kind: ConstructionRecipeKind::Body,
        design: Some(crate::records::recipes::ConstructionRecipeDesign {
            id: crate::records::identity::RecordedValue {
                value: "301".into(),
                offset: 0,
            },
            selector: Some(crate::records::recipes::ConstructionRecipeSelector {
                value: 6,
                byte_offset: 0,
            }),
        }),
        recipe_index: 0,
        record_index: Some(crate::records::identity::RecordedValue {
            value: 0,
            offset: 0,
        }),
    };

    let operand = parse_body_recipe_operand(&bytes, &group, 0, &record, &recipe)
        .expect("class-367 body recipe operand");
    assert_eq!(operand.references().len(), 1);
    assert_eq!(operand.references()[0].design_reference, 301);
    assert_eq!(operand.references()[0].form, 33);
    assert_eq!(
        operand
            .clone()
            .into_draft()
            .selector_tail
            .map(|tail| tail.value),
        Some([1, 0, 0, 0])
    );
    assert_eq!(
        operand
            .clone()
            .into_draft()
            .selector_tail
            .map(|tail| tail.offset),
        Some(208)
    );
    assert_eq!(operand.nested_record_index(), 103);
    assert_eq!(operand.next_byte_offset(), u64_from_index(next_at));
}

#[test]
fn body_recipe_envelope_uses_its_structural_record_boundary() {
    const RECORD_INDEX: u32 = 100;
    const NEXT_AT: usize = 70_000;
    const EARLY_RECIPE_AT: usize = 100;
    const LATE_RECIPE_AT: usize = NEXT_AT - 32;

    let mut bytes = Vec::new();
    for record_index in [
        RECORD_INDEX,
        RECORD_INDEX,
        RECORD_INDEX + 1,
        RECORD_INDEX + 2,
        RECORD_INDEX + 3,
    ] {
        indexed_header(&mut bytes, *b"365", record_index);
    }
    let prologue_end = bytes.len();
    bytes.resize(NEXT_AT, 0xaa);
    for recipe_at in [EARLY_RECIPE_AT, LATE_RECIPE_AT] {
        bytes[recipe_at..recipe_at + b"body_recipe_data".len()]
            .copy_from_slice(b"body_recipe_data");
    }
    indexed_header(&mut bytes, *b"311", RECORD_INDEX + 4);

    let header = DesignRecordHeader {
        id: "stream:record-100".into(),
        record_index: RECORD_INDEX,
        class_tag: crate::records::references::DesignClassTag::try_from("365".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let early = ConstructionRecipe {
        id: "stream:recipe-early".into(),
        byte_offset: u64_from_index(EARLY_RECIPE_AT),
        kind: ConstructionRecipeKind::Body,
        design: None,
        recipe_index: 0,
        record_index: Some(crate::records::identity::RecordedValue {
            value: 0,
            offset: 0,
        }),
    };
    let late = ConstructionRecipe {
        id: "stream:recipe-late".into(),
        byte_offset: u64_from_index(LATE_RECIPE_AT),
        recipe_index: 1,
        ..early.clone()
    };

    assert_eq!(
        body_recipe_prologue_end(&bytes, 0, RECORD_INDEX),
        Some(prologue_end)
    );
    assert_eq!(
        body_recipe_operand_end(&bytes, prologue_end, RECORD_INDEX, EARLY_RECIPE_AT),
        Some((RECORD_INDEX + 4, NEXT_AT))
    );
    assert_eq!(unique_body_recipe(&bytes, &header, &[&early]), Some(&early));
    assert_eq!(unique_body_recipe(&bytes, &header, &[&early, &late]), None);
}

/// A whole-body member `100` through `103` with no record `104`, its recipe
/// record, and the indexed header `next_index` that follows `gap` bytes after
/// the family name. Returns the bytes, the family-name offset, and the header
/// offset.
fn member_without_successor(
    recipe_records: &[([u8; 3], u32)],
    gap: usize,
    next_index: u32,
) -> (Vec<u8>, usize, usize) {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"387", 100);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&361u64.to_le_bytes());
    bytes.extend_from_slice(&33u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&103u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "53aa8ab4-194a-434b-bd52-8c6d761dc147");
    lp_utf16(&mut bytes, "8e685642-4d68-4909-96d0-0dd4437491b6");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    indexed_header(&mut bytes, *b"258", 100);
    indexed_header(&mut bytes, *b"288", 101);
    indexed_header(&mut bytes, *b"421", 102);
    indexed_header(&mut bytes, *b"409", 103);
    for (class_tag, record_index) in recipe_records {
        indexed_header(&mut bytes, *class_tag, *record_index);
    }
    let recipe_at = bytes.len();
    bytes.extend_from_slice(b"body_recipe_data");
    bytes.resize(bytes.len() + gap, 0);
    let next_at = bytes.len();
    indexed_header(&mut bytes, *b"397", next_index);
    (bytes, recipe_at, next_at)
}

fn member_group() -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
            scope_record_index: 80,
            scope_reference_ordinal: 1,
            record_index: 90,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("274".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 100,
                offset: 21,
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
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::BODIES_A,
                ),
            role_offset: 0,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "258".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap()
}

fn member_header() -> DesignRecordHeader {
    DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("387".to_owned()).unwrap(),
        record_index: 100,
    }
}

fn member_recipe(recipe_at: usize) -> ConstructionRecipe {
    ConstructionRecipe {
        id: format!("f3d:Design/BulkStream.dat:construction-recipe#{recipe_at}"),
        byte_offset: u64_from_index(recipe_at),
        kind: ConstructionRecipeKind::Body,
        design: None,
        recipe_index: 0,
        record_index: Some(crate::records::identity::RecordedValue {
            value: 0,
            offset: 0,
        }),
    }
}

#[test]
fn body_recipe_operand_without_a_successor_record_closes_at_the_recipe_end() {
    let (bytes, recipe_at, next_at) = member_without_successor(&[(*b"361", 7)], 8, 1739);
    let recipe = member_recipe(recipe_at);
    let header = member_header();

    assert_eq!(
        unique_body_recipe(&bytes, &header, &[&recipe]),
        Some(&recipe)
    );
    let operand = parse_body_recipe_operand(&bytes, &member_group(), 0, &header, &recipe)
        .expect("body recipe operand without a successor record");
    assert_eq!(operand.next_record_index(), 1739);
    assert_eq!(operand.next_byte_offset(), u64_from_index(next_at));
    assert_eq!(operand.nested_record_index(), 103);

    let mut draft = operand.into_draft();
    draft.next_record_index = 103;
    assert!(
        crate::records::topology::body_recipe::DesignBodyRecipeOperand::try_new(draft).is_err()
    );
}

#[test]
fn body_recipe_operand_without_a_successor_record_refuses_another_frame() {
    let header = member_header();

    let (bytes, recipe_at, _) = member_without_successor(&[(*b"361", 7)], 12, 1739);
    let recipe = member_recipe(recipe_at);
    assert_eq!(unique_body_recipe(&bytes, &header, &[&recipe]), None);

    let (bytes, recipe_at, _) = member_without_successor(&[(*b"290", 900), (*b"361", 7)], 8, 1739);
    let recipe = member_recipe(recipe_at);
    assert_eq!(unique_body_recipe(&bytes, &header, &[&recipe]), None);

    let (bytes, recipe_at, _) = member_without_successor(&[], 8, 1739);
    let recipe = member_recipe(recipe_at);
    assert_eq!(unique_body_recipe(&bytes, &header, &[&recipe]), None);
}
