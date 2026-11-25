//! Shaft Spline (#17693): the numbers of a spline on a shaft and in its hub, checked and planned by the kernel
//! (`solidcore_ops::shaft_spline`), before any body is touched. The rebuild of a `shaft_spline` feature
//! (`eval_shaft_spline.rs`), the command that adds one and the dialog's drawing all go through [`plan`], so they
//! cannot disagree about a value, and every refusal names the parameter to change in the same words.
//!
//! The kinds of teeth, what a new part is, the finish of a straight-sided tooth's tip corners and the master
//! spline are the enums of the feature ([`SplineForm`], [`SplineMake`], [`SplineCorner`], [`SplineMaster`]): the
//! kernel's own, as data. This file computes no geometry: the tooth outlines, the checks of the numbers and the
//! numbers they come to are the kernel's, and so is every point, direction, length and angle the rebuild works with
//! (the seat on a round face, the outlines as profiles, the sections of the tools).
//!
//! The kernel refuses in data: the number of the spline a refusal is about ([`SplineQuantity`]) and why. This file
//! is where a refusal gets the parameter's name ([`key`]) and its words ([`SplineRefusal`]): the parameters are
//! this app's, not the kernel's.

use solidcore_record::shaft_spline::{PlanRefusal, ProfileWhy, RequiredUnit, SplineQuantity, SplineRefusal as Refused, ToothWhy};

pub(crate) use solidcore_ops::round_face::{end_carried, end_run_out, end_run_past, far_end, shut_end};
pub(crate) use solidcore_ops::shaft_spline::{
    axis_gap, chamfer_plane, chamfer_section, copy_turn, lies_inward, lies_turned_back, moved_out, off_cylinder, points_apart, quarter_turn_in,
    seat_frame, start_candidates, tool_margin, tooth_at, turn_tells_sense,
};
pub use solidcore_ops::shaft_spline::{fit_to_shaft_within, module_for_major_radius, same_size};
pub use solidcore_record::shaft_spline::{
    MasterArc, MasterPlaced, OutlinePiece, SpaceSection, SplineCorner, SplineForm, SplineMade, SplineMake, SplineMaster, SplineNumbers, SplinePlan,
    ToothOutline, ToothRole,
};

/// Why the numbers make nothing, as the app says it: the parameter to change, where the refusal names one, and the
/// words.
#[derive(Clone, Debug, PartialEq)]
pub struct SplineRefusal {
    /// The parameter to change.
    pub key: Option<String>,
    /// The words.
    pub words: String,
}

impl std::fmt::Display for SplineRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.words)
    }
}

impl std::error::Error for SplineRefusal {}

impl From<Refused> for SplineRefusal {
    fn from(r: Refused) -> Self {
        SplineRefusal { key: r.quantity().map(|q| key(q).to_string()), words: words(&r) }
    }
}

/// The parameter of `shaft_spline.insert` (and the feature's field) that holds a number of the spline.
#[must_use]
pub fn key(quantity: SplineQuantity) -> &'static str {
    match quantity {
        SplineQuantity::Teeth => "teeth",
        SplineQuantity::Length => "length",
        SplineQuantity::Module => "module",
        SplineQuantity::PressureAngle => "pressure_angle",
        SplineQuantity::Addendum => "addendum",
        SplineQuantity::Dedendum => "dedendum",
        SplineQuantity::RootFillet => "root_fillet",
        SplineQuantity::ProfileShift => "profile_shift",
        SplineQuantity::Thinning => "thinning",
        SplineQuantity::HubAddendum => "hub_addendum",
        SplineQuantity::TipClearance => "tip_clearance",
        SplineQuantity::FormClearance => "form_clearance",
        SplineQuantity::MajorDiameter => "major_diameter",
        SplineQuantity::MinorDiameter => "minor_diameter",
        SplineQuantity::ToothWidth => "tooth_width",
        SplineQuantity::FlankAngle => "flank_angle",
        SplineQuantity::RootRound => "root_round",
        SplineQuantity::TipSize => "tip_size",
        SplineQuantity::MajorClearance => "major_clearance",
        SplineQuantity::MinorClearance => "minor_clearance",
        SplineQuantity::HubTipRound => "hub_tip_round",
        SplineQuantity::SideClearance => "side_clearance",
        SplineQuantity::HubRootRound => "hub_root_round",
        SplineQuantity::HubDiameter => "hub_diameter",
        SplineQuantity::MasterSpline => "master_spline",
        SplineQuantity::MasterAngle => "master_angle",
        SplineQuantity::CutterDiameter => "cutter_diameter",
        SplineQuantity::EndChamfer => "end_chamfer",
        SplineQuantity::EntryChamfer => "entry_chamfer",
        SplineQuantity::EndChamferDistance2 => "end_chamfer_distance2",
        SplineQuantity::EndChamferAngle => "end_chamfer_angle",
        SplineQuantity::EntryChamferDistance2 => "entry_chamfer_distance2",
        SplineQuantity::EntryChamferAngle => "entry_chamfer_angle",
    }
}

/// A number in plain words: no trailing zeros, at most six decimals, and a number too small for six decimals in the
/// form 1e-9.
fn plain(x: f64) -> String {
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

/// What a required number is measured in.
fn unit(unit: RequiredUnit) -> &'static str {
    match unit {
        RequiredUnit::Mm => "mm",
        RequiredUnit::MmBothSidesOnPitch => "mm, both sides together on the pitch circle",
        RequiredUnit::MmEachSideSquareToFlanks => "mm on each side, square to the flanks",
        RequiredUnit::MmDownTheFlank => "mm, the chamfer's length down the flank",
        RequiredUnit::MmRoundRadius => "mm, the round's radius",
    }
}

/// A bound that could not be proved, in words.
fn unproved(what: &str, asked: f64, reached: f64) -> String {
    format!("{what} cannot be proved within the work allowed: asked {asked:e}, reached {reached:e}")
}

/// Why numbers that are each acceptable make no teeth, in words.
fn reason(why: ToothWhy) -> &'static str {
    match why {
        ToothWhy::NotASpline => "the knots and control points do not describe a spline",
        ToothWhy::SourceTooCoarse => "the outline must be made at a fortieth of the tolerance asked of the splines, or finer",
        ToothWhy::ToleranceTooFineForSize => "the tolerance is too fine for numbers of the outline's size",
        ToothWhy::SplineNotWithin => "a spline could not be brought within the tolerance",
        ToothWhy::FittedNotFinite => "the fitted spline would not be finite",
        ToothWhy::CurveHasNoLength => "a curve of the outline has no length",
        ToothWhy::FitNotSolvable => "the points do not fix the curve: the fit cannot be solved",
        ToothWhy::SystemNotSquare => "the system is not square or its sides do not agree",
        ToothWhy::TooManyControlPoints => "a curve would need more control points than allowed to keep the tolerance",
        ToothWhy::CurvePointCount => "a curve of the outline has too few or too many points",
        ToothWhy::RunNotOnePiece => "the run cannot be held as one piece within the tolerance",
        ToothWhy::RunNotJoined => "the pieces of the run do not join",
        ToothWhy::RunEmpty => "the run has no pieces",
        ToothWhy::RunNotFinite => "the run would not be finite",
        ToothWhy::RunTooLong => "the run is too long to be held in points",
        ToothWhy::FilletMissesFlank => "the root fillet and the flank would not meet",
        ToothWhy::ToothThickness => "the tooth would be thinner than nothing or as wide as the pitch",
        ToothWhy::RootAboveTipRound => "the root must lie below the pitch circle by more than the tool's tip radius",
        ToothWhy::RootInsideAxis => "the root circle would be inside the axis",
        ToothWhy::TipRoundsOverlap => "the tool's tip radius is too large: its two rounds would overlap",
        ToothWhy::CutThrough => "the undercut would cut the tooth through at its root: its two root fillets would meet",
        ToothWhy::FilletsCross => "the root fillets of neighbouring teeth would cross",
        ToothWhy::NoFlankLeft => "the tip circle is at or below the start of the flank: no flank is left",
        ToothWhy::PointedBelowTip => "the teeth would come to a point below the tip circle",
        ToothWhy::SpacesAsWideAsPitch => "the spaces would be as wide as the pitch: no tooth is left",
        ToothWhy::TipsBelowBase => "the tips of the teeth would be below the base circle",
        ToothWhy::RootsInsideRound => "the roots must lie outside the tips by more than the fillet radius",
        ToothWhy::SpacesMeetAtTips => "the spaces would meet at the tips: no tooth is left",
        ToothWhy::RootRoundsOverlap => "the root rounds are too large: they would overlap",
        ToothWhy::HubReachesFillet => "the hub's teeth would reach into the shaft's root fillet",
        ToothWhy::HubRoundsReachTips => "the hub's root rounds would reach the shaft's tip corners: the rounds are too large for the tip clearance",
        ToothWhy::AngleNotANumber => "the angle is not a number",
        ToothWhy::NoCornerOnSmallest => "no corner of the teeth lies on the smallest circle: no tooth or space can be closed there",
        ToothWhy::NoCornerOnLargest => {
            "no corner of the teeth lies on the largest circle: the tops of the teeth and the bottoms of the spaces need a flat part to join"
        }
        ToothWhy::MasterAngleRange => "the angle of the master spline must be from -360 to 360 degrees",
        ToothWhy::NotAShaftAndHub => {
            "a master spline needs a shaft and its hub: the same count of teeth on one axis, the hub around the shaft and turning with it"
        }
        ToothWhy::HubStartNotANumber => "the hub's start is not a number",
        ToothWhy::BodyNotFinite => "the outline is not finite",
        ToothWhy::BodyCrossesItself => "the outline crosses itself",
        ToothWhy::PartNotFinite => "a part is not at a finite place",
        ToothWhy::MeasureTooIntricate => "the outlines are too fine or too intricate to be measured against each other in one go",
        ToothWhy::CurveTooFine => "the tolerance is too fine: a curve would need too many points",
        ToothWhy::CrossingCheckTooIntricate => "the outline is too intricate to check that it does not cross itself",
        ToothWhy::OutlineNotFinite => "the outline would not be finite",
        ToothWhy::PiecesDoNotJoin => "the pieces of the tooth would not join into a closed outline",
        ToothWhy::OutlineTooFine => "the tolerance is too fine: the outline would need too many points",
        ToothWhy::TooCoarse => "the tolerance is too coarse: the outline would have fewer than three points",
        ToothWhy::NotCounterClockwise => "the outline must run counter-clockwise around an area",
        ToothWhy::CrossesItself => "the outline would cross itself",
        ToothWhy::NoTopNearAngle => "no top of a tooth lies near the angle asked for",
        ToothWhy::TopsMeet => "the tops of two teeth meet: there is no space between them",
        ToothWhy::ToothAngleNotANumber => "the angle of the tooth is not a number",
        ToothWhy::TooMuchTurning => "the outlines are too fine, or the reach too small, to be turned through so many steps in one go",
        ToothWhy::FirstAxisNotFinite => "the first part's axis is not at a finite place",
        ToothWhy::SecondNotFinite => "the second part would not be at a finite place",
        ToothWhy::TurnsFreely => "the parts can turn freely: nothing stops the one on the other",
        ToothWhy::FlankMissesCircles => "the flank does not cross both circles: the teeth are too wide",
        ToothWhy::ChamferTooLarge => "the chamfer is too large for the tooth",
        ToothWhy::MajorRoundTooLarge => "the round at the major circle is too large for the tooth",
        ToothWhy::MinorRoundTooLarge => "the round at the minor circle is too large for the tooth",
        ToothWhy::TipCornersMeet => "the corners of a tooth would meet: no tip is left on the major circle",
        ToothWhy::TeethMeetAtRoot => "neighbouring teeth would meet at the root: no space is left between them",
        ToothWhy::HubRootRoundsMeet => "the hub's root rounds would meet: no root is left at the bottom of the space",
        ToothWhy::HubTeethTooNarrow => "the hub's teeth would be too narrow: neighbouring spaces would meet",
        ToothWhy::NoStraightFlank => "the corners leave no straight flank between them",
        ToothWhy::HubRootRoundsTooClose => "the hub's root rounds would come closer to the shaft's tips than the smallest clearance",
        ToothWhy::HubTipsTooClose => "the tips of the hub's teeth would come closer to the shaft's root rounds than the smallest clearance",
        ToothWhy::MinorNotSmaller => "the minor diameter must be smaller than the major diameter",
        ToothWhy::MinorClearanceTooLarge => "the minor clearance would take the hub's teeth out beyond the shaft's tips: nothing would drive",
        ToothWhy::FlanksParallel => "the flanks are parallel: they do not meet",
        ToothWhy::FlanksNearlyParallel => "the flanks are so nearly parallel that they meet too far away to say",
    }
}

/// A reason as it is said of a spline's own numbers: the two reasons that name the tool's tip radius name the root
/// fillet, the number a Shaft Spline has for it.
fn planner_reason(why: ToothWhy) -> &'static str {
    match why {
        ToothWhy::RootAboveTipRound => "the root must lie below the pitch circle by more than the root fillet",
        ToothWhy::TipRoundsOverlap => "the root fillet is too large: its two rounds would overlap",
        other => reason(other),
    }
}

/// Why an outline is not a profile, in words.
fn profile(why: ProfileWhy) -> &'static str {
    match why {
        ProfileWhy::NotFitted => "a curve of the outline was not fitted",
        ProfileWhy::Spline(why) => reason(why),
    }
}

/// The planner's refusal in words, each number by its parameter.
fn plan_words(r: &PlanRefusal) -> String {
    let named = |q: &SplineQuantity| format!("`{}`", key(*q));
    match r {
        PlanRefusal::Number { quantity, what, value, above, up_to } => {
            let (value, up_to) = (*value, *up_to);
            let name = quantity.as_ref().map_or_else(|| format!("the {what}"), named);
            // A lower limit a hair above nothing is "more than 0" in plain words.
            let above = if *above > 0.0 && *above < 1e-4 { 0.0 } else { *above };
            if !value.is_finite() {
                format!("{name} must be a number")
            } else if value > up_to {
                format!("{name} must be at most {}, not {}", plain(up_to), plain(value))
            } else if value >= up_to {
                format!("{name} must be less than {}", plain(up_to))
            } else if value < 0.0 && above <= 0.0 && above >= 0.0 {
                format!("{name} cannot be negative, not {}", plain(value))
            } else {
                format!("{name} must be more than {}, not {}", plain(above), plain(value))
            }
        }
        PlanRefusal::Reason { quantity: Some(q), why } => format!("{}: {}", named(q), planner_reason(*why)),
        PlanRefusal::Reason { quantity: None, why } => reason(*why).to_string(),
        PlanRefusal::Unproved { quantity, what, asked, reached } => {
            let inner = unproved(what, *asked, *reached);
            match quantity {
                Some(q) => format!("{}: {inner}", named(q)),
                None => inner,
            }
        }
        PlanRefusal::MasterToothOut => format!(
            "`master_spline`: a tooth cannot be left out of these teeth: {}; make a wide tooth instead (wide_tooth), or make the shaft's root rounds and the hub's tip rounds smaller",
            reason(ToothWhy::NoCornerOnSmallest)
        ),
        PlanRefusal::MasterWideTooth => format!(
            "`master_spline`: a wide tooth cannot be made on these teeth: {}; leave a tooth out instead (tooth_out), or use plain or chamfered tip corners and smaller root rounds in the hub",
            reason(ToothWhy::NoCornerOnLargest)
        ),
        PlanRefusal::NotANumber { quantity } => format!("{} must be a number", named(quantity)),
        PlanRefusal::Required { quantity, unit: measured } => format!("{} is required ({})", named(quantity), unit(*measured)),
        PlanRefusal::LengthRange { least, most, value } => {
            format!("`length` must be from {} to {} mm, not {}", plain(*least), plain(*most), plain(*value))
        }
        PlanRefusal::HubDiameterWithoutHub => "`hub_diameter` is the hub's outside diameter: give it only when `make` is hub or both".to_string(),
        PlanRefusal::HubDiameterRequired => "`hub_diameter` is required to make the hub (mm, its outside diameter)".to_string(),
        PlanRefusal::MasterAngleWithoutMaster => {
            "`master_angle` is where the master spline sits: give it only with `master_spline` tooth_out or wide_tooth".to_string()
        }
        PlanRefusal::MasterAngleRange { value } => format!("`master_angle` must be from -360 to 360 (degrees), not {}", plain(*value)),
        PlanRefusal::ChamferRange { quantity, most, value } => {
            format!("{} must be from 0 to {} mm, not {}", named(quantity), plain(*most), plain(*value))
        }
        PlanRefusal::CutterNotANumber { most } => format!("`cutter_diameter` must be a number of at most {} mm", plain(*most)),
        PlanRefusal::CutterTooSmall { at_least } => {
            format!("`cutter_diameter` must be more than {} mm: the cutter's radius must reach past the depth of the teeth", plain(*at_least))
        }
        PlanRefusal::HubDiameterTooLarge { most, value } => format!("`hub_diameter` must be at most {} mm, not {}", plain(*most), plain(*value)),
        PlanRefusal::NoRim { at_least } => {
            format!("`hub_diameter` leaves no rim around the hub's teeth: it must be more than {} mm", plain(*at_least))
        }
        PlanRefusal::ChamferShapeWithout { quantity, chamfer } => {
            let chamfer = named(chamfer);
            format!("{} shapes the chamfer {chamfer} sets: give it only with {chamfer}", named(quantity))
        }
        PlanRefusal::ChamferBothShapes { distance2, angle, chamfer } => format!(
            "{} and {} both set how far the chamfer {} reaches across the end: give one of them, not both",
            named(distance2),
            named(angle),
            named(chamfer)
        ),
        PlanRefusal::ChamferDistanceRange { quantity, most, value } => {
            if value.is_finite() {
                format!("{} must be more than 0 and at most {} mm, not {}", named(quantity), plain(*most), plain(*value))
            } else {
                format!("{} must be a number of more than 0 and at most {} mm", named(quantity), plain(*most))
            }
        }
        PlanRefusal::ChamferAngleRange { quantity, value } => {
            if value.is_finite() {
                format!("{} must be more than 0 and less than 90 (degrees), not {}", named(quantity), plain(*value))
            } else {
                format!("{} must be a number of more than 0 and less than 90 (degrees)", named(quantity))
            }
        }
        PlanRefusal::NotAProfile { why } => format!("the spline's outline cannot be made into a profile: {}", profile(*why)),
    }
}

/// The kernel's refusal in words.
fn words(r: &Refused) -> String {
    match r {
        Refused::OutOfRange { what, value, above, up_to } => format!("{what} is {value}; it must be above {above} and at most {up_to}"),
        Refused::Invalid { why } => reason(*why).to_string(),
        Refused::Refused { refusal } => plan_words(refusal),
        Refused::NotProved { what, asked, reached } => unproved(what, *asked, *reached),
        Refused::Kernel { operation } => format!("the kernel could not work it out ({operation})"),
    }
}

/// What the numbers make, with the last few answers kept: the dialog's drawing, the command and the rebuild ask
/// for the same numbers one after another, and the kernel's work on involute flanks is not small.
pub fn plan(n: &SplineNumbers) -> Result<SplinePlan, SplineRefusal> {
    use std::sync::{Mutex, PoisonError};
    type Kept = Vec<(String, Result<SplinePlan, SplineRefusal>)>;
    static KEPT: Mutex<Kept> = Mutex::new(Vec::new());
    // Every number in full.
    let key = format!("{n:?}");
    let hit = KEPT.lock().unwrap_or_else(PoisonError::into_inner).iter().find(|(k, _)| *k == key).map(|(_, made)| made.clone());
    if let Some(made) = hit {
        return made;
    }
    let made = solidcore_ops::shaft_spline::plan(n).map_err(SplineRefusal::from);
    let mut kept = KEPT.lock().unwrap_or_else(PoisonError::into_inner);
    if kept.len() >= 8 {
        drop(kept.remove(0));
    }
    kept.push((key, made.clone()));
    made
}

/// The points of a tooth outline turned by `turn` about the axis, for a drawing.
///
/// # Errors
///
/// The kernel's refusal of an outline that gives no points.
pub fn outline_points(outline: &ToothOutline, turn: f64) -> Result<Vec<[f64; 2]>, SplineRefusal> {
    solidcore_ops::shaft_spline::outline_points(outline, turn).map_err(SplineRefusal::from)
}

/// A spline as the Bezier pieces between its knots.
///
/// # Errors
///
/// The kernel's refusal of knots and control points that are not a spline.
pub fn bezier_pieces(degree: usize, knots: &[f64], control: &[[f64; 2]]) -> Result<Vec<Vec<[f64; 2]>>, SplineRefusal> {
    solidcore_ops::shaft_spline::bezier_pieces(degree, knots, control).map_err(SplineRefusal::from)
}

/// One tooth space of the shaft's outline as a section out to `out_to` from the axis.
///
/// # Errors
///
/// The kernel's refusal of an outline with no such space.
pub fn space_section(outline: &ToothOutline, tooth_at: f64, out_to: f64) -> Result<SpaceSection, SplineRefusal> {
    solidcore_ops::shaft_spline::space_section(outline, tooth_at, out_to).map_err(SplineRefusal::from)
}

/// The radius of a cutter of `diameter` whose section reaches from `root` out to `out_to`, with `margin` to spare.
///
/// # Errors
///
/// The kernel's refusal of a cutter too small, by the parameter `cutter_diameter`.
pub(crate) fn cutter_radius(diameter: f64, root: f64, out_to: f64, margin: f64) -> Result<f64, SplineRefusal> {
    solidcore_ops::shaft_spline::cutter_radius(diameter, root, out_to, margin).map_err(SplineRefusal::from)
}

/// A whole outline as one closed loop of profile segments, turned by `turn`.
///
/// # Errors
///
/// The kernel's refusal of an outline that cannot be made into a profile.
pub(crate) fn outline_loop(outline: &ToothOutline, turn: f64) -> Result<solidcore_record::shaft_spline::OutlineLoop, SplineRefusal> {
    solidcore_ops::shaft_spline::outline_loop(outline, turn).map_err(SplineRefusal::from)
}

/// The section a cutter takes out of one tooth space where it leaves the shaft; `None` for a space too wide for
/// one cutter's section to reach round (it ends square: [`space_region`]).
///
/// # Errors
///
/// The kernel's refusal of a floor that cannot be made into a profile.
pub(crate) fn cutter_section(
    space: &SpaceSection,
    root: f64,
    cutter_radius: f64,
    out_to: f64,
) -> Result<Option<solidcore_record::shaft_spline::CutterSection>, SplineRefusal> {
    solidcore_ops::shaft_spline::cutter_section(space, root, cutter_radius, out_to).map_err(SplineRefusal::from)
}

/// The whole cross-section of one tooth space out to the circle of `out_to` about the axis: what a space that ends
/// square takes out of the shaft.
///
/// # Errors
///
/// The kernel's refusal of a floor that cannot be made into a profile.
pub(crate) fn space_region(space: &SpaceSection, out_to: f64) -> Result<solidcore_record::shaft_spline::SpaceRegion, SplineRefusal> {
    solidcore_ops::shaft_spline::space_region(space, out_to).map_err(SplineRefusal::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every number of a spline has its own parameter, and the names are the feature's own.
    #[test]
    fn every_quantity_has_its_own_parameter() {
        let all = [
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
        let keys: std::collections::BTreeSet<&str> = all.iter().map(|q| key(*q)).collect();
        assert_eq!(keys.len(), all.len());
    }

    /// A refusal the kernel gives as data comes out in the words a Shaft Spline has always refused in, with the
    /// parameter to change in backticks first.
    #[test]
    fn a_refusal_names_its_parameter_in_words() {
        let refused = |refusal: PlanRefusal| SplineRefusal::from(Refused::Refused { refusal });
        let r = refused(PlanRefusal::CutterTooSmall { at_least: 13.6 });
        assert_eq!(r.key.as_deref(), Some("cutter_diameter"));
        assert_eq!(r.words, "`cutter_diameter` must be more than 13.6 mm: the cutter's radius must reach past the depth of the teeth");
        let r = refused(PlanRefusal::Required { quantity: SplineQuantity::SideClearance, unit: RequiredUnit::MmBothSidesOnPitch });
        assert_eq!(r.words, "`side_clearance` is required (mm, both sides together on the pitch circle)");
        let r = refused(PlanRefusal::ChamferBothShapes {
            distance2: SplineQuantity::EndChamferDistance2,
            angle: SplineQuantity::EndChamferAngle,
            chamfer: SplineQuantity::EndChamfer,
        });
        assert_eq!(r.key.as_deref(), Some("end_chamfer_distance2"));
        assert_eq!(
            r.words,
            "`end_chamfer_distance2` and `end_chamfer_angle` both set how far the chamfer `end_chamfer` reaches across the end: give one of them, not both"
        );
        let r = refused(PlanRefusal::Number { quantity: Some(SplineQuantity::Module), what: "module".into(), value: -1.0, above: 0.0, up_to: 100.0 });
        assert_eq!(r.words, "`module` cannot be negative, not -1");
        let r = refused(PlanRefusal::Reason { quantity: Some(SplineQuantity::RootFillet), why: ToothWhy::TipRoundsOverlap });
        assert_eq!(r.words, "`root_fillet`: the root fillet is too large: its two rounds would overlap");
        let r = SplineRefusal::from(Refused::Invalid { why: ToothWhy::FlanksParallel });
        assert_eq!((r.key, r.words.as_str()), (None, "the flanks are parallel: they do not meet"));
    }
}
