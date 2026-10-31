//! Conservative reconstruction of mechanical meshes from certified straight-extrusion parts.
//! Geometry is inferred from closed, manifold input, never from a display mesh.

use std::collections::BTreeMap;

use solidcraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};

use crate::{Body, KernelError, Result};

#[path = "mesh_convert_curves.rs"]
mod curves;
#[path = "mesh_convert_parts.rs"]
mod parts;

const MAX_TRIANGLES: usize = 50_000;
const MAX_BOUNDARY: usize = 8192;
/// Nesting certification runs two planar booleans per pair of boundary loops,
/// so the loop count (not the segment count) bounds its cost: 50 loops (an
/// outline with 49 holes) is at most 1225 pairs and 2450 booleans.
const MAX_LOOPS: usize = 50;

fn invalid(message: &str) -> KernelError {
    KernelError::Invalid(format!("mesh conversion: {message}"))
}

/// A mesh over the triangle budget: its size, the limit and the supported shape family.
fn over_budget(triangles: usize) -> KernelError {
    invalid(&format!(
        "the mesh has {triangles} triangles, more than the {MAX_TRIANGLES} conversion can take; only closed constant-section parts and certified combinations joined at planar shoulders can be converted, so a freeform mesh is refused at any size"
    ))
}

/// Reconstruct straight-extrusion parts and their planar shoulders as analytic planes and, where certified
/// within `tolerance` mm, circular cylinders. The source mesh is not modified.
/// Unsupported shapes are refused instead of silently returning a faceted solid.
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
        for &(axis, _) in axes.iter().take(16) {
            if let Some(result) = extrusion(mesh, axis, tolerance)? {
                return Ok(result);
            }
        }
        if let Some(result) = parts::reconstruct(mesh, tolerance)? {
            return Ok(result);
        }
        Err(invalid("this mesh cannot be certified as straight-extrusion parts joined at planar shoulders; no geometry was converted"))
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
    let triangles = mesh
        .triangles
        .iter()
        .map(|t| {
            let (Some(&a), Some(&b), Some(&c)) = (remap.get(t[0] as usize), remap.get(t[1] as usize), remap.get(t[2] as usize)) else {
                return Err(invalid("missing triangle vertex"));
            };
            Ok([a, b, c])
        })
        .collect::<Result<Vec<_>>>()?;
    crate::mesh_body(&positions, &triangles)
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
            // STL stores coordinates as f32. Test the projected triangle's
            // distance from a line rather than amplifying that rounding into
            // an angular error on narrow side triangles.
            let [a, b, c] = [plane.to_local(*pa), plane.to_local(*pb), plane.to_local(*pc)];
            let length = a.dist(b).max(b.dist(c)).max(c.dist(a));
            if (b - a).cross(c - a).abs() > epsilon * 3. * length {
                return Ok(None);
            }
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
    let (Some(mut adjacency), Some(mut other)) = (boundary(0), boundary(1)) else { return Ok(None) };
    // Clipping a triangle at a shoulder can introduce different collinear nodes
    // on the two caps. Compare their actual polygon edges, not tessellation counts.
    let simplify = |graph: &mut BTreeMap<(i64, i64), Vec<(i64, i64)>>| -> Option<()> {
        if graph.len() > MAX_BOUNDARY || graph.values().any(|v| v.len() != 2) {
            return None;
        }
        let mut pending: std::collections::VecDeque<_> = graph.keys().copied().collect();
        while let Some(key) = pending.pop_front() {
            let Some(neighbours) = graph.get(&key) else { continue };
            let (Some(&a), Some(&b)) = (neighbours.first(), neighbours.get(1)) else { return None };
            if a == b {
                return None;
            }
            let (pa, pb, p) = (*positions.get(&a)?, *positions.get(&b)?, *positions.get(&key)?);
            let line = solidcraft_geom::Seg2::Line { a: pa, b: pb };
            if line.point_at(line.closest_param(p)).dist(p) > epsilon * 3. {
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
                adjacent.sort();
                pending.push_back(from);
            }
        }
        Some(())
    };
    if simplify(&mut adjacency).is_none() || simplify(&mut other).is_none() {
        return Ok(None);
    }
    if adjacency != other || adjacency.is_empty() || adjacency.len() > MAX_BOUNDARY || adjacency.values().any(|n| n.len() != 2) {
        return Ok(None);
    }
    // Every side vertex must lie on an unchanged straight extrusion of the cap.
    // This excludes twisted, tapered and stepped meshes before fitting any circles.
    let point = |k: (i64, i64)| Vec2::new(k.0 as f64 * epsilon, k.1 as f64 * epsilon);
    let boundary_segments: Vec<_> =
        adjacency.iter().flat_map(|(&a, bs)| bs.iter().filter(move |&&b| a < b).map(move |&b| (point(a), point(b)))).collect();
    if mesh.positions.len().saturating_mul(boundary_segments.len()) > 32_000_000 {
        return Err(invalid("boundary certification exceeds its work budget"));
    }
    for p in &mesh.positions {
        let h = p.dot(axis);
        if (h - lo).abs() <= epsilon || (h - hi).abs() <= epsilon {
            continue;
        }
        let p = plane.to_local(*p);
        if !boundary_segments.iter().any(|&(a, b)| {
            solidcraft_geom::Seg2::Line { a, b }.point_at(solidcraft_geom::Seg2::Line { a, b }.closest_param(p)).dist(p) <= epsilon * 3.
        }) {
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
            let Some(neighbours) = adjacency.remove(&current) else { return Ok(None) };
            chain.push(*positions.get(&current).unwrap_or(&point(current)));
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
        raw_loops.push(Loop2::polygon(&chain));
        loops.push(fit_loop(&chain, tolerance, epsilon));
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
    let mut points = points.to_vec();
    let linear_tolerance = epsilon / points.len().max(1) as f64;
    loop {
        let n = points.len();
        if n <= 3 {
            break;
        }
        let remove = (0..n).find(|&i| {
            let (Some(a), Some(b), Some(c)) = (points.get((i + n - 1) % n), points.get(i), points.get((i + 1) % n)) else { return false };
            let edge = solidcraft_geom::Seg2::Line { a: *a, b: *c };
            edge.point_at(edge.closest_param(*b)).dist(*b) <= linear_tolerance
        });
        let Some(index) = remove else { break };
        points.remove(index);
    }
    fit_partial_arcs(&points, tolerance, epsilon)
}

/// A circular arc through both actual endpoints, independently certified against every
/// source vertex and chord. Short polygon corners and coarse facets are not rounded.
fn partial_arc(points: &[Vec2], tolerance: f64, epsilon: f64) -> Option<solidcraft_geom::Seg2> {
    use solidcraft_geom::Seg2;
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
    let mean = points.iter().copied().fold(Vec2::ZERO, |a, b| a + b) / points.len() as f64;
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
    if determinant.abs() <= (xx + yy).powi(2) * 1e-10 {
        return None;
    }
    let seed = mean + Vec2::new(yy * xq - xy * yq, xx * yq - xy * xq) / (2. * determinant);
    let normal = Vec2::new(-chord.y, chord.x) / length;
    let middle = (first + last) * 0.5;
    let center = middle + normal * (seed - middle).dot(normal);
    let radius = center.dist(first);
    if !radius.is_finite() || radius <= epsilon {
        return None;
    }
    let residual = points.iter().map(|p| (p.dist(center) - radius).abs()).fold(0., f64::max);
    if residual > epsilon * 5. {
        return None;
    }
    let mut sweep = 0.;
    let mut direction = 0.;
    for pair in points.windows(2) {
        let (a, b) = (pair[0] - center, pair[1] - center);
        let angle = a.cross(b).atan2(a.dot(b));
        if direction == 0. {
            direction = angle.signum();
        }
        if angle * direction <= 0.
            || angle.abs() > std::f64::consts::PI / 6. + 1e-8
            || radius * (1. - (angle * 0.5).cos()) + residual + epsilon * 10. > tolerance
        {
            return None;
        }
        sweep += angle;
    }
    if sweep.abs() < 0.04 || sweep.abs() > std::f64::consts::TAU - 0.01 {
        return None;
    }
    let start = (first - center).y.atan2((first - center).x);
    Some(Seg2::Arc { center, radius, start, sweep })
}

fn fit_partial_arcs(points: &[Vec2], tolerance: f64, epsilon: f64) -> Loop2 {
    use solidcraft_geom::Seg2;
    let n = points.len();
    if n < 5 {
        return Loop2::polygon(points);
    }
    // Put the cyclic seam at the sharpest join rather than inside a circular run.
    let turn = |i: usize| {
        let a = points[i] - points[(i + n - 1) % n];
        let b = points[(i + 1) % n] - points[i];
        a.cross(b).atan2(a.dot(b)).abs()
    };
    let offset = (0..n).max_by(|&a, &b| turn(a).total_cmp(&turn(b))).unwrap_or(0);
    let ring: Vec<_> = (0..=n).filter_map(|i| points.get((i + offset) % n).copied()).collect();
    let mut at = 0;
    let mut work = 0usize;
    let mut segs = Vec::new();
    while at < n {
        let mut fitted = None;
        for end in at + 4..=n {
            work = work.saturating_add((end - at + 1) * 2);
            if work > 4_000_000 {
                return Loop2::polygon(points);
            }
            let Some(run) = ring.get(at..=end) else { break };
            let Some(arc) = partial_arc(run, tolerance, epsilon) else { break };
            fitted = Some((end, arc));
        }
        if let Some((end, arc)) = fitted {
            segs.push(arc);
            at = end;
        } else {
            let (Some(&a), Some(&b)) = (ring.get(at), ring.get(at + 1)) else { return Loop2::polygon(points) };
            segs.push(Seg2::Line { a, b });
            at += 1;
        }
    }
    Loop2 { segs }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh_of(body: &Body) -> Body {
        let mesh = body.tessellate(0.005).unwrap();
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
        let fine_volume = |body: &Body| body.tessellate_with(1e-5, true).unwrap().measure().volume;
        assert!((fine_volume(&converted) - expected).abs() < 0.02);
        assert!((crate::measure(&converted).unwrap().volume - crate::measure(&source).unwrap().volume).abs() < 1e-7);
        assert_eq!(crate::measure(&converted).unwrap().merged.face_types.get("cylinder"), Some(&1));
        assert!(converted.faces(0.01).unwrap().len() < 15);
        let step = crate::step_export(&[&converted], "SolidCraft regression").unwrap();
        let reopened = crate::step_import(&step).unwrap();
        assert!((fine_volume(&reopened.bodies[0].body) - expected).abs() < 0.02);
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
        assert_eq!(fit_loop(&points, 0.2, 1e-6).segs.len(), 2);
    }

    #[test]
    fn refuses_open_nonprismatic_and_hostile_inputs() {
        let open = crate::mesh_body(&[Vec3::ZERO, Vec3::X, Vec3::Y], &[[0, 1, 2]]).unwrap();
        assert!(convert_prismatic_mesh(&open, 0.05).is_err());
        let solid = crate::box_solid(Vec3::ZERO, Vec3::new(10., 10., 10.)).unwrap();
        assert!(convert_prismatic_mesh(&solid, 0.05).is_err());
        for tolerance in [f64::NAN, f64::INFINITY, -1., 0., 2.] {
            assert!(convert_prismatic_mesh(&mesh_of(&solid), tolerance).is_err());
        }
        let sphere = crate::sphere(Vec3::ZERO, 10.).unwrap();
        assert!(convert_prismatic_mesh(&mesh_of(&sphere), 0.05).is_err());
        for coordinate in [f64::NAN, f64::INFINITY, f64::MAX, 1e30] {
            let mesh = crate::meshbody::TriMesh { positions: vec![Vec3::new(coordinate, 0., 0.)], triangles: vec![] };
            assert!(weld(&mesh, 0.05).is_err());
        }
    }

    #[test]
    fn a_mesh_over_the_budget_says_how_big_it_is_and_what_converts() {
        // #346: a freeform mesh just over the limit (a strip of 50001 triangles) is refused with
        // its size, the limit, and that only constant-section parts convert at any size.
        let n = MAX_TRIANGLES + 1;
        let positions: Vec<Vec3> = (0..n + 2).map(|k| Vec3::new((k / 2) as f64, (k % 2) as f64, 0.)).collect();
        let triangles: Vec<[u32; 3]> = (0..n as u32).map(|i| [i, i + 1, i + 2]).collect();
        let body = crate::mesh_body(&positions, &triangles).unwrap();
        let err = convert_prismatic_mesh(&body, 0.05).unwrap_err().to_string();
        assert!(err.contains("50001 triangles") && err.contains("50000"), "{err}");
        assert!(err.contains("constant-section") && err.contains("freeform"), "{err}");
        let mesh = crate::meshbody::TriMesh { positions: vec![Vec3::ZERO; MAX_TRIANGLES * 3 + 1], triangles: vec![] };
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
        assert_eq!(fitted.segs.len(), 2);
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

    #[test]
    fn restores_a_partial_circular_notch_without_faceting_its_wall() {
        use solidcraft_geom::Seg2;
        let profile = Loop2 {
            segs: vec![
                Seg2::Line { a: Vec2::new(0., 0.), b: Vec2::new(20., 0.) },
                Seg2::Line { a: Vec2::new(20., 0.), b: Vec2::new(20., 12.) },
                Seg2::Line { a: Vec2::new(20., 12.), b: Vec2::new(14., 12.) },
                Seg2::Arc { center: Vec2::new(10., 12.), radius: 4., start: 0., sweep: -std::f64::consts::PI },
                Seg2::Line { a: Vec2::new(6., 12.), b: Vec2::new(0., 12.) },
                Seg2::Line { a: Vec2::new(0., 12.), b: Vec2::new(0., 0.) },
            ],
        };
        let source = crate::extrude_planar_regions(&Plane::XY, &[Region2 { outer: profile, holes: vec![] }], 0., 5.).unwrap();
        let converted = convert_prismatic_mesh(&mesh_of(&source), 0.05).unwrap();
        assert!(converted.validity().is_empty());
        assert_eq!(crate::measure(&converted).unwrap().merged.face_types.get("cylinder"), Some(&1));
        assert!(converted.face_count() < 12, "{}", converted.face_count());
        assert!((converted.tessellate_with(1e-5, true).unwrap().measure().volume - (240. - 8. * std::f64::consts::PI) * 5.).abs() < 0.01);
    }

    #[test]
    fn reconstructs_a_plate_with_perpendicular_notched_extrusions() {
        use solidcraft_geom::Seg2;
        let profile = || Region2 {
            outer: Loop2 {
                segs: vec![
                    Seg2::Line { a: Vec2::new(0., 0.), b: Vec2::new(20., 0.) },
                    Seg2::Line { a: Vec2::new(20., 0.), b: Vec2::new(20., 12.) },
                    Seg2::Line { a: Vec2::new(20., 12.), b: Vec2::new(14., 12.) },
                    Seg2::Arc { center: Vec2::new(10., 12.), radius: 4., start: 0., sweep: -std::f64::consts::PI },
                    Seg2::Line { a: Vec2::new(6., 12.), b: Vec2::new(0., 12.) },
                    Seg2::Line { a: Vec2::new(0., 12.), b: Vec2::new(0., 0.) },
                ],
            },
            holes: vec![],
        };
        let mut source = crate::box_solid(Vec3::ZERO, Vec3::new(40., 28., 3.)).unwrap();
        for y in [8., 24.] {
            let plane = Plane { origin: Vec3::new(0., y, 3.), x: Vec3::X, y: Vec3::Z };
            let upper = crate::extrude_planar_regions(&plane, &[profile()], 0., 4.).unwrap();
            source = crate::boolean(&source, &upper, crate::BoolOp::Union).unwrap().unwrap();
        }
        for rotated in [false, true] {
            let source = if rotated {
                crate::transform(&source, Vec3::new(11., -7., 5.), Vec3::ZERO, Vec3::new(1., 2., 3.), 0.4).unwrap()
            } else {
                source.clone()
            };
            let raw = source.tessellate(0.005).unwrap();
            let positions: Vec<_> = raw.positions.iter().map(|p| Vec3::new(p.x as f32 as f64, p.y as f32 as f64, p.z as f32 as f64)).collect();
            let mesh = crate::mesh_body(&positions, &raw.triangles).unwrap();
            let converted = convert_prismatic_mesh(&mesh, 0.05).unwrap();
            assert!(!converted.is_mesh());
            assert!(converted.validity().is_empty());
            let m = crate::measure(&converted).unwrap();
            assert!((m.volume - (3360. + 8. * (240. - 8. * std::f64::consts::PI))).abs() < 0.1, "{}", m.volume);
            assert!(m.merged.face_types.get("cylinder").copied().unwrap_or(0) >= 2, "{:?}", m.merged.face_types);
            assert!(converted.face_count() < 50);
            let bytes = crate::step_export(&[&converted], "Public analytic reconstruction test").unwrap();
            let imported = crate::step_import(&bytes).unwrap();
            assert_eq!(imported.bodies.len(), 1);
            assert!(imported.bodies[0].body.validity().is_empty());
        }
    }

    fn phase_shifted_stepped_cylinder() -> Body {
        let n = 96usize;
        let mut positions = Vec::new();
        let mut triangles = Vec::new();
        for (radius, z, phase) in [(12., 0., 0.), (12., 4., 0.3), (8., 4., 0.7), (8., 9., 0.1)] {
            for i in 0..n {
                let angle = (i as f64 + phase) * std::f64::consts::TAU / n as f64;
                positions.push(Vec3::new((radius * angle.cos()) as f32 as f64, (radius * angle.sin()) as f32 as f64, z));
            }
        }
        for layer in 0..3 {
            for i in 0..n {
                let a = (layer * n + i) as u32;
                let b = (layer * n + (i + 1) % n) as u32;
                let c = ((layer + 1) * n + (i + 1) % n) as u32;
                let d = ((layer + 1) * n + i) as u32;
                triangles.extend([[a, b, c], [a, c, d]]);
            }
        }
        positions.extend([Vec3::ZERO, Vec3::new(0., 0., 9.)]);
        for i in 0..n {
            triangles.push([(4 * n) as u32, ((i + 1) % n) as u32, i as u32]);
            triangles.push([(4 * n + 1) as u32, (3 * n + i) as u32, (3 * n + (i + 1) % n) as u32]);
        }
        crate::mesh_body(&positions, &triangles).unwrap()
    }

    #[test]
    fn reconstructs_a_stepped_cylinder_with_different_tessellation_phases() {
        let mesh = phase_shifted_stepped_cylinder();
        let converted = convert_prismatic_mesh(&mesh, 0.05).unwrap();
        assert!(converted.validity().is_empty());
        let volume = converted.tessellate_with(1e-5, true).unwrap().measure().volume;
        assert!((volume - 896. * std::f64::consts::PI).abs() < 0.03, "{volume}");
        assert!(converted.face_count() < 16, "{}", converted.face_count());
        assert_eq!(crate::measure(&converted).unwrap().merged.face_types.get("cylinder"), Some(&2));
    }

    #[test]
    fn rotated_f32_stepped_cylinder_preserves_the_analytic_shape() {
        let source = phase_shifted_stepped_cylinder();
        let mesh = source.mesh.as_ref().unwrap();
        let normal = Vec3::new(1., 2., 3.).normalized().unwrap();
        let angle = 0.43f64;
        let positions: Vec<_> = mesh
            .positions
            .iter()
            .map(|p| {
                let q = *p * angle.cos() + normal.cross(*p) * angle.sin() + normal * (normal.dot(*p) * (1. - angle.cos())) + Vec3::new(13., -9., 7.);
                Vec3::new(q.x as f32 as f64, q.y as f32 as f64, q.z as f32 as f64)
            })
            .collect();
        let source = crate::mesh_body(&positions, &mesh.triangles).unwrap();
        let converted = convert_prismatic_mesh(&source, 0.05).unwrap();
        assert!(converted.validity().is_empty());
        let volume = converted.tessellate_with(1e-5, true).unwrap().measure().volume;
        assert!((volume - 896. * std::f64::consts::PI).abs() < 0.03, "{volume}");
        assert!(converted.face_count() < 16);
    }

    #[test]
    fn radial_damage_outside_tolerance_is_refused() {
        let source = phase_shifted_stepped_cylinder();
        let mut mesh = source.mesh.as_ref().unwrap().as_ref().clone();
        mesh.positions[0].x += 0.4;
        let source = crate::mesh_body(&mesh.positions, &mesh.triangles).unwrap();
        assert!(convert_prismatic_mesh(&source, 0.05).is_err());
    }
}
