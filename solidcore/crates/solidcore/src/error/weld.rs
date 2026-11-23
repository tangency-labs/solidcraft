//! Why a weld is refused, in solidcore's terms: one enum for every weld type (`crate::weld`).
//!
//! Not upstream: SolidCraft's own (#15132, #18407). A weld is display geometry beside the
//! parts it joins; these say why the weld the user defined cannot be built as defined. Where
//! a smaller weld builds, the failure's [`Fit`](super::Fit) names it. Bad input (sizes that are not
//! finite and positive, a point not finite) stays
//! [`SolidError::InvalidInput`](super::SolidError::InvalidInput).
//!
//! | Rust                                      | Status difftest compares |
//! |-------------------------------------------|--------------------------|
//! | [`WeldFailure::SpotPointOffFace`]         | `SM_ERR` (no C++)        |
//! | [`WeldFailure::SpotFaceNotOuter`]         | `SM_ERR` (no C++)        |
//! | [`WeldFailure::SpotSheetMissed`]          | `SM_ERR` (no C++)        |
//! | [`WeldFailure::SpotSheetCrossedAgain`]    | `SM_ERR` (no C++)        |
//! | [`WeldFailure::SpotSheetsOverlap`]        | `SM_ERR` (no C++)        |
//! | [`WeldFailure::SpotSheetsApart`]          | `SM_ERR` (no C++)        |
//! | [`WeldFailure::SpotFaceNotFlat`]          | `SM_ERR` (no C++)        |
//! | [`WeldFailure::SpotFaceNotSquare`]        | `SM_ERR` (no C++)        |
//! | [`WeldFailure::SpotPastFaceEdge`]         | `SM_ERR` (no C++)        |
//! | [`WeldFailure::SpotEdgeNotMeasured`]      | `SM_ERR` (no C++)        |

use super::{ErrorKind, SM_ERR};
use crate::topology::FaceId;

/// Why a weld cannot be built as defined.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum WeldFailure {
    /// A spot's point is not on the picked face: the line from it along the face's inward
    /// normal does not meet that face within the zone tolerance of the point.
    SpotPointOffFace,
    /// A spot's picked face is not an outer face of the stack: listed sheet `sheet` lies
    /// beyond it, on its outer side.
    SpotFaceNotOuter {
        /// The sheet, by its place in the list given.
        sheet: usize,
    },
    /// A spot's line misses listed sheet `sheet`.
    SpotSheetMissed {
        /// The sheet, by its place in the list given.
        sheet: usize,
    },
    /// A spot's line crosses sheet `sheet` more than once (a hem or a folded sheet).
    SpotSheetCrossedAgain {
        /// The sheet, by its place in the list given.
        sheet: usize,
    },
    /// Along a spot's line, sheet `sheet` starts before the sheet before it ends: the sheets
    /// overlap.
    SpotSheetsOverlap {
        /// The sheet, by its place in the list given.
        sheet: usize,
    },
    /// Along a spot's line, sheet `sheet` starts `gap` past the end of the sheet before it,
    /// more than the zone tolerance: the sheets do not touch.
    SpotSheetsApart {
        /// The sheet, by its place in the list given.
        sheet: usize,
        /// The gap.
        gap: f64,
    },
    /// A face a spot crosses is not flat: its surface's control net is not planar within the
    /// zone tolerance.
    SpotFaceNotFlat {
        /// The sheet, by its place in the list given.
        sheet: usize,
        /// The face.
        face: FaceId,
    },
    /// A face a spot crosses is flat but not square to the spot's line (beyond the kernel's
    /// angular tolerance).
    SpotFaceNotSquare {
        /// The sheet, by its place in the list given.
        sheet: usize,
        /// The face.
        face: FaceId,
    },
    /// A spot of `diameter` passes the boundary of a face it crosses: an edge (of the face, a
    /// hole in it, a bend or a seam) lies closer to the spot's line than half the diameter.
    /// The failure's fit is the largest diameter that fits on every face.
    SpotPastFaceEdge {
        /// The sheet, by its place in the list given.
        sheet: usize,
        /// The face.
        face: FaceId,
        /// The diameter asked for.
        diameter: f64,
    },
    /// The nearest point on an edge of a face a spot crosses was not found, so how far the
    /// edge lies from the spot's line is unknown and the footprint cannot be checked. No fit:
    /// nothing measured one.
    SpotEdgeNotMeasured {
        /// The sheet, by its place in the list given.
        sheet: usize,
        /// The face.
        face: FaceId,
    },
}

impl WeldFailure {
    /// The status the C++ reports for it (a comparison detail): `SM_ERR`; the C++ has no
    /// welds.
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::SpotPointOffFace
            | Self::SpotFaceNotOuter { .. }
            | Self::SpotSheetMissed { .. }
            | Self::SpotSheetCrossedAgain { .. }
            | Self::SpotSheetsOverlap { .. }
            | Self::SpotSheetsApart { .. }
            | Self::SpotFaceNotFlat { .. }
            | Self::SpotFaceNotSquare { .. }
            | Self::SpotPastFaceEdge { .. }
            | Self::SpotEdgeNotMeasured { .. } => SM_ERR,
        }
    }

    /// Its stable [`ErrorKind`]: a size or a combination the weld does not support.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::SpotPointOffFace
            | Self::SpotFaceNotOuter { .. }
            | Self::SpotSheetMissed { .. }
            | Self::SpotSheetCrossedAgain { .. }
            | Self::SpotSheetsOverlap { .. }
            | Self::SpotSheetsApart { .. }
            | Self::SpotFaceNotFlat { .. }
            | Self::SpotFaceNotSquare { .. }
            | Self::SpotPastFaceEdge { .. }
            | Self::SpotEdgeNotMeasured { .. } => ErrorKind::Unsupported,
        }
    }
}

impl core::fmt::Display for WeldFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::SpotPointOffFace => write!(f, "the spot's point is not on the picked face"),
            Self::SpotFaceNotOuter { sheet } => write!(
                f,
                "the spot's picked face is not an outer face: sheet {sheet} lies beyond it"
            ),
            Self::SpotSheetMissed { sheet } => write!(f, "the spot's line misses sheet {sheet}"),
            Self::SpotSheetCrossedAgain { sheet } => {
                write!(f, "the spot's line crosses sheet {sheet} more than once")
            }
            Self::SpotSheetsOverlap { sheet } => write!(
                f,
                "sheet {sheet} overlaps the sheet before it along the spot's line"
            ),
            Self::SpotSheetsApart { sheet, gap } => write!(
                f,
                "sheet {sheet} lies {gap} from the sheet before it along the spot's line"
            ),
            Self::SpotFaceNotFlat { sheet, face } => {
                write!(
                    f,
                    "face {face:?} of sheet {sheet} under the spot is not flat"
                )
            }
            Self::SpotFaceNotSquare { sheet, face } => write!(
                f,
                "face {face:?} of sheet {sheet} is not square to the spot's line"
            ),
            Self::SpotPastFaceEdge {
                sheet,
                face,
                diameter,
            } => write!(
                f,
                "a spot of diameter {diameter} passes the edge of face {face:?} of sheet {sheet}"
            ),
            Self::SpotEdgeNotMeasured { sheet, face } => write!(
                f,
                "how far an edge of face {face:?} of sheet {sheet} lies from the spot was not found"
            ),
        }
    }
}
