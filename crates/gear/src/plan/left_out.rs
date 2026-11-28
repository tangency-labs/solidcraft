//! Which of the spec's options each type uses, and the one warning per group for those it
//! leaves out (only when they are set).
//!
//! The options here are the tooth tuning, chamfers, shafts, hub hardware and the hub, bore and
//! lightening holes: the ones the window shows for some types and not others. [`why_left_out`]
//! answers for every type and option in one `match` with no catch-all arm, so a new type has to
//! say what it does with each. `body_tests::every_type_uses_or_names_every_option` holds the
//! answers against the planners: an option said to be left out changes no body, and one said
//! to be used changes a body or is warned about by the planner itself.

use super::{Kind, Plan, Spec};

/// One option (or a few fields that go together) that a type may leave out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opt {
    TipRound,
    XyCompensation,
    ElephantFoot,
    Nozzle,
    Clearance,
    TipShortening,
    TipDiameter,
    TipDiameter2,
    TipRelief,
    Crowning,
    Backlash2,
    /// `face_width2` and `face_offset2`.
    Face2,
    Bore,
    HubDiameter,
    HubLength,
    /// `holes` (and `hole_diameter` with them).
    Holes,
    FrontShaft,
    BackShaft,
    /// `hardware`: set screws, nut traps, inserts, clamp slit and screw, bearing pocket.
    Hardware,
    Bore2,
    FrontShaft2,
    BackShaft2,
    Hardware2,
    /// `tooth_chamfer_front` and `tooth_chamfer_back`.
    ToothChamfers,
    BoreChamfer,
    HubChamfer,
}

impl Opt {
    /// Every option, in the order warnings list them.
    pub const ALL: &[Opt] = &[
        Opt::TipRound,
        Opt::XyCompensation,
        Opt::ElephantFoot,
        Opt::Nozzle,
        Opt::Clearance,
        Opt::TipShortening,
        Opt::TipDiameter,
        Opt::TipDiameter2,
        Opt::TipRelief,
        Opt::Crowning,
        Opt::Backlash2,
        Opt::Face2,
        Opt::Bore,
        Opt::HubDiameter,
        Opt::HubLength,
        Opt::Holes,
        Opt::FrontShaft,
        Opt::BackShaft,
        Opt::Hardware,
        Opt::Bore2,
        Opt::FrontShaft2,
        Opt::BackShaft2,
        Opt::Hardware2,
        Opt::ToothChamfers,
        Opt::BoreChamfer,
        Opt::HubChamfer,
    ];

    /// What warnings call it (gear 2's are prefixed "gear 2's").
    pub fn label(self) -> &'static str {
        match self {
            Opt::TipRound => "tip rounding",
            Opt::XyCompensation => "horizontal-expansion compensation",
            Opt::ElephantFoot => "elephant-foot relief",
            Opt::Nozzle => "the nozzle check",
            Opt::Clearance => "tip clearance",
            Opt::TipShortening => "tip shortening",
            Opt::TipDiameter | Opt::TipDiameter2 => "tip diameter",
            Opt::TipRelief => "tip relief",
            Opt::Crowning => "crowning",
            Opt::Backlash2 => "own backlash",
            Opt::Face2 => "face width and offset",
            Opt::Bore | Opt::Bore2 => "bore",
            Opt::HubDiameter => "hub diameter",
            Opt::HubLength => "hub length",
            Opt::Holes => "lightening holes",
            Opt::FrontShaft | Opt::FrontShaft2 => "front shaft",
            Opt::BackShaft | Opt::BackShaft2 => "back shaft",
            Opt::Hardware | Opt::Hardware2 => "hub hardware and bearing pocket",
            Opt::ToothChamfers => "tooth chamfers",
            Opt::BoreChamfer => "bore chamfer",
            Opt::HubChamfer => "hub chamfer",
        }
    }

    /// Gear 2's (mate, planets, the worm, the ring gear of a hypoid set, a spline's hub).
    pub fn second(self) -> bool {
        matches!(self, Opt::TipDiameter2 | Opt::Backlash2 | Opt::Face2 | Opt::Bore2 | Opt::FrontShaft2 | Opt::BackShaft2 | Opt::Hardware2)
    }

    /// Part of the tooth form (the rest make the body round the teeth).
    pub fn tooth_form(self) -> bool {
        matches!(
            self,
            Opt::TipRound
                | Opt::XyCompensation
                | Opt::ElephantFoot
                | Opt::Nozzle
                | Opt::Clearance
                | Opt::TipShortening
                | Opt::TipDiameter
                | Opt::TipDiameter2
                | Opt::TipRelief
                | Opt::Crowning
                | Opt::Backlash2
        )
    }

    /// Whether the spec sets it (off at its default; modifiers such as `elephant_foot_height`
    /// or `bearing_fit` alone don't count).
    pub fn is_set(self, s: &Spec) -> bool {
        let hardware = |h: &crate::hub::Hardware| {
            h.set_screw > 0.0 || h.nut_width > 0.0 || h.insert_diameter > 0.0 || h.clamp_slit > 0.0 || h.clamp_screw > 0.0 || h.bearing_od > 0.0
        };
        match self {
            Opt::TipRound => s.tip_round > 0.0,
            Opt::XyCompensation => s.xy_compensation > 0.0,
            Opt::ElephantFoot => s.elephant_foot > 0.0,
            Opt::Nozzle => s.nozzle > 0.0,
            Opt::Clearance => s.clearance > 0.0,
            Opt::TipShortening => s.tip_shortening > 0.0,
            Opt::TipDiameter => s.tip_diameter > 0.0,
            Opt::TipDiameter2 => s.tip_diameter2 > 0.0,
            Opt::TipRelief => s.tip_relief > 0.0,
            Opt::Crowning => s.crowning > 0.0,
            Opt::Backlash2 => s.backlash2.is_some(),
            Opt::Face2 => s.face_width2 > 0.0 || s.face_offset2 != 0.0,
            Opt::Bore => s.bore.outline().is_some(),
            Opt::HubDiameter => s.hub_diameter > 0.0,
            Opt::HubLength => s.hub_length > 0.0,
            Opt::Holes => s.holes > 0,
            Opt::FrontShaft => s.shaft_front.exists(),
            Opt::BackShaft => s.shaft_back.exists(),
            Opt::Hardware => hardware(&s.hardware),
            Opt::Bore2 => s.bore2.outline().is_some(),
            Opt::FrontShaft2 => s.shaft2_front.exists(),
            Opt::BackShaft2 => s.shaft2_back.exists(),
            Opt::Hardware2 => hardware(&s.hardware2),
            Opt::ToothChamfers => s.tooth_chamfer_front.size > 0.0 || s.tooth_chamfer_back.size > 0.0,
            Opt::BoreChamfer => s.bore_chamfer > 0.0,
            Opt::HubChamfer => s.hub_chamfer > 0.0,
        }
    }
}

/// Why a type leaves an option out: the reason, and whether the warning says it doesn't apply
/// to the teeth ("… don't apply to its teeth (why); left out") or is left out of the body
/// ("… left out (why)").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Why {
    pub reason: &'static str,
    pub teeth: bool,
}

const fn teeth(reason: &'static str) -> Option<Why> {
    Some(Why { reason, teeth: true })
}

const fn body(reason: &'static str) -> Option<Why> {
    Some(Why { reason, teeth: false })
}

const OWN_FORM: Option<Why> = teeth("they have their own form");
const SINGLE: Option<Why> = body("a single gear has no gear 2");
const NO_MATE: Option<Why> = body("there is no gear 2: mating teeth 0");

/// Why the spec's type leaves `o` out, or `None` when it uses it.
pub fn why_left_out(s: &Spec, o: Opt) -> Option<Why> {
    use Opt::*;
    // Involute teeth cut from the spec's own rack: the clearance, the tip shortening and the
    // horizontal-expansion compensation (`Spec::involute`) reach them, nothing else.
    let rack_only = matches!(o, Clearance | TipShortening | XyCompensation);
    if s.train.is_some() {
        // Gear 1 takes gear 1's options, every other gear gear 2's.
        return match o {
            TipDiameter | TipDiameter2 | Face2 => body("a gear train keeps the rack's tips and one face width"),
            _ => None,
        };
    }
    match s.kind {
        Kind::Spur | Kind::Helical | Kind::Herringbone => {
            if o.second() {
                SINGLE
            } else {
                None
            }
        }
        Kind::Pair => None,
        Kind::Planetary => match o {
            TipDiameter | TipDiameter2 | Face2 => body("a planetary set keeps the rack's tips and one face width"),
            _ => None,
        },
        Kind::Internal => match o {
            _ if rack_only => None,
            ToothChamfers => None,
            _ if o.second() => SINGLE,
            _ if o.tooth_form() => teeth("a ring gear's teeth are drawn as plain tooth spaces"),
            _ => body("a ring gear is a toothed rim: no bore, hub or shafts"),
        },
        Kind::Rack => match o {
            _ if rack_only => None,
            _ if o.second() => SINGLE,
            _ if o.tooth_form() => teeth("a rack's teeth are drawn straight from the basic rack"),
            _ => body("a rack is a plain toothed bar"),
        },
        Kind::Bevel | Kind::SpiralBevel | Kind::BevelPlanetary => match o {
            Bore | Bore2 | HubDiameter | HubLength | FrontShaft | BackShaft | FrontShaft2 | BackShaft2 => None,
            ToothChamfers | BoreChamfer | HubChamfer => body("chamfers are not built on bevel gears: their ends are cones"),
            Face2 => None,
            _ if o.tooth_form() => teeth("bevel teeth are cut by their own crown gear"),
            _ => body("bevel gears take a hub, bores and shafts only"),
        },
        Kind::Hypoid => match o {
            Bore | Bore2 | HubDiameter | HubLength => None,
            // `face_width` is the ring gear's, `face_width2` and `face_offset2` the pinion's.
            Face2 => None,
            _ if o.tooth_form() => teeth("hypoid teeth are generated from the ring gear's form"),
            _ => body("a hypoid set takes bores and the pinion's shank (hub diameter and length) only"),
        },
        Kind::CrossedHelical => match o {
            _ if rack_only => None,
            Backlash2 => None,
            Face2 => body("both gears share one face width"),
            _ if o.tooth_form() => teeth("crossed helical teeth take only the tip clearance, tip shortening, compensation and gear 2's backlash"),
            _ => None,
        },
        Kind::Worm => match o {
            Backlash2 => None,
            Hardware2 => body("the worm has no hub"),
            Face2 => body("the face width is the wheel's"),
            _ if o.tooth_form() => teeth("the wheel's teeth are generated from the worm's thread"),
            _ => None,
        },
        Kind::Cycloidal | Kind::Sprocket | Kind::Pulley => match o {
            _ if o.tooth_form() => OWN_FORM,
            Face2 => OWN_FORM,
            _ if o.second() && s.teeth2 == 0 => NO_MATE,
            _ => None,
        },
        Kind::CycloidalDrive => match o {
            Bore | Holes => None,
            _ if o.tooth_form() || o == Face2 => teeth("the disc's lobes have their own form"),
            _ => body("the disc and the ring pins are plain plates: the disc takes a bore and output holes only"),
        },
        Kind::Spline => match o {
            Bore | HubDiameter => None,
            _ if o.tooth_form() || o == Face2 => OWN_FORM,
            _ => body("the shaft and the hub are the spline itself"),
        },
        Kind::FaceGear => match o {
            _ if o.tooth_form() || o == Face2 => OWN_FORM,
            FrontShaft | BackShaft | Hardware => body("the face gear itself takes none; the pinion takes gear 2's"),
            _ => None,
        },
        // Its members come from its own settings (Spec::diff).
        Kind::Differential => body("a differential's gears, case and drive are set by its own settings"),
        Kind::Elliptical => match o {
            _ if o.tooth_form() || o == Face2 => OWN_FORM,
            ToothChamfers => body("the tips are no circle"),
            _ => None,
        },
    }
}

/// The spec's type uses `o` (when it is set).
pub fn uses(s: &Spec, o: Opt) -> bool {
    why_left_out(s, o).is_none()
}

/// Warns once a group (one reason) about the options that are set and left out.
pub(crate) fn left_out(s: &Spec, p: &mut Plan) {
    let what = if s.train.is_some() { "gear train".to_string() } else { s.kind.label().to_lowercase() };
    // A spline's two members are its shaft and its hub: their options aren't told apart.
    let qualify = s.kind != Kind::Spline;
    let mut groups: Vec<(Why, Vec<String>)> = Vec::new();
    for o in Opt::ALL.iter().copied() {
        if !o.is_set(s) {
            continue;
        }
        let Some(why) = why_left_out(s, o) else { continue };
        let label = if o.second() && (qualify || o == Opt::Bore2) { format!("gear 2's {}", o.label()) } else { o.label().to_string() };
        match groups.iter_mut().find(|(w, _)| *w == why) {
            Some((_, list)) => {
                if !list.contains(&label) {
                    list.push(label);
                }
            }
            None => groups.push((why, vec![label])),
        }
    }
    for (why, list) in groups {
        if why.teeth {
            p.warn(format!("{what}: {} don't apply to its teeth ({}); left out", list.join(", "), why.reason));
        } else {
            p.warn(format!("{what}: {} left out ({})", list.join(", "), why.reason));
        }
    }
}
