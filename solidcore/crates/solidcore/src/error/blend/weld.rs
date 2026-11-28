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
    /// A joint that is not a straight edge between two planes: only the exact planar bead is
    /// built so far; the bead on any joint (the foot blend) comes with its certificate
    /// (#18028). The failure's site names the edge.
    CurvedJoint,
    /// A bead whose toe on one face is obtuse (its second leg at most its first times the
    /// cosine of the angle between the faces, or the other way round): the exact planar
    /// bead's surface is read through the chamfer's ball, which such legs fold. The failure's
    /// site names the edge.
    ObtuseToe,
}
