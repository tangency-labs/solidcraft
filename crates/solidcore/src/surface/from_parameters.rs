//! A surface made from a record's numbers and a parameter box: the one constructor that
//! turns the plain data of a surface (a plane's origin and axes, a cone's frame, radius and
//! half angle, a spline's knots and points) and the box of its parameters it is wanted over
//! into the kernel's [`Surface`], for every layer that holds records (the conversion of a
//! body's faces, an intersection's tools).
//!
//! Not upstream: SolidCraft's composition (its SSI tool constructors and their record
//! entry), moved into the kernel so the record, conversion and modelling layers compute no
//! geometry (docs/layers.md §1). Every geometric step of it is here: angles from the record's
//! radians to the kernel's degrees, a plane's parameters scaled by its axes' lengths, a
//! cone's apex and its radii `h tan(half angle)` along the axis, the torus's tube angle from
//! its start, and a rational spline's homogeneous points divided by their weights.
//!
//! **Exact classes at bound 0** (owner rule 13564): a plane, cylinder, cone, sphere or torus
//! is made as that analytic kind, and is returned only once its NURBS form is proved to lie
//! on the implicit surface of the record's own numbers ([`lies_on`], to the rounding of
//! checking it), so the kernel's surface is the record's, not one near it. A spline is its
//! own definition.
//!
//! The parametrisation is the kernel's: a plane runs along its axes made unit with unit
//! scales (its parameters the record's times the axes' lengths), a revolution in degrees.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use crate::error::{Result, SolidError};
use crate::eval::{Frame, Surface as Record};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::Vec2;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::nurbs::b_spline_curve::KnotType;
use crate::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData};
use crate::nurbs::math::gamma;
use crate::nurbs::math::CPoint;
use crate::surface::cone::Cone;
use crate::surface::implicit::{lies_on, Implicit};
use crate::surface::plane::Plane;
use crate::surface::sphere::{Sphere, SphereOptions};
use crate::surface::surface::Surface;
use crate::surface::torus::{Torus, TorusOptions};

const OP: &str = "surface::from_parameters::from_parameters";

/// The surface the record's numbers `record` describe, over the box `domain`
/// (`[[u0, u1], [v0, v1]]` in the record's parameters: radians for angles), and that box in
/// the kernel's parameters (the face's uv domain). A spline's parameters are the kernel's:
/// the surface is its whole knot domain and the box is returned as given.
///
/// - **Plane:** the kernel's plane with unit axes along the record's, unit scales, over the
///   box scaled by the axes' lengths. The axes must be square to rounding: the kernel's
///   plane cannot hold a skew parametrisation.
/// - **Cylinder:** about the frame's z through the origin moved by `v0` along it, over the
///   angles in degrees and the height `v1 - v0`.
/// - **Cone:** from its apex (where the radius is 0, `-radius_at_origin / tan` along z),
///   between the heights from it `max(v0 - apex, 0)` and `v1 - apex` (the record's cone is the
///   nappe where the radius is not negative), radii `h tan`, over the angles in degrees.
/// - **Sphere, torus:** over the angles in degrees, the torus's tube angle from its start.
///
/// # Errors
///
/// `InvalidInput` for numbers not finite, axes not square or of no length, a box not
/// increasing, a cone of half angle not in `(0, pi/2)` or with its box past the apex, or a
/// result whose NURBS form is not proved on the record's surface; `Unsupported` for a ball
/// patch (its label has no parameters: its face's NURBS form is the surface) and a placed
/// revolution; those of the kind's constructor and of [`lies_on`].
pub fn from_parameters(
    k: &Kernel,
    record: &Record,
    domain: [[f64; 2]; 2],
) -> Result<(Surface, Extent2d)> {
    let [u, v] = domain;
    let bad = |reason: &'static str| SolidError::invalid_input(OP, reason);
    let finite = |xs: &[f64]| xs.iter().all(|x| x.is_finite());
    let analytic = !matches!(
        record,
        Record::Spline { .. } | Record::RationalSpline { .. }
    );
    if analytic && !(finite(&[u[0], u[1], v[0], v[1]]) && u[0] < u[1] && v[0] < v[1]) {
        return Err(bad("the box is not finite and increasing"));
    }
    let deg = |r: [f64; 2]| [r[0].to_degrees(), r[1].to_degrees()];
    let p3 = |a: [f64; 3]| Point3::new(a[0], a[1], a[2]);
    let (surface, implicit) = match record {
        Record::Plane {
            origin,
            u_axis,
            v_axis,
        } => plane(p3(*origin), p3(*u_axis), p3(*v_axis), u, v)?,
        Record::PlaneThrough {
            origin,
            u_point,
            v_point,
        } => {
            let o = p3(*origin);
            plane(o, p3(*u_point) - o, p3(*v_point) - o, u, v)?
        }
        Record::Cylinder { frame, radius } => {
            let (fo, fx, fy, z) = axes(frame)?;
            if !(radius.is_finite() && *radius > 0.0) {
                return Err(bad("the radius is not finite and positive"));
            }
            let a = deg(u);
            let o = fo + z * v[0];
            let c = Cone::new_cylinder_bounded(
                o,
                fx,
                fy,
                *radius,
                a[0],
                a[1],
                v[1] - v[0],
                false,
                false,
                false,
            );
            let i = Implicit::Cone {
                origin: fo,
                axis: z,
                radius: *radius,
                tan_angle: 0.0,
            };
            (Surface::from(c), i)
        }
        Record::Cone {
            frame,
            radius_at_origin,
            half_angle,
        } => {
            let (fo, fx, fy, z) = axes(frame)?;
            if !(finite(&[*radius_at_origin, *half_angle])
                && *half_angle > 0.0
                && *half_angle < core::f64::consts::FRAC_PI_2)
            {
                return Err(bad("the half angle is not in (0, pi/2)"));
            }
            let t = half_angle.tan();
            // the apex where the radius is 0, heights from it along z
            let at = -radius_at_origin / t;
            let (h0, h1) = ((v[0] - at).max(0.0), v[1] - at);
            if h1 <= h0 {
                return Err(bad("the box lies past the apex"));
            }
            let apex = fo + z * at;
            let pl = Axis2Placement::new(apex + z * h0, fx, fy);
            let mut c = Cone::create_canonical(k, &pl, h0 * t, h1 * t, h1 - h0, false, false)?;
            let a = deg(u);
            if a[1] - a[0] < 360.0 {
                c.adjust_step_uv_domain(k, &Extent2d::from_bounds(a[0], 0.0, a[1], h1 - h0))?;
            }
            let i = Implicit::Cone {
                origin: fo,
                axis: z,
                radius: *radius_at_origin,
                tan_angle: t,
            };
            (Surface::from(c), i)
        }
        Record::Sphere { frame, radius } => {
            let (fo, fx, fy, _) = axes(frame)?;
            let (a, b) = (deg(u), deg(v));
            let anal = Extent2d::from_bounds(a[0], b[0], a[1], b[1]);
            let opts = SphereOptions {
                swap_uv: false,
                inside_out: false,
                circle_nurb: None,
            };
            let s = Sphere::new(k, fo, fx, fy, anal, *radius, opts)?;
            let i = Implicit::Sphere {
                centre: fo,
                radius: *radius,
            };
            (Surface::from(s), i)
        }
        Record::Torus {
            frame,
            major_radius,
            minor_radius,
            tube_start,
        } => {
            let (fo, fx, fy, z) = axes(frame)?;
            let (a, b) = (deg(u), deg([v[0] + tube_start, v[1] + tube_start]));
            let anal = Extent2d::from_bounds(a[0], b[0], a[1], b[1]);
            let t = Torus::new(
                k,
                fo,
                fx,
                fy,
                anal,
                *major_radius,
                *minor_radius,
                TorusOptions::default(),
            )?;
            let i = Implicit::Torus {
                centre: fo,
                axis: z,
                major: *major_radius,
                minor: *minor_radius,
            };
            (Surface::from(t), i)
        }
        Record::Spline {
            u_knots,
            v_knots,
            points,
        } => {
            let rows: Vec<Vec<[f64; 4]>> = points
                .iter()
                .map(|r| r.iter().map(|&[x, y, z]| [x, y, z, 1.0]).collect())
                .collect();
            let s = spline(u_knots, v_knots, &rows)?;
            return Ok((s, given(u, v)));
        }
        Record::RationalSpline {
            u_knots,
            v_knots,
            homogeneous,
        } => {
            let s = spline(u_knots, v_knots, homogeneous)?;
            return Ok((s, given(u, v)));
        }
        Record::BallPatch { .. } | Record::PlacedRevolution { .. } => {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "a ball patch or placed revolution is made from its face's form",
            });
        }
    };
    let form = crate::blend::fillet_section::station::as_b_spline(k, &surface)?
        .ok_or_else(|| bad("the surface has no NURBS form"))?;
    if !lies_on(&form, &implicit)? {
        return Err(bad("the surface is off the record's numbers"));
    }
    let box_ = surface.natural_uv_domain_with_nurb(k)?;
    Ok((surface, box_))
}

/// The box `u x v` as given.
fn given(u: [f64; 2], v: [f64; 2]) -> Extent2d {
    Extent2d::from_bounds(u[0], v[0], u[1], v[1])
}

/// A frame's origin, x, y and z axes, its x and y checked unit and square to rounding.
fn axes(f: &Frame) -> Result<(Point3, Vec3, Vec3, Vec3)> {
    let ulps = 8.0 * f64::EPSILON;
    let v = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
    let (x, y) = (v(f.x_axis), v(f.y_axis));
    let unit = |v: Vec3| (v.length() - 1.0).abs() <= ulps;
    if !(unit(x) && unit(y) && x.dot(y).abs() <= ulps) {
        return Err(SolidError::invalid_input(
            OP,
            "the frame's axes are not unit and square",
        ));
    }
    Ok((v(f.origin), x, y, x.cross(y)))
}

/// The plane `origin + u du + v dv` over `u x v`: unit axes along `du`, `dv`, unit scales,
/// the box scaled by their lengths ([`axis_scale`]); and its implicit surface.
fn plane(
    origin: Point3,
    du: Vec3,
    dv: Vec3,
    u: [f64; 2],
    v: [f64; 2],
) -> Result<(Surface, Implicit)> {
    let (a, b) = (du.length(), dv.length());
    if !(a.is_finite() && b.is_finite() && a > 0.0 && b > 0.0) {
        return Err(SolidError::invalid_input(OP, "a plane axis has no length"));
    }
    // square to the rounding of the dot product
    if du.dot(dv).abs() > gamma(3) * a * b {
        return Err(SolidError::invalid_input(
            OP,
            "the plane's axes are not square: the kernel's plane cannot hold a skew parametrisation",
        ));
    }
    let (a, b) = (axis_scale(du), axis_scale(dv));
    let dom = Extent2d::from_bounds(u[0] * a, v[0] * b, u[1] * a, v[1] * b);
    let p = Plane::new(origin, du, dv, Vec2::new(1.0, 1.0), &dom);
    let normal = p.position().x_axis().cross(p.position().y_axis());
    Ok((Surface::from(p), Implicit::Plane { origin, normal }))
}

/// How far a plane's parameter runs along its unit axis for a unit of the record's along
/// `axis`: the axis's length, except that an axis unit to the rounding the kernel's placement
/// keeps it as it is (`SetCanonical`) is the placement's axis itself, so the scale is 1 and
/// the record's parameters are the kernel's exactly.
#[must_use]
pub fn axis_scale(axis: Vec3) -> f64 {
    let kept = Axis2Placement::new(Point3::new(0.0, 0.0, 0.0), axis, Vec3::new(0.0, 0.0, 0.0));
    if kept.x_axis() == axis {
        1.0
    } else {
        axis.length()
    }
}

/// A spline from its knots and homogeneous rows `(w x, w y, w z, w)` (`w = 1` for a
/// polynomial one), as the kernel's: filled directly when both knot vectors are clamped, else
/// clamped to its domain by the kernel's component constructor.
fn spline(u: &[f64], v: &[f64], rows: &[Vec<[f64; 4]>]) -> Result<Surface> {
    let bad = |reason: &'static str| SolidError::invalid_input(OP, reason);
    let (nu, nv) = (rows.len(), rows.first().map_or(0, Vec::len));
    if nu == 0 || nv == 0 || rows.iter().any(|r| r.len() != nv) {
        return Err(bad("the spline's net is empty or ragged"));
    }
    let p = u
        .len()
        .checked_sub(nu + 1)
        .ok_or_else(|| bad("too few u knots"))?;
    let q = v
        .len()
        .checked_sub(nv + 1)
        .ok_or_else(|| bad("too few v knots"))?;
    if rows.iter().flatten().any(|&[x, y, z, w]| {
        !(x.is_finite() && y.is_finite() && z.is_finite() && w.is_finite() && w > 0.0)
    }) {
        return Err(bad("a point is not finite or a weight not positive"));
    }
    let clamped = |k: &[f64], d: usize| {
        let (Some(a), Some(b)) = (k.first(), k.get(d)) else {
            return false;
        };
        let (Some(c), Some(e)) = (k.last(), k.len().checked_sub(d + 1).and_then(|i| k.get(i)))
        else {
            return false;
        };
        a == b && c == e
    };
    if clamped(u, p) && clamped(v, q) {
        let pw: Vec<Vec<CPoint>> = rows
            .iter()
            .map(|r| {
                r.iter()
                    .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                    .collect()
            })
            .collect();
        let (r, s) = (u.len() - 1, v.len() - 1); // u.len() > nu + 1 > 1: checked above
        return Ok(Surface::from(BSplineSurface::from_data(
            nu - 1, // nu > 0: checked above
            nv - 1, // nv > 0: checked above
            p,
            q,
            r,
            s,
            &pw,
            u,
            v,
        )?));
    }
    let unique = |k: &[f64]| {
        let (mut values, mut mults): (Vec<f64>, Vec<usize>) = (Vec::new(), Vec::new());
        for &x in k {
            match (values.last(), mults.last_mut()) {
                (Some(&last), Some(m)) if last == x => *m += 1,
                _ => {
                    values.push(x);
                    mults.push(1);
                }
            }
        }
        (values, mults)
    };
    let (ku, mu) = unique(u);
    let (kv, mv) = unique(v);
    let pts: Vec<Point3> = rows
        .iter()
        .flatten()
        .map(|&[x, y, z, w]| Point3::new(x / w, y / w, z / w))
        .collect();
    let wts: Vec<f64> = rows.iter().flatten().map(|&[.., w]| w).collect();
    let data = CanonicalSurfaceData {
        u_degree: p,
        v_degree: q,
        control_points: &pts,
        form: BSplineSurfaceForm::Unspecified,
        u_multiplicities: &mu,
        v_multiplicities: &mv,
        u_knots: &ku,
        v_knots: &kv,
        knot_type: KnotType::Unspecified,
        weights: Some(&wts),
        uv_domain: None,
    };
    Ok(Surface::from(BSplineSurface::create_canonical(&data)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::vector2d::Point2;
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    fn k() -> Kernel {
        Kernel::default()
    }

    fn frame(o: [f64; 3]) -> Frame {
        Frame {
            origin: o,
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
        }
    }

    fn a(v: Vec3) -> [f64; 3] {
        [v.x, v.y, v.z]
    }

    /// The points at the corners of the surface's natural box.
    fn corners(s: &Surface) -> Vec<Vec3> {
        let k = k();
        let d = s.natural_uv_domain_with_nurb(&k).unwrap();
        let (lo, hi) = (d.min(), d.max());
        [lo, Point2::new(hi.x, lo.y), hi, Point2::new(lo.x, hi.y)]
            .into_iter()
            .map(|uv| s.evaluate_point(&k, uv).unwrap())
            .collect()
    }

    /// Every point of `got` is one of `want`, and every one of `want` is met.
    fn same_set(got: &[Vec3], want: &[[f64; 3]]) {
        let want: Vec<Vec3> = want.iter().map(|w| Vec3::new(w[0], w[1], w[2])).collect();
        let near = |a: &Vec3, set: &[Vec3]| set.iter().any(|b| a.distance(*b) < 1e-12);
        assert!(got.iter().all(|g| near(g, &want)), "{got:?} vs {want:?}");
        assert!(want.iter().all(|w| near(w, got)), "{got:?} vs {want:?}");
    }

    fn make(p: &Record, u: [f64; 2], v: [f64; 2]) -> Result<Surface> {
        from_parameters(&k(), p, [u, v]).map(|(s, _)| s)
    }

    #[test]
    fn a_plane_runs_along_its_axes_made_unit() {
        let p = Record::Plane {
            origin: a(Point3::new(1.0, 2.0, 3.0)),
            u_axis: a(Vec3::new(2.0, 0.0, 0.0)),
            v_axis: a(Vec3::new(0.0, 0.0, 3.0)),
        };
        let s = make(&p, [0.0, 1.0], [-1.0, 1.0]).unwrap();
        let d = s.natural_uv_domain_with_nurb(&k()).unwrap();
        assert_eq!(
            (d.min().x, d.max().x, d.min().y, d.max().y),
            (0.0, 2.0, -3.0, 3.0)
        );
        same_set(
            &corners(&s),
            &[
                [1.0, 2.0, 0.0],
                [3.0, 2.0, 0.0],
                [3.0, 2.0, 6.0],
                [1.0, 2.0, 6.0],
            ],
        );
        let through = Record::PlaneThrough {
            origin: a(Point3::new(1.0, 2.0, 3.0)),
            u_point: a(Point3::new(3.0, 2.0, 3.0)),
            v_point: a(Point3::new(1.0, 2.0, 6.0)),
        };
        same_set(
            &corners(&make(&through, [0.0, 1.0], [-1.0, 1.0]).unwrap()),
            &[
                [1.0, 2.0, 0.0],
                [3.0, 2.0, 0.0],
                [3.0, 2.0, 6.0],
                [1.0, 2.0, 6.0],
            ],
        );
        let skew = Record::Plane {
            origin: a(Point3::ZERO),
            u_axis: a(Vec3::X),
            v_axis: a(Vec3::new(1.0, 1.0, 0.0)),
        };
        assert!(make(&skew, [0.0, 1.0], [0.0, 1.0]).is_err());
    }

    #[test]
    fn a_cylinder_cone_sphere_and_torus_meet_their_closed_forms() {
        let cyl = Record::Cylinder {
            frame: frame([1.0, 0.0, 0.0]),
            radius: 2.0,
        };
        same_set(
            &corners(&make(&cyl, [0.0, FRAC_PI_2], [1.0, 4.0]).unwrap()),
            &[
                [3.0, 0.0, 1.0],
                [1.0, 2.0, 1.0],
                [1.0, 2.0, 4.0],
                [3.0, 0.0, 4.0],
            ],
        );
        // radius 1 at the origin, 2 at height 3
        let cone = Record::Cone {
            frame: frame([0.0, 0.0, 0.0]),
            radius_at_origin: 1.0,
            half_angle: (1.0f64 / 3.0).atan(),
        };
        same_set(
            &corners(&make(&cone, [0.0, TAU], [0.0, 3.0]).unwrap()),
            &[[1.0, 0.0, 0.0], [2.0, 0.0, 3.0]],
        );
        same_set(
            &corners(&make(&cone, [0.0, FRAC_PI_2], [-1.5, 3.0]).unwrap()),
            &[
                [0.5, 0.0, -1.5],
                [0.0, 0.5, -1.5],
                [0.0, 2.0, 3.0],
                [2.0, 0.0, 3.0],
            ],
        );
        // the nappe below the apex (at -3) is not the record's
        same_set(
            &corners(&make(&cone, [0.0, FRAC_PI_2], [-5.0, 0.0]).unwrap()),
            &[[0.0, 0.0, -3.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        );
        assert!(make(&cone, [0.0, PI], [-9.0, -4.0]).is_err());
        let sph = Record::Sphere {
            frame: frame([0.0, 1.0, 0.0]),
            radius: 2.0,
        };
        same_set(
            &corners(&make(&sph, [0.0, FRAC_PI_2], [0.0, FRAC_PI_2]).unwrap()),
            &[[2.0, 1.0, 0.0], [0.0, 3.0, 0.0], [0.0, 1.0, 2.0]],
        );
        let tor = Record::Torus {
            frame: frame([0.0, 0.0, 0.0]),
            major_radius: 5.0,
            minor_radius: 1.0,
            tube_start: 0.0,
        };
        same_set(
            &corners(&make(&tor, [0.0, FRAC_PI_2], [0.0, FRAC_PI_2]).unwrap()),
            &[
                [6.0, 0.0, 0.0],
                [0.0, 6.0, 0.0],
                [0.0, 5.0, 1.0],
                [5.0, 0.0, 1.0],
            ],
        );
    }

    #[test]
    fn splines_polynomial_and_rational() {
        let p = Record::Spline {
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 2.0, 2.0],
            points: vec![
                vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                vec![[1.0, 0.0, 0.5], [1.0, 1.0, 1.0]],
            ],
        };
        same_set(
            &corners(&make(&p, [0.0, 0.0], [0.0, 0.0]).unwrap()),
            &[
                [0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 0.0, 0.5],
                [1.0, 1.0, 1.0],
            ],
        );
        // a quarter circle of radius 2 in u, a line in v
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let r = Record::RationalSpline {
            u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            homogeneous: vec![
                vec![[2.0, 0.0, 0.0, 1.0], [2.0, 0.0, 1.0, 1.0]],
                vec![[2.0 * h, 2.0 * h, 0.0, h], [2.0 * h, 2.0 * h, h, h]],
                vec![[0.0, 2.0, 0.0, 1.0], [0.0, 2.0, 1.0, 1.0]],
            ],
        };
        let s = make(&r, [0.0, 0.0], [0.0, 0.0]).unwrap();
        let mid = s.evaluate_point(&k(), Point2::new(0.5, 0.5)).unwrap();
        assert!((mid.x.hypot(mid.y) - 2.0).abs() < 1e-14 && (mid.z - 0.5).abs() < 1e-14);
        let bad = Record::RationalSpline {
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            homogeneous: vec![vec![[0.0; 4]; 2]; 2],
        };
        assert!(make(&bad, [0.0, 0.0], [0.0, 0.0]).is_err());
    }

    #[test]
    fn bad_numbers_are_refused() {
        let skewed = Frame {
            origin: a(Point3::ZERO),
            x_axis: a(Vec3::X),
            y_axis: a(Vec3::new(1e-6, 1.0, 0.0)),
        };
        let cyl = Record::Cylinder {
            frame: skewed,
            radius: 1.0,
        };
        assert!(make(&cyl, [0.0, 1.0], [0.0, 1.0]).is_err());
        let cyl = Record::Cylinder {
            frame: frame([0.0; 3]),
            radius: 1.0,
        };
        assert!(make(&cyl, [1.0, 0.0], [0.0, 1.0]).is_err());
        assert!(make(&cyl, [0.0, f64::NAN], [0.0, 1.0]).is_err());
        let flat = Record::Cone {
            frame: frame([0.0; 3]),
            radius_at_origin: 1.0,
            half_angle: 0.0,
        };
        assert!(make(&flat, [0.0, 1.0], [0.0, 1.0]).is_err());
    }
}
