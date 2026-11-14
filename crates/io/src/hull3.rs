//! Simple shapes around a mesh's points for robot collision (URDF export): a convex hull and
//! fitted primitives.

use std::collections::{BTreeMap, BTreeSet};

use solidcore_record::Vec3;
use solidcraft_geom::inertia::principal;
use solidcraft_geom::{Mesh, MeshGeom, Vec3Geom};

/// The most points [`convex_hull`] takes.
pub const MAX_HULL_POINTS: usize = 2_000_000;
/// Distances this small, as a share of the points' extent, count as zero.
const FLAT: f64 = 1e-9;
/// Principal moments this close count as equal (`inertia::principal`).
const EQUAL_MOMENTS: f64 = 1e-3;

struct Face {
    v: [usize; 3],
    normal: Vec3,
    /// The face across edge `k` (`v[k]` to `v[k + 1]`).
    next: [usize; 3],
    /// Points outside this face, each held by one face.
    outside: Vec<usize>,
    alive: bool,
}

impl Face {
    /// Its normal follows the order of `v` (counter-clockwise seen from outside).
    fn new(points: &[Vec3], v: [usize; 3]) -> Option<Face> {
        let at = |i: usize| points.get(i).copied();
        let (a, b, c) = (at(v[0])?, at(v[1])?, at(v[2])?);
        let normal = (b - a).cross(c - a).normalized()?;
        Some(Face { v, normal, next: [usize::MAX; 3], outside: Vec::new(), alive: true })
    }

    fn distance(&self, points: &[Vec3], p: Vec3) -> f64 {
        points.get(self.v[0]).map_or(f64::NEG_INFINITY, |a| self.normal.dot(p - *a))
    }

    /// Which of its edges runs from `p` to `q`.
    fn edge(&self, p: usize, q: usize) -> Option<usize> {
        (0..3).find(|&k| self.v.get(k) == Some(&p) && self.v.get((k + 1) % 3) == Some(&q))
    }
}

/// The convex hull of `points` as a closed mesh, outward faces counter-clockwise (quickhull).
/// `None` for non-finite input, too many points, or points that don't span a volume. Each round
/// looks only at the faces the new point sees, found by walking from face to face, so a dense
/// mesh is quick.
pub fn convex_hull(points: &[Vec3]) -> Option<Mesh> {
    if points.len() < 4 || points.len() > MAX_HULL_POINTS || !points.iter().all(|p| finite(*p)) {
        return None;
    }
    let (lo, hi) = extent(points)?;
    let d = hi - lo;
    let eps = d.dot(d).sqrt() * FLAT;
    if eps <= 0.0 {
        return None;
    }
    let [a, b, c, e] = first_tetrahedron(points, eps)?;
    let at = |i: usize| points.get(i).copied();
    let inside = (at(a)? + at(b)? + at(c)? + at(e)?) * 0.25;
    let mut faces: Vec<Face> = Vec::new();
    for v in [[a, b, c], [a, b, e], [a, c, e], [b, c, e]] {
        let mut f = Face::new(points, v)?;
        if f.distance(points, inside) > 0.0 {
            f = Face::new(points, [v[0], v[2], v[1]])?;
        }
        faces.push(f);
    }
    // Link the four faces: the face across p→q is the one with q→p.
    let edges: BTreeMap<(usize, usize), usize> =
        faces.iter().enumerate().flat_map(|(i, f)| (0..3).map(move |k| ((f.v[k], f.v[(k + 1) % 3]), i))).collect();
    for f in &mut faces {
        for k in 0..3 {
            f.next[k] = *edges.get(&(f.v[(k + 1) % 3], f.v[k]))?;
        }
    }
    assign(points, &mut faces, 0..4, 0..points.len(), eps);
    // Ordered, so the same points always give the same faces and the same file.
    let mut waiting: BTreeSet<usize> = (0..4).filter(|&i| faces.get(i).is_some_and(|f| !f.outside.is_empty())).collect();
    // Each round adds a point to the hull for good, so it can't take more rounds than points.
    for _ in 0..points.len() {
        let Some(from) = waiting.pop_first() else { break };
        let face = faces.get(from)?;
        if !face.alive || face.outside.is_empty() {
            continue;
        }
        let top = *face.outside.iter().max_by(|x, y| {
            let dist = |i: usize| points.get(i).map_or(f64::NEG_INFINITY, |p| face.distance(points, *p));
            dist(**x).total_cmp(&dist(**y))
        })?;
        let apex = at(top)?;
        // The faces the apex sees, walking out from the first one.
        let mut seen = BTreeSet::from([from]);
        let mut walk = vec![from];
        let mut horizon = Vec::new();
        while let Some(i) = walk.pop() {
            let f = faces.get(i)?;
            for k in 0..3 {
                let n = f.next[k];
                if seen.contains(&n) {
                    continue;
                }
                if faces.get(n)?.distance(points, apex) > eps {
                    seen.insert(n);
                    walk.push(n);
                } else {
                    horizon.push((f.v[k], f.v[(k + 1) % 3], n));
                }
            }
        }
        // The horizon around the seen faces must be one loop, or the faces don't close.
        horizon.sort_unstable();
        horizon.dedup();
        if horizon.is_empty() {
            return None;
        }
        let mut orphans = Vec::new();
        for &i in &seen {
            let f = faces.get_mut(i)?;
            f.alive = false;
            orphans.append(&mut f.outside);
            waiting.remove(&i);
        }
        let first = faces.len();
        let (mut starts, mut ends) = (BTreeMap::new(), BTreeMap::new());
        for (j, &(p, q, n)) in horizon.iter().enumerate() {
            let id = first + j;
            let mut f = Face::new(points, [p, q, top])?;
            f.next[0] = n;
            let across = faces.get_mut(n)?;
            let k = across.edge(q, p)?;
            *across.next.get_mut(k)? = id;
            if starts.insert(p, id).is_some() || ends.insert(q, id).is_some() {
                return None;
            }
            faces.push(f);
        }
        for f in faces.get_mut(first..)? {
            // q→apex meets the face that starts at q; apex→p the face that ends at p.
            f.next[1] = *starts.get(&f.v[1])?;
            f.next[2] = *ends.get(&f.v[0])?;
        }
        orphans.retain(|i| *i != top);
        orphans.sort_unstable();
        let last = faces.len();
        assign(points, &mut faces, first..last, orphans, eps);
        waiting.extend((first..last).filter(|&i| faces.get(i).is_some_and(|f| !f.outside.is_empty())));
    }
    if faces.iter().any(|f| f.alive && !f.outside.is_empty()) {
        return None;
    }
    let mut index = vec![None; points.len()];
    let mut mesh = Mesh::default();
    for f in faces.iter().filter(|f| f.alive) {
        let mut triangle = [0u32; 3];
        for (out, v) in triangle.iter_mut().zip(f.v) {
            let slot = index.get_mut(v)?;
            *out = match *slot {
                Some(k) => k,
                None => {
                    let k = u32::try_from(mesh.positions.len()).ok()?;
                    mesh.positions.push(at(v)?);
                    *slot = Some(k);
                    k
                }
            };
        }
        mesh.triangles.push(triangle);
    }
    Some(mesh)
}

/// Give each point to the first of the faces `among` it is outside of, if any.
fn assign(points: &[Vec3], faces: &mut [Face], among: std::ops::Range<usize>, which: impl IntoIterator<Item = usize>, eps: f64) {
    let Some(candidates) = faces.get_mut(among) else { return };
    for i in which {
        let Some(p) = points.get(i).copied() else { continue };
        if let Some(f) = candidates.iter_mut().find(|f| f.distance(points, p) > eps) {
            f.outside.push(i);
        }
    }
}

/// Four points far apart: the widest pair of the extreme points, then the farthest from their
/// line, then the farthest from their plane.
fn first_tetrahedron(points: &[Vec3], eps: f64) -> Option<[usize; 4]> {
    let farthest = |score: &dyn Fn(Vec3) -> f64| -> Option<(usize, f64)> {
        points.iter().enumerate().map(|(i, p)| (i, score(*p))).max_by(|x, y| x.1.total_cmp(&y.1))
    };
    let a = farthest(&|p: Vec3| p.x + p.y * 1e-3 + p.z * 1e-6)?.0;
    let pa = *points.get(a)?;
    let (b, ab) = farthest(&|p: Vec3| (p - pa).dot(p - pa).sqrt())?;
    if ab <= eps {
        return None;
    }
    let pb = *points.get(b)?;
    let (c, ac) = farthest(&|p: Vec3| {
        let n = (pb - pa).cross(p - pa);
        n.dot(n).sqrt() / ab
    })?;
    if ac <= eps {
        return None;
    }
    let pc = *points.get(c)?;
    let normal = (pb - pa).cross(pc - pa).normalized()?;
    let (e, ae) = farthest(&|p: Vec3| normal.dot(p - pa).abs())?;
    if ae <= eps {
        return None;
    }
    Some([a, b, c, e])
}

fn finite(p: Vec3) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.z.is_finite()
}

fn extent(points: &[Vec3]) -> Option<(Vec3, Vec3)> {
    let first = *points.first()?;
    Some(points.iter().fold((first, first), |(lo, hi), p| {
        (Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z)), Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z)))
    }))
}

/// A fitted shape: `frame` (column-major, like a placement) has it centred on its origin. A box's
/// `size` is its full lengths along x, y and z; a cylinder runs along z, with `size.x` its radius
/// and `size.z` its length; a sphere's radius is `size.x`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fitted {
    pub frame: [[f64; 4]; 4],
    pub size: Vec3,
}

fn usable(mesh: &Mesh) -> bool {
    !mesh.positions.is_empty() && mesh.positions.iter().all(|p| finite(*p))
}

/// The mesh's principal axes (right-handed): of its volume when it is closed, else of its points.
fn axes(mesh: &Mesh) -> [Vec3; 3] {
    let world = [Vec3::X, Vec3::Y, Vec3::Z];
    let tensor = match mesh.mass_properties() {
        Some(m) => m.inertia,
        None => {
            let n = mesh.positions.len() as f64;
            let c = mesh.positions.iter().fold(Vec3::ZERO, |a, p| a + *p) * (1.0 / n);
            let mut spread = [[0.0; 3]; 3];
            for p in &mesh.positions {
                let d = *p - c;
                let d = [d.x, d.y, d.z];
                for (row, x) in spread.iter_mut().zip(d) {
                    for (cell, y) in row.iter_mut().zip(d) {
                        *cell += x * y;
                    }
                }
            }
            spread
        }
    };
    principal(&tensor, EQUAL_MOMENTS).map_or(world, |(_, a)| a)
}

fn frame(x: Vec3, y: Vec3, z: Vec3, origin: Vec3) -> [[f64; 4]; 4] {
    [[x.x, x.y, x.z, 0.0], [y.x, y.y, y.z, 0.0], [z.x, z.y, z.z, 0.0], [origin.x, origin.y, origin.z, 1.0]]
}

/// Lowest and highest of the points along `axis`.
fn along(mesh: &Mesh, axis: Vec3) -> (f64, f64) {
    mesh.positions.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
        let x = axis.dot(*p);
        (lo.min(x), hi.max(x))
    })
}

/// The tight box along the mesh's principal axes. `None` for an empty or non-finite mesh.
pub fn fit_box(mesh: &Mesh) -> Option<Fitted> {
    if !usable(mesh) {
        return None;
    }
    let [x, y, z] = axes(mesh);
    let (ax, ay, az) = (along(mesh, x), along(mesh, y), along(mesh, z));
    let origin = x * ((ax.0 + ax.1) / 2.0) + y * ((ay.0 + ay.1) / 2.0) + z * ((az.0 + az.1) / 2.0);
    Some(Fitted { frame: frame(x, y, z, origin), size: Vec3::new(ax.1 - ax.0, ay.1 - ay.0, az.1 - az.0) })
}

/// The cylinder along whichever principal axis gives the least volume. `None` for an empty or
/// non-finite mesh.
pub fn fit_cylinder(mesh: &Mesh) -> Option<Fitted> {
    if !usable(mesh) {
        return None;
    }
    let [a, b, c] = axes(mesh);
    let mut best: Option<(f64, Fitted)> = None;
    for (x, y, z) in [(a, b, c), (b, c, a), (c, a, b)] {
        let (ax, ay, az) = (along(mesh, x), along(mesh, y), along(mesh, z));
        let (cx, cy) = ((ax.0 + ax.1) / 2.0, (ay.0 + ay.1) / 2.0);
        let radius = mesh.positions.iter().map(|p| (x.dot(*p) - cx).hypot(y.dot(*p) - cy)).fold(0.0, f64::max);
        let length = az.1 - az.0;
        let origin = x * cx + y * cy + z * ((az.0 + az.1) / 2.0);
        let volume = radius * radius * length;
        if best.as_ref().is_none_or(|(v, _)| volume < *v) {
            best = Some((volume, Fitted { frame: frame(x, y, z, origin), size: Vec3::new(radius, radius, length) }));
        }
    }
    best.map(|(_, f)| f)
}

/// A sphere around the points, centred in their bounding box. `None` for an empty or non-finite
/// mesh.
pub fn fit_sphere(mesh: &Mesh) -> Option<Fitted> {
    if !usable(mesh) {
        return None;
    }
    let (lo, hi) = extent(&mesh.positions)?;
    let c = (lo + hi) * 0.5;
    let radius = mesh.positions.iter().map(|p| (*p - c).dot(*p - c).sqrt()).fold(0.0, f64::max);
    Some(Fitted { frame: frame(Vec3::X, Vec3::Y, Vec3::Z, c), size: Vec3::new(radius, radius, radius) })
}

#[cfg(test)]
#[path = "hull3_tests.rs"]
mod tests;
