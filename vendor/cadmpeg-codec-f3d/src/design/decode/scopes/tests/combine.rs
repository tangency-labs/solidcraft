// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::combine::{
    exact_combine_operation, take_external_reference_identity,
};
use crate::design::decode::scopes::parameter_scope::{
    parse_parameter_scope, parse_parameter_scope_with_type_guid,
};
use crate::design::feature_project::project_combine;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::combine::{
    DesignCombineBodyNode, DesignCombineBodySelection, DesignCombineForm, DesignCombineOperation,
};
use crate::records::feature::scope::DesignParameterScope;
use crate::test_support::indexed_header;
use crate::test_support::lp_utf16;
use crate::test_support::push_reference_u64;

#[test]
fn external_reference_text_fields_keep_link_and_version() {
    let asset = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    let link = "component-body-link";
    let property = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
    let version = "urn:test:version:2";
    let mut bytes = vec![1];
    bytes.extend_from_slice(&7_u64.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    lp_utf16(&mut bytes, asset);
    bytes.push(0);
    lp_utf16(&mut bytes, link);
    bytes.push(1);
    lp_utf16(&mut bytes, property);
    lp_utf16(&mut bytes, version);
    let mut cursor = 0;
    let identity = take_external_reference_identity(&bytes, &mut cursor)
        .unwrap()
        .expect("admitted external reference");
    assert_eq!(cursor, bytes.len());
    assert_eq!(identity.link_name, link);
    assert_eq!(identity.version.unwrap().version_urn.value, version);
}

#[test]
fn combine_tools_keep_first_and_additional_records() {
    fn operation_record(bytes: &mut Vec<u8>, record_index: u32, selection_record_index: u32) {
        indexed_header(bytes, *b"283", record_index);
        bytes.extend_from_slice(&[0; 9]);
        bytes.push(1);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&24u32.to_le_bytes());
        bytes.extend_from_slice(b"DcFeatureOperationIdFlag");
        bytes.extend_from_slice(&23u32.to_le_bytes());
        bytes.extend_from_slice(b"IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&7u64.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&selection_record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        indexed_header(bytes, *b"259", record_index);
    }
    fn target_record(bytes: &mut Vec<u8>, record_index: u32, selection_record_index: u32) {
        indexed_header(bytes, *b"283", record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&selection_record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        indexed_header(bytes, *b"259", record_index);
    }
    let references = [91u32, 92, 93, 94, 95, 96];
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"382", 90);
    bytes.extend_from_slice(&[0; 9]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(0);
    bytes.push(1);
    bytes.extend_from_slice(&[0; 7]);
    bytes.resize(64, 0);
    bytes.extend_from_slice(
        &(u32::try_from(references.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for reference in references {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&17u32.to_le_bytes());
    lp_utf16(&mut bytes, "Combine");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&2u32.to_le_bytes());
    tail[31..35].copy_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    indexed_header(&mut bytes, *b"259", 90);
    for (ordinal, pair) in references.chunks_exact(2).enumerate() {
        if ordinal == 2 {
            target_record(&mut bytes, pair[0], pair[1]);
        } else {
            operation_record(&mut bytes, pair[0], pair[1]);
        }
        indexed_header(&mut bytes, *b"389", pair[1]);
        lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000001");
        lp_utf16(&mut bytes, "10000000-0000-0000-0000-000000000001");
        indexed_header(&mut bytes, *b"306", pair[1]);
    }
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 90,
        class_tag: crate::records::references::DesignClassTag::try_from("382".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let scope = parse_parameter_scope(
        &bytes,
        &records,
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("Combine scope");
    let operation = exact_combine_operation(&bytes, &records, &scope)
        .unwrap()
        .expect("two admitted Combine tools");
    assert_eq!(operation.tools.first.record_index, 92);
    assert_eq!(operation.tools.additional[0].record_index, 94);
}

#[test]
fn combine_scope_projects_ordered_target_tools_and_retention() {
    fn operation_record(bytes: &mut Vec<u8>, record_index: u32, selection_record_index: u32) {
        indexed_header(bytes, *b"283", record_index);
        bytes.extend_from_slice(&[0; 9]);
        bytes.push(1);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&24u32.to_le_bytes());
        bytes.extend_from_slice(b"DcFeatureOperationIdFlag");
        bytes.extend_from_slice(&23u32.to_le_bytes());
        bytes.extend_from_slice(b"IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&7u64.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&selection_record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        indexed_header(bytes, *b"259", record_index);
    }
    fn target_record(bytes: &mut Vec<u8>, record_index: u32, selection_record_index: u32) {
        indexed_header(bytes, *b"283", record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&selection_record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        indexed_header(bytes, *b"259", record_index);
    }
    fn selection_record(bytes: &mut Vec<u8>, record_index: u32, suffix: u8) {
        indexed_header(bytes, *b"389", record_index);
        lp_utf16(
            bytes,
            &format!("00000000-0000-0000-0000-0000000000{suffix:02x}"),
        );
        lp_utf16(
            bytes,
            &format!("10000000-0000-0000-0000-0000000000{suffix:02x}"),
        );
        indexed_header(bytes, *b"306", record_index);
    }

    let scope_record_index = 90u32;
    let references = [91u32, 92, 93, 94, 95, 96];
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"382", scope_record_index);
    bytes.extend_from_slice(&[0; 9]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(0);
    bytes.push(1);
    bytes.extend_from_slice(&[0; 7]);
    bytes.resize(64, 0);
    bytes.extend_from_slice(
        &(u32::try_from(references.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for reference in references {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&17u32.to_le_bytes());
    lp_utf16(&mut bytes, "Combine");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&2u32.to_le_bytes());
    tail[31..35].copy_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    indexed_header(&mut bytes, *b"259", scope_record_index);
    for (ordinal, pair) in references.chunks_exact(2).enumerate() {
        if ordinal == 2 {
            target_record(&mut bytes, pair[0], pair[1]);
        } else {
            operation_record(&mut bytes, pair[0], pair[1]);
        }
        selection_record(&mut bytes, pair[1], u8::try_from(pair[1]).unwrap());
    }

    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: scope_record_index,
        class_tag: crate::records::references::DesignClassTag::try_from("382".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let mut scope = parse_parameter_scope(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("Combine scope");
    let operation = exact_combine_operation(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
    )
    .unwrap()
    .expect("Combine construction");
    assert_eq!(
        operation,
        DesignCombineOperation {
            form: DesignCombineForm::Standard,
            operation: cadmpeg_ir::features::BooleanKind::Join,
            operation_offset: 20,
            keep_tools: true,
            keep_tools_offset: 25,
            target_record_index: 96,
            target_body_node: None,
            tools: crate::records::feature::combine::DesignCombineTools {
                first: DesignCombineBodySelection {
                    record_index: 92,
                    external_identity: None,
                    body_node: None,
                },
                additional: vec![DesignCombineBodySelection {
                    record_index: 94,
                    external_identity: None,
                    body_node: None,
                },]
            },
        }
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::Combine(slot) =
        scope.payload_mut()
    {
        *slot = Some(operation);
    }
    assert_eq!(
        project_combine(&scope, "Design1/BulkStream.dat"),
        Some(cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Combine {
                operands: cadmpeg_ir::features::CombineOperands::new(
                    cadmpeg_ir::features::BodySelection::Native(
                        "Design1/BulkStream.dat:design-record#96".into(),
                    ),
                    cadmpeg_ir::features::BodySelection::NativeSet(
                        vec![
                            "Design1/BulkStream.dat:design-record#92".into(),
                            "Design1/BulkStream.dat:design-record#94".into(),
                        ]
                        .try_into()
                        .unwrap()
                    ),
                )
                .unwrap(),

                op: cadmpeg_ir::features::BooleanKind::Join,
                keep_tools: true,
            }
        ))
    );

    let mut compact_bytes = bytes.clone();
    compact_bytes[4..7].copy_from_slice(b"387");
    compact_bytes[11..21].fill(0);
    compact_bytes[21..25].copy_from_slice(&1u32.to_le_bytes());
    compact_bytes[25] = 0;
    compact_bytes[26..29].fill(0);
    compact_bytes[29..31].copy_from_slice(&[1, 0]);
    compact_bytes[31..35].copy_from_slice(&1u32.to_le_bytes());
    compact_bytes[35] = 1;
    compact_bytes[36..44].copy_from_slice(&200u64.to_le_bytes());
    compact_bytes[44..46].fill(0);
    let mut compact_scope = scope.clone();
    compact_scope.class_tag =
        crate::records::references::DesignClassTag::try_from("387".to_owned()).unwrap();
    compact_scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
    compact_scope
        .try_edit(|draft| {
            draft.frame_length = 328;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let compact = exact_combine_operation(
        &compact_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&compact_bytes),
        &compact_scope,
    )
    .unwrap()
    .expect("compact Combine construction");
    assert_eq!(compact.operation, cadmpeg_ir::features::BooleanKind::Join);
    assert_eq!(compact.operation_offset, 21);
    assert!(!compact.keep_tools);
    assert_eq!(compact.form, DesignCombineForm::Compact);
    assert_eq!(compact.target_record_index, 96);
    assert_eq!(
        compact
            .tools
            .iter()
            .map(|tool| tool.record_index)
            .collect::<Vec<_>>(),
        [92, 94]
    );

    let mut malformed_compact_tail = compact_bytes;
    malformed_compact_tail[45] = 1;
    assert!(exact_combine_operation(
        &malformed_compact_tail,
        &crate::design::test_support::indexed_record_offsets_for_test(&malformed_compact_tail),
        &compact_scope
    )
    .unwrap()
    .is_none());
}

#[test]
fn combine_extended_reference_scope_retains_external_tool_identity() {
    fn operation_record(bytes: &mut Vec<u8>, record_index: u32, selection_record_index: u32) {
        indexed_header(bytes, *b"304", record_index);
        bytes.extend_from_slice(&[0; 9]);
        bytes.push(1);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&24u32.to_le_bytes());
        bytes.extend_from_slice(b"DcFeatureOperationIdFlag");
        bytes.extend_from_slice(&23u32.to_le_bytes());
        bytes.extend_from_slice(b"IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&7u64.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&selection_record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        indexed_header(bytes, *b"261", record_index);
    }
    fn target_record(bytes: &mut Vec<u8>, record_index: u32, selection_record_index: u32) {
        indexed_header(bytes, *b"304", record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&selection_record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        indexed_header(bytes, *b"261", record_index);
    }
    fn external_selection_record(bytes: &mut Vec<u8>, scope: u32, record_index: u32) {
        const ASSET: &str = "11111111-1111-4111-8111-111111111111";
        const CONTEXT: &str = "22222222-2222-4222-8222-222222222222";
        indexed_header(bytes, *b"312", record_index);
        bytes.extend_from_slice(&[0; 14]);
        push_reference_u64(bytes, u64::from(record_index + 3));
        bytes.extend_from_slice(&1u32.to_le_bytes());
        lp_utf16(bytes, ASSET);
        lp_utf16(bytes, CONTEXT);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        push_reference_u64(bytes, 5_001);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&6_001u64.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&7u32.to_le_bytes());
        lp_utf16(bytes, ASSET);
        bytes.push(0);
        lp_utf16(bytes, "component-body-link");
        bytes.push(1);
        lp_utf16(bytes, "33333333-3333-4333-8333-333333333333");
        lp_utf16(bytes, "urn:example:version:4");
        bytes.extend_from_slice(&9u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&11u64.to_le_bytes());
        bytes.extend_from_slice(&48u32.to_le_bytes());
        bytes.extend_from_slice(&12u64.to_le_bytes());
        push_reference_u64(bytes, u64::from(record_index + 2));
        bytes.extend_from_slice(&[0; 2]);
        push_reference_u64(bytes, u64::from(record_index + 1));
        bytes.push(0);
        push_reference_u64(bytes, u64::from(scope));
        indexed_header(bytes, *b"261", record_index);
    }
    fn simple_selection_record(bytes: &mut Vec<u8>, record_index: u32) {
        indexed_header(bytes, *b"312", record_index);
        lp_utf16(bytes, "44444444-4444-4444-8444-444444444444");
        lp_utf16(bytes, "55555555-5555-4555-8555-555555555555");
        indexed_header(bytes, *b"261", record_index);
    }

    let scope_record_index = 90u32;
    let mut bytes = vec![0; 363];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"329");
    bytes[7..11].copy_from_slice(&scope_record_index.to_le_bytes());
    bytes[29] = 1;
    bytes[30] = 1;
    bytes[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes[35] = 1;
    bytes[36..44].copy_from_slice(&700u64.to_le_bytes());
    bytes[44..46].fill(0);
    indexed_header(&mut bytes, *b"261", scope_record_index);
    operation_record(&mut bytes, 91, 92);
    external_selection_record(&mut bytes, scope_record_index, 92);
    target_record(&mut bytes, 93, 94);
    simple_selection_record(&mut bytes, 94);

    let mut scope = DesignParameterScope::empty(
        "scope",
        crate::records::feature::scope::DesignFeatureKind::Combine,
        scope_record_index,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 0;
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("329".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("261".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 363;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![91, 92, 93, 94]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let operation = exact_combine_operation(&bytes, &records, &scope)
        .unwrap()
        .expect("extended-reference Combine construction");
    assert_eq!(operation.form, DesignCombineForm::ExtendedReference);
    assert_eq!(operation.operation, cadmpeg_ir::features::BooleanKind::Cut);
    assert_eq!(operation.operation_offset, 31);
    assert!(operation.keep_tools);
    assert_eq!(operation.keep_tools_offset, 30);
    assert_eq!(operation.target_record_index, 94);
    assert!(operation.tools.additional.is_empty());
    let tool = &operation.tools.first;
    let identity = tool
        .external_identity
        .as_ref()
        .expect("cross-document body identity");
    assert_eq!(identity.occurrence_reference(), 5_001);
    assert_eq!(identity.external_body_reference(), 6_001);
    assert_eq!(identity.external_segment(), 7);
    assert_eq!(identity.external_link_name(), "component-body-link");
    assert_eq!(
        identity
            .external_version()
            .map(|version| version.property_key.value.as_str()),
        Some("33333333-3333-4333-8333-333333333333")
    );
    assert_eq!(
        identity
            .external_version()
            .map(|version| version.version_urn.value.as_str()),
        Some("urn:example:version:4")
    );
    assert_eq!(identity.tail_values(), [11, 12]);
    assert_eq!(
        identity.tail_value_offsets()[1],
        identity.tail_value_offsets()[0] + 12
    );

    let stamp_at = usize::try_from(identity.tail_value_offsets()[0]).unwrap();
    let scalar_at = usize::try_from(identity.tail_value_offsets()[1]).unwrap();
    let tail_at = stamp_at - crate::layout::combine_external_selector_tail::SELECTION_PAYLOAD_STAMP;
    let epoch_at = tail_at + crate::layout::combine_external_selector_tail::EVALUATION_EPOCH;
    for (epoch, scalar) in [(0_u32, 0.0_f64), (7, 0.375), (u32::MAX, f64::MAX)] {
        let mut changed = bytes.clone();
        changed[epoch_at..epoch_at + 4].copy_from_slice(&epoch.to_le_bytes());
        changed[scalar_at..scalar_at + 8].copy_from_slice(&scalar.to_le_bytes());
        let operation = exact_combine_operation(&changed, &records, &scope)
            .unwrap()
            .expect("complete external selector");
        let identity = operation.tools.first.external_identity.as_ref().unwrap();
        assert_eq!(identity.tail_values(), [11, scalar.to_bits()]);
        assert_eq!(
            identity.tail_value_offsets(),
            [
                u64::try_from(stamp_at).unwrap(),
                u64::try_from(scalar_at).unwrap()
            ]
        );
    }
    for scalar in [-0.375_f64, f64::INFINITY, f64::NAN] {
        let mut changed = bytes.clone();
        changed[scalar_at..scalar_at + 8].copy_from_slice(&scalar.to_le_bytes());
        let operation = exact_combine_operation(&changed, &records, &scope)
            .unwrap()
            .expect("operation retains its native selector");
        assert!(operation.tools.first.external_identity.is_none());
    }

    let mut malformed_scope = scope.clone();
    malformed_scope
        .try_edit(|draft| {
            draft.frame_length = 367;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(exact_combine_operation(&bytes, &records, &malformed_scope)
        .unwrap()
        .is_none());
    let mut malformed_reference = bytes.clone();
    malformed_reference[35] = 0;
    assert!(exact_combine_operation(
        &malformed_reference,
        &crate::design::test_support::indexed_record_offsets_for_test(&malformed_reference),
        &scope
    )
    .unwrap()
    .is_none());

    let mut malformed_external_asset = bytes;
    malformed_external_asset[usize::try_from(identity.external_asset_id_offset()).unwrap()] = b'g';
    let operation = exact_combine_operation(
        &malformed_external_asset,
        &crate::design::test_support::indexed_record_offsets_for_test(&malformed_external_asset),
        &scope,
    )
    .unwrap()
    .expect("operation remains exact when only the optional external identity is malformed");
    assert!(operation.tools.first.external_identity.is_none());
}

/// Operand records of one operand-list `Combine` scope.
struct OperandListFixture {
    operation: u32,
    keep_tools: u8,
    /// Operation-identity record and selection record, in table order.
    tools: Vec<(u32, u32)>,
    target: (u32, u32),
    target_position: usize,
    listed_tools: Option<Vec<u32>>,
    listed_target: Option<u32>,
    consumed: Option<Vec<u32>>,
    guid_units: u32,
    guid_tail_zeros: usize,
    node_flag: u8,
    node_entries: usize,
    node_trailing_bytes: usize,
}

impl OperandListFixture {
    fn new(operation: u32, keep_tools: u8) -> Self {
        Self {
            operation,
            keep_tools,
            tools: vec![(91, 92)],
            target: (93, 94),
            target_position: 1,
            listed_tools: None,
            listed_target: None,
            consumed: None,
            guid_units: 36,
            guid_tail_zeros: 3,
            node_flag: 1,
            node_entries: 1,
            node_trailing_bytes: 0,
        }
    }

    fn two_tools(mut self) -> Self {
        self.tools = vec![(91, 92), (93, 94)];
        self.target = (95, 96);
        self.target_position = 2;
        self
    }

    fn table(&self) -> Vec<(u32, u32)> {
        let mut table = self.tools.clone();
        table.insert(self.target_position, self.target);
        table
    }

    fn bytes(&self) -> Vec<u8> {
        fn marked(bytes: &mut Vec<u8>, record: u32) {
            bytes.push(1);
            bytes.extend_from_slice(&record.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }
        fn marked_list(bytes: &mut Vec<u8>, records: &[u32]) {
            bytes.extend_from_slice(&(u32::try_from(records.len()).unwrap()).to_le_bytes());
            for record in records {
                marked(bytes, *record);
            }
        }
        let tool_operations: Vec<u32> =
            self.tools.iter().map(|(operation, _)| *operation).collect();
        let consumed = self.consumed.clone().unwrap_or_else(|| {
            if self.keep_tools == 0 {
                self.tools
                    .iter()
                    .map(|(_, selection)| selection + 100)
                    .collect()
            } else {
                Vec::new()
            }
        });
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"412", 90);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&self.operation.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.push(self.keep_tools);
        bytes.push(0);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        marked(&mut bytes, 377);
        bytes.extend_from_slice(&306u64.to_le_bytes());
        marked_list(
            &mut bytes,
            self.listed_tools.as_deref().unwrap_or(&tool_operations),
        );
        bytes.extend_from_slice(&[0; 8]);
        marked_list(&mut bytes, &consumed);
        marked_list(&mut bytes, &[self.listed_target.unwrap_or(self.target.0)]);
        bytes.extend_from_slice(&self.guid_units.to_le_bytes());
        for unit in "00000000-0000-0000-0000-000000000000"
            .bytes()
            .take(usize::try_from(self.guid_units).unwrap())
        {
            bytes.extend_from_slice(&[unit, 0]);
        }
        bytes.resize(bytes.len() + self.guid_tail_zeros, 0);
        let table = self.table();
        bytes.extend_from_slice(&(u32::try_from(table.len() * 2).unwrap()).to_le_bytes());
        for (operation, selection) in &table {
            marked(&mut bytes, *operation);
            marked(&mut bytes, *selection);
        }
        bytes.extend_from_slice(&5u32.to_le_bytes());
        lp_utf16(&mut bytes, "Combiner");
        let mut tail = [0; 78];
        tail[0..4].copy_from_slice(&2u32.to_le_bytes());
        tail[31..35].copy_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        indexed_header(&mut bytes, *b"258", 90);
        for (operation, selection) in &table {
            indexed_header(&mut bytes, *b"342", *operation);
            if (*operation, *selection) == self.target {
                bytes.extend_from_slice(&[0; 10]);
                bytes.extend_from_slice(&1u32.to_le_bytes());
            } else {
                bytes.extend_from_slice(&[0; 9]);
                bytes.push(1);
                bytes.extend_from_slice(&1u32.to_le_bytes());
                bytes.extend_from_slice(&24u32.to_le_bytes());
                bytes.extend_from_slice(b"DcFeatureOperationIdFlag");
                bytes.extend_from_slice(&23u32.to_le_bytes());
                bytes.extend_from_slice(b"IntrinsicMetaTypeuint64");
                bytes.extend_from_slice(&306u64.to_le_bytes());
                bytes.extend_from_slice(&1u32.to_le_bytes());
            }
            marked(&mut bytes, *selection);
            indexed_header(&mut bytes, *b"258", *operation);
            indexed_header(&mut bytes, *b"366", *selection);
            bytes.extend_from_slice(&[0; 10]);
            bytes.extend_from_slice(&1u32.to_le_bytes());
            bytes.extend_from_slice(&304u64.to_le_bytes());
            bytes.extend_from_slice(&33u32.to_le_bytes());
            marked(&mut bytes, selection + 3);
            bytes.extend_from_slice(&1u32.to_le_bytes());
            lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000001");
            lp_utf16(&mut bytes, "10000000-0000-0000-0000-000000000001");
            bytes.extend_from_slice(&2u32.to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes());
            marked_list(&mut bytes, &[selection + 50]);
            let node = selection + 100;
            marked_list(&mut bytes, &vec![node; self.node_entries]);
            bytes.extend_from_slice(&9u32.to_le_bytes());
            indexed_header(&mut bytes, *b"258", *selection);
            indexed_header(&mut bytes, *b"304", node);
            bytes.extend_from_slice(&[0; 10]);
            lp_utf16(&mut bytes, "20000000-0000-0000-0000-000000000001");
            bytes.push(1);
            bytes.push(self.node_flag);
            marked(&mut bytes, selection + 150);
            marked(&mut bytes, selection + 200);
            bytes.resize(bytes.len() + self.node_trailing_bytes, 0);
            indexed_header(&mut bytes, *b"258", node);
        }
        bytes
    }

    fn decode(&self) -> Option<DesignCombineOperation> {
        decode_combine_frame(&self.bytes())
    }
}

fn decode_combine_frame(bytes: &[u8]) -> Option<DesignCombineOperation> {
    let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
    let class_tag = crate::records::references::DesignClassTag::try_from("412".to_owned()).unwrap();
    let scope = parse_parameter_scope_with_type_guid(
        bytes,
        &records,
        90,
        &class_tag,
        0,
        Some("2A94257F-2020-4B19-9A68-103A2672F1B7"),
    )
    .unwrap()
    .expect("Combine scope");
    exact_combine_operation(bytes, &records, &scope).unwrap()
}

#[test]
fn operand_list_prologue_reads_zero_based_operation_and_keep_tools() {
    use cadmpeg_ir::features::BooleanKind;
    for (code, expected) in [
        (0, BooleanKind::Join),
        (1, BooleanKind::Cut),
        (2, BooleanKind::Intersect),
    ] {
        for (keep_tools, expected_keep) in [(0, false), (1, true)] {
            let operation = OperandListFixture::new(code, keep_tools)
                .decode()
                .expect("operand-list Combine");
            assert_eq!(operation.form, DesignCombineForm::OperandLists);
            assert_eq!(operation.operation, expected);
            assert_eq!(operation.operation_offset, 21);
            assert_eq!(operation.keep_tools, expected_keep);
            assert_eq!(operation.keep_tools_offset, 29);
            assert_eq!(operation.target_record_index, 94);
            assert_eq!(operation.tools.first.record_index, 92);
        }
    }
    assert_eq!(OperandListFixture::new(3, 0).decode(), None);
    assert_eq!(OperandListFixture::new(0, 2).decode(), None);
}

#[test]
fn operand_list_prologue_takes_its_target_from_any_table_position() {
    for target_position in [0, 1, 2] {
        let mut fixture = OperandListFixture::new(0, 0).two_tools();
        fixture.target_position = target_position;
        let operation = fixture.decode().expect("operand-list Combine");
        assert_eq!(operation.target_record_index, 96);
        assert_eq!(operation.tools.first.record_index, 92);
        assert_eq!(operation.tools.additional[0].record_index, 94);
    }
}

#[test]
fn operand_list_prologue_requires_every_list_to_agree_with_the_table() {
    let two_tools = || OperandListFixture::new(0, 0).two_tools();
    let mut one_listed_tool = two_tools();
    one_listed_tool.listed_tools = Some(vec![91]);
    assert_eq!(one_listed_tool.decode(), None);

    let mut reordered_tools = two_tools();
    reordered_tools.listed_tools = Some(vec![93, 91]);
    assert_eq!(reordered_tools.decode(), None);

    let mut one_valid_of_two = two_tools();
    one_valid_of_two.listed_tools = Some(vec![91, 95]);
    assert_eq!(one_valid_of_two.decode(), None);

    let mut target_names_tool = two_tools();
    target_names_tool.listed_target = Some(91);
    assert_eq!(target_names_tool.decode(), None);

    let mut one_consumed_of_two = two_tools();
    one_consumed_of_two.consumed = Some(vec![192]);
    assert_eq!(one_consumed_of_two.decode(), None);

    let mut kept_tool_consumed = OperandListFixture::new(0, 1);
    kept_tool_consumed.consumed = Some(vec![192]);
    assert_eq!(kept_tool_consumed.decode(), None);

    let mut unkept_tool_not_consumed = OperandListFixture::new(0, 0);
    unkept_tool_not_consumed.consumed = Some(Vec::new());
    assert_eq!(unkept_tool_not_consumed.decode(), None);

    let mut short_guid = OperandListFixture::new(0, 0);
    short_guid.guid_units = 35;
    assert_eq!(short_guid.decode(), None);

    let mut gap_before_table = OperandListFixture::new(0, 0);
    gap_before_table.guid_tail_zeros = 4;
    assert_eq!(gap_before_table.decode(), None);
}

#[test]
fn operand_list_prologue_rejects_nonzero_fixed_fields() {
    for (offset, value) in [
        (11, 1),
        (19, 1),
        (25, 1),
        (28, 1),
        (30, 1),
        (31, 2),
        (35, 0),
        (44, 1),
        (45, 1),
    ] {
        let mut bytes = OperandListFixture::new(1, 0).bytes();
        bytes[offset] = value;
        assert_eq!(decode_combine_frame(&bytes), None, "offset {offset}");
    }
    let mut bytes = OperandListFixture::new(1, 0).bytes();
    bytes[36..44].fill(0);
    assert_eq!(decode_combine_frame(&bytes), None);
}

#[test]
fn operand_list_prologue_admits_any_operation_identity_value() {
    let mut bytes = OperandListFixture::new(1, 0).bytes();
    bytes[46..54].copy_from_slice(&0x1234u64.to_le_bytes());
    assert_eq!(
        decode_combine_frame(&bytes).map(|operation| operation.form),
        Some(DesignCombineForm::OperandLists)
    );
}

#[test]
fn operand_list_selections_name_their_browser_nodes() {
    let node = |selection: u32| DesignCombineBodyNode {
        record_index: selection + 100,
        body_entity_suffix: selection + 150,
        creating_scope_record_index: selection + 200,
    };
    for node_flag in [0, 1] {
        let mut fixture = OperandListFixture::new(0, 0).two_tools();
        fixture.node_flag = node_flag;
        let operation = fixture.decode().expect("operand-list Combine");
        assert_eq!(operation.target_body_node, Some(node(96)));
        assert_eq!(operation.tools.first.body_node, Some(node(92)));
        assert_eq!(operation.tools.additional[0].body_node, Some(node(94)));
    }
}

#[test]
fn operand_list_keeps_its_operation_when_a_browser_node_is_not_exact() {
    let mut invalid_flag = OperandListFixture::new(1, 0);
    invalid_flag.node_flag = 2;
    let mut two_entries = OperandListFixture::new(1, 0);
    two_entries.node_entries = 2;
    let mut no_entry = OperandListFixture::new(1, 0);
    no_entry.node_entries = 0;
    let mut trailing_bytes = OperandListFixture::new(1, 0);
    trailing_bytes.node_trailing_bytes = 1;
    for fixture in [invalid_flag, two_entries, no_entry, trailing_bytes] {
        let operation = fixture.decode().expect("operand-list Combine");
        assert_eq!(operation.operation, cadmpeg_ir::features::BooleanKind::Cut);
        assert_eq!(operation.target_body_node, None);
        assert_eq!(operation.tools.first.body_node, None);
    }
}

#[test]
fn nonzero_byte_twenty_is_not_the_operand_list_prologue() {
    let mut bytes = OperandListFixture::new(0, 0).bytes();
    bytes[20] = 1;
    assert_eq!(decode_combine_frame(&bytes), None);
}
