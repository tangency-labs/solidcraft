//! Analytic section certificates independent of a cylinder's tessellation phase.
use super::{MAX_BOUNDARY, MAX_LOOPS, certify_nesting, fit_loop, invalid};
use crate::{Body, Result, meshbody::TriMesh};
use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
pub(super) struct Circle {
    center: Vec2,
    radius: f64,
}

fn section(mesh: &TriMesh, plane: &Plane, height: f64, epsilon: f64) -> Option<Vec<Vec<Vec2>>> {
    let axis = plane.normal();
    let mut edges = BTreeMap::<(u32, u32), usize>::new();
    for t in &mesh.triangles {
        if !t.iter().all(|&i| mesh.positions.get(i as usize).is_some_and(|p| (p.dot(axis) - height).abs() <= epsilon * 3.)) {
            continue;
        }
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            *edges.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    let mut adjacency = BTreeMap::<u32, Vec<u32>>::new();
    for ((a, b), count) in edges {
        if count == 2 {
            continue;
        }
        if count != 1 {
            return None;
        }
        adjacency.entry(a).or_default().push(b);
        adjacency.entry(b).or_default().push(a);
    }
    if adjacency.is_empty() || adjacency.len() > MAX_BOUNDARY || adjacency.values().any(|v| v.len() != 2) {
        return None;
    }
    let mut out = Vec::new();
    while let Some((&start, _)) = adjacency.first_key_value() {
        if out.len() >= MAX_LOOPS {
            return None;
        }
        let mut at = start;
        let mut previous = None;
        let mut points = Vec::new();
        loop {
            let next = adjacency.remove(&at)?.into_iter().find(|v| Some(*v) != previous)?;
            points.push(plane.to_local(*mesh.positions.get(at as usize)?));
            if next == start {
                break;
            }
            previous = Some(at);
            at = next;
        }
        if points.len() < 3 {
            return None;
        }
        out.push(points);
    }
    Some(out)
}

/// Only circles already certified on an original planar face become reconstruction hints.
pub(super) fn hints(mesh: &TriMesh, axis: Vec3, tolerance: f64, epsilon: f64) -> Vec<Circle> {
    let Some(plane) = Plane::from_normal(Vec3::ZERO, axis) else { return Vec::new() };
    let mut levels = Vec::new();
    for t in &mesh.triangles {
        let Some(h) = t.iter().map(|&i| mesh.positions.get(i as usize).map(|p| p.dot(axis))).collect::<Option<Vec<_>>>() else { return Vec::new() };
        if h.iter().copied().fold(f64::NEG_INFINITY, f64::max) - h.iter().copied().fold(f64::INFINITY, f64::min) <= epsilon * 3. {
            levels.push(h.iter().sum::<f64>() / 3.);
        }
    }
    levels.sort_by(f64::total_cmp);
    levels.dedup_by(|a, b| (*a - *b).abs() <= epsilon * 3.);
    if levels.len() > 64 {
        return Vec::new();
    }
    let mut out: Vec<Circle> = Vec::new();
    for height in levels {
        let Some(loops) = section(mesh, &plane, height, epsilon) else { continue };
        for points in loops {
            for seg in fit_loop(&points, tolerance, epsilon).segs {
                if let Seg2::Arc { center, radius, .. } = seg {
                    if !out.iter().any(|c| c.center.dist(center) <= epsilon * 10. && (c.radius - radius).abs() <= epsilon * 10.) {
                        out.push(Circle { center, radius });
                    }
                    if out.len() > 128 {
                        return Vec::new();
                    }
                }
            }
        }
    }
    out
}

fn arc(points: &[Vec2], circle: Circle, tolerance: f64, epsilon: f64) -> Option<Seg2> {
    if points.len() < 5 {
        return None;
    }
    let Circle { center, radius } = circle;
    let residual = points.iter().map(|p| (p.dist(center) - radius).abs()).fold(0., f64::max);
    if residual + epsilon * 10. > tolerance {
        return None;
    }
    let mut sweep = 0.;
    let mut sign = 0.;
    for pair in points.windows(2) {
        let (a, b) = (pair[0] - center, pair[1] - center);
        let angle = a.cross(b).atan2(a.dot(b));
        if sign == 0. {
            sign = angle.signum();
        }
        if angle * sign <= 0.
            || angle.abs() > std::f64::consts::PI / 6. + 1e-8
            || radius * (1. - (angle / 2.).cos()) + residual + epsilon * 10. > tolerance
        {
            return None;
        }
        sweep += angle;
    }
    let p = *points.first()? - center;
    Some(Seg2::Arc { center, radius, start: p.y.atan2(p.x), sweep })
}

fn circle_of(s: &Seg2) -> Option<Circle> {
    if let Seg2::Arc { center, radius, .. } = s { Some(Circle { center: *center, radius: *radius }) } else { None }
}
fn join(a: &Seg2, b: &Seg2, near: Vec2, tolerance: f64) -> Option<Vec2> {
    let mut points = Vec::new();
    match (circle_of(a), circle_of(b)) {
        (Some(a), Some(b)) => {
            let d = b.center - a.center;
            let length = d.len();
            if length < 1e-7 && (a.radius - b.radius).abs() < 1e-7 {
                let v = (near - a.center).normalized()?;
                return Some(a.center + v * a.radius);
            }
            if length <= 1e-12 {
                return None;
            }
            let x = (a.radius * a.radius - b.radius * b.radius + length * length) / (2. * length);
            let h2 = a.radius * a.radius - x * x;
            if h2 < -1e-9 {
                return None;
            }
            let n = d / length;
            let middle = a.center + n * x;
            let perpendicular = Vec2::new(-n.y, n.x) * h2.max(0.).sqrt();
            points.extend([middle + perpendicular, middle - perpendicular]);
        }
        (Some(c), None) | (None, Some(c)) => {
            let line = if circle_of(a).is_none() { a } else { b };
            let direction = (line.end() - line.start()).normalized()?;
            let offset = line.start() - c.center;
            let along = -offset.dot(direction);
            let h2 = c.radius * c.radius - offset.len2() + along * along;
            if h2 < -1e-9 {
                return None;
            }
            for t in [along + h2.max(0.).sqrt(), along - h2.max(0.).sqrt()] {
                points.push(line.start() + direction * t);
            }
        }
        (None, None) => return Some(near),
    }
    points.into_iter().filter(|p| p.dist(near) <= tolerance).min_by(|a, b| a.dist(near).total_cmp(&b.dist(near)))
}

fn fitted(points: &[Vec2], models: &[Circle], tolerance: f64, epsilon: f64) -> Loop2 {
    let original = fit_loop(points, tolerance, epsilon);
    if points.len() < 5 || models.is_empty() {
        return original;
    }
    let mut ring = points.to_vec();
    let n = ring.len();
    let offset = (0..n)
        .max_by(|&a, &b| {
            let turn = |i: usize| {
                let p = ring[i] - ring[(i + n - 1) % n];
                let q = ring[(i + 1) % n] - ring[i];
                p.cross(q).atan2(p.dot(q)).abs()
            };
            turn(a).total_cmp(&turn(b))
        })
        .unwrap_or(0);
    ring.rotate_left(offset);
    let Some(&first) = ring.first() else { return original };
    ring.push(first);
    // A whole circle has no preferred seam and is represented by two analytic half-arcs.
    for &model in models {
        if let Some(Seg2::Arc { center, radius, sweep, .. }) = arc(&ring, model, tolerance, epsilon)
            && (sweep.abs() - std::f64::consts::TAU).abs() < 1e-5
        {
            return Loop2::circle(center, radius);
        }
    }
    let mut at = 0;
    let mut segs = Vec::new();
    let mut vertices = Vec::new();
    let mut work = 0usize;
    while at < n {
        let mut best = None;
        for &model in models {
            for end in at + 4..=n {
                work = work.saturating_add((end - at + 1) * 2);
                if work > 4_000_000 {
                    return original;
                }
                let Some(run) = ring.get(at..=end) else { break };
                let Some(segment) = arc(run, model, tolerance, epsilon) else { break };
                if best.as_ref().is_none_or(|(old, _)| end > *old) {
                    best = Some((end, segment));
                }
            }
        }
        let Some(&a) = ring.get(at) else { return original };
        vertices.push(a);
        if let Some((end, segment)) = best {
            segs.push(segment);
            at = end;
        } else {
            let Some(&b) = ring.get(at + 1) else { return original };
            segs.push(Seg2::Line { a, b });
            at += 1;
        }
    }
    let mut joints = Vec::new();
    for (i, &near) in vertices.iter().enumerate() {
        let (Some(a), Some(b)) = (segs.get((i + segs.len() - 1) % segs.len()), segs.get(i)) else { return original };
        let Some(p) = join(a, b, near, tolerance) else { return original };
        joints.push(p);
    }
    let mut out = Vec::new();
    for (i, segment) in segs.iter().enumerate() {
        let (Some(&a), Some(&b)) = (joints.get(i), joints.get((i + 1) % joints.len())) else { return original };
        match segment {
            Seg2::Line { .. } => out.push(Seg2::Line { a, b }),
            Seg2::Arc { center, radius, sweep, .. } => {
                let start = (a - *center).y.atan2((a - *center).x);
                let end = (b - *center).y.atan2((b - *center).x);
                let angle =
                    if *sweep > 0. { (end - start).rem_euclid(std::f64::consts::TAU) } else { -(start - end).rem_euclid(std::f64::consts::TAU) };
                out.push(Seg2::Arc { center: *center, radius: *radius, start, sweep: angle });
            }
            _ => return original,
        }
    }
    Loop2 { segs: out }
}

fn line_distance(a: Vec2, b: Vec2, p: Vec2) -> f64 {
    let line = Seg2::Line { a, b };
    line.point_at(line.closest_param(p)).dist(p)
}
fn certified(points: &[Vec2], curves: &[Seg2], tolerance: f64) -> bool {
    curves.iter().any(|curve| match curve {
        Seg2::Line { a, b } => points.iter().all(|p| line_distance(*a, *b, *p) <= tolerance),
        Seg2::Arc { center, radius, start, sweep } => {
            if points.iter().any(|p| p.dist(*center) > radius + tolerance) {
                return false;
            }
            let mut minimum = f64::INFINITY;
            for (&a, &b) in points.iter().zip(points.iter().cycle().skip(1)).take(points.len()) {
                minimum = minimum.min(line_distance(a, b, *center));
            }
            if points.len() == 3 {
                let mut signs = points.iter().zip(points.iter().cycle().skip(1)).take(3).map(|(a, b)| (*b - *a).cross(*center - *a));
                let first = signs.next().unwrap_or(0.);
                if first.abs() > 1e-14 && signs.all(|s| s * first > 0.) {
                    minimum = 0.;
                }
            }
            if minimum < radius - tolerance {
                return false;
            }
            let full = curves
                .iter()
                .filter_map(|s| {
                    if let Seg2::Arc { center: c, radius: r, sweep: w, .. } = s {
                        if c.dist(*center) < 1e-8 && (*r - radius).abs() < 1e-8 { Some(w.abs()) } else { None }
                    } else {
                        None
                    }
                })
                .sum::<f64>()
                >= std::f64::consts::TAU - 1e-8;
            if full {
                return true;
            }
            if points.iter().any(|p| curve.point_at(curve.closest_param(*p)).dist(*p) > tolerance) {
                return false;
            }
            let angular = tolerance / radius.max(tolerance);
            let mut angles = Vec::new();
            for p in points {
                let d = *p - *center;
                let mut angle = ((d.y.atan2(d.x) - start) * sweep.signum()).rem_euclid(std::f64::consts::TAU);
                if angle > std::f64::consts::TAU - angular {
                    angle -= std::f64::consts::TAU;
                }
                if angle < -angular || angle > sweep.abs() + angular {
                    return false;
                }
                angles.push(angle);
            }
            angles.iter().copied().fold(f64::NEG_INFINITY, f64::max) - angles.iter().copied().fold(f64::INFINITY, f64::min)
                <= std::f64::consts::PI / 6. + angular
        }
        _ => false,
    })
}

pub(super) fn extrusion(mesh: &TriMesh, axis: Vec3, tolerance: f64, models: &[Circle]) -> Result<Option<Body>> {
    if models.is_empty() {
        return Ok(None);
    }
    let (lo, hi) = mesh.positions.iter().map(|p| p.dot(axis)).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), h| (a.min(h), b.max(h)));
    let bounds = mesh.positions.iter().fold(solidcraft_geom::Aabb3::EMPTY, |mut b, p| {
        b.add(*p);
        b
    });
    let size = bounds.size().len();
    let epsilon = (size.max(1.) * 1e-7).min(tolerance * 0.01).max(1e-9);
    if hi - lo <= epsilon {
        return Ok(None);
    }
    let Some(plane) = Plane::from_normal(axis * lo, axis) else { return Ok(None) };
    let (Some(lower), Some(upper)) = (section(mesh, &plane, lo, epsilon), section(mesh, &plane, hi, epsilon)) else { return Ok(None) };
    if lower.len() != upper.len() {
        return Ok(None);
    }
    let a: Vec<_> = lower.iter().map(|p| fitted(p, models, tolerance, epsilon)).collect();
    let b: Vec<_> = upper.iter().map(|p| fitted(p, models, tolerance, epsilon)).collect();
    let count = |p: &[Loop2]| p.iter().map(|p| p.segs.len()).sum::<usize>();
    let loops = if count(&a) <= count(&b) { a } else { b };
    let curves: Vec<_> = loops.iter().flat_map(|p| p.segs.iter().cloned()).collect();
    if curves.len() > 128 || mesh.triangles.len().saturating_mul(curves.len()) > 4_000_000 {
        return Ok(None);
    }
    // Every cap loop is paired bijectively and every entire source chord is bounded.
    let mut lower_matched = Vec::new();
    for (cap_index, cap) in [&lower, &upper].into_iter().enumerate() {
        let mut used = std::collections::BTreeSet::new();
        for raw in cap {
            let matches: Vec<_> = loops
                .iter()
                .enumerate()
                .filter(|(_, lp)| raw.iter().zip(raw.iter().cycle().skip(1)).take(raw.len()).all(|(&a, &b)| certified(&[a, b], &lp.segs, tolerance)))
                .map(|(i, _)| i)
                .collect();
            let [index] = matches.as_slice() else { return Ok(None) };
            if !used.insert(*index) {
                return Ok(None);
            }
            if cap_index == 0 {
                let Some(lp) = loops.get(*index) else { return Ok(None) };
                lower_matched.push(lp.clone());
            }
        }
    }
    for t in &mesh.triangles {
        let Some(points) = t.iter().map(|&i| mesh.positions.get(i as usize).copied()).collect::<Option<Vec<_>>>() else {
            return Err(invalid("missing analytic section vertex"));
        };
        if points.iter().all(|p| (p.dot(axis) - lo).abs() <= epsilon * 3.) || points.iter().all(|p| (p.dot(axis) - hi).abs() <= epsilon * 3.) {
            continue;
        }
        let projected: Vec<_> = points.iter().map(|p| plane.to_local(*p)).collect();
        if !certified(&projected, &curves, tolerance) {
            return Ok(None);
        }
    }
    let raw: Vec<_> = lower.iter().map(|p| Loop2::polygon(p)).collect();
    certify_nesting(&raw, &lower_matched, epsilon, size)?;
    let mut regions = Vec::new();
    for lp in &loops {
        let Some(probe) = lp.segs.first().map(Seg2::start) else { return Ok(None) };
        let owners = loops.iter().filter(|p| !std::ptr::eq(*p, lp) && p.contains(probe)).count();
        if owners % 2 == 0 {
            regions.push(Region2 { outer: lp.ccw(), holes: Vec::new() });
        }
    }
    for lp in &loops {
        let Some(probe) = lp.segs.first().map(Seg2::start) else { return Ok(None) };
        let owners = loops.iter().filter(|p| !std::ptr::eq(*p, lp) && p.contains(probe)).count();
        if owners % 2 == 1 {
            let owner = regions
                .iter()
                .enumerate()
                .filter(|(_, r)| r.outer.contains(probe))
                .min_by(|(_, a), (_, b)| a.area().total_cmp(&b.area()))
                .map(|(i, _)| i);
            let Some(region) = owner.and_then(|i| regions.get_mut(i)) else { return Ok(None) };
            region.holes.push(lp.ccw().reversed());
        }
    }
    let body = crate::extrude_planar_regions(&plane, &regions, 0., hi - lo)?;
    let original = mesh.to_mesh().measure().volume;
    let volume = crate::measure(&body)?.volume;
    let perimeter: f64 = curves.iter().map(Seg2::length).sum();
    if !body.validity().is_empty()
        || !volume.is_finite()
        || volume <= 0.
        || (volume - original).abs() > perimeter * (hi - lo) * tolerance + epsilon * size * size
    {
        return Err(invalid("analytic section failed solid or volume validation"));
    }
    Ok(Some(body))
}
