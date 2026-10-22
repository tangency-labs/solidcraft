//! Group disconnected material regions without moving a cavity to another enclosing shell.
//! A valid shell hierarchy alternates material and void at every nesting level.

use solidcraft_geom::{Aabb3, BoxedTriangle, Mesh, Vec3, find_close_pair};

use crate::{KernelError, Result};

const MAX_PAIR_WORK: usize = 40_000_000;

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

