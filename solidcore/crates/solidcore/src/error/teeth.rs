//! Why numbers give no teeth, in solidcore's terms: the failures of the tooth module
//! ([`crate::teeth`]).
//!
//! Upstream: none. The tooth module makes the outlines of splined shafts and hubs from plain
//! numbers. A number that is not finite or lies outside what is accepted is refused as
//! [`TeethFailure::OutOfRange`]; numbers that are each acceptable but together make no teeth are
//! refused as [`TeethFailure::Invalid`] with a typed reason ([`Why`]); the shaft-spline planner
//! refuses as [`TeethFailure::Refused`] naming the number to change as a [`SplineQuantity`]
//! ([`PlanRefusal`]); and a bound that could not be proved within the work allowed is refused as
//! [`TeethFailure::NotProved`]. A refusal names a number of the spline by what it is, never by a
//! key of an application: an application maps a [`SplineQuantity`] to its own parameter and
//! words the refusal for its users. The `Display` here is plain English for logs and tests.
//!
//! The enums here are exhaustive: a record that mirrors them field for field
//! (`solidcore-record`'s shaft spline) is checked against every variant when it is converted.
//! Each records, for difftest only, the status the C++ would report (crate-private
//! `comparison_status`, read through [`crate::SolidError::sm_status`]).
//!
//! | Rust                           | Status difftest compares |
//! |--------------------------------|--------------------------|
//! | [`TeethFailure::OutOfRange`]   | `SM_ERR`                 |
//! | [`TeethFailure::Invalid`]      | `SM_ERR`                 |
//! | [`TeethFailure::Refused`]      | `SM_ERR`                 |
//! | [`TeethFailure::NotProved`]    | `SM_ERR`                 |
//!
//! Deviations: not upstream. A failure says in plain words what to change; no failure names an
//! entity. The failure's `operation` path says which public function of `crate::teeth` refused.

use super::{ErrorKind, Failure, Reason, SM_ERR};

/// What [`TeethFailure::NotProved`] names when the root fillets of a tooth could not be proved
/// apart (step 3). Matched by the shaft-spline planner to find the number to change.
pub const FILLET_SEPARATION: &str = "the separation of the root fillets";
/// What [`TeethFailure::NotProved`] names when a chord of a sampled curve could not be proved
/// within the tolerance (steps 4 and 5).
pub const CHORD: &str = "the chord error of a curve";
/// What [`TeethFailure::NotProved`] names when a fitted spline's distance from its source could
/// not be proved within the work limit (step 6).
pub const FIT_DISTANCE: &str = "the distance of a fitted spline from its source";

/// Why a set of numbers gives no teeth.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TeethFailure {
    /// A number is not finite or lies outside what is accepted: above `above` and at most
    /// `up_to`.
    OutOfRange {
        /// Which number, in the tooth modules' words (for example "pressure angle").
        what: &'static str,
        /// What was given.
        value: f64,
        /// It must be above this.
        above: f64,
        /// It must be at most this.
        up_to: f64,
    },
    /// Each number is acceptable, but together they do not make teeth: why, as a typed reason.
    Invalid(Why),
    /// The shaft-spline planner's refusal, or one of a tooth profile: the number to change and
    /// why (`crate::teeth::shaft_spline`, `crate::teeth::section`).
    Refused(PlanRefusal),
    /// A bound could not be proved within the work allowed.
    NotProved {
        /// What could not be proved: [`FILLET_SEPARATION`], [`CHORD`] or [`FIT_DISTANCE`].
        what: &'static str,
        /// The bound that had to be met (a tolerance; 0 for a separation that must be
        /// positive).
        asked: f64,
        /// The best bound proved when the work ran out (infinite where none was).
        reached: f64,
    },
}

/// The reasons why numbers that are each acceptable make no teeth: one variant per sentence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Why {
    /// The knots and control points given are not a B-spline.
    NotASpline,
    /// The outline to fit was made coarser than a fortieth of the fit's tolerance.
    SourceTooCoarse,
    /// The tolerance is below what numbers of the outline's size can resolve.
    ToleranceTooFineForSize,
    /// No spline the fit makes comes within the tolerance.
    SplineNotWithin,
    /// The fitted spline's numbers would not be finite.
    FittedNotFinite,
    /// A curve of the outline to fit has no length.
    CurveHasNoLength,
    /// The fit's equations have no single solution.
    FitNotSolvable,
    /// The fit's system is not square or its sides do not agree.
    SystemNotSquare,
    /// Keeping the tolerance would need more control points than allowed.
    TooManyControlPoints,
    /// A curve of the outline has too few or too many points to fit.
    CurvePointCount,
    /// A run of pieces cannot be held as one piece within the tolerance.
    RunNotOnePiece,
    /// The pieces of a run do not join.
    RunNotJoined,
    /// A run has no pieces.
    RunEmpty,
    /// A run would not be finite.
    RunNotFinite,
    /// A run is too long to be held in points.
    RunTooLong,
    /// The root fillet of an external tooth does not meet its flank.
    FilletMissesFlank,
    /// The tooth would be thinner than nothing or as wide as the pitch.
    ToothThickness,
    /// The root does not lie below the pitch circle by more than the tool's tip radius.
    RootAboveTipRound,
    /// The root circle would be inside the axis.
    RootInsideAxis,
    /// The tool's tip radius is so large that its two rounds overlap.
    TipRoundsOverlap,
    /// The undercut would cut the tooth through at its root.
    CutThrough,
    /// The root fillets of neighbouring teeth would cross.
    FilletsCross,
    /// The tip circle is at or below the start of the flank.
    NoFlankLeft,
    /// The teeth would come to a point below the tip circle.
    PointedBelowTip,
    /// A hub's spaces would be as wide as the pitch.
    SpacesAsWideAsPitch,
    /// A hub's tips would be below the base circle.
    TipsBelowBase,
    /// A hub's roots do not lie outside its tips by more than the fillet radius.
    RootsInsideRound,
    /// A hub's spaces would meet at the tips.
    SpacesMeetAtTips,
    /// A hub's root rounds are so large that they overlap.
    RootRoundsOverlap,
    /// The hub's teeth would reach into the shaft's root fillet.
    HubReachesFillet,
    /// The hub's root rounds would reach the shaft's tip corners.
    HubRoundsReachTips,
    /// An angle is not a number.
    AngleNotANumber,
    /// No corner of the teeth lies on the smallest circle.
    NoCornerOnSmallest,
    /// No corner of the teeth lies on the largest circle.
    NoCornerOnLargest,
    /// The angle of a master spline is outside -360 to 360 degrees.
    MasterAngleRange,
    /// The two outlines are not a shaft and its hub on one axis.
    NotAShaftAndHub,
    /// The hub's start is not a number.
    HubStartNotANumber,
    /// A body's outline is not finite.
    BodyNotFinite,
    /// A body's outline crosses itself.
    BodyCrossesItself,
    /// A part is not at a finite place.
    PartNotFinite,
    /// Two outlines are too fine or too intricate to be measured against each other in one go.
    MeasureTooIntricate,
    /// The tolerance is so fine that a curve would need too many points.
    CurveTooFine,
    /// The outline is too intricate to check that it does not cross itself.
    CrossingCheckTooIntricate,
    /// The outline would not be finite.
    OutlineNotFinite,
    /// The pieces of the tooth would not join into a closed outline.
    PiecesDoNotJoin,
    /// The tolerance is so fine that the outline would need too many points.
    OutlineTooFine,
    /// The tolerance is so coarse that the outline would have fewer than three points.
    TooCoarse,
    /// The outline does not run counter-clockwise around an area.
    NotCounterClockwise,
    /// The outline would cross itself.
    CrossesItself,
    /// No top of a tooth lies near the angle asked for.
    NoTopNearAngle,
    /// The tops of two teeth meet, with no space between them.
    TopsMeet,
    /// The angle of the tooth is not a number.
    ToothAngleNotANumber,
    /// The outlines are too fine, or the reach too small, to be turned through so many steps.
    TooMuchTurning,
    /// The first part's axis is not at a finite place.
    FirstAxisNotFinite,
    /// The second part would not be at a finite place.
    SecondNotFinite,
    /// Nothing stops the one part turning on the other.
    TurnsFreely,
    /// A straight flank does not cross both circles.
    FlankMissesCircles,
    /// The chamfer is too large for the tooth.
    ChamferTooLarge,
    /// The round at the major circle is too large for the tooth.
    MajorRoundTooLarge,
    /// The round at the minor circle is too large for the tooth.
    MinorRoundTooLarge,
    /// The corners of a shaft's tooth would meet.
    TipCornersMeet,
    /// Neighbouring teeth of a shaft would meet at the root.
    TeethMeetAtRoot,
    /// The hub's root rounds would meet.
    HubRootRoundsMeet,
    /// The hub's teeth would be too narrow.
    HubTeethTooNarrow,
    /// The corners leave no straight flank between them.
    NoStraightFlank,
    /// The hub's root rounds would come closer to the shaft's tips than the smallest clearance.
    HubRootRoundsTooClose,
    /// The hub's tips would come closer to the shaft's root rounds than the smallest clearance.
    HubTipsTooClose,
    /// The minor diameter is not smaller than the major diameter.
    MinorNotSmaller,
    /// The minor clearance would take the hub's teeth out beyond the shaft's tips.
    MinorClearanceTooLarge,
    /// The flanks are parallel.
    FlanksParallel,
    /// The flanks are so nearly parallel that where they meet cannot be said.
    FlanksNearlyParallel,
}

impl Why {
    /// Every reason, once, in the order of the enum (for tests and tables).
    pub const ALL: &'static [Why] = &[
        Why::NotASpline,
        Why::SourceTooCoarse,
        Why::ToleranceTooFineForSize,
        Why::SplineNotWithin,
        Why::FittedNotFinite,
        Why::CurveHasNoLength,
        Why::FitNotSolvable,
        Why::SystemNotSquare,
        Why::TooManyControlPoints,
        Why::CurvePointCount,
        Why::RunNotOnePiece,
        Why::RunNotJoined,
        Why::RunEmpty,
        Why::RunNotFinite,
        Why::RunTooLong,
        Why::FilletMissesFlank,
        Why::ToothThickness,
        Why::RootAboveTipRound,
        Why::RootInsideAxis,
        Why::TipRoundsOverlap,
        Why::CutThrough,
        Why::FilletsCross,
        Why::NoFlankLeft,
        Why::PointedBelowTip,
        Why::SpacesAsWideAsPitch,
        Why::TipsBelowBase,
        Why::RootsInsideRound,
        Why::SpacesMeetAtTips,
        Why::RootRoundsOverlap,
        Why::HubReachesFillet,
        Why::HubRoundsReachTips,
        Why::AngleNotANumber,
        Why::NoCornerOnSmallest,
        Why::NoCornerOnLargest,
        Why::MasterAngleRange,
        Why::NotAShaftAndHub,
        Why::HubStartNotANumber,
        Why::BodyNotFinite,
        Why::BodyCrossesItself,
        Why::PartNotFinite,
        Why::MeasureTooIntricate,
        Why::CurveTooFine,
        Why::CrossingCheckTooIntricate,
        Why::OutlineNotFinite,
        Why::PiecesDoNotJoin,
        Why::OutlineTooFine,
        Why::TooCoarse,
        Why::NotCounterClockwise,
        Why::CrossesItself,
        Why::NoTopNearAngle,
        Why::TopsMeet,
        Why::ToothAngleNotANumber,
        Why::TooMuchTurning,
        Why::FirstAxisNotFinite,
        Why::SecondNotFinite,
        Why::TurnsFreely,
        Why::FlankMissesCircles,
        Why::ChamferTooLarge,
        Why::MajorRoundTooLarge,
        Why::MinorRoundTooLarge,
        Why::TipCornersMeet,
        Why::TeethMeetAtRoot,
        Why::HubRootRoundsMeet,
        Why::HubTeethTooNarrow,
        Why::NoStraightFlank,
        Why::HubRootRoundsTooClose,
        Why::HubTipsTooClose,
        Why::MinorNotSmaller,
        Why::MinorClearanceTooLarge,
        Why::FlanksParallel,
        Why::FlanksNearlyParallel,
    ];

    /// The reason in plain words.
    #[must_use]
    pub const fn words(self) -> &'static str {
        match self {
            Self::NotASpline => "the knots and control points do not describe a spline",
            Self::SourceTooCoarse => {
                "the outline must be made at a fortieth of the tolerance asked of the splines, \
                 or finer"
            }
            Self::ToleranceTooFineForSize => {
                "the tolerance is too fine for numbers of the outline's size"
            }
            Self::SplineNotWithin => "a spline could not be brought within the tolerance",
            Self::FittedNotFinite => "the fitted spline would not be finite",
            Self::CurveHasNoLength => "a curve of the outline has no length",
            Self::FitNotSolvable => "the points do not fix the curve: the fit cannot be solved",
            Self::SystemNotSquare => "the system is not square or its sides do not agree",
            Self::TooManyControlPoints => {
                "a curve would need more control points than allowed to keep the tolerance"
            }
            Self::CurvePointCount => "a curve of the outline has too few or too many points",
            Self::RunNotOnePiece => "the run cannot be held as one piece within the tolerance",
            Self::RunNotJoined => "the pieces of the run do not join",
            Self::RunEmpty => "the run has no pieces",
            Self::RunNotFinite => "the run would not be finite",
            Self::RunTooLong => "the run is too long to be held in points",
            Self::FilletMissesFlank => "the root fillet and the flank would not meet",
            Self::ToothThickness => {
                "the tooth would be thinner than nothing or as wide as the pitch"
            }
            Self::RootAboveTipRound => {
                "the root must lie below the pitch circle by more than the tool's tip radius"
            }
            Self::RootInsideAxis => "the root circle would be inside the axis",
            Self::TipRoundsOverlap => {
                "the tool's tip radius is too large: its two rounds would overlap"
            }
            Self::CutThrough => {
                "the undercut would cut the tooth through at its root: its two root fillets \
                 would meet"
            }
            Self::FilletsCross => "the root fillets of neighbouring teeth would cross",
            Self::NoFlankLeft => {
                "the tip circle is at or below the start of the flank: no flank is left"
            }
            Self::PointedBelowTip => "the teeth would come to a point below the tip circle",
            Self::SpacesAsWideAsPitch => {
                "the spaces would be as wide as the pitch: no tooth is left"
            }
            Self::TipsBelowBase => "the tips of the teeth would be below the base circle",
            Self::RootsInsideRound => {
                "the roots must lie outside the tips by more than the fillet radius"
            }
            Self::SpacesMeetAtTips => "the spaces would meet at the tips: no tooth is left",
            Self::RootRoundsOverlap => "the root rounds are too large: they would overlap",
            Self::HubReachesFillet => "the hub's teeth would reach into the shaft's root fillet",
            Self::HubRoundsReachTips => {
                "the hub's root rounds would reach the shaft's tip corners: the rounds are too \
                 large for the tip clearance"
            }
            Self::AngleNotANumber => "the angle is not a number",
            Self::NoCornerOnSmallest => {
                "no corner of the teeth lies on the smallest circle: no tooth or space can be \
                 closed there"
            }
            Self::NoCornerOnLargest => {
                "no corner of the teeth lies on the largest circle: the tops of the teeth and \
                 the bottoms of the spaces need a flat part to join"
            }
            Self::MasterAngleRange => {
                "the angle of the master spline must be from -360 to 360 degrees"
            }
            Self::NotAShaftAndHub => {
                "a master spline needs a shaft and its hub: the same count of teeth on one axis, \
                 the hub around the shaft and turning with it"
            }
            Self::HubStartNotANumber => "the hub's start is not a number",
            Self::BodyNotFinite => "the outline is not finite",
            Self::BodyCrossesItself => "the outline crosses itself",
            Self::PartNotFinite => "a part is not at a finite place",
            Self::MeasureTooIntricate => {
                "the outlines are too fine or too intricate to be measured against each other \
                 in one go"
            }
            Self::CurveTooFine => "the tolerance is too fine: a curve would need too many points",
            Self::CrossingCheckTooIntricate => {
                "the outline is too intricate to check that it does not cross itself"
            }
            Self::OutlineNotFinite => "the outline would not be finite",
            Self::PiecesDoNotJoin => "the pieces of the tooth would not join into a closed outline",
            Self::OutlineTooFine => {
                "the tolerance is too fine: the outline would need too many points"
            }
            Self::TooCoarse => {
                "the tolerance is too coarse: the outline would have fewer than three points"
            }
            Self::NotCounterClockwise => "the outline must run counter-clockwise around an area",
            Self::CrossesItself => "the outline would cross itself",
            Self::NoTopNearAngle => "no top of a tooth lies near the angle asked for",
            Self::TopsMeet => "the tops of two teeth meet: there is no space between them",
            Self::ToothAngleNotANumber => "the angle of the tooth is not a number",
            Self::TooMuchTurning => {
                "the outlines are too fine, or the reach too small, to be turned through so many \
                 steps in one go"
            }
            Self::FirstAxisNotFinite => "the first part's axis is not at a finite place",
            Self::SecondNotFinite => "the second part would not be at a finite place",
            Self::TurnsFreely => "the parts can turn freely: nothing stops the one on the other",
            Self::FlankMissesCircles => {
                "the flank does not cross both circles: the teeth are too wide"
            }
            Self::ChamferTooLarge => "the chamfer is too large for the tooth",
            Self::MajorRoundTooLarge => "the round at the major circle is too large for the tooth",
            Self::MinorRoundTooLarge => "the round at the minor circle is too large for the tooth",
            Self::TipCornersMeet => {
                "the corners of a tooth would meet: no tip is left on the major circle"
            }
            Self::TeethMeetAtRoot => {
                "neighbouring teeth would meet at the root: no space is left between them"
            }
            Self::HubRootRoundsMeet => {
                "the hub's root rounds would meet: no root is left at the bottom of the space"
            }
            Self::HubTeethTooNarrow => {
                "the hub's teeth would be too narrow: neighbouring spaces would meet"
            }
            Self::NoStraightFlank => "the corners leave no straight flank between them",
            Self::HubRootRoundsTooClose => {
                "the hub's root rounds would come closer to the shaft's tips than the smallest \
                 clearance"
            }
            Self::HubTipsTooClose => {
                "the tips of the hub's teeth would come closer to the shaft's root rounds than \
                 the smallest clearance"
            }
            Self::MinorNotSmaller => "the minor diameter must be smaller than the major diameter",
            Self::MinorClearanceTooLarge => {
                "the minor clearance would take the hub's teeth out beyond the shaft's tips: \
                 nothing would drive"
            }
            Self::FlanksParallel => "the flanks are parallel: they do not meet",
            Self::FlanksNearlyParallel => {
                "the flanks are so nearly parallel that they meet too far away to say"
            }
        }
    }

    /// The reason in the shaft-spline planner's words: [`Why::words`], except that the two
    /// reasons that name "the tool's tip radius" name "the root fillet" instead
    /// ([`Why::RootAboveTipRound`], [`Why::TipRoundsOverlap`]).
    #[must_use]
    pub const fn planner_words(self) -> &'static str {
        match self {
            Self::RootAboveTipRound => {
                "the root must lie below the pitch circle by more than the root fillet"
            }
            Self::TipRoundsOverlap => "the root fillet is too large: its two rounds would overlap",
            other => other.words(),
        }
    }
}

impl core::fmt::Display for Why {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.words())
    }
}

/// A number of a shaft spline that a refusal is about: one variant for each number of
/// [`crate::teeth::shaft_spline::SplineNumbers`] the planner names. An application maps each to
/// its own parameter; [`SplineQuantity::words`] names it in plain English.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SplineQuantity {
    /// The count of teeth.
    Teeth,
    /// How far the teeth run along the axis.
    Length,
    /// The module of involute teeth.
    Module,
    /// The pressure angle of involute teeth.
    PressureAngle,
    /// The height of the shaft's teeth above the pitch circle.
    Addendum,
    /// The depth of the shaft's spaces below the pitch circle.
    Dedendum,
    /// The radius of the cutting tool's tip, the root fillet.
    RootFillet,
    /// The profile shift.
    ProfileShift,
    /// What is taken off each tooth's thickness.
    Thinning,
    /// The height of the hub's teeth inside the pitch circle.
    HubAddendum,
    /// The gap between the shaft's tips and the hub's roots.
    TipClearance,
    /// The least gap between the hub's tips and the start of the shaft's root fillet.
    FormClearance,
    /// The diameter over the shaft's tips.
    MajorDiameter,
    /// The diameter at the bottoms of the shaft's spaces.
    MinorDiameter,
    /// The width of a straight-sided tooth.
    ToothWidth,
    /// The angle between the two flanks of a serration.
    FlankAngle,
    /// The round where a flank meets the minor circle.
    RootRound,
    /// The size of a chamfered or rounded tip corner.
    TipSize,
    /// The gap at the major circle.
    MajorClearance,
    /// The gap at the minor circle.
    MinorClearance,
    /// The round on the tips of the hub's teeth.
    HubTipRound,
    /// The play between the flanks.
    SideClearance,
    /// The round at the roots of the hub's teeth.
    HubRootRound,
    /// The outside diameter of a new hub.
    HubDiameter,
    /// The master spline: which tooth is made different.
    MasterSpline,
    /// Where the middle of the master spline sits.
    MasterAngle,
    /// The diameter of the cutter that ends the tooth spaces of a shaft.
    CutterDiameter,
    /// The chamfer on the ends of a shaft's teeth.
    EndChamfer,
    /// The chamfer where the teeth run out of a hub's bore.
    EntryChamfer,
    /// How far the end chamfer runs across the end.
    EndChamferDistance2,
    /// The angle of the end chamfer to the round face.
    EndChamferAngle,
    /// How far the entry chamfer runs across the end.
    EntryChamferDistance2,
    /// The angle of the entry chamfer to the bore.
    EntryChamferAngle,
}

impl SplineQuantity {
    /// Every quantity, once, in the order of the enum (for tests and tables).
    pub const ALL: &'static [SplineQuantity] = &[
        SplineQuantity::Teeth,
        SplineQuantity::Length,
        SplineQuantity::Module,
        SplineQuantity::PressureAngle,
        SplineQuantity::Addendum,
        SplineQuantity::Dedendum,
        SplineQuantity::RootFillet,
        SplineQuantity::ProfileShift,
        SplineQuantity::Thinning,
        SplineQuantity::HubAddendum,
        SplineQuantity::TipClearance,
        SplineQuantity::FormClearance,
        SplineQuantity::MajorDiameter,
        SplineQuantity::MinorDiameter,
        SplineQuantity::ToothWidth,
        SplineQuantity::FlankAngle,
        SplineQuantity::RootRound,
        SplineQuantity::TipSize,
        SplineQuantity::MajorClearance,
        SplineQuantity::MinorClearance,
        SplineQuantity::HubTipRound,
        SplineQuantity::SideClearance,
        SplineQuantity::HubRootRound,
        SplineQuantity::HubDiameter,
        SplineQuantity::MasterSpline,
        SplineQuantity::MasterAngle,
        SplineQuantity::CutterDiameter,
        SplineQuantity::EndChamfer,
        SplineQuantity::EntryChamfer,
        SplineQuantity::EndChamferDistance2,
        SplineQuantity::EndChamferAngle,
        SplineQuantity::EntryChamferDistance2,
        SplineQuantity::EntryChamferAngle,
    ];

    /// The quantity in plain words, with its article.
    #[must_use]
    pub const fn words(self) -> &'static str {
        match self {
            Self::Teeth => "the count of teeth",
            Self::Length => "the splined length",
            Self::Module => "the module",
            Self::PressureAngle => "the pressure angle",
            Self::Addendum => "the addendum",
            Self::Dedendum => "the dedendum",
            Self::RootFillet => "the root fillet",
            Self::ProfileShift => "the profile shift",
            Self::Thinning => "the thinning",
            Self::HubAddendum => "the hub's addendum",
            Self::TipClearance => "the tip clearance",
            Self::FormClearance => "the form clearance",
            Self::MajorDiameter => "the major diameter",
            Self::MinorDiameter => "the minor diameter",
            Self::ToothWidth => "the tooth width",
            Self::FlankAngle => "the flank angle",
            Self::RootRound => "the root round",
            Self::TipSize => "the size of the tip corners",
            Self::MajorClearance => "the major clearance",
            Self::MinorClearance => "the minor clearance",
            Self::HubTipRound => "the round on the hub's tips",
            Self::SideClearance => "the side clearance",
            Self::HubRootRound => "the round at the hub's roots",
            Self::HubDiameter => "the hub's outside diameter",
            Self::MasterSpline => "the master spline",
            Self::MasterAngle => "the master spline's angle",
            Self::CutterDiameter => "the cutter's diameter",
            Self::EndChamfer => "the end chamfer",
            Self::EntryChamfer => "the entry chamfer",
            Self::EndChamferDistance2 => "the end chamfer's second distance",
            Self::EndChamferAngle => "the end chamfer's angle",
            Self::EntryChamferDistance2 => "the entry chamfer's second distance",
            Self::EntryChamferAngle => "the entry chamfer's angle",
        }
    }
}

impl core::fmt::Display for SplineQuantity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.words())
    }
}

/// What a number the planner requires is measured in, for [`PlanRefusal::Required`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RequiredUnit {
    /// Millimetres.
    Mm,
    /// Millimetres, both sides together on the pitch circle (an involute side clearance).
    MmBothSidesOnPitch,
    /// Millimetres on each side, square to the flanks (a straight-sided side clearance).
    MmEachSideSquareToFlanks,
    /// Millimetres, a tip chamfer's length down the flank.
    MmDownTheFlank,
    /// Millimetres, a tip round's radius.
    MmRoundRadius,
}

impl RequiredUnit {
    /// The unit in plain words.
    #[must_use]
    pub const fn words(self) -> &'static str {
        match self {
            Self::Mm => "mm",
            Self::MmBothSidesOnPitch => "mm, both sides together on the pitch circle",
            Self::MmEachSideSquareToFlanks => "mm on each side, square to the flanks",
            Self::MmDownTheFlank => "mm, the chamfer's length down the flank",
            Self::MmRoundRadius => "mm, the round's radius",
        }
    }
}

/// A refusal of the shaft-spline planner, as data: the number it is about (a
/// [`SplineQuantity`]) and why. Its Display names the number in plain words.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlanRefusal {
    /// A tooth module's number out of range ([`TeethFailure::OutOfRange`]), with the spline's
    /// number it comes from where the planner's tables know it.
    Number {
        /// The spline's number to change, where the planner's tables know the tooth module's
        /// name.
        quantity: Option<SplineQuantity>,
        /// The tooth module's name for the number.
        what: &'static str,
        /// What was given.
        value: f64,
        /// It must be above this (as the tooth module gave it).
        above: f64,
        /// It must be at most this.
        up_to: f64,
    },
    /// A tooth module's reason ([`TeethFailure::Invalid`]), with the spline's number to change
    /// where the planner's tables know the reason.
    Reason {
        /// The spline's number to change, where the planner's tables know the reason.
        quantity: Option<SplineQuantity>,
        /// The tooth module's reason.
        why: Why,
    },
    /// A bound a tooth module could not prove ([`TeethFailure::NotProved`]), with the spline's
    /// number to change where the planner's tables know what was not proved.
    Unproved {
        /// The spline's number to change, where the planner's tables know it.
        quantity: Option<SplineQuantity>,
        /// What could not be proved: [`FILLET_SEPARATION`], [`CHORD`] or [`FIT_DISTANCE`].
        what: &'static str,
        /// The bound that had to be met.
        asked: f64,
        /// The best bound proved when the work ran out.
        reached: f64,
    },
    /// A tooth cannot be left out of these teeth ([`Why::NoCornerOnSmallest`]).
    MasterToothOut,
    /// A wide tooth cannot be made on these teeth ([`Why::NoCornerOnLargest`]).
    MasterWideTooth,
    /// A number given is not a number.
    NotANumber {
        /// The number.
        quantity: SplineQuantity,
    },
    /// A number this kind of teeth needs was not given.
    Required {
        /// The number.
        quantity: SplineQuantity,
        /// What it is measured in.
        unit: RequiredUnit,
    },
    /// The splined length is outside what is taken.
    LengthRange {
        /// The shortest length taken, in mm.
        least: f64,
        /// The longest length taken, in mm.
        most: f64,
        /// What was given.
        value: f64,
    },
    /// The hub's outside diameter was given, but no hub is made.
    HubDiameterWithoutHub,
    /// The hub's outside diameter was not given, but the hub is made.
    HubDiameterRequired,
    /// The master spline's angle was given, but there is no master spline.
    MasterAngleWithoutMaster,
    /// The master spline's angle is outside -360 to 360 degrees.
    MasterAngleRange {
        /// What was given, in degrees.
        value: f64,
    },
    /// A chamfer is outside what is taken.
    ChamferRange {
        /// The chamfer: [`SplineQuantity::EndChamfer`] or [`SplineQuantity::EntryChamfer`].
        quantity: SplineQuantity,
        /// The largest chamfer taken, in mm.
        most: f64,
        /// What was given.
        value: f64,
    },
    /// The cutter's diameter is not a number the planner takes.
    CutterNotANumber {
        /// The largest diameter taken, in mm.
        most: f64,
    },
    /// The cutter's radius does not reach past the depth of the teeth.
    CutterTooSmall {
        /// The diameter the cutter must be larger than, in mm.
        at_least: f64,
    },
    /// The hub's outside diameter is larger than what is taken.
    HubDiameterTooLarge {
        /// The largest diameter taken, in mm.
        most: f64,
        /// What was given.
        value: f64,
    },
    /// The hub's outside diameter leaves no rim around its teeth.
    NoRim {
        /// The diameter the hub's must be larger than, in mm.
        at_least: f64,
    },
    /// A second distance or an angle of a chamfer was given, but not the chamfer itself.
    ChamferShapeWithout {
        /// The number given: a chamfer's second distance or angle.
        quantity: SplineQuantity,
        /// The chamfer it shapes.
        chamfer: SplineQuantity,
    },
    /// Both a second distance and an angle were given for one chamfer.
    ChamferBothShapes {
        /// The chamfer's second distance.
        distance2: SplineQuantity,
        /// The chamfer's angle.
        angle: SplineQuantity,
        /// The chamfer.
        chamfer: SplineQuantity,
    },
    /// A chamfer's second distance is not more than 0, or more than what is taken.
    ChamferDistanceRange {
        /// The second distance.
        quantity: SplineQuantity,
        /// The largest distance taken, in mm.
        most: f64,
        /// What was given.
        value: f64,
    },
    /// A chamfer's angle is not between 0 and 90 degrees.
    ChamferAngleRange {
        /// The angle.
        quantity: SplineQuantity,
        /// What was given, in degrees.
        value: f64,
    },
    /// The outline cannot be made into a profile of lines, arcs and Bezier pieces.
    NotAProfile(ProfileWhy),
}

/// Why a tooth outline cannot be made into a profile of lines, arcs and Bezier pieces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileWhy {
    /// A curve of the outline is held as points only, not fitted as a spline.
    NotFitted,
    /// A spline of the outline cannot be cut into Bezier pieces: the tooth module's reason.
    Spline(Why),
}

impl core::fmt::Display for ProfileWhy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotFitted => f.write_str("a curve of the outline was not fitted"),
            Self::Spline(why) => f.write_str(why.words()),
        }
    }
}

impl PlanRefusal {
    /// The number of the spline to change, where the refusal names one.
    #[must_use]
    pub const fn quantity(&self) -> Option<SplineQuantity> {
        match self {
            Self::Number { quantity, .. }
            | Self::Reason { quantity, .. }
            | Self::Unproved { quantity, .. } => *quantity,
            Self::MasterToothOut | Self::MasterWideTooth => Some(SplineQuantity::MasterSpline),
            Self::NotANumber { quantity }
            | Self::Required { quantity, .. }
            | Self::ChamferRange { quantity, .. }
            | Self::ChamferShapeWithout { quantity, .. }
            | Self::ChamferDistanceRange { quantity, .. }
            | Self::ChamferAngleRange { quantity, .. } => Some(*quantity),
            Self::ChamferBothShapes { distance2, .. } => Some(*distance2),
            Self::LengthRange { .. } => Some(SplineQuantity::Length),
            Self::HubDiameterWithoutHub
            | Self::HubDiameterRequired
            | Self::HubDiameterTooLarge { .. }
            | Self::NoRim { .. } => Some(SplineQuantity::HubDiameter),
            Self::MasterAngleWithoutMaster | Self::MasterAngleRange { .. } => {
                Some(SplineQuantity::MasterAngle)
            }
            Self::CutterNotANumber { .. } | Self::CutterTooSmall { .. } => {
                Some(SplineQuantity::CutterDiameter)
            }
            Self::NotAProfile(_) => None,
        }
    }
}

impl core::fmt::Display for PlanRefusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::Number {
                quantity,
                what,
                value,
                above,
                up_to,
            } => {
                let name = quantity.map_or_else(
                    || format!("the {what}"),
                    |quantity| quantity.words().to_string(),
                );
                // A lower limit a hair above nothing is "more than 0" in plain words.
                let above = if above > 0.0 && above < 1e-4 {
                    0.0
                } else {
                    above
                };
                if !value.is_finite() {
                    write!(f, "{name} must be a number")
                } else if value > up_to {
                    write!(
                        f,
                        "{name} must be at most {}, not {}",
                        plain(up_to),
                        plain(value)
                    )
                } else if value == up_to {
                    write!(f, "{name} must be less than {}", plain(up_to))
                } else if value < 0.0 && above == 0.0 {
                    write!(f, "{name} cannot be negative, not {}", plain(value))
                } else {
                    write!(
                        f,
                        "{name} must be more than {}, not {}",
                        plain(above),
                        plain(value)
                    )
                }
            }
            Self::Reason { quantity, why } => match quantity {
                Some(quantity) => write!(f, "{quantity}: {}", why.planner_words()),
                None => f.write_str(why.words()),
            },
            Self::Unproved {
                quantity,
                what,
                asked,
                reached,
            } => {
                let inner = TeethFailure::NotProved {
                    what,
                    asked,
                    reached,
                };
                match quantity {
                    Some(quantity) => write!(f, "{quantity}: {inner}"),
                    None => write!(f, "{inner}"),
                }
            }
            Self::MasterToothOut => write!(
                f,
                "the master spline: a tooth cannot be left out of these teeth: {}; make a wide \
                 tooth instead, or make the shaft's root rounds and the hub's tip rounds smaller",
                Why::NoCornerOnSmallest.words()
            ),
            Self::MasterWideTooth => write!(
                f,
                "the master spline: a wide tooth cannot be made on these teeth: {}; leave a tooth \
                 out instead, or use plain or chamfered tip corners and smaller root rounds in \
                 the hub",
                Why::NoCornerOnLargest.words()
            ),
            Self::NotANumber { quantity } => write!(f, "{quantity} must be a number"),
            Self::Required { quantity, unit } => {
                write!(f, "{quantity} is required ({})", unit.words())
            }
            Self::LengthRange { least, most, value } => write!(
                f,
                "the splined length must be from {} to {} mm, not {}",
                plain(least),
                plain(most),
                plain(value)
            ),
            Self::HubDiameterWithoutHub => f.write_str(
                "the hub's outside diameter is for a hub that is made: give it only when a hub is \
                 made",
            ),
            Self::HubDiameterRequired => {
                f.write_str("the hub's outside diameter is required to make the hub (mm)")
            }
            Self::MasterAngleWithoutMaster => f.write_str(
                "the master spline's angle is where the master spline sits: give it only with a \
                 master spline",
            ),
            Self::MasterAngleRange { value } => write!(
                f,
                "the master spline's angle must be from -360 to 360 (degrees), not {}",
                plain(value)
            ),
            Self::ChamferRange {
                quantity,
                most,
                value,
            } => write!(
                f,
                "{quantity} must be from 0 to {} mm, not {}",
                plain(most),
                plain(value)
            ),
            Self::CutterNotANumber { most } => write!(
                f,
                "the cutter's diameter must be a number of at most {} mm",
                plain(most)
            ),
            Self::CutterTooSmall { at_least } => write!(
                f,
                "the cutter's diameter must be more than {} mm: the cutter's radius must reach \
                 past the depth of the teeth",
                plain(at_least)
            ),
            Self::HubDiameterTooLarge { most, value } => write!(
                f,
                "the hub's outside diameter must be at most {} mm, not {}",
                plain(most),
                plain(value)
            ),
            Self::NoRim { at_least } => write!(
                f,
                "the hub's outside diameter leaves no rim around the hub's teeth: it must be more \
                 than {} mm",
                plain(at_least)
            ),
            Self::ChamferShapeWithout { quantity, chamfer } => write!(
                f,
                "{quantity} shapes {chamfer}: give it only with {chamfer}"
            ),
            Self::ChamferBothShapes {
                distance2,
                angle,
                chamfer,
            } => write!(
                f,
                "{distance2} and {angle} both set how far {chamfer} reaches across the end: give \
                 one of them, not both"
            ),
            Self::ChamferDistanceRange {
                quantity,
                most,
                value,
            } => {
                if value.is_finite() {
                    write!(
                        f,
                        "{quantity} must be more than 0 and at most {} mm, not {}",
                        plain(most),
                        plain(value)
                    )
                } else {
                    write!(
                        f,
                        "{quantity} must be a number of more than 0 and at most {} mm",
                        plain(most)
                    )
                }
            }
            Self::ChamferAngleRange { quantity, value } => {
                if value.is_finite() {
                    write!(
                        f,
                        "{quantity} must be more than 0 and less than 90 (degrees), not {}",
                        plain(value)
                    )
                } else {
                    write!(
                        f,
                        "{quantity} must be a number of more than 0 and less than 90 (degrees)"
                    )
                }
            }
            Self::NotAProfile(why) => write!(
                f,
                "the spline's outline cannot be made into a profile: {why}"
            ),
        }
    }
}

impl TeethFailure {
    /// The status the C++ would report: [`SM_ERR`] (a comparison detail; nothing upstream).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::OutOfRange { .. }
            | Self::Invalid(_)
            | Self::Refused(_)
            | Self::NotProved { .. } => SM_ERR,
        }
    }

    /// Its stable [`ErrorKind`]: [`ErrorKind::Unsupported`] for [`TeethFailure::NotProved`] and
    /// for a [`PlanRefusal::Unproved`] (a bound the planner named; an undecided certificate, as
    /// #17907 classes one); [`ErrorKind::InvalidInput`] for [`TeethFailure::OutOfRange`],
    /// [`TeethFailure::Invalid`] and every other [`TeethFailure::Refused`].
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::NotProved { .. } | Self::Refused(PlanRefusal::Unproved { .. }) => {
                ErrorKind::Unsupported
            }
            Self::OutOfRange { .. } | Self::Invalid(_) | Self::Refused(_) => {
                ErrorKind::InvalidInput
            }
        }
    }

    /// The number of the spline to change, for a [`TeethFailure::Refused`]
    /// ([`PlanRefusal::quantity`]); `None` otherwise.
    #[must_use]
    pub const fn quantity(&self) -> Option<SplineQuantity> {
        match self {
            Self::Refused(r) => r.quantity(),
            Self::OutOfRange { .. } | Self::Invalid(_) | Self::NotProved { .. } => None,
        }
    }

    /// The reason of a [`TeethFailure::Invalid`]; `None` otherwise.
    #[must_use]
    pub const fn why(&self) -> Option<Why> {
        match self {
            Self::Invalid(w) => Some(*w),
            Self::OutOfRange { .. } | Self::Refused(_) | Self::NotProved { .. } => None,
        }
    }
}

impl core::fmt::Display for TeethFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::OutOfRange {
                what,
                value,
                above,
                up_to,
            } => write!(
                f,
                "{what} is {value}; it must be above {above} and at most {up_to}"
            ),
            Self::Invalid(w) => f.write_str(w.words()),
            Self::Refused(r) => r.fmt(f),
            Self::NotProved {
                what,
                asked,
                reached,
            } => write!(
                f,
                "{what} cannot be proved within the work allowed: asked {asked:e}, reached \
                 {reached:e}"
            ),
        }
    }
}

/// A number in plain words: no trailing zeros, at most six decimals, and a number too small for
/// six decimals in the form 1e-9.
pub(crate) fn plain(x: f64) -> String {
    let s = format!("{x:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s != "0" && s != "-0" {
        s.to_string()
    } else if x != 0.0 && x.is_finite() {
        format!("{x:e}")
    } else {
        "0".to_string()
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a tooth function `operation` that refuses with
    /// `failure`.
    #[must_use]
    #[track_caller]
    pub(crate) fn teeth(operation: &'static str, failure: TeethFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// The tooth failure behind this error, for a [`crate::SolidError::Failed`] with a
    /// [`Reason::Teeth`].
    #[must_use]
    pub fn teeth_failure(&self) -> Option<&TeethFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Teeth(t) => Some(t),
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SolidError;
    use std::collections::{BTreeSet, HashSet};

    /// How a refusal named its number before the numbers were typed: the first word in backticks,
    /// when it is lowercase letters, digits and `_` (as the app's command read the parameter out
    /// of the words).
    fn old_key(words: &str) -> Option<&str> {
        let is_key = |k: &&str| {
            !k.is_empty()
                && k.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        };
        words.split('`').nth(1).filter(is_key)
    }

    /// A quantity's name as the enum spells it, in lowercase words joined by `_`: the key the
    /// refusals named it by before they were typed.
    fn snake(q: SplineQuantity) -> String {
        let mut out = String::new();
        for c in format!("{q:?}").chars() {
            if c.is_ascii_uppercase() {
                if !out.is_empty() {
                    out.push('_');
                }
                out.push(c.to_ascii_lowercase());
            } else {
                out.push(c);
            }
        }
        out
    }

    /// Every form of the planner's refusals, with numbers chosen for each: the words each gave
    /// before the refusals named their numbers in words, and the words each gives now.
    fn forms() -> Vec<(PlanRefusal, &'static str, &'static str)> {
        vec![
            (
                PlanRefusal::Number {
                    quantity: Some(SplineQuantity::Teeth),
                    what: "teeth",
                    value: 1.0,
                    above: 2.0,
                    up_to: 2000.0,
                },
                "`teeth` must be more than 2, not 1",
                "the count of teeth must be more than 2, not 1",
            ),
            (
                PlanRefusal::Number {
                    quantity: None,
                    what: "reach",
                    value: f64::NAN,
                    above: 0.0,
                    up_to: 1e5,
                },
                "the reach must be a number",
                "the reach must be a number",
            ),
            (
                PlanRefusal::Number {
                    quantity: Some(SplineQuantity::Module),
                    what: "module",
                    value: 2000.0,
                    above: 0.0,
                    up_to: 1000.0,
                },
                "`module` must be at most 1000, not 2000",
                "the module must be at most 1000, not 2000",
            ),
            (
                PlanRefusal::Number {
                    quantity: Some(SplineQuantity::Module),
                    what: "module",
                    value: 1000.0,
                    above: 0.0,
                    up_to: 1000.0,
                },
                "`module` must be less than 1000",
                "the module must be less than 1000",
            ),
            (
                PlanRefusal::Number {
                    quantity: None,
                    what: "tip clearance",
                    value: -0.5,
                    above: 1e-9,
                    up_to: 10.0,
                },
                "the tip clearance cannot be negative, not -0.5",
                "the tip clearance cannot be negative, not -0.5",
            ),
            (
                PlanRefusal::Reason {
                    quantity: Some(SplineQuantity::ProfileShift),
                    why: Why::CutThrough,
                },
                "`profile_shift`: the undercut would cut the tooth through at its root: its two \
                 root fillets would meet",
                "the profile shift: the undercut would cut the tooth through at its root: its two \
                 root fillets would meet",
            ),
            (
                PlanRefusal::Reason {
                    quantity: Some(SplineQuantity::RootFillet),
                    why: Why::TipRoundsOverlap,
                },
                "`root_fillet`: the root fillet is too large: its two rounds would overlap",
                "the root fillet: the root fillet is too large: its two rounds would overlap",
            ),
            (
                PlanRefusal::Reason {
                    quantity: None,
                    why: Why::TipRoundsOverlap,
                },
                "the tool's tip radius is too large: its two rounds would overlap",
                "the tool's tip radius is too large: its two rounds would overlap",
            ),
            (
                PlanRefusal::Unproved {
                    quantity: Some(SplineQuantity::RootFillet),
                    what: FILLET_SEPARATION,
                    asked: 0.0,
                    reached: -1e-9,
                },
                "`root_fillet`: the separation of the root fillets cannot be proved within the \
                 work allowed: asked 0e0, reached -1e-9",
                "the root fillet: the separation of the root fillets cannot be proved within the \
                 work allowed: asked 0e0, reached -1e-9",
            ),
            (
                PlanRefusal::Unproved {
                    quantity: None,
                    what: CHORD,
                    asked: 1e-6,
                    reached: 3e-6,
                },
                "the chord error of a curve cannot be proved within the work allowed: asked \
                 1e-6, reached 3e-6",
                "the chord error of a curve cannot be proved within the work allowed: asked \
                 1e-6, reached 3e-6",
            ),
            (
                PlanRefusal::MasterToothOut,
                "`master_spline`: a tooth cannot be left out of these teeth: no corner of the \
                 teeth lies on the smallest circle: no tooth or space can be closed there; make \
                 a wide tooth instead (wide_tooth), or make the shaft's root rounds and the \
                 hub's tip rounds smaller",
                "the master spline: a tooth cannot be left out of these teeth: no corner of the \
                 teeth lies on the smallest circle: no tooth or space can be closed there; make \
                 a wide tooth instead, or make the shaft's root rounds and the hub's tip rounds \
                 smaller",
            ),
            (
                PlanRefusal::MasterWideTooth,
                "`master_spline`: a wide tooth cannot be made on these teeth: no corner of the \
                 teeth lies on the largest circle: the tops of the teeth and the bottoms of the \
                 spaces need a flat part to join; leave a tooth out instead (tooth_out), or use \
                 plain or chamfered tip corners and smaller root rounds in the hub",
                "the master spline: a wide tooth cannot be made on these teeth: no corner of the \
                 teeth lies on the largest circle: the tops of the teeth and the bottoms of the \
                 spaces need a flat part to join; leave a tooth out instead, or use plain or \
                 chamfered tip corners and smaller root rounds in the hub",
            ),
            (
                PlanRefusal::NotANumber {
                    quantity: SplineQuantity::Length,
                },
                "`length` must be a number",
                "the splined length must be a number",
            ),
            (
                PlanRefusal::Required {
                    quantity: SplineQuantity::Module,
                    unit: RequiredUnit::Mm,
                },
                "`module` is required (mm)",
                "the module is required (mm)",
            ),
            (
                PlanRefusal::LengthRange {
                    least: 1e-3,
                    most: 1e5,
                    value: 0.0,
                },
                "`length` must be from 0.001 to 100000 mm, not 0",
                "the splined length must be from 0.001 to 100000 mm, not 0",
            ),
            (
                PlanRefusal::HubDiameterWithoutHub,
                "`hub_diameter` is the hub's outside diameter: give it only when `make` is hub \
                 or both",
                "the hub's outside diameter is for a hub that is made: give it only when a hub \
                 is made",
            ),
            (
                PlanRefusal::HubDiameterRequired,
                "`hub_diameter` is required to make the hub (mm, its outside diameter)",
                "the hub's outside diameter is required to make the hub (mm)",
            ),
            (
                PlanRefusal::MasterAngleWithoutMaster,
                "`master_angle` is where the master spline sits: give it only with \
                 `master_spline` tooth_out or wide_tooth",
                "the master spline's angle is where the master spline sits: give it only with a \
                 master spline",
            ),
            (
                PlanRefusal::MasterAngleRange { value: 400.0 },
                "`master_angle` must be from -360 to 360 (degrees), not 400",
                "the master spline's angle must be from -360 to 360 (degrees), not 400",
            ),
            (
                PlanRefusal::ChamferRange {
                    quantity: SplineQuantity::EndChamfer,
                    most: 1e5,
                    value: -1.0,
                },
                "`end_chamfer` must be from 0 to 100000 mm, not -1",
                "the end chamfer must be from 0 to 100000 mm, not -1",
            ),
            (
                PlanRefusal::CutterNotANumber { most: 1e5 },
                "`cutter_diameter` must be a number of at most 100000 mm",
                "the cutter's diameter must be a number of at most 100000 mm",
            ),
            (
                PlanRefusal::CutterTooSmall { at_least: 5.2 },
                "`cutter_diameter` must be more than 5.2 mm: the cutter's radius must reach past \
                 the depth of the teeth",
                "the cutter's diameter must be more than 5.2 mm: the cutter's radius must reach \
                 past the depth of the teeth",
            ),
            (
                PlanRefusal::HubDiameterTooLarge {
                    most: 1e5,
                    value: 2e5,
                },
                "`hub_diameter` must be at most 100000 mm, not 200000",
                "the hub's outside diameter must be at most 100000 mm, not 200000",
            ),
            (
                PlanRefusal::NoRim { at_least: 31.25 },
                "`hub_diameter` leaves no rim around the hub's teeth: it must be more than 31.25 \
                 mm",
                "the hub's outside diameter leaves no rim around the hub's teeth: it must be more \
                 than 31.25 mm",
            ),
            (
                PlanRefusal::ChamferShapeWithout {
                    quantity: SplineQuantity::EndChamferAngle,
                    chamfer: SplineQuantity::EndChamfer,
                },
                "`end_chamfer_angle` shapes the chamfer `end_chamfer` sets: give it only with \
                 `end_chamfer`",
                "the end chamfer's angle shapes the end chamfer: give it only with the end \
                 chamfer",
            ),
            (
                PlanRefusal::ChamferBothShapes {
                    distance2: SplineQuantity::EntryChamferDistance2,
                    angle: SplineQuantity::EntryChamferAngle,
                    chamfer: SplineQuantity::EntryChamfer,
                },
                "`entry_chamfer_distance2` and `entry_chamfer_angle` both set how far the chamfer \
                 `entry_chamfer` reaches across the end: give one of them, not both",
                "the entry chamfer's second distance and the entry chamfer's angle both set how \
                 far the entry chamfer reaches across the end: give one of them, not both",
            ),
            (
                PlanRefusal::ChamferDistanceRange {
                    quantity: SplineQuantity::EndChamferDistance2,
                    most: 1e5,
                    value: -0.5,
                },
                "`end_chamfer_distance2` must be more than 0 and at most 100000 mm, not -0.5",
                "the end chamfer's second distance must be more than 0 and at most 100000 mm, not \
                 -0.5",
            ),
            (
                PlanRefusal::ChamferAngleRange {
                    quantity: SplineQuantity::EntryChamferAngle,
                    value: 90.0,
                },
                "`entry_chamfer_angle` must be more than 0 and less than 90 (degrees), not 90",
                "the entry chamfer's angle must be more than 0 and less than 90 (degrees), not 90",
            ),
            (
                PlanRefusal::NotAProfile(ProfileWhy::Spline(Why::NotASpline)),
                "the spline's outline cannot be made into a profile: the knots and control \
                 points do not describe a spline",
                "the spline's outline cannot be made into a profile: the knots and control \
                 points do not describe a spline",
            ),
        ]
    }

    /// Every quantity is named once, in its own words, and its name as the enum spells it is the
    /// key the refusals named it by before they were typed.
    #[test]
    fn every_quantity_has_its_own_words() {
        assert_eq!(SplineQuantity::ALL.len(), 33);
        for (i, q) in SplineQuantity::ALL.iter().enumerate() {
            assert_eq!(*q as usize, i, "{q:?}");
            assert!(q.words().starts_with("the "), "{q:?}");
            assert!(
                !q.words().contains('`') && !q.words().contains('_'),
                "{q:?}"
            );
            assert_eq!(q.to_string(), q.words());
        }
        let words: BTreeSet<&str> = SplineQuantity::ALL.iter().map(|q| q.words()).collect();
        assert_eq!(words.len(), 33, "two quantities share their words");
        assert_eq!(
            snake(SplineQuantity::EndChamferDistance2),
            "end_chamfer_distance2"
        );
        assert_eq!(snake(SplineQuantity::MasterSpline), "master_spline");
        let units = [
            RequiredUnit::Mm,
            RequiredUnit::MmBothSidesOnPitch,
            RequiredUnit::MmEachSideSquareToFlanks,
            RequiredUnit::MmDownTheFlank,
            RequiredUnit::MmRoundRadius,
        ];
        let unit_words: BTreeSet<&str> = units.iter().map(|u| u.words()).collect();
        assert_eq!(unit_words.len(), units.len());
        assert!(unit_words.iter().all(|w| w.starts_with("mm")));
    }

    /// A tooth outline that cannot be made into a profile says why, and a chamfer's second
    /// distance or angle that is not a number says so.
    #[test]
    fn the_profile_and_chamfer_words() {
        assert_eq!(
            PlanRefusal::NotAProfile(ProfileWhy::NotFitted).to_string(),
            "the spline's outline cannot be made into a profile: a curve of the outline was not \
             fitted"
        );
        let distance = PlanRefusal::ChamferDistanceRange {
            quantity: SplineQuantity::EntryChamferDistance2,
            most: 1e5,
            value: f64::NAN,
        };
        assert_eq!(
            distance.to_string(),
            "the entry chamfer's second distance must be a number of more than 0 and at most \
             100000 mm"
        );
        let angle = PlanRefusal::ChamferAngleRange {
            quantity: SplineQuantity::EndChamferAngle,
            value: f64::INFINITY,
        };
        assert_eq!(
            angle.to_string(),
            "the end chamfer's angle must be a number of more than 0 and less than 90 (degrees)"
        );
        assert_eq!(
            PlanRefusal::NotAProfile(ProfileWhy::NotFitted).quantity(),
            None
        );
    }

    #[test]
    fn every_reason_has_its_own_words() {
        assert_eq!(Why::ALL.len(), 71);
        // In the order of the enum, each variant once.
        for (i, w) in Why::ALL.iter().enumerate() {
            assert_eq!(*w as usize, i, "{w:?}");
        }
        let variants: HashSet<Why> = Why::ALL.iter().copied().collect();
        assert_eq!(variants.len(), 71);
        let words: BTreeSet<&str> = Why::ALL.iter().copied().map(Why::words).collect();
        assert_eq!(words.len(), 71, "two reasons share their words");
        let renamed = [Why::RootAboveTipRound, Why::TipRoundsOverlap];
        for &w in Why::ALL {
            assert!(!w.words().is_empty(), "{w:?}");
            assert!(!w.words().contains('`'), "{w:?}");
            assert_eq!(w.to_string(), w.words(), "{w:?}");
            if renamed.contains(&w) {
                let replaced = w
                    .words()
                    .replace("the tool's tip radius", "the root fillet");
                assert_eq!(w.planner_words(), replaced, "{w:?}");
                assert_ne!(w.planner_words(), w.words(), "{w:?}");
            } else {
                assert_eq!(w.planner_words(), w.words(), "{w:?}");
                assert!(!w.words().contains("the tool's tip radius"), "{w:?}");
            }
        }
    }

    /// Each refusal gives its words now, names the number the words before named, and names it
    /// in words, not by a key: no word in backticks is left.
    #[test]
    fn the_words_name_the_numbers_the_words_before_named() {
        let out_of_range = TeethFailure::OutOfRange {
            what: "module",
            value: -1.0,
            above: 0.0,
            up_to: 1000.0,
        };
        assert_eq!(
            out_of_range.to_string(),
            "module is -1; it must be above 0 and at most 1000"
        );
        assert_eq!(
            TeethFailure::Invalid(Why::CutThrough).to_string(),
            "the undercut would cut the tooth through at its root: its two root fillets would meet"
        );
        let unproved = TeethFailure::NotProved {
            what: CHORD,
            asked: 1e-6,
            reached: 3e-6,
        };
        assert_eq!(
            unproved.to_string(),
            "the chord error of a curve cannot be proved within the work allowed: asked 1e-6, \
             reached 3e-6"
        );
        let forms = forms();
        // Every one of the twenty-two forms is written out above.
        let kinds: HashSet<_> = forms
            .iter()
            .map(|(r, _, _)| std::mem::discriminant(r))
            .collect();
        assert_eq!(kinds.len(), 22);
        for (refusal, before, now) in &forms {
            assert_eq!(refusal.to_string(), *now, "{refusal:?}");
            assert_eq!(
                TeethFailure::Refused(*refusal).to_string(),
                *now,
                "{refusal:?}"
            );
            assert!(!now.contains('`'), "{refusal:?}");
            // The number named is the one the words before named.
            assert_eq!(
                refusal.quantity().map(snake).as_deref(),
                old_key(before),
                "{refusal:?}"
            );
            // Where the words before named the number first, the words now name it first too.
            if let Some(q) = refusal.quantity() {
                if before.starts_with('`') {
                    assert!(now.starts_with(q.words()), "{q:?}: {now}");
                }
            }
        }
        assert_eq!(plain(1e-9), "1e-9");
        assert_eq!(plain(-0.0), "0");
        assert_eq!(plain(2.5), "2.5");
    }

    #[test]
    fn kinds_quantities_and_the_error() {
        let mut failures = vec![
            TeethFailure::OutOfRange {
                what: "module",
                value: -1.0,
                above: 0.0,
                up_to: 1000.0,
            },
            TeethFailure::NotProved {
                what: FIT_DISTANCE,
                asked: 1e-6,
                reached: f64::INFINITY,
            },
        ];
        failures.extend(Why::ALL.iter().copied().map(TeethFailure::Invalid));
        failures.extend(forms().iter().map(|(r, _, _)| TeethFailure::Refused(*r)));
        for f in &failures {
            let unproved = matches!(
                f,
                TeethFailure::NotProved { .. }
                    | TeethFailure::Refused(PlanRefusal::Unproved { .. })
            );
            let kind = if unproved {
                ErrorKind::Unsupported
            } else {
                ErrorKind::InvalidInput
            };
            assert_eq!(f.kind(), kind, "{f:?}");
            assert_eq!(f.comparison_status(), SM_ERR, "{f:?}");
            match f {
                TeethFailure::Refused(r) => {
                    assert_eq!(f.quantity(), r.quantity(), "{r:?}");
                    assert_eq!(f.why(), None, "{r:?}");
                }
                TeethFailure::Invalid(w) => {
                    assert_eq!(f.why(), Some(*w));
                    assert_eq!(f.quantity(), None);
                }
                _ => {
                    assert_eq!(f.quantity(), None, "{f:?}");
                    assert_eq!(f.why(), None, "{f:?}");
                }
            }
            let e = SolidError::teeth("teeth::x", *f);
            // As written out, since a failure that holds a number that is not a number does not
            // equal itself.
            assert_eq!(format!("{:?}", e.teeth_failure()), format!("{:?}", Some(f)));
            assert_eq!(e.to_string(), format!("teeth::x: {f}"));
            assert_eq!(e.kind(), f.kind(), "{f:?}");
            assert_eq!(e.operation(), "teeth::x");
        }
        // Both forms of a bound not proved, with a quantity and without, are unsupported.
        for quantity in [Some(SplineQuantity::RootFillet), None] {
            let named = TeethFailure::Refused(PlanRefusal::Unproved {
                quantity,
                what: FILLET_SEPARATION,
                asked: 0.0,
                reached: -1e-12,
            });
            assert_eq!(named.kind(), ErrorKind::Unsupported, "{quantity:?}");
            assert_eq!(named.quantity(), quantity);
        }
        assert_eq!(SolidError::sm_err("teeth::x").teeth_failure(), None);
    }
}
