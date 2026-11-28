//! Simple mode: design a set by what it must do (shafts, sizes, a ratio) and let the solver pick
//! the rest (module, teeth, shifts, face widths).
//!
//! - [`Simple`]: the outputs a person thinks in, all optional: how the two shafts lie (angle and
//!   offset, or a [`Relation`]), a ratio, ONE size driver (a pitch diameter of either member, a
//!   centre distance, an outer diameter or a cone distance) and per-family extras (helix,
//!   spiral angle, starts, belt, chain…). `kind: None` lets [`auto_kind`] choose the family from
//!   the shafts.
//! - [`solve`]: a [`Simple`] and a base [`Spec`] (whose Advanced settings survive) to a buildable
//!   spec, with what was chosen and why.
//! - [`outputs_of`] / [`measures`]: the reverse, a spec's driving outputs / all its outputs.
//! - [`schematic`]: the set without teeth (axes, pitch/tip/root cylinders and cones, worm helix,
//!   chain or belt path) and the dimensions to annotate, for an instant 3D preview and drawing.
//!
//! Sketch rules (circles are pitch circles, lines are shafts): the window's recogniser measures
//! the circles and lines and fills [`Simple`]; this module turns the drawn pitch diameters into
//! standard modules and whole teeth, reports drawn against achieved, keeps centre distances exact
//! by profile shift, and with `exact` uses the non-standard module that hits them exactly.
//!
//! Members and their fields, per family (see [`fields`] for labels):
//!
//! | family | member 1 (`pitch_diameter`) | member 2 (`pitch_diameter2`) | `ring_diameter` |
//! |---|---|---|---|
//! | spur / helical / herringbone | the gear | – | – |
//! | pair | gear 1 | gear 2 | – |
//! | internal | the pinion inside (not built) | – | the ring |
//! | planetary | sun | planet | ring |
//! | rack | the pinion (not built) | – | – |
//! | bevel, spiral bevel, hypoid, face gear | pinion | gear (ring gear) | – |
//! | bevel planetary | sun | planet | – (the second sun or ring follows from the cone angle) |
//! | crossed helical | gear 1 | gear 2 | – |
//! | worm | worm | wheel | – |
//! | cycloidal | wheel | pinion | – |
//! | sprocket / pulley | sprocket (pulley) 1 | 2 | – |
//! | spline | reference diameter | – | – |
//! | elliptical | – | – | – |
//!
//! `ratio` is always the speed reduction from member 1 (the driver) to member 2: gear 2's teeth
//! over gear 1's; a worm's wheel teeth over its starts; a planetary set's ring fixed, sun in,
//! carrier out; a cycloidal drive's pins − 1; cycloidal clock gears wheel over pinion; a bevel
//! planetary set's planet teeth over the sun's (carrier fixed; its reductions are in `chosen`).

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use crate::axes::{self, Axis, Drive, Relation, num};
use crate::design::{self, Goal, ISO54_SERIES_2};
use crate::hub::{Bore, BoreKind};
use crate::involute::{Internal, Involute, mesh_external, shift_sum_for_center};
use crate::plan::bevel_planet::{self, BevelPlanetMode};
use crate::plan::{Fact, Frame, Kind, Spec, Unit};

// =============================================================================================
// What a person gives.

/// A set described by its outputs. Lengths in mm, angles in degrees; `None`: work it out.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Simple {
    /// The family (`None`: [`auto_kind`]'s first for the shafts and sizes).
    pub kind: Option<Kind>,
    /// The angle between the two shafts (0: parallel or the same axis; 90: square) and the
    /// shortest distance between them (parallel: the centre distance; skew: the offset).
    pub shaft_angle: Option<f64>,
    pub offset: Option<f64>,
    /// Speed reduction from member 1 to member 2 (see the module docs).
    pub ratio: Option<f64>,
    /// Size drivers: centre distance (parallel pairs, crossed helical, worm; otherwise
    /// `offset`), pitch diameters (see the table in the module docs), the ring's pitch diameter,
    /// the largest outer (tip) diameter, a bevel's outer cone distance.
    pub center_distance: Option<f64>,
    pub pitch_diameter: Option<f64>,
    pub pitch_diameter2: Option<f64>,
    pub ring_diameter: Option<f64>,
    pub outer_diameter: Option<f64>,
    pub cone_distance: Option<f64>,
    /// Pins: member 1's teeth (a single gear's, the pinion's, the sun's, a worm wheel's teeth
    /// are member 2's: use `ratio` and `starts`), and the module.
    pub teeth: Option<u32>,
    pub module: Option<f64>,
    /// Hit drawn pitch diameters exactly with a non-standard module (otherwise the nearest
    /// ISO 54 module, series I before II, and the centre distance by profile shift).
    pub exact: bool,
    /// Face widths (member 2's: pairs only; default ≈ 10 × module, 30 % of the cone distance
    /// for bevels and hypoids).
    pub face_width: Option<f64>,
    pub face_width2: Option<f64>,
    /// Parallel and single gears: helix angle (0 spur, 15–30 typical), its hand (gear 1 left-
    /// handed; also a worm's thread and a hypoid pinion's spiral) and herringbone teeth.
    pub helix: Option<f64>,
    pub left_hand: Option<bool>,
    pub herringbone: bool,
    /// Crossed helical: gear 1's helix angle β1 (β2 = Σ − β1; default Σ / 2).
    pub helix_split: Option<f64>,
    /// Spiral bevel and hypoid pinion mean spiral angle.
    pub spiral_angle: Option<f64>,
    pub planets: Option<u32>,
    /// Bevel planetary: what meshes the planets besides the sun (`None`: the base spec's).
    pub bevel_planet_mode: Option<BevelPlanetMode>,
    pub starts: Option<u32>,
    /// Rack length.
    pub length: Option<f64>,
    /// Elliptical gears' eccentricity.
    pub eccentricity: Option<f64>,
    /// Round bores of member 1 and member 2 (0: none).
    pub bore: Option<f64>,
    pub bore2: Option<f64>,
    /// Chain (an id of [`crate::sprocket::CHAINS`]) and belt (of [`crate::pulley::BELTS`]).
    pub chain: Option<&'static str>,
    pub belt: Option<&'static str>,
    /// Apply the FDM print settings ([`Spec::apply_print_defaults`], 0.4 mm nozzle, 0.2 mm
    /// layers) and keep modules at 1 or more.
    pub printable: bool,
}

/// The fields a form shows, for [`Simple::get`] and [`Simple::set`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    ShaftAngle,
    Offset,
    Ratio,
    CenterDistance,
    PitchDiameter,
    PitchDiameter2,
    RingDiameter,
    OuterDiameter,
    ConeDistance,
    Teeth,
    Module,
    FaceWidth,
    FaceWidth2,
    Helix,
    HelixSplit,
    SpiralAngle,
    Planets,
    Starts,
    Length,
    Eccentricity,
    Bore,
    Bore2,
    /// Not numbers: `get` and `set` ignore them (use the struct's fields).
    LeftHand,
    Herringbone,
    Exact,
    Chain,
    Belt,
    Printable,
    /// Bevel planetary: `Simple::bevel_planet_mode`.
    PlanetMode,
}

impl Field {
    pub fn numeric(self) -> bool {
        !matches!(self, Field::LeftHand | Field::Herringbone | Field::Exact | Field::Chain | Field::Belt | Field::Printable | Field::PlanetMode)
    }
}

/// One row of a form.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldInfo {
    pub field: Field,
    pub label: &'static str,
    pub unit: Unit,
}

fn fi(field: Field, label: &'static str, unit: Unit) -> FieldInfo {
    FieldInfo { field, label, unit }
}

/// The fields that matter for a family, in the order to show them (`None`: the shafts-first
/// form, whose family comes from [`auto_kind`]).
pub fn fields(kind: Option<Kind>) -> Vec<FieldInfo> {
    use Field as F;
    use Unit::{Deg, Mm, None as N};
    let Some(kind) = kind else {
        return vec![
            fi(F::ShaftAngle, "Shaft angle", Deg),
            fi(F::Offset, "Shaft offset / distance", Mm),
            fi(F::Ratio, "Ratio", N),
            fi(F::PitchDiameter, "Driver pitch diameter", Mm),
            fi(F::PitchDiameter2, "Driven pitch diameter", Mm),
            fi(F::Helix, "Helix angle", Deg),
            fi(F::FaceWidth, "Face width", Mm),
            fi(F::Printable, "3D printable", N),
        ];
    };
    let face = fi(F::FaceWidth, "Face width", Mm);
    let print = fi(F::Printable, "3D printable", N);
    let bore = fi(F::Bore, "Bore", Mm);
    match kind {
        Kind::Spur | Kind::Helical | Kind::Herringbone => {
            let mut v = vec![
                fi(F::PitchDiameter, "Pitch diameter", Mm),
                fi(F::OuterDiameter, "Outer diameter", Mm),
                fi(F::Teeth, "Teeth", N),
                fi(F::Module, "Module", N),
                face,
            ];
            if kind != Kind::Spur {
                v.push(fi(F::Helix, "Helix angle", Deg));
                v.push(fi(F::LeftHand, "Left hand", N));
            }
            v.extend([bore, print]);
            v
        }
        Kind::Pair => vec![
            fi(F::Ratio, "Ratio", N),
            fi(F::CenterDistance, "Centre distance", Mm),
            fi(F::PitchDiameter, "Gear 1 pitch diameter", Mm),
            fi(F::PitchDiameter2, "Gear 2 pitch diameter", Mm),
            fi(F::OuterDiameter, "Largest outer diameter", Mm),
            fi(F::Teeth, "Gear 1 teeth", N),
            fi(F::Helix, "Helix angle (0: spur)", Deg),
            fi(F::LeftHand, "Gear 1 left hand", N),
            fi(F::Herringbone, "Herringbone", N),
            face,
            fi(F::FaceWidth2, "Gear 2 face width", Mm),
            fi(F::Exact, "Exact diameters (non-standard module)", N),
            bore,
            fi(F::Bore2, "Gear 2 bore", Mm),
            print,
        ],
        Kind::Internal => vec![
            fi(F::RingDiameter, "Ring pitch diameter", Mm),
            fi(F::PitchDiameter, "Pinion pitch diameter", Mm),
            fi(F::OuterDiameter, "Ring outside diameter", Mm),
            fi(F::Teeth, "Ring teeth", N),
            fi(F::Module, "Module", N),
            face,
            print,
        ],
        Kind::Planetary => vec![
            fi(F::Ratio, "Ratio (ring fixed)", N),
            fi(F::Planets, "Planets", N),
            fi(F::RingDiameter, "Ring pitch diameter", Mm),
            fi(F::PitchDiameter, "Sun pitch diameter", Mm),
            fi(F::PitchDiameter2, "Planet pitch diameter", Mm),
            fi(F::OuterDiameter, "Ring outside diameter", Mm),
            face,
            print,
        ],
        Kind::Rack => vec![
            fi(F::Length, "Rack length", Mm),
            fi(F::PitchDiameter, "Pinion pitch diameter", Mm),
            fi(F::Teeth, "Pinion teeth", N),
            fi(F::Module, "Module", N),
            face,
            print,
        ],
        Kind::Bevel | Kind::SpiralBevel => {
            let mut v = vec![
                fi(F::ShaftAngle, "Shaft angle", Deg),
                fi(F::Ratio, "Ratio", N),
                fi(F::PitchDiameter, "Pinion outer pitch diameter", Mm),
                fi(F::PitchDiameter2, "Gear outer pitch diameter", Mm),
                fi(F::ConeDistance, "Cone distance", Mm),
                fi(F::Teeth, "Pinion teeth", N),
                face,
            ];
            if kind == Kind::SpiralBevel {
                v.push(fi(F::SpiralAngle, "Spiral angle", Deg));
            }
            v.extend([bore, fi(F::Bore2, "Gear bore", Mm), print]);
            v
        }
        Kind::Hypoid => vec![
            fi(F::ShaftAngle, "Shaft angle", Deg),
            fi(F::Offset, "Offset", Mm),
            fi(F::Ratio, "Ratio", N),
            fi(F::PitchDiameter2, "Ring gear outer pitch diameter", Mm),
            fi(F::PitchDiameter, "Pinion pitch diameter (sets the ring by the ratio)", Mm),
            fi(F::SpiralAngle, "Pinion spiral angle", Deg),
            fi(F::LeftHand, "Left-hand pinion", N),
            face,
        ],
        Kind::CrossedHelical => vec![
            fi(F::ShaftAngle, "Shaft angle", Deg),
            fi(F::CenterDistance, "Centre distance (offset)", Mm),
            fi(F::Ratio, "Ratio", N),
            fi(F::PitchDiameter, "Gear 1 pitch diameter", Mm),
            fi(F::HelixSplit, "Gear 1 helix angle", Deg),
            face,
            print,
        ],
        Kind::Worm => vec![
            fi(F::Ratio, "Ratio", N),
            fi(F::CenterDistance, "Centre distance", Mm),
            fi(F::PitchDiameter2, "Wheel pitch diameter", Mm),
            fi(F::PitchDiameter, "Worm pitch diameter", Mm),
            fi(F::Starts, "Starts", N),
            fi(F::LeftHand, "Left hand", N),
            face,
            print,
        ],
        Kind::Cycloidal => vec![
            fi(F::Ratio, "Ratio (wheel ÷ pinion)", N),
            fi(F::CenterDistance, "Centre distance", Mm),
            fi(F::PitchDiameter, "Wheel pitch diameter", Mm),
            fi(F::Teeth, "Wheel teeth", N),
            face,
        ],
        Kind::CycloidalDrive => vec![
            fi(F::Ratio, "Reduction", N),
            fi(F::OuterDiameter, "Outer diameter (over the pins)", Mm),
            fi(F::PitchDiameter, "Pin circle diameter", Mm),
            face,
            bore,
        ],
        Kind::Sprocket => vec![
            fi(F::Chain, "Chain", N),
            fi(F::Teeth, "Teeth", N),
            fi(F::PitchDiameter, "Pitch diameter", Mm),
            fi(F::Ratio, "Ratio (adds the driven sprocket)", N),
            fi(F::CenterDistance, "Centre distance", Mm),
            bore,
        ],
        Kind::Pulley => vec![
            fi(F::Belt, "Belt", N),
            fi(F::Teeth, "Teeth", N),
            fi(F::PitchDiameter, "Pitch diameter", Mm),
            fi(F::FaceWidth, "Belt width", Mm),
            fi(F::Ratio, "Ratio (adds the driven pulley)", N),
            fi(F::CenterDistance, "Centre distance", Mm),
            bore,
        ],
        Kind::Spline => {
            vec![fi(F::PitchDiameter, "Reference diameter", Mm), fi(F::Teeth, "Teeth", N), fi(F::Module, "Module", N), fi(F::FaceWidth, "Length", Mm)]
        }
        Kind::FaceGear => vec![
            fi(F::Ratio, "Ratio", N),
            fi(F::PitchDiameter, "Pinion pitch diameter", Mm),
            fi(F::PitchDiameter2, "Face gear pitch diameter", Mm),
            fi(F::Teeth, "Pinion teeth", N),
            face,
        ],
        Kind::Elliptical => {
            vec![fi(F::CenterDistance, "Centre distance", Mm), fi(F::Teeth, "Teeth", N), fi(F::Eccentricity, "Eccentricity", N), face]
        }
        // Designed by its own goal (crate::diff::Goal): the window shows those fields.
        Kind::Differential => Vec::new(),
        Kind::BevelPlanetary => vec![
            fi(F::PlanetMode, "Mode", N),
            fi(F::ShaftAngle, "Cone angle (sun shaft to planet shaft)", Deg),
            fi(F::Ratio, "Ratio (planet ÷ sun teeth)", N),
            fi(F::Planets, "Planets", N),
            fi(F::PitchDiameter, "Sun outer pitch diameter", Mm),
            fi(F::PitchDiameter2, "Planet outer pitch diameter", Mm),
            fi(F::ConeDistance, "Cone distance", Mm),
            fi(F::Teeth, "Sun teeth", N),
            fi(F::SpiralAngle, "Spiral angle (0: straight)", Deg),
            face,
            bore,
            fi(F::Bore2, "Planet bore", Mm),
            print,
        ],
    }
}

impl Simple {
    /// A numeric field's value (counts as numbers).
    pub fn get(&self, f: Field) -> Option<f64> {
        let c = |v: Option<u32>| v.map(f64::from);
        match f {
            Field::ShaftAngle => self.shaft_angle,
            Field::Offset => self.offset,
            Field::Ratio => self.ratio,
            Field::CenterDistance => self.center_distance,
            Field::PitchDiameter => self.pitch_diameter,
            Field::PitchDiameter2 => self.pitch_diameter2,
            Field::RingDiameter => self.ring_diameter,
            Field::OuterDiameter => self.outer_diameter,
            Field::ConeDistance => self.cone_distance,
            Field::Teeth => c(self.teeth),
            Field::Module => self.module,
            Field::FaceWidth => self.face_width,
            Field::FaceWidth2 => self.face_width2,
            Field::Helix => self.helix,
            Field::HelixSplit => self.helix_split,
            Field::SpiralAngle => self.spiral_angle,
            Field::Planets => c(self.planets),
            Field::Starts => c(self.starts),
            Field::Length => self.length,
            Field::Eccentricity => self.eccentricity,
            Field::Bore => self.bore,
            Field::Bore2 => self.bore2,
            _ => None,
        }
    }
    /// Sets a numeric field (counts are rounded; `None` or a value that is not a number frees it).
    pub fn set(&mut self, f: Field, v: Option<f64>) {
        let v = v.filter(|x| x.is_finite());
        let c = v.map(|x| x.round().clamp(0.0, 1e6) as u32);
        match f {
            Field::ShaftAngle => self.shaft_angle = v,
            Field::Offset => self.offset = v,
            Field::Ratio => self.ratio = v,
            Field::CenterDistance => self.center_distance = v,
            Field::PitchDiameter => self.pitch_diameter = v,
            Field::PitchDiameter2 => self.pitch_diameter2 = v,
            Field::RingDiameter => self.ring_diameter = v,
            Field::OuterDiameter => self.outer_diameter = v,
            Field::ConeDistance => self.cone_distance = v,
            Field::Teeth => self.teeth = c,
            Field::Module => self.module = v,
            Field::FaceWidth => self.face_width = v,
            Field::FaceWidth2 => self.face_width2 = v,
            Field::Helix => self.helix = v,
            Field::HelixSplit => self.helix_split = v,
            Field::SpiralAngle => self.spiral_angle = v,
            Field::Planets => self.planets = c,
            Field::Starts => self.starts = c,
            Field::Length => self.length = v,
            Field::Eccentricity => self.eccentricity = v,
            Field::Bore => self.bore = v,
            Field::Bore2 => self.bore2 = v,
            _ => {}
        }
    }

    /// How the shafts lie, from `shaft_angle` and `offset` (or the centre distance when no
    /// offset is given): see [`relation_of`].
    pub fn relation(&self) -> Relation {
        relation_of(self.shaft_angle.unwrap_or(0.0), self.offset.or(self.center_distance).unwrap_or(0.0))
    }

    /// Takes the shafts' angle and offset from a relation (as [`axes::relate`] gives).
    pub fn set_relation(&mut self, rel: &Relation) {
        let (a, e) = match *rel {
            Relation::Coincident => (0.0, 0.0),
            Relation::Parallel { distance, .. } => (0.0, distance),
            Relation::Intersecting { angle, .. } => (angle, 0.0),
            Relation::Skew { angle, offset, .. } => (angle, offset),
        };
        self.shaft_angle = Some(a);
        self.offset = Some(e);
    }
}

/// Two shafts from numbers: the angle between them (degrees) and the shortest distance between
/// them. Shaft 1 is world Z through the origin; shaft 2 passes through (offset, 0, 0), turned
/// by the angle about x.
pub fn relation_of(angle: f64, offset: f64) -> Relation {
    let angle = if angle.is_finite() { angle.clamp(0.0, 180.0) } else { 0.0 };
    let e = if offset.is_finite() { offset.abs() } else { 0.0 };
    let parallel = angle.to_radians().sin().abs() < 1e-6;
    match (parallel, e < 1e-6) {
        (true, true) => Relation::Coincident,
        (true, false) => Relation::Parallel { distance: e, foot1: [0.0; 3], foot2: [e, 0.0, 0.0] },
        (false, true) => Relation::Intersecting { angle, apex: [0.0; 3] },
        (false, false) => Relation::Skew { angle, offset: e, foot1: [0.0; 3], foot2: [e, 0.0, 0.0] },
    }
}

// =============================================================================================
// Which family.

fn smooth(x: f64, a: f64, b: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Hypoid offsets: full score up to this fraction of the ring gear's pitch diameter (25 % of
/// de2, the usual limit in published hypoid design guidance), none from [`HYPOID_OFFSET_NONE`] (where a line drawn through
/// the ring gear's circle leaves it).
pub const HYPOID_OFFSET_FULL: f64 = 0.25;
pub const HYPOID_OFFSET_NONE: f64 = 0.5;
/// Worm sets: shafts this square (degrees off 90°), ratios from [`WORM_RATIO_LOW`] (none) to
/// [`WORM_RATIO_HIGH`] (full).
pub const WORM_SQUARE: f64 = 0.01;
pub const WORM_RATIO_LOW: f64 = 4.0;
pub const WORM_RATIO_HIGH: f64 = 8.0;
/// Crossed helicals suit low ratios: full up to [`CROSSED_RATIO_FULL`], none from
/// [`CROSSED_RATIO_HIGH`]. Their offset fits within about ±15 % of the pitch cylinders touching
/// (none past ±45 %), and only from 30 % of the larger gear's Ø (full from 45 %).
pub const CROSSED_RATIO_FULL: f64 = 2.5;
pub const CROSSED_RATIO_HIGH: f64 = 5.0;
/// Gear size assumed when none is given (the larger member's pitch diameter, mm): only to judge
/// an offset against; the solver sizes the set to the shafts.
pub const ASSUMED_SIZE: f64 = 100.0;

/// The families that fit two shafts and the sizes given, best first: (family, why, score).
/// Scores are comparable (1 a natural fit, 0 none) and change smoothly with the offset, angle
/// and sizes, so a window can show the first as a label and offer the others when they come
/// close. Rules:
///
/// - same axis: a single gear (helical / herringbone by the helix fields); with a ratio over
///   about 2.5, a planetary set (a cycloidal drive above about 10); internal ring and spline as
///   alternatives.
/// - parallel: a gear pair (spur/helical/herringbone are its helix fields); a timing belt or
///   chain when the sizes given leave the gears far short of the distance; cycloidal and
///   elliptical as alternatives.
/// - intersecting: straight bevel (spiral first when a spiral angle is set); a face gear at 90°.
/// - skew (the shafts miss), by the offset E against the larger member's pitch diameter D:
///   hypoid for E ≤ 25 % of D, fading out to 50 % (E → 0 is a spiral bevel); crossed helical
///   where the pitch cylinders touch, E ≈ D/2·(1 + 1/ratio), for ratios under about 2.5 to 5;
///   worm and wheel at 90° where E ≥ D/2 and the ratio is high (4 → 8) or the driver small.
pub fn auto_kind(relation: &Relation, sizes: &Simple) -> Vec<(Kind, &'static str, f64)> {
    let mut v: Vec<(Kind, &'static str, f64)> = Vec::new();
    let u = sizes.ratio.filter(|r| r.is_finite() && *r > 0.0).map(|r| if r < 1.0 { 1.0 / r } else { r });
    let (d1, d2) = (sizes.pitch_diameter.filter(|d| *d > 0.0), sizes.pitch_diameter2.filter(|d| *d > 0.0));
    let helical = sizes.helix.is_some_and(|h| h.abs() > 1e-9);
    let single = if sizes.herringbone {
        (Kind::Herringbone, "one gear on the axis (herringbone)")
    } else if helical {
        (Kind::Helical, "one gear on the axis (helical)")
    } else {
        (Kind::Spur, "one gear on the axis")
    };
    match *relation {
        Relation::Coincident => {
            let ur = u.unwrap_or(1.0);
            let geared = if u.is_some() { smooth(ur, 2.1, 2.6) } else { 0.0 };
            v.push((single.0, single.1, 1.0 - 0.8 * geared));
            v.push((
                Kind::Planetary,
                "input and output on one axis: sun in, carrier out, ring fixed",
                0.5 + 0.6 * geared * (1.0 - smooth(ur, 10.0, 14.0)),
            ));
            v.push((Kind::CycloidalDrive, "a large reduction on one axis", 0.2 + 0.9 * smooth(ur, 8.0, 14.0)));
            v.push((Kind::Internal, "a ring gear on the axis", 0.3));
            v.push((Kind::Spline, "a shaft-to-hub coupling (1:1)", 0.25 * (1.0 - geared)));
        }
        Relation::Parallel { distance, .. } => {
            // How far gears of the sizes given reach towards the distance.
            let reach = match (d1, d2, u) {
                (Some(a), Some(b), _) => Some((a + b) / 2.0),
                (Some(a), None, Some(r)) => Some(a * (1.0 + r) / 2.0),
                (None, Some(b), Some(r)) => Some(b * (1.0 + 1.0 / r) / 2.0),
                (Some(a), None, None) => Some(a),
                (None, Some(b), None) => Some(b),
                _ => None,
            };
            let far = reach.map_or(0.0, |r| smooth(distance / r.max(1e-9), 1.05, 1.5));
            let why = if sizes.herringbone {
                "parallel shafts: a herringbone pair"
            } else if helical {
                "parallel shafts: a helical pair"
            } else {
                "parallel shafts: a spur pair"
            };
            v.push((Kind::Pair, why, 1.0 - 0.9 * far));
            v.push((Kind::Pulley, "parallel shafts too far apart for gears of these sizes: a timing belt", 0.3 + 0.65 * far));
            v.push((Kind::Sprocket, "parallel shafts too far apart for gears of these sizes: a roller chain", 0.25 + 0.6 * far));
            v.push((Kind::Cycloidal, "parallel shafts: cycloidal (clock) gears", 0.2 * (1.0 - far)));
            v.push((Kind::Elliptical, "parallel shafts: a varying ratio", 0.1 * (1.0 - far)));
        }
        Relation::Intersecting { angle, .. } => {
            let af = angle.min(180.0 - angle);
            // (Cones flatter than 2° or opening past 85° either way round aren't built.)
            let buildable = u.is_none_or(|r| cones(angle, 1.0, r).is_some() || cones(180.0 - angle, 1.0, r).is_some());
            let ok = smooth(af, 3.0, 8.0) * if buildable { 1.0 } else { 0.0 };
            let spiral = sizes.spiral_angle.is_some_and(|s| s.abs() > 1e-9);
            v.push((Kind::Bevel, "the shafts meet: straight bevel gears, cones meeting where the shafts do", ok * if spiral { 0.85 } else { 1.0 }));
            v.push((Kind::SpiralBevel, "the shafts meet: spiral bevel gears (quieter, stronger)", ok * if spiral { 1.0 } else { 0.8 }));
            let square = 1.0 - smooth((af - 90.0).abs(), 0.0, 0.5);
            v.push((Kind::FaceGear, "the shafts meet square: a face gear and spur pinion", 0.4 * square));
            // Planets asked for (or a bevel planetary mode): planets round a bevel sun.
            let planets = sizes.planets.is_some_and(|n| n >= 1) || sizes.bevel_planet_mode.is_some();
            v.push((
                Kind::BevelPlanetary,
                "the shafts meet: bevel planets round a bevel sun (a differential, or a bevel epicyclic with a ring)",
                ok * if planets { 1.05 } else { 0.3 },
            ));
        }
        Relation::Skew { angle, offset, .. } => {
            let af = angle.min(180.0 - angle);
            let e = offset;
            // The larger member's pitch diameter, given or worked out from the other and the ratio.
            let big = match (d1, d2, u) {
                (Some(a), Some(b), _) => Some(a.max(b)),
                (Some(a), None, Some(r)) => Some(a * r),
                (None, Some(b), _) => Some(b),
                (Some(a), None, None) => Some(a),
                _ => None,
            };
            let sized = big.is_some();
            let dbig = big.unwrap_or(ASSUMED_SIZE);
            let q = e / dbig;
            let ur = u.unwrap_or(1.0);
            // Hypoid: small offsets for the ring gear; ratios 1.5 to 10; shafts not too oblique.
            let h_size = 1.0 - smooth(q, HYPOID_OFFSET_FULL, HYPOID_OFFSET_NONE);
            let h_ratio = if u.is_some() { smooth(ur, 1.0, 1.6) * (1.0 - smooth(ur, 8.0, 12.0)) } else { 1.0 };
            v.push((
                Kind::Hypoid,
                "the shafts miss by a small offset for the gear size: hypoid (offset 0 is a spiral bevel)",
                h_size * h_ratio * smooth(af, 20.0, 35.0),
            ));
            // Crossed helical: the pitch cylinders touch across the offset.
            let ec = dbig / 2.0 * (1.0 + 1.0 / ur);
            // (Within about ±15 % of touching in full, none past ±45 %; never where a hypoid is natural.)
            let c_size = if sized { 1.0 - smooth((e / ec).ln().abs(), 0.15, 0.6) } else { 1.0 } * smooth(q, 0.3, 0.45);
            let c_ratio = if u.is_some() { 1.0 - smooth(ur, CROSSED_RATIO_FULL, CROSSED_RATIO_HIGH) } else { 0.8 };
            v.push((
                Kind::CrossedHelical,
                "the shafts miss with the gears side by side: crossed helical gears, pitch cylinders touching across the offset",
                0.9 * c_size * c_ratio,
            ));
            // Worm: square shafts, a high ratio or a small driver, E about half the wheel and more.
            let square = if (af - 90.0).abs() <= WORM_SQUARE { 1.0 } else { 0.0 };
            let w_ratio = if u.is_some() { smooth(ur, WORM_RATIO_LOW, WORM_RATIO_HIGH) } else { 0.5 };
            let w_small = match (d1, d2) {
                (Some(a), Some(b)) => 1.0 - smooth(a / b, 0.4, 0.8),
                _ => 0.0,
            };
            let w_size = match (d1, d2) {
                (Some(a), Some(b)) => 1.0 - smooth((2.0 * e / (a + b)).ln().abs(), 0.1, 0.4),
                // The worm's circle alone: the wheel is what is left of twice the offset.
                (Some(a), None) if u.is_none() => smooth((2.0 * e - a) / a, 1.5, 2.5),
                _ if sized => smooth(q, 0.45, 0.55) * (1.0 - smooth(q, 0.9, 1.3)),
                _ => smooth(q, 0.45, 0.55),
            };
            v.push((Kind::Worm, "square shafts far apart for the size with a high ratio: worm and wheel", square * w_size * w_ratio.max(w_small)));
        }
    }
    // Stable order: by score, then as listed.
    let mut idx: Vec<usize> = (0..v.len()).collect();
    idx.sort_by(|a, b| v[*b].2.total_cmp(&v[*a].2).then(a.cmp(b)));
    idx.into_iter().map(|i| v[i]).collect()
}

/// The family `solve` uses for a [`Simple`]: its own, or [`auto_kind`]'s first.
pub fn kind_of(s: &Simple) -> Kind {
    s.kind.unwrap_or_else(|| auto_kind(&s.relation(), s).first().map_or(Kind::Spur, |c| c.0))
}

// =============================================================================================
// Solving.

/// What [`solve`] gives.
#[derive(Clone, Debug, PartialEq)]
pub struct Solved {
    pub kind: Kind,
    /// The set to plan (always filled: the best the solver could do, also with an error).
    pub spec: Spec,
    /// What was chosen and what it came to (module, teeth, shifts, diameters, achieved ratio and
    /// its error, drawn against achieved sizes).
    pub chosen: Vec<Fact>,
    /// Why, and where a target could not be met exactly.
    pub notes: Vec<String>,
    /// Why the set can't be made (the targets don't fit, or the spec fails [`crate::plan::check`]).
    pub error: Option<String>,
    /// The families that fit when `kind` was None ([`auto_kind`]; empty otherwise).
    pub alternatives: Vec<(Kind, &'static str, f64)>,
    /// A member the plan doesn't build but the drawing has: the pinion inside an internal ring,
    /// the pinion on a rack (a spur or helical gear spec, placed where it meshes).
    pub companion: Option<Spec>,
}

struct Ctx<'a> {
    s: &'a Simple,
    base: &'a Spec,
    same: bool,
    spec: Spec,
    chosen: Vec<Fact>,
    notes: Vec<String>,
    companion: Option<Spec>,
}

impl Ctx<'_> {
    fn row(&mut self, label: &str, value: f64, unit: Unit) {
        self.chosen.push(Fact { label: label.into(), value, unit });
    }
    fn note(&mut self, n: impl Into<String>) {
        self.notes.push(n.into());
    }
    /// Drawn against achieved, as rows and (when off) a note.
    fn versus(&mut self, what: &str, drawn: Option<f64>, got: f64) {
        self.row(what, got, Unit::Mm);
        if let Some(d) = drawn {
            let err = got - d;
            if err.abs() > 1e-6 {
                self.row(&format!("{what} asked"), d, Unit::Mm);
                self.note(format!(
                    "{what}: {} asked, {} with the standard module ({:+.2} %); tick Exact for a non-standard module that hits it",
                    num(d),
                    num(got),
                    100.0 * err / d
                ));
            }
        }
    }
    fn module_unchanged(&self) -> bool {
        self.same && (self.spec.module - self.base.module).abs() < 1e-12
    }
    /// A face width: the one asked, the base's when the module stayed, or `default`.
    fn face(&mut self, default: f64) {
        self.spec.face_width = match self.s.face_width {
            Some(f) if f > 0.0 => f,
            _ if self.module_unchanged() => self.base.face_width,
            _ => round_to(default, 0.5).max(1.0),
        };
    }
    fn signed_helix(&self, kind: Kind) -> f64 {
        if kind == Kind::Spur {
            return 0.0;
        }
        let mag = self.s.helix.map(f64::abs).unwrap_or(self.spec.helix.abs());
        let left = self.s.left_hand.unwrap_or(self.spec.helix < 0.0);
        if left { -mag } else { mag }
    }
}

fn round_to(v: f64, step: f64) -> f64 {
    (v / step).round() * step
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// The fewest teeth without undercut for the spec's tooth form at a helix angle.
fn zmin(spec: &Spec, helix_deg: f64) -> u32 {
    let g = Involute {
        module: 1.0,
        pressure_angle: spec.pressure_angle.to_radians(),
        helix: helix_deg.to_radians(),
        rack: spec.rack_eff(),
        shift: 0.0,
        ..Involute::default()
    };
    (g.min_teeth_no_undercut() - 1e-9).ceil().max(4.0) as u32
}

/// The profile shift that just clears undercut on `z` teeth (0 when none is needed).
fn shift_against_undercut(spec: &Spec, z: u32, helix_deg: f64) -> f64 {
    let g = Involute {
        module: 1.0,
        pressure_angle: spec.pressure_angle.to_radians(),
        helix: helix_deg.to_radians(),
        rack: spec.rack_eff(),
        ..Involute::default()
    };
    let least = g.min_teeth_no_undercut();
    let x = g.rack.addendum * (least - f64::from(z)) / least;
    if x <= 0.0 { 0.0 } else { (x * 100.0).ceil() / 100.0 }
}

/// Modules to try: the pinned one, or the ISO 54 series (I and II, fine ones under 1) between
/// the bounds; true for second choice.
fn modules(pin: Option<f64>, lo: f64, hi: f64, printable: bool) -> Vec<(f64, bool)> {
    if let Some(m) = pin.filter(|m| *m > 0.0) {
        return vec![(m, false)];
    }
    let lo = if printable { lo.max(1.0) } else { lo };
    let hi = hi.max(lo);
    let mut v: Vec<(f64, bool)> = design::standard_modules(lo, hi, true).into_iter().map(|m| (m, ISO54_SERIES_2.contains(&m))).collect();
    if v.is_empty() {
        // Out of the table: the nearest standard one.
        let all = design::standard_modules(0.0, 100.0, false);
        let want = 0.5 * (lo + hi);
        if let Some(m) = all.into_iter().min_by(|a, b| (a / want).ln().abs().total_cmp(&(b / want).ln().abs())) {
            v.push((m, false));
        }
    }
    v
}

/// Tooth counts for a ratio at a module that leaves the size free: hunting where it costs no
/// accuracy, member 1 near `want`, both at `least` or more.
fn counts(ratio: f64, want: u32, least: u32, pin: Option<u32>) -> (u32, u32) {
    let lo = least.max(3);
    let range: Vec<u32> = match pin {
        Some(z) => vec![z.max(3)],
        None => (lo..=lo.max(want) + 80).collect(),
    };
    let mut best = (f64::MAX, want.max(lo), ((f64::from(want.max(lo)) * ratio).round() as u32).max(lo));
    for z1 in range {
        let z2 = (f64::from(z1) * ratio).round().max(1.0) as u32;
        if z2 < lo && pin.is_none() {
            continue;
        }
        let z2 = z2.max(3);
        let err = (f64::from(z2) / f64::from(z1) / ratio).ln().abs();
        let score = 400.0 * err + 0.02 * (f64::from(z1) - f64::from(want)).abs() + if gcd(z1, z2) == 1 { 0.0 } else { 0.05 };
        if score < best.0 - 1e-12 {
            best = (score, z1, z2);
        }
    }
    (best.1, best.2)
}

/// Where the carried-over Advanced settings come from: the base when it is the same family,
/// otherwise the family's defaults with the base's tooth form, finish and hardware.
fn start(kind: Kind, base: &Spec) -> Spec {
    if base.kind == kind {
        return base.clone();
    }
    let mut s = Spec::defaults_for(kind);
    let b = base;
    if kind != Kind::Spline && b.kind != Kind::Spline && kind != Kind::Hypoid && b.kind != Kind::Hypoid {
        s.pressure_angle = b.pressure_angle;
    }
    s.rack = b.rack;
    s.backlash = b.backlash;
    s.backlash2 = b.backlash2;
    if kind != Kind::Hypoid {
        s.bore = b.bore;
        s.bore2 = b.bore2;
    }
    s.hub_diameter = b.hub_diameter;
    s.hub_length = b.hub_length;
    s.holes = b.holes;
    s.hole_diameter = b.hole_diameter;
    s.origin = b.origin;
    s.name = b.name.clone();
    s.placement = b.placement;
    s.load = b.load.clone();
    s.tip_round = b.tip_round;
    s.xy_compensation = b.xy_compensation;
    s.elephant_foot = b.elephant_foot;
    s.elephant_foot_height = b.elephant_foot_height;
    s.nozzle = b.nozzle;
    s.clearance = b.clearance;
    s.tip_shortening = b.tip_shortening;
    s.tooth_chamfer_front = b.tooth_chamfer_front;
    s.tooth_chamfer_back = b.tooth_chamfer_back;
    s.bore_chamfer = b.bore_chamfer;
    s.hub_chamfer = b.hub_chamfer;
    s.shaft_front = b.shaft_front;
    s.shaft_back = b.shaft_back;
    s.shaft2_front = b.shaft2_front;
    s.shaft2_back = b.shaft2_back;
    s.hardware = b.hardware;
    s.hardware2 = b.hardware2;
    s
}

/// Solves a set from its outputs. `base` holds the Advanced settings to keep (tooth form,
/// backlash, print tuning, chamfers, shafts, hardware, load, name, placement); when it is the
/// same family its module and teeth are kept wherever they already meet the targets, so a set
/// switched from Advanced to Simple ([`outputs_of`]) solves to itself. Fast: well under 5 ms.
pub fn solve(simple: &Simple, base: &Spec) -> Solved {
    let alternatives = if simple.kind.is_none() { auto_kind(&simple.relation(), simple) } else { Vec::new() };
    let kind = simple.kind.unwrap_or_else(|| alternatives.first().map_or(Kind::Spur, |c| c.0));
    let mut c = Ctx { s: simple, base, same: base.kind == kind, spec: start(kind, base), chosen: Vec::new(), notes: Vec::new(), companion: None };
    if simple.kind.is_none()
        && let Some(first) = alternatives.first()
    {
        c.note(format!("{}: {}", kind.label(), first.1));
    }
    if simple.printable {
        c.spec.apply_print_defaults(0.4, 0.2);
    }
    let r = match kind {
        Kind::Spur | Kind::Helical | Kind::Herringbone => single(&mut c, kind),
        Kind::Pair => pair(&mut c),
        Kind::Internal => internal(&mut c),
        Kind::Planetary => planetary(&mut c),
        Kind::Rack => rack(&mut c),
        Kind::Bevel | Kind::SpiralBevel => bevel(&mut c, kind),
        Kind::Hypoid => hypoid(&mut c),
        Kind::CrossedHelical => crossed(&mut c),
        Kind::Worm => worm(&mut c),
        Kind::Cycloidal => cycloidal(&mut c),
        Kind::CycloidalDrive => drive(&mut c),
        Kind::Sprocket | Kind::Pulley => wrapped(&mut c, kind),
        Kind::Spline => spline(&mut c),
        Kind::FaceGear => face_gear(&mut c),
        Kind::Elliptical => elliptical(&mut c),
        Kind::BevelPlanetary => bevel_planetary(&mut c),
        Kind::Differential => differential(&mut c),
    };
    if simple.printable {
        // Again, now that the module is known (the tip rounding and compensation follow it).
        c.spec.apply_print_defaults(0.4, 0.2);
    }
    for (d, bore) in [(simple.bore, &mut c.spec.bore), (simple.bore2, &mut c.spec.bore2)] {
        if let Some(d) = d {
            if d > 0.0 {
                let kind = if bore.kind == BoreKind::None { BoreKind::Round } else { bore.kind };
                *bore = Bore { kind, diameter: d, ..*bore };
            } else {
                *bore = Bore::default();
            }
        }
    }
    let mut error = r.err();
    if error.is_none()
        && let Err(e) = crate::plan::check(&c.spec)
    {
        error = Some(e);
    }
    Solved { kind, spec: c.spec, chosen: c.chosen, notes: c.notes, error, alternatives, companion: c.companion }
}

/// A differential is designed from its own goal ([`crate::diff::solve`]: axle, ratio, torque,
/// style) in its own panel: here its settings stay as they are, and the rows say what they are.
fn differential(c: &mut Ctx) -> Result<(), String> {
    let d = c.spec.diff.clone();
    c.row("differential module", d.module, Unit::None);
    c.row("side gear teeth", f64::from(d.side_teeth), Unit::None);
    c.row("pinion or element teeth", f64::from(d.pinion_teeth), Unit::None);
    c.row("axle diameter", d.axle_diameter, Unit::Mm);
    if d.drive != crate::diff::DriveKind::None {
        c.row("final drive ratio", f64::from(d.ring_teeth) / f64::from(d.drive_pinion_teeth.max(1)), Unit::None);
    }
    c.note("a differential is designed from its goal (axle, ratio, torque, style) in its own panel");
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Single gears.

/// The best (module, teeth) for a size `target` that `size(m, z)` gives, teeth in `zr`.
fn fit_one(
    mods: &[(f64, bool)],
    zr: (u32, u32, u32),
    pin_z: Option<u32>,
    target: f64,
    size: &dyn Fn(f64, u32) -> f64,
    base: Option<(f64, u32)>,
) -> Option<(f64, u32)> {
    let mut best: Option<(f64, f64, u32)> = None;
    for &(m, second) in mods {
        let zs: Vec<u32> = match pin_z {
            Some(z) => vec![z],
            None => {
                // The size grows with z: the counts either side of the target.
                let per = (size(m, 101) - size(m, 100)).max(1e-12);
                let z0 = ((target - size(m, 0)) / per).floor();
                [z0, z0 + 1.0].into_iter().filter(|z| *z >= f64::from(zr.0) && *z <= f64::from(zr.1)).map(|z| z as u32).collect()
            }
        };
        for z in zs {
            let got = size(m, z);
            let pct = 100.0 * (got - target).abs() / target.max(1e-9);
            let stay = base.map_or(0.0, |(bm, bz)| if (bm - m).abs() < 1e-12 && bz == z { -0.01 } else { 0.0 });
            let ideal = f64::from(zr.2);
            let score =
                pct + 0.03 * (f64::from(z) - ideal).max(0.0) + 0.1 * (0.75 * ideal - f64::from(z)).max(0.0) + if second { 0.3 } else { 0.0 } + stay;
            if best.is_none_or(|b| score < b.0 - 1e-12) {
                best = Some((score, m, z));
            }
        }
    }
    best.map(|b| (b.1, b.2))
}

fn single(c: &mut Ctx, kind: Kind) -> Result<(), String> {
    let s = c.s;
    let helix = c.signed_helix(kind);
    c.spec.helix = helix;
    let cb = helix.to_radians().cos();
    let zlo = zmin(&c.spec, helix).min(s.teeth.unwrap_or(u32::MAX));
    let ha = c.spec.rack_eff().addendum;
    let pitch = move |m: f64, z: u32| m * f64::from(z) / cb;
    let outer = move |m: f64, z: u32| m * f64::from(z) / cb + 2.0 * ha * m;
    let base = c.same.then_some((c.base.module, c.base.teeth));
    let (target, size, what): (Option<f64>, Box<dyn Fn(f64, u32) -> f64>, &str) = match (s.pitch_diameter, s.outer_diameter) {
        (Some(d), _) if d > 0.0 => (Some(d), Box::new(pitch), "pitch"),
        (_, Some(d)) if d > 0.0 => (Some(d), Box::new(outer), "outer"),
        _ => (None, Box::new(pitch), ""),
    };
    match (target, s.module, s.teeth) {
        (Some(d), None, Some(z)) if s.exact => {
            c.spec.module = if what == "pitch" { d * cb / f64::from(z) } else { d / (f64::from(z) / cb + 2.0 * ha) };
            c.spec.teeth = z;
            c.note(format!("non-standard module {} hits Ø{} exactly", num(c.spec.module), num(d)));
        }
        (Some(d), pin_m, pin_z) => {
            let guess = d * cb / 24.0;
            let mods = if s.exact && pin_m.is_none() { vec![(guess, false)] } else { modules(pin_m, guess / 6.0, guess * 4.0, s.printable) };
            let mods = if s.exact && pin_m.is_none() {
                // Exact: the standard module's teeth, then the module that hits the size.
                let (m, z) =
                    fit_one(&modules(None, guess / 6.0, guess * 4.0, s.printable), (zlo, 400, 24), pin_z, d, &*size, base).unwrap_or((guess, 24));
                let exact = m * d / size(m, z);
                vec![(exact, false)]
            } else {
                mods
            };
            let (m, z) = fit_one(&mods, (zlo, 400, 24), pin_z, d, &*size, base).ok_or("no module and teeth give that diameter")?;
            c.spec.module = m;
            c.spec.teeth = z;
        }
        (None, pin_m, pin_z) => {
            if let Some(m) = pin_m {
                c.spec.module = m;
            }
            if let Some(z) = pin_z {
                c.spec.teeth = z;
            }
        }
    }
    c.spec.shift = if c.spec.teeth < zmin(&c.spec, helix) {
        shift_against_undercut(&c.spec, c.spec.teeth, helix)
    } else if c.same && c.module_unchanged() && c.spec.teeth == c.base.teeth {
        c.base.shift
    } else {
        0.0
    };
    let m = c.spec.module;
    c.face(10.0 * m);
    let g = c.spec.involute(c.spec.teeth, c.spec.shift, helix);
    c.row("module", m, Unit::None);
    c.row("teeth", f64::from(c.spec.teeth), Unit::None);
    if c.spec.shift != 0.0 {
        c.row("profile shift", c.spec.shift, Unit::None);
        c.note(format!("{} teeth would be undercut: profile shift {} clears it", c.spec.teeth, num(c.spec.shift)));
    }
    c.versus("pitch diameter", s.pitch_diameter, 2.0 * g.r());
    c.versus("outer diameter", if s.pitch_diameter.is_some() { None } else { s.outer_diameter }, 2.0 * g.ra());
    if helix != 0.0 {
        c.row("helix angle", helix, Unit::Deg);
    }
    c.row("face width", c.spec.face_width, Unit::Mm);
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Parallel pairs.

/// A pair's working geometry as the plan makes it: gear 1, gear 2 (gear 2's shift solved for a
/// centre distance) and the centre distance.
fn pair_gears(s: &Spec) -> (Involute, Involute, f64) {
    let g1 = s.involute(s.teeth, s.shift, s.helix);
    let mut x2 = s.shift2;
    if s.center_distance > 0.0
        && let Some(sum) = shift_sum_for_center(&g1, &s.involute2(s.teeth2, 0.0, -s.helix), s.center_distance)
    {
        x2 = sum - s.shift;
    }
    let mut g1 = g1;
    let mut g2 = s.involute2(s.teeth2, x2, -s.helix);
    let a = mesh_external(&g1, &g2, s.face_width).center_distance;
    let shorten = (s.shift + x2) * s.module - (a - g1.r() - g2.r());
    if shorten > 1e-9 {
        g1.tip_shortening += shorten;
        g2.tip_shortening += shorten;
    }
    (g1, g2, a)
}

fn pair(c: &mut Ctx) -> Result<(), String> {
    let s = c.s;
    let helix = c.signed_helix(Kind::Pair);
    c.spec.helix = helix;
    c.spec.herringbone = s.herringbone;
    if s.herringbone && helix == 0.0 {
        c.note("herringbone needs a helix angle: give one, or the pair is built spur");
    }
    let cb = helix.to_radians().cos();
    let (d1, d2) = (s.pitch_diameter.filter(|d| *d > 0.0), s.pitch_diameter2.filter(|d| *d > 0.0));
    let ratio = s.ratio.or(match (d1, d2) {
        (Some(a), Some(b)) => Some(b / a),
        _ => None,
    });
    let ratio = ratio.unwrap_or(f64::from(c.spec.teeth2) / f64::from(c.spec.teeth));
    if !(ratio.is_finite() && ratio > 0.0) {
        return Err("the ratio must be greater than 0".into());
    }
    let zlo = zmin(&c.spec, helix);
    let cd = s.center_distance.or(s.offset).filter(|a| *a > 0.0).or(match (d1, d2) {
        (Some(a), Some(b)) => Some((a + b) / 2.0),
        _ => None,
    });
    let base = c.same.then_some((c.base.module, c.base.teeth, c.base.teeth2));
    if let Some(a) = cd {
        pair_at(c, ratio, a, helix)?;
    } else if let Some(d) = d1.or(d2) {
        let first = d1.is_some();
        let guess = d * cb / 24.0;
        let mods = modules(s.module, guess / 6.0, guess * 4.0, s.printable);
        let mut best: Option<(f64, f64, u32, u32)> = None;
        for (m, second) in mods {
            let mt = m / cb;
            let zc = if first { d / mt } else { d / mt / ratio };
            for z1 in [zc.floor(), zc.ceil()] {
                let z1 = s.teeth.unwrap_or(z1.max(1.0) as u32);
                let z2 = (f64::from(z1) * ratio).round() as u32;
                if z1 < zlo.min(s.teeth.unwrap_or(u32::MAX)) || z2 < zlo || z2 > 2000 {
                    continue;
                }
                let got = mt * f64::from(if first { z1 } else { z2 });
                let stay = base.map_or(0.0, |b| if (b.0 - m).abs() < 1e-12 && b.1 == z1 && b.2 == z2 { -0.01 } else { 0.0 });
                let score = 100.0 * (got - d).abs() / d
                    + 40.0 * (f64::from(z2) / f64::from(z1) / ratio).ln().abs()
                    + 0.03 * (f64::from(z1.min(z2)) - 24.0).max(0.0)
                    + if second { 0.3 } else { 0.0 }
                    + if gcd(z1, z2) == 1 { 0.0 } else { 0.02 }
                    + stay;
                if best.is_none_or(|b| score < b.0 - 1e-12) {
                    best = Some((score, m, z1, z2));
                }
            }
        }
        let (_, m, z1, z2) = best.ok_or("no standard module and teeth give that diameter at this ratio")?;
        let m = if s.exact { m * d / (m / cb * f64::from(if first { z1 } else { z2 })) } else { m };
        (c.spec.module, c.spec.teeth, c.spec.teeth2) = (m, z1, z2);
        (c.spec.shift, c.spec.shift2, c.spec.center_distance) = (0.0, 0.0, 0.0);
    } else if let Some(od) = s.outer_diameter.filter(|d| *d > 0.0) {
        // The largest pair whose bigger gear stays inside the outer diameter.
        let mut found = Vec::new();
        for tol in [0.0, 0.005, 0.02, 0.05] {
            let goal = goal_for(c, ratio, tol, 0.0, od, helix, (od / 300.0).max(0.2), od / f64::from(zlo));
            found = design::suggest(&goal);
            found.retain(|g| s.teeth.is_none_or(|z| g.teeth == z) && s.module.is_none_or(|m| (g.module - m).abs() < 1e-9));
            if !found.is_empty() {
                break;
            }
        }
        let pick = found
            .iter()
            .min_by(|a, b| {
                let sc = |g: &design::Suggestion| {
                    100.0 * (od - g.outer_diameter.max(g.outer_diameter2)) / od
                        + 40.0 * g.ratio_error.abs()
                        + 0.03 * (f64::from(g.teeth.min(g.teeth2)) - 24.0).max(0.0)
                        + if g.hunting { 0.0 } else { 0.02 }
                };
                sc(a).total_cmp(&sc(b))
            })
            .ok_or("no pair at this ratio fits inside that outer diameter")?;
        (c.spec.module, c.spec.teeth, c.spec.teeth2, c.spec.shift, c.spec.shift2, c.spec.center_distance) =
            (pick.module, pick.teeth, pick.teeth2, pick.shift, pick.shift2, 0.0);
    } else {
        if let Some(m) = s.module {
            c.spec.module = m;
        }
        let keep =
            c.same && (f64::from(c.spec.teeth2) / f64::from(c.spec.teeth) / ratio - 1.0).abs() < 1e-9 && s.teeth.is_none_or(|z| z == c.spec.teeth);
        if !keep {
            let (z1, z2) = counts(ratio, c.spec.teeth.max(zlo), zlo, s.teeth);
            (c.spec.teeth, c.spec.teeth2, c.spec.shift, c.spec.shift2) = (z1, z2, 0.0, 0.0);
        }
        c.spec.center_distance = 0.0;
    }
    // Undercut on few pinned teeth.
    if c.spec.teeth < zlo && c.spec.center_distance == 0.0 && c.spec.shift == 0.0 {
        c.spec.shift = shift_against_undercut(&c.spec, c.spec.teeth, helix);
    }
    let m = c.spec.module;
    c.face(10.0 * m);
    if let Some(f2) = s.face_width2.filter(|f| *f > 0.0) {
        c.spec.face_width2 = f2;
    }
    let (g1, g2, a) = pair_gears(&c.spec);
    c.row("module", m, Unit::None);
    c.row("gear 1 teeth", f64::from(c.spec.teeth), Unit::None);
    c.row("gear 2 teeth", f64::from(c.spec.teeth2), Unit::None);
    ratio_rows(c, ratio, f64::from(c.spec.teeth2) / f64::from(c.spec.teeth));
    c.row("gear 1 profile shift", g1.shift, Unit::None);
    c.row("gear 2 profile shift", g2.shift, Unit::None);
    c.versus("centre distance", cd, a);
    c.versus("gear 1 pitch diameter", d1, 2.0 * g1.r());
    c.versus("gear 2 pitch diameter", d2, 2.0 * g2.r());
    c.row("gear 1 outer diameter", 2.0 * g1.ra(), Unit::Mm);
    c.row("gear 2 outer diameter", 2.0 * g2.ra(), Unit::Mm);
    if helix != 0.0 {
        c.row("helix angle", helix, Unit::Deg);
    }
    c.row("face width", c.spec.face_width, Unit::Mm);
    Ok(())
}

fn ratio_rows(c: &mut Ctx, asked: f64, got: f64) {
    c.row("ratio", got, Unit::None);
    let err = got / asked - 1.0;
    c.row("ratio error %", 100.0 * err, Unit::None);
    if err.abs() > 1e-9 {
        c.note(format!("ratio {} asked, {} with whole teeth ({:+.2} %)", num(asked), num(got), 100.0 * err));
    }
}

#[allow(clippy::too_many_arguments)]
fn goal_for(c: &Ctx, ratio: f64, tol: f64, cd: f64, od: f64, helix: f64, lo: f64, hi: f64) -> Goal {
    let (lo, hi) = match c.s.module {
        Some(m) => (m, m),
        None => (if c.s.printable { lo.max(1.0) } else { lo }, hi.max(lo)),
    };
    Goal {
        ratio,
        ratio_tolerance: tol,
        max_outer_diameter: od,
        center_distance: cd,
        min_module: lo,
        max_module: hi.max(lo),
        second_choice_modules: true,
        pressure_angle: c.spec.pressure_angle,
        helix,
        rack: c.spec.rack_eff(),
        backlash: c.spec.backlash,
        face_width: c.s.face_width.unwrap_or(0.0),
        min_teeth: 0,
        max_teeth: 400,
        allow_shift: true,
        load: Default::default(),
        prefer: design::Prefer::Smallest,
        limit: 2000,
    }
}

/// A pair across a centre distance `a`: standard module and teeth, profile shifts that reach
/// `a` exactly ([`design::suggest`], then [`axes::solve`]'s pair as the fallback).
fn pair_at(c: &mut Ctx, ratio: f64, a: f64, helix: f64) -> Result<(), String> {
    let s = c.s;
    let cb = helix.to_radians().cos();
    let zlo = zmin(&c.spec, helix);
    if s.exact
        && let (Some(d1), Some(d2)) = (s.pitch_diameter, s.pitch_diameter2)
    {
        // Exact drawn circles: their teeth at the nearest standard module, then the module that
        // makes them exact (their centre distance with it, no shift).
        let z1 = s.teeth.unwrap_or_else(|| {
            fit_one(&modules(None, d1 / 150.0, d1 / 8.0, s.printable), (zlo.min(8), 400, 24), None, d1, &|m, z| m * f64::from(z) / cb, None)
                .map_or(24, |f| f.1)
        });
        let z2 = ((f64::from(z1) * d2 / d1).round() as u32).max(3);
        let m = (d1 + d2) * cb / f64::from(z1 + z2);
        (c.spec.module, c.spec.teeth, c.spec.teeth2, c.spec.shift, c.spec.shift2) = (m, z1, z2, 0.0, 0.0);
        c.spec.center_distance = if ((d1 + d2) / 2.0 - a).abs() > 1e-9 { a } else { 0.0 };
        c.note(format!("non-standard module {} draws the circles exactly", num(m)));
        return Ok(());
    }
    let base = c.same.then_some((c.base.module, c.base.teeth, c.base.teeth2));
    let hi = 2.0 * a * cb / (f64::from(zlo) * (1.0 + ratio.max(1.0 / ratio)).max(2.0));
    let lo = (2.0 * a * cb / 400.0).max(0.2);
    let mut pick: Option<design::Suggestion> = None;
    for tol in [0.0, 0.003, 0.01, 0.03, 0.08] {
        let goal = goal_for(c, ratio, tol, a, 0.0, helix, lo, hi);
        let found = design::suggest(&goal);
        let score = |g: &design::Suggestion| {
            let small = f64::from(g.teeth.min(g.teeth2));
            let stay = base.map_or(0.0, |b| if (b.0 - g.module).abs() < 1e-12 && b.1 == g.teeth && b.2 == g.teeth2 { -0.02 } else { 0.0 });
            40.0 * (g.ratio / ratio).ln().abs()
                + 0.05 * (small - 24.0).max(0.0)
                + 0.3 * (g.shift.abs() + g.shift2.abs())
                + if ISO54_SERIES_2.contains(&g.module) { 0.15 } else { 0.0 }
                + if g.hunting { 0.0 } else { 0.02 }
                + stay
        };
        pick = found.into_iter().filter(|g| s.teeth.is_none_or(|z| g.teeth == z)).min_by(|x, y| score(x).total_cmp(&score(y)));
        if pick.is_some() {
            break;
        }
    }
    if let Some(g) = pick {
        (c.spec.module, c.spec.teeth, c.spec.teeth2, c.spec.shift, c.spec.shift2) = (g.module, g.teeth, g.teeth2, g.shift, g.shift2);
        let reference = (g.module / cb) * f64::from(g.teeth + g.teeth2) / 2.0;
        c.spec.center_distance = if (a - reference).abs() < 1e-9 { 0.0 } else { a };
        if c.spec.center_distance == 0.0 {
            (c.spec.shift, c.spec.shift2) = (0.0, 0.0);
        }
        // The base's own shifts when it is this set at this distance (its split kept).
        if c.same && (c.base.module, c.base.teeth, c.base.teeth2) == (g.module, g.teeth, g.teeth2) && (pair_gears(c.base).2 - a).abs() < 1e-9 {
            (c.spec.shift, c.spec.shift2, c.spec.center_distance) = (c.base.shift, c.base.shift2, c.base.center_distance);
        }
        return Ok(());
    }
    // The axis-first solver: any module (exact if no standard one fits) and the shifts.
    let mut probe = c.spec.clone();
    if let Some(m) = s.module {
        probe.module = m;
    }
    if let Some(z) = s.teeth {
        probe.teeth = z;
    }
    probe.shift = 0.0;
    let a2 = Axis { origin: [a, 0.0, 0.0], dir: [0.0, 0.0, 1.0] };
    let drive = Drive { ratio: Some(ratio), module_free: s.module.is_none(), teeth_free: s.teeth.is_none(), teeth2_free: true, ..Drive::default() };
    let out = axes::solve(&Axis::Z, Some(&a2), Some(Kind::Pair), &probe, &drive)?;
    c.notes.extend(out.notes);
    let (placement, origin) = (c.spec.placement, c.spec.origin);
    c.spec = Spec { placement, origin, ..out.spec };
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Rings and planetary sets.

fn internal(c: &mut Ctx) -> Result<(), String> {
    let s = c.s;
    let cb = c.spec.helix.to_radians().cos();
    let rack = c.spec.rack_eff();
    let rim_k = if c.spec.rim > 0.0 { None } else { Some(3.0) };
    let rim_mm = c.spec.rim;
    let outer = move |m: f64, z: u32| m * f64::from(z) / cb + 2.0 * (rack.dedendum * m + rim_k.map_or(rim_mm, |k| k * m));
    let pitch = move |m: f64, z: u32| m * f64::from(z) / cb;
    // The pinion inside, for its size and the ring's teeth from a ratio.
    let ring_d = s.ring_diameter.filter(|d| *d > 0.0).or(match (s.pitch_diameter, s.ratio) {
        (Some(p), Some(r)) => Some(p * r),
        _ => None,
    });
    let base = c.same.then_some((c.base.module, c.base.teeth));
    let target: Option<(f64, &dyn Fn(f64, u32) -> f64)> = match (ring_d, s.outer_diameter) {
        (Some(d), _) => Some((d, &pitch)),
        (None, Some(d)) if d > 0.0 => Some((d, &outer)),
        _ => None,
    };
    if let Some((d, f)) = target {
        let guess = d * cb / 60.0;
        let (m, z) = fit_one(
            &modules(s.module, guess / 6.0, guess * 4.0, s.printable),
            (20, 1000, if c.same { c.base.teeth } else { 60 }),
            s.teeth,
            d,
            f,
            base,
        )
        .ok_or("no module and teeth give that ring diameter")?;
        c.spec.module = if s.exact { m * d / f(m, z) } else { m };
        c.spec.teeth = z;
    } else {
        if let Some(m) = s.module {
            c.spec.module = m;
        }
        if let Some(z) = s.teeth {
            c.spec.teeth = z;
        }
    }
    let m = c.spec.module;
    c.face(10.0 * m);
    let ring = Internal { gear: c.spec.involute(c.spec.teeth, c.spec.shift, c.spec.helix) };
    let rim = if c.spec.rim > 0.0 { c.spec.rim } else { 3.0 * m };
    c.row("module", m, Unit::None);
    c.row("ring teeth", f64::from(c.spec.teeth), Unit::None);
    c.versus("ring pitch diameter", ring_d, 2.0 * ring.gear.r());
    c.versus("ring outside diameter", if ring_d.is_some() { None } else { s.outer_diameter }, 2.0 * (ring.r_root() + rim));
    c.row("face width", c.spec.face_width, Unit::Mm);
    // The pinion inside: from its drawn circle, or the ratio.
    let zp = match (s.pitch_diameter, s.ratio) {
        (Some(p), _) if p > 0.0 => Some((p * cb / m).round() as u32),
        (_, Some(r)) if r > 1.0 => Some((f64::from(c.spec.teeth) / r).round() as u32),
        _ => None,
    };
    if let Some(zp) = zp.filter(|z| *z >= 3 && *z + 3 <= c.spec.teeth) {
        let pin = Spec { kind: if c.spec.helix != 0.0 { Kind::Helical } else { Kind::Spur }, teeth: zp, helix: c.spec.helix, ..c.spec.clone() };
        let a = m / cb * f64::from(c.spec.teeth - zp) / 2.0;
        c.row("pinion teeth", f64::from(zp), Unit::None);
        c.versus("pinion pitch diameter", s.pitch_diameter, m / cb * f64::from(zp));
        c.row("pinion centre offset", a, Unit::Mm);
        ratio_rows(c, s.ratio.unwrap_or(f64::from(c.spec.teeth) / f64::from(zp)), f64::from(c.spec.teeth) / f64::from(zp));
        c.note("the pinion inside the ring is not part of this set: it comes as the companion gear spec");
        c.companion = Some(Spec { origin: [c.spec.origin[0] + a, c.spec.origin[1], c.spec.origin[2]], ..pin });
    }
    Ok(())
}

fn planetary(c: &mut Ctx) -> Result<(), String> {
    let s = c.s;
    let helix = c.spec.helix;
    let cb = helix.to_radians().cos();
    let n = s.planets.unwrap_or(c.spec.planets).clamp(1, 24);
    c.spec.planets = n;
    let (ds, dp, dr) = (s.pitch_diameter.filter(|d| *d > 0.0), s.pitch_diameter2.filter(|d| *d > 0.0), s.ring_diameter.filter(|d| *d > 0.0));
    let zr0 = if c.spec.teeth3 > 0 { c.spec.teeth3 } else { c.spec.teeth + 2 * c.spec.teeth2 };
    let ratio = s
        .ratio
        .or(match (ds, dp, dr) {
            (Some(a), _, Some(r)) => Some(1.0 + r / a),
            (Some(a), Some(p), None) => Some(1.0 + (a + 2.0 * p) / a),
            (None, Some(p), Some(r)) => Some(1.0 + r / (r - 2.0 * p)),
            _ => None,
        })
        .unwrap_or(1.0 + f64::from(zr0) / f64::from(c.spec.teeth));
    if !(ratio > 2.0) {
        return Err(format!("a planetary set (ring fixed, sun in, carrier out) gives more than 2:1; {} asked", num(ratio)));
    }
    let zlo = zmin(&c.spec, helix).min(s.teeth.unwrap_or(u32::MAX));
    let ha = c.spec.rack_eff().addendum;
    // The size: the ring's pitch, its outside, the sun's or the planet's pitch.
    let rim = c.spec.rim;
    let hf = c.spec.rack_eff().dedendum;
    let size_of = |m: f64, zs: u32, zp: u32| -> Vec<(f64, f64)> {
        let mt = m / cb;
        let zr = zs + 2 * zp;
        let mut v = Vec::new();
        if let Some(d) = dr {
            v.push((mt * f64::from(zr), d));
        }
        if let Some(d) = ds {
            v.push((mt * f64::from(zs), d));
        }
        if let Some(d) = dp {
            v.push((mt * f64::from(zp), d));
        }
        if v.is_empty()
            && let Some(d) = s.outer_diameter
        {
            v.push((mt * f64::from(zr) + 2.0 * hf * m + 2.0 * if rim > 0.0 { rim } else { 3.0 * m }, d));
        }
        v
    };
    let sized = !size_of(1.0, 20, 20).is_empty();
    let big = dr.or(s.outer_diameter).or(ds.map(|d| d * (ratio - 1.0))).or(dp.map(|p| 2.0 * p * (ratio - 1.0) / (ratio - 2.0)));
    let mods = if sized {
        let guess = big.unwrap_or(100.0) * cb / 80.0;
        modules(s.module, guess / 8.0, guess * 6.0, s.printable)
    } else {
        vec![(s.module.unwrap_or(c.spec.module), false)]
    };
    let base = c.same.then_some((c.base.module, c.base.teeth, c.base.teeth2));
    let want = c.spec.teeth.max(zlo);
    let mut best: Option<(f64, f64, u32, u32)> = None;
    for (m, second) in mods {
        let zs_range: Vec<u32> = match s.teeth {
            Some(z) => vec![z],
            None => (zlo..=200).collect(),
        };
        for zs in zs_range {
            let zr_t = (ratio - 1.0) * f64::from(zs);
            for zr in [zr_t.floor() as i64 - 1, zr_t.floor() as i64, zr_t.ceil() as i64, zr_t.ceil() as i64 + 1] {
                if zr <= i64::from(zs) || (zr - i64::from(zs)) % 2 != 0 || zr > 2000 {
                    continue;
                }
                let zp = ((zr - i64::from(zs)) / 2) as u32;
                if zp < zlo {
                    continue;
                }
                let zr = zr as u32;
                let mt = m / cb;
                let a = mt * f64::from(zs + zp) / 2.0;
                let ra_p = mt * f64::from(zp) / 2.0 + ha * m;
                if n > 1 && 2.0 * a * (PI / f64::from(n)).sin() <= 2.0 * ra_p + 0.5 * m {
                    continue;
                }
                let even = (zs + zr).is_multiple_of(n);
                let got = 1.0 + f64::from(zr) / f64::from(zs);
                let mut score = 40.0 * (got / ratio).ln().abs() + if even { 0.0 } else { 3.0 } + if second { 0.3 } else { 0.0 };
                for (have, want) in size_of(m, zs, zp) {
                    score += 100.0 * (have - want).abs() / want;
                }
                if !sized {
                    score += 0.02 * (f64::from(zs) - f64::from(want)).abs();
                } else {
                    score += 0.03 * (f64::from(zs.min(zp)) - 24.0).max(0.0);
                }
                if base.is_some_and(|b| (b.0 - m).abs() < 1e-12 && b.1 == zs && b.2 == zp) {
                    score -= 0.01;
                }
                if best.is_none_or(|b| score < b.0 - 1e-12) {
                    best = Some((score, m, zs, zp));
                }
            }
        }
    }
    let (_, m, zs, zp) =
        best.ok_or("no sun, planet and ring teeth give this ratio with these planets (they would collide): use fewer planets or another ratio")?;
    let zr = zs + 2 * zp;
    let m = if s.exact
        && let Some((have, want)) = size_of(m, zs, zp).first().copied()
    {
        m * want / have
    } else {
        m
    };
    (c.spec.module, c.spec.teeth, c.spec.teeth2, c.spec.teeth3) = (m, zs, zp, zr);
    if !(c.same && c.base.teeth == zs && c.base.teeth2 == zp) {
        (c.spec.shift, c.spec.shift2) = (0.0, 0.0);
    }
    c.face(10.0 * m);
    let mt = m / cb;
    c.row("module", m, Unit::None);
    c.row("sun teeth", f64::from(zs), Unit::None);
    c.row("planet teeth", f64::from(zp), Unit::None);
    c.row("ring teeth", f64::from(zr), Unit::None);
    c.row("planets", f64::from(n), Unit::None);
    ratio_rows(c, ratio, 1.0 + f64::from(zr) / f64::from(zs));
    if (zs + zr) % n != 0 {
        c.note(format!("(sun + ring) / planets = {}/{} is not whole: the planets can't be spaced evenly", zs + zr, n));
    }
    c.versus("sun pitch diameter", ds, mt * f64::from(zs));
    c.versus("planet pitch diameter", dp, mt * f64::from(zp));
    c.versus("ring pitch diameter", dr, mt * f64::from(zr));
    c.row("planet centre radius", mt * f64::from(zs + zp) / 2.0, Unit::Mm);
    if ds.is_some() && dp.is_some() && dr.is_some() {
        let (a, b, r) = (ds.unwrap_or(0.0), dp.unwrap_or(0.0), dr.unwrap_or(0.0));
        if (a + 2.0 * b - r).abs() > 1e-6 * r {
            c.note(format!(
                "the drawn circles don't close (sun + 2 × planet = Ø{} against the ring's Ø{}): snapped to a set that assembles",
                num(a + 2.0 * b),
                num(r)
            ));
        }
    }
    c.row("face width", c.spec.face_width, Unit::Mm);
    Ok(())
}

fn rack(c: &mut Ctx) -> Result<(), String> {
    let s = c.s;
    let helix = c.spec.helix;
    let cb = helix.to_radians().cos();
    let zlo = zmin(&c.spec, helix);
    let base = c.same.then_some((c.base.module, c.base.teeth));
    if let Some(d) = s.pitch_diameter.filter(|d| *d > 0.0) {
        let guess = d * cb / 20.0;
        let pitch = move |m: f64, z: u32| m * f64::from(z) / cb;
        let (m, z) =
            fit_one(&modules(s.module, guess / 6.0, guess * 4.0, s.printable), (zlo.min(s.teeth.unwrap_or(zlo)), 400, 20), s.teeth, d, &pitch, base)
                .ok_or("no module and teeth give that pinion")?;
        c.spec.module = if s.exact { m * d / pitch(m, z) } else { m };
        c.spec.teeth = z;
    } else {
        if let Some(m) = s.module {
            c.spec.module = m;
        }
        if let Some(z) = s.teeth {
            c.spec.teeth = z;
        }
    }
    let m = c.spec.module;
    let p = PI * m / cb;
    if let Some(l) = s.length.filter(|l| *l > 0.0) {
        c.spec.rack_teeth = ((l / p).round() as u32).max(1);
    }
    c.face(10.0 * m);
    c.row("module", m, Unit::None);
    c.row("pitch", p, Unit::Mm);
    c.row("rack teeth", f64::from(c.spec.rack_teeth), Unit::None);
    c.versus("rack length", s.length, p * f64::from(c.spec.rack_teeth));
    c.row("pinion teeth", f64::from(c.spec.teeth), Unit::None);
    c.versus("pinion pitch diameter", s.pitch_diameter, m / cb * f64::from(c.spec.teeth));
    c.row("face width", c.spec.face_width, Unit::Mm);
    // The pinion, on the pitch line (y = 0) above the rack's middle.
    let r = m / cb * f64::from(c.spec.teeth) / 2.0;
    let x = p * f64::from(c.spec.rack_teeth) / 2.0;
    let shift = if c.spec.teeth < zlo { shift_against_undercut(&c.spec, c.spec.teeth, helix) } else { 0.0 };
    c.companion = Some(Spec {
        kind: if helix != 0.0 { Kind::Helical } else { Kind::Spur },
        helix: -helix,
        shift,
        origin: [c.spec.origin[0] + x, c.spec.origin[1] + r + (shift + c.spec.shift) * m, c.spec.origin[2]],
        ..c.spec.clone()
    });
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Bevels and hypoids.

/// Pitch cone angles of a bevel pair (radians), or None where the builder refuses them.
fn cones(sigma_deg: f64, z1: f64, z2: f64) -> Option<(f64, f64)> {
    let sigma = sigma_deg.to_radians();
    let d1 = sigma.sin().atan2(z2 / z1 + sigma.cos());
    let d2 = sigma - d1;
    let ok = |d: f64| (2.0..=85.0).contains(&d.to_degrees());
    (ok(d1) && ok(d2)).then_some((d1, d2))
}

fn bevel(c: &mut Ctx, kind: Kind) -> Result<(), String> {
    let s = c.s;
    if kind == Kind::SpiralBevel
        && let Some(a) = s.spiral_angle
    {
        c.spec.spiral_angle = a;
    }
    let (d1, d2) = (s.pitch_diameter.filter(|d| *d > 0.0), s.pitch_diameter2.filter(|d| *d > 0.0));
    let ratio = s
        .ratio
        .or(match (d1, d2) {
            (Some(a), Some(b)) => Some(b / a),
            _ => None,
        })
        .unwrap_or(f64::from(c.spec.teeth2) / f64::from(c.spec.teeth));
    let mut sigma = s.shaft_angle.unwrap_or(c.spec.shaft_angle);
    if !(sigma > 0.0 && sigma < 180.0) {
        return Err("the shaft angle must be between 0° and 180°".into());
    }
    if cones(sigma, 1.0, ratio).is_none() && cones(180.0 - sigma, 1.0, ratio).is_some() {
        c.note(format!("at {}° the gear's cone would open past 85° (an internal bevel): the gear goes on the other side of the apex, {}° between the shafts' rays", num(sigma), num(180.0 - sigma)));
        sigma = 180.0 - sigma;
    }
    c.spec.shaft_angle = sigma;
    let (dl1, _) =
        cones(sigma, 1.0, ratio).ok_or(format!("a {}° shaft angle at ratio {} needs a cone over 85°: these aren't built", num(sigma), num(ratio)))?;
    // The size, as the pinion's outer pitch diameter it asks for.
    let target = d1.or(d2.map(|d| d / ratio)).or(s.cone_distance.filter(|r| *r > 0.0).map(|re| 2.0 * re * dl1.sin()));
    let zl = (14.0 * dl1.cos()).ceil().max(6.0) as u32;
    let base = c.same.then_some((c.base.module, c.base.teeth, c.base.teeth2));
    if let Some(d) = target {
        let guess = d / 20.0;
        let mut best: Option<(f64, f64, u32, u32)> = None;
        for (m, second) in modules(s.module, guess / 5.0, guess * 4.0, s.printable) {
            let zc = d / m;
            let firsts: Vec<u32> = match s.teeth {
                Some(z) => vec![z],
                None => [zc.floor(), zc.ceil()].into_iter().map(|z| z.max(1.0) as u32).collect(),
            };
            for z1 in firsts {
                let z2 = (f64::from(z1) * ratio).round() as u32;
                if z1 < zl.min(s.teeth.unwrap_or(zl)) || !(6..=2000).contains(&z2) || cones(sigma, f64::from(z1), f64::from(z2)).is_none() {
                    continue;
                }
                let got = m * f64::from(z1);
                let stay = base.is_some_and(|b| (b.0 - m).abs() < 1e-12 && b.1 == z1 && b.2 == z2);
                let score = 100.0 * (got - d).abs() / d
                    + 40.0 * (f64::from(z2) / f64::from(z1) / ratio).ln().abs()
                    + 0.03 * (f64::from(z1) - 24.0).max(0.0)
                    + 0.1 * (16.0 - f64::from(z1)).max(0.0)
                    + if second { 0.3 } else { 0.0 }
                    + if gcd(z1, z2) == 1 { 0.0 } else { 0.02 }
                    - if stay { 0.01 } else { 0.0 };
                if best.is_none_or(|b| score < b.0 - 1e-12) {
                    best = Some((score, m, z1, z2));
                }
            }
        }
        let (_, m, z1, z2) = best.ok_or("no standard module and teeth give that size at this ratio")?;
        c.spec.module = if s.exact { m * d / (m * f64::from(z1)) } else { m };
        (c.spec.teeth, c.spec.teeth2) = (z1, z2);
    } else {
        if let Some(m) = s.module {
            c.spec.module = m;
        }
        let keep =
            c.same && (f64::from(c.spec.teeth2) / f64::from(c.spec.teeth) / ratio - 1.0).abs() < 1e-9 && s.teeth.is_none_or(|z| z == c.spec.teeth);
        if !keep {
            let (z1, z2) = counts(ratio, c.spec.teeth.max(zl), zl, s.teeth);
            (c.spec.teeth, c.spec.teeth2) = (z1, z2);
        }
    }
    if !(c.same && c.base.teeth == c.spec.teeth && c.base.teeth2 == c.spec.teeth2) {
        c.spec.shift = 0.0;
    }
    let (z1, z2) = (f64::from(c.spec.teeth), f64::from(c.spec.teeth2));
    let (dl1, dl2) = cones(sigma, z1, z2).ok_or("the tooth counts make a cone over 85°")?;
    let m = c.spec.module;
    let re = m * z1 / 2.0 / dl1.sin();
    c.face(0.3 * re);
    c.row("module", m, Unit::None);
    c.row("pinion teeth", z1, Unit::None);
    c.row("gear teeth", z2, Unit::None);
    ratio_rows(c, ratio, z2 / z1);
    c.row("shaft angle", sigma, Unit::Deg);
    c.row("pinion pitch cone angle", dl1.to_degrees(), Unit::Deg);
    c.row("gear pitch cone angle", dl2.to_degrees(), Unit::Deg);
    c.versus("pinion outer pitch diameter", d1, m * z1);
    c.versus("gear outer pitch diameter", d2, m * z2);
    c.versus("cone distance", s.cone_distance, re);
    c.row("face width", c.spec.face_width, Unit::Mm);
    Ok(())
}

/// A bevel sun with planets round it: the cone angle (sun shaft to planet shaft), the ratio
/// (planet teeth over the sun's) and one size, as a bevel pair's. With a second sun or a ring
/// the cone angle is set by whole teeth (far teeth = sun + 2 × planet × cos Σ), so the counts
/// are chosen to keep it near the one asked, and to space the planets evenly and apart.
fn bevel_planetary(c: &mut Ctx) -> Result<(), String> {
    let s = c.s;
    let mut mode = s.bevel_planet_mode.unwrap_or(c.spec.bevel_planet_mode);
    c.spec.bevel_planet_mode = mode;
    let n = s.planets.unwrap_or(c.spec.planets).clamp(1, 24);
    c.spec.planets = n;
    if let Some(a) = s.spiral_angle {
        c.spec.spiral_angle = a;
    }
    let mut sigma = s.shaft_angle.unwrap_or(c.spec.shaft_angle);
    // A ring with no cone angle asked: where the base's angle can't take one (a differential's
    // 90° would need a ring flatter than 85°), a bevel epicyclic's usual 45°.
    if mode == BevelPlanetMode::Ring
        && s.shaft_angle.is_none()
        && bevel_planet::geometry(&Spec { shaft_angle: sigma, teeth3: 0, ..c.spec.clone() }).is_err()
    {
        c.note(format!("a ring can't be made at the {}° cone angle: 45° taken (set the cone angle to choose another)", num(sigma)));
        sigma = 45.0;
    }
    if !(5.0..=175.0).contains(&sigma) {
        return Err(format!("the cone angle (sun shaft to planet shaft) must be 5° to 175°; {} asked", num(sigma)));
    }
    // A differential (the default) whose second sun can't stand at this cone angle, with no
    // mode asked: the sun and planets alone.
    if mode == BevelPlanetMode::Differential
        && s.bevel_planet_mode.is_none()
        && bevel_planet::geometry(&Spec { shaft_angle: sigma, teeth3: 0, ..c.spec.clone() }).is_err()
    {
        c.note(format!("a second sun can't be made at the {}° cone angle: built as sun and planets (single); set the mode to choose", num(sigma)));
        mode = BevelPlanetMode::Single;
        c.spec.bevel_planet_mode = mode;
    }
    c.spec.shaft_angle = sigma;
    let (d1, d2) = (s.pitch_diameter.filter(|d| *d > 0.0), s.pitch_diameter2.filter(|d| *d > 0.0));
    let ratio = s
        .ratio
        .or(match (d1, d2) {
            (Some(a), Some(b)) => Some(b / a),
            _ => None,
        })
        .unwrap_or(f64::from(c.spec.teeth2) / f64::from(c.spec.teeth.max(1)));
    if !(ratio.is_finite() && ratio > 0.0) {
        return Err("the ratio (planet ÷ sun teeth) must be greater than 0".into());
    }
    let (dl1, _) = cones(sigma, 1.0, ratio).ok_or(format!(
        "a {}° cone angle at ratio {} needs a pitch cone over 85°: these aren't built",
        num(sigma),
        num(ratio)
    ))?;
    let target = d1.or(d2.map(|d| d / ratio)).or(s.cone_distance.filter(|r| *r > 0.0).map(|re| 2.0 * re * dl1.sin()));
    let zl = (14.0 * dl1.cos()).ceil().max(6.0) as u32;
    let base = c.same.then_some((c.base.module, c.base.teeth, c.base.teeth2));
    let want = c.spec.teeth.max(zl);
    let mods = match target {
        Some(d) => modules(s.module, d / 100.0, d / 5.0, s.printable),
        None => vec![(s.module.unwrap_or(c.spec.module), false)],
    };
    let mut best: Option<(f64, f64, u32, u32)> = None;
    for (m, second) in mods {
        let firsts: Vec<u32> = match (s.teeth, target) {
            (Some(z), _) => vec![z],
            (None, Some(d)) => [(d / m).floor(), (d / m).ceil()].into_iter().map(|z| z.max(1.0) as u32).collect(),
            (None, None) => (zl..=want + 40).collect(),
        };
        for z1 in firsts {
            let z2 = (f64::from(z1) * ratio).round() as u32;
            if z1 < zl.min(s.teeth.unwrap_or(zl)) || !(6..=2000).contains(&z2) {
                continue;
            }
            let cand = Spec { module: m, teeth: z1, teeth2: z2, teeth3: 0, ..c.spec.clone() };
            let Ok(g) = bevel_planet::geometry(&cand) else { continue };
            let mut score = 40.0 * (f64::from(z2) / f64::from(z1) / ratio).ln().abs()
                + 2.0 * (g.sigma.to_degrees() - sigma).abs()
                + if second { 0.3 } else { 0.0 };
            if mode != BevelPlanetMode::Single && (z1 + g.z_far) % n != 0 {
                score += 3.0;
            }
            if g.warnings.iter().any(|w| w.contains("collide")) {
                score += 20.0;
            }
            score += match target {
                Some(d) => 100.0 * (m * f64::from(z1) - d).abs() / d + 0.03 * (f64::from(z1) - 24.0).max(0.0) + 0.1 * (16.0 - f64::from(z1)).max(0.0),
                None => 0.02 * (f64::from(z1) - f64::from(want)).abs(),
            };
            if base.is_some_and(|b| (b.0 - m).abs() < 1e-12 && b.1 == z1 && b.2 == z2) {
                score -= 0.01;
            }
            if best.is_none_or(|b| score < b.0 - 1e-12) {
                best = Some((score, m, z1, z2));
            }
        }
    }
    let (_, m, z1, z2) = best.ok_or("no module and teeth make this set: a pitch cone would open past 85°, or the second sun or ring can't be made at this cone angle; change the cone angle or the ratio")?;
    c.spec.module = match target {
        Some(d) if s.exact => d / f64::from(z1),
        _ => m,
    };
    (c.spec.teeth, c.spec.teeth2, c.spec.teeth3) = (z1, z2, 0);
    let g = bevel_planet::geometry(&c.spec)?;
    // Built at the angle the whole teeth give (the notes say how far it moved).
    c.spec.teeth3 = g.z_far;
    c.spec.shaft_angle = g.sigma.to_degrees();
    (c.spec.shift, c.spec.shift2) = (0.0, 0.0);
    let m = c.spec.module;
    c.face(0.3 * g.re);
    let (fs, fp, ff) = (f64::from(z1), f64::from(z2), f64::from(g.z_far));
    let far = if mode == BevelPlanetMode::Ring { "ring" } else { "sun 2" };
    c.row("module", m, Unit::None);
    c.row("sun teeth", fs, Unit::None);
    c.row("planet teeth", fp, Unit::None);
    if mode != BevelPlanetMode::Single {
        c.row(&format!("{far} teeth"), ff, Unit::None);
    }
    c.row("planets", f64::from(n), Unit::None);
    ratio_rows(c, ratio, fp / fs);
    c.row("cone angle", g.sigma.to_degrees(), Unit::Deg);
    if (g.sigma.to_degrees() - sigma).abs() > 0.005 {
        c.row("cone angle asked", sigma, Unit::Deg);
    }
    c.row("sun pitch cone angle", g.delta_sun.to_degrees(), Unit::Deg);
    c.row("planet pitch cone angle", g.delta_planet.to_degrees(), Unit::Deg);
    c.versus("sun outer pitch diameter", d1, m * fs);
    c.versus("planet outer pitch diameter", d2, m * fp);
    c.versus("cone distance", s.cone_distance, g.re);
    match mode {
        BevelPlanetMode::Single => {}
        BevelPlanetMode::Differential => {
            c.row("sun 2 outer pitch diameter", m * ff, Unit::Mm);
            c.row("ratio (sun 2 fixed, sun in, carrier out)", 1.0 + ff / fs, Unit::None);
        }
        BevelPlanetMode::Ring => {
            c.row("ring outer pitch diameter", m * ff, Unit::Mm);
            c.row("ratio (ring fixed, sun in, carrier out)", 1.0 + ff / fs, Unit::None);
        }
    }
    c.row("face width", c.spec.face_width, Unit::Mm);
    for w in g.warnings {
        c.note(w);
    }
    Ok(())
}

fn hypoid(c: &mut Ctx) -> Result<(), String> {
    let s = c.s;
    if let Some(a) = s.spiral_angle {
        c.spec.spiral_angle = a.abs();
    }
    if let Some(l) = s.left_hand {
        c.spec.left_hand = l;
    }
    let ratio = s
        .ratio
        .or(match (s.pitch_diameter, s.pitch_diameter2) {
            (Some(a), Some(b)) if a > 0.0 => Some(b / a),
            _ => None,
        })
        .unwrap_or(f64::from(c.spec.teeth2) / f64::from(c.spec.teeth));
    if !(ratio >= 1.0) {
        return Err("a hypoid's pinion drives the ring gear: the ratio must be 1 or more".into());
    }
    let sigma = s.shaft_angle.unwrap_or(c.spec.shaft_angle);
    let sigma = if sigma > 90.0 && cones(sigma, 1.0, ratio).is_none() { 180.0 - sigma } else { sigma };
    c.spec.shaft_angle = sigma;
    c.spec.offset = s.offset.unwrap_or(c.spec.offset).abs();
    // Teeth: kept when they give the ratio; otherwise a hunting pair, the pinion about 42 / ratio
    // (about 56 teeth in all at common ratios: 14:42 at 3, 11:41 at 3.7).
    let keep = c.same && (f64::from(c.spec.teeth2) / f64::from(c.spec.teeth) / ratio - 1.0).abs() < 1e-9 && s.teeth.is_none_or(|z| z == c.spec.teeth);
    if !keep {
        let want = (42.0 / ratio).round().clamp(6.0, 30.0) as u32;
        let (z1, z2) = counts(ratio, want, 5, s.teeth.or(Some(want)).filter(|_| s.teeth.is_some()));
        (c.spec.teeth, c.spec.teeth2) = (z1, z2);
    }
    let (z1, z2) = (f64::from(c.spec.teeth), f64::from(c.spec.teeth2));
    // The ring gear's size: its outer pitch diameter (any module: a hypoid's is rarely standard).
    let had = if c.same {
        c.base.module * f64::from(c.base.teeth2)
    } else {
        let d = Spec::defaults_for(Kind::Hypoid);
        d.module * f64::from(d.teeth2)
    };
    let ring = s.pitch_diameter2.filter(|d| *d > 0.0).or(s.pitch_diameter.filter(|d| *d > 0.0).map(|d| d * z2 / z1)).unwrap_or(had);
    c.spec.module = ring / z2;
    // The ring gear's carrier hole goes with its size (the default's is 40 % of it).
    if s.bore2.is_none() && c.spec.bore2.kind != BoreKind::None && (c.spec.bore2.diameter - 0.4 * had).abs() < 1e-9 && (ring - had).abs() > 1e-9 {
        c.spec.bore2.diameter = 0.4 * ring;
    }
    let dl2 = (ratio * sigma.to_radians().sin()).atan2(1.0 + ratio * sigma.to_radians().cos());
    let cone = ring / 2.0 / dl2.sin();
    c.face(0.3 * cone);
    // An offset this large for the ring gear unwinds its spiral (under
    // `hypoid::LEAST_RING_SPIRAL`): unless the spiral angle was given, a steeper pinion spiral
    // (up to 60°) keeps it, as the refusal suggests.
    if s.spiral_angle.is_none() && c.spec.offset > 0.0 && !crate::hypoid::ring_spirals(&c.spec) {
        let was = c.spec.spiral_angle;
        if let Some(a) = (1..=12)
            .map(|k| was + 2.5 * f64::from(k))
            .take_while(|a| *a <= 60.0)
            .find(|a| crate::hypoid::ring_spirals(&Spec { spiral_angle: *a, ..c.spec.clone() }))
        {
            c.spec.spiral_angle = a;
            c.note(format!("pinion spiral angle raised to {}° so the ring gear's teeth still spiral at this offset", num(a)));
        }
    }
    c.row("pinion teeth", z1, Unit::None);
    c.row("ring gear teeth", z2, Unit::None);
    ratio_rows(c, ratio, z2 / z1);
    c.versus("ring gear outer pitch diameter", s.pitch_diameter2, ring);
    c.row("module (outer, transverse)", c.spec.module, Unit::None);
    c.row("offset", c.spec.offset, Unit::Mm);
    c.row("offset / ring gear diameter %", 100.0 * c.spec.offset / ring, Unit::None);
    c.row("shaft angle", sigma, Unit::Deg);
    c.row("pinion spiral angle", c.spec.spiral_angle, Unit::Deg);
    c.row("face width", c.spec.face_width, Unit::Mm);
    // Its pitch cones must exist for this offset (the design is cached for the schematic).
    hypoid_parts(&c.spec)?;
    if c.spec.offset > HYPOID_OFFSET_FULL * ring {
        c.note(format!(
            "an offset over 25 % of the ring gear's diameter (Ø{}: {} mm) is beyond what ISO 23509 designs use",
            num(ring),
            num(HYPOID_OFFSET_FULL * ring)
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Skew sets.

fn crossed(c: &mut Ctx) -> Result<(), String> {
    let s = c.s;
    let mut sigma = s.shaft_angle.unwrap_or(c.spec.shaft_angle);
    if sigma > 120.0 {
        c.note(format!("{}° between the shafts' directions: gear 2 turns the other way round, {}°", num(sigma), num(180.0 - sigma)));
        sigma = 180.0 - sigma;
    }
    c.spec.shaft_angle = sigma;
    let b1 = s.helix_split.unwrap_or(if s.shaft_angle.is_some() || !c.same { sigma / 2.0 } else { c.spec.helix });
    c.spec.helix = b1;
    let b2 = sigma - b1;
    let (cb1, cb2) = (b1.to_radians().cos(), b2.to_radians().cos());
    let ratio = s
        .ratio
        .or(match (s.pitch_diameter, s.pitch_diameter2) {
            (Some(a), Some(b)) if a > 0.0 => Some(b * cb2 / (a * cb1)),
            _ => None,
        })
        .unwrap_or(f64::from(c.spec.teeth2) / f64::from(c.spec.teeth));
    let e = s.center_distance.or(s.offset).filter(|e| *e > 0.0).or(match (s.pitch_diameter, s.pitch_diameter2) {
        (Some(a), Some(b)) => Some((a + b) / 2.0),
        _ => None,
    });
    let zl = zmin(&c.spec, b1.abs().max(b2.abs())).max(8);
    let span = |m: f64, z1: u32, z2: u32| m / 2.0 * (f64::from(z1) / cb1 + f64::from(z2) / cb2);
    let base = c.same.then_some((c.base.module, c.base.teeth, c.base.teeth2));
    let mut best: Option<(f64, f64, u32, u32)> = None;
    let target = e.or(s.pitch_diameter.map(|d| d / 2.0 * (1.0 + ratio * cb1 / cb2)));
    if let Some(a) = target {
        // Standard module and teeth whose pitch cylinders reach a little short of the offset;
        // profile shift takes up the rest.
        let guess = 2.0 * a / 40.0;
        for (m, second) in modules(s.module, guess / 6.0, guess * 4.0, s.printable) {
            let per = span(m, 1, 0) + span(m, 0, 1) * ratio;
            let zc = a / per;
            let firsts: Vec<u32> = match s.teeth {
                Some(z) => vec![z],
                None => [zc.floor() - 1.0, zc.floor(), zc.ceil()].into_iter().filter(|z| *z >= 1.0).map(|z| z as u32).collect(),
            };
            for z1 in firsts {
                let z2 = ((f64::from(z1) * ratio).round() as u32).max(1);
                if z1.min(z2) < zl.min(s.teeth.unwrap_or(zl)) {
                    continue;
                }
                let gap = (a - span(m, z1, z2)) / m;
                if e.is_some() && !(-0.3..=0.8).contains(&gap) {
                    continue;
                }
                let stay = base.is_some_and(|b| (b.0 - m).abs() < 1e-12 && b.1 == z1 && b.2 == z2);
                let score = 40.0 * (f64::from(z2) / f64::from(z1) / ratio).ln().abs()
                    + 0.5 * gap.abs()
                    + 0.03 * (f64::from(z1.min(z2)) - 24.0).max(0.0)
                    + if second { 0.15 } else { 0.0 }
                    - if stay { 0.01 } else { 0.0 };
                if best.is_none_or(|b| score < b.0 - 1e-12) {
                    best = Some((score, m, z1, z2));
                }
            }
        }
        let (_, m, z1, z2) = best.ok_or("no standard module and teeth make crossed helical gears across that offset at this ratio")?;
        (c.spec.module, c.spec.teeth, c.spec.teeth2) = (m, z1, z2);
        let sum = if e.is_some() { (a - span(m, z1, z2)) / m } else { 0.0 };
        let sum = if sum.abs() < 1e-9 { 0.0 } else { sum };
        (c.spec.shift, c.spec.shift2) = (sum / 2.0, sum / 2.0);
        if s.exact && e.is_some() && sum != 0.0 {
            c.spec.module = a / span(1.0, z1, z2);
            (c.spec.shift, c.spec.shift2) = (0.0, 0.0);
        }
    } else {
        if let Some(m) = s.module {
            c.spec.module = m;
        }
        let keep =
            c.same && (f64::from(c.spec.teeth2) / f64::from(c.spec.teeth) / ratio - 1.0).abs() < 1e-9 && s.teeth.is_none_or(|z| z == c.spec.teeth);
        if !keep {
            let (z1, z2) = counts(ratio, c.spec.teeth.max(zl), zl, s.teeth);
            (c.spec.teeth, c.spec.teeth2, c.spec.shift, c.spec.shift2) = (z1, z2, 0.0, 0.0);
        }
    }
    let m = c.spec.module;
    c.face(10.0 * m);
    let got = span(m, c.spec.teeth, c.spec.teeth2) + (c.spec.shift + c.spec.shift2) * m;
    c.row("normal module", m, Unit::None);
    c.row("gear 1 teeth", f64::from(c.spec.teeth), Unit::None);
    c.row("gear 2 teeth", f64::from(c.spec.teeth2), Unit::None);
    ratio_rows(c, ratio, f64::from(c.spec.teeth2) / f64::from(c.spec.teeth));
    c.row("gear 1 helix angle", b1, Unit::Deg);
    c.row("gear 2 helix angle", b2, Unit::Deg);
    c.row("profile shift (each)", c.spec.shift, Unit::None);
    c.versus("centre distance", e, got);
    c.row("gear 1 pitch diameter", m * f64::from(c.spec.teeth) / cb1, Unit::Mm);
    c.row("gear 2 pitch diameter", m * f64::from(c.spec.teeth2) / cb2, Unit::Mm);
    c.row("face width", c.spec.face_width, Unit::Mm);
    Ok(())
}

fn worm(c: &mut Ctx) -> Result<(), String> {
    let s = c.s;
    if let Some(l) = s.left_hand {
        c.spec.helix = if l { -c.spec.helix.abs().max(1.0) } else { 0.0 };
    }
    c.spec.shaft_angle = 90.0;
    let ratio = s.ratio.unwrap_or(f64::from(c.spec.teeth) / f64::from(c.spec.starts.max(1)));
    if !(ratio >= 1.0) {
        return Err("a worm drives its wheel down: the ratio must be 1 or more".into());
    }
    let keep = c.same
        && (f64::from(c.spec.teeth) / f64::from(c.spec.starts.max(1)) / ratio - 1.0).abs() < 1e-9
        && s.starts.is_none_or(|z| z == c.spec.starts);
    if !keep {
        c.spec.starts = s.starts.unwrap_or_else(|| (1..=6u32).find(|k| ratio * f64::from(*k) >= 19.5).unwrap_or(6)).clamp(1, 12);
        c.spec.teeth = ((ratio * f64::from(c.spec.starts)).round() as u32).max(3);
    }
    let zw = f64::from(c.spec.teeth);
    let (dw, d2) = (s.pitch_diameter.filter(|d| *d > 0.0), s.pitch_diameter2.filter(|d| *d > 0.0));
    let a = s.center_distance.or(s.offset).filter(|a| *a > 0.0).or(match (dw, d2) {
        (Some(x), Some(y)) => Some((x + y) / 2.0),
        _ => None,
    });
    // The module: from the wheel's circle, or the one the axis-first solver picks across `a`
    // (the worm about 10 modules), or kept.
    if let Some(d) = d2 {
        let m = if s.exact {
            d / zw
        } else {
            modules(s.module, d / zw / 1.3, d / zw * 1.3, s.printable)
                .into_iter()
                .min_by(|x, y| (x.0 * zw / d).ln().abs().total_cmp(&(y.0 * zw / d).ln().abs()))
                .map_or(d / zw, |x| x.0)
        };
        c.spec.module = m;
    } else if let Some(m) = s.module {
        c.spec.module = m;
    } else if let Some(a) = a {
        let a2 = Axis { origin: [a, 0.0, 0.0], dir: [0.0, 1.0, 0.0] };
        let probe = Spec { shift: 0.0, ..c.spec.clone() };
        let out = axes::solve(&Axis::Z, Some(&a2), Some(Kind::Worm), &probe, &Drive { module_free: true, ..Drive::default() })?;
        c.spec.module = out.spec.module;
    } else if let Some(d) = dw {
        c.spec.module = modules(None, d / 14.0, d / 8.0, s.printable)
            .into_iter()
            .min_by(|x, y| (d / x.0 / 10.0).ln().abs().total_cmp(&(d / y.0 / 10.0).ln().abs()))
            .map_or(d / 10.0, |x| x.0);
    }
    let m = c.spec.module;
    // The worm's diameter: what is left of the centre distance after the wheel, or drawn.
    c.spec.shift = if keep { c.spec.shift } else { 0.0 };
    c.spec.worm_diameter = match (a, dw) {
        (Some(a), _) => 2.0 * (a - c.spec.shift * m) - m * zw,
        (None, Some(d)) => d,
        _ if keep => c.spec.worm_diameter,
        _ => 0.0,
    };
    let d1 = if c.spec.worm_diameter > 0.0 { c.spec.worm_diameter } else { 10.0 * m };
    if d1 <= 2.0 * c.spec.rack.dedendum * m + 0.5 * m {
        return Err(format!(
            "the wheel (Ø{}) leaves no room for a worm across {} mm: use a smaller wheel or a larger distance",
            num(m * zw),
            num(a.unwrap_or(0.0))
        ));
    }
    c.face(10.0 * m);
    c.row("axial module", m, Unit::None);
    c.row("starts", f64::from(c.spec.starts), Unit::None);
    c.row("wheel teeth", zw, Unit::None);
    ratio_rows(c, ratio, zw / f64::from(c.spec.starts));
    c.versus("wheel pitch diameter", d2, m * zw);
    c.versus("worm pitch diameter", if a.is_some() { None } else { dw }, d1);
    c.versus("centre distance", a, (d1 + m * zw) / 2.0 + c.spec.shift * m);
    c.row("lead angle", (f64::from(c.spec.starts) * m / d1).atan().to_degrees(), Unit::Deg);
    c.row("face width", c.spec.face_width, Unit::Mm);
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The rest.

fn cycloidal(c: &mut Ctx) -> Result<(), String> {
    let s = c.s;
    if let Some(z) = s.teeth {
        c.spec.teeth = z;
    }
    let single_only = c.spec.teeth2 == 0 && s.ratio.is_none();
    if let Some(r) = s.ratio.filter(|r| *r > 0.0) {
        let keep = c.spec.teeth2 > 0 && (f64::from(c.spec.teeth) / f64::from(c.spec.teeth2) / r - 1.0).abs() < 1e-9;
        if !keep {
            c.spec.teeth2 = ((f64::from(c.spec.teeth) / r).round() as u32).max(6);
        }
    }
    if let Some(a) = s.center_distance.or(s.offset).filter(|a| *a > 0.0).filter(|_| !single_only) {
        c.spec.module = 2.0 * a / f64::from(c.spec.teeth + c.spec.teeth2);
        c.note(format!("cycloidal teeth take no shift: module {} gives the distance exactly", num(c.spec.module)));
    } else if let Some(d) = s.pitch_diameter.filter(|d| *d > 0.0) {
        c.spec.module = d / f64::from(c.spec.teeth);
    } else if let Some(m) = s.module {
        c.spec.module = m;
    }
    let m = c.spec.module;
    c.face(3.0 * m);
    c.row("module", m, Unit::None);
    c.row("wheel teeth", f64::from(c.spec.teeth), Unit::None);
    c.versus("wheel pitch diameter", s.pitch_diameter, m * f64::from(c.spec.teeth));
    if c.spec.teeth2 > 0 {
        c.row("pinion teeth", f64::from(c.spec.teeth2), Unit::None);
        ratio_rows(c, s.ratio.unwrap_or(f64::from(c.spec.teeth) / f64::from(c.spec.teeth2)), f64::from(c.spec.teeth) / f64::from(c.spec.teeth2));
        c.row("centre distance", m * f64::from(c.spec.teeth + c.spec.teeth2) / 2.0, Unit::Mm);
    }
    c.row("face width", c.spec.face_width, Unit::Mm);
    Ok(())
}

fn drive(c: &mut Ctx) -> Result<(), String> {
    use crate::cycloid::CycloidalDrive;
    let s = c.s;
    let pins = s.ratio.map_or(c.spec.pins, |r| (r.round() as u32 + 1).clamp(3, 200));
    let pc = s.pitch_diameter.filter(|d| *d > 0.0).or(s.outer_diameter.filter(|d| *d > 0.0).map(|d| d / (1.0 + 0.382 * PI / f64::from(pins))));
    let changed = pins != c.spec.pins || pc.is_some_and(|p| (p - c.spec.pin_circle).abs() > 1e-9) || !c.same;
    c.spec.pins = pins;
    if let Some(p) = pc {
        c.spec.pin_circle = p;
    }
    if changed {
        let pitch = PI * c.spec.pin_circle / f64::from(pins);
        c.spec.pin_diameter = round_to(0.382 * pitch, 0.1).max(0.5);
        let mut e = round_to(0.25 * c.spec.pin_diameter, 0.05).max(0.05);
        for _ in 0..40 {
            let d = CycloidalDrive {
                pins,
                pin_circle_radius: c.spec.pin_circle / 2.0,
                pin_radius: c.spec.pin_diameter / 2.0,
                eccentricity: e,
                clearance: c.spec.backlash / 2.0,
            };
            if e < 0.9 * d.max_eccentricity() && !d.undercut() && d.valley_radius() > 0.05 * c.spec.pin_circle {
                break;
            }
            e *= 0.85;
        }
        c.spec.eccentricity = e;
        c.face(0.13 * c.spec.pin_circle);
    } else {
        c.face(0.13 * c.spec.pin_circle);
    }
    c.row("pins", f64::from(pins), Unit::None);
    ratio_rows(c, s.ratio.unwrap_or(f64::from(pins - 1)), f64::from(pins - 1));
    c.versus("pin circle diameter", s.pitch_diameter, c.spec.pin_circle);
    c.row("pin diameter", c.spec.pin_diameter, Unit::Mm);
    c.versus(
        "outer diameter (over the pins)",
        if s.pitch_diameter.is_some() { None } else { s.outer_diameter },
        c.spec.pin_circle + c.spec.pin_diameter,
    );
    c.row("eccentricity", c.spec.eccentricity, Unit::Mm);
    c.row("face width", c.spec.face_width, Unit::Mm);
    Ok(())
}

fn wrapped(c: &mut Ctx, kind: Kind) -> Result<(), String> {
    let s = c.s;
    let chain = kind == Kind::Sprocket;
    if chain {
        if let Some(id) = s.chain {
            c.spec.chain = id;
        }
    } else if let Some(id) = s.belt {
        c.spec.belt = id;
    }
    let (pitch, extra) = if chain {
        let ch = crate::sprocket::chain_by_id(c.spec.chain).map_or(c.spec.chain_pitch, |ch| ch.pitch);
        (ch, 0.0)
    } else {
        let b = crate::pulley::belt_by_id(c.spec.belt).copied().ok_or(format!("unknown belt `{}`", c.spec.belt))?;
        (b.pitch, b.pld)
    };
    let _ = extra;
    let (lo, hi) = if chain { (6, 250) } else { (6, 400) };
    let from_d = |d: f64| -> u32 {
        let z = if chain { PI / (pitch / d).clamp(-1.0, 1.0).asin() } else { PI * d / pitch };
        (z.round() as u32).clamp(lo, hi)
    };
    let dia = |z: u32| if chain { pitch / (PI / f64::from(z)).sin() } else { f64::from(z) * pitch / PI };
    if let Some(z) = s.teeth {
        c.spec.teeth = z.clamp(lo, hi);
    } else if let Some(d) = s.pitch_diameter.filter(|d| *d > 0.0) {
        c.spec.teeth = from_d(d);
    }
    if let Some(d2) = s.pitch_diameter2.filter(|d| *d > 0.0) {
        c.spec.teeth2 = from_d(d2);
    } else if let Some(r) = s.ratio.filter(|r| *r > 0.0) {
        c.spec.teeth2 = ((f64::from(c.spec.teeth) * r).round() as u32).clamp(lo, hi);
    }
    if let Some(a) = s.center_distance.or(s.offset).filter(|a| *a > 0.0) {
        c.spec.center_distance = a;
        if c.spec.teeth2 == 0 {
            c.spec.teeth2 = c.spec.teeth;
        }
    }
    if !chain {
        c.face(c.spec.face_width);
    }
    c.row("teeth", f64::from(c.spec.teeth), Unit::None);
    c.versus("pitch diameter", s.pitch_diameter, dia(c.spec.teeth));
    if c.spec.teeth2 > 0 {
        c.row("teeth (driven)", f64::from(c.spec.teeth2), Unit::None);
        ratio_rows(c, s.ratio.unwrap_or(f64::from(c.spec.teeth2) / f64::from(c.spec.teeth)), f64::from(c.spec.teeth2) / f64::from(c.spec.teeth));
        c.versus("pitch diameter (driven)", s.pitch_diameter2, dia(c.spec.teeth2));
        if let Some(a) = wrap_center(&c.spec) {
            c.row("centre distance (whole links / belt teeth)", a, Unit::Mm);
            if (a - c.spec.center_distance).abs() > 1e-6 && c.spec.center_distance > 0.0 {
                c.note(format!(
                    "centre distance {} asked: the {} takes the nearest whole number of {} and sits at {}",
                    num(c.spec.center_distance),
                    if chain { "chain" } else { "belt" },
                    if chain { "links" } else { "teeth" },
                    num(a)
                ));
            }
        }
    }
    if !chain {
        c.row("belt width", c.spec.face_width, Unit::Mm);
    }
    Ok(())
}

/// Where a sprocket or pulley pair's second member goes (the plan's taut chain or belt).
fn wrap_center(s: &Spec) -> Option<f64> {
    if s.teeth2 == 0 {
        return None;
    }
    let (z1, z2) = (f64::from(s.teeth), f64::from(s.teeth2));
    if s.kind == Kind::Sprocket {
        let (_, pitch, ..) = crate::plan::extras::chain_of(s);
        let a_in = if s.center_distance > 0.0 { s.center_distance } else { 40.0 * pitch };
        let mut links = crate::wrap::chain_links(z1, z2, a_in, pitch).ceil();
        if links % 2.0 != 0.0 {
            links += 1.0;
        }
        Some(crate::wrap::chain_center(z1, z2, links, pitch).unwrap_or(a_in))
    } else {
        let belt = crate::plan::extras::belt_of(s);
        let r1 = crate::pulley::Pulley { belt, teeth: s.teeth }.pitch_diameter() / 2.0;
        let r2 = crate::pulley::Pulley { belt, teeth: s.teeth2 }.pitch_diameter() / 2.0;
        let a_in = if s.center_distance > 0.0 { s.center_distance } else { 1.5 * (r1 + r2) * 2.0 };
        let teeth_belt = (crate::wrap::belt_length(r1, r2, a_in) / belt.pitch).round().max(1.0);
        Some(crate::plan::extras::belt_center(r1, r2, teeth_belt * belt.pitch).unwrap_or(a_in))
    }
}

/// DIN 5480's modules.
const SPLINE_MODULES: [f64; 16] = [0.5, 0.6, 0.75, 0.8, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0];

fn spline(c: &mut Ctx) -> Result<(), String> {
    let s = c.s;
    if let Some(m) = s.module {
        c.spec.module = m;
    }
    if let Some(d) = s.pitch_diameter.filter(|d| *d > 0.0) {
        let had = if c.base.spline_diameter > 0.0 { c.base.spline_diameter } else { c.base.module * (f64::from(c.base.teeth) + 1.1) };
        let keep = c.same && (had - d).abs() < 1e-9 && s.module.is_none() && s.teeth.is_none();
        if !keep {
            c.spec.spline_diameter = d;
            // Teeth near 18 to 24 with a profile shift inside DIN 5480's range.
            let mods: Vec<f64> = match s.module {
                Some(m) => vec![m],
                None => SPLINE_MODULES.to_vec(),
            };
            let mut best: Option<(f64, f64, u32)> = None;
            for m in mods {
                let zs: Vec<u32> = match s.teeth {
                    Some(z) => vec![z],
                    None => {
                        let zc = (d - 1.1 * m) / m;
                        [zc.floor(), zc.ceil()].into_iter().filter(|z| *z >= 6.0).map(|z| z as u32).collect()
                    }
                };
                for z in zs {
                    let x = (d - m * f64::from(z) - 1.1 * m) / (2.0 * m);
                    if !(-0.05..=0.45).contains(&x) && s.teeth.is_none() {
                        continue;
                    }
                    let score = (f64::from(z) - 21.0).abs() + 2.0 * (x - 0.2).abs();
                    if best.is_none_or(|b| score < b.0) {
                        best = Some((score, m, z));
                    }
                }
            }
            let (_, m, z) = best.ok_or("no DIN 5480 module and teeth suit that diameter")?;
            (c.spec.module, c.spec.teeth) = (m, z);
        }
    } else if let Some(z) = s.teeth {
        c.spec.teeth = z;
    }
    let m = c.spec.module;
    let d = if c.spec.spline_diameter > 0.0 { c.spec.spline_diameter } else { m * (f64::from(c.spec.teeth) + 1.1) };
    c.face(0.85 * d);
    c.row("module", m, Unit::None);
    c.row("teeth", f64::from(c.spec.teeth), Unit::None);
    c.row("reference diameter", d, Unit::Mm);
    c.row("profile shift", (d - m * f64::from(c.spec.teeth) - 1.1 * m) / (2.0 * m), Unit::None);
    c.row("length", c.spec.face_width, Unit::Mm);
    Ok(())
}

fn face_gear(c: &mut Ctx) -> Result<(), String> {
    let s = c.s;
    let (d1, d2) = (s.pitch_diameter.filter(|d| *d > 0.0), s.pitch_diameter2.filter(|d| *d > 0.0));
    let ratio = s
        .ratio
        .or(match (d1, d2) {
            (Some(a), Some(b)) => Some(b / a),
            _ => None,
        })
        .unwrap_or(f64::from(c.spec.teeth2) / f64::from(c.spec.teeth));
    let target = d1.or(d2.map(|d| d / ratio));
    let base = c.same.then_some((c.base.module, c.base.teeth));
    if let Some(d) = target {
        let pitch = |m: f64, z: u32| m * f64::from(z);
        let (m, z) = fit_one(&modules(s.module, d / 80.0, d / 8.0, s.printable), (12, 60, 18), s.teeth, d, &pitch, base)
            .ok_or("no module and teeth give that pinion")?;
        c.spec.module = if s.exact { d / f64::from(z) } else { m };
        c.spec.teeth = z;
    } else {
        if let Some(m) = s.module {
            c.spec.module = m;
        }
        if let Some(z) = s.teeth {
            c.spec.teeth = z;
        }
    }
    let keep = c.same && c.base.teeth == c.spec.teeth && (f64::from(c.spec.teeth2) / f64::from(c.spec.teeth) / ratio - 1.0).abs() < 1e-9;
    if !keep {
        c.spec.teeth2 = ((f64::from(c.spec.teeth) * ratio).round() as u32).max(c.spec.teeth + 2);
    }
    let m = c.spec.module;
    c.face(2.5 * m);
    c.row("module", m, Unit::None);
    c.row("pinion teeth", f64::from(c.spec.teeth), Unit::None);
    c.row("face gear teeth", f64::from(c.spec.teeth2), Unit::None);
    ratio_rows(c, ratio, f64::from(c.spec.teeth2) / f64::from(c.spec.teeth));
    c.versus("pinion pitch diameter", d1, m * f64::from(c.spec.teeth));
    c.versus("face gear pitch diameter", d2, m * f64::from(c.spec.teeth2));
    c.row("tooth length", c.spec.face_width, Unit::Mm);
    Ok(())
}

fn elliptical(c: &mut Ctx) -> Result<(), String> {
    let s = c.s;
    if let Some(z) = s.teeth {
        c.spec.teeth = z;
    }
    if let Some(e) = s.eccentricity {
        c.spec.ellipse_eccentricity = e;
    }
    if let Some(a) = s.center_distance.or(s.offset).filter(|a| *a > 0.0) {
        c.spec.module = a / elliptic_unit(c.spec.teeth, c.spec.ellipse_eccentricity).0;
        c.note(format!("module {} gives the distance exactly", num(c.spec.module)));
    } else if let Some(m) = s.module {
        c.spec.module = m;
    }
    let m = c.spec.module;
    c.face(4.0 * m);
    let (a, sa, sb) = elliptic_unit(c.spec.teeth, c.spec.ellipse_eccentricity);
    c.row("module", m, Unit::None);
    c.row("teeth", f64::from(c.spec.teeth), Unit::None);
    c.row("centre distance", a * m, Unit::Mm);
    c.row("ellipse semi-major axis", sa * m, Unit::Mm);
    c.row("ellipse semi-minor axis", sb * m, Unit::Mm);
    c.row("face width", c.spec.face_width, Unit::Mm);
    Ok(())
}

thread_local! {
    static ELLIPSE: std::cell::RefCell<Vec<((u32, u64), (f64, f64, f64, Vec<[f64; 2]>))>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// An elliptical pair at module 1: centre distance, semi-axes and gear 2's pitch curve (sampled),
/// cached (its pitch curves take about a millisecond, and everything scales with the module).
fn elliptic_full(teeth: u32, ecc: f64) -> (f64, f64, f64, Vec<[f64; 2]>) {
    let key = (teeth, ecc.to_bits());
    if let Some(v) = ELLIPSE.with(|c| c.borrow().iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone())) {
        return v;
    }
    let p = crate::elliptic::EllipticPair {
        teeth,
        module: 1.0,
        eccentricity: ecc,
        pressure_angle: 20f64.to_radians(),
        addendum: 1.0,
        dedendum: 1.25,
        backlash: 0.0,
    }
    .pitch();
    let step = (p.curve2.len() / 120).max(1);
    let curve2: Vec<[f64; 2]> = p.curve2.iter().step_by(step).map(|q| [q.x, q.y]).collect();
    let v = (p.center, p.a, p.b, curve2);
    ELLIPSE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 16 {
            c.remove(0);
        }
        c.push((key, v.clone()));
    });
    v
}

fn elliptic_unit(teeth: u32, ecc: f64) -> (f64, f64, f64) {
    let (a, sa, sb, _) = elliptic_full(teeth, ecc);
    (a, sa, sb)
}

// =============================================================================================
// The reverse.

/// Every output of a spec in [`Simple`]'s terms (sizes, ratio, shafts), for showing; see
/// [`outputs_of`] for the few that drive a solve.
pub fn measures(spec: &Spec) -> Simple {
    let s = spec;
    let mut o = Simple { kind: Some(s.kind), face_width: Some(s.face_width), printable: s.nozzle > 0.0, ..Simple::default() };
    let bore = |b: &Bore| if b.kind == BoreKind::None { None } else { Some(b.diameter) };
    o.bore = bore(&s.bore);
    o.bore2 = bore(&s.bore2);
    let r = |a: u32, b: u32| f64::from(b) / f64::from(a.max(1));
    match s.kind {
        Kind::Spur | Kind::Helical | Kind::Herringbone => {
            let helix = if s.kind == Kind::Spur { 0.0 } else { s.helix };
            let g = s.involute(s.teeth, s.shift, helix);
            o.pitch_diameter = Some(2.0 * g.r());
            o.outer_diameter = Some(2.0 * g.ra());
            o.teeth = Some(s.teeth);
            o.module = Some(s.module);
            o.shaft_angle = Some(0.0);
            o.offset = Some(0.0);
            if s.kind != Kind::Spur {
                o.helix = Some(s.helix.abs());
                o.left_hand = Some(s.helix < 0.0);
            }
            o.herringbone = s.kind == Kind::Herringbone;
        }
        Kind::Pair => {
            let (g1, g2, a) = pair_gears(s);
            o.ratio = Some(r(s.teeth, s.teeth2));
            o.center_distance = Some(a);
            o.shaft_angle = Some(0.0);
            o.offset = Some(a);
            o.pitch_diameter = Some(2.0 * g1.r());
            o.pitch_diameter2 = Some(2.0 * g2.r());
            o.outer_diameter = Some(2.0 * g1.ra().max(g2.ra()));
            o.teeth = Some(s.teeth);
            o.module = Some(s.module);
            o.helix = Some(s.helix.abs());
            o.herringbone = s.herringbone;
            o.left_hand = Some(s.helix < 0.0);
            o.face_width2 = (s.face_width2 > 0.0).then_some(s.face_width2);
        }
        Kind::Internal => {
            let ring = Internal { gear: s.involute(s.teeth, s.shift, s.helix) };
            let rim = if s.rim > 0.0 { s.rim } else { 3.0 * s.module };
            o.ring_diameter = Some(2.0 * ring.gear.r());
            o.outer_diameter = Some(2.0 * (ring.r_root() + rim));
            o.teeth = Some(s.teeth);
            o.module = Some(s.module);
            o.shaft_angle = Some(0.0);
            o.offset = Some(0.0);
        }
        Kind::Planetary => {
            let zr = if s.teeth3 > 0 { s.teeth3 } else { s.teeth + 2 * s.teeth2 };
            let mt = s.module / s.helix.to_radians().cos();
            o.ratio = Some(1.0 + f64::from(zr) / f64::from(s.teeth));
            o.planets = Some(s.planets);
            o.pitch_diameter = Some(mt * f64::from(s.teeth));
            o.pitch_diameter2 = Some(mt * f64::from(s.teeth2));
            o.ring_diameter = Some(mt * f64::from(zr));
            let rim = if s.rim > 0.0 { s.rim } else { 3.0 * s.module };
            o.outer_diameter = Some(mt * f64::from(zr) + 2.0 * s.rack_eff().dedendum * s.module + 2.0 * rim);
            o.teeth = Some(s.teeth);
            o.module = Some(s.module);
            o.shaft_angle = Some(0.0);
            o.offset = Some(0.0);
        }
        Kind::Rack => {
            let mt = s.module / s.helix.to_radians().cos();
            o.length = Some(PI * mt * f64::from(s.rack_teeth.max(1)));
            o.pitch_diameter = Some(mt * f64::from(s.teeth));
            o.teeth = Some(s.teeth);
            o.module = Some(s.module);
        }
        Kind::Bevel | Kind::SpiralBevel => {
            o.shaft_angle = Some(s.shaft_angle);
            o.offset = Some(0.0);
            o.ratio = Some(r(s.teeth, s.teeth2));
            o.pitch_diameter = Some(s.module * f64::from(s.teeth));
            o.pitch_diameter2 = Some(s.module * f64::from(s.teeth2));
            if let Some((d1, _)) = cones(s.shaft_angle, f64::from(s.teeth), f64::from(s.teeth2)) {
                o.cone_distance = Some(s.module * f64::from(s.teeth) / 2.0 / d1.sin());
            }
            o.teeth = Some(s.teeth);
            o.module = Some(s.module);
            if s.kind == Kind::SpiralBevel {
                o.spiral_angle = Some(s.spiral_angle);
            }
        }
        Kind::Hypoid => {
            o.shaft_angle = Some(s.shaft_angle);
            o.offset = Some(s.offset);
            o.ratio = Some(r(s.teeth, s.teeth2));
            o.pitch_diameter2 = Some(s.module * f64::from(s.teeth2));
            o.teeth = Some(s.teeth);
            o.spiral_angle = Some(s.spiral_angle);
            o.left_hand = Some(s.left_hand);
        }
        Kind::CrossedHelical => {
            let b1 = s.helix;
            let b2 = s.shaft_angle - b1;
            let g1 = s.involute(s.teeth, s.shift, b1);
            let g2 = s.involute2(s.teeth2, s.shift2, b2);
            let e = g1.r() + g2.r() + (s.shift + s.shift2) * s.module;
            o.shaft_angle = Some(s.shaft_angle);
            o.offset = Some(e);
            o.center_distance = Some(e);
            o.ratio = Some(r(s.teeth, s.teeth2));
            o.pitch_diameter = Some(2.0 * g1.r());
            o.pitch_diameter2 = Some(2.0 * g2.r());
            o.helix_split = Some(b1);
            o.teeth = Some(s.teeth);
            o.module = Some(s.module);
        }
        Kind::Worm => {
            let m = s.module;
            let d1 = if s.worm_diameter > 0.0 { s.worm_diameter } else { 10.0 * m };
            let a = d1 / 2.0 + m * f64::from(s.teeth) / 2.0 + s.shift * m;
            o.shaft_angle = Some(90.0);
            o.offset = Some(a);
            o.center_distance = Some(a);
            o.ratio = Some(f64::from(s.teeth) / f64::from(s.starts.max(1)));
            o.pitch_diameter = Some(d1);
            o.pitch_diameter2 = Some(m * f64::from(s.teeth));
            o.starts = Some(s.starts);
            o.module = Some(m);
            o.left_hand = Some(s.helix < 0.0);
        }
        Kind::Cycloidal => {
            o.teeth = Some(s.teeth);
            o.pitch_diameter = Some(s.module * f64::from(s.teeth));
            o.module = Some(s.module);
            if s.teeth2 > 0 {
                o.ratio = Some(f64::from(s.teeth) / f64::from(s.teeth2));
                o.pitch_diameter2 = Some(s.module * f64::from(s.teeth2));
                let a = s.module * f64::from(s.teeth + s.teeth2) / 2.0;
                o.center_distance = Some(a);
                o.offset = Some(a);
                o.shaft_angle = Some(0.0);
            }
        }
        Kind::CycloidalDrive => {
            o.ratio = Some(f64::from(s.pins.max(2) - 1));
            o.pitch_diameter = Some(s.pin_circle);
            o.outer_diameter = Some(s.pin_circle + s.pin_diameter);
            o.shaft_angle = Some(0.0);
            o.offset = Some(0.0);
        }
        Kind::Sprocket | Kind::Pulley => {
            let dia = |z: u32| {
                if s.kind == Kind::Sprocket {
                    let (_, p, ..) = crate::plan::extras::chain_of(s);
                    p / (PI / f64::from(z)).sin()
                } else {
                    f64::from(z) * crate::plan::extras::belt_of(s).pitch / PI
                }
            };
            o.teeth = Some(s.teeth);
            o.pitch_diameter = Some(dia(s.teeth));
            if s.kind == Kind::Sprocket {
                o.chain = Some(s.chain);
                o.face_width = None;
            } else {
                o.belt = Some(s.belt);
            }
            if s.teeth2 > 0 {
                o.ratio = Some(r(s.teeth, s.teeth2));
                o.pitch_diameter2 = Some(dia(s.teeth2));
                if s.center_distance > 0.0 {
                    o.center_distance = Some(s.center_distance);
                }
                o.offset = wrap_center(s);
                o.shaft_angle = Some(0.0);
            }
        }
        Kind::Spline => {
            o.pitch_diameter = Some(if s.spline_diameter > 0.0 { s.spline_diameter } else { s.module * (f64::from(s.teeth) + 1.1) });
            o.teeth = Some(s.teeth);
            o.module = Some(s.module);
        }
        Kind::FaceGear => {
            o.shaft_angle = Some(90.0);
            o.offset = Some(0.0);
            o.ratio = Some(r(s.teeth, s.teeth2));
            o.pitch_diameter = Some(s.module * f64::from(s.teeth));
            o.pitch_diameter2 = Some(s.module * f64::from(s.teeth2));
            o.teeth = Some(s.teeth);
            o.module = Some(s.module);
        }
        Kind::Elliptical => {
            let a = elliptic_unit(s.teeth, s.ellipse_eccentricity).0 * s.module;
            o.center_distance = Some(a);
            o.offset = Some(a);
            o.shaft_angle = Some(0.0);
            o.teeth = Some(s.teeth);
            o.eccentricity = Some(s.ellipse_eccentricity);
            o.module = Some(s.module);
        }
        Kind::Differential => {}
        Kind::BevelPlanetary => {
            let g = bevel_planet::geometry(s).ok();
            o.shaft_angle = Some(g.as_ref().map_or(s.shaft_angle, |g| g.sigma.to_degrees()));
            o.offset = Some(0.0);
            o.ratio = Some(r(s.teeth, s.teeth2));
            o.planets = Some(s.planets);
            o.bevel_planet_mode = Some(s.bevel_planet_mode);
            o.pitch_diameter = Some(s.module * f64::from(s.teeth));
            o.pitch_diameter2 = Some(s.module * f64::from(s.teeth2));
            if let Some(g) = &g {
                o.cone_distance = Some(g.re);
                if g.z_far > 0 {
                    o.ring_diameter = Some(s.module * f64::from(g.z_far));
                }
            }
            o.teeth = Some(s.teeth);
            o.module = Some(s.module);
            o.spiral_angle = Some(s.spiral_angle);
        }
    }
    o
}

/// The outputs that drive a solve of this spec, for a form switched from Advanced to Simple:
/// a minimal, consistent set (a person's numbers: ratio, centre distance or one size, angles,
/// face width), so that editing one of them re-solves the rest. `solve(&outputs_of(s), s)`
/// gives `s` back (its module and teeth are kept while they meet the targets). Everything else
/// is in [`measures`] and in [`Solved::chosen`].
pub fn outputs_of(spec: &Spec) -> Simple {
    let all = measures(spec);
    let mut o =
        Simple { kind: all.kind, face_width: all.face_width, printable: all.printable, bore: all.bore, bore2: all.bore2, ..Simple::default() };
    match spec.kind {
        Kind::Spur | Kind::Helical | Kind::Herringbone => {
            o.outer_diameter = all.outer_diameter;
            o.helix = all.helix;
            o.left_hand = all.left_hand;
            o.herringbone = all.herringbone;
        }
        Kind::Pair => {
            o.ratio = all.ratio;
            o.center_distance = all.center_distance;
            o.shaft_angle = Some(0.0);
            o.helix = all.helix;
            o.left_hand = all.left_hand;
            o.face_width2 = all.face_width2;
            o.herringbone = all.herringbone;
        }
        Kind::Internal => o.outer_diameter = all.outer_diameter,
        Kind::Planetary => {
            o.ratio = all.ratio;
            o.planets = all.planets;
            o.ring_diameter = all.ring_diameter;
        }
        Kind::Rack => {
            o.length = all.length;
            o.pitch_diameter = all.pitch_diameter;
        }
        Kind::Bevel | Kind::SpiralBevel => {
            o.shaft_angle = all.shaft_angle;
            o.ratio = all.ratio;
            o.pitch_diameter = all.pitch_diameter;
            o.spiral_angle = all.spiral_angle;
        }
        Kind::Hypoid => {
            o.shaft_angle = all.shaft_angle;
            o.offset = all.offset;
            o.ratio = all.ratio;
            o.pitch_diameter2 = all.pitch_diameter2;
            o.spiral_angle = all.spiral_angle;
            o.left_hand = all.left_hand;
        }
        Kind::CrossedHelical => {
            o.shaft_angle = all.shaft_angle;
            o.center_distance = all.center_distance;
            o.ratio = all.ratio;
            o.helix_split = all.helix_split;
        }
        Kind::Worm => {
            o.ratio = all.ratio;
            o.center_distance = all.center_distance;
            o.starts = all.starts;
            o.left_hand = all.left_hand;
        }
        Kind::Cycloidal => {
            o.teeth = all.teeth;
            o.ratio = all.ratio;
            if all.ratio.is_some() {
                o.center_distance = all.center_distance;
            } else {
                o.pitch_diameter = all.pitch_diameter;
            }
        }
        Kind::CycloidalDrive => {
            o.ratio = all.ratio;
            o.outer_diameter = all.outer_diameter;
        }
        Kind::Sprocket | Kind::Pulley => {
            o.teeth = all.teeth;
            o.chain = all.chain;
            o.belt = all.belt;
            o.ratio = all.ratio;
            o.center_distance = all.center_distance;
            if spec.kind == Kind::Sprocket {
                o.face_width = None;
            }
        }
        Kind::Spline => o.pitch_diameter = all.pitch_diameter,
        Kind::FaceGear => {
            o.ratio = all.ratio;
            o.pitch_diameter = all.pitch_diameter;
        }
        Kind::Elliptical => {
            o.center_distance = all.center_distance;
            o.teeth = all.teeth;
            o.eccentricity = all.eccentricity;
        }
        Kind::Differential => {}
        Kind::BevelPlanetary => {
            o.shaft_angle = all.shaft_angle;
            o.ratio = all.ratio;
            o.planets = all.planets;
            o.bevel_planet_mode = all.bevel_planet_mode;
            o.pitch_diameter = all.pitch_diameter;
            o.spiral_angle = all.spiral_angle;
        }
    }
    o
}

// =============================================================================================
// The schematic.

/// What a part of a member's blank stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// The pitch surface (circle, cylinder, cone, curve).
    Pitch,
    Tip,
    Root,
    /// The rest of the blank (a ring's rim, a rack's bar, a face gear's base, a disc).
    Body,
    Bore,
    Hub,
    Shaft,
    /// A worm's thread (its helix on the pitch cylinder).
    Thread,
    /// A chain or belt path, its pitch line.
    Wrap,
}

/// A simple solid or line in a member's frame (z along its axis, lengths in mm).
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// A cylinder about the axis from z0 to z1.
    Cylinder { r: f64, z0: f64, z1: f64 },
    /// A tube about the axis.
    Tube { r_in: f64, r_out: f64, z0: f64, z1: f64 },
    /// A frustum about the axis: radius r0 at z0 to r1 at z1, on a cone whose apex is at
    /// `apex_z` on the axis with `half_angle` (degrees).
    Cone { r0: f64, z0: f64, r1: f64, z1: f64, apex_z: f64, half_angle: f64 },
    /// A surface of revolution through (radius, z) points (a hyperboloid's section, a worm
    /// wheel's throat).
    Revolved { profile: Vec<[f64; 2]> },
    /// A cylinder parallel to the axis, centred at (x, y).
    Offset { x: f64, y: f64, r: f64, z0: f64, z1: f64 },
    /// A box, corners in the member's frame.
    Box { min: [f64; 3], max: [f64; 3] },
    /// A polyline in the member's frame (helix, chain or belt path, an elliptical pitch curve).
    Path { points: Vec<[f64; 3]>, closed: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub role: Role,
    pub shape: Shape,
}

/// One member of a set.
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    /// The name and colour index of the same member in `plan().preview` (the index is its place
    /// there).
    pub name: String,
    pub color: usize,
    /// Its body's index in `plan().bodies` (None: not a body, as a chain, or not built with
    /// `Spec::only`).
    pub body: Option<usize>,
    /// Its frame in world coordinates (after `Spec::placement`): the same as its body's.
    pub frame: Frame,
    pub pitch_diameter: f64,
    pub parts: Vec<Part>,
}

impl Member {
    /// A point of the member's frame in world coordinates.
    pub fn world(&self, p: [f64; 3]) -> [f64; 3] {
        self.frame.world(p)
    }
    /// The member's axis: a world point on it and its direction.
    pub fn axis(&self) -> ([f64; 3], [f64; 3]) {
        (self.frame.origin, self.frame.z)
    }
}

/// What a dimension measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DimKind {
    ShaftAngle,
    Offset,
    CenterDistance,
    PitchDiameter,
    OuterDiameter,
    ConeDistance,
    FaceWidth,
    Length,
}

/// A dimension to annotate: its value and the world points it is drawn between (angle: the
/// vertex then a point on each arm; diameter: the centre, then a point on the circle, then the
/// axis direction as a point one unit from the centre).
#[derive(Clone, Debug, PartialEq)]
pub struct Dim {
    pub kind: DimKind,
    pub label: String,
    pub value: f64,
    pub unit: Unit,
    /// The members it relates (indices into `members`).
    pub members: Vec<usize>,
    pub points: Vec<[f64; 3]>,
    /// The Simple field that edits it.
    pub field: Option<Field>,
}

/// A set without its teeth, for an instant preview and a drawing.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Schematic {
    pub members: Vec<Member>,
    pub dims: Vec<Dim>,
    /// Why some of it couldn't be worked out (the rest is still drawn).
    pub note: Option<String>,
}

fn cyl(role: Role, r: f64, z0: f64, z1: f64) -> Part {
    Part { role, shape: Shape::Cylinder { r, z0, z1 } }
}

fn v_add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn v_sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn v_mul(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn v_dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The feet of the common perpendicular of two lines (None: parallel).
fn feet(p1: [f64; 3], d1: [f64; 3], p2: [f64; 3], d2: [f64; 3]) -> Option<([f64; 3], [f64; 3])> {
    let w = v_sub(p1, p2);
    let (a, b, c, d, e) = (v_dot(d1, d1), v_dot(d1, d2), v_dot(d2, d2), v_dot(d1, w), v_dot(d2, w));
    let den = a * c - b * b;
    if den.abs() < 1e-12 * a * c {
        return None;
    }
    Some((v_add(p1, v_mul(d1, (b * e - c * d) / den)), v_add(p2, v_mul(d2, (a * e - b * d) / den))))
}

/// Hub, bore and shafts of a cylindrical member whose blank runs from z0 to z1 (the hub and
/// the front shaft on the z1 side).
fn extras(s: &Spec, second: bool, z0: f64, z1: f64, parts: &mut Vec<Part>) {
    let bore = if second { &s.bore2 } else { &s.bore };
    if bore.kind != BoreKind::None && bore.diameter > 0.0 {
        parts.push(cyl(Role::Bore, bore.diameter / 2.0, z0, z1));
    }
    if !second && s.hub_diameter > 0.0 && s.hub_length > 0.0 {
        parts.push(cyl(Role::Hub, s.hub_diameter / 2.0, z1, z1 + s.hub_length));
    }
    let (front, back) = (s.shaft(second, true), s.shaft(second, false));
    let hub = if !second && s.hub_diameter > 0.0 { s.hub_length } else { 0.0 };
    for (sh, front) in [(front, true), (back, false)] {
        if !sh.exists() {
            continue;
        }
        let (start, dir) = if front { (z1 + hub, 1.0) } else { (z0, -1.0) };
        let end1 = start + dir * sh.length;
        parts.push(cyl(Role::Shaft, sh.diameter / 2.0, start.min(end1), start.max(end1)));
        if sh.stepped() {
            let end2 = end1 + dir * sh.step_length;
            parts.push(cyl(Role::Shaft, sh.step_diameter / 2.0, end1.min(end2), end1.max(end2)));
        }
    }
}

/// A cylindrical gear's pitch, tip and root cylinders from z0 to z1.
fn blank(g: &Involute, z0: f64, z1: f64) -> Vec<Part> {
    vec![cyl(Role::Pitch, g.r(), z0, z1), cyl(Role::Tip, g.ra(), z0, z1), cyl(Role::Root, g.rf().max(0.0), z0, z1)]
}

struct Build<'a> {
    s: &'a Spec,
    out: Schematic,
    next_body: usize,
}

impl Build<'_> {
    /// Adds a member; `member` is its number for `Spec::only` (0: not a body).
    fn add(&mut self, name: &str, member: u8, frame: Frame, pitch_diameter: f64, parts: Vec<Part>) -> usize {
        let built = member > 0 && (self.s.only == 0 || self.s.only == member);
        let body = built.then(|| {
            self.next_body += 1;
            self.next_body - 1
        });
        let frame = match &self.s.placement {
            Some(p) => p.compose(&frame),
            None => frame,
        };
        let color = self.out.members.len();
        self.out.members.push(Member { name: name.into(), color, body, frame, pitch_diameter, parts });
        color
    }
    fn dim(&mut self, kind: DimKind, label: &str, value: f64, unit: Unit, members: Vec<usize>, points: Vec<[f64; 3]>, field: Option<Field>) {
        self.out.dims.push(Dim { kind, label: label.into(), value, unit, members, points, field });
    }
    /// A pitch diameter dimension on member `i` at its local height z.
    fn diameter(&mut self, i: usize, label: &str, d: f64, z: f64, field: Option<Field>) {
        let m = &self.out.members[i];
        let c = m.world([0.0, 0.0, z]);
        let p = m.world([d / 2.0, 0.0, z]);
        let ax = v_add(c, m.frame.z);
        self.dim(DimKind::PitchDiameter, label, d, Unit::Mm, vec![i], vec![c, p, ax], field);
    }
    /// The centre distance between two members' axes at member `i`'s local height z.
    fn center(&mut self, i: usize, j: usize, value: f64, z: f64, field: Option<Field>) {
        let (a, b) = (&self.out.members[i], &self.out.members[j]);
        let p = a.world([0.0, 0.0, z]);
        let (q0, qd) = b.axis();
        let q = v_add(q0, v_mul(qd, v_dot(v_sub(p, q0), qd)));
        self.dim(DimKind::CenterDistance, "centre distance", value, Unit::Mm, vec![i, j], vec![p, q], field);
    }
    /// Shaft angle and offset between two members' axes (at the common perpendicular, or the apex).
    fn shafts(&mut self, i: usize, j: usize, angle: f64, offset_field: Option<Field>, offset_kind: DimKind) {
        let (a, b) = (&self.out.members[i], &self.out.members[j]);
        let ((p1, d1), (p2, d2)) = (a.axis(), b.axis());
        let Some((f1, f2)) = feet(p1, d1, p2, d2) else { return };
        let reach = 0.5 * (a.pitch_diameter + b.pitch_diameter).max(10.0);
        // The arms point towards the members.
        let toward = |f: [f64; 3], p: [f64; 3], d: [f64; 3]| if v_dot(v_sub(p, f), d) >= 0.0 { d } else { v_mul(d, -1.0) };
        let (t1, t2) = (toward(f1, p1, d1), toward(f2, p2, d2));
        let e = v_sub(f2, f1);
        let e_len = v_dot(e, e).sqrt();
        let vertex = f1;
        self.dim(
            DimKind::ShaftAngle,
            "shaft angle",
            angle,
            Unit::Deg,
            vec![i, j],
            vec![vertex, v_add(vertex, v_mul(t1, reach)), v_add(vertex, v_mul(t2, reach))],
            Some(Field::ShaftAngle),
        );
        if e_len > 1e-6 {
            let label = if offset_kind == DimKind::Offset { "offset" } else { "centre distance" };
            self.dim(offset_kind, label, e_len, Unit::Mm, vec![i, j], vec![f1, f2], offset_field);
        }
    }
}

/// The set's schematic: every member's frame, pitch/tip/root surfaces and blank, and the
/// dimensions between them, from the math alone (no teeth): well under a millisecond except a
/// hypoid's first call (its pitch cone design, cached after). Frames agree with `plan()`'s
/// bodies, names and colour indices with its preview members, diameters with its facts.
pub fn schematic(spec: &Spec) -> Schematic {
    if spec.kind == Kind::Differential {
        return crate::diff::schematic(spec);
    }
    let mut b = Build { s: spec, out: Schematic::default(), next_body: 0 };
    if let Some(t) = &spec.train {
        train_schematic(&mut b, t);
        return b.out;
    }
    let s = spec;
    let o = s.origin;
    let at = |d: [f64; 3]| Frame::at([o[0] + d[0], o[1] + d[1], o[2] + d[2]]);
    match s.kind {
        Kind::Spur | Kind::Helical | Kind::Herringbone => {
            let helix = if s.kind == Kind::Spur { 0.0 } else { s.helix };
            let g = s.involute(s.teeth, s.shift, helix);
            let mut parts = blank(&g, 0.0, s.face_width);
            extras(s, false, 0.0, s.face_width, &mut parts);
            let i = b.add("gear", 1, at([0.0; 3]), 2.0 * g.r(), parts);
            b.diameter(i, "pitch diameter", 2.0 * g.r(), s.face_width, Some(Field::PitchDiameter));
            face_dim(&mut b, i, s.face_width, g.ra());
        }
        Kind::Internal => {
            let ring = Internal { gear: s.involute(s.teeth, s.shift, s.helix) };
            let rim = if s.rim > 0.0 { s.rim } else { 3.0 * s.module };
            let parts = vec![
                cyl(Role::Pitch, ring.gear.r(), 0.0, s.face_width),
                Part { role: Role::Body, shape: Shape::Tube { r_in: ring.r_tip(), r_out: ring.r_root() + rim, z0: 0.0, z1: s.face_width } },
                cyl(Role::Root, ring.r_root(), 0.0, s.face_width),
            ];
            let i = b.add("ring", 1, at([0.0; 3]), 2.0 * ring.gear.r(), parts);
            b.diameter(i, "ring pitch diameter", 2.0 * ring.gear.r(), s.face_width, Some(Field::RingDiameter));
        }
        Kind::Rack => {
            let g = s.involute(s.teeth.max(1), s.shift, s.helix);
            let below = if s.rack_height > 0.0 { s.rack_height } else { (3.0 * s.module).max((s.rack.dedendum - s.shift + 2.0) * s.module) };
            let n = s.rack_teeth.max(1);
            let len = PI * g.mt() * f64::from(n);
            let top = (s.rack.addendum + s.shift) * s.module;
            let parts = vec![
                Part { role: Role::Body, shape: Shape::Box { min: [0.0, -below, 0.0], max: [len, top, s.face_width] } },
                Part { role: Role::Pitch, shape: Shape::Path { points: vec![[0.0, 0.0, s.face_width], [len, 0.0, s.face_width]], closed: false } },
            ];
            let i = b.add("rack", 1, at([0.0; 3]), 0.0, parts);
            let m = &b.out.members[i];
            let pts = vec![m.world([0.0, top, s.face_width]), m.world([len, top, s.face_width])];
            b.dim(DimKind::Length, "rack length", len, Unit::Mm, vec![i], pts, Some(Field::Length));
        }
        Kind::Pair => {
            let (g1, g2, a) = pair_gears(s);
            let face2 = s.face_width_of2();
            let dz2 = (s.face_width - face2) / 2.0 + s.face_offset2;
            let mut p1 = blank(&g1, 0.0, s.face_width);
            extras(s, false, 0.0, s.face_width, &mut p1);
            let mut p2 = blank(&g2, 0.0, face2);
            extras(s, true, 0.0, face2, &mut p2);
            let i = b.add("gear 1", 1, at([0.0; 3]), 2.0 * g1.r(), p1);
            let j = b.add("gear 2", 2, at([a, 0.0, dz2]), 2.0 * g2.r(), p2);
            b.center(i, j, a, s.face_width / 2.0, Some(Field::CenterDistance));
            b.diameter(i, "gear 1 pitch diameter", 2.0 * g1.r(), s.face_width, Some(Field::PitchDiameter));
            b.diameter(j, "gear 2 pitch diameter", 2.0 * g2.r(), face2, Some(Field::PitchDiameter2));
            face_dim(&mut b, i, s.face_width, g1.ra());
        }
        Kind::Planetary => {
            let (zs, zp) = (s.teeth, s.teeth2);
            let zr = if s.teeth3 > 0 { s.teeth3 } else { zs + 2 * zp };
            let n = s.planets.max(1);
            let sun = s.involute(zs, s.shift, s.helix);
            let planet = s.involute2(zp, s.shift2, -s.helix);
            let a = mesh_external(&sun, &planet, s.face_width).center_distance;
            let ring = Internal { gear: s.involute(zr, 0.0, -s.helix) };
            let rim = if s.rim > 0.0 { s.rim } else { 3.0 * s.module };
            let mut ps = blank(&sun, 0.0, s.face_width);
            extras(s, false, 0.0, s.face_width, &mut ps);
            let i = b.add("sun", 1, at([0.0; 3]), 2.0 * sun.r(), ps);
            let total = f64::from(zs + zr);
            let mut first = None;
            for k in 0..n {
                let th = TAU * (f64::from(k) * total / f64::from(n)).round() / total;
                let mut pp = blank(&planet, 0.0, s.face_width);
                extras(s, true, 0.0, s.face_width, &mut pp);
                let j = b.add(&format!("planet {}", k + 1), 2, at([a * th.cos(), a * th.sin(), 0.0]), 2.0 * planet.r(), pp);
                first.get_or_insert(j);
            }
            let parts = vec![
                cyl(Role::Pitch, ring.gear.r(), 0.0, s.face_width),
                Part { role: Role::Body, shape: Shape::Tube { r_in: ring.r_tip(), r_out: ring.r_root() + rim, z0: 0.0, z1: s.face_width } },
            ];
            let r = b.add("ring", 3, at([0.0; 3]), 2.0 * ring.gear.r(), parts);
            if let Some(j) = first {
                b.center(i, j, a, s.face_width / 2.0, None);
                b.diameter(j, "planet pitch diameter", 2.0 * planet.r(), s.face_width, Some(Field::PitchDiameter2));
            }
            b.diameter(i, "sun pitch diameter", 2.0 * sun.r(), s.face_width, Some(Field::PitchDiameter));
            b.diameter(r, "ring pitch diameter", 2.0 * ring.gear.r(), s.face_width, Some(Field::RingDiameter));
        }
        Kind::Bevel | Kind::SpiralBevel => bevel_schematic(&mut b),
        Kind::BevelPlanetary => bevel_planet_schematic(&mut b),
        Kind::Differential => {}
        Kind::Hypoid => hypoid_schematic(&mut b),
        Kind::CrossedHelical => {
            let sigma = s.shaft_angle.to_radians();
            let (b1, b2) = (s.helix, s.shaft_angle - s.helix);
            let g1 = s.involute(s.teeth, s.shift, b1);
            let g2 = s.involute2(s.teeth2, s.shift2, b2);
            let e = g1.r() + g2.r() + (s.shift + s.shift2) * s.module;
            let h = s.face_width / 2.0;
            let mut p1 = blank(&g1, -h, h);
            extras(s, false, -h, h, &mut p1);
            let mut p2 = blank(&g2, -h, h);
            extras(s, true, -h, h, &mut p2);
            let i = b.add("gear 1", 1, at([0.0; 3]), 2.0 * g1.r(), p1);
            let j = b.add(
                "gear 2",
                2,
                Frame { origin: [o[0] + e, o[1], o[2]], x: [-1.0, 0.0, 0.0], z: [0.0, sigma.sin(), sigma.cos()] },
                2.0 * g2.r(),
                p2,
            );
            b.shafts(i, j, s.shaft_angle, Some(Field::CenterDistance), DimKind::CenterDistance);
            b.diameter(i, "gear 1 pitch diameter", 2.0 * g1.r(), 0.0, Some(Field::PitchDiameter));
            b.diameter(j, "gear 2 pitch diameter", 2.0 * g2.r(), 0.0, Some(Field::PitchDiameter2));
        }
        Kind::Worm => worm_schematic(&mut b),
        Kind::Cycloidal => {
            let m = s.module;
            let (r1, r2) = (m * f64::from(s.teeth) / 2.0, m * f64::from(s.teeth2) / 2.0);
            let mut p1 = vec![cyl(Role::Pitch, r1, 0.0, s.face_width), cyl(Role::Tip, r1 + m, 0.0, s.face_width)];
            extras(s, false, 0.0, s.face_width, &mut p1);
            let i = b.add("gear 1", 1, at([0.0; 3]), 2.0 * r1, p1);
            b.diameter(i, "wheel pitch diameter", 2.0 * r1, s.face_width, Some(Field::PitchDiameter));
            if s.teeth2 > 0 {
                let mut p2 = vec![cyl(Role::Pitch, r2, 0.0, s.face_width), cyl(Role::Tip, r2 + m, 0.0, s.face_width)];
                extras(s, true, 0.0, s.face_width, &mut p2);
                let j = b.add("gear 2", 2, at([r1 + r2, 0.0, 0.0]), 2.0 * r2, p2);
                b.center(i, j, r1 + r2, s.face_width / 2.0, Some(Field::CenterDistance));
            }
        }
        Kind::CycloidalDrive => {
            let (pc, pr, e) = (s.pin_circle / 2.0, s.pin_diameter / 2.0, s.eccentricity);
            let mut disc = vec![cyl(Role::Body, pc - pr - e, 0.0, s.face_width), cyl(Role::Tip, pc - pr + e, 0.0, s.face_width)];
            extras(s, false, 0.0, s.face_width, &mut disc);
            let i = b.add("disc", 1, at([e, 0.0, 0.0]), 2.0 * (pc - pr), disc);
            let n = s.pins.max(1);
            let pins: Vec<Part> = (0..n)
                .map(|k| {
                    let t = TAU * f64::from(k) / f64::from(n);
                    Part { role: Role::Body, shape: Shape::Offset { x: pc * t.cos(), y: pc * t.sin(), r: pr, z0: 0.0, z1: s.face_width } }
                })
                .chain(std::iter::once(cyl(Role::Pitch, pc, 0.0, s.face_width)))
                .collect();
            let j = b.add("pins", 2, at([0.0; 3]), 2.0 * pc, pins);
            b.diameter(j, "pin circle diameter", 2.0 * pc, s.face_width, Some(Field::PitchDiameter));
            let _ = i;
        }
        Kind::Sprocket | Kind::Pulley => wrapped_schematic(&mut b),
        Kind::Spline => {
            let sp = crate::plan::extras::spline_of(s);
            let (da1, df1) = sp.shaft_diameters();
            let (da2, df2) = sp.hub_diameters();
            let face = s.face_width;
            let r_out = if s.hub_diameter > 0.0 {
                s.hub_diameter / 2.0
            } else {
                df2 / 2.0 + if s.rim > 0.0 { s.rim } else { (3.0 * s.module).max(0.25 * sp.reference_diameter()) }
            };
            let mut shaft = vec![
                cyl(Role::Pitch, sp.pitch_diameter() / 2.0, -face / 2.0, 1.5 * face),
                cyl(Role::Tip, da1 / 2.0, -face / 2.0, 1.5 * face),
                cyl(Role::Root, df1 / 2.0, -face / 2.0, 1.5 * face),
            ];
            if s.bore.kind != BoreKind::None && s.bore.diameter > 0.0 {
                shaft.push(cyl(Role::Bore, s.bore.diameter / 2.0, -face / 2.0, 1.5 * face));
            }
            let hub = vec![Part { role: Role::Body, shape: Shape::Tube { r_in: da2 / 2.0, r_out, z0: 0.0, z1: face } }];
            // Preview order: hub, then shaft; bodies: shaft (member 1), then hub (member 2).
            let shaft_body = s.only == 0 || s.only == 1;
            let hub_body = s.only == 0 || s.only == 2;
            let place = |f: Frame| match &s.placement {
                Some(p) => p.compose(&f),
                None => f,
            };
            b.out.members.push(Member {
                name: "hub".into(),
                color: 0,
                body: hub_body.then_some(usize::from(shaft_body)),
                frame: place(at([0.0; 3])),
                pitch_diameter: sp.pitch_diameter(),
                parts: hub,
            });
            b.out.members.push(Member {
                name: "shaft".into(),
                color: 1,
                body: shaft_body.then_some(0),
                frame: place(at([0.0; 3])),
                pitch_diameter: sp.pitch_diameter(),
                parts: shaft,
            });
            b.diameter(1, "reference diameter", sp.reference_diameter(), 1.5 * face, Some(Field::PitchDiameter));
            let m = &b.out.members[0];
            let pts = vec![m.world([r_out, 0.0, 0.0]), m.world([r_out, 0.0, face])];
            b.dim(DimKind::FaceWidth, "length", face, Unit::Mm, vec![0], pts, Some(Field::FaceWidth));
        }
        Kind::FaceGear => {
            let m = s.module;
            let plain = Spec { clearance: 0.0, xy_compensation: 0.0, tip_shortening: 0.0, ..s.clone() };
            let pinion = plain.involute(s.teeth, 0.0, 0.0);
            let r1 = pinion.r();
            // The undercut limit (FaceGear::undercut_radius without its cutter).
            let top = s.rack.addendum * m;
            let rw = (pinion.rb().powi(2) / (r1 - top).max(1e-9)).max(pinion.rb() / 8f64.to_radians().cos());
            let r_min = rw * f64::from(s.teeth2) / f64::from(s.teeth);
            let ri = if s.inner_diameter > 0.0 { (s.inner_diameter / 2.0).max(r_min) } else { r_min };
            // (The outer end may be held back where the teeth come to a point: not worked out here.)
            let ro = ri + s.face_width;
            let rim = if s.rim > 0.0 { s.rim } else { 2.5 * m };
            let bottom = -s.rack.dedendum * m;
            let fg = vec![
                Part { role: Role::Pitch, shape: Shape::Tube { r_in: ri, r_out: ro, z0: 0.0, z1: 0.0 } },
                Part { role: Role::Tip, shape: Shape::Tube { r_in: ri, r_out: ro, z0: bottom, z1: top } },
                Part { role: Role::Body, shape: Shape::Cylinder { r: ro + m, z0: bottom - rim, z1: bottom } },
            ];
            let len = ro - ri;
            let mut pp = blank(&pinion, 0.0, len);
            extras(s, true, 0.0, len, &mut pp);
            let pin_frame = Frame { origin: [o[0] + ri, o[1], o[2] + r1], x: [0.0, 0.0, -1.0], z: [1.0, 0.0, 0.0] };
            // Preview order: pinion, face gear; bodies: face gear (1), pinion (2).
            let (fg_body, pin_body) = (s.only == 0 || s.only == 1, s.only == 0 || s.only == 2);
            let place = |f: Frame| match &s.placement {
                Some(p) => p.compose(&f),
                None => f,
            };
            b.out.members.push(Member {
                name: "pinion".into(),
                color: 0,
                body: pin_body.then_some(usize::from(fg_body)),
                frame: place(pin_frame),
                pitch_diameter: 2.0 * r1,
                parts: pp,
            });
            b.out.members.push(Member {
                name: "face gear".into(),
                color: 1,
                body: fg_body.then_some(0),
                frame: place(at([0.0; 3])),
                pitch_diameter: m * f64::from(s.teeth2),
                parts: fg,
            });
            b.diameter(0, "pinion pitch diameter", 2.0 * r1, len, Some(Field::PitchDiameter));
            b.shafts(0, 1, 90.0, None, DimKind::Offset);
            let c = b.out.members[1].world([0.0, 0.0, 0.0]);
            let p = b.out.members[1].world([m * f64::from(s.teeth2) / 2.0, 0.0, 0.0]);
            let ax = v_add(c, b.out.members[1].frame.z);
            b.dim(
                DimKind::PitchDiameter,
                "face gear pitch diameter (m·z₂)",
                m * f64::from(s.teeth2),
                Unit::Mm,
                vec![1],
                vec![c, p, ax],
                Some(Field::PitchDiameter2),
            );
        }
        Kind::Elliptical => {
            let (a, sa, sb, curve2) = elliptic_full(s.teeth, s.ellipse_eccentricity);
            let m = s.module;
            let e = s.ellipse_eccentricity;
            let z = s.face_width;
            let c1: Vec<[f64; 3]> = (0..120)
                .map(|i| {
                    let t = TAU * f64::from(i) / 120.0;
                    let r = sb * m / (1.0 - e * e * t.cos().powi(2)).max(1e-12).sqrt();
                    [r * t.cos(), r * t.sin(), z]
                })
                .collect();
            let c2: Vec<[f64; 3]> = curve2.iter().map(|q| [q[0] * m, q[1] * m, z]).collect();
            let mut p1 = vec![Part { role: Role::Pitch, shape: Shape::Path { points: c1, closed: true } }, cyl(Role::Body, sb * m, 0.0, z)];
            extras(s, false, 0.0, z, &mut p1);
            let mut p2 = vec![Part { role: Role::Pitch, shape: Shape::Path { points: c2, closed: true } }, cyl(Role::Body, (a - sa) * m, 0.0, z)];
            extras(s, true, 0.0, z, &mut p2);
            let i = b.add("gear 1", 1, at([0.0; 3]), 2.0 * sb * m, p1);
            let j = b.add("gear 2", 2, at([a * m, 0.0, 0.0]), 2.0 * (a - sa) * m, p2);
            b.center(i, j, a * m, z / 2.0, Some(Field::CenterDistance));
        }
    }
    b.out
}

fn face_dim(b: &mut Build, i: usize, face: f64, r: f64) {
    let m = &b.out.members[i];
    let pts = vec![m.world([r, 0.0, 0.0]), m.world([r, 0.0, face])];
    b.dim(DimKind::FaceWidth, "face width", face, Unit::Mm, vec![i], pts, Some(Field::FaceWidth));
}

fn bevel_schematic(b: &mut Build) {
    let s = b.s;
    let o = s.origin;
    let sigma = s.shaft_angle.to_radians();
    let (z1, z2) = (f64::from(s.teeth), f64::from(s.teeth2));
    let d1 = sigma.sin().atan2(z2 / z1 + sigma.cos());
    let d2 = sigma - d1;
    let m = s.module;
    let (r1, r2) = (m * z1 / 2.0, m * z2 / 2.0);
    let re = r1 / d1.sin();
    let (h1, h2) = (re * d1.cos(), re * d2.cos());
    // Each member's teeth run from its toe to its heel along the pitch cone (gear 2's own face
    // width and heel offset, as the plan builds them).
    let faces = crate::plan::faces_of(s, re);
    let scale = faces[0].toe() / re;
    let spiral = if s.kind == Kind::SpiralBevel { s.spiral_angle.to_radians() } else { 0.0 };
    let mn = m * spiral.cos();
    let member = |r: f64, h: f64, delta: f64, sign: f64, face: crate::plan::ConeFace| -> Vec<Part> {
        let shift = sign * s.shift;
        let (ha, hf) = ((s.rack.addendum + shift) * mn, (s.rack.dedendum - shift) * mn);
        // The heel and the toe along the axis (the large end's pitch circle, at the cone
        // distance, on z = 0).
        let (z0, z1) = (h * (1.0 - face.heel / re), h * (1.0 - face.toe() / re));
        let cone = |rr: f64, role: Role| {
            let apex = rr / delta.tan();
            let at = |z: f64| (rr - z * delta.tan()).max(0.0);
            Part { role, shape: Shape::Cone { r0: at(z0), z0, r1: at(z1), z1, apex_z: apex.min(1e9), half_angle: delta.to_degrees() } }
        };
        vec![cone(r, Role::Pitch), cone(r + ha.max(0.0) * delta.cos(), Role::Tip), cone((r - hf * delta.cos()).max(0.0), Role::Root)]
    };
    let mut p1 = member(r1, h1, d1, 1.0, faces[0]);
    let mut p2 = member(r2, h2, d2, -1.0, faces[1]);
    extras(s, false, 0.0, 0.0, &mut p1);
    extras(s, true, 0.0, 0.0, &mut p2);
    for p in p1.iter_mut().chain(p2.iter_mut()) {
        // A bevel's bore runs through to the small end; its hub sits behind the large end.
        if let Shape::Cylinder { z0, z1, .. } = &mut p.shape
            && p.role == Role::Bore
        {
            *z1 = *z0 + h1.max(h2) * (1.0 - scale);
        }
    }
    let apex = [o[0], o[1], o[2] + h1];
    let ax2 = [-sigma.sin(), 0.0, sigma.cos()];
    let origin2 = v_sub(apex, v_mul(ax2, h2));
    let frame2 = Frame { origin: origin2, x: [sigma.cos(), 0.0, sigma.sin()], z: ax2 };
    let i = b.add("gear 1", 1, Frame::at(o), 2.0 * r1, p1);
    let j = b.add("gear 2", 2, frame2, 2.0 * r2, p2);
    let apex_w = b.out.members[i].world([0.0, 0.0, h1]);
    let large1 = b.out.members[i].world([r1, 0.0, 0.0]);
    let reach = re;
    let arm1 = v_sub(b.out.members[i].world([0.0; 3]), apex_w);
    let arm2 = v_sub(b.out.members[j].world([0.0; 3]), apex_w);
    let norm = |v: [f64; 3]| v_mul(v, reach / v_dot(v, v).sqrt().max(1e-12));
    b.dim(
        DimKind::ShaftAngle,
        "shaft angle",
        s.shaft_angle,
        Unit::Deg,
        vec![i, j],
        vec![apex_w, v_add(apex_w, norm(arm1)), v_add(apex_w, norm(arm2))],
        Some(Field::ShaftAngle),
    );
    b.dim(DimKind::ConeDistance, "cone distance", re, Unit::Mm, vec![i, j], vec![apex_w, large1], Some(Field::ConeDistance));
    b.diameter(i, "pinion outer pitch diameter", 2.0 * r1, 0.0, Some(Field::PitchDiameter));
    b.diameter(j, "gear outer pitch diameter", 2.0 * r2, 0.0, Some(Field::PitchDiameter2));
}

/// A bevel planetary set: the sun, every planet and the second sun or ring, on the frames
/// `plan()` gives them (the apex at the spec's origin, the sun below it on −z).
fn bevel_planet_schematic(b: &mut Build) {
    let s = b.s;
    let g = match bevel_planet::geometry(s) {
        Ok(g) => g,
        Err(e) => {
            b.out.note = Some(e);
            return;
        }
    };
    let o = s.origin;
    let m = s.module;
    let re = g.re;
    let scale = (re - g.face) / re;
    let mn = m * g.spiral.cos();
    let (ha, hf) = (s.rack.addendum * mn, s.rack.dedendum * mn);
    // Pitch, tip and root cones from the large end (z = 0) towards the apex (+z); a ring's tip
    // is inside its pitch cone and its root outside.
    // The sun, sun 2 and the ring share gear 1's face; the planets have their own (face_width2,
    // heel face_offset2 in from the cone distance).
    let faces = crate::plan::faces_of(s, re);
    let cones_on = |delta: f64, internal: bool, face: crate::plan::ConeFace| -> Vec<Part> {
        let h = re * delta.cos();
        let r = re * delta.sin();
        let (z0, z1) = (h * (1.0 - face.heel / re), h * (1.0 - face.toe() / re));
        let cone = |rr: f64, role: Role| {
            let at = |z: f64| rr - z * delta.tan();
            Part { role, shape: Shape::Cone { r0: at(z0), z0, r1: at(z1), z1, apex_z: (rr / delta.tan()).min(1e9), half_angle: delta.to_degrees() } }
        };
        let (tip, root) =
            if internal { (r - ha * delta.cos(), r + hf * delta.cos()) } else { (r + ha * delta.cos(), (r - hf * delta.cos()).max(0.0)) };
        vec![cone(r, Role::Pitch), cone(tip.max(0.0), Role::Tip), cone(root, Role::Root)]
    };
    let cones = |delta: f64, internal: bool| cones_on(delta, internal, faces[0]);
    let bored = |parts: &mut Vec<Part>, bore: &Bore, delta: f64| {
        if bore.kind != BoreKind::None && bore.diameter > 0.0 {
            parts.push(cyl(Role::Bore, bore.diameter / 2.0, 0.0, re * delta.cos() * (1.0 - scale)));
        }
    };
    let (sigma, h_sun, h_planet) = (g.sigma, re * g.delta_sun.cos(), re * g.delta_planet.cos());
    let mut ps = cones(g.delta_sun, false);
    bored(&mut ps, &s.bore, g.delta_sun);
    let sun = b.add("sun", 1, Frame { origin: [o[0], o[1], o[2] - h_sun], x: [1.0, 0.0, 0.0], z: [0.0, 0.0, 1.0] }, m * f64::from(g.z_sun), ps);
    let rot = |v: [f64; 3], t: f64| {
        let (sn, cs) = t.sin_cos();
        [v[0] * cs - v[1] * sn, v[0] * sn + v[1] * cs, v[2]]
    };
    let mut first = None;
    for (k, th) in g.azimuths.iter().enumerate() {
        let ax = rot([-sigma.sin(), 0.0, sigma.cos()], *th);
        let frame = Frame { origin: [0, 1, 2].map(|i| o[i] - ax[i] * h_planet), x: rot([sigma.cos(), 0.0, sigma.sin()], *th), z: ax };
        let mut pp = cones_on(g.delta_planet, false, faces[1]);
        bored(&mut pp, &s.bore2, g.delta_planet);
        let j = b.add(&format!("planet {}", k + 1), 2, frame, m * f64::from(g.z_planet), pp);
        // The colour (and preview place) of the one planet the preview shows.
        b.out.members[j].color = 1;
        first.get_or_insert(j);
    }
    let far = match g.mode {
        BevelPlanetMode::Single => None,
        BevelPlanetMode::Differential => {
            let d = PI - g.delta_far;
            let mut pf = cones(d, false);
            bored(&mut pf, &s.bore, d);
            Some(b.add(
                "sun 2",
                3,
                Frame { origin: [o[0], o[1], o[2] + re * d.cos()], x: [1.0, 0.0, 0.0], z: [0.0, 0.0, -1.0] },
                m * f64::from(g.z_far),
                pf,
            ))
        }
        BevelPlanetMode::Ring => {
            let d = g.delta_far;
            let mut pr = cones(d, true);
            let rim = if s.rim > 0.0 { s.rim } else { 3.0 * m };
            let outer = re * d.sin() + hf * d.cos() + rim;
            pr.push(Part {
                role: Role::Body,
                shape: Shape::Tube { r_in: re * d.sin() * scale, r_out: outer, z0: 0.0, z1: re * d.cos() * (1.0 - scale) },
            });
            Some(b.add(
                "ring",
                3,
                Frame { origin: [o[0], o[1], o[2] - re * d.cos()], x: [1.0, 0.0, 0.0], z: [0.0, 0.0, 1.0] },
                m * f64::from(g.z_far),
                pr,
            ))
        }
    };
    if let Some(f) = far {
        b.out.members[f].color = 2;
    }
    let apex_w = b.out.members[sun].world([0.0, 0.0, h_sun]);
    let norm = |v: [f64; 3]| v_mul(v, re / v_dot(v, v).sqrt().max(1e-12));
    if let Some(j) = first {
        let arm1 = v_sub(b.out.members[sun].world([0.0; 3]), apex_w);
        let arm2 = v_sub(b.out.members[j].world([0.0; 3]), apex_w);
        b.dim(
            DimKind::ShaftAngle,
            "cone angle",
            sigma.to_degrees(),
            Unit::Deg,
            vec![sun, j],
            vec![apex_w, v_add(apex_w, norm(arm1)), v_add(apex_w, norm(arm2))],
            Some(Field::ShaftAngle),
        );
        b.diameter(j, "planet outer pitch diameter", m * f64::from(g.z_planet), 0.0, Some(Field::PitchDiameter2));
    }
    let large = b.out.members[sun].world([re * g.delta_sun.sin(), 0.0, 0.0]);
    b.dim(DimKind::ConeDistance, "cone distance", re, Unit::Mm, vec![sun], vec![apex_w, large], Some(Field::ConeDistance));
    b.diameter(sun, "sun outer pitch diameter", m * f64::from(g.z_sun), 0.0, Some(Field::PitchDiameter));
    if let Some(f) = far {
        let label = if g.mode == BevelPlanetMode::Ring { "ring outer pitch diameter" } else { "sun 2 outer pitch diameter" };
        b.diameter(f, label, m * f64::from(g.z_far), 0.0, None);
    }
}

thread_local! {
    static HYPOID: std::cell::RefCell<Vec<(Spec, Result<HypoidParts, String>)>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[derive(Clone)]
struct HypoidParts {
    pinion: Frame,
    gear: Frame,
    pinion_parts: Vec<Part>,
    gear_parts: Vec<Part>,
    rm1: f64,
    re2: f64,
    feet: ([f64; 3], [f64; 3]),
}

/// The hypoid's layout as body-frame parts (cached on the spec's numbers that shape it).
fn hypoid_parts(s: &Spec) -> Result<HypoidParts, String> {
    let key = Spec { only: 0, placement: None, name: String::new(), bore: Bore::default(), hub_diameter: 0.0, hub_length: 0.0, ..s.clone() };
    if let Some(v) = HYPOID.with(|c| c.borrow().iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone())) {
        return v;
    }
    let v = (|| -> Result<HypoidParts, String> {
        crate::plan::check(s)?;
        let l = crate::hypoid::layout(s)?;
        let d = &l.d;
        let sub = v_sub;
        // The gear: its pitch cone over the face, in its body frame (z up from the back face).
        let (sd2, cd2) = d.delta2.sin_cos();
        let apex2_z = d.apex2[2] - l.back;
        let gear_cone = |c: f64| [c * sd2, apex2_z - c * cd2];
        let (c_in, c_out) = (d.cone_m2 - d.b2 / 2.0, d.cone_m2 + d.b2 / 2.0);
        let (gi, go) = (gear_cone(c_in), gear_cone(c_out));
        let gear_parts = vec![
            Part {
                role: Role::Pitch,
                shape: Shape::Cone { r0: go[0], z0: go[1], r1: gi[0], z1: gi[1], apex_z: apex2_z, half_angle: d.delta2.to_degrees() },
            },
            Part { role: Role::Tip, shape: Shape::Cylinder { r: l.r_out, z0: l.z_hi - l.back - (d.ha2 + d.hf2), z1: l.z_hi - l.back } },
            Part { role: Role::Body, shape: Shape::Cylinder { r: gi[0], z0: 0.0, z1: (l.z_hi - l.back - (d.ha2 + d.hf2)).max(0.0) } },
        ];
        // The pinion: its pitch cone about its own axis over its toothed length (the hyperboloid
        // of a hypoid's true pitch surface, approximated by the cone through the mean point).
        let world = |q: [f64; 3]| [q[0] + s.origin[0], q[1] + s.origin[1], q[2] - l.back + s.origin[2]];
        let pz = l.pinion_z;
        let apex1 = v_dot(sub(world(d.apex1), l.pinion_origin), pz);
        let t1 = d.delta1.tan();
        // (Between its heel and toe cones.)
        let (z_heel, z_toe) = (l.pinion_heel[1], l.pinion_toe[1]);
        let profile: Vec<[f64; 2]> =
            (0..=8).map(|k| z_heel + (z_toe - z_heel) * f64::from(k) / 8.0).map(|z| [((apex1 - z).abs() * t1).max(0.0), z]).collect();
        let pinion_parts = vec![
            Part { role: Role::Pitch, shape: Shape::Revolved { profile } },
            Part {
                role: Role::Shaft,
                shape: Shape::Cylinder {
                    r: 0.6 * ((apex1 - l.length).abs().min(apex1.abs()) * t1 - d.hf1).max(0.3 * d.rm1),
                    z0: -0.8 * l.length,
                    z1: 0.0,
                },
            },
        ];
        let mut pinion = Frame { origin: l.pinion_origin, x: l.pinion_x, z: l.pinion_z };
        let mut gear = Frame { origin: l.gear_origin, x: [1.0, 0.0, 0.0], z: [0.0, 0.0, 1.0] };
        let (mut f1, mut f2) = (world(d.o1), world([0.0, 0.0, d.o1[2]]));
        if !s.left_hand {
            let y0 = s.origin[1];
            let m = |v: [f64; 3]| [v[0], -v[1], v[2]];
            let mp = |v: [f64; 3]| [v[0], 2.0 * y0 - v[1], v[2]];
            pinion = Frame { origin: mp(pinion.origin), x: m(pinion.x), z: m(pinion.z) };
            gear = Frame { origin: mp(gear.origin), x: m(gear.x), z: m(gear.z) };
            (f1, f2) = (mp(f1), mp(f2));
        }
        // (The parts are symmetric about their axes, so the mirror leaves them as they are.)
        Ok(HypoidParts { pinion, gear, pinion_parts, gear_parts, rm1: d.rm1, re2: d.re2, feet: (f1, f2) })
    })();
    HYPOID.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 8 {
            c.drain(..1);
        }
        c.push((key, v.clone()));
    });
    v
}

fn hypoid_schematic(b: &mut Build) {
    let s = b.s;
    match hypoid_parts(s) {
        Ok(h) => {
            let i = b.add("pinion", 1, h.pinion, 2.0 * h.rm1, h.pinion_parts.clone());
            let j = b.add("gear", 2, h.gear, 2.0 * h.re2, h.gear_parts.clone());
            let place = |p: [f64; 3]| match &s.placement {
                Some(f) => f.world(p),
                None => p,
            };
            let (f1, f2) = (place(h.feet.0), place(h.feet.1));
            let reach = h.re2;
            let (d1, d2) = (b.out.members[i].frame.z, b.out.members[j].frame.z);
            let toward = |f: [f64; 3], p: [f64; 3], d: [f64; 3]| if v_dot(v_sub(p, f), d) >= 0.0 { d } else { v_mul(d, -1.0) };
            let (t1, t2) = (toward(f1, b.out.members[i].frame.origin, d1), toward(f2, b.out.members[j].frame.origin, d2));
            b.dim(
                DimKind::ShaftAngle,
                "shaft angle",
                s.shaft_angle,
                Unit::Deg,
                vec![i, j],
                vec![f1, v_add(f1, v_mul(t1, reach)), v_add(f1, v_mul(t2, reach))],
                Some(Field::ShaftAngle),
            );
            b.dim(DimKind::Offset, "offset", s.offset, Unit::Mm, vec![i, j], vec![f1, f2], Some(Field::Offset));
            let z = match h.gear_parts[0].shape {
                Shape::Cone { z0, .. } => z0,
                _ => 0.0,
            };
            b.diameter(j, "ring gear outer pitch diameter", 2.0 * h.re2, z, Some(Field::PitchDiameter2));
        }
        Err(e) => {
            // The axes as the spec gives them, without the cones: the ring gear on z, the pinion
            // across it at the shaft angle, the offset below.
            let sigma = s.shaft_angle.to_radians();
            let o = s.origin;
            let ring = s.module * f64::from(s.teeth2);
            let side = if s.left_hand { -1.0 } else { 1.0 };
            let i = b.add(
                "pinion",
                1,
                Frame { origin: [o[0], o[1] + side * s.offset, o[2] + 0.3 * ring], x: [0.0, 0.0, 1.0], z: [sigma.sin(), 0.0, sigma.cos()] },
                s.module * f64::from(s.teeth),
                vec![],
            );
            let j = b.add("gear", 2, Frame::at(o), ring, vec![cyl(Role::Pitch, ring / 2.0, 0.0, s.face_width)]);
            b.shafts(i, j, s.shaft_angle, Some(Field::Offset), DimKind::Offset);
            b.out.note = Some(e);
        }
    }
}

fn worm_schematic(b: &mut Build) {
    let s = b.s;
    let o = s.origin;
    let m = s.module;
    let z1 = s.starts.max(1);
    let zw = s.teeth;
    let d1 = if s.worm_diameter > 0.0 { s.worm_diameter } else { 10.0 * m };
    let lead = PI * m * f64::from(z1);
    let gamma = (lead / (PI * d1)).atan();
    let ax = (s.pressure_angle.to_radians().tan() / gamma.cos()).atan();
    let (ha, hf) = (s.rack.addendum * m, s.rack.dedendum * m);
    let clearance = hf - ha;
    let (r1, ra1, rf1) = (d1 / 2.0, d1 / 2.0 + ha, d1 / 2.0 - hf);
    let r2 = m * f64::from(zw) / 2.0;
    let a = r1 + r2 + s.shift * m;
    let throat = a - rf1 - clearance;
    let outside = throat + 0.5 * m;
    // The wheel's face, as the plan narrows it.
    let lean_limit = {
        let ok = |th: f64| ax.tan() * th.cos() - gamma.tan() * th.sin() >= 6f64.to_radians().tan();
        let (mut lo, mut hi) = (0.0, FRAC_PI_2);
        for _ in 0..50 {
            let mid = 0.5 * (lo + hi);
            if ok(mid) { lo = mid } else { hi = mid }
        }
        2.0 * r1 * lo.sin()
    };
    let face = s.face_width.min((1.8 * rf1).min(lean_limit));
    let length =
        if s.worm_length > 0.0 { s.worm_length } else { (2.0 * (outside.powi(2) - (r2 - m).powi(2)).max(0.0).sqrt() + lead).max(4.0 * PI * m) };
    // The wheel's throat: its tips follow the worm's root round.
    let throat_at = |w: f64| a - ((rf1 + clearance).powi(2) - w * w).max(0.0).sqrt();
    let profile: Vec<[f64; 2]> = (0..=10).map(|k| -face / 2.0 + face * f64::from(k) / 10.0).map(|w| [throat_at(w).min(outside), w]).collect();
    let mut pw = vec![cyl(Role::Pitch, r2, -face / 2.0, face / 2.0), Part { role: Role::Tip, shape: Shape::Revolved { profile } }];
    extras(s, false, -face / 2.0, face / 2.0, &mut pw);
    let left = s.helix < 0.0;
    let hand = if left { -1.0 } else { 1.0 };
    let mut pworm = vec![
        cyl(Role::Pitch, r1, -length / 2.0, length / 2.0),
        cyl(Role::Tip, ra1, -length / 2.0, length / 2.0),
        cyl(Role::Root, rf1, -length / 2.0, length / 2.0),
    ];
    let turns = length / lead;
    let steps = ((turns * 24.0).ceil() as usize).clamp(8, 2000);
    for k in 0..z1 {
        let pts: Vec<[f64; 3]> = (0..=steps)
            .map(|i| {
                let z = -length / 2.0 + length * i as f64 / steps as f64;
                let t = hand * TAU * (z / lead) + TAU * f64::from(k) / f64::from(z1);
                [r1 * t.cos(), r1 * t.sin(), z]
            })
            .collect();
        pworm.push(Part { role: Role::Thread, shape: Shape::Path { points: pts, closed: false } });
    }
    extras(s, true, -length / 2.0, length / 2.0, &mut pworm);
    let i = b.add("wheel", 1, Frame::at(o), 2.0 * r2, pw);
    let j = b.add("worm", 2, Frame { origin: [o[0] + a, o[1], o[2]], x: [-1.0, 0.0, 0.0], z: [0.0, 1.0, 0.0] }, d1, pworm);
    b.shafts(i, j, 90.0, Some(Field::CenterDistance), DimKind::CenterDistance);
    b.diameter(i, "wheel pitch diameter", 2.0 * r2, 0.0, Some(Field::PitchDiameter2));
    b.diameter(j, "worm pitch diameter", d1, length / 2.0, Some(Field::PitchDiameter));
}

fn wrapped_schematic(b: &mut Build) {
    let s = b.s;
    let o = s.origin;
    let chain = s.kind == Kind::Sprocket;
    let (pitch, width, tip, dia): (f64, f64, Box<dyn Fn(u32) -> f64>, Box<dyn Fn(u32) -> f64>) = if chain {
        let (_, pitch, roller, b1, pt) = crate::plan::extras::chain_of(s);
        let strands = s.strands.clamp(1, 4);
        let bf1 = crate::sprocket::tooth_width(pitch, b1, strands);
        let width = f64::from(strands - 1) * pt + bf1;
        (
            pitch,
            width,
            Box::new(move |z| crate::sprocket::Sprocket { teeth: z, pitch, roller }.tip_diameter()),
            Box::new(move |z| crate::sprocket::Sprocket { teeth: z, pitch, roller }.pitch_diameter()),
        )
    } else {
        let belt = crate::plan::extras::belt_of(s);
        let tooth_len = s.face_width + (0.1 * s.face_width).max(0.5);
        let tf = if s.flanges { (0.4 * belt.pitch).max(1.0) } else { 0.0 };
        (
            belt.pitch,
            tooth_len + 2.0 * tf,
            Box::new(move |z| crate::pulley::Pulley { belt, teeth: z }.outside_diameter()),
            Box::new(move |z| crate::pulley::Pulley { belt, teeth: z }.pitch_diameter()),
        )
    };
    let _ = pitch;
    let who = if chain { "sprocket" } else { "pulley" };
    let member = |z: u32, second: bool| {
        let mut p = vec![cyl(Role::Pitch, dia(z) / 2.0, 0.0, width), cyl(Role::Tip, tip(z) / 2.0, 0.0, width)];
        extras(s, second, 0.0, width, &mut p);
        p
    };
    let wrap_name = if chain { "chain" } else { "belt" };
    if s.teeth2 == 0 {
        let r1 = dia(s.teeth) / 2.0;
        let i = b.add(who, 1, Frame::at(o), 2.0 * r1, member(s.teeth, false));
        if chain {
            // A strand lying on the upper half.
            let pts: Vec<[f64; 3]> = (0..=24).map(|k| PI * f64::from(k) / 24.0).map(|t| [r1 * t.cos(), r1 * t.sin(), width / 2.0]).collect();
            let pl = Frame::at(o);
            let place = |f: Frame| match &s.placement {
                Some(p) => p.compose(&f),
                None => f,
            };
            let color = b.out.members.len();
            b.out.members.push(Member {
                name: "chain".into(),
                color,
                body: None,
                frame: place(pl),
                pitch_diameter: 0.0,
                parts: vec![Part { role: Role::Wrap, shape: Shape::Path { points: pts, closed: false } }],
            });
        }
        b.diameter(i, "pitch diameter", 2.0 * r1, width, Some(Field::PitchDiameter));
        return;
    }
    let a = wrap_center(s).unwrap_or(s.center_distance);
    let (r1, r2) = (dia(s.teeth) / 2.0, dia(s.teeth2) / 2.0);
    let path: Vec<[f64; 3]> = crate::wrap::belt_path(r1, r2, a, 0.0)
        .map(|ol| ol.polyline(0.02 * (r1 + r2)).into_iter().map(|q| [q.x, q.y, width / 2.0]).collect())
        .unwrap_or_default();
    let place = |f: Frame| match &s.placement {
        Some(p) => p.compose(&f),
        None => f,
    };
    b.out.members.push(Member {
        name: wrap_name.into(),
        color: 0,
        body: None,
        frame: place(Frame::at(o)),
        pitch_diameter: 0.0,
        parts: vec![Part { role: Role::Wrap, shape: Shape::Path { points: path, closed: true } }],
    });
    let i = b.add(&format!("{who} 1"), 1, Frame::at(o), 2.0 * r1, member(s.teeth, false));
    let j = b.add(&format!("{who} 2"), 2, Frame::at([o[0] + a, o[1], o[2]]), 2.0 * r2, member(s.teeth2, true));
    b.center(i, j, a, width / 2.0, Some(Field::CenterDistance));
    b.diameter(i, &format!("{who} 1 pitch diameter"), 2.0 * r1, width, Some(Field::PitchDiameter));
    b.diameter(j, &format!("{who} 2 pitch diameter"), 2.0 * r2, width, Some(Field::PitchDiameter2));
}

/// The schematic of what a [`Simple`] solves to; when it can't be solved, the bare shafts (two
/// axis lines with their angle and offset) so the drawing still shows what was dimensioned.
pub fn schematic_of(simple: &Simple, base: &Spec) -> (Solved, Schematic) {
    let solved = solve(simple, base);
    let sch = if solved.error.is_none() { schematic(&solved.spec) } else { shafts_only(simple, solved.error.clone()) };
    (solved, sch)
}

/// A gear train: each gear's pitch cylinder (a ring's rim round it) on its own shaft, at its
/// centre and layer, in the order of the train's gears (its bodies').
fn train_schematic(b: &mut Build, t: &crate::plan::Train) {
    let s = b.s;
    let o = s.origin;
    for (i, g) in t.gears.iter().enumerate() {
        let r = g.module * f64::from(g.teeth) / 2.0 / g.helix.to_radians().cos().max(1e-6);
        let (z0, z1) = (g.z, g.z + s.face_width);
        let mut parts = vec![cyl(Role::Pitch, r, z0, z1)];
        if g.ring {
            let rim = if s.rim > 0.0 { s.rim } else { 3.0 * g.module };
            parts.push(Part { role: Role::Body, shape: Shape::Tube { r_in: r, r_out: r + 1.25 * g.module + rim, z0, z1 } });
        }
        let member = u8::try_from(i + 1).unwrap_or(u8::MAX);
        b.add(&format!("gear {}", i + 1), member, Frame::at([o[0] + g.center.x, o[1] + g.center.y, o[2]]), 2.0 * r, parts);
    }
}

/// Two shafts and their relation, without gears.
pub fn shafts_only(simple: &Simple, note: Option<String>) -> Schematic {
    let angle = simple.shaft_angle.unwrap_or(0.0);
    let e = simple.offset.or(simple.center_distance).unwrap_or(0.0);
    let sg = angle.to_radians();
    let mut out = Schematic { note, ..Schematic::default() };
    let f1 = Frame::at([0.0; 3]);
    let f2 = Frame { origin: [e, 0.0, 0.0], x: [-1.0, 0.0, 0.0], z: [0.0, sg.sin(), sg.cos()] };
    for (k, f) in [f1, f2].into_iter().enumerate() {
        out.members.push(Member { name: format!("shaft {}", k + 1), color: k, body: None, frame: f, pitch_diameter: 0.0, parts: vec![] });
    }
    let reach = e.max(50.0);
    if sg.sin().abs() > 1e-6 {
        out.dims.push(Dim {
            kind: DimKind::ShaftAngle,
            label: "shaft angle".into(),
            value: angle,
            unit: Unit::Deg,
            members: vec![0, 1],
            points: vec![[0.0; 3], [0.0, 0.0, reach], [0.0, reach * sg.sin(), reach * sg.cos()]],
            field: Some(Field::ShaftAngle),
        });
    }
    if e > 1e-6 {
        out.dims.push(Dim {
            kind: DimKind::Offset,
            label: if sg.sin().abs() > 1e-6 { "offset".into() } else { "centre distance".into() },
            value: e,
            unit: Unit::Mm,
            members: vec![0, 1],
            points: vec![[0.0; 3], [e, 0.0, 0.0]],
            field: Some(Field::Offset),
        });
    }
    out
}

#[cfg(test)]
#[path = "simple_tests.rs"]
mod tests;
