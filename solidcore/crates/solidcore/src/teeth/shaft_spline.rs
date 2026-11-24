//! A spline on a shaft and in its hub from plain numbers: the checks of the numbers, the pair of
//! outlines the tooth modules make of them, the numbers the pair comes to, and a master spline.
//! This is the planning half of a shaft spline: it knows numbers and outlines, not a body, a
//! command or a cut.
//!
//! [`plan`] takes [`SplineNumbers`] (lengths in millimetres, angles in degrees) and gives a
//! [`SplinePlan`], or refuses with a [`crate::error::teeth::PlanRefusal`] (a
//! [`crate::SolidError`] whose `teeth_failure()` is `Refused`) that names the number to change, in
//! backticks, and says why in plain words. A caller that draws the teeth, one that cuts them and
//! one that reports them all ask [`plan`], so they cannot disagree about a value.
//!
//! The kinds of teeth, what a new part is, the finish of a straight-sided tooth's tip corners and
//! the master spline are [`SplineForm`], [`SplineMake`], [`SplineCorner`] and [`SplineMaster`].
//!
//! Not upstream: SolidCraft's shaft spline planner, a kernel primitive because it computes geometry
//! any client could use (docs/layers.md §1). No recursion. Inputs are hostile: every number is
//! checked before it is used.

use crate::error::teeth::{PlanRefusal, TeethFailure, Why, FILLET_SEPARATION};
use crate::teeth::involute::{self, External};
use crate::teeth::master_spline::{self, Form};
use crate::teeth::straight::{self, Serration, StraightSpline, TipCorner};
use crate::teeth::{fit, Pair};
use crate::SolidError;

/// The kind of teeth of a shaft spline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SplineForm {
    /// Involute flanks, as a gear's.
    #[default]
    Involute,
    /// Straight-sided teeth of one width.
    Straight,
    /// Serrations: straight flanks at an angle to each other.
    Serration,
}

/// Which new part a shaft spline makes where it is not cut on a part that is there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SplineMaster {
    /// Every tooth alike.
    #[default]
    None,
    /// A tooth left out of the shaft, its space in the hub left uncut.
    ToothOut,
    /// A space of the shaft left uncut (a wide tooth), a tooth left out of the hub.
    WideTooth,
}

/// A shaft spline's numbers as plain values: lengths in millimetres, angles in degrees.
#[derive(Clone, Debug, Default, PartialEq)]
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
    /// The chamfer on the ends of the teeth of a shaft that is there (mm down the shaft and in from
    /// the major diameter).
    pub end_chamfer: Option<f64>,
    /// The chamfer where the teeth run out of a bore that is there (mm into the bore and out from
    /// the tops of the hub's teeth).
    pub entry_chamfer: Option<f64>,
    /// How far the end chamfer runs across the end, in from the major diameter, where that is not
    /// `end_chamfer` itself.
    pub end_chamfer_distance2: Option<f64>,
    /// The angle of the end chamfer to the round face, where it is not 45 degrees.
    pub end_chamfer_angle: Option<f64>,
    /// How far the entry chamfer runs across the end, out from the tops of the hub's teeth, where
    /// that is not `entry_chamfer` itself.
    pub entry_chamfer_distance2: Option<f64>,
    /// The angle of the entry chamfer to the bore, where it is not 45 degrees.
    pub entry_chamfer_angle: Option<f64>,
}

/// A chamfer as the numbers make it, in millimetres.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChamferSize {
    /// How far it runs along the round face.
    pub axial: f64,
    /// How far it runs across the end.
    pub radial: f64,
}

/// What the numbers make, before any body is touched.
#[derive(Clone, Debug)]
pub struct SplinePlan {
    /// The shaft (first) and its hub (second) with the master spline cut in and the whole spline
    /// turned.
    pub pair: Pair,
    /// The numbers the spline comes to.
    pub made: SplineMade,
    /// The master spline as placed.
    pub master: Option<master_spline::MasterSpline>,
    /// The angle from one tooth to the next of the teeth as counted, in radians: a full turn over
    /// the count of teeth. Not the outline's own pitch angle, which is a full turn once a master
    /// spline is cut in.
    pub tooth_pitch: f64,
    /// The angle of the middle of a tooth that stands, in radians counter-clockwise from the x
    /// axis: tooth 0, or with a master spline the tooth opposite it.
    pub tooth_at: f64,
    /// With a master spline: the arc that stands in the hub's outline for what was changed,
    /// turned by the hub's start so that it lies over the shaft.
    pub hub_arc_on_shaft: Option<master_spline::Span>,
    /// The chamfer on the ends of a shaft's teeth as made; none where `end_chamfer` is left out
    /// or 0.
    pub end_chamfer: Option<ChamferSize>,
    /// The chamfer at the entry of a hub's bore as made; none where `entry_chamfer` is left out
    /// or 0.
    pub entry_chamfer: Option<ChamferSize>,
}

/// The numbers a shaft spline comes to. Diameters and gaps in millimetres.
#[derive(Clone, Debug, Default, PartialEq)]
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
    /// With a master spline: the tooth left out, or the tooth before the space left uncut, counted
    /// counter-clockwise from the tooth on angle 0.
    pub master_tooth: Option<u32>,
    /// With a master spline: how far the whole spline was turned to put it at its angle, in
    /// degrees.
    pub master_turn: Option<f64>,
    /// How far the involute flanks and fillets may be from the true curves, in millimetres.
    pub flank_tolerance: Option<f64>,
}

/// The largest length or diameter taken, in mm: the tooth modules' own limit.
const MOST_MM: f64 = crate::teeth::MAX_LENGTH;

/// The shortest splined length taken, in mm.
pub const LEAST_LENGTH: f64 = 1e-3;

/// The rim the hub keeps outside the bottoms of its spaces, at least, as a share of the depth of
/// the teeth.
const LEAST_RIM: f64 = 0.5;

/// The tooth modules' names for the numbers they check (an `OutOfRange` `what`, a `NotProved`
/// `what`), and the number of a spline each comes from.
pub(super) type Numbers = &'static [(&'static str, &'static str)];
/// The tooth modules' reasons, and the number to change for each.
pub(super) type Reasons = &'static [(Why, &'static str)];

/// The numbers and reasons of an involute shaft.
pub(super) const INVOLUTE_SHAFT: (Numbers, Reasons) = (
    &[
        ("module", "module"),
        ("teeth", "teeth"),
        ("pressure angle", "pressure_angle"),
        ("addendum", "addendum"),
        ("dedendum", "dedendum"),
        ("profile shift", "profile_shift"),
        ("tool tip radius", "root_fillet"),
        ("thinning", "thinning"),
        (FILLET_SEPARATION, "root_fillet"),
    ],
    &[
        (Why::PointedBelowTip, "addendum"),
        (Why::NoFlankLeft, "addendum"),
        (Why::TipRoundsOverlap, "root_fillet"),
        (Why::RootAboveTipRound, "root_fillet"),
        (Why::FilletsCross, "root_fillet"),
        (Why::FilletMissesFlank, "root_fillet"),
        (Why::CutThrough, "profile_shift"),
        (Why::RootInsideAxis, "dedendum"),
        (Why::ToothThickness, "thinning"),
    ],
);

/// The numbers and reasons of the hub that fits an involute shaft.
pub(super) const INVOLUTE_FIT: (Numbers, Reasons) = (
    &[
        ("side clearance", "side_clearance"),
        ("hub addendum", "hub_addendum"),
        ("tip clearance", "tip_clearance"),
        ("form clearance", "form_clearance"),
        ("fillet radius", "hub_root_round"),
        ("space width", "side_clearance"),
        ("minor radius", "hub_addendum"),
        ("major radius", "tip_clearance"),
    ],
    &[
        (Why::HubReachesFillet, "hub_addendum"),
        (Why::HubRoundsReachTips, "hub_root_round"),
        (Why::SpacesAsWideAsPitch, "side_clearance"),
        (Why::TipsBelowBase, "hub_addendum"),
        (Why::RootsInsideRound, "hub_root_round"),
        (Why::SpacesMeetAtTips, "side_clearance"),
        (Why::RootRoundsOverlap, "hub_root_round"),
    ],
);

/// The numbers and reasons of a straight-sided shaft.
pub(super) const STRAIGHT_SHAFT: (Numbers, Reasons) = (
    &[
        ("teeth", "teeth"),
        ("major diameter", "major_diameter"),
        ("minor diameter", "minor_diameter"),
        ("tooth width", "tooth_width"),
        ("flank angle", "flank_angle"),
        ("root round", "root_round"),
        ("chamfer size", "tip_size"),
        ("tip round radius", "tip_size"),
    ],
    &[
        (Why::FlankMissesCircles, "tooth_width"),
        (Why::ChamferTooLarge, "tip_size"),
        (Why::MajorRoundTooLarge, "tip_size"),
        (Why::MinorRoundTooLarge, "root_round"),
        (Why::TipCornersMeet, "tip_size"),
        (Why::TeethMeetAtRoot, "tooth_width"),
        (Why::NoStraightFlank, "tip_size"),
        (Why::MinorNotSmaller, "minor_diameter"),
    ],
);

/// The numbers and reasons of a serrated shaft.
pub(super) const SERRATION_SHAFT: (Numbers, Reasons) = (
    STRAIGHT_SHAFT.0,
    &[
        (Why::FlankMissesCircles, "flank_angle"),
        (Why::ChamferTooLarge, "tip_size"),
        (Why::MajorRoundTooLarge, "tip_size"),
        (Why::MinorRoundTooLarge, "root_round"),
        (Why::TipCornersMeet, "tip_size"),
        (Why::TeethMeetAtRoot, "flank_angle"),
        (Why::NoStraightFlank, "tip_size"),
        (Why::MinorNotSmaller, "minor_diameter"),
    ],
);

/// The numbers and reasons of the hub that fits a straight-sided or serrated shaft.
pub(super) const STRAIGHT_FIT: (Numbers, Reasons) = (
    &[
        ("side clearance", "side_clearance"),
        ("major clearance", "major_clearance"),
        ("minor clearance", "minor_clearance"),
        ("hub root round", "hub_root_round"),
        ("hub tip round", "hub_tip_round"),
        ("space width", "side_clearance"),
        ("root round", "hub_root_round"),
        ("tip round", "hub_tip_round"),
    ],
    &[
        (Why::MinorClearanceTooLarge, "minor_clearance"),
        (Why::HubRootRoundsTooClose, "hub_root_round"),
        (Why::HubTipsTooClose, "hub_tip_round"),
        (Why::HubRootRoundsMeet, "hub_root_round"),
        (Why::HubTeethTooNarrow, "side_clearance"),
        (Why::MajorRoundTooLarge, "hub_root_round"),
        (Why::MinorRoundTooLarge, "hub_tip_round"),
        (Why::NoStraightFlank, "hub_root_round"),
        (Why::FlankMissesCircles, "side_clearance"),
        (Why::TeethMeetAtRoot, "side_clearance"),
        (Why::TurnsFreely, "side_clearance"),
    ],
);

/// A refusal of the tooth modules as a shaft spline's: the number it comes from, found in the
/// tables, and the failure as data ([`PlanRefusal`]), whose words name that number. The tables are
/// asked in turn and the first that knows the name or the reason counts. A number no table knows
/// is named in the tooth modules' words, not as a number of the spline. A refusal of the planner
/// itself comes through as it is.
pub(super) fn refused(tables: &[(Numbers, Reasons)], e: TeethFailure) -> TeethFailure {
    let number_key = |what: &str| {
        tables
            .iter()
            .flat_map(|(numbers, _)| numbers.iter())
            .find(|(name, _)| *name == what)
            .map(|(_, key)| *key)
    };
    match e {
        TeethFailure::OutOfRange {
            what,
            value,
            above,
            up_to,
        } => TeethFailure::Refused(PlanRefusal::Number {
            key: number_key(what),
            what,
            value,
            above,
            up_to,
        }),
        TeethFailure::Invalid(why) => {
            let key = tables
                .iter()
                .flat_map(|(_, reasons)| reasons.iter())
                .find(|(reason, _)| *reason == why)
                .map(|(_, key)| *key);
            TeethFailure::Refused(PlanRefusal::Reason { key, why })
        }
        TeethFailure::NotProved {
            what,
            asked,
            reached,
        } => TeethFailure::Refused(PlanRefusal::Unproved {
            key: number_key(what),
            what,
            asked,
            reached,
        }),
        planner @ TeethFailure::Refused(_) => planner,
    }
}

/// A refusal of the master spline: one that the form asked for cannot be made on these teeth names
/// `master_spline` and says what to change. Matched on the tooth modules' reasons.
pub(super) fn master_refused(e: TeethFailure) -> TeethFailure {
    match e {
        TeethFailure::Invalid(Why::NoCornerOnSmallest) => {
            TeethFailure::Refused(PlanRefusal::MasterToothOut)
        }
        TeethFailure::Invalid(Why::NoCornerOnLargest) => {
            TeethFailure::Refused(PlanRefusal::MasterWideTooth)
        }
        other => refused(&[(&[("teeth", "teeth")], &[])], other),
    }
}

/// A number the numbers may give: none, or a finite number.
fn number(value: Option<f64>, key: &'static str) -> Result<Option<f64>, TeethFailure> {
    match value {
        None => Ok(None),
        Some(x) if x.is_finite() => Ok(Some(x)),
        Some(_) => Err(TeethFailure::Refused(PlanRefusal::NotANumber { key })),
    }
}

/// A number the numbers must give for this kind of teeth.
fn required(
    value: Option<f64>,
    key: &'static str,
    unit: &'static str,
) -> Result<f64, TeethFailure> {
    number(value, key)?.ok_or(TeethFailure::Refused(PlanRefusal::Required { key, unit }))
}

/// What the numbers make before the master spline: the pair, and the numbers so far.
struct Made {
    pair: Pair,
    made: SplineMade,
}

/// A report with the numbers every kind has, the rest left empty.
fn made_of(teeth: u32) -> SplineMade {
    SplineMade {
        teeth,
        positions: teeth,
        ..SplineMade::default()
    }
}

/// An involute shaft and its hub: the pair with flanks and fillets fitted as splines, and the
/// numbers.
fn involute_pair(n: &SplineNumbers) -> Result<Made, TeethFailure> {
    let module = required(n.module, "module", "mm")?;
    let shaft = External {
        module,
        teeth: n.teeth,
        pressure_angle: number(n.pressure_angle, "pressure_angle")?.unwrap_or(30.0),
        addendum: number(n.addendum, "addendum")?.unwrap_or(0.5),
        dedendum: number(n.dedendum, "dedendum")?.unwrap_or(0.85),
        shift: number(n.profile_shift, "profile_shift")?.unwrap_or(0.0),
        tool_tip_radius: number(n.root_fillet, "root_fillet")?.unwrap_or(0.4),
        thinning: number(n.thinning, "thinning")?.unwrap_or(0.0),
    };
    let fit = involute::Fit {
        side_clearance: required(
            n.side_clearance,
            "side_clearance",
            "mm, both sides together on the pitch circle",
        )?,
        hub_addendum: number(n.hub_addendum, "hub_addendum")?.unwrap_or(0.5),
        tip_clearance: required(n.tip_clearance, "tip_clearance", "mm")?,
        hub_fillet_radius: number(n.hub_root_round, "hub_root_round")?.unwrap_or(0.0),
        form_clearance: number(n.form_clearance, "form_clearance")?.unwrap_or(0.0),
    };
    let no = |e| refused(&[INVOLUTE_SHAFT], e);
    let (r, rb, ra, rf) = (
        shaft.pitch_radius_typed().map_err(no)?,
        shaft.base_radius_typed().map_err(no)?,
        shaft.tip_radius_typed().map_err(no)?,
        shaft.root_radius_typed().map_err(no)?,
    );
    let form = shaft.form_radius_typed().map_err(no)?;
    let undercut = shaft.is_undercut_typed().map_err(no)?;
    // The splines stay within this of the true curves; the points they are fitted to, within a
    // fortieth of it (SOURCE_SHARE).
    let within = (module * 5e-4).clamp(1e-6, 1e-2);
    shaft
        .outline_typed(within * fit::SOURCE_SHARE)
        .map_err(no)?;
    let fit_no = |e| refused(&[INVOLUTE_FIT, INVOLUTE_SHAFT], e);
    let hub = involute::hub_for_typed(&shaft, &fit).map_err(fit_no)?;
    let mut pair =
        involute::spline_pair_typed(&shaft, &fit, within * fit::SOURCE_SHARE).map_err(fit_no)?;
    pair.first.outline = fit::fitted_typed(&pair.first.outline, within).map_err(no)?;
    pair.second.outline = fit::fitted_typed(&pair.second.outline, within).map_err(fit_no)?;
    let mut made = made_of(n.teeth);
    made.shaft_major_diameter = 2.0 * ra;
    made.shaft_minor_diameter = 2.0 * rf;
    made.hub_major_diameter = 2.0 * hub.major_radius;
    made.hub_minor_diameter = 2.0 * hub.minor_radius;
    made.pitch_diameter = Some(2.0 * r);
    made.base_diameter = Some(2.0 * rb);
    made.form_diameter = Some(2.0 * form);
    made.undercut = Some(undercut);
    made.side_clearance_each = fit.side_clearance / 2.0;
    made.tip_gap = hub.major_radius - ra;
    made.root_gap = hub.minor_radius - rf;
    made.form_gap = Some(hub.minor_radius - form);
    // Involute flanks moved apart along the pitch circle: the hub turns by that arc before they
    // touch.
    made.turning_play = (fit.side_clearance / (2.0 * r)).to_degrees();
    made.flank_tolerance = Some(within);
    Ok(Made { pair, made })
}

/// A straight-sided or serrated shaft and its hub, all exact lines and arcs, and the numbers.
fn straight_pair(n: &SplineNumbers) -> Result<Made, TeethFailure> {
    let major = required(n.major_diameter, "major_diameter", "mm")?;
    let minor = required(n.minor_diameter, "minor_diameter", "mm")?;
    let root_round = number(n.root_round, "root_round")?.unwrap_or(0.0);
    let tip = match n.tip_corner {
        SplineCorner::Plain => TipCorner::Plain,
        SplineCorner::Chamfered => TipCorner::Chamfer {
            size: required(
                n.tip_size,
                "tip_size",
                "mm, the chamfer's length down the flank",
            )?,
        },
        SplineCorner::Rounded => TipCorner::Round {
            radius: required(n.tip_size, "tip_size", "mm, the round's radius")?,
        },
    };
    let fit = straight::Fit {
        side_clearance: required(
            n.side_clearance,
            "side_clearance",
            "mm on each side, square to the flanks",
        )?,
        major_clearance: required(n.major_clearance, "major_clearance", "mm")?,
        minor_clearance: required(n.minor_clearance, "minor_clearance", "mm")?,
        hub_root_round: number(n.hub_root_round, "hub_root_round")?.unwrap_or(0.0),
        hub_tip_round: number(n.hub_tip_round, "hub_tip_round")?.unwrap_or(0.0),
    };
    // The arcs are exact; the tolerance only says how finely the outline is checked.
    let tolerance = ((major - minor).abs() * 5e-4).clamp(1e-6, 1.0);
    let straight_teeth = n.form == SplineForm::Straight;
    let shaft_table = if straight_teeth {
        STRAIGHT_SHAFT
    } else {
        SERRATION_SHAFT
    };
    let fit_no = |e| refused(&[STRAIGHT_FIT, shaft_table], e);
    let mut made = made_of(n.teeth);
    let (pair, (hub_major, hub_minor), play) = if straight_teeth {
        let no = |e| refused(&[STRAIGHT_SHAFT], e);
        let shaft = StraightSpline {
            teeth: n.teeth,
            major_diameter: major,
            minor_diameter: minor,
            tooth_width: required(n.tooth_width, "tooth_width", "mm")?,
            root_round,
            tip,
        };
        shaft.outline_typed(tolerance).map_err(no)?;
        let hub = straight::hub_for_typed(&shaft, &fit).map_err(fit_no)?;
        (
            straight::spline_pair_typed(&shaft, &fit, tolerance).map_err(fit_no)?,
            (hub.major_diameter, hub.minor_diameter),
            straight::turning_play_typed(&shaft, &fit).map_err(fit_no)?,
        )
    } else {
        let no = |e| refused(&[SERRATION_SHAFT], e);
        let shaft = Serration {
            teeth: n.teeth,
            major_diameter: major,
            minor_diameter: minor,
            flank_angle: number(n.flank_angle, "flank_angle")?.unwrap_or(90.0),
            tip,
            root_round,
        };
        shaft.outline_typed(tolerance).map_err(no)?;
        made.sharp_tip_diameter = shaft.sharp_tip_radius_typed().ok().map(|r| 2.0 * r);
        let hub = straight::serration_hub_for_typed(&shaft, &fit).map_err(fit_no)?;
        let diameters = (
            hub.major_diameter_typed().map_err(fit_no)?,
            hub.minor_diameter_typed().map_err(fit_no)?,
        );
        (
            straight::serration_pair_typed(&shaft, &fit, tolerance).map_err(fit_no)?,
            diameters,
            straight::serration_play_typed(&shaft, &fit).map_err(fit_no)?,
        )
    };
    made.shaft_major_diameter = major;
    made.shaft_minor_diameter = minor;
    made.hub_major_diameter = hub_major;
    made.hub_minor_diameter = hub_minor;
    made.side_clearance_each = fit.side_clearance;
    made.tip_gap = (hub_major - major) / 2.0;
    made.root_gap = (hub_minor - minor) / 2.0;
    made.turning_play = play.to_degrees();
    Ok(Made { pair, made })
}

/// The module that puts the tips of involute teeth on the circle of radius `radius`: the tips lie
/// at half the module times the teeth, and the addendum and the profile shift in modules, so the
/// module is `2 radius / (teeth + 2 (addendum + profile_shift))`. None where that divisor or the
/// module is not a positive finite number.
#[must_use]
pub fn module_for_major_radius(
    teeth: f64,
    addendum: f64,
    profile_shift: f64,
    radius: f64,
) -> Option<f64> {
    let across = teeth + 2.0 * (addendum + profile_shift);
    if across.is_finite() && across > 0.0 {
        let module = 2.0 * radius / across;
        if module.is_finite() && module > 0.0 {
            return Some(module);
        }
    }
    None
}

/// For a spline cut on a shaft that is there, the size left out is the shaft's own: the spline's
/// major diameter is the diameter of the shaft, whose radius is `radius`. For involute teeth that
/// fixes the module (the circle of the tips is the pitch circle and, each side, the addendum and
/// the profile shift in modules); for straight-sided teeth and serrations it is the major diameter
/// itself. A size that is given stays: the numbers then win over the shaft.
fn fit_to_shaft(n: &mut SplineNumbers, radius: f64) {
    match n.form {
        SplineForm::Involute if n.module.is_none() => {
            // The same values the plan takes where they are left out.
            n.module = module_for_major_radius(
                f64::from(n.teeth),
                n.addendum.unwrap_or(0.5),
                n.profile_shift.unwrap_or(0.0),
                radius,
            );
        }
        SplineForm::Straight | SplineForm::Serration if n.major_diameter.is_none() => {
            n.major_diameter = Some(2.0 * radius);
        }
        _ => {}
    }
}

/// How far a radius of the spline may be from the radius `radius` of the shaft or bore it is cut on
/// and still be taken as the same (mm): a thousandth of the radius, and a thousandth of a
/// millimetre at least.
#[must_use]
pub fn same_size(radius: f64) -> f64 {
    (1e-3 * radius).max(1e-3)
}

/// The major radius the numbers ask for, where they give the size: for involute teeth half the
/// module times the teeth, and the addendum and the profile shift in modules (the values the plan
/// takes where they are left out); for straight-sided teeth and serrations half the major diameter.
fn asked_major_radius(n: &SplineNumbers) -> Option<f64> {
    match n.form {
        SplineForm::Involute => {
            let module = n.module?;
            Some(
                module
                    * (f64::from(n.teeth) / 2.0
                        + n.addendum.unwrap_or(0.5)
                        + n.profile_shift.unwrap_or(0.0)),
            )
        }
        SplineForm::Straight | SplineForm::Serration => n.major_diameter.map(|d| d / 2.0),
    }
}

/// The numbers of a spline cut on a shaft that is there, whose radius is `radius`: the size left
/// out is the shaft's own (for involute teeth the module that puts the tips on the shaft's surface,
/// for straight-sided teeth and serrations the major diameter), and a size given that is within
/// [`same_size`] of the shaft's own is the shaft's own too. A module given to a few places means
/// the shaft as it is, and the spline is then made to the shaft exactly, with nothing left between
/// the tops of its teeth and the shaft's surface. Any other size given stays: the numbers then win
/// over the shaft.
pub fn fit_to_shaft_within(n: &mut SplineNumbers, radius: f64) {
    if asked_major_radius(n).is_some_and(|major| (major - radius).abs() <= same_size(radius)) {
        match n.form {
            SplineForm::Involute => n.module = None,
            SplineForm::Straight | SplineForm::Serration => n.major_diameter = None,
        }
    }
    fit_to_shaft(n, radius);
}

/// How far the tools that cut a spline reach past the faces they would otherwise only touch, for
/// teeth `depth` deep.
#[must_use]
pub fn tool_margin(depth: f64) -> f64 {
    (0.1 * depth.max(0.0)).max(1e-3)
}

/// The radius of the cutter of diameter `diameter` that ends the tooth spaces of a shaft, or why
/// there is none. The cutter's section (the tooth space from `root` out to `out_to`) must lie on
/// one side of the cutter's axis with `margin` to spare, so the radius must reach past the depth it
/// cuts.
pub fn cutter_radius(diameter: f64, root: f64, out_to: f64, margin: f64) -> crate::Result<f64> {
    cutter_radius_typed(diameter, root, out_to, margin)
        .map_err(|e| SolidError::teeth("teeth::shaft_spline::cutter_radius", e))
}

/// [`cutter_radius`] with its failure as a [`TeethFailure`], for the tooth module's own callers:
/// always a [`TeethFailure::Refused`] that names `cutter_diameter`.
pub(crate) fn cutter_radius_typed(
    diameter: f64,
    root: f64,
    out_to: f64,
    margin: f64,
) -> Result<f64, TeethFailure> {
    if !(diameter.is_finite() && diameter <= MOST_MM) {
        return Err(TeethFailure::Refused(PlanRefusal::CutterNotANumber {
            most: MOST_MM,
        }));
    }
    let least = out_to - root + margin;
    if !(diameter / 2.0 > least) {
        return Err(TeethFailure::Refused(PlanRefusal::CutterTooSmall {
            at_least: 2.0 * least,
        }));
    }
    Ok(diameter / 2.0)
}

/// The keys of one chamfer's numbers: the chamfer, its second distance and its angle.
struct ChamferKeys {
    chamfer: &'static str,
    distance2: &'static str,
    angle: &'static str,
}

/// The chamfer that `size` (mm along the round face), with `distance2` (mm across the end) or
/// `angle` (degrees to the round face), makes: `size` both ways (45 degrees) when neither is
/// given. None for no chamfer (`size` left out or 0). Refused: a second distance or an angle
/// given without the chamfer, both given, a second distance that is not more than 0 or more than
/// the largest length taken, and an angle not between 0 and 90 degrees. `size` itself was
/// checked before.
fn chamfer_size(
    size: Option<f64>,
    distance2: Option<f64>,
    angle: Option<f64>,
    keys: &ChamferKeys,
) -> Result<Option<ChamferSize>, TeethFailure> {
    if size.is_none() {
        for (given, key) in [(distance2, keys.distance2), (angle, keys.angle)] {
            if given.is_some() {
                return Err(TeethFailure::Refused(PlanRefusal::ChamferShapeWithout {
                    key,
                    chamfer: keys.chamfer,
                }));
            }
        }
    }
    if distance2.is_some() && angle.is_some() {
        return Err(TeethFailure::Refused(PlanRefusal::ChamferBothShapes {
            distance2: keys.distance2,
            angle: keys.angle,
            chamfer: keys.chamfer,
        }));
    }
    if let Some(value) = distance2 {
        if !(value.is_finite() && value > 0.0 && value <= MOST_MM) {
            return Err(TeethFailure::Refused(PlanRefusal::ChamferDistanceRange {
                key: keys.distance2,
                most: MOST_MM,
                value,
            }));
        }
    }
    if let Some(value) = angle {
        if !(value.is_finite() && value > 0.0 && value < 90.0) {
            return Err(TeethFailure::Refused(PlanRefusal::ChamferAngleRange {
                key: keys.angle,
                value,
            }));
        }
    }
    let Some(size) = size.filter(|size| *size > 0.0) else {
        return Ok(None);
    };
    let radial = match (distance2, angle) {
        (Some(across), _) => across,
        (None, Some(degrees)) => size * degrees.to_radians().tan(),
        (None, None) => size,
    };
    Ok(Some(ChamferSize {
        axial: size,
        radial,
    }))
}

/// What the numbers make, or why they make nothing: the checks of the values and the tooth modules'
/// work. A number that does not apply to the chosen kind of teeth is not read.
pub fn plan(n: &SplineNumbers) -> crate::Result<SplinePlan> {
    plan_typed(n).map_err(|e| SolidError::teeth("teeth::shaft_spline::plan", e))
}

/// [`plan`] with its failure as a [`TeethFailure`], for the tooth module's own callers: always a
/// [`TeethFailure::Refused`].
pub(crate) fn plan_typed(n: &SplineNumbers) -> Result<SplinePlan, TeethFailure> {
    if !n.length.is_finite() {
        return Err(TeethFailure::Refused(PlanRefusal::NotANumber {
            key: "length",
        }));
    }
    if !(n.length >= LEAST_LENGTH && n.length <= MOST_MM) {
        return Err(TeethFailure::Refused(PlanRefusal::LengthRange {
            least: LEAST_LENGTH,
            most: MOST_MM,
            value: n.length,
        }));
    }
    let makes_hub = n.make != SplineMake::Shaft;
    if !makes_hub && n.hub_diameter.is_some() {
        return Err(TeethFailure::Refused(PlanRefusal::HubDiameterWithoutHub));
    }
    let hub_diameter = number(n.hub_diameter, "hub_diameter")?;
    if makes_hub && hub_diameter.is_none() {
        return Err(TeethFailure::Refused(PlanRefusal::HubDiameterRequired));
    }
    if n.master == SplineMaster::None && n.master_angle.is_some() {
        return Err(TeethFailure::Refused(PlanRefusal::MasterAngleWithoutMaster));
    }
    let master_angle = number(n.master_angle, "master_angle")?.unwrap_or(0.0);
    if !(master_angle.abs() <= master_spline::MOST_ANGLE) {
        return Err(TeethFailure::Refused(PlanRefusal::MasterAngleRange {
            value: master_angle,
        }));
    }
    number(n.cutter_diameter, "cutter_diameter")?;
    for (chamfer, key) in [
        (n.end_chamfer, "end_chamfer"),
        (n.entry_chamfer, "entry_chamfer"),
    ] {
        if let Some(size) = number(chamfer, key)? {
            if !(0.0..=MOST_MM).contains(&size) {
                return Err(TeethFailure::Refused(PlanRefusal::ChamferRange {
                    key,
                    most: MOST_MM,
                    value: size,
                }));
            }
        }
    }
    let end_chamfer = chamfer_size(
        n.end_chamfer,
        n.end_chamfer_distance2,
        n.end_chamfer_angle,
        &ChamferKeys {
            chamfer: "end_chamfer",
            distance2: "end_chamfer_distance2",
            angle: "end_chamfer_angle",
        },
    )?;
    let entry_chamfer = chamfer_size(
        n.entry_chamfer,
        n.entry_chamfer_distance2,
        n.entry_chamfer_angle,
        &ChamferKeys {
            chamfer: "entry_chamfer",
            distance2: "entry_chamfer_distance2",
            angle: "entry_chamfer_angle",
        },
    )?;

    let Made { mut pair, mut made } = match n.form {
        SplineForm::Involute => involute_pair(n)?,
        SplineForm::Straight | SplineForm::Serration => straight_pair(n)?,
    };
    // A cutter that ends the tooth spaces must reach past the depth of the teeth, whatever shaft it
    // meets.
    if let Some(diameter) = n.cutter_diameter {
        let (root, top) = pair.first.outline.radii();
        let margin = tool_margin(top - root);
        cutter_radius_typed(diameter, root, top + margin, margin)?;
    }
    let hub_diameter = hub_diameter.filter(|_| makes_hub);
    if let Some(od) = hub_diameter {
        let (hub_major, shaft_minor) = (
            made.hub_major_diameter / 2.0,
            made.shaft_minor_diameter / 2.0,
        );
        let least = hub_major + LEAST_RIM * (hub_major - shaft_minor).max(0.0);
        if od > MOST_MM {
            return Err(TeethFailure::Refused(PlanRefusal::HubDiameterTooLarge {
                most: MOST_MM,
                value: od,
            }));
        }
        if !(od / 2.0 > least) {
            // Up to the next hundredth, past rounding in the last digit.
            let at_least = (2.0 * least * 100.0 - 1e-6).ceil() / 100.0;
            return Err(TeethFailure::Refused(PlanRefusal::NoRim { at_least }));
        }
    }
    let form = match n.master {
        SplineMaster::None => None,
        SplineMaster::ToothOut => Some(Form::ToothLeftOut),
        SplineMaster::WideTooth => Some(Form::WideTooth),
    };
    let mut placed = None;
    if let Some(form) = form {
        let keyed = master_spline::master_spline_typed(&pair, form, master_angle)
            .map_err(master_refused)?;
        pair = keyed.pair.clone();
        made.positions = 1;
        made.master_tooth = Some(keyed.placement.tooth);
        made.master_turn = Some(keyed.placement.turn.to_degrees());
        placed = Some(keyed);
    }
    // The teeth as counted, a tooth to a pitch: with a master spline the outline is one period of
    // the whole ring, a full turn, so its own pitch angle is not this.
    let tooth_pitch = std::f64::consts::TAU / f64::from(n.teeth.max(1));
    let tooth_at = placed.as_ref().map_or(0.0, |keyed| {
        let opposite = keyed.placement.tooth.saturating_add(n.teeth.max(1) / 2) % n.teeth.max(1);
        keyed.placement.turn + tooth_pitch * f64::from(opposite)
    });
    let hub_arc_on_shaft = placed.as_ref().map(|keyed| master_spline::Span {
        radius: keyed.hub_arc.radius,
        start: keyed.hub_arc.start + pair.second_start,
        end: keyed.hub_arc.end + pair.second_start,
    });
    Ok(SplinePlan {
        pair,
        made,
        master: placed,
        tooth_pitch,
        tooth_at,
        hub_arc_on_shaft,
        end_chamfer,
        entry_chamfer,
    })
}
