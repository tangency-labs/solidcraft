//! The shapes of what the tool and the machine must keep out of, beyond a box: prisms along the A
//! axis (a chuck, a ring that turns with A, a jaw block, a cylinder on a rotary unit) and
//! triangle meshes (a fixture or a unit given as an STL or STEP file). Each is checked exactly
//! against a zone, an upright prism above the tip (a circle or a rectangle seen from above, and
//! the heights it spans: [`crate::machine_model::Zone`]).
//!
//! A prism along X is held as its section across the axis (Y and Z): the part of a zone between
//! the prism's two X ends, seen along X, is a rectangle, and the prism is hit when that rectangle
//! meets the section. A prism that turns with A is held as it is at A = 0 and turned by the A
//! angle at each place checked ([`Turn`]); where the angle is not known it is checked as the
//! ring it sweeps. A mesh is checked triangle by triangle, each cut to the zone's heights and
//! held against its outline, through a grid of the triangles seen from above.

use std::sync::Arc;

use solidcore_record::Vec3;
use solidcraft_geom::Mesh;
use solidcraft_geom::prelude::*;

use crate::sim::Footprint;
use crate::{CamError, Result};

/// Most triangles of every checked mesh together.
pub const MAX_CHECKED_TRIANGLES: usize = 2_000_000;
/// Most cells of a mesh's grid.
const MAX_CELLS: usize = 1 << 16;
/// Most triangle entries in the grids of a scene's meshes together (a triangle is listed in
/// every cell it covers).
pub const MAX_ENTRIES: usize = 24_000_000;
/// Largest coordinate accepted (mm).
const MAX_LEN: f64 = 1.0e5;

/// How the things fixed to the work stand against the bed: turned by `angle` (radians, about
/// +X) about the line through `center` (bed Y and Z) along X.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Turn {
    pub angle: f64,
    pub center: [f64; 2],
}

impl Turn {
    /// `p` (Y, Z) turned.
    pub fn apply(&self, p: [f64; 2]) -> [f64; 2] {
        let (s, c) = self.angle.sin_cos();
        let (y, z) = (p[0] - self.center[0], p[1] - self.center[1]);
        [self.center[0] + c * y - s * z, self.center[1] + s * y + c * z]
    }
}

/// A section across the A axis (Y and Z, work coordinates with the rotaries at zero).
#[derive(Clone, Debug, PartialEq)]
pub enum Section {
    /// A disk of radius `r` about `c`.
    Disk { c: [f64; 2], r: f64 },
    /// A ring about `c`, from radius `r0` to `r1`.
    Ring { c: [f64; 2], r0: f64, r1: f64 },
    /// A convex polygon, its corners in order.
    Polygon(Vec<[f64; 2]>),
}

impl Section {
    /// Turned by `t`.
    pub fn turned(&self, t: &Turn) -> Section {
        match self {
            Section::Disk { c, r } => Section::Disk { c: t.apply(*c), r: *r },
            Section::Ring { c, r0, r1 } => Section::Ring { c: t.apply(*c), r0: *r0, r1: *r1 },
            Section::Polygon(p) => Section::Polygon(p.iter().map(|q| t.apply(*q)).collect()),
        }
    }

    /// The ring it sweeps turning about `axis` through any angle.
    pub fn swept(&self, axis: [f64; 2]) -> Section {
        let d = |p: [f64; 2]| (p[0] - axis[0]).hypot(p[1] - axis[1]);
        match self {
            Section::Disk { c, r } => {
                let off = d(*c);
                if off < 1e-9 { self.clone() } else { Section::Ring { c: axis, r0: (off - r).max(0.0), r1: off + r } }
            }
            Section::Ring { c, r0, r1 } => {
                let off = d(*c);
                Section::Ring { c: axis, r0: (r0 - off).max(0.0), r1: off + r1 }
            }
            Section::Polygon(p) => {
                let r1 = p.iter().map(|q| d(*q)).fold(0.0, f64::max);
                let r0 = if contains(p, axis) { 0.0 } else { edges(p).map(|(a, b)| seg_dist(axis, a, b)).fold(f64::INFINITY, f64::min) };
                Section::Ring { c: axis, r0: if r0.is_finite() { r0 } else { 0.0 }, r1 }
            }
        }
    }

    /// Its bounds (Y, Z): low corner and high corner.
    pub fn bounds(&self) -> ([f64; 2], [f64; 2]) {
        match self {
            Section::Disk { c, r } | Section::Ring { c, r1: r, .. } => ([c[0] - r, c[1] - r], [c[0] + r, c[1] + r]),
            Section::Polygon(p) => {
                let mut lo = [f64::INFINITY; 2];
                let mut hi = [f64::NEG_INFINITY; 2];
                for q in p {
                    for k in 0..2 {
                        lo[k] = lo[k].min(q[k]);
                        hi[k] = hi[k].max(q[k]);
                    }
                }
                (lo, hi)
            }
        }
    }

    /// The prism from `x[0]` to `x[1]` as triangles, for drawing (a disk and a ring as 48-sided).
    pub fn prism(&self, x: [f64; 2]) -> Mesh {
        const SIDES: u32 = 48;
        let circle = |c: [f64; 2], r: f64| -> Vec<[f64; 2]> {
            (0..SIDES)
                .map(|k| {
                    let (s, co) = (f64::from(k) * std::f64::consts::TAU / f64::from(SIDES)).sin_cos();
                    [c[0] + r * co, c[1] + r * s]
                })
                .collect()
        };
        let (outer, inner) = match self {
            Section::Disk { c, r } => (circle(*c, *r), None),
            Section::Ring { c, r0, r1 } if *r0 > 1e-9 => (circle(*c, *r1), Some(circle(*c, *r0))),
            Section::Ring { c, r1, .. } => (circle(*c, *r1), None),
            Section::Polygon(p) => (p.clone(), None),
        };
        let mut m = Mesh::default();
        let ring = |pts: &[[f64; 2]], m: &mut Mesh| -> u32 {
            let base = m.positions.len() as u32;
            for xx in x {
                for q in pts {
                    m.positions.push(Vec3::new(xx, q[0], q[1]));
                }
            }
            let n = pts.len() as u32;
            for k in 0..n {
                let (a, b) = (base + k, base + (k + 1) % n);
                m.triangles.push([a, b, b + n]);
                m.triangles.push([a, b + n, a + n]);
            }
            base
        };
        let n = outer.len() as u32;
        if n < 2 {
            return m;
        }
        let ob = ring(&outer, &mut m);
        match &inner {
            None => {
                for k in 1..n.saturating_sub(1) {
                    m.triangles.push([ob, ob + k + 1, ob + k]);
                    m.triangles.push([ob + n, ob + n + k, ob + n + k + 1]);
                }
            }
            Some(inner) => {
                let ib = ring(inner, &mut m);
                for k in 0..n {
                    let k1 = (k + 1) % n;
                    for end in [0, n] {
                        m.triangles.push([ob + end + k, ib + end + k, ib + end + k1]);
                        m.triangles.push([ob + end + k, ib + end + k1, ob + end + k1]);
                    }
                }
            }
        }
        m
    }

    /// Whether it meets the rectangle `lo`..`hi` (Y, Z), by more than touching.
    pub fn meets_rect(&self, lo: [f64; 2], hi: [f64; 2]) -> bool {
        if !(lo[0] < hi[0] && lo[1] < hi[1]) {
            return false;
        }
        match self {
            Section::Disk { c, r } => rect_dist(*c, lo, hi) < *r,
            Section::Ring { c, r0, r1 } => rect_dist(*c, lo, hi) < *r1 && rect_far(*c, lo, hi) > *r0,
            Section::Polygon(p) => {
                let rect = [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]];
                convex_overlap(p, &rect)
            }
        }
    }
}

fn edges(p: &[[f64; 2]]) -> impl Iterator<Item = ([f64; 2], [f64; 2])> + '_ {
    let n = p.len();
    (0..n).filter_map(move |i| Some((*p.get(i)?, *p.get((i + 1) % n)?)))
}

/// Whether the convex polygon `p` holds `q` (on its edge counts).
fn contains(p: &[[f64; 2]], q: [f64; 2]) -> bool {
    if p.len() < 3 {
        return false;
    }
    let mut sign = 0.0f64;
    for (a, b) in edges(p) {
        let cross = (b[0] - a[0]) * (q[1] - a[1]) - (b[1] - a[1]) * (q[0] - a[0]);
        if cross.abs() < 1e-12 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

fn seg_dist(q: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    (q[0] - a[0] - t * dx).hypot(q[1] - a[1] - t * dy)
}

/// Distance from `c` to the rectangle (0 inside).
fn rect_dist(c: [f64; 2], lo: [f64; 2], hi: [f64; 2]) -> f64 {
    let dx = (lo[0] - c[0]).max(0.0).max(c[0] - hi[0]);
    let dy = (lo[1] - c[1]).max(0.0).max(c[1] - hi[1]);
    dx.hypot(dy)
}

/// Distance from `c` to the rectangle's farthest corner.
fn rect_far(c: [f64; 2], lo: [f64; 2], hi: [f64; 2]) -> f64 {
    let dx = (c[0] - lo[0]).abs().max((hi[0] - c[0]).abs());
    let dy = (c[1] - lo[1]).abs().max((hi[1] - c[1]).abs());
    dx.hypot(dy)
}

/// Whether two convex polygons (a polygon of fewer than three corners is a segment or a point)
/// overlap by more than touching: no separating axis among their edges' normals.
fn convex_overlap(a: &[[f64; 2]], b: &[[f64; 2]]) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let mut axes: Vec<[f64; 2]> = Vec::with_capacity(a.len() + b.len() + 2);
    for poly in [a, b] {
        if poly.len() == 1 {
            continue;
        }
        for (p, q) in edges(poly) {
            let n = [q[1] - p[1], p[0] - q[0]];
            let len = n[0].hypot(n[1]);
            if len > 1e-12 {
                axes.push([n[0] / len, n[1] / len]);
            }
        }
    }
    // Two points, or a point and a segment along its line: the axis between them.
    axes.push([1.0, 0.0]);
    axes.push([0.0, 1.0]);
    let span = |poly: &[[f64; 2]], n: [f64; 2]| {
        poly.iter().map(|p| p[0] * n[0] + p[1] * n[1]).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), d| (lo.min(d), hi.max(d)))
    };
    for n in axes {
        let (a0, a1) = span(a, n);
        let (b0, b1) = span(b, n);
        if a1 <= b0 + 1e-12 || b1 <= a0 + 1e-12 {
            return false;
        }
    }
    true
}

/// The rectangle (Y, Z) a zone covers between `x[0]` and `x[1]`, seen along X, shrunk by `tol`;
/// `None` when the zone does not reach between them.
pub fn zone_across(fp: Footprint, z0: f64, z1: f64, x: [f64; 2], tol: f64) -> Option<([f64; 2], [f64; 2])> {
    let (y0, y1) = match fp {
        Footprint::Circle { x: cx, y, r } => {
            let dx = (x[0] - cx).max(0.0).max(cx - x[1]);
            if dx >= r - tol {
                return None;
            }
            let w = ((r - tol).powi(2) - dx * dx).max(0.0).sqrt();
            (y - w, y + w)
        }
        Footprint::Rect { x0, x1, y0, y1 } => {
            if x1 <= x[0] + tol || x0 >= x[1] - tol {
                return None;
            }
            (y0 + tol, y1 - tol)
        }
    };
    let (lo, hi) = ([y0, z0 + tol], [y1, z1 - tol]);
    (lo[0] < hi[0] && lo[1] < hi[1]).then_some((lo, hi))
}

/// A triangle mesh to keep out of, in work coordinates with the rotaries at zero, with a grid of
/// its triangles seen from above.
#[derive(Debug)]
pub struct MeshGrid {
    tris: Vec<[Vec3; 3]>,
    pub min: Vec3,
    pub max: Vec3,
    cell: f64,
    nx: usize,
    ny: usize,
    /// For each cell: where its list starts in `entries`, and its lowest and highest Z.
    starts: Vec<u32>,
    zr: Vec<[f64; 2]>,
    entries: Vec<u32>,
}

/// A mesh obstacle, compared by identity.
#[derive(Clone, Debug)]
pub struct MeshShape(pub Arc<MeshGrid>);

impl PartialEq for MeshShape {
    fn eq(&self, other: &MeshShape) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl MeshGrid {
    /// The grid of `mesh`'s triangles moved by `shift`. Refused: no triangles, a coordinate that
    /// is not finite or beyond 100 m, more than [`MAX_CHECKED_TRIANGLES`].
    pub fn new(mesh: &Mesh, shift: Vec3) -> Result<MeshGrid> {
        MeshGrid::within(mesh, shift, MAX_CHECKED_TRIANGLES, MAX_ENTRIES)
    }

    /// [`MeshGrid::new`] within what is left of a scene's budget: at most `max_triangles`
    /// triangles and `max_entries` grid entries.
    pub fn within(mesh: &Mesh, shift: Vec3, max_triangles: usize, max_entries: usize) -> Result<MeshGrid> {
        let max_triangles = max_triangles.min(MAX_CHECKED_TRIANGLES);
        let max_entries = max_entries.min(MAX_ENTRIES);
        if mesh.triangles.len() > max_triangles {
            return Err(CamError::TooLarge(format!("the checked meshes have at most {MAX_CHECKED_TRIANGLES} triangles together")));
        }
        let mut tris = Vec::with_capacity(mesh.triangles.len());
        let mut min = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut max = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for t in &mesh.triangles {
            let mut tri = [Vec3::ZERO; 3];
            for (k, i) in t.iter().enumerate() {
                let Some(p) = mesh.positions.get(*i as usize) else { return crate::invalid("a mesh triangle names a corner it does not have") };
                if !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite() && p.x.abs().max(p.y.abs()).max(p.z.abs()) <= MAX_LEN) {
                    return crate::invalid("a mesh's coordinates must be finite, within 100 m");
                }
                let q = *p + shift;
                tri[k] = q;
                min = min.min(q);
                max = max.max(q);
            }
            tris.push(tri);
        }
        if tris.is_empty() {
            return crate::invalid("a checked mesh has no triangles");
        }
        let (w, d) = ((max.x - min.x).max(1e-6), (max.y - min.y).max(1e-6));
        // About four triangles a cell, at most MAX_CELLS cells.
        let cell = (w * d / (tris.len() as f64 / 4.0).max(1.0)).sqrt().max((w * d / MAX_CELLS as f64).sqrt()).max(0.25);
        let nx = ((w / cell).ceil() as usize).clamp(1, MAX_CELLS);
        let ny = ((d / cell).ceil() as usize).clamp(1, (MAX_CELLS / nx).max(1));
        let cell = cell.max(w / nx as f64).max(d / ny as f64);
        let index = |v: f64, lo: f64, n: usize| (((v - lo) / cell).floor().max(0.0) as usize).min(n - 1);
        let mut counts = vec![0u32; nx * ny + 1];
        let mut zr = vec![[f64::INFINITY, f64::NEG_INFINITY]; nx * ny];
        let mut total = 0usize;
        let span = |t: &[Vec3; 3]| {
            let (x0, x1) = (t[0].x.min(t[1].x).min(t[2].x), t[0].x.max(t[1].x).max(t[2].x));
            let (y0, y1) = (t[0].y.min(t[1].y).min(t[2].y), t[0].y.max(t[1].y).max(t[2].y));
            (index(x0, min.x, nx), index(x1, min.x, nx), index(y0, min.y, ny), index(y1, min.y, ny))
        };
        for t in &tris {
            let (i0, i1, j0, j1) = span(t);
            total = total.saturating_add((i1 - i0 + 1) * (j1 - j0 + 1));
            if total > max_entries {
                return Err(CamError::TooLarge("the checked meshes have too many large triangles together".into()));
            }
            let (z0, z1) = (t[0].z.min(t[1].z).min(t[2].z), t[0].z.max(t[1].z).max(t[2].z));
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let c = j * nx + i;
                    if let (Some(n), Some(z)) = (counts.get_mut(c + 1), zr.get_mut(c)) {
                        *n += 1;
                        z[0] = z[0].min(z0);
                        z[1] = z[1].max(z1);
                    }
                }
            }
        }
        for c in 1..counts.len() {
            let before = counts.get(c - 1).copied().unwrap_or(0);
            if let Some(n) = counts.get_mut(c) {
                *n = n.saturating_add(before);
            }
        }
        let starts = counts.clone();
        let mut fill = counts;
        let mut entries = vec![0u32; total];
        for (k, t) in tris.iter().enumerate() {
            let (i0, i1, j0, j1) = span(t);
            let id = u32::try_from(k).map_err(|_| CamError::TooLarge("a checked mesh is too large".into()))?;
            for j in j0..=j1 {
                for i in i0..=i1 {
                    if let Some(at) = fill.get_mut(j * nx + i) {
                        if let Some(e) = entries.get_mut(*at as usize) {
                            *e = id;
                        }
                        *at += 1;
                    }
                }
            }
        }
        Ok(MeshGrid { tris, min, max, cell, nx, ny, starts, zr, entries })
    }

    pub fn triangles(&self) -> usize {
        self.tris.len()
    }

    /// Its grid's triangle entries.
    pub fn entries(&self) -> usize {
        self.entries.len()
    }

    /// Whether a zone (an outline seen from above, from `z0` to `z1`) meets a triangle by more
    /// than `tol`.
    pub fn meets(&self, fp: Footprint, z0: f64, z1: f64, tol: f64) -> bool {
        let (z0, z1) = (z0 + tol, z1 - tol);
        if z0 >= z1 || z0 >= self.max.z || z1 <= self.min.z {
            return false;
        }
        let (fx0, fx1, fy0, fy1) = fp.bounds();
        if fx1 <= self.min.x || fx0 >= self.max.x || fy1 <= self.min.y || fy0 >= self.max.y {
            return false;
        }
        let index = |v: f64, lo: f64, n: usize| (((v - lo) / self.cell).floor().max(0.0) as usize).min(n - 1);
        let (i0, i1) = (index(fx0, self.min.x, self.nx), index(fx1, self.min.x, self.nx));
        let (j0, j1) = (index(fy0, self.min.y, self.ny), index(fy1, self.min.y, self.ny));
        for j in j0..=j1 {
            for i in i0..=i1 {
                let c = j * self.nx + i;
                let Some(z) = self.zr.get(c) else { continue };
                if z[0] >= z1 || z[1] <= z0 {
                    continue;
                }
                let (Some(&s), Some(&e)) = (self.starts.get(c), self.starts.get(c + 1)) else { continue };
                for id in self.entries.get(s as usize..e as usize).unwrap_or(&[]) {
                    if let Some(t) = self.tris.get(*id as usize)
                        && tri_meets(t, fp, z0, z1, tol)
                    {
                        return true;
                    }
                }
            }
        }
        // No triangle crosses the zone: it is all inside the mesh or all outside. A point of it
        // tells which.
        let (cx, cy) = match fp {
            Footprint::Circle { x, y, .. } => (x, y),
            Footprint::Rect { x0, x1, y0, y1 } => ((x0 + x1) * 0.5, (y0 + y1) * 0.5),
        };
        self.holds(Vec3::new(cx, cy, (z0 + z1) * 0.5))
    }

    /// Whether `p` is inside the mesh: a ray straight up from it crosses its triangles an odd
    /// number of times (moved off by a hair, so it runs along no edge).
    pub fn holds(&self, p: Vec3) -> bool {
        let (x, y) = (p.x + 1.234_567e-7, p.y + 2.345_678e-7);
        if x <= self.min.x || x >= self.max.x || y <= self.min.y || y >= self.max.y || p.z >= self.max.z || p.z <= self.min.z {
            return false;
        }
        let index = |v: f64, lo: f64, n: usize| (((v - lo) / self.cell).floor().max(0.0) as usize).min(n - 1);
        let c = index(y, self.min.y, self.ny) * self.nx + index(x, self.min.x, self.nx);
        let (Some(&s), Some(&e)) = (self.starts.get(c), self.starts.get(c + 1)) else { return false };
        let mut crossings = 0usize;
        for id in self.entries.get(s as usize..e as usize).unwrap_or(&[]) {
            let Some([a, b, t]) = self.tris.get(*id as usize) else { continue };
            let d = (b.x - a.x) * (t.y - a.y) - (t.x - a.x) * (b.y - a.y);
            if d.abs() < 1e-12 {
                continue;
            }
            let u = ((x - a.x) * (t.y - a.y) - (t.x - a.x) * (y - a.y)) / d;
            let v = ((b.x - a.x) * (y - a.y) - (x - a.x) * (b.y - a.y)) / d;
            if u >= 0.0 && v >= 0.0 && u + v <= 1.0 && a.z + u * (b.z - a.z) + v * (t.z - a.z) > p.z {
                crossings += 1;
            }
        }
        crossings % 2 == 1
    }
}

/// The triangle cut to `z0`..`z1`, seen from above, against the outline `fp` (shrunk by `tol`).
fn tri_meets(t: &[Vec3; 3], fp: Footprint, z0: f64, z1: f64, tol: f64) -> bool {
    let mut poly: Vec<Vec3> = t.to_vec();
    for (keep_above, z) in [(true, z0), (false, z1)] {
        let inside = |p: &Vec3| if keep_above { p.z >= z } else { p.z <= z };
        let mut out = Vec::with_capacity(poly.len() + 2);
        let n = poly.len();
        for k in 0..n {
            let (Some(&a), Some(&b)) = (poly.get(k), poly.get((k + 1) % n)) else { continue };
            let (ia, ib) = (inside(&a), inside(&b));
            if ia {
                out.push(a);
            }
            if ia != ib {
                let f = (z - a.z) / (b.z - a.z);
                out.push(a.lerp(b, f));
            }
        }
        poly = out;
        if poly.is_empty() {
            return false;
        }
    }
    let flat: Vec<[f64; 2]> = poly.iter().map(|p| [p.x, p.y]).collect();
    match fp {
        Footprint::Circle { x, y, r } => {
            let r = r - tol;
            if r <= 0.0 {
                return false;
            }
            let c = [x, y];
            if flat.len() >= 3 && contains(&flat, c) && !degenerate(&flat) {
                return true;
            }
            match flat.as_slice() {
                [p] => (p[0] - x).hypot(p[1] - y) < r,
                _ => edges(&flat).any(|(a, b)| seg_dist(c, a, b) < r),
            }
        }
        Footprint::Rect { x0, x1, y0, y1 } => {
            let rect = [[x0 + tol, y0 + tol], [x1 - tol, y0 + tol], [x1 - tol, y1 - tol], [x0 + tol, y1 - tol]];
            if x0 + tol >= x1 - tol || y0 + tol >= y1 - tol {
                return false;
            }
            convex_overlap(&flat, &rect)
        }
    }
}

/// Whether a polygon has no area (a triangle seen edge-on from above).
fn degenerate(p: &[[f64; 2]]) -> bool {
    let mut area = 0.0;
    for (a, b) in edges(p) {
        area += a[0] * b[1] - b[0] * a[1];
    }
    area.abs() < 1e-12
}

/// What an obstacle is, beyond its box.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum ObstacleShape {
    /// The box itself.
    #[default]
    Box,
    /// A prism along X from `x[0]` to `x[1]` of `section`; with `turns`, fixed to the work (it
    /// turns with A about `axis`, Y and Z, the A axis in work coordinates).
    AlongX { x: [f64; 2], section: Section, turns: bool, axis: [f64; 2] },
    /// Triangles.
    Mesh(MeshShape),
}

impl ObstacleShape {
    /// Whether a zone already found inside the obstacle's box meets the shape, the work turned by
    /// `turn` (`None`: not known, a turning prism is held as the ring it sweeps).
    pub fn meets(&self, fp: Footprint, z0: f64, z1: f64, turn: Option<&Turn>, tol: f64) -> bool {
        match self {
            ObstacleShape::Box => true,
            ObstacleShape::AlongX { x, section, turns, axis } => {
                let Some((lo, hi)) = zone_across(fp, z0, z1, *x, tol) else { return false };
                if !*turns {
                    return section.meets_rect(lo, hi);
                }
                match turn {
                    Some(t) => section.turned(t).meets_rect(lo, hi),
                    None => section.swept(*axis).meets_rect(lo, hi),
                }
            }
            ObstacleShape::Mesh(m) => m.0.meets(fp, z0, z1, tol),
        }
    }
}
