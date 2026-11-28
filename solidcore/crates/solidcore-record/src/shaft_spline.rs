//! A spline on a shaft and in its hub as data: the numbers it is planned from, the outlines the
//! kernel makes of them (`solidcore::teeth`), and the numbers the outlines come to. Plain data:
//! the meaning of each number, every check and every outline are the kernel's.
use serde::{Deserialize, Serialize};

use crate::{Plane, Seg2, Vec2, Vec3};

/// The kind of teeth of a shaft spline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplineForm {
    /// Involute flanks, as a gear's.
    #[default]
    Involute,
    /// Straight-sided teeth of one width.
    Straight,
    /// Serrations: straight flanks at an angle to each other.
    Serration,
}

/// Which new part a shaft spline makes when no face is picked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplineMake {
    /// The splined shaft.
    #[default]
    Shaft,
    /// The hub that fits it.
    Hub,
    /// Both.
    Both,
}

/// The finish of the corner where a straight-sided tooth's flank meets the major circle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplineCorner {
    /// Left sharp.
    #[default]
    Plain,
    /// A chamfer `tip_size` down the flank.
    Chamfered,
    /// A round of radius `tip_size`.
    Rounded,
}

/// The master spline: one tooth made different so that the hub goes on one way only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplineMaster {
    /// Every tooth alike.
    #[default]
    None,
    /// A tooth left out of the shaft, its space in the hub left uncut.
    ToothOut,
    /// A space of the shaft left uncut (a wide tooth), a tooth left out of the hub.
    WideTooth,
}

/// A shaft spline's numbers as plain values: lengths in millimetres, angles in degrees. A number
/// left out takes the kernel's own value, or is required, as the kernel's plan says.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SplineNumbers {
    /// The kind of teeth.
    pub form: SplineForm,
    /// Which new part is made.
    pub make: SplineMake,
    /// The count of teeth.
    pub teeth: u32,
    /// How far the teeth run along the axis.
    pub length: f64,
    /// Involute: the module, the pitch diameter divided by the count of teeth.
    pub module: Option<f64>,
    /// Involute: the pressure angle.
    pub pressure_angle: Option<f64>,
    /// Involute: the height of the shaft's teeth above the pitch circle, in modules.
    pub addendum: Option<f64>,
    /// Involute: the depth of the shaft's spaces below the pitch circle, in modules.
    pub dedendum: Option<f64>,
    /// Involute: the radius of the cutting tool's tip, in modules.
    pub root_fillet: Option<f64>,
    /// Involute: the profile shift, in modules.
    pub profile_shift: Option<f64>,
    /// Involute: what is taken off each tooth's thickness on the pitch circle.
    pub thinning: Option<f64>,
    /// Involute: the height of the hub's teeth inside the pitch circle, in modules.
    pub hub_addendum: Option<f64>,
    /// Involute: the gap between the shaft's tips and the hub's roots.
    pub tip_clearance: Option<f64>,
    /// Involute: the least gap between the hub's tips and the start of the shaft's root fillet.
    pub form_clearance: Option<f64>,
    /// Straight-sided and serration: the diameter over the shaft's tips.
    pub major_diameter: Option<f64>,
    /// Straight-sided and serration: the diameter at the bottoms of the shaft's spaces.
    pub minor_diameter: Option<f64>,
    /// Straight-sided: the width of a tooth.
    pub tooth_width: Option<f64>,
    /// Serration: the angle between the two flanks of a tooth.
    pub flank_angle: Option<f64>,
    /// Straight-sided and serration: the round where a flank meets the minor circle.
    pub root_round: Option<f64>,
    /// Straight-sided and serration: the finish of the tip corners.
    pub tip_corner: SplineCorner,
    /// The size of a chamfered or rounded tip corner.
    pub tip_size: Option<f64>,
    /// Straight-sided and serration: the gap at the major circle.
    pub major_clearance: Option<f64>,
    /// Straight-sided and serration: the gap at the minor circle.
    pub minor_clearance: Option<f64>,
    /// Straight-sided and serration: the round on the tips of the hub's teeth.
    pub hub_tip_round: Option<f64>,
    /// The play between the flanks.
    pub side_clearance: Option<f64>,
    /// The round at the roots of the hub's teeth.
    pub hub_root_round: Option<f64>,
    /// The outside diameter of a new hub.
    pub hub_diameter: Option<f64>,
    /// The master spline.
    pub master: SplineMaster,
    /// Where the middle of the master spline sits.
    pub master_angle: Option<f64>,
    /// The diameter of the cutter that ends the tooth spaces of a shaft.
    pub cutter_diameter: Option<f64>,
    /// The chamfer on the ends of a shaft's teeth.
    pub end_chamfer: Option<f64>,
    /// The chamfer where the teeth run out of a hub's bore.
    pub entry_chamfer: Option<f64>,
    /// How far the end chamfer runs across the end (mm), where that is not `end_chamfer` itself.
    pub end_chamfer_distance2: Option<f64>,
    /// The angle of the end chamfer to the round face (degrees), where it is not 45.
    pub end_chamfer_angle: Option<f64>,
    /// How far the entry chamfer runs across the end (mm), where that is not `entry_chamfer`
    /// itself.
    pub entry_chamfer_distance2: Option<f64>,
    /// The angle of the entry chamfer to the bore (degrees), where it is not 45.
    pub entry_chamfer_angle: Option<f64>,
    /// Whether a hub is made or cut.
    pub with_hub: bool,
    /// Whether a shaft is made or cut.
    pub with_shaft: bool,
}

/// The numbers a shaft spline comes to. Diameters and gaps in millimetres.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SplineMade {
    /// The count of teeth.
    pub teeth: u32,
    /// The diameter over the shaft's tips.
    pub shaft_major_diameter: f64,
    /// The diameter at the bottoms of the shaft's spaces.
    pub shaft_minor_diameter: f64,
    /// The diameter at the bottoms of the hub's spaces.
    pub hub_major_diameter: f64,
    /// The diameter over the tips of the hub's teeth.
    pub hub_minor_diameter: f64,
    /// Involute: the pitch diameter.
    pub pitch_diameter: Option<f64>,
    /// Involute: the base diameter.
    pub base_diameter: Option<f64>,
    /// Involute: the diameter where the shaft's flanks start.
    pub form_diameter: Option<f64>,
    /// Involute: whether the cutting tool undercuts the shaft's teeth.
    pub undercut: Option<bool>,
    /// Serration: the diameter the flanks would meet at.
    pub sharp_tip_diameter: Option<f64>,
    /// The play on each side.
    pub side_clearance_each: f64,
    /// The gap at the tips of the shaft's teeth.
    pub tip_gap: f64,
    /// The gap at the roots of the shaft's teeth.
    pub root_gap: f64,
    /// Involute: the gap at the start of the shaft's root fillet.
    pub form_gap: Option<f64>,
    /// How far the hub turns on the shaft before the flanks touch, in degrees.
    pub turning_play: f64,
    /// In how many positions the hub goes on: the count of teeth, or 1 with a master spline.
    pub positions: u32,
    /// With a master spline: the tooth left out, or the tooth before the space left uncut,
    /// counted counter-clockwise from the tooth on angle 0.
    pub master_tooth: Option<u32>,
    /// With a master spline: how far the whole spline was turned to put it at its angle, in
    /// degrees.
    pub master_turn: Option<f64>,
    /// How far the involute flanks and fillets may be from the true curves.
    pub flank_tolerance: Option<f64>,
}

/// A number of a shaft spline that a refusal is about (the kernel's `SplineQuantity`). Its name
/// in the data is the number's name in [`SplineNumbers`], in lowercase words joined by `_`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

/// What a number the planner requires is measured in (the kernel's `RequiredUnit`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

/// Why numbers that are each acceptable make no teeth (the kernel's tooth `Why`): one variant per
/// reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToothWhy {
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

/// Why a tooth outline cannot be made into a profile of lines, arcs and Bezier pieces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "why", rename_all = "snake_case")]
pub enum ProfileWhy {
    /// A curve of the outline is held as points only, not fitted as a spline.
    NotFitted,
    /// A spline of the outline cannot be cut into Bezier pieces: the tooth module's reason.
    Spline(ToothWhy),
}

/// A refusal of the shaft-spline planner as data (the kernel's `PlanRefusal`): the number it is
/// about and the numbers of the refusal. An application maps the quantity to its own parameter
/// and words the refusal for its users.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlanRefusal {
    /// A tooth module's number out of range, with the spline's number it comes from where the
    /// planner knows it.
    Number {
        /// The spline's number to change, where the planner knows it.
        quantity: Option<SplineQuantity>,
        /// The tooth module's name for the number.
        what: String,
        /// What was given.
        value: f64,
        /// It must be above this.
        above: f64,
        /// It must be at most this.
        up_to: f64,
    },
    /// A tooth module's reason, with the spline's number to change where the planner knows it.
    Reason {
        /// The spline's number to change, where the planner knows it.
        quantity: Option<SplineQuantity>,
        /// The tooth module's reason.
        why: ToothWhy,
    },
    /// A bound a tooth module could not prove, with the spline's number to change where the
    /// planner knows it.
    Unproved {
        /// The spline's number to change, where the planner knows it.
        quantity: Option<SplineQuantity>,
        /// What could not be proved, as the kernel names it.
        what: String,
        /// The bound that had to be met.
        asked: f64,
        /// The best bound proved when the work ran out.
        reached: f64,
    },
    /// A tooth cannot be left out of these teeth: no corner of the teeth lies on the smallest
    /// circle.
    MasterToothOut,
    /// A wide tooth cannot be made on these teeth: no corner of the teeth lies on the largest
    /// circle.
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
        /// The chamfer.
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
    NotAProfile {
        /// Why.
        why: ProfileWhy,
    },
}

impl PlanRefusal {
    /// The number of the spline to change, where the refusal names one.
    #[must_use]
    pub fn quantity(&self) -> Option<SplineQuantity> {
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
            Self::NotAProfile { .. } => None,
        }
    }
}

/// Why a shaft spline's numbers, outlines or sections make nothing, as data (the kernel's
/// `TeethFailure`, or another refusal of the kernel's).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SplineRefusal {
    /// A number of the tooth modules is not finite or lies outside what is accepted: above
    /// `above` and at most `up_to`.
    OutOfRange {
        /// The tooth modules' name for the number.
        what: String,
        /// What was given.
        value: f64,
        /// It must be above this.
        above: f64,
        /// It must be at most this.
        up_to: f64,
    },
    /// Each number is acceptable, but together they do not make teeth.
    Invalid {
        /// Why.
        why: ToothWhy,
    },
    /// The planner's refusal, or one of a tooth profile: the number to change and why.
    Refused {
        /// The refusal.
        refusal: PlanRefusal,
    },
    /// A bound could not be proved within the work allowed.
    NotProved {
        /// What could not be proved, as the kernel names it.
        what: String,
        /// The bound that had to be met.
        asked: f64,
        /// The best bound proved when the work ran out.
        reached: f64,
    },
    /// A refusal of the kernel that is not a tooth module's: the routine that refused.
    Kernel {
        /// The kernel routine that refused, by its path.
        operation: String,
    },
}

impl SplineRefusal {
    /// The number of the spline to change, where the refusal names one.
    #[must_use]
    pub fn quantity(&self) -> Option<SplineQuantity> {
        match self {
            Self::Refused { refusal } => refusal.quantity(),
            Self::OutOfRange { .. }
            | Self::Invalid { .. }
            | Self::NotProved { .. }
            | Self::Kernel { .. } => None,
        }
    }
}

/// What a piece of a tooth's outline is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToothRole {
    /// A working flank.
    Flank,
    /// The round between a flank and the root.
    Fillet,
    /// The top of a tooth, and its corners.
    Tip,
    /// The bottom of the space between two teeth.
    Root,
}

/// One piece of a tooth's outline, in the plane square to the axis. The pieces of a tooth follow
/// each other counter-clockwise around the axis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutlinePiece {
    /// A circular arc from the angle `start` (radians, from the x axis) sweeping `sweep`
    /// (positive: counter-clockwise).
    Arc {
        /// The centre of its circle.
        centre: [f64; 2],
        /// The radius of its circle.
        radius: f64,
        /// The angle it starts at.
        start: f64,
        /// How far it sweeps.
        sweep: f64,
        /// What the piece is.
        role: ToothRole,
    },
    /// A straight line.
    Line {
        /// Where it starts.
        from: [f64; 2],
        /// Where it ends.
        to: [f64; 2],
        /// What the piece is.
        role: ToothRole,
    },
    /// A curve as points close enough together to stand for it.
    Curve {
        /// The points, in order.
        points: Vec<[f64; 2]>,
        /// What the piece is.
        role: ToothRole,
    },
    /// A B-spline curve.
    Spline {
        /// Its degree.
        degree: usize,
        /// Its knots.
        knots: Vec<f64>,
        /// Its control points.
        control: Vec<[f64; 2]>,
        /// What the piece is.
        role: ToothRole,
    },
}

impl OutlinePiece {
    /// What the piece is.
    #[must_use]
    pub fn role(&self) -> ToothRole {
        match self {
            OutlinePiece::Arc { role, .. }
            | OutlinePiece::Line { role, .. }
            | OutlinePiece::Curve { role, .. }
            | OutlinePiece::Spline { role, .. } => *role,
        }
    }
}

/// The outline of a toothed part about its axis: one tooth, and how many times it repeats.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToothOutline {
    /// How many teeth the part has.
    pub teeth: u32,
    /// The angle from one tooth to the next, in radians.
    pub pitch_angle: f64,
    /// The pieces of one tooth, counter-clockwise. The other teeth are this one turned by whole
    /// pitches.
    pub tooth: Vec<OutlinePiece>,
    /// How far a chord between two points of the outline may be from the curve.
    pub tolerance: f64,
    /// The smallest distance of the outline from the axis.
    pub least_radius: f64,
    /// The largest distance of the outline from the axis.
    pub most_radius: f64,
}

/// An arc about the axis: where a master spline closed a space or left a tooth out.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MasterArc {
    /// The radius of the arc's circle.
    pub radius: f64,
    /// The angle it starts at, in radians.
    pub start: f64,
    /// The angle it ends at, in radians.
    pub end: f64,
}

/// A master spline as the kernel placed it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MasterPlaced {
    /// The tooth left out, or the tooth before the space left uncut, counted counter-clockwise
    /// from the tooth on angle 0.
    pub tooth: u32,
    /// How far the whole spline was turned to put the master spline at its angle, in radians.
    pub turn: f64,
    /// The angle of the middle of the master spline, in radians.
    pub middle: f64,
    /// The arc that stands in the shaft's outline for what was changed.
    pub shaft_arc: MasterArc,
    /// The arc that stands in the hub's outline for what was changed.
    pub hub_arc: MasterArc,
    /// `hub_arc` turned by the hub's start, so that it lies over the shaft.
    pub hub_arc_on_shaft: MasterArc,
}

/// What the numbers make, before any body is touched.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SplinePlan {
    /// The shaft's outline, with the master spline cut in and the whole spline turned.
    pub shaft: ToothOutline,
    /// The outline of the hub's opening, as it lies about its own axis.
    pub hub: ToothOutline,
    /// The angle the hub's outline is turned by to lie on the shaft, in radians.
    pub hub_start: f64,
    /// The numbers the spline comes to.
    pub made: SplineMade,
    /// The master spline as the kernel placed it.
    pub master: Option<MasterPlaced>,
    /// The angle from one tooth to the next of the teeth as counted, in radians: a full turn over
    /// the count of teeth. Not the outline's own pitch angle, which is a full turn once a master
    /// spline is cut in.
    pub tooth_pitch: f64,
    /// The angle of the middle of a tooth that stands, in radians counter-clockwise from the x
    /// axis: tooth 0, or with a master spline the tooth opposite it.
    pub tooth_at: f64,
    /// The chamfer on the ends of a shaft's teeth as made; none where `end_chamfer` is left out
    /// or 0.
    pub end_chamfer: Option<ChamferSize>,
    /// The chamfer at the entry of a hub's bore as made; none where `entry_chamfer` is left out
    /// or 0.
    pub entry_chamfer: Option<ChamferSize>,
    /// An angle about the axis, in radians, that lies in a tooth space of the shaft: where a tool
    /// turned once about the axis has its seam, so that the seam is cut away with the space.
    pub shaft_seam: f64,
    /// An angle about the axis, in radians, that lies in a tooth space of the hub (over a tooth of
    /// the shaft), for the seam of a tool turned once about the axis.
    pub hub_seam: f64,
    /// Each tooth of the shaft with a space of its own after it, in order: every tooth but the one
    /// a master spline left out or made wide.
    pub spaced_teeth: Vec<SpacedTooth>,
}

/// A tooth of the shaft with a space of its own after it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SpacedTooth {
    /// Its number, counted counter-clockwise from tooth 0.
    pub tooth: u32,
    /// The angle of its middle, in radians counter-clockwise from the x axis.
    pub angle: f64,
}

/// A chamfer as the numbers make it: how far it runs along the round face, and how far across
/// the end (mm).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChamferSize {
    /// How far it runs along the round face.
    pub axial: f64,
    /// How far it runs across the end.
    pub radial: f64,
}

/// A round face of a part as numbers: a point on its axis, the axis (a unit vector), its radius,
/// and where the face starts and ends along the axis, measured from the axis point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RoundFace {
    /// A point on its axis.
    pub axis_point: Vec3,
    /// The axis, a unit vector.
    pub axis: Vec3,
    /// Its radius.
    pub radius: f64,
    /// Where the face starts along the axis.
    pub start: f64,
    /// Where the face ends along the axis.
    pub end: f64,
}

/// Where a spline sits on a round face.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeatFrame {
    /// The plane square to the axis at the free end: its x is angle 0, its normal (x cross y) the
    /// axis as the world axes orient it.
    pub plane: Plane,
    /// 1.0 when the face runs on from the free end along the plane's normal, -1.0 when against it.
    pub inward: f64,
    /// The length of the face along its axis (`end - start`).
    pub room: f64,
    /// The pick, brought onto the face's cylinder along its radius.
    pub at: Vec3,
}

/// Why a round face gives no seat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeatRefusal {
    /// The axis has no direction.
    NoDirection,
    /// No plane square to the axis can be made.
    NoPlane,
    /// The face has no length or no radius.
    NoSize,
}

/// How one line or axis lies to another.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AxisGap {
    /// The length of the cross product of the two directions (0 when parallel).
    pub lean: f64,
    /// The distance of the point from the axis line.
    pub off: f64,
}

/// A chord against an axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChordGap {
    /// The chord's length.
    pub length: f64,
    /// The length of the cross product of the chord with the axis.
    pub lean: f64,
}

/// A piece of an outline as it lies in the whole loop.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoopPiece {
    /// An arc about the axis on the outline's largest circle.
    pub top: bool,
    /// What the piece is.
    pub role: ToothRole,
    /// The angle of its middle, where it is a top (else 0 plus the tooth's turn).
    pub middle: f64,
    /// The angle of its start, where its first segment is a free-form curve that starts on the
    /// largest circle.
    pub corner: Option<f64>,
    /// The first of the loop's segments that are its.
    pub first: usize,
    /// How many of the loop's segments are its.
    pub segs: usize,
}

/// A whole outline as one closed loop of profile segments.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OutlineLoop {
    /// The segments, counter-clockwise.
    pub segs: Vec<Seg2>,
    /// Where two segments of one curve meet.
    pub joints: Vec<Vec2>,
    /// The pieces of the outline, in the loop's order.
    pub pieces: Vec<LoopPiece>,
}

/// The section a cutter takes out of one tooth space where it leaves the shaft.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CutterSection {
    /// The floor's spans in the floor's order, then three lines: the wall above the tooth before
    /// the space, the top, the wall above the tooth after it.
    pub segs: Vec<Seg2>,
    /// How many of `segs` each piece of the space's floor became, in the floor's order (0 for a
    /// piece too short to draw).
    pub spans: Vec<usize>,
    /// The angle of the space's middle line.
    pub middle: f64,
    /// The corner of the tooth before the space, as it lies with the middle line on angle 0.
    pub from: [f64; 2],
    /// The corner of the tooth after the space, as it lies with the middle line on angle 0.
    pub to: [f64; 2],
    /// Where two spans of one curve meet.
    pub joints: Vec<Vec2>,
    /// A point of the cutter's axis in the section's plane.
    pub axis_at: Vec2,
    /// The direction of the cutter's axis in the section's plane.
    pub axis_dir: Vec2,
}

/// A chamfer at an end of a round face: the corner it takes off lies at `radius`, `past` beyond
/// the face's own end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EndChamfer {
    /// The radius of the round face: the tops of a shaft's teeth, of a hub's.
    pub radius: f64,
    /// How far the chamfer runs across the end.
    pub radial: f64,
    /// How far the chamfer runs along the face.
    pub axial: f64,
    /// How far past the face's end the body's own end lies.
    pub past: f64,
    /// At a step: the tool stops at the end, and at this radius where the chamfer would run
    /// deeper than the step.
    pub flush: Option<f64>,
}

/// The section of a chamfer's tool in a plane through the axis, counter-clockwise, and which of
/// its sides is the slope.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChamferSection {
    /// The corners of the section.
    pub points: Vec<Vec2>,
    /// The side from `points[slope]` to the next corner is the slope.
    pub slope: usize,
}

/// Why a chamfer at the end of a round face has no tool's section.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChamferRefusal {
    /// A number of the chamfer, or the margin, is not finite.
    NotFinite,
    /// The chamfer runs no distance across the end or along the face, or the face has no radius.
    NoSize,
    /// At a step, the body past the end stands as high as the face: there is no corner to take
    /// off.
    NoCorner,
    /// The section would reach the axis or past it: the chamfer runs too far across the end.
    ReachesAxis,
}

/// The whole cross-section of one tooth space out to a circle about the axis, counter-clockwise:
/// what a space that ends square takes out of the shaft.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpaceRegion {
    /// The floor's spans in the floor's order, then three: the line out from the corner of the
    /// tooth before the space, the arc of the circle, the line in to the corner of the tooth
    /// after it.
    pub segs: Vec<Seg2>,
    /// How many of `segs` each piece of the space's floor became, in the floor's order (0 for a
    /// piece too short to draw).
    pub spans: Vec<usize>,
    /// Where two spans of one curve meet.
    pub joints: Vec<Vec2>,
}

/// One tooth space of a shaft as a region closed above by a circle about the axis: what a cutter
/// takes out between two teeth.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpaceSection {
    /// The outline's own pieces between the tops of the two teeth: from the corner of the next
    /// tooth down its side, along the bottom of the space and up the side of the tooth before, to
    /// its corner.
    pub floor: Vec<OutlinePiece>,
    /// Where `floor` starts: the corner of the tooth after the space.
    pub start: [f64; 2],
    /// Where `floor` ends: the corner of the tooth before the space.
    pub end: [f64; 2],
    /// Whether the region stays clear of the regions of the neighbouring spaces.
    pub apart: bool,
    /// The outline's tolerance: how far the Bezier pieces a spline of `floor` becomes may be from
    /// it.
    pub tolerance: f64,
}
