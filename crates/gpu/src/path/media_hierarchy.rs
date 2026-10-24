//! Conservative, bounded convex-parent nesting certificate. AABB containment is
//! only a candidate; parent convexity and child vertices are checked against
//! every outward parent half-space. Intersections and touching boundaries refuse.
use super::scene::{Triangle, reserved};
use solidcraft_geom::Vec3;
use solidcraft_render::job::{CancelToken, ErrorCode, JobError, Result};
use std::ops::Range;

pub(super) struct Region {
    pub lo: Vec3,
    pub hi: Vec3,
    pub closed: bool,
    pub triangles: Range<usize>,
    pub medium: u32,
}
fn invalid(message: &str) -> JobError {
    JobError::new(ErrorCode::InvalidScene, message)
}
fn disjoint(a: &Region, b: &Region) -> bool {
    a.hi.x < b.lo.x || b.hi.x < a.lo.x || a.hi.y < b.lo.y || b.hi.y < a.lo.y || a.hi.z < b.lo.z || b.hi.z < a.lo.z
}
fn box_contains(a: &Region, b: &Region) -> bool {
    a.lo.x <= b.lo.x && a.lo.y <= b.lo.y && a.lo.z <= b.lo.z && a.hi.x >= b.hi.x && a.hi.y >= b.hi.y && a.hi.z >= b.hi.z
}
fn check_halfspaces(parent: &[Triangle], child: &[Triangle], strict: bool, work: &mut u64, cancel: &CancelToken) -> Result<()> {
    for face in parent {
        let normal = (face.p[1] - face.p[0]).cross(face.p[2] - face.p[0]);
        let length = normal.len();
        if !length.is_finite() || length <= 1e-12 {
            return Err(invalid("invalid parent medium plane"));
        }
        let n = normal / length;
        for triangle in child {
            for point in triangle.p {
                *work += 1;
                if (*work).is_multiple_of(2048) {
                    cancel.check()?;
                }
                if *work > 50_000_000 {
                    return Err(JobError::new(
                        ErrorCode::LimitExceeded,
                        "nested-medium half-space validation exceeds 50 million bounded checks; simplify only the selected optical boundary explicitly",
                    ));
                }
                let scale = point.x.abs().max(point.y.abs()).max(point.z.abs()).max(1.0);
                let tolerance = (scale * f64::from(f32::EPSILON) * 4.0).max(2e-6);
                let distance = (point - face.p[0]).dot(n);
                if !distance.is_finite() || (strict && distance >= -tolerance) || (!strict && distance > 2e-6) {
                    return Err(invalid(if strict {
                        "nested dielectric is not strictly inside the certified convex parent; touching or intersecting surfaces are unsupported"
                    } else {
                        "nested-medium parent must be geometrically convex; bounding-box containment is not a certificate"
                    }));
                }
            }
        }
    }
    cancel.check()
}

/// Return immediate parent medium identities, with zero for air, independent of
/// body order. A maximum of 16 containing media matches the device stack exactly.
pub(super) fn parents(regions: &[Region], triangles: &[Triangle], allow_nested: bool, cancel: &CancelToken) -> Result<Vec<(u32, u32)>> {
    if regions.len() > 32 {
        return Err(invalid("at most 32 dielectric bodies are supported"));
    }
    let mut contains = [[false; 32]; 32];
    let mut convex = [false; 32];
    let mut work = 0;
    for (i, a) in regions.iter().enumerate() {
        for (j, b) in regions.iter().enumerate().skip(i + 1) {
            cancel.check()?;
            if disjoint(a, b) || (!a.closed && !b.closed) {
                continue;
            }
            if !allow_nested {
                return Err(invalid("nested or overlapping dielectric bounds require explicit nested_media and a containment certificate"));
            }
            if !a.closed || !b.closed {
                return Err(invalid("a thin-sheet boundary may not overlap a closed nested medium"));
            }
            let (parent, child, pi, ci) = if box_contains(a, b) {
                (a, b, i, j)
            } else if box_contains(b, a) {
                (b, a, j, i)
            } else {
                return Err(invalid("dielectric overlap is not a strict containment hierarchy"));
            };
            let parent_triangles = triangles.get(parent.triangles.clone()).ok_or_else(|| invalid("missing parent triangles"))?;
            let child_triangles = triangles.get(child.triangles.clone()).ok_or_else(|| invalid("missing child triangles"))?;
            if !convex[pi] {
                check_halfspaces(parent_triangles, parent_triangles, false, &mut work, cancel)?;
                convex[pi] = true;
            }
            check_halfspaces(parent_triangles, child_triangles, true, &mut work, cancel)?;
            if parent_triangles.iter().chain(child_triangles).any(|t| t.transmission.is_some_and(|m| m.shadow_transmission())) {
                return Err(invalid("nested media require disabled straight shadow transmission; true refracted paths remain active"));
            }
            contains[pi][ci] = true;
        }
    }
    let mut result = reserved(regions.len())?;
    for (child, region) in regions.iter().enumerate() {
        let depth = contains.iter().filter(|parents| parents[child]).count();
        if depth >= 16 {
            return Err(JobError::new(ErrorCode::LimitExceeded, "at most 16 nested dielectric layers are supported"));
        }
        let mut parent = 0;
        for (candidate, record) in regions.iter().enumerate() {
            if contains[candidate][child] && !(0..regions.len()).any(|between| contains[candidate][between] && contains[between][child]) {
                if parent != 0 {
                    return Err(invalid("ambiguous nested medium parent"));
                }
                parent = record.medium;
            }
        }
        result.push((region.medium, parent));
    }
    Ok(result)
}
