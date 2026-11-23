//! The clearance body (keep-out volume) of a gear set: one solid holding exactly the space the
//! assembled set needs to turn, so a housing is any block with this body cut out of it.
//!
//! What it holds, all grown by a radial and an axial clearance (0 is 0: an envelope then ends at
//! its member's material along the axis, and stands only the sampling allowance past it round
//! the axis):
//! - every member's turning envelope: a revolve about its axis of the most radius its material
//!   reaches at each position along the axis, taken from the planned steps themselves (points on
//!   every outline of every step, through the steps' and bodies' frames; see
//!   [`crate::mesh3d::Solid::material_points`]), so it clears the teeth as built: cylinders for
//!   spur and helical teeth, cone frusta for bevels, the lofted hypoid members' own radius along
//!   the axis, a worm's thread and a throated wheel's throat;
//! - planets orbiting the sun (the carrier may turn) and a cycloidal drive's disc orbiting the
//!   input (disc radius + eccentricity): revolves about the set's axis; a bevel planetary set's
//!   planets, tilted to the sun's axis, are turned about their own axes first (their material
//!   swept round) and then revolved about the sun's, a second sun turning on the sun's axis and
//!   a bevel ring taken as a ring;
//! - a rack's slot: its bounding box made longer by the travel it slides over;
//! - an internal ring (and a cycloidal drive's pin ring): its outer rim, or nothing when the ring
//!   is part of the housing ([`Clearance::ring_as_housing`]);
//! - a chain or belt between two sprockets or pulleys: the region round both wheels and the two
//!   runs, as a prism over the chain's or belt's width;
//! - each member's shaft (its bore's size, else the bearing's bore when it has seats), on through the housing walls
//!   by [`Clearance::shaft_extension`] beyond the gear on each side; integral shafts as built;
//! - bearing seats: one bearing size (from [`crate::hub::BEARINGS`] or given), at points on the
//!   shafts, each a cylinder of the bearing's outside diameter plus a fit allowance and its width.
//!
//! The members on one axis share one revolved profile (gear, shaft and seats together), so the
//! body is a handful of revolves joined, plus the prisms of a rack's slot or a chain's run.

use crate::axes::{cross, dot, len, mul, num, sub};
use crate::geom::{Outline, Region, Seg, V2};
use crate::hub::{Bore, BoreKind};
use crate::mesh3d::Solid;
use crate::plan::{Body, Frame, Kind, Op, Plan, PreviewGear, Spec, Step, Unit};

/// A bearing's size in mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BearingSize {
    /// The bore (0: not known; the shaft check is skipped).
    pub bore: f64,
    pub od: f64,
    pub width: f64,
}

/// Where a bearing seat goes: a dot on a member's shaft.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seat {
    /// On member `member` (1-based: the set's bodies in their order, gear 1 first), `along` mm
    /// along its axis (signed, the axis's own direction) from the middle of its teeth.
    Along { member: u32, along: f64 },
    /// A point (world coordinates, mm), moved onto the nearest member's axis.
    Point([f64; 3]),
}

/// What the clearance body is made with. Off unless `Spec::clearance_body` is set; these
/// defaults are what a front end starts from.
#[derive(Clone, Debug, PartialEq)]
pub struct Clearance {
    /// Radial clearance round every tip envelope, shaft and chain run, mm.
    pub radial: f64,
    /// Axial clearance at each end of every member's material, mm.
    pub axial: f64,
    /// How far each member's shaft runs on past its gear on each side, mm (0: twice the face
    /// width, enough to pass through a housing wall). Integral shafts are taken as built.
    pub shaft_extension: f64,
    /// An internal ring gear (and a cycloidal drive's pin ring) is part of the housing: it is
    /// left out (true), or its outer rim is cleared like any other member (false).
    pub ring_as_housing: bool,
    /// The bearing, by its name in [`crate::hub::BEARINGS`] ("608", "6001", "MR105"...). Taken
    /// only with `seats`: with none it makes nothing (and is warned of).
    pub bearing: Option<String>,
    /// A bearing of any size instead (used when `bearing` is not set).
    pub bearing_size: Option<BearingSize>,
    /// How much larger than the bearing's outside diameter a seat is made, on the diameter, mm.
    pub bearing_fit: f64,
    /// Where the bearing seats go.
    pub seats: Vec<Seat>,
    /// A rack's travel: how far it slides in all (mm; 0: its own length), its slot that much
    /// longer than the rack.
    pub travel: f64,
    /// Make the keep-out body itself (`Plan::clearance`); false places the bearing seats only
    /// (for `bearing_bodies`).
    pub keep_out: bool,
    /// Also build each seat's bearing as two bodies, its outer and inner ring (see
    /// [`add_bearing_bodies`]).
    pub bearing_bodies: bool,
}

/// A bearing placed at a seat ([`Plan::bearings`]).
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedBearing {
    /// The seat (1-based, as the warnings count them).
    pub seat: usize,
    /// The member (an index into the plan's bodies) whose shaft it carries.
    pub member: usize,
    /// Where it is (world): its middle on the member's axis, z along the axis.
    pub frame: Frame,
    /// The bearing's name ("608"; "custom" for a size given) and size.
    pub name: String,
    pub size: BearingSize,
}

impl Default for Clearance {
    fn default() -> Self {
        Self {
            radial: 0.5,
            axial: 0.5,
            shaft_extension: 0.0,
            ring_as_housing: true,
            bearing: None,
            bearing_size: None,
            bearing_fit: 0.0,
            seats: Vec::new(),
            travel: 0.0,
            keep_out: true,
            bearing_bodies: false,
        }
    }
}

impl Clearance {
    /// The bearing chosen: its name ("custom" for a size given) and size.
    pub fn chosen_bearing(&self) -> Result<Option<(String, BearingSize)>, String> {
        if let Some(name) = &self.bearing {
            return match crate::hub::bearing(name) {
                Some((bore, od, width)) => Ok(Some((name.trim().to_string(), BearingSize { bore, od, width }))),
                None => Err(format!("clearance: no standard bearing is called '{}'", name.trim())),
            };
        }
        Ok(self.bearing_size.map(|b| ("custom".to_string(), b)))
    }
}

/// Validates the clearance settings (the members a seat names are checked when the body is
/// made).
pub fn check(c: &Clearance) -> Result<(), String> {
    let within = |v: f64, lo: f64, hi: f64, what: &str| {
        if v.is_finite() && (lo..=hi).contains(&v) { Ok(()) } else { Err(format!("clearance: {what} must be between {lo} and {hi} mm")) }
    };
    within(c.radial, 0.0, 50.0, "the radial clearance")?;
    within(c.axial, 0.0, 50.0, "the axial clearance")?;
    within(c.shaft_extension, 0.0, 10_000.0, "the shaft extension")?;
    within(c.travel, 0.0, 100_000.0, "the rack's travel")?;
    within(c.bearing_fit, -1.0, 5.0, "the bearing fit")?;
    if let Some(b) = c.bearing_size
        && c.bearing.is_none()
    {
        let ok = [b.bore, b.od, b.width].iter().all(|v| v.is_finite())
            && b.od > 0.0
            && b.width > 0.0
            && b.bore >= 0.0
            && b.bore < b.od
            && b.od <= 2000.0
            && b.width <= 1000.0;
        if !ok {
            return Err("clearance: a bearing needs an outside diameter and a width over 0 (and a bore under the outside diameter)".into());
        }
    }
    c.chosen_bearing()?;
    if c.seats.len() > 64 {
        return Err("clearance: 64 bearing seats at most".into());
    }
    if !c.seats.is_empty() && c.chosen_bearing()?.is_none() {
        return Err("clearance: bearing seats need a bearing (`bearing` or its size)".into());
    }
    for (i, seat) in c.seats.iter().enumerate() {
        let ok = match seat {
            Seat::Along { member, along } => *member >= 1 && along.is_finite() && along.abs() <= 1e5,
            Seat::Point(p) => p.iter().all(|v| v.is_finite() && v.abs() <= 1e7),
        };
        if !ok {
            return Err(format!("clearance: bearing seat {} needs a member (1, 2, ...) and a finite distance along it, or a finite point", i + 1));
        }
    }
    Ok(())
}

/// How a member moves, for its clearance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Motion {
    /// Turns about its own frame's z axis.
    Turns,
    /// Turns about its own axis and goes round `about`'s z axis (planets on a turning carrier,
    /// a cycloidal drive's disc on its eccentric), `distance` from it.
    Orbits { about: Frame, distance: f64 },
    /// Slides along its own frame's x (a rack).
    Slides,
    /// A ring gear (or pin ring), which may be part of the housing.
    Ring,
}

/// How each of a whole set's bodies (in plan order) moves.
pub(crate) fn motions(kind: Kind, bodies: &[Body]) -> Vec<Motion> {
    let n = bodies.len();
    (0..n)
        .map(|i| match kind {
            Kind::Rack => Motion::Slides,
            Kind::Internal => Motion::Ring,
            Kind::Planetary if n >= 3 && i == n - 1 => Motion::Ring,
            Kind::Planetary if i > 0 => orbit(&bodies[0].frame, &bodies[i].frame),
            Kind::CycloidalDrive if n >= 2 && i == 1 => Motion::Ring,
            Kind::CycloidalDrive if n >= 2 && i == 0 => orbit(&bodies[1].frame, &bodies[0].frame),
            // Sun first; members off its axis are planets; on it, the ring has the sun's
            // apex-pointing z (a second sun's points the other way).
            // The carrier turns about the sun's axis.
            Kind::BevelPlanetary if bodies[i].name.contains(crate::plan::bevel_planet::CARRIER) => Motion::Turns,
            Kind::BevelPlanetary if i > 0 => {
                let (sun, f) = (&bodies[0].frame, &bodies[i].frame);
                if coaxial(sun, f) { if dot(sun.z, f.z) > 0.0 { Motion::Ring } else { Motion::Turns } } else { orbit(sun, f) }
            }
            _ => Motion::Turns,
        })
        .collect()
}

fn orbit(about: &Frame, f: &Frame) -> Motion {
    let d = sub(f.origin, about.origin);
    let along = dot(d, about.z);
    let off = sub(d, mul(about.z, along));
    Motion::Orbits { about: *about, distance: dot(off, off).sqrt() }
}

/// The point spacing round outlines (`h`) and along sweeps (`hz`) the envelopes are sampled at
/// for a set of module `m` and a body `span` mm long.
pub(crate) fn spacing(m: f64, span: f64) -> (f64, f64) {
    ((0.04 * m).clamp(0.01, 0.25), (span / 120.0).clamp(0.05, 1.0))
}

/// One axis the clearance revolves about: points of material (position along the axis,
/// radius) still to be grown by the clearances, and bands already sized (shafts, seats).
struct Group {
    frame: Frame,
    /// Points of material: r grown by `margin`, each point's need taken `reach` either way along
    /// the axis (the gap to the next row) but never out of its window.
    pts: Vec<GroupPt>,
    /// Each member's window along the axis (its material's ends, plus the axial clearance): the
    /// envelope of its points stops there exactly, so an axial clearance of 0 ends the envelope
    /// at the material's ends.
    wins: Vec<(f64, f64)>,
    /// (z0, z1, r): a cylinder already sized.
    bands: Vec<(f64, f64, f64)>,
    /// The largest row spacing of its members (where a vertex may follow a tip envelope).
    hz: f64,
}

#[derive(Clone, Copy)]
struct GroupPt {
    z: f64,
    r: f64,
    margin: f64,
    reach: f64,
    /// Its window, an index into `Group::wins`.
    win: usize,
}

/// How far a sampled outline's farthest vertex can fall short of the outline itself, for vertex
/// spacing `h`, on the radius: every vertex is on the outline, and on gear outlines the radius
/// along each segment is monotonic or constant (tip arcs centred on the axis), so the farthest
/// vertex is the farthest point; elsewhere (a pin's or lobe's arc of curvature radius ρ ≥ h)
/// the chord's sagitta h²/8ρ ≤ h/8. Plus a hair for rounding.
fn sampling_allowance(h: f64) -> f64 {
    h / 8.0 + 0.002
}

/// The row spacing along the axis the keep-out reads a member at: the spacing for its span, at
/// most 0.1 mm, so a radius step inside one body (a compound's layers) is not smeared far.
fn row_spacing(hz: f64) -> f64 {
    hz.min(0.1)
}

fn coaxial(a: &Frame, b: &Frame) -> bool {
    let par = len(cross(a.z, b.z)) < 1e-9;
    let d = sub(b.origin, a.origin);
    let off = sub(d, mul(a.z, dot(d, a.z)));
    par && len(off) < 1e-6
}

fn group_of(groups: &mut Vec<Group>, f: &Frame) -> usize {
    if let Some(i) = groups.iter().position(|g| coaxial(&g.frame, f)) {
        return i;
    }
    groups.push(Group { frame: *f, pts: Vec::new(), wins: Vec::new(), bands: Vec::new(), hz: 0.0 });
    groups.len() - 1
}

/// A member as the clearance sees it.
struct Member {
    who: String,
    body: Body,
    motion: Motion,
    /// Points of its material, its own coordinates.
    pts: Vec<[f64; 3]>,
    /// Its material's z range, and the middle of its teeth (the first step), own coordinates.
    z: (f64, f64),
    /// The z range of its teeth (its first step), and their middle.
    teeth: (f64, f64),
    mid: f64,
    /// A planet on a carrier: no shaft through the housing, no seats.
    planet: bool,
    hz: f64,
    h: f64,
    /// Gear 1 (1), gear 2 and its kind (2), or none (0): whose bore and shafts it has.
    role: u8,
    /// The group (axis) it turns about, once placed.
    group: Option<usize>,
    /// Its shaft's clearance band in its group's coordinates, and the shaft's diameter.
    shaft: Option<(f64, f64, f64)>,
    shaft_d: Option<f64>,
}

/// How a vertex of a clearance profile follows a parametric set's design parameters (its teeth
/// drawn from formulas): the profile is the numbers when planned, moved by how much the
/// parameters change what made it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VertexFollow {
    /// The radius moves with a member's tip envelope: (template over the teeth formulas' keys,
    /// its value when planned). A planet's is its orbit and tip, `{mesh_cd} + {g2_ra}`.
    pub r: Option<(String, f64)>,
    /// The position along the axis moves by `k` times the change of the face width: (k, the
    /// face width's key, its value when planned).
    pub z: Option<(f64, String, f64)>,
}

/// What a member's clearance follows in a parametric set: its tip envelope (template and its
/// value: orbit and tip radius) and its face width (key and value).
#[derive(Clone, Debug)]
struct MemberFollow {
    r_tpl: String,
    r_at: f64,
    face_key: String,
    face: f64,
}

/// The members of a planar set whose clearance can follow formulas (whole set, plan order):
/// gears whose teeth have a formula tip radius (`g<member>_ra`), and planets on the centre
/// distance's formula. Rings, racks and other kinds: none.
fn member_follows(s: &Spec, p: &Plan, bodies: &[Body]) -> Vec<Option<MemberFollow>> {
    let n = bodies.len();
    let ring = s.kind == Kind::Planetary && n >= 3;
    (0..n)
        .map(|i| {
            let member: u8 = match s.kind {
                Kind::Spur | Kind::Helical | Kind::Herringbone => 1,
                Kind::Pair => (i + 1) as u8,
                Kind::Planetary if i == 0 => 1,
                Kind::Planetary if ring && i == n - 1 => return None,
                Kind::Planetary => 2,
                _ => return None,
            };
            let src = p.teeth.iter().find(|t| t.member == member)?;
            let crate::equations::Form::External { f, .. } = &src.form else { return None };
            let tip = format!("{{g{member}_ra}}");
            let planet = s.kind == Kind::Planetary && i > 0;
            let (r_tpl, r_at) = if planet {
                let cd = p.placing.vars.value("mesh_cd")?;
                if !p.placing.centres.iter().any(|(k, ..)| *k == i) {
                    return None;
                }
                (format!("{{mesh_cd}} + {tip}"), cd + f.ra)
            } else {
                (tip, f.ra)
            };
            Some(MemberFollow { r_tpl, r_at, face_key: src.face_key.clone(), face: src.face })
        })
        .collect()
}

/// A group's rules for [`VertexFollow`]: tip envelopes (radius planned, the member's follow,
/// its z range in the group, how far past the tip a vertex of it may be), the face's stretch
/// (the teeth's z range in the group, key, value), and band ends that move by a multiple of the
/// face's change (z, k).
#[derive(Default)]
struct GroupFollow {
    tips: Vec<(f64, MemberFollow, (f64, f64), f64)>,
    stretch: Option<((f64, f64), String, f64)>,
    anchors: Vec<(f64, f64)>,
}

impl GroupFollow {
    fn vertex(&self, q: V2, reach: f64) -> VertexFollow {
        let r = self.tips.iter().find(|(at, _, (z0, z1), window)| {
            let d = q.x - at;
            d > 0.0 && d <= window + 1e-6 && q.y >= z0 - reach - 1e-6 && q.y <= z1 + reach + 1e-6
        });
        let z = self.stretch.as_ref().map(|((t0, t1), key, face)| {
            let k = match self.anchors.iter().find(|(z, _)| (z - q.y).abs() <= 2e-6) {
                Some((_, k)) => *k,
                None if t1 > t0 => ((q.y - t0) / (t1 - t0)).clamp(0.0, 1.0),
                None => 0.0,
            };
            (k, key.clone(), *face)
        });
        VertexFollow { r: r.map(|(_, m, ..)| (m.r_tpl.clone(), m.r_at)), z: z.filter(|(k, ..)| *k != 0.0) }
    }
}

/// The set's clearance body (see the module notes), named "<set name> clearance". Its facts
/// (envelope sizes, the bearing) and warnings go into `p`. `p` is the set as planned and placed;
/// when it is one member of a set (`only`), the whole set is planned for its clearance.
pub fn clearance_body(s: &Spec, p: &mut Plan) -> Result<Body, String> {
    let c = s.clearance_body.clone().unwrap_or_default();
    check(&c)?;
    let whole;
    let (bodies, preview): (&[Body], &[PreviewGear]) = if s.only == 0 {
        (&p.bodies, &p.preview)
    } else {
        whole = crate::plan::plan(&Spec { only: 0, clearance_body: None, ..s.clone() })?;
        (&whole.bodies, &whole.preview)
    };
    let mut facts = Vec::new();
    let mut warnings = Vec::new();
    // (Following the formulas: only for the whole set, whose plan holds the bodies' own teeth.)
    let follows = if s.only == 0 { member_follows(s, p, bodies) } else { vec![None; bodies.len()] };
    let mut placed = Vec::new();
    let built_shafts = if s.only == 0 { on_built_shaft(p) } else { vec![false; bodies.len()] };
    let (body, follow) = build(s, &c, bodies, preview, &follows, &built_shafts, &mut facts, &mut warnings, &mut placed)?;
    p.clearance_follow = follow;
    p.bearings = placed;
    for (label, value, unit) in facts {
        p.fact(label, value, unit);
    }
    for w in warnings {
        p.warn(w);
    }
    Ok(body)
}

fn set_name(s: &Spec) -> String {
    if s.name.trim().is_empty() { s.kind.label().to_string() } else { s.name.trim().to_string() }
}

/// A body's name without the set's name in front.
fn who_of(s: &Spec, body: &Body) -> String {
    let pre = format!("{} ", s.name.trim());
    if s.name.trim().is_empty() { body.name.clone() } else { body.name.strip_prefix(&pre).unwrap_or(&body.name).to_string() }
}

fn bore_of(s: &Spec, role: u8) -> (Bore, bool, f64) {
    let (bore, front, back) = match role {
        1 => (s.bore, s.shaft_front, s.shaft_back),
        2 => (s.bore2, s.shaft2_front, s.shaft2_back),
        _ => return (Bore::default(), false, 0.0),
    };
    let integral = front.exists() || back.exists();
    let d = if front.exists() { front.diameter } else { back.diameter };
    (bore, integral, d)
}

/// Per body, whether its shaft is a body built with the set (a drivetrain's Shaft item: a bar
/// with no teeth), or it is that bar: such a body's shaft clearance runs only as far as the
/// bar's own envelope (no stub on past it), since the bar's own clearance is the shaft's.
fn on_built_shaft(p: &Plan) -> Vec<bool> {
    let bars: Vec<usize> =
        p.shafts.iter().map(|sh| sh.body).filter(|b| p.members.iter().any(|m| m.body == Some(*b) && m.teeth == 0) && *b < p.bodies.len()).collect();
    (0..p.bodies.len())
        .map(|b| {
            if bars.contains(&b) {
                return true;
            }
            let f = p.bodies[b].frame;
            let clusters: Vec<usize> = p.members.iter().filter(|m| m.body == Some(b)).filter_map(|m| m.cluster).collect();
            bars.iter().any(|&bar| {
                let rigid = p.members.iter().any(|m| m.body == Some(bar) && m.cluster.is_some_and(|c| clusters.contains(&c)));
                let g = p.bodies[bar].frame;
                let d = sub(f.origin, g.origin);
                let off = len(sub(d, mul(g.z, dot(d, g.z))));
                rigid && len(cross(f.z, g.z)) < 1e-6 && off < 0.05
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn build(
    s: &Spec,
    c: &Clearance,
    bodies: &[Body],
    preview: &[PreviewGear],
    follows: &[Option<MemberFollow>],
    built_shafts: &[bool],
    facts: &mut Vec<(String, f64, Unit)>,
    warn: &mut Vec<String>,
    placed: &mut Vec<PlacedBearing>,
) -> Result<(Body, Vec<Vec<VertexFollow>>), String> {
    if bodies.is_empty() {
        return Err("clearance: the set has no bodies".into());
    }
    // A bearing is taken only where it has seats: with none it makes nothing (no seats, and
    // its bore is no shaft); named with none, say what to do.
    let bearing = match c.chosen_bearing()? {
        Some((name, _)) if c.seats.is_empty() => {
            warn.push(format!(
                "clearance: bearing {name} has nowhere to sit: put bearing points on a shaft line (Shafts & Bores tab), or turn bearings off"
            ));
            None
        }
        b => b,
    };
    let motions = motions(s.kind, bodies);
    let ext = if c.shaft_extension > 0.0 { c.shaft_extension } else { 2.0 * s.face_width };
    let (rad, ax) = (c.radial, c.axial);

    // Every member: its material's points (each member on a thread of its own), its shaft.
    let sampled = crate::par::map(bodies.len(), |i| {
        let b = &bodies[i];
        let coarse = Solid::new(b, 1.0);
        let span = (coarse.z.1 - coarse.z.0).max(1e-3);
        let (h, hz) = spacing(s.module, span);
        // A bevel planetary set's carrier has no teeth: plain round parts, sampled four times
        // coarser (its margin grows with the spacing, so it is still cleared).
        let (h, hz) = if b.name.contains(crate::plan::bevel_planet::CARRIER) { (4.0 * h, 4.0 * hz) } else { (h, row_spacing(hz)) };
        // A slide's box reads every point; anything else only each row's farthest from its axis
        // (the same envelope: a ring gear's rows hold millions of points).
        let solid = Solid::new(b, h);
        let pts = if matches!(motions.get(i), Some(Motion::Slides)) { solid.material_points(hz) } else { solid.farthest_per_row(hz) };
        let z = pts.iter().fold((f64::MAX, f64::MIN), |a, q| (a.0.min(q[2]), a.1.max(q[2])));
        (h, hz, pts, z)
    });
    let mut members: Vec<Member> = Vec::new();
    for (i, ((b, motion), (h, hz, pts, z))) in bodies.iter().zip(&motions).zip(sampled).enumerate() {
        let first = Solid::new(&Body { name: String::new(), frame: b.frame, steps: b.steps.iter().take(1).cloned().collect() }, 1.0);
        let mid = 0.5 * (first.z.0 + first.z.1);
        let planet = matches!(motion, Motion::Orbits { .. }) && matches!(s.kind, Kind::Planetary | Kind::BevelPlanetary);
        let role = match motion {
            Motion::Ring => 0,
            _ if i == 0 => 1,
            // A second sun has the sun's bore.
            Motion::Turns if s.kind == Kind::BevelPlanetary => 1,
            _ => 2,
        };
        members.push(Member {
            who: who_of(s, b),
            body: b.clone(),
            motion: *motion,
            pts,
            z,
            teeth: first.z,
            mid,
            planet,
            hz,
            h,
            role,
            group: None,
            shaft: None,
            shaft_d: None,
        });
    }
    let rings_only = members.iter().all(|m| m.motion == Motion::Ring);
    if rings_only && c.ring_as_housing {
        warn.push("clearance: the set is only a ring gear, so its rim is cleared although the ring is taken as part of the housing".into());
    }
    let skip_ring = c.ring_as_housing && !rings_only;

    let mut groups: Vec<Group> = Vec::new();
    let mut prisms: Vec<(Frame, Region, f64, f64)> = Vec::new();
    // What each group's profile follows (parametric sets), by group.
    let mut rules: Vec<GroupFollow> = Vec::new();
    for (mi, m) in members.iter_mut().enumerate() {
        let f = m.body.frame;
        let margin = rad + sampling_allowance(m.h);
        match m.motion {
            Motion::Ring if skip_ring => continue,
            Motion::Slides => {
                // The slot: the rack's box, longer by its travel.
                let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
                for q in &m.pts {
                    for k in 0..3 {
                        lo[k] = lo[k].min(q[k]);
                        hi[k] = hi[k].max(q[k]);
                    }
                }
                let length = hi[0] - lo[0];
                let travel = if c.travel > 0.0 { c.travel } else { length };
                let (x0, x1) = (lo[0] - travel / 2.0 - margin, hi[0] + travel / 2.0 + margin);
                let (y0, y1) = (lo[1] - margin, hi[1] + margin);
                let rect = Outline {
                    segs: vec![
                        Seg::Line(V2::new(x0, y0), V2::new(x1, y0)),
                        Seg::Line(V2::new(x1, y0), V2::new(x1, y1)),
                        Seg::Line(V2::new(x1, y1), V2::new(x0, y1)),
                        Seg::Line(V2::new(x0, y1), V2::new(x0, y0)),
                    ],
                };
                prisms.push((f, Region { outer: vec![rect], holes: vec![] }, lo[2] - ax, hi[2] - lo[2] + 2.0 * ax));
                facts.push((format!("clearance: {} slot length", m.who), x1 - x0, Unit::Mm));
                facts.push((format!("clearance: {} travel", m.who), travel, Unit::Mm));
                continue;
            }
            _ => {}
        }
        let (axis, orbit) = match m.motion {
            Motion::Orbits { about, distance } => (about, distance),
            _ => (f, 0.0),
        };
        let gi = group_of(&mut groups, &axis);
        m.group = Some(gi);
        let g = &mut groups[gi];
        g.hz = g.hz.max(m.hz);
        let mut zr = (f64::MAX, f64::MIN);
        let mut rmax = 0.0f64;
        // An orbiting member whose axis is tilted to the one it goes round (bevel planets):
        // each point swept round its own axis, every place measured about the group's. Its own
        // clearances run partly across the group's axis and partly along it, so they are
        // applied in its own frame: the radius grown by the radial one, the points also taken
        // the axial one along its own axis either way (and the group's margins on top).
        let tilted = orbit > 0.0 && len(cross(f.z, g.frame.z)) > 1e-9;
        let win = g.wins.len();
        // Each point's need is taken the axial clearance and a row either way, never out of
        // the window (a tilted member's runs partly across the group's axis: a row further).
        let reach = ax + m.hz;
        let mut push = |zg: f64, r: f64, margin: f64| {
            zr = (zr.0.min(zg), zr.1.max(zg));
            rmax = rmax.max(r);
            g.pts.push(GroupPt { z: zg, r, margin, reach, win });
        };
        if tilted {
            // Swept round its own axis each row of points is a disc of its largest radius, and
            // the distance from the group's axis along any chord of it is greatest at the rim:
            // so only that rim, per row.
            // Rows half a row spacing wide, at their middle with their largest radius, then
            // each row's radius the largest within the axial clearance (and half a row) of it
            // along its own axis: the points taken that far either way.
            let wide = 0.5 * m.hz;
            let mut rows: std::collections::BTreeMap<i64, f64> = std::collections::BTreeMap::new();
            for q in &m.pts {
                let row = rows.entry((q[2] / wide).floor() as i64).or_insert(0.0);
                *row = row.max(q[0].hypot(q[1]));
            }
            let (Some(&k0), Some(&k1)) = (rows.keys().next(), rows.keys().next_back()) else { continue };
            let w = ((ax + 0.5 * wide) / wide).ceil() as i64;
            let rims: Vec<(f64, f64)> = (k0 - w..=k1 + w)
                .filter_map(|k| {
                    let r = rows.range(k - w..=k + w).map(|(_, r)| *r).fold(0.0, f64::max);
                    (r > 0.0).then_some(((k as f64 + 0.5) * wide, r))
                })
                .collect();
            let fine = sampling_allowance(m.h) + 0.5 * m.hz;
            // The samples' need in bands along the group's axis: each band's largest radius at
            // both its edges (every sample in it is within them, at that radius or less).
            const BAND: f64 = 0.05;
            let mut bands: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();
            for (z, rho) in rims {
                let rho = rho + rad;
                // Round the rim at the point spacing: a tilted rim's steps go along the group's
                // axis too.
                let n = (std::f64::consts::TAU * rho / m.h.min(m.hz)).ceil().clamp(8.0, 4096.0) as usize;
                for k in 0..n {
                    let t = std::f64::consts::TAU * k as f64 / n as f64;
                    let d = sub(f.world([rho * t.cos(), rho * t.sin(), z]), g.frame.origin);
                    let zg = dot(d, g.frame.z);
                    let r = len(sub(d, mul(g.frame.z, zg)));
                    let b = bands.entry((zg / BAND).floor() as i64).or_insert(0.0);
                    *b = b.max(r);
                }
            }
            let mut bands: Vec<(i64, f64)> = bands.into_iter().collect();
            bands.sort_by_key(|(k, _)| *k);
            for (k, r) in bands {
                push(k as f64 * BAND, r, fine);
                push((k + 1) as f64 * BAND, r, fine);
            }
        } else {
            for q in &m.pts {
                let zg = dot(sub(f.world(*q), g.frame.origin), g.frame.z);
                push(zg, orbit + q[0].hypot(q[1]), margin);
            }
        }
        if zr.1 >= zr.0 {
            let grow = if tilted { ax + m.hz } else { ax };
            g.wins.push((zr.0 - grow, zr.1 + grow));
        }
        if rules.len() <= gi {
            rules.resize_with(gi + 1, GroupFollow::default);
        }
        let to_group = |z: f64| dot(sub(f.world([0.0, 0.0, z]), g.frame.origin), g.frame.z);
        let teeth_g = {
            let (a, b) = (to_group(m.teeth.0), to_group(m.teeth.1));
            (a.min(b), a.max(b))
        };
        if let Some(Some(mf)) = follows.get(mi).filter(|_| !tilted)
            && (rmax - mf.r_at).abs() <= 1e-6 * mf.r_at.max(1.0)
        {
            let rule = &mut rules[gi];
            rule.tips.push((rmax, mf.clone(), zr, margin + UNDER + OVER));
            if rule.stretch.is_none() {
                rule.stretch = Some((teeth_g, mf.face_key.clone(), mf.face));
            }
        }
        let what = if orbit > 0.0 { "orbit" } else { "envelope" };
        facts.push((format!("clearance: {} {what} diameter", m.who), 2.0 * (rmax + margin), Unit::Mm));
        facts.push((format!("clearance: {} {what} length", m.who), zr.1 - zr.0 + 2.0 * ax, Unit::Mm));
        // The shaft: through the gear and on past it, unless it is integral (built with the
        // gear and in its points) or the member orbits on a carrier.
        let (bore, integral, integral_d) = bore_of(s, m.role);
        let d = if bore.kind != BoreKind::None && bore.diameter > 0.0 {
            Some(2.0 * bore.reach())
        } else if integral {
            Some(integral_d)
        } else {
            bearing.as_ref().map(|(_, b)| b.bore).filter(|d| *d > 0.0)
        };
        m.shaft_d = if integral { Some(integral_d) } else { d };
        if let (Some(d), false, false) = (d, integral, m.planet) {
            // On a shaft built with the set (or that shaft): no stub past it.
            let built = built_shafts.get(mi).copied().unwrap_or(false);
            let ext = if built { 0.0 } else { ext };
            let band = (zr.0 - ext, zr.1 + ext, d / 2.0 + rad);
            // Its ends: on past the gear's ends, by twice the face width unless given.
            let more = if c.shaft_extension > 0.0 || built { 0.0 } else { 2.0 };
            rules[gi].anchors.extend([(band.0, -more), (band.1, 1.0 + more)]);
            g.bands.push(band);
            m.shaft = Some(band);
            facts.push((format!("clearance: {} shaft diameter", m.who), d + 2.0 * rad, Unit::Mm));
        } else if integral {
            m.shaft = Some((zr.0, zr.1, integral_d / 2.0 + rad));
        }
    }

    // A chain or belt between two wheels, and round a single one.
    chain_run(s, c, &members, preview, &mut groups, &mut prisms, facts);

    // Bearing seats.
    if let Some((name, b)) = &bearing {
        facts.push((format!("clearance bearing {name} bore"), b.bore, Unit::Mm));
        facts.push((format!("clearance bearing {name} outside diameter"), b.od, Unit::Mm));
        facts.push((format!("clearance bearing {name} width"), b.width, Unit::Mm));
        let r = (b.od + c.bearing_fit) / 2.0;
        let mut mismatched = Vec::new();
        for (i, seat) in c.seats.iter().enumerate() {
            let n = i + 1;
            let (mi, along) = match seat {
                Seat::Along { member, along } => {
                    let k = *member as usize;
                    if k == 0 || k > members.len() {
                        let list: Vec<String> = members.iter().enumerate().map(|(j, m)| format!("{} {}", j + 1, m.who)).collect();
                        return Err(format!("clearance: bearing seat {n} is on member {k}, and this set has {}: {}", members.len(), list.join(", ")));
                    }
                    (k - 1, *along)
                }
                Seat::Point(pt) => match nearest(&members, *pt) {
                    Some((k, along, off)) => {
                        if off > b.od {
                            warn.push(format!(
                                "clearance: bearing seat {n}'s point is {} mm off {}'s axis; it is moved onto the axis",
                                num(off),
                                members[k].who
                            ));
                        }
                        (k, along)
                    }
                    None => {
                        warn.push(format!("clearance: bearing seat {n}: no member turns on an axis of its own to put it on; left out"));
                        continue;
                    }
                },
            };
            let m = &members[mi];
            let Some(gi) = m.group else {
                warn.push(format!(
                    "clearance: bearing seat {n} is on {}, which has no axis in the housing (a slide or a ring left to the housing); left out",
                    m.who
                ));
                continue;
            };
            if m.planet {
                warn.push(format!(
                    "clearance: bearing seat {n} is on {}, which goes round with the carrier: its seat is not part of the housing; left out",
                    m.who
                ));
                continue;
            }
            let g = &mut groups[gi];
            // The dot, along the member's own axis from the middle of its teeth, in the group's
            // coordinates (the disc of a drive sits on the input's axis).
            let f = m.body.frame;
            let centre = dot(sub(f.world([0.0, 0.0, m.mid + along]), g.frame.origin), g.frame.z);
            let (z0, z1) = (centre - b.width / 2.0, centre + b.width / 2.0);
            // (From the middle of the teeth, which moves by half the face's change.)
            if let Some(rule) = rules.get_mut(gi) {
                rule.anchors.extend([(z0, 0.5), (z1, 0.5)]);
            }
            g.bands.push((z0, z1, r));
            placed.push(PlacedBearing {
                seat: n,
                member: mi,
                frame: Frame { origin: f.world([0.0, 0.0, m.mid + along]), x: f.x, z: f.z },
                name: name.clone(),
                size: *b,
            });
            facts.push((format!("clearance: bearing seat {n} on {} at", m.who), along, Unit::Mm));
            match m.shaft {
                Some((lo, hi, _)) if z0 >= lo - 1e-9 && z1 <= hi + 1e-9 => {}
                Some((lo, hi, _)) => warn.push(format!(
                    "clearance: bearing seat {n} ({}…{} mm along {}'s axis) is beyond its shaft ({}…{})",
                    num(z0 - centre + along),
                    num(z1 - centre + along),
                    m.who,
                    num(lo - centre + along),
                    num(hi - centre + along)
                )),
                None => {
                    warn.push(format!("clearance: bearing seat {n} is on {}, which has no shaft (no bore, integral shaft or bearing bore)", m.who))
                }
            }
            let (zlo, zhi) = (
                dot(sub(f.world([0.0, 0.0, m.teeth.0]), g.frame.origin), g.frame.z),
                dot(sub(f.world([0.0, 0.0, m.teeth.1]), g.frame.origin), g.frame.z),
            );
            let (zlo, zhi) = (zlo.min(zhi), zlo.max(zhi));
            if z1 > zlo + 1e-9 && z0 < zhi - 1e-9 {
                warn.push(format!(
                    "clearance: bearing seat {n} overlaps the teeth of {} (they run {}…{} mm from their middle)",
                    m.who,
                    num(m.teeth.0 - m.mid),
                    num(m.teeth.1 - m.mid)
                ));
            }
            if b.bore > 0.0
                && let Some(d) = m.shaft_d
                && (d - b.bore).abs() > 1e-6
                && !mismatched.contains(&mi)
            {
                mismatched.push(mi);
                warn.push(format!("clearance: bearing {name} has a {} mm bore but {}'s shaft is Ø{}", num(b.bore), m.who, num(d)));
            }
        }
    }

    // The body: one revolve per axis (each connected stretch of its profile), the prisms.
    let mut steps = Vec::new();
    let mut follow = Vec::new();
    for (gi, g) in groups.iter().enumerate() {
        let reach = ax + g.hz;
        for poly in profile(g) {
            let op = if steps.is_empty() { Op::New } else { Op::Join };
            let rule = rules.get(gi).filter(|r| !r.tips.is_empty());
            follow.push(rule.map(|r| poly.iter().map(|q| r.vertex(*q, reach)).collect()).unwrap_or_default());
            steps.push(Step::Placed { frame: g.frame, step: Box::new(Step::Revolve { profile: poly, op }) });
        }
    }
    for (frame, region, z0, height) in prisms {
        let op = if steps.is_empty() { Op::New } else { Op::Join };
        follow.push(Vec::new());
        steps.push(Step::Placed { frame, step: Box::new(Step::Prism { region, z0, height, twist: 0.0, op }) });
    }
    if steps.is_empty() {
        return Err("clearance: nothing to clear (every member is a ring left to the housing)".into());
    }
    facts.push(("clearance: radial".into(), rad, Unit::Mm));
    facts.push(("clearance: axial".into(), ax, Unit::Mm));
    Ok((Body { name: format!("{} clearance", set_name(s)), frame: Frame::at([0.0; 3]), steps }, follow))
}

/// The member whose axis is nearest a world point: its index, the point's distance along the
/// axis from the middle of its teeth, and how far off the axis it is.
fn nearest(members: &[Member], p: [f64; 3]) -> Option<(usize, f64, f64)> {
    members
        .iter()
        .enumerate()
        .filter(|(_, m)| m.group.is_some() && !m.planet)
        .map(|(k, m)| {
            let f = m.body.frame;
            let d = sub(p, f.origin);
            let along = dot(d, f.z);
            let off = len(sub(d, mul(f.z, along)));
            (k, along - m.mid, off)
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
}

/// Sprockets and pulleys: the chain or belt round a single wheel (a band on its axis), and
/// between two (a prism round both wheels and the runs).
fn chain_run(
    s: &Spec,
    c: &Clearance,
    members: &[Member],
    preview: &[PreviewGear],
    groups: &mut [Group],
    prisms: &mut Vec<(Frame, Region, f64, f64)>,
    facts: &mut Vec<(String, f64, Unit)>,
) {
    if !matches!(s.kind, Kind::Sprocket | Kind::Pulley) {
        return;
    }
    let chain = s.kind == Kind::Sprocket;
    let radius = |names: &[&str]| preview.iter().find(|g| names.contains(&g.name.as_str())).map(|g| g.pitch_radius).filter(|r| *r > 0.0);
    let (w1, w2): (&[&str], &[&str]) = if chain { (&["sprocket", "sprocket 1"], &["sprocket 2"]) } else { (&["pulley", "pulley 1"], &["pulley 2"]) };
    let wheels: Vec<(usize, f64, u32)> = [(0usize, radius(w1), s.teeth), (1, radius(w2), s.teeth2)]
        .into_iter()
        .filter_map(|(i, r, z)| Some((i, r?, z)))
        .filter(|(i, _, z)| *i < members.len() && *z > 0)
        .collect();
    // How far the chain's plates or the belt's back stand out past the pitch circle, and how
    // much wider than the teeth the chain is on each side.
    let pitch = |r: f64, z: u32| if chain { 2.0 * r * (std::f64::consts::PI / f64::from(z)).sin() } else { std::f64::consts::TAU * r / f64::from(z) };
    let Some(&(_, r1, z1)) = wheels.first() else { return };
    let p = pitch(r1, z1);
    let (out, side) = if chain { (0.5 * p, 0.4 * p) } else { (0.6 * p, 0.0) };
    let off = out + c.radial;
    for (i, r, _) in &wheels {
        let m = &members[*i];
        let Some(gi) = m.group else { continue };
        let g = &mut groups[gi];
        let f = m.body.frame;
        let (a, b) =
            (dot(sub(f.world([0.0, 0.0, m.z.0]), g.frame.origin), g.frame.z), dot(sub(f.world([0.0, 0.0, m.z.1]), g.frame.origin), g.frame.z));
        g.bands.push((a.min(b) - side - c.axial, a.max(b) + side + c.axial, r + off));
    }
    if wheels.len() == 2 {
        let (m1, m2) = (&members[wheels[0].0], &members[wheels[1].0]);
        let f1 = m1.body.frame;
        let d = sub(m2.body.frame.origin, f1.origin);
        let across = sub(d, mul(f1.z, dot(d, f1.z)));
        let a = len(across);
        let r2 = wheels[1].1;
        if a > (r1 - r2).abs() + 1e-6
            && let Some(path) = crate::wrap::belt_path(r1, r2, a, off)
        {
            let x = mul(across, 1.0 / a);
            let frame = Frame { origin: f1.origin, x, z: f1.z };
            let zs = [m1, m2]
                .iter()
                .flat_map(|m| [m.z.0, m.z.1].map(|z| dot(sub(m.body.frame.world([0.0, 0.0, z]), f1.origin), f1.z)))
                .fold((f64::MAX, f64::MIN), |a, z| (a.0.min(z), a.1.max(z)));
            let (z0, z1) = (zs.0 - side - c.axial, zs.1 + side + c.axial);
            prisms.push((frame, Region { outer: vec![path], holes: vec![] }, z0, z1 - z0));
            let what = if chain { "chain" } else { "belt" };
            facts.push((format!("clearance: {what} run width"), z1 - z0, Unit::Mm));
            facts.push((format!("clearance: {what} run height over the pitch line"), off, Unit::Mm));
        }
    }
}

/// The revolved profiles of an axis: closed polygons in its (radius, z) half plane, one for
/// each stretch along the axis that needs clearing.
fn profile(g: &Group) -> Vec<Vec<V2>> {
    // The points' needs, cell by cell and window by window: a cell takes the largest radius
    // (grown by the margin) of its window's points within a row of it, and only cells that
    // overlap the window; between cells the need is linear, and it ends at the window's ends.
    const CELL: f64 = 0.02;
    let (mut p0, mut p1) = (f64::MAX, f64::MIN);
    for (lo, hi) in &g.wins {
        p0 = p0.min(*lo);
        p1 = p1.max(*hi);
    }
    let have_pts = !g.pts.is_empty() && p1 > p0;
    let (n, dz) = if have_pts {
        let span = p1 - p0;
        let n = ((span / CELL).ceil() as usize).clamp(8, 20_000);
        (n, span / n as f64)
    } else {
        (0, 1.0)
    };
    let nw = if have_pts { g.wins.len() } else { 0 };
    let mut cells = vec![vec![0.0f64; n]; nw];
    if have_pts {
        let mut own = vec![vec![0.0f64; n]; nw];
        let mut reach = vec![0.0f64; nw];
        for q in &g.pts {
            let k = (((q.z - p0) / dz).floor() as isize).clamp(0, n as isize - 1) as usize;
            own[q.win][k] = own[q.win][k].max(q.r + q.margin);
            reach[q.win] = reach[q.win].max(q.reach);
        }
        for (w, (lo, hi)) in g.wins.iter().enumerate() {
            // (A point is somewhere in its cell: that many cells either way hold all within reach.)
            let spread = (reach[w] / dz).ceil() as usize;
            // The cells the window overlaps.
            let k0 = (((lo - p0) / dz).floor().max(0.0) as usize).min(n - 1);
            let k1 = (((hi - p0) / dz).ceil().max(1.0) as usize).min(n);
            let cell = &mut cells[w];
            for (k, v) in own[w].iter().enumerate() {
                if *v <= 0.0 {
                    continue;
                }
                for c in cell.iter_mut().take((k + spread + 1).min(k1)).skip(k.saturating_sub(spread).max(k0)) {
                    *c = c.max(*v);
                }
            }
        }
    }
    // A window's need at a cell boundary: the larger of the cells either side.
    let bval = |w: usize, k: usize| -> f64 {
        let l = if k > 0 { cells[w][k - 1] } else { 0.0 };
        let r = if k < n { cells[w][k] } else { 0.0 };
        l.max(r)
    };
    // Its need at z, just left or right of it: nothing outside the window.
    let fpw = |w: usize, z: f64, right: bool| -> f64 {
        let (lo, hi) = g.wins[w];
        let inside = if right { z >= lo - 1e-12 && z < hi - 1e-12 } else { z > lo + 1e-12 && z <= hi + 1e-12 };
        if !inside {
            return 0.0;
        }
        let t = ((z - p0) / dz).clamp(0.0, n as f64);
        let k = (t.floor() as usize).min(n - 1);
        let f = t - k as f64;
        bval(w, k) * (1.0 - f) + bval(w, k + 1) * f
    };
    let fp = |z: f64, right: bool| -> f64 { (0..nw).map(|w| fpw(w, z, right)).fold(0.0, f64::max) };
    let band = |z: f64, right: bool| -> f64 {
        g.bands.iter().filter(|(z0, z1, _)| if right { *z0 <= z && z < *z1 } else { *z0 < z && z <= *z1 }).fold(0.0, |a, b| a.max(b.2))
    };
    let mut at: Vec<f64> = if have_pts { (0..=n).map(|k| p0 + dz * k as f64).collect() } else { Vec::new() };
    if have_pts {
        at[n] = p1;
        for (lo, hi) in &g.wins {
            at.push(*lo);
            at.push(*hi);
        }
    }
    for (z0, z1, r) in &g.bands {
        if *r > 0.0 && z1 > z0 {
            at.push(*z0);
            at.push(*z1);
        }
    }
    at.sort_by(f64::total_cmp);
    at.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    // Walk the breakpoints: vertices where the need is, a vertical step where it jumps.
    let mut out = Vec::new();
    let mut cur: Vec<V2> = Vec::new();
    for &z in &at {
        let l = fp(z, false).max(band(z, false));
        let r = fp(z, true).max(band(z, true));
        let (l, r) = if (l - r).abs() < 1e-4 { (l.max(r), l.max(r)) } else { (l, r) };
        if l > 0.0 {
            cur.push(V2::new(l, z));
        }
        if r <= 0.0 {
            if !cur.is_empty() {
                out.push(close(simplify(std::mem::take(&mut cur))));
            }
        } else if (r - l).abs() > 0.0 || l <= 0.0 {
            cur.push(V2::new(r, z));
        }
    }
    if !cur.is_empty() {
        out.push(close(simplify(cur)));
    }
    out
}

/// The tolerance a profile's simplified chords may fall under a vertex (the profile is then
/// moved out by it), and how far they may stand over one.
const UNDER: f64 = 0.005;
const OVER: f64 = 0.02;

/// Fewer vertices: on each stretch between vertical steps, a chord replaces the vertices it
/// passes within (`-UNDER`, `+OVER`) of; then every radius is moved out by `UNDER`.
fn simplify(v: Vec<V2>) -> Vec<V2> {
    let mut out: Vec<V2> = Vec::new();
    let mut i = 0;
    while i < v.len() {
        // A run of strictly rising z from i.
        let mut j = i;
        while j + 1 < v.len() && v[j + 1].y > v[j].y + 1e-12 {
            j += 1;
        }
        let run = &v[i..=j];
        let mut a = 0;
        out.push(run[0]);
        while a + 1 < run.len() {
            let (mut lo, mut hi) = (f64::MIN, f64::MAX);
            let mut best = a + 1;
            for k in a + 1..run.len() {
                let dzk = run[k].y - run[a].y;
                let s = (run[k].x - run[a].x) / dzk;
                if s >= lo - 1e-12 && s <= hi + 1e-12 {
                    best = k;
                }
                lo = lo.max((run[k].x - UNDER - run[a].x) / dzk);
                hi = hi.min((run[k].x + OVER - run[a].x) / dzk);
                if lo > hi {
                    break;
                }
            }
            out.push(run[best]);
            a = best;
        }
        i = j + 1;
    }
    for q in &mut out {
        q.x += UNDER;
    }
    out
}

/// The profile closed to the axis at both ends.
fn close(v: Vec<V2>) -> Vec<V2> {
    let (z0, z1) = (v[0].y, v[v.len() - 1].y);
    let mut out = Vec::with_capacity(v.len() + 2);
    out.push(V2::new(0.0, z0));
    out.extend(v);
    out.push(V2::new(0.0, z1));
    // Round off what the kernel would see as noise.
    for q in &mut out {
        q.x = (q.x * 1e6).round() / 1e6;
        q.y = (q.y * 1e6).round() / 1e6;
    }
    out.dedup_by(|a, b| (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6);
    out
}

/// Builds each placed bearing ([`Plan::bearings`]) as two bodies, its outer ring and its inner
/// ring (plain rectangular sections, each a third of the bearing's wall; no balls), and adds
/// them to the assembly: the outer ring held (it sits in the housing), the inner ring joined
/// rigidly to the member whose shaft it carries. Only for the whole set (`Spec::only` 0).
pub(crate) fn add_bearing_bodies(s: &Spec, p: &mut Plan) {
    if p.bearings.is_empty() {
        p.warn("bearing bodies: no bearing seats placed, so no bearings are built");
        return;
    }
    if s.only != 0 {
        p.warn("bearing bodies are built with the whole set only");
        return;
    }
    let pre = if s.name.trim().is_empty() { String::new() } else { format!("{} ", s.name.trim()) };
    for b in p.bearings.clone() {
        let (ri, ro, w) = (b.size.bore.max(0.0) / 2.0, b.size.od / 2.0, b.size.width);
        // A bore not known: an inner ring at 0.4 of the outside radius.
        let ri = if ri > 0.0 { ri } else { 0.4 * ro };
        let t = (ro - ri) / 3.0;
        let ring = |r0: f64, r1: f64| Step::Revolve {
            profile: vec![V2::new(r0, -w / 2.0), V2::new(r1, -w / 2.0), V2::new(r1, w / 2.0), V2::new(r0, w / 2.0)],
            op: Op::New,
        };
        let axis = crate::axes::Axis { origin: b.frame.origin, dir: b.frame.z };
        for (part, r0, r1) in [("outer", ro - t, ro), ("inner", ri, ri + t)] {
            let index = p.bodies.len();
            p.bodies.push(Body { name: format!("{pre}Bearing {} {} {part} ring", b.seat, b.name), frame: b.frame, steps: vec![ring(r0, r1)] });
            let grounded = part == "outer";
            p.assembly.members.push(crate::plan::AssemblyMember { body: index, axis, grounded, carrier: None });
            if !grounded && b.member < p.assembly.members.len() {
                let m = p.assembly.members.len() - 1;
                p.assembly.links.push(crate::plan::AssemblyLink::Rigid { a: b.member, b: m });
            }
        }
    }
}
