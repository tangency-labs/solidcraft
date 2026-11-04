// SPDX-License-Identifier: Apache-2.0
//! Historical face, loop and edge contexts of a feature input topology.

use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::math::Point3;
use serde::Deserialize;
use serde::Serialize;

/// Stable surface-support relation from an active face candidate to the
/// topology preceding its owning feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignHistoricalFaceSupportContext {
    /// Stable slot of the active face candidate.
    pub(crate) active_face_slot: i64,
    /// Invariant stable surface-carrier slot.
    pub(crate) surface_slot: i64,
    /// Preceding face slots owning the surface carrier.
    pub(crate) preceding_face_slots: Vec<i64>,
    /// Ordered loop boundaries of the preceding carrier owners.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) preceding_face_boundaries: Vec<DesignHistoricalFaceBoundaryContext>,
    /// Preceding owners deleted or updated by the feature transition.
    pub(crate) changed_preceding_face_slots: Vec<i64>,
}

/// Historical edge-boundary context for one ordered edge-recipe prefix reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignEdgeRecipeReferenceContext {
    /// Zero-based position in the edge recipe's prefix reference sequence.
    pub(crate) reference_ordinal: u32,
    /// Referenced faces present in the owning feature's result topology.
    pub(crate) result_faces: Vec<FaceId>,
    /// `asm_face_boundaries` records of the referenced result faces in the
    /// result state, in `result_faces` order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) result_face_boundary_ids: Vec<String>,
    /// Stable result edge slots shared by the referenced-face boundaries and
    /// the primary candidate-face boundaries.
    pub(crate) result_shared_edge_slots: Vec<i64>,
    /// Referenced faces present in the immediately preceding ASM topology.
    pub(crate) preceding_faces: Vec<FaceId>,
    /// `asm_face_boundaries` records of the referenced preceding faces in the
    /// preceding state, in `preceding_faces` order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) preceding_face_boundary_ids: Vec<String>,
    /// Preceding faces uniquely owning the surface carriers of the referenced
    /// result faces.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) preceding_support_face_slots: Vec<i64>,
    /// `asm_face_boundaries` records of the uniquely matched preceding
    /// support faces in the preceding state, in `preceding_support_face_slots`
    /// order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) preceding_support_face_boundary_ids: Vec<String>,
    /// Stable edge slots shared by the referenced-face boundaries and the
    /// primary candidate-face boundaries.
    pub(crate) shared_edge_slots: Vec<i64>,
    /// Shared edge slots deleted or updated by the owning feature transition.
    pub(crate) changed_shared_edge_slots: Vec<i64>,
    /// Changed primary-boundary edges belonging to either a directly
    /// persistent referenced face or its unique preceding surface support.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) changed_reference_edge_slots: Vec<i64>,
}

/// Ordered loop topology of one face in one historical ASM state.
///
/// One record exists for each state and face slot that an edge-recipe
/// reference context names. The identity is a function of the owning state
/// and the face slot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AsmFaceBoundary {
    /// `{stream}:asm-face-boundary#{state byte offset}:{face slot}`.
    pub(crate) id: String,
    /// Owning `asm_delta_states` record.
    pub(crate) parent: String,
    /// Stable ASM face slot.
    pub(crate) face_slot: i64,
    /// Face loops in their serialized membership order.
    pub(crate) loops: Vec<DesignHistoricalFaceLoopContext>,
}

impl AsmFaceBoundary {
    /// The record identity of `face_slot` in the delta state `state_id`
    /// located at `state_byte_offset`.
    pub(crate) fn identity(
        state_id: &str,
        state_byte_offset: u64,
        face_slot: i64,
    ) -> Option<String> {
        let stream = crate::ids::native_stream(state_id)?;
        Some(format!(
            "{stream}:asm-face-boundary#{state_byte_offset:010}:{face_slot:010}"
        ))
    }

    /// The record named `id` in an arena sorted by identity.
    pub(crate) fn find<'a>(arena: &'a [Self], id: &str) -> Option<&'a Self> {
        arena
            .binary_search_by(|boundary| boundary.id.as_str().cmp(id))
            .ok()
            .and_then(|index| arena.get(index))
    }
}

/// Ordered loop topology retained for one historical face.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignHistoricalFaceBoundaryContext {
    /// Stable ASM face slot.
    pub(crate) face_slot: i64,
    /// Face loops in their serialized membership order.
    pub(crate) loops: Vec<DesignHistoricalFaceLoopContext>,
}

/// Ordered topology and available geometry of one historical face loop.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignHistoricalFaceLoopWire")]
pub(crate) struct DesignHistoricalFaceLoopContext {
    pub(crate) loop_slot: i64,
    pub(crate) boundary: DesignHistoricalLoopBoundary,
}

#[cfg(test)]
thread_local! {
    static HISTORICAL_FACE_LOOP_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignHistoricalFaceLoopContext {
    fn clone(&self) -> Self {
        #[cfg(test)]
        HISTORICAL_FACE_LOOP_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            loop_slot: self.loop_slot,
            boundary: self.boundary.clone(),
        }
    }
}

/// Complete runs of the available loop member bindings.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DesignHistoricalLoopBoundary {
    Coedges(Vec<DesignHistoricalLoopCoedge>),
    Vertices(Vec<DesignHistoricalLoopVertex>),
    Points(Vec<DesignHistoricalLoopPoint>),
    Positions(Vec<DesignHistoricalLoopPosition>),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignHistoricalLoopCoedge {
    pub(crate) coedge_slot: i64,
    pub(crate) edge_slot: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignHistoricalLoopVertex {
    pub(crate) coedge: DesignHistoricalLoopCoedge,
    pub(crate) vertex_slot: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignHistoricalLoopPoint {
    pub(crate) vertex: DesignHistoricalLoopVertex,
    pub(crate) point_slot: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignHistoricalLoopPosition {
    pub(crate) point: DesignHistoricalLoopPoint,
    pub(crate) position: Point3,
}

impl DesignHistoricalLoopBoundary {
    pub(crate) fn coedges(&self) -> impl Iterator<Item = &DesignHistoricalLoopCoedge> {
        let (mut coedges, mut vertices, mut points, mut positions): (&[_], &[_], &[_], &[_]) =
            (&[], &[], &[], &[]);
        match self {
            Self::Coedges(rows) => coedges = rows,
            Self::Vertices(rows) => vertices = rows,
            Self::Points(rows) => points = rows,
            Self::Positions(rows) => positions = rows,
        }
        coedges
            .iter()
            .chain(vertices.iter().map(|row| &row.coedge))
            .chain(points.iter().map(|row| &row.vertex.coedge))
            .chain(positions.iter().map(|row| &row.point.vertex.coedge))
    }
}

/// Ordered coedge and edge membership of one historical face loop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignHistoricalFaceLoopWire {
    /// Stable ASM loop slot.
    loop_slot: i64,
    /// Stable coedge slots in cyclic loop order.
    coedge_slots: Vec<i64>,
    /// Stable edge slots aligned one-to-one with `coedge_slots`.
    edge_slots: Vec<i64>,
    /// Stable boundary-vertex slots preceding the aligned coedges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    vertex_slots: Vec<i64>,
    /// Stable point-carrier slots aligned one-to-one with `vertex_slots`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    point_slots: Vec<i64>,
    /// Model-space positions aligned one-to-one with `point_slots`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    positions: Vec<cadmpeg_ir::math::Point3>,
}

#[derive(Clone, Copy)]
enum LoopColumnKind {
    Coedge,
    Edge,
    Vertex,
    Point,
    Position,
}

struct LoopColumn<'a> {
    boundary: &'a DesignHistoricalLoopBoundary,
    kind: LoopColumnKind,
}

impl LoopColumn<'_> {
    fn is_empty(&self) -> bool {
        match (self.kind, self.boundary) {
            (LoopColumnKind::Coedge | LoopColumnKind::Edge, boundary) => {
                boundary.coedges().next().is_none()
            }
            (
                LoopColumnKind::Vertex | LoopColumnKind::Point | LoopColumnKind::Position,
                DesignHistoricalLoopBoundary::Coedges(_),
            )
            | (
                LoopColumnKind::Point | LoopColumnKind::Position,
                DesignHistoricalLoopBoundary::Vertices(_),
            )
            | (LoopColumnKind::Position, DesignHistoricalLoopBoundary::Points(_)) => true,
            (LoopColumnKind::Vertex, DesignHistoricalLoopBoundary::Vertices(rows)) => {
                rows.is_empty()
            }
            (
                LoopColumnKind::Vertex | LoopColumnKind::Point,
                DesignHistoricalLoopBoundary::Points(rows),
            ) => rows.is_empty(),
            (
                LoopColumnKind::Vertex | LoopColumnKind::Point | LoopColumnKind::Position,
                DesignHistoricalLoopBoundary::Positions(rows),
            ) => rows.is_empty(),
        }
    }
}

impl Serialize for LoopColumn<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (self.kind, self.boundary) {
            (LoopColumnKind::Coedge, boundary) => {
                serializer.collect_seq(boundary.coedges().map(|row| row.coedge_slot))
            }
            (LoopColumnKind::Edge, boundary) => {
                serializer.collect_seq(boundary.coedges().map(|row| row.edge_slot))
            }
            (
                LoopColumnKind::Vertex | LoopColumnKind::Point | LoopColumnKind::Position,
                DesignHistoricalLoopBoundary::Coedges(_),
            )
            | (
                LoopColumnKind::Point | LoopColumnKind::Position,
                DesignHistoricalLoopBoundary::Vertices(_),
            )
            | (LoopColumnKind::Position, DesignHistoricalLoopBoundary::Points(_)) => {
                serializer.collect_seq(std::iter::empty::<i64>())
            }
            (LoopColumnKind::Vertex, DesignHistoricalLoopBoundary::Vertices(rows)) => {
                serializer.collect_seq(rows.iter().map(|row| row.vertex_slot))
            }
            (LoopColumnKind::Vertex, DesignHistoricalLoopBoundary::Points(rows)) => {
                serializer.collect_seq(rows.iter().map(|row| row.vertex.vertex_slot))
            }
            (LoopColumnKind::Point, DesignHistoricalLoopBoundary::Points(rows)) => {
                serializer.collect_seq(rows.iter().map(|row| row.point_slot))
            }
            (LoopColumnKind::Vertex, DesignHistoricalLoopBoundary::Positions(rows)) => {
                serializer.collect_seq(rows.iter().map(|row| row.point.vertex.vertex_slot))
            }
            (LoopColumnKind::Point, DesignHistoricalLoopBoundary::Positions(rows)) => {
                serializer.collect_seq(rows.iter().map(|row| row.point.point_slot))
            }
            (LoopColumnKind::Position, DesignHistoricalLoopBoundary::Positions(rows)) => {
                serializer.collect_seq(rows.iter().map(|row| row.position))
            }
        }
    }
}

#[derive(Serialize)]
struct DesignHistoricalFaceLoopWireRef<'a> {
    loop_slot: i64,
    coedge_slots: LoopColumn<'a>,
    edge_slots: LoopColumn<'a>,
    #[serde(skip_serializing_if = "LoopColumn::is_empty")]
    vertex_slots: LoopColumn<'a>,
    #[serde(skip_serializing_if = "LoopColumn::is_empty")]
    point_slots: LoopColumn<'a>,
    #[serde(skip_serializing_if = "LoopColumn::is_empty")]
    positions: LoopColumn<'a>,
}

impl Serialize for DesignHistoricalFaceLoopContext {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let column = |kind| LoopColumn {
            boundary: &self.boundary,
            kind,
        };
        DesignHistoricalFaceLoopWireRef {
            loop_slot: self.loop_slot,
            coedge_slots: column(LoopColumnKind::Coedge),
            edge_slots: column(LoopColumnKind::Edge),
            vertex_slots: column(LoopColumnKind::Vertex),
            point_slots: column(LoopColumnKind::Point),
            positions: column(LoopColumnKind::Position),
        }
        .serialize(serializer)
    }
}

impl TryFrom<DesignHistoricalFaceLoopWire> for DesignHistoricalFaceLoopContext {
    type Error = String;

    fn try_from(wire: DesignHistoricalFaceLoopWire) -> Result<Self, Self::Error> {
        let count = wire.coedge_slots.len();
        if wire.edge_slots.len() != count {
            return Err("coedge_slots and edge_slots must have equal lengths".into());
        }
        if !wire.vertex_slots.is_empty() && wire.vertex_slots.len() != count {
            return Err("vertex_slots must be empty or match coedge_slots".into());
        }
        if !wire.point_slots.is_empty() && wire.point_slots.len() != wire.vertex_slots.len() {
            return Err("point_slots must be empty or match vertex_slots".into());
        }
        if !wire.positions.is_empty() && wire.positions.len() != wire.point_slots.len() {
            return Err("positions must be empty or match point_slots".into());
        }
        let coedges =
            wire.coedge_slots
                .into_iter()
                .zip(wire.edge_slots)
                .map(|(coedge_slot, edge_slot)| DesignHistoricalLoopCoedge {
                    coedge_slot,
                    edge_slot,
                });
        let boundary = if wire.vertex_slots.is_empty() {
            DesignHistoricalLoopBoundary::Coedges(coedges.collect())
        } else {
            let vertices = coedges.zip(wire.vertex_slots).map(|(coedge, vertex_slot)| {
                DesignHistoricalLoopVertex {
                    coedge,
                    vertex_slot,
                }
            });
            if wire.point_slots.is_empty() {
                DesignHistoricalLoopBoundary::Vertices(vertices.collect())
            } else {
                let points = vertices
                    .zip(wire.point_slots)
                    .map(|(vertex, point_slot)| DesignHistoricalLoopPoint { vertex, point_slot });
                if wire.positions.is_empty() {
                    DesignHistoricalLoopBoundary::Points(points.collect())
                } else {
                    DesignHistoricalLoopBoundary::Positions(
                        points
                            .zip(wire.positions)
                            .map(|(point, position)| DesignHistoricalLoopPosition {
                                point,
                                position,
                            })
                            .collect(),
                    )
                }
            }
        };
        Ok(Self {
            loop_slot: wire.loop_slot,
            boundary,
        })
    }
}

#[cfg(test)]
impl From<DesignHistoricalFaceLoopContext> for DesignHistoricalFaceLoopWire {
    fn from(context: DesignHistoricalFaceLoopContext) -> Self {
        let mut wire = Self {
            loop_slot: context.loop_slot,
            coedge_slots: Vec::new(),
            edge_slots: Vec::new(),
            vertex_slots: Vec::new(),
            point_slots: Vec::new(),
            positions: Vec::new(),
        };
        match context.boundary {
            DesignHistoricalLoopBoundary::Coedges(rows) => {
                for row in rows {
                    wire.coedge_slots.push(row.coedge_slot);
                    wire.edge_slots.push(row.edge_slot);
                }
            }
            DesignHistoricalLoopBoundary::Vertices(rows) => {
                for row in rows {
                    wire.coedge_slots.push(row.coedge.coedge_slot);
                    wire.edge_slots.push(row.coedge.edge_slot);
                    wire.vertex_slots.push(row.vertex_slot);
                }
            }
            DesignHistoricalLoopBoundary::Points(rows) => {
                for row in rows {
                    wire.coedge_slots.push(row.vertex.coedge.coedge_slot);
                    wire.edge_slots.push(row.vertex.coedge.edge_slot);
                    wire.vertex_slots.push(row.vertex.vertex_slot);
                    wire.point_slots.push(row.point_slot);
                }
            }
            DesignHistoricalLoopBoundary::Positions(rows) => {
                for row in rows {
                    wire.coedge_slots.push(row.point.vertex.coedge.coedge_slot);
                    wire.edge_slots.push(row.point.vertex.coedge.edge_slot);
                    wire.vertex_slots.push(row.point.vertex.vertex_slot);
                    wire.point_slots.push(row.point.point_slot);
                    wire.positions.push(row.position);
                }
            }
        }
        wire
    }
}

/// Historical topology surrounding one candidate edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignHistoricalEdgeContext {
    /// Stable ASM edge slot.
    pub(crate) edge_slot: i64,
    /// Incident coedge uses in stable coedge-slot order.
    pub(crate) incident_loops: Vec<DesignHistoricalEdgeLoopContext>,
}

/// One historical coedge use of a candidate edge and its ordered loop neighbors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignHistoricalEdgeLoopContext {
    /// Stable ASM coedge slot using the candidate edge.
    pub(crate) coedge_slot: i64,
    /// Stable ASM owner-loop slot.
    pub(crate) loop_slot: i64,
    /// Stable ASM owner-face slot.
    pub(crate) face_slot: i64,
    /// Number of coedges in the owner loop.
    pub(crate) boundary_edge_count: u32,
    /// Zero-based position of this coedge in the owner loop's ordered membership.
    pub(crate) coedge_ordinal: u32,
    /// Stable edge slot used by the preceding coedge.
    pub(crate) previous_edge_slot: i64,
    /// Stable edge slot used by the following coedge.
    pub(crate) next_edge_slot: i64,
}

#[cfg(test)]
mod tests;

mod identity_rewrite;
