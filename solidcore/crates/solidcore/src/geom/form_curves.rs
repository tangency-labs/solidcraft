//! Geometry tests for ordered curve controls and directed rim stations.
use crate::geom::affine_fit;
use crate::{Transform, Vec3};

mod sampling;
pub use sampling::chord_samples;

/// Carry an authored coefficient from the ordinary subdivision stencil to its continuation.
pub fn continued_control(point: Vec3, ordinary: Vec3, continued: Vec3) -> Vec3 {
    point + (continued - ordinary)
}

/// Whether two finite chart stations agree at the supplied distance.
pub fn stations_agree(a: Vec3, b: Vec3, tolerance: f64) -> bool {
    a.distance(b) <= tolerance
}

/// Evaluate a chart station in its fitted tensor and place it in the Form frame.
pub fn fitted_station<C>(
    degree: [usize; 2],
    knots: &[Vec<f64>; 2],
    count: [usize; 2],
    controls: &[C],
    uv: [f64; 2],
    placement: &Transform,
    components: impl Fn(&C) -> [f64; 4],
) -> Option<Vec3> {
    for (((parameter, degree), count), knots) in uv.into_iter().zip(degree).zip(count).zip(knots) {
        if !(*knots.get(degree)? <= parameter && parameter <= *knots.get(count)?) {
            return None;
        }
    }
    let (p, _, _) =
        crate::eval::plan::nurbs_derivs(degree, knots, count, controls, uv, components)?;
    let point = affine_fit::point(placement, p);
    point
        .to_array()
        .iter()
        .all(|x| x.is_finite())
        .then_some(point)
}

/// Return the first plane's normal if the remaining oriented planes coincide to roundoff.
pub fn coplanar_normal(mut planes: impl Iterator<Item = (Vec3, Vec3, Vec3)>) -> Option<Vec3> {
    let (origin, x, y) = planes.next()?;
    let normal = x.cross(y);
    let tolerance = 64.0 * f64::EPSILON;
    planes
        .all(|(next, x, y)| {
            let height = (tolerance * (1.0 + origin.length().max(next.length()))).min(1e-7);
            normal.distance(x.cross(y)) <= tolerance && (next - origin).dot(normal).abs() <= height
        })
        .then_some(normal)
}

/// Offsets for the two caps of a planar wall and the sewing tolerance.
pub fn wall_offsets(normal: Vec3, thickness: f64, symmetric: bool) -> Option<([Vec3; 2], f64)> {
    if !thickness.is_finite() || thickness.abs() <= 1e-9 {
        return None;
    }
    let delta = normal
        * if symmetric {
            thickness.abs()
        } else {
            thickness
        };
    let start = if symmetric { -delta / 2.0 } else { Vec3::ZERO };
    Some(([start, start + delta], (thickness.abs() * 0.01).min(1e-7)))
}

/// A tensor strip joining two translated copies of a trimmed boundary curve.
pub fn swept_controls(
    degree: usize,
    knots: &[f64],
    controls: &[[f64; 4]],
    [a, b]: [f64; 2],
    offsets: [Vec3; 2],
    work: &mut usize,
) -> Option<(Vec<f64>, Vec<[f64; 4]>)> {
    if !(1..=16).contains(&degree)
        || controls.len() <= degree
        || knots.len() != controls.len() + degree + 1
    {
        return None;
    }
    *work = work.checked_sub(controls.len())?;
    let (knots, trimmed) = crate::eval::pcurve::segment(degree, knots, controls, a, b).ok()?;
    *work = work.checked_sub(trimmed.len().checked_mul(2)?)?;
    let rows = offsets
        .into_iter()
        .flat_map(|v| {
            trimmed
                .iter()
                .map(move |&[x, y, z, w]| [x + w * v.x, y + w * v.y, z + w * v.z, w])
        })
        .collect();
    Some((knots, rows))
}

/// A uniquely identified cyclic shift of a reversed rim.
pub enum RimPhase {
    /// No cyclic phase fits within the tolerance.
    Missing,
    /// The index on the second rim paired with the first edge.
    Unique(usize),
    /// Multiple cyclic phases fit within the tolerance.
    Ambiguous,
}

/// Match a directed edge's endpoints, retaining both possible orientations.
pub fn endpoint_directions([a, b]: [Vec3; 2], [c, d]: [Vec3; 2], tolerance: f64) -> [bool; 2] {
    [
        a.distance(c) <= tolerance && b.distance(d) <= tolerance,
        a.distance(d) <= tolerance && b.distance(c) <= tolerance,
    ]
}

/// Identify a reversed cyclic rim phase by both endpoints of every edge.
/// A match nominates a phase for subsequent complete-curve qualification.
pub fn reversed_rim_phase(first: &[[Vec3; 2]], second: &[[Vec3; 2]], tolerance: f64) -> RimPhase {
    let n = first.len();
    if n != second.len() || !(3..=256).contains(&n) || !tolerance.is_finite() || tolerance < 0.0 {
        return RimPhase::Missing;
    }
    let mut phases = (0..n).filter(|&phase| {
        first
            .iter()
            .zip(second.iter().rev().cycle().skip(n - 1 - phase))
            .all(|(&a, &b)| {
                let [_, reversed] = endpoint_directions(a, b, tolerance);
                reversed
            })
    });
    match (phases.next(), phases.next()) {
        (Some(index), None) => RimPhase::Unique(index),
        (Some(_), Some(_)) => RimPhase::Ambiguous,
        (None, _) => RimPhase::Missing,
    }
}

/// Euclidean controls in the supplied source frame and weights divided by the first weight.
pub fn rational_controls_in_frame(
    controls: &[[f64; 4]],
    inverse: &Transform,
) -> Option<(Vec<Vec3>, Vec<f64>)> {
    if !(2..=256).contains(&controls.len()) {
        return None;
    }
    let &[_, _, _, first] = controls.first()?;
    if !first.is_finite() || first <= 0.0 {
        return None;
    }
    controls
        .iter()
        .map(|&[x, y, z, w]| {
            if !w.is_finite() || w <= 0.0 {
                return None;
            }
            let point = affine_fit::point(inverse, Vec3::new(x / w, y / w, z / w));
            let weight = w / first;
            (point.to_array().iter().all(|p| p.is_finite()) && weight.is_finite() && weight > 0.0)
                .then_some((point, weight))
        })
        .collect::<Option<Vec<_>>>()
        .map(|controls| controls.into_iter().unzip())
}

/// Check whether an ordered net follows the original after translation and
/// uniform positive scaling. Preserving the order retains the curve's seam and direction.
pub fn controls_follow(
    saved: impl Iterator<Item = Vec3> + Clone + ExactSizeIterator,
    current: impl Iterator<Item = Vec3> + Clone + ExactSizeIterator,
) -> bool {
    if saved.len() != current.len() || !(2..=256).contains(&saved.len()) {
        return false;
    }
    let n = saved.len() as f64;
    let before = saved.clone().fold(Vec3::ZERO, |a, p| a + p) / n;
    let after = current.clone().fold(Vec3::ZERO, |a, p| a + p) / n;
    let old_size = saved
        .clone()
        .map(|p| (p - before).length_squared())
        .sum::<f64>();
    let new_size = current
        .clone()
        .map(|p| (p - after).length_squared())
        .sum::<f64>();
    let scale = (new_size / old_size).sqrt();
    if !scale.is_finite() || !(1e-9..=1e9).contains(&scale) {
        return false;
    }
    let extent = (new_size / n).sqrt();
    let position = current
        .clone()
        .flat_map(|p| p.to_array())
        .map(f64::abs)
        .fold(0.0_f64, f64::max);
    let tolerance = extent * 1e-8 + position * 64.0 * f64::EPSILON;
    tolerance.is_finite()
        && tolerance < extent * 0.01
        && saved
            .zip(current)
            .all(|(a, b)| (after + (a - before) * scale).distance(b) <= tolerance)
}

/// Compare a subdivision continuation to the old coefficients after refinement,
/// permitting arithmetic roundoff independently of the modeling tolerance.
pub fn continued_control_within_roundoff(actual: Vec3, expected: Vec3, levels: usize) -> bool {
    let coordinates = actual.to_array().into_iter().chain(expected.to_array());
    if !coordinates.clone().all(f64::is_finite) {
        return false;
    }
    let scale = coordinates.map(f64::abs).fold(1.0_f64, f64::max);
    let levels = levels as f64 + 1.0;
    let roundoff = 128.0 * f64::EPSILON * scale * (levels * levels);
    actual.distance(expected) <= roundoff
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_a_weighted_corner_in_its_rigid_placement() {
        let knots = [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]];
        let controls = [
            [0.0, 0.0, 0.0, 1.0],
            [4.0, 0.0, 0.0, 2.0],
            [0.0, 3.0, 0.0, 1.0],
            [4.0, 6.0, 0.0, 2.0],
        ];
        let placement = Transform::from_axes(Vec3::Y, Vec3::Z, Vec3::X, Vec3::new(9.0, 8.0, 7.0));
        let at = |uv| fitted_station([1, 1], &knots, [2, 2], &controls, uv, &placement, |p| *p);
        assert_eq!(at([1.0, 1.0]), Some(Vec3::new(9.0, 10.0, 10.0)));
        assert_eq!(at([1.1, 1.0]), None);
        assert_eq!(
            continued_control(Vec3::Z, Vec3::X, Vec3::X + Vec3::Y),
            Vec3::Y + Vec3::Z
        );
    }

    #[test]
    fn ordered_controls_follow_positive_scale_and_translation_but_refuse_direction_reversal() {
        let saved = [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::ZERO];
        let current = saved.map(|p| p * 2.0 + Vec3::new(12.0, -10.0, 1.0));
        assert!(controls_follow(saved.into_iter(), current.into_iter()));
        assert!(!controls_follow(
            saved.into_iter(),
            current.into_iter().rev()
        ));
    }

    #[test]
    fn reversed_rims_have_one_phase_and_coincident_edges_are_ambiguous() {
        let a = [
            [Vec3::ZERO, Vec3::X],
            [Vec3::X, Vec3::Y],
            [Vec3::Y, Vec3::ZERO],
        ];
        let b = [
            [Vec3::Y, Vec3::X],
            [Vec3::X, Vec3::ZERO],
            [Vec3::ZERO, Vec3::Y],
        ];
        assert!(matches!(
            reversed_rim_phase(&a, &b, 1e-7),
            RimPhase::Unique(1)
        ));
        assert!(matches!(
            reversed_rim_phase(&a, &a, 1e-7),
            RimPhase::Missing
        ));
        let repeated = [[Vec3::ZERO, Vec3::ZERO]; 3];
        assert!(matches!(
            reversed_rim_phase(&repeated, &repeated, 1e-7),
            RimPhase::Ambiguous
        ));
    }
}
