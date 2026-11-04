// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_asm::asm_header;

use crate::test_support::smbh_header_test::smbh_header_prefix;
use crate::test_support::tokens_test::{
    push_u8_string, t_attribute_base, t_dbl, t_end, t_ident, t_long, t_pos, t_ref, t_subident,
    t_vec,
};

/// Assemble the active slice: header prefix + records + `delta_state` boundary.
/// `RecordTable` indices are the order below, starting at 0 (`asmheader`).
pub(crate) fn synthetic_geometry_smbh() -> Vec<u8> {
    // Indices: 0 asmheader, 1 body, 2 region, 3 shell, 4 face, 5 loop,
    // 6 plane, 7/8/9 coedges, 10/11/12 edges, 13/14/15 vertices,
    // 16/17/18 points.
    let mut r = Vec::new();

    // 0: asmheader
    t_ident(&mut r, "asmheader");
    push_u8_string(&mut r, "231.6.3.65535");
    t_end(&mut r);

    // 1: body  (chunk3 = first_region)
    t_ident(&mut r, "body");
    t_ref(&mut r, -1); // 0 attrib
    t_long(&mut r, 42); // 1 native ASM body key
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, 2); // 3 first_region
    t_ref(&mut r, -1); // 4 wire
    t_ref(&mut r, -1); // 5 transform
    t_end(&mut r);

    // 2: region  (chunk4 = first_shell, chunk5 = owner_body)
    t_ident(&mut r, "region");
    t_ref(&mut r, -1); // 0 next
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 null
    t_ref(&mut r, 3); // 4 first_shell
    t_ref(&mut r, 1); // 5 owner_body
    t_end(&mut r);

    // 3: shell  (chunk5 = first_face, chunk7 = owner_region)
    t_ident(&mut r, "shell");
    t_ref(&mut r, -1); // 0 next
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 null
    t_ref(&mut r, -1); // 4 null
    t_ref(&mut r, 4); // 5 first_face
    t_ref(&mut r, -1); // 6 wire
    t_ref(&mut r, 2); // 7 owner_region
    t_end(&mut r);

    // 4: face  (chunk4 first_loop, chunk5 owner_shell, chunk7 surface, chunk8 sense)
    t_ident(&mut r, "face");
    t_ref(&mut r, -1); // 0 attrib
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 next_face
    t_ref(&mut r, 5); // 4 first_loop
    t_ref(&mut r, 3); // 5 owner_shell
    t_ref(&mut r, -1); // 6 null
    t_ref(&mut r, 6); // 7 surface
    r.push(0x0b); // 8 sense = forward
    r.push(0x0b); // 9 sides = single
    t_end(&mut r);

    // 5: loop  (chunk4 first_coedge, chunk5 owner_face)
    t_ident(&mut r, "loop");
    t_ref(&mut r, -1); // 0 attrib
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 next_loop
    t_ref(&mut r, 7); // 4 first_coedge
    t_ref(&mut r, 4); // 5 owner_face
    t_end(&mut r);

    // 6: plane-surface  (origin, normal, uv-origin)
    t_subident(&mut r, "plane");
    t_ident(&mut r, "surface");
    t_ref(&mut r, -1); // attrib
    t_long(&mut r, -1); // history
    t_ref(&mut r, -1); // null
    t_pos(&mut r, [0.0, 0.0, 0.0]); // root
    t_vec(&mut r, [0.0, 0.0, 1.0]); // normal
    t_vec(&mut r, [1.0, 0.0, 0.0]); // UV reference direction
    r.push(0x0b); // sense
    t_end(&mut r);

    // 7/8/9: coedges forming the ring 7 -> 8 -> 9 -> 7
    let coedges = [(7i64, 8, 9, 10), (8, 9, 7, 11), (9, 7, 8, 12)];
    for (_id, next, prev, edge) in coedges {
        t_ident(&mut r, "coedge");
        t_ref(&mut r, -1); // 0 attrib
        t_long(&mut r, -1); // 1 history
        t_ref(&mut r, -1); // 2 null
        t_ref(&mut r, next); // 3 next
        t_ref(&mut r, prev); // 4 prev
        t_ref(&mut r, -1); // 5 partner (open loop, none)
        t_ref(&mut r, edge); // 6 edge
        r.push(0x0b); // 7 sense = forward
        t_ref(&mut r, 5); // 8 owner_loop
        t_long(&mut r, 0); // 9 reserved
        t_ref(&mut r, -1); // 10 pcurve
        t_end(&mut r);
    }

    // 10/11/12: edges  (start, end vertices), curve = null
    let edges = [(10i64, 13, 14), (11, 14, 15), (12, 15, 13)];
    for (_id, start, end) in edges {
        t_ident(&mut r, "edge");
        t_ref(&mut r, -1); // 0 attrib
        t_long(&mut r, -1); // 1 history
        t_ref(&mut r, -1); // 2 null
        t_ref(&mut r, start); // 3 start_vertex
        t_dbl(&mut r, 0.0); // 4 t_start
        t_ref(&mut r, end); // 5 end_vertex
        t_dbl(&mut r, 1.0); // 6 t_end
        t_ref(&mut r, -1); // 7 owner_coedge
        t_ref(&mut r, -1); // 8 curve (degenerate: none)
        r.push(0x0b); // 9 sense
        push_u8_string(&mut r, "unknown"); // 10 continuity text
        t_end(&mut r);
    }

    // 13/14/15: vertices (owning_edge, index_flag, point)
    let verts = [(13i64, 10, 0, 16), (14, 10, 1, 17), (15, 12, 0, 18)];
    for (_id, edge, index_flag, point) in verts {
        t_ident(&mut r, "vertex");
        t_ref(&mut r, -1); // 0 attrib
        t_long(&mut r, -1); // 1 history
        t_ref(&mut r, -1); // 2 null
        t_ref(&mut r, edge); // 3 owning_edge
        t_long(&mut r, index_flag); // 4 index_flag
        t_ref(&mut r, point); // 5 point
        t_end(&mut r);
    }

    // 16/17/18: points  (coordinates in cm; ×10 = mm)
    let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    for p in points {
        t_ident(&mut r, "point");
        t_ref(&mut r, -1); // attrib
        t_long(&mut r, -1); // history
        t_ref(&mut r, -1); // null
        t_pos(&mut r, p);
        t_end(&mut r);
    }

    // History boundary: previous record's 0x11 + 0x0d 0x0b 'delta_state'.
    t_ident(&mut r, "delta_state"); // 0x0d 0x0b 'delta_state'

    let mut out = smbh_header_prefix();
    out.extend_from_slice(&r);
    out
}

pub(crate) fn replace_generated_record_head(bytes: &mut Vec<u8>, from: &str, to: &str) {
    let mut needle = vec![
        0x0d,
        u8::try_from(from.len()).expect("fixture value fits u8"),
    ];
    needle.extend_from_slice(from.as_bytes());
    let mut replacement = vec![0x0d, u8::try_from(to.len()).expect("fixture value fits u8")];
    replacement.extend_from_slice(to.as_bytes());
    let offsets = bytes
        .windows(needle.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == needle).then_some(offset))
        .collect::<Vec<_>>();
    for offset in offsets.into_iter().rev() {
        bytes.splice(offset..offset + needle.len(), replacement.iter().copied());
    }
}

pub(crate) fn append_generated_record_tail(bytes: &mut Vec<u8>, head: &str, tail: &[u8]) {
    let record_start = bytes
        .windows(b"\x0d\x09asmheader".len())
        .position(|window| window == b"\x0d\x09asmheader")
        .expect("generated ASM record table");
    let offsets = cadmpeg_asm::test_support::sab::frame(
        bytes,
        record_start,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("generated ASM records must frame")
    .into_iter()
    .filter(|record| record.head() == head)
    .map(|record| record.offset + record.len - 1)
    .collect::<Vec<_>>();
    for offset in offsets.into_iter().rev() {
        bytes.splice(offset..offset, tail.iter().copied());
    }
}

pub(crate) fn synthetic_geometry_with_history_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let name_tag = bytes
        .windows(b"\x0d\x0bdelta_state".len())
        .position(|window| window == b"\x0d\x0bdelta_state")
        .unwrap();
    let mut preamble = Vec::new();
    for name in ["Begin", "of", "ASM", "History"] {
        t_subident(&mut preamble, name);
    }
    t_ident(&mut preamble, "Data");
    t_ident(&mut preamble, "history_stream");
    for value in [2, 2, 0, 99] {
        t_long(&mut preamble, value);
    }
    for reference in [-1, 0, 1, -1] {
        t_ref(&mut preamble, reference);
    }
    t_end(&mut preamble);
    bytes.splice(name_tag..name_tag, preamble);

    let first_name_end = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        + b"delta_state".len();
    let mut tail = Vec::new();
    for value in [2, 1, 0] {
        t_long(&mut tail, value);
    }
    for reference in [-1, 1, 0, -1, 0] {
        t_ref(&mut tail, reference);
    }
    tail.push(0x0b);
    t_long(&mut tail, 1); // board present
    t_ref(&mut tail, 0); // board owner
    t_long(&mut tail, 2); // board number
    t_long(&mut tail, 1); // change present
    t_ref(&mut tail, 1830); // old
    t_ref(&mut tail, 1); // new: update
    t_long(&mut tail, 1); // change present
    t_ref(&mut tail, -1); // old null
    t_ref(&mut tail, 8); // new: insert
    t_long(&mut tail, 0); // end changes
    t_long(&mut tail, 0); // end boards
    t_end(&mut tail);
    t_ident(&mut tail, "history_payload");
    t_long(&mut tail, 37);
    t_ref(&mut tail, 1830);
    t_ref(&mut tail, -1);
    t_end(&mut tail);
    t_ident(&mut tail, "delta_state");
    for value in [3, 1, 0] {
        t_long(&mut tail, value);
    }
    for reference in [0, -1, 1, -1, 0] {
        t_ref(&mut tail, reference);
    }
    tail.push(0x0b);
    t_end(&mut tail);
    bytes.splice(first_name_end.., tail);
    bytes
}

pub(crate) fn synthetic_geometry_with_transform_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let limit = cadmpeg_asm::asm_header::solved_record_limit(&bytes).expect("history boundary");
    let start = cadmpeg_asm::asm_header::record_stream_start(&bytes).expect("record stream");
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("generated SAB");
    let body = &records[1];
    let transform_ref = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        body,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x0c,
    )
    .expect("body reference tokens")[4];
    bytes[transform_ref + 1..transform_ref + 9].copy_from_slice(&19i64.to_le_bytes());

    let mut transform = Vec::new();
    t_ident(&mut transform, "transform");
    for vector in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 2.0, 3.0],
    ] {
        t_vec(&mut transform, vector);
    }
    t_dbl(&mut transform, 1.0);
    transform.extend_from_slice(&[0x0b, 0x0b, 0x0b]);
    t_end(&mut transform);
    bytes.splice(limit..limit, transform);
    bytes
}

pub(crate) fn synthetic_geometry_with_body_color_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let limit = cadmpeg_asm::asm_header::solved_record_limit(&bytes).expect("history boundary");
    let start = cadmpeg_asm::asm_header::record_stream_start(&bytes).expect("record stream");
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("generated SAB");
    let body = &records[1];
    let attribute_ref = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        body,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x0c,
    )
    .expect("body reference tokens")[0];
    bytes[attribute_ref + 1..attribute_ref + 9].copy_from_slice(&19i64.to_le_bytes());

    let mut attribute = Vec::new();
    t_subident(&mut attribute, "rgb_color");
    t_subident(&mut attribute, "st");
    t_ident(&mut attribute, "attrib");
    t_attribute_base(&mut attribute, -1, -1, 1);
    t_dbl(&mut attribute, 0.1);
    t_dbl(&mut attribute, 0.2);
    t_dbl(&mut attribute, 0.3);
    t_dbl(&mut attribute, 1.0);
    t_end(&mut attribute);
    bytes.splice(limit..limit, attribute);
    bytes
}

fn synthetic_geometry_with_body_attribute_chain_smbh(attribute_chain: Vec<u8>) -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let limit = cadmpeg_asm::asm_header::solved_record_limit(&bytes).expect("history boundary");
    let start = cadmpeg_asm::asm_header::record_stream_start(&bytes).expect("record stream");
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("generated SAB");
    let body = &records[1];
    let attribute_ref = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        body,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x0c,
    )
    .expect("body reference tokens")[0];
    bytes[attribute_ref + 1..attribute_ref + 9].copy_from_slice(&19i64.to_le_bytes());
    bytes.splice(limit..limit, attribute_chain);
    bytes
}

pub(crate) fn synthetic_geometry_with_body_truecolor_chain_smbh() -> Vec<u8> {
    let mut attributes = Vec::new();
    t_subident(&mut attributes, "truecolor");
    t_subident(&mut attributes, "adesk");
    t_ident(&mut attributes, "attrib");
    t_attribute_base(&mut attributes, 20, -1, 1);
    attributes.push(0x17);
    attributes.extend_from_slice(&i64::from(0xc2_20_40_60_u32).to_le_bytes());
    t_end(&mut attributes);

    t_subident(&mut attributes, "rgb_color");
    t_subident(&mut attributes, "st");
    t_ident(&mut attributes, "attrib");
    t_attribute_base(&mut attributes, -1, 19, 1);
    for channel in [0.8, 0.7, 0.6, 1.0] {
        t_dbl(&mut attributes, channel);
    }
    t_end(&mut attributes);
    synthetic_geometry_with_body_attribute_chain_smbh(attributes)
}

pub(crate) fn synthetic_geometry_with_body_decimal_color_chain_smbh(decimal: &str) -> Vec<u8> {
    let mut attributes = Vec::new();
    t_subident(&mut attributes, "entatt_color");
    t_subident(&mut attributes, "bt");
    t_ident(&mut attributes, "attrib");
    t_attribute_base(&mut attributes, 20, -1, 1);
    push_u8_string(&mut attributes, decimal);
    t_end(&mut attributes);

    t_subident(&mut attributes, "rgb_color");
    t_subident(&mut attributes, "st");
    t_ident(&mut attributes, "attrib");
    t_attribute_base(&mut attributes, -1, 19, 1);
    for channel in [0.8, 0.7, 0.6, 1.0] {
        t_dbl(&mut attributes, channel);
    }
    t_end(&mut attributes);
    synthetic_geometry_with_body_attribute_chain_smbh(attributes)
}

pub(crate) fn synthetic_geometry_with_face_color_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let limit = cadmpeg_asm::asm_header::solved_record_limit(&bytes).expect("history boundary");
    let start = cadmpeg_asm::asm_header::record_stream_start(&bytes).expect("record stream");
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("generated SAB");
    let face = &records[4];
    let attribute_ref = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        face,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x0c,
    )
    .expect("face reference tokens")[0];
    bytes[attribute_ref + 1..attribute_ref + 9].copy_from_slice(&19i64.to_le_bytes());

    let mut attribute = Vec::new();
    t_subident(&mut attribute, "rgb_color");
    t_subident(&mut attribute, "st");
    t_ident(&mut attribute, "attrib");
    t_attribute_base(&mut attribute, -1, -1, 4);
    t_dbl(&mut attribute, 0.15);
    t_dbl(&mut attribute, 0.25);
    t_dbl(&mut attribute, 0.35);
    t_dbl(&mut attribute, 1.0);
    t_end(&mut attribute);
    bytes.splice(limit..limit, attribute);
    bytes
}

pub(crate) fn synthetic_geometry_with_mesh_surface_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let limit = cadmpeg_asm::asm_header::solved_record_limit(&bytes).expect("history boundary");
    let start = cadmpeg_asm::asm_header::record_stream_start(&bytes).expect("record stream");
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("generated SAB");
    let plane = records
        .iter()
        .find(|record| record.head() == "plane")
        .expect("generated plane surface");
    let mut sentinel = Vec::new();
    t_ident(&mut sentinel, "mesh_surface");
    t_end(&mut sentinel);
    bytes.splice(plane.offset..plane.offset + plane.len, sentinel);
    bytes
}

pub(crate) fn synthetic_geometry_with_attribute_smbh() -> Vec<u8> {
    synthetic_geometry_with_attribute_at(1)
}

pub(crate) fn synthetic_geometry_with_face_attribute_smbh() -> Vec<u8> {
    synthetic_geometry_with_attribute_at(4)
}

fn synthetic_geometry_with_attribute_at(owner_record_index: usize) -> Vec<u8> {
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
    let owner = &records[owner_record_index];
    let record = &mut bytes[owner.offset..owner.offset + owner.len];
    let attribute_ref = record.iter().position(|byte| *byte == 0x0c).unwrap();
    record[attribute_ref + 1..attribute_ref + 9].copy_from_slice(&19i64.to_le_bytes());

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut attribute = Vec::new();
    t_subident(&mut attribute, "ATTRIB_CUSTOM");
    t_ident(&mut attribute, "attrib");
    t_ref(&mut attribute, 20);
    push_u8_string(&mut attribute, "generic_tag_attrib_def");
    for value in [3, 3, -1] {
        t_long(&mut attribute, value);
    }
    push_u8_string(&mut attribute, "generic_tag_attrib_def ");
    t_long(&mut attribute, 3);
    if owner_record_index == 4 {
        for (selector, token, references) in [
            (1, "8", &[301, -314, 411][..]),
            (2, "-1", &[511][..]),
            (3, "42", &[][..]),
        ] {
            t_long(&mut attribute, selector);
            push_u8_string(&mut attribute, token);
            t_long(&mut attribute, 0);
            t_long(
                &mut attribute,
                i64::try_from(references.len()).expect("fixture value fits i64"),
            );
            for reference in references {
                t_long(&mut attribute, *reference);
            }
            t_long(&mut attribute, 0);
        }
    } else {
        for (kind, id, reference) in [(3, "311", 6), (4, "900", 42), (3, "322", 7)] {
            t_long(&mut attribute, kind);
            push_u8_string(&mut attribute, id);
            for value in [reference, 0, 0] {
                t_long(&mut attribute, value);
            }
        }
    }
    t_end(&mut attribute);
    t_subident(&mut attribute, "ATTRIB_CUSTOM");
    t_ident(&mut attribute, "attrib");
    t_ref(&mut attribute, -1);
    push_u8_string(&mut attribute, "Timestamp_attrib_def");
    t_long(&mut attribute, 1);
    t_dbl(&mut attribute, 1_579_392_000_000_007.0);
    t_end(&mut attribute);
    bytes.splice(delta..delta, attribute);
    bytes
}

/// One `sketch_attrib_def` payload form: the form selector the third header
/// integer carries and the members that follow it.
pub(crate) enum SketchLinkForm<'a> {
    /// Form `3`: the members as one tagged ASCII field.
    Tagged(&'a str),
    /// Form `2` or `0`: the members as integers.
    Integers(i64, &'a [i64]),
}

pub(crate) fn synthetic_geometry_with_sketch_link_smbh(form: SketchLinkForm<'_>) -> Vec<u8> {
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
    let coedge = &records[7];
    let record = &mut bytes[coedge.offset..coedge.offset + coedge.len];
    let attribute_ref = record.iter().position(|byte| *byte == 0x0c).unwrap();
    record[attribute_ref + 1..attribute_ref + 9].copy_from_slice(&19i64.to_le_bytes());

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut attribute = Vec::new();
    t_subident(&mut attribute, "ATTRIB_CUSTOM");
    t_ident(&mut attribute, "attrib");
    t_ref(&mut attribute, -1);
    push_u8_string(&mut attribute, "sketch_attrib_def");
    let (selector, members) = match form {
        SketchLinkForm::Tagged(_) => (3, &[][..]),
        SketchLinkForm::Integers(selector, members) => (selector, members),
    };
    for value in [1, 1, selector] {
        t_long(&mut attribute, value);
    }
    match form {
        SketchLinkForm::Tagged(tuple) => push_u8_string(&mut attribute, tuple),
        SketchLinkForm::Integers(..) => {
            for value in members {
                t_long(&mut attribute, *value);
            }
        }
    }
    t_end(&mut attribute);
    bytes.splice(delta..delta, attribute);
    bytes
}

pub(crate) fn synthetic_wire_body_smbh() -> Vec<u8> {
    let mut records = Vec::new();
    t_ident(&mut records, "asmheader");
    push_u8_string(&mut records, "231.6.3.65535");
    t_end(&mut records);

    t_ident(&mut records, "body");
    t_ref(&mut records, -1);
    t_long(&mut records, 1);
    t_ref(&mut records, -1);
    t_ref(&mut records, 2);
    t_ref(&mut records, -1);
    t_ref(&mut records, -1);
    t_end(&mut records);

    t_ident(&mut records, "region");
    for reference in [-1, -1, -1, -1, 3, 1] {
        t_ref(&mut records, reference);
    }
    t_end(&mut records);

    t_ident(&mut records, "shell");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    for reference in [-1, -1, -1, -1, 4, 2] {
        t_ref(&mut records, reference);
    }
    t_end(&mut records);

    t_ident(&mut records, "wire");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    for reference in [-1, -1, 5, 3, -1] {
        t_ref(&mut records, reference);
    }
    records.push(0x0b);
    t_end(&mut records);

    t_ident(&mut records, "coedge");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    for reference in [-1, 5, 5, -1, 6] {
        t_ref(&mut records, reference);
    }
    records.push(0x0b);
    t_ref(&mut records, 4);
    t_long(&mut records, 0);
    t_ref(&mut records, -1);
    t_end(&mut records);

    t_ident(&mut records, "edge");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    t_ref(&mut records, -1);
    t_ref(&mut records, 7);
    t_dbl(&mut records, 0.0);
    t_ref(&mut records, 8);
    t_dbl(&mut records, 2.0);
    t_ref(&mut records, 5);
    t_ref(&mut records, 11);
    records.push(0x0b);
    push_u8_string(&mut records, "unknown");
    t_end(&mut records);

    for (point, index_flag) in [(9, 0), (10, 1)] {
        t_ident(&mut records, "vertex");
        t_ref(&mut records, -1);
        t_long(&mut records, -1);
        t_ref(&mut records, -1);
        t_ref(&mut records, 6);
        t_long(&mut records, index_flag);
        t_ref(&mut records, point);
        t_end(&mut records);
    }
    for position in [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]] {
        t_ident(&mut records, "point");
        t_ref(&mut records, -1);
        t_long(&mut records, -1);
        t_ref(&mut records, -1);
        t_pos(&mut records, position);
        t_end(&mut records);
    }
    t_subident(&mut records, "straight");
    t_ident(&mut records, "curve");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    t_ref(&mut records, -1);
    t_pos(&mut records, [0.0, 0.0, 0.0]);
    t_vec(&mut records, [1.0, 0.0, 0.0]);
    t_end(&mut records);
    t_ident(&mut records, "delta_state");

    let mut out = smbh_header_prefix();
    out.extend_from_slice(&records);
    out
}

pub(crate) fn synthetic_free_vertex_body_smbh() -> Vec<u8> {
    let mut records = Vec::new();
    t_ident(&mut records, "asmheader");
    push_u8_string(&mut records, "231.6.3.65535");
    t_end(&mut records);

    t_ident(&mut records, "body");
    t_ref(&mut records, -1);
    t_long(&mut records, 1);
    for reference in [-1, 2, 4, -1] {
        t_ref(&mut records, reference);
    }
    t_end(&mut records);

    t_ident(&mut records, "region");
    for reference in [-1, -1, -1, -1, 3, 1] {
        t_ref(&mut records, reference);
    }
    t_end(&mut records);

    t_ident(&mut records, "shell");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    for reference in [-1, -1, -1, -1, 4, 2] {
        t_ref(&mut records, reference);
    }
    t_end(&mut records);

    t_ident(&mut records, "wire");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    for reference in [-1, -1, -1, 3, 5] {
        t_ref(&mut records, reference);
    }
    records.push(0x0b);
    t_end(&mut records);

    t_ident(&mut records, "vertex");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    t_ref(&mut records, -1);
    t_ref(&mut records, 4);
    t_long(&mut records, -1);
    t_ref(&mut records, 6);
    t_end(&mut records);

    t_ident(&mut records, "point");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    t_ref(&mut records, -1);
    t_pos(&mut records, [1.0, 2.0, 3.0]);
    t_end(&mut records);
    t_ident(&mut records, "delta_state");

    let mut out = smbh_header_prefix();
    out.extend_from_slice(&records);
    out
}

pub(crate) fn synthetic_mixed_face_wire_body_smbh() -> Vec<u8> {
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
    for (record_index, reference_ordinal) in [(1usize, 3usize), (3, 5)] {
        let record = &records[record_index];
        let offsets = cadmpeg_asm::test_support::sab::payload_token_offsets(
            &bytes,
            record,
            cadmpeg_asm::kernel_header::RefWidth::Eight,
            0x0c,
        )
        .expect("generated reference offsets");
        let offset = offsets[reference_ordinal];
        bytes[offset + 1..offset + 9].copy_from_slice(&19i64.to_le_bytes());
    }
    let updated = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    assert_eq!(updated[1].ref_at(4), Some(19));
    assert_eq!(updated[3].ref_at(6), Some(19));

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut appended = Vec::new();
    t_ident(&mut appended, "wire");
    t_ref(&mut appended, -1);
    t_long(&mut appended, -1);
    for reference in [-1, -1, 20, 3, -1] {
        t_ref(&mut appended, reference);
    }
    appended.push(0x0b);
    t_end(&mut appended);

    t_ident(&mut appended, "coedge");
    t_ref(&mut appended, -1);
    t_long(&mut appended, -1);
    for reference in [-1, 20, 20, -1, 21] {
        t_ref(&mut appended, reference);
    }
    appended.push(0x0b);
    t_ref(&mut appended, 19);
    t_long(&mut appended, 0);
    t_ref(&mut appended, -1);
    t_end(&mut appended);

    t_ident(&mut appended, "edge");
    t_ref(&mut appended, -1);
    t_long(&mut appended, -1);
    t_ref(&mut appended, -1);
    t_ref(&mut appended, 22);
    t_dbl(&mut appended, 0.0);
    t_ref(&mut appended, 23);
    t_dbl(&mut appended, 2.0);
    t_ref(&mut appended, 20);
    t_ref(&mut appended, 26);
    appended.push(0x0b);
    push_u8_string(&mut appended, "unknown");
    t_end(&mut appended);

    for (point, index_flag) in [(24, 0), (25, 1)] {
        t_ident(&mut appended, "vertex");
        t_ref(&mut appended, -1);
        t_long(&mut appended, -1);
        t_ref(&mut appended, -1);
        t_ref(&mut appended, 21);
        t_long(&mut appended, index_flag);
        t_ref(&mut appended, point);
        t_end(&mut appended);
    }
    for position in [[0.0, 0.0, 1.0], [2.0, 0.0, 1.0]] {
        t_ident(&mut appended, "point");
        t_ref(&mut appended, -1);
        t_long(&mut appended, -1);
        t_ref(&mut appended, -1);
        t_pos(&mut appended, position);
        t_end(&mut appended);
    }
    t_subident(&mut appended, "straight");
    t_ident(&mut appended, "curve");
    t_ref(&mut appended, -1);
    t_long(&mut appended, -1);
    t_ref(&mut appended, -1);
    t_pos(&mut appended, [0.0, 0.0, 1.0]);
    t_vec(&mut appended, [1.0, 0.0, 0.0]);
    t_end(&mut appended);
    bytes.splice(delta..delta, appended);
    bytes
}

pub(crate) fn synthetic_geometry_with_degenerate_curve_smbh() -> Vec<u8> {
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
    bytes[offsets[3] + 1..offsets[3] + 9].copy_from_slice(&13i64.to_le_bytes());
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let vertex = &records[14];
    let owner = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        vertex,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x0c,
    )
    .expect("generated vertex reference offsets")[2];
    bytes[owner + 1..owner + 9].copy_from_slice(&11i64.to_le_bytes());
    let endpoint = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        vertex,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x04,
    )
    .expect("generated vertex integer offsets")[1];
    bytes[endpoint + 1..endpoint + 9].copy_from_slice(&0i64.to_le_bytes());

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "degenerate_curve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    t_pos(&mut curve, [0.0, 0.0, 0.0]);
    curve.extend_from_slice(&[0x0b, 0x0b]);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

/// Two disjoint one-triangle bodies with the given native body keys. The second
/// triangle lies one centimetre above the first.
pub(crate) fn synthetic_two_body_smbh(keys: [i64; 2]) -> Vec<u8> {
    let mut r = Vec::new();
    t_ident(&mut r, "asmheader");
    push_u8_string(&mut r, "231.6.3.65535");
    t_end(&mut r);
    for (height, (first, key)) in [(1_i64, keys[0]), (19, keys[1])].into_iter().enumerate() {
        let at = |record: i64| first + record - 1;
        let height = f64::from(u32::try_from(height).unwrap());
        t_ident(&mut r, "body");
        t_ref(&mut r, -1);
        t_long(&mut r, key);
        t_ref(&mut r, -1);
        t_ref(&mut r, at(2));
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_end(&mut r);
        t_ident(&mut r, "region");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, at(3));
        t_ref(&mut r, at(1));
        t_end(&mut r);
        t_ident(&mut r, "shell");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, at(4));
        t_ref(&mut r, -1);
        t_ref(&mut r, at(2));
        t_end(&mut r);
        t_ident(&mut r, "face");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, at(5));
        t_ref(&mut r, at(3));
        t_ref(&mut r, -1);
        t_ref(&mut r, at(6));
        r.push(0x0b);
        r.push(0x0b);
        t_end(&mut r);
        t_ident(&mut r, "loop");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, at(7));
        t_ref(&mut r, at(4));
        t_end(&mut r);
        t_subident(&mut r, "plane");
        t_ident(&mut r, "surface");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_pos(&mut r, [0.0, 0.0, height]);
        t_vec(&mut r, [0.0, 0.0, 1.0]);
        t_vec(&mut r, [1.0, 0.0, 0.0]);
        r.push(0x0b);
        t_end(&mut r);
        for (next, prev, edge) in [(8, 9, 10), (9, 7, 11), (7, 8, 12)] {
            t_ident(&mut r, "coedge");
            t_ref(&mut r, -1);
            t_long(&mut r, -1);
            t_ref(&mut r, -1);
            t_ref(&mut r, at(next));
            t_ref(&mut r, at(prev));
            t_ref(&mut r, -1);
            t_ref(&mut r, at(edge));
            r.push(0x0b);
            t_ref(&mut r, at(5));
            t_long(&mut r, 0);
            t_ref(&mut r, -1);
            t_end(&mut r);
        }
        for (start, end) in [(13, 14), (14, 15), (15, 13)] {
            t_ident(&mut r, "edge");
            t_ref(&mut r, -1);
            t_long(&mut r, -1);
            t_ref(&mut r, -1);
            t_ref(&mut r, at(start));
            t_dbl(&mut r, 0.0);
            t_ref(&mut r, at(end));
            t_dbl(&mut r, 1.0);
            t_ref(&mut r, -1);
            t_ref(&mut r, -1);
            r.push(0x0b);
            push_u8_string(&mut r, "unknown");
            t_end(&mut r);
        }
        for (edge, index_flag, point) in [(10, 0, 16), (10, 1, 17), (12, 0, 18)] {
            t_ident(&mut r, "vertex");
            t_ref(&mut r, -1);
            t_long(&mut r, -1);
            t_ref(&mut r, -1);
            t_ref(&mut r, at(edge));
            t_long(&mut r, index_flag);
            t_ref(&mut r, at(point));
            t_end(&mut r);
        }
        for point in [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]] {
            t_ident(&mut r, "point");
            t_ref(&mut r, -1);
            t_long(&mut r, -1);
            t_ref(&mut r, -1);
            t_pos(&mut r, [point[0], point[1], height]);
            t_end(&mut r);
        }
    }
    t_ident(&mut r, "delta_state");
    let mut out = smbh_header_prefix();
    out.extend_from_slice(&r);
    out
}

/// Two triangular faces sharing one edge: face 4 rests on a plane (analytic),
/// face 5 on a `spline-surface` (undecoded → unknown-geometry carrier). The
/// shared edge 16 is used by coedge 10 (face 4, forward) and coedge 13 (face 5,
/// reversed), which must decode as mutually-referencing partners.
pub(crate) fn synthetic_mixed_smbh() -> Vec<u8> {
    let mut r = Vec::new();

    // 0: asmheader
    t_ident(&mut r, "asmheader");
    push_u8_string(&mut r, "231.6.3.65535");
    t_end(&mut r);

    // 1: body
    t_ident(&mut r, "body");
    t_ref(&mut r, -1); // 0 attrib
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, 2); // 3 first_region
    t_ref(&mut r, -1); // 4 wire
    t_ref(&mut r, -1); // 5 transform
    t_end(&mut r);

    // 2: region
    t_ident(&mut r, "region");
    t_ref(&mut r, -1);
    t_long(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, 3); // first_shell
    t_ref(&mut r, 1); // owner_body
    t_end(&mut r);

    // 3: shell (first_face = 4)
    t_ident(&mut r, "shell");
    t_ref(&mut r, -1);
    t_long(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, 4); // first_face
    t_ref(&mut r, -1);
    t_ref(&mut r, 2); // owner_region
    t_end(&mut r);

    // Face builder: next_face, first_loop, surface.
    let face = |r: &mut Vec<u8>, next: i64, first_loop: i64, surface: i64| {
        t_ident(r, "face");
        t_ref(r, -1); // 0 attrib
        t_long(r, -1); // 1 history
        t_ref(r, -1); // 2 null
        t_ref(r, next); // 3 next_face
        t_ref(r, first_loop); // 4 first_loop
        t_ref(r, 3); // 5 owner_shell
        t_ref(r, -1); // 6 null
        t_ref(r, surface); // 7 surface
        r.push(0x0b); // 8 sense forward
        r.push(0x0b); // 9 sides single
        t_end(r);
    };
    face(&mut r, 5, 6, 8); // 4: plane face
    face(&mut r, -1, 7, 9); // 5: spline face

    // Loop builder: first_coedge, owner_face.
    let lp = |r: &mut Vec<u8>, first_coedge: i64, owner_face: i64| {
        t_ident(r, "loop");
        t_ref(r, -1);
        t_long(r, -1);
        t_ref(r, -1);
        t_ref(r, -1); // next_loop
        t_ref(r, first_coedge);
        t_ref(r, owner_face);
        t_end(r);
    };
    lp(&mut r, 10, 4); // 6: loop of face 4
    lp(&mut r, 13, 5); // 7: loop of face 5

    // 8: plane-surface
    t_subident(&mut r, "plane");
    t_ident(&mut r, "surface");
    t_ref(&mut r, -1);
    t_long(&mut r, -1);
    t_ref(&mut r, -1);
    t_pos(&mut r, [0.0, 0.0, 0.0]);
    t_vec(&mut r, [0.0, 0.0, 1.0]);
    t_vec(&mut r, [1.0, 0.0, 0.0]);
    r.push(0x0b);
    t_end(&mut r);

    // 9: spline-surface (undecoded carrier; only needs to frame cleanly)
    t_subident(&mut r, "spline");
    t_ident(&mut r, "surface");
    t_ref(&mut r, -1);
    t_long(&mut r, -1);
    t_ref(&mut r, -1);
    t_dbl(&mut r, 0.0);
    r.push(0x0b);
    t_end(&mut r);

    // Coedge builder: next, prev, partner, edge, sense_reversed, owner_loop.
    let ce =
        |r: &mut Vec<u8>, next: i64, prev: i64, partner: i64, edge: i64, rev: bool, owner: i64| {
            t_ident(r, "coedge");
            t_ref(r, -1); // 0 attrib
            t_long(r, -1); // 1 history
            t_ref(r, -1); // 2 null
            t_ref(r, next); // 3 next
            t_ref(r, prev); // 4 prev
            t_ref(r, partner); // 5 partner
            t_ref(r, edge); // 6 edge
            r.push(if rev { 0x0a } else { 0x0b }); // 7 sense
            t_ref(r, owner); // 8 owner_loop
            t_long(r, 0); // 9 reserved
            t_ref(r, -1); // 10 pcurve
            t_end(r);
        };
    // Loop of face 4: 10 -> 11 -> 12 -> 10; coedge 10 partners coedge 13.
    ce(&mut r, 11, 12, 13, 16, false, 6); // 10 (shared edge, forward)
    ce(&mut r, 12, 10, -1, 17, false, 6); // 11
    ce(&mut r, 10, 11, -1, 18, false, 6); // 12
                                          // Loop of face 5: 13 -> 14 -> 15 -> 13; coedge 13 partners coedge 10.
    ce(&mut r, 14, 15, 10, 16, true, 7); // 13 (shared edge, reversed)
    ce(&mut r, 15, 13, -1, 19, false, 7); // 14
    ce(&mut r, 13, 14, -1, 20, false, 7); // 15

    // Edge builder: start_vertex, end_vertex.
    let edge = |r: &mut Vec<u8>, start: i64, end: i64| {
        t_ident(r, "edge");
        t_ref(r, -1); // 0 attrib
        t_long(r, -1); // 1 history
        t_ref(r, -1); // 2 null
        t_ref(r, start); // 3 start_vertex
        t_dbl(r, 0.0); // 4 t_start
        t_ref(r, end); // 5 end_vertex
        t_dbl(r, 1.0); // 6 t_end
        t_ref(r, -1); // 7 owner_coedge
        t_ref(r, -1); // 8 curve (none)
        r.push(0x0b); // 9 sense
        push_u8_string(r, "unknown"); // 10 continuity
        t_end(r);
    };
    edge(&mut r, 21, 22); // 16 A->B (shared)
    edge(&mut r, 22, 23); // 17 B->C
    edge(&mut r, 23, 21); // 18 C->A
    edge(&mut r, 21, 24); // 19 A->D
    edge(&mut r, 24, 22); // 20 D->B

    // Vertex builder: owning_edge, point.
    let vert = |r: &mut Vec<u8>, owning_edge: i64, index_flag: i64, point: i64| {
        t_ident(r, "vertex");
        t_ref(r, -1);
        t_long(r, -1);
        t_ref(r, -1);
        t_ref(r, owning_edge);
        t_long(r, index_flag);
        t_ref(r, point);
        t_end(r);
    };
    vert(&mut r, 16, 0, 25); // 21 A
    vert(&mut r, 16, 1, 26); // 22 B
    vert(&mut r, 17, 1, 27); // 23 C
    vert(&mut r, 19, 1, 28); // 24 D

    // Points.
    for p in [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
    ] {
        t_ident(&mut r, "point");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_pos(&mut r, p);
        t_end(&mut r);
    }

    // History boundary.
    t_ident(&mut r, "delta_state");

    let mut out = smbh_header_prefix();
    out.extend_from_slice(&r);
    out
}
