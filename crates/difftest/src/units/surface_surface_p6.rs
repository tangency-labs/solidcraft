//! Runner for unit `surface.surface_p6`: `SmSurface` part 6 (the continuities across the
//! knots, the natural UV trimming loop, the derivative-surface sample values, the local
//! curve intersection, the simple coincidence check, the range-solution classification,
//! the surface/surface identical-solution test and `sm_SCLocalSolve`).
//!
//! C++: `solidcore_ffi::units::surface_surface_p6` (upstream `source/SMLib/src/SmSurface.cpp`,
//! called through `SmSurface*` on B-spline surfaces, planes, curve-bounded surfaces and
//! surfaces of revolution). Rust: `solidcore::surface::surface::p6`.
//!
//! A case is one `op` on a `surface` (`{"bspline": {oob, form, net, p, q, u, v}}` with
//! control points as stored `[x, y, z, w]`, rows in u; `{"plane": {o, n}}`; `{"bounded":
//! {basis, implicit}}`; `{"revolution": {a, b, p, axis, swap}}`; or `{"sms": file}`). Both
//! sides produce the flat record documented in the ffi module (status first); the runner
//! reports it as the `out` values. A Rust error maps to its `SmStatus` (`1007` for
//! `InvalidInput`, `-1` for one upstream cannot return); `Unported` defers the case.
//!
//! The p4/p5 back-edges call the real implementations in both Rust and C++.
//! Missing downstream callees (for example `surface_p3` singularity discovery and
//! `surface_p8` global point solves) defer under their own unit identifiers.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve as SmCurve;
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::global_solver::{GlobalSolver, PreferredSolution, SolverOperation};
use solidcore::geom::solution_array::{ObjectKind, Solution, SolutionObject, SolutionType};
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm, SurfParamType};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::curve_bounded_surface::CurveBoundedSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::p2::{DerivSurfDefinition, TangencyType};
use solidcore::surface::surface::p6::{sc_local_solve, SampleOptions, ScLocalSolveNodes};
use solidcore::surface::surface::p7::SsGlobalSolver;
use solidcore::surface::surface::Surface;
use solidcore::SolidError;
use solidcore_ffi::units::surface_surface_p6::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `surface.surface_p6` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// The value the shim writes to an unwritten `rSolPoint`.
const UNWRITTEN: f64 = 12345.0;

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BssIn {
    #[serde(default)]
    oob: bool,
    #[serde(default = "default_form")]
    form: u8,
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

fn default_form() -> u8 {
    BSplineSurfaceForm::Unspecified as u8
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SrfIn {
    Bspline(BssIn),
    Plane {
        o: [f64; 3],
        n: [f64; 3],
    },
    /// An upstream `.sms` file (absolute path after fixture expansion), as a B-spline with
    /// the default form.
    Sms(String),
    /// An `SmCurveBoundedSurface` without boundary curves over `basis`.
    Bounded {
        basis: Box<SrfIn>,
        implicit: bool,
    },
    /// `SmSurfOfRevolution::CreateCanonical` of the line `a`-`b` about the axis through
    /// `p` along `axis`.
    Revolution {
        a: [f64; 3],
        b: [f64; 3],
        p: [f64; 3],
        axis: [f64; 3],
        swap: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CrvIn {
    Bspline {
        dim: u64,
        cpts: Vec<[f64; 4]>,
        p: usize,
        u: Vec<f64>,
    },
    Line {
        a: [f64; 3],
        b: [f64; 3],
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SolIn {
    ty: u8,
    nvars: usize,
    value: f64,
    end_value: f64,
    start: [f64; 5],
    end: [f64; 5],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Cont {
        param: u8,
    },
    Trim {
        dom: [f64; 4],
        side: u8,
        remove: bool,
    },
    Sample {
        n: u64,
        tt: u8,
        /// start/end derivative direction angles, start tangent length, end tangent
        /// scale, start/end rotation angles
        vals: [f64; 6],
        reference: Option<CrvIn>,
        curve: CrvIn,
        ivl: [f64; 2],
        right: bool,
        start: bool,
        want2: bool,
        want3: bool,
    },
    Lci {
        dom: [f64; 4],
        curve: CrvIn,
        ivl: [f64; 2],
        tol: f64,
        guess_uv: [f64; 2],
        guess_t: f64,
    },
    Scc {
        dom: [f64; 4],
        curve: CrvIn,
        ivl: [f64; 2],
        tol: f64,
        partial: bool,
    },
    Classify {
        curve: CrvIn,
        sol: SolIn,
    },
    Ssid {
        other: SrfIn,
        tol: f64,
        sol1: SolIn,
        sol2: SolIn,
    },
    Scls {
        dom: [f64; 4],
        uc: [u8; 2],
        vc: [u8; 2],
        sbox: [f64; 6],
        cbox: Option<[f64; 6]>,
        curve: CrvIn,
        ivl: [f64; 2],
        tc: [u8; 2],
        tol: f64,
        vec: Option<[f64; 3]>,
        sop: u8,
        skip: bool,
        process: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    surface: SrfIn,
    op: OpIn,
}

fn numbers(path: &str) -> Result<Vec<f64>, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    text.split_whitespace()
        .map(|t| t.parse::<f64>().map_err(|e| harness(format!("{path}: {e}"))))
        .collect()
}

fn int(path: &str, v: f64) -> Result<usize, HarnessError> {
    if v.fract() != 0.0 || v < 0.0 {
        return Err(harness(format!("{path}: bad count {v}")));
    }
    Ok(v as usize)
}

/// Reads an upstream `.sms` text file (as unit `surface.surface_p2`).
fn read_sms(path: &str) -> Result<BssIn, HarnessError> {
    let v = numbers(path)?;
    let mut it = v.into_iter();
    let mut num = || it.next().ok_or_else(|| harness(format!("{path}: truncated")));
    let n = int(path, num()?)?;
    let m = int(path, num()?)?;
    let p = int(path, num()?)?;
    let q = int(path, num()?)?;
    let rat = num()? != 0.0;
    let mut net = Vec::new();
    for _ in 0..=n {
        let mut row = Vec::new();
        for _ in 0..=m {
            let (x, y, z) = (num()?, num()?, num()?);
            let w = if rat { num()? } else { NOW };
            row.push([x, y, z, w]);
        }
        net.push(row);
    }
    let u = (0..n + p + 2).map(|_| num()).collect::<Result<Vec<_>, _>>()?;
    let v = (0..m + q + 2).map(|_| num()).collect::<Result<Vec<_>, _>>()?;
    Ok(BssIn {
        oob: false,
        form: default_form(),
        net,
        p,
        q,
        u,
        v,
    })
}

fn resolve(s: &SrfIn) -> Result<SrfIn, HarnessError> {
    match s {
        SrfIn::Sms(path) => Ok(SrfIn::Bspline(read_sms(path)?)),
        SrfIn::Bounded { basis, implicit } => Ok(SrfIn::Bounded {
            basis: Box::new(resolve(basis)?),
            implicit: *implicit,
        }),
        _ => Ok(s.clone()),
    }
}

// ---- C++ side -----------------------------------------------------------------------

fn b(x: bool) -> f64 {
    f64::from(u8::from(x))
}

fn shape(s: &BssIn) -> Result<(usize, usize), HarnessError> {
    let n = s.net.len().checked_sub(1).ok_or_else(|| harness("empty net"))?;
    let cols = s.net.first().map_or(0, Vec::len);
    let m = cols.checked_sub(1).ok_or_else(|| harness("empty net row"))?;
    if s.net.iter().any(|r| r.len() != cols) {
        return Err(harness("net rows differ in length"));
    }
    Ok((n, m))
}

fn push_srf(o: &mut Vec<f64>, s: &SrfIn) -> Result<(), HarnessError> {
    match &resolve(s)? {
        SrfIn::Sms(_) => return Err(harness("unresolved surface file")),
        SrfIn::Revolution {
            a,
            b: bb,
            p,
            axis,
            swap,
        } => {
            o.push(3.0);
            o.extend_from_slice(a);
            o.extend_from_slice(bb);
            o.extend_from_slice(p);
            o.extend_from_slice(axis);
            o.push(b(*swap));
        }
        SrfIn::Bounded { basis, implicit } => {
            o.push(2.0);
            push_srf(o, basis)?;
            o.push(b(*implicit));
        }
        SrfIn::Plane { o: org, n } => {
            o.push(1.0);
            o.extend_from_slice(org);
            o.extend_from_slice(n);
        }
        SrfIn::Bspline(bs) => {
            let (n, m) = shape(bs)?;
            o.extend([0.0, b(bs.oob), f64::from(bs.form)]);
            o.extend(
                [n, m, bs.p, bs.q, bs.u.len().saturating_sub(1), bs.v.len().saturating_sub(1)]
                    .map(|x| x as f64),
            );
            for row in &bs.net {
                for p in row {
                    o.extend_from_slice(p);
                }
            }
            o.extend_from_slice(&bs.u);
            o.extend_from_slice(&bs.v);
        }
    }
    Ok(())
}

fn push_crv(o: &mut Vec<f64>, c: &CrvIn) {
    match c {
        CrvIn::Line { a, b } => {
            o.push(1.0);
            o.extend_from_slice(a);
            o.extend_from_slice(b);
        }
        CrvIn::Bspline { dim, cpts, p, u } => {
            let n = cpts.len().saturating_sub(1);
            let m = u.len().saturating_sub(1);
            o.extend([0.0, *dim as f64, n as f64, *p as f64, m as f64]);
            for x in cpts {
                o.extend_from_slice(x);
            }
            o.extend_from_slice(u);
        }
    }
}

fn push_sol(o: &mut Vec<f64>, s: &SolIn) {
    o.extend([f64::from(s.ty), s.nvars as f64, s.value, s.end_value]);
    o.extend_from_slice(&s.start);
    o.extend_from_slice(&s.end);
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    push_srf(&mut v, &i.surface)?;
    let op = match &i.op {
        OpIn::Cont { param } => {
            v.push(f64::from(*param));
            Op::Cont
        }
        OpIn::Trim { dom, side, remove } => {
            v.extend(dom);
            v.extend([f64::from(*side), b(*remove)]);
            Op::Trim
        }
        OpIn::Sample {
            n,
            tt,
            vals,
            reference,
            curve,
            ivl,
            right,
            start,
            want2,
            want3,
        } => {
            v.extend([*n as f64, f64::from(*tt)]);
            v.extend(vals);
            v.push(b(reference.is_some()));
            if let Some(r) = reference {
                push_crv(&mut v, r);
            }
            push_crv(&mut v, curve);
            v.extend(ivl);
            v.extend([b(*right), b(*start), b(*want2), b(*want3)]);
            Op::Sample
        }
        OpIn::Lci {
            dom,
            curve,
            ivl,
            tol,
            guess_uv,
            guess_t,
        } => {
            v.extend(dom);
            push_crv(&mut v, curve);
            v.extend(ivl);
            v.push(*tol);
            v.extend(guess_uv);
            v.push(*guess_t);
            Op::Lci
        }
        OpIn::Scc {
            dom,
            curve,
            ivl,
            tol,
            partial,
        } => {
            v.extend(dom);
            push_crv(&mut v, curve);
            v.extend(ivl);
            v.extend([*tol, b(*partial)]);
            Op::Scc
        }
        OpIn::Classify { curve, sol } => {
            push_crv(&mut v, curve);
            push_sol(&mut v, sol);
            Op::Classify
        }
        OpIn::Ssid {
            other,
            tol,
            sol1,
            sol2,
        } => {
            push_srf(&mut v, other)?;
            v.push(*tol);
            push_sol(&mut v, sol1);
            push_sol(&mut v, sol2);
            Op::Ssid
        }
        OpIn::Scls {
            dom,
            uc,
            vc,
            sbox,
            cbox,
            curve,
            ivl,
            tc,
            tol,
            vec,
            sop,
            skip,
            process,
        } => {
            v.extend(dom);
            v.extend(uc.map(f64::from));
            v.extend(vc.map(f64::from));
            v.extend(sbox);
            v.push(b(cbox.is_some()));
            v.extend(cbox.unwrap_or([0.0; 6]));
            push_crv(&mut v, curve);
            v.extend(ivl);
            v.extend(tc.map(f64::from));
            v.push(*tol);
            v.push(b(vec.is_some()));
            v.extend(vec.unwrap_or([0.0; 3]));
            v.extend([f64::from(*sop), b(*skip), b(*process)]);
            Op::Scls
        }
    };
    Ok((op, v))
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, input) = cpp_input(i)?;
    let out = ffi::eval(op, &input).map_err(|e| harness(format!("c++: {e}")))?;
    Ok(Output::new().values("out", out))
}

// ---- Rust side ----------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

fn p2(p: [f64; 2]) -> Point2 {
    Point2::new(p[0], p[1])
}

fn dom_of(d: [f64; 4]) -> Extent2d {
    Extent2d::from_points(Point2::new(d[0], d[1]), Point2::new(d[2], d[3]))
}

fn ivl_of(d: [f64; 2]) -> Extent1d {
    Extent1d::from_min_max(d[0], d[1])
}

fn box_of(d: [f64; 6]) -> Extent3d {
    Extent3d::from_points(Vec3::new(d[0], d[1], d[2]), Vec3::new(d[3], d[4], d[5]))
}

/// The surface, `None` when its construction fails (the shim's `-3`).
fn surface_of(k: &Kernel, s: &SrfIn) -> Result<Option<Surface>, HarnessError> {
    Ok(match &resolve(s)? {
        SrfIn::Sms(_) => return Err(harness("unresolved surface file")),
        SrfIn::Revolution {
            a,
            b,
            p,
            axis,
            swap,
        } => {
            let mut gen = Line::from_points(v3(*a), v3(*b), 3);
            gen.make_nurb().map_err(|e| harness(format!("line nurb: {e}")))?;
            SurfOfRevolution::create_canonical(k, SmCurve::from(gen), v3(*p), v3(*axis), *swap)
                .ok()
                .map(Surface::from)
        }
        SrfIn::Bounded { basis, implicit } => surface_of(k, basis)?.map(|basis| {
            Surface::from(CurveBoundedSurface::new(basis, Vec::new(), Vec::new(), *implicit))
        }),
        SrfIn::Plane { o, n } => Some(Surface::from(Plane::from_normal(v3(*o), v3(*n)))),
        SrfIn::Bspline(bs) => {
            let net = bs
                .net
                .iter()
                .map(|r| r.iter().map(|&[x, y, z, w]| CPoint::new(x, y, z, w)).collect())
                .collect();
            let nl = NlSurface::new(net, bs.p, bs.q, bs.u.clone(), bs.v.clone())
                .map_err(|e| harness(format!("surface input: {e}")))?;
            let mut s = BSplineSurface::from_nurb(nl, false);
            *s.out_of_bounds_enabled_mut() = bs.oob;
            let form = BSplineSurfaceForm::from_code(u64::from(bs.form))
                .ok_or_else(|| harness(format!("bad form {}", bs.form)))?;
            s.set_b_spline_surface_form(form);
            Some(Surface::from(s))
        }
    })
}

/// The curve, `None` when its construction fails (the shim's `-3`).
fn curve_of(c: &CrvIn) -> Result<Option<SmCurve>, HarnessError> {
    Ok(match c {
        CrvIn::Line { a, b } => Line::create_line_segment(3, v3(*a), v3(*b), None)
            .ok()
            .map(SmCurve::from),
        CrvIn::Bspline { dim, cpts, p, u } => {
            let pw = cpts.iter().map(|&[x, y, z, w]| CPoint::new(x, y, z, w)).collect();
            let cur =
                Curve::new(pw, *p, u.clone()).map_err(|e| harness(format!("curve input: {e}")))?;
            Some(SmCurve::from(BSplineCurve::from_nurb(cur, *dim, false)))
        }
    })
}

fn cont_of(v: u8) -> Result<ContinuityType, HarnessError> {
    use ContinuityType as C;
    Ok(match v {
        0 => C::Undefined,
        1 => C::Discontinuous,
        2 => C::C0,
        3 => C::G1,
        4 => C::G1R,
        5 => C::G1G2,
        6 => C::G1G2G3,
        7 => C::C1,
        8 => C::C1G2,
        9 => C::C1G2G3,
        10 => C::C1C2,
        11 => C::C1C2G3,
        12 => C::C1C2C3,
        13 => C::CInfinity,
        _ => return Err(harness(format!("bad continuity {v}"))),
    })
}

fn param_of(v: u8) -> Result<SurfParamType, HarnessError> {
    SurfParamType::from_code(u64::from(v)).ok_or_else(|| harness(format!("bad param {v}")))
}

fn sop_of(v: u8) -> Result<SolverOperation, HarnessError> {
    SolverOperation::from_code(i32::from(v)).ok_or_else(|| harness(format!("bad solver op {v}")))
}

fn orient_code(o: OrientType) -> f64 {
    match o {
        OrientType::Unknown => 0.0,
        OrientType::Same => 1.0,
        OrientType::Opposite => 2.0,
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

/// A Rust failure: deferred for `Unported`, else a harness error.
enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Fail::Harness(e)
    }
}

/// `Ok(v)` or the status; `Unported` defers the case.
fn st<T>(r: solidcore::Result<T>) -> Result<Result<T, f64>, Fail> {
    match r {
        Ok(v) => Ok(Ok(v)),
        Err(e) => match Output::unported(&e) {
            Some(o) => Err(Fail::Deferred(Box::new(o))),
            None => Ok(Err(status(&e))),
        },
    }
}

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

/// The shim's curve record.
fn put_curve(o: &mut Vec<f64>, c: &SmCurve) -> Result<(), Fail> {
    let ivl = c.natural_interval();
    o.extend([ivl.min(), ivl.max()]);
    for i in 0..=4 {
        match st(c.evaluate_point(ivl.evaluate(f64::from(i) / 4.0)))? {
            Ok(p) => pv(o, p),
            Err(_) => o.extend([f64::NAN; 3]),
        }
    }
    Ok(())
}

/// A solution object: the op's curve (with its natural interval) or surface `i` (with
/// its natural domain).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Obj {
    Curve([f64; 2]),
    Surface(u8, [f64; 4]),
}

impl SolutionObject for Obj {
    fn kind(&self) -> ObjectKind {
        match self {
            Obj::Curve(_) => ObjectKind::Curve,
            Obj::Surface(..) => ObjectKind::Surface,
        }
    }

    fn natural_interval(&self) -> Option<Extent1d> {
        match self {
            Obj::Curve(i) => Some(ivl_of(*i)),
            Obj::Surface(..) => None,
        }
    }

    fn natural_uv_domain(&self) -> Option<Extent2d> {
        match self {
            Obj::Surface(_, d) => Some(dom_of(*d)),
            Obj::Curve(_) => None,
        }
    }
}

fn surface_obj(i: u8, s: &Surface) -> Obj {
    let d = s.natural_uv_domain();
    Obj::Surface(i, [d.min().x, d.min().y, d.max().x, d.max().y])
}

fn curve_obj(c: &SmCurve) -> Obj {
    let i = c.natural_interval();
    Obj::Curve([i.min(), i.max()])
}

fn solution_of(s: &SolIn, objects: [Obj; 2]) -> Result<Solution<Obj>, HarnessError> {
    let mut sol = Solution::new();
    sol.solution_type = SolutionType::from_code(i32::from(s.ty))
        .ok_or_else(|| harness(format!("bad solution type {}", s.ty)))?;
    sol.num_variables = s.nvars;
    sol.num_objects = 2;
    sol.objects[0] = Some(objects[0]);
    sol.objects[1] = Some(objects[1]);
    sol.start.solution_value = s.value;
    sol.end.solution_value = s.end_value;
    for (i, (a, e)) in s.start.iter().zip(s.end.iter()).enumerate() {
        *sol.start.param_mut(i).map_err(|e| harness(e.to_string()))? = *a;
        *sol.end.param_mut(i).map_err(|e| harness(e.to_string()))? = *e;
    }
    Ok(sol)
}

#[allow(clippy::too_many_lines)]
fn rust_run(i: &Inputs) -> Result<Vec<f64>, Fail> {
    let k = Kernel::default();
    let mut o = Vec::new();
    let Some(s) = surface_of(&k, &i.surface)? else {
        return Ok(vec![-3.0]);
    };
    match &i.op {
        OpIn::Cont { param } => match st(s.calculate_continuities(&k, param_of(*param)?))? {
            Ok((min, conts)) => {
                o.extend([0.0, f64::from(min as i32), conts.len() as f64]);
                o.extend(conts.iter().map(|c| f64::from(*c as i32)));
            }
            Err(c) => o.push(c),
        },
        OpIn::Trim { dom, side, remove } => {
            match st(s.create_natural_uv_trim_curves(&k, &dom_of(*dom), param_of(*side)?, *remove))? {
                Ok(t) => {
                    o.extend([0.0, t.curves_3d.len() as f64]);
                    for (j, c) in t.curves_3d.iter().enumerate() {
                        let orient = t.orients.get(j).copied().unwrap_or_default();
                        o.push(orient_code(orient));
                        o.push(t.max_gaps_3d.get(j).copied().unwrap_or(f64::NAN));
                        put_curve(&mut o, c)?;
                        let uv = t
                            .uv_curves
                            .get(j)
                            .cloned()
                            .ok_or_else(|| harness("missing uv curve"))?;
                        put_curve(&mut o, &SmCurve::from(uv))?;
                    }
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::Sample {
            n,
            tt,
            vals,
            reference,
            curve,
            ivl,
            right,
            start,
            want2,
            want3,
        } => {
            let reference = match reference {
                Some(r) => match curve_of(r)? {
                    Some(c) => Some(c),
                    None => return Ok(vec![-3.0]),
                },
                None => None,
            };
            let Some(bc) = curve_of(curve)? else {
                return Ok(vec![-3.0]);
            };
            let [sdir, edir, slen, escale, sang, eang] = *vals;
            let def = DerivSurfDefinition {
                num_samples: *n,
                tangency_type: TangencyType::from_code(u64::from(*tt))
                    .ok_or_else(|| harness(format!("bad tangency {tt}")))?,
                start_deriv_dir_angle_deg: sdir,
                end_deriv_dir_angle_deg: edir,
                tangent_reference: reference.as_ref(),
                start_tan_length: slen,
                end_tan_scale: escale,
                start_angle_deg: sang,
                end_angle_deg: eang,
                ..DerivSurfDefinition::default()
            };
            let opts = SampleOptions {
                off_to_the_right: *right,
                start_at_boundary: *start,
                want_higher: [*want2, *want3],
            };
            match st(def.sample_values(&k, &s, &bc, &ivl_of(*ivl), opts))? {
                Ok(r) => {
                    o.extend([0.0, r.params.len() as f64]);
                    for (j, t) in r.params.iter().enumerate() {
                        o.push(*t);
                        let get = |v: &[Vec3]| v.get(j).copied().ok_or_else(|| harness("short"));
                        pv(&mut o, get(&r.positions)?);
                        pv(&mut o, get(&r.first_derivs)?);
                        if let Some(d) = &r.second_derivs {
                            pv(&mut o, get(d)?);
                        }
                        if let Some(d) = &r.third_derivs {
                            pv(&mut o, get(d)?);
                        }
                    }
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::Lci {
            dom,
            curve,
            ivl,
            tol,
            guess_uv,
            guess_t,
        } => {
            let Some(c) = curve_of(curve)? else {
                return Ok(vec![-3.0]);
            };
            match st(s.local_curve_intersect(
                &dom_of(*dom),
                &c,
                &ivl_of(*ivl),
                *tol,
                p2(*guess_uv),
                *guess_t,
            ))? {
                Ok(None) => o.extend([0.0, 0.0]),
                Ok(Some(x)) => o.extend([0.0, 1.0, x.uv.x, x.uv.y, x.t, x.deviation]),
                Err(c) => o.push(c),
            }
        }
        OpIn::Scc {
            dom,
            curve,
            ivl,
            tol,
            partial,
        } => {
            let Some(c) = curve_of(curve)? else {
                return Ok(vec![-3.0]);
            };
            let objects = [Some(curve_obj(&c)), Some(surface_obj(0, &s))];
            match st(s.simple_coincidence_checker(
                &k,
                &dom_of(*dom),
                &c,
                &ivl_of(*ivl),
                *tol,
                *partial,
                objects,
            ))? {
                Ok(None) => o.extend([0.0, 0.0]),
                Ok(Some(sol)) => {
                    o.extend([0.0, 1.0, f64::from(sol.solution_type.code())]);
                    o.push(b(sol.objects[0] == objects[0]));
                    o.push(b(sol.objects[1] == objects[1]));
                    o.push(sol.start.solution_value);
                    o.extend(sol.start.parameters.iter().take(3));
                    o.push(sol.end.solution_value);
                    o.extend(sol.end.parameters.iter().take(3));
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::Classify { curve, sol } => {
            let Some(c) = curve_of(curve)? else {
                return Ok(vec![-3.0]);
            };
            let sol = solution_of(sol, [curve_obj(&c), surface_obj(0, &s)])?;
            match st(s.classify_intersecting_range_solution(&k, &c, &sol))? {
                Ok((degenerate, point)) => {
                    o.extend([0.0, b(degenerate)]);
                    match point {
                        Some(p) => {
                            o.push(p.solution_value);
                            o.extend(p.parameters);
                        }
                        None => o.extend([UNWRITTEN; 4]),
                    }
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::Ssid {
            other,
            tol,
            sol1,
            sol2,
        } => {
            let Some(s2) = surface_of(&k, other)? else {
                return Ok(vec![-3.0]);
            };
            let objects = [surface_obj(0, &s), surface_obj(1, &s2)];
            let a = solution_of(sol1, objects)?;
            let bb = solution_of(sol2, objects)?;
            let mut base = GlobalSolver::new();
            base.tolerance_3d = *tol;
            let solver = SsGlobalSolver {
                base,
                surface1: Some(&s),
                surface2: Some(&s2),
                uv_domain1: s.natural_uv_domain(),
                uv_domain2: s2.natural_uv_domain(),
                kernel: &k,
            };
            match st(solver.solutions_identical(&a, &bb))? {
                Ok((t, pref)) => {
                    let pref = match pref {
                        None => 0.0,
                        Some(PreferredSolution::First) => 1.0,
                        Some(PreferredSolution::Second) => 2.0,
                    };
                    o.extend([0.0, f64::from(t as i32), pref]);
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::Scls {
            dom,
            uc,
            vc,
            sbox,
            cbox,
            curve,
            ivl,
            tc,
            tol,
            vec,
            sop,
            skip,
            process,
        } => {
            let Some(c) = curve_of(curve)? else {
                return Ok(vec![-3.0]);
            };
            let nodes = ScLocalSolveNodes {
                eval_surface: &s,
                patch_surface: &s,
                surface_box: box_of(*sbox),
                curve_box: cbox.map(box_of),
            };
            let vecs: Vec<Vec3> = vec.iter().map(|v| v3(*v)).collect();
            let r = sc_local_solve::<Obj>(&k,
                &nodes,
                &dom_of(*dom),
                [cont_of(uc[0])?, cont_of(uc[1])?],
                [cont_of(vc[0])?, cont_of(vc[1])?],
                &c,
                &ivl_of(*ivl),
                [cont_of(tc[0])?, cont_of(tc[1])?],
                *tol,
                &vecs,
                sop_of(*sop)?,
                *skip,
                *process,
            );
            match st(r)? {
                Ok(sols) => {
                    o.extend([0.0, sols.len() as f64]);
                    for sol in sols.as_slice() {
                        o.extend([
                            f64::from(sol.solution_type.code()),
                            sol.num_variables as f64,
                            sol.start.solution_value,
                        ]);
                        o.extend(sol.start.parameters.iter().take(3));
                    }
                }
                Err(c) => o.push(c),
            }
        }
    }
    Ok(o)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    match rust_run(i) {
        Ok(v) => Ok(Output::new().values("out", v)),
        Err(Fail::Deferred(o)) => Ok(*o),
        Err(Fail::Harness(e)) => Err(e),
    }
}

// ---- generator --------------------------------------------------------------------

fn gen_knots(rng: &mut Rng, p: usize, n: usize, kink: bool) -> Vec<f64> {
    let interior = n - p;
    let mut inner: Vec<f64> = (0..interior).map(|_| rng.uniform(0.05, 0.95)).collect();
    inner.sort_by(f64::total_cmp);
    if kink && interior >= p && p >= 1 {
        // a knot of full multiplicity p (a C0 joint)
        let a = inner.first().copied().unwrap_or(0.5);
        for x in inner.iter_mut().take(p) {
            *x = a;
        }
        inner.sort_by(f64::total_cmp);
    } else if rng.chance(0.2) && interior >= 2 && p >= 2 {
        if let Some(&a) = inner.first() {
            if let Some(b) = inner.get_mut(1) {
                *b = a;
            }
        }
        inner.sort_by(f64::total_cmp);
    }
    let (a, b) = if rng.chance(0.5) {
        (0.0, 1.0)
    } else {
        let a = rng.uniform(-2.0, 2.0);
        (a, a + rng.uniform(0.5, 3.0))
    };
    let mut k = vec![a; p + 1];
    k.extend(inner.iter().map(|t| a + t * (b - a)));
    k.extend(vec![b; p + 1]);
    k
}

/// The surface shapes the generator makes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shape {
    Bumpy,
    Flat,
    /// The `u = min` row collapsed to one point (a pole).
    Pole,
    /// Both `u` end rows collapsed (a single span with poles at both ends).
    TwoPoles,
    /// A crease along an interior `u` knot (C0 in u, the iso curves kinked).
    Crease,
    /// A smooth surface with a C0 knot whose control points stay collinear (an iso-curve
    /// kink that the normals show smooth).
    Herringbone,
}

fn scale_of(rng: &mut Rng) -> f64 {
    match rng.below(20) {
        0 => 1e-4,
        1 => 1e4,
        2..=5 => rng.uniform(0.1, 50.0),
        _ => 1.0,
    }
}

fn gen_bss(rng: &mut Rng, shape: Shape) -> BssIn {
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let kink = matches!(shape, Shape::Crease | Shape::Herringbone);
    let n = if shape == Shape::TwoPoles {
        p
    } else if kink {
        2 * p + rng.below(2) as usize
    } else {
        p + rng.below(4) as usize
    };
    let m = q + rng.below(3) as usize;
    let amp = if shape == Shape::Flat {
        *rng.choose(&[0.0, 0.0, 1e-9, 1e-4]).unwrap_or(&0.0)
    } else if shape == Shape::Herringbone {
        0.0
    } else {
        rng.uniform(0.0, 1.0)
    };
    let rational = rng.chance(0.15);
    let scale = scale_of(rng);
    let mut net: Vec<Vec<[f64; 3]>> = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let jit = if shape == Shape::Herringbone { 0.0 } else { 0.2 };
                    let x = i as f64 + rng.uniform(-jit, jit);
                    let y = j as f64 + rng.uniform(-jit, jit);
                    let z = amp * rng.uniform(-1.0, 1.0);
                    [x * scale, y * scale, z * scale]
                })
                .collect()
        })
        .collect();
    match shape {
        Shape::Pole | Shape::TwoPoles => {
            let apex = [-scale, m as f64 * scale / 2.0, rng.uniform(-0.5, 0.5) * scale];
            if let Some(row) = net.first_mut() {
                for c in row.iter_mut() {
                    *c = apex;
                }
            }
            if shape == Shape::TwoPoles {
                let apex2 = [(n as f64 + 1.0) * scale, m as f64 * scale / 2.0, 0.0];
                if let Some(row) = net.last_mut() {
                    for c in row.iter_mut() {
                        *c = apex2;
                    }
                }
            }
        }
        Shape::Crease => {
            // lift the row at the C0 knot: a ridge
            if let Some(row) = net.get_mut(p) {
                for c in row.iter_mut() {
                    c[2] += scale * rng.uniform(0.5, 2.0);
                }
            }
        }
        Shape::Bumpy | Shape::Flat | Shape::Herringbone => {}
    }
    let net = net
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|[x, y, z]| {
                    if rational {
                        let w = rng.uniform(0.5, 2.0);
                        [x * w, y * w, z * w, w]
                    } else {
                        [x, y, z, NOW]
                    }
                })
                .collect()
        })
        .collect();
    BssIn {
        oob: rng.chance(0.05),
        form: if rng.chance(0.9) { 11 } else { rng.below(10) as u8 },
        net,
        p,
        q,
        u: gen_knots(rng, p, n, kink),
        v: gen_knots(rng, q, m, false),
    }
}

fn gen_plane(rng: &mut Rng) -> SrfIn {
    let mut n = rng.point3(-1.0, 1.0);
    if n.iter().map(|c| c * c).sum::<f64>() < 1e-2 {
        n = [0.0, 0.0, 1.0];
    }
    if rng.chance(0.3) {
        n = *rng
            .choose(&[[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]])
            .unwrap_or(&[0.0, 0.0, 1.0]);
    }
    SrfIn::Plane {
        o: rng.point3(-5.0, 5.0),
        n,
    }
}

/// A revolution of a line coplanar with the axis (a cylinder or a cone).
fn gen_revolution(rng: &mut Rng) -> SrfIn {
    let axis = *rng
        .choose(&[[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.5]])
        .unwrap_or(&[0.0, 0.0, 1.0]);
    let p = rng.point3(-2.0, 2.0);
    // a radial direction perpendicular to the axis
    let radial = if axis[0] == 0.0 && axis[1] == 0.0 {
        [1.0, 0.0, 0.0]
    } else if axis[0] == 0.0 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, -1.0, 0.0]
    };
    let r0 = rng.uniform(0.5, 3.0);
    let r1 = if rng.chance(0.5) { r0 } else { rng.uniform(0.5, 3.0) };
    let h0 = rng.uniform(-2.0, 0.0);
    let h1 = rng.uniform(0.5, 3.0);
    let at = |r: f64, h: f64| {
        [
            p[0] + r * radial[0] + h * axis[0],
            p[1] + r * radial[1] + h * axis[1],
            p[2] + r * radial[2] + h * axis[2],
        ]
    };
    SrfIn::Revolution {
        a: at(r0, h0),
        b: at(r1, h1),
        p,
        axis,
        swap: rng.chance(0.3),
    }
}

fn gen_surface(rng: &mut Rng) -> SrfIn {
    match rng.below(20) {
        0..=2 => gen_plane(rng),
        3..=4 => gen_revolution(rng),
        5 => SrfIn::Bounded {
            basis: Box::new(SrfIn::Bspline(gen_bss(rng, Shape::Bumpy))),
            implicit: rng.chance(0.5),
        },
        6..=7 => SrfIn::Bspline(gen_bss(rng, Shape::Pole)),
        8 => SrfIn::Bspline(gen_bss(rng, Shape::TwoPoles)),
        9..=10 => SrfIn::Bspline(gen_bss(rng, Shape::Crease)),
        11..=12 => SrfIn::Bspline(gen_bss(rng, Shape::Herringbone)),
        13 => SrfIn::Bspline(gen_bss(rng, Shape::Flat)),
        _ => SrfIn::Bspline(gen_bss(rng, Shape::Bumpy)),
    }
}

fn gen_bspline_surface(rng: &mut Rng) -> BssIn {
    let shape = *rng
        .choose(&[Shape::Bumpy, Shape::Bumpy, Shape::Flat, Shape::Crease, Shape::Pole])
        .unwrap_or(&Shape::Bumpy);
    gen_bss(rng, shape)
}

/// The natural domain of a generated surface (the unit square when unknown).
fn natural_domain(s: &SrfIn) -> [f64; 4] {
    match s {
        SrfIn::Bspline(b) => {
            let (u0, u1) = (b.u.first().copied().unwrap_or(0.0), b.u.last().copied().unwrap_or(1.0));
            let (v0, v1) = (b.v.first().copied().unwrap_or(0.0), b.v.last().copied().unwrap_or(1.0));
            [u0, v0, u1, v1]
        }
        SrfIn::Bounded { basis, .. } => natural_domain(basis),
        SrfIn::Plane { .. } => [-1.0, -1.0, 1.0, 1.0],
        _ => [0.0, 0.0, 1.0, 1.0],
    }
}

/// A sub-domain of `d`: the whole, a random part, or a thin sliver.
fn gen_subdomain(rng: &mut Rng, d: [f64; 4]) -> [f64; 4] {
    let [u0, v0, u1, v1] = d;
    match rng.below(4) {
        0 => d,
        1 => {
            let a = u0 + (u1 - u0) * rng.uniform(0.0, 0.4);
            let b = u0 + (u1 - u0) * rng.uniform(0.6, 1.0);
            let c = v0 + (v1 - v0) * rng.uniform(0.0, 0.4);
            let e = v0 + (v1 - v0) * rng.uniform(0.6, 1.0);
            [a, c, b, e]
        }
        2 => {
            let a = u0 + (u1 - u0) * rng.uniform(0.0, 0.9);
            [a, v0, a + (u1 - u0) * 1e-7, v1]
        }
        _ => {
            let c = v0 + (v1 - v0) * rng.uniform(0.0, 0.5);
            [u0, c, u1, v1]
        }
    }
}

fn gen_curve(rng: &mut Rng, scale: f64) -> (CrvIn, [f64; 2]) {
    if rng.chance(0.3) {
        let a = rng.point3(-2.0 * scale, 2.0 * scale);
        let mut bb = rng.point3(-2.0 * scale, 2.0 * scale);
        if rng.chance(0.1) {
            bb = [a[0] + scale, a[1], a[2]];
        }
        let len = ((bb[0] - a[0]).powi(2) + (bb[1] - a[1]).powi(2) + (bb[2] - a[2]).powi(2)).sqrt();
        let hi = if rng.chance(0.5) { len } else { len * rng.uniform(0.2, 1.0) };
        return (CrvIn::Line { a, b: bb }, [0.0, hi.max(1e-3)]);
    }
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(4) as usize;
    let degenerate = rng.chance(0.05);
    let base = rng.point3(-scale, scale);
    let cpts = (0..=n)
        .map(|_| {
            let q = if degenerate { base } else { rng.point3(-2.0 * scale, 2.0 * scale) };
            [q[0], q[1], q[2], NOW]
        })
        .collect();
    let u = gen_knots(rng, p, n, false);
    let (a, b) = (u.first().copied().unwrap_or(0.0), u.last().copied().unwrap_or(1.0));
    let ivl = if rng.chance(0.6) {
        [a, b]
    } else {
        let x = a + (b - a) * rng.uniform(0.0, 0.4);
        [x, x + (b - a) * rng.uniform(0.2, 0.6)]
    };
    (
        CrvIn::Bspline {
            dim: 3,
            cpts,
            p,
            u,
        },
        ivl,
    )
}

fn gen_sol(rng: &mut Rng, ty: u8, nvars: usize, near: [f64; 4]) -> SolIn {
    let mut start = [0.0; 5];
    let mut end = [0.0; 5];
    for (i, (s, e)) in start.iter_mut().zip(end.iter_mut()).enumerate() {
        let c = near.get(i % 4).copied().unwrap_or(0.5);
        *s = c + rng.uniform(-0.3, 0.3);
        *e = c + rng.uniform(-0.3, 0.3);
    }
    SolIn {
        ty,
        nvars,
        value: rng.uniform(0.0, 1e-3),
        end_value: rng.uniform(0.0, 1e-3),
        start,
        end,
    }
}

/// A patch boundary continuity, mostly a discontinuity (a boundary curve solve).
fn gen_bdry_cont(rng: &mut Rng) -> u8 {
    *rng.choose(&[0, 1, 1, 1, 2, 2, 3, 10]).unwrap_or(&1)
}

fn gen_cont(rng: &mut Rng) -> u8 {
    *rng.choose(&[1, 2, 3, 7, 10, 13]).unwrap_or(&10)
}

#[allow(clippy::too_many_lines)]
fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(16) {
        0..=3 => Inputs {
            surface: gen_surface(rng),
            op: OpIn::Cont {
                param: rng.below(2) as u8,
            },
        },
        4..=6 => {
            let surface = gen_surface(rng);
            let dom = gen_subdomain(rng, natural_domain(&surface));
            Inputs {
                surface,
                op: OpIn::Trim {
                    dom,
                    side: *rng.choose(&[0, 1, 1, 3, 4]).unwrap_or(&1),
                    remove: rng.chance(0.5),
                },
            }
        }
        7 => {
            let surface = gen_surface(rng);
            let (curve, ivl) = gen_curve(rng, 1.0);
            let reference = rng.chance(0.4).then(|| gen_curve(rng, 1.0).0);
            Inputs {
                surface,
                op: OpIn::Sample {
                    n: *rng.choose(&[0, 1, 4, 10, 64]).unwrap_or(&4),
                    tt: rng.below(9) as u8,
                    vals: [
                        rng.uniform(-400.0, 400.0),
                        rng.uniform(-90.0, 90.0),
                        rng.uniform(-1.0, 2.0),
                        rng.uniform(0.5, 2.0),
                        rng.uniform(-30.0, 30.0),
                        rng.uniform(-30.0, 30.0),
                    ],
                    reference,
                    curve,
                    ivl,
                    right: rng.chance(0.5),
                    start: rng.chance(0.5),
                    want2: rng.chance(0.5),
                    want3: rng.chance(0.3),
                },
            }
        }
        8 => {
            let surface = gen_surface(rng);
            let nd = natural_domain(&surface);
            let dom = gen_subdomain(rng, nd);
            let (curve, ivl) = gen_curve(rng, 1.0);
            Inputs {
                surface,
                op: OpIn::Lci {
                    dom,
                    curve,
                    ivl,
                    tol: *rng.choose(&[1e-6, 1e-3, 0.1]).unwrap_or(&1e-3),
                    guess_uv: [(nd[0] + nd[2]) / 2.0, (nd[1] + nd[3]) / 2.0],
                    guess_t: (ivl[0] + ivl[1]) / 2.0,
                },
            }
        }
        9..=10 => {
            let surface = gen_surface(rng);
            let dom = gen_subdomain(rng, natural_domain(&surface));
            // far curves have disjoint boxes (the comparable path)
            let off = if rng.chance(0.7) { rng.uniform(50.0, 500.0) } else { 0.0 };
            let (mut curve, ivl) = gen_curve(rng, 1.0);
            match &mut curve {
                CrvIn::Line { a, b } => {
                    a[2] += off;
                    b[2] += off;
                }
                CrvIn::Bspline { cpts, .. } => {
                    for c in cpts.iter_mut() {
                        c[2] += off;
                    }
                }
            }
            Inputs {
                surface,
                op: OpIn::Scc {
                    dom,
                    curve,
                    ivl,
                    tol: *rng.choose(&[1e-6, 1e-3, 0.1, 10.0]).unwrap_or(&1e-3),
                    partial: rng.chance(0.5),
                },
            }
        }
        11 => {
            let surface = gen_surface(rng);
            let (curve, ivl) = gen_curve(rng, 1.0);
            let nd = natural_domain(&surface);
            let near = [ivl[0], (nd[0] + nd[2]) / 2.0, (nd[1] + nd[3]) / 2.0, 0.5];
            let ty = if rng.chance(0.7) { 1 } else { 2 };
            Inputs {
                surface,
                op: OpIn::Classify {
                    curve,
                    sol: gen_sol(rng, ty, 3, near),
                },
            }
        }
        12..=13 => {
            let (surface, other) = if rng.chance(0.6) {
                (gen_plane(rng), gen_plane(rng))
            } else {
                (gen_surface(rng), gen_surface(rng))
            };
            let d1 = natural_domain(&surface);
            let d2 = natural_domain(&other);
            let near = [0.0, (d1[0] + d1[2]) / 2.0, (d1[1] + d1[3]) / 2.0, (d2[0] + d2[2]) / 2.0];
            let ty = *rng.choose(&[1, 1, 1, 2]).unwrap_or(&1);
            let sol1 = gen_sol(rng, ty, 4, near);
            let mut sol2 = gen_sol(rng, ty, 4, near);
            match rng.below(4) {
                // identical
                0 => sol2 = sol1.clone(),
                // nearly identical parameters
                1 => {
                    sol2 = sol1.clone();
                    sol2.start[1] += 1e-3;
                }
                // different variable counts
                2 => sol2.nvars = 3,
                _ => {}
            }
            Inputs {
                surface,
                op: OpIn::Ssid {
                    other,
                    tol: *rng.choose(&[0.0, 1e-6, 1e-3, 1.0, 1e3]).unwrap_or(&1e-3),
                    sol1,
                    sol2,
                },
            }
        }
        _ => {
            let bs = gen_bspline_surface(rng);
            let nd = natural_domain(&SrfIn::Bspline(bs.clone()));
            let dom = gen_subdomain(rng, nd);
            let (curve, ivl) = gen_curve(rng, 1.0);
            let r = |rng: &mut Rng| {
                let c = rng.point3(-3.0, 3.0);
                let s = rng.uniform(0.01, 10.0);
                [c[0], c[1], c[2], c[0] + s, c[1] + s, c[2] + s]
            };
            let sop = *rng.choose(&[3, 3, 4, 5, 9]).unwrap_or(&3);
            let cbox = rng.chance(0.5).then(|| r(rng));
            Inputs {
                surface: SrfIn::Bspline(bs),
                op: OpIn::Scls {
                    dom,
                    uc: [gen_bdry_cont(rng), gen_bdry_cont(rng)],
                    vc: [gen_bdry_cont(rng), gen_bdry_cont(rng)],
                    sbox: r(rng),
                    cbox,
                    curve,
                    ivl,
                    tc: if rng.chance(0.7) {
                        [10, 10]
                    } else {
                        [gen_cont(rng), gen_cont(rng)]
                    },
                    tol: *rng.choose(&[1e-6, 1e-3]).unwrap_or(&1e-3),
                    vec: (sop == 9 || rng.chance(0.3)).then_some([0.0, 0.0, 1.0]),
                    sop,
                    // (without skipping, the curve/surface solves defer: surface.surface_p4)
                    skip: rng.chance(0.85),
                    process: rng.chance(0.85),
                },
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surface_p6"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let i = gen_case(rng);
            let Ok((op, input)) = cpp_input(&i) else {
                continue;
            };
            if ffi::validate(op, &input).is_err() {
                continue;
            }
            if let Err(Fail::Harness(_)) = rust_run(&i) {
                continue;
            }
            return serde_json::to_value(i).ok();
        }
        None
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
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
    use std::collections::BTreeMap;

    /// Prints generated case `P6_CASE` as JSON (a debugging aid:
    /// `P6_CASE=6 cargo test -p difftest --features cpp --lib p6_dump_case -- --ignored --nocapture`).
    #[test]
    #[ignore = "debugging aid; needs P6_CASE"]
    fn p6_dump_case() {
        let i: usize = std::env::var("P6_CASE").unwrap().parse().unwrap();
        let mut rng = Rng::new(CaseContext::seed_for("surface.surface_p6", i));
        let v = Runner.generate(&mut rng).unwrap();
        println!("{}", serde_json::to_string(&v).unwrap());
    }

    /// Generated cases reach every op, and the comparable ones mostly compare values
    /// (per op: cases, value records, error statuses, deferred, records past the
    /// status and a count or flag).
    #[test]
    fn generated_cases_are_diverse() {
        let mut stats: BTreeMap<String, [usize; 5]> = BTreeMap::new();
        for i in 0..MIN_GENERATED {
            let mut rng = Rng::new(CaseContext::seed_for("surface.surface_p6", i));
            let Some(v) = Runner.generate(&mut rng) else {
                continue;
            };
            let i: Inputs = serde_json::from_value(v).unwrap();
            let name = serde_json::to_value(&i.op).unwrap()["op"]
                .as_str()
                .unwrap()
                .to_string();
            let e = stats.entry(name).or_default();
            e[0] += 1;
            match rust_run(&i) {
                Ok(o) if o.first() == Some(&0.0) => {
                    e[1] += 1;
                    if o.len() > 3 {
                        e[4] += 1;
                    }
                }
                Ok(_) => e[2] += 1,
                Err(_) => e[3] += 1,
            }
        }
        eprintln!("{stats:?}");
        assert_eq!(stats.len(), 8);
    }
}
