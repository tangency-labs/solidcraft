//! Whether two analytic surfaces, given by their parameters, are one surface within a
//! tolerance over a reach: the test an application asks of two faces anywhere on two bodies
//! (does a cut leave a face on a surface the body had before?).
//!
//! Upstream: none. [`crate::surface::surface::Surface::same_analytic_surface`] compares the
//! kernel's own surface objects inside a box, and [`crate::query::faces_continue`] proves two
//! faces across an edge continue one surface bit for bit; this one takes the surfaces as
//! numbers, as an application reads them from a record's labels or from a round's torus.
//!
//! | Rust             | Upstream |
//! |------------------|----------|
//! | [`same_surface`] | (none)   |
//! | [`Analytic`]     | (none)   |
//!
//! **The comparison.** Two surfaces of different kinds are never one. Of one kind, every
//! position (a plane's point, an axis's point, an apex, a centre) and every radius must agree
//! within `tol`, and every point of one within `reach` of its origin (a plane's point, a
//! point of an axis) must lie within `tol` of the other. For two planes through `p` and `q`
//! with unit normals `n` and `m` that distance is at most `|(q - p) · n| + reach |n × m|` (a
//! point `q + w` with `w ⊥ m` has `|w · n| <= |w| |n × m|`), and the same with the planes
//! swapped; for two axes through `o` and `p` along unit `u` and `v`, `|u × (p - o)| + reach
//! |u × v|`, and swapped. A cone's half-angles must give radii within `tol` at the height
//! `reach`: `reach |tan h - tan k| <= tol`. So no angle is a fixed constant: the angle bound is
//! the caller's distance tolerance over the region its faces lie in.
//!
//! **Rounding.** Every quantity compared is built from differences of the inputs (`q - p`), unit
//! directions made here, and `reach`, never from the inputs' absolute coordinates, so its
//! rounding is a few units in the last place of `|q - p|` and `reach`, however far the surfaces
//! lie from the origin (`Axis2Placement::are_coaxial` reads its lines at points in absolute
//! coordinates, whose rounding grows with that distance, so it is not used here). Identical
//! inputs compare equal exactly: their differences and cross products are exactly zero. The
//! rounding counts against `tol` only for a `tol` within some 1e-15 of `|q - p|` and `reach`,
//! far below any modelling tolerance.
//!
//! Deviations: not upstream. No recursion.

use crate::error::{Result, SolidError};
use crate::Vec3;

const OP: &str = "query::same_surface";

/// An analytic surface by its parameters. Directions need not be unit; they must be finite
/// and not zero.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Analytic {
    /// The plane through `point` with normal `normal` (either way round).
    Plane {
        /// A point of the plane.
        point: Vec3,
        /// Its normal.
        normal: Vec3,
    },
    /// The cylinder of radius `radius` about the line through `origin` along `axis`.
    Cylinder {
        /// A point of the axis.
        origin: Vec3,
        /// The axis's direction (either way round).
        axis: Vec3,
        /// The radius.
        radius: f64,
    },
    /// The cone with apex `apex`, axis `axis` pointing into the cone, and half-angle
    /// `half_angle` (radians).
    Cone {
        /// The apex.
        apex: Vec3,
        /// The axis, pointing into the cone.
        axis: Vec3,
        /// The half-angle, in radians.
        half_angle: f64,
    },
    /// The sphere of radius `radius` about `centre`.
    Sphere {
        /// The centre.
        centre: Vec3,
        /// The radius.
        radius: f64,
    },
    /// The torus about the line through `centre` along `axis`: its ring (major) and tube
    /// (minor) radii.
    Torus {
        /// The centre of the ring.
        centre: Vec3,
        /// The axis (either way round).
        axis: Vec3,
        /// The ring's radius.
        major: f64,
        /// The tube's radius.
        minor: f64,
    },
}

/// Whether `a` and `b` are one surface within `tol` over `reach` (module notes): positions and
/// radii within `tol`, the points of each plane or axis within `reach` of its origin within
/// `tol` of the other's, cones with radii within `tol` at the height `reach`. Surfaces of two
/// kinds are never one.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a `tol` or `reach` that is negative or not finite, a
/// number of either surface that is not finite, or a direction of zero length.
pub fn same_surface(a: &Analytic, b: &Analytic, tol: f64, reach: f64) -> Result<bool> {
    if !(tol.is_finite() && tol >= 0.0 && reach.is_finite() && reach >= 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "a tolerance or reach that is negative or not finite",
        ));
    }
    let (a, b) = (unit(a)?, unit(b)?);
    let near = |p: Vec3, q: Vec3| (p - q).length() <= tol;
    let close = |r: f64, s: f64| (r - s).abs() <= tol;
    let tilt = |u: Vec3, v: Vec3| reach * u.cross(v).length();
    // the farthest a point of either plane within `reach` of its point lies from the other
    let planes = |p: Vec3, n: Vec3, q: Vec3, m: Vec3| {
        (q - p).dot(n).abs() + tilt(n, m) <= tol && (p - q).dot(m).abs() + tilt(n, m) <= tol
    };
    // the farthest a point of either axis within `reach` of its origin lies from the other
    let coaxial = |o: Vec3, u: Vec3, p: Vec3, v: Vec3| {
        u.cross(p - o).length() + tilt(u, v) <= tol && v.cross(o - p).length() + tilt(u, v) <= tol
    };
    Ok(match (a, b) {
        (
            Analytic::Plane {
                point: p,
                normal: n,
            },
            Analytic::Plane {
                point: q,
                normal: m,
            },
        ) => planes(p, n, q, m),
        (
            Analytic::Cylinder {
                origin: o,
                axis: u,
                radius: r,
            },
            Analytic::Cylinder {
                origin: p,
                axis: v,
                radius: s,
            },
        ) => close(r, s) && coaxial(o, u, p, v),
        (
            Analytic::Cone {
                apex: o,
                axis: u,
                half_angle: h,
            },
            Analytic::Cone {
                apex: p,
                axis: v,
                half_angle: k,
            },
        ) => {
            near(o, p)
                && u.dot(v) > 0.0
                && coaxial(o, u, p, v)
                && reach * (h.tan() - k.tan()).abs() <= tol
        }
        (
            Analytic::Sphere {
                centre: c,
                radius: r,
            },
            Analytic::Sphere {
                centre: d,
                radius: s,
            },
        ) => near(c, d) && close(r, s),
        (
            Analytic::Torus {
                centre: c,
                axis: u,
                major: r,
                minor: t,
            },
            Analytic::Torus {
                centre: d,
                axis: v,
                major: s,
                minor: w,
            },
        ) => near(c, d) && close(r, s) && close(t, w) && coaxial(c, u, d, v),
        _ => false,
    })
}

/// `s` with its direction made unit, after checking that every number is finite.
fn unit(s: &Analytic) -> Result<Analytic> {
    let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    let bad = || SolidError::invalid_input(OP, "a surface number that is not finite");
    let dir = |v: Vec3| -> Result<Vec3> {
        if finite(v) {
            v.unitize()
        } else {
            Err(bad())
        }
    };
    let at = |v: Vec3| if finite(v) { Ok(v) } else { Err(bad()) };
    let num = |x: f64| if x.is_finite() { Ok(x) } else { Err(bad()) };
    Ok(match *s {
        Analytic::Plane { point, normal } => Analytic::Plane {
            point: at(point)?,
            normal: dir(normal)?,
        },
        Analytic::Cylinder {
            origin,
            axis,
            radius,
        } => Analytic::Cylinder {
            origin: at(origin)?,
            axis: dir(axis)?,
            radius: num(radius)?,
        },
        Analytic::Cone {
            apex,
            axis,
            half_angle,
        } => Analytic::Cone {
            apex: at(apex)?,
            axis: dir(axis)?,
            half_angle: num(half_angle)?,
        },
        Analytic::Sphere { centre, radius } => Analytic::Sphere {
            centre: at(centre)?,
            radius: num(radius)?,
        },
        Analytic::Torus {
            centre,
            axis,
            major,
            minor,
        } => Analytic::Torus {
            centre: at(centre)?,
            axis: dir(axis)?,
            major: num(major)?,
            minor: num(minor)?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    /// `same_surface(a, b)`, checked to be `same_surface(b, a)`.
    fn same(a: Analytic, b: Analytic, tol: f64, reach: f64) -> bool {
        let (Ok(x), Ok(y)) = (
            same_surface(&a, &b, tol, reach),
            same_surface(&b, &a, tol, reach),
        ) else {
            panic!("refused: {a:?} {b:?}");
        };
        assert_eq!(x, y, "not symmetric: {a:?} {b:?}");
        x
    }

    #[test]
    fn a_plane_is_one_surface_with_itself_turned_over_and_moved_along_it() {
        let p = Analytic::Plane {
            point: v(0.0, 0.0, 2.0),
            normal: v(0.0, 0.0, 1.0),
        };
        let flipped = Analytic::Plane {
            point: v(5.0, -3.0, 2.0),
            normal: v(0.0, 0.0, -2.0),
        };
        assert!(same(p, p, 0.0, 100.0));
        assert!(same(p, flipped, 0.0, 100.0));
        // 1e-6 above: within a tolerance of 1e-6, not of 1e-7
        let above = Analytic::Plane {
            point: v(0.0, 0.0, 2.0 + 1e-6),
            normal: v(0.0, 0.0, 1.0),
        };
        assert!(same(p, above, 1.1e-6, 100.0));
        assert!(!same(p, above, 1e-7, 100.0));
    }

    #[test]
    fn a_tilted_plane_is_one_surface_only_while_its_tilt_stays_within_the_tolerance_at_the_reach() {
        // tilted by the angle whose sine is 1e-8 about the x axis, through the same point: at
        // distance 100 along y it is 1e-6 off
        let s: f64 = 1e-8;
        let c = (1.0 - s * s).sqrt();
        let p = Analytic::Plane {
            point: v(0.0, 0.0, 0.0),
            normal: v(0.0, 0.0, 1.0),
        };
        let tilted = Analytic::Plane {
            point: v(0.0, 0.0, 0.0),
            normal: v(0.0, s, c),
        };
        assert!(same(p, tilted, 1.01e-6, 100.0));
        assert!(!same(p, tilted, 0.99e-6, 100.0));
        // the same tilt over a reach of 1 is within 1e-7
        assert!(same(p, tilted, 1e-7, 1.0));
    }

    #[test]
    fn a_cylinder_is_one_surface_with_one_on_the_same_axis_line_either_way_round() {
        let c = Analytic::Cylinder {
            origin: v(1.0, 2.0, 0.0),
            axis: v(0.0, 0.0, 1.0),
            radius: 3.0,
        };
        let along = Analytic::Cylinder {
            origin: v(1.0, 2.0, -40.0),
            axis: v(0.0, 0.0, -5.0),
            radius: 3.0,
        };
        assert!(same(c, along, 0.0, 50.0));
        let wider = Analytic::Cylinder {
            origin: v(1.0, 2.0, 0.0),
            axis: v(0.0, 0.0, 1.0),
            radius: 3.0 + 2e-6,
        };
        assert!(same(c, wider, 3e-6, 50.0));
        assert!(!same(c, wider, 1e-6, 50.0));
        // the axis moved 2e-6 across
        let beside = Analytic::Cylinder {
            origin: v(1.0 + 2e-6, 2.0, 0.0),
            axis: v(0.0, 0.0, 1.0),
            radius: 3.0,
        };
        assert!(same(c, beside, 3e-6, 50.0));
        assert!(!same(c, beside, 1e-6, 50.0));
        // the axis tilted by 1e-8 about its origin: 5e-7 off at the reach 50
        let s: f64 = 1e-8;
        let tilted = Analytic::Cylinder {
            origin: v(1.0, 2.0, 0.0),
            axis: v(s, 0.0, (1.0 - s * s).sqrt()),
            radius: 3.0,
        };
        assert!(same(c, tilted, 6e-7, 50.0));
        assert!(!same(c, tilted, 4e-7, 50.0));
    }

    #[test]
    fn a_cone_is_one_surface_only_with_the_same_apex_sense_and_half_angle() {
        let half = 0.5_f64;
        let c = Analytic::Cone {
            apex: v(0.0, 0.0, 0.0),
            axis: v(0.0, 0.0, 1.0),
            half_angle: half,
        };
        assert!(same(
            c,
            Analytic::Cone {
                apex: v(0.0, 0.0, 0.0),
                axis: v(0.0, 0.0, 7.0),
                half_angle: half
            },
            0.0,
            10.0
        ));
        // the other nappe: its axis points the other way
        assert!(!same(
            c,
            Analytic::Cone {
                apex: v(0.0, 0.0, 0.0),
                axis: v(0.0, 0.0, -1.0),
                half_angle: half
            },
            1e-3,
            10.0
        ));
        // a half-angle whose tangent is 1e-7 larger: radii 1e-6 apart at the height 10
        let k = (half.tan() + 1e-7).atan();
        let wider = Analytic::Cone {
            apex: v(0.0, 0.0, 0.0),
            axis: v(0.0, 0.0, 1.0),
            half_angle: k,
        };
        assert!(same(c, wider, 1.01e-6, 10.0));
        assert!(!same(c, wider, 0.99e-6, 10.0));
        let moved = Analytic::Cone {
            apex: v(0.0, 0.0, 1e-5),
            axis: v(0.0, 0.0, 1.0),
            half_angle: half,
        };
        assert!(!same(c, moved, 1e-6, 10.0));
    }

    #[test]
    fn spheres_and_tori_compare_their_centres_radii_and_axes() {
        let s = Analytic::Sphere {
            centre: v(1.0, 1.0, 1.0),
            radius: 2.0,
        };
        assert!(same(
            s,
            Analytic::Sphere {
                centre: v(1.0, 1.0, 1.0 + 1e-6),
                radius: 2.0
            },
            2e-6,
            10.0
        ));
        assert!(!same(
            s,
            Analytic::Sphere {
                centre: v(1.0, 1.0, 1.0),
                radius: 2.0 + 1e-5
            },
            2e-6,
            10.0
        ));
        let t = Analytic::Torus {
            centre: v(0.0, 0.0, 0.0),
            axis: v(0.0, 1.0, 0.0),
            major: 5.0,
            minor: 1.0,
        };
        let flipped = Analytic::Torus {
            centre: v(0.0, 0.0, 0.0),
            axis: v(0.0, -1.0, 0.0),
            major: 5.0,
            minor: 1.0,
        };
        assert!(same(t, flipped, 0.0, 10.0));
        let fatter = Analytic::Torus {
            centre: v(0.0, 0.0, 0.0),
            axis: v(0.0, 1.0, 0.0),
            major: 5.0,
            minor: 1.0 + 1e-5,
        };
        assert!(!same(t, fatter, 1e-6, 10.0));
        let turned = Analytic::Torus {
            centre: v(0.0, 0.0, 0.0),
            axis: v(1.0, 0.0, 0.0),
            major: 5.0,
            minor: 1.0,
        };
        assert!(!same(t, turned, 1e-3, 10.0));
    }

    #[test]
    fn surfaces_of_two_kinds_are_never_one() {
        // a cylinder and a cone of half-angle 0 on the same axis, a sphere and a torus with no hole
        let cyl = Analytic::Cylinder {
            origin: v(0.0, 0.0, 0.0),
            axis: v(0.0, 0.0, 1.0),
            radius: 1.0,
        };
        let cone = Analytic::Cone {
            apex: v(0.0, 0.0, 0.0),
            axis: v(0.0, 0.0, 1.0),
            half_angle: 0.0,
        };
        assert!(!same(cyl, cone, 1.0, 1.0));
        let ball = Analytic::Sphere {
            centre: v(0.0, 0.0, 0.0),
            radius: 1.0,
        };
        let ring = Analytic::Torus {
            centre: v(0.0, 0.0, 0.0),
            axis: v(0.0, 0.0, 1.0),
            major: 0.0,
            minor: 1.0,
        };
        assert!(!same(ball, ring, 1.0, 1.0));
        let plane = Analytic::Plane {
            point: v(0.0, 0.0, 0.0),
            normal: v(0.0, 0.0, 1.0),
        };
        assert!(!same(plane, ball, 1.0, 1.0));
    }

    #[test]
    fn far_from_the_origin_an_identical_surface_still_compares_equal() {
        let far = 1e8;
        let c = Analytic::Cylinder {
            origin: v(far, far, far),
            axis: v(1.0, 2.0, 3.0),
            radius: 4.0,
        };
        assert!(same(c, c, 0.0, 1e3));
        let t = Analytic::Torus {
            centre: v(-far, far, 0.0),
            axis: v(3.0, -1.0, 2.0),
            major: 5.0,
            minor: 1.0,
        };
        assert!(same(t, t, 0.0, 1e3));
        let p = Analytic::Plane {
            point: v(far, -far, far),
            normal: v(1.0, 1.0, 1.0),
        };
        assert!(same(p, p, 0.0, 1e3));
    }

    #[test]
    fn bad_numbers_are_refused() {
        let p = Analytic::Plane {
            point: v(0.0, 0.0, 0.0),
            normal: v(0.0, 0.0, 1.0),
        };
        let zero = Analytic::Plane {
            point: v(0.0, 0.0, 0.0),
            normal: v(0.0, 0.0, 0.0),
        };
        let nan = Analytic::Sphere {
            centre: v(f64::NAN, 0.0, 0.0),
            radius: 1.0,
        };
        let inf = Analytic::Cylinder {
            origin: v(0.0, 0.0, 0.0),
            axis: v(0.0, 0.0, 1.0),
            radius: f64::INFINITY,
        };
        for (a, b, tol, reach) in [
            (p, zero, 1.0, 1.0),
            (nan, nan, 1.0, 1.0),
            (inf, inf, 1.0, 1.0),
            (p, p, -1.0, 1.0),
            (p, p, f64::NAN, 1.0),
            (p, p, 1.0, f64::INFINITY),
        ] {
            assert!(
                matches!(
                    same_surface(&a, &b, tol, reach),
                    Err(SolidError::InvalidInput { .. })
                ),
                "{a:?} {b:?} {tol} {reach}"
            );
        }
    }
}
