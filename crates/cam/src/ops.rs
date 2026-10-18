//! 2.5D operations: contours and pockets. Each one turns planar geometry
//! and cutting data into a [`Toolpath`] and checks it with [`verify`] before returning it.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};
use solidcraft_geom::{Region2, Vec2, Vec3};

use crate::check::verify;
use crate::offset::{Shape, reversed};
use crate::path::{MAX_MOVES, Move, Toolpath};
use crate::piece::Piece;
use crate::tool::{Tool, ToolKind};
use crate::{CamError, Result, invalid};

/// Most depth passes of one operation.
const MAX_LEVELS: usize = 10_000;
/// Most offset rings of one pocket.
const MAX_RINGS: usize = 2_000;
/// Most times a ramp goes round its loop; a loop too short for that is plunged into instead.
const MAX_RAMP_LAPS: f64 = 2000.0;
/// Largest coordinate or height (mm).
const MAX_COORD: f64 = 1.0e6;

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
    fn validate(&self) -> Result<()> {
        self.heights.validate()?;
        self.feeds.validate()?;
        if !(self.stepdown > 0.0) || !self.stepdown.is_finite() {
            return invalid("the stepdown must be above 0");
        }
        Ok(())
    }

    /// Depth passes from the top down to the bottom, evenly spaced and at most a stepdown apart.
    fn levels(&self) -> Result<Vec<f64>> {
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
struct Builder {
    moves: Vec<Move>,
    pos: Option<Vec3>,
    h: Heights,
    feeds: Feeds,
}

impl Builder {
    fn new(cut: &Cut) -> Builder {
        Builder { moves: Vec::new(), pos: None, h: cut.heights, feeds: cut.feeds }
    }

    fn push(&mut self, m: Move) -> Result<()> {
        if self.moves.len() >= MAX_MOVES {
            return Err(CamError::TooLarge(format!("the toolpath has more than {MAX_MOVES} moves")));
        }
        if let Some(to) = m.target() {
            self.pos = Some(to);
        }
        self.moves.push(m);
        Ok(())
    }

    fn at(&self, p: Vec2) -> bool {
        self.pos.is_some_and(|q| Vec2::new(q.x, q.y).dist(p) <= 1e-9)
    }

    /// Up to the clearance height (if below it), then across to `p`.
    fn travel_to(&mut self, p: Vec2) -> Result<()> {
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
    fn plunge_to(&mut self, z: f64) -> Result<()> {
        let Some(q) = self.pos else { return Err(CamError::Unsafe("a plunge before the first positioning move".into())) };
        if q.z > self.h.retract {
            self.push(Move::Rapid { to: Vec3::new(q.x, q.y, self.h.retract) })?;
        }
        if z < self.pos.map_or(z, |p| p.z) {
            self.push(Move::Feed { to: Vec3::new(q.x, q.y, z), feed: self.feeds.plunge })?;
        }
        Ok(())
    }

    fn cut_to(&mut self, p: Vec2, z: f64) -> Result<()> {
        if self.at(p) && self.pos.is_some_and(|q| (q.z - z).abs() <= 1e-9) {
            return Ok(());
        }
        self.push(Move::Feed { to: p.extend(z), feed: self.feeds.cut })
    }

    /// Cuts along `piece` from wherever the tool is (within the offsetting's joining tolerance
    /// of the piece's start), going evenly from depth `z0` to `z1`.
    fn cut_piece(&mut self, piece: &Piece, z0: f64, z1: f64) -> Result<()> {
        match *piece {
            Piece::Line { b, .. } => self.cut_to(b, z1),
            Piece::Arc { c, sweep, .. } => {
                // Halves, so that no arc move comes near a full turn.
                let parts = if sweep.abs() > PI { 2 } else { 1 };
                for k in 1..=parts {
                    let t = k as f64 / parts as f64;
                    let to = piece.point_at(t).extend(z0 + (z1 - z0) * t);
                    self.push(Move::Arc { to, center: c, ccw: sweep > 0.0, feed: self.feeds.cut })?;
                }
                Ok(())
            }
        }
    }

    fn cut_loop(&mut self, lp: &[Piece], z: f64) -> Result<()> {
        lp.iter().try_for_each(|piece| self.cut_piece(piece, z, z))
    }

    /// Goes down to `z` along `lp`, no steeper than `angle`, round as often as that takes; the
    /// tool ends where the loop starts. Straight down when there is no angle or the loop is too
    /// short to ramp along.
    fn ramp_loop(&mut self, lp: &[Piece], z: f64, angle: f64) -> Result<()> {
        let Some(from) = self.pos.map(|p| p.z) else { return Err(CamError::Unsafe("a ramp before the first positioning move".into())) };
        let drop = from - z;
        if !(drop > 1e-9) {
            return Ok(());
        }
        let length: f64 = lp.iter().map(Piece::length).sum();
        let laps = (drop / angle.tan() / length - 1e-9).ceil().max(1.0);
        if !(angle > 0.0) || !(length > 1e-3) || !(laps <= MAX_RAMP_LAPS) {
            return self.plunge_to(z);
        }
        let total = laps * length;
        let mut done = 0.0;
        for _ in 0..laps as usize {
            for piece in lp {
                let z0 = from - drop * done / total;
                done += piece.length();
                self.cut_piece(piece, z0, from - drop * (done / total).min(1.0))?;
            }
        }
        // Exactly at depth, whatever the rounding of the last step.
        match self.pos {
            Some(p) => self.cut_to(Vec2::new(p.x, p.y), z),
            None => Ok(()),
        }
    }

    fn retract(&mut self) -> Result<()> {
        match self.pos {
            Some(q) if q.z < self.h.clearance => self.push(Move::Rapid { to: Vec3::new(q.x, q.y, self.h.clearance) }),
            _ => Ok(()),
        }
    }

    fn finish(mut self) -> Result<Toolpath> {
        self.retract()?;
        let path = Toolpath { moves: self.moves };
        verify(&path, &self.h)?;
        Ok(path)
    }
}

fn check_ramp(angle: f64) -> Result<()> {
    if (0.0..=1.5).contains(&angle) { Ok(()) } else { invalid("the ramp angle must be between 0 and 85 degrees") }
}

fn check_leave(leave: f64) -> Result<()> {
    if leave.is_finite() && (0.0..=MAX_COORD).contains(&leave) { Ok(()) } else { invalid("the stock to leave must be 0 or more") }
}

/// A milling operation needs a tool that cuts with its side: a drill only cuts at its point.
fn mills(tool: &Tool) -> Result<()> {
    if tool.kind == ToolKind::Drill { invalid("a drill cannot mill: it cuts only at its point") } else { Ok(()) }
}

/// The loop turned to start at its corner nearest to `p`.
fn start_near(lp: &[Piece], p: Vec2) -> Vec<Piece> {
    let first = lp.iter().enumerate().min_by(|a, b| a.1.start().dist(p).total_cmp(&b.1.start().dist(p))).map_or(0, |(i, _)| i);
    lp.iter().cycle().skip(first).take(lp.len()).copied().collect()
}

/// Follows the boundaries of `regions` on the chosen side, one loop after the other, each
/// down through every depth pass before the next.
pub fn contour(regions: &[Region2], tool: &Tool, params: &ContourParams, cut: &Cut) -> Result<Toolpath> {
    tool.validate()?;
    mills(tool)?;
    cut.validate()?;
    check_leave(params.stock_to_leave)?;
    check_ramp(params.ramp_angle)?;
    let shape = Shape::from_regions(regions, params.tolerance)?;
    let away = tool.radius() + params.stock_to_leave;
    let loops = match params.side {
        Side::Outside => shape.offset(-away)?,
        Side::Inside => shape.offset(away)?,
        Side::On => shape.loops().to_vec(),
    };
    if loops.is_empty() {
        return invalid("the tool does not fit the profile");
    }
    // Loops come with the regions on their left. Around a part that puts the wall on the left
    // (conventional), inside an opening on the right (climb).
    let natural_is_climb = params.side != Side::Outside;
    let levels = cut.levels()?;
    let mut b = Builder::new(cut);
    for lp in &loops {
        let lp = if params.climb == natural_is_climb { lp.clone() } else { reversed(lp) };
        let Some(first) = lp.first() else { continue };
        b.travel_to(first.start())?;
        // Down to the last pass (through what it cut), then into the material along the loop.
        b.plunge_to(cut.heights.top)?;
        for &z in &levels {
            b.ramp_loop(&lp, z, params.ramp_angle)?;
            b.cut_loop(&lp, z)?;
            b.cut_to(first.start(), z)?;
        }
        b.retract()?;
    }
    b.finish()
}

/// Clears `regions` (holes are islands) with rings parallel to the boundary, from the inside out.
pub fn pocket(regions: &[Region2], tool: &Tool, params: &PocketParams, cut: &Cut) -> Result<Toolpath> {
    tool.validate()?;
    mills(tool)?;
    cut.validate()?;
    check_leave(params.stock_to_leave)?;
    check_ramp(params.ramp_angle)?;
    let r = tool.radius();
    if !(params.stepover > 0.0) || !(params.stepover <= tool.diameter) {
        return invalid("the stepover must be above 0 and at most the tool diameter");
    }
    let shape = Shape::from_regions(regions, params.tolerance)?;
    let wall = r + params.stock_to_leave;
    let mut rings: Vec<Vec<Vec<Piece>>> = Vec::new();
    loop {
        let ring = shape.offset(wall + params.stepover * rings.len() as f64)?;
        if ring.is_empty() {
            break;
        }
        rings.push(ring);
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
    // of the next ring's reach (corners between rings are cleared by the spurs below).
    if params.stepover > r + 1e-9 {
        let mut passes = Vec::new();
        for (k, ring) in rings.iter().enumerate() {
            passes.push(ring.clone());
            let next: Vec<&Piece> = rings.get(k + 1).into_iter().flatten().flatten().collect();
            let reached = |p: Vec2| next.iter().any(|piece| piece.dist(p) <= r + 1e-6);
            let extra: Vec<Vec<Piece>> = shape
                .offset(wall + params.stepover * k as f64 + r)?
                .into_iter()
                .filter(|lp| lp.iter().any(|piece| [0.25, 0.5, 0.75].iter().any(|t| !reached(piece.point_at(*t)))))
                .collect();
            if !extra.is_empty() {
                passes.push(extra);
            }
        }
        rings = passes;
    }
    let mut b = Builder::new(cut);
    // The depth already cut wherever this pass's rings run.
    let mut cleared = cut.heights.top;
    for z in cut.levels()? {
        // Whether the tool is down in this level's cut.
        let mut down: Option<Vec2> = None;
        for lp in rings.iter().rev().flatten() {
            // Offset loops keep the pocket on their left, so the wall is on the right: climb.
            let lp = if params.climb { lp.clone() } else { reversed(lp) };
            let lp = start_near(&lp, down.unwrap_or_default());
            let Some(first) = lp.first() else { continue };
            let start = first.start();
            match down {
                Some(from) if link_is_clear(&shape, from, start, wall, r) => b.cut_to(start, z)?,
                _ => {
                    b.travel_to(start)?;
                    b.plunge_to(cleared)?;
                    b.ramp_loop(&lp, z, params.ramp_angle)?;
                }
            }
            // Rings far apart do not reach into a sharp corner between them: there the tool
            // steps towards the next ring's corner and back.
            let inward = if params.climb { 1.0 } else { -1.0 };
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
        b.retract()?;
    }
    b.finish()
}

/// Where to step to from the corner between `piece` and `next` so that the corner between
/// this ring and the next one in (`gap` away) is cleared: along the corner's bisector, to within
/// a radius of the next ring's corner. `inward`: 1 with the pocket on the loop's left, −1 on
/// its right. `None` where the rings' own passes already clear the corner.
fn corner_spur(piece: &Piece, next: &Piece, inward: f64, gap: f64, r: f64) -> Option<Vec2> {
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

/// Whether the tool can feed straight from `a` to `b` without touching a wall.
fn link_is_clear(shape: &Shape, a: Vec2, b: Vec2, wall: f64, r: f64) -> bool {
    let n = ((a.dist(b) / (r * 0.25).max(1e-3)).ceil()).clamp(1.0, 2000.0) as usize;
    (0..=n).all(|i| shape.clear(a.lerp(b, i as f64 / n as f64), wall))
}
