// SPDX-License-Identifier: Apache-2.0

use super::{exact_assembly_operand_path, exact_assembly_operand_path_envelope};
use crate::records::feature::assembly::DesignAssemblyOperandPathLink;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};

const GUID: &str = "11111111-1111-1111-1111-111111111111";

fn path_link() -> DesignAssemblyOperandPathLink {
    DesignAssemblyOperandPathLink {
        locator_reference_offset: 0,
        locator_record_index: 64,
        locator_class_tag: "304".to_owned().try_into().unwrap(),
        locator_byte_offset: 0,
        locator_scope_reference_offset: 0,
        wrapper_record_index: 66,
        wrapper_reference_offset: 0,
        wrapper_class_tag: "382".to_owned().try_into().unwrap(),
        wrapper_byte_offset: 0,
        path_reference_offset: 0,
    }
}

fn path_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"329");
    bytes.extend_from_slice(&65_u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    for unit in GUID.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

#[test]
fn assembly_path_keeps_one_occurrence() {
    let bytes = path_bytes();
    let path = exact_assembly_operand_path(&bytes, 0, 65, bytes.len(), path_link(), None)
        .unwrap()
        .unwrap();
    assert_eq!(path.occurrence_guids().len(), 1);
}

fn write_reference(bytes: &mut [u8], at: usize, index: u32) {
    bytes[at] = 1;
    bytes[at + 1..at + 5].copy_from_slice(&index.to_le_bytes());
}

fn envelope_bytes(scope_record_index: u32) -> Vec<u8> {
    let mut locator = vec![0; 190];
    locator[0..4].copy_from_slice(&3_u32.to_le_bytes());
    locator[4..7].copy_from_slice(b"304");
    locator[7..11].copy_from_slice(&64_u32.to_le_bytes());
    write_reference(&mut locator, 21, 164);
    for ordinal in 0..16 {
        let value = if ordinal % 5 == 0 { 1.0_f64 } else { 0.0 };
        locator[33 + ordinal * 8..41 + ordinal * 8].copy_from_slice(&value.to_le_bytes());
    }
    write_reference(&mut locator, 162, scope_record_index);
    write_reference(&mut locator, 173, 66);
    locator[184..188].copy_from_slice(&2_u32.to_le_bytes());
    locator.extend_from_slice(&path_bytes());

    let mut wrapper = vec![0; 37];
    wrapper[0..4].copy_from_slice(&3_u32.to_le_bytes());
    wrapper[4..7].copy_from_slice(b"382");
    wrapper[7..11].copy_from_slice(&66_u32.to_le_bytes());
    wrapper[21] = 1;
    wrapper[22..26].copy_from_slice(&1_u32.to_le_bytes());
    write_reference(&mut wrapper, 26, 65);
    locator.extend_from_slice(&wrapper);
    locator.extend_from_slice(&3_u32.to_le_bytes());
    locator.extend_from_slice(b"396");
    locator.extend_from_slice(&67_u32.to_le_bytes());
    locator
}

#[test]
fn assembly_path_span_keeps_its_record_index() {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Assemble,
        10,
    );
    let bytes = envelope_bytes(scope.record_index);
    let path = exact_assembly_operand_path_envelope(
        &bytes,
        &scope,
        &std::collections::HashMap::new(),
        64,
        0,
        0,
    )
    .unwrap()
    .unwrap();
    assert_eq!(path.record_index, 65);
}

fn push_guid(bytes: &mut Vec<u8>, guid: &str) {
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    for unit in guid.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
}

/// An identity-qualified counted path record whose dynamic class tag is
/// `class_tag`: one occurrence GUID, then the four identity GUIDs.
fn identity_qualified_path_bytes(class_tag: [u8; 3], identities: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&class_tag);
    bytes.extend_from_slice(&65_u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    push_guid(&mut bytes, GUID);
    if identities {
        push_guid(&mut bytes, "22222222-2222-2222-2222-222222222222");
        push_guid(&mut bytes, "33333333-3333-3333-3333-333333333333");
        bytes.extend_from_slice(&2_u64.to_le_bytes());
        push_guid(&mut bytes, "44444444-4444-4444-4444-444444444444");
        push_guid(&mut bytes, "55555555-5555-5555-5555-555555555555");
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
    }
    bytes
}

#[test]
fn typed_path_record_reads_whatever_its_class_tag() {
    use crate::records::feature::assembly::TYPED_OCCURRENCE_PATH_TYPE_GUID as TYPED;
    let bytes = identity_qualified_path_bytes(*b"384", true);
    let read = |type_guid| {
        exact_assembly_operand_path(&bytes, 0, 65, bytes.len(), path_link(), type_guid).unwrap()
    };
    assert!(
        read(None).is_none(),
        "class 384 is not an admitted path class"
    );
    let path = read(Some(TYPED)).expect("typed path");
    assert!(path.admitted_by_type());
    assert_eq!(path.occurrence_guids().len(), 1);
    assert_eq!(path.identity_guids().len(), 4);

    // A typed path stores its identity suffix.
    let bare = identity_qualified_path_bytes(*b"384", false);
    assert!(
        exact_assembly_operand_path(&bare, 0, 65, bare.len(), path_link(), Some(TYPED))
            .unwrap()
            .is_none()
    );

    // An admitted class keeps its own reading and is not marked typed.
    let listed = identity_qualified_path_bytes(*b"390", true);
    let path = exact_assembly_operand_path(&listed, 0, 65, listed.len(), path_link(), None)
        .unwrap()
        .expect("class 390 path");
    assert!(!path.admitted_by_type());
}

/// A wide locator (record 64) with or without its transform, then the path
/// (65), its wrapper (66) and the next header.
fn wide_envelope_bytes(scope_record_index: u32, transform: bool) -> Vec<u8> {
    let mut locator = vec![0; 68];
    locator[0..4].copy_from_slice(&3_u32.to_le_bytes());
    locator[4..7].copy_from_slice(b"337");
    locator[7..11].copy_from_slice(&64_u32.to_le_bytes());
    locator[39..43].copy_from_slice(&3_u32.to_le_bytes());
    write_reference(&mut locator, 56, 164);
    if transform {
        for ordinal in 0..16 {
            let value = if ordinal % 5 == 0 { 1.0_f64 } else { 0.0 };
            locator.extend_from_slice(&value.to_le_bytes());
        }
    } else {
        locator[67] = 1;
    }
    let backlink = locator.len() + 1;
    locator.resize(backlink + 28, 0);
    write_reference(&mut locator, backlink, scope_record_index);
    write_reference(&mut locator, backlink + 11, 66);
    locator[backlink + 22..backlink + 26].copy_from_slice(&2_u32.to_le_bytes());
    locator.extend_from_slice(&path_bytes());

    let mut wrapper = vec![0; 37];
    wrapper[0..4].copy_from_slice(&3_u32.to_le_bytes());
    wrapper[4..7].copy_from_slice(b"341");
    wrapper[7..11].copy_from_slice(&66_u32.to_le_bytes());
    wrapper[21] = 1;
    wrapper[22..26].copy_from_slice(&1_u32.to_le_bytes());
    write_reference(&mut wrapper, 26, 65);
    locator.extend_from_slice(&wrapper);
    locator.extend_from_slice(&3_u32.to_le_bytes());
    locator.extend_from_slice(b"396");
    locator.extend_from_slice(&67_u32.to_le_bytes());
    locator
}

#[test]
fn wide_scope_locator_is_admitted_by_its_record_type() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Assemble,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 945;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let typed = std::collections::HashMap::from([(64, (super::WIDE_PATH_LOCATOR_TYPE_GUID, 5))]);
    for transform in [true, false] {
        let bytes = wide_envelope_bytes(scope.record_index, transform);
        let locator_length = if transform { 225 } else { 97 };
        assert_eq!(bytes.len(), locator_length + path_bytes().len() + 37 + 11);
        let path = exact_assembly_operand_path_envelope(&bytes, &scope, &typed, 64, 0, 0)
            .unwrap()
            .expect("wide locator");
        assert_eq!(path.record_index, 65);
        let untyped = std::collections::HashMap::new();
        assert!(
            exact_assembly_operand_path_envelope(&bytes, &scope, &untyped, 64, 0, 0)
                .unwrap()
                .is_none()
        );
    }
}

/// A typed path names a local occurrence by its decimal occurrence key.
#[test]
fn typed_path_reads_a_decimal_occurrence_key() {
    use crate::records::feature::assembly::TYPED_OCCURRENCE_PATH_TYPE_GUID as TYPED;
    let guid_path = identity_qualified_path_bytes(*b"366", true);
    // Replace the 36-unit occurrence GUID at offset 25 with the key "34".
    let mut bytes = guid_path[..25].to_vec();
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    for unit in "34".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes.extend_from_slice(&guid_path[25 + 4 + 72..]);
    let path = exact_assembly_operand_path(&bytes, 0, 65, bytes.len(), path_link(), Some(TYPED))
        .unwrap()
        .expect("typed path with a decimal key");
    assert_eq!(path.occurrence_guids()[0].value.as_str(), "34");
    assert_eq!(path.identity_guids().len(), 4);
}

/// A typed path's identity GUIDs can be followed by a zero word, a pair count
/// and that many pairs of occurrence identities.
#[test]
fn typed_path_reads_its_occurrence_identity_pairs() {
    use crate::records::feature::assembly::TYPED_OCCURRENCE_PATH_TYPE_GUID as TYPED;
    let plain = identity_qualified_path_bytes(*b"366", true);
    let mut bytes = plain[..plain.len() - 8].to_vec();
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    for unit in "43".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    push_guid(&mut bytes, "66666666-6666-6666-6666-666666666666");
    let path = exact_assembly_operand_path(&bytes, 0, 65, bytes.len(), path_link(), Some(TYPED))
        .unwrap()
        .expect("typed path with an identity pair");
    let pairs = path.paired_occurrence_keys();
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0].value.as_str(), "43");
    assert_eq!(
        pairs[1].value.as_str(),
        "66666666-6666-6666-6666-666666666666"
    );
    assert_eq!(path.identity_guids().len(), 4);

    // The plain zero tail has no pairs; a pair without its `u32 2` is refused.
    let plain = exact_assembly_operand_path(&plain, 0, 65, plain.len(), path_link(), Some(TYPED))
        .unwrap()
        .expect("typed path without pairs");
    assert!(plain.paired_occurrence_keys().is_empty());
    // A path its class admits, not its type, has no pairs.
    let mut untyped = bytes.clone();
    untyped[4..7].copy_from_slice(b"390");
    assert!(
        exact_assembly_operand_path(&untyped, 0, 65, untyped.len(), path_link(), None)
            .unwrap()
            .is_none()
    );
    let marker = bytes.len() - 4 - 72 - 8 - 4;
    bytes[marker] = 3;
    assert!(
        exact_assembly_operand_path(&bytes, 0, 65, bytes.len(), path_link(), Some(TYPED))
            .unwrap()
            .is_none()
    );
}

/// A wide locator's typed path can continue over a second typed path record;
/// the wrapper then counts and names both records.
#[test]
fn wide_locator_path_continues_over_typed_records() {
    use crate::records::feature::assembly::TYPED_OCCURRENCE_PATH_TYPE_GUID as TYPED;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Assemble,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 945;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let envelope = |continued: bool| {
        let mut bytes = wide_envelope_bytes(scope.record_index, false);
        bytes.truncate(97);
        write_reference(&mut bytes, 97 - 17, if continued { 67 } else { 66 });
        // The first record ends in one occurrence identity pair.
        let first = identity_qualified_path_bytes(*b"366", true);
        bytes.extend_from_slice(&first[..first.len() - 8]);
        for word in [0_u32, 1, 2, 2] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        for unit in "43".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        push_guid(&mut bytes, "66666666-6666-6666-6666-666666666666");
        let mut continuation = identity_qualified_path_bytes(*b"366", true);
        continuation[7..15].copy_from_slice(&66_u64.to_le_bytes());
        let guid = "77777777-7777-7777-7777-777777777777".encode_utf16();
        for (ordinal, unit) in guid.enumerate() {
            continuation[25 + 4 + 2 * ordinal..25 + 6 + 2 * ordinal]
                .copy_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&continuation);
        let mut wrapper = vec![0; 48];
        wrapper[0..4].copy_from_slice(&3_u32.to_le_bytes());
        wrapper[4..7].copy_from_slice(b"341");
        wrapper[7..11].copy_from_slice(&67_u32.to_le_bytes());
        wrapper[21] = 1;
        wrapper[22..26].copy_from_slice(&2_u32.to_le_bytes());
        write_reference(&mut wrapper, 26, 65);
        write_reference(&mut wrapper, 37, 66);
        bytes.extend_from_slice(&wrapper);
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(b"396");
        bytes.extend_from_slice(&68_u32.to_le_bytes());
        bytes
    };
    let types = std::collections::HashMap::from([
        (64, (super::WIDE_PATH_LOCATOR_TYPE_GUID, 5)),
        (65, (TYPED, 4)),
        (66, (TYPED, 4)),
    ]);
    let bytes = envelope(true);
    let path = exact_assembly_operand_path_envelope(&bytes, &scope, &types, 64, 0, 0)
        .unwrap()
        .expect("continued wide path");
    let occurrences = path
        .occurrence_guids()
        .iter()
        .map(|guid| guid.value.as_str())
        .collect::<Vec<_>>();
    assert_eq!(occurrences, [GUID, "77777777-7777-7777-7777-777777777777"]);
    assert_eq!(path.identity_guids().len(), 8);
    assert_eq!(path.link().wrapper_record_index, 67);
    // The joined path keeps the first record's pair and its type.
    assert!(path.admitted_by_type());
    let pairs = path
        .paired_occurrence_keys()
        .iter()
        .map(|key| key.value.as_str())
        .collect::<Vec<_>>();
    assert_eq!(pairs, ["43", "66666666-6666-6666-6666-666666666666"]);

    // A continuation record that its type does not admit does not continue.
    let untyped = std::collections::HashMap::from([
        (64, (super::WIDE_PATH_LOCATOR_TYPE_GUID, 5)),
        (65, (TYPED, 4)),
    ]);
    assert!(
        exact_assembly_operand_path_envelope(&bytes, &scope, &untyped, 64, 0, 0)
            .unwrap()
            .is_none()
    );
}

fn versioned_rigid_group_envelope(version: u32, matrix: bool, construction: bool) -> Vec<u8> {
    fn reference(bytes: &mut Vec<u8>, target: u32, typed: bool) {
        if typed {
            bytes.push(1);
            bytes.extend_from_slice(&u64::from(target).to_le_bytes());
            bytes.extend_from_slice(&36_u32.to_le_bytes());
            bytes.extend_from_slice(b"11111111-2222-3333-4444-555555555555");
            bytes.extend_from_slice(&[0; 2]);
        } else {
            let start = bytes.len();
            bytes.resize(start + 11, 0);
            write_reference(bytes, start, target);
        }
    }
    let typed = version == 2;
    let mut bytes = envelope_bytes(10)[..11].to_vec();
    if version == 5 {
        bytes.extend_from_slice(&[0; 28]);
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 13]);
    } else {
        bytes.extend_from_slice(&[0; 10]);
    }
    if construction {
        reference(&mut bytes, 164, typed);
    } else {
        bytes.push(0);
    }
    bytes.push(u8::from(!matrix));
    if matrix {
        for ordinal in 0..16 {
            let value: f64 = if ordinal % 5 == 0 {
                1.0
            } else if ordinal == 3 {
                2.5
            } else {
                0.0
            };
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    if !typed {
        bytes.push(0);
    }
    reference(&mut bytes, 10, typed);
    reference(&mut bytes, 66, typed);
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&path_bytes());
    let start = bytes.len();
    bytes.extend_from_slice(&envelope_bytes(10)[..11]);
    bytes[start + 7..start + 11].copy_from_slice(&66_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    reference(&mut bytes, 65, typed);
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"396");
    bytes.extend_from_slice(&67_u32.to_le_bytes());
    bytes
}

#[test]
fn rigid_group_locator_version_selects_its_exact_frame_and_reference_width() {
    use crate::design::decode::scopes::assembly_operand_frames::exact_as_built_operand_frame;
    let mut scope = DesignParameterScope::empty("generated:scope", DesignFeatureKind::AsBuilt, 10);
    scope
        .try_edit(|draft| {
            crate::design::test_support::lengthen_scope_frame(draft, 257);
            draft.reference_count_offset = 153;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(Vec::new());
            draft.layout_fixture_references();
            draft.feature_ordinal_offset = 181;
        })
        .unwrap();
    scope.class_tag = "283".to_owned().try_into().unwrap();
    scope.paired_class_tag = "264".to_owned().try_into().unwrap();
    for (version, matrix, construction, length) in [
        (3, true, false, 180),
        (3, false, false, 52),
        (3, true, true, 190),
        (3, false, true, 62),
        (2, true, true, 309),
        (2, false, true, 181),
        (2, true, false, 259),
        (2, false, false, 131),
    ] {
        let bytes = versioned_rigid_group_envelope(version, matrix, construction);
        let read = |bytes: &[u8], version| {
            let types = std::collections::HashMap::from([(
                64,
                (super::WIDE_PATH_LOCATOR_TYPE_GUID, version),
            )]);
            exact_assembly_operand_path_envelope(bytes, &scope, &types, 64, 1, 0).unwrap()
        };
        assert!(exact_assembly_operand_path_envelope(
            &bytes,
            &scope,
            &std::collections::HashMap::new(),
            64,
            1,
            0,
        )
        .unwrap()
        .is_none());
        let path = read(&bytes, version).unwrap();
        let discriminator = usize::try_from(length).unwrap() - 6;
        let mut pathless = bytes.clone();
        pathless[discriminator..discriminator + 4].copy_from_slice(&0_u32.to_le_bytes());
        assert!(read(&pathless, version).is_none());
        assert_eq!(path.byte_offset(), length);
        let frame = exact_as_built_operand_frame(
            &bytes,
            &path,
            &scope,
            &std::collections::HashMap::from([(64, (super::WIDE_PATH_LOCATOR_TYPE_GUID, version))]),
        )
        .unwrap();
        assert_eq!(
            frame.reference_record_index,
            if construction { 164 } else { 64 }
        );
        assert_eq!(frame.reference_offset, if construction { 22 } else { 7 });
        let flag = if construction {
            if version == 2 {
                72
            } else {
                32
            }
        } else {
            22
        };
        assert_eq!(frame.transform_offset, flag + u64::from(matrix));
        let mut bad_flag = bytes.clone();
        bad_flag[usize::try_from(flag).unwrap()] = 2;
        assert!(read(&bad_flag, version).is_none());
        assert_eq!(frame.transform.rows()[0][3], if matrix { 2.5 } else { 0.0 });
        for other_version in [1, 2, 3, 4, 5, 6] {
            if other_version != version {
                assert!(read(&bytes, other_version).is_none());
            }
        }
        for discriminator in [0_u32, 3] {
            let mut pathless = bytes.clone();
            let at = usize::try_from(length - 6).unwrap();
            pathless[at..at + 4].copy_from_slice(&discriminator.to_le_bytes());
            assert!(read(&pathless, version).is_none());
        }
        for at in [
            length - 1,
            length + u64::try_from(path_bytes().len()).unwrap() + 26,
        ] {
            let mut bad = bytes.clone();
            bad[usize::try_from(at).unwrap()] ^= 1;
            assert!(read(&bad, version).is_none(), "mutation at {at}");
        }
        let mut padded = bytes.clone();
        padded.insert(usize::try_from(length).unwrap(), 0);
        assert!(read(&padded, version).is_none());
        let mut wrapper_padded = bytes.clone();
        let end = wrapper_padded.len() - 11;
        wrapper_padded.insert(end, 0);
        assert!(read(&wrapper_padded, version).is_none());
    }
}

#[test]
fn typed_root_operand_path_keeps_two_identities_and_exact_closure() {
    use crate::records::feature::assembly::{
        DesignAssemblyOperandPath, TYPED_OCCURRENCE_PATH_TYPE_GUID as TYPED,
    };
    let mut bytes = path_bytes()[..21].to_vec();
    bytes[4..7].copy_from_slice(b"294");
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    push_guid(&mut bytes, "00000000-0000-0000-0000-000000000000");
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u64.to_le_bytes());
    push_guid(&mut bytes, "22222222-2222-2222-2222-222222222222");
    push_guid(&mut bytes, "33333333-3333-3333-3333-333333333333");
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    assert_eq!(bytes.len(), 277);
    let read = |bytes: &[u8], typed| {
        exact_assembly_operand_path(bytes, 0, 65, bytes.len(), path_link(), typed).unwrap()
    };
    let path = read(&bytes, Some(TYPED)).unwrap();
    assert_eq!(
        path.occurrence_guids()[0].value.as_str(),
        "00000000-0000-0000-0000-000000000000"
    );
    assert_eq!(path.identity_guids().len(), 2);
    let round_trip: DesignAssemblyOperandPath =
        serde_json::from_value(serde_json::to_value(&path).unwrap()).unwrap();
    assert_eq!(path, round_trip);
    assert!(read(&bytes, None).is_none());
    for at in [21, 29, 101, 105, 265, 276] {
        let mut bad = bytes.clone();
        bad[at] ^= 1;
        assert!(read(&bad, Some(TYPED)).is_none(), "mutation at {at}");
    }
    let mut padded = bytes.clone();
    padded.push(0);
    assert!(read(&padded, Some(TYPED)).is_none());
}

#[test]
fn rigid_group_nullable_locators_keep_counted_path_order_and_reference_width() {
    use crate::records::feature::assembly::TYPED_OCCURRENCE_PATH_TYPE_GUID as TYPED;
    let mut scope = DesignParameterScope::empty("generated:scope", DesignFeatureKind::AsBuilt, 10);
    scope
        .try_edit(|draft| {
            crate::design::test_support::lengthen_scope_frame(draft, 257);
            draft.reference_count_offset = 153;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(Vec::new());
            draft.layout_fixture_references();
            draft.feature_ordinal_offset = 181;
        })
        .unwrap();
    for (version, matrix, length) in [
        (3, false, 52),
        (3, true, 180),
        (2, false, 131),
        (2, true, 259),
    ] {
        let original = versioned_rigid_group_envelope(version, matrix, false);
        let stride = if version == 2 { 51 } else { 11 };
        let mut bytes = original[..length].to_vec();
        write_reference(&mut bytes, length - 6 - stride, 67);
        let first = identity_qualified_path_bytes(*b"384", true);
        let mut second = first.clone();
        second[7..15].copy_from_slice(&66_u64.to_le_bytes());
        bytes.extend_from_slice(&first);
        bytes.extend_from_slice(&second);
        let wrapper_at = bytes.len();
        let original_wrapper = length + path_bytes().len();
        let mut wrapper = original[original_wrapper..original.len() - 11].to_vec();
        wrapper[7..11].copy_from_slice(&67_u32.to_le_bytes());
        wrapper[22..26].copy_from_slice(&2_u32.to_le_bytes());
        let mut reference = wrapper[26..].to_vec();
        reference[1..5].copy_from_slice(&66_u32.to_le_bytes());
        wrapper.extend_from_slice(&reference);
        bytes.extend_from_slice(&wrapper);
        bytes.extend_from_slice(&original[original.len() - 11..]);
        let end = bytes.len();
        bytes[end - 4..].copy_from_slice(&68_u32.to_le_bytes());
        let types = std::collections::HashMap::from([
            (64, (super::WIDE_PATH_LOCATOR_TYPE_GUID, version)),
            (65, (TYPED, 4)),
            (66, (TYPED, 4)),
        ]);
        let read = |bytes: &[u8]| {
            exact_assembly_operand_path_envelope(bytes, &scope, &types, 64, 1, 0).unwrap()
        };
        let path = read(&bytes).unwrap();
        assert_eq!(path.occurrence_guids().len(), 2);
        assert_eq!(path.identity_guids().len(), 8);
        assert_eq!(path.link().wrapper_record_index, 67);
        for at in [wrapper_at + 22, wrapper_at + 27, wrapper_at + 27 + stride] {
            let mut bad = bytes.clone();
            bad[at] ^= 1;
            assert!(read(&bad).is_none());
        }
        let mut invalid_tail = bytes.clone();
        invalid_tail.insert(wrapper_at, 0xff);
        assert!(read(&invalid_tail).is_none());
    }
}

#[test]
fn rigid_group_locator_refuses_cross_version_lengths() {
    use crate::design::decode::scopes::assembly_operand_frames::exact_as_built_operand_frame;
    let mut scope = DesignParameterScope::empty("generated:scope", DesignFeatureKind::AsBuilt, 10);
    scope
        .try_edit(|draft| {
            crate::design::test_support::lengthen_scope_frame(draft, 257);
            draft.reference_count_offset = 153;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(Vec::new());
            draft.layout_fixture_references();
            draft.feature_ordinal_offset = 181;
        })
        .unwrap();
    for (frame_version, matrix, construction) in [
        (2, true, true),
        (2, false, true),
        (2, true, false),
        (2, false, false),
        (3, true, false),
        (3, false, false),
        (3, true, true),
        (3, false, true),
        (5, true, true),
        (5, false, true),
    ] {
        let bytes = versioned_rigid_group_envelope(frame_version, matrix, construction);
        let types = std::collections::HashMap::from([(
            64,
            (super::WIDE_PATH_LOCATOR_TYPE_GUID, frame_version),
        )]);
        let path = exact_assembly_operand_path_envelope(&bytes, &scope, &types, 64, 1, 0)
            .unwrap()
            .unwrap();
        assert!(exact_as_built_operand_frame(&bytes, &path, &scope, &types).is_some());
        for version in [1, 2, 3, 4, 5, 6] {
            let types = std::collections::HashMap::from([(
                64,
                (super::WIDE_PATH_LOCATOR_TYPE_GUID, version),
            )]);
            let decoded =
                exact_assembly_operand_path_envelope(&bytes, &scope, &types, 64, 1, 0).unwrap();
            assert_eq!(decoded.is_some(), version == frame_version);
            assert_eq!(
                exact_as_built_operand_frame(&bytes, &path, &scope, &types).is_some(),
                version == frame_version
            );
        }
    }
}
