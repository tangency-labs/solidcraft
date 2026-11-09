// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::sketch::{
    decode_derived_sketch_point_record, decode_sketch_point_companion, decode_sketch_point_record,
    is_derived_sketch_point_type, SKETCH_CONTAINER_TYPE_GUID, SKETCH_POINT_COMPANION_TYPE,
    SKETCH_POINT_TYPE_GUID,
};
use crate::records::sketch_geometry::{
    SketchPointClosure, SketchPointCompanion, SketchPointCompanionReferenceEncoding,
    SketchPointRecordForm,
};
use crate::test_support::lp_ascii;
use std::collections::HashMap;

const POINT: u32 = 41;
const COMPANION: u32 = 43;
const OWNER: u32 = 17;
const LINE: &str = "DCA267ED-D615-4934-B64F-AD805E8003E2";
const ARC: &str = "F0130424-8B7E-4092-93C9-1CA807482534";

fn push_header(out: &mut Vec<u8>, class_tag: &str, record_index: u32) {
    out.extend_from_slice(&u32::try_from(class_tag.len()).unwrap().to_le_bytes());
    out.extend_from_slice(class_tag.as_bytes());
    out.extend_from_slice(&record_index.to_le_bytes());
}

fn push_reference(out: &mut Vec<u8>, target: u32, type_guid: Option<&str>) {
    out.push(1);
    out.extend_from_slice(&u64::from(target).to_le_bytes());
    if let Some(type_guid) = type_guid {
        lp_ascii(out, type_guid);
    }
    out.extend_from_slice(&[0; 2]);
}

fn tagged_point_payload(
    version: u32,
    inline_typed: bool,
    selector: u64,
    state: u8,
    padded_paired_reference: bool,
) -> Vec<u8> {
    let mut payload = Vec::new();
    push_header(&mut payload, "257", POINT);
    payload.extend_from_slice(&[0; 9]);
    payload.push(1);
    payload.extend_from_slice(&1u32.to_le_bytes());
    lp_ascii(&mut payload, "pt_tag");
    lp_ascii(&mut payload, "IntrinsicMetaTypeuint64");
    payload.extend_from_slice(&500u64.to_le_bytes());
    let paired_type = inline_typed.then_some(SKETCH_POINT_COMPANION_TYPE.0);
    push_reference(&mut payload, COMPANION, paired_type);
    payload.extend(std::iter::repeat_n(0, if version == 11 { 8 } else { 7 }));
    for value in [1.25f64, -2.5, 0.25] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&selector.to_le_bytes());
    payload.push(state);
    payload.extend(std::iter::repeat_n(0, if version == 8 { 8 } else { 12 }));
    payload.extend_from_slice(&1.0f32.to_le_bytes());
    payload.extend_from_slice(&1.0f32.to_le_bytes());
    payload.extend_from_slice(&[0, 1, 0, 0, 0]);
    push_reference(&mut payload, COMPANION, paired_type);
    if padded_paired_reference {
        payload.extend_from_slice(&[0; 4]);
    }
    push_reference(
        &mut payload,
        OWNER,
        inline_typed.then_some(SKETCH_CONTAINER_TYPE_GUID),
    );
    payload
}

#[test]
fn point_record_parser_closes_every_versioned_three_coordinate_form() {
    let cases = [
        (8, false, 0, 0, false),
        (10, false, 0, 1, false),
        (10, true, 0, 1, false),
        (10, true, 2, 1, false),
        (11, true, 0, 0, false),
    ];
    for (version, inline_typed, selector, state, padded) in cases {
        let decoded = decode_sketch_point_record(
            &tagged_point_payload(version, inline_typed, selector, state, padded),
            version,
        )
        .expect("synthetic point form");
        assert_eq!(decoded.record_form.class_version(), version);
        assert_eq!(
            decoded.record_form.uses_inline_typed_references(),
            inline_typed
        );
        assert_eq!(decoded.record_form.persistent_id(), Some(500));
        assert_eq!(decoded.paired_reference, COMPANION);
        assert_eq!(decoded.coordinates, [1.25, -2.5]);
        assert_eq!(decoded.record_form.depth(), 0.25 * 10.0);
        assert_eq!(
            decoded.record_form.closure(),
            SketchPointClosure::from_pair(selector, state)
        );
    }
    assert!(
        decode_sketch_point_record(&tagged_point_payload(10, false, 2, 1, false), 10,).is_none()
    );
    for (selector, state) in [(0, 0), (0, 1), (1, 0), (2, 1), (4, 0), (4, 1)] {
        for padded_paired_reference in [false, true] {
            let decoded = decode_sketch_point_record(
                &tagged_point_payload(11, false, selector, state, padded_paired_reference),
                11,
            )
            .expect("synthetic version-11 point");
            assert_eq!(
                decoded.record_form,
                SketchPointRecordForm::Version11 {
                    depth: 0.25 * 10.0,
                    entity_genesis: None,
                    padded_paired_reference,
                    derived_type: false,
                    companion_prefix_present_zero: false,
                    persistent_id: std::num::NonZeroU64::new(500).unwrap(),
                    flags: [false; 8],
                    closure: SketchPointClosure::from_pair(selector, state).unwrap(),
                }
            );
            assert_eq!(
                decoded.record_form.closure(),
                SketchPointClosure::from_pair(selector, state)
            );
        }
    }
    for (selector, state) in [(1, 1), (2, 0), (4, 2), (3, 0), (0, 2)] {
        assert!(decode_sketch_point_record(
            &tagged_point_payload(11, false, selector, state, false),
            11,
        )
        .is_none());
    }
}

#[test]
fn version_zero_point_retains_its_one_flag_and_source_local_identity() {
    let mut payload = Vec::new();
    push_header(&mut payload, "257", POINT);
    payload.extend_from_slice(&[0; 10]);
    push_reference(&mut payload, COMPANION, None);
    payload.push(1);
    payload.extend_from_slice(&1.25f64.to_le_bytes());
    payload.extend_from_slice(&(-2.5f64).to_le_bytes());
    payload.extend_from_slice(&[0; 20]);
    payload.extend_from_slice(&1.0f32.to_le_bytes());
    payload.extend_from_slice(&[0; 12]);
    payload.extend_from_slice(&1.0f32.to_le_bytes());
    payload.extend_from_slice(&1.0f32.to_le_bytes());
    payload.extend_from_slice(&[1, 1, 0, 0, 0, 0, 1, 0, 0, 0]);
    push_reference(&mut payload, COMPANION, None);
    push_reference(&mut payload, OWNER, None);
    let decoded = decode_sketch_point_record(&payload, 0).expect("version-0 point");
    assert_eq!(
        decoded.record_form,
        SketchPointRecordForm::Version0 { flag: true }
    );
    assert_eq!(decoded.record_form.persistent_id(), None);
    assert_eq!(decoded.record_form.flags(), [1, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(decoded.coordinates, [1.25, -2.5]);
    assert_eq!(decoded.record_form.depth(), 0.0);
    assert_eq!(decoded.record_form.closure(), None);
}

#[test]
fn point_companion_retains_both_prefixes_and_reference_encodings() {
    let types = HashMap::from([
        (POINT, (SKETCH_POINT_TYPE_GUID, 11, "Geometry")),
        (71, (LINE, 2, "Geometry")),
        (72, (ARC, 0, "Geometry")),
    ]);
    for prefix_present_zero in [false, true] {
        for record_form in [
            SketchPointRecordForm::version11(500, SketchPointClosure::Selector0State0, None, 0.0),
            SketchPointRecordForm::Version11InlineTyped {
                depth: 0.0,
                entity_genesis: None,
                trailing_reference: OWNER,
                companion_prefix_present_zero: false,
                persistent_id: std::num::NonZeroU64::new(500).unwrap(),
                flags: [false; 8],
                closure: SketchPointClosure::Selector0State0,
            },
        ] {
            let reference_encoding = SketchPointCompanionReferenceEncoding::for_form(&record_form);
            if prefix_present_zero
                && reference_encoding == SketchPointCompanionReferenceEncoding::InlineTyped
            {
                continue;
            }
            let mut payload = Vec::new();
            push_header(&mut payload, "258", COMPANION);
            if prefix_present_zero {
                payload.extend_from_slice(&[0; 9]);
                payload.extend_from_slice(&[1, 0, 0, 0, 0]);
            } else {
                payload.extend_from_slice(&[0; 10]);
            }
            payload.extend_from_slice(&2u32.to_le_bytes());
            let inline_typed =
                reference_encoding == SketchPointCompanionReferenceEncoding::InlineTyped;
            push_reference(&mut payload, 71, inline_typed.then_some(LINE));
            push_reference(&mut payload, 72, inline_typed.then_some(ARC));
            payload.push(0);
            push_reference(
                &mut payload,
                POINT,
                inline_typed.then_some(SKETCH_POINT_TYPE_GUID),
            );
            assert_eq!(
                decode_sketch_point_companion(&payload, POINT, record_form.clone(), &types),
                Some((
                    record_form
                        .clone()
                        .with_companion_prefix_present_zero(prefix_present_zero)
                        .expect("version-11 form carries the companion prefix"),
                    SketchPointCompanion {
                        incident_curves: vec![71, 72],
                    }
                ))
            );
        }
    }
}

/// A derived sketch-point record: the primary frame stores the property block,
/// and the nested base-type frame stores eight zero bytes and the version-11
/// members without a property block.
fn derived_point_stream(nested_property_block: bool) -> (Vec<u8>, usize) {
    let base = tagged_point_payload(11, false, 4, 0, false);
    let property_block = 20..70;
    let mut bytes = Vec::new();
    push_header(&mut bytes, "256", POINT);
    bytes.extend_from_slice(&[0; 9]);
    bytes.extend_from_slice(&base[property_block.clone()]);
    let nested_at = bytes.len();
    if nested_property_block {
        bytes.extend_from_slice(&base);
    } else {
        bytes.extend_from_slice(&base[..19]);
        bytes.extend_from_slice(&base[property_block.end..]);
    }
    (bytes, nested_at)
}

fn derived_point_meta(secondary_at: Option<usize>) -> crate::metastream::MetaStream {
    crate::metastream::MetaStream {
        types: vec![
            crate::design::test_support::design_type(
                "00D157C2-2332-4BD5-BB53-048C4081D1C3",
                Some(SKETCH_POINT_TYPE_GUID),
                0,
                "MSketch",
                vec![u64::from(POINT)],
            ),
            crate::design::test_support::design_type(
                SKETCH_POINT_TYPE_GUID,
                None,
                11,
                "Geometry",
                Vec::new(),
            ),
        ],
        records: vec![crate::design::test_support::primary_record(
            u64::from(POINT),
            0,
        )],
        secondary_records: secondary_at
            .map(|at| crate::design::test_support::primary_record(u64::from(POINT), at))
            .into_iter()
            .collect(),
    }
}

#[test]
fn derived_point_reads_the_nested_base_record_with_the_primary_properties() {
    let (bytes, nested_at) = derived_point_stream(false);
    for secondary_at in [None, Some(nested_at)] {
        let meta = derived_point_meta(secondary_at);
        let frames = crate::design::decode::meta::design_primary_frames(&bytes, &meta).unwrap();
        assert!(is_derived_sketch_point_type(frames[0].design_type));
        let decoded = decode_derived_sketch_point_record(&bytes, &meta, &frames[0])
            .expect("derived sketch point");
        assert_eq!(decoded.coordinates, [1.25, -2.5]);
        assert_eq!(decoded.paired_reference, COMPANION);
        assert_eq!(decoded.owner_reference, Some(OWNER));
        assert_eq!(decoded.coordinate_offset, 108);
        assert_eq!(decoded.record_form.persistent_id(), Some(500));
        assert!(matches!(
            decoded.record_form,
            SketchPointRecordForm::Version11 {
                derived_type: true,
                closure: SketchPointClosure::Selector4State0,
                ..
            }
        ));
    }
}

#[test]
fn derived_point_nested_record_stores_no_property_block() {
    let (bytes, nested_at) = derived_point_stream(true);
    let meta = derived_point_meta(Some(nested_at));
    let frames = crate::design::decode::meta::design_primary_frames(&bytes, &meta).unwrap();
    assert!(decode_derived_sketch_point_record(&bytes, &meta, &frames[0]).is_none());
}

fn geometry_group(members: &[u32], sketch: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_header(&mut bytes, "258", 50);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(0);
    push_header(&mut bytes, "259", 50);
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&[0; 12]);
    bytes.extend_from_slice(&1.0f32.to_le_bytes());
    bytes.extend_from_slice(&1.0f32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&u32::try_from(members.len()).unwrap().to_le_bytes());
    for &member in members {
        push_reference(&mut bytes, member, None);
    }
    bytes.extend_from_slice(&[0; 4]);
    push_reference(&mut bytes, sketch, None);
    bytes
}

#[test]
fn sketch_geometry_group_validates_the_complete_member_sequence() {
    use crate::design::decode::sketch::parse_sketch_geometry_group;
    let bytes = geometry_group(&[41, 60], OWNER);
    assert_eq!(bytes.len(), 90 + 11 * 2);
    let group = parse_sketch_geometry_group(&bytes).unwrap();
    assert_eq!(group.sketch, OWNER);
    assert_eq!(group.ordinal, 3);
    assert_eq!(group.members, [41, 60]);
    for offset in [11, 20, 29, 41, 48, 49, 50, 61, 70, 97] {
        let mut invalid = bytes.clone();
        invalid[offset] ^= 1;
        assert!(
            parse_sketch_geometry_group(&invalid).is_none(),
            "offset {offset}"
        );
    }
    for offset in [62, 66] {
        let mut invalid = bytes.clone();
        invalid[offset..offset + 4].copy_from_slice(&2.0f32.to_le_bytes());
        assert!(parse_sketch_geometry_group(&invalid).is_none());
    }
    assert!(parse_sketch_geometry_group(&bytes[..bytes.len() - 1]).is_none());
    let mut extended = bytes.clone();
    extended.push(0);
    assert!(parse_sketch_geometry_group(&extended).is_none());
    assert!(parse_sketch_geometry_group(&geometry_group(&[60, 41], OWNER)).is_none());
    assert!(parse_sketch_geometry_group(&geometry_group(&[41, 41], OWNER)).is_none());
    let mut missing_zeros = bytes.clone();
    missing_zeros.drain(97..101);
    assert!(parse_sketch_geometry_group(&missing_zeros).is_none());
}

fn group_stream(members: &[u32], group_owner: u32) -> (Vec<u8>, crate::metastream::MetaStream) {
    use crate::design::test_support::{design_type, primary_record};
    let mut point = tagged_point_payload(11, false, 0, 0, false);
    point[4..7].copy_from_slice(b"256");
    let tail = point.len() - 11;
    point[tail + 1..tail + 9].copy_from_slice(&50u64.to_le_bytes());
    let mut companion = Vec::new();
    push_header(&mut companion, "257", COMPANION);
    companion.extend_from_slice(&[0; 10]);
    companion.extend_from_slice(&0u32.to_le_bytes());
    companion.push(0);
    push_reference(&mut companion, POINT, None);
    let group = geometry_group(members, group_owner);
    let mut owner = Vec::new();
    push_header(&mut owner, "260", OWNER);
    let mut curve = Vec::new();
    push_header(&mut curve, "261", 60);
    curve.extend_from_slice(&[0; 9]);
    curve.push(1);
    curve.extend_from_slice(&2u32.to_le_bytes());
    for (name, value) in [("crv_primary_id", 700u64), ("crv_secondary_id", 0)] {
        lp_ascii(&mut curve, name);
        lp_ascii(&mut curve, "IntrinsicMetaTypeuint64");
        curve.extend_from_slice(&value.to_le_bytes());
    }
    push_reference(&mut curve, 50, None);
    let mut bytes = Vec::new();
    let mut records = Vec::new();
    for (id, frame) in [
        (41, point),
        (43, companion),
        (50, group),
        (17, owner),
        (60, curve),
    ] {
        records.push(primary_record(id, bytes.len()));
        bytes.extend_from_slice(&frame);
    }
    let meta = crate::metastream::MetaStream {
        types: vec![
            design_type(SKETCH_POINT_TYPE_GUID, None, 11, "Geometry", vec![41]),
            design_type(SKETCH_POINT_COMPANION_TYPE.0, None, 0, "Geometry", vec![43]),
            design_type(
                "67F4EECB-EB2C-48E6-8A25-1751E9924EAE",
                None,
                0,
                "MSketch",
                vec![50],
            ),
            design_type(
                "0F551F3B-B195-4486-93D0-711CB83482CE",
                None,
                0,
                "Geometry",
                vec![],
            ),
            design_type(SKETCH_CONTAINER_TYPE_GUID, None, 0, "MSketch", vec![17]),
            design_type(
                "00000000-0000-0000-0000-000000000001",
                None,
                0,
                "Geometry",
                vec![60],
            ),
        ],
        records,
        secondary_records: vec![],
    };
    (bytes, meta)
}

type GroupDecode = (
    Vec<crate::records::sketch_geometry::SketchPoint>,
    Vec<crate::records::sketch_geometry::SketchCurveIdentity>,
    Vec<cadmpeg_ir::report::loss::LossNote>,
);

fn decode_group_stream(bytes: &[u8], meta: &crate::metastream::MetaStream) -> GroupDecode {
    use crate::design::decode::sketch::{
        decode_sketch_curve_identities_from_stream, decode_sketch_points_from_stream,
    };
    let mut losses = Vec::new();
    let points =
        decode_sketch_points_from_stream(bytes, meta, "Design/BulkStream.dat", &mut losses)
            .unwrap();
    let curves = decode_sketch_curve_identities_from_stream(
        bytes,
        meta,
        "Design/BulkStream.dat",
        &mut losses,
    )
    .unwrap();
    (points, curves, losses)
}

fn group_stream_error(bytes: &[u8], meta: &crate::metastream::MetaStream) -> String {
    crate::design::decode::sketch::decode_sketch_points_from_stream(
        bytes,
        meta,
        "Design/BulkStream.dat",
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string()
}

#[test]
fn sketch_group_resolves_point_and_curve_owners() {
    let (bytes, meta) = group_stream(&[41, 60], OWNER);
    let (points, curves, losses) = decode_group_stream(&bytes, &meta);
    assert_eq!(points[0].owner_reference, Some(OWNER));
    assert_eq!(points[0].geometry_group, Some(50));
    assert_eq!(curves[0].owner_reference, Some(OWNER));
    assert_eq!(curves[0].geometry_group, Some(50));
    assert!(losses.is_empty());
    let (bytes, meta) = group_stream(&[41, 60], 43);
    assert!(group_stream_error(&bytes, &meta).contains("invalid member sequence"));
    for nested in [b"256".as_slice(), b"260".as_slice()] {
        let (mut bytes, meta) = group_stream(&[41, 60], OWNER);
        let group_start = usize::try_from(meta.records[2].bulk_offset).unwrap();
        bytes[group_start + 34..group_start + 37].copy_from_slice(nested);
        assert!(group_stream_error(&bytes, &meta).contains("invalid member sequence"));
    }
    let (mut bytes, meta) = group_stream(&[41, 60], OWNER);
    let group_start = usize::try_from(meta.records[2].bulk_offset).unwrap();
    bytes[group_start + 37..group_start + 41].copy_from_slice(&51u32.to_le_bytes());
    assert!(group_stream_error(&bytes, &meta).contains("invalid member sequence"));
}

#[test]
fn sketch_group_assigns_an_unlisted_backlink_to_the_group_sketch() {
    for (members, kind, record) in [([60], "point", 41), ([41], "curve", 60)] {
        let (bytes, meta) = group_stream(&members, OWNER);
        let (points, curves, losses) = decode_group_stream(&bytes, &meta);
        assert_eq!(points[0].owner_reference, Some(OWNER));
        assert_eq!(points[0].geometry_group, Some(50));
        assert_eq!(curves[0].owner_reference, Some(OWNER));
        assert_eq!(curves[0].geometry_group, Some(50));
        let [loss] = losses.as_slice() else {
            panic!("one membership loss for the {kind}");
        };
        assert_eq!(
            loss.code,
            crate::loss::F3dLossCode::SketchGeometryGroupMemberUnlisted.kind()
        );
        assert!(loss.message.starts_with(&format!(
            "1 sketch {kind} record(s) in Design/BulkStream.dat, first record {record},"
        )));
    }
}

fn decode_group_records(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> (
    Vec<crate::records::sketch_geometry::SketchGeometryGroup>,
    Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    let (points, curves, mut losses) = decode_group_stream(bytes, meta);
    let groups = crate::design::decode::sketch::decode_sketch_geometry_groups_from_stream(
        bytes,
        meta,
        "Design/BulkStream.dat",
        &points,
        &curves,
        &mut losses,
    )
    .unwrap();
    (groups, losses)
}

#[test]
fn sketch_geometry_group_keeps_ordinal_and_member_run() {
    let (mut bytes, meta) = group_stream(&[41, 60], OWNER);
    let group_start = usize::try_from(meta.records[2].bulk_offset).unwrap();
    let (groups, losses) = decode_group_records(&bytes, &meta);
    let [group] = groups.as_slice() else {
        panic!("one sketch geometry group");
    };
    assert_eq!(group.record_index, 50);
    assert_eq!(group.byte_offset, u64::try_from(group_start).unwrap());
    assert!(group
        .id
        .ends_with(&format!(":sketch-geometry-group#{group_start}")));
    assert_eq!(group.ordinal, 3);
    assert_eq!(group.members, [41, 60]);
    assert_eq!(group.sketch_reference, OWNER);
    assert!(losses.is_empty());
    bytes[group_start + 21..group_start + 25].copy_from_slice(&9u32.to_le_bytes());
    let (groups, losses) = decode_group_records(&bytes, &meta);
    assert_eq!(groups[0].ordinal, 9);
    assert!(losses.is_empty());
}

#[test]
fn sketch_geometry_group_reports_a_member_without_a_backlink() {
    let (bytes, meta) = group_stream(&[41, 55, 60], OWNER);
    let (groups, losses) = decode_group_records(&bytes, &meta);
    assert_eq!(groups[0].members, [41, 55, 60]);
    let [loss] = losses.as_slice() else {
        panic!("one unbacklinked-member loss");
    };
    assert_eq!(
        loss.code,
        crate::loss::F3dLossCode::SketchGeometryGroupMemberUnbacklinked.kind()
    );
    assert!(loss.message.starts_with(
        "1 sketch geometry group member reference(s) in Design/BulkStream.dat, first group 50 member 55,"
    ));
}

#[test]
fn sketch_geometry_group_does_not_create_legacy_sketch_header() {
    use std::io::{Cursor, Write};
    let bulk = geometry_group(&[41, 60], OWNER);
    assert!(crate::design::decode::sketch::parse_legacy_sketch_member_run(&bulk, 0, 50).is_some());
    let meta = crate::test_support::streams_test::design_metastream_with_records(
        &[(
            "67F4EECB-EB2C-48E6-8A25-1751E9924EAE",
            "",
            0,
            "MSketch",
            &[50],
        )],
        &[(50, 0)],
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&bulk).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&meta).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        assert!(crate::design::decode::sketch::decode_entity_headers(scan)
            .unwrap()
            .is_empty());
    });
}
