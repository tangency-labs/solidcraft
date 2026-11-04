// SPDX-License-Identifier: Apache-2.0
//! Synthetic Protein instance-property payloads.
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::index_from_u32;

use crate::test_support::lp_ascii;
use cadmpeg_protein::{
    CONTINUATION_MARKER, PAGE_SIZE, RECORD_MARKER, STREAM_HEADER_LEN, TERMINAL_MARKER,
};

pub(crate) fn generated_instance_properties_for(guid: &str) -> Vec<u8> {
    let mut logical = RECORD_MARKER.to_vec();
    lp_ascii(&mut logical, "GenericSchema");
    lp_ascii(&mut logical, guid);
    lp_ascii(&mut logical, "Prism-001");
    lp_ascii(&mut logical, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    let value_block = logical.len();
    logical.resize(value_block + 209, 0);
    for (ordinal, value) in [0.1f64, 0.2, 0.3, 1.0].into_iter().enumerate() {
        let offset = value_block + 112 + ordinal * 8;
        logical[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    logical[value_block + 171..value_block + 175].copy_from_slice(b"\x0c\x00\x00\x00");
    logical[value_block + 175..value_block + 183].copy_from_slice(&0.25f64.to_le_bytes());
    logical[value_block + 197..value_block + 201].copy_from_slice(b"\x0c\x00\x00\x00");
    logical[value_block + 201..value_block + 209].copy_from_slice(&1.5f64.to_le_bytes());

    paged_instance_properties(&logical)
}

pub(crate) fn generated_prism_instance_properties(schema: &str, guid: &str) -> Vec<u8> {
    let mut logical = RECORD_MARKER.to_vec();
    lp_ascii(&mut logical, schema);
    lp_ascii(&mut logical, guid);
    lp_ascii(&mut logical, "Prism-001");
    lp_ascii(&mut logical, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    let position = logical.len();
    match schema {
        "PrismOpaqueSchema" => {
            logical.resize(position + 96, 0);
            for (ordinal, value) in [0.1f64, 0.2, 0.3, 1.0].into_iter().enumerate() {
                let offset = position + 8 + ordinal * 8;
                logical[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            }
            logical[position + 64..position + 68].copy_from_slice(b"\x0e\x20\x00\x00");
            logical[position + 68..position + 76].copy_from_slice(&0.25f64.to_le_bytes());
        }
        "PrismTransparentSchema" => {
            logical.resize(position + 177, 0);
            for (ordinal, value) in [0.1f64, 0.2, 0.3, 1.0].into_iter().enumerate() {
                let offset = position + 121 + ordinal * 8;
                logical[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            }
            logical[position + 169..position + 177].copy_from_slice(&1.5f64.to_le_bytes());
        }
        _ => panic!("unsupported generated Prism schema"),
    }
    paged_instance_properties(&logical)
}

fn paged_instance_properties(logical: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(
        &(u32::try_from(PAGE_SIZE).expect("fixture value fits u32")).to_le_bytes(),
    );
    bytes.extend_from_slice(&[0xff; 8]);
    bytes.extend_from_slice(&0u32.to_le_bytes());

    let first = logical.len().min(PAGE_SIZE - 4);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&logical[..first]);
    bytes.resize(STREAM_HEADER_LEN + PAGE_SIZE, 0);
    let mut rest = &logical[first..];
    while rest.len() > PAGE_SIZE - 8 {
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(CONTINUATION_MARKER);
        bytes.extend_from_slice(&rest[..PAGE_SIZE - 8]);
        rest = &rest[PAGE_SIZE - 8..];
    }
    if !rest.is_empty() {
        bytes.extend_from_slice(TERMINAL_MARKER);
        bytes.extend_from_slice(
            &(u16::try_from(rest.len()).expect("fixture value fits u16")).to_le_bytes(),
        );
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(rest);
        let page_end =
            STREAM_HEADER_LEN + (bytes.len() - STREAM_HEADER_LEN).next_multiple_of(PAGE_SIZE);
        bytes.resize(page_end, 0);
    }
    bytes
}

pub(super) fn generated_schema_from_paged(properties: &[u8]) -> &str {
    let length = index_from_u32(u32::from_le_bytes(properties[24..28].try_into().unwrap()));
    std::str::from_utf8(&properties[28..28 + length]).unwrap()
}

pub(super) fn generated_definition_catalog_for(schema: &str) -> Vec<u8> {
    let mut out = RECORD_MARKER.to_vec();
    lp_ascii(&mut out, schema);
    out.push(0);
    lp_ascii(&mut out, "Prism-001");
    lp_ascii(&mut out, "Prism-001");
    out.extend_from_slice(&2_u32.to_le_bytes());
    for value in ["Plastic/Thermoplastic", "Default", "Generated appearance"] {
        lp_ascii(&mut out, value);
    }
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&1_u32.to_le_bytes());
    lp_ascii(&mut out, "");
    paged_instance_properties(&out)
}
