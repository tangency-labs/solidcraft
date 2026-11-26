//! Why a channel fill (`crate::prism::fill`) is refused, in solidcore's terms.
//!
//! Not upstream: SolidCraft's own (#15132, #18500). These say why a fill cannot be built as
//! asked. Where a smaller one builds, the failure's [`Fit`](super::Fit) names it. Bad input
//! (walls that are not lines and arcs, sizes that are not positive) stays
//! [`SolidError::InvalidInput`](super::SolidError::InvalidInput).
//!
//! | Rust                                      | Status difftest compares |
//! |-------------------------------------------|--------------------------|
//! | [`FillFailure::ConcaveFaceCrossesWall`]   | `SM_ERR` (no C++)        |
//! | [`FillFailure::ConcaveFaceTooDeep`]       | `SM_ERR` (no C++)        |
//! | [`FillFailure::FaceTooShallow`]           | `SM_ERR` (no C++)        |
//! | [`FillFailure::ConvexFaceTooHigh`]        | `SM_ERR` (no C++)        |
//! | [`FillFailure::BlendedToesOffChord`]      | `SM_ERR` (no C++)        |

use super::{ErrorKind, SM_ERR};

/// Which face of a fill something is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FillSide {
    /// The face (across the toes, the side the walls start from).
    Face,
    /// The second face, closing the far side between the wall ends.
    Second,
}

/// Why a fill cannot be built as asked.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum FillFailure {
    /// A concave face `depth` deep reaches the section's other sides (a wall, the other
    /// face or the bottom). The face first touches one at a depth `F`, in closed form, each
    /// side grown towards it by twice the crossing check's near-miss tolerance (100 times the
    /// section's conservative allowance). The
    /// failure's fit is the deepest that builds: `F - δ`, δ the depth margin (the larger of the
    /// section's conservative allowance, 1e-9 of its size, and the kernel's zone tolerance), where
    /// that reaches the smallest arc the profile builds; else the zone tolerance, up to which a
    /// face is built flat.
    ConcaveFaceCrossesWall {
        /// Which face.
        side: FillSide,
        /// The depth asked for.
        depth: f64,
    },
    /// A concave face `depth` deep dips past half a circle, touching nothing. The failure's
    /// fit is the half circle: half the toe distance, which builds.
    ConcaveFaceTooDeep {
        /// Which face.
        side: FillSide,
        /// The depth asked for.
        depth: f64,
    },
    /// A face (convex or concave) `size` high or deep: past the kernel's zone tolerance, so
    /// not built flat, but shallower than the smallest arc the profile builds on its chord.
    /// The failure's fit builds: that smallest arc's height for a convex face; for a concave
    /// one, the deepest that builds below what it touches if that reaches the smallest arc,
    /// else the zone tolerance (built flat).
    FaceTooShallow {
        /// Which face.
        side: FillSide,
        /// The height or depth asked for.
        size: f64,
    },
    /// A convex face `height` high is past half a circle (its main arc's half sweep past a
    /// quarter turn). The failure's fit is the highest that builds: half the toe distance,
    /// plus the blend's overlap for blended toes.
    ConvexFaceTooHigh {
        /// Which face.
        side: FillSide,
        /// The height asked for.
        height: f64,
    },
    /// Blended toes where the sides beyond the toes do not run along the chord between them
    /// (no depth normal given, or one not square to the chord): the run-out would not be
    /// tangent to them.
    BlendedToesOffChord {
        /// Which face.
        side: FillSide,
    },
}

impl FillFailure {
    /// The status the C++ reports for it (a comparison detail): `SM_ERR`; the C++ has no
    /// channel fill.
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::ConcaveFaceCrossesWall { .. }
            | Self::ConcaveFaceTooDeep { .. }
            | Self::FaceTooShallow { .. }
            | Self::ConvexFaceTooHigh { .. }
            | Self::BlendedToesOffChord { .. } => SM_ERR,
        }
    }

    /// Its stable [`ErrorKind`]: a size or a combination the fill does not support.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::ConcaveFaceCrossesWall { .. }
            | Self::ConcaveFaceTooDeep { .. }
            | Self::FaceTooShallow { .. }
            | Self::ConvexFaceTooHigh { .. }
            | Self::BlendedToesOffChord { .. } => ErrorKind::Unsupported,
        }
    }
}

impl core::fmt::Display for FillFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let name = |side: FillSide| match side {
            FillSide::Face => "face",
            FillSide::Second => "second face",
        };
        match *self {
            Self::ConcaveFaceCrossesWall { side, depth } => write!(
                f,
                "a concave {} {depth} deep reaches the section's other sides",
                name(side)
            ),
            Self::ConcaveFaceTooDeep { side, depth } => write!(
                f,
                "a concave {} {depth} deep is past half a circle",
                name(side)
            ),
            Self::FaceTooShallow { side, size } => write!(
                f,
                "a {} {size} high or deep is too shallow for an arc and past being built flat",
                name(side)
            ),
            Self::ConvexFaceTooHigh { side, height } => write!(
                f,
                "a convex {} {height} high is past half a circle",
                name(side)
            ),
            Self::BlendedToesOffChord { side } => write!(
                f,
                "blended toes on a {} whose sides beyond the toes do not run along its chord",
                name(side)
            ),
        }
    }
}
