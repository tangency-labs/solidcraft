// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::base_feature::exact_base_feature_construction;
use crate::records::feature::base_feature::DesignBaseFeatureConstruction;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::u64_from_index;

/// Count field written after the two parallel 15-byte runs.
#[derive(Clone, Copy)]
enum Count {
    /// `u8 1`, six zero bytes, u32 count.
    Passive,
    /// `u8 1`, five zero bytes, `u8 1`, u32 count.
    BodySuffix,
    /// `u8 1`, ten zero bytes, u32 count.
    Ordinary,
}

/// A result-body frame with bodies `101..`, passive references `201..`,
/// metadata record 301, and results `401..`, under the given class pair.
fn frame(
    count: Count,
    metadata_width: usize,
    body_count: usize,
    classes: (&str, &str),
) -> (Vec<u8>, DesignParameterScope) {
    let count_width = match count {
        Count::Passive | Count::BodySuffix => 11,
        Count::Ordinary => 15,
    };
    let kind_offset = 24 + 52 * body_count + count_width + 1 + 9 + metadata_width + 4 + 110;
    let frame_length = kind_offset + 101;
    let mut bytes = vec![0u8; frame_length];
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&(2 * u32::try_from(body_count).unwrap()).to_le_bytes());
    let mut cursor = 24;
    for base in [101u64, 201] {
        for ordinal in 0..body_count {
            bytes[cursor] = 1;
            bytes[cursor + 1..cursor + 9]
                .copy_from_slice(&(base + u64_from_index(ordinal)).to_le_bytes());
            cursor += 15;
        }
    }
    bytes[cursor] = 1;
    let count_at = match count {
        Count::Passive => cursor + 7,
        Count::BodySuffix => {
            bytes[cursor + 6] = 1;
            cursor + 7
        }
        Count::Ordinary => cursor + 11,
    };
    bytes[count_at..count_at + 4]
        .copy_from_slice(&u32::try_from(body_count).unwrap().to_le_bytes());
    cursor += count_width;
    let repeated = match count {
        Count::BodySuffix => 101,
        Count::Passive | Count::Ordinary => 201,
    };
    for ordinal in 0..body_count {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 5]
            .copy_from_slice(&(repeated + u32::try_from(ordinal).unwrap()).to_le_bytes());
        cursor += 11;
    }
    cursor += 1;
    bytes[cursor] = 1;
    bytes[cursor + 1..cursor + 9].copy_from_slice(&301u64.to_le_bytes());
    cursor += 9 + metadata_width;
    bytes[cursor..cursor + 4].copy_from_slice(&u32::try_from(body_count).unwrap().to_le_bytes());
    cursor += 4;
    for ordinal in 0..body_count {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 5]
            .copy_from_slice(&(401 + u32::try_from(ordinal).unwrap()).to_le_bytes());
        cursor += 11;
    }
    assert!(cursor <= kind_offset - 102);
    let mut scope = DesignParameterScope::empty(
        "f3d:scope#base-feature-unclassed",
        crate::records::feature::scope::DesignFeatureKind::BaseFeature,
        70,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from(classes.0.to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from(classes.1.to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = u64_from_index(frame_length);
            draft.kind_offset = u64_from_index(kind_offset);
            draft.paired_byte_offset = u64_from_index(frame_length);
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![301]);
            draft.reference_count_offset =
                draft.kind_offset - 12 - 11 * u64_from_index(draft.reference_members.len());
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope)
}

fn decoded(bytes: &[u8], scope: &DesignParameterScope) -> Option<(Vec<[u64; 3]>, u32, usize)> {
    let DesignBaseFeatureConstruction::ResultBodies {
        bodies,
        metadata_record,
        metadata_field,
        ..
    } = exact_base_feature_construction(bytes, scope).unwrap()?
    else {
        panic!("a result-body frame selected another form");
    };
    let rows = bodies
        .iter()
        .map(|body| {
            [
                body.entity.value,
                u64::from(body.reference.value),
                u64::from(body.result.value),
            ]
        })
        .collect();
    Some((rows, metadata_record, metadata_field.len()))
}

#[test]
fn base_feature_result_bodies_under_an_unlisted_class_pair_follow_their_count_field() {
    for body_count in [1, 2] {
        let expected = (0..u64_from_index(body_count))
            .map(|ordinal| [101 + ordinal, 201 + ordinal, 401 + ordinal])
            .collect::<Vec<_>>();
        for (count, metadata_width, classes) in [
            (Count::Passive, 2, ("325", "258")),
            (Count::BodySuffix, 2, ("325", "258")),
            (Count::Ordinary, 2, ("404", "262")),
            (Count::Ordinary, 6, ("404", "262")),
        ] {
            let (bytes, scope) = frame(count, metadata_width, body_count, classes);
            assert_eq!(
                decoded(&bytes, &scope),
                Some((expected.clone(), 301, metadata_width)),
                "{} bodies, {metadata_width}-byte metadata",
                body_count
            );
        }
    }
}

#[test]
fn an_unlisted_class_pair_needs_a_count_field_and_size_that_agree() {
    let (bytes, scope) = frame(Count::Passive, 2, 1, ("325", "258"));
    let mut wrong_count = bytes.clone();
    wrong_count[24 + 30 + 7] = 2;
    assert_eq!(decoded(&wrong_count, &scope), None);
    let mut wrong_repeat_marker = bytes.clone();
    wrong_repeat_marker[24 + 30 + 6] = 2;
    assert_eq!(decoded(&wrong_repeat_marker, &scope), None);

    let (bytes, scope) = frame(Count::Ordinary, 4, 1, ("404", "262"));
    assert_eq!(decoded(&bytes, &scope), None);
}

#[test]
fn a_listed_class_pair_keeps_its_own_count_field() {
    let (bytes, scope) = frame(Count::BodySuffix, 2, 1, ("409", "262"));
    assert_eq!(decoded(&bytes, &scope), None);
    let (mut bytes, scope) = frame(Count::Passive, 2, 1, ("420", "258"));
    assert_eq!(decoded(&bytes, &scope), None);
    // A body suffix equal to its passive reference satisfies both repeat runs.
    bytes[25..33].copy_from_slice(&201u64.to_le_bytes());
    assert_eq!(decoded(&bytes, &scope), None);
}
