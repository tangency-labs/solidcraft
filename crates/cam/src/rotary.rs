//! 4-axis rotary machining: paths wrapped around a rotary axis ([`wrap`]), and parts turned on
//! it while the tool finishes ([`rotary_finish`]) or roughs them from round stock
//! ([`rotary_roughing`]).
//!
//! The convention, which machine kinematics rely on: the rotary axis is work X through the
//! origin, and θ = 0 is the tool coming from +Z. A pose at `x`, angle θ and radius ρ has its
//! tip at `(x, ρ·sin θ, ρ·cos θ)` and its tool axis radial, `(0, sin θ, cos θ)` ([`radial`]),
//! pointing out from the rotary axis through the tip: positive θ turns the tool from +Z towards
//! +Y. That is the angle a rotary table about +X is written at to bring that side of the work
//! up under a vertical tool ([`crate::kinematics::Machine::four_axis_a`]: `A = θ`), so a flat
//! path's arc length `y` round a cylinder of radius R is written as `A = y / R`. Angles along a
//! path keep accumulating past a full turn (a helix goes on round), the poses being geometry
//! only.
//!
//! The paths suit controllers without tool-centre-point control: no cutting move turns the tool
//! axis by more than a set angle step (2° unless given), so every cut is short in angle, and
//! the tip moves on a straight line between poses that are all exactly on the wrapped path or
//! the surface. Rapids go round at the clearance radius on the polygon whose sides touch that
//! circle, so a rapid is never closer to the axis than the clearance radius.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::cutter::Cutter;
use crate::dropcutter::drop_along;
use crate::multiaxis::{Move5, Pose, Toolpath5, verify5};
use crate::ops::Feeds;
use crate::path::{MAX_MOVES, Move, Toolpath};
use crate::surface::Surface;
use crate::surface_ops::{
    Budget, MAX_SAMPLES, Pass, Profile, Tip, check_leave, check_step, check_stepdown, check_tolerance, depth_for, descent_feed, levels, spans,
    tool_cutter,
};
use crate::tool::Tool;
use crate::{CamError, Result, invalid};

/// The largest turn of the tool axis in one cutting move unless one is given: 2°.
pub const DEFAULT_MAX_ANGLE_STEP: f64 = PI / 90.0;
/// Largest radius or coordinate (mm).
const MAX_COORD: f64 = 1.0e6;
/// Most samples along one link checked for clearance.
const MAX_LINK_SAMPLES: usize = 10_000;
/// How much bigger than the tool (on top of the stock to leave) the cutter is dropped as, so
/// that a face it only touches at a tangency (a ball whose side is exactly its radius from the
/// part's end face, on a range that starts a tool radius beyond the free end) is touched at
/// every angle round rather than found at some and missed at others by rounding, which made the
/// tool dive towards the axis and back thousands of times (mm; the tool runs this much further
/// out).
const TANGENCY: f64 = 1e-10;
/// Most profile samples walked over all levels (levels times samples): beyond it the operation
/// is refused rather than left running.
const MAX_LEVEL_SAMPLES: usize = 400_000_000;

fn default_angle_step() -> f64 {
    DEFAULT_MAX_ANGLE_STEP
}

/// The unit direction out from the rotary axis at angle `theta`: `(0, sin θ, cos θ)`.
pub fn radial(theta: f64) -> Vec3 {
    let (s, c) = theta.sin_cos();
    Vec3::new(0.0, s, c)
}

/// The pose at `x` along the rotary axis, angle `theta` and radius `rho`, its axis radial.
pub fn rotary_pose(x: f64, theta: f64, rho: f64) -> Pose {
    let u = radial(theta);
    Pose::new(Vec3::new(x, 0.0, 0.0) + u * rho, u)
}

fn check_angle_step(step: f64) -> Result<()> {
    if step.is_finite() && (1e-4..=PI / 4.0).contains(&step) { Ok(()) } else { invalid("the angle step must be from 0.0001 radians to 45 degrees") }
}

/// [`wrap_with_step`] with the axis turning at most [`DEFAULT_MAX_ANGLE_STEP`] per move.
pub fn wrap(path: &Toolpath, radius: f64, tolerance: f64) -> Result<Toolpath5> {
    wrap_with_step(path, radius, tolerance, DEFAULT_MAX_ANGLE_STEP)
}

/// A flat 3-axis path drawn on the unrolled cylinder of `radius` wrapped round the rotary axis:
/// x stays along the axis, y is the arc length round it (angle θ = y / radius), and z is the
/// depth from the cylinder's surface (0 on it, negative into the material), so the tip is at
/// radius `radius + z`, with the tool axis radial. Every move is split so that the tip's
/// straight moves stay within `tolerance` of the true wrapped curve (a helix or a circle) and
/// the axis turns at most `max_angle_step` per move; arcs are followed within the tolerance.
/// Rapids are split the same way but go round outside their radius (see the module notes), so
/// a rapid at a clearance radius never comes inside it. Feeds are kept as they are.
pub fn wrap_with_step(path: &Toolpath, radius: f64, tolerance: f64, max_angle_step: f64) -> Result<Toolpath5> {
    if !(radius > 0.0) || !(radius <= MAX_COORD) {
        return invalid("the wrapping radius must be above 0 and at most 1000 m");
    }
    check_tolerance(tolerance)?;
    check_angle_step(max_angle_step)?;
    if path.moves.len() > MAX_MOVES {
        return Err(CamError::TooLarge(format!("the toolpath has more than {MAX_MOVES} moves")));
    }
    let w = Wrapper { radius, tol: tolerance, max_step: max_angle_step };
    let mut out = Vec::new();
    let mut pos: Option<Vec3> = None;
    for m in &path.moves {
        match *m {
            Move::Rapid { to } => match pos {
                Some(from) => w.segment(from, to, None, &mut out)?,
                None => out.push(Move5::Rapid { to: w.pose(to, 0.0)? }),
            },
            Move::Feed { to, feed } => match pos {
                Some(from) => w.segment(from, to, Some(feed), &mut out)?,
                None => out.push(Move5::Feed { to: w.pose(to, 0.0)?, feed }),
            },
            Move::Arc { feed, .. } | Move::PlaneArc { feed, .. } => {
                let Some(from) = pos else { return invalid("a toolpath must not start with an arc") };
                // The arc as flat points, close enough that wrapping keeps it within half the
                // tolerance; then each piece wrapped within the other half.
                let reach = m.target().map_or(radius, |t| radius + t.z.max(from.z).max(0.0));
                let flat_tol = 0.5 * tolerance * (radius / reach).min(1.0);
                let one = Toolpath { moves: vec![Move::Rapid { to: from }, *m] };
                let half = Wrapper { tol: 0.5 * tolerance, ..w };
                let mut at = from;
                for (p, _) in one.points(flat_tol).into_iter().skip(1) {
                    half.segment(at, p, Some(feed), &mut out)?;
                    at = p;
                }
            }
            Move::Dwell { seconds } => out.push(Move5::Dwell { seconds }),
        }
        if let Some(to) = m.target() {
            pos = Some(to);
        }
        if out.len() > MAX_MOVES {
            return Err(CamError::TooLarge(format!("the wrapped toolpath has more than {MAX_MOVES} moves")));
        }
    }
    let path = Toolpath5 { moves: out };
    verify5(&path, max_angle_step)?;
    Ok(path)
}

struct Wrapper {
    radius: f64,
    tol: f64,
    max_step: f64,
}

impl Wrapper {
    /// The pose of the flat point `p`, its radius grown by the fraction `grow` (0 on the path).
    fn pose(&self, p: Vec3, grow: f64) -> Result<Pose> {
        if !p.is_finite() {
            return invalid("a toolpath coordinate is not a finite number");
        }
        let rho = self.radius + p.z;
        if !(rho > 0.0) {
            return invalid("a point of the toolpath is at or past the rotary axis");
        }
        Ok(rotary_pose(p.x, p.y / self.radius, rho * (1.0 + grow)))
    }

    /// The flat straight move from `a` to `b`, wrapped: a cut at `feed`, or a rapid.
    fn segment(&self, a: Vec3, b: Vec3, feed: Option<f64>, out: &mut Vec<Move5>) -> Result<()> {
        if !a.is_finite() || !b.is_finite() {
            return invalid("a toolpath coordinate is not a finite number");
        }
        let dth = (b.y - a.y) / self.radius;
        let drho = b.z - a.z;
        let rho_max = self.radius + a.z.max(b.z);
        // The tip's curve x(t), ρ(t), θ(t), all linear in t, bends by at most
        // 2|Δρ||Δθ| + ρ·Δθ² over the move; a chord over 1/n of it is off by an eighth of that / n².
        let bend = 2.0 * drho.abs() * dth.abs() + rho_max.max(0.0) * dth * dth;
        let n = (bend / (8.0 * self.tol)).sqrt().ceil().max((dth.abs() / self.max_step).ceil()).max(1.0);
        if !(n <= (MAX_MOVES - out.len().min(MAX_MOVES)) as f64) {
            return Err(CamError::TooLarge(format!("the wrapped toolpath has more than {MAX_MOVES} moves")));
        }
        let n = n as usize;
        match feed {
            Some(feed) => {
                for k in 1..=n {
                    out.push(Move5::Feed { to: self.pose(a.lerp(b, k as f64 / n as f64), 0.0)?, feed });
                }
            }
            None => {
                if dth.abs() > 1e-12 {
                    // Corners of the polygon whose sides touch the circle at the even steps.
                    let half = 0.5 * dth / n as f64;
                    let grow = 1.0 / half.cos() - 1.0;
                    for k in 0..n {
                        out.push(Move5::Rapid { to: self.pose(a.lerp(b, (2 * k + 1) as f64 / (2 * n) as f64), grow)? });
                    }
                }
                out.push(Move5::Rapid { to: self.pose(b, 0.0)? });
            }
        }
        Ok(())
    }
}

/// How the passes of a rotary operation run.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RotaryPattern {
    /// Passes along the axis, one every `angle_step` radians round (at most that, evenly over
    /// a full turn); every other one runs back when `zigzag`.
    Lines { angle_step: f64, zigzag: bool },
    /// One pass going round and round, advancing `pitch` mm along +X per turn.
    Helix { pitch: f64 },
    /// Circumferential passes: a full turn of A at each X, the rings at most `stepover` mm
    /// apart along X (evenly over the range, both ends included). With `zigzag` every other ring
    /// turns back, so A stays within one turn; without it every ring turns the same way (one
    /// cutting direction) and A winds on, a turn per ring.
    Rings { stepover: f64, zigzag: bool },
}

/// Finishing a part turned on the rotary axis: the tool, radial, follows the surface.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RotaryParams {
    pub pattern: RotaryPattern,
    /// Radii from the rotary axis (mm): rapids go round at `clearance`, and the tool comes in
    /// from `retract` at the plunge feed. The retract radius must clear the part.
    pub clearance: f64,
    pub retract: f64,
    /// The smallest radius the tip may reach (mm): where the surface is closer to the axis the
    /// tool runs at it.
    pub floor: f64,
    pub feeds: Feeds,
    /// From where to where along X the passes run (mm); left out, the surface's extent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<[f64; 2]>,
    /// Distance between samples along a pass before refinement (mm).
    pub step: f64,
    /// Largest distance between the path and where the cutter rests on the surface (mm).
    pub tolerance: f64,
    /// Material left on the surface (mm).
    pub stock_to_leave: f64,
    /// Largest turn of the tool axis in one cutting move (radians).
    #[serde(default = "default_angle_step")]
    pub max_angle_step: f64,
}

/// Roughing round stock on the rotary axis down to the part, in levels of radius.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RotaryRoughingParams {
    pub pattern: RotaryPattern,
    /// The stock's radius (mm): the first level is at most a stepdown inside it.
    pub stock_radius: f64,
    /// Radii from the rotary axis (mm), as for [`RotaryParams`]; both at or outside the stock.
    pub clearance: f64,
    pub retract: f64,
    /// The last level's radius (mm).
    pub floor: f64,
    /// Most material taken per level, in radius (mm).
    pub stepdown: f64,
    pub feeds: Feeds,
    /// From where to where along X the stock is roughed (mm); left out, the surface's extent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<[f64; 2]>,
    /// Distance between samples along a pass before refinement (mm).
    pub step: f64,
    /// Largest distance between the path and where the cutter rests on the surface (mm).
    pub tolerance: f64,
    /// Material left on the part (mm).
    pub stock_to_leave: f64,
    /// Largest turn of the tool axis in one cutting move (radians).
    #[serde(default = "default_angle_step")]
    pub max_angle_step: f64,
}

/// What both rotary operations share, checked.
struct Setup {
    pattern: RotaryPattern,
    lo: f64,
    hi: f64,
    clearance: f64,
    retract: f64,
    feeds: Feeds,
    step: f64,
    tol: f64,
    max_step: f64,
}

/// The surface seen by a radial cutter.
struct Drum<'a> {
    surface: &'a Surface,
    /// The cutter grown by the stock to leave.
    cutter: Cutter,
    leave: f64,
    budget: Budget,
    /// With rings: the cutter shrunk by the part's tessellation error (a quarter of the
    /// tolerance) and how far its tip is up its axis from the cutter's: see [`Drum::at`].
    steady: Option<(Cutter, f64)>,
    tol: f64,
}

impl Drum<'_> {
    /// Where the tool's tip rests coming in radially at `x` and `theta`.
    ///
    /// Rings hold X for a whole turn, so a ring where the cutter meets the part only at a steep
    /// tangency (a ball whose side just reaches a wall it runs beside, as a ring beside a
    /// shoulder at the ball's radius from it) would be held up by the facets of the part's
    /// mesh, each within its tessellation error of the true surface, a millimetre or more
    /// higher at one angle than the next, and dip in and out all the way round. With rings the
    /// tip rests where the cutter shrunk by that error does, when that is more than the
    /// tolerance lower: into the mesh by at most that error, the depth the gouge check allows
    /// for the tessellation. Elsewhere the two differ by about that error, and the cutter's own
    /// rest is kept.
    fn at(&self, x: f64, theta: f64) -> Result<Tip> {
        self.budget.spend()?;
        let (o, dir) = (Vec3::new(x, 0.0, 0.0), radial(theta));
        let Some(d) = drop_along(self.surface, &self.cutter, o, dir)? else { return Ok(Tip::Clear) };
        if let Some((shrunk, raise)) = &self.steady
            && let Some(s) = drop_along(self.surface, shrunk, o, dir)?
            && d - (s - raise) > self.tol
        {
            return Ok(Tip::At(s - raise + self.leave));
        }
        Ok(Tip::At(d + self.leave))
    }

    /// The farthest the part reaches from the axis within reach of `lo..=hi` along it, the
    /// stock to leave included.
    fn top(&self, lo: f64, hi: f64) -> Option<f64> {
        let reach = self.cutter.radius;
        let mut top = f64::NEG_INFINITY;
        self.surface.visit(
            |b| b.max.x >= lo - reach && b.min.x <= hi + reach,
            |_, t| {
                if t.iter().any(|p| p.x >= lo - reach) && t.iter().any(|p| p.x <= hi + reach) {
                    for p in t {
                        top = top.max(p.y.hypot(p.z));
                    }
                }
            },
        );
        top.is_finite().then_some(top + self.leave)
    }
}

/// How a pass's parameter maps to a place: along X at a fixed angle, round a ring at a fixed X,
/// or round a helix.
#[derive(Clone, Copy, Debug)]
enum Way {
    Line(f64),
    /// Round at `x`, the parameter the angle.
    Ring(f64),
    /// At angle `t`, x is `x0 + pitch·(t − lead)/2π` held within `lo..=hi`: with a lead of a
    /// turn the tool goes a full turn round at `lo` before the helix and at `hi` after it.
    Helix {
        x0: f64,
        pitch: f64,
        lead: f64,
        lo: f64,
        hi: f64,
    },
}

impl Way {
    /// `(x, θ)` at parameter `t` (x along a line, the angle round a ring or a helix).
    fn at(self, t: f64) -> (f64, f64) {
        match self {
            Way::Line(theta) => (t, theta),
            Way::Ring(x) => (x, t),
            Way::Helix { x0, pitch, lead, lo, hi } => ((x0 + pitch * (t - lead) / TAU).clamp(lo, hi), t),
        }
    }
}

struct RPass {
    way: Way,
    profile: Profile,
    backward: bool,
    /// The lowest radius the cutter rests at along the profile, `-∞` where nothing is under it
    /// somewhere: at a level whose material above is no higher than this nothing is cut.
    lowest: f64,
}

impl RPass {
    fn new(way: Way, profile: Profile, backward: bool) -> RPass {
        let lowest = profile.iter().fold(f64::INFINITY, |m, (_, tip)| match tip {
            Tip::At(h) if !h.is_nan() => m.min(*h),
            _ => f64::NEG_INFINITY,
        });
        RPass { way, profile, backward, lowest }
    }
}

fn setup_check(
    pattern: RotaryPattern,
    range: Option<[f64; 2]>,
    clearance: f64,
    retract: f64,
    feeds: &Feeds,
    step: f64,
    tol: f64,
    leave: f64,
    max_step: f64,
    surface: &Surface,
) -> Result<Setup> {
    feeds.validate()?;
    check_step(step)?;
    check_tolerance(tol)?;
    check_leave(leave)?;
    check_angle_step(max_step)?;
    if !(clearance.is_finite() && retract > 0.0 && clearance >= retract && clearance <= MAX_COORD) {
        return invalid("the clearance and retract radii must be ordered: clearance ≥ retract > 0");
    }
    let [lo, hi] = match range {
        Some(r) => r,
        None => {
            let b = surface.bounds();
            if b.is_empty() {
                return invalid("the surface has no triangles: give the range along X");
            }
            [b.min.x, b.max.x]
        }
    };
    if !(lo.is_finite() && hi.is_finite() && lo.abs() <= MAX_COORD && hi.abs() <= MAX_COORD && lo < hi) {
        return invalid("the range along X must be two finite numbers, the first below the second");
    }
    match pattern {
        RotaryPattern::Lines { angle_step, .. } => {
            if !(angle_step.is_finite() && (1e-4..=TAU).contains(&angle_step)) {
                return invalid("the angle between passes must be from 0.0001 radians to a full turn");
            }
        }
        RotaryPattern::Helix { pitch } => {
            if !(1e-4..=MAX_COORD).contains(&pitch) {
                return invalid("the helix pitch must be above 0");
            }
        }
        RotaryPattern::Rings { stepover, .. } => {
            if !(1e-4..=MAX_COORD).contains(&stepover) {
                return invalid("the stepover between rings must be above 0");
            }
        }
    }
    Ok(Setup { pattern, lo, hi, clearance, retract, feeds: *feeds, step, tol, max_step })
}

/// Refuses roughing passes farther apart than the tool is wide, which would leave ridges of
/// stock at full height between them: lines farther apart at the stock radius (the straight
/// distance between neighbours, the whole stock's width for two lines, and never cleared for
/// one), or a helix of a larger pitch, or rings farther apart along X.
fn check_rough_stepover(pattern: RotaryPattern, stock_radius: f64, tool: &Tool) -> Result<()> {
    let stepover = match pattern {
        RotaryPattern::Lines { angle_step, .. } => {
            let lines = (TAU / angle_step - 1e-9).ceil().max(1.0);
            if lines < 2.0 {
                return invalid("the stepover must be above 0 and at most the tool diameter: one pass round never clears the far side of the stock");
            }
            2.0 * stock_radius * (PI / lines).min(FRAC_PI_2).sin()
        }
        RotaryPattern::Helix { pitch } => pitch,
        RotaryPattern::Rings { stepover, .. } => stepover,
    };
    if stepover <= tool.diameter {
        Ok(())
    } else {
        invalid(format!(
            "the stepover must be above 0 and at most the tool diameter: the passes are {stepover:.3} mm apart at the stock radius, the tool {} mm wide",
            tool.diameter
        ))
    }
}

impl Setup {
    /// How far a pass may stray from where the cutter rests between its samples (the chord of
    /// two samples from the rest between them, a link from the rest under it): within the
    /// tolerance less the part's tessellation error (a quarter of it, which the gouge check
    /// allows for the mesh), less with rings the depth the steadied rest may go into the mesh
    /// (another quarter), so that the pass is never further into the mesh than the gouge check
    /// allows.
    fn stray(&self) -> f64 {
        match self.pattern {
            RotaryPattern::Rings { .. } => 0.5 * self.tol,
            RotaryPattern::Lines { .. } | RotaryPattern::Helix { .. } => 0.75 * self.tol,
        }
    }

    fn drum<'a>(&self, surface: &'a Surface, tool: &Tool, leave: f64) -> Result<Drum<'a>> {
        let leave = leave + TANGENCY;
        let cutter = tool_cutter(tool)?.grown(leave)?;
        let steady = match self.pattern {
            RotaryPattern::Rings { .. } => Some(cutter.shrunk(0.25 * self.tol)?),
            RotaryPattern::Lines { .. } | RotaryPattern::Helix { .. } => None,
        };
        let drum = Drum { surface, cutter, leave, budget: Budget::new(), steady, tol: self.tol };
        if let Some(top) = drum.top(self.lo, self.hi)
            && self.retract < top - 1e-9
        {
            return invalid(format!("the retract radius {} is inside the part ({top} from the axis, stock to leave included)", self.retract));
        }
        Ok(drum)
    }

    /// The passes, sampled. With `ends` a helix also goes a full turn round at each end of the
    /// range, so that the whole range is swept at every angle (roughing clears it all).
    ///
    /// Base samples are at most the cutter's radius apart whatever the step: every point of the
    /// part within the swept band is then under the cutter at some sample, so a feature between
    /// two samples that both rest on the surface, or both see nothing, is never cut through.
    fn passes(&self, drum: &Drum, ends: bool) -> Result<Vec<RPass>> {
        let step = self.step.min(drum.cutter.radius);
        let depth = depth_for(step, self.stray());
        let len = self.hi - self.lo;
        let sample = |way: Way, t0: f64, t1: f64, n: usize| {
            let tip = |t: f64| {
                let (x, th) = way.at(t);
                drum.at(x, th)
            };
            let point = |t: f64, h: f64| {
                let (x, th) = way.at(t);
                rotary_pose(x, th, h).tip
            };
            Pass { tip: &tip, point: &point, tol: self.stray() }.sample(t0, t1, n, depth)
        };
        match self.pattern {
            RotaryPattern::Lines { angle_step, zigzag } => {
                let lines = (TAU / angle_step - 1e-9).ceil().max(1.0);
                let per = (len / step).ceil().max(1.0);
                if !(lines * (per + 1.0) <= MAX_SAMPLES as f64) {
                    return Err(CamError::TooLarge(format!("more than {MAX_SAMPLES} samples; use a larger step, angle step or tool")));
                }
                let (lines, per) = (lines as usize, per as usize);
                (0..lines)
                    .map(|k| {
                        let way = Way::Line(TAU * k as f64 / lines as f64);
                        Ok(RPass::new(way, sample(way, self.lo, self.hi, per)?, zigzag && k % 2 == 1))
                    })
                    .collect()
            }
            RotaryPattern::Helix { pitch } => {
                let lead = if ends { TAU } else { 0.0 };
                let total = TAU * len / pitch + 2.0 * lead;
                let dth = (step / self.retract.hypot(pitch / TAU)).min(self.max_step);
                let n = (total / dth).ceil().max(1.0);
                if !(n <= MAX_SAMPLES as f64) {
                    return Err(CamError::TooLarge(format!("more than {MAX_SAMPLES} samples; use a larger step, pitch or tool")));
                }
                let way = Way::Helix { x0: self.lo, pitch, lead, lo: self.lo, hi: self.hi };
                Ok(vec![RPass::new(way, sample(way, 0.0, total, n as usize)?, false)])
            }
            RotaryPattern::Rings { stepover, zigzag } => {
                let gaps = (len / stepover - 1e-9).ceil().max(1.0);
                let per = (TAU / (step / self.retract).min(self.max_step)).ceil().max(1.0);
                if !((gaps + 1.0) * (per + 1.0) <= MAX_SAMPLES as f64) {
                    return Err(CamError::TooLarge(format!("more than {MAX_SAMPLES} samples; use a larger step, stepover or tool")));
                }
                let (gaps, per) = (gaps as usize, per as usize);
                (0..=gaps)
                    .map(|k| {
                        let way = Way::Ring(if k == gaps { self.hi } else { self.lo + len * k as f64 / gaps as f64 });
                        // Zigzag rings all go round 0..2π, every other one backwards; the others
                        // go on round, ring k from k turns, so each starts where the last ended.
                        let t0 = if zigzag { 0.0 } else { TAU * k as f64 };
                        Ok(RPass::new(way, sample(way, t0, t0 + TAU, per)?, zigzag && k % 2 == 1))
                    })
                    .collect()
            }
        }
    }

    /// The longest link fed straight across between passes (mm).
    fn max_link(&self, tool: &Tool) -> f64 {
        let spacing = match self.pattern {
            RotaryPattern::Lines { angle_step, .. } => self.retract * angle_step,
            RotaryPattern::Helix { pitch } => pitch,
            RotaryPattern::Rings { stepover, .. } => stepover,
        };
        (2.0 * spacing).max(tool.diameter)
    }

    /// Runs the passes at each level of radius: the tool at `max(level, resting radius)`,
    /// where that is inside the level before; where nothing is under it, at the level when
    /// `clear_cuts` and not at all otherwise.
    fn run(&self, drum: &Drum, tool: &Tool, passes: &[RPass], levels: &[f64], first_above: f64, clear_cuts: bool) -> Result<Toolpath5> {
        // Every level walks every pass's samples, with no drop-cutter work to count: bounded here.
        let samples = passes.iter().fold(0usize, |n, p| n.saturating_add(p.profile.len()));
        if samples.saturating_mul(levels.len()) > MAX_LEVEL_SAMPLES {
            return Err(CamError::TooLarge(format!(
                "{} levels over {samples} samples are more than {MAX_LEVEL_SAMPLES} to walk; use a larger stepdown, step or angle step",
                levels.len()
            )));
        }
        let mut out = Out5 { moves: Vec::new(), at: None, down: false, s: self };
        let max_link = self.max_link(tool);
        let spacing = self.step.min(drum.cutter.radius * 0.5);
        let mut above = first_above;
        for &level in levels {
            out.lift()?;
            // A pass resting nowhere inside the material left above cuts nothing at this level.
            for pass in passes.iter().filter(|p| !(p.lowest >= above)) {
                let rest = |t: f64| {
                    let (x, th) = pass.way.at(t);
                    drum.at(x, th)
                };
                let mut stretches: Vec<Vec<(f64, f64, f64)>> = spans(&pass.profile, level, above, clear_cuts, &rest)?
                    .into_iter()
                    .map(|s| {
                        s.into_iter()
                            .map(|(t, h)| {
                                let (x, th) = pass.way.at(t);
                                (x, th, h)
                            })
                            .collect()
                    })
                    .collect();
                if pass.backward {
                    stretches.reverse();
                    stretches.iter_mut().for_each(|s| s.reverse());
                }
                for s in &stretches {
                    out.span(s, drum, max_link, spacing)?;
                }
            }
            above = level;
        }
        out.finish()
    }
}

/// Collects rotary moves, keeping where the tool is as `(x, θ, ρ)`.
struct Out5<'a> {
    moves: Vec<Move5>,
    at: Option<(f64, f64, f64)>,
    down: bool,
    s: &'a Setup,
}

impl Out5<'_> {
    fn push(&mut self, m: Move5, at: (f64, f64, f64)) -> Result<()> {
        if self.moves.len() >= MAX_MOVES {
            return Err(CamError::TooLarge(format!("the toolpath has more than {MAX_MOVES} moves")));
        }
        self.moves.push(m);
        self.at = Some(at);
        Ok(())
    }

    fn rapid(&mut self, x: f64, th: f64, rho: f64) -> Result<()> {
        self.push(Move5::Rapid { to: rotary_pose(x, th, rho) }, (x, th, rho))
    }

    fn feed(&mut self, x: f64, th: f64, rho: f64, feed: f64) -> Result<()> {
        if self.at.is_some_and(|(x0, t0, r0)| (x0 - x).abs() <= 1e-12 && (t0 - th).abs() <= 1e-12 && (r0 - rho).abs() <= 1e-12) {
            return Ok(());
        }
        self.push(Move5::Feed { to: rotary_pose(x, th, rho), feed }, (x, th, rho))
    }

    /// Straight out to the clearance radius, if inside it.
    fn lift(&mut self) -> Result<()> {
        self.down = false;
        match self.at {
            Some((x, th, rho)) if rho < self.s.clearance => self.rapid(x, th, self.s.clearance),
            _ => Ok(()),
        }
    }

    /// Out, round at the clearance radius, in to the retract radius and down to the point at
    /// the plunge feed.
    fn enter(&mut self, (x, th, rho): (f64, f64, f64)) -> Result<()> {
        match self.at {
            None => self.rapid(x, th, self.s.clearance)?,
            Some(_) => {
                self.lift()?;
                if let Some((x0, th0, _)) = self.at {
                    self.round(x0, th0, x, th)?;
                }
            }
        }
        if self.s.clearance > self.s.retract {
            self.rapid(x, th, self.s.retract)?;
        }
        self.feed(x, th, rho, self.s.feeds.plunge)?;
        self.down = true;
        Ok(())
    }

    /// A rapid at the clearance radius from `(x0, th0)` to `(x1, th1)`, the short way round,
    /// along the corners of the polygon whose sides touch the clearance circle.
    fn round(&mut self, x0: f64, th0: f64, x1: f64, th1: f64) -> Result<()> {
        let c = self.s.clearance;
        let d = (th1 - th0 + PI).rem_euclid(TAU) - PI;
        if d.abs() > 1e-12 {
            let n = (d.abs() / self.s.max_step).ceil().max(1.0) as usize;
            let half = 0.5 * d / n as f64;
            for k in 0..n {
                let u = (2 * k + 1) as f64 / (2 * n) as f64;
                self.rapid(x0 + (x1 - x0) * u, th0 + d * u, c / half.cos())?;
            }
        }
        self.rapid(x1, th1, c)
    }

    /// Feeds straight on to `to` in steps (no more than the angle step each) when that link is
    /// short and stays outside where the cutter rests; `false` when it does not.
    /// A link goes straight across when that stays outside where the cutter rests; else out to
    /// the higher of its two ends' radii first (or across at it, then in), as a link from a
    /// ring beside a shoulder down onto the smaller diameter past it must: straight down across
    /// the shoulder's edge it cuts into it.
    fn link(&mut self, to: (f64, f64, f64), drum: &Drum, max_len: f64, spacing: f64) -> Result<bool> {
        let Some(from) = self.at.filter(|_| self.down) else { return Ok(false) };
        let len = rotary_pose(from.0, from.1, from.2).tip.dist(rotary_pose(to.0, to.1, to.2).tip);
        if !(len <= max_len) {
            return Ok(false);
        }
        let high = from.2.max(to.2);
        let corner = if from.2 >= to.2 { (to.0, to.1, high) } else { (from.0, from.1, high) };
        for path in [vec![to], vec![corner, to]] {
            let mut at = from;
            let mut clear = true;
            for &next in &path {
                if !self.clear_between(at, next, drum, spacing)? {
                    clear = false;
                    break;
                }
                at = next;
            }
            if !clear {
                continue;
            }
            let mut at = from;
            for &next in &path {
                // In moves turning A no more than the angle step each, about `spacing` long.
                let seg = rotary_pose(at.0, at.1, at.2).tip.dist(rotary_pose(next.0, next.1, next.2).tip);
                let n = ((next.1 - at.1).abs() / self.s.max_step).max(seg / spacing).ceil().clamp(2.0, MAX_LINK_SAMPLES as f64) as usize;
                for k in 1..=n {
                    let u = k as f64 / n as f64;
                    self.cut(at.0 + (next.0 - at.0) * u, at.1 + (next.1 - at.1) * u, at.2 + (next.2 - at.2) * u)?;
                }
                at = next;
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// Whether a straight move from `a` to `b` stays outside where the cutter rests (within the
    /// pass's stray), read at points along it no further apart than `spacing`, the angle step,
    /// and across the axis (along X and round) twice the tolerance: where the cutter rests
    /// on an edge beside the move, its rest changes by a millimetre over a tenth along X.
    fn clear_between(&self, a: (f64, f64, f64), b: (f64, f64, f64), drum: &Drum, spacing: f64) -> Result<bool> {
        let len = rotary_pose(a.0, a.1, a.2).tip.dist(rotary_pose(b.0, b.1, b.2).tip);
        let across = (b.0 - a.0).hypot(0.5 * (a.2 + b.2).abs() * (b.1 - a.1));
        let fine = (2.0 * self.s.tol).max(1e-3);
        let n = ((b.1 - a.1).abs() / self.s.max_step).max(len / spacing).max(across / fine).ceil().max(2.0);
        if !(n <= MAX_LINK_SAMPLES as f64) {
            return Ok(false);
        }
        let n = n as usize;
        for k in 1..n {
            let u = k as f64 / n as f64;
            let (x, th, rho) = (a.0 + (b.0 - a.0) * u, a.1 + (b.1 - a.1) * u, a.2 + (b.2 - a.2) * u);
            if let Tip::At(h) = drum.at(x, th)?
                && rho < h - self.s.stray()
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn span(&mut self, pts: &[(f64, f64, f64)], drum: &Drum, max_link: f64, spacing: f64) -> Result<()> {
        let Some(&first) = pts.first() else { return Ok(()) };
        if !self.link(first, drum, max_link, spacing)? {
            self.enter(first)?;
        }
        for &(x, th, rho) in pts.iter().skip(1) {
            self.cut(x, th, rho)?;
        }
        self.down = true;
        Ok(())
    }

    /// A cutting move to `(x, θ, ρ)`, slowed when it goes in towards the axis so the tool never
    /// goes into the material faster than the plunge feed ([`descent_feed`]).
    fn cut(&mut self, x: f64, th: f64, rho: f64) -> Result<()> {
        let feed = self.at.map_or(self.s.feeds.cut, |(x0, t0, r0)| {
            descent_feed(&self.s.feeds, rotary_pose(x0, t0, r0).tip.dist(rotary_pose(x, th, rho).tip), r0 - rho)
        });
        self.feed(x, th, rho, feed)
    }

    fn finish(mut self) -> Result<Toolpath5> {
        self.lift()?;
        let path = Toolpath5 { moves: self.moves };
        verify5(&path, self.s.max_step)?;
        Ok(path)
    }
}

/// Finishes a part turned on the rotary axis: for each sample the tool comes in radially and
/// its tip stops where the cutter first touches the surface ([`drop_along`] towards the axis),
/// lines along X at angle steps, rings round at X steps or one continuous helix, refined as
/// the 3-axis passes are ([`crate::surface_ops`]). Where nothing is under the tool it lifts out and comes back in
/// where the surface starts again; where the surface is inside the floor radius the tool runs
/// at the floor. Passes are linked by feeding across when that is short and clear, otherwise
/// out to the clearance radius and round.
pub fn rotary_finish(surface: &Surface, tool: &Tool, p: &RotaryParams) -> Result<Toolpath5> {
    let s = setup_check(p.pattern, p.range, p.clearance, p.retract, &p.feeds, p.step, p.tolerance, p.stock_to_leave, p.max_angle_step, surface)?;
    if !(p.floor >= 0.0 && p.floor <= s.retract) {
        return invalid("the floor radius must be from 0 to the retract radius");
    }
    let drum = s.drum(surface, tool, p.stock_to_leave)?;
    let passes = s.passes(&drum, false)?;
    s.run(&drum, tool, &passes, &[p.floor], f64::INFINITY, false)
}

/// Roughs round stock of `stock_radius` down towards the part in levels of radius, at most a
/// stepdown apart, down to the floor radius: at each level the tool runs at
/// `max(level, resting radius + stock to leave)`, only where that is inside the level before
/// (what is left of the stock), so it never cuts into the part. Where no part is under the tool
/// it cuts at the level. Entries plunge radially at the plunge feed; links are as in
/// [`rotary_finish`]. Passes may be at most the tool diameter apart at the stock radius, and a
/// helix goes a full turn round at each end of the range, so no stock is left at full height.
pub fn rotary_roughing(surface: &Surface, tool: &Tool, p: &RotaryRoughingParams) -> Result<Toolpath5> {
    tool.validate()?;
    crate::chamfer::roughs(tool, "rotary roughing")?;
    let s = setup_check(p.pattern, p.range, p.clearance, p.retract, &p.feeds, p.step, p.tolerance, p.stock_to_leave, p.max_angle_step, surface)?;
    check_stepdown(p.stepdown)?;
    if !(p.stock_radius.is_finite() && p.stock_radius > 0.0 && p.stock_radius <= s.retract) {
        return invalid("the stock radius must be above 0 and at most the retract radius");
    }
    if !(p.floor >= 0.0 && p.floor <= p.stock_radius) {
        return invalid("the floor radius must be from 0 to the stock radius");
    }
    let drum = s.drum(surface, tool, p.stock_to_leave)?;
    check_rough_stepover(p.pattern, p.stock_radius, tool)?;
    let passes = s.passes(&drum, true)?;
    let levels = levels(p.stock_radius, p.floor, p.stepdown, false)?;
    s.run(&drum, tool, &passes, &levels, p.stock_radius, true)
}
