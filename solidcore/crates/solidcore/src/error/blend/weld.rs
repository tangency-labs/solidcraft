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
    /// Beads that meet at a vertex need a corner piece (#17916); the failure's site names the
    /// vertex.
    BeadsMeetAtVertex,
    /// A joint that is not a straight edge between two planes for the exact planar bead
    /// ([`crate::blend::weld_bead::beads_apart`]); the bead on any joint is the foot blend's
    /// ([`crate::blend::weld_bead::bead_body`]). The failure's site names the edge.
    CurvedJoint,
    /// A tapered or rounded end of a flat bead (sagitta 0): the planar sweep that cuts those
    /// ends takes only a bead with a sagitta so far, even straight between two planes.
    EndShapeOfFlatBead,
    /// A tapered or rounded end of a bead on a joint that is not straight between two planes:
    /// only the planar sweep cuts those ends so far; the foot blend ends square.
    EndShapeOnCurvedJoint,
    /// A bead whose toe on one face is obtuse (its second leg at most its first times the
    /// cosine of the angle between the faces, or the other way round): the exact planar
    /// bead's surface is read through the chamfer's ball, which such legs fold. The failure's
    /// site names the edge.
    ObtuseToe,
}
