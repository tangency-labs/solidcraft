//! The class of a blend larger than its faces allow, by its reason.
//!
//! Upstream: none. One of the modules of the blend failure causes, re-exported from
//! [`super`].

use crate::blend::fillet_executive::NoRoomReason;
use crate::error::ErrorKind;

/// The class of a blend larger than its faces allow, for `reason`:
/// [`ErrorKind::FilletTooLarge`] (100) for a weld bead larger than its faces take, one of
/// the blend's refusal classes 101 to 124 (the cliff overflow's own refusals grouped by
/// what decides them), [`ErrorKind::TangentContinuationEnd`] (128) or
/// [`ErrorKind::OverflowKeepsNothing`] (129).
/// [`ErrorKind::CliffSubtractionLeavesNoSolid`] is the one defect among them.
#[must_use]
#[allow(
    clippy::match_same_arms,
    reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
)]
pub const fn no_room_class(reason: NoRoomReason) -> ErrorKind {
    use NoRoomReason as R;
    match reason {
        // a Join's: the round's ball does not fit its face where its contact line must cross
        // the edge between the rounds' faces
        R::BallDoesNotFit => ErrorKind::BallDoesNotFit,
        R::BallsFold => ErrorKind::BallsFold,
        R::BandsOverlap => ErrorKind::BandsOverlap,
        // a weld bead larger than its faces take: the error names the size that builds
        R::BeadCutsFace => ErrorKind::FilletTooLarge,
        R::BreaksThrough => ErrorKind::BreaksThrough,
        R::CliffBoundaryBranches => ErrorKind::CliffOverflowShape,
        R::CliffBoundaryClosed => ErrorKind::CliffOverflowShape,
        R::CliffBoundaryLeavesAlongNoSideEdge => ErrorKind::CliffOverflowShape,
        R::CliffBoundaryOnTwoFaces => ErrorKind::CliffOverflowShape,
        R::CliffClosingFaceUnbounded => ErrorKind::CliffOverflowShape,
        R::CliffEndFaceNotPlanar => ErrorKind::CliffEndFaceNotPlanar,
        R::CliffEndNotCut => ErrorKind::CliffOverflowShape,
        R::CliffLawNotHeld => ErrorKind::CliffOverflowShape,
        R::CliffSubtractionLeavesNoSolid => ErrorKind::CliffSubtractionLeavesNoSolid,
        R::ConcaveOverflow => ErrorKind::RailLeavesFace,
        R::CornersTakeWholeEdge => ErrorKind::CornersTakeWholeEdge,
        R::CreaseAcrossTangentEdge => ErrorKind::CreaseAcrossTangentEdge,
        R::CutsThePart => ErrorKind::CutsThePart,
        R::EndAtSeamVertex => ErrorKind::EndAtSeamVertex,
        // an End the round overflows: past its side edge, or over its end face whole
        R::EndOverflowsSideEdge => ErrorKind::EndOverflowsSideEdge,
        // an End the round overflows: past its side edge, or over its end face whole
        R::EndPassesOverEndFace => ErrorKind::EndOverflowsSideEdge,
        R::EndPastSideEdge => ErrorKind::EndPastSideEdge,
        R::EndRollsIntoSmoothEnd => ErrorKind::EndRollsIntoSmoothEnd,
        R::FaceConsumed => ErrorKind::FaceConsumed,
        R::JoinOffsetsDiffer => ErrorKind::JoinOffsetsDiffer,
        // a Join's: the round's ball does not fit its face where its contact line must cross
        // the edge between the rounds' faces
        R::JoinRailMissesSideEdge => ErrorKind::BallDoesNotFit,
        // a weld bead larger than its faces take: the error names the size that builds
        R::LegPastFace => ErrorKind::FilletTooLarge,
        // a weld bead larger than its faces take: the error names the size that builds
        R::NoToeAtLeg => ErrorKind::FilletTooLarge,
        R::OffsetCollapses => ErrorKind::BallOffsetCollapses,
        R::OffsetFolds => ErrorKind::ChamferOffsetFolds,
        R::OverflowKeepsNothing => ErrorKind::OverflowKeepsNothing,
        R::OverflowWithConcaveBlend => ErrorKind::CliffOverflowRefused,
        R::OverflowWithGlobalMerge => ErrorKind::CliffOverflowRefused,
        R::OverflowWithRollover => ErrorKind::CliffOverflowRefused,
        R::RailBelowResolution => ErrorKind::RailBelowResolution,
        R::RailLeavesFace => ErrorKind::RailLeavesFace,
        R::RailPastAxis => ErrorKind::RailPastAxis,
        R::RailRunsPastFaceIntoMaterial => ErrorKind::RunsIntoMaterial,
        R::RoundIntoMaterial => ErrorKind::RunsIntoMaterial,
        R::StripBelowResolution => ErrorKind::StripBelowResolution,
        R::TangentContinuationEndUnsupported => ErrorKind::TangentContinuationEnd,
    }
}
