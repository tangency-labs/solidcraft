//! Rebuilding a scanned mesh as a stack of slabs: what a 2.5D part was modelled from.
//!
//! [`recognize_mesh`](crate::recognize_mesh) certifies analytic surfaces and refuses whatever it
//! cannot certify. That is the right answer for a mesh that came from exact geometry and the wrong
//! one for a scan: noise puts every facet a few tenths of a millimetre off its own plane, so a
//! scanned plate recognizes as thousands of tiny patches and no plan at all.
//!
//! This module takes the other road. It never asks which surface a triangle lies on. It measures
//! the part the way someone with a caliper would: find the face it sits flattest on, read the
//! heights it steps between, cut a section through each step, fit that outline with lines, arcs
//! and circles, then tidy what was measured — square up the near-square edges, make the edges that
//! should line up line up, average the features either side of a mirror plane, and round what is
//! left to a grid. That last stage is what separates a rebuild from a tracing: a scan of a 60 mm
//! plate measures 59.94, and a part someone can work with says 60.
//!
//! What comes out is a plan, not a body: a sketch plane, the levels the part steps between, and a
//! closed outline per level built from lines, arcs and circles. The caller builds it with the
//! ordinary sketch and extrude commands, so a rebuilt scan is a timeline to edit, exactly as
//! [`prismatic_plan`](crate::prismatic_plan)'s rebuild is.
//!
//! Every number here is a measurement, and the module says so rather than implying otherwise: each
//! loop reports the deviation it was fitted within, a loop that would not fit at all is kept as a
//! polygon and marked as one, and the finished stack reports how far it sits from the scan it came
//! from. A scan has no exact answer to give. The honest thing is to say how close this one is.
//!
//! The method suits parts that are a profile extruded along one axis, possibly in several stepped
//! layers: plates, brackets, housings, panels. It does not suit a shape that curves along the
//! build axis — a scanned bottle is not a stack of slabs, and the deviation report is what tells
//! the caller so.

use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::body::Body;
use crate::meshbody::{MAX_TRIANGLES, TriMesh};
use crate::{KernelError, Result};

/// Levels one stack may step between. A machined part has a handful; the limit only bounds the
/// work on a mesh whose every facet lands at its own height.
const MAX_LEVELS: usize = 256;
/// Closed loops read out of one section.
const MAX_LOOPS: usize = 512;
/// Lines and arcs one loop may be fitted with.
const MAX_PARTS: usize = 1024;
/// Points one loop is resampled to before fitting.
const MAX_SAMPLES: usize = 4096;
/// Candidate directions scored when looking for the build axis.
const MAX_CANDIDATES: usize = 256;
/// Triangles whose normals are scored against a candidate direction.
const MAX_SCORED: usize = 60_000;
/// Scan points the finished plan is measured against.
const MAX_PROBES: usize = 4000;
/// How far a facet's normal may lean off the build axis and still count as a flat face (degrees).
const FLAT_DEGREES: f64 = 8.;
/// How far a facet's normal may lean off square to the build axis and still count as a wall
/// (degrees), when looking for the direction the walls run in.
const WALL_DEGREES: f64 = 10.;
/// How far a candidate direction may lean and still count as agreeing with it (degrees).
const AGREE_DEGREES: f64 = 5.;
/// The share of a section's own area below which a loop is dropped as scanning debris.
const DEBRIS_AREA: f64 = 0.002;
/// How far a primitive's fitted ends may be moved to meet its neighbour, as a share of the shorter
/// of the two. A tangent meeting is fuzzy in scan data, so the allowance grows with the part.
const JUNCTION_SHARE: f64 = 0.6;
/// The smallest junction allowance (mm), for two short primitives.
const JUNCTION_FLOOR: f64 = 2.;

fn invalid(message: &str) -> KernelError {
    KernelError::Invalid(format!("mesh slabs: {message}"))
}

/// Which mirror planes to look for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Symmetry {
    /// Both in-plane axes; whichever mirrors cleanly is used.
    Auto,
    /// Only the plane square to the sketch's x direction.
    AcrossX,
    /// Only the plane square to the sketch's y direction.
    AcrossY,
    /// Do not look, and do not average anything.
    Off,
}

/// How to read a scan. Lengths are mm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlabOptions {
    /// The scan's own noise: how far a measured point may sit off the surface it belongs to, and
    /// so how close a line or arc has to come before it counts as fitting.
    pub tolerance: f64,
    /// Round finished dimensions to this step; zero leaves them as measured.
    pub grid: f64,
    pub symmetry: Symmetry,
    /// Snap an edge's direction to a multiple of this many degrees when it is already within three
    /// degrees of one. Fifteen catches square and 45-degree edges without touching a real taper.
    pub snap_angle: f64,
    /// The thinnest layer kept. Two levels closer together than this are one level: a scan's top
    /// face is not flat enough to tell a 0.2 mm step from its own noise.
    pub min_thickness: f64,
    /// The direction to build along, where the caller knows it. Without one the stack takes the
    /// direction the most facet area lies square to.
    pub axis: Option<Vec3>,
}

impl Default for SlabOptions {
    fn default() -> SlabOptions {
        SlabOptions { tolerance: 0.25, grid: 0.1, symmetry: Symmetry::Auto, snap_angle: 15., min_thickness: 0.6, axis: None }
    }
}

/// One piece of a fitted outline, in the sketch's own coordinates (mm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Part {
    Line {
        from: [f64; 2],
        to: [f64; 2],
    },
    /// An arc the short way round from `from` to `to`, turning counter-clockwise when `ccw`.
    Arc {
        from: [f64; 2],
        to: [f64; 2],
        centre: [f64; 2],
        radius: f64,
        ccw: bool,
    },
    /// A whole circle: a bore, a boss, or a round outline.
    Circle {
        centre: [f64; 2],
        radius: f64,
    },
}

/// A closed outline of one section.
#[derive(Debug, Clone, PartialEq)]
pub struct Loop {
    pub parts: Vec<Part>,
    /// The signed area the measured points enclose (mm^2). Positive turns the way the sketch's own
    /// axes do; a negative one is a hole in the loop around it.
    pub area: f64,
    /// How far the fitted outline sits from the measured points (mm), at the 95th percentile.
    pub deviation: f64,
    /// Whether lines and arcs fitted. A loop that would not fit is kept as the measured outline,
    /// simplified; it still builds, and it is still worth having, but nothing about it is a
    /// dimension anyone should trust.
    pub fitted: bool,
}

/// One extruded layer of the part.
#[derive(Debug, Clone, PartialEq)]
pub struct Slab {
    /// Where the layer starts and ends along the build axis, measured from the sketch plane.
    pub from: f64,
    pub to: f64,
    pub loops: Vec<Loop>,
}

/// A mirror plane the part was found to be symmetric about.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mirror {
    /// Which in-plane axis the plane is square to: the sketch's x, or its y.
    pub across_x: bool,
    /// Where the plane sits along that axis, in sketch coordinates.
    pub at: f64,
    /// The median distance between the scan and its own reflection (mm): how true the symmetry is.
    pub mismatch: f64,
    /// How many measured features were averaged with their partner across the plane.
    pub paired: usize,
}

/// How far the finished plan sits from the scan it was read from (mm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Deviation {
    pub mean: f64,
    pub p95: f64,
    pub max: f64,
    /// How many scan points were measured.
    pub probes: usize,
}

/// How to rebuild a scanned part: a sketch plane and the layers stacked on it.
#[derive(Debug, Clone, PartialEq)]
pub struct SlabStack {
    /// Where the sketch sits: on the part's lowest level, at the datum the outlines were measured
    /// from (a mirror plane where there is one, the part's own corner otherwise).
    pub origin: Vec3,
    /// The direction the layers stack along, away from `origin`.
    pub axis: Vec3,
    /// The sketch's own x and y directions, right-handed about `axis`.
    pub x_dir: Vec3,
    pub y_dir: Vec3,
    /// The heights the part steps between, from the sketch plane, lowest first.
    pub levels: Vec<f64>,
    pub slabs: Vec<Slab>,
    pub mirrors: Vec<Mirror>,
    /// Triangles the scan was read from, after welding.
    pub triangles: usize,
    /// The scan's total area (mm^2).
    pub area: f64,
    /// The part's size along the sketch's own x, y and the build axis (mm).
    pub size: [f64; 3],
    pub deviation: Option<Deviation>,
}

/// Read a scanned mesh body as a stack of extruded layers.
///
/// Fails rather than guesses: a mesh with no two levels to extrude between, or no section worth
/// fitting, is refused with what was missing, because a plan built from one level would be a
/// plausible-looking part that the scan never showed.
pub fn slab_plan(body: &Body, options: &SlabOptions) -> Result<SlabStack> {
    crate::guard("plan a slab rebuild", || {
        let o = checked(options)?;
        let input = body.mesh.as_ref().ok_or_else(|| invalid("select a mesh body"))?;
        let welded = crate::mesh_convert::weld(input, o.tolerance.min(0.05), MAX_TRIANGLES)?;
        let mesh = welded.mesh.as_ref().ok_or_else(|| invalid("empty mesh"))?;
        let facets = Facets::read(mesh)?;
        if facets.count() < 20 {
            return Err(invalid("too few triangles to read a part from"));
        }
        let frame = Frame::fit(&facets, o.axis)?;
        plan_in(mesh, &facets, &frame, &o)
    })
}

/// The options, with every number checked. Hostile numbers are refused here so nothing below has
/// to wonder whether a tolerance is negative or a grid is infinite.
fn checked(o: &SlabOptions) -> Result<SlabOptions> {
    if !o.tolerance.is_finite() || !(0.001..=10.).contains(&o.tolerance) {
        return Err(invalid("tolerance must be between 0.001 and 10 mm"));
    }
    if !o.grid.is_finite() || !(0. ..=10.).contains(&o.grid) {
        return Err(invalid("grid must be between 0 and 10 mm; 0 leaves dimensions as measured"));
    }
    if !o.snap_angle.is_finite() || !(0. ..=90.).contains(&o.snap_angle) {
        return Err(invalid("snap_angle must be between 0 and 90 degrees; 0 leaves edge directions as measured"));
    }
    if !o.min_thickness.is_finite() || !(0.001..=1000.).contains(&o.min_thickness) {
        return Err(invalid("min_thickness must be between 0.001 and 1000 mm"));
    }
    let axis = match o.axis {
        None => None,
        Some(given) => Some(given.normalized().ok_or_else(|| invalid("the build axis must not be zero length"))?),
    };
    Ok(SlabOptions { axis, ..*o })
}

// ---------------------------------------------------------------------------------------------
// The mesh, read once
// ---------------------------------------------------------------------------------------------

/// Per-triangle facts the whole pipeline works from: the unit normal, the area and the centroid.
/// Read once, because every stage below wants all three and a scan has millions of triangles.
struct Facets {
    normals: Vec<Vec3>,
    areas: Vec<f64>,
    centres: Vec<Vec3>,
    total: f64,
}

impl Facets {
    fn read(mesh: &TriMesh) -> Result<Facets> {
        let mut out = Facets { normals: Vec::new(), areas: Vec::new(), centres: Vec::new(), total: 0. };
        out.normals.reserve(mesh.triangles.len());
        out.areas.reserve(mesh.triangles.len());
        out.centres.reserve(mesh.triangles.len());
        for t in &mesh.triangles {
            let (Some(a), Some(b), Some(c)) =
                (mesh.positions.get(t[0] as usize), mesh.positions.get(t[1] as usize), mesh.positions.get(t[2] as usize))
            else {
                continue;
            };
            let cross = (*b - *a).cross(*c - *a);
            let Some(normal) = cross.normalized() else { continue };
            let area = 0.5 * cross.len();
            if !(area > 0.) {
                continue;
            }
            out.normals.push(normal);
            out.areas.push(area);
            out.centres.push((*a + *b + *c) * (1. / 3.));
            out.total += area;
        }
        if !(out.total > 0.) {
            return Err(invalid("the mesh has no area"));
        }
        Ok(out)
    }

    fn count(&self) -> usize {
        self.areas.len()
    }
}

// ---------------------------------------------------------------------------------------------
// The frame: which way is up, and which way is along
// ---------------------------------------------------------------------------------------------

/// The part's own axes: where the sketch sits and which way it is built.
struct Frame {
    origin: Vec3,
    u: Vec3,
    v: Vec3,
    w: Vec3,
}

impl Frame {
    /// The frame a part was modelled in: the build axis is the direction most of the facet area
    /// faces along, and the in-plane axes run with the walls.
    fn fit(f: &Facets, given: Option<Vec3>) -> Result<Frame> {
        let w = match given {
            Some(axis) => axis,
            None => dominant(f).ok_or_else(|| invalid("no direction has enough flat area to build along; give the axis"))?,
        };
        // Point the axis the way a part is usually drawn, so two runs of the same scan agree.
        let w = if w.z < 0. || (w.z.abs() < 1e-9 && w.x < 0.) { -w } else { w };
        let helper = if w.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
        let Some(e1) = w.cross(helper).normalized() else {
            return Err(invalid("the build axis is degenerate"));
        };
        let e2 = w.cross(e1);
        let u = wall_direction(f, w, e1, e2);
        // Run x along the part's longer side, the way a drawing is laid out. The 2% margin keeps a
        // near-square part from flipping between two runs of the same scan.
        let span = |d: Vec3| spread(f.centres.iter().map(|c| c.dot(d)));
        let u = if span(w.cross(u)) > 1.02 * span(u) { w.cross(u) } else { u };
        // One of the two opposite directions, picked the same way every time.
        let u = if u.x < 0. || (u.x.abs() < 1e-9 && u.y < 0.) { -u } else { u };
        let Some(u) = u.normalized() else { return Err(invalid("the in-plane axis is degenerate")) };
        let v = w.cross(u);
        // The area-weighted centroid: a starting point, moved onto the part's datum once the
        // outlines have been measured and any mirror plane is known.
        let mut origin = Vec3::new(0., 0., 0.);
        for (c, a) in f.centres.iter().zip(&f.areas) {
            origin += *c * *a;
        }
        Ok(Frame { origin: origin * (1. / f.total), u, v, w })
    }

    /// A point in the part's own coordinates.
    fn local(&self, p: Vec3) -> [f64; 3] {
        let d = p - self.origin;
        [d.dot(self.u), d.dot(self.v), d.dot(self.w)]
    }
}

/// The unoriented direction the most facet area faces along.
///
/// A scan's largest flat faces are its top and bottom, so the direction they share is the one the
/// part was built along.
///
/// The candidates are the directions the mesh's own area gathers around, found by rounding every
/// normal into a coarse bucket and keeping the heaviest buckets. Taking the largest facets
/// instead would miss the answer on anything tessellated unevenly: a cylinder's flat ends are cut
/// into a fan of slivers, each one smaller than a single panel of the curved wall beside it, so
/// the ends would never be proposed at all even though together they hold most of the area.
fn dominant(f: &Facets) -> Option<Vec3> {
    let cos_tol = AGREE_DEGREES.to_radians().cos();
    // A bucket about six degrees across: fine enough to keep two real directions apart, coarse
    // enough that a scan's noise does not scatter one face over several.
    let bucket = |n: Vec3| {
        // Unoriented, so a face and the one opposite it land together.
        let n = if n.z < 0. || (n.z.abs() < 1e-9 && (n.x < 0. || (n.x.abs() < 1e-9 && n.y < 0.))) { -n } else { n };
        [(n.x * 16.).round() as i32, (n.y * 16.).round() as i32, (n.z * 16.).round() as i32]
    };
    let mut gathered: std::collections::HashMap<[i32; 3], (f64, Vec3)> = std::collections::HashMap::new();
    for (n, a) in f.normals.iter().zip(&f.areas) {
        let facing = if n.z < 0. || (n.z.abs() < 1e-9 && (n.x < 0. || (n.x.abs() < 1e-9 && n.y < 0.))) { -*n } else { *n };
        let slot = gathered.entry(bucket(*n)).or_insert((0., Vec3::new(0., 0., 0.)));
        slot.0 += *a;
        slot.1 += facing * *a;
    }
    let mut heaviest: Vec<(f64, Vec3)> = gathered.into_values().collect();
    heaviest.sort_by(|a, b| b.0.total_cmp(&a.0));
    heaviest.truncate(MAX_CANDIDATES);
    // Score against an even stride through the mesh rather than all of it: a million triangles say
    // nothing more about which direction wins than sixty thousand spread through them do.
    let stride = f.count().div_ceil(MAX_SCORED).max(1);
    let mut best: Option<(f64, Vec3)> = None;
    for (_, sum) in heaviest {
        let Some(candidate) = sum.normalized() else { continue };
        let mut score = 0.;
        let mut at = 0;
        while at < f.count() {
            if let (Some(n), Some(a)) = (f.normals.get(at), f.areas.get(at))
                && n.dot(candidate).abs() >= cos_tol
            {
                score += *a;
            }
            at += stride;
        }
        if best.is_none_or(|(b, _)| score > b) {
            best = Some((score, candidate));
        }
    }
    let (_, mut axis) = best?;
    // Settle on the average of everything that agrees, so the answer is the faces' own direction
    // and not whichever single facet happened to win.
    for _ in 0..3 {
        let mut sum = Vec3::new(0., 0., 0.);
        for (n, a) in f.normals.iter().zip(&f.areas) {
            let along = n.dot(axis);
            if along.abs() >= cos_tol {
                sum += *n * (*a * along.signum());
            }
        }
        match sum.normalized() {
            Some(next) => axis = next,
            None => break,
        }
    }
    Some(axis)
}

/// The in-plane direction the part's walls run along.
///
/// A machined part's walls are square to each other, so their directions cluster four to a turn.
/// Summing `area * e^(4i*theta)` over the walls collapses those four into one: a strong sum means
/// the part really is square, and a quarter of its angle is the direction to use. Where no such
/// sum appears the part has no square walls to line up with, and the longest direction through the
/// scan serves instead.
fn wall_direction(f: &Facets, w: Vec3, e1: Vec3, e2: Vec3) -> Vec3 {
    let square = WALL_DEGREES.to_radians().sin();
    let (mut re, mut im, mut total) = (0., 0., 0.);
    for (n, a) in f.normals.iter().zip(&f.areas) {
        if n.dot(w).abs() >= square {
            continue;
        }
        let angle = n.dot(e2).atan2(n.dot(e1));
        re += *a * (4. * angle).cos();
        im += *a * (4. * angle).sin();
        total += *a;
    }
    if total > 0. && (re * re + im * im).sqrt() > 0.15 * total {
        let theta = im.atan2(re) / 4.;
        return e1 * theta.cos() + e2 * theta.sin();
    }
    // No square walls: take the direction the scan reaches furthest along, which is the one a
    // drawing of it would be laid out on.
    let mut best = (f64::NEG_INFINITY, e1);
    for step in 0..36 {
        let theta = f64::from(step) * std::f64::consts::PI / 36.;
        let d = e1 * theta.cos() + e2 * theta.sin();
        let reach = spread(f.centres.iter().map(|c| c.dot(d)));
        if reach > best.0 {
            best = (reach, d);
        }
    }
    best.1
}

/// How far a set of numbers reaches, highest less lowest.
fn spread(values: impl Iterator<Item = f64>) -> f64 {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for v in values {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    if lo <= hi { hi - lo } else { 0. }
}

// ---------------------------------------------------------------------------------------------
// The pipeline
// ---------------------------------------------------------------------------------------------

fn plan_in(mesh: &TriMesh, f: &Facets, frame: &Frame, o: &SlabOptions) -> Result<SlabStack> {
    let local: Vec<[f64; 3]> = mesh.positions.iter().map(|p| frame.local(*p)).collect();
    let levels = levels(f, frame, o)?;
    let step = o.tolerance.max(0.15);
    let min_area = (4. * o.tolerance * o.tolerance).max(1.);

    // Cut a section through the middle of each layer rather than at its boundary: a boundary sits
    // in the plane of a face, where a section is whatever the noise there happens to be.
    let mut measured: Vec<(f64, f64, Vec<Vec<[f64; 2]>>)> = Vec::new();
    for pair in levels.windows(2) {
        let (Some(&z0), Some(&z1)) = (pair.first(), pair.get(1)) else { continue };
        let loops = section(&local, &mesh.triangles, 0.5 * (z0 + z1), min_area);
        if !loops.is_empty() {
            measured.push((z0, z1, loops));
        }
    }
    if measured.is_empty() {
        return Err(invalid("no section through the part gave a closed outline; the scan may have holes in it"));
    }

    let mut fitted: Vec<Vec<Vec<Piece>>> = Vec::new();
    for (_, _, loops) in &measured {
        fitted.push(loops.iter().map(|points| fit_loop(points, o.tolerance, step)).collect());
    }

    snap_angles(&mut fitted, o.snap_angle);
    refit_fillets(&mut fitted);
    cluster_features(&mut fitted, (1.5 * o.tolerance).max(0.3));
    let mirrors = symmetrize(&mut fitted, &measured, o);
    let datum = datum(&mirrors, &measured);
    round_features(&mut fitted, o.grid, datum);

    // Everything is measured from the part's own datum and its lowest level, so the sketch sits at
    // the origin and the numbers that come out are the ones someone would have typed in.
    let base = levels.first().copied().unwrap_or(0.);
    let mut slabs = Vec::new();
    for ((z0, z1, loops), pieces) in measured.iter().zip(&fitted) {
        let (mut from, mut to) = (z0 - base, z1 - base);
        if o.grid > 0. {
            from = (from / o.grid).round() * o.grid;
            to = (to / o.grid).round() * o.grid;
        }
        if !(to - from > 1e-6) {
            continue;
        }
        let built: Vec<Loop> = loops.iter().zip(pieces).map(|(points, parts)| close_loop(parts, points, o.tolerance, datum)).collect();
        if !built.is_empty() {
            slabs.push(Slab { from, to, loops: built });
        }
    }
    if slabs.is_empty() {
        return Err(invalid("every layer came out thinner than the grid it was rounded to"));
    }

    let origin = frame.origin + frame.u * datum[0] + frame.v * datum[1] + frame.w * base;
    let moved = Frame { origin, u: frame.u, v: frame.v, w: frame.w };
    let deviation = deviation(&local, datum, base, &slabs);
    let size = [
        spread(local.iter().map(|p| p[0])),
        spread(local.iter().map(|p| p[1])),
        slabs.last().map(|s| s.to).unwrap_or(0.) - slabs.first().map(|s| s.from).unwrap_or(0.),
    ];
    Ok(SlabStack {
        origin: moved.origin,
        axis: moved.w,
        x_dir: moved.u,
        y_dir: moved.v,
        levels: levels.iter().map(|z| z - base).collect(),
        slabs,
        mirrors,
        triangles: f.count(),
        area: f.total,
        size,
        deviation,
    })
}

// ---------------------------------------------------------------------------------------------
// Levels
// ---------------------------------------------------------------------------------------------

/// The heights the part steps between.
///
/// A flat face square to the build axis is a step, so the heights of those faces, clustered, are
/// the levels. The extremes are added whether or not a face was found there, because a scan's very
/// top and bottom are its noisiest places and may carry no flat face at all, and a part always
/// ends somewhere.
fn levels(f: &Facets, frame: &Frame, o: &SlabOptions) -> Result<Vec<f64>> {
    let flat = FLAT_DEGREES.to_radians().cos();
    let mut heights: Vec<(f64, f64)> = Vec::new();
    for ((n, a), c) in f.normals.iter().zip(&f.areas).zip(&f.centres) {
        if n.dot(frame.w).abs() >= flat {
            heights.push((frame.local(*c)[2], *a));
        }
    }
    let reach = {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for c in &f.centres {
            let z = frame.local(*c)[2];
            lo = lo.min(z);
            hi = hi.max(z);
        }
        (lo, hi)
    };
    let least = (DEBRIS_AREA * f.total).max(1.);
    let mut found: Vec<f64> = cluster(&mut heights, o.tolerance.max(0.3)).into_iter().filter(|(_, a)| *a >= least).map(|(z, _)| z).collect();
    for end in [reach.0, reach.1] {
        if end.is_finite() && found.iter().all(|z| (z - end).abs() > 3. * o.tolerance.max(0.3)) {
            found.push(end);
        }
    }
    found.sort_by(f64::total_cmp);
    // Two levels closer than the thinnest layer the caller will accept are one level. Keeping the
    // one with more area behind it keeps the real face and drops the noise beside it.
    let mut kept: Vec<f64> = Vec::new();
    for z in found {
        match kept.last() {
            Some(last) if z - last < o.min_thickness => {}
            _ => kept.push(z),
        }
        if kept.len() >= MAX_LEVELS {
            break;
        }
    }
    if kept.len() < 2 {
        return Err(invalid("could not find a top and a bottom to build between; the part may not be a stack of layers"));
    }
    Ok(kept)
}

/// Single-linkage clusters of weighted values: every run of values no further apart than `tol`
/// becomes one, placed at the run's weighted mean. Returns `(mean, weight)` per cluster.
fn cluster(values: &mut [(f64, f64)], tol: f64) -> Vec<(f64, f64)> {
    values.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = Vec::new();
    let mut run: Vec<(f64, f64)> = Vec::new();
    let flush = |run: &mut Vec<(f64, f64)>, out: &mut Vec<(f64, f64)>| {
        let weight: f64 = run.iter().map(|(_, w)| *w).sum();
        let mean = if weight > 0. {
            run.iter().map(|(v, w)| v * w).sum::<f64>() / weight
        } else {
            run.iter().map(|(v, _)| *v).sum::<f64>() / run.len().max(1) as f64
        };
        if !run.is_empty() {
            out.push((mean, weight));
        }
        run.clear();
    };
    for &(v, w) in values.iter() {
        match run.last() {
            Some(&(prev, _)) if v - prev > tol => flush(&mut run, &mut out),
            _ => {}
        }
        run.push((v, w));
    }
    flush(&mut run, &mut out);
    out
}

// ---------------------------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------------------------

/// The closed outlines where the mesh crosses the plane at height `z`.
///
/// Every triangle that straddles the plane contributes one segment between the two edges it
/// crosses. Keying each crossing by the edge it sits on, rather than by its coordinates, is what
/// makes the chaining exact: two triangles sharing an edge give the same crossing, so the loops
/// close without a tolerance to tune.
fn section(local: &[[f64; 3]], triangles: &[[u32; 3]], z: f64, min_area: f64) -> Vec<Vec<[f64; 2]>> {
    use std::collections::HashMap;
    let mut points: Vec<[f64; 2]> = Vec::new();
    let mut at: HashMap<(u32, u32), u32> = HashMap::new();
    let mut links: HashMap<u32, Vec<u32>> = HashMap::new();
    let height = |i: u32| local.get(i as usize).map(|p| p[2] - z);

    for t in triangles {
        let (Some(a), Some(b), Some(c)) = (height(t[0]), height(t[1]), height(t[2])) else { continue };
        // A vertex exactly on the plane is nudged above it, so a triangle is never half on the
        // plane: every triangle then either straddles cleanly or does not cross at all.
        let side = |d: f64| if d.abs() < 1e-9 { true } else { d > 0. };
        let (sa, sb, sc) = (side(a), side(b), side(c));
        if sa == sb && sb == sc {
            continue;
        }
        let mut ends: Vec<u32> = Vec::with_capacity(2);
        for (i, j, di, dj) in [(t[0], t[1], a, b), (t[1], t[2], b, c), (t[2], t[0], c, a)] {
            if side(di) == side(dj) {
                continue;
            }
            let key = (i.min(j), i.max(j));
            let index = match at.get(&key) {
                Some(&index) => index,
                None => {
                    let (Some(p), Some(q)) = (local.get(i as usize), local.get(j as usize)) else { continue };
                    let span = di - dj;
                    let f = if span.abs() > 1e-12 { di / span } else { 0.5 };
                    let Ok(index) = u32::try_from(points.len()) else { continue };
                    points.push([p[0] + (q[0] - p[0]) * f, p[1] + (q[1] - p[1]) * f]);
                    at.insert(key, index);
                    index
                }
            };
            ends.push(index);
        }
        let (Some(&p), Some(&q)) = (ends.first(), ends.get(1)) else { continue };
        if p != q {
            links.entry(p).or_default().push(q);
            links.entry(q).or_default().push(p);
        }
    }

    let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut starts: Vec<u32> = links.keys().copied().collect();
    starts.sort_unstable();
    let mut out = Vec::new();
    for start in starts {
        if seen.contains(&start) || out.len() >= MAX_LOOPS {
            continue;
        }
        let mut chain = vec![start];
        seen.insert(start);
        let (mut previous, mut current) = (None, start);
        // Bounded by the crossings themselves: the walk only ever steps onto an unvisited one.
        for _ in 0..points.len() {
            let Some(next) = links.get(&current).and_then(|ns| ns.iter().find(|n| Some(**n) != previous && !seen.contains(n)).copied()) else {
                break;
            };
            previous = Some(current);
            current = next;
            seen.insert(current);
            chain.push(current);
        }
        if chain.len() < 4 {
            continue;
        }
        let loop_: Vec<[f64; 2]> = chain.iter().filter_map(|i| points.get(*i as usize).copied()).collect();
        if signed_area(&loop_).abs() >= min_area {
            out.push(loop_);
        }
    }
    orient(out)
}

/// Turn every outline the way its nesting says it should go: an outer one counter-clockwise, a
/// hole inside it clockwise, a boss inside that hole counter-clockwise again.
///
/// The walk that chained the crossings together went whichever way it happened to start, so
/// without this a part's outline and the bore through it can come back turning the same way, and
/// nothing downstream can tell which of them is the hole.
fn orient(mut loops: Vec<Vec<[f64; 2]>>) -> Vec<Vec<[f64; 2]>> {
    let box_of = |outline: &Vec<[f64; 2]>| {
        let mut bounds = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        for p in outline {
            bounds = [bounds[0].min(p[0]), bounds[1].min(p[1]), bounds[2].max(p[0]), bounds[3].max(p[1])];
        }
        bounds
    };
    // Largest first, so everything that could contain a given outline has already been seen.
    loops.sort_by(|a, b| signed_area(b).abs().total_cmp(&signed_area(a).abs()));
    let boxes: Vec<[f64; 4]> = loops.iter().map(box_of).collect();
    let mut depth = vec![0usize; loops.len()];
    let mut tests = 0usize;
    for i in 0..loops.len() {
        let (Some(inner), Some(small)) = (loops.get(i), boxes.get(i)) else { continue };
        let Some(probe) = inner.first() else { continue };
        for j in 0..i {
            let (Some(outer), Some(big)) = (loops.get(j), boxes.get(j)) else { continue };
            if small[0] < big[0] || small[1] < big[1] || small[2] > big[2] || small[3] > big[3] {
                continue;
            }
            tests += 1;
            if tests > 4096 {
                break;
            }
            if polyline_crossings(*probe, outer) % 2 == 1
                && let Some(slot) = depth.get_mut(i)
            {
                *slot += 1;
            }
        }
    }
    for (outline, depth) in loops.iter_mut().zip(&depth) {
        let wanted = depth.is_multiple_of(2);
        if (signed_area(outline) > 0.) != wanted {
            outline.reverse();
        }
    }
    loops
}

/// The signed area a closed polygon encloses; positive turns counter-clockwise.
fn signed_area(points: &[[f64; 2]]) -> f64 {
    let mut sum = 0.;
    for (i, p) in points.iter().enumerate() {
        let Some(q) = points.get((i + 1) % points.len()) else { continue };
        sum += p[0] * q[1] - q[0] * p[1];
    }
    0.5 * sum
}

// ---------------------------------------------------------------------------------------------
// Fitting an outline with lines and arcs
// ---------------------------------------------------------------------------------------------

/// A line or an arc fitted to a run of measured points, before its ends are settled against its
/// neighbours. A line keeps its direction and a point on it rather than its ends, because the
/// direction is what was measured well and the ends are where two fits meet.
#[derive(Debug, Clone)]
enum Piece {
    Line {
        /// A point the line passes through, and its unit direction.
        at: [f64; 2],
        dir: [f64; 2],
        /// The first and last measured points of the run it was fitted to. The fitted line is
        /// endless; these say which stretch of it was seen, which is what settles where it meets
        /// its neighbours and how far that meeting may be from what was measured.
        a: [f64; 2],
        b: [f64; 2],
    },
    Arc {
        centre: [f64; 2],
        radius: f64,
        /// The measured points this arc was fitted to, kept so a corner round can be re-fitted
        /// once its neighbouring lines are known.
        points: Vec<[f64; 2]>,
        /// Whether it is a corner round between two lines, which is fitted differently.
        fillet: bool,
    },
    /// A loop that is one whole circle.
    Circle { centre: [f64; 2], radius: f64 },
}

/// Fit one measured outline with lines and arcs, or with a single circle where it is round.
fn fit_loop(points: &[[f64; 2]], tol: f64, step: f64) -> Vec<Piece> {
    let sampled = resample(points, step);
    if sampled.len() < 4 {
        return Vec::new();
    }
    if let Some((centre, radius, residuals)) = fit_circle(&sampled)
        && fits(&residuals, tol)
    {
        return vec![Piece::Circle { centre, radius }];
    }
    // Start at the sharpest corner, so no line or arc is split across the seam and then has to be
    // put back together.
    let start = sharpest(&sampled);
    let mut rolled: Vec<[f64; 2]> = sampled.get(start..).unwrap_or(&[]).to_vec();
    rolled.extend_from_slice(sampled.get(..start).unwrap_or(&[]));
    if let Some(first) = rolled.first().copied() {
        rolled.push(first);
    }
    let pieces = merge(split(&rolled, tol), &rolled, tol);
    pieces.into_iter().map(|(piece, _, _)| piece).collect()
}

/// Resample a closed outline to points about `step` apart along it, so that a fit is not pulled by
/// whichever stretch the scanner happened to sample densely.
fn resample(points: &[[f64; 2]], step: f64) -> Vec<[f64; 2]> {
    let mut lengths = vec![0.];
    let mut total = 0.;
    for i in 0..points.len() {
        let (Some(p), Some(q)) = (points.get(i), points.get((i + 1) % points.len())) else { continue };
        total += distance(*p, *q);
        lengths.push(total);
    }
    if !(total > 0.) {
        return Vec::new();
    }
    let count = ((total / step.max(1e-6)).round() as usize).clamp(12, MAX_SAMPLES);
    let mut out = Vec::with_capacity(count);
    let mut at = 0usize;
    for i in 0..count {
        let target = total * (i as f64) / (count as f64);
        while lengths.get(at + 1).is_some_and(|next| *next < target) {
            at += 1;
        }
        let (Some(&from), Some(&to)) = (lengths.get(at), lengths.get(at + 1)) else { break };
        let (Some(p), Some(q)) = (points.get(at), points.get((at + 1) % points.len())) else { break };
        let f = if to - from > 1e-12 { (target - from) / (to - from) } else { 0. };
        out.push([p[0] + (q[0] - p[0]) * f, p[1] + (q[1] - p[1]) * f]);
    }
    out
}

/// Where a closed outline turns most sharply: the corner to start fitting from.
fn sharpest(points: &[[f64; 2]]) -> usize {
    let n = points.len();
    let reach = 3.min(n / 4).max(1);
    let mut best = (0., 0usize);
    for i in 0..n {
        let (Some(&back), Some(&here), Some(&ahead)) = (points.get((i + n - reach) % n), points.get(i), points.get((i + reach) % n)) else {
            continue;
        };
        let a = [here[0] - back[0], here[1] - back[1]];
        let b = [ahead[0] - here[0], ahead[1] - here[1]];
        let turn = (a[0] * b[1] - a[1] * b[0]).atan2(a[0] * b[0] + a[1] * b[1]).abs();
        if turn > best.0 {
            best = (turn, i);
        }
    }
    best.1
}

/// Split a run of points until every piece is a line or an arc, splitting at whichever point sits
/// furthest off the chord each time.
fn split(points: &[[f64; 2]], tol: f64) -> Vec<(Piece, usize, usize)> {
    let reach = spread(points.iter().map(|p| p[0])).max(spread(points.iter().map(|p| p[1])));
    let least = 3usize;
    let mut out: Vec<(Piece, usize, usize)> = Vec::new();
    let mut todo = vec![(0usize, points.len().saturating_sub(1))];
    while let Some((from, to)) = todo.pop() {
        if out.len() + todo.len() >= MAX_PARTS {
            break;
        }
        let Some(run) = points.get(from..=to) else { continue };
        if let Some(piece) = classify(run, tol, reach) {
            out.push((piece, from, to));
            continue;
        }
        if to - from + 1 > 2 * least
            && let Some(at) = furthest(run, least)
        {
            todo.push((from + at, to));
            todo.push((from, from + at));
            continue;
        }
        // Nothing fits and it is too short to split again: a line through it is the honest answer,
        // and the loop's own deviation will say how good it is.
        if let Some(piece) = line_through(run) {
            out.push((piece, from, to));
        }
    }
    out.sort_by_key(|(_, from, _)| *from);
    out
}

/// Put neighbouring pieces of the same kind back together where one fit covers both: splitting is
/// greedy and often cuts a long edge in two at a noisy point.
fn merge(mut pieces: Vec<(Piece, usize, usize)>, points: &[[f64; 2]], tol: f64) -> Vec<(Piece, usize, usize)> {
    let reach = spread(points.iter().map(|p| p[0])).max(spread(points.iter().map(|p| p[1])));
    // Bounded: every pass that changes anything removes a piece.
    for _ in 0..MAX_PARTS {
        let mut joined = false;
        for i in 0..pieces.len().saturating_sub(1) {
            let (Some(a), Some(b)) = (pieces.get(i), pieces.get(i + 1)) else { continue };
            if !same_kind(&a.0, &b.0) {
                continue;
            }
            let (from, to) = (a.1, b.2);
            let Some(run) = points.get(from..=to) else { continue };
            let Some(piece) = classify(run, tol, reach) else { continue };
            if !same_kind(&piece, &a.0) {
                continue;
            }
            pieces.splice(i..=i + 1, [(piece, from, to)]);
            joined = true;
            break;
        }
        if !joined {
            break;
        }
    }
    pieces
}

fn same_kind(a: &Piece, b: &Piece) -> bool {
    matches!((a, b), (Piece::Line { .. }, Piece::Line { .. }) | (Piece::Arc { .. }, Piece::Arc { .. }))
}

/// Fit a run of points with a line or an arc, or say that neither comes close enough.
///
/// Where both fit, the arc wins only when it turns far enough to be a real round and fits clearly
/// better: a 2 mm fillet on a 60 mm edge is barely off a straight line, and calling every slightly
/// bowed edge an arc would fill the rebuilt part with radii nobody put there.
fn classify(run: &[[f64; 2]], tol: f64, reach: f64) -> Option<Piece> {
    let line = line_through(run).and_then(|piece| {
        let Piece::Line { at, dir, .. } = &piece else { return None };
        let normal = [-dir[1], dir[0]];
        let residuals: Vec<f64> = run.iter().map(|p| ((p[0] - at[0]) * normal[0] + (p[1] - at[1]) * normal[1]).abs()).collect();
        fits(&residuals, tol).then_some((piece, rms(&residuals)))
    });
    let arc = (run.len() >= 6)
        .then(|| fit_circle(run))
        .flatten()
        .filter(|(_, radius, residuals)| *radius < 5. * reach.max(1e-6) && fits(residuals, tol))
        .and_then(|(centre, radius, residuals)| {
            let (sweep, steady) = sweep(run, centre)?;
            (steady && sweep >= 12_f64.to_radians())
                .then(|| (Piece::Arc { centre, radius, points: run.to_vec(), fillet: false }, rms(&residuals), sweep))
        });
    match (line, arc) {
        (Some((line, line_rms)), Some((arc, arc_rms, sweep))) => {
            Some(if sweep >= 20_f64.to_radians() && arc_rms < 0.5 * line_rms { arc } else { line })
        }
        (Some((line, _)), None) => Some(line),
        (None, Some((arc, _, _))) => Some(arc),
        (None, None) => None,
    }
}

/// Whether a fit is close enough everywhere: within tolerance across all but the worst twentieth
/// of the points, and never wildly off. One stray scan point should not reject a good edge, and a
/// run that leaves the edge entirely should not pass on its average.
fn fits(residuals: &[f64], tol: f64) -> bool {
    if residuals.is_empty() {
        return false;
    }
    let mut sorted = residuals.to_vec();
    sorted.sort_by(f64::total_cmp);
    percentile(&sorted, 0.95) <= tol && sorted.last().is_some_and(|worst| *worst <= 2.5 * tol)
}

fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.;
    }
    let at = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted.get(at.min(sorted.len() - 1)).copied().unwrap_or(0.)
}

fn rms(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.;
    }
    (values.iter().map(|v| v * v).sum::<f64>() / values.len() as f64).sqrt()
}

/// The best-fit line through a run of points: its centroid, and the direction its spread lies
/// along, from the closed-form principal direction of the two-by-two covariance.
fn line_through(run: &[[f64; 2]]) -> Option<Piece> {
    if run.len() < 2 {
        return None;
    }
    let n = run.len() as f64;
    let cx = run.iter().map(|p| p[0]).sum::<f64>() / n;
    let cy = run.iter().map(|p| p[1]).sum::<f64>() / n;
    let (mut xx, mut xy, mut yy) = (0., 0., 0.);
    for p in run {
        let (dx, dy) = (p[0] - cx, p[1] - cy);
        xx += dx * dx;
        xy += dx * dy;
        yy += dy * dy;
    }
    let theta = 0.5 * (2. * xy).atan2(xx - yy);
    let dir = [theta.cos(), theta.sin()];
    let (Some(first), Some(last)) = (run.first(), run.last()) else { return None };
    Some(Piece::Line { at: [cx, cy], dir, a: *first, b: *last })
}

/// The point of a run furthest off the chord between its ends: where to split it.
fn furthest(run: &[[f64; 2]], least: usize) -> Option<usize> {
    let (first, last) = (*run.first()?, *run.last()?);
    let chord = [last[0] - first[0], last[1] - first[1]];
    let length = (chord[0] * chord[0] + chord[1] * chord[1]).sqrt();
    let mut best = (f64::NEG_INFINITY, None);
    for (i, p) in run.iter().enumerate() {
        if i < least || i + least >= run.len() {
            continue;
        }
        let d = [p[0] - first[0], p[1] - first[1]];
        let off = if length > 1e-12 { (d[0] * -chord[1] + d[1] * chord[0]).abs() / length } else { (d[0] * d[0] + d[1] * d[1]).sqrt() };
        if off > best.0 {
            best = (off, Some(i));
        }
    }
    best.1
}

/// Fit a circle to a run of points: an algebraic first guess, then a few steps of the geometric
/// fit it is biased away from. The algebraic fit is what makes this cheap and the refinement is
/// what makes it right on a short arc, where the algebraic one pulls the radius wide.
fn fit_circle(run: &[[f64; 2]]) -> Option<([f64; 2], f64, Vec<f64>)> {
    if run.len() < 3 {
        return None;
    }
    let n = run.len() as f64;
    let (mut sx, mut sy, mut sxx, mut sxy, mut syy, mut sz, mut sxz, mut syz) = (0., 0., 0., 0., 0., 0., 0., 0.);
    for p in run {
        let (x, y) = (p[0], p[1]);
        let z = x * x + y * y;
        sx += x;
        sy += y;
        sxx += x * x;
        sxy += x * y;
        syy += y * y;
        sz += z;
        sxz += x * z;
        syz += y * z;
    }
    let m = [[2. * sxx, 2. * sxy, sx], [2. * sxy, 2. * syy, sy], [2. * sx, 2. * sy, n]];
    let solved = solve3(m, [sxz, syz, sz])?;
    let (mut cx, mut cy) = (solved[0], solved[1]);
    let squared = solved[2] + cx * cx + cy * cy;
    if !(squared > 0.) {
        return None;
    }
    let mut r = squared.sqrt();
    // Gauss-Newton on the true distances. Five steps is plenty: the algebraic guess is already
    // close, and a fit that has not settled by then is not a circle.
    for _ in 0..5 {
        let (mut jtj, mut jtr) = ([[0.; 3]; 3], [0.; 3]);
        for p in run {
            let (dx, dy) = (p[0] - cx, p[1] - cy);
            let d = (dx * dx + dy * dy).sqrt();
            if d < 1e-12 {
                continue;
            }
            let j = [-dx / d, -dy / d, -1.];
            let residual = d - r;
            for a in 0..3 {
                for b in 0..3 {
                    let (Some(ja), Some(jb)) = (j.get(a), j.get(b)) else { continue };
                    if let Some(cell) = jtj.get_mut(a).and_then(|row| row.get_mut(b)) {
                        *cell += ja * jb;
                    }
                }
                if let (Some(ja), Some(cell)) = (j.get(a), jtr.get_mut(a)) {
                    *cell += -ja * residual;
                }
            }
        }
        let Some(step) = solve3(jtj, jtr) else { break };
        cx += step[0];
        cy += step[1];
        r += step[2];
        if !(r > 0.) {
            return None;
        }
    }
    if !r.is_finite() || !(r > 0.) || !cx.is_finite() || !cy.is_finite() {
        return None;
    }
    let residuals = run.iter().map(|p| (distance(*p, [cx, cy]) - r).abs()).collect();
    Some(([cx, cy], r, residuals))
}

/// Solve a three-by-three system by Cramer's rule, or say it is too close to singular to trust.
fn solve3(m: [[f64; 3]; 3], rhs: [f64; 3]) -> Option<[f64; 3]> {
    let det = |a: [[f64; 3]; 3]| {
        a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
    };
    let base = det(m);
    if !base.is_finite() || base.abs() < 1e-12 {
        return None;
    }
    let mut out = [0.; 3];
    for column in 0..3 {
        let mut swapped = m;
        for row in 0..3 {
            if let (Some(cell), Some(value)) = (swapped.get_mut(row).and_then(|r| r.get_mut(column)), rhs.get(row)) {
                *cell = *value;
            }
        }
        let Some(slot) = out.get_mut(column) else { continue };
        *slot = det(swapped) / base;
    }
    out.iter().all(|v| v.is_finite()).then_some(out)
}

/// How far a run of points turns about a centre, and whether it keeps turning the same way. A run
/// that doubles back is not an arc however well a circle happens to fit it.
fn sweep(run: &[[f64; 2]], centre: [f64; 2]) -> Option<(f64, bool)> {
    let angle = |p: &[f64; 2]| (p[1] - centre[1]).atan2(p[0] - centre[0]);
    let mut previous = angle(run.first()?);
    let mut total = 0.;
    let (mut up, mut down) = (false, false);
    for p in run.iter().skip(1) {
        let here = angle(p);
        let mut step = here - previous;
        while step > std::f64::consts::PI {
            step -= std::f64::consts::TAU;
        }
        while step < -std::f64::consts::PI {
            step += std::f64::consts::TAU;
        }
        if step > 1e-3 {
            up = true;
        }
        if step < -1e-3 {
            down = true;
        }
        total += step;
        previous = here;
    }
    Some((total.abs(), !(up && down)))
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

// ---------------------------------------------------------------------------------------------
// Tidying what was measured
// ---------------------------------------------------------------------------------------------

/// Where one fitted piece lives: which slab, which loop of it, which piece of that loop.
type Path = (usize, usize, usize);

fn paths(fitted: &[Vec<Vec<Piece>>]) -> Vec<Path> {
    let mut out = Vec::new();
    for (s, slab) in fitted.iter().enumerate() {
        for (l, outline) in slab.iter().enumerate() {
            for p in 0..outline.len() {
                out.push((s, l, p));
            }
        }
    }
    out
}

fn piece_at(fitted: &[Vec<Vec<Piece>>], at: Path) -> Option<&Piece> {
    fitted.get(at.0)?.get(at.1)?.get(at.2)
}

fn piece_mut(fitted: &mut [Vec<Vec<Piece>>], at: Path) -> Option<&mut Piece> {
    fitted.get_mut(at.0)?.get_mut(at.1)?.get_mut(at.2)
}

/// Square up the edges that are already nearly square.
///
/// A scan of a milled edge comes back a quarter of a degree off vertical. Left alone that quarter
/// degree becomes a taper in the rebuilt part, and every dimension taken off that edge disagrees
/// with every other. An edge within three degrees of a multiple of `snap` was meant to be on it.
fn snap_angles(fitted: &mut [Vec<Vec<Piece>>], snap: f64) {
    if !(snap > 0.) {
        return;
    }
    let snap = snap.to_radians();
    let slack = 3_f64.to_radians();
    for at in paths(fitted) {
        let Some(Piece::Line { dir, .. }) = piece_mut(fitted, at) else { continue };
        let angle = dir[1].atan2(dir[0]);
        let steps = (angle / snap).round();
        if (angle - steps * snap).abs() <= slack {
            let settled = steps * snap;
            *dir = [settled.cos(), settled.sin()];
        }
    }
}

/// Which axis a line runs along once it has been snapped: 0 along x, 1 along y, neither otherwise.
fn runs_along(piece: &Piece) -> Option<usize> {
    let Piece::Line { dir, .. } = piece else { return None };
    if dir[1].abs() < 1e-9 {
        Some(0)
    } else if dir[0].abs() < 1e-9 {
        Some(1)
    } else {
        None
    }
}

/// Make the measurements that should agree agree.
///
/// Two edges on one face of a part measure a few hundredths apart, and two holes drilled with one
/// bit measure two radii. Grouping the readings that sit within a tolerance of each other and
/// giving the whole group its weighted mean is what turns a scan's readings back into the handful
/// of dimensions the part was made to.
fn cluster_features(fitted: &mut [Vec<Vec<Piece>>], tol: f64) {
    let all = paths(fitted);
    for coord in [0usize, 1] {
        // A line running along x is held by where it sits in y, and the other way about.
        let lines: Vec<Path> = all.iter().copied().filter(|at| piece_at(fitted, *at).and_then(runs_along) == Some(1 - coord)).collect();
        let measured: Vec<(f64, f64)> = lines
            .iter()
            .filter_map(|at| match piece_at(fitted, *at) {
                Some(Piece::Line { at: on, a, b, .. }) => Some((*on.get(coord)?, distance(*a, *b).max(1e-3))),
                _ => None,
            })
            .collect();
        for (at, settled) in lines.iter().zip(cluster_to(&measured, tol)) {
            if let Some(Piece::Line { at: on, .. }) = piece_mut(fitted, *at)
                && let Some(slot) = on.get_mut(coord)
            {
                *slot = settled;
            }
        }

        let rounds: Vec<Path> =
            all.iter().copied().filter(|at| matches!(piece_at(fitted, *at), Some(Piece::Arc { fillet: false, .. } | Piece::Circle { .. }))).collect();
        let measured: Vec<(f64, f64)> = rounds
            .iter()
            .filter_map(|at| match piece_at(fitted, *at) {
                Some(Piece::Arc { centre, .. } | Piece::Circle { centre, .. }) => Some((*centre.get(coord)?, 1.)),
                _ => None,
            })
            .collect();
        for (at, settled) in rounds.iter().zip(cluster_to(&measured, tol)) {
            if let Some(Piece::Arc { centre, .. } | Piece::Circle { centre, .. }) = piece_mut(fitted, *at)
                && let Some(slot) = centre.get_mut(coord)
            {
                *slot = settled;
            }
        }
    }
    // Radii last, and more tightly: two drill sizes can sit closer together than two readings of
    // one edge do.
    let rounds: Vec<Path> =
        all.iter().copied().filter(|at| matches!(piece_at(fitted, *at), Some(Piece::Arc { .. } | Piece::Circle { .. }))).collect();
    let measured: Vec<(f64, f64)> = rounds
        .iter()
        .filter_map(|at| match piece_at(fitted, *at) {
            Some(Piece::Arc { radius, .. } | Piece::Circle { radius, .. }) => Some((*radius, 1.)),
            _ => None,
        })
        .collect();
    for (at, settled) in rounds.iter().zip(cluster_to(&measured, 0.5 * tol)) {
        if let Some(Piece::Arc { radius, .. } | Piece::Circle { radius, .. }) = piece_mut(fitted, *at) {
            *radius = settled;
        }
    }
}

/// The value each entry takes once every run of entries no further apart than `tol` is made one,
/// in the order the entries came in.
fn cluster_to(values: &[(f64, f64)], tol: f64) -> Vec<f64> {
    let mut settled: Vec<f64> = values.iter().map(|(v, _)| *v).collect();
    let mut order: Vec<usize> = (0..values.len()).collect();
    let key = |i: &usize| values.get(*i).map(|(v, _)| *v).unwrap_or(0.);
    order.sort_by(|a, b| key(a).total_cmp(&key(b)));
    let flush = |run: &mut Vec<usize>, settled: &mut Vec<f64>| {
        let weight: f64 = run.iter().filter_map(|i| values.get(*i)).map(|(_, w)| *w).sum();
        if weight > 0. {
            let mean = run.iter().filter_map(|i| values.get(*i)).map(|(v, w)| v * w).sum::<f64>() / weight;
            for i in run.iter() {
                if let Some(slot) = settled.get_mut(*i) {
                    *slot = mean;
                }
            }
        }
        run.clear();
    };
    let mut run: Vec<usize> = Vec::new();
    for i in order {
        if run.last().is_some_and(|last| key(&i) - key(last) > tol) {
            flush(&mut run, &mut settled);
        }
        run.push(i);
    }
    flush(&mut run, &mut settled);
    settled
}

/// Whether two lines meet at a real corner rather than carrying on in much the same direction.
fn is_corner(a: &Piece, b: &Piece) -> bool {
    let (Piece::Line { dir: p, .. }, Piece::Line { dir: q, .. }) = (a, b) else { return false };
    (p[0] * q[0] + p[1] * q[1]).abs() < 20_f64.to_radians().cos()
}

/// The centre of the circle of radius `r` tangent to both lines, on the side `towards` lies.
fn fillet_centre(before: &Piece, after: &Piece, r: f64, towards: [f64; 2]) -> Option<[f64; 2]> {
    let mut moved = Vec::with_capacity(2);
    for line in [before, after] {
        let Piece::Line { at, dir, a, b } = line else { return None };
        let normal = [-dir[1], dir[0]];
        let side = if (towards[0] - at[0]) * normal[0] + (towards[1] - at[1]) * normal[1] >= 0. { 1. } else { -1. };
        moved.push(Piece::Line { at: [at[0] + normal[0] * side * r, at[1] + normal[1] * side * r], dir: *dir, a: *a, b: *b });
    }
    cross_lines(moved.first()?, moved.get(1)?)
}

/// Re-fit the corner rounds, each held tangent to the two edges it joins.
///
/// A free circle fit on a short, noisy 90-degree arc is unstable: the centre and the radius trade
/// off against each other and a scan's own noise moves both. The edges either side are measured
/// far better, so holding the arc tangent to them leaves only the radius to look for, and the
/// radius that comes out is the one the cutter had.
fn refit_fillets(fitted: &mut [Vec<Vec<Piece>>]) {
    for slab in fitted.iter_mut() {
        for outline in slab.iter_mut() {
            let n = outline.len();
            if n < 3 {
                continue;
            }
            for i in 0..n {
                let (Some(before), Some(after)) = (outline.get((i + n - 1) % n).cloned(), outline.get((i + 1) % n).cloned()) else { continue };
                if !is_corner(&before, &after) {
                    continue;
                }
                let Some(Piece::Arc { centre, radius, points, fillet }) = outline.get_mut(i) else { continue };
                // Drop the ends of the measured run: they bleed into the straight edges either
                // side, which drags the fitted radius wider than it is.
                let trim = points.len() / 5;
                let inner = points.get(trim..points.len().saturating_sub(trim)).filter(|run| run.len() >= 3).unwrap_or(points.as_slice()).to_vec();
                let mut best: Option<(f64, f64, [f64; 2])> = None;
                for step in 0..=140 {
                    let r = *radius * (0.4 + 1.4 * f64::from(step) / 140.);
                    let Some(found) = fillet_centre(&before, &after, r, *centre) else { continue };
                    let mut off: Vec<f64> = inner.iter().map(|p| (distance(*p, found) - r).abs()).collect();
                    off.sort_by(f64::total_cmp);
                    let error = percentile(&off, 0.5);
                    if best.is_none_or(|(worst, _, _)| error < worst) {
                        best = Some((error, r, found));
                    }
                }
                if let Some((_, r, found)) = best {
                    *radius = r;
                    *centre = found;
                    *fillet = true;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Symmetry
// ---------------------------------------------------------------------------------------------

/// The measured section points of every layer, at the height each was cut, thinned to a bounded
/// number: enough to tell a mirror plane from a near miss, few enough to search quickly.
fn section_points(measured: &[(f64, f64, Vec<Vec<[f64; 2]>>)]) -> Vec<[f64; 3]> {
    let total: usize = measured.iter().flat_map(|(_, _, loops)| loops.iter()).map(Vec::len).sum();
    let stride = total.div_ceil(MAX_PROBES.saturating_mul(2)).max(1);
    let mut out = Vec::new();
    let mut at = 0usize;
    for (z0, z1, loops) in measured {
        for outline in loops {
            for p in outline {
                if at.is_multiple_of(stride) {
                    out.push([p[0], p[1], 0.5 * (z0 + z1)]);
                }
                at += 1;
            }
        }
    }
    out
}

/// Points in buckets, so the nearest one to a given point is found by looking at the buckets
/// around it rather than at all of them.
struct Near {
    cell: f64,
    bins: std::collections::HashMap<[i64; 3], Vec<u32>>,
    points: Vec<[f64; 3]>,
}

impl Near {
    fn build(points: Vec<[f64; 3]>, cell: f64) -> Near {
        let cell = cell.max(1e-6);
        let mut bins: std::collections::HashMap<[i64; 3], Vec<u32>> = std::collections::HashMap::new();
        for (i, p) in points.iter().enumerate() {
            let Ok(i) = u32::try_from(i) else { break };
            bins.entry(Near::bin(*p, cell)).or_default().push(i);
        }
        Near { cell, bins, points }
    }

    fn bin(p: [f64; 3], cell: f64) -> [i64; 3] {
        [(p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64, (p[2] / cell).floor() as i64]
    }

    /// The nearest point and how far off it is, looking two buckets out before giving up: a point
    /// with nothing that close is not matched at all, which is the answer symmetry wants.
    fn nearest(&self, p: [f64; 3]) -> Option<(f64, usize)> {
        let home = Near::bin(p, self.cell);
        let mut best: Option<(f64, usize)> = None;
        for dx in -2..=2i64 {
            for dy in -2..=2i64 {
                for dz in -2..=2i64 {
                    let Some(found) = self.bins.get(&[home[0] + dx, home[1] + dy, home[2] + dz]) else { continue };
                    for i in found {
                        let Some(q) = self.points.get(*i as usize) else { continue };
                        let d = ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
                        if best.is_none_or(|(worst, _)| d < worst) {
                            best = Some((d, *i as usize));
                        }
                    }
                }
            }
        }
        best
    }
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    percentile(values, 0.5)
}

/// Find the mirror plane square to `axis`, if the part has one.
///
/// A plane is proposed at the middle of the part and then walked to where it fits: each round
/// reflects the measured points, matches each reflection to the nearest real point, and shifts the
/// plane by half the median disagreement. A plane that is really there settles in a round or two;
/// one that is not never gets close, and the final check on the median and the tail is what tells
/// the two apart.
fn detect_mirror(near: &Near, axis: usize, tol: f64) -> Option<(f64, f64)> {
    let along = |p: &[f64; 3]| p.get(axis).copied().unwrap_or(0.);
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in &near.points {
        lo = lo.min(along(p));
        hi = hi.max(along(p));
    }
    if !(hi > lo) || near.points.len() < 50 {
        return None;
    }
    let mut at = 0.5 * (lo + hi);
    let reflect = |p: &[f64; 3], at: f64| {
        let mut q = *p;
        if let Some(slot) = q.get_mut(axis) {
            *slot = 2. * at - *slot;
        }
        q
    };
    for _ in 0..4 {
        let mut shifts: Vec<f64> = Vec::new();
        for p in &near.points {
            let mirrored = reflect(p, at);
            let Some((_, found)) = near.nearest(mirrored) else { continue };
            let Some(real) = near.points.get(found) else { continue };
            shifts.push(along(real) - along(&mirrored));
        }
        if shifts.is_empty() {
            return None;
        }
        at += 0.5 * median(&mut shifts);
    }
    let mut off: Vec<f64> = near.points.iter().map(|p| near.nearest(reflect(p, at)).map_or(f64::INFINITY, |(d, _)| d)).collect();
    off.sort_by(f64::total_cmp);
    let middle = percentile(&off, 0.5);
    (middle <= tol && percentile(&off, 0.9) <= 3. * tol).then_some((at, middle))
}

/// The same piece seen in the mirror.
fn reflected(piece: &Piece, axis: usize, at: f64) -> Piece {
    let flip = |p: [f64; 2]| {
        let mut q = p;
        if let Some(slot) = q.get_mut(axis) {
            *slot = 2. * at - *slot;
        }
        q
    };
    match piece {
        Piece::Line { at: on, dir, a, b } => {
            let mut turned = *dir;
            if let Some(slot) = turned.get_mut(axis) {
                *slot = -*slot;
            }
            Piece::Line { at: flip(*on), dir: turned, a: flip(*a), b: flip(*b) }
        }
        Piece::Arc { centre, radius, points, fillet } => {
            Piece::Arc { centre: flip(*centre), radius: *radius, points: points.iter().map(|p| flip(*p)).collect(), fillet: *fillet }
        }
        Piece::Circle { centre, radius } => Piece::Circle { centre: flip(*centre), radius: *radius },
    }
}

/// How well one piece stands in for another, or nothing if they are not the same feature.
fn alike(p: &Piece, q: &Piece, tol: f64) -> Option<f64> {
    match (p, q) {
        (Piece::Line { at: pa, dir: pd, a: p0, b: p1 }, Piece::Line { at: qa, dir: qd, a: q0, b: q1 }) => {
            let (ps, qs) = (distance(*p0, *p1), distance(*q0, *q1));
            if ((pd[0] * qd[0] + pd[1] * qd[1]).abs() - 1.).abs() > 1e-3 {
                return None;
            }
            let normal = [-pd[1], pd[0]];
            let off = ((qa[0] - pa[0]) * normal[0] + (qa[1] - pa[1]) * normal[1]).abs();
            let along = ((qa[0] - pa[0]) * pd[0] + (qa[1] - pa[1]) * pd[1]).abs();
            (off <= tol && along <= 0.5 * (ps + qs) + tol).then_some(off + 0.01 * along)
        }
        (
            Piece::Arc { centre: pc, radius: pr, .. } | Piece::Circle { centre: pc, radius: pr },
            Piece::Arc { centre: qc, radius: qr, .. } | Piece::Circle { centre: qc, radius: qr },
        ) => {
            let apart = distance(*pc, *qc);
            let sizes = (pr - qr).abs();
            (apart <= tol && sizes <= tol).then_some(apart + sizes)
        }
        _ => None,
    }
}

/// Average every feature with its partner across the mirror plane, and give the partner the
/// averaged feature's own reflection, so the two end up exactly symmetric rather than nearly so.
fn pair_across(fitted: &mut [Vec<Vec<Piece>>], axis: usize, at: f64, tol: f64) -> usize {
    let mut paired = 0;
    for s in 0..fitted.len() {
        let here: Vec<Path> = paths(fitted).into_iter().filter(|p| p.0 == s).collect();
        let mut done: Vec<Path> = Vec::new();
        for left in here.iter().copied() {
            if done.contains(&left) {
                continue;
            }
            let Some(mine) = piece_at(fitted, left).cloned() else { continue };
            let mut best: Option<(f64, Path)> = None;
            for right in here.iter().copied() {
                if done.contains(&right) {
                    continue;
                }
                let Some(theirs) = piece_at(fitted, right) else { continue };
                let Some(score) = alike(&mine, &reflected(theirs, axis, at), tol) else { continue };
                if best.is_none_or(|(worst, _)| score < worst) {
                    best = Some((score, right));
                }
            }
            let Some((_, right)) = best else { continue };
            let Some(theirs) = piece_at(fitted, right).map(|p| reflected(p, axis, at)) else { continue };
            let Some(mine) = piece_mut(fitted, left) else { continue };
            match (mine, &theirs) {
                (Piece::Line { at: on, dir, .. }, Piece::Line { at: other, .. }) => {
                    let normal = [-dir[1], dir[0]];
                    let shift = 0.5 * ((other[0] - on[0]) * normal[0] + (other[1] - on[1]) * normal[1]);
                    *on = [on[0] + normal[0] * shift, on[1] + normal[1] * shift];
                }
                (
                    Piece::Arc { centre, radius, .. } | Piece::Circle { centre, radius },
                    Piece::Arc { centre: other, radius: size, .. } | Piece::Circle { centre: other, radius: size },
                ) => {
                    *centre = [0.5 * (centre[0] + other[0]), 0.5 * (centre[1] + other[1])];
                    *radius = 0.5 * (*radius + size);
                }
                _ => {}
            }
            if right != left
                && let Some(settled) = piece_at(fitted, left).map(|p| reflected(p, axis, at))
                && let Some(partner) = piece_mut(fitted, right)
            {
                // Keep the partner's own measured points: only what was fitted is replaced.
                match (partner, settled) {
                    (Piece::Line { at: on, dir, .. }, Piece::Line { at: from, dir: towards, .. }) => {
                        *on = from;
                        *dir = towards;
                    }
                    (
                        Piece::Arc { centre, radius, .. } | Piece::Circle { centre, radius },
                        Piece::Arc { centre: from, radius: size, .. } | Piece::Circle { centre: from, radius: size },
                    ) => {
                        *centre = from;
                        *radius = size;
                    }
                    _ => {}
                }
                paired += 1;
            }
            done.push(left);
            done.push(right);
        }
    }
    paired
}

/// Find the part's mirror planes and make the features either side of each one match exactly.
fn symmetrize(fitted: &mut [Vec<Vec<Piece>>], measured: &[(f64, f64, Vec<Vec<[f64; 2]>>)], o: &SlabOptions) -> Vec<Mirror> {
    if o.symmetry == Symmetry::Off {
        return Vec::new();
    }
    let points = section_points(measured);
    if points.len() < 50 {
        return Vec::new();
    }
    let tol = (2. * o.tolerance).max(0.4);
    let near = Near::build(points, tol.max(0.5));
    let mut out = Vec::new();
    for (axis, wanted) in [(0usize, Symmetry::AcrossX), (1, Symmetry::AcrossY)] {
        if !matches!(o.symmetry, Symmetry::Auto) && o.symmetry != wanted {
            continue;
        }
        let Some((at, mismatch)) = detect_mirror(&near, axis, tol) else { continue };
        let paired = pair_across(fitted, axis, at, (2. * o.tolerance).max(0.5));
        out.push(Mirror { across_x: axis == 0, at, mismatch, paired });
    }
    out
}

// ---------------------------------------------------------------------------------------------
// The datum, and rounding to it
// ---------------------------------------------------------------------------------------------

/// Where the part's own zero sits: on a mirror plane where there is one, because that is what the
/// dimensions on a drawing of it would be taken from, and at the part's own corner otherwise.
fn datum(mirrors: &[Mirror], measured: &[(f64, f64, Vec<Vec<[f64; 2]>>)]) -> [f64; 2] {
    let mut out = [0.; 2];
    for (axis, slot) in out.iter_mut().enumerate() {
        if let Some(mirror) = mirrors.iter().find(|m| m.across_x == (axis == 0)) {
            *slot = mirror.at;
            continue;
        }
        let mut lowest = f64::INFINITY;
        for (_, _, loops) in measured {
            for outline in loops {
                for p in outline {
                    lowest = lowest.min(p.get(axis).copied().unwrap_or(0.));
                }
            }
        }
        *slot = if lowest.is_finite() { lowest } else { 0. };
    }
    out
}

/// Round what was measured to the grid the part was made to.
///
/// This is the step that turns a reading into a dimension. A 59.94 mm edge becomes 60, a 5.97 mm
/// bore becomes 6, and the part that comes out is one someone can work with. Radii round to half
/// the grid so that a diameter, which is what a hole is called out by, lands on the grid itself. A
/// corner round keeps its measured centre: its position follows from the edges it joins, and
/// moving it would only break the tangency it was just fitted to hold.
fn round_features(fitted: &mut [Vec<Vec<Piece>>], grid: f64, datum: [f64; 2]) {
    if !(grid > 0.) {
        return;
    }
    let snap = |value: f64, from: f64| from + ((value - from) / grid).round() * grid;
    for at in paths(fitted) {
        let Some(piece) = piece_mut(fitted, at) else { continue };
        match piece {
            Piece::Line { at: on, dir, .. } => {
                let along = if dir[1].abs() < 1e-9 {
                    Some(1usize)
                } else if dir[0].abs() < 1e-9 {
                    Some(0)
                } else {
                    None
                };
                if let Some(coord) = along
                    && let (Some(slot), Some(from)) = (on.get_mut(coord), datum.get(coord))
                {
                    *slot = snap(*slot, *from);
                }
            }
            Piece::Arc { centre, radius, fillet, .. } => {
                if !*fillet {
                    *centre = [snap(centre[0], datum[0]), snap(centre[1], datum[1])];
                }
                *radius = (2. * *radius / grid).round() * grid / 2.;
                if !(*radius > 0.) {
                    *radius = grid / 2.;
                }
            }
            Piece::Circle { centre, radius } => {
                *centre = [snap(centre[0], datum[0]), snap(centre[1], datum[1])];
                *radius = (2. * *radius / grid).round() * grid / 2.;
                if !(*radius > 0.) {
                    *radius = grid / 2.;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Settling the ends, and the finished outline
// ---------------------------------------------------------------------------------------------

/// Where two fitted lines cross, or nothing when they run too nearly parallel to say.
fn cross_lines(a: &Piece, b: &Piece) -> Option<[f64; 2]> {
    let (Piece::Line { at: pa, dir: pd, .. }, Piece::Line { at: qa, dir: qd, .. }) = (a, b) else { return None };
    let det = qd[0] * pd[1] - pd[0] * qd[1];
    if !det.is_finite() || det.abs() < 1e-9 {
        return None;
    }
    let (rx, ry) = (qa[0] - pa[0], qa[1] - pa[1]);
    let s = (qd[0] * ry - rx * qd[1]) / det;
    let out = [pa[0] + pd[0] * s, pa[1] + pd[1] * s];
    out.iter().all(|v| v.is_finite()).then_some(out)
}

/// Where a fitted line meets a circle, taking the crossing nearer `near`. A line that misses the
/// circle by less than the scan's own noise is treated as touching it, at the point it comes
/// closest: on a real part a line that nearly touches a round was tangent to it.
fn cross_line_circle(line: &Piece, centre: [f64; 2], radius: f64, near: [f64; 2]) -> Option<[f64; 2]> {
    let Piece::Line { at, dir, .. } = line else { return None };
    let f = [at[0] - centre[0], at[1] - centre[1]];
    let along = f[0] * dir[0] + f[1] * dir[1];
    let foot = [at[0] - dir[0] * along, at[1] - dir[1] * along];
    let squared = along * along - (f[0] * f[0] + f[1] * f[1] - radius * radius);
    if !(squared > 0.) {
        return Some(foot);
    }
    let half = squared.sqrt();
    let candidates =
        [[at[0] + dir[0] * (-along + half), at[1] + dir[1] * (-along + half)], [at[0] + dir[0] * (-along - half), at[1] + dir[1] * (-along - half)]];
    candidates.into_iter().min_by(|a, b| distance(*a, near).total_cmp(&distance(*b, near)))
}

/// Where two circles meet, taking the crossing nearer `near`.
fn cross_circles(c1: [f64; 2], r1: f64, c2: [f64; 2], r2: f64, near: [f64; 2]) -> Option<[f64; 2]> {
    let between = [c2[0] - c1[0], c2[1] - c1[1]];
    let apart = (between[0] * between[0] + between[1] * between[1]).sqrt();
    if !(apart > 1e-9) {
        return None;
    }
    let along = (r1 * r1 - r2 * r2 + apart * apart) / (2. * apart);
    let base = [c1[0] + between[0] * (along / apart), c1[1] + between[1] * (along / apart)];
    let squared = r1 * r1 - along * along;
    if !(squared > 0.) {
        return Some(base);
    }
    let out = squared.sqrt();
    let across = [-between[1] / apart, between[0] / apart];
    let candidates = [[base[0] + across[0] * out, base[1] + across[1] * out], [base[0] - across[0] * out, base[1] - across[1] * out]];
    candidates.into_iter().min_by(|a, b| distance(*a, near).total_cmp(&distance(*b, near)))
}

/// The first and last points of the run a piece was fitted to.
fn measured_ends(piece: &Piece) -> Option<([f64; 2], [f64; 2])> {
    match piece {
        Piece::Line { a, b, .. } => Some((*a, *b)),
        Piece::Arc { points, .. } => Some((*points.first()?, *points.last()?)),
        Piece::Circle { .. } => None,
    }
}

/// Where two neighbouring pieces meet.
///
/// Two fits do not end at the same point, so the corner between them is where the geometry says
/// they cross. Where that crossing lands implausibly far from where the scan showed the corner —
/// two edges nearly parallel, a circle that misses, a fit that went wrong — the measured corner is
/// the safer answer, so the allowance grows with the pieces and never lets a bad fit throw a
/// vertex across the part.
fn junction(p: &Piece, q: &Piece) -> Option<[f64; 2]> {
    let (p0, p1) = measured_ends(p)?;
    let (q0, q1) = measured_ends(q)?;
    let near = [0.5 * (p1[0] + q0[0]), 0.5 * (p1[1] + q0[1])];
    let allowance = (JUNCTION_SHARE * distance(p0, p1).min(distance(q0, q1))).max(JUNCTION_FLOOR);
    let crossing = match (p, q) {
        (Piece::Line { .. }, Piece::Line { .. }) => cross_lines(p, q),
        (Piece::Line { .. }, Piece::Arc { centre, radius, .. }) => cross_line_circle(p, *centre, *radius, near),
        (Piece::Arc { centre, radius, .. }, Piece::Line { .. }) => cross_line_circle(q, *centre, *radius, near),
        (Piece::Arc { centre: a, radius: ra, .. }, Piece::Arc { centre: b, radius: rb, .. }) => cross_circles(*a, *ra, *b, *rb, near),
        _ => None,
    };
    Some(match crossing {
        Some(found) if distance(found, near) <= allowance => found,
        _ => near,
    })
}

/// How far a run of points turns about a centre, signed: positive counter-clockwise.
fn signed_turn(run: &[[f64; 2]], centre: [f64; 2]) -> f64 {
    let angle = |p: &[f64; 2]| (p[1] - centre[1]).atan2(p[0] - centre[0]);
    let Some(first) = run.first() else { return 0. };
    let mut previous = angle(first);
    let mut total = 0.;
    for p in run.iter().skip(1) {
        let here = angle(p);
        let mut step = here - previous;
        while step > std::f64::consts::PI {
            step -= std::f64::consts::TAU;
        }
        while step < -std::f64::consts::PI {
            step += std::f64::consts::TAU;
        }
        total += step;
        previous = here;
    }
    total
}

/// Turn the fitted pieces into one closed outline, measured from the part's own datum.
///
/// The pieces are fitted one run at a time and do not join up; this is where they are made to. A
/// corner round is re-centred between the edges it joins so it stays tangent to both, each
/// neighbouring pair is given the point where it crosses, and any line left with no length is
/// dropped. If what comes out does not close, or encloses an area the scan never showed, the
/// measured outline is kept instead, simplified and marked as unfitted: a rebuilt part that is
/// honestly a traced polygon is worth more than a tidy one in the wrong shape.
fn close_loop(pieces: &[Piece], points: &[[f64; 2]], tol: f64, datum: [f64; 2]) -> Loop {
    let measured_area = signed_area(points);
    let shift = |p: [f64; 2]| [p[0] - datum[0], p[1] - datum[1]];
    let traced = || {
        let simple: Vec<[f64; 2]> = simplify(points, tol);
        let parts = simple
            .iter()
            .enumerate()
            .filter_map(|(i, p)| Some(Part::Line { from: shift(*p), to: shift(*simple.get((i + 1) % simple.len())?) }))
            .collect();
        Loop { parts, area: measured_area, deviation: tol, fitted: false }
    };

    if let [Piece::Circle { centre, radius }] = pieces {
        let parts = vec![Part::Circle { centre: shift(*centre), radius: *radius }];
        let deviation = outline_deviation(&parts, points, datum);
        return Loop { parts, area: measured_area, deviation, fitted: true };
    }
    if pieces.len() < 3 {
        return traced();
    }

    // Hold every corner round tangent to the two edges it joins, now that those edges have been
    // squared up and lined up with each other.
    let mut settled = pieces.to_vec();
    let n = settled.len();
    for i in 0..n {
        let (Some(before), Some(after)) = (settled.get((i + n - 1) % n).cloned(), settled.get((i + 1) % n).cloned()) else { continue };
        if !is_corner(&before, &after) {
            continue;
        }
        let Some(Piece::Arc { centre, radius, fillet: true, .. }) = settled.get_mut(i) else { continue };
        if let Some(found) = fillet_centre(&before, &after, *radius, *centre)
            && distance(found, *centre) < 3. * *radius
        {
            *centre = found;
        }
    }

    // Corners, then the lines that were left with no length between two of them, then the corners
    // again. Three passes is enough for the handful of slivers a scan leaves.
    let corners = |pieces: &[Piece]| -> Vec<[f64; 2]> {
        (0..pieces.len())
            .map(|i| {
                let (Some(p), Some(q)) = (pieces.get(i), pieces.get((i + 1) % pieces.len())) else { return [0.; 2] };
                junction(p, q).unwrap_or([0.; 2])
            })
            .collect()
    };
    let mut vertices = corners(&settled);
    for _ in 0..3 {
        let n = settled.len();
        let slivers: Vec<usize> = (0..n)
            .filter(|i| {
                matches!(settled.get(*i), Some(Piece::Line { .. }))
                    && matches!((vertices.get(*i), vertices.get((i + n - 1) % n)), (Some(a), Some(b)) if distance(*a, *b) < 0.05)
            })
            .collect();
        if slivers.is_empty() || n - slivers.len() < 3 {
            break;
        }
        settled = settled.into_iter().enumerate().filter(|(i, _)| !slivers.contains(i)).map(|(_, p)| p).collect();
        vertices = corners(&settled);
    }

    let n = settled.len();
    let mut parts: Vec<Part> = Vec::new();
    for (i, piece) in settled.iter().enumerate() {
        let (Some(&to), Some(&from)) = (vertices.get(i), vertices.get((i + n - 1) % n)) else { continue };
        if distance(from, to) < 1e-6 {
            continue;
        }
        match piece {
            Piece::Line { .. } => parts.push(Part::Line { from: shift(from), to: shift(to) }),
            Piece::Arc { centre, radius, points: run, .. } => parts.push(Part::Arc {
                from: shift(from),
                to: shift(to),
                centre: shift(*centre),
                radius: *radius,
                ccw: signed_turn(run, *centre) >= 0.,
            }),
            Piece::Circle { centre, radius } => parts.push(Part::Circle { centre: shift(*centre), radius: *radius }),
        }
    }
    if parts.len() < 3 {
        return traced();
    }
    // A fitted outline that encloses a tenth more or less than the scan did is not this outline,
    // whatever it is.
    let built = built_area(&parts);
    if !(built.abs() - measured_area.abs()).abs().le(&(0.1 * measured_area.abs() + 1.)) {
        return traced();
    }
    let deviation = outline_deviation(&parts, points, datum);
    Loop { parts, area: measured_area, deviation, fitted: true }
}

/// The area a finished outline encloses: the polygon through its corners, with each arc's bulge
/// added back.
fn built_area(parts: &[Part]) -> f64 {
    let mut corners: Vec<[f64; 2]> = Vec::new();
    let mut bulge = 0.;
    for part in parts {
        match part {
            Part::Line { from, .. } => corners.push(*from),
            Part::Arc { from, to, centre, radius, ccw } => {
                corners.push(*from);
                let angle = |p: &[f64; 2]| (p[1] - centre[1]).atan2(p[0] - centre[0]);
                let mut sweep = angle(to) - angle(from);
                while sweep <= 0. {
                    sweep += std::f64::consts::TAU;
                }
                if !ccw {
                    sweep = std::f64::consts::TAU - sweep;
                }
                let segment = 0.5 * radius * radius * (sweep - sweep.sin());
                bulge += if *ccw { segment } else { -segment };
            }
            Part::Circle { radius, .. } => return std::f64::consts::PI * radius * radius,
        }
    }
    signed_area(&corners) + bulge
}

/// How far a finished outline sits from the points it was fitted to, at the 95th percentile.
fn outline_deviation(parts: &[Part], points: &[[f64; 2]], datum: [f64; 2]) -> f64 {
    let mut off: Vec<f64> = points
        .iter()
        .map(|p| {
            let here = [p[0] - datum[0], p[1] - datum[1]];
            parts.iter().map(|part| part_distance(here, part)).fold(f64::INFINITY, f64::min)
        })
        .filter(|d| d.is_finite())
        .collect();
    off.sort_by(f64::total_cmp);
    percentile(&off, 0.95)
}

/// How far a point lies off one piece of a finished outline.
fn part_distance(p: [f64; 2], part: &Part) -> f64 {
    match part {
        Part::Line { from, to } => {
            let along = [to[0] - from[0], to[1] - from[1]];
            let length = (along[0] * along[0] + along[1] * along[1]).sqrt();
            if !(length > 1e-12) {
                return distance(p, *from);
            }
            let t = (((p[0] - from[0]) * along[0] + (p[1] - from[1]) * along[1]) / (length * length)).clamp(0., 1.);
            distance(p, [from[0] + along[0] * t, from[1] + along[1] * t])
        }
        Part::Circle { centre, radius } => (distance(p, *centre) - radius).abs(),
        Part::Arc { from, to, centre, radius, ccw } => {
            let angle = |q: &[f64; 2]| (q[1] - centre[1]).atan2(q[0] - centre[0]);
            let turn = |a: f64| {
                let mut d = a;
                while d < 0. {
                    d += std::f64::consts::TAU;
                }
                while d >= std::f64::consts::TAU {
                    d -= std::f64::consts::TAU;
                }
                d
            };
            let (start, end) = if *ccw { (angle(from), angle(to)) } else { (angle(to), angle(from)) };
            let span = turn(end - start);
            if turn(angle(&p) - start) <= span { (distance(p, *centre) - radius).abs() } else { distance(p, *from).min(distance(p, *to)) }
        }
    }
}

/// Drop the points of a measured outline that say nothing: the ones within `tol` of the line
/// between the points either side of them.
fn simplify(points: &[[f64; 2]], tol: f64) -> Vec<[f64; 2]> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    if let Some(first) = keep.first_mut() {
        *first = true;
    }
    if let Some(last) = keep.last_mut() {
        *last = true;
    }
    let mut todo = vec![(0usize, points.len() - 1)];
    while let Some((from, to)) = todo.pop() {
        if to <= from + 1 {
            continue;
        }
        let Some(run) = points.get(from..=to) else { continue };
        let Some(at) = furthest(run, 1) else { continue };
        let (Some(a), Some(b), Some(p)) = (points.get(from), points.get(to), run.get(at)) else { continue };
        let chord = [b[0] - a[0], b[1] - a[1]];
        let length = (chord[0] * chord[0] + chord[1] * chord[1]).sqrt();
        let off = if length > 1e-12 { ((p[0] - a[0]) * -chord[1] + (p[1] - a[1]) * chord[0]).abs() / length } else { distance(*p, *a) };
        if off > tol {
            if let Some(slot) = keep.get_mut(from + at) {
                *slot = true;
            }
            todo.push((from, from + at));
            todo.push((from + at, to));
        }
    }
    points.iter().zip(&keep).filter_map(|(p, keep)| keep.then_some(*p)).collect()
}

// ---------------------------------------------------------------------------------------------
// How close the plan came
// ---------------------------------------------------------------------------------------------

/// How far the finished plan sits from the scan it was read from.
///
/// This is the number that says whether the part really was a stack of layers. A plate rebuilds to
/// within its own scanning noise; a curved shell forced into slabs comes back millimetres out, and
/// the caller can see that in the report rather than in the finished model.
///
/// It is an approximation: each scan point is measured against the plan's own faces, flattened,
/// rather than against a built body. That is close enough to tell those two cases apart, which is
/// what it is for.
fn deviation(local: &[[f64; 3]], datum: [f64; 2], base: f64, slabs: &[Slab]) -> Option<Deviation> {
    if slabs.is_empty() || local.is_empty() {
        return None;
    }
    let walls: Vec<Vec<Vec<[f64; 2]>>> = slabs.iter().map(|slab| slab.loops.iter().map(flatten).collect()).collect();
    let stride = local.len().div_ceil(MAX_PROBES).max(1);
    let mut off: Vec<f64> = Vec::new();
    let mut at = 0usize;
    while at < local.len() {
        let Some(p) = local.get(at) else { break };
        at += stride;
        let here = [p[0] - datum[0], p[1] - datum[1]];
        let z = p[2] - base;
        let mut best = f64::INFINITY;
        for (slab, loops) in slabs.iter().zip(&walls) {
            let mut side = f64::INFINITY;
            let mut crossings = 0usize;
            for outline in loops {
                side = side.min(polyline_distance(here, outline));
                crossings += polyline_crossings(here, outline);
            }
            let inside = crossings % 2 == 1;
            let above = z - slab.to;
            let below = slab.from - z;
            let out = above.max(below);
            best = best.min(if out <= 0. {
                if inside { side.min(-above).min(-below) } else { side }
            } else if inside {
                out
            } else {
                (side * side + out * out).sqrt()
            });
        }
        if best.is_finite() {
            off.push(best);
        }
    }
    if off.is_empty() {
        return None;
    }
    let probes = off.len();
    let mean = off.iter().sum::<f64>() / probes as f64;
    off.sort_by(f64::total_cmp);
    Some(Deviation { mean, p95: percentile(&off, 0.95), max: off.last().copied().unwrap_or(0.), probes })
}

/// A finished outline as a closed polyline, for measuring against.
fn flatten(outline: &Loop) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    for part in &outline.parts {
        match part {
            Part::Line { from, .. } => out.push(*from),
            Part::Arc { from, to, centre, radius, ccw } => {
                let angle = |p: &[f64; 2]| (p[1] - centre[1]).atan2(p[0] - centre[0]);
                let start = angle(from);
                let mut sweep = angle(to) - start;
                while sweep <= 0. {
                    sweep += std::f64::consts::TAU;
                }
                if !ccw {
                    sweep -= std::f64::consts::TAU;
                }
                for step in 0..16 {
                    let a = start + sweep * f64::from(step) / 16.;
                    out.push([centre[0] + radius * a.cos(), centre[1] + radius * a.sin()]);
                }
            }
            Part::Circle { centre, radius } => {
                for step in 0..32 {
                    let a = std::f64::consts::TAU * f64::from(step) / 32.;
                    out.push([centre[0] + radius * a.cos(), centre[1] + radius * a.sin()]);
                }
            }
        }
    }
    out
}

fn polyline_distance(p: [f64; 2], outline: &[[f64; 2]]) -> f64 {
    let mut best = f64::INFINITY;
    for (i, a) in outline.iter().enumerate() {
        let Some(b) = outline.get((i + 1) % outline.len()) else { continue };
        best = best.min(part_distance(p, &Part::Line { from: *a, to: *b }));
    }
    best
}

/// How many times a ray along +x from the point crosses the outline: odd means inside it.
fn polyline_crossings(p: [f64; 2], outline: &[[f64; 2]]) -> usize {
    let mut count = 0;
    for (i, a) in outline.iter().enumerate() {
        let Some(b) = outline.get((i + 1) % outline.len()) else { continue };
        if (a[1] > p[1]) == (b[1] > p[1]) {
            continue;
        }
        let span = b[1] - a[1];
        if span.abs() < 1e-12 {
            continue;
        }
        if p[0] < a[0] + (p[1] - a[1]) / span * (b[0] - a[0]) {
            count += 1;
        }
    }
    count
}

#[cfg(test)]
#[path = "mesh_slabs_tests.rs"]
mod tests;
