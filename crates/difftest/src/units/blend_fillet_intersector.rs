//! Runner for unit `blend.fillet_intersector`: `SmFilletIntersector`.
//!
//! C++: `solidcore_ffi::units::blend_fillet_intersector` (a real `SmFilletIntersector` over a
//! real `SmConstantRadiusAssistedFS` (offsets of `SmPlane` / `SmBSplineSurface` objects, built
//! on a box edge use whose offsets are replaced), plus the file-static helpers). Rust:
//! `solidcore::blend::fillet_intersector` over the co-ported `ConstantRadiusAssistedFs`, so the
//! intersector's `PointOnPlaneSolve` virtual is the assisted solver's override on both sides.
//!
//! A case is two surfaces (B may be A itself), their domains, the solver's radii, orientations
//! and tolerances, the intersector's tolerances and flags, a point arena and one op; both sides
//! produce the record documented in the ffi module (statuses `0` = `SM_SUCCESS`, else the
//! `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot produce;
//! op values only on success), reported as the values `rec`. A case whose Rust side reaches an
//! unported unit is deferred as a whole. Every case's C++ side runs in a fresh child
//! process, where `ComputePointValues`' function-static `sdStaticStep` (ND-3) starts
//! unlatched as on a first call, and every Rust case on a fresh [`Kernel`]; the step each
//! side leaves latched is compared as `fillet_point_step` (the C++ one read in the child).
//!
//! The constant-radius `SetupOffsetValues` dispatch runs the ported standard solver,
//! so `ComputePointValuesDelta` can compare both mutated surface records.
//! Deferred cases reach unavailable solver calls (`SmFilletSolver::StepSolve` in
//! `ComputeNextPoint` from a crossing point, `ComputePointValues` in solver p4,
//! `PointOnPlaneSolve` in `EvaluateLawPoint`)
//! or the span test past its angle check on a plane
//! (`SmCrvOnSurf` over an `SmPlane`, a dispatch gap of `curve.crv_on_surf`). `FlushCurve` of two
//! or more points is not run: in C++ it dereferences the solver's unset executive and the
//! fillet geometry's `NULL` rails (the Rust side evaluates the offset rails, then stops at
//! `SmFilletSolver::GetSelfIntersectionHandler` with an invalid-input error for the missing
//! executive; unit tests cover it).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use solidcore::blend::fillet_custom_solver::{ConstantRadiusAssistedFs, OffsetSurface, SurfaceId};
use solidcore::blend::fillet_geom::p1::{FilletBrep, FilletGeomId, FilletSolverId};
use solidcore::blend::fillet_intersector::{
    find_guess_mid_tsect_pnt, flush_crv, FilletIntersector, TYPE,
};
use solidcore::curve::curve::Curve;
use solidcore::geom::extent1d::{Extent1d, ExtentPointType};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::adv_surface_intersector::{NoSurfaceHandle, TYPE as ADV_TYPE};
use solidcore::intersect::surface_intersector::p1::{IntersectionPointType, TsectId, TsectPnt};
use solidcore::intersect::surface_intersector::p2::TsectCurves;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::offset_surface::SS_UNKNOWN;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::SolidError;
use solidcore_ffi::units::blend_fillet_intersector::{self as ffi};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::{flag, p2, v3};
use crate::{Case, CaseSource, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1200;

/// `blend.fillet_intersector` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

const NOW: f64 = -f64::MAX;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SolverIn {
    radii: [f64; 2],
    orient: [bool; 2],
    tol: f64,
    ang_tol: f64,
    tangency_tol: f64,
}

/// An intersection point; the parameter types are `SM_EP_INSIDE` and the surface
/// matrices zero unless given.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PointIn {
    /// `SmIntersectionPointType`.
    t: u8,
    uv: [[f64; 2]; 2],
    duv: [[f64; 2]; 2],
    pos: [f64; 3],
    deriv: [f64; 3],
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
#[serde(rename_all = "snake_case", tag = "op")]
enum OpIn {
    State,
    Span {
        a: usize,
        b: usize,
        mid: Option<usize>,
    },
    Next {
        a: usize,
        step: f64,
        b: usize,
    },
    Flush {
        ids: Vec<usize>,
    },
    Mid {
        a: usize,
        b: usize,
        f: f64,
    },
    FlushCrv {
        dim: u32,
        knots: Vec<f64>,
        poly: Vec<[f64; 3]>,
    },
    Delta {
        uv: [[f64; 2]; 2],
        uv_delta: [[f64; 2]; 2],
        a: usize,
        b: usize,
    },
    Values {
        uv: [[f64; 2]; 2],
        a: usize,
        prev: Option<usize>,
    },
    Law {
        param: f64,
        uv0: [f64; 2],
        uv1: [f64; 2],
        origin: [f64; 3],
        normal: [f64; 3],
    },
    SetSelf {
        flag: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    a: SurfaceIn,
    /// `None`: B is A itself.
    b: Option<SurfaceIn>,
    dom1: [f64; 4],
    dom2: [f64; 4],
    solver: SolverIn,
    ang_tol: f64,
    approx_tol: f64,
    clipped: bool,
    trace_dir: f64,
    points: Vec<PointIn>,
    op: OpIn,
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
    v.extend([1.0; 4]);
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

fn ffi_surface(s: &SurfaceIn) -> ffi::SurfaceIn {
    match s {
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
    }
}

fn ffi_op(o: &OpIn) -> ffi::Op {
    match o {
        OpIn::State => ffi::Op::State,
        OpIn::Span { a, b, mid } => ffi::Op::Span {
            a: *a,
            b: *b,
            mid: *mid,
        },
        OpIn::Next { a, step, b } => ffi::Op::Next {
            a: *a,
            step: *step,
            b: *b,
        },
        OpIn::Flush { ids } => ffi::Op::Flush(ids.clone()),
        OpIn::Mid { a, b, f } => ffi::Op::Mid {
            a: *a,
            b: *b,
            f: *f,
        },
        OpIn::FlushCrv { dim, knots, poly } => ffi::Op::FlushCrv {
            dim: *dim,
            knots: knots.clone(),
            poly: poly.clone(),
        },
        OpIn::Delta { uv, uv_delta, a, b } => ffi::Op::Delta {
            uv: *uv,
            uv_delta: *uv_delta,
            a: *a,
            b: *b,
        },
        OpIn::Values { uv, a, prev } => ffi::Op::Values {
            uv: *uv,
            a: *a,
            prev: *prev,
        },
        OpIn::Law {
            param,
            uv0,
            uv1,
            origin,
            normal,
        } => ffi::Op::Law {
            param: *param,
            uv0: *uv0,
            uv1: *uv1,
            origin: *origin,
            normal: *normal,
        },
        OpIn::SetSelf { flag } => ffi::Op::SetSelf(*flag),
    }
}

fn ffi_program(i: &Inputs) -> ffi::Program {
    ffi::Program {
        a: ffi_surface(&i.a),
        b: i.b.as_ref().map(ffi_surface),
        dom1: i.dom1,
        dom2: i.dom2,
        solver: ffi::SolverIn {
            radii: i.solver.radii,
            orient: i.solver.orient,
            tol: i.solver.tol,
            ang_tol: i.solver.ang_tol,
            tangency_tol: i.solver.tangency_tol,
        },
        ang_tol: i.ang_tol,
        approx_tol: i.approx_tol,
        clipped: i.clipped,
        trace_dir: i.trace_dir,
        points: i.points.iter().map(point_values).collect(),
        op: ffi_op(&i.op),
    }
}

// ---- C++ side -----------------------------------------------------------------------------

/// Compared key: the `sdStaticStep` latch (ND-3: `SmFilletSolver::ComputePointValues`'
/// function static, fixed by the first call in a process) after the case.
const POINT_STEP: &str = "fillet_point_step";

/// Every case's C++ side runs in a fresh child process
/// ([`with_fresh_static_step`](solidcore_ffi::units::blend_fillet_solver_p4::with_fresh_static_step)),
/// where `sdStaticStep` starts unlatched as on a first call whatever ran before; the step
/// the case leaves latched is compared, never seeded into the Rust side.
fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let program = ffi_program(i);
    match solidcore_ffi::units::blend_fillet_solver_p4::with_fresh_static_step(|| {
        ffi::run(&program)
    }) {
        Ok((rec, step)) => Ok(Output::new()
            .values("rec", rec)
            .values(POINT_STEP, vec![step])),
        Err(e) => Err(harness(format!("C++: {e}"))),
    }
}

// ---- Rust side ----------------------------------------------------------------------------

/// A failed Rust operation: the whole case is deferred, or the harness failed.
enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Fail::Harness(e)
    }
}

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// `Ok(v)` (status `0` pushed), an op status (pushed, `None`), or a deferral.
fn st<T>(o: &mut Vec<f64>, r: solidcore::Result<T>) -> Result<Option<T>, Fail> {
    match r {
        Ok(v) => {
            o.push(0.0);
            Ok(Some(v))
        }
        Err(e) => {
            if let Some(d) = Output::unported(&e) {
                return Err(Fail::Deferred(Box::new(d)));
            }
            o.push(status(&e));
            Ok(None)
        }
    }
}

fn ext(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn rust_surface(s: &SurfaceIn) -> Result<Surface, HarnessError> {
    match s {
        SurfaceIn::Plane {
            o,
            x,
            y,
            scale,
            domain,
        } => Ok(Surface::from(Plane::new(
            v3(*o),
            v3(*x),
            v3(*y),
            p2(*scale),
            &ext(*domain),
        ))),
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
            Ok(Surface::from(BSplineSurface::from_nurb(sur, false)))
        }
    }
}

fn ept(code: f64) -> Result<ExtentPointType, HarnessError> {
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

fn put_vec(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_uv(o: &mut Vec<f64>, v: Vec2) {
    o.extend([v.x, v.y]);
}

fn put_dom(o: &mut Vec<f64>, e: &Extent2d) {
    o.extend([e.min().x, e.min().y, e.max().x, e.max().y]);
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
        put_uv(o, *uv);
    }
    for vec in t.surface_pv.iter().flatten().flatten() {
        put_vec(o, *vec);
    }
    put_vec(o, t.curve_pv[0]);
    put_vec(o, t.curve_pv[1]);
    o.extend([t.curve_parameter, t.tangent_plane_angle_rad, t.deviation]);
    o.extend(t.user_doubles);
}

fn put_bspline(o: &mut Vec<f64>, c: &Curve) -> Result<(), Fail> {
    o.extend([c.type_code() as f64, c.dim() as f64]);
    let solidcore::curve::curve::CurveKind::BSpline(bs) = c.kind() else {
        return Err(Fail::Harness(harness("flushed curve is not a B-spline")));
    };
    o.extend([
        f64::from(bs.knot_type() as i32),
        f64::from(bs.form() as i32),
    ]);
    match bs.gw_nurb() {
        None => o.push(0.0),
        Some(cur) => {
            o.extend([1.0, cur.n() as f64, cur.degree() as f64, cur.m() as f64]);
            for p in cur.cpts() {
                o.extend([p.x, p.y, p.z, p.w]);
            }
            o.extend_from_slice(cur.knots());
        }
    }
    let ivl = c.natural_interval();
    o.extend([ivl.min(), ivl.max()]);
    Ok(())
}

fn id(i: usize) -> TsectId {
    TsectId(i)
}

type Fi<'s> = FilletIntersector<'s, 'static, NoSurfaceHandle, FilletGeomId>;

/// Stores `p` as arena point `i`: the shim hands its points to the op by reference, so
/// what the op wrote (also on a failure) is in the emitted arena.
fn write_back(fi: &mut Fi<'_>, i: usize, p: TsectPnt) -> Result<(), Fail> {
    *fi.base
        .base
        .point_mut(id(i))
        .map_err(|e| Fail::Harness(harness(e.to_string())))? = p;
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn run_op(
    op: &OpIn,
    k: &Kernel,
    fi: &mut Fi<'_>,
    solver: &mut ConstantRadiusAssistedFs,
    pseudo: &mut FilletBrep,
    o: &mut Vec<f64>,
) -> Result<(), Fail> {
    match op {
        OpIn::State => {}
        OpIn::Span { a, b, mid } => {
            let r = fi.test_span_accuracy(solver, k, id(*a), id(*b), mid.map(id));
            if let Some(s) = st(o, r)? {
                o.push(flag(s.found_good_point));
                o.push(s.deviation_found.unwrap_or(ffi::UNSET));
                o.push(s.angle_found_rad);
            }
        }
        OpIn::Next { a, step, b } => {
            let r = fi.compute_next_point(solver, k, id(*a), *step, id(*b));
            if let Some(s) = st(o, r)? {
                o.push(flag(s.found_good_point));
                o.push(s.deviation_found.unwrap_or(ffi::UNSET));
                o.push(s.angle_found_rad.unwrap_or(ffi::UNSET));
                o.push(flag(s.clipped));
                o.push(flag(s.boundary_hit));
            }
        }
        OpIn::Flush { ids } => {
            for &i in ids {
                fi.base
                    .base
                    .add_point_to_curve(id(i))
                    .map_err(|e| Fail::Harness(harness(format!("add point: {e}"))))?;
            }
            let mut out = TsectCurves::default();
            let r = fi.flush_curve(solver, k, pseudo, &mut out);
            if st(o, r)?.is_some() {
                o.push(out.curve_types.len() as f64);
                o.push(out.deviations.len() as f64);
            }
        }
        OpIn::Mid { a, b, f } => {
            let pa = fi
                .base
                .base
                .point(id(*a))
                .map_err(|e| Fail::Harness(harness(e.to_string())))?;
            let pb = fi
                .base
                .base
                .point(id(*b))
                .map_err(|e| Fail::Harness(harness(e.to_string())))?;
            let mut mid = TsectPnt::new();
            find_guess_mid_tsect_pnt(pa, pb, *f, &mut mid);
            o.push(0.0);
            put_point(o, &mid);
        }
        OpIn::FlushCrv { dim, knots, poly } => {
            let poly: Vec<Vec3> = poly.iter().map(|p| v3(*p)).collect();
            let mut curves = Vec::new();
            let r = flush_crv(u64::from(*dim), &poly, knots, &mut curves);
            if st(o, r)?.is_some() {
                o.push(curves.len() as f64);
                for c in curves {
                    put_bspline(o, &Curve::from(c))?;
                }
            }
        }
        OpIn::Delta { uv, uv_delta, a, b } => {
            let mut pa = fi
                .base
                .base
                .point(id(*a))
                .map_err(|e| Fail::Harness(harness(e.to_string())))?
                .clone();
            let mut pb = fi
                .base
                .base
                .point(id(*b))
                .map_err(|e| Fail::Harness(harness(e.to_string())))?
                .clone();
            let r = fi.compute_point_values_delta(
                solver,
                k,
                uv.map(p2),
                uv_delta.map(p2),
                &mut pa,
                &mut pb,
            );
            // the shim passes the arena points by reference: they are emitted as changed
            write_back(fi, *a, pa)?;
            write_back(fi, *b, pb)?;
            st(o, r)?;
        }
        OpIn::Values { uv, a, prev } => {
            let mut pa = fi
                .base
                .base
                .point(id(*a))
                .map_err(|e| Fail::Harness(harness(e.to_string())))?
                .clone();
            let pp = match prev {
                Some(p) => Some(
                    fi.base
                        .base
                        .point(id(*p))
                        .map_err(|e| Fail::Harness(harness(e.to_string())))?
                        .clone(),
                ),
                None => None,
            };
            let r = fi.compute_point_values(solver, k, uv.map(p2), &mut pa, pp.as_ref(), None);
            write_back(fi, *a, pa)?;
            st(o, r)?;
        }
        OpIn::Law {
            param,
            uv0,
            uv1,
            origin,
            normal,
        } => {
            let r = fi.evaluate_law_point(
                solver,
                k,
                *param,
                p2(*uv0),
                p2(*uv1),
                &(),
                false,
                &Extent1d::from_min_max(0.0, 1.0),
                [1.0, 1.0],
                v3(*origin),
                v3(*normal),
            );
            if let Some((cp, uvs)) = st(o, r)? {
                o.push(cp);
                put_uv(o, uvs[0]);
                put_uv(o, uvs[1]);
            }
        }
        OpIn::SetSelf { flag } => {
            fi.set_self_intersect(*flag);
            o.push(0.0);
        }
    }
    Ok(())
}

fn op_code(op: &OpIn) -> f64 {
    match op {
        OpIn::State => 0.0,
        OpIn::Span { .. } => 1.0,
        OpIn::Next { .. } => 2.0,
        OpIn::Flush { .. } => 3.0,
        OpIn::Mid { .. } => 4.0,
        OpIn::FlushCrv { .. } => 5.0,
        OpIn::Delta { .. } => 6.0,
        OpIn::Values { .. } => 7.0,
        OpIn::Law { .. } => 8.0,
        OpIn::SetSelf { .. } => 9.0,
    }
}

/// A rail's `SmOffsetSurface(d, base)` as `SmConstantRadiusFS` builds it (default
/// `lSingularities`, `SS_UNKNOWN`).
fn rail_offset(k: &Kernel, d: f64, base: Surface, id: SurfaceId) -> Result<OffsetSurface, Fail> {
    OffsetSurface::new(k, d, base, id, SS_UNKNOWN).map_err(|e| match Output::unported(&e) {
        Some(o) => Fail::Deferred(Box::new(o)),
        None => Fail::Harness(harness(format!("offset surface: {e}"))),
    })
}

fn rust_record(k: &Kernel, i: &Inputs) -> Result<Vec<f64>, Fail> {
    let a = rust_surface(&i.a)?;
    let b = match &i.b {
        Some(b) => Some(rust_surface(b)?),
        None => None,
    };
    let bref = b.as_ref().unwrap_or(&a);
    let s = &i.solver;
    let orient = s.orient.map(|f| if f { 1.0 } else { -1.0 });
    let off0 = rail_offset(k, s.radii[0] * orient[0], a.clone(), SurfaceId::Free(0))?;
    let off1 = rail_offset(
        k,
        s.radii[1] * orient[1],
        bref.clone(),
        SurfaceId::Free(u32::from(b.is_some())),
    )?;
    let mut solver = ConstantRadiusAssistedFs::from_parts(
        s.tol,
        s.ang_tol,
        s.tangency_tol,
        s.radii,
        [Some(off0), Some(off1)],
        orient,
        [None, None],
    );
    for slot in solver.solver.surfaces.iter_mut().flatten() {
        slot.surface.surface.set_allow_extension(true);
    }
    // the base constructor's fillet geometry (with the offset copies FinishConstruction
    // installs), in the pseudo B-rep the flush writes to
    let mut pseudo = FilletBrep::new();
    if let Err(e) = solver
        .solver
        .add_constructor_fillet_geom(k, &mut pseudo, FilletSolverId(0))
    {
        return Err(Fail::Harness(harness(format!("fillet geometry: {e}"))));
    }

    let mut fi: Fi<'_> =
        match FilletIntersector::new(k, &a, &ext(i.dom1), bref, &ext(i.dom2), None) {
            Ok(fi) => fi,
            Err(e) => {
                if let Some(d) = Output::unported(&e) {
                    return Err(Fail::Deferred(Box::new(d)));
                }
                return Err(Fail::Harness(harness(format!("constructor: {e}"))));
            }
        };
    fi.base.base.this_ang_tol_rad = i.ang_tol;
    fi.base.base.this_approx_tol_3d = i.approx_tol;
    fi.base.base.clipped_by_through_point = i.clipped;
    fi.base.base.curve_trace_direction = i.trace_dir;
    for p in &i.points {
        let t = rust_point(&point_values(p))?;
        fi.base.base.alloc_point(t);
    }

    let mut o = vec![op_code(&i.op)];
    run_op(&i.op, k, &mut fi, &mut solver, &mut pseudo, &mut o)?;

    // state
    let si = &fi.base.base;
    put_dom(&mut o, &si.uv_domains[0]);
    put_dom(&mut o, &si.uv_domains[1]);
    o.extend([
        si.this_ang_tol_rad,
        si.this_approx_tol_3d,
        flag(si.do_boundary_point_on_curve_test),
        flag(fi.curr_fillet_touch_boundary()),
        flag(fi.self_intersect()),
        flag(si.clipped_by_through_point),
        si.curve_trace_direction,
        flag(fi.is_kind_of(TYPE)),
        flag(fi.is_kind_of(ADV_TYPE)),
        flag(fi.is_kind_of(7)),
    ]);
    for slot in &solver.solver.surfaces {
        o.push(
            slot.as_ref()
                .map_or(ffi::UNSET, |s| s.surface.offset_distance()),
        );
    }
    o.push(si.curve_points().len() as f64);
    o.push(si.points().len() as f64);
    for p in si.points() {
        put_point(&mut o, p);
    }
    Ok(o)
}

/// Every case runs on a fresh kernel, whose `sdStaticStep` latch sets itself.
fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    match rust_record(&k, i) {
        // read as the C++ probe reads `sdStaticStep` (its crossing point latches 1e-5)
        Ok(rec) => Ok(Output::new().values("rec", rec).values(
            POINT_STEP,
            vec![k.fillet_point_step.get().unwrap_or(1.0e-5)],
        )),
        Err(Fail::Deferred(d)) => Ok(*d),
        Err(Fail::Harness(e)) => Err(e),
    }
}

// ---- generated cases ----------------------------------------------------------------------

fn gen_scale(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0 => 1e-3,
        1 => 1e3,
        2 => 1e-6,
        3 => 1e5,
        _ => rng.uniform(0.5, 4.0),
    }
}

fn unit_vec(rng: &mut Rng) -> [f64; 3] {
    loop {
        let v = rng.point3(-1.0, 1.0);
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if l > 0.2 {
            return [v[0] / l, v[1] / l, v[2] / l];
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

fn gen_plane(rng: &mut Rng, s: f64) -> SurfaceIn {
    let o = rng.point3(-s, s);
    let x = unit_vec(rng);
    let mut y = cross(unit_vec(rng), x);
    let l = (y[0] * y[0] + y[1] * y[1] + y[2] * y[2]).sqrt();
    if l > 1e-3 {
        y = [y[0] / l, y[1] / l, y[2] / l];
    } else {
        y = cross(x, [0.0, 0.0, 1.0]);
    }
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
        o,
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

fn gen_bspline(rng: &mut Rng, s: f64) -> SurfaceIn {
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
            let (x, y, z) = (
                s * i as f64 / (nu - 1) as f64,
                s * j as f64 / (nv - 1) as f64,
                s * rng.uniform(-bend, bend),
            );
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

fn natural(s: &SurfaceIn) -> [f64; 4] {
    match s {
        SurfaceIn::Plane { domain, .. } => *domain,
        SurfaceIn::BSpline { u, v, .. } => [
            u.first().copied().unwrap_or(0.0),
            v.first().copied().unwrap_or(0.0),
            u.last().copied().unwrap_or(1.0),
            v.last().copied().unwrap_or(1.0),
        ],
    }
}

fn gen_domain(rng: &mut Rng, nat: [f64; 4]) -> [f64; 4] {
    let [a, b0, c, d] = nat;
    let (w, h) = (c - a, d - b0);
    match rng.below(10) {
        0 => [a + 0.1 * w, b0 + 0.2 * h, c - 0.3 * w, d - 0.1 * h],
        1 => [a - 0.2 * w, b0 - 0.1 * h, c + 0.1 * w, d + 0.3 * h],
        2 => [a, b0, a + 1e-9 * w, d],
        _ => nat,
    }
}

fn lerp_dom(d: [f64; 4], fu: f64, fv: f64) -> [f64; 2] {
    [d[0] + fu * (d[2] - d[0]), d[1] + fv * (d[3] - d[1])]
}

/// A point of the generated trace at `t`: UV points `base + t dir` on both surfaces,
/// the curve point on the first surface and its unit tangent (central differences),
/// UV tangents scaled to the unit-speed parameter `t |tangent|`.
fn trace_point(
    k: &Kernel,
    srfs: &[Surface; 2],
    base: [[f64; 2]; 2],
    dir: [[f64; 2]; 2],
    t: f64,
) -> PointIn {
    let uv = [
        [base[0][0] + t * dir[0][0], base[0][1] + t * dir[0][1]],
        [base[1][0] + t * dir[1][0], base[1][1] + t * dir[1][1]],
    ];
    let at = |s: &Surface, uv: [f64; 2]| s.evaluate_point(k, p2(uv)).ok();
    let h = 1e-4;
    let p0 = at(&srfs[0], uv[0]).unwrap_or(Vec3::new(0.0, 0.0, 0.0));
    let pp = at(
        &srfs[0],
        [uv[0][0] + h * dir[0][0], uv[0][1] + h * dir[0][1]],
    );
    let pm = at(
        &srfs[0],
        [uv[0][0] - h * dir[0][0], uv[0][1] - h * dir[0][1]],
    );
    let tan = match (pp, pm) {
        (Some(a), Some(b)) => (a - b) / (2.0 * h),
        _ => Vec3::new(1.0, 0.0, 0.0),
    };
    let len = tan.length();
    let (deriv, speed) = if len > 0.0 && len.is_finite() {
        (tan / len, len)
    } else {
        (Vec3::new(1.0, 0.0, 0.0), 1.0)
    };
    PointIn {
        t: 2,
        uv,
        duv: [
            [dir[0][0] / speed, dir[0][1] / speed],
            [dir[1][0] / speed, dir[1][1] / speed],
        ],
        pos: [p0.x, p0.y, p0.z],
        deriv: [deriv.x, deriv.y, deriv.z],
        param: t * speed,
        angle: 0.0,
        dev: 0.0,
        user: [0.0; 4],
        srf: None,
    }
}

fn rot(v: [f64; 3], ang: f64) -> [f64; 3] {
    // rotate about an axis perpendicular to v (in the plane of v and z, or x)
    let w = if v[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let n = cross(v, w);
    let ln = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-300);
    let n = [n[0] / ln, n[1] / ln, n[2] / ln];
    let (c, s) = (ang.cos(), ang.sin());
    [
        v[0] * c + n[0] * s,
        v[1] * c + n[1] * s,
        v[2] * c + n[2] * s,
    ]
}

#[allow(clippy::too_many_lines)]
fn gen_case(k: &Kernel, rng: &mut Rng) -> Option<Inputs> {
    let s = gen_scale(rng);
    let a = if rng.chance(0.65) {
        gen_bspline(rng, s)
    } else {
        gen_plane(rng, s)
    };
    let b = match rng.below(10) {
        0 => None,
        1..=6 => Some(gen_bspline(rng, s)),
        _ => Some(gen_plane(rng, s)),
    };
    let na = natural(&a);
    let nb = natural(b.as_ref().unwrap_or(&a));
    let dom1 = gen_domain(rng, na);
    let dom2 = gen_domain(rng, nb);
    let sa = rust_surface(&a).ok()?;
    let sb = rust_surface(b.as_ref().unwrap_or(&a)).ok()?;
    let srfs = [sa, sb];

    let r = s * rng.uniform(0.01, 0.5);
    let solver = SolverIn {
        radii: [r, if rng.chance(0.7) { r } else { -r * 0.5 }],
        orient: [rng.chance(0.5), rng.chance(0.5)],
        tol: s * 1e-3,
        ang_tol: 30.0_f64.to_radians(),
        tangency_tol: 2.0_f64.to_radians(),
    };
    let ang_tol = match rng.below(5) {
        0 => 1e-3,
        1 => rng.uniform(0.01, 0.2),
        2 => 3.5,
        _ => 20.0_f64.to_radians(),
    };
    let approx_tol = s * match rng.below(5) {
        0 => 1e-9,
        1 => 1e-1,
        2 => 0.0,
        _ => 1e-3,
    };
    let clipped = rng.chance(0.2);
    let trace_dir = if rng.chance(0.5) { 1.0 } else { -1.0 };

    // a trace through both UV domains
    let base = [
        lerp_dom(na, rng.uniform(0.1, 0.9), rng.uniform(0.1, 0.9)),
        lerp_dom(nb, rng.uniform(0.1, 0.9), rng.uniform(0.1, 0.9)),
    ];
    let mut dir = [[0.0; 2]; 2];
    for (d, n) in dir.iter_mut().zip([na, nb]) {
        let ang = rng.uniform(0.0, core::f64::consts::TAU);
        let mag = match rng.below(4) {
            0 => 1e-6,
            1 => 1e-3,
            2 => 0.05,
            _ => 0.3,
        };
        *d = [
            mag * (n[2] - n[0]) * ang.cos(),
            mag * (n[3] - n[1]) * ang.sin(),
        ];
    }
    let t1 = match rng.below(5) {
        0 => 1e-15,
        1 => 0.05,
        _ => rng.uniform(0.1, 1.0),
    };
    let mut p0 = trace_point(k, &srfs, base, dir, 0.0);
    let mut p1 = trace_point(k, &srfs, base, dir, t1);
    let pm = trace_point(k, &srfs, base, dir, t1 * rng.uniform(0.3, 0.7));
    // variants: turn the tangent, zero it, change the types, angles and deviations
    match rng.below(8) {
        0 => p1.deriv = rot(p1.deriv, rng.uniform(0.0, 3.1)),
        1 => p1.deriv = [0.0; 3],
        2 => p0.deriv = [0.0; 3],
        3 => p1.deriv = rot(p1.deriv, rng.uniform(0.0, 0.05)),
        _ => {}
    }
    if rng.chance(0.3) {
        p0.t = rng.below(8) as u8;
    }
    if rng.chance(0.3) {
        p1.t = if rng.chance(0.5) {
            5
        } else {
            rng.below(8) as u8
        };
    }
    p0.angle = rng.uniform(0.0, core::f64::consts::FRAC_PI_2);
    p1.angle = if rng.chance(0.3) {
        0.0
    } else {
        rng.uniform(0.0, 1.5)
    };
    p0.dev = rng.uniform(0.0, 1e-3) * s;
    p1.dev = rng.uniform(0.0, 1e-3) * s;
    if rng.chance(0.2) {
        p1.param = p0.param - rng.uniform(0.0, 1.0);
    }
    let (uv_a, uv_b, pos_a, der_a) = (p0.uv, p1.uv, p0.pos, p0.deriv);
    let mut points = vec![p0, p1, pm];

    let op = match rng.below(20) {
        0 => OpIn::State,
        1 => OpIn::SetSelf {
            flag: rng.chance(0.5),
        },
        2..=3 => OpIn::Mid {
            a: 0,
            b: 1,
            f: match rng.below(4) {
                0 => 0.5,
                1 => rng.uniform(-0.5, 1.5),
                2 => 0.0,
                _ => rng.unit(),
            },
        },
        4..=5 => {
            let n = rng.below(5) as usize;
            let dim = 1 + rng.below(3) as u32;
            let mut knots = Vec::new();
            let mut k0 = rng.uniform(-s, s);
            if n > 0 {
                for _ in 0..=n {
                    knots.push(k0);
                    k0 += rng.uniform(1e-3, 1.0) * s;
                }
            }
            let np = if knots.is_empty() {
                0
            } else {
                3 * (knots.len() - 1) + 1
            };
            let poly = (0..np).map(|_| rng.point3(-s, s)).collect();
            OpIn::FlushCrv { dim, knots, poly }
        }
        6..=12 => OpIn::Span {
            a: 0,
            b: 1,
            mid: if rng.chance(0.5) { Some(2) } else { None },
        },
        13..=15 => {
            let step = match rng.below(3) {
                0 => 1e-16,
                _ => {
                    points.get(1).map_or(0.1, |p| p.param) - points.first().map_or(0.0, |p| p.param)
                }
            };
            OpIn::Next { a: 0, step, b: 1 }
        }
        16..=17 => {
            let n = rng.below(2) as usize;
            if n == 1 && rng.chance(0.5) {
                // put the point on a domain boundary
                if let Some(p) = points.get_mut(1) {
                    p.uv[0][0] = dom1[2];
                }
            }
            OpIn::Flush {
                ids: (0..n).map(|_| 1).collect(),
            }
        }
        18 => OpIn::Values {
            uv: uv_a,
            a: 1,
            prev: if rng.chance(0.5) { Some(0) } else { None },
        },
        _ => {
            if rng.chance(0.5) {
                OpIn::Delta {
                    uv: uv_a,
                    uv_delta: uv_b,
                    a: 0,
                    b: 1,
                }
            } else {
                OpIn::Law {
                    param: rng.unit(),
                    uv0: uv_a[0],
                    uv1: uv_a[1],
                    origin: pos_a,
                    normal: der_a,
                }
            }
        }
    };
    if points.iter().any(|p| {
        point_values(p)
            .iter()
            .any(|x| !x.is_finite() || x.abs() > ffi::MAX_COORD)
    }) {
        return None;
    }
    Some(Inputs {
        a,
        b,
        dom1,
        dom2,
        solver,
        ang_tol,
        approx_tol,
        clipped,
        trace_dir,
        points,
        op,
    })
}

// ---- edge cases ---------------------------------------------------------------------------

fn plane_json(o: [f64; 3], x: [f64; 3], y: [f64; 3], d: [f64; 4]) -> Value {
    json!({"kind": "plane", "o": o, "x": x, "y": y, "scale": [1.0, 1.0], "domain": d})
}

fn pt_json(
    t: u8,
    uv0: [f64; 2],
    uv1: [f64; 2],
    pos: [f64; 3],
    deriv: [f64; 3],
    param: f64,
) -> Value {
    json!({"t": t, "uv": [uv0, uv1], "duv": [[1.0, 0.0], [1.0, 0.0]], "pos": pos,
           "deriv": deriv, "param": param})
}

fn edge_cases() -> Vec<(String, Value)> {
    let xy = plane_json(
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0, 1.0],
    );
    let xz = plane_json(
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 1.0, 1.0],
    );
    let d = [0.0, 0.0, 1.0, 1.0];
    let solver = json!({"radii": [0.1, 0.1], "orient": [true, true], "tol": 1e-3,
                        "ang_tol": 0.5, "tangency_tol": 0.03});
    let pts = json!([
        pt_json(
            2,
            [0.1, 0.0],
            [0.1, 0.0],
            [0.1, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            0.0
        ),
        pt_json(
            2,
            [0.2, 0.0],
            [0.2, 0.0],
            [0.2, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            0.1
        ),
        pt_json(
            5,
            [1.0, 0.5],
            [0.2, 0.0],
            [1.0, 0.5, 0.0],
            [1.0, 0.0, 0.0],
            0.2
        ),
        pt_json(
            4,
            [0.1, 0.0],
            [0.1, 0.0],
            [0.1, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            0.0
        ),
        pt_json(
            2,
            [0.3, 0.0],
            [0.3, 0.0],
            [0.3, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            0.2
        ),
    ]);
    let case = |op: Value| {
        json!({"a": xy, "b": xz, "dom1": d, "dom2": d, "solver": solver, "ang_tol": 0.35,
               "approx_tol": 1e-3, "clipped": false, "trace_dir": 1.0, "points": pts, "op": op})
    };
    vec![
        ("edge_state".into(), case(json!({"op": "state"}))),
        (
            "edge_set_self".into(),
            case(json!({"op": "set_self", "flag": true})),
        ),
        (
            "edge_span_turn".into(),
            case(json!({"op": "span", "a": 0, "b": 1, "mid": null})),
        ),
        (
            "edge_span_singular_turn".into(),
            case(json!({"op": "span", "a": 0, "b": 2, "mid": null})),
        ),
        (
            "edge_span_zero_next".into(),
            case(json!({"op": "span", "a": 1, "b": 4, "mid": 3})),
        ),
        (
            "edge_span_zero_current".into(),
            case(json!({"op": "span", "a": 4, "b": 1, "mid": null})),
        ),
        (
            "edge_next_tangent_curve".into(),
            case(json!({"op": "next", "a": 3, "step": 0.1, "b": 1})),
        ),
        (
            "edge_flush_empty".into(),
            case(json!({"op": "flush", "ids": []})),
        ),
        (
            "edge_flush_boundary".into(),
            case(json!({"op": "flush", "ids": [2]})),
        ),
        (
            "edge_flush_inside".into(),
            case(json!({"op": "flush", "ids": [0]})),
        ),
        (
            "edge_mid_half".into(),
            case(json!({"op": "mid", "a": 0, "b": 1, "f": 0.5})),
        ),
        (
            "edge_flush_crv_none".into(),
            case(json!({"op": "flush_crv", "dim": 3, "knots": [], "poly": []})),
        ),
        (
            "edge_flush_crv_two_spans".into(),
            case(
                json!({"op": "flush_crv", "dim": 2, "knots": [0.0, 0.5, 2.0],
                        "poly": [[0,0,0],[1,0,0],[2,1,0],[3,1,0],[4,0,0],[5,2,0],[6,0,0]]}),
            ),
        ),
    ]
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_intersector"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let k = &Kernel::default();
        for _ in 0..64 {
            let Some(i) = gen_case(k, rng) else { continue };
            if ffi::encode(&ffi_program(&i)).is_ok() {
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
        for (name, inputs) in edge_cases() {
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

// Separate from the corpus and its 189-value Next record. Runs only in the
// existing native-feature runner tests; no tracing in ordinary comparisons.
#[cfg(test)]
#[path = "blend_fillet_intersector/span_probe.rs"]
mod span_probe;
