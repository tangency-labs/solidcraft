//! Drivetrain analysis over a planned set's assembly ([`Assembly`]): the ratio between any two
//! members and the path of meshes and shared shafts that ties them, the speed, direction and
//! torque at every member from a driver's speed and torque (with an efficiency per stage), the
//! per-stage and overall ratios, and per-mesh checks (contact ratio, hunting tooth, ratio error).
//!
//! Members are [`Assembly::members`] indices. For a set read from a sketch or a selection, gear
//! `k` of the drawing is member [`member_of_gear`]`(plan, k)` (a compound cluster's gears are
//! one member). Each member turns about its own [`crate::plan::AssemblyMember::axis`]; ratios
//! are signed: positive when `b` turns the same way round its axis as `a` round its own, as
//! [`Assembly::ratio`] gives them. A rack slides along its axis (mm), so a ratio from a turning
//! member to a rack is mm per radian and [`RatioReport::mm_per_rev`] gives it per turn.
//!
//! What a link doesn't say (helical or spur, bevel or spiral bevel, hypoid or crossed helical,
//! chain or ring gear, a worm's lead angle) comes from [`Efficiencies`]: its styles default to
//! what the shafts' geometry says, and [`Efficiencies::for_spec`] sets them from a set's spec.
//!
//! Plain data, no engine types: the engine and the dialog show these as they are.

use std::f64::consts::TAU;

use crate::axes::dot;
use crate::plan::{Assembly, AssemblyLink, Kind, Plan, Spec};

/// What kind of mesh a link is (or, for [`StepKind::Shaft`], a shared shaft).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshKind {
    Spur,
    Helical,
    Herringbone,
    /// A gear inside a ring gear (both turn the same way).
    Internal,
    /// A pinion and a rack (the rack slides).
    Rack,
    Bevel,
    SpiralBevel,
    Hypoid,
    CrossedHelical,
    Worm,
    /// A chain or belt between two wheels (both turn the same way).
    Wrap,
}

impl MeshKind {
    /// For people.
    pub fn name(self) -> &'static str {
        match self {
            MeshKind::Spur => "spur",
            MeshKind::Helical => "helical",
            MeshKind::Herringbone => "herringbone",
            MeshKind::Internal => "internal",
            MeshKind::Rack => "rack and pinion",
            MeshKind::Bevel => "bevel",
            MeshKind::SpiralBevel => "spiral bevel",
            MeshKind::Hypoid => "hypoid",
            MeshKind::CrossedHelical => "crossed helical",
            MeshKind::Worm => "worm",
            MeshKind::Wrap => "chain or belt",
        }
    }
}

/// One step of a path between two members.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepKind {
    Mesh(MeshKind),
    /// Joined as one: a shared shaft or a rigid link (ratio 1, no loss).
    Shaft,
}

/// Which way a member turns compared with another, each about its own axis (for a rack: along
/// its axis).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Same,
    Opposite,
}

impl Direction {
    fn of(r: f64) -> Direction {
        if r >= 0.0 { Direction::Same } else { Direction::Opposite }
    }
}

/// A step from member `from` to member `to` across assembly link `link`.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    /// The link (an index into [`Assembly::links`]).
    pub link: usize,
    pub from: usize,
    pub to: usize,
    pub kind: StepKind,
    /// Teeth of the `from` and `to` sides when the link holds tooth counts (a worm: its starts).
    pub teeth: Option<(u32, u32)>,
    /// `to`'s motion per unit of `from`'s (signed; rad/rad, mm/rad onto a rack, rad/mm off one).
    pub ratio: f64,
    /// The step's efficiency in this direction ([`Efficiencies`]); 0 when it can't be driven
    /// this way (a self-locking worm driven from its wheel).
    pub efficiency: f64,
    /// A worm step that can't be driven from its wheel (self-locking), whichever way it is
    /// walked.
    pub self_locking: bool,
}

/// The ratio between two members ([`ratio_between`]).
#[derive(Clone, Debug, PartialEq)]
pub struct RatioReport {
    pub from: usize,
    pub to: usize,
    /// `to`'s motion per unit of `from`'s, signed (rad/rad; mm/rad when `to` is a rack; rad/mm
    /// when `from` is one): ω_to / ω_from.
    pub ratio: f64,
    pub direction: Direction,
    /// |ω_from / ω_to|: the reduction (> 1 slows down). Infinite when `to` doesn't move.
    pub reduction: f64,
    /// `to` slides (a rack) and `from` turns: its travel per turn of `from` (mm, signed).
    pub mm_per_rev: Option<f64>,
    /// `from` slides and `to` turns: `to`'s turns per mm of `from` (signed).
    pub rev_per_mm: Option<f64>,
    /// The steps from `from` to `to`, in order (empty when they are the same member).
    pub path: Vec<Step>,
    /// The product of the steps' efficiencies along the path.
    pub efficiency: f64,
    /// For people: carriers held, self-locking steps.
    pub notes: Vec<String>,
}

/// The state of one member when the drivetrain is driven ([`drive_table`]).
#[derive(Clone, Debug, PartialEq)]
pub struct MemberState {
    pub member: usize,
    /// Its body (an index into [`Plan::bodies`]).
    pub body: usize,
    /// Joined to the driver by links (otherwise everything below is 0 / `None`).
    pub reached: bool,
    /// It slides (a rack): `linear_speed` and `force` hold its motion.
    pub linear: bool,
    /// Turns per minute about its own axis (signed; 0 for a rack).
    pub rpm: f64,
    /// A rack's speed along its axis (mm/s, signed).
    pub linear_speed: Option<f64>,
    /// Compared with the driver's turn, each about its own axis.
    pub direction: Option<Direction>,
    /// Torque available at it (N·m), signed as its turn: the driver's power times the
    /// efficiency, over its speed. 0 for a rack.
    pub torque: f64,
    /// A rack's force along its axis (N), signed as its travel.
    pub force: Option<f64>,
    /// Power reaching it (W).
    pub power: f64,
    /// Efficiency from the driver to it (the product along its path).
    pub efficiency: f64,
    /// Motion stopped: a self-locking worm driven from its wheel on its path, a held member, or
    /// a loop whose ratios don't agree.
    pub locked: bool,
    /// The steps from the driver to it.
    pub path: Vec<Step>,
    pub note: Option<String>,
}

/// The overall ratio from a driver to an output, its stages, and its error against a target
/// ([`overall_ratio`]).
#[derive(Clone, Debug, PartialEq)]
pub struct OverallRatio {
    pub report: RatioReport,
    /// The meshes along the path (shared shafts left out), each with its own ratio.
    pub stages: Vec<Step>,
    /// The target asked for: a reduction |ω_in / ω_out| (for a rack output: mm per turn).
    pub target: Option<f64>,
    /// (got − target) / target.
    pub error: Option<f64>,
    pub note: Option<String>,
}

/// One mesh's checks ([`mesh_checks`], [`plan_mesh_checks`]).
#[derive(Clone, Debug, PartialEq)]
pub struct MeshCheck {
    /// Its assembly link, when the assembly holds it (a mesh closing a loop isn't a link).
    pub link: Option<usize>,
    /// The train's gears (indices into `Spec::train`'s gears / [`Plan::members`]), for a set
    /// read from a sketch or a selection.
    pub gears: Option<(usize, usize)>,
    /// The members it joins.
    pub members: (usize, usize),
    pub kind: MeshKind,
    /// Tooth counts (a worm: starts, wheel teeth).
    pub teeth: Option<(u32, u32)>,
    /// Tooth ratio zb / za (≥ 0).
    pub ratio: f64,
    /// Transverse contact ratio, and total with the overlap, where the plan works them out.
    pub contact_ratio: Option<f64>,
    pub total_contact_ratio: Option<f64>,
    /// The tooth counts' greatest common factor; 1 is a hunting-tooth pair (every tooth meets
    /// every tooth of its mate).
    pub common_factor: Option<u32>,
    pub hunting: Option<bool>,
    pub notes: Vec<String>,
}

/// Efficiencies per stage type, the styles that say what kind each link is, and per-link
/// overrides. Worm efficiency comes from its lead angle and friction ([`worm_efficiency`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Efficiencies {
    pub spur: f64,
    pub helical: f64,
    pub herringbone: f64,
    pub internal: f64,
    pub rack: f64,
    pub bevel: f64,
    pub spiral_bevel: f64,
    pub hypoid: f64,
    pub crossed_helical: f64,
    pub wrap: f64,
    /// Worm: friction coefficient μ, normal pressure angle (degrees), and the lead angle
    /// (degrees; `None`: from the starts and `worm_diameter_quotient`, tan λ = starts / q).
    pub worm_friction: f64,
    pub worm_pressure_angle: f64,
    pub worm_lead_angle: Option<f64>,
    pub worm_diameter_quotient: f64,
    /// The kind of an outside mesh on parallel shafts (spur, helical or herringbone).
    pub parallel: MeshKind,
    /// The kind of a mesh on parallel shafts whose gears turn the same way (internal, or a
    /// chain or belt).
    pub parallel_same: MeshKind,
    /// The kind of a mesh on shafts that meet (bevel or spiral bevel).
    pub meeting: MeshKind,
    /// The kind of a mesh on shafts that cross without meeting (crossed helical, hypoid, or a
    /// worm: then the side with fewer teeth is the worm).
    pub skew: MeshKind,
    /// Per assembly link (index into [`Assembly::links`]): its kind, overriding the styles.
    pub kinds: Vec<(usize, MeshKind)>,
    /// Per assembly link: its efficiency, overriding everything else (both ways).
    pub per_link: Vec<(usize, f64)>,
    /// Per assembly link that is a worm: its lead angle (degrees), overriding
    /// `worm_lead_angle` and the diameter quotient (a drivetrain with several worms).
    pub worm_leads: Vec<(usize, f64)>,
}

impl Default for Efficiencies {
    fn default() -> Self {
        Self {
            spur: 0.99,
            helical: 0.985,
            herringbone: 0.985,
            internal: 0.99,
            rack: 0.98,
            bevel: 0.97,
            spiral_bevel: 0.97,
            hypoid: 0.95,
            crossed_helical: 0.90,
            wrap: 0.97,
            worm_friction: 0.05,
            worm_pressure_angle: 20.0,
            worm_lead_angle: None,
            worm_diameter_quotient: 10.0,
            parallel: MeshKind::Spur,
            parallel_same: MeshKind::Internal,
            meeting: MeshKind::Bevel,
            skew: MeshKind::CrossedHelical,
            kinds: Vec::new(),
            per_link: Vec::new(),
            worm_leads: Vec::new(),
        }
    }
}

impl Efficiencies {
    /// The defaults with the styles a set's spec says: its tooth style (helical when it has a
    /// helix), spiral bevel, hypoid, worm (with its diameter quotient), chain or belt.
    pub fn for_spec(s: &Spec) -> Efficiencies {
        let parallel = match s.kind {
            Kind::Herringbone => MeshKind::Herringbone,
            Kind::Helical => MeshKind::Helical,
            _ if s.helix.abs() > 1e-9 && matches!(s.kind, Kind::Spur | Kind::Pair | Kind::Planetary | Kind::Internal | Kind::Rack) => {
                MeshKind::Helical
            }
            _ => MeshKind::Spur,
        };
        let mut e = Efficiencies { parallel, ..Efficiencies::default() };
        if let Some(t) = &s.train
            && t.gears.iter().any(|g| g.herringbone)
        {
            e.parallel = MeshKind::Herringbone;
        } else if let Some(t) = &s.train
            && t.gears.iter().any(|g| g.helix.abs() > 1e-9)
        {
            e.parallel = MeshKind::Helical;
        }
        match s.kind {
            Kind::SpiralBevel => e.meeting = MeshKind::SpiralBevel,
            Kind::Hypoid => {
                // A hypoid with no offset is a spiral bevel pair.
                e.meeting = MeshKind::SpiralBevel;
                e.skew = MeshKind::Hypoid;
            }
            Kind::Worm => {
                e.skew = MeshKind::Worm;
                if s.worm_diameter > 0.0 && s.module > 0.0 {
                    e.worm_diameter_quotient = s.worm_diameter / s.module;
                }
            }
            Kind::Sprocket | Kind::Pulley => e.parallel_same = MeshKind::Wrap,
            _ => {}
        }
        e
    }

    /// The efficiency of a kind of mesh (a worm's driven from its worm).
    pub fn of(&self, kind: MeshKind, starts: u32) -> f64 {
        match kind {
            MeshKind::Spur => self.spur,
            MeshKind::Helical => self.helical,
            MeshKind::Herringbone => self.herringbone,
            MeshKind::Internal => self.internal,
            MeshKind::Rack => self.rack,
            MeshKind::Bevel => self.bevel,
            MeshKind::SpiralBevel => self.spiral_bevel,
            MeshKind::Hypoid => self.hypoid,
            MeshKind::CrossedHelical => self.crossed_helical,
            MeshKind::Wrap => self.wrap,
            MeshKind::Worm => self.worm(starts).forward,
        }
    }

    /// A worm of `starts` threads: its lead angle (the given one, or tan λ = starts / q) and
    /// efficiencies.
    pub fn worm(&self, starts: u32) -> WormEfficiency {
        self.worm_of(None, starts)
    }

    /// [`Efficiencies::worm`] for assembly link `link`: its own lead angle where
    /// [`Efficiencies::worm_leads`] gives one.
    pub fn worm_of(&self, link: Option<usize>, starts: u32) -> WormEfficiency {
        if let Some(lead) = link.and_then(|k| self.worm_leads.iter().rev().find(|(i, _)| *i == k)).map(|(_, l)| *l) {
            return worm_efficiency(lead, self.worm_friction, self.worm_pressure_angle);
        }
        let lead = self.worm_lead_angle.unwrap_or_else(|| (f64::from(starts.max(1)) / self.worm_diameter_quotient.max(1e-9)).atan().to_degrees());
        worm_efficiency(lead, self.worm_friction, self.worm_pressure_angle)
    }
}

/// A worm's efficiency driven from the worm and from the wheel ([`worm_efficiency`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WormEfficiency {
    /// Lead angle (degrees).
    pub lead_angle: f64,
    /// The worm driving the wheel.
    pub forward: f64,
    /// The wheel driving the worm (0 when self-locking).
    pub back: f64,
    /// The wheel can't drive the worm: tan λ ≤ μ / cos αn.
    pub self_locking: bool,
}

/// A worm's efficiency from its lead angle λ (degrees), friction μ and normal pressure angle αn
/// (degrees): driven from the worm η = (cos αn − μ tan λ) / (cos αn + μ cot λ); from the wheel
/// η' = (cos αn − μ cot λ) / (cos αn + μ tan λ), self-locking when that is ≤ 0.
pub fn worm_efficiency(lead_angle: f64, friction: f64, pressure_angle: f64) -> WormEfficiency {
    let (t, c, mu) = (lead_angle.to_radians().tan().max(1e-12), pressure_angle.to_radians().cos(), friction.max(0.0));
    let forward = ((c - mu * t) / (c + mu / t)).clamp(0.0, 1.0);
    let back_raw = (c - mu / t) / (c + mu * t);
    WormEfficiency { lead_angle, forward, back: back_raw.clamp(0.0, 1.0), self_locking: back_raw <= 0.0 }
}

/// The greatest common factor of two tooth counts.
pub fn common_factor(a: u32, b: u32) -> u32 {
    let (mut a, mut b) = (a, b);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// The member gear `k` of a set read from a sketch or a selection is ([`Plan::members`]).
pub fn member_of_gear(p: &Plan, k: usize) -> Option<usize> {
    p.members.get(k)?.body
}

/// A size that is a whole tooth count.
fn whole(x: f64) -> Option<u32> {
    (x >= 1.0 && (x - x.round()).abs() < 1e-9 && x < f64::from(u32::MAX)).then(|| x.round() as u32)
}

/// Members that slide (racks).
fn linear_members(asm: &Assembly) -> Vec<bool> {
    let mut v = vec![false; asm.members.len()];
    for l in &asm.links {
        if let AssemblyLink::RackPinion { rack, .. } = *l
            && rack < v.len()
        {
            v[rack] = true;
        }
    }
    v
}

/// What a link is, its tooth counts in its member order, and its worm side (when a worm).
pub(crate) fn classify(asm: &Assembly, k: usize, e: &Efficiencies) -> (StepKind, Option<(u32, u32)>, Option<usize>) {
    let l = &asm.links[k];
    let given = e.kinds.iter().rev().find(|(i, _)| *i == k).map(|(_, m)| *m);
    match *l {
        AssemblyLink::Rigid { .. } => (StepKind::Shaft, None, None),
        AssemblyLink::RackPinion { .. } => (StepKind::Mesh(given.unwrap_or(MeshKind::Rack)), None, None),
        AssemblyLink::Worm { worm, starts, wheel_teeth, .. } => {
            (StepKind::Mesh(given.unwrap_or(MeshKind::Worm)), Some((starts, wheel_teeth)), Some(worm))
        }
        AssemblyLink::Gear { a, b, ra, rb, .. } => {
            let teeth = whole(ra).zip(whole(rb));
            let kind = given.unwrap_or_else(|| {
                let (Some(ma), Some(mb)) = (asm.members.get(a), asm.members.get(b)) else { return e.parallel };
                let (da, db) = (ma.axis.dir, mb.axis.dir);
                let par = dot(da, db);
                if par.abs() > 1.0 - 1e-9 {
                    // The same way round in space: a ring gear (or a chain).
                    if l.ratio() * par > 0.0 { e.parallel_same } else { e.parallel }
                } else {
                    let w = [ma.axis.origin[0] - mb.axis.origin[0], ma.axis.origin[1] - mb.axis.origin[1], ma.axis.origin[2] - mb.axis.origin[2]];
                    let n = [da[1] * db[2] - da[2] * db[1], da[2] * db[0] - da[0] * db[2], da[0] * db[1] - da[1] * db[0]];
                    let gap = dot(w, n).abs() / dot(n, n).sqrt();
                    let scale = dot(w, w).sqrt().max(1.0);
                    if gap < 1e-6 * scale { e.meeting } else { e.skew }
                }
            });
            let worm = (kind == MeshKind::Worm).then_some(if ra <= rb { a } else { b });
            (StepKind::Mesh(kind), teeth, worm)
        }
    }
}

/// Walk link `k` from member `from` (one of its ends): the step.
fn step(asm: &Assembly, k: usize, from: usize, e: &Efficiencies) -> Option<Step> {
    let l = &asm.links[k];
    let (a, b) = l.members();
    let forward = a == from;
    let to = if forward { b } else { a };
    let r = l.ratio();
    if !forward && r == 0.0 {
        return None;
    }
    let ratio = if forward { r } else { 1.0 / r };
    let (kind, teeth, worm) = classify(asm, k, e);
    let teeth = teeth.map(|(x, y)| if forward { (x, y) } else { (y, x) });
    let override_eff = e.per_link.iter().rev().find(|(i, _)| *i == k).map(|(_, v)| *v);
    let (efficiency, self_locking) = match (kind, worm) {
        (StepKind::Shaft, _) => (override_eff.unwrap_or(1.0), false),
        (StepKind::Mesh(MeshKind::Worm), Some(w)) => {
            let starts = teeth.map(|(x, y)| x.min(y)).unwrap_or(1);
            let we = e.worm_of(Some(k), starts);
            let eff = if from == w { we.forward } else { we.back };
            (override_eff.unwrap_or(eff), we.self_locking)
        }
        (StepKind::Mesh(m), _) => (override_eff.unwrap_or_else(|| e.of(m, 1)), false),
    };
    Some(Step { link: k, from, to, kind, teeth, ratio, efficiency, self_locking })
}

/// A breadth-first walk of the links from `root`: each reached member's motion per unit of
/// root's, the step that reached it, and the first loop whose ratios don't agree.
struct Walk {
    speed: Vec<Option<f64>>,
    via: Vec<Option<Step>>,
    conflict: Option<String>,
}

fn walk(asm: &Assembly, root: usize, e: &Efficiencies) -> Walk {
    let n = asm.members.len();
    let mut w = Walk { speed: vec![None; n], via: vec![None; n], conflict: None };
    if root >= n {
        return w;
    }
    w.speed[root] = Some(1.0);
    let mut queue = std::collections::VecDeque::from([root]);
    let mut used = vec![false; asm.links.len()];
    while let Some(m) = queue.pop_front() {
        let here = w.speed[m].unwrap_or(1.0);
        for (k, link) in asm.links.iter().enumerate() {
            let (a, b) = link.members();
            if used[k] || a >= n || b >= n || a == b || (a != m && b != m) {
                continue;
            }
            let Some(s) = step(asm, k, m, e) else { continue };
            used[k] = true;
            let across = here * s.ratio;
            match w.speed[s.to] {
                None => {
                    w.speed[s.to] = Some(across);
                    let to = s.to;
                    w.via[to] = Some(s);
                    queue.push_back(to);
                }
                Some(have) if (have - across).abs() > 1e-9 * have.abs().max(across.abs()).max(1.0) => {
                    if w.conflict.is_none() {
                        w.conflict = Some(format!(
                            "the drivetrain can't turn: a loop of meshes through members {} and {} doesn't agree (member {} would turn {:.6} one way round and {:.6} the other)",
                            m + 1,
                            s.to + 1,
                            s.to + 1,
                            have,
                            across
                        ));
                    }
                }
                Some(_) => {}
            }
        }
    }
    w
}

impl Walk {
    fn path(&self, to: usize) -> Vec<Step> {
        let mut v = Vec::new();
        let mut x = to;
        while let Some(s) = &self.via[x] {
            v.push(s.clone());
            x = s.from;
        }
        v.reverse();
        v
    }
}

/// The ratio between members `a` and `b` (ω_b / ω_a, each about its own axis) and the path of
/// meshes and shared shafts between them, with the default [`Efficiencies`]. An error when
/// either isn't a member, they aren't joined, one is held, or the links between them hold a
/// loop whose ratios don't agree (then nothing joined to it turns).
pub fn ratio_between(asm: &Assembly, a: usize, b: usize) -> Result<RatioReport, String> {
    ratio_between_with(asm, a, b, &Efficiencies::default())
}

/// [`ratio_between`] with the styles and efficiencies of `e`.
pub fn ratio_between_with(asm: &Assembly, a: usize, b: usize, e: &Efficiencies) -> Result<RatioReport, String> {
    let n = asm.members.len();
    for x in [a, b] {
        if x >= n {
            return Err(format!("there is no member {} (the drivetrain has {n})", x + 1));
        }
    }
    let w = walk(asm, a, e);
    if let Some(c) = &w.conflict {
        return Err(c.clone());
    }
    let Some(ratio) = w.speed[b] else {
        return Err(format!("members {} and {} aren't connected: no chain of meshes or shared shafts joins them", a + 1, b + 1));
    };
    let path = w.path(b);
    let mut notes = Vec::new();
    for x in std::iter::once(a).chain(path.iter().map(|s| s.to)) {
        let m = &asm.members[x];
        if m.grounded {
            return Err(format!("member {} is held (grounded): it doesn't turn, so neither does what it joins", x + 1));
        }
        if let Some(c) = m.carrier {
            notes.push(format!("member {} turns relative to its carrier (member {}); the carrier is taken as held", x + 1, c + 1));
        }
    }
    for s in &path {
        if s.self_locking {
            let w = if s.efficiency == 0.0 {
                "is driven from its wheel and is self-locking: it won't turn this way"
            } else {
                "is self-locking: the wheel can't drive it back"
            };
            notes.push(format!("the worm between members {} and {} {w}", s.from + 1, s.to + 1));
        }
    }
    let linear = linear_members(asm);
    let efficiency = path.iter().map(|s| s.efficiency).product();
    let reduction = if ratio == 0.0 { f64::INFINITY } else { 1.0 / ratio.abs() };
    let mm_per_rev = (linear[b] && !linear[a]).then_some(ratio * TAU);
    let rev_per_mm = (linear[a] && !linear[b]).then_some(ratio / TAU);
    Ok(RatioReport { from: a, to: b, ratio, direction: Direction::of(ratio), reduction, mm_per_rev, rev_per_mm, path, efficiency, notes })
}

/// [`ratio_between_with`] over a planned set: also an error when the plan's mesh graph has a
/// loop whose ratios don't agree joined to `a` ([`Plan::assembly_issues`]; the assembly leaves
/// its closing mesh out, so the links alone would hide it).
pub fn plan_ratio_between(spec: &Spec, p: &Plan, a: usize, b: usize) -> Result<RatioReport, String> {
    plan_ratio_between_with(p, a, b, &Efficiencies::for_spec(spec))
}

/// [`plan_ratio_between`] with the styles and efficiencies of `e` (for a drivetrain of items,
/// each link's own stage kind: [`crate::plan::system::SystemPlan::efficiencies`]).
pub fn plan_ratio_between_with(p: &Plan, a: usize, b: usize, e: &Efficiencies) -> Result<RatioReport, String> {
    for issue in &p.assembly_issues {
        if issue.members.iter().any(|&m| m == a || p.assembly.ratio(a, m).is_some()) {
            return Err(format!("the drivetrain can't turn: {}", issue.message));
        }
    }
    ratio_between_with(&p.assembly, a, b, e)
}

/// The speed, direction, torque and power at every member when member `driver` turns at `rpm`
/// with `torque` (N·m) on it, losing power at each step by `e`'s efficiencies along the path
/// from the driver. A sliding driver (a rack) is read as `rpm` mm/s and `torque` N. Members not
/// joined to the driver are listed unreached; a self-locking worm driven from its wheel stops
/// everything beyond it; a loop whose ratios don't agree locks everything joined to it.
pub fn drive_table(asm: &Assembly, driver: usize, rpm: f64, torque: f64, e: &Efficiencies) -> Vec<MemberState> {
    let n = asm.members.len();
    let linear = linear_members(asm);
    let w = walk(asm, driver, e);
    // The driver's speed (rad/s, or mm/s for a rack) and power (W).
    let (w_in, p_in) = if driver < n && linear[driver] { (rpm, torque * rpm / 1000.0) } else { (rpm * TAU / 60.0, torque * rpm * TAU / 60.0) };
    (0..n)
        .map(|i| {
            let m = &asm.members[i];
            let mut st = MemberState {
                member: i,
                body: m.body,
                reached: false,
                linear: linear[i],
                rpm: 0.0,
                linear_speed: None,
                direction: None,
                torque: 0.0,
                force: None,
                power: 0.0,
                efficiency: 0.0,
                locked: false,
                path: Vec::new(),
                note: None,
            };
            let Some(s) = w.speed[i] else {
                st.note = Some(if driver >= n {
                    format!("there is no member {}", driver + 1)
                } else {
                    format!("not connected to the driver (member {})", driver + 1)
                });
                return st;
            };
            st.reached = true;
            st.direction = Some(Direction::of(s));
            st.path = w.path(i);
            let v = s * w_in;
            if linear[i] {
                st.linear_speed = Some(v);
            } else {
                st.rpm = v * 60.0 / TAU;
            }
            if let Some(c) = &w.conflict {
                st.locked = true;
                st.note = Some(c.clone());
            } else if let Some(x) = std::iter::once(i).chain(st.path.iter().map(|s| s.from)).find(|x| asm.members[*x].grounded) {
                st.locked = true;
                st.note = Some(format!("member {} is held (grounded): nothing it joins turns", x + 1));
            } else if let Some(sl) = st.path.iter().find(|s| s.self_locking && s.efficiency == 0.0) {
                st.locked = true;
                st.note =
                    Some(format!("the worm between members {} and {} is self-locking: driven from its wheel it won't turn", sl.from + 1, sl.to + 1));
            }
            if st.locked {
                st.rpm = 0.0;
                st.linear_speed = st.linear_speed.map(|_| 0.0);
                st.force = linear[i].then_some(0.0);
                return st;
            }
            st.efficiency = st.path.iter().map(|s| s.efficiency).product();
            st.power = p_in * st.efficiency;
            if v.abs() > 1e-15 {
                if linear[i] {
                    // mm/s → m/s.
                    st.force = Some(st.power / (v / 1000.0));
                } else {
                    st.torque = st.power / v;
                }
            } else if linear[i] {
                st.force = Some(0.0);
            }
            st
        })
        .collect()
}

/// The meshes from `driver` to `output` (shared shafts left out), each with its own ratio: the
/// stages of a gearbox. Their ratios multiply to the overall one.
pub fn stage_ratios(asm: &Assembly, driver: usize, output: usize, e: &Efficiencies) -> Result<Vec<Step>, String> {
    Ok(ratio_between_with(asm, driver, output, e)?.path.into_iter().filter(|s| s.kind != StepKind::Shaft).collect())
}

/// The overall ratio from `driver` to `output`, its stages, and, given a `target` reduction
/// |ω_in / ω_out| (for a rack output: mm per turn), the error against it with a note.
pub fn overall_ratio(asm: &Assembly, driver: usize, output: usize, target: Option<f64>, e: &Efficiencies) -> Result<OverallRatio, String> {
    let report = ratio_between_with(asm, driver, output, e)?;
    let stages = report.path.iter().filter(|s| s.kind != StepKind::Shaft).cloned().collect();
    let got = report.mm_per_rev.map(f64::abs).unwrap_or(report.reduction);
    let (error, note) = match target {
        Some(t) if t > 0.0 && t.is_finite() => {
            let err = (got - t) / t;
            let what = if report.mm_per_rev.is_some() { "travel per turn" } else { "reduction" };
            let note = if err.abs() < 1e-9 {
                format!("the {what} is {got:.6}: exactly the target")
            } else {
                format!("the {what} is {got:.6} against a target of {t:.6}: {:+.3}% off", err * 100.0)
            };
            (Some(err), Some(note))
        }
        Some(t) => (None, Some(format!("a target of {t} isn't a ratio: give a positive number"))),
        None => (None, None),
    };
    Ok(OverallRatio { report, stages, target, error, note })
}

fn hunting_notes(c: &mut MeshCheck) {
    let Some((za, zb)) = c.teeth else { return };
    let f = common_factor(za, zb);
    c.common_factor = Some(f);
    c.hunting = Some(f == 1);
    if f > 1 {
        c.notes.push(format!(
            "{za} and {zb} teeth share a factor of {f}: each tooth meets only {} of its mate's {zb} teeth, so wear and any tooth error repeat; a tooth more or less on one gear makes a hunting-tooth pair",
            zb / f
        ));
    }
}

fn contact_notes(c: &mut MeshCheck) {
    if let Some(e) = c.total_contact_ratio.or(c.contact_ratio) {
        if e < 1.0 {
            c.notes.push(format!("contact ratio {e:.3} is below 1: the teeth lose contact between pairs"));
        } else if e < 1.2 {
            c.notes.push(format!("contact ratio {e:.3} is below 1.2: rough, noisy running"));
        }
    }
}

/// The meshes of an assembly's links (shared shafts left out): their kinds, tooth counts and
/// hunting-tooth check. Contact ratios need the teeth's geometry: see [`plan_mesh_checks`].
pub fn mesh_checks(asm: &Assembly, e: &Efficiencies) -> Vec<MeshCheck> {
    let mut v = Vec::new();
    for k in 0..asm.links.len() {
        let (kind, teeth, _) = classify(asm, k, e);
        let StepKind::Mesh(kind) = kind else { continue };
        let members = asm.links[k].members();
        let ratio = teeth.map(|(a, b)| f64::from(b) / f64::from(a.max(1))).unwrap_or_else(|| asm.links[k].ratio().abs().recip());
        let mut c = MeshCheck {
            link: Some(k),
            gears: None,
            members,
            kind,
            teeth,
            ratio,
            contact_ratio: None,
            total_contact_ratio: None,
            common_factor: None,
            hunting: None,
            notes: Vec::new(),
        };
        hunting_notes(&mut c);
        v.push(c);
    }
    v
}

fn fact(p: &Plan, label: &str) -> Option<f64> {
    p.facts.iter().find(|f| f.label == label).map(|f| f.value)
}

/// The meshes of a planned set with the plan's contact ratios: every mesh of a train (also one
/// that closes a loop, which isn't an assembly link), a pair's, a planetary set's (sun–planet,
/// planet–ring); other sets as [`mesh_checks`] with the plan's "contact ratio" where it has one.
pub fn plan_mesh_checks(spec: &Spec, p: &Plan) -> Vec<MeshCheck> {
    let e = Efficiencies::for_spec(spec);
    if let Some(t) = &spec.train {
        use crate::plan::train::LinkKind;
        let mut v = Vec::new();
        for l in t.links.iter().filter(|l| l.kind != LinkKind::Compound) {
            let (Some(ga), Some(gb)) = (t.gears.get(l.a), t.gears.get(l.b)) else { continue };
            let member = |g: usize| member_of_gear(p, g).unwrap_or(g);
            let members = (member(l.a), member(l.b));
            let link = p.assembly.links.iter().position(|x| {
                matches!(x, AssemblyLink::Gear { .. }) && {
                    let (a, b) = x.members();
                    (a, b) == members || (b, a) == members
                }
            });
            let kind = if l.kind == LinkKind::Internal { MeshKind::Internal } else { e.parallel };
            let pair = format!("gears {}–{}", l.a + 1, l.b + 1);
            let contact = fact(p, &format!("{pair} contact ratio"));
            let total = fact(p, &format!("{pair} total contact ratio")).or(contact);
            let mut c = MeshCheck {
                link,
                gears: Some((l.a, l.b)),
                members,
                kind,
                teeth: Some((ga.teeth, gb.teeth)),
                ratio: f64::from(gb.teeth) / f64::from(ga.teeth.max(1)),
                contact_ratio: contact,
                total_contact_ratio: total,
                common_factor: None,
                hunting: None,
                notes: Vec::new(),
            };
            hunting_notes(&mut c);
            contact_notes(&mut c);
            v.push(c);
        }
        return v;
    }
    let mut v = mesh_checks(&p.assembly, &e);
    for c in &mut v {
        let (contact, total) = match spec.kind {
            Kind::Planetary if c.kind == MeshKind::Internal => (fact(p, "planet–ring contact ratio"), None),
            Kind::Planetary => (fact(p, "sun–planet contact ratio"), None),
            _ => (fact(p, "contact ratio"), fact(p, "total contact ratio")),
        };
        c.contact_ratio = contact;
        c.total_contact_ratio = total.or(contact);
        contact_notes(c);
    }
    v
}
