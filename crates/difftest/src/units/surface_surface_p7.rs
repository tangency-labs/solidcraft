//! Runner for unit `surface.surface_p7`: `SmSurface` part 7 (the unit normal and its
//! fall-backs, closure and planarity, first-derivative helpers, the degenerate-boundary
//! direction search, the global and local solves, flat corners, the parallel projection,
//! the file-static helpers) and the co-ported `Evaluate1stDerivatives`,
//! `Evaluate2ndDerivatives` and `EvaluateDirectionalDerivs`.
//!
//! C++: `solidcore_ffi::units::surface_surface_p7` (upstream `source/SMLib/src/SmSurface.cpp`,
//! called through `SmSurface*` on B-spline surfaces and planes). Rust:
//! `solidcore::surface::surface::p7`.
//!
//! A case is one `op` on a `surface` (`{"bspline": {oob, form, net, p, q, u, v}}` with
//! control points as stored `[x, y, z, w]`, rows in u, or `{"plane": {o, n}}`). Both sides
//! produce the flat record documented in the ffi module (status first); the runner reports
//! it as the `out` values. A Rust error maps to its `SmStatus` (`1007` for
//! `InvalidInput`, `-1` for one upstream cannot return); `Unported` defers the case.
//!
//! Deferred cases: the global solves and the parallel projection (surface cache,
//! `surface.surface_p1`), the local curve solve
//! (`SmCSNormalizeENFO`), the Newton step of `FindDegenParamForDirection`, `GetFlatCorners`
//! (`IsFlatCorner`) and `sm_CheckSeamDrop` on a seam (`EvaluateSTEP`). Planes are compared
//! (`SmPlane::IsClosed` and `EvaluateNormal` overrides). The solvers'
//! `BranchMayContainAnswers` need surface cache trees and are not run here.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve as SmCurve;
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::global_solver::{SolutionRequested, SolverOperation};
use solidcore::geom::solution_array::{
    ObjectKind, SolutionArray, SolutionEnd, SolutionObject, SolutionType,
};
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, LocationType, SurfParamType,
};
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::curve_bounded_surface::CurveBoundedSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::p7::{
    approx_derivative_lengths, base_evaluate_normal, base_is_closed, check_seam_drop,
    compute_higher_order_derivs, compute_inward_vector, drop_points_to_tangent_plane,
    drop_vectors_at, good_drop_result, load_guesses, pass_sc_convergence_test,
    pass_ss_convergence_test,
};
use solidcore::surface::surface::p1::{
    evaluate_1st_derivatives, evaluate_2nd_derivatives, evaluate_directional_derivs,
};
use solidcore::surface::surface::Surface;
use solidcore::SolidError;
use solidcore_ffi::units::surface_surface_p7::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `surface.surface_p7` runner.
pub struct Runner;

pub(crate) fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// `SM_ERR`.
const SM_ERR: f64 = 1001.0;

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BssIn {
    #[serde(default)]
    pub(crate) oob: bool,
    #[serde(default = "default_form")]
    pub(crate) form: u8,
    pub(crate) net: Vec<Vec<[f64; 4]>>,
    pub(crate) p: usize,
    pub(crate) q: usize,
    pub(crate) u: Vec<f64>,
    pub(crate) v: Vec<f64>,
}

pub(crate) fn default_form() -> u8 {
    BSplineSurfaceForm::Unspecified as u8
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SrfIn {
    Bspline(BssIn),
    Plane { o: [f64; 3], n: [f64; 3] },
    /// An upstream `.sms` file (absolute path after fixture expansion), as a B-spline
    /// with the default form.
    Sms(String),
    /// An `SmCurveBoundedSurface` without boundary curves over `basis`.
    Bounded { basis: Box<SrfIn>, implicit: bool },
    /// An `SmSurfOfRevolution` (`CreateCanonical`) of the line `a`-`b` about the axis
    /// through `p` along `axis`, with `SetOutOfBoundsEnabled(oob)` (surface and generator).
    Revolution {
        a: [f64; 3],
        b: [f64; 3],
        p: [f64; 3],
        axis: [f64; 3],
        swap: bool,
        oob: bool,
    },
}

#[path = "fixtures/surface_surface_p7_sms.rs"]
mod sms;

/// Reads an upstream `.sms` text file (as unit `surface.surface_p2`): `n m`, `p q`,
/// rational flag, `(n+1)(m+1)` points (`x y z`, or `wx wy wz w` when rational, v index
/// fastest), `n+p+2` u knots, `m+q+2` v knots.
fn read_sms(path: &str) -> Result<BssIn, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let parsed = sms::parse(path, &text, NOW).map_err(harness)?;
    Ok(BssIn {
        oob: false,
        form: default_form(),
        net: parsed.net,
        p: parsed.p,
        q: parsed.q,
        u: parsed.u,
        v: parsed.v,
    })
}

/// `s` with a file replaced by its inline B-spline.
pub(crate) fn resolve(s: &SrfIn) -> Result<SrfIn, HarnessError> {
    match s {
        SrfIn::Sms(path) => Ok(SrfIn::Bspline(read_sms(path)?)),
        _ => Ok(s.clone()),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CrvIn {
    pub(crate) dim: u64,
    pub(crate) cpts: Vec<[f64; 4]>,
    pub(crate) p: usize,
    pub(crate) u: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Eval1 { uv: [f64; 2], ul: bool, vl: bool },
    Eval2 { uv: [f64; 2], ul: bool, vl: bool },
    DirDeriv { uv: [f64; 2], dir: [f64; 2], h: usize, ul: bool, vl: bool },
    Normal { uv: [f64; 2], ul: bool, vl: bool, base: bool },
    IsClosed { dom: [f64; 4], param: u8, tol: Option<f64>, want: bool, base: bool },
    IsPlanar { tol: f64, base: bool },
    ApproxLen { dom: [f64; 4] },
    Higher { uv: [f64; 2], g1: [f64; 2], want3: bool, want4: bool },
    Inward { uv: [f64; 2] },
    DropPts { uv: [f64; 2], ul: bool, vl: bool, pts: Vec<[f64; 3]> },
    DropVecs { uv: [f64; 2], ul: bool, vl: bool, vecs: Vec<[f64; 3]> },
    Degen { uv: [f64; 2], dir: [f64; 3], from_pole: bool, sdir: u8, tol: f64 },
    Flat { angle: f64, want: bool },
    Brep,
    Gps0 { dom: [f64; 4], sop: u8, p: [f64; 3], tol: f64, target: Option<f64>, req: u8 },
    Gcs {
        dom: [f64; 4],
        curve: CrvIn,
        ivl: [f64; 2],
        sop: u8,
        tol: f64,
        vecs: Vec<[f64; 3]>,
        target: Option<f64>,
        req: u8,
    },
    Gss {
        dom: [f64; 4],
        other: SrfIn,
        dom2: [f64; 4],
        sop: u8,
        tol: f64,
        vecs: Vec<[f64; 3]>,
        target: Option<f64>,
        req: u8,
    },
    Lcs {
        dom: [f64; 4],
        curve: CrvIn,
        ivl: [f64; 2],
        sop: u8,
        tol: f64,
        vecs: Vec<[f64; 3]>,
        guess_uv: [f64; 2],
        guess_t: f64,
    },
    Proj {
        dom: [f64; 4],
        curve: CrvIn,
        #[serde(with = "non_finite3")]
        vec: [f64; 3],
        dist: Option<f64>,
    },
    LoadGuesses { dom: [f64; 4], p: [f64; 3], sop: u8 },
    PassSc { uv: [f64; 2], curve: CrvIn, t: f64 },
    PassSs { uv: [f64; 2], other: SrfIn, uv2: [f64; 2] },
    GoodDrop { dom: [f64; 4], p: [f64; 3], guess: [f64; 2], value: f64, sol: [f64; 2] },
    CheckSeam {
        uv: [f64; 2],
        dom: [f64; 4],
        other: SrfIn,
        uv2: [f64; 2],
        dom2: [f64; 4],
        loc: u8,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    surface: SrfIn,
    op: OpIn,
}

/// A `[f64; 3]` whose entries may be `"NaN"`, `"inf"` or `"-inf"` (JSON has no
/// non-finite numbers).
mod non_finite3 {
    use serde::de::Error;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use serde_json::Value;

    use crate::json::num;

    pub fn serialize<S: Serializer>(v: &[f64; 3], s: S) -> Result<S::Ok, S::Error> {
        v.map(num).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[f64; 3], D::Error> {
        let float = |v: &Value| match v {
            Value::Number(n) => n.as_f64().ok_or_else(|| D::Error::custom(format!("bad number {n}"))),
            Value::String(s) if s == "NaN" => Ok(f64::NAN),
            Value::String(s) if s == "inf" => Ok(f64::INFINITY),
            Value::String(s) if s == "-inf" => Ok(f64::NEG_INFINITY),
            other => Err(D::Error::custom(format!("bad float {other}"))),
        };
        let [a, b, c] = <[Value; 3]>::deserialize(d)?;
        Ok([float(&a)?, float(&b)?, float(&c)?])
    }
}

// ---- C++ side -----------------------------------------------------------------------

pub(crate) fn b(x: bool) -> f64 {
    f64::from(u8::from(x))
}

pub(crate) fn shape(s: &BssIn) -> Result<(usize, usize), HarnessError> {
    let n = s.net.len().checked_sub(1).ok_or_else(|| harness("empty net"))?;
    let cols = s.net.first().map_or(0, Vec::len);
    let m = cols.checked_sub(1).ok_or_else(|| harness("empty net row"))?;
    if s.net.iter().any(|r| r.len() != cols) {
        return Err(harness("net rows differ in length"));
    }
    Ok((n, m))
}

pub(crate) fn push_srf(o: &mut Vec<f64>, s: &SrfIn) -> Result<(), HarnessError> {
    match &resolve(s)? {
        SrfIn::Sms(_) => return Err(harness("unresolved surface file")),
        SrfIn::Bounded { basis, implicit } => {
            o.push(2.0);
            push_srf(o, basis)?;
            o.push(b(*implicit));
        }
        SrfIn::Revolution { a, b: bb, p, axis, swap, oob } => {
            o.push(3.0);
            for v in [a, bb, p, axis] {
                o.extend_from_slice(v);
            }
            o.extend([b(*swap), b(*oob)]);
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

pub(crate) fn push_crv(o: &mut Vec<f64>, c: &CrvIn) {
    let n = c.cpts.len().saturating_sub(1);
    let m = c.u.len().saturating_sub(1);
    o.extend([c.dim as f64, n as f64, c.p as f64, m as f64]);
    for p in &c.cpts {
        o.extend_from_slice(p);
    }
    o.extend_from_slice(&c.u);
}

pub(crate) fn push_vecs(o: &mut Vec<f64>, v: &[[f64; 3]]) {
    o.push(v.len() as f64);
    for x in v {
        o.extend_from_slice(x);
    }
}

pub(crate) fn push_opt(o: &mut Vec<f64>, x: Option<f64>) {
    o.extend([b(x.is_some()), x.unwrap_or(0.0)]);
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    push_srf(&mut v, &i.surface)?;
    let op = match &i.op {
        OpIn::Eval1 { uv, ul, vl } | OpIn::Eval2 { uv, ul, vl } => {
            v.extend(uv);
            v.extend([b(*ul), b(*vl)]);
            if matches!(i.op, OpIn::Eval1 { .. }) {
                Op::Eval1
            } else {
                Op::Eval2
            }
        }
        OpIn::DirDeriv { uv, dir, h, ul, vl } => {
            v.extend(uv);
            v.extend(dir);
            v.extend([*h as f64, b(*ul), b(*vl)]);
            Op::DirDeriv
        }
        OpIn::Normal { uv, ul, vl, base } => {
            v.extend(uv);
            v.extend([b(*ul), b(*vl), b(*base)]);
            Op::Normal
        }
        OpIn::IsClosed {
            dom,
            param,
            tol,
            want,
            base,
        } => {
            v.extend(dom);
            v.push(f64::from(*param));
            v.extend([b(tol.is_some()), tol.unwrap_or(1.0), b(*want), b(*base)]);
            Op::IsClosed
        }
        OpIn::IsPlanar { tol, base } => {
            v.extend([*tol, b(*base)]);
            Op::IsPlanar
        }
        OpIn::ApproxLen { dom } => {
            v.extend(dom);
            Op::ApproxLen
        }
        OpIn::Higher {
            uv,
            g1,
            want3,
            want4,
        } => {
            v.extend(uv);
            v.extend(g1);
            v.extend([b(*want3), b(*want4)]);
            Op::Higher
        }
        OpIn::Inward { uv } => {
            v.extend(uv);
            Op::Inward
        }
        OpIn::DropPts { uv, ul, vl, pts } => {
            v.extend(uv);
            v.extend([b(*ul), b(*vl)]);
            push_vecs(&mut v, pts);
            Op::DropPts
        }
        OpIn::DropVecs { uv, ul, vl, vecs } => {
            v.extend(uv);
            v.extend([b(*ul), b(*vl)]);
            push_vecs(&mut v, vecs);
            Op::DropVecs
        }
        OpIn::Degen {
            uv,
            dir,
            from_pole,
            sdir,
            tol,
        } => {
            v.extend(uv);
            v.extend(dir);
            v.extend([b(*from_pole), f64::from(*sdir), *tol]);
            Op::Degen
        }
        OpIn::Flat { angle, want } => {
            v.extend([*angle, b(*want)]);
            Op::Flat
        }
        OpIn::Brep => Op::Brep,
        OpIn::Gps0 {
            dom,
            sop,
            p,
            tol,
            target,
            req,
        } => {
            v.extend(dom);
            v.push(f64::from(*sop));
            v.extend(p);
            v.push(*tol);
            push_opt(&mut v, *target);
            v.push(f64::from(*req));
            Op::Gps0
        }
        OpIn::Gcs {
            dom,
            curve,
            ivl,
            sop,
            tol,
            vecs,
            target,
            req,
        } => {
            v.extend(dom);
            push_crv(&mut v, curve);
            v.extend(ivl);
            v.extend([f64::from(*sop), *tol]);
            push_vecs(&mut v, vecs);
            push_opt(&mut v, *target);
            v.push(f64::from(*req));
            Op::Gcs
        }
        OpIn::Gss {
            dom,
            other,
            dom2,
            sop,
            tol,
            vecs,
            target,
            req,
        } => {
            v.extend(dom);
            push_srf(&mut v, other)?;
            v.extend(dom2);
            v.extend([f64::from(*sop), *tol]);
            push_vecs(&mut v, vecs);
            push_opt(&mut v, *target);
            v.push(f64::from(*req));
            Op::Gss
        }
        OpIn::Lcs {
            dom,
            curve,
            ivl,
            sop,
            tol,
            vecs,
            guess_uv,
            guess_t,
        } => {
            v.extend(dom);
            push_crv(&mut v, curve);
            v.extend(ivl);
            v.extend([f64::from(*sop), *tol]);
            push_vecs(&mut v, vecs);
            v.extend(guess_uv);
            v.push(*guess_t);
            Op::Lcs
        }
        OpIn::Proj {
            dom,
            curve,
            vec,
            dist,
        } => {
            v.extend(dom);
            push_crv(&mut v, curve);
            v.extend(vec);
            push_opt(&mut v, *dist);
            Op::Proj
        }
        OpIn::LoadGuesses { dom, p, sop } => {
            v.extend(dom);
            v.extend(p);
            v.push(f64::from(*sop));
            Op::LoadGuesses
        }
        OpIn::PassSc { uv, curve, t } => {
            v.extend(uv);
            push_crv(&mut v, curve);
            v.push(*t);
            Op::PassSc
        }
        OpIn::PassSs { uv, other, uv2 } => {
            v.extend(uv);
            push_srf(&mut v, other)?;
            v.extend(uv2);
            Op::PassSs
        }
        OpIn::GoodDrop {
            dom,
            p,
            guess,
            value,
            sol,
        } => {
            v.extend(dom);
            v.extend(p);
            v.extend(guess);
            v.push(*value);
            v.extend(sol);
            Op::GoodDrop
        }
        OpIn::CheckSeam {
            uv,
            dom,
            other,
            uv2,
            dom2,
            loc,
        } => {
            v.extend(uv);
            v.extend(dom);
            push_srf(&mut v, other)?;
            v.extend(uv2);
            v.extend(dom2);
            v.push(f64::from(*loc));
            Op::CheckSeam
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

pub(crate) fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

pub(crate) fn p2(p: [f64; 2]) -> Point2 {
    Point2::new(p[0], p[1])
}

pub(crate) fn dom_of(d: [f64; 4]) -> Extent2d {
    Extent2d::from_points(Point2::new(d[0], d[1]), Point2::new(d[2], d[3]))
}

pub(crate) fn side(left: bool) -> Side {
    if left {
        Side::Left
    } else {
        Side::Right
    }
}

pub(crate) fn surface_of(s: &SrfIn) -> Result<Surface, HarnessError> {
    match &resolve(s)? {
        SrfIn::Sms(_) => Err(harness("unresolved surface file")),
        SrfIn::Bounded { basis, implicit } => Ok(Surface::from(CurveBoundedSurface::new(
            surface_of(basis)?,
            Vec::new(),
            Vec::new(),
            *implicit,
        ).map_err(|e| HarnessError::new(e.to_string()))?)),
        SrfIn::Plane { o, n } => Ok(Surface::from(Plane::from_normal(v3(*o), v3(*n)))),
        SrfIn::Revolution { a, b, p, axis, swap, oob } => {
            let k = Kernel::default();
            let mut l = Line::from_points(v3(*a), v3(*b), 3);
            l.make_nurb().map_err(|e| harness(format!("line nurb: {e}")))?;
            let mut r = SurfOfRevolution::create_canonical(
                &k,
                SmCurve::from(l),
                v3(*p),
                v3(*axis),
                *swap,
            )
            .map_err(|e| harness(format!("revolution: {e}")))?;
            r.set_out_of_bounds_enabled(*oob);
            Ok(Surface::from(r))
        }
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
            Ok(Surface::from(s))
        }
    }
}

pub(crate) fn curve_of(c: &CrvIn) -> Result<SmCurve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
        .collect();
    let cur = Curve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve input: {e}")))?;
    Ok(SmCurve::from(BSplineCurve::from_nurb(cur, c.dim, false)))
}

pub(crate) fn sop_of(v: u8) -> Result<SolverOperation, HarnessError> {
    SolverOperation::from_code(i32::from(v)).ok_or_else(|| harness(format!("bad solver op {v}")))
}

pub(crate) fn param_of(v: u8) -> Result<SurfParamType, HarnessError> {
    SurfParamType::from_code(u64::from(v)).ok_or_else(|| harness(format!("bad param {v}")))
}

fn loc_of(v: u8) -> Result<LocationType, HarnessError> {
    use LocationType as L;
    Ok(match v {
        0 => L::Pole,
        1 => L::USeam,
        2 => L::VSeam,
        3 => L::UvSeam,
        4 => L::Interior,
        5 => L::Exterior,
        _ => return Err(harness(format!("bad location {v}"))),
    })
}

pub(crate) fn req_of(v: u8) -> Result<SolutionRequested, HarnessError> {
    SolutionRequested::ALL
        .get(usize::from(v))
        .copied()
        .ok_or_else(|| harness(format!("bad request {v}")))
}

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
pub(crate) fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// A Rust failure: deferred for `Unported`, else a harness error.
pub(crate) enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Fail::Harness(e)
    }
}

/// `Ok(v)` or the status; `Unported` defers the case.
pub(crate) fn st<T>(r: solidcore::Result<T>) -> Result<Result<T, f64>, Fail> {
    match r {
        Ok(v) => Ok(Ok(v)),
        Err(e) => match Output::unported(&e) {
            Some(o) => Err(Fail::Deferred(Box::new(o))),
            None => Ok(Err(status(&e))),
        },
    }
}

pub(crate) fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

pub(crate) fn puv(o: &mut Vec<f64>, v: Vec2) {
    o.extend([v.x, v.y]);
}

/// A solution object for the solves (never stored: they are deferred).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Obj;

impl SolutionObject for Obj {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Surface
    }
}

/// The handles of the global curve and surface solves (wired by unit
/// `surface.surface_p11`): the solved curve or surface, with the natural interval or
/// domain upstream reads through the object pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Handle {
    id: u8,
    interval: Option<Extent1d>,
    domain: Option<Extent2d>,
}

impl SolutionObject for Handle {
    fn kind(&self) -> ObjectKind {
        if self.interval.is_some() {
            ObjectKind::Curve
        } else {
            ObjectKind::Surface
        }
    }
    fn natural_interval(&self) -> Option<Extent1d> {
        self.interval
    }
    fn natural_uv_domain(&self) -> Option<Extent2d> {
        self.domain
    }
}

fn surface_handle(id: u8, s: &Surface) -> Handle {
    Handle {
        id,
        interval: None,
        // (runner helper without a kernel in scope)
        domain: Some(s.natural_uv_domain_if_built()),
    }
}

/// The first [`SOLS_OUT`] solutions of a global solve, as the shim's `put_sols`
/// writes them: type, value, variable count (at most 4), the start parameters and,
/// for a range, the end parameters.
const SOLS_OUT: usize = 16;

fn put_sols(o: &mut Vec<f64>, sols: &SolutionArray<Handle>) -> Result<(), Fail> {
    for s in sols.as_slice().iter().take(SOLS_OUT) {
        let n = s.num_variables.min(4);
        o.extend([f64::from(s.solution_type as i32), s.start.solution_value, n as f64]);
        for j in 0..n {
            o.push(s.start.param(j).map_err(|e| harness(e.to_string()))?);
        }
        if s.solution_type == SolutionType::RangeOfValues {
            for j in 0..n {
                o.push(s.end.param(j).map_err(|e| harness(e.to_string()))?);
            }
        }
    }
    Ok(())
}

fn rust_run(k: &Kernel, i: &Inputs) -> Result<Vec<f64>, Fail> {
    let s = surface_of(&i.surface)?;
    let mut o = Vec::new();
    match &i.op {
        OpIn::Eval1 { uv, ul, vl } => {
            match st(evaluate_1st_derivatives(k, &s, p2(*uv), side(*ul), side(*vl)))? {
                Ok((p, du, dv)) => {
                    o.push(0.0);
                    for x in [p, du, dv] {
                        pv(&mut o, x);
                    }
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::Eval2 { uv, ul, vl } => {
            match st(evaluate_2nd_derivatives(k, &s, p2(*uv), side(*ul), side(*vl)))? {
                Ok(d) => {
                    o.push(0.0);
                    for x in [d.point, d.du, d.dv, d.duv, d.duu, d.dvv] {
                        pv(&mut o, x);
                    }
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::DirDeriv { uv, dir, h, ul, vl } => {
            match st(evaluate_directional_derivs(
                k, &s,
                p2(*uv),
                p2(*dir),
                *h,
                side(*ul),
                side(*vl),
            ))? {
                Ok(d) => {
                    o.push(0.0);
                    for x in d {
                        pv(&mut o, x);
                    }
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::Normal { uv, ul, vl, base } => {
            let r = if *base {
                base_evaluate_normal(k, &s, p2(*uv), side(*ul), side(*vl))
            } else {
                s.evaluate_normal(k, p2(*uv), side(*ul), side(*vl))
            };
            match st(r)? {
                Ok(n) => {
                    o.push(0.0);
                    pv(&mut o, n);
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::IsClosed {
            dom,
            param,
            tol,
            want,
            base,
        } => {
            let d = dom_of(*dom);
            let prm = param_of(*param)?;
            let r = if *base {
                base_is_closed(k, &s, &d, prm, *tol, *want)
            } else {
                s.is_closed(k, &d, prm, *tol, *want)
            };
            let (closed, cont) = st(r)?.map_err(|c| harness(format!("IsClosed failed: {c}")))?;
            o.extend([0.0, b(closed), cont.map_or(-1.0, |c| f64::from(c as i32))]);
        }
        OpIn::IsPlanar { tol, base } => {
            if *base && matches!(i.surface, SrfIn::Plane { .. }) {
                return Err(harness("base IsPlanar of a plane is not compared").into());
            }
            let pl = st(s.is_planar(k, *tol))?.map_err(|c| harness(format!("IsPlanar failed: {c}")))?;
            o.extend([0.0, b(pl.is_planar)]);
            pv(&mut o, pl.point);
            pv(&mut o, pl.normal);
            o.push(pl.deviation);
        }
        OpIn::ApproxLen { dom } => {
            let l = st(approx_derivative_lengths(k, &s, &dom_of(*dom)))?
                .map_err(|c| harness(format!("ApproxDerivativeLengths failed: {c}")))?;
            o.push(0.0);
            puv(&mut o, l);
        }
        OpIn::Higher {
            uv,
            g1,
            want3,
            want4,
        } => match st(compute_higher_order_derivs(k, &s, p2(*uv), p2(*g1), *want3, *want4))? {
            Ok(h) => {
                o.push(0.0);
                pv(&mut o, h.g2);
                if let Some(g) = h.g3 {
                    pv(&mut o, g);
                }
                if let Some(g) = h.g4 {
                    pv(&mut o, g);
                }
            }
            Err(c) => o.push(c),
        },
        OpIn::Inward { uv } => match st(compute_inward_vector(k, &s, p2(*uv)))? {
            Ok(v) => {
                o.push(0.0);
                pv(&mut o, v);
            }
            Err(c) => o.push(c),
        },
        OpIn::DropPts { uv, ul, vl, pts } => {
            let pts: Vec<Vec3> = pts.iter().map(|p| v3(*p)).collect();
            match st(drop_points_to_tangent_plane(k, &s, p2(*uv), side(*ul), side(*vl), &pts))? {
                Ok(r) => {
                    o.push(0.0);
                    for x in r {
                        puv(&mut o, x);
                    }
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::DropVecs { uv, ul, vl, vecs } => {
            let vecs: Vec<Vec3> = vecs.iter().map(|p| v3(*p)).collect();
            match st(drop_vectors_at(k, &s, p2(*uv), side(*ul), side(*vl), &vecs))? {
                Ok(r) => {
                    o.push(0.0);
                    for x in r {
                        puv(&mut o, x);
                    }
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::Degen {
            uv,
            dir,
            from_pole,
            sdir,
            tol,
        } => {
            let r = s.find_degen_param_for_direction(
                k, p2(*uv),
                v3(*dir),
                *from_pole,
                param_of(*sdir)?,
                *tol,
            );
            match st(r)? {
                Ok(d) => {
                    o.push(if d.converged { 0.0 } else { SM_ERR });
                    puv(&mut o, d.uv);
                }
                Err(c) => {
                    o.push(c);
                    o.extend(uv);
                }
            }
        }
        OpIn::Flat { angle, want } => {
            let f = st(s.flat_corners(&Kernel::default(), *angle, *want))?
                .map_err(|c| harness(format!("GetFlatCorners failed: {c}")))?;
            o.extend([0.0, f64::from(f.flags)]);
            if let Some(pts) = f.points {
                for x in pts {
                    pv(&mut o, x);
                }
            }
        }
        OpIn::Brep => {
            let r = st(s.brep())?.map_err(|c| harness(format!("GetBrep failed: {c}")))?;
            o.extend([0.0, b(r.is_some())]);
        }
        OpIn::Gps0 {
            dom,
            sop,
            p,
            tol,
            target,
            req,
        } => {
            let k = Kernel::default();
            let r: solidcore::Result<SolutionArray<Obj>> = s.global_point_solve_0(
                &k,
                Obj,
                &dom_of(*dom),
                sop_of(*sop)?,
                v3(*p),
                *tol,
                *target,
                req_of(*req)?,
            );
            match st(r)? {
                Ok(a) => {
                    o.extend([0.0, a.len() as f64]);
                    for sol in a.as_slice() {
                        o.extend([
                            sol.start.solution_value,
                            sol.start.parameters.first().copied().unwrap_or(0.0),
                            sol.start.parameters.get(1).copied().unwrap_or(0.0),
                        ]);
                    }
                }
                Err(c) => o.extend([c, 0.0]),
            }
        }
        OpIn::Gcs {
            dom,
            curve,
            ivl,
            sop,
            tol,
            vecs,
            target,
            req,
        } => {
            let c = curve_of(curve)?;
            let vecs: Vec<Vec3> = vecs.iter().map(|p| v3(*p)).collect();
            let k = Kernel::default();
            let curve_handle = Some(Handle {
                id: 0,
                interval: Some(c.natural_interval()),
                domain: None,
            });
            let r: solidcore::Result<SolutionArray<Handle>> = s.global_curve_solve(
                &k,
                &dom_of(*dom),
                &c,
                &Extent1d::from_min_max(ivl[0], ivl[1]),
                sop_of(*sop)?,
                *tol,
                *target,
                &vecs,
                req_of(*req)?,
                [curve_handle, Some(surface_handle(1, &s))],
            );
            match st(r)? {
                Ok(a) => {
                    o.extend([0.0, a.len() as f64]);
                    put_sols(&mut o, &a)?;
                }
                Err(c) => o.extend([c, 0.0]),
            }
        }
        OpIn::Gss {
            dom,
            other,
            dom2,
            sop,
            tol,
            vecs,
            target,
            req,
        } => {
            let s2 = surface_of(other)?;
            let vecs: Vec<Vec3> = vecs.iter().map(|p| v3(*p)).collect();
            let k = Kernel::default();
            let handles = [Some(surface_handle(1, &s)), Some(surface_handle(2, &s2))];
            let r: solidcore::Result<SolutionArray<Handle>> = s.global_surface_solve(
                &k,
                &dom_of(*dom),
                &s2,
                &dom_of(*dom2),
                sop_of(*sop)?,
                *tol,
                *target,
                &vecs,
                req_of(*req)?,
                handles,
            );
            match st(r)? {
                Ok(a) => {
                    o.extend([0.0, a.len() as f64]);
                    put_sols(&mut o, &a)?;
                }
                Err(c) => o.extend([c, 0.0]),
            }
        }
        OpIn::Lcs {
            dom,
            curve,
            ivl,
            sop,
            tol,
            vecs,
            guess_uv,
            guess_t,
        } => {
            let c = curve_of(curve)?;
            let vecs: Vec<Vec3> = vecs.iter().map(|p| v3(*p)).collect();
            let r = s.local_curve_solve(
                k, &dom_of(*dom),
                &c,
                &Extent1d::from_min_max(ivl[0], ivl[1]),
                sop_of(*sop)?,
                *tol,
                &vecs,
                p2(*guess_uv),
                *guess_t,
            );
            match st(r)? {
                Ok(Some(x)) => o.extend([0.0, 1.0, x.distance, x.t, x.uv.x, x.uv.y]),
                Ok(None) => o.extend([0.0, 0.0]),
                Err(c) => o.extend([c, 0.0]),
            }
        }
        OpIn::Proj {
            dom,
            curve,
            vec,
            dist,
        } => {
            let pw = curve
                .cpts
                .iter()
                .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                .collect();
            let cur = Curve::new(pw, curve.p, curve.u.clone())
                .map_err(|e| harness(format!("curve input: {e}")))?;
            let bc = BSplineCurve::from_nurb(cur, curve.dim, false);
            let k = Kernel::default();
            match st(s.create_parallel_projection_curves(&k, &dom_of(*dom), &bc, v3(*vec), *dist, None, None))? {
                Ok((c3d, _)) => o.extend([0.0, c3d.len() as f64]),
                Err(c) => o.extend([c, 0.0]),
            }
        }
        OpIn::LoadGuesses { dom, p, sop } => {
            match st(load_guesses(k, &s, &dom_of(*dom), v3(*p), sop_of(*sop)?))? {
                Ok(g) => {
                    o.push(0.0);
                    puv(&mut o, g);
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::PassSc { uv, curve, t } => {
            let c = curve_of(curve)?;
            let r = st(pass_sc_convergence_test(k, &s, p2(*uv), &c, *t))?
                .map_err(|c| harness(format!("pass failed: {c}")))?;
            o.extend([0.0, b(r)]);
        }
        OpIn::PassSs { uv, other, uv2 } => {
            let s2 = surface_of(other)?;
            let r = st(pass_ss_convergence_test(k, &s, p2(*uv), &s2, p2(*uv2)))?
                .map_err(|c| harness(format!("pass failed: {c}")))?;
            o.extend([0.0, b(r)]);
        }
        OpIn::GoodDrop {
            dom,
            p,
            guess,
            value,
            sol,
        } => {
            let mut end = SolutionEnd::new();
            end.solution_value = *value;
            if let (Some(a), Some(c)) = (end.parameters.get_mut(0), Some(sol[0])) {
                *a = c;
            }
            if let Some(a) = end.parameters.get_mut(1) {
                *a = sol[1];
            }
            let r = st(good_drop_result(k, &s, &dom_of(*dom), v3(*p), p2(*guess), &end))?
                .map_err(|c| harness(format!("good drop failed: {c}")))?;
            o.extend([0.0, b(r)]);
        }
        OpIn::CheckSeam {
            uv,
            dom,
            other,
            uv2,
            dom2,
            loc,
        } => {
            let s2 = surface_of(other)?;
            match st(check_seam_drop(
                k, &s,
                p2(*uv),
                &dom_of(*dom),
                &s2,
                p2(*uv2),
                &dom_of(*dom2),
                loc_of(*loc)?,
            ))? {
                Ok(r) => {
                    o.push(0.0);
                    puv(&mut o, r);
                }
                Err(c) => o.push(c),
            }
        }
    }
    Ok(o)
}

fn rust(k: &Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    match rust_run(k, i) {
        Ok(v) => Ok(Output::new().values("out", v)),
        Err(Fail::Deferred(o)) => Ok(*o),
        Err(Fail::Harness(e)) => Err(e),
    }
}

// ---- generator --------------------------------------------------------------------

pub(crate) fn gen_knots(rng: &mut Rng, p: usize, n: usize) -> Vec<f64> {
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
pub(crate) enum Shape {
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

pub(crate) fn gen_bss(rng: &mut Rng, shape: Shape) -> BssIn {
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
            let apex = [-scale, m as f64 * scale / 2.0, rng.uniform(-0.5, 0.5) * scale];
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
                    *c = [t * scale, 2.0 * t * scale, rng.uniform(-noise, noise) * scale];
                }
            }
        }
        Shape::Pinched => {
            // P[1][0] and P[0][1] on one line through P[0][0]
            let p00 = net.first().and_then(|r| r.first()).copied().unwrap_or([0.0; 3]);
            let dir = [1.0, 0.5, 0.25];
            let (a, bb) = (rng.uniform(0.5, 1.5), -rng.uniform(0.5, 1.5));
            let at = |k: f64| [p00[0] + k * dir[0] * scale, p00[1] + k * dir[1] * scale, p00[2] + k * dir[2] * scale];
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
        form: if rng.chance(0.9) { 11 } else { rng.below(10) as u8 },
        net,
        p,
        q,
        u: gen_knots(rng, p, n),
        v: gen_knots(rng, q, m),
    }
}

pub(crate) fn gen_plane(rng: &mut Rng) -> SrfIn {
    let mut n = rng.point3(-1.0, 1.0);
    if n.iter().map(|c| c * c).sum::<f64>() < 1e-2 {
        n = [0.0, 0.0, 1.0];
    }
    SrfIn::Plane {
        o: rng.point3(-5.0, 5.0),
        n,
    }
}

pub(crate) fn gen_surface(rng: &mut Rng) -> SrfIn {
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
pub(crate) fn natural(s: &SrfIn) -> [f64; 4] {
    match s {
        SrfIn::Sms(_) => [0.0, 0.0, 1.0, 1.0],
        SrfIn::Bounded { basis, .. } => natural(basis),
        SrfIn::Bspline(bs) => [
            bs.u.first().copied().unwrap_or(0.0),
            bs.v.first().copied().unwrap_or(0.0),
            bs.u.last().copied().unwrap_or(1.0),
            bs.v.last().copied().unwrap_or(1.0),
        ],
        SrfIn::Plane { .. } | SrfIn::Revolution { .. } => {
            let d = surface_of(s).map_or_else(
                |_| Extent2d::from_points(Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)),
                // (runner helper without a kernel in scope)
                |x| x.natural_uv_domain_if_built(),
            );
            [d.min().x, d.min().y, d.max().x, d.max().y]
        }
    }
}

/// A parameter in `[a, b]`: interior, at an end, at a knot.
pub(crate) fn gen_param(rng: &mut Rng, a: f64, b: f64, knots: Option<&[f64]>) -> f64 {
    match rng.below(10) {
        0 => a,
        1 => b,
        2 => knots.and_then(|k| rng.choose(k).copied()).unwrap_or(0.5 * (a + b)),
        3 => a + (b - a) * rng.uniform(0.0, 1e-6),
        _ => a + (b - a) * rng.unit(),
    }
}

pub(crate) fn gen_uv(rng: &mut Rng, s: &SrfIn) -> [f64; 2] {
    let [u0, v0, u1, v1] = natural(s);
    let base = match s {
        SrfIn::Bounded { basis, .. } => basis.as_ref(),
        _ => s,
    };
    let (ku, kv) = match base {
        SrfIn::Bspline(bs) => (Some(bs.u.as_slice()), Some(bs.v.as_slice())),
        SrfIn::Plane { .. } | SrfIn::Revolution { .. } | SrfIn::Sms(_) | SrfIn::Bounded { .. } => {
            (None, None)
        }
    };
    [gen_param(rng, u0, u1, ku), gen_param(rng, v0, v1, kv)]
}

/// A `uv` point on a domain boundary or corner half the time.
pub(crate) fn gen_boundary_uv(rng: &mut Rng, s: &SrfIn) -> [f64; 2] {
    let [u0, v0, u1, v1] = natural(s);
    let mut uv = gen_uv(rng, s);
    if rng.chance(0.5) {
        match rng.below(3) {
            0 => uv[0] = if rng.chance(0.7) { u0 } else { u1 },
            1 => uv[1] = if rng.chance(0.7) { v0 } else { v1 },
            _ => uv = [if rng.chance(0.7) { u0 } else { u1 }, if rng.chance(0.7) { v0 } else { v1 }],
        }
    }
    uv
}

pub(crate) fn gen_dom(rng: &mut Rng, s: &SrfIn) -> [f64; 4] {
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

pub(crate) fn gen_curve(rng: &mut Rng) -> CrvIn {
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

pub(crate) fn gen_point_near(k: &Kernel, rng: &mut Rng, s: &SrfIn) -> [f64; 3] {
    if let Ok(srf) = surface_of(s) {
        let uv = gen_uv(rng, s);
        if let Ok(p) = srf.evaluate_point(k, p2(uv)) {
            let d = *rng.choose(&[0.0, 1e-9, 1e-4, 0.1, 1.0, 5.0]).unwrap_or(&0.1);
            let e = rng.point3(-d, d);
            return [p.x + e[0], p.y + e[1], p.z + e[2]];
        }
    }
    rng.point3(-5.0, 5.0)
}

pub(crate) fn gen_vecs(rng: &mut Rng, n: usize) -> Vec<[f64; 3]> {
    (0..n)
        .map(|_| {
            if rng.chance(0.1) {
                [0.0; 3]
            } else {
                rng.point3(-2.0, 2.0)
            }
        })
        .collect()
}

/// A non-zero direction (the signed directed solves' `m_cpOptVectors[0]`).
fn gen_direction(rng: &mut Rng) -> [f64; 3] {
    let mut d = rng.point3(-1.0, 1.0);
    d[2] += if d[2] < 0.0 { -0.5 } else { 0.5 };
    d
}

/// The `z` range of a B-spline surface's control points (homogeneous, weight `NOW`
/// for non-rational).
fn z_range(bs: &BssIn) -> (f64, f64) {
    bs.net
        .iter()
        .flatten()
        .map(|p| p[2] / if p[3] == NOW { 1.0 } else { p[3] })
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| (lo.min(z), hi.max(z)))
}

/// Moves `other` in `z` so its control hull (and so the surface) lies `gap` above
/// `below`'s.
fn lift_above(other: &mut BssIn, below: &BssIn, gap: f64) {
    let dz = z_range(below).1 - z_range(other).0 + gap;
    for p in other.net.iter_mut().flatten() {
        let w = if p[3] == NOW { 1.0 } else { p[3] };
        p[2] += dz * w;
    }
}

fn gen_op(k: &Kernel, rng: &mut Rng, s: &SrfIn) -> OpIn {
    // planes and curve-bounded surfaces: not for the B-spline-only ops
    let is_plane = !matches!(s, SrfIn::Bspline(_) | SrfIn::Sms(_));
    let flag = |rng: &mut Rng| rng.chance(0.5);
    match rng.below(40) {
        0..=2 => OpIn::Eval1 {
            uv: gen_uv(rng, s),
            ul: flag(rng),
            vl: flag(rng),
        },
        3..=4 => OpIn::Eval2 {
            uv: gen_uv(rng, s),
            ul: flag(rng),
            vl: flag(rng),
        },
        5..=6 => OpIn::DirDeriv {
            uv: gen_uv(rng, s),
            dir: if rng.chance(0.1) {
                [0.0, 0.0]
            } else {
                [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)]
            },
            h: rng.below(5) as usize,
            ul: flag(rng),
            vl: flag(rng),
        },
        7..=13 => OpIn::Normal {
            uv: gen_boundary_uv(rng, s),
            ul: flag(rng),
            vl: flag(rng),
            base: rng.chance(0.3),
        },
        14..=17 => OpIn::IsClosed {
            dom: gen_dom(rng, s),
            param: *rng.choose(&[0u8, 1, 2, 3]).unwrap_or(&0),
            tol: rng.chance(0.4).then(|| *rng.choose(&[0.0, -1e-3, 1e-9, 1e-6, 1e-3, 0.1]).unwrap_or(&1e-6)),
            want: flag(rng),
            base: is_plane && rng.chance(0.5),
        },
        18..=19 => OpIn::IsPlanar {
            tol: *rng.choose(&[0.0, 1e-12, 1e-9, 1e-6, 1e-3, 0.5]).unwrap_or(&1e-6),
            base: !is_plane && rng.chance(0.3),
        },
        20 => OpIn::ApproxLen { dom: gen_dom(rng, s) },
        21..=22 => OpIn::Higher {
            uv: gen_uv(rng, s),
            g1: if rng.chance(0.05) {
                [0.0, 0.0]
            } else {
                [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)]
            },
            want3: flag(rng),
            want4: flag(rng),
        },
        23 => OpIn::Inward { uv: gen_uv(rng, s) },
        24 => OpIn::DropPts {
            uv: gen_uv(rng, s),
            ul: flag(rng),
            vl: flag(rng),
            pts: (0..rng.below(4) as usize).map(|_| gen_point_near(k, rng, s)).collect(),
        },
        25 => {
            let n = rng.below(4) as usize;
            OpIn::DropVecs {
                uv: gen_uv(rng, s),
                ul: flag(rng),
                vl: flag(rng),
                vecs: gen_vecs(rng, n),
            }
        }
        26..=28 => {
            let [u0, v0, u1, v1] = natural(s);
            let sdir = rng.below(2) as u8;
            // on the degenerate (t) boundary: v = min/max for a u-singularity
            let mut uv = gen_uv(rng, s);
            if sdir == 0 {
                uv[1] = if rng.chance(0.5) { v0 } else { v1 };
            } else {
                uv[0] = if rng.chance(0.5) { u0 } else { u1 };
            }
            OpIn::Degen {
                uv,
                dir: if rng.chance(0.05) { [0.0; 3] } else { rng.point3(-1.0, 1.0) },
                from_pole: flag(rng),
                sdir,
                tol: *rng.choose(&[1e-9, 1e-6, 1e-3]).unwrap_or(&1e-6),
            }
        }
        29 => OpIn::Flat {
            angle: *rng.choose(&[1e-4, 0.5, 5.0]).unwrap_or(&0.5),
            want: flag(rng),
        },
        30 => OpIn::Brep,
        31 if !is_plane => OpIn::Gps0 {
            dom: gen_dom(rng, s),
            sop: *rng.choose(&[0u8, 3, 4, 5]).unwrap_or(&3),
            p: gen_point_near(k, rng, s),
            tol: 1e-6,
            target: None,
            req: rng.below(2) as u8,
        },
        32 if !is_plane => {
            let c = gen_curve(rng);
            let (a, bb) = (c.u.first().copied().unwrap_or(0.0), c.u.last().copied().unwrap_or(1.0));
            // minimize, maximize, normalize and signed directed minimize (along a
            // non-zero vector) reach every branch of SmCSGlobalSolver::LocalSolve
            let sop = *rng.choose(&[3u8, 4, 5, 9]).unwrap_or(&3);
            OpIn::Gcs {
                dom: gen_dom(rng, s),
                curve: c,
                ivl: [a, bb],
                sop,
                tol: 1e-6,
                vecs: if sop == 9 { vec![gen_direction(rng)] } else { Vec::new() },
                target: None,
                req: 0,
            }
        }
        33 if !is_plane => {
            let c = gen_curve(rng);
            let (a, bb) = (c.u.first().copied().unwrap_or(0.0), c.u.last().copied().unwrap_or(1.0));
            let sop = *rng.choose(&[3u8, 4, 5, 9]).unwrap_or(&3);
            OpIn::Lcs {
                dom: gen_dom(rng, s),
                curve: c,
                ivl: [a, bb],
                sop,
                tol: 1e-6,
                vecs: if sop == 9 { gen_vecs(rng, 1) } else { Vec::new() },
                guess_uv: gen_uv(rng, s),
                guess_t: 0.5 * (a + bb),
            }
        }
        // surface/surface global solves: a bumpy patch lifted clear above a B-spline
        // surface (two random surfaces often intersect, and minimizing over the
        // intersection curve's continuum of zero distances takes upstream minutes);
        // intersect (for the node pairs) on the crossing surfaces
        34 => match s {
            SrfIn::Bspline(bs) => {
                let sop = *rng.choose(&[0u8, 3, 4, 5, 9]).unwrap_or(&3);
                let mut other = gen_bss(rng, Shape::Bumpy);
                if sop != 0 {
                    let gap = *rng.choose(&[0.05, 0.5, 2.0]).unwrap_or(&0.5);
                    lift_above(&mut other, bs, gap);
                }
                let other = SrfIn::Bspline(other);
                OpIn::Gss {
                    dom: gen_dom(rng, s),
                    dom2: gen_dom(rng, &other),
                    other,
                    sop,
                    tol: 1e-6,
                    vecs: if sop == 9 { vec![gen_direction(rng)] } else { Vec::new() },
                    target: None,
                    req: if sop == 0 { 3 } else { rng.below(2) as u8 },
                }
            }
            _ => OpIn::Brep,
        },
        35 if !is_plane => OpIn::Proj {
            dom: gen_dom(rng, s),
            curve: gen_curve(rng),
            // a NaN vector fails ZERO_VEC_ER like a zero one (an infinite one would
            // reach the upstream intersector with NaN geometry)
            vec: match rng.below(10) {
                0..=5 => [0.0; 3],
                6 | 7 => [0.0, f64::NAN, 1.0],
                _ => [0.0, 0.0, 1.0],
            },
            dist: None,
        },
        36 => OpIn::LoadGuesses {
            dom: gen_dom(rng, s),
            p: gen_point_near(k, rng, s),
            sop: *rng.choose(&[0u8, 3, 4, 5]).unwrap_or(&3),
        },
        37 => {
            let c = gen_curve(rng);
            let (a, bb) = (c.u.first().copied().unwrap_or(0.0), c.u.last().copied().unwrap_or(1.0));
            OpIn::PassSc {
                uv: gen_uv(rng, s),
                t: a + (bb - a) * rng.unit(),
                curve: c,
            }
        }
        38 => {
            let o = gen_surface(rng);
            OpIn::PassSs {
                uv: gen_uv(rng, s),
                uv2: gen_uv(rng, &o),
                other: o,
            }
        }
        _ => {
            if rng.chance(0.5) {
                let dom = gen_dom(rng, s);
                let guess = gen_uv(rng, s);
                let sol = if rng.chance(0.5) { guess } else { gen_uv(rng, s) };
                OpIn::GoodDrop {
                    dom,
                    p: gen_point_near(k, rng, s),
                    guess,
                    value: *rng.choose(&[0.0, 1e-13, 1e-3, 0.5]).unwrap_or(&0.5),
                    sol,
                }
            } else {
                let o = gen_surface(rng);
                OpIn::CheckSeam {
                    uv: gen_uv(rng, s),
                    dom: gen_dom(rng, s),
                    uv2: gen_uv(rng, &o),
                    dom2: gen_dom(rng, &o),
                    other: o,
                    loc: rng.below(6) as u8,
                }
            }
        }
    }
}

/// The ops that run a global solve.
fn is_solve(op: &OpIn) -> bool {
    matches!(
        op,
        OpIn::Gps0 { .. } | OpIn::Gcs { .. } | OpIn::Gss { .. } | OpIn::Lcs { .. } | OpIn::Proj { .. }
    )
}

/// Whether a surface's control points span roughly unit size (`0.5..=100`).
pub(crate) fn unit_scale(s: &SrfIn) -> bool {
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
        _ => false,
    }
}

fn gen_case(k: &Kernel, rng: &mut Rng) -> Inputs {
    let surface = gen_surface(rng);
    let op = gen_op(k, rng, &surface);
    Inputs { surface, op }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surface_p7"
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
            // keep the global solves and the projection off extreme scales, where the
            // subdivision of both sides runs for minutes (checked before the Rust run
            // below, which runs the solve too)
            if is_solve(&i.op) && !unit_scale(&i.surface) {
                continue;
            }
            if let OpIn::Gss { other, .. } = &i.op {
                if !unit_scale(other) {
                    continue;
                }
            }
            if let Err(Fail::Harness(_)) = rust_run(k, &i) {
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
        let k = &Kernel::default();
        rust(k, &case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Prints generated case `P7_CASE` as JSON (a debugging aid:
    /// `P7_CASE=6 cargo test -p difftest --features cpp --lib p7_dump_case -- --ignored --nocapture`).
    #[test]
    #[ignore = "debugging aid; needs P7_CASE"]
    fn p7_dump_case() {
        let i: usize = std::env::var("P7_CASE").unwrap().parse().unwrap();
        let mut rng = Rng::new(CaseContext::seed_for("surface.surface_p7", i));
        let v = Runner.generate(&mut rng).unwrap();
        println!("{}", serde_json::to_string(&v).unwrap());
    }

    /// Generated cases cover every op, most of the comparable ones not deferred.
    #[test]
    fn generated_cases_are_diverse() {
        let k = &Kernel::default();
        let mut rng = Rng::new(7);
        let mut stats: BTreeMap<String, [usize; 3]> = BTreeMap::new();
        for _ in 0..1500 {
            let Some(v) = Runner.generate(&mut rng) else {
                continue;
            };
            let i: Inputs = serde_json::from_value(v).unwrap();
            let name = serde_json::to_value(&i.op).unwrap()["op"].as_str().unwrap().to_string();
            let e = stats.entry(name).or_default();
            e[0] += 1;
            match rust_run(k, &i) {
                Ok(_) => e[1] += 1,
                Err(_) => e[2] += 1,
            }
        }
        for (k, v) in &stats {
            eprintln!("{k}: total {} ran {} deferred {}", v[0], v[1], v[2]);
        }
        assert!(stats.len() >= 22, "{}", stats.len());
    }
}

