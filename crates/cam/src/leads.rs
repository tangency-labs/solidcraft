//! Lead-ins and lead-outs, entries, linking, holding tabs and finishing passes of the 2D
//! operations. [`contour_with`] and [`pocket_with`] are [`crate::contour`] and
//! [`crate::pocket`] with these choices; with the default options they make the same paths.
//!
//! - A **lead** is a tangent arc in XY (with an optional straight line before it) from the side
//!   of the pass away from the part into the pass's start, and the same out of it at its end.
//!   It is checked against the profile with exact 2D geometry and refused where it would cut
//!   the part.
//! - An **entry** is how the tool gets down to each depth: straight down at the plunge feed, a
//!   ramp along the pass (along a contour's lead, or the stretch before its first tab), or a
//!   helix tangent to the pass at its start. A helix that does not fit is made smaller, then
//!   replaced by the ramp, which in turn is replaced by the plunge where there is no room.
//! - **Linking**: within a level the tool stays down where the straight link runs where the
//!   passes cut so far (or the one it leads to) took the material away, tracked by the passes'
//!   distances from the boundary. Otherwise it goes up to the clearance height, or, with
//!   [`Linking::StayDown`], only to the retract height when the caller says that is clear of the
//!   part, and a pocket also stays down from one level to the next through the level it cleared.
//! - **Tabs** hold a part cut out round its outside: placed evenly along the profile on straight
//!   stretches (away from corners), the tool rises over each on ramps.
//! - **Finishing passes** go round the final wall at full depth after the depth passes, each
//!   `finish_stepover` closer, and a **spring pass** repeats the last one.

use std::f64::consts::{PI, TAU};

use serde::{Deserialize, Serialize};
use solidcore_record::{Region2, Vec2};
use solidcraft_geom::prelude::*;

use crate::offset::{Shape, reversed};
use crate::ops::{
    Builder, ContourParams, Cut, Feeds, Heights, MAX_COORD, MAX_RAMP_LAPS, PocketParams, Side, check_leave, check_ramp, corner_spur, link_is_clear,
    mills, pocket_rings, start_near,
};
use crate::path::{Move, Toolpath, arc_sweep, vertical_quarter};
use crate::piece::{Piece, wrap};
use crate::tool::{Tool, ToolKind};
use crate::{CamError, Result, invalid};

/// Most tabs on one profile.
pub const MAX_TABS: usize = 200;
/// Most finishing passes of one contour.
pub const MAX_FINISH_PASSES: usize = 20;
/// Most depth steps a finishing pass is taken down in.
const MAX_FINISH_STEPS: usize = 1000;
/// Largest lead radius or line (mm).
const MAX_LEAD: f64 = 1.0e4;
/// Smallest helix and lead radius (mm): smaller arcs are lost to the rounding of the program.
pub const MIN_ARC_RADIUS: f64 = 0.05;
/// Most points one lead piece or link is checked at.
const MAX_SAMPLES: usize = 4000;
/// Most places along a pocket ring tried for a helix.
const MAX_HELIX_SPOTS: usize = 64;

/// A tangent arc into and out of each pass of a contour, in the plane of the cut (XY).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lead {
    /// Radius of the arc (mm).
    pub radius: f64,
    /// How far round the arc turns (radians, from 1° to π).
    pub sweep: f64,
    /// A straight line along the arc before the lead-in and after the lead-out (mm; 0: none).
    pub line: f64,
}

/// How the tool goes down into each depth pass.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Entry {
    /// Straight down at the plunge feed.
    Plunge,
    /// Down along the pass no steeper than `angle` (radians): a contour along its lead (back and
    /// forth), or before its first tab, else round the loop; a pocket round its ring.
    Ramp { angle: f64 },
    /// Down round a circle of `radius` (at most the tool's radius, so that no core is left)
    /// tangent to the pass at its start, no steeper than `angle`, then once round at the depth.
    Helix { radius: f64, angle: f64 },
}

/// How many tabs a profile gets.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabLayout {
    Count(usize),
    /// About this far apart along the profile (mm), at least one.
    Spacing(f64),
}

/// Holding tabs on an outside contour that cuts through the stock.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tabs {
    pub layout: TabLayout,
    /// Length of the material left along the wall (mm).
    pub width: f64,
    /// Height of the tabs above the bottom (mm), less than the depth of the cut.
    pub height: f64,
    /// Steepness of the ramps up onto and down from each tab (radians).
    pub ramp_angle: f64,
}

/// Where the tool goes between passes that it cannot link in the cut.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Linking {
    /// Up to the clearance height.
    #[default]
    Clearance,
    /// Up to the retract height where the link is clear of the part above it, else to the
    /// clearance height; a pocket stays down from one level to the next.
    StayDown,
}

/// A contour's leads, entry, linking, tabs and finishing passes. The default is none of them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ContourOptions {
    pub lead: Option<Lead>,
    /// `None`: the contour's `ramp_angle` (0: plunge).
    pub entry: Option<Entry>,
    /// Outside contours only.
    pub tabs: Option<Tabs>,
    /// Passes at full depth after the depth passes, `finish_stepover` apart, the last on the
    /// final wall; the depth passes leave `finish_passes × finish_stepover` for them.
    pub finish_passes: usize,
    /// Distance between finishing passes (mm), above 0 and at most half the tool radius.
    pub finish_stepover: f64,
    /// Repeat the last pass once.
    pub spring_pass: bool,
    pub linking: Linking,
    /// The radius of a vertical lead (mm): each pass is entered down a quarter arc in the
    /// vertical plane of its start's direction, from this far above and behind its start (its
    /// lead's, if it has one), and left up a quarter arc beyond its end. `G18`/`G19` arcs where
    /// that direction runs along X or Y, lines along the arc otherwise. It takes the place of
    /// the entry (a plunge only). `None`: none.
    #[serde(default)]
    pub vertical_lead: Option<f64>,
}

/// A pocket's entry and linking. The default is the pocket's `ramp_angle` and clearance links.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PocketOptions {
    /// `None`: the pocket's `ramp_angle` (0: plunge).
    pub entry: Option<Entry>,
    pub linking: Linking,
}

fn check_angle(angle: f64, what: &str) -> Result<()> {
    if angle > 0.0 && angle <= 1.5 { Ok(()) } else { invalid(format!("the {what} angle must be above 0 and at most 85 degrees")) }
}

impl Entry {
    fn validate(&self, tool_radius: f64) -> Result<()> {
        match *self {
            Entry::Plunge => Ok(()),
            Entry::Ramp { angle } => check_angle(angle, "ramp"),
            Entry::Helix { radius, angle } => {
                check_angle(angle, "helix")?;
                if radius >= MIN_ARC_RADIUS && radius <= tool_radius + 1e-9 {
                    Ok(())
                } else {
                    invalid(format!("the helix radius must be from {MIN_ARC_RADIUS} mm to the tool's radius (a larger helix leaves a core standing)"))
                }
            }
        }
    }
}

impl Lead {
    fn validate(&self) -> Result<()> {
        if !(self.radius >= MIN_ARC_RADIUS && self.radius <= MAX_LEAD) {
            return invalid(format!("the lead radius must be from {MIN_ARC_RADIUS} to {MAX_LEAD} mm"));
        }
        if !(self.sweep >= 1f64.to_radians() && self.sweep <= PI) {
            return invalid("the lead's sweep must be from 1 to 180 degrees");
        }
        if !(self.line >= 0.0 && self.line <= MAX_LEAD) {
            return invalid(format!("the lead's line must be from 0 to {MAX_LEAD} mm"));
        }
        Ok(())
    }
}

impl Tabs {
    fn validate(&self, cut: &Cut) -> Result<()> {
        match self.layout {
            TabLayout::Count(n) if n == 0 || n > MAX_TABS => return invalid(format!("the tab count must be from 1 to {MAX_TABS}")),
            TabLayout::Spacing(s) if !(s > 0.0 && s <= MAX_COORD) => return invalid("the tab spacing must be above 0"),
            _ => {}
        }
        if !(self.width > 0.0 && self.width <= MAX_COORD) {
            return invalid("the tab width must be above 0");
        }
        if !(self.height > 0.0 && self.height < cut.heights.top - cut.heights.bottom - 1e-6) {
            return invalid("the tab height must be above 0 and less than the depth of the cut");
        }
        check_angle(self.ramp_angle, "tab ramp")
    }
}

impl Entry {
    /// How steeply it goes down into the material (radians): 0 for a plunge.
    fn angle(&self) -> f64 {
        match *self {
            Entry::Plunge => 0.0,
            Entry::Ramp { angle } | Entry::Helix { angle, .. } => angle,
        }
    }
}

/// The entry given, or the one the ramp angle stands for.
fn entry_of(entry: Option<Entry>, ramp: f64, tool_radius: f64) -> Result<Entry> {
    match entry {
        None => Ok(if ramp > 0.0 { Entry::Ramp { angle: ramp } } else { Entry::Plunge }),
        Some(_) if ramp > 0.0 => invalid("give an entry or a ramp angle, not both"),
        Some(e) => {
            e.validate(tool_radius)?;
            Ok(e)
        }
    }
}

fn xy(p: solidcore_record::Vec3) -> Vec2 {
    Vec2::new(p.x, p.y)
}

/// Slows every cut that goes down so that the tool goes down no faster than the plunge feed.
/// A ramp, a helix or the way down off a tab at the cutting feed would otherwise drive the tool
/// down many times faster than that (a 30° ramp at 2880 mm/min goes down at 1440 mm/min, where
/// a small tool may be rated for 100).
fn limit_descent(moves: &mut [Move], plunge: f64) {
    /// Points a vertical arc's slope is taken at, its ends included.
    const STEEPEST_SAMPLES: usize = 64;
    let cap = |feed: &mut f64, len: f64, dz: f64| {
        let most = plunge * (len / dz).max(1.0);
        if *feed > most {
            *feed = most;
        }
    };
    let mut pos: Option<solidcore_record::Vec3> = None;
    for m in moves.iter_mut() {
        let from = pos;
        pos = m.target().or(pos);
        let Some(p) = from else { continue };
        match m {
            Move::Feed { to, feed } if p.z - to.z > 1e-9 => cap(feed, p.dist(*to), p.z - to.z),
            Move::Arc { to, center, ccw, feed } if p.z - to.z > 1e-9 => {
                let run = xy(p).dist(*center) * arc_sweep(p, *to, *center, *ccw);
                cap(feed, run.hypot(p.z - to.z), p.z - to.z);
            }
            // A vertical arc goes down fastest where it is steepest (a quarter arc into a cut
            // starts straight down), not at its average.
            Move::PlaneArc { to, center, plane, ccw, feed } => {
                let arc = crate::path::PlaneArc { from: p, to: *to, center: *center, plane: *plane, ccw: *ccw };
                let sampled =
                    (0..=STEEPEST_SAMPLES).filter_map(|k| arc.tangent(k as f64 / STEEPEST_SAMPLES as f64)).map(|t| -t.z).fold(0.0f64, f64::max);
                // Between two samples the slope is at most this much steeper.
                let between = 1.0 - (arc.sweep().abs() / (2 * STEEPEST_SAMPLES) as f64).cos();
                let steepest = (sampled + between).min(1.0);
                if sampled > 1e-9 {
                    cap(feed, 1.0, steepest);
                }
            }
            _ => {}
        }
    }
}

/// Whether every point of `lp` is within `reach` of one of `others`; sampled about a quarter of
/// a tool radius apart.
fn runs_near(lp: &[Piece], others: &[Vec<Piece>], reach: f64, r: f64) -> bool {
    lp.iter().all(|piece| {
        let len = piece.length();
        let step = (r * 0.25).max(len / MAX_SAMPLES as f64).max(1e-3);
        let n = ((len / step).ceil() as usize).clamp(1, MAX_SAMPLES);
        (0..=n).all(|i| {
            let q = piece.point_at(i as f64 / n as f64);
            others.iter().flatten().any(|o| o.dist(q) <= reach)
        })
    })
}

fn length(lp: &[Piece]) -> f64 {
    lp.iter().map(Piece::length).sum()
}

/// The point and direction of travel `s` along `lp`.
fn at_length(lp: &[Piece], s: f64) -> (Vec2, Vec2) {
    let mut acc = 0.0;
    for piece in lp {
        let l = piece.length();
        if s <= acc + l {
            let t = if l > 0.0 { ((s - acc) / l).clamp(0.0, 1.0) } else { 0.0 };
            return (piece.point_at(t), piece.tangent_at(t));
        }
        acc += l;
    }
    lp.last().map_or((Vec2::default(), Vec2::default()), |p| (p.end(), p.tangent_at(1.0)))
}

/// The loop turned to start `s` along it (a piece is split there).
fn start_at(lp: &[Piece], s: f64) -> Vec<Piece> {
    let mut acc = 0.0;
    for (i, piece) in lp.iter().enumerate() {
        let l = piece.length();
        if s < acc + l - 1e-9 {
            let t = if l > 0.0 { ((s - acc) / l).clamp(0.0, 1.0) } else { 0.0 };
            let rest = lp.iter().skip(i + 1).chain(lp.iter().take(i)).copied();
            if t * l <= 1e-9 {
                return std::iter::once(*piece).chain(rest).collect();
            }
            return std::iter::once(piece.sub(t, 1.0)).chain(rest).chain(std::iter::once(piece.sub(0.0, t))).collect();
        }
        acc += l;
    }
    lp.to_vec()
}

/// The first `s` of the loop.
fn head(lp: &[Piece], s: f64) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut acc = 0.0;
    for piece in lp {
        let l = piece.length();
        if acc + l >= s {
            let t = if l > 0.0 { ((s - acc) / l).clamp(0.0, 1.0) } else { 0.0 };
            if t * l > 1e-9 {
                out.push(piece.sub(0.0, t));
            }
            break;
        }
        out.push(*piece);
        acc += l;
    }
    out
}

/// The parameter of the point of `piece` nearest to `p`.
fn param_near(piece: &Piece, p: Vec2) -> f64 {
    match *piece {
        Piece::Line { a, b } => {
            let d = b - a;
            let l2 = d.len2();
            if l2 > 0.0 { ((p - a).dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 }
        }
        Piece::Arc { c, a0, sweep, .. } => {
            let span = sweep.abs();
            if !(span > 0.0) {
                return 0.0;
            }
            let theta = (p - c).angle();
            let delta = if sweep > 0.0 { wrap(theta - a0) } else { wrap(a0 - theta) };
            if delta <= span {
                delta / span
            } else if piece.start().dist(p) <= piece.end().dist(p) {
                0.0
            } else {
                1.0
            }
        }
    }
}

/// How far along `lp` its point nearest to `p` is.
fn nearest_length(lp: &[Piece], p: Vec2) -> f64 {
    let (mut best, mut best_d, mut acc) = (0.0, f64::INFINITY, 0.0);
    for piece in lp {
        let t = param_near(piece, p);
        let d = piece.point_at(t).dist(p);
        if d < best_d {
            (best, best_d) = (acc + t * piece.length(), d);
        }
        acc += piece.length();
    }
    best
}

/// The middle of the loop's longest straight piece (of its longest piece when it has none).
fn longest_middle(lp: &[Piece]) -> f64 {
    let mut acc = 0.0;
    let mut best: Option<(bool, f64, f64)> = None;
    for piece in lp {
        let (line, l) = (matches!(piece, Piece::Line { .. }), piece.length());
        if best.is_none_or(|(bl, bv, _)| (line, l) > (bl, bv)) {
            best = Some((line, l, acc + l * 0.5));
        }
        acc += l;
    }
    best.map_or(0.0, |b| b.2)
}

/// What a contour's leads and entries are checked against.
struct Geo<'a> {
    shape: &'a Shape,
    side: Side,
    /// Closest a tool centre may come to the profile.
    away: f64,
    r: f64,
}

impl Geo<'_> {
    /// Whether a tool centre at `p`, grown by `extra`, stays off the part: on the cutting side
    /// of the profile, at least `away + extra` from it.
    fn free(&self, p: Vec2, extra: f64) -> bool {
        let d = self.shape.dist(p);
        if !(d >= self.away + extra - 1e-6) {
            return false;
        }
        let inside = self.shape.inside(p, d.max(1e-6));
        if self.side == Side::Outside { !inside } else { inside }
    }

    /// Whether a tool centre at `p` is between the passes `lo` and `hi` from the profile, on
    /// its cutting side.
    fn in_band(&self, p: Vec2, lo: f64, hi: f64) -> bool {
        let d = self.shape.dist(p);
        if !(d >= lo - 1e-6 && d <= hi + 1e-6) {
            return false;
        }
        let inside = self.shape.inside(p, d.max(1e-6));
        if self.side == Side::Outside { !inside } else { inside }
    }

    /// Whether every point of `pieces` but `except` is free; samples about an eighth of a tool
    /// radius apart.
    fn path_is_free(&self, pieces: &[Piece], except: Vec2) -> bool {
        pieces.iter().all(|piece| {
            let len = piece.length();
            let step = (self.r * 0.125).max(len / MAX_SAMPLES as f64).max(1e-3);
            let n = ((len / step).ceil() as usize).clamp(1, MAX_SAMPLES);
            (0..=n).map(|i| piece.point_at(i as f64 / n as f64)).filter(|q| q.dist(except) > 1e-6).all(|q| self.free(q, 0.0))
        })
    }

    /// Whether the straight link from `a` to `b` stays between the passes `lo` and `hi`.
    fn link_in_band(&self, a: Vec2, b: Vec2, lo: f64, hi: f64) -> bool {
        let n = (a.dist(b) / (self.r * 0.25).max(1e-3)).ceil();
        if !(n <= MAX_SAMPLES as f64) {
            return false;
        }
        let n = (n as usize).max(1);
        (0..=n).all(|i| self.in_band(a.lerp(b, i as f64 / n as f64), lo, hi))
    }
}

/// The tabs of a profile: where they are (along the pass) and how the tool rises over them.
#[derive(Clone, Copy)]
struct TabPlan {
    /// Half the length of the top of a tab's pass: half the tab width plus the tool radius.
    plateau: f64,
    /// Height of the top of the tabs.
    top: f64,
    /// Tangent of the ramp angle.
    slope: f64,
    /// Lowest pass.
    bottom: f64,
}

impl TabPlan {
    /// How far a tab's pass reaches either side of its centre at the bottom.
    fn reach(&self) -> f64 {
        self.plateau + (self.top - self.bottom) / self.slope
    }

    /// The pass's height along a loop of length `total` with tabs at `tabs` at level `z`: `(s, z)`.
    fn knots(&self, tabs: &[f64], z: f64, total: f64) -> Vec<(f64, f64)> {
        let mut k = vec![(0.0, z)];
        if z < self.top - 1e-9 {
            let ramp = (self.top - z) / self.slope;
            for &c in tabs {
                k.extend([(c - self.plateau - ramp, z), (c - self.plateau, self.top), (c + self.plateau, self.top), (c + self.plateau + ramp, z)]);
            }
        }
        k.push((total, z));
        k
    }

    /// Whether tabs at `tabs` (sorted) keep `margin` from the loop's start and end and stay apart.
    fn fits(&self, tabs: &[f64], total: f64, margin: f64) -> bool {
        let e = self.reach();
        tabs.first().is_none_or(|f| f - e >= margin - 1e-9)
            && tabs.last().is_none_or(|l| l + e <= total - margin + 1e-9)
            && tabs.windows(2).all(|w| matches!(w, [a, b] if b - a >= 2.0 * e - 1e-9))
    }
}

/// Where the tabs go along a loop: evenly spaced, each moved onto the nearest straight piece
/// long enough to hold it, `margin` clear of the loop's start.
fn place_tabs(lp: &[Piece], tabs: &Tabs, plan: &TabPlan, margin: f64) -> Result<Vec<f64>> {
    let total = length(lp);
    let n = match tabs.layout {
        TabLayout::Count(n) => n,
        TabLayout::Spacing(s) => {
            let n = (total / s).round().max(1.0);
            if !(n <= MAX_TABS as f64) {
                return Err(CamError::TooLarge(format!("the spacing gives more than {MAX_TABS} tabs")));
            }
            n as usize
        }
    };
    let e = plan.reach();
    let (lo, hi) = (margin + e, total - margin - e);
    if !(hi >= lo) {
        return invalid("the profile is too short for its tabs");
    }
    let mut good = Vec::new();
    let mut acc = 0.0;
    for piece in lp {
        let l = piece.length();
        if matches!(piece, Piece::Line { .. }) {
            let (a, b) = ((acc + e).max(lo), (acc + l - e).min(hi));
            if b >= a {
                good.push((a, b));
            }
        }
        acc += l;
    }
    if good.is_empty() {
        good.push((lo, hi));
    }
    let mut chosen: Vec<f64> = Vec::with_capacity(n);
    for k in 0..n {
        let u = total * (k as f64 + 0.5) / n as f64;
        let mut tries: Vec<f64> = Vec::new();
        for &(a, b) in &good {
            tries.push(u.clamp(a, b));
            for c in &chosen {
                tries.extend([(c + 2.0 * e).clamp(a, b), (c - 2.0 * e).clamp(a, b)]);
            }
        }
        tries.sort_by(|a, b| (a - u).abs().total_cmp(&(b - u).abs()));
        let Some(x) = tries.into_iter().find(|x| chosen.iter().all(|c| (x - c).abs() >= 2.0 * e - 1e-9)) else {
            return invalid(format!("there is no room along the profile for {n} tabs this wide and high"));
        };
        chosen.push(x);
    }
    chosen.sort_by(f64::total_cmp);
    Ok(chosen)
}

/// One pass round a contour loop: the loop (starting where the tool comes in), its tabs (how far
/// along) and its leads.
#[derive(Clone)]
struct Pass {
    lp: Vec<Piece>,
    tabs: Vec<f64>,
    lead_in: Vec<Piece>,
    lead_out: Vec<Piece>,
    /// The radius of its vertical leads ([`ContourOptions::vertical_lead`]).
    vertical: Option<f64>,
}

impl Pass {
    fn start(&self) -> Vec2 {
        self.lp.first().map_or(Vec2::default(), Piece::start)
    }

    /// Where the cut at depth starts (its lead's start, if it has one) and its direction there.
    fn cut_start(&self) -> (Vec2, Vec2) {
        match self.lead_in.first().or(self.lp.first()) {
            Some(piece) => (piece.start(), piece.tangent_at(0.0)),
            None => (Vec2::default(), Vec2::new(1.0, 0.0)),
        }
    }

    /// Where the cut at depth ends (its lead out's end, if it has one) and its direction there.
    fn cut_end(&self) -> (Vec2, Vec2) {
        match self.lead_out.last() {
            Some(piece) => (piece.end(), piece.tangent_at(1.0)),
            None => (self.start(), self.lp.last().map_or(Vec2::new(1.0, 0.0), |piece| piece.tangent_at(1.0))),
        }
    }

    /// Where the tool goes down: behind the start by the vertical lead's radius, if it has one.
    fn entry(&self) -> Vec2 {
        let (p, t) = self.cut_start();
        match self.vertical {
            Some(r) => p - t * r,
            None => p,
        }
    }

    /// Whether the tool leaves the pass's start to come back to it: by a lead or a vertical lead.
    fn leads(&self) -> bool {
        !self.lead_in.is_empty() || self.vertical.is_some()
    }

    /// Down a quarter arc from above and behind the start to the start at `z`, from wherever
    /// the tool is above [`Pass::entry`].
    fn vertical_in(&self, b: &mut Builder, z: f64, r: f64) -> Result<()> {
        let (p, t) = self.cut_start();
        let Some(q) = b.pos else { return Err(CamError::Unsafe("a lead before the first positioning move".into())) };
        let top = z + r;
        if q.z > top + 1e-9 {
            b.plunge_to(top)?;
        } else if q.z < top - 1e-9 {
            // Back up the way it came down.
            b.push(Move::Rapid { to: xy(q).extend(top) })?;
        }
        let Some(from) = b.pos else { return Err(CamError::Unsafe("a lead before the first positioning move".into())) };
        for m in vertical_quarter(from, p.extend(z), p.extend(top), t, b.feeds.cut) {
            b.push(m)?;
        }
        Ok(())
    }

    /// Up a quarter arc from the end at `z` to above and beyond it.
    fn vertical_out(&self, b: &mut Builder, z: f64, r: f64) -> Result<()> {
        let (e, t) = self.cut_end();
        for m in vertical_quarter(e.extend(z), (e + t * r).extend(z + r), e.extend(z + r), t, b.feeds.cut) {
            b.push(m)?;
        }
        Ok(())
    }

    fn pieces(&self) -> impl Iterator<Item = &Piece> {
        self.lp.iter().chain(&self.lead_in).chain(&self.lead_out)
    }
}

/// The lead into a loop's start and out of its end, on the side `away_left` (of travel) or right.
fn lead_paths(lead: &Lead, lp: &[Piece], away_left: bool) -> Option<(Vec<Piece>, Vec<Piece>)> {
    let (first, last) = (lp.first()?, lp.last()?);
    let p = first.start();
    let arc = |t: Vec2, into: bool| -> Vec<Piece> {
        let n = if away_left { t.perp() } else { -t.perp() };
        let c = p + n * lead.radius;
        let sweep = if away_left { lead.sweep } else { -lead.sweep };
        let at_p = (p - c).angle();
        if into {
            let a = Piece::Arc { c, r: lead.radius, a0: at_p - sweep, sweep };
            let mut out = Vec::new();
            if lead.line > 1e-9 {
                let s = a.start();
                out.push(Piece::Line { a: s - a.tangent_at(0.0) * lead.line, b: s });
            }
            out.push(a);
            out
        } else {
            let a = Piece::Arc { c, r: lead.radius, a0: at_p, sweep };
            let mut out = vec![a];
            if lead.line > 1e-9 {
                let e = a.end();
                out.push(Piece::Line { a: e, b: e + a.tangent_at(1.0) * lead.line });
            }
            out
        }
    };
    Some((arc(first.tangent_at(0.0), true), arc(last.tangent_at(1.0), false)))
}

fn make_pass(lp: Vec<Piece>, tabs: Vec<f64>, lead: Option<Lead>, vertical: Option<f64>, geo: &Geo<'_>, away_left: bool) -> Result<Pass> {
    let (lead_in, lead_out) = match lead {
        None => (Vec::new(), Vec::new()),
        Some(l) => {
            let Some((i, o)) = lead_paths(&l, &lp, away_left) else { return invalid("a profile is empty") };
            let p = lp.first().map_or(Vec2::default(), Piece::start);
            if !geo.path_is_free(&i, p) || !geo.path_is_free(&o, p) {
                return invalid("the lead would cut into the part: give it a smaller radius or sweep, or a shorter line");
            }
            (i, o)
        }
    };
    let pass = Pass { lp, tabs, lead_in, lead_out, vertical };
    if let Some(r) = vertical {
        // The vertical leads come down behind the start and go up beyond the end: both there
        // must be clear of the part.
        let ((p, t), (e, te)) = (pass.cut_start(), pass.cut_end());
        let behind = [Piece::Line { a: p - t * r, b: p }];
        let beyond = [Piece::Line { a: e, b: e + te * r }];
        if !geo.path_is_free(&behind, p) || !geo.path_is_free(&beyond, e) {
            return invalid("the vertical lead would cut into the part: give it a smaller radius, or a lead that ends clear of the part");
        }
    }
    Ok(pass)
}

/// To `to` between passes: up to the retract height and across when `linking` allows it and
/// `above` says that is clear of the part, else up to the clearance height and across.
fn hop(b: &mut Builder, to: Vec2, linking: Linking, above: &dyn Fn(Vec2, Vec2, f64) -> bool) -> Result<()> {
    let Some(q) = b.pos else { return b.travel_to(to) };
    let h = b.h.retract;
    if linking != Linking::StayDown || q.z >= b.h.clearance || !above(xy(q), to, h) {
        return b.travel_to(to);
    }
    if q.z < h {
        b.push(Move::Rapid { to: xy(q).extend(h) })?;
    }
    if !b.at(to) {
        b.push(Move::Rapid { to: to.extend(q.z.max(h)) })?;
    }
    Ok(())
}

/// Down from where the tool is to `z` round the circle about `c` through it, no steeper than
/// `angle`, then once round at `z`: it ends where it started. False when the circle is too
/// small to go down round.
fn helix(b: &mut Builder, c: Vec2, ccw: bool, z: f64, angle: f64) -> Result<bool> {
    let Some(p) = b.pos else { return Err(CamError::Unsafe("a helix before the first positioning move".into())) };
    let start = xy(p);
    let radius = start.dist(c);
    let drop = p.z - z;
    if !(drop > 1e-9) {
        return Ok(true);
    }
    let laps = (drop / (TAU * radius * angle.tan()) - 1e-9).ceil().max(1.0);
    if !(radius >= MIN_ARC_RADIUS * 0.5) || !(laps <= MAX_RAMP_LAPS) {
        return Ok(false);
    }
    let a0 = (start - c).angle();
    let sign = if ccw { 1.0 } else { -1.0 };
    let halves = 2 * laps as usize;
    for k in 1..=halves + 2 {
        let t = k.min(halves) as f64 / halves as f64;
        let to = c + Vec2::from_angle(a0 + sign * PI * k as f64) * radius;
        b.push(Move::Arc { to: to.extend(p.z - drop * t), center: c, ccw, feed: b.feeds.cut })?;
    }
    Ok(true)
}

/// Down from where the tool is (the start of `path`) to `z` along `path` and back, as often as
/// it takes to be no steeper than `angle`: it ends at the start. False when the path is too short.
fn zigzag(b: &mut Builder, path: &[Piece], z: f64, angle: f64) -> Result<bool> {
    let Some(from) = b.pos.map(|p| p.z) else { return Err(CamError::Unsafe("a ramp before the first positioning move".into())) };
    let drop = from - z;
    if !(drop > 1e-9) {
        return Ok(true);
    }
    let len = length(path);
    let mut runs = (drop / (len * angle.tan()) - 1e-9).ceil().max(1.0);
    if runs % 2.0 != 0.0 {
        runs += 1.0;
    }
    if !(angle > 0.0) || !(len > 1e-3) || !(runs <= 2.0 * MAX_RAMP_LAPS) {
        return Ok(false);
    }
    let back = reversed(path);
    let total = runs * len;
    let mut done = 0.0;
    for k in 0..runs as usize {
        let run: &[Piece] = if k % 2 == 0 { path } else { &back };
        for piece in run {
            let z0 = from - drop * done / total;
            done += piece.length();
            b.cut_piece(piece, z0, from - drop * (done / total).min(1.0))?;
        }
    }
    if let Some(p) = b.pos {
        b.cut_to(xy(p), z)?;
    }
    Ok(true)
}

/// Where a helix of at most `radius` tangent to direction `t` at `q` fits, trying the side
/// `left` of `t` first: its centre and whether it turns counter-clockwise.
fn helix_centre(geo: &Geo<'_>, q: Vec2, t: Vec2, radius: f64, left: bool) -> Option<(Vec2, bool)> {
    for k in [1.0, 0.75, 0.5, 0.25] {
        let rr = radius * k;
        if rr < MIN_ARC_RADIUS {
            break;
        }
        for side in [left, !left] {
            let c = q + if side { t.perp() } else { -t.perp() } * rr;
            if geo.free(c, rr) {
                return Some((c, side));
            }
        }
    }
    None
}

/// How the contour gets from where the tool is down to `z` at the pass's entry.
struct Down<'a> {
    entry: Entry,
    geo: &'a Geo<'a>,
    plan: Option<&'a TabPlan>,
    away_left: bool,
}

impl Down<'_> {
    fn enter(&self, b: &mut Builder, pass: &Pass, z: f64) -> Result<()> {
        if let Some(r) = pass.vertical {
            return pass.vertical_in(b, z, r);
        }
        match self.entry {
            Entry::Plunge => b.plunge_to(z),
            Entry::Ramp { angle } => self.ramp(b, pass, z, angle),
            Entry::Helix { radius, angle } => {
                let Some(p) = b.pos else { return Err(CamError::Unsafe("an entry before the first positioning move".into())) };
                let t = pass.lead_in.first().or(pass.lp.first()).map_or(Vec2::default(), |piece| piece.tangent_at(0.0));
                match helix_centre(self.geo, xy(p), t, radius, self.away_left) {
                    Some((c, ccw)) if helix(b, c, ccw, z, angle)? => Ok(()),
                    _ => self.ramp(b, pass, z, angle),
                }
            }
        }
    }

    fn ramp(&self, b: &mut Builder, pass: &Pass, z: f64, angle: f64) -> Result<()> {
        if !pass.lead_in.is_empty() {
            return if zigzag(b, &pass.lead_in, z, angle)? { Ok(()) } else { b.plunge_to(z) };
        }
        match (self.plan.filter(|p| z < p.top - 1e-9), pass.tabs.first()) {
            (Some(plan), Some(&first)) => {
                let stretch = head(&pass.lp, first - plan.plateau - (plan.top - z) / plan.slope);
                if zigzag(b, &stretch, z, angle)? { Ok(()) } else { b.plunge_to(z) }
            }
            _ => b.ramp_loop(&pass.lp, z, angle),
        }
    }
}

/// Along `lp` from its start at the heights `knots` (`(s, z)`, by `s`) give, back to the start.
fn cut_profile(b: &mut Builder, lp: &[Piece], knots: &[(f64, f64)]) -> Result<()> {
    let z_at = |s: f64| -> f64 {
        for w in knots.windows(2) {
            if let [(s0, z0), (s1, z1)] = *w
                && s <= s1
            {
                return if s1 - s0 > 1e-12 { z0 + (z1 - z0) * ((s - s0) / (s1 - s0)).clamp(0.0, 1.0) } else { z1 };
            }
        }
        knots.last().map_or(0.0, |k| k.1)
    };
    let mut acc = 0.0;
    for piece in lp {
        let l = piece.length();
        if !(l > 1e-12) {
            continue;
        }
        let mut s0 = acc;
        let inner = knots.iter().map(|k| k.0).filter(|s| *s > acc + 1e-9 && *s < acc + l - 1e-9);
        for s1 in inner.chain(std::iter::once(acc + l)) {
            let sub = piece.sub((s0 - acc) / l, (s1 - acc) / l);
            b.cut_piece(&sub, z_at(s0), z_at(s1))?;
            s0 = s1;
        }
        acc += l;
    }
    Ok(())
}

/// The depths a finishing pass runs at: the bottom alone where the wall from `top` to
/// `bottom` is within the tool's flutes; else steps down from the top, evenly and no deeper
/// apart than the flutes reach, the last at the bottom. A finishing pass takes its stepover
/// off the whole wall below it, so deeper than the flutes the shank above them would rub (or
/// cut with) the wall; stepped, the wall above each step is already finished at the pass's
/// offset, and a shank no wider than the cutter passes it. A shank wider than the cutter
/// would rub that finished wall: refused.
fn finish_levels(tool: &Tool, top: f64, bottom: f64) -> Result<Vec<f64>> {
    let (depth, flutes) = (top - bottom, tool.flute_length);
    if !(depth > flutes + 1e-9) {
        return Ok(vec![bottom]);
    }
    if tool.shank_diameter.is_some_and(|s| s > tool.diameter + 1e-9) {
        return invalid(format!(
            "the finishing passes run down a {depth:.3} mm wall, deeper than tool {}'s {flutes:.3} mm flutes, and its shank is wider than its cutter, so it would rub the finished wall above each step: use a tool whose flutes reach the depth, or no finishing passes",
            tool.number
        ));
    }
    let n = (depth / flutes - 1e-9).ceil();
    if !(n <= MAX_FINISH_STEPS as f64) {
        return Err(CamError::TooLarge(format!(
            "the finishing passes need more than {MAX_FINISH_STEPS} steps down the wall; use a tool with longer flutes"
        )));
    }
    let n = n as usize;
    Ok((1..=n).map(|k| if k == n { bottom } else { top - depth * k as f64 / n as f64 }).collect())
}

/// Round the pass at `z` (its tabs where they stand above `z`), leads included.
fn cut_pass(b: &mut Builder, pass: &Pass, z: f64, plan: Option<&TabPlan>) -> Result<()> {
    b.cut_loop(&pass.lead_in, z)?;
    match plan.filter(|p| z < p.top - 1e-9 && !pass.tabs.is_empty()) {
        Some(p) => cut_profile(b, &pass.lp, &p.knots(&pass.tabs, z, length(&pass.lp)))?,
        None => b.cut_loop(&pass.lp, z)?,
    }
    if let Some(first) = pass.lp.first() {
        b.cut_to(first.start(), z)?;
    }
    b.cut_loop(&pass.lead_out, z)?;
    match pass.vertical {
        Some(r) => pass.vertical_out(b, z, r),
        None => Ok(()),
    }
}

/// Whether a contour runs its loops the other way round from how they come (loops come with
/// the regions on their left: around a part that puts the wall on the left, conventional;
/// inside an opening on the right, climb).
fn flipped(params: &ContourParams) -> bool {
    let natural_is_climb = params.side != Side::Outside;
    params.climb != natural_is_climb
}

/// A loop as a contour's pass runs it: turned the way it cuts, and started in the middle of its
/// longest straight piece where something (a lead, tabs, a helix) needs room there.
fn pass_loop(raw: &[Piece], flip: bool, relocate: bool) -> Vec<Piece> {
    let lp = if flip { reversed(raw) } else { raw.to_vec() };
    if relocate { start_at(&lp, longest_middle(&lp)) } else { lp }
}

/// How the tool rises over tabs `t` of a cut whose lowest pass is at `bottom`, tool radius `r`.
fn tab_plan(t: &Tabs, r: f64, bottom: f64) -> TabPlan {
    TabPlan { plateau: t.width * 0.5 + r, top: bottom + t.height, slope: t.ramp_angle.tan(), bottom }
}

/// Whether tabs `tabs` have room round an outside contour of `regions` with `tool`, placed as
/// [`contour_with`] places them: on every loop its depth passes run, `finish` (mm: its finishing
/// passes' stepovers together) farther out than the tool's radius and the stock to leave. The
/// error says why not. For choosing tabs that fit before a contour is planned.
pub fn tabs_fit(regions: &[Region2], tool: &Tool, params: &ContourParams, finish: f64, tabs: &Tabs) -> Result<()> {
    tool.validate()?;
    if params.side != Side::Outside {
        return invalid("tabs hold a part cut out round its outside: they need the outside side");
    }
    if !(finish.is_finite() && finish >= 0.0 && params.stock_to_leave.is_finite()) {
        return invalid("the finishing passes and the stock to leave must be finite");
    }
    // Any depth above the tabs: their room along the loop does not depend on it.
    let depth = tabs.height.abs().max(1.0) * 2.0;
    tabs.validate(&Cut {
        heights: Heights { clearance: depth + 2.0, retract: depth + 1.0, top: depth, bottom: 0.0 },
        feeds: Feeds { cut: 1.0, plunge: 1.0 },
        stepdown: depth,
    })?;
    let r = tool.radius();
    let shape = Shape::from_regions(regions, params.tolerance)?;
    let loops = shape.offset(-(r + params.stock_to_leave + finish))?;
    if loops.is_empty() {
        return invalid("the tool does not fit the profile");
    }
    let plan = tab_plan(tabs, r, 0.0);
    let flip = flipped(params);
    for raw in &loops {
        place_tabs(&pass_loop(raw, flip, true), tabs, &plan, 2.0 * r)?;
    }
    Ok(())
}

/// Follows the boundaries of `regions` on the chosen side, one loop after the other, each down
/// through every depth pass, then round its finishing and spring passes, before the next.
/// `above(a, b, z)`: whether a rapid from `a` to `b` at height `z` clears the part (only asked
/// with [`Linking::StayDown`]).
pub fn contour_with(
    regions: &[Region2],
    tool: &Tool,
    params: &ContourParams,
    opts: &ContourOptions,
    cut: &Cut,
    above: &dyn Fn(Vec2, Vec2, f64) -> bool,
) -> Result<Toolpath> {
    tool.validate()?;
    cut.validate()?;
    check_leave(params.stock_to_leave)?;
    check_ramp(params.ramp_angle)?;
    let r = tool.radius();
    let entry = entry_of(opts.entry, params.ramp_angle, r)?;
    if tool.kind == ToolKind::Chamfer && params.side == Side::On {
        // A V-trace (engraving): the cone's point runs along the line to the depth.
        traces(tool, cut)?;
    } else {
        mills(tool, entry.angle())?;
    }
    if let Some(l) = &opts.lead {
        l.validate()?;
    }
    if let Some(v) = opts.vertical_lead {
        if !(MIN_ARC_RADIUS..=MAX_LEAD).contains(&v) {
            return invalid(format!("the vertical lead's radius must be from {MIN_ARC_RADIUS} to {MAX_LEAD} mm"));
        }
        if entry != Entry::Plunge {
            return invalid("a vertical lead takes the place of the entry: give no ramp or helix with it");
        }
    }
    if params.side == Side::On && (opts.lead.is_some() || opts.vertical_lead.is_some() || matches!(entry, Entry::Helix { .. })) {
        return invalid("on the profile both sides of the line are the part: a lead or a helix would cut beside it; use a ramp or a plunge");
    }
    if let Some(t) = &opts.tabs {
        if params.side != Side::Outside {
            return invalid("tabs hold a part cut out round its outside: they need the outside side");
        }
        t.validate(cut)?;
    }
    let n = opts.finish_passes;
    if n > MAX_FINISH_PASSES {
        return invalid(format!("at most {MAX_FINISH_PASSES} finishing passes"));
    }
    let fs = if n > 0 { opts.finish_stepover } else { 0.0 };
    if n > 0 {
        if params.side == Side::On {
            return invalid("finishing passes need the outside or inside side");
        }
        if !(fs > 0.0 && fs <= 0.5 * r) {
            return invalid("the finishing stepover must be above 0 and at most half the tool radius");
        }
    }
    let shape = Shape::from_regions(regions, params.tolerance)?;
    let away = if params.side == Side::On { 0.0 } else { r + params.stock_to_leave };
    let offset = |d: f64| match params.side {
        Side::Outside => shape.offset(-d),
        Side::Inside => shape.offset(d),
        Side::On => Ok(shape.loops().to_vec()),
    };
    let rough_d = away + n as f64 * fs;
    let loops = offset(rough_d)?;
    if loops.is_empty() {
        return invalid("the tool does not fit the profile");
    }
    let finishing = (0..n).map(|k| Ok((away + (n - 1 - k) as f64 * fs, offset(away + (n - 1 - k) as f64 * fs)?))).collect::<Result<Vec<_>>>()?;
    // A finishing pass takes its stepover off the wall the pass before it left. Where it reaches
    // into a notch or a gap that pass did not fit in, it would cut a full slot at full depth.
    let mut before: (f64, &[Vec<Piece>]) = (rough_d, loops.as_slice());
    for (d, set) in &finishing {
        if !set.iter().all(|lp| runs_near(lp, before.1, before.0 - d + r + params.tolerance, r)) {
            return invalid(
                "a finishing pass would reach where the depth passes did not fit and cut a full slot at full depth: use fewer finishing passes or a smaller finishing stepover",
            );
        }
        before = (*d, set.as_slice());
    }
    // Each finishing loop belongs to the depth-pass loop nearest to it.
    let owner = |lp: &[Piece]| -> usize {
        let p = lp.first().map_or(Vec2::default(), Piece::start);
        let gap = |l: &Vec<Piece>| l.iter().map(|q| q.dist(p)).fold(f64::INFINITY, f64::min);
        loops.iter().enumerate().min_by(|a, b| gap(a.1).total_cmp(&gap(b.1))).map_or(0, |(i, _)| i)
    };
    // Loops come with the regions on their left. Around a part that puts the wall on the left
    // (conventional), inside an opening on the right (climb).
    let flip = flipped(params);
    let away_left = if params.side == Side::Outside { flip } else { !flip };
    let orient = |lp: &Vec<Piece>| if flip { reversed(lp) } else { lp.clone() };
    let relocate = opts.lead.is_some() || opts.vertical_lead.is_some() || opts.tabs.is_some() || matches!(entry, Entry::Helix { .. });
    let geo = Geo { shape: &shape, side: params.side, away, r };
    let (top, bottom) = (cut.heights.top, cut.heights.bottom);
    let plan = opts.tabs.map(|t| tab_plan(&t, r, bottom));
    let down = Down { entry, geo: &geo, plan: plan.as_ref(), away_left };
    let margin = 2.0 * r;
    let levels = cut.levels()?;
    let steps = if n > 0 { finish_levels(tool, top, bottom)? } else { vec![bottom] };
    let mut b = Builder::new(cut);
    for (i, raw) in loops.iter().enumerate() {
        let lp = pass_loop(raw, flip, relocate);
        let tabs = match (&opts.tabs, &plan) {
            (Some(t), Some(p)) => place_tabs(&lp, t, p, margin)?,
            _ => Vec::new(),
        };
        let tab_points: Vec<Vec2> = tabs.iter().map(|s| at_length(&lp, *s).0).collect();
        let rough = make_pass(lp, tabs, opts.lead, opts.vertical_lead, &geo, away_left)?;
        // Down to the top (at rapid to just above it), then into the material by the entry, as
        // often as there are levels.
        hop(&mut b, rough.entry(), opts.linking, above)?;
        b.descend_to(top, top)?;
        let mut prev = top;
        for (k, &z) in levels.iter().enumerate() {
            if k > 0 && rough.leads() {
                hop(&mut b, rough.entry(), opts.linking, above)?;
                // Down to the level above, or with a vertical lead to where its arc started on
                // that level (the tool went no lower there).
                let back = prev + rough.vertical.unwrap_or(0.0);
                b.descend_to(back, back)?;
            }
            down.enter(&mut b, &rough, z)?;
            cut_pass(&mut b, &rough, z, plan.as_ref())?;
            prev = z;
        }
        let mut cut_so_far: Vec<Piece> = rough.pieces().copied().collect();
        let mut last = rough;
        // Round `pass` (`d` out from the wall) at each of `steps`, down from the top.
        let finish = |b: &mut Builder, pass: Pass, d: f64, steps: &[f64], cut_so_far: &mut Vec<Piece>| -> Result<Pass> {
            let to = pass.entry();
            for (j, &z) in steps.iter().enumerate() {
                let at_z = b.pos.filter(|q| (q.z - z).abs() <= 1e-9);
                // Back at the pass's start after the step above: straight on down.
                let on_down = j > 0 && !pass.leads() && b.at(to) && b.pos.is_some_and(|q| q.z > z);
                match at_z {
                    Some(q) if !pass.leads() && geo.link_in_band(xy(q), to, d, rough_d) => b.cut_to(to, z)?,
                    _ if on_down => b.plunge_to(z)?,
                    _ => {
                        hop(b, to, opts.linking, above)?;
                        // Down where the depth passes cut, or into the material by the entry.
                        if cut_so_far.iter().any(|piece| piece.dist(to) <= 0.9 * r) {
                            b.descend_to(z, top)?;
                        } else if entry == Entry::Plunge && pass.vertical.is_none_or(|v| z + v < top - 1e-9) {
                            // A vertical lead whose arc starts below the top plunges down to it.
                            return invalid(
                                "a finishing pass would plunge where the depth passes did not cut: give it a smaller lead, a vertical lead \
                                 that starts above the top, or an entry",
                            );
                        } else {
                            b.descend_to(top, top)?;
                            down.enter(b, &pass, z)?;
                        }
                    }
                }
                cut_pass(b, &pass, z, plan.as_ref())?;
            }
            cut_so_far.extend(pass.pieces().copied());
            Ok(pass)
        };
        for (d, set) in &finishing {
            for raw_f in set.iter().filter(|l| owner(l) == i) {
                let mut lp = orient(raw_f);
                lp = start_at(&lp, nearest_length(&lp, last.start()));
                let mut tabs = Vec::new();
                if let Some(p) = &plan {
                    let total = length(&lp);
                    let project = |lp: &[Piece]| {
                        let mut t: Vec<f64> = tab_points.iter().map(|q| nearest_length(lp, *q)).collect();
                        t.sort_by(f64::total_cmp);
                        t
                    };
                    tabs = project(&lp);
                    if !p.fits(&tabs, total, r) {
                        // Start in the middle of the widest gap between tabs instead.
                        let mut widest = (0.0, 0.0);
                        for (k, c) in tabs.iter().enumerate() {
                            let next = tabs.get(k + 1).copied().unwrap_or(tabs.first().map_or(0.0, |f| f + total));
                            if next - c > widest.0 {
                                widest = (next - c, (c + next) * 0.5);
                            }
                        }
                        lp = start_at(&lp, widest.1.rem_euclid(total.max(1e-9)));
                        tabs = project(&lp);
                        if !p.fits(&tabs, length(&lp), r) {
                            return invalid("the tabs do not fit a finishing pass: use fewer or narrower tabs");
                        }
                    }
                }
                let pass = make_pass(lp, tabs, opts.lead, opts.vertical_lead, &geo, away_left)?;
                last = finish(&mut b, pass, *d, &steps, &mut cut_so_far)?;
            }
        }
        if opts.spring_pass {
            let d = if n > 0 { away } else { rough_d };
            // At the bottom alone: it takes nothing off the wall the last pass finished.
            finish(&mut b, last, d, &[bottom], &mut cut_so_far)?;
        }
        if opts.linking == Linking::Clearance {
            b.retract()?;
        }
    }
    limit_descent(&mut b.moves, cut.feeds.plunge);
    b.finish()
}

/// Where a helix of at most `radius` fits at the start of a pocket ring, or else in the middle
/// of one of its pieces (the longest first): the ring turned to start there, the centre and
/// whether it turns counter-clockwise. `left`: the pocket is on the ring's left.
fn helix_spot(shape: &Shape, lp: &[Piece], left: bool, wall: f64, radius: f64) -> Option<(Vec<Piece>, Vec2, bool)> {
    let mut mids: Vec<(f64, f64)> = Vec::with_capacity(lp.len());
    let mut acc = 0.0;
    for piece in lp {
        mids.push((acc + piece.length() * 0.5, piece.length()));
        acc += piece.length();
    }
    mids.sort_by(|a, b| b.1.total_cmp(&a.1));
    let spots: Vec<f64> = std::iter::once(0.0).chain(mids.into_iter().map(|m| m.0)).take(MAX_HELIX_SPOTS).collect();
    for k in [1.0, 0.75, 0.5, 0.25] {
        let rr = radius * k;
        if rr < MIN_ARC_RADIUS {
            break;
        }
        for &s in &spots {
            let (p, t) = at_length(lp, s);
            let c = p + if left { t.perp() } else { -t.perp() } * rr;
            if shape.clear(c, wall + rr) {
                return Some((if s > 0.0 { start_at(lp, s) } else { lp.to_vec() }, c, left));
            }
        }
    }
    None
}

/// The order a pocket's rings are cut in at every level, each with its distance from the
/// boundary: a ring only after every ring next inside it that borders it (the material between
/// them is the ring's to cut), otherwise the nearest one first, so that the rooms of a pocket
/// are cleared one after the other rather than a ring of each in turn.
fn ring_order(rings: &[(f64, Vec<Vec<Piece>>)]) -> Vec<(f64, &Vec<Piece>)> {
    let nodes: Vec<(usize, f64, &Vec<Piece>)> = rings.iter().enumerate().flat_map(|(j, (d, ring))| ring.iter().map(move |lp| (j, *d, lp))).collect();
    let n = nodes.len();
    let mut waiting = vec![0usize; n];
    let mut outer_of: Vec<Vec<usize>> = vec![Vec::new(); n];
    // Rings border each other where they are their distances apart. A ring is checked against
    // the next two further in: the next ring past an extra one (which may run along only part
    // of it) as well as the extra one.
    let mut first = 0;
    for (j, (_, ring)) in rings.iter().enumerate() {
        let next = first + ring.len();
        for (ia, (_, da, outer)) in nodes.iter().enumerate().skip(first).take(ring.len()) {
            for (ib, (jb, db, inner)) in nodes.iter().enumerate().skip(next) {
                if *jb > j + 2 {
                    break;
                }
                let gap = (db - da) * 1.001 + 1e-6;
                let borders = inner.iter().flat_map(|p| [p.start(), p.point_at(0.5)]).any(|q| outer.iter().any(|p| p.dist(q) <= gap));
                if borders {
                    if let Some(w) = waiting.get_mut(ia) {
                        *w += 1;
                    }
                    if let Some(o) = outer_of.get_mut(ib) {
                        o.push(ia);
                    }
                }
            }
        }
        first = next;
    }
    let near = |at: Vec2, lp: &[Piece]| lp.iter().map(|p| p.start().dist(at)).fold(f64::INFINITY, f64::min);
    let mut ready: Vec<usize> = (0..n).filter(|i| waiting.get(*i) == Some(&0)).collect();
    let mut done = vec![false; n];
    let mut order = Vec::with_capacity(n);
    let mut at = Vec2::default();
    while !ready.is_empty() {
        let pick = ready
            .iter()
            .enumerate()
            .filter_map(|(k, &i)| nodes.get(i).map(|node| (k, i, near(at, node.2), node.0)))
            .min_by(|a, b| a.2.total_cmp(&b.2).then(b.3.cmp(&a.3)));
        let Some((k, i, ..)) = pick else { break };
        ready.swap_remove(k);
        let Some(&(_, d, lp)) = nodes.get(i) else { break };
        if let Some(flag) = done.get_mut(i) {
            *flag = true;
        }
        order.push((d, lp));
        at = start_near(lp, at).first().map_or(at, Piece::start);
        for &o in outer_of.get(i).map_or(&[][..], Vec::as_slice) {
            if let Some(w) = waiting.get_mut(o) {
                *w = w.saturating_sub(1);
                if *w == 0 {
                    ready.push(o);
                }
            }
        }
    }
    // Whatever is left (none: rings only border rings next to them) goes innermost first.
    for (i, &(_, d, lp)) in nodes.iter().enumerate().rev() {
        if done.get(i) == Some(&false) {
            order.push((d, lp));
        }
    }
    order
}

/// Clears `regions` (holes are islands) with rings parallel to the boundary, from the inside
/// out, entering each ring it cannot link to in the cut by `opts.entry`. `above(a, b, z)`: whether
/// a rapid from `a` to `b` at height `z` clears the part (only asked with [`Linking::StayDown`]).
pub fn pocket_with(
    regions: &[Region2],
    tool: &Tool,
    params: &PocketParams,
    opts: &PocketOptions,
    cut: &Cut,
    above: &dyn Fn(Vec2, Vec2, f64) -> bool,
) -> Result<Toolpath> {
    tool.validate()?;
    cut.validate()?;
    check_leave(params.stock_to_leave)?;
    check_ramp(params.ramp_angle)?;
    let r = tool.radius();
    let entry = entry_of(opts.entry, params.ramp_angle, r)?;
    mills(tool, entry.angle())?;
    let shape = Shape::from_regions(regions, params.tolerance)?;
    let wall = r + params.stock_to_leave;
    let rings = pocket_rings(&shape, tool, params)?;
    let stay = opts.linking == Linking::StayDown;
    let order = ring_order(&rings);
    let inward = if params.climb { 1.0 } else { -1.0 };
    let mut b = Builder::new(cut);
    // The depth already cut wherever this pass's rings run.
    let mut cleared = cut.heights.top;
    for z in cut.levels()? {
        // Where the tool is down in this level's cut.
        let mut down: Option<Vec2> = None;
        for &(d, ring) in &order {
            // Offset loops keep the pocket on their left, so the wall is on the right: climb.
            let lp = if params.climb { ring.clone() } else { reversed(ring) };
            let near = down.or(if stay { b.pos.map(xy) } else { None }).unwrap_or_default();
            let mut lp = start_near(&lp, near);
            let Some(first) = lp.first() else { continue };
            let linked = down.is_some_and(|from| link_is_clear(&shape, from, first.start(), d, r));
            if linked {
                b.cut_to(first.start(), z)?;
            } else {
                let spot = match entry {
                    Entry::Helix { radius, .. } => helix_spot(&shape, &lp, params.climb, wall, radius),
                    _ => None,
                };
                if let Some((turned, ..)) = &spot {
                    lp = turned.clone();
                }
                let start = lp.first().map_or(Vec2::default(), Piece::start);
                // From one level to the next through the level just cleared.
                let through = stay
                    && down.is_none()
                    && cleared < cut.heights.top
                    && b.pos.is_some_and(|q| (q.z - cleared).abs() <= 1e-9 && link_is_clear(&shape, xy(q), start, wall, r));
                if through {
                    b.cut_to(start, cleared)?;
                } else {
                    hop(&mut b, start, opts.linking, above)?;
                    // Through the air the levels before cut, then down into this one.
                    b.descend_to(cleared, cleared)?;
                }
                match (entry, spot) {
                    (Entry::Helix { angle, .. }, Some((_, c, ccw))) if helix(&mut b, c, ccw, z, angle)? => {}
                    (Entry::Helix { angle, .. } | Entry::Ramp { angle }, _) => b.ramp_loop(&lp, z, angle)?,
                    (Entry::Plunge, _) => b.plunge_to(z)?,
                }
            }
            let start = lp.first().map_or(Vec2::default(), Piece::start);
            // Rings far apart do not reach into a sharp corner between them: there the tool
            // steps towards the next ring's corner and back.
            for (piece, next) in lp.iter().zip(lp.iter().cycle().skip(1)) {
                b.cut_piece(piece, z, z)?;
                if let Some(to) = corner_spur(piece, next, inward, params.stepover, r).filter(|to| link_is_clear(&shape, piece.end(), *to, wall, r)) {
                    b.cut_to(to, z)?;
                    b.cut_to(piece.end(), z)?;
                }
            }
            b.cut_to(start, z)?;
            down = Some(start);
        }
        cleared = z;
        if !stay {
            b.retract()?;
        }
    }
    limit_descent(&mut b.moves, cut.feeds.plunge);
    b.finish()
}

/// Refuses a V-trace with `tool`, a chamfer mill, deeper than its cone: along the line its
/// point cuts a groove as wide as its cone is at the top.
fn traces(tool: &Tool, cut: &Cut) -> Result<()> {
    crate::chamfer::within_cone(tool, cut.heights.top - cut.heights.bottom, "trace")
}
