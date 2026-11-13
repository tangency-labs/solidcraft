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
///   between the heights from it `v0 - apex` and `v1 - apex`, radii `h tan`, over the angles
///   in degrees. The record's cone runs through its apex into the other nappe (its radius
///   goes negative past it); the kernel's is one nappe, so a box reaching past the apex
///   (beyond the rounding of finding it) is refused, never clipped.
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
    // at most a turn, to the rounding of the difference
    let turn = |r: [f64; 2], what: &'static str| {
        let rounding = gamma(2) * (r[0].abs() + r[1].abs()).max(core::f64::consts::TAU);
        if r[1] - r[0] - core::f64::consts::TAU > rounding {
            Err(bad(what))
        } else {
            Ok(())
        }
    };
    // the record's parameters to the kernel's STEP ones, by kind
    let step_of: Box<dyn Fn([f64; 2]) -> [f64; 2]>;
    let p3 = |a: [f64; 3]| Point3::new(a[0], a[1], a[2]);
    let (surface, implicit) = match record {
        Record::Plane {
            origin,
            u_axis,
            v_axis,
        } => {
            let (sf, i, [la, lb]) = plane(p3(*origin), p3(*u_axis), p3(*v_axis), u, v)?;
            step_of = Box::new(move |[a, b]| [a * la, b * lb]);
            (sf, i)
        }
        Record::PlaneThrough {
            origin,
            u_point,
            v_point,
        } => {
            let o = p3(*origin);
            let (sf, i, [la, lb]) = plane(o, p3(*u_point) - o, p3(*v_point) - o, u, v)?;
            step_of = Box::new(move |[a, b]| [a * la, b * lb]);
            (sf, i)
        }
        Record::Cylinder { frame, radius } => {
            let (fo, fx, fy, z) = axes(frame)?;
            if !(radius.is_finite() && *radius > 0.0) {
                return Err(bad("the radius is not finite and positive"));
            }
            turn(u, "the sweep is over a turn")?;
            let v0 = v[0];
            step_of = Box::new(move |[a, b]| [a.to_degrees(), b - v0]);
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
            turn(u, "the sweep is over a turn")?;
            let t = half_angle.tan();
            // the apex where the radius is 0, heights from it along z. The record's cone runs
            // through it (its radius goes negative past it, the other nappe); the kernel's is
            // one nappe, so a box reaching past the apex (beyond the rounding of finding it)
            // is refused, never clipped
            let at = -radius_at_origin / t;
            let rounding = gamma(4) * (at.abs() + v[0].abs());
            if v[0] - at < -rounding {
                return Err(bad("the box reaches past the cone's apex"));
            }
            let (h0, h1) = ((v[0] - at).max(0.0), v[1] - at);
            if h1 <= h0 {
                return Err(bad("the box has no height above the apex"));
            }
            let v0 = v[0];
            step_of = Box::new(move |[a, b]| [a.to_degrees(), b - v0]);
            let apex = fo + z * at;
            let pl = Axis2Placement::new(apex + z * h0, fx, fy);
            let mut c = Cone::create_canonical(k, &pl, h0 * t, h1 * t, h1 - h0, false, false)?;
            let a = deg(u);
            // the sweep as asked, a full turn included: its seam where the record's is
            c.adjust_step_uv_domain(k, &Extent2d::from_bounds(a[0], 0.0, a[1], h1 - h0))?;
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
            turn(u, "the sweep is over a turn")?;
            let pole = core::f64::consts::FRAC_PI_2;
            let rounding = gamma(2) * pole;
            if v[0] < -pole - rounding || v[1] > pole + rounding {
                return Err(bad("the latitude is past a pole"));
            }
            step_of = Box::new(|[a, b]| [a.to_degrees(), b.to_degrees()]);
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
            turn(u, "the sweep is over a turn")?;
            turn(v, "the tube angle is over a turn")?;
            let ts = *tube_start;
            step_of = Box::new(move |[a, b]| [a.to_degrees(), (b + ts).to_degrees()]);
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
            in_knots(u, u_knots)?;
            in_knots(v, v_knots)?;
            return Ok((s, given(u, v)));
        }
        Record::RationalSpline {
            u_knots,
            v_knots,
            homogeneous,
        } => {
            let s = spline(u_knots, v_knots, homogeneous)?;
            in_knots(u, u_knots)?;
            in_knots(v, v_knots)?;
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
    // the domain: the kernel's STEP box is the record box's image
    let [a0, b0] = step_of([u[0], v[0]]);
    let [a1, b1] = step_of([u[1], v[1]]);
    let got = surface.step_uv_domain_with_nurb(k)?;
    let near = |x: f64, y: f64| (x - y).abs() <= gamma(8) * (x.abs() + y.abs());
    let (lo, hi) = (got.min(), got.max());
    if !(near(lo.x, a0.min(a1))
        && near(hi.x, a0.max(a1))
        && near(lo.y, b0.min(b1))
        && near(hi.y, b0.max(b1)))
    {
        return Err(bad("the kernel's domain is not the record's box"));
    }
    // the parametrisation: the record's points are the kernel's at the mapped parameters,
    // at the box's corners, its sides' middles and its middle
    evaluates_as_record(k, record, &surface, &form, u, v, &*step_of)?;
    let box_ = surface.natural_uv_domain_with_nurb(k)?;
    Ok((surface, box_))
}

/// The record's evaluator against the kernel's surface at the parameters `step_of` maps the
/// record's to (STEP, then the kernel's NURBS), on a 3 x 3 grid of `u x v` (its corners
/// included). The two agree within the rounding of evaluating them: the kernel's de Boor
/// evaluation of its form (`16 (p + q + 2)` operations on the net, as
/// [`crate::surface::implicit`]'s form checks) and the record's trigonometry (16 more), times
/// the larger of the net's and the points' size.
fn evaluates_as_record(
    k: &Kernel,
    record: &Record,
    surface: &Surface,
    form: &BSplineSurface,
    u: [f64; 2],
    v: [f64; 2],
    step_of: &dyn Fn([f64; 2]) -> [f64; 2],
) -> Result<()> {
    let bad = |reason: &'static str| SolidError::invalid_input(OP, reason);
    let eval =
        crate::eval::SurfaceEval::new(record).map_err(|_| bad("the record does not evaluate"))?;
    let net = form
        .gw_nurb()
        .ok_or_else(|| SolidError::surface_failed(OP, "the B-spline form has no NURB"))?;
    let (p, q) = net.degrees();
    let size = net.rows().flatten().fold(0.0_f64, |m, c| {
        let w = if c.w == crate::nurbs::math::NOW {
            1.0
        } else {
            c.w
        };
        m.max((c.x / w).abs())
            .max((c.y / w).abs())
            .max((c.z / w).abs())
    });
    let g = gamma(32 * (p + q + 3));
    for s in [0.0, 0.5, 1.0] {
        for t in [0.0, 0.5, 1.0] {
            let ru = u[0] + s * (u[1] - u[0]);
            let rv = v[0] + t * (v[1] - v[0]);
            let want = eval
                .point([ru, rv])
                .ok_or_else(|| bad("the record does not evaluate"))?;
            let [a, b] = step_of([ru, rv]);
            let uv = surface.convert_uv_from_step_to_nurbs(k, Vec2::new(a, b))?;
            let got = surface.evaluate_point(k, uv)?;
            let off = (got - Point3::new(want.x, want.y, want.z)).length();
            if off.is_nan() || off > g * size.max(want.length()) {
                return Err(bad(
                    "the kernel's surface is not the record's at its parameters",
                ));
            }
        }
    }
    Ok(())
}

/// A spline box `r` finite, increasing and within the knot domain of `knots`.
fn in_knots(r: [f64; 2], knots: &[f64]) -> Result<()> {
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(SolidError::invalid_input(OP, "the spline has no knots"));
    };
    if !(r[0].is_finite() && r[1].is_finite() && r[0] < r[1] && r[0] >= a && r[1] <= b) {
        return Err(SolidError::invalid_input(
            OP,
            "the box is not finite, increasing and within the knots",
        ));
    }
    Ok(())
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
) -> Result<(Surface, Implicit, [f64; 2])> {
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
    // the record's own normal, du x dv made unit
    let n = du.cross(dv);
    let normal = n / n.length();
    Ok((Surface::from(p), Implicit::Plane { origin, normal }, [a, b]))
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
    use crate::surface::implicit::AnalyticSupport;
    use crate::surface::side::BoxSide;
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
        // a box reaching past the apex (at -3) is refused, never clipped: the record's cone
        // runs on into the other nappe, the kernel's is one
        assert!(make(&cone, [0.0, FRAC_PI_2], [-5.0, 0.0]).is_err());
        assert!(make(&cone, [0.0, PI], [-9.0, -4.0]).is_err());
        // from the apex itself: built
        assert!(make(&cone, [0.0, FRAC_PI_2], [-3.0, 0.0]).is_ok());
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
    fn a_full_turn_keeps_its_seam_and_more_than_a_turn_is_refused() {
        let cone = Record::Cone {
            frame: frame([0.0, 0.0, 0.0]),
            radius_at_origin: 1.0,
            half_angle: (1.0f64 / 3.0).atan(),
        };
        let cyl = Record::Cylinder {
            frame: frame([0.0, 0.0, 0.0]),
            radius: 2.0,
        };
        for rec in [&cone, &cyl] {
            let s = make(rec, [FRAC_PI_2, FRAC_PI_2 + TAU], [0.0, 3.0]).unwrap();
            let d = s.step_uv_domain_with_nurb(&k()).unwrap();
            assert!(
                (d.min().x - 90.0).abs() < 1e-12 && (d.max().x - 450.0).abs() < 1e-12,
                "{d:?}"
            );
            assert!(make(rec, [0.0, TAU + 1e-9], [0.0, 3.0]).is_err());
        }
        let sph = Record::Sphere {
            frame: frame([0.0, 0.0, 0.0]),
            radius: 2.0,
        };
        assert!(make(&sph, [0.0, TAU + 1e-9], [0.0, 1.0]).is_err());
        assert!(make(&sph, [0.0, 1.0], [0.0, FRAC_PI_2 + 1e-9]).is_err());
    }

    #[test]
    fn interior_points_are_the_records_at_their_parameters() {
        // the cone's middle, off its corners: the record's evaluator against the kernel's
        let cone = Record::Cone {
            frame: frame([1.0, -2.0, 0.5]),
            radius_at_origin: 1.0,
            half_angle: (1.0f64 / 3.0).atan(),
        };
        let (u, v) = ([0.3, 2.0], [-1.0, 2.5]);
        let (s, _) = from_parameters(&k(), &cone, [u, v]).unwrap();
        let e = crate::eval::SurfaceEval::new(&cone).unwrap();
        let (ru, rv) = (1.1, 0.7);
        let want = e.point([ru, rv]).unwrap();
        let uv = s
            .convert_uv_from_step_to_nurbs(&k(), Vec2::new(ru.to_degrees(), rv - v[0]))
            .unwrap();
        let got = s.evaluate_point(&k(), uv).unwrap();
        assert!(got.distance(want) < 1e-12, "{got:?} vs {want:?}");
    }

    #[test]
    fn a_spline_box_outside_its_knots_is_refused() {
        let p = Record::Spline {
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 2.0, 2.0],
            points: vec![
                vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                vec![[1.0, 0.0, 0.5], [1.0, 1.0, 1.0]],
            ],
        };
        assert!(make(&p, [0.0, 1.5], [0.0, 2.0]).is_err());
        assert!(make(&p, [0.5, 0.5], [0.0, 2.0]).is_err());
        assert!(make(&p, [0.0, f64::NAN], [0.0, 2.0]).is_err());
        assert!(make(&p, [0.25, 0.75], [0.5, 2.0]).is_ok());
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
            &corners(&make(&p, [0.0, 1.0], [0.0, 2.0]).unwrap()),
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
        let s = make(&r, [0.0, 1.0], [0.0, 1.0]).unwrap();
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

    // TODO(#2554): once AnalyticSupport::build
    // takes a cone made by Cone::create_canonical, also round-trip the label through it.
    #[test]
    fn a_continued_cone_round_trips_through_its_record() {
        // a cone continued past a side, written as the record's numbers and made again:
        // the same surface, proved on the record's implicit surface
        let k = k();
        let pl = Axis2Placement::new(Vec3::new(1.0, -2.0, 0.5), Vec3::X, Vec3::Y);
        let mut c = Cone::create_canonical(&k, &pl, 1.0, 2.0, 3.0, false, false).unwrap();
        c.adjust_step_uv_domain(&k, &Extent2d::from_bounds(0.0, 0.0, 90.0, 3.0))
            .unwrap();
        let dom = Surface::from(c.clone())
            .natural_uv_domain_with_nurb(&k)
            .unwrap();
        let grown = Surface::from(c.continued(&k, &dom, BoxSide::VMax, 1.0).unwrap());
        let Some(AnalyticSupport::Cone {
            placement,
            base_radius,
            semi_angle_deg,
        }) = AnalyticSupport::of(&grown)
        else {
            panic!("not a cone");
        };
        let b = grown.step_uv_domain_with_nurb(&k).unwrap();
        let record = Record::Cone {
            frame: Frame {
                origin: a(placement.origin()),
                x_axis: a(placement.x_axis()),
                y_axis: a(placement.y_axis()),
            },
            radius_at_origin: base_radius,
            half_angle: semi_angle_deg.to_radians(),
        };
        let (back, _) = from_parameters(
            &k,
            &record,
            [
                [b.min().x.to_radians(), b.max().x.to_radians()],
                [b.min().y, b.max().y],
            ],
        )
        .unwrap();
        let want: Vec<[f64; 3]> = corners(&grown).iter().map(|p| [p.x, p.y, p.z]).collect();
        same_set(&corners(&back), &want);
        AnalyticSupport::check(&k, &back).unwrap();
    }

    /// The conversion failures keep the operation, status, kind and entity of the bare
    /// `Upstream` statuses they replace (a sample).
    #[test]
    fn typed_surface_failures_keep_their_upstream_answers() {
        use crate::error::surface::SurfaceFailure;
        let samples: [(&'static str, SurfaceFailure, i32); 1] = [(
            OP,
            SurfaceFailure::Failed("the B-spline form has no NURB"),
            crate::error::SM_ERR,
        )];
        for (operation, failure, code) in samples {
            let typed = SolidError::surface(operation, failure);
            let before = SolidError::Upstream {
                operation,
                code,
                entity: None,
            };
            assert_eq!(typed.sm_status(), before.sm_status(), "{typed:?}");
            assert_eq!(typed.upstream_status(), Some(code), "{typed:?}");
            assert_eq!(typed.kind(), before.kind(), "{typed:?}");
            assert_eq!(typed.operation(), before.operation(), "{typed:?}");
            assert_eq!(typed.entity(), None, "{typed:?}");
            assert_eq!(typed.is_port_failure(), before.is_port_failure());
            assert_eq!(typed.is_status_answer(), before.is_status_answer());
            assert_eq!(typed.surface_failure(), Some(&failure));
        }
    }
}
