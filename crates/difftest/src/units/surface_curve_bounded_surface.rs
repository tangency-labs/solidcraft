//! Runner for unit `surface.curve_bounded_surface`: `SmCurveBoundedSurface` (construction,
//! the evaluation overrides passed along to the basis surface, the `SmSurface` defaults it
//! does not override, type queries, `operator==`, `SetContext` and `ReadFromDB`).
//!
//! C++: `solidcore_ffi::units::surface_curve_bounded_surface` (upstream
//! `source/SMLib/src/SmCurveBoundedSurface.cpp`, called through `SmSurface*` so the virtual
//! dispatch is upstream's). Rust: `solidcore::surface::curve_bounded_surface` through
//! `solidcore::surface::surface::p2::Surface`.
//!
//! A case is a curve bounded surface `this` (a B-spline basis, inline or an upstream `.sms`
//! file, the implicit-outer flag, 3D and UV B-spline boundaries, inline or upstream `.smc`
//! files) and an `op` with its parameters, or the static `read_db` op (an ASCII record built
//! by `ffi::cbs_record` from the same kind of data, read by both sides). Both sides produce
//! the flat record documented in the ffi module; the runner reports its first entry as the
//! `status` (`0` = `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust `InvalidInput`, `-1`
//! for a Rust error upstream cannot produce) and the rest as the `out` values.
//!
//! Deferred cases: the evaluation overrides whose basis-surface call is not in the Rust
//! `Surface` dispatch yet (`EvaluateNormal`, `EvaluateSimple`, `EvaluateNormalAtSingularity`,
//! `EvaluateGeometric`, `EvaluateNormalSection`, `EvaluateSTEP`, `EvaluateSTEPPoint`) and
//! `operator==` between two objects (`SmSurface::operator==`). Every run has at least
//! [`MIN_GENERATED`] generated cases; generated inputs the ffi wrapper rejects are redrawn.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::CurveKind;
use solidcore::curve::curve::Curve as SmCurve;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm, SurfParamType};
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::curve_bounded_surface::{self as cbs, CurveBoundedSurface};
use solidcore::surface::surface::p2::{Surface, SurfaceKind};
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::SolidError;
use solidcore_ffi::units::surface_curve_bounded_surface::{
    self as ffi, CbsRecord, Op, RecordCurve, RecordSurface,
};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 400;

/// `surface.curve_bounded_surface` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

/// An inline surface: control net as stored (`[x, y, z, w]`, rows in u), degrees,
/// knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineSrf {
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

/// A surface input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum SrfIn {
    /// An upstream `.sms` file (absolute path after fixture expansion).
    File { sms: String },
    /// Inline data.
    Inline(InlineSrf),
}

/// An inline curve: dimension, control points as stored, degree, knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineCrv {
    dim: u64,
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
}

/// A curve input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum CrvIn {
    /// An upstream `.smc` file (absolute path after fixture expansion).
    File { smc: String },
    /// Inline data.
    Inline(InlineCrv),
}

/// A curve bounded surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CbsIn {
    #[serde(default)]
    oob: bool,
    #[serde(default = "default_form")]
    form: u8,
    srf: SrfIn,
    #[serde(default)]
    implicit: bool,
    #[serde(default)]
    c3d: Vec<CrvIn>,
    #[serde(default)]
    cuv: Vec<CrvIn>,
}

fn default_form() -> u8 {
    BSplineSurfaceForm::Unspecified as u8
}

fn default_basis_type() -> i64 {
    ffi::B_SPLINE_SURFACE_TYPE
}

fn default_uv_header_type() -> i64 {
    ffi::B_SPLINE_CURVE_TYPE
}

fn default_version() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    State,
    Eval {
        uv: [f64; 2],
        hu: usize,
        hv: usize,
        #[serde(default)]
        u_left: bool,
        #[serde(default)]
        v_left: bool,
        #[serde(default)]
        upper: bool,
        #[serde(default = "yes")]
        nzt: bool,
    },
    EvalPt {
        uv: [f64; 2],
    },
    Queries {
        types: Vec<i64>,
    },
    Defaults {
        v: [f64; 3],
    },
    EqSelf,
    EqOther {
        other: CbsIn,
    },
    Normal {
        uv: [f64; 2],
        #[serde(default)]
        u_left: bool,
        #[serde(default)]
        v_left: bool,
    },
    Simple {
        uv: [f64; 2],
        hu: usize,
        hv: usize,
        #[serde(default)]
        u_left: bool,
        #[serde(default)]
        v_left: bool,
        #[serde(default)]
        upper: bool,
    },
    Singular {
        uv: [f64; 2],
        tol: f64,
    },
    Geometric {
        uv: [f64; 2],
        #[serde(default)]
        u_left: bool,
        #[serde(default)]
        v_left: bool,
    },
    NormalSection {
        uv: [f64; 2],
        #[serde(default)]
        u_left: bool,
        #[serde(default)]
        v_left: bool,
        dir: [f64; 3],
    },
    Step {
        uv: [f64; 2],
        hu: usize,
        hv: usize,
        #[serde(default)]
        u_left: bool,
        #[serde(default)]
        v_left: bool,
        #[serde(default)]
        upper: bool,
    },
    StepPt {
        uv: [f64; 2],
    },
    SetContext,
    /// Static: `SmSurface::ReadFromDB(SmCurveBoundedSurface_TYPE, ..)` of a record.
    ReadDb {
        #[serde(default = "default_version")]
        version: u64,
        #[serde(default)]
        implicit: bool,
        #[serde(default = "default_basis_type")]
        basis_type: i64,
        srf: SrfIn,
        #[serde(default)]
        c3d: Vec<CrvIn>,
        #[serde(default = "default_uv_header_type")]
        uv_header_type: i64,
        #[serde(default)]
        cuv: Vec<CrvIn>,
    },
}

fn yes() -> bool {
    true
}

impl OpIn {
    fn ffi(&self) -> Op {
        match self {
            Self::State => Op::State,
            Self::Eval { .. } => Op::Eval,
            Self::EvalPt { .. } => Op::EvalPt,
            Self::Queries { .. } => Op::Queries,
            Self::Defaults { .. } => Op::Defaults,
            Self::EqSelf => Op::EqSelf,
            Self::EqOther { .. } => Op::EqOther,
            Self::Normal { .. } => Op::Normal,
            Self::Simple { .. } => Op::Simple,
            Self::Singular { .. } => Op::Singular,
            Self::Geometric { .. } => Op::Geometric,
            Self::NormalSection { .. } => Op::NormalSection,
            Self::Step { .. } => Op::Step,
            Self::StepPt { .. } => Op::StepPt,
            Self::SetContext => Op::SetContext,
            Self::ReadDb { .. } => Op::ReadDb,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    this: Option<CbsIn>,
    #[serde(flatten)]
    op: OpIn,
}

// ---- file readers -------------------------------------------------------------------

fn numbers(path: &str) -> Result<Vec<f64>, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    text.split_whitespace()
        .map(|t| {
            t.parse::<f64>()
                .map_err(|e| harness(format!("{path}: {e}")))
        })
        .collect()
}

fn int(path: &str, v: f64) -> Result<usize, HarnessError> {
    if v.fract() != 0.0 || v < 0.0 {
        return Err(harness(format!("{path}: bad count {v}")));
    }
    Ok(v as usize)
}

/// Reads an upstream `.sms` text file: `n m`, `p q`, rational flag, `(n+1)(m+1)`
/// points (`x y z`, or `wx wy wz w` when rational, v index fastest), `n+p+2` u knots,
/// `m+q+2` v knots.
fn read_sms(path: &str) -> Result<InlineSrf, HarnessError> {
    let v = numbers(path)?;
    let mut it = v.into_iter();
    let mut num = || {
        it.next()
            .ok_or_else(|| harness(format!("{path}: truncated")))
    };
    let n = int(path, num()?)?;
    let m = int(path, num()?)?;
    let p = int(path, num()?)?;
    let q = int(path, num()?)?;
    let rat = num()? != 0.0;
    let mut net = Vec::with_capacity(n + 1);
    for _ in 0..=n {
        let mut row = Vec::with_capacity(m + 1);
        for _ in 0..=m {
            let (x, y, z) = (num()?, num()?, num()?);
            let w = if rat { num()? } else { NOW };
            row.push([x, y, z, w]);
        }
        net.push(row);
    }
    let u = (0..n + p + 2)
        .map(|_| num())
        .collect::<Result<Vec<_>, _>>()?;
    let v = (0..m + q + 2)
        .map(|_| num())
        .collect::<Result<Vec<_>, _>>()?;
    Ok(InlineSrf { net, p, q, u, v })
}

/// Reads an upstream `.smc` text file (an `SmBSplineCurve::WriteToDB` record, version <=
/// 32): `n`, `p`, rational flag, dimension, `n+1` points (`x y [z] [w]`), `n+p+2` knots.
fn read_smc(path: &str) -> Result<InlineCrv, HarnessError> {
    let v = numbers(path)?;
    let mut it = v.into_iter();
    let mut num = || {
        it.next()
            .ok_or_else(|| harness(format!("{path}: truncated")))
    };
    let n = int(path, num()?)?;
    let p = int(path, num()?)?;
    let rat = num()? != 0.0;
    let dim = int(path, num()?)? as u64;
    let mut cpts = Vec::with_capacity(n + 1);
    for _ in 0..=n {
        let x = num()?;
        let y = num()?;
        let z = if dim == 2 { 0.0 } else { num()? };
        let w = if rat { num()? } else { NOW };
        cpts.push([x, y, z, w]);
    }
    let u = (0..n + p + 2)
        .map(|_| num())
        .collect::<Result<Vec<_>, _>>()?;
    Ok(InlineCrv { dim, cpts, p, u })
}

fn srf(s: &SrfIn) -> Result<InlineSrf, HarnessError> {
    match s {
        SrfIn::File { sms } => read_sms(sms),
        SrfIn::Inline(i) => Ok(i.clone()),
    }
}

fn crv(c: &CrvIn) -> Result<InlineCrv, HarnessError> {
    match c {
        CrvIn::File { smc } => read_smc(smc),
        CrvIn::Inline(i) => Ok(i.clone()),
    }
}

fn shape(s: &InlineSrf) -> Result<(usize, usize), HarnessError> {
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

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn record_of(op: &OpIn) -> Result<CbsRecord, HarnessError> {
    let OpIn::ReadDb {
        implicit,
        basis_type,
        srf: s,
        c3d,
        uv_header_type,
        cuv,
        ..
    } = op
    else {
        return Err(harness("not a read op"));
    };
    let s = srf(s)?;
    let rc = |c: &CrvIn| -> Result<RecordCurve, HarnessError> {
        let c = crv(c)?;
        Ok(RecordCurve {
            dim: c.dim,
            p: c.p,
            cpts: c.cpts,
            knots: c.u,
        })
    };
    Ok(CbsRecord {
        implicit: *implicit,
        basis_type: *basis_type,
        basis: RecordSurface {
            pq: (s.p, s.q),
            net: s.net,
            u: s.u,
            v: s.v,
        },
        curves_3d: c3d.iter().map(rc).collect::<Result<_, _>>()?,
        uv_header_type: *uv_header_type,
        curves_uv: cuv.iter().map(rc).collect::<Result<_, _>>()?,
    })
}

// ---- C++ side -----------------------------------------------------------------------

fn push_srf(o: &mut Vec<f64>, s: &InlineSrf) -> Result<(), HarnessError> {
    let (n, m) = shape(s)?;
    o.extend(
        [
            n,
            m,
            s.p,
            s.q,
            s.u.len().saturating_sub(1),
            s.v.len().saturating_sub(1),
        ]
        .map(|x| x as f64),
    );
    for row in &s.net {
        for p in row {
            o.extend_from_slice(p);
        }
    }
    o.extend_from_slice(&s.u);
    o.extend_from_slice(&s.v);
    Ok(())
}

fn push_crv(o: &mut Vec<f64>, c: &CrvIn) -> Result<(), HarnessError> {
    let c = crv(c)?;
    let n = c.cpts.len().saturating_sub(1);
    o.extend([
        c.dim as f64,
        n as f64,
        c.p as f64,
        c.u.len().saturating_sub(1) as f64,
    ]);
    for p in &c.cpts {
        o.extend_from_slice(p);
    }
    o.extend_from_slice(&c.u);
    Ok(())
}

fn push_cbs(o: &mut Vec<f64>, b: &CbsIn) -> Result<(), HarnessError> {
    o.push(flag(b.oob));
    o.push(f64::from(b.form));
    push_srf(o, &srf(&b.srf)?)?;
    o.push(flag(b.implicit));
    o.push(b.c3d.len() as f64);
    for c in &b.c3d {
        push_crv(o, c)?;
    }
    o.push(b.cuv.len() as f64);
    for c in &b.cuv {
        push_crv(o, c)?;
    }
    Ok(())
}

fn push_derivs(o: &mut Vec<f64>, uv: [f64; 2], hu: usize, hv: usize, flags: [bool; 3]) {
    o.extend_from_slice(&uv);
    o.extend([hu as f64, hv as f64]);
    o.extend(flags.map(flag));
}

/// The ffi input of a surface op.
fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let this = i
        .this
        .as_ref()
        .ok_or_else(|| harness("surface op without this"))?;
    let mut o = vec![1.0];
    push_cbs(&mut o, this)?;
    match &i.op {
        OpIn::State | OpIn::EqSelf | OpIn::SetContext => {}
        OpIn::Eval {
            uv,
            hu,
            hv,
            u_left,
            v_left,
            upper,
            nzt,
        } => {
            push_derivs(&mut o, *uv, *hu, *hv, [*u_left, *v_left, *upper]);
            o.push(flag(*nzt));
        }
        OpIn::EvalPt { uv } | OpIn::StepPt { uv } => o.extend_from_slice(uv),
        OpIn::Queries { types } => {
            o.push(types.len() as f64);
            o.extend(types.iter().map(|&t| t as f64));
        }
        OpIn::Defaults { v } => o.extend_from_slice(v),
        OpIn::EqOther { other } => push_cbs(&mut o, other)?,
        OpIn::Normal { uv, u_left, v_left } | OpIn::Geometric { uv, u_left, v_left } => {
            o.extend_from_slice(uv);
            o.extend([flag(*u_left), flag(*v_left)]);
        }
        OpIn::Simple {
            uv,
            hu,
            hv,
            u_left,
            v_left,
            upper,
        }
        | OpIn::Step {
            uv,
            hu,
            hv,
            u_left,
            v_left,
            upper,
        } => push_derivs(&mut o, *uv, *hu, *hv, [*u_left, *v_left, *upper]),
        OpIn::Singular { uv, tol } => {
            o.extend_from_slice(uv);
            o.push(*tol);
        }
        OpIn::NormalSection {
            uv,
            u_left,
            v_left,
            dir,
        } => {
            o.extend_from_slice(uv);
            o.extend([flag(*u_left), flag(*v_left)]);
            o.extend_from_slice(dir);
        }
        OpIn::ReadDb { .. } => return Err(harness("read_db is static")),
    }
    Ok((i.op.ffi(), o))
}

fn to_output(flat: &[f64]) -> Output {
    let (status, rest) = flat
        .split_first()
        .map_or((f64::NAN, &[][..]), |(s, r)| (*s, r));
    Output::new()
        .values("status", vec![status])
        .values("out", rest.to_vec())
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let flat = if let OpIn::ReadDb { version, .. } = &i.op {
        ffi::read_db(cbs::TYPE, *version, &record_of(&i.op)?)
    } else {
        let (op, input) = cpp_input(i)?;
        ffi::eval(op, &input)
    }
    .map_err(|e| harness(e.to_string()))?;
    Ok(to_output(&flat))
}

/// Checks the inputs as the C++ side does (generators redraw rejected inputs).
fn validate(i: &Inputs) -> Result<(), HarnessError> {
    if let OpIn::ReadDb { version, .. } = &i.op {
        return ffi::validate_read_db(cbs::TYPE, *version, &record_of(&i.op)?)
            .map_err(|e| harness(e.to_string()));
    }
    let (op, input) = cpp_input(i)?;
    ffi::validate(op, &input).map_err(|e| harness(e.to_string()))
}

// ---- Rust side ----------------------------------------------------------------------

fn surface_of(s: &InlineSrf) -> Result<NlSurface, HarnessError> {
    let net = s
        .net
        .iter()
        .map(|r| {
            r.iter()
                .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                .collect()
        })
        .collect();
    NlSurface::new(net, s.p, s.q, s.u.clone(), s.v.clone())
        .map_err(|e| harness(format!("surface input: {e}")))
}

fn curve_of(c: &CrvIn) -> Result<SmCurve, HarnessError> {
    let c = crv(c)?;
    let pw = c
        .cpts
        .iter()
        .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
        .collect();
    let cur = Curve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve input: {e}")))?;
    Ok(SmCurve::from(BSplineCurve::from_nurb(cur, c.dim, false)))
}

fn cbs_of(b: &CbsIn) -> Result<Surface, HarnessError> {
    let mut s = BSplineSurface::from_nurb(surface_of(&srf(&b.srf)?)?, false);
    *s.out_of_bounds_enabled_mut() = b.oob;
    let form = BSplineSurfaceForm::from_code(u64::from(b.form))
        .ok_or_else(|| harness(format!("bad form {}", b.form)))?;
    s.set_b_spline_surface_form(form);
    let c3d = b.c3d.iter().map(curve_of).collect::<Result<_, _>>()?;
    let cuv = b.cuv.iter().map(curve_of).collect::<Result<_, _>>()?;
    Ok(Surface::from(CurveBoundedSurface::new(
        Surface::from(s),
        c3d,
        cuv,
        b.implicit,
    )))
}

fn p2(p: [f64; 2]) -> Point2 {
    Point2::new(p[0], p[1])
}

fn side(left: bool) -> Side {
    if left {
        Side::Left
    } else {
        Side::Right
    }
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

fn push3(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn push_dom(o: &mut Vec<f64>, d: &Extent2d) {
    o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
}

fn push_str(o: &mut Vec<f64>, s: &str) {
    o.push(s.len() as f64);
    o.extend(s.bytes().map(f64::from));
}

fn dump_bss(o: &mut Vec<f64>, s: &Surface) {
    let SurfaceKind::BSpline(s) = s.kind() else {
        o.push(0.0);
        return;
    };
    o.extend([
        1.0,
        flag(s.gw_nurb().is_some()),
        flag(s.is_out_of_bounds_enabled()),
        flag(s.is_nurb_borrowed()),
        f64::from(s.knot_type() as i32),
        f64::from(s.form() as i32),
    ]);
    if let Some(sur) = s.gw_nurb() {
        let ix = sur.indices();
        o.extend([ix.n, ix.m, ix.p, ix.q, ix.r, ix.s].map(|x| x as f64));
        for row in sur.rows() {
            for p in row {
                o.extend([p.x, p.y, p.z, p.w]);
            }
        }
        o.extend_from_slice(sur.u_knots());
        o.extend_from_slice(sur.v_knots());
    }
}

/// `put_curve`: type, dimension, interval, five points, then the B-spline data.
fn dump_curve(k: &Kernel, o: &mut Vec<f64>, c: &SmCurve) {
    o.push(c.type_code() as f64);
    o.push(c.dim() as f64);
    let ivl = c.natural_interval();
    let (a, b) = (ivl.min(), ivl.max());
    o.extend([a, b]);
    for i in 0..=4 {
        let t = a + (b - a) * (f64::from(i) / 4.0);
        match c.evaluate_point(k, t) {
            Ok(p) => {
                o.push(0.0);
                push3(o, p);
            }
            Err(e) => {
                o.push(status(&e));
                o.extend([0.0; 3]);
            }
        }
    }
    let CurveKind::BSpline(bc) = c.kind() else {
        return;
    };
    o.push(flag(bc.gw_nurb().is_some()));
    if let Some(cur) = bc.gw_nurb() {
        o.extend([cur.n() as f64, cur.degree() as f64, cur.m() as f64]);
        for p in cur.cpts() {
            o.extend([p.x, p.y, p.z, p.w]);
        }
        o.extend_from_slice(cur.knots());
    }
}

/// `put_state`: the type, then for a curve bounded surface its members.
fn dump_state(k: &Kernel, o: &mut Vec<f64>, s: &Surface) {
    o.push(s.type_code() as f64);
    let SurfaceKind::CurveBounded(c) = s.kind() else {
        return;
    };
    o.push(flag(c.implicit_outer()));
    dump_bss(o, c.basis());
    for cs in [c.boundaries_3d(), c.boundaries_uv()] {
        o.push(cs.len() as f64);
        for x in cs {
            dump_curve(k, o, x);
        }
    }
}

/// The outcome of a Rust op: the record, or an operation error (its status is the
/// record unless it is unported).
type Run = Result<Vec<f64>, SolidError>;

/// Runs a fallible op whose record on failure is only its status.
fn record(f: impl FnOnce(&mut Vec<f64>) -> solidcore::Result<()>) -> Run {
    let mut o = vec![0.0];
    f(&mut o)?;
    Ok(o)
}

fn push_matrix(
    o: &mut Vec<f64>,
    d: &[Vec<Vec3>],
    hu: usize,
    hv: usize,
    upper: bool,
) -> solidcore::Result<()> {
    for i in 0..=hu {
        for j in 0..=hv {
            if !upper || i + j <= hu {
                let v = d
                    .get(i)
                    .and_then(|r| r.get(j))
                    .copied()
                    .ok_or_else(|| SolidError::sm_err("difftest: short matrix"))?;
                push3(o, v);
            }
        }
    }
    Ok(())
}

/// The status of a call that only reports one (unported errors propagate).
fn st<T>(r: solidcore::Result<T>) -> solidcore::Result<f64> {
    match r {
        Ok(_) => Ok(0.0),
        Err(e) if e.is_unported() => Err(e),
        Err(e) => Ok(status(&e)),
    }
}

fn the_cbs(s: &Surface) -> solidcore::Result<&CurveBoundedSurface> {
    match s.kind() {
        SurfaceKind::CurveBounded(c) => Ok(c),
        SurfaceKind::BSpline(_)
        | SurfaceKind::Plane(_)
        | SurfaceKind::Cone(_)
        | SurfaceKind::SurfOfRevolution(_)
        | SurfaceKind::Step(_)
        | SurfaceKind::OffsetSurface(_)
        | SurfaceKind::SrfInVolume(_) => {
            Err(SolidError::sm_err("difftest: not a curve bounded surface"))
        }
    }
}

fn rust_surface_op(k: &Kernel, op: &OpIn, this: &CbsIn) -> Result<Run, HarnessError> {
    let mut s = cbs_of(this)?;
    let run = match op {
        OpIn::State => record(|o| {
            dump_state(k, o, &s);
            Ok(())
        }),
        OpIn::Eval {
            uv,
            hu,
            hv,
            u_left,
            v_left,
            upper,
            nzt,
        } => record(|o| {
            let flags = EvaluateFlags {
                u_side: side(*u_left),
                v_side: side(*v_left),
                only_upper_half: *upper,
                non_zero_tangents: *nzt,
            };
            let d = s.evaluate(k, p2(*uv), *hu, *hv, flags)?;
            push_matrix(o, &d, *hu, *hv, *upper)
        }),
        OpIn::EvalPt { uv } => record(|o| {
            push3(o, s.evaluate_point(k, p2(*uv))?);
            Ok(())
        }),
        OpIn::Queries { types } => record(|o| {
            push_dom(o, &s.natural_uv_domain());
            push_dom(o, &s.step_uv_domain());
            o.push(s.degree(SurfParamType::U) as f64);
            o.push(s.degree(SurfParamType::V) as f64);
            o.push(s.number_control_points(SurfParamType::U) as f64);
            o.push(s.number_control_points(SurfParamType::V) as f64);
            o.push(flag(s.is_bilinear()));
            o.push(s.type_code() as f64);
            for &t in types {
                o.push(flag(s.is_kind_of(t)?));
            }
            push_str(o, s.type_string());
            push_str(o, s.class_string());
            Ok(())
        }),
        OpIn::Defaults { v } => {
            let v = Vec3::new(v[0], v[1], v[2]);
            let mut o = vec![0.0];
            let r = (|| -> solidcore::Result<()> {
                o.push(st(s.knots(SurfParamType::U, None))?);
                o.push(flag(s.is_parallel_to_vector(v)?));
                push_dom(&mut o, &s.max_analytic_domain(k)?);
                o.push(st(s.copy(k))?);
                o.push(st(s.copy_and_add_analytics(k))?);
                o.push(st(s.split_at(k, 0.5, SurfParamType::U))?);
                o.push(st(s.reverse(&Kernel::default(), SurfParamType::U))?);
                o.push(st(s.swap_uv(&Kernel::default()))?);
                dump_state(k, &mut o, &s);
                Ok(())
            })();
            r.map(|()| o)
        }
        OpIn::EqSelf => record(|o| {
            o.push(flag(the_cbs(&s)?.approx_eq(k, &s)?));
            Ok(())
        }),
        OpIn::EqOther { other } => {
            let other = cbs_of(other)?;
            record(|o| {
                o.push(flag(the_cbs(&s)?.approx_eq(k, &other)?));
                Ok(())
            })
        }
        OpIn::Normal { uv, u_left, v_left } => record(|o| {
            push3(
                o,
                the_cbs(&s)?.evaluate_normal(k, p2(*uv), side(*u_left), side(*v_left))?,
            );
            Ok(())
        }),
        OpIn::Simple {
            uv,
            hu,
            hv,
            u_left,
            v_left,
            upper,
        } => record(|o| {
            let d = the_cbs(&s)?.evaluate_simple(
                k, p2(*uv),
                *hu,
                *hv,
                side(*u_left),
                side(*v_left),
                *upper,
            )?;
            push_matrix(o, &d, *hu, *hv, *upper)
        }),
        OpIn::Step {
            uv,
            hu,
            hv,
            u_left,
            v_left,
            upper,
        } => record(|o| {
            let d = the_cbs(&s)?.evaluate_step(
                k, p2(*uv),
                *hu,
                *hv,
                side(*u_left),
                side(*v_left),
                *upper,
            )?;
            push_matrix(o, &d, *hu, *hv, *upper)
        }),
        OpIn::Singular { uv, tol } => record(|o| {
            push3(
                o,
                the_cbs(&s)?.evaluate_normal_at_singularity(&Kernel::default(), p2(*uv), *tol)?,
            );
            Ok(())
        }),
        OpIn::Geometric { uv, u_left, v_left } => record(|o| {
            let g = the_cbs(&s)?.evaluate_geometric(k, p2(*uv), side(*u_left), side(*v_left))?;
            o.extend([
                g.gaussian_curvature,
                g.normal_curvature,
                g.principle_curvature1,
                g.principle_curvature2,
            ]);
            push3(o, g.efg_of_first_fund_form);
            push3(o, g.lmn_of_second_fund_form);
            push3(o, g.principle_curvature_vector1);
            push3(o, g.principle_curvature_vector2);
            Ok(())
        }),
        OpIn::NormalSection {
            uv,
            u_left,
            v_left,
            dir,
        } => record(|o| {
            let n = the_cbs(&s)?.evaluate_normal_section(
                k, p2(*uv),
                side(*u_left),
                side(*v_left),
                Vec3::new(dir[0], dir[1], dir[2]),
            )?;
            push3(o, n.tangent_plane_direction);
            o.push(n.curvature);
            push3(o, n.normal);
            Ok(())
        }),
        OpIn::StepPt { uv } => record(|o| {
            push3(o, the_cbs(&s)?.evaluate_step_point(k, p2(*uv))?);
            Ok(())
        }),
        OpIn::SetContext => record(|o| {
            the_cbs(&s)?.set_context();
            dump_state(k, o, &s);
            Ok(())
        }),
        OpIn::ReadDb { .. } => return Err(harness("read_db is static")),
    };
    Ok(run)
}

fn rust_read(op: &OpIn) -> Result<Run, HarnessError> {
    let OpIn::ReadDb { version, .. } = op else {
        return Err(harness("not a read op"));
    };
    let text = ffi::cbs_record(&record_of(op)?);
    let mut db = DatabaseReader::new();
    db.open_for_read(text.into_bytes(), FileType::Ascii);
    let k = Kernel::default();
    Ok(record(|o| {
        let s = Surface::read_from_db(&k, cbs::TYPE, &mut db, *version)?;
        dump_state(&k, o, &s);
        Ok(())
    }))
}

fn rust(k: &Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    let run = match (&i.op, &i.this) {
        (OpIn::ReadDb { .. }, _) => rust_read(&i.op)?,
        (op, Some(t)) => rust_surface_op(k, op, t)?,
        (_, None) => return Err(harness("surface op without this")),
    };
    Ok(match run {
        Ok(o) => to_output(&o),
        Err(e) => Output::unported(&e).unwrap_or_else(|| to_output(&[status(&e)])),
    })
}

// ---- generated cases ----------------------------------------------------------------

fn gen_knots(rng: &mut Rng, p: usize, n: usize) -> Vec<f64> {
    let interior = n - p;
    let mut inner: Vec<f64> = (0..interior).map(|_| rng.uniform(0.05, 0.95)).collect();
    if rng.chance(0.2) && interior >= 2 {
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

fn gen_srf(rng: &mut Rng) -> InlineSrf {
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let n = p + rng.below(3) as usize;
    let m = q + rng.below(3) as usize;
    let amp = rng.uniform(0.0, 1.0);
    let rational = rng.chance(0.2);
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let x = i as f64 + rng.uniform(-0.2, 0.2);
                    let y = j as f64 + rng.uniform(-0.2, 0.2);
                    let z = amp * rng.uniform(-1.0, 1.0);
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
    InlineSrf {
        net,
        p,
        q,
        u: gen_knots(rng, p, n),
        v: gen_knots(rng, q, m),
    }
}

fn gen_curve(rng: &mut Rng, dim: u64) -> CrvIn {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(4) as usize;
    let rational = rng.chance(0.15);
    let cpts = (0..=n)
        .map(|i| {
            let x = i as f64 + rng.uniform(-0.3, 0.3);
            let y = rng.uniform(-1.0, 1.0);
            let z = if dim == 2 {
                0.0
            } else {
                rng.uniform(-1.0, 1.0)
            };
            if rational {
                let w = rng.uniform(0.5, 2.0);
                [x * w, y * w, z * w, w]
            } else {
                [x, y, z, NOW]
            }
        })
        .collect();
    CrvIn::Inline(InlineCrv {
        dim,
        cpts,
        p,
        u: gen_knots(rng, p, n),
    })
}

fn gen_curves(rng: &mut Rng, dim: u64) -> Vec<CrvIn> {
    let k = rng.below(4) as usize;
    (0..k).map(|_| gen_curve(rng, dim)).collect()
}

fn gen_cbs(rng: &mut Rng) -> CbsIn {
    CbsIn {
        oob: rng.chance(0.1),
        form: if rng.chance(0.85) {
            11
        } else {
            rng.below(10) as u8
        },
        srf: SrfIn::Inline(gen_srf(rng)),
        implicit: rng.chance(0.5),
        c3d: gen_curves(rng, 3),
        cuv: gen_curves(rng, 2),
    }
}

fn domain_of(b: &CbsIn) -> [f64; 4] {
    let s = srf(&b.srf).ok();
    let r = |k: Option<&Vec<f64>>| {
        k.map_or((0.0, 1.0), |k| {
            (
                k.first().copied().unwrap_or(0.0),
                k.last().copied().unwrap_or(1.0),
            )
        })
    };
    let (u0, u1) = r(s.as_ref().map(|s| &s.u));
    let (v0, v1) = r(s.as_ref().map(|s| &s.v));
    [u0, v0, u1, v1]
}

fn gen_uv(rng: &mut Rng, d: [f64; 4]) -> [f64; 2] {
    let pick = |rng: &mut Rng, a: f64, b: f64| {
        if rng.chance(0.15) {
            if rng.chance(0.5) {
                a
            } else {
                b
            }
        } else {
            rng.uniform(a, b)
        }
    };
    [pick(rng, d[0], d[2]), pick(rng, d[1], d[3])]
}

fn gen_case(rng: &mut Rng) -> Inputs {
    if rng.chance(0.2) {
        let c = gen_cbs(rng);
        let c3d = if rng.chance(0.15) { Vec::new() } else { c.c3d };
        return Inputs {
            this: None,
            op: OpIn::ReadDb {
                version: 30,
                implicit: c.implicit,
                basis_type: if rng.chance(0.1) {
                    ffi::B_SPLINE_CURVE_TYPE
                } else {
                    ffi::B_SPLINE_SURFACE_TYPE
                },
                srf: c.srf,
                c3d,
                uv_header_type: if rng.chance(0.3) {
                    14010
                } else {
                    ffi::B_SPLINE_CURVE_TYPE
                },
                cuv: c.cuv,
            },
        };
    }
    let this = gen_cbs(rng);
    let d = domain_of(&this);
    let uv = gen_uv(rng, d);
    let hu = rng.below(3) as usize;
    let upper = rng.chance(0.3);
    let hv = if upper { hu } else { rng.below(3) as usize };
    let (ul, vl) = (rng.chance(0.5), rng.chance(0.5));
    let op = match rng.below(16) {
        0 => OpIn::State,
        1..=3 => OpIn::Eval {
            uv,
            hu,
            hv,
            u_left: ul,
            v_left: vl,
            upper,
            nzt: rng.chance(0.7),
        },
        4 | 5 => OpIn::EvalPt { uv },
        6 => OpIn::Queries {
            types: vec![
                cbs::TYPE,
                solidcore::surface::surface::p2::TYPE,
                BSplineSurface::TYPE,
                rng.below(20000) as i64,
            ],
        },
        7 => OpIn::Defaults {
            v: rng.point3(-1.0, 1.0),
        },
        8 => OpIn::EqSelf,
        9 => OpIn::EqOther {
            other: if rng.chance(0.5) {
                this.clone()
            } else {
                gen_cbs(rng)
            },
        },
        10 => OpIn::Normal {
            uv,
            u_left: ul,
            v_left: vl,
        },
        11 => OpIn::Simple {
            uv,
            hu,
            hv,
            u_left: ul,
            v_left: vl,
            upper,
        },
        12 => OpIn::Geometric {
            uv,
            u_left: ul,
            v_left: vl,
        },
        13 => OpIn::NormalSection {
            uv,
            u_left: ul,
            v_left: vl,
            dir: rng.point3(-1.0, 1.0),
        },
        14 => OpIn::StepPt { uv },
        _ => OpIn::SetContext,
    };
    Inputs {
        this: Some(this),
        op,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.curve_bounded_surface"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let i = gen_case(rng);
            if validate(&i).is_ok() {
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

    /// The generators reach the ops with successful outcomes on the Rust side.
    #[test]
    fn generated_cases_cover_ops() {
        let k = &Kernel::default();
        let mut tally: BTreeMap<(String, String), usize> = BTreeMap::new();
        for seed in 0..1000u64 {
            let mut rng = Rng::new(0x5eed_0000 + seed);
            let i = gen_case(&mut rng);
            if validate(&i).is_err() {
                continue;
            }
            let name = format!("{:?}", i.op.ffi());
            let out = rust(k, &i).expect("harness");
            let kind = if let Some(d) = &out.deferred {
                format!("deferred:{}", d.unit)
            } else {
                let st = out.values.get("status").and_then(|v| v.first()).copied();
                format!("status:{}", st.unwrap_or(f64::NAN))
            };
            *tally.entry((name, kind)).or_default() += 1;
        }
        for (k, v) in &tally {
            println!("{k:?}: {v}");
        }
        for op in [
            "State",
            "Eval",
            "EvalPt",
            "Queries",
            "Defaults",
            "EqSelf",
            "SetContext",
            "ReadDb",
        ] {
            assert!(
                tally.keys().any(|(n, k)| n == op && k == "status:0"),
                "{op} never succeeds"
            );
        }
    }
}
