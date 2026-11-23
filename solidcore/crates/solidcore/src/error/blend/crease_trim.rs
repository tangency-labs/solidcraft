//! Why a blend trimmed at its creases is not built.
//!
//! Upstream: none. One of the modules of the blend failure causes, re-exported from
//! [`super`].

#[cfg(doc)]
use super::BlendFailure;
use crate::error::ErrorKind;

/// Why a blend trimmed at its creases is not built ([`BlendFailure::CreaseTrimNotBuilt`]),
/// each with its kind ([`CreaseTrimStop::class`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CreaseTrimStop {
    /// A corner's face between blends that add and blends that take off material, past their
    /// faces or at an open setback ([`ErrorKind::Unsupported`]; noted `CornerUnsupported`).
    CornerBetweenAddingAndTaking,
    /// A body edge moved onto a crease leaves geometry the trim does not take off
    /// ([`ErrorKind::Failed`]; noted `BadMerge`).
    MovedEdgeLeavesGeometry,
    /// The filleted edge has no tangent at an open end's vertex, so no section through the
    /// vertex ends the round there ([`ErrorKind::Degenerate`]).
    NoSectionThroughEndVertex,
    /// The trimmed sheet does not sew into the body as a manifold solid
    /// ([`ErrorKind::Failed`]; noted `BadMerge`).
    NotSewn,
    /// A piece of a blend past its faces lies on the body's boundary away from its creases
    /// ([`ErrorKind::Unsupported`]; noted `BadMerge`).
    PieceOnBoundaryAwayFromCreases,
    /// The creases and rails enclose no part of the body: the trim reaches every face
    /// ([`ErrorKind::NotFound`]; noted `BadMerge`).
    ReachesEveryFace,
    /// A round's section end lies along an edge of the body its rail does not cut
    /// ([`ErrorKind::Unsupported`]; noted `BadMerge`).
    SectionEndAlongUncutEdge,
    /// The trim takes off nothing round the blended edges ([`ErrorKind::NotFound`]; noted
    /// `BadMerge`).
    TakesOffNothing,
}

impl CreaseTrimStop {
    /// The kind of the failure.
    #[must_use]
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    pub const fn class(self) -> ErrorKind {
        match self {
            Self::CornerBetweenAddingAndTaking => ErrorKind::Unsupported,
            Self::MovedEdgeLeavesGeometry => ErrorKind::Failed,
            Self::NoSectionThroughEndVertex => ErrorKind::Degenerate,
            Self::NotSewn => ErrorKind::Failed,
            Self::PieceOnBoundaryAwayFromCreases => ErrorKind::Unsupported,
            Self::ReachesEveryFace => ErrorKind::NotFound,
            Self::SectionEndAlongUncutEdge => ErrorKind::Unsupported,
            Self::TakesOffNothing => ErrorKind::NotFound,
        }
    }

    pub(super) const fn what(self) -> &'static str {
        match self {
            Self::CornerBetweenAddingAndTaking => {
                "a corner's face between blends that add and blends that take off material is \
                 not built"
            }
            Self::MovedEdgeLeavesGeometry => {
                "a body edge moved onto a blend's crease leaves geometry the trim does not take off"
            }
            Self::NoSectionThroughEndVertex => {
                "the filleted edge has no tangent at an open end's vertex, so no section ends \
                 the round there"
            }
            Self::NotSewn => {
                "a blend past its faces does not sew into the body along its creases and rails"
            }
            Self::PieceOnBoundaryAwayFromCreases => {
                "a blend past its faces lies on the body's boundary away from its creases"
            }
            Self::ReachesEveryFace => {
                "a blend past its faces reaches every face of the body (its creases and rails \
                 enclose no part of it)"
            }
            Self::SectionEndAlongUncutEdge => {
                "a round's section end lies along an edge of the body that its rail does not cut"
            }
            Self::TakesOffNothing => "a blend past its faces takes off nothing round its edges",
        }
    }
}
