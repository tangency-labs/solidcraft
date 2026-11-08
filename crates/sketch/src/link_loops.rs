//! Conservative analytic boundary roles for associative face identity.
//! Endpoint walks exclude support centres. Crossings/touches refuse before nesting.
use super::{LCurve, curve_points};
use crate::{Shape, intersections};
use solidcore_record::Vec2;
use solidcraft_geom::Aabb2;
use solidcraft_geom::prelude::*;
use std::f64::consts::TAU;

struct Boundary {
    curves: Vec<usize>,
    shapes: Vec<Shape>,
    bounds: Aabb2,
    depth: usize,
    signature: [usize; 3],
}
pub(super) struct Roles {
    pub old: Vec<Vec<usize>>,
    pub fresh: Vec<Vec<usize>>,
    pub old_normal: Vec<Vec2>,
    pub fresh_normal: Vec<Vec2>,
    pub old_lines: Vec<Vec<Vec2>>,
    pub fresh_lines: Vec<Vec<Vec2>>,
    /// The directions the lines at each point leave it in (as `old_lines`, but with their sense):
    /// the two end corners of a thin arm have the same lines, leaving them opposite ways.
    pub old_leaving: Vec<Vec<Vec2>>,
    pub fresh_leaving: Vec<Vec<Vec2>>,
}

pub(super) fn same_lines(a: &[Vec2], b: &[Vec2]) -> bool {
    a.len() == b.len() && a.iter().all(|x| b.iter().any(|y| x.dot(*y).abs() > 0.98))
}

/// Whether the lines leave two points the same ways ([`Roles::old_leaving`]): each direction
/// of one point matches a different direction of the other. At a sharp corner two nearly
/// parallel directions must not both match one direction while the other line turned round.
/// A point with more than 8 lines is never matched: the search stays small.
pub(super) fn same_leaving(a: &[Vec2], b: &[Vec2]) -> bool {
    fn assign(a: &[Vec2], b: &[Vec2], used: &mut [bool]) -> bool {
        let Some((x, rest)) = a.split_first() else { return true };
        for (i, y) in b.iter().enumerate() {
            if used.get(i).copied().unwrap_or(true) || x.dot(*y) <= 0.98 {
                continue;
            }
            if let Some(slot) = used.get_mut(i) {
                *slot = true;
            }
            if assign(rest, b, used) {
                return true;
            }
            if let Some(slot) = used.get_mut(i) {
                *slot = false;
            }
        }
        false
    }
    a.len() == b.len() && a.len() <= 8 && assign(a, b, &mut vec![false; b.len()])
}
fn on(s: &Shape, p: Vec2, tol: f64) -> bool {
    match *s {
        Shape::Line { a, b } => {
            let d = b - a;
            let t = (p - a).dot(d) / d.len2().max(1e-30);
            t >= -tol / d.len().max(tol) && t <= 1.0 + tol / d.len().max(tol) && (a + d * t).dist(p) <= tol
        }
        Shape::Round { c, r, start, sweep } => {
            (p.dist(c) - r).abs() <= tol && (sweep >= TAU - 1e-12 || ((p - c).angle() - start).rem_euclid(TAU) <= sweep + tol / r)
        }
    }
}
fn endpoints(s: &Shape) -> [Vec2; 2] {
    let (_, end) = s.range();
    [s.point(0.0), s.point(end)]
}
fn stable_intersections(a: &Shape, b: &Shape, tol: f64) -> Vec<Vec2> {
    match (*a, *b) {
        (Shape::Line { a, b }, Shape::Round { c, r, .. }) | (Shape::Round { c, r, .. }, Shape::Line { a, b }) => {
            let Some(direction) = (b - a).normalized() else { return Vec::new() };
            // Work in the circle's local orthonormal frame. The world-coordinate
            // quadratic discriminant can lose every significant bit on long lines.
            let foot = a + direction * (c - a).dot(direction);
            let distance = foot.dist(c);
            if distance > r + tol {
                return Vec::new();
            }
            let half = ((r - distance).max(0.0) * (r + distance)).sqrt();
            vec![foot - direction * half, foot + direction * half]
        }
        (Shape::Round { c: ac, r: ar, .. }, Shape::Round { c: bc, r: br, .. }) => {
            let ((small, r), (large, radius)) = if ar <= br { ((ac, ar), (bc, br)) } else { ((bc, br), (ac, ar)) };
            let delta = large - small;
            let distance = delta.len();
            if distance <= tol || distance > radius + r + tol || distance < radius - r - tol {
                return Vec::new();
            }
            let direction = delta / distance;
            // Compute from the smaller circle; factoring d^2-R^2 avoids the
            // cancellation of R^2-a^2 for a small hole crossing a huge circle.
            let along = ((distance - radius) * (distance + radius) + r * r) / (2.0 * distance);
            let height = ((r - along).max(0.0) * (r + along).max(0.0)).sqrt();
            let foot = small + direction * along;
            vec![foot - direction.perp() * height, foot + direction.perp() * height]
        }
        _ => intersections(a, b),
    }
}
fn clear(a: &Shape, b: &Shape, shared: &[Vec2], tol: f64) -> bool {
    let allowed = |p: Vec2| {
        shared.iter().any(|q| {
            let joint_tol = 1e-7 + 128.0 * f64::EPSILON * (1.0 + p.len().max(q.len()));
            q.dist(p) <= joint_tol
        })
    };
    // Parallel coincident lines/circles have no isolated intersections.
    // Interior overlap is nevertheless ambiguous, including duplicate boundaries.
    for s in [a, b] {
        let other = if std::ptr::eq(s, a) { b } else { a };
        let (_, end) = s.range();
        for t in [0.0, 0.5 * end, end] {
            let p = s.point(t);
            if on(other, p, tol) && !allowed(p) {
                return false;
            }
        }
    }
    stable_intersections(a, b, tol).into_iter().all(|p| !on(a, p, tol) || !on(b, p, tol) || allowed(p))
}
fn contains(boundary: &Boundary, p: Vec2, tol: f64) -> Option<bool> {
    let mut crossings = 0;
    for shape in &boundary.shapes {
        // Avoid ray/vertex coincidences rather than guessing a half-open arc rule.
        if endpoints(shape).iter().any(|q| (q.y - p.y).abs() <= tol) {
            return None;
        }
        match *shape {
            Shape::Line { a, b } => {
                if (a.y > p.y) != (b.y > p.y) {
                    let x = a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y);
                    if (x - p.x).abs() <= tol {
                        return None;
                    }
                    crossings += usize::from(x > p.x);
                }
            }
            Shape::Round { c, r, start, sweep } => {
                let y = (p.y - c.y) / r;
                if (y.abs() - 1.0).abs() <= tol / r {
                    return None;
                }
                if y.abs() < 1.0 {
                    let angle = y.asin();
                    for theta in [angle, std::f64::consts::PI - angle] {
                        let off = (theta - start).rem_euclid(TAU);
                        if off < sweep {
                            let x = c.x + r * theta.cos();
                            if (x - p.x).abs() <= tol {
                                return None;
                            }
                            crossings += usize::from(x > p.x);
                        }
                    }
                }
            }
        }
    }
    Some(crossings % 2 == 1)
}
fn boundaries(curves: &[LCurve], pts: &[Vec2]) -> Option<Vec<Boundary>> {
    let get = |i| pts.get(i).copied();
    let mut shapes = Vec::new();
    let mut ends = Vec::new();
    let mut incidence = vec![Vec::new(); pts.len()];
    for (i, curve) in curves.iter().enumerate() {
        let (shape, ep) = match *curve {
            LCurve::Line(a, b) => (Shape::Line { a: get(a)?, b: get(b)? }, Some((a, b))),
            LCurve::Arc(c, a, b) => {
                let (c, a_pos, b_pos) = (get(c)?, get(a)?, get(b)?);
                let sweep = ((b_pos - c).angle() - (a_pos - c).angle()).rem_euclid(TAU);
                (Shape::Round { c, r: c.dist(a_pos), start: (a_pos - c).angle(), sweep }, Some((a, b)))
            }
            LCurve::Circle(c, r) => (Shape::Round { c: get(c)?, r, start: 0.0, sweep: TAU }, None),
            // Existing free-form identity matching remains conservative and unchanged.
            _ => return None,
        };
        if let Some((a, b)) = ep {
            incidence.get_mut(a)?.push(i);
            incidence.get_mut(b)?.push(i);
        }
        shapes.push(shape);
        ends.push(ep);
    }
    if incidence.iter().any(|v| !v.is_empty() && v.len() != 2) {
        return None;
    }
    let mut visited = vec![false; curves.len()];
    let mut out = Vec::new();
    for first in 0..curves.len() {
        if *visited.get(first)? {
            continue;
        }
        let mut members = Vec::new();
        let mut pending = vec![first];
        while let Some(i) = pending.pop() {
            if *visited.get(i)? {
                continue;
            }
            *visited.get_mut(i)? = true;
            members.push(i);
            if let Some((a, b)) = *ends.get(i)? {
                pending.extend(incidence.get(a)?);
                pending.extend(incidence.get(b)?);
            }
        }
        let mut bounds = Aabb2::EMPTY;
        let mut signature = [0; 3];
        let mut bound_shapes = Vec::new();
        for i in &members {
            let shape = *shapes.get(*i)?;
            let mut candidates = endpoints(&shape).to_vec();
            if let Shape::Round { c, r, .. } = shape {
                for angle in [0.0, std::f64::consts::FRAC_PI_2, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2] {
                    let p = c + Vec2::from_angle(angle) * r;
                    if on(&shape, p, 1e-7) {
                        candidates.push(p);
                    }
                }
            }
            bounds.extend(candidates);
            signature[match curves.get(*i)? {
                LCurve::Line(..) => 0,
                LCurve::Arc(..) => 1,
                _ => 2,
            }] += 1;
            bound_shapes.push(shape);
        }
        out.push(Boundary { curves: members, shapes: bound_shapes, bounds, depth: 0, signature });
    }
    let scale = out.iter().map(|b| b.bounds.diagonal()).fold(1.0, f64::max);
    let tol = (scale * 1e-10).max(1e-7);
    let mut work = 0usize;
    for i in 0..out.len() {
        for j in i..out.len() {
            for (a_i, a) in out.get(i)?.shapes.iter().enumerate() {
                for (b_i, b) in out.get(j)?.shapes.iter().enumerate() {
                    if i == j && a_i >= b_i {
                        continue;
                    }
                    work += 1;
                    if work > 262_144 {
                        return None;
                    }
                    let mut shared = Vec::new();
                    if i == j {
                        let ai = *out.get(i)?.curves.get(a_i)?;
                        let bi = *out.get(j)?.curves.get(b_i)?;
                        if let (Some((a0, a1)), Some((b0, b1))) = (*ends.get(ai)?, *ends.get(bi)?) {
                            for point in [a0, a1] {
                                if point == b0 || point == b1 {
                                    shared.push(get(point)?);
                                }
                            }
                        }
                    }
                    if !clear(a, b, &shared, tol) {
                        return None;
                    }
                }
            }
        }
    }
    for i in 0..out.len() {
        let mut depth = 0;
        for j in 0..out.len() {
            if i == j {
                continue;
            }
            let shape = out.get(i)?.shapes.first()?;
            let (_, end) = shape.range();
            let inside = [0.317, 0.613, 0.823].into_iter().find_map(|t| contains(out.get(j)?, shape.point(end * t), tol))?;
            depth += usize::from(inside);
        }
        out.get_mut(i)?.depth = depth;
    }
    Some(out)
}

pub(super) fn valid(curves: &[LCurve], points: &[Vec2]) -> bool {
    boundaries(curves, points).is_some()
}
pub(super) fn roles(old_curves: &[LCurve], a: &[Vec2], fresh_curves: &[LCurve], b: &[Vec2]) -> Option<Roles> {
    let (previous, incoming) = (boundaries(old_curves, a)?, boundaries(fresh_curves, b)?);
    if previous.len() != incoming.len() {
        return None;
    }
    let bounds = |loops: &[Boundary]| loops.iter().fold(Aabb2::EMPTY, |a, x| a.union(&x.bounds));
    let (outer_a, outer_b) = (bounds(&previous), bounds(&incoming));
    let mut mapping = Vec::new();
    for x in &previous {
        let mut candidates: Vec<_> = incoming
            .iter()
            .enumerate()
            .filter(|(_, y)| x.depth == y.depth && x.signature == y.signature)
            .map(|(i, y)| (i, outer_a.relative(x.bounds.center()).dist(outer_b.relative(y.bounds.center()))))
            .collect();
        candidates.sort_by(|x, y| x.1.total_cmp(&y.1));
        let (i, d) = *candidates.first()?;
        if mapping.contains(&i) || (candidates.len() > 1 && (d >= 0.1 || candidates.get(1).is_some_and(|(_, second)| d >= second * 0.5))) {
            return None;
        }
        let y = incoming.get(i)?;
        // Preserve the conservative bound on relative boundary changes. This does
        // not mistake a stationary hole's coordinates for the outer loop's scaling.
        let old_size = outer_a.relative(x.bounds.max) - outer_a.relative(x.bounds.min);
        let new_size = outer_b.relative(y.bounds.max) - outer_b.relative(y.bounds.min);
        if old_size.dist(new_size) >= 0.1 {
            return None;
        }
        mapping.push(i);
    }
    let membership = |loops: &[Boundary], curves: &[LCurve], pts: &[Vec2], remap: Option<&[usize]>| -> Option<(Vec<Vec<usize>>, Vec<Vec2>)> {
        let mut roles = vec![Vec::new(); pts.len()];
        let mut coordinates = vec![Vec2::ZERO; pts.len()];
        for (i, loop_) in loops.iter().enumerate() {
            let target = remap.and_then(|m| m.get(i)).copied().unwrap_or(i);
            for curve in &loop_.curves {
                for p in curve_points(curves.get(*curve)?) {
                    if !roles.get(p)?.contains(&target) {
                        roles.get_mut(p)?.push(target);
                        *coordinates.get_mut(p)? += loop_.bounds.relative(*pts.get(p)?);
                    }
                }
            }
        }
        for (role, coordinate) in roles.iter_mut().zip(&mut coordinates) {
            if role.is_empty() {
                return None;
            }
            role.sort_unstable();
            *coordinate = *coordinate / role.len() as f64;
        }
        Some((roles, coordinates))
    };
    let ((old, old_normal), (fresh, fresh_normal)) =
        (membership(&previous, old_curves, a, Some(&mapping))?, membership(&incoming, fresh_curves, b, None)?);
    type Lines = Vec<Vec<Vec2>>;
    let line_roles = |loops: &[Boundary], curves: &[LCurve], pts: &[Vec2]| -> Option<(Lines, Lines)> {
        let mut roles = vec![Vec::new(); pts.len()];
        let mut leaving = vec![Vec::new(); pts.len()];
        for loop_ in loops {
            let size = loop_.bounds.max - loop_.bounds.min;
            for i in &loop_.curves {
                if let LCurve::Line(a, b) = *curves.get(*i)? {
                    let d = *pts.get(b)? - *pts.get(a)?;
                    let direction = Vec2::new(d.x / size.x.max(1e-9), d.y / size.y.max(1e-9)).normalized()?;
                    roles.get_mut(a)?.push(direction);
                    roles.get_mut(b)?.push(direction);
                    leaving.get_mut(a)?.push(direction);
                    leaving.get_mut(b)?.push(direction * -1.0);
                }
            }
        }
        Some((roles, leaving))
    };
    let (old_lines, old_leaving) = line_roles(&previous, old_curves, a)?;
    let (fresh_lines, fresh_leaving) = line_roles(&incoming, fresh_curves, b)?;
    Some(Roles { old, fresh, old_normal, fresh_normal, old_lines, fresh_lines, old_leaving, fresh_leaving })
}
