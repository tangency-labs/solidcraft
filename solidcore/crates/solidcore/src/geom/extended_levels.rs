//! Where the lines along a plane's normal meet a surface carried on without end (a plane, or a
//! cylinder, cone, sphere or torus along its own surface): how far an extrusion over a
//! profile's points must run to reach the To face it ends on, even where that surface lies
//! farther along than the face's own body does.

use crate::geom::vector3d::{Point3, Vec3};

/// A surface carried on without end, as a face's analytic surface gives it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Carried {
    /// A plane: a point on it and its normal (unit).
    Plane {
        /// A point on the plane.
        point: Point3,
        /// The plane's normal (unit).
        normal: Vec3,
    },
    /// A cylinder: a point on its axis, the axis (unit) and the radius.
    Cylinder {
        /// A point on the axis.
        origin: Point3,
        /// The axis direction (unit).
        axis: Vec3,
        /// The radius.
        radius: f64,
    },
    /// A cone: its apex, the axis (unit) and the half angle (radians).
    Cone {
        /// The apex.
        apex: Point3,
        /// The axis direction (unit).
        axis: Vec3,
        /// The angle between the axis and each line of the cone (radians).
        half_angle: f64,
    },
    /// A sphere.
    Sphere {
        /// The centre.
        center: Point3,
        /// The radius.
        radius: f64,
    },
    /// A torus: the centre and the radii.
    Torus {
        /// The centre.
        center: Point3,
        /// The distance from the centre to the middle of the tube.
        major: f64,
        /// The tube's radius.
        minor: f64,
    },
}

/// The roots `t` of `a t^2 + 2 b t + c`, pushed on `out`. `a` is a product of unit directions, so
/// it is a pure number and is judged against rounding alone; `b` and `c` carry lengths and are
/// never compared with it. A vanishing `a` leaves a line.
fn roots(a: f64, b: f64, c: f64, out: &mut Vec<f64>) {
    if a.abs() <= 16.0 * f64::EPSILON {
        if b != 0.0 {
            out.push(-c / (2.0 * b));
        }
        return;
    }
    let disc = b * b - a * c;
    if disc >= 0.0 {
        // The root of larger size first, the other from the product `c / a` of the two, so
        // neither is lost to cancellation.
        let q = -(b + b.signum() * disc.sqrt());
        if q == 0.0 {
            out.push(0.0);
        } else {
            out.extend([q / a, c / q]);
        }
    }
}

/// The levels `t` along the unit normal of the plane through `plane_origin` spanned by `plane_x`
/// and `plane_y` where the line through each of `points` (in that plane) along the normal meets
/// `surface` carried on without end, at most (a torus: the span of its bounding sphere, which
/// holds every meeting): where an extrusion over those points must run to reach it. `None`
/// for axes that do not span a plane.
#[must_use]
pub fn extended_levels(
    surface: &Carried,
    plane_origin: Point3,
    plane_x: Vec3,
    plane_y: Vec3,
    points: &[Point3],
) -> Option<Vec<f64>> {
    let n = plane_x.cross(plane_y).unitize().ok()?;
    let mut out = Vec::new();
    match *surface {
        Carried::Plane { point, normal } => {
            let across = n.dot(normal);
            if across != 0.0 {
                for q in points {
                    out.push((point - *q).dot(normal) / across);
                }
            }
        }
        Carried::Cylinder {
            origin,
            axis,
            radius,
        } => {
            let u = n - axis * n.dot(axis);
            for q in points {
                let w = *q - origin;
                let v = w - axis * w.dot(axis);
                roots(u.dot(u), u.dot(v), v.dot(v) - radius * radius, &mut out);
            }
        }
        Carried::Cone {
            apex,
            axis,
            half_angle,
        } => {
            let c2 = half_angle.cos().powi(2);
            for q in points {
                let w = *q - apex;
                roots(
                    n.dot(axis).powi(2) - c2,
                    w.dot(axis) * n.dot(axis) - c2 * w.dot(n),
                    w.dot(axis).powi(2) - c2 * w.dot(w),
                    &mut out,
                );
            }
        }
        Carried::Sphere { center, radius } => {
            for q in points {
                let w = *q - center;
                roots(1.0, w.dot(n), w.dot(w) - radius * radius, &mut out);
            }
        }
        Carried::Torus {
            center,
            major,
            minor,
        } => {
            let h = (center - plane_origin).dot(n);
            out.extend([h - major - minor, h + major + minor]);
        }
    }
    out.retain(|t| t.is_finite());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const O: Vec3 = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    const X: Vec3 = Vec3 {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    };
    const Y: Vec3 = Vec3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    };
    const Z: Vec3 = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    };

    /// From the XY plane (normal Z), the line over (3, 0) meets a Z-perpendicular plane at
    /// height 7 at 7, a cylinder along Y of radius 5 at +-4, a sphere of radius 5 at +-4, and a
    /// torus centred 2 up with radii 6 and 1 over the whole span 2 -+ 7.
    #[test]
    fn a_line_up_from_a_point_meets_each_surface_where_its_equation_says() {
        let p = [Vec3 {
            x: 3.0,
            y: 0.0,
            z: 0.0,
        }];
        let at = |s: Carried| extended_levels(&s, O, X, Y, &p).unwrap_or_default();
        assert_eq!(
            at(Carried::Plane {
                point: Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 7.0
                },
                normal: Z
            }),
            vec![7.0]
        );
        assert_eq!(
            at(Carried::Cylinder {
                origin: O,
                axis: Y,
                radius: 5.0
            }),
            vec![-4.0, 4.0]
        );
        assert_eq!(
            at(Carried::Sphere {
                center: O,
                radius: 5.0
            }),
            vec![-4.0, 4.0]
        );
        assert_eq!(
            at(Carried::Torus {
                center: Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 2.0
                },
                major: 6.0,
                minor: 1.0
            }),
            vec![-5.0, 9.0]
        );
        // A line far from a sphere centre still gets both of its meetings, and a plane that is
        // nearly but not exactly along the line is still met.
        let far = [Vec3 {
            x: 3.0,
            y: 0.0,
            z: 0.0,
        }];
        let tall = Vec3 {
            x: 0.0,
            y: 0.0,
            z: 2_000_000.0,
        };
        let mut hit = extended_levels(
            &Carried::Sphere {
                center: tall,
                radius: 5.0,
            },
            O,
            X,
            Y,
            &far,
        )
        .unwrap_or_default();
        hit.sort_by(f64::total_cmp);
        assert_eq!(hit.len(), 2);
        assert!(
            (hit[0] - 1_999_996.0).abs() < 1e-6 && (hit[1] - 2_000_004.0).abs() < 1e-6,
            "{hit:?}"
        );
        let slant = Vec3 {
            x: 1.0,
            y: 0.0,
            z: 1e-10,
        }
        .unitize()
        .unwrap_or(X);
        let met = extended_levels(
            &Carried::Plane {
                point: Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0,
                },
                normal: slant,
            },
            O,
            X,
            Y,
            &[O],
        )
        .unwrap_or_default();
        assert_eq!(met.len(), 1);
        assert!((met[0] - 1.0).abs() < 1e-6, "{met:?}");
        // A plane the line runs along has no meeting, and axes that span no plane give none.
        assert!(at(Carried::Plane {
            point: O,
            normal: X
        })
        .is_empty());
        assert!(extended_levels(
            &Carried::Sphere {
                center: O,
                radius: 1.0
            },
            O,
            X,
            X,
            &p
        )
        .is_none());
    }
}
