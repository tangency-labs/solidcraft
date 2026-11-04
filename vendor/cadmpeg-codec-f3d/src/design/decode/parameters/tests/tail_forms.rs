// SPDX-License-Identifier: Apache-2.0
//! Parameter-tail and header-form tests.

use super::*;

fn parameter_text_field(text: &str) -> Vec<u8> {
    let mut field = Vec::new();
    lp_utf16(&mut field, text);
    field
}

/// The parameter tail is a reference to the stream's parameter collection, so
/// every header form admits any collection record index.
#[test]
fn parameter_tail_names_any_collection_record() {
    let set_target = |frame: &mut Vec<u8>, target: u32| {
        let tail = frame.len() - 10;
        frame[tail..tail + 4].copy_from_slice(&target.to_le_bytes());
    };
    let compact = |target| {
        let mut frame = compact_owned_parameter_record(
            428,
            9,
            "3.40 mm",
            "HoleDiameter",
            Some("mm"),
            "d9",
            0.34,
        );
        set_target(&mut frame, target);
        frame
    };
    let mut discriminated =
        parameter_record(Some(428), "3.40 mm", "HoleDiameter", Some("mm"), "d9", 0.34);
    set_target(&mut discriminated, 175);
    assert_eq!(
        parse_design_parameter_record(&discriminated)
            .expect("discriminated parameter")
            .name(),
        "d9"
    );
    let mut class_287 = class_287_parameter_record("HoleDepth", "d20");
    set_target(&mut class_287, 174);
    assert_eq!(
        parse_design_parameter_record(&class_287)
            .expect("class-287 parameter")
            .name(),
        "d20"
    );
    for target in [18, 19, 173, 174, 175, 0x0001_00AF] {
        assert_eq!(
            parse_design_parameter_record(&compact(target))
                .expect("parameter naming a collection")
                .name(),
            "d9"
        );
    }

    // A marked reference names a record, so its index is not zero.
    assert!(parse_design_parameter_record(&compact(0)).is_none());
    // The reference occupies eleven bytes: six zero bytes follow its index.
    let mut trailing = compact(175);
    *trailing.last_mut().unwrap() = 1;
    assert!(parse_design_parameter_record(&trailing).is_none());
    // Twelve bytes end the frame.
    let mut longer = compact(175);
    longer.push(0);
    assert!(parse_design_parameter_record(&longer).is_none());
}

/// A family discriminator does not depend on the source kind or the tail.
#[test]
fn parameter_discriminator_is_independent_of_source_kind_and_tail() {
    for discriminator in [0u64, 1, 2, 3, 4, 5, 6] {
        for target in [16u32, 19, 175] {
            for (source_kind, name) in [("AlongDistance", "d12"), ("TangencyWeight", "d81")] {
                let mut frame =
                    parameter_record(Some(44), "Width / 2", source_kind, Some("mm"), name, 3.0);
                frame[22..30].copy_from_slice(&discriminator.to_le_bytes());
                let tail = frame.len() - 10;
                frame[tail..tail + 4].copy_from_slice(&target.to_le_bytes());
                assert_eq!(
                    parse_design_parameter_record(&frame)
                        .expect("discriminated parameter")
                        .family_discriminator()
                        .map(|value| value.value.code()),
                    Some(discriminator)
                );
            }
        }
    }
    let mut frame = parameter_record(
        Some(44),
        "Width / 2",
        "AlongDistance",
        Some("mm"),
        "d12",
        3.0,
    );
    frame[22..30].copy_from_slice(&7u64.to_le_bytes());
    assert!(parse_design_parameter_record(&frame).is_none());
}

/// Discriminator 0 leaves offsets 22 through 29 zero, so the narrow reading is
/// tried first and the wide reading follows when the narrow reading does not close.
#[test]
fn wide_parameter_with_discriminator_0_reads_after_the_narrow_form_fails() {
    for (source_ordinal, owner) in [(4u32, 229u32), (1, 201), (0x200, 0x2_C2E2), (0x100, 0x100)] {
        let mut frame = parameter_record(Some(owner), "t", "AlongDistance", Some("mm"), "d4", 0.6);
        frame[22..30].copy_from_slice(&0u64.to_le_bytes());
        frame[31..35].copy_from_slice(&source_ordinal.to_le_bytes());
        frame.splice(22..22, [0; 4]);
        let expression_end = 50 + 4 + 2 * "t".len();
        frame.splice(expression_end..expression_end, [0; 2]);
        let parameter = parse_design_parameter_record(&frame).expect("wide parameter");
        assert_eq!(parameter.source_ordinal, source_ordinal);
        assert_eq!(parameter.owner_record_index(), Some(owner));
        assert_eq!(parameter.expression(), "t");
        assert_eq!(parameter.source_kind(), "AlongDistance");
        let discriminator = parameter.family_discriminator().expect("discriminator");
        assert_eq!((discriminator.value.code(), discriminator.offset), (0, 26));
    }
    let mut document = parameter_record(None, "3 mm", "User Parameter", Some("mm"), "w", 0.3);
    document[22..30].copy_from_slice(&0u64.to_le_bytes());
    document[31..35].copy_from_slice(&3u32.to_le_bytes());
    document.splice(22..22, [0; 4]);
    let expression_end = 40 + 4 + 2 * "3 mm".len();
    document.splice(expression_end..expression_end, [0; 2]);
    let parameter = parse_design_parameter_record(&document).expect("wide document parameter");
    assert_eq!(parameter.source_ordinal, 3);
    assert_eq!(parameter.owner_record_index(), None);
    assert_eq!(parameter.source_kind(), "User Parameter");

    // The eleven-byte document trailer marks a localized document source kind.
    let kind_at = 40 + 4 + 2 * "3 mm".len() + 11;
    let localize = |frame: &[u8], kind: &str| {
        let mut localized = frame.to_vec();
        localized.splice(
            kind_at..kind_at + 4 + 2 * "User Parameter".len(),
            parameter_text_field(kind),
        );
        localized
    };
    let localized = parse_design_parameter_record(&localize(&document, "Paramètre utilisateur"))
        .expect("localized document parameter");
    assert_eq!(localized.kind(), DesignParameterKind::User);
    assert_eq!(localized.source_kind(), "Paramètre utilisateur");
    assert!(parse_design_parameter_record(&localize(&document, "Expression")).is_none());

    // Discriminator 6 also stores the eleven-byte document trailer.
    let mut other = document;
    other[26..34].copy_from_slice(&6u64.to_le_bytes());
    let parameter = parse_design_parameter_record(&other).expect("discriminator-6 document");
    assert_eq!(parameter.kind(), DesignParameterKind::User);
    assert_eq!(
        parameter
            .family_discriminator()
            .map(|field| field.value.code()),
        Some(6)
    );
    other[26..34].copy_from_slice(&5u64.to_le_bytes());
    assert!(parse_design_parameter_record(&other).is_none());

    // The narrow reading wins when it closes.
    let mut narrow = parameter_record(Some(229), "t", "AlongDistance", Some("mm"), "d4", 0.6);
    narrow[22..30].copy_from_slice(&0u64.to_le_bytes());
    let parameter = parse_design_parameter_record(&narrow).expect("narrow parameter");
    assert_eq!(
        parameter.family_discriminator().map(|field| field.offset),
        Some(22)
    );
}

#[test]
fn decode_reports_frames_that_name_a_parameter_collection_and_match_no_grammar() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let mut bulk = parameter_record(Some(44), "first", "AlongDistance", Some("mm"), "d1", 1.0);
    let mut rejected = parameter_record(Some(46), "second", "AlongDistance", Some("mm"), "d2", 2.0);
    rejected[7] = 47;
    let expression_end = 46 + 4 + 2 * "second".len();
    rejected[expression_end] = 7;
    bulk.extend_from_slice(&rejected);
    let mut other_target = rejected.clone();
    other_target[7] = 49;
    let tail = other_target.len() - 10;
    other_target[tail] = 20;
    bulk.extend_from_slice(&other_target);
    // The document flag with a source kind outside the document kinds matches
    // no grammar.
    let mut localized = parameter_record(None, "12 mm", "Expression", Some("mm"), "o", 1.2);
    localized[7] = 51;
    bulk.extend_from_slice(&localized);
    // A non-finite value closes the grammar and defines no record; the decode
    // continues.
    let mut non_finite = parameter_record(
        Some(54),
        "12 mm",
        "AlongDistance",
        Some("mm"),
        "n",
        f64::NAN,
    );
    non_finite[7] = 55;
    bulk.extend_from_slice(&non_finite);
    let mut last = parameter_record(Some(52), "last", "AlongDistance", Some("mm"), "d3", 3.0);
    last[7] = 53;
    bulk.extend_from_slice(&last);
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    let metadata = crate::test_support::streams_test::design_metastream_with_records(
        &[(
            "B0556F58-EE09-46A7-B18F-E296F48E5B13",
            "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081",
            3,
            "CommonData",
            &[19],
        )],
        &[],
    );
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&metadata).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let decoded = with_scan(&archive, decode_parameters).unwrap();
    let names: Vec<_> = decoded
        .parameters
        .iter()
        .map(crate::records::parameters::DesignParameter::name)
        .collect();
    assert_eq!(names, ["d1", "d3"]);
    let [unparsed] = decoded.unparsed.as_slice() else {
        panic!("expected one stream with unparsed parameter frames");
    };
    assert_eq!(unparsed.stream, stream);
    assert_eq!(unparsed.count, 3);
}

#[test]
fn class_287_frame_outside_the_compact_form_reads_as_a_wide_owned_parameter() {
    let mut frame = parameter_record(Some(0x106), "360 deg", "AlongAngle", Some("deg"), "d1", 6.0);
    frame[4..7].copy_from_slice(b"287");
    frame[22..30].copy_from_slice(&6u64.to_le_bytes());
    frame[31..35].copy_from_slice(&1u32.to_le_bytes());
    frame.splice(22..22, [0; 4]);
    let expression_end = 50 + 4 + 2 * "360 deg".len();
    frame.splice(expression_end..expression_end, [0; 2]);
    let parameter = parse_design_parameter_record(&frame).expect("wide owned class-287 parameter");
    assert_eq!(parameter.class_tag.as_str(), "287");
    assert_eq!(parameter.owner_record_index(), Some(0x106));
    assert_eq!(parameter.source_kind(), "AlongAngle");
    assert_eq!(parameter.name(), "d1");
    assert_eq!(
        parameter
            .family_discriminator()
            .map(|field| (field.value.code(), field.offset)),
        Some((6, 26))
    );
}
