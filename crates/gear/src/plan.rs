//! What to build: a gear set described by a [`Spec`] becomes a [`Plan`] of bodies, each a short
//! list of solid steps (prisms, twisted prisms, lofts, revolves, coils) in its own frame, with the
//! numbers that describe the set, warnings, and 2D outlines for a meshing preview.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use crate::axes::num;
use crate::cycloid::{Cycloidal, CycloidalDrive, cycloidal_outline};
use crate::geom::{Outline, Region, V2, inv, wrap_period};
use crate::hub::{Bore, Chamfer, Hardware, Shaft, lightening_holes};
use crate::involute::{
    BasicRack, Internal, Involute, ToothFlank, ToothSides, contact_ratio_external, external_flank, external_outline, internal_tip_interference,
    mesh_external, mesh_internal, mesh_phase, rack_outline, shift_sum_for_center, sided_outline, teeth_outline,
};
use crate::octoid::Crown;
use crate::spline::SplineStandard;
use crate::worm::Hob;

pub(crate) mod assembly;
pub mod bevel_planet;
pub mod chamfer;
pub(crate) mod extras;
pub mod fused;
pub mod left_out;
pub mod solid;
pub mod system;
pub mod train;

pub use bevel_planet::BevelPlanetMode;
pub use train::{LinkKind, SidePreset, Train, TrainGear, TrainLink, TrainShaft};

/// Gear set types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum Kind {
    #[default]
    Spur,
    Helical,
    Herringbone,
    Internal,
    Rack,
    Pair,
    Planetary,
    Bevel,
    SpiralBevel,
    Hypoid,
    CrossedHelical,
    Worm,
    Cycloidal,
    CycloidalDrive,
    /// Roller chain sprocket (or a pair joined by the chain).
    Sprocket,
    /// Timing belt pulley (or a pair joined by the belt).
    Pulley,
    /// Involute spline shaft and hub.
    Spline,
    /// Face (crown) gear with its spur pinion.
    FaceGear,
    /// Elliptical gear and its conjugate (non-circular pair).
    Elliptical,
    /// Bevel sun with bevel planets whose shafts meet its axis at one apex at the cone angle
    /// (`shaft_angle`); optionally a second sun (differential) or a bevel ring.
    BevelPlanetary,
    /// A complete differential: side gears, pinions or element gears, the case and its drive gear
    /// (settings in `Spec::diff`; see [`crate::diff`]).
    Differential,
}

/// How the axes of a set lie, for grouping types in a picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axes {
    Parallel,
    Intersecting,
    Crossed,
    Special,
    /// Chains and belts.
    Wrapped,
}

impl Kind {
    pub const ALL: &[Kind] = &[
        Kind::Spur,
        Kind::Helical,
        Kind::Herringbone,
        Kind::Internal,
        Kind::Rack,
        Kind::Pair,
        Kind::Planetary,
        Kind::Bevel,
        Kind::SpiralBevel,
        Kind::Hypoid,
        Kind::CrossedHelical,
        Kind::Worm,
        Kind::Cycloidal,
        Kind::CycloidalDrive,
        Kind::Sprocket,
        Kind::Pulley,
        Kind::Spline,
        Kind::FaceGear,
        Kind::Elliptical,
        Kind::BevelPlanetary,
        Kind::Differential,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Kind::Spur => "spur",
            Kind::Helical => "helical",
            Kind::Herringbone => "herringbone",
            Kind::Internal => "internal",
            Kind::Rack => "rack",
            Kind::Pair => "pair",
            Kind::Planetary => "planetary",
            Kind::Bevel => "bevel",
            Kind::SpiralBevel => "spiral_bevel",
            Kind::Hypoid => "hypoid",
            Kind::CrossedHelical => "crossed_helical",
            Kind::Worm => "worm",
            Kind::Cycloidal => "cycloidal",
            Kind::CycloidalDrive => "cycloidal_drive",
            Kind::Sprocket => "sprocket",
            Kind::Pulley => "pulley",
            Kind::Spline => "spline",
            Kind::FaceGear => "face_gear",
            Kind::Elliptical => "elliptical",
            Kind::BevelPlanetary => "bevel_planetary",
            Kind::Differential => "differential",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Kind::Spur => "Spur gear",
            Kind::Helical => "Helical gear",
            Kind::Herringbone => "Herringbone gear",
            Kind::Internal => "Internal (ring) gear",
            Kind::Rack => "Rack",
            Kind::Pair => "Gear pair",
            Kind::Planetary => "Planetary set",
            Kind::Bevel => "Straight bevel",
            Kind::SpiralBevel => "Spiral bevel",
            Kind::Hypoid => "Hypoid (offset spiral bevel)",
            Kind::CrossedHelical => "Crossed helical",
            Kind::Worm => "Worm and wheel",
            Kind::Cycloidal => "Cycloidal (clock) gears",
            Kind::CycloidalDrive => "Cycloidal drive",
            Kind::Sprocket => "Chain sprocket",
            Kind::Pulley => "Timing pulley",
            Kind::Spline => "Involute spline",
            Kind::FaceGear => "Face (crown) gear",
            Kind::Elliptical => "Elliptical pair",
            Kind::BevelPlanetary => "Bevel planetary (differential)",
            Kind::Differential => "Differential (gears, case and drive)",
        }
    }
    pub fn axes(self) -> Axes {
        match self {
            Kind::Spur | Kind::Helical | Kind::Herringbone | Kind::Internal | Kind::Rack | Kind::Pair | Kind::Planetary => Axes::Parallel,
            Kind::Bevel | Kind::SpiralBevel | Kind::FaceGear | Kind::BevelPlanetary => Axes::Intersecting,
            Kind::Hypoid | Kind::CrossedHelical | Kind::Worm => Axes::Crossed,
            Kind::Cycloidal | Kind::CycloidalDrive | Kind::Spline | Kind::Elliptical | Kind::Differential => Axes::Special,
            Kind::Sprocket | Kind::Pulley => Axes::Wrapped,
        }
    }
    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.iter().copied().find(|k| k.name() == s)
    }
    /// Involute tooth form (module, pressure angle, shift, rack apply).
    pub fn involute(self) -> bool {
        !matches!(self, Kind::Cycloidal | Kind::CycloidalDrive | Kind::Sprocket | Kind::Pulley)
    }
    /// Makes two (or more) meshing members.
    pub fn is_set(self) -> bool {
        matches!(
            self,
            Kind::Pair
                | Kind::Planetary
                | Kind::Bevel
                | Kind::SpiralBevel
                | Kind::Hypoid
                | Kind::CrossedHelical
                | Kind::Worm
                | Kind::Cycloidal
                | Kind::CycloidalDrive
                | Kind::Sprocket
                | Kind::Pulley
                | Kind::Spline
                | Kind::FaceGear
                | Kind::Elliptical
                | Kind::BevelPlanetary
                | Kind::Differential
        )
    }
}

/// Everything that describes a gear set. Angles in degrees, lengths in millimetres; zero means
/// "work it out" where the field says so.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub kind: Kind,
    /// Normal module (bevel: at the large end; worm: axial module).
    pub module: f64,
    /// Gear 1 teeth (pinion, sun, the worm wheel's teeth for a worm set, the gear of a single type).
    pub teeth: u32,
    /// Gear 2 teeth (mate, planet).
    pub teeth2: u32,
    /// Ring teeth for a planetary set (0: sun + 2 × planet); bevel planetary: the second sun's
    /// or the ring's (0: sun + 2 × planet × cos Σ, rounded).
    pub teeth3: u32,
    pub planets: u32,
    pub pressure_angle: f64,
    /// Helix angle (spur types: 0; crossed helical: gear 1's).
    pub helix: f64,
    pub shift: f64,
    pub shift2: f64,
    pub rack: BasicRack,
    pub backlash: f64,
    pub face_width: f64,
    /// Pair: working centre distance (0: from the shifts; otherwise gear 2's shift is set to
    /// reach it).
    pub center_distance: f64,
    pub shaft_angle: f64,
    /// Spiral bevel: the mean spiral angle; hypoid: the pinion's mean spiral angle.
    pub spiral_angle: f64,
    /// Hypoid offset between the axes (0: a spiral bevel pair).
    pub offset: f64,
    /// No longer used (the old hyperboloid hypoid builder's face position).
    pub face_start: f64,
    /// Hypoid: the pinion's spiral is left-handed (the gear's right-handed), as in car axles.
    pub left_hand: bool,
    /// Worm threads (starts), pitch diameter (0: 10 modules) and length (0: enough to engage).
    pub starts: u32,
    pub worm_diameter: f64,
    pub worm_length: f64,
    /// Ring gear rim: radial thickness beyond the roots (0: 3 modules).
    pub rim: f64,
    /// Rack: teeth along it and its height below the pitch line (0: 3 modules).
    pub rack_teeth: u32,
    pub rack_height: f64,
    /// Cycloidal drive.
    pub pins: u32,
    pub pin_circle: f64,
    pub pin_diameter: f64,
    pub eccentricity: f64,
    pub bore: Bore,
    pub bore2: Bore,
    pub hub_diameter: f64,
    pub hub_length: f64,
    pub holes: u32,
    pub hole_diameter: f64,
    /// Which members to build: 0 all, 1 the first, 2 the second, 3 the third.
    pub only: u8,
    /// A gear pair's teeth as herringbone (two opposite helical halves; needs a helix angle);
    /// a spiral bevel's as V (chevron) teeth: the spiral at `spiral_angle` on the half of the face
    /// towards the apex and the opposite hand on the outer half, meeting at mid-face (the mate
    /// mirrored).
    pub herringbone: bool,
    /// Where the set goes (gear 1's centre).
    pub origin: [f64; 3],
    /// Body name prefix.
    pub name: String,
    /// Where the whole set goes: every body's frame, as built about `origin` on world +Z, is
    /// read in this frame (`None`: the world). The axis-first solver ([`crate::axes`]) sets it
    /// so gear 1 turns about the first picked axis.
    pub placement: Option<Frame>,
    /// Chain sprockets: the chain (an id from [`crate::sprocket::CHAINS`], or "custom" for the
    /// four sizes below), strands (1 simplex, 2 duplex, 3 triplex) and the ISO 606 tooth side
    /// chamfer. `teeth2` > 0 adds a driven sprocket at `center_distance`.
    pub chain: &'static str,
    pub chain_pitch: f64,
    pub roller_diameter: f64,
    /// The chain's width between its inner plates.
    pub roller_width: f64,
    /// The transverse pitch between strands.
    pub strand_pitch: f64,
    pub strands: u32,
    pub side_chamfer: bool,
    /// Timing pulleys: the belt (an id from [`crate::pulley::BELTS`]) and flanges; `face_width`
    /// is the belt's width. `teeth2` > 0 adds a second pulley at `center_distance`.
    pub belt: &'static str,
    pub flanges: bool,
    /// Involute splines: the standard, DIN 5480's reference diameter (0: module × (teeth +
    /// 1.1)), a fillet root instead of a flat one. `face_width` is the hub's length.
    pub spline_standard: SplineStandard,
    pub spline_diameter: f64,
    pub fillet_root: bool,
    /// Face gear: where the teeth start (0: at the undercut limit); `face_width` is their length.
    pub inner_diameter: f64,
    /// Elliptical pair: the ellipse's eccentricity (0 a circle); `teeth` on each gear.
    pub ellipse_eccentricity: f64,
    /// The load to rate the set for (default: none, not rated).
    pub load: crate::strength::Load,

    // ---- Tooth form tuning (see `crate::body`); all off at their defaults. ----
    /// Tip rounding radius in modules: each tip corner becomes an arc tangent to the flank and
    /// the tip circle (0: sharp corners).
    pub tip_round: f64,
    /// Horizontal-expansion (XY) compensation in mm: the tooth outline is drawn this far inside
    /// where the printed tooth should end up (flanks, tips and roots).
    pub xy_compensation: f64,
    /// Elephant-foot relief: the first `elephant_foot_height` (0: 0.2 mm) of the teeth is drawn
    /// this many mm inside the rest.
    pub elephant_foot: f64,
    pub elephant_foot_height: f64,
    /// Nozzle width in mm: a tip land under two of them is warned about (0: no check).
    pub nozzle: f64,
    /// Tip clearance coefficient c* (0: the rack's, dedendum − addendum); sets the dedendum.
    pub clearance: f64,
    /// Tip shortening in mm, off every gear's tip radius (on top of what a shifted pair needs).
    pub tip_shortening: f64,
    /// Tip diameter of gear 1 and of gear 2 in mm (0: from the rack; only lowers the tips).
    pub tip_diameter: f64,
    pub tip_diameter2: f64,
    /// Tip relief: the flank falls away by this much (mm, normal to it) at the tip, linearly
    /// over the top `tip_relief_height` mm of radius (0: 0.4 modules).
    pub tip_relief: f64,
    pub tip_relief_height: f64,
    /// Lead crowning in mm: each flank is this much thinner at the two faces than at mid face
    /// (straight teeth only).
    pub crowning: f64,
    /// Gear 2's own backlash in mm (mate, planets, the worm's thread); None: the same as
    /// `backlash`. The mesh's backlash is the two together.
    pub backlash2: Option<f64>,
    /// Pair: gear 2's face width (0: the same as gear 1's) and how far its mid plane sits from
    /// gear 1's along the axis. Hypoid (where `face_width` is the ring gear's): the pinion's face
    /// width along its pitch cone (0: covering the ring gear's face, a tenth of it more at each
    /// end) and how far its face sits towards its heel from there.
    pub face_width2: f64,
    pub face_offset2: f64,

    // ---- Chamfers. ----
    /// Tooth edge chamfers at the tip circle, on the front face (z = face width, the hub's side)
    /// and the back face (z = 0).
    pub tooth_chamfer_front: Chamfer,
    pub tooth_chamfer_back: Chamfer,
    /// 45° chamfer on both ends of the bores, and on the hub's outer edge (mm).
    pub bore_chamfer: f64,
    pub hub_chamfer: f64,

    // ---- Integral shafts: front (from the front face along +z) and back, for gear 1 and for
    // gear 2 (mate, planets, the worm). ----
    pub shaft_front: Shaft,
    pub shaft_back: Shaft,
    pub shaft2_front: Shaft,
    pub shaft2_back: Shaft,

    // ---- Hub hardware and bearing pockets, for gear 1 and gear 2. ----
    pub hardware: Hardware,
    pub hardware2: Hardware,
    /// Pair and planetary: plan a print test coupon instead of the set (a few teeth of each
    /// member at the true centre distance, and a base to hold them; see [`crate::coupon`]).
    pub coupon: bool,
    /// Teeth on each coupon piece (3 to 40; one more where that centres them on the mesh).
    pub coupon_teeth: u32,

    // ---- Bevel planetary. ----
    /// The first planet's azimuth about the sun's axis (degrees; the rest are patterned from it).
    pub first_azimuth: f64,
    /// What meshes the planets besides the sun.
    pub bevel_planet_mode: BevelPlanetMode,
    /// Also plan the set's clearance body (keep-out volume for a housing; see
    /// [`crate::clearance`]): `Plan::clearance`. None: not made.
    pub clearance_body: Option<crate::clearance::Clearance>,
    /// A differential's own settings (`Kind::Differential`; see [`crate::diff::Diff`]).
    pub diff: crate::diff::Diff,
    /// A gear train (any number of parallel-axis gears meshing outside, inside rings or on
    /// shared shafts; see [`train`]): when set, the set is this train and `kind` is its tooth
    /// style (spur, helical, herringbone). `module`, `teeth` and `teeth2` are gear 1's and
    /// gear 2's; gear 1 takes `bore` and the shaft and hardware settings, the others gear 2's.
    pub train: Option<Train>,
}

impl Default for Spec {
    fn default() -> Self {
        Self {
            kind: Kind::Spur,
            module: 2.0,
            teeth: 20,
            teeth2: 30,
            teeth3: 0,
            planets: 3,
            pressure_angle: 20.0,
            helix: 0.0,
            shift: 0.0,
            shift2: 0.0,
            rack: BasicRack::default(),
            backlash: 0.0,
            face_width: 10.0,
            center_distance: 0.0,
            shaft_angle: 90.0,
            spiral_angle: 35.0,
            offset: 0.0,
            face_start: 0.0,
            left_hand: true,
            starts: 1,
            worm_diameter: 0.0,
            worm_length: 0.0,
            rim: 0.0,
            rack_teeth: 10,
            rack_height: 0.0,
            pins: 12,
            pin_circle: 60.0,
            pin_diameter: 6.0,
            eccentricity: 1.5,
            bore: Bore::default(),
            bore2: Bore::default(),
            hub_diameter: 0.0,
            hub_length: 0.0,
            holes: 0,
            hole_diameter: 0.0,
            only: 0,
            herringbone: false,
            origin: [0.0; 3],
            name: String::new(),
            placement: None,
            chain: "08b",
            chain_pitch: 12.7,
            roller_diameter: 8.51,
            roller_width: 7.75,
            strand_pitch: 13.92,
            strands: 1,
            side_chamfer: true,
            belt: "gt2_2",
            flanges: true,
            spline_standard: SplineStandard::Din5480,
            spline_diameter: 0.0,
            fillet_root: false,
            inner_diameter: 0.0,
            ellipse_eccentricity: 0.3,
            load: crate::strength::Load::default(),
            tip_round: 0.0,
            xy_compensation: 0.0,
            elephant_foot: 0.0,
            elephant_foot_height: 0.0,
            nozzle: 0.0,
            clearance: 0.0,
            tip_shortening: 0.0,
            tip_diameter: 0.0,
            tip_diameter2: 0.0,
            tip_relief: 0.0,
            tip_relief_height: 0.0,
            crowning: 0.0,
            backlash2: None,
            face_width2: 0.0,
            face_offset2: 0.0,
            tooth_chamfer_front: Chamfer::default(),
            tooth_chamfer_back: Chamfer::default(),
            bore_chamfer: 0.0,
            hub_chamfer: 0.0,
            shaft_front: Shaft::default(),
            shaft_back: Shaft::default(),
            shaft2_front: Shaft::default(),
            shaft2_back: Shaft::default(),
            hardware: Hardware::default(),
            hardware2: Hardware::default(),
            coupon: false,
            coupon_teeth: 4,
            first_azimuth: 0.0,
            bevel_planet_mode: BevelPlanetMode::Single,
            clearance_body: None,
            diff: crate::diff::Diff::default(),
            train: None,
        }
    }
}

impl Spec {
    /// Sensible starting values for a type (keeps the module and tooth count already chosen).
    pub fn defaults_for(kind: Kind) -> Spec {
        let mut s = Spec { kind, ..Spec::default() };
        match kind {
            Kind::Helical | Kind::Herringbone => s.helix = 20.0,
            Kind::Internal => {
                s.teeth = 60;
                s.face_width = 10.0;
            }
            Kind::Pair => s.teeth2 = 32,
            Kind::Planetary => {
                s.teeth = 18;
                s.teeth2 = 18;
                // Ring teeth follow sun + 2 × planet unless asked (54 here): a fixed 54 left a
                // ring that no longer meshed once the sun or planets changed.
                s.teeth3 = 0;
                s.planets = 3;
            }
            Kind::Bevel => {
                s.teeth = 20;
                s.teeth2 = 30;
                s.face_width = 12.0;
            }
            Kind::SpiralBevel => {
                s.teeth = 17;
                s.teeth2 = 34;
                s.face_width = 12.0;
                s.spiral_angle = 35.0;
            }
            Kind::Hypoid => {
                // A car axle: 41:11, ring gear Ø200, pinion 25 mm below centre.
                s.teeth = 11;
                s.teeth2 = 41;
                s.module = 200.0 / 41.0;
                s.face_width = 30.0;
                s.offset = 25.0;
                s.spiral_angle = 50.0;
                // The ring gear's centre hole (it bolts to a carrier).
                s.bore2 = Bore { kind: crate::hub::BoreKind::Round, diameter: 80.0, ..Bore::default() };
            }
            Kind::CrossedHelical => {
                s.teeth = 15;
                s.teeth2 = 25;
                s.helix = 45.0;
                s.shaft_angle = 90.0;
            }
            Kind::Worm => {
                s.teeth = 30;
                s.starts = 1;
                s.module = 2.0;
                s.face_width = 12.0;
            }
            Kind::Cycloidal => {
                s.teeth = 60;
                s.teeth2 = 8;
                s.module = 1.0;
                s.face_width = 3.0;
            }
            Kind::CycloidalDrive => {
                s.face_width = 8.0;
            }
            Kind::Sprocket => {
                s.teeth = 17;
                s.teeth2 = 0;
            }
            Kind::Pulley => {
                s.teeth = 20;
                s.teeth2 = 0;
                s.face_width = 6.0;
            }
            Kind::Spline => {
                s.module = 1.25;
                s.teeth = 18;
                s.pressure_angle = 30.0;
                s.face_width = 20.0;
            }
            Kind::Elliptical => {
                s.module = 2.0;
                s.teeth = 30;
                s.face_width = 8.0;
            }
            Kind::BevelPlanetary => {
                // A car differential: two 16-tooth side gears and four 10-tooth spiders at 90°.
                s.module = 2.5;
                s.teeth = 16;
                s.teeth2 = 10;
                s.teeth3 = 0;
                s.planets = 4;
                s.shaft_angle = 90.0;
                s.spiral_angle = 0.0;
                s.face_width = 7.0;
                s.bevel_planet_mode = BevelPlanetMode::Differential;
            }
            Kind::Differential => {
                s.diff = crate::diff::Diff::default();
                s.module = s.diff.module;
                s.teeth = s.diff.side_teeth;
                s.teeth2 = s.diff.pinion_teeth;
            }
            Kind::FaceGear => {
                s.module = 2.0;
                s.teeth = 18;
                s.teeth2 = 40;
                s.face_width = 5.0;
            }
            _ => {}
        }
        s
    }

    pub(crate) fn involute(&self, teeth: u32, shift: f64, helix_deg: f64) -> Involute {
        Involute {
            teeth,
            module: self.module,
            pressure_angle: self.pressure_angle.to_radians(),
            helix: helix_deg.to_radians(),
            shift,
            rack: self.rack_eff(),
            backlash: self.backlash + self.xy_thinning(helix_deg),
            tip_shortening: self.tip_shortening + self.xy_compensation,
            virtual_teeth: 0.0,
        }
    }
}

/// A placement: origin, x and z directions (unit, perpendicular) in world coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub origin: [f64; 3],
    pub x: [f64; 3],
    pub z: [f64; 3],
}

impl Frame {
    pub fn at(origin: [f64; 3]) -> Frame {
        Frame { origin, x: [1.0, 0.0, 0.0], z: [0.0, 0.0, 1.0] }
    }
    pub fn y(&self) -> [f64; 3] {
        let (a, b) = (self.z, self.x);
        [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
    }
    /// A point in this frame, in world coordinates.
    pub fn world(&self, p: [f64; 3]) -> [f64; 3] {
        let y = self.y();
        [0, 1, 2].map(|i| self.origin[i] + p[0] * self.x[i] + p[1] * y[i] + p[2] * self.z[i])
    }
    /// A direction given in this frame, in world coordinates.
    pub fn dir(&self, d: [f64; 3]) -> [f64; 3] {
        let y = self.y();
        [0, 1, 2].map(|i| d[0] * self.x[i] + d[1] * y[i] + d[2] * self.z[i])
    }
    /// `local`, a frame given in this one, in world coordinates.
    pub fn compose(&self, local: &Frame) -> Frame {
        Frame { origin: self.world(local.origin), x: self.dir(local.x), z: self.dir(local.z) }
    }
    /// The same as [`Frame::compose`] (the name the body builders use).
    pub fn then(&self, local: &Frame) -> Frame {
        self.compose(local)
    }
    /// This frame moved by `d` (world).
    pub fn shifted(mut self, d: [f64; 3]) -> Frame {
        for (o, d) in self.origin.iter_mut().zip(d) {
            *o += d;
        }
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    New,
    Join,
    Cut,
    Intersect,
}

/// One solid step, in its body's frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// The region on the plane z = `z0`, extruded `height` along +z, turned `twist` radians
    /// (counter-clockwise about +z) over that height.
    Prism { region: Region, z0: f64, height: f64, twist: f64, op: Op },
    /// Sections on planes z = h, joined in order.
    Loft { sections: Vec<(Outline, f64)>, op: Op },
    /// The outline on z = `z0` swept `height` along +z, scaled linearly about the axis to
    /// `scale` at the far end (towards the cone's apex) and turned `twist` radians in all, the
    /// turn growing with the logarithm of the scale ([`taper_turn`]): every section is the first
    /// one scaled and turned, the surface a cone's teeth, straight or on a logarithmic spiral
    /// (constant spiral angle).
    Taper { outline: Outline, z0: f64, height: f64, twist: f64, scale: f64, op: Op },
    /// An internal gear's teeth on a cone: the region between the circle of radius `rim` about
    /// the axis (the same at every height: a cylinder) and `outline` (inside it), the outline
    /// tapered and turned as a [`Step::Taper`]'s. Built as one loft of sections that each hold
    /// both loops, so no boolean takes a toothed cutter out of a blank (the kernel's cut of a
    /// bevel ring's spaces from its blank came out open).
    RingTaper { outline: Outline, rim: f64, z0: f64, height: f64, twist: f64, scale: f64, op: Op },
    /// A closed polygon in the (radius, z) half plane, turned a full turn about z.
    Revolve { profile: Vec<V2>, op: Op },
    /// A closed polygon in the (x, z) plane (x radial) swept round z as a helix of `pitch`, from
    /// `z0` for `height`; right-handed unless `left`.
    Coil { section: Vec<V2>, pitch: f64, z0: f64, height: f64, left: bool, op: Op },
    /// A step in a frame of its own, given in the body's frame ([`Frame::then`]): a radial hole
    /// is a prism along a frame whose z points out from the axis. Not nested.
    Placed { frame: Frame, step: Box<Step> },
    /// A solid of its own, built from `steps` in order (the first new, none of them a group),
    /// then combined with the body by `op`: a part that needs a boolean of its own before it
    /// joins, such as one hand of V bevel teeth trimmed to its half of the face.
    Group { steps: Vec<Step>, op: Op },
}

impl Step {
    /// What the step does to its body (a placed step's own).
    pub fn op(&self) -> Op {
        match self {
            Step::Placed { step, .. } => step.op(),
            Step::Group { op, .. } => *op,
            Step::Prism { op, .. }
            | Step::Taper { op, .. }
            | Step::RingTaper { op, .. }
            | Step::Loft { op, .. }
            | Step::Revolve { op, .. }
            | Step::Coil { op, .. } => *op,
        }
    }
    /// The step's operation, to change (a placed step's own).
    pub fn op_mut(&mut self) -> &mut Op {
        match self {
            Step::Placed { step, .. } => step.op_mut(),
            Step::Group { op, .. } => op,
            Step::Prism { op, .. }
            | Step::Taper { op, .. }
            | Step::RingTaper { op, .. }
            | Step::Loft { op, .. }
            | Step::Revolve { op, .. }
            | Step::Coil { op, .. } => op,
        }
    }
}

/// The most curves the kernel takes in one loft (all sections together, each spline counted as
/// its spans: [`loft_curves`]).
pub const LOFT_CURVE_LIMIT: usize = 16_384;

/// The curves the kernel makes of one loft section: one per line or arc, one per span of a
/// spline.
pub fn loft_curves(o: &Outline) -> usize {
    crate::hypoid::corners(o).len()
}
/// The most sections a worm wheel is lofted through to keep the kernel's alignment.
const WHEEL_SECTIONS_SLIP_MAX: usize = 41;

/// How many sections a spiral (twisted) [`Step::Taper`] is lofted through: the fewest from 7
/// with which the kernel lines every section up with the one before (it restarts each at the
/// segment that best matches, so a section turned too far from its neighbour slips by a few
/// segments and the loft's walls cut across the teeth), within the kernel's loft curve limit.
pub fn taper_sections(outline: &Outline, twist: f64, scale: f64) -> usize {
    let per = loft_curves(outline).max(1);
    let most = (LOFT_CURVE_LIMIT / per).clamp(2, 61);
    let mut n = 7usize.min(most);
    loop {
        let sections: Vec<Outline> = (0..n)
            .map(|i| {
                let t = i as f64 / (n - 1) as f64;
                let k = 1.0 + (scale - 1.0) * t;
                outline.scaled(k).rotated(taper_turn(twist, scale, k))
            })
            .collect();
        if !crate::hypoid::slips(&sections, 0) || n + 2 > most {
            return n;
        }
        n += 2;
    }
}

/// How far a [`Step::Taper`]'s section has turned where its scale is `s` (1 at the start).
pub fn taper_turn(twist: f64, scale: f64, s: f64) -> f64 {
    if (scale - 1.0).abs() < 1e-12 || scale <= 0.0 || s <= 0.0 { 0.0 } else { twist * s.ln() / scale.ln() }
}

/// Section `i` of `n` (n ≥ 2) of a taper ([`Step::Taper`], [`Step::RingTaper`]): the outline
/// scaled and turned, and how far along the taper's height it lies (0…1).
pub fn taper_section(outline: &Outline, twist: f64, scale: f64, i: usize, n: usize) -> (Outline, f64) {
    let t = i as f64 / (n - 1) as f64;
    let k = 1.0 + (scale - 1.0) * t;
    (outline.scaled(k).rotated(taper_turn(twist, scale, k)), t)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Body {
    pub name: String,
    pub frame: Frame,
    pub steps: Vec<Step>,
}

/// A gear outline for the 2D preview, placed in the preview's plane.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewGear {
    pub name: String,
    pub region: Region,
    pub center: V2,
    /// Angular speed against the driver's (signed); 0 for a fixed member.
    pub speed: f64,
    /// Sliding speed along +x for a rack, per driver radian.
    pub slide: f64,
    /// The pitch circle radius, drawn as a dashed circle (0: none).
    pub pitch_radius: f64,
    /// For a sliding member: the tooth pitch it repeats with (its slide wraps by this much).
    pub period: f64,
    /// Turns of `center` about the preview's origin per driver radian (0: the centre stays; 1 for
    /// a cycloidal drive's disc, whose centre goes round with the input crank).
    pub orbit: f64,
}

impl PreviewGear {
    /// A member that turns about its own centre at `speed` (no slide, no orbit).
    pub fn turning(name: impl Into<String>, region: Region, center: V2, speed: f64, pitch_radius: f64) -> PreviewGear {
        PreviewGear { name: name.into(), region, center, speed, slide: 0.0, pitch_radius, period: 0.0, orbit: 0.0 }
    }
    /// Where the outline's point `q` is when the driver has turned `t` radians: turned about the
    /// centre (which itself goes round the origin when `orbit` is set), or slid along +x (a
    /// worm's axial section along +y) by whole-pitch-wrapped steps.
    pub fn place(&self, q: V2, t: f64) -> V2 {
        let c = if self.orbit != 0.0 { self.center.rot(self.orbit * t) } else { self.center };
        if self.slide != 0.0 {
            let s = if self.period > 0.0 { (self.slide * t).rem_euclid(self.period) - self.period / 2.0 } else { 0.0 };
            c + q + if self.name == "worm" { V2::new(0.0, s) } else { V2::new(s, 0.0) }
        } else {
            c + q.rot(self.speed * t)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Unit {
    Mm,
    Deg,
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fact {
    pub label: String,
    pub value: f64,
    pub unit: Unit,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Plan {
    pub bodies: Vec<Body>,
    pub facts: Vec<Fact>,
    pub warnings: Vec<String>,
    pub preview: Vec<PreviewGear>,
    /// What the 2D preview shows, when it is not the gears' own plane.
    pub preview_note: String,
    /// Which preview members mesh (parallel-axis sets; see [`crate::mesh2d::check_mesh`]).
    pub meshes: Vec<crate::mesh2d::MeshPair>,
    /// The toothed members' tooth data, for drawing their teeth as formulas
    /// ([`crate::equations::cell_for`]); one per member (the first of each name).
    pub teeth: Vec<crate::equations::TeethSource>,
    /// Which bodies mesh in sets whose axes are not parallel (bevel, hypoid, crossed helical,
    /// worm; see [`crate::mesh3d::check`]), with the whole set's bodies.
    pub meshes3d: Vec<crate::mesh3d::MeshPair3>,
    /// The set's clearance body, when `Spec::clearance_body` asks for it: one solid of the space
    /// the assembled set needs (turning envelopes, shafts, bearing seats), in world coordinates,
    /// built after the gears. Not one of `bodies`.
    pub clearance: Option<Body>,
    /// Where members sit as formulas, for parametric teeth ([`crate::equations::Placing`]).
    pub placing: crate::equations::Placing,
    /// How each step's profile of the clearance body follows a parametric set's formulas (one
    /// entry per vertex of a revolved profile; empty where it stays as numbers).
    pub clearance_follow: Vec<Vec<crate::clearance::VertexFollow>>,
    /// How the set goes together as an assembly: which bodies turn about which shafts, which are
    /// held, and how their motions are tied ([`Assembly`]). Filled by every kind's planner; the
    /// engine makes components and joints of it (`gear.create` with `assembly: true`).
    pub assembly: Assembly,
    /// Loops of the set's mesh graph whose ratios don't agree (an over-constrained train: it
    /// would lock), one per closing mesh, naming the members round the loop
    /// ([`Assembly::from_graph`]). The assembly leaves the closing mesh out.
    pub assembly_issues: Vec<AssemblyIssue>,
    /// The bearings placed at the clearance body's seats (`Spec::clearance_body`), where they
    /// are; built as bodies too when it asks for `bearing_bodies`.
    pub bearings: Vec<crate::clearance::PlacedBearing>,
    /// A set read from a sketch or a selection: what each gear became (one per gear of a train,
    /// in its order; see [`SetMember`]), its mesh graph (`meshes_with`) and clusters.
    pub members: Vec<SetMember>,
    /// Gears joined on one shaft as one part (compound gears): each cluster's gears (indices
    /// into `members`, which are the bodies' order when the whole set is built).
    pub clusters: Vec<Vec<usize>>,
    /// Shafts built with a train ([`train::TrainShaft`]): each one's body and the gears it
    /// carries (joined to them as one part).
    pub shafts: Vec<PlanShaft>,
    /// Where each mesh of a train touches (world, after the placement): the pitch line across
    /// the faces both gears share, for an abstract preview.
    pub contacts: Vec<Contact>,
    /// Each gear's shape without teeth, for an abstract preview (world, after the placement):
    /// trains' gears and bevel pairs' members.
    pub shapes: Vec<MemberShape>,
    /// Why the set can't be built, found while planning it ([`plan`] returns it as the error).
    refused: Option<String>,
}

/// Where two gears touch ([`Plan::contacts`]): the pitch point at each end of the face they
/// share (the same point twice where they share none).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Contact {
    /// The two gears (indices into `Plan::members`).
    pub a: usize,
    pub b: usize,
    pub from: [f64; 3],
    pub to: [f64; 3],
}

/// A gear's shape without teeth ([`Plan::shapes`]): its pitch, tip and root circles on its
/// back face (`center`), the same at the front face `face` mm along `axis`; a bevel's at the
/// heel, with its apex and its pitch circle at the toe.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MemberShape {
    /// The gear (an index into `Plan::members`; a bevel pair's: 0 and 1).
    pub member: usize,
    pub center: [f64; 3],
    pub axis: [f64; 3],
    pub face: f64,
    pub pitch_diameter: f64,
    pub tip_diameter: f64,
    pub root_diameter: f64,
    pub apex: Option<[f64; 3]>,
    pub toe_pitch_diameter: Option<f64>,
    /// A bevel gear's cone, as its teeth are built (what a tooth chamfer needs).
    pub bevel: Option<BevelShape>,
}

/// A bevel gear's cone as its teeth are built, in its own frame: `center` on z = 0 and the apex
/// at z = `apex_z` along `axis`. Distances along the pitch cone are from the apex.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct BevelShape {
    /// The pitch cone's angle from the axis (radians).
    pub delta: f64,
    pub apex_z: f64,
    /// Where the teeth run along the pitch cone: the large end (heel) and the small end (toe).
    pub heel: f64,
    pub toe: f64,
    /// The tips' radius where they end at the heel.
    pub tip_heel: f64,
    /// The teeth end on the back and front cones (a cone flatter than [`FLAT_CONE`]); else on
    /// planes square to the axis through the pitch circles at the heel and the toe.
    pub cone_ends: bool,
    /// V teeth: the relief groove's two edges along the pitch cone.
    pub v_groove: Option<(f64, f64)>,
}

/// A shaft built with a set ([`Plan::shafts`]).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct PlanShaft {
    /// Its body (an index into `Plan::bodies`).
    pub body: usize,
    /// The gears it carries (indices into `Plan::members` / the train's gears); the first is the
    /// one its assembly link joins it to.
    pub gears: Vec<usize>,
    pub diameter: f64,
    pub length: f64,
}

/// One gear of a set read from a sketch or a selection ([`Plan::members`]).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct SetMember {
    /// Where it came from: its index in what was read (the drawing's circles, the selection).
    pub source: usize,
    pub module: f64,
    pub teeth: u32,
    pub pitch_diameter: f64,
    pub face_width: f64,
    /// How far its face reaches each side of its sketch plane along its axis (mm): side A along
    /// the axis, side B against it (`face_a + face_b = face_width`; [`train::SidePreset`]). A
    /// gear read before sides existed has all its face on side A.
    pub face_a: f64,
    pub face_b: f64,
    /// A ring gear (teeth inside).
    pub ring: bool,
    /// The gears (indices into `Plan::members`) it meshes with.
    pub meshes_with: Vec<usize>,
    /// Its cluster (an index into `Plan::clusters`), when it shares a shaft with other gears.
    pub cluster: Option<usize>,
    /// The body it is (an index into `Plan::bodies`), when built.
    pub body: Option<usize>,
}

/// A planned set as an assembly: its moving members (each one of [`Plan::bodies`]) and the links
/// that tie their motions together, enough to make one component per member, a revolute joint per
/// member on its shaft, and a gear, rack-and-pinion or rigid link per mesh.
///
/// Sign conventions (the same as SolidCraft's `joint.gear`): a member's turn is measured
/// counter-clockwise about its [`AssemblyMember::axis`] direction (right hand). For
/// [`AssemblyLink::Gear`] member `b` turns `-ra / rb` radians per radian of `a`, the other sign
/// when `internal` is set, and the sign flipped again when `reverse` is set. With the members'
/// axes these reproduce [`crate::mesh3d::motion`] and the 2D preview's speeds
/// ([`Assembly::ratio`]).
///
/// The clearance body is never a member: the engine adds it as a grounded component of its own.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Assembly {
    pub members: Vec<AssemblyMember>,
    pub links: Vec<AssemblyLink>,
}

/// One moving (or held) member of an [`Assembly`].
#[derive(Clone, Debug, PartialEq)]
pub struct AssemblyMember {
    /// The body it is: an index into [`Plan::bodies`].
    pub body: usize,
    /// The shaft it turns about, in world coordinates (after `Spec::placement`); for a rack, the
    /// line it slides along (positive travel along the direction).
    pub axis: crate::axes::Axis,
    /// Held fixed (a housing, a fixed ring gear, a cycloidal drive's pin ring): grounded, on no
    /// joint.
    pub grounded: bool,
    /// The member (index into [`Assembly::members`]) it rides on: a planet on its carrier or a
    /// differential's case. `None`: it turns about a shaft fixed in the world.
    pub carrier: Option<usize>,
}

/// How two members' motions are tied. Indices are into [`Assembly::members`].
#[derive(Clone, Debug, PartialEq)]
pub enum AssemblyLink {
    /// Two gears: `b` turns `-ra / rb` per radian of `a` (each about its own axis), the same way
    /// round when `internal`, and the other way again when `reverse`. A chain or belt is a gear
    /// link with `internal` set (both wheels turn the same way). `ra` and `rb` are pitch radii
    /// (mm), or tooth counts where pitch radii don't apply. For a planet on a carrier the turns are
    /// relative to the carrier.
    Gear { a: usize, b: usize, ra: f64, rb: f64, internal: bool, reverse: bool },
    /// A rack slides `pitch_radius` mm along its axis per radian the pinion turns about its own.
    RackPinion { pinion: usize, rack: usize, pitch_radius: f64 },
    /// A worm and its wheel: the wheel turns `-starts / wheel_teeth` radians about its axis per
    /// radian of the worm about its own, as a [`AssemblyLink::Gear`] with `ra = starts` and
    /// `rb = wheel_teeth` (the planner points the worm's axis so that this holds for either
    /// hand).
    Worm { worm: usize, wheel: usize, starts: u32, wheel_teeth: u32 },
    /// Joined as one (a spline shaft in its hub).
    Rigid { a: usize, b: usize },
}

impl AssemblyLink {
    /// The two members it ties: the driving one first.
    pub fn members(&self) -> (usize, usize) {
        match *self {
            AssemblyLink::Gear { a, b, .. } | AssemblyLink::Rigid { a, b } => (a, b),
            AssemblyLink::RackPinion { pinion, rack, .. } => (pinion, rack),
            AssemblyLink::Worm { worm, wheel, .. } => (worm, wheel),
        }
    }

    /// The driven member's motion per radian of the driving one ([`AssemblyLink::members`]):
    /// radians, or mm for a rack; 1 for a rigid link.
    pub fn ratio(&self) -> f64 {
        match *self {
            AssemblyLink::Gear { ra, rb, internal, reverse, .. } => {
                let s = if internal { 1.0 } else { -1.0 } * if reverse { -1.0 } else { 1.0 };
                s * ra / rb
            }
            AssemblyLink::RackPinion { pitch_radius, .. } => pitch_radius,
            AssemblyLink::Worm { starts, wheel_teeth, .. } => -f64::from(starts) / f64::from(wheel_teeth.max(1)),
            AssemblyLink::Rigid { .. } => 1.0,
        }
    }
}

/// A loop of a mesh graph whose ratios don't agree: the meshes round it give a member another
/// turn than the closing mesh asks for ([`Assembly::from_graph`]).
#[derive(Clone, Debug, PartialEq)]
pub struct AssemblyIssue {
    /// The members round the loop, in order (the closing mesh joins the last to the first).
    pub members: Vec<usize>,
    /// The closing mesh (an index into the graph's edges).
    pub link: usize,
    /// The turn the loop gives the closing mesh's far member, and the one the mesh asks for
    /// (per radian of the root; mm for a rack).
    pub given: f64,
    pub asked: f64,
    /// For people: which members, and why.
    pub message: String,
}

impl AssemblyLink {
    /// The same tie driven from its other end. A rigid link swaps its ends; a rack and pinion
    /// names its ends by role, so it stays as it is ([`Assembly::from_graph`] puts the driving
    /// end's link first).
    pub fn reversed(&self) -> AssemblyLink {
        match self.clone() {
            AssemblyLink::Gear { a, b, ra, rb, internal, reverse } => AssemblyLink::Gear { a: b, b: a, ra: rb, rb: ra, internal, reverse },
            // The wheel driving the worm: a gear of `wheel_teeth` against one of `starts`.
            AssemblyLink::Worm { worm, wheel, starts, wheel_teeth } => AssemblyLink::Gear {
                a: wheel,
                b: worm,
                ra: f64::from(wheel_teeth.max(1)),
                rb: f64::from(starts.max(1)),
                internal: false,
                reverse: false,
            },
            AssemblyLink::Rigid { a, b } => AssemblyLink::Rigid { a: b, b: a },
            l => l,
        }
    }
}

impl Assembly {
    /// The assembly of a mesh graph: `members`, and `edges` between them in any direction and
    /// number, loops included (every mesh of a sketch-driven system; a compound cluster's shared
    /// shaft as an [`AssemblyLink::Rigid`] link). The links become a tree driven from `root`,
    /// breadth first, each pointing away from it (`members().0` drives; a rack and pinion keeps
    /// its roles, and its driving end is the one reached first: the links come in the order they
    /// are reached). A mesh that closes a loop is left out; where the turn it asks for isn't the
    /// one the loop gives, that is an issue naming the members round the loop. Members no edge
    /// reaches from `root` start trees of their own, in order.
    pub fn from_graph(members: Vec<AssemblyMember>, edges: &[AssemblyLink], root: usize) -> (Assembly, Vec<AssemblyIssue>) {
        let n = members.len();
        let mut speed: Vec<Option<f64>> = vec![None; n];
        let mut parent: Vec<Option<usize>> = vec![None; n];
        let mut used = vec![false; edges.len()];
        let mut links = Vec::new();
        let mut issues = Vec::new();
        let path = |parent: &[Option<usize>], mut x: usize| {
            let mut v = vec![x];
            while let Some(p) = parent[x] {
                v.push(p);
                x = p;
            }
            v
        };
        for start in std::iter::once(root).chain(0..n).filter(|r| *r < n) {
            if speed[start].is_some() {
                continue;
            }
            speed[start] = Some(1.0);
            let mut queue = std::collections::VecDeque::from([start]);
            while let Some(m) = queue.pop_front() {
                let here = speed[m].unwrap_or(1.0);
                for (k, e) in edges.iter().enumerate() {
                    let (a, b) = e.members();
                    if used[k] || a >= n || b >= n || a == b || (a != m && b != m) {
                        continue;
                    }
                    let forward = a == m;
                    let other = if forward { b } else { a };
                    let r = e.ratio();
                    if !forward && r == 0.0 {
                        continue;
                    }
                    // The turn the other end gets across this edge.
                    let across = if forward { here * r } else { here / r };
                    used[k] = true;
                    match speed[other] {
                        None => {
                            speed[other] = Some(across);
                            parent[other] = Some(m);
                            links.push(if forward { e.clone() } else { e.reversed() });
                            queue.push_back(other);
                        }
                        Some(have) if (have - across).abs() > 1e-9 * have.abs().max(across.abs()).max(1.0) => {
                            // Round the loop: from `other` up to where its path to the root meets
                            // `m`'s, then down to `m` (the closing mesh joins `m` back to `other`).
                            let (po, pm) = (path(&parent, other), path(&parent, m));
                            let meet = po.iter().position(|x| pm.contains(x)).unwrap_or(po.len() - 1);
                            let top = po[meet];
                            let mut round: Vec<usize> = po[..=meet].to_vec();
                            let below: Vec<usize> = pm.iter().take_while(|x| **x != top).copied().collect();
                            round.extend(below.into_iter().rev());
                            let names: Vec<String> = round.iter().map(|i| format!("member {}", i + 1)).collect();
                            let message = format!(
                                "the loop {} doesn't turn: the meshes round it turn member {} {:.6} times, the mesh closing it from member {} asks {:.6} (an odd number of outside meshes round a loop, or ratios that don't multiply to 1); that mesh is left out of the assembly",
                                names.join(" → "),
                                other + 1,
                                have,
                                m + 1,
                                across
                            );
                            issues.push(AssemblyIssue { members: round, link: k, given: have, asked: across, message });
                        }
                        Some(_) => {}
                    }
                }
            }
        }
        (Assembly { members, links }, issues)
    }

    /// The same assembly driven from member `root` (its links turned to point away from it).
    pub fn rooted(&self, root: usize) -> Assembly {
        Assembly::from_graph(self.members.clone(), &self.links, root).0
    }

    /// Member `to`'s turn per radian of member `from`'s through the links between them (each
    /// about its own axis; the carrier held), or `None` when no chain of links joins them.
    pub fn ratio(&self, from: usize, to: usize) -> Option<f64> {
        let mut seen = vec![None; self.members.len()];
        *seen.get_mut(from)? = Some(1.0);
        let mut queue = vec![from];
        while let Some(m) = queue.pop() {
            let here = seen[m]?;
            for l in &self.links {
                let (a, b) = l.members();
                let r = l.ratio();
                let next = if a == m {
                    Some((b, here * r))
                } else if b == m && r != 0.0 {
                    Some((a, here / r))
                } else {
                    None
                };
                if let Some((n, v)) = next
                    && n < seen.len()
                    && seen[n].is_none()
                {
                    seen[n] = Some(v);
                    queue.push(n);
                }
            }
        }
        seen.get(to).copied().flatten()
    }
}

impl Plan {
    pub(crate) fn fact(&mut self, label: impl Into<String>, value: f64, unit: Unit) {
        self.facts.push(Fact { label: label.into(), value, unit });
    }
    /// Gives up on the set: the first reason is the one reported.
    pub(crate) fn refuse(&mut self, why: impl Into<String>) {
        if self.refused.is_none() {
            self.refused = Some(why.into());
        }
    }
    /// Records a toothed member's tooth data (the first under each name).
    pub(crate) fn teeth_source(&mut self, src: crate::equations::TeethSource) {
        if !self.teeth.iter().any(|t| t.who == src.who) {
            self.teeth.push(src);
        }
    }
    /// Gives member `who`'s teeth a phase formula ([`crate::equations::TeethSource::phases`]).
    pub(crate) fn teeth_phase(&mut self, who: &str, key: &str, tpl: String) {
        if let Some(t) = self.teeth.iter_mut().find(|t| t.who == who) {
            t.phases.push((key.to_string(), tpl));
        }
    }
    pub(crate) fn warn(&mut self, w: impl Into<String>) {
        let w = w.into();
        if !self.warnings.contains(&w) {
            self.warnings.push(w);
        }
    }
}

/// Validates the spec; an error says which value and why.
pub fn check(s: &Spec) -> Result<(), String> {
    if s.kind == Kind::Differential {
        return crate::diff::check(s);
    }
    if let Some(t) = &s.train {
        if !matches!(s.kind, Kind::Spur | Kind::Helical | Kind::Herringbone) {
            return Err(format!("a gear train's teeth are spur, helical or herringbone, not {}", s.kind.label().to_lowercase()));
        }
        train::check(t)?;
    }
    let pos = |v: f64, what: &str| if v.is_finite() && v > 0.0 { Ok(()) } else { Err(format!("{what} must be greater than 0")) };
    pos(s.module, "module")?;
    pos(s.face_width, "face width")?;
    if s.kind.involute() && !(5.0..=45.0).contains(&s.pressure_angle) {
        return Err("pressure angle must be between 5° and 45°".into());
    }
    if s.helix.abs() >= 80.0 {
        return Err("helix angle must be under 80°".into());
    }
    let min = if s.kind == Kind::Cycloidal { 4 } else { 3 };
    if s.kind != Kind::CycloidalDrive && s.kind != Kind::Rack && !(min..=2000).contains(&s.teeth) {
        return Err(format!("teeth must be between {min} and 2000"));
    }
    if s.kind.is_set()
        && !matches!(s.kind, Kind::CycloidalDrive | Kind::Worm | Kind::Spline | Kind::Elliptical)
        && !(min..=2000).contains(&s.teeth2)
        && !(matches!(s.kind, Kind::Cycloidal | Kind::Sprocket | Kind::Pulley) && s.teeth2 == 0)
    {
        return Err(format!("mating teeth must be between {min} and 2000"));
    }
    if matches!(s.kind, Kind::Bevel | Kind::SpiralBevel | Kind::Hypoid | Kind::CrossedHelical | Kind::BevelPlanetary)
        && !(5.0..=175.0).contains(&s.shaft_angle)
    {
        return Err("shaft angle must be between 5° and 175°".into());
    }
    if s.kind == Kind::BevelPlanetary {
        if !(1..=24).contains(&s.planets) {
            return Err("planets must be between 1 and 24".into());
        }
        if !s.first_azimuth.is_finite() {
            return Err("the first planet's azimuth must be a number".into());
        }
        if s.spiral_angle.abs() > 60.0 {
            return Err("spiral angle must be within ±60°".into());
        }
        if s.teeth3 > 2000 {
            return Err("second sun or ring teeth must be 2000 or fewer".into());
        }
    }
    if s.kind == Kind::Planetary && !(1..=24).contains(&s.planets) {
        return Err("planets must be between 1 and 24".into());
    }
    if s.kind == Kind::Worm && !(1..=12).contains(&s.starts) {
        return Err("worm starts must be between 1 and 12".into());
    }
    if s.kind == Kind::CycloidalDrive {
        if !(3..=200).contains(&s.pins) {
            return Err("pins must be between 3 and 200".into());
        }
        pos(s.pin_circle, "pin circle diameter")?;
        pos(s.pin_diameter, "pin diameter")?;
        pos(s.eccentricity, "eccentricity")?;
    }
    let rack_ok = [s.rack.addendum, s.rack.dedendum, s.rack.root_radius].iter().all(|v| v.is_finite());
    if !rack_ok || s.rack.addendum <= 0.0 || s.rack.dedendum <= s.rack.addendum * 0.5 || s.rack.root_radius < 0.0 {
        return Err("the basic rack's addendum, dedendum and root radius don't make a tooth".into());
    }
    // Numbers that must be numbers, and sizes that can't be negative.
    let b = [&s.bore, &s.bore2];
    let sizes = [
        (s.backlash, "backlash"),
        (s.center_distance, "centre distance"),
        (s.offset, "offset"),
        (s.face_start, "face start"),
        (s.worm_diameter, "worm diameter"),
        (s.worm_length, "worm length"),
        (s.rim, "rim thickness"),
        (s.rack_height, "rack height"),
        (s.hub_diameter, "hub diameter"),
        (s.hub_length, "hub length"),
        (s.hole_diameter, "hole diameter"),
        (b[0].diameter, "bore diameter"),
        (b[1].diameter, "bore diameter"),
        (b[0].key_width, "key width"),
        (b[1].key_width, "key width"),
        (b[0].key_depth, "key depth"),
        (b[1].key_depth, "key depth"),
        (b[0].flat, "bore flat"),
        (b[1].flat, "bore flat"),
    ];
    for (v, what) in sizes {
        if !(v.is_finite() && v >= 0.0) {
            return Err(format!("{what} can't be negative"));
        }
    }
    for (v, what) in [(s.shift, "profile shift"), (s.shift2, "gear 2 profile shift")] {
        if !(v.is_finite() && v.abs() <= 3.0) {
            return Err(format!("{what} must be between −3 and 3"));
        }
    }
    if !s.helix.is_finite() || !s.spiral_angle.is_finite() || !s.shaft_angle.is_finite() || s.origin.iter().any(|v| !v.is_finite()) {
        return Err("angles and the origin must be numbers".into());
    }
    if s.holes > 360 {
        return Err("lightening holes must be 360 or fewer".into());
    }
    if s.kind == Kind::Spline {
        // A spline has its own tooth proportions (checked with its other fields).
    } else if s.kind.involute() {
        // The cutter that makes the teeth must have a tip: its flanks meet at a point when the
        // dedendum is too deep for the pressure angle.
        let tip = PI / 4.0 - s.rack.dedendum * s.pressure_angle.to_radians().tan();
        if tip < 0.01 {
            let most = (PI / 4.0 - 0.01) / s.pressure_angle.to_radians().tan();
            return Err(format!(
                "a {}° pressure angle with a dedendum of {} modules gives a cutter with pointed teeth: lower the dedendum to {} or less, or the pressure angle",
                num(s.pressure_angle),
                num(s.rack.dedendum),
                num((most * 100.0).floor() / 100.0)
            ));
        }
        if s.backlash >= PI * s.module / 4.0 {
            return Err(format!("backlash must be under a quarter of the pitch ({} mm)", num(PI * s.module / 4.0)));
        }
    } else if s.kind == Kind::Cycloidal && s.backlash >= PI * s.module / 4.0 {
        return Err(format!("backlash must be under a quarter of the pitch ({} mm)", num(PI * s.module / 4.0)));
    }
    if matches!(s.kind, Kind::SpiralBevel) && s.spiral_angle.abs() > 60.0 {
        return Err("spiral angle must be within ±60°".into());
    }
    if s.kind == Kind::Rack {
        if !(1..=2000).contains(&s.rack_teeth) {
            return Err("rack teeth must be between 1 and 2000".into());
        }
        let least = (s.rack.dedendum - s.shift + 0.25) * s.module;
        if s.rack_height > 0.0 && s.rack_height < least {
            return Err(format!("rack height below the pitch line must be over {} mm (the tooth roots plus a quarter module)", num(least)));
        }
    }
    crate::strength::check(&s.load)?;
    crate::body::check(s)?;
    if let Some(c) = &s.clearance_body {
        crate::clearance::check(c)?;
    }
    extras::check(s)
}

/// The plan for a spec.
pub fn plan(s: &Spec) -> Result<Plan, String> {
    let _span = solidcraft_profile::span("gear", "plan");
    check(s)?;
    if crate::coupon::applies(s) {
        let mut p = crate::coupon::plan(s)?;
        if s.clearance_body.is_some() {
            p.warn("a test coupon has no clearance body: it is made for the set itself");
        }
        return Ok(p);
    }
    let mut p = Plan::default();
    match s.kind {
        Kind::Spur | Kind::Helical | Kind::Herringbone if let Some(t) = s.train.as_ref() => train::build(s, t, &mut p),
        Kind::Spur | Kind::Helical | Kind::Herringbone => single(s, &mut p),
        Kind::Internal => internal(s, &mut p),
        Kind::Rack => rack(s, &mut p),
        Kind::Pair => pair(s, &mut p),
        Kind::Planetary => planetary(s, &mut p),
        Kind::Bevel | Kind::SpiralBevel => bevel(s, &mut p),
        Kind::Hypoid => crate::hypoid::build(s, &mut p)?,
        Kind::CrossedHelical => crossed(s, &mut p),
        Kind::Worm => worm(s, &mut p),
        Kind::Cycloidal => cycloidal(s, &mut p),
        Kind::CycloidalDrive => drive(s, &mut p),
        Kind::Sprocket => extras::sprocket(s, &mut p),
        Kind::Pulley => extras::pulley(s, &mut p),
        Kind::Spline => extras::spline(s, &mut p),
        Kind::FaceGear => extras::face_gear(s, &mut p),
        Kind::Elliptical => extras::elliptical(s, &mut p),
        Kind::BevelPlanetary => bevel_planet::build(s, &mut p),
        Kind::Differential => crate::diff::build(s, &mut p)?,
    }
    left_out::left_out(s, &mut p);
    // The inputs the phase formulas name beyond each member's own.
    let shared = p.placing.vars.clone();
    for t in &mut p.teeth {
        t.extra = shared.clone();
    }
    if let Some(why) = p.refused.take() {
        return Err(why);
    }
    if s.load.rated() && s.train.is_some() {
        p.warn("load rating is not made for gear trains yet: rate each mesh as a pair");
    } else if s.load.rated() {
        crate::strength::rate(s, &mut p);
    }
    p.meshes = crate::mesh2d::topology(s, &p.preview);
    if matches!(s.kind, Kind::Bevel | Kind::SpiralBevel | Kind::Hypoid | Kind::CrossedHelical | Kind::Worm) {
        // The mesh is checked between the whole set's members, also when one is built.
        let whole = if s.only == 0 { None } else { Some(plan(&Spec { only: 0, ..s.clone() }).map(|w| w.bodies).unwrap_or_default()) };
        p.meshes3d = crate::mesh3d::topology(s, &p.bodies, whole);
    }
    if s.coupon {
        p.warn("a test coupon is made for gear pairs and planetary sets: this is the whole set");
    }
    if p.bodies.is_empty() {
        return Err("nothing to build with these choices".into());
    }
    // The overall placement wraps whatever frames the builders chose.
    if let Some(place) = &s.placement {
        for b in &mut p.bodies {
            b.frame = place.compose(&b.frame);
        }
        // A differential's keep-out body is planned with the set (in the set's frame).
        if let Some(c) = &mut p.clearance {
            c.frame = place.compose(&c.frame);
        }
        for c in &mut p.contacts {
            (c.from, c.to) = (place.world(c.from), place.world(c.to));
        }
        for sh in &mut p.shapes {
            (sh.center, sh.axis, sh.apex) = (place.world(sh.center), place.dir(sh.axis), sh.apex.map(|a| place.world(a)));
        }
    }
    // How the bodies go together, where they are (a differential's is planned with it).
    assembly::fill(s, &mut p);
    // The clearance body, from the bodies where they are.
    if let Some(c) = s.clearance_body.as_ref().filter(|_| s.kind != Kind::Differential) {
        let body = crate::clearance::clearance_body(s, &mut p)?;
        if c.keep_out {
            p.clearance = Some(body);
        }
        if c.bearing_bodies {
            crate::clearance::add_bearing_bodies(s, &mut p);
        }
    }
    Ok(p)
}

fn wanted(s: &Spec, member: u8) -> bool {
    s.only == 0 || s.only == member
}

fn name(s: &Spec, base: &str) -> String {
    if s.name.trim().is_empty() { base.to_string() } else { format!("{} {base}", s.name.trim()) }
}

/// Notes about one involute gear's teeth.
fn tooth_warnings(p: &mut Plan, who: &str, g: &Involute, f: &ToothFlank) {
    if let Some(d) = f.defect() {
        p.refuse(format!("{who}: {d} ({} teeth, profile shift {}): use more teeth or a larger profile shift", g.teeth, num(g.shift)));
        return;
    }
    if f.undercut {
        p.warn(format!(
            "{who}: undercut ({} teeth; about {:.1} or more avoid it at this profile shift): use more teeth (a finer module) or a larger pressure angle",
            g.teeth,
            g.min_teeth_no_undercut()
        ));
    }
    if f.pointed {
        p.warn(format!("{who}: the teeth come to a point; the tip is lowered to Ø{}", num(2.0 * f.ra)));
    } else {
        // (The land before any tip rounding, which `f.tip_half` has taken off.)
        let sa = g.thickness_at(f.ra);
        if sa < 0.25 * g.module {
            p.warn(format!("{who}: thin tooth tips ({} mm; 0.25 × module or more is usual)", num(sa)));
        }
    }
}

fn involute_facts(p: &mut Plan, who: &str, g: &Involute, f: &ToothFlank) {
    let pre = if who.is_empty() { String::new() } else { format!("{who} ") };
    p.fact(format!("{pre}pitch diameter"), 2.0 * g.r(), Unit::Mm);
    p.fact(format!("{pre}tip diameter"), 2.0 * f.ra, Unit::Mm);
    p.fact(format!("{pre}root diameter"), 2.0 * f.rf, Unit::Mm);
    p.fact(format!("{pre}base diameter"), 2.0 * g.rb(), Unit::Mm);
    if g.helix != 0.0 {
        p.fact(format!("{pre}transverse module"), g.mt(), Unit::None);
        p.fact(format!("{pre}lead"), (TAU * g.r() / g.helix.tan()).abs(), Unit::Mm);
    }
    p.fact(format!("{pre}tooth thickness (pitch)"), g.thickness(), Unit::Mm);
    p.fact(format!("{pre}tip land"), if f.pointed { 2.0 * f.ra * f.tip_half } else { g.thickness_at(f.ra) }, Unit::Mm);
    let (k, w) = g.span();
    p.fact(format!("{pre}span over {k} teeth"), w, Unit::Mm);
    let (m, dp) = g.over_pins(None);
    p.fact(format!("{pre}over two Ø{} pins", num(dp)), m, Unit::Mm);
}

/// The steps that add a hub, bore and lightening holes to a gear body from z = 0 to `face`.
fn hub_steps(s: &Spec, bore: &Bore, root_radius: f64, face: f64, steps: &mut Vec<Step>, p: &mut Plan, who: &str) {
    let tip = if s.tooth_chamfer_front.size > 0.0 || s.tooth_chamfer_back.size > 0.0 { crate::body::tip_radius(steps) } else { 0.0 };
    hub_steps_tip(s, bore, root_radius, tip, face, steps, p, who)
}

/// [`hub_steps`] given the gear's tip radius (0: no tooth chamfers). Also the shafts, chamfers,
/// bearing pocket and hub hardware of `crate::body`: gear 2's when `bore` is the spec's `bore2`.
#[allow(clippy::too_many_arguments)]
fn hub_steps_tip(s: &Spec, bore: &Bore, root_radius: f64, tip: f64, face: f64, steps: &mut Vec<Step>, p: &mut Plan, who: &str) {
    let member = crate::body::Member { who, second: std::ptr::eq(bore, &s.bore2), bore, root: root_radius, tip, face };
    let hub_r = s.hub_diameter / 2.0;
    let reach = bore.reach();
    if bore.outline().is_some() && reach >= root_radius - 0.05 * s.module {
        p.refuse(format!("{who}{}", crate::hub::root_words(bore, 2.0 * root_radius, s.module)));
        return;
    }
    if hub_r > 0.0 && s.hub_length > 0.0 {
        if hub_r <= reach {
            p.warn(format!("{who}: the hub is no bigger than the bore"));
        }
        steps.push(Step::Prism {
            region: Region { outer: vec![Outline::circle(V2::ZERO, hub_r)], holes: vec![] },
            z0: face,
            height: s.hub_length,
            twist: 0.0,
            op: Op::Join,
        });
    }
    let built = crate::body::add_solids(s, &member, steps, p);
    let rim = root_radius - 2.0 * s.module;
    if s.holes > 0 {
        // The ring the holes go in: clear of the bore (or hub) and of the rim under the teeth.
        let shaft_r = [built.front, built.back].iter().flatten().map(|sh| sh.diameter / 2.0).fold(0.0, f64::max);
        let pocket_r = crate::body::pocket(s, &member).map_or(0.0, |(r, _)| r + 0.5 * s.module);
        let inner = hub_r.max(reach + s.module).max(rim * 0.25).max(shaft_r).max(pocket_r);
        let room = rim - inner;
        let at = (rim + inner) / 2.0;
        // Neighbouring holes keep a fifth of their spacing between them.
        let spacing = if s.holes > 1 { 2.0 * at * (PI / f64::from(s.holes)).sin() } else { f64::INFINITY };
        let most = room.min(0.8 * spacing);
        let d = if s.hole_diameter > 0.0 { s.hole_diameter } else { (0.8 * room).min(most) };
        if !(room > s.module) || !(d > 0.0) {
            p.warn(format!("{who}: no room for lightening holes; left out"));
        } else if d > most {
            p.warn(format!(
                "{who}: {} lightening holes Ø{} don't fit between Ø{} and Ø{} (Ø{} at most); left out",
                s.holes,
                num(d),
                num(2.0 * inner),
                num(2.0 * rim),
                num(most)
            ));
        } else {
            let holes = lightening_holes(s.holes, d, at, PI / f64::from(s.holes));
            steps.push(Step::Prism { region: Region { outer: holes, holes: vec![] }, z0: -1.0, height: face + 2.0, twist: 0.0, op: Op::Cut });
        }
    }
    crate::body::pockets(s, &member, steps, p);
    if let Some(o) = bore.outline() {
        if reach >= root_radius - s.module {
            p.warn(format!("{who}: under one module of material between the bore and the tooth roots"));
        }
        steps.push(Step::Prism {
            region: Region { outer: vec![o], holes: vec![] },
            z0: built.lo - 1.0,
            height: built.hi - built.lo + 2.0,
            twist: 0.0,
            op: Op::Cut,
        });
    }
    crate::body::add_details(s, &member, &built, steps, p);
}

/// A cylindrical gear's tooth steps: plain, helical or herringbone, from z = 0 to `face`.
fn tooth_steps(region: Region, face: f64, twist_total: f64, herringbone: bool, op: Op) -> Vec<Step> {
    if herringbone {
        let half = face / 2.0;
        let turned = Region {
            outer: region.outer.iter().map(|o| o.rotated(twist_total / 2.0)).collect(),
            holes: region.holes.iter().map(|o| o.rotated(twist_total / 2.0)).collect(),
        };
        vec![
            Step::Prism { region, z0: 0.0, height: half, twist: twist_total / 2.0, op },
            Step::Prism { region: turned, z0: half, height: half, twist: -twist_total / 2.0, op: Op::Join },
        ]
    } else {
        vec![Step::Prism { region, z0: 0.0, height: face, twist: twist_total, op }]
    }
}

fn twist_for(g: &Involute, face: f64) -> f64 {
    face * g.helix.tan() / g.r()
}

fn single(s: &Spec, p: &mut Plan) {
    let helix = if s.kind == Kind::Spur { 0.0 } else { s.helix };
    let g = s.involute(s.teeth, s.shift, helix);
    let mut g = g;
    crate::body::tip_override(s, &mut g, false, p, "gear");
    let (outline, f) = crate::body::teeth(s, &g, 0.0, p, "gear");
    tooth_warnings(p, "gear", &g, &f);
    involute_facts(p, "", &g, &f);
    if helix != 0.0 {
        p.fact("overlap ratio", s.face_width * g.helix.abs().sin() / (PI * s.module), Unit::None);
    }
    let region = Region { outer: vec![outline], holes: vec![] };
    let mut steps = crate::body::tooth_body(s, &g, &f, 0.0, s.face_width, s.kind == Kind::Herringbone, p, "gear");
    hub_steps(s, &s.bore, f.rf - s.elephant_foot, s.face_width, &mut steps, p, "gear");
    p.bodies.push(Body { name: name(s, &format!("Gear z{}", s.teeth)), frame: Frame::at(s.origin), steps });
    p.preview.push(PreviewGear::turning("gear", with_bore(region, &s.bore), V2::ZERO, 1.0, g.r()));
}

/// A ring gear's tooth data for [`Plan::teeth`].
fn ring_source(s: &Spec, who: &str, member: u8, ring: &Internal, f: &ToothFlank) -> crate::equations::TeethSource {
    crate::equations::TeethSource {
        who: who.into(),
        member,
        form: crate::equations::Form::Internal { ring: *ring, f: f.clone() },
        backlash: s.backlash,
        face: s.face_width,
        face_key: "face_width".into(),
        phases: Vec::new(),
        extra: Default::default(),
    }
}

fn with_bore(mut r: Region, b: &Bore) -> Region {
    if let Some(o) = b.outline() {
        r.holes.push(o);
    }
    r
}

/// Notes about a ring gear's teeth (`f`: its spaces, see [`Internal::space_flank`]).
fn ring_warnings(p: &mut Plan, who: &str, ring: &Internal, f: &ToothFlank) {
    let g = &ring.gear;
    let tooth_tip = (PI / g.z() - f.root_half) * f.rf;
    if f.defect().is_some() || !(tooth_tip > 0.0) || !f.tip_half.is_finite() {
        p.refuse(format!(
            "{who}: {} teeth with profile shift {} leave no room for internal teeth: use more teeth or a shift nearer 0",
            g.teeth,
            num(g.shift)
        ));
        return;
    }
    if f.rf > ring.r_tip() + 1e-9 * g.module {
        p.warn(format!("{who}: the teeth come to a point; the inner tips are raised to Ø{}", num(2.0 * f.rf)));
    }
    if f.pointed {
        p.warn(format!("{who}: the tooth spaces close before the root circle; the root is lowered to Ø{}", num(2.0 * f.ra)));
    }
}

fn internal(s: &Spec, p: &mut Plan) {
    let g = s.involute(s.teeth, s.shift, s.helix);
    let ring = Internal { gear: g };
    let rim = if s.rim > 0.0 { s.rim } else { 3.0 * s.module };
    let outer_r = ring.r_root() + rim;
    let sf = ring.space_flank();
    ring_warnings(p, "ring", &ring, &sf);
    let teeth = teeth_outline(&sf, g.teeth, PI / g.z());
    p.fact("pitch diameter", 2.0 * g.r(), Unit::Mm);
    p.fact("tip diameter (inner)", 2.0 * sf.rf, Unit::Mm);
    p.fact("root diameter", 2.0 * sf.ra, Unit::Mm);
    p.fact("outside diameter", 2.0 * outer_r, Unit::Mm);
    p.fact("base diameter", 2.0 * g.rb(), Unit::Mm);
    // The biggest standard pinion whose tips still clear the ring's going in and out of mesh.
    let fits = |z: u32| {
        let pinion = s.involute(z, 0.0, s.helix);
        !internal_tip_interference(&pinion, pinion.ra(), &ring, &mesh_internal(&pinion, &ring, s.face_width))
    };
    if let Some(z) = (3..s.teeth).rev().find(|z| fits(*z)) {
        p.fact("largest pinion (no tip interference)", f64::from(z), Unit::None);
    }
    if ring.r_tip() < g.rb() {
        p.warn("ring: the inner tips are inside the base circle; a pinion may interfere (use more teeth or a positive shift)");
    }
    p.teeth_source(ring_source(s, "ring", 1, &ring, &sf));
    let region = Region { outer: vec![Outline::circle(V2::ZERO, outer_r)], holes: vec![teeth] };
    let mut steps = tooth_steps(region.clone(), s.face_width, twist_for(&g, s.face_width), false, Op::New);
    crate::body::ring_chamfers(s, sf.rf, outer_r, s.face_width, &mut steps, p, "ring");
    p.bodies.push(Body { name: name(s, &format!("Ring gear z{}", s.teeth)), frame: Frame::at(s.origin), steps });
    p.preview.push(PreviewGear {
        name: "ring".into(),
        region,
        center: V2::ZERO,
        speed: 1.0,
        slide: 0.0,
        pitch_radius: g.r(),
        orbit: 0.0,
        period: 0.0,
    });
}

fn rack(s: &Spec, p: &mut Plan) {
    let g = s.involute(s.teeth.max(1), s.shift, s.helix);
    // By default three modules below the pitch line, or two below the roots when a shift drops them.
    let below = if s.rack_height > 0.0 { s.rack_height } else { (3.0 * s.module).max((s.rack.dedendum - s.shift + 2.0) * s.module) };
    let n = s.rack_teeth.max(1);
    let o = rack_outline(&g, n, below);
    p.teeth_source(crate::equations::TeethSource {
        who: "rack".into(),
        member: 1,
        form: crate::equations::Form::Rack { g, count: n, below },
        backlash: s.backlash,
        face: s.face_width,
        face_key: "face_width".into(),
        phases: Vec::new(),
        extra: Default::default(),
    });
    let length = PI * g.mt() * f64::from(n);
    p.fact("length", length, Unit::Mm);
    p.fact("pitch", PI * g.mt(), Unit::Mm);
    p.fact("height (tips to back)", below + (s.rack.addendum + s.shift) * s.module, Unit::Mm);
    p.fact("pitch line to back", below, Unit::Mm);
    let region = Region { outer: vec![o.clone()], holes: vec![] };
    let steps = if s.helix != 0.0 {
        // Teeth slanted across the face: a loft between the outline and the same outline slid
        // along the rack.
        let slide = s.face_width * s.helix.to_radians().tan();
        vec![Step::Loft { sections: vec![(o.clone(), 0.0), (o.translated(V2::new(slide, 0.0)), s.face_width)], op: Op::New }]
    } else {
        vec![Step::Prism { region: region.clone(), z0: 0.0, height: s.face_width, twist: 0.0, op: Op::New }]
    };
    p.bodies.push(Body { name: name(s, &format!("Rack m{}", num(s.module))), frame: Frame::at(s.origin), steps });
    p.preview.push(PreviewGear {
        name: "rack".into(),
        region,
        center: V2::ZERO,
        speed: 0.0,
        slide: 1.0,
        pitch_radius: 0.0,
        orbit: 0.0,
        period: PI * g.mt(),
    });
}

fn pair(s: &Spec, p: &mut Plan) {
    let g1 = s.involute(s.teeth, s.shift, s.helix);
    let mut x2 = s.shift2;
    if s.center_distance > 0.0 {
        let probe = s.involute2(s.teeth2, 0.0, -s.helix);
        match shift_sum_for_center(&g1, &probe, s.center_distance) {
            Some(sum) => {
                x2 = sum - s.shift;
                p.fact("shift sum for the centre distance", sum, Unit::None);
            }
            None => p.warn("the centre distance can't be reached by shifting"),
        }
    }
    let mut g1 = g1;
    let mut g2 = s.involute2(s.teeth2, x2, -s.helix);
    // Gear 2's own face width, centred on gear 1's mid plane unless offset along the axis.
    let face2 = s.face_width_of2();
    let dz2 = (s.face_width - face2) / 2.0 + s.face_offset2;
    let shared = s.face_width.min(dz2 + face2) - dz2.max(0.0);
    if !(shared > 0.0) {
        p.refuse(format!("gear 2's face (offset {} mm) doesn't overlap gear 1's: lower the face offset", num(s.face_offset2)));
        return;
    }
    // Shifted gears sit closer than their shifts push the tips out: shorten both tips by the
    // difference so the tip clearance stays the basic rack's.
    let shorten = (s.shift + x2) * s.module - (mesh_external(&g1, &g2, shared).center_distance - g1.r() - g2.r());
    if shorten > 1e-9 {
        g1.tip_shortening += shorten;
        g2.tip_shortening += shorten;
        p.fact("tip shortening", shorten, Unit::Mm);
    }
    crate::body::tip_override(s, &mut g1, false, p, "gear 1");
    crate::body::tip_override(s, &mut g2, true, p, "gear 2");
    let mut m = mesh_external(&g1, &g2, shared);
    let (o1, f1) = crate::body::teeth(s, &g1, 0.0, p, "gear 1");
    let phase2 = mesh_phase(s.teeth, 0.0, s.teeth2, 0.0);
    let (o2, f2) = crate::body::teeth(s, &g2, phase2, p, "gear 2");
    m.contact_ratio = contact_ratio_external(&g1, &f1, &g2, &f2, &m);
    m.total_contact_ratio = m.contact_ratio + m.overlap_ratio;
    tooth_warnings(p, "gear 1", &g1, &f1);
    tooth_warnings(p, "gear 2", &g2, &f2);
    mesh_facts(p, &m);
    if s.backlash2.is_some() || s.xy_compensation > 0.0 {
        p.fact("backlash (both gears, pitch circle)", g1.backlash + g2.backlash, Unit::Mm);
    }
    p.fact("gear 2 profile shift", x2, Unit::None);
    involute_facts(p, "gear 1", &g1, &f1);
    involute_facts(p, "gear 2", &g2, &f2);
    let a = m.center_distance;
    let herring = s.kind == Kind::Herringbone || (s.herringbone && s.helix != 0.0);
    if s.herringbone && s.helix == 0.0 {
        p.warn("herringbone needs a helix angle: built as a spur pair");
    }
    if wanted(s, 1) {
        let mut steps = crate::body::tooth_body(s, &g1, &f1, 0.0, s.face_width, herring, p, "gear 1");
        hub_steps(s, &s.bore, f1.rf - s.elephant_foot, s.face_width, &mut steps, p, "gear 1");
        p.bodies.push(Body { name: name(s, &format!("Gear z{}", s.teeth)), frame: Frame::at(s.origin), steps });
    }
    if wanted(s, 2) {
        // Its back face is `dz2` up gear 1's axis: the section there is the one on z = 0 turned
        // along the helix.
        let mut steps = crate::body::tooth_body(s, &g2, &f2, phase2 + twist_for(&g2, dz2), face2, herring, p, "gear 2");
        hub_steps(s, &s.bore2, f2.rf - s.elephant_foot, face2, &mut steps, p, "gear 2");
        let index = p.bodies.len();
        p.bodies.push(Body { name: name(s, &format!("Gear z{}", s.teeth2)), frame: Frame::at(s.origin).shifted([a, 0.0, dz2]), steps });
        // As formulas (parametric teeth): gear 2 at the working centre distance, its teeth
        // turned to mesh.
        let mut v = crate::equations::Vars::default();
        if crate::equations::mesh_vars(&mut v, &g1, &g2, ("teeth2", "shift2")).is_ok_and(|cd| (cd - a).abs() <= 1e-6 * a.max(1.0)) {
            p.placing.vars = v;
            p.placing.centres.push((index, "{mesh_cd}".into(), "0 mm".into()));
            let tw = twist_for(&g2, dz2);
            let turn = if tw != 0.0 { format!(" + {}", crate::equations::literal(tw, crate::equations::Quantity::Angle)) } else { String::new() };
            p.teeth_phase("gear 2", "gear2_phase", format!("pi * 1 rad + pi / {{teeth2}} * 1 rad{turn}"));
        }
    }
    p.preview.push(PreviewGear::turning("gear 1", with_bore(Region { outer: vec![o1], holes: vec![] }, &s.bore), V2::ZERO, 1.0, g1.r()));
    p.preview.push(PreviewGear::turning(
        "gear 2",
        with_bore(Region { outer: vec![o2], holes: vec![] }, &s.bore2),
        V2::new(a, 0.0),
        -f64::from(s.teeth) / f64::from(s.teeth2),
        g2.r(),
    ));
}

fn mesh_facts(p: &mut Plan, m: &crate::involute::Mesh) {
    p.fact("ratio", m.ratio, Unit::None);
    p.fact("centre distance", m.center_distance, Unit::Mm);
    if (m.center_distance - m.center_distance_ref).abs() > 1e-6 {
        p.fact("reference centre distance", m.center_distance_ref, Unit::Mm);
    }
    p.fact("working pressure angle", m.working_pressure_angle.to_degrees(), Unit::Deg);
    p.fact("contact ratio", m.contact_ratio, Unit::None);
    if m.overlap_ratio > 0.0 {
        p.fact("overlap ratio", m.overlap_ratio, Unit::None);
        p.fact("total contact ratio", m.total_contact_ratio, Unit::None);
    }
    mesh_warnings(p, "", m);
}

/// The warnings about one mesh (`who`: which, for a set of several; empty for the only one).
pub(crate) fn mesh_warnings(p: &mut Plan, who: &str, m: &crate::involute::Mesh) {
    let pre = if who.is_empty() { String::new() } else { format!("{who}: ") };
    if m.contact_ratio < 1.0 {
        p.warn(format!("{pre}contact ratio {:.2} is below 1: the teeth lose contact", m.contact_ratio));
    } else if m.contact_ratio < 1.2 {
        p.warn(format!("{pre}contact ratio {:.2} is low (1.2 or more is usual)", m.contact_ratio));
    }
    if m.clearance_1 < 0.0 || m.clearance_2 < 0.0 {
        p.warn(format!("{pre}tips hit the mating roots (negative tip clearance): shorten the tips or change the shifts"));
    }
}

/// The profile shift of ring gear `ring` (its teeth, module and helix; its own shift unused) that
/// meshes it with `pinion` (shifted as it is) at centre distance `a`; `None` when no shift
/// reaches it.
pub(crate) fn ring_shift_for_center(pinion: &Involute, ring: &Involute, a: f64) -> Option<f64> {
    let reach = (ring.r() - pinion.r()) * pinion.alpha_t().cos() / a;
    (ring.teeth > pinion.teeth && reach > 0.0 && reach <= 1.0).then(|| {
        pinion.shift + (inv(reach.acos()) - inv(pinion.alpha_t())) * f64::from(ring.teeth - pinion.teeth) / (2.0 * pinion.pressure_angle.tan())
    })
}

/// Shortens a ring's tips so they stay on the involute of `pinion` (an undercut or shifted
/// pinion has less of it than the ring's standard tip allows for); the pinion's flank.
pub(crate) fn clear_ring_tips(ring: &mut Internal, pinion: &Involute, face: f64) -> ToothFlank {
    let flank = external_flank(pinion);
    let clear = ring.r_tip_clear(pinion, flank.flank.first().map_or(flank.rf, |q| q.0), &mesh_internal(pinion, ring, face));
    if clear > ring.r_tip() {
        ring.gear.tip_shortening += clear - ring.r_tip();
    }
    flank
}

fn planetary(s: &Spec, p: &mut Plan) {
    let (zs, zp) = (s.teeth, s.teeth2);
    let zr = if s.teeth3 > 0 { s.teeth3 } else { zs + 2 * zp };
    let n = s.planets.max(1);
    if zr > 6000 || zr < zp + 3 {
        p.refuse(format!("ring teeth ({zr}) must be more than the planet's ({zp}) by 3 or more; sun + 2 × planet = {} meshes", zs + 2 * zp));
        return;
    }
    let mut sun = s.involute(zs, s.shift, s.helix);
    let mut planet = s.involute2(zp, s.shift2, -s.helix);
    let mut m_sp = mesh_external(&sun, &planet, s.face_width);
    // As for a pair: shifted gears keep the basic rack's tip clearance.
    let shorten = (s.shift + s.shift2) * s.module - (m_sp.center_distance - sun.r() - planet.r());
    if shorten > 1e-9 {
        sun.tip_shortening += shorten;
        planet.tip_shortening += shorten;
        m_sp = mesh_external(&sun, &planet, s.face_width);
    }
    // The ring's profile shift puts the planets at the same distance from it as from the sun:
    // the planet's own shift when the tooth counts close (ring = sun + 2 × planet), more or
    // less when they don't.
    let mut ring_gear = Internal { gear: s.involute(zr, 0.0, -s.helix) };
    if let Some(x) = ring_shift_for_center(&planet, &ring_gear.gear, m_sp.center_distance) {
        if x.abs() > 1e-9 {
            ring_gear.gear.shift = x;
            p.fact("ring profile shift", x, Unit::None);
        }
        if (x - s.shift2).abs() > 0.6 {
            p.warn(format!(
                "the ring needs a profile shift of {x:.2} to reach the planets: ring teeth = sun + 2 × planet ({}) fits without",
                zs + 2 * zp
            ));
        }
    }
    // The ring's tips stay on the planets' involute: an undercut or shifted planet has less of
    // it than the ring's standard tip allows for.
    let planet_flank = clear_ring_tips(&mut ring_gear, &planet, s.face_width);
    let m_pr = mesh_internal(&planet, &ring_gear, s.face_width);
    if internal_tip_interference(&planet, planet_flank.ra, &ring_gear, &m_pr) {
        p.warn(format!(
            "the planets' tips hit the ring's tips going in and out of mesh (ring − planet = {} teeth is too few)",
            i64::from(zr) - i64::from(zp)
        ));
    }
    if m_pr.contact_ratio < 1.0 {
        p.warn(format!("planet–ring contact ratio {:.2} is below 1: the teeth lose contact", m_pr.contact_ratio));
    }
    if m_sp.clearance_1 < 0.0 || m_sp.clearance_2 < 0.0 || m_pr.clearance_1 < 0.0 || m_pr.clearance_2 < 0.0 {
        p.warn("tips hit the mating roots (negative tip clearance): change the shifts or the tooth counts");
    }
    let a = m_sp.center_distance;
    if (m_pr.center_distance - a).abs() > 0.01 * s.module {
        p.warn(format!(
            "the planets sit at {} from the sun but {} from the ring: use ring teeth = sun + 2 × planet ({}) or adjust shifts",
            num(a),
            num(m_pr.center_distance),
            zs + 2 * zp
        ));
    }
    if (zs + zr) % n != 0 {
        p.warn(format!("(sun + ring) / planets = {}/{} is not whole: the planets are spaced unevenly to mesh", zs + zr, n));
    }
    // Neighbouring planets must clear each other.
    let ra_p = planet.ra();
    if n > 1 && 2.0 * a * (PI / f64::from(n)).sin() <= 2.0 * ra_p + 0.5 * s.module {
        p.warn("neighbouring planets collide: use fewer or smaller planets");
    }
    p.fact("ratio (ring fixed, sun in, carrier out)", 1.0 + f64::from(zr) / f64::from(zs), Unit::None);
    p.fact("ratio (carrier fixed, sun in, ring out)", -f64::from(zr) / f64::from(zs), Unit::None);
    p.fact("ratio (sun fixed, ring in, carrier out)", 1.0 + f64::from(zs) / f64::from(zr), Unit::None);
    p.fact("planet centre radius", a, Unit::Mm);
    p.fact("ring teeth", f64::from(zr), Unit::None);
    p.fact("sun–planet contact ratio", m_sp.contact_ratio, Unit::None);
    p.fact("planet–ring contact ratio", m_pr.contact_ratio, Unit::None);
    let total = f64::from(zs + zr);
    let thetas: Vec<f64> = (0..n).map(|k| TAU * (f64::from(k) * total / f64::from(n)).round() / total).collect();
    let (o_sun, f_sun) = crate::body::teeth(s, &sun, 0.0, p, "sun");
    tooth_warnings(p, "sun", &sun, &f_sun);
    let mut planets = Vec::new();
    let planet_flank = crate::body::flank(s, &planet, p, "planet");
    for th in &thetas {
        let ph = mesh_phase(zs, 0.0, zp, *th);
        planets.push((*th, teeth_outline(&planet_flank, zp, ph), planet_flank.clone()));
    }
    if let Some((_, _, f)) = planets.first() {
        tooth_warnings(p, "planet", &planet, &f.clone());
    }
    let th0 = thetas[0];
    let ph0 = mesh_phase(zs, 0.0, zp, th0);
    let delta = wrap_period(ph0 - th0, TAU / f64::from(zp));
    let ring_phase = th0 + delta * f64::from(zp) / f64::from(zr) + PI / f64::from(zr);
    let rim = if s.rim > 0.0 { s.rim } else { 3.0 * s.module };
    let ring_flank = ring_gear.space_flank();
    ring_warnings(p, "ring", &ring_gear, &ring_flank);
    let ring_teeth = teeth_outline(&ring_flank, zr, ring_phase + PI / f64::from(zr));
    let ring_region = Region { outer: vec![Outline::circle(V2::ZERO, ring_gear.r_root() + rim)], holes: vec![ring_teeth] };
    p.teeth_source(ring_source(s, "ring", 3, &ring_gear, &ring_flank));
    if wanted(s, 1) {
        let mut steps = crate::body::tooth_body(s, &sun, &f_sun, 0.0, s.face_width, false, p, "sun");
        hub_steps(s, &s.bore, f_sun.rf - s.elephant_foot, s.face_width, &mut steps, p, "sun");
        p.bodies.push(Body { name: name(s, &format!("Sun z{zs}")), frame: Frame::at(s.origin), steps });
    }
    // As formulas (parametric teeth): the planets at the working centre distance, each at its
    // place round the sun (the same rounding to whole teeth), turned to mesh.
    let mut placing = crate::equations::Vars::default();
    let placed = crate::equations::mesh_vars(&mut placing, &sun, &planet, ("teeth2", "shift2")).is_ok_and(|cd| (cd - a).abs() <= 1e-6 * a.max(1.0));
    placing.input("teeth3", f64::from(zr), crate::equations::Quantity::Number, "ring: number of teeth");
    let theta = |k: usize| format!("(2 * pi * round({k} * ({{teeth}} + {{teeth3}}) / {n}) / ({{teeth}} + {{teeth3}}))");
    for (k, (th, o, f)) in planets.iter().enumerate() {
        let c = V2::polar(a, *th);
        if wanted(s, 2) {
            // Each planet in its own frame at its centre.
            let mut steps = crate::body::tooth_body(s, &planet, f, mesh_phase(zs, 0.0, zp, *th), s.face_width, false, p, "planet");
            hub_steps(s, &s.bore2, f.rf - s.elephant_foot, s.face_width, &mut steps, p, "planet");
            let index = p.bodies.len();
            p.bodies.push(Body { name: name(s, &format!("Planet {} z{zp}", k + 1)), frame: Frame::at(s.origin).shifted([c.x, c.y, 0.0]), steps });
            let t = theta(k);
            let (x, y) = (format!("{{mesh_cd}} * cos({t} * 1 rad)"), format!("{{mesh_cd}} * sin({t} * 1 rad)"));
            let at = placing.eval(&x, None).and_then(|x| Ok((x, placing.eval(&y, None)?)));
            if placed && at.is_ok_and(|(x, y)| (x - c.x).abs() + (y - c.y).abs() <= 1e-6 * a.max(1.0)) {
                p.placing.centres.push((index, x, y));
                p.teeth_phase(
                    "planet",
                    &format!("planet{}_phase", k + 1),
                    format!("{t} * 1 rad + pi * 1 rad + pi / {{teeth2}} * 1 rad + {{teeth}} / {{teeth2}} * {t} * 1 rad"),
                );
            }
        }
        p.preview.push(PreviewGear::turning(
            format!("planet {}", k + 1),
            with_bore(Region { outer: vec![o.clone()], holes: vec![] }, &s.bore2),
            c,
            -f64::from(zs) / f64::from(zp),
            planet.r(),
        ));
    }
    if placed {
        p.placing.vars = placing;
        // (Planet 1 is at 0: the ring meshes with it half a ring tooth on when its tooth count is
        // even.)
        p.teeth_phase("ring", "ring_phase", "pi * (1 - mod({teeth2}, 2)) / {teeth3} * 1 rad".into());
    }
    if wanted(s, 3) {
        let mut steps = tooth_steps(ring_region.clone(), s.face_width, twist_for(&ring_gear.gear, s.face_width), false, Op::New);
        crate::body::ring_chamfers(s, ring_flank.rf, ring_gear.r_root() + rim, s.face_width, &mut steps, p, "ring");
        p.bodies.push(Body { name: name(s, &format!("Ring z{zr}")), frame: Frame::at(s.origin), steps });
    }
    p.preview.insert(0, PreviewGear::turning("sun", with_bore(Region { outer: vec![o_sun], holes: vec![] }, &s.bore), V2::ZERO, 1.0, sun.r()));
    p.preview.push(PreviewGear::turning("ring", ring_region, V2::ZERO, -f64::from(zs) / f64::from(zr), ring_gear.gear.r()));
}

/// Where a bevel member's teeth run along its pitch cone: from the toe (`heel − width` from the
/// apex) to the heel, and the distance `mid` at which a spiral bevel's spiral is at its mean
/// angle (the same for every member of a set, so their spirals stay in phase).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ConeFace {
    pub heel: f64,
    pub width: f64,
    pub mid: f64,
}

impl ConeFace {
    pub fn toe(&self) -> f64 {
        self.heel - self.width
    }
}

/// The faces of a bevel set's two members (gear 1, gear 2; or sun, planets) at cone distance
/// `re`: gear 1's face width from its heel at `re`; gear 2's own face width (`face_width2`, 0:
/// the same) with its heel `face_offset2` in from `re` towards the apex. Refused when gear 2's
/// face misses gear 1's or reaches too near the apex.
pub(crate) fn cone_faces(s: &Spec, re: f64, p: &mut Plan, names: [&str; 2]) -> Option<[ConeFace; 2]> {
    let faces = faces_of(s, re);
    let (b1, heel2, b2) = (faces[0].width, faces[1].heel, faces[1].width);
    for (f, who) in faces.iter().zip(names) {
        if f.width > f.heel / 3.0 {
            p.warn(format!("{who}: face width over a third of the cone distance ({}) makes weak small ends", num(f.heel / 3.0)));
        }
    }
    if !(heel2.is_finite() && b2 > 0.0) || heel2 - b2 < 0.25 * re || heel2 > 1.5 * re {
        p.refuse(format!(
            "{}: a heel {} mm in from the cone distance ({}) puts the teeth too near the apex or too far out; lower the heel offset or the face width",
            names[1],
            num(s.face_offset2),
            num(re)
        ));
        return None;
    }
    if heel2 <= re - b1 + 0.05 * s.module || heel2 - b2 >= re - 0.05 * s.module {
        p.refuse(format!("{}'s face (heel offset {} mm) doesn't overlap {}'s: lower the heel offset", names[1], num(s.face_offset2), names[0]));
        return None;
    }
    Some(faces)
}

/// The two faces [`cone_faces`] gives, unchecked.
pub(crate) fn faces_of(s: &Spec, re: f64) -> [ConeFace; 2] {
    let b1 = s.face_width.min(re * 0.45);
    let mid = re - b1 / 2.0;
    let heel2 = re - s.face_offset2;
    let b2 = s.face_width_of2().min(heel2 * 0.45);
    [ConeFace { heel: re, width: b1, mid }, ConeFace { heel: heel2, width: b2, mid }]
}

/// Where the two members' faces overlap along the pitch cone (from the apex: the least and the
/// most distance both have teeth at), for the flank gap a backlash makes.
pub(crate) fn faces_overlap(s: &Spec, re: f64) -> (f64, f64) {
    let [a, b] = faces_of(s, re);
    (a.toe().max(b.toe()), a.heel.min(b.heel))
}

/// A straight or spiral bevel gear's teeth, in a frame with the large end's pitch circle (at
/// cone distance `re`) on z = 0 and the cone's apex on +z: cut by a crown gear ([`Crown`]), so
/// the two members of a pair (cut by the two sides of the same one) are conjugate along the
/// whole face; the teeth run along `face` and end on the back and front cones.
/// A bevel gear's virtual spur gear on its back cone (Tredgold): `teeth` at the normal module
/// m·cos `spiral`, the helix `helix`.
pub(crate) fn bevel_involute(s: &Spec, teeth: u32, spiral: f64, helix: f64, shift: f64, virtual_teeth: f64) -> Involute {
    Involute {
        teeth,
        module: s.module * spiral.cos(),
        pressure_angle: s.pressure_angle.to_radians(),
        helix,
        shift,
        rack: s.rack,
        backlash: s.backlash,
        tip_shortening: 0.0,
        virtual_teeth,
    }
}

/// The fewest teeth a bevel gear needs at its cone angle, from how far over the two limits it
/// is (each ratio is tooth size over cone size: at a fixed cone angle the cone grows with the
/// tooth count and both grow with the module, so only the tooth count helps).
pub(crate) fn min_cone_teeth(z: u32, over_tip: f64, over_root: f64) -> u32 {
    let need = |over: f64| if over.is_finite() && over > 0.0 { (f64::from(z) * over).floor() as u32 + 1 } else { 0 };
    need(over_tip).max(need(over_root)).max(z + 1)
}

/// The refusal for teeth too tall for their cone: the fewest teeth that fit, and that the
/// module doesn't change it.
pub(crate) fn too_tall_for_cone(delta: f64, z: u32, need: u32, angle: &str) -> String {
    format!(
        "the teeth are too tall for its cone (pitch cone angle {}°): at this angle it needs at least {need} teeth, it has {z}. The module doesn't change this, since the whole gear scales with it: give both gears more teeth at the same ratio, or change {angle}",
        num(delta.to_degrees())
    )
}

#[allow(clippy::too_many_arguments)]
/// One half of V (chevron) bevel teeth's room, as a profile to revolve about the member's axis
/// (its frame: apex at (0, `apex_z`)): inside the sphere of radius `rm` about the apex (the half
/// towards it), or outside it within `reach` of the apex (the outer half). The sphere is drawn as
/// a fine polygon; both halves use the same one, so they meet with no gap.
/// How wide (× module) the relief groove is where V bevel teeth change hand.
const V_RELIEF: f64 = 0.2;

/// The core under V bevel teeth's relief groove, to revolve about the member's axis: between the
/// spheres of radius `r0` and `r1` about the apex (0, `apex_z`), between the angles `band`
/// (radians, from the axis seen from the apex: a band just inside the root cone).
fn v_groove_core(apex_z: f64, r0: f64, r1: f64, band: (f64, f64)) -> Vec<V2> {
    const N: usize = 8;
    let at = |r: f64, t: f64| V2::new(r * t.sin(), apex_z - r * t.cos());
    let t = |i: usize| band.0 + (band.1 - band.0) * i as f64 / N as f64;
    let mut out: Vec<V2> = (0..=N).map(|i| at(r0, t(i))).collect();
    out.extend((0..=N).rev().map(|i| at(r1, t(i))));
    out
}

fn v_half_region(apex_z: f64, rm: f64, reach: f64, inner: bool) -> Vec<V2> {
    const N: usize = 96;
    let arc = (0..=N).map(|i| {
        let t = PI * i as f64 / N as f64;
        V2::new(rm * t.sin(), apex_z - rm * t.cos())
    });
    if inner {
        return arc.collect();
    }
    let big = reach.max(2.0 * rm);
    let mut out = vec![V2::new(0.0, apex_z - big), V2::new(big, apex_z - big), V2::new(big, apex_z + big), V2::new(0.0, apex_z + big)];
    out.extend(arc.collect::<Vec<_>>().into_iter().rev());
    out
}

fn bevel_member(s: &Spec, z: u32, delta: f64, spiral: f64, phase: f64, face: ConeFace, p: &mut Plan, who: &str) -> (ConeTeeth, ToothFlank) {
    let m = s.module;
    let r = m * f64::from(z) / 2.0;
    let re = r / delta.sin();
    let h = re * delta.cos();
    // Tredgold's virtual spur gear on the back cone, for the checks and notes that go by tooth
    // count. The module is the transverse one at the large end: the normal module is m·cos β.
    let zv = f64::from(z) / delta.cos();
    let shift = if who.contains('1') { s.shift } else { -s.shift };
    let v = bevel_involute(s, z, spiral, spiral, shift, zv);
    let mn = v.module;
    let at = v.alpha_t();
    // The crown gear's tooth in this gear's spaces: where the gear's teeth are thicker (a
    // positive shift) it is thinner, and the mate's, cut by the other side, thicker.
    let crown = Crown {
        delta,
        cone_distance: re,
        pressure_angle: at,
        addendum: (s.rack.addendum + shift) * mn,
        dedendum: (s.rack.dedendum - shift) * mn,
        tip_radius: s.rack.root_radius * mn,
        half_thickness: PI * m / 4.0 - shift * mn * at.tan() + s.backlash / 2.0,
    };
    let (dc, ds) = (delta.cos(), delta.sin());
    let ha = crown.addendum.max(0.0);
    if ha * ds >= 0.5 * h || crown.dedendum >= 0.9 * re * delta.tan() {
        // A flat cone at a wide shaft angle: the tooth ratio that keeps both cones at 85° or less.
        let sigma = s.shaft_angle.to_radians();
        let range = if delta.to_degrees() > 80.0 && s.shaft_angle > 90.0 && s.shaft_angle < 170.0 {
            let k = |d: f64| sigma.sin() / d.to_radians().tan() - sigma.cos();
            format!(
                "; at this {}° shaft angle a tooth ratio of about {} to {} keeps both cones at 85° or less",
                num(s.shaft_angle),
                num((k(85.0) * 100.0).round() / 100.0),
                num((k(s.shaft_angle - 85.0) * 100.0).round() / 100.0)
            )
        } else {
            String::new()
        };
        let need = min_cone_teeth(z, ha * ds / (0.5 * h), crown.dedendum / (0.9 * re * delta.tan()));
        p.refuse(format!("{who}: {}{range}", too_tall_for_cone(delta, z, need, "the shaft angle")));
        let none = ConeTeeth { steps: Vec::new(), ends: ConeEnds { back: 0.0, front: 0.0, small_root: 0.0 }, tip_radius: 0.0 };
        return (
            none,
            ToothFlank { rf: 0.0, ra: 0.0, root_half: 0.0, tip_half: 0.0, fillet: Vec::new(), flank: Vec::new(), undercut: false, pointed: false },
        );
    }
    // A spiral bevel's teeth are the straight bevel's with every point turned about the axis by
    // the logarithm of its distance from the apex (a logarithmic spiral of constant spiral angle
    // on the pitch cone, none at the middle of the face). A tooth's tip and root on one flat
    // section are at different distances from the apex, so the section's teeth lean.
    let rm = face.mid;
    let wind = if spiral.abs() > 1e-6 { spiral.tan() / ds } else { 0.0 };
    let skew = |rr: f64| if wind == 0.0 { 0.0 } else { wind * (rr.hypot(h) / rm).ln() };
    let (teeth, mut flank) = if s.herringbone && wind != 0.0 && face.toe() < rm && rm < face.heel {
        // V (chevron) teeth: the spiral's hand changes at mid-face (cone distance `rm`). The half
        // towards the apex runs at the spiral angle, the outer half at the opposite one. Each half
        // is the whole face's spiral teeth of its hand, kept on its side of the sphere of radius
        // `rm` about the apex (a group of its own: the trim mustn't touch the other half). Both
        // hands' teeth are the same on that sphere (neither has turned there), so the halves meet
        // along the line where the hand changes, with no groove, at any cone angle (flat sections
        // through a wheel near a crown gear's ran across most of its face).
        let mirrored = |rr: f64| -skew(rr);
        let (a, fl) = lined_up_cone_teeth(&crown, z, mn, &skew, phase, delta, re, face, wind, m);
        let (b, _) = lined_up_cone_teeth(&crown, z, mn, &mirrored, phase, delta, re, face, -wind, m);
        // A narrow relief groove where the hands meet (as a herringbone's): at the kink the two
        // hands' contact runs a few thousandths of a millimetre into the mate. The same width
        // at any cone angle, with the core under the roots across it so the halves are one part.
        let groove = V_RELIEF * m;
        let side = |inner: bool| v_half_region(h, if inner { rm - groove / 2.0 } else { rm + groove / 2.0 }, re + h + 10.0 * m, inner);
        let half = |mut steps: Vec<Step>, inner: bool, op: Op| {
            steps.push(Step::Revolve { profile: side(inner), op: Op::Intersect });
            Step::Group { steps, op }
        };
        // The root cone's angle from the axis (seen from the apex), a little inside the roots, and
        // a band 0.4 modules under it: material both halves have (a flat wheel's body is only
        // half a module thick under its roots).
        let root = 0.98 * (delta - crown.dedendum.max(0.0).atan2(re));
        let under = (root - 0.4 * m / rm).max(0.0);
        let mut steps = vec![half(a.steps, true, Op::New), half(b.steps, false, Op::Join)];
        if root > under {
            steps.push(Step::Revolve { profile: v_groove_core(h, rm - groove, rm + groove, (under, root)), op: Op::Join });
        }
        p.fact(format!("{who} V teeth relief groove at mid-face"), groove, Unit::Mm);
        let ends = ConeEnds {
            back: a.ends.back.min(b.ends.back),
            front: a.ends.front.max(b.ends.front),
            small_root: a.ends.small_root.min(b.ends.small_root),
        };
        let tip_radius = a.tip_radius.max(b.tip_radius);
        (ConeTeeth { steps, ends, tip_radius }, fl)
    } else {
        lined_up_cone_teeth(&crown, z, mn, &skew, phase, delta, re, face, wind, m)
    };
    flank.undercut = v.undercut();
    // At the heel (the teeth's depth goes with the distance from the apex).
    let k = face.heel / re;
    p.fact(format!("{who} pitch cone angle"), delta.to_degrees(), Unit::Deg);
    p.fact(format!("{who} outer pitch diameter"), 2.0 * r, Unit::Mm);
    p.fact(format!("{who} outer tip diameter"), 2.0 * k * (r + ha * dc), Unit::Mm);
    p.fact(format!("{who} cone distance"), re, Unit::Mm);
    if (face.heel - re).abs() > 1e-9 {
        p.fact(format!("{who} heel cone distance"), face.heel, Unit::Mm);
    }
    p.fact(format!("{who} face width"), face.width, Unit::Mm);
    p.fact(format!("{who} virtual teeth"), zv, Unit::None);
    tooth_warnings(p, who, &v, &flank);
    (teeth, flank)
}

/// Whether the engine's loft of a spiral [`Step::Taper`] of `teeth` teeth (through
/// [`taper_sections`] sections) would have the kernel pair a tooth's segments with others than
/// the same tooth's (a restart by whole teeth is harmless).
pub(crate) fn taper_slips(step: &Step, teeth: u32) -> bool {
    let Step::Taper { outline, twist, scale, .. } = step else { return false };
    if twist.abs() <= 1e-12 {
        return false;
    }
    let n = taper_sections(outline, *twist, *scale);
    let sections: Vec<Outline> = (0..n)
        .map(|i| {
            let k = 1.0 + (scale - 1.0) * i as f64 / (n - 1) as f64;
            outline.scaled(k).rotated(taper_turn(*twist, *scale, k))
        })
        .collect();
    crate::hypoid::slips(&sections, teeth as usize)
}

/// A bevel member's teeth ([`Crown::tooth`], [`cone_teeth`]): with fewer fit points when a long
/// spiral taper's loft would otherwise slip within the kernel's curve limit (as a big worm
/// wheel's do).
#[allow(clippy::too_many_arguments)]
pub(crate) fn lined_up_cone_teeth(
    crown: &Crown,
    z: u32,
    mn: f64,
    skew: &dyn Fn(f64) -> f64,
    phase: f64,
    delta: f64,
    re: f64,
    face: ConeFace,
    wind: f64,
    m: f64,
) -> (ConeTeeth, ToothFlank) {
    let attempt = |coarse: f64| {
        let (sides, flank) = crown.tooth_with(z, mn, skew, coarse);
        let teeth = cone_teeth(&sides, z, phase, delta, re, face, wind, m);
        // (Only a flat cone's taper is long enough to need it; a big gear's check would cost
        // more than the plan.)
        let slips = wind != 0.0 && delta > 60f64.to_radians() && z <= 120 && teeth.steps.first().is_some_and(|st| taper_slips(st, z));
        (teeth, flank, slips)
    };
    // The finest first, then coarser while the loft would slip; the coarsest is kept whatever.
    let mut out = attempt(1.0);
    for coarse in [1.5, 2.0, 3.0] {
        if !out.2 {
            break;
        }
        out = attempt(coarse);
    }
    (out.0, out.1)
}

/// Where a bevel member's body ends along its axis, in its frame (the large end's pitch circle on
/// z = 0, the apex on +z): its back face, its front (small end's) face, and the root radius at
/// the small end (the most a bore may reach).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ConeEnds {
    pub back: f64,
    pub front: f64,
    pub small_root: f64,
}

/// A bevel member's teeth as built: the steps, where the body ends, and the outer tip radius
/// the steps make (at the heel).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ConeTeeth {
    pub steps: Vec<Step>,
    pub ends: ConeEnds,
    pub tip_radius: f64,
}

/// Pitch cone angles above this (70°) end their teeth on the back and front cones; flatter
/// cones' plane sections square to the axis run far past them (on a crown's the tips never
/// meet that plane). Up to it the teeth end on planes square to the axis, as always: no boolean
/// on the teeth at all. (SMLib's check refused every curved trim of a bevel's teeth, "Bad
/// CurveCusp", while the engine handed a taper's sections in as the sketch's fit splines, whose
/// speed jumps at each fit point; it takes them on the even-speed sections the engine makes now.)
pub const FLAT_CONE: f64 = 70.0 * PI / 180.0;

/// How deep a bevel's body runs under its roots (× module, along the back cone at the heel and
/// along the front cone at the toe; less on a slim pinion, to stay clear of the axis). Kept
/// small: the taper has to reach that far, and the further it reaches the more a spiral
/// turns over it and the more sections its loft needs.
const HEEL_RIM: f64 = 0.5;
const TOE_RIM: f64 = 0.5;

/// The solid of a bevel member's teeth from their outline on the plane square to the axis
/// through the pitch circle at cone distance `re` (`sides`, as [`Crown::tooth`] gives them, at
/// h = re·cos δ from the apex): `z` teeth turned by `phase`, running along `face`, spiral `wind`
/// (turn per unit of the logarithm of the distance from the apex; 0 for straight teeth).
///
/// Every tooth surface is a cone through the apex (turned by the logarithmic spiral), so the
/// plane section scaled about the apex and turned is exact on any plane square to the axis: one
/// [`Step::Taper`]. A taper ends on planes square to the axis, but on such a plane a bevel's
/// teeth run from the root cone, inside the back cone, to the tip cone, outside it (by far on a
/// flat cone: a crown's tips never meet that plane, and a taper ending there makes long thin
/// blades). Up to [`FLAT_CONE`] the taper does end on planes, through the pitch circles at the
/// heel and the toe (the tips run on a little past the back cone, the roots stop a little short
/// of it). Flatter, the taper is made long enough to hold the whole face (its roots past the back
/// cone at its start, its tips inside the front cone at its end) and is intersected with a
/// revolved blank bounded by the back and front cones, square to the pitch cone's elements at
/// the heel and the toe. The body's back is a disc [`HEEL_RIM`] modules (along the back cone)
/// under the roots at the heel; its front, inside the front cone, a flat face [`TOE_RIM`]
/// modules under the roots at the toe. No face of the blank lies on or along one of the
/// taper's (the old trouble: the kernel refused cones that graze the root fillets where a
/// taper's flat end meets them), so the cones cut the teeth square across. Should the kernel
/// still refuse the trim, the engine leaves the taper as it is: the teeth run on a little past
/// the cones.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cone_teeth(sides: &ToothSides, z: u32, phase: f64, delta: f64, re: f64, face: ConeFace, wind: f64, m: f64) -> ConeTeeth {
    let h = re * delta.cos();
    let outline = sided_outline(sides, z, phase);
    let (heel, toe) = (face.heel, face.toe());
    if delta <= FLAT_CONE {
        // Flat ends: the planes square to the axis through the pitch circles at the heel and
        // the toe (at the heel at the cone distance, the plane of `sides` itself).
        let (k0, k1) = (heel / re, toe / re);
        let base = outline.scaled(k0).rotated(wind * k0.ln());
        let scale = k1 / k0;
        let small_root = k1 * outline.polyline(1e-3).iter().map(|q| q.len()).fold(f64::INFINITY, f64::min);
        let (z0, z1) = (h * (1.0 - k0), h * (1.0 - k1));
        let steps = vec![Step::Taper { outline: base, z0, height: z1 - z0, twist: wind * scale.ln(), scale, op: Op::New }];
        return ConeTeeth { steps, ends: ConeEnds { back: z0, front: z1, small_root }, tip_radius: k0 * sides.ra };
    }
    let (sd, cd) = delta.sin_cos();
    // The root and tip cones' angles from the axis (seen from the apex).
    let (phi_r, phi_t) = (sides.rf.atan2(h), sides.ra.atan2(h));
    // Along a direction at φ from the axis, the point at distance ρ from the apex is at
    // ρ·cos(φ − δ) along the pitch element: `heel` on the back cone, `toe` on the front one.
    let on = |phi: f64, along: f64| -> (f64, f64) {
        let rho = along / (phi - delta).cos();
        (rho * phi.sin(), h - rho * phi.cos())
    };
    let (r_root_toe, z_root_toe) = on(phi_r, toe);
    let (_, z_root_heel) = on(phi_r, heel);
    let (r_tip_heel, _) = on(phi_t, heel);
    let (_, z_tip_toe) = on(phi_t, toe);
    // The back and front cones as radius against height.
    let cone_r = |along: f64, z: f64| (along + (z - h) * cd) / sd;
    // Down the back cone from the roots, and down the front cone (inwards: along −(cos δ,
    // sin δ) in the radius-height plane), at most half way to the axis.
    let (r_root_heel, _) = on(phi_r, heel);
    let t_heel = (HEEL_RIM * m).min(0.5 * r_root_heel / cd);
    let t_toe = (TOE_RIM * m).min(0.5 * r_root_toe / cd);
    let z_lo = z_root_heel - t_heel * sd;
    let z_face = z_root_toe - t_toe * sd;
    let z_hi = z_tip_toe + (0.1 * m).min(0.5 * (h - z_tip_toe));
    let (k0, k1) = ((h - z_lo) / h, (h - z_hi) / h);
    let base = outline.scaled(k0).rotated(wind * k0.ln());
    let scale = k1 / k0;
    let e = m;
    // Below the back face and above the tips the blank only has to hold the taper.
    let blank = vec![
        V2::new(0.0, z_lo - e),
        V2::new(cone_r(heel, z_lo), z_lo - e),
        V2::new(cone_r(heel, z_lo), z_lo),
        V2::new(cone_r(heel, z_hi + e), z_hi + e),
        V2::new(cone_r(toe, z_hi + e), z_hi + e),
        V2::new(cone_r(toe, z_face), z_face),
        V2::new(0.0, z_face),
    ];
    let steps = vec![
        Step::Taper { outline: base, z0: z_lo, height: z_hi - z_lo, twist: wind * scale.ln(), scale, op: Op::New },
        Step::Revolve { profile: blank, op: Op::Intersect },
    ];
    ConeTeeth { steps, ends: ConeEnds { back: z_lo, front: z_face, small_root: r_root_toe }, tip_radius: r_tip_heel }
}

fn bevel(s: &Spec, p: &mut Plan) {
    let sigma = s.shaft_angle.to_radians();
    let (z1, z2) = (f64::from(s.teeth), f64::from(s.teeth2));
    let d1 = sigma.sin().atan2(z2 / z1 + sigma.cos());
    let d2 = sigma - d1;
    let spiral = if s.kind == Kind::SpiralBevel { s.spiral_angle.to_radians() } else { 0.0 };
    // (85° itself is built: a 170° shaft angle's equal gears.)
    if d1.to_degrees() > 85.0 + 1e-6 || d2.to_degrees() > 85.0 + 1e-6 || d1.to_degrees() < 2.0 || d2.to_degrees() < 2.0 {
        p.refuse(format!(
            "pitch cone angles of {}° and {}°: crown and internal bevels (a cone angle over 85°) aren't built; lower the shaft angle or bring the tooth counts closer",
            num(d1.to_degrees()),
            num(d2.to_degrees())
        ));
        return;
    }
    p.fact("ratio", z2 / z1, Unit::None);
    p.fact("shaft angle", s.shaft_angle, Unit::Deg);
    if spiral != 0.0 {
        p.fact("mean spiral angle", s.spiral_angle, Unit::Deg);
    }
    let r1 = s.module * z1 / 2.0;
    let r2 = s.module * z2 / 2.0;
    let re = r1 / d1.sin();
    let h1 = re * d1.cos();
    let h2 = re * d2.cos();
    // Gear 1 along +z, apex at (0, 0, h1); gear 2's axis through the same apex at the shaft
    // angle, in the xz plane.
    // Gear 1 has a tooth on the pitch line the two cones share (towards +x) at the middle of
    // the face; gear 2, which sees that line at its own angle π, a space.
    let Some([f1, f2]) = cone_faces(s, re, p, ["gear 1", "gear 2"]) else { return };
    let (t1, _) = bevel_member(s, s.teeth, d1, spiral, 0.0, f1, p, "gear 1");
    let ph2 = PI / z2 + PI;
    let (t2, _) = bevel_member(s, s.teeth2, d2, -spiral, ph2, f2, p, "gear 2");
    let apex = [s.origin[0], s.origin[1], s.origin[2] + h1];
    // Gear 2's axis points from its large end towards the apex: from the apex, back along the
    // direction at the shaft angle from gear 1's axis.
    let ax2 = [-sigma.sin(), 0.0, sigma.cos()];
    let origin2 = [apex[0] - ax2[0] * h2, apex[1] - ax2[1] * h2, apex[2] - ax2[2] * h2];
    let frame2 = Frame { origin: origin2, x: [sigma.cos(), 0.0, sigma.sin()], z: ax2 };
    // The abstract shapes (heel circles, toe pitch circle, apex) and the pitch line the cones
    // share (gear 1's tooth towards +x), for a preview.
    let built = [(t1.tip_radius, h1), (t2.tip_radius, h2)];
    let v_teeth = s.herringbone && spiral != 0.0;
    for (k, (f, r, d, face, frame)) in [(f1, r1, d1, f1, Frame::at(s.origin)), (f2, r2, d2, f2, frame2)].into_iter().enumerate() {
        let (ha, hf) = (s.rack.addendum * s.module, s.rack.dedendum * s.module);
        let k_heel = f.heel / re;
        let (tip_heel, apex_z) = built[k];
        let groove = V_RELIEF * s.module;
        let bevel_shape = BevelShape {
            delta: d,
            apex_z,
            heel: f.heel,
            toe: f.toe(),
            tip_heel,
            cone_ends: d > FLAT_CONE,
            v_groove: (v_teeth && f.toe() < f.mid && f.mid < f.heel).then_some((f.mid - groove / 2.0, f.mid + groove / 2.0)),
        };
        p.shapes.push(MemberShape {
            member: k,
            center: frame.origin,
            axis: frame.z,
            face: face.width * d.cos(),
            pitch_diameter: 2.0 * r * k_heel,
            tip_diameter: 2.0 * k_heel * (r + ha * d.cos()),
            root_diameter: 2.0 * k_heel * (r - hf * d.cos()),
            apex: Some(apex),
            toe_pitch_diameter: Some(2.0 * r * face.toe() / re),
            bevel: Some(bevel_shape),
        });
    }
    {
        let (lo, hi) = faces_overlap(s, re);
        let heel = [s.origin[0] + r1, s.origin[1], s.origin[2]];
        let along = |t: f64| [0, 1, 2].map(|i| apex[i] + (heel[i] - apex[i]) * t / re);
        p.contacts.push(Contact { a: 0, b: 1, from: along(lo), to: along(hi) });
    }
    if wanted(s, 1) {
        let mut steps = t1.steps;
        bevel_hub(s, &s.bore, r1 * 0.6, &t1.ends, &mut steps, p, "gear 1");
        p.bodies.push(Body { name: name(s, &format!("Bevel z{}", s.teeth)), frame: Frame::at(s.origin), steps });
    }
    if wanted(s, 2) {
        let mut steps = t2.steps;
        bevel_hub(s, &s.bore2, r2 * 0.6, &t2.ends, &mut steps, p, "gear 2");
        p.bodies.push(Body { name: name(s, &format!("Bevel z{}", s.teeth2)), frame: frame2, steps });
    }
    // The preview: the two virtual (back-cone) spur gears in mesh. A virtual gear's tooth count
    // (teeth / cos of the pitch cone angle) is rarely whole, and a gear drawn all the way round
    // needs a whole count, so each is drawn with the nearest: the teeth are the 3D ones to
    // within that rounding, and the pair meshes all the way round.
    let virt = |z: f64, d: f64, helix: f64, shift: f64| {
        bevel_involute(s, (z / d.cos().max(1e-3)).round().clamp(3.0, 2000.0) as u32, spiral, helix, shift, 0.0)
    };
    let (v1, v2) = (virt(z1, d1, spiral, s.shift), virt(z2, d2, -spiral, -s.shift));
    let (rv1, rv2) = (v1.r(), v2.r());
    p.preview_note = "back-cone (Tredgold) development at the large end, virtual tooth counts rounded".into();
    p.preview.push(PreviewGear::turning("gear 1", Region { outer: vec![external_outline(&v1, 0.0).0], holes: vec![] }, V2::ZERO, 1.0, rv1));
    p.preview.push(PreviewGear::turning(
        "gear 2",
        Region { outer: vec![external_outline(&v2, mesh_phase(v1.teeth, 0.0, v2.teeth, 0.0)).0], holes: vec![] },
        V2::new(rv1 + rv2, 0.0),
        -f64::from(v1.teeth) / f64::from(v2.teeth),
        rv2,
    ));
}

/// A bevel's hub on its back face and its bore.
fn bevel_hub(s: &Spec, bore: &Bore, max_r: f64, ends: &ConeEnds, steps: &mut Vec<Step>, p: &mut Plan, who: &str) {
    let back = ends.back;
    let built_teeth = !steps.is_empty();
    // The bore runs through to the small end, where the roots are closest to the axis.
    let small_root = built_teeth.then_some(ends.small_root);
    if let (Some(root), Some(_)) = (small_root, bore.outline())
        && bore.reach() >= root - 0.05 * s.module
    {
        let words = crate::hub::root_words(bore, 2.0 * root, s.module);
        p.refuse(format!(
            "{who}{}",
            if bore.on_shaft {
                words.replace("its tooth roots", "its tooth roots at the small end")
            } else {
                words.replace("the tooth roots", "the tooth roots at the small end")
            }
        ));
        return;
    }
    let hub_r = (s.hub_diameter / 2.0).min(max_r);
    let hub_l = if hub_r > 0.0 && s.hub_length > 0.0 { s.hub_length } else { 0.0 };
    if hub_l > 0.0 {
        steps.push(Step::Prism {
            region: Region { outer: vec![Outline::circle(V2::ZERO, hub_r)], holes: vec![] },
            z0: back - s.hub_length,
            height: s.hub_length + 0.01,
            twist: 0.0,
            op: Op::Join,
        });
    }
    // Shafts: the back one from the large end's face (through the hub), the front one from the
    // small end, where the roots are the limit.
    let front = built_teeth.then_some(ends.front);
    let second = std::ptr::eq(bore, &s.bore2);
    let built = front.map(|z| crate::body::shafts(s, second, bore, back, max_r / 0.6, z, small_root.unwrap_or(max_r), steps, p, who));
    if let Some(o) = bore.outline() {
        if bore.reach() > max_r {
            p.warn(format!("{who}: the bore is large for this gear"));
        }
        let low = built.map_or(back, |b| b.lo).min(back - s.hub_length.max(0.0)) - 1.0;
        steps.push(Step::Prism {
            region: Region { outer: vec![o], holes: vec![] },
            z0: low,
            height: 1e4_f64.min(10.0 * s.module * f64::from(s.teeth.max(s.teeth2)) + 100.0),
            twist: 0.0,
            op: Op::Cut,
        });
    }
    if let (Some(b), Some(z)) = (built, front) {
        crate::body::shaft_ends(&b, bore, back, hub_l, z, 0.0, steps, p, who);
    }
}

/// Crossed helical and hypoid geometry share frames: gear 1 on +z at the origin, gear 2's
/// centre `e` along +x with its axis turned `sigma` about x.
fn crossed_frame2(origin: [f64; 3], e: f64, sigma: f64) -> Frame {
    Frame { origin: [origin[0] + e, origin[1], origin[2]], x: [-1.0, 0.0, 0.0], z: [0.0, sigma.sin(), sigma.cos()] }
}

fn crossed(s: &Spec, p: &mut Plan) {
    let sigma = s.shaft_angle.to_radians();
    let b1 = s.helix.to_radians();
    // b1 + b2 is the shaft angle, or (the mirror set, left-hand past square) 180° less.
    let b2 = match sigma - b1 {
        b if b > FRAC_PI_2 + 1e-9 => b - PI,
        b if b < -FRAC_PI_2 - 1e-9 => b + PI,
        b => b,
    };
    if b2.abs() >= 75f64.to_radians() || b1.abs() >= 75f64.to_radians() {
        p.refuse(format!(
            "gear 2's helix angle is the shaft angle less gear 1's: {}° − {}° = {}°; both must be under 75°",
            num(s.shaft_angle),
            num(s.helix),
            num(b2.to_degrees())
        ));
        return;
    }
    if b2.abs() > 60f64.to_radians() + 1e-9 || b1.abs() > 60f64.to_radians() + 1e-9 {
        p.warn("a helix angle over 60° makes impractical gears; balance gear 1's helix against the shaft angle");
    }
    let g1 = s.involute(s.teeth, s.shift, b1.to_degrees());
    let g2 = s.involute2(s.teeth2, s.shift2, b2.to_degrees());
    let e = g1.r() + g2.r() + (s.shift + s.shift2) * s.module;
    let (o1, f1) = external_outline(&g1, 0.0);
    let (o2, f2) = external_outline(&g2, PI / f64::from(s.teeth2));
    tooth_warnings(p, "gear 1", &g1, &f1);
    tooth_warnings(p, "gear 2", &g2, &f2);
    p.fact("ratio", f64::from(s.teeth2) / f64::from(s.teeth), Unit::None);
    p.fact("centre distance", e, Unit::Mm);
    p.fact("gear 1 helix angle", b1.to_degrees(), Unit::Deg);
    p.fact("gear 2 helix angle", b2.to_degrees(), Unit::Deg);
    involute_facts(p, "gear 1", &g1, &f1);
    involute_facts(p, "gear 2", &g2, &f2);
    let face = s.face_width;
    if wanted(s, 1) {
        let mut steps = vec![Step::Prism {
            region: Region { outer: vec![o1.rotated(-twist_for(&g1, face) / 2.0)], holes: vec![] },
            z0: -face / 2.0,
            height: face,
            twist: twist_for(&g1, face),
            op: Op::New,
        }];
        hub_steps_centered(s, &s.bore, f1.rf, face, &mut steps, p, "gear 1");
        p.bodies.push(Body { name: name(s, &format!("Crossed helical z{}", s.teeth)), frame: Frame::at(s.origin), steps });
    }
    if wanted(s, 2) {
        let mut steps = vec![Step::Prism {
            region: Region { outer: vec![o2.rotated(-twist_for(&g2, face) / 2.0)], holes: vec![] },
            z0: -face / 2.0,
            height: face,
            twist: twist_for(&g2, face),
            op: Op::New,
        }];
        hub_steps_centered(s, &s.bore2, f2.rf, face, &mut steps, p, "gear 2");
        p.bodies.push(Body { name: name(s, &format!("Crossed helical z{}", s.teeth2)), frame: crossed_frame2(s.origin, e, sigma), steps });
    }
    p.preview_note = "each gear's transverse section".into();
    side_by_side(p, o1, g1.r(), o2, g2.r(), f64::from(s.teeth) / f64::from(s.teeth2));
}

/// Two gears drawn apart (their axes don't share a plane), each turning at its own speed.
fn side_by_side(p: &mut Plan, o1: Outline, r1: f64, o2: Outline, r2: f64, speed2: f64) {
    let gap = 0.3 * (r1 + r2);
    p.preview.push(PreviewGear::turning("gear 1", Region { outer: vec![o1], holes: vec![] }, V2::ZERO, 1.0, r1));
    p.preview.push(PreviewGear::turning(
        "gear 2",
        Region { outer: vec![o2], holes: vec![] },
        V2::new(r1 + r2 + gap + 0.0 * FRAC_PI_2, 0.0),
        -speed2,
        r2,
    ));
}

/// Like [`hub_steps`] for a gear whose face runs from −face/2 to face/2.
fn hub_steps_centered(s: &Spec, bore: &Bore, rf: f64, face: f64, steps: &mut Vec<Step>, p: &mut Plan, who: &str) {
    let mut local = Vec::new();
    let tip = if s.tooth_chamfer_front.size > 0.0 || s.tooth_chamfer_back.size > 0.0 { crate::body::tip_radius(steps) } else { 0.0 };
    hub_steps_tip(s, bore, rf, tip, face, &mut local, p, who);
    steps.extend(local.into_iter().map(|st| crate::body::shift_z(st, -face / 2.0)));
}

/// Sections a throated worm wheel is lofted through, across its face: at least, and at most.
const WHEEL_SECTIONS_MIN: usize = 9;
const WHEEL_SECTIONS_MAX: usize = 15;
/// How far (× module) the loft's skin may be from the true tooth between its sections.
const WHEEL_SKIN_TOLERANCE: f64 = 4e-4;

/// The flanks' points as plane points (lower side then upper), short of the tips: where the
/// teeth work against the thread.
fn side_points_xy(t: &ToothSides) -> Vec<V2> {
    [&t.lower, &t.upper].iter().filter_map(|s| s.last()).flat_map(|f| f[..f.len().saturating_sub(2)].iter()).map(|q| V2::polar(q.0, q.1)).collect()
}

/// How far the cubic skin through the wheel's sections strays from the flanks the hob cuts
/// half way between them (the outermost gaps and one next to the mid plane, where it is worst): the
/// largest distance, along the circle, of an interpolated flank point from the true flank.
fn skin_error(hob: &Hob, sections: &[(f64, ToothSides)], zw: u32) -> f64 {
    let n = sections.len();
    let ws: Vec<f64> = sections.iter().map(|s| s.0).collect();
    let pts: Vec<Vec<V2>> = sections.iter().map(|s| side_points_xy(&s.1)).collect();
    if pts.iter().any(|p| p.len() != pts[0].len()) {
        return f64::INFINITY;
    }
    let half = pts[0].len() / 2;
    let mut worst = 0.0f64;
    // The skin is about symmetric across the mid plane: one side's gaps are checked.
    for k in [0, 1, n / 2] {
        let w = 0.5 * (ws[k] + ws[k + 1]);
        let side = hob.side_angle(w, zw);
        let weights = crate::mesh3d::spline_weights(&ws, w);
        for i in 0..pts[0].len() {
            let q = weights.iter().zip(&pts).fold(V2::ZERO, |acc, (c, p)| acc + p[i] * *c);
            let r = q.len();
            let e = r * crate::geom::wrap(q.angle() - side(usize::from(i >= half), r)).abs();
            worst = worst.max(e);
        }
    }
    worst
}

fn worm(s: &Spec, p: &mut Plan) {
    let m = s.module;
    let z1 = s.starts.max(1);
    let zw = s.teeth;
    let d1 = if s.worm_diameter > 0.0 { s.worm_diameter } else { 10.0 * m };
    let lead = PI * m * f64::from(z1);
    let gamma = (lead / (PI * d1)).atan();
    let an = s.pressure_angle.to_radians();
    let ax = (an.tan() / gamma.cos()).atan();
    let ha = s.rack.addendum * m;
    let hf = s.rack.dedendum * m;
    let clearance = hf - ha;
    let (r1, ra1, rf1) = (d1 / 2.0, d1 / 2.0 + ha, d1 / 2.0 - hf);
    // A negative helix angle asks for a left-hand worm; the wheel's teeth lean the same way.
    let left = s.helix < 0.0;
    let hand = if left { -1.0 } else { 1.0 };
    let r2 = m * f64::from(zw) / 2.0;
    let a = r1 + r2 + s.shift * m;
    // The wheel's throat (its tips at the mid plane, a root clearance off the worm's root) and
    // the cylinder its tips are cut back to further out.
    let throat = a - rf1 - clearance;
    let outside = throat + 0.5 * m;
    p.fact("ratio", f64::from(zw) / f64::from(z1), Unit::None);
    p.fact("centre distance", a, Unit::Mm);
    p.fact("lead angle", gamma.to_degrees(), Unit::Deg);
    p.fact("lead", lead, Unit::Mm);
    p.fact("axial pitch", PI * m, Unit::Mm);
    p.fact("worm pitch diameter", d1, Unit::Mm);
    p.fact("worm tip diameter", 2.0 * ra1, Unit::Mm);
    p.fact("worm root diameter", 2.0 * rf1, Unit::Mm);
    p.fact("wheel pitch diameter", 2.0 * r2, Unit::Mm);
    p.fact("wheel tip diameter", 2.0 * throat, Unit::Mm);
    p.fact("diameter quotient q", d1 / m, Unit::None);
    if gamma.to_degrees() < 5.0 {
        p.fact("self-locking (likely, lead angle under ~5°)", 1.0, Unit::None);
    }
    if gamma.to_degrees() > 30.0 {
        p.warn("lead angle over 30°: use a larger worm diameter or fewer starts");
    }
    if rf1 <= 0.5 * m {
        p.refuse(format!("the worm's root (Ø{}) is at or near its axis: use a worm diameter over {} mm", num(2.0 * rf1), num(2.0 * (hf + 0.5 * m))));
        return;
    }
    if gamma.to_degrees() > 45.0 {
        p.refuse(format!("lead angle {}° is over 45°: use a larger worm diameter or fewer starts", num(gamma.to_degrees())));
        return;
    }
    if a - ra1 - clearance <= 0.5 * m {
        p.refuse("the wheel is too small for this worm: use more teeth or a smaller worm diameter");
        return;
    }
    if s.bore2.outline().is_some() && s.bore2.reach() >= rf1 - 0.05 * m {
        p.refuse(format!(
            "worm: the bore (reaching Ø{}) cuts into the thread roots (Ø{}); use a smaller bore",
            num(2.0 * s.bore2.reach()),
            num(2.0 * rf1)
        ));
        return;
    }
    // The wheel's face: no wider than the worm's root (beyond it the teeth no longer wrap the
    // worm), nor than where the screw leans a thread flank over to under 6° of pressure angle
    // on the planes furthest off the mid plane (past that the threads undercut the teeth).
    let lean_limit = {
        let ok = |th: f64| ax.tan() * th.cos() - gamma.tan() * th.sin() >= 6f64.to_radians().tan();
        let (mut lo, mut hi) = (0.0, FRAC_PI_2);
        for _ in 0..50 {
            let mid = 0.5 * (lo + hi);
            if ok(mid) { lo = mid } else { hi = mid }
        }
        2.0 * r1 * lo.sin()
    };
    let widest = (1.8 * rf1).min(lean_limit);
    let face = s.face_width.min(widest);
    if face < s.face_width - 1e-9 {
        p.warn(format!("wheel: the face is narrowed to {} mm, the most this worm's threads wrap without undercutting the teeth", num(face)));
    }
    p.fact("wheel face width", face, Unit::Mm);
    let length =
        if s.worm_length > 0.0 { s.worm_length } else { (2.0 * (outside.powi(2) - (r2 - m).powi(2)).max(0.0).sqrt() + lead).max(4.0 * PI * m) };
    // Planning time grows with the turns of the thread: a worm far longer than any wheel engages
    // is refused rather than built for minutes.
    const MOST_WORM_TURNS: f64 = 1000.0;
    if length / lead > MOST_WORM_TURNS {
        p.refuse(format!(
            "worm: a {} mm worm is {} turns of its thread, over the {} that can be built: use a worm length of at most {} mm",
            num(length),
            num((length / lead).round()),
            num(MOST_WORM_TURNS),
            num((MOST_WORM_TURNS * lead).floor())
        ));
        return;
    }
    p.fact("worm length", length, Unit::Mm);
    // The thread in axial section: straight sides at the axial pressure angle (a ZA worm). The
    // worm's thread is thinner by half the backlash, the hob's (the wheel's spaces) wider.
    let hob = Hob {
        centre: a,
        wheel_pitch: r2,
        reference: r1,
        // The hob's tooth is the thread made taller by the clearance, but no taller than where
        // its flanks (wider apart than the worm's by the backlash) would meet: a steep or thin
        // thread's hob is cut back to a narrow land there (the wheel's root a little shallower).
        tip: (ra1 + clearance).min(r1 + (PI * m / 4.0 + s.backlash / 4.0 - 0.02 * m) / ax.tan()),
        corner: s.rack.root_radius * m,
        low: rf1 - 0.5 * m,
        half: PI * m / 4.0 + s.backlash / 4.0,
        tan_axial: ax.tan(),
        screw: hand * lead / TAU,
    };
    // The worm (member 2) is thinned by its own backlash.
    let half = |rr: f64| (PI * m / 4.0 - s.backlash_of2() / 4.0 - (rr - r1) * ax.tan()).max(0.01 * m);
    if PI * m / 4.0 - s.backlash_of2() / 4.0 - ha * ax.tan() < 0.05 * m {
        p.refuse("the worm's threads come to a point: lower the pressure angle or the addendum");
        return;
    }
    // The wheel: sections across the face, each cut by the thread as that plane meets it, the
    // tips following the worm's root (the throat) and eased onto the outside cylinder: the
    // easing is smooth across the face (the throat's rise less a quadratic of how near it comes
    // to the cylinder), as a cubic skin through the sections can follow, where a sharp edge in
    // the tip height would ripple every flank point the sections place by it.
    let ease = outside - throat;
    let tip_at = |w: f64| {
        let u = a - ((rf1 + clearance).powi(2) - w * w).max(0.0).sqrt() - outside;
        outside
            + if u <= -ease {
                u
            } else if u >= ease {
                0.0
            } else {
                u - (u + ease).powi(2) / (4.0 * ease)
            }
    };
    p.fact("wheel outside diameter", 2.0 * tip_at(face / 2.0), Unit::Mm);
    // The kernel skins the loft cubic across its sections; the true tooth bends more across the
    // face the steeper the lead, so sections are added (an odd count, one on the mid plane)
    // until the skin between them is within a small part of the profile tolerance of the
    // section the hob cuts there.
    let mut sections: Vec<(f64, ToothSides)> = Vec::new();
    let mut count;
    // Fit points per tooth side (fillet, flank): the full count first; a big, steep wheel whose
    // loft cannot line up within the kernel's curve limit takes fewer points per tooth, so that
    // more sections fit (and coarser segments line up across a bigger turn).
    let densities = [(crate::worm::FILLET_POINTS, crate::worm::FLANK_POINTS), (5, 6), (4, 4)];
    let mut misaligned = false;
    let mut few = None;
    for (d, &(fillet_points, flank_points)) in densities.iter().enumerate() {
        let last = d + 1 == densities.len();
        let tooth = |w: f64| hob.tooth_with(w, tip_at(w), zw, m, fillet_points, flank_points);
        // The most sections within the kernel's loft limit (an odd count, one on the mid plane).
        let per_section = tooth(0.0).map_or(1, |t| loft_curves(&sided_outline(&t, zw, PI / f64::from(zw))).max(1));
        let most = {
            let n = (LOFT_CURVE_LIMIT / per_section).max(3);
            if n.is_multiple_of(2) { n - 1 } else { n }
        };
        count = WHEEL_SECTIONS_MIN.min(most);
        loop {
            sections.clear();
            for i in 0..count {
                let w = -face / 2.0 * (PI * i as f64 / (count - 1) as f64).cos();
                match tooth(w) {
                    Some(t) => sections.push((w, t)),
                    None => {
                        p.refuse(format!("wheel: {zw} teeth are too few for this worm (no tooth is left {} mm off the mid plane): use more teeth, a narrower face or a larger pressure angle", num(w.abs())));
                        return;
                    }
                }
            }
            let err = skin_error(&hob, &sections, zw);
            // The kernel restarts each loft section where it best lines up with the one before;
            // a steep multi-start wheel turns enough between sections to be matched a few
            // segments off (its walls would cut across the teeth), so sections are added until
            // it lines up.
            let outlines: Vec<Outline> = sections.iter().map(|(_, t)| sided_outline(t, zw, PI / f64::from(zw))).collect();
            let lined_up = !crate::hypoid::slips(&outlines, zw as usize);
            if count + 2 > most || (count >= WHEEL_SECTIONS_MAX || err <= WHEEL_SKIN_TOLERANCE * m) && lined_up || count >= WHEEL_SECTIONS_SLIP_MAX {
                misaligned = !lined_up;
                few = (most < WHEEL_SECTIONS_MIN).then_some(most);
                break;
            }
            count += 2;
        }
        if !misaligned || last {
            if d > 0 {
                p.warn(format!("wheel: {zw} teeth on a steep worm make a large loft; each tooth side is drawn with fewer points ({fillet_points} on the fillet, {} on the flank) to stay within the kernel's loft limit", flank_points + 1));
            }
            break;
        }
    }
    let count = sections.len();
    if let Some(most) = few {
        p.warn(format!("wheel: {zw} teeth make a large loft; it is built through {most} sections (fewer than usual), so check the teeth across the face after building"));
    }
    if misaligned {
        p.warn("wheel: the kernel may mis-align this wheel's loft sections (a steep multi-start worm): check the teeth after building, or use fewer starts or a narrower face");
    }
    p.fact("wheel loft sections", count as f64, Unit::None);
    let mid_tooth = sections[count / 2].1.clone();
    let outlines: Vec<(Outline, f64)> = sections.iter().map(|(w, t)| (sided_outline(t, zw, PI / f64::from(zw)), *w)).collect();
    let mid_outline = outlines[count / 2].0.clone();
    let tip_land = |t: &ToothSides| {
        t.ra * (t.upper.last().and_then(|q| q.last()).map_or(0.0, |q| q.1) - t.lower.last().and_then(|q| q.last()).map_or(0.0, |q| q.1))
    };
    if sections.iter().any(|(w, t)| t.ra < tip_at(*w) - 1e-6) {
        p.warn("wheel: the teeth come to a point towards the edges of the face; the tips are lowered there");
    } else if tip_land(&mid_tooth) < 0.25 * m {
        p.warn(format!("wheel: thin tooth tips ({} mm; 0.25 × module or more is usual)", num(tip_land(&mid_tooth))));
    }
    // In the mid plane the wheel is a spur gear of the worm's axial module and pressure angle.
    let mid_gear = Involute {
        teeth: zw,
        module: m,
        pressure_angle: ax,
        helix: 0.0,
        shift: s.shift,
        rack: s.rack,
        backlash: s.backlash / 2.0,
        tip_shortening: 0.0,
        virtual_teeth: 0.0,
    };
    if mid_gear.undercut() {
        p.warn(format!("wheel: undercut ({zw} teeth; about {:.1} or more avoid it at this profile shift): use more teeth (a finer module) or a larger pressure angle", mid_gear.min_teeth_no_undercut()));
    }
    if wanted(s, 1) {
        let mut steps = vec![Step::Loft { sections: outlines, op: Op::New }];
        hub_steps_centered(s, &s.bore, hob.root_radius(0.0), face, &mut steps, p, "wheel");
        p.bodies.push(Body { name: name(s, &format!("Worm wheel z{zw}")), frame: Frame::at(s.origin), steps });
    }
    if wanted(s, 2) {
        let section_at = |z: f64| -> Vec<V2> {
            let inner = rf1 - 0.05 * m;
            vec![V2::new(inner, z - half(inner)), V2::new(ra1, z - half(ra1)), V2::new(ra1, z + half(ra1)), V2::new(inner, z + half(inner))]
        };
        let mut steps = vec![Step::Prism {
            region: Region { outer: vec![Outline::circle(V2::ZERO, rf1)], holes: vec![] },
            z0: -length / 2.0,
            height: length,
            twist: 0.0,
            op: Op::New,
        }];
        // The worm's frame has x towards the wheel's axis: a coil starts on that side, and one
        // thread is centred on the wheel's mid plane there, in the tooth space the wheel turns
        // towards the worm.
        let below = (length / 2.0 / lead).ceil() + 1.0;
        for k in 0..z1 {
            let z = -below * lead + PI * m * f64::from(k);
            steps.push(Step::Coil { section: section_at(z), pitch: lead, z0: z, height: 2.0 * below * lead, left, op: Op::Join });
        }
        steps.push(Step::Prism {
            region: Region { outer: vec![Outline::circle(V2::ZERO, ra1 + 0.01)], holes: vec![] },
            z0: -length / 2.0,
            height: length,
            twist: 0.0,
            op: Op::Intersect,
        });
        // The worm's shafts (gear 2's) run on from its two ends.
        let built = crate::body::shafts(s, true, &s.bore2, -length / 2.0, rf1, length / 2.0, rf1, &mut steps, p, "worm");
        if let Some(b) = s.bore2.outline() {
            steps.push(Step::Prism {
                region: Region { outer: vec![b], holes: vec![] },
                z0: built.lo - 1.0,
                height: built.hi - built.lo + 2.0,
                twist: 0.0,
                op: Op::Cut,
            });
        }
        crate::body::shaft_ends(&built, &s.bore2, -length / 2.0, 0.0, length / 2.0, 0.0, &mut steps, p, "worm");
        p.bodies.push(Body { name: name(s, &format!("Worm {z1}-start")), frame: crossed_frame2(s.origin, a, FRAC_PI_2), steps });
    }
    // The preview: the wheel's mid plane section and the worm's axial section (a rack) in it.
    p.preview_note = "wheel mid plane, worm in axial section".into();
    let rack_g = Involute {
        teeth: 1,
        module: m,
        pressure_angle: ax,
        helix: 0.0,
        shift: 0.0,
        rack: s.rack,
        backlash: s.backlash / 2.0,
        tip_shortening: 0.0,
        virtual_teeth: 0.0,
    };
    let n = (length / (PI * m)).ceil().max(1.0) as u32;
    // Teeth towards the wheel (−x) from the worm's reference line; a thread centred on the
    // wheel's mid plane at rest (the preview slides it from half a pitch back), as the 3D
    // worm's is.
    let rack_o = rack_outline(&rack_g, n, ha + 0.5 * m).map(&|q: V2| V2::new(-q.y, q.x - PI * m * f64::from(n / 2)), false);
    p.preview.push(PreviewGear::turning("wheel", Region { outer: vec![mid_outline], holes: vec![] }, V2::ZERO, 1.0, r2));
    p.preview.push(PreviewGear {
        name: "worm".into(),
        region: Region { outer: vec![rack_o], holes: vec![] },
        center: V2::new(a - r1, 0.0),
        speed: 0.0,
        slide: r2,
        pitch_radius: 0.0,
        orbit: 0.0,
        period: PI * m,
    });
}

fn cycloidal(s: &Spec, p: &mut Plan) {
    let m = s.module;
    let (z1, z2) = (s.teeth, s.teeth2);
    let r1 = m * f64::from(z1) / 2.0;
    let r2 = m * f64::from(z2) / 2.0;
    let single_only = z2 == 0;
    let c1 = Cycloidal {
        teeth: z1,
        module: m,
        rolling_radius_addendum: if single_only { r1 / 2.0 } else { r2 / 2.0 },
        rolling_radius_dedendum: r1 / 2.0,
        addendum: 1.0 + 0.0 * s.rack.addendum,
        dedendum: s.rack.dedendum.max(1.2),
        backlash: s.backlash,
    };
    let (o1, f1) = cycloidal_outline(&c1, 0.0);
    p.fact("gear 1 pitch diameter", 2.0 * r1, Unit::Mm);
    p.fact("gear 1 tip diameter", 2.0 * f1.ra, Unit::Mm);
    if wanted(s, 1) {
        let mut steps =
            vec![Step::Prism { region: Region { outer: vec![o1.clone()], holes: vec![] }, z0: 0.0, height: s.face_width, twist: 0.0, op: Op::New }];
        hub_steps(s, &s.bore, f1.rf, s.face_width, &mut steps, p, "gear 1");
        p.bodies.push(Body { name: name(s, &format!("Cycloidal z{z1}")), frame: Frame::at(s.origin), steps });
    }
    p.preview.push(PreviewGear::turning("gear 1", with_bore(Region { outer: vec![o1], holes: vec![] }, &s.bore), V2::ZERO, 1.0, r1));
    if !single_only {
        let c2 = Cycloidal {
            teeth: z2,
            module: m,
            rolling_radius_addendum: r1 / 2.0,
            rolling_radius_dedendum: r2 / 2.0,
            addendum: 1.0,
            dedendum: s.rack.dedendum.max(1.2),
            backlash: s.backlash,
        };
        let ph2 = mesh_phase(z1, 0.0, z2, 0.0);
        let (o2, f2) = cycloidal_outline(&c2, ph2);
        p.fact("ratio", f64::from(z1) / f64::from(z2), Unit::None);
        p.fact("centre distance", r1 + r2, Unit::Mm);
        p.fact("gear 2 pitch diameter", 2.0 * r2, Unit::Mm);
        p.fact("gear 2 tip diameter", 2.0 * f2.ra, Unit::Mm);
        if wanted(s, 2) {
            let mut steps = vec![Step::Prism {
                region: Region { outer: vec![o2.clone()], holes: vec![] },
                z0: 0.0,
                height: s.face_width,
                twist: 0.0,
                op: Op::New,
            }];
            hub_steps(s, &s.bore2, f2.rf, s.face_width, &mut steps, p, "gear 2");
            p.bodies.push(Body { name: name(s, &format!("Cycloidal z{z2}")), frame: Frame::at(s.origin).shifted([r1 + r2, 0.0, 0.0]), steps });
        }
        p.preview.push(PreviewGear::turning(
            "gear 2",
            with_bore(Region { outer: vec![o2], holes: vec![] }, &s.bore2),
            V2::new(r1 + r2, 0.0),
            -f64::from(z1) / f64::from(z2),
            r2,
        ));
    }
}

fn drive(s: &Spec, p: &mut Plan) {
    let d = CycloidalDrive {
        pins: s.pins,
        pin_circle_radius: s.pin_circle / 2.0,
        pin_radius: s.pin_diameter / 2.0,
        eccentricity: s.eccentricity,
        clearance: s.backlash / 2.0,
    };
    let disc = d.disc();
    let n = f64::from(s.pins);
    p.fact("reduction", n - 1.0, Unit::None);
    p.fact("lobes", n - 1.0, Unit::None);
    p.fact("largest eccentricity without cusps", d.max_eccentricity(), Unit::Mm);
    if s.eccentricity >= 0.98 * d.max_eccentricity() {
        p.refuse(format!(
            "eccentricity {} is at or over the most these pins allow ({}): the disc's lobes would loop; lower it, or use fewer pins or a bigger pin circle",
            num(s.eccentricity),
            num(0.98 * d.max_eccentricity())
        ));
        return;
    }
    let pitch_gap = TAU * d.pin_circle_radius / n - s.pin_diameter;
    if pitch_gap <= 0.02 * s.pin_diameter {
        p.refuse(format!(
            "the ring pins overlap ({} pins of Ø{} on a Ø{} circle): fewer pins, smaller pins or a bigger pin circle",
            s.pins,
            num(s.pin_diameter),
            num(s.pin_circle)
        ));
        return;
    }
    let valley = d.valley_radius();
    if valley <= 0.05 * s.pin_circle || d.undercut() {
        p.refuse(format!(
            "pins of Ø{} are too big for lobes this eccentric (the disc's profile would cut back on itself): use smaller pins, less eccentricity or fewer pins",
            num(s.pin_diameter)
        ));
        return;
    }
    // The disc on its eccentric (shifted e along +x), the pins round the origin.
    let mut holes = Vec::new();
    let reach = s.bore.reach();
    let wall = 0.03 * s.pin_circle;
    if let Some(b) = s.bore.outline() {
        if reach >= valley - wall {
            p.refuse(format!(
                "the bore (reaching Ø{}) leaves no disc inside the lobes (Ø{} at the valleys); use a smaller bore",
                num(2.0 * reach),
                num(2.0 * valley)
            ));
            return;
        }
        holes.push(b);
    }
    if s.holes > 0 {
        // Output holes: the output pins' diameter plus twice the eccentricity, on a circle
        // midway between the bore and the lobes' valleys.
        let (inner, outer) = (reach + wall, valley - wall);
        let at = 0.5 * (inner + outer);
        let spacing = if s.holes > 1 { 2.0 * at * (PI / f64::from(s.holes)).sin() } else { f64::INFINITY };
        let most = (outer - inner).min(0.85 * spacing);
        let pin = if s.hole_diameter > 0.0 { s.hole_diameter } else { (0.15 * s.pin_circle).min(most - 2.0 * s.eccentricity) };
        let hole = pin + 2.0 * s.eccentricity;
        if !(pin > 0.0) || hole > most {
            p.warn(format!(
                "{} output holes of Ø{} don't fit between the bore and the lobes (Ø{} at most); left out",
                s.holes,
                num(hole.max(0.0)),
                num(most.max(0.0))
            ));
        } else {
            holes.extend(lightening_holes(s.holes, hole, at, 0.0));
            p.fact("output hole diameter", hole, Unit::Mm);
            p.fact("output pin circle", 2.0 * at, Unit::Mm);
        }
    }
    let region = Region { outer: vec![disc.clone()], holes: holes.clone() };
    if wanted(s, 1) {
        p.bodies.push(Body {
            name: name(s, &format!("Cycloid disc {}-lobe", s.pins - 1)),
            frame: Frame::at(s.origin).shifted([s.eccentricity, 0.0, 0.0]),
            steps: vec![Step::Prism { region: region.clone(), z0: 0.0, height: s.face_width, twist: 0.0, op: Op::New }],
        });
    }
    let pins: Vec<Outline> = d.pin_centres().into_iter().map(|c| Outline::circle(c, d.pin_radius)).collect();
    if wanted(s, 2) {
        p.bodies.push(Body {
            name: name(s, &format!("Ring pins ×{}", s.pins)),
            frame: Frame::at(s.origin),
            steps: vec![Step::Prism {
                region: Region { outer: pins.clone(), holes: vec![] },
                z0: 0.0,
                height: s.face_width,
                twist: 0.0,
                op: Op::New,
            }],
        });
    }
    // The input crank carries the disc's centre round the pin circle's; the pins turn the disc
    // back one lobe per turn of the crank.
    p.preview.push(PreviewGear {
        name: "disc".into(),
        region,
        center: V2::new(s.eccentricity, 0.0),
        speed: -1.0 / (n - 1.0),
        slide: 0.0,
        pitch_radius: 0.0,
        orbit: 1.0,
        period: 0.0,
    });
    p.preview.push(PreviewGear::turning("pins", Region { outer: pins, holes: vec![] }, V2::ZERO, 0.0, 0.0));
    p.preview_note = "the disc turns slowly backwards while its centre goes round with the input".into();
}
