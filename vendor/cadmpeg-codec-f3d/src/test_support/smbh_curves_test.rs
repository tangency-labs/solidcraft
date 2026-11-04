// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_asm::asm_header;

use crate::test_support::smbh_blocks_test::{
    generated_curve_block, generated_pcurve_block, generated_rational_pcurve_block,
    generated_surface_block,
};
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::smbh_surfaces_test::append_generated_float_array;
use crate::test_support::tokens_test::{
    push_native_enum, push_u8_string, t_dbl, t_end, t_ident, t_long, t_pos, t_ref, t_subident,
    t_u16_string, t_vec,
};

pub(crate) fn synthetic_geometry_with_procedural_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let edge = &records[10];
    let record = &mut bytes[edge.offset..edge.offset + edge.len];
    let curve_ref_tag = record.iter().rposition(|byte| *byte == 0x0c).unwrap();
    record[curve_ref_tag + 1..curve_ref_tag + 9].copy_from_slice(&19i64.to_le_bytes());

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "surf_surf_int_cur");
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0005);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

pub(crate) fn synthetic_geometry_with_helix_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        edge,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x0c,
    )
    .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "helix_int_cur");
    curve.push(0x0a);
    t_dbl(&mut curve, 0.0);
    curve.push(0x0a);
    t_dbl(&mut curve, std::f64::consts::TAU);
    t_pos(&mut curve, [1.0, 2.0, 3.0]);
    t_pos(&mut curve, [2.0, 0.0, 0.0]);
    t_pos(&mut curve, [0.0, 2.0, 0.0]);
    t_pos(&mut curve, [0.0, 0.0, 4.0]);
    t_dbl(&mut curve, 0.25);
    t_vec(&mut curve, [0.0, 0.0, 1.0]);
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0005);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

pub(crate) fn synthetic_geometry_with_cacheless_helix_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_helix_curve_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let helix = records.iter().find(|record| record.index == 19).unwrap();
    let block = generated_curve_block();
    let relative = bytes[helix.offset..helix.offset + helix.len]
        .windows(block.len())
        .position(|window| window == block)
        .unwrap();
    let cache = helix.offset + relative;
    bytes.drain(cache..cache + block.len() + 9);
    bytes
}

pub(crate) fn synthetic_geometry_with_law_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        edge,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x0c,
    )
    .unwrap();
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "law_int_cur");
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0005);
    for origin in [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0]] {
        t_ident(&mut curve, "plane");
        t_pos(&mut curve, origin);
        t_vec(&mut curve, [0.0, 0.0, 1.0]);
        t_vec(&mut curve, [1.0, 0.0, 0.0]);
        curve.push(0x0b);
    }
    curve.extend_from_slice(&generated_pcurve_block());
    curve.extend_from_slice(&generated_pcurve_block());
    t_dbl(&mut curve, -1.0);
    t_dbl(&mut curve, 2.0);
    for values in [&[0.25][..], &[][..], &[][..]] {
        append_generated_float_array(&mut curve, values);
    }
    t_long(&mut curve, 0);
    push_u8_string(&mut curve, "primary_law");
    t_long(&mut curve, 1);
    push_u8_string(&mut curve, "EDGE");
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, -0.5);
    t_dbl(&mut curve, 1.5);
    t_long(&mut curve, 2);
    push_u8_string(&mut curve, "null_law");
    push_u8_string(&mut curve, "null_law");
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

/// Append a vector-serialized `TRANS` law variable: the operator string, four
/// `0x14` vectors, a `0x06` scale, and three bare boolean flags.
fn append_transform_vec_variable(bytes: &mut Vec<u8>) {
    push_u8_string(bytes, "TRANS");
    for vector in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0],
    ] {
        t_vec(bytes, vector);
    }
    t_dbl(bytes, 0.1);
    bytes.push(0x0b); // false
    bytes.push(0x0b); // false
    bytes.push(0x0a); // true
}

/// Build the version-stamped `law_int_cur` subtype span (opening `0x0f` through
/// the `0x10` terminator): a `04 <20900> 15 <0>` version prefix, solved cache,
/// two `null_surface` and two `nullbs` carriers, bare-`0b` unbounded interval
/// bounds, three empty discontinuity arrays, and primary/additional formulas —
/// the primary carrying a vector-form `TRANS`, the additional list the fixed
/// four-slot `[null_law, null_law, raw-law, TRANS-wrapped]` shape.
pub(crate) fn stamped_law_curve_subtype(primary_name: &str, raw_name: &str) -> Vec<u8> {
    let mut c = Vec::new();
    c.push(0x0f);
    t_ident(&mut c, "law_int_cur");
    t_long(&mut c, 20900);
    push_native_enum(&mut c, 0);
    c.extend_from_slice(&generated_curve_block());
    t_dbl(&mut c, 0.0005);
    t_ident(&mut c, "null_surface");
    t_ident(&mut c, "null_surface");
    t_ident(&mut c, "nullbs");
    t_ident(&mut c, "nullbs");
    c.push(0x0b);
    c.push(0x0b);
    for _ in 0..3 {
        append_generated_float_array(&mut c, &[]);
    }
    t_long(&mut c, 0);
    t_u16_string(&mut c, primary_name);
    t_long(&mut c, 1);
    append_transform_vec_variable(&mut c);
    t_long(&mut c, 4);
    push_u8_string(&mut c, "null_law");
    push_u8_string(&mut c, "null_law");
    t_u16_string(&mut c, raw_name);
    t_long(&mut c, 0);
    push_u8_string(&mut c, "TRANS(VEC(X,X2,X3),TRANS1)");
    t_long(&mut c, 1);
    append_transform_vec_variable(&mut c);
    c.push(0x10);
    c
}

pub(crate) fn synthetic_geometry_with_stamped_law_curve_smbh(subtype: &[u8]) -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        edge,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x0c,
    )
    .unwrap();
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.extend_from_slice(subtype);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

pub(crate) fn synthetic_geometry_with_vector_offset_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        edge,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x0c,
    )
    .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "offset_int_cur");
    curve.push(0x0b);
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, -2.0);
    t_dbl(&mut curve, 5.0);
    t_vec(&mut curve, [0.5, -1.0, 2.0]);
    push_u8_string(&mut curve, "source");
    t_long(&mut curve, 7);
    push_u8_string(&mut curve, "offset");
    t_long(&mut curve, 9);
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0008);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

pub(crate) fn synthetic_geometry_with_subset_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        edge,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x0c,
    )
    .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "subset_int_cur");
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, -1.5);
    t_dbl(&mut curve, 3.5);
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0006);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

pub(crate) fn synthetic_geometry_with_exact_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        edge,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x0c,
    )
    .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "exact_int_cur");
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0004);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

pub(crate) fn synthetic_geometry_with_consistent_exact_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_exact_curve_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let point = &records[17];
    let position = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        point,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x13,
    )
    .unwrap()[0];
    let mut endpoint = Vec::new();
    t_pos(&mut endpoint, [2.0, 0.0, 0.0]);
    bytes.splice(position..position + endpoint.len(), endpoint);
    bytes
}

pub(crate) fn synthetic_geometry_with_decoy_curve_sense_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_consistent_exact_curve_smbh();
    let marker = b"\x0f\x0d\x0dexact_int_cur";
    let subtype = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("generated exact intcurve subtype");
    bytes.splice(subtype..subtype, [0x0a, 0x0b]);
    bytes
}

pub(crate) fn with_legacy_subtype(mut bytes: Vec<u8>, modern: &str, legacy: &str) -> Vec<u8> {
    let position = bytes
        .windows(modern.len())
        .position(|window| window == modern.as_bytes())
        .expect("generated modern subtype");
    bytes[position - 1] = u8::try_from(legacy.len()).expect("fixture value fits u8");
    bytes.splice(
        position..position + modern.len(),
        legacy.as_bytes().iter().copied(),
    );
    bytes
}

pub(crate) fn synthetic_geometry_with_compound_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        edge,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x0c,
    )
    .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "comp_int_cur");
    t_long(&mut curve, 3);
    for value in [0.0, 0.5, 1.0] {
        t_dbl(&mut curve, value);
    }
    t_long(&mut curve, 2);
    t_dbl(&mut curve, -2.0);
    t_dbl(&mut curve, 4.0);
    curve.push(0x0b);
    curve.extend_from_slice(&generated_curve_block());
    curve.extend_from_slice(&generated_curve_block());
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0003);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

pub(crate) fn synthetic_geometry_with_two_sided_offset_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        edge,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x0c,
    )
    .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "off_int_cur");
    for name in ["null_surface", "null_surface", "nullbs", "nullbs"] {
        t_ident(&mut curve, name);
    }
    t_dbl(&mut curve, -1.0);
    t_dbl(&mut curve, 2.0);
    t_long(&mut curve, 2);
    t_dbl(&mut curve, 0.25);
    t_dbl(&mut curve, 0.75);
    t_long(&mut curve, 0);
    t_long(&mut curve, 1);
    t_dbl(&mut curve, 0.5);
    curve.push(0x0a);
    t_dbl(&mut curve, -0.2);
    t_dbl(&mut curve, 0.4);
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0002);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

pub(crate) fn synthetic_geometry_with_embedded_offset_supports_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        edge,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x0c,
    )
    .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "off_int_cur");
    for _ in 0..2 {
        t_ident(&mut curve, "spline");
        curve.extend_from_slice(&generated_surface_block());
    }
    curve.extend_from_slice(&generated_pcurve_block());
    curve.extend_from_slice(&generated_rational_pcurve_block());
    t_dbl(&mut curve, 0.0);
    t_dbl(&mut curve, 1.0);
    for _ in 0..3 {
        t_long(&mut curve, 0);
    }
    curve.push(0x0b);
    t_dbl(&mut curve, -0.1);
    t_dbl(&mut curve, 0.3);
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0001);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

pub(crate) fn synthetic_geometry_with_analytic_offset_supports_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        edge,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x0c,
    )
    .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "off_int_cur");
    t_ident(&mut curve, "cone");
    t_pos(&mut curve, [1.0, 2.0, 3.0]);
    t_vec(&mut curve, [0.0, 0.0, 1.0]);
    t_vec(&mut curve, [1.0, 0.0, 0.0]);
    t_dbl(&mut curve, 0.4);
    curve.extend_from_slice(&[0x0b; 2]);
    t_dbl(&mut curve, -0.5);
    t_dbl(&mut curve, 3.0_f64.sqrt() / 2.0);
    t_dbl(&mut curve, 1.25);
    curve.extend_from_slice(&[0x0b; 5]);
    t_ident(&mut curve, "torus");
    t_pos(&mut curve, [-1.0, 0.5, 2.0]);
    t_vec(&mut curve, [0.0, 1.0, 0.0]);
    t_dbl(&mut curve, 2.5);
    t_dbl(&mut curve, -0.75);
    t_vec(&mut curve, [1.0, 0.0, 0.0]);
    curve.extend_from_slice(&[0x0b; 5]);
    curve.extend_from_slice(&generated_pcurve_block());
    curve.extend_from_slice(&generated_pcurve_block());
    t_dbl(&mut curve, 0.0);
    t_dbl(&mut curve, 1.0);
    for _ in 0..3 {
        t_long(&mut curve, 0);
    }
    curve.push(0x0b);
    t_dbl(&mut curve, -0.15);
    t_dbl(&mut curve, 0.25);
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0001);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

pub(crate) fn synthetic_geometry_with_surface_intersection_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_analytic_offset_supports_smbh();
    let subtype = bytes
        .windows(b"off_int_cur".len())
        .position(|window| window == b"off_int_cur")
        .expect("generated offset subtype");
    bytes[subtype..subtype + b"int_int_cur".len()].copy_from_slice(b"int_int_cur");
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    bytes[solved - 19] = 0x0a;
    bytes.drain(solved - 18..solved);
    bytes
}

pub(crate) fn synthetic_geometry_with_projection_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_analytic_offset_supports_smbh();
    let subtype = bytes
        .windows(b"off_int_cur".len())
        .position(|window| window == b"off_int_cur")
        .expect("generated offset subtype");
    bytes[subtype - 1] = u8::try_from(b"proj_int_cur".len()).expect("fixture value fits u8");
    bytes.splice(
        subtype..subtype + b"off_int_cur".len(),
        b"proj_int_cur".iter().copied(),
    );
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    bytes[solved - 19] = 0x0a;
    let mut tail = generated_curve_block();
    tail.push(0x0a);
    t_dbl(&mut tail, -2.0);
    t_dbl(&mut tail, 3.0);
    push_u8_string(&mut tail, "surf2");
    bytes.splice(solved - 18..solved, tail);
    bytes
}

pub(crate) fn synthetic_geometry_with_early_close_projection_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_projection_smbh();
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    let source = bytes[..solved]
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated projection source curve");
    let source_end = source + generated_curve_block().len();
    bytes.splice(source_end..solved, [0x0a, 0x10]);
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("shifted solved curve cache");
    let fit_end = solved + generated_curve_block().len() + 9;
    assert_eq!(bytes[fit_end], 0x10);
    bytes.remove(fit_end);
    bytes
}

pub(crate) fn synthetic_geometry_with_three_surface_intersection_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_analytic_offset_supports_smbh();
    let subtype = bytes
        .windows(b"off_int_cur".len())
        .position(|window| window == b"off_int_cur")
        .expect("generated offset subtype");
    bytes[subtype..subtype + b"sss_int_cur".len()].copy_from_slice(b"sss_int_cur");
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    let mut third = Vec::new();
    t_long(&mut third, 7);
    t_ident(&mut third, "sphere");
    t_pos(&mut third, [0.5, 1.0, -2.0]);
    t_dbl(&mut third, -1.25);
    t_vec(&mut third, [1.0, 0.0, 0.0]);
    t_vec(&mut third, [0.0, 0.0, 1.0]);
    third.extend_from_slice(&[0x0b; 5]);
    third.extend_from_slice(&generated_rational_pcurve_block());
    bytes.splice(solved - 19..solved, third);
    bytes
}

pub(crate) fn synthetic_geometry_with_surface_curve_smbh(name: &str) -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_surface_intersection_smbh();
    let subtype = bytes
        .windows(b"int_int_cur".len())
        .position(|window| window == b"int_int_cur")
        .expect("generated intersection subtype");
    bytes[subtype - 1] = u8::try_from(name.len()).expect("fixture value fits u8");
    bytes.splice(
        subtype..subtype + b"int_int_cur".len(),
        name.as_bytes().iter().copied(),
    );
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    bytes.remove(solved - 1);
    bytes
}

pub(crate) fn synthetic_geometry_with_silhouette_smbh(
    name: &str,
    draft_factor: Option<f64>,
) -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_surface_intersection_smbh();
    let subtype = bytes
        .windows(b"int_int_cur".len())
        .position(|window| window == b"int_int_cur")
        .expect("generated intersection subtype");
    bytes[subtype - 1] = u8::try_from(name.len()).expect("fixture value fits u8");
    bytes.splice(
        subtype..subtype + b"int_int_cur".len(),
        name.as_bytes().iter().copied(),
    );
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    let mut tail = Vec::new();
    t_ident(&mut tail, "sphere");
    t_pos(&mut tail, [0.0, 0.0, 0.0]);
    t_dbl(&mut tail, 1.5);
    t_vec(&mut tail, [1.0, 0.0, 0.0]);
    t_vec(&mut tail, [0.0, 0.0, 1.0]);
    tail.extend_from_slice(&[0x0b; 5]);
    t_vec(&mut tail, [0.0, -2.0, 0.0]);
    if let Some(draft_factor) = draft_factor {
        t_dbl(&mut tail, draft_factor);
    }
    bytes.splice(solved - 1..solved, tail);
    bytes
}

pub(crate) fn synthetic_geometry_with_surface_offset_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_analytic_offset_supports_smbh();
    let subtype = bytes
        .windows(b"off_int_cur".len())
        .position(|window| window == b"off_int_cur")
        .expect("generated offset subtype");
    bytes[subtype - 1] = u8::try_from(b"off_surf_int_cur".len()).expect("fixture value fits u8");
    bytes.splice(
        subtype..subtype + b"off_int_cur".len(),
        b"off_surf_int_cur".iter().copied(),
    );
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    let mut tail = vec![0x0a];
    for value in [-1.0, 2.0, -3.0, 4.0] {
        t_dbl(&mut tail, value);
    }
    tail.extend_from_slice(&generated_curve_block());
    t_dbl(&mut tail, -0.5);
    t_dbl(&mut tail, 1.5);
    t_dbl(&mut tail, -0.25);
    t_dbl(&mut tail, 0.75);
    t_dbl(&mut tail, 1.25);
    bytes.splice(solved - 19..solved, tail);
    bytes
}

pub(crate) fn synthetic_geometry_with_spring_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_surface_intersection_smbh();
    let subtype = bytes
        .windows(b"int_int_cur".len())
        .position(|window| window == b"int_int_cur")
        .expect("generated intersection subtype");
    bytes[subtype - 1] = u8::try_from(b"spring_int_cur".len()).expect("fixture value fits u8");
    bytes.splice(
        subtype..subtype + b"int_int_cur".len(),
        b"spring_int_cur".iter().copied(),
    );
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    let mut direction = Vec::new();
    direction.push(0x15);
    direction.extend_from_slice(&(-3i64).to_le_bytes());
    bytes.splice(solved..solved, direction);
    bytes
}

pub(crate) fn synthetic_geometry_with_null_support_spring_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        edge,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x0c,
    )
    .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "spring_int_cur");
    t_ident(&mut curve, "null_surface");
    for value in [-2.0, 3.0, -4.0, 5.0] {
        t_dbl(&mut curve, value);
    }
    t_ident(&mut curve, "null_surface");
    for value in [-6.0, 7.0, -8.0, 9.0] {
        t_dbl(&mut curve, value);
    }
    t_ident(&mut curve, "nullbs");
    t_dbl(&mut curve, -10.0);
    t_dbl(&mut curve, 11.0);
    t_ident(&mut curve, "nullbs");
    t_dbl(&mut curve, -1.0);
    t_dbl(&mut curve, 2.0);
    t_long(&mut curve, 1);
    t_dbl(&mut curve, 0.25);
    t_long(&mut curve, 0);
    t_long(&mut curve, 2);
    t_dbl(&mut curve, 0.5);
    t_dbl(&mut curve, 0.75);
    curve.push(0x0a);
    curve.push(0x15);
    curve.extend_from_slice(&4i64.to_le_bytes());
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0004);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

/// The cache form `0` head of the shared cache-first intcurve context: the
/// enum, the solved curve cache, and its fit tolerance.
pub(crate) fn push_solved_cache_first_head(curve: &mut Vec<u8>) {
    curve.push(0x15);
    curve.extend_from_slice(&0i64.to_le_bytes());
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(curve, 0.0004);
}

/// Splice one cache-first intcurve record built by `head` and `tail` into the
/// synthetic geometry stream and point edge 10 at it.
pub(crate) fn synthetic_geometry_with_cache_first_curve_smbh(
    subtype: &str,
    head: fn(&mut Vec<u8>),
    tail: impl FnOnce(&mut Vec<u8>),
) -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let edge = &records[10];
    let offsets = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        edge,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x0c,
    )
    .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, subtype);
    t_long(&mut curve, 23100);
    head(&mut curve);
    t_ident(&mut curve, "null_surface");
    t_ident(&mut curve, "null_surface");
    t_ident(&mut curve, "nullbs");
    t_ident(&mut curve, "nullbs");
    curve.push(0x0a);
    t_dbl(&mut curve, -1.0);
    curve.push(0x0a);
    t_dbl(&mut curve, 2.0);
    t_long(&mut curve, 0);
    t_long(&mut curve, 0);
    t_long(&mut curve, 0);
    t_long(&mut curve, 7);
    tail(&mut curve);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

pub(crate) fn synthetic_geometry_with_deformable_curve_smbh(mode: i64) -> Vec<u8> {
    synthetic_geometry_with_cache_first_curve_smbh(
        "defm_int_cur",
        push_solved_cache_first_head,
        |curve| {
            curve.extend_from_slice(&generated_curve_block());
            curve.push(0x0a);
            t_dbl(curve, 0.0);
            curve.push(0x0a);
            t_dbl(curve, 1.0);
            t_long(curve, mode);
            match mode {
                8 => {
                    for vector in [
                        [1.0, 2.0, 3.0],
                        [4.0, 5.0, 6.0],
                        [7.0, 8.0, 9.0],
                        [10.0, 11.0, 12.0],
                    ] {
                        t_vec(curve, vector);
                    }
                    t_long(curve, 2);
                    for value in [-1.0, 0.25, 2.0, 3.5] {
                        t_dbl(curve, value);
                    }
                }
                3 => {
                    for vector in [
                        [1.0, 2.0, 3.0],
                        [4.0, 5.0, 6.0],
                        [7.0, 8.0, 9.0],
                        [10.0, 11.0, 12.0],
                    ] {
                        t_vec(curve, vector);
                    }
                    t_dbl(curve, 0.5);
                    curve.extend_from_slice(&[0x0a, 0x0b, 0x0a]);
                    t_pos(curve, [13.0, 14.0, 15.0]);
                    for vector in [[16.0, 17.0, 18.0], [19.0, 20.0, 21.0]] {
                        t_vec(curve, vector);
                    }
                    t_dbl(curve, 1.5);
                    curve.extend_from_slice(&[0x0b, 0x0a]);
                    for value in [2.5, 3.5, 4.5] {
                        t_dbl(curve, value);
                    }
                    curve.extend_from_slice(&[0x0a, 0x0b, 0x0a, 0x0b, 0x0a]);
                    t_dbl(curve, 5.5);
                    t_long(curve, 6);
                }
                // The tail is mode-specific. A mode outside this set states no
                // tail, and the decode assertion in the calling test then fails.
                _ => {}
            }
        },
    )
}
