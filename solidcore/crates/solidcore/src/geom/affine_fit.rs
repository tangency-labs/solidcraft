//! Corresponding point sets and their rigid or affine transforms.
//! Every input pair must meet the residual tolerance.

use crate::{Transform, Vec3};

/// A whole-set fit keeps the same distance between points or carries an affine stretch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fit {
    /// All pair distances are preserved.
    Rigid(Transform),
    /// A positive-determinant map includes a non-rigid stretch.
    Affine(Transform),
}

const DIRECTION_EPS: f64 = 1e-9;
const RESIDUAL: f64 = 1e-8;

fn finite(v: Vec3) -> bool {
    v.to_array().iter().all(|x| x.is_finite())
}

fn unit(v: Vec3) -> Option<Vec3> {
    let len = v.length();
    (len > DIRECTION_EPS && len.is_finite()).then(|| v / len)
}

/// Check the axes of a proper orthonormal frame at a caller's tolerance.
pub fn proper_axes([x, y, z]: [Vec3; 3], tolerance: f64) -> bool {
    tolerance.is_finite()
        && (0.0..1.0).contains(&tolerance)
        && [x, y, z]
            .iter()
            .all(|&v| finite(v) && (v.length_squared() - 1.0).abs() <= tolerance)
        && x.dot(y).abs() <= tolerance
        && y.dot(z).abs() <= tolerance
        && z.dot(x).abs() <= tolerance
        && x.cross(y).dot(z) >= 1.0 - tolerance
}

/// Resolve a proper frame from a first axis and a second direction in its xy-plane.
pub fn frame(x: Vec3, xy: Vec3) -> Option<[Vec3; 3]> {
    let x = unit(x)?;
    let z = unit(x.cross(xy))?;
    Some([x, z.cross(x), z])
}

/// Construct an affine transform from the images of its axes and origin.
pub fn from_frame(origin: Vec3, [x, y, z]: [Vec3; 3]) -> Transform {
    Transform::from_axes(x, y, z, origin)
}

/// Orthonormalise the images of the frame's first two directions.
pub fn mapped_axes(map: &Transform, [x, y, _]: [Vec3; 3]) -> Option<[Vec3; 3]> {
    frame(map.transform_vector(x), map.transform_vector(y))
}

/// Resolve local components in an orthonormal frame, with translation handled separately.
pub fn frame_vector([x, y, z]: [Vec3; 3], v: Vec3) -> Vec3 {
    Vec3::new(v.dot(x), v.dot(y), v.dot(z))
}

/// Resolve a point relative to an orthonormal frame and its origin.
pub fn frame_point(origin: Vec3, axes: [Vec3; 3], point: Vec3) -> Vec3 {
    frame_vector(axes, point - origin)
}

/// Turn about the supplied line, then move by `translation`.
/// If the turn is within `angle_tolerance` of zero, use the positive z axis.
pub fn rigid_motion(
    translation: Vec3,
    origin: Vec3,
    axis: Vec3,
    angle: f64,
    angle_tolerance: f64,
) -> Option<Transform> {
    if !finite(translation)
        || !finite(origin)
        || !angle.is_finite()
        || !angle_tolerance.is_finite()
        || angle_tolerance < 0.0
    {
        return None;
    }
    let axis = if angle.abs() > angle_tolerance {
        unit(axis)?
    } else {
        Vec3::Z
    };
    let turn = Transform::rotation(origin, axis, angle).ok()?;
    let map = turn.concat(&Transform::translation(translation));
    map.m.iter().flatten().all(|v| v.is_finite()).then_some(map)
}

/// Prefer a rigid map for a translation or a rotation, otherwise try a proper affine map.
/// Coplanar data sets have no information about the normal stretch. Extend the map along
/// the transformed normal using the geometric mean of the two in-plane stretches.
pub fn between(pairs: impl Iterator<Item = (Vec3, Vec3)> + Clone) -> Option<Fit> {
    if pairs.clone().any(|(p, q)| !finite(p) || !finite(q)) {
        return None;
    }
    let (a, b) = pairs.clone().next()?;
    let shift = b - a;
    if pairs.clone().all(|(p, q)| (q - p).distance(shift) < 1e-9) {
        return Some(Fit::Rigid(Transform::translation(shift)));
    }

    let (second_from, second_to) = farthest(pairs.clone(), |p| p.distance(a))?;
    let e1 = second_from - a;
    let d1 = second_to - b;
    // Test the same stable farthest-plane construction for both fits. The rigid
    // case uses the normalised first axis before measuring this distance.
    if let Some(map) = rigid(pairs.clone(), a, b, e1, d1) {
        return Some(Fit::Rigid(map));
    }
    let (third_from, third_to) = farthest(pairs.clone(), |p| (p - a).cross(e1).length_squared())?;
    let e2 = third_from - a;
    let d2 = third_to - b;
    let normal = unit(e1.cross(e2))?;
    let (fourth_from, fourth_to) = farthest(pairs.clone(), |p| (p - a).dot(normal).abs())?;
    let fourth_from = fourth_from - a;
    let (e3, d3) = if fourth_from.dot(normal).abs() > RESIDUAL {
        (fourth_from, fourth_to - b)
    } else {
        let cross = d1.cross(d2);
        (
            normal,
            unit(cross)? * (cross.length() / e1.cross(e2).length()).sqrt(),
        )
    };
    let determinant = e1.dot(e2.cross(e3));
    if !determinant.is_finite()
        || determinant.abs() < 1e-12 * e1.length() * e2.length() * e3.length()
    {
        return None;
    }
    let [g1, g2, g3] = [e2.cross(e3), e3.cross(e1), e1.cross(e2)].map(|v| v / determinant);
    let vector = |v: Vec3| d1 * v.dot(g1) + d2 * v.dot(g2) + d3 * v.dot(g3);
    let [x, y, z] = [Vec3::X, Vec3::Y, Vec3::Z].map(vector);
    let map = Transform::from_axes(x, y, z, b - vector(a));
    let determinant = super::expansion::det3(x, y, z)?;
    let minimum = super::expansion::sub(&determinant, &[1e-12]);
    if super::expansion::sign(&minimum) <= 0
        || map.is_regular(1e-12) != super::certified::Verdict::Yes
    {
        return None;
    }
    fit_pairs(pairs, map).map(Fit::Affine)
}

fn farthest(
    pairs: impl Iterator<Item = (Vec3, Vec3)>,
    distance: impl Fn(Vec3) -> f64,
) -> Option<(Vec3, Vec3)> {
    pairs.max_by(|(p, _), (q, _)| distance(*p).total_cmp(&distance(*q)))
}

fn rigid(
    pairs: impl Iterator<Item = (Vec3, Vec3)> + Clone,
    a: Vec3,
    b: Vec3,
    e1: Vec3,
    d1: Vec3,
) -> Option<Transform> {
    let x = unit(e1)?;
    let (third_from, third_to) = farthest(pairs.clone(), |p| (p - a).cross(x).length_squared())?;
    let e2 = third_from - a;
    let d2 = third_to - b;
    let z = unit(x.cross(e2))?;
    let u = unit(d1)?;
    let w = unit(u.cross(d2))?;
    let from_axes = [x, z.cross(x), z];
    let v = w.cross(u);
    let rotate = |p| {
        let q = frame_vector(from_axes, p);
        u * q.x + v * q.y + w * q.z
    };
    let [x, y, z] = [Vec3::X, Vec3::Y, Vec3::Z].map(rotate);
    fit_pairs(pairs, Transform::from_axes(x, y, z, b - rotate(a)))
}

/// Map a point with the translation added after the linear part.
pub fn point(map: &Transform, point: Vec3) -> Vec3 {
    map.transform_vector(point) + map.translation_part()
}

/// Map a displacement without translation.
pub fn vector(map: &Transform, vector: Vec3) -> Vec3 {
    map.transform_vector(vector)
}

fn fit_pairs(mut pairs: impl Iterator<Item = (Vec3, Vec3)>, map: Transform) -> Option<Transform> {
    pairs
        .all(|(p, q)| point(&map, p).distance(q) < RESIDUAL)
        .then_some(map)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn fit(from: &[Vec3], to: &[Vec3]) -> Option<Fit> {
        between(from.iter().copied().zip(to.iter().copied()))
    }

    #[test]
    fn a_rounding_positive_determinant_does_not_make_a_collapsing_map_regular() {
        let source = [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z];
        let x = Vec3::new(100000003.0, 100000004.0, 100000006.0) / 4096.0;
        let y = Vec3::new(100000000.0, 100000001.0, 100000002.0) / 4096.0;
        assert!(fit(&source, &[Vec3::ZERO, x, y, x + y]).is_none());
    }

    #[test]
    fn rigid_and_affine_fits_keep_the_supplied_correspondence() {
        let from = [
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            Vec3::Z,
            Vec3::new(2.0, 3.0, 4.0),
        ];
        let turn = Transform::rotation(Vec3::new(4.0, -3.0, 1.0), Vec3::X + Vec3::Z, 0.7).unwrap();
        let to = from.map(|p| turn.transform_point(p));
        let Some(Fit::Rigid(rigid)) = fit(&from, &to) else {
            panic!("expected a rigid fit")
        };
        assert!(from
            .iter()
            .zip(to)
            .all(|(&p, q)| rigid.transform_point(p).distance(q) < 1e-12));

        let stretch = Transform::from_axes(
            Vec3::new(2.0, 0.1, 0.0),
            Vec3::Y * 3.0,
            Vec3::Z * 4.0,
            Vec3::X,
        );
        let mut to = from.map(|p| stretch.transform_point(p));
        let Some(Fit::Affine(affine)) = fit(&from, &to) else {
            panic!("expected an affine fit")
        };
        assert!(from
            .iter()
            .zip(to)
            .all(|(&p, q)| affine.transform_point(p).distance(q) < 1e-12));
        to[4].x += 0.01;
        assert!(fit(&from, &to).is_none());
        assert!(fit(&from, &from.map(|p| Vec3::new(-p.x, p.y, p.z))).is_none());
    }

    #[test]
    fn a_planar_fit_extends_to_a_proper_normal_and_rejects_a_collapsed_plane() {
        let from = [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::X + Vec3::Y];
        let to = from.map(|p| Vec3::new(p.x * 2.0, 3.0, -p.y * 8.0));
        let Some(Fit::Affine(map)) = fit(&from, &to) else {
            panic!("expected a planar affine fit")
        };
        assert!(map.transform_vector(Vec3::Z).distance(Vec3::Y * 4.0) < 1e-12);
        assert!(from
            .iter()
            .zip(to)
            .all(|(&p, q)| map.transform_point(p).distance(q) < 1e-12));
        assert!(fit(&from, &from.map(|p| Vec3::new(p.x, 0.0, 0.0))).is_none());
        assert!(fit(&from, &[Vec3::new(f64::NAN, 0.0, 0.0); 4]).is_none());
    }
}
