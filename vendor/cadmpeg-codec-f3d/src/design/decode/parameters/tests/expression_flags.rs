// SPDX-License-Identifier: Apache-2.0

use super::*;

/// A wide discriminator-6 owned dimension parameter with the given wide
/// expression trailer.
fn wide_owned(trailer: [u8; 11]) -> Vec<u8> {
    let mut frame = parameter_record(
        Some(0xee),
        "40.00 mm",
        "Linear Dimension-2",
        Some("mm"),
        "d1",
        4.0,
    );
    frame[22..30].copy_from_slice(&6u64.to_le_bytes());
    frame.splice(22..22, [0; 4]);
    let expression_end = 50 + 4 + 2 * "40.00 mm".len();
    frame.splice(expression_end..expression_end + 9, trailer);
    frame
}

/// A wide discriminator-6 document parameter with the given wide expression
/// trailer.
fn wide_document(trailer: [u8; 11]) -> Vec<u8> {
    let mut frame = parameter_record(None, "12 mm", "User Parameter", Some("mm"), "uf", 1.2);
    frame[22..30].copy_from_slice(&6u64.to_le_bytes());
    frame.splice(22..22, [0; 4]);
    let expression_end = 40 + 4 + 2 * "12 mm".len();
    frame.splice(expression_end..expression_end + 9, trailer);
    frame
}

#[test]
fn wide_expression_trailer_states_the_driven_and_favorite_flags() {
    let read = |trailer: [u8; 11]| {
        parse_design_parameter_record(&wide_owned(trailer)).map(|parameter| {
            let flags = parameter.flags();
            (flags.driven, flags.favorite, parameter.driving())
        })
    };
    assert_eq!(read([0; 11]), Some((false, false, None)));
    assert_eq!(
        read([0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]),
        Some((true, false, Some(false)))
    );
    assert_eq!(
        read([0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0]),
        Some((false, true, None))
    );
    assert_eq!(
        read([0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0]),
        Some((true, true, Some(false)))
    );
    // Reserved bytes, other flag values, and an owned parameter with the
    // document flag match no grammar.
    for refused in [
        [0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    ] {
        assert_eq!(read(refused), None, "{refused:?}");
    }
}

#[test]
fn wide_document_trailer_states_the_favorite_flag() {
    let parameter =
        parse_design_parameter_record(&wide_document([0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1]))
            .expect("favorite document parameter");
    assert_eq!(parameter.kind(), DesignParameterKind::User);
    assert!(parameter.flags().favorite);
    assert_eq!(parameter.driving(), None);
    let plain = parse_design_parameter_record(&wide_document([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]))
        .expect("document parameter");
    assert!(!plain.flags().favorite);
    // A document parameter has driven flag 0.
    assert!(
        parse_design_parameter_record(&wide_document([0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1])).is_none()
    );
}

#[test]
fn compact_owned_trailer_states_the_driven_flag() {
    let read = |trailer: [u8; 5]| {
        let mut frame = compact_owned_parameter_record(
            0x454,
            0x0f,
            "1.8150388199 mm",
            "Linear Dimension-2",
            Some("mm"),
            "d15",
            0.181_503_881_99,
        );
        let expression_end = 41 + 4 + 2 * "1.8150388199 mm".len();
        frame[expression_end..expression_end + 5].copy_from_slice(&trailer);
        parse_design_parameter_record(&frame)
    };
    let driven = read([0, 1, 0, 0, 0]).expect("driven compact parameter");
    assert!(driven.flags().driven);
    assert_eq!(driven.driving(), Some(false));
    let driving = read([0; 5]).expect("driving compact parameter");
    assert!(!driving.flags().driven);
    assert_eq!(driving.driving(), None);
    for refused in [
        [1, 0, 0, 0, 0],
        [0, 2, 0, 0, 0],
        [0, 0, 1, 0, 0],
        [0, 0, 0, 0, 1],
    ] {
        assert!(read(refused).is_none(), "{refused:?}");
    }
}
