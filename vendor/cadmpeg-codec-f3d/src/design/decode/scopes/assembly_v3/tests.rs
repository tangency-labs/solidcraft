// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::records::feature::scope::DesignScopePayloadMut;
use crate::records::identity::ReferenceRun;
use crate::records::parameters::DesignParameterOwnerWire;
use crate::test_support::indexed_header;

const OTHER_GUID: &str = "11111111-1111-1111-1111-111111111111";
const EPS_ALIGNMENT: f64 = 1e-12;

fn reference(bytes: &mut Vec<u8>, index: u32, typed: bool) {
    bytes.push(1);
    bytes.extend(u64::from(index).to_le_bytes());
    if typed {
        bytes.extend(36_u32.to_le_bytes());
        bytes.extend(if index == 29 {
            "7F5A0426-1D2A-4DC4-9F06-58631F69D2F5".as_bytes()
        } else {
            OTHER_GUID.as_bytes()
        });
    }
    bytes.extend([0; 2]);
}

fn versioned_fixture(
    version: u32,
    reference_count: u32,
    typed: bool,
    expanded: bool,
    identity: bool,
    first_owner: u32,
) -> (
    Vec<u8>,
    DesignParameterScope,
    Vec<DesignParameterOwner>,
    StreamTypes<'static>,
) {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, if version == 5 { *b"332" } else { *b"499" }, 30);
    bytes.extend([0; 9]);
    bytes.push(u8::from(expanded));
    if expanded {
        bytes.extend([0; 4]);
    }
    bytes.extend([1, 0, 0]);
    if version == 5 {
        bytes.extend([0; 9]);
    }
    for (ordinal, index) in [10, 20].into_iter().enumerate() {
        reference(&mut bytes, index, typed);
        if version == 5 {
            bytes.extend([0; 6]);
        }
        let omitted = identity && ordinal == 0;
        bytes.push(u8::from(omitted));
        if !omitted {
            let matrix = if ordinal == 0 {
                [
                    [1.0_f64, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ]
            } else {
                [
                    [0.0, 1.0, 0.0, 3.0],
                    [1.0, 0.0, 0.0, -2.0],
                    [0.0, 0.0, -1.0, 4.0],
                    [0.0, 0.0, 0.0, 1.0],
                ]
            };
            for value in matrix.into_iter().flatten() {
                bytes.extend(value.to_le_bytes());
            }
        }
        if version == 5 {
            bytes.extend(crate::bytes::lp_utf16_bytes(OTHER_GUID).unwrap());
        }
    }
    bytes.extend([0; 4]);
    for lane in [0, 3, 1, 2] {
        reference(&mut bytes, 100 + 3 * (first_owner + lane), typed);
        if version == 5 && lane == 0 {
            bytes.extend([0; 2]);
        }
    }
    bytes.extend([0; 6]);
    bytes.extend(2_u32.to_le_bytes());
    reference(&mut bytes, 40, typed);
    reference(&mut bytes, 50, typed);
    reference(&mut bytes, 29, typed);
    bytes.extend(crate::bytes::lp_utf16_bytes(OTHER_GUID).unwrap());
    bytes.extend([0; 3]);
    let reference_count_offset = bytes.len();
    bytes.extend(reference_count.to_le_bytes());
    let mut refs = Vec::new();
    for ordinal in 0..reference_count {
        let index = 100 + 3 * ordinal;
        refs.push(Located {
            value: index,
            offset: u64_from_index(bytes.len() + 1),
        });
        reference(&mut bytes, index, typed);
    }
    bytes.extend(0_u32.to_le_bytes());
    let kind_offset = bytes.len() + 4;
    bytes.extend(crate::bytes::lp_utf16_bytes("Assemble").unwrap());
    let feature_ordinal_offset = bytes.len();
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend([0; 15]);
    reference(&mut bytes, 60, typed);
    if version == 5 {
        reference(&mut bytes, 63, typed);
    }
    let separator = usize::from(!typed && version == 3);
    bytes.extend(std::iter::repeat_n(0, separator));
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(7_u32.to_le_bytes());
    bytes.extend(0_f64.to_le_bytes());
    bytes.extend(9_u32.to_le_bytes());
    reference(&mut bytes, 61, typed);
    if version == 5 {
        bytes.extend([1, 1, 0]);
    } else {
        bytes.extend([0, 0]);
    }
    reference(&mut bytes, 62, typed);
    bytes.extend([0; 2]);
    let end = bytes.len();
    indexed_header(&mut bytes, if version == 5 { *b"264" } else { *b"498" }, 30);
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Assemble,
        30,
    );
    scope.class_tag = if version == 5 { "332" } else { "499" }
        .to_owned()
        .try_into()
        .unwrap();
    scope.paired_class_tag = if version == 5 { "264" } else { "498" }
        .to_owned()
        .try_into()
        .unwrap();
    scope
        .try_edit(|draft| {
            draft.byte_offset = 0;
            draft.frame_length = u64_from_index(end);
            draft.paired_byte_offset = u64_from_index(end);
            draft.reference_count_offset = u64_from_index(reference_count_offset);
            draft.reference_members = ReferenceRun::located(refs);
            draft.kind_offset = u64_from_index(kind_offset);
            draft.feature_ordinal_offset = u64_from_index(feature_ordinal_offset);
        })
        .unwrap();
    let owners = (0..if version == 5 { 4 } else { 12 })
        .map(|ordinal| {
            let index = 100 + 3 * ordinal;
            let evaluated_value = if ordinal == first_owner {
                std::f64::consts::FRAC_PI_2
            } else if ordinal == first_owner + 1 {
                2.0
            } else if ordinal == first_owner + 2 {
                -3.0
            } else if ordinal == first_owner + 3 {
                4.0
            } else {
                100.0
            };
            DesignParameterOwner::try_from(DesignParameterOwnerWire {
                id: format!("f3d:Design/BulkStream.dat:design-parameter-owner#{index}"),
                byte_offset: u64::from(index) * 200,
                frame_length: 104,
                class_tag: "457".to_owned().try_into().unwrap(),
                record_index: index,
                scope_record_index: 30,
                local_ordinal: ordinal,
                evaluated_value,
                evaluated_value_offset: u64::from(index) * 200 + 40,
                parameter_record_index: index + 1,
                owned_ordinal: ordinal,
                variant: Some(0),
                companion_record_index: index + 2,
            })
            .unwrap()
        })
        .collect();
    let mut types = (1..200)
        .map(|index| (index, (OTHER_GUID, 0)))
        .collect::<StreamTypes<'_>>();
    types.insert(
        29,
        (
            "7F5A0426-1D2A-4DC4-9F06-58631F69D2F5",
            if version == 5 { 6 } else { 4 },
        ),
    );
    types.insert(30, (TYPE_GUID, version));
    (bytes, scope, owners, types)
}

fn fixture(
    typed: bool,
    expanded: bool,
    identity: bool,
    first_owner: u32,
) -> (
    Vec<u8>,
    DesignParameterScope,
    Vec<DesignParameterOwner>,
    StreamTypes<'static>,
) {
    versioned_fixture(3, 12, typed, expanded, identity, first_owner)
}

#[test]
fn version_five_grammar_selects_lanes_with_variable_reference_counts() {
    for count in [4, 7, 11, 14, 16] {
        for expanded in [false, true] {
            let (bytes, scope, owners, types) =
                versioned_fixture(5, count, false, expanded, false, 0);
            let grammar = parse(&bytes, &scope, &types).unwrap();
            assert_eq!(grammar.owners, [100, 109, 103, 106]);
            let expected_span = 377 + (u64::from(count) + 12) * 11 + 256 + 4 * u64::from(expanded);
            assert_eq!(scope.frame_length(), expected_span);
            let decoded = super::super::assembly_alignment::exact_assembly_alignment_with_types(
                &bytes,
                &IndexedRecordOffsets::build(&bytes),
                &scope,
                &owners,
                &types,
            )
            .unwrap()
            .unwrap();
            assert!((decoded.angle() - std::f64::consts::FRAC_PI_2).abs() < EPS_ALIGNMENT);
            assert_eq!(decoded.offset(), [2.0, -3.0, 4.0]);
            assert_eq!(decoded.frame_sense, Some(DesignJointFrameSense::Opposed));
            assert!(valid_alignment(&scope, &decoded, &types, &owners));
            assert_eq!(
                relation_only(&scope, &decoded, &types),
                expected_span != u64_from_index(five::LEN)
            );
            let mut damaged = decoded.clone();
            if let Some(DesignAssemblyAlignmentForm::Frames { frames }) = damaged.form.as_mut() {
                frames[1].transform_offset += 1;
            }
            assert!(!valid_alignment(&scope, &damaged, &types, &owners));
        }
    }
}

#[test]
fn version_five_grammar_refuses_wrong_version_truncation_and_unproved_extensions() {
    let (bytes, scope, owners, mut types) = versioned_fixture(5, 7, false, true, false, 0);
    types.insert(30, (TYPE_GUID, 4));
    assert!(parse(&bytes, &scope, &types).is_none());
    types.insert(30, (TYPE_GUID, 3));
    assert!(parse(&bytes, &scope, &types).is_none());
    types.insert(30, (TYPE_GUID, 5));
    assert!(parse(&bytes[..bytes.len() - 12], &scope, &types).is_none());
    for offset in [28, 48, 496, 183] {
        let mut damaged = bytes.clone();
        damaged[offset] = 2;
        assert!(parse(&damaged, &scope, &types).is_none());
    }
    let mut extra_owners = owners.clone();
    let (_, _, additional, _) = fixture(false, true, false, 4);
    extra_owners.push(additional[4].clone());
    assert!(alignment(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &extra_owners,
        &types
    )
    .unwrap()
    .is_none());
    let decoded = alignment(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &owners,
        &types,
    )
    .unwrap()
    .unwrap();
    assert!(!valid_alignment(&scope, &decoded, &types, &extra_owners));
    let mut extended = bytes.clone();
    let end = usize::try_from(scope.paired_byte_offset()).unwrap();
    extended.splice(end..end, [0; 22]);
    let mut extended_scope = scope;
    extended_scope
        .try_edit(|draft| {
            draft.paired_byte_offset += 22;
            draft.frame_length += 22;
        })
        .unwrap();
    assert!(parse(&extended, &extended_scope, &types).is_none());
    assert!(!valid_alignment(&extended_scope, &decoded, &types, &owners));
}

#[test]
fn version_three_grammar_consumes_compact_typed_and_identity_forms() {
    for (typed, expanded, identity) in [
        (false, false, false),
        (false, true, false),
        (true, true, false),
        (false, false, true),
        (true, true, true),
    ] {
        let (bytes, scope, _, types) = fixture(typed, expanded, identity, 8);
        let grammar = parse(&bytes, &scope, &types).unwrap();
        assert_eq!(grammar.stride, if typed { 51 } else { 11 });
        assert_eq!(grammar.owners, [124, 133, 127, 130]);
        assert_eq!(grammar.facing, DesignJointFrameSense::Opposed);
        if identity {
            assert_eq!(
                grammar.frames[0].transform_offset,
                grammar.frames[0].reference_offset + u64_from_index(grammar.stride - 1)
            );
        }
    }
}

#[test]
fn version_three_decoder_selects_owner_references_and_connector_facing() {
    for first_owner in [4, 8] {
        let (bytes, scope, owners, types) = fixture(false, true, true, first_owner);
        let decoded = super::super::assembly_alignment::exact_assembly_alignment_with_types(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &scope,
            &owners,
            &types,
        )
        .unwrap()
        .unwrap();
        assert!((decoded.angle() - std::f64::consts::FRAC_PI_2).abs() < EPS_ALIGNMENT);
        assert_eq!(decoded.offset(), [2.0, -3.0, 4.0]);
        assert_eq!(decoded.frame_sense, Some(DesignJointFrameSense::Opposed));
        assert_eq!(decoded.owners[0].value, 100 + 3 * first_owner);
        assert_eq!(
            decoded.operand_frames().unwrap()[0].reference_record_index,
            10
        );
        let mut decoded_scope = scope.clone();
        if let DesignScopePayloadMut::Assemble(slot) = decoded_scope.payload_mut() {
            *slot = Some(decoded);
        }
        assert!(valid_alignment(
            &decoded_scope,
            decoded_scope.assembly_alignment().unwrap(),
            &types,
            &owners
        ));
    }
}

#[test]
fn version_three_grammar_refuses_wrong_types_versions_and_truncated_spans() {
    let (bytes, scope, _, mut types) = fixture(false, false, false, 4);
    types.insert(30, (OTHER_GUID, 3));
    assert!(parse(&bytes, &scope, &types).is_none());
    types.insert(30, (TYPE_GUID, 4));
    assert!(parse(&bytes, &scope, &types).is_none());
    types.insert(29, ("7F5A0426-1D2A-4DC4-9F06-58631F69D2F5", 4));
    types.insert(30, (TYPE_GUID, 3));
    assert!(parse(&bytes[..bytes.len() - 12], &scope, &types).is_none());
    let mut damaged = bytes;
    damaged[20] = 2;
    assert!(parse(&damaged, &scope, &types).is_none());
}

fn append_paths(bytes: &mut Vec<u8>, types: &mut StreamTypes<'static>) {
    append_locator_paths(bytes, types, None);
}

fn append_locator_paths(
    bytes: &mut Vec<u8>,
    types: &mut StreamTypes<'static>,
    wide: Option<(bool, u32)>,
) {
    for locator in [40, 50] {
        indexed_header(bytes, *b"497", locator);
        if let Some((null, word)) = wide {
            bytes.extend([0; 28]);
            bytes.extend(word.to_le_bytes());
            bytes.extend([0; 13]);
            if null {
                bytes.push(0);
            } else {
                reference(bytes, 10, false);
            }
            bytes.push(0);
            for row in 0..4 {
                for column in 0..4 {
                    bytes.extend(f64::from(u8::from(row == column)).to_le_bytes());
                }
            }
            bytes.push(0);
        } else {
            bytes.extend([0; 10]);
            reference(bytes, 10, false);
            bytes.extend([1, 0]);
        }
        reference(bytes, 30, false);
        reference(bytes, locator + 2, false);
        bytes.extend(2_u32.to_le_bytes());
        bytes.extend([0; 2]);
        indexed_header(bytes, *b"496", locator + 1);
        bytes.extend([0; 10]);
        bytes.extend(1_u32.to_le_bytes());
        bytes.extend(crate::bytes::lp_utf16_bytes(OTHER_GUID).unwrap());
        for _ in 0..2 {
            bytes.extend(crate::bytes::lp_utf16_bytes(OTHER_GUID).unwrap());
        }
        bytes.extend(2_u64.to_le_bytes());
        for _ in 0..2 {
            bytes.extend(crate::bytes::lp_utf16_bytes(OTHER_GUID).unwrap());
        }
        bytes.extend(2_u32.to_le_bytes());
        indexed_header(bytes, *b"495", locator + 2);
        bytes.extend([0; 10]);
        bytes.push(1);
        bytes.extend(1_u32.to_le_bytes());
        reference(bytes, locator + 1, false);
        types.insert(
            u64::from(locator),
            (
                super::super::assembly_operand_paths::WIDE_PATH_LOCATOR_TYPE_GUID,
                if wide.is_some() { 5 } else { 3 },
            ),
        );
        types.insert(
            u64::from(locator + 1),
            (
                crate::records::feature::assembly::TYPED_OCCURRENCE_PATH_TYPE_GUID,
                4,
            ),
        );
    }
    indexed_header(bytes, *b"494", 200);
}

#[test]
fn version_three_decoder_keeps_qualified_scope_connectors_and_reports_partial_binding() {
    let (mut bytes, mut scope, owners, mut types) = fixture(false, true, true, 8);
    append_paths(&mut bytes, &mut types);
    let records = IndexedRecordOffsets::build(&bytes);
    let decoded = super::super::assembly_alignment::exact_assembly_alignment_with_types(
        &bytes, &records, &scope, &owners, &types,
    )
    .unwrap()
    .unwrap();
    assert!(matches!(
        decoded.form,
        Some(DesignAssemblyAlignmentForm::Qualified(_))
    ));
    assert!(valid_alignment(&scope, &decoded, &types, &owners));
    assert_eq!(decoded.offset(), [2.0, -3.0, 4.0]);
    assert_eq!(decoded.frame_sense, Some(DesignJointFrameSense::Opposed));
    let expected: crate::records::sketch_placement::SketchPlacementMatrix = [
        [0.0, 1.0, 0.0, 3.0],
        [1.0, 0.0, 0.0, -2.0],
        [0.0, 0.0, -1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
    .try_into()
    .unwrap();
    assert_eq!(decoded.operand_frames().unwrap()[1].transform, expected);
    let mut changed = decoded.clone();
    if let Some(DesignAssemblyAlignmentForm::Qualified(operands)) = changed.form.as_mut() {
        operands[0].frame.transform_offset += 1;
    }
    assert!(!valid_alignment(&scope, &changed, &types, &owners));
    let grammar = parse(&bytes, &scope, &types).unwrap();
    let joint_at = grammar.locator_offsets[1] + grammar.stride;
    bytes[joint_at + 1..joint_at + 9].copy_from_slice(&28_u64.to_le_bytes());
    types.insert(28, ("7F5A0426-1D2A-4DC4-9F06-58631F69D2F5", 4));
    let partial = super::super::assembly_alignment::exact_assembly_alignment_with_types(
        &bytes, &records, &scope, &owners, &types,
    )
    .unwrap()
    .unwrap();
    assert!(matches!(
        partial.form,
        Some(DesignAssemblyAlignmentForm::Frames { .. })
    ));
    if let DesignScopePayloadMut::Assemble(slot) = scope.payload_mut() {
        *slot = Some(partial);
    }
    let projection = crate::design::assembly::project_assembly_joints(
        &[scope],
        &[],
        &crate::design::components::ComponentInserts::decide(&[], &[], &[]),
        &[],
    );
    assert!(projection.joints.is_empty());
    assert!(projection
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::AssemblyAlignmentPartial.kind()));
}

#[test]
fn version_three_nonzero_tail_state_retains_partial_alignment() {
    for state in [[0, 1], [1, 0], [1, 1]] {
        let (mut bytes, mut scope, owners, mut types) = fixture(false, false, false, 4);
        append_paths(&mut bytes, &mut types);
        let records = IndexedRecordOffsets::build(&bytes);
        assert!(matches!(
            alignment(&bytes, &records, &scope, &owners, &types)
                .unwrap()
                .unwrap()
                .form,
            Some(DesignAssemblyAlignmentForm::Qualified(_))
        ));
        let end = usize::try_from(scope.paired_byte_offset()).unwrap();
        let state_at = end - 11 - 4;
        bytes[state_at..state_at + 2].copy_from_slice(&state);
        assert_eq!(parse(&bytes, &scope, &types).unwrap().tail_state, state);
        let records = IndexedRecordOffsets::build(&bytes);
        let decoded = alignment(&bytes, &records, &scope, &owners, &types)
            .unwrap()
            .unwrap();
        assert!(matches!(
            decoded.form,
            Some(DesignAssemblyAlignmentForm::Frames { .. })
        ));
        if let DesignScopePayloadMut::Assemble(slot) = scope.payload_mut() {
            *slot = Some(decoded);
        }
        let projection = crate::design::assembly::project_assembly_joints(
            &[scope],
            &[],
            &crate::design::components::ComponentInserts::decide(&[], &[], &[]),
            &[],
        );
        assert!(projection.joints.is_empty());
        assert!(projection
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::AssemblyAlignmentPartial.kind()));
    }
}

#[test]
fn version_five_decoder_keeps_connector_frames_with_qualified_paths() {
    let (mut bytes, scope, owners, mut types) = versioned_fixture(5, 7, false, false, false, 0);
    append_paths(&mut bytes, &mut types);
    let decoded = super::super::assembly_alignment::exact_assembly_alignment_with_types(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &owners,
        &types,
    )
    .unwrap()
    .unwrap();
    assert!(matches!(
        decoded.form,
        Some(DesignAssemblyAlignmentForm::Qualified(_))
    ));
    assert!(valid_alignment(&scope, &decoded, &types, &owners));
    assert_eq!(
        std::array::from_fn::<_, 3, _>(
            |axis| decoded.operand_frames().unwrap()[1].transform[axis][3]
        ),
        [3.0, -2.0, 4.0]
    );
}

#[test]
fn version_five_compact_identity_at_legacy_length_remains_a_relation() {
    let (bytes, scope, owners, types) = versioned_fixture(5, 28, false, false, true, 0);
    assert_eq!(scope.frame_length(), u64_from_index(five::LEN));
    let decoded = alignment(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &owners,
        &types,
    )
    .unwrap()
    .unwrap();
    assert!(valid_alignment(&scope, &decoded, &types, &owners));
    assert!(relation_only(&scope, &decoded, &types));
}

#[test]
fn version_five_locator_matrix_forms_qualify_validate_and_project() {
    for (null, word) in [(true, 2), (true, 3), (false, 2), (false, 3)] {
        let (mut bytes, mut scope, owners, mut types) =
            versioned_fixture(5, 7, false, false, false, 0);
        append_locator_paths(&mut bytes, &mut types, Some((null, word)));
        let decoded = alignment(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &scope,
            &owners,
            &types,
        )
        .unwrap()
        .unwrap();
        assert!(matches!(
            decoded.form,
            Some(DesignAssemblyAlignmentForm::Qualified(_))
        ));
        assert!(valid_alignment(&scope, &decoded, &types, &owners));
        let frames = decoded.operand_frames().unwrap();
        assert_eq!(frames[0].reference_record_index, 10);
        assert_eq!(frames[1].reference_record_index, 20);
        assert_eq!(frames[1].transform[0][3], 3.0);
        if let DesignScopePayloadMut::Assemble(slot) = scope.payload_mut() {
            *slot = Some(decoded);
        }
        let projected = crate::design::assembly::project_assembly_joints(
            &[scope],
            &[],
            &crate::design::components::ComponentInserts::default(),
            &[],
        );
        assert_eq!(projected.joints.len(), 1);
    }
}

#[test]
fn version_five_locator_wrong_word_and_span_keep_partial_alignment() {
    for wrong_span in [false, true] {
        let (mut bytes, mut scope, owners, mut types) =
            versioned_fixture(5, 7, false, false, false, 0);
        let locator_at = bytes.len();
        append_locator_paths(
            &mut bytes,
            &mut types,
            Some((true, if wrong_span { 2 } else { 4 })),
        );
        if wrong_span {
            bytes.insert(locator_at + 215, 0);
        }
        let decoded = alignment(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &scope,
            &owners,
            &types,
        )
        .unwrap()
        .unwrap();
        assert!(matches!(
            decoded.form,
            Some(DesignAssemblyAlignmentForm::Frames { .. })
        ));
        assert!(valid_alignment(&scope, &decoded, &types, &owners));
        if let DesignScopePayloadMut::Assemble(slot) = scope.payload_mut() {
            *slot = Some(decoded);
        }
        let projection = crate::design::assembly::project_assembly_joints(
            &[scope],
            &[],
            &crate::design::components::ComponentInserts::default(),
            &[],
        );
        assert!(projection.joints.is_empty());
        assert_eq!(
            projection
                .losses
                .iter()
                .filter(
                    |loss| loss.code == crate::loss::F3dLossCode::AssemblyAlignmentPartial.kind()
                )
                .count(),
            1
        );
    }
}
