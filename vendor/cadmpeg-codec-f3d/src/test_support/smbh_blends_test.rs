// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_asm::asm_header;

use crate::test_support::smbh_blocks_test::{
    generated_curve_block, generated_pcurve_block, generated_rational_surface_block,
    generated_surface_block,
};
use crate::test_support::smbh_geometry_test::synthetic_mixed_smbh;
use crate::test_support::smbh_surfaces_test::{
    append_revision_surface_tail_discontinuities, append_revision_surface_tail_head,
    synthetic_cyl_spl_sur_smbh, synthetic_versioned_cyl_spl_sur_smbh,
};
use crate::test_support::tokens_test::{
    push_tagged_i64, push_u8_string, t_dbl, t_end, t_ident, t_long, t_pos, t_ref, t_subident, t_vec,
};

fn append_generated_g2_side(bytes: &mut Vec<u8>, label: &str) {
    push_u8_string(bytes, label);
    t_ident(bytes, "plane");
    t_pos(bytes, [1.0, -2.0, 3.0]);
    t_vec(bytes, [0.0, 0.0, 1.0]);
    t_vec(bytes, [1.0, 0.0, 0.0]);
    bytes.push(0x0b);
    bytes.extend_from_slice(&generated_curve_block());
    bytes.extend_from_slice(&generated_pcurve_block());
    t_vec(bytes, [0.0, 1.0, 0.0]);
    bytes.extend_from_slice(&generated_pcurve_block());
}

pub(crate) fn synthetic_g2_blend_spl_sur_smbh(name: &str, full: bool) -> Vec<u8> {
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
    append_generated_g2_side(&mut surface, "first");
    surface.push(0x15);
    surface.extend_from_slice(&(if full { 11i64 } else { 12i64 }).to_le_bytes());
    if full {
        surface.extend_from_slice(&generated_surface_block());
        t_dbl(&mut surface, 0.002);
    } else {
        for value in 1..=9 {
            t_dbl(&mut surface, f64::from(value));
        }
        t_dbl(&mut surface, 0.003);
        t_long(&mut surface, 44);
        surface.extend_from_slice(&generated_pcurve_block());
    }
    append_generated_g2_side(&mut surface, "second");
    surface.extend_from_slice(&generated_surface_block());
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -0.5);
    t_dbl(&mut surface, 1.5);
    t_long(&mut surface, 8);
    for value in [-1.0, 2.0, -3.0, 4.0, 0.1, 0.2, 0.3, 0.4] {
        t_dbl(&mut surface, value);
    }
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.0095);
    t_long(&mut surface, 1);
    t_dbl(&mut surface, 0.25);
    t_long(&mut surface, 0);
    t_long(&mut surface, 2);
    t_dbl(&mut surface, 0.5);
    t_dbl(&mut surface, 0.75);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_rational_cyl_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_cyl_spl_sur_smbh();
    let old = generated_surface_block();
    let start = bytes
        .windows(old.len())
        .rposition(|window| window == old)
        .expect("generated solved surface cache");
    bytes.splice(start..start + old.len(), generated_rational_surface_block());
    bytes
}

pub(crate) fn synthetic_ref_cyl_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_cyl_spl_sur_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let asmheader = &records[0];
    let surface = &records[9];
    let marker = b"\x0f\x0d\x0bcyl_spl_sur";
    let relative = bytes[surface.offset..surface.offset + surface.len]
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap();
    let target_start = surface.offset + relative;
    let target_end = surface.offset + surface.len - 1;
    let target = bytes[target_start..target_end].to_vec();

    let mut reference = Vec::new();
    reference.extend_from_slice(b"\x0f\x0d\x03ref\x04");
    reference.extend_from_slice(&0i64.to_le_bytes());
    reference.push(0x10);
    bytes.splice(target_start..target_end, reference);
    let asmheader_end = asmheader.offset + asmheader.len - 1;
    bytes.splice(asmheader_end..asmheader_end, target);
    bytes
}

pub(crate) fn synthetic_revision_ref_directrix_cyl_spl_sur_smbh() -> Vec<u8> {
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
    let asmheader = &records[0];

    let mut target = Vec::new();
    target.push(0x0f);
    t_ident(&mut target, "exact_int_cur");
    target.extend_from_slice(&generated_curve_block());
    target.extend_from_slice(&generated_surface_block());
    t_dbl(&mut target, 0.009);
    target.push(0x10);
    let target_start = bytes
        .windows(target.len())
        .position(|window| window == target)
        .expect("inline directrix definition");

    let mut reference = vec![0x0f, 0x04];
    reference.extend_from_slice(&0i64.to_le_bytes());
    reference.push(0x10);
    bytes.splice(target_start..target_start + target.len(), reference);
    let asmheader_end = asmheader.offset + asmheader.len - 1;
    bytes.splice(asmheader_end..asmheader_end, target);
    bytes
}

pub(crate) fn synthetic_rb_blend_spl_sur_smbh() -> Vec<u8> {
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
    t_ident(&mut surface, "rb_blend_spl_sur");
    push_u8_string(&mut surface, "blend_support_surface");
    t_subident(&mut surface, "plane");
    surface.extend_from_slice(&generated_surface_block());
    push_u8_string(&mut surface, "blend_support_surface");
    t_subident(&mut surface, "sphere");
    surface.extend_from_slice(&generated_surface_block());
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -0.3);
    t_dbl(&mut surface, -0.3);
    push_tagged_i64(&mut surface, 0x15, -1);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.001);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn append_generated_rolling_ball_side(bytes: &mut Vec<u8>, label: &str, x: f64) {
    push_u8_string(
        bytes,
        if label == "left" {
            "blend_support_surface"
        } else {
            "blend_support_curve"
        },
    );
    t_ident(bytes, "plane");
    t_pos(bytes, [x, 0.0, 0.0]);
    t_vec(bytes, [0.0, 0.0, 1.0]);
    t_vec(bytes, [1.0, 0.0, 0.0]);
    bytes.push(0x0b);
    bytes.extend_from_slice(&[0x0b; 4]);
    bytes.extend_from_slice(&generated_curve_block());
    bytes.extend_from_slice(&[0x0b, 0x0b]);
    bytes.extend_from_slice(&generated_pcurve_block());
    t_pos(bytes, [x, 2.0, 3.0]);
    t_ident(bytes, "nullbs");
    t_long(bytes, if label == "left" { 3 } else { 4 });
    t_ident(bytes, "nullbs");
}

pub(crate) fn synthetic_full_rolling_ball_smbh(name: &str) -> Vec<u8> {
    synthetic_full_rolling_ball_with_tail_smbh(name, 0)
}

pub(crate) fn synthetic_full_rolling_ball_with_tail_smbh(name: &str, tail_form: i64) -> Vec<u8> {
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
    t_long(&mut surface, 22507);
    append_generated_rolling_ball_side(&mut surface, "left", 1.0);
    append_generated_rolling_ball_side(&mut surface, "right", 4.0);
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&[0x0b, 0x0b]);
    for value in [-0.3, -0.6] {
        t_dbl(&mut surface, value);
    }
    surface.push(0x15);
    surface.extend_from_slice(&(-1i64).to_le_bytes());
    for value in [-1.0, 2.0] {
        surface.push(0x0a);
        t_dbl(&mut surface, value);
    }
    surface.push(0x0b);
    surface.push(0x0b);
    t_long(&mut surface, 1);
    for value in [0.1, 0.2] {
        t_dbl(&mut surface, value);
    }
    t_long(&mut surface, 17);
    append_revision_surface_tail_head(&mut surface, tail_form, 0.004);
    append_revision_surface_tail_discontinuities(&mut surface);
    if matches!(name, "sss_blend_spl_sur" | "sssblndsur") {
        push_u8_string(&mut surface, "third");
        t_ident(&mut surface, "plane");
        t_pos(&mut surface, [0.0, 0.0, 1.0]);
        t_vec(&mut surface, [0.0, 1.0, 0.0]);
        t_vec(&mut surface, [1.0, 0.0, 0.0]);
        surface.push(0x0b);
        surface.extend_from_slice(&generated_curve_block());
        t_ident(&mut surface, "nullbs");
        t_vec(&mut surface, [0.0, 1.0, 0.0]);
        surface.extend_from_slice(&generated_pcurve_block());
        t_long(&mut surface, 23);
        t_ident(&mut surface, "nullbs");
        surface.push(0x0b);
    }
    for value in [11, 12, 13] {
        t_long(&mut surface, value);
    }
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn append_generated_variable_blend_side(bytes: &mut Vec<u8>, label: &str, x: f64) {
    push_u8_string(
        bytes,
        if label == "left" {
            "blend_support_surface"
        } else {
            "blendsupcur"
        },
    );
    t_ident(bytes, "plane");
    t_pos(bytes, [x, 0.0, 0.0]);
    t_vec(bytes, [0.0, 0.0, 1.0]);
    t_vec(bytes, [1.0, 0.0, 0.0]);
    bytes.push(0x0b);
    bytes.extend_from_slice(&[0x0b; 4]);
    bytes.extend_from_slice(&generated_curve_block());
    bytes.extend_from_slice(&[0x0b, 0x0b]);
    bytes.extend_from_slice(&generated_pcurve_block());
    t_pos(bytes, [x, 2.0, 3.0]);
    t_ident(bytes, "nullbs");
    t_long(bytes, if label == "left" { 0 } else { 5 });
    t_ident(bytes, "nullbs");
}

fn append_generated_variable_blend_value(
    bytes: &mut Vec<u8>,
    parameters: [f64; 2],
    radii: [f64; 2],
) {
    push_u8_string(bytes, "two_ends");
    t_long(bytes, 7);
    bytes.push(0x15);
    bytes.extend_from_slice(&3i64.to_le_bytes());
    bytes.push(0x0a);
    for value in parameters.into_iter().chain(radii) {
        t_dbl(bytes, value);
    }
}

/// An `edge_offset` radius law with no leading sub-discriminator: the
/// law-domain parameter range and one offset length.
fn append_generated_variable_blend_edge_offset_value(
    bytes: &mut Vec<u8>,
    parameters: [f64; 2],
    offset: f64,
) {
    push_u8_string(bytes, "edge_offset");
    push_tagged_i64(bytes, 0x15, 3);
    bytes.push(0x0a);
    for value in parameters.into_iter().chain([offset]) {
        t_dbl(bytes, value);
    }
}

/// An `interp` radius law: the law-domain parameter range, a `(u,radius)` BS2
/// function, the extension enum, the point count, and one radius point. The
/// payload ends at that point — nothing gates a trailing scalar pair.
fn append_generated_variable_blend_interp_value(bytes: &mut Vec<u8>) {
    push_u8_string(bytes, "interp");
    push_tagged_i64(bytes, 0x15, 0);
    bytes.push(0x0a);
    t_dbl(bytes, 0.0);
    t_dbl(bytes, 1.0);
    bytes.extend_from_slice(&generated_pcurve_block());
    push_tagged_i64(bytes, 0x15, 2);
    push_tagged_i64(bytes, 0x04, 1);
    for value in [0.5, 1.5, 0.25, 0.75] {
        t_dbl(bytes, value);
    }
    bytes.push(0x13);
    for value in [1.0f64, 2.0, 3.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0x14);
    for value in [0.0f64, 0.0, 1.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

/// Which radius law the synthetic stream stores as its first blend value.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FirstRadiusLaw {
    TwoEnds,
    Interp,
    /// `edge_offset` with no leading sub-discriminator: two law-domain
    /// parameters and one offset length.
    EdgeOffset,
}

pub(crate) fn synthetic_variable_blend_smbh(name: &str) -> Vec<u8> {
    synthetic_variable_blend_smbh_with_selector(name, false, None, [None, None])
}

pub(crate) fn synthetic_variable_blend_smbh_with_branch(
    name: &str,
    rounded_chamfer: bool,
) -> Vec<u8> {
    synthetic_variable_blend_smbh_with_selector(
        name,
        rounded_chamfer,
        rounded_chamfer.then_some(3),
        [None, None],
    )
}

pub(crate) fn synthetic_variable_blend_smbh_with_selector(
    name: &str,
    two_radii: bool,
    cross_section_selector: Option<i64>,
    v_range: [Option<f64>; 2],
) -> Vec<u8> {
    synthetic_variable_blend_smbh_inner(
        name,
        two_radii,
        cross_section_selector,
        v_range,
        FirstRadiusLaw::TwoEnds,
        0,
        11,
    )
}

/// The same stream whose shared revision-gated surface tail takes the given
/// form.
pub(crate) fn synthetic_variable_blend_smbh_with_tail_form(name: &str, tail_form: i64) -> Vec<u8> {
    synthetic_variable_blend_smbh_inner(
        name,
        false,
        None,
        [None, None],
        FirstRadiusLaw::TwoEnds,
        tail_form,
        11,
    )
}

/// The same stream with an `interp` first radius law, which places a radius
/// point immediately before the cross-section enum.
pub(crate) fn synthetic_variable_blend_smbh_with_interp_radius(
    name: &str,
    cross_section_selector: Option<i64>,
) -> Vec<u8> {
    synthetic_variable_blend_smbh_inner(
        name,
        false,
        cross_section_selector,
        [None, None],
        FirstRadiusLaw::Interp,
        0,
        11,
    )
}

/// The same stream with an `edge_offset` first radius law carrying no leading
/// sub-discriminator.
pub(crate) fn synthetic_variable_blend_smbh_with_edge_offset_radius(name: &str) -> Vec<u8> {
    synthetic_variable_blend_smbh_inner(
        name,
        false,
        None,
        [None, None],
        FirstRadiusLaw::EdgeOffset,
        0,
        11,
    )
}

/// The same cache-bearing stream with an explicit approximation-current value.
pub(crate) fn synthetic_variable_blend_smbh_with_cache_state(
    name: &str,
    shape_prefix: i64,
) -> Vec<u8> {
    synthetic_variable_blend_smbh_inner(
        name,
        false,
        None,
        [None, None],
        FirstRadiusLaw::TwoEnds,
        0,
        shape_prefix,
    )
}

fn synthetic_variable_blend_smbh_inner(
    name: &str,
    two_radii: bool,
    cross_section_selector: Option<i64>,
    v_range: [Option<f64>; 2],
    first_value: FirstRadiusLaw,
    tail_form: i64,
    shape_prefix: i64,
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
    t_long(&mut surface, 23100);
    append_generated_variable_blend_side(&mut surface, "left", 1.0);
    append_generated_variable_blend_side(&mut surface, "right", 4.0);
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&[0x0b, 0x0b]);
    t_dbl(&mut surface, -0.2);
    t_dbl(&mut surface, 0.4);
    surface.push(0x15);
    surface.extend_from_slice(&i64::from(two_radii).to_le_bytes());
    match first_value {
        FirstRadiusLaw::Interp => append_generated_variable_blend_interp_value(&mut surface),
        FirstRadiusLaw::EdgeOffset => {
            append_generated_variable_blend_edge_offset_value(&mut surface, [0.25, 0.75], 1.5);
        }
        FirstRadiusLaw::TwoEnds => {
            append_generated_variable_blend_value(&mut surface, [0.25, 0.75], [1.5, 2.5]);
        }
    }
    if !two_radii {
        if let Some(selector) = cross_section_selector {
            surface.push(0x15);
            surface.extend_from_slice(&selector.to_le_bytes());
            if matches!(selector, 1 | 7) {
                t_dbl(&mut surface, 2.0);
                t_dbl(&mut surface, 2.0);
            }
        }
    }
    if two_radii {
        append_generated_variable_blend_value(&mut surface, [0.1, 0.9], [3.5, 4.5]);
        if let Some(selector) = cross_section_selector {
            surface.push(0x15);
            surface.extend_from_slice(&selector.to_le_bytes());
            if selector == 3 {
                surface.push(0x0a);
                append_generated_variable_blend_value(&mut surface, [0.0, 1.0], [5.5, 6.5]);
            }
        }
    }
    for value in [-1.0, 2.0] {
        surface.push(0x0a);
        t_dbl(&mut surface, value);
    }
    // Second interval `(T lo, F)`: a lower bound with an unbounded-above
    // marker, or both bounds absent when `v_range` is `[None, None]`.
    for bound in v_range {
        match bound {
            Some(value) => {
                surface.push(0x0a);
                t_dbl(&mut surface, value);
            }
            None => surface.push(0x0b),
        }
    }
    t_long(&mut surface, shape_prefix);
    t_dbl(&mut surface, 0.125);
    t_dbl(&mut surface, 0.6);
    t_long(&mut surface, 12);
    append_revision_surface_tail_head(&mut surface, tail_form, 0.004);
    for values in [
        &[0.125][..],
        &[][..],
        &[0.25, 0.375][..],
        &[][..],
        &[0.5][..],
        &[][..],
    ] {
        t_long(&mut surface, i64::try_from(values.len()).unwrap());
        for value in values {
            t_dbl(&mut surface, *value);
        }
    }
    surface.push(0x0a);
    for value in [31, 32, 33] {
        t_long(&mut surface, value);
    }
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&[0x0b, 0x0b]);
    surface.push(0x0a);
    surface.push(0x0b);
    surface.push(0x0a);
    t_dbl(&mut surface, 0.0);
    surface.push(0x0a);
    t_dbl(&mut surface, 1.0);
    t_ident(&mut surface, "nullbs");
    surface.extend_from_slice(&generated_curve_block());
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn append_vertex_boundary_common(bytes: &mut Vec<u8>, kind: &str, x: f64) {
    push_u8_string(bytes, kind);
    bytes.push(0x0a);
    t_pos(bytes, [x, 0.0, 0.0]);
    bytes.push(0x0b);
    bytes.push(0x0a);
    t_dbl(bytes, x + 0.25);
}

pub(crate) fn synthetic_vertex_blend_smbh(name: &str) -> Vec<u8> {
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
    t_long(&mut surface, 4);

    append_vertex_boundary_common(&mut surface, "circle", 1.0);
    surface.extend_from_slice(&generated_curve_block());
    surface.push(0x15);
    surface.extend_from_slice(&1i64.to_le_bytes());
    t_pos(&mut surface, [2.0, 3.0, 4.0]);
    t_dbl(&mut surface, 0.1);
    t_dbl(&mut surface, 0.9);
    surface.push(0x0b);

    append_vertex_boundary_common(&mut surface, "deg", 2.0);
    t_pos(&mut surface, [5.0, 6.0, 7.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    t_vec(&mut surface, [0.0, 1.0, 0.0]);

    append_vertex_boundary_common(&mut surface, "pcurve", 3.0);
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [0.0, 0.0, 0.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    surface.extend_from_slice(&generated_pcurve_block());
    surface.push(0x0a);
    t_dbl(&mut surface, 0.002);

    append_vertex_boundary_common(&mut surface, "plane", 4.0);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_dbl(&mut surface, -0.5);
    t_dbl(&mut surface, 1.5);
    surface.extend_from_slice(&generated_curve_block());

    t_long(&mut surface, 17);
    t_dbl(&mut surface, 0.003);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(crate) fn synthetic_partial_rb_blend_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_rb_blend_spl_sur_smbh();
    let marker = b"\x0e\x06sphere";
    let start = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap();
    bytes.drain(start..start + marker.len());
    bytes
}
