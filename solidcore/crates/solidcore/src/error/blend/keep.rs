//! Why a round kept round a feature standing in its band is not built.
//!
//! Upstream: none. One of the modules of the blend failure causes, re-exported from
//! [`super`].

#[cfg(doc)]
use super::BlendFailure;
use crate::error::ErrorKind;

/// Why a round kept round a feature is not built ([`BlendFailure::KeptFeatureNotBuilt`]),
/// each with its kind ([`KeepStop::class`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeepStop {
    /// The kept round's rails and creases enclose no part of the body, or all of it
    /// ([`ErrorKind::NotFound`]; noted `BadMerge`).
    RoundNotClosed,
    /// The kept round does not sew into the body as a manifold solid
    /// ([`ErrorKind::Failed`]; noted `BadMerge`).
    RoundNotSewn,
    /// A kept boss's wall continued down meets no round ([`ErrorKind::NotFound`]; noted
    /// `BadMerge`).
    WallMeetsNoRound,
    /// The faces of a boss's walls swept down are not all copied into the sheet
    /// ([`ErrorKind::Failed`]).
    WallsNotAllCopied,
    /// The walls of a kept boss are not upright to the planar face it stands on: their
    /// continuation down is not built ([`ErrorKind::Unsupported`]; noted `NotExact`).
    WallsNotUpright,
}

impl KeepStop {
    /// The kind of the failure.
    #[must_use]
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    pub const fn class(self) -> ErrorKind {
        match self {
            Self::RoundNotClosed => ErrorKind::NotFound,
            Self::RoundNotSewn => ErrorKind::Failed,
            Self::WallMeetsNoRound => ErrorKind::NotFound,
            Self::WallsNotAllCopied => ErrorKind::Failed,
            Self::WallsNotUpright => ErrorKind::Unsupported,
        }
    }

    pub(super) const fn what(self) -> &'static str {
        match self {
            Self::RoundNotClosed => "a round kept round a feature does not close on the body",
            Self::RoundNotSewn => "a round kept round a feature does not sew into the body",
            Self::WallMeetsNoRound => "a kept boss's wall continued down meets no round",
            Self::WallsNotAllCopied => "not every face of a kept boss's walls continued is copied",
            Self::WallsNotUpright => {
                "a kept boss's walls are not upright to the planar face it stands on"
            }
        }
    }
}
