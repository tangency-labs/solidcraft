//! Bounded decomposition at existing planar shoulders. Each final part must pass the
//! straight-extrusion certificate; cutting never makes a faceted B-rep.
//!
//! Deviations: no upstream counterpart. Shoulder decomposition descends at most eight levels.

use std::collections::{BTreeMap, VecDeque};

use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::{Brep, Result};

use super::geometry::{normalize, MeshPlane};
use super::{extrusion, invalid, native, TriMesh, MAX_BOUNDARY, MAX_TRIANGLES};

const MAX_ATTEMPTS: usize = 24;
const MAX_DEPTH: usize = 8;
const MAX_COMPONENTS: usize = 32;
const MAX_FACES: usize = 256;

/// Start bounded shoulder decomposition with one shared cut budget and original-mesh circle hints.
pub(super) fn reconstruct(k: &mut Kernel, mesh: &TriMesh, tolerance: f64) -> Result<Option<Brep>> {
    recover(k, mesh, mesh, tolerance, 0, &mut 0, &mut BTreeMap::new())
}

/// Rank at most sixteen canonical face-normal directions by triangle area to bound the reconstruction search.
fn axes(mesh: &TriMesh) -> Vec<(Vec3, f64)> {
    let mut out = BTreeMap::new();
    for &[a, b, c] in &mesh.triangles {
        let (Some(a), Some(b), Some(c)) = (
            mesh.positions.get(a as usize),
            mesh.positions.get(b as usize),
            mesh.positions.get(c as usize),
        ) else {
            continue;
        };
        let cross = (*b - *a).cross(*c - *a);
        let Some(mut n) = normalize(cross) else {
            continue;
        };
        if n.x < -1e-8 || (n.x.abs() <= 1e-8 && (n.y < -1e-8 || (n.y.abs() <= 1e-8 && n.z < 0.))) {
            n = -n;
        }
        let key = (
            (n.x * 1e7).round() as i64,
            (n.y * 1e7).round() as i64,
            (n.z * 1e7).round() as i64,
        );
        let e = out.entry(key).or_insert((n, 0.));
        e.1 += cross.length();
    }
    let mut out: Vec<_> = out.into_values().collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out.truncate(16);
    out
}

/// Try certified extrusions before splitting at existing shoulders, sharing cut, depth and recovered-face budgets.
fn recover(
    k: &mut Kernel,
    mesh: &TriMesh,
    root: &TriMesh,
    tolerance: f64,
    depth: usize,
    attempts: &mut usize,
    models: &mut BTreeMap<(i64, i64, i64), Vec<super::curves::Circle>>,
) -> Result<Option<Brep>> {
    if depth > MAX_DEPTH || *attempts >= MAX_ATTEMPTS {
        return Ok(None);
    }
    let candidates = axes(mesh);
    {
        for &(axis, _) in &candidates {
            let key = (
                (axis.x * 1e6).round() as i64,
                (axis.y * 1e6).round() as i64,
                (axis.z * 1e6).round() as i64,
            );
            let circles = models.entry(key).or_insert_with(|| {
                super::curves::hints(root, axis, tolerance, tolerance.min(1e-3) * 0.01)
            });
            let result = match super::curves::extrusion(k, mesh, axis, tolerance, circles)? {
                Some(body) => Some(body),
                None if depth > 0 => extrusion(k, mesh, axis, tolerance)?,
                None => None,
            };
            if let Some(body) = result {
                if body.face_list().len() <= MAX_FACES {
                    return Ok(Some(body));
                }
                return Ok(None);
            }
        }
    }
    let size = mesh.size();
    let epsilon = (size.max(1.) * 1e-7).min(tolerance * 0.01).max(1e-9);
    let mut cuts = Vec::new();
    let area = candidates.iter().map(|x| x.1).sum::<f64>();
    for &(axis, _) in candidates.iter().take(6) {
        let lo = mesh
            .positions
            .iter()
            .map(|p| p.dot(axis))
            .fold(f64::INFINITY, f64::min);
        let hi = mesh
            .positions
            .iter()
            .map(|p| p.dot(axis))
            .fold(f64::NEG_INFINITY, f64::max);
        let mut planes = Vec::new();
        for &[a, b, c] in &mesh.triangles {
            let (Some(a), Some(b), Some(c)) = (
                mesh.positions.get(a as usize),
                mesh.positions.get(b as usize),
                mesh.positions.get(c as usize),
            ) else {
                return Err(invalid("missing part vertex"));
            };
            let h = [a.dot(axis), b.dot(axis), c.dot(axis)];
            if h.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                - h.iter().copied().fold(f64::INFINITY, f64::min)
                <= epsilon
            {
                let height = h.iter().sum::<f64>() / 3.;
                if height > lo + epsilon * 8. && height < hi - epsilon * 8. {
                    planes.push((height, (*b - *a).cross(*c - *a).length()));
                }
            }
        }
        planes.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for (h, a) in planes {
            if let Some(last) = merged.last_mut().filter(|p| (p.0 - h).abs() <= epsilon) {
                last.1 += a;
            } else {
                merged.push((h, a));
            }
        }
        for (height, a) in merged {
            if a >= area * 1e-3 {
                cuts.push((a, axis, height));
            }
        }
    }
    cuts.sort_by(|a, b| b.0.total_cmp(&a.0));
    cuts.truncate(8);
    for (_, axis, height) in cuts {
        if *attempts >= MAX_ATTEMPTS {
            break;
        }
        *attempts += 1;
        let Some(low) = half(mesh, axis, height, false, epsilon)? else {
            continue;
        };
        let Some(high) = half(mesh, axis, height, true, epsilon)? else {
            continue;
        };
        let Some(mut pieces) = components(&low)? else {
            continue;
        };
        let Some(other) = components(&high)? else {
            continue;
        };
        pieces.extend(other);
        if pieces.len() < 2 || pieces.len() > MAX_COMPONENTS {
            continue;
        }
        let original = mesh.volume();
        let split_volume: f64 = pieces.iter().map(TriMesh::volume).sum();
        if (split_volume - original).abs() > epsilon * size.powi(2) * 10. {
            continue;
        }
        let mut bodies = Vec::new();
        let mut faces = 0;
        for piece in &pieces {
            let Some(body) = recover(k, piece, root, tolerance, depth + 1, attempts, models)?
            else {
                bodies.clear();
                break;
            };
            faces += body.face_list().len();
            if faces > MAX_FACES {
                bodies.clear();
                break;
            }
            bodies.push(body);
        }
        if bodies.len() != pieces.len() {
            continue;
        }
        let mut result: Option<Brep> = None;
        for body in bodies {
            result = Some(match result {
                None => body,
                Some(previous) => native::union(k, previous, body)?,
            });
        }
        if let Some(mut body) = result {
            if !native::is_valid(k, &mut body)? {
                return Err(invalid("reconstructed parts did not form a valid solid"));
            }
            let volume = native::volume(k, &mut body)?;
            let bound = size.powi(2) * tolerance * 4.;
            if !volume.is_finite() || volume <= 0. || (volume - original).abs() > bound {
                return Err(invalid("reconstructed part volume differs from the source"));
            }
            return Ok(Some(body));
        }
    }
    Ok(None)
}

struct CutMesh {
    mesh: TriMesh,
    grid: BTreeMap<(i64, i64, i64), Vec<u32>>,
    epsilon: f64,
}
impl CutMesh {
    /// Reuse a cut vertex within epsilon across neighboring grid cells and report vertex-index overflow.
    fn point(&mut self, p: Vec3) -> Result<u32> {
        let e = self.epsilon;
        let key = (
            (p.x / e).floor() as i64,
            (p.y / e).floor() as i64,
            (p.z / e).floor() as i64,
        );
        for x in -1..=1 {
            for y in -1..=1 {
                for z in -1..=1 {
                    for &i in self
                        .grid
                        .get(&(key.0 + x, key.1 + y, key.2 + z))
                        .into_iter()
                        .flatten()
                    {
                        if self
                            .mesh
                            .positions
                            .get(i as usize)
                            .is_some_and(|q| q.distance(p) <= e)
                        {
                            return Ok(i);
                        }
                    }
                }
            }
        }
        let i =
            u32::try_from(self.mesh.positions.len()).map_err(|_| invalid("part vertex budget"))?;
        self.mesh.positions.push(p);
        self.grid.entry(key).or_default().push(i);
        Ok(i)
    }
    /// Weld a clipped triangle, discard degenerate corners and enforce the clipping triangle budget.
    fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3) -> Result<()> {
        if (b - a).cross(c - a).length() <= self.epsilon.powi(2) {
            return Ok(());
        }
        let (a, b, c) = (self.point(a)?, self.point(b)?, self.point(c)?);
        if a != b && b != c && c != a {
            self.mesh.triangles.push([a, b, c]);
        }
        if self.mesh.triangles.len() > MAX_TRIANGLES {
            return Err(invalid("part clipping exceeds its triangle budget"));
        }
        Ok(())
    }
}

/// Clip to one half-space and cap its planar cut, refusing ambiguous, oversized or unclosed boundaries.
fn half(
    mesh: &TriMesh,
    axis: Vec3,
    height: f64,
    positive: bool,
    epsilon: f64,
) -> Result<Option<TriMesh>> {
    let mut out = CutMesh {
        mesh: TriMesh::default(),
        grid: BTreeMap::new(),
        epsilon,
    };
    let sign = if positive { 1. } else { -1. };
    for t in &mesh.triangles {
        let mut points = Vec::new();
        for &i in t {
            let Some(&p) = mesh.positions.get(i as usize) else {
                return Err(invalid("missing clipping vertex"));
            };
            let d = p.dot(axis) - height;
            points.push(if d.abs() <= epsilon { p - axis * d } else { p });
        }
        if points
            .iter()
            .all(|p| (p.dot(axis) - height).abs() <= epsilon)
        {
            let (Some(a), Some(b), Some(c)) = (points.first(), points.get(1), points.get(2)) else {
                return Ok(None);
            };
            if (*b - *a).cross(*c - *a).dot(axis) * sign < 0. {
                out.triangle(*a, *b, *c)?;
            }
            continue;
        }
        let mut polygon = Vec::new();
        for (a, b) in points.iter().zip(points.iter().cycle().skip(1)).take(3) {
            let da = (a.dot(axis) - height) * sign;
            let db = (b.dot(axis) - height) * sign;
            if da >= 0. {
                polygon.push(*a);
            }
            if (da > 0. && db < 0.) || (da < 0. && db > 0.) {
                let p = *a + (*b - *a) * (da / (da - db));
                polygon.push(p - axis * (p.dot(axis) - height));
            }
        }
        polygon.dedup_by(|a, b| a.distance(*b) <= epsilon);
        if polygon.len() > 2
            && polygon
                .first()
                .zip(polygon.last())
                .is_some_and(|(a, b)| a.distance(*b) <= epsilon)
        {
            polygon.pop();
        }
        if let Some(&a) = polygon.first() {
            for (&b, &c) in polygon.iter().skip(1).zip(polygon.iter().skip(2)) {
                out.triangle(a, b, c)?;
            }
        }
    }
    if out.mesh.triangles.is_empty() {
        return Ok(None);
    }
    let edges = edge_map(&out.mesh);
    let mut next = BTreeMap::new();
    for (&(a, b), uses) in &edges {
        if uses.len() == 2 {
            continue;
        }
        if uses.len() != 1 {
            return Ok(None);
        }
        let (Some(pa), Some(pb)) = (
            out.mesh.positions.get(a as usize),
            out.mesh.positions.get(b as usize),
        ) else {
            return Ok(None);
        };
        if (pa.dot(axis) - height).abs() > epsilon * 4.
            || (pb.dot(axis) - height).abs() > epsilon * 4.
        {
            return Ok(None);
        }
        let Some(&(u, v, _)) = uses.first() else {
            return Ok(None);
        };
        if next.insert(v, u).is_some() {
            return Ok(None);
        }
    }
    if next.len() > MAX_BOUNDARY {
        return Err(invalid("part cap exceeds its boundary budget"));
    }
    let Some(plane) = MeshPlane::from_normal(axis * height, axis) else {
        return Ok(None);
    };
    let mut boundary = Vec::new();
    let mut ids = Vec::new();
    let mut loops = Vec::new();
    while let Some((&start, _)) = next.first_key_value() {
        let from = boundary.len();
        let mut at = start;
        loop {
            let Some(to) = next.remove(&at) else {
                return Ok(None);
            };
            let Some(&p) = out.mesh.positions.get(at as usize) else {
                return Ok(None);
            };
            let p = plane.to_local(p);
            boundary.push([p.x, p.y]);
            ids.push(at);
            at = to;
            if at == start {
                break;
            }
        }
        if boundary.len() - from < 3 {
            return Ok(None);
        }
        loops.push(from..boundary.len());
    }
    if !boundary.is_empty() {
        let Some(triangles) = super::cap::triangulate(&boundary, &loops) else {
            return Ok(None);
        };
        for [a, b, c] in triangles {
            let (Some(&a), Some(&b), Some(&c)) = (
                ids.get(a as usize),
                ids.get(b as usize),
                ids.get(c as usize),
            ) else {
                return Ok(None);
            };
            out.mesh
                .triangles
                .push(if positive { [a, c, b] } else { [a, b, c] });
        }
    }
    if out.mesh.triangles.len() > MAX_TRIANGLES {
        return Err(invalid("part cap exceeds its triangle budget"));
    }
    if !closed(&out.mesh) {
        return Ok(None);
    }
    Ok(Some(out.mesh))
}

type EdgeUses = BTreeMap<(u32, u32), Vec<(u32, u32, usize)>>;
/// Index oriented edge uses and their owning triangles for closedness and component checks.
fn edge_map(mesh: &TriMesh) -> EdgeUses {
    let mut edges = EdgeUses::new();
    for (i, &[a, b, c]) in mesh.triangles.iter().enumerate() {
        for (a, b) in [(a, b), (b, c), (c, a)] {
            edges
                .entry((a.min(b), a.max(b)))
                .or_default()
                .push((a, b, i));
        }
    }
    edges
}
/// Repair the orientation of a welded mesh, as welding can join previously separate patches.
pub(super) fn orient(mesh: &mut TriMesh) {
    let edges = edge_map(mesh);
    let mut seen = vec![false; mesh.triangles.len()];
    for seed in 0..mesh.triangles.len() {
        let Some(slot @ false) = seen.get_mut(seed) else {
            continue;
        };
        *slot = true;
        let mut stack = vec![seed];
        while let Some(i) = stack.pop() {
            let Some(&[a, b, c]) = mesh.triangles.get(i) else {
                continue;
            };
            for (a, b) in [(a, b), (b, c), (c, a)] {
                let Some([(_, _, left), (_, _, right)]) =
                    edges.get(&(a.min(b), a.max(b))).map(Vec::as_slice)
                else {
                    continue;
                };
                let other = if *left == i { *right } else { *left };
                let Some(slot @ false) = seen.get_mut(other) else {
                    continue;
                };
                *slot = true;
                let Some(triangle) = mesh.triangles.get_mut(other) else {
                    continue;
                };
                let [x, y, z] = *triangle;
                if [(x, y), (y, z), (z, x)].contains(&(a, b)) {
                    *triangle = [x, z, y];
                }
                stack.push(other);
            }
        }
    }
    if mesh.volume() < 0. && closed(mesh) {
        for triangle in &mut mesh.triangles {
            let [a, b, c] = *triangle;
            *triangle = [a, c, b];
        }
    }
}

/// Require each mesh edge to have exactly two uses with opposite winding.
fn closed(mesh: &TriMesh) -> bool {
    edge_map(mesh)
        .values()
        .all(|v| matches!(v.as_slice(),[(a,b,_),(c,d,_)] if a==d && b==c))
}
/// Split edge-connected pieces with compact vertex indices, requiring two uses per edge and bounded component count.
fn components(mesh: &TriMesh) -> Result<Option<Vec<TriMesh>>> {
    let edges = edge_map(mesh);
    let mut neighbours = vec![Vec::new(); mesh.triangles.len()];
    for uses in edges.values() {
        let [(_, _, a), (_, _, b)] = uses.as_slice() else {
            return Ok(None);
        };
        let Some(n) = neighbours.get_mut(*a) else {
            return Ok(None);
        };
        n.push(*b);
        let Some(n) = neighbours.get_mut(*b) else {
            return Ok(None);
        };
        n.push(*a);
    }
    let mut seen = vec![false; mesh.triangles.len()];
    let mut out = Vec::new();
    for seed in 0..mesh.triangles.len() {
        if seen.get(seed).copied().unwrap_or(true) {
            continue;
        }
        if out.len() >= MAX_COMPONENTS {
            return Err(invalid("too many reconstructed components"));
        }
        let mut part = TriMesh::default();
        let mut remap = BTreeMap::new();
        let mut queue = VecDeque::from([seed]);
        while let Some(i) = queue.pop_front() {
            let Some(done) = seen.get_mut(i) else {
                return Ok(None);
            };
            if *done {
                continue;
            }
            *done = true;
            let Some(t) = mesh.triangles.get(i) else {
                return Ok(None);
            };
            let mut mapped = [0; 3];
            for (slot, &v) in mapped.iter_mut().zip(t) {
                let id = if let Some(&j) = remap.get(&v) {
                    j
                } else {
                    let Some(&p) = mesh.positions.get(v as usize) else {
                        return Ok(None);
                    };
                    let j = u32::try_from(part.positions.len())
                        .map_err(|_| invalid("component vertex budget"))?;
                    part.positions.push(p);
                    remap.insert(v, j);
                    j
                };
                *slot = id;
            }
            part.triangles.push(mapped);
            if let Some(next) = neighbours.get(i) {
                queue.extend(next.iter().copied());
            }
        }
        out.push(part);
    }
    Ok(Some(out))
}
