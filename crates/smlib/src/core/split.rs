//! Split Face and Split Body: the C API's `smlib_split_faces`, `smlib_split_body` and
//! `smlib_split_body_by_sheet` (the fork's `capi/src/smlib_split.cpp`), each solidcore-ops' own
//! ([`solidcore_ops::split`]) with its tool as the record surface its description makes
//! ([`ops_tool`]), and the tool surfaces of `capi/src/smlib_ssi.cpp` (`smlib_surface_plane`,
//! `_cylinder`, `_cone`, `_sphere`, `_torus`, `_nurbs`) as solidcore holds them for the
//! surface/surface intersection ([`tool`]).

use solidcore::Result as SolidResult;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::p1::KnotType;
use solidcore::nurbs::b_spline_surface::p1::{BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData};
use solidcore::nurbs::math::CPoint;
use solidcore::surface::cone::p1::Cone;
use solidcore::surface::plane::p1::Plane;
use solidcore::surface::sphere::{Sphere, SphereOptions};
use solidcore::surface::surface::Surface;
use solidcore::surface::torus::{Torus, TorusOptions};

use super::Body;
use super::call::ops_as;
use crate::ssi::Spec;
use crate::{Error, Result};

/// `SMLIB_SPLIT_MAX_FACES`, `SMLIB_SPLIT_MAX_TOOLS`.
pub(crate) const MAX_FACES: usize = 100_000;
pub(crate) const MAX_TOOLS: usize = 4096;

/// The capi's statuses as SolidCraft errors: an operation's own result.
type Out<T> = std::result::Result<T, Error>;

/// A tool surface as solidcore holds it, for the surface/surface intersection ([`super::ssi`]):
/// the surface over its natural UV domain (`smlib_surface`'s `s` and `domain`).
pub(crate) struct Tool {
    pub(super) surface: Surface,
    pub(super) domain: Extent2d,
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// The runner's `unit_cross`.
fn unit_cross(a: Vec3, b: Vec3) -> Vec3 {
    let c = a.cross(b);
    let l = c.length();
    Vec3::new(c.x / l, c.y / l, c.z / l)
}

/// `[k0, k0, k1, ...]` as unique values and their multiplicities (`unique_knots`).
fn unique_knots(k: &[f64]) -> (Vec<f64>, Vec<usize>) {
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
}

/// `clamped`: the first and the last knot each p + 1 times.
fn clamped(k: &[f64], p: usize) -> bool {
    match (k.first(), k.get(p), k.last(), k.len().checked_sub(1 + p).and_then(|i| k.get(i))) {
        (Some(a), Some(b), Some(c), Some(d)) => a == b && c == d,
        _ => false,
    }
}

/// The tool's surface as `smlib_ssi.cpp`'s constructors make it, then its natural UV domain
/// (`construct`: finite and not empty, else `SM_ERR_INVALID_INPUT`).
pub(super) fn tool(k: &Kernel, spec: &Spec) -> SolidResult<Out<Tool>> {
    let surface = match spec {
        Spec::Plane { origin, xaxis, yaxis, u, v } => {
            let dom = Extent2d::from_bounds(u[0], v[0], u[1], v[1]);
            Surface::from(Plane::new(v3(*origin), v3(*xaxis), v3(*yaxis), Vec2::new(1.0, 1.0), &dom))
        }
        Spec::Cylinder { origin, xaxis, yaxis, radius, angle_deg, height } => {
            let (x, y) = (v3(*xaxis), v3(*yaxis));
            let o = v3(*origin) + unit_cross(x, y) * height[0];
            Surface::from(Cone::new_cylinder_bounded(o, x, y, *radius, angle_deg[0], angle_deg[1], height[1] - height[0], false, false, false))
        }
        Spec::Cone { apex, xaxis, yaxis, half_angle_deg, height } => {
            let (x, y) = (v3(*xaxis), v3(*yaxis));
            let (h0, h1) = (height[0], height[1]);
            let t = half_angle_deg.to_radians().tan();
            let o = v3(*apex) + unit_cross(x, y) * h0;
            let pl = Axis2Placement::new(o, x, y);
            Surface::from(Cone::create_canonical(k, &pl, h0 * t, h1 * t, h1 - h0, false, false)?)
        }
        Spec::Sphere { center, xaxis, yaxis, radius, lon_deg, lat_deg } => {
            let anal = Extent2d::from_bounds(lon_deg[0], lat_deg[0], lon_deg[1], lat_deg[1]);
            let opts = SphereOptions { swap_uv: false, inside_out: false, circle_nurb: None };
            Surface::from(Sphere::new(k, v3(*center), v3(*xaxis), v3(*yaxis), anal, *radius, opts)?)
        }
        Spec::Torus { center, xaxis, yaxis, major_radius, minor_radius, major_deg, minor_deg } => {
            let anal = Extent2d::from_bounds(major_deg[0], minor_deg[0], major_deg[1], minor_deg[1]);
            Surface::from(Torus::new(k, v3(*center), v3(*xaxis), v3(*yaxis), anal, *major_radius, *minor_radius, TorusOptions::default())?)
        }
        Spec::Nurbs { degree, nu, nv, knots_u, knots_v, points, weights } => {
            let [p, q] = *degree;
            let w = |i: usize| weights.as_ref().and_then(|w| w.get(i).copied()).unwrap_or(1.0);
            if !clamped(knots_u, p) || !clamped(knots_v, q) {
                // SMLib's component constructor clamps the surface to its domain.
                let (ku, mu) = unique_knots(knots_u);
                let (kv, mv) = unique_knots(knots_v);
                let wts: Vec<f64> = (0..points.len()).map(w).collect();
                let pts: Vec<Point3> = points.iter().map(|p| v3(*p)).collect();
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
                Surface::from(BSplineSurface::create_canonical(&data)?)
            } else {
                // The runner's way: the homogeneous net filled directly.
                let mut pw = Vec::with_capacity(*nu);
                for i in 0..*nu {
                    let mut row = Vec::with_capacity(*nv);
                    for j in 0..*nv {
                        let at = i * nv + j;
                        let (Some(pt), wt) = (points.get(at), w(at)) else {
                            return Ok(Err(Error::Argument));
                        };
                        row.push(CPoint::new(pt[0] * wt, pt[1] * wt, pt[2] * wt, wt));
                    }
                    pw.push(row);
                }
                let (n, m) = (nu.saturating_sub(1), nv.saturating_sub(1));
                let (r, s) = (knots_u.len().saturating_sub(1), knots_v.len().saturating_sub(1));
                Surface::from(BSplineSurface::from_data(n, m, p, q, r, s, &pw, knots_u, knots_v)?)
            }
        }
    };
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let (lo, hi) = (domain.min(), domain.max());
    if !(lo.x.is_finite() && lo.y.is_finite() && hi.x.is_finite() && hi.y.is_finite() && lo.x < hi.x && lo.y < hi.y) {
        return Ok(Err(Error::Status(1007)));
    }
    Ok(Ok(Tool { surface, domain }))
}

/// `spec` as solidcore-ops' split tool: the record surface its numbers describe and the box of
/// its parameters (radians for angles), which the kernel's `from_parameters` makes into the
/// surface `smlib_ssi.cpp`'s constructors make ([`tool`]). A cone's box is its whole turn; a
/// NURBS net's is its knot range, rows of `nv` points, weighted as homogeneous points.
fn ops_tool(spec: &Spec) -> solidcore_ops::split::Tool {
    use solidcore_record::brep::{Frame, Surface as R};
    let frame = |origin: [f64; 3], x_axis: [f64; 3], y_axis: [f64; 3]| Frame { origin, x_axis, y_axis };
    let rad = |d: [f64; 2]| [d[0].to_radians(), d[1].to_radians()];
    let (surface, domain) = match spec {
        Spec::Plane { origin, xaxis, yaxis, u, v } => (R::Plane { origin: *origin, u_axis: *xaxis, v_axis: *yaxis }, [*u, *v]),
        Spec::Cylinder { origin, xaxis, yaxis, radius, angle_deg, height } => {
            (R::Cylinder { frame: frame(*origin, *xaxis, *yaxis), radius: *radius }, [rad(*angle_deg), *height])
        }
        Spec::Cone { apex, xaxis, yaxis, half_angle_deg, height } => (
            R::Cone { frame: frame(*apex, *xaxis, *yaxis), radius_at_origin: 0.0, half_angle: half_angle_deg.to_radians() },
            [[0.0, std::f64::consts::TAU], *height],
        ),
        Spec::Sphere { center, xaxis, yaxis, radius, lon_deg, lat_deg } => {
            (R::Sphere { frame: frame(*center, *xaxis, *yaxis), radius: *radius }, [rad(*lon_deg), rad(*lat_deg)])
        }
        Spec::Torus { center, xaxis, yaxis, major_radius, minor_radius, major_deg, minor_deg } => (
            R::Torus { frame: frame(*center, *xaxis, *yaxis), major_radius: *major_radius, minor_radius: *minor_radius, tube_start: 0.0 },
            [rad(*major_deg), rad(*minor_deg)],
        ),
        Spec::Nurbs { nv, knots_u, knots_v, points, weights, .. } => {
            let range = |k: &[f64]| [k.first().copied().unwrap_or(0.0), k.last().copied().unwrap_or(0.0)];
            let domain = [range(knots_u), range(knots_v)];
            let rows = points.chunks((*nv).max(1));
            let surface = match weights {
                None => R::Spline { u_knots: knots_u.clone(), v_knots: knots_v.clone(), points: rows.map(<[_]>::to_vec).collect() },
                Some(w) => R::RationalSpline {
                    u_knots: knots_u.clone(),
                    v_knots: knots_v.clone(),
                    homogeneous: rows
                        .enumerate()
                        .map(|(i, row)| {
                            row.iter()
                                .enumerate()
                                .map(|(j, p)| {
                                    let wt = w.get(i * nv + j).copied().unwrap_or(1.0);
                                    [p[0] * wt, p[1] * wt, p[2] * wt, wt]
                                })
                                .collect()
                        })
                        .collect(),
                },
            };
            (surface, domain)
        }
    };
    solidcore_ops::split::Tool { surface, domain }
}

/// solidcore-ops' refusal of a split as the capi's: a tool that ends inside a face as
/// [`Error::ToolStopsShort`], the rest as solidcore-ops' blends and stitches are worded
/// ([`super::blend::refused`]).
fn split_error(call: &'static str, e: solidcore_ops::Error, detail: Option<String>) -> Error {
    match e {
        solidcore_ops::Error::ToolStopsShort => Error::ToolStopsShort,
        solidcore_ops::Error::NotSolid => Error::NotSolid,
        e => super::blend::refused(call, e, detail),
    }
}

/// `smlib_split_faces`: a copy of `body` with faces `faces` (indices in its face order, distinct)
/// cut where each tool meets them, the tools in turn, each cutting the pieces the earlier ones
/// left; and for each face how many faces it became besides itself
/// ([`solidcore_ops::split::split_faces`]). `None` when no face is split (`SMLIB_ERR_NO_CUT`), and
/// [`Error::ToolStopsShort`] when none is but a tool ran into a face and ended inside it.
pub(crate) fn split_faces(body: &Body, faces: &[usize], tools: &[&Spec]) -> Result<Option<(Body, Vec<usize>)>> {
    if faces.is_empty() || faces.len() > MAX_FACES || tools.is_empty() || tools.len() > MAX_TOOLS {
        return Err(Error::Argument);
    }
    if body.kernel_left_unfinished() {
        return Err(Error::Internal("smlib_split_faces: the body was left unfinished by an earlier panic".to_owned()));
    }
    let order = body.own_order();
    let tools: Vec<solidcore_ops::split::Tool> = tools.iter().map(|t| ops_tool(t)).collect();
    ops_as("smlib_split_faces", &[("brep", body)], split_error, || solidcore_ops::split::split_faces(body, &order, faces, &tools))
}

/// `smlib_split_body`: the manifold solid `body` cut in two by the tool: the part behind the
/// surface (its normal points away) and the part in front; a tool that closes on itself cuts it
/// into the part inside and the part outside ([`solidcore_ops::split::split_body`]). A side left
/// empty is `None` (`SMLIB_ERR_NO_CUT`), and so is a tool that ends inside the body; a body that
/// is no manifold solid is [`Error::NotSolid`].
pub(crate) fn split_body(body: &Body, spec: &Spec) -> Result<Option<(Body, Body)>> {
    if body.kernel_left_unfinished() {
        return Err(Error::Internal("smlib_split_body: the body was left unfinished by an earlier panic".to_owned()));
    }
    let tool = ops_tool(spec);
    ops_as("smlib_split_body", &[("brep", body)], split_error, || solidcore_ops::split::split_body(body, &tool))
}

/// `smlib_split_body_by_sheet`: the manifold solid `body` cut in two by `sheet` as it is, its
/// surfaces turned to agree with face `face`'s (its face order), for the back all turned over
/// ([`solidcore_ops::split::split_body_by_sheet`]). A sheet that does not divide the body is
/// `None` (`SMLIB_ERR_NO_CUT`). A `sheet` that is no sheet, or a `face` out of range, is
/// [`Error::Argument`].
pub(crate) fn split_body_by_sheet(body: &Body, sheet: &Body, face: usize) -> Result<Option<(Body, Body)>> {
    if std::ptr::eq(body, sheet) {
        return Err(Error::Argument);
    }
    if body.kernel_left_unfinished() || sheet.kernel_left_unfinished() {
        return Err(Error::Internal("smlib_split_body_by_sheet: a body was left unfinished by an earlier panic".to_owned()));
    }
    let order = sheet.own_order();
    ops_as("smlib_split_body_by_sheet", &[("brep", body), ("sheet", sheet)], split_error, || {
        solidcore_ops::split::split_body_by_sheet(body, sheet, &order, face)
    })
}
