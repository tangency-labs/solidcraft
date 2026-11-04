// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::scopes::parameter_scope::parse_parameter_scope;
use crate::design::decode::scopes::work_geometry::{
    exact_joint_origin_frame, exact_work_axis_construction, exact_work_plane_frame,
};
use crate::layout::joint_origin_legacy_class_337_266_frame as joint_origin_class_337_266;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::DesignParameterScope;
use crate::test_support::lp_utf16;
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::features::{Feature, FeatureDefinition};
use cadmpeg_ir::math::{Point3, Vector3};

#[test]
fn parameter_scope_uses_same_index_pair_and_fixed_kind_tail() {
    let (bytes, scope, transform) = fixed_kind_frames();
    super::fixed_kind_tail_operations::fixed_kind_tail_operations(bytes, scope, transform);
}

fn fixed_kind_frames() -> (Vec<u8>, DesignParameterScope, [[f64; 4]; 4]) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"301");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    let reference_count_at = bytes.len();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    let reference_at = bytes.len();
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "Sketch");
    let feature_ordinal_at = bytes.len();
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned()).unwrap(),
        byte_offset: 0,
    };

    let scope = parse_parameter_scope(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        scope.kind(),
        crate::records::feature::scope::DesignFeatureKind::Sketch
    );
    assert_eq!(scope.feature_ordinal.get(), 1);
    assert_eq!(
        scope.feature_ordinal_offset(),
        u64_from_index(feature_ordinal_at)
    );
    assert_eq!(scope.history_state_id(), Some(7));
    assert_eq!(scope.previous_history_state_id(), Some(2));
    assert_eq!(
        scope.reference_count_offset(),
        u64_from_index(reference_count_at)
    );
    assert_eq!(
        scope
            .reference_members()
            .values()
            .copied()
            .collect::<Vec<_>>(),
        [55]
    );
    assert_eq!(
        scope
            .reference_members()
            .offsets()
            .copied()
            .collect::<Vec<_>>(),
        [u64_from_index(reference_at)]
    );
    assert_eq!(scope.frame_length(), u64_from_index(paired_at));
    assert_eq!(scope.paired_class_tag.as_str(), "261");
    assert_eq!(scope.paired_byte_offset(), u64_from_index(paired_at));
    let discovered =
        crate::design::decode::scopes::parameter_scope::parameter_scope_candidate_headers(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        )
        .unwrap()
        .into_iter()
        .filter_map(|header| {
            parse_parameter_scope(
                &bytes,
                &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
                header.record_index,
                &header.class_tag,
                header.byte_offset,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].record_index, 12);

    let mut compact_tail = bytes.clone();
    compact_tail.remove(paired_at - 1);
    let compact = parse_parameter_scope(
        &compact_tail,
        &crate::design::test_support::indexed_record_offsets_for_test(&compact_tail),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("scope with compact fixed tail");
    assert_eq!(
        compact.kind(),
        crate::records::feature::scope::DesignFeatureKind::Sketch
    );
    assert_eq!(compact.frame_length(), u64_from_index(paired_at) - 1);
    assert_eq!(compact.previous_history_state_id(), Some(2));
    assert!(
        !crate::design::decode::scopes::parameter_scope::parameter_scope_tail_length_is_valid(
            "CopyPasteBodies",
            78,
        )
    );

    for tail_length in [72, 76] {
        let mut legacy = bytes[..feature_ordinal_at].to_vec();
        let mut tail = vec![0; tail_length];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[30..34].copy_from_slice(&2u32.to_le_bytes());
        legacy.extend_from_slice(&tail);
        legacy.extend_from_slice(&3u32.to_le_bytes());
        legacy.extend_from_slice(b"261");
        legacy.extend_from_slice(&12u32.to_le_bytes());
        let decoded = parse_parameter_scope(
            &legacy,
            &crate::design::test_support::indexed_record_offsets_for_test(&legacy),
            header.record_index,
            &header.class_tag,
            header.byte_offset,
        )
        .unwrap()
        .expect("scope with legacy fixed tail");
        assert_eq!(
            decoded.kind(),
            crate::records::feature::scope::DesignFeatureKind::Sketch
        );
        assert_eq!(decoded.previous_history_state_id(), Some(2));
        assert_eq!(
            decoded.previous_history_state_id_offset(),
            Some(u64_from_index(feature_ordinal_at + 30))
        );
    }

    for (second_reference, previous_at) in [(true, 41), (false, 31)] {
        let mut extended_tail = bytes[..feature_ordinal_at].to_vec();
        let mut tail = [0; 87];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[19] = 1;
        tail[20..24].copy_from_slice(&40u32.to_le_bytes());
        if second_reference {
            tail[30] = 1;
            tail[31..35].copy_from_slice(&41u32.to_le_bytes());
        }
        tail[previous_at..previous_at + 4].copy_from_slice(&3u32.to_le_bytes());
        extended_tail.extend_from_slice(&tail);
        extended_tail.extend_from_slice(&3u32.to_le_bytes());
        extended_tail.extend_from_slice(b"261");
        extended_tail.extend_from_slice(&12u32.to_le_bytes());
        let extended = parse_parameter_scope(
            &extended_tail,
            &crate::design::test_support::indexed_record_offsets_for_test(&extended_tail),
            header.record_index,
            &header.class_tag,
            header.byte_offset,
        )
        .unwrap()
        .expect("scope with extended fixed tail");
        assert_eq!(extended.previous_history_state_id(), Some(3));
        assert_eq!(
            extended.previous_history_state_id_offset(),
            Some(u64_from_index(feature_ordinal_at + previous_at))
        );
    }

    for tail_length in [82, 104] {
        let mut variant = bytes[..feature_ordinal_at].to_vec();
        let mut tail = vec![0; tail_length];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        variant.extend_from_slice(&tail);
        variant.extend_from_slice(&3u32.to_le_bytes());
        variant.extend_from_slice(b"261");
        variant.extend_from_slice(&12u32.to_le_bytes());
        let decoded = parse_parameter_scope(
            &variant,
            &crate::design::test_support::indexed_record_offsets_for_test(&variant),
            header.record_index,
            &header.class_tag,
            header.byte_offset,
        )
        .unwrap()
        .expect("scope with extended no-history fixed tail");
        assert_eq!(
            decoded.kind(),
            crate::records::feature::scope::DesignFeatureKind::Sketch
        );
        assert_eq!(decoded.previous_history_state_id(), None);
        assert_eq!(decoded.previous_history_state_id_offset(), None);
    }

    let mut copy_scope = Vec::new();
    copy_scope.extend_from_slice(&3u32.to_le_bytes());
    copy_scope.extend_from_slice(b"316");
    copy_scope.extend_from_slice(&12u32.to_le_bytes());
    copy_scope.extend_from_slice(&[0; 10]);
    copy_scope.extend_from_slice(&1u32.to_le_bytes());
    copy_scope.push(1);
    copy_scope.extend_from_slice(&55u32.to_le_bytes());
    copy_scope.extend_from_slice(&[0; 6]);
    copy_scope.extend_from_slice(&u32::MAX.to_le_bytes());
    lp_utf16(&mut copy_scope, "CopyPasteBodies");
    let copy_feature_ordinal_at = copy_scope.len();
    let mut copy_tail = [0; 110];
    copy_tail[0..4].copy_from_slice(&2u32.to_le_bytes());
    copy_tail[53..57].copy_from_slice(&u32::MAX.to_le_bytes());
    copy_scope.extend_from_slice(&copy_tail);
    let copy_paired_at = copy_scope.len();
    copy_scope.extend_from_slice(&3u32.to_le_bytes());
    copy_scope.extend_from_slice(b"259");
    copy_scope.extend_from_slice(&12u32.to_le_bytes());
    let copy = parse_parameter_scope(
        &copy_scope,
        &crate::design::test_support::indexed_record_offsets_for_test(&copy_scope),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("CopyPasteBodies scope with extended tail");
    assert_eq!(
        copy.kind(),
        crate::records::feature::scope::DesignFeatureKind::CopyPasteBodies
    );
    assert_eq!(copy.feature_ordinal.get(), 2);
    assert_eq!(
        copy.feature_ordinal_offset(),
        u64_from_index(copy_feature_ordinal_at)
    );
    assert_eq!(copy.history_state_id(), None);
    assert_eq!(copy.previous_history_state_id(), None);
    assert_eq!(
        copy.previous_history_state_id_offset(),
        Some(u64_from_index(copy_feature_ordinal_at + 53))
    );
    assert_eq!(copy.frame_length(), u64_from_index(copy_paired_at));

    let mut operation_bytes = vec![0; 148];
    operation_bytes[29] = 1;
    operation_bytes[30..34].copy_from_slice(&55u32.to_le_bytes());
    operation_bytes[34..40].fill(0);
    operation_bytes[40] = 1;
    operation_bytes[41..45].copy_from_slice(&44u32.to_le_bytes());
    operation_bytes[45..51].fill(0);
    let body_group_at = operation_bytes.len();
    operation_bytes.extend_from_slice(&3u32.to_le_bytes());
    operation_bytes.extend_from_slice(b"264");
    operation_bytes.extend_from_slice(&55u32.to_le_bytes());
    operation_bytes.extend_from_slice(&[0; 10]);
    operation_bytes.extend_from_slice(&1u32.to_le_bytes());
    operation_bytes.push(1);
    operation_bytes.extend_from_slice(&66u32.to_le_bytes());
    operation_bytes.extend_from_slice(&[0; 6]);
    let relation_at = operation_bytes.len();
    operation_bytes.extend_from_slice(&3u32.to_le_bytes());
    operation_bytes.extend_from_slice(b"314");
    operation_bytes.extend_from_slice(&44u32.to_le_bytes());
    operation_bytes.extend_from_slice(&[0; 8]);
    operation_bytes.push(1);
    operation_bytes.extend_from_slice(&2u32.to_le_bytes());
    for suffix in [1206, 1215] {
        operation_bytes.push(1);
        operation_bytes.extend_from_slice(&u32::to_le_bytes(suffix));
        operation_bytes.extend_from_slice(&[0; 10]);
    }
    let mut operation_scope = copy.clone();
    operation_scope
        .try_edit(|draft| {
            draft.byte_offset = 0;
            draft.paired_byte_offset = 128;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![55, 66]);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let operation =
        crate::design::decode::scopes::copy_paste_bodies::exact_copy_paste_bodies_operation(
            &operation_bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&operation_bytes),
            &operation_scope,
        )
        .expect("single-body CopyPasteBodies relation");
    assert_eq!(operation.body_group_record_index, 55);
    assert_eq!(
        operation.body_group_byte_offset(),
        u64_from_index(body_group_at)
    );
    assert_eq!(
        operation
            .bodies()
            .iter()
            .map(|body| body.operand.value)
            .collect::<Vec<_>>(),
        [66]
    );
    assert_eq!(operation.relation_record_index, 44);
    assert_eq!(
        operation.relation_byte_offset(),
        u64_from_index(relation_at)
    );
    assert_eq!(
        operation
            .bodies()
            .iter()
            .map(|body| body.source.value)
            .collect::<Vec<_>>(),
        [1206]
    );
    assert_eq!(
        operation
            .bodies()
            .iter()
            .map(|body| body.copied.value)
            .collect::<Vec<_>>(),
        [1215]
    );

    // A Sketch scope may also carry the generic ordered reference table
    // used by `EntityGenesis`-form streams; the table then has more than
    // one member and the entity join happens by unique suffix match.
    let mut generic_reference = vec![1];
    generic_reference.extend_from_slice(&56u32.to_le_bytes());
    generic_reference.extend_from_slice(&[0; 6]);
    let mut generic_references = bytes.clone();
    generic_references[reference_count_at..reference_count_at + 4]
        .copy_from_slice(&2u32.to_le_bytes());
    generic_references.splice(reference_at + 10..reference_at + 10, generic_reference);
    let generic_scope = parse_parameter_scope(
        &generic_references,
        &crate::design::test_support::indexed_record_offsets_for_test(&generic_references),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("generic-table Sketch scope");
    assert_eq!(
        generic_scope.kind(),
        crate::records::feature::scope::DesignFeatureKind::Sketch
    );
    assert_eq!(
        generic_scope
            .reference_members()
            .values()
            .copied()
            .collect::<Vec<_>>(),
        [55, 56]
    );

    let work_plane_at = bytes.len();
    let mut work_plane = vec![0; 362];
    work_plane[0..4].copy_from_slice(&3u32.to_le_bytes());
    work_plane[4..7].copy_from_slice(b"293");
    work_plane[7..11].copy_from_slice(&55u32.to_le_bytes());
    work_plane[55] = 1;
    work_plane[57] = 1;
    work_plane[58..62].copy_from_slice(&99u32.to_le_bytes());
    let transform: [[f64; 4]; 4] = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 76 + ordinal * 8;
        work_plane[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    work_plane.extend_from_slice(&3u32.to_le_bytes());
    work_plane.extend_from_slice(b"261");
    work_plane.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&work_plane);
    let decoded = exact_work_plane_frame(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
    )
    .expect("exact WorkPlane frame");
    assert_eq!(decoded.transform, transform.try_into().unwrap());
    assert_eq!(decoded.transform_offset, u64_from_index(work_plane_at + 76));
    assert_eq!(
        decoded.reference,
        Some((99, u64_from_index(work_plane_at + 58)))
    );

    let extended_at = bytes.len();
    let mut extended = vec![0; 373];
    extended[0..4].copy_from_slice(&3u32.to_le_bytes());
    extended[4..7].copy_from_slice(b"263");
    extended[7..11].copy_from_slice(&57u32.to_le_bytes());
    extended[55..58].copy_from_slice(&[1, 0, 1]);
    extended[58..62].copy_from_slice(&100u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 76 + ordinal * 8;
        extended[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    extended.extend_from_slice(&3u32.to_le_bytes());
    extended.extend_from_slice(b"261");
    extended.extend_from_slice(&57u32.to_le_bytes());
    bytes.extend_from_slice(&extended);
    let mut extended_scope = scope.clone();
    extended_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![57]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = exact_work_plane_frame(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &extended_scope,
    )
    .expect("extended referenced WorkPlane frame");
    assert_eq!(decoded.transform, transform.try_into().unwrap());
    assert_eq!(decoded.transform_offset, u64_from_index(extended_at + 76));
    assert_eq!(
        decoded.reference,
        Some((100, u64_from_index(extended_at + 58)))
    );

    let direct_at = bytes.len();
    let mut direct = vec![0; 352];
    direct[0..4].copy_from_slice(&3u32.to_le_bytes());
    direct[4..7].copy_from_slice(b"293");
    direct[7..11].copy_from_slice(&56u32.to_le_bytes());
    direct[55] = 1;
    direct[62..64].copy_from_slice(&0x4091u16.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 66 + ordinal * 8;
        direct[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    direct.extend_from_slice(&3u32.to_le_bytes());
    direct.extend_from_slice(b"261");
    direct.extend_from_slice(&56u32.to_le_bytes());
    bytes.extend_from_slice(&direct);
    let mut direct_scope = scope.clone();
    direct_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![56]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = exact_work_plane_frame(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &direct_scope,
    )
    .expect("direct WorkPlane frame");
    assert_eq!(decoded.transform, transform.try_into().unwrap());
    assert_eq!(decoded.transform_offset, u64_from_index(direct_at + 66));
    assert_eq!(decoded.reference, None);

    let extended_direct_at = bytes.len();
    let mut extended_direct = vec![0; 363];
    extended_direct[0..4].copy_from_slice(&3u32.to_le_bytes());
    extended_direct[4..7].copy_from_slice(b"289");
    extended_direct[7..11].copy_from_slice(&61u32.to_le_bytes());
    extended_direct[55] = 1;
    extended_direct[62..64].copy_from_slice(&0x309cu16.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 66 + ordinal * 8;
        extended_direct[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    extended_direct.extend_from_slice(&3u32.to_le_bytes());
    extended_direct.extend_from_slice(b"258");
    extended_direct.extend_from_slice(&61u32.to_le_bytes());
    bytes.extend_from_slice(&extended_direct);
    let mut extended_direct_scope = scope.clone();
    extended_direct_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![61]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = exact_work_plane_frame(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &extended_direct_scope,
    )
    .expect("extended direct WorkPlane frame");
    assert_eq!(decoded.transform, transform.try_into().unwrap());
    assert_eq!(
        decoded.transform_offset,
        u64_from_index(extended_direct_at + 66)
    );
    assert_eq!(decoded.reference, None);

    let large_direct_at = bytes.len();
    let mut large_direct = vec![0; 374];
    large_direct[0..4].copy_from_slice(&3u32.to_le_bytes());
    large_direct[4..7].copy_from_slice(b"267");
    large_direct[7..11].copy_from_slice(&62u32.to_le_bytes());
    large_direct[55] = 1;
    large_direct[62..64].copy_from_slice(&0x904du16.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 66 + ordinal * 8;
        large_direct[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    large_direct.extend_from_slice(&3u32.to_le_bytes());
    large_direct.extend_from_slice(b"258");
    large_direct.extend_from_slice(&62u32.to_le_bytes());
    bytes.extend_from_slice(&large_direct);
    let mut large_direct_scope = scope.clone();
    large_direct_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![62]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = exact_work_plane_frame(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &large_direct_scope,
    )
    .expect("large direct WorkPlane frame");
    assert_eq!(decoded.transform, transform.try_into().unwrap());
    assert_eq!(
        decoded.transform_offset,
        u64_from_index(large_direct_at + 66)
    );
    assert_eq!(decoded.reference, None);

    for (frame_offset, test_scope) in [
        (direct_at, &direct_scope),
        (extended_direct_at, &extended_direct_scope),
        (large_direct_at, &large_direct_scope),
    ] {
        for corrupt_offset in [
            frame_offset + 56,
            frame_offset + 61,
            frame_offset + 64,
            frame_offset + 65,
        ] {
            let mut corrupted = bytes.clone();
            corrupted[corrupt_offset] = 1;
            assert_eq!(
                exact_work_plane_frame(
                    &corrupted,
                    &crate::design::test_support::indexed_record_offsets_for_test(&corrupted),
                    test_scope,
                ),
                None,
                "corrupt byte at offset {corrupt_offset} should refuse direct WorkPlane frame"
            );
        }

        let mut non_finite = bytes.clone();
        non_finite[frame_offset + 66..frame_offset + 74].copy_from_slice(&f64::NAN.to_le_bytes());
        assert_eq!(
            exact_work_plane_frame(
                &non_finite,
                &crate::design::test_support::indexed_record_offsets_for_test(&non_finite),
                test_scope,
            ),
            None,
            "non-finite matrix at offset {frame_offset} should refuse WorkPlane frame"
        );
    }

    let mut alt_class = bytes.clone();
    alt_class[direct_at + 4..direct_at + 7].copy_from_slice(b"316");
    alt_class[direct_at + 352 + 4..direct_at + 352 + 7].copy_from_slice(b"260");
    let decoded_alt = exact_work_plane_frame(
        &alt_class,
        &crate::design::test_support::indexed_record_offsets_for_test(&alt_class),
        &direct_scope,
    )
    .expect("class-316/260 direct WorkPlane frame");
    assert_eq!(decoded_alt.transform, transform.try_into().unwrap());
    assert_eq!(decoded_alt.transform_offset, u64_from_index(direct_at + 66));
    assert_eq!(decoded_alt.reference, None);

    let mut corrupt_class = bytes.clone();
    corrupt_class[direct_at + 4..direct_at + 7].copy_from_slice(b"999");
    assert_eq!(
        exact_work_plane_frame(
            &corrupt_class,
            &crate::design::test_support::indexed_record_offsets_for_test(&corrupt_class),
            &direct_scope,
        ),
        None,
        "unadmitted class tag should refuse direct WorkPlane frame"
    );

    let mut axis_bytes = vec![0; 232];
    axis_bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    axis_bytes[4..7].copy_from_slice(b"701");
    axis_bytes[7..11].copy_from_slice(&100u32.to_le_bytes());
    axis_bytes[21..25].copy_from_slice(&8u32.to_le_bytes());
    let axis_values = [1.0_f64, 2.0, 3.0, 0.0, -3.0, 4.0, 0.0, 0.0];
    for (ordinal, value) in axis_values.into_iter().enumerate() {
        let at = 25 + ordinal * 8;
        axis_bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    axis_bytes[118..122].copy_from_slice(&2u32.to_le_bytes());
    for (ordinal, record_index) in [102_u32, 104].into_iter().enumerate() {
        let at = 122 + ordinal * 11;
        axis_bytes[at] = 1;
        axis_bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    }
    axis_bytes.extend_from_slice(&3u32.to_le_bytes());
    axis_bytes.extend_from_slice(b"258");
    axis_bytes.extend_from_slice(&100u32.to_le_bytes());
    for (record_index, point) in [(102_u32, [1.0_f64, 2.0, 3.0]), (104, [1.0, -1.0, 7.0])] {
        let start = axis_bytes.len();
        axis_bytes.resize(start + 197, 0);
        axis_bytes[start..start + 4].copy_from_slice(&3u32.to_le_bytes());
        axis_bytes[start + 4..start + 7].copy_from_slice(b"702");
        axis_bytes[start + 7..start + 11].copy_from_slice(&record_index.to_le_bytes());
        for (ordinal, value) in point.into_iter().enumerate() {
            let at = start + 42 + ordinal * 8;
            axis_bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        axis_bytes.extend_from_slice(&3u32.to_le_bytes());
        axis_bytes.extend_from_slice(b"258");
        axis_bytes.extend_from_slice(&record_index.to_le_bytes());
    }
    let mut axis_scope = scope.clone();
    axis_scope.id = "f3d:native/BulkStream.dat:parameter-scope#55".into();
    axis_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::WorkAxis
                .try_into()
                .unwrap();
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![100, 101, 102, 103, 104]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let construction = exact_work_axis_construction(
        &axis_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&axis_bytes),
        &axis_scope,
    )
    .expect("exact two-point WorkAxis construction");
    assert_eq!(
        construction.origin.map(cadmpeg_ir::scalar::FiniteReal::get),
        [1.0, 2.0, 3.0]
    );
    assert_eq!(
        construction
            .displacement
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        [0.0, -3.0, 4.0]
    );
    assert_eq!(construction.origin_offset, 25);
    assert_eq!(construction.displacement_offset, 49);
    assert!(matches!(
        construction.source,
        Some(
            crate::records::feature::work_geometry::DesignWorkAxisSource::TwoPoint {
                point_record_indices: [102, 104],
                ..
            }
        )
    ));
    if let crate::records::feature::scope::DesignScopePayloadMut::WorkAxis(slot) =
        axis_scope.payload_mut()
    {
        *slot = Some(construction);
    }
    let crate::design::feature_project::ProjectedParameterDesign {
        features: axis_features,
        ..
    } = crate::test_support::with_decode_context(|ctx| {
        let scopes = std::slice::from_ref(&axis_scope);
        let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                scopes,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .expect("test projection has a synthetic exact timeline")
    });
    assert!(matches!(
        axis_features.as_slice(), [Feature {
            evaluation,
            ..
        }] if matches!((evaluation.definition(),), (FeatureDefinition::Operation(FeatureOperation::DatumAxis { origin, direction }),) if *origin == Point3::new(10.0, 20.0, 30.0)
            && *direction == Vector3::new(0.0, -0.6, 0.8))));

    let compact_at = bytes.len();
    let mut compact = vec![0; 321];
    compact[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact[4..7].copy_from_slice(b"293");
    compact[7..11].copy_from_slice(&58u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 49 + ordinal * 8;
        compact[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact.extend_from_slice(&3u32.to_le_bytes());
    compact.extend_from_slice(b"261");
    compact.extend_from_slice(&58u32.to_le_bytes());
    bytes.extend_from_slice(&compact);
    let mut compact_scope = scope.clone();
    compact_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![58]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = exact_work_plane_frame(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &compact_scope,
    )
    .expect("compact direct WorkPlane frame");
    assert_eq!(decoded.transform, transform.try_into().unwrap());
    assert_eq!(decoded.transform_offset, u64_from_index(compact_at + 49));
    assert_eq!(decoded.reference, None);

    let compact_431_at = bytes.len();
    let mut compact_431 = vec![0; 325];
    compact_431[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_431[4..7].copy_from_slice(b"431");
    compact_431[7..11].copy_from_slice(&67u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 49 + ordinal * 8;
        compact_431[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_431.extend_from_slice(&3u32.to_le_bytes());
    compact_431.extend_from_slice(b"257");
    compact_431.extend_from_slice(&67u32.to_le_bytes());
    bytes.extend_from_slice(&compact_431);
    let mut compact_431_scope = scope.clone();
    compact_431_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![67]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = exact_work_plane_frame(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &compact_431_scope,
    )
    .expect("class-431 compact direct WorkPlane frame");
    assert_eq!(decoded.transform, transform.try_into().unwrap());
    assert_eq!(
        decoded.transform_offset,
        u64_from_index(compact_431_at + 49)
    );
    assert_eq!(decoded.reference, None);

    let compact_364_at = bytes.len();
    let mut compact_364 = vec![0; 321];
    compact_364[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_364[4..7].copy_from_slice(b"364");
    compact_364[7..11].copy_from_slice(&65u32.to_le_bytes());
    compact_364[46] = 1;
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 49 + ordinal * 8;
        compact_364[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_364.extend_from_slice(&3u32.to_le_bytes());
    compact_364.extend_from_slice(b"264");
    compact_364.extend_from_slice(&65u32.to_le_bytes());
    bytes.extend_from_slice(&compact_364);
    let mut compact_364_scope = scope.clone();
    compact_364_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![65]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = exact_work_plane_frame(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &compact_364_scope,
    )
    .expect("class-364 marked compact direct WorkPlane frame");
    assert_eq!(decoded.transform, transform.try_into().unwrap());
    assert_eq!(
        decoded.transform_offset,
        u64_from_index(compact_364_at + 49)
    );
    assert_eq!(decoded.reference, None);

    let compact_364_variant_at = bytes.len();
    let mut compact_364_variant = vec![0; 321];
    compact_364_variant[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_364_variant[4..7].copy_from_slice(b"364");
    compact_364_variant[7..11].copy_from_slice(&66u32.to_le_bytes());
    compact_364_variant[45..49].copy_from_slice(&[0xcc, 0xcd, 0, 0]);
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 49 + ordinal * 8;
        compact_364_variant[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_364_variant.extend_from_slice(&3u32.to_le_bytes());
    compact_364_variant.extend_from_slice(b"264");
    compact_364_variant.extend_from_slice(&66u32.to_le_bytes());
    bytes.extend_from_slice(&compact_364_variant);
    let mut compact_364_variant_scope = scope.clone();
    compact_364_variant_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![66]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = exact_work_plane_frame(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &compact_364_variant_scope,
    )
    .expect("class-364 compact direct WorkPlane frame variant");
    assert_eq!(decoded.transform, transform.try_into().unwrap());
    assert_eq!(
        decoded.transform_offset,
        u64_from_index(compact_364_variant_at + 49)
    );
    assert_eq!(decoded.reference, None);

    let compact_450_at = bytes.len();
    let mut compact_450 = vec![0; 326];
    compact_450[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_450[4..7].copy_from_slice(b"450");
    compact_450[7..11].copy_from_slice(&59u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 50 + ordinal * 8;
        compact_450[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_450.extend_from_slice(&3u32.to_le_bytes());
    compact_450.extend_from_slice(b"259");
    compact_450.extend_from_slice(&59u32.to_le_bytes());
    bytes.extend_from_slice(&compact_450);
    let mut compact_450_scope = scope.clone();
    compact_450_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![59]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = exact_work_plane_frame(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &compact_450_scope,
    )
    .expect("class-450 compact direct WorkPlane frame");
    assert_eq!(decoded.transform, transform.try_into().unwrap());
    assert_eq!(
        decoded.transform_offset,
        u64_from_index(compact_450_at + 50)
    );
    assert_eq!(decoded.reference, None);

    let class_279_at = bytes.len();
    let mut class_279 = vec![0; 326];
    class_279[0..4].copy_from_slice(&3u32.to_le_bytes());
    class_279[4..7].copy_from_slice(b"279");
    class_279[7..11].copy_from_slice(&69u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 50 + ordinal * 8;
        class_279[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    class_279.extend_from_slice(&3u32.to_le_bytes());
    class_279.extend_from_slice(b"266");
    class_279.extend_from_slice(&69u32.to_le_bytes());
    bytes.extend_from_slice(&class_279);
    let mut class_279_scope = scope.clone();
    class_279_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![69]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = exact_work_plane_frame(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &class_279_scope,
    )
    .expect("class-279 compact direct WorkPlane frame");
    assert_eq!(decoded.transform, transform.try_into().unwrap());
    assert_eq!(decoded.transform_offset, u64_from_index(class_279_at + 50));
    assert_eq!(decoded.reference, None);

    let compact_409_short_at = bytes.len();
    let mut compact_409_short = compact_450.clone();
    compact_409_short[4..7].copy_from_slice(b"409");
    compact_409_short[7..11].copy_from_slice(&64u32.to_le_bytes());
    compact_409_short[330..333].copy_from_slice(b"258");
    compact_409_short[333..337].copy_from_slice(&64u32.to_le_bytes());
    bytes.extend_from_slice(&compact_409_short);
    let mut compact_409_short_scope = scope.clone();
    compact_409_short_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![64]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = exact_work_plane_frame(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &compact_409_short_scope,
    )
    .expect("short class-409 compact direct WorkPlane frame");
    assert_eq!(decoded.transform, transform.try_into().unwrap());
    assert_eq!(
        decoded.transform_offset,
        u64_from_index(compact_409_short_at + 50)
    );
    assert_eq!(decoded.reference, None);

    let compact_409_at = bytes.len();
    let mut compact_409 = vec![0; 337];
    compact_409[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_409[4..7].copy_from_slice(b"409");
    compact_409[7..11].copy_from_slice(&63u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 50 + ordinal * 8;
        compact_409[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_409.extend_from_slice(&3u32.to_le_bytes());
    compact_409.extend_from_slice(b"258");
    compact_409.extend_from_slice(&63u32.to_le_bytes());
    bytes.extend_from_slice(&compact_409);
    let mut compact_409_scope = scope.clone();
    compact_409_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![63]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = exact_work_plane_frame(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &compact_409_scope,
    )
    .expect("class-409 compact direct WorkPlane frame");
    assert_eq!(decoded.transform, transform.try_into().unwrap());
    assert_eq!(
        decoded.transform_offset,
        u64_from_index(compact_409_at + 50)
    );
    assert_eq!(decoded.reference, None);

    let joint_origin_at = bytes.len();
    let mut joint_origin = vec![0; 336];
    joint_origin[0..4].copy_from_slice(&3u32.to_le_bytes());
    joint_origin[4..7].copy_from_slice(b"450");
    joint_origin[7..11].copy_from_slice(&60u32.to_le_bytes());
    joint_origin[45] = 1;
    joint_origin[46..50].copy_from_slice(&61u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 60 + ordinal * 8;
        joint_origin[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    joint_origin.extend_from_slice(&3u32.to_le_bytes());
    joint_origin.extend_from_slice(b"259");
    joint_origin.extend_from_slice(&60u32.to_le_bytes());
    bytes.extend_from_slice(&joint_origin);
    let mut joint_origin_scope = scope.clone();
    joint_origin_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::JointOrigin
                .try_into()
                .unwrap();
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![60]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = exact_joint_origin_frame(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &joint_origin_scope,
    )
    .expect("exact JointOrigin frame");
    assert_eq!(decoded.transform, transform.try_into().unwrap());
    assert_eq!(
        decoded.transform_offset,
        u64_from_index(joint_origin_at + 60)
    );
    assert_eq!(
        decoded.reference,
        Some((61, u64_from_index(joint_origin_at + 46)))
    );

    for frame_length in [300, 322, 344] {
        let mut construction_scope = joint_origin_scope.clone();
        construction_scope
            .try_edit(|draft| {
                draft.frame_length = frame_length;
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_tail();
            })
            .unwrap();
        assert!(
            exact_joint_origin_frame(
                &bytes,
                &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
                &construction_scope,
            )
            .is_none(),
            "construction envelope {frame_length} must defer its solved frame to Assemble"
        );
    }

    let compact_joint_origin_at = bytes.len();
    let mut compact_joint_origin = vec![0; 385];
    compact_joint_origin[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_joint_origin[4..7].copy_from_slice(b"364");
    compact_joint_origin[7..11].copy_from_slice(&67u32.to_le_bytes());
    compact_joint_origin[45..49].copy_from_slice(&[1, 1, 0, 0]);
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 49 + ordinal * 8;
        compact_joint_origin[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_joint_origin.extend_from_slice(&3u32.to_le_bytes());
    compact_joint_origin.extend_from_slice(b"264");
    compact_joint_origin.extend_from_slice(&67u32.to_le_bytes());
    bytes.extend_from_slice(&compact_joint_origin);
    let mut compact_joint_origin_scope = scope.clone();
    compact_joint_origin_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::JointOrigin
                .try_into()
                .unwrap();
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![67]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = exact_joint_origin_frame(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &compact_joint_origin_scope,
    )
    .expect("exact compact JointOrigin frame");
    assert_eq!(decoded.transform, transform.try_into().unwrap());
    assert_eq!(
        decoded.transform_offset,
        u64_from_index(compact_joint_origin_at + 49)
    );
    assert_eq!(decoded.reference, None);

    let legacy_joint_origin_at = bytes.len();
    let mut legacy_joint_origin = vec![0; joint_origin_class_337_266::LEN];
    legacy_joint_origin[0..4].copy_from_slice(&3u32.to_le_bytes());
    legacy_joint_origin[4..7].copy_from_slice(b"337");
    legacy_joint_origin[7..11].copy_from_slice(&72u32.to_le_bytes());
    legacy_joint_origin
        [joint_origin_class_337_266::MATRIX_PREFIX..joint_origin_class_337_266::MATRIX]
        .copy_from_slice(&joint_origin_class_337_266::MATRIX_PREFIX_VALUE);
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = joint_origin_class_337_266::MATRIX + ordinal * 8;
        legacy_joint_origin[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    legacy_joint_origin.extend_from_slice(&3u32.to_le_bytes());
    legacy_joint_origin.extend_from_slice(b"266");
    legacy_joint_origin.extend_from_slice(&72u32.to_le_bytes());
    bytes.extend_from_slice(&legacy_joint_origin);
    let mut legacy_joint_origin_scope = scope.clone();
    legacy_joint_origin_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::JointOrigin
                .try_into()
                .unwrap();
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![72]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = exact_joint_origin_frame(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &legacy_joint_origin_scope,
    )
    .expect("exact class-337/266 JointOrigin frame");
    assert_eq!(decoded.transform, transform.try_into().unwrap());
    assert_eq!(
        decoded.transform_offset,
        u64_from_index(legacy_joint_origin_at + joint_origin_class_337_266::MATRIX)
    );
    assert_eq!(decoded.reference, None);

    let mut invalid_legacy_joint_origin = legacy_joint_origin.clone();
    invalid_legacy_joint_origin[joint_origin_class_337_266::MATRIX_PREFIX] = 0;
    let mut invalid_bytes = bytes[..legacy_joint_origin_at].to_vec();
    invalid_bytes.extend_from_slice(&invalid_legacy_joint_origin);
    assert_eq!(
        exact_joint_origin_frame(
            &invalid_bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&invalid_bytes),
            &legacy_joint_origin_scope,
        ),
        None
    );

    (bytes, scope, transform)
}

mod work_planes;
