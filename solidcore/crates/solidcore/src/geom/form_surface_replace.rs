//! Positive-weight support qualification for replacing a planar face by a skin.
//! Conservative convex-hull bounds; unsupported surfaces remain unqualified.

use crate::eval::plan::EvalSurface;
use crate::Vec3;

/// An oriented plane, with its supplied unit normal.
#[derive(Clone, Copy)]
pub struct HalfSpace {
    /// A point of the plane.
    pub origin: Vec3,
    /// The outward unit normal.
    pub normal: Vec3,
}

impl HalfSpace {
    /// Plane of the supplied oriented axes.
    pub fn from_axes(origin: Vec3, x: Vec3, y: Vec3) -> Self {
        Self {
            origin,
            normal: x.cross(y),
        }
    }
    /// Signed support height.
    pub fn height(self, point: Vec3) -> f64 {
        (point - self.origin).dot(self.normal)
    }
}

/// Finite Cartesian controls of a bounded, positive-weight rational hull.
pub fn points(homogeneous: &[[f64; 4]]) -> Option<Vec<Vec3>> {
    if homogeneous.is_empty() || homogeneous.len() > 262_144 {
        return None;
    }
    homogeneous
        .iter()
        .map(|&[x, y, z, w]| {
            if ![x, y, z, w].into_iter().all(f64::is_finite) || w <= 1e-12 {
                return None;
            }
            let point = Vec3::new(x / w, y / w, z / w);
            point
                .to_array()
                .into_iter()
                .all(f64::is_finite)
                .then_some(point)
        })
        .collect()
}

/// A surface's complete positive support hull, or its planar boundary hull.
pub fn hull(surface: &EvalSurface, planar_boundary: &[[f64; 4]]) -> Option<Vec<Vec3>> {
    match surface {
        EvalSurface::Nurbs { ctrl, closed, .. } if !closed.iter().any(|closed| *closed) => {
            points(ctrl)
        }
        EvalSurface::Plane { .. } => points(planar_boundary),
        _ => None,
    }
}

/// Maximum signed height of an untrimmed support; an upper bound for its face.
pub fn maximum_height(
    surface: &EvalSurface,
    planar_boundary: &[[f64; 4]],
    plane: HalfSpace,
) -> Option<f64> {
    let values: Vec<f64> = match surface {
        EvalSurface::Plane { .. } => points(planar_boundary)?
            .into_iter()
            .map(|p| plane.height(p))
            .collect(),
        EvalSurface::Nurbs { ctrl, .. } => {
            points(ctrl)?.into_iter().map(|p| plane.height(p)).collect()
        }
        EvalSurface::Revolved {
            profile,
            origin,
            axis,
            transform,
            ..
        } => {
            let n = plane.normal;
            let [x, y, z] = *transform;
            let pullback = Vec3::new(
                n.dot(Vec3::new(x[0], y[0], z[0])),
                n.dot(Vec3::new(x[1], y[1], z[1])),
                n.dot(Vec3::new(x[2], y[2], z[2])),
            );
            let parallel = pullback.dot(*axis);
            let radial = (pullback - *axis * parallel).length();
            let offset = (Vec3::new(x[3], y[3], z[3]) - plane.origin).dot(n) + origin.dot(pullback);
            points(&profile.ctrl)?
                .into_iter()
                .map(|p| {
                    let d = p - *origin;
                    let axial = d.dot(*axis);
                    offset + axial * parallel + (d - *axis * axial).length() * radial
                })
                .collect()
        }
    };
    if values.iter().any(|height| !height.is_finite()) {
        None
    } else {
        values.into_iter().reduce(f64::max)
    }
}

/// Whether a bounded hull stays in every supplied half-space.
pub fn inside(hull: &[Vec3], planes: &[HalfSpace], tolerance: f64) -> bool {
    tolerance.is_finite()
        && !hull.is_empty()
        && hull.iter().all(|&point| {
            planes.iter().all(|plane| {
                let height = plane.height(point);
                height.is_finite() && height <= tolerance
            })
        })
}

/// Every support lies on one specified plane within the supplied tolerance.
pub fn coplanar(hull: &[Vec3], plane: HalfSpace, tolerance: f64) -> bool {
    tolerance.is_finite()
        && !hull.is_empty()
        && hull
            .iter()
            .all(|&point| plane.height(point).abs() <= tolerance)
}

/// Every nonempty tensor span has a strictly interior supporting coefficient.
/// Positive basis weights then exclude a patch lying entirely in a contact plane.
pub fn strictly_inside(
    surface: &EvalSurface,
    hull: &[Vec3],
    planes: &[HalfSpace],
    epsilon: f64,
) -> bool {
    if !epsilon.is_finite() || epsilon < 0.0 {
        return false;
    }
    let EvalSurface::Nurbs {
        degree,
        knots,
        count,
        ..
    } = surface
    else {
        return planes
            .iter()
            .all(|plane| hull.iter().any(|&p| plane.height(p) < -epsilon));
    };
    let [pu, pv] = *degree;
    let [nu, nv] = *count;
    let [ku, kv] = knots;
    if nu.checked_mul(nv) != Some(hull.len()) || pu >= nu || pv >= nv {
        return false;
    }
    let spans = |degree, count, knots: &[f64]| {
        (degree..count)
            .filter(|&i| {
                knots
                    .get(i)
                    .zip(knots.get(i + 1))
                    .is_some_and(|(a, b)| b > a)
            })
            .collect::<Vec<_>>()
    };
    let u = spans(pu, nu, ku);
    let v = spans(pv, nv, kv);
    let work = u
        .len()
        .saturating_mul(v.len())
        .saturating_mul(planes.len())
        .saturating_mul(pu + 1)
        .saturating_mul(pv + 1);
    if work > 5_000_000 || u.is_empty() || v.is_empty() {
        return false;
    }
    v.iter().all(|&v| {
        u.iter().all(|&u| {
            planes.iter().all(|plane| {
                (v - pv..=v).any(|j| {
                    (u - pu..=u).any(|i| {
                        hull.get(j * nu + i)
                            .is_some_and(|&p| plane.height(p) < -epsilon)
                    })
                })
            })
        })
    })
}

/// Scale-aware separation epsilon used by the qualified replacement path.
pub fn epsilon(size: f64, relative: f64) -> f64 {
    (size * relative).max(relative)
}

/// Reverse an oriented support axis.
pub fn reverse_axis(axis: Vec3) -> Vec3 {
    -axis
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn support_hulls_preserve_half_space_and_contact_qualification() {
        let plane = HalfSpace::from_axes(Vec3::ZERO, Vec3::X, Vec3::Y);
        let controls = [
            [0., 0., -1., 1.],
            [1., 0., -1., 1.],
            [0., 1., 0., 1.],
            [1., 1., 0., 1.],
        ];
        let points = points(&controls).unwrap();
        let surface = EvalSurface::Nurbs {
            degree: [1, 1],
            knots: [vec![0., 0., 1., 1.], vec![0., 0., 1., 1.]],
            count: [2, 2],
            ctrl: controls.to_vec(),
            closed: [false; 2],
        };
        assert_eq!(maximum_height(&surface, &[], plane), Some(0.));
        assert!(inside(&points, &[plane], 0.));
        assert!(strictly_inside(&surface, &points, &[plane], 1e-9));
        assert!(!coplanar(&points, plane, 1e-9));
        assert!(!inside(
            &points,
            &[HalfSpace {
                normal: -plane.normal,
                ..plane
            }],
            1e-9
        ));
        let flat = [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::new(1., 1., 0.)];
        assert!(coplanar(&flat, plane, 1e-9));
        assert!(!strictly_inside(&surface, &flat, &[plane], 1e-9));
    }

    #[test]
    fn support_hulls_refuse_invalid_controls_and_truncated_tensor() {
        assert!(points(&[]).is_none());
        assert!(points(&[[0., 0., 0., 0.]]).is_none());
        assert!(points(&[[f64::NAN, 0., 0., 1.]]).is_none());
        let surface = EvalSurface::Nurbs {
            degree: [1, 1],
            knots: [vec![0., 0., 1., 1.], vec![0., 0., 1., 1.]],
            count: [2, 2],
            ctrl: vec![],
            closed: [false; 2],
        };
        assert!(!strictly_inside(&surface, &[Vec3::ZERO], &[], 0.));
    }
}
