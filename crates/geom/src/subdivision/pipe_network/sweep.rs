//! Rotation-minimizing rings with distributed closed-loop frame correction.
use super::*;
use std::f64::consts::{PI, TAU};

pub(super) struct Sweep {
    pub cage: Cage,
    pub ends: [Vec<usize>; 2],
    pub tangents: [Vec3; 2],
}

/// Sweep a sampled path. A closed path has no terminal caps; an open path may
/// leave either terminal ring open for a network junction to attach to it.
pub fn swept_path(points: &[Vec3], radius: f64, density: usize, closed: bool, caps: [bool; 2]) -> Result<Cage> {
    Ok(tube(points, radius, density, closed, caps)?.cage)
}
pub(super) fn tube(input: &[Vec3], radius: f64, density: usize, closed: bool, caps: [bool; 2]) -> Result<Sweep> {
    if !(1..=12).contains(&density)
        || !radius.is_finite()
        || !(0.05..=5_000.0).contains(&radius)
        || !(2..=4096).contains(&input.len())
        || input.iter().any(|p| !p.is_finite() || p.len() > 100_000.0)
    {
        return Err(bad("pipe sweep needs a finite bounded path, radius and section density 1..12"));
    }
    let mut points = input.to_vec();
    if closed {
        if points.first().zip(points.last()).is_none_or(|(a, b)| a.dist(*b) > JOIN_TOLERANCE) {
            return Err(bad("a closed pipe path must return to its first point"));
        }
        points.pop();
        if points.len() < 3 {
            return Err(bad("a closed pipe needs at least three distinct path samples"));
        }
    }
    let sides = 8 * density;
    let caps_count = if closed { 0 } else { caps.into_iter().filter(|cap| *cap).count() };
    let side_layers = if closed { points.len() } else { points.len() - 1 };
    if sides * points.len() + caps_count * (2 * density - 1).pow(2) > MAX_POINTS
        || sides * side_layers + caps_count * (2 * density).pow(2) > MAX_FACES
    {
        return Err(bad("pipe sweep exceeds its control-cage budget"));
    }
    let frames = frames(&points, closed)?;
    let mut cage = Cage::default();
    for (&center, &(u, v, _)) in points.iter().zip(&frames) {
        for j in 0..sides {
            let a = TAU * j as f64 / sides as f64;
            cage.points.push(center + (u * a.cos() + v * a.sin()) * radius);
        }
    }
    for i in 0..side_layers {
        let next = (i + 1) % points.len();
        for j in 0..sides {
            let k = (j + 1) % sides;
            cage.faces.push(vec![i * sides + j, i * sides + k, next * sides + k, next * sides + j]);
        }
    }
    let ends: [Vec<_>; 2] = [0, points.len() - 1].map(|i| (0..sides).map(|j| i * sides + j).collect());
    if !closed {
        for (end, &cap) in caps.iter().enumerate() {
            if cap {
                let i = if end == 0 { 0 } else { points.len() - 1 };
                add_cap(&mut cage, points[i], frames[i], radius, density, &ends[end], end == 0)?;
            }
        }
    }
    budget(&cage)?;
    cage.validate()?;
    Ok(Sweep { cage, ends, tangents: [frames[0].2, frames[frames.len() - 1].2] })
}

fn add_cap(cage: &mut Cage, center: Vec3, (u, v, _): (Vec3, Vec3, Vec3), radius: f64, density: usize, ring: &[usize], reverse: bool) -> Result<()> {
    let n = 2 * density;
    let mut grid = Vec::with_capacity((n + 1).pow(2));
    for j in 0..=n {
        for i in 0..=n {
            let (x, y) = (i as f64 / density as f64 - 1.0, j as f64 / density as f64 - 1.0);
            let distance = x.abs().max(y.abs());
            let (x, y) = if distance > 0.0 { (x / distance, y / distance) } else { (0.0, 0.0) };
            let angle = if x.abs() >= y.abs() {
                if x >= 0.0 { PI / 4.0 * y } else { PI - PI / 4.0 * y }
            } else if y >= 0.0 {
                PI / 2.0 - PI / 4.0 * x
            } else {
                -PI / 2.0 + PI / 4.0 * x
            };
            let id = if i == 0 || i == n || j == 0 || j == n {
                let index = (angle.rem_euclid(TAU) / TAU * ring.len() as f64).round() as usize % ring.len();
                *ring.get(index).ok_or_else(|| bad("pipe cap lost a perimeter vertex"))?
            } else {
                let id = cage.points.len();
                cage.points.push(center + (u * angle.cos() + v * angle.sin()) * (radius * distance));
                id
            };
            grid.push(id);
        }
    }
    for j in 0..n {
        for i in 0..n {
            let a = j * (n + 1) + i;
            let mut face = vec![grid[a], grid[a + 1], grid[a + n + 2], grid[a + n + 1]];
            if reverse {
                face.reverse();
            }
            cage.faces.push(face);
        }
    }
    Ok(())
}

fn transport(u: Vec3, a: Vec3, b: Vec3) -> Result<Vec3> {
    let dot = a.dot(b).clamp(-1.0, 1.0);
    if dot < -1.0 + 1e-10 {
        return Err(bad("pipe path reverses direction"));
    }
    let axis = a.cross(b);
    let rotated = u + axis.cross(u) + axis.cross(axis.cross(u)) / (1.0 + dot);
    (rotated - b * rotated.dot(b)).normalized().ok_or_else(|| bad("pipe frame lost a perpendicular direction"))
}
fn frames(points: &[Vec3], closed: bool) -> Result<Vec<(Vec3, Vec3, Vec3)>> {
    let n = points.len();
    let directions = (0..if closed { n } else { n - 1 })
        .map(|i| (points[(i + 1) % n] - points[i]).normalized().ok_or_else(|| bad("pipe path has repeated samples")))
        .collect::<Result<Vec<_>>>()?;
    let tangents = (0..n)
        .map(|i| {
            if !closed && i == 0 {
                return Ok(directions[0]);
            }
            if !closed && i + 1 == n {
                return Ok(directions[n - 2]);
            }
            (directions[(i + directions.len() - 1) % directions.len()] + directions[i % directions.len()])
                .normalized()
                .ok_or_else(|| bad("pipe path reverses direction at a sample"))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut up = tangents[0].any_perp().normalized().ok_or_else(|| bad("pipe frame has no first perpendicular"))?;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        if i > 0 {
            up = transport(up, tangents[i - 1], tangents[i])?;
        }
        out.push((up, tangents[i].cross(up), tangents[i]));
    }
    if closed {
        let arrived = transport(up, tangents[n - 1], tangents[0])?;
        let correction = tangents[0].dot(arrived.cross(out[0].0)).atan2(arrived.dot(out[0].0));
        let total = length(points) + points[n - 1].dist(points[0]);
        let mut walked = 0.0;
        for i in 0..n {
            if i > 0 {
                walked += points[i - 1].dist(points[i]);
            }
            let angle = correction * walked / total;
            let (u, v, tangent) = out[i];
            let up = u * angle.cos() + v * angle.sin();
            out[i] = (up, tangent.cross(up), tangent);
        }
    }
    Ok(out)
}
