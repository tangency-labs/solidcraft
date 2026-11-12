//! 2.5D operations: contours and pockets. Each one turns planar geometry
//! and cutting data into a [`Toolpath`] and checks it with [`verify`] before returning it.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};
use solidcore_record::{Region2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::check::verify;
use crate::offset::Shape;
use crate::path::{MAX_MOVES, Move, Toolpath};
use crate::piece::Piece;
use crate::surface_ops::{ENTRY_GAP, descent_feed};
use crate::tool::{Tool, ToolKind};
use crate::{CamError, Result, invalid};

/// Most depth passes of one operation.
const MAX_LEVELS: usize = 10_000;
/// Most offset rings of one pocket.
const MAX_RINGS: usize = 2_000;
/// Most times a ramp goes round its loop; a loop too short for that is plunged into instead.
pub(crate) const MAX_RAMP_LAPS: f64 = 2000.0;
/// Largest coordinate or height (mm).
pub(crate) const MAX_COORD: f64 = 1.0e6;

/// The Z levels of an operation (work coordinates, mm).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Heights {
    /// Where the tool travels between cuts.
    pub clearance: f64,
    /// Where rapid motion towards the material stops and feed begins.
    pub retract: f64,
    /// Top of the material to cut.
    pub top: f64,
    /// Final depth.
    pub bottom: f64,
}

impl Heights {
    pub fn validate(&self) -> Result<()> {
        let all = [self.clearance, self.retract, self.top, self.bottom];
        if !all.iter().all(|z| z.is_finite() && z.abs() <= MAX_COORD) {
            return invalid("the heights must be finite numbers");
        }
        if self.clearance < self.retract || self.retract < self.top || self.top < self.bottom {
            return invalid("the heights must be ordered: clearance ≥ retract ≥ top ≥ bottom");
        }
        Ok(())
    }
}

/// Feed rates (mm/min).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Feeds {
    pub cut: f64,
    pub plunge: f64,
}

impl Feeds {
    pub fn validate(&self) -> Result<()> {
        if [self.cut, self.plunge].iter().all(|f| *f > 0.0 && *f <= 1.0e6) { Ok(()) } else { invalid("the feed rates must be above 0") }
    }
}

/// What every operation needs to cut.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cut {
    pub heights: Heights,
    pub feeds: Feeds,
    /// Most material taken per depth pass (mm).
    pub stepdown: f64,
}

impl Cut {
    pub(crate) fn validate(&self) -> Result<()> {
        self.heights.validate()?;
        self.feeds.validate()?;
        if !(self.stepdown > 0.0) || !self.stepdown.is_finite() {
            return invalid("the stepdown must be above 0");
        }
        Ok(())
    }

    /// Depth passes from the top down to the bottom, evenly spaced and at most a stepdown apart.
    pub(crate) fn levels(&self) -> Result<Vec<f64>> {
        let (top, bottom) = (self.heights.top, self.heights.bottom);
        let n = ((top - bottom) / self.stepdown - 1e-9).ceil().max(1.0);
        if !(n <= MAX_LEVELS as f64) {
            return Err(CamError::TooLarge(format!("more than {MAX_LEVELS} depth passes")));
        }
        let n = n as usize;
        Ok((1..=n).map(|k| if k == n { bottom } else { top - (top - bottom) * k as f64 / n as f64 }).collect())
    }
}

/// Which side of a profile the tool runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// Around the regions: the regions are the part that stays.
    Outside,
    /// Inside the regions' boundary: the regions are the opening.
    Inside,
    /// Tool centre on the boundary.
    On,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContourParams {
    pub side: Side,
    /// Climb milling (with a clockwise spindle the wall is on the right of the tool's travel);
    /// otherwise conventional.
    pub climb: bool,
    /// Material left on the wall (mm).
    pub stock_to_leave: f64,
    /// Chord error allowed on spline boundaries (mm).
    pub tolerance: f64,
    /// Steepest descent into each depth pass, along the profile (radians); 0 plunges straight down.
    pub ramp_angle: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PocketParams {
    /// Distance between passes (mm), at most the tool diameter.
    pub stepover: f64,
    pub climb: bool,
    /// Material left on the walls (mm).
    pub stock_to_leave: f64,
    /// Chord error allowed on spline boundaries (mm).
    pub tolerance: f64,
    /// Steepest descent into uncut material, along the first pass (radians); 0 plunges straight down.
    pub ramp_angle: f64,
}

/// Collects moves, keeping the tool's position.
pub(crate) struct Builder {
    pub(crate) moves: Vec<Move>,
    pub(crate) pos: Option<Vec3>,
    pub(crate) h: Heights,
    pub(crate) feeds: Feeds,
}

impl Builder {
    pub(crate) fn new(cut: &Cut) -> Builder {
        Builder { moves: Vec::new(), pos: None, h: cut.heights, feeds: cut.feeds }
    }

    pub(crate) fn push(&mut self, m: Move) -> Result<()> {
        if self.moves.len() >= MAX_MOVES {
            return Err(CamError::TooLarge(format!("the toolpath has more than {MAX_MOVES} moves")));
        }
        if let Some(to) = m.target() {
            self.pos = Some(to);
        }
        self.moves.push(m);
        Ok(())
    }

    pub(crate) fn at(&self, p: Vec2) -> bool {
        self.pos.is_some_and(|q| Vec2::new(q.x, q.y).dist(p) <= 1e-9)
    }

    /// Up to the clearance height (if below it), then across to `p`.
    pub(crate) fn travel_to(&mut self, p: Vec2) -> Result<()> {
        if let Some(q) = self.pos {
            if self.at(p) && q.z >= self.h.clearance {
                return Ok(());
            }
            if q.z < self.h.clearance {
                self.push(Move::Rapid { to: Vec3::new(q.x, q.y, self.h.clearance) })?;
            }
        }
        self.push(Move::Rapid { to: p.extend(self.h.clearance) })
    }

    /// Straight down to `z`: rapid to the retract height, then at the plunge feed.
    pub(crate) fn plunge_to(&mut self, z: f64) -> Result<()> {
        let Some(q) = self.pos else { return Err(CamError::Unsafe("a plunge before the first positioning move".into())) };
        if q.z > self.h.retract {
            self.push(Move::Rapid { to: Vec3::new(q.x, q.y, self.h.retract) })?;
        }
        if z < self.pos.map_or(z, |p| p.z) {
            self.push(Move::Feed { to: Vec3::new(q.x, q.y, z), feed: self.feeds.plunge })?;
        }
        Ok(())
    }

    /// Down to `z` where the material left reaches no higher than `top`: at rapid to the
    /// retract height and on through the air to [`ENTRY_GAP`] above `top` (where that is
    /// lower), then at the plunge feed.
    pub(crate) fn descend_to(&mut self, z: f64, top: f64) -> Result<()> {
        let Some(q) = self.pos else { return Err(CamError::Unsafe("a plunge before the first positioning move".into())) };
        let air = (top + ENTRY_GAP).max(z).min(self.h.retract);
        if q.z > air + 1e-9 {
            self.push(Move::Rapid { to: Vec3::new(q.x, q.y, air) })?;
        }
        self.plunge_to(z)
    }

    pub(crate) fn cut_to(&mut self, p: Vec2, z: f64) -> Result<()> {
        if self.at(p) && self.pos.is_some_and(|q| (q.z - z).abs() <= 1e-9) {
            return Ok(());
        }
        self.push(Move::Feed { to: p.extend(z), feed: self.feeds.cut })
    }

    /// Cuts along `piece` from wherever the tool is (within the offsetting's joining tolerance
    /// of the piece's start), going evenly from depth `z0` to `z1`: at the cut feed, slowed
    /// where it goes down so the tool goes down no faster than the plunge feed.
    pub(crate) fn cut_piece(&mut self, piece: &Piece, z0: f64, z1: f64) -> Result<()> {
        let drop = z0 - z1;
        let feed = descent_feed(&self.feeds, piece.length().hypot(drop), drop);
        match *piece {
            Piece::Line { b, .. } => {
                if self.at(b) && self.pos.is_some_and(|q| (q.z - z1).abs() <= 1e-9) {
                    return Ok(());
                }
                self.push(Move::Feed { to: b.extend(z1), feed })
            }
            Piece::Arc { c, sweep, .. } => {
                // Halves, so that no arc move comes near a full turn.
                let parts = if sweep.abs() > PI { 2 } else { 1 };
                for k in 1..=parts {
                    let t = k as f64 / parts as f64;
                    let to = piece.point_at(t).extend(z0 + (z1 - z0) * t);
                    self.push(Move::Arc { to, center: c, ccw: sweep > 0.0, feed })?;
                }
                Ok(())
            }
        }
    }

    pub(crate) fn cut_loop(&mut self, lp: &[Piece], z: f64) -> Result<()> {
        lp.iter().try_for_each(|piece| self.cut_piece(piece, z, z))
    }

    /// Goes down to `z` along `lp` at `angle`, round as often as that takes, then on round at
    /// `z` to where the loop starts, where the tool ends. Each ramping move goes down no faster
    /// than the plunge feed. Straight down when there is no angle or the loop is too short to
    /// ramp along.
    pub(crate) fn ramp_loop(&mut self, lp: &[Piece], z: f64, angle: f64) -> Result<()> {
        let Some(from) = self.pos.map(|p| p.z) else { return Err(CamError::Unsafe("a ramp before the first positioning move".into())) };
        let drop = from - z;
        if !(drop > 1e-9) {
            return Ok(());
        }
        let length: f64 = lp.iter().map(Piece::length).sum();
        // The distance along the loop the ramp takes.
        let run = drop / angle.tan();
        let laps = (run / length - 1e-9).ceil().max(1.0);
        if !(angle > 0.0) || !(length > 1e-3) || !(laps <= MAX_RAMP_LAPS) || !(run > 0.0) {
            return self.plunge_to(z);
        }
        let depth_at = |d: f64| from - drop * (d / run).min(1.0);
        let mut done = 0.0;
        for _ in 0..laps as usize {
            for piece in lp {
                let (d0, l) = (done, piece.length());
                done += l;
                if d0 >= run || l <= 1e-12 {
                    self.cut_piece(piece, z, z)?;
                } else if done <= run {
                    self.cut_piece(piece, depth_at(d0), depth_at(done))?;
                } else {
                    // The ramp reaches the depth part way along this piece.
                    let t = (run - d0) / l;
                    self.cut_piece(&piece.sub(0.0, t), depth_at(d0), z)?;
                    self.cut_piece(&piece.sub(t, 1.0), z, z)?;
                }
            }
        }
        // Exactly at depth, whatever the rounding of the last step.
        match self.pos {
            Some(p) => self.cut_to(Vec2::new(p.x, p.y), z),
            None => Ok(()),
        }
    }

    pub(crate) fn retract(&mut self) -> Result<()> {
        match self.pos {
            Some(q) if q.z < self.h.clearance => self.push(Move::Rapid { to: Vec3::new(q.x, q.y, self.h.clearance) }),
            _ => Ok(()),
        }
    }

    pub(crate) fn finish(mut self) -> Result<Toolpath> {
        self.retract()?;
        let path = Toolpath { moves: self.moves };
        verify(&path, &self.h)?;
        Ok(path)
    }
}

pub(crate) fn check_ramp(angle: f64) -> Result<()> {
    if (0.0..=1.5).contains(&angle) { Ok(()) } else { invalid("the ramp angle must be between 0 and 85 degrees") }
}

pub(crate) fn check_leave(leave: f64) -> Result<()> {
    if leave.is_finite() && (0.0..=MAX_COORD).contains(&leave) { Ok(()) } else { invalid("the stock to leave must be 0 or more") }
}

/// A milling operation needs a tool that cuts with a straight side: a drill only cuts at its
/// point, a thread mill's teeth only cut threads, and a chamfer mill's tapered side cuts
/// chamfers (see [`crate::chamfer`]). A face mill goes into the material only along a ramp
/// (`ramp_angle`).
pub(crate) fn mills(tool: &Tool, ramp_angle: f64) -> Result<()> {
    cuts_sideways(tool)?;
    match tool.kind {
        ToolKind::Chamfer => invalid(
            "a chamfer mill's side is tapered: it cuts chamfers (the chamfer operation), traces a line to a depth (a contour `side: on`) and spots holes (drill), but does not cut beside a contour, pocket or bore",
        ),
        _ => crate::chamfer::enters(tool, ramp_angle),
    }
}

/// A tool that cuts with its side at all: not a drill (it cuts only at its point) and not a
/// thread mill (its teeth only cut threads). Surface operations take any other kind; the
/// stricter [`mills`] is for 2D operations.
pub(crate) fn cuts_sideways(tool: &Tool) -> Result<()> {
    match tool.kind {
        ToolKind::Drill => invalid("a drill cannot mill: it cuts only at its point"),
        ToolKind::ThreadMill => invalid("a thread mill cuts threads only: use it in a thread milling operation"),
        _ => Ok(()),
    }
}

/// The loop turned to start at its corner nearest to `p`.
pub(crate) fn start_near(lp: &[Piece], p: Vec2) -> Vec<Piece> {
    let first = lp.iter().enumerate().min_by(|a, b| a.1.start().dist(p).total_cmp(&b.1.start().dist(p))).map_or(0, |(i, _)| i);
    lp.iter().cycle().skip(first).take(lp.len()).copied().collect()
}

/// Follows the boundaries of `regions` on the chosen side, one loop after the other, each
/// down through every depth pass before the next. [`crate::contour_with`] adds leads, entries,
/// tabs and finishing passes.
pub fn contour(regions: &[Region2], tool: &Tool, params: &ContourParams, cut: &Cut) -> Result<Toolpath> {
    crate::leads::contour_with(regions, tool, params, &crate::leads::ContourOptions::default(), cut, &|_, _, _| false)
}

/// Clears `regions` (holes are islands) with rings parallel to the boundary, from the inside out.
/// [`crate::pocket_with`] adds entries and linking choices.
pub fn pocket(regions: &[Region2], tool: &Tool, params: &PocketParams, cut: &Cut) -> Result<Toolpath> {
    crate::leads::pocket_with(regions, tool, params, &crate::leads::PocketOptions::default(), cut, &|_, _, _| false)
}

/// The passes of a pocket, outermost first, each with its distance from the boundary: rings
/// `stepover` apart from a tool radius (plus the stock to leave) in, and where rings are more
/// than a radius apart, extra rings a radius further in.
pub(crate) fn pocket_rings(shape: &Shape, tool: &Tool, params: &PocketParams) -> Result<Vec<(f64, Vec<Vec<Piece>>)>> {
    let r = tool.radius();
    if !(params.stepover > 0.0) || !(params.stepover <= tool.diameter) {
        return invalid("the stepover must be above 0 and at most the tool diameter");
    }
    let wall = r + params.stock_to_leave;
    let mut rings: Vec<(f64, Vec<Vec<Piece>>)> = Vec::new();
    loop {
        let d = wall + params.stepover * rings.len() as f64;
        let ring = shape.offset(d)?;
        if ring.is_empty() {
            break;
        }
        rings.push((d, ring));
        if rings.len() > MAX_RINGS {
            return Err(CamError::TooLarge(format!("the pocket needs more than {MAX_RINGS} passes; use a larger stepover")));
        }
    }
    if rings.is_empty() {
        return invalid("the tool does not fit the pocket");
    }
    // Rings more than a radius apart leave material wherever the next ring in does not run
    // within a radius: the middle of a part of the pocket it no longer fits in, a passage that
    // narrows. A ring a radius further in clears it; it is cut where some stretch of it is out
    // of the next ring's reach (corners between rings are cleared by the spurs).
    if params.stepover > r + 1e-9 {
        let mut passes = Vec::new();
        for (k, (d, ring)) in rings.iter().enumerate() {
            passes.push((*d, ring.clone()));
            let next: Vec<&Piece> = rings.get(k + 1).into_iter().flat_map(|(_, ring)| ring.iter().flatten()).collect();
            let reached = |p: Vec2| next.iter().any(|piece| piece.dist(p) <= r + 1e-6);
            let extra: Vec<Vec<Piece>> = shape
                .offset(d + r)?
                .into_iter()
                .filter(|lp| lp.iter().any(|piece| [0.25, 0.5, 0.75].iter().any(|t| !reached(piece.point_at(*t)))))
                .collect();
            if !extra.is_empty() {
                passes.push((d + r, extra));
            }
        }
        rings = passes;
    }
    Ok(rings)
}

/// Where to step to from the corner between `piece` and `next` so that the corner between
/// this ring and the next one in (`gap` away) is cleared: along the corner's bisector, to within
/// a radius of the next ring's corner. `inward`: 1 with the pocket on the loop's left, −1 on
/// its right. `None` where the rings' own passes already clear the corner.
pub(crate) fn corner_spur(piece: &Piece, next: &Piece, inward: f64, gap: f64, r: f64) -> Option<Vec2> {
    let (t_in, t_out) = (piece.tangent_at(1.0), next.tangent_at(0.0));
    let bend = crate::piece::turn(t_in, t_out);
    // Half the corner's inside angle has this sine.
    let sharp = (bend.abs() * 0.5).cos();
    if bend * inward <= 1e-6 || sharp < 0.05 || gap <= r * (1.0 + sharp) + 1e-9 {
        return None;
    }
    let dir = (t_out - t_in).normalized()?;
    Some(piece.end() + dir * (gap / sharp - r))
}

/// Whether the tool can feed straight from `a` to `b` staying at least `wall` from the
/// boundary. With `wall` the distance of the pass the link leads to, every point of the link
/// is where the passes cut so far (or the one it leads to) have taken the material away.
pub(crate) fn link_is_clear(shape: &Shape, a: Vec2, b: Vec2, wall: f64, r: f64) -> bool {
    let n = ((a.dist(b) / (r * 0.25).max(1e-3)).ceil()).clamp(1.0, 2000.0) as usize;
    (0..=n).all(|i| shape.clear(a.lerp(b, i as f64 / n as f64), wall))
}
