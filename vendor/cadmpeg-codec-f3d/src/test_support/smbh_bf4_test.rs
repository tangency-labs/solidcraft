// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_asm::asm_header;

use crate::test_support::smbh_header_test::bf4_header_prefix;
use crate::test_support::tokens_test::{
    push_u8_string, t_dbl, t_end, t_ident, t_pos, t_subident, t_vec,
};

/// The minimal `BinaryFile4` active model slice: the planar-face graph of
/// `synthetic_geometry_smbh` with 4-byte integer/ref fields, the ASM-227
/// `lump` head for the body-subdivision record, and one edge resting on an
/// ellipse arc whose stored range is negative.
pub(crate) fn synthetic_geometry_bf4_smbh() -> Vec<u8> {
    synthetic_geometry_bf4_smbh_with_arc_sense(0x0b)
}

pub(crate) fn synthetic_geometry_bf4_nurbs_smbh() -> Vec<u8> {
    fn tagged_i32(bytes: &mut Vec<u8>, tag: u8, value: i32) {
        bytes.push(tag);
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    let mut bytes = synthetic_geometry_bf4_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Four,
    )
    .unwrap();
    let ellipse_range = records[19].offset..records[19].offset + records[19].len;

    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    tagged_i32(&mut curve, 0x0c, -1);
    tagged_i32(&mut curve, 0x04, -1);
    tagged_i32(&mut curve, 0x0c, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "surf_surf_int_cur");
    curve.extend_from_slice(b"\x0d\x04nubs");
    tagged_i32(&mut curve, 0x04, 2);
    tagged_i32(&mut curve, 0x15, 0);
    tagged_i32(&mut curve, 0x04, 2);
    for (knot, multiplicity) in [(0.0, 2), (1.0, 2)] {
        t_dbl(&mut curve, knot);
        tagged_i32(&mut curve, 0x04, multiplicity);
    }
    for point in [[0.0, 0.0, 0.0], [0.5, 0.5, 0.0], [1.0, 0.0, 0.0]] {
        for coordinate in point {
            t_dbl(&mut curve, coordinate);
        }
    }
    t_dbl(&mut curve, 0.0005);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(ellipse_range, curve);
    bytes
}

/// `synthetic_geometry_bf4_smbh` with the arc edge's sense byte set to
/// `arc_edge_sense` (`0x0b` forward, `0x0a` reversed).
pub(crate) fn synthetic_geometry_bf4_smbh_with_arc_sense(arc_edge_sense: u8) -> Vec<u8> {
    // Width-4 writers; the remaining tag writers are width-independent.
    fn t_ref(b: &mut Vec<u8>, v: i32) {
        b.push(0x0c);
        b.extend_from_slice(&v.to_le_bytes());
    }
    fn t_long(b: &mut Vec<u8>, v: i32) {
        b.push(0x04);
        b.extend_from_slice(&v.to_le_bytes());
    }

    // Indices: 0 asmheader, 1 body, 2 lump, 3 shell, 4 face, 5 loop,
    // 6 plane, 7/8/9 coedges, 10/11/12 edges, 13/14/15 vertices,
    // 16/17/18 points, 19 ellipse.
    let mut r = Vec::new();

    // 0: asmheader
    t_ident(&mut r, "asmheader");
    push_u8_string(&mut r, "227.5.0.65535");
    t_end(&mut r);

    // 1: body  (chunk3 = first_lump)
    t_ident(&mut r, "body");
    t_ref(&mut r, -1); // 0 attrib
    t_long(&mut r, 42); // 1 native ASM body key
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, 2); // 3 first_lump
    t_ref(&mut r, -1); // 4 wire
    t_ref(&mut r, -1); // 5 transform
    t_end(&mut r);

    // 2: lump  (chunk4 = first_shell, chunk5 = owner_body)
    t_ident(&mut r, "lump");
    t_ref(&mut r, -1); // 0 next
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 null
    t_ref(&mut r, 3); // 4 first_shell
    t_ref(&mut r, 1); // 5 owner_body
    t_end(&mut r);

    // 3: shell  (chunk5 = first_face, chunk7 = owner_lump)
    t_ident(&mut r, "shell");
    t_ref(&mut r, -1); // 0 next
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 null
    t_ref(&mut r, -1); // 4 null
    t_ref(&mut r, 4); // 5 first_face
    t_ref(&mut r, -1); // 6 wire
    t_ref(&mut r, 2); // 7 owner_lump
    t_end(&mut r);

    // 4: face
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

    // 5: loop
    t_ident(&mut r, "loop");
    t_ref(&mut r, -1); // 0 attrib
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 next_loop
    t_ref(&mut r, 7); // 4 first_coedge
    t_ref(&mut r, 4); // 5 owner_face
    t_end(&mut r);

    // 6: plane-surface
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

    // 7/8/9: coedges forming the ring 7 -> 8 -> 9 -> 7
    let coedges = [(8i32, 9, 10), (9, 7, 11), (7, 8, 12)];
    for (next, prev, edge) in coedges {
        t_ident(&mut r, "coedge");
        t_ref(&mut r, -1); // 0 attrib
        t_long(&mut r, -1); // 1 history
        t_ref(&mut r, -1); // 2 null
        t_ref(&mut r, next); // 3 next
        t_ref(&mut r, prev); // 4 prev
        t_ref(&mut r, -1); // 5 partner
        t_ref(&mut r, edge); // 6 edge
        r.push(0x0b); // 7 sense = forward
        t_ref(&mut r, 5); // 8 owner_loop
        t_long(&mut r, 0); // 9 reserved
        t_ref(&mut r, -1); // 10 pcurve
        t_end(&mut r);
    }

    // 10/11/12: edges. Edge 10 rests on the ellipse arc (19) with the stored
    // ASM range [-π, -π/2]; edges 11/12 carry no curve.
    let edges = [(13i32, 14, 19), (14, 15, -1), (15, 13, -1)];
    for (start, end, curve) in edges {
        t_ident(&mut r, "edge");
        t_ref(&mut r, -1); // 0 attrib
        t_long(&mut r, -1); // 1 history
        t_ref(&mut r, -1); // 2 null
        t_ref(&mut r, start); // 3 start_vertex
        t_dbl(&mut r, -std::f64::consts::PI); // 4 t_start
        t_ref(&mut r, end); // 5 end_vertex
        t_dbl(&mut r, -std::f64::consts::FRAC_PI_2); // 6 t_end
        t_ref(&mut r, -1); // 7 owner_coedge
        t_ref(&mut r, curve); // 8 curve
        r.push(if curve >= 0 { arc_edge_sense } else { 0x0b }); // 9 sense
        push_u8_string(&mut r, "unknown"); // 10 continuity text
        t_end(&mut r);
    }

    // 13/14/15: vertices
    let verts = [(10i32, 16), (11, 17), (12, 18)];
    for (edge, point) in verts {
        t_ident(&mut r, "vertex");
        t_ref(&mut r, -1); // 0 attrib
        t_long(&mut r, -1); // 1 history
        t_ref(&mut r, -1); // 2 null
        t_ref(&mut r, edge); // 3 owning_edge
        t_long(&mut r, 0); // 4 index_flag
        t_ref(&mut r, point); // 5 point
        t_end(&mut r);
    }

    // 16/17/18: points
    let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    for p in points {
        t_ident(&mut r, "point");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_pos(&mut r, p);
        t_end(&mut r);
    }

    // 19: ellipse-curve (circle: ratio 1) carrying edge 10's arc.
    t_subident(&mut r, "ellipse");
    t_ident(&mut r, "curve");
    t_ref(&mut r, -1); // attrib
    t_long(&mut r, -1); // history
    t_ref(&mut r, -1); // null
    t_pos(&mut r, [0.5, 0.0, 0.0]); // center
    t_vec(&mut r, [0.0, 0.0, 1.0]); // normal
    t_vec(&mut r, [0.5, 0.0, 0.0]); // major axis (radius 0.5 cm)
    t_dbl(&mut r, 1.0); // ratio
    t_end(&mut r);

    // History boundary.
    t_ident(&mut r, "delta_state");

    let mut out = bf4_header_prefix(5);
    out.extend_from_slice(&r);
    out
}
