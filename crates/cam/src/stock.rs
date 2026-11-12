//! Stock and facing: the raw material a job starts from, and the operation that faces its top
//! flat.
//!
//! A [`Stock`] is a box or a cylinder about work X (round bar on a 4th axis), in work
//! coordinates. It is what the program may not rapid through: [`Stock::intrusion`] says how
//! deep a cutter, taken as a cylinder of its radius endless along its axis above the tip (the
//! shank and spindle are there too), is inside it. Nothing tracks what earlier cuts removed, so
//! the stock as given is the only safe bound.
//!
//! [`face`] cuts the stock's top (or any rectangle) down to a depth in levels, in straight
//! passes along an angle a stepover apart. Every pass starts and ends with the whole cutter
//! clear of the area by [`FACE_LEAD`], so the tool goes down to each level beside the stock and
//! enters it at depth, never plunging into material; the outermost passes reach past the edges
//! by the overhang, so the edges come out clean. Zigzag passes link at depth outside the area;
//! one-way passes rise to the retract height between passes.

use serde::{Deserialize, Serialize};
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::Aabb3;
use solidcraft_geom::prelude::*;

use crate::check::verify;
use crate::multiaxis::Pose;
use crate::ops::{Feeds, Heights};
use crate::path::{MAX_MOVES, Move, Toolpath, vertical_quarter};
use crate::tool::{Tool, ToolKind};
use crate::{CamError, Result, invalid};

/// Largest coordinate, size or margin accepted (mm).
const MAX_COORD: f64 = 1.0e6;
/// How far every facing pass starts and ends beyond the cutter touching the area (mm).
pub const FACE_LEAD: f64 = 1.0;
/// What the default overhang adds to the tool radius (mm): the outermost passes' centres lie
/// this far outside the edges.
pub const FACE_MARGIN: f64 = 0.5;
/// Most passes of one facing level, and most levels.
const MAX_PASSES: usize = 100_000;
const MAX_LEVELS: usize = 10_000;
/// Golden-section steps that find the least of a convex function (to about 1e-17 of the range).
const CONVEX_STEPS: usize = 80;

/// The raw material, in work coordinates (mm).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stock {
    /// A box from its lowest to its highest corner.
    Box { min: Vec3, max: Vec3 },
    /// A round bar about work X (the A axis of a 4th axis set up the usual way), from `x_min`
    /// to `x_max`.
    Cylinder { radius: f64, x_min: f64, x_max: f64 },
}

fn finite(v: f64) -> bool {
    v.is_finite() && v.abs() <= MAX_COORD
}

impl Stock {
    pub fn validate(&self) -> Result<()> {
        match *self {
            Stock::Box { min, max } => {
                if ![min.x, min.y, min.z, max.x, max.y, max.z].into_iter().all(finite) {
                    return invalid("the stock's corners must be finite numbers within 1000 m");
                }
                if !(min.x < max.x && min.y < max.y && min.z < max.z) {
                    return invalid("the stock box's `min` must be below its `max` along every axis");
                }
            }
            Stock::Cylinder { radius, x_min, x_max } => {
                if !(radius > 0.0) || !finite(radius) {
                    return invalid("the stock cylinder's radius must be above 0 and at most 1000 m");
                }
                if !finite(x_min) || !finite(x_max) || !(x_min < x_max) {
                    return invalid("the stock cylinder's `x_min` must be below its `x_max`, both finite");
                }
            }
        }
        Ok(())
    }

    /// The box round `part` (a non-empty extent) grown by `margin` on every side.
    pub fn around(part: &Aabb3, margin: Vec3) -> Result<Stock> {
        if ![margin.x, margin.y, margin.z].iter().all(|m| m.is_finite() && *m >= 0.0 && *m <= 1.0e4) {
            return invalid("the stock margin must be from 0 to 10 m along each axis");
        }
        if part.is_empty() || !part.min.is_finite() || !part.max.is_finite() {
            return invalid("the part has no extent to put the stock round");
        }
        let stock = Stock::Box { min: part.min - margin, max: part.max + margin };
        // A flat part with no margin has no height: give it the thinnest box there is.
        if let Stock::Box { min, max } = stock
            && !(min.x < max.x && min.y < max.y && min.z < max.z)
        {
            return invalid("the part is flat along an axis: give the stock a margin along it");
        }
        stock.validate()?;
        Ok(stock)
    }

    /// The highest Z of the stock at A = 0.
    pub fn top(&self) -> f64 {
        match *self {
            Stock::Box { max, .. } => max.z,
            Stock::Cylinder { radius, .. } => radius,
        }
    }

    /// What is left of the stock after [`face`] has faced `area` (work XY) down to `bottom`: a
    /// box whose whole top the area covers is lower; anything else (a part of the top faced, a
    /// bar's flat) is kept as it was, on the safe side.
    pub fn faced(&self, area: [Vec2; 2], bottom: f64) -> Stock {
        let [lo, hi] = area;
        match *self {
            Stock::Box { min, max }
                if lo.x <= min.x + 1e-9
                    && lo.y <= min.y + 1e-9
                    && hi.x >= max.x - 1e-9
                    && hi.y >= max.y - 1e-9
                    && bottom > min.z
                    && bottom < max.z =>
            {
                Stock::Box { min, max: Vec3::new(max.x, max.y, bottom) }
            }
            other => other,
        }
    }

    /// The lowest Z of the stock at A = 0.
    pub fn bottom(&self) -> f64 {
        match *self {
            Stock::Box { min, .. } => min.z,
            Stock::Cylinder { radius, .. } => -radius,
        }
    }

    /// What a vertical cutter with its tip at height `z` (endless above it) would meet of the
    /// stock, seen from above: the low and high corners of the stock's widest section at or
    /// above `z`, shrunk by `tol`; `None` where the stock is all below `z`.
    pub fn section_above(&self, z: f64, tol: f64) -> Option<[Vec2; 2]> {
        let (lo, hi) = match *self {
            Stock::Box { min, max } => {
                if !(z < max.z - tol) {
                    return None;
                }
                (Vec2::new(min.x + tol, min.y + tol), Vec2::new(max.x - tol, max.y - tol))
            }
            Stock::Cylinder { radius, x_min, x_max } => {
                let r = radius - tol;
                if !(z < r) {
                    return None;
                }
                let half = if z <= 0.0 { r } else { (r * r - z * z).max(0.0).sqrt() };
                (Vec2::new(x_min + tol, -half), Vec2::new(x_max - tol, half))
            }
        };
        (lo.x < hi.x && lo.y < hi.y).then_some([lo, hi])
    }

    /// The low and high corners of the stock seen from above (work XY).
    pub fn footprint(&self) -> [Vec2; 2] {
        match *self {
            Stock::Box { min, max } => [Vec2::new(min.x, min.y), Vec2::new(max.x, max.y)],
            Stock::Cylinder { radius, x_min, x_max } => [Vec2::new(x_min, -radius), Vec2::new(x_max, radius)],
        }
    }

    /// The stock's extent along X.
    pub fn x_range(&self) -> (f64, f64) {
        match *self {
            Stock::Box { min, max } => (min.x, max.x),
            Stock::Cylinder { x_min, x_max, .. } => (x_min, x_max),
        }
    }

    /// The farthest the stock reaches from the line through `center` along X: what it sweeps
    /// as the A axis turns it.
    pub fn radius_about_x(&self, center: Vec2) -> f64 {
        match *self {
            Stock::Box { min, max } => [(min.y, min.z), (min.y, max.z), (max.y, min.z), (max.y, max.z)]
                .iter()
                .map(|&(y, z)| (y - center.x).hypot(z - center.y))
                .fold(0.0, f64::max),
            Stock::Cylinder { radius, .. } => radius + center.len(),
        }
    }

    /// Whether `b` lies inside the stock, within `tol`.
    pub fn holds(&self, b: &Aabb3, tol: f64) -> bool {
        if b.is_empty() {
            return true;
        }
        match *self {
            Stock::Box { min, max } => {
                b.min.x >= min.x - tol
                    && b.min.y >= min.y - tol
                    && b.min.z >= min.z - tol
                    && b.max.x <= max.x + tol
                    && b.max.y <= max.y + tol
                    && b.max.z <= max.z + tol
            }
            Stock::Cylinder { radius, x_min, x_max } => {
                let r = [(b.min.y, b.min.z), (b.min.y, b.max.z), (b.max.y, b.min.z), (b.max.y, b.max.z)]
                    .iter()
                    .map(|&(y, z)| y.hypot(z))
                    .fold(0.0, f64::max);
                b.min.x >= x_min - tol && b.max.x <= x_max + tol && r <= radius + tol
            }
        }
    }

    /// Whether every point of `part` lies inside the stock, within `tol`: a bar is held against
    /// how far the part's points reach from its axis, not its box's corners.
    pub fn holds_surface(&self, part: &crate::Surface, tol: f64) -> bool {
        match *self {
            Stock::Box { .. } => self.holds(&part.bounds(), tol),
            Stock::Cylinder { radius, x_min, x_max } => {
                part.triangles().iter().flatten().all(|p| p.x >= x_min - tol && p.x <= x_max + tol && p.y.hypot(p.z) <= radius + tol)
            }
        }
    }

    /// How deep the cutter at `pose` (a cylinder of `radius` from the tip along its axis
    /// without end) is in the stock, where it is deeper than `tol`: the least it would have to
    /// move to clear it, along its axis or sideways. `None` when it is clear.
    pub fn intrusion(&self, pose: &Pose, radius: f64, tol: f64) -> Option<f64> {
        match *self {
            Stock::Box { min, max } => box_intrusion(min + Vec3::new(tol, tol, tol), max - Vec3::new(tol, tol, tol), pose, radius),
            Stock::Cylinder { radius: big, x_min, x_max } => axis_intrusion(pose, radius, Vec2::new(0.0, 0.0), big - tol, x_min + tol, x_max - tol),
        }
    }
}

fn coord(v: Vec3, k: usize) -> f64 {
    match k {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

/// The distance from `p` to the box `lo`–`hi` (0 inside).
fn point_box(p: Vec3, lo: Vec3, hi: Vec3) -> f64 {
    let d = |v: f64, a: f64, b: f64| (a - v).max(0.0).max(v - b);
    Vec3::new(d(p.x, lo.x, hi.x), d(p.y, lo.y, hi.y), d(p.z, lo.z, hi.z)).len()
}

/// The least of `f`, convex on `[a, b]` (flat stretches included), found by golden sections.
fn convex_min(f: impl Fn(f64) -> f64, a: f64, b: f64) -> f64 {
    const G: f64 = 0.618_033_988_749_894_8;
    let (mut lo, mut hi) = (a, b);
    let mut best = f(a).min(f(b));
    let (mut c, mut d) = (hi - G * (hi - lo), lo + G * (hi - lo));
    let (mut fc, mut fd) = (f(c), f(d));
    for _ in 0..CONVEX_STEPS {
        best = best.min(fc).min(fd);
        if fc <= fd {
            (hi, d, fd) = (d, c, fc);
            c = hi - G * (hi - lo);
            fc = f(c);
        } else {
            (lo, c, fc) = (c, d, fd);
            d = lo + G * (hi - lo);
            fd = f(d);
        }
    }
    best.min(fc).min(fd)
}

/// The distance from `p` to the rectangle `lo`–`hi` (0 inside).
fn point_rect(p: Vec2, lo: Vec2, hi: Vec2) -> f64 {
    let d = |v: f64, a: f64, b: f64| (a - v).max(0.0).max(v - b);
    d(p.x, lo.x, hi.x).hypot(d(p.y, lo.y, hi.y))
}

/// The distance from `p` to the segment `a`–`b`.
fn point_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let d = b - a;
    let t = if d.len2() > 0.0 { ((p - a).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
    p.dist(a + d * t)
}

/// The distance between the segment `a`–`b` and the rectangle `lo`–`hi` (0 where they meet):
/// exact, the rectangle being convex.
pub(crate) fn segment_rect(a: Vec2, b: Vec2, lo: Vec2, hi: Vec2) -> f64 {
    let corners = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)];
    let ends = point_rect(a, lo, hi).min(point_rect(b, lo, hi));
    if ends <= 0.0 {
        return 0.0;
    }
    // Both ends outside: the segment meets the rectangle where it crosses one of its sides.
    let side = |p: Vec2, q: Vec2, r: Vec2| (q - p).cross(r - p);
    let crosses = |c: Vec2, e: Vec2| {
        let (d1, d2) = (side(a, b, c), side(a, b, e));
        let (d3, d4) = (side(c, e, a), side(c, e, b));
        // Along a side (or a point): the corners' distances below cover it.
        !(d1 == 0.0 && d2 == 0.0) && d1 * d2 <= 0.0 && d3 * d4 <= 0.0
    };
    let [c0, c1, c2, c3] = corners;
    let sides = [(c0, c1), (c1, c2), (c2, c3), (c3, c0)];
    if sides.iter().any(|&(c, e)| crosses(c, e)) {
        return 0.0;
    }
    corners.iter().map(|&c| point_segment(c, a, b)).fold(ends, f64::min)
}

/// [`Stock::intrusion`] of the box `lo`–`hi` (already shrunk by the tolerance).
fn box_intrusion(lo: Vec3, hi: Vec3, pose: &Pose, radius: f64) -> Option<f64> {
    if !(lo.x < hi.x && lo.y < hi.y && lo.z < hi.z) {
        return None;
    }
    let (p, a) = (pose.tip, pose.axis);
    // A cutter along a work axis: exact.
    for k in 0..3 {
        let s = coord(a, k);
        if s.abs() < 1.0 - 1e-9 {
            continue;
        }
        let along = if s > 0.0 { coord(hi, k) - coord(p, k) } else { coord(p, k) - coord(lo, k) };
        if !(along > 0.0) {
            return None;
        }
        let (i, j) = ((k + 1) % 3, (k + 2) % 3);
        let side = |c: usize| (coord(lo, c) - coord(p, c)).max(0.0).max(coord(p, c) - coord(hi, c));
        let gap = side(i).hypot(side(j));
        return (gap < radius).then(|| along.min(radius - gap));
    }
    // A tilted cutter: the nearest its axis comes to the box, grown by the radius (a little
    // larger than the cutter around its tip, so on the safe side). The distance to a convex box
    // along a line is convex, so its least is found exactly; past `reach` the axis is farther
    // from the box than the radius.
    let reach = point_box(p, lo, hi) + (hi - lo).len() + radius;
    let nearest = convex_min(|t| point_box(p + a * t, lo, hi), 0.0, reach);
    (nearest < radius).then_some(radius - nearest)
}

/// How deep the cutter at `pose` (a cylinder of `radius` from the tip along its axis without
/// end) reaches inside the round region within `big` of the line along X through `(center.x,
/// center.y)` in Y and Z, from `x_lo` to `x_hi`: `big` less the cutter's nearest approach to
/// the line, where that is positive and the cutter is level with the region along X.
pub(crate) fn axis_intrusion(pose: &Pose, radius: f64, center: Vec2, big: f64, x_lo: f64, x_hi: f64) -> Option<f64> {
    if !(x_lo < x_hi) || !(big > 0.0) {
        return None;
    }
    let (p, a) = (pose.tip, pose.axis);
    let q = Vec2::new(p.y - center.x, p.z - center.y);
    let nearest = if a.x.abs() < 1e-9 {
        // Square to the line: the cutter's cross-section in YZ is a strip `radius` either side
        // of its axis, from the tip outwards.
        let along = Vec2::new(a.y, a.z).normalized()?;
        let across = along.perp();
        if !(p.x - radius < x_hi && p.x + radius > x_lo) {
            return None;
        }
        q.dot(along).max(0.0).hypot((q.dot(across).abs() - radius).max(0.0))
    } else if a.x.abs() > 1.0 - 1e-9 {
        // Along the line, from the tip one way.
        let level = if a.x > 0.0 { p.x < x_hi } else { p.x > x_lo };
        if !level {
            return None;
        }
        (q.len() - radius).max(0.0)
    } else {
        // Neither: the nearest the axis comes to the line while level with the region along X
        // (grown by the radius), each point grown by the radius. Both the distance to a line and
        // the stretch of the axis level with the region are convex, so the least is exact
        // however far along the axis that stretch is.
        let (t1, t2) = ((x_lo - radius - p.x) / a.x, (x_hi + radius - p.x) / a.x);
        let (lo, hi) = (t1.min(t2).max(0.0), t1.max(t2));
        if !(lo < hi) || !hi.is_finite() {
            return None;
        }
        convex_min(|t| ((p.y + a.y * t - center.x).hypot(p.z + a.z * t - center.y) - radius).max(0.0), lo, hi)
    };
    (nearest < big).then_some(big - nearest)
}

/// How a [`face`] operation cuts.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceParams {
    /// The low and high corners of the area to face (work XY): the stock's footprint.
    pub area: [Vec2; 2],
    /// `top` is where the first level starts from, `bottom` the faced height.
    pub heights: Heights,
    pub feeds: Feeds,
    /// Most material per level (mm).
    pub stepdown: f64,
    /// Distance between passes (mm), at most the flat of the tool's end.
    pub stepover: f64,
    /// Direction of the passes, radians counter-clockwise from +X.
    pub angle: f64,
    /// How far the cutter reaches past the area's edges on the outermost passes (mm).
    pub overhang: f64,
    /// Every other pass runs back, linked at depth outside the area; otherwise every pass runs
    /// the same way and the tool rises to the retract height between them.
    pub zigzag: bool,
    /// The job's stock, when it has one: the facing is refused where a descent or a link
    /// between passes would run in it (an area that does not reach past the stock along the
    /// passes), or where the bottom is below the stock's (what holds the stock is there).
    #[serde(default)]
    pub stock: Option<Stock>,
    /// The radius of a quarter arc in the vertical plane of the passes (mm), from `lead_radius`
    /// above and behind a pass's start down into it, and out of its end up and beyond: `G18`
    /// or `G19` arcs when the passes run along X or Y, lines along the arc otherwise. With
    /// `zigzag` the passes of a level link round half circles at depth, outside the area, and
    /// only the level's first pass leads in and its last leads out. `None`: straight down
    /// beside the stock and straight links.
    #[serde(default)]
    pub lead_radius: Option<f64>,
}

/// Most a facing lead's radius may be, as a share of the tool's diameter.
const MAX_FACE_LEAD: f64 = 10.0;

/// The diameter of the flat of `tool`'s end: what leaves a flat floor.
pub fn flat_diameter(tool: &Tool) -> f64 {
    match tool.kind {
        ToolKind::BullNose | ToolKind::FaceMill => tool.diameter - 2.0 * tool.corner_radius.unwrap_or(0.0),
        _ => tool.diameter,
    }
}

/// The default stepover of a facing operation: 70% of the flat of the tool's end.
pub fn default_face_stepover(tool: &Tool) -> f64 {
    0.7 * flat_diameter(tool)
}

/// The default overhang of a facing operation: half the tool's diameter and [`FACE_MARGIN`],
/// which puts the outermost passes' centres just outside the edges.
pub fn default_face_overhang(tool: &Tool) -> f64 {
    tool.radius() + FACE_MARGIN
}

/// Depth levels from `top` down to `bottom`, evenly spaced, at most `stepdown` apart.
fn levels(top: f64, bottom: f64, stepdown: f64) -> Result<Vec<f64>> {
    let n = ((top - bottom) / stepdown - 1e-9).ceil().max(1.0);
    if !(n <= MAX_LEVELS as f64) {
        return Err(CamError::TooLarge(format!("more than {MAX_LEVELS} facing levels")));
    }
    let n = n as usize;
    Ok((1..=n).map(|k| if k == n { bottom } else { top - (top - bottom) * k as f64 / n as f64 }).collect())
}

struct Path {
    moves: Vec<Move>,
}

/// Where a facing's passes run: from `u.0` to `u.1` along the pass direction (`angle`), at
/// `passes` across it, at each of `levels`.
struct Passes<'a> {
    u: (f64, f64),
    passes: &'a [f64],
    levels: &'a [f64],
    angle: f64,
}

fn push_all(path: &mut Path, moves: Vec<Move>) -> Result<()> {
    moves.into_iter().try_for_each(|m| path.push(m))
}

/// [`face`]'s passes with vertical leads (see [`FaceParams::lead_radius`]).
fn face_with_leads(path: &mut Path, entries: &mut Vec<(Vec3, Vec3)>, ps: &Passes<'_>, p: &FaceParams, lead: f64) -> Result<()> {
    let h = p.heights;
    let (c, s) = (ps.angle.cos(), ps.angle.sin());
    let world = |u: f64, v: f64, z: f64| Vec3::new(u * c - v * s, u * s + v * c, z);
    let flat = |q: Vec3| Vec2::new(q.x, q.y);
    let mut at_start_side = true;
    let mut last_v: Option<f64> = None;
    for (li, &z) in ps.levels.iter().enumerate() {
        let order: Vec<f64> = if p.zigzag && li % 2 == 1 { ps.passes.iter().rev().copied().collect() } else { ps.passes.to_vec() };
        let count = order.len();
        for (k, v) in order.into_iter().enumerate() {
            if !p.zigzag {
                at_start_side = true;
            }
            let (ua, ub) = if at_start_side { ps.u } else { (ps.u.1, ps.u.0) };
            let dir = if ub >= ua { 1.0 } else { -1.0 };
            let along = Vec2::new(dir * c, dir * s);
            let (a, b) = (world(ua, v, z), world(ub, v, z));
            let at_depth = path.pos().filter(|q| p.zigzag && k > 0 && (q.z - z).abs() <= 1e-9);
            match (at_depth, last_v) {
                (Some(q), Some(lv)) => {
                    // Round half a circle outside the area, through the point farthest out.
                    let rad = 0.5 * (v - lv).abs();
                    let center = world(ua, 0.5 * (v + lv), z);
                    let out = world(ua - dir * rad, 0.5 * (v + lv), z);
                    let (cs, os) = (flat(q) - flat(center), flat(out) - flat(center));
                    let ccw = cs.x * os.y - cs.y * os.x > 0.0;
                    path.push(Move::Arc { to: a, center: flat(center), ccw, feed: p.feeds.cut })?;
                    entries.push((q, a));
                }
                _ => {
                    let start = world(ua - dir * lead, v, z + lead);
                    match path.pos() {
                        None => path.push(Move::Rapid { to: Vec3::new(start.x, start.y, h.clearance) })?,
                        Some(q) if flat(q).dist(flat(start)) > 1e-9 => {
                            if q.z < h.retract {
                                path.push(Move::Rapid { to: Vec3::new(q.x, q.y, h.retract) })?;
                            }
                            path.push(Move::Rapid { to: Vec3::new(start.x, start.y, h.retract.max(q.z)) })?;
                        }
                        Some(_) => {}
                    }
                    let air = h.retract.max(start.z);
                    if let Some(q) = path.pos()
                        && q.z > air + 1e-9
                    {
                        path.push(Move::Rapid { to: Vec3::new(q.x, q.y, air) })?;
                    }
                    if let Some(q) = path.pos() {
                        entries.push((q, a));
                        if q.z > start.z + 1e-9 {
                            path.push(Move::Feed { to: start, feed: p.feeds.plunge })?;
                        } else if q.z < start.z - 1e-9 {
                            // A lead taller than the clearance: up to where its arc starts.
                            path.push(Move::Rapid { to: start })?;
                        }
                    }
                    push_all(path, vertical_quarter(start, a, world(ua, v, z + lead), along, p.feeds.cut))?;
                }
            }
            path.push(Move::Feed { to: b, feed: p.feeds.cut })?;
            if !p.zigzag || k + 1 == count {
                push_all(path, vertical_quarter(b, world(ub + dir * lead, v, z + lead), world(ub, v, z + lead), along, p.feeds.cut))?;
            }
            last_v = Some(v);
            at_start_side = !at_start_side;
        }
    }
    Ok(())
}

impl Path {
    fn push(&mut self, m: Move) -> Result<()> {
        if self.moves.len() >= MAX_MOVES {
            return Err(CamError::TooLarge(format!("the toolpath has more than {MAX_MOVES} moves")));
        }
        self.moves.push(m);
        Ok(())
    }

    fn pos(&self) -> Option<Vec3> {
        self.moves.iter().rev().find_map(Move::target)
    }
}

/// Faces the area of `p` with `tool` (a face mill, a flat end mill or a bull nose) down to
/// `p.heights.bottom`; see the module notes. Every pass goes down beside the stock, so a face
/// mill never plunges into material.
pub fn face(tool: &Tool, p: &FaceParams) -> Result<Toolpath> {
    tool.validate()?;
    if !matches!(tool.kind, ToolKind::FlatEndmill | ToolKind::BullNose | ToolKind::FaceMill) {
        return invalid("facing needs a face mill, a flat end mill or a bull nose");
    }
    p.heights.validate()?;
    p.feeds.validate()?;
    let h = p.heights;
    if !(h.top > h.bottom) {
        return invalid("the top must be above the bottom: there is nothing to face");
    }
    let [lo, hi] = p.area;
    if ![lo.x, lo.y, hi.x, hi.y].into_iter().all(finite) || !(lo.x < hi.x && lo.y < hi.y) {
        return invalid("the area to face must be [[low x, low y], [high x, high y]], finite numbers");
    }
    if !(p.stepdown > 0.0) || !finite(p.stepdown) {
        return invalid("the stepdown must be above 0");
    }
    let flat = flat_diameter(tool);
    if !(p.stepover > 0.0) || !(p.stepover <= flat + 1e-9) {
        return invalid(format!("the stepover must be above 0 and at most the flat of the tool's end ({flat} mm)"));
    }
    if !(p.angle.is_finite() && p.angle.abs() <= 1.0e4) {
        return invalid("the pass angle must be a finite angle");
    }
    if !(p.overhang >= 0.0) || !(p.overhang <= 10.0 * tool.diameter) {
        return invalid("the overhang must be from 0 to ten times the tool's diameter");
    }
    let r = tool.radius();
    let (c, s) = (p.angle.cos(), p.angle.sin());
    let local = |q: Vec2| Vec2::new(q.x * c + q.y * s, -q.x * s + q.y * c);
    let corners = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)].map(local);
    let (mut umin, mut umax, mut vmin, mut vmax) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for q in corners {
        (umin, umax, vmin, vmax) = (umin.min(q.x), umax.max(q.x), vmin.min(q.y), vmax.max(q.y));
    }
    // Every pass starts and ends on a line beyond the whole area: the cutter clears it there,
    // and the links between passes run along those lines.
    let (u_start, u_end) = (umin - r - FACE_LEAD, umax + r + FACE_LEAD);
    let (v_first, v_last) = (vmin + r - p.overhang, vmax - r + p.overhang);
    let passes: Vec<f64> = if v_last <= v_first + 1e-9 {
        vec![0.5 * (vmin + vmax)]
    } else {
        let n = ((v_last - v_first) / p.stepover - 1e-9).ceil().max(1.0);
        if !(n < MAX_PASSES as f64) {
            return Err(CamError::TooLarge(format!("more than {MAX_PASSES} facing passes")));
        }
        let n = n as usize;
        (0..=n).map(|k| v_first + (v_last - v_first) * k as f64 / n as f64).collect()
    };
    let world = |u: f64, v: f64, z: f64| Vec3::new(u * c - v * s, u * s + v * c, z);

    if let Some(stock) = p.stock {
        stock.validate()?;
        if h.bottom < stock.bottom() - 1e-6 {
            return invalid(format!(
                "the bottom ({}) is below the stock's bottom ({}): the tool would cut into what holds the stock",
                h.bottom,
                stock.bottom()
            ));
        }
    }
    let mut path = Path { moves: Vec::new() };
    // The descents and the links between passes, which must be clear of the stock.
    let mut entries: Vec<(Vec3, Vec3)> = Vec::new();
    let mut at_start_side = true;
    let levels = levels(h.top, h.bottom, p.stepdown)?;
    if let Some(lead) = p.lead_radius {
        if !(lead > 0.0 && lead <= MAX_FACE_LEAD * tool.diameter) {
            return invalid(format!("the lead radius must be above 0 and at most {MAX_FACE_LEAD} times the tool's diameter"));
        }
        let pass = Passes { u: (u_start, u_end), passes: &passes, levels: &levels, angle: p.angle };
        face_with_leads(&mut path, &mut entries, &pass, p, lead)?;
    }
    for (li, z) in levels.into_iter().enumerate().filter(|_| p.lead_radius.is_none()) {
        // Each level runs the passes the other way round from the last, so a zigzag carries on
        // from where the last level ended.
        let order: Vec<f64> = if p.zigzag && li % 2 == 1 { passes.iter().rev().copied().collect() } else { passes.clone() };
        for (k, v) in order.into_iter().enumerate() {
            if !p.zigzag {
                at_start_side = true;
            }
            let (ua, ub) = if at_start_side { (u_start, u_end) } else { (u_end, u_start) };
            let (a, b) = (world(ua, v, z), world(ub, v, z));
            let pos = path.pos();
            let carry_on = p.zigzag && pos.is_some_and(|q| (k > 0 && (q.z - z).abs() <= 1e-9) || (k == 0 && q.x == a.x && q.y == a.y));
            if carry_on {
                // Along the line outside the area, or down beside it to the next level.
                let feed = if k == 0 { p.feeds.plunge } else { p.feeds.cut };
                if let Some(q) = pos {
                    entries.push((q, a));
                }
                path.push(Move::Feed { to: a, feed })?;
            } else {
                match pos {
                    None => path.push(Move::Rapid { to: world(ua, v, h.clearance) })?,
                    Some(q) => {
                        if q.z < h.retract {
                            path.push(Move::Rapid { to: Vec3::new(q.x, q.y, h.retract) })?;
                        }
                        path.push(Move::Rapid { to: world(ua, v, h.retract.max(q.z)) })?;
                    }
                }
                if path.pos().is_some_and(|q| q.z > h.retract) {
                    path.push(Move::Rapid { to: world(ua, v, h.retract) })?;
                }
                if let Some(q) = path.pos() {
                    entries.push((q, a));
                }
                path.push(Move::Feed { to: a, feed: p.feeds.plunge })?;
            }
            path.push(Move::Feed { to: b, feed: p.feeds.cut })?;
            at_start_side = !at_start_side;
        }
    }
    if let Some(q) = path.pos() {
        path.push(Move::Rapid { to: Vec3::new(q.x, q.y, h.clearance) })?;
    }
    if let Some(stock) = p.stock {
        for (q, a) in entries {
            let Some([slo, shi]) = stock.section_above(q.z.min(a.z), 0.0) else { continue };
            if segment_rect(Vec2::new(q.x, q.y), Vec2::new(a.x, a.y), slo, shi) < r - 1e-6 {
                return invalid(format!(
                    "the tool would go down into the stock at [{:.3}, {:.3}, {:.3}]: give an area that reaches past the \
                     stock along the passes, or face the whole stock",
                    a.x, a.y, a.z
                ));
            }
        }
    }
    let out = Toolpath { moves: path.moves };
    verify(&out, &h)?;
    Ok(out)
}
