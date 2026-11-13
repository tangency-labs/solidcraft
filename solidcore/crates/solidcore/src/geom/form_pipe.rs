//! Arc-length sampling and branch clearances of sampled Form pipe paths.

use crate::{Result, SolidError, Vec3};

pub mod junction;
pub mod sweep;
pub use sweep::JOIN_TOLERANCE;

/// Maximum number of source samples in a pipe network.
pub const MAX_PATH_SAMPLES: usize = 32_768;
/// Maximum number of named source paths in a pipe network.
pub const MAX_PATHS: usize = 128;

/// Check the junction branching limit before converting or constructing geometry.
pub fn validate_branch_count(count: usize) -> Result<()> {
    if count > 6 {
        return Err(bad("pipe junction supports at most six branches"));
    }
    Ok(())
}

fn bad(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: "Form pipe",
        reason,
    }
}

fn segments(points: &[Vec3]) -> impl Iterator<Item = [Vec3; 2]> + '_ {
    points
        .iter()
        .copied()
        .zip(points.iter().copied().skip(1))
        .map(|(a, b)| [a, b])
}

/// Length of a bounded sampled path.
pub fn length(points: &[Vec3]) -> Result<f64> {
    if !(2..=MAX_PATH_SAMPLES).contains(&points.len()) {
        return Err(bad("a path requires at least two bounded samples"));
    }
    let total = segments(points).map(|[a, b]| (b - a).length()).sum::<f64>();
    if !total.is_finite() {
        return Err(bad("pipe chain has no finite length"));
    }
    Ok(total)
}

fn at_length(points: &[Vec3], mut distance: f64) -> Result<Vec3> {
    for [a, b] in segments(points) {
        let step = b - a;
        let length = step.length();
        if distance <= length && length > 0.0 {
            return Ok(a + step * (distance / length).clamp(0.0, 1.0));
        }
        distance -= length;
    }
    points
        .last()
        .copied()
        .ok_or_else(|| bad("pipe source edge has no endpoint"))
}

/// Retain a sampled path between two distances measured from its ends.
pub fn trim(points: &[Vec3], start: f64, end: f64) -> Result<Vec<Vec3>> {
    if !start.is_finite() || !end.is_finite() || start < 0.0 || end < 0.0 {
        return Err(bad("invalid pipe branch clearance"));
    }
    let total = length(points)?;
    if start + end >= total * 0.9 {
        return Err(bad(
            "pipe branches are too short for this diameter; reduce the diameter or lengthen the path",
        ));
    }
    let mut out = vec![at_length(points, start)?];
    let mut walked = 0.0;
    for [a, b] in segments(points) {
        walked += (b - a).length();
        if walked > start + 1e-9 && walked < total - end - 1e-9 {
            out.push(b);
        }
    }
    out.push(at_length(points, total - end)?);
    Ok(out)
}

/// Allocate the average number of intervals by arc length, with at least one
/// interval per source path. Each pair gives one path and whether it runs forward.
pub fn sample(paths: &[(&[Vec3], bool)], average: usize, minimum: usize) -> Result<Vec<Vec3>> {
    if !(1..=MAX_PATHS).contains(&paths.len())
        || !(1..=20).contains(&average)
        || minimum > 3
        || paths
            .iter()
            .fold(0usize, |n, (path, _)| n.saturating_add(path.len()))
            > MAX_PATH_SAMPLES
    {
        return Err(bad("pipe path sample budget exceeded"));
    }
    let lengths = paths
        .iter()
        .map(|(points, _)| length(points))
        .collect::<Result<Vec<_>>>()?;
    let total = lengths.iter().sum::<f64>();
    if !total.is_finite() || total <= 1e-7 {
        return Err(bad("pipe chain has no finite length"));
    }
    let target = (average * paths.len()).max(minimum);
    let mut counts = vec![1usize; paths.len()];
    for _ in paths.len()..target {
        let mut best = None;
        for (id, (&length, &count)) in lengths.iter().zip(&counts).enumerate() {
            // Keep the earliest path on a tie, as in stable source ordering.
            let width = length / count as f64;
            if best.is_none_or(|(_, old)| width > old) {
                best = Some((id, width));
            }
        }
        let (widest, _) = best.ok_or_else(|| bad("empty sampled pipe path"))?;
        *counts
            .get_mut(widest)
            .ok_or_else(|| bad("missing sample path"))? += 1;
    }
    let mut out = Vec::new();
    for (((points, forward), count), length) in paths.iter().zip(counts).zip(lengths) {
        for i in usize::from(!out.is_empty())..=count {
            let t = i as f64 / count as f64;
            out.push(at_length(
                points,
                if *forward {
                    t * length
                } else {
                    (1.0 - t) * length
                },
            )?);
        }
    }
    Ok(out)
}

/// Clearance for one junction, from each outward branch chord and diameter.
pub fn clearance(chords: &[([Vec3; 2], f64)]) -> Result<f64> {
    validate_branch_count(chords.len())?;
    let mut clearance = 0.0_f64;
    let mut directions = Vec::new();
    for &([start, next], diameter) in chords {
        let chord = next - start;
        let len = chord.length();
        if !len.is_finite() || len <= 1e-9 || !(0.1..=10_000.0).contains(&diameter) {
            return Err(bad(
                "pipe junction has no finite branch direction or diameter",
            ));
        }
        directions.push(chord / len);
        clearance = clearance.max(diameter * 1.1);
    }
    let mut sine = 1.0_f64;
    for (i, a) in directions.iter().enumerate() {
        for b in directions.iter().skip(i + 1) {
            let dot = a.dot(*b).clamp(-1.0, 1.0);
            if dot > 10.0_f64.to_radians().cos() {
                return Err(bad(
                    "pipe branches meet at an angle too small for a junction",
                ));
            }
            sine = sine.min(((1.0 - dot) * 0.5).sqrt());
        }
    }
    Ok(clearance * (0.75 / sine).max(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampling_respects_direction_and_trimming_follows_arc_length() {
        let a = [Vec3::ZERO, Vec3::X];
        let b = [Vec3::X * 10.0, Vec3::X];
        let path = sample(&[(&a, true), (&b, false)], 3, 1).unwrap();
        assert_eq!(path.len(), 7);
        assert_eq!(path[1], Vec3::X);
        assert_eq!(path[6], Vec3::X * 10.0);
        assert_eq!(
            trim(&[Vec3::ZERO, Vec3::X, Vec3::X + Vec3::Y], 0.5, 0.25).unwrap(),
            [Vec3::X * 0.5, Vec3::X, Vec3::X + Vec3::Y * 0.75]
        );
        assert!(trim(&a, 0.7, 0.3).is_err());
    }

    #[test]
    fn branch_clearance_responds_to_angle_and_rejects_a_folded_path() {
        let branch = |axis, diameter| ([Vec3::ZERO, axis], diameter);
        assert!(
            (clearance(&[branch(Vec3::X, 2.0), branch(Vec3::Y, 1.0)]).unwrap()
                - 2.2 * 0.75 * 2.0_f64.sqrt())
            .abs()
                < 1e-12
        );
        assert!(clearance(&[branch(Vec3::X, 2.0), branch(Vec3::X + Vec3::Y * 0.01, 1.0)]).is_err());
    }
}
