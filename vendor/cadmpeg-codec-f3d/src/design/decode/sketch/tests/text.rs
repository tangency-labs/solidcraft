// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use crate::design::test_support::push_reference;
use crate::test_support::lp_utf16;
use cadmpeg_ir::math::Point2;

const EPS_TEXT_ROTATION: f64 = 1.0e-12;

use crate::design::decode::sketch::decode_sketch_points_from_stream;

use crate::design::decode::sketch::decode_sketch_texts_from_stream;

use crate::design::decode::sketch::decode_sketch_curve_identities_from_stream;

#[test]
fn text_fixture_records_preserve_styles_and_refuse_unrepresented_orientation() {
    let cases = crate::test_support::sketch_text::text_fixture_records();
    assert_eq!(
        cases.len(),
        crate::test_support::verification::case_count(6, 65)
    );
    for case in cases {
        let text = case.decode();
        assert_eq!(text.text, case.text, "{}", case.label);
        assert_eq!(text.font_family, case.font_family, "{}", case.label);
        assert_eq!(text.font_weight, case.font_weight, "{}", case.label);
        assert_eq!(
            text.height.get().to_bits(),
            (case.height_cm * 10.0).to_bits(),
            "{}",
            case.label
        );
        assert_eq!(text.raw_bytes, case.source_hex, "{}", case.label);
        assert_eq!(
            super::super::sketch_text_neutral_state(&text),
            Some(case.neutral_state),
            "{}",
            case.label
        );
        let replay: crate::records::sketch_geometry::SketchText =
            serde_json::from_value(serde_json::to_value(&text).unwrap()).unwrap();
        assert_eq!(replay, text, "{}", case.label);
    }
}

#[test]
fn txt_tag_font_weight_is_retained_without_a_neutral_weight_assignment() {
    let mut bytes = txt_tag_sketch_text_record(&[("txt_tag", 115)], &[261], &[261], (0.0, 0.0));
    let weight_offsets: Vec<_> = bytes
        .windows(4)
        .enumerate()
        .filter_map(|(offset, value)| (value == 400i32.to_le_bytes()).then_some(offset))
        .collect();
    assert_eq!(weight_offsets.len(), 1);
    let weight_offset = weight_offsets[0];
    for weight in [i32::MIN, -1, 0, 700, 701, 900, i32::MAX] {
        bytes[weight_offset..weight_offset + 4].copy_from_slice(&weight.to_le_bytes());
        let text = super::super::decode_sketch_text_record(
            &bytes,
            "Design/BulkStream.dat",
            "329".to_owned().try_into().unwrap(),
            4,
            304,
            0,
        )
        .unwrap()
        .unwrap();
        assert_eq!(text.font_weight, weight);
        assert_eq!(text.raw_bytes.as_ref(), bytes);
        assert!(cadmpeg_ir::sketches::SketchFontWeight::try_from(text.font_weight).is_err());
    }
}

#[test]
fn native_sketch_text_refuses_zero_height() {
    let text = decode_sketch_text(&sketch_text_record(
        &[("textex_tag", 109)],
        [None, None],
        None,
    ))
    .expect("sketch text record");
    let mut zero_height = serde_json::to_value(&text).expect("serialize sketch text");
    zero_height["height"] = serde_json::json!(0.0);
    assert!(
        serde_json::from_value::<crate::records::sketch_geometry::SketchText>(zero_height).is_err()
    );
}

#[test]
fn native_sketch_text_refuses_negative_width_factor() {
    let text = decode_sketch_text(&sketch_text_record(
        &[("textex_tag", 109)],
        [None, None],
        None,
    ))
    .expect("sketch text record");
    let mut negative_width = serde_json::to_value(&text).expect("serialize sketch text");
    negative_width["width_factor"] = serde_json::json!(-1.0);
    assert!(
        serde_json::from_value::<crate::records::sketch_geometry::SketchText>(negative_width)
            .is_err()
    );
}

#[test]
fn indexed_textex_tag_sketch_text_record_decodes_frame_and_path_types() {
    for text_type in [0, 1] {
        let bytes = indexed_sketch_text_record(text_type);
        let text = decode_sketch_text_at(&bytes, 3).expect("indexed sketch text record");
        assert_eq!(text.record_index, 304);
        assert_eq!(text.owner_reference, 227);
        assert_eq!(text.entity_genesis, Some(0));
        assert_eq!(text.persistent_id, Some(117));
        assert_eq!(text.text, "B6 Probe 47");
        assert_eq!(text.font_family, "Arial");
        assert_eq!(text.font_weight, 400);
        assert_eq!(text.height.get(), 6.0);
        assert_eq!(
            text.width_factor()
                .map(cadmpeg_ir::scalar::NonNegativeReal::get),
            Some(1.0)
        );
        assert_eq!(
            text.alignment().map(|alignment| alignment.horizontal),
            Some(3)
        );
        assert_eq!(
            text.alignment().map(|alignment| alignment.vertical),
            Some(3)
        );
        assert_eq!(
            text.color,
            cadmpeg_ir::topology::Color::new(0.0, 0.0, 0.0, 1.0).expect("valid color")
        );
        assert_eq!(
            text.placement().map(|placement| placement.anchor.get()),
            None
        );
        assert_eq!(
            text.placement().map(|placement| placement.rotation.get()),
            None
        );
        assert_eq!(
            (match text.layout {
                crate::records::sketch_geometry::SketchTextLayout::TextexTag {
                    first_reference,
                    ..
                } => first_reference,
                crate::records::sketch_geometry::SketchTextLayout::TxtTag { .. } => None,
            }),
            Some(319)
        );
        assert_eq!(
            (match text.layout {
                crate::records::sketch_geometry::SketchTextLayout::TextexTag {
                    second_reference,
                    ..
                } => second_reference,
                crate::records::sketch_geometry::SketchTextLayout::TxtTag { .. } => None,
            }),
            Some(322)
        );
        assert_eq!(text.raw_bytes, bytes);
    }
}

#[test]
fn sketch_text_stream_decodes_its_text_records() {
    use crate::metastream::{MetaStream, RecordIndexEntry};
    use crate::records::entity_header::{BaseTypeGuid, SegmentTypeData};
    use crate::records::identity::{Located, ReferenceRun};

    let bytes = indexed_sketch_text_record(1);
    let types = (0..32)
        .map(|ordinal| SegmentTypeData {
            byte_offset: 0,
            type_guid: (if ordinal == 31 {
                "E0618268-3A06-450E-9E94-7CF4C2E66802"
            } else {
                "00000000-0000-0000-0000-000000000000"
            })
            .to_owned()
            .try_into()
            .unwrap(),
            type_guid_offset: 0,
            base_type_guid: BaseTypeGuid::Absent,
            version: 3,
            version_offset: 0,
            module: "Geometry".into(),
            entities: ReferenceRun::located(if ordinal == 31 {
                vec![Located {
                    value: 304,
                    offset: 0,
                }]
            } else {
                Vec::new()
            }),
        })
        .collect();
    let meta = MetaStream {
        types,
        records: vec![RecordIndexEntry {
            entity_id: 304,
            bulk_offset: 0,
        }],
        secondary_records: Vec::new(),
    };
    assert_eq!(
        crate::design::decode::sketch::decode_sketch_texts_from_stream(
            &bytes,
            &meta,
            "Design/BulkStream.dat"
        )
        .unwrap()
        .len(),
        1
    );
}

fn indexed_sketch_fixture() -> (
    Vec<u8>,
    crate::metastream::MetaStream,
    usize,
    usize,
    usize,
    usize,
) {
    use crate::metastream::{MetaStream, RecordIndexEntry};
    use crate::records::entity_header::SegmentTypeData;

    const PARENT: u64 = 900;
    const POINT: u64 = 50;
    const CURVE: u64 = 51;
    const TEXT: u64 = 52;
    const COMPANION: u64 = 60;

    let push_ascii = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(
            &(u32::try_from(value.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        bytes.extend_from_slice(value.as_bytes());
    };
    let record_prefix = |class_tag: u32, entity_id: u64| {
        let mut bytes = Vec::new();
        push_ascii(&mut bytes, &class_tag.to_string());
        bytes.extend_from_slice(
            &u32::try_from(entity_id)
                .expect("synthetic indexed-record identity")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(&[0; 9]);
        bytes
    };
    let local_reference = |bytes: &mut Vec<u8>, target: u64| {
        bytes.push(1);
        bytes.extend_from_slice(&target.to_le_bytes());
        bytes.extend_from_slice(&[0; 2]);
    };
    let owner_reference = |bytes: &mut Vec<u8>| {
        local_reference(bytes, PARENT);
    };
    let point_record = |x: f64, y: f64| {
        let mut bytes = record_prefix(257, POINT);
        bytes.push(1);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        push_ascii(&mut bytes, "pt_tag");
        push_ascii(&mut bytes, "IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&500u64.to_le_bytes());
        local_reference(&mut bytes, COMPANION);
        bytes.extend_from_slice(&[0; 7]);
        bytes.extend_from_slice(&x.to_le_bytes());
        bytes.extend_from_slice(&y.to_le_bytes());
        bytes.extend_from_slice(&0.0f64.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&[0; 12]);
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[0, 1, 0, 0, 0]);
        local_reference(&mut bytes, COMPANION);
        owner_reference(&mut bytes);
        bytes
    };
    let curve_record = |start_x: f64| {
        let mut bytes = record_prefix(258, CURVE);
        bytes.push(1);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        push_ascii(&mut bytes, "crv_primary_id");
        push_ascii(&mut bytes, "IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&700u64.to_le_bytes());
        push_ascii(&mut bytes, "crv_secondary_id");
        push_ascii(&mut bytes, "IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&0u64.to_le_bytes());
        for value in [
            start_x, 0.0, 0.0, 2.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        owner_reference(&mut bytes);
        bytes
    };
    let text_record = |text: &str| {
        let mut bytes = sketch_text_record(&[("textex_tag", 800)], [None, None], None);
        bytes[4..7].copy_from_slice(b"259");
        bytes[7..11].copy_from_slice(
            &u32::try_from(TEXT)
                .expect("synthetic text record index")
                .to_le_bytes(),
        );
        bytes[11..20].fill(0);
        let old = "path text"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let new = text
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(new.len(), old.len());
        let at = bytes
            .windows(old.len())
            .position(|window| window == old)
            .expect("synthetic text payload");
        bytes[at..at + old.len()].copy_from_slice(&new);
        bytes
    };
    let design_type =
        |type_guid: &str, version: u32, module: &str, entity_ids: Vec<u64>| SegmentTypeData {
            byte_offset: 0,
            type_guid: type_guid.to_owned().try_into().expect("type GUID"),
            type_guid_offset: 0,
            base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
            version,
            version_offset: 0,
            module: module.into(),
            entities: crate::records::identity::ReferenceRun::located(
                entity_ids
                    .into_iter()
                    .map(|value| crate::records::identity::Located { value, offset: 0 })
                    .collect(),
            ),
        };

    let mut bytes = record_prefix(256, PARENT);
    bytes.extend_from_slice(&point_record(1.0, 2.0));
    bytes.extend_from_slice(&curve_record(1.0));
    bytes.extend_from_slice(&text_record("stale txt"));
    let nested_at = bytes.len();
    bytes.extend_from_slice(&record_prefix(260, PARENT));
    let live_point_at = bytes.len();
    bytes.extend_from_slice(&point_record(7.0, -3.0));
    let live_curve_at = bytes.len();
    bytes.extend_from_slice(&curve_record(5.0));
    let live_text_at = bytes.len();
    bytes.extend_from_slice(&text_record("live text"));
    let companion_at = bytes.len();
    let mut companion = record_prefix(261, COMPANION);
    companion.push(0);
    companion.extend_from_slice(&0u32.to_le_bytes());
    companion.push(0);
    local_reference(&mut companion, POINT);
    bytes.extend_from_slice(&companion);

    let meta = MetaStream {
        types: vec![
            design_type(
                crate::design::decode::sketch::SKETCH_CONTAINER_TYPE_GUID,
                0,
                "Fusion",
                vec![PARENT],
            ),
            design_type(
                "C2CEDAE7-1716-47C1-B7B1-07B70081D0FB",
                10,
                "Geometry",
                vec![POINT],
            ),
            design_type(
                "DCA267ED-D615-4934-B64F-AD805E8003E2",
                2,
                "Geometry",
                vec![CURVE],
            ),
            design_type(
                "E0618268-3A06-450E-9E94-7CF4C2E66802",
                4,
                "Geometry",
                vec![TEXT],
            ),
            design_type(
                "00000000-0000-0000-0000-000000000004",
                0,
                "Fusion",
                Vec::new(),
            ),
            design_type(
                crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE.0,
                crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE.1,
                crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE.2,
                vec![COMPANION],
            ),
        ],
        records: vec![
            RecordIndexEntry {
                entity_id: PARENT,
                bulk_offset: 0,
            },
            RecordIndexEntry {
                entity_id: POINT,
                bulk_offset: u64_from_index(live_point_at),
            },
            RecordIndexEntry {
                entity_id: CURVE,
                bulk_offset: u64_from_index(live_curve_at),
            },
            RecordIndexEntry {
                entity_id: TEXT,
                bulk_offset: u64_from_index(live_text_at),
            },
            RecordIndexEntry {
                entity_id: COMPANION,
                bulk_offset: u64_from_index(companion_at),
            },
        ],
        secondary_records: vec![RecordIndexEntry {
            entity_id: PARENT,
            bulk_offset: u64_from_index(nested_at),
        }],
    };

    (
        bytes,
        meta,
        live_point_at,
        live_curve_at,
        live_text_at,
        nested_at,
    )
}

#[test]
fn sketch_records_use_the_primary_index_live_copy() {
    use crate::records::sketch_geometry::SketchCurveGeometry;
    use cadmpeg_ir::math::{Point2, Point3};

    let (bytes, mut meta, live_point_at, live_curve_at, live_text_at, nested_at) =
        indexed_sketch_fixture();
    let points = decode_sketch_points_from_stream(&bytes, &meta, "Design/BulkStream.dat")
        .expect("indexed sketch points");
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].byte_offset, u64_from_index(live_point_at));
    assert_eq!(points[0].coordinates(), Point2::new(70.0, -30.0));
    meta.types[1].type_guid = "00000000-0000-0000-0000-000000000002"
        .to_owned()
        .try_into()
        .expect("type GUID");
    assert!(
        decode_sketch_points_from_stream(&bytes, &meta, "Design/BulkStream.dat",)
            .expect("structurally point-shaped foreign type")
            .is_empty()
    );
    meta.types[1].type_guid = crate::design::decode::sketch::CURRENT_SKETCH_POINT_TYPE
        .0
        .to_owned()
        .try_into()
        .expect("type GUID");
    let mut malformed_point = bytes.clone();
    malformed_point[live_point_at + 70] = 0;
    assert!(matches!(
        decode_sketch_points_from_stream(&malformed_point, &meta, "Design/BulkStream.dat",),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));

    let curves = decode_sketch_curve_identities_from_stream(&bytes, &meta, "Design/BulkStream.dat")
        .expect("indexed sketch curves");
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].byte_offset, u64_from_index(live_curve_at));
    assert!(matches!(
        curves[0].geometry,
        Some(SketchCurveGeometry::Line { start, .. }) if start.get() == Point3::new(50.0, 0.0, 0.0)
    ));

    let texts = decode_sketch_texts_from_stream(&bytes, &meta, "Design/BulkStream.dat")
        .expect("indexed sketch texts");
    assert_eq!(texts.len(), 1);
    assert_eq!(texts[0].byte_offset, u64_from_index(live_text_at));
    assert_eq!(texts[0].text, "live text");

    let mut mismatched_primary = bytes.clone();
    mismatched_primary[live_point_at + 7..live_point_at + 11]
        .copy_from_slice(&999u32.to_le_bytes());
    assert!(matches!(
        decode_sketch_points_from_stream(&mismatched_primary, &meta, "Design/BulkStream.dat",),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));

    let mut mismatched_secondary = bytes;
    mismatched_secondary[nested_at + 7..nested_at + 11].copy_from_slice(&999u32.to_le_bytes());
    assert!(matches!(
        decode_sketch_points_from_stream(&mismatched_secondary, &meta, "Design/BulkStream.dat",),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn sketch_text_record_decodes_typed_content_and_metrics() {
    let bytes = sketch_text_record(
        &[
            ("EntityGenesis", 4),
            ("textex_tag", 109),
            ("txt_tag_base", 305),
        ],
        [Some(307), Some(310)],
        None,
    );
    let text = decode_sketch_text(&bytes).expect("sketch text record");
    assert_eq!(text.record_index, 304);
    assert_eq!(text.owner_reference, 201);
    assert_eq!(text.entity_genesis, Some(4));
    assert_eq!(text.persistent_id, Some(109));
    assert_eq!(text.base_id, Some(305));
    assert_eq!(text.text, "path text");
    assert_eq!(text.font_family, "Arial");
    assert_eq!(text.font_weight, 400);
    // The height is the field after the font family, in centimetres; the width
    // factor is the field before it.
    assert_eq!(text.height.get(), 10.0);
    assert_eq!(
        text.width_factor()
            .map(cadmpeg_ir::scalar::NonNegativeReal::get),
        Some(0.8)
    );
    assert_eq!(
        text.alignment().map(|alignment| alignment.horizontal),
        Some(3)
    );
    assert_eq!(
        text.alignment().map(|alignment| alignment.vertical),
        Some(3)
    );
    // The four f32 after the width factor are red, green, blue, and alpha in
    // that order.
    assert_eq!(
        text.color,
        cadmpeg_ir::topology::Color::new(0.25, 0.5, 0.75, 1.0).expect("valid color")
    );
    assert_eq!(
        (match text.layout {
            crate::records::sketch_geometry::SketchTextLayout::TextexTag {
                first_reference,
                ..
            } => first_reference,
            crate::records::sketch_geometry::SketchTextLayout::TxtTag { .. } => None,
        }),
        Some(307)
    );
    assert_eq!(
        (match text.layout {
            crate::records::sketch_geometry::SketchTextLayout::TextexTag {
                second_reference,
                ..
            } => second_reference,
            crate::records::sketch_geometry::SketchTextLayout::TxtTag { .. } => None,
        }),
        Some(310)
    );
}

#[test]
fn sketch_text_record_refuses_a_colour_component_outside_the_unit_range() {
    let bytes = sketch_text_record(&[("textex_tag", 109)], [None, None], None);
    let mut over = bytes.clone();
    let at = bytes
        .windows(4)
        .position(|window| window == 0.5f32.to_le_bytes())
        .expect("green component");
    over[at..at + 4].copy_from_slice(&1.5f32.to_le_bytes());
    assert!(decode_sketch_text(&over).is_none());
    let mut under = bytes;
    under[at..at + 4].copy_from_slice(&(-0.5f32).to_le_bytes());
    assert!(decode_sketch_text(&under).is_none());
}

#[test]
fn sketch_text_record_decodes_without_the_optional_property_keys() {
    let text = decode_sketch_text(&sketch_text_record(
        &[("textex_tag", 109)],
        [None, None],
        None,
    ))
    .expect("sketch text record");
    assert_eq!(text.entity_genesis, None);
    assert_eq!(text.base_id, None);
    assert_eq!(text.persistent_id, Some(109));
    assert_eq!(
        (match text.layout {
            crate::records::sketch_geometry::SketchTextLayout::TextexTag {
                first_reference,
                ..
            } => first_reference,
            crate::records::sketch_geometry::SketchTextLayout::TxtTag { .. } => None,
        }),
        None
    );
    assert_eq!(
        (match text.layout {
            crate::records::sketch_geometry::SketchTextLayout::TextexTag {
                second_reference,
                ..
            } => second_reference,
            crate::records::sketch_geometry::SketchTextLayout::TxtTag { .. } => None,
        }),
        None
    );
    assert_eq!(text.height.get(), 10.0);
    assert_eq!(
        text.width_factor()
            .map(cadmpeg_ir::scalar::NonNegativeReal::get),
        Some(0.8)
    );
}

#[test]
fn frame_sketch_text_record_takes_its_anchor_and_rotation_from_the_transform() {
    let rotation = std::f64::consts::FRAC_PI_2;
    let text = decode_sketch_text(&sketch_text_record(
        &[("textex_tag", 109), ("txt_tag_base", 305)],
        [None, None],
        Some((2.175, -0.5, rotation)),
    ))
    .expect("sketch text record");
    assert_eq!(text.base_id, Some(305));
    assert_eq!(text.owner_reference, 201);
    // The anchor is the transform's last column in centimetres and the
    // rotation is the angle of its first basis column.
    assert_eq!(
        text.placement().map(|placement| placement.anchor.get()),
        Some(cadmpeg_ir::math::Point2::new(21.75, -5.0))
    );
    assert!(
        (text
            .placement()
            .map(|placement| placement.rotation.get())
            .expect("rotation")
            - rotation)
            .abs()
            < EPS_TEXT_ROTATION
    );
    // Frame text stores 128 more bytes than path text.
    assert_eq!(
        text.raw_bytes.len(),
        sketch_text_record(
            &[("textex_tag", 109), ("txt_tag_base", 305)],
            [None, None],
            None
        )
        .len()
            + 128
    );
}

#[test]
fn path_sketch_text_record_stores_neither_anchor_nor_rotation() {
    let text = decode_sketch_text(&sketch_text_record(
        &[("textex_tag", 109)],
        [None, None],
        None,
    ))
    .expect("sketch text record");
    assert_eq!(
        text.placement().map(|placement| placement.anchor.get()),
        None
    );
    assert_eq!(
        text.placement().map(|placement| placement.rotation.get()),
        None
    );
}

#[test]
fn frame_sketch_text_record_refuses_a_transform_that_is_not_a_planar_rotation() {
    let bytes = sketch_text_record(&[("textex_tag", 109)], [None, None], Some((1.0, 2.0, 0.25)));
    let at = bytes.len() - 128 - 30 - 11;
    // A scaled basis, a third row that is not the identity's, and a bottom row
    // that is not `(0, 0, 0, 1)` each leave the placement.
    for (element, value) in [(0usize, 2.0f64), (10, 0.5), (15, 2.0)] {
        let mut broken = bytes.clone();
        broken[at + element * 8..at + element * 8 + 8].copy_from_slice(&value.to_le_bytes());
        assert!(decode_sketch_text(&broken).is_none());
    }
}

#[test]
fn frame_sketch_text_record_with_a_turned_over_plane_is_mirrored() {
    let bytes = sketch_text_record(&[("textex_tag", 109)], [None, None], Some((1.0, 2.0, 0.0)));
    let at = bytes.len() - 128 - 30 - 11;
    let mut mirrored = bytes.clone();
    for element in [5usize, 10] {
        mirrored[at + element * 8..at + element * 8 + 8].copy_from_slice(&(-1.0f64).to_le_bytes());
    }
    let text = decode_sketch_text(&mirrored).expect("mirrored sketch text record");
    assert_eq!(
        text.placement().map(|placement| placement.anchor.get()),
        None
    );
    assert_eq!(super::super::sketch_text_neutral_state(&text), Some(false));

    // A turned-over plane with a rotation basis is not a rigid placement.
    let mut turned = bytes;
    turned[at + 10 * 8..at + 10 * 8 + 8].copy_from_slice(&(-1.0f64).to_le_bytes());
    assert!(decode_sketch_text(&turned).is_none());
}

#[test]
fn sketch_text_record_refuses_a_flag_byte_that_does_not_repeat_the_text_type() {
    let bytes = sketch_text_record(&[("textex_tag", 109)], [None, None], None);
    // The flag byte sits between the text-type enum and the transform slot,
    // ahead of the trailing run and the owning-sketch reference.
    let at = bytes.len() - 30 - 11 - 1;
    assert_eq!(bytes[at], 1);
    let mut broken = bytes;
    broken[at] = 0;
    assert!(decode_sketch_text(&broken).is_none());
}

#[test]
fn sketch_text_record_refuses_a_payload_that_does_not_end_on_its_owner() {
    let mut bytes = sketch_text_record(&[("textex_tag", 109)], [None, None], None);
    bytes.push(0);
    assert!(decode_sketch_text(&bytes).is_none());
}

#[test]
fn txt_tag_sketch_text_record_decodes_its_anchor_and_metrics() {
    let text = decode_sketch_text(&txt_tag_sketch_text_record(
        &[("EntityGenesis", 4), ("txt_tag", 115)],
        &[261, 262, 263, 264],
        &[261, 262, 263, 264, 261, 262, 263, 264],
        (0.25, -1.5),
    ))
    .expect("sketch text record");
    assert_eq!(text.record_index, 304);
    assert_eq!(text.owner_reference, 201);
    assert_eq!(text.entity_genesis, Some(4));
    assert_eq!(text.persistent_id, Some(115));
    assert_eq!(text.base_id, None);
    assert_eq!(text.text, "sketch text");
    assert_eq!(text.font_family, "Arial");
    assert_eq!(text.font_weight, 400);
    assert_eq!(
        text.placement().map(|placement| placement.rotation.get()),
        Some(0.0)
    );
    assert_eq!(text.height.get(), 5.0);
    // The form stores no width factor, and the anchor is the field pair the
    // other form omits.
    assert_eq!(text.width_factor(), None);
    assert_eq!(text.alignment().map(|alignment| alignment.horizontal), None);
    assert_eq!(text.alignment().map(|alignment| alignment.vertical), None);
    assert_eq!(
        text.placement().map(|placement| placement.anchor.get()),
        Some(cadmpeg_ir::math::Point2::new(2.5, -15.0))
    );
    // The colour closes the twenty-nine-byte run in the same component order
    // as the other form.
    assert_eq!(
        text.color,
        cadmpeg_ir::topology::Color::new(0.0, 0.3, 1.0, 1.0).expect("valid color")
    );
    assert_eq!(
        (match text.layout {
            crate::records::sketch_geometry::SketchTextLayout::TextexTag {
                first_reference,
                ..
            } => first_reference,
            crate::records::sketch_geometry::SketchTextLayout::TxtTag { .. } => None,
        }),
        None
    );
    assert_eq!(
        (match text.layout {
            crate::records::sketch_geometry::SketchTextLayout::TextexTag {
                second_reference,
                ..
            } => second_reference,
            crate::records::sketch_geometry::SketchTextLayout::TxtTag { .. } => None,
        }),
        None
    );
}

#[test]
fn txt_tag_sketch_text_record_decodes_stored_rotation() {
    let stored_rotation = std::f64::consts::TAU - std::f64::consts::FRAC_PI_6;
    let text = decode_sketch_text(&txt_tag_sketch_text_record_at_with_rotation(
        &[("txt_tag", 115)],
        &[261],
        &[261],
        (0.811_473_722_624_350_2, -1.434_008_059_576_836_5),
        4,
        stored_rotation,
    ))
    .expect("rotated txt_tag");
    assert_eq!(
        text.placement().map(|placement| placement.rotation.get()),
        Some(stored_rotation)
    );
    assert_eq!(
        text.placement().map(|placement| placement.anchor.get()),
        Some(Point2::new(8.114_737_226_243_502, -14.340_080_595_768_365,))
    );
}

#[test]
fn txt_tag_sketch_text_record_decodes_an_empty_reference_run() {
    let text = decode_sketch_text(&txt_tag_sketch_text_record(
        &[("txt_tag", 115), ("txt_tag_base", 305)],
        &[],
        &[],
        (0.0, 0.0),
    ))
    .expect("sketch text record");
    assert_eq!(text.base_id, Some(305));
    assert_eq!(
        text.placement().map(|placement| placement.anchor.get()),
        Some(cadmpeg_ir::math::Point2::new(0.0, 0.0))
    );
}

#[test]
fn txt_tag_sketch_text_record_refuses_a_payload_that_does_not_end_on_its_owner() {
    let mut bytes = txt_tag_sketch_text_record(&[("txt_tag", 115)], &[261], &[261], (0.0, 0.0));
    bytes.push(0);
    assert!(decode_sketch_text(&bytes).is_none());
}

#[test]
fn sketch_text_record_refuses_a_property_block_without_an_identity_key() {
    assert!(decode_sketch_text(&txt_tag_sketch_text_record(
        &[("txt_tag_base", 305)],
        &[261],
        &[261],
        (0.0, 0.0),
    ))
    .is_none());
    assert!(decode_sketch_text(&sketch_text_record(
        &[("txt_tag_base", 305)],
        [None, None],
        None
    ))
    .is_none());
}

#[test]
fn a_txt_tag_sketch_text_record_below_the_identity_key_version_stores_no_identity() {
    let text = decode_sketch_text_at(
        &txt_tag_sketch_text_record_at(&[("txt_tag_base", 300)], &[261], &[261], (0.25, -1.5), 3),
        3,
    )
    .expect("sketch text record");
    assert_eq!(text.class_version, 3);
    assert_eq!(text.persistent_id, None);
    assert_eq!(text.base_id, Some(300));
    assert_eq!(text.text, "sketch text");
    assert_eq!(
        text.placement().map(|placement| placement.anchor.get()),
        Some(cadmpeg_ir::math::Point2::new(2.5, -15.0))
    );
}

#[test]
fn the_txt_tag_anchor_run_widens_with_the_class_version() {
    // The run between the anchor and the text string is ten bytes below class
    // version 4 and eleven from it, so a record read at the other version's
    // width does not end on its owning-sketch reference.
    for (written, read) in [(3u32, 4u32), (4, 3)] {
        assert!(decode_sketch_text_at(
            &txt_tag_sketch_text_record_at(
                &[("txt_tag", 115)],
                &[261],
                &[261],
                (0.0, 0.0),
                written
            ),
            read,
        )
        .is_none());
    }
}

/// Build one sketch-text record: `properties` are the property-block keys in
/// stream order, `slots` says whether each parameter-reference member is
/// written, and `frame` gives the anchor in centimetres and the rotation in
/// radians of a frame text's placement transform, or `None` for path text,
/// which stores no transform.
fn sketch_text_record(
    properties: &[(&str, u64)],
    slots: [Option<u32>; 2],
    frame: Option<(f64, f64, f64)>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    let push_ascii = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(
            &(u32::try_from(value.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        bytes.extend_from_slice(value.as_bytes());
    };
    push_ascii(&mut bytes, "329");
    bytes.extend_from_slice(&304u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 5]);
    bytes.push(1);
    bytes.extend_from_slice(
        &(u32::try_from(properties.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for (key, value) in properties {
        push_ascii(&mut bytes, key);
        push_ascii(&mut bytes, "IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(1);
    bytes.extend_from_slice(&0.8f64.to_le_bytes());
    for component in [0.25f32, 0.5, 0.75, 1.0] {
        bytes.extend_from_slice(&component.to_le_bytes());
    }
    lp_utf16(&mut bytes, "Arial");
    bytes.push(0);
    bytes.extend_from_slice(&1.0f64.to_le_bytes());
    if let Some(reference) = slots[0] {
        push_reference(&mut bytes, reference);
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 3]);
    lp_utf16(&mut bytes, "path text");
    if let Some(reference) = slots[1] {
        push_reference(&mut bytes, reference);
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&400i32.to_le_bytes());
    bytes.extend_from_slice(&u32::from(frame.is_none()).to_le_bytes());
    bytes.push(u8::from(frame.is_none()));
    if let Some((anchor_u, anchor_v, rotation)) = frame {
        // A planar rigid placement: the 2x2 rotation basis, the anchor in the
        // last column, and the identity's third row and column.
        let (sin, cos) = rotation.sin_cos();
        for element in [
            cos, -sin, 0.0, anchor_u, sin, cos, 0.0, anchor_v, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0,
            1.0,
        ] {
            bytes.extend_from_slice(&element.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&[0; 30]);
    push_reference(&mut bytes, 201);
    bytes.extend_from_slice(&[0; 6]);
    bytes
}

/// Build the indexed Design form of a `textex_tag` record. Its header carries
/// a u32 record index and a nine-byte zero entity lane, and its class tail ends
/// after the fixed frame suffix and owning-sketch reference.
fn indexed_sketch_text_record(text_type: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    let push_ascii = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(
            &(u32::try_from(value.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        bytes.extend_from_slice(value.as_bytes());
    };
    let push_padded_reference = |bytes: &mut Vec<u8>, reference: u32| {
        push_reference(bytes, reference);
        bytes.extend_from_slice(&[0; 6]);
    };
    push_ascii(&mut bytes, "287");
    bytes.extend_from_slice(&304u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 9]);
    bytes.push(1);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for (key, value) in [("EntityGenesis", 0u64), ("textex_tag", 117)] {
        push_ascii(&mut bytes, key);
        push_ascii(&mut bytes, "IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0);
    bytes.extend_from_slice(&1.0f64.to_le_bytes());
    for component in [0.0f32, 0.0, 0.0, 1.0] {
        bytes.extend_from_slice(&component.to_le_bytes());
    }
    lp_utf16(&mut bytes, "Arial");
    bytes.push(0);
    bytes.extend_from_slice(&0.6f64.to_le_bytes());
    push_padded_reference(&mut bytes, 319);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 3]);
    lp_utf16(&mut bytes, "B6 Probe 47");
    push_padded_reference(&mut bytes, 322);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&400i32.to_le_bytes());
    bytes.extend_from_slice(&text_type.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&256u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&1.0f32.to_le_bytes());
    bytes.extend_from_slice(&1.0f32.to_le_bytes());
    bytes.extend_from_slice(&[0; 5]);
    push_padded_reference(&mut bytes, 227);
    bytes
}

/// Decode one sketch-text record at `class_version`, the version its Design
/// `MetaStream` type table gives its class.
fn decode_sketch_text_at(
    bytes: &[u8],
    class_version: u32,
) -> Option<crate::records::sketch_geometry::SketchText> {
    crate::design::decode::sketch::decode_sketch_text_record(
        bytes,
        "Design/BulkStream.dat",
        crate::records::references::DesignClassTag::try_from("329".to_owned()).unwrap(),
        class_version,
        304,
        7,
    )
    .unwrap()
}

/// Decode one sketch-text record at the class version that writes an identity
/// key and the wider anchor run.
fn decode_sketch_text(bytes: &[u8]) -> Option<crate::records::sketch_geometry::SketchText> {
    decode_sketch_text_at(bytes, 4)
}

/// Build one sketch-text record in the `txt_tag` identity form: `properties`
/// are the property-block keys in stream order, `frame` is the leading block's
/// reference run, `run` is the counted reference run after the text, `anchor`
/// is the text anchor point in centimetres, and `class_version` selects the
/// width of the run between the anchor and the text string.
fn txt_tag_sketch_text_record_at(
    properties: &[(&str, u64)],
    frame: &[u32],
    run: &[u32],
    anchor: (f64, f64),
    class_version: u32,
) -> Vec<u8> {
    txt_tag_sketch_text_record_at_with_rotation(properties, frame, run, anchor, class_version, 0.0)
}

fn txt_tag_sketch_text_record_at_with_rotation(
    properties: &[(&str, u64)],
    frame: &[u32],
    run: &[u32],
    anchor: (f64, f64),
    class_version: u32,
    rotation: f64,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    let push_ascii = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(
            &(u32::try_from(value.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        bytes.extend_from_slice(value.as_bytes());
    };
    let push_padded_reference = |bytes: &mut Vec<u8>, reference: u32| {
        push_reference(bytes, reference);
        bytes.extend_from_slice(&[0; 6]);
    };
    push_ascii(&mut bytes, "329");
    bytes.extend_from_slice(&304u64.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    // The leading block: a reference and a u32 per entry.
    bytes.push(1);
    bytes.extend_from_slice(
        &(u32::try_from(frame.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for reference in frame {
        push_padded_reference(&mut bytes, *reference);
        bytes.extend_from_slice(&[0; 4]);
    }
    bytes.push(1);
    bytes.extend_from_slice(
        &(u32::try_from(properties.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for (key, value) in properties {
        push_ascii(&mut bytes, key);
        push_ascii(&mut bytes, "IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&rotation.to_le_bytes());
    // Five bytes separate the rotation from the four f32 RGBA components.
    bytes.extend_from_slice(&[0; 5]);
    for component in [0.0f32, 0.3, 1.0, 1.0] {
        bytes.extend_from_slice(&component.to_le_bytes());
    }
    lp_utf16(&mut bytes, "Arial");
    bytes.extend_from_slice(&0.5f64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&anchor.0.to_le_bytes());
    bytes.extend_from_slice(&anchor.1.to_le_bytes());
    bytes.extend_from_slice(&vec![0u8; if class_version < 4 { 10 } else { 11 }]);
    lp_utf16(&mut bytes, "sketch text");
    bytes.extend_from_slice(
        &(u32::try_from(run.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for reference in run {
        push_padded_reference(&mut bytes, *reference);
    }
    let mut member_run = [0; 15];
    member_run[3..7].copy_from_slice(&400i32.to_le_bytes());
    bytes.extend_from_slice(&member_run);
    bytes.extend_from_slice(&[0; 30]);
    push_padded_reference(&mut bytes, 201);
    bytes
}

/// Build one `txt_tag` sketch-text record at the class version that writes an
/// identity key and the wider anchor run.
fn txt_tag_sketch_text_record(
    properties: &[(&str, u64)],
    frame: &[u32],
    run: &[u32],
    anchor: (f64, f64),
) -> Vec<u8> {
    txt_tag_sketch_text_record_at(properties, frame, run, anchor, 4)
}
