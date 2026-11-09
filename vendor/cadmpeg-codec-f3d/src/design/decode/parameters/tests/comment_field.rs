// SPDX-License-Identifier: Apache-2.0

use super::*;

/// The LP-UTF16 field of `text`.
fn text_field(text: &str) -> Vec<u8> {
    let mut field = Vec::new();
    lp_utf16(&mut field, text);
    field
}

/// A wide discriminator-6 owned dimension parameter with the given comment.
fn wide_owned(comment: &str) -> Vec<u8> {
    let mut frame = parameter_record(
        Some(0xee),
        "40.00 mm",
        "Linear Dimension-2",
        Some("mm"),
        "d1",
        4.0,
    );
    frame[22..30].copy_from_slice(&6u64.to_le_bytes());
    frame[31..35].copy_from_slice(&3u32.to_le_bytes());
    frame.splice(22..22, [0; 4]);
    let expression_end = 50 + 4 + 2 * "40.00 mm".len();
    frame.splice(expression_end..expression_end, [0; 2]);
    frame.splice(31..35, text_field(comment));
    frame
}

/// A wide discriminator-6 document parameter with the given comment.
fn wide_document(comment: &str) -> Vec<u8> {
    let mut frame = parameter_record(None, "12 mm", "User Parameter", Some("mm"), "uf", 1.2);
    frame[22..30].copy_from_slice(&6u64.to_le_bytes());
    frame.splice(22..22, [0; 4]);
    let expression_end = 40 + 4 + 2 * "12 mm".len();
    frame.splice(expression_end..expression_end, [0; 2]);
    frame.splice(31..35, text_field(comment));
    frame
}

#[test]
fn wide_parameter_comment_moves_the_later_fields() {
    for comment in ["note", "Ø entraxe 12"] {
        let parameter = parse_design_parameter_record(&wide_owned(comment)).expect("commented");
        assert_eq!(parameter.comment(), Some(comment));
        assert_eq!(parameter.source_ordinal, 3);
        assert_eq!(parameter.owner_record_index(), Some(0xee));
        assert_eq!(parameter.expression(), "40.00 mm");
        assert_eq!(parameter.name(), "d1");
        assert_eq!(
            parameter
                .family_discriminator()
                .map(|field| (field.value.code(), field.offset)),
            Some((6, 26))
        );
        assert_eq!(
            parameter.expression_offset(),
            u64::try_from(50 + 4 + 2 * comment.encode_utf16().count()).unwrap()
        );

        let document = parse_design_parameter_record(&wide_document(comment)).expect("document");
        assert_eq!(document.kind(), DesignParameterKind::User);
        assert_eq!(document.comment(), Some(comment));
        assert_eq!(document.expression(), "12 mm");
    }
    let plain = parse_design_parameter_record(&wide_owned("")).expect("no comment");
    assert_eq!(plain.comment(), None);
    assert_eq!(plain.expression_offset(), 54);
}

#[test]
fn wide_parameter_keeps_whitespace_comment_and_offset() {
    let parameter = parse_design_parameter_record(&wide_owned("  ")).expect("comment");
    assert_eq!(parameter.comment(), Some("  "));
    assert_eq!(
        serde_json::to_value(&parameter).unwrap()["comment_offset"],
        35
    );
    assert_eq!(parameter.expression_offset(), 58);
}

#[test]
fn wide_parameter_comment_requires_its_zero_byte_and_complete_code_units() {
    let mut marked = wide_owned("note");
    assert_eq!(wide_parameter_comment(&marked), Some(("note".into(), 43)));
    marked[30] = 1;
    assert!(wide_parameter_comment(&marked).is_none());
    assert!(parse_design_parameter_record(&marked).is_none());

    let mut overlong = wide_owned("note");
    overlong[31..35].copy_from_slice(&5u32.to_le_bytes());
    assert!(parse_design_parameter_record(&overlong).is_none());

    let mut discriminator = wide_owned("note");
    discriminator[26..30].copy_from_slice(&7u32.to_le_bytes());
    assert!(parse_design_parameter_record(&discriminator).is_none());
}

fn comment_discriminated(owner: Option<u32>, comment: &str, trailer: [u8; 5]) -> Vec<u8> {
    let mut frame = Vec::new();
    frame.extend_from_slice(&3u32.to_le_bytes());
    frame.extend_from_slice(b"328");
    frame.extend_from_slice(&42u32.to_le_bytes());
    frame.extend_from_slice(&[0; 11]);
    frame.extend_from_slice(&6u32.to_le_bytes());
    lp_utf16(&mut frame, comment);
    frame.extend_from_slice(&3u32.to_le_bytes());
    if let Some(owner) = owner {
        frame.push(1);
        frame.extend_from_slice(&owner.to_le_bytes());
        frame.extend_from_slice(&[0; 6]);
    } else {
        frame.push(0);
    }
    lp_utf16(&mut frame, "40 mm");
    frame.extend_from_slice(&trailer);
    lp_utf16(
        &mut frame,
        if owner.is_some() {
            "Linear Dimension-2"
        } else {
            "Benutzerparameter"
        },
    );
    lp_utf16(&mut frame, "mm");
    lp_utf16(&mut frame, "d1");
    frame.extend_from_slice(&4.0f64.to_le_bytes());
    frame.extend_from_slice(&[0, 1, 19, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    frame
}

#[test]
fn comment_discriminated_owned_parameter_keeps_its_fields() {
    let frame = comment_discriminated(Some(0xee), "", [1, 0, 1, 0, 0]);
    let parameter = parse_design_parameter_record(&frame).expect("owned parameter");
    assert_eq!(parameter.family_discriminator().unwrap().value.code(), 6);
    assert_eq!(parameter.source_ordinal, 3);
    assert_eq!(parameter.owner_record_index(), Some(0xee));
    assert_eq!(parameter.kind(), DesignParameterKind::Dimension);
    assert_eq!(parameter.expression(), "40 mm");
    assert_eq!(parameter.source_kind_str(), "Linear Dimension-2");
    assert_eq!(parameter.unit().unwrap().value, "mm");
    assert_eq!(parameter.name(), "d1");
    assert_eq!(parameter.evaluated_value().get(), 4.0);
    assert_eq!(parameter.comment(), None);
    assert!(parameter.flags().driven);
    assert!(parameter.flags().favorite);
}

#[test]
fn comment_discriminated_document_parameter_keeps_comment_and_flag() {
    let frame = comment_discriminated(None, "note", [0, 0, 1, 0, 1]);
    let parameter = parse_design_parameter_record(&frame).expect("document parameter");
    assert_eq!(parameter.kind(), DesignParameterKind::User);
    assert_eq!(parameter.source_kind_str(), "Benutzerparameter");
    assert_eq!(parameter.owner_record_index(), None);
    assert_eq!(parameter.comment(), Some("note"));
    assert!(!parameter.flags().driven);
    assert!(parameter.flags().favorite);
    let wire = serde_json::to_value(&parameter).unwrap();
    assert_eq!(wire["comment_offset"], 30);
    let round_trip: crate::records::parameters::DesignParameter =
        serde_json::from_value(wire).unwrap();
    assert_eq!(round_trip.kind(), DesignParameterKind::User);
    assert_eq!(round_trip.comment(), Some("note"));
}

#[test]
fn comment_discriminated_parameter_keeps_whitespace_comment_and_offset() {
    let frame = comment_discriminated(Some(0xee), "  ", [0, 0, 0, 0, 0]);
    let parameter = parse_design_parameter_record(&frame).expect("comment");
    assert_eq!(parameter.comment(), Some("  "));
    assert_eq!(
        serde_json::to_value(&parameter).unwrap()["comment_offset"],
        30
    );
}

#[test]
fn comment_discriminated_unit_reading_requires_frame_closure() {
    let frame = comment_discriminated(None, "note", [0, 0, 0, 0, 1]);
    let unit = [2, 0, 0, 0, b'm', 0, b'm', 0];
    let unit_at = frame
        .windows(unit.len())
        .position(|bytes| bytes == unit)
        .unwrap();

    let mut empty_unit = frame.clone();
    empty_unit.splice(unit_at..unit_at + unit.len(), [0; 4]);
    let parameter = parse_design_parameter_record(&empty_unit).unwrap();
    assert!(parameter.unit().is_none());
    assert_eq!(parameter.name(), "d1");

    let mut no_unit = frame;
    no_unit.drain(unit_at..unit_at + unit.len());
    let scalar_at = no_unit.len() - 20;
    no_unit[scalar_at..scalar_at + 8].copy_from_slice(&[1, 0, 0, 0, b'A', 0, 0, 0]);
    let parameter = parse_design_parameter_record(&no_unit).unwrap();
    assert!(parameter.unit().is_none());
    assert_eq!(parameter.name(), "d1");
}

#[test]
fn comment_discriminated_document_parameter_rejects_driven_flag() {
    let frame = comment_discriminated(None, "note", [1, 0, 1, 0, 1]);
    assert!(parse_design_parameter_record(&frame).is_none());
}

#[test]
fn comment_discriminated_rejects_invalid_fields_and_extra_bytes() {
    assert!(
        parse_design_parameter_record(&comment_discriminated(Some(0xee), "", [0, 0, 0, 0, 1]))
            .is_none()
    );
    assert!(
        parse_design_parameter_record(&comment_discriminated(None, "note", [0, 1, 0, 0, 1]))
            .is_none()
    );
    assert!(
        parse_design_parameter_record(&comment_discriminated(None, "note", [0, 0, 0, 1, 1]))
            .is_none()
    );
    for trailer in [[2, 0, 0, 0, 1], [0, 0, 2, 0, 1], [0, 0, 0, 0, 2]] {
        assert!(
            parse_design_parameter_record(&comment_discriminated(None, "note", trailer)).is_none()
        );
    }
    let mut discriminator = comment_discriminated(None, "note", [0, 0, 0, 0, 1]);
    discriminator[22..26].copy_from_slice(&7u32.to_le_bytes());
    assert!(parse_design_parameter_record(&discriminator).is_none());
    let mut longer = comment_discriminated(None, "note", [0, 0, 0, 0, 1]);
    longer.push(0);
    assert!(parse_design_parameter_record(&longer).is_none());
}
