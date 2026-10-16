//! FFI wrappers for unit `intersect.surface_tracer`.
//!
//! Shim: `harness/cpp/shim/intersect_surface_tracer.cpp` (`sc_intersect_surface_tracer_run`).
//! Upstream: `source/SMLib/src/SmSurfaceTracer.cpp`, `source/SMLib/inc/SmSurfaceTracer.h`
//! (`SmSurfaceTracer`).
//!
//! [`run`] builds a surface, a `SectionTracer` (the shim's `SmSurfaceTracer` subclass tracing
//! the curves where the surface meets the plane `n.(P - p0) = 0`) over a domain, applies the
//! optional tolerances (as `DoTrace` stores them), runs the [`Op`]s in order and returns the
//! record:
//!
//! * the constructor's approximation tolerance;
//! * per op `op status [values]` (see the table);
//! * the final state: `approx_tol ang_tol direction closed curve_point_count`, the start
//!   points `count (type u v)*`, then the accumulated 3D curves, UV curves, drop distances and
//!   deviations (`count` then items).
//!
//! Statuses are `0` (`SM_SUCCESS`) or the `SmStatus`. A curve is `type dim ivl[2]`, then for
//! B-splines that are not lines `has [n p m cpts(x y z w) knots]`.
//!
//! | [`Op`]      | upstream                                                   | values |
//! |-------------|------------------------------------------------------------|--------|
//! | `Step`      | `ComputeStepSize(pnt, old, new)`                           | new |
//! | `Trace`     | `FindInteriorCurves`' loop body for a node whose middle is `uv` (refine, `IsPointOnCurve`, `ComputePointValues`, `TraceCurve` forward, `ReverseCurveDirection`, `TraceCurve` backward, `FlushCurve`) | `stage status closed n3d` (stage `0` = flushed, `1`..`6` = the `continue` taken) |
//! | `Flush`     | the points appended to `m_vCurvePoints`, then `FlushCurve(.., opt_curve)` | `n3d` |
//! | `AddCurve`  | a curve appended to the accumulated 3D curves              | |
//! | `OnCurve`   | `IsPointOnCurve(p, curves)`                                | flag |
//! | `Iso`       | `TestIsoCurve(param, value)`                               | flag |
//! | `Start`     | new points offered to `AddStartPoint` in order             | start points |
//! | `Test`      | `TestPoint` on a new point                                 | `done closed uv[2] p[3]` start points (status 0 only) |
//! | `DoTrace`   | `DoTrace(ctx, &tol, &ang, ..)` with the current tolerances | 3D curves, UV curves, drops, devs (status 0 only) |
//!
//! [`run`] accepts only finite inputs of magnitude at most [`MAX_COORD`]; well-formed clamped
//! B-splines (at most [`MAX_INDEX`] + 1 control points per surface direction, 33 per curve,
//! degree at most [`MAX_DEGREE`], positive weights or none); planes with independent axes and
//! positive scales; a trace domain with positive extent inside the surface's natural domain;
//! at most [`MAX_OPS`] ops, [`MAX_TRACES`] `Trace` and one `DoTrace`; `Flush` only before any
//! `Trace` or `DoTrace` (a trace that fails leaves its points on the curve, whose parameters
//! would then not increase: upstream builds an invalid knot vector) with at most 64 points of
//! strictly increasing parameters.

use core::cmp::Ordering;

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_intersect_surface_tracer_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Largest control point index `n` of a surface direction.
pub const MAX_INDEX: usize = 16;
/// Largest control point index `n` of a curve.
pub const MAX_CURVE_INDEX: usize = 32;
/// Largest degree.
pub const MAX_DEGREE: usize = 8;
/// Largest input magnitude.
pub const MAX_COORD: f64 = 1e9;
/// Most ops per program.
pub const MAX_OPS: usize = 64;
/// Most `Trace` ops per program.
pub const MAX_TRACES: usize = 16;
/// Most points per `Flush` or `Start` op.
pub const MAX_POINTS: usize = 64;
/// `NL_NOW`: the weight of a non-rational control point.
pub const NOW: f64 = -f64::MAX;

/// A clamped B-spline surface: `SmBSplineSurface(gw_SURFACE*, FALSE, ctx)`.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceData {
    /// u degree.
    pub p: usize,
    /// v degree.
    pub q: usize,
    /// Control net, row `i` (u index) of `x y z w` points.
    pub net: Vec<Vec<[f64; 4]>>,
    /// u knots.
    pub u: Vec<f64>,
    /// v knots.
    pub v: Vec<f64>,
}

/// The traced surface.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceIn {
    /// A B-spline surface.
    BSpline(SurfaceData),
    /// `SmPlane(o, x, y, SmVector2d(scale), SmExtent2d(domain), ctx)`.
    Plane {
        /// Origin.
        o: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// u and v scales.
        scale: [f64; 2],
        /// `umin vmin umax vmax`.
        domain: [f64; 4],
    },
}

/// A clamped 3D B-spline curve: `SmBSplineCurve(3, n, m, p, Pw, U)`.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveData {
    /// Degree.
    pub p: usize,
    /// Control points as stored (`x y z w`, `w` = [`NOW`] when non-rational).
    pub cpts: Vec<[f64; 4]>,
    /// Knots (`n + p + 2` of them).
    pub knots: Vec<f64>,
}

/// A curve operand.
#[derive(Debug, Clone, PartialEq)]
pub enum CurveIn {
    /// `SmLine::CreateLineSegment(ctx, 3, a, b, l)`.
    Segment([f64; 3], [f64; 3]),
    /// A B-spline curve.
    BSpline(CurveData),
}

/// A trace point given in full (`SmTracePnt` fields; the rest are zero).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PointIn {
    /// `m_ePointType` (0..=4).
    pub kind: u8,
    /// `m_dCurveParameter`.
    pub param: f64,
    /// `m_vUVCurvePV[0]`.
    pub uv: [f64; 2],
    /// `m_vUVCurvePV[1]`.
    pub uv_tan: [f64; 2],
    /// `m_v3DCurvePV[0]`.
    pub p: [f64; 3],
    /// `m_v3DCurvePV[1]`.
    pub t: [f64; 3],
    /// `m_vSurfacePV[0][0]`.
    pub srf: [f64; 3],
    /// `m_vSurfacePV[2][2]`.
    pub nrm: [f64; 3],
    /// `m_dSurfDropDist`.
    pub drop: f64,
    /// `m_dSurfNormLineDist`.
    pub dev: f64,
}

/// A candidate start point (`m_ePointType`, `m_vUVCurvePV[0]`, `m_v3DCurvePV[1]`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StartIn {
    /// `m_ePointType` (0..=4).
    pub kind: u8,
    /// UV position.
    pub uv: [f64; 2],
    /// 3D tangent.
    pub t: [f64; 3],
}

/// One operation (see the module table).
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// `ComputeStepSize` from the old step.
    Step(f64),
    /// One interior trace from a node middle.
    Trace([f64; 2]),
    /// `FlushCurve` of the given points.
    Flush {
        /// The points, in curve order.
        points: Vec<PointIn>,
        /// `pOptCurve`.
        curve: Option<CurveIn>,
    },
    /// Appends a curve to the accumulated 3D curves.
    AddCurve(CurveIn),
    /// `IsPointOnCurve`.
    OnCurve([f64; 3]),
    /// `TestIsoCurve`.
    Iso {
        /// `SM_SP_V` (else `SM_SP_U`).
        v_dir: bool,
        /// Constant parameter.
        value: f64,
    },
    /// `AddStartPoint` of each point.
    Start(Vec<StartIn>),
    /// `TestPoint`.
    Test(PointIn),
    /// `DoTrace`.
    DoTrace,
}

/// A whole case.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// The surface.
    pub surface: SurfaceIn,
    /// The trace domain `umin vmin umax vmax`.
    pub domain: [f64; 4],
    /// A point of the section plane.
    pub p0: [f64; 3],
    /// The section plane normal.
    pub n: [f64; 3],
    /// `m_dThisApproxTol3d` override.
    pub tol: Option<f64>,
    /// `m_dThisAngTolRad` override.
    pub ang: Option<f64>,
    /// The operations.
    pub ops: Vec<Op>,
}

fn bad(msg: &str) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

fn finite(x: f64) -> Result<f64, FfiError> {
    if x.is_finite() && x.abs() <= MAX_COORD {
        Ok(x)
    } else {
        Err(bad("non-finite or huge value"))
    }
}

/// Checks a clamped knot vector for `n1` control points of degree `p`.
fn check_knots(knots: &[f64], n1: usize, p: usize) -> Result<(), FfiError> {
    if knots.len() != n1 + p + 1 {
        return Err(bad("knot count"));
    }
    if knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad("knots decreasing"));
    }
    let (Some(&k0), Some(&km)) = (knots.first(), knots.last()) else {
        return Err(bad("knot count"));
    };
    if km <= k0
        || knots.iter().take(p + 1).any(|&k| k != k0)
        || knots.iter().rev().take(p + 1).any(|&k| k != km)
    {
        return Err(bad("knots not clamped"));
    }
    let mut run = 0usize;
    let mut last = f64::NAN;
    for &k in knots.iter().filter(|&&k| k != k0 && k != km) {
        run = if k == last { run + 1 } else { 1 };
        last = k;
        if run > p {
            return Err(bad("interior knot multiplicity"));
        }
    }
    Ok(())
}

/// Checks the weights: all [`NOW`] or all in `(0, MAX_COORD]`.
fn check_weights<'a>(mut ws: impl Iterator<Item = &'a [f64; 4]>) -> Result<(), FfiError> {
    let Some(first) = ws.next() else {
        return Err(bad("no control points"));
    };
    let rational = first[3] != NOW;
    let ok = |w: f64| {
        if rational {
            w > 0.0 && w <= MAX_COORD
        } else {
            w == NOW
        }
    };
    if !ok(first[3]) || ws.any(|c| !ok(c[3])) {
        return Err(bad("weights"));
    }
    Ok(())
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

struct Enc(Vec<f64>);

impl Enc {
    fn num(&mut self, x: f64) -> Result<(), FfiError> {
        self.0.push(finite(x)?);
        Ok(())
    }
    fn int(&mut self, x: usize) {
        self.0.push(x as f64);
    }
    fn flag(&mut self, b: bool) {
        self.0.push(if b { 1.0 } else { 0.0 });
    }
    fn nums(&mut self, v: &[f64]) -> Result<(), FfiError> {
        v.iter().try_for_each(|&x| self.num(x))
    }
    fn cpt(&mut self, c: &[f64; 4]) -> Result<(), FfiError> {
        self.nums(&c[..3])?;
        // the weight was checked (NL_NOW or positive)
        self.0.push(c[3]);
        Ok(())
    }

    /// The surface block; returns its natural domain.
    fn surface(&mut self, s: &SurfaceIn) -> Result<[f64; 4], FfiError> {
        match s {
            SurfaceIn::BSpline(d) => {
                let n1 = d.net.len();
                let m1 = d.net.first().map_or(0, Vec::len);
                if !(2..=MAX_INDEX + 1).contains(&n1)
                    || !(2..=MAX_INDEX + 1).contains(&m1)
                    || d.net.iter().any(|r| r.len() != m1)
                {
                    return Err(bad("control net size"));
                }
                if d.p == 0 || d.q == 0 || d.p > MAX_DEGREE || d.q > MAX_DEGREE {
                    return Err(bad("degree"));
                }
                if d.p >= n1 || d.q >= m1 {
                    return Err(bad("degree too high for the net"));
                }
                check_knots(&d.u, n1, d.p)?;
                check_knots(&d.v, m1, d.q)?;
                check_weights(d.net.iter().flatten())?;
                self.0.push(0.0);
                self.int(n1 - 1);
                self.int(m1 - 1);
                self.int(d.p);
                self.int(d.q);
                self.int(d.u.len() - 1);
                self.int(d.v.len() - 1);
                for c in d.net.iter().flatten() {
                    self.cpt(c)?;
                }
                self.nums(&d.u)?;
                self.nums(&d.v)?;
                let (Some(&u0), Some(&u1), Some(&v0), Some(&v1)) =
                    (d.u.first(), d.u.last(), d.v.first(), d.v.last())
                else {
                    return Err(bad("knot count"));
                };
                Ok([u0, v0, u1, v1])
            }
            SurfaceIn::Plane {
                o,
                x,
                y,
                scale,
                domain,
            } => {
                let c = norm(cross(*x, *y));
                let independent =
                    c.partial_cmp(&(1e-6 * norm(*x) * norm(*y))) == Some(Ordering::Greater);
                if !independent || norm(*x) < 1e-9 || norm(*y) < 1e-9 {
                    return Err(bad("plane axes"));
                }
                if !(scale[0] > 1e-9 && scale[1] > 1e-9) {
                    return Err(bad("plane scale"));
                }
                if !(domain[0] < domain[2] && domain[1] < domain[3]) {
                    return Err(bad("plane domain"));
                }
                self.0.push(1.0);
                self.nums(o)?;
                self.nums(x)?;
                self.nums(y)?;
                self.nums(scale)?;
                self.nums(domain)?;
                Ok(*domain)
            }
        }
    }

    fn curve(&mut self, c: &CurveIn) -> Result<(), FfiError> {
        match c {
            CurveIn::Segment(a, b) => {
                if a == b {
                    return Err(bad("degenerate segment"));
                }
                self.0.push(0.0);
                self.nums(a)?;
                self.nums(b)
            }
            CurveIn::BSpline(d) => {
                let n1 = d.cpts.len();
                if !(2..=MAX_CURVE_INDEX + 1).contains(&n1)
                    || d.p == 0
                    || d.p > MAX_DEGREE
                    || d.p >= n1
                {
                    return Err(bad("curve size"));
                }
                check_knots(&d.knots, n1, d.p)?;
                check_weights(d.cpts.iter())?;
                self.0.push(1.0);
                self.int(n1 - 1);
                self.int(d.p);
                self.int(d.knots.len() - 1);
                for c in &d.cpts {
                    self.cpt(c)?;
                }
                self.nums(&d.knots)
            }
        }
    }

    fn point(&mut self, p: &PointIn) -> Result<(), FfiError> {
        if p.kind > 4 {
            return Err(bad("point type"));
        }
        self.int(usize::from(p.kind));
        self.num(p.param)?;
        self.nums(&p.uv)?;
        self.nums(&p.uv_tan)?;
        self.nums(&p.p)?;
        self.nums(&p.t)?;
        self.nums(&p.srf)?;
        self.nums(&p.nrm)?;
        self.num(p.drop)?;
        self.num(p.dev)
    }
}

/// Encodes and validates `prog` (see the module docs for what is accepted).
fn encode(prog: &Program) -> Result<Vec<f64>, FfiError> {
    let mut e = Enc(Vec::new());
    let nat = e.surface(&prog.surface)?;
    let d = prog.domain;
    if !(d[0] < d[2] && d[1] < d[3]) {
        return Err(bad("trace domain extent"));
    }
    if d[0] < nat[0] || d[1] < nat[1] || d[2] > nat[2] || d[3] > nat[3] {
        return Err(bad("trace domain outside the surface domain"));
    }
    e.nums(&d)?;
    e.nums(&prog.p0)?;
    e.nums(&prog.n)?;
    e.flag(prog.tol.is_some());
    e.num(prog.tol.unwrap_or(0.0))?;
    e.flag(prog.ang.is_some());
    e.num(prog.ang.unwrap_or(0.0))?;
    if prog.ops.len() > MAX_OPS {
        return Err(bad("too many ops"));
    }
    e.int(prog.ops.len());
    let mut traces = 0usize;
    let mut do_traces = 0usize;
    for op in &prog.ops {
        match op {
            Op::Step(old) => {
                e.int(1);
                e.num(*old)?;
            }
            Op::Trace(uv) => {
                traces += 1;
                if traces > MAX_TRACES {
                    return Err(bad("too many traces"));
                }
                e.int(2);
                e.nums(uv)?;
            }
            Op::Flush { points, curve } => {
                if traces > 0 || do_traces > 0 {
                    return Err(bad("flush after a trace"));
                }
                if points.len() > MAX_POINTS {
                    return Err(bad("too many points"));
                }
                if points.windows(2).any(|w| match w {
                    [a, b] => a.param.partial_cmp(&b.param) != Some(Ordering::Less),
                    _ => false,
                }) {
                    return Err(bad("flush parameters must increase"));
                }
                e.int(3);
                e.int(points.len());
                for p in points {
                    e.point(p)?;
                }
                e.flag(curve.is_some());
                if let Some(c) = curve {
                    e.curve(c)?;
                }
            }
            Op::AddCurve(c) => {
                e.int(4);
                e.curve(c)?;
            }
            Op::OnCurve(p) => {
                e.int(5);
                e.nums(p)?;
            }
            Op::Iso { v_dir, value } => {
                e.int(6);
                e.flag(*v_dir);
                e.num(*value)?;
            }
            Op::Start(pts) => {
                if pts.len() > MAX_POINTS {
                    return Err(bad("too many points"));
                }
                e.int(7);
                e.int(pts.len());
                for p in pts {
                    if p.kind > 4 {
                        return Err(bad("point type"));
                    }
                    e.int(usize::from(p.kind));
                    e.nums(&p.uv)?;
                    e.nums(&p.t)?;
                }
            }
            Op::Test(p) => {
                e.int(8);
                e.point(p)?;
            }
            Op::DoTrace => {
                do_traces += 1;
                if do_traces > 1 {
                    return Err(bad("more than one DoTrace"));
                }
                e.int(9);
            }
        }
    }
    Ok(e.0)
}

/// Runs `prog` through the shim and returns the record (module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for inputs outside what the module docs accept;
/// [`FfiError::Status`] for a shim failure.
pub fn run(prog: &Program) -> Result<Vec<f64>, FfiError> {
    let input = encode(prog)?;
    let n_in = i64::try_from(input.len()).map_err(|_| bad("input too long"))?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input valid for n_in reads; buf valid for buf.len() writes; len valid.
        unsafe {
            sys::sc_intersect_surface_tracer_run(
                input.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
