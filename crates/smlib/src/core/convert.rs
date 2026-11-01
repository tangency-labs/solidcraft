//! solidcore's B-rep to and from the C API's flat arrays ([`crate::flat`]): ports of
//! `capi/src/smlib_flat.cpp`, so a body converts exactly as it does through the C++ kernel.
//! - [`to_arrays`]: `FlatFromBrep` (SmBrep, through `SmBrepData::FromBrep`, to arrays): the same
//!   ids, the same face, edge and vertex order (`GetFaces`, `GetEdges`), analytic kinds with
//!   ranges in radians, NURBS as splines, SMLib's own UV trims and tolerances.
//! - [`from_arrays`]: `BrepFromFlat` (`Validate`, `FlatToBrepData`, then
//!   `SmBrep::MakeTopologyFromData`, healing unless the arrays say SMLib wrote them).
//!
//! Tolerances differ from the C++ on purpose: `to_arrays` writes the B-rep's own tolerance as
//! TOLERANCE, not the largest of its entities', and `from_arrays` takes the B-rep's and the faces'
//! from the edges' and vertices' own ([`own_tolerances`]). Each entity keeps its tolerance across
//! the kernels instead of every one taking the largest at each crossing.
//!
//! A face whose surface is not a B-spline kind (an offset) exports its approximation, as
//! `FlatFromBrep` does (`SmSurface::ApproximateSurface`).
//!
//! [`from_arrays`] also runs the capi's `PutEnclosingShellsFirst` ([`put_enclosing_shells_first`],
//! an addition to SmuConvert): a bounded region of several shells starts its list at the shell
//! `SmShell::IsInnerShellOf` shows enclosing the others, whatever order the arrays gave.

// The SmBrepData records are filled field by field, as FlatToBrepData fills them after ReSet.
#![allow(clippy::field_reassign_with_default)]

use std::borrow::Cow;
use std::collections::HashMap;

use solidcore::curve::curve::p1::ApproximateCurveOptions;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::line::Line;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::tol::approx_tol3d;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData};
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::query::brep::{BoundingBoxOptions, calculate_bounding_box};
use solidcore::surface::cone::Cone;
use solidcore::surface::plane::Plane;
use solidcore::surface::sphere::Sphere;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::surface::torus::{Torus, TorusOptions};
use solidcore::topology::brep::p1 as bl;
use solidcore::topology::brep::p10::{HEALER_VERSION, MakeTopologyOptions, TopologyEnv, make_topology_from_data};
use solidcore::topology::brep_data::{
    BrepData, DataSourceType, EdgeData, EuData, FaceData, FaceuseData, Keyed, LoopData, NO_OBJECT, RegionData, ShellData, UNDEF_ULONG, UNSURE,
    VertexData,
};
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::edgeuse::{p1 as eu1, p3 as eu3};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{Brep, EdgeId, EdgeuseId, FaceId, ShellId, VertexId, edge, shell, vertex};
use solidcore::{EntityRef, Kernel, Point2, Point3, SolidError, Vec3};

use crate::flat::*;
use crate::import::Arrays;
use crate::{Error, Result};

use super::{Body, answer, run, with_body};

// ---- shared ------------------------------------------------------------------------------------

/// A solidcore error from `call`.
fn sc(call: &'static str) -> impl Fn(SolidError) -> Error {
    move |e| Error::Solidcore(format!("{call}: {e}"))
}

/// The arrays (or the body) are not what the conversion can carry: SMLib's `SM_ERR`.
fn bad(what: &str) -> Error {
    Error::Solidcore(format!("flat arrays: {what}"))
}

fn get<T: Copy>(v: &[T], i: usize) -> Result<T> {
    v.get(i).copied().ok_or_else(|| bad("index past its array"))
}

fn get3(v: &[f64], i: usize) -> Result<Vec3> {
    let at = |k: usize| i.checked_mul(3).and_then(|j| j.checked_add(k)).map_or_else(|| Err(bad("index past its array")), |j| get(v, j));
    Ok(Vec3::new(at(0)?, at(1)?, at(2)?))
}

fn idx(x: i64) -> Result<usize> {
    usize::try_from(x).map_err(|_| bad("negative index"))
}

fn count(n: usize) -> Result<i64> {
    i64::try_from(n).map_err(|_| bad("count"))
}

fn index_u64(x: u64) -> Result<usize> {
    usize::try_from(x).map_err(|_| bad("index"))
}

/// `SM_DEG2RAD`.
fn deg2rad(d: f64) -> f64 {
    d * (std::f64::consts::PI / 180.0)
}

/// `SM_RAD2DEG`.
fn rad2deg(r: f64) -> f64 {
    r * (180.0 / std::f64::consts::PI)
}

/// `SnapSMLibAngularBoundaryDeg`.
fn snap_angular_boundary_deg(degrees: f64) -> f64 {
    for quarter in -4..=4 {
        let boundary = 90.0 * f64::from(quarter);
        if (degrees - boundary).abs() <= 1.0e-9 {
            return boundary;
        }
    }
    degrees
}

/// `UsdRadiansToSMLibDegrees`.
fn radians_to_smlib_degrees(radians: f64) -> f64 {
    snap_angular_boundary_deg(rad2deg(radians))
}

/// `SM_INFINITE_PARAMETER`.
const INFINITE_PARAMETER: f64 = 1_234_567.0;
/// `SM_EFF_ZERO`.
const EFF_ZERO: f64 = 1.0e-12;
/// `SM_IS_INFINITE`.
fn is_infinite(a: f64) -> bool {
    a.abs() == INFINITE_PARAMETER || a.abs() == 1111.111
}

/// The flat arrays' writer: every array id, empty until written.
struct Out(Arrays);

impl Out {
    fn new() -> Out {
        let mut a = Arrays::default();
        for id in 1..=WIRE_EDGE_NURBS_FORM {
            a.ints.insert(id, Vec::new());
        }
        for id in TOLERANCE..=VERTEX_TOLERANCE {
            a.reals.insert(id, Vec::new());
        }
        Out(a)
    }
    fn i(&mut self, id: i32) -> &mut Vec<i64> {
        self.0.ints.entry(id).or_default()
    }
    fn r(&mut self, id: i32) -> &mut Vec<f64> {
        self.0.reals.entry(id).or_default()
    }
    fn r3(&mut self, id: i32, p: Vec3) {
        self.r(id).extend([p.x, p.y, p.z]);
    }
}

/// The arrays of one edge family (edges or wire edges), by id (`EdgeArraysOut`).
struct EdgeIds {
    ty: i32,
    range: i32,
    count: i32,
    order: i32,
    cvs: i32,
    knots: i32,
    weights: i32,
    circle_center: i32,
    circle_axis: i32,
    circle_ref: i32,
    circle_radius: i32,
    line_origin: i32,
    line_direction: i32,
    ellipse_center: i32,
    ellipse_axis: i32,
    ellipse_ref: i32,
    ellipse_x: i32,
    ellipse_y: i32,
    form: i32,
    vertices: i32,
}

const EDGES: EdgeIds = EdgeIds {
    ty: EDGE_CURVE_TYPE,
    range: EDGE_RANGE,
    count: EDGE_NURBS_VERTEX_COUNT,
    order: EDGE_NURBS_ORDER,
    cvs: EDGE_NURBS_CVS,
    knots: EDGE_NURBS_KNOTS,
    weights: EDGE_NURBS_WEIGHTS,
    circle_center: EDGE_CIRCLE_CENTER,
    circle_axis: EDGE_CIRCLE_AXIS,
    circle_ref: EDGE_CIRCLE_REF_DIRECTION,
    circle_radius: EDGE_CIRCLE_RADIUS,
    line_origin: EDGE_LINE_ORIGIN,
    line_direction: EDGE_LINE_DIRECTION,
    ellipse_center: EDGE_ELLIPSE_CENTER,
    ellipse_axis: EDGE_ELLIPSE_AXIS,
    ellipse_ref: EDGE_ELLIPSE_REF_DIRECTION,
    ellipse_x: EDGE_ELLIPSE_X_RADIUS,
    ellipse_y: EDGE_ELLIPSE_Y_RADIUS,
    form: EDGE_NURBS_FORM,
    vertices: EDGE_VERTICES,
};

const WIRE_EDGES: EdgeIds = EdgeIds {
    ty: WIRE_EDGE_CURVE_TYPE,
    range: WIRE_EDGE_RANGE,
    count: WIRE_EDGE_NURBS_VERTEX_COUNT,
    order: WIRE_EDGE_NURBS_ORDER,
    cvs: WIRE_EDGE_NURBS_CVS,
    knots: WIRE_EDGE_NURBS_KNOTS,
    weights: WIRE_EDGE_NURBS_WEIGHTS,
    circle_center: WIRE_EDGE_CIRCLE_CENTER,
    circle_axis: WIRE_EDGE_CIRCLE_AXIS,
    circle_ref: WIRE_EDGE_CIRCLE_REF_DIRECTION,
    circle_radius: WIRE_EDGE_CIRCLE_RADIUS,
    line_origin: WIRE_EDGE_LINE_ORIGIN,
    line_direction: WIRE_EDGE_LINE_DIRECTION,
    ellipse_center: WIRE_EDGE_ELLIPSE_CENTER,
    ellipse_axis: WIRE_EDGE_ELLIPSE_AXIS,
    ellipse_ref: WIRE_EDGE_ELLIPSE_REF_DIRECTION,
    ellipse_x: WIRE_EDGE_ELLIPSE_X_RADIUS,
    ellipse_y: WIRE_EDGE_ELLIPSE_Y_RADIUS,
    form: WIRE_EDGE_NURBS_FORM,
    vertices: WIRE_EDGE_VERTICES,
};

const CURVE_NURBS_I: i64 = CURVE_NURBS as i64;
const CURVE_LINE_I: i64 = CURVE_LINE as i64;
const CURVE_CIRCLE_I: i64 = CURVE_CIRCLE as i64;
const CURVE_ELLIPSE_I: i64 = CURVE_ELLIPSE as i64;
const SURFACE_NURBS_I: i64 = SURFACE_NURBS as i64;
const SURFACE_PLANE_I: i64 = SURFACE_PLANE as i64;
const SURFACE_CYLINDER_I: i64 = SURFACE_CYLINDER as i64;
const SURFACE_CONE_I: i64 = SURFACE_CONE as i64;
const SURFACE_SPHERE_I: i64 = SURFACE_SPHERE as i64;
const SURFACE_TORUS_I: i64 = SURFACE_TORUS as i64;

/// The `SmBSplineCurve` part of a curve that is one (`IsKindOf(SmBSplineCurve_TYPE)`), with its
/// NURB (`GetOrCreateGwNurbPointer`: a lazy line or conic makes it on a copy); `None` for the
/// other kinds.
pub(super) fn bspline_curve_part<'c>(k: &Kernel, c: &'c Curve) -> Result<Option<Cow<'c, BSplineCurve>>> {
    Ok(Some(match c.kind() {
        CurveKind::BSpline(b) => Cow::Borrowed(b),
        CurveKind::Line(l) => l.base_with_nurb().map_err(sc("SmLine::GetOrCreateGwNurbPointer"))?,
        CurveKind::Ellipse(e) if e.base().gw_nurb().is_none() => {
            let mut made = e.clone();
            made.make_nurb(k).map_err(sc("SmEllipse::MakeNurb"))?;
            Cow::Owned(made.base().clone_exact())
        }
        CurveKind::Ellipse(e) => Cow::Borrowed(e.base()),
        CurveKind::Hyperbola(h) if h.base().gw_nurb().is_none() => {
            let mut made = h.clone();
            made.make_nurb(None).map_err(sc("SmHyperbola::MakeNurb"))?;
            Cow::Owned(made.base().clone_exact())
        }
        CurveKind::Hyperbola(h) => Cow::Borrowed(h.base()),
        CurveKind::Parabola(p) if p.base().gw_nurb().is_none() => {
            let mut made = p.clone();
            made.make_nurb(None).map_err(sc("SmParabola::MakeNurb"))?;
            Cow::Owned(made.base().clone_exact())
        }
        CurveKind::Parabola(p) => Cow::Borrowed(p.base()),
        _ => return Ok(None),
    }))
}

/// The `SmBSplineSurface` part of a surface that is one (`IsKindOf(SmBSplineSurface_TYPE)`), its
/// NURB made first (`GetOrCreateGwNurbPointer`); `None` for the other kinds.
fn bspline_surface_part<'s>(k: &Kernel, s: &'s Surface) -> Result<Option<&'s BSplineSurface>> {
    Ok(match s.kind_with_nurb(k).map_err(sc("SmSurface::GetOrCreateGwNurbPointer"))? {
        SurfaceKind::BSpline(b) => Some(b),
        SurfaceKind::Plane(p) => Some(p.base()),
        SurfaceKind::Cone(c) => Some(c.rev().base()),
        SurfaceKind::Sphere(s) => Some(s.base().base()),
        SurfaceKind::Torus(t) => Some(t.base().base()),
        SurfaceKind::SurfOfRevolution(r) => Some(r.base()),
        SurfaceKind::SurfOfExtrusion(e) => Some(e.base()),
        _ => None,
    })
}

// ---- export: FlatFromBrep ----------------------------------------------------------------------

/// `body` as the flat arrays the C API exports (`FlatFromBrep`).
pub(crate) fn to_arrays(body: &Body) -> Result<Arrays> {
    with_body("FlatFromBrep", body, |k, b| Ok(flat_from_brep(k, b)))?
}

/// One NURBS curve's arrays (`smu_AddSmBSplineCurve3d`, `smu_AddSmBSplineCurve2d`): counts and
/// orders, cartesian control points of `dim` coordinates, knots and weights.
struct NurbsOut<'a> {
    count: &'a mut Vec<i64>,
    order: &'a mut Vec<i64>,
    cvs: &'a mut Vec<f64>,
    knots: &'a mut Vec<f64>,
    weights: &'a mut Vec<f64>,
}

/// `smu_AddSmBSplineCurve3d`: the curve's NURB, its control points made cartesian.
fn add_bspline_curve_3d(nurb: &NurbCurve, o: NurbsOut<'_>) -> Result<()> {
    o.count.push(count(nurb.cpts().len())?);
    o.order.push(count(nurb.degree())?.saturating_add(1));
    o.knots.extend_from_slice(nurb.knots());
    for p in nurb.cpts() {
        let (mut x, mut y, mut z, mut w) = (p.x, p.y, p.z, 1.0);
        if p.w != NOW {
            x /= p.w;
            y /= p.w;
            z /= p.w;
            w = p.w;
        }
        o.cvs.extend([x, y, z]);
        o.weights.push(w);
    }
    Ok(())
}

/// The NURB of a B-spline part (`GetOrCreateGwNurbPointer`, NULL being `SM_ERR`).
fn nurb_of(part: &BSplineCurve) -> Result<&NurbCurve> {
    part.gw_nurb().ok_or_else(|| bad("a B-spline curve without its NURB"))
}

/// `smu_AddSmBSplineCurve2d`: an edgeuse's UV trim, reversed when the edgeuse runs against its
/// edge; `None` appends the empty curve. `remap` maps the knots from the edge's interval
/// `[remap[0], remap[1]]` onto `[remap[2], remap[3]]`, `affine` the face's NURBS (u, v) onto the
/// arrays' plane (u, v).
fn add_bspline_curve_2d(o: &mut Out, trim: Option<&NurbCurve>, same: bool, remap: Option<[f64; 4]>, affine: Option<[f64; 6]>) -> Result<()> {
    let Some(nurb) = trim else {
        o.i(EDGEUSE_NURBS_VERTEX_COUNT).push(0);
        o.i(EDGEUSE_NURBS_ORDER).push(0);
        return Ok(());
    };
    let reversed;
    let nurb = if same {
        nurb
    } else {
        reversed = nurb.reversed().map_err(sc("N_CrvReverse"))?;
        &reversed
    };
    o.i(EDGEUSE_NURBS_VERTEX_COUNT).push(count(nurb.cpts().len())?);
    o.i(EDGEUSE_NURBS_ORDER).push(count(nurb.degree())?.saturating_add(1));
    for &k in nurb.knots() {
        let k = match remap {
            Some(r) if k == r[1] => r[3],
            Some(r) => r[2] + (k - r[0]) / (r[1] - r[0]) * (r[3] - r[2]),
            None => k,
        };
        o.r(EDGEUSE_NURBS_KNOTS).push(k);
    }
    for p in nurb.cpts() {
        let (mut x, mut y, mut w) = (p.x, p.y, 1.0);
        if p.w != NOW {
            x /= p.w;
            y /= p.w;
            w = p.w;
        }
        if let Some(a) = affine {
            let ax = a[0] + a[1] * x + a[2] * y;
            y = a[3] + a[4] * x + a[5] * y;
            x = ax;
        }
        o.r(EDGEUSE_NURBS_CVS).extend([x, y]);
        o.r(EDGEUSE_NURBS_WEIGHTS).push(w);
    }
    Ok(())
}

/// `smu_AddSmBSplineSurface`: a surface's NURB, control points U-major and cartesian. A knot
/// vector without extent is `SM_ERR`.
fn add_bspline_surface(o: &mut Out, nurb: &NurbSurface) -> Result<()> {
    let (u, v) = (nurb.u_knots(), nurb.v_knots());
    let spans = |k: &[f64]| matches!((k.first(), k.last()), (Some(a), Some(b)) if a < b);
    if !spans(u) || !spans(v) {
        return Err(bad("NURBS surface has degenerate knots and can't be converted"));
    }
    let (nu, nv) = (nurb.n().saturating_add(1), nurb.m().saturating_add(1));
    o.i(FACE_NURBS_U_VERTEX_COUNT).push(count(nu)?);
    o.i(FACE_NURBS_V_VERTEX_COUNT).push(count(nv)?);
    o.i(FACE_NURBS_U_ORDER).push(count(nurb.p())?.saturating_add(1));
    o.i(FACE_NURBS_V_ORDER).push(count(nurb.q())?.saturating_add(1));
    o.r(FACE_NURBS_U_KNOTS).extend_from_slice(u);
    o.r(FACE_NURBS_V_KNOTS).extend_from_slice(v);
    for row in nurb.rows() {
        if row.len() != nv {
            return Err(bad("NURBS surface net"));
        }
        for p in row {
            let (mut x, mut y, mut z, mut w) = (p.x, p.y, p.z, 1.0);
            if p.w != NOW {
                w = p.w;
                x /= w;
                y /= w;
                z /= w;
            }
            o.r3(FACE_NURBS_CVS, Vec3::new(x, y, z));
            o.r(FACE_NURBS_WEIGHTS).push(w);
        }
    }
    Ok(())
}

/// `smu_CompactLocalIndex`: a BrepData index less the sorted removed indices below it.
fn compact_index(i: u64, removed: &[u64]) -> Result<i64> {
    let below = removed.partition_point(|&r| r < i);
    count(index_u64(i)?.saturating_sub(below))
}

/// `EdgeCurveAsBSpline`: an edge curve's B-spline part, else its approximation within
/// `approx_tol` over the edge interval (`ApproximateCurve`); `None` when it has neither.
fn edge_curve_as_bspline<'c>(k: &Kernel, c: &'c Curve, ivl: &Extent1d, approx_tol: f64) -> Result<Option<Cow<'c, BSplineCurve>>> {
    if let Some(part) = bspline_curve_part(k, c)? {
        return Ok(Some(part));
    }
    let Ok(approx) = c.approximate_curve(k, &[ivl.min(), ivl.max()], approx_tol, &ApproximateCurveOptions::default()) else {
        return Ok(None);
    };
    Ok(bspline_curve_part(k, &approx.curve)?.map(|p| Cow::Owned(p.clone_exact())))
}

/// `BuildAnalyticEdgeCurve`: the curve the import builds for a line (unbounded,
/// `center + s * axis`), circle or ellipse (over `[d_min, d_max]` degrees, its NURBS parameter
/// then set to that interval with its notification, `EditParameterization`).
fn build_analytic_edge_curve(k: &Kernel, ty: i64, d_min: f64, d_max: f64, center: Point3, axis: Vec3, refd: Vec3, r1: f64, r2: f64) -> Result<Curve> {
    analytic_edge_curve(k, ty, d_min, d_max, center, axis, refd, r1, r2).map_err(|(call, e)| sc(call)(e))
}

/// [`build_analytic_edge_curve`] with solidcore's own error, and the call that gave it.
#[allow(clippy::too_many_arguments)] // BuildAnalyticEdgeCurve's
fn analytic_edge_curve(
    k: &Kernel,
    ty: i64,
    d_min: f64,
    d_max: f64,
    center: Point3,
    axis: Vec3,
    refd: Vec3,
    r1: f64,
    r2: f64,
) -> std::result::Result<Curve, (&'static str, SolidError)> {
    if ty == CURVE_LINE_I {
        return Ok(Curve::from(Line::create_canonical(center, axis)));
    }
    let mut placement = Axis2Placement::default();
    placement.set_canonical(center, refd, axis.cross(refd)).map_err(|e| ("SmAxis2Placement::SetCanonical", e))?;
    let degrees = Extent1d::from_min_max(d_min, d_max);
    let mut e = if ty == CURVE_CIRCLE_I {
        Ellipse::create_canonical_circle(k, &placement, r1, Some(&degrees), None).map_err(|e| ("SmCircle::CreateCanonical", e))?
    } else {
        Ellipse::create_canonical(k, &placement, r1, r2, Some(&degrees), None).map_err(|e| ("SmEllipse::CreateCanonical", e))?
    };
    e.edit_parameterization(k, &degrees, true, None).map_err(|e| ("SmEllipse::EditParameterization", e))?;
    Ok(Curve::from(e))
}

/// How one edge's curve is exported (`EdgeExport`).
#[derive(Clone, Copy)]
struct EdgeExport {
    ty: i64,
    /// The exported range: radians for circles and ellipses.
    lo: f64,
    hi: f64,
    /// The imported edge's interval, in SMLib units (degrees for circles and ellipses).
    sm_lo: f64,
    sm_hi: f64,
    center: Point3,
    axis: Vec3,
    refd: Vec3,
    r1: f64,
    r2: f64,
    /// The imported curve's parameter is an affine image of the edge's.
    affine: bool,
}

const NURBS_EXPORT: EdgeExport = EdgeExport {
    ty: CURVE_NURBS_I,
    lo: 0.0,
    hi: 0.0,
    sm_lo: 0.0,
    sm_hi: 0.0,
    center: Vec3::new(0.0, 0.0, 0.0),
    axis: Vec3::new(0.0, 0.0, 0.0),
    refd: Vec3::new(0.0, 0.0, 0.0),
    r1: 0.0,
    r2: 0.0,
    affine: false,
};

/// `ClassifyEdgeCurve`: a circle, ellipse or line kept as its kind when the curve the import
/// builds from it ([`build_analytic_edge_curve`]) matches it pointwise at the edge's ends and
/// along the STEP parameter; anything else NURBS. A curve whose STEP parameter runs against its
/// NURBS one goes with its axis reversed and its range negated. A call that returns a status
/// other than success makes the edge NURBS, as there ([`answer`]); any other solidcore error
/// fails the export.
fn classify_edge_curve(k: &Kernel, c: Option<&Curve>, ivl: &Extent1d) -> Result<EdgeExport> {
    let Some(c) = c else { return Ok(NURBS_EXPORT) };
    Ok(classify(k, c, ivl).map_err(sc("ClassifyEdgeCurve"))?.unwrap_or(NURBS_EXPORT))
}

fn classify(k: &Kernel, c: &Curve, ivl: &Extent1d) -> std::result::Result<Option<EdgeExport>, SolidError> {
    let mut e = NURBS_EXPORT;
    match c.kind() {
        CurveKind::Ellipse(el) => {
            let can = el.canonical();
            e.ty = if el.is_circle_class() { CURVE_CIRCLE_I } else { CURVE_ELLIPSE_I };
            e.center = can.position.origin();
            e.axis = can.position.z_axis();
            e.refd = can.position.x_axis();
            e.r1 = can.radius_x;
            e.r2 = if el.is_circle_class() { can.radius_x } else { can.radius_y };
        }
        CurveKind::Line(l) => {
            let can = l.canonical();
            e.ty = CURVE_LINE_I;
            e.center = can.point;
            e.axis = can.vector;
        }
        _ => return Ok(None),
    }
    let mut s0 = {
        let Some(x) = answer(c.convert_t_from_nurbs_to_step(k, ivl.min()))? else { return Ok(None) };
        x
    };
    let mut s1 = {
        let Some(x) = answer(c.convert_t_from_nurbs_to_step(k, ivl.max()))? else { return Ok(None) };
        x
    };
    if !s0.is_finite() || !s1.is_finite() {
        return Ok(None);
    }
    // sNew = sign * sOld + shift
    let (mut sign, mut shift) = (1.0, 0.0);
    if s0 > s1 {
        sign = -1.0;
        s0 = -s0;
        s1 = -s1;
        e.axis = Vec3::new(-e.axis.x, -e.axis.y, -e.axis.z);
    }
    if !(s1 > s0) {
        return Ok(None);
    }
    if e.ty == CURVE_LINE_I {
        if s0.abs() > INFINITE_PARAMETER || s1.abs() > INFINITE_PARAMETER {
            return Ok(None);
        }
        (e.lo, e.sm_lo, e.hi, e.sm_hi) = (s0, s0, s1, s1);
    } else {
        // SmCircle/SmEllipse::CreateCanonical take intervals within [-360, 360] degrees. (Not
        // in the C++: angles past a million degrees are refused, bounding the loops below.)
        if s1 - s0 > 360.0 + 1e-9 || s0.abs() > 1.0e6 || s1.abs() > 1.0e6 {
            return Ok(None);
        }
        while s1 > 360.0 + 1e-9 {
            s0 -= 360.0;
            s1 -= 360.0;
            shift -= 360.0;
        }
        while s0 < -360.0 - 1e-9 {
            s0 += 360.0;
            s1 += 360.0;
            shift += 360.0;
        }
        if s1 > 360.0 + 1e-9 {
            return Ok(None);
        }
        e.lo = deg2rad(s0);
        e.hi = deg2rad(s1);
        // exactly what the import computes
        e.sm_lo = radians_to_smlib_degrees(e.lo);
        e.sm_hi = radians_to_smlib_degrees(e.hi);
    }

    let built = {
        let Some(x) = answer(analytic_edge_curve(k, e.ty, e.sm_lo, e.sm_hi, e.center, e.axis, e.refd, e.r1, e.r2).map_err(|(_, e)| e))? else {
            return Ok(None);
        };
        x
    };
    let a = {
        let Some(x) = answer(c.evaluate_point(k, ivl.min()))? else { return Ok(None) };
        x
    };
    let b = {
        let Some(x) = answer(c.evaluate_point(k, ivl.max()))? else { return Ok(None) };
        x
    };
    let tol = 1e-9 * (1.0 + e.r1 + e.r2 + a.max_dimension() + b.max_dimension());
    let mut affine = true;
    for step in 0..=8 {
        let f = f64::from(step) / 8.0;
        // the same point by the STEP parameter of each curve
        let s_new = e.sm_lo + f * (e.sm_hi - e.sm_lo);
        let s_old = (s_new - shift) * sign;
        let t_old = {
            let Some(x) = answer(c.convert_t_from_step_to_nurbs(k, s_old))? else { return Ok(None) };
            x
        };
        let t_new = {
            let Some(x) = answer(built.convert_t_from_step_to_nurbs(k, s_new))? else { return Ok(None) };
            x
        };
        let p_old = {
            let Some(x) = answer(c.evaluate_point(k, t_old))? else { return Ok(None) };
            x
        };
        let p_new = {
            let Some(x) = answer(built.evaluate_point(k, t_new))? else { return Ok(None) };
            x
        };
        if !(p_old.distance(p_new) <= tol) {
            return Ok(None);
        }
        // the same point by each edge's NURBS parameter, affinely matched
        let q_old = {
            let Some(x) = answer(c.evaluate_point(k, ivl.min() + f * (ivl.max() - ivl.min())))? else { return Ok(None) };
            x
        };
        let q_new = {
            let Some(x) = answer(built.evaluate_point(k, e.sm_lo + f * (e.sm_hi - e.sm_lo)))? else { return Ok(None) };
            x
        };
        let d = q_old.distance(q_new);
        if (step == 0 || step == 8) && !(d <= tol) {
            return Ok(None); // the ends must match exactly as the edge's vertices do
        }
        affine = affine && d <= tol;
    }
    e.affine = affine;
    Ok(Some(e))
}

/// `WriteEdgeCurve`: one edge's kind, range and geometry. A circle or ellipse also goes with its
/// own NURBS form (`*_NURBS_FORM`), whose interval its range is.
fn write_edge_curve(k: &Kernel, o: &mut Out, ids: &EdgeIds, e: &EdgeExport, c: Option<&Curve>, ivl: &Extent1d, approx_tol: f64) -> Result<()> {
    o.i(ids.ty).push(e.ty);
    let form = e.ty == CURVE_CIRCLE_I || e.ty == CURVE_ELLIPSE_I;
    o.i(ids.form).push(i64::from(form));
    let c = c.ok_or_else(|| bad("an edge without a curve"))?;
    let nurbs = |o: &mut Out, part: &BSplineCurve| -> Result<()> {
        let (mut count, mut order, mut cvs, mut knots, mut weights) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        add_bspline_curve_3d(
            nurb_of(part)?,
            NurbsOut { count: &mut count, order: &mut order, cvs: &mut cvs, knots: &mut knots, weights: &mut weights },
        )?;
        o.i(ids.count).append(&mut count);
        o.i(ids.order).append(&mut order);
        o.r(ids.cvs).append(&mut cvs);
        o.r(ids.knots).append(&mut knots);
        o.r(ids.weights).append(&mut weights);
        Ok(())
    };
    if e.ty == CURVE_NURBS_I {
        o.r(ids.range).extend([ivl.min(), ivl.max()]);
        // SmuConvert asserts and dereferences NULL
        let part = edge_curve_as_bspline(k, c, ivl, approx_tol)?.ok_or_else(|| bad("an edge curve SMLib could not approximate"))?;
        return nurbs(o, &part);
    }
    if form {
        o.r(ids.range).extend([ivl.min(), ivl.max()]);
    } else {
        o.r(ids.range).extend([e.lo, e.hi]);
    }
    if e.ty == CURVE_LINE_I {
        o.r3(ids.line_origin, e.center);
        o.r3(ids.line_direction, e.axis);
        return Ok(());
    }
    if e.ty == CURVE_CIRCLE_I {
        o.r3(ids.circle_center, e.center);
        o.r3(ids.circle_axis, e.axis);
        o.r3(ids.circle_ref, e.refd);
        o.r(ids.circle_radius).push(e.r1);
    } else {
        o.r3(ids.ellipse_center, e.center);
        o.r3(ids.ellipse_axis, e.axis);
        o.r3(ids.ellipse_ref, e.refd);
        o.r(ids.ellipse_x).push(e.r1);
        o.r(ids.ellipse_y).push(e.r2);
    }
    // every SMLib conic is an SmBSplineCurve: its own NURBS, in edge order with the NURBS
    // edges' curves
    let part = bspline_curve_part(k, c)?.ok_or_else(|| bad("a conic that is not a B-spline"))?;
    nurbs(o, &part)
}

/// `ZoneTolWithFaces`: `own` or, where larger, the B-rep's tolerance or that of a face round it.
fn zone_tol_with_faces(b: &Brep, own: f64, brep_tol: f64, faces: &[FaceId]) -> Result<f64> {
    let mut tol = own.max(brep_tol);
    for &f in faces {
        let t = solidcore::topology::face::p2::tolerance(b, f).map_err(sc("SmFace::GetTolerance"))?;
        if t.is_finite() && t > tol {
            tol = t;
        }
    }
    Ok(tol)
}

/// `GetMaxZoneTol3d`: the largest of the B-rep's, its faces', edges' and vertices' tolerances.
fn max_zone_tol(b: &Brep) -> Result<f64> {
    let mut max = b.tolerance();
    let mut take = |t: f64| {
        if t.is_finite() && t >= 0.0 && t > max {
            max = t;
        }
    };
    for f in bl::faces(b, None).map_err(sc("GetFaces"))? {
        take(solidcore::topology::face::p2::tolerance(b, f).map_err(sc("SmFace::GetTolerance"))?);
    }
    for e in bl::edges(b, None).map_err(sc("GetEdges"))? {
        take(edge::tolerance(b, e).map_err(sc("SmEdge::GetTolerance"))?);
    }
    for v in bl::vertices(b, None).map_err(sc("GetVertices"))? {
        take(vertex::tolerance(b, v).map_err(sc("SmVertex::GetTolerance"))?);
    }
    Ok(max)
}

/// The shells in `SmBrepData::FromBrep`'s order: the regions, the infinite one first, each
/// region's shells in list order.
fn shells_in_order(b: &Brep) -> Result<Vec<ShellId>> {
    let mut regions = bl::regions(b, None).map_err(sc("GetRegions"))?;
    let infinite = b.infinite_region();
    if regions.len() > 1
        && regions.first().copied() != infinite
        && let Some(i) = regions.iter().skip(1).position(|&r| Some(r) == infinite)
    {
        let r = regions.remove(i + 1);
        regions.insert(0, r);
    }
    let mut shells = Vec::new();
    for r in regions {
        shells.extend(b.region(r).map_err(sc("GetShells"))?.shells.iter().copied());
    }
    Ok(shells)
}

/// The topology identity [`BrepData::from_brep`] and [`make_topology_from_data`] name entities
/// by (upstream: their addresses): the type code and the entity's index.
pub(crate) fn key_of(e: EntityRef) -> ObjectRef {
    let (type_code, kind, i) = match e {
        EntityRef::Brep(x) => (16011, 0u64, x.index()),
        EntityRef::Region(x) => (16012, 1, x.index()),
        EntityRef::Shell(x) => (16013, 2, x.index()),
        EntityRef::Faceuse(x) => (16022, 3, x.index()),
        EntityRef::Face(x) => (16002, 4, x.index()),
        EntityRef::Loopuse(x) => (16004, 5, x.index()),
        EntityRef::Loop(x) => (16003, 6, x.index()),
        EntityRef::Edgeuse(x) => (16005, 7, x.index()),
        EntityRef::Edge(x) => (16016, 8, x.index()),
        EntityRef::Vertexuse(x) => (16006, 9, x.index()),
        EntityRef::Vertex(x) => (16021, 10, x.index()),
    };
    ObjectRef { type_code, key: ObjectKey(((kind + 1) << 32) | u64::from(i)) }
}

/// `FlatFromBrep`: `b` as flat arrays, through `SmBrepData::FromBrep`. Every edgeuse on a face
/// without a UV trim first gets SMLib's own (`GetOrCreateUVTrimCurve`); edges and wire edges,
/// vertices and point-shell vertices, edgeuses and wire edgeuses are split apart, every index
/// compacted for the split.
fn flat_from_brep(k: &mut Kernel, b: &mut Brep) -> Result<Arrays> {
    for eu in solidcore::topology::brep::p2::edgeuses(b, None).map_err(sc("GetEdgeuses"))? {
        if eu1::faceuse(b, eu).map_err(sc("GetFaceuse"))?.is_none()
            || eu1::uv_trim_curve_pointer(b, eu).map_err(sc("GetUVTrimCurvePointer"))?.is_some()
        {
            continue;
        }
        let made = eu3::get_or_create_uv_trim_curve(k, b, eu, false, false, true).map_err(sc("GetOrCreateUVTrimCurve"))?;
        made.status.map_err(sc("GetOrCreateUVTrimCurve"))?;
    }
    let b: &Brep = b;

    // the BrepData; its records keep the B-rep's lists' order
    let store = AttributeStore::default();
    let records = NotifyRecords::new();
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &key_of,
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    let bd = BrepData::from_brep(b, &env, &mut Vec::new()).map_err(sc("SmBrepData::FromBrep"))?;
    let shell_ids = shells_in_order(b)?;
    let face_ids = bl::faces(b, None).map_err(sc("GetFaces"))?;
    let edge_ids = bl::edges(b, None).map_err(sc("GetEdges"))?;
    let vertex_ids = bl::vertices(b, None).map_err(sc("GetVertices"))?;
    if shell_ids.len() != bd.shells.len()
        || face_ids.len() != bd.faces.len()
        || edge_ids.len() != bd.edges.len()
        || vertex_ids.len() != bd.vertices.len()
    {
        return Err(bad("SmBrepData::FromBrep's lists differ from the B-rep's"));
    }

    // TOLERANCE is the B-rep's own tolerance, and each edge and vertex carries its own
    // (EDGE_TOLERANCE, VERTEX_TOLERANCE). Writing the largest one there instead, as SMLib's
    // `FlatFromBrep` does, makes it every entity's on import (MakeTopologyFromData raises each
    // entity to the B-rep's), so every crossing between the kernels spread the largest tolerance
    // over the body. Curves are still approximated to the largest.
    let zone = max_zone_tol(b)?;
    let brep_tol = b.tolerance();
    let xsect = 2.0 * if brep_tol.is_finite() && brep_tol > 0.0 && brep_tol < zone { brep_tol } else { zone };
    let approx_tol = zone / 2.0;
    let bbox = calculate_bounding_box(k, b, BoundingBoxOptions { approx: false, tight: false, expand_by_zone_tol_3d: false })
        .map_err(sc("CalculateBoundingBox"))?;

    let mut o = Out::new();
    o.r(TOLERANCE).push(xsect);
    let (lo, hi) = (bbox.min(), bbox.max());
    o.r(EXTENT).extend([lo.x, lo.y, lo.z, hi.x, hi.y, hi.z]);
    o.i(SOURCE).push(1);

    let edge_index: HashMap<EdgeId, usize> = edge_ids.iter().enumerate().map(|(i, &e)| (e, i)).collect();
    let edge_curve = |i: usize| -> Result<Option<&Curve>> { edge::curve(b, get(&edge_ids, i)?).map_err(sc("SmEdge::GetCurve")) };
    let mut exports = Vec::with_capacity(bd.edges.len());
    for (i, ed) in bd.edges.iter().enumerate() {
        exports.push(classify_edge_curve(k, edge_curve(i)?, &ed.interval)?);
    }

    // Regions
    for r in &bd.regions {
        o.i(REGION_SHELL_COUNT).push(i64::try_from(r.num_shells).map_err(|_| bad("count"))?);
        o.i(REGION_TYPE).push(i64::from(r.is_void != 0));
    }

    // Shells, wire edges and their curves, point-shell points
    let mut point_shell_vertices: Vec<u64> = Vec::new();
    let mut wire_edges: Vec<u64> = Vec::new();
    let mut wire_vertices_raw: Vec<u64> = Vec::new();
    for (sd, &s) in bd.shells.iter().zip(&shell_ids) {
        let sh = b.shell(s).map_err(sc("GetShell"))?;
        let faceuses = sh.get_faceuses().map_err(sc("SmShell::GetFaceuses"))?.len();
        let wires = shell::wire_edges(k, b, s).map_err(sc("SmShell::GetWireEdges"))?;
        let point = faceuses == 0 && wires.is_empty();
        o.i(SHELL_FACEUSE_COUNT).push(count(faceuses)?);
        o.i(SHELL_WIRE_EDGE_COUNT).push(count(wires.len())?);
        o.i(SHELL_POINT_TYPE).push(i64::from(point));
        for w in wires {
            let i = *edge_index.get(&w).ok_or_else(|| bad("a wire edge not in the B-rep"))?;
            let wd = bd.edges.get(i).ok_or_else(|| bad("wire edge record"))?;
            wire_edges.push(i as u64);
            wire_vertices_raw.extend([wd.start_vertex, wd.end_vertex]);
            write_edge_curve(k, &mut o, &WIRE_EDGES, &get(&exports, i)?, edge_curve(i)?, &wd.interval, approx_tol)?;
        }
        if point {
            let v = sh.vertex(b).map_err(sc("SmShell::GetVertex"))?.ok_or_else(|| bad("a point shell without its vertex"))?;
            point_shell_vertices.push(sd.vertex);
            let p = vertex::point(b, v).map_err(sc("SmVertex::GetPoint"))?;
            o.r3(SHELL_POINT_POSITION, p);
        }
    }
    point_shell_vertices.sort_unstable();
    wire_edges.sort_unstable();
    for v in wire_vertices_raw {
        let c = compact_index(v, &point_shell_vertices)?;
        o.i(WIRE_EDGE_VERTICES).push(c);
    }

    // Faceuses
    for fu in &bd.faceuses {
        o.i(FACEUSE_FACE).push(i64::try_from(fu.face).map_err(|_| bad("faceuse face"))?);
        o.i(FACEUSE_ORIENTATION).push(i64::from(fu.orientation == 1));
    }

    // Faces and their surfaces
    let n_loops = bd.loops.len();
    let mut loop_exports_uv = vec![false; n_loops];
    let mut loop_plane_map: Vec<Option<[f64; 6]>> = vec![None; n_loops];
    for (fd, &f) in bd.faces.iter().zip(&face_ids) {
        let surface = b.face(f).map_err(sc("GetFace"))?.surface.as_ref().ok_or_else(|| bad("a face without a surface"))?;
        // `m_lSurface >= m_vSurfaces.GetSize()`: a surface no face of the B-rep owns.
        if !usize::try_from(fd.surface).is_ok_and(|s| s < bd.surfaces.len()) {
            return Err(bad("a face whose surface its B-rep does not hold"));
        }
        let trim_type = i64::from(fd.rectangular_trim == 1);
        let ty = match surface.kind() {
            SurfaceKind::Sphere(_) => SURFACE_SPHERE_I,
            SurfaceKind::Plane(_) => SURFACE_PLANE_I,
            SurfaceKind::Cone(c) if c.is_cylinder_class() => SURFACE_CYLINDER_I,
            SurfaceKind::Cone(_) => SURFACE_CONE_I,
            SurfaceKind::Torus(_) => SURFACE_TORUS_I,
            _ => SURFACE_NURBS_I,
        };
        // A curved analytic face goes with its NURBS form, whose parameters its range and trims
        // are in; a plane's (u, v) are distances along its unit axes, an affine image of its
        // NURBS (u, v).
        let nurbs_form = ty != SURFACE_NURBS_I && ty != SURFACE_PLANE_I;
        let mut plane_map = None;
        if let SurfaceKind::Plane(p) = surface.kind() {
            let scale = p.uv_scale();
            let mut q = [Point2::new(0.0, 0.0); 3];
            let mut ok = true;
            for (qk, at) in q.iter_mut().zip([Point2::new(0.0, 0.0), Point2::new(1.0, 0.0), Point2::new(0.0, 1.0)]) {
                match answer(surface.convert_uv_from_nurbs_to_step(k, at)).map_err(sc("SmSurface::ConvertUVFromNURBSToSTEP"))? {
                    Some(st) => *qk = Point2::new(st.x * scale.x, st.y * scale.y),
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                let [q0, q1, q2] = q;
                plane_map = Some([q0.x, q1.x - q0.x, q2.x - q0.x, q0.y, q1.y - q0.y, q2.y - q0.y]);
            }
        }
        for l in 0..fd.num_loops {
            let li = index_u64(fd.start_loop.saturating_add(l))?;
            let (Some(x), Some(m)) = (loop_exports_uv.get_mut(li), loop_plane_map.get_mut(li)) else {
                return Err(bad("face loop index is out of bounds"));
            };
            *x = ty == SURFACE_NURBS_I || nurbs_form || plane_map.is_some();
            *m = plane_map;
        }

        // face range: a NURBS (or NURBS form) face's native (u, v); an analytic surface's STEP
        // domain, angles in radians
        let d = fd.uv_domain;
        let (mut umin, mut vmin, mut umax, mut vmax) = (d.u_min(), d.v_min(), d.u_max(), d.v_max());
        if ty != SURFACE_NURBS_I && !nurbs_form {
            let step = surface.step_uv_domain();
            if ty == SURFACE_SPHERE_I || ty == SURFACE_TORUS_I {
                (umin, vmin, umax, vmax) = (deg2rad(step.u_min()), deg2rad(step.v_min()), deg2rad(step.u_max()), deg2rad(step.v_max()));
            } else if ty == SURFACE_CYLINDER_I || ty == SURFACE_CONE_I {
                (umin, vmin, umax, vmax) = (deg2rad(step.u_min()), step.v_min(), deg2rad(step.u_max()), step.v_max());
            } else if let SurfaceKind::Plane(p) = surface.kind() {
                // plane parameters are distances along unit placement axes (SmExtent2d::Scale)
                let s = p.uv_scale();
                let scale = |x: f64, by: f64| if is_infinite(x) { x } else { x * by };
                (umin, vmin, umax, vmax) = (scale(step.u_min(), s.x), scale(step.v_min(), s.y), scale(step.u_max(), s.x), scale(step.v_max(), s.y));
            }
        }
        o.i(FACE_LOOP_COUNT).push(i64::try_from(fd.num_loops).map_err(|_| bad("count"))?);
        o.i(FACE_SURFACE_TYPE).push(ty);
        o.i(FACE_TRIM_TYPE).push(trim_type);
        o.i(FACE_NURBS_FORM).push(i64::from(nurbs_form));
        o.r(FACE_RANGE).extend([umin, vmin, umax, vmax]);

        match surface.kind() {
            SurfaceKind::Sphere(s) => {
                let pos = s.base().position();
                o.r3(FACE_SPHERE_CENTER, pos.origin());
                o.r3(FACE_SPHERE_AXIS, pos.z_axis());
                o.r3(FACE_SPHERE_REF_DIRECTION, pos.x_axis());
                o.r(FACE_SPHERE_RADIUS).push(s.radius());
            }
            SurfaceKind::Plane(p) => {
                let pos = p.position();
                o.r3(FACE_PLANE_ORIGIN, pos.origin());
                o.r3(FACE_PLANE_AXIS, pos.z_axis());
                o.r3(FACE_PLANE_REF_DIRECTION, pos.x_axis());
            }
            SurfaceKind::Cone(c) if c.is_cylinder_class() => {
                let pos = c.canonical().origin;
                o.r3(FACE_CYLINDER_ORIGIN, pos.origin());
                o.r3(FACE_CYLINDER_AXIS, pos.z_axis());
                o.r3(FACE_CYLINDER_REF_DIRECTION, pos.x_axis());
                o.r(FACE_CYLINDER_RADIUS).push(c.bot_radius());
            }
            SurfaceKind::Cone(c) => {
                let can = c.canonical();
                o.r3(FACE_CONE_ORIGIN, can.origin.origin());
                o.r3(FACE_CONE_AXIS, can.origin.z_axis());
                o.r3(FACE_CONE_REF_DIRECTION, can.origin.x_axis());
                o.r(FACE_CONE_RADIUS).push(can.radius);
                o.r(FACE_CONE_SEMI_ANGLE).push(deg2rad(can.semi_angle_deg));
            }
            SurfaceKind::Torus(t) => {
                let pos = t.base().position();
                o.r3(FACE_TORUS_ORIGIN, pos.origin());
                o.r3(FACE_TORUS_AXIS, pos.z_axis());
                o.r3(FACE_TORUS_REF_DIRECTION, pos.x_axis());
                o.r(FACE_TORUS_MAJOR_RADIUS).push(t.major_radius());
                o.r(FACE_TORUS_MINOR_RADIUS).push(t.minor_radius());
            }
            _ => {}
        }
        if nurbs_form || ty == SURFACE_NURBS_I {
            // Every analytic class is an SmBSplineSurface: its own NURBS, in face order with the
            // NURBS faces' surfaces. Another surface (an offset) is approximated within the
            // curves' tolerance, splitting its spans up to 5 times; its status fails the export,
            // as does no surface, which would leave the arrays without this face's NURBS.
            let approximated;
            let part = match bspline_surface_part(k, surface)? {
                Some(part) => part,
                None => {
                    approximated = surface
                        .approximate_surface(k, approx_tol3d(k, approx_tol), 5, true)
                        .map_err(sc("SmSurface::ApproximateSurface"))?
                        .surface
                        .ok_or_else(|| bad("a face surface SMLib could not convert to a B-spline"))?;
                    bspline_surface_part(k, &approximated)?.ok_or_else(|| bad("an approximated face surface that is not a B-spline"))?
                }
            };
            add_bspline_surface(&mut o, part.gw_nurb().ok_or_else(|| bad("a B-spline surface without its NURB"))?)?;
        }
    }

    // Loops
    for l in &bd.loops {
        if l.loop_type == 0 && l.num_eu > 0 {
            o.i(LOOP_EDGEUSE_COUNT).push(i64::try_from(l.num_eu).map_err(|_| bad("count"))?);
            o.i(LOOP_VERTEX).push(-1);
        } else if l.loop_type == 1 && l.vertex != NO_OBJECT && l.vertex != UNDEF_ULONG {
            o.i(LOOP_EDGEUSE_COUNT).push(0);
            let c = compact_index(l.vertex, &point_shell_vertices)?;
            o.i(LOOP_VERTEX).push(c);
        } else {
            return Err(bad("incorrect loop type"));
        }
    }

    // Edgeuses connecting edges to faces; wire edges' edgeuses are removed. A pair's radial
    // entry: 1 when its top edgeuse's next is its own bottom one.
    let non_face: Vec<u64> = bd.edgeuses.iter().enumerate().filter(|(_, e)| e.eu_type != 0).map(|(i, _)| i as u64).collect();
    for (j, eu) in bd.edgeuses.iter().enumerate() {
        if eu.eu_type != 0 {
            continue;
        }
        let j = count(j)?;
        o.i(EDGEUSE_EDGE).push(compact_index(eu.edge, &wire_edges)?);
        o.i(EDGEUSE_ORIENTATION).push(i64::from(eu.orientation == 1));
        let top_next = if eu.next_eu >= 0 { eu.next_eu } else { -eu.next_eu - 1 };
        let bot_next = if eu.mate_next_eu >= 0 { eu.mate_next_eu } else { -eu.mate_next_eu - 1 };
        let top_entry = top_next == j;
        o.i(EDGEUSE_RADIAL_ENTRY).push(i64::from(top_entry));
        let next = u64::try_from(if top_entry { bot_next } else { top_next }).map_err(|_| bad("radial index"))?;
        let next = compact_index(next, &non_face)?;
        o.i(EDGEUSE_NEXT_RADIAL).push(next);

        // the UV trim, on faces whose loops carry them
        let li = index_u64(eu.loop_)?;
        let exports_uv = *loop_exports_uv.get(li).ok_or_else(|| bad("edgeuse loop index is out of bounds"))?;
        let trim = if exports_uv && eu.uv_curve != NO_OBJECT { bd.uv_curves.get(index_u64(eu.uv_curve)?) } else { None };
        let part = match trim {
            Some(t) => bspline_curve_part(k, &t.object)?,
            None => None,
        };
        let mut nurb = part.as_deref().map(nurb_of).transpose()?;
        // On an edge exported as a line, the trim's knots follow the line's parameter, or the
        // trim is dropped where that is not an affine image of the edge's (SMLib rebuilds it).
        // Circles and ellipses go out on their NURBS form, whose parameter the trims are in.
        let mut remap = None;
        let ei = index_u64(eu.edge)?;
        if nurb.is_some()
            && let Some(ex) = exports.get(ei)
            && ex.ty == CURVE_LINE_I
        {
            if ex.affine {
                let ivl = bd.edges.get(ei).ok_or_else(|| bad("edge record"))?.interval;
                remap = Some([ivl.min(), ivl.max(), ex.sm_lo, ex.sm_hi]);
            } else {
                nurb = None;
            }
        }
        let affine = loop_plane_map.get(li).copied().flatten();
        let before = o.i(EDGEUSE_NURBS_VERTEX_COUNT).len();
        if add_bspline_curve_2d(&mut o, nurb, eu.orientation == 1, remap, affine).is_err() && o.i(EDGEUSE_NURBS_VERTEX_COUNT).len() == before {
            // UV trims are optional: keep the edgeuse / trim records one to one
            o.i(EDGEUSE_NURBS_VERTEX_COUNT).push(0);
            o.i(EDGEUSE_NURBS_ORDER).push(0);
        }
    }

    // Edges (face edges; wire edges went with their shells)
    for (j, ed) in bd.edges.iter().enumerate() {
        if wire_edges.binary_search(&(j as u64)).is_ok() {
            continue;
        }
        let e = get(&edge_ids, j)?;
        let start = compact_index(ed.start_vertex, &point_shell_vertices)?;
        let end = compact_index(ed.end_vertex, &point_shell_vertices)?;
        o.i(EDGE_VERTICES).extend([start, end]);
        let faces = edge::faces(b, e, None).map_err(sc("SmEdge::GetFaces"))?;
        let own = edge::tolerance(b, e).map_err(sc("SmEdge::GetTolerance"))?;
        o.r(EDGE_TOLERANCE).push(2.0 * zone_tol_with_faces(b, own, brep_tol, &faces)?);
        write_edge_curve(k, &mut o, &EDGES, &get(&exports, j)?, edge_curve(j)?, &ed.interval, approx_tol)?;
    }

    // Vertices (point-shell vertices went with their shells)
    for (j, vd) in bd.vertices.iter().enumerate() {
        if point_shell_vertices.binary_search(&(j as u64)).is_ok() {
            continue;
        }
        o.i(VERTEX_POINT_TYPE).push(1);
        o.r3(VERTEX_POSITION, vd.point);
        let v: VertexId = get(&vertex_ids, j)?;
        let faces = vertex::faces(b, v).map_err(sc("SmVertex::GetFaces"))?;
        let own = vertex::tolerance(b, v).map_err(sc("SmVertex::GetTolerance"))?;
        o.r(VERTEX_TOLERANCE).push(2.0 * zone_tol_with_faces(b, own, brep_tol, &faces)?);
    }
    Ok(o.0)
}

// ---- import: Validate --------------------------------------------------------------------------

/// The arrays of one B-rep, read as hostile input.
struct In<'a>(&'a Arrays);

impl<'a> In<'a> {
    fn i(&self, id: i32) -> &'a [i64] {
        self.0.ints.get(&id).map_or(&[], Vec::as_slice)
    }
    fn r(&self, id: i32) -> &'a [f64] {
        self.0.reals.get(&id).map_or(&[], Vec::as_slice)
    }
}

/// What `Validate` learns about valid input (`Shape`).
#[derive(Default)]
struct Shape {
    regions: usize,
    shells: usize,
    faceuses: usize,
    faces: usize,
    loops: usize,
    edgeuses: usize,
    edges: usize,
    wire_edges: usize,
    vertices: usize,
    point_shells: usize,
    uv_curves: bool,
}

/// Below SMLib's index sentinels (`SM_NO_OBJECT`, `MakeTopologyFromData`'s 999999).
const MAX_TOPOLOGY: i64 = 999_000;
/// `NL_MAXDEG + 1`.
const MAX_ORDER: i64 = 18;
const MAX_VALUE: f64 = 1.0e9;
const MAX_FLAT_LEN: i64 = 1 << 27;

fn bits(v: &[i64]) -> bool {
    v.iter().all(|&x| x == 0 || x == 1)
}

fn in_range(v: &[i64], lo: i64, hi: i64) -> bool {
    v.iter().all(|&x| x >= lo && x <= hi)
}

/// `SumCounts`: the sum of non-negative counts, `None` past [`MAX_TOPOLOGY`].
fn sum_counts(v: &[i64]) -> Option<i64> {
    let mut s = 0i64;
    for &x in v {
        if !(0..=MAX_TOPOLOGY).contains(&x) {
            return None;
        }
        s += x;
        if s > MAX_TOPOLOGY {
            return None;
        }
    }
    Some(s)
}

fn has_form(form: &[i64], i: usize) -> bool {
    form.get(i) == Some(&1)
}

/// `GoodFrame`: an axis and a reference direction that span a plane.
fn good_frame(axis: &[f64], refd: &[f64], i: usize) -> bool {
    let (Ok(a), Ok(r)) = (get3(axis, i), get3(refd, i)) else { return false };
    let (la, lr) = (a.length(), r.length());
    la > 1e-12 && lr > 1e-12 && a.cross(r).length() > 1e-9 * la * lr
}

/// `GoodKnots`: `count + order` knots, non-decreasing, with a non-empty interval.
fn good_knots(k: &[f64], count: i64, order: i64) -> bool {
    let (Ok(n), Ok(o)) = (usize::try_from(count), usize::try_from(order)) else { return false };
    let Some(k) = n.checked_add(o).and_then(|m| k.get(..m)) else { return false };
    if !k.windows(2).all(|w| matches!(w, [a, b] if b >= a)) {
        return false;
    }
    matches!((o.checked_sub(1).and_then(|i| k.get(i)), k.get(n)), (Some(a), Some(b)) if a < b)
}

fn good_weights(w: &[f64]) -> bool {
    w.iter().all(|&x| x > 0.0)
}

/// `GoodNurbsCurves`: one family of NURBS curves; with `optional`, a curve of count and order 0
/// is none.
fn good_nurbs_curves(count: &[i64], order: &[i64], cvs: &[f64], knots: &[f64], weights: &[f64], dim: i64, optional: bool) -> bool {
    if count.len() != order.len() {
        return false;
    }
    let (mut points, mut knot_total) = (0i64, 0i64);
    for (&c, &o) in count.iter().zip(order) {
        if optional && c == 0 && o == 0 {
            continue;
        }
        if !(2..=MAX_ORDER).contains(&o) || c < o || c > MAX_FLAT_LEN {
            return false;
        }
        points += c;
        knot_total += c + o;
        if points > MAX_FLAT_LEN || knot_total > MAX_FLAT_LEN {
            return false;
        }
    }
    if i64::try_from(cvs.len()).ok() != Some(dim * points) || i64::try_from(knots.len()).ok() != Some(knot_total) {
        return false;
    }
    if !weights.is_empty() && (i64::try_from(weights.len()).ok() != Some(points) || !good_weights(weights)) {
        return false;
    }
    let mut k = 0usize;
    for (&c, &o) in count.iter().zip(order) {
        if optional && c == 0 {
            continue;
        }
        if !knots.get(k..).is_some_and(|s| good_knots(s, c, o)) {
            return false;
        }
        k += (c + o) as usize;
    }
    true
}

/// One edge family's arrays (`GoodEdges`, `MakeEdgeCurve`).
struct EdgeIn<'a> {
    ty: &'a [i64],
    vertices: &'a [i64],
    range: &'a [f64],
    count: &'a [i64],
    order: &'a [i64],
    cvs: &'a [f64],
    knots: &'a [f64],
    weights: &'a [f64],
    circle_center: &'a [f64],
    circle_axis: &'a [f64],
    circle_ref: &'a [f64],
    circle_radius: &'a [f64],
    line_origin: &'a [f64],
    line_direction: &'a [f64],
    ellipse_center: &'a [f64],
    ellipse_axis: &'a [f64],
    ellipse_ref: &'a [f64],
    ellipse_x: &'a [f64],
    ellipse_y: &'a [f64],
    form: &'a [i64],
}

impl<'a> EdgeIn<'a> {
    fn new(f: &In<'a>, ids: &EdgeIds) -> EdgeIn<'a> {
        EdgeIn {
            ty: f.i(ids.ty),
            vertices: f.i(ids.vertices),
            range: f.r(ids.range),
            count: f.i(ids.count),
            order: f.i(ids.order),
            cvs: f.r(ids.cvs),
            knots: f.r(ids.knots),
            weights: f.r(ids.weights),
            circle_center: f.r(ids.circle_center),
            circle_axis: f.r(ids.circle_axis),
            circle_ref: f.r(ids.circle_ref),
            circle_radius: f.r(ids.circle_radius),
            line_origin: f.r(ids.line_origin),
            line_direction: f.r(ids.line_direction),
            ellipse_center: f.r(ids.ellipse_center),
            ellipse_axis: f.r(ids.ellipse_axis),
            ellipse_ref: f.r(ids.ellipse_ref),
            ellipse_x: f.r(ids.ellipse_x),
            ellipse_y: f.r(ids.ellipse_y),
            form: f.i(ids.form),
        }
    }
}

/// `GoodEdges`: an edge family's ranges against its curves.
fn good_edges(e: &EdgeIn<'_>, n: usize, n_vertices: usize) -> bool {
    // a NURBS form is given for circles and ellipses only, one flag per edge, or for none
    if !e.form.is_empty() && (e.form.len() != n || !bits(e.form)) {
        return false;
    }
    if e.form.iter().zip(e.ty).any(|(&f, &t)| f == 1 && t != CURVE_CIRCLE_I && t != CURVE_ELLIPSE_I) {
        return false;
    }
    let Ok(last_vertex) = i64::try_from(n_vertices).map(|v| v - 1) else { return false };
    if e.ty.len() != n
        || Some(e.vertices.len()) != n.checked_mul(2)
        || Some(e.range.len()) != n.checked_mul(2)
        || !in_range(e.ty, CURVE_NURBS_I, CURVE_ELLIPSE_I)
        || !in_range(e.vertices, 0, last_vertex)
    {
        return false;
    }
    let (mut n_n, mut n_c, mut n_l, mut n_e) = (0usize, 0usize, 0usize, 0usize);
    for (i, &t) in e.ty.iter().enumerate() {
        n_n += usize::from(t == CURVE_NURBS_I || has_form(e.form, i));
        n_c += usize::from(t == CURVE_CIRCLE_I);
        n_l += usize::from(t == CURVE_LINE_I);
        n_e += usize::from(t == CURVE_ELLIPSE_I);
    }
    if e.count.len() != n_n || !good_nurbs_curves(e.count, e.order, e.cvs, e.knots, e.weights, 3, false) {
        return false;
    }
    if e.circle_center.len() != 3 * n_c
        || e.circle_axis.len() != 3 * n_c
        || e.circle_ref.len() != 3 * n_c
        || e.circle_radius.len() != n_c
        || e.line_origin.len() != 3 * n_l
        || e.line_direction.len() != 3 * n_l
        || e.ellipse_center.len() != 3 * n_e
        || e.ellipse_axis.len() != 3 * n_e
        || e.ellipse_ref.len() != 3 * n_e
        || e.ellipse_x.len() != n_e
        || e.ellipse_y.len() != n_e
    {
        return false;
    }
    for i in 0..n_c {
        if !(e.circle_radius.get(i).is_some_and(|&r| r > 0.0)) || !good_frame(e.circle_axis, e.circle_ref, i) {
            return false;
        }
    }
    for i in 0..n_l {
        if !get3(e.line_direction, i).is_ok_and(|d| d.length() > 1e-12) {
            return false;
        }
    }
    for i in 0..n_e {
        if !(e.ellipse_x.get(i).is_some_and(|&r| r > 0.0) && e.ellipse_y.get(i).is_some_and(|&r| r > 0.0))
            || !good_frame(e.ellipse_axis, e.ellipse_ref, i)
        {
            return false;
        }
    }
    // each edge's range: non-empty; inside its NURBS curve's knot interval; at most one turn
    // for circles and ellipses (radians)
    let (mut i_nurbs, mut knot) = (0usize, 0usize);
    for i in 0..n {
        let (Some(&lo), Some(&hi)) = (e.range.get(2 * i), e.range.get(2 * i + 1)) else { return false };
        let t = e.ty.get(i).copied().unwrap_or(-1);
        if !(lo < hi) {
            return false;
        }
        if t == CURVE_NURBS_I || has_form(e.form, i) {
            let (Some(&c), Some(&o)) = (e.count.get(i_nurbs), e.order.get(i_nurbs)) else { return false };
            let (Ok(cu), Ok(ou)) = (usize::try_from(c), usize::try_from(o)) else { return false };
            let (Some(&k0), Some(&k1)) = (e.knots.get(knot + ou - 1), e.knots.get(knot + cu)) else { return false };
            let tol = 1e-9 * (k1 - k0).max(1.0);
            if lo < k0 - tol || hi > k1 + tol {
                return false;
            }
            knot += cu + ou;
            i_nurbs += 1;
        } else if (t == CURVE_CIRCLE_I || t == CURVE_ELLIPSE_I)
            && (hi - lo > std::f64::consts::TAU + 1e-9
                || lo.abs() > std::f64::consts::TAU * (1.0 + 1e-9)
                || hi.abs() > std::f64::consts::TAU * (1.0 + 1e-9))
        {
            return false; // SmCircle/SmEllipse::CreateCanonical take [-360, 360] degrees
        } else if t == CURVE_LINE_I && (lo.abs() > INFINITE_PARAMETER || hi.abs() > INFINITE_PARAMETER) {
            return false; // an imported line is unbounded over +-SM_INFINITE_PARAMETER
        }
    }
    true
}

fn face_has_nurbs(f: &In<'_>, i: usize) -> bool {
    f.i(FACE_SURFACE_TYPE).get(i) == Some(&SURFACE_NURBS_I) || has_form(f.i(FACE_NURBS_FORM), i)
}

/// `Validate`: every check the build relies on; `None` on anything malformed.
fn validate(f: &In<'_>) -> Option<Shape> {
    let mut s = Shape::default();
    // every double is finite and of sane magnitude
    if f.0.reals.values().flatten().any(|x| !x.is_finite() || x.abs() > MAX_VALUE) {
        return None;
    }

    // B-rep data
    let tol = f.r(TOLERANCE);
    if tol.len() != 1 || !tol.iter().all(|&t| t > 0.0 && t <= 1.0) {
        return None;
    }
    if !f.r(EXTENT).is_empty() && f.r(EXTENT).len() != 6 {
        return None;
    }
    if f.i(SOURCE).len() > 1 || !bits(f.i(SOURCE)) {
        return None;
    }

    // Regions
    s.regions = f.i(REGION_SHELL_COUNT).len();
    let shells = sum_counts(f.i(REGION_SHELL_COUNT))?;
    if s.regions < 1 || s.regions > MAX_TOPOLOGY as usize || f.i(REGION_TYPE).len() != s.regions || !bits(f.i(REGION_TYPE)) {
        return None;
    }
    s.shells = shells as usize;

    // Shells: a faceuse shell, a wire-edge shell (one edge) or a point shell
    let (fu_count, we_count, pt_type) = (f.i(SHELL_FACEUSE_COUNT), f.i(SHELL_WIRE_EDGE_COUNT), f.i(SHELL_POINT_TYPE));
    if fu_count.len() != s.shells || we_count.len() != s.shells || pt_type.len() != s.shells || !bits(we_count) || !bits(pt_type) {
        return None;
    }
    let faceuses = sum_counts(fu_count)?;
    if faceuses % 2 != 0 {
        return None;
    }
    for i in 0..s.shells {
        let (fu, we, pt) = (fu_count.get(i)?, we_count.get(i)?, pt_type.get(i)?);
        if *fu > 0 {
            if *we != 0 {
                return None;
            }
        } else if *we == 0 {
            if *pt != 1 {
                return None;
            }
            s.point_shells += 1;
        }
    }
    s.wire_edges = sum_counts(we_count)? as usize;
    if f.r(SHELL_POINT_POSITION).len() != 3 * s.point_shells {
        return None;
    }

    // Faceuses: each face has exactly two, one on each side
    s.faceuses = faceuses as usize;
    s.faces = s.faceuses / 2;
    let (fu_face, fu_orient) = (f.i(FACEUSE_FACE), f.i(FACEUSE_ORIENTATION));
    if fu_face.len() != s.faceuses || fu_orient.len() != s.faceuses || !bits(fu_orient) || !in_range(fu_face, 0, s.faces as i64 - 1) {
        return None;
    }
    let mut sides = vec![0u8; s.faces];
    for (&face, &o) in fu_face.iter().zip(fu_orient) {
        let bit = if o == 1 { 1 } else { 2 };
        let side = sides.get_mut(face as usize)?;
        if *side & bit != 0 {
            return None;
        }
        *side |= bit;
    }

    // Faces
    let (loop_count, surface_type, trim_type, range) = (f.i(FACE_LOOP_COUNT), f.i(FACE_SURFACE_TYPE), f.i(FACE_TRIM_TYPE), f.r(FACE_RANGE));
    if loop_count.len() != s.faces
        || surface_type.len() != s.faces
        || trim_type.len() != s.faces
        || range.len() != 4 * s.faces
        || !bits(trim_type)
        || !in_range(surface_type, SURFACE_NURBS_I, SURFACE_TORUS_I)
        || !in_range(loop_count, 1, MAX_TOPOLOGY)
    {
        return None;
    }
    let form = f.i(FACE_NURBS_FORM);
    if !form.is_empty() {
        if form.len() != s.faces || !bits(form) {
            return None;
        }
        if form.iter().zip(surface_type).any(|(&fm, &t)| fm == 1 && (t == SURFACE_NURBS_I || t == SURFACE_PLANE_I)) {
            return None;
        }
    }
    for i in 0..s.faces {
        let (u0, v0, u1, v1) = (*range.get(4 * i)?, *range.get(4 * i + 1)?, *range.get(4 * i + 2)?, *range.get(4 * i + 3)?);
        if !(u0 < u1) || !(v0 < v1) {
            return None;
        }
        // angular parameters (radians): at most one turn, near the principal range; a sphere's
        // latitude within [-pi/2, pi/2]. A face given with its NURBS form has a NURBS range.
        let t = if face_has_nurbs(f, i) { SURFACE_NURBS_I } else { *surface_type.get(i)? };
        let u_angle = t == SURFACE_SPHERE_I || t == SURFACE_TORUS_I || t == SURFACE_CYLINDER_I || t == SURFACE_CONE_I;
        let v_angle = t == SURFACE_SPHERE_I || t == SURFACE_TORUS_I;
        let pi = std::f64::consts::PI;
        let (turn, far) = (2.0 * pi * (1.0 + 1e-9), 4.0 * pi);
        if (u_angle && (u1 - u0 > turn || u0.abs() > far || u1.abs() > far))
            || (v_angle && (v1 - v0 > turn || v0.abs() > far || v1.abs() > far))
            || (t == SURFACE_SPHERE_I && (v0 < -0.5 * pi - 1e-9 || v1 > 0.5 * pi + 1e-9))
        {
            return None;
        }
    }
    s.loops = sum_counts(loop_count)? as usize;

    // Vertices
    s.vertices = f.i(VERTEX_POINT_TYPE).len();
    if s.vertices > MAX_TOPOLOGY as usize
        || s.vertices + s.point_shells > MAX_TOPOLOGY as usize
        || !in_range(f.i(VERTEX_POINT_TYPE), 1, 1)
        || f.r(VERTEX_POSITION).len() != 3 * s.vertices
    {
        return None;
    }
    // per-edge and per-vertex tolerances: none, or one each
    let tolerances_ok = |t: &[f64], n: usize| t.is_empty() || (t.len() == n && t.iter().all(|&x| x.is_finite() && x > 0.0 && x <= 1.0));
    if !tolerances_ok(f.r(VERTEX_TOLERANCE), s.vertices) {
        return None;
    }

    // Loops
    let (lp_count, lp_vertex) = (f.i(LOOP_EDGEUSE_COUNT), f.i(LOOP_VERTEX));
    if lp_count.len() != s.loops || lp_vertex.len() != s.loops {
        return None;
    }
    s.edgeuses = sum_counts(lp_count)? as usize;
    for (&n, &v) in lp_count.iter().zip(lp_vertex) {
        if if n == 0 { !(v >= 0 && v < s.vertices as i64) } else { v != -1 } {
            return None;
        }
    }

    // Edges and wire edges
    s.edges = f.i(EDGE_CURVE_TYPE).len();
    if s.edges > MAX_TOPOLOGY as usize || s.edges + s.wire_edges > MAX_TOPOLOGY as usize || s.wire_edges + s.edgeuses > MAX_TOPOLOGY as usize {
        return None;
    }
    if !tolerances_ok(f.r(EDGE_TOLERANCE), s.edges) {
        return None;
    }
    if !good_edges(&EdgeIn::new(f, &EDGES), s.edges, s.vertices) || !good_edges(&EdgeIn::new(f, &WIRE_EDGES), s.wire_edges, s.vertices) {
        return None;
    }

    // Edgeuses
    let (eu_edge, eu_orient, eu_next, eu_entry) = (f.i(EDGEUSE_EDGE), f.i(EDGEUSE_ORIENTATION), f.i(EDGEUSE_NEXT_RADIAL), f.i(EDGEUSE_RADIAL_ENTRY));
    if eu_edge.len() != s.edgeuses
        || eu_orient.len() != s.edgeuses
        || eu_next.len() != s.edgeuses
        || eu_entry.len() != s.edgeuses
        || !bits(eu_orient)
        || !bits(eu_entry)
        || !in_range(eu_edge, 0, s.edges as i64 - 1)
        || !in_range(eu_next, 0, s.edgeuses as i64 - 1)
    {
        return None;
    }
    {
        // the radial lists: NEXT_RADIAL is a permutation that stays on each edge and cycles once
        // through all of each edge's edgeuses; every edge has at least one edgeuse
        let mut per_edge = vec![0i64; s.edges];
        let mut hit = vec![false; s.edgeuses];
        let mut first = vec![-1i64; s.edges];
        for i in 0..s.edgeuses {
            let e = *eu_edge.get(i)? as usize;
            let n = *eu_next.get(i)? as usize;
            if eu_edge.get(n)? != eu_edge.get(i)? || *hit.get(n)? {
                return None;
            }
            *hit.get_mut(n)? = true;
            *per_edge.get_mut(e)? += 1;
            if *first.get(e)? < 0 {
                *first.get_mut(e)? = i as i64;
            }
        }
        for e in 0..s.edges {
            let start = *first.get(e)?;
            if start < 0 {
                return None;
            }
            let (mut steps, mut cur) = (0i64, start);
            loop {
                cur = *eu_next.get(cur as usize)?;
                steps += 1;
                if cur == start || steps > *per_edge.get(e)? {
                    break;
                }
            }
            if steps != *per_edge.get(e)? {
                return None;
            }
        }
    }

    // every vertex is used; every edgeuse loop is a closed chain of its edges' vertices
    {
        let ev = f.i(EDGE_VERTICES);
        let mut used = vec![false; s.vertices];
        for &v in ev.iter().chain(f.i(WIRE_EDGE_VERTICES)) {
            *used.get_mut(v as usize)? = true;
        }
        let mut eu = 0usize;
        for l in 0..s.loops {
            let n = *lp_count.get(l)? as usize;
            if n == 0 {
                *used.get_mut(*lp_vertex.get(l)? as usize)? = true;
                continue;
            }
            for k in 0..n {
                let (a, b) = (eu + k, eu + (k + 1) % n);
                let (ea, eb) = (*eu_edge.get(a)? as usize, *eu_edge.get(b)? as usize);
                let end_a = ev.get(2 * ea + usize::from(*eu_orient.get(a)? == 1))?;
                let start_b = ev.get(2 * eb + usize::from(*eu_orient.get(b)? != 1))?;
                if end_a != start_b {
                    return None;
                }
            }
            eu += n;
        }
        if !used.iter().all(|&u| u) {
            return None;
        }
    }

    // UV trim curves: none, or one record per edgeuse
    let (uv_count, uv_order) = (f.i(EDGEUSE_NURBS_VERTEX_COUNT), f.i(EDGEUSE_NURBS_ORDER));
    if uv_count.len() != uv_order.len() || (!uv_count.is_empty() && uv_count.len() != s.edgeuses) {
        return None;
    }
    s.uv_curves = !uv_count.is_empty();
    if uv_count.iter().zip(uv_order).any(|(&c, &o)| (c == 0) != (o == 0)) {
        return None;
    }
    if !good_nurbs_curves(uv_count, uv_order, f.r(EDGEUSE_NURBS_CVS), f.r(EDGEUSE_NURBS_KNOTS), f.r(EDGEUSE_NURBS_WEIGHTS), 2, true) {
        return None;
    }

    // Surfaces
    let mut n_t = [0usize; 6];
    for &t in surface_type {
        *n_t.get_mut(t as usize)? += 1;
    }
    let n_n = (0..s.faces).filter(|&i| face_has_nurbs(f, i)).count();
    let [_, n_p, n_cy, n_co, n_s, n_to] = n_t;
    let (cu_v, cv_v, ou_v, ov_v) = (f.i(FACE_NURBS_U_VERTEX_COUNT), f.i(FACE_NURBS_V_VERTEX_COUNT), f.i(FACE_NURBS_U_ORDER), f.i(FACE_NURBS_V_ORDER));
    if cu_v.len() != n_n || cv_v.len() != n_n || ou_v.len() != n_n || ov_v.len() != n_n {
        return None;
    }
    {
        let (uks, vks) = (f.r(FACE_NURBS_U_KNOTS), f.r(FACE_NURBS_V_KNOTS));
        let (mut points, mut uk, mut vk) = (0i64, 0i64, 0i64);
        for i in 0..n_n {
            let (cu, cv, ou, ov) = (*cu_v.get(i)?, *cv_v.get(i)?, *ou_v.get(i)?, *ov_v.get(i)?);
            if !(2..=MAX_ORDER).contains(&ou)
                || !(2..=MAX_ORDER).contains(&ov)
                || cu < ou
                || cv < ov
                || cu > MAX_FLAT_LEN
                || cv > MAX_FLAT_LEN
                || cu * cv > MAX_FLAT_LEN
            {
                return None;
            }
            points += cu * cv;
            uk += cu + ou;
            vk += cv + ov;
            if points > MAX_FLAT_LEN || uk > MAX_FLAT_LEN || vk > MAX_FLAT_LEN {
                return None;
            }
        }
        if f.r(FACE_NURBS_CVS).len() as i64 != 3 * points || uks.len() as i64 != uk || vks.len() as i64 != vk {
            return None;
        }
        let w = f.r(FACE_NURBS_WEIGHTS);
        if !w.is_empty() && (w.len() as i64 != points || !good_weights(w)) {
            return None;
        }
        let (mut uk, mut vk) = (0usize, 0usize);
        for i in 0..n_n {
            let (cu, cv, ou, ov) = (*cu_v.get(i)?, *cv_v.get(i)?, *ou_v.get(i)?, *ov_v.get(i)?);
            if !good_knots(uks.get(uk..)?, cu, ou) || !good_knots(vks.get(vk..)?, cv, ov) {
                return None;
            }
            uk += (cu + ou) as usize;
            vk += (cv + ov) as usize;
        }
        // each NURBS face's range overlaps its surface's knot domain (MakeTopologyFromData
        // intersects the two)
        let (mut i_n, mut uk, mut vk) = (0usize, 0usize, 0usize);
        for i in 0..s.faces {
            if !face_has_nurbs(f, i) {
                continue;
            }
            let (cu, cv, ou, ov) = (*cu_v.get(i_n)? as usize, *cv_v.get(i_n)? as usize, *ou_v.get(i_n)? as usize, *ov_v.get(i_n)? as usize);
            let (u_lo, u_hi) = (*uks.get(uk + ou - 1)?, *uks.get(uk + cu)?);
            let (v_lo, v_hi) = (*vks.get(vk + ov - 1)?, *vks.get(vk + cv)?);
            if !(range.get(4 * i + 2)?.min(u_hi) > range.get(4 * i)?.max(u_lo))
                || !(range.get(4 * i + 3)?.min(v_hi) > range.get(4 * i + 1)?.max(v_lo))
            {
                return None;
            }
            uk += cu + ou;
            vk += cv + ov;
            i_n += 1;
        }
    }
    let lens = |id: i32, n: usize| f.r(id).len() == n;
    if !(lens(FACE_PLANE_ORIGIN, 3 * n_p)
        && lens(FACE_PLANE_AXIS, 3 * n_p)
        && lens(FACE_PLANE_REF_DIRECTION, 3 * n_p)
        && lens(FACE_CYLINDER_ORIGIN, 3 * n_cy)
        && lens(FACE_CYLINDER_AXIS, 3 * n_cy)
        && lens(FACE_CYLINDER_REF_DIRECTION, 3 * n_cy)
        && lens(FACE_CYLINDER_RADIUS, n_cy)
        && lens(FACE_CONE_ORIGIN, 3 * n_co)
        && lens(FACE_CONE_AXIS, 3 * n_co)
        && lens(FACE_CONE_REF_DIRECTION, 3 * n_co)
        && lens(FACE_CONE_RADIUS, n_co)
        && lens(FACE_CONE_SEMI_ANGLE, n_co)
        && lens(FACE_SPHERE_CENTER, 3 * n_s)
        && lens(FACE_SPHERE_AXIS, 3 * n_s)
        && lens(FACE_SPHERE_REF_DIRECTION, 3 * n_s)
        && lens(FACE_SPHERE_RADIUS, n_s)
        && lens(FACE_TORUS_ORIGIN, 3 * n_to)
        && lens(FACE_TORUS_AXIS, 3 * n_to)
        && lens(FACE_TORUS_REF_DIRECTION, 3 * n_to)
        && lens(FACE_TORUS_MAJOR_RADIUS, n_to)
        && lens(FACE_TORUS_MINOR_RADIUS, n_to))
    {
        return None;
    }
    let positive = |id: i32, i: usize| f.r(id).get(i).is_some_and(|&r| r > 0.0);
    for i in 0..n_p {
        if !good_frame(f.r(FACE_PLANE_AXIS), f.r(FACE_PLANE_REF_DIRECTION), i) {
            return None;
        }
    }
    for i in 0..n_cy {
        if !positive(FACE_CYLINDER_RADIUS, i) || !good_frame(f.r(FACE_CYLINDER_AXIS), f.r(FACE_CYLINDER_REF_DIRECTION), i) {
            return None;
        }
    }
    for i in 0..n_co {
        let half = 0.5 * std::f64::consts::PI * (1.0 - 1e-9);
        if !f.r(FACE_CONE_RADIUS).get(i).is_some_and(|&r| r >= 0.0)
            || !f.r(FACE_CONE_SEMI_ANGLE).get(i).is_some_and(|a| a.abs() < half)
            || !good_frame(f.r(FACE_CONE_AXIS), f.r(FACE_CONE_REF_DIRECTION), i)
        {
            return None;
        }
    }
    for i in 0..n_s {
        if !positive(FACE_SPHERE_RADIUS, i) || !good_frame(f.r(FACE_SPHERE_AXIS), f.r(FACE_SPHERE_REF_DIRECTION), i) {
            return None;
        }
    }
    for i in 0..n_to {
        if !positive(FACE_TORUS_MAJOR_RADIUS, i)
            || !positive(FACE_TORUS_MINOR_RADIUS, i)
            || !good_frame(f.r(FACE_TORUS_AXIS), f.r(FACE_TORUS_REF_DIRECTION), i)
        {
            return None;
        }
    }
    Some(s)
}

// ---- import: FlatToBrepData --------------------------------------------------------------------

/// The running read positions in the packed geometry arrays (`Cursor`).
#[derive(Default)]
struct Cursor {
    nurbs: usize,
    cv: usize,
    knot: usize,
    vknot: usize,
    circle: usize,
    line: usize,
    ellipse: usize,
}

/// `smu_IsFlatKnotVectorClamped`: the first and last knots each repeat `degree + 1` times.
fn is_clamped(knots: &[f64], degree: usize) -> bool {
    let n = knots.len();
    if n < 2 || degree > (n - 2) / 2 {
        return false;
    }
    let (Some(&first), Some(&last)) = (knots.first(), knots.last()) else { return false };
    (1..=degree).all(|i| knots.get(i) == Some(&first) && knots.get(n - 1 - i) == Some(&last))
}

/// `smu_CompressFlatKnots`: the distinct knots and their multiplicities.
fn compress_knots(knots: &[f64]) -> (Vec<f64>, Vec<usize>) {
    let (mut unique, mut mults): (Vec<f64>, Vec<usize>) = (Vec::new(), Vec::new());
    for &k in knots {
        match (unique.last(), mults.last_mut()) {
            (Some(&u), Some(m)) if u == k => *m += 1,
            _ => {
                unique.push(k);
                mults.push(1);
            }
        }
    }
    (unique, mults)
}

/// `smu_CreateSmBSplineCurve_FromArrays`: a curve of `dim` 2 or 3; an unclamped knot vector goes
/// through `SmBSplineCurve::CreateCanonical`, which clamps it.
fn bspline_curve_from_arrays(points: &[Point3], weights: &[f64], knots: &[f64], degree: usize, dim: u64) -> Result<BSplineCurve> {
    if !is_clamped(knots, degree) {
        let (unique, mults) = compress_knots(knots);
        let data = CanonicalData {
            dim,
            degree,
            control_points: points,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &mults,
            knots: &unique,
            knot_type: KnotType::Unspecified,
            weights: Some(weights),
            trim: None,
        };
        return BSplineCurve::create_canonical(&data).map_err(sc("SmBSplineCurve::CreateCanonical"));
    }
    let mut pw = Vec::with_capacity(points.len());
    for (p, &w) in points.iter().zip(weights) {
        let z = if dim == 3 { if w == 1.0 { p.z } else { w * p.z } } else { NOZ };
        pw.push(if w == 1.0 { CPoint::new(p.x, p.y, z, 1.0) } else { CPoint::new(w * p.x, w * p.y, z, w) });
    }
    let nurb = NurbCurve::new(pw, degree, knots.to_vec()).map_err(sc("sm_AllocateNurbCurve"))?;
    let mut c = BSplineCurve::new(dim);
    c.set_from_gw_nurb(Some(&nurb)).map_err(sc("SmBSplineCurve::SetFromGwNurb"))?;
    Ok(c)
}

/// `smu_CreateSmBSplineSurface_FromArrays`: control points U-major; an unclamped knot vector goes
/// through `SmBSplineSurface::CreateCanonical`.
fn bspline_surface_from_arrays(
    points: &[Point3],
    weights: &[f64],
    u_knots: &[f64],
    v_knots: &[f64],
    u_degree: usize,
    v_degree: usize,
) -> Result<BSplineSurface> {
    if !is_clamped(u_knots, u_degree) || !is_clamped(v_knots, v_degree) {
        let (uu, um) = compress_knots(u_knots);
        let (vu, vm) = compress_knots(v_knots);
        let data = CanonicalSurfaceData {
            u_degree,
            v_degree,
            control_points: points,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &um,
            v_multiplicities: &vm,
            u_knots: &uu,
            v_knots: &vu,
            knot_type: KnotType::Unspecified,
            weights: Some(weights),
            uv_domain: None,
        };
        return BSplineSurface::create_canonical(&data).map_err(sc("SmBSplineSurface::CreateCanonical"));
    }
    let nu = u_knots.len().checked_sub(u_degree + 1).ok_or_else(|| bad("u knots"))?;
    let nv = v_knots.len().checked_sub(v_degree + 1).ok_or_else(|| bad("v knots"))?;
    let mut net = Vec::with_capacity(nu);
    for i in 0..nu {
        let mut row = Vec::with_capacity(nv);
        for j in 0..nv {
            let at = i * nv + j;
            let (p, w) = (get(points, at)?, get(weights, at)?);
            row.push(CPoint::new(w * p.x, w * p.y, w * p.z, w));
        }
        net.push(row);
    }
    let nurb = NurbSurface::new(net, u_degree, v_degree, u_knots.to_vec(), v_knots.to_vec()).map_err(sc("sm_AllocateNurbSurface"))?;
    Ok(BSplineSurface::from_nurb(nurb, false))
}

/// The packed control points `[c, c + n)` of `dim` coordinates (z 0 for 2D) and their weights,
/// all 1 without a weight array.
fn packed_points(cvs: &[f64], weights: &[f64], c: usize, n: usize, dim: usize, reversed: bool) -> Result<(Vec<Point3>, Vec<f64>)> {
    let weighted = weights.len() * dim == cvs.len();
    let (mut pts, mut ws) = (Vec::with_capacity(n), Vec::with_capacity(n));
    for j in 0..n {
        let src = c + if reversed { n - 1 - j } else { j };
        let p = if dim == 3 { get3(cvs, src)? } else { Vec3::new(get(cvs, 2 * src)?, get(cvs, 2 * src + 1)?, 0.0) };
        pts.push(p);
        ws.push(if weighted { get(weights, src)? } else { 1.0 });
    }
    Ok((pts, ws))
}

/// `smu_CreateSmBSplineCurve3d`: the next NURBS curve of an edge family.
fn bspline_curve_3d(e: &EdgeIn<'_>, c: &mut Cursor) -> Result<BSplineCurve> {
    let degree = idx(get(e.order, c.nurbs)? - 1)?;
    let n = idx(get(e.count, c.nurbs)?)?;
    let n_knots = n + degree + 1;
    let (points, weights) = packed_points(e.cvs, e.weights, c.cv, n, 3, false)?;
    let knots = e.knots.get(c.knot..c.knot + n_knots).ok_or_else(|| bad("edge knots"))?;
    let curve = bspline_curve_from_arrays(&points, &weights, knots, degree, 3);
    c.nurbs += 1;
    c.cv += n;
    c.knot += n_knots;
    curve
}

/// `smu_CreateSmBSplineCurve2d_ForEdgeuseIndex`: edgeuse `k`'s UV trim, reversed onto its edge's
/// direction and its knots mapped affinely onto the edge's range (in SMLib units); `None` when it
/// has none, or its face no NURBS block (an analytic face's trims are rebuilt from the edges).
fn bspline_curve_2d(f: &In<'_>, c: &mut Cursor, k: usize, face_has_nurbs: bool, same: bool) -> Result<Option<BSplineCurve>> {
    let (order, n) = (get(f.i(EDGEUSE_NURBS_ORDER), k)?, get(f.i(EDGEUSE_NURBS_VERTEX_COUNT), k)?);
    if order == 0 || n == 0 {
        return Ok(None);
    }
    let degree = idx(order - 1)?;
    let n = idx(n)?;
    let n_knots = n + degree + 1;
    let (cv, knot) = (c.cv, c.knot);
    c.cv += n;
    c.knot += n_knots;
    if !face_has_nurbs {
        return Ok(None);
    }
    let (points, weights) = packed_points(f.r(EDGEUSE_NURBS_CVS), f.r(EDGEUSE_NURBS_WEIGHTS), cv, n, 2, !same)?;
    // SMLib requires every UV trim to run over its edge's interval: map the knots affinely onto
    // the edge's range (degrees for an analytic circle or ellipse)
    let e = idx(get(f.i(EDGEUSE_EDGE), k)?)?;
    let (mut lo, mut hi) = (get(f.r(EDGE_RANGE), 2 * e)?, get(f.r(EDGE_RANGE), 2 * e + 1)?);
    let ty = get(f.i(EDGE_CURVE_TYPE), e)?;
    if (ty == CURVE_CIRCLE_I || ty == CURVE_ELLIPSE_I) && !has_form(f.i(EDGE_NURBS_FORM), e) {
        lo = radians_to_smlib_degrees(lo);
        hi = radians_to_smlib_degrees(hi);
    }
    let src = f.r(EDGEUSE_NURBS_KNOTS).get(knot..knot + n_knots).ok_or_else(|| bad("trim knots"))?;
    let (s_lo, s_hi) = (get(src, degree)?, get(src, n)?);
    let scale = (hi - lo) / (s_hi - s_lo);
    let mut knots = Vec::with_capacity(n_knots);
    for j in 0..n_knots {
        let s = if same { get(src, j)? } else { s_lo + s_hi - get(src, n_knots - 1 - j)? };
        knots.push(lo + (s - s_lo) * scale);
    }
    bspline_curve_from_arrays(&points, &weights, &knots, degree, 2).map(Some)
}

/// `smu_CreateSmBSplineSurface3d`: the next NURBS face's surface.
fn bspline_surface_3d(f: &In<'_>, c: &mut Cursor) -> Result<BSplineSurface> {
    let ud = idx(get(f.i(FACE_NURBS_U_ORDER), c.nurbs)? - 1)?;
    let vd = idx(get(f.i(FACE_NURBS_V_ORDER), c.nurbs)? - 1)?;
    let nu = idx(get(f.i(FACE_NURBS_U_VERTEX_COUNT), c.nurbs)?)?;
    let nv = idx(get(f.i(FACE_NURBS_V_VERTEX_COUNT), c.nurbs)?)?;
    let (nuk, nvk, n) = (nu + ud + 1, nv + vd + 1, nu * nv);
    let (points, weights) = packed_points(f.r(FACE_NURBS_CVS), f.r(FACE_NURBS_WEIGHTS), c.cv, n, 3, false)?;
    let uk = f.r(FACE_NURBS_U_KNOTS).get(c.knot..c.knot + nuk).ok_or_else(|| bad("u knots"))?;
    let vk = f.r(FACE_NURBS_V_KNOTS).get(c.vknot..c.vknot + nvk).ok_or_else(|| bad("v knots"))?;
    let s = bspline_surface_from_arrays(&points, &weights, uk, vk, ud, vd);
    c.nurbs += 1;
    c.cv += n;
    c.knot += nuk;
    c.vknot += nvk;
    s
}

/// `SameNurbs`: equal sizes and orders, knots and weights within rounding, control points within
/// `tol`.
fn same_nurbs(a: &NurbSurface, b: &NurbSurface, tol: f64) -> bool {
    let (mut fa, mut fb) = (Out::new(), Out::new());
    if add_bspline_surface(&mut fa, a).is_err() || add_bspline_surface(&mut fb, b).is_err() {
        return false;
    }
    for id in [FACE_NURBS_U_VERTEX_COUNT, FACE_NURBS_V_VERTEX_COUNT, FACE_NURBS_U_ORDER, FACE_NURBS_V_ORDER] {
        if fa.i(id) != fb.i(id) {
            return false;
        }
    }
    let close = |x: &[f64], y: &[f64], t: f64| x.len() == y.len() && x.iter().zip(y).all(|(a, b)| (a - b).abs() <= t * a.abs().max(1.0));
    close(fa.r(FACE_NURBS_U_KNOTS), fb.r(FACE_NURBS_U_KNOTS), 1e-12)
        && close(fa.r(FACE_NURBS_V_KNOTS), fb.r(FACE_NURBS_V_KNOTS), 1e-12)
        && close(fa.r(FACE_NURBS_WEIGHTS), fb.r(FACE_NURBS_WEIGHTS), 1e-12)
        && close(fa.r(FACE_NURBS_CVS), fb.r(FACE_NURBS_CVS), tol)
}

/// `AnalyticOfNurbs`: the analytic surface of kind `ty` SMLib's own test finds in `nurbs`, kept
/// only when its NURBS is `nurbs` (the face's trims are in its parameters).
fn analytic_of_nurbs(k: &Kernel, nurbs: &BSplineSurface, ty: i64, tol: f64) -> Result<Option<Surface>> {
    let found: Option<Surface> = if ty == SURFACE_CONE_I {
        answer(Cone::is_nurb_surface_cone(k, nurbs, 1.0)).map_err(sc("SmCone::IsNurbSurfaceCone"))?.flatten().map(Surface::from)
    } else if ty == SURFACE_SPHERE_I {
        answer(Sphere::is_nurb_surface_sphere(k, nurbs, 1.0)).map_err(sc("SmSphere::IsNurbSurfaceSphere"))?.flatten().map(Surface::from)
    } else if ty == SURFACE_CYLINDER_I {
        answer(Cone::is_nurb_surface_cylinder(k, nurbs, 1.0)).map_err(sc("SmCylinder::IsNurbSurfaceCylinder"))?.flatten().map(Surface::from)
    } else if ty == SURFACE_TORUS_I {
        answer(Torus::is_nurb_surface_torus(k, nurbs, 1.0)).map_err(sc("SmTorus::IsNurbSurfaceTorus"))?.flatten().map(Surface::from)
    } else {
        None
    };
    let Some(found) = found else { return Ok(None) };
    let (Some(want), Some(part)) = (nurbs.gw_nurb(), bspline_surface_part(k, &found)?) else { return Ok(None) };
    Ok(part.gw_nurb().is_some_and(|got| same_nurbs(want, got, tol)).then_some(found))
}

/// `Placement`: X the reference direction, Y the axis cross it (`SetCanonical`).
fn placement(origin: Point3, z: Vec3, x: Vec3) -> Result<Axis2Placement> {
    let mut p = Axis2Placement::default();
    p.set_canonical(origin, x, z.cross(x)).map_err(|_| bad("a frame that spans no plane"))?;
    Ok(p)
}

/// `SameNurbsCurve`: equal counts and orders, knots and weights within rounding, control points
/// within rounding of the curve's size (`smu_AddSmBSplineCurve3d` of each).
fn same_nurbs_curve(a: &NurbCurve, b: &NurbCurve) -> bool {
    let arrays = |n: &NurbCurve| {
        let (mut count, mut order, mut cvs, mut knots, mut weights) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let o = NurbsOut { count: &mut count, order: &mut order, cvs: &mut cvs, knots: &mut knots, weights: &mut weights };
        add_bspline_curve_3d(n, o).ok().map(|()| (count, order, cvs, knots, weights))
    };
    let (Some((ac, ao, ap, ak, aw)), Some((bc, bo, bp, bk, bw))) = (arrays(a), arrays(b)) else { return false };
    if ac != bc || ao != bo || ap.len() != bp.len() || ak.len() != bk.len() || aw.len() != bw.len() {
        return false;
    }
    let size = ap.iter().fold(1.0_f64, |m, x| m.max(x.abs()));
    let near = |x: &[f64], y: &[f64]| x.iter().zip(y).all(|(a, b)| (a - b).abs() <= 1e-12 * a.abs().max(1.0));
    ap.iter().zip(&bp).all(|(a, b)| (a - b).abs() <= 1e-12 * size) && near(&ak, &bk) && near(&aw, &bw)
}

/// `MakeEdgeCurve` (capi/src/smlib_flat.cpp): one edge's 3D curve, as its kind says;
/// `d_min`/`d_max` its range in SMLib units. A circle or ellipse with its NURBS form (`form`) is that NURBS as the conic of its
/// canonical entry (frame and radii; the domain from `ConicDomainOfNurbs`), keeping that NURBS.
/// A circle's NURBS must pass SMLib's own test (`SmCircle::IsNurbCurveCircle`), whose circle is
/// not the one kept (its frame has its x axis towards the NURBS's start), and the kept circle
/// must hold the same NURBS (`SameNurbsCurve`); either failing is `SM_ERR_INVALID_INPUT`.
fn make_edge_curve(k: &Kernel, ty: i64, d_min: f64, d_max: f64, e: &EdgeIn<'_>, c: &mut Cursor, form: bool) -> Result<Curve> {
    if form {
        let circle = ty == CURVE_CIRCLE_I;
        let i = if circle {
            c.circle += 1;
            c.circle - 1
        } else {
            c.ellipse += 1;
            c.ellipse - 1
        };
        let nurbs = bspline_curve_3d(e, c)?;
        let (o, mut z, mut x, rx, ry) = if circle {
            let r = get(e.circle_radius, i)?;
            (get3(e.circle_center, i)?, get3(e.circle_axis, i)?, get3(e.circle_ref, i)?, r, r)
        } else {
            (get3(e.ellipse_center, i)?, get3(e.ellipse_axis, i)?, get3(e.ellipse_ref, i)?, get(e.ellipse_x, i)?, get(e.ellipse_y, i)?)
        };
        if circle {
            let test = Curve::from(nurbs.clone_exact());
            let is_circle = answer(Ellipse::is_nurb_curve_circle(k, &test, None)).map_err(sc("SmCircle::IsNurbCurveCircle"))?.flatten();
            if is_circle.is_none() {
                return Err(bad("a circle NURBS SMLib's circle test rejects"));
            }
        }
        let domain = conic_domain_of_nurbs(&nurbs, o, &mut z, &mut x, rx, ry).ok_or_else(|| bad("a conic NURBS off its conic"))?;
        let opts = EllipseOptions { dim: 3, nurb: Some(&nurbs), inside_out: false };
        if circle {
            let cir = Ellipse::new_circle(k, o, x, z.cross(x), domain, rx, &opts).map_err(sc("SmCircle::SmCircle"))?;
            let kept = nurb_of(cir.base())?;
            if !nurbs.gw_nurb().is_some_and(|want| same_nurbs_curve(want, kept)) {
                return Err(bad("a circle that does not keep its NURBS"));
            }
            return Ok(Curve::from(cir));
        }
        let el = Ellipse::new(k, o, x, z.cross(x), domain, rx, ry, &opts).map_err(sc("SmEllipse::SmEllipse"))?;
        return Ok(Curve::from(el));
    }
    if ty == CURVE_NURBS_I {
        return bspline_curve_3d(e, c).map(Curve::from);
    }
    if ty == CURVE_CIRCLE_I {
        let i = c.circle;
        c.circle += 1;
        let r = get(e.circle_radius, i)?;
        return build_analytic_edge_curve(k, ty, d_min, d_max, get3(e.circle_center, i)?, get3(e.circle_axis, i)?, get3(e.circle_ref, i)?, r, r);
    }
    if ty == CURVE_LINE_I {
        let i = c.line;
        c.line += 1;
        let zero = Vec3::new(0.0, 0.0, 0.0);
        return build_analytic_edge_curve(k, ty, d_min, d_max, get3(e.line_origin, i)?, get3(e.line_direction, i)?, zero, 0.0, 0.0);
    }
    let i = c.ellipse;
    c.ellipse += 1;
    build_analytic_edge_curve(
        k,
        ty,
        d_min,
        d_max,
        get3(e.ellipse_center, i)?,
        get3(e.ellipse_axis, i)?,
        get3(e.ellipse_ref, i)?,
        get(e.ellipse_x, i)?,
        get(e.ellipse_y, i)?,
    )
}

/// `ConicDomainOfNurbs`: the domain in degrees of the conic (centre `o`, axis `z`, reference `x`,
/// radii `rx`, `ry`) the NURBS traces, `z` and `x` made an orthonormal frame; `None` when a point
/// is off that conic or the NURBS turns clockwise about `z`.
fn conic_domain_of_nurbs(nurbs: &BSplineCurve, o: Point3, z: &mut Vec3, x: &mut Vec3, rx: f64, ry: f64) -> Option<Extent1d> {
    *z = z.unitize().ok()?;
    *x = (*x - *z * x.dot(*z)).unitize().ok()?;
    if !(rx > 0.0 && ry > 0.0) {
        return None;
    }
    let y = z.cross(*x);
    let ivl = nurbs.natural_interval();
    let angle = |t: f64| -> Option<f64> {
        let p = nurbs.evaluate_point(t).ok()?;
        let d = p - o;
        let (u, v) = (d.dot(*x) / rx, d.dot(y) / ry);
        let on = ((u.hypot(v)) - 1.0).abs() <= 1e-9 && d.dot(*z).abs() <= 1e-9 * (1.0 + rx + ry);
        on.then(|| v.atan2(u) * 180.0 / std::f64::consts::PI)
    };
    let n = 16;
    let (mut a0, mut prev) = (0.0, 0.0);
    for step in 0..=n {
        let a = angle(ivl.min() + (ivl.max() - ivl.min()) * f64::from(step) / f64::from(n))?;
        if step == 0 {
            a0 = a;
        } else if !(ieee_remainder(a - prev, 360.0) > 0.0) {
            return None;
        }
        prev = a;
    }
    let mut sweep = (prev - a0 + 720.0) % 360.0;
    if sweep <= 1e-9 {
        sweep = 360.0;
    }
    if a0 + sweep > 360.0 { Some(Extent1d::from_raw(a0 - 360.0, a0 - 360.0 + sweep)) } else { Some(Extent1d::from_raw(a0, a0 + sweep)) }
}

/// `std::remainder`: `x - n * y` with `n` the integer nearest `x / y`, ties to even.
fn ieee_remainder(x: f64, y: f64) -> f64 {
    let q = x / y;
    let mut n = q.round();
    if (q - q.trunc()).abs() == 0.5 && n % 2.0 != 0.0 {
        n -= q.signum();
    }
    x - n * y
}

/// `FlatToBrepData`: the validated arrays as `SmBrepData`. Edges are `[face edges, wire edges]`,
/// vertices `[point-shell vertices, vertices]`, edgeuses `[wire edgeuses, face edgeuses]`.
fn flat_to_brep_data(k: &Kernel, f: &In<'_>, s: &Shape) -> Result<BrepData> {
    let xsect = get(f.r(TOLERANCE), 0)?;
    let zone = xsect / 2.0;
    let mut d = BrepData::new(UNSURE, DataSourceType::default());
    d.data_source_type = DataSourceType::Usd;
    d.zone_tol_3d = zone;
    d.approx_tol_3d = xsect / 4.0;
    d.start_region = 0;
    d.num_regions = s.regions as u64;
    d.color = Vec3::new(0.0, 0.0, 0.0);
    d.is_geom_borrowed = false;
    let mut next_key = 1u64;
    let mut key = || {
        next_key += 1;
        ObjectKey(next_key - 1)
    };
    let (n_edges, n_wire, n_point) = (s.edges as u64, s.wire_edges as u64, s.point_shells as u64);
    let undefined = |t: &mut solidcore::topology::brep_data::TopologyData| {
        t.flags = 0;
        t.user_index1 = 0;
        t.user_index2 = 0;
    };

    // Regions
    let mut start_shell = 0u64;
    for i in 0..s.regions {
        let mut r = RegionData::default();
        r.start_shell = start_shell;
        r.num_shells = get(f.i(REGION_SHELL_COUNT), i)? as u64;
        r.is_void = i32::from(get(f.i(REGION_TYPE), i)? == 1);
        undefined(&mut r.topology);
        start_shell += r.num_shells;
        d.regions.push(r);
    }

    // Shells, the wire edges' edgeuses (first in the edgeuse list) and point-shell vertices
    let mut faceuse_start = 0u64;
    let mut wire = 0u64;
    let mut point = 0usize;
    for i in 0..s.shells {
        let mut sd = ShellData::default();
        let (n_fu, n_we) = (get(f.i(SHELL_FACEUSE_COUNT), i)? as u64, get(f.i(SHELL_WIRE_EDGE_COUNT), i)? as u64);
        sd.shell_type = if n_fu > 0 {
            0
        } else if n_we > 0 {
            1
        } else {
            2
        };
        sd.num_faceuses = 0;
        undefined(&mut sd.topology);
        if n_fu > 0 {
            sd.faceuse_start = faceuse_start;
            sd.num_faceuses = n_fu;
            faceuse_start += n_fu;
        }
        if n_we > 0 {
            sd.edge = wire + n_edges;
            for _ in 0..n_we {
                let mut eu = EuData::default();
                eu.eu_type = 2;
                eu.edge = n_edges + wire;
                eu.shell = i as u64;
                eu.orientation = 1;
                undefined(&mut eu.topology);
                eu.mate_flags = 0;
                eu.mate_user_index1 = 0;
                eu.mate_user_index2 = 0;
                eu.uv_curve = NO_OBJECT;
                d.edgeuses.push(eu);
                wire += 1;
            }
        }
        if sd.shell_type == 2 {
            sd.vertex = point as u64;
            let mut v = VertexData::default();
            v.zone_tol_3d = zone;
            undefined(&mut v.topology);
            v.point = get3(f.r(SHELL_POINT_POSITION), point)?;
            d.vertices.push(v);
            point += 1;
        }
        d.shells.push(sd);
    }

    // Faceuses
    for i in 0..s.faceuses {
        let mut fu = FaceuseData::default();
        fu.face = get(f.i(FACEUSE_FACE), i)? as u64;
        fu.orientation = i32::from(get(f.i(FACEUSE_ORIENTATION), i)? == 1);
        undefined(&mut fu.topology);
        d.faceuses.push(fu);
    }

    // Faces and their surfaces
    let mut start_loop = 0u64;
    let mut nurbs_cursor = Cursor::default();
    let (mut i_sphere, mut i_plane, mut i_cyl, mut i_cone, mut i_torus) = (0usize, 0usize, 0usize, 0usize, 0usize);
    for i in 0..s.faces {
        let ty = get(f.i(FACE_SURFACE_TYPE), i)?;
        let form = ty != SURFACE_NURBS_I && face_has_nurbs(f, i);
        // "rectangular" -> TRUE; "general" -> UNSURE (not determined)
        let rectangular = if get(f.i(FACE_TRIM_TYPE), i)? == 1 { 1 } else { UNSURE };
        let r = f.r(FACE_RANGE);
        let (mut u0, mut v0, mut u1, mut v1) = (get(r, 4 * i)?, get(r, 4 * i + 1)?, get(r, 4 * i + 2)?, get(r, 4 * i + 3)?);
        // analytic angles: radians to SMLib's degrees (a NURBS form's range is its own)
        if !form {
            if ty == SURFACE_SPHERE_I || ty == SURFACE_TORUS_I {
                (u0, v0, u1, v1) =
                    (radians_to_smlib_degrees(u0), radians_to_smlib_degrees(v0), radians_to_smlib_degrees(u1), radians_to_smlib_degrees(v1));
            } else if ty == SURFACE_CYLINDER_I || ty == SURFACE_CONE_I {
                (u0, u1) = (radians_to_smlib_degrees(u0), radians_to_smlib_degrees(u1));
            }
        }
        let mut domain = extent2d(u0, v0, u1, v1)?;
        let mut fd = FaceData::default();
        fd.zone_tol_3d = zone;
        fd.surface = d.surfaces.len() as u64;
        fd.start_loop = start_loop;
        fd.num_loops = get(f.i(FACE_LOOP_COUNT), i)? as u64;
        fd.uv_domain = domain;
        fd.rectangular_trim = rectangular;
        undefined(&mut fd.topology);

        let mut surface: Surface = if form {
            // an analytic face with its NURBS form: that NURBS as the class the arrays name
            i_sphere += usize::from(ty == SURFACE_SPHERE_I);
            i_cyl += usize::from(ty == SURFACE_CYLINDER_I);
            i_cone += usize::from(ty == SURFACE_CONE_I);
            i_torus += usize::from(ty == SURFACE_TORUS_I);
            let nurbs = bspline_surface_3d(f, &mut nurbs_cursor)?;
            analytic_of_nurbs(k, &nurbs, ty, xsect)?.ok_or_else(|| bad("a NURBS form that is not its analytic surface"))?
        } else if ty == SURFACE_SPHERE_I {
            let j = i_sphere;
            i_sphere += 1;
            let p = placement(get3(f.r(FACE_SPHERE_CENTER), j)?, get3(f.r(FACE_SPHERE_AXIS), j)?, get3(f.r(FACE_SPHERE_REF_DIRECTION), j)?)?;
            Surface::from(Sphere::create_canonical(k, &p, get(f.r(FACE_SPHERE_RADIUS), j)?).map_err(sc("SmSphere::CreateCanonical"))?)
        } else if ty == SURFACE_PLANE_I {
            let j = i_plane;
            i_plane += 1;
            let p = placement(get3(f.r(FACE_PLANE_ORIGIN), j)?, get3(f.r(FACE_PLANE_AXIS), j)?, get3(f.r(FACE_PLANE_REF_DIRECTION), j)?)?;
            // its domain is restricted below without changing its parameterisation
            Surface::from(Plane::create_canonical(&p).map_err(sc("SmPlane::CreateCanonical"))?)
        } else if ty == SURFACE_CYLINDER_I {
            let j = i_cyl;
            i_cyl += 1;
            let p = placement(get3(f.r(FACE_CYLINDER_ORIGIN), j)?, get3(f.r(FACE_CYLINDER_AXIS), j)?, get3(f.r(FACE_CYLINDER_REF_DIRECTION), j)?)?;
            Surface::from(Cone::create_canonical_cylinder(k, &p, get(f.r(FACE_CYLINDER_RADIUS), j)?).map_err(sc("SmCylinder::CreateCanonical"))?)
        } else if ty == SURFACE_CONE_I {
            let j = i_cone;
            i_cone += 1;
            let mut origin = get3(f.r(FACE_CONE_ORIGIN), j)?;
            let mut z = get3(f.r(FACE_CONE_AXIS), j)?;
            let x = get3(f.r(FACE_CONE_REF_DIRECTION), j)?;
            let radius = get(f.r(FACE_CONE_RADIUS), j)?;
            let mut semi = rad2deg(get(f.r(FACE_CONE_SEMI_ANGLE), j)?);
            // a negative semi-angle is the same cone with its axis and both parameters reversed
            if semi < 0.0 {
                semi = -semi;
                z = Vec3::new(-z.x, -z.y, -z.z);
                domain = extent2d(-domain.u_max(), -domain.v_max(), -domain.u_min(), -domain.v_min())?;
            }
            let mut cone = None;
            if domain.is_bounded() {
                // the finite segment, built at its bottom and its axial parameter then [0, height]
                let (vmin, vmax) = (domain.v_min(), domain.v_max());
                let height = vmax - vmin;
                let tan = deg2rad(semi).tan();
                let (bottom, top) = (radius + vmin * tan, radius + vmax * tan);
                if height > EFF_ZERO && bottom >= -EFF_ZERO && top >= -EFF_ZERO {
                    origin += z * vmin;
                    let p = placement(origin, z, x)?;
                    cone = Some(
                        Cone::create_canonical(k, &p, bottom.max(0.0), top.max(0.0), height, false, false).map_err(sc("SmCone::CreateCanonical")),
                    );
                    domain = extent2d(domain.u_min(), 0.0, domain.u_max(), height)?;
                }
            } else {
                let p = placement(origin, z, x)?;
                cone = Some(Cone::create_canonical_with_semi_angle(k, &p, radius, semi, None, false, false).map_err(sc("SmCone::CreateCanonical")));
            }
            Surface::from(cone.ok_or_else(|| bad("a cone segment through its apex"))??)
        } else if ty == SURFACE_TORUS_I {
            let j = i_torus;
            i_torus += 1;
            let p = placement(get3(f.r(FACE_TORUS_ORIGIN), j)?, get3(f.r(FACE_TORUS_AXIS), j)?, get3(f.r(FACE_TORUS_REF_DIRECTION), j)?)?;
            let (major, minor) = (get(f.r(FACE_TORUS_MAJOR_RADIUS), j)?, get(f.r(FACE_TORUS_MINOR_RADIUS), j)?);
            Surface::from(Torus::create_canonical(k, &p, major, minor, TorusOptions::default(), None).map_err(sc("SmTorus::CreateCanonical"))?)
        } else {
            Surface::from(bspline_surface_3d(f, &mut nurbs_cursor)?)
        };

        // An analytic surface over its STEP range, and the face's domain its NURBS image (the
        // natural domain the topology build intersects with).
        if ty != SURFACE_NURBS_I && !form && domain.is_bounded() {
            surface.adjust_step_uv_domain(k, &domain).map_err(sc("AdjustSTEPUVDomain"))?;
            let nurbs_domain = surface.convert_domain_from_step_to_nurbs(k, &domain).map_err(sc("ConvertDomainFromSTEPToNURBS"))?;
            fd.uv_domain = extent2d(nurbs_domain.u_min(), nurbs_domain.v_min(), nurbs_domain.u_max(), nurbs_domain.v_max())?;
        }
        d.surfaces.push(Keyed { key: key(), object: surface });
        start_loop += fd.num_loops;
        d.faces.push(fd);
    }

    // Loops; each edgeuse's loop and face follow from the loop counts
    let mut eu_loop = vec![0u64; s.edgeuses];
    let mut eu_face = vec![0usize; s.edgeuses];
    // (face, vertex) of each vertex loop
    let mut loop_vertices = Vec::new();
    {
        let mut next_eu = n_wire;
        let (mut eu, mut face) = (0usize, 0usize);
        let mut face_loop_end = if s.faces > 0 { get(f.i(FACE_LOOP_COUNT), 0)? as usize } else { 0 };
        for l in 0..s.loops {
            while l >= face_loop_end && face + 1 < s.faces {
                face += 1;
                face_loop_end += get(f.i(FACE_LOOP_COUNT), face)? as usize;
            }
            let n_eu = get(f.i(LOOP_EDGEUSE_COUNT), l)? as u64;
            let mut ld = LoopData::default();
            ld.loop_type = if n_eu > 0 { 0 } else { 1 };
            ld.num_eu = n_eu;
            undefined(&mut ld.topology);
            if n_eu > 0 {
                ld.start_eu = next_eu;
                next_eu += n_eu;
                for _ in 0..n_eu {
                    *eu_loop.get_mut(eu).ok_or_else(|| bad("edgeuse count"))? = l as u64;
                    *eu_face.get_mut(eu).ok_or_else(|| bad("edgeuse count"))? = face;
                    eu += 1;
                }
            } else {
                // no SmEUData for a loop vertex
                ld.vertex = n_point + get(f.i(LOOP_VERTEX), l)? as u64;
                loop_vertices.push((face, ld.vertex));
            }
            d.loops.push(ld);
        }
    }

    // Edgeuses (face edges; the wire edges' came with their shells)
    let mut prim_eu = vec![NO_OBJECT as i64; s.edges];
    let mut uv_cursor = Cursor::default();
    let (eu_next, eu_entry) = (f.i(EDGEUSE_NEXT_RADIAL), f.i(EDGEUSE_RADIAL_ENTRY));
    for kk in 0..s.edgeuses {
        let here = (n_wire + kk as u64) as i64;
        let next = idx(get(eu_next, kk)?)?;
        let next_index = (n_wire as i64) + next as i64;
        let this_top = get(eu_entry, kk)? == 1;
        let next_top = get(eu_entry, next)? == 1;
        let next_flag = if next_top { next_index } else { -next_index - 1 };
        let same = get(f.i(EDGEUSE_ORIENTATION), kk)? == 1;
        let mut eu = EuData::default();
        eu.eu_type = 0;
        eu.edge = get(f.i(EDGEUSE_EDGE), kk)? as u64;
        eu.loop_ = get(&eu_loop, kk)?;
        eu.orientation = i32::from(same);
        eu.downward_lu = false;
        undefined(&mut eu.topology);
        // topEntry: top -> bottom -> next; bottomEntry: bottom -> top -> next
        if this_top {
            eu.next_eu = -here - 1;
            eu.mate_next_eu = next_flag;
        } else {
            eu.next_eu = next_flag;
            eu.mate_next_eu = here;
        }
        eu.uv_curve = NO_OBJECT;
        if s.uv_curves
            && let Some(trim) = bspline_curve_2d(f, &mut uv_cursor, kk, face_has_nurbs(f, get(&eu_face, kk)?), same)?
        {
            eu.uv_curve = d.uv_curves.len() as u64;
            d.uv_curves.push(Keyed { key: key(), object: Curve::from(trim) });
        }
        // the primary edgeuse is the first on the radial list (its mate the second)
        let slot = prim_eu.get_mut(index_u64(eu.edge)?).ok_or_else(|| bad("edgeuse edge"))?;
        if *slot == NO_OBJECT as i64 {
            *slot = if this_top { here } else { -here - 1 };
        }
        d.edgeuses.push(eu);
    }

    // Edges, then wire edges
    for (family, n, offset) in [(&EDGES, s.edges, 0u64), (&WIRE_EDGES, s.wire_edges, n_edges)] {
        let e = EdgeIn::new(f, family);
        let tolerances = if offset == 0 { f.r(EDGE_TOLERANCE) } else { &[] };
        let mut c = Cursor::default();
        for i in 0..n {
            let mut ed = EdgeData::default();
            ed.zone_tol_3d = if tolerances.is_empty() { zone } else { get(tolerances, i)? / 2.0 };
            let ty = get(e.ty, i)?;
            let form = has_form(e.form, i);
            let (mut lo, mut hi) = (get(e.range, 2 * i)?, get(e.range, 2 * i + 1)?);
            // circle and ellipse ranges are radians; SMLib's circles and ellipses take degrees
            if !form && (ty == CURVE_CIRCLE_I || ty == CURVE_ELLIPSE_I) {
                (lo, hi) = (radians_to_smlib_degrees(lo), radians_to_smlib_degrees(hi));
            }
            if !(lo <= hi) {
                return Err(bad("edge range"));
            }
            ed.interval = Extent1d::from_raw(lo, hi);
            ed.curve = d.curves_3d.len() as u64;
            ed.start_vertex = n_point + get(e.vertices, 2 * i)? as u64;
            ed.end_vertex = n_point + get(e.vertices, 2 * i + 1)? as u64;
            // a wire edge's edgeuse index is its index among the wire edges
            ed.prim_eu = if offset == 0 { get(&prim_eu, i)? } else { i as i64 };
            undefined(&mut ed.topology);
            let curve = make_edge_curve(k, ty, lo, hi, &e, &mut c, form)?;
            d.curves_3d.push(Keyed { key: key(), object: curve });
            d.edges.push(ed);
        }
    }

    // Vertices
    for v in 0..s.vertices {
        let mut vd = VertexData::default();
        vd.zone_tol_3d = match f.r(VERTEX_TOLERANCE) {
            [] => zone,
            t => get(t, v)? / 2.0,
        };
        undefined(&mut vd.topology);
        vd.point = get3(f.r(VERTEX_POSITION), v)?;
        d.vertices.push(vd);
    }
    own_tolerances(&mut d, f, &eu_face, &loop_vertices, n_point)?;
    Ok(d)
}

/// The B-rep's and each face's tolerance from the edges' and vertices' own, when the arrays carry
/// them. TOLERANCE is the largest tolerance in arrays SMLib writes, and MakeTopologyFromData
/// raises every entity to the B-rep's: taken as the B-rep's, it would become every entity's, and
/// each crossing between the kernels spread the largest over the whole body. Each written edge or
/// vertex tolerance is at least the B-rep's and its faces' (`ZoneTolWithFaces`), so the B-rep's is
/// at most the least of them and a face's at most the least of its edges' and loop vertices'.
fn own_tolerances(d: &mut BrepData, f: &In<'_>, eu_face: &[usize], loop_vertices: &[(usize, u64)], n_point: u64) -> Result<()> {
    if f.r(EDGE_TOLERANCE).is_empty() || f.r(VERTEX_TOLERANCE).is_empty() {
        return Ok(());
    }
    let zone = d.zone_tol_3d;
    let mut faces = vec![f64::INFINITY; d.faces.len()];
    let mut lower = |face: usize, t: f64| -> Result<()> {
        let slot = faces.get_mut(face).ok_or_else(|| bad("edgeuse face"))?;
        *slot = slot.min(t);
        Ok(())
    };
    for (kk, &face) in eu_face.iter().enumerate() {
        let edge = index_u64(get(f.i(EDGEUSE_EDGE), kk)? as u64)?;
        lower(face, d.edges.get(edge).ok_or_else(|| bad("edgeuse edge"))?.zone_tol_3d)?;
    }
    for &(face, vertex) in loop_vertices {
        lower(face, d.vertices.get(index_u64(vertex)?).ok_or_else(|| bad("loop vertex"))?.zone_tol_3d)?;
    }
    let face_edges = d.edges.len().min(f.r(EDGE_TOLERANCE).len());
    let own_vertices = d.vertices.iter().skip(index_u64(n_point)?).map(|v| v.zone_tol_3d);
    let least = d.edges.iter().take(face_edges).map(|e| e.zone_tol_3d).chain(own_vertices).fold(zone, f64::min);
    if !(least.is_finite() && least > 0.0) {
        return Ok(());
    }
    d.zone_tol_3d = least;
    for (fd, t) in d.faces.iter_mut().zip(faces) {
        fd.zone_tol_3d = if t.is_finite() { t.min(zone) } else { zone };
    }
    Ok(())
}

/// An `SmExtent2d` with its ends in order (`Set`, `SetMinMax`).
fn extent2d(u0: f64, v0: f64, u1: f64, v1: f64) -> Result<Extent2d> {
    if !(u0 <= u1 && v0 <= v1) {
        return Err(bad("a domain with its ends reversed"));
    }
    Ok(Extent2d::from_bounds(u0, v0, u1, v1))
}

/// Whether `Validate` refuses these arrays.
#[cfg(all(test, feature = "native"))]
pub(super) fn validate_refuses(a: &Arrays) -> bool {
    validate(&In(a)).is_none()
}

/// A body from the flat arrays the C API imports (`BrepFromFlat`): `Validate`,
/// `FlatToBrepData`, then `MakeTopologyFromData` with the B-rep's tolerance as it is ([`own_tolerances`]) and no UV
/// trims or analytic conversion made; it heals unless `SOURCE` says SMLib wrote the arrays.
pub(crate) fn from_arrays(a: &Arrays) -> Result<Body> {
    let f = In(a);
    let s = validate(&f).ok_or(Error::Record(crate::import::Unsupported::Invalid("flat arrays")))?;
    let b = run("BrepFromFlat", |k| Ok(brep_from_flat(k, &f, &s)))??;
    Ok(Body::new(b))
}

fn brep_from_flat(k: &mut Kernel, f: &In<'_>, s: &Shape) -> Result<Brep> {
    let mut data = flat_to_brep_data(k, f, s)?;
    // no heal for B-reps SMLib wrote
    let trusted = f.i(SOURCE).first() == Some(&1);
    data.set_healer_version(if trusted { HEALER_VERSION } else { HEALER_VERSION - 2 });
    let mut b = Brep::new_with_infinite_region().map_err(sc("SmBrep::SmBrep"))?;
    let mut store = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let next_key = (data.uv_curves.len() + data.curves_3d.len() + data.surfaces.len()) as u64 + 1;
    let mut env = TopologyEnv { all_attributes: &[], attributes: &mut store, key_of: &key_of, records: &mut records, next_key };
    let options = MakeTopologyOptions {
        make_uv_trim_curves: false,
        add_analytics: false,
        force_same_tol: true,
        check_uv_trim_curves: false,
        always_heal: false,
        healer_enabled: true,
    };
    make_topology_from_data(k, &mut b, &mut data, &mut env, options).map_err(sc("SmBrep::MakeTopologyFromData"))?;
    put_enclosing_shells_first(k, &mut b)?;
    Ok(b)
}

/// The most `SmShell::IsInnerShellOf` calls one conversion makes; a body that would need more
/// is refused instead of ray casting without end.
const MAX_CONTAINMENT_TESTS: usize = 1 << 16;

/// `PutEnclosingShellsFirst` (capi `smlib_flat.cpp`): SMLib takes the first shell of every region
/// but the infinite one as the shell that encloses the region, the others lying inside it
/// (`SmBrep::AssertValid` test 12). The arrays may list a region's shells in any order, so the one
/// that encloses all the others is put first, decided by `SmShell::IsInnerShellOf`
/// ([`shell::is_inner_shell_of`]). Candidates are the closed face-use shells whose bounding box
/// can be computed, largest box first (a stable sort: equal boxes keep the arrays' order). Only
/// `SM_TRUE` counts as inside: `SM_FALSE` and `SM_UNSURE` rule the candidate out, as the capi's
/// `== TRUE`, and a region where no candidate is shown to enclose the others keeps its order.
/// solidcore errors the C++ has no counterpart for (a call without a port, a dangling id) refuse
/// the conversion, as does a body needing more than [`MAX_CONTAINMENT_TESTS`] containment tests.
fn put_enclosing_shells_first(k: &mut Kernel, b: &mut Brep) -> Result<()> {
    use solidcore::query::face::p1::calculate_shell_bounding_box;
    let infinite = b.infinite_region();
    let mut tests = 0usize;
    for r in bl::regions(b, None).map_err(sc("GetRegions"))? {
        if Some(r) == infinite {
            continue;
        }
        let shells = b.region(r).map_err(sc("GetShells"))?.shells.clone();
        if shells.len() < 2 {
            continue;
        }
        let mut candidates: Vec<(f64, ShellId)> = Vec::new();
        for &s in &shells {
            let rec = b.shell(s).map_err(sc("GetShells"))?;
            if !rec.is_faceuse_shell() || !rec.is_closed(b, s).map_err(sc("SmShell::IsClosed"))? {
                continue;
            }
            match calculate_shell_bounding_box(k, b, s, false) {
                Ok(bx) => {
                    let size = bx.size();
                    candidates.push((size.x * size.y * size.z, s));
                }
                Err(e) if e.is_unported() => return Err(sc("SmShell::CalculateBoundingBox")(e)),
                // != SM_SUCCESS: not a candidate
                Err(_) => {}
            }
        }
        // stable: equal boxes keep the arrays' order
        candidates.sort_by(|a, c| c.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        for &(_, candidate) in &candidates {
            let mut encloses = true;
            for &s in &shells {
                if s == candidate {
                    continue;
                }
                tests += 1;
                if tests > MAX_CONTAINMENT_TESTS {
                    return Err(bad("too many shells to order (SmShell::IsInnerShellOf)"));
                }
                if shell::is_inner_shell_of(k, b, s, Some(candidate)).map_err(sc("SmShell::IsInnerShellOf"))? != shell::SM_TRUE {
                    encloses = false;
                    break;
                }
            }
            if encloses {
                // SmRegion::SetList(candidate): the circular list now starts at it
                let list = &mut b.region_mut(r).map_err(sc("SmRegion::SetList"))?.shells;
                if let Some(at) = list.iter().position(|&s| s == candidate) {
                    list.rotate_left(at);
                }
                break;
            }
        }
    }
    Ok(())
}
