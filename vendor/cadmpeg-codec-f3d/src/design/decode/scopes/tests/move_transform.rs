// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::direct_face::{exact_move_face_operation, exact_move_operation};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};

fn fixture(class: &str, length: usize) -> (Vec<u8>, DesignParameterScope) {
    let mut bytes = vec![0; length];
    bytes[..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(class.as_bytes());
    bytes[7..11].copy_from_slice(&71u32.to_le_bytes());
    let form: u32 = if class == "452" { 5 } else { 1 };
    bytes[43..47].copy_from_slice(&form.to_le_bytes());
    let transform: [[f64; 4]; 4] = [
        [0., -1., 0., 7.],
        [1., 0., 0., 11.],
        [0., 0., 1., 13.],
        [0., 0., 0., 1.],
    ];
    for (cell, value) in transform.into_iter().flatten().enumerate() {
        let at = 48 + cell * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"262");
    bytes.extend_from_slice(&71u32.to_le_bytes());
    let mut scope = DesignParameterScope::empty("f3d:test:scope#17", DesignFeatureKind::Move, 17);
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![71]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope)
}

#[test]
fn move_transform_added_classes_decode_exact_matrix() {
    for (class, length) in [
        ("330", 273),
        ("273", 273),
        ("408", 273),
        ("372", 273),
        ("289", 273),
        ("394", 273),
        ("406", 273),
        ("282", 273),
        ("403", 273),
        ("391", 273),
        ("452", 253),
    ] {
        let (bytes, scope) = fixture(class, length);
        let decoded = exact_move_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
        )
        .unwrap();
        assert_eq!(
            decoded.transform.rows(),
            [
                [0., -1., 0., 7.],
                [1., 0., 0., 11.],
                [0., 0., 1., 13.],
                [0., 0., 0., 1.],
            ]
        );
        assert_eq!(u32::from(decoded.form), if class == "452" { 5 } else { 1 });
        assert_eq!(decoded.form_offset, 43);
        assert_eq!(decoded.transform_offset, 48);
        assert_eq!(decoded.transform_record_index, 71);
    }
}

/// Places the reference table, kind, and paired header of a `MoveFace` frame.
fn lay_out_move_face(
    scope: &mut DesignParameterScope,
    start: usize,
    count_offset: u64,
    references: Vec<u32>,
) {
    let start = u64::try_from(start).unwrap();
    let kind_offset = count_offset + 12 + 11 * u64::try_from(references.len()).unwrap();
    scope
        .try_edit(|draft| {
            draft.byte_offset = start;
            draft.reference_count_offset = start + count_offset;
            draft.kind_offset = start + kind_offset;
            draft.frame_length = kind_offset + 93;
            draft.paired_byte_offset = start + draft.frame_length;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(references);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
}

/// A class-433 transform record 71, then a `MoveFace` scope frame that names
/// group 50, member 60, and that transform.
fn move_face_fixture(marker_one: bool) -> (Vec<u8>, DesignParameterScope, usize) {
    move_face_fixture_with("433", 253, DesignFeatureKind::MoveFace, marker_one)
}

fn move_face_fixture_with(
    class: &str,
    length: usize,
    kind: DesignFeatureKind,
    marker_one: bool,
) -> (Vec<u8>, DesignParameterScope, usize) {
    move_face_over(fixture(class, length).0, kind, marker_one)
}

fn move_face_over(
    mut bytes: Vec<u8>,
    kind: DesignFeatureKind,
    marker_one: bool,
) -> (Vec<u8>, DesignParameterScope, usize) {
    let start = bytes.len();
    bytes.resize(start + 200, 0);
    let marker = if marker_one {
        bytes[start + 20] = 1;
        26
    } else {
        22
    };
    bytes[start + marker - 1] = 1;
    bytes[start + marker] = 1;
    bytes[start + marker + 1..start + marker + 5].copy_from_slice(&71u32.to_le_bytes());
    bytes[start + marker + 11..start + marker + 15].copy_from_slice(&1u32.to_le_bytes());
    bytes[start + marker + 15] = 1;
    bytes[start + marker + 16..start + marker + 20].copy_from_slice(&50u32.to_le_bytes());
    let mut scope = DesignParameterScope::empty("f3d:test:scope#17", kind, 17);
    lay_out_move_face(
        &mut scope,
        start,
        if marker_one { 131 } else { 127 },
        vec![50, 60, 71],
    );
    (bytes, scope, marker)
}

fn decoded_move_face(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<crate::records::feature::direct_face::DesignMoveOperation> {
    exact_move_face_operation(
        bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        scope,
    )
}

#[test]
fn move_face_decodes_both_prologues_and_refuses_broken_links() {
    for marker_one in [true, false] {
        let (bytes, scope, marker) = move_face_fixture(marker_one);
        let decoded = decoded_move_face(&bytes, &scope).unwrap();
        assert_eq!(decoded.transform_record_index, 71);
        assert_eq!(decoded.transform_offset, 48);
        let start = usize::try_from(scope.byte_offset()).unwrap();
        for at in [
            marker - 1,
            marker,
            marker + 1,
            marker + 11,
            marker + 15,
            marker + 16,
        ] {
            let mut changed = bytes.clone();
            changed[start + at] ^= 0x02;
            assert!(
                decoded_move_face(&changed, &scope).is_none(),
                "offset {at} accepted"
            );
        }
        let mut shifted = scope.clone();
        lay_out_move_face(
            &mut shifted,
            start,
            if marker_one { 132 } else { 128 },
            vec![50, 60, 71],
        );
        assert!(decoded_move_face(&bytes, &shifted).is_none());
    }

    let (mut bytes, scope, _) = move_face_fixture(true);
    let start = usize::try_from(scope.byte_offset()).unwrap();
    bytes[start + 21] = 1;
    assert!(decoded_move_face(&bytes, &scope).is_none());

    let (bytes, mut no_member, _) = move_face_fixture(true);
    let start = usize::try_from(no_member.byte_offset()).unwrap();
    lay_out_move_face(&mut no_member, start, 131, vec![50, 71]);
    assert!(decoded_move_face(&bytes, &no_member).is_none());
}

#[test]
fn move_face_transform_is_selected_by_its_reference_not_by_class_or_length() {
    for kind in [DesignFeatureKind::MoveFace, DesignFeatureKind::DeplacerFace] {
        for (class, length) in [("313", 256), ("396", 256), ("999", 300), ("433", 253)] {
            let (bytes, scope, _) = move_face_fixture_with(class, length, kind.clone(), false);
            let decoded = decoded_move_face(&bytes, &scope).unwrap();
            assert_eq!(decoded.transform_record_index, 71);
            assert_eq!(decoded.form_offset, 43);
            assert_eq!(decoded.transform_offset, 48);
            assert_eq!(decoded.transform.rows()[1], [1., 0., 0., 11.]);
        }
    }
}

#[test]
fn move_face_transform_refuses_a_short_frame_reserved_bytes_and_a_broken_matrix() {
    let refused = |bytes: &[u8], scope: &DesignParameterScope| {
        assert!(decoded_move_face(bytes, scope).is_none());
    };
    let (bytes, scope, _) = move_face_fixture_with("313", 256, DesignFeatureKind::MoveFace, false);
    let (mut short, _) = fixture("313", 176);
    short.remove(175);
    let (short, short_scope, _) = move_face_over(short, DesignFeatureKind::MoveFace, false);
    refused(&short, &short_scope);
    for (at, value) in [(11, 1), (42, 1), (43, 2), (47, 1)] {
        let mut changed = bytes.clone();
        changed[at] = value;
        refused(&changed, &scope);
    }
    let mut changed = bytes.clone();
    changed[168..176].copy_from_slice(&2.0f64.to_le_bytes());
    refused(&changed, &scope);
}

#[test]
fn move_face_transform_refuses_a_record_with_two_admitted_frames() {
    let (first, _) = fixture("313", 256);
    let second_frame = first.len();
    let mut frames = first;
    frames.extend_from_slice(&fixture("313", 256).0);
    let (bytes, scope, _) = move_face_over(frames.clone(), DesignFeatureKind::MoveFace, false);
    assert!(decoded_move_face(&bytes, &scope).is_none());

    frames[second_frame + 168..second_frame + 176].copy_from_slice(&2.0f64.to_le_bytes());
    let (bytes, scope, _) = move_face_over(frames, DesignFeatureKind::MoveFace, false);
    assert_eq!(
        decoded_move_face(&bytes, &scope).unwrap().transform_offset,
        48
    );
}

#[test]
fn move_transform_refuses_wrong_length_controls_matrix_and_pair() {
    let (bytes, scope) = fixture("330", 273);
    let mut variants = Vec::new();
    for (at, value) in [(11, 1), (43, 2), (47, 1)] {
        let mut changed = bytes.clone();
        changed[at] = value;
        variants.push(changed);
    }
    let mut changed = bytes.clone();
    changed[168..176].copy_from_slice(&2.0f64.to_le_bytes());
    variants.push(changed);
    variants.push(fixture("330", 253).0);
    variants.push(fixture("330", 274).0);
    let mut changed = bytes.clone();
    changed[43..47].copy_from_slice(&5u32.to_le_bytes());
    variants.push(changed);
    let mut changed = fixture("452", 253).0;
    changed[43..47].copy_from_slice(&1u32.to_le_bytes());
    variants.push(changed);
    let mut changed = fixture("452", 253).0;
    changed[257..260].copy_from_slice(b"258");
    variants.push(changed);
    variants.push(fixture("999", 273).0);
    for bytes in variants {
        assert!(exact_move_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
        )
        .is_none());
    }
}
