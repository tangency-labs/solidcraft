// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::native::bytes::NativeBytes;

#[derive(serde::Deserialize)]
pub(crate) struct RelationRecordCase {
    pub(crate) label: String,
    pub(crate) type_guid: String,
    pub(crate) class_version: u32,
    pub(crate) record_index: u32,
    pub(crate) class_tag: String,
    pub(crate) source_hex: NativeBytes,
    pub(crate) rectangular_flags: Option<[u8; 3]>,
    pub(crate) pattern_neutral_state: Option<bool>,
}

impl RelationRecordCase {
    pub(crate) fn decode(&self) -> crate::records::sketch_relations::SketchRelation {
        let record = crate::records::decal::DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:design-record-header#0".into(),
            record_index: self.record_index,
            class_tag: self.class_tag.clone().try_into().unwrap(),
            byte_offset: 0,
        };
        let mut records = Vec::new();
        crate::design::decode::sketch::decode_sketch_relation_record(
            &mut records,
            "Design/BulkStream.dat",
            &record,
            self.source_hex.as_ref(),
            &self.type_guid,
            self.class_version,
        )
        .unwrap();
        assert_eq!(records.len(), 1, "{}", self.label);
        records.pop().unwrap()
    }
}

pub(crate) fn relation_fixture_records() -> Vec<RelationRecordCase> {
    serde_json::from_str(&crate::test_support::verification::text(
        "sketch_relations/original_records.json",
        include_str!("sketch_relations/original_records.json"),
    ))
    .unwrap()
}

pub(crate) fn rectangular_pattern_record(
    count: u32,
    distance: f64,
    flags: [u8; 3],
    direction_reference_count: u32,
) -> Vec<u8> {
    rectangular_grid_pattern_record(
        [count, 1],
        [distance, 0.0],
        flags,
        direction_reference_count,
    )
}

pub(crate) fn rectangular_grid_pattern_record(
    counts: [u32; 2],
    distances: [f64; 2],
    flags: [u8; 3],
    direction_reference_count: u32,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    super::lp_ascii(&mut bytes, "300");
    bytes.extend_from_slice(&10u64.to_le_bytes());
    super::lp_ascii(&mut bytes, "");
    bytes.push(1);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&flags);
    bytes.extend_from_slice(&direction_reference_count.to_le_bytes());
    for index in 0..direction_reference_count {
        super::push_reference_u64(&mut bytes, u64::from(100 + index));
    }
    bytes.extend_from_slice(&[0; 8]);
    for (n, count_parameter, direction, value, distance_parameter) in [
        (counts[0], 20u64, [1.0f64, 0.0, 0.0], distances[0], 21u64),
        (counts[1], 22, [0.0, 1.0, 0.0], distances[1], 23),
    ] {
        bytes.extend_from_slice(&n.to_le_bytes());
        super::push_reference_u64(&mut bytes, count_parameter);
        for component in direction {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
        bytes.extend_from_slice(&value.to_le_bytes());
        super::push_reference_u64(&mut bytes, distance_parameter);
    }
    super::push_reference_u64(&mut bytes, 1);
    bytes.extend_from_slice(&0x2000_0000u64.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0);
    bytes
}

pub(crate) fn circular_pattern_record(angle: f64, count: u32, suppressed: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::new();
    super::lp_ascii(&mut bytes, "300");
    bytes.extend_from_slice(&10u64.to_le_bytes());
    super::lp_ascii(&mut bytes, "");
    bytes.push(1);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0);
    super::push_reference_u64(&mut bytes, 20);
    super::push_reference_u64(&mut bytes, 21);
    bytes.extend_from_slice(&angle.to_le_bytes());
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(suppressed.len()).unwrap().to_le_bytes());
    for ordinal in suppressed {
        bytes.extend_from_slice(&ordinal.to_le_bytes());
    }
    bytes.push(0);
    super::push_reference_u64(&mut bytes, 1);
    bytes.extend_from_slice(&0x1000_0000u64.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0);
    bytes
}
