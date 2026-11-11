//! Why a defeature keeps a feature on its base, in solidcore's terms (#3218).
//!
//! Upstream: none. `SmFeatureExecutive::DoDefeature` leaves a feature whose base patch is not
//! built as an open hole; solidcore puts it back and names it ([`crate::blend::feature_executive::KeptFeature`]),
//! and these say why its patch is not built. Each is decided where the patch's
//! construction refuses, from what that construction found there; the error of the
//! refusing step is kept as the failure's cause. Every one is a limit of the construction
//! (the patch is built from the faces along the loop, extended, or as a planar cap), not a
//! defect of the input.
//!
//! | Rust                                         | Status difftest compares |
//! |----------------------------------------------|--------------------------|
//! | [`DefeatureFailure::LoopNotPlanar`]          | `SM_ERR` (WR-656)        |
//! | [`DefeatureFailure::ExtensionRefused`]       | `SM_ERR` (WR-656)        |
//! | [`DefeatureFailure::TangentNeighbours`]      | `SM_ERR` (WR-656)        |
//! | [`DefeatureFailure::NotEnclosed`]            | `SM_ERR` (WR-656)        |
//!
//! Deviations: not upstream. The failure's entity is the base loop edge of the hole the
//! kept feature would have left open.

use super::{ErrorKind, SM_ERR};

/// Why a feature's base patch is not built, so the feature is kept on its base.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum DefeatureFailure {
    /// A planar cap was asked for a loop whose curves no plane holds (the planar face's own
    /// refusal is the cause).
    LoopNotPlanar,
    /// A face along the loop could not be extended by `distance` (the extension's refusal
    /// is the cause).
    ExtensionRefused {
        /// The distance asked.
        distance: f64,
    },
    /// The faces along the loop, extended, meet only tangentially where the cap would need a
    /// bound: a piece of the patch on the loop is cut from its neighbour only along an edge
    /// the kernel proves smooth ([`crate::query::tangent_edge::tangent_all_along`] within the
    /// kernel's parallel angle, `ang_tol_deg`), so nothing bounds it there and it runs on to
    /// the end of its extension. A fillet ending on a tangent fillet is one.
    TangentNeighbours {
        /// The angle the smooth edge was proved within, degrees (the kernel's `ang_tol_deg`).
        within_deg: f64,
    },
    /// The faces along the loop, extended by `extended_by`, do not enclose it: a piece of the
    /// patch on the loop runs on to the end of its extension with nothing to bound it. A
    /// wall of a box (its neighbours run past it), a feature whose loops are patched one at
    /// a time from one face each, or one whose sides run past faces not along its loop.
    NotEnclosed {
        /// How far the faces were extended, when the patch was built in this call.
        extended_by: Option<f64>,
    },
}

impl DefeatureFailure {
    /// The status the C++ reports for it (a comparison detail): `SM_ERR`, the open-base
    /// refusal's.
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::LoopNotPlanar
            | Self::ExtensionRefused { .. }
            | Self::TangentNeighbours { .. }
            | Self::NotEnclosed { .. } => SM_ERR,
        }
    }

    /// Its stable [`ErrorKind`]: each is a combination the construction does not support.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::LoopNotPlanar
            | Self::ExtensionRefused { .. }
            | Self::TangentNeighbours { .. }
            | Self::NotEnclosed { .. } => ErrorKind::Unsupported,
        }
    }
}

impl core::fmt::Display for DefeatureFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::LoopNotPlanar => f.write_str("a planar cap was asked for a loop no plane holds"),
            Self::ExtensionRefused { distance } => write!(
                f,
                "a face along the loop could not be extended by {distance}"
            ),
            Self::TangentNeighbours { within_deg } => write!(
                f,
                "faces along the loop meet only tangentially (within {within_deg} degrees) where the patch needs a bound"
            ),
            Self::NotEnclosed {
                extended_by: Some(d),
            } => write!(
                f,
                "the faces along the loop, extended by {d}, do not enclose it"
            ),
            Self::NotEnclosed { extended_by: None } => {
                f.write_str("the faces along the loop, extended, do not enclose it")
            }
        }
    }
}
