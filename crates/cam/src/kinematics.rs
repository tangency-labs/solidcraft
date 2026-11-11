//! Machine kinematics: how a machine's linear and rotary axes place the tool against the work.
//!
//! The model. Machine coordinates are fixed to the machine's base. The linear position `X`
//! (the `X Y Z` the controller is told) moves the head. With tool length compensation the tool
//! tip sits at `X` itself while every head rotary is at zero: that point is the head's control
//! point, and a head rotary's centre is given from it (so a head's pivot distance includes the
//! tool's gauge length: the distance from the pivot to the tip, set for the tool in use).
//!
//! Head rotaries turn the tool rigidly about their lines, the axis nearest the spindle first:
//! the tip goes to `X + H(θh)·0` and the tool's direction to `R_head·Z`. Table rotaries turn the
//! work about their lines, the axis nearest the work first: a work point `w` sits in the machine
//! at `T(θt)·(o + w)`, where `o` ([`Machine::work_origin`]) is where the work origin is in the
//! machine while every table rotary is at zero. So
//!
//! `X = T(θt)·(o + w) − H(θh)·0`, and the tool axis `a` in work coordinates obeys
//! `R_head·Z = R_table·a`.
//!
//! Every axis line and direction is given with all the rotaries at zero, and the work's axes are
//! the machine's there. A positive angle turns whatever the axis carries right-handed about its
//! direction: a table at `A = +30°` about `+X` turns the work's `+Y` towards `+Z`. A machine
//! whose axis turns the other way for a positive word has its direction reversed.
//!
//! The inverse. Turning a tool axis into rotary angles comes down, on every two-rotary machine,
//! to the angles `φ1`, `φ2` with `Rot(w1, φ1)·Rot(w2, φ2)·p = q`: a table-table machine needs
//! `R_t1·R_t2·a = Z` (`p = a`, `q = Z`), a head-head one `R_h1·R_h2·Z = a` (`p = Z`, `q = a`),
//! and a head-table one `R_t(−θt)·R_h(θh)·Z = a`. The vector `z = Rot(w2, φ2)·p` between the
//! two turns lies on the cone of `p` about `w2` and on the cone of `q` about `w1`; the two cones
//! meet in no, one or two such vectors, each giving the two angles as plain turns about one
//! axis. Of the solutions within the limits, the one nearest the previous position wins, angles
//! of continuous axes taken as the turn nearest the previous one (so they unwind past ±180°).
//! Where an angle does not matter (a table-table machine with the tool along its inner axis),
//! the previous value is kept, or zero.

use std::f64::consts::{PI, TAU};

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::frame::{angle_about, rotate, rotate_about, unit};
use crate::gcode::{MAX_FEED, MIN_FEED, feed_in_range};
use crate::multiaxis::{Move5, Pose, Toolpath5, slerp};
use crate::path::MAX_MOVES;
use crate::{CamError, Result, invalid};

/// How far a tool axis may be from one the machine can reach (radians).
const REACH_TOL: f64 = 1e-6;
/// Largest rotary angle and limit accepted (radians, about 1600 turns).
const MAX_ANGLE: f64 = 1.0e4;
/// Largest coordinate, centre or travel accepted (mm).
const MAX_COORD: f64 = 1.0e7;
/// Default distance from a head's pivot to the tool tip (mm), the tool's gauge length included.
pub const DEFAULT_PIVOT: f64 = 100.0;
/// Most pieces one feed move is cut into to follow its line within tolerance.
pub const MAX_PIECES: usize = 20_000;
/// Deepest halving of one feed move.
const MAX_DEPTH: u32 = 40;
/// Largest turn of the tool axis between the points a feed move is checked at for a flip.
const FLIP_SAMPLE: f64 = PI / 180.0;
/// Most points a feed move is checked at for a flip.
const MAX_FLIP_SAMPLES: usize = 1024;
/// Largest rotary turn between the points a written piece is checked at (radians).
const PIECE_SAMPLE: f64 = 2.0 * PI / 180.0;
/// Most points on each side of a written piece's middle it is checked at.
const MAX_PIECE_SAMPLES: usize = 32;
/// Tip travel (mm) over which the time of a piece goes over from the axis turn in degrees
/// taken as millimetres (a turn on the spot) to the tip's travel.
const TURN_FADE: f64 = 0.01;
/// The lever (mm) a turn of the tool axis is timed at once the tip moves: the travel of a
/// point that far from the tip.
const TURN_LEVER: f64 = 1.0;

/// The letter a rotary axis goes by: `A` turns about X, `B` about Y, `C` about Z by convention,
/// though the direction is the machine's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AxisLetter {
    A,
    B,
    C,
}

impl AxisLetter {
    pub fn char(self) -> char {
        match self {
            AxisLetter::A => 'A',
            AxisLetter::B => 'B',
            AxisLetter::C => 'C',
        }
    }
}

/// What a rotary axis turns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Carrier {
    /// The spindle head: the tool turns.
    Head,
    /// The table: the work turns.
    Table,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RotaryAxis {
    pub letter: AxisLetter,
    /// Unit direction in machine coordinates, every rotary at zero. A positive angle turns what
    /// the axis carries right-handed about it.
    pub direction: Vec3,
    /// A point on the axis line, every rotary at zero: in machine coordinates for a table axis,
    /// from the head's control point for a head axis.
    pub center: Vec3,
    pub carrier: Carrier,
    /// Lowest and highest angle (radians, as written); `None` for an axis that turns without
    /// end, its angles adding up past ±360° (a wrapped helix keeps turning one way).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<(f64, f64)>,
    /// Whether the controller's positive angle turns the axis the other way (left-handed about
    /// `direction`), as rotary tables wired the other way round do. Angles in [`Joints`] and in
    /// the program are the controller's; the kinematics turn them round.
    #[serde(default, skip_serializing_if = "is_false")]
    pub reversed: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl RotaryAxis {
    pub fn new(letter: AxisLetter, direction: Vec3, center: Vec3, carrier: Carrier, limits: Option<(f64, f64)>) -> RotaryAxis {
        RotaryAxis { letter, direction, center, carrier, limits, reversed: false }
    }

    /// The axis with the controller's positive angle turning the other way.
    pub fn reversed(mut self, reversed: bool) -> RotaryAxis {
        self.reversed = reversed;
        self
    }

    /// The unit direction (validated machines have a usable one).
    fn dir(&self) -> Vec3 {
        self.direction.normalized().unwrap_or(Vec3::Z)
    }

    /// 1, or -1 for a reversed axis: the right-handed turn of a written angle.
    fn sign(&self) -> f64 {
        if self.reversed { -1.0 } else { 1.0 }
    }

    fn validate(&self) -> Result<()> {
        let c = self.letter.char();
        unit(self.direction, &format!("the {c} axis direction"))?;
        if !self.center.is_finite() || !(self.center.len() <= MAX_COORD) {
            return invalid(format!("the {c} axis centre must be a finite point"));
        }
        if let Some((lo, hi)) = self.limits
            && (!(lo.abs() <= MAX_ANGLE) || !(hi.abs() <= MAX_ANGLE) || !(lo <= hi))
        {
            return invalid(format!("the {c} axis limits must be finite angles, the lowest first"));
        }
        Ok(())
    }
}

/// A machine tool's axes: three linear axes moving the head, and up to two rotaries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    pub name: String,
    /// None, one or two rotaries; head axes from the base to the spindle, table axes from the
    /// base to the work. Joint angles follow this order.
    pub rotaries: Vec<RotaryAxis>,
    /// Lowest and highest linear position (mm); `None` for unchecked travel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linear_limits: Option<(Vec3, Vec3)>,
    /// Where the work origin is in machine coordinates while every table rotary is at zero.
    /// For output in machine coordinates that is measured from the machine zero the program runs
    /// in, which is usually the rotary centre set as the work offset.
    #[serde(default)]
    pub work_origin: Vec3,
}

fn degrees(d: f64) -> f64 {
    d.to_radians()
}

impl Machine {
    /// Three linear axes and no rotary: the tool is always along `+Z`.
    pub fn three_axis() -> Machine {
        Machine { name: "3-axis".into(), rotaries: Vec::new(), linear_limits: None, work_origin: Vec3::ZERO }
    }

    /// A 4th axis: a rotary table (`A`) about `+X`, turning without end, set up the usual way
    /// with the work origin on its centreline (`Y0 Z0` on the axis). With the tool along `+Z`
    /// the program's `X Y Z` are then simply the pose's tip turned by `A` about X: `A = +90°`
    /// brings the work's `+Y` side up under the tool.
    pub fn four_axis_a() -> Machine {
        Machine::four_axis_a_with(Vec3::ZERO, false, None)
    }

    /// [`Machine::four_axis_a`] with the axis line through `center` (machine coordinates, the
    /// work origin at the machine origin): for a work origin off the centreline, `center` is
    /// where the centreline is from the work origin.
    pub fn four_axis_a_at(center: Vec3) -> Machine {
        Machine::four_axis_a_with(center, false, None)
    }

    /// [`Machine::four_axis_a_at`] for a controller whose positive `A` turns the table the
    /// other way (`reversed`, common where the rotary's motor is wired the other way round),
    /// and with `limits` (radians, as written) for a table that cannot turn without end.
    pub fn four_axis_a_with(center: Vec3, reversed: bool, limits: Option<(f64, f64)>) -> Machine {
        Machine {
            name: "4-axis A".into(),
            rotaries: vec![RotaryAxis::new(AxisLetter::A, Vec3::X, center, Carrier::Table, limits).reversed(reversed)],
            linear_limits: None,
            work_origin: Vec3::ZERO,
        }
    }

    /// A trunnion turning about `+X` (`A`, ±120°) carrying a rotary table about `+Z` (`C`,
    /// without end), both through the machine origin.
    pub fn table_table_ac() -> Machine {
        Machine::table_table_ac_at(Vec3::ZERO, Vec3::ZERO)
    }

    /// [`Machine::table_table_ac`] with the trunnion's line through `a_center` and the table's
    /// through `c_center` (with `A` at zero).
    pub fn table_table_ac_at(a_center: Vec3, c_center: Vec3) -> Machine {
        Machine {
            name: "table-table AC".into(),
            rotaries: vec![
                RotaryAxis::new(AxisLetter::A, Vec3::X, a_center, Carrier::Table, Some((degrees(-120.0), degrees(120.0)))),
                RotaryAxis::new(AxisLetter::C, Vec3::Z, c_center, Carrier::Table, None),
            ],
            linear_limits: None,
            work_origin: Vec3::ZERO,
        }
    }

    /// A trunnion turning about `+Y` (`B`, ±120°) carrying a rotary table about `+Z` (`C`,
    /// without end), both through the machine origin.
    pub fn table_table_bc() -> Machine {
        Machine::table_table_bc_at(Vec3::ZERO, Vec3::ZERO)
    }

    /// [`Machine::table_table_bc`] with the trunnion's line through `b_center` and the table's
    /// through `c_center` (with `B` at zero).
    pub fn table_table_bc_at(b_center: Vec3, c_center: Vec3) -> Machine {
        Machine {
            name: "table-table BC".into(),
            rotaries: vec![
                RotaryAxis::new(AxisLetter::B, Vec3::Y, b_center, Carrier::Table, Some((degrees(-120.0), degrees(120.0)))),
                RotaryAxis::new(AxisLetter::C, Vec3::Z, c_center, Carrier::Table, None),
            ],
            linear_limits: None,
            work_origin: Vec3::ZERO,
        }
    }

    /// A head tilting about `+Y` (`B`, ±120°) [`DEFAULT_PIVOT`] above the tool tip, and a
    /// rotary table about `+Z` (`C`, without end) through the machine origin.
    pub fn head_table_bc() -> Machine {
        Machine::head_table_bc_at(DEFAULT_PIVOT, Vec3::ZERO)
    }

    /// [`Machine::head_table_bc`] with the head's pivot `pivot` above the tool tip (pivot
    /// length plus tool gauge length) and the table's line through `c_center`.
    pub fn head_table_bc_at(pivot: f64, c_center: Vec3) -> Machine {
        Machine {
            name: "head-table BC".into(),
            rotaries: vec![
                RotaryAxis::new(AxisLetter::B, Vec3::Y, Vec3::new(0.0, 0.0, pivot), Carrier::Head, Some((degrees(-120.0), degrees(120.0)))),
                RotaryAxis::new(AxisLetter::C, Vec3::Z, c_center, Carrier::Table, None),
            ],
            linear_limits: None,
            work_origin: Vec3::ZERO,
        }
    }

    /// A fork head: `C` about `+Z` along the spindle line (without end) carrying `A` about `+X`
    /// (±120°) [`DEFAULT_PIVOT`] above the tool tip.
    pub fn head_head_ca() -> Machine {
        Machine::head_head_ca_at(DEFAULT_PIVOT)
    }

    /// [`Machine::head_head_ca`] with the fork's pivot `pivot` above the tool tip (pivot length
    /// plus tool gauge length).
    pub fn head_head_ca_at(pivot: f64) -> Machine {
        Machine {
            name: "head-head CA".into(),
            rotaries: vec![
                RotaryAxis::new(AxisLetter::C, Vec3::Z, Vec3::ZERO, Carrier::Head, None),
                RotaryAxis::new(AxisLetter::A, Vec3::X, Vec3::new(0.0, 0.0, pivot), Carrier::Head, Some((degrees(-120.0), degrees(120.0)))),
            ],
            linear_limits: None,
            work_origin: Vec3::ZERO,
        }
    }

    /// The machine with the work origin at `origin` (machine coordinates, every table rotary at zero).
    pub fn with_work_origin(mut self, origin: Vec3) -> Machine {
        self.work_origin = origin;
        self
    }

    /// The rotary letters in joint order.
    pub fn letters(&self) -> Vec<AxisLetter> {
        self.rotaries.iter().map(|r| r.letter).collect()
    }

    pub fn validate(&self) -> Result<()> {
        if self.name.len() > 200 {
            return invalid("a machine's name must be at most 200 characters");
        }
        if self.rotaries.len() > 2 {
            return Err(CamError::Unsupported("machines with more than two rotary axes".into()));
        }
        for r in &self.rotaries {
            r.validate()?;
        }
        if let [a, b] = self.rotaries.as_slice() {
            if a.letter == b.letter {
                return invalid(format!("the machine has two {} axes", a.letter.char()));
            }
            if a.dir().cross(b.dir()).len() < 1e-3 {
                return Err(CamError::Unsupported("two parallel rotary axes".into()));
            }
        }
        if let Some((lo, hi)) = self.linear_limits {
            let ok = |v: Vec3| v.is_finite() && v.len() <= MAX_COORD;
            if !ok(lo) || !ok(hi) || !(lo.x <= hi.x && lo.y <= hi.y && lo.z <= hi.z) {
                return invalid("the machine's linear limits must be finite, the lowest first");
            }
        }
        if !self.work_origin.is_finite() || !(self.work_origin.len() <= MAX_COORD) {
            return invalid("the machine's work origin must be a finite point");
        }
        Ok(())
    }
}

/// Where a machine's axes are: the linear position (mm) and the rotary angles (radians, in the
/// order of [`Machine::rotaries`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Joints {
    pub linear: Vec3,
    pub rotary: Vec<f64>,
}

impl Joints {
    /// The position a fraction `t` of the way to `other`, every axis moving evenly.
    pub fn lerp(&self, other: &Joints, t: f64) -> Joints {
        Joints { linear: self.linear.lerp(other.linear, t), rotary: self.rotary.iter().zip(&other.rotary).map(|(a, b)| a + (b - a) * t).collect() }
    }

    /// The largest turn of any rotary between this position and `other` (radians).
    pub fn rotary_change(&self, other: &Joints) -> f64 {
        self.rotary.iter().zip(&other.rotary).map(|(a, b)| (b - a).abs()).fold(0.0, f64::max)
    }
}

fn angle(rotary: &[f64], i: usize) -> f64 {
    rotary.get(i).copied().unwrap_or(0.0)
}

/// `p` (head coordinates, from the control point) turned by the head rotaries.
pub(crate) fn head_point(m: &Machine, rotary: &[f64], p: Vec3) -> Vec3 {
    m.rotaries
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, r)| r.carrier == Carrier::Head)
        .fold(p, |p, (i, r)| rotate_about(p, r.center, r.dir(), r.sign() * angle(rotary, i)))
}

/// The direction `d` turned by the head rotaries.
pub(crate) fn head_dir(m: &Machine, rotary: &[f64], d: Vec3) -> Vec3 {
    m.rotaries
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, r)| r.carrier == Carrier::Head)
        .fold(d, |d, (i, r)| rotate(d, r.dir(), r.sign() * angle(rotary, i)))
}

/// Where the point `p` (machine coordinates, every table rotary at zero) is with the table turned.
pub(crate) fn table_point(m: &Machine, rotary: &[f64], p: Vec3) -> Vec3 {
    m.rotaries
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, r)| r.carrier == Carrier::Table)
        .fold(p, |p, (i, r)| rotate_about(p, r.center, r.dir(), r.sign() * angle(rotary, i)))
}

/// The point of the table at zero that the turned table brings to `p`.
pub(crate) fn table_point_inv(m: &Machine, rotary: &[f64], p: Vec3) -> Vec3 {
    m.rotaries
        .iter()
        .enumerate()
        .filter(|(_, r)| r.carrier == Carrier::Table)
        .fold(p, |p, (i, r)| rotate_about(p, r.center, r.dir(), -r.sign() * angle(rotary, i)))
}

/// The direction of the table at zero that the turned table brings to `d`.
pub(crate) fn table_dir_inv(m: &Machine, rotary: &[f64], d: Vec3) -> Vec3 {
    m.rotaries.iter().enumerate().filter(|(_, r)| r.carrier == Carrier::Table).fold(d, |d, (i, r)| rotate(d, r.dir(), -r.sign() * angle(rotary, i)))
}

/// The tool axis in work coordinates with the rotaries at `rotary`.
pub fn tool_axis(machine: &Machine, rotary: &[f64]) -> Vec3 {
    table_dir_inv(machine, rotary, head_dir(machine, rotary, Vec3::Z))
}

/// Where the tool is against the work (work coordinates) with the machine's axes at `joints`.
pub fn forward(machine: &Machine, joints: &Joints) -> Pose {
    let tip = joints.linear + head_point(machine, &joints.rotary, Vec3::ZERO);
    Pose { tip: table_point_inv(machine, &joints.rotary, tip) - machine.work_origin, axis: tool_axis(machine, &joints.rotary) }
}

/// The angle that turns `p` into `q` about the unit `w`; `None` when either lies along `w`.
fn turn(p: Vec3, q: Vec3, w: Vec3) -> Option<f64> {
    angle_about(p, q, w)
}

/// The vectors `z` with `w1·z = w1·q`, `w2·z = w2·p` and `|z| = |p|` (unit `p`, `q`, `w1`, `w2`,
/// the two not parallel): none when the cones of `p` about `w2` and `q` about `w1` miss.
fn cones_meet(w1: Vec3, w2: Vec3, p: Vec3, q: Vec3) -> Vec<Vec3> {
    let c = w1.dot(w2);
    let n = w1.cross(w2);
    let n2 = n.len2();
    let d = c * c - 1.0;
    let alpha = (c * w2.dot(p) - w1.dot(q)) / d;
    let beta = (c * w1.dot(q) - w2.dot(p)) / d;
    let left = p.len2() - alpha * alpha - beta * beta - 2.0 * alpha * beta * c;
    if !(left >= -REACH_TOL * REACH_TOL) {
        return Vec::new();
    }
    let gamma = (left.max(0.0) / n2).sqrt();
    let base = w1 * alpha + w2 * beta;
    if gamma * n.len() <= 1e-12 { vec![base] } else { vec![base + n * gamma, base - n * gamma] }
}

/// How the orientation equation of a machine is put: which rotary is `φ1` and `φ2` and with which sign.
enum Setup {
    None,
    One { i: usize, p: Vec3, q: Vec3 },
    Two { i1: usize, s1: f64, i2: usize, s2: f64, p: Vec3, q: Vec3 },
}

fn setup(m: &Machine, a: Vec3) -> Setup {
    let heads: Vec<usize> = m.rotaries.iter().enumerate().filter(|(_, r)| r.carrier == Carrier::Head).map(|(i, _)| i).collect();
    let tables: Vec<usize> = m.rotaries.iter().enumerate().filter(|(_, r)| r.carrier == Carrier::Table).map(|(i, _)| i).collect();
    let z = Vec3::Z;
    match (heads.as_slice(), tables.as_slice()) {
        ([i], []) => Setup::One { i: *i, p: z, q: a },
        ([], [i]) => Setup::One { i: *i, p: a, q: z },
        ([i1, i2], []) => Setup::Two { i1: *i1, s1: 1.0, i2: *i2, s2: 1.0, p: z, q: a },
        ([], [i1, i2]) => Setup::Two { i1: *i1, s1: 1.0, i2: *i2, s2: 1.0, p: a, q: z },
        ([h], [t]) => Setup::Two { i1: *t, s1: -1.0, i2: *h, s2: 1.0, p: z, q: a },
        _ => Setup::None,
    }
}

fn describe(a: Vec3) -> String {
    format!("({:.4}, {:.4}, {:.4})", a.x, a.y, a.z)
}

/// Every set of rotary angles (radians, each in `-π..=π`, joint order) that turns the tool to
/// the work-coordinate direction `axis`, limits not applied. An angle that does not matter is
/// taken from `reference`. Errors when the machine cannot point the tool that way.
fn candidates(m: &Machine, a: Vec3, reference: &[f64]) -> Result<Vec<Vec<f64>>> {
    let unreachable = || invalid(format!("machine `{}` cannot point the tool along {}", m.name, describe(a)));
    let rf = |i: usize| angle(reference, i);
    match setup(m, a) {
        Setup::None => {
            if a.cross(Vec3::Z).len().atan2(a.z) <= REACH_TOL {
                Ok(vec![Vec::new()])
            } else {
                unreachable()
            }
        }
        Setup::One { i, p, q } => {
            let w = m.rotaries.get(i).map_or(Vec3::Z, RotaryAxis::dir);
            if (w.dot(p) - w.dot(q)).abs() > REACH_TOL {
                return unreachable();
            }
            let sign = m.rotaries.get(i).map_or(1.0, RotaryAxis::sign);
            Ok(vec![vec![sign * turn(p, q, w).unwrap_or(sign * rf(i))]])
        }
        Setup::Two { i1, s1, i2, s2, p, q } => {
            // A reversed axis turns its written angle round: fold that into the sign.
            let s1 = s1 * m.rotaries.get(i1).map_or(1.0, RotaryAxis::sign);
            let s2 = s2 * m.rotaries.get(i2).map_or(1.0, RotaryAxis::sign);
            let w1 = m.rotaries.get(i1).map_or(Vec3::Z, RotaryAxis::dir);
            let w2 = m.rotaries.get(i2).map_or(Vec3::X, RotaryAxis::dir);
            let mut out: Vec<Vec<f64>> = Vec::new();
            for z in cones_meet(w1, w2, p, q) {
                let phi2 = turn(p, z, w2).unwrap_or(s2 * rf(i2));
                let pz = rotate(p, w2, phi2);
                let phi1 = turn(pz, q, w1).unwrap_or(s1 * rf(i1));
                if rotate(pz, w1, phi1).dist(q) > 1e-5 {
                    continue;
                }
                let mut sol = vec![0.0; m.rotaries.len()];
                if let Some(v) = sol.get_mut(i1) {
                    *v = s1 * phi1;
                }
                if let Some(v) = sol.get_mut(i2) {
                    *v = s2 * phi2;
                }
                let wrap = |x: f64| x - TAU * (x / TAU).round();
                let sol: Vec<f64> = sol.into_iter().map(wrap).collect();
                let same = |o: &Vec<f64>| o.iter().zip(&sol).all(|(x, y)| (x - y).abs() <= 1e-12);
                if !out.iter().any(same) {
                    out.push(sol);
                }
            }
            if out.is_empty() { unreachable() } else { Ok(out) }
        }
    }
}

/// The angle equal to `theta` (modulo a turn) nearest `reference` that is within `limits`.
pub(crate) fn nearest_turn(theta: f64, reference: f64, limits: Option<(f64, f64)>) -> Option<f64> {
    let k = ((reference - theta) / TAU).round();
    match limits {
        None => Some(theta + TAU * k),
        Some((lo, hi)) => {
            let kmin = ((lo - 1e-9 - theta) / TAU).ceil();
            let kmax = ((hi + 1e-9 - theta) / TAU).floor();
            if kmin > kmax { None } else { Some(theta + TAU * k.clamp(kmin, kmax)) }
        }
    }
}

/// Every rotary solution for the work-coordinate tool direction `axis`, each angle in `-π..=π`
/// (joint order), limits not applied: none, one or two of them. An angle that does not matter
/// (a singular direction) is zero.
pub fn solutions(machine: &Machine, axis: Vec3) -> Result<Vec<Vec<f64>>> {
    machine.validate()?;
    let a = unit(axis, "the tool axis")?;
    candidates(machine, a, &[])
}

/// The rotary angles (radians, joint order) that point the tool along the work-coordinate
/// direction `axis`: of the solutions within the limits, the one nearest `previous` (or zero),
/// each continuous axis taking the turn nearest its previous angle.
pub fn orient(machine: &Machine, axis: Vec3, previous: Option<&Joints>) -> Result<Vec<f64>> {
    machine.validate()?;
    let a = unit(axis, "the tool axis")?;
    let reference: Vec<f64> = match previous {
        Some(p) => {
            if p.rotary.len() != machine.rotaries.len() {
                return invalid(format!("the previous position has {} rotary angles; the machine has {}", p.rotary.len(), machine.rotaries.len()));
            }
            if p.rotary.iter().any(|v| !(v.abs() <= MAX_ANGLE)) {
                return invalid("the previous rotary angles must be finite");
            }
            p.rotary.clone()
        }
        None => vec![0.0; machine.rotaries.len()],
    };
    let mut best: Option<(f64, Vec<f64>)> = None;
    let mut beyond: Option<(usize, f64)> = None;
    for sol in candidates(machine, a, &reference)? {
        let mut cost = 0.0;
        let mut chosen = Vec::with_capacity(sol.len());
        let mut ok = true;
        for (i, (r, &theta)) in machine.rotaries.iter().zip(&sol).enumerate() {
            let rf = angle(&reference, i);
            match nearest_turn(theta, rf, r.limits) {
                Some(t) => {
                    cost += (t - rf).abs();
                    chosen.push(t);
                }
                None => {
                    beyond.get_or_insert((i, theta));
                    ok = false;
                    break;
                }
            }
        }
        if ok && best.as_ref().is_none_or(|(c, _)| cost < *c - 1e-12) {
            best = Some((cost, chosen));
        }
    }
    match (best, beyond) {
        (Some((_, angles)), _) => Ok(angles),
        (None, Some((i, theta))) => {
            let r = machine.rotaries.get(i);
            let letter = r.map_or('?', |r| r.letter.char());
            let (lo, hi) = r.and_then(|r| r.limits).unwrap_or((0.0, 0.0));
            Err(CamError::Unsafe(format!(
                "the tool axis {} needs {letter} at {:.3}°, beyond its limits {:.3}° to {:.3}°",
                describe(a),
                theta.to_degrees(),
                lo.to_degrees(),
                hi.to_degrees()
            )))
        }
        (None, None) => invalid(format!("machine `{}` cannot point the tool along {}", machine.name, describe(a))),
    }
}

/// The machine position that puts the tool at `pose` (work coordinates): the rotaries from
/// [`orient`], and the linear position `X = T(θt)·(o + tip) − H(θh)·0`. Linear limits are not
/// checked here: see [`check_limits`].
pub fn inverse(machine: &Machine, pose: &Pose, previous: Option<&Joints>) -> Result<Joints> {
    if !pose.tip.is_finite() || !(pose.tip.len() <= MAX_COORD) {
        return invalid("the tool tip must be a finite point");
    }
    let rotary = orient(machine, pose.axis, previous)?;
    let linear = table_point(machine, &rotary, machine.work_origin + pose.tip) - head_point(machine, &rotary, Vec3::ZERO);
    Ok(Joints { linear, rotary })
}

/// Whether `joints` is within the machine's travel; the error names the axis.
pub fn check_limits(machine: &Machine, joints: &Joints) -> Result<()> {
    for (r, &theta) in machine.rotaries.iter().zip(&joints.rotary) {
        if let Some((lo, hi)) = r.limits
            && !(theta >= lo - 1e-9 && theta <= hi + 1e-9)
        {
            return Err(CamError::Unsafe(format!(
                "{} at {:.3}° is beyond its limits {:.3}° to {:.3}°",
                r.letter.char(),
                theta.to_degrees(),
                lo.to_degrees(),
                hi.to_degrees()
            )));
        }
    }
    if let Some((lo, hi)) = machine.linear_limits {
        let l = joints.linear;
        for (name, v, a, b) in [('X', l.x, lo.x, hi.x), ('Y', l.y, lo.y, hi.y), ('Z', l.z, lo.z, hi.z)] {
            if !(v >= a - 1e-9 && v <= b + 1e-9) {
                return Err(CamError::Unsafe(format!("{name} at {v:.4} is beyond its travel {a:.4} to {b:.4}")));
            }
        }
    }
    Ok(())
}

/// How [`solve`] turns a toolpath into machine positions.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SolveOptions {
    /// Largest distance (mm) of the tip from its planned line along each written piece, the
    /// machine's axes moving evenly between the piece's ends.
    pub tolerance: f64,
    /// Largest angle (radians) of the tool axis from its planned turn there.
    pub angle_tolerance: f64,
    /// Whether the controller keeps the tip on its line itself (tool centre point control), so
    /// feed moves are written whole.
    pub tcp: bool,
    /// Largest turn (radians) of a rotary between two points of a feed move whose tool axes are
    /// at most a degree apart. More means the solution flips (the tool passes close to a
    /// direction where an angle stops mattering), and the move is refused.
    pub flip_threshold: f64,
}

impl Default for SolveOptions {
    fn default() -> SolveOptions {
        SolveOptions { tolerance: 0.005, angle_tolerance: 0.1f64.to_radians(), tcp: false, flip_threshold: 45f64.to_radians() }
    }
}

impl SolveOptions {
    pub fn validate(&self) -> Result<()> {
        if !(self.tolerance > 0.0) || !(self.tolerance <= 100.0) {
            return invalid("the kinematics tolerance must be above 0 mm and at most 100 mm");
        }
        if !(self.angle_tolerance > 0.0) || !(self.angle_tolerance <= PI) {
            return invalid("the kinematics angle tolerance must be above 0 and at most π");
        }
        if !(self.flip_threshold > 0.0) || !(self.flip_threshold <= TAU) {
            return invalid("the flip threshold must be above 0 and at most a full turn");
        }
        Ok(())
    }
}

/// One move of the machine's axes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JointMove {
    /// Positioning at rapid rate. `rotary_change` is the largest turn of a rotary on the way
    /// (radians; zero for the first).
    Rapid {
        source: usize,
        to: Joints,
        pose: Pose,
        rotary_change: f64,
    },
    /// A cut, the axes moving evenly from where they were to `to` in `minutes`: the planned tip
    /// travel at `feed` (mm/min), or where the tip stays put, the axis's turn in degrees taken
    /// as millimetres, as controllers time a turn on the spot. `pose` is where it ends as
    /// planned.
    Feed {
        source: usize,
        to: Joints,
        pose: Pose,
        feed: f64,
        minutes: f64,
    },
    Dwell {
        source: usize,
        seconds: f64,
    },
}

impl JointMove {
    /// The index of the toolpath move this one comes from.
    pub fn source(&self) -> usize {
        match *self {
            JointMove::Rapid { source, .. } | JointMove::Feed { source, .. } | JointMove::Dwell { source, .. } => source,
        }
    }

    pub fn joints(&self) -> Option<&Joints> {
        match self {
            JointMove::Rapid { to, .. } | JointMove::Feed { to, .. } => Some(to),
            JointMove::Dwell { .. } => None,
        }
    }
}

/// The planned pose a fraction `t` along the move from `a` to `b`.
pub(crate) fn planned(a: &Pose, b: &Pose, t: f64) -> Pose {
    Pose { tip: a.tip.lerp(b.tip, t), axis: slerp(a.axis, b.axis, t) }
}

/// The angle between two directions.
pub(crate) fn between(a: Vec3, b: Vec3) -> f64 {
    a.cross(b).len().atan2(a.dot(b))
}

/// The angle (radians) from the unit `v` to the shorter great-circle arc from `a` to `b`.
pub(crate) fn arc_distance(v: Vec3, a: Vec3, b: Vec3) -> f64 {
    let ends = between(v, a).min(between(v, b));
    let Some(n) = a.cross(b).normalized().filter(|_| a.cross(b).len() > 1e-12) else { return ends };
    let p = v - n * n.dot(v);
    if p.len() <= 1e-12 {
        return ends;
    }
    if a.cross(p).dot(n) >= 0.0 && p.cross(b).dot(n) >= 0.0 { between(v, p) } else { ends }
}

/// The length (mm) a feed piece from `a` to `b` is timed by at its feed. Where the tip stays
/// put it is the axis's turn in degrees taken as millimetres, as controllers time a turn on the
/// spot; as the tip starts to move that part fades out over [`TURN_FADE`] of travel, so the time
/// never jumps. It is never less than the tip's travel, nor than the travel of a point
/// [`TURN_LEVER`] from the tip as the axis turns, so a turn with the tip almost still is never
/// run at the machine's top rate.
pub(crate) fn piece_length(a: &Pose, b: &Pose) -> f64 {
    let len = a.tip.dist(b.tip);
    let turn = a.axis_angle(b);
    let on_the_spot = turn.to_degrees() * (1.0 - len / TURN_FADE).max(0.0);
    len.max(turn * TURN_LEVER).max(on_the_spot)
}

/// [`solve_from`] with no position before the path.
pub fn solve(machine: &Machine, path: &Toolpath5, opts: &SolveOptions) -> Result<Vec<JointMove>> {
    solve_from(machine, path, opts, None)
}

/// The machine moves that follow `path` (work coordinates), continuing from `previous`.
///
/// Every pose is turned into axis positions with [`inverse`], each nearest the one before. A
/// feed move is checked for flips at points at most a degree of tool axis apart and refused
/// (`Unsafe`) when a rotary would swing more than [`SolveOptions::flip_threshold`] between two
/// of them. Without TCP it is then halved until the machine's axes, moving evenly between the
/// ends of each piece, keep the tip within the tolerance of its line and the axis within the
/// angle tolerance of its turn all along the piece (`TooLarge` past [`MAX_PIECES`]). Every
/// position is checked against the machine's limits (`Unsafe`, naming the move and axis), and
/// every feed must be 0.001 to 1,000,000 mm/min.
///
/// A rotary without limits is followed within a turn of zero and its whole turns are kept
/// apart, so a long helix adds up its angles without end (they are written unwound) and never
/// reaches angles the solver refuses.
pub fn solve_from(machine: &Machine, path: &Toolpath5, opts: &SolveOptions, previous: Option<&Joints>) -> Result<Vec<JointMove>> {
    machine.validate()?;
    opts.validate()?;
    if path.moves.len() > MAX_MOVES {
        return Err(CamError::TooLarge(format!("the toolpath has more than {MAX_MOVES} moves")));
    }
    let at = |i: usize| {
        move |e: CamError| match e {
            CamError::Invalid(s) => CamError::Invalid(format!("move {i}: {s}")),
            CamError::Unsafe(s) => CamError::Unsafe(format!("move {i}: {s}")),
            CamError::TooLarge(s) => CamError::TooLarge(format!("move {i}: {s}")),
            other => other,
        }
    };
    let mut out = Vec::new();
    // `joints` is where the axes are with the whole turns of the rotaries without limits taken
    // out (`turns`, a count per joint); `lift` puts them back.
    let mut turns = vec![0.0f64; machine.rotaries.len()];
    let rebase = |j: &mut Joints, turns: &mut [f64]| {
        for ((r, a), k) in machine.rotaries.iter().zip(j.rotary.iter_mut()).zip(turns.iter_mut()) {
            let whole = (*a / TAU).round();
            if r.limits.is_none() && whole != 0.0 && whole.is_finite() {
                *a -= whole * TAU;
                *k += whole;
            }
        }
    };
    let lift = |j: &Joints, turns: &[f64]| Joints { linear: j.linear, rotary: j.rotary.iter().zip(turns).map(|(a, k)| a + k * TAU).collect() };
    let mut joints: Option<Joints> = previous.cloned();
    if let Some(j) = joints.as_mut() {
        if j.rotary.len() != machine.rotaries.len() {
            return invalid(format!("the previous position has {} rotary angles; the machine has {}", j.rotary.len(), machine.rotaries.len()));
        }
        if j.rotary.iter().any(|a| !a.is_finite()) {
            return invalid("the previous rotary angles must be finite");
        }
        rebase(j, &mut turns);
    }
    let mut pose: Option<Pose> = None;
    for (i, m) in path.moves.iter().enumerate() {
        match *m {
            Move5::Rapid { to } => {
                if !to.is_valid() {
                    return Err(CamError::Unsafe(format!("move {i}: a pose is not finite or its tool axis is not a unit direction")));
                }
                let mut j = inverse(machine, &to, joints.as_ref()).map_err(at(i))?;
                check_limits(machine, &j).map_err(at(i))?;
                let rotary_change = joints.as_ref().map_or(0.0, |p| p.rotary_change(&j));
                out.push(JointMove::Rapid { source: i, to: lift(&j, &turns), pose: to, rotary_change });
                rebase(&mut j, &mut turns);
                joints = Some(j);
                pose = Some(to);
            }
            Move5::Feed { to, feed } => {
                let (Some(from), Some(j0)) = (pose, joints.clone()) else {
                    return Err(CamError::Unsafe(format!("move {i}: a cut before the first positioning move")));
                };
                if !to.is_valid() {
                    return Err(CamError::Unsafe(format!("move {i}: a pose is not finite or its tool axis is not a unit direction")));
                }
                if !feed_in_range(feed) {
                    return Err(CamError::Unsafe(format!("move {i}: the feed must be {MIN_FEED} to {MAX_FEED} mm/min")));
                }
                let j1 = follow(machine, &from, &to, &j0, opts).map_err(at(i))?;
                let pieces = if opts.tcp { vec![(1.0, j1)] } else { linearize(machine, &from, &to, &j0, &j1, opts).map_err(at(i))? };
                let mut ta = 0.0;
                for (tb, j) in pieces {
                    check_limits(machine, &j).map_err(at(i))?;
                    let (pa, pb) = (planned(&from, &to, ta), planned(&from, &to, tb));
                    out.push(JointMove::Feed { source: i, to: lift(&j, &turns), pose: pb, feed, minutes: piece_length(&pa, &pb) / feed });
                    joints = Some(j);
                    ta = tb;
                }
                if let Some(j) = joints.as_mut() {
                    rebase(j, &mut turns);
                }
                pose = Some(to);
            }
            Move5::Dwell { seconds } => {
                if !(seconds >= 0.0) || !(seconds <= 1.0e6) {
                    return Err(CamError::Unsafe(format!("move {i}: a dwell must be 0 seconds or longer")));
                }
                out.push(JointMove::Dwell { source: i, seconds });
            }
        }
        if out.len() > MAX_MOVES {
            return Err(CamError::TooLarge(format!("the machine moves would be more than {MAX_MOVES}")));
        }
    }
    Ok(out)
}

/// The axis positions at the end of the feed move `from`→`to`, followed from `j0` at points at
/// most a degree of tool axis apart; refused when a rotary swings past the flip threshold
/// between two of them.
fn follow(machine: &Machine, from: &Pose, to: &Pose, j0: &Joints, opts: &SolveOptions) -> Result<Joints> {
    let n = ((from.axis_angle(to) / FLIP_SAMPLE).ceil() as usize).clamp(1, MAX_FLIP_SAMPLES);
    let mut last = j0.clone();
    for k in 1..=n {
        let p = planned(from, to, k as f64 / n as f64);
        let j = inverse(machine, &p, Some(&last))?;
        for (r, (a, b)) in machine.rotaries.iter().zip(last.rotary.iter().zip(&j.rotary)) {
            if (b - a).abs() > opts.flip_threshold {
                // A swing of about a whole turn is the axis wrapping round inside limits that
                // leave no room to carry on: say so rather than call it a flip.
                let on = b + TAU * ((a - b) / TAU).round();
                if let Some((lo, hi)) = r.limits
                    && (on - a).abs() <= opts.flip_threshold
                    && (on < lo - 1e-9 || on > hi + 1e-9)
                {
                    let narrow = if hi - lo < TAU - 1e-9 {
                        format!(
                            " ({:.3}° between them, less than a whole turn, so it cannot be turned back a turn to carry on)",
                            (hi - lo).to_degrees()
                        )
                    } else {
                        String::new()
                    };
                    return Err(CamError::Unsafe(format!(
                        "the cut needs {} to turn on to {:.3}°, beyond its limits {:.3}° to {:.3}°{narrow}; widen the limits or keep the cut within them",
                        r.letter.char(),
                        on.to_degrees(),
                        lo.to_degrees(),
                        hi.to_degrees()
                    )));
                }
                return Err(CamError::Unsafe(format!(
                    "{} would swing {:.1}° while the tool axis turns at most 1° (a solution flip near a direction where an angle stops mattering); the cut is refused rather than swinging the tool through the part",
                    r.letter.char(),
                    (b - a).abs().to_degrees()
                )));
            }
        }
        last = j;
    }
    Ok(last)
}

/// Whether the machine's axes, moving evenly from `ja` to `jb`, keep the tip within tolerance
/// of the planned line `pa`→`pb` and the axis within the angle tolerance of its turn. The tip
/// can swing out and back across the line on the way (an S under joint interpolation, near the
/// line at the middle), so the piece is checked at its middle and quarters and at points at
/// most [`PIECE_SAMPLE`] of rotary turn apart, not at its middle alone.
fn piece_within(machine: &Machine, ja: &Joints, jb: &Joints, pa: &Pose, pb: &Pose, opts: &SolveOptions) -> bool {
    let half = ((ja.rotary_change(jb) / PIECE_SAMPLE).ceil() as usize).clamp(2, MAX_PIECE_SAMPLES);
    let n = 2 * half;
    (1..n).all(|k| {
        let got = forward(machine, &ja.lerp(jb, k as f64 / n as f64));
        got.tip.dist_to_segment(pa.tip, pb.tip) <= opts.tolerance && arc_distance(got.axis, pa.axis, pb.axis) <= opts.angle_tolerance
    })
}

/// The pieces `(t, joints at t)` of the feed move `from`→`to` (from `j0` at `t = 0` to `j1` at
/// `t = 1`) the machine can move evenly through within tolerance (see [`piece_within`]).
fn linearize(machine: &Machine, from: &Pose, to: &Pose, j0: &Joints, j1: &Joints, opts: &SolveOptions) -> Result<Vec<(f64, Joints)>> {
    let mut out: Vec<(f64, Joints)> = Vec::new();
    let mut stack = vec![(0.0, j0.clone(), 1.0, j1.clone(), 0u32)];
    while let Some((ta, ja, tb, jb, depth)) = stack.pop() {
        let (pa, pb) = (planned(from, to, ta), planned(from, to, tb));
        if piece_within(machine, &ja, &jb, &pa, &pb, opts) {
            out.push((tb, jb));
            continue;
        }
        if depth >= MAX_DEPTH || out.len() + stack.len() >= MAX_PIECES {
            return Err(CamError::TooLarge(format!("more than {MAX_PIECES} pieces are needed to follow the cut within tolerance")));
        }
        let tm = 0.5 * (ta + tb);
        let jm = inverse(machine, &planned(from, to, tm), Some(&ja))?;
        stack.push((tm, jm.clone(), tb, jb, depth + 1));
        stack.push((ta, ja, tm, jm, depth + 1));
    }
    Ok(out)
}
