//! Why numbers give no teeth, in solidcore's terms: the failures of the tooth module
//! ([`crate::teeth`]).
//!
//! Upstream: none. The tooth module makes the outlines of splined shafts and hubs from plain
//! numbers. A number that is not finite or lies outside what is accepted is refused as
//! [`TeethFailure::OutOfRange`]; numbers that are each acceptable but together make no teeth are
//! refused as [`TeethFailure::Invalid`] with a typed reason ([`Why`]); the shaft-spline planner
//! refuses as [`TeethFailure::Refused`] naming the number to change ([`PlanRefusal`]); and a bound
//! that could not be proved within the work allowed is refused as [`TeethFailure::NotProved`].
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
#[non_exhaustive]
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
    /// The shaft-spline planner's refusal: the number to change and why
    /// (`crate::teeth::shaft_spline`).
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
#[non_exhaustive]
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

/// A refusal of the shaft-spline planner, as data: the number it is about and why. Its Display is
/// the planner's words, letter for letter as they were.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum PlanRefusal {
    /// A tooth module's number out of range ([`TeethFailure::OutOfRange`]), named by the
    /// planner's key where a table knows it.
    Number {
        /// The number to change, where the planner's tables know the tooth module's name.
        key: Option<&'static str>,
        /// The tooth module's name for the number.
        what: &'static str,
        /// What was given.
        value: f64,
        /// It must be above this (as the tooth module gave it).
        above: f64,
        /// It must be at most this.
        up_to: f64,
    },
    /// A tooth module's reason ([`TeethFailure::Invalid`]), with the number to change where the
    /// planner's tables know the reason.
    Reason {
        /// The number to change, where the planner's tables know the reason.
        key: Option<&'static str>,
        /// The tooth module's reason.
        why: Why,
    },
    /// A bound a tooth module could not prove ([`TeethFailure::NotProved`]), with the number to
    /// change where the planner's tables know what was not proved.
    Unproved {
        /// The number to change, where the planner's tables know it.
        key: Option<&'static str>,
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
        key: &'static str,
    },
    /// A number this kind of teeth needs was not given.
    Required {
        /// The number.
        key: &'static str,
        /// What it is measured in, in words (for example "mm").
        unit: &'static str,
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
        /// The chamfer's key, `end_chamfer` or `entry_chamfer`, without the backticks the words
        /// put around it.
        key: &'static str,
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
        /// The number given, `end_chamfer_distance2` for example.
        key: &'static str,
        /// The chamfer it shapes, `end_chamfer` or `entry_chamfer`.
        chamfer: &'static str,
    },
    /// Both a second distance and an angle were given for one chamfer.
    ChamferBothShapes {
        /// The chamfer's second distance, `end_chamfer_distance2` or `entry_chamfer_distance2`.
        distance2: &'static str,
        /// The chamfer's angle, `end_chamfer_angle` or `entry_chamfer_angle`.
        angle: &'static str,
        /// The chamfer, `end_chamfer` or `entry_chamfer`.
        chamfer: &'static str,
    },
    /// A chamfer's second distance is not more than 0, or more than what is taken.
    ChamferDistanceRange {
        /// The second distance's key.
        key: &'static str,
        /// The largest distance taken, in mm.
        most: f64,
        /// What was given.
        value: f64,
    },
    /// A chamfer's angle is not between 0 and 90 degrees.
    ChamferAngleRange {
        /// The angle's key.
        key: &'static str,
        /// What was given, in degrees.
        value: f64,
    },
    /// A tooth space is too wide for its end to follow the cutter's curve.
    CutterSpaceTooWide,
    /// The outline cannot be made into a profile of lines, arcs and Bezier pieces.
    NotAProfile(ProfileWhy),
}

/// Why a tooth outline cannot be made into a profile of lines, arcs and Bezier pieces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProfileWhy {
    /// A Bezier piece of a spline has more control points than a profile segment holds.
    DegreeTooHigh,
    /// A curve of the outline is held as points only, not fitted as a spline.
    NotFitted,
    /// A spline of the outline cannot be cut into Bezier pieces: the tooth module's reason.
    Spline(Why),
}

impl core::fmt::Display for ProfileWhy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::DegreeTooHigh => f.write_str("a curve of too high a degree"),
            Self::NotFitted => f.write_str("a curve of the outline was not fitted"),
            Self::Spline(why) => f.write_str(why.words()),
        }
    }
}

impl PlanRefusal {
    /// The number to change (the dialog's field key), where the refusal names one.
    #[must_use]
    pub const fn key(&self) -> Option<&'static str> {
        match self {
            Self::Number { key, .. } | Self::Reason { key, .. } | Self::Unproved { key, .. } => {
                *key
            }
            Self::MasterToothOut | Self::MasterWideTooth => Some("master_spline"),
            Self::NotANumber { key }
            | Self::Required { key, .. }
            | Self::ChamferRange { key, .. }
            | Self::ChamferShapeWithout { key, .. }
            | Self::ChamferDistanceRange { key, .. }
            | Self::ChamferAngleRange { key, .. } => Some(*key),
            Self::ChamferBothShapes { distance2, .. } => Some(*distance2),
            Self::LengthRange { .. } => Some("length"),
            Self::HubDiameterWithoutHub
            | Self::HubDiameterRequired
            | Self::HubDiameterTooLarge { .. }
            | Self::NoRim { .. } => Some("hub_diameter"),
            Self::MasterAngleWithoutMaster | Self::MasterAngleRange { .. } => Some("master_angle"),
            Self::CutterNotANumber { .. }
            | Self::CutterTooSmall { .. }
            | Self::CutterSpaceTooWide => Some("cutter_diameter"),
            Self::NotAProfile(_) => None,
        }
    }
}

impl core::fmt::Display for PlanRefusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::Number {
                key,
                what,
                value,
                above,
                up_to,
            } => {
                let name = key.map_or_else(|| format!("the {what}"), |key| format!("`{key}`"));
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
            Self::Reason { key, why } => match key {
                Some(key) => write!(f, "`{key}`: {}", why.planner_words()),
                None => f.write_str(why.words()),
            },
            Self::Unproved {
                key,
                what,
                asked,
                reached,
            } => {
                let inner = TeethFailure::NotProved {
                    what,
                    asked,
                    reached,
                };
                match key {
                    Some(key) => write!(f, "`{key}`: {inner}"),
                    None => write!(f, "{inner}"),
                }
            }
            Self::MasterToothOut => write!(
                f,
                "`master_spline`: a tooth cannot be left out of these teeth: {}; make a wide \
                 tooth instead (wide_tooth), or make the shaft's root rounds and the hub's tip \
                 rounds smaller",
                Why::NoCornerOnSmallest.words()
            ),
            Self::MasterWideTooth => write!(
                f,
                "`master_spline`: a wide tooth cannot be made on these teeth: {}; leave a tooth \
                 out instead (tooth_out), or use plain or chamfered tip corners and smaller root \
                 rounds in the hub",
                Why::NoCornerOnLargest.words()
            ),
            Self::NotANumber { key } => write!(f, "`{key}` must be a number"),
            Self::Required { key, unit } => write!(f, "`{key}` is required ({unit})"),
            Self::LengthRange { least, most, value } => write!(
                f,
                "`length` must be from {} to {} mm, not {}",
                plain(least),
                plain(most),
                plain(value)
            ),
            Self::HubDiameterWithoutHub => f.write_str(
                "`hub_diameter` is the hub's outside diameter: give it only when `make` is hub or \
                 both",
            ),
            Self::HubDiameterRequired => {
                f.write_str("`hub_diameter` is required to make the hub (mm, its outside diameter)")
            }
            Self::MasterAngleWithoutMaster => f.write_str(
                "`master_angle` is where the master spline sits: give it only with \
                 `master_spline` tooth_out or wide_tooth",
            ),
            Self::MasterAngleRange { value } => write!(
                f,
                "`master_angle` must be from -360 to 360 (degrees), not {}",
                plain(value)
            ),
            Self::ChamferRange { key, most, value } => write!(
                f,
                "`{key}` must be from 0 to {} mm, not {}",
                plain(most),
                plain(value)
            ),
            Self::CutterNotANumber { most } => write!(
                f,
                "`cutter_diameter` must be a number of at most {} mm",
                plain(most)
            ),
            Self::CutterTooSmall { at_least } => write!(
                f,
                "`cutter_diameter` must be more than {} mm: the cutter's radius must reach past \
                 the depth of the teeth",
                plain(at_least)
            ),
            Self::HubDiameterTooLarge { most, value } => write!(
                f,
                "`hub_diameter` must be at most {} mm, not {}",
                plain(most),
                plain(value)
            ),
            Self::NoRim { at_least } => write!(
                f,
                "`hub_diameter` leaves no rim around the hub's teeth: it must be more than {} mm",
                plain(at_least)
            ),
            Self::ChamferShapeWithout { key, chamfer } => write!(
                f,
                "`{key}` shapes the chamfer `{chamfer}` sets: give it only with `{chamfer}`"
            ),
            Self::ChamferBothShapes {
                distance2,
                angle,
                chamfer,
            } => write!(
                f,
                "`{distance2}` and `{angle}` both set how far the chamfer `{chamfer}` reaches \
                 across the end: give one of them, not both"
            ),
            Self::ChamferDistanceRange { key, most, value } => {
                if value.is_finite() {
                    write!(
                        f,
                        "`{key}` must be more than 0 and at most {} mm, not {}",
                        plain(most),
                        plain(value)
                    )
                } else {
                    write!(
                        f,
                        "`{key}` must be a number of more than 0 and at most {} mm",
                        plain(most)
                    )
                }
            }
            Self::ChamferAngleRange { key, value } => {
                if value.is_finite() {
                    write!(
                        f,
                        "`{key}` must be more than 0 and less than 90 (degrees), not {}",
                        plain(value)
                    )
                } else {
                    write!(
                        f,
                        "`{key}` must be a number of more than 0 and less than 90 (degrees)"
                    )
                }
            }
            Self::CutterSpaceTooWide => f.write_str(
                "a tooth space is too wide to end in the cutter's curve: leave out \
                 `cutter_diameter` (the spaces then end square) or use more teeth",
            ),
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
    /// for a [`PlanRefusal::Unproved`] (a bound the planner keyed; an undecided certificate, as
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

    /// The number to change, for a [`TeethFailure::Refused`] ([`PlanRefusal::key`]); `None`
    /// otherwise.
    #[must_use]
    pub const fn key(&self) -> Option<&'static str> {
        match self {
            Self::Refused(r) => r.key(),
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

    /// Today's parse of a refusal's key: the first word in backticks, when it is lowercase
    /// letters, digits and `_` (as the app's command reads the parameter out of the words).
    fn old_key(words: &str) -> Option<&str> {
        let is_key = |k: &&str| {
            !k.is_empty()
                && k.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        };
        words.split('`').nth(1).filter(is_key)
    }

    /// Every form of the planner's refusals, with numbers chosen for each, and the words each
    /// gave before the refusals were typed.
    fn forms() -> Vec<(PlanRefusal, &'static str)> {
        vec![
            (
                PlanRefusal::Number {
                    key: Some("teeth"),
                    what: "teeth",
                    value: 1.0,
                    above: 2.0,
                    up_to: 2000.0,
                },
                "`teeth` must be more than 2, not 1",
            ),
            (
                PlanRefusal::Number {
                    key: None,
                    what: "reach",
                    value: f64::NAN,
                    above: 0.0,
                    up_to: 1e5,
                },
                "the reach must be a number",
            ),
            (
                PlanRefusal::Number {
                    key: Some("module"),
                    what: "module",
                    value: 2000.0,
                    above: 0.0,
                    up_to: 1000.0,
                },
                "`module` must be at most 1000, not 2000",
            ),
            (
                PlanRefusal::Number {
                    key: Some("module"),
                    what: "module",
                    value: 1000.0,
                    above: 0.0,
                    up_to: 1000.0,
                },
                "`module` must be less than 1000",
            ),
            (
                PlanRefusal::Number {
                    key: None,
                    what: "tip clearance",
                    value: -0.5,
                    above: 1e-9,
                    up_to: 10.0,
                },
                "the tip clearance cannot be negative, not -0.5",
            ),
            (
                PlanRefusal::Reason {
                    key: Some("profile_shift"),
                    why: Why::CutThrough,
                },
                "`profile_shift`: the undercut would cut the tooth through at its root: its two \
                 root fillets would meet",
            ),
            (
                PlanRefusal::Reason {
                    key: Some("root_fillet"),
                    why: Why::TipRoundsOverlap,
                },
                "`root_fillet`: the root fillet is too large: its two rounds would overlap",
            ),
            (
                PlanRefusal::Reason {
                    key: None,
                    why: Why::TipRoundsOverlap,
                },
                "the tool's tip radius is too large: its two rounds would overlap",
            ),
            (
                PlanRefusal::Unproved {
                    key: Some("root_fillet"),
                    what: FILLET_SEPARATION,
                    asked: 0.0,
                    reached: -1e-9,
                },
                "`root_fillet`: the separation of the root fillets cannot be proved within the \
                 work allowed: asked 0e0, reached -1e-9",
            ),
            (
                PlanRefusal::Unproved {
                    key: None,
                    what: CHORD,
                    asked: 1e-6,
                    reached: 3e-6,
                },
                "the chord error of a curve cannot be proved within the work allowed: asked \
                 1e-6, reached 3e-6",
            ),
            (
                PlanRefusal::MasterToothOut,
                "`master_spline`: a tooth cannot be left out of these teeth: no corner of the \
                 teeth lies on the smallest circle: no tooth or space can be closed there; make \
                 a wide tooth instead (wide_tooth), or make the shaft's root rounds and the \
                 hub's tip rounds smaller",
            ),
            (
                PlanRefusal::MasterWideTooth,
                "`master_spline`: a wide tooth cannot be made on these teeth: no corner of the \
                 teeth lies on the largest circle: the tops of the teeth and the bottoms of the \
                 spaces need a flat part to join; leave a tooth out instead (tooth_out), or use \
                 plain or chamfered tip corners and smaller root rounds in the hub",
            ),
            (
                PlanRefusal::NotANumber { key: "length" },
                "`length` must be a number",
            ),
            (
                PlanRefusal::Required {
                    key: "module",
                    unit: "mm",
                },
                "`module` is required (mm)",
            ),
            (
                PlanRefusal::LengthRange {
                    least: 1e-3,
                    most: 1e5,
                    value: 0.0,
                },
                "`length` must be from 0.001 to 100000 mm, not 0",
            ),
            (
                PlanRefusal::HubDiameterWithoutHub,
                "`hub_diameter` is the hub's outside diameter: give it only when `make` is hub \
                 or both",
            ),
            (
                PlanRefusal::HubDiameterRequired,
                "`hub_diameter` is required to make the hub (mm, its outside diameter)",
            ),
            (
                PlanRefusal::MasterAngleWithoutMaster,
                "`master_angle` is where the master spline sits: give it only with \
                 `master_spline` tooth_out or wide_tooth",
            ),
            (
                PlanRefusal::MasterAngleRange { value: 400.0 },
                "`master_angle` must be from -360 to 360 (degrees), not 400",
            ),
            (
                PlanRefusal::ChamferRange {
                    key: "end_chamfer",
                    most: 1e5,
                    value: -1.0,
                },
                "`end_chamfer` must be from 0 to 100000 mm, not -1",
            ),
            (
                PlanRefusal::CutterNotANumber { most: 1e5 },
                "`cutter_diameter` must be a number of at most 100000 mm",
            ),
            (
                PlanRefusal::CutterTooSmall { at_least: 5.2 },
                "`cutter_diameter` must be more than 5.2 mm: the cutter's radius must reach past \
                 the depth of the teeth",
            ),
            (
                PlanRefusal::HubDiameterTooLarge {
                    most: 1e5,
                    value: 2e5,
                },
                "`hub_diameter` must be at most 100000 mm, not 200000",
            ),
            (
                PlanRefusal::NoRim { at_least: 31.25 },
                "`hub_diameter` leaves no rim around the hub's teeth: it must be more than 31.25 \
                 mm",
            ),
            (
                PlanRefusal::ChamferShapeWithout {
                    key: "end_chamfer_angle",
                    chamfer: "end_chamfer",
                },
                "`end_chamfer_angle` shapes the chamfer `end_chamfer` sets: give it only with \
                 `end_chamfer`",
            ),
            (
                PlanRefusal::ChamferBothShapes {
                    distance2: "entry_chamfer_distance2",
                    angle: "entry_chamfer_angle",
                    chamfer: "entry_chamfer",
                },
                "`entry_chamfer_distance2` and `entry_chamfer_angle` both set how far the chamfer \
                 `entry_chamfer` reaches across the end: give one of them, not both",
            ),
            (
                PlanRefusal::ChamferDistanceRange {
                    key: "end_chamfer_distance2",
                    most: 1e5,
                    value: -0.5,
                },
                "`end_chamfer_distance2` must be more than 0 and at most 100000 mm, not -0.5",
            ),
            (
                PlanRefusal::ChamferAngleRange {
                    key: "entry_chamfer_angle",
                    value: 90.0,
                },
                "`entry_chamfer_angle` must be more than 0 and less than 90 (degrees), not 90",
            ),
            (
                PlanRefusal::CutterSpaceTooWide,
                "a tooth space is too wide to end in the cutter's curve: leave out \
                 `cutter_diameter` (the spaces then end square) or use more teeth",
            ),
            (
                PlanRefusal::NotAProfile(ProfileWhy::Spline(Why::NotASpline)),
                "the spline's outline cannot be made into a profile: the knots and control \
                 points do not describe a spline",
            ),
        ]
    }

    /// A tooth outline that cannot be made into a profile says why in the words the rebuild gave,
    /// and a chamfer's second distance or angle that is not a number says so.
    #[test]
    fn the_profile_and_chamfer_words() {
        let words = |why| PlanRefusal::NotAProfile(why).to_string();
        assert_eq!(
            words(ProfileWhy::DegreeTooHigh),
            "the spline's outline cannot be made into a profile: a curve of too high a degree"
        );
        assert_eq!(
            words(ProfileWhy::NotFitted),
            "the spline's outline cannot be made into a profile: a curve of the outline was not \
             fitted"
        );
        let distance = PlanRefusal::ChamferDistanceRange {
            key: "entry_chamfer_distance2",
            most: 1e5,
            value: f64::NAN,
        };
        assert_eq!(
            distance.to_string(),
            "`entry_chamfer_distance2` must be a number of more than 0 and at most 100000 mm"
        );
        let angle = PlanRefusal::ChamferAngleRange {
            key: "end_chamfer_angle",
            value: f64::INFINITY,
        };
        assert_eq!(
            angle.to_string(),
            "`end_chamfer_angle` must be a number of more than 0 and less than 90 (degrees)"
        );
        assert_eq!(PlanRefusal::NotAProfile(ProfileWhy::NotFitted).key(), None);
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

    #[test]
    fn the_words_are_todays() {
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
        // Every one of the twenty-three forms is written out above.
        let kinds: HashSet<_> = forms
            .iter()
            .map(|(r, _)| std::mem::discriminant(r))
            .collect();
        assert_eq!(kinds.len(), 23);
        for (refusal, words) in &forms {
            assert_eq!(refusal.to_string(), *words, "{refusal:?}");
            assert_eq!(
                TeethFailure::Refused(*refusal).to_string(),
                *words,
                "{refusal:?}"
            );
        }
        assert_eq!(plain(1e-9), "1e-9");
        assert_eq!(plain(-0.0), "0");
        assert_eq!(plain(2.5), "2.5");
    }

    #[test]
    fn kinds_keys_and_the_error() {
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
        failures.extend(forms().iter().map(|(r, _)| TeethFailure::Refused(*r)));
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
                    // The key is the one today's parse finds in the words.
                    assert_eq!(r.key(), old_key(&r.to_string()), "{r:?}");
                    assert_eq!(f.key(), r.key(), "{r:?}");
                    assert_eq!(f.why(), None, "{r:?}");
                }
                TeethFailure::Invalid(w) => {
                    assert_eq!(f.why(), Some(*w));
                    assert_eq!(f.key(), None);
                }
                _ => {
                    assert_eq!(f.key(), None, "{f:?}");
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
        // Both forms of a bound not proved, with a key and without, are unsupported.
        for key in [Some("root_fillet"), None] {
            let keyed = TeethFailure::Refused(PlanRefusal::Unproved {
                key,
                what: FILLET_SEPARATION,
                asked: 0.0,
                reached: -1e-12,
            });
            assert_eq!(keyed.kind(), ErrorKind::Unsupported, "{key:?}");
            assert_eq!(keyed.key(), key);
        }
        assert_eq!(SolidError::sm_err("teeth::x").teeth_failure(), None);
    }
}
