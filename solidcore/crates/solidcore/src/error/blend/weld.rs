//! The weld bead gaps a blend failure names not built yet.
//!
//! Upstream: none. One of the modules of the blend failure causes, re-exported from
//! [`super`].

#[cfg(doc)]
use super::BlendFailure;

/// What a weld bead the kernel does not build yet needs ([`BlendFailure::WeldNotBuilt`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WeldGap {
    /// Beads that meet at a vertex in a way their corner is not built for yet (#17916): not two
    /// beads at an outside corner, beads at a vertex with spans or a shaped end there, faces
    /// not planar at the vertex, a wall not square to the face both beads lie on, or sections
    /// that differ at an outside corner; the failure's site names the vertex.
    BeadsMeetAtVertex,
    /// A concave bead (sagitta below 0) on a face where the bound on its sagitta is not proved
    /// to keep the arc off the face between the toe and the edge: any face but a plane and the
    /// outside of a cylinder, a cone or a sphere
    /// (`crate::blend::foot_blend::FootBlend::station_bound`). The failure's site names the
    /// edge, the face and the toe.
    ConcaveNotProved,
    /// A joint that is not a straight edge between two planes for the exact planar bead
    /// ([`crate::blend::weld_bead::beads_apart`]); the bead on any joint is the foot blend's
    /// ([`crate::blend::weld_bead::bead_body`]). The failure's site names the edge.
    CurvedJoint,
    /// A bead whose toe on one face is obtuse (its second leg at most its first times the
    /// cosine of the angle between the faces, or the other way round): the exact planar
    /// bead's surface is read through the chamfer's ball, which such legs fold. The failure's
    /// site names the edge.
    ObtuseToe,
}
