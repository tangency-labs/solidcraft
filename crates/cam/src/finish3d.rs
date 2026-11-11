//! 3-axis finishing strategies on a [`crate::Surface`] that place their passes by the shape of the
//! cutter-location (CL) height field rather than along straight lines: constant-scallop
//! offsetting ([`scallop`]), z-levels on steep ground and offsets or rasters on shallow ground
//! ([`steep_shallow`]), tracing the concave creases a cutter touches on two sides ([`pencil`]) and
//! cutting only what a larger tool left ([`rest_finish`]).
//!
//! Each starts from a grid of drop-cutter samples over the boundary's rectangle: the tip's height
//! where the cutter rests (never below the floor, `heights.bottom`). That grid is a surface in
//! space, and distances are measured over it: a front is marched outward from where it starts
//! (the boundary, a crease, the edge of an area) with a first-order fast marching method on the
//! grid's triangles, each node reached by the plane wave through two neighbours already reached
//! when the wave comes from between them, otherwise from the nearer one. Passes are the lines
//! where that distance is a multiple of the spacing, so neighbouring passes are a spacing apart
//! along the surface whatever its slope. Passes are only XY curves at that stage: each is then
//! sampled and refined with the drop-cutter exactly as a raster pass is
//! ([`crate::surface_ops::parallel`]), so no point it cuts at is below where the cutter rests,
//! stretches with nothing under the cutter are skipped, moves that go down are slowed to the
//! plunge feed, and passes are linked by feeding across when the link is short and clears the
//! part, otherwise by lifting to the retract height. The path is checked with [`crate::verify`].
//!
//! The grid is a sampling: a feature narrower than its step can be missed by the detection (a
//! crease, a steep strip), never cut into, because every cutting point is dropped again.
//! Everything is bounded: the grid's nodes, the drop-cutter samples, the levels and the lines;
//! an operation past a bound is refused with [`CamError::TooLarge`] before it runs for hours.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::cutter::Cutter;
use crate::dropcutter::drop_z;
use crate::ops::{Feeds, Heights};
use crate::path::{MAX_MOVES, Toolpath};
use crate::surface::Surface;
use crate::surface_ops::{
    Boundary, DEFAULT_DESCENT_LIMIT, Fence, Field, MAX_GRID_NODES, Out3, Pass, Profile, Rect, Tip, check_angle, check_leave, check_step,
    check_stepdown, check_tolerance, depth_for, descent_of, levels, link_path, spans, tool_cutter,
};
use crate::tool::Tool;
use crate::{CamError, Result, invalid};

/// Most grid visits of one operation's levels and lines, and most comparisons of the rest test.
const MAX_WORK: f64 = 1.0e9;
/// Most levels of distance or height one operation traces.
const MAX_LEVELS: usize = 10_000;
/// A gap between passes wider than this share of the spacing from every pass gets a pass of its
/// own, at this distance from the passes round it.
const GAP: f64 = 0.575;
/// Most rounds of filling gaps.
const GAP_ROUNDS: usize = 3;
/// A grid cell whose heights rise more than this many times its width between two neighbouring
/// nodes is a cliff in the height field (a wall the cutter's side runs down): distance lines
/// there are not cut, since a pass can only follow the field's top.
const CLIFF: f64 = 5.67;
/// Most offset passes on each side of a crease.
const MAX_PENCIL_PASSES: u32 = 100;
/// The least turn of the tip's path across a crease that [`scallop`] and [`steep_shallow`]
/// finish with a pass of its own (degrees).
const CLEANUP_ANGLE: f64 = 30.0;
/// Most passes of a group put in order by distance; a larger group keeps the order found.
const MAX_ORDERED: usize = 4000;
/// Most points of the straight shallow passes of [`steep_shallow`] before they are clipped and
/// cut: each becomes a move, and a path holds at most [`MAX_MOVES`].
const MAX_LINE_POINTS: usize = 2 * MAX_MOVES;

/// The 8 neighbours of a node, counter-clockwise from +X.
const RING: [(isize, isize); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];

/// How far apart passes are.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spacing {
    /// The height of the ridge left between two passes (mm), above 0 and at most the cutter's
    /// corner radius: the passes are spaced for it, closer where the surface is concave and
    /// further where it is convex. A flat end mill leaves no such ridge and needs a stepover.
    ScallopHeight(f64),
    /// Distance between passes measured along the surface (mm), above 0 and at most the tool
    /// diameter.
    Stepover(f64),
}

/// The order offset passes run in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OffsetDirection {
    /// From the boundary towards the middle.
    #[default]
    Inward,
    /// From the middle out to the boundary.
    Outward,
}

/// Constant-scallop finishing: passes offset from the boundary along the surface.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScallopParams {
    /// `bottom` is the floor: where the surface is lower the tool runs at it. `top` is not used.
    pub heights: Heights,
    pub feeds: Feeds,
    /// Feed for the moves inside a pass that go down the surface at least as steeply as the
    /// descent limit (mm/min); entries from above always come down at the plunge feed. Left out,
    /// the cut feed: the pass runs over material a roughing left (the stock to leave), so going
    /// down a wall is a side cut, not a plunge. Give the plunge feed when finishing straight
    /// from uncut stock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descent_feed: Option<f64>,
    pub spacing: Spacing,
    #[serde(default)]
    pub direction: OffsetDirection,
    #[serde(default)]
    pub boundary: Boundary,
    /// Material left on the surface (mm).
    pub stock_to_leave: f64,
    /// Spacing of the grid the distances are measured on, and of the samples along each pass
    /// before refinement (mm); a sample step above the tool's radius is taken as the radius.
    pub step: f64,
    /// Largest distance between the path and the surface's dropped height (mm).
    pub tolerance: f64,
    /// Climb milling (what is still to cut on the right of the tool's travel); otherwise
    /// conventional.
    pub climb: bool,
}

/// The passes on ground shallower than the threshold.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ShallowPattern {
    /// Passes offset from the boundary and from the steep ground, as [`scallop`].
    Scallop,
    /// Straight passes at `angle` (radians from +X), close enough that they are at most the
    /// spacing apart along the steepest shallow ground.
    Parallel { angle: f64 },
}

/// Z-level passes on steep ground and offset or raster passes on shallow ground.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SteepShallowParams {
    /// The z-levels run from `top` down to `bottom`; `bottom` is the floor too.
    pub heights: Heights,
    pub feeds: Feeds,
    /// Feed for the moves inside a pass that go down the surface at least as steeply as the
    /// descent limit (mm/min); entries from above always come down at the plunge feed. Left out,
    /// the cut feed: the pass runs over material a roughing left (the stock to leave), so going
    /// down a wall is a side cut, not a plunge. Give the plunge feed when finishing straight
    /// from uncut stock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descent_feed: Option<f64>,
    /// The spacing on shallow ground, and on steep ground too unless `stepdown` is given.
    pub spacing: Spacing,
    /// Ground steeper than this (radians from horizontal, 1° to 89°) is steep, as the cutter
    /// meets it (the slope of the tip's height).
    pub threshold: f64,
    /// How far each kind of pass runs on into the other's ground (mm), so they overlap.
    pub overlap: f64,
    /// Most height between z-levels (mm); left out, the spacing times the sine of the threshold,
    /// which keeps the levels a spacing apart along the shallowest steep ground.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stepdown: Option<f64>,
    pub shallow: ShallowPattern,
    #[serde(default)]
    pub boundary: Boundary,
    pub stock_to_leave: f64,
    pub step: f64,
    pub tolerance: f64,
    pub climb: bool,
}

/// Tracing the concave creases where the cutter touches the surface on two sides.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PencilParams {
    /// `bottom` is the floor. `top` is not used.
    pub heights: Heights,
    pub feeds: Feeds,
    /// Feed for the moves inside a pass that go down the surface at least as steeply as the
    /// descent limit (mm/min); entries from above always come down at the plunge feed. Left out,
    /// the cut feed: the pass runs over material a roughing left (the stock to leave), so going
    /// down a wall is a side cut, not a plunge. Give the plunge feed when finishing straight
    /// from uncut stock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descent_feed: Option<f64>,
    /// Least turn of the tip's path across a crease for it to count (radians, 1° to 170°): where
    /// the cutter touches two faces the tip's height bends sharply, where it rolls over one
    /// surface it bends no more than the surface does.
    pub min_angle: f64,
    /// Offset passes on each side of every crease (0: the crease alone).
    pub passes: u32,
    /// Distance between offset passes along the surface (mm); used with `passes`.
    pub stepover: f64,
    /// Creases shorter than this are left out (mm).
    pub min_length: f64,
    #[serde(default)]
    pub boundary: Boundary,
    pub stock_to_leave: f64,
    /// Spacing of the grid creases are looked for on (mm).
    pub step: f64,
    pub tolerance: f64,
    pub climb: bool,
}

/// Rest finishing: cutting only where this tool reaches deeper than a larger one did.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RestParams {
    /// `bottom` is the floor. `top` is not used.
    pub heights: Heights,
    pub feeds: Feeds,
    /// Feed for the moves inside a pass that go down the surface at least as steeply as the
    /// descent limit (mm/min); entries from above always come down at the plunge feed. Left out,
    /// the cut feed: the pass runs over material a roughing left (the stock to leave), so going
    /// down a wall is a side cut, not a plunge. Give the plunge feed when finishing straight
    /// from uncut stock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descent_feed: Option<f64>,
    /// The tool that cut before, usually larger than this one; not of the same shape.
    pub previous: Tool,
    /// Least material the previous tool left, measured square to the surface, for a place to be
    /// cut (mm).
    pub threshold: f64,
    pub spacing: Spacing,
    /// How far round what is left the passes run on (mm), so they blend into what was cut.
    pub overlap: f64,
    #[serde(default)]
    pub boundary: Boundary,
    /// Material left on the surface, by this tool and as the previous tool left it (mm).
    pub stock_to_leave: f64,
    pub step: f64,
    pub tolerance: f64,
    pub climb: bool,
}

fn finite_in(v: f64, lo: f64, hi: f64) -> bool {
    v.is_finite() && v >= lo && v <= hi
}

/// Tip heights over a grid on a rectangle.
struct Map {
    rect: Rect,
    nx: usize,
    ny: usize,
    gx: f64,
    gy: f64,
    /// The tip's height at each node, never below the floor (the floor where nothing is under
    /// the cutter).
    z: Vec<f64>,
    /// Whether the part is under the cutter at each node.
    on: Vec<bool>,
    /// Whether each node is inside the boundary's region.
    inside: Vec<bool>,
}

impl Map {
    fn build(field: &Field, fence: Option<&Fence>, rect: Rect, step: f64, floor: f64) -> Result<Map> {
        let (w, h) = (rect.max.x - rect.min.x, rect.max.y - rect.min.y);
        if !(w > 0.0) || !(h > 0.0) {
            return invalid("the boundary must have an area");
        }
        let (nx, ny) = ((w / step).ceil().max(1.0), (h / step).ceil().max(1.0));
        if !((nx + 1.0) * (ny + 1.0) <= MAX_GRID_NODES as f64) {
            return Err(CamError::TooLarge(format!("the grid has more than {MAX_GRID_NODES} nodes; use a larger step or a smaller boundary")));
        }
        let (nx, ny) = (nx as usize, ny as usize);
        let mut m = Map { rect, nx, ny, gx: w / nx as f64, gy: h / ny as f64, z: Vec::new(), on: Vec::new(), inside: Vec::new() };
        let n = m.len();
        let (mut z, mut on, mut inside) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
        for k in 0..n {
            let p = m.node(k);
            let (h, under) = match field.at(p)? {
                Tip::At(h) => (h.max(floor), true),
                Tip::Clear | Tip::Out => (floor, false),
            };
            z.push(h);
            on.push(under);
            inside.push(fence.is_none_or(|f| f.contains(p)));
        }
        (m.z, m.on, m.inside) = (z, on, inside);
        Ok(m)
    }

    fn len(&self) -> usize {
        (self.nx + 1) * (self.ny + 1)
    }

    fn id(&self, i: usize, j: usize) -> usize {
        j * (self.nx + 1) + i
    }

    fn ij(&self, k: usize) -> (usize, usize) {
        (k % (self.nx + 1), k / (self.nx + 1))
    }

    fn node(&self, k: usize) -> Vec2 {
        let (i, j) = self.ij(k);
        let x = if i == self.nx { self.rect.max.x } else { self.rect.min.x + self.gx * i as f64 };
        let y = if j == self.ny { self.rect.max.y } else { self.rect.min.y + self.gy * j as f64 };
        Vec2::new(x, y)
    }

    fn zk(&self, k: usize) -> f64 {
        self.z.get(k).copied().unwrap_or(f64::NAN)
    }

    fn onk(&self, k: usize) -> bool {
        self.on.get(k).copied().unwrap_or(false)
    }

    fn p3(&self, k: usize) -> Vec3 {
        self.node(k).extend(self.zk(k))
    }

    fn neighbour(&self, k: usize, di: isize, dj: isize) -> Option<usize> {
        let (i, j) = self.ij(k);
        let (i, j) = (i.checked_add_signed(di)?, j.checked_add_signed(dj)?);
        (i <= self.nx && j <= self.ny).then(|| self.id(i, j))
    }

    fn on_border(&self, k: usize) -> bool {
        let (i, j) = self.ij(k);
        i == 0 || j == 0 || i == self.nx || j == self.ny
    }

    /// The cell holding `p` and where in it, clamped to the grid.
    fn cell(&self, p: Vec2) -> (usize, usize, f64, f64) {
        let fx = ((p.x - self.rect.min.x) / self.gx).clamp(0.0, self.nx as f64);
        let fy = ((p.y - self.rect.min.y) / self.gy).clamp(0.0, self.ny as f64);
        let (i, j) = ((fx.floor() as usize).min(self.nx.saturating_sub(1)), (fy.floor() as usize).min(self.ny.saturating_sub(1)));
        (i, j, fx - i as f64, fy - j as f64)
    }

    /// `f` interpolated at `p` between the corners of its cell.
    fn sample(&self, f: &[f64], p: Vec2) -> f64 {
        let (i, j, u, v) = self.cell(p);
        let at = |a: usize, b: usize| f.get(self.id(a, b)).copied().unwrap_or(f64::NAN);
        let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
        (a * (1.0 - u) + b * u) * (1.0 - v) + (d * (1.0 - u) + c * u) * v
    }

    /// The slope of the tip's height at a node from its nearest neighbours, along each axis the
    /// gentler of the two sides: at the foot of a cliff the slope of the ground the tip stands
    /// on, not of the wall beside it.
    fn grad_gentle(&self, k: usize) -> Option<Vec2> {
        let d = |di: isize, dj: isize, g: f64| -> Option<f64> {
            let z = self.zk(k);
            let ahead = self.neighbour(k, di, dj).map(|a| (self.zk(a) - z) / g);
            let behind = self.neighbour(k, -di, -dj).map(|b| (z - self.zk(b)) / g);
            let s = match (ahead, behind) {
                (Some(a), Some(b)) => {
                    if a.abs() <= b.abs() {
                        a
                    } else {
                        b
                    }
                }
                (Some(a), None) => a,
                (None, Some(b)) => b,
                (None, None) => return None,
            };
            s.is_finite().then_some(s)
        };
        Some(Vec2::new(d(1, 0, self.gx)?, d(0, 1, self.gy)?))
    }

    /// The mean curvature of the tip's height at a node over `m` nodes either side, positive
    /// where it is convex (a dome); `None` near the edge.
    fn curvature(&self, k: usize, m: usize) -> Option<f64> {
        let m = m.max(1) as isize;
        let z = |di: isize, dj: isize| self.neighbour(k, di * m, dj * m).map(|n| self.zk(n));
        let (hx, hy) = (m as f64 * self.gx, m as f64 * self.gy);
        let (c, e, w, n, s) = (self.zk(k), z(1, 0)?, z(-1, 0)?, z(0, 1)?, z(0, -1)?);
        let (ne, nw, se, sw) = (z(1, 1)?, z(-1, 1)?, z(1, -1)?, z(-1, -1)?);
        let (zx, zy) = ((e - w) / (2.0 * hx), (n - s) / (2.0 * hy));
        let (zxx, zyy) = ((e - 2.0 * c + w) / (hx * hx), (n - 2.0 * c + s) / (hy * hy));
        let zxy = (ne - nw - se + sw) / (4.0 * hx * hy);
        let q = 1.0 + zx * zx + zy * zy;
        let k = -((1.0 + zy * zy) * zxx - 2.0 * zx * zy * zxy + (1.0 + zx * zx) * zyy) / (2.0 * q * q.sqrt());
        k.is_finite().then_some(k)
    }

    /// Whether a cell is a cliff: two neighbouring corners differ by more than [`CLIFF`] times
    /// the cell's size.
    fn cliff(&self, i: usize, j: usize) -> bool {
        let at = |a: usize, b: usize| self.zk(self.id(a, b));
        let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
        let lim = CLIFF * self.gx.min(self.gy);
        (a - b).abs() > lim || (b - c).abs() > lim || (c - d).abs() > lim || (d - a).abs() > lim
    }
}

/// An item of the marching front: a tentative distance and its node, smallest first.
#[derive(PartialEq)]
struct Item(f64, usize);

impl Eq for Item {}

impl Ord for Item {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.total_cmp(&self.0).then_with(|| o.1.cmp(&self.1))
    }
}

impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// The time the plane wave through `a` (reached at `ta`) and `b` (at `tb`) reaches `c`, lengths
/// scaled by `w`; infinite when the wave reaching `c` does not come from between `a` and `b`.
fn wave(a: Vec3, ta: f64, b: Vec3, tb: f64, c: Vec3, w: f64) -> f64 {
    let (a, ta, b, tb) = if ta <= tb { (a, ta, b, tb) } else { (b, tb, a, ta) };
    let e = (b - a) * w;
    let len = e.len();
    if !(len > 1e-12) {
        return f64::INFINITY;
    }
    let ex = e * (1.0 / len);
    let v = (c - a) * w;
    let cx = v.dot(ex);
    let cy = (v - ex * cx).len();
    let u = tb - ta;
    if !(cy > 1e-12) || !(u < len) {
        return f64::INFINITY;
    }
    let nx = u / len;
    let ny = (1.0 - nx * nx).sqrt();
    if !(ny > 1e-12) {
        return f64::INFINITY;
    }
    // The wave travels along (nx, ny); traced back from `c` it must cross the edge from `a`
    // to `b` for the two to bound it.
    let foot = cx - cy * nx / ny;
    if !(foot >= 0.0 && foot <= len) {
        return f64::INFINITY;
    }
    ta + nx * cx + ny * cy
}

/// Distance over the surface the nodes `pos` gives (`None`: not on it) from `seeds` (node and
/// its starting distance), each node's lengths scaled by its `weight`, marched up to `limit`;
/// nodes not reached are infinite.
fn march(map: &Map, pos: &dyn Fn(usize) -> Option<Vec3>, weight: &[f64], seeds: &[(usize, f64)], limit: f64) -> Vec<f64> {
    let n = map.len();
    let mut t = vec![f64::INFINITY; n];
    let mut done = vec![false; n];
    let mut heap = BinaryHeap::new();
    for &(k, v) in seeds {
        if let Some(slot) = t.get_mut(k)
            && v < *slot
            && pos(k).is_some()
        {
            *slot = v;
            heap.push(Item(v, k));
        }
    }
    while let Some(Item(v, k)) = heap.pop() {
        if done.get(k).copied().unwrap_or(true) || v > t.get(k).copied().unwrap_or(f64::NEG_INFINITY) {
            continue;
        }
        if v > limit {
            break;
        }
        if let Some(d) = done.get_mut(k) {
            *d = true;
        }
        for (di, dj) in RING {
            let Some(c) = map.neighbour(k, di, dj) else { continue };
            if done.get(c).copied().unwrap_or(true) {
                continue;
            }
            let Some(pc) = pos(c) else { continue };
            let w = weight.get(c).copied().unwrap_or(1.0);
            // Every neighbour already reached, round the ring, and the triangles of two in turn.
            let ring: [Option<(Vec3, f64)>; 8] = RING.map(|(a, b)| {
                let q = map.neighbour(c, a, b)?;
                if !done.get(q).copied().unwrap_or(false) {
                    return None;
                }
                Some((pos(q)?, t.get(q).copied()?))
            });
            let mut best = t.get(c).copied().unwrap_or(f64::INFINITY);
            for (i, r) in ring.iter().enumerate() {
                let Some((pa, ta)) = *r else { continue };
                best = best.min(ta + w * pa.dist(pc));
                if let Some(Some((pb, tb))) = ring.get((i + 1) % ring.len()) {
                    best = best.min(wave(pa, ta, *pb, *tb, pc, w));
                }
            }
            if let Some(slot) = t.get_mut(c)
                && best < *slot
            {
                *slot = best;
                heap.push(Item(best, c));
            }
        }
    }
    for (v, d) in t.iter_mut().zip(&done) {
        if !*d {
            *v = f64::INFINITY;
        }
    }
    t
}

/// A line where a field is at one of the levels traced, its higher side on the left.
struct Iso {
    pts: Vec<Vec2>,
    level: usize,
}

/// The lines where `f` is at each of `levels` (ascending), each with the side above the level
/// on its left: closed ones end where they start. A cell with a corner that is not a number is
/// skipped, and so is a cliff cell when `skip_cliffs`. `at(high, low, level)` places the
/// crossing on the grid edge from the node above the level to the one below.
fn iso_lines(map: &Map, f: &[f64], levels: &[f64], skip_cliffs: bool, at: &dyn Fn(usize, usize, f64) -> Result<Vec2>) -> Result<Vec<Iso>> {
    let edges = 2 * map.len() as u64;
    let mut points: HashMap<u64, Vec2> = HashMap::new();
    let mut segs: Vec<(u64, u64)> = Vec::new();
    let mut work = 0.0;
    for j in 0..map.ny {
        for i in 0..map.nx {
            let c = [map.id(i, j), map.id(i + 1, j), map.id(i + 1, j + 1), map.id(i, j + 1)];
            let v = c.map(|k| f.get(k).copied().unwrap_or(f64::NAN));
            if v.iter().any(|x| x.is_nan()) || (skip_cliffs && map.cliff(i, j)) {
                continue;
            }
            let (lo, hi) = (v.iter().copied().fold(f64::INFINITY, f64::min), v.iter().copied().fold(f64::NEG_INFINITY, f64::max));
            let (a, b) = (levels.partition_point(|l| *l < lo), levels.partition_point(|l| *l < hi));
            work += 1.0 + (b - a) as f64;
            if !(work <= MAX_WORK) {
                return Err(CamError::TooLarge(format!("tracing the passes visits more than {MAX_WORK} grid cells; use a larger step or spacing")));
            }
            // The edges counter-clockwise, edge `k` from corner `k` to corner `k + 1`.
            let e = [2 * c[0] as u64, 2 * c[1] as u64 + 1, 2 * c[3] as u64, 2 * c[0] as u64 + 1];
            for (li, &level) in levels.iter().enumerate().take(b).skip(a) {
                let s = v.map(|x| x > level);
                // (key, entering the high side going round counter-clockwise)
                let mut xs: Vec<(u64, bool)> = Vec::with_capacity(4);
                for k in 0..4 {
                    let (sa, sb) = (s[k], s[(k + 1) % 4]);
                    if sa == sb {
                        continue;
                    }
                    let key = li as u64 * edges + e[k];
                    if let std::collections::hash_map::Entry::Vacant(slot) = points.entry(key) {
                        let (ka, kb) = (c[k], c[(k + 1) % 4]);
                        let (high, low) = if sa { (ka, kb) } else { (kb, ka) };
                        slot.insert(at(high, low, level)?);
                    }
                    xs.push((key, sb));
                }
                let joined = xs.len() == 4 && 0.25 * (v[0] + v[1] + v[2] + v[3]) > level;
                let n = xs.len();
                for (k, &(key, enters)) in xs.iter().enumerate() {
                    if !enters {
                        continue;
                    }
                    let partner = if joined { (k + n - 1) % n } else { (k + 1) % n };
                    if let Some(&(from, false)) = xs.get(partner) {
                        segs.push((from, key));
                    }
                }
                if segs.len() > MAX_MOVES {
                    return Err(CamError::TooLarge(format!("the passes have more than {MAX_MOVES} points; use a larger spacing or step")));
                }
            }
        }
    }
    Ok(chain(&segs, &points, edges))
}

/// The crossings [`iso_lines`] looks for (without skipping cliffs): each edge of a cell whose
/// ends are on either side of a level, its high end, its low end and the level, once each; none
/// when the cells to look at are more than it would trace.
fn crossings(map: &Map, f: &[f64], levels: &[f64]) -> Vec<(usize, usize, f64)> {
    let mut seen: HashSet<(usize, usize, u64)> = HashSet::new();
    let mut out = Vec::new();
    let mut work = 0.0;
    for j in 0..map.ny {
        for i in 0..map.nx {
            let c = [map.id(i, j), map.id(i + 1, j), map.id(i + 1, j + 1), map.id(i, j + 1)];
            let v = c.map(|k| f.get(k).copied().unwrap_or(f64::NAN));
            if v.iter().any(|x| x.is_nan()) {
                continue;
            }
            let (lo, hi) = (v.iter().copied().fold(f64::INFINITY, f64::min), v.iter().copied().fold(f64::NEG_INFINITY, f64::max));
            let (a, b) = (levels.partition_point(|l| *l < lo), levels.partition_point(|l| *l < hi));
            work += 1.0 + (b - a) as f64;
            if !(work <= MAX_WORK) || out.len() > MAX_MOVES {
                return Vec::new();
            }
            for &level in levels.iter().take(b).skip(a) {
                for k in 0..4 {
                    let (Some(&va), Some(&vb), Some(&ka), Some(&kb)) = (v.get(k), v.get((k + 1) % 4), c.get(k), c.get((k + 1) % 4)) else { continue };
                    let (sa, sb) = (va > level, vb > level);
                    if sa == sb {
                        continue;
                    }
                    let (high, low) = if sa { (ka, kb) } else { (kb, ka) };
                    if seen.insert((high, low, level.to_bits())) {
                        out.push((high, low, level));
                    }
                }
            }
        }
    }
    out
}

/// Least crossings of a waterline worth a thread of their own.
const CROSSINGS_PER_THREAD: usize = 64;

/// Joins segments that share an edge into lines: open ones from their free ends, then loops.
fn chain(segs: &[(u64, u64)], points: &HashMap<u64, Vec2>, edges: u64) -> Vec<Iso> {
    let by_start: HashMap<u64, usize> = segs.iter().enumerate().map(|(i, s)| (s.0, i)).collect();
    let ends: HashSet<u64> = segs.iter().map(|s| s.1).collect();
    let mut used = vec![false; segs.len()];
    let mut out = Vec::new();
    let walk = |first: usize, used: &mut Vec<bool>| -> Option<Iso> {
        let &(start, _) = segs.get(first)?;
        let mut pts = vec![*points.get(&start)?];
        let mut at = Some(first);
        while let Some(i) = at {
            match used.get_mut(i) {
                Some(u) if !*u => *u = true,
                _ => break,
            }
            let &(_, end) = segs.get(i)?;
            pts.extend(points.get(&end).copied());
            at = by_start.get(&end).copied();
        }
        (pts.len() >= 2).then_some(Iso { pts, level: (start / edges.max(1)) as usize })
    };
    for (i, s) in segs.iter().enumerate() {
        if !ends.contains(&s.0) && !used.get(i).copied().unwrap_or(true) {
            out.extend(walk(i, &mut used));
        }
    }
    for i in 0..segs.len() {
        if !used.get(i).copied().unwrap_or(true) {
            out.extend(walk(i, &mut used));
        }
    }
    out
}

/// The crossing of `level` on the grid edge from node `hi` (above it) to `lo`, by the straight
/// line between their values of `f`.
fn linear(map: &Map, f: &[f64], hi: usize, lo: usize, level: f64) -> Vec2 {
    let (a, b) = (f.get(hi).copied().unwrap_or(level), f.get(lo).copied().unwrap_or(level));
    let u = if a - b > 1e-300 { ((a - level) / (a - b)).clamp(0.0, 1.0) } else { 0.5 };
    map.node(hi).lerp(map.node(lo), u)
}

fn is_closed(pts: &[Vec2]) -> bool {
    pts.len() > 2 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.dist(*b) <= 1e-9)
}

/// The parts of `pts` where `keep` is at or above 0, each crossing placed between the points
/// by the straight line between their values. A closed line kept whole stays closed.
fn clip(pts: Vec<Vec2>, keep: &dyn Fn(Vec2) -> f64) -> Vec<Vec<Vec2>> {
    let vals: Vec<f64> = pts.iter().map(|p| keep(*p)).collect();
    clip_by(pts, &vals)
}

/// [`clip`] with the values at the points given.
fn clip_by(pts: Vec<Vec2>, vals: &[f64]) -> Vec<Vec<Vec2>> {
    if vals.iter().all(|v| *v >= 0.0) {
        return if pts.len() >= 2 { vec![pts] } else { Vec::new() };
    }
    let closed = is_closed(&pts);
    // A closed line is started at a point left out, so no part is cut in two at its seam.
    let n = if closed { pts.len() - 1 } else { pts.len() };
    let start = if closed { vals.iter().position(|v| *v < 0.0).unwrap_or(0) } else { 0 };
    let order: Vec<usize> = (0..n).map(|k| (start + k) % n.max(1)).chain(closed.then_some(start)).collect();
    let mut out = Vec::new();
    let mut cur: Vec<Vec2> = Vec::new();
    let mut prev: Option<(Vec2, f64)> = None;
    for i in order {
        let (Some(&p), Some(&v)) = (pts.get(i), vals.get(i)) else { continue };
        if let Some((q, w)) = prev
            && (w >= 0.0) != (v >= 0.0)
        {
            let u = if (w - v).abs() > 1e-300 { (w / (w - v)).clamp(0.0, 1.0) } else { 0.5 };
            let x = q.lerp(p, u);
            if w >= 0.0 {
                // Leaving what is kept.
                cur.push(x);
                if cur.len() >= 2 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
            } else {
                cur.clear();
                cur.push(x);
            }
        }
        if v >= 0.0 {
            cur.push(p);
        }
        prev = Some((p, v));
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out.retain(|r| r.windows(2).any(|w| w.first().zip(w.get(1)).is_some_and(|(a, b)| a.dist(*b) > 1e-9)));
    out
}

/// One pass to cut: a line seen from above, followed at `level` or the surface, whichever is
/// higher.
struct Run {
    pts: Vec<Vec2>,
    level: f64,
}

/// Everything a strategy cuts with.
struct Plan<'a> {
    /// The part seen by the cutter, inside the boundary's region.
    field: Field<'a>,
    /// The same over the whole rectangle.
    all: Field<'a>,
    map: Map,
    heights: Heights,
    feeds: Feeds,
    /// The feed the passes go down the surface at ([`ParallelParams::descent_feed`]).
    descent: f64,
    /// The base sample step along a pass.
    step: f64,
    tol: f64,
    link_len: f64,
}

/// The checks every strategy makes, and its grid.
#[allow(clippy::too_many_arguments)]
fn prepare<'a>(surface: &'a Surface, tool: &Tool, h: &Heights, feeds: &Feeds, b: &Boundary, leave: f64, step: f64, tol: f64) -> Result<Plan<'a>> {
    let cutter = tool_cutter(tool)?;
    h.validate()?;
    feeds.validate()?;
    check_leave(leave)?;
    check_step(step)?;
    check_tolerance(tol)?;
    let rect = b.resolve(surface)?;
    let field = Field::new(surface, &cutter, leave, b.region.as_ref(), tol)?;
    field.check_retract(&rect, h.retract)?;
    let all = Field::new(surface, &cutter, leave, None, tol)?;
    let fence = b.region.as_ref().map(|r| Fence::new(r, tol.min(1e-3)));
    let map = Map::build(&all, fence.as_ref(), rect, step, h.bottom)?;
    Ok(Plan { field, all, map, heights: *h, feeds: *feeds, descent: feeds.cut, step: step.min(cutter.radius), tol, link_len: tool.diameter })
}

impl Plan<'_> {
    /// The cutter as it rests on the surface (grown by the stock to leave).
    fn cutter(&self) -> Cutter {
        self.field.cutter
    }

    /// The distance between passes, and whether it was sized for a scallop height.
    fn spacing(&self, s: Spacing, tool: &Tool) -> Result<(f64, bool)> {
        match s {
            Spacing::Stepover(d) => {
                if finite_in(d, 1e-4, tool.diameter) {
                    Ok((d, false))
                } else {
                    invalid("the stepover must be above 0 and at most the tool diameter")
                }
            }
            Spacing::ScallopHeight(h) => {
                let rc = self.cutter().corner;
                if !(rc > 0.0) {
                    return invalid("a flat end mill leaves no scallop to space passes by: give a stepover");
                }
                if !finite_in(h, 1e-6, rc) {
                    return invalid("the scallop height must be above 0 and at most the cutter's corner radius");
                }
                Ok(((2.0 * (2.0 * rc * h - h * h).max(0.0).sqrt()).min(tool.diameter), true))
            }
        }
    }

    /// Each node's length scale for passes a scallop height apart: where the surface is convex
    /// a ridge between passes a given distance apart is lower than on a plane, where concave
    /// higher, by `1 - corner · curvature` of the tip's height; the passes are moved apart or
    /// together by its square root, within a factor of two either way and never more than the
    /// tool's diameter apart.
    fn weights(&self, scallop: bool, d: f64, diameter: f64) -> Vec<f64> {
        let n = self.map.len();
        if !scallop {
            return vec![1.0; n];
        }
        let rc = self.cutter().corner;
        let m = ((0.5 * rc) / self.map.gx.min(self.map.gy)).round().max(1.0) as usize;
        let lo = 0.5f64.max(d / diameter.max(1e-9)).min(1.0);
        (0..n)
            .map(|k| match self.map.curvature(k, m) {
                Some(c) if self.map.onk(k) => (1.0 - rc * c).max(0.0).sqrt().clamp(lo, 2.0),
                _ => 1.0,
            })
            .collect()
    }

    /// Passes `d` apart over the nodes of `domain`, from its edge (the boundary's rectangle, and
    /// nodes outside it) inward, as groups to run in turn: one per distance, then the passes
    /// that fill gaps where fronts from different sides meet further apart than the spacing.
    fn offsets(&self, domain: &[bool], weight: &[f64], d: f64) -> Result<Vec<Vec<Vec<Vec2>>>> {
        let map = &self.map;
        let inside = |k: usize| domain.get(k).copied().unwrap_or(false);
        let pos = |k: usize| inside(k).then(|| map.p3(k));
        let seeds: Vec<(usize, f64)> = (0..map.len())
            .filter(|&k| inside(k) && (map.on_border(k) || RING.iter().any(|&(a, b)| map.neighbour(k, a, b).is_some_and(|q| !inside(q)))))
            .map(|k| (k, 0.0))
            .collect();
        let t = march(map, &pos, weight, &seeds, f64::INFINITY);
        let field = |t: &[f64]| -> Vec<f64> { t.iter().enumerate().map(|(k, v)| if inside(k) && v.is_finite() { *v } else { -1.0 }).collect() };
        let f = field(&t);
        let top = f.iter().copied().fold(0.0, f64::max);
        let count = (top / d).ceil();
        if !(count <= MAX_LEVELS as f64) {
            return Err(CamError::TooLarge(format!("more than {MAX_LEVELS} passes deep; use a larger spacing")));
        }
        let first = (1e-3 * d.min(map.gx.min(map.gy))).max(1e-9);
        let mut lv: Vec<f64> = (0..count as usize).map(|k| if k == 0 { first } else { d * k as f64 }).filter(|l| *l < top).collect();
        if lv.is_empty() && top > first {
            lv.push(first);
        }
        let lines = iso_lines(map, &f, &lv, true, &|a, b, l| Ok(linear(map, &f, a, b, l)))?;
        let mut groups: Vec<Vec<Vec<Vec2>>> = (0..lv.len()).map(|_| Vec::new()).collect();
        let mut all: Vec<Vec<Vec2>> = Vec::new();
        for l in lines {
            if let Some(g) = groups.get_mut(l.level) {
                all.push(l.pts.clone());
                g.push(l.pts);
            }
        }
        // Gaps: wherever every pass so far is further than the gap share of the spacing.
        for _ in 0..GAP_ROUNDS {
            let mut near = vec![f64::INFINITY; map.len()];
            for &(k, _) in &seeds {
                if let Some(v) = near.get_mut(k) {
                    *v = 0.0;
                }
            }
            for p in all.iter().flatten() {
                let (i, j, ..) = map.cell(*p);
                let at = p.extend(map.sample(&map.z, *p));
                for k in [map.id(i, j), map.id(i + 1, j), map.id(i + 1, j + 1), map.id(i, j + 1)] {
                    if let Some(v) = near.get_mut(k) {
                        *v = v.min(weight.get(k).copied().unwrap_or(1.0) * map.p3(k).dist(at));
                    }
                }
            }
            let near: Vec<(usize, f64)> = near.iter().enumerate().filter(|(_, v)| v.is_finite()).map(|(k, v)| (k, *v)).collect();
            let gap = march(map, &pos, weight, &near, f64::INFINITY);
            let g = field(&gap);
            let level = GAP * d;
            if !g.iter().any(|v| *v > level) {
                break;
            }
            let lines = iso_lines(map, &g, &[level], true, &|a, b, l| Ok(linear(map, &g, a, b, l)))?;
            if lines.is_empty() {
                break;
            }
            let pts: Vec<Vec<Vec2>> = lines.into_iter().map(|l| l.pts).collect();
            all.extend(pts.iter().cloned());
            groups.push(pts);
        }
        Ok(groups)
    }

    /// The z-level lines at each of `zs` (ascending), each with the material on its left: every
    /// crossing found to within the tolerance by halving with the drop-cutter, on the side
    /// where the cutter is clear of the level.
    fn waterlines(&self, zs: &[f64]) -> Result<Vec<Iso>> {
        let floor = self.heights.bottom;
        let crossing = |hi: usize, lo: usize, level: f64| -> Result<Vec2> {
            let (mut a, mut b) = (self.map.node(hi), self.map.node(lo));
            for _ in 0..64 {
                if a.dist(b) <= self.tol {
                    break;
                }
                let mid = a.lerp(b, 0.5);
                match self.all.at(mid)? {
                    Tip::At(h) if h.max(floor) > level => a = mid,
                    _ => b = mid,
                }
            }
            Ok(b)
        };
        // Every crossing is found first, on several threads; tracing then looks them up.
        let wanted = crossings(&self.map, &self.map.z, zs);
        let ranges = crate::par::split(&vec![1; wanted.len()], crate::par::threads_for(wanted.len(), CROSSINGS_PER_THREAD));
        let found = crate::par::map(ranges, |r| {
            Ok(wanted.get(r).unwrap_or_default().iter().map(|&(hi, lo, l)| ((hi, lo, l.to_bits()), crossing(hi, lo, l))).collect::<Vec<_>>())
        })?;
        let known: HashMap<(usize, usize, u64), Result<Vec2>> = found.into_iter().flatten().collect();
        let look = |hi: usize, lo: usize, level: f64| -> Result<Vec2> {
            match known.get(&(hi, lo, level.to_bits())) {
                Some(Ok(q)) => Ok(*q),
                Some(Err(e)) => Err(e.clone()),
                None => crossing(hi, lo, level),
            }
        };
        iso_lines(&self.map, &self.map.z, zs, false, &look)
    }

    /// The concave creases as lines (see [`pencil`]), with every point and the tip's height
    /// there.
    fn crease_lines(&self, min_angle: f64, min_length: f64) -> Result<(Vec<Vec<Vec2>>, Vec<Vec3>)> {
        let map = &self.map;
        let floor = self.heights.bottom;
        let height = |q: Vec2| -> Result<Option<f64>> {
            Ok(match self.all.at(q)? {
                Tip::At(h) => Some(h.max(floor)),
                _ => None,
            })
        };
        let g = map.gx.max(map.gy);
        let mut lines: Vec<Vec<Vec2>> = Vec::new();
        let mut points: Vec<Vec3> = Vec::new();
        for chain in creases(map, min_angle) {
            let nodes: Vec<Vec2> = chain.iter().map(|k| map.node(*k)).collect();
            let length: f64 = nodes.windows(2).map(|w| w.first().zip(w.get(1)).map_or(0.0, |(a, b)| a.dist(*b))).sum();
            if nodes.len() < 2 || length < min_length {
                continue;
            }
            let mut pts = Vec::with_capacity(nodes.len());
            for (i, q) in nodes.iter().enumerate() {
                let (a, b) = (nodes.get(i.saturating_sub(2)).unwrap_or(q), nodes.get(i + 2).or(nodes.last()).unwrap_or(q));
                let at = match (*b - *a).normalized().map(Vec2::perp) {
                    Some(n) => settle(&height, *q, n, g, self.tol)?,
                    None => *q,
                };
                if let Some(z) = height(at)? {
                    points.push(at.extend(z));
                }
                pts.push(at);
            }
            lines.push(pts);
        }
        Ok((lines, points))
    }

    /// A pass along every concave crease sharper than [`CLEANUP_ANGLE`] at least the cutter's
    /// radius long: where the cutter touches two faces at once (the foot of a wall, the
    /// corner of a pocket) offset and raster passes land within a grid step at best, and this
    /// one reaches it.
    fn cleanup(&self) -> Result<Vec<Run>> {
        let (lines, _) = self.crease_lines(CLEANUP_ANGLE.to_radians(), self.cutter().radius)?;
        Ok(lines.into_iter().map(|pts| Run { pts, level: self.heights.bottom }).collect())
    }

    /// The parts of a pass that cut, as points: sampled and refined along each of its
    /// segments, followed at `level` or the surface, whichever is higher, and left out where
    /// nothing is under the cutter.
    fn drape(&self, run: &Run) -> Result<Vec<Vec<Vec3>>> {
        let pts = &run.pts;
        let mut cum = Vec::with_capacity(pts.len());
        let mut total = 0.0;
        cum.push(0.0);
        for w in pts.windows(2) {
            if let [a, b] = w {
                total += a.dist(*b);
            }
            cum.push(total);
        }
        if !(total > 1e-9) {
            return Ok(Vec::new());
        }
        let at = |t: f64| -> Vec2 {
            let k = cum.partition_point(|c| *c < t).clamp(1, pts.len().saturating_sub(1).max(1));
            match (pts.get(k - 1), pts.get(k), cum.get(k - 1), cum.get(k)) {
                (Some(a), Some(b), Some(ca), Some(cb)) if cb - ca > 1e-12 => a.lerp(*b, ((t - ca) / (cb - ca)).clamp(0.0, 1.0)),
                (Some(a), ..) => *a,
                _ => Vec2::new(0.0, 0.0),
            }
        };
        #[cfg(test)]
        let dropped = std::cell::RefCell::new(Vec::new());
        let tip = |t: f64| {
            #[cfg(test)]
            dropped.borrow_mut().push(t);
            self.field.at(at(t))
        };
        let point = |t: f64, h: f64| at(t).extend(h);
        let pass = Pass { tip: &tip, point: &point, tol: self.tol };
        let depth = depth_for(self.step, self.tol);
        let mut profile: Profile = Vec::new();
        for w in cum.windows(2) {
            let [t0, t1] = w else { continue };
            if t1 - t0 <= 1e-12 {
                continue;
            }
            let n = ((t1 - t0) / self.step).ceil().max(1.0) as usize;
            // Each stretch starts where the one before ended: that sample is not taken again.
            let first = match profile.last() {
                Some(&(t, s)) if t == *t0 => (t, s),
                _ => (*t0, tip(*t0)?),
            };
            let part = pass.sample_within_from(first, *t1, n, depth, self.field.stretch)?;
            let skip = usize::from(!profile.is_empty());
            profile.extend(part.into_iter().skip(skip));
            if profile.len() > MAX_MOVES {
                return Err(CamError::TooLarge(format!("a pass has more than {MAX_MOVES} points")));
            }
        }
        #[cfg(test)]
        {
            let mut ts = dropped.into_inner();
            let all = ts.len();
            ts.sort_by(f64::total_cmp);
            ts.dedup();
            DRAPE_DROPS.with(|n| n.set(n.get() + all));
            DRAPE_REDROPS.with(|n| n.set(n.get() + all - ts.len()));
        }
        let stretches = spans(&profile, run.level, f64::INFINITY, false, &|t| self.field.at(at(t)))?;
        Ok(stretches.into_iter().map(|s| s.into_iter().map(|(t, z)| at(t).extend(z)).collect()).collect())
    }

    /// Cuts the groups in turn, each group's passes nearest first, linked as a raster's.
    fn cut(&self, groups: Vec<Vec<Run>>) -> Result<Toolpath> {
        let spacing = self.step.min(self.cutter().radius * 0.5);
        let link = |a: Vec3, b: Vec3| {
            solidcraft_profile::scope("cam", "steep_shallow.link", || link_path(&self.field, a, b, self.link_len, spacing, self.tol))
        };
        let mut out = Out3::new(self.heights, self.feeds, DEFAULT_DESCENT_LIMIT).descending(self.descent);
        let mut at: Option<Vec2> = None;
        for group in groups {
            // The group's passes are draped side by side (each on its own, from the surface
            // alone) and linked in order here: the same path as one after the other.
            let runs = nearest_first(group, at);
            let threads = crate::par::threads_for(runs.len(), RUNS_PER_THREAD);
            #[cfg(test)]
            if threads > 1 {
                DRAPED_SIDE_BY_SIDE.with(|n| n.set(n.get() + runs.len()));
            }
            let draped = solidcraft_profile::scope("cam", "steep_shallow.drape", || {
                crate::par::map_pulled(runs.len(), threads, |k| runs.get(k).map_or(Ok(Vec::new()), |r| self.drape(r)))
            })?;
            for pieces in draped {
                for piece in pieces? {
                    solidcraft_profile::scope("cam", "steep_shallow.span", || out.span(&piece, &link, None))?;
                    if let Some(p) = piece.last() {
                        at = Some(Vec2::new(p.x, p.y));
                    }
                }
            }
        }
        solidcraft_profile::scope("cam", "steep_shallow.finish", || out.finish_over(&self.field))
    }
}

/// Fewest passes of a group draped on one thread ([`Plan::cut`]).
const RUNS_PER_THREAD: usize = 4;

#[cfg(test)]
thread_local! {
    /// Passes [`Plan::cut`] draped side by side on this thread's calls.
    pub(crate) static DRAPED_SIDE_BY_SIDE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Drop-cutter samples [`Plan::drape`] took on this thread.
    pub(crate) static DRAPE_DROPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Samples [`Plan::drape`] took again at a parameter of the same pass it had sampled.
    pub(crate) static DRAPE_REDROPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The runs in the order a tool at `from` reaches them nearest first; a closed run is started
/// at its point nearest the tool. A group too large to sort keeps its order.
fn nearest_first(mut runs: Vec<Run>, mut from: Option<Vec2>) -> Vec<Run> {
    if runs.len() > MAX_ORDERED {
        return runs;
    }
    let mut out = Vec::with_capacity(runs.len());
    while !runs.is_empty() {
        let Some(p) = from else {
            let r = runs.remove(0);
            from = r.pts.last().copied();
            out.push(r);
            continue;
        };
        // (distance, run, point to start at)
        let mut best = (f64::INFINITY, 0, 0);
        for (i, r) in runs.iter().enumerate() {
            if is_closed(&r.pts) {
                for (k, q) in r.pts.iter().enumerate() {
                    let d = q.dist(p);
                    if d < best.0 {
                        best = (d, i, k);
                    }
                }
            } else if let Some(q) = r.pts.first() {
                let d = q.dist(p);
                if d < best.0 {
                    best = (d, i, 0);
                }
            }
        }
        let mut r = runs.remove(best.1.min(runs.len() - 1));
        if best.2 > 0 && is_closed(&r.pts) {
            let n = r.pts.len() - 1;
            let k = best.2.min(n);
            let mut pts: Vec<Vec2> = r.pts.iter().skip(k).take(n - k).copied().collect();
            pts.extend(r.pts.iter().take(k + 1).copied());
            r.pts = pts;
        }
        from = r.pts.last().copied();
        out.push(r);
    }
    out
}

/// XY lines as runs at `level`, each turned so what is still to cut is on its right for climb
/// milling and on its left for conventional. The lines are distance lines traced with the
/// further side on their left; `ahead_further` says whether the passes still to cut are on that
/// side (passes run away from where the distance is measured from) or on the nearer side (they
/// run towards it: outward offsets, a pencil's offsets closing on its crease).
fn runs_at(lines: Vec<Vec<Vec2>>, level: f64, climb: bool, ahead_further: bool) -> Vec<Run> {
    lines
        .into_iter()
        .map(|mut pts| {
            if climb == ahead_further {
                pts.reverse();
            }
            Run { pts, level }
        })
        .collect()
}

/// Constant-scallop finishing: passes offset from the boundary (its rectangle, and its region
/// when it has one) along the surface, each a spacing from the one before as measured over the
/// cutter-location surface, so the scallop left between them is the same on steep and flat
/// ground. With a scallop height the spacing follows the surface's curvature too (closer where
/// it is concave). Where fronts from different sides meet further apart than the spacing, a
/// pass is added between them. Passes in cliffs of the height field (walls the cutter's side
/// runs down) are left out: finish those with [`steep_shallow`] or a waterline.
pub fn scallop(surface: &Surface, tool: &Tool, p: &ScallopParams) -> Result<Toolpath> {
    let _span = solidcraft_profile::span("cam", "scallop");
    let mut plan = prepare(surface, tool, &p.heights, &p.feeds, &p.boundary, p.stock_to_leave, p.step, p.tolerance)?;
    plan.descent = descent_of(&p.feeds, p.descent_feed)?;
    let (d, by_height) = plan.spacing(p.spacing, tool)?;
    plan.link_len = (2.0 * d).max(tool.diameter);
    let weight = plan.weights(by_height, d, tool.diameter);
    let domain = plan.map.inside.clone();
    let inward = p.direction == OffsetDirection::Inward;
    let mut groups: Vec<Vec<Run>> = plan.offsets(&domain, &weight, d)?.into_iter().map(|g| runs_at(g, p.heights.bottom, p.climb, inward)).collect();
    if !inward {
        groups.reverse();
    }
    groups.push(plan.cleanup()?);
    plan.cut(groups)
}

/// Distance over the plane (heights left out) from the nodes `from` picks, up to `limit`.
fn flat_distance(map: &Map, from: &dyn Fn(usize) -> bool, limit: f64) -> Vec<f64> {
    let seeds: Vec<(usize, f64)> = (0..map.len()).filter(|k| from(*k)).map(|k| (k, 0.0)).collect();
    let pos = |k: usize| Some(map.node(k).extend(0.0));
    // Far is a large number rather than infinity, so it can be interpolated.
    march(map, &pos, &vec![1.0; map.len()], &seeds, limit).into_iter().map(|v| v.min(1e9)).collect()
}

/// Z-level passes where the ground is steep and offset or raster passes where it is shallow.
/// The slope is that of the tip's height (as the cutter meets the surface, so the side of a
/// ball rolling over an edge counts as steep). Each kind runs `overlap` on into the other's
/// ground. The z-levels are traced on the grid, each crossing found to within the tolerance by
/// halving with the drop-cutter on the side where the cutter is clear, and then followed like
/// every pass here; they run top down, climb milling keeping the wall on the right. On shallow
/// ground, scallop passes are offset from the boundary and from the edge of the steep ground.
pub fn steep_shallow(surface: &Surface, tool: &Tool, p: &SteepShallowParams) -> Result<Toolpath> {
    let _span = solidcraft_profile::span("cam", "steep_shallow");
    let mut plan = solidcraft_profile::scope("cam", "steep_shallow.prepare", || {
        prepare(surface, tool, &p.heights, &p.feeds, &p.boundary, p.stock_to_leave, p.step, p.tolerance)
    })?;
    plan.descent = descent_of(&p.feeds, p.descent_feed)?;
    let (d, by_height) = plan.spacing(p.spacing, tool)?;
    if !finite_in(p.threshold, 1f64.to_radians(), 89f64.to_radians()) {
        return invalid("the steep threshold must be from 1° to 89°");
    }
    if !finite_in(p.overlap, 0.0, 1000.0) {
        return invalid("the overlap must be from 0 to 1000 mm");
    }
    let stepdown = p.stepdown.unwrap_or(d * p.threshold.sin());
    check_stepdown(stepdown)?;
    if let ShallowPattern::Parallel { angle } = p.shallow {
        check_angle(angle)?;
    }
    plan.link_len = (2.0 * d).max(tool.diameter);
    let map = &plan.map;
    let limit = p.threshold.tan();
    // Steep where the tip's height is steep on both sides of a node, and either side of a
    // cliff (where it jumps between two nodes).
    let cliffside = |k: usize| {
        [(1, 0, map.gx), (-1, 0, map.gx), (0, 1, map.gy), (0, -1, map.gy)]
            .iter()
            .any(|&(di, dj, g)| map.neighbour(k, di, dj).is_some_and(|q| (map.zk(q) - map.zk(k)).abs() > CLIFF * g))
    };
    let steep: Vec<bool> =
        (0..map.len()).map(|k| map.onk(k) && (cliffside(k) || map.grad_gentle(k).is_some_and(|g| g.x.hypot(g.y) > limit))).collect();
    let is_steep = |k: usize| steep.get(k).copied().unwrap_or(false);
    let reach = p.overlap + 0.5 * map.gx.max(map.gy);
    let to_steep = flat_distance(map, &is_steep, reach * 1.5 + map.gx + map.gy);
    let to_shallow = flat_distance(map, &|k| !is_steep(k), reach * 1.5 + map.gx + map.gy);
    let keep_steep = |q: Vec2| reach - map.sample(&to_steep, q);
    let keep_shallow = |q: Vec2| reach - map.sample(&to_shallow, q);
    let mut groups: Vec<Vec<Run>> = Vec::new();

    // Steep ground: z-levels.
    let (mut zmax, mut zmin) = (f64::NEG_INFINITY, f64::INFINITY);
    for k in (0..map.len()).filter(|k| is_steep(*k)) {
        zmax = zmax.max(map.zk(k));
        zmin = zmin.min(map.zk(k));
    }
    let (top, bottom) = (zmax.min(p.heights.top), zmin.max(p.heights.bottom));
    if top.is_finite() && bottom.is_finite() && top > bottom {
        let mut zs = levels(top, bottom, stepdown, true)?;
        zs.reverse();
        let lines = solidcraft_profile::scope("cam", "steep_shallow.waterlines", || plan.waterlines(&zs))?;
        let mut by_level: Vec<Vec<Run>> = (0..zs.len()).map(|_| Vec::new()).collect();
        for l in lines {
            let (Some(g), Some(z)) = (by_level.get_mut(l.level), zs.get(l.level)) else { continue };
            for mut pts in clip(l.pts, &keep_steep) {
                if p.climb {
                    pts.reverse();
                }
                g.push(Run { pts, level: *z });
            }
        }
        by_level.reverse();
        groups.extend(by_level);
    }
    groups.push(solidcraft_profile::scope("cam", "steep_shallow.cleanup", || plan.cleanup())?);

    // Shallow ground.
    match p.shallow {
        ShallowPattern::Scallop => {
            let weight = plan.weights(by_height, d, tool.diameter);
            let domain: Vec<bool> = (0..map.len()).map(|k| map.inside.get(k).copied().unwrap_or(false) && !is_steep(k)).collect();
            for g in plan.offsets(&domain, &weight, d)? {
                let lines: Vec<Vec<Vec2>> = g.into_iter().flat_map(|l| clip(l, &keep_shallow)).collect();
                groups.push(runs_at(lines, p.heights.bottom, p.climb, true));
            }
        }
        ShallowPattern::Parallel { angle } => {
            // Close enough that passes are a spacing apart along the steepest shallow ground.
            let gap = d * p.threshold.cos();
            let dir = Vec2::from_angle(angle);
            let nrm = dir.perp();
            let r = &map.rect;
            let corners = [r.min, Vec2::new(r.max.x, r.min.y), r.max, Vec2::new(r.min.x, r.max.y)];
            let (lo, hi) = corners.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), c| (a.min(c.dot(nrm)), b.max(c.dot(nrm))));
            let (ta, tb) = corners.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), c| (a.min(c.dot(dir)), b.max(c.dot(dir))));
            let count = ((hi - lo) / gap).ceil();
            let along = ((tb - ta) / map.gx.min(map.gy)).ceil();
            // Every point of these lines becomes at least one move once they are cut, so lines
            // with far more points than a path may hold are refused before they are built.
            if !((count + 1.0) * (along + 1.0) <= MAX_LINE_POINTS as f64) {
                return Err(CamError::TooLarge(format!("the shallow passes have more than {MAX_LINE_POINTS} points; use a larger spacing or step")));
            }
            let inside_rect = |q: Vec2| {
                let e = 1e-9;
                (q.x - r.min.x + e).min(r.max.x + e - q.x).min(q.y - r.min.y + e).min(r.max.y + e - q.y)
            };
            let mut runs = Vec::new();
            for k in 0..=(count as usize) {
                let v = lo + (hi - lo) * k as f64 / count.max(1.0);
                let n = along.max(1.0) as usize;
                let pts: Vec<Vec2> = (0..=n).map(|s| nrm * v + dir * (ta + (tb - ta) * s as f64 / n as f64)).collect();
                for piece in clip(pts, &|q| inside_rect(q).min(keep_shallow(q))) {
                    let pts = if p.climb == (k % 2 == 1) { piece.into_iter().rev().collect() } else { piece };
                    runs.push(Run { pts, level: p.heights.bottom });
                }
            }
            groups.push(runs);
        }
    }
    solidcraft_profile::scope("cam", "steep_shallow.cut", || plan.cut(groups))
}

/// How sharply the tip's height bends up at a node along one grid direction, in radians,
/// positive where it is concave (a valley): the turn of its slope at the node and at the next
/// node when that is concave too (a crease between the two splits its turn between them), less
/// twice the turn at the nodes either side of those. A crease turns at one place with no turn round
/// it; a smooth curve turns about as much at its neighbours, which is taken off, so a fillet
/// the cutter rolls along is no crease even where it rises steeply and a grid step spans much
/// of its turn.
fn kink(map: &Map, k: usize, di: isize, dj: isize) -> Option<f64> {
    let len = (di as f64 * map.gx).hypot(dj as f64 * map.gy);
    let z = |s: isize| -> Option<f64> {
        let q = if s == 0 { Some(k) } else { map.neighbour(k, di * s, dj * s) }?;
        map.onk(q).then(|| map.zk(q))
    };
    let zs = [z(-2)?, z(-1)?, z(0)?, z(1)?, z(2)?, z(3)?];
    let slope = |i: usize| -> f64 { zs.get(i).zip(zs.get(i + 1)).map_or(0.0, |(a, b)| ((b - a) / len).atan()) };
    // The turn at the node `n` places along from the first.
    let turn = |n: usize| slope(n) - slope(n - 1);
    let (before, at, next, after) = (turn(1), turn(2), turn(3), turn(4));
    Some(at + next.max(0.0) - 2.0 * before.max(after).max(0.0))
}

/// The concave creases of the tip's height: nodes whose kink is at least `min_angle` and the
/// largest along its direction, as chains of neighbouring nodes.
fn creases(map: &Map, min_angle: f64) -> Vec<Vec<usize>> {
    const DIRS: [(isize, isize); 4] = [(1, 0), (0, 1), (1, 1), (1, -1)];
    let n = map.len();
    let mut mark = vec![false; n];
    for k in 0..n {
        if !map.inside.get(k).copied().unwrap_or(false) {
            continue;
        }
        let mut best: Option<(f64, (isize, isize))> = None;
        for (di, dj) in DIRS {
            if let Some(v) = kink(map, k, di, dj)
                && best.is_none_or(|b| v > b.0)
            {
                best = Some((v, (di, dj)));
            }
        }
        let Some((v, (di, dj))) = best else { continue };
        if !(v >= min_angle) {
            continue;
        }
        let side = |s: isize| map.neighbour(k, di * s, dj * s).and_then(|q| kink(map, q, di, dj)).unwrap_or(f64::NEG_INFINITY);
        if v >= side(-1)
            && v > side(1)
            && let Some(m) = mark.get_mut(k)
        {
            *m = true;
        }
    }
    let marked = |k: usize| mark.get(k).copied().unwrap_or(false);
    let next = |k: usize, used: &[bool]| -> Vec<usize> {
        RING.iter().filter_map(|&(a, b)| map.neighbour(k, a, b)).filter(|q| marked(*q) && !used.get(*q).copied().unwrap_or(true)).collect()
    };
    let mut used = vec![false; n];
    let mut out = Vec::new();
    // Ends first (a node with one marked neighbour), then whatever is left (loops).
    let degree = |k: usize| RING.iter().filter_map(|&(a, b)| map.neighbour(k, a, b)).filter(|q| marked(*q)).count();
    let starts: Vec<usize> = (0..n).filter(|k| marked(*k) && degree(*k) <= 1).chain((0..n).filter(|k| marked(*k))).collect();
    for s in starts {
        if used.get(s).copied().unwrap_or(true) {
            continue;
        }
        let mut chain = vec![s];
        if let Some(u) = used.get_mut(s) {
            *u = true;
        }
        let mut at = s;
        let mut heading: Option<Vec2> = None;
        loop {
            let options = next(at, &used);
            // Straight on where it can.
            let pick = options.into_iter().max_by(|a, b| {
                let score = |q: usize| {
                    let d = map.node(q) - map.node(at);
                    let dn = d.normalized().unwrap_or(d);
                    heading.map_or(0.0, |h| h.dot(dn)) - 1e-6 * d.x.hypot(d.y)
                };
                score(*a).total_cmp(&score(*b))
            });
            let Some(q) = pick else { break };
            heading = (map.node(q) - map.node(at)).normalized();
            if let Some(u) = used.get_mut(q) {
                *u = true;
            }
            chain.push(q);
            at = q;
        }
        // A chain that comes back next to where it started is a loop.
        if chain.len() > 3 && chain.first().is_some_and(|f| RING.iter().any(|&(a, b)| map.neighbour(at, a, b) == Some(*f))) {
            chain.push(s);
        }
        out.push(chain);
    }
    out
}

/// Pencil finishing: along the concave creases of the surface as the cutter meets it, where it
/// touches two faces at once (a corner sharper than the cutter, a fillet tighter than it). A
/// crease is where the tip's height bends up by at least `min_angle` across a few grid nodes,
/// at the node where it bends most; neighbouring crease nodes are joined into chains and chains
/// shorter than `min_length` are left out. Each crease point is then moved across the crease
/// to where the tip's height leaves the straight line of its flatter side, found by halving
/// with the drop-cutter, so the pass runs in the crease rather than on the nearest node. With
/// `passes`, offset passes `stepover` apart along the surface run on both sides of every crease
/// (outermost first), then the crease itself.
pub fn pencil(surface: &Surface, tool: &Tool, p: &PencilParams) -> Result<Toolpath> {
    let _span = solidcraft_profile::span("cam", "pencil");
    let mut plan = prepare(surface, tool, &p.heights, &p.feeds, &p.boundary, p.stock_to_leave, p.step, p.tolerance)?;
    plan.descent = descent_of(&p.feeds, p.descent_feed)?;
    if !finite_in(p.min_angle, 1f64.to_radians(), 170f64.to_radians()) {
        return invalid("the least crease angle must be from 1° to 170°");
    }
    if p.passes > MAX_PENCIL_PASSES {
        return invalid(format!("a pencil runs at most {MAX_PENCIL_PASSES} offset passes each side"));
    }
    if p.passes > 0 && !finite_in(p.stepover, 1e-4, tool.diameter) {
        return invalid("the stepover must be above 0 and at most the tool diameter");
    }
    if !finite_in(p.min_length, 0.0, 1.0e6) {
        return invalid("the least crease length must be from 0 to 1000 m");
    }
    plan.link_len = (2.0 * p.stepover).max(tool.diameter);
    let map = &plan.map;
    let floor = p.heights.bottom;
    let g = map.gx.max(map.gy);
    let (lines, points) = plan.crease_lines(p.min_angle, p.min_length)?;
    let mut groups: Vec<Vec<Run>> = Vec::new();
    if p.passes > 0 && !lines.is_empty() {
        let mut seed = vec![f64::INFINITY; map.len()];
        for &at in &points {
            let (i, j, ..) = map.cell(Vec2::new(at.x, at.y));
            for k in [map.id(i, j), map.id(i + 1, j), map.id(i + 1, j + 1), map.id(i, j + 1)] {
                if let Some(v) = seed.get_mut(k) {
                    *v = v.min(map.p3(k).dist(at));
                }
            }
        }
        let seeds: Vec<(usize, f64)> = seed.iter().enumerate().filter(|(_, v)| v.is_finite()).map(|(k, v)| (k, *v)).collect();
        let inside = |k: usize| map.inside.get(k).copied().unwrap_or(false);
        let pos = |k: usize| inside(k).then(|| map.p3(k));
        let reach = p.stepover * f64::from(p.passes) + 3.0 * g;
        let t = march(map, &pos, &vec![1.0; map.len()], &seeds, reach);
        let f: Vec<f64> = t.iter().enumerate().map(|(k, v)| if inside(k) { v.min(1e12) } else { -1.0 }).collect();
        let lv: Vec<f64> = (1..=p.passes).map(|k| p.stepover * f64::from(k)).collect();
        let mut by_level: Vec<Vec<Vec<Vec2>>> = (0..lv.len()).map(|_| Vec::new()).collect();
        for l in iso_lines(map, &f, &lv, true, &|a, b, l| Ok(linear(map, &f, a, b, l)))? {
            if let Some(g) = by_level.get_mut(l.level) {
                g.push(l.pts);
            }
        }
        for g in by_level.into_iter().rev() {
            // Outermost first, closing on the crease: what is still to cut is crease-side.
            groups.push(runs_at(g, floor, p.climb, false));
        }
    }
    groups.push(lines.into_iter().map(|pts| Run { pts, level: floor }).collect());
    plan.cut(groups)
}

/// The crease point near `q`, looking along the unit vector `n` either way up to twice the
/// grid step `g`: where the tip's height leaves the straight line through the two samples
/// furthest out on the flatter side, found to within `tol` by halving. `q` itself when the
/// height does not leave that line, or nothing is under the cutter out there.
fn settle(height: &dyn Fn(Vec2) -> Result<Option<f64>>, q: Vec2, n: Vec2, g: f64, tol: f64) -> Result<Vec2> {
    let at = |s: f64| height(q + n * s);
    let (Some(l3), Some(l2), Some(r2), Some(r3)) = (at(-3.0 * g)?, at(-2.0 * g)?, at(2.0 * g)?, at(3.0 * g)?) else { return Ok(q) };
    let (sl, sr) = ((l2 - l3) / g, (r3 - r2) / g);
    // The flatter side's line, looked along from that side.
    let (sign, base, slope) = if sl.abs() <= sr.abs() { (1.0, l2, sl) } else { (-1.0, r2, -sr) };
    let line = |s: f64| base + slope * (s + 2.0 * g);
    let off = |s: f64| -> Result<bool> {
        Ok(match at(sign * s)? {
            Some(h) => (h - line(s)).abs() > tol,
            None => true,
        })
    };
    let (mut a, mut b) = (-2.0 * g, 2.0 * g);
    if !off(b)? {
        return Ok(q);
    }
    for _ in 0..64 {
        if b - a <= tol {
            break;
        }
        let m = 0.5 * (a + b);
        if off(m)? {
            b = m;
        } else {
            a = m;
        }
    }
    Ok(q + n * (sign * a))
}

/// Rest finishing: only where this tool reaches material the previous, larger tool left. At
/// every node of the grid the tool's contact with the surface is found from its tip and the
/// slope of the tip's height (the surface's normal there); the previous tool is set touching
/// the surface at the same point with the same normal and dropped there. Where it rests higher
/// than that by more than the threshold, what it left at the point is measured from the
/// envelope of every position it can take on the grid: the lowest its cutting surface reaches
/// over the point, less the point's height, square to the surface. Where that is more than the
/// threshold the point is to cut. Those places, grown by `overlap` measured over the tip's
/// surface (so not up a cliff onto ground nothing was left on), are cut with passes offset
/// from their edges inward, as [`scallop`] offsets from a boundary. A path with nothing to cut
/// has no moves.
pub fn rest_finish(surface: &Surface, tool: &Tool, p: &RestParams) -> Result<Toolpath> {
    let _span = solidcraft_profile::span("cam", "rest_finish");
    let mut plan = prepare(surface, tool, &p.heights, &p.feeds, &p.boundary, p.stock_to_leave, p.step, p.tolerance)?;
    plan.descent = descent_of(&p.feeds, p.descent_feed)?;
    let (d, by_height) = plan.spacing(p.spacing, tool)?;
    let big = tool_cutter(&p.previous)?.grown(p.stock_to_leave)?;
    let small = plan.cutter();
    if (big.radius - small.radius).abs() <= 1e-9 && (big.corner - small.corner).abs() <= 1e-9 {
        return invalid("the previous tool has this tool's shape: it left nothing this one can reach");
    }
    if !finite_in(p.threshold, 1e-6, 1000.0) {
        return invalid("the rest threshold must be above 0 and at most 1000 mm");
    }
    if !finite_in(p.overlap, 0.0, 1000.0) {
        return invalid("the overlap must be from 0 to 1000 mm");
    }
    plan.link_len = (2.0 * d).max(tool.diameter);
    let map = &plan.map;
    let leave = p.stock_to_leave;
    let floor = p.heights.bottom;
    // The previous tool's tip (of the grown cutter) at every node.
    let prev = Field::new(surface, &tool_cutter(&p.previous)?, leave, None, p.tolerance)?;
    let mut zb = Vec::with_capacity(map.len());
    for k in 0..map.len() {
        zb.push(match prev.at(map.node(k))? {
            Tip::At(h) => h.max(floor) - leave,
            _ => floor - leave,
        });
    }
    let reach = (big.radius / map.gx).ceil() as isize;
    let reach_y = (big.radius / map.gy).ceil() as isize;
    let per = (2 * reach + 1) as f64 * (2 * reach_y + 1) as f64;
    let mut work = 0.0;
    let mut rest = vec![false; map.len()];
    for k in 0..map.len() {
        if !map.onk(k) || !map.inside.get(k).copied().unwrap_or(false) {
            continue;
        }
        let Some(gr) = map.grad_gentle(k) else { continue };
        let Some(nrm) = Vec3::new(-gr.x, -gr.y, 1.0).normalized() else { continue };
        let u = -nrm;
        let flat = Vec2::new(u.x, u.y).normalized().map_or(Vec3::new(0.0, 0.0, 0.0), |v| v.extend(0.0));
        // The grown cutter's point with outward normal `u`: on its corner, or under its axis
        // on a floor.
        let tip = map.node(k).extend(map.zk(k) - leave);
        let contact = |c: &Cutter, tip: Vec3| tip + Vec3::new(0.0, 0.0, c.corner) + flat * c.flat_radius() + u * c.corner;
        let point = contact(&small, tip);
        let big_tip = point - Vec3::new(0.0, 0.0, big.corner) - flat * big.flat_radius() - u * big.corner;
        let Some(rests) = drop_z(surface, &big, big_tip.x, big_tip.y) else { continue };
        if !(rests.max(floor - leave) - big_tip.z > p.threshold) {
            continue;
        }
        work += per;
        if !(work <= MAX_WORK) {
            return Err(CamError::TooLarge(format!("the rest test makes more than {MAX_WORK} comparisons; use a larger step")));
        }
        // The lowest the previous tool's cutting surface reaches over the point.
        let (ci, cj, ..) = map.cell(Vec2::new(point.x, point.y));
        let mut lowest = f64::INFINITY;
        for dj in -reach_y..=reach_y + 1 {
            for di in -reach..=reach + 1 {
                let (Some(i), Some(j)) = (ci.checked_add_signed(di), cj.checked_add_signed(dj)) else { continue };
                if i > map.nx || j > map.ny {
                    continue;
                }
                let q = map.id(i, j);
                let Some(h) = big.height(map.node(q).dist(Vec2::new(point.x, point.y))) else { continue };
                lowest = lowest.min(zb.get(q).copied().unwrap_or(f64::INFINITY) + h);
            }
        }
        if (lowest - point.z) * nrm.z > p.threshold
            && let Some(r) = rest.get_mut(k)
        {
            *r = true;
        }
    }
    if !rest.iter().any(|r| *r) {
        return Out3::new(p.heights, p.feeds, DEFAULT_DESCENT_LIMIT).finish();
    }
    let is_rest = |k: usize| rest.get(k).copied().unwrap_or(false);
    // Grown over the tip's surface, not the plane, so the overlap runs on over the ground round
    // what is left and stops at a cliff (the small tool riding up onto a wall's top edge, where
    // nothing was left).
    let seeds: Vec<(usize, f64)> = (0..map.len()).filter(|k| is_rest(*k)).map(|k| (k, 0.0)).collect();
    let grow = march(map, &|k| Some(map.p3(k)), &vec![1.0; map.len()], &seeds, p.overlap + 1e-9);
    let domain: Vec<bool> = grow.iter().enumerate().map(|(k, v)| *v <= p.overlap + 1e-9 && map.inside.get(k).copied().unwrap_or(false)).collect();
    let weight = plan.weights(by_height, d, tool.diameter);
    let mut groups: Vec<Vec<Run>> = plan.offsets(&domain, &weight, d)?.into_iter().map(|g| runs_at(g, floor, p.climb, true)).collect();
    // The creases inside what is left, where the offsets land a grid step short at best.
    let g = map.gx.max(map.gy);
    let near = flat_distance(map, &|k| domain.get(k).copied().unwrap_or(false), 2.0 * g);
    let mut creases = Vec::new();
    for run in plan.cleanup()? {
        for pts in clip(run.pts, &|q| g - map.sample(&near, q)) {
            creases.push(Run { pts, level: floor });
        }
    }
    groups.push(creases);
    plan.cut(groups)
}
