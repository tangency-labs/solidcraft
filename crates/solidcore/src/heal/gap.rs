//! Topology gap records (`SmGap` and its derived classes): the cached 3D gaps between
//! an edge use and its face, its counter-clockwise neighbour, and a vertex use and its
//! face. Only the data part is here so far.
//!
//! Upstream: `source/SMLib/inc/SmGap.h`.
//!
//! | Rust                          | Upstream                                         |
//! |-------------------------------|--------------------------------------------------|
//! | [`EdgeFaceGap`]               | `SmEdgeFaceGap` (members, `ReSet`, `IsInit`, `SetEdgeuse`) |
//! | [`EdgeFaceGap::new`]          | `SmEdgeFaceGap()` (default arguments)            |
//! | [`EdgeEdgeGap`]               | `SmEdgeEdgeGap` (members, `ReSet`, `IsInit`)     |
//! | [`EdgeEdgeGap::new`]          | `SmEdgeEdgeGap()` (default arguments)            |
//! | [`VertexFaceGap`]             | `SmVertexFaceGap` (members, `ReSet`, `IsInit`)   |
//! | [`VertexFaceGap::new`]        | `SmVertexFaceGap()` (default arguments)          |
//!
//! Deviations: CO-PORTED by unit `topology.edgeuse_p1` (the records its edge and vertex
//! uses cache), owned by unit `heal.gap`. The topology pointers are ids. The
//! constructors take no arguments: with upstream's defaults they reach the reset state
//! (gap `-SM_UNDEF_DOUBLE`, nothing set); setting a gap (`Set`, `SetEdgeEdgeGap`,
//! which evaluate the gap through `CacheGap`) belongs to `heal.gap`.

use crate::geom::vector2d::Point2;
use crate::math::UNDEF_DOUBLE;
use crate::topology::{EdgeuseId, VertexuseId};

/// A gap between an edge's curve and its face's surface (`SmEdgeFaceGap`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeFaceGap {
    /// `m_dGap3d`: the gap (`-SM_UNDEF_DOUBLE` when undefined).
    pub gap_3d: f64,
    /// `m_pEdgeuse`: the edge use connecting the edge to the face (`None`: not set).
    pub edgeuse: Option<EdgeuseId>,
    /// `m_dEdgeT`: the edge curve parameter bounding the gap.
    pub edge_t: f64,
    /// `m_sFaceUV`: the surface parameter bounding the gap.
    pub face_uv: Point2,
}

impl Default for EdgeFaceGap {
    fn default() -> Self {
        Self::new()
    }
}

impl EdgeFaceGap {
    /// `SmEdgeFaceGap()`: the reset state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            gap_3d: -UNDEF_DOUBLE,
            edgeuse: None,
            edge_t: UNDEF_DOUBLE,
            face_uv: Point2::UNDEF,
        }
    }

    /// `IsInit`: an edge use is set.
    #[must_use]
    pub const fn is_init(&self) -> bool {
        self.edgeuse.is_some()
    }

    /// `ReSet`: back to the uninitialised state.
    pub fn reset(&mut self) {
        self.gap_3d = -UNDEF_DOUBLE;
        self.edgeuse = None;
        self.edge_t = UNDEF_DOUBLE;
        self.face_uv.set_uninitialized();
    }

    /// `SetEdgeuse`: names a different edge use (a mate sharing the gap).
    pub fn set_edgeuse(&mut self, edgeuse: Option<EdgeuseId>) {
        self.edgeuse = edgeuse;
    }
}

/// A gap between an edge use's end and the next edge use's start
/// (`SmEdgeEdgeGap`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeEdgeGap {
    /// `m_dGap3d`: the gap (`-SM_UNDEF_DOUBLE` when undefined).
    pub gap_3d: f64,
    /// `m_pEdgeuse0`.
    pub edgeuse0: Option<EdgeuseId>,
    /// `m_pEdgeuse1`.
    pub edgeuse1: Option<EdgeuseId>,
    /// `m_dEdgeT0`: the first edge's curve parameter bounding the gap.
    pub edge_t0: f64,
    /// `m_dEdgeT1`: the second edge's curve parameter bounding the gap.
    pub edge_t1: f64,
}

impl Default for EdgeEdgeGap {
    fn default() -> Self {
        Self::new()
    }
}

impl EdgeEdgeGap {
    /// `SmEdgeEdgeGap()`: the reset state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            gap_3d: -UNDEF_DOUBLE,
            edgeuse0: None,
            edgeuse1: None,
            edge_t0: UNDEF_DOUBLE,
            edge_t1: UNDEF_DOUBLE,
        }
    }

    /// `IsInit`: both edge uses are set.
    #[must_use]
    pub const fn is_init(&self) -> bool {
        self.edgeuse0.is_some() && self.edgeuse1.is_some()
    }

    /// `ReSet`: back to the uninitialised state.
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

/// A gap between a vertex and the surface of a face it bounds (`SmVertexFaceGap`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VertexFaceGap {
    /// `m_dGap3d`: the gap (`-SM_UNDEF_DOUBLE` when undefined).
    pub gap_3d: f64,
    /// `m_pVertexuse`: the vertex use connecting the vertex to the face.
    pub vertexuse: Option<VertexuseId>,
    /// `m_sFaceUV`: the surface parameter bounding the gap.
    pub face_uv: Point2,
}

impl Default for VertexFaceGap {
    fn default() -> Self {
        Self::new()
    }
}

impl VertexFaceGap {
    /// `SmVertexFaceGap()`: the reset state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            gap_3d: -UNDEF_DOUBLE,
            vertexuse: None,
            face_uv: Point2::UNDEF,
        }
    }

    /// `IsInit`: a vertex use is set.
    #[must_use]
    pub const fn is_init(&self) -> bool {
        self.vertexuse.is_some()
    }

    /// `ReSet`: back to the uninitialised state.
    pub fn reset(&mut self) {
        self.gap_3d = -UNDEF_DOUBLE;
        self.vertexuse = None;
        self.face_uv.set_uninitialized();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_states() {
        let mut g = EdgeFaceGap::new();
        assert!(!g.is_init());
        g.set_edgeuse(Some(EdgeuseId::from_index(3)));
        g.gap_3d = 1.0;
        assert!(g.is_init());
        g.reset();
        assert_eq!(g, EdgeFaceGap::default());

        let mut e = EdgeEdgeGap::new();
        e.edgeuse0 = Some(EdgeuseId::from_index(0));
        assert!(!e.is_init());
        e.edgeuse1 = Some(EdgeuseId::from_index(1));
        assert!(e.is_init());
        e.reset();
        assert!(!e.is_init());

        let mut v = VertexFaceGap::new();
        v.vertexuse = Some(VertexuseId::from_index(2));
        assert!(v.is_init());
        v.reset();
        assert_eq!(v, VertexFaceGap::default());
    }
}
