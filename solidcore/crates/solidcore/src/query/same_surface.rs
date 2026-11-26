//! Whether two analytic surfaces, given by their parameters, are one surface within a
//! tolerance inside a box: the test an application asks of two faces anywhere on two bodies
//! (does a cut leave a face on a surface the body had before?).
//!
//! Upstream: none. [`crate::query::faces_continue`] proves two faces across an edge continue
//! one surface bit for bit; this one takes the surfaces as numbers, as an application reads
//! them from a record's labels or from a round's torus.
//!
//! | Rust             | Upstream |
//! |------------------|----------|
//! | [`same_surface`] | (none)   |
//! | [`Analytic`]     | (none)   |
//!
//! **The comparison.** A cylinder is a cone of half-angle 0, and a torus with no hole (major
//! radius within `tol` of 0) is the sphere of its minor radius, as
//! [`crate::surface::surface::Surface::same_analytic_surface`] takes them; any other two kinds are never one surface.
//! Two cones, two spheres, or two tori are compared by the definitions
//! [`crate::surface::surface::Surface::same_analytic_surface`] compares them by, the same code
//! ([`coaxial_cones_agree`], [`balls_agree`], [`tori_agree`]): coaxial within `tol` at the box's
//! diagonal, radii within `tol` at both ends of the box along the axis (both nappes of a cone
//! are one surface), centres and radii within `tol`. Its further enclosure of two cones or
//! spheres that fail that comparison but stay within `tol` over the box (two axes a little
//! apart) is not run here: an application asks this of every pair of faces of two bodies, and
//! that search reads up to 256 boxes for every pair it cannot settle. Such a pair is two
//! surfaces here.
//!
//! Two planes are one surface when every point of either inside the box lies within `tol` of
//! the other. The points of plane `B` inside the box lie in the hull of the box's corners
//! dropped onto `B`, and the distance to plane `A` is affine on `B`, so it is largest at one of
//! those eight points: `|(c - p)·n - ((c - q)·m)(m·n)|` for a corner `c`, `p` and `n` the point
//! and unit normal of `A`, `q` and `m` those of `B`.
//!
//! **Rounding.** The plane test counts its rounding: with unit normals, the computed distance is
//! within `gamma(12) (|c - p| + |c - q|)` of the exact one (two differences, two dot products of
//! three terms, a product and a difference, and the normals' own rounding to unit length), and
//! that is added before the comparison, so a pair is one surface only when every corner surely
//! lies within `tol`. The cone, sphere and torus comparisons are the boolean's, which compare
//! their computed values with `tol` as they come; their rounding is a few units in the last
//! place of the coordinates and the box's diagonal.
//!
//! Deviations: not upstream. No recursion.

use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent3d::Extent3d;
use crate::nurbs::math::gamma;
use crate::surface::surface::p8::{balls_agree, coaxial_cones_agree, tori_agree};
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
    /// `half_angle` (radians, below a right angle in size).
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

/// What [`same_surface`] compares a surface as (module notes).
enum Kind {
    Plane {
        point: Vec3,
        normal: Vec3,
    },
    /// A cone's axis placement, its radius at the placement's origin and its slope.
    Cone(Axis2Placement, f64, f64),
    /// A sphere's centre and radius.
    Ball(Vec3, f64),
    /// A torus's placement and its major and minor radii.
    Torus(Axis2Placement, f64, f64),
}

/// Whether `a` and `b` are one surface within `tol` inside `region` (module notes): cones,
/// spheres and tori by the definitions [`crate::surface::surface::Surface::same_analytic_surface`] compares, planes when
/// every point of either inside the box surely lies within `tol` of the other.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a `tol` that is negative or not finite, a box that is
/// empty, not finite or of no extent, a number of either surface that is not finite, a
/// direction of zero length, or a cone's half-angle of a right angle or more in size.
pub fn same_surface(a: &Analytic, b: &Analytic, tol: f64, region: &Extent3d) -> Result<bool> {
    if !(tol.is_finite() && tol >= 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "a tolerance that is negative or not finite",
        ));
    }
    let (lo, hi) = (region.min(), region.max());
    let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    let reach = region.size().length();
    if !(finite(lo) && finite(hi) && lo.x <= hi.x && lo.y <= hi.y && lo.z <= hi.z)
        || !(reach.is_finite() && reach > 0.0)
    {
        return Err(SolidError::invalid_input(
            OP,
            "a box that is empty, not finite or of no extent",
        ));
    }
    Ok(match (kind(a, tol)?, kind(b, tol)?) {
        (
            Kind::Plane {
                point: p,
                normal: n,
            },
            Kind::Plane {
                point: q,
                normal: m,
            },
        ) => planes_agree((p, n), (q, m), tol, region),
        (Kind::Cone(pa, ra, sa), Kind::Cone(pb, rb, sb)) => {
            coaxial_cones_agree((&pa, ra, sa), (&pb, rb, sb), tol, region) == Some(true)
        }
        (Kind::Ball(c, r), Kind::Ball(d, s)) => balls_agree((c, r), (d, s), tol),
        (Kind::Torus(pa, ra, ta), Kind::Torus(pb, rb, tb)) => {
            tori_agree((&pa, ra, ta), (&pb, rb, tb), tol, reach)
        }
        _ => false,
    })
}

/// Whether every point of either plane inside `region` surely lies within `tol` of the other
/// (module notes).
fn planes_agree(a: (Vec3, Vec3), b: (Vec3, Vec3), tol: f64, region: &Extent3d) -> bool {
    // the farthest the points of `on` inside the box lie from `from`, with its rounding
    let within = |from: (Vec3, Vec3), on: (Vec3, Vec3)| {
        let ((p, n), (q, m)) = (from, on);
        region.corners().iter().all(|&c| {
            let (to_p, to_q) = (c - p, c - q);
            let gap = (to_p.dot(n) - to_q.dot(m) * m.dot(n)).abs();
            gap + gamma(12) * (to_p.length() + to_q.length()) <= tol
        })
    };
    within(a, b) && within(b, a)
}

/// What `s` is compared as, its numbers checked and its directions made unit.
fn kind(s: &Analytic, tol: f64) -> Result<Kind> {
    let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    let bad = || SolidError::invalid_input(OP, "a surface number that is not finite");
    let at = |v: Vec3| if finite(v) { Ok(v) } else { Err(bad()) };
    let num = |x: f64| if x.is_finite() { Ok(x) } else { Err(bad()) };
    let dir = |v: Vec3| at(v)?.unitize();
    // the placement whose Z axis is the line through `origin` along `axis`
    let line = |origin: Vec3, axis: Vec3| -> Result<Axis2Placement> {
        let [_, y, z] = dir(axis)?.make_unit_ortho_vectors(None)?;
        Ok(Axis2Placement::from_stored(at(origin)?, y, z))
    };
    Ok(match *s {
        Analytic::Plane { point, normal } => Kind::Plane {
            point: at(point)?,
            normal: dir(normal)?,
        },
        Analytic::Cylinder {
            origin,
            axis,
            radius,
        } => Kind::Cone(line(origin, axis)?, num(radius)?, 0.0),
        Analytic::Cone {
            apex,
            axis,
            half_angle,
        } => {
            if num(half_angle)?.abs() >= core::f64::consts::FRAC_PI_2 {
                return Err(SolidError::invalid_input(
                    OP,
                    "a cone's half-angle of a right angle or more",
                ));
            }
            Kind::Cone(line(apex, axis)?, 0.0, half_angle.tan())
        }
        Analytic::Sphere { centre, radius } => Kind::Ball(at(centre)?, num(radius)?),
        Analytic::Torus {
            centre,
            axis,
            major,
            minor,
        } => {
            let (major, minor) = (num(major)?, num(minor)?);
            let placement = line(centre, axis)?;
            if major.abs() <= tol {
                Kind::Ball(placement.origin(), minor)
            } else {
                Kind::Torus(placement, major, minor)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    /// The box of half-side `half` about `centre`.
    fn cube(centre: Vec3, half: f64) -> Extent3d {
        Extent3d::from_points(centre - v(half, half, half), centre + v(half, half, half))
    }

    /// `same_surface(a, b)`, checked to be `same_surface(b, a)`.
    fn same(a: Analytic, b: Analytic, tol: f64, region: &Extent3d) -> bool {
        let (Ok(x), Ok(y)) = (
            same_surface(&a, &b, tol, region),
            same_surface(&b, &a, tol, region),
        ) else {
            panic!("refused: {a:?} {b:?}");
        };
        assert_eq!(x, y, "not symmetric: {a:?} {b:?}");
        x
    }

    fn plane(point: Vec3, normal: Vec3) -> Analytic {
        Analytic::Plane { point, normal }
    }

    #[test]
    fn a_plane_is_one_surface_with_itself_turned_over_and_moved_along_it() {
        let region = cube(v(0.0, 0.0, 0.0), 50.0);
        let p = plane(v(0.0, 0.0, 2.0), v(0.0, 0.0, 1.0));
        assert!(same(p, p, 1e-9, &region));
        assert!(same(
            p,
            plane(v(5.0, -3.0, 2.0), v(0.0, 0.0, -2.0)),
            1e-9,
            &region
        ));
        // 1e-6 above: within a tolerance of 1.1e-6, not of 1e-7
        let above = plane(v(0.0, 0.0, 2.0 + 1e-6), v(0.0, 0.0, 1.0));
        assert!(same(p, above, 1.1e-6, &region));
        assert!(!same(p, above, 1e-7, &region));
        // an oblique plane given by another point and a longer normal
        let oblique = plane(v(1.0, 2.0, 3.0), v(1.0, 2.0, 3.0));
        assert!(same(
            oblique,
            plane(v(4.0, 2.0, 2.0), v(3.0, 6.0, 9.0)),
            1e-9,
            &region
        ));
        // a plane at right angles is never the same
        assert!(!same(
            p,
            plane(v(0.0, 0.0, 2.0), v(1.0, 0.0, 0.0)),
            1.0,
            &region
        ));
    }

    #[test]
    fn a_tilted_plane_is_one_surface_only_while_it_stays_within_the_tolerance_in_the_box() {
        // tilted by the angle whose sine is 1e-8 about the x axis, through the origin: at
        // |y| = 50, the box's side, it is 5e-7 off
        let s: f64 = 1e-8;
        let c = (1.0 - s * s).sqrt();
        let p = plane(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0));
        let tilted = plane(v(0.0, 0.0, 0.0), v(0.0, s, c));
        let region = cube(v(0.0, 0.0, 0.0), 50.0);
        assert!(same(p, tilted, 5.1e-7, &region));
        assert!(!same(p, tilted, 4.9e-7, &region));
        // a box of half-side 5 about (1000, 0, 0) with the plane tilted about the y axis by
        // 5e-8: the plane's own points lie at the origin, but the body is 5e-5 off at x = 1000
        let body = cube(v(1000.0, 0.0, 0.0), 5.0);
        let far_tilt = plane(v(0.0, 0.0, 0.0), v(-5e-8, 0.0, (1.0 - 2.5e-15_f64).sqrt()));
        assert!(!same(p, far_tilt, 1e-6, &body));
        assert!(same(p, far_tilt, 5.1e-5, &body));
    }

    #[test]
    fn far_from_the_origin_planes_a_hair_apart_compare_by_their_gap() {
        let far = 1e6;
        let region = cube(v(far, far, far), 10.0);
        let n = v(1.0, 2.0, 2.0);
        let p = plane(v(far, far, far), n);
        // moved 3e-7 along the unit normal (1, 2, 2)/3
        let q = plane(v(far + 1e-7, far + 2e-7, far + 2e-7), n * 4.0);
        assert!(same(p, q, 1e-6, &region));
        assert!(!same(p, q, 1e-7, &region));
    }

    fn cylinder(origin: Vec3, axis: Vec3, radius: f64) -> Analytic {
        Analytic::Cylinder {
            origin,
            axis,
            radius,
        }
    }

    #[test]
    fn a_cylinder_is_one_surface_with_one_on_the_same_axis_line_either_way_round() {
        let region = cube(v(0.0, 0.0, 0.0), 25.0);
        let c = cylinder(v(1.0, 2.0, 0.0), v(0.0, 0.0, 1.0), 3.0);
        assert!(same(
            c,
            cylinder(v(1.0, 2.0, -40.0), v(0.0, 0.0, -5.0), 3.0),
            1e-9,
            &region
        ));
        let wider = cylinder(v(1.0, 2.0, 0.0), v(0.0, 0.0, 1.0), 3.0 + 2e-6);
        assert!(same(c, wider, 3e-6, &region));
        assert!(!same(c, wider, 1e-6, &region));
        // the axis moved 2e-6 across
        let beside = cylinder(v(1.0 + 2e-6, 2.0, 0.0), v(0.0, 0.0, 1.0), 3.0);
        assert!(same(c, beside, 3e-6, &region));
        assert!(!same(c, beside, 1e-6, &region));
        // the axis tilted by 1e-9 about its origin: 8.7e-8 off at the box's diagonal, 86.6
        let s: f64 = 1e-9;
        let tilted = cylinder(v(1.0, 2.0, 0.0), v(s, 0.0, (1.0 - s * s).sqrt()), 3.0);
        assert!(same(c, tilted, 1e-7, &region));
        assert!(!same(c, tilted, 1e-8, &region));
    }

    fn cone(apex: Vec3, axis: Vec3, half_angle: f64) -> Analytic {
        Analytic::Cone {
            apex,
            axis,
            half_angle,
        }
    }

    #[test]
    fn a_cone_is_one_surface_with_the_same_apex_axis_and_half_angle() {
        let half = 0.5_f64;
        let region = cube(v(0.0, 0.0, 0.0), 10.0);
        let c = cone(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), half);
        assert!(same(
            c,
            cone(v(0.0, 0.0, 0.0), v(0.0, 0.0, 7.0), half),
            1e-9,
            &region
        ));
        // the other nappe is the same surface, as the boolean takes it
        assert!(same(
            c,
            cone(v(0.0, 0.0, 0.0), v(0.0, 0.0, -1.0), half),
            1e-9,
            &region
        ));
        // a half-angle whose tangent is 1e-7 larger: radii 1e-6 apart at the box's ends
        let k = (half.tan() + 1e-7).atan();
        let wider = cone(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), k);
        assert!(same(c, wider, 1.01e-6, &region));
        assert!(!same(c, wider, 0.99e-6, &region));
        // the apex moved 1e-5 along the axis: radii tan(0.5) 1e-5 apart
        let moved = cone(v(0.0, 0.0, 1e-5), v(0.0, 0.0, 1.0), half);
        assert!(!same(c, moved, 1e-6, &region));
        // the axis tilted by 1e-6
        let s: f64 = 1e-6;
        let tilted = cone(v(0.0, 0.0, 0.0), v(s, 0.0, (1.0 - s * s).sqrt()), half);
        assert!(!same(c, tilted, 1e-6, &region));
        // a cylinder is a cone of half-angle 0: never this one
        assert!(!same(
            c,
            cylinder(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), 1.0),
            1e-3,
            &region
        ));
    }

    #[test]
    fn spheres_compare_their_centres_and_radii() {
        let region = cube(v(0.0, 0.0, 0.0), 10.0);
        let s = Analytic::Sphere {
            centre: v(1.0, 1.0, 1.0),
            radius: 2.0,
        };
        let moved = |d: f64| Analytic::Sphere {
            centre: v(1.0, 1.0, 1.0 + d),
            radius: 2.0,
        };
        assert!(same(s, moved(1e-6), 2e-6, &region));
        assert!(!same(s, moved(1e-5), 2e-6, &region));
        let bigger = Analytic::Sphere {
            centre: v(1.0, 1.0, 1.0),
            radius: 2.0 + 1e-5,
        };
        assert!(!same(s, bigger, 2e-6, &region));
    }

    fn torus(centre: Vec3, axis: Vec3, major: f64, minor: f64) -> Analytic {
        Analytic::Torus {
            centre,
            axis,
            major,
            minor,
        }
    }

    #[test]
    fn tori_compare_their_centres_axes_and_both_radii() {
        let region = cube(v(0.0, 0.0, 0.0), 10.0);
        let o = v(0.0, 0.0, 0.0);
        let t = torus(o, v(0.0, 1.0, 0.0), 5.0, 1.0);
        assert!(same(
            t,
            torus(o, v(0.0, -1.0, 0.0), 5.0, 1.0),
            1e-9,
            &region
        ));
        assert!(!same(
            t,
            torus(v(0.0, 0.0, 1e-5), v(0.0, 1.0, 0.0), 5.0, 1.0),
            1e-6,
            &region
        ));
        assert!(!same(
            t,
            torus(o, v(0.0, 1.0, 0.0), 5.0 + 1e-5, 1.0),
            1e-6,
            &region
        ));
        assert!(!same(
            t,
            torus(o, v(0.0, 1.0, 0.0), 5.0, 1.0 + 1e-5),
            1e-6,
            &region
        ));
        assert!(!same(
            t,
            torus(o, v(1.0, 0.0, 0.0), 5.0, 1.0),
            1e-3,
            &region
        ));
        // a torus with no hole is the sphere of its minor radius
        let ball = Analytic::Sphere {
            centre: o,
            radius: 1.0,
        };
        assert!(same(
            torus(o, v(0.0, 0.0, 1.0), 0.0, 1.0),
            ball,
            1e-9,
            &region
        ));
        assert!(!same(t, ball, 1e-3, &region));
    }

    #[test]
    fn surfaces_of_two_kinds_are_never_one() {
        let region = cube(v(0.0, 0.0, 0.0), 1.0);
        let ball = Analytic::Sphere {
            centre: v(0.0, 0.0, 0.0),
            radius: 1.0,
        };
        let flat = plane(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0));
        let round = cylinder(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), 1.0);
        let ring = torus(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), 3.0, 1.0);
        for (a, b) in [
            (flat, ball),
            (flat, round),
            (round, ball),
            (round, ring),
            (flat, ring),
        ] {
            assert!(!same(a, b, 1.0, &region), "{a:?} {b:?}");
        }
    }

    #[test]
    fn bad_numbers_are_refused() {
        let region = cube(v(0.0, 0.0, 0.0), 1.0);
        let p = plane(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0));
        let zero = plane(v(0.0, 0.0, 0.0), v(0.0, 0.0, 0.0));
        let nan = Analytic::Sphere {
            centre: v(f64::NAN, 0.0, 0.0),
            radius: 1.0,
        };
        let inf = cylinder(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), f64::INFINITY);
        let flat_cone = cone(
            v(0.0, 0.0, 0.0),
            v(0.0, 0.0, 1.0),
            core::f64::consts::FRAC_PI_2,
        );
        let point = Extent3d::from_points(v(1.0, 1.0, 1.0), v(1.0, 1.0, 1.0));
        let unbounded = Extent3d::from_points(v(0.0, 0.0, 0.0), v(f64::INFINITY, 1.0, 1.0));
        for (a, b, tol, region) in [
            (p, zero, 1.0, &region),
            (nan, nan, 1.0, &region),
            (inf, inf, 1.0, &region),
            (flat_cone, flat_cone, 1.0, &region),
            (p, p, -1.0, &region),
            (p, p, f64::NAN, &region),
            (p, p, 1.0, &Extent3d::new()),
            (p, p, 1.0, &point),
            (p, p, 1.0, &unbounded),
        ] {
            assert!(
                matches!(
                    same_surface(&a, &b, tol, region),
                    Err(SolidError::InvalidInput { .. })
                ),
                "{a:?} {b:?} {tol} {region:?}"
            );
        }
    }
}
