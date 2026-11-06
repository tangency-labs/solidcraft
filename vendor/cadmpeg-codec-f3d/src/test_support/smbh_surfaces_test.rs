// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_asm::asm_header;

use crate::test_support::smbh_blocks_test::{
    generated_curve_block, generated_pcurve_block, generated_rational_surface_block,
    generated_surface_block,
};
use crate::test_support::smbh_geometry_test::synthetic_mixed_smbh;
use crate::test_support::tokens_test::{
    push_native_enum, push_tagged_i64, push_u8_string, t_dbl, t_end, t_ident, t_long, t_pos, t_ref,
    t_subident, t_u16_string, t_vec,
};

pub(crate) fn synthetic_cyl_spl_sur_smbh() -> Vec<u8> {
    synthetic_cyl_spl_sur_with_cache_smbh(true)
}

/// Append the head of the shared revision-gated surface tail. Form `0` stores
/// the solved cache followed by its fit tolerance; form `2` stores the U
/// parameter interval and the V parameter interval in the optional bool-gated
/// encoding, then the U closure, V closure, U singularity, and V singularity
/// enums. Every slot carries a distinct value so a reordering fails loudly.
pub(super) fn append_revision_surface_tail_head(
    bytes: &mut Vec<u8>,
    form: i64,
    fit_tolerance: f64,
) {
    push_tagged_i64(bytes, 0x15, form);
    if form == 0 {
        bytes.extend_from_slice(&generated_surface_block());
        t_dbl(bytes, fit_tolerance);
        return;
    }
    for value in [0.25, 0.75, -1.5, 3.5] {
        bytes.push(0x0a);
        t_dbl(bytes, value);
    }
    for value in [1, 2, 3, 4] {
        push_tagged_i64(bytes, 0x15, value);
    }
}

/// Append the six counted discontinuity arrays and the boolean closing the
/// shared revision-gated surface tail.
pub(super) fn append_revision_surface_tail_discontinuities(bytes: &mut Vec<u8>) {
    for values in [
        &[0.25][..],
        &[][..],
        &[0.5, 0.75][..],
        &[1.5][..],
        &[][..],
        &[2.5, 3.5][..],
    ] {
        t_long(bytes, i64::try_from(values.len()).unwrap());
        for value in values {
            t_dbl(bytes, *value);
        }
    }
    bytes.push(0x0b);
}

/// The discontinuity arrays `append_revision_surface_tail_discontinuities`
/// writes.
pub(crate) fn expected_revision_surface_tail_discontinuities() -> [Vec<f64>; 6] {
    [
        vec![0.25],
        vec![],
        vec![0.5, 0.75],
        vec![1.5],
        vec![],
        vec![2.5, 3.5],
    ]
}

/// The parameterization `append_revision_surface_tail_head` writes for form `2`.
pub(crate) fn expected_revision_surface_tail_parameterization(
) -> cadmpeg_ir::geometry::RevisionSurfaceParameterization {
    cadmpeg_ir::geometry::RevisionSurfaceParameterization {
        u_interval: [Some(0.25), Some(0.75)],
        v_interval: [Some(-1.5), Some(3.5)],
        u_closure: 1,
        v_closure: 2,
        u_singularity: 3,
        v_singularity: 4,
    }
}

pub(crate) fn synthetic_versioned_cyl_spl_sur_smbh() -> Vec<u8> {
    synthetic_versioned_cyl_spl_sur_with_tail_smbh(0)
}

pub(crate) fn synthetic_versioned_cyl_spl_sur_with_trailing_token_smbh() -> Vec<u8> {
    let mut bytes = synthetic_versioned_cyl_spl_sur_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let record = &records[9];
    let subtype_close = record.offset + record.len - 2;
    assert_eq!(bytes[subtype_close], 0x10);
    let mut trailing = Vec::new();
    t_dbl(&mut trailing, 17.0);
    bytes.splice(subtype_close..subtype_close, trailing);
    bytes
}

/// A revision-gated `cyl_spl_sur` closing with the shared surface tail. Its
/// directrix scope carries a surface block and a trailing scalar of its own, so
/// a decoder that locates the face cache by scanning the scope rather than by
/// parsing the tail picks that block up and reads its trailing scalar as the
/// fit tolerance.
pub(crate) fn synthetic_versioned_cyl_spl_sur_with_tail_smbh(tail_form: i64) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old_offset = records[9].offset;
    let old_len = records[9].len;

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "cyl_spl_sur");
    t_long(&mut surface, 23100);
    t_ident(&mut surface, "intcurve");
    surface.push(0x0a);
    surface.push(0x0f);
    t_ident(&mut surface, "exact_int_cur");
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.009);
    surface.push(0x10);
    surface.push(0x0a);
    t_dbl(&mut surface, 0.25);
    surface.push(0x0a);
    t_dbl(&mut surface, 0.75);
    t_vec(&mut surface, [0.0, 0.0, 2.0]);
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    append_revision_surface_tail_head(&mut surface, tail_form, 0.002);
    append_revision_surface_tail_discontinuities(&mut surface);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old_offset..old_offset + old_len, surface);
    bytes
}

pub(crate) fn synthetic_cacheless_cyl_spl_sur_smbh() -> Vec<u8> {
    synthetic_cyl_spl_sur_with_cache_smbh(false)
}

fn synthetic_cyl_spl_sur_with_cache_smbh(include_cache: bool) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old_offset = records[9].offset;
    let old_len = records[9].len;

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "cyl_spl_sur");
    t_dbl(&mut surface, 0.25);
    t_dbl(&mut surface, 0.75);
    t_vec(&mut surface, [0.0, 0.0, 2.0]);
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    surface.extend_from_slice(&generated_curve_block());
    if include_cache {
        surface.extend_from_slice(&generated_surface_block());
        t_dbl(&mut surface, 0.002);
    }
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old_offset..old_offset + old_len, surface);
    bytes
}

pub(crate) fn synthetic_exact_spl_sur_smbh(name: &str) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.0015);
    for value in [-2.0, 3.0, -4.0, 5.0] {
        t_dbl(&mut surface, value);
    }
    t_long(&mut surface, 7);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_exact_spl_sur_with_decoy_sense_smbh() -> Vec<u8> {
    let mut bytes = synthetic_exact_spl_sur_smbh("exact_spl_sur");
    let marker = b"\x0f\x0d\x0dexact_spl_sur";
    let subtype = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("generated exact spline-surface subtype");
    bytes.splice(subtype..subtype, [0x0a, 0x0b]);
    bytes
}

pub(crate) fn synthetic_ruled_spl_sur_smbh(name: &str, include_cache: bool) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&generated_curve_block());
    if include_cache {
        surface.extend_from_slice(&generated_surface_block());
        t_dbl(&mut surface, 0.0025);
    }
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_sum_spl_sur_smbh(name: &str, include_cache: bool) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&generated_curve_block());
    t_pos(&mut surface, [1.0, -2.0, 3.0]);
    if include_cache {
        surface.extend_from_slice(&generated_surface_block());
        t_dbl(&mut surface, 0.0035);
    }
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_rot_spl_sur_smbh(name: &str) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    surface.extend_from_slice(&generated_curve_block());
    t_pos(&mut surface, [1.0, -2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.0045);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_off_spl_sur_smbh(name: &str) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [1.0, -2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    t_dbl(&mut surface, -1.25);
    surface.push(0x15);
    surface.extend_from_slice(&3i64.to_le_bytes());
    surface.push(0x15);
    surface.extend_from_slice(&(-4i64).to_le_bytes());
    if name == "off_spl_sur" {
        surface.extend_from_slice(&[0x0a, 0x0b, 0x0a]);
    }
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.0055);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_comp_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "comp_spl_sur");
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.0065);
    t_long(&mut surface, 2);
    t_dbl(&mut surface, -0.5);
    t_dbl(&mut surface, 1.5);
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [1.0, -2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    t_ident(&mut surface, "spline");
    surface.extend_from_slice(&generated_rational_surface_block());
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_taper_spl_sur_smbh(name: &str) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [1.0, -2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&generated_pcurve_block());
    t_dbl(&mut surface, 0.35);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.0075);
    match name {
        "ortho_spl_sur" | "orthosur" => surface.push(0x0a),
        "edge_tpr_spl_sur" => t_vec(&mut surface, [1.0, 2.0, 3.0]),
        "shadow_tpr_spl_sur" | "shadowtapersur" | "swept_tpr_spl_sur" | "swepttapersur" => {
            t_vec(&mut surface, [1.0, 2.0, 3.0]);
            t_dbl(&mut surface, 0.6);
            t_dbl(&mut surface, 0.8);
        }
        "ruled_tpr_spl_sur" | "ruledtapersur" => {
            t_vec(&mut surface, [1.0, 2.0, 3.0]);
            t_dbl(&mut surface, 0.6);
            t_dbl(&mut surface, 0.8);
            t_dbl(&mut surface, 1.25);
        }
        // "taper_spl_sur", and any taper name outside this set, states no
        // extra field. The decode assertion in the calling test then fails.
        _ => {}
    }
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn append_generated_loft_section(bytes: &mut Vec<u8>, parameter: f64, direction: bool) {
    t_long(bytes, 1);
    t_dbl(bytes, parameter);
    t_long(bytes, 1);
    t_long(bytes, 9);
    bytes.extend_from_slice(&generated_curve_block());
    t_ident(bytes, "plane");
    t_pos(bytes, [1.0, -2.0, 3.0]);
    t_vec(bytes, [0.0, 0.0, 1.0]);
    t_vec(bytes, [1.0, 0.0, 0.0]);
    bytes.push(0x0b);
    bytes.extend_from_slice(&generated_pcurve_block());
    bytes.push(0x0b);
    t_long(bytes, -1);
    t_long(bytes, 211);
    t_long(bytes, 4);
    t_long(bytes, 0);
    t_dbl(bytes, -0.25);
    t_dbl(bytes, 0.75);
    bytes.push(if direction { 0x0a } else { 0x0b });
    if direction {
        t_vec(bytes, [0.0, 1.0, 0.0]);
    }
    bytes.extend_from_slice(&generated_curve_block());
    t_long(bytes, 1);
    bytes.extend_from_slice(&generated_curve_block());
    t_long(bytes, 6);
}

pub(crate) fn synthetic_loft_spl_sur_smbh(name: &str) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    append_generated_loft_section(&mut surface, 0.0, true);
    append_generated_loft_section(&mut surface, 1.0, false);
    for value in [-1.0, 2.0, -3.0, 4.0] {
        t_dbl(&mut surface, value);
    }
    for value in [1i64, 2, 3, 4] {
        surface.push(0x15);
        surface.extend_from_slice(&value.to_le_bytes());
    }
    t_long(&mut surface, 2);
    surface.push(0x0a);
    t_long(&mut surface, 17);
    t_dbl(&mut surface, 0.125);
    push_u8_string(&mut surface, "bridge");
    surface.push(0x15);
    surface.extend_from_slice(&(-7i64).to_le_bytes());
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.0085);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_net_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "net_spl_sur");
    append_generated_loft_section(&mut surface, 0.0, true);
    append_generated_loft_section(&mut surface, 1.0, false);
    for value in 0..12 {
        t_dbl(&mut surface, f64::from(value) / 10.0);
    }
    t_long(&mut surface, 17);
    for direction in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
    ] {
        t_vec(&mut surface, direction);
    }
    for _ in 0..4 {
        push_u8_string(&mut surface, "null_law");
    }
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.005);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_profile_first_sweep_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "sweep_spl_sur");
    surface.push(0x15);
    surface.extend_from_slice(&3i64.to_le_bytes());
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&generated_curve_block());
    surface.push(0x15);
    surface.extend_from_slice(&4i64.to_le_bytes());
    for direction in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
        [0.0, -1.0, 0.0],
    ] {
        t_vec(&mut surface, direction);
    }
    t_pos(&mut surface, [1.0, 2.0, 3.0]);
    for value in [0.1, 0.2, 0.3, 0.4] {
        t_dbl(&mut surface, value);
    }
    for _ in 0..3 {
        push_u8_string(&mut surface, "null_law");
    }
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.005);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_t_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "t_spl_sur");
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    for value in [-2.0, 3.0, -4.0, 5.0] {
        t_dbl(&mut surface, value);
    }
    t_long(&mut surface, 7);
    surface.push(0x0f);
    t_ident(&mut surface, "t_spl_subtrans_object");
    t_u16_string(
        &mut surface,
        "degree 3\nunits mm\nv 1 0 0 0\nv 2 1 0 0\ne 1 1 2\n",
    );
    surface.push(0x0b);
    t_u16_string(&mut surface, "100verts 1 2\n");
    surface.push(0x10);
    t_long(&mut surface, 9);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_helix_surface_smbh(circular: bool) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(
        &mut surface,
        if circular {
            "helix_spl_circ"
        } else {
            "helix_spl_line"
        },
    );
    t_dbl(&mut surface, -0.5);
    t_dbl(&mut surface, 0.5);
    t_dbl(&mut surface, -2.0);
    t_dbl(&mut surface, 3.0);
    if circular {
        t_dbl(&mut surface, 1.25);
    }
    t_dbl(&mut surface, 0.0);
    t_dbl(&mut surface, std::f64::consts::TAU);
    t_pos(&mut surface, [1.0, 2.0, 3.0]);
    t_pos(&mut surface, [2.0, 0.0, 0.0]);
    t_pos(&mut surface, [0.0, 2.0, 0.0]);
    t_pos(&mut surface, [0.0, 0.0, 4.0]);
    t_dbl(&mut surface, 0.25);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    for sentinel in ["null_surface", "null_surface", "nullbs", "nullbs"] {
        t_ident(&mut surface, sentinel);
    }
    if circular {
        t_dbl(&mut surface, 0.75);
    } else {
        t_pos(&mut surface, [5.0, 6.0, 7.0]);
    }
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_minimal_deformable_surface_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "defm_spl_sur");
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [1.0, 2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    t_long(&mut surface, 8);
    for vector in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
    ] {
        t_vec(&mut surface, vector);
    }
    t_long(&mut surface, 0);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_framed_deformable_surface_smbh(mode: i64) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "defm_spl_sur");
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [1.0, 2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    t_long(&mut surface, mode);
    for vector in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
    ] {
        t_vec(&mut surface, vector);
    }
    t_dbl(&mut surface, 0.5);
    surface.extend_from_slice(&[0x0a, 0x0b, 0x0a]);
    for vector in [[1.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 0.0, 1.0]] {
        t_vec(&mut surface, vector);
    }
    t_dbl(&mut surface, 0.75);
    surface.extend_from_slice(&[0x0b, 0x0a]);
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    surface.extend_from_slice(&[0x0a, 0x0b, 0x0a, 0x0b, 0x0a]);
    if mode == 1 {
        t_long(&mut surface, 2);
        for value in [0.1, 0.2, 0.3, 0.4, 0.5, 0.6] {
            t_dbl(&mut surface, value);
        }
    } else {
        t_long(&mut surface, 1);
        t_dbl(&mut surface, 0.9);
    }
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_revision_deformable_surface_smbh() -> Vec<u8> {
    let mut bytes = synthetic_minimal_deformable_surface_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "defm_spl_sur");
    t_long(&mut surface, 22_506);
    t_ident(&mut surface, "cone");
    t_pos(&mut surface, [0.0, 0.0, 0.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [2.0, 0.0, 0.0]);
    t_dbl(&mut surface, 1.2);
    surface.extend_from_slice(&[0x0b, 0x0b]);
    t_dbl(&mut surface, 0.5);
    t_dbl(&mut surface, 0.866_025_403_784_438_6);
    t_dbl(&mut surface, 0.25);
    surface.push(0x0b);
    for bound in [0.0, 1.0, 0.0, 1.0] {
        surface.push(0x0a);
        t_dbl(&mut surface, bound);
    }
    t_long(&mut surface, 3);
    for vector in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
    ] {
        t_vec(&mut surface, vector);
    }
    t_dbl(&mut surface, 2.5);
    surface.extend_from_slice(&[0x0a, 0x0b, 0x0a]);
    t_pos(&mut surface, [1.0, 2.0, 3.0]);
    t_vec(&mut surface, [2.0, 0.0, 0.0]);
    t_vec(&mut surface, [0.0, 2.0, 0.0]);
    t_dbl(&mut surface, 3.5);
    surface.extend_from_slice(&[0x0b, 0x0a]);
    for value in [4.5, 5.5, 6.5] {
        t_dbl(&mut surface, value);
    }
    surface.extend_from_slice(&[0x0a, 0x0b, 0x0a, 0x0b, 0x0a]);
    t_dbl(&mut surface, 7.5);
    t_long(&mut surface, 19);
    push_native_enum(&mut surface, 0);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    for values in [&[][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0b);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_surface_curve_deformable_smbh() -> Vec<u8> {
    let mut bytes = synthetic_minimal_deformable_surface_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "defm_spl_sur");
    for z in [0.0, 1.0] {
        t_ident(&mut surface, "plane");
        t_pos(&mut surface, [0.0, 0.0, z]);
        t_vec(&mut surface, [0.0, 0.0, 1.0]);
        t_vec(&mut surface, [1.0, 0.0, 0.0]);
        surface.push(0x0b);
        if z == 0.0 {
            t_long(&mut surface, 5);
        }
    }
    t_long(&mut surface, 42);
    surface.push(0x0a);
    t_dbl(&mut surface, 0.2);
    t_long(&mut surface, 3);
    t_dbl(&mut surface, 0.4);
    surface.extend_from_slice(&generated_curve_block());
    for v in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
    ] {
        t_vec(&mut surface, v);
    }
    t_dbl(&mut surface, 0.6);
    surface.extend_from_slice(&[0x0a, 0x0b, 0x0a]);
    t_long(&mut surface, 1);
    for v in [0.1, 0.2, 0.3] {
        t_dbl(&mut surface, v);
    }
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_full_deformable_surface_smbh(version_value: Option<i64>) -> Vec<u8> {
    let mut bytes = synthetic_minimal_deformable_surface_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "defm_spl_sur");
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [0.0, 0.0, 0.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    t_long(&mut surface, 6);
    for v in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
    ] {
        t_vec(&mut surface, v);
    }
    t_dbl(&mut surface, 0.1);
    surface.extend_from_slice(&[0x0a, 0x0b, 0x0a]);
    t_long(&mut surface, 7);
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    t_long(&mut surface, 42);
    surface.push(0x0a);
    t_dbl(&mut surface, 0.2);
    if let Some(version_value) = version_value {
        t_long(&mut surface, version_value);
    }
    t_dbl(&mut surface, 0.3);
    surface.extend_from_slice(&generated_curve_block());
    for frame in 0..2 {
        for v in [
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 1.0],
            [1.0, 0.0, 1.0],
            [-1.0, 1.0, 0.0],
        ] {
            t_vec(&mut surface, v);
        }
        t_dbl(&mut surface, 0.4 + f64::from(frame) * 0.1);
        surface.extend_from_slice(&[0x0b, 0x0a, 0x0b]);
    }
    t_long(&mut surface, 99);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_referenced_t_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old_offset = records[9].offset;
    let old_len = records[9].len;
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "t_spl_subtrans_object");
    t_u16_string(&mut surface, "degree 3\nv 1 0 0 0\n");
    t_u16_string(&mut surface, "100verts 1\n");
    surface.push(0x10);
    surface.push(0x0f);
    t_ident(&mut surface, "t_spl_sur");
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0b);
    for value in [-2.0, 3.0, -4.0, 5.0] {
        t_dbl(&mut surface, value);
    }
    t_long(&mut surface, 7);
    surface.push(0x0f);
    t_ident(&mut surface, "ref");
    let reference_value_offset = surface.len() + 1;
    t_long(&mut surface, 0);
    surface.push(0x10);
    t_long(&mut surface, 9);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old_offset..old_offset + old_len, surface);
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        asm_header::record_stream_start(&bytes).unwrap(),
        asm_header::solved_record_limit(&bytes).unwrap(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    // Resolve the fixture's unique shared definition in framed token order,
    // including nested definitions and excluding reference wrappers.
    let definitions: Vec<&str> = records
        .iter()
        .flat_map(|record| record.tokens.windows(2))
        .filter_map(|pair| match pair {
            [cadmpeg_asm::sab::Token::SubtypeOpen,
             cadmpeg_asm::sab::Token::Ident(name) | cadmpeg_asm::sab::Token::SubIdent(name)]
                if name != "ref" => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        definitions
            .iter()
            .filter(|name| **name == "t_spl_subtrans_object")
            .count(),
        1
    );
    let index = definitions
        .iter()
        .position(|name| *name == "t_spl_subtrans_object")
        .expect("shared T-spline subtype index");
    bytes[old_offset + reference_value_offset..old_offset + reference_value_offset + 8]
        .copy_from_slice(&i64::try_from(index).unwrap().to_le_bytes());
    bytes
}

pub(crate) fn synthetic_explicit_formula_sweep_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "sweep_spl_sur");
    surface.push(0x15);
    surface.extend_from_slice(&2i64.to_le_bytes());
    t_long(&mut surface, 7);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -0.5);
    t_dbl(&mut surface, 1.5);
    surface.push(0x0a);
    t_pos(&mut surface, [1.0, 2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    for direction in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        t_vec(&mut surface, direction);
    }
    t_long(&mut surface, 1);
    surface.push(0x0a);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -2.0);
    t_dbl(&mut surface, 3.0);
    t_dbl(&mut surface, 0.75);
    surface.push(0x0b);
    push_u8_string(&mut surface, "null_law");
    surface.push(0x0a);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.005);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0b);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_explicit_guide_sweep_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "sweep_spl_sur");
    surface.push(0x15);
    surface.extend_from_slice(&2i64.to_le_bytes());
    t_long(&mut surface, 8);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -0.25);
    t_dbl(&mut surface, 1.25);
    surface.push(0x0b);
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    for direction in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        t_vec(&mut surface, direction);
    }
    t_long(&mut surface, 2);
    surface.push(0x0a);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -2.0);
    t_dbl(&mut surface, 3.0);
    t_dbl(&mut surface, 0.5);
    surface.extend_from_slice(&[0x0a, 0x0b]);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, 0.0);
    t_dbl(&mut surface, 1.0);
    t_long(&mut surface, 11);
    t_long(&mut surface, 12);
    for value in [0.1, 0.2, 0.3, 0.4, 0.5, 0.6] {
        t_dbl(&mut surface, value);
    }
    surface.extend_from_slice(&[0x0a, 0x0b, 0x0a]);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.005);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_explicit_surface_sweep_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "sweep_spl_sur");
    surface.push(0x15);
    surface.extend_from_slice(&2i64.to_le_bytes());
    t_long(&mut surface, 9);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, 0.0);
    t_dbl(&mut surface, 1.0);
    surface.push(0x0b);
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    for direction in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        t_vec(&mut surface, direction);
    }
    t_long(&mut surface, 3);
    surface.push(0x0b);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -2.0);
    t_dbl(&mut surface, 3.0);
    t_dbl(&mut surface, 0.25);
    surface.push(0x15);
    surface.extend_from_slice(&1i64.to_le_bytes());
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [1.0, 2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    surface.push(0x0a);
    surface.extend_from_slice(&generated_curve_block());
    surface.push(0x0a);
    surface.push(0x0b);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.005);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_law_driven_sweep_smbh() -> Vec<u8> {
    synthetic_law_driven_sweep_smbh_with_law_slots(None, None)
}

pub(crate) fn synthetic_text_law_driven_sweep_smbh() -> Vec<u8> {
    synthetic_law_driven_sweep_smbh_with_law_slots(
        Some("0.008726867790758789*X"),
        Some("VEC(1,1,1)"),
    )
}

pub(crate) fn synthetic_revision_text_law_sweep_smbh() -> Vec<u8> {
    synthetic_revision_text_law_sweep_with_tail_smbh(0)
}

pub(crate) fn synthetic_cacheless_revision_text_law_sweep_smbh() -> Vec<u8> {
    synthetic_revision_text_law_sweep_with_tail_smbh(2)
}

fn synthetic_revision_text_law_sweep_with_tail_smbh(tail_form: i64) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "sweep_sur");
    t_long(&mut surface, 23100);
    surface.push(0x0a);
    t_long(&mut surface, 10);
    surface.extend_from_slice(&generated_curve_block());
    for value in [0.0, 1.0] {
        surface.push(0x0a);
        t_dbl(&mut surface, value);
    }
    for value in [0.0, 1.0] {
        surface.push(0x0a);
        t_dbl(&mut surface, value);
    }
    surface.push(0x0b);
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    for direction in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        t_vec(&mut surface, direction);
    }
    push_u8_string(&mut surface, "0.008726867790758789*X");
    t_long(&mut surface, 21);
    for value in [-1.0, 1.0] {
        surface.push(0x0a);
        t_dbl(&mut surface, value);
    }
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_long(&mut surface, 1);
    surface.push(0x0a);
    t_ident(&mut surface, "straight");
    t_pos(&mut surface, [0.0, 0.0, 0.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    surface.extend_from_slice(&[0x0b, 0x0b]);
    for value in [0.0, 0.8] {
        surface.push(0x0a);
        t_dbl(&mut surface, value);
    }
    t_dbl(&mut surface, 0.0);
    surface.push(0x0a);
    push_u8_string(&mut surface, "VEC(1,1,1)");
    t_long(&mut surface, 0);
    push_u8_string(&mut surface, "ROTATE(DOMAIN(VEC(1,0,0),0,0.8),TRANS1)");
    t_long(&mut surface, 1);
    push_u8_string(&mut surface, "TRANS");
    for vector in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0],
    ] {
        t_vec(&mut surface, vector);
    }
    t_dbl(&mut surface, 1.0);
    surface.extend_from_slice(&[0x0b, 0x0b, 0x0b]);
    surface.push(0x0b);
    append_revision_surface_tail_head(&mut surface, tail_form, 0.005);
    append_revision_surface_tail_discontinuities(&mut surface);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_law_driven_sweep_smbh_with_law_slots(
    first_law_text: Option<&str>,
    second_law_text: Option<&str>,
) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "sweep_spl_sur");
    surface.push(0x15);
    surface.extend_from_slice(&5i64.to_le_bytes());
    t_long(&mut surface, 10);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, 0.0);
    t_dbl(&mut surface, 1.0);
    surface.push(0x0b);
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    for direction in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        t_vec(&mut surface, direction);
    }
    if let Some(value) = first_law_text {
        push_u8_string(&mut surface, value);
    } else {
        t_dbl(&mut surface, 2.5);
    }
    t_long(&mut surface, 21);
    t_dbl(&mut surface, -1.0);
    t_dbl(&mut surface, 1.0);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_long(&mut surface, 22);
    surface.push(0x0a);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -2.0);
    t_dbl(&mut surface, 3.0);
    t_dbl(&mut surface, 0.75);
    surface.push(0x0b);
    if let Some(value) = second_law_text {
        push_u8_string(&mut surface, value);
    } else {
        t_vec(&mut surface, [1.0, 2.0, 3.0]);
    }
    t_long(&mut surface, 23);
    push_u8_string(&mut surface, "null_law");
    surface.push(0x0a);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.005);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0b);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn append_generated_compound_loft_scale(bytes: &mut Vec<u8>) {
    t_long(bytes, 1);
    t_long(bytes, 9);
    bytes.extend_from_slice(&generated_curve_block());
    t_ident(bytes, "plane");
    t_pos(bytes, [1.0, -2.0, 3.0]);
    t_vec(bytes, [0.0, 0.0, 1.0]);
    t_vec(bytes, [1.0, 0.0, 0.0]);
    bytes.push(0x0b);
    bytes.extend_from_slice(&generated_pcurve_block());
    bytes.push(0x0b);
    t_long(bytes, -1);
    t_long(bytes, 211);
    t_long(bytes, 4);
    t_long(bytes, 0);
    t_dbl(bytes, -0.25);
    t_dbl(bytes, 0.75);
    bytes.push(0x0a);
    t_vec(bytes, [0.0, 1.0, 0.0]);
    bytes.extend_from_slice(&generated_curve_block());
    t_long(bytes, 1);
    bytes.extend_from_slice(&generated_curve_block());
    t_long(bytes, 2);
    t_long(bytes, 3);
}

pub(crate) fn synthetic_compound_loft_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "cl_loft_spl_sur");
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    append_generated_compound_loft_scale(&mut surface);
    surface.push(0x0a);
    surface.push(0x0b);
    t_long(&mut surface, 0);
    surface.push(0x0b);
    surface.push(0x0a);
    t_long(&mut surface, 0);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    surface.push(0x0a);
    surface.push(0x0b);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn append_generated_float_array(bytes: &mut Vec<u8>, values: &[f64]) {
    t_long(bytes, i64::try_from(values.len()).unwrap());
    for value in values {
        t_dbl(bytes, *value);
    }
}

pub(crate) fn synthetic_scaled_compound_loft_smbh(full: bool) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "scaled_cloft_spl_sur");
    surface.push(0x15);
    surface.extend_from_slice(&11i64.to_le_bytes());
    if full {
        surface.extend_from_slice(&generated_surface_block());
        t_dbl(&mut surface, 0.004);
    } else {
        for value in [-1.0, 2.0, -3.0, 4.0] {
            t_dbl(&mut surface, value);
        }
        append_generated_float_array(&mut surface, &[0.25]);
        append_generated_float_array(&mut surface, &[0.5, 0.75]);
    }
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    append_generated_compound_loft_scale(&mut surface);
    surface.push(0x0a);
    surface.push(0x0b);
    t_long(&mut surface, 0);
    surface.push(0x0b);
    surface.push(0x0a);
    t_long(&mut surface, 0);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    surface.push(0x0b);
    surface.push(0x0a);
    t_long(&mut surface, 2);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    t_vec(&mut surface, [0.0, 1.0, 0.0]);
    surface.push(0x15);
    surface.extend_from_slice(&12i64.to_le_bytes());
    surface.extend_from_slice(&generated_curve_block());
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_skin_spl_sur_smbh(law_case: u8, expanded: bool) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "skin_spl_sur");
    for value in [1i64, 2, 3] {
        surface.push(0x15);
        surface.extend_from_slice(&value.to_le_bytes());
    }
    t_long(&mut surface, 4);
    t_dbl(&mut surface, 0.25);
    t_long(&mut surface, 1);
    if expanded {
        t_long(&mut surface, 9);
        surface.extend_from_slice(&generated_curve_block());
        t_ident(&mut surface, "plane");
        t_pos(&mut surface, [1.0, -2.0, 3.0]);
        t_vec(&mut surface, [0.0, 0.0, 1.0]);
        t_vec(&mut surface, [1.0, 0.0, 0.0]);
        surface.push(0x0b);
        surface.extend_from_slice(&generated_pcurve_block());
        surface.push(0x0b);
        t_long(&mut surface, -1);
        t_long(&mut surface, 211);
        t_long(&mut surface, 4);
        t_long(&mut surface, 0);
        t_dbl(&mut surface, -0.5);
        t_dbl(&mut surface, 1.5);
        surface.push(0x0a);
        t_vec(&mut surface, [0.0, 1.0, 0.0]);
        surface.extend_from_slice(&generated_curve_block());
        t_long(&mut surface, -1);
        t_long(&mut surface, 7);
    } else {
        surface.extend_from_slice(&generated_curve_block());
        t_long(&mut surface, 211);
        t_long(&mut surface, 4);
        t_long(&mut surface, 0);
        t_dbl(&mut surface, -0.5);
        t_dbl(&mut surface, 1.5);
        t_long(&mut surface, -1);
        surface.extend_from_slice(&generated_curve_block());
        t_long(&mut surface, 7);
    }
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_dbl(&mut surface, 0.75);
    if law_case == 1 {
        push_u8_string(&mut surface, "structural-law");
        t_long(&mut surface, 3);
        push_u8_string(&mut surface, "null_law");
        push_u8_string(&mut surface, "TRANS");
        for value in 0..13 {
            t_dbl(&mut surface, f64::from(value) / 10.0);
        }
        for value in [4i64, 5, 6] {
            surface.push(0x15);
            surface.extend_from_slice(&value.to_le_bytes());
        }
        push_u8_string(&mut surface, "EDGE");
        surface.extend_from_slice(&generated_curve_block());
        t_dbl(&mut surface, -0.25);
        t_dbl(&mut surface, 1.25);
    } else if law_case == 2 {
        push_u8_string(&mut surface, "algebraic-law");
        t_long(&mut surface, 2);
        push_u8_string(&mut surface, "SIN");
        push_u8_string(&mut surface, "ABS");
        t_dbl(&mut surface, -2.5);
        push_u8_string(&mut surface, "DOT");
        t_vec(&mut surface, [1.0, 0.0, 0.0]);
        t_vec(&mut surface, [0.0, 1.0, 0.0]);
    } else {
        push_u8_string(&mut surface, "skin-law");
        t_long(&mut surface, 1);
        push_u8_string(&mut surface, "SPLINE_LAW");
        t_long(&mut surface, 5);
        append_generated_float_array(&mut surface, &[0.0, 0.5, 1.0]);
        append_generated_float_array(&mut surface, &[1.0, 2.0, 3.0]);
        t_pos(&mut surface, [1.0, 2.0, 3.0]);
    }
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.006);
    for values in [
        &[0.1][..],
        &[0.2, 0.3][..],
        &[][..],
        &[][..],
        &[][..],
        &[][..],
    ] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_law_spl_sur_smbh(
    name: &str,
    legacy_ranges: bool,
    tail_selector: i64,
) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    if legacy_ranges {
        for value in [-1.0, 2.0, -3.0, 4.0] {
            t_dbl(&mut surface, value);
        }
    }
    push_u8_string(&mut surface, "primary-law");
    t_long(&mut surface, 1);
    push_u8_string(&mut surface, "SET");
    t_dbl(&mut surface, -2.5);
    t_long(&mut surface, 1);
    push_u8_string(&mut surface, "aux-law");
    t_long(&mut surface, 1);
    push_u8_string(&mut surface, "TERM");
    t_vec(&mut surface, [1.0, 2.0, 3.0]);
    t_long(&mut surface, 1);
    if !legacy_ranges {
        surface.push(0x15);
        surface.extend_from_slice(&tail_selector.to_le_bytes());
    } else {
        assert_eq!(tail_selector, 0);
    }
    match tail_selector {
        0 => {
            surface.extend_from_slice(&generated_surface_block());
            t_dbl(&mut surface, 0.007);
        }
        1 => {
            append_generated_float_array(&mut surface, &[0.0, 0.5, 1.0]);
            append_generated_float_array(&mut surface, &[-1.0, 1.0]);
            t_dbl(&mut surface, 0.008);
            for value in [0i64, 2, 1, 3] {
                surface.push(0x15);
                surface.extend_from_slice(&value.to_le_bytes());
            }
        }
        2 => {
            for value in [-0.5, 1.5, -2.0, 2.0] {
                t_dbl(&mut surface, value);
            }
            for value in [1i64, 2, 0, 4] {
                surface.push(0x15);
                surface.extend_from_slice(&value.to_le_bytes());
            }
        }
        3 | 4 => {}
        _ => panic!("invalid law tail selector"),
    }
    for values in [
        &[0.1][..],
        &[0.2, 0.3][..],
        &[][..],
        &[][..],
        &[][..],
        &[][..],
    ] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_sub_spl_sur_smbh(name: &str) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = &records[9];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, name);
    for value in [-1.0, 2.0, -3.0, 4.0] {
        t_dbl(&mut surface, value);
    }
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [0.1, -0.2, 0.3]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}
