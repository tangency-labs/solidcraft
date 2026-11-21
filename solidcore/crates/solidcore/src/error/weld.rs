//! Why a weld is refused, in solidcore's terms: one enum for every weld type (`crate::weld`).
//!
//! Not upstream: SolidCraft's own (#15132, #18500). A weld is display geometry beside the
//! parts it joins; these say why the weld the user defined cannot be built as defined. Where
//! a smaller weld builds, the failure's [`Fit`](super::Fit) names it. Bad input (walls that are
//! not lines and arcs, sizes that are not positive) stays
//! [`SolidError::InvalidInput`](super::SolidError::InvalidInput).
//!
//! | Rust                                      | Status difftest compares |
//! |-------------------------------------------|--------------------------|
//! | [`WeldFailure::ConcaveFaceCrossesWall`]   | `SM_ERR` (no C++)        |

use super::{ErrorKind, SM_ERR};

/// Which side of a weld something is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WeldSide {
    /// The face (the side the weld is made from; the side its walls start from).
    Face,
    /// The second face of a weld from both sides (a weld symbol above and below the
    /// reference line).
    Second,
}

/// Why a weld cannot be built as defined.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum WeldFailure {
    /// A concave face `depth` deep would cross a groove wall: the arc leaves a toe steeper
    /// than the wall there and runs inside the part. The failure's fit is the deepest that
    /// builds.
    ConcaveFaceCrossesWall {
        /// Which face.
        side: WeldSide,
        /// The depth asked for.
        depth: f64,
    },
}

impl WeldFailure {
    /// The status the C++ reports for it (a comparison detail): `SM_ERR`; the C++ has no
    /// welds.
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::ConcaveFaceCrossesWall { .. } => SM_ERR,
        }
    }

    /// Its stable [`ErrorKind`]: a size the weld does not support (a smaller one builds).
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::ConcaveFaceCrossesWall { .. } => ErrorKind::Unsupported,
        }
    }
}

impl core::fmt::Display for WeldFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::ConcaveFaceCrossesWall { side, depth } => write!(
                f,
                "a concave {} {depth} deep crosses a groove wall",
                match side {
                    WeldSide::Face => "face",
                    WeldSide::Second => "second face",
                }
            ),
        }
    }
}
