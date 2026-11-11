//! 2D pockets and contours wrapped onto a cylinder round the 4th axis ([`wrap_pocket`],
//! [`wrap_contour`]): a profile drawn in unrolled coordinates, x along the rotary axis and y the
//! arc length round a cylinder of radius R, cut into that cylinder in depth levels with the tool
//! pointing at the axis (radial), as [`crate::rotary`]'s convention has it (`A = y / R`).
//!
//! Unrolling distorts with depth. A profile drawn at radius R keeps its angles as it goes down:
//! a wall at arc length y is the radial plane at angle y / R, whose arc length at a level of
//! radius ρ is y·ρ/R. A tool of radius r there must have its centre an arc length r from the
//! wall *at ρ*, which is an angle r/ρ: wider in angle the deeper the level. Offsetting once at
//! R and wrapping the result at every depth would leave walls that lean out (cut too far round)
//! by r·(1/ρ − 1/R) of angle at the floor. So every level is planned on its own: the profile's
//! y is scaled by ρ/R (the unrolled plane at the cutting radius, where lengths are true), the
//! 2D pocket or contour is offset there, and the result is wrapped at ρ. Along x nothing is
//! distorted.
//!
//! What a radial tool leaves against a radial wall: the tool's side is parallel to its axis,
//! so above the tip it is farther in angle from the wall than at the tip (its angle from the
//! centre is atan(r/ρ') at radius ρ' ≥ ρ, which shrinks as ρ' grows): it never cuts the wall
//! above the level, and each level leaves a step of at most a stepdown. At the tip the side
//! is at atan(r/ρ) ≤ r/ρ round, so the wall is left at most r − ρ·atan(r/ρ) ≈ r³/(3ρ²) short
//! (0.02 mm for a 6 mm tool at radius 20): never cut into.
//!
//! Lines stay exact; arcs and curves of the profile and the arcs of the offset paths are
//! followed in chords (a quarter of the tolerance each), and the wrapped moves keep within half
//! of it of the true helices, every cut turning the axis by at most the angle step, so the
//! post writes them as short moves (in inverse time where it says so). Everything cut at one
//! level must fit within one turn at that level.

use std::f64::consts::{PI, TAU};

use serde::{Deserialize, Serialize};
use solidcore_record::{Loop2, Region2, Seg2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::multiaxis::Toolpath5;
use crate::ops::{ContourParams, Cut, Feeds, Heights, PocketParams, Side, contour, pocket};
use crate::path::{MAX_MOVES, Move, Toolpath};
use crate::rotary::{DEFAULT_MAX_ANGLE_STEP, wrap_with_step};
use crate::surface_ops::{check_stepdown, check_tolerance, levels};
use crate::tool::Tool;
use crate::{CamError, Result, invalid};

/// Largest radius, depth or coordinate (mm).
const MAX_COORD: f64 = 1.0e6;
/// Most depth levels of one wrapped operation: each is offset on its own.
pub const MAX_WRAP_LEVELS: usize = 1000;
/// Most profile points after curves are followed in chords.
const MAX_PROFILE_POINTS: usize = 100_000;

fn default_angle_step() -> f64 {
    DEFAULT_MAX_ANGLE_STEP
}

/// The cylinder, depths and motion every wrapped operation shares.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WrapCut {
    /// Radius of the cylinder the profile is drawn on (mm): the profile's y is the arc length
    /// round it.
    pub radius: f64,
    /// Depth of the top of the material from the cylinder (mm; 0 on it, negative into it).
    pub top: f64,
    /// Depth of the floor from the cylinder (mm, negative into it): the last level.
    pub bottom: f64,
    /// Most depth per level (mm).
    pub stepdown: f64,
    /// Heights above the cylinder (mm): rapids go round at `radius + clearance`, and the tool
    /// comes in from `radius + retract` at the plunge feed.
    pub clearance: f64,
    pub retract: f64,
    pub feeds: Feeds,
    /// Largest distance of the path from the true wrapped profile (mm).
    pub tolerance: f64,
    /// Largest turn of the tool axis in one move (radians).
    #[serde(default = "default_angle_step")]
    pub max_angle_step: f64,
}

/// A pocket wrapped onto the cylinder: [`crate::pocket`] at every level.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WrapPocketParams {
    pub cut: WrapCut,
    /// Distance between passes at the cutting radius (mm), at most the tool diameter.
    pub stepover: f64,
    pub climb: bool,
    /// Material left on the walls (mm).
    pub stock_to_leave: f64,
    /// Steepest descent into uncut material along the first pass (radians); 0 plunges.
    pub ramp_angle: f64,
}

/// A contour wrapped onto the cylinder: [`crate::contour`] at every level.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WrapContourParams {
    pub cut: WrapCut,
    pub side: Side,
    pub climb: bool,
    /// Material left on the wall (mm).
    pub stock_to_leave: f64,
    /// Steepest descent into each level along the profile (radians); 0 plunges.
    pub ramp_angle: f64,
}

impl WrapCut {
    pub fn validate(&self) -> Result<()> {
        if !(self.radius > 0.0 && self.radius <= MAX_COORD) {
            return invalid("the wrapping radius must be above 0 and at most 1000 m");
        }
        let all = [self.top, self.bottom, self.clearance, self.retract];
        if !all.iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD) {
            return invalid("the depths and heights must be finite numbers");
        }
        if !(self.clearance > self.retract && self.retract >= self.top && self.top >= self.bottom) {
            return invalid("the heights must be ordered: clearance > retract ≥ top ≥ bottom (depths from the cylinder, negative into it)");
        }
        if !(self.clearance <= 1000.0) {
            return invalid("the clearance must be at most 1000 mm above the cylinder");
        }
        if !(self.radius + self.bottom > 0.0) {
            return invalid("the floor must be outside the rotary axis: `bottom` above minus the radius");
        }
        check_stepdown(self.stepdown)?;
        check_tolerance(self.tolerance)?;
        self.feeds.validate()?;
        if !(self.max_angle_step.is_finite() && (1e-4..=PI / 4.0).contains(&self.max_angle_step)) {
            return invalid("the angle step must be from 0.0001 radians to 45 degrees");
        }
        Ok(())
    }

    /// The depths of the levels, from the first below `top` down to `bottom`.
    fn levels(&self) -> Result<Vec<f64>> {
        let n = ((self.top - self.bottom) / self.stepdown - 1e-9).ceil();
        if !(n <= MAX_WRAP_LEVELS as f64) {
            return Err(CamError::TooLarge(format!("more than {MAX_WRAP_LEVELS} levels; use a larger stepdown")));
        }
        levels(self.top, self.bottom, self.stepdown, false)
    }
}

/// The profile as polygons (lines kept, every other curve in chords within `tol`).
fn polygons(regions: &[Region2], tol: f64) -> Result<Vec<Region2>> {
    let mut count = 0usize;
    let mut lp = |l: &Loop2| -> Result<Loop2> {
        let mut segs = Vec::with_capacity(l.segs.len());
        for seg in &l.segs {
            let before = segs.len();
            match *seg {
                Seg2::Line { a, b } => segs.push(Seg2::Line { a, b }),
                _ => {
                    for w in seg.polyline(tol).windows(2) {
                        if let [a, b] = *w {
                            segs.push(Seg2::Line { a, b });
                        }
                    }
                }
            }
            count = count.saturating_add(segs.len() - before);
            if count > MAX_PROFILE_POINTS {
                return Err(CamError::TooLarge(format!("the profiles have more than {MAX_PROFILE_POINTS} points")));
            }
        }
        Ok(Loop2 { segs })
    };
    let mut out = Vec::with_capacity(regions.len());
    for r in regions {
        let outer = lp(&r.outer)?;
        let holes = r.holes.iter().map(&mut lp).collect::<Result<Vec<_>>>()?;
        out.push(Region2 { outer, holes });
    }
    Ok(out)
}

/// The polygons with every y scaled by `k`.
fn scaled(regions: &[Region2], k: f64) -> Vec<Region2> {
    let s = |p: Vec2| Vec2::new(p.x, p.y * k);
    let lp = |l: &Loop2| Loop2 {
        segs: l
            .segs
            .iter()
            .map(|seg| match *seg {
                Seg2::Line { a, b } => Seg2::Line { a: s(a), b: s(b) },
                other => other,
            })
            .collect(),
    };
    regions.iter().map(|r| Region2 { outer: lp(&r.outer), holes: r.holes.iter().map(lp).collect() }).collect()
}

/// The extent of the polygons along y (arc length at the cylinder).
fn y_extent(regions: &[Region2]) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for r in regions {
        for l in std::iter::once(&r.outer).chain(&r.holes) {
            for seg in &l.segs {
                if let Seg2::Line { a, b } = *seg {
                    lo = lo.min(a.y).min(b.y);
                    hi = hi.max(a.y).max(b.y);
                }
            }
        }
    }
    (lo <= hi).then_some((lo, hi))
}

/// Plans `level` (a 2D operation on the unrolled plane at a level's radius) at every level and
/// wraps the result.
fn wrapped(regions: &[Region2], tool: &Tool, cut: &WrapCut, level: &dyn Fn(&[Region2], &Cut) -> Result<Toolpath>) -> Result<Toolpath5> {
    cut.validate()?;
    tool.validate()?;
    if regions.is_empty() {
        return invalid("there is no profile to machine");
    }
    let tol = cut.tolerance;
    let flat = polygons(regions, 0.25 * tol)?;
    let Some((lo, hi)) = y_extent(&flat) else { return invalid("there is no profile to machine") };
    if !(lo.is_finite() && hi.is_finite()) {
        return invalid("a profile has a coordinate that is not a finite number");
    }
    let turn = TAU * cut.radius;
    if hi - lo > turn + 1e-9 {
        return invalid(format!(
            "the profiles reach {:.3} mm round the cylinder, more than once round ({turn:.3} mm at radius {})",
            hi - lo,
            cut.radius
        ));
    }
    let mut moves: Vec<Move> = Vec::new();
    for z in cut.levels()? {
        let rho = cut.radius + z;
        let k = rho / cut.radius;
        // Every level is entered from the top of the material, not from the level above: the
        // levels above are cleared only where their own cuts ran, and the tool goes up to the
        // clearance between cuts, so a rapid back down to the level above would come down
        // through material wherever the next cut starts beyond them (the stock round the
        // pocket's ends, a corner the level above left). The tool rapids no lower than the
        // top (the entry gap above it) and feeds the rest of the way down.
        let heights = Heights { clearance: cut.clearance, retract: cut.retract, top: cut.top, bottom: z };
        // One level: a stepdown deeper than the level's depth.
        let one = Cut { heights, feeds: cut.feeds, stepdown: (cut.top - z).max(0.0) + 1.0 };
        let path = level(&scaled(&flat, k), &one)?;
        // What this level cuts must fit in one turn at its radius.
        let (mut ylo, mut yhi) = (f64::INFINITY, f64::NEG_INFINITY);
        for (p, cutting) in path.points(0.25 * tol) {
            if cutting {
                ylo = ylo.min(p.y);
                yhi = yhi.max(p.y);
            }
        }
        if ylo <= yhi && yhi - ylo + tool.diameter > TAU * rho + 1e-9 {
            return invalid(format!(
                "at depth {z} the cut reaches {:.3} mm round the cylinder, more than once round ({:.3} mm at radius {rho})",
                yhi - ylo + tool.diameter,
                TAU * rho
            ));
        }
        unscale(&path, k, 0.25 * tol, &mut moves)?;
    }
    wrap_with_step(&Toolpath { moves }, cut.radius, 0.5 * tol, cut.max_angle_step)
}

/// `path` (on the unrolled plane at a level, y scaled by `k`) with its y back at the cylinder's
/// radius, arcs followed in chords within `tol`, appended to `out`.
fn unscale(path: &Toolpath, k: f64, tol: f64, out: &mut Vec<Move>) -> Result<()> {
    let back = |p: Vec3| Vec3::new(p.x, p.y / k, p.z);
    let push = |m: Move, out: &mut Vec<Move>| {
        if out.len() >= MAX_MOVES {
            return Err(CamError::TooLarge(format!("the toolpath has more than {MAX_MOVES} moves")));
        }
        out.push(m);
        Ok(())
    };
    let mut pos: Option<Vec3> = None;
    for m in &path.moves {
        match *m {
            Move::Rapid { to } => push(Move::Rapid { to: back(to) }, out)?,
            Move::Feed { to, feed } => push(Move::Feed { to: back(to), feed }, out)?,
            Move::Arc { feed, .. } => {
                let Some(from) = pos else { return invalid("a toolpath must not start with an arc") };
                let one = Toolpath { moves: vec![Move::Rapid { to: from }, *m] };
                for (p, _) in one.points(tol).into_iter().skip(1) {
                    push(Move::Feed { to: back(p), feed }, out)?;
                }
            }
            Move::Dwell { seconds } => push(Move::Dwell { seconds }, out)?,
        }
        if let Some(to) = m.target() {
            pos = Some(to);
        }
    }
    Ok(())
}

/// Clears `regions` (drawn in unrolled coordinates: x along the rotary axis, y the arc length
/// round the cylinder of `p.cut.radius`; holes are islands) into the cylinder, level by level
/// from `top` to `bottom`, the tool radial: at each level a 2D pocket is planned on the unrolled
/// plane at that level's radius (see the module notes) and wrapped. The floor is the cylinder
/// of radius `radius + bottom`, exactly.
pub fn wrap_pocket(regions: &[Region2], tool: &Tool, p: &WrapPocketParams) -> Result<Toolpath5> {
    let tol = p.cut.tolerance;
    let params =
        PocketParams { stepover: p.stepover, climb: p.climb, stock_to_leave: p.stock_to_leave, tolerance: 0.25 * tol, ramp_angle: p.ramp_angle };
    wrapped(regions, tool, &p.cut, &|r, c| pocket(r, tool, &params, c))
}

/// Follows the boundaries of `regions` (unrolled, as for [`wrap_pocket`]) on the chosen side,
/// wrapped into the cylinder, every loop at each level from `top` to `bottom` before the next
/// level, each level offset at its own radius.
pub fn wrap_contour(regions: &[Region2], tool: &Tool, p: &WrapContourParams) -> Result<Toolpath5> {
    let tol = p.cut.tolerance;
    let params = ContourParams { side: p.side, climb: p.climb, stock_to_leave: p.stock_to_leave, tolerance: 0.25 * tol, ramp_angle: p.ramp_angle };
    wrapped(regions, tool, &p.cut, &|r, c| contour(r, tool, &params, c))
}
