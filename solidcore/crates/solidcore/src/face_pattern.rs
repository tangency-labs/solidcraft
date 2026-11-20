//! Pattern on Face: where copies of bodies stand on a face of a B-rep record and how each is
//! placed there, on the record's exact geometry.
//!
//! Not upstream. A face pattern puts a copy at each of its stations: a point of the face with the
//! face's unit normal out of the material and a direction along it. [`grid_stations`] lays a grid
//! of cells across the face's extent in its surface parameters (u, v), one station at each cell's
//! centre, turned and staggered as asked; [`random_stations`] draws them at random, evenly over
//! the face's area and spread as a [`Noise`] says. Each keeps the stations inside the face's loops
//! ([`Query`]'s exact even–odd test on its pcurves), farther than a margin from its boundary (its
//! edges, not its seams, measured on their curves). [`placement`] is the matrix that stands a copy
//! on a station; [`attraction`] and [`ramp`] give how much an attractor changes a copy by its
//! distance (the kernel's closest-point solve measures it); [`edge_points`] gives points along a
//! face's edges for clearances measured from them.
//!
//! A station's normal is the surface's natural normal, turned where the face's shell uses the face
//! against it (as [`Query::face_normal`]): out of a solid's material, and the shell's own side for
//! a surface body.
//!
//! Every function checks its input and returns [`SolidError::InvalidInput`] for a number that is
//! not finite or out of range, or a face the record lacks; nothing here panics on input.
//!
//! Deviations: none (no upstream counterpart). A random placement draws from its own [`Rng`],
//! seeded by the caller, not from a kernel's: the same seed gives the same places in any session,
//! as a saved design needs.

// A comparison that is not true takes the refusing branch, so a NaN is refused too.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::collections::BTreeMap;

use crate::eval::query::{Loop, Query};
use crate::eval::vec::VecExt as _;
use crate::eval::SurfaceEval;
use crate::math::Vec3;
use crate::surface::procedural_sweep::path::rodrigues;
use crate::{Result, Rng, SolidError};

const OP: &str = "face pattern";

/// Most grid cells [`grid_stations`] lays across a face each way, and most stations
/// [`random_stations`] places.
pub const MAX_STATION_COUNT: usize = 1000;

/// Most points [`edge_points`] puts along one edge.
pub const MAX_EDGE_POINTS: usize = 1 << 16;

/// Tries per station a random placement makes before it stops short.
const RANDOM_TRIES: usize = 400;

/// Candidates per station blue noise takes the farthest of.
const BLUE_CANDIDATES: usize = 12;

/// Points each way across a face's extent where a random placement reads its area element and
/// its density field for their largest and smallest values.
const FIELD_GRID: usize = 48;

/// Mixed into a random placement's seed, so seed 0 is not `SplitMix64`'s own start.
const SEED_MIX: u64 = 0x005E_ED0F_FACE;

/// The sag, as a share of an edge's length, of the points an edge's distance is searched from.
const EDGE_SAG_SHARE: f64 = 1e-3;

/// A point on a face where a pattern puts a copy, with the face's frame there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Station {
    /// The point, on the face's surface.
    pub point: Vec3,
    /// The face's unit normal there, out of the material.
    pub normal: Vec3,
    /// A unit direction along the face, square to `normal`: the way its first parameter runs.
    pub along: Vec3,
    /// How far the point is from the face's boundary: its edges, not its seams.
    pub margin: f64,
}

/// A grid of stations across a face ([`grid_stations`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    /// Cells along the face's first parameter (round a cylinder), 1 to [`MAX_STATION_COUNT`].
    pub count_u: usize,
    /// Cells along its second parameter (along a cylinder), 1 to [`MAX_STATION_COUNT`].
    pub count_v: usize,
    /// How far the grid is turned about the extent's centre (radians), measured in the extent's
    /// own unit square.
    pub angle: f64,
    /// Every other column moved half a cell along.
    pub staggered: bool,
}

/// How a random placement spreads its stations: the colour of the noise that places them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Noise {
    /// Independent places, equally likely on every part of the face's area.
    White,
    /// Random but evenly spread: each station as far from the others as a few candidates allow
    /// (best-candidate sampling), so no clumps and no gaps.
    Blue,
    /// Loose clusters: the density follows a random field whose waves weaken as 1/f.
    Pink,
    /// Large clusters and bare patches: the density follows a field whose waves weaken as 1/f².
    Brown,
}

/// A random placement on a face ([`random_stations`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Random {
    /// How many stations, at most [`MAX_STATION_COUNT`].
    pub count: usize,
    /// The seed: the same seed gives the same stations.
    pub seed: u64,
    /// How they spread.
    pub noise: Noise,
}

/// How an attractor's effect runs from its near value to its far value ([`attraction`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Falloff {
    /// In proportion to the distance.
    Linear,
    /// Smoothstep: easing out of the near value and into the far one.
    Smooth,
}

fn invalid(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: OP,
        reason,
    }
}

/// One face of a record read for stations: its surface, extent in its parameters, whether its
/// shell turns its normal over, and its boundary edges with points along them.
struct Face<'q> {
    query: &'q Query,
    index: usize,
    surface: &'q SurfaceEval,
    extent: [[f64; 2]; 2],
    flip: bool,
    boundary: Boundary,
}

impl<'q> Face<'q> {
    fn new(query: &'q Query, index: usize) -> Result<Face<'q>> {
        let face = query
            .record()
            .faces
            .get(index)
            .ok_or_else(|| invalid("the record has no such face"))?;
        let surface = query
            .surface(index)
            .ok_or_else(|| invalid("the face's surface can't be evaluated"))?;
        let extent = query
            .trim_extent(index)
            .ok_or_else(|| invalid("the face's loops can't be read"))?;
        let [[u0, u1], [v0, v1]] = extent;
        if !(u1 > u0 && v1 > v0) {
            return Err(invalid("the face has no extent"));
        }
        Ok(Face {
            query,
            index,
            surface,
            extent,
            flip: face.forward == Some(false),
            boundary: Boundary::of(query, &[index]),
        })
    }

    /// The parameters at `(x, y)` of the extent's unit square.
    fn at(&self, x: f64, y: f64) -> [f64; 2] {
        let [[u0, u1], [v0, v1]] = self.extent;
        [u0 + x * (u1 - u0), v0 + y * (v1 - v0)]
    }

    /// The station at `uv`, when `uv` is inside the face, the surface has a normal there (not at a
    /// pole) and the point is at least `margin` from the face's boundary.
    fn station(&self, uv: [f64; 2], margin: f64) -> Option<Station> {
        if self.query.holds(self.index, uv) != Some(true) {
            return None;
        }
        let (point, su, sv) = self.surface.derivs(uv[0], uv[1])?;
        let normal = su.cross(sv).normalized()?;
        let along = (su - normal * su.dot(normal)).normalized()?;
        let to_edge = self.boundary.distance(self.query, point);
        if to_edge < margin {
            return None;
        }
        let normal = if self.flip { -normal } else { normal };
        Some(Station {
            point,
            normal,
            along,
            margin: to_edge,
        })
    }

    /// The area element `|S_u × S_v|` at `uv`.
    fn area(&self, uv: [f64; 2]) -> Option<f64> {
        self.surface
            .derivs(uv[0], uv[1])
            .map(|(_, su, sv)| su.cross(sv).len())
    }
}

/// The edges bounding some faces (each face's edges its loops use once, so not its seams), each
/// with points along it that its distance is searched from.
struct Boundary {
    edges: Vec<(usize, Vec<(f64, Vec3)>)>,
}

impl Boundary {
    fn of(query: &Query, faces: &[usize]) -> Boundary {
        let mut edges = BTreeMap::new();
        for &f in faces {
            for e in boundary_edges(query, f) {
                edges.entry(e).or_insert_with(|| search_points(query, e));
            }
        }
        Boundary {
            edges: edges
                .into_iter()
                .filter_map(|(e, points)| Some((e, points?)))
                .collect(),
        }
    }

    /// How far `p` is from the nearest edge (infinite with none).
    fn distance(&self, query: &Query, p: Vec3) -> f64 {
        self.edges
            .iter()
            .filter_map(|(e, points)| query.edge_distance_from(*e, p, points))
            .fold(f64::INFINITY, f64::min)
    }
}

/// The edges face `f`'s loops use once, in the order of their indices: its boundary, without the
/// seams a periodic face's loops run along twice.
fn boundary_edges(query: &Query, f: usize) -> Vec<usize> {
    let mut uses: BTreeMap<usize, usize> = BTreeMap::new();
    if let Some(face) = query.record().faces.get(f) {
        for l in &face.loops {
            if let Loop::Edges(edges) = l {
                for u in edges {
                    *uses.entry(u.edge).or_insert(0) += 1;
                }
            }
        }
    }
    uses.into_iter()
        .filter(|(_, n)| *n == 1)
        .map(|(e, _)| e)
        .collect()
}

/// Points along edge `e` within [`EDGE_SAG_SHARE`] of its length of its curve, for
/// [`Query::edge_distance_from`].
fn search_points(query: &Query, e: usize) -> Option<Vec<(f64, Vec3)>> {
    let coarse = query.edge_points(e, f64::INFINITY)?;
    let length: f64 = coarse
        .iter()
        .zip(coarse.iter().skip(1))
        .map(|((_, a), (_, b))| a.dist(*b))
        .sum();
    query.edge_points(e, (length * EDGE_SAG_SHARE).max(1e-9))
}

/// The stations a grid puts on face `face` of `query`'s record, one at each cell's centre inside
/// the face and at least `margin` from its boundary. The grid covers the face's extent in its
/// surface parameters, so on a cylinder `count_u` runs round it and `count_v` along it.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a count out of range, an angle or a margin that is not a
/// number or a negative margin, a face the record lacks, or one whose surface or loops can't be
/// read or that has no extent.
pub fn grid_stations(query: &Query, face: usize, grid: &Grid, margin: f64) -> Result<Vec<Station>> {
    let counts = 1..=MAX_STATION_COUNT;
    if !(counts.contains(&grid.count_u) && counts.contains(&grid.count_v)) {
        return Err(invalid("a grid has 1 to 1000 cells each way"));
    }
    if !(grid.angle.is_finite() && margin.is_finite() && margin >= 0.0) {
        return Err(invalid(
            "the angle and the margin must be numbers, the margin not negative",
        ));
    }
    let f = Face::new(query, face)?;
    // Cell centres in the extent's unit square, turned about its centre. The turned lattice runs
    // a whole extent past each side, so it still fills the square at any angle.
    let (s, c) = grid.angle.sin_cos();
    let (nu, nv) = (grid.count_u as f64, grid.count_v as f64);
    let reach = |n: usize| -(n as i64)..2 * n as i64;
    let mut out = Vec::new();
    for i in reach(grid.count_u) {
        let shift = if grid.staggered && i & 1 == 1 {
            0.5
        } else {
            0.0
        };
        for j in reach(grid.count_v) {
            let a = (i as f64 + 0.5) / nu - 0.5;
            let b = (j as f64 + 0.5 + shift) / nv - 0.5;
            let (x, y) = (0.5 + c * a - s * b, 0.5 + s * a + c * b);
            if !((0.0..1.0).contains(&x) && (0.0..1.0).contains(&y)) {
                continue;
            }
            if let Some(station) = f.station(f.at(x, y), margin) {
                out.push(station);
            }
        }
    }
    Ok(out)
}

/// `random.count` stations placed at random on face `face` of `query`'s record, the same for the
/// same seed, spread as `random.noise` says, each inside the face, at least `margin` from its
/// boundary and one `keep` takes (it may say no to a station where a copy would not fit). Places
/// are even over the face's area, not its parameters. Fewer come back when the face has no room
/// for more within the tries allowed.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a count past [`MAX_STATION_COUNT`], a margin that is not a
/// number or is negative, a face the record lacks, or one whose surface or loops can't be read or
/// that has no extent or no area.
pub fn random_stations(
    query: &Query,
    face: usize,
    random: &Random,
    margin: f64,
    keep: &mut dyn FnMut(&Station) -> bool,
) -> Result<Vec<Station>> {
    if random.count > MAX_STATION_COUNT {
        return Err(invalid("at most 1000 random stations"));
    }
    if !(margin.is_finite() && margin >= 0.0) {
        return Err(invalid("the margin must not be negative"));
    }
    let f = Face::new(query, face)?;
    let grid = |i: usize| i as f64 / FIELD_GRID as f64;
    // The largest area element bounds the rejection that makes places even over the area.
    let mut most: f64 = 0.0;
    for i in 0..=FIELD_GRID {
        for j in 0..=FIELD_GRID {
            if let Some(a) = f.area(f.at(grid(i), grid(j))) {
                most = most.max(a);
            }
        }
    }
    if !(most > 0.0 && most.is_finite()) {
        return Err(invalid("the face has no area"));
    }
    let mut rng = Rng::new(random.seed ^ SEED_MIX);
    let field = Field::new(random.noise, &mut rng);
    let candidate = |rng: &mut Rng| -> Option<Station> {
        for _ in 0..RANDOM_TRIES {
            let (x, y) = (rng.next_f64(), rng.next_f64());
            let uv = f.at(x, y);
            if f.query.holds(f.index, uv) != Some(true)
                || rng.next_f64() * most > f.area(uv).unwrap_or(0.0)
                || rng.next_f64() > field.density(x, y)
            {
                continue;
            }
            if let Some(s) = f.station(uv, margin) {
                return Some(s);
            }
        }
        None
    };
    let mut out: Vec<Station> = Vec::new();
    // Rounds of drawing in a row in which `keep` took nothing.
    let mut refused = 0;
    while out.len() < random.count && refused < RANDOM_TRIES {
        let picked = if random.noise == Noise::Blue && !out.is_empty() {
            // The candidate farthest from the stations already placed that `keep` takes: asked
            // farthest first, so it is asked about as few as it can be.
            let mut drawn = Vec::new();
            for _ in 0..BLUE_CANDIDATES {
                let Some(c) = candidate(&mut rng) else { break };
                let gap = out
                    .iter()
                    .map(|s| s.point.dist(c.point))
                    .fold(f64::INFINITY, f64::min);
                drawn.push((gap, c));
            }
            if drawn.is_empty() {
                break;
            }
            drawn.sort_by(|a, b| b.0.total_cmp(&a.0));
            drawn.into_iter().map(|(_, c)| c).find(|c| keep(c))
        } else {
            let Some(c) = candidate(&mut rng) else { break };
            keep(&c).then_some(c)
        };
        match picked {
            Some(s) => {
                out.push(s);
                refused = 0;
            }
            None => refused += 1,
        }
    }
    Ok(out)
}

/// A random placement's density over the extent's unit square: even for white and blue noise; for
/// pink and brown, a random field of waves whose strength falls off as 1/f or 1/f², normalised to
/// 0..1 and squared.
struct Field {
    /// Each wave's frequencies along x and y, its strength and its phase.
    waves: Vec<(f64, f64, f64, f64)>,
    lo: f64,
    hi: f64,
}

impl Field {
    fn new(noise: Noise, rng: &mut Rng) -> Field {
        let power = match noise {
            Noise::White | Noise::Blue => {
                return Field {
                    waves: Vec::new(),
                    lo: 0.0,
                    hi: 0.0,
                }
            }
            Noise::Pink => 1.0,
            Noise::Brown => 2.0,
        };
        let mut waves = Vec::new();
        for kx in -6i32..=6 {
            for ky in 0i32..=6 {
                if ky == 0 && kx <= 0 {
                    continue;
                }
                let f = f64::from(kx * kx + ky * ky).sqrt();
                waves.push((
                    f64::from(kx),
                    f64::from(ky),
                    f.powf(-power),
                    rng.next_f64() * std::f64::consts::TAU,
                ));
            }
        }
        let mut field = Field {
            waves,
            lo: f64::INFINITY,
            hi: f64::NEG_INFINITY,
        };
        let grid = |i: usize| i as f64 / FIELD_GRID as f64;
        for i in 0..=FIELD_GRID {
            for j in 0..=FIELD_GRID {
                let v = field.value(grid(i), grid(j));
                field.lo = field.lo.min(v);
                field.hi = field.hi.max(v);
            }
        }
        field
    }

    fn value(&self, x: f64, y: f64) -> f64 {
        self.waves
            .iter()
            .map(|(kx, ky, a, phase)| a * (std::f64::consts::TAU * (kx * x + ky * y) + phase).cos())
            .sum()
    }

    /// The share, 0 to 1, of the places drawn at `(x, y)` that are kept.
    fn density(&self, x: f64, y: f64) -> f64 {
        if self.waves.is_empty() || !(self.hi > self.lo) {
            return 1.0;
        }
        let t = ((self.value(x, y) - self.lo) / (self.hi - self.lo)).clamp(0.0, 1.0);
        t * t
    }
}

/// Points along each edge of face `face` of `query`'s record (not its seams), one list for each
/// edge in the order of their indices, from its start to its end, on the edge's curve: the
/// parameter halved until no two neighbours are more than `spacing` apart, or
/// [`MAX_EDGE_POINTS`] reached.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a spacing that is not above zero, a face the record lacks, or
/// an edge whose curve can't be evaluated.
pub fn edge_points(query: &Query, face: usize, spacing: f64) -> Result<Vec<Vec<Vec3>>> {
    if !(spacing.is_finite() && spacing > 0.0) {
        return Err(invalid("the spacing must be above zero"));
    }
    if query.record().faces.get(face).is_none() {
        return Err(invalid("the record has no such face"));
    }
    let cannot = || invalid("an edge's curve can't be evaluated");
    let mut out = Vec::new();
    for e in boundary_edges(query, face) {
        let [t0, t1] = query.edge_interval(e).ok_or_else(cannot)?;
        let mut n = 16usize;
        loop {
            let points = (0..=n)
                .map(|i| {
                    query
                        .edge_at(e, t0 + (t1 - t0) * i as f64 / n as f64)
                        .map(|(p, _)| p)
                })
                .collect::<Option<Vec<Vec3>>>()
                .ok_or_else(cannot)?;
            let widest = points
                .iter()
                .zip(points.iter().skip(1))
                .map(|(a, b)| a.dist(*b))
                .fold(0.0, f64::max);
            if widest <= spacing || n >= MAX_EDGE_POINTS {
                out.push(points);
                break;
            }
            n = (n * 2).min(MAX_EDGE_POINTS);
        }
    }
    Ok(out)
}

/// The placement (a column-major 4 × 4 matrix: the images of x, y and z, then the translation)
/// that stands a copy on `station`: the copy's point `anchor` on the station, its +z along the
/// normal (against it with `flip`) and moved out along it by `offset`, its +x along the station's
/// `along` turned by `spin` (radians) about that axis, and everything scaled by `scale`.
///
/// # Errors
/// [`SolidError::InvalidInput`] for an input that is not finite, a scale not above zero, or a
/// station whose normal and direction are not unit and square to each other.
pub fn placement(
    station: &Station,
    anchor: Vec3,
    flip: bool,
    scale: f64,
    offset: f64,
    spin: f64,
) -> Result<[[f64; 4]; 4]> {
    if !(anchor.is_finite() && scale.is_finite() && offset.is_finite() && spin.is_finite()) {
        return Err(invalid(
            "the anchor, scale, offset and spin must be numbers",
        ));
    }
    if !(scale > 0.0) {
        return Err(invalid("the scale must be above zero"));
    }
    let (n, a) = (station.normal, station.along);
    if !(station.point.is_finite()
        && (n.len() - 1.0).abs() <= 1e-9
        && (a.len() - 1.0).abs() <= 1e-9
        && n.dot(a).abs() <= 1e-9)
    {
        return Err(invalid(
            "a station's normal and direction must be unit and square",
        ));
    }
    let z = if flip { -n } else { n };
    let x = rodrigues(a, z, spin);
    let y = z.cross(x);
    let origin = station.point + z * offset;
    let (sx, sy, sz) = (x * scale, y * scale, z * scale);
    let t = origin - (sx * anchor.x + sy * anchor.y + sz * anchor.z);
    Ok([
        [sx.x, sx.y, sx.z, 0.0],
        [sy.x, sy.y, sy.z, 0.0],
        [sz.x, sz.y, sz.z, 0.0],
        [t.x, t.y, t.z, 1.0],
    ])
}

/// How far along an attractor's effect a copy is, 0 (its near value) to 1 (its far value), by
/// `distance` to the nearest near attractor.
///
/// With far attractors (`far_distance`, the distance to the nearest of them), the copy is between
/// two sets: `near` and `far` are hold zones round each set, within which a copy takes that set's
/// value in full, and between them the share is the part of the way from one zone to the other.
/// With none, the effect runs from `near` to `far` in distance (equal: no change).
/// `falloff` shapes the share.
#[must_use]
pub fn attraction(
    distance: f64,
    far_distance: Option<f64>,
    near: f64,
    far: f64,
    falloff: Falloff,
) -> f64 {
    let t = match far_distance {
        Some(df) => {
            let x = (distance - near).max(0.0);
            let y = (df - far).max(0.0);
            if x <= 0.0 {
                0.0
            } else if y <= 0.0 {
                1.0
            } else {
                (x / (x + y)).clamp(0.0, 1.0)
            }
        }
        None => {
            if (far - near).abs() < 1e-12 {
                0.0
            } else {
                ((distance - near) / (far - near)).clamp(0.0, 1.0)
            }
        }
    };
    match falloff {
        Falloff::Linear => t,
        Falloff::Smooth => t * t * (3.0 - 2.0 * t),
    }
}

/// The value `t` of the way from `at_near` to `at_far` ([`attraction`]'s share).
#[must_use]
pub fn ramp(at_near: f64, at_far: f64, t: f64) -> f64 {
    at_near + (at_far - at_near) * t
}

#[cfg(test)]
#[path = "face_pattern_tests.rs"]
mod tests;
