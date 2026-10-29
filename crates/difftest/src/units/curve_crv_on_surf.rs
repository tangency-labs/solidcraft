//! Runner for unit `curve.crv_on_surf`: `SmCrvOnSurf`, a 2D UV curve traced through a
//! surface.
//!
//! C++: `solidcore_ffi::units::curve_crv_on_surf` (upstream
//! `source/SMLib/src/SmCrvOnSurf.cpp` on objects built from a 2D `SmBSplineCurve` UV
//! curve, or the UV-line constructor, over a B-spline base or a real
//! `SmOffsetSurface`). Rust: `solidcore::curve::crv_on_surf` (the `&mut` methods, so
//! the caches evolve as upstream's; `Info` and `Equal` go through the [`Curve`] dispatch).
//!
//! A case is either an object (`cos`: `uv` (`bspline` or `line`), B-spline `srf` (inline
//! or an upstream `.sms` file), optional `offset`, `domain`, `owner`, `cache` `{need, breaks}`)
//! with an op list run in order on it, or a database record (`read_db`). Inline UV
//! B-splines and surfaces accept `borrow_nurb` to preserve raw knots.
//! Both sides produce the flat record documented in the ffi module (every op's status,
//! `0` = `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a
//! Rust error upstream cannot produce, then its outputs), compared as `out`.
//!
//! Knot and drop operations compare the break-cache state as well as their answers:
//! a local solve may leave breaks uncached, while a later global solve populates
//! them through the base curve's virtual knot queries. Every run has at least
//! [`MIN_GENERATED`] generated cases; generated inputs the ffi wrapper rejects are
//! redrawn. Any genuinely unported downstream call still defers the whole case.
//!
//! Deviations: the runner-local SMS reader validates counts below the exclusive
//! `2^usize::BITS` bound, checks dimension and payload arithmetic and the available
//! tokens before constructing the net, and grows storage incrementally. Valid point
//! order, weights, knots, lazy float parsing and path-bearing errors are unchanged.

#[path = "fixtures/crv_on_surf_sms.rs"]
mod sms;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::crv_on_surf::CrvOnSurf;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::global_solver::SolverOperation;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BoundingBoxRequest};
use solidcore::nurbs::b_spline_curve_n_lib::TrimMode;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::srf_basic::Surface;
use solidcore::surface::offset_surface::{OffsetSurface, SS_UNKNOWN};
use solidcore::surface::surface::{Surface as SmSurface, SurfaceKind};
use solidcore::topology::database_io::{DatabaseReader, FileType, InputStream};
use solidcore::SolidError;
use solidcore_ffi::units::curve_crv_on_surf::{self as ffi, ObjOp, Op, NOW, NOZ};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `curve.crv_on_surf` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

/// A 2D B-spline UV curve: control points `[x, y, w]` (`w` = `NOW` when
/// non-rational; `x`, `y` homogeneous otherwise), degree, knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct UvBspline {
    cpts: Vec<[f64; 3]>,
    p: usize,
    u: Vec<f64>,
    /// Borrow the supplied NURB without constructor knot repair on both sides.
    #[serde(default)]
    borrow_nurb: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum UvIn {
    Bspline(UvBspline),
    /// The UV-line constructor `[x0, y0, x1, y1]`.
    Line([f64; 4]),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineSrf {
    /// Control net `[x, y, z, w]` (`w` = `NOW` when non-rational), `net[i][j]`, `i` in u.
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
    /// Borrow the supplied NURB without constructor knot repair on both sides.
    #[serde(default)]
    borrow_nurb: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum SrfIn {
    /// An upstream `.sms` surface file (path relative to the repository root).
    File {
        sms: String,
    },
    Inline(InlineSrf),
}

/// A real `SmOffsetSurface` wrapped around the B-spline `srf`; ownership controls
/// whether the wrapper deletes its base.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OffsetIn {
    distance: f64,
    #[serde(default)]
    owns_base: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheIn {
    need: bool,
    breaks: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CosIn {
    uv: UvIn,
    srf: SrfIn,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    offset: Option<OffsetIn>,
    domain: Option<[f64; 4]>,
    #[serde(default)]
    owner: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache: Option<CacheIn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Info {
        types: Vec<i64>,
    },
    State,
    Eval {
        t: f64,
        n: usize,
        left: bool,
        nonzero: bool,
    },
    EvalPoint {
        t: f64,
    },
    EvalUv {
        t: f64,
    },
    EvalNormal {
        t: f64,
        left: bool,
    },
    Eval1st {
        t: f64,
        n: usize,
        left: bool,
    },
    Knots {
        ivl: Option<[f64; 2]>,
        want: bool,
    },
    Nnk,
    Conts {
        angle: f64,
    },
    Equal {
        other: Option<Box<CosIn>>,
    },
    Copy,
    CopyEval {
        t: f64,
        n: usize,
        left: bool,
        nonzero: bool,
    },
    EditParam {
        a: f64,
        b: f64,
    },
    Reverse {
        a: f64,
        b: f64,
    },
    Trim {
        a: f64,
        b: f64,
        mode: u64,
    },
    Transform {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: Option<[f64; 3]>,
    },
    Approx {
        breaks: Vec<f64>,
        tol: f64,
        #[serde(rename = "match")]
        match_param: bool,
    },
    GetOrCreate {
        tol: f64,
        breaks: Option<Vec<f64>>,
        #[serde(rename = "match")]
        match_param: Option<bool>,
    },
    Bbox {
        a: f64,
        b: f64,
    },
    Drop {
        a: f64,
        b: f64,
        point: [f64; 3],
        vec: Option<[f64; 3]>,
        tol: f64,
        guess: Option<f64>,
        solver_op: i32,
    },
    Gap {
        other: Option<Box<CosIn>>,
        a: f64,
        b: f64,
        s0: f64,
        s1: f64,
        samples: usize,
        tol: f64,
        max: Option<f64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReadDbIn {
    binary: bool,
    version: u64,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum Inputs {
    Object { cos: Box<CosIn>, ops: Vec<OpIn> },
    ReadDb { read_db: ReadDbIn },
}

// ---- surfaces ---------------------------------------------------------------------

/// Reads an upstream `.sms` text file: `n m`, `p q`, rational flag, `(n+1)(m+1)`
/// points (`x y z`, or `wx wy wz w` when rational, v index fastest), `n+p+2` u knots,
/// `m+q+2` v knots.
fn read_sms(path: &str) -> Result<InlineSrf, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let s = sms::parse_sms(&text, path)?;
    Ok(InlineSrf {
        net: s.net,
        p: s.p,
        q: s.q,
        u: s.u,
        v: s.v,
        borrow_nurb: false,
    })
}

fn resolve(s: &SrfIn) -> Result<InlineSrf, HarnessError> {
    match s {
        SrfIn::File { sms } => read_sms(sms),
        SrfIn::Inline(i) => Ok(i.clone()),
    }
}

fn srf_domain(s: &InlineSrf) -> [f64; 4] {
    [
        s.u.first().copied().unwrap_or(0.0),
        s.v.first().copied().unwrap_or(0.0),
        s.u.last().copied().unwrap_or(1.0),
        s.v.last().copied().unwrap_or(1.0),
    ]
}

// ---- C++ input --------------------------------------------------------------------

fn b(x: bool) -> f64 {
    f64::from(u8::from(x))
}

fn cos_block(v: &mut Vec<f64>, c: &CosIn) -> Result<(), HarnessError> {
    match &c.uv {
        UvIn::Bspline(bs) => {
            let n = bs.cpts.len().saturating_sub(1);
            let m = bs.u.len().saturating_sub(1);
            v.extend([0.0, b(bs.borrow_nurb), n as f64, bs.p as f64, m as f64]);
            for &[x, y, w] in &bs.cpts {
                v.extend([x, y, NOZ, w]);
            }
            v.extend_from_slice(&bs.u);
        }
        UvIn::Line(l) => {
            v.push(1.0);
            v.extend_from_slice(l);
        }
    }
    v.push(b(c.offset.is_some()));
    let s = resolve(&c.srf)?;
    v.push(b(s.borrow_nurb));
    let n = s.net.len().saturating_sub(1);
    let m = s.net.first().map_or(0, Vec::len).saturating_sub(1);
    v.extend([
        n as f64,
        m as f64,
        s.p as f64,
        s.q as f64,
        s.u.len().saturating_sub(1) as f64,
        s.v.len().saturating_sub(1) as f64,
    ]);
    for c in s.net.iter().flatten() {
        v.extend_from_slice(c);
    }
    v.extend_from_slice(&s.u);
    v.extend_from_slice(&s.v);
    if let Some(offset) = &c.offset {
        v.extend([offset.distance, b(offset.owns_base)]);
    }
    v.push(b(c.domain.is_some()));
    v.extend_from_slice(&c.domain.unwrap_or([0.0; 4]));
    v.push(c.owner as f64);
    match &c.cache {
        None => v.push(0.0),
        Some(k) => {
            v.extend([1.0, b(k.need), k.breaks.len() as f64]);
            v.extend_from_slice(&k.breaks);
        }
    }
    Ok(())
}

fn obj_op_of(op: &OpIn) -> ObjOp {
    match op {
        OpIn::Info { .. } => ObjOp::Info,
        OpIn::State => ObjOp::State,
        OpIn::Eval { .. } => ObjOp::Eval,
        OpIn::EvalPoint { .. } => ObjOp::EvalPoint,
        OpIn::EvalUv { .. } => ObjOp::EvalUv,
        OpIn::EvalNormal { .. } => ObjOp::EvalNormal,
        OpIn::Eval1st { .. } => ObjOp::Eval1st,
        OpIn::Knots { .. } => ObjOp::Knots,
        OpIn::Nnk => ObjOp::Nnk,
        OpIn::Conts { .. } => ObjOp::Conts,
        OpIn::Equal { .. } => ObjOp::Equal,
        OpIn::Copy => ObjOp::Copy,
        OpIn::CopyEval { .. } => ObjOp::CopyEval,
        OpIn::EditParam { .. } => ObjOp::EditParam,
        OpIn::Reverse { .. } => ObjOp::Reverse,
        OpIn::Trim { .. } => ObjOp::Trim,
        OpIn::Transform { .. } => ObjOp::Transform,
        OpIn::Approx { .. } => ObjOp::Approx,
        OpIn::GetOrCreate { .. } => ObjOp::GetOrCreate,
        OpIn::Bbox { .. } => ObjOp::Bbox,
        OpIn::Drop { .. } => ObjOp::Drop,
        OpIn::Gap { .. } => ObjOp::Gap,
    }
}

fn op_params(v: &mut Vec<f64>, op: &OpIn) -> Result<(), HarnessError> {
    match op {
        OpIn::Info { types } => {
            v.push(types.len() as f64);
            v.extend(types.iter().map(|&t| t as f64));
        }
        OpIn::State | OpIn::Nnk | OpIn::Copy => {}
        OpIn::Eval {
            t,
            n,
            left,
            nonzero,
        }
        | OpIn::CopyEval {
            t,
            n,
            left,
            nonzero,
        } => v.extend([*t, *n as f64, b(*left), b(*nonzero)]),
        OpIn::EvalPoint { t } | OpIn::EvalUv { t } => v.push(*t),
        OpIn::EvalNormal { t, left } => v.extend([*t, b(*left)]),
        OpIn::Eval1st { t, n, left } => v.extend([*t, *n as f64, b(*left)]),
        OpIn::Knots { ivl, want } => {
            let [x, y] = ivl.unwrap_or([0.0, 0.0]);
            v.extend([b(ivl.is_some()), x, y, b(*want)]);
        }
        OpIn::Conts { angle } => v.push(*angle),
        OpIn::Equal { other } => match other {
            None => v.push(1.0),
            Some(o) => {
                v.push(0.0);
                cos_block(v, o)?;
            }
        },
        OpIn::EditParam { a, b: bb } | OpIn::Reverse { a, b: bb } | OpIn::Bbox { a, b: bb } => {
            v.extend([*a, *bb]);
        }
        OpIn::Trim { a, b: bb, mode } => v.extend([*a, *bb, *mode as f64]),
        OpIn::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            v.extend_from_slice(origin);
            v.extend_from_slice(x);
            v.extend_from_slice(y);
            v.push(b(scale.is_some()));
            v.extend_from_slice(&scale.unwrap_or([1.0, 1.0, 1.0]));
        }
        OpIn::Approx {
            breaks,
            tol,
            match_param,
        } => {
            v.push(breaks.len() as f64);
            v.extend_from_slice(breaks);
            v.extend([*tol, b(*match_param)]);
        }
        OpIn::GetOrCreate {
            tol,
            breaks,
            match_param,
        } => {
            v.push(*tol);
            v.push(b(breaks.is_some()));
            let br = breaks.clone().unwrap_or_default();
            v.push(br.len() as f64);
            v.extend_from_slice(&br);
            v.push(b(match_param.is_some()));
            v.push(b(match_param.unwrap_or(false)));
        }
        OpIn::Drop {
            a,
            b: bb,
            point,
            vec,
            tol,
            guess,
            solver_op,
        } => {
            v.extend([*a, *bb]);
            v.extend_from_slice(point);
            v.push(b(vec.is_some()));
            v.extend_from_slice(&vec.unwrap_or([0.0; 3]));
            v.push(*tol);
            v.push(b(guess.is_some()));
            v.push(guess.unwrap_or(*a));
            v.push(f64::from(*solver_op));
        }
        OpIn::Gap {
            other,
            a,
            b: bb,
            s0,
            s1,
            samples,
            tol,
            max,
        } => {
            match other {
                None => v.push(1.0),
                Some(o) => {
                    v.push(0.0);
                    cos_block(v, o)?;
                }
            }
            v.extend([*a, *bb, *s0, *s1, *samples as f64, *tol]);
            v.push(b(max.is_some()));
            v.push(max.unwrap_or(0.0));
        }
    }
    Ok(())
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    match i {
        Inputs::Object { cos, ops } => {
            cos_block(&mut v, cos)?;
            v.push(ops.len() as f64);
            for op in ops {
                v.push(f64::from(obj_op_of(op).code()));
                op_params(&mut v, op)?;
            }
            Ok((Op::Object, v))
        }
        Inputs::ReadDb { read_db } => {
            v.extend([
                b(read_db.binary),
                read_db.version as f64,
                read_db.bytes.len() as f64,
            ]);
            v.extend(read_db.bytes.iter().map(|&c| f64::from(c)));
            Ok((Op::ReadDb, v))
        }
    }
}

fn to_output(flat: Vec<f64>) -> Output {
    Output::new().values("out", flat)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, input) = cpp_input(i)?;
    match ffi::eval(op, &input) {
        Ok(flat) => Ok(to_output(flat)),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side --------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

fn build_uv(bs: &UvBspline) -> Result<Curve, HarnessError> {
    let pw = bs
        .cpts
        .iter()
        .map(|&[x, y, w]| CPoint::new(x, y, NOZ, w))
        .collect();
    let cur =
        NurbCurve::new(pw, bs.p, bs.u.clone()).map_err(|e| harness(format!("uv curve: {e}")))?;
    Ok(Curve::from(BSplineCurve::from_nurb(cur, 2, bs.borrow_nurb)))
}

fn build_surface(
    k: &Kernel,
    s: &SrfIn,
    offset: Option<&OffsetIn>,
) -> Result<SmSurface, HarnessError> {
    let i = resolve(s)?;
    let net = i
        .net
        .iter()
        .map(|row| {
            row.iter()
                .map(|c| CPoint::new(c[0], c[1], c[2], c[3]))
                .collect()
        })
        .collect();
    let sur =
        Surface::new(net, i.p, i.q, i.u, i.v).map_err(|e| harness(format!("surface: {e}")))?;
    let base = SmSurface::from(BSplineSurface::from_nurb(sur, i.borrow_nurb));
    match offset {
        None => Ok(base),
        Some(offset) => OffsetSurface::new(k, offset.distance, base, offset.owns_base, SS_UNKNOWN)
            .map(SmSurface::from)
            .map_err(|e| harness(format!("offset surface: {e}"))),
    }
}

/// The object, or the error (status) its construction gives.
fn build(c: &CosIn) -> Result<solidcore::Result<CrvOnSurf>, HarnessError> {
    let k = Kernel::default();
    let srf = build_surface(&k, &c.srf, c.offset.as_ref())?;
    let domain = c
        .domain
        .map(|[a, bb, cc, d]| Extent2d::from_bounds(a, bb, cc, d));
    let r = match &c.uv {
        UvIn::Bspline(bs) => CrvOnSurf::new(&k, build_uv(bs)?, srf, domain, c.owner),
        UvIn::Line([x0, y0, x1, y1]) => CrvOnSurf::from_uv_line(
            &k,
            Point2::new(*x0, *y0),
            Point2::new(*x1, *y1),
            srf,
            domain,
            c.owner,
        ),
    };
    Ok(r.map(|mut cos| {
        if let Some(k) = &c.cache {
            cos.set_breaks_cache(k.need, k.breaks.clone());
        }
        cos
    }))
}

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_nurb(o: &mut Vec<f64>, bs: &BSplineCurve) {
    match bs.gw_nurb() {
        None => o.push(-1.0),
        Some(cur) => {
            o.extend([cur.n() as f64, cur.degree() as f64, cur.m() as f64]);
            for p in cur.cpts() {
                o.extend([p.x, p.y, p.z, p.w]);
            }
            o.extend_from_slice(cur.knots());
        }
    }
}

/// `type [dim nurb]` (B-splines and lines).
fn put_curve(o: &mut Vec<f64>, c: &Curve) {
    o.push(c.type_code() as f64);
    match c.kind() {
        CurveKind::BSpline(bs) => {
            o.push(bs.dim() as f64);
            put_nurb(o, bs);
        }
        CurveKind::Line(l) => {
            o.push(l.dim() as f64);
            put_nurb(o, l.base());
        }
        _ => {}
    }
}

fn put_surface(o: &mut Vec<f64>, s: &SmSurface) {
    o.push(s.type_code() as f64);
    match s.kind() {
        SurfaceKind::BSpline(s) => match s.gw_nurb() {
            None => o.push(-1.0),
            Some(sur) => {
                o.extend([sur.n() as f64, sur.m() as f64]);
                for row in sur.cpts() {
                    for p in row {
                        o.extend([p.x, p.y, p.z, p.w]);
                    }
                }
                o.extend_from_slice(sur.u_knots());
                o.extend_from_slice(sur.v_knots());
            }
        },
        SurfaceKind::OffsetSurface(s) => {
            o.extend([s.offset_distance(), b(s.owns_surface())]);
            put_surface(o, s.base_surface());
        }
        _ => {}
    }
}

fn put_state(o: &mut Vec<f64>, c: &CrvOnSurf) {
    o.push(c.owner_flag() as f64);
    let d = c.uv_domain();
    o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
    o.push(b(c.need_breaks()));
    let breaks = c.breaks();
    o.push(breaks.len() as f64);
    o.extend_from_slice(&breaks);
    o.push(c.bs_approx_max_gap_3d());
    o.push(b(c.parametrization_matches()));
    match c.bs_approx() {
        None => o.push(0.0),
        Some(a) => {
            o.push(1.0);
            put_nurb(o, &a);
        }
    }
    put_curve(o, c.uv_curve());
    put_surface(o, c.surface());
}

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// A deferral or a harness failure that ends the Rust run.
enum Stop {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Stop {
    fn from(e: HarnessError) -> Self {
        Stop::Harness(e)
    }
}

/// The status of `r` (deferring on `Unported`).
fn st<T>(r: &solidcore::Result<T>) -> Result<f64, Stop> {
    match r {
        Ok(_) => Ok(0.0),
        Err(e) => match Output::unported(e) {
            Some(d) => Err(Stop::Deferred(Box::new(d))),
            None => Ok(status(e)),
        },
    }
}

fn wrap(c: &CrvOnSurf) -> Curve {
    Curve::from(c.clone())
}

fn cos_of(c: Curve) -> Result<CrvOnSurf, HarnessError> {
    match c.into_kind() {
        CurveKind::CrvOnSurf(x) => Ok(*x),
        _ => Err(harness("not a curve on surface")),
    }
}

fn solver_op(code: i32) -> Result<SolverOperation, HarnessError> {
    SolverOperation::from_code(code).ok_or_else(|| harness("bad solver op"))
}

fn rust_op(o: &mut Vec<f64>, c: &mut CrvOnSurf, op: &OpIn, k: &Kernel) -> Result<(), Stop> {
    match op {
        OpIn::Info { types } => {
            let w = wrap(c);
            let deg = w.degree();
            o.push(st(&deg)?);
            let ivl = w.natural_interval();
            o.extend([
                w.type_code() as f64,
                w.dim() as f64,
                deg.unwrap_or(0) as f64,
                ivl.min(),
                ivl.max(),
                b(w.is_analytic()),
                b(w.is_bounded()),
            ]);
            o.extend(types.iter().map(|&t| b(w.is_kind_of(t))));
            put_state(o, c);
        }
        OpIn::State => {
            o.push(0.0);
            put_state(o, c);
        }
        OpIn::Eval {
            t,
            n,
            left,
            nonzero,
        } => {
            let r = c.evaluate(k, *t, *n, *left, *nonzero);
            o.push(st(&r)?);
            if let Ok(v) = r {
                for p in v {
                    pv(o, p);
                }
            }
        }
        OpIn::EvalPoint { t } => {
            let r = c.evaluate_point(k, *t);
            o.push(st(&r)?);
            if let Ok(p) = r {
                pv(o, p);
            }
        }
        OpIn::EvalUv { t } => {
            let r = c.evaluate_uv_point(k, *t);
            o.push(st(&r)?);
            if let Ok(p) = r {
                o.extend([p.x, p.y]);
            }
        }
        OpIn::EvalNormal { t, left } => {
            let r = c.evaluate_surface_normal(k, *t, *left);
            o.push(st(&r)?);
            if let Ok(s) = r {
                o.extend([s.uv.x, s.uv.y, s.uv_tangent.x, s.uv_tangent.y]);
                pv(o, s.normal);
            }
        }
        OpIn::Eval1st { t, left, .. } => {
            let r = c.evaluate_surface_1st_deriv_derivs(k, *t, *left);
            o.push(st(&r)?);
            if let Ok((du, dv)) = r {
                for (u, v) in du.iter().zip(&dv) {
                    pv(o, *u);
                    pv(o, *v);
                }
            }
        }
        OpIn::Knots { ivl, want } => {
            let iv = ivl.map(|[x, y]| Extent1d::from_min_max(x, y));
            // the status, then what GetKnots wrote (also on a failing status)
            let (kn, m) = match c.knots_with_status(k, iv.as_ref()) {
                Ok(w) => {
                    o.push(st(&w.status)?);
                    (w.knots, w.mults)
                }
                Err(e) => {
                    o.push(st(&Err::<(), _>(e))?);
                    (Vec::new(), Vec::new())
                }
            };
            o.push(kn.len() as f64);
            o.extend_from_slice(&kn);
            if *want {
                o.push(m.len() as f64);
                o.extend(m.iter().map(|&x| x as f64));
            }
            put_state(o, c);
        }
        OpIn::Nnk => {
            let r = c.number_natural_knots(k);
            st(&r)?;
            o.extend([0.0, r.unwrap_or(0) as f64]);
            put_state(o, c);
        }
        OpIn::Conts { angle } => {
            let r = c.calculate_continuities(k, *angle);
            o.push(st(&r)?);
            let (mn, cs) = r.unwrap_or((
                solidcore::geom::geom_utility::p2::ContinuityType::Undefined,
                Vec::new(),
            ));
            o.extend([f64::from(mn as i32), cs.len() as f64]);
            o.extend(cs.iter().map(|&x| f64::from(x as i32)));
            put_state(o, c);
        }
        OpIn::Equal { other } => match other {
            None => {
                let w = wrap(c);
                let r = w.approx_eq(k, &w);
                o.push(st(&r)?);
                o.push(b(r.unwrap_or(false)));
            }
            Some(oc) => {
                let d = match build(oc)? {
                    Ok(d) => d,
                    Err(e) => return Err(Stop::Harness(harness(format!("other: {e}")))),
                };
                let (wa, wb) = (wrap(c), Curve::from(d));
                let r1 = wa.approx_eq(k, &wb);
                let r2 = wb.approx_eq(k, &wa);
                st(&r1)?;
                st(&r2)?;
                o.extend([0.0, b(r1.unwrap_or(false)), b(r2.unwrap_or(false))]);
            }
        },
        OpIn::Copy => {
            let r = wrap(c).copy(k);
            o.push(st(&r)?);
            if let Ok(cp) = r {
                let eq = cp.approx_eq(k, &wrap(c));
                st(&eq)?;
                let cp = cos_of(cp)?;
                put_state(o, &cp);
                o.push(b(eq.unwrap_or(false)));
            }
        }
        OpIn::CopyEval {
            t,
            n,
            left,
            nonzero,
        } => {
            let r = wrap(c).copy(k);
            o.push(st(&r)?);
            if let Ok(cp) = r {
                let cp = cos_of(cp)?;
                put_state(o, &cp);
                let r = cp.evaluate(k, *t, *n, *left, *nonzero);
                o.push(st(&r)?);
                if let Ok(v) = r {
                    for p in v {
                        pv(o, p);
                    }
                }
            }
        }
        OpIn::EditParam { a, b: bb } => {
            let r = c.edit_parameterization(&Kernel::default(), &Extent1d::from_min_max(*a, *bb));
            o.push(st(&r)?);
            put_state(o, c);
        }
        OpIn::Reverse { a, b: bb } => {
            let r =
                c.reverse_parameterization(&Kernel::default(), &Extent1d::from_min_max(*a, *bb));
            o.push(st(&r)?);
            if let Ok(nv) = r {
                o.extend([nv.min(), nv.max()]);
            }
            put_state(o, c);
        }
        OpIn::Trim { a, b: bb, mode } => {
            let mode = match mode {
                0 => TrimMode::Notify,
                1 => TrimMode::Silent,
                _ => TrimMode::IntervalOnly,
            };
            let mut ivl = Extent1d::from_min_max(*a, *bb);
            let r = c.trim(k, &mut ivl, mode);
            o.push(st(&r)?);
            o.extend([ivl.min(), ivl.max()]);
            put_state(o, c);
        }
        OpIn::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            let ax = Axis2Placement::new(v3(*origin), v3(*x), v3(*y));
            let r = c.transform(k, &ax, scale.map(v3));
            o.push(st(&r)?);
            put_state(o, c);
        }
        OpIn::Approx {
            breaks,
            tol,
            match_param,
        } => {
            let r = c.approximate_curve(&Kernel::default(), breaks, *tol, *match_param);
            o.push(st(&r)?);
            match r {
                Ok((bs, gap)) => {
                    o.extend([gap, 1.0]);
                    put_curve(o, &Curve::from(bs));
                }
                Err(_) => o.extend([c.bs_approx_max_gap_3d(), 0.0]),
            }
            put_state(o, c);
        }
        OpIn::GetOrCreate {
            tol,
            breaks,
            match_param,
        } => {
            let r = c.get_or_create_bs_approx(
                &Kernel::default(),
                *tol,
                breaks.as_deref(),
                *match_param,
            );
            o.push(st(&r)?);
            match r {
                Ok(bs) => {
                    o.push(1.0);
                    put_curve(o, &Curve::from(bs));
                }
                Err(_) => o.push(0.0),
            }
            put_state(o, c);
        }
        OpIn::Bbox { a, b: bb } => {
            let req = BoundingBoxRequest {
                normal: true,
                pseudo: false,
                polar: false,
            };
            let r = c.calculate_bounding_box(
                &Kernel::default(),
                &Extent1d::from_min_max(*a, *bb),
                req,
                true,
            );
            o.push(st(&r)?);
            if let Ok(bx) = r {
                let n = bx.normal.ok_or_else(|| harness("no normal box"))?;
                pv(o, n.min());
                pv(o, n.max());
            }
            put_state(o, c);
        }
        OpIn::Drop {
            a,
            b: bb,
            point,
            vec,
            tol,
            guess,
            solver_op: sop,
        } => {
            let r = c.drop_point(
                k,
                &Extent1d::from_min_max(*a, *bb),
                v3(*point),
                vec.map(v3),
                *tol,
                *guess,
                solver_op(*sop)?,
            );
            o.push(st(&r)?);
            let d = r.unwrap_or_default();
            o.extend([b(d.success), d.parameter, d.distance]);
            put_state(o, c);
        }
        OpIn::Gap {
            other,
            a,
            b: bb,
            s0,
            s1,
            samples,
            tol,
            max,
        } => {
            // the FFI rejects more than 8 samples; upstream (and the port) would loop that
            // many times, so a larger count is not a case for Rust either
            if *samples > 8 {
                return Err(HarnessError::new(format!("{samples} samples")).into());
            }
            let ivl = Extent1d::from_min_max(*a, *bb);
            let run = |this: &CrvOnSurf, oc: &Curve| {
                this.surface_curve_max_distance_between(k, &ivl, oc, *s0, *s1, *samples, *tol, *max)
            };
            let r = match other {
                None => {
                    // the same object as the other curve
                    let w = Curve::from(c.clone());
                    match w.kind() {
                        CurveKind::CrvOnSurf(x) => run(x, &w),
                        _ => return Err(Stop::Harness(harness("not a curve on surface"))),
                    }
                }
                Some(oc) => {
                    let d = match build(oc)? {
                        Ok(d) => d,
                        Err(e) => return Err(Stop::Harness(harness(format!("other: {e}")))),
                    };
                    run(c, &Curve::from(d))
                }
            };
            o.push(st(&r)?);
            if let Ok(g) = r {
                o.extend([
                    g.max_dist,
                    g.max_dev,
                    g.curve_t_dist,
                    g.other_t_dist,
                    g.curve_t_dev,
                    g.other_t_dev,
                ]);
            }
        }
    }
    Ok(())
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut o: Vec<f64> = Vec::new();
    match i {
        Inputs::ReadDb { read_db } => {
            let ft = if read_db.binary {
                FileType::Binary
            } else {
                FileType::Ascii
            };
            let mut db = DatabaseReader::with_stream(ft, InputStream::new(read_db.bytes.clone()));
            match CrvOnSurf::read_from_db(&Kernel::default(), &mut db, read_db.version) {
                Ok(c) => {
                    o.extend([0.0, 1.0]);
                    put_state(&mut o, &c);
                }
                Err(e) => {
                    if let Some(d) = Output::unported(&e) {
                        return Ok(d);
                    }
                    o.extend([status(&e), 0.0]);
                }
            }
            Ok(to_output(o))
        }
        Inputs::Object { cos, ops } => {
            let mut c = match build(cos)? {
                Ok(c) => c,
                Err(e) => {
                    if let Some(d) = Output::unported(&e) {
                        return Ok(d);
                    }
                    return Err(harness(format!("construction failed: {e}")));
                }
            };
            let k = Kernel::default();
            for op in ops {
                match rust_op(&mut o, &mut c, op, &k) {
                    Ok(()) => {}
                    Err(Stop::Deferred(d)) => return Ok(*d),
                    Err(Stop::Harness(e)) => return Err(e),
                }
            }
            Ok(to_output(o))
        }
    }
}

// ---- generation -------------------------------------------------------------------

/// A random clamped knot vector of degree `p` on `[a, a + len]` with `extra` interior
/// knots (multiplicities up to `p`).
fn gen_knots(rng: &mut Rng, p: usize, extra: usize, a: f64, len: f64) -> Vec<f64> {
    let bnd = a + len;
    let mut inner: Vec<f64> = Vec::new();
    let mut remaining = extra;
    while remaining > 0 {
        let v = if rng.chance(0.4) {
            a + len * (1.0 + rng.below(15) as f64) / 16.0
        } else {
            a + len * rng.uniform(0.02, 0.98)
        };
        if v <= a || v >= bnd || inner.contains(&v) {
            remaining -= 1;
            continue;
        }
        let mult = if p >= 2 && rng.chance(0.2) {
            1 + rng.below(p.min(remaining) as u64) as usize
        } else {
            1
        };
        inner.extend(std::iter::repeat_n(v, mult));
        remaining -= mult;
    }
    inner.sort_by(f64::total_cmp);
    let mut u = vec![a; p + 1];
    u.extend(inner);
    u.extend(std::iter::repeat_n(bnd, p + 1));
    u
}

fn gen_range(rng: &mut Rng) -> (f64, f64) {
    let unit_interval = rng.chance(0.5);
    if unit_interval {
        (0.0, 1.0)
    } else if rng.chance(0.5) {
        (rng.below(5) as f64 - 2.0, 1.0 + rng.below(6) as f64)
    } else {
        (rng.uniform(-5.0, 5.0), rng.uniform(0.25, 10.0))
    }
}

/// A random clamped 3D surface, rational or not.
fn gen_srf(rng: &mut Rng) -> InlineSrf {
    if rng.chance(0.15) {
        // a bilinear patch (upstream's planar and bilinear test surfaces)
        let (a, la) = gen_range(rng);
        let (c, lc) = gen_range(rng);
        let z = |rng: &mut Rng| {
            if rng.chance(0.5) {
                0.0
            } else {
                rng.uniform(-2.0, 2.0)
            }
        };
        let s = rng.uniform(1.0, 10.0);
        return InlineSrf {
            net: vec![
                vec![[0.0, 0.0, z(rng), NOW], [0.0, s, z(rng), NOW]],
                vec![[s, 0.0, z(rng), NOW], [s, s, z(rng), NOW]],
            ],
            p: 1,
            q: 1,
            u: vec![a, a, a + la, a + la],
            v: vec![c, c, c + lc, c + lc],
            borrow_nurb: false,
        };
    }
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let (a, la) = gen_range(rng);
    let eu = rng.below(4) as usize;
    let u = gen_knots(rng, p, eu, a, la);
    let (c, lc) = gen_range(rng);
    let ev = rng.below(4) as usize;
    let v = gen_knots(rng, q, ev, c, lc);
    let n = u.len() - p - 2;
    let m = v.len() - q - 2;
    let rational = rng.chance(0.3);
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let [x, y, z] = if rng.chance(0.6) {
                        [
                            i as f64 + rng.uniform(-0.3, 0.3),
                            j as f64 + rng.uniform(-0.3, 0.3),
                            rng.uniform(-1.0, 1.0),
                        ]
                    } else {
                        rng.point3(-10.0, 10.0)
                    };
                    if rational {
                        let w = rng.uniform(0.3, 3.0);
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
        u,
        v,
        borrow_nurb: false,
    }
}

/// The upstream test surfaces.
const SMS_FILES: [&str; 2] = [
    "upstream/TestFiles/pt_TestFiles/Surface.sms",
    "upstream/TestFiles/pt_TestFiles/DropCurveSurface.sms",
];

/// A point of the domain `d` (shrunk by `margin` of its size), or outside it.
fn gen_uv(rng: &mut Rng, d: [f64; 4], margin: f64, outside: bool) -> [f64; 2] {
    let [u0, v0, u1, v1] = d;
    let (lu, lv) = (u1 - u0, v1 - v0);
    if outside {
        return [
            u0 + lu * rng.uniform(-0.5, 1.5),
            v0 + lv * rng.uniform(-0.5, 1.5),
        ];
    }
    let pick = |rng: &mut Rng, a: f64, l: f64| match rng.below(12) {
        0 => a + l * margin,
        1 => a + l * (1.0 - margin),
        _ => a + l * rng.uniform(margin, 1.0 - margin),
    };
    [pick(rng, u0, lu), pick(rng, v0, lv)]
}

fn gen_uv_curve(rng: &mut Rng, d: [f64; 4], outside: bool) -> UvIn {
    let margin = if rng.chance(0.2) { 0.0 } else { 0.02 };
    if rng.chance(0.2) {
        let a = gen_uv(rng, d, margin, outside);
        let mut bb = gen_uv(rng, d, margin, outside);
        if (a[0] - bb[0]).abs() + (a[1] - bb[1]).abs() < 1e-3 {
            bb[0] = d[0] + 0.5 * (d[2] - d[0]);
            bb[1] = d[1] + 0.5 * (d[3] - d[1]);
        }
        return UvIn::Line([a[0], a[1], bb[0], bb[1]]);
    }
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(5) as usize;
    let (a, len) = gen_range(rng);
    let u = gen_knots(rng, p, n - p, a, len);
    let rational = rng.chance(0.2);
    let cpts = (0..=n)
        .map(|_| {
            let [x, y] = gen_uv(rng, d, margin, outside);
            if rational {
                let w = rng.uniform(0.5, 2.0);
                [x * w, y * w, w]
            } else {
                [x, y, NOW]
            }
        })
        .collect();
    UvIn::Bspline(UvBspline {
        cpts,
        p,
        u,
        borrow_nurb: false,
    })
}

fn range_of(c: &CosIn) -> (f64, f64) {
    match &c.uv {
        UvIn::Bspline(bs) => (
            bs.u.first().copied().unwrap_or(0.0),
            bs.u.last().copied().unwrap_or(1.0),
        ),
        UvIn::Line(_) => (0.0, 1.0),
    }
}

fn uv_knots(c: &CosIn) -> Vec<f64> {
    match &c.uv {
        UvIn::Bspline(bs) => {
            let mut k = bs.u.clone();
            k.dedup();
            k
        }
        UvIn::Line(_) => vec![0.0, 1.0],
    }
}

/// Increasing breaks in `[a, b]`: the UV knots plus a few interior values.
fn gen_breaks(rng: &mut Rng, c: &CosIn, a: f64, bb: f64) -> Vec<f64> {
    let mut v: Vec<f64> = uv_knots(c)
        .into_iter()
        .filter(|&t| t >= a && t <= bb)
        .collect();
    for _ in 0..rng.below(3) {
        v.push(a + (bb - a) * rng.uniform(0.05, 0.95));
    }
    if rng.chance(0.2) {
        v.push(a);
        v.push(bb);
    }
    v.sort_by(f64::total_cmp);
    v.dedup();
    if rng.chance(0.05) {
        v.truncate(rng.below(2) as usize);
    }
    v
}

fn gen_cos(rng: &mut Rng, outside: bool) -> (CosIn, [f64; 4]) {
    let srf = if rng.chance(0.08) {
        SrfIn::File {
            sms: (*rng.choose(&SMS_FILES).unwrap_or(&"")).to_string(),
        }
    } else {
        SrfIn::Inline(gen_srf(rng))
    };
    let d = resolve(&srf).map_or([0.0, 0.0, 1.0, 1.0], |s| srf_domain(&s));
    let uv = gen_uv_curve(rng, d, outside);
    let domain = match rng.below(10) {
        0 => Some(d),
        1 => {
            let [u0, v0, u1, v1] = d;
            Some([
                u0 + 0.1 * (u1 - u0),
                v0 + 0.1 * (v1 - v0),
                u1 - 0.2 * (u1 - u0),
                v1,
            ])
        }
        _ => None,
    };
    let owner = if rng.chance(0.9) {
        rng.below(4)
    } else {
        rng.below(8)
    };
    let mut c = CosIn {
        uv,
        srf,
        offset: None,
        domain,
        owner,
        cache: None,
    };
    if rng.chance(0.6) {
        let (a, bb) = range_of(&c);
        c.cache = Some(CacheIn {
            need: rng.chance(0.1),
            breaks: gen_breaks(rng, &c, a, bb),
        });
    }
    (c, d)
}

/// A parameter of the range `[a, b]`: interior, at an end, or slightly outside.
fn gen_t(rng: &mut Rng, a: f64, bb: f64, knots: &[f64]) -> f64 {
    match rng.below(10) {
        0 => a,
        1 => bb,
        2 => *rng.choose(knots).unwrap_or(&a),
        3 => a + (bb - a) * rng.uniform(-0.2, 1.2),
        _ => a + (bb - a) * rng.unit(),
    }
}

fn gen_inner(rng: &mut Rng, a: f64, bb: f64) -> (f64, f64) {
    if rng.chance(0.3) {
        return (a, bb);
    }
    let x = a + (bb - a) * rng.uniform(0.0, 0.45);
    let y = a + (bb - a) * rng.uniform(0.55, 1.0);
    (x, y)
}

fn rot_frame(rng: &mut Rng) -> ([f64; 3], [f64; 3]) {
    let t = rng.uniform(0.0, std::f64::consts::TAU);
    let (s, c) = t.sin_cos();
    match rng.below(3) {
        0 => ([c, s, 0.0], [-s, c, 0.0]),
        1 => ([c, 0.0, -s], [0.0, 1.0, 0.0]),
        _ => ([1.0, 0.0, 0.0], [0.0, c, s]),
    }
}

/// An op on the object with current range `[a, b]` (updated by the editing ops).
fn gen_op(rng: &mut Rng, c: &CosIn, d: [f64; 4], range: &mut (f64, f64), outside: bool) -> OpIn {
    let (a, bb) = *range;
    let knots = uv_knots(c);
    let pick = if outside {
        // only the ops safe on a UV curve outside the domain
        *rng.choose(&[0, 2, 3, 4, 10, 11]).unwrap_or(&0)
    } else {
        rng.below(40)
    };
    match pick {
        0 => OpIn::Info {
            types: vec![
                CrvOnSurf::TYPE,
                14001,
                14033,
                14053,
                10001,
                rng.below(30000) as i64,
            ],
        },
        1 => OpIn::State,
        2 | 12 | 13 => OpIn::Eval {
            t: gen_t(rng, a, bb, &knots),
            n: rng.below(7) as usize,
            left: rng.chance(0.5),
            nonzero: rng.chance(0.5),
        },
        3 | 14 => OpIn::EvalPoint {
            t: gen_t(rng, a, bb, &knots),
        },
        4 => OpIn::EvalUv {
            t: gen_t(rng, a, bb, &knots),
        },
        5 => OpIn::EvalNormal {
            t: gen_t(rng, a, bb, &knots),
            left: rng.chance(0.5),
        },
        6 => OpIn::Eval1st {
            t: gen_t(rng, a, bb, &knots),
            n: rng.below(4) as usize,
            left: rng.chance(0.5),
        },
        7 | 15 => OpIn::Knots {
            ivl: if rng.chance(0.4) {
                let (x, y) = gen_inner(rng, a, bb);
                Some([x, y])
            } else {
                None
            },
            want: rng.chance(0.7),
        },
        8 => OpIn::Nnk,
        9 | 16 => OpIn::Conts {
            angle: if rng.chance(0.5) {
                0.5
            } else {
                rng.uniform(0.0, 10.0)
            },
        },
        10 => OpIn::Equal {
            other: match rng.below(10) {
                0..=1 => None,
                2..=4 => Some(Box::new(c.clone())),
                5..=6 => {
                    let mut o = c.clone();
                    o.domain = Some([d[0], d[1], d[2] * 0.5 + d[0] * 0.5, d[3]]);
                    Some(Box::new(o))
                }
                _ => Some(Box::new(gen_cos(rng, false).0)),
            },
        },
        11 | 17 => OpIn::Copy,
        18 => {
            let x = rng.uniform(-10.0, 10.0);
            let y = x + rng.uniform(0.01, 20.0) * if rng.chance(0.1) { -1.0 } else { 1.0 };
            *range = (x.min(y), x.max(y));
            OpIn::EditParam { a: x, b: y }
        }
        19 => {
            let (x, y) = gen_inner(rng, a, bb);
            OpIn::Reverse { a: x, b: y }
        }
        20 | 21 => {
            let (x, y) = gen_inner(rng, a, bb);
            let mode = rng.below(3);
            if c.owner != 0 && mode != 2 {
                *range = (x, y);
            }
            OpIn::Trim { a: x, b: y, mode }
        }
        22 | 23 => {
            let identity = rng.chance(0.3);
            let (x, y) = if identity {
                ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
            } else {
                rot_frame(rng)
            };
            OpIn::Transform {
                origin: if identity && rng.chance(0.7) {
                    [0.0; 3]
                } else {
                    rng.point3(-5.0, 5.0)
                },
                x,
                y,
                scale: match rng.below(4) {
                    0 => None,
                    1 => Some([1.0, 1.0, 1.0]),
                    2 => {
                        let s = rng.uniform(0.5, 3.0);
                        Some([s, s, s])
                    }
                    _ => Some([
                        rng.uniform(0.5, 2.0),
                        rng.uniform(0.5, 2.0),
                        rng.uniform(0.5, 2.0),
                    ]),
                },
            }
        }
        24..=28 => OpIn::Approx {
            breaks: gen_breaks(rng, c, a, bb),
            tol: *rng.choose(&[1e-3, 1e-4, 5e-6, 1e-2]).unwrap_or(&1e-3),
            match_param: rng.chance(0.5),
        },
        29..=31 => OpIn::GetOrCreate {
            tol: *rng.choose(&[1e-3, 1e-4, 5e-6, 1e-2]).unwrap_or(&1e-3),
            breaks: if rng.chance(0.6) {
                Some(gen_breaks(rng, c, a, bb))
            } else {
                None
            },
            match_param: if rng.chance(0.5) {
                Some(rng.chance(0.5))
            } else {
                None
            },
        },
        32 | 33 => {
            let (x, y) = gen_inner(rng, a, bb);
            OpIn::Bbox { a: x, b: y }
        }
        34..=37 => {
            let (x, y) = gen_inner(rng, a, bb);
            let [u, v] = gen_uv(rng, d, 0.0, false);
            let s = resolve(&c.srf).ok();
            // a point near the surface: a corner-ish control point plus noise
            let base = s
                .as_ref()
                .and_then(|s| s.net.first().and_then(|r| r.first()).copied())
                .map_or([u, v, 0.0], |p| {
                    let w = if p[3] == NOW { 1.0 } else { p[3] };
                    [p[0] / w, p[1] / w, p[2] / w]
                });
            let noise = rng.point3(-3.0, 3.0);
            OpIn::Drop {
                a: x,
                b: y,
                point: [base[0] + noise[0], base[1] + noise[1], base[2] + noise[2]],
                vec: if rng.chance(0.2) {
                    Some(rng.point3(-1.0, 1.0))
                } else {
                    None
                },
                tol: *rng.choose(&[1e-5, 1e-3, 0.0, 0.1]).unwrap_or(&1e-5),
                guess: if rng.chance(0.5) {
                    Some(x + (y - x) * rng.unit())
                } else {
                    None
                },
                solver_op: *rng.choose(&[3, 3, 3, 0, 5, 4]).unwrap_or(&3),
            }
        }
        _ => {
            let (x, y) = if rng.chance(0.6) {
                (a, bb)
            } else {
                gen_inner(rng, a, bb)
            };
            let same = rng.chance(0.7);
            let (s0, s1) = if same {
                if rng.chance(0.5) {
                    (x, y)
                } else {
                    (y, x)
                }
            } else {
                gen_inner(rng, a, bb)
            };
            OpIn::Gap {
                other: if same && rng.chance(0.8) {
                    None
                } else {
                    Some(Box::new(c.clone()))
                },
                a: x,
                b: y,
                s0,
                s1,
                samples: *rng.choose(&[0, 2, 3, 5]).unwrap_or(&2),
                tol: 1e-5,
                max: if rng.chance(0.3) { Some(1e-3) } else { None },
            }
        }
    }
}

/// A database record written by upstream `WriteToDB` for a random object, sometimes
/// with the UV curve type replaced by one that is not a curve.
fn gen_read_db(rng: &mut Rng) -> Option<Inputs> {
    let (c, _) = gen_cos(rng, false);
    let binary = rng.chance(0.5);
    let version = *rng.choose(&[30u64, 33, 40])?;
    let mut v = vec![b(binary), version as f64];
    cos_block(&mut v, &c).ok()?;
    let out = ffi::eval(Op::WriteDb, &v).ok()?;
    let (&status, bytes) = out.split_first()?;
    if status != 0.0 {
        return None;
    }
    let mut bytes: Vec<u8> = bytes.iter().map(|&x| x as u8).collect();
    if binary {
        // the UV curve type follows the domain (4 doubles) and the breaks flag (an int)
        if rng.chance(0.5) {
            let to: i64 = *rng.choose(&[14099, 1])?;
            if let Some(t) = bytes.get_mut(36..44) {
                t.copy_from_slice(&to.to_le_bytes());
            }
        }
    } else if rng.chance(0.4) {
        let text = String::from_utf8(bytes.clone()).ok()?;
        let from = ["Object Type 14033", "Object Type 14010"];
        let to = *rng.choose(&["Object Type 14099", "Object Type 1"])?;
        let mut t = text.clone();
        for f in from {
            if let Some(pos) = t.find(f) {
                t.replace_range(pos..pos + f.len(), to);
                break;
            }
        }
        bytes = t.into_bytes();
    }
    Some(Inputs::ReadDb {
        read_db: ReadDbIn {
            binary,
            version,
            bytes,
        },
    })
}

fn gen_case(rng: &mut Rng) -> Option<Inputs> {
    if rng.chance(0.05) {
        return gen_read_db(rng);
    }
    let outside = rng.chance(0.05);
    let (c, d) = gen_cos(rng, outside);
    let mut range = range_of(&c);
    let nops = if rng.chance(0.7) {
        1
    } else {
        2 + rng.below(3) as usize
    };
    let ops = (0..nops)
        .map(|_| gen_op(rng, &c, d, &mut range, outside))
        .collect();
    Some(Inputs::Object {
        cos: Box::new(c),
        ops,
    })
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.crv_on_surf"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let Some(i) = gen_case(rng) else {
                continue;
            };
            let Ok((op, input)) = cpp_input(&i) else {
                continue;
            };
            if ffi::validate(op, &input).is_ok() {
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
        let i = case.parse()?;
        // Run only what the FFI accepts, as the C++ side: its guards refuse inputs
        // upstream (and the faithful port) never finish on (fuzz Rust-side hangs).
        let (code, input) = cpp_input(&i)?;
        ffi::validate(code, &input).map_err(|e| HarnessError::new(e.to_string()))?;
        rust(&i)
    }
}
