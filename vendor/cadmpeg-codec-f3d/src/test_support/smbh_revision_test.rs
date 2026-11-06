// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_asm::asm_header;

use crate::test_support::smbh_blocks_test::{
    generated_curve_block, generated_pcurve_block, generated_surface_block,
};
use crate::test_support::smbh_geometry_test::synthetic_mixed_smbh;
use crate::test_support::tokens_test::{t_dbl, t_end, t_ident, t_long, t_ref, t_subident};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;

pub(crate) fn push_revision_surface_tail(surface: &mut Vec<u8>) {
    surface.push(0x15);
    surface.extend_from_slice(&0i64.to_le_bytes());
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(surface, 0.002);
    for _ in 0..6 {
        t_long(surface, 0);
    }
    surface.push(0x0b);
}

/// The shared revision-gated surface tail in cache form `2`: no solved cache
/// and no fit tolerance, the U parameter interval, the V parameter interval,
/// then the U closure, V closure, U singularity, and V singularity enums.
pub(crate) fn push_parameterized_revision_surface_tail(surface: &mut Vec<u8>) {
    surface.push(0x15);
    surface.extend_from_slice(&2i64.to_le_bytes());
    // U interval: present lower bound, absent upper bound.
    surface.push(0x0a);
    t_dbl(surface, 0.25);
    surface.push(0x0b);
    // V interval: both bounds present.
    for value in [-1.5, 3.5] {
        surface.push(0x0a);
        t_dbl(surface, value);
    }
    for value in [1, 0, 2, 3] {
        surface.push(0x15);
        surface.extend_from_slice(&i64::from(value).to_le_bytes());
    }
    for _ in 0..6 {
        t_long(surface, 0);
    }
    surface.push(0x0b);
}

/// Replace record 9 of the mixed stream with a revision-gated spline-surface
/// record whose subtype body is built by `body`.
pub(crate) fn synthetic_revision_surface_smbh(
    subtype: &str,
    body: impl FnOnce(&mut Vec<u8>),
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
    t_ident(&mut surface, subtype);
    t_long(&mut surface, 23100);
    body(&mut surface);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

pub(super) fn scrubbed_definition(
    definition: &cadmpeg_ir::geometry::ProceduralSurfaceDefinition,
) -> String {
    let text = serde_json::to_string(definition).expect("definition JSON");
    let mut out = String::with_capacity(text.len());
    let mut in_index = false;
    for c in text.chars() {
        if in_index && c.is_ascii_digit() {
            continue;
        }
        in_index = c == '#';
        out.push(c);
    }
    out
}

/// A revision-gated `loft_spl_sur` body holding one section entry with one
/// profile member. `type_code` selects the member payload: a nonzero type
/// stores the support surface, one nullable pcurve, and the first flag; a zero
/// type stores two nullable pcurve slots and no first flag. `asm_extension`
/// carries the ASM integer only when the stream save format stores it, and
/// `tail` writes the shared revision-gated surface tail in the cache form
/// under test.
pub(crate) fn push_revision_loft_body(
    surface: &mut Vec<u8>,
    type_code: i64,
    asm_extension: Option<i64>,
    tail: fn(&mut Vec<u8>),
) {
    t_long(surface, 1);
    t_dbl(surface, 0.0);
    t_long(surface, 1);
    t_long(surface, type_code);
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&[0x0b, 0x0b]);
    if type_code == 0 {
        surface.extend_from_slice(&generated_pcurve_block());
        t_ident(surface, "nullbs");
    } else {
        t_ident(surface, "null_surface");
        t_ident(surface, "nullbs");
        surface.push(0x0b);
    }
    if let Some(value) = asm_extension {
        t_long(surface, value);
    }
    t_long(surface, 213);
    t_long(surface, 1);
    t_long(surface, 1);
    for value in [0.0, 1.0, 0.25, 0.75, 0.5, 1.5] {
        t_dbl(surface, value);
    }
    surface.push(0x0b);
    t_ident(surface, "null_curve");
    t_long(surface, 0);
    t_long(surface, -1);
    t_long(surface, 0);
    for value in [0.0, 1.0, 0.0, 1.0] {
        surface.push(0x0a);
        t_dbl(surface, value);
    }
    surface.extend_from_slice(&[0x0b; 4]);
    t_long(surface, 0);
    t_long(surface, 0);
    tail(surface);
}

/// The single revision-gated profile member of a decoded loft construction.
pub(crate) fn decoded_revision_loft_member(
    ir: &cadmpeg_ir::document::CadIr,
) -> cadmpeg_ir::geometry::LoftProfileMember {
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Loft(definition_payload) = &ir
        .model
        .procedural_surfaces
        .first()
        .expect("revision loft construction")
        .definition()
    else {
        panic!("expected a loft construction")
    };
    let sections = definition_payload.sections();
    let revision_form = definition_payload.revision_form();

    assert!(revision_form.is_some());
    sections[0].entries[0].profile[0].to_raw()
}

/// Byte-exact re-emission of the decoded construction's subtype span.
pub(crate) fn regenerated_procedural_surface_span(ir: &cadmpeg_ir::document::CadIr) -> Vec<u8> {
    let procedural = ir
        .model
        .procedural_surfaces
        .first()
        .expect("procedural construction");
    let surface = ir
        .model
        .surfaces
        .iter()
        .find(|surface| ir.model.procedural_surface_owner(&procedural.id) == Some(&surface.id))
        .expect("solved surface");
    let Some(SolvedSurfaceGeometry::Nurbs(cache)) = surface.geometry.solved_cache() else {
        panic!("expected a solved NURBS cache")
    };
    let mut bytes = Vec::new();
    crate::writer::generate::native_geometry::native_procedural_surface(
        &mut bytes, ir, surface, cache,
    )
    .expect("regenerate procedural surface");
    let inner = bytes
        .iter()
        .position(|&byte| byte == 0x0f)
        .expect("subtype opening");
    cadmpeg_asm::nurbs::subtypes::subtype_span(
        &bytes,
        inner,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("subtype span")
    .bytes()
    .to_vec()
}

/// The subtype span of the synthetic stream's revision-gated surface record.
pub(crate) fn synthetic_revision_surface_subtype_span(smbh: &[u8]) -> Vec<u8> {
    let start = asm_header::record_stream_start(smbh).unwrap();
    let limit = asm_header::solved_record_limit(smbh).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        smbh,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let record = &records[9];
    let slice = &smbh[record.offset..record.offset + record.len];
    let inner = slice.iter().position(|&byte| byte == 0x0f).unwrap();
    cadmpeg_asm::nurbs::subtypes::subtype_span(
        slice,
        inner,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap()
    .bytes()
    .to_vec()
}

/// The parameterization the shared form-`2` tail builder writes.
pub(crate) fn assert_parameterized_tail(cache: &cadmpeg_ir::geometry::RevisionCacheForm) {
    let parameterization = cache.parameterization().expect("tail parameterization");
    assert_eq!(parameterization.u_interval, [Some(0.25), None]);
    assert_eq!(parameterization.v_interval, [Some(-1.5), Some(3.5)]);
    assert_eq!(
        (parameterization.u_closure, parameterization.v_closure),
        (1, 0)
    );
    assert_eq!(
        (
            parameterization.u_singularity,
            parameterization.v_singularity
        ),
        (2, 3)
    );
}
