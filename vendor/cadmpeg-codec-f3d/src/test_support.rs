// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic byte-fixture builders for the crate's `#[cfg(test)]` suites.
//!
//! Helpers hand-build `.f3d` ZIP archives and ASM stream payloads.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

pub(crate) mod align;

use std::io::Write;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{report::export::WritePath, CadIr, SourceFidelity};

use crate::F3dCodec;

pub(crate) mod act_links;
pub(crate) mod archive_session;
pub(crate) mod configuration_table;
pub(crate) mod procedural_support;
pub(crate) mod sketch_relations;
pub(crate) mod sketch_text;
pub(crate) mod verification;

pub(crate) fn with_decode_context<T>(
    f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    with_decode_policy(&cadmpeg_core::decode::DecodePolicy::service(), f)
}

pub(crate) fn with_decode_policy<T>(
    policy: &cadmpeg_core::decode::DecodePolicy,
    f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, policy)
        .expect("test decode context");
    f(&ctx)
}

/// Plans an inherited write through the sealed encoder and writes its bytes.
pub(crate) fn plan_inherited_write(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    writer: &mut dyn Write,
) -> Result<WritePath, CodecError> {
    let plan = F3dCodec.plan(EncodeInput::new(ir, Some(fidelity)), TargetRequest::Inherit)?;
    Ok(plan.write_to(writer)?.write_path().clone())
}

/// A finite test value as the checked scalar a record holds.
pub(crate) fn real(value: f64) -> cadmpeg_ir::scalar::FiniteReal {
    cadmpeg_ir::scalar::FiniteReal::new(value).expect("a finite test value")
}

/// Finite test values as the checked lane a record holds.
pub(crate) fn reals<const N: usize>(values: [f64; N]) -> [cadmpeg_ir::scalar::FiniteReal; N] {
    values.map(real)
}

/// Write a length-prefixed ASCII string: a little-endian `u32` byte count then
/// the bytes.
pub(crate) fn lp_ascii(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(
        &u32::try_from(value.len())
            .expect("test ASCII string length")
            .to_le_bytes(),
    );
    out.extend_from_slice(value.as_bytes());
}

/// Write a length-prefixed UTF-16 string: a little-endian `u32` code-unit count
/// then the little-endian units.
pub(crate) fn lp_utf16(out: &mut Vec<u8>, value: &str) {
    let units = value.encode_utf16().collect::<Vec<_>>();
    out.extend_from_slice(
        &u32::try_from(units.len())
            .expect("test UTF-16 unit count")
            .to_le_bytes(),
    );
    for unit in units {
        out.extend_from_slice(&unit.to_le_bytes());
    }
}

/// Write a same-document reference to a `u64` target: the presence byte, the
/// target, then the two zero bytes that close the reference.
pub(crate) fn push_reference_u64(out: &mut Vec<u8>, target: u64) {
    out.push(1);
    out.extend_from_slice(&target.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
}

/// A same-document reference record to `target`.
pub(crate) fn local_reference(target: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_reference_u64(&mut bytes, target);
    bytes
}

pub(crate) fn legacy_attributed_external_placement(
    role: &str,
    attributes: bool,
    matrix: Option<[[f64; 4]; 4]>,
) -> Vec<u8> {
    let target = || {
        let mut reference = vec![1];
        reference.extend_from_slice(&3_u64.to_le_bytes());
        reference.push(1);
        reference.extend_from_slice(&0x0102_0304_0506_0708_u64.to_le_bytes());
        reference.push(0);
        lp_ascii(&mut reference, "E03784ED-5E19-4E14-B9F2-3B07017018CD");
        reference.push(0);
        reference
    };
    let mut bytes = Vec::new();
    lp_ascii(&mut bytes, "380");
    bytes.extend_from_slice(&10_u64.to_le_bytes());
    lp_ascii(&mut bytes, "");
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(target());
    bytes.extend_from_slice(&9_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&(if attributes { 3_u32 } else { 0_u32 }).to_le_bytes());
    if attributes {
        lp_ascii(&mut bytes, "GatedByParent");
        lp_ascii(&mut bytes, "IntrinsicMetaTypebool");
        bytes.push(0);
        lp_ascii(&mut bytes, "ResolutionTime");
        lp_ascii(&mut bytes, "IntrinsicMetaTypeHString");
        lp_utf16(&mut bytes, "");
        lp_ascii(&mut bytes, "SelfGated");
        lp_ascii(&mut bytes, "IntrinsicMetaTypebool");
        bytes.push(0);
    }
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    lp_utf16(&mut bytes, "11111111-2222-3333-4444-555555555555");
    lp_utf16(&mut bytes, "66666666-7777-8888-9999-aaaaaaaaaaaa");
    bytes.push(1);
    bytes.extend_from_slice(&71_u64.to_le_bytes());
    bytes.extend_from_slice(&[0, 1]);
    bytes.extend_from_slice(&5_u32.to_le_bytes());
    bytes.push(0);
    bytes.extend(target());
    bytes.push(u8::from(matrix.is_none()));
    if let Some(matrix) = matrix {
        for value in matrix.into_iter().flatten() {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    lp_utf16(&mut bytes, role);
    bytes.push(0);
    bytes.extend(local_reference(7));
    bytes
}

/// A cross-document reference record: the presence byte, the target, the
/// cross-document marker, the source type GUID, the link type GUID, and the
/// link name.
pub(crate) fn cross_document_reference(target: u64, link_name: &str) -> Vec<u8> {
    typed_cross_document_reference(target, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", link_name)
}

/// A cross-document reference whose target has type `type_guid`.
pub(crate) fn typed_cross_document_reference(
    target: u64,
    type_guid: &str,
    link_name: &str,
) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&target.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    lp_utf16(&mut bytes, "11111111-2222-3333-4444-555555555555");
    bytes.push(0);
    lp_ascii(&mut bytes, type_guid);
    lp_utf16(&mut bytes, link_name);
    bytes.push(0);
    bytes
}

/// Append an indexed record header: the three-byte class tag behind its
/// little-endian `u32` length, then the little-endian record index.
pub(crate) fn indexed_header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&class_tag);
    bytes.extend_from_slice(&record_index.to_le_bytes());
}

/// Write an indexed record header over the first eleven bytes of `bytes`.
fn write_indexed_header(bytes: &mut [u8], class_tag: [u8; 3], record_index: u32) {
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(&class_tag);
    bytes[7..11].copy_from_slice(&record_index.to_le_bytes());
}

/// Write a present marked reference to `record_index` at `at`: the presence
/// byte then the little-endian record index.
pub(crate) fn write_marked_reference(bytes: &mut [u8], at: usize, record_index: u32) {
    bytes[at] = 1;
    bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
}

/// Append a present marked reference to `record_index`: the presence byte, the
/// little-endian record index, then the six zero bytes that close the mark.
pub(crate) fn push_marked_reference(bytes: &mut Vec<u8>, record_index: u32) {
    bytes.push(1);
    bytes.extend_from_slice(&record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
}

pub(crate) mod tokens_test;

pub(crate) mod smbh_header_test;

pub(crate) mod smbh_blocks_test;

pub(crate) mod sketch_region_test;
pub(crate) mod smbh_geometry_test;

pub(crate) mod smbh_pcurves_test;

pub(crate) mod smbh_curves_test;

pub(crate) mod smbh_surfaces_test;

pub(crate) mod smbh_revision_test;

pub(crate) mod smbh_blends_test;

pub(crate) mod smbh_bf4_test;

pub(crate) mod native_test;

pub(crate) mod manifest_test;

pub(crate) mod protein_test;

pub(crate) mod empty_attribute_occurrences;
pub(crate) mod streams_test;

pub(crate) mod zip_test;

pub(crate) mod assembly_test;

pub(crate) mod procedural_test;

pub(crate) mod history;

pub(crate) mod history_stream;

pub(crate) mod body_map_test;

pub(crate) mod component_insert_test;

pub(crate) mod lost_edge_test;

pub(crate) mod step_conversion;

pub(crate) mod body_reference_tail_test;

pub(crate) mod placement_construction;

pub(crate) mod pipe;

pub(crate) mod work_plane;

pub(crate) mod component_body_test;

pub(crate) mod scan_counts;

pub(crate) mod multiple_profiles;
