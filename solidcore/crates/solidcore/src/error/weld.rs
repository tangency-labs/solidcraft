//! Why a weld is refused, in solidcore's terms: one enum for every weld type (`crate::weld`).
//!
//! Not upstream: SolidCraft's own (#15132, #18500, #18407). A weld is display geometry beside the
//! parts it joins; these say why the weld the user defined cannot be built as defined. Where
//! a smaller weld builds, the failure's [`Fit`](super::Fit) names it. Bad input (walls that are
//! not lines and arcs, sizes that are not positive) stays
//! [`SolidError::InvalidInput`](super::SolidError::InvalidInput).
//!
//! | Rust                                      | Status difftest compares |
//! |-------------------------------------------|--------------------------|
//! | [`WeldFailure::ConcaveFaceCrossesWall`]   | `SM_ERR` (no C++)        |
//! | [`WeldFailure::ConcaveFaceTooDeep`]       | `SM_ERR` (no C++)        |
//! | [`WeldFailure::ConvexFaceTooHigh`]        | `SM_ERR` (no C++)        |
//! | [`WeldFailure::BlendedToesOffPlane`]      | `SM_ERR` (no C++)        |
//! | [`WeldFailure::SpotPointOffFace`]         | `SM_ERR` (no C++)        |
//! | [`WeldFailure::SpotFaceNotOuter`]         | `SM_ERR` (no C++)        |
//! | [`WeldFailure::SpotSheetMissed`]          | `SM_ERR` (no C++)        |
//! | [`WeldFailure::SpotSheetCrossedAgain`]    | `SM_ERR` (no C++)        |
//! | [`WeldFailure::SpotSheetsOverlap`]        | `SM_ERR` (no C++)        |
//! | [`WeldFailure::SpotSheetsApart`]          | `SM_ERR` (no C++)        |
//! | [`WeldFailure::SpotFaceNotFlat`]          | `SM_ERR` (no C++)        |
//! | [`WeldFailure::SpotFaceNotSquare`]        | `SM_ERR` (no C++)        |
//! | [`WeldFailure::SpotPastFaceEdge`]         | `SM_ERR` (no C++)        |

use super::{ErrorKind, SM_ERR};
use crate::topology::FaceId;

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
    /// A concave face `depth` deep reaches the section's other sides (a groove wall, the other
    /// face or the bottom). The failure's fit is the depth where the face first touches one,
    /// in closed form; every depth below it less the kernel's zone tolerance builds.
    ConcaveFaceCrossesWall {
        /// Which face.
        side: WeldSide,
        /// The depth asked for.
        depth: f64,
    },
    /// A concave face `depth` deep dips past half a circle, touching nothing. The failure's
    /// fit is the half circle: half the toe distance, which builds.
    ConcaveFaceTooDeep {
        /// Which face.
        side: WeldSide,
        /// The depth asked for.
        depth: f64,
    },
    /// A convex face `height` high is past half a circle (its main arc's half sweep past a
    /// quarter turn). The failure's fit is the highest that builds: half the toe distance,
    /// plus the blend's overlap for blended toes.
    ConvexFaceTooHigh {
        /// Which face.
        side: WeldSide,
        /// The height asked for.
        height: f64,
    },
    /// Blended toes where the plates' outer faces on that side do not run along the chord
    /// between the toes (offset plates, plates of unequal thickness on that side, or no
    /// common plate normal, as in a T- or corner joint): the run-out would not be tangent to
    /// either plate.
    BlendedToesOffPlane {
        /// Which face.
        side: WeldSide,
    },
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
}

impl WeldFailure {
    /// The status the C++ reports for it (a comparison detail): `SM_ERR`; the C++ has no
    /// welds.
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::ConcaveFaceCrossesWall { .. }
            | Self::ConcaveFaceTooDeep { .. }
            | Self::ConvexFaceTooHigh { .. }
            | Self::BlendedToesOffPlane { .. }
            | Self::SpotPointOffFace
            | Self::SpotFaceNotOuter { .. }
            | Self::SpotSheetMissed { .. }
            | Self::SpotSheetCrossedAgain { .. }
            | Self::SpotSheetsOverlap { .. }
            | Self::SpotSheetsApart { .. }
            | Self::SpotFaceNotFlat { .. }
            | Self::SpotFaceNotSquare { .. }
            | Self::SpotPastFaceEdge { .. } => SM_ERR,
        }
    }

    /// Its stable [`ErrorKind`]: a size or a combination the weld does not support.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::ConcaveFaceCrossesWall { .. }
            | Self::ConcaveFaceTooDeep { .. }
            | Self::ConvexFaceTooHigh { .. }
            | Self::BlendedToesOffPlane { .. }
            | Self::SpotPointOffFace
            | Self::SpotFaceNotOuter { .. }
            | Self::SpotSheetMissed { .. }
            | Self::SpotSheetCrossedAgain { .. }
            | Self::SpotSheetsOverlap { .. }
            | Self::SpotSheetsApart { .. }
            | Self::SpotFaceNotFlat { .. }
            | Self::SpotFaceNotSquare { .. }
            | Self::SpotPastFaceEdge { .. } => ErrorKind::Unsupported,
        }
    }
}

impl core::fmt::Display for WeldFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let name = |side: WeldSide| match side {
            WeldSide::Face => "face",
            WeldSide::Second => "second face",
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
            Self::ConvexFaceTooHigh { side, height } => write!(
                f,
                "a convex {} {height} high is past half a circle",
                name(side)
            ),
            Self::BlendedToesOffPlane { side } => write!(
                f,
                "blended toes on a {} whose plates do not run along its chord",
                name(side)
            ),
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
        }
    }
}
