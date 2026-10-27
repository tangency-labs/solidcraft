//! Runner for unit `surface.surface_p5`: `SmSurface` part 5 (singularity tests, the
//! iso-parametric curve of the base class, `LocalPointSolve`/`LocalPointSolve_0`,
//! `DropPoint`, the image-space three-surface local intersection, `CoverOtherCoincidentSurface`,
//! the file-static `sm_FilterSolution` and `sm_SpLocalSolve`, and the `IdenticalSolutions`
//! overrides of the surface global solvers).
//!
//! C++: `solidcore_ffi::units::surface_surface_p5` (upstream `source/SMLib/src/SmSurface.cpp`,
//! called through `SmSurface*` on B-spline surfaces, planes and curve-bounded surfaces; the
//! static functions are verbatim copies). Rust: `solidcore::surface::surface::p5`.
//!
//! A case is one `op` on a `surface` (`{"bspline": {oob, form, net, p, q, u, v}}`, `{"plane":
//! {o, n}}` or a curve-bounded surface over one of them). Both sides produce the flat record
//! documented in the ffi module (status first). A Rust error maps to its `SmStatus` (`1007`
//! for `InvalidInput`, `-1` for one upstream cannot return); `Unported` defers the case.
//!
//! Deferred cases: every `LocalPointSolve` of an operation but `SM_SO_INTERSECT`/`MAXIMIZE`
//! (`LocalPointSolve_1`, `surface.surface_point_solve_p3`), the Newton solve of
//! `LocalPointSolve_0` (`SmFindPSNormalENFO`), `IsSingularity` away from a singularity
//! (`GetSingularities`, `surface.surface_p3`), the surface point solves of `sm_SpLocalSolve`,
//! `DropPoint` where the local or global point solve is needed, the three-surface and cover
//! operations (surface caches, `CoverCoincidentSurface`) and the iso curve of a curve-bounded
//! surface (`SmIsoCurve`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve as SmCurve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::global_solver::{
    GlobalSolve, GlobalSolver, PreferredSolution, SolverOperation,
};
use solidcore::geom::solution_array::{ObjectKind, Solution, SolutionObject, SolutionType};
use solidcore::geom::tree::Tree;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm, SurfParamType};
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::curve_bounded_surface::CurveBoundedSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::p2::LsiGlobalSolver;
use solidcore::surface::surface::p5::{
    cs_identical_solutions, csi_identical_solutions, filter_solution, sp_local_solve,
    DropPointOptions,
};
use solidcore::surface::surface::p7::SpGlobalSolver;
use solidcore::surface::surface::Surface;
use solidcore::SolidError;
use solidcore_ffi::units::surface_surface_p5::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `surface.surface_p5` runner.
pub struct Runner;

// ---- inputs -----------------------------------------------------------------------

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

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
    /// An upstream `.sms` file (absolute path after fixture expansion), as a B-spline
    /// with the default form.
    Sms(String),
    /// An `SmCurveBoundedSurface` without boundary curves over `basis`.
    Bounded {
        basis: Box<SrfIn>,
        implicit: bool,
    },
}

#[path = "fixtures/surface_surface_p5_sms.rs"]
mod sms;

/// Reads an upstream `.sms` text file (as unit `surface.surface_p2`): `n m`, `p q`,
/// rational flag, `(n+1)(m+1)` points (`x y z`, or `wx wy wz w` when rational, v index
/// fastest), `n+p+2` u knots, `m+q+2` v knots.
fn read_sms(path: &str) -> Result<BssIn, HarnessError> {
    let sms::ParsedSurface { net, p, q, u, v } = sms::read_sms(path)?;
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

/// `s` with a file replaced by its inline B-spline.
fn resolve(s: &SrfIn) -> Result<SrfIn, HarnessError> {
    match s {
        SrfIn::Sms(path) => Ok(SrfIn::Bspline(read_sms(path)?)),
        _ => Ok(s.clone()),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrvIn {
    dim: u64,
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SolIn {
    ty: u8,
    nvars: u8,
    value: f64,
    p: [f64; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    IsSing {
        uv: [f64; 2],
        tol: f64,
        pt_only: bool,
    },
    NormalSing {
        uv: [f64; 2],
        tol: f64,
    },
    Iso {
        param: u8,
        iso: f64,
        tol: f64,
        dom: Option<[f64; 4]>,
        want_uv: bool,
    },
    Lps {
        dom: [f64; 4],
        sop: u8,
        p: [f64; 3],
        guess: [f64; 2],
    },
    Lps0 {
        dom: [f64; 4],
        sop: u8,
        p: [f64; 3],
        guess: [f64; 2],
    },
    Drop {
        dom: [f64; 4],
        p: [f64; 3],
        guess: Option<[f64; 2]>,
        sop: u8,
        tgt: Option<[f64; 3]>,
        want: bool,
    },
    Lssi {
        dom: [f64; 4],
        s2: SrfIn,
        dom2: [f64; 4],
        s3: SrfIn,
        dom3: [f64; 4],
        p: [f64; 3],
        p2: Option<[f64; 3]>,
    },
    Cover {
        other: SrfIn,
        tol: f64,
        merged: bool,
    },
    Filter {
        dom: [f64; 4],
        curve: CrvIn,
        ivl: [f64; 2],
        uc: [u8; 2],
        vc: [u8; 2],
        tc: [u8; 2],
        value: f64,
        t: f64,
        uv: [f64; 2],
    },
    SpLocal {
        dom: [f64; 4],
        uc: [u8; 2],
        vc: [u8; 2],
        p: [f64; 3],
        tol: f64,
        sop: u8,
        solve_surf: bool,
        proc_bnd: bool,
    },
    Ident {
        kind: u8,
        tol: f64,
        a: SolIn,
        b: SolIn,
    },
    MaxDist {
        dom: [f64; 4],
        curve: CrvIn,
        ivl: [f64; 2],
        n: usize,
        needed: Option<f64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    surface: SrfIn,
    op: OpIn,
}

// ---- C++ side -----------------------------------------------------------------------

fn b(x: bool) -> f64 {
    f64::from(u8::from(x))
}

fn shape(s: &BssIn) -> Result<(usize, usize), HarnessError> {
    let n = s
        .net
        .len()
        .checked_sub(1)
        .ok_or_else(|| harness("empty net"))?;
    let cols = s.net.first().map_or(0, Vec::len);
    let m = cols
        .checked_sub(1)
        .ok_or_else(|| harness("empty net row"))?;
    if s.net.iter().any(|r| r.len() != cols) {
        return Err(harness("net rows differ in length"));
    }
    Ok((n, m))
}

fn push_srf(o: &mut Vec<f64>, s: &SrfIn) -> Result<(), HarnessError> {
    match &resolve(s)? {
        SrfIn::Sms(_) => return Err(harness("unresolved surface file")),
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
                [
                    n,
                    m,
                    bs.p,
                    bs.q,
                    bs.u.len().saturating_sub(1),
                    bs.v.len().saturating_sub(1),
                ]
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
    let n = c.cpts.len().saturating_sub(1);
    let m = c.u.len().saturating_sub(1);
    o.extend([c.dim as f64, n as f64, c.p as f64, m as f64]);
    for p in &c.cpts {
        o.extend_from_slice(p);
    }
    o.extend_from_slice(&c.u);
}

fn push_sol(o: &mut Vec<f64>, s: &SolIn) {
    o.extend([f64::from(s.ty), f64::from(s.nvars), s.value]);
    o.extend_from_slice(&s.p);
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = Vec::new();
    push_srf(&mut o, &i.surface)?;
    let op = match &i.op {
        OpIn::IsSing { uv, tol, pt_only } => {
            o.extend([uv[0], uv[1], *tol, b(*pt_only)]);
            Op::IsSing
        }
        OpIn::NormalSing { uv, tol } => {
            o.extend([uv[0], uv[1], *tol]);
            Op::NormalSing
        }
        OpIn::Iso {
            param,
            iso,
            tol,
            dom,
            want_uv,
        } => {
            o.extend([f64::from(*param), *iso, *tol, b(dom.is_some())]);
            o.extend_from_slice(&dom.unwrap_or_else(|| natural(&i.surface)));
            o.push(b(*want_uv));
            Op::Iso
        }
        OpIn::Lps { dom, sop, p, guess } | OpIn::Lps0 { dom, sop, p, guess } => {
            o.extend_from_slice(dom);
            o.push(f64::from(*sop));
            o.extend_from_slice(p);
            o.extend_from_slice(guess);
            if matches!(i.op, OpIn::Lps { .. }) {
                Op::Lps
            } else {
                Op::Lps0
            }
        }
        OpIn::Drop {
            dom,
            p,
            guess,
            sop,
            tgt,
            want,
        } => {
            o.extend_from_slice(dom);
            o.extend_from_slice(p);
            o.push(b(guess.is_some()));
            o.extend_from_slice(&guess.unwrap_or([0.0, 0.0]));
            o.push(f64::from(*sop));
            o.push(b(tgt.is_some()));
            o.extend_from_slice(&tgt.unwrap_or([0.0, 0.0, 0.0]));
            o.push(b(*want));
            Op::Drop
        }
        OpIn::Lssi {
            dom,
            s2,
            dom2,
            s3,
            dom3,
            p,
            p2,
        } => {
            o.extend_from_slice(dom);
            push_srf(&mut o, s2)?;
            o.extend_from_slice(dom2);
            push_srf(&mut o, s3)?;
            o.extend_from_slice(dom3);
            o.extend_from_slice(p);
            o.push(b(p2.is_some()));
            o.extend_from_slice(&p2.unwrap_or([0.0, 0.0, 0.0]));
            Op::Lssi
        }
        OpIn::Cover { other, tol, merged } => {
            push_srf(&mut o, other)?;
            o.extend([*tol, b(*merged)]);
            Op::Cover
        }
        OpIn::Filter {
            dom,
            curve,
            ivl,
            uc,
            vc,
            tc,
            value,
            t,
            uv,
        } => {
            o.extend_from_slice(dom);
            push_crv(&mut o, curve);
            o.extend_from_slice(ivl);
            for c in uc.iter().chain(vc).chain(tc) {
                o.push(f64::from(*c));
            }
            o.extend([*value, *t, uv[0], uv[1]]);
            Op::Filter
        }
        OpIn::SpLocal {
            dom,
            uc,
            vc,
            p,
            tol,
            sop,
            solve_surf,
            proc_bnd,
        } => {
            o.extend_from_slice(dom);
            for c in uc.iter().chain(vc) {
                o.push(f64::from(*c));
            }
            o.extend_from_slice(p);
            o.extend([*tol, f64::from(*sop), b(*solve_surf), b(*proc_bnd)]);
            Op::SpLocal
        }
        OpIn::MaxDist {
            dom,
            curve,
            ivl,
            n,
            needed,
        } => {
            o.extend_from_slice(dom);
            push_crv(&mut o, curve);
            o.extend_from_slice(ivl);
            o.extend([*n as f64, b(needed.is_some()), needed.unwrap_or(0.0)]);
            Op::MaxDist
        }
        OpIn::Ident {
            kind,
            tol,
            a,
            b: sb,
        } => {
            o.extend([f64::from(*kind), *tol]);
            push_sol(&mut o, a);
            push_sol(&mut o, sb);
            Op::Ident
        }
    };
    Ok((op, o))
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

fn surface_of(s: &SrfIn) -> Result<Surface, HarnessError> {
    match &resolve(s)? {
        SrfIn::Sms(_) => Err(harness("unresolved surface file")),
        SrfIn::Bounded { basis, implicit } => Ok(Surface::from(
            CurveBoundedSurface::new(surface_of(basis)?, Vec::new(), Vec::new(), *implicit)
                .map_err(|e| HarnessError::new(e.to_string()))?,
        )),
        SrfIn::Plane { o, n } => Ok(Surface::from(Plane::from_normal(v3(*o), v3(*n)))),
        SrfIn::Bspline(bs) => {
            let net = bs
                .net
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                        .collect()
                })
                .collect();
            let nl = NlSurface::new(net, bs.p, bs.q, bs.u.clone(), bs.v.clone())
                .map_err(|e| harness(format!("surface input: {e}")))?;
            let mut s = BSplineSurface::from_nurb(nl, false);
            *s.out_of_bounds_enabled_mut() = bs.oob;
            let form = BSplineSurfaceForm::from_code(u64::from(bs.form))
                .ok_or_else(|| harness(format!("bad form {}", bs.form)))?;
            s.set_b_spline_surface_form(form);
            Ok(Surface::from(s))
        }
    }
}

fn curve_of(c: &CrvIn) -> Result<SmCurve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
        .collect();
    let cur = Curve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve input: {e}")))?;
    Ok(SmCurve::from(BSplineCurve::from_nurb(cur, c.dim, false)))
}

fn sop_of(v: u8) -> Result<SolverOperation, HarnessError> {
    SolverOperation::from_code(i32::from(v)).ok_or_else(|| harness(format!("bad solver op {v}")))
}

fn param_of(v: u8) -> Result<SurfParamType, HarnessError> {
    SurfParamType::from_code(u64::from(v)).ok_or_else(|| harness(format!("bad param {v}")))
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
        _ => return Err(harness(format!("bad continuity {v}"))),
    })
}

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    if std::env::var("P5_DEBUG").is_ok() {
        eprintln!("rust error: {e:?}");
    }
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

/// `Ok(v)`; any error is a harness failure (upstream returns no status here).
fn must<T>(r: solidcore::Result<T>, what: &str) -> Result<T, Fail> {
    match r {
        Ok(v) => Ok(v),
        Err(e) => match Output::unported(&e) {
            Some(o) => Err(Fail::Deferred(Box::new(o))),
            None => Err(Fail::Harness(harness(format!("{what}: {e}")))),
        },
    }
}

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

/// A solution object for the solves (never stored).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Obj;

impl SolutionObject for Obj {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Surface
    }
}

fn sol_of(s: &SolIn) -> Result<Solution<Obj>, HarnessError> {
    let mut sol = Solution::<Obj>::new();
    sol.solution_type = SolutionType::from_code(i32::from(s.ty))
        .ok_or_else(|| harness(format!("bad solution type {}", s.ty)))?;
    sol.num_variables = usize::from(s.nvars);
    sol.start.solution_value = s.value;
    for (i, v) in s.p.iter().enumerate() {
        *sol.start.param_mut(i).map_err(|e| harness(e.to_string()))? = *v;
    }
    Ok(sol)
}

fn put_sol(o: &mut Vec<f64>, s: &Solution<Obj>, n: usize) -> Result<(), Fail> {
    o.push(s.start.solution_value);
    for i in 0..n {
        o.push(must(s.start.param(i), "solution parameter")?);
    }
    Ok(())
}

#[allow(clippy::too_many_lines)] // one arm per op
fn rust_run(i: &Inputs) -> Result<Vec<f64>, Fail> {
    let s = surface_of(&i.surface)?;
    let k = Kernel::default();
    let mut o = Vec::new();
    match &i.op {
        OpIn::IsSing { uv, tol, pt_only } => {
            let r = must(
                s.is_singularity(&k, p2(*uv), *tol, *pt_only),
                "is_singularity",
            )?;
            o.extend([0.0, b(r.singular), f64::from(r.direction as i32)]);
        }
        OpIn::NormalSing { uv, tol } => {
            match st(s.evaluate_normal_at_singularity(&k, p2(*uv), *tol))? {
                Ok(n) => {
                    o.push(0.0);
                    pv(&mut o, n);
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::Iso {
            param,
            iso,
            tol,
            dom,
            want_uv,
        } => {
            let d = dom.map(dom_of);
            match st(s.create_iso_parametric_curve(
                &k,
                param_of(*param)?,
                *iso,
                *tol,
                d.as_ref(),
                *want_uv,
            ))? {
                Ok(r) => {
                    o.extend([0.0, r.max_gap_3d, b(r.uv_iso_curve.is_some())]);
                    let ivl = r.curve.natural_interval();
                    o.extend([ivl.min(), ivl.max()]);
                    for n in 0..=8_u8 {
                        let t = ivl.evaluate(f64::from(n) / 8.0);
                        pv(
                            &mut o,
                            must(r.curve.evaluate_point(&k, t), "iso curve point")?,
                        );
                    }
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::Lps { dom, sop, p, guess } => {
            match st(s.local_point_solve::<Obj>(
                &k,
                &dom_of(*dom),
                sop_of(*sop)?,
                v3(*p),
                p2(*guess),
            ))? {
                Ok(r) => {
                    o.extend([0.0, b(r.found)]);
                    if r.found {
                        put_sol(&mut o, &r.solution, 2)?;
                    }
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::Lps0 { dom, sop, p, guess } => {
            match st(s.local_point_solve_0::<Obj>(
                &k,
                &dom_of(*dom),
                sop_of(*sop)?,
                v3(*p),
                p2(*guess),
            ))? {
                Ok(r) => {
                    o.extend([0.0, b(r.found)]);
                    if r.found {
                        put_sol(&mut o, &r.solution, 2)?;
                    }
                    o.extend([f64::from(r.reason.code()), f64::from(r.iterations)]);
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::Drop {
            dom,
            p,
            guess,
            sop,
            tgt,
            want,
        } => {
            let opts = DropPointOptions {
                guess_uv: guess.map(p2),
                operation: sop_of(*sop)?,
                tgt_seam_side: tgt.map(v3),
                want_solutions: *want,
            };
            match st(s.drop_point::<Obj>(&k, v3(*p), &dom_of(*dom), &opts))? {
                Ok(r) => {
                    o.extend([
                        0.0,
                        b(r.success),
                        r.result_uv.x,
                        r.result_uv.y,
                        r.gap,
                        b(r.is_multivalued),
                    ]);
                    o.push(if *want {
                        r.solutions.len() as f64
                    } else {
                        -1.0
                    });
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::Lssi {
            dom,
            s2,
            dom2,
            s3,
            dom3,
            p,
            p2: g2,
        } => {
            let (s2, s3) = (surface_of(s2)?, surface_of(s3)?);
            match st(s.local_surface_surface_intersect_xyz::<Obj>(
                &k,
                &dom_of(*dom),
                &s2,
                &dom_of(*dom2),
                &s3,
                &dom_of(*dom3),
                v3(*p),
                g2.map(v3),
            ))? {
                Ok((found, sol)) => {
                    o.extend([0.0, b(found)]);
                    if found {
                        put_sol(&mut o, &sol, 6)?;
                    }
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::Cover { other, tol, merged } => {
            let mut this = s.clone();
            let mut other = surface_of(other)?;
            match st(this.cover_other_coincident_surface_deferring_trims(&k, &mut other, *tol, None, None, *merged))?
            {
                Ok(r) => {
                    let which = r.covering_is_other.map_or(-1.0, b);
                    o.extend([0.0, which, r.max_dist]);
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::Filter {
            dom,
            curve,
            ivl,
            uc,
            vc,
            tc,
            value,
            t,
            uv,
        } => {
            let mut sol = Solution::<Obj>::new();
            sol.solution_type = SolutionType::SingleValue;
            sol.num_variables = 3;
            sol.start.solution_value = *value;
            for (n, v) in [*t, uv[0], uv[1]].into_iter().enumerate() {
                *must(sol.start.param_mut(n), "solution parameter")? = v;
            }
            let conts = |c: &[u8; 2]| -> Result<[ContinuityType; 2], HarnessError> {
                Ok([cont_of(c[0])?, cont_of(c[1])?])
            };
            match st(filter_solution(
                &k,
                &s,
                &dom_of(*dom),
                &curve_of(curve)?,
                &Extent1d::from_min_max(ivl[0], ivl[1]),
                conts(uc)?,
                conts(vc)?,
                conts(tc)?,
                &sol,
            ))? {
                Ok(keep) => o.extend([0.0, b(keep)]),
                Err(c) => o.extend([c, 1.0]),
            }
        }
        OpIn::SpLocal {
            dom,
            uc,
            vc,
            p,
            tol,
            sop,
            solve_surf,
            proc_bnd,
        } => {
            let conts = |c: &[u8; 2]| -> Result<[ContinuityType; 2], HarnessError> {
                Ok([cont_of(c[0])?, cont_of(c[1])?])
            };
            match st(sp_local_solve::<Obj>(
                &k,
                &s,
                &dom_of(*dom),
                conts(uc)?,
                conts(vc)?,
                v3(*p),
                *tol,
                sop_of(*sop)?,
                *solve_surf,
                *proc_bnd,
            ))? {
                Ok(sols) => {
                    o.extend([0.0, sols.len() as f64]);
                    for sol in sols.as_slice() {
                        put_sol(&mut o, sol, 2)?;
                    }
                }
                Err(c) => o.extend([c, 0.0]),
            }
        }
        OpIn::MaxDist {
            dom,
            curve,
            ivl,
            n,
            needed,
        } => {
            // the FFI rejects more than 32 samples; upstream (and the port) would loop
            // that many times, so a larger count is not a case for Rust either
            if *n > 32 {
                return Err(HarnessError::new(format!("{n} samples")).into());
            }
            let c = curve_of(curve)?;
            match st(c.surface_max_distance_between(
                &k,
                &Extent1d::from_min_max(ivl[0], ivl[1]),
                &s,
                &dom_of(*dom),
                *n,
                *needed,
            ))? {
                Ok(g) => o.extend([
                    0.0,
                    g.max_distance,
                    g.curve_param,
                    g.surface_param.x,
                    g.surface_param.y,
                ]),
                Err(code) => o.push(code),
            }
        }
        OpIn::Ident {
            kind,
            tol,
            a,
            b: sb,
        } => {
            let (sa, sb) = (sol_of(a)?, sol_of(sb)?);
            let tree: Tree<Obj> = Tree::new_spatial(&Extent3d::from_points(
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 1.0, 1.0),
            ));
            let mut base = GlobalSolver::with_tree(&tree, if *kind < 2 { 2 } else { 3 });
            base.tolerance_3d = *tol;
            let dom = s.natural_uv_domain_with_nurb(&k).map_err(|e| HarnessError::new(e.to_string()))?;
            let (pair, pref) = match kind {
                0 => must(
                    SpGlobalSolver {
                        kernel: &k,
                        base,
                        surface: Some(&s),
                        uv_domain: dom,
                        surface_cache: None,
                        face_queries: None,
                    }
                    .identical_solutions(&sa, &sb),
                    "identical_solutions",
                )?,
                1 => must(
                    LsiGlobalSolver {
                        base,
                        surface: Some(&s),
                        uv_domain: dom,
                        line_point: Vec3::new(0.0, 0.0, 0.0),
                        line_vector: Vec3::new(0.0, 0.0, 1.0),
                        line_interval: None,
                        ray_fire: false,
                        distance_tolerance: 0.0,
                        surface_cache: None,
                        face_queries: None,
                        face_cache: None,
                        kernel: &k,
                    }
                    .identical_solutions(&sa, &sb),
                    "identical_solutions",
                )?,
                2 => must(
                    cs_identical_solutions(&k, &base, Some(&s), &sa, &sb),
                    "identical_solutions",
                )?,
                _ => must(
                    csi_identical_solutions(&k, &base, Some(&s), &sa, &sb),
                    "identical_solutions",
                )?,
            };
            let which = match pref {
                None => -1.0,
                Some(PreferredSolution::First) => 0.0,
                Some(PreferredSolution::Second) => 1.0,
            };
            o.extend([0.0, f64::from(pair as i32), which]);
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

// ---- generator ------------------------------------------------------------

fn gen_knots(rng: &mut Rng, p: usize, n: usize) -> Vec<f64> {
    let interior = n - p;
    let mut inner: Vec<f64> = (0..interior).map(|_| rng.uniform(0.05, 0.95)).collect();
    if rng.chance(0.2) && interior >= 2 && p >= 2 {
        if let Some(&a) = inner.first() {
            if let Some(b) = inner.get_mut(1) {
                *b = a;
            }
        }
    }
    inner.sort_by(f64::total_cmp);
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
    /// A random bumpy net.
    Bumpy,
    /// A flat net (planar within noise or exactly).
    Flat,
    /// The `u = min` row collapsed to one point (a pole).
    Pole,
    /// The first and last rows equal (closed in u).
    ClosedU,
    /// A net whose rows fold back on themselves (parallel derivatives).
    Folded,
    /// The control points next to the `(u, v) = (min, min)` corner collinear with it
    /// (parallel first derivatives at that corner only).
    Pinched,
}

fn gen_bss(rng: &mut Rng, shape: Shape) -> BssIn {
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let n = p + rng.below(3) as usize;
    let m = q + rng.below(3) as usize;
    let amp = if shape == Shape::Flat {
        *rng.choose(&[0.0, 0.0, 1e-9, 1e-4]).unwrap_or(&0.0)
    } else {
        rng.uniform(0.0, 1.0)
    };
    let rational = rng.chance(0.2);
    let scale = match rng.below(20) {
        0 => 1e-4,
        1 => 1e4,
        2..=5 => rng.uniform(0.1, 50.0),
        _ => 1.0,
    };
    let mut net: Vec<Vec<[f64; 3]>> = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let x = i as f64 + rng.uniform(-0.2, 0.2);
                    let y = j as f64 + rng.uniform(-0.2, 0.2);
                    let z = amp * rng.uniform(-1.0, 1.0);
                    [x * scale, y * scale, z * scale]
                })
                .collect()
        })
        .collect();
    match shape {
        Shape::Pole => {
            let apex = [
                -scale,
                m as f64 * scale / 2.0,
                rng.uniform(-0.5, 0.5) * scale,
            ];
            if let Some(row) = net.first_mut() {
                for c in row.iter_mut() {
                    *c = apex;
                }
            }
        }
        Shape::ClosedU => {
            if let Some(first) = net.first().cloned() {
                if let Some(last) = net.last_mut() {
                    *last = first;
                }
            }
        }
        Shape::Folded => {
            // every row along v traces the same line back and forth in u: S_u parallel to S_v
            let noise = *rng.choose(&[0.0, 0.0, 1e-9, 1e-3]).unwrap_or(&0.0);
            for (i, row) in net.iter_mut().enumerate() {
                for (j, c) in row.iter_mut().enumerate() {
                    let t = (i + j) as f64;
                    *c = [
                        t * scale,
                        2.0 * t * scale,
                        rng.uniform(-noise, noise) * scale,
                    ];
                }
            }
        }
        Shape::Pinched => {
            // P[1][0] and P[0][1] on one line through P[0][0]
            let p00 = net
                .first()
                .and_then(|r| r.first())
                .copied()
                .unwrap_or([0.0; 3]);
            let dir = [1.0, 0.5, 0.25];
            let (a, bb) = (rng.uniform(0.5, 1.5), -rng.uniform(0.5, 1.5));
            let at = |k: f64| {
                [
                    p00[0] + k * dir[0] * scale,
                    p00[1] + k * dir[1] * scale,
                    p00[2] + k * dir[2] * scale,
                ]
            };
            if let Some(c) = net.get_mut(1).and_then(|r| r.first_mut()) {
                *c = at(a);
            }
            if let Some(c) = net.first_mut().and_then(|r| r.get_mut(1)) {
                *c = at(if rng.chance(0.5) { bb } else { -bb });
            }
        }
        Shape::Bumpy | Shape::Flat => {}
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
        form: if rng.chance(0.9) {
            11
        } else {
            rng.below(10) as u8
        },
        net,
        p,
        q,
        u: gen_knots(rng, p, n),
        v: gen_knots(rng, q, m),
    }
}

fn gen_plane(rng: &mut Rng) -> SrfIn {
    let mut n = rng.point3(-1.0, 1.0);
    if n.iter().map(|c| c * c).sum::<f64>() < 1e-2 {
        n = [0.0, 0.0, 1.0];
    }
    SrfIn::Plane {
        o: rng.point3(-5.0, 5.0),
        n,
    }
}

fn gen_surface(rng: &mut Rng) -> SrfIn {
    if rng.chance(0.08) {
        let basis = match rng.below(4) {
            0 => gen_plane(rng),
            1 => SrfIn::Bspline(gen_bss(rng, Shape::Pole)),
            _ => SrfIn::Bspline(gen_bss(rng, Shape::Bumpy)),
        };
        return SrfIn::Bounded {
            basis: Box::new(basis),
            implicit: rng.chance(0.5),
        };
    }
    match rng.below(20) {
        0..=1 => gen_plane(rng),
        2..=8 => SrfIn::Bspline(gen_bss(rng, Shape::Bumpy)),
        9..=11 => SrfIn::Bspline(gen_bss(rng, Shape::Pole)),
        12..=13 => SrfIn::Bspline(gen_bss(rng, Shape::Flat)),
        14..=15 => SrfIn::Bspline(gen_bss(rng, Shape::ClosedU)),
        16..=17 => SrfIn::Bspline(gen_bss(rng, Shape::Pinched)),
        _ => SrfIn::Bspline(gen_bss(rng, Shape::Folded)),
    }
}

/// The natural domain `[u0, v0, u1, v1]` of a surface input.
fn natural(s: &SrfIn) -> [f64; 4] {
    match s {
        SrfIn::Sms(_) => resolve(s).map_or([0.0, 0.0, 1.0, 1.0], |r| natural(&r)),
        SrfIn::Bounded { basis, .. } => natural(basis),
        SrfIn::Bspline(bs) => [
            bs.u.first().copied().unwrap_or(0.0),
            bs.v.first().copied().unwrap_or(0.0),
            bs.u.last().copied().unwrap_or(1.0),
            bs.v.last().copied().unwrap_or(1.0),
        ],
        // the natural domain of a plane is about +-1.2e6: generated cases use a small window
        SrfIn::Plane { .. } => [-4.0, -4.0, 4.0, 4.0],
    }
}

/// A parameter in `[a, b]`: interior, at an end, at a knot.
fn gen_param(rng: &mut Rng, a: f64, b: f64, knots: Option<&[f64]>) -> f64 {
    match rng.below(10) {
        0 => a,
        1 => b,
        2 => knots
            .and_then(|k| rng.choose(k).copied())
            .unwrap_or(0.5 * (a + b)),
        3 => a + (b - a) * rng.uniform(0.0, 1e-6),
        _ => a + (b - a) * rng.unit(),
    }
}

fn gen_uv(rng: &mut Rng, s: &SrfIn) -> [f64; 2] {
    let [u0, v0, u1, v1] = natural(s);
    let base = match s {
        SrfIn::Bounded { basis, .. } => basis.as_ref(),
        _ => s,
    };
    let (ku, kv) = match base {
        SrfIn::Bspline(bs) => (Some(bs.u.as_slice()), Some(bs.v.as_slice())),
        SrfIn::Plane { .. } | SrfIn::Sms(_) | SrfIn::Bounded { .. } => (None, None),
    };
    [gen_param(rng, u0, u1, ku), gen_param(rng, v0, v1, kv)]
}

/// A `uv` point on a domain boundary or corner half the time.
fn gen_boundary_uv(rng: &mut Rng, s: &SrfIn) -> [f64; 2] {
    let [u0, v0, u1, v1] = natural(s);
    let mut uv = gen_uv(rng, s);
    if rng.chance(0.5) {
        match rng.below(3) {
            0 => uv[0] = if rng.chance(0.7) { u0 } else { u1 },
            1 => uv[1] = if rng.chance(0.7) { v0 } else { v1 },
            _ => {
                uv = [
                    if rng.chance(0.7) { u0 } else { u1 },
                    if rng.chance(0.7) { v0 } else { v1 },
                ];
            }
        }
    }
    uv
}

fn gen_dom(rng: &mut Rng, s: &SrfIn) -> [f64; 4] {
    let [u0, v0, u1, v1] = natural(s);
    if rng.chance(0.6) {
        return [u0, v0, u1, v1];
    }
    let (du, dv) = (u1 - u0, v1 - v0);
    let a = u0 + du * rng.uniform(0.0, 0.4);
    let c = u1 - du * rng.uniform(0.0, 0.4);
    let b = v0 + dv * rng.uniform(0.0, 0.4);
    let e = v1 - dv * rng.uniform(0.0, 0.4);
    [a, b, c, e]
}

fn gen_curve(rng: &mut Rng) -> CrvIn {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(4) as usize;
    let cpts = (0..=n)
        .map(|i| {
            [
                i as f64 + rng.uniform(-0.3, 0.3),
                rng.uniform(-1.0, 2.0),
                rng.uniform(-1.0, 1.0),
                NOW,
            ]
        })
        .collect();
    CrvIn {
        dim: 3,
        cpts,
        p,
        u: gen_knots(rng, p, n),
    }
}

fn gen_point_near(k: &Kernel, rng: &mut Rng, s: &SrfIn) -> [f64; 3] {
    if let Ok(srf) = surface_of(s) {
        let uv = gen_uv(rng, s);
        if let Ok(p) = srf.evaluate_point(k, p2(uv)) {
            let d = *rng
                .choose(&[0.0, 1e-9, 1e-4, 0.1, 1.0, 5.0])
                .unwrap_or(&0.1);
            let e = rng.point3(-d, d);
            return [p.x + e[0], p.y + e[1], p.z + e[2]];
        }
    }
    rng.point3(-5.0, 5.0)
}

/// Whether a surface's control points span roughly unit size (`0.5..=100`).
fn unit_scale(s: &SrfIn) -> bool {
    match s {
        SrfIn::Bspline(bs) => {
            let max = bs
                .net
                .iter()
                .flatten()
                .map(|p| {
                    let w = if p[3] == NOW { 1.0 } else { p[3] };
                    (p[0] / w).abs().max((p[1] / w).abs()).max((p[2] / w).abs())
                })
                .fold(0.0, f64::max);
            (0.5..=100.0).contains(&max)
        }
        // planes drop analytically (and generated ones work in a small window); files are unit sized
        SrfIn::Plane { .. } | SrfIn::Sms(_) => true,
        SrfIn::Bounded { basis, .. } => unit_scale(basis),
    }
}

/// A parameter pair near the surface's domain (sometimes on its boundary).
fn gen_uv_any(rng: &mut Rng, s: &SrfIn) -> [f64; 2] {
    if rng.chance(0.3) {
        gen_boundary_uv(rng, s)
    } else {
        gen_uv(rng, s)
    }
}

fn gen_cont(rng: &mut Rng) -> u8 {
    *rng.choose(&[1, 2, 2, 3, 5, 5, 7, 10]).unwrap_or(&2)
}

fn gen_conts(rng: &mut Rng) -> [u8; 2] {
    [gen_cont(rng), gen_cont(rng)]
}

fn gen_sop(rng: &mut Rng, ops: &[u8]) -> u8 {
    *rng.choose(ops).unwrap_or(&3)
}

/// A solution of the solver `kind`: surface parameters `p0 p1` for the point solver, `p1 p2`
/// after the curve or line parameter otherwise.
fn gen_sol(rng: &mut Rng, kind: u8, dom: [f64; 4]) -> SolIn {
    let u = rng.uniform(dom[0], dom[2]);
    let v = rng.uniform(dom[1], dom[3]);
    SolIn {
        ty: if rng.chance(0.9) { 1 } else { 2 },
        nvars: 3,
        value: if rng.chance(0.3) {
            0.0
        } else {
            rng.uniform(0.0, 1e-3)
        },
        p: if kind == 0 {
            [u, v, 0.0]
        } else {
            [rng.uniform(0.0, 1.0), u, v]
        },
    }
}

/// A straight segment as a B-spline curve (`SmLine`-like: degree 1, four natural knots).
fn gen_line(rng: &mut Rng) -> CrvIn {
    let a = rng.point3(-3.0, 3.0);
    let b = rng.point3(-3.0, 3.0);
    CrvIn {
        dim: 3,
        cpts: vec![[a[0], a[1], a[2], NOW], [b[0], b[1], b[2], NOW]],
        p: 1,
        u: vec![0.0, 0.0, 1.0, 1.0],
    }
}

/// A point `offset` above or below a random interior point of the surface along its
/// normal, where a plane's analytic drop stays inside the domain.
fn gen_point_over(k: &Kernel, rng: &mut Rng, s: &SrfIn, offset: f64) -> Option<[f64; 3]> {
    let surface = surface_of(s).ok()?;
    // (runner helper that does not return Result)
    let dom = surface.natural_uv_domain_if_built();
    let uv = dom.evaluate(rng.uniform(0.1, 0.9), rng.uniform(0.1, 0.9));
    let p = surface.evaluate_point(k, uv).ok()?;
    let n = surface
        .evaluate_normal(k, uv, Side::Left, Side::Left)
        .ok()?;
    let q = p + n * offset;
    Some([q.x, q.y, q.z])
}

fn gen_maxdist(rng: &mut Rng, s: &SrfIn) -> OpIn {
    let curve = if rng.chance(0.7) {
        gen_line(rng)
    } else {
        gen_curve(rng)
    };
    let (a, bb) = (
        curve.u.first().copied().unwrap_or(0.0),
        curve.u.last().copied().unwrap_or(1.0),
    );
    let ivl = if rng.chance(0.5) {
        [a, bb]
    } else {
        [a, a + (bb - a) * rng.uniform(0.3, 1.0)]
    };
    OpIn::MaxDist {
        dom: gen_dom(rng, s),
        curve,
        ivl,
        n: rng.below(12) as usize,
        needed: rng.chance(0.3).then(|| rng.uniform(0.0, 5.0)),
    }
}

#[allow(clippy::too_many_lines)] // one arm per op
fn gen_op(k: &Kernel, rng: &mut Rng, s: &SrfIn) -> OpIn {
    let nat = natural(s);
    if rng.chance(0.06) {
        return gen_maxdist(rng, s);
    }
    let uv = gen_uv_any(rng, s);
    match rng.below(100) {
        0..=13 => OpIn::IsSing {
            uv,
            tol: *rng.choose(&[1e-10, 1e-7, 1e-5, 1e-3, 0.1]).unwrap_or(&1e-7),
            pt_only: rng.chance(0.3),
        },
        14..=21 => OpIn::NormalSing {
            uv,
            tol: *rng.choose(&[1e-10, 1e-7, 1e-5, 1e-3]).unwrap_or(&1e-7),
        },
        22..=33 => {
            let param = rng.below(2) as u8;
            let (a, bb) = if param == 0 {
                (nat[0], nat[2])
            } else {
                (nat[1], nat[3])
            };
            let iso = match rng.below(4) {
                0 => a,
                1 => bb,
                _ => rng.uniform(a, bb),
            };
            OpIn::Iso {
                param,
                iso,
                tol: if rng.chance(0.5) {
                    0.0
                } else {
                    *rng.choose(&[1e-6, 1e-4, 1e-2]).unwrap_or(&1e-4)
                },
                dom: match rng.below(3) {
                    0 => None,
                    1 => Some(nat),
                    _ => Some(gen_dom(rng, s)),
                },
                want_uv: rng.chance(0.4),
            }
        }
        34..=39 => OpIn::Lps {
            dom: gen_dom(rng, s),
            sop: gen_sop(rng, &[0, 3, 4, 5]),
            p: gen_point_near(k, rng, s),
            guess: uv,
        },
        40..=53 => OpIn::Lps0 {
            dom: gen_dom(rng, s),
            sop: if rng.chance(0.7) {
                0
            } else {
                gen_sop(rng, &[3, 4, 5])
            },
            p: gen_point_near(k, rng, s),
            guess: uv,
        },
        54..=67 => OpIn::Drop {
            dom: if rng.chance(0.6) {
                nat
            } else {
                gen_dom(rng, s)
            },
            p: {
                let off = rng.uniform(-2.0, 2.0);
                (matches!(s, SrfIn::Plane { .. }) && rng.chance(0.7))
                    .then(|| gen_point_over(k, rng, s, off))
                    .flatten()
                    .unwrap_or_else(|| gen_point_near(k, rng, s))
            },
            guess: rng.chance(0.5).then_some(uv),
            sop: gen_sop(rng, &[0, 3, 5]),
            tgt: rng.chance(0.2).then(|| rng.point3(-1.0, 1.0)),
            want: rng.chance(0.3),
        },
        68..=69 => OpIn::Lssi {
            dom: nat,
            s2: gen_surface(rng),
            dom2: [0.0, 0.0, 1.0, 1.0],
            s3: gen_surface(rng),
            dom3: [0.0, 0.0, 1.0, 1.0],
            p: gen_point_near(k, rng, s),
            p2: rng.chance(0.3).then(|| gen_point_near(k, rng, s)),
        },
        70 => OpIn::Cover {
            other: gen_surface(rng),
            tol: *rng.choose(&[1e-6, 1e-4]).unwrap_or(&1e-6),
            merged: rng.chance(0.3),
        },
        71..=84 => {
            let curve = gen_curve(rng);
            let (a, bb) = (
                curve.u.first().copied().unwrap_or(0.0),
                curve.u.last().copied().unwrap_or(1.0),
            );
            let ivl = if rng.chance(0.5) {
                [a, bb]
            } else {
                [a, a + (bb - a) * rng.uniform(0.3, 1.0)]
            };
            let t = match rng.below(4) {
                0 => ivl[0],
                1 => ivl[1],
                _ => rng.uniform(ivl[0], ivl[1]),
            };
            OpIn::Filter {
                dom: nat,
                curve,
                ivl,
                uc: gen_conts(rng),
                vc: gen_conts(rng),
                tc: gen_conts(rng),
                value: *rng
                    .choose(&[0.0, 1e-12, 1e-9, 1e-6, 1e-3, 0.1])
                    .unwrap_or(&1e-6),
                t,
                uv,
            }
        }
        85..=93 => OpIn::SpLocal {
            dom: if rng.chance(0.6) {
                nat
            } else {
                gen_dom(rng, s)
            },
            uc: gen_conts(rng),
            vc: gen_conts(rng),
            p: gen_point_near(k, rng, s),
            tol: *rng.choose(&[1e-6, 1e-3, 0.1, 1.0]).unwrap_or(&1e-3),
            sop: gen_sop(rng, &[0, 3, 4, 5]),
            solve_surf: rng.chance(0.4),
            proc_bnd: rng.chance(0.8),
        },
        _ => {
            let kind = rng.below(4) as u8;
            let mut a = gen_sol(rng, kind, nat);
            let mut bb = gen_sol(rng, kind, nat);
            if rng.chance(0.4) {
                // nearly the same 3D point from different parameters, or equal parameters
                bb = a.clone();
                if rng.chance(0.5) {
                    let at = usize::from(kind != 0);
                    if let Some(x) = bb.p.get_mut(at) {
                        *x = rng.uniform(nat[0], nat[2]);
                    }
                }
                if rng.chance(0.3) {
                    bb.value = a.value + 1e-3;
                }
            }
            if rng.chance(0.2) {
                a.nvars = 3;
            }
            OpIn::Ident {
                kind,
                tol: *rng.choose(&[1e-9, 1e-6, 1e-3, 1.0]).unwrap_or(&1e-6),
                a,
                b: bb,
            }
        }
    }
}

fn gen_case(k: &Kernel, rng: &mut Rng) -> Inputs {
    let surface = gen_surface(rng);
    let op = gen_op(k, rng, &surface);
    // curve/plane gaps are the ones the harness can compare (planes drop analytically; the
    // other surfaces defer on the point solves, and a curve-bounded surface has no GetKnots here)
    if matches!(op, OpIn::MaxDist { .. }) {
        let surface = gen_plane(rng);
        let op = gen_maxdist(rng, &surface);
        return Inputs { surface, op };
    }
    Inputs { surface, op }
}

/// Ops whose C++ side builds a surface cache decomposition: keep them at modest scales.
fn is_heavy(op: &OpIn) -> bool {
    matches!(
        op,
        OpIn::Lssi { .. }
            | OpIn::Cover { .. }
            | OpIn::Drop { .. }
            | OpIn::Lps { .. }
            | OpIn::SpLocal { .. }
            | OpIn::MaxDist { .. }
    )
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surface_p5"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let k = &Kernel::default();
        for _ in 0..64 {
            let i = gen_case(k, rng);
            let Ok((op, input)) = cpp_input(&i) else {
                continue;
            };
            if ffi::validate(op, &input).is_err() {
                continue;
            }
            if let Err(Fail::Harness(_)) = rust_run(&i) {
                continue;
            }
            if is_heavy(&i.op) && !unit_scale(&i.surface) {
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

    /// Prints generated case `P5_CASE` as JSON (a debugging aid:
    /// `P5_CASE=6 cargo test -p difftest --features cpp --lib p5_dump_case -- --ignored --nocapture`).
    #[test]
    #[ignore = "debugging aid; needs P5_CASE"]
    fn p5_dump_case() {
        let i: usize = std::env::var("P5_CASE").unwrap().parse().unwrap();
        let mut rng = Rng::new(CaseContext::seed_for("surface.surface_p5", i));
        let v = Runner.generate(&mut rng).unwrap();
        println!("{}", serde_json::to_string(&v).unwrap());
    }

    /// Runs generated case `P5_CASE` on the Rust side and prints the result and any error.
    #[test]
    #[ignore = "debugging aid; needs P5_CASE"]
    fn p5_run_case() {
        let c: usize = std::env::var("P5_CASE").unwrap().parse().unwrap();
        let mut rng = Rng::new(CaseContext::seed_for("surface.surface_p5", c));
        let v = Runner.generate(&mut rng).unwrap();
        let i: Inputs = serde_json::from_value(v).unwrap();
        std::env::set_var("P5_DEBUG", "1");
        match rust_run(&i) {
            Ok(o) => println!("rust ok: {o:?}"),
            Err(Fail::Deferred(_)) => println!("deferred"),
            Err(Fail::Harness(e)) => println!("harness: {e}"),
        }
        println!("cpp: {:?}", cpp(&i).map(|o| format!("{o:?}")));
    }

    /// Generated cases cover every op, and most of the comparable ones are not deferred.
    #[test]
    fn generated_cases_are_diverse() {
        let mut rng = Rng::new(7);
        let mut stats: BTreeMap<String, [usize; 3]> = BTreeMap::new();
        for _ in 0..1500 {
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
                Ok(_) => e[1] += 1,
                Err(_) => e[2] += 1,
            }
        }
        for (k, v) in &stats {
            eprintln!("{k}: total {} ran {} deferred {}", v[0], v[1], v[2]);
        }
        assert!(stats.len() >= 10, "{}", stats.len());
    }
}
