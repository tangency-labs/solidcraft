//! Rotation-minimizing quad sweeps and closed-loop frame correction.

use super::{bad, length};
use crate::eval::vec::VecExt;
use crate::geom::form_cage::{CageGeometry, MAX_FACES, MAX_POINTS};
use crate::{Result, Vec3};
use std::f64::consts::{PI, TAU};

/// Maximum gap between source path endpoints that are meant to join.
pub const JOIN_TOLERANCE: f64 = 1e-7;

/// A swept cage and its directed, ordered terminal rings.
pub struct Sweep {
    /// Quad cage before attaching other branches.
    pub cage: CageGeometry,
    /// Initial and final rings in section order.
    pub ends: [Vec<usize>; 2],
    /// Path tangents at the two ends, both in the sweep direction.
    pub tangents: [Vec3; 2],
}

/// Sweep a sampled path. A closed path has no terminal caps. An open path may
/// leave either terminal ring open for a junction to attach to it.
pub fn tube(
    input: &[Vec3],
    radius: f64,
    density: usize,
    closed: bool,
    caps: [bool; 2],
) -> Result<Sweep> {
    if !(1..=12).contains(&density)
        || !(0.05..=5_000.0).contains(&radius)
        || !(2..=4096).contains(&input.len())
        || input
            .iter()
            .any(|&point| !point.is_finite() || point.len() > 100_000.0)
    {
        return Err(bad(
            "pipe sweep needs a finite bounded path, radius and section density 1..12",
        ));
    }
    let points = if closed {
        let (last, points) = input.split_last().ok_or_else(|| bad("empty pipe path"))?;
        if points
            .first()
            .is_none_or(|&first| first.dist(*last) > JOIN_TOLERANCE)
        {
            return Err(bad("a closed pipe path must return to its first point"));
        }
        if points.len() < 3 {
            return Err(bad(
                "a closed pipe needs at least three distinct path samples",
            ));
        }
        points
    } else {
        input
    };
    let sides = 8 * density;
    let caps_count = if closed {
        0
    } else {
        caps.into_iter().filter(|cap| *cap).count()
    };
    let side_layers = if closed {
        points.len()
    } else {
        points.len() - 1
    };
    if sides * points.len() + caps_count * (2 * density - 1).pow(2) > MAX_POINTS
        || sides * side_layers + caps_count * (2 * density).pow(2) > MAX_FACES
    {
        return Err(bad("pipe sweep exceeds its control-cage budget"));
    }
    let frames = frames(points, closed)?;
    let mut cage = CageGeometry {
        points: Vec::new(),
        faces: Vec::new(),
    };
    for (&center, &(u, v, _)) in points.iter().zip(&frames) {
        for j in 0..sides {
            let angle = TAU * j as f64 / sides as f64;
            cage.points
                .push(center + (u * angle.cos() + v * angle.sin()) * radius);
        }
    }
    for i in 0..side_layers {
        let next = if i + 1 == points.len() { 0 } else { i + 1 };
        for j in 0..sides {
            let k = if j + 1 == sides { 0 } else { j + 1 };
            cage.faces.push(vec![
                i * sides + j,
                i * sides + k,
                next * sides + k,
                next * sides + j,
            ]);
        }
    }
    let ends: [Vec<_>; 2] =
        [0, points.len() - 1].map(|i| (0..sides).map(|j| i * sides + j).collect());
    let (&first, &first_frame) = points
        .first()
        .zip(frames.first())
        .ok_or_else(|| bad("empty pipe path"))?;
    let (&last, &last_frame) = points
        .last()
        .zip(frames.last())
        .ok_or_else(|| bad("empty pipe path"))?;
    let tangents = [first_frame.2, last_frame.2];
    if !closed {
        for ((center, frame, reverse), (cap, ring)) in
            [(first, first_frame, true), (last, last_frame, false)]
                .into_iter()
                .zip(caps.into_iter().zip(&ends))
        {
            if cap {
                add_cap(&mut cage, center, frame, radius, density, ring, reverse)?;
            }
        }
    }
    Ok(Sweep {
        cage,
        ends,
        tangents,
    })
}

fn add_cap(
    cage: &mut CageGeometry,
    center: Vec3,
    (u, v, _): (Vec3, Vec3, Vec3),
    radius: f64,
    density: usize,
    ring: &[usize],
    reverse: bool,
) -> Result<()> {
    let n = 2 * density;
    let mut grid = Vec::with_capacity((n + 1).pow(2));
    for j in 0..=n {
        for i in 0..=n {
            let (x, y) = (
                i as f64 / density as f64 - 1.0,
                j as f64 / density as f64 - 1.0,
            );
            let distance = x.abs().max(y.abs());
            let (x, y) = if distance > 0.0 {
                (x / distance, y / distance)
            } else {
                (0.0, 0.0)
            };
            let angle = if x.abs() >= y.abs() {
                if x >= 0.0 {
                    PI / 4.0 * y
                } else {
                    PI - PI / 4.0 * y
                }
            } else if y >= 0.0 {
                PI / 2.0 - PI / 4.0 * x
            } else {
                -PI / 2.0 + PI / 4.0 * x
            };
            let id = if i == 0 || i == n || j == 0 || j == n {
                let index = (angle.rem_euclid(TAU) / TAU * ring.len() as f64).round() as usize;
                index
                    .checked_rem(ring.len())
                    .and_then(|i| ring.get(i))
                    .copied()
                    .ok_or_else(|| bad("pipe cap lost a perimeter vertex"))?
            } else {
                let id = cage.points.len();
                cage.points
                    .push(center + (u * angle.cos() + v * angle.sin()) * (radius * distance));
                id
            };
            grid.push(id);
        }
    }
    let at = |index| {
        grid.get(index)
            .copied()
            .ok_or_else(|| bad("pipe cap lost a grid vertex"))
    };
    for j in 0..n {
        for i in 0..n {
            let a = j * (n + 1) + i;
            let mut face = vec![at(a)?, at(a + 1)?, at(a + n + 2)?, at(a + n + 1)?];
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
    (rotated - b * rotated.dot(b))
        .normalized()
        .ok_or_else(|| bad("pipe frame lost a perpendicular direction"))
}

type Frame = (Vec3, Vec3, Vec3);

fn frames(points: &[Vec3], closed: bool) -> Result<Vec<Frame>> {
    let segments = points
        .iter()
        .copied()
        .zip(points.iter().copied().cycle().skip(1));
    let directions = segments
        .take(if closed {
            points.len()
        } else {
            points.len() - 1
        })
        .map(|(a, b)| {
            (b - a)
                .normalized()
                .ok_or_else(|| bad("pipe path has repeated samples"))
        })
        .collect::<Result<Vec<_>>>()?;
    let first = *directions.first().ok_or_else(|| bad("empty pipe path"))?;
    let last = *directions.last().ok_or_else(|| bad("empty pipe path"))?;
    let mut tangents = Vec::with_capacity(points.len());
    let bisect = |a, b| {
        <Vec3 as VecExt>::normalized(a + b)
            .ok_or_else(|| bad("pipe path reverses direction at a sample"))
    };
    tangents.push(if closed { bisect(last, first)? } else { first });
    for (&a, &b) in directions.iter().zip(directions.iter().skip(1)) {
        tangents.push(bisect(a, b)?);
    }
    if !closed {
        tangents.push(last);
    }
    let start = *tangents.first().ok_or_else(|| bad("empty pipe path"))?;
    let axis = if start.x.abs() < 0.9 {
        Vec3::X
    } else {
        Vec3::Y
    };
    let mut up = start
        .cross(axis)
        .normalized()
        .unwrap_or(Vec3::Z)
        .normalized()
        .ok_or_else(|| bad("pipe frame has no first perpendicular"))?;
    let mut out = Vec::with_capacity(points.len());
    let mut previous = start;
    for (i, tangent) in tangents.into_iter().enumerate() {
        if i > 0 {
            up = transport(up, previous, tangent)?;
        }
        out.push((up, tangent.cross(up), tangent));
        previous = tangent;
    }
    if closed {
        let arrived = transport(up, previous, start)?;
        let first_up = out.first().ok_or_else(|| bad("empty pipe frame"))?.0;
        let correction = start
            .dot(arrived.cross(first_up))
            .atan2(arrived.dot(first_up));
        let (first, last) = points
            .first()
            .zip(points.last())
            .ok_or_else(|| bad("empty pipe path"))?;
        let total = length(points)? + first.dist(*last);
        let mut walked = 0.0;
        let distances = std::iter::once(0.0).chain(
            points
                .iter()
                .zip(points.iter().skip(1))
                .map(|(&a, &b)| a.dist(b)),
        );
        for (frame, distance) in out.iter_mut().zip(distances) {
            walked += distance;
            let angle = correction * walked / total;
            let (u, v, tangent) = *frame;
            let up = u * angle.cos() + v * angle.sin();
            *frame = (up, tangent.cross(up), tangent);
        }
    }
    Ok(out)
}
