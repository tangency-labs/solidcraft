//! Runner for unit `blend.fillet_solver_p4`: `SmFilletSolver::ComputePointValues` and the
//! early-exit / status paths of `SmFilletSolver::CalcFilletGeom`.
//!
//! C++: `solidcore_ffi::units::blend_fillet_solver_p4` (a real `SmConstantRadiusFS` over real
//! `SmPlane`, `SmBSplineSurface` and `SmSurfOfRevolution` surfaces; for the
//! finite-difference branch an `SmConstantRadiusFS` whose only override is
//! `OffsetRadiiCanChange() == TRUE`). Rust: [`FilletSolver::compute_point_values`] /
//! [`FilletSolver::calc_fillet_geom`] over the co-ported `ConstantRadiusAssistedFs`
//! (`SmConstantRadiusFS`'s members and virtuals), wrapped where the C++ solver overrides a
//! virtual.
//!
//! Ops (the `op` of a case):
//! - `point_values`: `ComputePointValues(uv, dom1, dom2, dir, point, prev)`. Compared:
//!   the status (`0` = `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust
//!   `InvalidInput`, `-1` for an error upstream cannot return), the whole point after the
//!   call (80 values, whatever the status) and both offset distances. Surfaces of
//!   revolution are built as unit `surface.surf_of_revolution_p1` builds them (cylinder,
//!   cone, sphere and torus shapes from line and circle generators); the build status is
//!   compared when one fails. Before every case that can reach the finite-difference
//!   block (finite-difference solver, incoming `SM_IP_TANGENT_POINT`, or a previous point)
//!   the C++ `sdStaticStep` is probed ([`ffi::static_step`], which latches it with `1e-5`
//!   when nothing has) and the case's [`Kernel::fillet_point_step`] is seeded with it, so
//!   both sides step with the same value whatever ran earlier in the process (ND-3).
//! - `calc_geom`: `CalcFilletGeom` on a rectangle sheet's edge (see the ffi module):
//!   a non-B-spline (`SmHermiteCurve`) edge curve, and a B-spline edge curve with a bare
//!   fillet geometry (no rails; G1 knots and split flag optional), which stops at the first
//!   rail at the latest. Compared: status, solver status, extension flags, the G1 knots
//!   after the call and the knots given. The Rust solver replays `SetExtensionFlags` (the
//!   C++ override) and imports the rectangle exported by the shim. With `uv_curves` the
//!   shim makes the face's UV trim curves first (`SmFace::CreateUVTrimCurves`), so both
//!   sides find them (and the G1 / split knots reach `EvaluateBinormal`); without, both
//!   sides build them (`GetOrCreateUVTrimCurve`, `topology.edgeuse_p3`).
//!
//! Deferred: the finite-difference cases on `StepSolve` (`blend.fillet_solver_p3`) and the
//! `calc_geom` cases on the unported callees (`SmSurface::DropCurve` of the UV trim curve
//! creation, `intersect.surface_drop_lib_p3`; `sm_FindFilletTsectPnt` after
//! `EvaluateBinormal`, `blend.fillet_solver_p3`).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use solidcore::blend::fillet_custom_solver::{ConstantRadiusAssistedFs, OffsetSurface, SurfaceId};
use solidcore::blend::fillet_geom::p1::{
    FilletBrep, FilletGeom, FilletGeomId, FilletGeomType, FilletSolverId,
};
use solidcore::blend::fillet_solver::p1::{FilletGeomRef, FilletSolver, FilletSolverOps};
use solidcore::curve::curve::Curve;
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::surface_intersector::p1::{IntersectionPointType, TsectPnt};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::offset_surface::SS_UNKNOWN;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::{Brep, EdgeId};
use solidcore::SolidError;
use solidcore_ffi::units::blend_fillet_solver_p4::{self as ffi};

use super::surface_surf_of_revolution_p1 as rev;
use super::topology_edgeuse_p1::import;
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::{p2, v3};
use crate::{Case, CaseSource, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// `blend.fillet_solver_p4` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

const NOW: f64 = -f64::MAX;
/// `SM_UNDEF_DOUBLE`.
const UNDEF: f64 = 1.23456e-61;

// ---- inputs ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
enum SurfaceIn {
    Plane {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: [f64; 2],
        domain: [f64; 4],
    },
    BSpline {
        p: usize,
        q: usize,
        /// Rows (u index) of `[x, y, z, w]`; `w` = `null` for a non-rational point.
        net: Vec<Vec<[Option<f64>; 4]>>,
        u: Vec<f64>,
        v: Vec<f64>,
    },
    Revolution {
        surf: rev::SurfIn,
    },
}

/// An intersection point; the parameter types and the surface matrices are zero unless
/// given (`ptype` codes are `SmExtentPointType`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PointIn {
    /// `SmIntersectionPointType`.
    t: u8,
    #[serde(default)]
    ptype: [u8; 4],
    uv: [[f64; 2]; 2],
    duv: [[f64; 2]; 2],
    pos: [f64; 3],
    deriv: [f64; 3],
    #[serde(default)]
    param: f64,
    #[serde(default)]
    angle: f64,
    #[serde(default)]
    dev: f64,
    #[serde(default)]
    user: [f64; 4],
    /// The 54 values of the two surface matrices, row major.
    #[serde(default)]
    srf: Option<Vec<f64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PointCase {
    a: SurfaceIn,
    /// `None`: B is A itself.
    b: Option<SurfaceIn>,
    dom1: [f64; 4],
    dom2: [f64; 4],
    radii: [f64; 2],
    orient: [bool; 2],
    tol: f64,
    ang_tol: f64,
    tangency_tol: f64,
    fd: bool,
    uv: [[f64; 2]; 2],
    dir: f64,
    point: PointIn,
    prev: Option<PointIn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools, reason = "independent upstream flags")]
struct CalcCase {
    w: f64,
    h: f64,
    tol: f64,
    /// The rectangle's UV trim curves are made before the export and the call (both
    /// sides then find them instead of building them).
    #[serde(default)]
    uv_curves: bool,
    edge: usize,
    eu1_same: bool,
    hermite: bool,
    geom: bool,
    split: bool,
    extend: [bool; 2],
    g1: Vec<f64>,
    radii: [f64; 2],
    orient: [bool; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
enum Inputs {
    PointValues(Box<PointCase>),
    CalcGeom(CalcCase),
}

fn cpt4(c: &[Option<f64>; 4]) -> [f64; 4] {
    [
        c[0].unwrap_or(f64::NAN),
        c[1].unwrap_or(f64::NAN),
        c[2].unwrap_or(f64::NAN),
        c[3].unwrap_or(NOW),
    ]
}

fn point_values(p: &PointIn) -> Vec<f64> {
    let mut v = Vec::with_capacity(ffi::POINT_LEN);
    v.push(f64::from(p.t));
    v.extend(p.ptype.map(f64::from));
    for (uv, duv) in p.uv.iter().zip(&p.duv) {
        v.extend(uv);
        v.extend(duv);
    }
    match &p.srf {
        Some(m) => v.extend(m.iter().copied()),
        None => v.extend([0.0; 54]),
    }
    v.extend(p.pos);
    v.extend(p.deriv);
    v.extend([p.param, p.angle, p.dev]);
    v.extend(p.user);
    v
}

fn ffi_surface(s: &SurfaceIn) -> Result<ffi::SurfaceIn, HarnessError> {
    Ok(match s {
        SurfaceIn::Plane {
            o,
            x,
            y,
            scale,
            domain,
        } => ffi::SurfaceIn::Plane {
            o: *o,
            x: *x,
            y: *y,
            scale: *scale,
            domain: *domain,
        },
        SurfaceIn::BSpline { p, q, net, u, v } => ffi::SurfaceIn::BSpline {
            p: *p,
            q: *q,
            net: net.iter().map(|r| r.iter().map(cpt4).collect()).collect(),
            u: u.clone(),
            v: v.clone(),
        },
        SurfaceIn::Revolution { surf } => ffi::SurfaceIn::Revolution(rev::ffi_surf(surf)?),
    })
}

fn ffi_point_program(i: &PointCase) -> Result<ffi::PointProgram, HarnessError> {
    Ok(ffi::PointProgram {
        a: ffi_surface(&i.a)?,
        b: i.b.as_ref().map(ffi_surface).transpose()?,
        dom1: i.dom1,
        dom2: i.dom2,
        radii: i.radii,
        orient: i.orient,
        tol: i.tol,
        ang_tol: i.ang_tol,
        tangency_tol: i.tangency_tol,
        fd: i.fd,
        uv: i.uv,
        dir: i.dir,
        point: point_values(&i.point),
        prev: i.prev.as_ref().map(point_values),
    })
}

fn ffi_calc_program(c: &CalcCase) -> ffi::CalcProgram {
    ffi::CalcProgram {
        w: c.w,
        h: c.h,
        tol: c.tol,
        uv_curves: c.uv_curves,
        edge: c.edge,
        eu1_same: c.eu1_same,
        hermite: c.hermite,
        geom: c.geom,
        split: c.split,
        extend: c.extend,
        g1: c.g1.clone(),
        radii: c.radii,
        orient: c.orient,
    }
}

// ---- outputs ------------------------------------------------------------------------------

fn point_output(rec: &[f64]) -> Result<Output, HarnessError> {
    let (head, rest) = rec.split_first().ok_or_else(|| harness("empty record"))?;
    if *head == -1.0 {
        let b = rest.first().ok_or_else(|| harness("short build record"))?;
        return Ok(Output::new().int("build", *b as i64));
    }
    if rest.len() != ffi::POINT_LEN + 2 {
        return Err(harness("point record length"));
    }
    let (pt, off) = rest.split_at(ffi::POINT_LEN);
    Ok(Output::new()
        .int("status", *head as i64)
        .values("point", pt.to_vec())
        .values("offsets", off.to_vec()))
}

fn calc_output(rec: &[f64], n_in: usize) -> Result<Output, HarnessError> {
    let mut it = rec.iter().copied();
    let mut next = || it.next().ok_or_else(|| harness("short calc record"));
    let status = next()? as i64;
    let solver_status = next()? as i64;
    let before = next()? as i64;
    let after = next()? as i64;
    let n = next()? as usize;
    if n > 64 {
        return Err(harness("calc record G1 count"));
    }
    let mut g1 = Vec::new();
    for _ in 0..n {
        g1.push(next()?);
    }
    let mut knots_in = Vec::new();
    for _ in 0..n_in {
        knots_in.push(next()?);
    }
    if it.next().is_some() {
        return Err(harness("trailing calc record"));
    }
    Ok(Output::new()
        .int("status", status)
        .int("solver_status", solver_status)
        .ints("extend", vec![before, after])
        .values("g1", g1)
        .values("knots_in", knots_in))
}

// ---- C++ side -----------------------------------------------------------------------------

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    match i {
        Inputs::PointValues(p) => {
            let rec = ffi::point_values(&ffi_point_program(p)?)
                .map_err(|e| harness(format!("C++: {e}")))?;
            point_output(&rec)
        }
        Inputs::CalcGeom(c) => {
            let rec =
                ffi::calc_geom(&ffi_calc_program(c)).map_err(|e| harness(format!("C++: {e}")))?;
            calc_output(&rec, c.g1.len())
        }
    }
}

// ---- Rust side ----------------------------------------------------------------------------

/// A failed Rust step: the whole case is deferred, or the harness failed.
enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Fail::Harness(e)
    }
}

/// `Unported` defers the case; anything else is a harness failure.
fn setup<T>(r: solidcore::Result<T>, what: &str) -> Result<T, Fail> {
    r.map_err(|e| match Output::unported(&e) {
        Some(out) => Fail::Deferred(Box::new(out)),
        None => Fail::Harness(harness(format!("{what}: {e}"))),
    })
}

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> i64 {
    match e {
        SolidError::InvalidInput { .. } => 1007,
        SolidError::Upstream { code, .. } => i64::from(*code),
        _ => -1,
    }
}

/// `0` or the status; `Unported` defers the case.
fn op_status(r: solidcore::Result<()>) -> Result<i64, Fail> {
    match r {
        Ok(()) => Ok(0),
        Err(e) => match Output::unported(&e) {
            Some(d) => Err(Fail::Deferred(Box::new(d))),
            None => Ok(status(&e)),
        },
    }
}

fn ext(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

/// The surface, or the status of a failed surface-of-revolution build.
fn rust_surface(k: &Kernel, s: &SurfaceIn) -> Result<Result<Surface, i64>, HarnessError> {
    match s {
        SurfaceIn::Plane {
            o,
            x,
            y,
            scale,
            domain,
        } => Ok(Ok(Surface::from(Plane::new(
            v3(*o),
            v3(*x),
            v3(*y),
            p2(*scale),
            &ext(*domain),
        )))),
        SurfaceIn::BSpline { p, q, net, u, v } => {
            let net = net
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|c| {
                            let [x, y, z, w] = cpt4(c);
                            CPoint::new(x, y, z, w)
                        })
                        .collect()
                })
                .collect();
            let sur = NurbSurface::new(net, *p, *q, u.clone(), v.clone())
                .map_err(|e| harness(format!("surface: {e}")))?;
            Ok(Ok(Surface::from(BSplineSurface::from_nurb(sur, false))))
        }
        SurfaceIn::Revolution { surf } => Ok(match rev::rust_surf(k, surf)? {
            Ok(r) => Ok(Surface::from(r)),
            Err(e) => Err(status(&e)),
        }),
    }
}

fn ept(code: f64) -> Result<solidcore::geom::extent1d::ExtentPointType, HarnessError> {
    use solidcore::geom::extent1d::ExtentPointType;
    Ok(match code as i64 {
        0 => ExtentPointType::Start,
        1 => ExtentPointType::Inside,
        2 => ExtentPointType::End,
        3 => ExtentPointType::Both,
        4 => ExtentPointType::Outside,
        5 => ExtentPointType::Unknown,
        _ => return Err(harness("extent point type")),
    })
}

fn rust_point(v: &[f64]) -> Result<TsectPnt, HarnessError> {
    if v.len() != ffi::POINT_LEN {
        return Err(harness("point length"));
    }
    let mut it = v.iter().copied();
    let mut g = || it.next().unwrap_or(f64::NAN);
    let mut t = TsectPnt::new();
    t.point_type =
        IntersectionPointType::from_code(g() as i32).ok_or_else(|| harness("point type"))?;
    for pt in t.uv_param_type.iter_mut().flatten() {
        *pt = ept(g())?;
    }
    for uv in t.uv_curve_pv.iter_mut().flatten() {
        *uv = Vec2::new(g(), g());
    }
    for vec in t.surface_pv.iter_mut().flatten().flatten() {
        *vec = Vec3::new(g(), g(), g());
    }
    for vec in &mut t.curve_pv {
        *vec = Vec3::new(g(), g(), g());
    }
    t.curve_parameter = g();
    t.tangent_plane_angle_rad = g();
    t.deviation = g();
    for u in &mut t.user_doubles {
        *u = g();
    }
    Ok(t)
}

fn put_point(o: &mut Vec<f64>, t: &TsectPnt) {
    o.push(f64::from(t.point_type.code()));
    o.extend(
        t.uv_param_type
            .iter()
            .flatten()
            .map(|pt| f64::from(pt.code())),
    );
    for uv in t.uv_curve_pv.iter().flatten() {
        o.extend([uv.x, uv.y]);
    }
    for v in t.surface_pv.iter().flatten().flatten() {
        o.extend([v.x, v.y, v.z]);
    }
    for v in &t.curve_pv {
        o.extend([v.x, v.y, v.z]);
    }
    o.extend([t.curve_parameter, t.tangent_plane_angle_rad, t.deviation]);
    o.extend(t.user_doubles);
}

/// A rail offset as `SmConstantRadiusFS` builds it (default `lSingularities`,
/// `SM_SS_UNKNOWN`, extension allowed).
fn rail_offset(k: &Kernel, d: f64, base: Surface, id: SurfaceId) -> Result<OffsetSurface, Fail> {
    let mut s = setup(
        OffsetSurface::new(k, d, base, id, SS_UNKNOWN),
        "offset surface",
    )?;
    s.surface.set_allow_extension(true);
    Ok(s)
}

/// `SmConstantRadiusFS(ctx, tol, ang, tan, r0, r1, A, B, o0, o1)`.
#[allow(clippy::too_many_arguments)]
fn cr_solver(
    k: &Kernel,
    tol: f64,
    ang: f64,
    tan: f64,
    radii: [f64; 2],
    orient: [bool; 2],
    a: Surface,
    b: Surface,
    b_id: u32,
) -> Result<ConstantRadiusAssistedFs, Fail> {
    let o = orient.map(|f| if f { 1.0 } else { -1.0 });
    let s0 = rail_offset(k, radii[0] * o[0], a, SurfaceId::Free(0))?;
    let s1 = rail_offset(k, radii[1] * o[1], b, SurfaceId::Free(b_id))?;
    Ok(ConstantRadiusAssistedFs::from_parts(
        tol,
        ang,
        tan,
        radii,
        [Some(s0), Some(s1)],
        o,
        [None, None],
    ))
}

/// The C++ shim's derived solvers: `SmConstantRadiusFS` with `OffsetRadiiCanChange()`
/// returning `fd` (the finite-difference solver `FdFS`) and, for `calc_geom`, with
/// `SetExtensionFlags` setting the given flags (`CalcFS`). Everything else is
/// `SmConstantRadiusFS`'s.
struct ShimSolver {
    inner: ConstantRadiusAssistedFs,
    fd: bool,
    extend: Option<[bool; 2]>,
}

impl FilletSolverOps for ShimSolver {
    fn solver(&self) -> &FilletSolver {
        self.inner.solver()
    }

    fn solver_mut(&mut self) -> &mut FilletSolver {
        self.inner.solver_mut()
    }

    fn jacobian_size(&self) -> solidcore::Result<usize> {
        <ConstantRadiusAssistedFs as FilletSolverOps>::jacobian_size(&self.inner)
    }

    fn setup_offset_values(
        &mut self,
        tsect: &TsectPnt,
        offset_dist: Option<&mut f64>,
    ) -> solidcore::Result<()> {
        <ConstantRadiusAssistedFs as FilletSolverOps>::setup_offset_values(
            &mut self.inner,
            tsect,
            offset_dist,
        )
    }

    fn offset_radii_can_change(&self) -> solidcore::Result<bool> {
        if self.fd {
            Ok(true)
        } else {
            <ConstantRadiusAssistedFs as FilletSolverOps>::offset_radii_can_change(&self.inner)
        }
    }

    fn load_jacobian(
        &mut self,
        num_equations: &mut usize,
        num_parameters: &mut usize,
        load: &mut solidcore::blend::fillet_custom_solver::JacobianLoad<'_>,
        f: &mut [f64],
        jacobian: Option<&mut solidcore::geom::matrix::Matrix>,
    ) -> solidcore::Result<bool> {
        <ConstantRadiusAssistedFs as FilletSolverOps>::load_jacobian(
            &mut self.inner,
            num_equations,
            num_parameters,
            load,
            f,
            jacobian,
        )
    }

    fn set_extension_flags(&mut self, b: &Brep) -> solidcore::Result<()> {
        match self.extend {
            Some([before, after]) => {
                let s = self.inner.solver_mut();
                s.extend_before = before;
                s.extend_after = after;
                Ok(())
            }
            None => self.inner.set_extension_flags(b),
        }
    }
}

fn rust_point_case(i: &PointCase) -> Result<Output, Fail> {
    let k = Kernel::default();
    let may_step =
        i.fd || i.point.t == IntersectionPointType::TangentPoint.code() as u8 || i.prev.is_some();
    if may_step {
        let step = ffi::static_step().map_err(|e| harness(format!("static step probe: {e}")))?;
        k.fillet_point_step.set(Some(step));
    }
    let a = match rust_surface(&k, &i.a)? {
        Ok(s) => s,
        Err(b) => return Ok(Output::new().int("build", b)),
    };
    let b = match &i.b {
        None => None,
        Some(s) => match rust_surface(&k, s)? {
            Ok(s) => Some(s),
            Err(b) => return Ok(Output::new().int("build", b)),
        },
    };
    let b_id = u32::from(b.is_some());
    let bs = b.unwrap_or_else(|| a.clone());
    let inner = cr_solver(
        &k,
        i.tol,
        i.ang_tol,
        i.tangency_tol,
        i.radii,
        i.orient,
        a,
        bs,
        b_id,
    )?;
    let mut ops = ShimSolver {
        inner,
        fd: i.fd,
        extend: None,
    };
    let mut point = rust_point(&point_values(&i.point))?;
    let prev = match &i.prev {
        Some(p) => Some(rust_point(&point_values(p))?),
        None => None,
    };
    let r = FilletSolver::compute_point_values(
        &mut ops,
        &k,
        i.uv.map(p2),
        &ext(i.dom1),
        &ext(i.dom2),
        i.dir,
        &mut point,
        prev.as_ref(),
    );
    let st = op_status(r)?;
    let mut pt = Vec::with_capacity(ffi::POINT_LEN);
    put_point(&mut pt, &point);
    let offsets: Vec<f64> = ops
        .solver()
        .surfaces
        .iter()
        .map(|s| {
            s.as_ref()
                .map_or(ffi::UNSET, |s| s.surface.offset_distance())
        })
        .collect();
    Ok(Output::new()
        .int("status", st)
        .values("point", pt)
        .values("offsets", offsets))
}

fn rust_calc_case(c: &CalcCase) -> Result<Output, Fail> {
    let k = Kernel::default();
    let ex = ffi::calc_target(c.w, c.h, c.tol, c.uv_curves)
        .map_err(|e| harness(format!("C++ target: {e}")))?;
    let mut target = import(&ex)?;
    let edge_id = EdgeId::from_index(u32::try_from(c.edge).map_err(|_| harness("edge"))?);
    let edge = target
        .edge(edge_id)
        .map_err(|e| harness(format!("edge: {e}")))?
        .clone();
    let eu0 = *edge
        .edgeuses
        .first()
        .ok_or_else(|| harness("edge without edge uses"))?;
    let eu1 = if c.eu1_same {
        eu0
    } else {
        edge.edgeuses.get(1).copied().unwrap_or(eu0)
    };
    let face = target
        .faces()
        .ids()
        .next()
        .ok_or_else(|| harness("rectangle without a face"))?;
    let srf = target
        .face(face)
        .map_err(|e| harness(format!("face: {e}")))?
        .surface
        .clone()
        .ok_or_else(|| harness("face without a surface"))?;
    let knots_in: Vec<f64> = c.g1.iter().map(|&f| edge.interval.evaluate(f)).collect();
    if c.hermite {
        let pa = target
            .vertex(edge.vertices[0])
            .map_err(|e| harness(format!("vertex: {e}")))?
            .point;
        let pb = target
            .vertex(edge.vertices[1])
            .map_err(|e| harness(format!("vertex: {e}")))?
            .point;
        let t = pb - pa;
        target
            .edge_mut(edge_id)
            .map_err(|e| harness(format!("edge: {e}")))?
            .curve = Some(Curve::from(HermiteCurve::new(pa, t, pb, t, 3)));
    }
    let mut inner = cr_solver(&k, 1e-5, 0.1, 0.03, c.radii, c.orient, srf.clone(), srf, 0)?;
    inner.solver.edgeuses = [Some(eu0), Some(eu1)];
    inner.solver.g1_knots.clone_from(&knots_in);
    let mut fb = FilletBrep::new();
    let mut geoms: Vec<FilletGeomId> = Vec::new();
    if c.geom {
        let mut g = setup(
            FilletGeom::new(
                &k,
                Some((FilletSolverId(0), &inner)),
                FilletGeomType::Default,
            ),
            "fillet geometry",
        )?;
        g.need_split = c.split;
        geoms.push(fb.add_geom(g));
        inner.solver.fillet_geoms.push(FilletGeomRef { index: 0 });
    }
    let mut ops = ShimSolver {
        inner,
        fd: false,
        extend: Some(c.extend),
    };
    let r = FilletSolver::calc_fillet_geom(&mut ops, &k, &mut target, &mut fb, &mut geoms);
    let st = op_status(r)?;
    let s = ops.solver();
    Ok(Output::new()
        .int("status", st)
        .int("solver_status", s.status as i64)
        .ints(
            "extend",
            vec![i64::from(s.extend_before), i64::from(s.extend_after)],
        )
        .values("g1", s.g1_knots.clone())
        .values("knots_in", knots_in))
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let r = match i {
        Inputs::PointValues(p) => rust_point_case(p),
        Inputs::CalcGeom(c) => rust_calc_case(c),
    };
    match r {
        Ok(o) => Ok(o),
        Err(Fail::Deferred(d)) => Ok(*d),
        Err(Fail::Harness(e)) => Err(e),
    }
}

// ---- generated cases ----------------------------------------------------------------------

fn gen_scale(rng: &mut Rng) -> f64 {
    match rng.below(12) {
        0 => 1e-6,
        1 => 1e-3,
        2 => 1e3,
        3 => 1e6,
        _ => rng.uniform(0.5, 4.0),
    }
}

fn norm(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / l, v[1] / l, v[2] / l]
}

fn unit_vec(rng: &mut Rng) -> [f64; 3] {
    loop {
        let v = rng.point3(-1.0, 1.0);
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if l > 0.2 && l <= 1.0 {
            return norm(v);
        }
    }
}

fn cross(a: [f64; 3], c: [f64; 3]) -> [f64; 3] {
    [
        a[1] * c[2] - a[2] * c[1],
        a[2] * c[0] - a[0] * c[2],
        a[0] * c[1] - a[1] * c[0],
    ]
}

fn add(a: [f64; 3], b: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] + s * b[0], a[1] + s * b[1], a[2] + s * b[2]]
}

fn scale3(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// An orthonormal frame `(e1, e2, e3)`.
fn frame(rng: &mut Rng) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let e1 = unit_vec(rng);
    let mut t = unit_vec(rng);
    while cross(e1, t).iter().map(|c| c * c).sum::<f64>() < 0.1 {
        t = unit_vec(rng);
    }
    let e3 = norm(cross(e1, t));
    let e2 = cross(e3, e1);
    (e1, e2, e3)
}

/// `e` turned by `ang` about `axis` (unit, perpendicular to `e`).
fn turn(e: [f64; 3], axis: [f64; 3], ang: f64) -> [f64; 3] {
    let w = cross(axis, e);
    add(scale3(e, ang.cos()), w, ang.sin())
}

fn gen_plane(rng: &mut Rng, s: f64) -> SurfaceIn {
    let (x, y, _) = frame(rng);
    let scale = if rng.chance(0.7) {
        [1.0, 1.0]
    } else {
        [rng.uniform(0.5, 2.0), rng.uniform(0.5, 2.0)]
    };
    let domain = if rng.chance(0.5) {
        [0.0, 0.0, s, s]
    } else {
        [-s * rng.uniform(0.2, 1.0), -s, s, s * rng.uniform(0.2, 2.0)]
    };
    SurfaceIn::Plane {
        o: rng.point3(-s, s),
        x,
        y,
        scale,
        domain,
    }
}

fn clamped_knots(rng: &mut Rng, n: usize, p: usize, lo: f64, hi: f64) -> Vec<f64> {
    let interior = n - p - 1;
    let mut inner: Vec<f64> = (0..interior)
        .map(|i| {
            lo + (hi - lo) * (i as f64 + 1.0 + rng.uniform(-0.3, 0.3)) / (interior as f64 + 1.0)
        })
        .collect();
    inner.sort_by(f64::total_cmp);
    let mut k = vec![lo; p + 1];
    k.extend(inner);
    k.extend(vec![hi; p + 1]);
    k
}

/// `mode` 0: a bumpy sheet; 1: pinched (the u = min row collapsed to a pole); 2: flat
/// in one direction (every row the same line: zero normals everywhere).
fn gen_bspline(rng: &mut Rng, s: f64, mode: u64) -> SurfaceIn {
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let nu = p + 1 + rng.below(3) as usize;
    let nv = q + 1 + rng.below(3) as usize;
    let rational = rng.chance(0.2);
    let o = rng.point3(-s, s);
    let bend = rng.uniform(0.0, 0.4);
    let mut net = Vec::new();
    for i in 0..nu {
        let mut row = Vec::new();
        for j in 0..nv {
            let fi = i as f64 / (nu - 1) as f64;
            let fj = j as f64 / (nv - 1) as f64;
            let (x, y, z) = match mode {
                1 => (
                    s * fi,
                    s * fi * (fj - 0.5),
                    s * rng.uniform(-bend, bend) * fi,
                ),
                2 => (s * fj, 0.0, 0.0),
                _ => (s * fi, s * fj, s * rng.uniform(-bend, bend)),
            };
            let w = if rational {
                Some(rng.uniform(0.5, 2.0))
            } else {
                None
            };
            let ww = w.unwrap_or(1.0);
            row.push([
                Some((o[0] + x) * ww),
                Some((o[1] + y) * ww),
                Some((o[2] + z) * ww),
                w,
            ]);
        }
        net.push(row);
    }
    let (ulo, uhi) = if rng.chance(0.7) {
        (0.0, 1.0)
    } else {
        let a = rng.uniform(-5.0, 5.0);
        (a, a + rng.uniform(0.5, 10.0))
    };
    let (vlo, vhi) = if rng.chance(0.7) {
        (0.0, 1.0)
    } else {
        let a = rng.uniform(-5.0, 5.0);
        (a, a + rng.uniform(0.5, 10.0))
    };
    let u = clamped_knots(rng, nu, p, ulo, uhi);
    let v = clamped_knots(rng, nv, q, vlo, vhi);
    SurfaceIn::BSpline { p, q, net, u, v }
}

/// A surface of revolution: `shape` 0 cylinder, 1 cone, 2 cone with its apex on the axis,
/// 3 sphere, 4 torus.
fn gen_revolution(rng: &mut Rng, s: f64, shape: u64) -> SurfaceIn {
    let c = rng.point3(-s, s);
    let (e1, _, e3) = frame(rng);
    let r = s * rng.uniform(0.3, 2.0);
    let h = s * rng.uniform(0.5, 3.0);
    let gen = match shape {
        0..=2 => {
            let r0 = match shape {
                0 => r,
                1 => r * rng.uniform(0.2, 0.8),
                _ => 0.0,
            };
            let a = add(add(c, e1, r0), e3, -0.5 * h);
            let b = add(add(c, e1, r), e3, 0.5 * h);
            if rng.chance(0.5) {
                rev::GenIn::Line { a, b }
            } else {
                rev::GenIn::Line { a: b, b: a }
            }
        }
        3 => rev::GenIn::Circle {
            c,
            x: e1,
            y: e3,
            d: [-90.0, 90.0],
            r,
        },
        _ => rev::GenIn::Circle {
            c: add(c, e1, r),
            x: e1,
            y: e3,
            d: if rng.chance(0.5) {
                [0.0, 360.0]
            } else {
                [-180.0, 180.0]
            },
            r: r * rng.uniform(0.1, 0.7),
        },
    };
    SurfaceIn::Revolution {
        surf: rev::SurfIn {
            gen,
            build: rev::BuildIn::Canonical { point: c, axis: e3 },
            oob: false,
            swap: rng.chance(0.3),
        },
    }
}

fn gen_surface(rng: &mut Rng, s: f64) -> SurfaceIn {
    match rng.below(20) {
        0..=4 => gen_plane(rng, s),
        5..=10 => gen_bspline(rng, s, 0),
        11 => gen_bspline(rng, s, 1),
        12 => gen_bspline(rng, s, 2),
        n => gen_revolution(rng, s, (n - 13) % 5),
    }
}

/// The natural UV domain the Rust surface reports (`umin vmin umax vmax`).
fn natural(k: &Kernel, s: &SurfaceIn) -> Option<[f64; 4]> {
    let srf = rust_surface(k, s).ok()?.ok()?;
    let d = srf.natural_uv_domain();
    Some([d.min().x, d.min().y, d.max().x, d.max().y])
}

/// A sub-domain, the natural one, or one extended past it.
fn gen_domain(rng: &mut Rng, nat: [f64; 4]) -> [f64; 4] {
    let [a, b0, c, d] = nat;
    let (w, h) = (c - a, d - b0);
    match rng.below(8) {
        0 => [a + 0.1 * w, b0 + 0.2 * h, c - 0.3 * w, d - 0.1 * h],
        1 => [a - 0.2 * w, b0 - 0.1 * h, c + 0.1 * w, d + 0.3 * h],
        _ => nat,
    }
}

/// A UV in (or on, or just past) the domain `nat`: corners, edges, poles, interior.
fn gen_uv(rng: &mut Rng, nat: [f64; 4]) -> [f64; 2] {
    let [a, b0, c, d] = nat;
    let pick = |rng: &mut Rng, lo: f64, hi: f64| match rng.below(10) {
        0 => lo,
        1 => hi,
        2 => lo - 1e-3 * (hi - lo),
        3 => hi - 1e-12 * (hi - lo),
        _ => lo + rng.unit() * (hi - lo),
    };
    [pick(rng, a, c), pick(rng, b0, d)]
}

fn gen_point(rng: &mut Rng, uv: [[f64; 2]; 2], s: f64) -> PointIn {
    let t = match rng.below(10) {
        0..=3 => 2,
        4..=5 => 3,
        6 => 1,
        _ => rng.below(8) as u8,
    };
    PointIn {
        t,
        ptype: [0, 1, 2, rng.below(6) as u8],
        uv,
        duv: [[rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)]; 2],
        pos: rng.point3(-s, s),
        deriv: unit_vec(rng),
        param: rng.uniform(-s, s),
        angle: rng.uniform(0.0, 1.5),
        dev: rng.uniform(0.0, 1e-3) * s,
        user: [rng.unit(), rng.unit(), 0.0, -rng.unit()],
        srf: None,
    }
}

fn gen_prev(rng: &mut Rng, uv: [[f64; 2]; 2], s: f64) -> Option<PointIn> {
    if rng.chance(0.55) {
        return None;
    }
    let mut p = gen_point(rng, uv, s);
    p.deriv = match rng.below(6) {
        0 => [0.0; 3],
        1 => [UNDEF; 3],
        2 => scale3(unit_vec(rng), rng.uniform(1e-3, 1e3)),
        _ => unit_vec(rng),
    };
    Some(p)
}

/// The trace direction: mostly `+-1`, sometimes `0`, other scales, or huge / tiny
/// magnitudes (the FFI bounds `dir` by finiteness only).
fn gen_dir(rng: &mut Rng) -> f64 {
    match rng.below(12) {
        0 => 0.0,
        1 => 0.5,
        2 => -2.0,
        3 => 1.0e300,
        5 => -1.0e-300,
        n if n % 2 == 0 => 1.0,
        _ => -1.0,
    }
}

/// Two planes through one point at angle `ang` (about A's x axis), straddling the
/// tangency threshold `|n0 x n1|^2 < 10000 SM_EFF_ZERO_SQ` for small angles.
fn gen_near_parallel(rng: &mut Rng, s: f64) -> (SurfaceIn, SurfaceIn) {
    let (x, y, z) = frame(rng);
    let o = rng.point3(-s, s);
    let angs = [
        0.0, 1e-11, 5e-11, 9.9e-11, 1e-10, 1.01e-10, 2e-10, 1e-9, 1e-8, 1e-6,
    ];
    let mut ang = angs
        .get(rng.below(angs.len() as u64) as usize)
        .copied()
        .unwrap_or(0.0);
    if rng.chance(0.3) {
        ang = core::f64::consts::PI - ang;
    }
    let y2 = turn(y, x, ang);
    let _ = z;
    let d = [-s, -s, s, s];
    (
        SurfaceIn::Plane {
            o,
            x,
            y,
            scale: [1.0, 1.0],
            domain: d,
        },
        SurfaceIn::Plane {
            o,
            x,
            y: y2,
            scale: [1.0, 1.0],
            domain: d,
        },
    )
}

fn gen_point_case(rng: &mut Rng) -> Option<PointCase> {
    let k = Kernel::default();
    let s = gen_scale(rng);
    let near = rng.chance(0.15);
    let (a, b) = if near {
        let (a, b) = gen_near_parallel(rng, s);
        (a, Some(b))
    } else {
        let a = gen_surface(rng, s);
        let b = if rng.chance(0.08) {
            None
        } else {
            Some(gen_surface(rng, s))
        };
        (a, b)
    };
    let na = natural(&k, &a)?;
    let nb = natural(&k, b.as_ref().unwrap_or(&a))?;
    let uv0 = if near {
        [rng.uniform(-0.5, 0.5) * s, rng.uniform(-0.5, 0.5) * s]
    } else {
        gen_uv(rng, na)
    };
    let uv1 = if near || (b.is_none() && rng.chance(0.5)) {
        uv0
    } else {
        gen_uv(rng, nb)
    };
    let uv = [uv0, uv1];
    let r = s * rng.uniform(0.01, 0.5);
    let radii = match rng.below(8) {
        0 => [0.0, 0.0],
        1 => [r, 0.5 * r],
        _ => [r, r],
    };
    let point = gen_point(rng, uv, s);
    let prev = gen_prev(rng, uv, s);
    Some(PointCase {
        dom1: gen_domain(rng, na),
        dom2: gen_domain(rng, nb),
        a,
        b,
        radii,
        orient: [rng.chance(0.5), rng.chance(0.5)],
        tol: s * 1e-5,
        ang_tol: 0.1,
        tangency_tol: 0.03,
        fd: rng.chance(0.25),
        uv,
        dir: gen_dir(rng),
        point,
        prev,
    })
}

fn gen_calc_case(rng: &mut Rng) -> CalcCase {
    let s = match rng.below(6) {
        0 => 1e-3,
        1 => 1e3,
        _ => rng.uniform(0.5, 20.0),
    };
    let w = s * rng.uniform(0.5, 2.0);
    let h = s * rng.uniform(0.5, 2.0);
    let hermite = rng.chance(0.5);
    let g1 = (0..rng.below(4))
        .map(|_| match rng.below(4) {
            0 => 0.5,
            _ => rng.uniform(0.05, 0.95),
        })
        .collect();
    let r = w.min(h) * rng.uniform(0.01, 0.4);
    CalcCase {
        w,
        h,
        tol: (1e-5_f64).min(1e-3 * w.min(h)),
        edge: rng.below(4) as usize,
        eu1_same: rng.chance(0.3),
        hermite,
        geom: !hermite || rng.chance(0.5),
        split: rng.chance(0.4),
        extend: [rng.chance(0.5), rng.chance(0.5)],
        g1,
        radii: if rng.chance(0.8) {
            [r, r]
        } else {
            [r, 0.5 * r]
        },
        orient: [rng.chance(0.5), rng.chance(0.5)],
        uv_curves: rng.chance(0.5),
    }
}

fn gen_case(rng: &mut Rng) -> Option<Inputs> {
    if rng.chance(0.12) {
        return Some(Inputs::CalcGeom(gen_calc_case(rng)));
    }
    gen_point_case(rng).map(|p| Inputs::PointValues(Box::new(p)))
}

fn encodable(i: &Inputs) -> bool {
    match i {
        Inputs::PointValues(p) => {
            ffi_point_program(p).is_ok_and(|prog| ffi::encode_point(&prog).is_ok())
        }
        Inputs::CalcGeom(c) => ffi::encode_calc(&ffi_calc_program(c)).is_ok(),
    }
}

// ---- edge cases ---------------------------------------------------------------------------

fn plane_json(o: [f64; 3], x: [f64; 3], y: [f64; 3], d: [f64; 4]) -> Value {
    json!({"kind": "plane", "o": o, "x": x, "y": y, "scale": [1.0, 1.0], "domain": d})
}

fn pt_json(t: u8, uv0: [f64; 2], uv1: [f64; 2], deriv: [f64; 3]) -> Value {
    json!({"t": t, "uv": [uv0, uv1], "duv": [[1.0, 0.0], [1.0, 0.0]], "pos": [0.0, 0.0, 0.0],
           "deriv": deriv})
}

fn sphere_json(r: f64) -> Value {
    json!({"kind": "revolution", "surf": {
        "gen": {"kind": "circle", "c": [0.0, 0.0, 0.0], "x": [1.0, 0.0, 0.0],
                "y": [0.0, 0.0, 1.0], "d": [-90.0, 90.0], "r": r},
        "build": {"mode": "canonical", "point": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0]}}})
}

fn cone_apex_json() -> Value {
    json!({"kind": "revolution", "surf": {
        "gen": {"kind": "line", "a": [0.0, 0.0, 0.0], "b": [1.0, 0.0, 1.0]},
        "build": {"mode": "canonical", "point": [0.0, 0.0, 0.0], "axis": [0.0, 0.0, 1.0]}}})
}

#[allow(clippy::too_many_lines)]
fn edge_cases(k: &Kernel) -> Vec<(String, Value)> {
    let d = [0.0, 0.0, 1.0, 1.0];
    let xy = plane_json([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], d);
    let xz = plane_json([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], d);
    let xy2 = plane_json([0.0, 0.0, 0.5], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], d);
    let anti = plane_json([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0], d);
    let degen = plane_json([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0], d);
    let sphere = sphere_json(1.0);
    let cone = cone_apex_json();
    let nat = |s: &Value| -> [f64; 4] {
        serde_json::from_value::<SurfaceIn>(s.clone())
            .ok()
            .and_then(|s| natural(k, &s))
            .unwrap_or(d)
    };
    let ns = nat(&sphere);
    let nc = nat(&cone);
    let pc = |a: &Value,
              b: Option<&Value>,
              uv: [[f64; 2]; 2],
              t: u8,
              prev: Option<Value>,
              fd: bool,
              dir: f64| {
        let da = nat(a);
        let db = b.map_or(da, nat);
        json!({"op": "point_values", "a": a, "b": b, "dom1": da, "dom2": db,
               "radii": [0.1, 0.1], "orient": [true, true], "tol": 1e-5, "ang_tol": 0.1,
               "tangency_tol": 0.03, "fd": fd, "uv": uv, "dir": dir,
               "point": pt_json(t, uv[0], uv[1], [1.0, 0.0, 0.0]), "prev": prev})
    };
    let mid = [[0.3, 0.4], [0.3, 0.4]];
    let prev_zero = Some(pt_json(2, [0.3, 0.4], [0.3, 0.4], [0.0, 0.0, 0.0]));
    let prev_undef = Some(pt_json(2, [0.3, 0.4], [0.3, 0.4], [UNDEF, UNDEF, UNDEF]));
    let prev_unit = Some(pt_json(2, [0.3, 0.4], [0.3, 0.4], [0.0, 1.0, 0.0]));
    let pole = [[ns[0] + 0.3 * (ns[2] - ns[0]), ns[3]], [0.2, 0.2]];
    let south = [[ns[0] + 0.7 * (ns[2] - ns[0]), ns[1]], [0.2, 0.2]];
    let apex = [[nc[0] + 0.25 * (nc[2] - nc[0]), nc[1]], [0.2, 0.2]];
    let apex_v = [[nc[0], nc[1] + 0.25 * (nc[3] - nc[1])], [0.2, 0.2]];
    let mut v = vec![
        (
            "edge_crossing_xy_xz".to_string(),
            pc(&xy, Some(&xz), mid, 2, None, false, 1.0),
        ),
        (
            "edge_crossing_dir_minus".into(),
            pc(&xy, Some(&xz), mid, 1, None, false, -1.0),
        ),
        (
            "edge_crossing_dir_zero".into(),
            pc(&xy, Some(&xz), mid, 2, None, false, 0.0),
        ),
        (
            "edge_parallel_no_prev".into(),
            pc(&xy, Some(&xy2), mid, 2, None, false, 1.0),
        ),
        (
            "edge_parallel_prev_unit".into(),
            pc(&xy, Some(&xy2), mid, 2, prev_unit.clone(), false, 1.0),
        ),
        (
            "edge_parallel_prev_zero".into(),
            pc(&xy, Some(&xy2), mid, 2, prev_zero.clone(), false, 1.0),
        ),
        (
            "edge_parallel_prev_undef".into(),
            pc(&xy, Some(&xy2), mid, 2, prev_undef, false, 1.0),
        ),
        (
            "edge_antiparallel".into(),
            pc(&xy, Some(&anti), mid, 2, None, false, 1.0),
        ),
        (
            "edge_same_surface_same_uv".into(),
            pc(&xy, None, mid, 2, None, false, 1.0),
        ),
        (
            "edge_same_surface_prev".into(),
            pc(&xy, None, mid, 3, prev_unit.clone(), false, 1.0),
        ),
        (
            "edge_degenerate_plane".into(),
            pc(&degen, Some(&xz), mid, 2, None, false, 1.0),
        ),
        (
            "edge_incoming_tangent_cr".into(),
            pc(&xy, Some(&xz), mid, 3, None, false, 1.0),
        ),
        (
            "edge_fd_crossing".into(),
            pc(&xy, Some(&xz), mid, 2, None, true, 1.0),
        ),
        (
            "edge_fd_tangent_prev".into(),
            pc(&xy, Some(&xy2), mid, 2, prev_zero, true, 1.0),
        ),
        (
            "edge_sphere_pole".into(),
            pc(&sphere, Some(&xz), pole, 2, None, false, 1.0),
        ),
        (
            "edge_sphere_south_pole".into(),
            pc(&sphere, Some(&xz), south, 2, None, false, 1.0),
        ),
        (
            "edge_cone_apex_u".into(),
            pc(&cone, Some(&xz), apex, 2, None, false, 1.0),
        ),
        (
            "edge_cone_apex_v".into(),
            pc(&cone, Some(&xz), apex_v, 2, None, false, 1.0),
        ),
        (
            "edge_crossing_dir_huge".into(),
            pc(&xy, Some(&xz), mid, 2, None, false, 1.0e300),
        ),
        (
            "edge_crossing_dir_minus_huge".into(),
            pc(&xy, Some(&xz), mid, 2, None, false, -1.0e300),
        ),
        (
            "edge_crossing_dir_max".into(),
            pc(&xy, Some(&xz), mid, 2, None, false, f64::MAX),
        ),
        (
            "edge_crossing_dir_sq_overflow".into(),
            pc(&xy, Some(&xz), mid, 2, None, false, 1.0e155),
        ),
        (
            "edge_crossing_dir_sq_finite".into(),
            pc(&xy, Some(&xz), mid, 2, None, false, 1.0e153),
        ),
        (
            "edge_crossing_dir_tiny".into(),
            pc(&xy, Some(&xz), mid, 2, None, false, 1.0e-300),
        ),
        (
            "edge_crossing_dir_subnormal".into(),
            pc(&xy, Some(&xz), mid, 2, None, false, -5.0e-324),
        ),
        (
            "edge_crossing_dir_tiny_prev".into(),
            pc(&xy, Some(&xz), mid, 2, prev_unit.clone(), false, 1.0e-300),
        ),
        (
            "edge_parallel_prev_dir_huge".into(),
            pc(&xy, Some(&xy2), mid, 2, prev_unit.clone(), false, 1.0e300),
        ),
        (
            "edge_fd_crossing_dir_huge".into(),
            pc(&xy, Some(&xz), mid, 2, None, true, 1.0e300),
        ),
        (
            "edge_fd_tangent_dir_max".into(),
            pc(&xy, Some(&xy2), mid, 2, prev_unit.clone(), true, -f64::MAX),
        ),
        (
            "edge_sphere_pole_dir_huge".into(),
            pc(&sphere, Some(&xz), pole, 2, None, false, 1.0e300),
        ),
        (
            "edge_sphere_fd".into(),
            pc(
                &sphere,
                Some(&xz),
                [
                    [ns[0] + 0.3 * (ns[2] - ns[0]), ns[1] + 0.5 * (ns[3] - ns[1])],
                    [0.2, 0.2],
                ],
                2,
                None,
                true,
                1.0,
            ),
        ),
    ];
    // calc_geom: every edge, Hermite and B-spline curves
    for e in 0..4 {
        v.push((
            format!("edge_calc_hermite_e{e}"),
            json!({"op": "calc_geom", "w": 5.0, "h": 10.0, "tol": 1e-5, "edge": e,
                   "eu1_same": false, "hermite": true, "geom": false, "split": false,
                   "extend": [e % 2 == 0, e > 1], "g1": [], "radii": [0.5, 0.5],
                   "orient": [true, true]}),
        ));
        v.push((
            format!("edge_calc_rails_e{e}"),
            json!({"op": "calc_geom", "w": 5.0, "h": 10.0, "tol": 1e-5, "edge": e,
                   "eu1_same": e == 3, "hermite": false, "geom": true, "split": false,
                   "extend": [false, true], "g1": [], "radii": [0.5, 0.5],
                   "orient": [true, false]}),
        ));
    }
    // the same with the UV trim curves made first: the trim-curve fetch finds them
    for e in 0..4 {
        v.push((
            format!("edge_calc_rails_uv_e{e}"),
            json!({"op": "calc_geom", "w": 5.0, "h": 10.0, "tol": 1e-5, "uv_curves": true,
                   "edge": e, "eu1_same": e == 3, "hermite": false, "geom": true,
                   "split": false, "extend": [false, true], "g1": [], "radii": [0.5, 0.5],
                   "orient": [true, false]}),
        ));
    }
    v.push((
        "edge_calc_split_uv".into(),
        json!({"op": "calc_geom", "w": 5.0, "h": 10.0, "tol": 1e-5, "uv_curves": true,
               "edge": 1, "eu1_same": false, "hermite": false, "geom": true, "split": true,
               "extend": [true, true], "g1": [], "radii": [0.5, 0.5], "orient": [true, true]}),
    ));
    v.push((
        "edge_calc_g1_uv".into(),
        json!({"op": "calc_geom", "w": 2.0e-3, "h": 1.0e-3, "tol": 1e-8, "uv_curves": true,
               "edge": 2, "eu1_same": false, "hermite": false, "geom": true, "split": false,
               "extend": [false, false], "g1": [0.25, 0.5], "radii": [1.0e-4, 1.0e-4],
               "orient": [false, true]}),
    ));
    v.push((
        "edge_calc_split_no_g1".into(),
        json!({"op": "calc_geom", "w": 5.0, "h": 10.0, "tol": 1e-5, "edge": 1,
               "eu1_same": false, "hermite": false, "geom": true, "split": true,
               "extend": [true, true], "g1": [], "radii": [0.5, 0.5], "orient": [true, true]}),
    ));
    v.push((
        "edge_calc_split_g1".into(),
        json!({"op": "calc_geom", "w": 5.0, "h": 10.0, "tol": 1e-5, "edge": 2,
               "eu1_same": false, "hermite": false, "geom": true, "split": true,
               "extend": [false, false], "g1": [0.25, 0.5, 0.75], "radii": [0.5, 0.5],
               "orient": [true, true]}),
    ));
    v
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_solver_p4"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let Some(i) = gen_case(rng) else { continue };
            if encodable(&i) {
                return serde_json::to_value(i).ok();
            }
        }
        None
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut v = default_cases(self, &ctx)?;
        let k = Kernel::default();
        for (name, inputs) in edge_cases(&k) {
            v.push(Case {
                unit: self.id().into(),
                name,
                source: CaseSource::Inline,
                inputs,
            });
        }
        Ok(v)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_cases_encode() {
        let mut rng = Rng::new(7);
        let mut n = 0;
        for _ in 0..200 {
            if Runner.generate(&mut rng).is_some() {
                n += 1;
            }
        }
        assert!(n > 150, "{n}");
    }

    /// Most generated point cases reach a successful C++ evaluation (they compare real
    /// values once the Rust side lands), and the probe names one of the two steps.
    #[test]
    fn cpp_outcomes() {
        let step = ffi::static_step().expect("probe");
        assert!(step == 1e-3 || step == 1e-5, "{step}");
        let mut rng = Rng::new(11);
        let (mut ok, mut total) = (0, 0);
        let mut statuses = std::collections::BTreeMap::new();
        for _ in 0..300 {
            let Some(v) = Runner.generate(&mut rng) else {
                continue;
            };
            let i: Inputs = serde_json::from_value(v).expect("inputs");
            let out = cpp(&i).expect("C++ ran");
            let op = match &i {
                Inputs::PointValues(p) if p.fd || p.point.t == 3 || p.prev.is_some() => {
                    "point-may-step"
                }
                Inputs::PointValues(p) => match &p.a {
                    SurfaceIn::Plane { .. } => "point-nofd-plane",
                    SurfaceIn::BSpline { .. } => "point-nofd-bsp",
                    SurfaceIn::Revolution { .. } => "point-nofd-rev",
                },
                Inputs::CalcGeom(_) => "calc",
            };
            let tag = match (out.ints.get("status"), out.ints.get("build")) {
                (Some(st), _) => format!("{op} status {st:?}"),
                (None, Some(b)) => format!("{op} build {b:?}"),
                _ => format!("{op} ?"),
            };
            if op.starts_with("point") {
                total += 1;
                if out.ints.get("status") == Some(&vec![0]) {
                    ok += 1;
                }
            }
            *statuses.entry(tag).or_insert(0) += 1;
        }
        eprintln!("{statuses:#?}");
        assert!(ok * 2 > total, "{ok}/{total}");
    }
}
