//! Geometric primitives used to lay out finger-joint cutters.
//!
//! The modelling workflow and its body selection live in `solidcore-ops`; these routines own
//! the vector arithmetic and the numeric layout that define the joint geometry.

use crate::{Point3, SolidError, Transform, Vec3};

const OP: &str = "finger_joint";
const ANGULAR_TOLERANCE: f64 = 1e-6;
const MINIMUM_SEGMENT: f64 = 1e-6;

/// One interval in a finger-joint pattern, with the plate that retains its material.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval {
    /// Start coordinate along the joint line.
    pub start: f64,
    /// End coordinate along the joint line.
    pub end: f64,
    /// Whether the first plate retains material in this interval.
    pub retained_by_first: bool,
}

/// The signed dot product of two plate normals.
#[must_use]
pub fn normal_dot(first: Vec3, second: Vec3) -> f64 {
    first.dot(second)
}

/// The positive side lengths of a finite, ordered axis-aligned box.
#[must_use]
pub fn bounds_size(lower: [f64; 3], upper: [f64; 3]) -> Option<[f64; 3]> {
    if !lower.iter().chain(&upper).all(|value| value.is_finite())
        || lower.iter().zip(upper).any(|(lo, hi)| *lo > hi)
    {
        return None;
    }
    let size = [
        upper[0] - lower[0],
        upper[1] - lower[1],
        upper[2] - lower[2],
    ];
    size.iter()
        .all(|value| value.is_finite() && *value > 0.0)
        .then_some(size)
}

/// The dominant outward normal of a plate, inferred from its planar face normals and areas.
///
/// Opposite parallel faces contribute to one pair. A second, equally dominant direction makes
/// the plate ambiguous and returns `None`.
#[must_use]
pub fn plate_axis(faces: &[(Vec3, f64)]) -> Option<Vec3> {
    let planar: Vec<(Vec3, f64)> = faces
        .iter()
        .filter_map(|(normal, area)| {
            let unit = normal.unitize().ok()?;
            area.is_finite().then_some((unit, *area))
        })
        .collect();
    let score = |normal: Vec3| {
        let (mut same, mut opposite) = (0.0, 0.0);
        for (other, area) in &planar {
            let cosine = normal.dot(*other);
            if cosine > 1.0 - ANGULAR_TOLERANCE {
                same += area;
            } else if cosine < -1.0 + ANGULAR_TOLERANCE {
                opposite += area;
            }
        }
        same.min(opposite)
    };
    let (best, best_score) = planar
        .iter()
        .map(|(normal, _)| (*normal, score(*normal)))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    if best_score <= 0.0 {
        return None;
    }
    let rival = planar.iter().any(|(normal, _)| {
        normal.dot(best).abs() < 1.0 - ANGULAR_TOLERANCE
            && score(*normal) >= best_score * (1.0 - ANGULAR_TOLERANCE)
    });
    (!rival).then_some(best)
}

/// The unit line direction at the intersection of two plate planes, or `None` if parallel.
#[must_use]
pub fn joint_line(first_normal: Vec3, second_normal: Vec3) -> Option<Vec3> {
    let cross = first_normal.cross(second_normal);
    if cross.length() <= 1e-4 {
        return None;
    }
    cross.unitize().ok()
}

/// A right-handed joint frame with x along the joint line and y along the plate normal,
/// orthogonalized to remove floating-point skew between the directions.
///
/// Returns `None` when the axes are parallel or either input cannot form a unit vector.
#[must_use]
pub fn frame(line: Vec3, normal: Vec3, origin: Point3) -> Option<Transform> {
    line.cross(normal).unitize().ok()?;
    let [x, y] = Transform::orthonormal_frame(line, normal).ok()?;
    let z = x.cross(y).unitize().ok()?;
    Some(Transform::from_axes(x, y, z, origin))
}

/// The midpoint of a finite, ordered axis-aligned box.
#[must_use]
pub fn bounds_center(lower: [f64; 3], upper: [f64; 3]) -> Option<Point3> {
    let [lx, ly, lz] = lower;
    let [ux, uy, uz] = upper;
    if !lower.iter().chain(&upper).all(|value| value.is_finite())
        || lower.iter().zip(upper).any(|(lo, hi)| *lo > hi)
    {
        return None;
    }
    Some(Point3::new(
        lx + (ux - lx) * 0.5,
        ly + (uy - ly) * 0.5,
        lz + (uz - lz) * 0.5,
    ))
}

/// The lower corner and positive size of one clearance-expanded through-cut box in a joint
/// frame. The box spans the plate thickness on y and the overlap depth on z.
///
/// # Errors
/// Returns `InvalidInput` if the dimensions are unordered, non-finite, or too small to make a
/// solid box, or if clearance is invalid.
pub fn cutter_box(
    interval: Interval,
    plate_lower: [f64; 3],
    plate_upper: [f64; 3],
    overlap_lower: [f64; 3],
    overlap_upper: [f64; 3],
    clearance: f64,
) -> crate::Result<([f64; 3], [f64; 3])> {
    let [_, plate_y0, _] = plate_lower;
    let [_, plate_y1, _] = plate_upper;
    let [_, _, overlap_z0] = overlap_lower;
    let [_, _, overlap_z1] = overlap_upper;
    if !clearance.is_finite() || clearance < 0.0 {
        return Err(SolidError::invalid_input(
            OP,
            "clearance must be a finite non-negative length",
        ));
    }
    let half_clearance = clearance * 0.5;
    let lower = [
        interval.start - half_clearance,
        plate_y0 - MINIMUM_SEGMENT,
        overlap_z0 - half_clearance,
    ];
    let upper = [
        interval.end + half_clearance,
        plate_y1 + MINIMUM_SEGMENT,
        overlap_z1 + half_clearance,
    ];
    let [x0, y0, z0] = lower;
    let [x1, y1, z1] = upper;
    let size = [x1 - x0, y1 - y0, z1 - z0];
    if !lower.iter().chain(&size).all(|value| value.is_finite())
        || size.iter().any(|value| *value <= 0.0)
    {
        return Err(SolidError::invalid_input(
            OP,
            "the cutter dimensions are not positive and finite",
        ));
    }
    Ok((lower, size))
}

/// The alternating slot/tab intervals over `span`, before edge offsets and clearance.
///
/// # Errors
/// Returns `InvalidInput` if the count overflows or any segment is non-finite or too short.
pub fn pattern(span: [f64; 2], count: usize) -> crate::Result<Vec<Interval>> {
    let [start, end] = span;
    let segments = count
        .checked_mul(2)
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| SolidError::invalid_input(OP, "finger count is too large"))?;
    let width = (end - start) / segments as f64;
    if !width.is_finite() || width <= MINIMUM_SEGMENT {
        return Err(SolidError::invalid_input(
            OP,
            "the overlap is too short for the requested finger count",
        ));
    }
    Ok((0..segments)
        .map(|index| Interval {
            start: start + width * index as f64,
            end: start + width * (index + 1) as f64,
            retained_by_first: index % 2 == 0,
        })
        .collect())
}

/// Applies end offsets to the pattern and checks that clearance fits each segment.
///
/// # Errors
/// Returns `InvalidInput` for an invalid offset or clearance, or if either leaves no usable
/// segment width.
pub fn allowances(
    pattern: &mut [Interval],
    span: [f64; 2],
    edge_offset: f64,
    clearance: f64,
) -> crate::Result<()> {
    let [start, end] = span;
    if !edge_offset.is_finite() || edge_offset < 0.0 || !clearance.is_finite() || clearance < 0.0 {
        return Err(SolidError::invalid_input(
            OP,
            "edge offset and clearance must be finite non-negative lengths",
        ));
    }
    if pattern.is_empty() {
        return Err(SolidError::invalid_input(OP, "the pattern has no segments"));
    }
    let width = (end - start - edge_offset * 2.0) / pattern.len() as f64;
    if !width.is_finite() || width <= MINIMUM_SEGMENT {
        return Err(SolidError::invalid_input(
            OP,
            "edge offset leaves too little overlap for the requested finger count",
        ));
    }
    if clearance >= width {
        return Err(SolidError::invalid_input(
            OP,
            "clearance must be less than a finger width",
        ));
    }
    let slots = pattern.len();
    for (index, interval) in pattern.iter_mut().enumerate() {
        interval.start = start + width * index as f64 + if index > 0 { edge_offset } else { 0.0 };
        interval.end = start
            + width * (index + 1) as f64
            + edge_offset
            + if index + 1 == slots { edge_offset } else { 0.0 };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{bounds_size, normal_dot};
    use crate::Vec3;

    #[test]
    fn normal_dot_measures_alignment() {
        assert_eq!(
            normal_dot(Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
            0.0
        );
        assert_eq!(
            normal_dot(Vec3::new(1.0, 0.0, 0.0), Vec3::new(-1.0, 0.0, 0.0)),
            -1.0
        );
    }

    #[test]
    fn bounds_size_rejects_invalid_extents() {
        assert_eq!(
            bounds_size([1.0, 2.0, 3.0], [4.0, 7.0, 9.0]),
            Some([3.0, 5.0, 6.0])
        );
        assert_eq!(bounds_size([1.0, 2.0, 3.0], [0.0, 7.0, 9.0]), None);
        assert_eq!(bounds_size([1.0, 2.0, 3.0], [1.0, 7.0, 9.0]), None);
        assert_eq!(
            bounds_size([0.0, 0.0, 0.0], [f64::INFINITY, 1.0, 1.0]),
            None
        );
    }
}
