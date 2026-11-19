//! Bounded identity witnesses for ordered points that moved a small distance.
//!
//! An ordered candidate is accepted only when it remains uniquely nearer than every
//! competing candidate. Sorting both axes bounds the candidate search for sparse data;
//! dense or ambiguous data is refused when its work budget is exhausted.

use crate::math::Vec2;
use crate::{Result, SolidError};

/// Whether every ordered candidate is nearby and clearly its old point's nearest match.
///
/// The caller establishes the point-sharing graph separately. This witnesses identity,
/// not unchanged geometry: accepted candidates must still be applied by the caller.
/// Nearness permits `within * (1 + max(|old|, |new|))` displacement. Candidate searches
/// use twice the actual displacement, keeping small edits bounded far from the origin.
/// Returns false for ambiguous matches or exhausted work. Invalid inputs return an error.
pub fn ordered_nearby_points(
    old: &[Vec2],
    new: &[Vec2],
    within: f64,
    mut work: usize,
) -> Result<bool> {
    const OP: &str = "query::point_identity::ordered_nearby_points";
    if old.len() != new.len()
        || old.len() > 20_000
        || !(within.is_finite() && within > 0.0)
        || old
            .iter()
            .chain(new)
            .any(|p| !p.x.is_finite() || !p.y.is_finite())
    {
        return Err(SolidError::invalid_input(
            OP,
            "invalid point identity inputs",
        ));
    }
    let mut xs: Vec<_> = new.iter().copied().enumerate().collect();
    let mut ys = xs.clone();
    xs.sort_by(|a, b| a.1.x.total_cmp(&b.1.x).then(a.0.cmp(&b.0)));
    ys.sort_by(|a, b| a.1.y.total_cmp(&b.1.y).then(a.0.cmp(&b.0)));
    for (i, (p, q)) in old.iter().zip(new).enumerate() {
        let distance = (p.x - q.x).hypot(p.y - q.y);
        let scale = 1.0 + p.x.hypot(p.y).max(q.x.hypot(q.y));
        let allowance = within * scale;
        let radius = 2.0 * distance;
        let (xlo, xhi) = (p.x - radius, p.x + radius);
        let (ylo, yhi) = (p.y - radius, p.y + radius);
        if !(distance.is_finite()
            && scale.is_finite()
            && allowance.is_finite()
            && distance <= allowance
            && radius.is_finite()
            && xlo.is_finite()
            && xhi.is_finite()
            && ylo.is_finite()
            && yhi.is_finite())
        {
            return Ok(false);
        }
        let xr = xs.partition_point(|(_, q)| q.x < xlo)..xs.partition_point(|(_, q)| q.x <= xhi);
        let yr = ys.partition_point(|(_, q)| q.y < ylo)..ys.partition_point(|(_, q)| q.y <= yhi);
        let candidates = if xr.len() <= yr.len() {
            xs.get(xr)
        } else {
            ys.get(yr)
        };
        let Some(candidates) = candidates else {
            return Err(SolidError::invalid_input(OP, "invalid point search range"));
        };
        for (j, candidate) in candidates {
            let Some(remaining) = work.checked_sub(1) else {
                return Ok(false);
            };
            work = remaining;
            if i == *j {
                continue;
            }
            let other = (p.x - candidate.x).hypot(p.y - candidate.y);
            if !other.is_finite() || distance >= 0.75 * other {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_moves_are_distinct_from_permutations_and_ambiguous_matches() {
        let old: Vec<_> = (0..20_000).map(|i| Vec2::new(f64::from(i), 0.0)).collect();
        let new: Vec<_> = old.iter().map(|p| Vec2::new(p.x, 1e-8)).collect();
        assert!(ordered_nearby_points(&old, &new, 1e-6, 100_000).unwrap());
        assert!(!ordered_nearby_points(&old, &new, 1e-6, 0).unwrap());
        let old = [Vec2::new(0.0, 0.0), Vec2::new(2e-8, 0.0)];
        assert!(!ordered_nearby_points(&old, &[old[1], old[0]], 1e-6, 100).unwrap());
        assert!(!ordered_nearby_points(&old, &[Vec2::new(1e-8, 0.0); 2], 1e-6, 100).unwrap());
        assert!(ordered_nearby_points(&old, &old, f64::NAN, 100).is_err());
    }

    #[test]
    fn translated_small_relative_moves_keep_a_bounded_unique_match() {
        for (offset, step, displacement) in [(1_000.0, 1.0, 1e-5), (100_000.0, 0.01, 1e-5)] {
            let old: Vec<_> = (0..20_000)
                .map(|i| Vec2::new(offset + f64::from(i) * step, offset))
                .collect();
            let new: Vec<_> = old
                .iter()
                .map(|p| Vec2::new(p.x + displacement, p.y))
                .collect();
            // The relative allowance includes many neighbours at the large offset;
            // a search over that allowance would exhaust this budget.
            assert!(ordered_nearby_points(&old, &new, 1e-6, 20_000).unwrap());
            assert!(!ordered_nearby_points(&old, &new, 1e-6, 19_999).unwrap());
        }
        let old = [
            Vec2::new(100_000.0, 100_000.0),
            Vec2::new(100_000.001, 100_000.0),
        ];
        assert!(!ordered_nearby_points(&old, &[old[1], old[0]], 1e-6, 100).unwrap());
    }

    #[test]
    fn overflowing_identity_arithmetic_refuses() {
        let old = [Vec2::new(f64::MAX, 0.0)];
        let new = [Vec2::new(-f64::MAX, 0.0)];
        assert!(!ordered_nearby_points(&old, &new, 1e-6, 100).unwrap());
        assert!(!ordered_nearby_points(&old, &old, 2.0, 100).unwrap());
        let old = [Vec2::new(f64::MAX, f64::MAX)];
        assert!(!ordered_nearby_points(&old, &old, 1e-6, 100).unwrap());
        // Both axis windows include the other point, whose diagonal distance overflows.
        let old = [Vec2::new(0.0, 0.0), Vec2::new(-1.3e308, -1.3e308)];
        let new = [Vec2::new(6.5e307, 0.0), Vec2::new(-1.3e308, -1.3e308)];
        assert!(!ordered_nearby_points(&old, &new, 1.0, 100).unwrap());
    }
}
