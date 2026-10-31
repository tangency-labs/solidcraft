//! Split Face and Split Body on solidcore: the C API's composition, ported from the fork's
//! `capi/src/smlib_split.cpp` (`smlib_split_faces`, `smlib_split_body`, `cut_face`,
//! `plane_covers`, `copy_of`, `slice`, `both_sides`, `smlib_split_body_by_sheet`) and the tool surfaces of `capi/src/smlib_ssi.cpp`
//! (`smlib_surface_plane`, `_cylinder`, `_cone`, `_sphere`, `_torus`, `_nurbs`).
//!
//! As there, every operation works on copies of its body (`SmApiBrepCopy`). A face is cut where
//! a tool meets it the way SMLib's plane cutter cuts a subset of faces: the face's surface is
//! intersected with the tool over the face's UV domain (`CreatePlanarSectionCurves` for a plane
//! reaching over the face, `GlobalSurfaceIntersect` otherwise) and the untrimmed curves go to
//! `SmBrep::MergeCurvesOnSurface` with the tool as the intersecting surface. Unlike the plane
//! cutter, a failed merge fails the call. A body is split by slicing (`SM_BO_SLICE`) copies of it
//! with a one-face sheet on the tool surface, once with the surface turned over; or with a sheet
//! of several faces as it is, its surfaces first turned to agree.
//!
//! A tool is a [`crate::ssi::Surface`]; solidcore's own surface is made from its description
//! ([`Spec`]), as `smlib_ssi.cpp` makes SMLib's, over the surface's natural UV domain.

use std::sync::PoisonError;

use solidcore::EntityRef;
use solidcore::boolean::api_brep::boolean_with_options;
use solidcore::boolean::merge::MergeOptions;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::tol::{USE_DEFAULT, approx_tol3d};
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::p1::KnotType;
use solidcore::nurbs::b_spline_surface::p1::{BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData};
use solidcore::nurbs::math::CPoint;
use solidcore::query::brep::BrepFaceServices;
use solidcore::query::face::p1::calculate_bounding_box;
use solidcore::surface::cone::p1::Cone;
use solidcore::surface::plane::p1::Plane;
use solidcore::surface::sphere::{Sphere, SphereOptions};
use solidcore::surface::surface::Surface;
use solidcore::surface::surface::p2::TsectCurveType;
use solidcore::surface::torus::{Torus, TorusOptions};
use solidcore::topology::brep::p1 as brep_p1;
use solidcore::topology::brep::p2::{is_manifold_solid, is_sheet};
use solidcore::topology::brep::p3::{new_brep, with_kernel_callbacks};
use solidcore::topology::brep::p8::create_face_from_surface;
use solidcore::topology::brep::p9::orient_sheet_surfaces;
use solidcore::topology::brep::p10::merge_curves_on_surface_through_surface;
use solidcore::topology::brep::planar_section::face_planar_section_curves;
use solidcore::topology::brep_cache::CacheObject;
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::face::p2 as face_p2;
use solidcore::topology::{Brep, EdgeId, FaceId};

use super::blend::status;
use super::boolean::copy_of;
use super::sweep::build_faces;
use super::{Body, run};
use crate::ssi::Spec;
use crate::{Error, Result};

/// `SMLIB_SPLIT_MAX_FACES`, `SMLIB_SPLIT_MAX_TOOLS`.
pub(crate) const MAX_FACES: usize = crate::ffi::split::MAX_FACES;
pub(crate) const MAX_TOOLS: usize = crate::ffi::split::MAX_TOOLS;

/// `SM_ERR`.
const SM_ERR: i32 = 1001;

/// The capi's statuses as SolidCraft errors: an operation's own result.
type Out<T> = std::result::Result<T, Error>;

/// A tool surface as solidcore holds it: the surface over its natural UV domain (`smlib_surface`'s
/// `s` and `domain`), and for a plane its point and unit normal (`IsPlanar`).
struct Tool {
    surface: Surface,
    domain: Extent2d,
    plane: Option<(Point3, Vec3)>,
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
fn tool(k: &Kernel, spec: &Spec) -> solidcore::Result<Out<Tool>> {
    let mut plane = None;
    let surface = match spec {
        Spec::Plane { origin, xaxis, yaxis, u, v } => {
            let dom = Extent2d::from_bounds(u[0], v[0], u[1], v[1]);
            plane = Some((v3(*origin), unit_cross(v3(*xaxis), v3(*yaxis))));
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
    Ok(Ok(Tool { surface, domain, plane }))
}

/// `plane_covers`: whether the plane `tool` reaches over all of `bx` (each corner's foot on the
/// plane lies in the tool's parameter box): then its section with a face in the box is the
/// infinite plane's.
fn plane_covers(k: &Kernel, tool: &Tool, bx: &solidcore::geom::extent3d::Extent3d, tol: f64) -> bool {
    let (lo, hi) = (tool.domain.min(), tool.domain.max());
    let at = |u: f64, v: f64| tool.surface.evaluate_point(k, Vec2::new(u, v)).ok();
    let (Some(o), Some(pu), Some(pv)) = (at(lo.x, lo.y), at(hi.x, lo.y), at(lo.x, hi.y)) else {
        return false;
    };
    // The plane is affine in (u, v): P = o + (u - u0) X + (v - v0) Y.
    let x = (pu - o) / (hi.x - lo.x);
    let y = (pv - o) / (hi.y - lo.y);
    let (a, b, c) = (x.dot(x), x.dot(y), y.dot(y));
    let det = a * c - b * b;
    if !(det.abs() > 0.0) {
        return false;
    }
    let (m, mm) = (bx.min(), bx.max());
    (0..8).all(|kk| {
        let q = Vec3::new(if kk & 1 != 0 { mm.x } else { m.x }, if kk & 2 != 0 { mm.y } else { m.y }, if kk & 4 != 0 { mm.z } else { m.z });
        let r = q - o;
        let (g0, g1) = (x.dot(r), y.dot(r));
        let u = lo.x + (c * g0 - b * g1) / det;
        let v = lo.y + (a * g1 - b * g0) / det;
        let (su, sv) = (tol / a.sqrt(), tol / c.sqrt());
        u >= lo.x - su && u <= hi.x + su && v >= lo.y - sv && v <= hi.y + sv
    })
}

/// The name of each edge curve a merge makes (`SmApiCut`'s).
fn curve_ref(e: EdgeId, _type_code: i64) -> ObjectRef {
    solidcore::topology::loopuse::notify_ref(EntityRef::Edge(e))
}

/// `cut_face`: cuts `face` where `tool` meets it; the faces it was split into besides itself.
fn cut_face(k: &mut Kernel, b: &mut Brep, env: &mut NotifyEnv<'_>, face: FaceId, tool: &Tool, tol: f64) -> solidcore::Result<Out<Vec<FaceId>>> {
    let Some(surface) = face_p2::surface(b, face)? else {
        return Ok(Err(Error::Status(SM_ERR)));
    };
    let surface = surface.clone_for_evaluation()?;
    let domain = face_p2::uv_domain(b, face)?;
    let approx = approx_tol3d(k, approx_tol3d(k, tol) / 4.0);
    let bx = match calculate_bounding_box(k, &mut *b, face, false) {
        Ok(bx) => bx,
        Err(e) => return Ok(Err(status(e))),
    };
    let merge =
        |k: &mut Kernel, b: &mut Brep, env: &mut NotifyEnv<'_>, mut c3: Vec<_>, uv: Option<Vec<_>>| match merge_curves_on_surface_through_surface(
            k,
            b,
            env,
            &curve_ref,
            face,
            tol,
            &mut c3,
            uv.as_deref(),
            Some(tool.surface.clone()),
        ) {
            Ok((made, _)) => Ok(Ok(made)),
            Err(e) => Ok(Err(status(e))),
        };
    // A plane reaching over the face sections it exactly (SmPlaneCutter's way); a plane strip,
    // like any other tool, is intersected over its own domain.
    if let Some((at, normal)) = tool.plane
        && plane_covers(k, tool, &bx, tol)
    {
        // As SmPlaneCutter: a face far from the plane is not sectioned.
        if bx.distance_to_plane(at, normal).abs() > tol {
            return Ok(Ok(Vec::new()));
        }
        let section = match face_planar_section_curves(k, b, face, &surface, &domain, at, normal, Some(approx), None) {
            Ok(s) => s,
            Err(e) => return Ok(Err(status(e))),
        };
        if section.curves_3d.is_empty() {
            return Ok(Ok(Vec::new()));
        }
        let uv = (section.uv_curves.len() == section.curves_3d.len()).then_some(section.uv_curves);
        return merge(k, b, env, section.curves_3d, uv);
    }
    // `face->GetSurface()` reaches its face's trimmed cache: the solve runs through the B-rep's
    // face services, as solidcore's own face intersections do (a face-owned surface has no cache
    // of its own).
    let mut services = BrepFaceServices::new(&mut *b);
    let found = match surface.global_surface_intersect_with_context(
        k,
        [Some(CacheObject::Surface(face)), None],
        &mut services,
        &domain,
        &tool.surface,
        &tool.domain,
        [true, true],
        Some(approx),
        None,
    ) {
        Ok(f) => f,
        Err(e) => return Ok(Err(status(e))),
    };
    // A single point of contact cuts nothing.
    let (mut c3, mut uv1) = (Vec::new(), Vec::new());
    for (i, c) in found.curves_3d.into_iter().enumerate() {
        if found.curve_types.get(i) == Some(&TsectCurveType::Touching) {
            continue;
        }
        c3.push(c);
        uv1.push(found.surface1_uv_curves.get(i).cloned().flatten());
    }
    if c3.is_empty() {
        return Ok(Ok(Vec::new()));
    }
    let all_uv = uv1.iter().all(Option::is_some);
    merge(k, b, env, c3, all_uv.then_some(uv1))
}

/// `smlib_split_faces`: a copy of `body` with faces `faces` (indices in its face order, distinct)
/// cut where each tool meets them, the tools in turn, each cutting the pieces the earlier ones
/// left; and for each face how many faces it became besides itself. `None` when no face is split
/// (`SMLIB_ERR_NO_CUT`).
pub(crate) fn split_faces(body: &Body, faces: &[usize], tools: &[&Spec]) -> Result<Option<(Body, Vec<usize>)>> {
    if faces.is_empty() || faces.len() > MAX_FACES || tools.is_empty() || tools.len() > MAX_TOOLS {
        return Err(Error::Argument);
    }
    if body.0.is_poisoned() {
        return Err(Error::Internal("smlib_split_faces: the body was left unfinished by an earlier panic".to_owned()));
    }
    let r = run("smlib_split_faces", |k| {
        let mut a = {
            let g = body.0.lock().unwrap_or_else(PoisonError::into_inner);
            let count = brep_p1::faces(&g, None)?.len();
            let mut seen = std::collections::HashSet::new();
            if faces.iter().any(|&f| f >= count || !seen.insert(f)) {
                return Ok(Err(Error::Argument));
            }
            copy_of(k, &g, 1)?
        };
        let all = brep_p1::faces(&a, None)?;
        // Each chosen face and, as tools cut it, its pieces.
        let mut pieces: Vec<Vec<FaceId>> = Vec::with_capacity(faces.len());
        for &f in faces {
            let Some(&face) = all.get(f) else { return Ok(Err(Error::Status(SM_ERR))) };
            pieces.push(vec![face]);
        }
        let mut made = vec![0usize; faces.len()];
        let mut built = Vec::with_capacity(tools.len());
        for spec in tools {
            match tool(k, spec)? {
                Ok(t) => built.push(t),
                Err(e) => return Ok(Err(e)),
            }
        }
        let editing = a.editing_enabled();
        let doing = k.context.doing_boolean();
        a.set_editing_enabled(true);
        k.context.set_doing_boolean(true);
        let tol = a.tolerance();
        // The edits notify through the kernel's callbacks with the body's own attributes, so
        // the history tags follow each cut (as SMLib's objects notify their attributes).
        let cut = with_kernel_callbacks(k, &mut a, |k, a, env| {
            let mut total = 0usize;
            for t in &built {
                for (i, list) in pieces.iter_mut().enumerate() {
                    let now = list.clone();
                    for f in now {
                        let split = match cut_face(k, a, env, f, t, tol)? {
                            Ok(s) => s,
                            Err(e) => return Ok(Err(e)),
                        };
                        if let Some(m) = made.get_mut(i) {
                            *m += split.len();
                        }
                        total += split.len();
                        list.extend(split);
                    }
                }
            }
            Ok(Ok(total))
        });
        a.set_editing_enabled(editing);
        k.context.set_doing_boolean(doing);
        a.refresh_cached_topology();
        Ok(match cut? {
            Err(e) => Err(e),
            Ok(0) => Ok(None),
            Ok(_) => Ok(Some((a, made))),
        })
    })??;
    Ok(r.map(|(b, made)| (Body::new(b), made)))
}

/// `slice`: a copy of `body` sliced by `sheet` (SMLib's slice reads the side from the surfaces, not
/// the faces' uses). `None` when nothing is left.
fn slice(k: &mut Kernel, body: &Body, sheet: Brep) -> solidcore::Result<Out<Option<Brep>>> {
    let a = {
        let g = body.0.lock().unwrap_or_else(PoisonError::into_inner);
        copy_of(k, &g, 1)?
    };
    let options = MergeOptions::new(false, false, false, false, false);
    let r = match boolean_with_options(k, a, sheet, BooleanOperation::Slice, options) {
        Ok(r) => r.result,
        Err(e) => return Ok(Err(status(e))),
    };
    Ok(Ok((!r.face_list().is_empty()).then_some(r)))
}

/// `both_sides`: both parts, `side` making each side's sheet (`true`: turned over, for the back).
/// A body that is no manifold solid is [`Error::NotSolid`]; a side left empty is `None`
/// (`SMLIB_ERR_NO_CUT`), and with `whole` so is a part that is no manifold solid: `SM_BO_SLICE`
/// with a sheet that only nicks a region behaves as a difference, leaving the sheet's piece as a
/// slit in the body and a lamina on the other side, which is no cut.
fn both_sides(
    k: &mut Kernel,
    body: &Body,
    whole: bool,
    mut side: impl FnMut(&mut Kernel, bool) -> solidcore::Result<Out<Brep>>,
) -> solidcore::Result<Out<Option<(Brep, Brep)>>> {
    let solid = {
        let g = body.0.lock().unwrap_or_else(PoisonError::into_inner);
        is_manifold_solid(&g)?
    };
    if !solid {
        return Ok(Err(Error::NotSolid));
    }
    let mut parts = [None, None];
    for (part, flip) in parts.iter_mut().zip([true, false]) {
        let sheet = match side(k, flip)? {
            Ok(s) => s,
            Err(e) => return Ok(Err(e)),
        };
        *part = match slice(k, body, sheet)? {
            Ok(p) => p,
            Err(e) => return Ok(Err(e)),
        };
    }
    let [Some(back), Some(front)] = parts else { return Ok(Ok(None)) };
    if whole && !(is_manifold_solid(&back)? && is_manifold_solid(&front)?) {
        return Ok(Ok(None));
    }
    Ok(Ok(Some((back, front))))
}

/// One side of `smlib_split_body`: a one-face sheet on the tool's surface, turned over with `flip`.
fn side(k: &mut Kernel, body: &Body, spec: &Spec, flip: bool) -> solidcore::Result<Out<Brep>> {
    let tolerance = body.0.lock().unwrap_or_else(PoisonError::into_inner).tolerance();
    let t = match tool(k, spec)? {
        Ok(t) => t,
        Err(e) => return Ok(Err(e)),
    };
    let mut surface = t.surface.copy(k)?;
    if flip && let Err(e) = surface.swap_uv(k) {
        return Ok(Err(status(e)));
    }
    let mut sheet = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    sheet.set_tolerance(tolerance);
    let natural = surface.natural_uv_domain_with_nurb(k)?;
    if let Err(e) = build_faces(&mut sheet, |s, fenv| create_face_from_surface(k, s, Some(surface), &natural, fenv)) {
        return Ok(Err(status(e)));
    }
    sheet.refresh_cached_topology();
    Ok(Ok(sheet))
}

/// `smlib_split_body`: the manifold solid `body` cut in two by the tool: the part behind the
/// surface (its normal points away) and the part in front. A side left empty is `None`
/// (`SMLIB_ERR_NO_CUT`); a body that is no manifold solid is [`Error::NotSolid`].
pub(crate) fn split_body(body: &Body, spec: &Spec) -> Result<Option<(Body, Body)>> {
    if body.0.is_poisoned() {
        return Err(Error::Internal("smlib_split_body: the body was left unfinished by an earlier panic".to_owned()));
    }
    let r = run("smlib_split_body", |k| both_sides(k, body, false, |k, flip| side(k, body, spec, flip)))??;
    Ok(r.map(|(b, f)| (Body::new(b), Body::new(f))))
}

/// `smlib_split_body_by_sheet`: the manifold solid `body` cut in two by `sheet` as it is (one or
/// more faces joined edge to edge), its surfaces turned to agree with face `face`'s
/// (`SmBrep::OrientSheetSurfaces`), for the back all turned over: the part behind that normal and
/// the part in front. A sheet that does not divide the body is `None` (`SMLIB_ERR_NO_CUT`). A
/// `sheet` that is no sheet, or a `face` out of range, is [`Error::Argument`].
pub(crate) fn split_body_by_sheet(body: &Body, sheet: &Body, face: usize) -> Result<Option<(Body, Body)>> {
    if std::ptr::eq(body, sheet) {
        return Err(Error::Argument);
    }
    if body.0.is_poisoned() || sheet.0.is_poisoned() {
        return Err(Error::Internal("smlib_split_body_by_sheet: a body was left unfinished by an earlier panic".to_owned()));
    }
    let r = run("smlib_split_body_by_sheet", |k| {
        {
            let g = sheet.0.lock().unwrap_or_else(PoisonError::into_inner);
            if !is_sheet(k, &g)? || face >= g.face_list().len() {
                return Ok(Err(Error::Argument));
            }
        }
        both_sides(k, body, true, |k, flip| {
            let mut copy = {
                let g = sheet.0.lock().unwrap_or_else(PoisonError::into_inner);
                copy_of(k, &g, 2)?
            };
            let Some(&f) = brep_p1::faces(&copy, None)?.get(face) else { return Ok(Err(Error::Argument)) };
            let turned = orient_sheet_surfaces(k, &mut copy, Some(f), !flip);
            copy.refresh_cached_topology();
            Ok(match turned {
                Ok(()) => Ok(copy),
                Err(e) => Err(status(e)),
            })
        })
    })??;
    Ok(r.map(|(b, f)| (Body::new(b), Body::new(f))))
}
