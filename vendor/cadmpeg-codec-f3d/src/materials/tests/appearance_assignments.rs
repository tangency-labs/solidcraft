// SPDX-License-Identifier: Apache-2.0

use crate::bytes::lp_utf16_bytes;

#[test]
fn browser_body_appearance_joins_through_browser_node_guid() {
    let mut bytes = vec![0u8; 8];
    let records = [
        (
            "1b5e92d0-eade-40d5-ab4d-35af2eb411b4",
            "674E6024-4294-4322-B572-A88F64F0DA77_Post2015_Post2015",
            37_251u64,
        ),
        (
            "a349885b-a9b6-4b79-a9c9-7976717ee6be",
            "4218E352-E25F-423E-8DCD-527E5148C2F6_Post2015_Post2015",
            37_441u64,
        ),
    ];
    for (node_guid, visual, entity_suffix) in records {
        for value in [
            "e966e81d-2581-4d41-821d-839938974425",
            node_guid,
            "DE897CF7-F483-4D31-A2D8-41671FE36D3D",
            "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C",
            "PrismMaterial-018",
            "ba2d3026-32c4-4584-b0e1-a738e387fa35",
            visual,
            "BA5EE55E-9982-449B-9D66-9F036540E140",
            "Prism-090",
        ] {
            bytes.extend(lp_utf16_bytes(value).expect("fixture UTF-16 code-unit count fits u32"));
        }
        bytes.extend(lp_utf16_bytes(node_guid).expect("fixture UTF-16 code-unit count fits u32"));
        bytes.push(0);
        bytes.extend([0x01, 0x01]);
        bytes.extend(entity_suffix.to_le_bytes());
    }

    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            crate::materials::browser_body_appearances(ctx, &bytes).unwrap()
        }),
        [
            (
                37_251,
                crate::records::references::DesignVisualToken::try_from(records[0].1.to_string())
                    .unwrap()
            ),
            (
                37_441,
                crate::records::references::DesignVisualToken::try_from(records[1].1.to_string())
                    .unwrap()
            ),
        ]
    );
    assert!(
        crate::materials::face_appearance_assignments(&bytes).is_empty(),
        "a body-owned visual marker is not also a face assignment"
    );
}

#[test]
fn browser_body_appearance_scan_rejects_binary_utf16_length_candidates() {
    let mut bytes = vec![0u8; 8];
    for _ in 0..32 {
        bytes.extend_from_slice(&256u32.to_le_bytes());
        bytes.extend(std::iter::repeat_n(0, 256 * 2));
    }

    assert!(super::super::lp_utf16_strings(&bytes).is_empty());
}

#[test]
fn legacy_face_appearance_assignment_decodes_both_variable_width_forms() {
    let face_guid = "cd92d0f6-5b31-4bbf-84ae-4611f435537e";
    let visual_guid = "F0EF16AD-4AD3-4D25-9AA8-ECF48936A48F_Post2015";
    let first = legacy_face_appearance_entry(
        face_guid,
        [0.25, 0.5, 0.75, 1.0],
        visual_guid,
        0,
        None,
        "Prism-042",
    );
    let second = legacy_face_appearance_entry(
        "e6c14fe2-6c11-4a22-8ccc-c10fba912345",
        [0.75, 0.25, 0.5, 1.0],
        "A1C44310-E91B-4B59-B527-18265C123456_Post2015",
        1,
        Some("X"),
        "PrismOpaque",
    );
    assert_ne!(first.len(), second.len());

    let mut bytes = vec![0u8; 8];
    bytes.extend(first);
    bytes.extend(second);
    let out = crate::materials::face_appearance_assignments(&bytes);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].face_guid, face_guid);
    assert_eq!(&*out[0].visual_guid, visual_guid);
    assert_eq!(
        out[0].color,
        Some(cadmpeg_ir::topology::Color::new(0.25, 0.5, 0.75, 1.0).expect("valid color"))
    );
    assert_eq!(
        out[1].color,
        Some(cadmpeg_ir::topology::Color::new(0.75, 0.25, 0.5, 1.0).expect("valid color"))
    );
}

#[test]
fn face_appearance_assignment_rejects_entity_id_and_uppercase_targets() {
    for target in [
        "0_985",
        "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C",
        "c1eea57c-3f56-45fc-b8cb-a9ec46a9994C",
    ] {
        let bytes = legacy_face_appearance_entry(
            target,
            [0.25, 0.5, 0.75, 1.0],
            "F0EF16AD-4AD3-4D25-9AA8-ECF48936A48F_Post2015",
            1,
            None,
            "PrismOpaque",
        );
        assert!(crate::materials::face_appearance_assignments(&bytes).is_empty());
    }
}

#[test]
fn legacy_face_appearance_assignment_rejects_partial_and_malformed_envelopes() {
    let face_guid = "cd92d0f6-5b31-4bbf-84ae-4611f435537e";
    let visual_guid = "F0EF16AD-4AD3-4D25-9AA8-ECF48936A48F_Post2015";
    let mut partial = lp_utf16_bytes(face_guid).expect("fixture UTF-16 code-unit count fits u32");
    partial.extend(lp_utf16_bytes(visual_guid).expect("fixture UTF-16 code-unit count fits u32"));
    partial.extend(
        lp_utf16_bytes("BA5EE55E-9982-449B-9D66-9F036540E140")
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    assert!(crate::materials::face_appearance_assignments(&partial).is_empty());

    let mut malformed = legacy_face_appearance_entry(
        face_guid,
        [0.25, 0.5, 0.75, 1.0],
        visual_guid,
        1,
        None,
        "PrismOpaque",
    );
    let carrier_at = lp_utf16_bytes(face_guid)
        .expect("fixture UTF-16 code-unit count fits u32")
        .len()
        + 4 * size_of::<f32>();
    malformed[carrier_at + 2] = 1;
    assert!(crate::materials::face_appearance_assignments(&malformed).is_empty());
}

fn legacy_face_appearance_entry(
    face_guid: &str,
    color: [f32; 4],
    visual_guid: &str,
    selector_kind: u8,
    display_name: Option<&str>,
    selector: &str,
) -> Vec<u8> {
    let mut bytes = lp_utf16_bytes(face_guid).expect("fixture UTF-16 code-unit count fits u32");
    for component in color {
        bytes.extend(component.to_le_bytes());
    }
    bytes.extend([1, 1]);
    bytes.extend([0; 9]);
    bytes.push(selector_kind);
    bytes.extend(lp_utf16_bytes(visual_guid).expect("fixture UTF-16 code-unit count fits u32"));
    bytes.extend(
        lp_utf16_bytes("BA5EE55E-9982-449B-9D66-9F036540E140")
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    if let Some(display_name) = display_name {
        bytes
            .extend(lp_utf16_bytes(display_name).expect("fixture UTF-16 code-unit count fits u32"));
    } else {
        bytes.extend(0_u32.to_le_bytes());
    }
    bytes.extend(lp_utf16_bytes(selector).expect("fixture UTF-16 code-unit count fits u32"));
    bytes.extend(0_f32.to_le_bytes());
    bytes.extend(1_f32.to_le_bytes());
    bytes
}

#[test]
fn modern_face_appearance_assignment_uses_second_framed_lowercase_guid() {
    let unrelated_guid = "11111111-1111-1111-1111-111111111111";
    let first_guid = "22222222-2222-2222-2222-222222222222";
    let face_guid = "33333333-3333-3333-3333-333333333333";
    let visual_guid = "F0EF16AD-4AD3-4D25-9AA8-ECF48936A48F_Post2015";
    let mut bytes =
        lp_utf16_bytes(unrelated_guid).expect("fixture UTF-16 code-unit count fits u32");
    bytes.extend(lp_utf16_bytes(first_guid).expect("fixture UTF-16 code-unit count fits u32"));
    bytes.extend([0xa5; 8]);
    bytes.extend([0; 8]);
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend([1, 1, 0, 0, 0]);
    bytes.extend(lp_utf16_bytes(face_guid).expect("fixture UTF-16 code-unit count fits u32"));
    bytes.extend([0; 12]);
    bytes.extend(1_f32.to_le_bytes());
    bytes.extend([1, 1]);
    bytes.extend([0; 10]);
    for value in [visual_guid, "08861000-1D69-CF2A-C082-CBD98E7E5D7F"] {
        bytes.extend(lp_utf16_bytes(value).expect("fixture UTF-16 code-unit count fits u32"));
    }
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(
        lp_utf16_bytes("005E1000-55CE-AFB6-81A1-36E3EF077C5F")
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    let out = crate::materials::face_appearance_assignments(&bytes);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].face_guid, face_guid);
    assert_eq!(&*out[0].visual_guid, visual_guid);
    assert_eq!(out[0].color, None);

    let mut malformed = bytes;
    let first_gap_at = lp_utf16_bytes(unrelated_guid)
        .expect("fixture UTF-16 code-unit count fits u32")
        .len()
        + lp_utf16_bytes(first_guid)
            .expect("fixture UTF-16 code-unit count fits u32")
            .len();
    malformed[first_gap_at + 8] = 1;
    assert!(crate::materials::face_appearance_assignments(&malformed).is_empty());
}

#[test]
fn modern_face_appearance_assignment_requires_the_first_guid_carrier() {
    let mut bytes = vec![0u8; 8];
    for value in [
        "22222222-2222-2222-2222-222222222222",
        "F0EF16AD-4AD3-4D25-9AA8-ECF48936A48F_Post2015",
        "08861000-1D69-CF2A-C082-CBD98E7E5D7F",
    ] {
        bytes.extend(lp_utf16_bytes(value).expect("fixture UTF-16 code-unit count fits u32"));
    }
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(
        lp_utf16_bytes("005E1000-55CE-AFB6-81A1-36E3EF077C5F")
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    assert!(crate::materials::face_appearance_assignments(&bytes).is_empty());
}

#[test]
fn modern_body_appearance_is_not_a_face_assignment() {
    let mut bytes = vec![0u8; 8];
    for value in [
        "11111111-1111-1111-1111-111111111111",
        "PrismMaterial-018",
        "F0EF16AD-4AD3-4D25-9AA8-ECF48936A48F_Post2015",
        "08861000-1D69-CF2A-C082-CBD98E7E5D7F",
    ] {
        bytes.extend(lp_utf16_bytes(value).expect("fixture UTF-16 code-unit count fits u32"));
    }
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(
        lp_utf16_bytes("005E1000-55CE-AFB6-81A1-36E3EF077C5F")
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    assert!(crate::materials::face_appearance_assignments(&bytes).is_empty());
}
