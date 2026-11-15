//! Exact retained-f32 surface contact and oriented containment. Bounds accelerate
//! candidate selection; disjoint overlapping bounds never imply nested media.
use super::scene::{Triangle, reserved};
use solidcore_record::Vec3;
use solidcraft_render::{
    job::{CancelToken, ErrorCode, JobError, Result},
    optical_boundary::{ContactIndex, forbidden_contact, origin_crossing},
};
use std::ops::Range;

pub(super) struct Region<'a> {
    pub lo: Vec3,
    pub hi: Vec3,
    pub closed: bool,
    pub triangles: Range<usize>,
    pub medium: u32,
    pub name: &'a str,
}
fn invalid(message: &str) -> JobError {
    JobError::new(ErrorCode::InvalidScene, message)
}
fn disjoint(a: &Region<'_>, b: &Region<'_>) -> bool {
    a.hi.x < b.lo.x || b.hi.x < a.lo.x || a.hi.y < b.lo.y || b.hi.y < a.lo.y || a.hi.z < b.lo.z || b.hi.z < a.lo.z
}
fn tick(work: &mut u64, cancel: &CancelToken) -> Result<()> {
    cancel.check()?;
    *work = work.checked_add(1).ok_or_else(|| invalid("optical certificate work overflow"))?;
    if *work > 1_000_000_000 {
        return Err(JobError::new(ErrorCode::LimitExceeded, "optical surface certificate exceeds one billion bounded checks"));
    }
    Ok(())
}
fn winding(region: &Region<'_>, triangles: &[Triangle], point: Vec3, work: &mut u64, cancel: &CancelToken) -> Result<bool> {
    let mut sums = [0_i32; 3];
    // The topology certificate allows numerical CAD seams, while these tests
    // use the uploaded coordinates. A ray can escape through such a seam.
    // Cyclic permutations retain orientation and check all three axes without
    // moving geometry or perturbing the containment sample.
    let rotate = |p: Vec3, axis| match axis {
        0 => p,
        1 => Vec3::new(p.y, p.z, p.x),
        _ => Vec3::new(p.z, p.x, p.y),
    };
    for triangle in triangles.get(region.triangles.clone()).ok_or_else(|| invalid("missing optical containment boundary"))? {
        tick(work, cancel)?;
        for (axis, sum) in sums.iter_mut().enumerate() {
            *sum += i32::from(origin_crossing(triangle.p.map(|p| rotate(p, axis)), rotate(point, axis))?);
        }
    }
    if sums.iter().any(|sum| *sum != sums[0]) {
        return Err(invalid(&format!("inconsistent optical containment across uploaded seams for {}", region.name)));
    }
    match sums[0] {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(invalid(&format!("ambiguous optical containment winding for {}", region.name))),
    }
}
/// Certify all optical self/cross contacts with the actual uploaded coordinates.
/// This covers nonconvex surfaces without a triangle-pair scan.
fn contacts(regions: &[Region<'_>], triangles: &[Triangle], work: &mut u64, cancel: &CancelToken) -> Result<()> {
    let count = regions.iter().try_fold(0_usize, |n, r| n.checked_add(r.triangles.len()).ok_or_else(|| invalid("optical contact count overflow")))?;
    let mut ordinals = reserved(count)?;
    for region in regions {
        for i in region.triangles.clone() {
            if ordinals.len().is_multiple_of(1024) {
                cancel.check()?;
            }
            ordinals.push(i);
        }
    }
    let mut index = ContactIndex::new(count)?;
    while !index.step(|i| ordinals.get(i).and_then(|at| triangles.get(*at)).map(|t| t.p), cancel)? {
        tick(work, cancel)?;
    }
    for (i, at) in ordinals.iter().enumerate() {
        let a = triangles.get(*at).ok_or_else(|| invalid("missing optical contact face"))?;
        let mut query = index.query(a.p)?;
        loop {
            tick(work, cancel)?;
            let Some(candidate) = index.query_step(&mut query, cancel)? else { break };
            let Some(j) = candidate.filter(|j| *j > i) else { continue };
            let other = *ordinals.get(j).ok_or_else(|| invalid("missing optical contact ordinal"))?;
            let b = triangles.get(other).ok_or_else(|| invalid("missing optical contact candidate"))?;
            if forbidden_contact(a.p, b.p, a.medium == b.medium) {
                let identify = |id, triangle| {
                    regions.iter().find(|r| r.medium == id).map_or_else(
                        || format!("unknown triangle {triangle}"),
                        |region| format!("{} triangle {}", region.name, triangle - region.triangles.start),
                    )
                };
                return Err(invalid(&format!(
                    "glass boundaries intersect, touch, overlap or self-intersect: {} / {}",
                    identify(a.medium, *at),
                    identify(b.medium, other)
                )));
            }
        }
    }
    Ok(())
}
/// Immediate parent identities are independent of body order. Externally
/// disjoint interlocking boundaries remain children of air. Actual containment
/// retains the explicit nesting and disabled-shadow-shortcut requirements.
pub(super) fn parents(regions: &[Region<'_>], triangles: &[Triangle], allow_nested: bool, cancel: &CancelToken) -> Result<Vec<(u32, u32)>> {
    if regions.is_empty() {
        return Ok(Vec::new());
    }
    if regions.len() > solidcraft_render::job::MAX_GPU_DIELECTRICS {
        return Err(invalid("at most 128 dielectric bodies are supported"));
    }
    let mut contains = [[false; 128]; 128];
    let mut work = 0;
    contacts(regions, triangles, &mut work, cancel)?;
    for (i, a) in regions.iter().enumerate() {
        for (j, b) in regions.iter().enumerate().skip(i + 1) {
            tick(&mut work, cancel)?;
            if disjoint(a, b) || (!a.closed && !b.closed) {
                continue;
            }
            if !a.closed || !b.closed {
                return Err(invalid("a thin-sheet boundary may not overlap a closed nested medium"));
            }
            let ap = triangles.get(a.triangles.start).ok_or_else(|| invalid("missing optical containment sample"))?.p[0];
            let bp = triangles.get(b.triangles.start).ok_or_else(|| invalid("missing optical containment sample"))?.p[0];
            let ab = winding(a, triangles, bp, &mut work, cancel)?;
            let ba = winding(b, triangles, ap, &mut work, cancel)?;
            if ab && ba {
                return Err(invalid("cyclic optical containment"));
            }
            if !ab && !ba {
                continue;
            }
            if !allow_nested {
                return Err(invalid("nested dielectric bounds require explicit nested_media and a containment certificate"));
            }
            if triangles[a.triangles.clone()]
                .iter()
                .chain(triangles[b.triangles.clone()].iter())
                .any(|t| t.transmission.is_some_and(|m| m.shadow_transmission()))
            {
                return Err(invalid("nested media require disabled straight shadow transmission; true refracted paths remain active"));
            }
            if ab {
                contains[i][j] = true;
            } else {
                contains[j][i] = true;
            }
        }
    }
    let mut result = reserved(regions.len())?;
    for (child, region) in regions.iter().enumerate() {
        let depth = contains.iter().filter(|parents| parents[child]).count();
        if depth >= solidcraft_render::job::MAX_NESTED_MEDIA {
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
