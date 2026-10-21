//! Group disconnected material regions without moving a cavity to another enclosing shell.
//! A valid shell hierarchy alternates material and void at every nesting level.

use solidcraft_geom::{Aabb3, BoxedTriangle, Mesh, Vec3, find_close_pair};

use crate::{KernelError, Result};

pub(super) const MAX_SHELLS: usize = 1024;
pub(super) const MAX_POINTS: usize = 100_000;
pub(super) const MAX_TRIANGLES: usize = 100_000;
const MAX_PAIR_WORK: usize = 40_000_000;
const MAX_PROBES: usize = 2_000_000;
const MAX_CONTAINMENT_WORK: usize = 40_000_000;

fn failed(detail: &str) -> KernelError {
    KernelError::Failed(format!("lumps: {detail}"))
}

fn box_of(points: &[Vec3]) -> Aabb3 {
    let mut bounds = Aabb3::EMPTY;
    for p in points {
        bounds.add(*p);
    }
    bounds
}

pub(crate) fn boxes_separate(a: Aabb3, b: Aabb3, gap: f64) -> bool {
    a.max.x < b.min.x - gap
        || b.max.x < a.min.x - gap
        || a.max.y < b.min.y - gap
        || b.max.y < a.min.y - gap
        || a.max.z < b.min.z - gap
        || b.max.z < a.min.z - gap
}

fn unit(v: Vec3) -> Option<Vec3> {
    let scale = v.x.abs().max(v.y.abs()).max(v.z.abs());
    if !scale.is_finite() || scale == 0.0 {
        return None;
    }
    (v / scale).normalized()
}

/// Separating axes include the in-plane edge normals for coplanar triangles.
fn triangles_meet(a: &[Vec3; 3], b: &[Vec3; 3], gap: f64) -> bool {
    let [a0, a1, a2] = *a;
    let [b0, b1, b2] = *b;
    let ea = [a1 - a0, a2 - a1, a0 - a2];
    let eb = [b1 - b0, b2 - b1, b0 - b2];
    let na = (a1 - a0).cross(a2 - a0);
    let nb = (b1 - b0).cross(b2 - b0);
    let separated = |axis: Vec3| {
        let Some(n) = unit(axis) else { return false };
        // Translate projections first so a far-away body does not lose its clearance.
        let range = |points: &[Vec3; 3]| {
            points.iter().map(|p| (*p - a0).dot(n)).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(x), hi.max(x)))
        };
        let ((alo, ahi), (blo, bhi)) = (range(a), range(b));
        ahi < blo - gap || bhi < alo - gap
    };
    if separated(na) || separated(nb) {
        return false;
    }
    for e in ea {
        if separated(na.cross(e)) {
            return false;
        }
        for f in eb {
            if separated(e.cross(f)) {
                return false;
            }
        }
    }
    for e in eb {
        if separated(nb.cross(e)) {
            return false;
        }
    }
    true
}

/// Whether two closed meshes share material or touch: a boundary triangle of one comes within
/// `gap` of one of the other's, or one encloses the other.
pub(super) fn meshes_meet(a: &Mesh, b: &Mesh, gap: f64) -> Result<bool> {
    if !gap.is_finite() || gap <= 0.0 {
        return Err(failed("invalid contact clearance"));
    }
    let (ba, bb) = (a.bounds(), b.bounds());
    if ba.is_empty() || bb.is_empty() || boxes_separate(ba, bb, gap) {
        return Ok(false);
    }
    // Only triangles near the other body can touch it.
    let near = |m: &Mesh, other: Aabb3| -> Result<Vec<BoxedTriangle>> {
        let mut out = Vec::new();
        for t in &m.triangles {
            let tri = m.tri(t).ok_or_else(|| failed("invalid contact triangle"))?;
            let bounds = box_of(&tri);
            if !boxes_separate(bounds, other, gap) {
                out.push((tri, bounds));
            }
        }
        Ok(out)
    };
    let (ta, tb) = (near(a, bb)?, near(b, ba)?);
    let mut budget = MAX_PAIR_WORK;
    if find_close_pair(&ta, &tb, gap, &mut budget, |(x, _), (y, _)| triangles_meet(x, y, gap))
        .ok_or_else(|| failed("contact work exceeds the limit"))?
    {
        return Ok(true);
    }
    // No boundary contact: they meet only if one lies wholly inside the other.
    Ok(b.positions.first().is_some_and(|p| a.contains(*p)) || a.positions.first().is_some_and(|p| b.contains(*p)))
}

pub(super) fn groups(meshes: &[Mesh], volumes: &[f64], gap: f64) -> Result<Vec<Vec<usize>>> {
    let parent = parents(meshes, volumes, gap)?;
    let mut groups = Vec::new();
    for (i, v) in volumes.iter().enumerate() {
        if *v <= 0.0 {
            continue;
        }
        let mut group = vec![i];
        group.extend(parent.iter().enumerate().filter_map(|(j, p)| (*p == Some(i)).then_some(j)));
        groups.push(group);
    }
    Ok(groups)
}

/// The shell directly around each closed shell (`None` for one in the open): outward shells
/// (positive volume) bound material, inward ones voids, and nesting alternates them.
pub(super) fn parents(meshes: &[Mesh], volumes: &[f64], gap: f64) -> Result<Vec<Option<usize>>> {
    if meshes.is_empty() || meshes.len() != volumes.len() || meshes.len() > MAX_SHELLS {
        return Err(failed("shell count exceeds the grouping limit or is invalid"));
    }
    if !gap.is_finite() || gap <= 0.0 {
        return Err(failed("invalid shell clearance"));
    }
    let mut points = 0usize;
    let mut triangle_count = 0usize;
    let mut triangles = Vec::with_capacity(meshes.len());
    let mut bounds = Vec::with_capacity(meshes.len());
    for (mesh, volume) in meshes.iter().zip(volumes) {
        points = points.checked_add(mesh.positions.len()).ok_or_else(|| failed("point count exceeds the grouping limit"))?;
        triangle_count = triangle_count.checked_add(mesh.triangles.len()).ok_or_else(|| failed("triangle count exceeds the grouping limit"))?;
        if points > MAX_POINTS || triangle_count > MAX_TRIANGLES {
            return Err(failed("mesh exceeds the shell grouping limit"));
        }
        if !volume.is_finite()
            || *volume == 0.0
            || mesh.positions.is_empty()
            || mesh.triangles.is_empty()
            || mesh.positions.iter().any(|p| !p.is_finite())
        {
            return Err(failed("cannot determine a closed shell's orientation"));
        }
        let tris: Vec<_> = mesh.triangles.iter().map(|t| mesh.tri(t).ok_or_else(|| failed("invalid shell triangle"))).collect::<Result<_>>()?;
        if tris.iter().any(|[a, b, c]| unit((*b - *a).cross(*c - *a)).is_none()) {
            return Err(failed("degenerate shell triangle"));
        }
        triangles.push(tris.into_iter().map(|t| (t, box_of(&t))).collect::<Vec<BoxedTriangle>>());
        bounds.push(mesh.bounds());
    }
    let mut inside = vec![vec![false; meshes.len()]; meshes.len()];
    let mut pair_budget = MAX_PAIR_WORK;
    let mut probes = 0usize;
    let mut containment_work = 0usize;
    for (i, mesh) in meshes.iter().enumerate() {
        for j in i + 1..meshes.len() {
            let (Some(a_bounds), Some(b_bounds)) = (bounds.get(i), bounds.get(j)) else { return Err(failed("missing shell bounds")) };
            if boxes_separate(*a_bounds, *b_bounds, gap) {
                continue;
            }
            let (Some(a), Some(b)) = (triangles.get(i), triangles.get(j)) else { return Err(failed("missing shell triangles")) };
            if find_close_pair(a, b, gap, &mut pair_budget, |(x, _), (y, _)| triangles_meet(x, y, gap))
                .ok_or_else(|| failed("shell boundary work exceeds the grouping limit"))?
            {
                return Err(failed("shell boundaries intersect or touch; cavity ownership is ambiguous"));
            }
            for (inner, outer) in [(i, j), (j, i)] {
                let ib = bounds.get(inner).ok_or_else(|| failed("missing shell bounds"))?;
                let ob = bounds.get(outer).ok_or_else(|| failed("missing shell bounds"))?;
                // With boundary intersections already excluded, a connected shell
                // extending beyond another's bounds cannot be wholly contained.
                if ib.min.x < ob.min.x - gap
                    || ib.min.y < ob.min.y - gap
                    || ib.min.z < ob.min.z - gap
                    || ib.max.x > ob.max.x + gap
                    || ib.max.y > ob.max.y + gap
                    || ib.max.z > ob.max.z + gap
                {
                    continue;
                }
                let inner_mesh = if inner == i { mesh } else { meshes.get(inner).ok_or_else(|| failed("missing shell"))? };
                probes = probes.checked_add(inner_mesh.positions.len()).ok_or_else(|| failed("shell probes exceed the grouping limit"))?;
                if probes > MAX_PROBES {
                    return Err(failed("shell probes exceed the grouping limit"));
                }
                let outer_mesh = meshes.get(outer).ok_or_else(|| failed("missing shell"))?;
                let probe_count = inner_mesh.positions.len();
                let work = probe_count
                    .checked_mul(outer_mesh.triangles.len())
                    .and_then(|n| n.checked_mul(3))
                    .ok_or_else(|| failed("shell containment work exceeds the grouping limit"))?;
                containment_work = containment_work.checked_add(work).ok_or_else(|| failed("shell containment work exceeds the grouping limit"))?;
                if containment_work > MAX_CONTAINMENT_WORK {
                    return Err(failed("shell containment work exceeds the grouping limit"));
                }
                let mut contained = None;
                for p in inner_mesh.positions.iter().copied() {
                    let hit = outer_mesh.contains(p);
                    if contained.is_some_and(|previous| previous != hit) {
                        return Err(failed("shell containment is inconsistent; cavity ownership is ambiguous"));
                    }
                    contained = Some(hit);
                }
                if let Some(slot) = inside.get_mut(inner).and_then(|row| row.get_mut(outer)) {
                    *slot = contained == Some(true);
                }
            }
        }
    }
    let mut parent = Vec::with_capacity(meshes.len());
    for i in 0..meshes.len() {
        let row = inside.get(i).ok_or_else(|| failed("missing containment"))?;
        let candidates: Vec<_> = row.iter().enumerate().filter_map(|(j, hit)| hit.then_some(j)).collect();
        for (k, a) in candidates.iter().enumerate() {
            for b in candidates.iter().skip(k + 1) {
                if !inside.get(*a).and_then(|r| r.get(*b)).copied().unwrap_or(false)
                    && !inside.get(*b).and_then(|r| r.get(*a)).copied().unwrap_or(false)
                {
                    return Err(failed("multiple overlapping shell owners"));
                }
            }
        }
        let owner = candidates.into_iter().min_by(|a, b| {
            volumes.get(*a).copied().unwrap_or(f64::INFINITY).abs().total_cmp(&volumes.get(*b).copied().unwrap_or(f64::INFINITY).abs())
        });
        let outward = volumes.get(i).is_some_and(|v| *v > 0.0);
        match owner {
            None if !outward => return Err(failed("inward shell has no enclosing material shell")),
            Some(p) if volumes.get(p).is_some_and(|v| (*v > 0.0) == outward) => {
                return Err(failed("nested shells do not alternate material and void"));
            }
            _ => {}
        }
        parent.push(owner);
    }
    Ok(parent)
}
