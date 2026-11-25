//! The shaft spline's record data and the kernel's (`solidcore::teeth`), field for field: the
//! numbers go to the kernel, the plan and its outlines come back, and an outline's pieces go
//! both ways.

use solidcore::error::teeth as kt;
use solidcore::teeth as k;
use solidcore::teeth::master_spline::Span;
use solidcore::teeth::outline::SpaceSection as KernelSection;
use solidcore::teeth::seat as kseat;
use solidcore::teeth::section as ksection;
use solidcore::teeth::shaft_spline as ks;
use solidcore::SolidError;
use solidcore_record::shaft_spline::{
    AxisGap, ChamferRefusal, ChamferSection, ChamferSize, ChordGap, CutterSection, EndChamfer,
    LoopPiece, MasterArc, MasterPlaced, OutlineLoop, OutlinePiece, PlanRefusal, ProfileWhy,
    RequiredUnit, RoundFace, SeatFrame, SeatRefusal, SpaceRegion, SpaceSection, SpacedTooth,
    SplineCorner, SplineForm, SplineMade, SplineMake, SplineMaster, SplineNumbers, SplinePlan,
    SplineQuantity, SplineRefusal, ToothOutline, ToothRole, ToothWhy,
};

use crate::{record_plane, record_seg2, record_vec2, record_vec3, vec3};

/// A record kind of teeth as the kernel's.
#[must_use]
pub fn form(f: SplineForm) -> ks::SplineForm {
    match f {
        SplineForm::Involute => ks::SplineForm::Involute,
        SplineForm::Straight => ks::SplineForm::Straight,
        SplineForm::Serration => ks::SplineForm::Serration,
    }
}

/// A record choice of the new part as the kernel's.
#[must_use]
pub fn make(m: SplineMake) -> ks::SplineMake {
    match m {
        SplineMake::Shaft => ks::SplineMake::Shaft,
        SplineMake::Hub => ks::SplineMake::Hub,
        SplineMake::Both => ks::SplineMake::Both,
    }
}

/// A record tip corner as the kernel's.
#[must_use]
pub fn corner(c: SplineCorner) -> ks::SplineCorner {
    match c {
        SplineCorner::Plain => ks::SplineCorner::Plain,
        SplineCorner::Chamfered => ks::SplineCorner::Chamfered,
        SplineCorner::Rounded => ks::SplineCorner::Rounded,
    }
}

/// A record master spline as the kernel's.
#[must_use]
pub fn master(m: SplineMaster) -> ks::SplineMaster {
    match m {
        SplineMaster::None => ks::SplineMaster::None,
        SplineMaster::ToothOut => ks::SplineMaster::ToothOut,
        SplineMaster::WideTooth => ks::SplineMaster::WideTooth,
    }
}

/// The record's numbers as the kernel's. Which parts take the spline (`with_hub`, `with_shaft`)
/// is the caller's own business and has no counterpart in the kernel.
#[must_use]
pub fn numbers(n: &SplineNumbers) -> ks::SplineNumbers {
    ks::SplineNumbers {
        form: form(n.form),
        make: make(n.make),
        teeth: n.teeth,
        length: n.length,
        module: n.module,
        pressure_angle: n.pressure_angle,
        addendum: n.addendum,
        dedendum: n.dedendum,
        root_fillet: n.root_fillet,
        profile_shift: n.profile_shift,
        thinning: n.thinning,
        hub_addendum: n.hub_addendum,
        tip_clearance: n.tip_clearance,
        form_clearance: n.form_clearance,
        major_diameter: n.major_diameter,
        minor_diameter: n.minor_diameter,
        tooth_width: n.tooth_width,
        flank_angle: n.flank_angle,
        root_round: n.root_round,
        tip_corner: corner(n.tip_corner),
        tip_size: n.tip_size,
        major_clearance: n.major_clearance,
        minor_clearance: n.minor_clearance,
        hub_tip_round: n.hub_tip_round,
        side_clearance: n.side_clearance,
        hub_root_round: n.hub_root_round,
        hub_diameter: n.hub_diameter,
        master: master(n.master),
        master_angle: n.master_angle,
        cutter_diameter: n.cutter_diameter,
        end_chamfer: n.end_chamfer,
        entry_chamfer: n.entry_chamfer,
        end_chamfer_distance2: n.end_chamfer_distance2,
        end_chamfer_angle: n.end_chamfer_angle,
        entry_chamfer_distance2: n.entry_chamfer_distance2,
        entry_chamfer_angle: n.entry_chamfer_angle,
    }
}

/// The kernel's report of what a spline comes to as the record's.
#[must_use]
pub fn record_made(m: &ks::SplineMade) -> SplineMade {
    SplineMade {
        teeth: m.teeth,
        shaft_major_diameter: m.shaft_major_diameter,
        shaft_minor_diameter: m.shaft_minor_diameter,
        hub_major_diameter: m.hub_major_diameter,
        hub_minor_diameter: m.hub_minor_diameter,
        pitch_diameter: m.pitch_diameter,
        base_diameter: m.base_diameter,
        form_diameter: m.form_diameter,
        undercut: m.undercut,
        sharp_tip_diameter: m.sharp_tip_diameter,
        side_clearance_each: m.side_clearance_each,
        tip_gap: m.tip_gap,
        root_gap: m.root_gap,
        form_gap: m.form_gap,
        turning_play: m.turning_play,
        positions: m.positions,
        master_tooth: m.master_tooth,
        master_turn: m.master_turn,
        flank_tolerance: m.flank_tolerance,
    }
}

/// A refusal of the kernel's tooth functions as the record's: the tooth failure as data, or for
/// another error of the kernel's, the routine that refused.
#[must_use]
pub fn record_refusal(e: &SolidError) -> SplineRefusal {
    e.teeth_failure().map_or_else(
        || SplineRefusal::Kernel {
            operation: e.operation().to_string(),
        },
        record_failure,
    )
}

/// A kernel tooth failure as the record's.
#[must_use]
pub fn record_failure(f: &kt::TeethFailure) -> SplineRefusal {
    match *f {
        kt::TeethFailure::OutOfRange {
            what,
            value,
            above,
            up_to,
        } => SplineRefusal::OutOfRange {
            what: what.to_string(),
            value,
            above,
            up_to,
        },
        kt::TeethFailure::Invalid(why) => SplineRefusal::Invalid {
            why: record_why(why),
        },
        kt::TeethFailure::Refused(ref refusal) => SplineRefusal::Refused {
            refusal: record_plan_refusal(refusal),
        },
        kt::TeethFailure::NotProved {
            what,
            asked,
            reached,
        } => SplineRefusal::NotProved {
            what: what.to_string(),
            asked,
            reached,
        },
    }
}

/// A kernel refusal of the shaft-spline planner as the record's.
#[must_use]
pub fn record_plan_refusal(r: &kt::PlanRefusal) -> PlanRefusal {
    let q = record_quantity;
    match *r {
        kt::PlanRefusal::Number {
            quantity,
            what,
            value,
            above,
            up_to,
        } => PlanRefusal::Number {
            quantity: quantity.map(q),
            what: what.to_string(),
            value,
            above,
            up_to,
        },
        kt::PlanRefusal::Reason { quantity, why } => PlanRefusal::Reason {
            quantity: quantity.map(q),
            why: record_why(why),
        },
        kt::PlanRefusal::Unproved {
            quantity,
            what,
            asked,
            reached,
        } => PlanRefusal::Unproved {
            quantity: quantity.map(q),
            what: what.to_string(),
            asked,
            reached,
        },
        kt::PlanRefusal::MasterToothOut => PlanRefusal::MasterToothOut,
        kt::PlanRefusal::MasterWideTooth => PlanRefusal::MasterWideTooth,
        kt::PlanRefusal::NotANumber { quantity } => PlanRefusal::NotANumber {
            quantity: q(quantity),
        },
        kt::PlanRefusal::Required { quantity, unit } => PlanRefusal::Required {
            quantity: q(quantity),
            unit: record_unit(unit),
        },
        kt::PlanRefusal::LengthRange { least, most, value } => {
            PlanRefusal::LengthRange { least, most, value }
        }
        kt::PlanRefusal::HubDiameterWithoutHub => PlanRefusal::HubDiameterWithoutHub,
        kt::PlanRefusal::HubDiameterRequired => PlanRefusal::HubDiameterRequired,
        kt::PlanRefusal::MasterAngleWithoutMaster => PlanRefusal::MasterAngleWithoutMaster,
        kt::PlanRefusal::MasterAngleRange { value } => PlanRefusal::MasterAngleRange { value },
        kt::PlanRefusal::ChamferRange {
            quantity,
            most,
            value,
        } => PlanRefusal::ChamferRange {
            quantity: q(quantity),
            most,
            value,
        },
        kt::PlanRefusal::CutterNotANumber { most } => PlanRefusal::CutterNotANumber { most },
        kt::PlanRefusal::CutterTooSmall { at_least } => PlanRefusal::CutterTooSmall { at_least },
        kt::PlanRefusal::HubDiameterTooLarge { most, value } => {
            PlanRefusal::HubDiameterTooLarge { most, value }
        }
        kt::PlanRefusal::NoRim { at_least } => PlanRefusal::NoRim { at_least },
        kt::PlanRefusal::ChamferShapeWithout { quantity, chamfer } => {
            PlanRefusal::ChamferShapeWithout {
                quantity: q(quantity),
                chamfer: q(chamfer),
            }
        }
        kt::PlanRefusal::ChamferBothShapes {
            distance2,
            angle,
            chamfer,
        } => PlanRefusal::ChamferBothShapes {
            distance2: q(distance2),
            angle: q(angle),
            chamfer: q(chamfer),
        },
        kt::PlanRefusal::ChamferDistanceRange {
            quantity,
            most,
            value,
        } => PlanRefusal::ChamferDistanceRange {
            quantity: q(quantity),
            most,
            value,
        },
        kt::PlanRefusal::ChamferAngleRange { quantity, value } => PlanRefusal::ChamferAngleRange {
            quantity: q(quantity),
            value,
        },
        kt::PlanRefusal::NotAProfile(why) => PlanRefusal::NotAProfile {
            why: record_profile_why(why),
        },
    }
}

/// Why a kernel tooth outline is no profile, as the record's.
#[must_use]
pub fn record_profile_why(w: kt::ProfileWhy) -> ProfileWhy {
    match w {
        kt::ProfileWhy::NotFitted => ProfileWhy::NotFitted,
        kt::ProfileWhy::Spline(why) => ProfileWhy::Spline(record_why(why)),
    }
}

/// A kernel unit of a required number as the record's.
#[must_use]
pub fn record_unit(u: kt::RequiredUnit) -> RequiredUnit {
    match u {
        kt::RequiredUnit::Mm => RequiredUnit::Mm,
        kt::RequiredUnit::MmBothSidesOnPitch => RequiredUnit::MmBothSidesOnPitch,
        kt::RequiredUnit::MmEachSideSquareToFlanks => RequiredUnit::MmEachSideSquareToFlanks,
        kt::RequiredUnit::MmDownTheFlank => RequiredUnit::MmDownTheFlank,
        kt::RequiredUnit::MmRoundRadius => RequiredUnit::MmRoundRadius,
    }
}

/// A kernel number of a shaft spline, as a refusal names it, as the record's.
#[must_use]
pub fn record_quantity(q: kt::SplineQuantity) -> SplineQuantity {
    match q {
        kt::SplineQuantity::Teeth => SplineQuantity::Teeth,
        kt::SplineQuantity::Length => SplineQuantity::Length,
        kt::SplineQuantity::Module => SplineQuantity::Module,
        kt::SplineQuantity::PressureAngle => SplineQuantity::PressureAngle,
        kt::SplineQuantity::Addendum => SplineQuantity::Addendum,
        kt::SplineQuantity::Dedendum => SplineQuantity::Dedendum,
        kt::SplineQuantity::RootFillet => SplineQuantity::RootFillet,
        kt::SplineQuantity::ProfileShift => SplineQuantity::ProfileShift,
        kt::SplineQuantity::Thinning => SplineQuantity::Thinning,
        kt::SplineQuantity::HubAddendum => SplineQuantity::HubAddendum,
        kt::SplineQuantity::TipClearance => SplineQuantity::TipClearance,
        kt::SplineQuantity::FormClearance => SplineQuantity::FormClearance,
        kt::SplineQuantity::MajorDiameter => SplineQuantity::MajorDiameter,
        kt::SplineQuantity::MinorDiameter => SplineQuantity::MinorDiameter,
        kt::SplineQuantity::ToothWidth => SplineQuantity::ToothWidth,
        kt::SplineQuantity::FlankAngle => SplineQuantity::FlankAngle,
        kt::SplineQuantity::RootRound => SplineQuantity::RootRound,
        kt::SplineQuantity::TipSize => SplineQuantity::TipSize,
        kt::SplineQuantity::MajorClearance => SplineQuantity::MajorClearance,
        kt::SplineQuantity::MinorClearance => SplineQuantity::MinorClearance,
        kt::SplineQuantity::HubTipRound => SplineQuantity::HubTipRound,
        kt::SplineQuantity::SideClearance => SplineQuantity::SideClearance,
        kt::SplineQuantity::HubRootRound => SplineQuantity::HubRootRound,
        kt::SplineQuantity::HubDiameter => SplineQuantity::HubDiameter,
        kt::SplineQuantity::MasterSpline => SplineQuantity::MasterSpline,
        kt::SplineQuantity::MasterAngle => SplineQuantity::MasterAngle,
        kt::SplineQuantity::CutterDiameter => SplineQuantity::CutterDiameter,
        kt::SplineQuantity::EndChamfer => SplineQuantity::EndChamfer,
        kt::SplineQuantity::EntryChamfer => SplineQuantity::EntryChamfer,
        kt::SplineQuantity::EndChamferDistance2 => SplineQuantity::EndChamferDistance2,
        kt::SplineQuantity::EndChamferAngle => SplineQuantity::EndChamferAngle,
        kt::SplineQuantity::EntryChamferDistance2 => SplineQuantity::EntryChamferDistance2,
        kt::SplineQuantity::EntryChamferAngle => SplineQuantity::EntryChamferAngle,
    }
}

/// A kernel reason why numbers make no teeth as the record's.
#[must_use]
pub fn record_why(w: kt::Why) -> ToothWhy {
    match w {
        kt::Why::NotASpline => ToothWhy::NotASpline,
        kt::Why::SourceTooCoarse => ToothWhy::SourceTooCoarse,
        kt::Why::ToleranceTooFineForSize => ToothWhy::ToleranceTooFineForSize,
        kt::Why::SplineNotWithin => ToothWhy::SplineNotWithin,
        kt::Why::FittedNotFinite => ToothWhy::FittedNotFinite,
        kt::Why::CurveHasNoLength => ToothWhy::CurveHasNoLength,
        kt::Why::FitNotSolvable => ToothWhy::FitNotSolvable,
        kt::Why::SystemNotSquare => ToothWhy::SystemNotSquare,
        kt::Why::TooManyControlPoints => ToothWhy::TooManyControlPoints,
        kt::Why::CurvePointCount => ToothWhy::CurvePointCount,
        kt::Why::RunNotOnePiece => ToothWhy::RunNotOnePiece,
        kt::Why::RunNotJoined => ToothWhy::RunNotJoined,
        kt::Why::RunEmpty => ToothWhy::RunEmpty,
        kt::Why::RunNotFinite => ToothWhy::RunNotFinite,
        kt::Why::RunTooLong => ToothWhy::RunTooLong,
        kt::Why::FilletMissesFlank => ToothWhy::FilletMissesFlank,
        kt::Why::ToothThickness => ToothWhy::ToothThickness,
        kt::Why::RootAboveTipRound => ToothWhy::RootAboveTipRound,
        kt::Why::RootInsideAxis => ToothWhy::RootInsideAxis,
        kt::Why::TipRoundsOverlap => ToothWhy::TipRoundsOverlap,
        kt::Why::CutThrough => ToothWhy::CutThrough,
        kt::Why::FilletsCross => ToothWhy::FilletsCross,
        kt::Why::NoFlankLeft => ToothWhy::NoFlankLeft,
        kt::Why::PointedBelowTip => ToothWhy::PointedBelowTip,
        kt::Why::SpacesAsWideAsPitch => ToothWhy::SpacesAsWideAsPitch,
        kt::Why::TipsBelowBase => ToothWhy::TipsBelowBase,
        kt::Why::RootsInsideRound => ToothWhy::RootsInsideRound,
        kt::Why::SpacesMeetAtTips => ToothWhy::SpacesMeetAtTips,
        kt::Why::RootRoundsOverlap => ToothWhy::RootRoundsOverlap,
        kt::Why::HubReachesFillet => ToothWhy::HubReachesFillet,
        kt::Why::HubRoundsReachTips => ToothWhy::HubRoundsReachTips,
        kt::Why::AngleNotANumber => ToothWhy::AngleNotANumber,
        kt::Why::NoCornerOnSmallest => ToothWhy::NoCornerOnSmallest,
        kt::Why::NoCornerOnLargest => ToothWhy::NoCornerOnLargest,
        kt::Why::MasterAngleRange => ToothWhy::MasterAngleRange,
        kt::Why::NotAShaftAndHub => ToothWhy::NotAShaftAndHub,
        kt::Why::HubStartNotANumber => ToothWhy::HubStartNotANumber,
        kt::Why::BodyNotFinite => ToothWhy::BodyNotFinite,
        kt::Why::BodyCrossesItself => ToothWhy::BodyCrossesItself,
        kt::Why::PartNotFinite => ToothWhy::PartNotFinite,
        kt::Why::MeasureTooIntricate => ToothWhy::MeasureTooIntricate,
        kt::Why::CurveTooFine => ToothWhy::CurveTooFine,
        kt::Why::CrossingCheckTooIntricate => ToothWhy::CrossingCheckTooIntricate,
        kt::Why::OutlineNotFinite => ToothWhy::OutlineNotFinite,
        kt::Why::PiecesDoNotJoin => ToothWhy::PiecesDoNotJoin,
        kt::Why::OutlineTooFine => ToothWhy::OutlineTooFine,
        kt::Why::TooCoarse => ToothWhy::TooCoarse,
        kt::Why::NotCounterClockwise => ToothWhy::NotCounterClockwise,
        kt::Why::CrossesItself => ToothWhy::CrossesItself,
        kt::Why::NoTopNearAngle => ToothWhy::NoTopNearAngle,
        kt::Why::TopsMeet => ToothWhy::TopsMeet,
        kt::Why::ToothAngleNotANumber => ToothWhy::ToothAngleNotANumber,
        kt::Why::TooMuchTurning => ToothWhy::TooMuchTurning,
        kt::Why::FirstAxisNotFinite => ToothWhy::FirstAxisNotFinite,
        kt::Why::SecondNotFinite => ToothWhy::SecondNotFinite,
        kt::Why::TurnsFreely => ToothWhy::TurnsFreely,
        kt::Why::FlankMissesCircles => ToothWhy::FlankMissesCircles,
        kt::Why::ChamferTooLarge => ToothWhy::ChamferTooLarge,
        kt::Why::MajorRoundTooLarge => ToothWhy::MajorRoundTooLarge,
        kt::Why::MinorRoundTooLarge => ToothWhy::MinorRoundTooLarge,
        kt::Why::TipCornersMeet => ToothWhy::TipCornersMeet,
        kt::Why::TeethMeetAtRoot => ToothWhy::TeethMeetAtRoot,
        kt::Why::HubRootRoundsMeet => ToothWhy::HubRootRoundsMeet,
        kt::Why::HubTeethTooNarrow => ToothWhy::HubTeethTooNarrow,
        kt::Why::NoStraightFlank => ToothWhy::NoStraightFlank,
        kt::Why::HubRootRoundsTooClose => ToothWhy::HubRootRoundsTooClose,
        kt::Why::HubTipsTooClose => ToothWhy::HubTipsTooClose,
        kt::Why::MinorNotSmaller => ToothWhy::MinorNotSmaller,
        kt::Why::MinorClearanceTooLarge => ToothWhy::MinorClearanceTooLarge,
        kt::Why::FlanksParallel => ToothWhy::FlanksParallel,
        kt::Why::FlanksNearlyParallel => ToothWhy::FlanksNearlyParallel,
    }
}

/// A record role of an outline's piece as the kernel's.
#[must_use]
pub fn role(r: ToothRole) -> k::Role {
    match r {
        ToothRole::Flank => k::Role::Flank,
        ToothRole::Fillet => k::Role::Fillet,
        ToothRole::Tip => k::Role::Tip,
        ToothRole::Root => k::Role::Root,
    }
}

/// A kernel role of an outline's piece as the record's.
#[must_use]
pub fn record_role(r: k::Role) -> ToothRole {
    match r {
        k::Role::Flank => ToothRole::Flank,
        k::Role::Fillet => ToothRole::Fillet,
        k::Role::Tip => ToothRole::Tip,
        k::Role::Root => ToothRole::Root,
    }
}

/// A record piece of an outline as the kernel's.
#[must_use]
pub fn piece(p: &OutlinePiece) -> k::Piece {
    match p {
        OutlinePiece::Arc {
            centre,
            radius,
            start,
            sweep,
            role: r,
        } => k::Piece::Arc {
            centre: *centre,
            radius: *radius,
            start: *start,
            sweep: *sweep,
            role: role(*r),
        },
        OutlinePiece::Line { from, to, role: r } => k::Piece::Line {
            from: *from,
            to: *to,
            role: role(*r),
        },
        OutlinePiece::Curve { points, role: r } => k::Piece::Curve {
            points: points.clone(),
            role: role(*r),
        },
        OutlinePiece::Spline {
            degree,
            knots,
            control,
            role: r,
        } => k::Piece::Spline {
            degree: *degree,
            knots: knots.clone(),
            control: control.clone(),
            role: role(*r),
        },
    }
}

/// A kernel piece of an outline as the record's.
#[must_use]
pub fn record_piece(p: &k::Piece) -> OutlinePiece {
    match p {
        k::Piece::Arc {
            centre,
            radius,
            start,
            sweep,
            role,
        } => OutlinePiece::Arc {
            centre: *centre,
            radius: *radius,
            start: *start,
            sweep: *sweep,
            role: record_role(*role),
        },
        k::Piece::Line { from, to, role } => OutlinePiece::Line {
            from: *from,
            to: *to,
            role: record_role(*role),
        },
        k::Piece::Curve { points, role } => OutlinePiece::Curve {
            points: points.clone(),
            role: record_role(*role),
        },
        k::Piece::Spline {
            degree,
            knots,
            control,
            role,
        } => OutlinePiece::Spline {
            degree: *degree,
            knots: knots.clone(),
            control: control.clone(),
            role: record_role(*role),
        },
    }
}

/// The record's pieces of one tooth as the kernel's.
#[must_use]
pub fn tooth(pieces: &[OutlinePiece]) -> Vec<k::Piece> {
    pieces.iter().map(piece).collect()
}

/// A kernel outline as the record's.
#[must_use]
pub fn record_outline(o: &k::Outline) -> ToothOutline {
    let (least_radius, most_radius) = o.radii();
    ToothOutline {
        teeth: o.teeth(),
        pitch_angle: o.pitch_angle(),
        tooth: o.tooth().iter().map(record_piece).collect(),
        tolerance: o.tolerance(),
        least_radius,
        most_radius,
    }
}

/// A kernel arc of a master spline as the record's.
#[must_use]
pub fn record_arc(s: &Span) -> MasterArc {
    MasterArc {
        radius: s.radius,
        start: s.start,
        end: s.end,
    }
}

/// A kernel plan as the record's.
#[must_use]
pub fn record_plan(p: &ks::SplinePlan) -> SplinePlan {
    SplinePlan {
        shaft: record_outline(&p.pair.first.outline),
        hub: record_outline(&p.pair.second.outline),
        hub_start: p.pair.second_start,
        made: record_made(&p.made),
        master: p
            .master
            .as_ref()
            .zip(p.hub_arc_on_shaft.as_ref())
            .map(|(m, on_shaft)| MasterPlaced {
                tooth: m.placement.tooth,
                turn: m.placement.turn,
                middle: m.middle,
                shaft_arc: record_arc(&m.shaft_arc),
                hub_arc: record_arc(&m.hub_arc),
                hub_arc_on_shaft: record_arc(on_shaft),
            }),
        tooth_pitch: p.tooth_pitch,
        tooth_at: p.tooth_at,
        end_chamfer: p.end_chamfer.as_ref().map(record_chamfer_size),
        entry_chamfer: p.entry_chamfer.as_ref().map(record_chamfer_size),
        shaft_seam: p.shaft_seam,
        hub_seam: p.hub_seam,
        spaced_teeth: p.spaced_teeth.iter().map(record_spaced_tooth).collect(),
    }
}

/// A kernel tooth with a space after it as the record's.
#[must_use]
pub fn record_spaced_tooth(t: &ks::SpacedTooth) -> SpacedTooth {
    SpacedTooth {
        tooth: t.tooth,
        angle: t.angle,
    }
}

/// A kernel chamfer as made as the record's.
#[must_use]
pub fn record_chamfer_size(c: &ks::ChamferSize) -> ChamferSize {
    ChamferSize {
        axial: c.axial,
        radial: c.radial,
    }
}

/// A record round face as the kernel's.
#[must_use]
pub fn round_face(f: &RoundFace) -> kseat::RoundFace {
    kseat::RoundFace {
        axis_point: vec3(f.axis_point),
        axis: vec3(f.axis),
        radius: f.radius,
        start: f.start,
        end: f.end,
    }
}

/// A kernel seat as the record's.
#[must_use]
pub fn record_seat_frame(s: &kseat::SeatFrame) -> SeatFrame {
    SeatFrame {
        plane: record_plane(&s.plane),
        inward: s.inward,
        room: s.room,
        at: record_vec3(s.at),
    }
}

/// A kernel refusal of a seat as the record's.
#[must_use]
pub fn record_seat_refusal(r: kseat::SeatRefusal) -> SeatRefusal {
    match r {
        kseat::SeatRefusal::NoDirection => SeatRefusal::NoDirection,
        kseat::SeatRefusal::NoPlane => SeatRefusal::NoPlane,
        kseat::SeatRefusal::NoSize => SeatRefusal::NoSize,
    }
}

/// A kernel axis gap as the record's.
#[must_use]
pub fn record_axis_gap(g: kseat::AxisGap) -> AxisGap {
    AxisGap {
        lean: g.lean,
        off: g.off,
    }
}

/// A kernel chord gap as the record's.
#[must_use]
pub fn record_chord_gap(g: kseat::ChordGap) -> ChordGap {
    ChordGap {
        length: g.length,
        lean: g.lean,
    }
}

/// A kernel piece of a whole outline's loop as the record's.
#[must_use]
pub fn record_loop_piece(p: &ksection::LoopPiece) -> LoopPiece {
    LoopPiece {
        top: p.top,
        role: record_role(p.role),
        middle: p.middle,
        corner: p.corner,
        first: p.first,
        segs: p.segs,
    }
}

/// A kernel outline's loop as the record's.
#[must_use]
pub fn record_outline_loop(l: &ksection::OutlineLoop) -> OutlineLoop {
    OutlineLoop {
        segs: l.segs.iter().map(record_seg2).collect(),
        joints: l.joints.iter().copied().map(record_vec2).collect(),
        pieces: l.pieces.iter().map(record_loop_piece).collect(),
    }
}

/// A kernel cutter's section as the record's.
#[must_use]
pub fn record_cutter_section(s: &ksection::CutterSection) -> CutterSection {
    CutterSection {
        segs: s.segs.iter().map(record_seg2).collect(),
        spans: s.spans.clone(),
        middle: s.middle,
        from: s.from,
        to: s.to,
        joints: s.joints.iter().copied().map(record_vec2).collect(),
        axis_at: record_vec2(s.axis_at),
        axis_dir: record_vec2(s.axis_dir),
    }
}

/// A record chamfer at the end of a round face as the kernel's.
#[must_use]
pub fn end_chamfer(c: &EndChamfer) -> ksection::EndChamfer {
    ksection::EndChamfer {
        radius: c.radius,
        radial: c.radial,
        axial: c.axial,
        past: c.past,
        flush: c.flush,
    }
}

/// A kernel chamfer's section as the record's.
#[must_use]
pub fn record_chamfer_section(s: &ksection::ChamferSection) -> ChamferSection {
    ChamferSection {
        points: s.points.iter().copied().map(record_vec2).collect(),
        slope: s.slope,
    }
}

/// A kernel refusal of a chamfer's section as the record's.
#[must_use]
pub fn record_chamfer_refusal(r: ksection::ChamferRefusal) -> ChamferRefusal {
    match r {
        ksection::ChamferRefusal::NotFinite => ChamferRefusal::NotFinite,
        ksection::ChamferRefusal::NoSize => ChamferRefusal::NoSize,
        ksection::ChamferRefusal::NoCorner => ChamferRefusal::NoCorner,
        ksection::ChamferRefusal::ReachesAxis => ChamferRefusal::ReachesAxis,
    }
}

/// A kernel cross-section of a tooth space as the record's.
#[must_use]
pub fn record_space_region(r: &ksection::SpaceRegion) -> SpaceRegion {
    SpaceRegion {
        segs: r.segs.iter().map(record_seg2).collect(),
        spans: r.spans.clone(),
        joints: r.joints.iter().copied().map(record_vec2).collect(),
    }
}

/// A kernel section of a tooth space as the record's, with the tolerance of the outline it was
/// made from (`tolerance`), which its floor's splines are held to.
#[must_use]
pub fn record_section(s: &KernelSection, tolerance: f64) -> SpaceSection {
    SpaceSection {
        floor: s.floor.iter().map(record_piece).collect(),
        start: s.corners.0,
        end: s.corners.1,
        apart: s.apart,
        tolerance,
    }
}
