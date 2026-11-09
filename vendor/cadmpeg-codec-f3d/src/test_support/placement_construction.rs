// SPDX-License-Identifier: Apache-2.0
const FRAMES: [&str; 5] = [
    include_str!("placement_construction/extra-scalar-v14.json"),
    include_str!("placement_construction/unmarked-v4.json"),
    include_str!("placement_construction/explicit-v4.json"),
    include_str!("placement_construction/identity-v6.json"),
    include_str!("placement_construction/unmarked-alignment-v4.json"),
];

pub(crate) fn frames() -> [String; 5] {
    let names = [
        "extra-scalar-v14",
        "unmarked-v4",
        "explicit-v4",
        "identity-v6",
        "unmarked-alignment-v4",
    ];
    std::array::from_fn(|index| {
        crate::test_support::verification::text(
            &format!("placement_construction/{}.json", names[index]),
            FRAMES[index],
        )
    })
}

pub(crate) fn metadata(
    frame: crate::metastream::PrimaryRecordFrame,
    version: u32,
    type_guid: &str,
    base_guid: &str,
    with_secondary: bool,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    super::lp_ascii(&mut bytes, "Design");
    bytes.extend_from_slice(&0u32.to_le_bytes());
    super::lp_utf16(&mut bytes, "11111111-2222-3333-4444-555555555555");
    for value in [1234u32, 0, 0, 0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    super::lp_ascii(&mut bytes, "Design");
    super::lp_ascii(&mut bytes, "");
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.extend_from_slice(&2u32.to_le_bytes());
    super::lp_ascii(&mut bytes, type_guid);
    super::lp_ascii(&mut bytes, base_guid);
    bytes.extend_from_slice(&version.to_le_bytes());
    super::lp_ascii(&mut bytes, "Fusion");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&frame.entity_id.to_le_bytes());
    super::lp_ascii(&mut bytes, base_guid);
    super::lp_ascii(&mut bytes, "");
    bytes.extend_from_slice(&0u32.to_le_bytes());
    super::lp_ascii(&mut bytes, "Fusion");
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&frame.entity_id.to_le_bytes());
    bytes.extend_from_slice(&cadmpeg_core::decode::u64_from_index(frame.start).to_le_bytes());
    bytes.extend_from_slice(&u32::from(with_secondary).to_le_bytes());
    if with_secondary {
        bytes.extend_from_slice(&frame.entity_id.to_le_bytes());
        bytes.extend_from_slice(
            &cadmpeg_core::decode::u64_from_index(frame.member_end).to_le_bytes(),
        );
    }
    bytes
}

pub(crate) fn modern_frame(version: u32, identity: bool) -> Vec<u8> {
    use cadmpeg_ir::native::bytes::NativeBytes;
    let fixture: serde_json::Value = serde_json::from_str(FRAMES[0]).unwrap();
    let raw: NativeBytes<Vec<u8>> =
        serde_json::from_value(fixture["source_bytes"].clone()).unwrap();
    let native: crate::records::placement_construction::DesignPlacementConstruction =
        serde_json::from_value(fixture["native"].clone()).unwrap();
    let relative = |offset: u64| usize::try_from(offset - native.byte_offset).unwrap();
    let mut bytes = raw.into_inner();
    let arc = relative(
        native
            .fields
            .arc_length_for_periodic_face
            .as_ref()
            .unwrap()
            .offset,
    );
    if version == 16 {
        bytes.splice(arc..arc + 8, [0]);
    } else {
        bytes[relative(native.fields.use_u_direction.as_ref().unwrap().offset)] = 0;
    }
    if identity {
        let marker = relative(native.fields.identity.as_ref().unwrap().offset)
            - if version == 16 { 7 } else { 0 };
        bytes[marker] = 1;
        bytes.drain(marker + 1..marker + 129);
    }
    bytes
}

pub(crate) fn modern_placements(
) -> Vec<crate::records::placement_construction::DesignPlacementConstruction> {
    let mut result = Vec::new();
    crate::test_support::with_decode_context(|ctx| {
        for version in [15, 16] {
            for identity in [false, true] {
                let bytes = modern_frame(version, identity);
                result.push(
                    crate::design::decode::placement_construction::decode_frame(
                        ctx,
                        "f3d:Design/BulkStream.dat:design-placement-construction#0".into(),
                        cadmpeg_core::decode::View::over_retained(&bytes),
                        version,
                    )
                    .unwrap()
                    .unwrap(),
                );
            }
        }
    });
    result
}

pub(crate) const GRAMMAR_FRAME_ROWS: [[f64; 4]; 4] = [
    [0.0, 0.0, 1.0, -3.5],
    [1.0, 0.0, 0.0, 0.25],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

fn grammar_frame(version: u32, use_u_direction: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    super::indexed_header(&mut bytes, *b"328", 41);
    bytes.extend_from_slice(&[0; 10]);
    for value in [0.0_f64; 3] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    if version >= 8 {
        bytes.push(0);
    }
    if version >= 10 {
        bytes.push(use_u_direction);
    }
    bytes.push(0);
    if version >= 9 {
        bytes.extend_from_slice(&[0; 4]);
    }
    bytes.extend_from_slice(&[0; 4]);
    for value in GRAMMAR_FRAME_ROWS.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [std::f64::consts::FRAC_PI_2, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&38u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes
}

pub(crate) fn grammar_placement(
    version: u32,
    use_u_direction: u8,
) -> crate::records::placement_construction::DesignPlacementConstruction {
    let bytes = grammar_frame(version, use_u_direction);
    crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::placement_construction::decode_frame(
            ctx,
            "f3d:Design/BulkStream.dat:design-placement-construction#0".into(),
            cadmpeg_core::decode::View::over_retained(&bytes),
            version,
        )
        .unwrap()
        .unwrap()
    })
}
