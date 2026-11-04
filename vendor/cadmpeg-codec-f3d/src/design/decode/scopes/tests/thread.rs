// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::scopes::parameter_scope::parse_parameter_scope;
use crate::design::decode::scopes::thread::ThreadPrefix;
use crate::design::decode::scopes::thread::{exact_thread_construction, parse_thread_payload};
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::thread::{DesignThreadConstruction, DesignThreadForm};
use crate::test_support::lp_utf16;

#[test]
fn thread_scope_decodes_standard_size_and_face_group() {
    let mut bytes = vec![0; 148];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"335");
    bytes[7..11].copy_from_slice(&987u32.to_le_bytes());
    bytes[21..29].copy_from_slice(&60.0f64.to_le_bytes());
    bytes[29..34].copy_from_slice(&[1, 2, 0, 0, 0]);
    bytes[34..38].copy_from_slice(&[0x36, 0, 0x67, 0]);
    let mut payload = Vec::new();
    lp_utf16(&mut payload, "M30x3.5");
    lp_utf16(&mut payload, "30.0");
    lp_utf16(&mut payload, "ISO Metric profile");
    assert_eq!(payload.len(), 70);
    bytes[38..108].copy_from_slice(&payload);
    bytes[108..113].copy_from_slice(&[0, 1, 0, 0, 0]);
    bytes[113..121].copy_from_slice(&2.97345f64.to_le_bytes());
    bytes[121..129].copy_from_slice(&2.5732f64.to_le_bytes());
    bytes[129] = 1;
    bytes[130..138].copy_from_slice(&0.35f64.to_le_bytes());
    bytes[138..146].copy_from_slice(&2.7568f64.to_le_bytes());
    bytes[146..148].copy_from_slice(&[0, 1]);

    let expected = DesignThreadConstruction {
        form: DesignThreadForm::Standard,
        designation_offset: 38,
        designation: cadmpeg_core::text::NonBlankString::new("M30x3.5").unwrap(),
        nominal_size: crate::records::feature::thread::DesignThreadNominalSize::try_from(
            "30.0".to_owned(),
        )
        .expect("nominal size"),
        profile: cadmpeg_core::text::NonBlankString::new("ISO Metric profile").unwrap(),
        pitch: cadmpeg_ir::scalar::PositiveReal::new(0.35).unwrap(),
        face_group_record_indices: vec![988],
        diameters: crate::records::feature::thread::DesignThreadDiameters::new(
            2.97345, 2.5732, 2.7568,
        )
        .unwrap(),
        extended: None,
    };
    assert_thread_construction(
        parse_thread_payload(&bytes, 38, &ThreadPrefix::Standard, vec![988]),
        &expected,
    );
    let mut invalid_standard_pitch_marker = bytes.clone();
    invalid_standard_pitch_marker[129] = 0;
    assert_eq!(
        parse_thread_payload(
            &invalid_standard_pitch_marker,
            38,
            &ThreadPrefix::Standard,
            vec![988]
        ),
        None
    );

    let mut scope = DesignParameterScope::empty(
        "f3d:scope#standard-thread",
        crate::records::feature::scope::DesignFeatureKind::Thread,
        987,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("901".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("902".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 200;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![988, 989]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_thread_construction(exact_thread_construction(&bytes, &scope, &[]), &expected);

    let mut owner_marked = bytes;
    owner_marked.splice(20..20, [1, 0, 0, 0]);
    let shifted_expected =
        parse_thread_payload(&owner_marked, 42, &ThreadPrefix::Standard, vec![988])
            .expect("owner-marked standard Thread payload");
    assert_eq!(shifted_expected.designation_offset, 42);
    scope
        .try_edit(|draft| {
            draft.frame_length += 4;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_thread_construction(
        exact_thread_construction(&owner_marked, &scope, &[]),
        &shifted_expected,
    );
    let mut invalid_owner_marker = owner_marked.clone();
    invalid_owner_marker[20..24].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        exact_thread_construction(&invalid_owner_marker, &scope, &[]),
        None
    );
    assert_eq!(
        parse_thread_payload(&owner_marked, 42, &ThreadPrefix::Compact, vec![988]),
        None
    );
}

#[test]
fn thread_scope_decodes_class_334_legacy_standard_tail() {
    let mut bytes = vec![0; 148];
    bytes[21..29].copy_from_slice(&60.0f64.to_le_bytes());
    bytes[29..34].copy_from_slice(&[1, 2, 0, 0, 0]);
    bytes[34..38].copy_from_slice(&[0x36, 0, 0x67, 0]);
    let mut payload = Vec::new();
    lp_utf16(&mut payload, "M7x1");
    lp_utf16(&mut payload, "7.0");
    lp_utf16(&mut payload, "ISO Metric profile");
    assert_eq!(payload.len(), 62);
    bytes[38..100].copy_from_slice(&payload);
    bytes[100..105].copy_from_slice(&[1, 1, 0, 0, 0]);
    bytes[105..113].copy_from_slice(&0.71472f64.to_le_bytes());
    bytes[113..121].copy_from_slice(&0.60355f64.to_le_bytes());
    bytes[121] = 0;
    bytes[122..130].copy_from_slice(&0.1f64.to_le_bytes());
    bytes[130..138].copy_from_slice(&0.64255f64.to_le_bytes());
    bytes[138..142].copy_from_slice(&[0, 0, 0, 1]);

    let expected = DesignThreadConstruction {
        form: DesignThreadForm::StandardLegacy,
        designation_offset: 38,
        designation: cadmpeg_core::text::NonBlankString::new("M7x1").unwrap(),
        nominal_size: crate::records::feature::thread::DesignThreadNominalSize::try_from(
            "7.0".to_owned(),
        )
        .expect("nominal size"),
        profile: cadmpeg_core::text::NonBlankString::new("ISO Metric profile").unwrap(),
        pitch: cadmpeg_ir::scalar::PositiveReal::new(0.1).unwrap(),
        face_group_record_indices: vec![988],
        diameters: crate::records::feature::thread::DesignThreadDiameters::new(
            0.71472, 0.60355, 0.64255,
        )
        .unwrap(),
        extended: None,
    };
    assert_thread_construction(
        parse_thread_payload(&bytes, 38, &ThreadPrefix::Standard, vec![988]),
        &expected,
    );

    let mut scope = DesignParameterScope::empty(
        "f3d:scope#legacy-thread",
        crate::records::feature::scope::DesignFeatureKind::Thread,
        987,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("334".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("262".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![988, 991]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_thread_construction(exact_thread_construction(&bytes, &scope, &[]), &expected);

    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("335".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
    assert_eq!(exact_thread_construction(&bytes, &scope, &[]), None);
}

/// Standard-prefix Thread bytes with the compact construction marker.
///
/// `zero_run` is the count of zero bytes before the trailer `01`; the
/// trailer is followed by u32 `1` and a marked reference to `group`.
fn standard_compact_thread_bytes(zero_run: usize, group: u32) -> Vec<u8> {
    let mut bytes = vec![0; 38];
    bytes[21..29].copy_from_slice(&60.0f64.to_le_bytes());
    bytes[29..34].copy_from_slice(&[1, 2, 0, 0, 0]);
    bytes[34..38].copy_from_slice(&[0x36, 0, 0x67, 0]);
    lp_utf16(&mut bytes, "M5x0.8");
    lp_utf16(&mut bytes, "5.0");
    lp_utf16(&mut bytes, "ISO Metric profile");
    bytes.extend_from_slice(&[1, 2, 0, 0, 0]);
    bytes.extend_from_slice(&0.4891f64.to_le_bytes());
    bytes.extend_from_slice(&0.4213f64.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&0.08f64.to_le_bytes());
    bytes.extend_from_slice(&0.4529f64.to_le_bytes());
    bytes.extend(std::iter::repeat_n(0, zero_run));
    bytes.push(1);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&group.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes
}

#[test]
fn thread_scope_decodes_standard_prefix_compact_marker_form() {
    let expected = |designation_offset: u64| DesignThreadConstruction {
        form: DesignThreadForm::StandardCompact,
        designation_offset,
        designation: cadmpeg_core::text::NonBlankString::new("M5x0.8").unwrap(),
        nominal_size: crate::records::feature::thread::DesignThreadNominalSize::try_from(
            "5.0".to_owned(),
        )
        .expect("nominal size"),
        profile: cadmpeg_core::text::NonBlankString::new("ISO Metric profile").unwrap(),
        pitch: cadmpeg_ir::scalar::PositiveReal::new(0.08).unwrap(),
        face_group_record_indices: vec![612],
        diameters: crate::records::feature::thread::DesignThreadDiameters::new(
            0.4891, 0.4213, 0.4529,
        )
        .unwrap(),
        extended: None,
    };
    for zero_run in [2, 3] {
        let bytes = standard_compact_thread_bytes(zero_run, 612);
        assert_thread_construction(
            parse_thread_payload(&bytes, 38, &ThreadPrefix::Standard, vec![612]),
            &expected(38),
        );
    }

    // The scope-level entry point admits the owner-marked prefix with the same payload.
    let mut owner_marked = standard_compact_thread_bytes(3, 612);
    owner_marked.splice(20..20, [1, 0, 0, 0]);
    let mut scope = DesignParameterScope::empty(
        "f3d:scope#standard-compact-thread",
        crate::records::feature::scope::DesignFeatureKind::Thread,
        611,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("901".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("902".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 200;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![612, 615]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_thread_construction(
        exact_thread_construction(&owner_marked, &scope, &[]),
        &expected(42),
    );
}

#[test]
fn thread_scope_refuses_standard_prefix_compact_marker_trailer_deviations() {
    let admit = |bytes: &[u8]| parse_thread_payload(bytes, 38, &ThreadPrefix::Standard, vec![612]);
    assert!(admit(&standard_compact_thread_bytes(2, 612)).is_some());
    // The zero run is two or three bytes.
    assert_eq!(admit(&standard_compact_thread_bytes(1, 612)), None);
    assert_eq!(admit(&standard_compact_thread_bytes(4, 612)), None);
    // The marked reference names the first scope reference.
    assert_eq!(admit(&standard_compact_thread_bytes(2, 613)), None);
    let base = standard_compact_thread_bytes(2, 612);
    let flag_at = base.len() - (1 + 4 + 1 + 4 + 4);
    let count_at = flag_at + 1;
    let marker_at = count_at + 4;
    assert_eq!(base[flag_at], 1);
    for (at, value) in [(flag_at, 0u8), (count_at, 2), (marker_at, 0)] {
        let mut bytes = base.clone();
        bytes[at] = value;
        assert_eq!(admit(&bytes), None, "byte {at} set to {value}");
    }
    // The pitch marker is zero.
    let pitch_marker_at = flag_at - 2 - 8 - 8 - 1;
    let mut bytes = base.clone();
    bytes[pitch_marker_at] = 1;
    assert_eq!(admit(&bytes), None);
    // The reference padding is four zero bytes.
    let mut bytes = base.clone();
    *bytes.last_mut().unwrap() = 1;
    assert_eq!(admit(&bytes), None);
    // A truncated trailer is refused.
    assert_eq!(admit(&base[..base.len() - 8]), None);
    // The compact prefix does not admit the marker with the standard trailer.
    assert_eq!(
        parse_thread_payload(&base, 38, &ThreadPrefix::Compact, vec![612]),
        None
    );
}

fn assert_thread_construction(
    actual: Option<DesignThreadConstruction>,
    expected: &DesignThreadConstruction,
) {
    let actual = actual.expect("typed Thread construction");
    assert_eq!(actual.form, expected.form);
    assert_eq!(actual.designation_offset, expected.designation_offset);
    assert_eq!(actual.designation, expected.designation);
    assert_eq!(actual.nominal_size.text(), expected.nominal_size.text());
    assert_eq!(actual.profile, expected.profile);
    assert_eq!(
        actual.face_group_record_indices,
        expected.face_group_record_indices
    );
    for (actual, expected) in [
        (
            actual.nominal_size.value().expect("actual nominal size"),
            expected
                .nominal_size
                .value()
                .expect("expected nominal size"),
        ),
        (actual.diameters.major(), expected.diameters.major()),
        (actual.diameters.minor(), expected.diameters.minor()),
        (actual.pitch.get(), expected.pitch.get()),
        (actual.diameters.pitch(), expected.diameters.pitch()),
    ] {
        assert!((actual - expected).abs() < 1.0e-12);
    }
}

#[test]
fn thread_scope_decodes_compact_preamble_and_localized_profile() {
    let mut bytes = vec![0; 160];
    let mut payload = Vec::new();
    lp_utf16(&mut payload, "M3.5x0.6");
    lp_utf16(&mut payload, "3.5");
    lp_utf16(&mut payload, "GB Metric profile");
    bytes[21..29].copy_from_slice(&60.0f64.to_le_bytes());
    bytes[29..34].copy_from_slice(&[0, 2, 0, 0, 0]);
    bytes[34..38].copy_from_slice(&[0x36, 0, 0x48, 0]);
    bytes[38..38 + payload.len()].copy_from_slice(&payload);
    let after_profile = 38 + payload.len();
    assert_eq!(after_profile, 106);
    bytes[after_profile..after_profile + 5].copy_from_slice(&[1, 2, 0, 0, 0]);
    bytes[after_profile + 5..after_profile + 13].copy_from_slice(&0.35995f64.to_le_bytes());
    bytes[after_profile + 13..after_profile + 21].copy_from_slice(&0.293f64.to_le_bytes());
    bytes[after_profile + 21] = 0;
    bytes[after_profile + 22..after_profile + 30].copy_from_slice(&0.06f64.to_le_bytes());
    bytes[after_profile + 30..after_profile + 38].copy_from_slice(&0.3166f64.to_le_bytes());
    bytes[after_profile + 38..after_profile + 42].copy_from_slice(&[0, 0, 0, 1]);

    let expected = DesignThreadConstruction {
        form: DesignThreadForm::Compact(None),
        designation_offset: 38,
        designation: cadmpeg_core::text::NonBlankString::new("M3.5x0.6").unwrap(),
        nominal_size: crate::records::feature::thread::DesignThreadNominalSize::try_from(
            "3.5".to_owned(),
        )
        .expect("nominal size"),
        profile: cadmpeg_core::text::NonBlankString::new("GB Metric profile").unwrap(),
        pitch: cadmpeg_ir::scalar::PositiveReal::new(0.06).unwrap(),
        face_group_record_indices: vec![988],
        diameters: crate::records::feature::thread::DesignThreadDiameters::new(
            0.35995, 0.293, 0.3166,
        )
        .unwrap(),
        extended: None,
    };
    assert_thread_construction(
        parse_thread_payload(&bytes, 38, &ThreadPrefix::Compact, vec![988]),
        &expected,
    );
    let mut referenced = bytes.clone();
    referenced[after_profile + 38] = 1;
    referenced[after_profile + 39..after_profile + 43].copy_from_slice(&2075u32.to_le_bytes());
    referenced[after_profile + 43..after_profile + 49].fill(0);
    let mut referenced_expected = expected.clone();
    referenced_expected.form = DesignThreadForm::Compact(Some(crate::records::identity::Located {
        value: std::num::NonZeroU32::new(2075).expect("reference"),
        offset: u64_from_index(after_profile + 39),
    }));
    assert_thread_construction(
        parse_thread_payload(&referenced, 38, &ThreadPrefix::Compact, vec![988]),
        &referenced_expected,
    );

    let mut scope = DesignParameterScope::empty(
        "f3d:scope#compact-thread",
        crate::records::feature::scope::DesignFeatureKind::Thread,
        987,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("903".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 200;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![988, 989, 992, 993]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("904".to_owned()).unwrap();
    let mut plural_expected = expected.clone();
    plural_expected.face_group_record_indices.push(992);
    assert_thread_construction(
        exact_thread_construction(&bytes, &scope, &[]),
        &plural_expected,
    );

    let mut owner_marked = bytes;
    owner_marked.splice(20..20, [1, 0, 0, 0]);
    plural_expected.designation_offset += 4;
    scope
        .try_edit(|draft| {
            draft.frame_length += 4;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_thread_construction(
        exact_thread_construction(&owner_marked, &scope, &[]),
        &plural_expected,
    );
    let mut invalid_owner_separator = owner_marked.clone();
    invalid_owner_separator[24] = 1;
    assert_eq!(
        exact_thread_construction(&invalid_owner_separator, &scope, &[]),
        None
    );

    scope
        .try_edit(|draft| {
            draft.reference_members = {
                let mut values: Vec<u32> = draft.reference_members.values().copied().collect();
                values.push(994);
                crate::records::identity::ReferenceRun::unlocated(values)
            };
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(exact_thread_construction(&owner_marked, &scope, &[]), None);
}

#[test]
fn thread_scope_decodes_class_414_legacy_compact_tail() {
    let mut bytes = vec![0; 160];
    let mut payload = Vec::new();
    lp_utf16(&mut payload, "M190x8");
    lp_utf16(&mut payload, "190.0");
    lp_utf16(&mut payload, "ISO Metric profile");
    bytes[21..29].copy_from_slice(&60.0f64.to_le_bytes());
    bytes[29..34].copy_from_slice(&[0, 2, 0, 0, 0]);
    bytes[34..38].copy_from_slice(&[0x36, 0, 0x48, 0]);
    bytes[38..38 + payload.len()].copy_from_slice(&payload);
    let after_profile = 38 + payload.len();
    bytes[after_profile..after_profile + 5].copy_from_slice(&[1, 1, 0, 0, 0]);
    bytes[after_profile + 5..after_profile + 13].copy_from_slice(&19.08149f64.to_le_bytes());
    bytes[after_profile + 13..after_profile + 21].copy_from_slice(&18.18397f64.to_le_bytes());
    bytes[after_profile + 21] = 0;
    bytes[after_profile + 22..after_profile + 30].copy_from_slice(&0.8f64.to_le_bytes());
    bytes[after_profile + 30..after_profile + 38].copy_from_slice(&18.50413f64.to_le_bytes());
    bytes[after_profile + 38..after_profile + 42].copy_from_slice(&[0, 0, 0, 1]);

    let expected = DesignThreadConstruction {
        form: DesignThreadForm::CompactLegacy,
        designation_offset: 38,
        designation: cadmpeg_core::text::NonBlankString::new("M190x8").unwrap(),
        nominal_size: crate::records::feature::thread::DesignThreadNominalSize::try_from(
            "190.0".to_owned(),
        )
        .expect("nominal size"),
        profile: cadmpeg_core::text::NonBlankString::new("ISO Metric profile").unwrap(),
        pitch: cadmpeg_ir::scalar::PositiveReal::new(0.8).unwrap(),
        face_group_record_indices: vec![988],
        diameters: crate::records::feature::thread::DesignThreadDiameters::new(
            19.08149, 18.18397, 18.50413,
        )
        .unwrap(),
        extended: None,
    };
    assert_thread_construction(
        parse_thread_payload(&bytes, 38, &ThreadPrefix::Compact, vec![988]),
        &expected,
    );

    let mut scope = DesignParameterScope::empty(
        "f3d:scope#legacy-compact-thread",
        crate::records::feature::scope::DesignFeatureKind::Thread,
        987,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("414".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("263".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 200;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![988, 989]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_thread_construction(exact_thread_construction(&bytes, &scope, &[]), &expected);

    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("334".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("262".to_owned()).unwrap();
    assert_eq!(exact_thread_construction(&bytes, &scope, &[]), None);
}

#[test]
fn localized_sketch_scope_retains_its_generic_reference_table() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"301");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for record_index in [55u32, 56] {
        bytes.push(1);
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "Esquisse");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
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
    .expect("localized Sketch scope");
    assert_eq!(
        scope.kind(),
        crate::records::feature::scope::DesignFeatureKind::Esquisse
    );
    assert_eq!(
        scope
            .reference_members()
            .values()
            .copied()
            .collect::<Vec<_>>(),
        [55, 56]
    );
    assert!(scope.sketch_entity().is_none());
}

#[test]
fn scope_without_its_own_history_state_keeps_its_predecessor() {
    let frame = |current_state: u32, previous_state: u32| {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"344");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        for record_index in [16u32, 19] {
            bytes.push(1);
            bytes.extend_from_slice(&record_index.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }
        bytes.extend_from_slice(&current_state.to_le_bytes());
        lp_utf16(&mut bytes, "Thread");
        let mut tail = [0; 87];
        tail[0..4].copy_from_slice(&3u32.to_le_bytes());
        tail[31..35].copy_from_slice(&previous_state.to_le_bytes());
        bytes.extend_from_slice(&tail);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"262");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes
    };
    let parse = |bytes: &[u8]| {
        parse_parameter_scope(
            bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(bytes),
            12,
            &crate::records::references::DesignClassTag::try_from("344".to_owned()).unwrap(),
            0,
        )
        .unwrap()
    };

    let scope = parse(&frame(u32::MAX, 9)).expect("scope that commits no state");
    assert_eq!(
        scope.kind(),
        crate::records::feature::scope::DesignFeatureKind::Thread
    );
    assert_eq!(scope.history_state_id(), None);
    assert_eq!(scope.previous_history_state_id(), Some(9));

    assert!(parse(&frame(10, u32::MAX)).is_none());
}

#[test]
fn extended_thread_prefix_checks_standard_owner_references_and_compact_tail() {
    for standard in [false, true] {
        let mut bytes = vec![0; 46];
        bytes[21..29].copy_from_slice(&60.0f64.to_le_bytes());
        bytes[29] = u8::from(standard);
        bytes[34..38].copy_from_slice(&2u32.to_le_bytes());
        bytes[38..42].copy_from_slice(if standard {
            &[0x36, 0, 0x67, 0]
        } else {
            &[0x36, 0, 0x48, 0]
        });
        lp_utf16(&mut bytes, "M8x1");
        lp_utf16(&mut bytes, "8.0");
        lp_utf16(&mut bytes, "ISO Metric profile");
        bytes.extend_from_slice(if standard {
            &[1, 1, 0, 0, 0]
        } else {
            &[1, 2, 0, 0, 0]
        });
        bytes.extend_from_slice(&0.8f64.to_le_bytes());
        bytes.extend_from_slice(&0.6f64.to_le_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&0.1f64.to_le_bytes());
        bytes.extend_from_slice(&0.7f64.to_le_bytes());
        let trailer = bytes.len();
        if standard {
            bytes.push(0);
            for reference in [112u32, 109] {
                bytes.push(1);
                bytes.extend_from_slice(&reference.to_le_bytes());
                bytes.extend_from_slice(&[0; 6]);
            }
            bytes.push(1);
        } else {
            for reference in [90u32, 104, 101] {
                bytes.push(1);
                bytes.extend_from_slice(&reference.to_le_bytes());
                bytes.extend_from_slice(&[0; 6]);
            }
            bytes.push(1);
        }
        let mut scope = DesignParameterScope::empty(
            "f3d:test:extended-thread#100",
            crate::records::feature::scope::DesignFeatureKind::Thread,
            100,
        );
        scope
            .try_edit(|draft| {
                draft.frame_length = 500;
                draft.paired_byte_offset = 500;
                draft.reference_members =
                    crate::records::identity::ReferenceRun::unlocated(vec![101, 104, 109, 112]);
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        let owner_indices = if standard { [109u32, 112] } else { [101, 104] };
        let owners = owner_indices
            .into_iter()
            .enumerate()
            .map(|(ordinal, index)| {
                let owner = crate::design::decode::parameters::parse_parameter_owner(
                    &crate::design::test_support::parameter_owner_frame(),
                )
                .unwrap()
                .into_record("test", 1000 + u64_from_index(ordinal) * 200)
                .unwrap();
                let mut wire = crate::records::parameters::DesignParameterOwnerWire::from(owner);
                wire.record_index = index;
                wire.scope_record_index = 100;
                wire.local_ordinal = u32::try_from(ordinal).unwrap();
                wire.evaluated_value = f64::from(u32::try_from(ordinal).unwrap());
                wire.parameter_record_index = index + 1;
                wire.companion_record_index = index + 2;
                crate::records::parameters::DesignParameterOwner::try_from(wire).unwrap()
            })
            .collect::<Vec<_>>();
        let construction =
            exact_thread_construction(&bytes, &scope, &owners).expect("extended thread prefix");
        assert!(exact_thread_construction(&bytes, &scope, &[]).is_none());
        let mut shifted = owners.clone();
        let mut shifted_wire =
            crate::records::parameters::DesignParameterOwnerWire::from(shifted[0].clone());
        shifted_wire.evaluated_value = 0.25;
        shifted[0] =
            crate::records::parameters::DesignParameterOwner::try_from(shifted_wire).unwrap();
        assert!(exact_thread_construction(&bytes, &scope, &shifted).is_none());
        assert_eq!(construction.designation_offset, 46);
        assert_eq!(
            construction.form,
            if standard {
                DesignThreadForm::StandardExtended
            } else {
                DesignThreadForm::CompactExtended(crate::records::identity::Located {
                    value: std::num::NonZeroU32::new(90).unwrap(),
                    offset: u64_from_index(trailer + 1),
                })
            }
        );
        assert_eq!(
            construction.face_group_record_indices,
            if standard { vec![101] } else { vec![109] }
        );
        for offset in [30, 34, 42] {
            let mut mutated = bytes.clone();
            mutated[offset] = 9;
            assert!(exact_thread_construction(&mutated, &scope, &owners).is_none());
        }
        if !standard {
            for offset in [
                trailer,
                trailer + 5,
                trailer + 11,
                trailer + 12,
                trailer + 16,
                trailer + 22,
                trailer + 23,
                trailer + 27,
                trailer + 33,
            ] {
                let mut mutated = bytes.clone();
                mutated[offset] = 9;
                assert!(exact_thread_construction(&mutated, &scope, &owners).is_none());
            }
            let mut modeled = bytes.clone();
            modeled[30] = 1;
            modeled.splice(34..34, [1u64, 0x130].into_iter().flat_map(u64::to_le_bytes));
            assert!(exact_thread_construction(&modeled, &scope, &owners).is_none());
        }
        if standard {
            for offset in [
                trailer,
                trailer + 1,
                trailer + 2,
                trailer + 6,
                trailer + 12,
                trailer + 13,
                trailer + 17,
                trailer + 23,
            ] {
                let mut mutated = bytes.clone();
                mutated[offset] = 9;
                assert!(exact_thread_construction(&mutated, &scope, &owners).is_none());
            }
        }
    }
}
