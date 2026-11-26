//! Weighted point transforms and displacement in constrained axes.

use super::vec::VecExt;
use crate::math::{Transform, Vec3};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

/// Translate a point by a weighted displacement.
pub fn translate(point: Vec3, delta: Vec3, weight: f64) -> Vec3 {
    point + delta * weight
}

/// Rotate a point around a pivot by the weighted angle.
pub fn rotate(point: Vec3, pivot: Vec3, axis: Vec3, angle: f64, weight: f64) -> Option<Vec3> {
    let rotation = Transform::rotation(Vec3::ZERO, axis, angle * weight).ok()?;
    Some(pivot + rotation.transform_vector(point - pivot))
}

/// Scale a point in an orthonormal frame, interpolating each factor from one.
pub fn scale(point: Vec3, pivot: Vec3, basis: [Vec3; 3], factors: [f64; 3], weight: f64) -> Vec3 {
    let local = point - pivot;
    pivot
        + basis
            .into_iter()
            .zip(factors)
            .fold(Vec3::ZERO, |q, (axis, factor)| {
                let factor = if weight == 1.0 {
                    factor
                } else {
                    1.0 + (factor - 1.0) * weight
                };
                q + axis * (local.dot(axis) * factor)
            })
}

/// Apply scale, then successive turns around the fixed frame axes, then translation.
pub fn numeric(
    point: Vec3,
    pivot: Vec3,
    basis: [Vec3; 3],
    lengths: [f64; 3],
    angles: [f64; 3],
    factors: [f64; 3],
    weight: f64,
) -> Option<Vec3> {
    let scaled = scale(point, pivot, basis, factors, weight);
    let turned = basis
        .into_iter()
        .zip(angles)
        .try_fold(scaled, |p, (axis, angle)| {
            rotate(p, pivot, axis, angle, weight)
        })?;
    let delta = basis
        .into_iter()
        .zip(lengths)
        .fold(Vec3::ZERO, |v, (axis, length)| v + axis * length);
    let moved = translate(turned, delta, weight);
    moved.is_finite().then_some(moved)
}

/// Scale factors from a displacement along the three oriented axes.
pub fn scale_factors(delta: Vec3, length: f64, basis: [Vec3; 3]) -> [f64; 3] {
    basis.map(|axis| factor(delta, length, axis))
}

/// Uniform scale from the displacement along the sum of screen right and up.
pub fn uniform_factors(delta: Vec3, length: f64, right: Vec3, up: Vec3) -> [f64; 3] {
    [factor(delta, length, right + up); 3]
}

/// Length and components of a translation in the supplied frame.
pub fn translation_measure(delta: Vec3, basis: [Vec3; 3]) -> (f64, [f64; 3]) {
    (delta.len(), basis.map(|axis| delta.dot(axis)))
}

fn factor(delta: Vec3, length: f64, direction: Vec3) -> f64 {
    (1.0 + delta.dot(direction) / length).max(0.001)
}

/// A displacement constraint, with unit axes supplied by its constructors.
#[derive(Clone, Copy)]
pub struct Frame(Axes);

#[derive(Clone, Copy)]
enum Axes {
    Axis(Vec3),
    Plane([Vec3; 2]),
}

impl Frame {
    /// A line constraint along a nonzero direction.
    pub fn axis(direction: Vec3) -> Option<Self> {
        Some(Self(Axes::Axis(direction.normalized()?)))
    }

    /// A planar constraint with two orthogonal axes.
    pub fn plane([x, y]: [Vec3; 2]) -> Option<Self> {
        let (x, y) = (x.normalized()?, y.normalized()?);
        (x.dot(y).abs() <= 1e-8).then_some(Self(Axes::Plane([x, y])))
    }

    /// Quantise displacement along each axis, retaining the constraint.
    pub fn quantise(&self, delta: Vec3, step: f64) -> Option<Vec3> {
        if !step.is_finite() || step <= 0.0 {
            return None;
        }
        let snap = |axis: Vec3| {
            let count = delta.dot(axis) / step;
            // Beyond 2^52 adjacent integer increments cannot be represented reliably.
            (count.abs() <= 4_503_599_627_370_496.0).then(|| axis * (count.round() * step))
        };
        let out = match self.0 {
            Axes::Axis(axis) => snap(axis)?,
            Axes::Plane([x, y]) => snap(x)? + snap(y)?,
        };
        out.is_finite().then_some(out)
    }
}

/// Orthogonal axes of the plane normal to a view ray, aligned with screen right.
pub fn view_plane_basis(direction: Vec3, right: Vec3) -> Option<[Vec3; 2]> {
    let normal = direction.normalized()?;
    let x = (right - normal * right.dot(normal)).normalized()?;
    Some([x, x.cross(normal)])
}

/// A proportional selection query that could not be evaluated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeightError {
    /// The radius is not usable, or the falloff choice does not exist.
    InvalidRadius,
    /// The topology or its coordinates are invalid.
    InvalidSource,
    /// The source exceeds the point or face-vertex budget.
    Limit,
}

/// Maximum number of points in a proportional selection query.
pub const MAX_WEIGHT_POINTS: usize = 65_536;
const MAX_FACE_USES: usize = 262_144;

/// Validate the query dimensions before copying the input points.
pub fn check_weight_limits(points: usize, faces: &[Vec<usize>]) -> Result<(), WeightError> {
    if points > MAX_WEIGHT_POINTS
        || faces.len() > MAX_FACE_USES
        || faces
            .iter()
            .try_fold(0_usize, |n, face| n.checked_add(face.len()))
            .is_none_or(|n| n > MAX_FACE_USES)
    {
        return Err(WeightError::Limit);
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
struct Queue {
    distance: f64,
    id: usize,
}
impl Eq for Queue {}
impl PartialOrd for Queue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Queue {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .distance
            .total_cmp(&self.distance)
            .then_with(|| other.id.cmp(&self.id))
    }
}
fn falloff(distance: f64, kind: usize) -> f64 {
    let t = distance.clamp(0.0, 1.0);
    match kind {
        1 => 1.0 - t,
        2 => (1.0 - t).powi(2),
        _ => 1.0 - t * t * (3.0 - 2.0 * t),
    }
}
/// Distances follow logical edges. Close but disconnected islands never influence each other.
pub fn weights(
    points: &[Vec3],
    faces: &[Vec<usize>],
    seeds: &BTreeSet<usize>,
    editable: &BTreeSet<usize>,
    locked: &BTreeSet<usize>,
    radius: f64,
    kind: usize,
) -> Result<BTreeMap<usize, f64>, WeightError> {
    if !radius.is_finite() || !(0.000001..=100_000.0).contains(&radius) || kind > 2 {
        return Err(WeightError::InvalidRadius);
    }
    check_weight_limits(points.len(), faces)?;
    if points.iter().any(|p| !p.is_finite())
        || seeds
            .iter()
            .chain(editable)
            .chain(locked)
            .any(|&i| i >= points.len())
    {
        return Err(WeightError::InvalidSource);
    }
    let mut graph = vec![Vec::new(); points.len()];
    for face in faces {
        for (&a, &b) in face
            .iter()
            .zip(face.iter().cycle().skip(1))
            .take(face.len())
        {
            let (Some(pa), Some(pb)) = (points.get(a), points.get(b)) else {
                return Err(WeightError::InvalidSource);
            };
            let length = pa.dist(*pb);
            if !length.is_finite() {
                return Err(WeightError::InvalidSource);
            }
            graph
                .get_mut(a)
                .ok_or(WeightError::InvalidSource)?
                .push((b, length));
            graph
                .get_mut(b)
                .ok_or(WeightError::InvalidSource)?
                .push((a, length));
        }
    }
    let mut distances = vec![f64::INFINITY; points.len()];
    let mut queue = BinaryHeap::new();
    for &id in seeds
        .intersection(editable)
        .filter(|id| !locked.contains(*id))
    {
        *distances.get_mut(id).ok_or(WeightError::InvalidSource)? = 0.0;
        queue.push(Queue { distance: 0.0, id });
    }
    while let Some(Queue { distance, id }) = queue.pop() {
        if distance > *distances.get(id).ok_or(WeightError::InvalidSource)? || distance >= radius {
            continue;
        }
        for &(next, length) in graph.get(id).ok_or(WeightError::InvalidSource)? {
            let candidate = distance + length;
            let old = distances.get_mut(next).ok_or(WeightError::InvalidSource)?;
            if candidate < radius && candidate < *old {
                *old = candidate;
                queue.push(Queue {
                    distance: candidate,
                    id: next,
                });
            }
        }
    }
    Ok(editable
        .iter()
        .filter(|id| !locked.contains(*id))
        .filter_map(|&id| {
            let d = *distances.get(id)?;
            (d < radius).then(|| (id, falloff(d / radius, kind)))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weighted_transforms_preserve_the_requested_order_and_frame() {
        let basis = [Vec3::X, Vec3::Y, Vec3::Z];
        let pivot = Vec3::new(4.0, 5.0, 6.0);
        let point = pivot + Vec3::X;
        assert_eq!(translate(point, Vec3::Y * 4.0, 0.25), point + Vec3::Y);
        assert_eq!(
            scale(point, pivot, basis, [3.0; 3], 0.5),
            pivot + Vec3::X * 2.0
        );
        let angle = std::f64::consts::FRAC_PI_2;
        let diagonal = 0.5_f64.sqrt();
        let turned = rotate(point, pivot, Vec3::Z, angle, 0.5).unwrap();
        assert!(turned.dist(pivot + Vec3::new(diagonal, diagonal, 0.0)) < 1e-12);
        assert_eq!(rotate(point, pivot, Vec3::Z, angle, 0.0), Some(point));
        let transformed = numeric(
            point,
            pivot,
            basis,
            [0.0, 0.0, 4.0],
            [0.0, 0.0, angle],
            [3.0; 3],
            0.5,
        )
        .unwrap();
        assert!(transformed.dist(pivot + Vec3::new(2.0 * diagonal, 2.0 * diagonal, 2.0)) < 1e-12);
    }

    #[test]
    fn a_snap_preserves_its_oriented_plane_and_refuses_unrepresentable_ticks() {
        let normal = Vec3::new(0.6, 0.8, 0.0);
        let axis = Vec3::new(0.8, -0.6, 0.0);
        let frame = Frame::plane([axis, Vec3::Z]).unwrap();
        let snapped = frame.quantise(axis * 1.4 + Vec3::Z * 0.6, 1.0).unwrap();
        assert!(snapped.dot(normal).abs() < 1e-12);
        assert!(snapped.dist(axis + Vec3::Z) < 1e-12);
        assert!(frame.quantise(Vec3::Z * 1e20, 1.0).is_none());
        assert!(frame.quantise(Vec3::Z, f64::NAN).is_none());
        let axes = view_plane_basis(normal, Vec3::X).unwrap();
        assert!(axes.iter().all(|a| a.dot(normal).abs() < 1e-12));
        assert!(axes[0].dot(axes[1]).abs() < 1e-12);
    }

    #[test]
    fn soft_weights_follow_edges_and_leave_disconnected_points_unchanged() {
        let points = [Vec3::ZERO, Vec3::X, Vec3::X + Vec3::Y, Vec3::Y, Vec3::ZERO];
        let faces = vec![vec![0, 1, 2, 3]];
        let selected = BTreeSet::from([0]);
        let editable = (0..5).collect();
        let got = weights(
            &points,
            &faces,
            &selected,
            &editable,
            &BTreeSet::from([3]),
            3.0,
            0,
        )
        .unwrap();
        assert_eq!(got[&0], 1.0);
        assert!(got[&1] > got[&2] && got[&2] > 0.0);
        assert!(!got.contains_key(&3) && !got.contains_key(&4));
        assert_eq!(
            weights(
                &points,
                &faces,
                &selected,
                &editable,
                &BTreeSet::new(),
                f64::NAN,
                0
            ),
            Err(WeightError::InvalidRadius)
        );
    }
}
