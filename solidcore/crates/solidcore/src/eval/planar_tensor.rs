//! Planarity and exact parameter curves for a tensor patch's planar chart.

use super::vec::VecExt;
use super::{Curve, Curve2, SurfaceEval};
use crate::Vec3;

/// The frame of a planar, positive-weight tensor net.
#[derive(Clone, Copy, Debug)]
pub struct PlanarTensor {
    /// A point on the plane.
    pub origin: Vec3,
    /// The first unit parameter direction.
    pub u: Vec3,
    /// The second unit parameter direction.
    pub v: Vec3,
}

impl PlanarTensor {
    /// Accept a plane only when its complete control hull lies within roundoff.
    pub fn of(surface: &SurfaceEval, domain: [[f64; 2]; 2]) -> Option<Self> {
        let SurfaceEval::Eval(super::EvalSurface::Nurbs(nurbs), _) = surface else {
            return None;
        };
        let [u, v] = domain.map(|[a, b]| (a + b) * 0.5);
        let (origin, du, dv) = surface.derivs(u, v)?;
        let u = du.normalized()?;
        let normal = u.cross(dv.normalized()?).normalized()?;
        let project = |&[x, y, z, w]: &[f64; 4]| {
            if !w.is_finite() || w <= 0.0 {
                return None;
            }
            let point = Vec3::new(x / w, y / w, z / w);
            point.is_finite().then_some(point)
        };
        let anchor = project(nurbs.ctrl().first()?)?;
        let mut scale: f64 = 0.0;
        let mut distance = (origin - anchor).dot(normal).abs();
        for control in nurbs.ctrl() {
            let offset = project(control)? - anchor;
            scale = scale.max(offset.len());
            distance = distance.max(offset.dot(normal).abs());
        }
        if !scale.is_finite() || distance > 64.0 * f64::EPSILON * scale {
            return None;
        }
        Some(Self {
            origin,
            u,
            v: normal.cross(u),
        })
    }

    /// Project all the controls of a spline into the plane in their native parameter.
    pub fn parameter_curve(self, curve: Curve) -> Option<Curve2> {
        let point = |x, y, z, w| {
            let delta = Vec3::new(x, y, z) - self.origin * w;
            [delta.dot(self.u), delta.dot(self.v)]
        };
        Some(match curve {
            Curve::Spline { knots, points } => Curve2::Spline {
                knots,
                points: points
                    .into_iter()
                    .map(|[x, y, z]| point(x, y, z, 1.0))
                    .collect(),
            },
            Curve::RationalSpline { knots, homogeneous } => Curve2::RationalSpline {
                knots,
                homogeneous: homogeneous
                    .into_iter()
                    .map(|[x, y, z, w]| {
                        let [u, v] = point(x, y, z, w);
                        [u, v, w]
                    })
                    .collect(),
            },
            _ => return None,
        })
    }

    /// The bounded parameter box of complete positive-weight trim control hulls.
    pub fn parameter_box<'a>(curves: impl Iterator<Item = &'a Curve2>) -> Option<[[f64; 2]; 2]> {
        let mut bounds = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
        for curve in curves {
            let mut add = |point: [f64; 2]| {
                for (axis, x) in bounds.iter_mut().zip(point) {
                    axis[0] = axis[0].min(x);
                    axis[1] = axis[1].max(x);
                }
            };
            match curve {
                Curve2::Spline { points, .. } => points.iter().copied().for_each(&mut add),
                Curve2::RationalSpline { homogeneous, .. } => {
                    for &[x, y, w] in homogeneous {
                        if !w.is_finite() || w <= 0.0 {
                            return None;
                        }
                        add([x / w, y / w]);
                    }
                }
            }
        }
        bounds
            .iter()
            .all(|[a, b]| a.is_finite() && b.is_finite() && a < b)
            .then_some(bounds)
    }
}

/// Verify that every complete positive-weight trim spline stays along one
/// side of the parameter rectangle. Require all four sides to be represented.
pub fn rectangular_trim(curves: &[Curve2], span: [[f64; 2]; 2]) -> bool {
    let epsilon = span.map(|[a, b]| ((b - a) * 1e-6).max(1e-9));
    let mut used = [false; 4];
    for curve in curves {
        let control = |[x, y, w]: [f64; 3], side: usize| {
            if !w.is_finite() || w <= 0.0 {
                return false;
            }
            let point = [x / w, y / w];
            let axis = side / 2;
            let other = 1 - axis;
            (point[axis] - span[axis][side % 2]).abs() <= epsilon[axis]
                && point[other] >= span[other][0] - epsilon[other]
                && point[other] <= span[other][1] + epsilon[other]
        };
        let side = (0..4).find(|&side| match curve {
            Curve2::Spline { points, .. } => {
                !points.is_empty() && points.iter().all(|&[x, y]| control([x, y, 1.0], side))
            }
            Curve2::RationalSpline { homogeneous, .. } => {
                !homogeneous.is_empty() && homogeneous.iter().all(|&p| control(p, side))
            }
        });
        let Some(side) = side else {
            return false;
        };
        used[side] = true;
    }
    used.into_iter().all(|side| side)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Surface;

    #[test]
    fn flat_tensor_projects_rational_controls_in_the_original_parameter() {
        let knots = vec![0.0, 0.0, 1.0, 1.0];
        let tensor = |z| Surface::Spline {
            u_knots: knots.clone(),
            v_knots: knots.clone(),
            points: vec![
                vec![[2.0, 3.0, 4.0], [2.0, 5.0, 4.0]],
                vec![[4.0, 3.0, 4.0], [4.0, 5.0, z]],
            ],
        };
        let plane =
            PlanarTensor::of(&SurfaceEval::new(&tensor(4.0)).unwrap(), [[0.0, 1.0]; 2]).unwrap();
        let curve = Curve::RationalSpline {
            knots: knots.clone(),
            homogeneous: vec![[2.0, 3.0, 4.0, 1.0], [8.0, 10.0, 8.0, 2.0]],
        };
        let curve = plane.parameter_curve(curve).unwrap();
        assert_eq!(curve.eval(0.0), Some([-1.0, -1.0]));
        assert_eq!(curve.eval(1.0), Some([1.0, 1.0]));
        assert!(
            PlanarTensor::of(&SurfaceEval::new(&tensor(4.001)).unwrap(), [[0.0, 1.0]; 2]).is_none()
        );
    }

    #[test]
    fn coincident_support_normal_obeys_chart_orientation() {
        let plane = |u_axis, v_axis| {
            SurfaceEval::new(&Surface::Plane {
                origin: [0.0; 3],
                u_axis,
                v_axis,
            })
            .unwrap()
        };
        let x = [1.0, 0.0, 0.0];
        let y = [0.0, 1.0, 0.0];
        let a = plane(x, y);
        let b = plane(y, x);
        let domain = [[0.0, 1.0]; 2];
        assert_eq!(a.opposed_to(domain, &a, domain), Some(false));
        assert_eq!(a.opposed_to(domain, &b, domain), Some(true));
    }
}
