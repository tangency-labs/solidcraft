// SPDX-License-Identifier: Apache-2.0
//! Keyless BREP blobs that mix sketch-region and model bodies, and the snapshot
//! Design body map that selects from them.

use cadmpeg_core::decode::u64_from_index;

use super::smbh_header_test::smbh_header_prefix;
use super::streams_test::design_metastream_with_records;
use super::tokens_test::{
    push_u8_string, t_dbl, t_end, t_ident, t_long, t_pos, t_ref, t_subident, t_vec,
};
use super::{indexed_header, lp_utf16};

/// One triangular body. Body `i` of a stream lies at height `i` centimetres.
#[derive(Clone, Copy)]
pub(crate) struct TriangleBody {
    /// Native body key; `-1` is the null key.
    pub(crate) key: i64,
    /// Edges, of three, that store a `sketch_attrib_def` record.
    pub(crate) linked_edges: usize,
    /// Whether the face stores an attribute.
    pub(crate) face_attribute: bool,
}

/// All edges linked and an unattributed face: a sketch region.
pub(crate) const SKETCH_REGION: TriangleBody = TriangleBody {
    key: -1,
    linked_edges: 3,
    face_attribute: false,
};

/// No linked edge and an attributed face: an ordinary model body.
pub(crate) const MODEL_BODY: TriangleBody = TriangleBody {
    key: -1,
    linked_edges: 0,
    face_attribute: true,
};

/// All edges linked and an attributed face: a surface made from a Sketch.
pub(crate) const SKETCH_SURFACE: TriangleBody = TriangleBody {
    key: -1,
    linked_edges: 3,
    face_attribute: true,
};

const RECORDS_PER_BODY: i64 = 18;

fn null_prefix(r: &mut Vec<u8>) {
    t_long(r, -1);
    t_ref(r, -1);
}

/// A stream of the given bodies, each one face on its own plane.
pub(crate) fn triangle_bodies_smbh(bodies: &[TriangleBody]) -> Vec<u8> {
    let mut r = Vec::new();
    t_ident(&mut r, "asmheader");
    push_u8_string(&mut r, "231.6.3.65535");
    t_end(&mut r);
    let mut next_attribute = 1 + i64::try_from(bodies.len()).unwrap() * RECORDS_PER_BODY;
    let mut attributes = Vec::new();
    for (ordinal, body) in bodies.iter().enumerate() {
        let ordinal = i64::try_from(ordinal).unwrap();
        let at = |record: i64| ordinal * RECORDS_PER_BODY + record;
        let height = f64::from(u32::try_from(ordinal).unwrap());

        t_ident(&mut r, "body");
        t_ref(&mut r, -1);
        t_long(&mut r, body.key);
        t_ref(&mut r, -1);
        t_ref(&mut r, at(2));
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_end(&mut r);

        t_ident(&mut r, "region");
        t_ref(&mut r, -1);
        null_prefix(&mut r);
        t_ref(&mut r, -1);
        t_ref(&mut r, at(3));
        t_ref(&mut r, at(1));
        t_end(&mut r);

        t_ident(&mut r, "shell");
        t_ref(&mut r, -1);
        null_prefix(&mut r);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, at(4));
        t_ref(&mut r, -1);
        t_ref(&mut r, at(2));
        t_end(&mut r);

        t_ident(&mut r, "face");
        if body.face_attribute {
            t_ref(&mut r, next_attribute);
            next_attribute += 1;
            let mut record = Vec::new();
            t_subident(&mut record, "ATTRIB_CUSTOM");
            t_ident(&mut record, "attrib");
            t_ref(&mut record, -1);
            push_u8_string(&mut record, "Timestamp_attrib_def");
            t_long(&mut record, 1);
            t_dbl(&mut record, 1_579_392_000_000_007.0);
            t_end(&mut record);
            attributes.push(record);
        } else {
            t_ref(&mut r, -1);
        }
        null_prefix(&mut r);
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
        null_prefix(&mut r);
        t_ref(&mut r, -1);
        t_ref(&mut r, at(7));
        t_ref(&mut r, at(4));
        t_end(&mut r);

        t_subident(&mut r, "plane");
        t_ident(&mut r, "surface");
        t_ref(&mut r, -1);
        null_prefix(&mut r);
        t_pos(&mut r, [0.0, 0.0, height]);
        t_vec(&mut r, [0.0, 0.0, 1.0]);
        t_vec(&mut r, [1.0, 0.0, 0.0]);
        r.push(0x0b);
        t_end(&mut r);

        for (next, prev, edge) in [(8, 9, 10), (9, 7, 11), (7, 8, 12)] {
            t_ident(&mut r, "coedge");
            t_ref(&mut r, -1);
            null_prefix(&mut r);
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

        for (edge, (start, end)) in [(13, 14), (14, 15), (15, 13)].into_iter().enumerate() {
            t_ident(&mut r, "edge");
            if edge < body.linked_edges {
                t_ref(&mut r, next_attribute);
                next_attribute += 1;
                let mut record = Vec::new();
                t_subident(&mut record, "ATTRIB_CUSTOM");
                t_ident(&mut record, "attrib");
                t_ref(&mut record, -1);
                push_u8_string(&mut record, "sketch_attrib_def");
                for value in [1, 1, 0, i64::try_from(edge).unwrap() + 1, 0, 1, 0, 0] {
                    t_long(&mut record, value);
                }
                t_end(&mut record);
                attributes.push(record);
            } else {
                t_ref(&mut r, -1);
            }
            null_prefix(&mut r);
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
            null_prefix(&mut r);
            t_ref(&mut r, at(edge));
            t_long(&mut r, index_flag);
            t_ref(&mut r, at(point));
            t_end(&mut r);
        }

        for [x, y] in [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]] {
            t_ident(&mut r, "point");
            t_ref(&mut r, -1);
            null_prefix(&mut r);
            t_pos(&mut r, [x, y, height]);
            t_end(&mut r);
        }
    }
    for record in attributes {
        r.extend_from_slice(&record);
    }
    t_ident(&mut r, "delta_state");
    let mut out = smbh_header_prefix();
    out.extend_from_slice(&r);
    out
}

/// A Design stream whose one snapshot body map names `blob` and holds `pairs`
/// of `(selector, entity)`.
pub(crate) fn snapshot_map_design(blob: &str, pairs: &[(u64, u64)]) -> (Vec<u8>, Vec<u8>) {
    const CARRIER: u64 = 900;
    const CONTAINER: u64 = 700;
    let reference = |out: &mut Vec<u8>, target: u64| {
        out.push(1);
        out.extend_from_slice(&target.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
    };
    let mut bulk = Vec::new();
    bulk.extend_from_slice(&3_u32.to_le_bytes());
    bulk.extend_from_slice(b"256");
    bulk.extend_from_slice(&CARRIER.to_le_bytes());
    bulk.extend_from_slice(&[0; 6]);
    reference(&mut bulk, CARRIER + 1);
    bulk.extend_from_slice(&u32::try_from(pairs.len()).unwrap().to_le_bytes());
    for (selector, entity) in pairs {
        bulk.extend_from_slice(&selector.to_le_bytes());
        bulk.extend_from_slice(&entity.to_le_bytes());
    }
    reference(&mut bulk, CONTAINER);
    bulk.push(0);
    lp_utf16(&mut bulk, blob);
    let mut records = vec![(CARRIER, 0)];
    let entities = pairs.iter().map(|(_, entity)| *entity).collect::<Vec<_>>();
    for entity in &entities {
        records.push((*entity, u64_from_index(bulk.len())));
        indexed_header(&mut bulk, *b"258", u32::try_from(*entity).unwrap());
    }
    let base = crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID;
    let types: [(&str, &str, u32, &str, &[u64]); 4] = [
        (
            crate::design::body::SNAPSHOT_BODY_MAP_CARRIER_TYPE_GUID,
            base,
            1,
            "Body",
            &[CARRIER],
        ),
        (
            crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
            base,
            0,
            "Body",
            &[CARRIER + 1],
        ),
        (
            crate::design::presentation::BODY_PRESENTATION_TYPE_GUID,
            crate::design::presentation::BODY_PRESENTATION_BASE_TYPE_GUID,
            15,
            "Body",
            &entities,
        ),
        (
            crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
            base,
            0,
            "Body",
            &[CONTAINER],
        ),
    ];
    let meta = design_metastream_with_records(&types, &records);
    (bulk, meta)
}
