//! Conservative reconstruction of mechanical meshes from certified straight-extrusion parts.
//! Geometry is inferred from closed, manifold input, never from a display mesh.
//!
//! Deviations: no upstream counterpart. No recursion.

use crate::math::det;
use std::collections::BTreeMap;

use crate::kernel::Kernel;
use crate::math::{Vec2, Vec3};
use crate::prism::profile::{Loop2, Region2, Seg2};
use crate::{Brep, Result, SolidError};

use self::geometry::{
    circle, corners, distance_to_segment, nest_regions, normalize, polygon,
    ring_at_sharpest_corner, segment_length, MeshPlane, PlanarVector,
};

mod cap;
mod curves;
mod faceted;
mod geometry;
mod native;
mod parts;
mod topology;

#[cfg(test)]
mod tests;

/// Owned source triangles used only by mesh reconstruction, never a solid's display mesh.
#[derive(Clone, Debug, Default)]
struct TriMesh {
    positions: Vec<Vec3>,
    triangles: Vec<[u32; 3]>,
}

impl TriMesh {
    fn size(&self) -> f64 {
        mesh_size(&self.positions)
    }

    /// Signed source volume about a source point, avoiding cancellation from world translation.
    fn volume(&self) -> f64 {
        let origin = self.positions.first().copied().unwrap_or(Vec3::ZERO);
        let mut volume = crate::mesh_reduce::volume::Volume::new(origin);
        for &[a, b, c] in &self.triangles {
            if let (Some(a), Some(b), Some(c)) = (
                self.positions.get(a as usize),
                self.positions.get(b as usize),
                self.positions.get(c as usize),
            ) {
                volume.add([*a, *b, *c]);
            }
        }
        volume.value()
    }
}

/// Diagonal of the bounding box, used to scale the certification tolerances.
fn mesh_size(positions: &[Vec3]) -> f64 {
    let Some(&first) = positions.first() else {
        return 0.;
    };
    let (lo, hi) = positions.iter().fold((first, first), |(lo, hi), p| {
        (
            Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z)),
            Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z)),
        )
    });
    (hi - lo).length()
}

/// Maximum input triangle count. A mesh may also carry up to three vertices per triangle of this limit.
pub const MAX_TRIANGLES: usize = 50_000;
const MAX_BOUNDARY: usize = 8192;
/// Nesting certification runs two planar booleans per pair of boundary loops,
/// so the loop count (not the segment count) bounds its cost: 50 loops (an
/// outline with 49 holes) is at most 1225 pairs and 2450 booleans.
const MAX_LOOPS: usize = 50;

/// A malformed mesh or a reconstruction that fails a certificate.
fn invalid(message: &'static str) -> SolidError {
    SolidError::invalid_input("reconstruct mesh", message)
}

/// Version of this producer's face, edge and vertex construction order.
/// Bump this when either representation changes that order; applications include the
/// version in source-bound geometry identities rather than assuming an unstated order.
pub const FACE_ORDER_VERSION: u32 = 1;

/// How the converter represented the source mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconstructionKind {
    /// Certified planes and circular cylinders, including planar shoulders.
    Analytic,
    /// The source mesh's planar facets, with adjacent coplanar triangles joined.
    Faceted,
}

/// A converted solid and the representation selected from its source geometry.
#[derive(Debug)]
pub struct MeshReconstruction {
    /// The closed reconstructed solid.
    pub body: Brep,
    /// Whether analytic reconstruction was certified or the source facets were retained.
    pub kind: ReconstructionKind,
}

/// Convert a closed indexed source mesh to certified analytic geometry or its own facets.
/// Analytic work limits and failed certificates select the faceted representation of the
/// unchanged welded source; they never refuse a closed mesh solely because it is complex.
///
/// # Errors
/// Malformed source geometry, an invalid tolerance, an original input over its hard budget,
/// cancellation, or a kernel failure while constructing or exporting the solid.
pub fn reconstruct(
    k: &mut Kernel,
    positions: &[Vec3],
    triangles: &[[u32; 3]],
    tolerance: f64,
) -> Result<MeshReconstruction> {
    if !tolerance.is_finite() || !(1e-6..=1.).contains(&tolerance) {
        return Err(invalid("tolerance must be between 0.000001 and 1 mm"));
    }
    let mesh = weld(positions, triangles, tolerance)?;
    let mut edges: BTreeMap<(u32, u32), (usize, i32)> = BTreeMap::new();
    let mut axes: BTreeMap<(i64, i64, i64), (Vec3, f64)> = BTreeMap::new();
    for &[a, b, c] in &mesh.triangles {
        let (Some(pa), Some(pb), Some(pc)) = (
            mesh.positions.get(a as usize),
            mesh.positions.get(b as usize),
            mesh.positions.get(c as usize),
        ) else {
            return Err(invalid("triangle refers to a missing vertex"));
        };
        let cross = (*pb - *pa).cross(*pc - *pa);
        // Welding dropped the triangles of no area.
        let Some(mut n) = normalize(cross) else {
            return Err(invalid("degenerate triangle"));
        };
        for (a, b) in [(a, b), (b, c), (c, a)] {
            let entry = edges.entry((a.min(b), a.max(b))).or_default();
            entry.0 += 1;
            entry.1 += if a < b { 1 } else { -1 };
        }
        if n.x < -1e-8 || (n.x.abs() <= 1e-8 && (n.y < -1e-8 || (n.y.abs() <= 1e-8 && n.z < 0.))) {
            n = -n;
        }
        let key = (
            (n.x * 1e7).round() as i64,
            (n.y * 1e7).round() as i64,
            (n.z * 1e7).round() as i64,
        );
        let entry = axes.entry(key).or_insert((n, 0.));
        entry.1 += cross.length();
    }
    if edges
        .values()
        .any(|&(count, balance)| count != 2 || balance != 0)
    {
        return Err(invalid(
            "mesh must be closed and consistently oriented, with two triangles per edge",
        ));
    }
    let mut axes: Vec<_> = axes.into_values().collect();
    axes.sort_by(|a, b| b.1.total_cmp(&a.1));
    for &(axis, _) in axes.iter().take(16) {
        if let Some(result) = extrusion(k, &mesh, axis, tolerance)? {
            return Ok(MeshReconstruction {
                body: result,
                kind: ReconstructionKind::Analytic,
            });
        }
    }
    if let Some(body) = parts::reconstruct(k, &mesh, tolerance)? {
        return Ok(MeshReconstruction {
            body,
            kind: ReconstructionKind::Analytic,
        });
    }
    Ok(MeshReconstruction {
        body: faceted::build(k, &mesh)?,
        kind: ReconstructionKind::Faceted,
    })
}

fn check_budget(positions: &[Vec3], triangles: &[[u32; 3]]) -> Result<()> {
    if triangles.len() > MAX_TRIANGLES {
        return Err(invalid(
            "mesh exceeds the conversion limit of 50000 triangles",
        ));
    }
    if positions.len() > MAX_TRIANGLES * 3 {
        return Err(invalid(
            "mesh exceeds the conversion limit of 150000 vertices",
        ));
    }
    Ok(())
}

/// Merge nearby source vertices, discard degenerate triangles and unused vertices, and enforce bounded welding work.
fn weld(
    source_positions: &[Vec3],
    source_triangles: &[[u32; 3]],
    tolerance: f64,
) -> Result<TriMesh> {
    check_budget(source_positions, source_triangles)?;
    let size = mesh_size(source_positions);
    let epsilon = (size.max(1.) * 1e-7).min(tolerance * 0.01).max(1e-9);
    if !epsilon.is_finite()
        || source_positions.iter().any(|p| {
            [p.x, p.y, p.z]
                .iter()
                .any(|v| !v.is_finite() || (v / epsilon).abs() > 1e15)
        })
    {
        return Err(invalid("mesh coordinates exceed the conversion budget"));
    }
    let key = |p: Vec3| {
        (
            (p.x / epsilon).floor() as i64,
            (p.y / epsilon).floor() as i64,
            (p.z / epsilon).floor() as i64,
        )
    };
    let mut grid = BTreeMap::<_, Vec<u32>>::new();
    let mut positions: Vec<Vec3> = Vec::new();
    let mut remap = Vec::new();
    let mut comparisons = 0usize;
    for &p in source_positions {
        let k = key(p);
        let mut found = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    for &i in grid
                        .get(&(k.0 + dx, k.1 + dy, k.2 + dz))
                        .into_iter()
                        .flatten()
                    {
                        comparisons += 1;
                        if comparisons > 8_000_000 {
                            return Err(invalid("vertex welding exceeds its work budget"));
                        }
                        if positions
                            .get(i as usize)
                            .is_some_and(|q| q.distance(p) <= epsilon)
                        {
                            found = Some(i);
                            break;
                        }
                    }
                }
            }
        }
        let i = if let Some(i) = found {
            i
        } else {
            let i = u32::try_from(positions.len()).map_err(|_| invalid("too many vertices"))?;
            positions.push(p);
            grid.entry(k).or_default().push(i);
            i
        };
        remap.push(i);
    }
    // Preserve a closed collinear sliver's T-junction by splitting the sole triangle on
    // its long edge. Other zero-area triangles add no surface and are discarded.
    let mut kept = Vec::new();
    let mut slivers = Vec::new();
    for &[a, b, c] in source_triangles {
        let (Some(&a), Some(&b), Some(&c)) = (
            remap.get(a as usize),
            remap.get(b as usize),
            remap.get(c as usize),
        ) else {
            return Err(invalid("missing triangle vertex"));
        };
        let (Some(&pa), Some(&pb), Some(&pc)) = (
            positions.get(a as usize),
            positions.get(b as usize),
            positions.get(c as usize),
        ) else {
            return Err(invalid("missing triangle vertex"));
        };
        if normalize((pb - pa).cross(pc - pa)).is_some() {
            kept.push([a, b, c]);
        } else if a != b && b != c && c != a {
            slivers.extend(
                [
                    (pa.distance(pb), c, a, b),
                    (pb.distance(pc), a, b, c),
                    (pc.distance(pa), b, c, a),
                ]
                .into_iter()
                .max_by(|x, y| x.0.total_cmp(&y.0))
                .map(|(_, middle, u, v)| (u, v, middle)),
            );
        }
    }
    resolve_slivers(&mut kept, &slivers)?;
    let mut used: Vec<Option<u32>> = vec![None; positions.len()];
    let mut kept_positions = Vec::new();
    let mut triangles = Vec::new();
    for [a, b, c] in kept {
        let mut keep = |i: u32| -> Result<u32> {
            let slot = used
                .get_mut(i as usize)
                .ok_or_else(|| invalid("missing triangle vertex"))?;
            if let Some(k) = *slot {
                return Ok(k);
            }
            let k =
                u32::try_from(kept_positions.len()).map_err(|_| invalid("too many vertices"))?;
            kept_positions.push(
                positions
                    .get(i as usize)
                    .copied()
                    .ok_or_else(|| invalid("missing triangle vertex"))?,
            );
            *slot = Some(k);
            Ok(k)
        };
        triangles.push([keep(a)?, keep(b)?, keep(c)?]);
    }
    if triangles.is_empty() {
        return Err(invalid("the mesh has no triangle of any area"));
    }
    let mut mesh = TriMesh {
        positions: kept_positions,
        triangles,
    };
    // Welding can join formerly separate patches, so make their winding consistent
    // before testing manifoldness and orient a closed shell outward.
    parts::orient(&mut mesh);
    Ok(mesh)
}

/// Resolve the source's collinear slivers without inventing surface or losing edge subdivisions.
fn resolve_slivers(kept: &mut Vec<[u32; 3]>, slivers: &[(u32, u32, u32)]) -> Result<()> {
    let edge = |a: u32, b: u32| (a.min(b), a.max(b));
    let sides = |[a, b, c]: [u32; 3]| [(a, b, c), (b, c, a), (c, a, b)];
    let mut users = BTreeMap::<(u32, u32), Vec<usize>>::new();
    for (i, &t) in kept.iter().enumerate() {
        for (a, b, _) in sides(t) {
            users.entry(edge(a, b)).or_default().push(i);
        }
    }
    for &(u, v, middle) in slivers {
        let Some(&[i]) = users.get(&edge(u, v)).map(Vec::as_slice) else {
            continue;
        };
        let t = kept
            .get(i)
            .copied()
            .ok_or_else(|| invalid("missing sliver neighbour"))?;
        let Some((x, y, z)) = sides(t)
            .into_iter()
            .find(|&(a, b, _)| edge(a, b) == edge(u, v))
        else {
            continue;
        };
        for (a, b, _) in sides(t) {
            if let Some(list) = users.get_mut(&edge(a, b)) {
                list.retain(|&j| j != i);
            }
        }
        let j = kept.len();
        let left = [x, middle, z];
        let right = [middle, y, z];
        *kept
            .get_mut(i)
            .ok_or_else(|| invalid("missing sliver neighbour"))? = left;
        kept.push(right);
        for (index, tri) in [(i, left), (j, right)] {
            for (a, b, _) in sides(tri) {
                users.entry(edge(a, b)).or_default().push(index);
            }
        }
    }
    Ok(())
}

/// Certify matching caps and straight walls before building an analytic prism with valid nesting and volume.
fn extrusion(k: &mut Kernel, mesh: &TriMesh, axis: Vec3, tolerance: f64) -> Result<Option<Brep>> {
    type Boundary = BTreeMap<(i64, i64), Vec<(i64, i64)>>;
    let (lo, hi) = mesh
        .positions
        .iter()
        .map(|p| p.dot(axis))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), h| {
            (lo.min(h), hi.max(h))
        });
    let height = hi - lo;
    let size = mesh.size();
    let epsilon = (size.max(1.) * 1e-7).min(tolerance * 0.01).max(1e-9);
    if height <= epsilon {
        return Ok(None);
    }
    let Some(plane) = MeshPlane::from_normal(axis * lo, axis) else {
        return Ok(None);
    };
    let mut caps = [BTreeMap::<(u32, u32), usize>::new(), BTreeMap::new()];
    for triangle in &mesh.triangles {
        let [a, b, c] = *triangle;
        let (Some(pa), Some(pb), Some(pc)) = (
            mesh.positions.get(a as usize),
            mesh.positions.get(b as usize),
            mesh.positions.get(c as usize),
        ) else {
            return Err(invalid("missing triangle vertex"));
        };
        let hs = [pa.dot(axis), pb.dot(axis), pc.dot(axis)];
        let cap = if hs.iter().all(|h| (h - lo).abs() <= epsilon) {
            0
        } else if hs.iter().all(|h| (h - hi).abs() <= epsilon) {
            1
        } else {
            // STL stores coordinates as f32. Test the projected triangle's
            // distance from a line rather than amplifying that rounding into
            // an angular error on narrow side triangles.
            let [a, b, c] = [
                plane.to_local(*pa),
                plane.to_local(*pb),
                plane.to_local(*pc),
            ];
            let length = a.dist(b).max(b.dist(c)).max(c.dist(a));
            if (b - a).cross(c - a).abs() > epsilon * 3. * length {
                return Ok(None);
            }
            continue;
        };
        let Some(edges) = caps.get_mut(cap) else {
            return Ok(None);
        };
        for (a, b) in [(a, b), (b, c), (c, a)] {
            *edges.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    let raw_key = |p: Vec2| {
        (
            (p.x / epsilon).floor() as i64,
            (p.y / epsilon).floor() as i64,
        )
    };
    let mut positions = BTreeMap::<(i64, i64), Vec2>::new();
    let mut projected = Vec::new();
    for p in &mesh.positions {
        let p = plane.to_local(*p);
        let k = raw_key(p);
        let mut canonical = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                let candidate = (k.0 + dx, k.1 + dy);
                if positions
                    .get(&candidate)
                    .is_some_and(|q| q.distance(p) <= epsilon)
                {
                    canonical = Some(candidate);
                }
            }
        }
        let canonical = canonical.unwrap_or(k);
        positions.entry(canonical).or_insert(p);
        projected.push(canonical);
    }
    let boundary = |cap: usize| -> Option<Boundary> {
        let mut adjacency = Boundary::new();
        for (&(a, b), &count) in caps.get(cap)? {
            if count == 2 {
                continue;
            }
            if count != 1 {
                return None;
            }
            let (a, b) = (*projected.get(a as usize)?, *projected.get(b as usize)?);
            if a == b {
                return None;
            }
            adjacency.entry(a).or_default().push(b);
            adjacency.entry(b).or_default().push(a);
        }
        for neighbours in adjacency.values_mut() {
            neighbours.sort_unstable();
        }
        Some(adjacency)
    };
    let (Some(mut adjacency), Some(mut other)) = (boundary(0), boundary(1)) else {
        return Ok(None);
    };
    // Clipping a triangle at a shoulder can introduce different collinear nodes
    // on the two caps. Compare their actual polygon edges, not tessellation counts.
    let simplify = |graph: &mut Boundary| -> Option<()> {
        if graph.len() > MAX_BOUNDARY || graph.values().any(|v| v.len() != 2) {
            return None;
        }
        let mut pending: std::collections::VecDeque<_> = graph.keys().copied().collect();
        while let Some(key) = pending.pop_front() {
            let Some(neighbours) = graph.get(&key) else {
                continue;
            };
            let (Some(&a), Some(&b)) = (neighbours.first(), neighbours.get(1)) else {
                return None;
            };
            if a == b {
                return None;
            }
            let (pa, pb, p) = (
                *positions.get(&a)?,
                *positions.get(&b)?,
                *positions.get(&key)?,
            );
            if distance_to_segment(&Seg2::Line { a: pa, b: pb }, p) > epsilon * 3. {
                continue;
            }
            if graph.get(&a)?.contains(&b) {
                continue;
            }
            graph.remove(&key);
            for (from, to) in [(a, b), (b, a)] {
                let adjacent = graph.get_mut(&from)?;
                let slot = adjacent.iter_mut().find(|v| **v == key)?;
                *slot = to;
                adjacent.sort_unstable();
                pending.push_back(from);
            }
        }
        Some(())
    };
    if simplify(&mut adjacency).is_none() || simplify(&mut other).is_none() {
        return Ok(None);
    }
    if adjacency != other
        || adjacency.is_empty()
        || adjacency.len() > MAX_BOUNDARY
        || adjacency.values().any(|n| n.len() != 2)
    {
        return Ok(None);
    }
    // Every side vertex must lie on an unchanged straight extrusion of the cap.
    // This excludes twisted, tapered and stepped meshes before fitting any circles.
    let point = |k: (i64, i64)| Vec2::new(k.0 as f64 * epsilon, k.1 as f64 * epsilon);
    let boundary_segments: Vec<_> = adjacency
        .iter()
        .flat_map(|(&a, bs)| {
            bs.iter()
                .filter(move |&&b| a < b)
                .map(move |&b| (point(a), point(b)))
        })
        .collect();
    if mesh.positions.len().saturating_mul(boundary_segments.len()) > 32_000_000 {
        return Ok(None);
    }
    for p in &mesh.positions {
        let h = p.dot(axis);
        if (h - lo).abs() <= epsilon || (h - hi).abs() <= epsilon {
            continue;
        }
        let p = plane.to_local(*p);
        if !boundary_segments
            .iter()
            .any(|&(a, b)| distance_to_segment(&Seg2::Line { a, b }, p) <= epsilon * 3.)
        {
            return Ok(None);
        }
    }
    let mut loops = Vec::new();
    let mut raw_loops = Vec::new();
    while let Some((&start, _)) = adjacency.first_key_value() {
        let mut current = start;
        let mut previous = None;
        let mut chain = Vec::new();
        for _ in 0..=MAX_BOUNDARY {
            let Some(neighbours) = adjacency.remove(&current) else {
                return Ok(None);
            };
            chain.push(
                positions
                    .get(&current)
                    .copied()
                    .unwrap_or_else(|| point(current)),
            );
            let next = neighbours.into_iter().find(|n| Some(*n) != previous);
            let Some(next) = next else { return Ok(None) };
            if next == start {
                break;
            }
            previous = Some(current);
            current = next;
        }
        if chain.len() < 3 || current == start {
            return Ok(None);
        }
        raw_loops.push(polygon(&chain));
        loops.push(fit_loop(&chain, tolerance, epsilon));
    }
    if !certify_nesting(k, &raw_loops, &loops, epsilon, size)? {
        return Ok(None);
    }
    // Nest by the original polygon, so smoothing cannot change which loop is a hole.
    let Some(regions) = nest_regions(&raw_loops, &loops)? else {
        return Ok(None);
    };
    let Some(mut body) = native::extrude(k, &plane.frame(), &regions, height)? else {
        return Ok(None);
    };
    if !native::is_valid(k, &mut body)? {
        return Ok(None);
    }
    let perimeter: f64 = loops
        .iter()
        .flat_map(|lp| &lp.segs)
        .map(segment_length)
        .sum();
    let original = mesh.volume();
    let Some(rebuilt) = native::volume(k, &mut body)? else {
        return Ok(None);
    };
    if !original.is_finite()
        || original <= 0.
        || (rebuilt - original).abs()
            > perimeter * height * tolerance + epsilon * det::powi(size, 2)
    {
        return Ok(None);
    }
    Ok(Some(body))
}

/// Whether fitted boundaries keep containment, within the bounded pairwise Boolean work.
fn certify_nesting(
    k: &mut Kernel,
    raw: &[Loop2],
    fitted: &[Loop2],
    epsilon: f64,
    size: f64,
) -> Result<bool> {
    if raw.len() > MAX_LOOPS || raw.len() != fitted.len() {
        return Ok(false);
    }
    let mut work = 0usize;
    for (i, a) in raw.iter().enumerate() {
        for (j, b) in raw.iter().enumerate().skip(i + 1) {
            work = work.saturating_add(a.segs.len().saturating_mul(b.segs.len()));
            if work > 16_000_000 {
                return Ok(false);
            }
            let (Some(ab), Some(ba)) =
                (geometry::contains_loop(a, b), geometry::contains_loop(b, a))
            else {
                return Ok(false);
            };
            if ab && ba {
                return Ok(false);
            }
            for (a, b) in [
                (a, b),
                (
                    fitted
                        .get(i)
                        .ok_or_else(|| invalid("missing fitted loop"))?,
                    fitted
                        .get(j)
                        .ok_or_else(|| invalid("missing fitted loop"))?,
                ),
            ] {
                let region = |l: &Loop2| -> Result<Region2> {
                    Ok(Region2 {
                        outer: geometry::ccw(l)?,
                        holes: Vec::new(),
                    })
                };
                let Some(area) = native::intersection_area(
                    k,
                    &region(a)?,
                    &region(b)?,
                    epsilon.clamp(1e-9, 1.),
                )?
                else {
                    return Ok(false);
                };
                let expected = if ab {
                    b.signed_area()?.value.abs()
                } else if ba {
                    a.signed_area()?.value.abs()
                } else {
                    0.
                };
                if (area - expected).abs() > epsilon * size.max(1.) * 10. {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

/// Recover full circles or partial arcs only after sample and chord checks; otherwise retain polygon corners.
fn fit_loop(points: &[Vec2], tolerance: f64, epsilon: f64) -> Loop2 {
    // Fit a complete circular boundary only with enough samples to distinguish it
    // from a polygon. Certify the chord deviation, not just the vertex residual.
    if points.len() >= 12 {
        let (Some(&a), Some(&b), Some(&c)) = (
            points.first(),
            points.get(points.len() / 3),
            points.get(2 * points.len() / 3),
        ) else {
            return polygon(points);
        };
        let u = b - a;
        let v = c - a;
        let d = 2. * u.cross(v);
        if d.abs() > epsilon * epsilon {
            let center = a + Vec2::new(
                v.y * u.len2() - u.y * v.len2(),
                u.x * v.len2() - v.x * u.len2(),
            )
            .divided_by(d);
            let radius = center.dist(a);
            if radius.is_finite()
                && radius > epsilon
                && points
                    .iter()
                    .all(|p| (center.dist(*p) - radius).abs() <= epsilon * 3.)
            {
                let mut sweep = 0.;
                let mut direction = 0f64;
                let valid = points
                    .iter()
                    .zip(points.iter().cycle().skip(1))
                    .all(|(a, b)| {
                        let signed = det::atan2(
                            (*a - center).cross(*b - center),
                            (*a - center).dot(*b - center),
                        );
                        if direction == 0. {
                            direction = signed.signum();
                        }
                        if signed * direction <= 0. {
                            return false;
                        }
                        sweep += signed;
                        let angle = signed.abs();
                        angle <= std::f64::consts::PI / 6. + 1e-8
                            && radius * (1. - det::cos(angle * 0.5)) + epsilon * 10. <= tolerance
                    });
                if valid && (sweep.abs() - std::f64::consts::TAU).abs() <= 1e-7 {
                    return circle(center, radius);
                }
            }
        }
    }
    let points = straight_corners(points, epsilon);
    fit_partial_arcs(&points, tolerance, epsilon)
}

/// Remove only the source's straight subdivisions, with the same bounded total displacement.
fn straight_corners(points: &[Vec2], epsilon: f64) -> Vec<Vec2> {
    let mut points = points.to_vec();
    let linear_tolerance = epsilon / points.len().max(1) as f64;
    loop {
        let n = points.len();
        if n <= 3 {
            break;
        }
        let remove = corners(&points).position(|(a, b, c)| {
            distance_to_segment(&Seg2::Line { a, b: c }, b) <= linear_tolerance
        });
        let Some(index) = remove else { break };
        let mut at = 0;
        points.retain(|_| {
            let keep = at != index;
            at += 1;
            keep
        });
    }
    points
}

/// A circular arc through both actual endpoints, independently certified against every
/// source vertex and chord. Short polygon corners and coarse facets are not rounded.
fn partial_arc(points: &[Vec2], tolerance: f64, epsilon: f64) -> Option<Seg2> {
    if points.len() < 5 {
        return None;
    }
    let first = *points.first()?;
    let last = *points.last()?;
    let chord = last - first;
    let length = chord.len();
    if length <= epsilon * 10. {
        return None;
    }
    let mean = points
        .iter()
        .copied()
        .fold(Vec2::ZERO, |a, b| a + b)
        .divided_by(points.len() as f64);
    let (mut xx, mut xy, mut yy, mut xq, mut yq) = (0., 0., 0., 0., 0.);
    for p in points {
        let d = *p - mean;
        let q = d.len2();
        xx += d.x * d.x;
        xy += d.x * d.y;
        yy += d.y * d.y;
        xq += d.x * q;
        yq += d.y * q;
    }
    let determinant = xx * yy - xy * xy;
    if determinant.abs() <= det::powi(xx + yy, 2) * 1e-10 {
        return None;
    }
    let seed = mean + Vec2::new(yy * xq - xy * yq, xx * yq - xy * xq).divided_by(2. * determinant);
    let normal = Vec2::new(-chord.y, chord.x).divided_by(length);
    let middle = (first + last) * 0.5;
    let center = middle + normal * (seed - middle).dot(normal);
    let radius = center.dist(first);
    if !radius.is_finite() || radius <= epsilon {
        return None;
    }
    let residual = points
        .iter()
        .map(|p| (p.dist(center) - radius).abs())
        .fold(0., f64::max);
    if residual > epsilon * 5. {
        return None;
    }
    let mut sweep = 0.;
    let mut direction = 0.;
    for (&a, &b) in points.iter().zip(points.iter().skip(1)) {
        let (a, b) = (a - center, b - center);
        let angle = det::atan2(a.cross(b), a.dot(b));
        if direction == 0. {
            direction = angle.signum();
        }
        if angle * direction <= 0.
            || angle.abs() > std::f64::consts::PI / 6. + 1e-8
            || radius * (1. - det::cos(angle * 0.5)) + residual + epsilon * 10. > tolerance
        {
            return None;
        }
        sweep += angle;
    }
    if sweep.abs() < 0.04 || sweep.abs() > std::f64::consts::TAU - 0.01 {
        return None;
    }
    let start = det::atan2((first - center).y, (first - center).x);
    Some(Seg2::Arc {
        center,
        radius,
        start,
        sweep,
    })
}

/// Replace certified circular runs while keeping their actual endpoints and the remaining polygon edges.
fn fit_partial_arcs(points: &[Vec2], tolerance: f64, epsilon: f64) -> Loop2 {
    let n = points.len();
    if n < 5 {
        return polygon(points);
    }
    let ring = ring_at_sharpest_corner(points);
    let mut at = 0;
    let mut work = 0usize;
    let mut segs = Vec::new();
    while at < n {
        let mut fitted = None;
        for end in at + 4..=n {
            work = work.saturating_add((end - at + 1) * 2);
            if work > 4_000_000 {
                return polygon(points);
            }
            let Some(run) = ring.get(at..=end) else { break };
            let Some(arc) = partial_arc(run, tolerance, epsilon) else {
                break;
            };
            fitted = Some((end, arc));
        }
        if let Some((end, arc)) = fitted {
            segs.push(arc);
            at = end;
        } else {
            let (Some(&a), Some(&b)) = (ring.get(at), ring.get(at + 1)) else {
                return polygon(points);
            };
            segs.push(Seg2::Line { a, b });
            at += 1;
        }
    }
    Loop2 { segs }
}
