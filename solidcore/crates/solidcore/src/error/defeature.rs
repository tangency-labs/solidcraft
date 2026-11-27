//! Why a defeature keeps a feature on its base, in solidcore's terms (#3218).
//!
//! Upstream: none. `SmFeatureExecutive::DoDefeature` leaves a feature whose base patch is not
//! built as an open hole; solidcore puts it back and names it ([`crate::blend::feature_executive::KeptFeature`]),
//! and these say why its patch is not built. Each is decided where the patch's
//! construction fails, from what that step found. A geometric refusal describes a limit
//! of the construction. Losing a face while copying the kernel's own replacement patch
//! is a defect. A failing step's error is kept as the cause.
//!
//! | Rust                                         | Status difftest compares |
//! |----------------------------------------------|--------------------------|
//! | [`DefeatureFailure::ExtensionRefused`]       | `SM_ERR` (WR-656)        |
//! | [`DefeatureFailure::ExtensionWithinTolerance`] | `SM_ERR` (WR-656)      |
//! | [`DefeatureFailure::InsufficientAnnularReach`] | no upstream counterpart |
//! | [`DefeatureFailure::LoopNotPlanar`]          | `SM_ERR` (WR-656)        |
//! | [`DefeatureFailure::NotEnclosed`]            | `SM_ERR` (WR-656)        |
//! | [`DefeatureFailure::PatchEnclosesFace`]      | no upstream counterpart  |
//! | [`DefeatureFailure::PatchFaceNotCopied`]     | no upstream counterpart  |
//! | [`DefeatureFailure::PatchIntersectsFace`]    | no upstream counterpart  |
//! | [`DefeatureFailure::TangentNeighbours`]      | `SM_ERR` (WR-656)        |
//!
//! Deviations: not upstream. The failure's entity is the base loop edge of the hole the
//! kept feature would have left open.

use super::{ErrorKind, SM_ERR};

/// Why a feature's base patch is not built, so the feature is kept on its base.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum DefeatureFailure {
    /// A face along the loop could not be extended by `distance` (the extension's refusal
    /// is the cause).
    ExtensionRefused {
        /// The distance asked.
        distance: f64,
    },
    /// The faces along the loop, extended by `extended_by`, do not enclose it, and the
    /// extension is no longer than `identity_radius`: the distance within which a vertex of
    /// the loop and a vertex of the patch are one point (their zone tolerances summed, the
    /// largest over the loop's vertices). The extended boundary is then the same point set
    /// as the face's own to the kernel, so the extension is no feature it can resolve: an
    /// extension longer than `identity_radius` is needed (argued from the identity radius,
    /// not proved for every input; not sufficient either). The refusal is named so only after
    /// the patch failed to enclose its loop.
    ExtensionWithinTolerance {
        /// How far the faces were extended.
        extended_by: f64,
        /// The identity radius at the loop's vertices: extend by more than this.
        identity_radius: f64,
    },
    /// The two extensions fall short of a continuous annular patch. Distances are
    /// lengths along the two meridians, in increasing station order on the first axis.
    InsufficientAnnularReach {
        /// The available extension at each rim.
        available: [f64; 2],
        /// A sufficient pair. On a shared support the two rims may divide the missing
        /// length differently; on distinct supports each must reach the meeting circle.
        sufficient: [f64; 2],
    },
    /// A planar cap was asked for a loop whose curves no plane holds (the planar face's own
    /// refusal is the cause).
    LoopNotPlanar,
    /// The faces along the loop, extended by `extended_by`, do not enclose it: a piece of the
    /// patch on the loop runs on to the end of its extension with nothing to bound it. A
    /// wall of a box (its neighbours run past it), a feature whose loops are patched one at
    /// a time from one face each, or one whose sides run past faces not along its loop.
    NotEnclosed {
        /// How far the faces were extended, when the patch was built in this call.
        extended_by: Option<f64>,
    },
    /// A retained shell would cease to separate material from void after the patch.
    PatchEnclosesFace {
        /// A retained face that would cease to separate material from void.
        face: crate::topology::FaceId,
    },
    /// The kernel lost all or part of a face while copying its own replacement patch
    /// into the target B-rep.
    PatchFaceNotCopied {
        /// The face of the patch that could not be transferred.
        face: crate::topology::FaceId,
    },
    /// A face outside the feature and its supports obstructs the proposed patch.
    PatchIntersectsFace {
        /// The retained face the patch would intersect.
        face: crate::topology::FaceId,
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
}

impl DefeatureFailure {
    /// The status the C++ reports for it (a comparison detail): `SM_ERR`, the open-base
    /// refusal's.
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::ExtensionRefused { .. }
            | Self::ExtensionWithinTolerance { .. }
            | Self::InsufficientAnnularReach { .. }
            | Self::LoopNotPlanar
            | Self::NotEnclosed { .. }
            | Self::PatchEnclosesFace { .. }
            | Self::PatchFaceNotCopied { .. }
            | Self::PatchIntersectsFace { .. }
            | Self::TangentNeighbours { .. } => SM_ERR,
        }
    }

    /// Its stable [`ErrorKind`]. An incomplete transfer of the kernel's own patch is a
    /// defect. The geometric refusals describe unsupported constructions.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::ExtensionRefused { .. }
            | Self::ExtensionWithinTolerance { .. }
            | Self::InsufficientAnnularReach { .. }
            | Self::LoopNotPlanar
            | Self::NotEnclosed { .. }
            | Self::PatchEnclosesFace { .. }
            | Self::PatchIntersectsFace { .. }
            | Self::TangentNeighbours { .. } => ErrorKind::Unsupported,
            Self::PatchFaceNotCopied { .. } => ErrorKind::StageFailed,
        }
    }
}

impl core::fmt::Display for DefeatureFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::LoopNotPlanar => f.write_str("a planar cap was asked for a loop no plane holds"),
            Self::InsufficientAnnularReach { available: [a, b], sufficient: [x, y] } => write!(
                f, "the circular rim extensions of {a} and {b} fall short. Extending them by {x} and {y} closes the gap"
            ),
            Self::PatchEnclosesFace { face } => write!(
                f, "the circular patch would enclose retained face {}", face.index()
            ),
            Self::PatchIntersectsFace { face } => write!(
                f, "the circular patch intersects retained face {}", face.index()
            ),
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
            Self::PatchFaceNotCopied { face } => {
                write!(f, "the repair could not copy patch face {}", face.index())
            }
            Self::ExtensionWithinTolerance {
                extended_by,
                identity_radius,
            } => write!(
                f,
                "the faces along the loop, extended by {extended_by}, do not enclose it: the \
                 extension is within {identity_radius}, where the loop's vertices are one point \
                 with the patch's; extend by more than {identity_radius}"
            ),
        }
    }
}
