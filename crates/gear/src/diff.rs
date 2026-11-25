//! Differentials: the gear set, the case (diff carrier) that holds it, and the case's drive gear,
//! planned as one assembly ([`Kind::Differential`], settings in [`Diff`] on `Spec::diff`).
//!
//! # Styles
//!
//! * **Open bevel** ([`Style::Open`]): two equal bevel side gears and 2 to 6 spider pinions on
//!   pins square to the axle (a cross shaft for two). The side gears share the case's torque
//!   equally; with the case held they turn opposite ways at the same speed.
//! * **Torque-biasing bevel** ([`Style::Biased`]): two side gears with different tooth counts
//!   meshing the same pinions. A pinion meshes both on its one pitch cone, so its axis tilts to
//!   the axle: cos Σ = (z_B − z_A) / (2·z_p) (the bevel planetary set of
//!   [`crate::plan::bevel_planet`], built in its differential mode with `teeth3` = z_B). Every
//!   pinion is a lever with the same pitch radius at both meshes, so its two tooth forces are
//!   equal, and each side gear's torque is that force times its own pitch radius:
//!   T_A : T_B = z_A : z_B, always, whatever the speeds (a designed torque split, like an
//!   asymmetric centre differential, not a friction bias). Speeds:
//!   z_A·n_A + z_B·n_B = (z_A + z_B)·n_case.
//! * **Torsen Type B** ([`Style::TorsenB`], the T‑2): parallel-axis helical planetary. Two helical
//!   side gears of opposite hands; element gears in pairs, all parallel to the axle in pockets in
//!   the case. Element 1 of a pair meshes side gear A, element 2 side gear B, and the two mesh
//!   each other over the stretch between the side gears. The torque bias comes from friction
//!   where the helix and tooth loads press the elements into their pockets and the side gears
//!   onto their thrust faces; [`torsen_b_bias`] estimates it from the helix and pressure angles
//!   and an assumed friction coefficient (an estimate: real values depend on finish, lubricant and
//!   load).
//! * **Torsen Type A** ([`Style::TorsenA`], the T‑1, worm/invex): side gears are worms (high
//!   helix), element gears are worm wheels across the axle with spur gears on both ends meshing
//!   the paired element's. Built as crossed helical pairs at 90° (the crossed helical planner) with
//!   spur gears on each element's ends and integral journals turning in holes in the case; the lead
//!   angle and bias estimate are in [`torsen_a_layout`].
//!
//! # The case
//!
//! Plain stock, keeping all the material it can (the maker adds features in the CAD): one body
//! turning about the axle (z; the set's origin at the side gears' apex or the middle of a Torsen
//! set), a revolve of a cylinder round the gears with a plain cylindrical journal at each end for
//! the bearings, out of which only what the gears need to turn is taken: the side gears' turning
//! envelope (hubs included) and the axle bore, a pocket for each pinion or element (its own
//! envelope about its axis), and the pin bores. Every envelope is worked out from the planned
//! members' material, grown by the running clearance. Opt-in: assembly windows (`windows`) and a
//! bolted ring (`ring_bolts`: flange, through holes, tapped holes in the ring).
//!//! # The drive
//!
//! A ring gear on the case driven by a pinion: spiral or straight bevel and hypoid (axle drives)
//! or helical (transaxles), planned by the existing planners (`plan()` of a
//! [`Kind::SpiralBevel`], [`Kind::Bevel`], [`Kind::Hypoid`] or [`Kind::Pair`] spec) and moved so
//! the ring turns on the axle: a separate plain ring fitted on the case's cylinder (with
//! ring_bolts, on a flange with bolts) or integral (the ring's teeth joined to the case).
//!
//! References: Shigley's Mechanical Engineering Design (gear forces, bevel geometry), Dudley's
//! Handbook of Practical Gear Design (differentials), ISO 23509 (bevel and hypoid geometry, through
//! the existing planners), Gleason/Torsen technical notes on Torsen T‑1 and T‑2 (layout and the
//! friction origin of torque bias), Chocholek, "The development of a differential for the
//! improvement of traction control" (IMechE 1988, the Torsen T‑1 worm differential).

use std::f64::consts::{PI, TAU};

use crate::axes::{Axis, dot, len as norm, num, sub};
use crate::geom::{Outline, Region, Seg, V2};
use crate::hub::{Bore, BoreKind};
use crate::involute::{Involute, external_outline, mesh_external, mesh_phase};
use crate::mesh3d::{MeshPair3, Solid};
use crate::plan::{Assembly, AssemblyLink, AssemblyMember, BevelPlanetMode, Body, Fact, Frame, Kind, Op, Plan, PreviewGear, Spec, Step, Unit};

/// The differential's style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum Style {
    #[default]
    Open,
    Biased,
    TorsenB,
    TorsenA,
}

impl Style {
    pub const ALL: [Style; 4] = [Style::Open, Style::Biased, Style::TorsenB, Style::TorsenA];
    pub fn name(self) -> &'static str {
        match self {
            Style::Open => "open",
            Style::Biased => "biased",
            Style::TorsenB => "torsen_b",
            Style::TorsenA => "torsen_a",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Style::Open => "Open bevel",
            Style::Biased => "Torque-biasing bevel (uneven side gears)",
            Style::TorsenB => "Torsen Type B (helical planetary)",
            Style::TorsenA => "Torsen Type A (worm)",
        }
    }
    pub fn parse(s: &str) -> Option<Style> {
        Style::ALL.iter().copied().find(|k| k.name() == s)
    }
    /// A bevel style (side gears and spider pinions meeting at one apex).
    pub fn bevel(self) -> bool {
        matches!(self, Style::Open | Style::Biased)
    }
}

/// The gear that drives the case.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum DriveKind {
    /// No drive gear (the case is driven some other way).
    None,
    #[default]
    SpiralBevel,
    /// Straight bevel ring and pinion.
    Bevel,
    Hypoid,
    /// Helical ring and pinion on parallel shafts (a transaxle).
    Helical,
}

impl DriveKind {
    pub const ALL: [DriveKind; 5] = [DriveKind::None, DriveKind::SpiralBevel, DriveKind::Bevel, DriveKind::Hypoid, DriveKind::Helical];
    pub fn name(self) -> &'static str {
        match self {
            DriveKind::None => "none",
            DriveKind::SpiralBevel => "spiral_bevel",
            DriveKind::Bevel => "bevel",
            DriveKind::Hypoid => "hypoid",
            DriveKind::Helical => "helical",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            DriveKind::None => "None",
            DriveKind::SpiralBevel => "Spiral bevel ring and pinion",
            DriveKind::Bevel => "Straight bevel ring and pinion",
            DriveKind::Hypoid => "Hypoid ring and pinion (offset)",
            DriveKind::Helical => "Helical ring and pinion (transaxle)",
        }
    }
    pub fn parse(s: &str) -> Option<DriveKind> {
        DriveKind::ALL.iter().copied().find(|k| k.name() == s)
    }
}

/// How the ring gear is held on the case.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum Mount {
    /// A separate ring on the case (plainly fitted; with ring_bolts, bolted to a flange).
    #[default]
    Bolted,
    /// The ring's teeth are part of the case.
    Integral,
}

impl Mount {
    pub const ALL: [Mount; 2] = [Mount::Bolted, Mount::Integral];
    pub fn name(self) -> &'static str {
        match self {
            Mount::Bolted => "bolted",
            Mount::Integral => "integral",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Mount::Bolted => "Separate ring on the case",
            Mount::Integral => "Integral with the case",
        }
    }
    pub fn parse(s: &str) -> Option<Mount> {
        Mount::ALL.iter().copied().find(|k| k.name() == s)
    }
}

/// Everything about a differential beyond `Spec`'s own `name`, `origin`, `placement`, `load` and
/// `clearance_body` (lengths in mm, angles in degrees; 0 means "work it out" where it says so).
#[derive(Clone, Debug, PartialEq)]
pub struct Diff {
    pub style: Style,
    /// Module of the side gears and pinions (bevel: at the large end; Torsen: normal module).
    pub module: f64,
    /// Side gear A's teeth, side gear B's (0: as A; the torque-biasing style needs a different
    /// count), the spider pinions' or element gears' teeth.
    pub side_teeth: u32,
    pub side_teeth2: u32,
    pub pinion_teeth: u32,
    /// Spider pinions (bevel) or element pairs (Torsen).
    pub pinions: u32,
    /// Face width of the side gears (0: 0.3 × cone distance for bevels, 8 modules for Torsen).
    pub face_width: f64,
    /// Spiral angle of the bevel side gears and pinions (0: straight, the usual).
    pub spiral: f64,
    /// Helix angle of a Torsen set's side and element gears (Type A: the side worms' lead angle
    /// is 90° less this).
    pub helix: f64,
    /// Pressure angle of the differential gears (0: 20°).
    pub pressure_angle: f64,
    pub backlash: f64,
    /// The friction coefficient the Torsen bias estimate assumes (steel on steel, oiled: about
    /// 0.08 to 0.15).
    pub friction: f64,
    /// Axle shaft diameter (the side gears' bores).
    pub axle_diameter: f64,
    /// The pinion pins' diameter (0: as large as the pinions' small ends allow, about half).
    pub pin_diameter: f64,
    /// The case's wall (0: from the size), thrust washer allowance (0: from the size) and the
    /// running clearance round the gears inside it.
    pub wall: f64,
    pub washer: f64,
    pub clearance: f64,
    /// The case bearings, by name from [`CASE_BEARINGS`] or [`crate::hub::BEARINGS`] ("": the
    /// smallest whose bore takes the side gear hubs).
    pub bearing: String,
    /// Assembly windows in the case between the pinions (bevel styles; off: plain stock).
    pub windows: bool,
    pub drive: DriveKind,
    pub mount: Mount,
    /// The ring gear's and the drive pinion's teeth.
    pub ring_teeth: u32,
    pub drive_pinion_teeth: u32,
    /// The ring gear's module (bevel: at the large end; hypoid: ring outer pitch diameter / ring
    /// teeth; helical: normal module).
    pub drive_module: f64,
    /// The ring's face width (0: 0.3 × cone distance up to 10 modules; helical: 10 modules).
    pub drive_face: f64,
    /// The drive pinion's mean spiral angle (spiral bevel, hypoid) and the helical drive's helix.
    pub drive_spiral: f64,
    pub drive_helix: f64,
    /// Hypoid offset (0: 12 % of the ring's pitch diameter).
    pub drive_offset: f64,
    /// Along the axle from the set's middle: where the drive pinion's axis crosses it (bevel,
    /// hypoid) or the ring's middle plane (helical).
    pub drive_position: f64,
    /// A flange on the case with bolt holes, and tapped holes in the ring (off: the ring is a plain
    /// ring on the case, fixed however the maker likes). Bolts (0: from the bolt circle) and their
    /// size (0: the largest that fits).
    pub ring_bolts: bool,
    pub bolts: u32,
    pub bolt_diameter: f64,
    /// Which parts are built (all by default); the differential is designed whole either way.
    pub parts: Parts,
    /// Also make the keep-out body (the space the turning assembly needs, for cutting the axle
    /// housing): `Plan::clearance` (also made when `Spec::clearance_body` is set; Simple mode's
    /// designs turn it on).
    pub keep_out: bool,
}

impl Default for Diff {
    fn default() -> Self {
        Self {
            style: Style::Open,
            module: 2.5,
            side_teeth: 16,
            side_teeth2: 0,
            pinion_teeth: 10,
            pinions: 2,
            face_width: 0.0,
            spiral: 0.0,
            helix: 30.0,
            pressure_angle: 0.0,
            backlash: 0.0,
            friction: 0.1,
            axle_diameter: 16.0,
            pin_diameter: 0.0,
            wall: 0.0,
            washer: 0.0,
            clearance: 0.5,
            bearing: String::new(),
            windows: false,
            // Helical (a transaxle): a spiral bevel ring builds in seconds too, but the kernel's full
            // check of its 41 twisted teeth takes minutes, too long for a default.
            drive: DriveKind::Helical,
            mount: Mount::Bolted,
            ring_teeth: 41,
            drive_pinion_teeth: 11,
            drive_module: 4.0,
            drive_face: 0.0,
            drive_spiral: 35.0,
            drive_helix: 25.0,
            drive_offset: 0.0,
            drive_position: 0.0,
            ring_bolts: false,
            bolts: 0,
            bolt_diameter: 0.0,
            keep_out: false,
            parts: Parts::default(),
        }
    }
}

impl Diff {
    /// Sensible starting values for a style.
    pub fn defaults_for(style: Style) -> Diff {
        let mut d = Diff { style, ..Diff::default() };
        match style {
            Style::Open => {}
            Style::Biased => {
                // 12 : 18 with 12-tooth pinions: cos Σ = 6 / 24, Σ = 75.5°; (12 + 18) / 2 whole.
                d.side_teeth = 12;
                d.side_teeth2 = 18;
                d.pinion_teeth = 12;
                d.module = 3.0;
                d.drive_module = 6.0;
            }
            Style::TorsenB => {
                d.module = 2.0;
                d.side_teeth = 18;
                d.pinion_teeth = 11;
                d.pinions = 3;
                d.helix = 30.0;
                d.drive_module = 6.0;
            }
            Style::TorsenA => {
                d.module = 2.0;
                d.side_teeth = 6;
                d.pinion_teeth = 15;
                // (Three pairs crowd elements this long: two fit.)
                d.pinions = 2;
                d.helix = 60.0;
                d.drive_module = 8.0;
            }
        }
        d
    }
    /// Side gear B's teeth as built.
    pub fn side_b(&self) -> u32 {
        if self.side_teeth2 > 0 { self.side_teeth2 } else { self.side_teeth }
    }
    fn pa(&self) -> f64 {
        if self.pressure_angle > 0.0 { self.pressure_angle } else { 20.0 }
    }
    fn clr(&self) -> f64 {
        if self.clearance > 0.0 { self.clearance } else { 0.5 }
    }
}

/// Which of a differential's bodies are built. The set is designed whole either way (sizes,
/// meshes, phasing, facts); what is left out is just not made, and drops out of the meshes the
/// window checks and of the assembly.
#[derive(Clone, Debug, PartialEq)]
pub struct Parts {
    pub side_a: bool,
    pub side_b: bool,
    /// The spider pinions or element gears, and which of them (1-based, in plan order: pinion 1,
    /// 2, …; Torsen elements 1a, 1b, 2a, … counted 1, 2, 3, …; empty: all).
    pub pinions: bool,
    pub which_pinions: Vec<u32>,
    /// The pinion pins (cross shaft).
    pub pins: bool,
    pub case: bool,
    pub ring: bool,
    pub drive_pinion: bool,
}

impl Default for Parts {
    fn default() -> Self {
        Self { side_a: true, side_b: true, pinions: true, which_pinions: Vec::new(), pins: true, case: true, ring: true, drive_pinion: true }
    }
}

impl Parts {
    /// Only the gear set's toothed members: the side gears and the pinions or elements.
    pub fn gears_only() -> Parts {
        Parts { pins: false, case: false, ring: false, drive_pinion: false, ..Parts::default() }
    }
}

/// Tapered roller bearings for differential cases (ISO 355 series 302 and 320: bore, outside
/// diameter, width T), and a few larger deep groove ones, beyond [`crate::hub::BEARINGS`].
pub const CASE_BEARINGS: &[(&str, f64, f64, f64)] = &[
    ("30205", 25.0, 52.0, 16.25),
    ("30206", 30.0, 62.0, 17.25),
    ("30207", 35.0, 72.0, 18.25),
    ("30208", 40.0, 80.0, 19.75),
    ("30209", 45.0, 85.0, 20.75),
    ("30210", 50.0, 90.0, 21.75),
    ("30211", 55.0, 100.0, 22.75),
    ("30212", 60.0, 110.0, 23.75),
    ("32005", 25.0, 47.0, 15.0),
    ("32006", 30.0, 55.0, 17.0),
    ("32007", 35.0, 62.0, 18.0),
    ("32008", 40.0, 68.0, 19.0),
    ("32009", 45.0, 75.0, 20.0),
    ("32010", 50.0, 80.0, 20.0),
    ("32011", 55.0, 90.0, 23.0),
    ("32012", 60.0, 95.0, 23.0),
    ("6007", 35.0, 62.0, 14.0),
    ("6008", 40.0, 68.0, 15.0),
    ("6009", 45.0, 75.0, 16.0),
    ("6010", 50.0, 80.0, 16.0),
];

/// A case bearing's (bore, outside diameter, width) by name, from [`CASE_BEARINGS`] or the
/// gear generator's library.
pub fn case_bearing(name: &str) -> Option<(f64, f64, f64)> {
    let n = name.trim();
    CASE_BEARINGS.iter().find(|(k, ..)| k.eq_ignore_ascii_case(n)).map(|(_, d, od, w)| (*d, *od, *w)).or_else(|| crate::hub::bearing(n))
}

/// The smallest bearing whose bore is `bore` or more: deep groove from the library up to 30 mm,
/// tapered rollers (series 320, then 302) above.
pub fn pick_bearing(bore: f64) -> Option<(String, (f64, f64, f64))> {
    let mut all: Vec<(&str, f64, f64, f64)> = crate::hub::BEARINGS.iter().filter(|(_, d, ..)| *d >= 10.0).copied().collect();
    all.extend(CASE_BEARINGS.iter().copied());
    all.into_iter()
        .filter(|(_, d, ..)| *d >= bore - 1e-9)
        // Smallest bore, then the slimmest section (a ball bearing below 30 mm, a 320 above).
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)))
        .map(|(n, d, od, w)| (n.to_string(), (d, od, w)))
}

/// Validates a differential's settings.
pub fn check(s: &Spec) -> Result<(), String> {
    let d = &s.diff;
    let pos = |v: f64, what: &str| if v.is_finite() && v > 0.0 { Ok(()) } else { Err(format!("{what} must be greater than 0")) };
    let non_neg = |v: f64, what: &str| if v.is_finite() && v >= 0.0 { Ok(()) } else { Err(format!("{what} can't be negative")) };
    pos(d.module, "differential module")?;
    pos(d.axle_diameter, "axle diameter")?;
    for (v, what) in [
        (d.face_width, "face width"),
        (d.backlash, "backlash"),
        (d.pin_diameter, "pin diameter"),
        (d.wall, "case wall"),
        (d.washer, "thrust washer allowance"),
        (d.clearance, "running clearance"),
        (d.drive_face, "ring face width"),
        (d.drive_offset, "hypoid offset"),
        (d.bolt_diameter, "bolt diameter"),
        (d.pressure_angle, "pressure angle"),
    ] {
        non_neg(v, what)?;
    }
    if d.pressure_angle != 0.0 && !(14.5..=30.0).contains(&d.pressure_angle) {
        return Err("the differential gears' pressure angle must be between 14.5° and 30°".into());
    }
    let teeth = |z: u32, lo: u32, what: &str| if (lo..=200).contains(&z) { Ok(()) } else { Err(format!("{what} must be between {lo} and 200")) };
    let lo_side = if d.style == Style::TorsenA { 1 } else { 6 };
    teeth(d.side_teeth, lo_side, "side gear teeth")?;
    if d.side_teeth2 != 0 {
        teeth(d.side_teeth2, lo_side, "side gear B teeth")?;
    }
    teeth(d.pinion_teeth, 6, "pinion teeth")?;
    let most = if d.style.bevel() { 6 } else { 8 };
    if !(1..=most).contains(&d.pinions) {
        return Err(format!("{} must be between 1 and {most}", if d.style.bevel() { "pinions" } else { "element pairs" }));
    }
    if !d.spiral.is_finite() || d.spiral.abs() > 45.0 {
        return Err("the differential gears' spiral angle must be within ±45°".into());
    }
    if !d.helix.is_finite() || !(0.0..=80.0).contains(&d.helix) {
        return Err("the helix angle must be between 0° and 80°".into());
    }
    if !d.friction.is_finite() || !(0.0..=0.5).contains(&d.friction) {
        return Err("the friction coefficient must be between 0 and 0.5".into());
    }
    if !d.drive_position.is_finite() || d.drive_position.abs() > 1e4 {
        return Err("the drive position must be a number".into());
    }
    if d.drive != DriveKind::None {
        pos(d.drive_module, "ring gear module")?;
        teeth(d.ring_teeth, 10, "ring gear teeth")?;
        teeth(d.drive_pinion_teeth, 5, "drive pinion teeth")?;
        if d.ring_teeth <= d.drive_pinion_teeth {
            return Err("the ring gear needs more teeth than its pinion".into());
        }
        if !d.drive_spiral.is_finite() || d.drive_spiral.abs() > 60.0 {
            return Err("the drive's spiral angle must be within ±60°".into());
        }
        if !d.drive_helix.is_finite() || d.drive_helix.abs() >= 45.0 {
            return Err("the helical drive's helix angle must be under 45°".into());
        }
    }
    if d.parts.which_pinions.len() > 64 || d.parts.which_pinions.contains(&0) {
        return Err("which pinions: numbers from 1 (64 at most)".into());
    }
    if d.bolts > 64 {
        return Err("64 ring bolts at most".into());
    }
    if !d.bearing.trim().is_empty() && case_bearing(&d.bearing).is_none() {
        return Err(format!("no bearing is called '{}'", d.bearing.trim()));
    }
    if s.origin.iter().any(|v| !v.is_finite()) {
        return Err("the origin must be numbers".into());
    }
    crate::strength::check(&s.load)?;
    if let Some(c) = &s.clearance_body {
        crate::clearance::check(c)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Small vector helpers.

type V3 = [f64; 3];

fn unit(a: V3) -> V3 {
    let l = norm(a);
    [a[0] / l, a[1] / l, a[2] / l]
}
/// A unit vector square to `z`.
fn square_to(z: V3) -> V3 {
    let t = if z[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
    let c = [t[1] * z[2] - t[2] * z[1], t[2] * z[0] - t[0] * z[2], t[0] * z[1] - t[1] * z[0]];
    unit(c)
}

/// A world point in a frame's own coordinates.
fn local_of(f: &Frame, p: V3) -> V3 {
    let d = sub(p, f.origin);
    [dot(d, f.x), dot(d, f.y()), dot(d, f.z)]
}
fn local_dir(f: &Frame, v: V3) -> V3 {
    [dot(v, f.x), dot(v, f.y()), dot(v, f.z)]
}
/// Frame `b` carried along when frame `from` is moved onto `to`.
fn moved(b: &Frame, from: &Frame, to: &Frame) -> Frame {
    Frame { origin: to.world(local_of(from, b.origin)), x: to.dir(local_dir(from, b.x)), z: to.dir(local_dir(from, b.z)) }
}
fn shifted(b: &Body, d: V3) -> Body {
    Body { frame: b.frame.shifted(d), ..b.clone() }
}

fn circle(c: V2, r: f64) -> Region {
    Region { outer: vec![Outline::circle(c, r)], holes: vec![] }
}

/// A slot `2a` wide and `2b` high (b ≥ a: round ends at the top and bottom), centred.
fn stadium(a: f64, b: f64) -> Outline {
    let k = b - a;
    if k <= 1e-9 {
        return Outline::circle(V2::ZERO, a.min(b));
    }
    Outline {
        segs: vec![
            Seg::Line(V2::new(a, -k), V2::new(a, k)),
            Seg::Arc { c: V2::new(0.0, k), a: V2::new(a, k), b: V2::new(-a, k), ccw: true },
            Seg::Line(V2::new(-a, k), V2::new(-a, -k)),
            Seg::Arc { c: V2::new(0.0, -k), a: V2::new(-a, -k), b: V2::new(a, -k), ccw: true },
        ],
    }
}

/// The surface points of a body's first step (its teeth), in world coordinates.
/// A body's teeth: its first step, with the trim that follows it (a bevel's back and front
/// cones: its taper alone runs past them).
fn teeth_steps(b: &Body) -> Vec<Step> {
    let trims = b.steps.iter().skip(1).take_while(|st| matches!(st, Step::Revolve { op: Op::Intersect, .. })).count();
    b.steps.iter().take(1 + trims).cloned().collect()
}

fn teeth_points(b: &Body, h: f64) -> Vec<V3> {
    let first = Body { name: String::new(), frame: b.frame, steps: teeth_steps(b) };
    let solid = Solid::new(&first, h);
    solid.material_points(4.0 * h).into_iter().map(|q| b.frame.world(q)).collect()
}

/// The least radius about its own axis of a body's first step (its tooth roots at their
/// smallest).
fn least_root(b: &Body, h: f64) -> f64 {
    let first = Body { name: String::new(), frame: b.frame, steps: teeth_steps(b) };
    Solid::new(&first, h).material_points(4.0 * h).iter().map(|q| q[0].hypot(q[1])).fold(f64::INFINITY, f64::min)
}

/// A name with the set's name in front.
fn named(s: &Spec, base: &str) -> String {
    if s.name.trim().is_empty() { base.to_string() } else { format!("{} {base}", s.name.trim()) }
}

/// The words of the bevel planetary planner in a differential's.
fn renamed(t: &str) -> String {
    t.replace("bevel planetary", "bevel differential")
        .replace("sun 2", "side gear B")
        .replace("Sun 2", "Side gear B")
        .replace("second sun", "side gear B")
        .replace("sun", "side gear A")
        .replace("Sun", "Side gear A")
        .replace("planets", "pinions")
        .replace("Planets", "Pinions")
        .replace("planet", "pinion")
        .replace("Planet", "Pinion")
        .replace("carrier", "case")
        .replace("cone angle (side gear A axis to pinion axes)", "pinion axis angle to the axle")
}

// ---------------------------------------------------------------------------------------------
// The gear set inside the case.

/// What a member of the set is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Side,
    /// Turns on a pin or in a pocket of the case.
    Carried,
    /// Fixed to the case (the pinion pins).
    Fixed,
}

/// The gear set and what it asks of the case, in the set's frame (axle along z, origin in the
/// middle).
struct Core {
    bodies: Vec<Body>,
    roles: Vec<Role>,
    meshes: Vec<MeshPair3>,
    /// Gear links (body indices in `bodies`, pitch radii).
    links: Vec<(usize, usize, f64, f64)>,
    /// The least outer radius of the case round the gears, and its wall.
    case_r: f64,
    wall: f64,
    /// Opt-in cuts in the case (assembly windows).
    cuts: Vec<Step>,
    preview: Vec<PreviewGear>,
    preview_note: String,
    /// Each side gear's share of the case's torque.
    split: [f64; 2],
    /// The side gears' face width.
    face: f64,
}

/// A side gear's hub and axle bore, added behind its back face (local z = `back`, the hub
/// running towards −z for `length`).
fn side_hub(steps: &mut Vec<Step>, back: f64, r_hub: f64, length: f64, axle: f64) {
    steps.push(Step::Prism { region: circle(V2::ZERO, r_hub), z0: back - length, height: length + 0.01, twist: 0.0, op: Op::Join });
    steps.push(Step::Prism { region: circle(V2::ZERO, axle / 2.0), z0: back - length - 1.0, height: length + 2.0, twist: 0.0, op: Op::Cut });
}

/// The hub's radius and length for an axle.
fn hub_size(axle: f64) -> (f64, f64) {
    (axle / 2.0 + (0.25 * axle).max(2.0), (0.6 * axle).max(4.0))
}

fn bevel_core(s: &Spec, p: &mut Plan) -> Result<Core, String> {
    let d = &s.diff;
    let (za, zp, n) = (d.side_teeth, d.pinion_teeth, d.pinions);
    let biased = d.style == Style::Biased;
    let zb = if biased {
        let zb = d.side_teeth2;
        if zb == 0 || zb == za {
            return Err("a torque-biasing differential needs side gear B with a different tooth count from side gear A (equal ones make an open differential)".into());
        }
        zb
    } else {
        if d.side_teeth2 != 0 && d.side_teeth2 != za {
            p.warn("an open differential has equal side gears: side gear B is built like A (the torque-biasing style takes unequal ones)");
        }
        za
    };
    let (fa, fb, fp) = (f64::from(za), f64::from(zb), f64::from(zp));
    let c = (fb - fa) / (2.0 * fp);
    if c.abs() >= 0.9 {
        return Err(format!(
            "side gears of {za} and {zb} teeth with {zp}-tooth pinions need the pinion axes at cos Σ = ({zb} − {za}) / (2 × {zp}) = {}: too steep; use bigger pinions or side gears closer in size",
            num(c)
        ));
    }
    let sigma = c.acos();
    let m = d.module;
    let ds = sigma.sin().atan2(fp / fa + sigma.cos());
    let re = m * fa / (2.0 * ds.sin());
    let face = if d.face_width > 0.0 { d.face_width } else { (0.3 * re * 10.0).round() / 10.0 };
    let probe = Spec {
        kind: Kind::BevelPlanetary,
        bevel_planet_mode: BevelPlanetMode::Differential,
        module: m,
        teeth: za,
        teeth2: zp,
        teeth3: if biased { zb } else { 0 },
        planets: n,
        shaft_angle: sigma.to_degrees(),
        spiral_angle: d.spiral,
        pressure_angle: d.pa(),
        backlash: d.backlash,
        face_width: face,
        first_azimuth: 0.0,
        name: s.name.clone(),
        origin: [0.0; 3],
        ..Spec::defaults_for(Kind::BevelPlanetary)
    };
    let g = crate::plan::bevel_planet::geometry(&probe).map_err(|e| renamed(&e))?;
    // The pinion's small end: the pin goes through it.
    let mn = m * (d.spiral.to_radians()).cos();
    let k_small = (g.re - g.face) / g.re;
    let pinion_root_small = k_small * (m * fp / 2.0 - probe.rack.dedendum * mn * g.delta_planet.cos());
    let side_root_small = k_small * (m * fa / 2.0 - probe.rack.dedendum * mn * g.delta_sun.cos());
    let pin = if d.pin_diameter > 0.0 { d.pin_diameter } else { ((1.1 * pinion_root_small) * 2.0).floor() / 2.0 };
    if pin < 1.0 {
        return Err(format!(
            "the pinions are too small for a pin (small-end roots Ø{}): use a bigger module or more pinion teeth",
            num(2.0 * pinion_root_small)
        ));
    }
    let clr = d.clr();
    let axle = d.axle_diameter;
    if axle / 2.0 >= side_root_small - 0.05 * m {
        return Err(format!(
            "the axle (Ø{}) doesn't fit through side gear A's small end (roots Ø{}): use a bigger module, more side gear teeth or a thinner axle",
            num(axle),
            num(2.0 * side_root_small)
        ));
    }
    let sub = Spec {
        bore: Bore { kind: BoreKind::Round, diameter: axle, ..Bore::default() },
        bore2: Bore { kind: BoreKind::Round, diameter: pin + 0.1, ..Bore::default() },
        ..probe
    };
    let sp = crate::plan::plan(&sub).map_err(|e| renamed(&e))?;
    for w in &sp.warnings {
        p.warn(renamed(w));
    }
    // (The case holds the pins: the bevel planetary set's own carrier is left out.)
    let carrier = |name: &str| name.contains(crate::plan::bevel_planet::CARRIER) || name.contains("carrier pin");
    for f in sp.facts.iter().filter(|f| !carrier(&f.label)) {
        p.facts.push(Fact { label: renamed(&f.label), value: f.value, unit: f.unit });
    }
    let mut bodies: Vec<Body> = sp.bodies.iter().filter(|b| !carrier(&b.name)).map(|b| Body { name: renamed(&b.name), ..b.clone() }).collect();
    let nb = bodies.len();
    // Sun first, the planets, sun 2 last: side gears first, then the pinions.
    let side_b = bodies.remove(nb - 1);
    bodies.insert(1, side_b);
    let (r_hub, l_hub) = hub_size(axle);
    let mut roles = vec![Role::Side, Role::Side];
    roles.extend(std::iter::repeat_n(Role::Carried, nb - 2));
    let h = 0.05 * m;
    // The side gears' back faces and their tips at the heel. A bevel's teeth end on its back
    // cone, its body on a flat back face a little under the roots there (local z = the taper's
    // start, below the frame's origin on the large end's pitch plane).
    let back_of = |b: &Body| b.steps.iter().find_map(|st| if let Step::Taper { z0, .. } = st { Some(*z0) } else { None }).unwrap_or(0.0);
    let (back_a, back_b) = (back_of(&bodies[0]), back_of(&bodies[1]));
    let h_a = -(bodies[0].frame.origin[2] + back_a);
    let h_b = bodies[1].frame.origin[2] - back_b;
    let heel_tip = |b: &Body| teeth_points(b, h).iter().map(|q| local_of(&b.frame, *q)).map(|l| l[0].hypot(l[1])).fold(0.0, f64::max);
    let (ra_a, ra_b) = (heel_tip(&bodies[0]), heel_tip(&bodies[1]));
    let r_hub = r_hub.min(0.9 * (ra_a.min(ra_b) - 2.25 * m));
    for (i, back) in [back_a, back_b].into_iter().enumerate() {
        side_hub(&mut bodies[i].steps, back, r_hub, l_hub, axle);
    }
    // The sphere round the apex that holds every member's teeth: the pins reach past it.
    let mut reach = 0.0f64;
    for b in &bodies {
        for q in teeth_points(b, h) {
            reach = reach.max(norm(q));
        }
    }
    let rs = reach + clr;
    let wall = if d.wall > 0.0 { d.wall } else { (0.15 * rs).max(3.0).max(0.6 * pin) };

    // Pins: one stub per pinion from the apex out into the case wall (two opposite ones make a
    // cross shaft), all one body fixed to the case (the case's bores for them follow it).
    let mut cuts = Vec::new();
    let mut pin_steps = Vec::new();
    for b in &bodies[2..] {
        let u = b.frame.z.map(|v| -v);
        let f = Frame { origin: [0.0; 3], x: square_to(u), z: u };
        let op = if pin_steps.is_empty() { Op::New } else { Op::Join };
        pin_steps.push(Step::Placed {
            frame: f,
            step: Box::new(Step::Prism { region: circle(V2::ZERO, pin / 2.0), z0: 0.0, height: rs + 0.7 * wall, twist: 0.0, op }),
        });
    }
    let near_end = (g.re - g.face) * g.delta_sun.cos().min((PI - g.delta_far).cos());
    if pin / 2.0 + clr > near_end * (sigma.sin()).max(0.2) && (sigma - PI / 2.0).abs() < 1e-6 {
        p.warn(format!("the pinion pins (Ø{}) come within the running clearance of the side gears' small ends", num(pin)));
    }
    bodies.push(Body { name: named(s, "Pinion pins"), frame: Frame::at([0.0; 3]), steps: pin_steps });
    roles.push(Role::Fixed);
    // Windows between the pinions, to put the gears in.
    if d.windows && n >= 1 {
        let ra_p = bodies[2..nb]
            .iter()
            .map(|b| teeth_points(b, h).iter().map(|q| local_of(&b.frame, *q)).map(|l| l[0].hypot(l[1])).fold(0.0, f64::max))
            .fold(0.0, f64::max);
        let half_h = (ra_p + clr).min(0.85 * h_a.min(h_b));
        let gap = if n == 1 { PI } else { PI / f64::from(n) };
        let half_w = (ra_p + clr).min(rs * (gap - (pin / 2.0 + wall) / rs).max(0.0).sin());
        if half_w > 0.3 * ra_p && half_h > 0.3 * ra_p {
            for (i, th) in g.azimuths.iter().enumerate() {
                if n == 1 && i > 0 {
                    break;
                }
                let az = th + gap;
                let z = [az.cos(), az.sin(), 0.0];
                let f = Frame { origin: [0.0; 3], x: [-az.sin(), az.cos(), 0.0], z };
                let o = stadium(half_w.min(half_h), half_h.max(half_w));
                let o = if half_w > half_h { o.rotated(PI / 2.0) } else { o };
                cuts.push(Step::Placed {
                    frame: f,
                    step: Box::new(Step::Prism {
                        region: Region { outer: vec![o], holes: vec![] },
                        z0: 0.5 * rs,
                        height: rs + 2.0 * wall + 10.0,
                        twist: 0.0,
                        op: Op::Cut,
                    }),
                });
            }
            p.fact("assembly window width", 2.0 * half_w, Unit::Mm);
            p.fact("assembly window height", 2.0 * half_h, Unit::Mm);
        } else {
            p.warn("no room for assembly windows between the pinions: left out (the case would be made in two halves)");
        }
    }

    // Meshes and links, renamed and re-indexed (sun 0, planets 1..=n, sun 2 n+1 in the sub plan;
    // here side A 0, side B 1, pinions 2..).
    let meshes = sp
        .meshes3d
        .iter()
        .map(|mp| MeshPair3 { labels: mp.labels.as_ref().map(|[a, b]| [renamed(a), renamed(b)]), ..mp.clone() })
        .map(|mut mp| {
            if let Some(bs) = &mut mp.bodies {
                for b in bs.iter_mut() {
                    b.name = renamed(&b.name);
                }
                // Each side gear with the hub it now has (the mesh is checked on the bodies built).
                for b in bs.iter_mut() {
                    if let Some(full) = bodies[..2].iter().find(|x| x.name == b.name) {
                        *b = full.clone();
                    }
                }
            }
            mp
        })
        .collect();
    let (ra_pitch, rb_pitch, rp_pitch) = (m * fa / 2.0, m * fb / 2.0, m * fp / 2.0);
    let mut links = Vec::new();
    for i in 2..nb {
        links.push((0, i, ra_pitch, rp_pitch));
        links.push((i, 1, rp_pitch, rb_pitch));
    }
    p.fact("pinion pin diameter", pin, Unit::Mm);
    p.fact("side gear hub diameter", 2.0 * r_hub, Unit::Mm);
    p.fact("side gear hub length", l_hub, Unit::Mm);
    let preview = sp.preview.iter().map(|g| PreviewGear { name: renamed(&g.name), ..g.clone() }).collect();
    Ok(Core {
        bodies,
        roles,
        meshes,
        links,
        case_r: rs + wall,
        wall,
        cuts,
        preview,
        preview_note: renamed(&sp.preview_note),
        split: [fa / (fa + fb), fb / (fa + fb)],
        face: g.face,
    })
}

/// The Torsen Type B's torque bias, estimated from where friction acts: each element gear is
/// pressed into its pocket by its two meshes' tooth forces (taken at the pocket wall where each
/// is applied, at the element's tip radius), each side gear onto its thrust face by the helix's
/// axial force (at the face's mean radius `r_thrust`), and each mesh loses about 1 %. Power from
/// the faster side gear to the slower one passes a side thrust face, an element, three meshes, the
/// other element and the other thrust face; the bias is the inverse of that efficiency.
/// Returns (bias ratio, the element pocket loss fraction, the side thrust face loss fraction).
pub fn torsen_b_bias(zs: u32, ze: u32, module: f64, helix_deg: f64, pressure_deg: f64, mu: f64, r_thrust: f64) -> (f64, f64, f64) {
    let beta = helix_deg.to_radians();
    let at = (pressure_deg.to_radians().tan() / beta.cos()).atan();
    let mt = module / beta.cos();
    let (rs, re) = (mt * f64::from(zs) / 2.0, mt * f64::from(ze) / 2.0);
    let ra_e = re + module;
    // Element 1 at (a, 0); side A at the origin; element 2 at the same radius, 2·re away.
    let a = rs + re;
    let phi = 2.0 * (re / a).asin();
    let c1 = V2::new(a, 0.0);
    let c2 = V2::polar(a, phi);
    let u1 = (V2::ZERO - c1).norm();
    let u2 = (c2 - c1).norm();
    let perp = |u: V2| V2::new(-u.y, u.x);
    let fr = at.tan();
    // Unit tangential force: from side A driving, from element 2 resisting.
    let f_a = perp(u1) - u1 * fr;
    let f_2 = perp(u2) * -1.0 - u2 * fr;
    let wall_load = f_a.len() + f_2.len();
    let f_wall = mu * wall_load * ra_e / re;
    let f_side = mu * beta.tan() * r_thrust / rs;
    let eta_mesh = 0.99f64;
    let eta = (1.0 - f_side).max(0.0).powi(2) * (1.0 - f_wall).max(0.0).powi(2) * eta_mesh.powi(3);
    let tbr = if eta > 1e-6 { 1.0 / eta } else { f64::INFINITY };
    (tbr, f_wall, f_side)
}

fn torsen_b_core(s: &Spec, p: &mut Plan) -> Result<Core, String> {
    let d = &s.diff;
    let (zs, ze, n) = (d.side_teeth, d.pinion_teeth, d.pinions);
    if d.side_teeth2 != 0 && d.side_teeth2 != zs {
        p.warn("a Torsen Type B has equal side gears: side gear B is built like A");
    }
    let m = d.module;
    let beta = d.helix;
    if !(10.0..=50.0).contains(&beta) {
        return Err(format!("a Torsen Type B needs helical gears: a helix angle of 10° to 50° (not {}°)", num(beta)));
    }
    let clr = d.clr();
    let base = Spec { module: m, pressure_angle: d.pa(), backlash: d.backlash, ..Spec::default() };
    let ga = base.involute(zs, 0.0, beta);
    let e1 = base.involute(ze, 0.0, -beta);
    let e2 = base.involute(ze, 0.0, beta);
    let gb = base.involute(zs, 0.0, -beta);
    let w = if d.face_width > 0.0 { d.face_width } else { 8.0 * m };
    let lz = w;
    let cax = (0.15 * m).max(0.5);
    let a = mesh_external(&ga, &e1, w).center_distance;
    let aee = mesh_external(&e1, &e2, lz).center_distance;
    let phi = 2.0 * (aee / (2.0 * a)).asin();
    let ra_e = e1.ra();
    let ra_s = ga.ra();
    // Pairs round the side gears at whole multiples of π / zs: every pair then meshes side gear B
    // the same way (the case turned with side A held turns B twice as far).
    let step = TAU / f64::from(2 * zs);
    if (2 * zs) % n != 0 {
        p.warn(format!(
            "2 × side gear teeth / pairs = {}/{n} is not whole: the element pairs are spaced unevenly (the nearest places where they mesh)",
            2 * zs
        ));
    }
    let thetas: Vec<f64> = (0..n).map(|k| step * (f64::from(k) * f64::from(2 * zs) / f64::from(n)).round()).collect();
    if n > 1 {
        for k in 0..n as usize {
            let next = if k + 1 < n as usize { thetas[k + 1] } else { thetas[0] + TAU };
            let free = next - thetas[k] - phi;
            if 2.0 * a * (free / 2.0).sin() < 2.0 * ra_e + 2.0 * clr {
                return Err(format!(
                    "{n} element pairs don't fit round the side gears (neighbouring elements collide): use fewer pairs, fewer element teeth or more side gear teeth"
                ));
            }
        }
    }
    let axle = d.axle_diameter;
    if axle / 2.0 >= ga.rf() - 0.5 * m {
        return Err(format!(
            "the axle (Ø{}) doesn't fit in the side gears (roots Ø{}): use more side gear teeth or a thinner axle",
            num(axle),
            num(2.0 * ga.rf())
        ));
    }
    let (mut r_hub, l_hub) = hub_size(axle);
    r_hub = r_hub.min(ga.rf() - 0.5 * m);
    // Along the axle: side A, the elements' mesh stretch, side B.
    let za_hi = -lz / 2.0 - cax;
    let za_lo = za_hi - w;
    let zb_lo = lz / 2.0 + cax;
    let zb_hi = zb_lo + w;
    let tw = |g: &Involute, z: f64| z * g.helix.tan() / g.r();
    let prism = |g: &Involute, phase: f64, z0: f64, z1: f64| {
        let (o, _) = external_outline(g, phase + tw(g, z0));
        Step::Prism { region: Region { outer: vec![o], holes: vec![] }, z0, height: z1 - z0, twist: tw(g, z1 - z0), op: Op::New }
    };
    let mut side_a = vec![prism(&ga, 0.0, za_lo, za_hi)];
    side_hub(&mut side_a, za_lo, r_hub, l_hub, axle);
    side_a.push(Step::Prism { region: circle(V2::ZERO, axle / 2.0), z0: za_lo - 1.0, height: w + 2.0, twist: 0.0, op: Op::Cut });
    // Side B's phase from pair 0.
    let c_of = |th: f64| V2::polar(a, th);
    let phases = |th: f64| {
        let p1 = mesh_phase(zs, 0.0, ze, th);
        let (c1, c2) = (c_of(th), c_of(th + phi));
        let p2 = mesh_phase(ze, p1, ze, (c2 - c1).angle());
        let pb = mesh_phase(ze, p2, zs, (V2::ZERO - c2).angle());
        (p1, p2, pb)
    };
    let (_, _, pb0) = phases(thetas[0]);
    let pitch_s = TAU / f64::from(zs);
    for th in &thetas {
        let (_, _, pb) = phases(*th);
        let off = crate::geom::wrap_period(pb - pb0, pitch_s);
        if off.abs() > 1e-6 {
            p.warn(format!(
                "element pair at {}°: side gear B is {}° off its mesh (an assembly condition not met)",
                num(th.to_degrees()),
                num(off.to_degrees())
            ));
        }
    }
    // Side B's hub runs up (+z) from its top face: built in a frame turned over.
    let mut side_b = vec![prism(&gb, pb0, zb_lo, zb_hi)];
    side_b.push(Step::Prism { region: circle(V2::ZERO, r_hub), z0: zb_hi - 0.01, height: l_hub + 0.01, twist: 0.0, op: Op::Join });
    side_b.push(Step::Prism { region: circle(V2::ZERO, axle / 2.0), z0: zb_lo - 1.0, height: w + l_hub + 2.0, twist: 0.0, op: Op::Cut });
    let mut bodies = vec![
        Body { name: named(s, &format!("Side gear A z{zs}")), frame: Frame::at([0.0; 3]), steps: side_a },
        Body { name: named(s, &format!("Side gear B z{zs}")), frame: Frame::at([0.0; 3]), steps: side_b },
    ];
    let mut roles = vec![Role::Side, Role::Side];
    let mut meshes = Vec::new();
    let mut links = Vec::new();
    let mut preview = vec![PreviewGear::turning(
        "side gear A",
        Region { outer: vec![external_outline(&ga, 0.0).0], holes: vec![Outline::circle(V2::ZERO, axle / 2.0)] },
        V2::ZERO,
        1.0,
        ga.r(),
    )];
    let gap = d.backlash * d.pa().to_radians().cos() * beta.to_radians().cos();
    let cuts = Vec::new();
    let (fs, fe) = (f64::from(zs), f64::from(ze));
    for (k, th) in thetas.iter().enumerate() {
        let (p1, p2, _) = phases(*th);
        let (c1, c2) = (c_of(*th), c_of(th + phi));
        let i1 = bodies.len();
        let label = |j: u32| format!("element {}{}", k + 1, if j == 1 { "a" } else { "b" });
        bodies.push(Body {
            name: named(s, &format!("Element {}a z{ze}", k + 1)),
            frame: Frame::at([c1.x, c1.y, 0.0]),
            steps: vec![prism(&e1, p1, za_lo, lz / 2.0)],
        });
        bodies.push(Body {
            name: named(s, &format!("Element {}b z{ze}", k + 1)),
            frame: Frame::at([c2.x, c2.y, 0.0]),
            steps: vec![prism(&e2, p2, -lz / 2.0, zb_hi)],
        });
        roles.extend([Role::Carried, Role::Carried]);
        let pair = |a: usize, b: usize, ratio: f64, period: f64, la: String, lb: String, bodies: &Vec<Body>| MeshPair3 {
            a: 0,
            b: 1,
            bodies: Some([bodies[a].clone(), bodies[b].clone()]),
            ratio,
            period,
            gap: (gap, gap),
            scale: m,
            labels: Some([la, lb]),
        };
        meshes.push(pair(0, i1, -fs / fe, TAU / fs, "side gear A".into(), label(1), &bodies));
        meshes.push(pair(i1, i1 + 1, -1.0, TAU / fe, label(1), label(2), &bodies));
        meshes.push(pair(i1 + 1, 1, -fe / fs, TAU / fe, label(2), "side gear B".into(), &bodies));
        links.push((0, i1, ga.r(), e1.r()));
        links.push((i1, i1 + 1, e1.r(), e2.r()));
        links.push((i1 + 1, 1, e2.r(), gb.r()));
        preview.push(PreviewGear::turning(label(1), Region { outer: vec![external_outline(&e1, p1).0], holes: vec![] }, c1, -fs / fe, e1.r()));
        preview.push(PreviewGear::turning(label(2), Region { outer: vec![external_outline(&e2, p2).0], holes: vec![] }, c2, fs / fe, e2.r()));
    }
    let wall = if d.wall > 0.0 { d.wall } else { (2.0 * m).max(3.0) };
    let case_r = a + ra_e + clr + wall;

    // Facts and the bias estimate.
    let r_thrust = 0.5 * (r_hub + 0.85 * ga.rf());
    let (tbr, f_wall, f_side) = torsen_b_bias(zs, ze, m, beta, d.pa(), d.friction, r_thrust);
    p.fact("side gear teeth (both)", fs, Unit::None);
    p.fact("element gear teeth", fe, Unit::None);
    p.fact("element pairs", f64::from(n), Unit::None);
    p.fact("helix angle (side A right hand, side B left)", beta, Unit::Deg);
    p.fact("transverse module", ga.mt(), Unit::None);
    p.fact("side gear pitch diameter", 2.0 * ga.r(), Unit::Mm);
    p.fact("side gear tip diameter", 2.0 * ra_s, Unit::Mm);
    p.fact("element pitch diameter", 2.0 * e1.r(), Unit::Mm);
    p.fact("element tip diameter", 2.0 * ra_e, Unit::Mm);
    p.fact("element centre circle diameter", 2.0 * a, Unit::Mm);
    p.fact("angle between a pair's elements", phi.to_degrees(), Unit::Deg);
    p.fact("element length (each)", w + cax + lz, Unit::Mm);
    p.fact("element mesh stretch", lz, Unit::Mm);
    p.fact("side gear face width", w, Unit::Mm);
    p.fact("assumed friction coefficient μ", d.friction, Unit::None);
    p.fact("torque bias ratio TBR (estimate)", tbr, Unit::None);
    p.fact("element pocket friction loss (estimate, fraction)", f_wall, Unit::None);
    p.fact("side gear thrust face friction loss (estimate, fraction)", f_side, Unit::None);
    p.warn(format!(
        "the torque bias ratio {} is an estimate from the helix and pressure angles with μ = {} at the element pockets and thrust faces; measure a built unit",
        num((tbr * 100.0).round() / 100.0),
        num(d.friction)
    ));
    if let Some(e) = [mesh_external(&ga, &e1, w), mesh_external(&e1, &e2, lz)].iter().find(|m| m.contact_ratio < 1.0) {
        p.warn(format!("contact ratio {:.2} under 1", e.contact_ratio));
    }
    Ok(Core {
        bodies,
        roles,
        meshes,
        links,
        case_r,
        wall,
        cuts,
        preview,
        preview_note:
            "transverse section at the middle of the set: side gear A (side gear B lies behind it, meshing the second element of each pair)".into(),
        split: [0.5, 0.5],
        face: w,
    })
}

/// A Torsen Type A (T‑1) layout, worked out: (facts, what isn't built).
/// Side gears are worms of `side_teeth` starts with lead angle 90° − helix; elements are worm
/// wheels of `pinion_teeth` teeth across the axle (crossed helical at 90°), with a spur gear of the
/// same teeth on each end meshing the paired element's.
pub fn torsen_a_layout(d: &Diff) -> Vec<(String, f64, Unit)> {
    let m = d.module;
    let lambda = (90.0 - d.helix).to_radians();
    let (zw, ze) = (f64::from(d.side_teeth), f64::from(d.pinion_teeth));
    // The worm's pitch diameter from its lead: lead = zw·π·m_axial, tan λ = lead / (π·d).
    let mx = m / lambda.cos();
    let dw = zw * mx / lambda.tan();
    let de = ze * m / d.helix.to_radians().cos();
    let mu = d.friction;
    let rho = mu.atan();
    let eta_f = lambda.tan() / (lambda + rho).tan();
    let eta_b = if lambda > rho { (lambda - rho).tan() / lambda.tan() } else { 0.0 };
    let eta = eta_f * eta_b * 0.99f64.powi(2);
    let tbr = if eta > 1e-6 { 1.0 / eta } else { f64::INFINITY };
    vec![
        ("side worm starts".into(), zw, Unit::None),
        ("side worm lead angle".into(), lambda.to_degrees(), Unit::Deg),
        ("side worm pitch diameter".into(), dw, Unit::Mm),
        ("element wheel pitch diameter".into(), de, Unit::Mm),
        ("element axis distance from the axle".into(), (dw + de) / 2.0, Unit::Mm),
        ("element pairs".into(), f64::from(d.pinions), Unit::None),
        ("worm driving wheel efficiency (estimate)".into(), eta_f, Unit::None),
        ("wheel driving worm efficiency (estimate)".into(), eta_b, Unit::None),
        ("torque bias ratio TBR (estimate)".into(), tbr, Unit::None),
    ]
}

/// A body turned about its own frame's z by `a` (its teeth's phase).
fn spun(b: &Body, a: f64) -> Body {
    let f = b.frame;
    let y = f.y();
    let (s, c) = a.sin_cos();
    Body { frame: Frame { x: [0, 1, 2].map(|i| f.x[i] * c + y[i] * s), ..f }, ..b.clone() }
}

/// A body turned about the world's z axis (through the origin) by `a`.
fn turned_about_z(b: &Body, a: f64) -> Body {
    let (s, c) = a.sin_cos();
    let r = |v: V3| [v[0] * c - v[1] * s, v[0] * s + v[1] * c, v[2]];
    Body { frame: Frame { origin: r(b.frame.origin), x: r(b.frame.x), z: r(b.frame.z) }, ..b.clone() }
}

/// Every surface point of a body's material swept a full turn about its own axis (world).
fn swept_all(b: &Body, h: f64, n: u32) -> Vec<V3> {
    let pts = body_points(b, h, 4.0 * h);
    let mut out = Vec::with_capacity(pts.len() * n as usize);
    for q in pts {
        let r = q[0].hypot(q[1]);
        for k in 0..n {
            let t = TAU * f64::from(k) / f64::from(n);
            out.push(b.frame.world([r * t.cos(), r * t.sin(), q[2]]));
        }
    }
    out
}

/// Torsen Type A (T‑1): each side gear is a worm-like helical gear on the axle; each element is
/// a helical worm wheel across the axle (a crossed helical pair at 90°, planned by the crossed
/// helical planner), with a spur gear on each end; the two elements of a pair sit side by side
/// along the axle, one over each side gear, and their spur gears mesh. Side gear B, its elements
/// and the pair's second element are the first ones moved along the axle; each element's spur
/// teeth are clocked to mesh its partner's. The elements turn on journals in the case.
fn torsen_a_core(s: &Spec, p: &mut Plan) -> Result<Core, String> {
    let d = &s.diff;
    for (l, v, u) in torsen_a_layout(d) {
        p.fact(l, v, u);
    }
    let (zs, ze, n) = (d.side_teeth, d.pinion_teeth, d.pinions);
    let m = d.module;
    let clr = d.clr();
    let axle = d.axle_diameter;
    let w = if d.face_width > 0.0 { d.face_width } else { 8.0 * m };
    let beta_s = d.helix;
    if !(30.0..=74.0).contains(&beta_s) {
        return Err(format!(
            "a Torsen Type A's side gears are worms: their helix angle must be 30° to 74° (the lead angle 16° to 60°), not {}°",
            num(beta_s)
        ));
    }
    let sub = Spec {
        kind: Kind::CrossedHelical,
        module: m,
        teeth: zs,
        teeth2: ze,
        helix: beta_s,
        shaft_angle: 90.0,
        pressure_angle: d.pa(),
        backlash: d.backlash,
        face_width: w,
        name: s.name.clone(),
        bore: Bore { kind: BoreKind::Round, diameter: axle, ..Bore::default() },
        ..Spec::defaults_for(Kind::CrossedHelical)
    };
    let sp = crate::plan::plan(&sub)
        .map_err(|e| format!("Torsen Type A side gear and element: {}", e.replace("gear 1", "side gear").replace("gear 2", "element")))?;
    for w in &sp.warnings {
        p.warn(w.replace("gear 1", "side gear").replace("gear 2", "element"));
    }
    let e = sp.facts.iter().find(|f| f.label == "centre distance").map_or(0.0, |f| f.value);
    let ge = sub.involute2(ze, 0.0, 90.0 - beta_s);
    let gs = sub.involute(zs, 0.0, beta_s);
    let (ra_e, ra_s) = (ge.ra(), gs.ra());
    // The spur gears: as many teeth as the wheel, big enough that the two wheels clear.
    let z_sp = ze;
    let m_sp = ((2.0 * (ra_e + clr) / f64::from(z_sp)) / 0.05).ceil() * 0.05;
    let spur = Spec { module: m_sp, pressure_angle: 20.0, backlash: d.backlash, ..Spec::default() }.involute(z_sp, 0.0, 0.0);
    let dz = spur.r();
    let ra_sp = spur.ra();
    // Along the element's axis: the wheel in the middle, the spurs past the side gear's tips.
    let w_sp = 0.6 * w;
    let l0 = (w / 2.0 + clr).max(((ra_s + clr).powi(2) - (e - ra_sp).max(0.0).powi(2)).max(0.0).sqrt() + clr);
    let (o_sp1, _) = external_outline(&spur, 0.0);
    let psi2 = mesh_phase(z_sp, 0.0, z_sp, PI / 2.0);
    let (o_sp2, _) = external_outline(&spur, psi2);
    let spur_steps = |o: &Outline| {
        vec![
            Step::Prism { region: Region { outer: vec![o.clone()], holes: vec![] }, z0: l0, height: w_sp, twist: 0.0, op: Op::Join },
            Step::Prism { region: Region { outer: vec![o.clone()], holes: vec![] }, z0: -l0 - w_sp, height: w_sp, twist: 0.0, op: Op::Join },
        ]
    };
    let (side0, elem0) = (sp.bodies[0].clone(), sp.bodies[1].clone());
    let side_teeth: Vec<Step> = side0.steps.iter().take(1).cloned().collect();
    let (mut r_hub, mut l_hub) = hub_size(axle);
    r_hub = r_hub.min(gs.rf() - 0.5 * m);
    let mut e1 = elem0.clone();
    e1.steps.extend(spur_steps(&o_sp1));
    let mut e2 = elem0.clone();
    e2.steps.extend(spur_steps(&o_sp2));
    let e1 = shifted(&e1, [0.0, 0.0, -dz]);
    let e2 = shifted(&e2, [0.0, 0.0, dz]);
    // Room in the case: round the elements, and how far they reach along the axle near it.
    let h = 0.05 * m;
    let swept: Vec<V3> = [&e1, &e2].iter().flat_map(|b| swept_all(b, h, 48)).collect();
    let r1 = ra_s + clr;
    let rc = swept.iter().map(|q| q[0].hypot(q[1])).fold(0.0, f64::max) + clr;
    // (How far the elements reach along the axle is the case layout's to work out.)
    let washer = if d.washer > 0.0 { d.washer } else { 1.0 };
    let near = swept.iter().filter(|q| q[0].hypot(q[1]) < r1 + clr).map(|q| q[2].abs()).fold(0.0, f64::max) + clr;
    let za = (dz + w / 2.0 + washer).max(near);
    l_hub = l_hub.max(za - (dz + w / 2.0) + 2.0);
    // Pairs round the axle; side B and the second elements follow side A's phase.
    let ratio = -f64::from(zs) / f64::from(ze);
    let mut bodies = Vec::new();
    let mut side_a = Body { name: named(s, &format!("Side gear A z{zs}")), frame: Frame::at([0.0, 0.0, -dz]), steps: side_teeth.clone() };
    side_hub(&mut side_a.steps, -w / 2.0, r_hub, l_hub, axle);
    side_a.steps.push(Step::Prism { region: circle(V2::ZERO, axle / 2.0), z0: -w / 2.0 - 1.0, height: w + 2.0, twist: 0.0, op: Op::Cut });
    let mut side_b = Body { name: named(s, &format!("Side gear B z{zs}")), frame: Frame::at([0.0, 0.0, dz]), steps: side_teeth };
    side_b.steps.push(Step::Prism { region: circle(V2::ZERO, r_hub), z0: w / 2.0 - 0.01, height: l_hub + 0.01, twist: 0.0, op: Op::Join });
    side_b.steps.push(Step::Prism { region: circle(V2::ZERO, axle / 2.0), z0: -w / 2.0 - 1.0, height: w + l_hub + 2.0, twist: 0.0, op: Op::Cut });
    bodies.push(side_a);
    bodies.push(side_b);
    let mut roles = vec![Role::Side, Role::Side];
    let mut meshes = Vec::new();
    let mut links = Vec::new();
    let cuts = Vec::new();
    let r_j = (0.35 * ge.rf()).max(1.0);
    // The journals end inside the case wall (their far corners too), so the wall takes them.
    let wall = if d.wall > 0.0 { d.wall } else { (2.0 * m).max(3.0).max(3.0 * r_j) };
    let y_end = ((rc + 0.8 * wall).powi(2) - (e + r_j).powi(2)).max(0.0).sqrt();
    if (e * e + y_end * y_end).sqrt() < rc + 0.1 * wall || y_end < l0 + w_sp + r_j {
        return Err("Torsen Type A: the case wall is too thin to hold the element journals: give a thicker wall".into());
    }
    let an = d.pa().to_radians();
    let gap_sw = 0.5 * an.cos() * d.backlash * (beta_s.to_radians().cos() + (90.0 - beta_s).to_radians().cos());
    let gap_sp = d.backlash * 20f64.to_radians().cos();
    let fs = f64::from(zs);
    let fe = f64::from(ze);
    for k in 0..n {
        let th = TAU * f64::from(k) / f64::from(n);
        // Side A turned back by the pair's azimuth: the element turns by the ratio of it.
        let place = |b: &Body| {
            let mut b = turned_about_z(&spun(b, ratio * -th), th);
            b.steps.push(Step::Prism { region: circle(V2::ZERO, r_j), z0: -y_end, height: 2.0 * y_end, twist: 0.0, op: Op::Join });
            b
        };
        let (mut a, mut b) = (place(&e1), place(&e2));
        a.name = named(s, &format!("Element {}a z{ze}", k + 1));
        b.name = named(s, &format!("Element {}b z{ze}", k + 1));
        let i1 = bodies.len();
        bodies.push(a);
        bodies.push(b);
        roles.extend([Role::Carried, Role::Carried]);
        let label = |j: u32| format!("element {}{}", k + 1, if j == 1 { "a" } else { "b" });
        let pair = |x: usize, y: usize, ratio: f64, period: f64, gap: f64, la: String, lb: String, bodies: &Vec<Body>| MeshPair3 {
            a: 0,
            b: 1,
            bodies: Some([bodies[x].clone(), bodies[y].clone()]),
            ratio,
            period,
            gap: (gap, gap),
            scale: m,
            labels: Some([la, lb]),
        };
        meshes.push(pair(0, i1, ratio, TAU / fs, gap_sw, "side gear A".into(), label(1), &bodies));
        meshes.push(pair(i1, i1 + 1, -1.0, TAU / f64::from(z_sp), gap_sp, label(1), label(2), &bodies));
        meshes.push(pair(i1 + 1, 1, 1.0 / ratio, TAU / fe, gap_sw, label(2), "side gear B".into(), &bodies));
        links.push((0, i1, gs.r(), ge.r()));
        links.push((i1, i1 + 1, spur.r(), spur.r()));
        links.push((i1 + 1, 1, ge.r(), gs.r()));
    }
    // Neighbouring pairs mustn't meet: their ends at the journals' reach.
    if n > 1 {
        let half = (y_end.max(l0 + w_sp)).atan2(e - ra_sp.max(ra_e));
        if 2.0 * half > TAU / f64::from(n) {
            return Err(format!(
                "{n} element pairs don't fit round the side gears (each spans {}° with its spurs and journals): use fewer pairs or smaller elements",
                num((2.0 * half).to_degrees())
            ));
        }
    }
    p.fact("element centre distance from the axle", e, Unit::Mm);
    p.fact("element spur gears: teeth", f64::from(z_sp), Unit::None);
    p.fact("element spur gears: module", m_sp, Unit::None);
    p.fact("elements of a pair: axis spacing along the axle", 2.0 * dz, Unit::Mm);
    p.fact("element journal diameter", 2.0 * r_j, Unit::Mm);
    p.warn("Torsen Type A: the element journals are integral pins turning in holes in the case; the case is one piece here (a real one splits to put the elements in)");
    let preview = sp
        .preview
        .iter()
        .map(|g| PreviewGear { name: g.name.replace("gear 1", "side gear A").replace("gear 2", "element 1a"), ..g.clone() })
        .collect();
    Ok(Core {
        bodies,
        roles,
        meshes,
        links,
        case_r: rc + wall,
        wall,
        cuts,
        preview,
        preview_note: "each member's transverse section: a side gear and an element (their axes cross at 90°)".into(),
        split: [0.5, 0.5],
        face: w,
    })
}

// ---------------------------------------------------------------------------------------------
// The drive gear.

struct Drive {
    ring: Body,
    pinion: Body,
    /// Ring gear's back face (world z on the axle), its top, its least tooth-root radius and its
    /// bore (0: integral).
    z_back: f64,
    z_top: f64,
    root: f64,
    bore: f64,
    /// Ring bolts: count, size, bolt circle radius.
    bolts: Option<(u32, f64, f64)>,
    flange: Option<(f64, f64)>,
    mesh: MeshPair3,
    ratio: f64,
    radii: (f64, f64),
}

fn drive_spec(s: &Spec, ring_bore: f64) -> Option<Spec> {
    let d = &s.diff;
    let bore2 = if ring_bore > 0.0 { Bore { kind: BoreKind::Round, diameter: 2.0 * ring_bore, ..Bore::default() } } else { Bore::default() };
    let (zp, zr, m) = (d.drive_pinion_teeth, d.ring_teeth, d.drive_module);
    let base = |kind: Kind| Spec {
        name: s.name.clone(),
        teeth: zp,
        teeth2: zr,
        module: m,
        bore2,
        load: s.load.clone(),
        origin: [0.0; 3],
        ..Spec::defaults_for(kind)
    };
    Some(match d.drive {
        DriveKind::None => return None,
        DriveKind::SpiralBevel | DriveKind::Bevel => {
            let kind = if d.drive == DriveKind::Bevel { Kind::Bevel } else { Kind::SpiralBevel };
            let (z1, z2) = (f64::from(zp), f64::from(zr));
            let d2 = (z2 / z1).atan();
            let re = m * z2 / (2.0 * d2.sin());
            // (A straight bevel's own planner meshes cleanly with a narrower face.)
            let k = if d.drive == DriveKind::Bevel { 0.22 } else { 0.3 };
            let face = if d.drive_face > 0.0 { d.drive_face } else { (k * re).min(10.0 * m) };
            Spec { shaft_angle: 90.0, spiral_angle: d.drive_spiral, face_width: face, ..base(kind) }
        }
        DriveKind::Hypoid => {
            let dia = m * f64::from(zr);
            let face = if d.drive_face > 0.0 { d.drive_face } else { 0.15 * dia };
            let offset = if d.drive_offset > 0.0 { d.drive_offset } else { 0.12 * dia };
            Spec { spiral_angle: d.drive_spiral, face_width: face, offset, shaft_angle: 90.0, ..base(Kind::Hypoid) }
        }
        DriveKind::Helical => {
            let face = if d.drive_face > 0.0 { d.drive_face } else { 10.0 * m };
            Spec { helix: d.drive_helix, face_width: face, ..base(Kind::Pair) }
        }
    })
}

#[cfg(test)]
pub(crate) fn drive_spec_for_tests(s: &Spec) -> Option<Spec> {
    drive_spec(s, 0.0)
}

fn plan_drive(s: &Spec, core: &Core, r_case: f64, p: &mut Plan) -> Result<Option<Drive>, String> {
    let d = &s.diff;
    let bolted = d.mount == Mount::Bolted && d.ring_bolts;
    // A separate ring sits on the case's cylinder (a plain fit; it is fixed however the maker likes).
    let r_bore = if d.mount == Mount::Bolted { r_case } else { 0.0 };
    let Some(sub) = drive_spec(s, r_bore) else { return Ok(None) };
    let what = d.drive.label().to_lowercase();
    let mut sp = crate::plan::plan(&sub).map_err(|e| {
        if e.contains("bore") {
            format!(
                "the {what}: the ring gear is too small to go round the case (Ø{}): use a bigger ring gear (more teeth or a bigger module)",
                num(2.0 * r_case)
            )
        } else {
            format!("the {what}: {e}")
        }
    })?;
    if sp.bodies.len() != 2 {
        return Err(format!("the {what} didn't plan its two gears"));
    }
    let mut ring = sp.bodies.remove(1);
    let mut pinion = sp.bodies.remove(0);
    ring.name = named(s, &format!("Ring gear z{}", d.ring_teeth));
    pinion.name = named(s, &format!("Drive pinion z{}", d.drive_pinion_teeth));
    for f in &sp.facts {
        p.facts.push(Fact {
            label: format!("drive: {}", f.label.replace("gear 1", "pinion").replace("gear 2", "ring gear")),
            value: f.value,
            unit: f.unit,
        });
    }
    for w in &sp.warnings {
        p.warn(format!("drive: {}", w.replace("gear 1", "pinion").replace("gear 2", "ring gear")));
    }
    // Strength of a hypoid as the spiral bevel of its size (the rating has no hypoid model).
    if d.drive == DriveKind::Hypoid && s.load.rated() {
        let eq = Spec { kind: Kind::SpiralBevel, shaft_angle: 90.0, ..sub.clone() };
        let mut tmp = Plan::default();
        crate::strength::rate(&eq, &mut tmp);
        for f in tmp.facts {
            p.facts.push(Fact {
                label: format!("drive (rated as a spiral bevel): {}", f.label.replace("gear 1", "pinion").replace("gear 2", "ring gear")),
                ..f
            });
        }
        for w in tmp.warnings {
            p.warn(format!("drive: {w}"));
        }
    }
    let (zp, zr) = (f64::from(d.drive_pinion_teeth), f64::from(d.ring_teeth));
    let m = d.drive_module;
    // A shank on the bevel and helical pinions (the hypoid's has its own).
    match d.drive {
        DriveKind::SpiralBevel | DriveKind::Bevel => {
            let r = 0.45 * m * zp / 2.0;
            let l = 1.2 * m * zp;
            pinion.steps.push(Step::Prism { region: circle(V2::ZERO, r), z0: -l, height: l + 0.01, twist: 0.0, op: Op::Join });
        }
        DriveKind::Helical => {
            let r = 0.4 * m * zp / 2.0;
            let l = 0.8 * m * zp;
            pinion.steps.push(Step::Prism { region: circle(V2::ZERO, r), z0: -l, height: sub.face_width + 2.0 * l, twist: 0.0, op: Op::Join });
        }
        _ => {}
    }
    let h = (0.05 * m).clamp(0.02, 0.5);
    let ring_solid = Solid::new(&ring, 1.0);
    let lo = ring_solid.z.0;
    let hi = ring_solid.z.1;
    let root = least_root(&ring, h);
    // Where the ring goes: its axis on the axle, teeth facing +z.
    let pos = d.drive_position;
    let target_z = match d.drive {
        DriveKind::Helical => pos - sub.face_width / 2.0,
        _ => pos - dot(sub_v(pinion.frame.origin, ring.frame.origin), ring.frame.z),
    };
    let target = Frame { origin: [0.0, 0.0, target_z], x: [1.0, 0.0, 0.0], z: [0.0, 0.0, 1.0] };
    let from = ring.frame;
    ring.frame = moved(&ring.frame, &from, &target);
    pinion.frame = moved(&pinion.frame, &from, &target);
    let (z_back, z_top) = (target_z + lo, target_z + hi);
    // Bolts: tapped holes in the ring between its bore and its roots, through holes in a flange.
    let mut bolts = None;
    let mut flange = None;
    if bolted {
        let room = root - r_bore;
        let size = if d.bolt_diameter > 0.0 {
            Some(d.bolt_diameter)
        } else {
            [16.0, 14.0, 12.0, 10.0, 8.0, 6.0, 5.0, 4.0, 3.0].into_iter().find(|b| *b <= room / 1.8)
        };
        match size {
            Some(b) if b <= room / 1.65 => {
                let rbc = r_bore + room / 2.0;
                let n = if d.bolts > 0 { d.bolts } else { ((((TAU * rbc / (4.0 * b)).round() as u32) / 2) * 2).clamp(6, 16) };
                let depth = 1.5 * b;
                let holes = crate::hub::lightening_holes(n, 0.85 * b, rbc, PI / f64::from(n));
                ring.steps.push(Step::Prism {
                    region: Region { outer: holes, holes: vec![] },
                    z0: lo - 1.0,
                    height: depth + 1.0,
                    twist: 0.0,
                    op: Op::Cut,
                });
                bolts = Some((n, b, rbc));
                flange = Some((rbc + 0.55 * b + (0.8 * b).max(2.0), (1.2 * b).max(core.wall)));
            }
            _ => {
                p.warn(format!(
                    "no room for ring bolts between the case (Ø{}) and the ring's tooth roots (Ø{}): bolts left out; use a bigger ring gear or the integral mount",
                    num(2.0 * r_bore),
                    num(2.0 * root)
                ));
            }
        }
    }
    let (ratio, _, period) = crate::mesh3d::motion(&sub);
    let gap = crate::mesh3d::expected_gap(&sub);
    let radii = match d.drive {
        DriveKind::Helical => (m * zp / (2.0 * d.drive_helix.to_radians().cos()), m * zr / (2.0 * d.drive_helix.to_radians().cos())),
        _ => (m * zp / 2.0, m * zr / 2.0),
    };
    let mesh = MeshPair3 {
        a: 0,
        b: 1,
        bodies: Some([pinion.clone(), ring.clone()]),
        ratio,
        period,
        gap,
        scale: m,
        labels: Some(["drive pinion".into(), "ring gear".into()]),
    };
    Ok(Some(Drive { ring, pinion, z_back, z_top, root, bore: r_bore, bolts, flange, mesh, ratio: zr / zp, radii }))
}

fn sub_v(a: V3, b: V3) -> V3 {
    sub(a, b)
}

// ---------------------------------------------------------------------------------------------
// The case.

fn axle_bore(d: &Diff) -> f64 {
    d.axle_diameter / 2.0 + (0.25 * d.clr()).max(0.1)
}

/// The case as plain stock round the set: what it must clear and how big it is, worked out from
/// the planned members themselves (before the drive is placed round it).
struct Layout {
    /// The inside turned out of the case, top to bottom along the axle: (radius, z) vertices of
    /// the side gears' turning envelope (hubs included) grown by the running clearance.
    inner: Vec<V2>,
    /// Cuts: each pinion's or element's own turning envelope about its axis (a revolve in its
    /// frame), and the pin bores (the pins' own shape).
    pockets: Vec<Step>,
    /// Outer radius round the gears and the z range it runs over.
    r_o: f64,
    z: (f64, f64),
    bearing: (String, (f64, f64, f64)),
}

/// The most radius about an axis of a cloud of points (z, r), per band `dz` wide from `z0`,
/// each band taking the points within `reach` of it, grown by `grow`: (band low edge, radius).
fn bands(pts: &[(f64, f64)], dz: f64, reach: f64, grow: f64) -> Vec<(f64, f64)> {
    let (lo, hi) = pts.iter().fold((f64::MAX, f64::MIN), |a, q| (a.0.min(q.0), a.1.max(q.0)));
    let (lo, hi) = (lo - reach, hi + reach);
    let n = (((hi - lo) / dz).ceil() as usize).max(1);
    let mut own = vec![0.0f64; n];
    for (z, r) in pts {
        let k = (((z - lo) / dz).floor() as usize).min(n - 1);
        own[k] = own[k].max(*r);
    }
    let w = (reach / dz).ceil() as usize;
    (0..n)
        .map(|k| {
            let r = own[k.saturating_sub(w)..(k + w + 1).min(n)].iter().fold(0.0f64, |a, b| a.max(*b));
            (lo + dz * k as f64, if r > 0.0 { r + grow } else { 0.0 })
        })
        .collect()
}

/// The surface points of every part that adds material to a body (each part on its own: a
/// whole body's points can miss parts joined after others), in the body's own coordinates.
fn body_points(b: &Body, h: f64, hz: f64) -> Vec<V3> {
    let mut out = Vec::new();
    for st in &b.steps {
        let op = match st {
            Step::Prism { op, .. }
            | Step::Taper { op, .. }
            | Step::RingTaper { op, .. }
            | Step::Loft { op, .. }
            | Step::Revolve { op, .. }
            | Step::Coil { op, .. }
            | Step::Group { op, .. } => *op,
            Step::Placed { step, .. } => match step.as_ref() {
                Step::Prism { op, .. }
                | Step::Taper { op, .. }
                | Step::RingTaper { op, .. }
                | Step::Loft { op, .. }
                | Step::Revolve { op, .. }
                | Step::Coil { op, .. }
                | Step::Group { op, .. } => *op,
                Step::Placed { .. } => Op::Cut,
            },
        };
        if matches!(op, Op::New | Op::Join) {
            let mut one = st.clone();
            set_op(&mut one, Op::New);
            out.extend(Solid::new(&Body { name: String::new(), frame: Frame::at([0.0; 3]), steps: vec![one] }, h).material_points(hz));
        }
    }
    out
}

fn case_layout(s: &Spec, core: &Core) -> Result<Layout, String> {
    let d = &s.diff;
    let clr = d.clr();
    let m = d.module;
    let h = (0.05 * m).clamp(0.02, 0.25);
    let dz = (0.25 * m).clamp(0.2, 1.0);
    let ax = axle_bore(d);
    // The side gears turn on the axle: their envelope, hubs and all.
    let mut pts = Vec::new();
    for (b, r) in core.bodies.iter().zip(&core.roles) {
        if *r == Role::Side {
            for q in body_points(b, h, dz) {
                let w = b.frame.world(q);
                pts.push((w[2], w[0].hypot(w[1])));
            }
        }
    }
    if pts.is_empty() {
        return Err("the differential has no side gears to put a case round".into());
    }
    let env = bands(&pts, dz, clr + dz, clr);
    let mut inner = Vec::new();
    // Top to bottom: each band's radius over its stretch (never under the axle bore).
    let top = env.last().map_or(0.0, |b| b.0 + dz);
    inner.push(V2::new(ax, top));
    for (z0, r) in env.iter().rev() {
        let r = r.max(ax);
        inner.push(V2::new(r, z0 + dz));
        inner.push(V2::new(r, *z0));
    }
    inner.push(V2::new(ax, env[0].0));
    // Merge runs of one radius.
    inner.dedup_by(|b, a| (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9);
    let mut simple: Vec<V2> = Vec::new();
    for q in inner {
        let n = simple.len();
        if n >= 2 && (simple[n - 1].x - q.x).abs() < 1e-9 && (simple[n - 2].x - q.x).abs() < 1e-9 {
            simple[n - 1] = q;
        } else {
            simple.push(q);
        }
    }
    let mut zr = (env[0].0, top);
    let mut reach = env.iter().fold(0.0f64, |a, b| a.max(b.1));
    // The pinions or elements turn on their own axes in the case: a pocket each, their own
    // envelope about that axis grown by the clearance.
    let mut pockets = Vec::new();
    for (b, r) in core.bodies.iter().zip(&core.roles) {
        match r {
            Role::Carried => {
                let local: Vec<(f64, f64)> = body_points(b, h, dz).iter().map(|q| (q[2], q[0].hypot(q[1]))).collect();
                let bs = bands(&local, dz, clr + dz, clr);
                let mut profile = vec![V2::new(0.0, bs[0].0)];
                for (z0, r) in &bs {
                    let r = r.max(clr);
                    profile.push(V2::new(r, *z0));
                    profile.push(V2::new(r, z0 + dz));
                }
                profile.push(V2::new(0.0, bs[bs.len() - 1].0 + dz));
                pockets.push(Step::Placed { frame: b.frame, step: Box::new(Step::Revolve { profile, op: Op::Cut }) });
                for q in swept_all(b, h, 36) {
                    reach = reach.max(q[0].hypot(q[1]));
                    zr = (zr.0.min(q[2] - clr), zr.1.max(q[2] + clr));
                }
            }
            Role::Fixed => {
                // The pin bores: the pins themselves.
                for st in &b.steps {
                    let mut st = st.clone();
                    set_op(&mut st, Op::Cut);
                    pockets.push(st);
                }
            }
            Role::Side => {}
        }
    }
    let wall = core.wall;
    // The bearings on plain journals at the ends, round the axle bore with a wall.
    let wall_tr = (0.12 * d.axle_diameter).max(2.0);
    let need = 2.0 * (ax + wall_tr);
    let bearing = if d.bearing.trim().is_empty() {
        pick_bearing(need).ok_or_else(|| format!("no case bearing in the library has a bore of Ø{} or more: give a smaller axle", num(need)))?
    } else {
        let b = case_bearing(&d.bearing).ok_or_else(|| format!("no bearing is called '{}'", d.bearing.trim()))?;
        if b.0 < need - 1e-9 {
            return Err(format!(
                "bearing {} has a Ø{} bore: the journal round the axle bore needs Ø{} or more",
                d.bearing.trim(),
                num(b.0),
                num(need)
            ));
        }
        (d.bearing.trim().to_string(), b)
    };
    let r_o = (reach + wall).max(core.case_r).max(bearing.1.0 / 2.0 + 1.0);
    Ok(Layout { inner: simple, pockets, r_o, z: (zr.0 - wall, zr.1 + wall), bearing })
}

/// The case's outer profile, (radius, z) rising in z: for the clearance checks.
struct CaseShape {
    outer: Vec<V2>,
    flange: Option<(f64, f64, f64)>,
    bearing: (String, (f64, f64, f64)),
    /// Middle of each bearing seat (z).
    seats: [f64; 2],
}

impl CaseShape {
    /// The case's outer radius at z (the flange included).
    fn radius(&self, z: f64) -> f64 {
        let mut r: f64 = 0.0;
        for w in self.outer.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (lo, hi) = (a.y.min(b.y), a.y.max(b.y));
            if z >= lo && z <= hi {
                let t = if (b.y - a.y).abs() > 1e-12 { (z - a.y) / (b.y - a.y) } else { 0.0 };
                r = r.max(a.x + (b.x - a.x) * t).max(if (b.y - a.y).abs() <= 1e-12 { a.x.max(b.x) } else { 0.0 });
            }
        }
        if let Some((rf, z0, z1)) = self.flange
            && z >= z0
            && z <= z1
        {
            r = r.max(rf);
        }
        r
    }
}

/// The case: plain stock (one revolve: a cylinder round the gears with a plain journal at each
/// end for the bearings), the side gears' envelope and the axle bore turned out of it, a pocket
/// for each pinion or element and the pin bores; with an integral ring, the ring's teeth joined
/// to it; with ring bolts asked for, a flange and its holes.
fn case_body(s: &Spec, core: &Core, lay: &Layout, drive: Option<&Drive>, p: &mut Plan) -> Result<(Body, CaseShape), String> {
    let d = &s.diff;
    let wall = core.wall;
    let ax = axle_bore(d);
    let (bd, bod, bw) = lay.bearing.1;
    let r_tr = bd / 2.0;
    let r_o = lay.r_o;
    let (mut zc0, mut zc1) = lay.z;
    let mut flange = None;
    if let Some(dr) = drive {
        match d.mount {
            Mount::Bolted => {
                if let Some((rf, t)) = dr.flange {
                    flange = Some((rf, dr.z_back - t, dr.z_back));
                    zc0 = zc0.min(dr.z_back - t);
                    zc1 = zc1.max(dr.z_back);
                }
            }
            Mount::Integral => {
                zc0 = zc0.min(dr.z_back);
                zc1 = zc1.max(dr.z_top);
            }
        }
    }
    let seat = bw + 2.0;
    let (zt0, zt1) = (zc0 - seat, zc1 + seat);
    let outer = vec![V2::new(r_tr, zt0), V2::new(r_tr, zc0), V2::new(r_o, zc0), V2::new(r_o, zc1), V2::new(r_tr, zc1), V2::new(r_tr, zt1)];
    // One revolve: the outside up, the inside (axle bore, the side gears' envelope) back down.
    let mut profile = outer.clone();
    profile.push(V2::new(ax, zt1));
    profile.extend(lay.inner.iter().copied());
    profile.push(V2::new(ax, zt0));
    profile.dedup_by(|b, a| (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9);
    let mut steps = Vec::new();
    if let (Some(dr), Mount::Integral) = (drive, d.mount) {
        // The ring's teeth first (its own bore and holes left out), its middle taken out inside
        // the case's wall; the case then joins round it.
        for st in &dr.ring.steps {
            let op = match st {
                Step::Prism { op, .. }
                | Step::Taper { op, .. }
                | Step::RingTaper { op, .. }
                | Step::Loft { op, .. }
                | Step::Revolve { op, .. }
                | Step::Coil { op, .. }
                | Step::Group { op, .. } => *op,
                Step::Placed { .. } => {
                    return Err("the integral mount can't take this ring gear's details (a step in its own frame): use the separate ring".into());
                }
            };
            if op == Op::Cut || op == Op::Intersect {
                continue;
            }
            let mut st = st.clone();
            set_op(&mut st, if steps.is_empty() { Op::New } else { Op::Join });
            steps.push(Step::Placed { frame: dr.ring.frame, step: Box::new(st) });
        }
        let r_in = r_o - 0.5 * wall;
        steps.push(Step::Prism { region: circle(V2::ZERO, r_in), z0: dr.z_back - 1.0, height: dr.z_top - dr.z_back + 2.0, twist: 0.0, op: Op::Cut });
    }
    let op = if steps.is_empty() { Op::New } else { Op::Join };
    steps.push(Step::Revolve { profile, op });
    if let Some((rf, f0, f1)) = flange {
        steps.push(Step::Revolve { profile: vec![V2::new(r_o - 0.5, f0), V2::new(rf, f0), V2::new(rf, f1), V2::new(r_o - 0.5, f1)], op: Op::Join });
    }
    steps.extend(lay.pockets.iter().cloned());
    steps.extend(core.cuts.iter().cloned());
    if let (Some(dr), Some((n, b, rbc))) = (drive, drive.and_then(|x| x.bolts)) {
        let holes = crate::hub::lightening_holes(n, 1.1 * b, rbc, PI / f64::from(n));
        let (_, f0, f1) = flange.unwrap_or((0.0, dr.z_back - wall, dr.z_back));
        steps.push(Step::Prism { region: Region { outer: holes, holes: vec![] }, z0: f0 - 1.0, height: f1 - f0 + 1.0, twist: 0.0, op: Op::Cut });
    }
    p.fact("case outside diameter", 2.0 * r_o, Unit::Mm);
    p.fact("case length (journal ends)", zt1 - zt0, Unit::Mm);
    p.fact("case wall", wall, Unit::Mm);
    p.fact(format!("case bearings {} bore", lay.bearing.0), bd, Unit::Mm);
    p.fact(format!("case bearings {} outside diameter", lay.bearing.0), bod, Unit::Mm);
    p.fact(format!("case bearings {} width", lay.bearing.0), bw, Unit::Mm);
    p.fact("case bearing spread (seat middles)", (zt1 - zt0) - seat, Unit::Mm);
    p.fact("axle bore", 2.0 * ax, Unit::Mm);
    if let Some((rf, f0, f1)) = flange {
        p.fact("flange diameter", 2.0 * rf, Unit::Mm);
        p.fact("flange thickness", f1 - f0, Unit::Mm);
    }
    let body = Body { name: named(s, "Differential case"), frame: Frame::at([0.0; 3]), steps };
    Ok((body, CaseShape { outer, flange, bearing: lay.bearing.clone(), seats: [zt0 + seat / 2.0, zt1 - seat / 2.0] }))
}
fn set_op(st: &mut Step, to: Op) {
    match st {
        Step::Prism { op, .. }
        | Step::Taper { op, .. }
        | Step::RingTaper { op, .. }
        | Step::Loft { op, .. }
        | Step::Revolve { op, .. }
        | Step::Coil { op, .. }
        | Step::Group { op, .. } => *op = to,
        Step::Placed { step, .. } => set_op(step, to),
    }
}

/// The least gap between the drive pinion, turning about its own axis, and the case turning
/// about the axle (mm; negative: they hit).
fn pinion_to_case(pinion: &Body, case: &CaseShape, h: f64) -> f64 {
    let solid = Solid::new(pinion, h);
    let mut least = f64::INFINITY;
    let pts = solid.material_points(4.0 * h);
    for q in pts {
        let r = q[0].hypot(q[1]);
        for k in 0..36 {
            let t = TAU * f64::from(k) / 36.0;
            let w = pinion.frame.world([r * t.cos(), r * t.sin(), q[2]]);
            let rc = case.radius(w[2]);
            if rc > 0.0 {
                least = least.min(w[0].hypot(w[1]) - rc);
            }
        }
    }
    least
}

// ---------------------------------------------------------------------------------------------
// Strength.

fn rate(s: &Spec, core: &Core, drive: Option<&Drive>, p: &mut Plan) {
    use crate::strength::{Member, MeshLoad, SF_MIN, rate_mesh};
    let d = &s.diff;
    let l = &s.load;
    let t_in = l.torque_nm();
    let ratio = drive.map_or(1.0, |x| x.ratio);
    let t_case = t_in * ratio;
    p.fact("load: case torque (N·m)", t_case, Unit::None);
    p.fact("load: side gear A torque (N·m)", t_case * core.split[0], Unit::None);
    p.fact("load: side gear B torque (N·m)", t_case * core.split[1], Unit::None);
    let t_side = t_case * core.split[0].max(core.split[1]);
    let tau = 16.0 * t_side * 1000.0 / (PI * d.axle_diameter.powi(3));
    p.fact("load: axle shear stress (MPa)", tau, Unit::None);
    let n = f64::from(d.pinions.max(1));
    let pa = d.pa().to_radians();
    let ka = l.application_factor;
    let mut least_sf = f64::INFINITY;
    let mut least_sh = f64::INFINITY;
    let mut rate_one = |p: &mut Plan, label: &str, ga: Involute, gb: Involute, face: f64, ft: f64, k_beta: f64| {
        let mesh = mesh_external(&ga, &gb, face);
        let ml = MeshLoad {
            members: [
                Member { name: "a".into(), gear: ga, internal: false, material: l.material, cycles: 0.0, idler: false },
                Member { name: "b".into(), gear: gb, internal: false, material: l.material2, cycles: 0.0, idler: true },
            ],
            mesh,
            face,
            overlap: mesh.overlap_ratio,
            ft,
            v: 0.0,
            ka,
            quality: l.quality,
            k_beta,
        };
        let r = rate_mesh(&ml);
        p.fact(format!("{label}: tangential force Ft (N)"), ft, Unit::None);
        for (i, who) in label.split(" – ").enumerate().take(2) {
            let mr = &r.members[i];
            p.fact(format!("{label}: {who} root stress σF (MPa)"), mr.sigma_f, Unit::None);
            p.fact(format!("{label}: {who} bending safety SF"), mr.s_f, Unit::None);
            p.fact(format!("{label}: {who} contact safety SH"), mr.s_h, Unit::None);
            least_sf = least_sf.min(mr.s_f);
            least_sh = least_sh.min(mr.s_h);
            if mr.s_f < SF_MIN {
                p.warn(format!("{who}: bending safety SF {:.2} under {SF_MIN} at the rated torque: use a bigger module or wider face", mr.s_f));
            }
        }
    };
    if d.style.bevel() {
        let Ok(g) = crate::plan::bevel_planet::geometry(&Spec {
            kind: Kind::BevelPlanetary,
            bevel_planet_mode: BevelPlanetMode::Differential,
            module: d.module,
            teeth: d.side_teeth,
            teeth2: d.pinion_teeth,
            teeth3: if d.style == Style::Biased { d.side_b() } else { 0 },
            planets: d.pinions,
            face_width: core.face,
            shaft_angle: 90.0,
            spiral_angle: d.spiral,
            ..Spec::defaults_for(Kind::BevelPlanetary)
        }) else {
            return;
        };
        let bm = d.spiral.to_radians();
        let mt = d.module * (1.0 - 0.5 * g.face / g.re);
        let virt = |z: u32, delta: f64, helix: f64| Involute {
            teeth: z,
            module: mt * bm.cos(),
            pressure_angle: pa,
            helix,
            shift: 0.0,
            rack: Default::default(),
            backlash: 0.0,
            tip_shortening: 0.0,
            virtual_teeth: f64::from(z) / delta.cos(),
        };
        let za = d.side_teeth;
        let zb = if d.style == Style::Biased { d.side_b() } else { za };
        // One force on each pinion from both side gears (the pinion is a balanced lever).
        let ft = 2000.0 * t_case * core.split[0] / (mt * f64::from(za) * n);
        rate_one(p, "side gear A – pinion", virt(za, g.delta_sun, bm), virt(d.pinion_teeth, g.delta_planet, -bm), 0.85 * g.face, ft, 1.5);
        rate_one(p, "pinion – side gear B", virt(d.pinion_teeth, g.delta_planet, -bm), virt(zb, PI - g.delta_far, bm), 0.85 * g.face, ft, 1.5);
        // The pin carries both tooth forces of its pinion.
        let pin = p.facts.iter().find(|f| f.label == "pinion pin diameter").map_or(0.0, |f| f.value);
        if pin > 0.0 {
            let tau = 2.0 * ft / (PI * pin * pin / 4.0);
            p.fact("load: pinion pin shear stress (MPa)", tau, Unit::None);
            if tau > 250.0 {
                p.warn(format!(
                    "the pinion pins' shear stress ({} MPa) is over 250 MPa (hardened steel): use bigger pins or more pinions",
                    num(tau.round())
                ));
            }
        }
    } else if d.style == Style::TorsenB {
        let base = Spec { module: d.module, pressure_angle: d.pa(), ..Spec::default() };
        let gs = base.involute(d.side_teeth, 0.0, d.helix);
        let ge = base.involute(d.pinion_teeth, 0.0, -d.helix);
        let face = if d.face_width > 0.0 { d.face_width } else { 8.0 * d.module };
        let ft = 2000.0 * t_case * 0.5 / (2.0 * gs.r() * n);
        rate_one(p, "side gear – element", gs, ge, face, ft, 1.2);
        let ge2 = base.involute(d.pinion_teeth, 0.0, d.helix);
        rate_one(p, "element – element", ge, ge2, face, ft, 1.2);
        p.fact("load: side gear axial force (N)", ft * n * d.helix.to_radians().tan(), Unit::None);
    }
    if least_sf.is_finite() {
        p.fact("differential minimum bending safety SF (static)", least_sf, Unit::None);
        p.fact("differential minimum contact safety SH (static)", least_sh, Unit::None);
        p.warn("differential gears are rated statically (speed 0, endurance limits, K_A from the load): side and pinion gears turn only when a wheel slips");
    }
}

// ---------------------------------------------------------------------------------------------
// The whole assembly.

/// Plans a differential (`Spec::kind` = [`Kind::Differential`]).
pub(crate) fn build(s: &Spec, p: &mut Plan) -> Result<(), String> {
    // An integral ring with the case or the ring left out: planned as a separate ring, so the one
    // that is built stands on its own.
    let adjusted;
    let s = if s.diff.mount == Mount::Integral && s.diff.drive != DriveKind::None && (!s.diff.parts.case || !s.diff.parts.ring) {
        p.warn("the integral ring is planned as a separate ring here, since the case or the ring is left out");
        adjusted = Spec { diff: Diff { mount: Mount::Bolted, ..s.diff.clone() }, ..s.clone() };
        &adjusted
    } else {
        s
    };
    let d = &s.diff;
    let mut core = match d.style {
        Style::Open | Style::Biased => bevel_core(s, p)?,
        Style::TorsenB => torsen_b_core(s, p)?,
        Style::TorsenA => torsen_a_core(s, p)?,
    };
    // The case at least clears its bearings' shoulders (the ring's bore goes round it).
    let lay = case_layout(s, &core)?;
    core.case_r = lay.r_o;
    let drive = plan_drive(s, &core, lay.r_o, p)?;
    let (case, shape) = case_body(s, &core, &lay, drive.as_ref(), p)?;
    if let Some(dr) = &drive {
        let gap = pinion_to_case(&dr.pinion, &shape, (0.05 * d.drive_module).clamp(0.05, 0.5));
        p.fact("drive pinion to case clearance", gap, Unit::Mm);
        if gap < d.clr() * 0.5 {
            return Err(format!(
                "the drive pinion runs into the case ({} mm): use a bigger ring gear, move the ring along the axle (drive position) or a smaller case",
                num(gap)
            ));
        }
        p.fact("final drive ratio", dr.ratio, Unit::None);
        p.fact("ring gear pitch diameter", 2.0 * dr.radii.1, Unit::Mm);
        p.fact("ring gear least root diameter", 2.0 * dr.root, Unit::Mm);
        if dr.bore > 0.0 {
            p.fact("ring gear pilot bore", 2.0 * dr.bore, Unit::Mm);
        }
        if let Some((n, b, rbc)) = dr.bolts {
            p.fact("ring bolts", f64::from(n), Unit::None);
            p.fact("ring bolt size (M)", b, Unit::None);
            p.fact("ring bolt circle diameter", 2.0 * rbc, Unit::Mm);
        }
    }
    // Speeds and torque split.
    let (za, zb) = (f64::from(d.side_teeth), f64::from(if d.style == Style::Biased { d.side_b() } else { d.side_teeth }));
    let (fa, fb) = (core.split[0], core.split[1]);
    p.fact("torque share side A", fa, Unit::None);
    p.fact("torque share side B", fb, Unit::None);
    if d.style.bevel() {
        p.fact("torque split ratio T_B / T_A (side B over side A)", zb / za, Unit::None);
        p.fact("side B speed with side A held (× case speed)", (za + zb) / zb, Unit::None);
        p.fact("side A speed with side B held (× case speed)", (za + zb) / za, Unit::None);
        if d.style == Style::Biased {
            p.warn(format!(
                "uneven side gears split the case's torque {}:{} (side A : side B) always, at any speed: zA·nA + zB·nB = (zA + zB)·n_case; with equal wheel speeds both sides turn with the case",
                num(za),
                num(zb)
            ));
        }
    }
    if s.load.rated() {
        rate(s, &core, drive.as_ref(), p);
    }

    // Bodies: case, ring, drive pinion, then the set.
    let mut bodies = vec![case];
    let mut roles: Vec<Option<Role>> = vec![None];
    let mut ring_i = None;
    let mut pin_i = None;
    if let Some(dr) = &drive {
        if d.mount == Mount::Bolted {
            ring_i = Some(bodies.len());
            bodies.push(dr.ring.clone());
            roles.push(None);
        }
        pin_i = Some(bodies.len());
        bodies.push(dr.pinion.clone());
        roles.push(None);
    }
    let first = bodies.len();
    for (b, r) in core.bodies.iter().zip(&core.roles) {
        bodies.push(b.clone());
        roles.push(Some(*r));
    }
    // Meshes: the set's and the drive's (against the case when the ring is part of it).
    let mut meshes = core.meshes.clone();
    if let Some(dr) = &drive {
        let mut mp = dr.mesh.clone();
        if d.mount == Mount::Integral {
            mp.bodies = Some([dr.pinion.clone(), bodies[0].clone()]);
        }
        meshes.push(mp);
    }
    // Which bodies are built: the part toggles (and `only`, kept for old requests: 1 the gear set,
    // 2 the case and pins, 3 the drive). The whole differential is designed either way.
    let parts = &d.parts;
    let mut carried_k = 0u32;
    let keep: Vec<bool> = (0..bodies.len())
        .map(|i| {
            let part = if i == 0 {
                parts.case
            } else if Some(i) == ring_i {
                parts.ring
            } else if Some(i) == pin_i {
                parts.drive_pinion
            } else {
                match roles[i] {
                    Some(Role::Side) if i == first => parts.side_a,
                    Some(Role::Side) => parts.side_b,
                    Some(Role::Carried) => {
                        carried_k += 1;
                        parts.pinions && (parts.which_pinions.is_empty() || parts.which_pinions.contains(&carried_k))
                    }
                    Some(Role::Fixed) => parts.pins,
                    None => true,
                }
            };
            let old = match s.only {
                0 => true,
                1 => i >= first && roles[i] != Some(Role::Fixed),
                2 => i == 0 || roles[i] == Some(Role::Fixed),
                _ => i > 0 && i < first,
            };
            part && old
        })
        .collect();
    // Only what is built: the meshes between built members, and the assembly over them.
    let built = |name: &str| bodies.iter().zip(&keep).any(|(b, k)| *k && b.name == name);
    meshes.retain(|mp| mp.bodies.as_ref().is_none_or(|[a, b]| built(&a.name) && built(&b.name)));
    let mut new_index = vec![None; bodies.len()];
    let mut n_kept = 0;
    for (i, k) in keep.iter().enumerate() {
        if *k {
            new_index[i] = Some(n_kept);
            n_kept += 1;
        }
    }
    // The assembly: case on the axle, everything else on its axis, carried members on the case
    // (when it isn't built, they turn on fixed axes: the case held).
    let case_i = new_index[0];
    let mut asm = Assembly::default();
    for (i, b) in bodies.iter().enumerate() {
        if !keep[i] {
            continue;
        }
        let carrier = match roles[i] {
            Some(Role::Carried) => case_i,
            _ => None,
        };
        asm.members.push(AssemblyMember { body: asm.members.len(), axis: Axis { origin: b.frame.origin, dir: b.frame.z }, grounded: false, carrier });
    }
    // The links as a tree (each member driven by one link at most), over the built members: the
    // drive pinion turns the ring, the ring the case, the case holds the pins; side gear A turns
    // every pinion or element chain (their turns relative to the case), side gear B follows the
    // first built pinion (or element pair). Side gear A is left free: an open differential has
    // two degrees of freedom.
    let radius = |a: usize, b: usize| -> Option<(f64, f64)> {
        if let (Some(dr), Some(pi)) = (&drive, pin_i)
            && a == pi
            && (Some(b) == ring_i || b == 0)
        {
            return Some((dr.radii.0, dr.radii.1));
        }
        core.links.iter().find(|(x, y, ..)| first + x == a && first + y == b).map(|(_, _, ra, rb)| (*ra, *rb))
    };
    let index = |name: &str| bodies.iter().position(|b| b.name == name);
    let mut driven = vec![false; bodies.len()];
    for mp in &meshes {
        let Some([ba, bb]) = &mp.bodies else { continue };
        let (Some(a), Some(b)) = (index(&ba.name), index(&bb.name)) else { continue };
        let (Some(na), Some(nb)) = (new_index[a], new_index[b]) else { continue };
        if driven[b] || mp.ratio == 0.0 || !mp.ratio.is_finite() {
            continue;
        }
        let Some((ra, _)) = radius(a, b) else { continue };
        driven[b] = true;
        asm.links.push(AssemblyLink::Gear { a: na, b: nb, ra, rb: ra / mp.ratio.abs(), internal: mp.ratio > 0.0, reverse: false });
    }
    if let (Some(r), Some(c)) = (ring_i.and_then(|r| new_index[r]), case_i) {
        asm.links.push(AssemblyLink::Rigid { a: r, b: c });
    }
    for (i, r) in roles.iter().enumerate() {
        if *r == Some(Role::Fixed)
            && let (Some(c), Some(pi)) = (case_i, new_index[i])
        {
            asm.links.push(AssemblyLink::Rigid { a: c, b: pi });
        }
    }
    if keep[first] && keep.iter().skip(first + 1).any(|k| *k) {
        p.warn("assembly: side gear A is left free to turn (an open differential has two degrees of freedom: the pinions and side gear B follow side gear A relative to the case)");
    }
    if !keep[0] && keep.iter().enumerate().any(|(i, k)| *k && roles[i] == Some(Role::Carried)) {
        p.warn("assembly: the case isn't built, so the pinions or elements turn on fixed axes (as if the case were held)");
    }
    if !keep.iter().all(|k| *k) {
        let left: Vec<String> = bodies.iter().zip(&keep).filter(|(_, k)| !**k).map(|(b, _)| b.name.clone()).collect();
        p.fact("bodies left out (designed with the set, not built)", left.len() as f64, Unit::None);
        p.warn(format!("not built (designed with the rest): {}", left.join(", ")));
    }
    let origin = s.origin;
    for (i, b) in bodies.iter().enumerate() {
        if keep[i] {
            p.bodies.push(shifted(b, origin));
        }
    }
    for m in &mut asm.members {
        for (o, d) in m.axis.origin.iter_mut().zip(origin) {
            *o += d;
        }
    }
    p.assembly = asm;
    p.meshes3d = meshes;
    p.preview = core.preview.clone();
    p.preview_note = core.preview_note.clone();
    if d.keep_out || s.clearance_body.is_some() {
        let n_outer = 1 + usize::from(drive.is_some()) + usize::from(ring_i.is_some());
        let outer: Vec<Body> = bodies[..n_outer].iter().map(|b| shifted(b, origin)).collect();
        p.clearance = Some(keep_out(s, outer, &shape, p)?);
    }
    Ok(())
}

/// The keep-out body: the space the turning assembly needs (the case with its ring and flange
/// turning on the axle, the drive pinion turning on its axis, the axle and the pinion shaft
/// running on through the housing, the case bearings' seats), grown by the clearances, for
/// cutting the axle housing with one Boolean. `bodies`: the case, a bolted ring, the drive
/// pinion (placed at the set's origin).
fn keep_out(s: &Spec, bodies: Vec<Body>, shape: &CaseShape, p: &mut Plan) -> Result<Body, String> {
    let d = &s.diff;
    let c = s.clearance_body.clone().unwrap_or_default();
    // The pinion last: the case's shaft is the axle, the pinion's its shank.
    let shank = if d.drive != DriveKind::None && bodies.len() >= 2 {
        let pin = &bodies[bodies.len() - 1];
        let joins: Vec<Step> = pin
            .steps
            .iter()
            .skip(1)
            .filter(|st| matches!(st, Step::Prism { op: Op::Join, .. }))
            .cloned()
            .map(|mut st| {
                set_op(&mut st, Op::New);
                st
            })
            .collect();
        if joins.is_empty() { 0.0 } else { 2.0 * Solid::new(&Body { name: String::new(), frame: pin.frame, steps: joins }, 0.5).radius }
    } else {
        0.0
    };
    // The case bearings' seats, unless the clearance names a bearing of its own.
    let mut c2 = c.clone();
    if c2.bearing.is_none() && c2.bearing_size.is_none() {
        let (bd, bod, bw) = shape.bearing.1;
        c2.bearing_size = Some(crate::clearance::BearingSize { bore: bd, od: bod, width: bw });
        let case = &bodies[0];
        let first = Solid::new(&Body { name: String::new(), frame: case.frame, steps: case.steps.iter().take(1).cloned().collect() }, 1.0);
        let mid = 0.5 * (first.z.0 + first.z.1);
        c2.seats = shape.seats.iter().map(|z| crate::clearance::Seat::Along { member: 1, along: z - mid }).collect();
    }
    let tmp = Spec {
        kind: Kind::Differential,
        module: d.module,
        face_width: 2.0 * d.axle_diameter,
        bore: Bore { kind: BoreKind::Round, diameter: d.axle_diameter, ..Bore::default() },
        bore2: if shank > 0.0 { Bore { kind: BoreKind::Round, diameter: shank, ..Bore::default() } } else { Bore::default() },
        clearance_body: Some(c2),
        name: s.name.clone(),
        ..Spec::default()
    };
    let mut tmp_plan = Plan::default();
    tmp_plan.bodies = bodies;
    let body = crate::clearance::clearance_body(&tmp, &mut tmp_plan)?;
    for f in tmp_plan.facts {
        p.facts.push(f);
    }
    for w in tmp_plan.warnings {
        // The seats sit on the case's trunnions (its first step is the whole case) and the
        // bearing's bore is the trunnion's, not the axle's: those notes don't apply.
        if w.contains("overlaps the teeth of") || w.contains("bore but") {
            continue;
        }
        p.warn(w);
    }
    Ok(Body { name: named(s, "Differential clearance"), ..body })
}

// ---------------------------------------------------------------------------------------------
// Design by outputs.

/// What a differential is designed from in Simple mode.
#[derive(Clone, Debug, PartialEq)]
pub struct Goal {
    pub style: Style,
    pub drive: DriveKind,
    pub mount: Mount,
    /// Axle shaft diameter (mm).
    pub axle_diameter: f64,
    /// The ring gear's pitch diameter (0: the smallest that fits round the case and carries the
    /// torque).
    pub ring_diameter: f64,
    /// Final drive ratio (ring / pinion; 0: 3.9).
    pub ratio: f64,
    /// Input torque at the drive pinion (N·m; 0: not rated, sized by the axle).
    pub torque: f64,
    /// Spider pinions or element pairs (0: 2 for an open diff, 3 for a Torsen).
    pub pinions: u32,
    /// Wanted torque split (biased bevel: T_B / T_A) or bias ratio (Torsen); 0: the style's usual.
    pub bias: f64,
    pub material: crate::strength::Material,
}

impl Default for Goal {
    fn default() -> Self {
        Self {
            style: Style::Open,
            drive: DriveKind::SpiralBevel,
            mount: Mount::Bolted,
            axle_diameter: 16.0,
            ring_diameter: 0.0,
            ratio: 3.9,
            torque: 0.0,
            pinions: 0,
            bias: 0.0,
            material: crate::strength::Material::default(),
        }
    }
}

/// A solved Simple-mode differential.
#[derive(Clone, Debug, PartialEq)]
pub struct Solved {
    pub spec: Spec,
    pub notes: Vec<String>,
}

const MODULES: [f64; 16] = [1.0, 1.25, 1.5, 1.75, 2.0, 2.25, 2.5, 2.75, 3.0, 3.5, 4.0, 4.5, 5.0, 5.5, 6.0, 7.0];

fn min_sf(p: &Plan) -> f64 {
    p.facts.iter().filter(|f| f.label.ends_with("bending safety SF")).map(|f| f.value).fold(f64::INFINITY, f64::min)
}

/// Picks teeth, modules and angles for a goal; `base` gives everything else (name, origin,
/// placement, clearance).
pub fn solve(goal: &Goal, base: &Spec) -> Result<Solved, String> {
    let mut notes = Vec::new();
    let axle = if goal.axle_diameter > 0.0 { goal.axle_diameter } else { 16.0 };
    let ratio = if goal.ratio > 0.0 { goal.ratio } else { 3.9 };
    let mut d = Diff::defaults_for(goal.style);
    d.axle_diameter = axle;
    d.drive = goal.drive;
    d.mount = goal.mount;
    if goal.pinions > 0 {
        d.pinions = goal.pinions;
    } else if goal.style == Style::TorsenB {
        d.pinions = 3;
    }
    let load = crate::strength::Load { torque: goal.torque, material: goal.material, material2: goal.material, ..Default::default() };
    // The gear set's teeth.
    match goal.style {
        Style::Open => {
            d.side_teeth = 16;
            d.pinion_teeth = 10;
            if d.pinions > 2 && 32 % d.pinions != 0 {
                d.side_teeth = if d.pinions == 3 { 18 } else { 15 };
            }
        }
        Style::Biased => {
            let want = if goal.bias > 1.0 {
                goal.bias
            } else if goal.bias > 0.0 {
                1.0 / goal.bias
            } else {
                1.5
            };
            let n = d.pinions.max(1);
            let mut best: Option<(f64, u32, u32, u32)> = None;
            for za in 10..=24u32 {
                for zp in 9..=16u32 {
                    let zb = (f64::from(za) * want).round() as u32;
                    if zb <= za || !(za + zb).is_multiple_of(n) {
                        continue;
                    }
                    let c = f64::from(zb - za) / (2.0 * f64::from(zp));
                    if c >= 0.5 {
                        continue;
                    }
                    // Both side gears' pitch cones under 72° (flatter ones take no full-depth teeth).
                    let sg = c.acos();
                    let ds = sg.sin().atan2(f64::from(zp) / f64::from(za) + sg.cos());
                    let dfar = PI - (2.0 * sg - ds);
                    if ds.to_degrees() > 72.0 || dfar.to_degrees() > 72.0 {
                        continue;
                    }
                    let err = (f64::from(zb) / f64::from(za) - want).abs() + 0.002 * f64::from(za + zp);
                    if best.is_none_or(|b| err < b.0) {
                        best = Some((err, za, zb, zp));
                    }
                }
            }
            let Some((_, za, zb, zp)) = best else { return Err(format!("no tooth counts give a {} torque split with {n} pinions", num(want))) };
            d.side_teeth = za;
            d.side_teeth2 = zb;
            d.pinion_teeth = zp;
            notes.push(format!(
                "side gears {za} and {zb} with {zp}-tooth pinions: torque split {} (asked {})",
                num((f64::from(zb) / f64::from(za) * 1000.0).round() / 1000.0),
                num(want)
            ));
            if goal.bias > 0.0 && goal.bias < 1.0 {
                notes.push("side B is the bigger one: swap the axles for a split under 1".into());
            }
        }
        Style::TorsenB => {
            d.side_teeth = 18;
            d.pinion_teeth = 11;
            if !(2 * d.side_teeth).is_multiple_of(d.pinions) {
                d.side_teeth = 20;
            }
            if goal.bias > 0.0 {
                // The helix that comes nearest the asked bias, then the friction it would need.
                let r_thrust = 0.6 * d.module * f64::from(d.side_teeth) / 2.0;
                let (mut best, mut at) = (f64::INFINITY, 30.0);
                for b in (15..=45).map(f64::from) {
                    let (t, ..) = torsen_b_bias(d.side_teeth, d.pinion_teeth, d.module, b, 20.0, d.friction, r_thrust);
                    if (t - goal.bias).abs() < best {
                        best = (t - goal.bias).abs();
                        at = b;
                    }
                }
                d.helix = at;
                let (t, ..) = torsen_b_bias(d.side_teeth, d.pinion_teeth, d.module, at, 20.0, d.friction, r_thrust);
                if (t - goal.bias).abs() > 0.1 {
                    let mut mu = d.friction;
                    for k in 1..=40 {
                        let m2 = 0.01 * f64::from(k);
                        let (t2, ..) = torsen_b_bias(d.side_teeth, d.pinion_teeth, d.module, at, 20.0, m2, r_thrust);
                        if t2 >= goal.bias {
                            mu = m2;
                            break;
                        }
                    }
                    notes.push(format!(
                        "a bias of {} needs friction μ ≈ {} at {}° helix (estimate; the helix alone reaches {}): friction surfaces or washers set it",
                        num(goal.bias),
                        num(mu),
                        num(at),
                        num((t * 100.0).round() / 100.0)
                    ));
                    d.friction = mu;
                }
            }
        }
        Style::TorsenA => {}
    }
    // The smallest module that takes the axle and (with a torque) the load.
    let mut chosen = None;
    let mut last = String::new();
    for &m in &MODULES {
        let mut t = d.clone();
        t.module = m;
        t.drive = DriveKind::None;
        t.keep_out = false;
        let s = Spec { kind: Kind::Differential, diff: t.clone(), load: load.clone(), only: 0, clearance_body: None, ..base.clone() };
        // The ring gear's torque reaches the set multiplied by the ratio.
        let s = Spec { load: crate::strength::Load { torque: goal.torque * ratio, ..load.clone() }, ..s };
        let p = match crate::plan::plan(&s) {
            Ok(p) => p,
            Err(e) => {
                last = e;
                continue;
            }
        };
        if goal.torque <= 0.0 || min_sf(&p) >= crate::strength::SF_MIN {
            let od = p.facts.iter().find(|f| f.label == "case outside diameter").map_or(0.0, |f| f.value);
            chosen = Some((m, od));
            break;
        }
    }
    let Some((m, case_od)) = chosen else {
        return Err(format!("no module up to {} fits a Ø{} axle and the load ({last})", num(MODULES[MODULES.len() - 1]), num(axle)));
    };
    d.module = m;
    notes.push(format!("differential gears: module {}", num(m)));
    // The drive.
    if d.drive != DriveKind::None {
        let zp = if ratio >= 5.0 {
            7
        } else if ratio >= 3.5 {
            10
        } else {
            13
        };
        let zp = if d.drive == DriveKind::Helical { zp + 6 } else { zp };
        // A hunting ratio (no common factor) near the asked one.
        let mut zr = (ratio * f64::from(zp)).round() as u32;
        let gcd = |mut a: u32, mut b: u32| {
            while b != 0 {
                (a, b) = (b, a % b);
            }
            a
        };
        if gcd(zr, zp) > 1 {
            for dz in [1i32, -1, 2, -2] {
                let z = (zr as i32 + dz) as u32;
                if gcd(z, zp) == 1 {
                    zr = z;
                    break;
                }
            }
        }
        d.drive_pinion_teeth = zp;
        d.ring_teeth = zr;
        notes.push(format!("final drive {zr}:{zp} = {}", num((f64::from(zr) / f64::from(zp) * 1000.0).round() / 1000.0)));
        if d.drive == DriveKind::Hypoid {
            d.drive_spiral = 45.0;
        }
        let try_m = |dm: f64| {
            let mut t = d.clone();
            t.drive_module = dm;
            t.keep_out = false;
            let s = Spec { kind: Kind::Differential, diff: t, load: load.clone(), clearance_body: None, ..base.clone() };
            crate::plan::plan(&s).ok().filter(|p| goal.torque <= 0.0 || min_sf(p) >= crate::strength::SF_MIN).map(|p| (s, p))
        };
        // From a ring a little bigger than the case, up in quarter modules.
        let k0 = ((1.3 * case_od / f64::from(zr)) / 0.25).floor().max(2.0) as u32;
        let search = |notes: &mut Vec<String>| {
            let r = (k0..k0 + 80).map(|k| 0.25 * f64::from(k)).find_map(|dm| try_m(dm).map(|x| (dm, x)));
            if r.is_none() {
                notes.push("no ring gear module up to 20 above the case's size plans".into());
            }
            r
        };
        let found = if goal.ring_diameter > 0.0 {
            let dm = goal.ring_diameter / f64::from(zr);
            match try_m(dm) {
                Some(x) => Some((dm, x)),
                None => {
                    notes.push(format!(
                        "a Ø{} ring gear doesn't fit round the case or carry the load; the smallest that does is used",
                        num(goal.ring_diameter)
                    ));
                    search(&mut notes)
                }
            }
        } else {
            search(&mut notes)
        };
        let Some((dm, (s, _))) = found else { return Err("no ring gear size fits round the case and carries the load".into()) };
        d.drive_module = dm;
        notes.push(format!("ring gear Ø{} (module {})", num(dm * f64::from(zr)), num(dm)));
        let _ = s;
    }
    d.keep_out = true;
    let spec = Spec { kind: Kind::Differential, diff: d, load, ..base.clone() };
    crate::plan::plan(&spec)?;
    Ok(Solved { spec, notes })
}

/// The Simple-mode goal a spec answers (for opening a planned differential in Simple mode).
pub fn goal_of(s: &Spec) -> Goal {
    let d = &s.diff;
    let ring_d = match d.drive {
        DriveKind::Helical => d.drive_module * f64::from(d.ring_teeth) / d.drive_helix.to_radians().cos(),
        _ => d.drive_module * f64::from(d.ring_teeth),
    };
    Goal {
        style: d.style,
        drive: d.drive,
        mount: d.mount,
        axle_diameter: d.axle_diameter,
        ring_diameter: ring_d,
        ratio: f64::from(d.ring_teeth) / f64::from(d.drive_pinion_teeth.max(1)),
        torque: s.load.torque,
        pinions: d.pinions,
        bias: match d.style {
            Style::Biased => f64::from(d.side_b()) / f64::from(d.side_teeth.max(1)),
            _ => 0.0,
        },
        material: s.load.material,
    }
}

/// The differential's schematic for the window's 3D preview: every planned body as its turning
/// envelope about its own axis (no teeth), from a plan without the keep-out body. Slower than
/// the other kinds' math-only schematics (the members are planned), so a front end should debounce
/// it.
pub fn schematic(spec: &Spec) -> crate::simple::Schematic {
    use crate::simple::Schematic;
    // The last few, kept: a window asks for the same one every frame until an edit.
    thread_local! {
        static LAST: std::cell::RefCell<Vec<(Spec, Schematic)>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    if let Some(hit) = LAST.with(|l| l.borrow().iter().find(|(s, _)| s == spec).map(|(_, sch)| sch.clone())) {
        return hit;
    }
    let sch = schematic_of(spec);
    LAST.with(|l| {
        let mut l = l.borrow_mut();
        l.push((spec.clone(), sch.clone()));
        if l.len() > 4 {
            l.remove(0);
        }
    });
    sch
}

fn schematic_of(spec: &Spec) -> crate::simple::Schematic {
    use crate::simple::{Member, Part, Role as R, Schematic, Shape};
    let mut s = spec.clone();
    s.diff.keep_out = false;
    s.clearance_body = None;
    let p = match crate::plan::plan(&s) {
        Ok(p) => p,
        Err(e) => return Schematic { note: Some(e), ..Schematic::default() },
    };
    let mut out = Schematic::default();
    for (i, b) in p.bodies.iter().enumerate() {
        let h = (0.25 * s.diff.module).clamp(0.2, 2.0);
        let solid = Solid::new(b, h);
        let pts = solid.material_points(2.0 * h);
        if pts.is_empty() {
            continue;
        }
        let (z0, z1) = pts.iter().fold((f64::MAX, f64::MIN), |a, q| (a.0.min(q[2]), a.1.max(q[2])));
        let n = 40usize;
        let dz = ((z1 - z0) / n as f64).max(1e-6);
        let mut r = vec![0.0f64; n + 1];
        for q in &pts {
            let k = (((q[2] - z0) / dz).round() as usize).min(n);
            r[k] = r[k].max(q[0].hypot(q[1]));
        }
        let mut profile = Vec::new();
        for (k, rr) in r.iter().enumerate() {
            if *rr > 0.0 {
                profile.push([*rr, z0 + dz * k as f64]);
            }
        }
        let role = if i == 0 { R::Body } else { R::Tip };
        let lower = b.name.to_lowercase();
        let name = if s.name.trim().is_empty() { lower } else { lower.trim_start_matches(&s.name.trim().to_lowercase()).trim().to_string() };
        out.members.push(Member {
            name,
            color: i,
            body: Some(i),
            frame: b.frame,
            pitch_diameter: 0.0,
            parts: vec![Part { role, shape: Shape::Revolved { profile } }],
        });
    }
    out
}

/// Used by `plan()` after placement: the assembly's axes follow the bodies.
pub(crate) fn place_assembly(p: &mut Plan) {
    for m in &mut p.assembly.members {
        if let Some(b) = p.bodies.get(m.body) {
            m.axis = Axis { origin: b.frame.origin, dir: b.frame.z };
        }
    }
}
