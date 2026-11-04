// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::native::bytes::NativeBytes;

#[derive(serde::Deserialize)]
pub(crate) struct TextRecordCase {
    pub(crate) label: String,
    pub(crate) class_tag: String,
    pub(crate) class_version: u32,
    pub(crate) record_index: u32,
    pub(crate) source_hex: NativeBytes,
    pub(crate) text: String,
    pub(crate) font_family: String,
    pub(crate) font_weight: i32,
    pub(crate) height_cm: f64,
    pub(crate) neutral_state: bool,
}

impl TextRecordCase {
    pub(crate) fn decode(&self) -> crate::records::sketch_geometry::SketchText {
        crate::design::decode::sketch::decode_sketch_text_record(
            self.source_hex.as_ref(),
            "Design/BulkStream.dat",
            self.class_tag.clone().try_into().unwrap(),
            self.class_version,
            self.record_index,
            0,
        )
        .unwrap()
        .unwrap_or_else(|| panic!("{}", self.label))
    }
}

pub(crate) fn text_fixture_records() -> Vec<TextRecordCase> {
    serde_json::from_str(&crate::test_support::verification::text(
        "sketch_text/original_records.json",
        include_str!("sketch_text/original_records.json"),
    ))
    .unwrap()
}

pub(crate) fn neutral_text_record() -> crate::records::sketch_geometry::SketchText {
    let mut bytes = Vec::new();
    super::lp_ascii(&mut bytes, "376");
    bytes.extend_from_slice(&20u64.to_le_bytes());
    super::lp_ascii(&mut bytes, "");
    bytes.extend_from_slice(&[0, 1]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for (key, value) in [("EntityGenesis", 0u64), ("textex_tag", 20)] {
        super::lp_ascii(&mut bytes, key);
        super::lp_ascii(&mut bytes, "IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(1);
    bytes.extend_from_slice(&1.0f64.to_le_bytes());
    for component in [0.0f32, 0.0, 0.0, 1.0] {
        bytes.extend_from_slice(&component.to_le_bytes());
    }
    super::lp_utf16(&mut bytes, "Arial");
    bytes.push(0);
    bytes.extend_from_slice(&1.0f64.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 3]);
    super::lp_utf16(&mut bytes, "A");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&400i32.to_le_bytes());
    bytes.extend_from_slice(&[0; 5]);
    for row in 0..4 {
        for column in 0..4 {
            bytes.extend_from_slice(&(if row == column { 1.0f64 } else { 0.0 }).to_le_bytes());
        }
    }
    bytes.extend_from_slice(&[0, 0, 0, 0, 1]);
    for component in [0.0f32, 0.0, 0.0, 1.0, 1.0] {
        bytes.extend_from_slice(&component.to_le_bytes());
    }
    bytes.extend_from_slice(&[0; 5]);
    super::push_reference_u64(&mut bytes, 42);
    crate::design::decode::sketch::decode_sketch_text_record(
        &bytes,
        "BulkStream.dat",
        "376".to_owned().try_into().unwrap(),
        0,
        20,
        20,
    )
    .unwrap()
    .unwrap()
}
