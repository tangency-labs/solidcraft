//! Runner for unit `curve.curve_p1`: `SmCurve` part 1 (the global point solver and the
//! local solve of its curve/point global solver, `ApproximateCurve`, tessellation by
//! bisection and `Tessellate`, the classification of curve/curve range intersections)
//! and the static helpers `sm_TestSpanAccuracy` and `sm_RefineCCSolutions`.
//!
//! C++: `solidcore_ffi::units::curve_curve_p1` (upstream `source/SMLib/src/SmCurve.cpp`,
//! called through `SmCurve*` on B-spline, Hermite and line curves; the static helpers
//! are verbatim copies in the shim). Rust: `solidcore::curve::curve::p1`.
//!
//! A case is one `op` with its curves (`{"bspline": {dim, oob, cpts, p, u}}` with
//! control points as stored `[x, y, z, w]`, `{"hermite": {dim, p1, d1, p2, d2}}`,
//! `{"circle": ..}` (a quadratic B-spline arc) or `{"line": {p, v, domain, scale,
//! inside_out}}`) and parameters. Both sides produce the flat record documented in the
//! ffi module; the runner reports its first entry as the `status` int (`0` =
//! `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust
//! error upstream cannot produce) and the rest as the `out` values.
//!
//! `{"o", "x", "y", "scale"?, "repeat"?}` transforms (`gps` `xf`) are applied on both sides
//! (`SmAxis2Placement::SetCanonical` + `SmCurve::Transform` / `Curve::transform`) before
//! the op; `gps` `pts` are more points, each its own solve (`status_i` / `out_i`).
//!
//! Deferred cases (unit `curve.curve_p5`): `GlobalPointSolve` of the directed and
//! projected operations (`SmCurve::LocalPropertyAnalysis`) and of minimise / intersect
//! on curved B-splines (the cache's analytic conversion reaches `SmCurve::IsArc`), and
//! the classification and refinement of range solutions past their early exits
//! (`SmFindCCNormalENFO::Evaluate`, `SmCCIntersectENFO::Evaluate`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p1::{
    refine_cc_solutions, test_span_accuracy, ApproximateCurveOptions, GlobalPointSolveOptions,
};
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::global_solver::{SolutionRequested, SolverOperation};
use solidcore::geom::solution_array::{
    ObjectKind, Solution, SolutionArray, SolutionObject, SolutionType,
};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, Tessellation};
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::SolidError;
use solidcore_ffi::units::curve_curve_p1::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `curve.curve_p1` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BSplineIn {
    dim: u64,
    #[serde(default)]
    oob: bool,
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HermiteIn {
    dim: u64,
    p1: [f64; 3],
    d1: [f64; 3],
    p2: [f64; 3],
    d2: [f64; 3],
}

/// A quadratic circular arc built by the Rust `CreateCircleSegment` (bit-identical to
/// upstream, see unit `nurbs.b_spline_curve_p1`) and handed to both sides as inline data.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CircleIn {
    center: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    radius: f64,
    a0: f64,
    a1: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LineIn {
    p: [f64; 3],
    v: [f64; 3],
    domain: [f64; 2],
    #[serde(default = "one")]
    scale: f64,
    #[serde(default)]
    inside_out: bool,
}

fn one() -> f64 {
    1.0
}

/// A transform applied to the curve before the op, `repeat` times
/// (`SmAxis2Placement::SetCanonical(o, x, y)`, then `SmCurve::Transform` with `scale`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct XformIn {
    o: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scale: Option<[f64; 3]>,
    #[serde(default = "one_u64")]
    repeat: u64,
}

fn one_u64() -> u64 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CurveIn {
    Bspline(BSplineIn),
    Hermite(HermiteIn),
    Circle(CircleIn),
    Line(LineIn),
}


/// `c` with a circle replaced by its inline B-spline.
fn resolve(c: &CurveIn) -> Result<CurveIn, HarnessError> {
    let CurveIn::Circle(ci) = c else {
        return Ok(c.clone());
    };
    let frame = Axis2Placement::new(v3(ci.center), v3(ci.x), v3(ci.y));
    let arc = BSplineCurve::create_circle_segment(
        3,
        &frame,
        ci.radius,
        ci.a0,
        ci.a1,
        solidcore::nurbs::b_spline_curve::NurbCircleParam::Quadratic,
    )
    .map_err(|e| harness(format!("circle: {e}")))?;
    let cur = arc.gw_nurb().ok_or_else(|| harness("circle without NURB"))?;
    Ok(CurveIn::Bspline(BSplineIn {
        dim: 3,
        oob: false,
        cpts: cur.cpts().iter().map(|p| [p.x, p.y, p.z, p.w]).collect(),
        p: cur.degree(),
        u: cur.knots().to_vec(),
    }))
}

fn b(x: bool) -> f64 {
    f64::from(u8::from(x))
}

fn push_line(v: &mut Vec<f64>, l: &LineIn) {
    v.extend_from_slice(&l.p);
    v.extend_from_slice(&l.v);
    v.extend([l.domain[0], l.domain[1], l.scale, b(l.inside_out)]);
}

fn curve_block(v: &mut Vec<f64>, c: &CurveIn) -> Result<(), HarnessError> {
    match &resolve(c)? {
        CurveIn::Bspline(bs) => {
            let n = bs.cpts.len().saturating_sub(1);
            let m = bs.u.len().saturating_sub(1);
            v.extend([0.0, bs.dim as f64, b(bs.oob)]);
            v.extend([n as f64, bs.p as f64, m as f64]);
            for c in &bs.cpts {
                v.extend_from_slice(c);
            }
            v.extend_from_slice(&bs.u);
        }
        CurveIn::Hermite(h) => {
            v.extend([1.0, h.dim as f64]);
            for p in [h.p1, h.d1, h.p2, h.d2] {
                v.extend_from_slice(&p);
            }
        }
        CurveIn::Line(l) => {
            v.extend([2.0, 3.0]);
            push_line(v, l);
        }
        CurveIn::Circle(_) => return Err(harness("unresolved circle")),
    }
    Ok(())
}


fn push_vecs(v: &mut Vec<f64>, vecs: &[[f64; 3]]) {
    v.push(vecs.len() as f64);
    for x in vecs {
        v.extend_from_slice(x);
    }
}

/// The transforms block `nt (O X Y has_s S)*nt` (see the ffi module docs).
fn push_xf(v: &mut Vec<f64>, xf: &[XformIn]) {
    v.push(xf.iter().map(|t| t.repeat).sum::<u64>() as f64);
    for t in xf {
        for _ in 0..t.repeat {
            v.extend_from_slice(&t.o);
            v.extend_from_slice(&t.x);
            v.extend_from_slice(&t.y);
            v.push(b(t.scale.is_some()));
            v.extend_from_slice(&t.scale.unwrap_or([1.0; 3]));
        }
    }
}

fn push_opt(v: &mut Vec<f64>, x: Option<f64>) {
    v.extend([b(x.is_some()), x.unwrap_or(0.0)]);
}


fn to_output(flat: &[f64]) -> Output {
    let status = flat.first().copied().unwrap_or(f64::NAN);
    let rest = flat.get(1..).unwrap_or(&[]).to_vec();
    Output::new()
        .ints("status", vec![status as i64])
        .values("out", rest)
}


fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

fn build_line(l: &LineIn) -> Line {
    let opts = BoundedLineOptions {
        scale: l.scale,
        inside_out: l.inside_out,
        ..BoundedLineOptions::default()
    };
    Line::bounded(
        v3(l.p),
        v3(l.v),
        &Extent1d::from_min_max(l.domain[0], l.domain[1]),
        &opts,
    )
}

fn build(c: &CurveIn) -> Result<Curve, HarnessError> {
    match &resolve(c)? {
        CurveIn::Bspline(bs) => {
            let n = bs
                .cpts
                .len()
                .checked_sub(1)
                .ok_or_else(|| harness("no points"))?;
            let m = bs.u.len().checked_sub(1).ok_or_else(|| harness("no knots"))?;
            let mut cur =
                allocate_nurb_curve(n, bs.p, m).map_err(|e| harness(format!("curve: {e}")))?;
            for (d, s) in cur.cpts_mut().iter_mut().zip(&bs.cpts) {
                d.x = s[0];
                d.y = s[1];
                d.z = s[2];
                d.w = s[3];
            }
            cur.knots_mut().copy_from_slice(&bs.u);
            let mut c = BSplineCurve::from_nurb(cur, bs.dim, false);
            c.set_out_of_bounds_enabled(bs.oob);
            Ok(Curve::from(c))
        }
        CurveIn::Hermite(h) => Ok(Curve::from(HermiteCurve::new(
            v3(h.p1),
            v3(h.d1),
            v3(h.p2),
            v3(h.d2),
            h.dim,
        ))),
        CurveIn::Line(l) => Ok(Curve::from(build_line(l))),
        CurveIn::Circle(_) => Err(harness("unresolved circle")),
    }
}

/// `c` transformed by `xf` in order (`Curve::transform`).
fn apply_xf(k: &Kernel, c: &mut Curve, xf: &[XformIn]) -> Result<(), HarnessError> {
    for t in xf {
        let a2p = Axis2Placement::new(v3(t.o), v3(t.x), v3(t.y));
        for _ in 0..t.repeat {
            c.transform(k, &a2p, t.scale.map(v3))
                .map_err(|e| harness(format!("transform: {e}")))?;
        }
    }
    Ok(())
}

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
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

/// A Rust failure: deferred for `Unported`, else the status record.
enum Fail {
    Deferred(Box<Output>),
    Status(f64),
}

impl From<SolidError> for Fail {
    fn from(e: SolidError) -> Self {
        match Output::unported(&e) {
            Some(o) => Fail::Deferred(Box::new(o)),
            None => Fail::Status(status(&e)),
        }
    }
}

/// Evaluates `r`: `Ok(v)`, or records the status / deferral.
macro_rules! tri {
    ($o:ident, $r:expr) => {
        match $r {
            Ok(v) => v,
            Err(e) => match Fail::from(e) {
                Fail::Deferred(d) => return Ok(*d),
                Fail::Status(s) => {
                    $o.push(s);
                    return Ok(to_output(&$o));
                }
            },
        }
    };
}

/// The status of `r` (`0` for `Ok`), or the deferral.
fn st_of<T>(r: &solidcore::Result<T>) -> Result<f64, Box<Output>> {
    match r {
        Ok(_) => Ok(0.0),
        Err(e) => match Output::unported(e) {
            Some(o) => Err(Box::new(o)),
            None => Ok(status(e)),
        },
    }
}

/// A solution object: an id (`0` the curve, `1` the second curve, `2` another
/// object) and, for a curve, its natural interval.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Obj(i64, Option<Extent1d>);

impl SolutionObject for Obj {
    fn kind(&self) -> ObjectKind {
        if self.1.is_some() {
            ObjectKind::Curve
        } else {
            ObjectKind::Other
        }
    }

    fn natural_interval(&self) -> Option<Extent1d> {
        self.1
    }
}


fn sop_of(code: i32) -> Result<SolverOperation, HarnessError> {
    SolverOperation::from_code(code).ok_or_else(|| harness("solver operation"))
}

fn ivl_of(i: [f64; 2]) -> Extent1d {
    Extent1d::from_min_max(i[0], i[1])
}


/// A two-curve solution (`type v0 s0 s1 v1 e0 e1`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SolIn {
    range: bool,
    #[serde(default = "two")]
    nvar: u64,
    /// Objects `+1`: `0` = `NULL`, `1` = the first curve, `2` = the second.
    #[serde(default = "objs")]
    o: [u64; 2],
    v0: f64,
    s: [f64; 2],
    v1: f64,
    e: [f64; 2],
}

fn two() -> u64 {
    2
}

fn objs() -> [u64; 2] {
    [1, 2]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Gps {
        curve: CurveIn,
        /// Transforms applied to `curve` first.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        xf: Vec<XformIn>,
        ivl: [f64; 2],
        sop: i32,
        p: [f64; 3],
        /// More points, each solved like `p` (as its own solve; outputs `*_i`).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pts: Vec<[f64; 3]>,
        tol: f64,
        target: Option<f64>,
        vecs: Vec<[f64; 3]>,
        req: i32,
    },
    Approx {
        curve: CurveIn,
        breaks: Vec<f64>,
        tol: f64,
        analytics: bool,
        match_param: bool,
        copy: bool,
    },
    Bisect {
        curve: CurveIn,
        ivl: [f64; 2],
        chord: f64,
        angle: f64,
        max3d: f64,
    },
    Tess {
        curve: CurveIn,
        ivl: [f64; 2],
        chord: f64,
        angle: f64,
        minseg: u64,
    },
    Classify {
        c1: CurveIn,
        c2: CurveIn,
        sol: SolIn,
    },
    Span {
        curve: CurveIn,
        lt: f64,
        lp: [f64; 3],
        ld: [f64; 3],
        ct: f64,
        cp: [f64; 3],
        cd: [f64; 3],
        tol: f64,
        match_param: bool,
    },
    Refine {
        c1: CurveIn,
        c2: CurveIn,
        sols: Vec<SolIn>,
    },
}

// ---- C++ input --------------------------------------------------------------------

fn op_of(op: &OpIn) -> Op {
    match op {
        OpIn::Gps { .. } => Op::Gps,
        OpIn::Approx { .. } => Op::Approx,
        OpIn::Bisect { .. } => Op::Bisect,
        OpIn::Tess { .. } => Op::Tess,
        OpIn::Classify { .. } => Op::Classify,
        OpIn::Span { .. } => Op::Span,
        OpIn::Refine { .. } => Op::Refine,
    }
}

fn push_ends(v: &mut Vec<f64>, s: &SolIn) {
    v.extend([s.v0, s.s[0], s.s[1], s.v1, s.e[0], s.e[1]]);
}

fn type_code(s: &SolIn) -> f64 {
    if s.range {
        2.0
    } else {
        1.0
    }
}

fn cpp_input(op: &OpIn) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    match op {
        OpIn::Gps {
            curve,
            xf,
            ivl,
            sop,
            p,
            tol,
            target,
            vecs,
            req,
            ..
        } => {
            curve_block(&mut v, curve)?;
            push_xf(&mut v, xf);
            v.extend([ivl[0], ivl[1], f64::from(*sop)]);
            v.extend_from_slice(p);
            v.push(*tol);
            push_opt(&mut v, *target);
            push_vecs(&mut v, vecs);
            v.push(f64::from(*req));
        }
        OpIn::Approx {
            curve,
            breaks,
            tol,
            analytics,
            match_param,
            copy,
        } => {
            curve_block(&mut v, curve)?;
            v.push(breaks.len() as f64);
            v.extend_from_slice(breaks);
            v.extend([*tol, b(*analytics), b(*match_param), b(*copy)]);
        }
        OpIn::Bisect {
            curve,
            ivl,
            chord,
            angle,
            max3d,
        } => {
            curve_block(&mut v, curve)?;
            v.extend([ivl[0], ivl[1], *chord, *angle, *max3d]);
        }
        OpIn::Tess {
            curve,
            ivl,
            chord,
            angle,
            minseg,
        } => {
            curve_block(&mut v, curve)?;
            v.extend([ivl[0], ivl[1], *chord, *angle, *minseg as f64]);
        }
        OpIn::Classify { c1, c2, sol } => {
            curve_block(&mut v, c1)?;
            curve_block(&mut v, c2)?;
            v.push(type_code(sol));
            push_ends(&mut v, sol);
        }
        OpIn::Span {
            curve,
            lt,
            lp,
            ld,
            ct,
            cp,
            cd,
            tol,
            match_param,
        } => {
            curve_block(&mut v, curve)?;
            v.push(*lt);
            v.extend_from_slice(lp);
            v.extend_from_slice(ld);
            v.push(*ct);
            v.extend_from_slice(cp);
            v.extend_from_slice(cd);
            v.extend([*tol, b(*match_param)]);
        }
        OpIn::Refine { c1, c2, sols } => {
            curve_block(&mut v, c1)?;
            curve_block(&mut v, c2)?;
            v.push(sols.len() as f64);
            for s in sols {
                v.extend([type_code(s), s.nvar as f64, s.o[0] as f64, s.o[1] as f64]);
                push_ends(&mut v, s);
            }
        }
    }
    Ok((op_of(op), v))
}

/// A `Gps` with extra points `pts`: one solve per point (`p` first) by `f`, point `i > 0`
/// reported as `status_i` / `out_i`; deferred when any solve is. `None` for other cases.
fn per_point(
    op: &OpIn,
    f: impl Fn(&OpIn) -> Result<Output, HarnessError>,
) -> Result<Option<Output>, HarnessError> {
    let OpIn::Gps { p, pts, .. } = op else {
        return Ok(None);
    };
    if pts.is_empty() {
        return Ok(None);
    }
    let mut all = Output::new();
    for (i, q) in std::iter::once(p).chain(pts).enumerate() {
        let mut one = op.clone();
        if let OpIn::Gps { p, pts, .. } = &mut one {
            *p = *q;
            pts.clear();
        }
        let o = f(&one)?;
        if o.deferred.is_some() {
            return Ok(Some(o));
        }
        let sfx = if i == 0 { String::new() } else { format!("_{i}") };
        for (key, v) in o.ints {
            all.ints.insert(format!("{key}{sfx}"), v);
        }
        for (key, v) in o.values {
            all.values.insert(format!("{key}{sfx}"), v);
        }
    }
    Ok(Some(all))
}

/// The FFI's own validation of `op` (what [`cpp`] runs, point by point as there),
/// applied to the Rust side too: its guards refuse inputs upstream (and the
/// faithful port) never finish on (fuzz Rust-side hangs).
fn validate(op: &OpIn) -> Result<Output, HarnessError> {
    if let Some(o) = per_point(op, validate)? {
        return Ok(o);
    }
    let (code, input) = cpp_input(op)?;
    ffi::validate(code, &input).map_err(|e| harness(e.to_string()))?;
    Ok(Output::new())
}

fn cpp(op: &OpIn) -> Result<Output, HarnessError> {
    if let Some(o) = per_point(op, cpp)? {
        return Ok(o);
    }
    let (code, input) = cpp_input(op)?;
    match ffi::eval(code, &input) {
        Ok(flat) => Ok(to_output(&flat)),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side --------------------------------------------------------------------

/// A one-variable solution record (see the ffi module docs).
fn put_sol1(o: &mut Vec<f64>, s: &Solution<Obj>) {
    let id = |x: Option<Obj>| x.map_or(-1.0, |v| v.0 as f64);
    o.extend([
        f64::from(s.solution_type as i32),
        s.num_variables as f64,
        s.num_objects as f64,
        if s.num_objects > 0 {
            id(s.objects[0])
        } else {
            -1.0
        },
        s.start.solution_value,
        s.start.parameters[0],
    ]);
    if s.solution_type == SolutionType::RangeOfValues {
        o.extend([s.end.solution_value, s.end.parameters[0]]);
    }
}

/// A result curve record (see the ffi module docs).
fn put_curve(o: &mut Vec<f64>, c: &Curve) {
    o.extend([c.type_code() as f64, c.dim() as f64]);
    let nurb = match c.kind() {
        CurveKind::BSpline(b) => b.gw_nurb(),
        CurveKind::Line(l) => l.base().gw_nurb(),
        CurveKind::Hyperbola(h) => h.base().gw_nurb(),
        CurveKind::Parabola(p) => p.base().gw_nurb(),
        _ => None,
    };
    let Some(nurb) = nurb else {
        o.push(0.0);
        return;
    };
    o.push(1.0);
    o.extend([
        nurb.cpts().len() as f64 - 1.0,
        nurb.degree() as f64,
        nurb.knots().len() as f64 - 1.0,
    ]);
    for p in nurb.cpts() {
        o.extend([p.x, p.y, p.z, p.w]);
    }
    o.extend_from_slice(nurb.knots());
}

fn put_tess(o: &mut Vec<f64>, t: &Tessellation) {
    o.extend([
        0.0,
        t.parameters.len() as f64,
        t.points.len() as f64,
        t.tangents.len() as f64,
    ]);
    o.extend_from_slice(&t.parameters);
    for p in &t.points {
        pv(o, *p);
    }
    for d in &t.tangents {
        pv(o, *d);
    }
}

/// A solution of `c1` / `c2` (objects `Obj(0)`, `Obj(1)`) from its record.
fn solution_of(s: &SolIn) -> Solution<Obj> {
    let mut sol: Solution<Obj> = Solution::new();
    sol.solution_type = if s.range {
        SolutionType::RangeOfValues
    } else {
        SolutionType::SingleValue
    };
    sol.num_variables = s.nvar as usize;
    sol.num_objects = 2;
    for (slot, &ob) in sol.objects.iter_mut().zip(&s.o) {
        *slot = match ob {
            1 => Some(Obj(0, None)),
            2 => Some(Obj(1, None)),
            _ => None,
        };
    }
    sol.start.solution_value = s.v0;
    sol.start.parameters[0] = s.s[0];
    sol.start.parameters[1] = s.s[1];
    sol.end.solution_value = s.v1;
    sol.end.parameters[0] = s.e[0];
    sol.end.parameters[1] = s.e[1];
    sol
}

fn rust(op: &OpIn) -> Result<Output, HarnessError> {
    if let Some(o) = per_point(op, rust)? {
        return Ok(o);
    }
    let k = Kernel::default();
    let mut o: Vec<f64> = Vec::new();
    match op {
        OpIn::Gps {
            curve,
            xf,
            ivl,
            sop,
            p,
            tol,
            target,
            vecs,
            req,
            ..
        } => {
            let mut c = build(curve)?;
            apply_xf(&k, &mut c, xf)?;
            let vs: Vec<Vec3> = vecs.iter().map(|x| v3(*x)).collect();
            // SmTol::GetZoneTol3d of the (unowned) curve: the system zone tolerance
            let system_zone_tol = |_: Obj| {
                Ok(solidcore::geom::tol::zone_tol_3d(
                    &k,
                    solidcore::geom::tol::SmallTopoType::NormalSize,
                ))
            };
            let opts = GlobalPointSolveOptions {
                target_distance: *target,
                opt_vectors: if vs.is_empty() { None } else { Some(&vs) },
                handle: Some(Obj(0, Some(c.natural_interval()))),
                analytic_handle: Some(Obj(2, None)),
                zone_tol_3d: Some(solidcore::geom::global_solver::ObjectZoneTol(&system_zone_tol)),
            };
            let req = SolutionRequested::from_code(*req).ok_or_else(|| harness("req"))?;
            let sols = tri!(
                o,
                c.global_point_solve(&k, &ivl_of(*ivl), sop_of(*sop)?, v3(*p), *tol, req, &opts)
            );
            o.extend([0.0, sols.len() as f64]);
            for s in sols.as_slice() {
                put_sol1(&mut o, s);
            }
        }
        OpIn::Approx {
            curve,
            breaks,
            tol,
            analytics,
            match_param,
            copy,
        } => {
            let c = build(curve)?;
            let opts = ApproximateCurveOptions {
                create_analytics: *analytics,
                match_parameterization: *match_param,
                just_copy_bsplines: *copy,
            };
            let r = tri!(o, c.approximate_curve(&Kernel::default(), breaks, *tol, &opts));
            o.extend([
                0.0,
                b(r.achieved_tol.is_some()),
                r.achieved_tol.unwrap_or(0.0),
            ]);
            put_curve(&mut o, &r.curve);
        }
        OpIn::Bisect {
            curve,
            ivl,
            chord,
            angle,
            max3d,
        } => {
            let c = build(curve)?;
            let t = tri!(
                o,
                c.tessellate_by_bisection(&k, &ivl_of(*ivl), *chord, *angle, *max3d)
            );
            put_tess(&mut o, &t);
        }
        OpIn::Tess {
            curve,
            ivl,
            chord,
            angle,
            minseg,
        } => {
            let c = build(curve)?;
            let t = tri!(o, c.tessellate(&k, &ivl_of(*ivl), *chord, *angle, *minseg));
            put_tess(&mut o, &t);
        }
        OpIn::Classify { c1, c2, sol } => {
            let a = build(c1)?;
            let bb = build(c2)?;
            let s = solution_of(sol);
            let (degen, pt) = tri!(o, a.classify_intersecting_range_solution(&k, &bb, &s));
            o.extend([0.0, b(degen), b(pt.is_some())]);
            if let Some(pt) = pt {
                o.extend([pt.solution_value, pt.parameters[0], pt.parameters[1]]);
            }
        }
        OpIn::Span {
            curve,
            lt,
            lp,
            ld,
            ct,
            cp,
            cd,
            tol,
            match_param,
        } => {
            let c = build(curve)?;
            let r = tri!(
                o,
                test_span_accuracy(&k, 
                    &c,
                    *lt,
                    [v3(*lp), v3(*ld)],
                    *ct,
                    [v3(*cp), v3(*cd)],
                    *tol,
                    *match_param,
                )
            );
            o.extend([0.0, b(r.passes), r.max_dist]);
        }
        OpIn::Refine { c1, c2, sols } => {
            let a = build(c1)?;
            let bb = build(c2)?;
            let mut arr: SolutionArray<Obj> = SolutionArray::new();
            for s in sols {
                arr.add(&solution_of(s));
            }
            let r = refine_cc_solutions(&k, &mut arr, |ob| match ob {
                Some(Obj(0, _)) => Some(&a),
                Some(Obj(1, _)) => Some(&bb),
                _ => None,
            });
            let st = match st_of(&r) {
                Ok(s) => s,
                Err(d) => return Ok(*d),
            };
            o.extend([st, arr.len() as f64]);
            for s in arr.as_slice() {
                o.extend([
                    s.start.solution_value,
                    s.start.parameters[0],
                    s.start.parameters[1],
                    s.end.solution_value,
                    s.end.parameters[0],
                    s.end.parameters[1],
                ]);
            }
        }
    }
    Ok(to_output(&o))
}

// ---- generated cases --------------------------------------------------------------

fn rp(rng: &mut Rng, s: f64) -> [f64; 3] {
    rng.point3(-s, s)
}

/// A random clamped B-spline (sometimes rational, 2D or with repeated interior knots).
fn gen_bspline(rng: &mut Rng) -> BSplineIn {
    let p = 1 + rng.below(4) as usize;
    let n = p + rng.below(6) as usize;
    let rational = rng.chance(0.25);
    let two_d = rng.chance(0.15);
    let a = if rng.chance(0.5) {
        0.0
    } else {
        rng.uniform(-5.0, 5.0)
    };
    let len = if rng.chance(0.5) {
        1.0
    } else {
        rng.uniform(0.1, 10.0)
    };
    let mut u = vec![a; p + 1];
    let inner = n - p;
    let mut ks: Vec<f64> = (0..inner)
        .map(|_| a + len * rng.uniform(0.05, 0.95))
        .collect();
    ks.sort_by(f64::total_cmp);
    if inner >= 2 && p >= 2 && rng.chance(0.3) {
        if let Some(&v) = ks.first() {
            if let Some(x) = ks.get_mut(1) {
                *x = v;
            }
        }
    }
    u.extend(ks);
    u.extend(vec![a + len; p + 1]);
    let scale = if rng.chance(0.5) { 5.0 } else { 100.0 };
    let cpts = (0..=n)
        .map(|i| {
            let [x, y, z] = rp(rng, scale);
            let x = x + i as f64 * scale * 0.2;
            let w = if rational { rng.uniform(0.3, 3.0) } else { NOW };
            let z = if two_d { NOZ } else { z };
            if rational {
                [x * w, y * w, if two_d { z } else { z * w }, w]
            } else {
                [x, y, z, w]
            }
        })
        .collect();
    let dim = if two_d || rng.chance(0.1) { 2 } else { 3 };
    BSplineIn {
        dim,
        oob: rng.chance(0.1),
        cpts,
        p,
        u,
    }
}

fn gen_hermite(rng: &mut Rng) -> HermiteIn {
    let s = if rng.chance(0.5) { 3.0 } else { 50.0 };
    let mut d1 = rp(rng, s);
    if rng.chance(0.1) {
        d1 = [0.0; 3];
    }
    let p1 = rp(rng, s);
    let mut p2 = rp(rng, s);
    if rng.chance(0.1) {
        p2 = p1;
    }
    let mut h = HermiteIn {
        dim: if rng.chance(0.2) { 2 } else { 3 },
        p1,
        d1,
        p2,
        d2: if rng.chance(0.08) { [0.0; 3] } else { rp(rng, s) },
    };
    if h.dim == 2 {
        for v in [&mut h.p1, &mut h.d1, &mut h.p2, &mut h.d2] {
            v[2] = 0.0;
        }
    }
    h
}

fn gen_line(rng: &mut Rng) -> LineIn {
    let d0 = rng.uniform(-5.0, 5.0);
    LineIn {
        p: rp(rng, 10.0),
        v: rp(rng, 3.0),
        domain: [d0, d0 + rng.uniform(0.5, 10.0)],
        scale: *rng.choose(&[1.0, 1.0, 2.0, 0.5]).unwrap_or(&1.0),
        inside_out: rng.chance(0.3),
    }
}

fn gen_circle(rng: &mut Rng) -> CurveIn {
    let a0 = rng.uniform(-90.0, 90.0);
    CurveIn::Circle(CircleIn {
        center: rp(rng, 5.0),
        x: [1.0, 0.0, 0.0],
        y: [0.0, rng.uniform(0.5, 1.0), rng.uniform(-0.5, 0.5)],
        radius: rng.uniform(0.5, 10.0),
        a0,
        a1: a0 + rng.uniform(20.0, 360.0),
    })
}

fn gen_curve(rng: &mut Rng) -> CurveIn {
    let c = match rng.below(20) {
        0..=1 => gen_circle(rng),
        2..=4 => CurveIn::Line(gen_line(rng)),
        5..=12 => CurveIn::Bspline(gen_bspline(rng)),
        _ => CurveIn::Hermite(gen_hermite(rng)),
    };
    resolve(&c).unwrap_or(c)
}

/// A curve that is closed (or nearly) over its natural interval.
fn gen_closed_curve(rng: &mut Rng) -> CurveIn {
    match rng.below(3) {
        0 => {
            let a0 = rng.uniform(-90.0, 90.0);
            let c = CurveIn::Circle(CircleIn {
                center: rp(rng, 5.0),
                x: [1.0, 0.0, 0.0],
                y: [0.0, 1.0, 0.0],
                radius: rng.uniform(0.5, 10.0),
                a0,
                a1: a0 + 360.0,
            });
            resolve(&c).unwrap_or(c)
        }
        1 => {
            let mut h = gen_hermite(rng);
            h.p2 = h.p1;
            if rng.chance(0.3) {
                h.p2[0] += *rng.choose(&[1e-13, 1e-9, 1e-4]).unwrap_or(&1e-9);
            }
            CurveIn::Hermite(h)
        }
        _ => {
            let mut bs = gen_bspline(rng);
            if let Some(f) = bs.cpts.first().copied() {
                if let Some(l) = bs.cpts.last_mut() {
                    *l = f;
                }
            }
            CurveIn::Bspline(bs)
        }
    }
}

fn range_of(c: &CurveIn) -> (f64, f64) {
    let resolved = resolve(c).ok();
    match resolved.as_ref().unwrap_or(c) {
        CurveIn::Circle(ci) => (ci.a0, ci.a1),
        CurveIn::Bspline(bs) => (
            bs.u.first().copied().unwrap_or(0.0),
            bs.u.last().copied().unwrap_or(1.0),
        ),
        CurveIn::Hermite(_) => (0.0, 1.0),
        CurveIn::Line(l) => {
            let nat = build_line(l).natural_interval();
            (nat.min(), nat.max())
        }
    }
}

/// A parameter of `c`: interior, at a knot, at an end, or slightly outside.
fn gen_t(rng: &mut Rng, c: &CurveIn) -> f64 {
    let (a, bb) = range_of(c);
    match rng.below(10) {
        0 => a,
        1 => bb,
        2 => {
            if let CurveIn::Bspline(bs) = c {
                *rng.choose(&bs.u).unwrap_or(&a)
            } else {
                0.5 * (a + bb)
            }
        }
        3 => a + (bb - a) * rng.uniform(-0.3, 1.3),
        _ => a + (bb - a) * rng.unit(),
    }
}

fn gen_ivl(rng: &mut Rng, c: &CurveIn) -> [f64; 2] {
    let (a, bb) = range_of(c);
    if rng.chance(0.4) {
        return [a, bb];
    }
    let x = a + (bb - a) * rng.uniform(-0.2, 1.2);
    let y = if rng.chance(0.05) {
        x
    } else {
        a + (bb - a) * rng.uniform(-0.2, 1.2)
    };
    [x.min(y), x.max(y)]
}

/// A unit vector-ish direction.
fn gen_dir(rng: &mut Rng) -> [f64; 3] {
    loop {
        let v = rp(rng, 1.0);
        if v.iter().map(|c| c * c).sum::<f64>() > 0.05 {
            return v;
        }
    }
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale3(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}


fn gen_curve_or_degenerate(rng: &mut Rng) -> CurveIn {
    if rng.chance(0.08) {
        let p = rp(rng, 3.0);
        return CurveIn::Hermite(HermiteIn {
            dim: 3,
            p1: p,
            d1: [0.0; 3],
            p2: p,
            d2: [0.0; 3],
        });
    }
    gen_curve(rng)
}

/// A point near the curve (or anywhere).
fn gen_point_near(k: &Kernel, rng: &mut Rng, c: &CurveIn) -> [f64; 3] {
    let Ok(curve) = build(c) else {
        return rp(rng, 10.0);
    };
    let t = gen_t(rng, c);
    let base = curve.evaluate_point(k, t).map_or([0.0; 3], |p| [p.x, p.y, p.z]);
    match rng.below(5) {
        0 => base,
        1 => rp(rng, 20.0),
        _ => {
            let r = rng.uniform(0.01, 5.0);
            add(base, rp(rng, r))
        }
    }
}


fn gen_straight(rng: &mut Rng) -> CurveIn {
    let p = rp(rng, 5.0);
    let d = gen_dir(rng);
    let n = 2 + rng.below(4) as usize;
    let back = rng.chance(0.2);
    let cpts = (0..n)
        .map(|i| {
            let mut f = i as f64 / (n - 1) as f64;
            if back && i == n - 1 {
                f = 0.0;
            }
            let q = add(p, scale3(d, 3.0 * f));
            let jit = if rng.chance(0.2) { 1e-4 } else { 0.0 };
            [q[0] + jit, q[1], q[2], NOW]
        })
        .collect::<Vec<_>>();
    let deg = 1 + rng.below((n - 1).min(3) as u64) as usize;
    let mut u = vec![0.0; deg + 1];
    let inner = n - 1 - deg;
    for i in 0..inner {
        u.push((i + 1) as f64 / (inner + 1) as f64);
    }
    u.extend(vec![1.0; deg + 1]);
    CurveIn::Bspline(BSplineIn {
        dim: 3,
        oob: false,
        cpts,
        p: deg,
        u,
    })
}


/// Operations of `GlobalPointSolve` with the option vectors each needs.
const GPS_OPS: [(i32, usize); 13] = [
    (0, 0),
    (3, 0),
    (4, 0),
    (5, 0),
    (6, 0),
    (8, 1),
    (9, 1),
    (11, 2),
    (12, 2),
    (13, 2),
    (14, 1),
    (15, 1),
    (24, 1),
];

/// A curve for the global solvers and tessellations (no degenerate B-splines).
fn gen_any(rng: &mut Rng) -> CurveIn {
    match rng.below(10) {
        0 => gen_closed_curve(rng),
        1 => gen_straight(rng),
        2 => gen_curve_or_degenerate(rng),
        _ => gen_curve(rng),
    }
}

fn gen_gps(k: &Kernel, rng: &mut Rng) -> OpIn {
    let curve = gen_any(rng);
    let &(sop, need) = rng.choose(&GPS_OPS).unwrap_or(&(3, 0));
    // weight the operations the port runs without deferral
    let (sop, need) = if rng.chance(0.6) {
        *rng.choose(&[(0, 0), (3, 0), (4, 0), (5, 0), (6, 0), (8, 1), (9, 1)])
            .unwrap_or(&(5, 0))
    } else {
        (sop, need)
    };
    let p = if rng.chance(0.1) {
        // a closed curve's centre-ish point: many normals
        let (a, bb) = range_of(&curve);
        build(&curve)
            .ok()
            .and_then(|c| {
                let p0 = c.evaluate_point(k, a).ok()?;
                let p1 = c.evaluate_point(k, 0.5 * (a + bb)).ok()?;
                Some([(p0.x + p1.x) / 2.0, (p0.y + p1.y) / 2.0, (p0.z + p1.z) / 2.0])
            })
            .unwrap_or([0.0; 3])
    } else {
        gen_point_near(k, rng, &curve)
    };
    let ivl = if rng.chance(0.6) {
        let (a, bb) = range_of(&curve);
        [a, bb]
    } else {
        let mut i = gen_ivl(rng, &curve);
        if i[0] >= i[1] {
            i[1] = i[0] + 0.1;
        }
        i
    };
    let target = match sop {
        6 => Some(rng.uniform(0.05, 5.0)),
        3 | 4 if rng.chance(0.3) => Some(rng.uniform(-2.0, 20.0)),
        _ => None,
    };
    let vecs = (0..need.max(usize::from(rng.chance(0.2))))
        .map(|_| gen_dir(rng))
        .collect();
    OpIn::Gps {
        curve,
        xf: Vec::new(),
        ivl,
        sop,
        p,
        pts: Vec::new(),
        tol: *rng.choose(&[1e-5, 1e-6, 1e-3, 0.05]).unwrap_or(&1e-5),
        target,
        vecs,
        req: i32::from(rng.chance(0.6)),
    }
}

fn gen_breaks(rng: &mut Rng, c: &CurveIn) -> Vec<f64> {
    let (a, bb) = range_of(c);
    let n = 2 + rng.below(3) as usize;
    let mut v: Vec<f64> = (0..n).map(|_| a + (bb - a) * rng.unit()).collect();
    if rng.chance(0.7) {
        if let Some(f) = v.first_mut() {
            *f = a;
        }
        if let Some(l) = v.last_mut() {
            *l = bb;
        }
    }
    v.sort_by(f64::total_cmp);
    v
}

fn gen_approx(rng: &mut Rng) -> OpIn {
    let curve = gen_any(rng);
    let breaks = gen_breaks(rng, &curve);
    OpIn::Approx {
        curve,
        breaks,
        tol: *rng.choose(&[1e-3, 1e-2, 0.1, 1e-5]).unwrap_or(&1e-3),
        analytics: rng.chance(0.6),
        match_param: rng.chance(0.4),
        copy: rng.chance(0.15),
    }
}

fn gen_tess_tols(rng: &mut Rng) -> (f64, f64) {
    let chord = *rng.choose(&[0.0, 0.01, 0.001, 0.05]).unwrap_or(&0.01);
    let angle = if chord == 0.0 || rng.chance(0.5) {
        *rng.choose(&[5.0, 10.0, 15.0, 30.0]).unwrap_or(&10.0)
    } else {
        0.0
    };
    (chord, angle)
}

fn gen_tess_ivl(rng: &mut Rng, c: &CurveIn) -> [f64; 2] {
    let (a, bb) = range_of(c);
    if rng.chance(0.6) {
        return [a, bb];
    }
    let x = a + (bb - a) * rng.unit();
    let y = a + (bb - a) * rng.unit();
    if (x - y).abs() < 1e-3 * (bb - a) {
        return [a, bb];
    }
    [x.min(y), x.max(y)]
}

fn gen_bisect(rng: &mut Rng) -> OpIn {
    let curve = gen_any(rng);
    let ivl = gen_tess_ivl(rng, &curve);
    let (chord, angle) = gen_tess_tols(rng);
    let max3d = if rng.chance(0.3) {
        rng.uniform(0.5, 20.0)
    } else {
        0.0
    };
    OpIn::Bisect {
        curve,
        ivl,
        chord,
        angle,
        max3d,
    }
}

fn gen_tess(rng: &mut Rng) -> OpIn {
    let curve = gen_any(rng);
    let ivl = gen_tess_ivl(rng, &curve);
    let (chord, angle) = gen_tess_tols(rng);
    OpIn::Tess {
        curve,
        ivl,
        chord,
        angle,
        minseg: *rng.choose(&[0, 1, 2, 5, 16]).unwrap_or(&0),
    }
}

fn gen_sol(rng: &mut Rng, c1: &CurveIn, c2: &CurveIn) -> SolIn {
    SolIn {
        range: rng.chance(0.8),
        nvar: if rng.chance(0.9) { 2 } else { 1 },
        o: if rng.chance(0.9) {
            [1, 2]
        } else {
            [rng.below(3), rng.below(3)]
        },
        v0: *rng.choose(&[0.0, 1e-7, 1e-4, 0.01]).unwrap_or(&0.0),
        s: [gen_t(rng, c1), gen_t(rng, c2)],
        v1: *rng.choose(&[0.0, 1e-7, 1e-4, 0.01]).unwrap_or(&0.0),
        e: [gen_t(rng, c1), gen_t(rng, c2)],
    }
}

fn gen_classify(rng: &mut Rng) -> OpIn {
    let c1 = gen_curve(rng);
    let c2 = if rng.chance(0.3) { c1.clone() } else { gen_curve(rng) };
    let mut sol = gen_sol(rng, &c1, &c2);
    sol.o = [1, 2];
    sol.nvar = 2;
    OpIn::Classify { c1, c2, sol }
}

fn gen_span(k: &Kernel, rng: &mut Rng) -> OpIn {
    let curve = gen_any(rng);
    let (a, bb) = range_of(&curve);
    let mut lt = a + (bb - a) * rng.unit();
    let mut ct = a + (bb - a) * rng.unit();
    if lt > ct {
        std::mem::swap(&mut lt, &mut ct);
    }
    if rng.chance(0.1) {
        ct = lt + (bb - a) * 1e-5;
    }
    let ev = |t: f64| -> ([f64; 3], [f64; 3]) {
        build(&curve)
            .ok()
            .and_then(|c| c.evaluate(k, t, 1, true, true).ok())
            .map_or(([0.0; 3], [1.0, 0.0, 0.0]), |v| {
                let p = v.first().copied().unwrap_or(Vec3::ZERO);
                let d = v.get(1).copied().unwrap_or(Vec3::ZERO);
                ([p.x, p.y, p.z], [d.x, d.y, d.z])
            })
    };
    let (lp, ld) = ev(lt);
    let (cp, cd) = ev(ct);
    OpIn::Span {
        curve,
        lt,
        lp,
        ld,
        ct,
        cp,
        cd,
        tol: *rng.choose(&[1e-3, 1e-2, 1e-6, 0.5]).unwrap_or(&1e-3),
        match_param: rng.chance(0.5),
    }
}

fn gen_refine(rng: &mut Rng) -> OpIn {
    let c1 = gen_curve(rng);
    let c2 = gen_curve(rng);
    let n = rng.below(4) as usize;
    let sols = (0..n).map(|_| gen_sol(rng, &c1, &c2)).collect();
    OpIn::Refine { c1, c2, sols }
}

fn gen_case(k: &Kernel, rng: &mut Rng) -> OpIn {
    match rng.below(100) {
        0..=39 => gen_gps(k, rng),
        40..=54 => gen_approx(rng),
        55..=69 => gen_bisect(rng),
        70..=79 => gen_tess(rng),
        80..=86 => gen_classify(rng),
        87..=95 => gen_span(k, rng),
        _ => gen_refine(rng),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.curve_p1"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let k = &Kernel::default();
        for _ in 0..64 {
            let op = gen_case(k, rng);
            let Ok((code, input)) = cpp_input(&op) else {
                continue;
            };
            if ffi::validate(code, &input).is_err() {
                continue;
            }
            return serde_json::to_value(op).ok();
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
        let op = case.parse()?;
        validate(&op)?;
        rust(&op)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn approximation_of_a_break_at_1e300_fails_softly() {
        // fuzz01255 (`tests/fuzz_corpus/curve.curve_p1.json`): the FFI refuses the
        // `1e300` break, so the runner never hands it to the port; run directly, the
        // port's `approximate_curve` used to abort on a 6 GiB allocation and now stops
        // at `MAX_APPROX_SPANS` with an error status.
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fuzz_corpus/curve.curve_p1.json"
        ))
        .unwrap();
        let case = corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "fuzz01255_approx_break_1e300")
            .unwrap();
        let op: OpIn = serde_json::from_value(case["inputs"].clone()).unwrap();
        let out = rust(&op).unwrap();
        assert_ne!(out.ints["status"], vec![0], "{out:?}");
    }

    /// Generated cases cover every op, with passing (not deferred) cases.
    #[test]
    fn generated_cases_are_diverse() {
        let k = &Kernel::default();
        let mut rng = Rng::new(7);
        let mut stats: BTreeMap<String, [usize; 4]> = BTreeMap::new();
        for _ in 0..600 {
            let op = gen_case(k, &mut rng);
            let name = serde_json::to_value(&op).unwrap()["op"].as_str().unwrap().to_string();
            let out = rust(&op);
            let e = stats.entry(name).or_default();
            e[0] += 1;
            match out {
                Err(_) => e[3] += 1,
                Ok(o) if o.deferred.is_some() => e[3] += 1,
                Ok(o) if o.ints.get("status").is_some_and(|s| s == &vec![0]) => e[1] += 1,
                Ok(_) => e[2] += 1,
            }
        }
        for (k, v) in &stats {
            eprintln!("{k}: total {} ok {} err {} deferred/harness {}", v[0], v[1], v[2], v[3]);
        }
        assert_eq!(stats.len(), 7);
    }
}
