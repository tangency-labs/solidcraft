//! Conservative reconstruction of a mesh as a solid: a constant-section mechanical part as
//! analytic planes and certified cylinders, any other closed mesh as the faceted solid its
//! triangles describe (#358). Geometry is inferred from closed, manifold input, never from a
//! display mesh.

use solidcraft_geom::prelude::*;
use std::collections::BTreeMap;

use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};

use crate::{Body, EdgeSpec, FaceSpec, KernelError, Result, SurfSpec, sew_touching};

const MAX_TRIANGLES: usize = 50_000;
const MAX_BOUNDARY: usize = 8192;
/// Nesting certification runs two planar booleans per pair of boundary loops,
/// so the loop count (not the segment count) bounds its cost: 50 loops (an
/// outline with 49 holes) is at most 1225 pairs and 2450 booleans.
const MAX_LOOPS: usize = 50;
/// How far (mm) a triangle lies from its group's plane for the group to stay one planar face:
/// an absolute bound well under the 1e-6 mm within which sewing attaches a plane to a face's
/// whole wire, so triangles are fused only when they are on one plane, never smoothed onto one.
/// The faces are at most the triangles, so the triangle budget bounds this path too.
const COPLANAR: f64 = 1e-7;

fn invalid(message: &str) -> KernelError {
    KernelError::Invalid(format!("mesh conversion: {message}"))
}

/// A mesh over the triangle budget: how big it is and the limit (#346).
fn over_budget(triangles: usize) -> KernelError {
    invalid(&format!("the mesh has {triangles} triangles, more than the {MAX_TRIANGLES} conversion can take"))
}

/// Reconstruct a constant-section mesh as analytic planes and, where certified within
/// `tolerance` mm, circular cylinders; any other closed, consistently oriented mesh is rebuilt
/// as the faceted solid its triangles describe, one planar face per run of coplanar triangles
/// (#358). The source mesh is not modified.
pub fn convert_prismatic_mesh(body: &Body, tolerance: f64) -> Result<Body> {
    crate::guard("convert prismatic mesh", || {
        if !tolerance.is_finite() || !(1e-6..=1.).contains(&tolerance) {
            return Err(invalid("tolerance must be between 0.000001 and 1 mm"));
        }
        let input = body.mesh.as_ref().ok_or_else(|| invalid("select a mesh body"))?;
        let welded = weld(input, tolerance)?;
        let mesh = welded.mesh.as_ref().ok_or_else(|| invalid("empty mesh"))?;
        if mesh.triangles.len() > MAX_TRIANGLES {
            return Err(over_budget(mesh.triangles.len()));
        }
        let mut edges: BTreeMap<(u32, u32), (usize, i32)> = BTreeMap::new();
        let mut axes: BTreeMap<(i64, i64, i64), (Vec3, f64)> = BTreeMap::new();
        for triangle in &mesh.triangles {
            let [a, b, c] = *triangle;
            let (Some(pa), Some(pb), Some(pc)) = (mesh.positions.get(a as usize), mesh.positions.get(b as usize), mesh.positions.get(c as usize))
            else {
                return Err(invalid("triangle refers to a missing vertex"));
            };
            let cross = (*pb - *pa).cross(*pc - *pa);
            // `weld` dropped the triangles of no area, so none is left here.
            let Some(mut n) = cross.normalized() else { return Err(invalid("degenerate triangle")) };
            for (a, b) in [(a, b), (b, c), (c, a)] {
                let entry = edges.entry((a.min(b), a.max(b))).or_default();
                entry.0 += 1;
                entry.1 += if a < b { 1 } else { -1 };
            }
            if n.x < -1e-8 || (n.x.abs() <= 1e-8 && (n.y < -1e-8 || (n.y.abs() <= 1e-8 && n.z < 0.))) {
                n = -n;
            }
            let key = ((n.x * 1e7).round() as i64, (n.y * 1e7).round() as i64, (n.z * 1e7).round() as i64);
            let entry = axes.entry(key).or_insert((n, 0.));
            entry.1 += cross.len();
        }
        if edges.values().any(|&(count, balance)| count != 2 || balance != 0) {
            return Err(invalid("mesh must be closed and consistently oriented, with two triangles per edge"));
        }
        let mut axes: Vec<_> = axes.into_values().collect();
        axes.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (axis, _) in axes.into_iter().take(16) {
            if let Some(result) = extrusion(mesh, axis, tolerance)? {
                return Ok(result);
            }
        }
        // No axis certifies a constant section, so the mesh is rebuilt as the faceted solid it
        // is (#358). Nothing is fitted at the tolerance — each group of coplanar triangles
        // becomes one planar face, and a rounded wall stays a band of flat triangles rather
        // than a cylinder the mesh does not certify.
        faceted_solid(mesh)
    })
}

fn weld(mesh: &crate::meshbody::TriMesh, tolerance: f64) -> Result<Body> {
    if mesh.triangles.len() > MAX_TRIANGLES {
        return Err(over_budget(mesh.triangles.len()));
    }
    if mesh.positions.len() > MAX_TRIANGLES * 3 {
        return Err(invalid(&format!("the mesh has {} vertices, more than the {} conversion can take", mesh.positions.len(), MAX_TRIANGLES * 3)));
    }
    let bounds = mesh.positions.iter().fold(solidcraft_geom::Aabb3::EMPTY, |mut b, p| {
        b.add(*p);
        b
    });
    let epsilon = (bounds.size().len().max(1.) * 1e-7).min(tolerance * 0.01).max(1e-9);
    if !epsilon.is_finite() || mesh.positions.iter().any(|p| [p.x, p.y, p.z].iter().any(|v| !v.is_finite() || (v / epsilon).abs() > 1e15)) {
        return Err(invalid("mesh coordinates exceed the conversion budget"));
    }
    let key = |p: Vec3| ((p.x / epsilon).floor() as i64, (p.y / epsilon).floor() as i64, (p.z / epsilon).floor() as i64);
    let mut grid = BTreeMap::<_, Vec<u32>>::new();
    let mut positions: Vec<Vec3> = Vec::new();
    let mut remap = Vec::new();
    let mut comparisons = 0usize;
    for &p in &mesh.positions {
        let k = key(p);
        let mut found = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    for &i in grid.get(&(k.0 + dx, k.1 + dy, k.2 + dz)).into_iter().flatten() {
                        comparisons += 1;
                        if comparisons > 8_000_000 {
                            return Err(invalid("vertex welding exceeds its work budget"));
                        }
                        if positions.get(i as usize).is_some_and(|q| q.dist(p) <= epsilon) {
                            found = Some(i);
                            break;
                        }
                    }
                }
            }
        }
        let i = match found {
            Some(i) => i,
            None => {
                let i = u32::try_from(positions.len()).map_err(|_| invalid("too many vertices"))?;
                positions.push(p);
                grid.entry(k).or_default().push(i);
                i
            }
        };
        remap.push(i);
    }
    // A triangle of no area (its corners on one line, or welded into one another) adds no
    // surface, so it is dropped rather than refused (#4561); the closed-manifold check that
    // follows still refuses a mesh that needed it to close. Only the vertices the kept triangles
    // use stay, so a dropped triangle's corner cannot widen the part's extent.
    let mut used: Vec<Option<u32>> = vec![None; positions.len()];
    let mut kept_positions: Vec<Vec3> = Vec::new();
    let mut triangles = Vec::with_capacity(mesh.triangles.len());
    for t in &mesh.triangles {
        let (Some(&a), Some(&b), Some(&c)) = (remap.get(t[0] as usize), remap.get(t[1] as usize), remap.get(t[2] as usize)) else {
            return Err(invalid("missing triangle vertex"));
        };
        let (Some(&pa), Some(&pb), Some(&pc)) = (positions.get(a as usize), positions.get(b as usize), positions.get(c as usize)) else {
            return Err(invalid("missing triangle vertex"));
        };
        if (pb - pa).cross(pc - pa).normalized().is_none() {
            continue;
        }
        let mut keep = |i: u32| -> Result<u32> {
            let slot = used.get_mut(i as usize).ok_or_else(|| invalid("missing triangle vertex"))?;
            if let Some(k) = *slot {
                return Ok(k);
            }
            let k = u32::try_from(kept_positions.len()).map_err(|_| invalid("too many vertices"))?;
            kept_positions.push(positions.get(i as usize).copied().ok_or_else(|| invalid("missing triangle vertex"))?);
            *slot = Some(k);
            Ok(k)
        };
        triangles.push([keep(a)?, keep(b)?, keep(c)?]);
    }
    if triangles.is_empty() {
        return Err(invalid("the mesh has no triangle of any area"));
    }
    crate::mesh_body(&kept_positions, &triangles)
}

fn extrusion(mesh: &crate::meshbody::TriMesh, axis: Vec3, tolerance: f64) -> Result<Option<Body>> {
    let (lo, hi) = mesh.positions.iter().map(|p| p.dot(axis)).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), h| (lo.min(h), hi.max(h)));
    let height = hi - lo;
    let bounds = mesh.positions.iter().fold(solidcraft_geom::Aabb3::EMPTY, |mut b, p| {
        b.add(*p);
        b
    });
    let epsilon = (bounds.size().len().max(1.) * 1e-7).min(tolerance * 0.01).max(1e-9);
    if height <= epsilon {
        return Ok(None);
    }
    let Some(plane) = Plane::from_normal(axis * lo, axis) else { return Ok(None) };
    let mut caps = [BTreeMap::<(u32, u32), usize>::new(), BTreeMap::new()];
    // The side triangles, checked against the fitted section once it is known.
    let mut walls: Vec<[Vec3; 3]> = Vec::new();
    for triangle in &mesh.triangles {
        let [a, b, c] = *triangle;
        let (Some(pa), Some(pb), Some(pc)) = (mesh.positions.get(a as usize), mesh.positions.get(b as usize), mesh.positions.get(c as usize)) else {
            return Err(invalid("missing triangle vertex"));
        };
        let hs = [pa.dot(axis), pb.dot(axis), pc.dot(axis)];
        let cap = if hs.iter().all(|h| (h - lo).abs() <= epsilon) {
            0
        } else if hs.iter().all(|h| (h - hi).abs() <= epsilon) {
            1
        } else {
            walls.push([*pa, *pb, *pc]);
            continue;
        };
        let Some(edges) = caps.get_mut(cap) else { return Ok(None) };
        for (a, b) in [(a, b), (b, c), (c, a)] {
            *edges.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    let raw_key = |p: Vec2| ((p.x / epsilon).floor() as i64, (p.y / epsilon).floor() as i64);
    let mut positions = BTreeMap::<(i64, i64), Vec2>::new();
    let mut projected = Vec::with_capacity(mesh.positions.len());
    for p in &mesh.positions {
        let p = plane.to_local(*p);
        let k = raw_key(p);
        let mut canonical = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                let candidate = (k.0 + dx, k.1 + dy);
                if positions.get(&candidate).is_some_and(|q| q.dist(p) <= epsilon) {
                    canonical = Some(candidate);
                }
            }
        }
        let canonical = canonical.unwrap_or(k);
        positions.entry(canonical).or_insert(p);
        projected.push(canonical);
    }
    let boundary = |cap: usize| -> Option<BTreeMap<(i64, i64), Vec<(i64, i64)>>> {
        let mut adjacency = BTreeMap::<_, Vec<_>>::new();
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
            neighbours.sort();
        }
        Some(adjacency)
    };
    let (Some(adjacency), Some(other)) = (boundary(0), boundary(1)) else { return Ok(None) };
    let well_formed = |a: &BTreeMap<(i64, i64), Vec<(i64, i64)>>| !a.is_empty() && a.len() <= MAX_BOUNDARY && a.values().all(|n| n.len() == 2);
    if !well_formed(&adjacency) || !well_formed(&other) {
        return Ok(None);
    }
    let point = |k: (i64, i64)| Vec2::new(k.0 as f64 * epsilon, k.1 as f64 * epsilon);
    let Some((raw_loops, loops)) = cap_loops(adjacency, &positions, point, tolerance, epsilon) else { return Ok(None) };
    // The two caps are one section: the same number of loops, and each cap's boundary nodes on
    // the other's fitted loops. A mesh need not sample them alike (a mesher that refines a side
    // splits only that side's edge), so they are compared as curves, within the tolerance.
    let Some((other_raw, other_loops)) = cap_loops(other, &positions, point, tolerance, epsilon) else { return Ok(None) };
    let on = |loops: &[Loop2], p: Vec2| loops.iter().flat_map(|l| &l.segs).any(|s| s.point_at(s.closest_param(p)).dist(p) <= tolerance);
    let nodes = |raw: &[Loop2]| raw.iter().flat_map(|l| l.segs.iter().map(|s| s.start())).collect::<Vec<_>>();
    let segments = |l: &[Loop2]| l.iter().map(|l| l.segs.len()).sum::<usize>();
    let work = (nodes(&raw_loops).len().saturating_add(nodes(&other_raw).len()).saturating_add(walls.len().saturating_mul(4)))
        .saturating_mul(segments(&loops).max(segments(&other_loops)));
    if work > 32_000_000 {
        return Err(invalid("boundary certification exceeds its work budget"));
    }
    if other_loops.len() != loops.len()
        || !nodes(&other_raw).iter().all(|p| on(&loops, *p))
        || !nodes(&raw_loops).iter().all(|p| on(&other_loops, *p))
    {
        return Ok(None);
    }
    // Every side triangle lies on the straight extrusion of the section: its corners and its
    // centroid, projected along the axis, on the fitted loops within the tolerance. A mesh with
    // nodes inside its walls (a mesher's lattice, as on a quality mesh) passes; a twisted,
    // tapered or stepped one does not.
    for [a, b, c] in &walls {
        let centroid = (*a + *b + *c) / 3.;
        if ![*a, *b, *c, centroid].iter().all(|p| on(&loops, plane.to_local(*p))) {
            return Ok(None);
        }
    }
    certify_nesting(&raw_loops, &loops, epsilon, bounds.size().len())?;
    // Nest by the original polygon, so smoothing cannot change which loop is a hole.
    let mut regions: Vec<Region2> = Vec::new();
    for (i, lp) in loops.iter().enumerate() {
        let Some(probe) = raw_loops.get(i).and_then(|l| l.segs.first()).map(|s| s.start()) else { return Ok(None) };
        let owners: Vec<_> = raw_loops.iter().enumerate().filter(|(j, other)| *j != i && other.contains(probe)).collect();
        if owners.len() % 2 == 0 {
            regions.push(Region2 { outer: lp.ccw(), holes: Vec::new() });
        }
    }
    for (i, lp) in loops.iter().enumerate() {
        let Some(probe) = raw_loops.get(i).and_then(|l| l.segs.first()).map(|s| s.start()) else { return Ok(None) };
        let owners = raw_loops.iter().enumerate().filter(|(j, other)| *j != i && other.contains(probe)).count();
        if owners % 2 == 1 {
            let owner = regions
                .iter()
                .enumerate()
                .filter(|(_, r)| r.outer.contains(probe))
                .min_by(|(_, a), (_, b)| a.outer.signed_area().abs().total_cmp(&b.outer.signed_area().abs()))
                .map(|(i, _)| i);
            let Some(region) = owner.and_then(|i| regions.get_mut(i)) else { return Ok(None) };
            region.holes.push(lp.ccw().reversed());
        }
    }
    let body = crate::extrude_planar_regions(&plane, &regions, 0., height)?;
    if !body.validity().is_empty() {
        return Err(invalid("reconstructed solid did not pass validity checks"));
    }
    let perimeter: f64 = loops.iter().flat_map(|lp| &lp.segs).map(|s| s.length()).sum();
    let original = mesh.to_mesh().measure().volume;
    let rebuilt = crate::measure(&body)?.volume;
    if !original.is_finite() || original <= 0. || (rebuilt - original).abs() > perimeter * height * tolerance + epsilon * bounds.size().len().powi(2)
    {
        return Err(invalid("reconstructed volume is inconsistent with the source mesh"));
    }
    Ok(Some(body))
}

/// A cap's boundary (`adjacency`: each boundary node's two neighbours, by its key in the section
/// plane) walked into loops: each loop's polygon through the nodes (`positions`, else the key's
/// point) and its fitted form ([`fit_loop`]). `None` for a boundary that does not walk into
/// closed loops of three nodes or more.
fn cap_loops(
    mut adjacency: BTreeMap<(i64, i64), Vec<(i64, i64)>>,
    positions: &BTreeMap<(i64, i64), Vec2>,
    point: impl Fn((i64, i64)) -> Vec2,
    tolerance: f64,
    epsilon: f64,
) -> Option<(Vec<Loop2>, Vec<Loop2>)> {
    let mut loops = Vec::new();
    let mut raw_loops = Vec::new();
    while let Some((&start, _)) = adjacency.first_key_value() {
        let mut current = start;
        let mut previous = None;
        let mut chain = Vec::new();
        for _ in 0..=MAX_BOUNDARY {
            let neighbours = adjacency.remove(&current)?;
            chain.push(*positions.get(&current).unwrap_or(&point(current)));
            let next = neighbours.into_iter().find(|n| Some(*n) != previous)?;
            if next == start {
                break;
            }
            previous = Some(current);
            current = next;
        }
        if chain.len() < 3 || current == start {
            return None;
        }
        raw_loops.push(Loop2::polygon(&chain));
        loops.push(fit_loop(&chain, tolerance, epsilon));
    }
    Some((raw_loops, loops))
}

/// The mesh as one sewn faceted solid (#358): each run of coplanar neighbour triangles fused
/// into a single planar face, every other triangle a face of its own. Every face boundary runs
/// along mesh edges, so the faces close into the same shell the mesh is and no surface the mesh
/// does not describe is invented — unlike [`extrusion`], which certifies analytic cylinders.
fn faceted_solid(mesh: &crate::meshbody::TriMesh) -> Result<Body> {
    let bounds = mesh.positions.iter().fold(solidcraft_geom::Aabb3::EMPTY, |mut b, p| {
        b.add(*p);
        b
    });
    let size = bounds.size().len();
    let at = |i: u32| mesh.positions.get(i as usize).copied();
    // Every side of the closed, consistently oriented mesh joins two triangles, which the
    // checks in `convert_prismatic_mesh` have already certified; the faceted path relies on it.
    let mut touching: BTreeMap<(u32, u32), Vec<usize>> = BTreeMap::new();
    for (i, t) in mesh.triangles.iter().enumerate() {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            touching.entry((a.min(b), a.max(b))).or_default().push(i);
        }
    }
    if touching.values().any(|ts| ts.len() != 2) {
        return Err(invalid("mesh must be closed and consistently oriented, with two triangles per edge"));
    }
    // Coplanar groups grow from a seed triangle's plane, never from the neighbour's own, so a
    // chain of triangles each nearly on the next one's plane cannot drift off the group's
    // plane: the whole group stays within COPLANAR of one plane.
    let mut groups = vec![usize::MAX; mesh.triangles.len()];
    let mut planes: Vec<(Vec3, f64)> = Vec::new();
    let mut queue: Vec<usize> = Vec::new();
    for seed in 0..mesh.triangles.len() {
        if groups[seed] != usize::MAX {
            continue;
        }
        let [a, b, c] = mesh.triangles[seed];
        let (Some(pa), Some(pb), Some(pc)) = (at(a), at(b), at(c)) else { return Err(invalid("missing triangle vertex")) };
        let Some(n) = (pb - pa).cross(pc - pa).normalized() else { return Err(invalid("degenerate triangle")) };
        let group = planes.len();
        planes.push((n, n.dot(pa)));
        groups[seed] = group;
        queue.push(seed);
        while let Some(t) = queue.pop() {
            let [a, b, c] = mesh.triangles[t];
            for (a, b) in [(a, b), (b, c), (c, a)] {
                for &u in touching.get(&(a.min(b), a.max(b))).into_iter().flatten() {
                    if groups[u] != usize::MAX {
                        continue;
                    }
                    let (n, d) = planes[group];
                    if mesh.triangles[u].iter().all(|&v| at(v).is_some_and(|p| (n.dot(p) - d).abs() <= COPLANAR)) {
                        groups[u] = group;
                        queue.push(u);
                    }
                }
            }
        }
    }
    // A group's boundary sides are those its triangles use once: the partner triangle belongs
    // to another group. The mesh's own winding directs them, which orients each face's loops
    // about its outward normal.
    let mut uses: BTreeMap<(usize, (u32, u32)), Vec<(u32, u32)>> = BTreeMap::new();
    for (i, t) in mesh.triangles.iter().enumerate() {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            uses.entry((groups[i], (a.min(b), a.max(b)))).or_default().push((a, b));
        }
    }
    let mut faces: Vec<Vec<(u32, u32)>> = vec![Vec::new(); planes.len()];
    for ((group, _), directed) in &uses {
        if directed.len() == 1 {
            faces[*group].push(directed[0]);
        }
    }
    // The triangle that runs each directed side, for the boundary walk's turn around a vertex.
    let mut owner: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    for (i, t) in mesh.triangles.iter().enumerate() {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            owner.insert((a, b), i);
        }
    }
    let mut specs: Vec<FaceSpec> = Vec::with_capacity(faces.len());
    for (group, sides) in faces.iter().enumerate() {
        let loops = face_loops(sides, planes[group].0, &mesh.positions, &mesh.triangles, &owner)?;
        specs.push(FaceSpec { loops, surface: SurfSpec::Plane, probe: None });
    }
    let body = sew_touching(&specs)?;
    if !body.validity().is_empty() {
        return Err(invalid("reconstructed solid did not pass validity checks"));
    }
    // The sewn solid's faces are the mesh's own facets, so its volume is the mesh's; a seam
    // that does not close the same shell shows up here.
    let original = mesh.to_mesh().measure().volume;
    let rebuilt = crate::measure(&body)?.volume;
    if !original.is_finite() || original <= 0. || (rebuilt - original).abs() > original.abs() * 1e-6 + COPLANAR * size * size {
        return Err(invalid("reconstructed volume is inconsistent with the source mesh"));
    }
    Ok(body)
}

/// One group's boundary loops as face wire edges: each directed side is followed by rotating
/// from its triangle around the shared vertex through the group's interior triangles until the
/// next boundary side leaves it, which keeps a loop on one strand of material even where
/// several strands meet at a vertex. Each side becomes exactly one edge — the two faces that
/// share a rim run it in opposite directions, so fusing collinear runs of vertices greedily
/// could leave each face with a different subset and edges that pair with nothing. Points that
/// coincide within 1e-9 mm are fused (sewing could not tell the ends apart anyway), and the
/// loop that runs counter-clockwise comes first, where sewing reads the face's outward
/// direction from it.
fn face_loops(
    sides: &[(u32, u32)],
    normal: Vec3,
    positions: &[Vec3],
    triangles: &[[u32; 3]],
    owner: &BTreeMap<(u32, u32), usize>,
) -> Result<Vec<Vec<EdgeSpec>>> {
    let at = |i: u32| positions.get(i as usize).copied().ok_or_else(|| invalid("missing triangle vertex"));
    let boundary: std::collections::BTreeSet<(u32, u32)> = sides.iter().copied().collect();
    let mut used: std::collections::BTreeSet<(u32, u32)> = std::collections::BTreeSet::new();
    let mut chains: Vec<Vec<Vec3>> = Vec::new();
    for &(start_side, into) in sides {
        if used.contains(&(start_side, into)) {
            continue;
        }
        used.insert((start_side, into));
        let (mut previous, mut current) = (start_side, into);
        let mut chain = vec![at(previous)?];
        loop {
            chain.push(at(current)?);
            if chain.len() > sides.len() + 1 {
                return Err(invalid("a planar group's boundary does not close"));
            }
            // Rotate from the incoming side's triangle around `current` through the triangles
            // between the two boundary sides that meet there; interior sides are shared with a
            // neighbour of the same group, so the walk stays on this group's material.
            let mut side = (previous, current);
            let mut spin = 0usize;
            let next = loop {
                spin += 1;
                if spin > triangles.len() {
                    return Err(invalid("a vertex with no way around its triangles"));
                }
                let Some(&t) = owner.get(&side) else { return Err(invalid("a side without its triangle")) };
                let [x, y, z] = triangles[t];
                let third = if x != side.0 && x != side.1 {
                    x
                } else if y != side.0 && y != side.1 {
                    y
                } else {
                    z
                };
                if boundary.contains(&(current, third)) {
                    break third;
                }
                side = (third, current);
            };
            used.insert((current, next));
            previous = current;
            if next == start_side {
                break;
            }
            current = next;
        }
        if chain.len() < 3 {
            return Err(invalid("a planar group's boundary has no loop"));
        }
        chains.push(chain);
    }
    let mut loops: Vec<Vec<EdgeSpec>> = Vec::with_capacity(chains.len());
    for chain in &mut chains {
        loop {
            let n = chain.len();
            if n <= 3 {
                break;
            }
            let remove = (0..n).find(|&i| chain[i].dist(chain[(i + n - 1) % n]) <= 1e-9 || chain[i].dist(chain[(i + 1) % n]) <= 1e-9);
            let Some(i) = remove else { break };
            chain.remove(i);
        }
        let wire: Vec<EdgeSpec> = chain.iter().zip(chain.iter().skip(1).chain(chain.first())).map(|(&a, &b)| EdgeSpec::Line { a, b }).collect();
        loops.push(wire);
    }
    // The outer loop runs counter-clockwise about the outward normal and comes first: sewing
    // takes the face's outward direction from the first loop's plane.
    let signed = |wire: &Vec<EdgeSpec>| -> f64 {
        wire.iter()
            .map(|e| match e {
                EdgeSpec::Line { a, b } => a.cross(*b),
                _ => Vec3::ZERO,
            })
            .fold(Vec3::ZERO, |sum, cross| sum + cross)
            .dot(normal)
    };
    let outer = loops.iter().enumerate().find(|(_, l)| signed(l) > 0.).map(|(i, _)| i);
    let Some(outer) = outer else {
        return Err(invalid("a planar group's boundary does not enclose it"));
    };
    if outer != 0 {
        loops.swap(0, outer);
    }
    Ok(loops)
}

fn certify_nesting(raw: &[Loop2], fitted: &[Loop2], epsilon: f64, size: f64) -> Result<()> {
    if raw.len() > MAX_LOOPS {
        return Err(KernelError::Invalid(format!(
            "mesh conversion: too complex to convert ({} boundary loops; at most {MAX_LOOPS} are supported)",
            raw.len()
        )));
    }
    let mut work = 0usize;
    for (i, a) in raw.iter().enumerate() {
        for (j, b) in raw.iter().enumerate().skip(i + 1) {
            work = work.saturating_add(a.segs.len().saturating_mul(b.segs.len()));
            if work > 16_000_000 {
                return Err(invalid("loop certification exceeds its work budget"));
            }
            let contains = |a: &Loop2, b: &Loop2| b.segs.first().is_some_and(|s| a.contains(s.start()));
            let (ab, ba) = (contains(a, b), contains(b, a));
            if ab && ba {
                return Err(invalid("overlapping boundary loops"));
            }
            for (a, b) in
                [(a, b), (fitted.get(i).ok_or_else(|| invalid("missing fitted loop"))?, fitted.get(j).ok_or_else(|| invalid("missing fitted loop"))?)]
            {
                let region = |l: &Loop2| Region2 { outer: l.ccw(), holes: Vec::new() };
                let overlap = crate::planar_regions_boolean(&[region(a)], &[region(b)], crate::BoolOp::Intersect, epsilon.clamp(1e-9, 1.))?;
                let area: f64 = overlap.iter().map(Region2::area).sum();
                let expected = if ab {
                    b.signed_area().abs()
                } else if ba {
                    a.signed_area().abs()
                } else {
                    0.
                };
                if (area - expected).abs() > epsilon * size.max(1.) * 10. {
                    return Err(invalid("conversion would cross or merge boundary loops"));
                }
            }
        }
    }
    Ok(())
}

fn fit_loop(points: &[Vec2], tolerance: f64, epsilon: f64) -> Loop2 {
    // Fit a complete circular boundary only with enough samples to distinguish it
    // from a polygon. Certify the chord deviation, not just the vertex residual.
    if points.len() >= 12 {
        let (Some(&a), Some(&b), Some(&c)) = (points.first(), points.get(points.len() / 3), points.get(2 * points.len() / 3)) else {
            return Loop2::polygon(points);
        };
        let u = b - a;
        let v = c - a;
        let d = 2. * u.cross(v);
        if d.abs() > epsilon * epsilon {
            let center = a + Vec2::new(v.y * u.len2() - u.y * v.len2(), u.x * v.len2() - v.x * u.len2()) / d;
            let radius = center.dist(a);
            if radius.is_finite() && radius > epsilon && points.iter().all(|p| (center.dist(*p) - radius).abs() <= epsilon * 3.) {
                let mut sweep = 0.;
                let mut direction = 0f64;
                let valid = (0..points.len()).all(|i| {
                    let (Some(a), Some(b)) = (points.get(i), points.get((i + 1) % points.len())) else { return false };
                    let signed = ((*a - center).cross(*b - center)).atan2((*a - center).dot(*b - center));
                    if direction == 0. {
                        direction = signed.signum();
                    }
                    if signed * direction <= 0. {
                        return false;
                    }
                    sweep += signed;
                    let angle = signed.abs();
                    angle <= std::f64::consts::PI / 6. + 1e-8 && radius * (1. - (angle * 0.5).cos()) + epsilon * 10. <= tolerance
                });
                if valid && (sweep.abs() - std::f64::consts::TAU).abs() <= 1e-7 {
                    return Loop2::circle(center, radius);
                }
            }
        }
    }
    // Drop the nodes that lie on a straight run: a node goes where every node between its kept
    // neighbours, those dropped before it included, lies within the rounding allowance (three
    // times `epsilon`, as STL's f32 coordinates are allowed for elsewhere) of the line between
    // them. Measured against the original nodes, so dropping one after another never drifts: a
    // mesher that samples a straight edge with nodes along it (a quality mesh does) gives the
    // edge's one segment.
    let allowance = epsilon * 3.;
    let n = points.len();
    let mut kept: Vec<usize> = (0..n).collect();
    // The nodes from `from` to `to` going forward round the loop, both excluded.
    let between = |from: usize, to: usize| (1..n).map(move |k| (from + k) % n).take_while(move |&i| i != to);
    // At most a budget of node tests: the boundary is bounded (`MAX_BOUNDARY`), and so is this.
    let mut budget = 16_000_000usize;
    // Passes until one drops nothing: dropping a node can make its kept neighbour's run straight.
    let mut dropped = true;
    while dropped {
        dropped = false;
        let mut k = 0usize;
        while kept.len() > 3 && k < kept.len() {
            let m = kept.len();
            let (Some(&p), Some(&q)) = (kept.get((k + m - 1) % m), kept.get((k + 1) % m)) else { break };
            let (Some(&a), Some(&c)) = (points.get(p), points.get(q)) else { break };
            let edge = solidcore_record::Seg2::Line { a, b: c };
            let mut straight = true;
            for i in between(p, q) {
                if budget == 0 {
                    straight = false;
                    break;
                }
                budget -= 1;
                let Some(&x) = points.get(i) else { continue };
                if edge.point_at(edge.closest_param(x)).dist(x) > allowance {
                    straight = false;
                    break;
                }
            }
            if straight {
                kept.remove(k);
                dropped = true;
            } else {
                k += 1;
            }
        }
    }
    Loop2::polygon(&kept.iter().filter_map(|&i| points.get(i).copied()).collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh_of(body: &Body) -> Body {
        let mesh = body.mesh(&crate::MeshTolerance::chord(0.005)).unwrap();
        crate::mesh_body(&mesh.positions, &mesh.triangles).unwrap()
    }

    #[test]
    fn reconstructs_planes_holes_and_round_walls_in_arbitrary_orientation() {
        let region = Region2 {
            outer: Loop2::polygon(&[
                Vec2::new(0., 0.),
                Vec2::new(40., 0.),
                Vec2::new(40., 20.),
                Vec2::new(20., 20.),
                Vec2::new(20., 30.),
                Vec2::new(0., 30.),
            ]),
            holes: vec![Loop2::circle(Vec2::new(10., 10.), 4.).reversed()],
        };
        let source = crate::extrude_planar_regions(&Plane::XY, &[region], 0., 15.).unwrap();
        let source = crate::transform(&source, Vec3::new(31., -17., 9.), Vec3::ZERO, Vec3::new(1., 2., 3.), 0.73).unwrap();
        let converted = convert_prismatic_mesh(&mesh_of(&source), 0.05).unwrap();
        assert!(!converted.is_mesh());
        assert!(converted.validity().is_empty());
        let expected = (1000. - std::f64::consts::PI * 16.) * 15.;
        let fine_volume = |body: &Body| body.mesh(&crate::MeshTolerance::chord(1e-5)).unwrap().measure().volume;
        assert!((fine_volume(&converted) - expected).abs() < 0.02);
        assert!((crate::measure(&converted).unwrap().volume - crate::measure(&source).unwrap().volume).abs() < 1e-7);
        assert_eq!(crate::measure(&converted).unwrap().merged.face_types.get("cylinder"), Some(&1));
        assert!(converted.faces(0.01).unwrap().len() < 15);
        let step = crate::step_export(&[&converted], "SolidCraft regression").unwrap();
        let reopened = crate::step_import(&step).unwrap();
        assert!((fine_volume(&reopened.bodies[0].body) - expected).abs() < 0.02);
    }

    /// A quality mesh (the mesh path's, `Body::export_mesh`: lattice nodes inside every wall and
    /// cap, its caps' edges sampled each on their own) of the L with a bore converts as the
    /// display mesh does: its planes, its one cylinder and its volume. Before, the converter took
    /// only side facets running the part's full height and caps sampled alike, so it found no
    /// prism and fell back to the faceted solid.
    #[test]
    fn a_quality_mesh_with_nodes_inside_its_walls_reconstructs_planes_and_the_bore() {
        let region = Region2 {
            outer: Loop2::polygon(&[
                Vec2::new(0., 0.),
                Vec2::new(40., 0.),
                Vec2::new(40., 20.),
                Vec2::new(20., 20.),
                Vec2::new(20., 30.),
                Vec2::new(0., 30.),
            ]),
            holes: vec![Loop2::circle(Vec2::new(10., 10.), 4.).reversed()],
        };
        let source = crate::extrude_planar_regions(&Plane::XY, &[region], 0., 15.).unwrap();
        let spec = crate::MeshSpec::chord(source.size() * 2e-4);
        let quality = source.export_mesh(&spec).unwrap().mesh;
        let walls_inside = quality.positions.iter().filter(|p| p.z > 1e-6 && p.z < 15. - 1e-6).count();
        assert!(walls_inside > 0, "the quality mesh has nodes inside its walls");
        let welded = crate::mesh_body(&quality.positions, &quality.triangles).unwrap();
        let converted = convert_prismatic_mesh(&welded, 0.05).unwrap();
        assert!(converted.validity().is_empty());
        assert_eq!(crate::measure(&converted).unwrap().merged.face_types.get("cylinder"), Some(&1));
        // The L's six sides, the two caps and the bore.
        assert_eq!(converted.faces(0.01).unwrap().len(), 9);
        assert!((crate::measure(&converted).unwrap().volume - crate::measure(&source).unwrap().volume).abs() < 1e-7);
    }

    #[test]
    fn planar_boxes_are_exact_and_coarse_polygons_are_not_rounded() {
        let source = crate::box_solid(Vec3::ZERO, Vec3::new(10., 20., 30.)).unwrap();
        let result = convert_prismatic_mesh(&mesh_of(&source), 0.01).unwrap();
        assert_eq!(result.faces(0.01).unwrap().len(), 6);
        assert!((crate::measure(&result).unwrap().volume - 6000.).abs() < 1e-6);
        let points: Vec<_> = (0..6)
            .map(|i| {
                let angle = i as f64 * std::f64::consts::TAU / 6.;
                Vec2::new(angle.cos(), angle.sin()) * 10.
            })
            .collect();
        assert_eq!(fit_loop(&points, 1., 1e-6).segs.len(), 6);
        let points: Vec<_> = (0..16)
            .map(|i| {
                let angle = i as f64 * std::f64::consts::TAU / 16.;
                Vec2::new(angle.cos(), angle.sin()) * 10.
            })
            .collect();
        assert_eq!(fit_loop(&points, 0.001, 1e-6).segs.len(), 16);
        assert_exact_circle(&fit_loop(&points, 0.2, 1e-6), Vec2::ZERO, 10.);
    }

    /// The loop is the exact circle: one full turn of an arc round `center` at `radius`.
    fn assert_exact_circle(fitted: &Loop2, center: Vec2, radius: f64) {
        let [solidcore_record::Seg2::Arc { center: c, radius: r, sweep, .. }] = fitted.segs.as_slice() else { panic!("not one arc: {fitted:?}") };
        assert!(c.dist(center) < 1e-9 && (r - radius).abs() < 1e-9, "centre {c:?}, radius {r}");
        assert!((sweep.abs() - std::f64::consts::TAU).abs() < 1e-9, "sweep {sweep}");
    }

    #[test]
    fn refuses_open_and_hostile_inputs() {
        let open = crate::mesh_body(&[Vec3::ZERO, Vec3::X, Vec3::Y], &[[0, 1, 2]]).unwrap();
        assert!(convert_prismatic_mesh(&open, 0.05).is_err());
        let solid = crate::box_solid(Vec3::ZERO, Vec3::new(10., 10., 10.)).unwrap();
        assert!(convert_prismatic_mesh(&solid, 0.05).is_err());
        for tolerance in [f64::NAN, f64::INFINITY, -1., 0., 2.] {
            assert!(convert_prismatic_mesh(&mesh_of(&solid), tolerance).is_err());
        }
        for coordinate in [f64::NAN, f64::INFINITY, f64::MAX, 1e30] {
            let mesh = crate::meshbody::TriMesh { positions: vec![Vec3::new(coordinate, 0., 0.)], triangles: vec![], corner_normals: Vec::new() };
            assert!(weld(&mesh, 0.05).is_err());
        }
    }

    /// #358, #12281: a mesh that is no constant-section prism — the section of a rounded form
    /// changes every way you slice it — still converts, as the faceted solid its triangles
    /// describe: one planar face per triangle (or per coplanar run), nothing rounded into
    /// cylinders.
    #[test]
    fn a_rounded_mesh_converts_as_a_faceted_solid() {
        let source = crate::sphere(Vec3::ZERO, 10.).unwrap();
        let mesh = mesh_of(&source);
        let triangles = mesh.mesh.as_ref().unwrap().triangles.len();
        let converted = convert_prismatic_mesh(&mesh, 0.05).unwrap();
        assert!(!converted.is_mesh());
        assert!(converted.validity().is_empty());
        let faces = converted.faces(0.01).unwrap().len();
        assert!(faces <= triangles, "{faces} faces of {triangles} triangles");
        assert_eq!(crate::measure(&converted).unwrap().merged.face_types.get("plane"), Some(&faces));
        let expected = 4. / 3. * std::f64::consts::PI * 1000.;
        let fine_volume = |body: &Body| body.mesh(&crate::MeshTolerance::chord(1e-5)).unwrap().measure().volume;
        // The facets are the tessellation's own chords, so the volume sits just under the
        // sphere's, by the chord tolerance's sagitta.
        let volume = fine_volume(&converted);
        assert!((expected - volume).abs() < expected * 0.01, "{volume} of {expected}");
    }

    /// #358, #12281: a part whose section changes with height — a chamfered box, which no axis
    /// extrudes — converts with one face per flat: the flats the mesh drew, no more.
    #[test]
    fn a_chamfered_part_converts_as_one_face_per_flat() {
        let box_body = crate::box_solid(Vec3::ZERO, Vec3::new(20., 20., 20.)).unwrap();
        let plane = Plane::from_normal(Vec3::new(10., 10., 10.), Vec3::new(1., 1., 1.)).unwrap();
        let (kept, _) = crate::half_space(&box_body, &plane, false).unwrap();
        let source = kept.expect("the box meets the plane");
        let converted = convert_prismatic_mesh(&mesh_of(&source), 0.05).unwrap();
        assert!(!converted.is_mesh());
        assert!(converted.validity().is_empty());
        // The plane clips each of the cube's three flats at the origin to a pentagon and the
        // three at the far corner to a triangle, and adds its own hexagon.
        assert_eq!(converted.faces(0.01).unwrap().len(), 7);
        // The plane passes through the cube's centre, so the kept half is exactly half of it.
        assert!((crate::measure(&converted).unwrap().volume - 4000.).abs() < 1e-6);
        let step = crate::step_export(&[&converted], "SolidCraft regression").unwrap();
        let reopened = crate::step_import(&step).unwrap();
        assert!((crate::measure(&reopened.bodies[0].body).unwrap().volume - 4000.).abs() < 1e-6);
    }

    /// A mesh that subdivides every flat (each flat's ring carrying its edge midpoints,
    /// triangulated from the flat's centre) converts to one face per flat: the groups fuse the
    /// subdivisions, and every boundary side pairs with the neighbouring flat's along the rims
    /// both subdivide (#12281's real-world mesh shape).
    #[test]
    fn a_subdivided_box_converts_to_six_faces() {
        let (w, h, d) = (20., 10., 4.);
        let p = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
        let mut positions: Vec<Vec3> = Vec::new();
        let mut triangles: Vec<[u32; 3]> = Vec::new();
        let mut at = |point: Vec3| -> u32 {
            if let Some(i) = positions.iter().position(|q| *q == point) {
                return i as u32;
            }
            positions.push(point);
            (positions.len() - 1) as u32
        };
        // Each flat is the parallelogram o + s·u + t·v (s, t in 0..1) with u × v outward: its
        // ring of corners and edge midpoints, fanned from its centre, runs counter-clockwise.
        let flat = |o: Vec3, u: Vec3, v: Vec3, at: &mut dyn FnMut(Vec3) -> u32, triangles: &mut Vec<[u32; 3]>| {
            let corners = [o, o + u, o + u + v, o + v];
            let mid = |a: Vec3, b: Vec3, at: &mut dyn FnMut(Vec3) -> u32| at(a + (b - a) * 0.5);
            let ring = [
                at(corners[0]),
                mid(corners[0], corners[1], at),
                at(corners[1]),
                mid(corners[1], corners[2], at),
                at(corners[2]),
                mid(corners[2], corners[3], at),
                at(corners[3]),
                mid(corners[3], corners[0], at),
            ];
            let centre = at(o + (u + v) * 0.5);
            for i in 0..ring.len() {
                triangles.push([centre, ring[i], ring[(i + 1) % ring.len()]]);
            }
        };
        flat(p(0., 0., 0.), p(0., h, 0.), p(w, 0., 0.), &mut at, &mut triangles); // z = 0
        flat(p(0., 0., d), p(w, 0., 0.), p(0., h, 0.), &mut at, &mut triangles); // z = d
        flat(p(0., 0., 0.), p(0., 0., d), p(0., h, 0.), &mut at, &mut triangles); // x = 0
        flat(p(w, 0., 0.), p(0., h, 0.), p(0., 0., d), &mut at, &mut triangles); // x = w
        flat(p(0., 0., 0.), p(w, 0., 0.), p(0., 0., d), &mut at, &mut triangles); // y = 0
        flat(p(0., h, 0.), p(0., 0., d), p(w, 0., 0.), &mut at, &mut triangles); // y = h
        let body = crate::mesh_body(&positions, &triangles).unwrap();
        let converted = convert_prismatic_mesh(&body, 0.05).unwrap();
        assert!(!converted.is_mesh());
        assert!(converted.validity().is_empty());
        assert_eq!(converted.faces(0.01).unwrap().len(), 6);
        assert!((crate::measure(&converted).unwrap().volume - w * h * d).abs() < 1e-6);
    }

    /// #4561: a closed box mesh with one extra triangle of no area along its bottom front edge
    /// ((0,0,0), (10,0,0), (20,0,0)) converts to the box: the triangle adds no surface and is
    /// dropped, its middle corner with it. V = 20 * 10 * 5 = 1000, six faces.
    #[test]
    fn a_zero_area_triangle_is_dropped() {
        let source = crate::box_solid(Vec3::ZERO, Vec3::new(20., 10., 5.)).unwrap();
        let mesh = source.mesh(&crate::MeshTolerance::chord(0.005)).unwrap();
        let mut positions = mesh.positions.clone();
        let mut triangles = mesh.triangles.clone();
        let at = |ps: &[Vec3], p: Vec3| ps.iter().position(|q| q.dist(p) < 1e-9).and_then(|i| u32::try_from(i).ok()).unwrap();
        let (a, b) = (at(&positions, Vec3::ZERO), at(&positions, Vec3::new(20., 0., 0.)));
        positions.push(Vec3::new(10., 0., 0.));
        let m = u32::try_from(positions.len() - 1).unwrap();
        triangles.push([a, m, b]);
        let body = crate::mesh_body(&positions, &triangles).unwrap();
        let result = convert_prismatic_mesh(&body, 0.01).unwrap();
        assert_eq!(result.faces(0.01).unwrap().len(), 6);
        assert!((crate::measure(&result).unwrap().volume - 1000.).abs() < 1e-6);
        // A mesh of nothing but a triangle of no area has nothing left to convert.
        let sliver = crate::mesh_body(&[Vec3::ZERO, Vec3::X, Vec3::new(2., 0., 0.)], &[[0, 1, 2]]).unwrap();
        let refused = convert_prismatic_mesh(&sliver, 0.05).unwrap_err();
        assert!(refused.to_string().contains("no triangle of any area"), "{refused}");
    }

    #[test]
    fn a_mesh_over_the_budget_says_how_big_it_is() {
        // #346: a mesh just over the limit (a strip of 50001 triangles) is refused with its
        // size and the limit.
        let n = MAX_TRIANGLES + 1;
        let positions: Vec<Vec3> = (0..n + 2).map(|k| Vec3::new((k / 2) as f64, (k % 2) as f64, 0.)).collect();
        let triangles: Vec<[u32; 3]> = (0..n as u32).map(|i| [i, i + 1, i + 2]).collect();
        let body = crate::mesh_body(&positions, &triangles).unwrap();
        let err = convert_prismatic_mesh(&body, 0.05).unwrap_err().to_string();
        assert!(err.contains("50001 triangles") && err.contains("50000"), "{err}");
        let mesh = crate::meshbody::TriMesh { positions: vec![Vec3::ZERO; MAX_TRIANGLES * 3 + 1], triangles: vec![], corner_normals: Vec::new() };
        let err = weld(&mesh, 0.05).unwrap_err().to_string();
        assert!(err.contains("150001 vertices") && err.contains("150000"), "{err}");
    }

    #[test]
    fn fitting_cannot_open_a_hole_through_a_nearby_wall() {
        let outer = Loop2::polygon(&[Vec2::new(-1., -1.), Vec2::new(1., -1.), Vec2::new(1., 1.), Vec2::new(-1., 1.)]);
        let points: Vec<_> = (0..12)
            .map(|i| {
                let angle = (i as f64 + 0.5) * std::f64::consts::TAU / 12.;
                Vec2::new(angle.cos(), angle.sin()) * 1.02
            })
            .collect();
        let inner = Loop2::polygon(&points);
        let fitted = fit_loop(&points, 0.05, 1e-6);
        assert_exact_circle(&fitted, Vec2::ZERO, 1.02);
        assert!(certify_nesting(&[outer.clone(), inner], &[outer, fitted], 1e-6, 3.).is_err());
    }

    fn perforated_plate(rows: usize, cols: usize) -> Body {
        let w = cols as f64 * 4. + 2.;
        let h = rows as f64 * 4. + 2.;
        let holes = (0..rows)
            .flat_map(|r| (0..cols).map(move |c| (r, c)))
            .map(|(r, c)| {
                let (x, y) = (c as f64 * 4. + 2., r as f64 * 4. + 2.);
                Loop2::polygon(&[Vec2::new(x, y), Vec2::new(x + 2., y), Vec2::new(x + 2., y + 2.), Vec2::new(x, y + 2.)]).reversed()
            })
            .collect();
        let outer = Loop2::polygon(&[Vec2::new(0., 0.), Vec2::new(w, 0.), Vec2::new(w, h), Vec2::new(0., h)]);
        crate::extrude_planar_regions(&Plane::XY, &[Region2 { outer, holes }], 0., 3.).unwrap()
    }

    #[test]
    fn many_holes_are_refused_promptly_instead_of_stalling() {
        // 400 holes would be ~80k loop pairs and ~160k planar booleans.
        let mesh = mesh_of(&perforated_plate(20, 20));
        let start = std::time::Instant::now();
        let err = convert_prismatic_mesh(&mesh, 0.05).unwrap_err().to_string();
        assert!(err.contains("too complex to convert"), "{err}");
        assert!(start.elapsed() < std::time::Duration::from_secs(10), "took {:?}", start.elapsed());
        // At the limit, a plate still converts in bounded time.
        let mesh = mesh_of(&perforated_plate(7, 7));
        let result = convert_prismatic_mesh(&mesh, 0.05).unwrap();
        assert!((crate::measure(&result).unwrap().volume - (30. * 30. - 49. * 4.) * 3.).abs() < 1e-6);
    }
}
