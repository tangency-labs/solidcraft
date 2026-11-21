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

/// How near one of a face's loops a point outside its loops' even–odd test may be and still be
/// on the face, for a layout fitted to the edges (mm): the records' tolerance.
const ON_LOOP: f64 = 1e-6;

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

/// How far in from a face's edges a layout's outer stations stand, along the surface (mm): `u` from
/// the edges its first parameter runs between, `v` from the second's. A pattern sets it to its
/// copies' half-width each way and its margin, so its outer copies lie flush with the edges, or
/// its margin in from them. An end of a parameter's range where the face closes to a point (a
/// sphere's pole) is no edge: there the layout keeps half a step clear instead; a parameter the
/// face turns round in has no edges at all.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    /// In from the edges the first parameter runs between.
    pub u: f64,
    /// In from the edges the second parameter runs between.
    pub v: f64,
}

impl Fit {
    fn check(&self) -> Result<()> {
        if self.u.is_finite() && self.v.is_finite() && self.u >= 0.0 && self.v >= 0.0 {
            Ok(())
        } else {
            Err(invalid("a fit to the edges must be a number, not negative"))
        }
    }

    /// The fit along parameter `k`.
    fn along(&self, k: usize) -> f64 {
        if k == 0 {
            self.u
        } else {
            self.v
        }
    }
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

/// Most faces one pattern spans.
pub const MAX_FACES: usize = 256;

/// One face of a record read for stations: its surface, extent in its parameters and whether its
/// shell turns its normal over.
struct Face<'q> {
    query: &'q Query,
    index: usize,
    surface: &'q SurfaceEval,
    extent: [[f64; 2]; 2],
    flip: bool,
}

/// The faces a pattern spans, each read for stations, and the boundary of the whole set: the edges
/// one of its faces' loops uses and no other does, so not the seams of a face nor the edges two of
/// its faces share.
struct Region<'q> {
    faces: Vec<Face<'q>>,
    boundary: Boundary,
}

impl<'q> Region<'q> {
    fn new(query: &'q Query, faces: &[usize]) -> Result<Region<'q>> {
        if faces.is_empty() || faces.len() > MAX_FACES {
            return Err(invalid("a pattern spans 1 to 256 faces"));
        }
        for (i, f) in faces.iter().enumerate() {
            if faces.iter().skip(i + 1).any(|g| g == f) {
                return Err(invalid("a face is listed twice"));
            }
        }
        Ok(Region {
            faces: faces
                .iter()
                .map(|&f| Face::new(query, f))
                .collect::<Result<Vec<_>>>()?,
            boundary: Boundary::of(query, faces),
        })
    }

    /// The station at `uv` on the region's face `face`, its margin measured to the region's
    /// boundary ([`Face::station`]).
    fn station(&self, face: &Face, uv: [f64; 2], margin: f64) -> Option<Station> {
        face.station(uv, margin, &self.boundary, false)
    }

    /// The station at `uv` as [`Region::station`], a point on one of the face's loops on it too:
    /// for a layout fitted to the edges, whose outer stations may lie on them.
    fn fitted_station(&self, face: &Face, uv: [f64; 2], margin: f64) -> Option<Station> {
        face.station(uv, margin, &self.boundary, true)
    }
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
        })
    }

    /// The parameters at `(x, y)` of the extent's unit square.
    fn at(&self, x: f64, y: f64) -> [f64; 2] {
        let [[u0, u1], [v0, v1]] = self.extent;
        [u0 + x * (u1 - u0), v0 + y * (v1 - v0)]
    }

    /// The station at `uv`, when `uv` is inside the face, the surface has a normal there (not at a
    /// pole) and the point is at least `margin` from `boundary`.
    fn station(
        &self,
        uv: [f64; 2],
        margin: f64,
        boundary: &Boundary,
        on_loops: bool,
    ) -> Option<Station> {
        // With `on_loops`, a point on one of the face's loops is on the face too, though the
        // even–odd test of its parameters can't tell: a layout fitted to the edges stands
        // stations on them.
        if self.query.holds(self.index, uv) != Some(true) {
            if !on_loops {
                return None;
            }
            let point = self.surface.point(uv)?;
            if !self.query.face_contains(self.index, point, ON_LOOP) {
                return None;
            }
        }
        self.station_at(uv, margin, boundary)
    }

    /// The station at `uv` as [`Face::station`], taking `uv` to be inside the face.
    fn station_at(&self, uv: [f64; 2], margin: f64, boundary: &Boundary) -> Option<Station> {
        let (point, su, sv) = self.surface.derivs(uv[0], uv[1])?;
        let normal = su.cross(sv).normalized()?;
        let along = (su - normal * su.dot(normal)).normalized()?;
        let to_edge = boundary.distance(self.query, point);
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
        Boundary {
            edges: boundary_edges(query, faces)
                .into_iter()
                .filter_map(|e| Some((e, search_points(query, e)?)))
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

/// The edges the loops of `faces` use once between them, in the order of their indices: the
/// set's boundary, without the seams a periodic face's loops run along twice or the edges two of
/// the faces share.
fn boundary_edges(query: &Query, faces: &[usize]) -> Vec<usize> {
    let mut uses: BTreeMap<usize, usize> = BTreeMap::new();
    for face in faces.iter().filter_map(|&f| query.record().faces.get(f)) {
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
    grid_stations_across(query, &[face], grid, None, margin)
}

/// The stations a grid puts on each of `faces` ([`grid_stations`]), the margin measured to the
/// boundary of the set: the edges two of the faces share are not edges of it. With `fit`, each
/// way the grid runs from `fit`'s distance in from one edge of the face's extent to the same
/// distance in from the other ([`Fit`]), its outer stations on those lines; without it, each
/// station is at the centre of its cell, as [`grid_stations`]'s.
///
/// # Errors
/// As [`grid_stations`]'s, and for a fit that is not a number or is negative, no face, more than
/// [`MAX_FACES`] or a face listed twice.
pub fn grid_stations_across(
    query: &Query,
    faces: &[usize],
    grid: &Grid,
    fit: Option<&Fit>,
    margin: f64,
) -> Result<Vec<Station>> {
    let counts = 1..=MAX_STATION_COUNT;
    if !(counts.contains(&grid.count_u) && counts.contains(&grid.count_v)) {
        return Err(invalid("a grid has 1 to 1000 cells each way"));
    }
    if !(grid.angle.is_finite() && margin.is_finite() && margin >= 0.0) {
        return Err(invalid(
            "the angle and the margin must be numbers, the margin not negative",
        ));
    }
    if let Some(fit) = fit {
        fit.check()?;
    }
    let region = Region::new(query, faces)?;
    // Stations in the extent's unit square, turned about the middle of the part of it the grid
    // spans. The turned lattice runs a whole grid past each side, so it still fills the square at
    // any angle.
    let (s, c) = grid.angle.sin_cos();
    let reach = |n: usize| -(n as i64)..2 * n as i64;
    let mut out = Vec::new();
    for f in &region.faces {
        let axes = match fit {
            None => Some([
                layouts::Axis::cells(grid.count_u),
                layouts::Axis::cells(grid.count_v),
            ]),
            Some(fit) => layouts::fitted_axis(f, 0, grid.count_u, fit.u)
                .zip(layouts::fitted_axis(f, 1, grid.count_v, fit.v))
                .map(|(u, v)| [u, v]),
        };
        // A face too small for one station `fit` in from its edges stands none.
        let Some([au, av]) = axes else { continue };
        for i in reach(grid.count_u) {
            let shift = if grid.staggered && i & 1 == 1 {
                0.5
            } else {
                0.0
            };
            for j in reach(grid.count_v) {
                let (Some(x0), Some(y0)) = (au.at(i as f64), av.at(j as f64 + shift)) else {
                    continue;
                };
                let (a, b) = (x0 - au.middle(), y0 - av.middle());
                let (x, y) = (au.middle() + c * a - s * b, av.middle() + s * a + c * b);
                if !(au.holds(x) && av.holds(y)) {
                    continue;
                }
                let at = f.at(x, y);
                let station = match fit {
                    Some(_) => region.fitted_station(f, at, margin),
                    None => region.station(f, at, margin),
                };
                if let Some(station) = station {
                    out.push(station);
                }
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
    random_stations_across(query, &[face], random, margin, keep)
}

/// `random.count` stations placed at random over `faces` together ([`random_stations`]): even over
/// their joint area, the margin measured to the boundary of the set. One face draws exactly as
/// [`random_stations`] does.
///
/// # Errors
/// As [`random_stations`]'s, and for no face, more than [`MAX_FACES`] or a face listed twice.
pub fn random_stations_across(
    query: &Query,
    faces: &[usize],
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
    let region = Region::new(query, faces)?;
    let grid = |i: usize| i as f64 / FIELD_GRID as f64;
    // Each face's largest area element bounds the rejection that makes places even over its
    // area; a face is drawn in proportion to that bound times its extent, so places are even
    // over the faces' joint area.
    let mut most = Vec::new();
    for f in &region.faces {
        let mut m: f64 = 0.0;
        for i in 0..=FIELD_GRID {
            for j in 0..=FIELD_GRID {
                if let Some(a) = f.area(f.at(grid(i), grid(j))) {
                    m = m.max(a);
                }
            }
        }
        let [[u0, u1], [v0, v1]] = f.extent;
        let mass = m * (u1 - u0) * (v1 - v0);
        most.push((
            m,
            if m.is_finite() && mass.is_finite() {
                mass
            } else {
                0.0
            },
        ));
    }
    let total: f64 = most.iter().map(|(_, mass)| mass).sum();
    if !(total > 0.0 && total.is_finite()) {
        return Err(invalid("the face has no area"));
    }
    let mut rng = Rng::new(random.seed ^ SEED_MIX);
    let field = Field::new(random.noise, &mut rng);
    let candidate = |rng: &mut Rng| -> Option<Station> {
        for _ in 0..RANDOM_TRIES {
            // One face draws no face: its places are drawn as they were on one face.
            let k = if region.faces.len() == 1 {
                0
            } else {
                let mut left = rng.next_f64() * total;
                most.iter()
                    .position(|(_, mass)| {
                        left -= mass;
                        left < 0.0
                    })
                    .unwrap_or_else(|| most.len().saturating_sub(1))
            };
            let (Some(f), Some(&(m, _))) = (region.faces.get(k), most.get(k)) else {
                continue;
            };
            let (x, y) = (rng.next_f64(), rng.next_f64());
            let uv = f.at(x, y);
            if f.query.holds(f.index, uv) != Some(true)
                || rng.next_f64() * m > f.area(uv).unwrap_or(0.0)
                || rng.next_f64() > field.density(x, y)
            {
                continue;
            }
            if let Some(s) = region.station(f, uv, margin) {
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
    edge_points_across(query, &[face], spacing)
}

/// Points along each edge of the boundary of `faces` together ([`edge_points`]): not the seams of
/// a face nor the edges two of them share.
///
/// # Errors
/// As [`edge_points`]'s, and for no face, more than [`MAX_FACES`] or a face listed twice.
pub fn edge_points_across(query: &Query, faces: &[usize], spacing: f64) -> Result<Vec<Vec<Vec3>>> {
    Ok(edge_samples(query, faces, spacing)?
        .into_iter()
        .map(|(_, points)| points.into_iter().map(|(p, _)| p).collect())
        .collect())
}

/// A point on an edge's curve and the curve's derivative there.
type PointAndSlope = (Vec3, Vec3);

/// Each boundary edge of `faces` together ([`edge_points_across`]), with its points along it and
/// the edge's derivative at each: the parameter halved until no two neighbours are more than
/// `spacing` apart, or [`MAX_EDGE_POINTS`] reached.
fn edge_samples(
    query: &Query,
    faces: &[usize],
    spacing: f64,
) -> Result<Vec<(usize, Vec<PointAndSlope>)>> {
    if !(spacing.is_finite() && spacing > 0.0) {
        return Err(invalid("the spacing must be above zero"));
    }
    if faces.is_empty() || faces.len() > MAX_FACES {
        return Err(invalid("a pattern spans 1 to 256 faces"));
    }
    if faces.iter().any(|&f| query.record().faces.get(f).is_none()) {
        return Err(invalid("the record has no such face"));
    }
    let cannot = || invalid("an edge's curve can't be evaluated");
    let mut out = Vec::new();
    for e in boundary_edges(query, faces) {
        let [t0, t1] = query.edge_interval(e).ok_or_else(cannot)?;
        let mut n = 16usize;
        loop {
            let points = (0..=n)
                .map(|i| query.edge_at(e, t0 + (t1 - t0) * i as f64 / n as f64))
                .collect::<Option<Vec<(Vec3, Vec3)>>>()
                .ok_or_else(cannot)?;
            let widest = points
                .iter()
                .zip(points.iter().skip(1))
                .map(|(a, b)| a.0.dist(b.0))
                .fold(0.0, f64::max);
            if widest <= spacing || n >= MAX_EDGE_POINTS {
                out.push((e, points));
                break;
            }
            n = (n * 2).min(MAX_EDGE_POINTS);
        }
    }
    Ok(out)
}

/// A point along the boundary of a set of faces, with which way is off them there
/// ([`edge_frames_across`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeFrame {
    /// The point, on the edge's curve.
    pub point: Vec3,
    /// The unit direction off the faces across the edge: along the face's surface, square to the
    /// edge.
    pub out: Vec3,
    /// The face's unit normal there, out of the material ([`Query::face_normal`]).
    pub normal: Vec3,
}

/// Points along each edge of the boundary of `faces` together, as [`edge_points_across`], each
/// with the direction off the faces across the edge and the face's normal there: for telling
/// whether something standing on the faces reaches past their edges. The direction is the edge's
/// tangent crossed with the face's normal, turned so that a step back along it from the edge's
/// middle lands inside the face. A point where the face's normal can't be found takes the frame
/// of the point before it (at the start of an edge, with none before it, it is left out).
///
/// # Errors
/// As [`edge_points_across`]'s, and for an edge whose face's surface or loops can't be read there.
pub fn edge_frames_across(
    query: &Query,
    faces: &[usize],
    spacing: f64,
) -> Result<Vec<Vec<EdgeFrame>>> {
    let cannot = || invalid("an edge's face can't be read along it");
    let mut out = Vec::new();
    for (e, points) in edge_samples(query, faces, spacing)? {
        // The face of the set the edge bounds (it bounds only one of them: it is on the boundary).
        let face = faces
            .iter()
            .copied()
            .find(|&f| boundary_edges(query, &[f]).contains(&e))
            .ok_or_else(cannot)?;
        let length: f64 = points
            .windows(2)
            .map(|w| match w {
                [a, b] => a.0.dist(b.0),
                _ => 0.0,
            })
            .sum();
        // The face's normal at each point, from its parameters there.
        let accept = spacing.max(length * EDGE_SAG_SHARE);
        let frames: Vec<Option<(Vec3, Vec3)>> = points
            .iter()
            .map(|&(p, d)| {
                let uv = query.face_uv(face, p, accept)?;
                let n = query.face_normal(face, uv)?;
                Some((d.cross(n).normalized()?, n))
            })
            .collect();
        // Which way is off the face: a step back from the middle's lands inside it.
        let middle = frames.len() / 2;
        let (Some(&(p, _)), Some(&Some((o, _)))) = (points.get(middle), frames.get(middle)) else {
            return Err(cannot());
        };
        let step = (length * EDGE_SAG_SHARE).max(1e-9);
        let back = p - o * step;
        let inward = query
            .face_uv(face, back, 2.0 * step)
            .is_some_and(|uv| query.holds(face, uv) == Some(true));
        let sign = if inward { 1.0 } else { -1.0 };
        let mut last = None;
        let mut line = Vec::new();
        for (&(p, _), frame) in points.iter().zip(&frames) {
            let Some((o, n)) = frame.or(last) else {
                continue;
            };
            last = Some((o, n));
            line.push(EdgeFrame {
                point: p,
                out: o * sign,
                normal: n,
            });
        }
        out.push(line);
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

#[path = "face_pattern_layouts.rs"]
mod layouts;
pub use layouts::{
    fill_stations, geodesic_stations, spaced_stations, spiral_stations, Spacing,
    MAX_GEODESIC_FREQUENCY,
};

#[cfg(test)]
#[path = "face_pattern_tests.rs"]
mod tests;
