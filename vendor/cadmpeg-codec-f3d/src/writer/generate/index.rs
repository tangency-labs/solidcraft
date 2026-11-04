// SPDX-License-Identifier: Apache-2.0
//! Borrowed native metadata indexes for source-less generation.

use std::collections::HashMap;

use crate::native::F3dNative;
use cadmpeg_asm::brep::records::{
    EdgeContinuity, EdgeOwnership, FaceSidedness, TolerantCoedgeParameters, TolerantEdgeTail,
    TolerantVertexTail, TransformHints, VertexOwnership, WireTopology,
};

pub(super) struct NativeGenerationIndex<'a> {
    pub(super) edge_continuities: HashMap<&'a str, &'a EdgeContinuity>,
    pub(super) edge_ownerships: &'a [EdgeOwnership],
    pub(super) face_sidedness: HashMap<&'a str, &'a FaceSidedness>,
    pub(super) tolerant_coedges: HashMap<&'a str, &'a TolerantCoedgeParameters>,
    pub(super) tolerant_edges: HashMap<&'a str, &'a TolerantEdgeTail>,
    pub(super) tolerant_vertices: HashMap<&'a str, &'a TolerantVertexTail>,
    pub(super) transform_hints: HashMap<&'a str, &'a TransformHints>,
    pub(super) vertex_ownerships: HashMap<&'a str, &'a VertexOwnership>,
    pub(super) wires_by_shell: HashMap<&'a str, Vec<&'a WireTopology>>,
}

impl<'a> NativeGenerationIndex<'a> {
    pub(super) fn new(native: &'a F3dNative) -> Self {
        macro_rules! first_by_id {
            ($items:expr, $field:ident) => {{
                let mut map = HashMap::new();
                for item in $items {
                    map.entry(item.$field.as_str()).or_insert(item);
                }
                map
            }};
        }
        let mut wires_by_shell: HashMap<_, Vec<_>> = HashMap::new();
        for wire in &native.wire_topologies {
            wires_by_shell
                .entry(wire.shell.as_str())
                .or_default()
                .push(wire);
        }
        Self {
            edge_continuities: first_by_id!(&native.edge_continuities, edge),
            edge_ownerships: &native.edge_ownerships,
            face_sidedness: first_by_id!(&native.face_sidedness, face),
            tolerant_coedges: first_by_id!(&native.tolerant_coedge_parameters, coedge),
            tolerant_edges: first_by_id!(&native.tolerant_edge_tails, edge),
            tolerant_vertices: first_by_id!(&native.tolerant_vertex_tails, vertex),
            transform_hints: first_by_id!(&native.transform_hints, body),
            vertex_ownerships: first_by_id!(&native.vertex_ownerships, vertex),
            wires_by_shell,
        }
    }
}
